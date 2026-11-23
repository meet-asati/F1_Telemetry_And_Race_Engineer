//! F1 22 / 23 / 24 / 25 UDP telemetry codec.
//!
//! * [`decode`] turns one UDP datagram into a [`ParsedPacket`] whose payload is a
//!   version-neutral struct (fields missing in older formats stay at default /
//!   `None`), with all wheel arrays normalized to `[FL, FR, RL, RR]`.
//! * [`encode`] does the reverse (used by the synthetic generator and tests).
//! * [`capture`] records and replays raw datagrams.

pub mod capture;
pub mod codec;
pub mod lookup;
pub mod packets;

use codec::{Codec, Reader, Writer};
pub use packets::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DecodeError {
    #[error("datagram truncated: needed {needed} bytes, got {got}")]
    Truncated { needed: usize, got: usize },
    #[error("unsupported packet format {0}")]
    UnsupportedFormat(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Format {
    F2022,
    F2023,
    F2024,
    F2025,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::F2022, Format::F2023, Format::F2024, Format::F2025];

    pub fn from_packet_format(v: u16) -> Option<Self> {
        match v {
            2022 => Some(Format::F2022),
            2023 => Some(Format::F2023),
            2024 => Some(Format::F2024),
            2025 => Some(Format::F2025),
            _ => None,
        }
    }
    pub fn packet_format(self) -> u16 {
        match self {
            Format::F2022 => 2022,
            Format::F2023 => 2023,
            Format::F2024 => 2024,
            Format::F2025 => 2025,
        }
    }
    /// Two-digit game year (22..25).
    pub fn game_year(self) -> u8 {
        (self.packet_format() - 2000) as u8
    }
    pub fn header_size(self) -> usize {
        if self == Format::F2022 {
            24
        } else {
            29
        }
    }
}

impl std::str::FromStr for Format {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let n: u16 = s.trim().trim_start_matches("f1").trim_start_matches('-').parse().map_err(|_| format!("bad format '{s}'"))?;
        let n = if n < 100 { n + 2000 } else { n };
        Format::from_packet_format(n).ok_or_else(|| format!("unsupported format {n}"))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub packet_format: u16,
    /// Two-digit game year. Synthesized as 22 for the F1 22 format.
    pub game_year: u8,
    pub game_major_version: u8,
    pub game_minor_version: u8,
    pub packet_version: u8,
    pub packet_id: u8,
    pub session_uid: u64,
    pub session_time: f32,
    pub frame_identifier: u32,
    /// Equals `frame_identifier` for F1 22 (no flashback-independent counter).
    pub overall_frame_identifier: u32,
    pub player_car_index: u8,
    /// 255 if no second player.
    pub secondary_player_car_index: u8,
}

fn header<C: Codec>(c: &mut C, h: &mut Header, fmt: Format) -> codec::Result<()> {
    c.u16(&mut h.packet_format)?;
    if fmt >= Format::F2023 {
        c.u8(&mut h.game_year)?;
    } else if c.is_reader() {
        h.game_year = 22;
    }
    c.u8(&mut h.game_major_version)?;
    c.u8(&mut h.game_minor_version)?;
    c.u8(&mut h.packet_version)?;
    c.u8(&mut h.packet_id)?;
    c.u64(&mut h.session_uid)?;
    c.f32(&mut h.session_time)?;
    c.u32(&mut h.frame_identifier)?;
    if fmt >= Format::F2023 {
        c.u32(&mut h.overall_frame_identifier)?;
    } else if c.is_reader() {
        h.overall_frame_identifier = h.frame_identifier;
    }
    c.u8(&mut h.player_car_index)?;
    c.u8(&mut h.secondary_player_car_index)
}

pub mod packet_id {
    pub const MOTION: u8 = 0;
    pub const SESSION: u8 = 1;
    pub const LAP_DATA: u8 = 2;
    pub const EVENT: u8 = 3;
    pub const PARTICIPANTS: u8 = 4;
    pub const CAR_SETUPS: u8 = 5;
    pub const CAR_TELEMETRY: u8 = 6;
    pub const CAR_STATUS: u8 = 7;
    pub const FINAL_CLASSIFICATION: u8 = 8;
    pub const LOBBY_INFO: u8 = 9;
    pub const CAR_DAMAGE: u8 = 10;
    pub const SESSION_HISTORY: u8 = 11;
    pub const TYRE_SETS: u8 = 12;
    pub const MOTION_EX: u8 = 13;
    pub const TIME_TRIAL: u8 = 14;
    pub const LAP_POSITIONS: u8 = 15;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Packet {
    Motion(Box<MotionPacket>),
    Session(Box<SessionPacket>),
    LapData(Box<LapDataPacket>),
    Event(EventPacket),
    Participants(Box<ParticipantsPacket>),
    CarSetups(Box<CarSetupsPacket>),
    CarTelemetry(Box<CarTelemetryPacket>),
    CarStatus(Box<CarStatusPacket>),
    FinalClassification(Box<FinalClassificationPacket>),
    CarDamage(Box<CarDamagePacket>),
    SessionHistory(Box<SessionHistoryPacket>),
    /// A packet we recognise but do not decode (lobby, tyre sets, motion ex,
    /// time trial, lap positions) or an unknown ID.
    Ignored(u8),
}

impl Packet {
    pub fn id(&self) -> u8 {
        use packet_id::*;
        match self {
            Packet::Motion(_) => MOTION,
            Packet::Session(_) => SESSION,
            Packet::LapData(_) => LAP_DATA,
            Packet::Event(_) => EVENT,
            Packet::Participants(_) => PARTICIPANTS,
            Packet::CarSetups(_) => CAR_SETUPS,
            Packet::CarTelemetry(_) => CAR_TELEMETRY,
            Packet::CarStatus(_) => CAR_STATUS,
            Packet::FinalClassification(_) => FINAL_CLASSIFICATION,
            Packet::CarDamage(_) => CAR_DAMAGE,
            Packet::SessionHistory(_) => SESSION_HISTORY,
            Packet::Ignored(id) => *id,
        }
    }

    /// An all-default packet of the given ID (used for size tests).
    pub fn default_for(id: u8) -> Packet {
        use packet_id::*;
        match id {
            MOTION => Packet::Motion(Default::default()),
            SESSION => Packet::Session(Default::default()),
            LAP_DATA => Packet::LapData(Default::default()),
            EVENT => Packet::Event(Default::default()),
            PARTICIPANTS => Packet::Participants(Default::default()),
            CAR_SETUPS => Packet::CarSetups(Default::default()),
            CAR_TELEMETRY => Packet::CarTelemetry(Default::default()),
            CAR_STATUS => Packet::CarStatus(Default::default()),
            FINAL_CLASSIFICATION => Packet::FinalClassification(Default::default()),
            CAR_DAMAGE => Packet::CarDamage(Default::default()),
            SESSION_HISTORY => Packet::SessionHistory(Default::default()),
            other => Packet::Ignored(other),
        }
    }
}

fn body<C: Codec>(c: &mut C, p: &mut Packet, fmt: Format) -> codec::Result<()> {
    match p {
        Packet::Motion(x) => packets::motion(c, x, fmt),
        Packet::Session(x) => packets::session(c, x, fmt),
        Packet::LapData(x) => packets::lap_data_packet(c, x, fmt),
        Packet::Event(x) => packets::event(c, x, fmt),
        Packet::Participants(x) => packets::participants(c, x, fmt),
        Packet::CarSetups(x) => packets::car_setups(c, x, fmt),
        Packet::CarTelemetry(x) => packets::car_telemetry_packet(c, x, fmt),
        Packet::CarStatus(x) => packets::car_status_packet(c, x, fmt),
        Packet::FinalClassification(x) => packets::final_classification_packet(c, x, fmt),
        Packet::CarDamage(x) => packets::car_damage_packet(c, x, fmt),
        Packet::SessionHistory(x) => packets::session_history(c, x, fmt),
        Packet::Ignored(_) => Ok(()),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedPacket {
    pub format: Format,
    pub header: Header,
    pub packet: Packet,
}

/// Decode one UDP datagram.
pub fn decode(buf: &[u8]) -> Result<ParsedPacket, DecodeError> {
    if buf.len() < 2 {
        return Err(DecodeError::Truncated { needed: 2, got: buf.len() });
    }
    let raw_format = u16::from_le_bytes([buf[0], buf[1]]);
    let format = Format::from_packet_format(raw_format).ok_or(DecodeError::UnsupportedFormat(raw_format))?;
    let mut r = Reader::new(buf);
    let mut h = Header::default();
    header(&mut r, &mut h, format)?;
    let mut packet = Packet::default_for(h.packet_id);
    body(&mut r, &mut packet, format)?;
    Ok(ParsedPacket { format, header: h, packet })
}

/// Encode a packet in the given format. `header.packet_format`,
/// `header.packet_id` and (for F1 23+) `header.game_year` are filled in.
pub fn encode(format: Format, header_in: &Header, packet: &Packet) -> Vec<u8> {
    let mut h = *header_in;
    h.packet_format = format.packet_format();
    h.packet_id = packet.id();
    if h.game_year == 0 {
        h.game_year = format.game_year();
    }
    let mut w = Writer::default();
    let mut p = packet.clone();
    // Writer never fails.
    header(&mut w, &mut h, format).expect("writer is infallible");
    body(&mut w, &mut p, format).expect("writer is infallible");
    w.buf
}

/// Expected on-wire size (bytes, including header) for the packets we decode,
/// taken from the official EA UDP specifications.
pub fn expected_size(format: Format, id: u8) -> Option<usize> {
    use packet_id::*;
    let table: [usize; 12] = match format {
        //              0     1    2    3    4     5     6     7     8   9    10    11
        Format::F2022 => [1464, 632, 972, 40, 1257, 1102, 1347, 1058, 1015, 0, 948, 1155],
        Format::F2023 => [1349, 644, 1131, 45, 1306, 1107, 1352, 1239, 1020, 0, 953, 1460],
        Format::F2024 => [1349, 753, 1285, 45, 1350, 1133, 1352, 1239, 1020, 0, 953, 1460],
        Format::F2025 => [1349, 753, 1285, 45, 1284, 1133, 1352, 1239, 1042, 0, 1041, 1460],
    };
    match id {
        MOTION..=SESSION_HISTORY if id != LOBBY_INFO => Some(table[id as usize]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECODED_IDS: [u8; 11] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11];

    #[test]
    fn packet_sizes_match_spec_for_every_format() {
        for fmt in Format::ALL {
            for id in DECODED_IDS {
                let bytes = encode(fmt, &Header::default(), &Packet::default_for(id));
                assert_eq!(
                    Some(bytes.len()),
                    expected_size(fmt, id),
                    "format {fmt:?} packet {id} size mismatch"
                );
            }
        }
    }

    #[test]
    fn header_sizes() {
        for fmt in Format::ALL {
            let bytes = encode(fmt, &Header::default(), &Packet::Ignored(9));
            assert_eq!(bytes.len(), fmt.header_size());
        }
    }

    fn sample_packets() -> Vec<Packet> {
        let mut lap = LapDataPacket::default();
        lap.cars[3] = LapData {
            last_lap_time_ms: 91_234,
            current_lap_time_ms: 45_000,
            sector1_time_ms: 28_500,
            sector2_time_ms: 31_250,
            delta_to_car_in_front_ms: Some(1_234),
            delta_to_race_leader_ms: Some(75_432),
            lap_distance: 1234.5,
            total_distance: 25_000.0,
            car_position: 4,
            current_lap_num: 6,
            pit_status: 1,
            current_lap_invalid: true,
            penalties: 5,
            driver_status: 4,
            result_status: 2,
            ..Default::default()
        };
        let mut dmg = CarDamagePacket::default();
        dmg.cars[0].tyres_wear = [10.0, 20.0, 30.0, 40.0];
        dmg.cars[0].front_left_wing_damage = 12;
        dmg.cars[0].engine_blown = true;
        let mut tel = CarTelemetryPacket::default();
        tel.cars[0].speed_kph = 312;
        tel.cars[0].throttle = 1.0;
        tel.cars[0].gear = 8;
        tel.cars[0].brakes_temperature = [500, 510, 400, 410];
        tel.cars[0].tyres_surface_temperature = [90, 91, 92, 93];
        tel.suggested_gear = -1;
        let mut sess = SessionPacket {
            weather: 3,
            track_temperature: 31,
            air_temperature: -2,
            total_laps: 57,
            track_length: 5412,
            session_type: 10,
            track_id: 3,
            safety_car_status: 2,
            pit_stop_window_ideal_lap: 18,
            pit_stop_window_latest_lap: 22,
            pit_stop_rejoin_position: 7,
            ..Default::default()
        };
        sess.marshal_zones = vec![MarshalZone { zone_start: 0.1, zone_flag: 3 }; 5];
        sess.weather_forecast = vec![
            ForecastSample { time_offset_min: 5, weather: 3, rain_percentage: 60, ..Default::default() };
            10
        ];
        let mut parts = ParticipantsPacket { num_active_cars: 20, ..Default::default() };
        parts.participants[1].name = "VERSTAPPEN".into();
        parts.participants[1].race_number = 1;
        parts.participants[1].your_telemetry = 1;
        let hist = SessionHistoryPacket {
            car_idx: 5,
            best_lap_time_lap_num: 2,
            laps: vec![
                LapHistory {
                    lap_time_ms: 90_000,
                    sector1_time_ms: 30_000,
                    sector2_time_ms: 30_000,
                    sector3_time_ms: 30_000,
                    lap_valid_bit_flags: 0xF,
                };
                3
            ],
            tyre_stints: vec![TyreStintHistory { end_lap: 255, tyre_actual_compound: 18, tyre_visual_compound: 17 }],
            ..Default::default()
        };
        vec![
            Packet::LapData(Box::new(lap)),
            Packet::CarDamage(Box::new(dmg)),
            Packet::CarTelemetry(Box::new(tel)),
            Packet::Session(Box::new(sess)),
            Packet::Participants(Box::new(parts)),
            Packet::SessionHistory(Box::new(hist)),
            Packet::Event(EventPacket { details: EventDetails::Buttons { status: udp_action_bit(1) } }),
            Packet::Event(EventPacket {
                details: EventDetails::Penalty {
                    penalty_type: 2,
                    infringement_type: 7,
                    vehicle_idx: 3,
                    other_vehicle_idx: 255,
                    time: 5,
                    lap_num: 9,
                    places_gained: 0,
                },
            }),
            Packet::Event(EventPacket { details: EventDetails::Flashback { frame_identifier: 1234, session_time: 99.5 } }),
        ]
    }

    #[test]
    fn round_trip_all_formats() {
        let h = Header { session_uid: 0xDEAD_BEEF, session_time: 12.5, frame_identifier: 77, player_car_index: 3, ..Default::default() };
        for fmt in Format::ALL {
            for p in sample_packets() {
                let bytes = encode(fmt, &h, &p);
                let parsed = decode(&bytes).unwrap();
                assert_eq!(parsed.format, fmt);
                assert_eq!(parsed.header.session_uid, 0xDEAD_BEEF);
                assert_eq!(parsed.header.player_car_index, 3);
                let mut expected = p.clone();
                // Fields that do not exist in older formats decode as default.
                if let (Packet::LapData(e), true) = (&mut expected, fmt == Format::F2022) {
                    for car in e.cars.iter_mut() {
                        car.delta_to_car_in_front_ms = None;
                        car.delta_to_race_leader_ms = None;
                    }
                }
                if let (Packet::LapData(e), Format::F2023) = (&mut expected, fmt) {
                    for car in e.cars.iter_mut() {
                        car.delta_to_car_in_front_ms = Some(0);
                        car.delta_to_race_leader_ms = Some(0);
                    }
                    // F1 23 deltas are a bare u16 of ms (saturating).
                    e.cars[3].delta_to_car_in_front_ms = Some(1_234);
                    e.cars[3].delta_to_race_leader_ms = Some(65_535);
                }
                if let (Packet::LapData(e), Format::F2024 | Format::F2025) = (&mut expected, fmt) {
                    for (i, car) in e.cars.iter_mut().enumerate() {
                        if i != 3 {
                            car.delta_to_car_in_front_ms = Some(0);
                            car.delta_to_race_leader_ms = Some(0);
                        }
                    }
                }
                assert_eq!(parsed.packet, expected, "round trip failed for {fmt:?} packet {}", p.id());
            }
        }
    }

    #[test]
    fn wheel_order_is_normalized() {
        // Hand-build an F1 22 car damage packet: wear for car 0 in game order RL, RR, FL, FR.
        let mut bytes = encode(Format::F2022, &Header::default(), &Packet::default_for(packet_id::CAR_DAMAGE));
        let off = 24;
        for (i, v) in [1.0f32, 2.0, 3.0, 4.0].iter().enumerate() {
            bytes[off + i * 4..off + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        let parsed = decode(&bytes).unwrap();
        let Packet::CarDamage(d) = parsed.packet else { panic!("wrong packet") };
        // Model order FL, FR, RL, RR.
        assert_eq!(d.cars[0].tyres_wear, [3.0, 4.0, 1.0, 2.0]);
        assert_eq!(d.cars[0].tyres_wear[wheel::RL], 1.0);
    }

    #[test]
    fn truncated_and_unknown_are_errors_not_panics() {
        let bytes = encode(Format::F2024, &Header::default(), &Packet::default_for(packet_id::LAP_DATA));
        assert!(matches!(decode(&bytes[..100]), Err(DecodeError::Truncated { .. })));
        let mut bad = bytes.clone();
        bad[0..2].copy_from_slice(&2019u16.to_le_bytes());
        assert_eq!(decode(&bad), Err(DecodeError::UnsupportedFormat(2019)));
        assert!(decode(&[]).is_err());
    }

    #[test]
    fn ignored_packets_decode() {
        for id in [9u8, 12, 13, 14, 15, 42] {
            let mut bytes = encode(Format::F2025, &Header::default(), &Packet::Ignored(id));
            bytes.extend_from_slice(&[0u8; 64]);
            assert_eq!(decode(&bytes).unwrap().packet, Packet::Ignored(id));
        }
    }

    #[test]
    fn udp_action_bits() {
        assert_eq!(udp_action_bit(1), 0x0010_0000);
        assert_eq!(udp_action_bit(12), 0x8000_0000);
    }
}
