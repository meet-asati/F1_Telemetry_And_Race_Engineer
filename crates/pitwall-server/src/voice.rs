//! Driver voice input (M5): push-to-talk → microphone → speech-to-text →
//! answer (telemetry first, LLM second) → P1 radio reply.
//!
//! PTT comes from a wheel button mapped to "UDP Action N" (the game's BUTN
//! event) or from the UI (`ptt.set`). Typed questions (`driver.ask`) skip the
//! microphone and go straight to the answer pipeline.

use crate::actors::{now_ms, CommsCmd};
use crate::config::{PttMode, VoiceConfig};
use ai_sidecar::{LlmClient, Pcm, SttClient};
use comms::{answers, Persona};
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
    /// Switch microphone (name substring; empty = auto-pick).
    SetMic(String),
}

/// Where the chosen microphone is remembered between runs.
pub const VOICE_SETTINGS: &str = "data/voice-settings.json";

pub fn load_saved_mic() -> Option<String> {
    let text = std::fs::read_to_string(VOICE_SETTINGS).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v["inputDevice"].as_str().map(str::to_string).filter(|s| !s.is_empty())
}

fn save_mic(name: &str) {
    if let Some(dir) = std::path::Path::new(VOICE_SETTINGS).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(VOICE_SETTINGS, serde_json::json!({ "inputDevice": name }).to_string()) {
        tracing::warn!("could not save microphone choice: {e}");
    }
}

/// Start the local whisper.cpp server (`[stt] autostart`) when it is not
/// already reachable. The child is killed when the returned handle drops.
pub async fn autostart_whisper(stt: &SttClient) -> Option<tokio::process::Child> {
    let cfg = stt.config();
    if !cfg.autostart || cfg.engine != ai_sidecar::SttKind::Whispercpp || stt.health().await {
        return None;
    }
    let exe = std::path::Path::new(&cfg.server_exe);
    let model = std::path::Path::new(&cfg.server_model);
    if !exe.exists() || !model.exists() {
        tracing::warn!(
            "speech-to-text offline and whisper.cpp not installed at {} / {} (see README: Hearing you)",
            exe.display(),
            model.display()
        );
        return None;
    }
    let url = cfg.url.trim_end_matches('/');
    let hostport = url.rsplit("://").next().unwrap_or(url);
    let (host, port) = hostport.rsplit_once(':').unwrap_or((hostport, "8178"));
    let _ = std::fs::create_dir_all("data");
    let log = std::fs::File::create("data/whisper-server.log").ok();
    let mut cmd = tokio::process::Command::new(exe);
    cmd.arg("-m")
        .arg(model)
        .args(["--host", host, "--port", port, "-t", &cfg.server_threads.to_string(), "-l", &cfg.language])
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    match log.as_ref().and_then(|f| Some((f.try_clone().ok()?, f.try_clone().ok()?))) {
        Some((out, err)) => {
            cmd.stdout(out).stderr(err);
        }
        None => {
            cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        }
    }
    #[cfg(windows)]
    {
        // No console window for the child.
        cmd.creation_flags(0x0800_0000);
    }
    match cmd.spawn() {
        Ok(child) => {
            tracing::info!("started whisper.cpp speech-to-text on {host}:{port} (log: data/whisper-server.log)");
            Some(child)
        }
        Err(e) => {
            tracing::warn!("could not start whisper.cpp ({}): {e}", exe.display());
            None
        }
    }
}

/// Lowest "UDP Action N" (1-12) pressed in a BUTN bit field.
pub fn udp_action_pressed(status: u32) -> Option<u8> {
    (1..=12u8).find(|&n| status & f1_udp::packets::udp_action_bit(n) != 0)
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
    /// Auto mode: when the current press started, and whether a short tap
    /// latched the channel open.
    press_ms: u64,
    latched: bool,
}

/// Auto mode: a press shorter than this is a tap (latch), longer is hold-to-talk.
pub const TAP_MS: u64 = 450;

#[derive(Debug, PartialEq)]
pub enum PttEdge {
    Start,
    Stop,
}

