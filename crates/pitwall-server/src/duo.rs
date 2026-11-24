//! Duo mode: live voice between the friend's browser and the driver.
//!
//! * The **team-radio bus** ([`BusFrame`], 16 kHz mono PCM) carries what is
//!   said on the radio to every listening browser: the AI engineer's lines
//!   and the driver's push-to-talk audio.
//! * The friend's browser streams its microphone over `/ws/radio`; the
//!   [`DuoActor`] runs it through the live radio effect and plays it on the
//!   driver's speakers, while the AI holds its calls.

use crate::actors::{now_ms, CommsCmd};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use voice_comms::{AudioPlayer, RadioFx, RadioFxStream, StreamResampler};

/// Sample rate of the team-radio bus and of the browser's microphone stream.
pub const BUS_RATE: u32 = 16_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusSource {
    /// The AI engineer / typed radio (TTS).
    Engineer = 1,
    /// The driver's push-to-talk microphone.
    Driver = 2,
    /// A friend talking from the browser.
    Friend = 3,
}

#[derive(Debug, Clone)]
pub struct BusFrame {
    pub source: BusSource,
    /// Which radio socket sent it (friend audio only), so it isn't echoed back.
    pub from: u64,
    pub pcm: Arc<Vec<i16>>,
}

pub type Bus = broadcast::Sender<BusFrame>;

pub fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16).collect()
}

/// Put already-processed radio audio (at `rate`) on the bus in ~100 ms frames.
pub fn publish(bus: &Bus, source: BusSource, samples: &[f32], rate: u32) {
    if bus.receiver_count() == 0 || samples.is_empty() {
        return;
    }
    let pcm = to_i16(&voice_comms::resample(samples, rate, BUS_RATE));
    for chunk in pcm.chunks(BUS_RATE as usize / 10) {
        let _ = bus.send(BusFrame { source, from: 0, pcm: Arc::new(chunk.to_vec()) });
    }
}

/// Stream the driver's push-to-talk mic onto the bus (runs on its own thread
/// until the mic is closed).
pub fn attach_driver_mic(mic: &voice_comms::MicCapture, bus: Bus, fx: RadioFx) {
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(64);
    mic.set_tap(Some(tx));
    let rate = mic.sample_rate();
    let _ = std::thread::Builder::new().name("driver-radio".into()).spawn(move || {
        let mut fx = RadioFxStream::new(&fx, rate);
        let mut rs = StreamResampler::new(rate, BUS_RATE);
        let mut buf: Vec<i16> = Vec::new();
        while let Ok(chunk) = rx.recv() {
            if bus.receiver_count() == 0 {
                continue;
            }
            buf.extend(to_i16(&rs.process(&fx.process(&chunk))));
            // ~40 ms frames keep latency low without flooding the socket.
            while buf.len() >= 640 {
                let frame: Vec<i16> = buf.drain(..640).collect();
                let _ = bus.send(BusFrame { source: BusSource::Driver, from: 0, pcm: Arc::new(frame) });
            }
        }
    });
}

pub enum DuoCmd {
    TxStart { from: u64 },
    TxStop { from: u64 },
    /// 16 kHz mono PCM from the friend's browser.
    Audio { from: u64, pcm: Vec<i16> },
}

/// Plays the friend's live voice on the driver's speakers.
pub struct DuoActor {
    pub rx: mpsc::Receiver<DuoCmd>,
    pub player: Option<Arc<AudioPlayer>>,
    pub comms: mpsc::Sender<CommsCmd>,
    pub bus: Bus,
    pub fx: RadioFx,
}

/// Audio buffered before playback starts, to ride out network jitter.
const JITTER_MS: usize = 80;

