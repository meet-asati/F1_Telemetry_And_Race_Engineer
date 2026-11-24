//! Local radio playback through `cpal`.
//!
//! The cpal stream lives on its own OS thread (streams are not `Send` on
//! every platform) and pulls from a shared queue; the async runtime only
//! pushes samples. A failing or missing audio device never affects callers
//! beyond `play` returning an error.

use anyhow::{anyhow, Context};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

struct Shared {
    queue: Mutex<VecDeque<f32>>,
    alive: AtomicBool,
    /// Bumped by `stop()` so a multi-sentence line knows it was cut off.
    epoch: std::sync::atomic::AtomicU64,
}

pub struct AudioPlayer {
    shared: Arc<Shared>,
    sample_rate: u32,
    device_name: String,
    _keepalive: mpsc::Sender<()>,
}

pub fn list_output_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.output_devices().map(|ds| ds.filter_map(|d| d.name().ok()).collect()).unwrap_or_default()
}

impl AudioPlayer {
    /// Open the output device whose name contains `device` (case-insensitive),
    /// or the system default.
    pub fn start(device: Option<&str>) -> anyhow::Result<Self> {
        let wanted = device.map(|s| s.to_lowercase());
        let shared = Arc::new(Shared { queue: Mutex::new(VecDeque::new()), alive: AtomicBool::new(true), epoch: std::sync::atomic::AtomicU64::new(0) });
        let (ready_tx, ready_rx) = mpsc::channel::<anyhow::Result<(u32, String)>>();
        let (keep_tx, keep_rx) = mpsc::channel::<()>();
        let sh = shared.clone();
        std::thread::Builder::new()
            .name("radio-audio".into())
            .spawn(move || {
                let res = (|| -> anyhow::Result<(cpal::Stream, u32, String)> {
                    let host = cpal::default_host();
                    let dev = match &wanted {
                        Some(w) => host
                            .output_devices()?
                            .find(|d| d.name().map(|n| n.to_lowercase().contains(w)).unwrap_or(false))
                            .ok_or_else(|| anyhow!("no output device matching '{w}'"))?,
                        None => host.default_output_device().ok_or_else(|| anyhow!("no default output device"))?,
                    };
                    let name = dev.name().unwrap_or_else(|_| "unknown".into());
                    let supported = dev.default_output_config().context("query output config")?;
                    let fmt = supported.sample_format();
                    let config: cpal::StreamConfig = supported.into();
                    let channels = config.channels as usize;
                    let rate = config.sample_rate.0;
                    let err_sh = sh.clone();
                    let on_err = move |e: cpal::StreamError| {
                        tracing::warn!("audio stream error: {e}");
                        err_sh.alive.store(false, Ordering::Relaxed);
                    };
                    let stream = match fmt {
                        cpal::SampleFormat::F32 => {
                            let sh = sh.clone();
                            dev.build_output_stream(&config, move |out: &mut [f32], _| fill(&sh, out, channels, |v| v), on_err, None)?
                        }
                        cpal::SampleFormat::I16 => {
                            let sh = sh.clone();
                            dev.build_output_stream(&config, move |out: &mut [i16], _| fill(&sh, out, channels, |v| (v * i16::MAX as f32) as i16), on_err, None)?
                        }
                        cpal::SampleFormat::U16 => {
                            let sh = sh.clone();
                            dev.build_output_stream(&config, move |out: &mut [u16], _| fill(&sh, out, channels, |v| ((v * 0.5 + 0.5) * u16::MAX as f32) as u16), on_err, None)?
                        }
                        other => return Err(anyhow!("unsupported sample format {other:?}")),
                    };
                    stream.play()?;
                    Ok((stream, rate, name))
                })();
                match res {
                    Ok((stream, rate, name)) => {
                        let _ = ready_tx.send(Ok((rate, name)));
                        // Park until the player is dropped.
                        let _ = keep_rx.recv();
                        drop(stream);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })?;
        let (sample_rate, device_name) = ready_rx.recv().map_err(|_| anyhow!("audio thread died"))??;
        Ok(Self { shared, sample_rate, device_name, _keepalive: keep_tx })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn is_alive(&self) -> bool {
        self.shared.alive.load(Ordering::Relaxed)
    }

    /// Queue mono samples (already at `sample_rate`). With `interrupt`, anything
    /// still playing is cut first. Returns the playback duration in ms
    /// (including whatever was already queued ahead of it).
    pub fn play(&self, mono: &[f32], interrupt: bool) -> anyhow::Result<u64> {
        if !self.is_alive() {
            return Err(anyhow!("audio device unavailable"));
        }
        let mut q = self.shared.queue.lock().map_err(|_| anyhow!("audio queue poisoned"))?;
        if interrupt {
            q.clear();
        }
        q.extend(mono.iter().copied());
        Ok(q.len() as u64 * 1000 / self.sample_rate.max(1) as u64)
    }

    pub fn stop(&self) {
        self.shared.epoch.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut q) = self.shared.queue.lock() {
            q.clear();
        }
    }

    /// Changes whenever playback is stopped from outside (driver keyed up).
    pub fn epoch(&self) -> u64 {
        self.shared.epoch.load(Ordering::Relaxed)
    }
}

fn fill<T: Copy>(sh: &Shared, out: &mut [T], channels: usize, conv: impl Fn(f32) -> T) {
    let silence = conv(0.0);
    let Ok(mut q) = sh.queue.try_lock() else {
        out.iter_mut().for_each(|s| *s = silence);
        return;
    };
    for frame in out.chunks_mut(channels.max(1)) {
        let v = conv(q.pop_front().unwrap_or(0.0).clamp(-1.0, 1.0));
        frame.iter_mut().for_each(|s| *s = v);
    }
}
