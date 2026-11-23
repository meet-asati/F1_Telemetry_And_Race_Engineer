//! Comms arbitrator: decides what gets said, when.
//!
//! * P0 (flags, penalties, friend radio) — spoken immediately; preempts a P2
//!   line that is already playing.
//! * P1 (answers to driver questions, approved suggestions) — spoken as soon
//!   as the channel is free; not gated.
//! * P2 (proactive callouts) — held until a straight (`throttle > 0.85`,
//!   `|steer| < 0.15`), outside speech-free windows and friend override, and
//!   dropped after a max queue age. Per-key cooldowns prevent nagging; a key
//!   re-arms early only if severity escalates.
//! * Copilot mode — P2 callouts become suggestions awaiting friend approval.
//!
//! The arbitrator is pure logic: time is always passed in (ms).

use crate::rules::Alert;
use race_state::snapshot::*;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ArbConfig {
    pub p2_max_age_ms: u64,
    pub override_ms: u64,
    pub gate_throttle: f32,
    pub gate_steer: f32,
    /// Default cooldown per alert key.
    pub default_cooldown_ms: u64,
    pub cooldowns_ms: HashMap<String, u64>,
    /// Rough speech duration estimate used until playback reports back.
    pub ms_per_char: u64,
}

impl Default for ArbConfig {
    fn default() -> Self {
        let cooldowns_ms = [
            ("tyre_wear", 180_000),
            ("tyre_temp", 120_000),
            ("tyre_window", 300_000),
            ("fuel", 150_000),
            ("ers_low", 120_000),
            ("damage", 240_000),
            ("drs_fault", 600_000),
            ("brakes", 120_000),
            ("rain_soon", 240_000),
            ("rain_now", 180_000),
            ("defend", 60_000),
            ("push", 90_000),
            ("pu_failure", 600_000),
            // Event-driven P0s: one per occurrence.
            ("flag_sc", 5_000),
            ("flag_vsc", 5_000),
            ("flag_sc_end", 5_000),
            ("flag_vsc_end", 5_000),
            ("flag_green", 5_000),
            ("flag_red", 5_000),
            ("flag_chequered", 5_000),
            ("penalty", 3_000),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        Self { p2_max_age_ms: 20_000, override_ms: 10_000, gate_throttle: 0.85, gate_steer: 0.15, default_cooldown_ms: 120_000, cooldowns_ms, ms_per_char: 65 }
    }
}

/// Driving context used for gating.
#[derive(Debug, Clone, Default)]
pub struct GateCtx {
    pub throttle: f32,
    pub steer: f32,
    pub in_pit: bool,
    /// Snapshot `session.phase`: grid | start | racing | finished.
    pub phase: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    pub id: String,
    pub text: String,
    pub priority: Priority,
    pub source: RadioSource,
    pub attachments: Vec<String>,
    /// Stop whatever is playing (P0 preempting P2).
    pub interrupt: bool,
}

#[derive(Debug, Clone)]
struct Queued {
    id: String,
    key: Option<String>,
    text: String,
    priority: Priority,
    source: RadioSource,
    attachments: Vec<String>,
    created_ms: u64,
}

pub struct Arbitrator {
    pub cfg: ArbConfig,
    mode: CommsMode,
    muted: bool,
    override_until_ms: u64,
    queue: Vec<Queued>,
    suggestions: Vec<(Suggestion, Option<String>)>,
    cooldowns: HashMap<String, (u64, AlertLevel)>,
    speaking_until_ms: u64,
    speaking_priority: Option<Priority>,
    last_spoken: Option<String>,
    next_id: u64,
    tts_available: bool,
    ptt_active: bool,
    events: Vec<PitWallEvent>,
}

impl Arbitrator {
    pub fn new(cfg: ArbConfig) -> Self {
        Self {
            cfg,
            mode: CommsMode::Auto,
            muted: false,
            override_until_ms: 0,
            queue: Vec::new(),
            suggestions: Vec::new(),
            cooldowns: HashMap::new(),
            speaking_until_ms: 0,
            speaking_priority: None,
            last_spoken: None,
            next_id: 1,
            tts_available: false,
            ptt_active: false,
            events: Vec::new(),
        }
    }

