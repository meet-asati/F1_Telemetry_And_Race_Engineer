//! Wire types for the pit-wall WebSocket/REST contract (`UI.md` §5).
//! Serialized as camelCase JSON. Times are integer ms, temps °C, percentages
//! 0–100, wheel arrays `[FL, FR, RL, RR]`, `null` = unavailable/restricted.

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

pub type Wheels<T> = [T; 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AlertLevel {
    #[default]
    Nominal = 0,
    Watch = 1,
    Critical = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DataSource {
    Live,
    Replay,
    Synthetic,
    #[default]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySnapshot {
    pub schema_version: u32,
    pub timestamp_ms: u64,
    pub source: DataSource,
    /// Milliseconds since the last UDP/replay packet (`None` = never).
    pub last_packet_age_ms: Option<u64>,
    pub game_year: Option<u8>,
    pub session_uid: Option<String>,
    pub session: SessionInfo,
    pub timing: Timing,
    pub track: TrackInfo,
    pub tyres: Tyres,
    pub powertrain: Powertrain,
    pub damage: Damage,
    pub comms: CommsState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ForecastSampleView {
    pub time_offset_min: u8,
    pub weather: String,
    pub rain_pct: u8,
    pub air_temp_c: i8,
    pub track_temp_c: i8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MarshalZoneView {
    pub start_fraction: f32,
    pub flag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PitWindow {
    pub ideal_lap: u8,
    pub latest_lap: u8,
    pub rejoin_position: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub track_id: i8,
    pub track_name: String,
    pub track_length_m: u16,
    pub session_type: String,
    pub is_race: bool,
    pub lap: u8,
    pub total_laps: u8,
    pub time_left_ms: u64,
    pub weather: String,
    pub air_temp_c: i8,
    pub track_temp_c: i8,
    pub forecast_accuracy: String,
    pub weather_forecast: Vec<ForecastSampleView>,
    pub marshal_zones: Vec<MarshalZoneView>,
    pub safety_car_status: String,
    pub pit_window: Option<PitWindow>,
    pub paused: bool,
    /// "grid" | "start" (first 20 s after lights out) | "racing" | "finished".
    pub phase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StintView {
    pub compound: String,
    pub start_lap: u8,
    pub end_lap: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TimingEntry {
    pub car_index: u8,
    pub position: u8,
    pub driver_label: String,
    pub driver_name: String,
    pub race_number: u8,
    pub team_id: u8,
    pub is_player: bool,
    pub lap: u8,
    pub gap_to_leader_ms: Option<u32>,
    pub gap_to_ahead_ms: Option<u32>,
    pub last_lap_ms: Option<u32>,
    pub best_lap_ms: Option<u32>,
    /// Sectors of the last completed lap.
    pub sectors_ms: [Option<u32>; 3],
    pub compound: Option<String>,
    pub tyre_age_laps: Option<u8>,
    pub pit_stops: u8,
    pub pit_status: String,
    pub penalties_ms: u32,
    pub warnings: u8,
    pub driver_status: String,
    pub result_status: String,
    pub telemetry_restricted: bool,
    pub stints: Vec<StintView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Timing {
    pub player_index: u8,
    pub position: u8,
    pub gap_ahead_ms: Option<u32>,
    pub gap_behind_ms: Option<u32>,
    pub last_lap_ms: Option<u32>,
    pub best_lap_ms: Option<u32>,
    pub entries: Vec<TimingEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackCar {
    pub car_index: u8,
    pub x: f32,
    pub z: f32,
    pub lap_distance_pct: f32,
    pub speed_kph: u16,
    pub gear: i8,
    pub throttle_pct: u8,
    pub brake_pct: u8,
    pub steer: f32,
    pub ers_deploy_mode: Option<String>,
    pub is_player: bool,
    pub drs_active: bool,
    pub in_pit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    pub outline_available: bool,
    pub cars: Vec<TrackCar>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Tyres {
    pub compound: String,
    pub actual_compound: String,
    pub age_laps: u8,
    pub wear_pct: Wheels<f32>,
    pub wear_rate_per_lap: Wheels<Option<f32>>,
    pub damage_pct: Wheels<u8>,
    /// F1 25+ only.
    pub blisters_pct: Option<Wheels<u8>>,
    pub surface_temp_c: Wheels<u8>,
    pub inner_temp_c: Wheels<u8>,
    pub brake_temp_c: Wheels<u16>,
    pub pressure_psi: Wheels<f32>,
    pub projected_pit_lap: Option<u8>,
    pub alert_level: AlertLevel,
    pub alert_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ComponentWear {
    pub ice: u8,
    pub mgu_h: u8,
    pub mgu_k: u8,
    pub es: u8,
    pub ce: u8,
    pub tc: u8,
    pub gearbox: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Powertrain {
    pub fuel_remaining_laps_delta: f32,
    pub fuel_remaining_kg: f32,
    pub fuel_burn_per_lap_kg: Option<f32>,
    pub fuel_required_per_lap_kg: Option<f32>,
    pub ers_store_pct: f32,
    pub ers_deploy_mode: String,
    #[serde(rename = "ersHarvestedThisLapKJ")]
    pub ers_harvested_this_lap_kj: f32,
    #[serde(rename = "ersDeployedThisLapKJ")]
    pub ers_deployed_this_lap_kj: f32,
    pub wear_pct: ComponentWear,
    pub engine_blown: bool,
    pub engine_seized: bool,
    pub ers_fault: bool,
    pub alert_level: AlertLevel,
    pub alert_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SetupView {
    pub front_wing: u8,
    pub rear_wing: u8,
    pub on_throttle: u8,
    pub off_throttle: u8,
    pub brake_bias: u8,
    pub brake_pressure: u8,
    pub front_suspension: u8,
    pub rear_suspension: u8,
    pub front_anti_roll_bar: u8,
    pub rear_anti_roll_bar: u8,
    pub front_ride_height: u8,
    pub rear_ride_height: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Damage {
    pub front_left_wing_pct: u8,
    pub front_right_wing_pct: u8,
    pub rear_wing_pct: u8,
    pub floor_pct: u8,
    pub diffuser_pct: u8,
    pub sidepod_pct: u8,
    pub brake_damage_pct: Wheels<u8>,
    pub drs_fault: bool,
    pub setup: Option<SetupView>,
    pub alert_level: AlertLevel,
    pub alert_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CommsMode {
    #[default]
    Auto,
    Copilot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Priority {
    P0,
    P1,
    P2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub id: String,
    pub priority: Priority,
    pub category: String,
    pub message: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub context_attachments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CommsState {
    pub mode: CommsMode,
    pub muted: bool,
    pub friend_override_active: bool,
    pub override_remaining_ms: u64,
    pub ptt_active: bool,
    pub tts_available: bool,
    /// Voice input: speech-to-text sidecar reachable.
    pub stt_available: bool,
    /// Voice input: LLM reachable (free-form questions).
    pub llm_available: bool,
    /// A microphone is open for push-to-talk.
    pub mic_available: bool,
    /// The engineer is working on an answer (transcribing / LLM).
    pub thinking: bool,
    pub speaking: bool,
    pub last_spoken_line: Option<String>,
    /// The driver's last transcribed radio message.
    pub last_driver_line: Option<String>,
    pub pending_suggestions: Vec<Suggestion>,
}

// ------------------------------------------------------------------ Events

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RadioSource {
    Rule,
    Llm,
    Friend,
    Driver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RadioOutcome {
    Spoken,
    Queued,
    Dismissed,
    Expired,
    TextOnly,
    Suppressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlagStatus {
    Sc,
    Vsc,
    ScEnding,
    VscEnding,
    Red,
    Green,
    Chequered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SuggestionOutcome {
    Approved,
    Edited,
    Dismissed,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum PitWallEvent {
    #[serde(rename = "radio", rename_all = "camelCase")]
    Radio {
        at_ms: u64,
        source: RadioSource,
        priority: Priority,
        text: String,
        outcome: RadioOutcome,
        #[serde(default)]
        attachments: Vec<String>,
    },
    #[serde(rename = "flag", rename_all = "camelCase")]
    Flag { at_ms: u64, status: FlagStatus },
    #[serde(rename = "penalty", rename_all = "camelCase")]
    Penalty { at_ms: u64, car_index: u8, driver_label: String, description: String, time_sec: Option<u8>, lap: u8 },
    #[serde(rename = "suggestion.added", rename_all = "camelCase")]
    SuggestionAdded { suggestion_id: String },
    #[serde(rename = "suggestion.resolved", rename_all = "camelCase")]
    SuggestionResolved { suggestion_id: String, outcome: SuggestionOutcome },
    #[serde(rename = "session.reset", rename_all = "camelCase")]
    SessionReset { session_uid: String },
    #[serde(rename = "flashback", rename_all = "camelCase")]
    Flashback { at_ms: u64, to_lap: u8 },
    #[serde(rename = "lap.completed", rename_all = "camelCase")]
    LapCompleted { car_index: u8, lap: u8, lap_ms: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "hello", rename_all = "camelCase")]
    Hello { schema_version: u32, server_version: String, auth_required: bool },
    #[serde(rename = "snapshot")]
    Snapshot { seq: u64, data: Box<TelemetrySnapshot> },
    #[serde(rename = "event")]
    Event { event: PitWallEvent },
    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientCommand {
    #[serde(rename = "radio.send")]
    RadioSend {
        text: String,
        #[serde(default)]
        attachments: Vec<String>,
        token: Option<String>,
    },
    #[serde(rename = "suggestion.approve")]
    SuggestionApprove { id: String, token: Option<String> },
    #[serde(rename = "suggestion.edit")]
    SuggestionEdit { id: String, text: String, token: Option<String> },
    #[serde(rename = "suggestion.dismiss")]
    SuggestionDismiss { id: String, token: Option<String> },
    #[serde(rename = "comms.setMode")]
    CommsSetMode { mode: CommsMode, token: Option<String> },
    #[serde(rename = "comms.mute")]
    CommsMute { muted: bool, token: Option<String> },
    /// Push-to-talk from the UI (hold a button on the driver page / pit wall).
    #[serde(rename = "ptt.set")]
    PttSet { active: bool, token: Option<String> },
    /// A typed question, answered as if the driver had asked it on the radio.
    #[serde(rename = "driver.ask")]
    DriverAsk { text: String, token: Option<String> },
    #[serde(rename = "select.car", rename_all = "camelCase")]
    SelectCar { car_index: u8 },
}

impl ClientCommand {
    pub fn token(&self) -> Option<&str> {
        match self {
            ClientCommand::RadioSend { token, .. }
            | ClientCommand::SuggestionApprove { token, .. }
            | ClientCommand::SuggestionEdit { token, .. }
            | ClientCommand::SuggestionDismiss { token, .. }
            | ClientCommand::CommsSetMode { token, .. }
            | ClientCommand::CommsMute { token, .. }
            | ClientCommand::PttSet { token, .. }
            | ClientCommand::DriverAsk { token, .. } => token.as_deref(),
            ClientCommand::SelectCar { .. } => None,
        }
    }
    pub fn is_mutating(&self) -> bool {
        !matches!(self, ClientCommand::SelectCar { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shape_matches_contract() {
        let snap = TelemetrySnapshot::default();
        let v = serde_json::to_value(&snap).unwrap();
        assert!(v.get("schemaVersion").is_some());
        assert!(v["powertrain"].get("ersHarvestedThisLapKJ").is_some());
        assert!(v["powertrain"]["wearPct"].get("mguH").is_some());
        assert_eq!(v["tyres"]["alertLevel"], "nominal");

        let ev = PitWallEvent::SuggestionResolved { suggestion_id: "a".into(), outcome: SuggestionOutcome::Dismissed };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["kind"], "suggestion.resolved");
        assert_eq!(v["suggestionId"], "a");

        let cmd: ClientCommand = serde_json::from_str(r#"{"type":"suggestion.edit","id":"x","text":"Box box"}"#).unwrap();
        assert!(matches!(cmd, ClientCommand::SuggestionEdit { .. }));
        let cmd: ClientCommand = serde_json::from_str(r#"{"type":"select.car","carIndex":4}"#).unwrap();
        assert!(!cmd.is_mutating());
        let cmd: ClientCommand = serde_json::from_str(r#"{"type":"comms.setMode","mode":"copilot","token":"t"}"#).unwrap();
        assert_eq!(cmd.token(), Some("t"));
        let cmd: ClientCommand = serde_json::from_str(r#"{"type":"ptt.set","active":true}"#).unwrap();
        assert!(matches!(cmd, ClientCommand::PttSet { active: true, .. }) && cmd.is_mutating());
        let cmd: ClientCommand = serde_json::from_str(r#"{"type":"driver.ask","text":"gap?"}"#).unwrap();
        assert!(matches!(cmd, ClientCommand::DriverAsk { .. }));
    }
}
