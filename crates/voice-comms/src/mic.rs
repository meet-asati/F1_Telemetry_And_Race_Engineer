//! Microphone capture for push-to-talk.
//!
//! The input stream runs continuously on its own thread (opening a device on
//! every press would add latency and clip the first word). While idle it keeps
//! a short pre-roll ring; [`MicCapture::begin`] moves the pre-roll into the
//! clip and [`MicCapture::end`] returns everything captured since.

use ai_sidecar::Pcm;
use anyhow::{anyhow, Context};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};

const PREROLL_MS: u32 = 300;
/// Hard cap so a stuck PTT can never grow memory unbounded (60 s).
const MAX_CLIP_S: u32 = 60;

struct Shared {
    armed: AtomicBool,
    alive: AtomicBool,
    /// Decaying peak input level (f32 bits), for the UI level meter.
    level: AtomicU32,
    /// Any non-zero sample seen yet? A real mic always has a noise floor, so
    /// pure digital silence means access is blocked or the device is dead.
    any_signal: AtomicBool,
    /// Samples received (to know the stream is actually running).
    frames: std::sync::atomic::AtomicU64,
    clip: Mutex<Vec<f32>>,
    preroll: Mutex<VecDeque<f32>>,
}

pub struct MicCapture {
    shared: Arc<Shared>,
    sample_rate: u32,
    device_name: String,
    _keepalive: mpsc::Sender<()>,
}

pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices().map(|ds| ds.filter_map(|d| d.name().ok()).collect()).unwrap_or_default()
}

/// Loopback / streaming / virtual-cable inputs that are never a real microphone.
pub fn is_virtual_input(name: &str) -> bool {
    let n = name.to_lowercase();
    ["steam streaming", "stereo mix", "cable output", "voicemeeter", "virtual", "aux jack", "wave link", "loopback", "what u hear"].iter().any(|k| n.contains(k))
}

/// Ranking used when no device is configured: the system default if it is a
/// real mic; otherwise built-in / wired mics before Bluetooth headsets (opening
/// a Bluetooth headset mic drops the buds into low-quality call audio).
pub fn auto_pick_input(names: &[String], default: Option<&str>) -> Option<String> {
    if let Some(d) = default.filter(|d| !is_virtual_input(d)) {
        return Some(d.to_string());
    }
    let real: Vec<&String> = names.iter().filter(|n| !is_virtual_input(n)).collect();
    let score = |n: &str| {
        let l = n.to_lowercase();
        if l.contains("headset") || l.contains("hands-free") || l.contains("buds") || l.contains("airpods") {
            1
        } else if l.contains("microphone") || l.contains("mic") {
            3
        } else {
            2
        }
    };
    real.into_iter().max_by_key(|n| score(n)).cloned()
}