    fn new_id(&mut self) -> String {
        let id = format!("r{}", self.next_id);
        self.next_id += 1;
        id
    }

    fn cooldown_for(&self, key: &str) -> u64 {
        self.cfg.cooldowns_ms.get(key).copied().unwrap_or(self.cfg.default_cooldown_ms)
    }

    pub fn mode(&self) -> CommsMode {
        self.mode
    }
    pub fn set_mode(&mut self, mode: CommsMode) {
        self.mode = mode;
    }
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }
    pub fn set_tts_available(&mut self, v: bool) {
        self.tts_available = v;
    }
    pub fn set_ptt_active(&mut self, v: bool) {
        self.ptt_active = v;
    }

    /// Take lifecycle events (suggestions added/resolved, expired/suppressed radio).
    pub fn drain_events(&mut self) -> Vec<PitWallEvent> {
        std::mem::take(&mut self.events)
    }

    /// Offer an alert. Returns `true` if it was queued or turned into a suggestion.
    pub fn submit(&mut self, alert: Alert, now: u64) -> bool {
        if let Some(&(last, sev)) = self.cooldowns.get(&alert.key) {
            let in_cooldown = now.saturating_sub(last) < self.cooldown_for(&alert.key);
            if in_cooldown && alert.severity <= sev {
                return false;
            }
        }
        // Already waiting (queue or copilot card)? Replace the text if escalated, otherwise ignore.
        if let Some(q) = self.queue.iter_mut().find(|q| q.key.as_deref() == Some(&alert.key)) {
            q.text = alert.text;
            q.attachments = alert.attachments;
            return false;
        }
        if self.suggestions.iter().any(|(_, k)| k.as_deref() == Some(&alert.key)) {
            return false;
        }
        self.cooldowns.insert(alert.key.clone(), (now, alert.severity));
        let id = self.new_id();
        if alert.priority == Priority::P2 && self.mode == CommsMode::Copilot {
            let s = Suggestion {
                id: id.clone(),
                priority: alert.priority,
                category: alert.category.to_string(),
                message: alert.text,
                created_at_ms: now,
                expires_at_ms: now + self.cfg.p2_max_age_ms,
                context_attachments: alert.attachments,
            };
            self.suggestions.push((s, Some(alert.key)));
            self.events.push(PitWallEvent::SuggestionAdded { suggestion_id: id });
            return true;
        }
        self.queue.push(Queued {
            id,
            key: Some(alert.key),
            text: alert.text,
            priority: alert.priority,
            source: RadioSource::Rule,
            attachments: alert.attachments,
            created_ms: now,
        });
        true
    }

    /// The human engineer sends a radio message: spoken as P0 and starts the
    /// override window that holds back AI P2 callouts.
    pub fn friend_radio(&mut self, text: String, attachments: Vec<String>, now: u64) -> String {
        self.override_until_ms = now + self.cfg.override_ms;
        let id = self.new_id();
        self.queue.push(Queued { id: id.clone(), key: None, text, priority: Priority::P0, source: RadioSource::Friend, attachments, created_ms: now });
        id
    }

    /// The friend started speaking on voice chat (Discord, later milestone).
    pub fn friend_speaking(&mut self, now: u64) {
        self.override_until_ms = self.override_until_ms.max(now + self.cfg.override_ms);
    }

    /// Queue an answer to a driver question (P1).
    pub fn driver_answer(&mut self, text: String, now: u64) -> String {
        let id = self.new_id();
        self.queue.push(Queued { id: id.clone(), key: None, text, priority: Priority::P1, source: RadioSource::Llm, attachments: vec![], created_ms: now });
        id
    }

