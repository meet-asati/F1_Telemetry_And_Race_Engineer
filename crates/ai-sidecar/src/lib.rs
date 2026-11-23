//! HTTP clients for local AI sidecars. Every call has a timeout; failures are
//! returned as errors and never panic, so a missing sidecar only degrades the
//! radio (text-only, or no voice questions) and never the telemetry.
//!
//! * [`tts`]: **Kokoro** (OpenAI-compatible `/v1/audio/speech`, e.g.
//!   Kokoro-FastAPI) or **Piper** (`python -m piper.http_server`).
//! * [`stt`]: **whisper.cpp** `whisper-server` (`/inference`) or any
//!   OpenAI-compatible `/v1/audio/transcriptions` endpoint.
//! * [`llm`]: **Ollama** (`/api/chat`) or any OpenAI-compatible
//!   `/v1/chat/completions` endpoint.

pub mod llm;
pub mod stt;
pub mod tts;

pub use llm::{LlmClient, LlmConfig, LlmKind};
pub use stt::{SttClient, SttConfig, SttKind};
pub use tts::{TtsClient, TtsConfig, TtsKind};

use anyhow::Context;
use std::io::Cursor;

/// Mono PCM audio.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Pcm {
    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 {
            0
        } else {
            self.samples.len() as u64 * 1000 / self.sample_rate as u64
        }
    }

    /// Decode a WAV byte buffer (any bit depth / channel count) to mono f32.
    pub fn from_wav(bytes: &[u8]) -> anyhow::Result<Pcm> {
        let mut r = hound::WavReader::new(Cursor::new(bytes)).context("parse WAV")?;
        let spec = r.spec();
        let ch = spec.channels.max(1) as usize;
        let interleaved: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
            hound::SampleFormat::Int => {
                let scale = 1.0 / (1i64 << (spec.bits_per_sample.max(1) - 1)) as f32;
                r.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
            }
        };
        let samples = interleaved.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
        Ok(Pcm { samples, sample_rate: spec.sample_rate })
    }

    /// Linear-interpolation resample (good enough for speech).
    pub fn resampled(&self, to_rate: u32) -> Pcm {
        if self.sample_rate == to_rate || self.samples.is_empty() || self.sample_rate == 0 {
            return Pcm { samples: self.samples.clone(), sample_rate: to_rate.max(1) };
        }
        let ratio = self.sample_rate as f64 / to_rate as f64;
        let n = (self.samples.len() as f64 / ratio).floor() as usize;
        let samples = (0..n)
            .map(|i| {
                let pos = i as f64 * ratio;
                let k = pos as usize;
                let f = (pos - k as f64) as f32;
                let a = self.samples[k];
                let b = *self.samples.get(k + 1).unwrap_or(&a);
                a + (b - a) * f
            })
            .collect();
        Pcm { samples, sample_rate: to_rate }
    }

    /// Encode as 16-bit PCM mono WAV (what speech-to-text servers expect).
    pub fn to_wav_i16(&self) -> anyhow::Result<Vec<u8>> {
        let spec = hound::WavSpec { channels: 1, sample_rate: self.sample_rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut cur = Cursor::new(Vec::new());
        {
            let mut w = hound::WavWriter::new(&mut cur, spec)?;
            for s in &self.samples {
                w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
            }
            w.finalize()?;
        }
        Ok(cur.into_inner())
    }

    /// Root-mean-square level (0..1), used to drop silent push-to-talk clips.
    pub fn rms(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        (self.samples.iter().map(|s| s * s).sum::<f32>() / self.samples.len() as f32).sqrt()
    }

    /// Encode as 32-bit float mono WAV.
    pub fn to_wav(&self) -> anyhow::Result<Vec<u8>> {
        let spec = hound::WavSpec { channels: 1, sample_rate: self.sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let mut cur = Cursor::new(Vec::new());
        {
            let mut w = hound::WavWriter::new(&mut cur, spec)?;
            for s in &self.samples {
                w.write_sample(*s)?;
            }
            w.finalize()?;
        }
        Ok(cur.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_round_trip_and_int_decode() {
        let pcm = Pcm { samples: vec![0.0, 0.5, -0.5, 0.25], sample_rate: 22050 };
        let wav = pcm.to_wav().unwrap();
        assert_eq!(Pcm::from_wav(&wav).unwrap(), pcm);

        // 16-bit stereo -> mono
        let spec = hound::WavSpec { channels: 2, sample_rate: 16000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut cur = Cursor::new(Vec::new());
        {
            let mut w = hound::WavWriter::new(&mut cur, spec).unwrap();
            for s in [16384i16, 0, -16384, -16384] {
                w.write_sample(s).unwrap();
            }
            w.finalize().unwrap();
        }
        let m = Pcm::from_wav(&cur.into_inner()).unwrap();
        assert_eq!(m.samples.len(), 2);
        assert!((m.samples[0] - 0.25).abs() < 1e-3);
        assert!((m.samples[1] + 0.5).abs() < 1e-3);
        assert_eq!(m.duration_ms(), 0);
    }

    #[test]
    fn resample_and_i16_wav() {
        let pcm = Pcm { samples: (0..48000).map(|i| (i as f32 / 48000.0 * std::f32::consts::TAU * 440.0).sin() * 0.5).collect(), sample_rate: 48000 };
        let r = pcm.resampled(16000);
        assert_eq!(r.sample_rate, 16000);
        assert_eq!(r.samples.len(), 16000);
        assert!((r.rms() - pcm.rms()).abs() < 0.02);
        let back = Pcm::from_wav(&r.to_wav_i16().unwrap()).unwrap();
        assert_eq!(back.samples.len(), 16000);
        assert!((back.samples[100] - r.samples[100]).abs() < 1e-3);
    }
}
