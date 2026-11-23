//! Speech-to-text for the driver's push-to-talk clips.
//!
//! * **whisper.cpp** `whisper-server` (default): `POST /inference` (multipart
//!   `file`, `response_format=json`, `temperature`, optional `prompt`).
//! * **OpenAI-compatible** (`speaches`, `faster-whisper-server`, ...):
//!   `POST /v1/audio/transcriptions` (multipart `file`, `model`, `language`).
//!
//! Audio is sent as 16 kHz 16-bit mono WAV.

use crate::Pcm;
use anyhow::{bail, Context};
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SttKind {
    #[default]
    Whispercpp,
    Openai,
    None,
}

/// Vocabulary hint so whisper hears racing terms ("inters", "box", "DRS").
pub const RADIO_PROMPT: &str = "Team radio to the race engineer. Box, box. Tyres, softs, mediums, hards, inters, wets. \
Gap ahead, gap behind, delta, DRS, ERS, battery, fuel, lift and coast, undercut, overcut, safety car, VSC, front wing, pit window, deg.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SttConfig {
    pub engine: SttKind,
    /// `http://127.0.0.1:8178` (whisper.cpp) or e.g. `http://127.0.0.1:8000` (OpenAI-compatible).
    pub url: String,
    /// Model name for OpenAI-compatible servers (whisper.cpp picks its model at start).
    pub model: String,
    pub language: String,
    pub timeout_ms: u64,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self { engine: SttKind::Whispercpp, url: "http://127.0.0.1:8178".into(), model: "Systran/faster-whisper-base.en".into(), language: "en".into(), timeout_ms: 8000 }
    }
}

#[derive(Clone)]
pub struct SttClient {
    cfg: SttConfig,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct TextResponse {
    text: String,
}

impl SttClient {
    pub fn new(cfg: SttConfig) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder().timeout(Duration::from_millis(cfg.timeout_ms.max(1000))).build()?;
        Ok(Self { cfg, http })
    }

    pub fn enabled(&self) -> bool {
        self.cfg.engine != SttKind::None
    }

    pub async fn health(&self) -> bool {
        self.enabled() && self.http.get(&self.cfg.url).timeout(Duration::from_secs(2)).send().await.is_ok()
    }

    /// Transcribe a clip. Returns the cleaned transcript (may be empty).
    pub async fn transcribe(&self, pcm: &Pcm) -> anyhow::Result<String> {
        let wav = pcm.resampled(16_000).to_wav_i16()?;
        let base = self.cfg.url.trim_end_matches('/');
        let file = || Part::bytes(wav.clone()).file_name("ptt.wav").mime_str("audio/wav");
        let req = match self.cfg.engine {
            SttKind::None => bail!("STT disabled"),
            SttKind::Whispercpp => self.http.post(format!("{base}/inference")).multipart(
                Form::new()
                    .part("file", file()?)
                    .text("response_format", "json")
                    .text("temperature", "0.0")
                    .text("language", self.cfg.language.clone())
                    .text("prompt", RADIO_PROMPT),
            ),
            SttKind::Openai => self.http.post(format!("{base}/v1/audio/transcriptions")).multipart(
                Form::new()
                    .part("file", file()?)
                    .text("model", self.cfg.model.clone())
                    .text("language", self.cfg.language.clone())
                    .text("response_format", "json")
                    .text("prompt", RADIO_PROMPT),
            ),
        };
        let resp = req.send().await.context("STT request")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("STT HTTP {status}: {}", body.chars().take(200).collect::<String>());
        }
        let r: TextResponse = resp.json().await.context("STT response")?;
        Ok(clean_transcript(&r.text))
    }
}

/// Strip whisper artefacts: bracketed noise tags, music notes, the echoed
/// prompt and repeated whitespace.
pub fn clean_transcript(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut depth = 0i32;
    for c in raw.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            '♪' | '*' => {}
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let t = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.trim_matches(|c: char| c == '-' || c == '"' || c.is_whitespace()).to_string();
    // Whisper sometimes hallucinates these on silence.
    const SILENCE: [&str; 5] = ["thank you.", "thanks for watching!", "you", "bye.", "team radio to the race engineer."];
    if SILENCE.contains(&t.to_lowercase().as_str()) {
        return String::new();
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_whisper_artefacts() {
        assert_eq!(clean_transcript(" [BLANK_AUDIO] "), "");
        assert_eq!(clean_transcript("  How are my  tyres? (engine noise)\n"), "How are my tyres?");
        assert_eq!(clean_transcript("Thank you."), "");
        assert_eq!(clean_transcript("♪ gap behind ♪"), "gap behind");
    }

    #[tokio::test]
    async fn whispercpp_multipart_round_trip() {
        use axum::{extract::Multipart, routing::post, Json, Router};
        let app = Router::new().route(
            "/inference",
            post(|mut mp: Multipart| async move {
                let mut got_file = 0usize;
                let mut fmt = String::new();
                while let Some(f) = mp.next_field().await.unwrap() {
                    match f.name().unwrap_or("") {
                        "file" => got_file = f.bytes().await.unwrap().len(),
                        "response_format" => fmt = f.text().await.unwrap(),
                        _ => {}
                    }
                }
                assert!(got_file > 44, "wav body");
                assert_eq!(fmt, "json");
                Json(serde_json::json!({ "text": " What's the gap behind?\n" }))
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        let c = SttClient::new(SttConfig { url, ..Default::default() }).unwrap();
        let pcm = Pcm { samples: vec![0.05; 48_000], sample_rate: 48_000 };
        assert_eq!(c.transcribe(&pcm).await.unwrap(), "What's the gap behind?");
    }

    #[tokio::test]
    async fn unreachable_is_error() {
        let c = SttClient::new(SttConfig { url: "http://127.0.0.1:9".into(), timeout_ms: 500, ..Default::default() }).unwrap();
        assert!(c.transcribe(&Pcm { samples: vec![0.0; 1600], sample_rate: 16000 }).await.is_err());
        assert!(!c.health().await);
    }
}
