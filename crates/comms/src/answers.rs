//! Answering the driver's radio questions.
//!
//! Most questions on a race radio are the same dozen ("gap behind?", "how are
//! the tyres?", "fuel?", "when do we box?"). Those are answered here straight
//! from telemetry: instantly and always correct. Anything else falls through
//! to the LLM with a compact race brief ([`race_brief`]) and the engineer
//! persona ([`ENGINEER_PROMPT`]), and the reply is cleaned up for radio by
//! [`polish_reply`].

use crate::rules::{compound_name, lap_str, spoken_name};
use race_state::snapshot::*;

const CORNERS: [&str; 4] = ["front-left", "front-right", "rear-left", "rear-right"];

/// Said immediately when a question needs the LLM, so the driver knows we heard.
pub const ACK_THINKING: &str = "Copy, stand by.";
/// When the question could not be understood or answered.
pub const SAY_AGAIN: &str = "Sorry, say again? Didn't get that.";

pub const ENGINEER_PROMPT: &str = "You are the driver's race engineer on team radio during a Formula 1 session. \
You are the best in the business: calm, precise, confident and warm, British English, never flustered. \
Rules: answer in one or two short radio sentences, under 30 words. Lead with the answer, then at most one clear instruction. \
Use only facts and numbers from RACE DATA; if the data does not contain the answer, say you don't have that information. \
Round gaps to one decimal and temperatures to whole degrees. Refer to other drivers by surname or 'they', never 'he' or 'she'. \
No lists, no markdown, no emojis, no sign-offs, never mention data, telemetry or being an AI.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intent {
    RadioCheck,
    BoxDecision,
    Strategy,
    GapAhead,
    GapBehind,
    Leader,
    Tyres,
    Temps,
    Brakes,
    Fuel,
    Ers,
    Position,
    LapsLeft,
    Damage,
    Weather,
    Pace,
    Penalty,
    Flags,
}

const INTENTS: &[(Intent, &[&str])] = &[
    (Intent::RadioCheck, &["radio check", "can you hear", "do you hear", "hear me", "reading me"]),
    (Intent::BoxDecision, &["should i box", "should i pit", "do we box", "do we pit", "box now", "pit now", "box this lap", "pit this lap", "shall we box", "shall i box"]),
    (Intent::Strategy, &["box", "pit", "stop", "strategy", "window", "plan", "undercut", "overcut"]),
    (Intent::GapAhead, &["ahead", "in front", "car ahead", "front of me", "catching", "chase"]),
    (Intent::GapBehind, &["behind", "mirror", "chasing me", "catching me", "car behind", "who's behind"]),
    (Intent::Leader, &["leader", "leading", "lead"]),
    (Intent::Tyres, &["tyre", "tire", "grip", "wear", "rubber", "deg", "degradation", "softs", "mediums", "hards"]),
    (Intent::Temps, &["temp", "temperature", "overheat", "too hot", "too cold"]),
    (Intent::Brakes, &["brake", "bias"]),
    (Intent::Fuel, &["fuel", "lift and coast", "petrol", "gas", "enough to finish", "fuel save"]),
    (Intent::Ers, &["battery", "ers", "deploy", "harvest", "overtake mode", "energy", "hotlap"]),
    (Intent::Position, &["position", "where am i", "what place", "p what", "what p"]),
    (Intent::LapsLeft, &["laps left", "how many laps", "laps to go", "laps remaining", "how long left", "time left", "how long to go"]),
    (Intent::Damage, &["damage", "wing", "car ok", "car okay", "floor", "broken", "diffuser", "sidepod"]),
    (Intent::Weather, &["rain", "weather", "wet", "dry", "forecast", "inters", "intermediate", "cloud"]),
    (Intent::Pace, &["pace", "lap time", "last lap", "how fast", "quicker", "slower", "best lap", "my best", "purple"]),
    (Intent::Penalty, &["penalty", "penalties", "warning", "track limits", "investigation"]),
    (Intent::Flags, &["safety car", "vsc", "flag", "yellow", "restart"]),
];

