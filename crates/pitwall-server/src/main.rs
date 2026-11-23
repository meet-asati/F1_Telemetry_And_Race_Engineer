//! `pitwall-server` — the F1 AI race engineer daemon.
//!
//! ```text
//! pitwall-server                              # live UDP on :20777, UI on :8080
//! pitwall-server --replay captures/x.bin --replay-speed 5
//! pitwall-server --record --token secret
//! ```

mod actors;
mod config;
mod ingest;
mod voice;
mod web;

use actors::*;
use clap::Parser;
use comms::{ArbConfig, RuleConfig};
use config::{Args, Config};
use race_state::db::Db;
use race_state::snapshot::{CommsMode, CommsState, DataSource, TelemetrySnapshot};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,sqlx=warn,tower_http=warn".into())).init();
    let args = Args::parse();
    if args.list_audio_devices {
        println!("Output devices ([audio] device):");
        for d in voice_comms::list_output_devices() {
            println!("  {d}");
        }
        println!("Input devices ([voice] input_device):");
        for d in voice_comms::list_input_devices() {
            println!("  {d}");
        }
        return Ok(());
    }
    let cfg = Config::load(&args)?;

    // ---- Persistence.
    let db = if cfg.db.path.is_empty() {
        None
    } else {
        match Db::open(&cfg.db.path).await {
            Ok(db) => Some(db),
            Err(e) => {
                tracing::warn!("database disabled ({}): {e:#}", cfg.db.path);
                None
            }
        }
    };

    // ---- Channels.
    let (packet_tx, packet_rx) = mpsc::channel(8192);
    let (ctl_tx, ctl_rx) = mpsc::channel(16);
    let (db_tx, db_rx) = mpsc::unbounded_channel();
    let (comms_tx, comms_rx) = mpsc::channel(256);
    let (audio_tx, audio_rx) = mpsc::channel(32);
    let (voice_tx, voice_rx) = mpsc::channel(64);
    let (events_tx, _) = broadcast::channel(512);
    let (snapshot_json_tx, snapshot_json_rx) = watch::channel(axum::extract::ws::Utf8Bytes::from_static("{}"));
    let (snapshot_tx, snapshot_rx) = watch::channel(Arc::new(TelemetrySnapshot::default()));
    let (comms_state_tx, comms_state_rx) = watch::channel(CommsState::default());
    let (current_tx, current_rx) = watch::channel(None);
    let (health_tx, health_rx) = watch::channel(HealthInfo { db_enabled: db.is_some(), tts_engine: format!("{:?}", cfg.tts.engine).to_lowercase(), ..Default::default() });
    let stats = Arc::new(ingest::IngestStats::default());

    // ---- Ingest.
    let source = if let Some(path) = args.replay.clone() {
        let (tx, st) = (packet_tx.clone(), stats.clone());
        let speed = args.replay_speed;
        std::thread::spawn(move || {
            if let Err(e) = ingest::run_replay(path, speed, tx, st) {
                tracing::error!("replay failed: {e:#}");
            }
        });
        DataSource::Replay
    } else {
        let (c, tx, st) = (cfg.clone(), packet_tx.clone(), stats.clone());
        tokio::spawn(async move {
            if let Err(e) = ingest::run_udp(c, tx, st).await {
                tracing::error!("UDP ingest stopped: {e:#}");
            }
        });
        if args.synthetic {
            DataSource::Synthetic
        } else {
            DataSource::Live
        }
    };
    drop(packet_tx);

    // ---- State actor + DB writer.
    tokio::spawn(
        StateActor {
            thresholds: cfg.thresholds.clone(),
            source,
            packets: packet_rx,
            ctl: ctl_rx,
            snapshot_json: snapshot_json_tx,
            snapshot: snapshot_tx,
            comms_state: comms_state_rx,
            events: events_tx.clone(),
            db: db_tx.clone(),
            comms: comms_tx.clone(),
            health: health_tx.clone(),
            stats: stats.clone(),
        }
        .run(),
    );
    tokio::spawn(run_db(db.clone(), db_rx, ctl_tx, current_tx));

    // ---- Comms.
    let arb_cfg = ArbConfig {
        p2_max_age_ms: cfg.comms.p2_max_age_ms,
        override_ms: cfg.comms.override_ms,
        gate_throttle: cfg.comms.gate_throttle,
        gate_steer: cfg.comms.gate_steer,
        ..Default::default()
    };
    let initial_mode = if cfg.comms.mode.eq_ignore_ascii_case("copilot") { CommsMode::Copilot } else { CommsMode::Auto };
    tokio::spawn(
        CommsActor {
            cfg: arb_cfg,
            rules: RuleConfig { driver_name: Some(cfg.comms.driver_name.trim().to_string()).filter(|n| !n.is_empty()), ..Default::default() },
            voice: voice_tx,
            ptt_button: cfg.voice.ptt_button,
            initial_mode,
            rx: comms_rx,
            snapshot: snapshot_rx.clone(),
            state_out: comms_state_tx,
            events: events_tx.clone(),
            audio: audio_tx,
            db: db_tx.clone(),
        }
        .run(),
    );

    // ---- Audio.
    let tts = ai_sidecar::TtsClient::new(cfg.tts.clone())?;
    let player = if cfg.audio.enabled {
        let dev = (!cfg.audio.device.is_empty()).then(|| cfg.audio.device.clone());
        match tokio::task::spawn_blocking(move || voice_comms::AudioPlayer::start(dev.as_deref())).await? {
            Ok(p) => {
                tracing::info!("radio audio on '{}' @ {} Hz", p.device_name(), p.sample_rate());
                health_tx.send_modify(|h| h.audio_device = Some(p.device_name().to_string()));
                Some(Arc::new(p))
            }
            Err(e) => {
                tracing::warn!("audio output unavailable ({e:#}); radio will be text-only");
                None
            }
        }
    } else {
        None
    };
    let cache = match voice_comms::TtsCache::new(&cfg.audio.cache_dir) {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::warn!("TTS cache disabled: {e}");
            None
        }
    };
    if !tts.enabled() {
        tracing::info!("TTS engine is 'none' — radio calls appear as text on the pit wall (set [tts] in config.toml)");
    }
    tokio::spawn(run_tts_health(tts.clone(), player.is_some(), comms_tx.clone(), health_tx.clone()));

    // ---- Voice input (push-to-talk).
    // Speech-to-text: start the bundled whisper.cpp server if it isn't already running.
    let stt = ai_sidecar::SttClient::new(cfg.stt.clone())?;
    let _whisper = if cfg.voice.enabled { voice::autostart_whisper(&stt).await } else { None };

    // Microphone: config wins, then the one picked in the UI last time, then auto-pick.
    let mic = if cfg.voice.enabled {
        let dev = Some(cfg.voice.input_device.clone()).filter(|d| !d.is_empty()).or_else(voice::load_saved_mic);
        match tokio::task::spawn_blocking(move || voice_comms::MicCapture::start(dev.as_deref())).await? {
            Ok(m) => {
                tracing::info!(
                    "push-to-talk: mic '{}', wheel button 'UDP Action {}' ({:?})",
                    m.device_name(),
                    cfg.voice.ptt_button,
                    cfg.voice.ptt_mode
                );
                health_tx.send_modify(|h| h.mic_device = Some(m.device_name().to_string()));
                Some(m)
            }
            Err(e) => {
                tracing::warn!("microphone unavailable ({e:#}); typed questions still work");
                None
            }
        }
    } else {
        None
    };
    tokio::spawn(
        voice::VoiceActor {
            cfg: cfg.voice.clone(),
            rx: voice_rx,
            mic,
            stt,
            llm: ai_sidecar::LlmClient::new(cfg.llm.clone())?,
            player: player.clone(),
            snapshot: snapshot_rx.clone(),
            comms: comms_tx.clone(),
        }
        .run(),
    );
    tokio::spawn(AudioActor { rx: audio_rx, tts, cache, player, comms: comms_tx.clone(), events: events_tx.clone(), db: db_tx.clone() }.run());

    // ---- Web.
    let hub = Hub { snapshot_json: snapshot_json_rx, snapshot: snapshot_rx, events: events_tx, current_session: current_rx, health: health_rx };
    let app = web::router(web::AppState { hub, comms: comms_tx, db, token: cfg.http.token.clone() }, PathBuf::from(&cfg.http.ui_dir));
    let listener = tokio::net::TcpListener::bind(&cfg.http.bind).await.map_err(|e| anyhow::anyhow!("bind HTTP {}: {e}", cfg.http.bind))?;
    tracing::info!("pit wall on http://{}  (auth: {})", cfg.http.bind, if cfg.http.token.is_some() { "token" } else { "none" });
    axum::serve(listener, app).with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("shutting down");
    })
    .await?;
    Ok(())
}