impl DuoActor {
    pub async fn run(mut self) {
        let mut talking: Option<u64> = None;
        let mut last_audio = 0u64;
        let mut fx: Option<RadioFxStream> = None;
        let mut rs: Option<StreamResampler> = None;
        let mut pending: Vec<f32> = Vec::new();
        let mut playing = false;
        let mut watchdog = tokio::time::interval(Duration::from_millis(250));
        loop {
            tokio::select! {
                cmd = self.rx.recv() => {
                    let Some(cmd) = cmd else { break };
                    match cmd {
                        DuoCmd::TxStart { from } => {
                            if talking.is_some_and(|t| t != from) {
                                continue; // one engineer on the radio at a time
                            }
                            talking = Some(from);
                            last_audio = now_ms();
                            let out_rate = self.player.as_ref().map(|p| p.sample_rate()).unwrap_or(48_000);
                            fx = Some(RadioFxStream::new(&self.fx, BUS_RATE));
                            rs = Some(StreamResampler::new(BUS_RATE, out_rate));
                            pending.clear();
                            playing = false;
                            if let Some(p) = &self.player {
                                p.stop(); // cut the AI off
                                let _ = p.play(&voice_comms::chirp(p.sample_rate()), true);
                            }
                            tracing::info!("team radio: engineer transmitting");
                            let _ = self.comms.send(CommsCmd::EngineerTx(true)).await;
                        }
                        DuoCmd::TxStop { from } if talking == Some(from) => {
                            self.flush(&mut pending, playing);
                            talking = None;
                            tracing::info!("team radio: engineer finished");
                            let _ = self.comms.send(CommsCmd::EngineerTx(false)).await;
                        }
                        DuoCmd::TxStop { .. } => {}
                        DuoCmd::Audio { from, pcm } if talking == Some(from) => {
                            last_audio = now_ms();
                            let x: Vec<f32> = pcm.iter().map(|&v| v as f32 / 32768.0).collect();
                            let (Some(f), Some(r)) = (fx.as_mut(), rs.as_mut()) else { continue };
                            let wet = f.process(&x);
                            let _ = self.bus.send(BusFrame { source: BusSource::Friend, from, pcm: Arc::new(to_i16(&wet)) });
                            pending.extend(r.process(&wet));
                            let rate = self.player.as_ref().map(|p| p.sample_rate()).unwrap_or(48_000) as usize;
                            if playing || pending.len() >= rate * JITTER_MS / 1000 {
                                playing = true;
                                self.flush(&mut pending, true);
                            }
                        }
                        DuoCmd::Audio { .. } => {}
                    }
                }
                _ = watchdog.tick() => {
                    // Browser vanished mid-transmission: close the channel.
                    if talking.is_some() && now_ms().saturating_sub(last_audio) > 1500 {
                        self.flush(&mut pending, playing);
                        talking = None;
                        tracing::warn!("team radio: engineer audio stopped arriving, closing the channel");
                        let _ = self.comms.send(CommsCmd::EngineerTx(false)).await;
                    }
                }
            }
        }
    }

    fn flush(&self, pending: &mut Vec<f32>, _playing: bool) {
        if pending.is_empty() {
            return;
        }
        if let Some(p) = &self.player {
            if let Ok(ms) = p.play(pending, false) {
                let _ = self.comms.try_send(CommsCmd::SpeakingUntil(now_ms() + ms));
            }
        }
        pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_splits_into_frames_at_bus_rate() {
        let (bus, mut rx) = broadcast::channel(64);
        let one_sec: Vec<f32> = (0..24_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        publish(&bus, BusSource::Engineer, &one_sec, 24_000);
        let mut total = 0;
        while let Ok(f) = rx.try_recv() {
            assert_eq!(f.source, BusSource::Engineer);
            total += f.pcm.len();
        }
        assert!((total as i64 - 16_000).abs() <= 2, "{total}");
    }

    #[test]
    fn no_listeners_no_work() {
        let (bus, rx) = broadcast::channel::<BusFrame>(4);
        drop(rx);
        publish(&bus, BusSource::Engineer, &[0.1; 1000], 24_000); // must not panic
    }
}
