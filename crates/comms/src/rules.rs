//! Deterministic race-engineer rule engine.
//!
//! Every rule reads the current snapshot (plus a little memory for trends)
//! and yields [`Alert`]s with a stable `key`. The arbitrator decides whether
//! and when an alert is spoken (cooldowns, gating, Copilot approval). No LLM
//! is involved, so a callout can only ever state values from telemetry.

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
}

impl Default for RuleConfig {
    fn default() -> Self {
        Self { defend_gap_ms: 1000, push_gap_max_ms: 3000, rain_warn_pct: 60, rain_lookahead_min: 15 }
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
        Self { cfg, variant: HashMap::new(), gap_behind_hist: Vec::new(), gap_ahead_hist: Vec::new() }
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
                            "Safety car, safety car. Stay above the delta.{}",
                            if cheap_stop { " Box this lap for a cheap stop if you want it." } else { "" }
                        )],
                    ),
                    FlagStatus::Vsc => (
                        "flag_vsc",
                        vec![format!("Virtual safety car deployed. Keep it positive to the delta.{}", if cheap_stop { " Consider boxing." } else { "" })],
                    ),
                    FlagStatus::ScEnding => ("flag_sc_end", vec!["Safety car in this lap. Get ready for the restart.".into()]),
                    FlagStatus::VscEnding => ("flag_vsc_end", vec!["VSC ending. Be ready to go.".into()]),
                    FlagStatus::Green => ("flag_green", vec!["Green flag, green flag. Push now.".into(), "Track is green. Go, go, go.".into()]),
                    FlagStatus::Red => ("flag_red", vec!["Red flag, red flag. Slow down and return to the pits.".into()]),
                    FlagStatus::Chequered => ("flag_chequered", vec!["Chequered flag. That's the race, great job.".into()]),
                };
                out.push(self.alert(key, Priority::P0, AlertLevel::Critical, "flags", &variants, vec![]));
            }
            PitWallEvent::Penalty { car_index, description, time_sec, .. } if *car_index == player => {
                let txt = match time_sec {
                    Some(t) => format!("We have a {t} second penalty. {description}."),
                    None => format!("Penalty: {description}."),
                };
                out.push(self.alert("penalty", Priority::P0, AlertLevel::Critical, "penalty", &[txt], vec![description.clone()]));
            }
            _ => {}
        }
        out
    }

    /// Level-based alerts from the current snapshot.
    pub fn evaluate(&mut self, snap: &TelemetrySnapshot) -> Vec<Alert> {
        let mut out = Vec::new();
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
                let pit = t.projected_pit_lap.map(|l| format!(" Plan to box by lap {l}.")).unwrap_or_default();
                let v = vec![format!("{corner} tyre is at {w:.0} percent, that's critical.{pit}"), format!("Tyres are going off. {corner} at {w:.0} percent.{pit}")];
                out.push(self.alert("tyre_wear", Priority::P2, AlertLevel::Critical, "tyres", &v, att));
            } else if w >= 50.0 {
                let v = vec![format!("{corner} wear is {w:.0} percent. Look after it."), format!("Watch the {corner}, {w:.0} percent worn.")];
                out.push(self.alert("tyre_wear", Priority::P2, AlertLevel::Watch, "tyres", &v, att));
            }
        }

        // ---- Tyres won't make the game's pit window.
        if let (Some(proj), Some(win)) = (t.projected_pit_lap, &s.pit_window) {
            let pitted = snap.timing.entries.iter().find(|e| e.is_player).map(|e| e.pit_stops).unwrap_or(0) > 0;
            if !pitted && s.is_race && proj + 1 < win.ideal_lap && s.lap + 1 < win.ideal_lap {
                let v = vec![format!("Tyres won't make the planned window on lap {}. Projection says lap {proj}.", win.ideal_lap)];
                out.push(self.alert("tyre_window", Priority::P2, AlertLevel::Watch, "strategy", &v, vec![format!("Projected pit lap {proj}")]));
            }
        }

        // ---- Tyre surface temperature.
        let fronts_hot = t.surface_temp_c[0].max(t.surface_temp_c[1]);
        if fronts_hot >= 112 {
            let v = vec!["Fronts are overheating. Cool them on the straight.".to_string(), format!("Front tyres at {fronts_hot} degrees, manage the temps.")];
            out.push(self.alert("tyre_temp", Priority::P2, AlertLevel::Watch, "tyres", &v, vec![format!("Front surface {fronts_hot}°C")]));
        }

        // ---- Fuel.
        let fd = p.fuel_remaining_laps_delta;
        if snap.session.is_race && fd <= -0.2 && p.fuel_remaining_kg > 0.0 {
            let att = vec![format!("Fuel {fd:+.1} laps")];
            if fd <= -0.6 {
                let v = vec![format!("Fuel is critical, minus {:.1} laps. Heavy lift and coast now.", -fd)];
                out.push(self.alert("fuel", Priority::P2, AlertLevel::Critical, "fuel", &v, att));
            } else {
                let v = vec![format!("Fuel is minus {:.1} laps. Start some lift and coast.", -fd), format!("We're {:.1} laps short on fuel. Lift and coast into the big stops.", -fd)];
                out.push(self.alert("fuel", Priority::P2, AlertLevel::Watch, "fuel", &v, att));
            }
        }

        // ---- ERS.
        if p.ers_store_pct > 0.0 && p.ers_store_pct < 10.0 && s.phase == "racing" {
            let v = vec![format!("Battery is low, {:.0} percent. Harvest this lap.", p.ers_store_pct)];
            out.push(self.alert("ers_low", Priority::P2, AlertLevel::Watch, "ers", &v, vec![format!("ERS {:.0}%", p.ers_store_pct)]));
        }

        // ---- Powertrain failures.
        if p.ers_fault || p.engine_blown || p.engine_seized {
            let msg = if p.ers_fault { "We have an ERS fault. Manage the car." } else { "Engine failure. Pull over safely." };
            out.push(self.alert("pu_failure", Priority::P0, AlertLevel::Critical, "powertrain", &[msg.to_string()], vec![]));
        }

        // ---- Aero damage.
        let wings = [("front-left wing", d.front_left_wing_pct), ("front-right wing", d.front_right_wing_pct), ("rear wing", d.rear_wing_pct), ("floor", d.floor_pct), ("diffuser", d.diffuser_pct)];
        if let Some(&(name, val)) = wings.iter().max_by_key(|x| x.1) {
            if val >= 30 {
                let v = vec![format!("We have {val} percent {name} damage. Box for repairs.")];
                out.push(self.alert("damage", Priority::P2, AlertLevel::Critical, "damage", &v, vec![format!("{name} {val}%")]));
            } else if val >= 10 {
                let v = vec![format!("Confirming {name} damage, {val} percent. Car is driveable, keep going.")];
                out.push(self.alert("damage", Priority::P2, AlertLevel::Watch, "damage", &v, vec![format!("{name} {val}%")]));
            }
        }
        if d.drs_fault {
            out.push(self.alert("drs_fault", Priority::P2, AlertLevel::Watch, "damage", &["DRS fault. Don't rely on DRS.".to_string()], vec![]));
        }
        let brake_max = t.brake_temp_c.iter().copied().max().unwrap_or(0);
        if brake_max >= 1000 {
            let v = vec![format!("Brake temps are high, {brake_max} degrees. Shift the bias or lift earlier.")];
            out.push(self.alert("brakes", Priority::P2, AlertLevel::Watch, "brakes", &v, vec![format!("Brakes {brake_max}°C")]));
        }

        // ---- Weather.
        let slick = matches!(t.compound.as_str(), "S" | "M" | "H");
        let raining_now = matches!(s.weather.as_str(), "lightRain" | "heavyRain" | "storm");
        if slick && raining_now {
            let v = vec!["It's raining. Box for inters if the grip is gone.".to_string()];
            out.push(self.alert("rain_now", Priority::P2, AlertLevel::Critical, "weather", &v, vec![format!("Weather {}", s.weather)]));
        } else if slick {
            if let Some(f) = s.weather_forecast.iter().filter(|f| f.time_offset_min > 0 && f.time_offset_min <= self.cfg.rain_lookahead_min).find(|f| f.rain_pct >= self.cfg.rain_warn_pct) {
                let v = vec![format!("Rain expected in about {} minutes, {} percent chance.", f.time_offset_min, f.rain_pct)];
                out.push(self.alert("rain_soon", Priority::P2, AlertLevel::Watch, "weather", &v, vec![format!("Rain {}% in {} min", f.rain_pct, f.time_offset_min)]));
            }
        }

        // ---- Gaps (race only, green flag).
        if s.is_race && s.safety_car_status == "none" && s.phase == "racing" {
            let player = snap.timing.entries.iter().position(|e| e.is_player);
            if let Some(pi) = player {
                if let Some(gb) = snap.timing.gap_behind_ms {
                    push_hist(&mut self.gap_behind_hist, now, gb);
                    let closing = trend(&self.gap_behind_hist, now, 15_000).map(|d| d < -100).unwrap_or(false);
                    if gb <= self.cfg.defend_gap_ms && closing {
                        let label = snap.timing.entries.get(pi + 1).map(|e| e.driver_label.clone()).unwrap_or_default();
                        let v = vec![format!("{label} is {} behind and closing. Defend.", fmt_secs(gb)), format!("Car behind within a second, {label}. Cover the inside.")];
                        out.push(self.alert("defend", Priority::P2, AlertLevel::Watch, "gap", &v, vec![format!("Gap behind {}s", fmt_secs(gb))]));
                    }
                }
                if let Some(ga) = snap.timing.gap_ahead_ms {
                    push_hist(&mut self.gap_ahead_hist, now, ga);
                    let closing = trend(&self.gap_ahead_hist, now, 30_000).map(|d| d < -250).unwrap_or(false);
                    if ga > 1000 && ga <= self.cfg.push_gap_max_ms && closing && pi > 0 {
                        let label = snap.timing.entries[pi - 1].driver_label.clone();
                        let v = vec![format!("Gap to {label} is {}, you're catching. Push.", fmt_secs(ga))];
                        out.push(self.alert("push", Priority::P2, AlertLevel::Nominal, "gap", &v, vec![format!("Gap ahead {}s", fmt_secs(ga))]));
                    }
                }
            }
        }
        out
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
            TimingEntry { car_index: 0, position: 1, driver_label: "AAA".into(), ..Default::default() },
            TimingEntry { car_index: 1, position: 2, driver_label: "PLA".into(), is_player: true, ..Default::default() },
            TimingEntry { car_index: 2, position: 3, driver_label: "BBB".into(), ..Default::default() },
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
        assert!(a[0].text.contains("front-left"));
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
}
