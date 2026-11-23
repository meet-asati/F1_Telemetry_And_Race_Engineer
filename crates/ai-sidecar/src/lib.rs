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
    ///
    /// Tolerant of streamed WAVs (Kokoro-FastAPI writes `0xFFFFFFFF` as the
    /// RIFF and data sizes because it doesn't know the length up front): the
    /// data chunk is read to the end of the buffer and trimmed to whole frames.
    pub fn from_wav(bytes: &[u8]) -> anyhow::Result<Pcm> {
        use anyhow::bail;
        let u16le = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
        let u32le = |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            bail!("parse WAV: not a RIFF/WAVE file");
        }
        let mut fmt: Option<(u16, u16, u32, u16)> = None; // (format, channels, rate, bits)
        let mut o = 12;
        while o + 8 <= bytes.len() {
            let id = &bytes[o..o + 4];
            let declared = u32le(o + 4) as usize;
            let body = o + 8;
            if id == b"fmt " {
                if body + 16 > bytes.len() {
                    bail!("parse WAV: truncated fmt chunk");
                }
                let mut format = u16le(body);
                if format == 0xFFFE && declared >= 40 && body + 26 <= bytes.len() {
                    format = u16le(body + 24); // WAVE_FORMAT_EXTENSIBLE: sub-format GUID
                }
                fmt = Some((format, u16le(body + 2), u32le(body + 4), u16le(body + 14)));
            } else if id == b"data" {
                let (format, channels, rate, bits) = fmt.context("parse WAV: data before fmt")?;
                let ch = channels.max(1) as usize;
                let bps = (bits as usize).div_ceil(8).max(1);
                let avail = bytes.len() - body;
                let len = if declared == u32::MAX as usize || declared > avail { avail } else { declared };
                let len = len - len % (bps * ch);
                let raw = &bytes[body..body + len];
                let interleaved: Vec<f32> = match (format, bits) {
                    (3, 32) => raw.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
                    (1, 8) => raw.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect(),
                    (1, 16) => raw.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect(),
                    (1, 24) => raw.chunks_exact(3).map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0).collect(),
                    (1, 32) => raw.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0).collect(),
                    _ => bail!("parse WAV: unsupported format {format} / {bits}-bit"),
                };
                let samples = interleaved.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
                return Ok(Pcm { samples, sample_rate: rate });
            }
            if declared == u32::MAX as usize {
                break;
            }
            o = body + declared + (declared % 2);
        }
        bail!("parse WAV: no data chunk")
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
    fn streamed_wav_with_unknown_sizes() {
        // Kokoro-FastAPI: RIFF and data sizes are 0xFFFFFFFF, a LIST chunk
        // sits between fmt and data, and the data may end mid-sample.
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&u32::MAX.to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&24_000u32.to_le_bytes());
        b.extend_from_slice(&48_000u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"LIST");
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(b"INFO");
        b.extend_from_slice(b"data");
        b.extend_from_slice(&u32::MAX.to_le_bytes());
        for v in [16384i16, -16384, 0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.push(0x7f); // half a sample
        let p = Pcm::from_wav(&b).unwrap();
        assert_eq!(p.sample_rate, 24_000);
        assert_eq!(p.samples.len(), 3);
        assert!((p.samples[0] - 0.5).abs() < 1e-4 && (p.samples[1] + 0.5).abs() < 1e-4);
        assert!(Pcm::from_wav(b"nope").is_err());
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

#[cfg(test)]
mod real_file {
    /// `PCM_WAV=path cargo test -p ai-sidecar real_wav -- --ignored`
    #[test]
    #[ignore]
    fn real_wav() {
        let path = std::env::var("PCM_WAV").expect("set PCM_WAV");
        let p = super::Pcm::from_wav(&std::fs::read(path).unwrap()).unwrap();
        println!("rate {} samples {} ({} ms) rms {:.3}", p.sample_rate, p.samples.len(), p.duration_ms(), p.rms());
        assert!(p.duration_ms() > 300);
    }
}
