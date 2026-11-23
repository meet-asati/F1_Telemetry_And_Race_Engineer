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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

const PREROLL_MS: u32 = 300;
/// Hard cap so a stuck PTT can never grow memory unbounded (60 s).
const MAX_CLIP_S: u32 = 60;

struct Shared {
    armed: AtomicBool,
    alive: AtomicBool,
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

impl MicCapture {
    /// Open the input device whose name contains `device` (case-insensitive),
    /// or the system default microphone.
    pub fn start(device: Option<&str>) -> anyhow::Result<Self> {
        let wanted = device.map(|s| s.to_lowercase());
        let shared = Arc::new(Shared { armed: AtomicBool::new(false), alive: AtomicBool::new(true), clip: Mutex::new(Vec::new()), preroll: Mutex::new(VecDeque::new()) });
        let (ready_tx, ready_rx) = mpsc::channel::<anyhow::Result<(u32, String)>>();
        let (keep_tx, keep_rx) = mpsc::channel::<()>();
        let sh = shared.clone();
        std::thread::Builder::new().name("ptt-mic".into()).spawn(move || {
            let res = (|| -> anyhow::Result<(cpal::Stream, u32, String)> {
                let host = cpal::default_host();
                let dev = match &wanted {
                    Some(w) => host
                        .input_devices()?
                        .find(|d| d.name().map(|n| n.to_lowercase().contains(w)).unwrap_or(false))
                        .ok_or_else(|| anyhow!("no input device matching '{w}'"))?,
                    None => host.default_input_device().ok_or_else(|| anyhow!("no default microphone"))?,
                };
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
    let mono = data.chunks(ch.max(1)).map(|f| f.iter().map(|&v| conv(v)).sum::<f32>() / ch.max(1) as f32);
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