impl PttState {
    pub fn new(mode: PttMode, button: u8) -> Self {
        Self { mode, bit: f1_udp::packets::udp_action_bit(button), last_pressed: false, active: false, press_ms: 0, latched: false }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// Feed a BUTN bit field (`now` in ms).
    pub fn on_buttons(&mut self, status: u32, now: u64) -> Option<PttEdge> {
        let pressed = status & self.bit != 0;
        let rising = pressed && !self.last_pressed;
        let falling = !pressed && self.last_pressed;
        self.last_pressed = pressed;
        match self.mode {
            PttMode::Hold => self.set(pressed),
            PttMode::Toggle if rising => self.set(!self.active),
            PttMode::Toggle => None,
            PttMode::Auto => {
                if rising {
                    if self.active && self.latched {
                        // Second tap: send.
                        return self.set(false);
                    }
                    self.press_ms = now;
                    return self.set(true);
                }
                if falling && self.active && !self.latched {
                    if now.saturating_sub(self.press_ms) < TAP_MS {
                        // Short tap: keep the channel open until the next tap.
                        self.latched = true;
                        return None;
                    }
                    return self.set(false);
                }
                None
            }
        }
    }

    /// Latched open by a tap (Auto mode)?
    pub fn latched(&self) -> bool {
        self.active && self.latched
    }

    /// Direct set (UI button, or the stuck-PTT timeout).
    pub fn set(&mut self, on: bool) -> Option<PttEdge> {
        if on == self.active {
            return None;
        }
        self.active = on;
        if !on {
            self.latched = false;
        }
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
    pub persona: Persona,
    /// Team-radio bus (the driver's transmissions go to listening browsers).
    pub bus: crate::duo::Bus,
    pub fx: voice_comms::RadioFx,
}

impl VoiceActor {
    pub async fn run(mut self) {
        let mut ptt = PttState::new(self.cfg.ptt_mode.clone(), self.cfg.ptt_button);
        let mut started_ms = 0u64;
        let mut check = tokio::time::interval(Duration::from_millis(250));
        let mut last_health = 0u64;
        let mut status = VoiceStatus::default();
        if let Some(m) = &self.mic {
            crate::duo::attach_driver_mic(m, self.bus.clone(), self.fx.clone());
        }
        let mut last_level = -1.0f32;
        let mut last_blocked: Option<bool> = None;
        let mut level_tick = tokio::time::interval(Duration::from_millis(100));
        let _ = self.comms.send(CommsCmd::MicDevice(self.mic.as_ref().map(|m| m.device_name().to_string()))).await;
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
                    if let VoiceCmd::Buttons(b) = &cmd {
                        if let Some(n) = udp_action_pressed(*b) {
                            if n == self.cfg.ptt_button {
                                tracing::info!("wheel button: UDP Action {n} (push-to-talk)");
                            } else {
                                tracing::info!("wheel button: UDP Action {n} pressed, but push-to-talk is UDP Action {} ([voice] ptt_button)", self.cfg.ptt_button);
                            }
                            let _ = self.comms.send(CommsCmd::UdpAction(n)).await;
                        }
                    }
                    let edge = match cmd {
                        VoiceCmd::SetMic(name) => {
                            self.switch_mic(name).await;
                            last_health = 0;
                            None
                        }
                        VoiceCmd::Buttons(b) if self.cfg.enabled => {
                            let edge = ptt.on_buttons(b, now_ms());
                            if edge.is_none() && ptt.latched() {
                                tracing::info!("push-to-talk latched open (tap again to send)");
                            }
                            edge
                        }
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
                _ = level_tick.tick() => {
                    let level = self.mic.as_ref().map(|m| m.level()).unwrap_or(0.0);
                    if (level - last_level).abs() > 0.002 {
                        last_level = level;
                        let _ = self.comms.send(CommsCmd::MicLevel(level)).await;
                    }
                    let blocked = self.mic.as_ref().and_then(|m| m.digital_silence());
                    if blocked.is_some() && blocked != last_blocked {
                        last_blocked = blocked;
                        if blocked == Some(true) {
                            tracing::warn!(
                                "microphone '{}' delivers pure silence. Windows may be blocking it: Settings > Privacy & security > Microphone > \"Let desktop apps access your microphone\" must be On",
                                self.mic.as_ref().map(|m| m.device_name()).unwrap_or("?")
                            );
                        }
                        let _ = self.comms.send(CommsCmd::MicBlocked(blocked == Some(true))).await;
                    }
                }
                _ = check.tick() => {
                    let now = now_ms();
                    if ptt.active() && now.saturating_sub(started_ms) >= self.cfg.max_ptt_ms {
                        tracing::warn!("push-to-talk held for {} s (lost release?), closing the channel", self.cfg.max_ptt_ms / 1000);
                        ptt.set(false);
                        self.end_transmit(now - started_ms, status).await;
                    }
                    // Re-check quickly while something is down (e.g. whisper still starting).
                    let every = if status.stt && status.llm && status.mic { 15_000 } else { 4_000 };
                    if now.saturating_sub(last_health) >= every {
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

    /// Tell the driver, on the radio, why we could not hear them.
    async fn radio_problem(&self, text: &str) {
        let _ = self.comms.send(CommsCmd::DriverAnswer { text: text.into(), source: RadioSource::Rule }).await;
    }

    async fn switch_mic(&mut self, name: String) {
        let name = name.trim().to_string();
        self.mic = None; // closes the old stream
        let want = (!name.is_empty()).then(|| name.clone());
        match tokio::task::spawn_blocking(move || MicCapture::start(want.as_deref())).await {
            Ok(Ok(m)) => {
                tracing::info!("microphone switched to '{}'", m.device_name());
                crate::duo::attach_driver_mic(&m, self.bus.clone(), self.fx.clone());
                let _ = self.comms.send(CommsCmd::MicBlocked(false)).await;
                save_mic(&name);
                let _ = self.comms.send(CommsCmd::MicDevice(Some(m.device_name().to_string()))).await;
                self.mic = Some(m);
            }
            Ok(Err(e)) => {
                tracing::warn!("could not open microphone '{name}': {e:#}");
                let _ = self.comms.send(CommsCmd::MicDevice(None)).await;
            }
            Err(e) => tracing::warn!("microphone task failed: {e}"),
        }
    }

    async fn end_transmit(&self, held_ms: u64, status: VoiceStatus) {
        let _ = self.comms.send(CommsCmd::PttActive(false)).await;
        let Some(mic) = &self.mic else {
            self.radio_problem("No microphone on the pit wall, we can't hear you. Pick one in the AI Engineer monitor.").await;
            return;
        };
        let clip = mic.end();
        tracing::info!("push-to-talk released: {} ms held, {} ms recorded, level {:.4}", held_ms, clip.duration_ms(), clip.rms());
        if held_ms < self.cfg.min_ptt_ms || clip.duration_ms() < self.cfg.min_ptt_ms {
            tracing::info!("ignoring {held_ms} ms push-to-talk blip (hold the button while you talk)");
            return;
        }
        if mic.digital_silence() == Some(true) {
            self.radio_problem("We're getting nothing on your radio. Windows is blocking the microphone, check the privacy settings.").await;
            return;
        }
        if clip.rms() < 0.002 {
            tracing::info!("push-to-talk clip is silent on '{}'", mic.device_name());
            self.radio_problem("We're getting nothing on your radio. Check the microphone.").await;
            return;
        }
        if !self.stt.enabled() {
            tracing::warn!("PTT clip recorded but speech-to-text is disabled ([stt] engine)");
            self.radio_problem("Copy your transmission, but speech-to-text is switched off.").await;
            return;
        }
        let (stt, comms, llm, snap, persona) = (self.stt.clone(), self.comms.clone(), self.llm.clone(), self.snapshot.clone(), self.persona);
        tokio::spawn(async move {
            let _ = comms.send(CommsCmd::Thinking(true)).await;
            let t0 = now_ms();
            let text = transcribe(&stt, &clip).await;
            tracing::info!("driver ({} ms clip, {} ms STT): {:?}", clip.duration_ms(), now_ms() - t0, text);
            match text {
                Some(t) if !t.is_empty() => answer(t, &comms, &llm, &snap, status, persona).await,
                Some(_) => {
                    let _ = comms.send(CommsCmd::DriverAnswer { text: answers::SAY_AGAIN.into(), source: RadioSource::Rule }).await;
                }
                None => {
                    let text = "Radio problem, we can't hear you. The speech server is down.".to_string();
                    let _ = comms.send(CommsCmd::DriverAnswer { text, source: RadioSource::Rule }).await;
                }
            }
            let _ = comms.send(CommsCmd::Thinking(false)).await;
        });
    }

    fn spawn_answer(&self, text: String, status: VoiceStatus) {
        let (comms, llm, snap, persona) = (self.comms.clone(), self.llm.clone(), self.snapshot.clone(), self.persona);
        tokio::spawn(async move {
            let _ = comms.send(CommsCmd::Thinking(true)).await;
            answer(text, &comms, &llm, &snap, status, persona).await;
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
async fn answer(question: String, comms: &mpsc::Sender<CommsCmd>, llm: &LlmClient, snap: &watch::Receiver<Arc<TelemetrySnapshot>>, status: VoiceStatus, persona: Persona) {
    let question = question.trim().to_string();
    if question.is_empty() {
        return;
    }
    let _ = comms.send(CommsCmd::DriverSaid(question.clone())).await;
    let s = snap.borrow().clone();
    if let Some(a) = answers::answer_as(&question, &s, status.llm, persona) {
        let _ = comms.send(CommsCmd::DriverAnswer { text: a, source: RadioSource::Rule }).await;
        return;
    }
    if !llm.enabled() || !status.llm {
        let _ = comms.send(CommsCmd::DriverAnswer { text: "Copy. I don't have an answer for that one right now.".into(), source: RadioSource::Rule }).await;
        return;
    }
    let _ = comms.send(CommsCmd::DriverAnswer { text: answers::ack(persona).into(), source: RadioSource::Rule }).await;
    let t0 = now_ms();
    let reply = llm.chat(answers::system_prompt(persona), &answers::llm_user_message(&question, &s)).await;
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
            // Fall back to whatever telemetry can say about the question.
            let s = snap.borrow().clone();
            let text = answers::answer_as(&question, &s, false, persona).unwrap_or_else(|| "Copy, we'll come back to you on that.".into());
            let _ = comms.send(CommsCmd::DriverAnswer { text, source: RadioSource::Rule }).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udp_action_numbers() {
        assert_eq!(udp_action_pressed(0), None);
        assert_eq!(udp_action_pressed(0x0010_0000), Some(1));
        assert_eq!(udp_action_pressed(0x0020_0000 | 0x1), Some(2));
        assert_eq!(udp_action_pressed(0x8000_0000), Some(12));
    }

    #[test]
    fn auto_mode_supports_hold_and_tap() {
        let bit = f1_udp::packets::udp_action_bit(1);
        let mut p = PttState::new(PttMode::Auto, 1);
        // Hold to talk.
        assert_eq!(p.on_buttons(bit, 1000), Some(PttEdge::Start));
        assert_eq!(p.on_buttons(0, 3000), Some(PttEdge::Stop));
        // Tap (the 264 ms press from the log) latches open...
        assert_eq!(p.on_buttons(bit, 5000), Some(PttEdge::Start));
        assert_eq!(p.on_buttons(0, 5264), None);
        assert!(p.latched());
        // ...and the next tap sends; its release is ignored.
        assert_eq!(p.on_buttons(bit, 8000), Some(PttEdge::Stop));
        assert_eq!(p.on_buttons(0, 8100), None);
        assert!(!p.active());
    }

    #[test]
    fn hold_mode_follows_the_button() {
        let bit = f1_udp::packets::udp_action_bit(3);
        let mut p = PttState::new(PttMode::Hold, 3);
        assert_eq!(p.on_buttons(0x1, 0), None, "other buttons are ignored");
        assert_eq!(p.on_buttons(bit | 0x1, 0), Some(PttEdge::Start));
        assert_eq!(p.on_buttons(bit, 10), None, "still held");
        assert_eq!(p.on_buttons(0, 20), Some(PttEdge::Stop));
    }

    #[test]
    fn toggle_mode_flips_on_presses() {
        let bit = f1_udp::packets::udp_action_bit(1);
        let mut p = PttState::new(PttMode::Toggle, 1);
        assert_eq!(p.on_buttons(bit, 0), Some(PttEdge::Start));
        assert_eq!(p.on_buttons(0, 100), None, "release does nothing");
        assert_eq!(p.on_buttons(bit, 2000), Some(PttEdge::Stop));
        assert_eq!(p.on_buttons(0, 2100), None);
        // Timeout path.
        assert_eq!(p.on_buttons(bit, 3000), Some(PttEdge::Start));
        assert_eq!(p.set(false), Some(PttEdge::Stop));
        assert!(!p.active());
    }
}