impl MicCapture {
    /// Open the input device whose name contains `device` (case-insensitive),
    /// or the system default microphone.
    pub fn start(device: Option<&str>) -> anyhow::Result<Self> {
        let wanted = device.map(|s| s.to_lowercase());
        let shared = Arc::new(Shared {
            armed: AtomicBool::new(false),
            alive: AtomicBool::new(true),
            level: AtomicU32::new(0),
            any_signal: AtomicBool::new(false),
            frames: std::sync::atomic::AtomicU64::new(0),
            clip: Mutex::new(Vec::new()),
            preroll: Mutex::new(VecDeque::new()),
        });
        let (ready_tx, ready_rx) = mpsc::channel::<anyhow::Result<(u32, String)>>();
        let (keep_tx, keep_rx) = mpsc::channel::<()>();
        let sh = shared.clone();
        std::thread::Builder::new().name("ptt-mic".into()).spawn(move || {
            let res = (|| -> anyhow::Result<(cpal::Stream, u32, String)> {
                let host = cpal::default_host();
                let want = match &wanted {
                    Some(w) => w.clone(),
                    None => {
                        let names: Vec<String> = host.input_devices()?.filter_map(|d| d.name().ok()).collect();
                        let default = host.default_input_device().and_then(|d| d.name().ok());
                        let pick = auto_pick_input(&names, default.as_deref()).ok_or_else(|| anyhow!("no real microphone found (only virtual inputs)"))?;
                        if default.as_deref() != Some(pick.as_str()) {
                            tracing::info!("default recording device {:?} is virtual; using '{pick}'", default.unwrap_or_default());
                        }
                        pick.to_lowercase()
                    }
                };
                let dev = host
                    .input_devices()?
                    .find(|d| d.name().map(|n| n.to_lowercase().contains(&want)).unwrap_or(false))
                    .ok_or_else(|| anyhow!("no input device matching '{want}'"))?;
                let name = dev.name().unwrap_or_else(|_| "unknown".into());
                let supported = dev.default_input_config().context("query input config")?;
                let fmt = supported.sample_format();
                let config: cpal::StreamConfig = supported.into();
                let ch = config.channels as usize;
                let rate = config.sample_rate.0;
                let preroll_len = (rate * PREROLL_MS / 1000) as usize;
                let max_clip = (rate * MAX_CLIP_S) as usize;
                let err_sh = sh.clone();
                let on_err = move |e: cpal::StreamError| {
                    tracing::warn!("microphone stream error: {e}");
                    err_sh.alive.store(false, Ordering::Relaxed);
                };
                macro_rules! build {
                    ($t:ty, $conv:expr) => {{
                        let sh = sh.clone();
                        dev.build_input_stream(&config, move |data: &[$t], _| push(&sh, data, ch, preroll_len, max_clip, $conv), on_err, None)?
                    }};
                }
                let stream = match fmt {
                    cpal::SampleFormat::F32 => build!(f32, |v: f32| v),
                    cpal::SampleFormat::I16 => build!(i16, |v: i16| v as f32 / i16::MAX as f32),
                    cpal::SampleFormat::U16 => build!(u16, |v: u16| v as f32 / u16::MAX as f32 * 2.0 - 1.0),
                    other => return Err(anyhow!("unsupported microphone format {other:?}")),
                };
                stream.play()?;
                Ok((stream, rate, name))
            })();
            match res {
                Ok((stream, rate, name)) => {
                    let _ = ready_tx.send(Ok((rate, name)));
                    let _ = keep_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        })?;
        let (sample_rate, device_name) = ready_rx.recv().map_err(|_| anyhow!("microphone thread died"))??;
        Ok(Self { shared, sample_rate, device_name, _keepalive: keep_tx })
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn is_alive(&self) -> bool {
        self.shared.alive.load(Ordering::Relaxed)
    }

    /// Current input level, 0..1 (decaying peak), for a level meter.
    /// `Some(true)` once audio is flowing but every sample has been exactly
    /// zero (Windows privacy setting blocking desktop apps, muted or dead
    /// device). `None` until about a second of audio has arrived.
    pub fn digital_silence(&self) -> Option<bool> {
        let frames = self.shared.frames.load(Ordering::Relaxed);
        if self.shared.any_signal.load(Ordering::Relaxed) {
            Some(false)
        } else if frames >= self.sample_rate as u64 {
            Some(true)
        } else {
            None
        }
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.shared.level.load(Ordering::Relaxed))
    }

    pub fn is_recording(&self) -> bool {
        self.shared.armed.load(Ordering::Relaxed)
    }

    /// Start a clip (includes ~300 ms of pre-roll).
    pub fn begin(&self) {
        let pre: Vec<f32> = self.shared.preroll.lock().map(|mut p| p.drain(..).collect()).unwrap_or_default();
        if let Ok(mut c) = self.shared.clip.lock() {
            c.clear();
            c.extend(pre);
        }
        self.shared.armed.store(true, Ordering::Relaxed);
    }

    /// Finish the clip and return it.
    pub fn end(&self) -> Pcm {
        self.shared.armed.store(false, Ordering::Relaxed);
        let samples = self.shared.clip.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default();
        Pcm { samples, sample_rate: self.sample_rate }
    }
}

fn push<T: Copy>(sh: &Shared, data: &[T], ch: usize, preroll_len: usize, max_clip: usize, conv: impl Fn(T) -> f32) {
    let mono: Vec<f32> = data.chunks(ch.max(1)).map(|f| f.iter().map(|&v| conv(v)).sum::<f32>() / ch.max(1) as f32).collect();
    let peak = mono.iter().fold(0.0f32, |m, v| m.max(v.abs())).min(1.0);
    let prev = f32::from_bits(sh.level.load(Ordering::Relaxed));
    sh.level.store(peak.max(prev * 0.85).to_bits(), Ordering::Relaxed);
    sh.frames.fetch_add(mono.len() as u64, Ordering::Relaxed);
    if peak > 0.0 {
        sh.any_signal.store(true, Ordering::Relaxed);
    }
    let mono = mono.into_iter();
    if sh.armed.load(Ordering::Relaxed) {
        if let Ok(mut c) = sh.clip.try_lock() {
            if c.len() < max_clip {
                c.extend(mono);
            }
        }
    } else if let Ok(mut p) = sh.preroll.try_lock() {
        p.extend(mono);
        let excess = p.len().saturating_sub(preroll_len);
        p.drain(..excess);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_pick_skips_virtual_and_prefers_builtin_over_bluetooth() {
        let names: Vec<String> = [
            "Microphone (Steam Streaming Microphone)",
            "Internal AUX Jack (Steam Streaming Speakers)",
            "Headset (realme Buds T300)",
            "Microphone Array (Intel® Smart Sound Technology for Digital Microphones)",
        ]
        .map(String::from)
        .to_vec();
        let pick = auto_pick_input(&names, Some("Internal AUX Jack (Steam Streaming Speakers)")).unwrap();
        assert!(pick.starts_with("Microphone Array"), "{pick}");
        assert_eq!(auto_pick_input(&names, Some("Headset (realme Buds T300)")).unwrap(), "Headset (realme Buds T300)", "a real default is respected");
        assert!(auto_pick_input(&names[..2], None).is_none());
    }
}
