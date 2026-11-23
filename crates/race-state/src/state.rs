//! Live race state: ingests decoded packets, tracks per-car history and
//! derived metrics, emits state events and builds wire snapshots.

use crate::metrics;
use crate::snapshot::*;
use crate::timing::TimingLines;
use f1_udp::lookup::*;
use f1_udp::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Thresholds {
    pub tyre_wear_watch: f32,
    pub tyre_wear_critical: f32,
    /// Wear at which a stop is planned (pit lap projection).
    pub pit_wear_target: f32,
    pub tyre_surface_temp_watch: u8,
    pub brake_temp_watch: u16,
    pub fuel_delta_watch: f32,
    pub fuel_delta_critical: f32,
    pub ers_low_pct: f32,
    pub component_wear_watch: u8,
    pub aero_damage_watch: u8,
    pub aero_damage_critical: u8,
    pub timing_line_spacing_m: f32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            tyre_wear_watch: 50.0,
            tyre_wear_critical: 65.0,
            pit_wear_target: 70.0,
            tyre_surface_temp_watch: 112,
            brake_temp_watch: 1000,
            fuel_delta_watch: -0.2,
            fuel_delta_critical: -0.6,
            ers_low_pct: 10.0,
            component_wear_watch: 70,
            aero_damage_watch: 10,
            aero_damage_critical: 30,
            timing_line_spacing_m: 50.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub session_uid: String,
    pub game_year: u8,
    pub track_id: i8,
    pub track_name: String,
    pub session_type: String,
    pub total_laps: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LapRow {
    pub car_index: u8,
    pub driver: String,
    pub lap: u8,
    pub lap_ms: u32,
    pub s1_ms: Option<u32>,
    pub s2_ms: Option<u32>,
    pub s3_ms: Option<u32>,
    pub compound: Option<String>,
    pub tyre_age: Option<u8>,
    pub wear: Option<[f32; 4]>,
    pub fuel_kg: Option<f32>,
    pub ers_harvest_j: Option<f32>,
    pub ers_deploy_j: Option<f32>,
    pub valid: bool,
    pub position: u8,
    pub session_time: f32,
}

#[derive(Debug, Clone)]
pub enum StateEvent {
    /// Forward to WebSocket clients as-is.
    Wire(PitWallEvent),
    SessionStarted(SessionRow),
    LapCompleted(LapRow),
    /// Laps `>= to_lap` were rolled back.
    Flashback { to_lap: u8 },
    Buttons(u32),
    LightsOut,
    TrackOutline { track_id: i8, length_m: f32, points: Vec<[f32; 2]> },
    RaceFinished,
}

#[derive(Debug, Clone, Default)]
struct Stint {
    compound: String,
    start_lap: u8,
    end_lap: Option<u8>,
}

#[derive(Debug, Clone, Default)]
struct CarHistory {
    laps: Vec<LapRow>,
    stints: Vec<Stint>,
    last_tyre_age: Option<u8>,
    last_visual: Option<u8>,
    /// (lap progress, wear) samples in the current stint.
    wear_points: Vec<(f32, [f32; 4])>,
}

/// Builds the track outline from every running car's position, binned by lap
/// distance. With a full grid it completes within about a lap of green running,
/// and it does not need the player to drive a clean lap.
#[derive(Debug, Clone, Default)]
struct OutlineBuilder {
    len: f32,
    bins: Vec<([f64; 2], u32)>,
    filled: usize,
}

const OUTLINE_BIN_M: f32 = 8.0;
const OUTLINE_MAX_SAMPLES: u32 = 16;

impl OutlineBuilder {
    fn add(&mut self, len: f32, dist: f32, x: f32, z: f32) {
        if len < 500.0 || !(0.0..len).contains(&dist) || (x == 0.0 && z == 0.0) {
            return;
        }
        if self.len != len || self.bins.is_empty() {
            let n = (len / OUTLINE_BIN_M).ceil() as usize;
            *self = Self { len, bins: vec![([0.0; 2], 0); n], filled: 0 };
        }
        let n = self.bins.len();
        let i = (((dist / len) * n as f32) as usize).min(n - 1);
        let b = &mut self.bins[i];
        if b.1 == 0 {
            self.filled += 1;
        }
        if b.1 < OUTLINE_MAX_SAMPLES {
            b.0[0] += x as f64;
            b.0[1] += z as f64;
            b.1 += 1;
        }
    }

    fn complete(&self) -> bool {
        !self.bins.is_empty() && self.filled * 100 >= self.bins.len() * 97
    }

    /// Mean position per bin; empty bins are interpolated from their neighbours.
    fn points(&self) -> Vec<[f32; 2]> {
        let n = self.bins.len();
        let mean: Vec<Option<[f32; 2]>> = self
            .bins
            .iter()
            .map(|(s, c)| (*c > 0).then(|| [(s[0] / *c as f64) as f32, (s[1] / *c as f64) as f32]))
            .collect();
        if self.filled == 0 {
            return Vec::new();
        }
        (0..n)
            .map(|i| {
                if let Some(p) = mean[i] {
                    return p;
                }
                let prev = (1..n).map(|k| (i + n - k) % n).find(|&j| mean[j].is_some()).unwrap_or(i);
                let next = (1..n).map(|k| (i + k) % n).find(|&j| mean[j].is_some()).unwrap_or(i);
                let (a, b) = (mean[prev].unwrap_or_default(), mean[next].unwrap_or_default());
                let dp = (i + n - prev) % n;
                let dn = (next + n - i) % n;
                let t = dp as f32 / (dp + dn).max(1) as f32;
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
            })
            .collect()
    }
}

pub struct RaceState {
    pub thresholds: Thresholds,
    source: DataSource,
    format: Option<Format>,
    session_uid: u64,
    session_announced: bool,
    player: usize,
    session_time: f64,
    session: Option<SessionPacket>,
    lap: Option<LapDataPacket>,
    prev_lap: Option<LapDataPacket>,
    participants: Option<ParticipantsPacket>,
    telemetry: Option<CarTelemetryPacket>,
    status: Option<CarStatusPacket>,
    damage: Option<CarDamagePacket>,
    motion: Option<MotionPacket>,
    setups: Option<CarSetupsPacket>,
    history: Vec<Option<SessionHistoryPacket>>,
    cars: Vec<CarHistory>,
    timing: TimingLines,
    outline: OutlineBuilder,
    outline_available: bool,
    pending_flashback: bool,
    lights_out_at: Option<f64>,
    finished: bool,
    last_packet_wall_ms: u64,
    pub packets_total: u64,
    pub packets_ignored: u64,
}

impl RaceState {
    pub fn new(thresholds: Thresholds) -> Self {
        let spacing = thresholds.timing_line_spacing_m;
        Self {
            thresholds,
            source: DataSource::None,
            format: None,
            session_uid: 0,
            session_announced: false,
            player: 0,
            session_time: 0.0,
            session: None,
            lap: None,
            prev_lap: None,
            participants: None,
            telemetry: None,
            status: None,
            damage: None,
            motion: None,
            setups: None,
            history: vec![None; MAX_CARS],
            cars: vec![CarHistory::default(); MAX_CARS],
            timing: TimingLines::new(MAX_CARS, spacing),
            outline: OutlineBuilder::default(),
            outline_available: false,
            pending_flashback: false,
            lights_out_at: None,
            finished: false,
            last_packet_wall_ms: 0,
            packets_total: 0,
            packets_ignored: 0,
        }
    }

    pub fn set_source(&mut self, source: DataSource) {
        self.source = source;
    }
    pub fn source(&self) -> DataSource {
        self.source
    }
    pub fn set_outline_available(&mut self, v: bool) {
        self.outline_available = v;
    }
    pub fn track_id(&self) -> Option<i8> {
        self.session.as_ref().map(|s| s.track_id)
    }
    pub fn session_uid(&self) -> u64 {
        self.session_uid
    }
    pub fn last_packet_wall_ms(&self) -> u64 {
        self.last_packet_wall_ms
    }
    pub fn player_index(&self) -> usize {
        self.player
    }

    fn reset_session(&mut self, uid: u64) {
        let thresholds = std::mem::take(&mut self.thresholds);
        let source = self.source;
        let total = self.packets_total;
        let ignored = self.packets_ignored;
        *self = RaceState::new(thresholds);
        self.source = source;
        self.session_uid = uid;
        self.packets_total = total;
        self.packets_ignored = ignored;
    }

    fn driver_name(&self, car: usize) -> String {
        self.participants.as_ref().and_then(|p| p.participants.get(car)).map(|p| p.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("Car {}", car + 1))
    }

    fn driver_label(&self, car: usize) -> String {
        let name = self.driver_name(car);
        let last = name.split_whitespace().last().unwrap_or("").chars().filter(|c| c.is_alphabetic()).collect::<String>();
        if last.chars().count() >= 3 {
            last.chars().take(3).collect::<String>().to_uppercase()
        } else {
            format!("C{:02}", car + 1)
        }
    }

    fn restricted(&self, car: usize) -> bool {
        if car == self.player {
            return false;
        }
        self.participants.as_ref().and_then(|p| p.participants.get(car)).map(|p| p.telemetry_restricted()).unwrap_or(false)
    }

    fn track_length(&self) -> f32 {
        self.session.as_ref().map(|s| s.track_length as f32).unwrap_or(0.0)
    }

    /// Ingest one decoded packet. `wall_ms` is the wall-clock receive time.
    pub fn ingest(&mut self, p: &ParsedPacket, wall_ms: u64) -> Vec<StateEvent> {
        let mut ev = Vec::new();
        self.packets_total += 1;
        self.last_packet_wall_ms = wall_ms;
        let h = &p.header;
        if h.session_uid != 0 && h.session_uid != self.session_uid {
            let had_session = self.session_uid != 0;
            self.reset_session(h.session_uid);
            if had_session {
                ev.push(StateEvent::Wire(PitWallEvent::SessionReset { session_uid: format!("{:016x}", h.session_uid) }));
            }
        }
        self.format = Some(p.format);
        self.player = (h.player_car_index as usize).min(MAX_CARS - 1);
        let t = h.session_time as f64;
        if t + 0.5 < self.session_time && matches!(p.packet, Packet::LapData(_)) {
            self.pending_flashback = true;
        }
        self.session_time = t;

        match &p.packet {
            Packet::Session(s) => self.on_session(s, &mut ev),
            Packet::LapData(l) => self.on_lap_data(l, &mut ev),
            Packet::Event(e) => self.on_event(&e.details, &mut ev),
            Packet::Participants(x) => self.participants = Some((**x).clone()),
            Packet::CarTelemetry(x) => self.telemetry = Some((**x).clone()),
            Packet::CarStatus(x) => self.on_status(x),
            Packet::CarDamage(x) => self.damage = Some((**x).clone()),
            Packet::Motion(m) => self.on_motion(m, &mut ev),
            Packet::CarSetups(x) => self.setups = Some((**x).clone()),
            Packet::SessionHistory(hh) => {
                if let Some(slot) = self.history.get_mut(hh.car_idx as usize) {
                    *slot = Some((**hh).clone());
                }
            }
            Packet::FinalClassification(_) => {
                if !self.finished {
                    self.finished = true;
                    ev.push(StateEvent::RaceFinished);
                }
            }
            Packet::Ignored(_) => self.packets_ignored += 1,
        }
        ev
    }

    fn on_session(&mut self, s: &SessionPacket, ev: &mut Vec<StateEvent>) {
        let prev_sc = self.session.as_ref().map(|p| p.safety_car_status).unwrap_or(0);
        let at_ms = (self.session_time * 1000.0) as u64;
        if self.session.is_some() && prev_sc != s.safety_car_status {
            let status = match (prev_sc, s.safety_car_status) {
                (_, 1) => Some(FlagStatus::Sc),
                (_, 2) => Some(FlagStatus::Vsc),
                (1 | 2, 0) => Some(FlagStatus::Green),
                _ => None,
            };
            if let Some(status) = status {
                ev.push(StateEvent::Wire(PitWallEvent::Flag { at_ms, status }));
            }
        }
        self.session = Some(s.clone());
        if !self.session_announced {
            self.session_announced = true;
            let fmt = self.format.unwrap_or(Format::F2024);
            ev.push(StateEvent::SessionStarted(SessionRow {
                session_uid: format!("{:016x}", self.session_uid),
                game_year: fmt.game_year(),
                track_id: s.track_id,
                track_name: track_name(s.track_id).to_string(),
                session_type: session_type_name(fmt, s.session_type).to_string(),
                total_laps: s.total_laps,
            }));
        }
    }

    fn on_event(&mut self, e: &EventDetails, ev: &mut Vec<StateEvent>) {
        let at_ms = (self.session_time * 1000.0) as u64;
        match *e {
            EventDetails::Penalty { penalty_type, vehicle_idx, time, lap_num, .. } => {
                if penalty_is_significant(penalty_type) {
                    let label = self.driver_label(vehicle_idx as usize);
                    ev.push(StateEvent::Wire(PitWallEvent::Penalty {
                        at_ms,
                        car_index: vehicle_idx,
                        driver_label: label,
                        description: penalty_type_name(penalty_type).to_string(),
                        time_sec: (time != 255 && time > 0).then_some(time),
                        lap: lap_num,
                    }));
                }
            }
            EventDetails::Flashback { .. } => self.pending_flashback = true,
            EventDetails::Buttons { status } => ev.push(StateEvent::Buttons(status)),
            EventDetails::LightsOut => {
                self.lights_out_at = Some(self.session_time);
                ev.push(StateEvent::LightsOut);
            }
            EventDetails::ChequeredFlag => ev.push(StateEvent::Wire(PitWallEvent::Flag { at_ms, status: FlagStatus::Chequered })),
            EventDetails::RedFlag => ev.push(StateEvent::Wire(PitWallEvent::Flag { at_ms, status: FlagStatus::Red })),
            EventDetails::SafetyCar { sc_type, event_type: 1 } => {
                let status = if sc_type == 2 { FlagStatus::VscEnding } else { FlagStatus::ScEnding };
                ev.push(StateEvent::Wire(PitWallEvent::Flag { at_ms, status }));
            }
            _ => {}
        }
    }

    fn on_status(&mut self, s: &CarStatusPacket) {
        let lap_now: Vec<u8> = (0..MAX_CARS).map(|i| self.lap.as_ref().map(|l| l.cars[i].current_lap_num).unwrap_or(1)).collect();
        for (i, &lap_i) in lap_now.iter().enumerate() {
            if self.restricted(i) {
                continue;
            }
            let st = &s.cars[i];
            if st.visual_tyre_compound == 0 {
                continue;
            }
            let car = &mut self.cars[i];
            let new_stint = match (car.last_visual, car.last_tyre_age) {
                (None, _) => true,
                (Some(v), Some(age)) => v != st.visual_tyre_compound || st.tyres_age_laps < age,
                _ => false,
            };
            if new_stint {
                let lap = lap_i.max(1);
                if let Some(last) = car.stints.last_mut() {
                    if last.end_lap.is_none() {
                        last.end_lap = Some(lap.saturating_sub(1).max(last.start_lap));
                    }
                }
                car.stints.push(Stint { compound: visual_compound(st.visual_tyre_compound).to_string(), start_lap: lap, end_lap: None });
                // Wear points restart with the first lap completed on the new set
                // (the damage packet may still carry the old tyres' wear here).
                car.wear_points.clear();
            }
            car.last_visual = Some(st.visual_tyre_compound);
            car.last_tyre_age = Some(st.tyres_age_laps);
        }
        self.status = Some(s.clone());
    }

    fn on_motion(&mut self, m: &MotionPacket, ev: &mut Vec<StateEvent>) {
        self.motion = Some(m.clone());
        if self.outline_available {
            return;
        }
        let len = self.track_length();
        let Some(lap) = &self.lap else { return };
        for (l, c) in lap.cars.iter().zip(m.cars.iter()) {
            // Running cars only: active, out of the garage and not in the pit lane.
            if l.result_status != 2 || l.driver_status == 0 || l.pit_status != 0 {
                continue;
            }
            self.outline.add(len, l.lap_distance, c.world_position[0], c.world_position[2]);
        }
        if self.outline.complete() {
            let points = self.outline.points();
            self.outline = OutlineBuilder::default();
            self.outline_available = true;
            ev.push(StateEvent::TrackOutline { track_id: self.track_id().unwrap_or(-1), length_m: len, points });
        }
    }

    fn on_lap_data(&mut self, l: &LapDataPacket, ev: &mut Vec<StateEvent>) {
        let t = self.session_time;
        if self.pending_flashback {
            self.pending_flashback = false;
            self.timing.reset();
            self.outline = OutlineBuilder::default();
            for (i, car) in self.cars.iter_mut().enumerate() {
                let cur = l.cars[i].current_lap_num;
                car.laps.retain(|r| r.lap < cur);
                car.wear_points.retain(|(p, _)| *p < cur as f32);
            }
            let to_lap = l.cars[self.player].current_lap_num;
            ev.push(StateEvent::Flashback { to_lap });
            ev.push(StateEvent::Wire(PitWallEvent::Flashback { at_ms: (t * 1000.0) as u64, to_lap }));
            self.prev_lap = None;
        }

        let prev = self.prev_lap.take().or_else(|| self.lap.clone());
        for i in 0..MAX_CARS {
            let cur = &l.cars[i];
            if cur.result_status < 2 && cur.car_position == 0 {
                continue;
            }
            self.timing.update(i, cur.total_distance, t);
            let Some(prev) = &prev else { continue };
            let pv = &prev.cars[i];
            if pv.current_lap_num == 0 || cur.current_lap_num <= pv.current_lap_num {
                continue;
            }
            // Lap `pv.current_lap_num` completed.
            let completed = pv.current_lap_num;
            let lap_ms = cur.last_lap_time_ms;
            let (s1, s2) = (pv.sector1_time_ms, pv.sector2_time_ms);
            let s3 = (s1 > 0 && s2 > 0 && lap_ms > s1 + s2).then(|| lap_ms - s1 - s2);
            let restricted = self.restricted(i);
            let st = self.status.as_ref().map(|s| s.cars[i]).filter(|_| !restricted);
            let dm = self.damage.as_ref().map(|d| d.cars[i]).filter(|_| !restricted);
            let row = LapRow {
                car_index: i as u8,
                driver: self.driver_label(i),
                lap: completed,
                lap_ms,
                s1_ms: (s1 > 0).then_some(s1),
                s2_ms: (s2 > 0).then_some(s2),
                s3_ms: s3,
                compound: st.map(|s| visual_compound(s.visual_tyre_compound).to_string()),
                tyre_age: st.map(|s| s.tyres_age_laps),
                wear: dm.map(|d| d.tyres_wear),
                fuel_kg: st.map(|s| s.fuel_in_tank),
                ers_harvest_j: prev_status_harvest(st),
                ers_deploy_j: st.map(|s| s.ers_deployed_this_lap),
                valid: !pv.current_lap_invalid,
                position: cur.car_position,
                session_time: t as f32,
            };
            let car = &mut self.cars[i];
            car.laps.retain(|r| r.lap != completed);
            car.laps.push(row.clone());
            if let Some(w) = row.wear {
                // A drop in wear means fresh tyres: start a new series.
                let dropped = car.wear_points.last().map(|(_, prev)| (0..4).any(|k| w[k] + 1.0 < prev[k])).unwrap_or(false);
                if dropped {
                    car.wear_points.clear();
                }
                car.wear_points.push((completed as f32, w));
            }
            ev.push(StateEvent::Wire(PitWallEvent::LapCompleted { car_index: i as u8, lap: completed, lap_ms }));
            ev.push(StateEvent::LapCompleted(row));
        }
        self.prev_lap = Some(l.clone());
        self.lap = Some(l.clone());
    }

    fn gap_ms(&self, behind: usize, ahead: usize) -> Option<u32> {
        self.timing.gap(behind, ahead).map(|g| (g * 1000.0).round() as u32)
    }

    fn race_phase(&self) -> &'static str {
        if self.finished {
            return "finished";
        }
        let fmt = self.format.unwrap_or(Format::F2024);
        let is_race = self.session.as_ref().map(|s| is_race_session(fmt, s.session_type)).unwrap_or(false);
        match (is_race, self.lights_out_at) {
            (true, None) => {
                let moving = self.lap.as_ref().map(|l| l.cars[self.player].current_lap_time_ms > 0).unwrap_or(false);
                if moving {
                    "racing"
                } else {
                    "grid"
                }
            }
            (true, Some(lo)) if self.session_time - lo < 20.0 => "start",
            _ => "racing",
        }
    }

    /// Build the wire snapshot. `comms` is filled by the caller.
    pub fn snapshot(&self, timestamp_ms: u64) -> TelemetrySnapshot {
        let fmt = self.format;
        let player = self.player;
        let mut snap = TelemetrySnapshot {
            schema_version: SCHEMA_VERSION,
            timestamp_ms,
            source: self.source,
            game_year: fmt.map(|f| f.game_year()),
            session_uid: (self.session_uid != 0).then(|| format!("{:016x}", self.session_uid)),
            ..Default::default()
        };
        let lapd = self.lap.as_ref();
        let pl = lapd.map(|l| l.cars[player]);

        // ---- Session.
        if let Some(s) = &self.session {
            let f = fmt.unwrap_or(Format::F2024);
            snap.session = SessionInfo {
                track_id: s.track_id,
                track_name: track_name(s.track_id).to_string(),
                track_length_m: s.track_length,
                session_type: session_type_name(f, s.session_type).to_string(),
                is_race: is_race_session(f, s.session_type),
                lap: pl.map(|l| l.current_lap_num).unwrap_or(0),
                total_laps: s.total_laps,
                time_left_ms: s.session_time_left as u64 * 1000,
                weather: weather_name(s.weather).to_string(),
                air_temp_c: s.air_temperature,
                track_temp_c: s.track_temperature,
                forecast_accuracy: if s.forecast_accuracy == 0 { "perfect" } else { "approximate" }.to_string(),
                weather_forecast: s
                    .weather_forecast
                    .iter()
                    .filter(|w| w.session_type == s.session_type)
                    .map(|w| ForecastSampleView {
                        time_offset_min: w.time_offset_min,
                        weather: weather_name(w.weather).to_string(),
                        rain_pct: w.rain_percentage,
                        air_temp_c: w.air_temperature,
                        track_temp_c: w.track_temperature,
                    })
                    .collect(),
                marshal_zones: s.marshal_zones.iter().map(|z| MarshalZoneView { start_fraction: z.zone_start, flag: marshal_flag_name(z.zone_flag).to_string() }).collect(),
                safety_car_status: safety_car_name(s.safety_car_status).to_string(),
                pit_window: (s.pit_stop_window_ideal_lap > 0).then_some(PitWindow {
                    ideal_lap: s.pit_stop_window_ideal_lap,
                    latest_lap: s.pit_stop_window_latest_lap,
                    rejoin_position: s.pit_stop_rejoin_position,
                }),
                paused: s.game_paused,
                phase: self.race_phase().to_string(),
            };
        }
        let track_len = self.track_length();

        // ---- Timing.
        if let Some(l) = lapd {
            let mut by_pos: Vec<usize> = (0..MAX_CARS).filter(|&i| l.cars[i].car_position > 0 && l.cars[i].result_status >= 2).collect();
            by_pos.sort_by_key(|&i| l.cars[i].car_position);
            let leader = by_pos.first().copied();
            let mut entries = Vec::with_capacity(by_pos.len());
            for (k, &i) in by_pos.iter().enumerate() {
                let c = &l.cars[i];
                let ahead = if k > 0 { Some(by_pos[k - 1]) } else { None };
                let restricted = self.restricted(i);
                let st = self.status.as_ref().map(|s| s.cars[i]).filter(|s| !restricted && s.visual_tyre_compound != 0);
                let native_ahead = c.delta_to_car_in_front_ms.filter(|&d| d > 0);
                let native_leader = c.delta_to_race_leader_ms.filter(|&d| d > 0);
                let gap_ahead = ahead.and_then(|a| native_ahead.or_else(|| self.gap_ms(i, a)));
                let gap_leader = match leader {
                    Some(ld) if ld != i => native_leader.or_else(|| self.gap_ms(i, ld)),
                    _ => None,
                };
                let hist = &self.cars[i];
                let last_row = hist.laps.last();
                let best = hist.laps.iter().filter(|r| r.valid && r.lap_ms > 0).map(|r| r.lap_ms).min();
                let stints = if let Some(h) = self.history[i].as_ref().filter(|h| !h.tyre_stints.is_empty()) {
                    let mut start = 1u8;
                    h.tyre_stints
                        .iter()
                        .map(|s| {
                            let v = StintView {
                                compound: visual_compound(s.tyre_visual_compound).to_string(),
                                start_lap: start,
                                end_lap: (s.end_lap != 255).then_some(s.end_lap),
                            };
                            start = s.end_lap.saturating_add(1);
                            v
                        })
                        .collect()
                } else {
                    hist.stints.iter().map(|s| StintView { compound: s.compound.clone(), start_lap: s.start_lap, end_lap: s.end_lap }).collect()
                };
                entries.push(TimingEntry {
                    car_index: i as u8,
                    position: c.car_position,
                    driver_label: self.driver_label(i),
                    driver_name: self.driver_name(i),
                    race_number: self.participants.as_ref().map(|p| p.participants[i].race_number).unwrap_or(0),
                    team_id: self.participants.as_ref().map(|p| p.participants[i].team_id).unwrap_or(255),
                    is_player: i == player,
                    lap: c.current_lap_num,
                    gap_to_leader_ms: gap_leader,
                    gap_to_ahead_ms: gap_ahead,
                    last_lap_ms: (c.last_lap_time_ms > 0).then_some(c.last_lap_time_ms),
                    best_lap_ms: best,
                    sectors_ms: last_row.map(|r| [r.s1_ms, r.s2_ms, r.s3_ms]).unwrap_or([None; 3]),
                    compound: st.map(|s| visual_compound(s.visual_tyre_compound).to_string()),
                    tyre_age_laps: st.map(|s| s.tyres_age_laps),
                    pit_stops: c.num_pit_stops,
                    pit_status: pit_status_name(c.pit_status).to_string(),
                    penalties_ms: c.penalties as u32 * 1000,
                    warnings: c.total_warnings,
                    driver_status: driver_status_name(c.driver_status).to_string(),
                    result_status: result_status_name(c.result_status).to_string(),
                    telemetry_restricted: restricted,
                    stints,
                });
            }
            let pos = l.cars[player].car_position;
            let behind = by_pos.iter().position(|&i| i == player).and_then(|k| by_pos.get(k + 1)).copied();
            snap.timing = Timing {
                player_index: player as u8,
                position: pos,
                gap_ahead_ms: entries.iter().find(|e| e.is_player).and_then(|e| e.gap_to_ahead_ms),
                gap_behind_ms: behind.and_then(|b| entries.iter().find(|e| e.car_index as usize == b)).and_then(|e| e.gap_to_ahead_ms),
                last_lap_ms: (l.cars[player].last_lap_time_ms > 0).then_some(l.cars[player].last_lap_time_ms),
                best_lap_ms: entries.iter().find(|e| e.is_player).and_then(|e| e.best_lap_ms),
                entries,
            };
        }

        // ---- Track.
        snap.track.outline_available = self.outline_available;
        if let (Some(m), Some(l)) = (&self.motion, lapd) {
            for i in 0..MAX_CARS {
                let c = &l.cars[i];
                if c.car_position == 0 || c.result_status < 2 {
                    continue;
                }
                let tel = self.telemetry.as_ref().map(|t| t.cars[i]);
                let st = self.status.as_ref().map(|s| s.cars[i]).filter(|_| !self.restricted(i));
                let pos = m.cars[i].world_position;
                snap.track.cars.push(TrackCar {
                    car_index: i as u8,
                    x: pos[0],
                    z: pos[2],
                    lap_distance_pct: if track_len > 0.0 { (c.lap_distance / track_len * 100.0).clamp(0.0, 100.0) } else { 0.0 },
                    speed_kph: tel.map(|t| t.speed_kph).unwrap_or(0),
                    gear: tel.map(|t| t.gear).unwrap_or(0),
                    throttle_pct: tel.map(|t| (t.throttle * 100.0).clamp(0.0, 100.0) as u8).unwrap_or(0),
                    brake_pct: tel.map(|t| (t.brake * 100.0).clamp(0.0, 100.0) as u8).unwrap_or(0),
                    steer: tel.map(|t| t.steer).unwrap_or(0.0),
                    ers_deploy_mode: st.map(|s| ers_mode_name(s.ers_deploy_mode).to_string()),
                    is_player: i == player,
                    drs_active: tel.map(|t| t.drs).unwrap_or(false),
                    in_pit: c.pit_status != 0,
                });
            }
        }

        // ---- Player tyres / powertrain / damage.
        let th = &self.thresholds;
        let st = self.status.as_ref().map(|s| s.cars[player]);
        let dm = self.damage.as_ref().map(|d| d.cars[player]);
        let tel = self.telemetry.as_ref().map(|t| t.cars[player]);
        if let Some(st) = st {
            let wear = dm.map(|d| d.tyres_wear).unwrap_or([0.0; 4]);
            let rates = self.wear_rates(player);
            let lap_now = pl.map(|l| l.current_lap_num).unwrap_or(1);
            let mut tyres = Tyres {
                compound: visual_compound(st.visual_tyre_compound).to_string(),
                actual_compound: actual_compound(st.actual_tyre_compound).to_string(),
                age_laps: st.tyres_age_laps,
                wear_pct: wear.map(|w| (w * 10.0).round() / 10.0),
                wear_rate_per_lap: rates.map(|r| r.map(|v| (v * 100.0).round() / 100.0)),
                damage_pct: dm.map(|d| d.tyres_damage).unwrap_or_default(),
                blisters_pct: (fmt >= Some(Format::F2025)).then(|| dm.map(|d| d.tyre_blisters).unwrap_or_default()),
                surface_temp_c: tel.map(|t| t.tyres_surface_temperature).unwrap_or_default(),
                inner_temp_c: tel.map(|t| t.tyres_inner_temperature).unwrap_or_default(),
                brake_temp_c: tel.map(|t| t.brakes_temperature).unwrap_or_default(),
                pressure_psi: tel.map(|t| t.tyres_pressure).unwrap_or_default(),
                projected_pit_lap: metrics::projected_pit_lap(lap_now, wear, rates, th.pit_wear_target),
                ..Default::default()
            };
            let (lvl, reason) = tyre_alert(&tyres, th);
            tyres.alert_level = lvl;
            tyres.alert_reason = reason;
            snap.tyres = tyres;

            let fuel_ends: Vec<f32> = self.cars[player].laps.iter().filter_map(|r| r.fuel_kg).collect();
            let burn = metrics::fuel_burn_per_lap(&fuel_ends, 3);
            let remaining = match (pl, &self.session) {
                (Some(l), Some(s)) => Some(metrics::laps_remaining(s.total_laps, l.current_lap_num, l.lap_distance, s.track_length as f32)),
                _ => None,
            };
            let mut pt = Powertrain {
                fuel_remaining_laps_delta: (st.fuel_remaining_laps * 100.0).round() / 100.0,
                fuel_remaining_kg: (st.fuel_in_tank * 100.0).round() / 100.0,
                fuel_burn_per_lap_kg: burn.map(|b| (b * 1000.0).round() / 1000.0),
                fuel_required_per_lap_kg: remaining.filter(|r| *r > 0.1).map(|r| ((st.fuel_in_tank / r) * 1000.0).round() / 1000.0),
                ers_store_pct: (st.ers_store_energy / 4.0e6 * 1000.0).round() / 10.0,
                ers_deploy_mode: ers_mode_name(st.ers_deploy_mode).to_string(),
                ers_harvested_this_lap_kj: ((st.ers_harvested_this_lap_mguk + st.ers_harvested_this_lap_mguh) / 1000.0).round(),
                ers_deployed_this_lap_kj: (st.ers_deployed_this_lap / 1000.0).round(),
                wear_pct: dm
                    .map(|d| ComponentWear {
                        ice: d.engine_ice_wear,
                        mgu_h: d.engine_mguh_wear,
                        mgu_k: d.engine_mguk_wear,
                        es: d.engine_es_wear,
                        ce: d.engine_ce_wear,
                        tc: d.engine_tc_wear,
                        gearbox: d.gearbox_damage,
                    })
                    .unwrap_or_default(),
                engine_blown: dm.map(|d| d.engine_blown).unwrap_or(false),
                engine_seized: dm.map(|d| d.engine_seized).unwrap_or(false),
                ers_fault: dm.map(|d| d.ers_fault).unwrap_or(false),
                ..Default::default()
            };
            let (lvl, reason) = powertrain_alert(&pt, th);
            pt.alert_level = lvl;
            pt.alert_reason = reason;
            snap.powertrain = pt;
        }
        if let Some(d) = dm {
            let setup = self.setups.as_ref().map(|s| s.cars[player]).filter(|s| s.front_wing > 0).map(|s| SetupView {
                front_wing: s.front_wing,
                rear_wing: s.rear_wing,
                on_throttle: s.on_throttle,
                off_throttle: s.off_throttle,
                brake_bias: s.brake_bias,
                brake_pressure: s.brake_pressure,
                front_suspension: s.front_suspension,
                rear_suspension: s.rear_suspension,
                front_anti_roll_bar: s.front_anti_roll_bar,
                rear_anti_roll_bar: s.rear_anti_roll_bar,
                front_ride_height: s.front_suspension_height,
                rear_ride_height: s.rear_suspension_height,
            });
            let mut dmg = Damage {
                front_left_wing_pct: d.front_left_wing_damage,
                front_right_wing_pct: d.front_right_wing_damage,
                rear_wing_pct: d.rear_wing_damage,
                floor_pct: d.floor_damage,
                diffuser_pct: d.diffuser_damage,
                sidepod_pct: d.sidepod_damage,
                brake_damage_pct: d.brakes_damage,
                drs_fault: d.drs_fault,
                setup,
                ..Default::default()
            };
            let brake_max = tel.map(|t| t.brakes_temperature.iter().copied().max().unwrap_or(0)).unwrap_or(0);
            let (lvl, reason) = damage_alert(&dmg, brake_max, th);
            dmg.alert_level = lvl;
            dmg.alert_reason = reason;
            snap.damage = dmg;
        }
        snap
    }

    /// Per-corner wear rate (%/lap) for the current stint of `car`.
    pub fn wear_rates(&self, car: usize) -> [Option<f32>; 4] {
        let pts = &self.cars[car].wear_points;
        let mut out = [None; 4];
        for (w, slot) in out.iter_mut().enumerate() {
            let series: Vec<(f32, f32)> = pts.iter().map(|(x, ws)| (*x, ws[w])).collect();
            *slot = metrics::slope(&series).filter(|r| *r >= 0.0);
        }
        out
    }

    /// Completed laps for a car (most recent last).
    pub fn laps(&self, car: usize) -> &[LapRow] {
        self.cars.get(car).map(|c| c.laps.as_slice()).unwrap_or(&[])
    }
}

