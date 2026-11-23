//! Synthetic race simulation: 22 cars on a generated track with a scripted
//! scenario (tyre degradation, fuel deficit, front wing damage, a time
//! penalty, a safety car period, rain onset and pit stops).

use crate::track::Track;
use f1_udp::*;

pub struct SimConfig {
    pub format: Format,
    pub laps: u8,
    pub player_grid: usize,
    pub seed: u64,
    pub rate_hz: f32,
    pub track_length_m: f32,
}

const NAMES: [&str; 22] = [
    "Alvarez", "Brandt", "Castell", "Dubois", "Eriksen", "Ferrante", "Grayson", "Hoffman", "Ivanova", "Jansen",
    "Kowalski", "Laurent", "Moretti", "Novak", "Okafor", "Petrov", "Quinn", "Rossi", "Sato", "Tanaka", "Ueda", "Varga",
];

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// Uniform in [0, 1).
    fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Compound {
    Soft,
    Medium,
    Hard,
    Inter,
}

impl Compound {
    fn visual(self) -> u8 {
        match self {
            Compound::Soft => 16,
            Compound::Medium => 17,
            Compound::Hard => 18,
            Compound::Inter => 7,
        }
    }
    fn actual(self) -> u8 {
        match self {
            Compound::Soft => 16,   // C5
            Compound::Medium => 17, // C4
            Compound::Hard => 18,   // C3
            Compound::Inter => 7,
        }
    }
    fn wear_per_lap(self) -> f32 {
        match self {
            Compound::Soft => 4.2,
            Compound::Medium => 3.0,
            Compound::Hard => 2.2,
            Compound::Inter => 3.5,
        }
    }
    fn pace(self) -> f32 {
        match self {
            Compound::Soft => 1.006,
            Compound::Medium => 1.0,
            Compound::Hard => 0.995,
            Compound::Inter => 0.93,
        }
    }
}

struct Car {
    grid: usize,
    team: u8,
    race_number: u8,
    name: String,
    pace: f32,
    total: f32,
    speed: f32,
    lap: u8,
    lap_start_t: f32,
    s1_ms: u32,
    s2_ms: u32,
    last_lap_ms: u32,
    best_lap_ms: u32,
    lap_history: Vec<LapHistory>,
    stints: Vec<TyreStintHistory>,
    compound: Compound,
    tyre_age: u8,
    wear: [f32; 4],
    wear_mult: [f32; 4],
    fuel_kg: f32,
    burn_per_lap: f32,
    ers_j: f32,
    ers_deployed: f32,
    ers_harv_k: f32,
    ers_harv_h: f32,
    pit_stops: u8,
    pit_lap: u8,
    pit_timer: f32,
    finished: bool,
    finish_order: Option<usize>,
    penalties_s: u8,
    warnings: u8,
    damage: CarDamage,
    surface_t: [f32; 4],
    inner_t: [f32; 4],
    brake_t: [f32; 4],
    throttle: f32,
    brake: f32,
    steer: f32,
    gear: i8,
    lap_invalid: bool,
}

#[derive(PartialEq)]
enum Phase {
    Grid,
    Racing,
    Finished,
}

pub struct Sim {
    cfg: SimConfig,
    track: Track,
    cars: Vec<Car>,
    t: f32,
    tick: u64,
    phase: Phase,
    uid: u64,
    player: usize,
    rng: Rng,
    events: Vec<EventDetails>,
    sc_status: u8,
    sc_until_t: f32,
    sc_done: bool,
    sc_lap: u8,
    yellow_zone_until_t: f32,
    rain_start_t: f32,
    weather: u8,
    lights_shown: u8,
    chequered: bool,
    chequer_t: f32,
    finishers: usize,
    penalty_done: bool,
    damage_done: bool,
    history_cursor: usize,
    order: Vec<usize>,
    pub done: bool,
}

