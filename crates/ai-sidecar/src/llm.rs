//! Small local LLM for free-form driver questions.
//!
//! * **Ollama** (default): `POST /api/chat` with `stream: false` and a long
//!   `keep_alive` so the model stays loaded between questions.
//! * **OpenAI-compatible** (llama.cpp server, LM Studio, vLLM ...):
//!   `POST /v1/chat/completions`.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LlmKind {
    #[default]
    Ollama,
    Openai,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub engine: LlmKind,
    /// `http://127.0.0.1:11434` (Ollama) or an OpenAI-compatible base URL.
    pub url: String,
    pub model: String,
    /// Bearer token for OpenAI-compatible servers that want one.
    pub api_key: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub timeout_ms: u64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            engine: LlmKind::Ollama,
            url: "http://127.0.0.1:11434".into(),
            model: "qwen2.5:3b".into(),
            api_key: String::new(),
            max_tokens: 90,
            temperature: 0.3,
            // Cold load or CPU fallback (GPU busy with the game) can be slow.
            timeout_ms: 30_000,
        }
    }
}

#[derive(Clone)]
pub struct LlmClient {
    cfg: LlmConfig,
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new(cfg: LlmConfig) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder().timeout(Duration::from_millis(cfg.timeout_ms.max(1000))).build()?;
        Ok(Self { cfg, http })
    }

    pub fn enabled(&self) -> bool {
        self.cfg.engine != LlmKind::None
    }

    pub fn model(&self) -> &str {
        &self.cfg.model
    }

    pub async fn health(&self) -> bool {
        if !self.enabled() {
            return false;
        }
        let base = self.cfg.url.trim_end_matches('/');
        match self.cfg.engine {
            // Reachable is not enough: the configured model must be pulled.
            LlmKind::Ollama => {
                let Ok(r) = self.http.get(format!("{base}/api/tags")).timeout(Duration::from_secs(2)).send().await else { return false };
                let Ok(v) = r.json::<serde_json::Value>().await else { return false };
                let want = self.cfg.model.trim_end_matches(":latest");
                v["models"].as_array().is_some_and(|ms| ms.iter().filter_map(|m| m["name"].as_str()).any(|n| n.trim_end_matches(":latest") == want))
            }
            _ => matches!(self.http.get(format!("{base}/v1/models")).timeout(Duration::from_secs(2)).send().await, Ok(r) if r.status().is_success()),
        }
    }

    /// Models installed in Ollama (for helpful log messages).
    pub async fn installed_models(&self) -> Vec<String> {
        if self.cfg.engine != LlmKind::Ollama {
            return vec![];
        }
        let base = self.cfg.url.trim_end_matches('/');
        let Ok(r) = self.http.get(format!("{base}/api/tags")).timeout(Duration::from_secs(2)).send().await else { return vec![] };
        let v: serde_json::Value = r.json().await.unwrap_or_default();
        v["models"].as_array().map(|ms| ms.iter().filter_map(|m| m["name"].as_str().map(String::from)).collect()).unwrap_or_default()
    }

    /// Load the model into memory ahead of the first question (Ollama).
    pub async fn warm_up(&self) -> anyhow::Result<()> {
        if self.cfg.engine == LlmKind::Ollama {
            let base = self.cfg.url.trim_end_matches('/');
            let r = self
                .http
                .post(format!("{base}/api/generate"))
                .timeout(Duration::from_secs(120))
                .json(&serde_json::json!({ "model": self.cfg.model, "prompt": "", "keep_alive": "60m" }))
                .send()
                .await?;
            if !r.status().is_success() {
                bail!("Ollama warm-up HTTP {} (is the model pulled? `ollama pull {}`)", r.status(), self.cfg.model);
            }
        }
        Ok(())
    }

    /// One-shot chat: system prompt + user message → reply text.
    pub async fn chat(&self, system: &str, user: &str) -> anyhow::Result<String> {
        let base = self.cfg.url.trim_end_matches('/');
        let messages = serde_json::json!([
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ]);
        let text = match self.cfg.engine {
            LlmKind::None => bail!("LLM disabled"),
            LlmKind::Ollama => {
                let r = self
                    .http
                    .post(format!("{base}/api/chat"))
                    .json(&serde_json::json!({
                        "model": self.cfg.model,
                        "messages": messages,
                        "stream": false,
                        "keep_alive": "60m",
                        "options": { "temperature": self.cfg.temperature, "num_predict": self.cfg.max_tokens, "num_ctx": 2048 },
                    }))
                    .send()
                    .await
                    .context("LLM request")?;
                let status = r.status();
                if !status.is_success() {
                    bail!("LLM HTTP {status}: {}", r.text().await.unwrap_or_default().chars().take(200).collect::<String>());
                }
                let v: serde_json::Value = r.json().await?;
                v["message"]["content"].as_str().unwrap_or_default().to_string()
            }
            LlmKind::Openai => {
                let mut req = self.http.post(format!("{base}/v1/chat/completions")).json(&serde_json::json!({
                    "model": self.cfg.model,
                    "messages": messages,
                    "max_tokens": self.cfg.max_tokens,
                    "temperature": self.cfg.temperature,
                }));
                if !self.cfg.api_key.is_empty() {
                    req = req.bearer_auth(&self.cfg.api_key);
                }
                let r = req.send().await.context("LLM request")?;
                let status = r.status();
                if !status.is_success() {
                    bail!("LLM HTTP {status}: {}", r.text().await.unwrap_or_default().chars().take(200).collect::<String>());
                }
                let v: serde_json::Value = r.json().await?;
                v["choices"][0]["message"]["content"].as_str().unwrap_or_default().to_string()
            }
        };
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ollama_and_openai_shapes() {
        use axum::{routing::post, Json, Router};
        let app = Router::new()
            .route("/api/tags", axum::routing::get(|| async { Json(serde_json::json!({ "models": [{ "name": "qwen2.5:3b" }] })) }))
            .route(
                "/api/chat",
                post(|Json(v): Json<serde_json::Value>| async move {
                    assert_eq!(v["stream"], false);
                    assert_eq!(v["messages"][0]["role"], "system");
                    Json(serde_json::json!({ "message": { "role": "assistant", "content": "Gap is 1.2, keep pushing." } }))
                }),
            )
            .route(
                "/v1/chat/completions",
                post(|Json(v): Json<serde_json::Value>| async move {
                    assert_eq!(v["messages"][1]["content"], "q");
                    Json(serde_json::json!({ "choices": [{ "message": { "content": "Copy." } }] }))
                }),
            );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        let o = LlmClient::new(LlmConfig { url: url.clone(), ..Default::default() }).unwrap();
        assert_eq!(o.chat("sys", "q").await.unwrap(), "Gap is 1.2, keep pushing.");
        assert!(o.health().await, "configured model is installed");
        let missing = LlmClient::new(LlmConfig { url: url.clone(), model: "llama3.1:8b".into(), ..Default::default() }).unwrap();
        assert!(!missing.health().await, "model not pulled = not available");
        let oa = LlmClient::new(LlmConfig { engine: LlmKind::Openai, url, ..Default::default() }).unwrap();
        assert_eq!(oa.chat("sys", "q").await.unwrap(), "Copy.");
    }
}