fn prev_status_harvest(st: Option<CarStatus>) -> Option<f32> {
    st.map(|s| s.ers_harvested_this_lap_mguk + s.ers_harvested_this_lap_mguh)
}

const CORNERS: [&str; 4] = ["FL", "FR", "RL", "RR"];

fn worst(levels: &mut [(AlertLevel, String)]) -> (AlertLevel, Option<String>) {
    levels.sort_by_key(|(l, _)| std::cmp::Reverse(*l as u8));
    match levels.first() {
        Some((l, r)) => (*l, Some(r.clone())),
        None => (AlertLevel::Nominal, None),
    }
}

pub fn tyre_alert(t: &Tyres, th: &Thresholds) -> (AlertLevel, Option<String>) {
    let mut v = Vec::new();
    for (i, w) in t.wear_pct.iter().enumerate() {
        if *w >= th.tyre_wear_critical {
            v.push((AlertLevel::Critical, format!("{} wear {:.0}%", CORNERS[i], w)));
        } else if *w >= th.tyre_wear_watch {
            v.push((AlertLevel::Watch, format!("{} wear {:.0}%", CORNERS[i], w)));
        }
        if t.surface_temp_c[i] >= th.tyre_surface_temp_watch {
            v.push((AlertLevel::Watch, format!("{} surface {}°C", CORNERS[i], t.surface_temp_c[i])));
        }
        if t.damage_pct[i] >= 50 {
            v.push((AlertLevel::Critical, format!("{} tyre damage {}%", CORNERS[i], t.damage_pct[i])));
        }
    }
    worst(&mut v)
}

