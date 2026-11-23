//! Text-to-speech.
//!
//! * **Kokoro** (default) via an OpenAI-compatible server such as
//!   Kokoro-FastAPI: `POST /v1/audio/speech` with
//!   `{"model","input","voice","speed","response_format":"wav"}`.
//! * **Piper** HTTP server (`python -m piper.http_server -m <voice>`): `POST /`
//!   with `{"text": "..."}` returns a WAV file.

use crate::Pcm;
use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TtsKind {
    Piper,
    #[default]
    Kokoro,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TtsConfig {
    pub engine: TtsKind,
    /// Base URL, e.g. `http://127.0.0.1:8880` (Kokoro) or `http://127.0.0.1:5000` (Piper).
    pub url: String,
    /// Kokoro voice id, or a blend such as `bm_george+bm_lewis`
    /// (ignored by Piper, whose voice is chosen when its server starts).
    pub voice: String,
    /// Kokoro model name.
    pub model: String,
    /// Base speaking rate (Kokoro). Urgent calls are delivered a little quicker.
    pub speed: f32,
    pub timeout_ms: u64,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            engine: TtsKind::Kokoro,
            url: "http://127.0.0.1:8880".into(),
            // Calm, low British male: the classic pit-wall voice.
            voice: "bm_george".into(),
            model: "kokoro".into(),
            speed: 1.0,
            // Kokoro on CPU: the first lines after start can take several seconds.
            timeout_ms: 20_000,
        }
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

    pub fn engine_name(&self) -> &'static str {
        match self.cfg.engine {
            TtsKind::Piper => "piper",
            TtsKind::Kokoro => "kokoro",
            TtsKind::None => "none",
        }
    }

    /// Identifies engine + voice so cached audio is never reused across voices.
    pub fn cache_namespace(&self, speed: f32) -> String {
        match self.cfg.engine {
            TtsKind::Piper => format!("piper|{}", self.cfg.url),
            TtsKind::Kokoro => format!("kokoro|{}|{}|{:.2}", self.cfg.model, self.cfg.voice, self.effective_speed(speed)),
            TtsKind::None => "none".into(),
        }
    }

    fn effective_speed(&self, factor: f32) -> f32 {
        (self.cfg.speed * factor).clamp(0.5, 2.0)
    }

    /// Is the sidecar reachable? (Any HTTP response counts.)
    pub async fn health(&self) -> bool {
        if !self.enabled() {
            return false;
        }
        self.http.get(&self.cfg.url).timeout(Duration::from_secs(5)).send().await.is_ok()
    }

    pub async fn synthesize(&self, text: &str) -> anyhow::Result<Pcm> {
        self.synthesize_with(text, 1.0).await
    }

    /// Synthesize with a speed factor relative to the configured base speed.
    pub async fn synthesize_with(&self, text: &str, speed_factor: f32) -> anyhow::Result<Pcm> {
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
                        "speed": self.effective_speed(speed_factor),
                        "response_format": "wav",
                    }))
                    .send()
                    .await?
            }
        };
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("TTS HTTP {status}: {}", body.chars().take(200).collect::<String>());
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

    #[tokio::test]
    async fn unreachable_sidecar_is_an_error_not_a_panic() {
        let c = TtsClient::new(TtsConfig { url: "http://127.0.0.1:9".into(), timeout_ms: 500, ..Default::default() }).unwrap();
        assert!(c.synthesize("hello").await.is_err());
        assert!(!c.health().await);
        let off = TtsClient::new(TtsConfig { engine: TtsKind::None, ..Default::default() }).unwrap();
        assert!(off.synthesize("x").await.is_err());
    }

    #[test]
    fn cache_namespace_includes_voice_and_speed() {
        let c = TtsClient::new(TtsConfig::default()).unwrap();
        assert_ne!(c.cache_namespace(1.0), c.cache_namespace(1.1));
        assert!(c.cache_namespace(1.0).contains("bm_george"));
    }

    #[tokio::test]
    async fn kokoro_request_shape() {
        use axum::{routing::post, Json, Router};
        let app = Router::new().route(
            "/v1/audio/speech",
            post(|Json(v): Json<serde_json::Value>| async move {
                assert_eq!(v["voice"], "bm_george");
                assert_eq!(v["response_format"], "wav");
                assert!((v["speed"].as_f64().unwrap() - 1.1).abs() < 1e-6);
                Pcm { samples: vec![0.1; 240], sample_rate: 24000 }.to_wav().unwrap()
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        let c = TtsClient::new(TtsConfig { url, ..Default::default() }).unwrap();
        let pcm = c.synthesize_with("Box box", 1.1).await.unwrap();
        assert_eq!(pcm.sample_rate, 24000);
        assert_eq!(pcm.samples.len(), 240);
    }
}

#[cfg(test)]
mod live {
    /// `KOKORO_URL=http://127.0.0.1:8880 cargo test -p ai-sidecar kokoro_live -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn kokoro_live() {
        let url = std::env::var("KOKORO_URL").expect("set KOKORO_URL");
        let c = super::TtsClient::new(super::TtsConfig { url, ..Default::default() }).unwrap();
        let t0 = std::time::Instant::now();
        let p = c.synthesize_with("Box box, box box. Good stop, mediums fitted.", 1.08).await.unwrap();
        println!("kokoro: {} ms audio at {} Hz in {} ms", p.duration_ms(), p.sample_rate, t0.elapsed().as_millis());
        assert!(p.duration_ms() > 1000);
    }
}
