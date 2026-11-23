//! Deterministic race-engineer rule engine.
//!
//! Every rule reads the current snapshot (plus a little memory for trends)
//! and yields [`Alert`]s with a stable `key`. The arbitrator decides whether
//! and when an alert is spoken (cooldowns, gating, Copilot approval). No LLM
//! is involved, so a callout can only ever state values from telemetry.
//!
//! The voice is one consistent persona: a calm, precise, British race
//! engineer. Information first, then one clear instruction; rivals by
//! surname; praise when it is earned; never more words than the moment
//! allows.

use race_state::snapshot::*;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    /// Stable identity used for cooldown / de-duplication.
    pub key: String,
    pub priority: Priority,
    pub severity: AlertLevel,
    pub category: &'static str,
    pub text: String,
    pub attachments: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RuleConfig {
    pub defend_gap_ms: u32,
    pub push_gap_max_ms: u32,
    pub rain_warn_pct: u8,
    pub rain_lookahead_min: u8,
    /// The driver's first name, used sparingly (praise, the finish).
    pub driver_name: Option<String>,
}

impl Default for RuleConfig {
    fn default() -> Self {
        Self { defend_gap_ms: 1000, push_gap_max_ms: 3000, rain_warn_pct: 60, rain_lookahead_min: 15, driver_name: None }
    }
}

/// How the engineer says another driver's name: their surname in title case
/// ("VERSTAPPEN" -> "Verstappen"), falling back to the timing label.
pub fn spoken_name(e: &TimingEntry) -> String {
    if e.driver_name.starts_with("Car ") {
        return e.driver_name.clone();
    }
    let last = e.driver_name.split_whitespace().last().unwrap_or("");
    let clean: String = last.chars().filter(|c| c.is_alphabetic() || *c == '-').collect();
    if clean.chars().count() < 2 {
        return e.driver_label.clone();
    }
    let mut out = String::new();
    let mut up = true;
    for c in clean.to_lowercase().chars() {
        if up {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        up = c == '-';
    }
    out
}

/// Lap time as written on the pit wall, tenths only: 67033 -> "1:07.0".
pub fn lap_str(ms: u32) -> String {
    let m = ms / 60_000;
    let tenths = (ms % 60_000) / 100;
    format!("{m}:{:02}.{}", tenths / 10, tenths % 10)
}

pub fn compound_name(c: &str) -> &'static str {
    match c {
        "S" => "softs",
        "M" => "mediums",
        "H" => "hards",
        "I" => "inters",
        "W" => "wets",
        _ => "new tyres",
    }
}

const CORNER_NAMES: [&str; 4] = ["front-left", "front-right", "rear-left", "rear-right"];
const CORNER_SHORT: [&str; 4] = ["FL", "FR", "RL", "RR"];

pub struct RuleEngine {
    pub cfg: RuleConfig,
    variant: HashMap<String, usize>,
    /// (timestamp ms, gap ms) samples for trend detection.
    gap_behind_hist: Vec<(u64, u32)>,
    gap_ahead_hist: Vec<(u64, u32)>,
    /// Memory for event-like level changes.
    last_position: Option<u8>,
    last_best_ms: Option<u32>,
    last_pit_stops: Option<u8>,
    session_uid: Option<String>,
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self::new(RuleConfig::default())
    }
}

fn fmt_secs(ms: u32) -> String {
    format!("{:.1}", ms as f32 / 1000.0)
}

impl RuleEngine {
    pub fn new(cfg: RuleConfig) -> Self {
        Self {
            cfg,
            variant: HashMap::new(),
            gap_behind_hist: Vec::new(),
            gap_ahead_hist: Vec::new(),
            last_position: None,
            last_best_ms: None,
            last_pit_stops: None,
            session_uid: None,
        }
    }

    /// Rotate through phrase variants per key so repeated calls don't sound canned.
    fn pick(&mut self, key: &str, variants: &[String]) -> String {
        let n = self.variant.entry(key.to_string()).or_insert(0);
        let s = variants[*n % variants.len()].clone();
        *n += 1;
        s
    }

    fn alert(&mut self, key: &str, priority: Priority, severity: AlertLevel, category: &'static str, variants: &[String], attachments: Vec<String>) -> Alert {
        let text = self.pick(key, variants);
        Alert { key: key.to_string(), priority, severity, category, text, attachments }
    }