pub fn powertrain_alert(p: &Powertrain, th: &Thresholds) -> (AlertLevel, Option<String>) {
    let mut v = Vec::new();
    if p.engine_blown || p.engine_seized {
        v.push((AlertLevel::Critical, "Engine failure".to_string()));
    }
    if p.ers_fault {
        v.push((AlertLevel::Critical, "ERS fault".to_string()));
    }
    if p.fuel_remaining_laps_delta <= th.fuel_delta_critical {
        v.push((AlertLevel::Critical, format!("Fuel {:+.1} laps", p.fuel_remaining_laps_delta)));
    } else if p.fuel_remaining_laps_delta <= th.fuel_delta_watch {
        v.push((AlertLevel::Watch, format!("Fuel {:+.1} laps", p.fuel_remaining_laps_delta)));
    }
    if p.ers_store_pct < th.ers_low_pct {
        v.push((AlertLevel::Watch, format!("ERS {:.0}%", p.ers_store_pct)));
    }
    let w = &p.wear_pct;
    for (name, val) in [("ICE", w.ice), ("MGU-H", w.mgu_h), ("MGU-K", w.mgu_k), ("ES", w.es), ("CE", w.ce), ("TC", w.tc), ("Gearbox", w.gearbox)] {
        if val >= th.component_wear_watch {
            v.push((AlertLevel::Watch, format!("{name} wear {val}%")));
        }
    }
    worst(&mut v)
}

pub fn damage_alert(d: &Damage, brake_max: u16, th: &Thresholds) -> (AlertLevel, Option<String>) {
    let mut v = Vec::new();
    for (name, val) in [
        ("Front-left wing", d.front_left_wing_pct),
        ("Front-right wing", d.front_right_wing_pct),
        ("Rear wing", d.rear_wing_pct),
        ("Floor", d.floor_pct),
        ("Diffuser", d.diffuser_pct),
        ("Sidepod", d.sidepod_pct),
    ] {
        if val >= th.aero_damage_critical {
            v.push((AlertLevel::Critical, format!("{name} damage {val}%")));
        } else if val >= th.aero_damage_watch {
            v.push((AlertLevel::Watch, format!("{name} damage {val}%")));
        }
    }
    if d.drs_fault {
        v.push((AlertLevel::Watch, "DRS fault".to_string()));
    }
    if brake_max >= th.brake_temp_watch {
        v.push((AlertLevel::Watch, format!("Brakes {brake_max}°C")));
    }
    worst(&mut v)
}