/// Phrasings that ask for reasoning rather than a reading; these go to the
/// LLM when one is available.
const OPEN_ENDED: &[&str] = &[" if ", " what if", " why", " how do i", " how should", " how can i", " what should", " explain", " advice", " tips", " where am i losing", " where can i", " what would", " compare", " versus", " vs ", " plan b", " opinion", " think"];

fn is_open_ended(q: &str) -> bool {
    let n = normalize(q);
    // "should I box" is a decision we answer directly.
    !n.contains(" should i box") && !n.contains(" should i pit") && OPEN_ENDED.iter().any(|k| n.contains(k))
}

fn wants_to_finish(q: &str) -> bool {
    let n = normalize(q);
    [" end", " finish", " make it", " last until", " go to the end", " all the way"].iter().any(|k| n.contains(k))
}

fn normalize(q: &str) -> String {
    let s: String = q.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == '\'' || c == ' ' { c } else { ' ' }).collect();
    format!(" {} ", s.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn classify(q: &str) -> Vec<Intent> {
    let n = normalize(q);
    let mut scored: Vec<(Intent, usize, usize)> = INTENTS
        .iter()
        .enumerate()
        .filter_map(|(order, (intent, kws))| {
            let hits = kws.iter().filter(|k| n.contains(&format!(" {k}")) || (k.contains(' ') && n.contains(*k))).count();
            (hits > 0).then_some((*intent, hits, order))
        })
        .collect();
    // A box decision subsumes the generic strategy intent.
    if scored.iter().any(|x| x.0 == Intent::BoxDecision) {
        scored.retain(|x| x.0 != Intent::Strategy);
    }
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
    scored.into_iter().take(2).map(|x| x.0).collect()
}

fn secs(ms: u32) -> String {
    format!("{:.1}", ms as f32 / 1000.0)
}

fn player(snap: &TelemetrySnapshot) -> Option<(usize, &TimingEntry)> {
    snap.timing.entries.iter().enumerate().find(|(_, e)| e.is_player)
}

fn pace_compare(me: &TimingEntry, other: &TimingEntry) -> String {
    match (me.last_lap_ms, other.last_lap_ms) {
        (Some(a), Some(b)) if a > 0 && b > 0 => {
            let d = (b as i64 - a as i64) as f32 / 1000.0;
            if d.abs() < 0.05 {
                " Same pace last lap.".into()
            } else if d > 0.0 {
                format!(" You were {d:.1} quicker last lap.")
            } else {
                format!(" They were {:.1} quicker last lap.", -d)
            }
        }
        _ => String::new(),
    }
}

/// Worst corner index and value.
fn worst(w: &[f32; 4]) -> (usize, f32) {
    w.iter().copied().enumerate().fold((0, f32::MIN), |acc, (i, v)| if v > acc.1 { (i, v) } else { acc })
}

fn should_box(snap: &TelemetrySnapshot) -> (bool, String) {
    let t = &snap.tyres;
    let (wi, w) = worst(&t.wear_pct);
    let slick = matches!(t.compound.as_str(), "S" | "M" | "H");
    let raining = matches!(snap.session.weather.as_str(), "lightRain" | "heavyRain" | "storm");
    let d = &snap.damage;
    let wing = d.front_left_wing_pct.max(d.front_right_wing_pct);
    if raining && slick {
        return (true, "Yes, box this lap for inters. It's raining.".into());
    }
    if !raining && matches!(t.compound.as_str(), "I" | "W") && snap.session.weather_forecast.iter().filter(|f| f.time_offset_min <= 10).all(|f| f.rain_pct < 30) {
        return (true, "Track's drying. Box this lap for slicks if the line is dry.".into());
    }
    if wing >= 30 {
        return (true, format!("Yes, box this lap. Front wing damage is {wing} percent."));
    }
    if w >= 65.0 {
        return (true, format!("Yes, box this lap. {} is at {w:.0} percent.", capitalize(CORNERS[wi])));
    }
    if let Some(l) = t.projected_pit_lap {
        if snap.session.lap + 1 >= l {
            return (true, format!("Box this lap, box this lap. Tyres are done, {} at {w:.0} percent.", CORNERS[wi]));
        }
        return (false, format!("Negative, stay out. Tyres are good to about lap {l}."));
    }
    (false, format!("Negative, stay out. Tyres are fine, worst is {w:.0} percent."))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn answer_intent(intent: Intent, snap: &TelemetrySnapshot, question: &str, also: &[Intent]) -> Option<String> {
    let s = &snap.session;
    let t = &snap.tyres;
    let p = &snap.powertrain;
    let (pi, me) = player(snap)?;
    let entries = &snap.timing.entries;
    Some(match intent {
        Intent::RadioCheck => "Loud and clear.".into(),
        Intent::BoxDecision => should_box(snap).1,
        Intent::Strategy => {
            let win = s.pit_window.as_ref().filter(|w| w.ideal_lap > 0).map(|w| format!(" Game window is lap {} to {}, rejoin around P{}.", w.ideal_lap, w.latest_lap, w.rejoin_position)).unwrap_or_default();
            match t.projected_pit_lap {
                Some(l) => format!("We're looking at lap {l} for the stop.{win}"),
                None if me.pit_stops > 0 => format!("No more stops planned. Bring it home on the {}.", compound_name(&t.compound)),
                None => format!("Tyres are fine, no stop yet.{win}"),
            }
        }
        Intent::GapAhead => {
            if snap.timing.position == 1 {
                let behind = entries.get(pi + 1);
                match (behind, snap.timing.gap_behind_ms) {
                    (Some(b), Some(g)) => format!("You're leading. {} is {} behind.", spoken_name(b), secs(g)),
                    _ => "You're leading.".into(),
                }
            } else {
                let ahead = entries.get(pi.checked_sub(1)?)?;
                match snap.timing.gap_ahead_ms {
                    Some(g) => format!("{} ahead, gap is {}.{}", spoken_name(ahead), secs(g), pace_compare(me, ahead)),
                    None => format!("{} is ahead. No gap reading yet.", spoken_name(ahead)),
                }
            }
        }
        Intent::GapBehind => match (entries.get(pi + 1), snap.timing.gap_behind_ms) {
            (Some(b), Some(g)) => format!("{} behind, {}.{}", spoken_name(b), secs(g), pace_compare(me, b)),
            (Some(b), None) => format!("{} is behind. No gap reading yet.", spoken_name(b)),
            (None, _) => "Nobody behind you.".into(),
        },
        Intent::Leader => match (snap.timing.position, me.gap_to_leader_ms, entries.first()) {
            (1, _, _) => "You're leading the race.".into(),
            (_, Some(g), Some(l)) => format!("{} is leading, you're {} behind.", spoken_name(l), secs(g)),
            _ => "No leader gap yet.".into(),
        },
        Intent::Tyres => {
            let (wi, w) = worst(&t.wear_pct);
            let rate = t.wear_rate_per_lap[wi].filter(|r| *r > 0.0).map(|r| format!(", about {r:.1} a lap")).unwrap_or_default();
            if wants_to_finish(question) && s.is_race {
                return Some(match t.projected_pit_lap {
                    Some(l) if l < s.total_laps => format!("Not to the end, no. They're good to about lap {l}, {} at {w:.0} percent{rate}.", CORNERS[wi]),
                    _ => format!("Yes, they'll make the end. Worst is the {} at {w:.0} percent{rate}.", CORNERS[wi]),
                });
            }
            let proj = t.projected_pit_lap.map(|l| format!(" Good to about lap {l}.")).unwrap_or_default();
            let age = match t.age_laps {
                0 => "brand new".to_string(),
                1 => "1 lap old".to_string(),
                n => format!("{n} laps old"),
            };
            format!("{} are {age}. Worst is the {} at {w:.0} percent{rate}.{proj}", capitalize(compound_name(&t.compound)), CORNERS[wi])
        }
        Intent::Temps => {
            let st = t.surface_temp_c;
            let verdict = if st.iter().any(|&x| x >= 110) {
                " Running hot, cool them on the straights."
            } else if st.iter().all(|&x| x < 80) && t.compound != "W" && t.compound != "I" {
                " A bit cold, push to get heat in."
            } else {
                " That's a good window."
            };
            format!("Surface temps: fronts {} and {}, rears {} and {}.{verdict}", st[0], st[1], st[2], st[3])
        }
        Intent::Brakes => {
            let b = t.brake_temp_c.iter().copied().max().unwrap_or(0);
            let verdict = if b >= 1000 { " That's hot, bias rearwards." } else if b < 300 { " They're cold, lean on them." } else { " All good." };
            format!("Brakes peaking {b} degrees.{verdict}")
        }
        Intent::Fuel => {
            let fd = p.fuel_remaining_laps_delta;
            let verdict = if fd <= -0.6 {
                " That's critical, heavy lift and coast."
            } else if fd <= -0.2 {
                " Some lift and coast please."
            } else if fd >= 0.5 {
                " Plenty, you can use it."
            } else {
                " You're fine, keep pushing."
            };
            let sign = if fd >= 0.0 { "plus" } else { "minus" };
            format!("Fuel is {sign} {:.1} laps.{verdict}", fd.abs())
        }
        Intent::Ers => format!("Battery is {:.0} percent, {} mode.", p.ers_store_pct, p.ers_deploy_mode.to_lowercase()),
        Intent::Position => {
            if s.is_race {
                format!("P{}, lap {} of {}.", snap.timing.position, s.lap, s.total_laps)
            } else {
                format!("You're P{} on the timesheet.", snap.timing.position)
            }
        }
        Intent::LapsLeft => {
            if s.is_race {
                let n = s.total_laps.saturating_sub(s.lap) + 1;
                if n <= 1 {
                    "This is the last lap.".into()
                } else {
                    format!("{n} laps to go, including this one.")
                }
            } else {
                let m = s.time_left_ms / 60_000;
                format!("{m} minutes left in the session.")
            }
        }
        Intent::Damage => {
            let d = &snap.damage;
            let parts = [
                ("front-left wing", d.front_left_wing_pct),
                ("front-right wing", d.front_right_wing_pct),
                ("rear wing", d.rear_wing_pct),
                ("floor", d.floor_pct),
                ("diffuser", d.diffuser_pct),
                ("sidepods", d.sidepod_pct),
            ];
            let mut hurt: Vec<_> = parts.iter().filter(|x| x.1 >= 5).collect();
            hurt.sort_by_key(|x| std::cmp::Reverse(x.1));
            match hurt.first() {
                None => "No damage. Car looks good.".into(),
                Some((n, v)) if *v >= 30 => format!("{v} percent {n} damage. That's costing you, box for repairs."),
                Some((n, v)) => format!("{v} percent {n} damage. Car is fine, carry on."),
            }
        }
        Intent::Weather => {
            let now = match s.weather.as_str() {
                "lightRain" => "Light rain now",
                "heavyRain" => "Heavy rain now",
                "storm" => "Storm conditions now",
                _ => "Dry at the moment",
            };
            let next = s.weather_forecast.iter().filter(|f| f.time_offset_min > 0 && f.time_offset_min <= 30).max_by_key(|f| f.rain_pct);
            let fc = match next {
                Some(f) if f.rain_pct >= 40 => format!(" Rain {} percent in about {} minutes.", f.rain_pct, f.time_offset_min),
                Some(_) => " No rain expected in the next half hour.".into(),
                None => String::new(),
            };
            format!("{now}, track {} degrees.{fc}", s.track_temp_c)
        }
        Intent::Pace => match (me.last_lap_ms, me.best_lap_ms) {
            (Some(l), Some(b)) if l > 0 => {
                let ahead = pi.checked_sub(1).and_then(|i| entries.get(i));
                // The gap answer already compared pace with the car ahead.
                let cmp = if also.contains(&Intent::GapAhead) {
                    String::new()
                } else {
                    ahead.map(|a| pace_compare(me, a).replace("They were", &format!("{} was", spoken_name(a)))).unwrap_or_default()
                };
                if l == b {
                    format!("Last lap {}, that's your best.{cmp}", lap_str(l))
                } else {
                    format!("Last lap {}, best is {}.{cmp}", lap_str(l), lap_str(b))
                }
            }
            _ => "No lap time yet. Complete a lap first.".into(),
        },
        Intent::Penalty => match (me.penalties_ms, me.warnings) {
            (0, 0) => "No penalties, no warnings. All clean.".into(),
            (0, w) => format!("No penalties. {w} track limits warning{}, be careful.", if w == 1 { "" } else { "s" }),
            (pen, w) => format!("{} seconds of penalties, {w} warning{}.", pen / 1000, if w == 1 { "" } else { "s" }),
        },
        Intent::Flags => match s.safety_car_status.as_str() {
            "full" => "Safety car is out. Stay above the delta.".into(),
            "virtual" => "VSC is out. Keep it positive to the delta.".into(),
            _ if s.marshal_zones.iter().any(|z| z.flag == "yellow") => "Yellow flags out there, careful.".into(),
            _ => "Track is green, no flags.".into(),
        },
    })
}

/// Answer a question from telemetry, or `None` if it needs the LLM.
/// With `llm_available`, open-ended questions ("what if...", "why...") are
/// left to the model even when they mention a known topic.
pub fn answer(question: &str, snap: &TelemetrySnapshot, llm_available: bool) -> Option<String> {
    if llm_available && is_open_ended(question) {
        return None;
    }
    let intents = classify(question);
    if intents.is_empty() || snap.timing.entries.is_empty() {
        return None;
    }
    let parts: Vec<String> = intents.iter().filter_map(|&i| answer_intent(i, snap, question, &intents)).collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Compact snapshot summary for the LLM (a few hundred tokens).
pub fn race_brief(snap: &TelemetrySnapshot) -> String {
    let s = &snap.session;
    let t = &snap.tyres;
    let p = &snap.powertrain;
    let d = &snap.damage;
    let mut b = String::new();
    let mut line = |k: &str, v: String| {
        b.push_str(k);
        b.push_str(": ");
        b.push_str(&v);
        b.push('\n');
    };
    line("Session", format!("{} at {}, lap {} of {}, phase {}", s.session_type, s.track_name, s.lap, s.total_laps, s.phase));
    line("Flags", format!("safety car {}, yellow zones {}", s.safety_car_status, s.marshal_zones.iter().filter(|z| z.flag == "yellow").count()));
    if let Some((pi, me)) = player(snap) {
        line("Driver", format!("P{}, pit stops {}, penalties {}s, warnings {}", snap.timing.position, me.pit_stops, me.penalties_ms / 1000, me.warnings));
        line(
            "Laps",
            format!("last {}, best {}", me.last_lap_ms.map(lap_str).unwrap_or("-".into()), me.best_lap_ms.map(lap_str).unwrap_or("-".into())),
        );
        let fmt_car = |e: &TimingEntry, gap: Option<u32>| {
            format!(
                "{} P{}, gap {}s, last lap {}, tyre {} {} laps",
                spoken_name(e),
                e.position,
                gap.map(secs).unwrap_or("-".into()),
                e.last_lap_ms.map(lap_str).unwrap_or("-".into()),
                e.compound.clone().unwrap_or("?".into()),
                e.tyre_age_laps.map(|a| a.to_string()).unwrap_or("?".into())
            )
        };
        if let Some(a) = pi.checked_sub(1).and_then(|i| snap.timing.entries.get(i)) {
            line("Car ahead", fmt_car(a, snap.timing.gap_ahead_ms));
        }
        if let Some(bh) = snap.timing.entries.get(pi + 1) {
            line("Car behind", fmt_car(bh, snap.timing.gap_behind_ms));
        }
        if let Some(l) = snap.timing.entries.first().filter(|_| snap.timing.position > 1) {
            line("Leader", format!("{}, {}s ahead of us", spoken_name(l), me.gap_to_leader_ms.map(secs).unwrap_or("-".into())));
        }
    }
    line(
        "Tyres",
        format!(
            "{} ({}), {} laps old, wear FL {:.0}% FR {:.0}% RL {:.0}% RR {:.0}%, surface temps {:?}C, projected stop lap {}",
            compound_name(&t.compound),
            t.actual_compound,
            t.age_laps,
            t.wear_pct[0],
            t.wear_pct[1],
            t.wear_pct[2],
            t.wear_pct[3],
            t.surface_temp_c,
            t.projected_pit_lap.map(|l| l.to_string()).unwrap_or("none".into())
        ),
    );
    if let Some(w) = &s.pit_window {
        line("Game pit window", format!("lap {} to {}, rejoin P{}", w.ideal_lap, w.latest_lap, w.rejoin_position));
    }
    line(
        "Fuel and ERS",
        format!("fuel {:+.1} laps vs target, {:.1} kg, battery {:.0}% {} mode", p.fuel_remaining_laps_delta, p.fuel_remaining_kg, p.ers_store_pct, p.ers_deploy_mode),
    );
    line(
        "Damage",
        format!(
            "FL wing {}%, FR wing {}%, rear wing {}%, floor {}%, diffuser {}%, sidepods {}%, DRS fault {}, brakes peak {}C",
            d.front_left_wing_pct,
            d.front_right_wing_pct,
            d.rear_wing_pct,
            d.floor_pct,
            d.diffuser_pct,
            d.sidepod_pct,
            d.drs_fault,
            t.brake_temp_c.iter().max().unwrap_or(&0)
        ),
    );
    let fc: Vec<String> = s.weather_forecast.iter().filter(|f| f.time_offset_min <= 30).map(|f| format!("+{}min {} rain {}%", f.time_offset_min, f.weather, f.rain_pct)).collect();
    line("Weather", format!("{} now, air {}C, track {}C; forecast {}", s.weather, s.air_temp_c, s.track_temp_c, fc.join(", ")));
    b
}

/// The user message for the LLM.
pub fn llm_user_message(question: &str, snap: &TelemetrySnapshot) -> String {
    format!("RACE DATA\n{}\nDRIVER (on the radio): \"{}\"\nReply as the race engineer:", race_brief(snap), question.trim())
}

/// Make an LLM reply radio-ready: plain text, at most two sentences.
pub fn polish_reply(raw: &str) -> Option<String> {
    let mut t: String = raw.chars().filter(|c| !matches!(c, '*' | '#' | '`' | '_' | '"')).collect();
    for prefix in ["Engineer:", "Race engineer:", "Radio:", "Reply:"] {
        if let Some(rest) = t.trim_start().strip_prefix(prefix) {
            t = rest.to_string();
        }
    }
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    // Sentence ends are ". ", "! ", "? " or end of text (not the "." in "1.6").
    let chars: Vec<char> = t.chars().collect();
    let mut out = String::new();
    let mut sentences = 0;
    for (i, &ch) in chars.iter().enumerate() {
        out.push(ch);
        if matches!(ch, '.' | '!' | '?') && chars.get(i + 1).is_none_or(|n| n.is_whitespace()) {
            sentences += 1;
            if sentences == 2 {
                break;
            }
        }
    }
    let out = out.trim().to_string();
    if out.is_empty() {
        return None;
    }
    if out.chars().count() > 260 {
        let cut: String = out.chars().take(260).collect();
        let end = cut.rfind([',', ' ']).unwrap_or(cut.len());
        return Some(format!("{}.", cut[..end].trim_end_matches(',')));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> TelemetrySnapshot {
        let mut s = TelemetrySnapshot::default();
        s.session.is_race = true;
        s.session.lap = 12;
        s.session.total_laps = 30;
        s.session.weather = "clear".into();
        s.session.track_temp_c = 34;
        s.session.safety_car_status = "none".into();
        s.timing.position = 5;
        s.timing.player_index = 4;
        s.timing.gap_ahead_ms = Some(1640);
        s.timing.gap_behind_ms = Some(820);
        s.tyres.compound = "M".into();
        s.tyres.age_laps = 11;
        s.tyres.wear_pct = [34.0, 28.0, 25.0, 24.0];
        s.tyres.wear_rate_per_lap = [Some(2.6), Some(2.1), None, None];
        s.tyres.projected_pit_lap = Some(24);
        s.tyres.surface_temp_c = [98, 96, 93, 92];
        s.powertrain.fuel_remaining_laps_delta = -0.3;
        s.powertrain.ers_store_pct = 64.0;
        s.powertrain.ers_deploy_mode = "Medium".into();
        s.timing.entries = (0..7)
            .map(|i| TimingEntry {
                car_index: i,
                position: i + 1,
                is_player: i == 4,
                driver_name: ["Max VERSTAPPEN", "Lando NORRIS", "Charles LECLERC", "Oscar PIASTRI", "Player", "Lewis HAMILTON", "George RUSSELL"][i as usize].into(),
                driver_label: "XXX".into(),
                last_lap_ms: Some(if i == 4 { 67_000 } else { 67_300 }),
                best_lap_ms: Some(66_800),
                pit_status: "none".into(),
                result_status: "active".into(),
                ..Default::default()
            })
            .collect();
        s
    }

    fn answer_t(q: &str, s: &TelemetrySnapshot) -> Option<String> {
        answer(q, s, false)
    }

    #[test]
    fn open_ended_goes_to_the_llm_when_available() {
        let s = snap();
        let q = "What's the plan if the safety car comes out?";
        assert!(answer(q, &s, true).is_none());
        assert!(answer(q, &s, false).is_some(), "without an LLM we still say something useful");
        assert!(answer("should I box this lap?", &s, true).is_some(), "decisions stay deterministic");
    }

    #[test]
    fn tyres_to_the_end_and_no_duplicate_pace() {
        let s = snap();
        assert!(answer_t("can we make it to the end on these tyres?", &s).unwrap().starts_with("Not to the end, no. They're good to about lap 24"));
        let a = answer_t("Am I quicker than the guy in front?", &s).unwrap();
        assert_eq!(a.matches("quicker last lap").count(), 1, "{a}");
    }

    #[test]
    fn telemetry_answers() {
        let s = snap();
        assert_eq!(answer_t("What's the gap ahead?", &s).unwrap(), "Piastri ahead, gap is 1.6. You were 0.3 quicker last lap.");
        assert!(answer_t("who's behind me", &s).unwrap().starts_with("Hamilton behind, 0.8."));
        let tyres = answer_t("how are my tyres looking", &s).unwrap();
        assert!(tyres.contains("Mediums are 11 laps old") && tyres.contains("front-left at 34 percent") && tyres.contains("2.6 a lap") && tyres.contains("lap 24"), "{tyres}");
        assert_eq!(answer_t("fuel?", &s).unwrap(), "Fuel is minus 0.3 laps. Some lift and coast please.");
        assert_eq!(answer_t("Radio check", &s).unwrap(), "Loud and clear.");
        assert_eq!(answer_t("how many laps to go", &s).unwrap(), "19 laps to go, including this one.");
        assert!(answer_t("should I box this lap?", &s).unwrap().starts_with("Negative, stay out"));
        assert!(answer_t("tell me a joke", &s).is_none(), "falls through to the LLM");
    }

    #[test]
    fn box_decision_says_yes_when_needed() {
        let mut s = snap();
        s.tyres.wear_pct[0] = 71.0;
        assert!(answer_t("do we box?", &s).unwrap().starts_with("Yes, box this lap"));
        let mut s = snap();
        s.session.weather = "lightRain".into();
        assert!(answer_t("should i pit", &s).unwrap().contains("inters"));
    }

    #[test]
    fn two_intents_are_combined() {
        let a = answer_t("gap behind and fuel", &snap()).unwrap();
        assert!(a.contains("Hamilton") && a.contains("Fuel is minus"), "{a}");
    }

    #[test]
    fn brief_and_polish() {
        let b = race_brief(&snap());
        assert!(b.contains("Car ahead: Piastri P4") && b.contains("Car behind: Hamilton P6") && b.contains("projected stop lap 24"), "{b}");
        assert_eq!(polish_reply("**Engineer:** Gap is 1.6.  Keep pushing! Extra sentence here."), Some("Gap is 1.6. Keep pushing!".into()));
        assert_eq!(polish_reply("   "), None);
    }
}
