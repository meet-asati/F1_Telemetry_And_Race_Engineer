//! Packet sources: live UDP (with optional recording) and capture replay.

use crate::config::Config;
use f1_udp::capture::{CaptureReader, Recorder};
use f1_udp::{decode, ParsedPacket};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

#[derive(Default)]
pub struct IngestStats {
    pub datagrams: AtomicU64,
    pub decode_errors: AtomicU64,
    pub dropped: AtomicU64,
    pub rejected_source: AtomicU64,
}

pub async fn run_udp(cfg: Config, tx: mpsc::Sender<ParsedPacket>, stats: Arc<IngestStats>) -> anyhow::Result<()> {
    let sock = UdpSocket::bind(&cfg.udp.bind).await.map_err(|e| anyhow::anyhow!("bind UDP {}: {e}", cfg.udp.bind))?;
    tracing::info!("listening for F1 UDP telemetry on {}", cfg.udp.bind);
    let allow: Vec<IpAddr> = cfg.udp.allow_from.iter().filter_map(|s| s.parse().ok()).collect();
    let mut recorder: Option<Recorder> = None;
    let mut buf = vec![0u8; 4096];
    loop {
        let (n, from) = match sock.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                // Windows reports ICMP port-unreachable as a recv error; keep going.
                tracing::debug!("udp recv error: {e}");
                continue;
            }
        };
        if !allow.is_empty() && !allow.contains(&from.ip()) {
            stats.rejected_source.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        stats.datagrams.fetch_add(1, Ordering::Relaxed);
        let data = &buf[..n];
        if cfg.capture.record {
            if recorder.is_none() {
                let fmt = if n >= 2 { u16::from_le_bytes([data[0], data[1]]) } else { 0 };
                let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let path = PathBuf::from(&cfg.capture.dir).join(format!("capture-{fmt}-{secs}.bin"));
                match Recorder::create(&path, fmt) {
                    Ok(r) => {
                        tracing::info!("recording capture to {}", path.display());
                        recorder = Some(r);
                    }
                    Err(e) => tracing::warn!("cannot record capture: {e}"),
                }
            }
            if let Some(r) = recorder.as_mut() {
                if let Err(e) = r.record(data) {
                    tracing::warn!("capture write failed, recording stopped: {e}");
                    recorder = None;
                }
            }
        }
        match decode(data) {
            Ok(p) => {
                if tx.try_send(p).is_err() {
                    stats.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(e) => {
                let c = stats.decode_errors.fetch_add(1, Ordering::Relaxed);
                if c < 5 {
                    tracing::warn!("decode error from {from}: {e}");
                }
            }
        }
    }
}

/// Replay a capture file in real time (scaled by `speed`; 0 = flat out).
/// Runs on a blocking thread.
pub fn run_replay(path: PathBuf, speed: f32, tx: mpsc::Sender<ParsedPacket>, stats: Arc<IngestStats>) -> anyhow::Result<()> {
    let reader = CaptureReader::open(&path).map_err(|e| anyhow::anyhow!("open {}: {e}", path.display()))?;
    tracing::info!("replaying {} (format {}) at {}x", path.display(), reader.packet_format, speed);
    let start = Instant::now();
    let mut first_t: Option<u64> = None;
    for rec in reader {
        let rec = rec?;
        let t0 = *first_t.get_or_insert(rec.t_us);
        if speed > 0.0 {
            let due = Duration::from_micros(((rec.t_us - t0) as f64 / speed as f64) as u64);
            if let Some(wait) = due.checked_sub(start.elapsed()) {
                std::thread::sleep(wait);
            }
        }
        stats.datagrams.fetch_add(1, Ordering::Relaxed);
        match decode(&rec.payload) {
            Ok(p) => {
                if tx.blocking_send(p).is_err() {
                    break;
                }
            }
            Err(_) => {
                stats.decode_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    tracing::info!("replay finished");
    Ok(())
}
