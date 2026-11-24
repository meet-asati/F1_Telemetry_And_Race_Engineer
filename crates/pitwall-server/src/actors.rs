//! The daemon's long-running tasks:
//! * state actor — single writer of `RaceState`; publishes 15 Hz snapshots
//! * db writer   — persists sessions/laps/radio/outlines off the hot path
//! * comms       — rule engine + arbitrator; decides what to say
//! * audio       — TTS (cached) → radio FX → local playback
//! * voice       — driver push-to-talk → STT → answers (see `voice.rs`)

use crate::ingest::IngestStats;
use crate::voice::{VoiceCmd, VoiceStatus};
use ai_sidecar::TtsClient;
use axum::extract::ws::Utf8Bytes;
use comms::{ArbConfig, Arbitrator, GateCtx, RuleConfig, RuleEngine, Utterance};
use f1_udp::ParsedPacket;
use race_state::db::{Db, RadioRow};
use race_state::snapshot::*;
use race_state::{LapRow, RaceState, SessionRow, StateEvent, Thresholds};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, mpsc, watch};
use voice_comms::{AudioPlayer, RadioFx, TtsCache};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub const SNAPSHOT_HZ: u64 = 15;

/// Fan-out channels shared by the web layer.
#[derive(Clone)]
pub struct Hub {
    /// Latest serialized `ServerMessage::Snapshot`.
    pub snapshot_json: watch::Receiver<Utf8Bytes>,
    pub snapshot: watch::Receiver<Arc<TelemetrySnapshot>>,
    /// Serialized `ServerMessage::Event`s.
    pub events: broadcast::Sender<Utf8Bytes>,
    pub current_session: watch::Receiver<Option<i64>>,
    pub health: watch::Receiver<HealthInfo>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthInfo {
    pub source: Option<DataSource>,
    pub datagrams: u64,
    pub decode_errors: u64,
    pub dropped: u64,
    pub last_packet_age_ms: Option<u64>,
    pub game_year: Option<u8>,
    pub tts_engine: String,
    pub tts_available: bool,
    pub audio_device: Option<String>,
    pub mic_device: Option<String>,
    pub db_enabled: bool,
}

pub fn event_json(ev: &PitWallEvent) -> Utf8Bytes {
    let msg = ServerMessage::Event { event: ev.clone() };
    Utf8Bytes::from(serde_json::to_string(&msg).unwrap_or_default())
}

pub enum DbCmd {
    Session(SessionRow),
    Lap(LapRow),
    Flashback(u8),
    Radio(RadioRow),
    Outline { track_id: i8, length_m: f32, points: Vec<[f32; 2]> },
}

pub enum StateCtl {
    OutlineAvailable(bool),
}

pub enum CommsCmd {
    Client(ClientCommand),
    RaceEvent(PitWallEvent),
    /// Wheel-button state (push-to-talk).
    Buttons(u32),
    SpeakingUntil(u64),
    TtsAvailable(bool),
    /// The driver is transmitting.
    PttActive(bool),
    /// What the driver said (transcript or typed question).
    DriverSaid(String),
    /// The engineer's reply to the driver (P1).
    DriverAnswer { text: String, source: RadioSource },
    Thinking(bool),
    VoiceStatus(VoiceStatus),
    MicLevel(f32),
    /// The human engineer is talking live (duo mode).
    EngineerTx(bool),
    /// Race over: the engineer's spoken wrap-up.
    Debrief(Box<race_state::debrief::Debrief>),
    MicBlocked(bool),
    MicDevice(Option<String>),
    /// A wheel "UDP Action N" button was pressed (diagnostics).
    UdpAction(u8),
}

// ------------------------------------------------------------ state actor

pub struct StateActor {
    pub thresholds: Thresholds,
    pub source: DataSource,
    pub packets: mpsc::Receiver<ParsedPacket>,
    pub ctl: mpsc::Receiver<StateCtl>,
    pub snapshot_json: watch::Sender<Utf8Bytes>,
    pub snapshot: watch::Sender<Arc<TelemetrySnapshot>>,
    pub comms_state: watch::Receiver<CommsState>,
    pub events: broadcast::Sender<Utf8Bytes>,
    pub db: mpsc::UnboundedSender<DbCmd>,
    pub comms: mpsc::Sender<CommsCmd>,
    pub health: watch::Sender<HealthInfo>,
    pub stats: Arc<IngestStats>,
}

impl StateActor {
    pub async fn run(mut self) {
        let mut state = RaceState::new(self.thresholds.clone());
        state.set_source(self.source);
        let mut tick = tokio::time::interval(Duration::from_millis(1000 / SNAPSHOT_HZ));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut seq: u64 = 0;
        let mut have_packets = false;
        loop {
            tokio::select! {
                p = self.packets.recv() => {
                    let Some(p) = p else { break };
                    have_packets = true;
                    for ev in state.ingest(&p, now_ms()) {
                        if matches!(ev, StateEvent::RaceFinished) {
                            self.debrief(&state, now_ms());
                        }
                        self.handle(ev);
                    }
                }
                Some(c) = self.ctl.recv() => match c {
                    StateCtl::OutlineAvailable(v) => state.set_outline_available(v),
                },
                _ = tick.tick() => {
                    let now = now_ms();
                    let mut snap = state.snapshot(now);
                    snap.last_packet_age_ms = have_packets.then(|| now.saturating_sub(state.last_packet_wall_ms()));
                    if !have_packets {
                        snap.source = DataSource::None;
                    }
                    snap.comms = self.comms_state.borrow().clone();
                    seq += 1;
                    let snap = Arc::new(snap);
                    let msg = serde_json::json!({ "type": "snapshot", "seq": seq, "data": &*snap });
                    let _ = self.snapshot_json.send(Utf8Bytes::from(msg.to_string()));
                    let _ = self.snapshot.send(snap.clone());
                    if seq.is_multiple_of(SNAPSHOT_HZ) {
                        self.health.send_modify(|h| {
                            h.source = Some(snap.source);
                            h.datagrams = self.stats.datagrams.load(Ordering::Relaxed);
                            h.decode_errors = self.stats.decode_errors.load(Ordering::Relaxed);
                            h.dropped = self.stats.dropped.load(Ordering::Relaxed);
                            h.last_packet_age_ms = snap.last_packet_age_ms;
                            h.game_year = snap.game_year;
                        });
                    }
                }
            }
        }
    }

