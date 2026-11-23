//! Driver voice input (M5): push-to-talk → microphone → speech-to-text →
//! answer (telemetry first, LLM second) → P1 radio reply.
//!
//! PTT comes from a wheel button mapped to "UDP Action N" (the game's BUTN
//! event) or from the UI (`ptt.set`). Typed questions (`driver.ask`) skip the
//! microphone and go straight to the answer pipeline.

use crate::actors::{now_ms, CommsCmd};
use crate::config::{PttMode, VoiceConfig};
use ai_sidecar::{LlmClient, Pcm, SttClient};
use comms::answers;
use race_state::snapshot::{RadioSource, TelemetrySnapshot};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use voice_comms::{AudioPlayer, MicCapture};

#[derive(Debug)]
pub enum VoiceCmd {
    /// Raw wheel-button bit field from a BUTN event.
    Buttons(u32),
    /// UI push-to-talk (held = true).
    Ptt(bool),
    /// A typed question.
    Ask(String),
}

/// Status of the voice pipeline, shown in the comms state.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VoiceStatus {
    pub stt: bool,
    pub llm: bool,
    pub mic: bool,
}

/// Decides when a PTT transmission starts and stops.
#[derive(Debug)]
pub struct PttState {
    mode: PttMode,
    bit: u32,
    last_pressed: bool,
    active: bool,
}

#[derive(Debug, PartialEq)]
pub enum PttEdge {
    Start,
    Stop,
}

impl PttState {
    pub fn new(mode: PttMode, button: u8) -> Self {
        Self { mode, bit: f1_udp::packets::udp_action_bit(button), last_pressed: false, active: false }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// Feed a BUTN bit field.
    pub fn on_buttons(&mut self, status: u32) -> Option<PttEdge> {
        let pressed = status & self.bit != 0;
        let rising = pressed && !self.last_pressed;
        self.last_pressed = pressed;
        match self.mode {
            PttMode::Hold => self.set(pressed),
            PttMode::Toggle if rising => self.set(!self.active),
            PttMode::Toggle => None,
        }
    }

    /// Direct set (UI button, or the stuck-PTT timeout).
    pub fn set(&mut self, on: bool) -> Option<PttEdge> {
        if on == self.active {
            return None;
        }
        self.active = on;
        Some(if on { PttEdge::Start } else { PttEdge::Stop })
    }
}

pub struct VoiceActor {
    pub cfg: VoiceConfig,
    pub rx: mpsc::Receiver<VoiceCmd>,
    pub mic: Option<MicCapture>,
    pub stt: SttClient,
    pub llm: LlmClient,
    pub player: Option<Arc<AudioPlayer>>,
    pub snapshot: watch::Receiver<Arc<TelemetrySnapshot>>,
    pub comms: mpsc::Sender<CommsCmd>,
}

impl VoiceActor {
    pub async fn run(mut self) {
        let mut ptt = PttState::new(self.cfg.ptt_mode.clone(), self.cfg.ptt_button);
        let mut started_ms = 0u64;
        let mut check = tokio::time::interval(Duration::from_millis(250));
        let mut last_health = 0u64;
        let mut status = VoiceStatus::default();
        if self.llm.enabled() {
            let llm = self.llm.clone();
            tokio::spawn(async move {
                match llm.warm_up().await {
                    Ok(()) => tracing::info!("LLM '{}' loaded", llm.model()),
                    Err(e) => {
                        let have = llm.installed_models().await;
                        tracing::warn!("LLM warm-up failed: {e:#}. Installed models: {}", if have.is_empty() { "none".into() } else { have.join(", ") });
                    }
                }
            });
        }
        loop {
            tokio::select! {
                cmd = self.rx.recv() => {
                    let Some(cmd) = cmd else { break };
                    let edge = match cmd {
                        VoiceCmd::Buttons(b) if self.cfg.enabled => ptt.on_buttons(b),
                        VoiceCmd::Ptt(on) if self.cfg.enabled => ptt.set(on),
                        VoiceCmd::Buttons(_) | VoiceCmd::Ptt(_) => None,
                        VoiceCmd::Ask(text) => {
                            self.spawn_answer(text, status);
                            None
                        }
                    };
                    match edge {
                        Some(PttEdge::Start) => {
                            started_ms = now_ms();
                            self.start_transmit().await;
                        }
                        Some(PttEdge::Stop) => self.end_transmit(now_ms().saturating_sub(started_ms), status).await,
                        None => {}
                    }
                }
                _ = check.tick() => {
                    let now = now_ms();
                    if ptt.active() && now.saturating_sub(started_ms) >= self.cfg.max_ptt_ms {
                        tracing::warn!("push-to-talk held for {} s (lost release?), closing the channel", self.cfg.max_ptt_ms / 1000);
                        ptt.set(false);
                        self.end_transmit(now - started_ms, status).await;
                    }
                    if now.saturating_sub(last_health) >= 15_000 {
                        last_health = now;
                        let (stt, llm) = tokio::join!(self.stt.health(), self.llm.health());
                        let next = VoiceStatus { stt, llm, mic: self.mic.as_ref().is_some_and(|m| m.is_alive()) };
                        if next != status {
                            tracing::info!("voice input: mic {}, speech-to-text {}, LLM {}", ok(next.mic), ok(next.stt), ok(next.llm));
                            status = next;
                            let _ = self.comms.send(CommsCmd::VoiceStatus(status)).await;
                        }
                    }
                }
            }
        }
    }