    /// Alerts derived from discrete events (flags, penalties).
    pub fn on_event(&mut self, ev: &PitWallEvent, snap: &TelemetrySnapshot) -> Vec<Alert> {
        let player = snap.timing.player_index;
        let pit_stops = snap.timing.entries.iter().find(|e| e.is_player).map(|e| e.pit_stops).unwrap_or(0);
        let lap = snap.session.lap;
        let total = snap.session.total_laps.max(1);
        let cheap_stop = snap.session.is_race && pit_stops == 0 && lap as f32 >= total as f32 * 0.25 && lap + 2 < total;
        let mut out = Vec::new();
        match ev {
            PitWallEvent::Flag { status, .. } => {
                let (key, variants): (&str, Vec<String>) = match status {
                    FlagStatus::Sc => (
                        "flag_sc",
                        vec![format!(
                            "Safety car, safety car. Stay above the delta, no overtaking.{}",
                            if cheap_stop { " It's a cheap stop if we box this lap." } else { "" }
                        )],
                    ),
                    FlagStatus::Vsc => (
                        "flag_vsc",
                        vec![format!("VSC, VSC. Keep it positive to the delta.{}", if cheap_stop { " Boxing this lap is cheap, it's an option." } else { "" })],
                    ),
                    FlagStatus::ScEnding => (
                        "flag_sc_end",
                        vec!["Safety car in this lap. Get heat in the tyres and brakes, be ready.".into(), "Safety car is in this lap. Restart coming, wake the tyres up.".into()],
                    ),
                    FlagStatus::VscEnding => ("flag_vsc_end", vec!["VSC ending, VSC ending. Be ready.".into()]),
                    FlagStatus::Green => ("flag_green", vec!["Green, green, green. Go.".into(), "Track is clear. Full push now.".into()]),
                    FlagStatus::Red => ("flag_red", vec!["Red flag, red flag. Slow down and bring it back to the pit lane.".into()]),
                    FlagStatus::Chequered => ("flag_chequered", vec![self.finish_line(snap)]),
                };
                out.push(self.alert(key, Priority::P0, AlertLevel::Critical, "flags", &variants, vec![]));
            }
            PitWallEvent::Penalty { car_index, description, time_sec, .. } if *car_index == player => {
                let txt = match (time_sec, description.as_str()) {
                    (Some(t), "Time penalty") => format!("We've got a {t} second time penalty. Head down, we can build the gap to cover it."),
                    (Some(t), d) => format!("Penalty, {}, {t} seconds. Keep it clean from here.", d.to_lowercase()),
                    (None, d) => format!("We've been given a {} penalty. Stay focused.", d.to_lowercase()),
                };
                out.push(self.alert("penalty", Priority::P0, AlertLevel::Critical, "penalty", &[txt], vec![description.clone()]));
            }
            _ => {}
        }
        out
    }

    /// The engineer's line as the driver takes the flag.
    fn finish_line(&self, snap: &TelemetrySnapshot) -> String {
        let pos = snap.timing.position;
        let name = self.cfg.driver_name.as_deref().filter(|n| !n.is_empty()).map(|n| format!(", {n}")).unwrap_or_default();
        if !snap.session.is_race || pos == 0 {
            return format!("Chequered flag. That's the session, good work{name}.");
        }
        match pos {
            1 => format!("Chequered flag. P1, you've won it! Fantastic drive{name}, absolutely mega."),
            2 | 3 => format!("Chequered flag, that's P{pos}. A podium{name}, superb drive."),
            4..=10 => format!("Chequered flag, P{pos}. Points in the bag. Well driven{name}."),
            _ => format!("Chequered flag, P{pos}. Thanks for the effort{name}, we'll go through it together."),
        }
    }