    /// Build the player's debrief from the race just finished and hand it to comms.
    fn debrief(&self, state: &RaceState, now: u64) {
        let laps: Vec<LapRow> = (0..22).flat_map(|c| state.laps(c).to_vec()).collect();
        let snap = state.snapshot(now);
        let player = state.player_index() as u8;
        match race_state::debrief::build(&laps, player, snap.session.is_race, snap.session.total_laps) {
            Some(d) => {
                tracing::info!("debrief: {}", d.insights.iter().map(|i| i.text.as_str()).collect::<Vec<_>>().join(" "));
                let _ = self.comms.try_send(CommsCmd::Debrief(Box::new(d)));
            }
            None => tracing::info!("race finished, but no laps for a debrief"),
        }
    }

    fn handle(&self, ev: StateEvent) {
        match ev {
            StateEvent::Wire(e) => {
                let _ = self.events.send(event_json(&e));
                if matches!(e, PitWallEvent::Flag { .. } | PitWallEvent::Penalty { .. }) {
                    let _ = self.comms.try_send(CommsCmd::RaceEvent(e));
                }
            }
            StateEvent::SessionStarted(row) => {
                tracing::info!("session: {} {} ({} laps) F1 {}", row.track_name, row.session_type, row.total_laps, row.game_year);
                let _ = self.db.send(DbCmd::Session(row));
            }
            StateEvent::LapCompleted(row) => {
                let _ = self.db.send(DbCmd::Lap(row));
            }
            StateEvent::Flashback { to_lap } => {
                tracing::info!("flashback to lap {to_lap}");
                let _ = self.db.send(DbCmd::Flashback(to_lap));
            }
            StateEvent::Buttons(b) => {
                let _ = self.comms.try_send(CommsCmd::Buttons(b));
            }
            StateEvent::TrackOutline { track_id, length_m, points } => {
                tracing::info!("track outline captured ({} points)", points.len());
                let _ = self.db.send(DbCmd::Outline { track_id, length_m, points });
            }
            StateEvent::LightsOut => tracing::info!("lights out"),
            StateEvent::RaceFinished => tracing::info!("race finished"),
        }
    }
}

// --------------------------------------------------------------- db writer

pub async fn run_db(db: Option<Db>, mut rx: mpsc::UnboundedReceiver<DbCmd>, ctl: mpsc::Sender<StateCtl>, current: watch::Sender<Option<i64>>) {
    let mut session_id: Option<i64> = None;
    while let Some(cmd) = rx.recv().await {
        let Some(db) = &db else { continue };
        let res: anyhow::Result<()> = async {
            match cmd {
                DbCmd::Session(row) => {
                    let id = db.create_session(&row).await?;
                    session_id = Some(id);
                    let _ = current.send(Some(id));
                    let known = db.outline(row.track_id as i64).await?.is_some();
                    let _ = ctl.send(StateCtl::OutlineAvailable(known)).await;
                }
                DbCmd::Lap(row) => {
                    if let Some(id) = session_id {
                        db.insert_lap(id, &row).await?;
                    }
                }
                DbCmd::Flashback(lap) => {
                    if let Some(id) = session_id {
                        db.delete_laps_from(id, lap).await?;
                    }
                }
                DbCmd::Radio(row) => db.insert_radio(session_id, &row).await?,
                DbCmd::Outline { track_id, length_m, points } => db.upsert_outline(track_id, length_m, &points).await?,
            }
            Ok(())
        }
        .await;
        if let Err(e) = res {
            tracing::warn!("db write failed: {e:#}");
        }
    }
}

// ------------------------------------------------------------------ comms

pub struct CommsActor {
    pub cfg: ArbConfig,
    pub rules: RuleConfig,
    pub voice: mpsc::Sender<VoiceCmd>,
    /// Configured "UDP Action N" for push-to-talk (shown in the UI).
    pub ptt_button: u8,
    /// Browsers on the team-radio channel.
    pub radio_listeners: Arc<std::sync::atomic::AtomicUsize>,
    pub initial_mode: CommsMode,
    pub rx: mpsc::Receiver<CommsCmd>,
    pub snapshot: watch::Receiver<Arc<TelemetrySnapshot>>,
    pub state_out: watch::Sender<CommsState>,
    pub events: broadcast::Sender<Utf8Bytes>,
    pub audio: mpsc::Sender<Utterance>,
    pub db: mpsc::UnboundedSender<DbCmd>,
}

fn radio_row(e: &PitWallEvent) -> Option<RadioRow> {
    match e {
        PitWallEvent::Radio { at_ms, source, priority, text, outcome, attachments } => Some(RadioRow {
            at_ms: *at_ms as i64,
            priority: format!("{priority:?}"),
            source: serde_json::to_value(source).ok()?.as_str()?.to_string(),
            text: text.clone(),
            context: (!attachments.is_empty()).then(|| attachments.join("; ")),
            outcome: serde_json::to_value(outcome).ok()?.as_str()?.to_string(),
        }),
        _ => None,
    }
}

pub fn publish(events: &broadcast::Sender<Utf8Bytes>, db: &mpsc::UnboundedSender<DbCmd>, e: &PitWallEvent) {
    let _ = events.send(event_json(e));
    if let Some(row) = radio_row(e) {
        let _ = db.send(DbCmd::Radio(row));
    }
}

impl CommsActor {
    pub async fn run(mut self) {
        let mut rules = RuleEngine::new(self.rules.clone());
        let mut voice_status = VoiceStatus::default();
        let mut thinking = false;
        let mut last_driver_line: Option<String> = None;
        let mut mic_level = 0.0f32;
        let mut mic_blocked = false;
        let mut engineer_tx = false;
        let mut mic_device: Option<String> = None;
        let mut last_udp_action: Option<u8> = None;
        let mut arb = Arbitrator::new(self.cfg.clone());
        arb.set_mode(self.initial_mode);
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_rules = 0u64;
        loop {
            tokio::select! {
                cmd = self.rx.recv() => {
                    let Some(cmd) = cmd else { break };
                    let now = now_ms();
                    match cmd {
                        CommsCmd::Client(c) => match c {
                            ClientCommand::RadioSend { text, attachments, .. } => {
                                let text = text.trim().to_string();
                                if !text.is_empty() {
                                    arb.friend_radio(text, attachments, now);
                                }
                            }
                            ClientCommand::SuggestionApprove { id, .. } => { arb.approve(&id, None, now); }
                            ClientCommand::SuggestionEdit { id, text, .. } => { arb.approve(&id, Some(text), now); }
                            ClientCommand::SuggestionDismiss { id, .. } => { arb.dismiss(&id, now); }
                            ClientCommand::CommsSetMode { mode, .. } => arb.set_mode(mode),
                            ClientCommand::CommsMute { muted, .. } => arb.set_muted(muted),
                            ClientCommand::PttSet { active, .. } => { let _ = self.voice.try_send(VoiceCmd::Ptt(active)); }
                            ClientCommand::DriverAsk { text, .. } => {
                                let text = text.trim().to_string();
                                if !text.is_empty() {
                                    let _ = self.voice.try_send(VoiceCmd::Ask(text));
                                }
                            }
                            ClientCommand::VoiceSetMic { device, .. } => { let _ = self.voice.try_send(VoiceCmd::SetMic(device)); }
                            ClientCommand::SelectCar { .. } => {}
                        },
                        CommsCmd::RaceEvent(e) => {
                            let snap = self.snapshot.borrow().clone();
                            for a in rules.on_event(&e, &snap) {
                                arb.submit(a, now);
                            }
                        }
                        CommsCmd::Buttons(b) => { let _ = self.voice.try_send(VoiceCmd::Buttons(b)); }
                        CommsCmd::SpeakingUntil(t) => arb.set_speaking_until(t),
                        CommsCmd::TtsAvailable(v) => arb.set_tts_available(v),
                        CommsCmd::PttActive(v) => arb.set_ptt_active(v),
                        CommsCmd::DriverSaid(text) => {
                            last_driver_line = Some(text.clone());
                            let ev = PitWallEvent::Radio { at_ms: now, source: RadioSource::Driver, priority: Priority::P1, text, outcome: RadioOutcome::Spoken, attachments: vec![] };
                            publish(&self.events, &self.db, &ev);
                        }
                        CommsCmd::DriverAnswer { text, source } => { arb.driver_answer(text, source, now); }
                        CommsCmd::Thinking(v) => thinking = v,
                        CommsCmd::VoiceStatus(v) => voice_status = v,
                        CommsCmd::MicLevel(v) => mic_level = v,
                        CommsCmd::EngineerTx(on) => {
                            engineer_tx = on;
                            arb.set_engineer_tx(on, now);
                        }
                        CommsCmd::Debrief(d) => {
                            if let Some(text) = rules.debrief_lines(&d) {
                                arb.driver_answer(text, RadioSource::Rule, now);
                            }
                        }
                        CommsCmd::MicBlocked(v) => mic_blocked = v,
                        CommsCmd::MicDevice(d) => mic_device = d,
                        CommsCmd::UdpAction(n) => last_udp_action = Some(n),
                    }
                }
                _ = tick.tick() => {
                    let now = now_ms();
                    let snap = self.snapshot.borrow().clone();
                    let fresh = snap.last_packet_age_ms.map(|a| a < 2000).unwrap_or(false);
                    if fresh && now.saturating_sub(last_rules) >= 500 {
                        last_rules = now;
                        for a in rules.evaluate(&snap) {
                            arb.submit(a, now);
                        }
                    }
                    let player = snap.track.cars.iter().find(|c| c.is_player);
                    let ctx = GateCtx {
                        throttle: player.map(|c| c.throttle_pct as f32 / 100.0).unwrap_or(0.0),
                        steer: player.map(|c| c.steer).unwrap_or(0.0),
                        in_pit: player.map(|c| c.in_pit).unwrap_or(false),
                        // Paused game or lost feed: hold routine (P2) calls; P0/P1 still play.
                        phase: if fresh && !snap.session.paused { snap.session.phase.clone() } else { "paused".into() },
                    };
                    if let Some(u) = arb.tick(now, &ctx) {
                        if self.audio.try_send(u.clone()).is_err() {
                            tracing::warn!("audio queue full, dropping line: {}", u.text);
                        }
                    }
                    for e in arb.drain_events() {
                        publish(&self.events, &self.db, &e);
                    }
                    let mut st = arb.state(now);
                    st.stt_available = voice_status.stt;
                    st.llm_available = voice_status.llm;
                    st.mic_available = voice_status.mic;
                    st.thinking = thinking;
                    st.last_driver_line = last_driver_line.clone();
                    st.mic_level = (mic_level * 1000.0).round() / 1000.0;
                    st.mic_blocked = mic_blocked;
                    st.mic_device = mic_device.clone();
                    st.ptt_button = self.ptt_button;
                    st.last_udp_action = last_udp_action;
                    st.engineer_tx = engineer_tx;
                    st.radio_listeners = self.radio_listeners.load(Ordering::Relaxed) as u32;
                    self.state_out.send_if_modified(|cur| {
                        let changed = *cur != st;
                        if changed { *cur = st; }
                        changed
                    });
                }
            }
        }
    }
}

// ------------------------------------------------------------------ audio

pub struct AudioActor {
    pub rx: mpsc::Receiver<Utterance>,
    pub tts: TtsClient,
    pub cache: Option<TtsCache>,
    pub player: Option<Arc<AudioPlayer>>,
    pub comms: mpsc::Sender<CommsCmd>,
    pub events: broadcast::Sender<Utf8Bytes>,
    pub db: mpsc::UnboundedSender<DbCmd>,
    /// Radio colouring preset ([audio] radio_fx).
    pub fx: RadioFx,
    /// Fixed phrases to pre-synthesize in the background, with the priority
    /// they are spoken at (the speed is part of the cache key).
    pub warm: Vec<(String, Priority)>,
    /// Team-radio bus: what is said also goes to listening browsers.
    pub bus: crate::duo::Bus,
}

/// Urgent calls come a touch quicker, routine ones a touch calmer.
fn speed_for(p: Priority) -> f32 {
    match p {
        Priority::P0 => 1.08,
        Priority::P1 => 1.0,
        Priority::P2 => 0.97,
    }
}

/// One sentence of speech, from the disk cache or the TTS sidecar.
async fn synth_sentence(tts: &TtsClient, cache: Option<&TtsCache>, sentence: &str, speed: f32) -> anyhow::Result<(ai_sidecar::Pcm, bool)> {
    let ns = tts.cache_namespace(speed);
    if let Some(p) = cache.and_then(|c| c.get(&ns, sentence)) {
        return Ok((p, true));
    }
    let p = tts.synthesize_with(sentence, speed).await?;
    if let Some(c) = cache {
        if let Err(e) = c.put(&ns, sentence, &p) {
            tracing::debug!("tts cache write failed: {e}");
        }
    }
    Ok((p, false))
}

impl AudioActor {
    /// Pre-synthesize fixed phrases (flags, acknowledgements) into the cache
    /// in the background, so those calls play instantly later.
    fn spawn_warm_up(&self) {
        if !self.tts.enabled() || self.player.is_none() || self.warm.is_empty() {
            return;
        }
        let (tts, cache, warm) = (self.tts.clone(), self.cache.clone(), self.warm.clone());
        tokio::spawn(async move {
            let t0 = now_ms();
            let (mut made, mut hit) = (0, 0);
            for (text, prio) in warm {
                for sentence in voice_comms::split_sentences(&voice_comms::speechify(&text)) {
                    match synth_sentence(&tts, cache.as_ref(), &sentence, speed_for(prio)).await {
                        Ok((_, true)) => hit += 1,
                        Ok((_, false)) => made += 1,
                        Err(e) => {
                            tracing::info!("TTS warm-up stopped ({e:#})");
                            return;
                        }
                    }
                }
            }
            tracing::info!("TTS warm-up: {made} phrases synthesized, {hit} already cached ({} ms)", now_ms() - t0);
        });
    }