    fn take_suggestion(&mut self, id: &str) -> Option<(Suggestion, Option<String>)> {
        let i = self.suggestions.iter().position(|(s, _)| s.id == id)?;
        Some(self.suggestions.remove(i))
    }

    pub fn approve(&mut self, id: &str, edited_text: Option<String>, now: u64) -> bool {
        let Some((s, key)) = self.take_suggestion(id) else { return false };
        let edited = edited_text.is_some();
        let text = edited_text.unwrap_or(s.message);
        self.events.push(PitWallEvent::SuggestionResolved {
            suggestion_id: s.id.clone(),
            outcome: if edited { SuggestionOutcome::Edited } else { SuggestionOutcome::Approved },
        });
        // Approved by the human engineer: speak promptly (P1, ungated).
        let source = if edited { RadioSource::Friend } else { RadioSource::Rule };
        self.queue.push(Queued { id: s.id, key, text, priority: Priority::P1, source, attachments: s.context_attachments, created_ms: now });
        true
    }

    pub fn dismiss(&mut self, id: &str, now: u64) -> bool {
        let Some((s, _)) = self.take_suggestion(id) else { return false };
        self.events.push(PitWallEvent::SuggestionResolved { suggestion_id: s.id.clone(), outcome: SuggestionOutcome::Dismissed });
        self.events.push(PitWallEvent::Radio {
            at_ms: now,
            source: RadioSource::Rule,
            priority: s.priority,
            text: s.message,
            outcome: RadioOutcome::Dismissed,
            attachments: s.context_attachments,
        });
        true
    }

    fn override_active(&self, now: u64) -> bool {
        now < self.override_until_ms
    }

    fn gate_open(&self, ctx: &GateCtx, now: u64) -> bool {
        ctx.phase == "racing" && !ctx.in_pit && ctx.throttle >= self.cfg.gate_throttle && ctx.steer.abs() <= self.cfg.gate_steer && !self.override_active(now) && !self.ptt_active
    }

    /// Playback reports the real end time of the current line.
    pub fn set_speaking_until(&mut self, until_ms: u64) {
        self.speaking_until_ms = until_ms;
    }

    /// Advance time; returns at most one line to speak now.
    pub fn tick(&mut self, now: u64, ctx: &GateCtx) -> Option<Utterance> {
        // Expire stale P2 items and copilot suggestions.
        let max_age = self.cfg.p2_max_age_ms;
        let (expired, keep): (Vec<Queued>, Vec<Queued>) = self.queue.drain(..).partition(|q| q.priority == Priority::P2 && now.saturating_sub(q.created_ms) > max_age);
        self.queue = keep;
        for q in expired {
            self.events.push(PitWallEvent::Radio { at_ms: now, source: q.source, priority: q.priority, text: q.text, outcome: RadioOutcome::Expired, attachments: q.attachments });
        }
        let (expired, keep): (Vec<_>, Vec<_>) = self.suggestions.drain(..).partition(|(s, _)| now > s.expires_at_ms);
        self.suggestions = keep;
        for (s, _) in expired {
            self.events.push(PitWallEvent::SuggestionResolved { suggestion_id: s.id, outcome: SuggestionOutcome::Expired });
        }

        // Muted: AI lines are logged as text only; friend radio still plays.
        if self.muted {
            let (ai, rest): (Vec<Queued>, Vec<Queued>) = self.queue.drain(..).partition(|q| matches!(q.source, RadioSource::Rule | RadioSource::Llm));
            self.queue = rest;
            for q in ai {
                self.events.push(PitWallEvent::Radio { at_ms: now, source: q.source, priority: q.priority, text: q.text, outcome: RadioOutcome::Suppressed, attachments: q.attachments });
            }
        }

        // Highest priority first, then oldest.
        self.queue.sort_by_key(|q| (q.priority, q.created_ms));
        let speaking = now < self.speaking_until_ms;
        let idx = self.queue.iter().position(|q| match q.priority {
            Priority::P0 => !speaking || self.speaking_priority == Some(Priority::P2),
            Priority::P1 => !speaking,
            Priority::P2 => !speaking && self.gate_open(ctx, now),
        })?;
        let q = self.queue.remove(idx);
        let interrupt = speaking && q.priority == Priority::P0;
        self.speaking_until_ms = now + 600 + q.text.len() as u64 * self.cfg.ms_per_char;
        self.speaking_priority = Some(q.priority);
        self.last_spoken = Some(q.text.clone());
        if let Some(k) = &q.key {
            // Cooldown runs from when it was actually said.
            let sev = self.cooldowns.get(k).map(|c| c.1).unwrap_or_default();
            self.cooldowns.insert(k.clone(), (now, sev));
        }
        Some(Utterance { id: q.id, text: q.text, priority: q.priority, source: q.source, attachments: q.attachments, interrupt })
    }