    /// Level-based alerts from the current snapshot.
    pub fn evaluate(&mut self, snap: &TelemetrySnapshot) -> Vec<Alert> {
        let mut out = Vec::new();
        if snap.session_uid != self.session_uid {
            self.session_uid = snap.session_uid.clone();
            self.last_position = None;
            self.last_best_ms = None;
            self.last_pit_stops = None;
            self.gap_ahead_hist.clear();
            self.gap_behind_hist.clear();
        }
        if snap.timing.entries.is_empty() || snap.session.phase == "finished" {
            return out;
        }
        let now = snap.timestamp_ms;
        let t = &snap.tyres;
        let p = &snap.powertrain;
        let d = &snap.damage;
        let s = &snap.session;

        // ---- Tyre wear (worst corner).
        let (wi, &w) = t.wear_pct.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)).unwrap();
        if t.alert_level >= AlertLevel::Watch && w >= 1.0 {
            let att = vec![format!("{} wear {:.0}%", CORNER_SHORT[wi], w)];
            let corner = CORNER_NAMES[wi];
            let critical = t.wear_pct.iter().any(|&x| x >= 65.0);
            if critical {
                let pit = t.projected_pit_lap.map(|l| format!(" We need to box by lap {l}.")).unwrap_or_default();
                let v = vec![
                    format!("{} is at {w:.0} percent, that's critical.{pit}", capitalize(corner)),
                    format!("Tyres are going away. {} at {w:.0} percent.{pit}", capitalize(corner)),
                ];
                out.push(self.alert("tyre_wear", Priority::P2, AlertLevel::Critical, "tyres", &v, att));
            } else if w >= 50.0 {
                let v = vec![
                    format!("{} is at {w:.0} percent. Look after it through the quick stuff.", capitalize(corner)),
                    format!("Watch the {corner}, {w:.0} percent worn. Manage the slides."),
                ];
                out.push(self.alert("tyre_wear", Priority::P2, AlertLevel::Watch, "tyres", &v, att));
            }
        }

        // ---- Tyres won't make the game's pit window.
        if let (Some(proj), Some(win)) = (t.projected_pit_lap, &s.pit_window) {
            let pitted = snap.timing.entries.iter().find(|e| e.is_player).map(|e| e.pit_stops).unwrap_or(0) > 0;
            if !pitted && s.is_race && proj + 1 < win.ideal_lap && s.lap + 1 < win.ideal_lap {
                let v = vec![format!("Heads up, these tyres won't make lap {}. We're projecting lap {proj}.", win.ideal_lap)];
                out.push(self.alert("tyre_window", Priority::P2, AlertLevel::Watch, "strategy", &v, vec![format!("Projected pit lap {proj}")]));
            }
        }

        // ---- Tyre surface temperature.
        let fronts_hot = t.surface_temp_c[0].max(t.surface_temp_c[1]);
        if fronts_hot >= 112 {
            let v = vec!["Fronts are running hot. Cool them on the straights.".to_string(), format!("Front tyres at {fronts_hot} degrees. Bring the temps down, gentle on entry.")];
            out.push(self.alert("tyre_temp", Priority::P2, AlertLevel::Watch, "tyres", &v, vec![format!("Front surface {fronts_hot}°C")]));
        }

        // ---- Fuel.
        let fd = p.fuel_remaining_laps_delta;
        if snap.session.is_race && fd <= -0.2 && p.fuel_remaining_kg > 0.0 {
            let att = vec![format!("Fuel {fd:+.1} laps")];
            if fd <= -0.6 {
                let v = vec![format!("Fuel critical, minus {:.1} laps. Heavy lift and coast, every straight.", -fd)];
                out.push(self.alert("fuel", Priority::P2, AlertLevel::Critical, "fuel", &v, att));
            } else {
                let v = vec![
                    format!("Fuel is minus {:.1} laps. Start lift and coast, a hundred metres before the big stops.", -fd),
                    format!("We need to save fuel, we're {:.1} laps short. Lift and coast please.", -fd),
                ];
                out.push(self.alert("fuel", Priority::P2, AlertLevel::Watch, "fuel", &v, att));
            }
        }

        // ---- ERS.
        if p.ers_store_pct > 0.0 && p.ers_store_pct < 10.0 && s.phase == "racing" {
            let v = vec![format!("Battery's low, {:.0} percent. Harvest this lap.", p.ers_store_pct)];
            out.push(self.alert("ers_low", Priority::P2, AlertLevel::Watch, "ers", &v, vec![format!("ERS {:.0}%", p.ers_store_pct)]));
        }

        // ---- Powertrain failures.
        if p.ers_fault || p.engine_blown || p.engine_seized {
            let msg = if p.ers_fault { "We have an ERS fault. Manage the car, we're looking at it." } else { "Engine failure. Stop the car, pull over safely." };
            out.push(self.alert("pu_failure", Priority::P0, AlertLevel::Critical, "powertrain", &[msg.to_string()], vec![]));
        }

        // ---- Aero damage.
        let wings = [("front-left wing", d.front_left_wing_pct), ("front-right wing", d.front_right_wing_pct), ("rear wing", d.rear_wing_pct), ("floor", d.floor_pct), ("diffuser", d.diffuser_pct)];
        if let Some(&(name, val)) = wings.iter().max_by_key(|x| x.1) {
            if val >= 30 {
                let fix = if name.contains("front") { "Box this lap for a new front wing." } else { "Box for repairs." };
                let v = vec![format!("We have {val} percent {name} damage. {fix}")];
                out.push(self.alert("damage", Priority::P2, AlertLevel::Critical, "damage", &v, vec![format!("{name} {val}%")]));
            } else if val >= 10 {
                let v = vec![format!("Confirming {name} damage, {val} percent. Car is fine, carry on.")];
                out.push(self.alert("damage", Priority::P2, AlertLevel::Watch, "damage", &v, vec![format!("{name} {val}%")]));
            }
        }
        if d.drs_fault {
            out.push(self.alert("drs_fault", Priority::P2, AlertLevel::Watch, "damage", &["We have a DRS fault. Don't rely on it.".to_string()], vec![]));
        }
        let brake_max = t.brake_temp_c.iter().copied().max().unwrap_or(0);
        if brake_max >= 1000 {
            let v = vec![format!("Brakes are hot, {brake_max} degrees. Bias rearwards or lift a touch earlier.")];
            out.push(self.alert("brakes", Priority::P2, AlertLevel::Watch, "brakes", &v, vec![format!("Brakes {brake_max}°C")]));
        }

        // ---- Weather.
        let slick = matches!(t.compound.as_str(), "S" | "M" | "H");
        let raining_now = matches!(s.weather.as_str(), "lightRain" | "heavyRain" | "storm");
        if slick && raining_now {
            let v = vec!["It's raining. If the grip's gone, box for inters. Your call.".to_string()];
            out.push(self.alert("rain_now", Priority::P2, AlertLevel::Critical, "weather", &v, vec![format!("Weather {}", s.weather)]));
        } else if slick {
            if let Some(f) = s.weather_forecast.iter().filter(|f| f.time_offset_min > 0 && f.time_offset_min <= self.cfg.rain_lookahead_min).find(|f| f.rain_pct >= self.cfg.rain_warn_pct) {
                let v = vec![format!("Rain expected in about {} minutes, {} percent. Stay out for now, we'll call it.", f.time_offset_min, f.rain_pct)];
                out.push(self.alert("rain_soon", Priority::P2, AlertLevel::Watch, "weather", &v, vec![format!("Rain {}% in {} min", f.rain_pct, f.time_offset_min)]));
            }
        }

        // ---- Gaps (race only, green flag).
        if s.is_race && s.safety_car_status == "none" && s.phase == "racing" {
            let player = snap.timing.entries.iter().position(|e| e.is_player);
            if let Some(pi) = player {
                let racing = |e: &TimingEntry| e.pit_status == "none" && e.result_status == "active";
                let player_racing = racing(&snap.timing.entries[pi]);
                let behind_racing = snap.timing.entries.get(pi + 1).map(racing).unwrap_or(false);
                if let Some(gb) = snap.timing.gap_behind_ms.filter(|_| player_racing && behind_racing) {
                    push_hist(&mut self.gap_behind_hist, now, gb);
                    let closing = trend(&self.gap_behind_hist, now, 15_000).map(|d| d < -100).unwrap_or(false);
                    // Sub-0.1 s "gaps" are side-by-side moments or timing glitches, not a defend call.
                    if (100..=self.cfg.defend_gap_ms).contains(&gb) && closing {
                        let name = snap.timing.entries.get(pi + 1).map(spoken_name).unwrap_or_default();
                        let v = vec![
                            format!("{name} is {} behind and closing. Defend into the braking zones.", fmt_secs(gb)),
                            format!("{name} within a second now. Cover the inside."),
                        ];
                        out.push(self.alert("defend", Priority::P2, AlertLevel::Watch, "gap", &v, vec![format!("Gap behind {}s", fmt_secs(gb))]));
                    }
                }
                if let Some(ga) = snap.timing.gap_ahead_ms {
                    push_hist(&mut self.gap_ahead_hist, now, ga);
                    let closing = trend(&self.gap_ahead_hist, now, 30_000).map(|d| d < -250).unwrap_or(false);
                    if player_racing && ga > 1000 && ga <= self.cfg.push_gap_max_ms && closing && pi > 0 && racing(&snap.timing.entries[pi - 1]) {
                        let name = spoken_name(&snap.timing.entries[pi - 1]);
                        let v = vec![
                            format!("Gap to {name} is {}, you're catching. Keep pushing.", fmt_secs(ga)),
                            format!("{name} is {} ahead and you're quicker. Reel them in.", fmt_secs(ga)),
                        ];
                        out.push(self.alert("push", Priority::P2, AlertLevel::Nominal, "gap", &v, vec![format!("Gap ahead {}s", fmt_secs(ga))]));
                    }
                }
            }
        }
        self.race_moments(snap, &mut out);
        out
    }

    /// Personal bests, fastest lap, position changes, pit stops, laps to go.
    fn race_moments(&mut self, snap: &TelemetrySnapshot, out: &mut Vec<Alert>) {
        let s = &snap.session;
        let Some(me) = snap.timing.entries.iter().find(|e| e.is_player).cloned() else { return };
        let racing = s.phase == "racing";

        // Lap times.
        if let (Some(best), Some(last)) = (me.best_lap_ms, me.last_lap_ms) {
            let improved = self.last_best_ms.is_some_and(|prev| best < prev);
            if improved && last == best && me.lap >= 3 {
                let fastest = snap.timing.entries.iter().filter_map(|e| e.best_lap_ms).min() == Some(best);
                if fastest && s.is_race {
                    let v = vec![format!("Purple! Fastest lap of the race, {}.", lap_str(best)), format!("That's the fastest lap, {}. Mega.", lap_str(best))];
                    out.push(self.alert("fastest_lap", Priority::P2, AlertLevel::Nominal, "pace", &v, vec![format!("Best {}", lap_str(best))]));
                } else {
                    let v = vec![format!("Good lap. {}, personal best.", lap_str(best)), format!("Personal best, {}. Nice.", lap_str(best))];
                    out.push(self.alert("pb", Priority::P2, AlertLevel::Nominal, "pace", &v, vec![format!("Best {}", lap_str(best))]));
                }
            }
            self.last_best_ms = Some(best);
        }

        // Position changes (racing only).
        let pos = snap.timing.position;
        if pos > 0 {
            if let Some(prev) = self.last_position.filter(|_| s.is_race && racing && me.pit_status == "none") {
                if pos < prev {
                    let v = vec![format!("P{pos}. Nice move."), format!("Great move, P{pos} now."), format!("That's P{pos}. Keep it going.")];
                    out.push(self.alert("position", Priority::P2, AlertLevel::Nominal, "position", &v, vec![format!("P{prev} to P{pos}")]));
                } else if pos > prev {
                    let v = vec![format!("P{pos}. Lost one there, head down."), format!("Dropped to P{pos}. Reset, go again.")];
                    out.push(self.alert("position", Priority::P2, AlertLevel::Nominal, "position", &v, vec![format!("P{prev} to P{pos}")]));
                }
            }
            self.last_position = Some(pos);
        }

        // Pit stop completed.
        if let Some(prev) = self.last_pit_stops {
            if me.pit_stops > prev && s.is_race {
                let fitted = capitalize(compound_name(&snap.tyres.compound));
                let v = vec![format!("Good stop. {fitted} fitted, get some temperature in them."), format!("Good stop, good stop. {fitted}. Out lap, build the temps.")];
                out.push(self.alert("pit_done", Priority::P1, AlertLevel::Nominal, "strategy", &v, vec![format!("Stop {}", me.pit_stops)]));
            }
        }
        self.last_pit_stops = Some(me.pit_stops);

        // Laps to go.
        if s.is_race && racing && s.total_laps >= 5 && s.lap > 0 {
            let to_go = s.total_laps.saturating_sub(s.lap) + 1;
            if to_go == 1 {
                let v = vec!["Last lap. Bring it home.".to_string(), "Final lap. Nice and clean, bring it home.".to_string()];
                out.push(self.alert("last_lap", Priority::P2, AlertLevel::Nominal, "session", &v, vec![]));
            } else if to_go == 5 {
                let v = vec![format!("Five laps to go. P{pos}, keep it tidy.")];
                out.push(self.alert("five_to_go", Priority::P2, AlertLevel::Nominal, "session", &v, vec![]));
            }
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn push_hist(h: &mut Vec<(u64, u32)>, now: u64, v: u32) {
    h.push((now, v));
    h.retain(|(t, _)| now.saturating_sub(*t) <= 60_000);
}

/// Change in value over roughly the last `window_ms` (negative = closing).
fn trend(h: &[(u64, u32)], now: u64, window_ms: u64) -> Option<i64> {
    let oldest = h.iter().find(|(t, _)| now.saturating_sub(*t) <= window_ms)?;
    let latest = h.last()?;
    if latest.0.saturating_sub(oldest.0) < window_ms / 2 {
        return None;
    }
    Some(latest.1 as i64 - oldest.1 as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_snapshot() -> TelemetrySnapshot {
        let mut s = TelemetrySnapshot::default();
        s.session.is_race = true;
        s.session.phase = "racing".into();
        s.session.safety_car_status = "none".into();
        s.session.weather = "clear".into();
        s.session.lap = 10;
        s.session.total_laps = 30;
        s.tyres.compound = "M".into();
        s.powertrain.ers_store_pct = 60.0;
        s.powertrain.fuel_remaining_kg = 30.0;
        s.timing.player_index = 1;
        s.timing.entries = vec![
            TimingEntry { car_index: 0, position: 1, driver_label: "AAA".into(), pit_status: "none".into(), result_status: "active".into(), ..Default::default() },
            TimingEntry { car_index: 1, position: 2, driver_label: "PLA".into(), is_player: true, pit_status: "none".into(), result_status: "active".into(), ..Default::default() },
            TimingEntry { car_index: 2, position: 3, driver_label: "BBB".into(), pit_status: "none".into(), result_status: "active".into(), ..Default::default() },
        ];
        s
    }

    #[test]
    fn nominal_snapshot_is_quiet() {
        let mut r = RuleEngine::default();
        assert!(r.evaluate(&base_snapshot()).is_empty());
    }

    #[test]
    fn tyre_wear_watch_and_critical() {
        let mut r = RuleEngine::default();
        let mut s = base_snapshot();
        s.tyres.wear_pct = [55.0, 40.0, 30.0, 30.0];
        s.tyres.alert_level = AlertLevel::Watch;
        let a = r.evaluate(&s);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].key, "tyre_wear");
        assert_eq!(a[0].severity, AlertLevel::Watch);
        assert!(a[0].text.to_lowercase().contains("front-left"));
        assert_eq!(a[0].attachments, vec!["FL wear 55%"]);
        s.tyres.wear_pct[0] = 70.0;
        s.tyres.alert_level = AlertLevel::Critical;
        s.tyres.projected_pit_lap = Some(12);
        let a = r.evaluate(&s);
        assert_eq!(a[0].severity, AlertLevel::Critical);
        assert!(a[0].text.contains("lap 12"));
    }

    #[test]
    fn fuel_and_rain_and_damage() {
        let mut r = RuleEngine::default();
        let mut s = base_snapshot();
        s.powertrain.fuel_remaining_laps_delta = -0.3;
        s.damage.front_left_wing_pct = 18;
        s.session.weather_forecast = vec![ForecastSampleView { time_offset_min: 10, weather: "lightRain".into(), rain_pct: 75, ..Default::default() }];
        let keys: Vec<String> = r.evaluate(&s).into_iter().map(|a| a.key).collect();
        assert!(keys.contains(&"fuel".to_string()));
        assert!(keys.contains(&"damage".to_string()));
        assert!(keys.contains(&"rain_soon".to_string()));
        s.session.weather = "lightRain".into();
        let keys: Vec<String> = r.evaluate(&s).into_iter().map(|a| a.key).collect();
        assert!(keys.contains(&"rain_now".to_string()));
        assert!(!keys.contains(&"rain_soon".to_string()));
    }

    #[test]
    fn defend_needs_closing_trend() {
        let mut r = RuleEngine::default();
        let mut s = base_snapshot();
        for (i, gap) in [1400u32, 1250, 1100, 950, 800].iter().enumerate() {
            s.timestamp_ms = i as u64 * 4000;
            s.timing.gap_behind_ms = Some(*gap);
            let a = r.evaluate(&s);
            let defend = a.iter().any(|a| a.key == "defend");
            assert_eq!(defend, *gap <= 1000, "gap {gap}");
        }
        // A 0.0 s gap (side by side / glitch) and a car behind in the pits never trigger defend.
        for gap in [0u32, 50] {
            s.timestamp_ms += 4000;
            s.timing.gap_behind_ms = Some(gap);
            assert!(!r.evaluate(&s).iter().any(|a| a.key == "defend"));
        }
        s.timing.entries[2].pit_status = "pitting".into();
        for gap in [700u32, 600] {
            s.timestamp_ms += 4000;
            s.timing.gap_behind_ms = Some(gap);
            assert!(!r.evaluate(&s).iter().any(|a| a.key == "defend"));
        }
    }

    #[test]
    fn flags_and_penalties_are_p0() {
        let mut r = RuleEngine::default();
        let s = base_snapshot();
        let a = r.on_event(&PitWallEvent::Flag { at_ms: 0, status: FlagStatus::Sc }, &s);
        assert_eq!(a[0].priority, Priority::P0);
        assert!(a[0].text.contains("cheap stop"), "no stop yet at lap 10/30 → suggest box");
        let pen = PitWallEvent::Penalty { at_ms: 0, car_index: 1, driver_label: "PLA".into(), description: "Time penalty".into(), time_sec: Some(5), lap: 4 };
        assert_eq!(r.on_event(&pen, &s)[0].key, "penalty");
        let other = PitWallEvent::Penalty { at_ms: 0, car_index: 2, driver_label: "BBB".into(), description: "Time penalty".into(), time_sec: Some(5), lap: 4 };
        assert!(r.on_event(&other, &s).is_empty());
    }

    #[test]
    fn race_moments_pb_position_pit_and_finish() {
        let mut r = RuleEngine::new(RuleConfig { driver_name: Some("Alex".into()), ..Default::default() });
        let mut s = base_snapshot();
        s.timing.position = 2;
        s.timing.entries[1].lap = 5;
        s.timing.entries[1].best_lap_ms = Some(68_000);
        s.timing.entries[1].last_lap_ms = Some(68_000);
        s.timing.entries[0].best_lap_ms = Some(67_000);
        assert!(r.evaluate(&s).is_empty(), "first sample only primes memory");

        // Personal best (not the fastest overall).
        s.timing.entries[1].best_lap_ms = Some(67_500);
        s.timing.entries[1].last_lap_ms = Some(67_500);
        let a = r.evaluate(&s);
        assert!(a.iter().any(|a| a.key == "pb" && a.text.contains("1:07.5")), "{a:?}");

        // Fastest lap of the race.
        s.timing.entries[1].best_lap_ms = Some(66_900);
        s.timing.entries[1].last_lap_ms = Some(66_900);
        assert!(r.evaluate(&s).iter().any(|a| a.key == "fastest_lap" && a.text.starts_with("Purple")));

        // Gained a place.
        s.timing.position = 1;
        assert!(r.evaluate(&s).iter().any(|a| a.key == "position" && a.text.contains("P1")));

        // Pit stop done -> P1 "good stop".
        s.timing.entries[1].pit_stops = 1;
        s.tyres.compound = "H".into();
        let a = r.evaluate(&s);
        let stop = a.iter().find(|a| a.key == "pit_done").expect("good stop call");
        assert_eq!(stop.priority, Priority::P1);
        assert!(stop.text.contains("Hards"));

        // Finish line uses the result and the driver's name.
        let fin = r.on_event(&PitWallEvent::Flag { at_ms: 0, status: FlagStatus::Chequered }, &s);
        assert!(fin[0].text.contains("P1") && fin[0].text.contains("Alex"), "{}", fin[0].text);
    }

    #[test]
    fn names_and_lap_strings() {
        let e = TimingEntry { driver_name: "Max VERSTAPPEN".into(), driver_label: "VER".into(), ..Default::default() };
        assert_eq!(spoken_name(&e), "Verstappen");
        let e = TimingEntry { driver_name: "Car 7".into(), driver_label: "C07".into(), ..Default::default() };
        assert_eq!(spoken_name(&e), "Car 7");
        assert_eq!(lap_str(67_033), "1:07.0");
        assert_eq!(lap_str(92_456), "1:32.4");
    }
}
