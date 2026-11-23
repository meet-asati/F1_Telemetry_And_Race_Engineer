//! HTTP clients for local AI sidecars.
//!
//! M4 ships text-to-speech:
//! * **Piper** HTTP server (`python -m piper.http_server -m <voice>`): `POST /`
//!   with `{"text": "..."}` returns a WAV file.
//! * **Kokoro** via an OpenAI-compatible server (e.g. Kokoro-FastAPI):
//!   `POST /v1/audio/speech` with `{"model","input","voice","response_format":"wav"}`.
//!
//! Speech-to-text (whisper.cpp) and the LLM (Ollama) clients arrive in M5.
//! Every call has a timeout; failures are returned as errors and never panic.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::time::Duration;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TtsKind {
    Piper,
    Kokoro,
    #[default]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TtsConfig {
    pub engine: TtsKind,
    /// Base URL, e.g. `http://127.0.0.1:5000` (Piper) or `http://127.0.0.1:8880` (Kokoro).
    pub url: String,
    /// Kokoro voice id (ignored by Piper, whose voice is chosen at server start).
    pub voice: String,
    /// Kokoro model name.
    pub model: String,
    pub timeout_ms: u64,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self { engine: TtsKind::None, url: "http://127.0.0.1:5000".into(), voice: "am_michael".into(), model: "kokoro".into(), timeout_ms: 8000 }
    }
}

#[derive(Clone)]
pub struct TtsClient {
    cfg: TtsConfig,
    http: reqwest::Client,
}

impl TtsClient {
    pub fn new(cfg: TtsConfig) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder().timeout(Duration::from_millis(cfg.timeout_ms.max(500))).build()?;
        Ok(Self { cfg, http })
    }

    pub fn enabled(&self) -> bool {
        self.cfg.engine != TtsKind::None
    }

    /// Identifies engine + voice so cached audio is never reused across voices.
    pub fn cache_namespace(&self) -> String {
        match self.cfg.engine {
            TtsKind::Piper => format!("piper|{}", self.cfg.url),
            TtsKind::Kokoro => format!("kokoro|{}|{}", self.cfg.model, self.cfg.voice),
            TtsKind::None => "none".into(),
        }
    }

    /// Is the sidecar reachable? (Any HTTP response counts.)
    pub async fn health(&self) -> bool {
        if !self.enabled() {
            return false;
        }
        self.http.get(&self.cfg.url).timeout(Duration::from_secs(2)).send().await.is_ok()
    }

    pub async fn synthesize(&self, text: &str) -> anyhow::Result<Pcm> {
        let base = self.cfg.url.trim_end_matches('/');
        let resp = match self.cfg.engine {
            TtsKind::None => bail!("TTS disabled"),
            TtsKind::Piper => {
                let r = self.http.post(format!("{base}/")).json(&serde_json::json!({ "text": text })).send().await?;
                if r.status().is_success() {
                    r
                } else {
                    // Older Piper http_server.py accepts the raw text body.
                    self.http.post(format!("{base}/")).body(text.to_string()).send().await?
                }
            }
            TtsKind::Kokoro => {
                self.http
                    .post(format!("{base}/v1/audio/speech"))
                    .json(&serde_json::json!({
                        "model": self.cfg.model,
                        "input": text,
                        "voice": self.cfg.voice,
                        "response_format": "wav",
                    }))
                    .send()
                    .await?
            }
        };
        let status = resp.status();
        if !status.is_success() {
            bail!("TTS HTTP {status}");
        }
        let bytes = resp.bytes().await?;
        let pcm = Pcm::from_wav(&bytes)?;
        if pcm.samples.is_empty() {
            bail!("TTS returned empty audio");
        }
        Ok(pcm)
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

    #[tokio::test]
    async fn unreachable_sidecar_is_an_error_not_a_panic() {
        let c = TtsClient::new(TtsConfig { engine: TtsKind::Kokoro, url: "http://127.0.0.1:9".into(), timeout_ms: 500, ..Default::default() }).unwrap();
        assert!(c.synthesize("hello").await.is_err());
        assert!(!c.health().await);
        let off = TtsClient::new(TtsConfig::default()).unwrap();
        assert!(off.synthesize("x").await.is_err());
    }
}