    /// Speak a line sentence by sentence: the first sentence plays while the
    /// next is still being synthesized. Stops early if the driver keys up.
    /// Returns the ms until the first audio started.
    async fn speak(&self, u: &Utterance, seed: u32) -> anyhow::Result<u64> {
        let player = self.player.as_ref().ok_or_else(|| anyhow::anyhow!("no audio device"))?;
        let t0 = now_ms();
        let sentences = voice_comms::split_sentences(&voice_comms::speechify(&u.text));
        let n = sentences.len();
        let speed = speed_for(u.priority);
        let rate = player.sample_rate();
        let mut epoch: Option<u64> = None;
        let mut first_audio_ms = 0;
        for (i, sentence) in sentences.iter().enumerate() {
            let pcm = match synth_sentence(&self.tts, self.cache.as_ref(), sentence, speed).await {
                Ok((p, _)) => p,
                Err(e) if i > 0 => {
                    tracing::warn!("tts failed mid-line ({e:#}); line cut short");
                    break;
                }
                Err(e) => return Err(e),
            };
            if epoch.is_some_and(|e| e != player.epoch()) {
                tracing::info!("line cut off by the driver keying up");
                break;
            }
            let fx = self.fx.clone();
            let bus = self.bus.clone();
            let samples = tokio::task::spawn_blocking(move || {
                let wet = fx.process_part(&pcm.samples, pcm.sample_rate, seed.wrapping_add(i as u32), i == 0, i + 1 == n);
                crate::duo::publish(&bus, crate::duo::BusSource::Engineer, &wet, pcm.sample_rate);
                voice_comms::resample(&wet, pcm.sample_rate, rate)
            })
            .await?;
            let queued_ms = player.play(&samples, u.interrupt && i == 0)?;
            if i == 0 {
                epoch = Some(player.epoch());
                first_audio_ms = now_ms() - t0;
            }
            let _ = self.comms.send(CommsCmd::SpeakingUntil(now_ms() + queued_ms)).await;
        }
        Ok(first_audio_ms)
    }