    async fn start_transmit(&self) {
        let _ = self.comms.send(CommsCmd::PttActive(true)).await;
        if let Some(p) = &self.player {
            // Half-duplex radio: the driver keying up cuts the engineer off.
            p.stop();
            let _ = p.play(&voice_comms::chirp(p.sample_rate()), true);
        }
        match &self.mic {
            Some(m) => m.begin(),
            None => tracing::warn!("push-to-talk pressed but no microphone is open"),
        }
    }

    async fn end_transmit(&self, held_ms: u64, status: VoiceStatus) {
        let _ = self.comms.send(CommsCmd::PttActive(false)).await;
        let Some(mic) = &self.mic else { return };
        let clip = mic.end();
        if held_ms < self.cfg.min_ptt_ms || clip.duration_ms() < self.cfg.min_ptt_ms {
            tracing::debug!("ignoring {held_ms} ms PTT blip");
            return;
        }
        if clip.rms() < 0.002 {
            tracing::info!("PTT clip is silent (check the microphone / input device)");
            return;
        }
        if !self.stt.enabled() {
            tracing::warn!("PTT clip recorded but speech-to-text is disabled ([stt] engine)");
            return;
        }
        let (stt, comms, llm, snap) = (self.stt.clone(), self.comms.clone(), self.llm.clone(), self.snapshot.clone());
        tokio::spawn(async move {
            let _ = comms.send(CommsCmd::Thinking(true)).await;
            let t0 = now_ms();
            let text = transcribe(&stt, &clip).await;
            tracing::info!("driver ({} ms clip, {} ms STT): {:?}", clip.duration_ms(), now_ms() - t0, text);
            match text {
                Some(t) if !t.is_empty() => answer(t, &comms, &llm, &snap, status).await,
                Some(_) => {
                    let _ = comms.send(CommsCmd::DriverAnswer { text: answers::SAY_AGAIN.into(), source: RadioSource::Rule }).await;
                }
                None => {}
            }
            let _ = comms.send(CommsCmd::Thinking(false)).await;
        });
    }

    fn spawn_answer(&self, text: String, status: VoiceStatus) {
        let (comms, llm, snap) = (self.comms.clone(), self.llm.clone(), self.snapshot.clone());
        tokio::spawn(async move {
            let _ = comms.send(CommsCmd::Thinking(true)).await;
            answer(text, &comms, &llm, &snap, status).await;
            let _ = comms.send(CommsCmd::Thinking(false)).await;
        });
    }
}

fn ok(v: bool) -> &'static str {
    if v {
        "ready"
    } else {
        "offline"
    }
}

async fn transcribe(stt: &SttClient, clip: &Pcm) -> Option<String> {
    match stt.transcribe(clip).await {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!("speech-to-text failed: {e:#}");
            None
        }
    }
}

/// Log the driver's words, then answer from telemetry or the LLM.
async fn answer(question: String, comms: &mpsc::Sender<CommsCmd>, llm: &LlmClient, snap: &watch::Receiver<Arc<TelemetrySnapshot>>, status: VoiceStatus) {
    let question = question.trim().to_string();
    if question.is_empty() {
        return;
    }
    let _ = comms.send(CommsCmd::DriverSaid(question.clone())).await;
    let s = snap.borrow().clone();
    if let Some(a) = answers::answer(&question, &s, status.llm) {
        let _ = comms.send(CommsCmd::DriverAnswer { text: a, source: RadioSource::Rule }).await;
        return;
    }
    if !llm.enabled() || !status.llm {
        let _ = comms.send(CommsCmd::DriverAnswer { text: "Copy. I don't have an answer for that one right now.".into(), source: RadioSource::Rule }).await;
        return;
    }
    let _ = comms.send(CommsCmd::DriverAnswer { text: answers::ACK_THINKING.into(), source: RadioSource::Rule }).await;
    let t0 = now_ms();
    let reply = llm.chat(answers::ENGINEER_PROMPT, &answers::llm_user_message(&question, &s)).await;
    match reply.map(|r| answers::polish_reply(&r)) {
        Ok(Some(text)) => {
            tracing::info!("LLM answer in {} ms: {text}", now_ms() - t0);
            let _ = comms.send(CommsCmd::DriverAnswer { text, source: RadioSource::Llm }).await;
        }
        Ok(None) => {
            let _ = comms.send(CommsCmd::DriverAnswer { text: answers::SAY_AGAIN.into(), source: RadioSource::Rule }).await;
        }
        Err(e) => {
            tracing::warn!("LLM failed: {e:#}");
            let _ = comms.send(CommsCmd::DriverAnswer { text: "Copy, we'll come back to you on that.".into(), source: RadioSource::Rule }).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_mode_follows_the_button() {
        let bit = f1_udp::packets::udp_action_bit(3);
        let mut p = PttState::new(PttMode::Hold, 3);
        assert_eq!(p.on_buttons(0x1), None, "other buttons are ignored");
        assert_eq!(p.on_buttons(bit | 0x1), Some(PttEdge::Start));
        assert_eq!(p.on_buttons(bit), None, "still held");
        assert_eq!(p.on_buttons(0), Some(PttEdge::Stop));
    }

    #[test]
    fn toggle_mode_flips_on_presses() {
        let bit = f1_udp::packets::udp_action_bit(1);
        let mut p = PttState::new(PttMode::Toggle, 1);
        assert_eq!(p.on_buttons(bit), Some(PttEdge::Start));
        assert_eq!(p.on_buttons(0), None, "release does nothing");
        assert_eq!(p.on_buttons(bit), Some(PttEdge::Stop));
        assert_eq!(p.on_buttons(0), None);
        // Timeout path.
        assert_eq!(p.on_buttons(bit), Some(PttEdge::Start));
        assert_eq!(p.set(false), Some(PttEdge::Stop));
        assert!(!p.active());
    }
}