impl Sim {
    pub fn new(cfg: SimConfig) -> Self {
        let track = Track::generate(cfg.track_length_m);
        let mut rng = Rng(cfg.seed.max(1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let player = cfg.player_grid.min(21);
        let ref_lap = track.reference_lap_time();
        let laps = cfg.laps.max(3);
        let mut cars = Vec::with_capacity(22);
        for i in 0..22 {
            // Faster cars toward the front, with some spread.
            let mut pace = 1.012 - i as f32 * 0.0011 + rng.range(-0.002, 0.002);
            if i == player {
                pace = 1.012 - i as f32 * 0.0011;
            }
            if i == player + 1 {
                // The car behind the player is a touch quicker: "defend" scenario.
                pace = 1.012 - player as f32 * 0.0011 + 0.0035;
            }
            let compound = if i % 3 == 0 { Compound::Medium } else if i % 3 == 1 { Compound::Soft } else { Compound::Medium };
            let burn = 1.65 + rng.range(-0.05, 0.05);
            let fuel = if i == player { burn * laps as f32 * 0.975 } else { burn * (laps as f32 + 0.8) };
            let mut wear_mult = [1.1, 1.0, 0.9, 0.88];
            if i == player {
                wear_mult = [1.35, 1.05, 0.9, 0.85];
            }
            cars.push(Car {
                grid: i,
                team: (i / 2) as u8,
                race_number: [44, 63, 16, 55, 1, 11, 4, 81, 14, 18, 10, 31, 22, 3, 23, 2, 77, 24, 20, 27, 5, 6][i],
                name: if i == player { "Player".into() } else { NAMES[i].to_uppercase() },
                pace,
                total: -(i as f32) * 8.0 - 4.0,
                speed: 0.0,
                lap: 1,
                lap_start_t: 0.0,
                s1_ms: 0,
                s2_ms: 0,
                last_lap_ms: 0,
                best_lap_ms: 0,
                lap_history: Vec::new(),
                stints: vec![TyreStintHistory { end_lap: 255, tyre_actual_compound: compound.actual(), tyre_visual_compound: compound.visual() }],
                compound,
                tyre_age: if i == player { 0 } else { (rng.f() * 3.0) as u8 },
                wear: [0.0; 4],
                wear_mult,
                fuel_kg: fuel,
                burn_per_lap: burn,
                ers_j: 4.0e6,
                ers_deployed: 0.0,
                ers_harv_k: 0.0,
                ers_harv_h: 0.0,
                pit_stops: 0,
                pit_lap: if i == player { ((laps as f32) * 0.55).round() as u8 } else { ((laps as f32) * rng.range(0.4, 0.62)).round() as u8 },
                pit_timer: 0.0,
                finished: false,
                finish_order: None,
                penalties_s: 0,
                warnings: 0,
                damage: CarDamage::default(),
                surface_t: [75.0; 4],
                inner_t: [90.0; 4],
                brake_t: [200.0; 4],
                throttle: 0.0,
                brake: 0.0,
                steer: 0.0,
                gear: 1,
                lap_invalid: false,
            });
        }
        let race_duration = ref_lap * laps as f32;
        let uid = rng.next();
        Sim {
            rain_start_t: 8.0 + race_duration * 0.68,
            sc_lap: ((laps as f32) * 0.3).round().max(2.0) as u8,
            cfg,
            track,
            cars,
            t: 0.0,
            tick: 0,
            phase: Phase::Grid,
            uid,
            player,
            rng,
            events: vec![EventDetails::SessionStarted],
            sc_status: 0,
            sc_until_t: 0.0,
            sc_done: false,
            yellow_zone_until_t: 0.0,
            weather: 1,
            lights_shown: 0,
            chequered: false,
            chequer_t: 0.0,
            finishers: 0,
            penalty_done: false,
            damage_done: false,
            history_cursor: 0,
            order: (0..22).collect(),
            done: false,
        }
    }

    pub fn header(&self) -> Header {
        Header {
            session_uid: self.uid,
            session_time: self.t,
            frame_identifier: self.tick as u32,
            overall_frame_identifier: self.tick as u32,
            player_car_index: self.player as u8,
            secondary_player_car_index: 255,
            game_major_version: 1,
            game_minor_version: 0,
            packet_version: 1,
            ..Default::default()
        }
    }

    pub fn session_time(&self) -> f32 {
        self.t
    }

    fn race_session_type(&self) -> u8 {
        if self.cfg.format >= Format::F2024 {
            15
        } else {
            10
        }
    }

    fn lap_dist(&self, c: &Car) -> f32 {
        c.total - (c.lap as f32 - 1.0) * self.track.length
    }

    /// Advance by `dt` seconds and return the packets to send for this tick.
    pub fn step(&mut self, dt: f32) -> Vec<Packet> {
        if self.done {
            return Vec::new();
        }
        self.t += dt;
        self.tick += 1;
        match self.phase {
            Phase::Grid => self.step_grid(),
            Phase::Racing => self.step_race(dt),
            Phase::Finished => {}
        }
        self.update_order();
        self.emit()
    }

    fn step_grid(&mut self) {
        let n = ((self.t - 1.0).max(0.0)) as u8;
        if self.t >= 1.0 && n < 5 && n + 1 > self.lights_shown {
            self.lights_shown = n + 1;
            self.events.push(EventDetails::StartLights { num_lights: self.lights_shown });
        }
        if self.t >= 6.5 + (self.cfg.seed % 3) as f32 * 0.3 {
            self.events.push(EventDetails::LightsOut);
            self.phase = Phase::Racing;
            for c in &mut self.cars {
                c.lap_start_t = self.t;
            }
        }
    }

    fn leader_lap(&self) -> u8 {
        self.cars.iter().map(|c| c.lap).max().unwrap_or(1)
    }

    fn step_race(&mut self, dt: f32) {
        let fmt = self.cfg.format;
        let l = self.track.length;
        let laps = self.cfg.laps.max(3);

        // ---- Scripted race control.
        let leader_lap = self.leader_lap();
        if !self.sc_done && self.sc_status == 0 && leader_lap >= self.sc_lap && self.lap_dist(&self.cars[self.order[0]]) > l * 0.5 {
            self.sc_status = 1;
            self.sc_until_t = self.t + self.track.reference_lap_time() * 1.6;
            self.yellow_zone_until_t = self.t + 20.0;
            if fmt >= Format::F2024 {
                self.events.push(EventDetails::SafetyCar { sc_type: 1, event_type: 0 });
            }
        }
        if self.sc_status == 1 && self.t >= self.sc_until_t {
            self.sc_status = 0;
            self.sc_done = true;
            if fmt >= Format::F2024 {
                self.events.push(EventDetails::SafetyCar { sc_type: 1, event_type: 3 });
            }
        }
        if self.t >= self.rain_start_t {
            self.weather = if self.t >= self.rain_start_t + 240.0 { 4 } else { 3 };
        } else if self.t >= self.rain_start_t - 300.0 {
            self.weather = 2;
        }

        // ---- Cars.
        let order = self.order.clone();
        for (pos, &ci) in order.iter().enumerate() {
            let ahead = if pos > 0 { Some(order[pos - 1]) } else { None };
            let ahead_total = ahead.map(|a| self.cars[a].total);
            self.step_car(ci, ahead_total, dt, laps);
        }

        // ---- Player-specific script.
        let p = self.player;
        let p_lap = self.cars[p].lap;
        let p_ld = self.lap_dist(&self.cars[p]);
        if !self.damage_done && p_lap >= 3 && p_ld > l * 0.4 {
            self.damage_done = true;
            self.cars[p].damage.front_left_wing_damage = 18;
            self.cars[p].damage.floor_damage = 6;
            if fmt >= Format::F2024 {
                self.events.push(EventDetails::Collision { vehicle1_idx: p as u8, vehicle2_idx: (p + 1) as u8 });
            }
        }
        if !self.penalty_done && p_lap >= 4 && p_ld > l * 0.2 {
            self.penalty_done = true;
            self.cars[p].penalties_s += 5;
            self.cars[p].warnings += 1;
            self.events.push(EventDetails::Penalty {
                penalty_type: 4,
                infringement_type: 7,
                vehicle_idx: p as u8,
                other_vehicle_idx: 255,
                time: 5,
                lap_num: p_lap,
                places_gained: 255,
            });
        }

        // ---- Chequered flag / finish.
        if !self.chequered && self.cars.iter().any(|c| c.lap > laps) {
            self.chequered = true;
            self.chequer_t = self.t;
            self.events.push(EventDetails::ChequeredFlag);
        }
        if self.chequered && (self.finishers >= 22 || self.t - self.chequer_t > 120.0) {
            self.phase = Phase::Finished;
            self.events.push(EventDetails::SessionEnded);
        }
    }

    fn step_car(&mut self, ci: usize, ahead_total: Option<f32>, dt: f32, laps: u8) {
        let l = self.track.length;
        let t = self.t;
        let sc = self.sc_status == 1;
        let weather = self.weather;
        let noise = self.rng.range(-0.004, 0.004);
        let player = self.player;
        let track = &self.track;
        let c = &mut self.cars[ci];

        if c.finished {
            // Cool-down lap: cruise and then stop.
            c.speed = (c.speed - 5.0 * dt).max(0.0);
            c.total += c.speed * dt;
            c.throttle = 0.0;
            return;
        }

        if c.pit_timer > 0.0 {
            c.pit_timer -= dt;
            c.speed = 0.0;
            c.throttle = 0.0;
            c.brake = 0.0;
            if c.pit_timer <= 0.0 {
                // Fit new tyres: inters if it is raining, otherwise hards/mediums.
                let new = if weather >= 3 { Compound::Inter } else if c.compound == Compound::Soft { Compound::Medium } else { Compound::Hard };
                if let Some(last) = c.stints.last_mut() {
                    last.end_lap = c.lap.saturating_sub(1);
                }
                c.stints.push(TyreStintHistory { end_lap: 255, tyre_actual_compound: new.actual(), tyre_visual_compound: new.visual() });
                c.compound = new;
                c.tyre_age = 0;
                c.wear = [0.0; 4];
                if ci == player {
                    c.damage.front_left_wing_damage = 0; // wing changed at the stop
                }
            }
            return;
        }

        let s = c.total.rem_euclid(l);
        let avg_wear = c.wear.iter().sum::<f32>() / 4.0;
        let mut target = track.speed_at(s) * c.pace * c.compound.pace() * (1.0 - 0.0012 * avg_wear) * (1.0 + noise);
        if weather >= 3 && c.compound != Compound::Inter {
            target *= if weather >= 4 { 0.82 } else { 0.9 };
        }
        if c.damage.front_left_wing_damage > 0 {
            target *= 1.0 - c.damage.front_left_wing_damage as f32 * 0.0004;
        }
        if sc {
            let gap = ahead_total.map(|a| a - c.total).unwrap_or(1e9);
            let base = track.speed_at(s).min(70.0);
            target = if gap > 80.0 { base * 0.85 } else if gap < 30.0 { base * 0.5 } else { base * 0.65 };
        } else if let Some(a) = ahead_total {
            // Dirty air: limited ability to close within ~10 m.
            let gap = a - c.total;
            if gap > 0.0 && gap < 10.0 {
                target = target.min(track.speed_at(s) * 0.995);
            }
        }
        if target > c.speed {
            c.speed = (c.speed + 12.0 * dt).min(target);
        } else {
            c.speed = (c.speed - 40.0 * dt).max(target);
        }
        let ds = c.speed * dt;
        c.total += ds;

        // Inputs (derived from the speed profile).
        let accel = track.accel_at(s);
        let curv = track.curvature_at(s);
        c.throttle = if sc { 0.45 } else if accel > 0.5 { 1.0 } else if accel > -3.0 { 0.65 } else { 0.0 };
        c.brake = if !sc && accel < -5.0 { (-accel / 38.0).min(1.0) } else { 0.0 };
        c.steer = (curv * 45.0).clamp(-1.0, 1.0);
        c.gear = ((c.speed / 12.0) as i8 + 1).clamp(1, 8);

        // Tyres, fuel, temperatures, ERS.
        let frac = ds / l;
        let rate = c.compound.wear_per_lap() * if sc { 0.4 } else { 1.0 };
        for w in 0..4 {
            c.wear[w] += rate * c.wear_mult[w] * frac;
        }
        c.fuel_kg = (c.fuel_kg - c.burn_per_lap * frac * if sc { 0.6 } else { 1.0 }).max(0.0);
        let lat = curv.abs() * c.speed * c.speed / 40.0;
        let k = (dt * 0.5).min(1.0);
        for w in 0..4 {
            let front = if w < 2 { 1.0 } else { 0.8 };
            let surf_target = if weather >= 3 && c.compound != Compound::Inter { 70.0 } else { 82.0 } + 18.0 * lat * front + 6.0 * c.throttle;
            c.surface_t[w] += (surf_target - c.surface_t[w]) * k * 2.0;
            c.inner_t[w] += (92.0 + 0.3 * (c.surface_t[w] - 85.0) + 0.05 * c.wear[w] - c.inner_t[w]) * k * 0.3;
            let brake_target = 280.0 + c.brake * 700.0 * if w < 2 { 1.0 } else { 0.75 };
            c.brake_t[w] += (brake_target - c.brake_t[w]) * (dt * 1.5).min(1.0);
        }
        if accel > 0.5 && c.speed > 55.0 && !sc {
            let j = 120_000.0 * dt;
            c.ers_j -= j;
            c.ers_deployed += j;
        } else if c.brake > 0.1 {
            let j = 160_000.0 * dt;
            c.ers_j += j;
            c.ers_harv_k += j * 0.7;
            c.ers_harv_h += j * 0.3;
        } else {
            let j = 15_000.0 * dt;
            c.ers_j += j;
            c.ers_harv_h += j;
        }
        c.ers_j = c.ers_j.clamp(0.0, 4.0e6);

        // Sectors and lap completion.
        let ld = c.total - (c.lap as f32 - 1.0) * l;
        let elapsed_ms = ((t - c.lap_start_t) * 1000.0) as u32;
        if c.s1_ms == 0 && ld >= l / 3.0 {
            c.s1_ms = elapsed_ms;
        }
        if c.s2_ms == 0 && ld >= 2.0 * l / 3.0 && c.s1_ms > 0 {
            c.s2_ms = elapsed_ms - c.s1_ms;
        }
        if ld >= l {
            let lap_ms = elapsed_ms;
            let s3 = lap_ms.saturating_sub(c.s1_ms + c.s2_ms);
            c.last_lap_ms = lap_ms;
            if !c.lap_invalid && (c.best_lap_ms == 0 || lap_ms < c.best_lap_ms) {
                c.best_lap_ms = lap_ms;
            }
            c.lap_history.push(LapHistory {
                lap_time_ms: lap_ms,
                sector1_time_ms: c.s1_ms,
                sector2_time_ms: c.s2_ms,
                sector3_time_ms: s3,
                lap_valid_bit_flags: if c.lap_invalid { 0 } else { 0xF },
            });
            let completed = c.lap;
            c.lap += 1;
            c.lap_start_t = t;
            c.s1_ms = 0;
            c.s2_ms = 0;
            c.lap_invalid = false;
            c.tyre_age = c.tyre_age.saturating_add(1);
            c.ers_deployed = 0.0;
            c.ers_harv_k = 0.0;
            c.ers_harv_h = 0.0;
            if completed >= laps {
                c.finished = true;
            } else if completed == c.pit_lap && c.pit_stops == 0 {
                c.pit_stops += 1;
                c.pit_timer = 21.0 + if ci == player { 3.0 } else { 0.0 };
            }
        }
    }

    fn update_order(&mut self) {
        let mut idx: Vec<usize> = (0..22).collect();
        // Finished cars keep their finishing order at the front.
        let mut next_finish = self.finishers;
        for i in 0..22 {
            if self.cars[i].finished && self.cars[i].finish_order.is_none() {
                self.cars[i].finish_order = Some(next_finish);
                next_finish += 1;
            }
        }
        self.finishers = next_finish;
        idx.sort_by(|&a, &b| {
            let ca = &self.cars[a];
            let cb = &self.cars[b];
            match (ca.finish_order, cb.finish_order) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => {
                    if self.phase == Phase::Grid {
                        ca.grid.cmp(&cb.grid)
                    } else {
                        cb.total.partial_cmp(&ca.total).unwrap_or(std::cmp::Ordering::Equal)
                    }
                }
            }
        });
        self.order = idx;
    }

    fn position_of(&self, ci: usize) -> u8 {
        self.order.iter().position(|&x| x == ci).unwrap_or(ci) as u8 + 1
    }

    fn emit(&mut self) -> Vec<Packet> {
        let rate = self.cfg.rate_hz.max(1.0) as u64;
        let mut out = Vec::new();
        for e in self.events.drain(..) {
            out.push(Packet::Event(EventPacket { details: e }));
        }
        if self.phase == Phase::Finished {
            out.push(Packet::FinalClassification(Box::new(self.final_classification())));
            self.done = true;
            return out;
        }
        out.push(Packet::Motion(Box::new(self.motion())));
        out.push(Packet::LapData(Box::new(self.lap_data())));
        out.push(Packet::CarTelemetry(Box::new(self.telemetry())));
        out.push(Packet::CarStatus(Box::new(self.status())));
        if self.tick.is_multiple_of(2) {
            out.push(Packet::CarDamage(Box::new(self.damage())));
        }
        if self.tick % (rate / 2).max(1) == 1 || self.tick == 1 {
            out.push(Packet::Session(Box::new(self.session())));
        }
        if self.tick % (rate * 5) == 1 || self.tick == 1 {
            out.push(Packet::Participants(Box::new(self.participants())));
            out.push(Packet::CarSetups(Box::new(self.setups())));
        }
        if self.tick.is_multiple_of((rate / 10).max(1)) {
            out.push(Packet::SessionHistory(Box::new(self.history(self.history_cursor))));
            self.history_cursor = (self.history_cursor + 1) % 22;
        }
        out
    }

    fn motion(&self) -> MotionPacket {
        let mut p = MotionPacket::default();
        for (i, c) in self.cars.iter().enumerate() {
            let s = c.total.rem_euclid(self.track.length);
            let (x, z) = self.track.position(s);
            let (hx, hz) = self.track.heading(s);
            let m = &mut p.cars[i];
            m.world_position = [x, 0.0, z];
            m.world_velocity = [hx * c.speed, 0.0, hz * c.speed];
            m.world_forward_dir = [(hx * 32767.0) as i16, 0, (hz * 32767.0) as i16];
            m.world_right_dir = [(-hz * 32767.0) as i16, 0, (hx * 32767.0) as i16];
            m.g_force_lateral = c.steer * 3.5;
            m.g_force_longitudinal = c.throttle - c.brake * 4.0;
            m.g_force_vertical = 1.0;
            m.yaw = hz.atan2(hx);
        }
        p
    }

    fn lap_data(&self) -> LapDataPacket {
        let l = self.track.length;
        let mut p = LapDataPacket { time_trial_pb_car_idx: 255, time_trial_rival_car_idx: 255, ..Default::default() };
        let leader = self.order[0];
        for (i, c) in self.cars.iter().enumerate() {
            let pos = self.position_of(i);
            let ahead = if pos > 1 { Some(self.order[pos as usize - 2]) } else { None };
            let to_time = |gap_m: f32| -> u32 { ((gap_m / c.speed.max(30.0)) * 1000.0).max(0.0) as u32 };
            let ld = c.total - (c.lap as f32 - 1.0) * l;
            let d = &mut p.cars[i];
            d.last_lap_time_ms = c.last_lap_ms;
            d.current_lap_time_ms = if self.phase == Phase::Racing { ((self.t - c.lap_start_t) * 1000.0) as u32 } else { 0 };
            d.sector1_time_ms = c.s1_ms;
            d.sector2_time_ms = c.s2_ms;
            if self.cfg.format >= Format::F2023 {
                d.delta_to_car_in_front_ms = Some(ahead.map(|a| to_time(self.cars[a].total - c.total)).unwrap_or(0));
                d.delta_to_race_leader_ms = Some(if i == leader { 0 } else { to_time(self.cars[leader].total - c.total) });
            }
            d.lap_distance = ld;
            d.total_distance = c.total;
            d.car_position = pos;
            d.current_lap_num = c.lap.min(self.cfg.laps.max(3));
            d.pit_status = if c.pit_timer > 0.0 { 2 } else { 0 };
            d.num_pit_stops = c.pit_stops;
            d.sector = if ld < l / 3.0 { 0 } else if ld < 2.0 * l / 3.0 { 1 } else { 2 };
            d.current_lap_invalid = c.lap_invalid;
            d.penalties = c.penalties_s;
            d.total_warnings = c.warnings;
            d.grid_position = c.grid as u8 + 1;
            d.driver_status = if c.pit_timer > 0.0 { 0 } else { 4 };
            d.result_status = if c.finished { 3 } else { 2 };
            d.pit_lane_timer_active = c.pit_timer > 0.0;
        }
        p
    }

    fn telemetry(&self) -> CarTelemetryPacket {
        let mut p = CarTelemetryPacket { suggested_gear: 0, ..Default::default() };
        for (i, c) in self.cars.iter().enumerate() {
            let t = &mut p.cars[i];
            t.speed_kph = (c.speed * 3.6) as u16;
            t.throttle = c.throttle;
            t.brake = c.brake;
            t.steer = c.steer;
            t.gear = if c.speed < 0.1 && self.phase != Phase::Racing { 0 } else { c.gear };
            t.engine_rpm = (6000.0 + (c.speed % 12.0) / 12.0 * 6000.0) as u16;
            t.drs = false;
            t.rev_lights_percent = ((c.speed % 12.0) / 12.0 * 100.0) as u8;
            t.brakes_temperature = c.brake_t.map(|v| v as u16);
            t.tyres_surface_temperature = c.surface_t.map(|v| v.clamp(0.0, 255.0) as u8);
            t.tyres_inner_temperature = c.inner_t.map(|v| v.clamp(0.0, 255.0) as u8);
            t.engine_temperature = 105;
            t.tyres_pressure = [23.5, 23.5, 21.5, 21.5];
        }
        p
    }

    fn laps_remaining(&self, c: &Car) -> f32 {
        let laps = self.cfg.laps.max(3) as f32;
        let frac = (c.total / self.track.length).max(0.0);
        (laps - frac).max(0.0)
    }

    fn status(&self) -> CarStatusPacket {
        let mut p = CarStatusPacket::default();
        for (i, c) in self.cars.iter().enumerate() {
            let s = &mut p.cars[i];
            s.traction_control = 0;
            s.anti_lock_brakes = false;
            s.fuel_mix = 1;
            s.front_brake_bias = 56;
            s.fuel_in_tank = c.fuel_kg;
            s.fuel_capacity = 110.0;
            s.fuel_remaining_laps = c.fuel_kg / c.burn_per_lap - self.laps_remaining(c);
            s.max_rpm = 13000;
            s.idle_rpm = 4000;
            s.max_gears = 8;
            s.drs_allowed = 0;
            s.actual_tyre_compound = c.compound.actual();
            s.visual_tyre_compound = c.compound.visual();
            s.tyres_age_laps = c.tyre_age;
            s.engine_power_ice = 600_000.0;
            s.engine_power_mguk = 120_000.0;
            s.ers_store_energy = c.ers_j;
            s.ers_deploy_mode = 1;
            s.ers_harvested_this_lap_mguk = c.ers_harv_k;
            s.ers_harvested_this_lap_mguh = c.ers_harv_h;
            s.ers_deployed_this_lap = c.ers_deployed;
        }
        p
    }

    fn damage(&self) -> CarDamagePacket {
        let mut p = CarDamagePacket::default();
        let race_frac = (self.cars[self.player].total / (self.track.length * self.cfg.laps.max(3) as f32)).clamp(0.0, 1.0);
        for (i, c) in self.cars.iter().enumerate() {
            let mut d = c.damage;
            d.tyres_wear = c.wear;
            d.tyres_damage = c.wear.map(|w| (w * 0.4) as u8);
            d.brakes_damage = [1, 1, 1, 1];
            d.engine_ice_wear = (8.0 + race_frac * 3.0) as u8;
            d.engine_mguh_wear = (12.0 + race_frac * 4.0) as u8;
            d.engine_mguk_wear = (10.0 + race_frac * 3.0) as u8;
            d.engine_es_wear = 9;
            d.engine_ce_wear = 7;
            d.engine_tc_wear = (11.0 + race_frac * 3.0) as u8;
            d.gearbox_damage = (14.0 + race_frac * 6.0) as u8;
            p.cars[i] = d;
        }
        p
    }

    fn session(&self) -> SessionPacket {
        let player = &self.cars[self.player];
        let mins_to_rain = (self.rain_start_t - self.t) / 60.0;
        let race_type = self.race_session_type();
        let forecast = [0u8, 5, 10, 15, 30, 45, 60]
            .iter()
            .map(|&off| {
                let until = mins_to_rain - off as f32;
                let (weather, rain) = if until <= 0.0 {
                    (if until < -4.0 { 4 } else { 3 }, 85)
                } else if until < 6.0 {
                    (2, (100.0 - until * 9.0).clamp(0.0, 100.0) as u8)
                } else {
                    (1, (40.0 - until * 2.0).clamp(0.0, 100.0) as u8)
                };
                ForecastSample {
                    session_type: race_type,
                    time_offset_min: off,
                    weather,
                    track_temperature: if weather >= 3 { 26 } else { 34 },
                    track_temperature_change: if weather >= 3 { 1 } else { 2 },
                    air_temperature: if weather >= 3 { 19 } else { 24 },
                    air_temperature_change: 2,
                    rain_percentage: rain,
                }
            })
            .collect();
        let zones = (0..10)
            .map(|z| MarshalZone {
                zone_start: z as f32 / 10.0,
                zone_flag: if (self.t < self.yellow_zone_until_t && z == 4) || self.sc_status == 1 { 3 } else { 1 },
            })
            .collect();
        let raining = self.weather >= 3;
        SessionPacket {
            weather: self.weather,
            track_temperature: if raining { 27 } else { 34 },
            air_temperature: if raining { 20 } else { 24 },
            total_laps: self.cfg.laps.max(3),
            track_length: self.track.length as u16,
            session_type: race_type,
            track_id: 3,
            formula: 0,
            session_time_left: 7200u16.saturating_sub(self.t as u16),
            session_duration: 7200,
            pit_speed_limit: 80,
            spectator_car_index: 255,
            marshal_zones: zones,
            safety_car_status: self.sc_status,
            network_game: false,
            weather_forecast: forecast,
            forecast_accuracy: 0,
            ai_difficulty: 90,
            pit_stop_window_ideal_lap: player.pit_lap,
            pit_stop_window_latest_lap: player.pit_lap + 3,
            pit_stop_rejoin_position: 12,
            num_safety_car_periods: u8::from(self.sc_done || self.sc_status == 1),
            sector2_lap_distance_start: self.track.length / 3.0,
            sector3_lap_distance_start: self.track.length * 2.0 / 3.0,
            ..Default::default()
        }
    }

    fn participants(&self) -> ParticipantsPacket {
        let mut p = ParticipantsPacket { num_active_cars: 22, ..Default::default() };
        for (i, c) in self.cars.iter().enumerate() {
            p.participants[i] = Participant {
                ai_controlled: i != self.player,
                driver_id: if i == self.player { 255 } else { 100 + i as u8 },
                network_id: 255,
                team_id: c.team,
                my_team: false,
                race_number: c.race_number,
                nationality: 0,
                name: c.name.clone(),
                your_telemetry: 1,
                show_online_names: true,
                tech_level: 0,
                platform: 255,
            };
        }
        p
    }

    fn setups(&self) -> CarSetupsPacket {
        let mut p = CarSetupsPacket::default();
        for s in p.cars.iter_mut() {
            *s = CarSetup {
                front_wing: 24,
                rear_wing: 20,
                on_throttle: 60,
                off_throttle: 55,
                front_camber: -2.8,
                rear_camber: -1.2,
                front_toe: 0.05,
                rear_toe: 0.2,
                front_suspension: 22,
                rear_suspension: 8,
                front_anti_roll_bar: 10,
                rear_anti_roll_bar: 6,
                front_suspension_height: 22,
                rear_suspension_height: 50,
                brake_pressure: 100,
                brake_bias: 56,
                engine_braking: 50,
                tyre_pressures: [23.5, 23.5, 21.5, 21.5],
                ballast: 6,
                fuel_load: 30.0,
            };
        }
        p
    }

    fn history(&self, ci: usize) -> SessionHistoryPacket {
        let c = &self.cars[ci];
        let best = c
            .lap_history
            .iter()
            .enumerate()
            .filter(|(_, l)| l.lap_valid_bit_flags & 1 == 1)
            .min_by_key(|(_, l)| l.lap_time_ms)
            .map(|(i, _)| i as u8 + 1)
            .unwrap_or(0);
        SessionHistoryPacket {
            car_idx: ci as u8,
            best_lap_time_lap_num: best,
            laps: c.lap_history.iter().rev().take(MAX_LAP_HISTORY).rev().cloned().collect(),
            tyre_stints: c.stints.iter().take(MAX_TYRE_STINTS).cloned().collect(),
            ..Default::default()
        }
    }

    fn final_classification(&self) -> FinalClassificationPacket {
        let mut p = FinalClassificationPacket { num_cars: 22, ..Default::default() };
        let points = [25u8, 18, 15, 12, 10, 8, 6, 4, 2, 1];
        for (i, c) in self.cars.iter().enumerate() {
            let pos = self.position_of(i);
            let mut f = FinalClassification {
                position: pos,
                num_laps: c.lap_history.len() as u8,
                grid_position: c.grid as u8 + 1,
                points: points.get(pos as usize - 1).copied().unwrap_or(0),
                num_pit_stops: c.pit_stops,
                result_status: if c.finished { 3 } else { 2 },
                best_lap_time_ms: c.best_lap_ms,
                total_race_time_s: c.lap_history.iter().map(|l| l.lap_time_ms as f64 / 1000.0).sum(),
                penalties_time: c.penalties_s,
                num_penalties: u8::from(c.penalties_s > 0),
                num_tyre_stints: c.stints.len().min(MAX_TYRE_STINTS) as u8,
                ..Default::default()
            };
            for (k, s) in c.stints.iter().take(MAX_TYRE_STINTS).enumerate() {
                f.tyre_stints_actual[k] = s.tyre_actual_compound;
                f.tyre_stints_visual[k] = s.tyre_visual_compound;
                f.tyre_stints_end_laps[k] = if s.end_lap == 255 { c.lap_history.len() as u8 } else { s.end_lap };
            }
            p.cars[i] = f;
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_race_runs_to_completion_with_scenario_events() {
        for format in Format::ALL {
            let mut sim = Sim::new(SimConfig { format, laps: 5, player_grid: 7, seed: 7, rate_hz: 20.0, track_length_m: 3000.0 });
            let mut saw = std::collections::HashSet::new();
            let mut sc_seen = false;
            let mut steps = 0;
            while !sim.done && steps < 20 * 60 * 20 {
                for p in sim.step(0.05) {
                    let bytes = encode(format, &sim.header(), &p);
                    let parsed = decode(&bytes).expect("synth output must decode");
                    match parsed.packet {
                        Packet::Event(e) => {
                            saw.insert(e.details.code());
                        }
                        Packet::Session(s) if s.safety_car_status == 1 => sc_seen = true,
                        _ => {}
                    }
                }
                steps += 1;
            }
            assert!(sim.done, "{format:?}: race did not finish");
            for code in [b"SSTA", b"LGOT", b"PENA", b"CHQF", b"SEND"] {
                assert!(saw.contains(code), "{format:?}: missing event {}", String::from_utf8_lossy(code));
            }
            assert!(sc_seen, "{format:?}: no safety car period");
        }
    }
}