    pub async fn run(mut self) {
        self.spawn_warm_up();
        let mut seed = 1u32;
        while let Some(u) = self.rx.recv().await {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let started = now_ms();
            let outcome = if self.tts.enabled() && self.player.is_some() {
                match self.speak(&u, seed).await {
                    Ok(first_ms) => {
                        tracing::info!("radio [{:?}] ({} ms to first audio) {}", u.priority, first_ms, u.text);
                        RadioOutcome::Spoken
                    }
                    Err(e) => {
                        tracing::warn!("tts failed ({e:#}); text only: {}", u.text);
                        RadioOutcome::TextOnly
                    }
                }
            } else {
                tracing::info!("radio [{:?}] (text only) {}", u.priority, u.text);
                RadioOutcome::TextOnly
            };
            let ev = PitWallEvent::Radio { at_ms: started, source: u.source, priority: u.priority, text: u.text, outcome, attachments: u.attachments };
            publish(&self.events, &self.db, &ev);
        }
    }
}

/// Periodically probe the TTS sidecar so the UI can show its health.
pub async fn run_tts_health(tts: TtsClient, has_player: bool, comms: mpsc::Sender<CommsCmd>, health: watch::Sender<HealthInfo>) {
    let mut last: Option<bool> = None;
    let mut fails = 0u32;
    loop {
        let reachable = has_player && tts.health().await;
        fails = if reachable { 0 } else { fails + 1 };
        // A busy Kokoro (synthesizing on CPU) can miss one probe; need two in a row.
        let ok = reachable || (fails < 2 && last == Some(true));
        if last != Some(ok) {
            last = Some(ok);
            if tts.enabled() {
                tracing::info!("TTS sidecar {}", if ok { "reachable" } else { "unreachable (radio will be text-only)" });
            }
            let _ = comms.send(CommsCmd::TtsAvailable(ok)).await;
            health.send_modify(|h| h.tts_available = ok);
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}
