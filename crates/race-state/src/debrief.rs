//! Post-session debrief (M7): result, pace against the field, stints and
//! degradation, sectors, fuel, and short engineer insights.
//!
//! Built from lap rows only, so it works the same from the live race (at the
//! chequered flag) and from any stored session.

use crate::state::LapRow;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Debrief {
    pub car_index: u8,
    pub driver: String,
    pub is_race: bool,
    pub total_laps: u8,
    pub result: ResultSummary,
    pub pace: PaceSummary,
    pub stints: Vec<StintSummary>,
    pub fuel: Option<FuelSummary>,
    /// Per-lap classification of the driver's laps (for charts).
    pub lap_kinds: Vec<LapKind>,
    /// Every car's pace, sorted by finishing position.
    pub field: Vec<FieldRow>,
    /// Short engineer observations, most important first.
    pub insights: Vec<Insight>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResultSummary {
    pub finish_position: u8,
    /// Position at the end of lap 1 (the closest thing to the grid we store).
    pub lap1_position: u8,
    pub positions_gained: i32,
    pub laps_completed: u8,
    pub pit_stops: u8,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PaceSummary {
    pub best_lap_ms: Option<u32>,
    /// 1 = fastest lap of the session.
    pub best_lap_rank: Option<usize>,
    /// Median of clean laps (no in/out laps, lap 1, invalid or slow outliers).
    pub median_lap_ms: Option<u32>,
    pub median_rank: Option<usize>,
    /// Standard deviation of clean laps (lower = more consistent).
    pub consistency_ms: Option<u32>,
    pub clean_laps: usize,
    pub best_sectors_ms: [Option<u32>; 3],
    pub field_best_sectors_ms: [Option<u32>; 3],
    /// Sum of the driver's best sectors.
    pub theoretical_best_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StintSummary {
    pub compound: String,
    pub start_lap: u8,
    pub end_lap: u8,
    pub laps: u8,
    pub avg_lap_ms: Option<u32>,
    /// Lap-time trend over the stint's clean laps, ms per lap (+ = getting slower).
    pub deg_ms_per_lap: Option<f32>,
    /// Worst-corner wear added per lap.
    pub wear_per_lap: Option<f32>,
    pub end_wear: Option<f32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FuelSummary {
    pub start_kg: f32,
    pub end_kg: f32,
    pub avg_burn_kg: f32,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LapKindTag {
    Clean,
    FirstLap,
    InLap,
    OutLap,
    Invalid,
    /// Much slower than normal: traffic, mistake or safety car.
    Slow,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LapKind {
    pub lap: u8,
    pub kind: LapKindTag,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FieldRow {
    pub car_index: u8,
    pub driver: String,
    pub finish_position: u8,
    pub best_lap_ms: Option<u32>,
    pub median_lap_ms: Option<u32>,
    pub pit_stops: u8,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Insight {
    /// result | pace | consistency | sectors | tyres | fuel | strategy
    pub kind: &'static str,
    pub text: String,
    /// Higher = more worth saying.
    #[serde(skip)]
    pub weight: u8,
}

fn lap_str(ms: u32) -> String {
    let t = ms / 100;
    format!("{}:{:02}.{}", t / 600, (t / 10) % 60, t % 10)
}

fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

fn median(v: &mut [u32]) -> Option<u32> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some(v[v.len() / 2])
}

/// Least-squares slope of (x, y).
fn slope(points: &[(f32, f32)]) -> Option<f32> {
    if points.len() < 3 {
        return None;
    }
    let n = points.len() as f32;
    let mx = points.iter().map(|p| p.0).sum::<f32>() / n;
    let my = points.iter().map(|p| p.1).sum::<f32>() / n;
    let sxx: f32 = points.iter().map(|p| (p.0 - mx).powi(2)).sum();
    if sxx <= f32::EPSILON {
        return None;
    }
    Some(points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f32>() / sxx)
}

/// Classify one car's laps (sorted by lap number).
fn classify(laps: &[LapRow]) -> Vec<LapKindTag> {
    let mut times: Vec<u32> = laps.iter().filter(|l| l.lap_ms > 0).map(|l| l.lap_ms).collect();
    let typical = median(&mut times).unwrap_or(u32::MAX);
    laps.iter()
        .enumerate()
        .map(|(i, l)| {
            let next_new_tyre = laps.get(i + 1).is_some_and(|n| n.compound != l.compound || n.tyre_age.zip(l.tyre_age).is_some_and(|(a, b)| a < b));
            let this_new_tyre = i > 0 && (laps[i - 1].compound != l.compound || l.tyre_age.zip(laps[i - 1].tyre_age).is_some_and(|(a, b)| a < b));
            if l.lap <= 1 {
                LapKindTag::FirstLap
            } else if next_new_tyre {
                LapKindTag::InLap
            } else if this_new_tyre {
                LapKindTag::OutLap
            } else if !l.valid {
                LapKindTag::Invalid
            } else if l.lap_ms as f32 > typical as f32 * 1.07 {
                LapKindTag::Slow
            } else {
                LapKindTag::Clean
            }
        })
        .collect()
}

fn pit_stops(laps: &[LapRow]) -> u8 {
    laps.windows(2).filter(|w| w[0].compound != w[1].compound || w[1].tyre_age.zip(w[0].tyre_age).is_some_and(|(a, b)| a < b)).count() as u8
}

fn clean_times(laps: &[LapRow], kinds: &[LapKindTag]) -> Vec<u32> {
    laps.iter().zip(kinds).filter(|(_, k)| **k == LapKindTag::Clean).map(|(l, _)| l.lap_ms).collect()
}

/// Build the debrief for `car` from every car's laps in the session.
pub fn build(all_laps: &[LapRow], car: u8, is_race: bool, total_laps: u8) -> Option<Debrief> {
    let mut by_car: BTreeMap<u8, Vec<LapRow>> = BTreeMap::new();
    for l in all_laps {
        by_car.entry(l.car_index).or_default().push(l.clone());
    }
    for v in by_car.values_mut() {
        v.sort_by_key(|l| l.lap);
    }
    let mine = by_car.get(&car)?.clone();
    if mine.is_empty() {
        return None;
    }
    let kinds = classify(&mine);

    // ---- Field.
    let mut field: Vec<FieldRow> = by_car
        .iter()
        .map(|(&ci, laps)| {
            let k = classify(laps);
            let last = laps.last().expect("non-empty");
            FieldRow {
                car_index: ci,
                driver: last.driver.clone(),
                finish_position: last.position,
                best_lap_ms: laps.iter().filter(|l| l.valid && l.lap_ms > 0).map(|l| l.lap_ms).min(),
                median_lap_ms: median(&mut clean_times(laps, &k)),
                pit_stops: pit_stops(laps),
            }
        })
        .collect();
    field.sort_by_key(|f| (f.finish_position == 0, f.finish_position));
    let me_row = field.iter().find(|f| f.car_index == car)?.clone();

    let rank_of = |val: Option<u32>, pick: fn(&FieldRow) -> Option<u32>| {
        val.map(|v| 1 + field.iter().filter(|f| f.car_index != car && pick(f).is_some_and(|x| x < v)).count())
    };
    let best_lap_rank = rank_of(me_row.best_lap_ms, |f| f.best_lap_ms);
    let median_rank = rank_of(me_row.median_lap_ms, |f| f.median_lap_ms);

    // ---- Pace + sectors.
    let clean = clean_times(&mine, &kinds);
    let consistency_ms = (clean.len() >= 3).then(|| {
        let m = clean.iter().map(|&x| x as f64).sum::<f64>() / clean.len() as f64;
        (clean.iter().map(|&x| (x as f64 - m).powi(2)).sum::<f64>() / clean.len() as f64).sqrt() as u32
    });
    let sector = |laps: &[LapRow], i: usize| {
        laps.iter()
            .filter(|l| l.valid)
            .filter_map(|l| match i {
                0 => l.s1_ms,
                1 => l.s2_ms,
                _ => l.s3_ms,
            })
            .filter(|&v| v > 0)
            .min()
    };
    let best_sectors_ms = [sector(&mine, 0), sector(&mine, 1), sector(&mine, 2)];
    let field_best_sectors_ms = [0, 1, 2].map(|i| by_car.values().filter_map(|laps| sector(laps, i)).min());
    let theoretical_best_ms = match best_sectors_ms {
        [Some(a), Some(b), Some(c)] => Some(a + b + c),
        _ => None,
    };
    let pace = PaceSummary {
        best_lap_ms: me_row.best_lap_ms,
        best_lap_rank,
        median_lap_ms: me_row.median_lap_ms,
        median_rank,
        consistency_ms,
        clean_laps: clean.len(),
        best_sectors_ms,
        field_best_sectors_ms,
        theoretical_best_ms,
    };

    // ---- Stints.
    let mut stints = Vec::new();
    let mut start = 0usize;
    for i in 0..mine.len() {
        let boundary = i + 1 == mine.len() || kinds[i] == LapKindTag::InLap;
        if !boundary {
            continue;
        }
        let seg = &mine[start..=i];
        let seg_kinds = &kinds[start..=i];
        let clean_pts: Vec<(f32, f32)> = seg.iter().zip(seg_kinds).filter(|(_, k)| **k == LapKindTag::Clean).map(|(l, _)| (l.lap as f32, l.lap_ms as f32)).collect();
        let wear_pts: Vec<(f32, f32)> = seg.iter().filter_map(|l| l.wear.map(|w| (l.lap as f32, w.iter().cloned().fold(0.0, f32::max)))).collect();
        let avg = (!clean_pts.is_empty()).then(|| (clean_pts.iter().map(|p| p.1).sum::<f32>() / clean_pts.len() as f32) as u32);
        stints.push(StintSummary {
            compound: seg.iter().rev().find_map(|l| l.compound.clone()).unwrap_or_else(|| "?".into()),
            start_lap: seg[0].lap,
            end_lap: seg[seg.len() - 1].lap,
            laps: seg.len() as u8,
            avg_lap_ms: avg,
            deg_ms_per_lap: slope(&clean_pts),
            wear_per_lap: slope(&wear_pts).filter(|s| *s >= 0.0),
            end_wear: wear_pts.last().map(|p| p.1),
        });
        start = i + 1;
    }

    // ---- Fuel.
    let fuel_pts: Vec<f32> = mine.iter().filter_map(|l| l.fuel_kg).collect();
    let fuel = (fuel_pts.len() >= 2).then(|| {
        let (a, b) = (fuel_pts[0], fuel_pts[fuel_pts.len() - 1]);
        FuelSummary { start_kg: a, end_kg: b, avg_burn_kg: ((a - b) / (fuel_pts.len() - 1) as f32).max(0.0) }
    });

    // ---- Result.
    let lap1_position = mine.first().map(|l| l.position).unwrap_or(0);
    let finish_position = me_row.finish_position;
    let result = ResultSummary {
        finish_position,
        lap1_position,
        positions_gained: lap1_position as i32 - finish_position as i32,
        laps_completed: mine.last().map(|l| l.lap).unwrap_or(0),
        pit_stops: me_row.pit_stops,
    };

    let mut d = Debrief {
        car_index: car,
        driver: me_row.driver.clone(),
        is_race,
        total_laps,
        result,
        pace,
        stints,
        fuel,
        lap_kinds: mine.iter().zip(&kinds).map(|(l, k)| LapKind { lap: l.lap, kind: *k }).collect(),
        field,
        insights: Vec::new(),
    };
    d.insights = insights(&d);
    Some(d)
}

fn insights(d: &Debrief) -> Vec<Insight> {
    let mut out = Vec::new();
    let mut add = |kind: &'static str, weight: u8, text: String| out.push(Insight { kind, text, weight });
    let r = &d.result;
    let p = &d.pace;
    let cars = d.field.len();

    if d.is_race && r.finish_position > 0 {
        let moved = match r.positions_gained {
            g if g > 0 => format!(", gained {g} from lap one"),
            g if g < 0 => format!(", lost {} from lap one", -g),
            _ => String::new(),
        };
        add("result", 10, format!("Finished P{}{moved}.", r.finish_position));
    }
    if let (Some(best), Some(rank)) = (p.best_lap_ms, p.best_lap_rank) {
        let w = if rank == 1 { 9 } else { 6 };
        let what = if rank == 1 { "fastest lap of the session".to_string() } else { format!("{} fastest of {cars}", ordinal(rank)) };
        add("pace", w, format!("Best lap {}, {what}.", lap_str(best)));
    }
    if let (Some(rank), Some(m)) = (p.median_rank, p.median_lap_ms) {
        if d.is_race && cars > 1 {
            add("pace", 7, format!("Race pace {}, {} in the field.", lap_str(m), ordinal(rank)));
        }
    }
    if let (Some(best), Some(theo)) = (p.best_lap_ms, p.theoretical_best_ms) {
        if best > theo + 150 {
            add("pace", 5, format!("{:.1} left on the table: ideal lap {}.", (best - theo) as f32 / 1000.0, lap_str(theo)));
        }
    }
    // Weakest sector relative to the field's best.
    let gaps: Vec<(usize, u32)> = (0..3).filter_map(|i| Some((i, p.best_sectors_ms[i]?.checked_sub(p.field_best_sectors_ms[i]?)?))).collect();
    if let Some(&(i, g)) = gaps.iter().max_by_key(|x| x.1) {
        if g >= 100 && gaps.len() == 3 {
            add("sectors", 6, format!("Sector {} is where the time is: {:.1} off the best.", i + 1, g as f32 / 1000.0));
        }
    }
    if let Some(c) = p.consistency_ms {
        if p.clean_laps >= 4 {
            let (w, txt) = if c <= 250 {
                (4, format!("Very consistent, laps within {:.2} of each other.", c as f32 / 1000.0))
            } else if c >= 700 {
                (6, format!("Inconsistent: lap times spread {:.1}. Tidy that up.", c as f32 / 1000.0))
            } else {
                (2, format!("Consistency {:.2} spread.", c as f32 / 1000.0))
            };
            add("consistency", w, txt);
        }
    }
    for s in &d.stints {
        if let Some(deg) = s.deg_ms_per_lap {
            if s.laps >= 4 {
                let name = compound_word(&s.compound);
                if deg >= 120.0 {
                    add("tyres", 6, format!("{name} fell away, {:.2} a lap from lap {}.", deg / 1000.0, s.start_lap));
                } else if deg <= 40.0 {
                    add("tyres", 3, format!("{name} held up well over {} laps.", s.laps));
                }
            }
        }
    }
    if let Some(f) = &d.fuel {
        if f.end_kg < 0.3 && d.is_race {
            add("fuel", 6, "Finished on fumes. Too tight on fuel.".into());
        } else if f.end_kg > 2.5 && d.is_race {
            add("fuel", 4, format!("{:.1} kilos left at the flag, we could run lighter.", f.end_kg));
        }
    }
    out.sort_by_key(|i| std::cmp::Reverse(i.weight));
    out
}

fn compound_word(c: &str) -> &'static str {
    match c {
        "S" => "Softs",
        "M" => "Mediums",
        "H" => "Hards",
        "I" => "Inters",
        "W" => "Wets",
        _ => "Tyres",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lap(car: u8, lap: u8, ms: u32, pos: u8, compound: &str, age: u8) -> LapRow {
        LapRow {
            car_index: car,
            driver: format!("Driver {car}"),
            lap,
            lap_ms: ms,
            s1_ms: Some(ms / 3),
            s2_ms: Some(ms / 3),
            s3_ms: Some(ms - 2 * (ms / 3)),
            compound: Some(compound.into()),
            tyre_age: Some(age),
            wear: Some([age as f32 * 3.0; 4]),
            fuel_kg: Some(20.0 - lap as f32 * 1.5),
            ers_harvest_j: None,
            ers_deploy_j: None,
            valid: true,
            position: pos,
            session_time: lap as f32 * 90.0,
        }
    }

    /// Car 0: 10-lap race, softs 1-5, pit on lap 5, hards 6-10, P5 -> P2.
    /// Car 1: steady and quicker, P1.
    fn race() -> Vec<LapRow> {
        let mut v = Vec::new();
        for l in 1..=10u8 {
            let (c, age) = if l <= 5 { ("S", l) } else { ("H", l - 5) };
            let mut ms = 90_000 + l as u32 * 150; // degrading softs, then hards
            if l == 5 {
                ms += 20_000; // in-lap
            }
            if l == 6 {
                ms += 3_000; // out-lap
            }
            v.push(lap(0, l, ms, if l < 6 { 5 } else { 2 }, c, age));
            v.push(lap(1, l, 89_500, 1, "M", l));
        }
        v
    }

    #[test]
    fn debrief_result_stints_and_ranks() {
        let d = build(&race(), 0, true, 10).unwrap();
        assert_eq!(d.result.finish_position, 2);
        assert_eq!(d.result.positions_gained, 3);
        assert_eq!(d.result.pit_stops, 1);
        assert_eq!(d.stints.len(), 2);
        assert_eq!((d.stints[0].compound.as_str(), d.stints[0].start_lap, d.stints[0].end_lap), ("S", 1, 5));
        assert_eq!((d.stints[1].compound.as_str(), d.stints[1].start_lap, d.stints[1].end_lap), ("H", 6, 10));
        assert!(d.stints[1].deg_ms_per_lap.unwrap() > 100.0, "lap times rise 150 ms/lap");
        let tags: Vec<_> = d.lap_kinds.iter().map(|k| k.kind).collect();
        assert_eq!(tags[0], LapKindTag::FirstLap);
        assert_eq!(tags[4], LapKindTag::InLap);
        assert_eq!(tags[5], LapKindTag::OutLap);
        assert_eq!(d.pace.best_lap_rank, Some(2), "car 1 was quicker");
        assert_eq!(d.field[0].car_index, 1, "field sorted by finish");
        assert!((d.fuel.as_ref().unwrap().avg_burn_kg - 1.5).abs() < 1e-3);
        assert_eq!(d.insights[0].text, "Finished P2, gained 3 from lap one.");
        assert!(d.insights.iter().any(|i| i.text.starts_with("Best lap 1:30.1, 2nd fastest of 2")), "{:?}", d.insights);
        assert!(d.insights.iter().any(|i| i.kind == "tyres"));
    }

    #[test]
    fn unknown_car_or_empty_is_none() {
        assert!(build(&race(), 7, true, 10).is_none());
        assert!(build(&[], 0, true, 10).is_none());
    }

    #[test]
    fn helpers() {
        assert_eq!(lap_str(90_312), "1:30.3");
        assert_eq!(ordinal(1), "1st");
        assert_eq!(ordinal(12), "12th");
        assert_eq!(ordinal(22), "22nd");
    }
}