    pub fn state(&self, now: u64) -> CommsState {
        CommsState {
            mode: self.mode,
            muted: self.muted,
            friend_override_active: self.override_active(now),
            override_remaining_ms: self.override_until_ms.saturating_sub(now),
            ptt_active: self.ptt_active,
            tts_available: self.tts_available,
            speaking: now < self.speaking_until_ms,
            last_spoken_line: self.last_spoken.clone(),
            pending_suggestions: self.suggestions.iter().map(|(s, _)| s.clone()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(key: &str, p: Priority, sev: AlertLevel) -> Alert {
        Alert { key: key.into(), priority: p, severity: sev, category: "test", text: format!("{key} text"), attachments: vec![] }
    }

    fn straight() -> GateCtx {
        GateCtx { throttle: 1.0, steer: 0.0, in_pit: false, phase: "racing".into() }
    }
    fn corner() -> GateCtx {
        GateCtx { throttle: 0.3, steer: 0.5, in_pit: false, phase: "racing".into() }
    }

    #[test]
    fn p2_waits_for_straight_then_speaks() {
        let mut a = Arbitrator::new(ArbConfig::default());
        assert!(a.submit(alert("fuel", Priority::P2, AlertLevel::Watch), 0));
        assert!(a.tick(100, &corner()).is_none());
        let u = a.tick(200, &straight()).unwrap();
        assert_eq!(u.text, "fuel text");
        assert!(!u.interrupt);
    }

    #[test]
    fn p2_expires_after_max_age() {
        let mut a = Arbitrator::new(ArbConfig::default());
        a.submit(alert("fuel", Priority::P2, AlertLevel::Watch), 0);
        assert!(a.tick(25_000, &straight()).is_none());
        let ev = a.drain_events();
        assert!(matches!(ev[0], PitWallEvent::Radio { outcome: RadioOutcome::Expired, .. }));
    }

    #[test]
    fn cooldown_blocks_repeat_unless_escalated() {
        let mut a = Arbitrator::new(ArbConfig::default());
        a.submit(alert("tyre_wear", Priority::P2, AlertLevel::Watch), 0);
        a.tick(10, &straight()).unwrap();
        assert!(!a.submit(alert("tyre_wear", Priority::P2, AlertLevel::Watch), 30_000), "same severity within cooldown");
        assert!(a.submit(alert("tyre_wear", Priority::P2, AlertLevel::Critical), 30_000), "escalation re-arms");
        assert!(a.tick(40_000, &straight()).is_some());
        assert!(a.submit(alert("tyre_wear", Priority::P2, AlertLevel::Critical), 40_000 + 181_000), "cooldown elapsed");
    }

    #[test]
    fn p0_preempts_p2_and_ignores_gating() {
        let mut a = Arbitrator::new(ArbConfig::default());
        a.submit(alert("fuel", Priority::P2, AlertLevel::Watch), 0);
        a.tick(10, &straight()).unwrap();
        a.submit(alert("flag_sc", Priority::P0, AlertLevel::Critical), 100);
        let u = a.tick(200, &corner()).unwrap();
        assert_eq!(u.priority, Priority::P0);
        assert!(u.interrupt, "P2 still playing at t=200");
        // A second P0 does not interrupt a P0.
        a.submit(alert("penalty", Priority::P0, AlertLevel::Critical), 300);
        assert!(a.tick(400, &corner()).is_none());
    }

    #[test]
    fn friend_override_holds_p2() {
        let mut a = Arbitrator::new(ArbConfig::default());
        let _ = a.friend_radio("Box box".into(), vec![], 0);
        let u = a.tick(10, &corner()).unwrap();
        assert_eq!(u.source, RadioSource::Friend);
        a.submit(alert("fuel", Priority::P2, AlertLevel::Watch), 2_000);
        assert!(a.tick(5_000, &straight()).is_none(), "override active");
        assert!(a.state(5_000).friend_override_active);
        assert!(a.tick(10_500, &straight()).is_some(), "override over");
    }

    #[test]
    fn speech_free_windows() {
        let mut a = Arbitrator::new(ArbConfig::default());
        a.submit(alert("fuel", Priority::P2, AlertLevel::Watch), 0);
        for phase in ["grid", "start", "finished"] {
            assert!(a.tick(10, &GateCtx { phase: phase.into(), ..straight() }).is_none());
        }
        assert!(a.tick(10, &GateCtx { in_pit: true, ..straight() }).is_none());
        assert!(a.tick(10, &straight()).is_some());
    }

    #[test]
    fn copilot_suggestion_flow() {
        let mut a = Arbitrator::new(ArbConfig::default());
        a.set_mode(CommsMode::Copilot);
        a.submit(alert("fuel", Priority::P2, AlertLevel::Watch), 0);
        a.submit(alert("damage", Priority::P2, AlertLevel::Watch), 0);
        // P0 is never held for approval.
        a.submit(alert("flag_sc", Priority::P0, AlertLevel::Critical), 0);
        let st = a.state(0);
        assert_eq!(st.pending_suggestions.len(), 2);
        let (fuel_id, dmg_id) = (st.pending_suggestions[0].id.clone(), st.pending_suggestions[1].id.clone());
        assert_eq!(a.tick(10, &corner()).unwrap().priority, Priority::P0);
        assert!(a.approve(&fuel_id, Some("Lift and coast please".into()), 5_000));
        assert!(a.dismiss(&dmg_id, 5_000));
        let u = a.tick(20_000, &corner()).unwrap();
        assert_eq!(u.text, "Lift and coast please");
        assert_eq!(u.priority, Priority::P1);
        let ev = a.drain_events();
        assert!(ev.iter().any(|e| matches!(e, PitWallEvent::SuggestionResolved { outcome: SuggestionOutcome::Edited, .. })));
        assert!(ev.iter().any(|e| matches!(e, PitWallEvent::SuggestionResolved { outcome: SuggestionOutcome::Dismissed, .. })));
        assert!(a.state(20_000).pending_suggestions.is_empty());
    }

    #[test]
    fn copilot_suggestions_expire() {
        let mut a = Arbitrator::new(ArbConfig::default());
        a.set_mode(CommsMode::Copilot);
        a.submit(alert("fuel", Priority::P2, AlertLevel::Watch), 0);
        a.tick(21_000, &straight());
        assert!(a.state(21_000).pending_suggestions.is_empty());
        assert!(a.drain_events().iter().any(|e| matches!(e, PitWallEvent::SuggestionResolved { outcome: SuggestionOutcome::Expired, .. })));
    }

    #[test]
    fn mute_suppresses_ai_but_not_friend() {
        let mut a = Arbitrator::new(ArbConfig::default());
        a.set_muted(true);
        a.submit(alert("flag_sc", Priority::P0, AlertLevel::Critical), 0);
        a.friend_radio("Copy".into(), vec![], 0);
        let u = a.tick(10, &straight()).unwrap();
        assert_eq!(u.source, RadioSource::Friend);
        assert!(a.drain_events().iter().any(|e| matches!(e, PitWallEvent::Radio { outcome: RadioOutcome::Suppressed, .. })));
    }
}
