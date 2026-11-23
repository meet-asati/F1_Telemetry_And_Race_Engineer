//! Configuration: `config.toml` (optional) overlaid by CLI flags.

use ai_sidecar::TtsConfig;
use clap::Parser;
use race_state::Thresholds;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about = "F1 AI race engineer & pit-wall daemon")]
pub struct Args {
    /// Path to the TOML config file (default: ./config.toml if present).
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Replay a capture file instead of listening for UDP.
    #[arg(long)]
    pub replay: Option<PathBuf>,
    /// Replay speed multiplier (1, 5, 10 ...). 0 = as fast as possible.
    #[arg(long, default_value_t = 1.0)]
    pub replay_speed: f32,
    /// Record incoming UDP datagrams to the capture directory.
    #[arg(long)]
    pub record: bool,
    /// UDP bind address (overrides config).
    #[arg(long)]
    pub udp_bind: Option<String>,
    /// HTTP bind address (overrides config).
    #[arg(long)]
    pub http_bind: Option<String>,
    /// Shared token required for mutating commands (overrides config).
    #[arg(long)]
    pub token: Option<String>,
    /// Disable local audio playback (radio becomes text-only).
    #[arg(long)]
    pub no_audio: bool,
    /// Label live UDP data as coming from the synthetic generator.
    #[arg(long)]
    pub synthetic: bool,
    /// SQLite database path (overrides config). Use ":memory:" for none.
    #[arg(long)]
    pub db: Option<String>,
    /// List audio output devices and exit.
    #[arg(long)]
    pub list_audio_devices: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct Config {
    pub udp: UdpConfig,
    pub http: HttpConfig,
    pub db: DbConfig,
    pub capture: CaptureConfig,
    pub tts: TtsConfig,
    pub audio: AudioConfig,
    pub comms: CommsConfig,
    pub thresholds: Thresholds,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UdpConfig {
    pub bind: String,
    /// If non-empty, only accept datagrams from these source IPs.
    pub allow_from: Vec<String>,
}
impl Default for UdpConfig {
    fn default() -> Self {
        Self { bind: "0.0.0.0:20777".into(), allow_from: vec![] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HttpConfig {
    /// `0.0.0.0:8080` for LAN/Tailscale, or `100.x.y.z:8080` to bind to the Tailscale IP only.
    pub bind: String,
    /// Optional shared token for mutating WebSocket commands and REST POSTs.
    pub token: Option<String>,
    /// Built UI directory (`ui/build`).
    pub ui_dir: String,
}
impl Default for HttpConfig {
    fn default() -> Self {
        Self { bind: "0.0.0.0:8080".into(), token: None, ui_dir: "ui/build".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DbConfig {
    pub path: String,
}
impl Default for DbConfig {
    fn default() -> Self {
        Self { path: "data/pitwall.db".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub record: bool,
    pub dir: String,
}
impl Default for CaptureConfig {
    fn default() -> Self {
        Self { record: false, dir: "captures".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub enabled: bool,
    /// Substring of the output device name; empty = system default.
    pub device: String,
    pub cache_dir: String,
}
impl Default for AudioConfig {
    fn default() -> Self {
        Self { enabled: true, device: String::new(), cache_dir: "tts-cache".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CommsConfig {
    /// "auto" or "copilot".
    pub mode: String,
    pub override_ms: u64,
    pub p2_max_age_ms: u64,
    pub gate_throttle: f32,
    pub gate_steer: f32,
}
impl Default for CommsConfig {
    fn default() -> Self {
        Self { mode: "auto".into(), override_ms: 10_000, p2_max_age_ms: 20_000, gate_throttle: 0.85, gate_steer: 0.15 }
    }
}

impl Config {
    pub fn load(args: &Args) -> anyhow::Result<Config> {
        let path = args.config.clone().or_else(|| {
            let p = PathBuf::from("config.toml");
            p.exists().then_some(p)
        });
        let mut cfg: Config = match &path {
            Some(p) => {
                let text = std::fs::read_to_string(p).map_err(|e| anyhow::anyhow!("read {}: {e}", p.display()))?;
                toml::from_str(&text).map_err(|e| anyhow::anyhow!("parse {}: {e}", p.display()))?
            }
            None => Config::default(),
        };
        if let Some(v) = &args.udp_bind {
            cfg.udp.bind = v.clone();
        }
        if let Some(v) = &args.http_bind {
            cfg.http.bind = v.clone();
        }
        if let Some(v) = &args.token {
            cfg.http.token = Some(v.clone());
        }
        if let Some(v) = &args.db {
            cfg.db.path = v.clone();
        }
        if args.record {
            cfg.capture.record = true;
        }
        if args.no_audio {
            cfg.audio.enabled = false;
        }
        cfg.http.token = cfg.http.token.filter(|t| !t.is_empty());
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_parses() {
        let text = include_str!("../../../config.example.toml");
        let cfg: Config = toml::from_str(text).unwrap();
        assert_eq!(cfg.udp.bind, "0.0.0.0:20777");
        assert_eq!(cfg.comms.override_ms, 10_000);
    }
}
