//! Version-neutral packet structs plus their per-format layouts.
//!
//! Each struct holds the union of fields across F1 22–25. Fields that do not
//! exist in a given format stay at their default (or `None`) after decoding.
//! All wheel arrays are in model order **`[FL, FR, RL, RR]`**.

use crate::codec::{
    counted_slots, fixed_str, split_time, wheels_f32, wheels_u16, wheels_u8, Codec, Result,
};
use crate::Format;
use serde::{Deserialize, Serialize};

pub const MAX_CARS: usize = 22;
pub const MAX_MARSHAL_ZONES: usize = 21;
pub const MAX_LAP_HISTORY: usize = 100;
pub const MAX_TYRE_STINTS: usize = 8;

pub type Wheels<T> = [T; 4];

/// Index helpers for model-order wheel arrays.
pub mod wheel {
    pub const FL: usize = 0;
    pub const FR: usize = 1;
    pub const RL: usize = 2;
    pub const RR: usize = 3;
}

fn cars<C: Codec, T>(
    c: &mut C,
    cars: &mut [T; MAX_CARS],
    fmt: Format,
    each: impl Fn(&mut C, &mut T, Format) -> Result<()>,
) -> Result<()> {
    for car in cars.iter_mut() {
        each(c, car, fmt)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- Motion (0)

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CarMotion {
    pub world_position: [f32; 3],
    pub world_velocity: [f32; 3],
    pub world_forward_dir: [i16; 3],
    pub world_right_dir: [i16; 3],
    pub g_force_lateral: f32,
    pub g_force_longitudinal: f32,
    pub g_force_vertical: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MotionPacket {
    pub cars: [CarMotion; MAX_CARS],
}

fn car_motion<C: Codec>(c: &mut C, m: &mut CarMotion, _fmt: Format) -> Result<()> {
    for v in &mut m.world_position {
        c.f32(v)?;
    }
    for v in &mut m.world_velocity {
        c.f32(v)?;
    }
    for v in &mut m.world_forward_dir {
        c.i16(v)?;
    }
    for v in &mut m.world_right_dir {
        c.i16(v)?;
    }
    c.f32(&mut m.g_force_lateral)?;
    c.f32(&mut m.g_force_longitudinal)?;
    c.f32(&mut m.g_force_vertical)?;
    c.f32(&mut m.yaw)?;
    c.f32(&mut m.pitch)?;
    c.f32(&mut m.roll)
}

pub fn motion<C: Codec>(c: &mut C, p: &mut MotionPacket, fmt: Format) -> Result<()> {
    cars(c, &mut p.cars, fmt, car_motion)?;
    if fmt == Format::F2022 {
        // Player-only extended motion data (suspension, wheel speed/slip,
        // local velocity, angular velocity/acceleration, front wheel angle).
        // Moved to the separate Motion Ex packet (ID 13) from F1 23.
        c.skip(120)?;
    }
    Ok(())
}

// --------------------------------------------------------------- Session (1)

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct MarshalZone {
    /// Fraction (0..1) of the way through the lap the zone starts.
    pub zone_start: f32,
    /// -1 invalid/unknown, 0 none, 1 green, 2 blue, 3 yellow, 4 red.
    pub zone_flag: i8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ForecastSample {
    pub session_type: u8,
    pub time_offset_min: u8,
    pub weather: u8,
    pub track_temperature: i8,
    pub track_temperature_change: i8,
    pub air_temperature: i8,
    pub air_temperature_change: i8,
    pub rain_percentage: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionPacket {
    /// 0 clear, 1 light cloud, 2 overcast, 3 light rain, 4 heavy rain, 5 storm.
    pub weather: u8,
    pub track_temperature: i8,
    pub air_temperature: i8,
    pub total_laps: u8,
    pub track_length: u16,
    pub session_type: u8,
    pub track_id: i8,
    pub formula: u8,
    pub session_time_left: u16,
    pub session_duration: u16,
    pub pit_speed_limit: u8,
    pub game_paused: bool,
    pub is_spectating: bool,
    pub spectator_car_index: u8,
    pub marshal_zones: Vec<MarshalZone>,
    /// 0 none, 1 full, 2 virtual, 3 formation lap.
    pub safety_car_status: u8,
    pub network_game: bool,
    pub weather_forecast: Vec<ForecastSample>,
    /// 0 perfect, 1 approximate.
    pub forecast_accuracy: u8,
    pub ai_difficulty: u8,
    pub pit_stop_window_ideal_lap: u8,
    pub pit_stop_window_latest_lap: u8,
    pub pit_stop_rejoin_position: u8,
    /// F1 23+.
    pub num_safety_car_periods: u8,
    pub num_virtual_safety_car_periods: u8,
    pub num_red_flag_periods: u8,
    /// F1 24+ (metres).
    pub sector2_lap_distance_start: f32,
    pub sector3_lap_distance_start: f32,
}

pub fn session<C: Codec>(c: &mut C, p: &mut SessionPacket, fmt: Format) -> Result<()> {
    c.u8(&mut p.weather)?;
    c.i8(&mut p.track_temperature)?;
    c.i8(&mut p.air_temperature)?;
    c.u8(&mut p.total_laps)?;
    c.u16(&mut p.track_length)?;
    c.u8(&mut p.session_type)?;
    c.i8(&mut p.track_id)?;
    c.u8(&mut p.formula)?;
    c.u16(&mut p.session_time_left)?;
    c.u16(&mut p.session_duration)?;
    c.u8(&mut p.pit_speed_limit)?;
    c.bool(&mut p.game_paused)?;
    c.bool(&mut p.is_spectating)?;
    c.u8(&mut p.spectator_car_index)?;
    c.skip(1)?; // sliProNativeSupport
    counted_slots(c, &mut p.marshal_zones, MAX_MARSHAL_ZONES, 5, |c, z| {
        c.f32(&mut z.zone_start)?;
        c.i8(&mut z.zone_flag)
    })?;
    c.u8(&mut p.safety_car_status)?;
    c.bool(&mut p.network_game)?;
    let max_samples = if fmt >= Format::F2024 { 64 } else { 56 };
    counted_slots(c, &mut p.weather_forecast, max_samples, 8, |c, s| {
        c.u8(&mut s.session_type)?;
        c.u8(&mut s.time_offset_min)?;
        c.u8(&mut s.weather)?;
        c.i8(&mut s.track_temperature)?;
        c.i8(&mut s.track_temperature_change)?;
        c.i8(&mut s.air_temperature)?;
        c.i8(&mut s.air_temperature_change)?;
        c.u8(&mut s.rain_percentage)
    })?;
    c.u8(&mut p.forecast_accuracy)?;
    c.u8(&mut p.ai_difficulty)?;
    c.skip(12)?; // season / weekend / session link identifiers
    c.u8(&mut p.pit_stop_window_ideal_lap)?;
    c.u8(&mut p.pit_stop_window_latest_lap)?;
    c.u8(&mut p.pit_stop_rejoin_position)?;
    // 11 assist / mode bytes, timeOfDay (u32), sessionLength (u8)
    c.skip(11 + 4 + 1)?;
    if fmt >= Format::F2023 {
        c.skip(4)?; // speed / temperature units for both players
        c.u8(&mut p.num_safety_car_periods)?;
        c.u8(&mut p.num_virtual_safety_car_periods)?;
        c.u8(&mut p.num_red_flag_periods)?;
    }
    if fmt >= Format::F2024 {
        c.skip(24)?; // simulation / ruleset settings
        c.skip(1 + 12)?; // numSessionsInWeekend + weekendStructure[12]
        c.f32(&mut p.sector2_lap_distance_start)?;
        c.f32(&mut p.sector3_lap_distance_start)?;
    }
    Ok(())
}

// -------------------------------------------------------------- Lap Data (2)

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct LapData {
    pub last_lap_time_ms: u32,
    pub current_lap_time_ms: u32,
    pub sector1_time_ms: u32,
    pub sector2_time_ms: u32,
    /// F1 23+ only.
    pub delta_to_car_in_front_ms: Option<u32>,
    /// F1 23+ only.
    pub delta_to_race_leader_ms: Option<u32>,
    pub lap_distance: f32,
    pub total_distance: f32,
    pub safety_car_delta: f32,
    pub car_position: u8,
    pub current_lap_num: u8,
    /// 0 none, 1 pitting, 2 in pit area.
    pub pit_status: u8,
    pub num_pit_stops: u8,
    /// 0 = sector 1, 1 = sector 2, 2 = sector 3.
    pub sector: u8,
    pub current_lap_invalid: bool,
    /// Accumulated time penalties in seconds.
    pub penalties: u8,
    pub total_warnings: u8,
    pub corner_cutting_warnings: u8,
    pub num_unserved_drive_through_pens: u8,
    pub num_unserved_stop_go_pens: u8,
    pub grid_position: u8,
    /// 0 garage, 1 flying lap, 2 in lap, 3 out lap, 4 on track.
    pub driver_status: u8,
    /// 0 invalid, 1 inactive, 2 active, 3 finished, 4 DNF, 5 DSQ, 6 not classified, 7 retired.
    pub result_status: u8,
    pub pit_lane_timer_active: bool,
    pub pit_lane_time_in_lane_ms: u16,
    pub pit_stop_timer_ms: u16,
    pub pit_stop_should_serve_pen: bool,
    /// F1 24+.
    pub speed_trap_fastest_speed: f32,
    pub speed_trap_fastest_lap: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LapDataPacket {
    pub cars: [LapData; MAX_CARS],
    pub time_trial_pb_car_idx: u8,
    pub time_trial_rival_car_idx: u8,
}

fn opt_delta<C: Codec>(c: &mut C, v: &mut Option<u32>, fmt: Format) -> Result<()> {
    if fmt < Format::F2023 {
        if c.is_reader() {
            *v = None;
        }
        return Ok(());
    }
    let mut t = v.unwrap_or(0);
    split_time(c, &mut t, fmt >= Format::F2024)?;
    if c.is_reader() {
        *v = Some(t);
    }
    Ok(())
}

fn lap_data<C: Codec>(c: &mut C, l: &mut LapData, fmt: Format) -> Result<()> {
    let minutes = fmt >= Format::F2023;
    c.u32(&mut l.last_lap_time_ms)?;
    c.u32(&mut l.current_lap_time_ms)?;
    split_time(c, &mut l.sector1_time_ms, minutes)?;
    split_time(c, &mut l.sector2_time_ms, minutes)?;
    opt_delta(c, &mut l.delta_to_car_in_front_ms, fmt)?;
    opt_delta(c, &mut l.delta_to_race_leader_ms, fmt)?;
    c.f32(&mut l.lap_distance)?;
    c.f32(&mut l.total_distance)?;
    c.f32(&mut l.safety_car_delta)?;
    c.u8(&mut l.car_position)?;
    c.u8(&mut l.current_lap_num)?;
    c.u8(&mut l.pit_status)?;
    c.u8(&mut l.num_pit_stops)?;
    c.u8(&mut l.sector)?;
    c.bool(&mut l.current_lap_invalid)?;
    c.u8(&mut l.penalties)?;
    c.u8(&mut l.total_warnings)?;
    if fmt >= Format::F2023 {
        c.u8(&mut l.corner_cutting_warnings)?;
    }
    c.u8(&mut l.num_unserved_drive_through_pens)?;
    c.u8(&mut l.num_unserved_stop_go_pens)?;
    c.u8(&mut l.grid_position)?;
    c.u8(&mut l.driver_status)?;
    c.u8(&mut l.result_status)?;
    c.bool(&mut l.pit_lane_timer_active)?;
    c.u16(&mut l.pit_lane_time_in_lane_ms)?;
    c.u16(&mut l.pit_stop_timer_ms)?;
    c.bool(&mut l.pit_stop_should_serve_pen)?;
    if fmt >= Format::F2024 {
        c.f32(&mut l.speed_trap_fastest_speed)?;
        c.u8(&mut l.speed_trap_fastest_lap)?;
    }
    Ok(())
}

pub fn lap_data_packet<C: Codec>(c: &mut C, p: &mut LapDataPacket, fmt: Format) -> Result<()> {
    cars(c, &mut p.cars, fmt, lap_data)?;
    c.u8(&mut p.time_trial_pb_car_idx)?;
    c.u8(&mut p.time_trial_rival_car_idx)
}

// ----------------------------------------------------------------- Event (3)

/// Bit for `UDP Action N` (N = 1..=12) in the `BUTN` button status.
pub fn udp_action_bit(n: u8) -> u32 {
    debug_assert!((1..=12).contains(&n));
    0x0010_0000u32 << (n.clamp(1, 12) - 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[derive(Default)]
pub enum EventDetails {
    #[default]
    SessionStarted,
    SessionEnded,
    FastestLap { vehicle_idx: u8, lap_time_s: f32 },
    Retirement { vehicle_idx: u8 },
    DrsEnabled,
    DrsDisabled,
    TeamMateInPits { vehicle_idx: u8 },
    ChequeredFlag,
    RaceWinner { vehicle_idx: u8 },
    Penalty {
        penalty_type: u8,
        infringement_type: u8,
        vehicle_idx: u8,
        other_vehicle_idx: u8,
        time: u8,
        lap_num: u8,
        places_gained: u8,
    },
    SpeedTrap { vehicle_idx: u8, speed_kph: f32 },
    StartLights { num_lights: u8 },
    LightsOut,
    DriveThroughServed { vehicle_idx: u8 },
    StopGoServed { vehicle_idx: u8 },
    Flashback { frame_identifier: u32, session_time: f32 },
    Buttons { status: u32 },
    /// F1 23+.
    RedFlag,
    /// F1 23+.
    Overtake { overtaking_idx: u8, overtaken_idx: u8 },
    /// F1 24+. `sc_type`: 0 none, 1 full, 2 virtual, 3 formation.
    /// `event_type`: 0 deployed, 1 returning, 2 returned, 3 resume race.
    SafetyCar { sc_type: u8, event_type: u8 },
    /// F1 24+.
    Collision { vehicle1_idx: u8, vehicle2_idx: u8 },
    Other { code: [u8; 4] },
}


impl EventDetails {
    pub fn code(&self) -> [u8; 4] {
        use EventDetails::*;
        *match self {
            SessionStarted => b"SSTA",
            SessionEnded => b"SEND",
            FastestLap { .. } => b"FTLP",
            Retirement { .. } => b"RTMT",
            DrsEnabled => b"DRSE",
            DrsDisabled => b"DRSD",
            TeamMateInPits { .. } => b"TMPT",
            ChequeredFlag => b"CHQF",
            RaceWinner { .. } => b"RCWN",
            Penalty { .. } => b"PENA",
            SpeedTrap { .. } => b"SPTP",
            StartLights { .. } => b"STLG",
            LightsOut => b"LGOT",
            DriveThroughServed { .. } => b"DTSV",
            StopGoServed { .. } => b"SGSV",
            Flashback { .. } => b"FLBK",
            Buttons { .. } => b"BUTN",
            RedFlag => b"RDFL",
            Overtake { .. } => b"OVTK",
            SafetyCar { .. } => b"SCAR",
            Collision { .. } => b"COLL",
            Other { code } => code,
        }
    }

    fn from_code(code: [u8; 4]) -> Self {
        use EventDetails::*;
        match &code {
            b"SSTA" => SessionStarted,
            b"SEND" => SessionEnded,
            b"FTLP" => FastestLap { vehicle_idx: 0, lap_time_s: 0.0 },
            b"RTMT" => Retirement { vehicle_idx: 0 },
            b"DRSE" => DrsEnabled,
            b"DRSD" => DrsDisabled,
            b"TMPT" => TeamMateInPits { vehicle_idx: 0 },
            b"CHQF" => ChequeredFlag,
            b"RCWN" => RaceWinner { vehicle_idx: 0 },
            b"PENA" => Penalty {
                penalty_type: 0,
                infringement_type: 0,
                vehicle_idx: 0,
                other_vehicle_idx: 0,
                time: 0,
                lap_num: 0,
                places_gained: 0,
            },
            b"SPTP" => SpeedTrap { vehicle_idx: 0, speed_kph: 0.0 },
            b"STLG" => StartLights { num_lights: 0 },
            b"LGOT" => LightsOut,
            b"DTSV" => DriveThroughServed { vehicle_idx: 0 },
            b"SGSV" => StopGoServed { vehicle_idx: 0 },
            b"FLBK" => Flashback { frame_identifier: 0, session_time: 0.0 },
            b"BUTN" => Buttons { status: 0 },
            b"RDFL" => RedFlag,
            b"OVTK" => Overtake { overtaking_idx: 0, overtaken_idx: 0 },
            b"SCAR" => SafetyCar { sc_type: 0, event_type: 0 },
            b"COLL" => Collision { vehicle1_idx: 0, vehicle2_idx: 0 },
            _ => Other { code },
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct EventPacket {
    pub details: EventDetails,
}

/// Size of the `EventDataDetails` union in every supported format.
const EVENT_DETAILS_SIZE: usize = 12;

pub fn event<C: Codec>(c: &mut C, p: &mut EventPacket, _fmt: Format) -> Result<()> {
    use EventDetails::*;
    let mut code = p.details.code();
    c.bytes(&mut code)?;
    if c.is_reader() {
        p.details = EventDetails::from_code(code);
    }
    let start = c.pos();
    match &mut p.details {
        FastestLap { vehicle_idx, lap_time_s } => {
            c.u8(vehicle_idx)?;
            c.f32(lap_time_s)?;
        }
        Retirement { vehicle_idx }
        | TeamMateInPits { vehicle_idx }
        | RaceWinner { vehicle_idx }
        | DriveThroughServed { vehicle_idx }
        | StopGoServed { vehicle_idx } => c.u8(vehicle_idx)?,
        Penalty {
            penalty_type,
            infringement_type,
            vehicle_idx,
            other_vehicle_idx,
            time,
            lap_num,
            places_gained,
        } => {
            c.u8(penalty_type)?;
            c.u8(infringement_type)?;
            c.u8(vehicle_idx)?;
            c.u8(other_vehicle_idx)?;
            c.u8(time)?;
            c.u8(lap_num)?;
            c.u8(places_gained)?;
        }
        SpeedTrap { vehicle_idx, speed_kph } => {
            c.u8(vehicle_idx)?;
            c.f32(speed_kph)?;
        }
        StartLights { num_lights } => c.u8(num_lights)?,
        Flashback { frame_identifier, session_time } => {
            c.u32(frame_identifier)?;
            c.f32(session_time)?;
        }
        Buttons { status } => c.u32(status)?,
        Overtake { overtaking_idx, overtaken_idx } => {
            c.u8(overtaking_idx)?;
            c.u8(overtaken_idx)?;
        }
        SafetyCar { sc_type, event_type } => {
            c.u8(sc_type)?;
            c.u8(event_type)?;
        }
        Collision { vehicle1_idx, vehicle2_idx } => {
            c.u8(vehicle1_idx)?;
            c.u8(vehicle2_idx)?;
        }
        SessionStarted | SessionEnded | DrsEnabled | DrsDisabled | ChequeredFlag | LightsOut
        | RedFlag | Other { .. } => {}
    }
    let used = c.pos() - start;
    c.skip(EVENT_DETAILS_SIZE.saturating_sub(used))
}

// ---------------------------------------------------------- Participants (4)

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Participant {
    pub ai_controlled: bool,
    pub driver_id: u8,
    pub network_id: u8,
    pub team_id: u8,
    pub my_team: bool,
    pub race_number: u8,
    pub nationality: u8,
    pub name: String,
    /// 0 = restricted, 1 = public.
    pub your_telemetry: u8,
    /// F1 23+.
    pub show_online_names: bool,
    /// F1 24+.
    pub tech_level: u16,
    /// F1 23+.
    pub platform: u8,
}

impl Participant {
    pub fn telemetry_restricted(&self) -> bool {
        self.your_telemetry == 0 && !self.ai_controlled
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParticipantsPacket {
    pub num_active_cars: u8,
    pub participants: [Participant; MAX_CARS],
}

fn participant<C: Codec>(c: &mut C, p: &mut Participant, fmt: Format) -> Result<()> {
    c.bool(&mut p.ai_controlled)?;
    c.u8(&mut p.driver_id)?;
    c.u8(&mut p.network_id)?;
    c.u8(&mut p.team_id)?;
    c.bool(&mut p.my_team)?;
    c.u8(&mut p.race_number)?;
    c.u8(&mut p.nationality)?;
    let name_len = if fmt >= Format::F2025 { 32 } else { 48 };
    fixed_str(c, &mut p.name, name_len)?;
    c.u8(&mut p.your_telemetry)?;
    if fmt >= Format::F2023 {
        c.bool(&mut p.show_online_names)?;
    }
    if fmt >= Format::F2024 {
        c.u16(&mut p.tech_level)?;
    }
    if fmt >= Format::F2023 {
        c.u8(&mut p.platform)?;
    }
    if fmt >= Format::F2025 {
        c.skip(1 + 4 * 3)?; // numColours + liveryColours[4] (RGB)
    }
    Ok(())
}

pub fn participants<C: Codec>(c: &mut C, p: &mut ParticipantsPacket, fmt: Format) -> Result<()> {
    c.u8(&mut p.num_active_cars)?;
    cars(c, &mut p.participants, fmt, participant)
}

// ------------------------------------------------------------ Car Setups (5)

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CarSetup {
    pub front_wing: u8,
    pub rear_wing: u8,
    pub on_throttle: u8,
    pub off_throttle: u8,
    pub front_camber: f32,
    pub rear_camber: f32,
    pub front_toe: f32,
    pub rear_toe: f32,
    pub front_suspension: u8,
    pub rear_suspension: u8,
    pub front_anti_roll_bar: u8,
    pub rear_anti_roll_bar: u8,
    pub front_suspension_height: u8,
    pub rear_suspension_height: u8,
    pub brake_pressure: u8,
    pub brake_bias: u8,
    /// F1 24+.
    pub engine_braking: u8,
    pub tyre_pressures: Wheels<f32>,
    pub ballast: u8,
    pub fuel_load: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CarSetupsPacket {
    pub cars: [CarSetup; MAX_CARS],
    /// F1 24+.
    pub next_front_wing_value: f32,
}

fn car_setup<C: Codec>(c: &mut C, s: &mut CarSetup, fmt: Format) -> Result<()> {
    c.u8(&mut s.front_wing)?;
    c.u8(&mut s.rear_wing)?;
    c.u8(&mut s.on_throttle)?;
    c.u8(&mut s.off_throttle)?;
    c.f32(&mut s.front_camber)?;
    c.f32(&mut s.rear_camber)?;
    c.f32(&mut s.front_toe)?;
    c.f32(&mut s.rear_toe)?;
    c.u8(&mut s.front_suspension)?;
    c.u8(&mut s.rear_suspension)?;
    c.u8(&mut s.front_anti_roll_bar)?;
    c.u8(&mut s.rear_anti_roll_bar)?;
    c.u8(&mut s.front_suspension_height)?;
    c.u8(&mut s.rear_suspension_height)?;
    c.u8(&mut s.brake_pressure)?;
    c.u8(&mut s.brake_bias)?;
    if fmt >= Format::F2024 {
        c.u8(&mut s.engine_braking)?;
    }
    wheels_f32(c, &mut s.tyre_pressures)?;
    c.u8(&mut s.ballast)?;
    c.f32(&mut s.fuel_load)
}

pub fn car_setups<C: Codec>(c: &mut C, p: &mut CarSetupsPacket, fmt: Format) -> Result<()> {
    cars(c, &mut p.cars, fmt, car_setup)?;
    if fmt >= Format::F2024 {
        c.f32(&mut p.next_front_wing_value)?;
    }
    Ok(())
}

// --------------------------------------------------------- Car Telemetry (6)

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CarTelemetry {
    pub speed_kph: u16,
    pub throttle: f32,
    pub steer: f32,
    pub brake: f32,
    pub clutch: u8,
    pub gear: i8,
    pub engine_rpm: u16,
    pub drs: bool,
    pub rev_lights_percent: u8,
    pub rev_lights_bits: u16,
    pub brakes_temperature: Wheels<u16>,
    pub tyres_surface_temperature: Wheels<u8>,
    pub tyres_inner_temperature: Wheels<u8>,
    pub engine_temperature: u16,
    pub tyres_pressure: Wheels<f32>,
    pub surface_type: Wheels<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CarTelemetryPacket {
    pub cars: [CarTelemetry; MAX_CARS],
    pub mfd_panel_index: u8,
    pub mfd_panel_index_secondary: u8,
    pub suggested_gear: i8,
}

fn car_telemetry<C: Codec>(c: &mut C, t: &mut CarTelemetry, _fmt: Format) -> Result<()> {
    c.u16(&mut t.speed_kph)?;
    c.f32(&mut t.throttle)?;
    c.f32(&mut t.steer)?;
    c.f32(&mut t.brake)?;
    c.u8(&mut t.clutch)?;
    c.i8(&mut t.gear)?;
    c.u16(&mut t.engine_rpm)?;
    c.bool(&mut t.drs)?;
    c.u8(&mut t.rev_lights_percent)?;
    c.u16(&mut t.rev_lights_bits)?;
    wheels_u16(c, &mut t.brakes_temperature)?;
    wheels_u8(c, &mut t.tyres_surface_temperature)?;
    wheels_u8(c, &mut t.tyres_inner_temperature)?;
    c.u16(&mut t.engine_temperature)?;
    wheels_f32(c, &mut t.tyres_pressure)?;
    wheels_u8(c, &mut t.surface_type)
}

pub fn car_telemetry_packet<C: Codec>(
    c: &mut C,
    p: &mut CarTelemetryPacket,
    fmt: Format,
) -> Result<()> {
    cars(c, &mut p.cars, fmt, car_telemetry)?;
    c.u8(&mut p.mfd_panel_index)?;
    c.u8(&mut p.mfd_panel_index_secondary)?;
    c.i8(&mut p.suggested_gear)
}

// ------------------------------------------------------------ Car Status (7)

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CarStatus {
    pub traction_control: u8,
    pub anti_lock_brakes: bool,
    pub fuel_mix: u8,
    pub front_brake_bias: u8,
    pub pit_limiter_status: bool,
    pub fuel_in_tank: f32,
    pub fuel_capacity: f32,
    /// Fuel remaining in terms of laps (the MFD delta value).
    pub fuel_remaining_laps: f32,
    pub max_rpm: u16,
    pub idle_rpm: u16,
    pub max_gears: u8,
    pub drs_allowed: u8,
    pub drs_activation_distance: u16,
    /// 16 C5 .. 20 C1, 21 C0, 22 C6, 7 inter, 8 wet (F1 modern).
    pub actual_tyre_compound: u8,
    /// 16 soft, 17 medium, 18 hard, 7 inter, 8 wet (F1 modern).
    pub visual_tyre_compound: u8,
    pub tyres_age_laps: u8,
    pub vehicle_fia_flags: i8,
    /// F1 23+ (watts).
    pub engine_power_ice: f32,
    /// F1 23+ (watts).
    pub engine_power_mguk: f32,
    /// Joules (max 4 MJ).
    pub ers_store_energy: f32,
    /// 0 none, 1 medium, 2 hotlap, 3 overtake.
    pub ers_deploy_mode: u8,
    pub ers_harvested_this_lap_mguk: f32,
    pub ers_harvested_this_lap_mguh: f32,
    pub ers_deployed_this_lap: f32,
    pub network_paused: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CarStatusPacket {
    pub cars: [CarStatus; MAX_CARS],
}

fn car_status<C: Codec>(c: &mut C, s: &mut CarStatus, fmt: Format) -> Result<()> {
    c.u8(&mut s.traction_control)?;
    c.bool(&mut s.anti_lock_brakes)?;
    c.u8(&mut s.fuel_mix)?;
    c.u8(&mut s.front_brake_bias)?;
    c.bool(&mut s.pit_limiter_status)?;
    c.f32(&mut s.fuel_in_tank)?;
    c.f32(&mut s.fuel_capacity)?;
    c.f32(&mut s.fuel_remaining_laps)?;
    c.u16(&mut s.max_rpm)?;
    c.u16(&mut s.idle_rpm)?;
    c.u8(&mut s.max_gears)?;
    c.u8(&mut s.drs_allowed)?;
    c.u16(&mut s.drs_activation_distance)?;
    c.u8(&mut s.actual_tyre_compound)?;
    c.u8(&mut s.visual_tyre_compound)?;
    c.u8(&mut s.tyres_age_laps)?;
    c.i8(&mut s.vehicle_fia_flags)?;
    if fmt >= Format::F2023 {
        c.f32(&mut s.engine_power_ice)?;
        c.f32(&mut s.engine_power_mguk)?;
    }
    c.f32(&mut s.ers_store_energy)?;
    c.u8(&mut s.ers_deploy_mode)?;
    c.f32(&mut s.ers_harvested_this_lap_mguk)?;
    c.f32(&mut s.ers_harvested_this_lap_mguh)?;
    c.f32(&mut s.ers_deployed_this_lap)?;
    c.bool(&mut s.network_paused)
}

pub fn car_status_packet<C: Codec>(c: &mut C, p: &mut CarStatusPacket, fmt: Format) -> Result<()> {
    cars(c, &mut p.cars, fmt, car_status)
}

// -------------------------------------------------- Final Classification (8)

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct FinalClassification {
    pub position: u8,
    pub num_laps: u8,
    pub grid_position: u8,
    pub points: u8,
    pub num_pit_stops: u8,
    pub result_status: u8,
    /// F1 25+.
    pub result_reason: u8,
    pub best_lap_time_ms: u32,
    pub total_race_time_s: f64,
    pub penalties_time: u8,
    pub num_penalties: u8,
    pub num_tyre_stints: u8,
    pub tyre_stints_actual: [u8; MAX_TYRE_STINTS],
    pub tyre_stints_visual: [u8; MAX_TYRE_STINTS],
    pub tyre_stints_end_laps: [u8; MAX_TYRE_STINTS],
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FinalClassificationPacket {
    pub num_cars: u8,
    pub cars: [FinalClassification; MAX_CARS],
}

fn final_classification<C: Codec>(
    c: &mut C,
    f: &mut FinalClassification,
    fmt: Format,
) -> Result<()> {
    c.u8(&mut f.position)?;
    c.u8(&mut f.num_laps)?;
    c.u8(&mut f.grid_position)?;
    c.u8(&mut f.points)?;
    c.u8(&mut f.num_pit_stops)?;
    c.u8(&mut f.result_status)?;
    if fmt >= Format::F2025 {
        c.u8(&mut f.result_reason)?;
    }
    c.u32(&mut f.best_lap_time_ms)?;
    c.f64(&mut f.total_race_time_s)?;
    c.u8(&mut f.penalties_time)?;
    c.u8(&mut f.num_penalties)?;
    c.u8(&mut f.num_tyre_stints)?;
    c.bytes(&mut f.tyre_stints_actual)?;
    c.bytes(&mut f.tyre_stints_visual)?;
    c.bytes(&mut f.tyre_stints_end_laps)
}

pub fn final_classification_packet<C: Codec>(
    c: &mut C,
    p: &mut FinalClassificationPacket,
    fmt: Format,
) -> Result<()> {
    c.u8(&mut p.num_cars)?;
    cars(c, &mut p.cars, fmt, final_classification)
}

// ------------------------------------------------------------ Car Damage (10)

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CarDamage {
    /// Percent.
    pub tyres_wear: Wheels<f32>,
    pub tyres_damage: Wheels<u8>,
    pub brakes_damage: Wheels<u8>,
    /// F1 25+.
    pub tyre_blisters: Wheels<u8>,
    pub front_left_wing_damage: u8,
    pub front_right_wing_damage: u8,
    pub rear_wing_damage: u8,
    pub floor_damage: u8,
    pub diffuser_damage: u8,
    pub sidepod_damage: u8,
    pub drs_fault: bool,
    pub ers_fault: bool,
    pub gearbox_damage: u8,
    pub engine_damage: u8,
    pub engine_mguh_wear: u8,
    pub engine_es_wear: u8,
    pub engine_ce_wear: u8,
    pub engine_ice_wear: u8,
    pub engine_mguk_wear: u8,
    pub engine_tc_wear: u8,
    pub engine_blown: bool,
    pub engine_seized: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CarDamagePacket {
    pub cars: [CarDamage; MAX_CARS],
}

fn car_damage<C: Codec>(c: &mut C, d: &mut CarDamage, fmt: Format) -> Result<()> {
    wheels_f32(c, &mut d.tyres_wear)?;
    wheels_u8(c, &mut d.tyres_damage)?;
    wheels_u8(c, &mut d.brakes_damage)?;
    if fmt >= Format::F2025 {
        wheels_u8(c, &mut d.tyre_blisters)?;
    }
    c.u8(&mut d.front_left_wing_damage)?;
    c.u8(&mut d.front_right_wing_damage)?;
    c.u8(&mut d.rear_wing_damage)?;
    c.u8(&mut d.floor_damage)?;
    c.u8(&mut d.diffuser_damage)?;
    c.u8(&mut d.sidepod_damage)?;
    c.bool(&mut d.drs_fault)?;
    c.bool(&mut d.ers_fault)?;
    c.u8(&mut d.gearbox_damage)?;
    c.u8(&mut d.engine_damage)?;
    c.u8(&mut d.engine_mguh_wear)?;
    c.u8(&mut d.engine_es_wear)?;
    c.u8(&mut d.engine_ce_wear)?;
    c.u8(&mut d.engine_ice_wear)?;
    c.u8(&mut d.engine_mguk_wear)?;
    c.u8(&mut d.engine_tc_wear)?;
    c.bool(&mut d.engine_blown)?;
    c.bool(&mut d.engine_seized)
}

pub fn car_damage_packet<C: Codec>(c: &mut C, p: &mut CarDamagePacket, fmt: Format) -> Result<()> {
    cars(c, &mut p.cars, fmt, car_damage)
}

// ------------------------------------------------------- Session History (11)

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct LapHistory {
    pub lap_time_ms: u32,
    pub sector1_time_ms: u32,
    pub sector2_time_ms: u32,
    pub sector3_time_ms: u32,
    /// bit 0 lap valid, bit 1 S1 valid, bit 2 S2 valid, bit 3 S3 valid.
    pub lap_valid_bit_flags: u8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct TyreStintHistory {
    /// 255 = current tyre.
    pub end_lap: u8,
    pub tyre_actual_compound: u8,
    pub tyre_visual_compound: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionHistoryPacket {
    pub car_idx: u8,
    pub best_lap_time_lap_num: u8,
    pub best_sector1_lap_num: u8,
    pub best_sector2_lap_num: u8,
    pub best_sector3_lap_num: u8,
    pub laps: Vec<LapHistory>,
    pub tyre_stints: Vec<TyreStintHistory>,
}

pub fn session_history<C: Codec>(
    c: &mut C,
    p: &mut SessionHistoryPacket,
    fmt: Format,
) -> Result<()> {
    let minutes = fmt >= Format::F2023;
    c.u8(&mut p.car_idx)?;
    let mut num_laps = p.laps.len().min(MAX_LAP_HISTORY) as u8;
    let mut num_stints = p.tyre_stints.len().min(MAX_TYRE_STINTS) as u8;
    c.u8(&mut num_laps)?;
    c.u8(&mut num_stints)?;
    c.u8(&mut p.best_lap_time_lap_num)?;
    c.u8(&mut p.best_sector1_lap_num)?;
    c.u8(&mut p.best_sector2_lap_num)?;
    c.u8(&mut p.best_sector3_lap_num)?;
    let num_laps = (num_laps as usize).min(MAX_LAP_HISTORY);
    let num_stints = (num_stints as usize).min(MAX_TYRE_STINTS);
    if c.is_reader() {
        p.laps = vec![LapHistory::default(); num_laps];
        p.tyre_stints = vec![TyreStintHistory::default(); num_stints];
    }
    let lap_size = if minutes { 14 } else { 11 };
    for lap in p.laps.iter_mut().take(num_laps) {
        c.u32(&mut lap.lap_time_ms)?;
        split_time(c, &mut lap.sector1_time_ms, minutes)?;
        split_time(c, &mut lap.sector2_time_ms, minutes)?;
        split_time(c, &mut lap.sector3_time_ms, minutes)?;
        c.u8(&mut lap.lap_valid_bit_flags)?;
    }
    c.skip((MAX_LAP_HISTORY - num_laps) * lap_size)?;
    for s in p.tyre_stints.iter_mut().take(num_stints) {
        c.u8(&mut s.end_lap)?;
        c.u8(&mut s.tyre_actual_compound)?;
        c.u8(&mut s.tyre_visual_compound)?;
    }
    c.skip((MAX_TYRE_STINTS - num_stints) * 3)
}
