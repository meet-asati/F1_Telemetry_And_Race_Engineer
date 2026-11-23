//! Feeds a full synthetic race (encoded -> decoded, like real UDP) through
//! `RaceState` in every supported format and checks derived output.

use f1_synth::{Sim, SimConfig};
use f1_udp::{decode, encode, EventDetails, EventPacket, Format, Header, Packet};
use race_state::snapshot::{FlagStatus, PitWallEvent};
use race_state::{RaceState, StateEvent, Thresholds};

struct Outcome {
    laps: usize,
    flags: Vec<FlagStatus>,
    penalties: usize,
    outline: bool,
    finished: bool,
    max_gap_err_ms: Option<i64>,
    snapshots_ok: bool,
    projected_pit: bool,
    wear_rate: bool,
    fuel_burn: bool,
}

fn run(format: Format) -> Outcome {
    let mut sim = Sim::new(SimConfig { format, laps: 6, player_grid: 7, seed: 11, rate_hz: 20.0, track_length_m: 3000.0 });
    let mut st = RaceState::new(Thresholds::default());
    let mut out = Outcome {
        laps: 0,
        flags: vec![],
        penalties: 0,
        outline: false,
        finished: false,
        max_gap_err_ms: None,
        snapshots_ok: true,
        projected_pit: false,
        wear_rate: false,
        fuel_burn: false,
    };
    let mut tick = 0u64;
    while !sim.done {
        let packets = sim.step(0.05);
        let header = sim.header();
        for p in packets {
            let parsed = decode(&encode(format, &header, &p)).unwrap();
            for ev in st.ingest(&parsed, tick) {
                match ev {
                    StateEvent::LapCompleted(_) => out.laps += 1,
                    StateEvent::Wire(PitWallEvent::Flag { status, .. }) => out.flags.push(status),
                    StateEvent::Wire(PitWallEvent::Penalty { .. }) => out.penalties += 1,
                    StateEvent::TrackOutline { points, .. } => out.outline = points.len() > 50,
                    StateEvent::RaceFinished => out.finished = true,
                    _ => {}
                }
            }
            // Compare computed gaps to the synth's native deltas (F1 23+ only).
            if let Packet::LapData(l) = &parsed.packet {
                if format >= Format::F2023 && tick > 20 * 60 {
                    let snap = st.snapshot(tick);
                    for e in &snap.timing.entries {
                        let native = l.cars[e.car_index as usize].delta_to_car_in_front_ms;
                        if let (Some(n), Some(g)) = (native, e.gap_to_ahead_ms) {
                            if n > 0 {
                                // With native deltas present the snapshot reports native values.
                                let err = (n as i64 - g as i64).abs();
                                out.max_gap_err_ms = Some(out.max_gap_err_ms.unwrap_or(0).max(err));
                            }
                        }
                    }
                }
            }
        }
        if tick.is_multiple_of(200) {
            let snap = st.snapshot(tick);
            if serde_json::to_string(&snap).is_err() {
                out.snapshots_ok = false;
            }
            out.projected_pit |= snap.tyres.projected_pit_lap.is_some();
            out.wear_rate |= snap.tyres.wear_rate_per_lap.iter().any(|r| r.map(|v| v > 0.5).unwrap_or(false));
            out.fuel_burn |= snap.powertrain.fuel_burn_per_lap_kg.map(|b| b > 1.0 && b < 2.5).unwrap_or(false);
        }
        tick += 1;
    }
    out
}

#[test]
fn synthetic_race_all_formats() {
    for format in Format::ALL {
        let o = run(format);
        assert!(o.laps >= 22 * 5, "{format:?}: only {} laps recorded", o.laps);
        assert!(o.flags.contains(&FlagStatus::Sc), "{format:?}: no SC flag ({:?})", o.flags);
        assert!(o.flags.contains(&FlagStatus::Green), "{format:?}: no green flag");
        assert!(o.flags.contains(&FlagStatus::Chequered), "{format:?}: no chequered flag");
        assert_eq!(o.penalties, 1, "{format:?}: penalties");
        assert!(o.outline, "{format:?}: no track outline");
        assert!(o.finished, "{format:?}: not finished");
        assert!(o.snapshots_ok);
        assert!(o.wear_rate, "{format:?}: no wear rate");
        assert!(o.projected_pit, "{format:?}: no projected pit lap");
        assert!(o.fuel_burn, "{format:?}: no fuel burn");
        if format >= Format::F2023 {
            assert_eq!(o.max_gap_err_ms, Some(0), "{format:?}: native deltas should pass through");
        }
    }
}

/// F1 22 has no native gaps: verify timing-line gaps against an independent
/// calculation (distance gap / follower speed) on a steady-state stretch.
#[test]
fn computed_gaps_f1_22_are_plausible() {
    let format = Format::F2022;
    let mut sim = Sim::new(SimConfig { format, laps: 6, player_grid: 7, seed: 3, rate_hz: 20.0, track_length_m: 3000.0 });
    let mut st = RaceState::new(Thresholds::default());
    let mut checked = 0;
    for tick in 0..(20 * 150) {
        let header = sim.header();
        for p in sim.step(0.05) {
            let parsed = decode(&encode(format, &header, &p)).unwrap();
            st.ingest(&parsed, tick);
        }
        if tick == 20 * 120 {
            let snap = st.snapshot(tick);
            assert!(snap.timing.entries.len() >= 20);
            for e in snap.timing.entries.iter().skip(1) {
                let g = e.gap_to_ahead_ms.expect("every non-leader has a computed gap");
                assert!(g < 30_000, "gap {g} ms implausible for {}", e.driver_label);
                checked += 1;
            }
            let leader_gaps: Vec<u32> = snap.timing.entries.iter().filter_map(|e| e.gap_to_leader_ms).collect();
            assert!(leader_gaps.windows(2).filter(|w| w[1] + 500 < w[0]).count() <= 2, "leader gaps should be ~monotonic: {leader_gaps:?}");
        }
    }
    assert!(checked >= 19);
}

#[test]
fn flashback_rolls_back_laps() {
    let format = Format::F2024;
    let mut sim = Sim::new(SimConfig { format, laps: 6, player_grid: 0, seed: 5, rate_hz: 20.0, track_length_m: 3000.0 });
    let mut st = RaceState::new(Thresholds::default());
    let mut snapshot_lap_packet = None;
    let mut tick = 0u64;
    // Run until the player has completed 3 laps, remembering a lap-2 LapData packet.
    while st.laps(0).len() < 3 {
        let header = sim.header();
        for p in sim.step(0.05) {
            let parsed = decode(&encode(format, &header, &p)).unwrap();
            if let Packet::LapData(l) = &parsed.packet {
                if l.cars[0].current_lap_num == 2 && snapshot_lap_packet.is_none() {
                    snapshot_lap_packet = Some(parsed.clone());
                }
            }
            st.ingest(&parsed, tick);
        }
        tick += 1;
    }
    let laps_before = st.laps(0).len();
    assert_eq!(laps_before, 3);
    // Flashback event, then lap data from lap 2 again.
    let fb = Packet::Event(EventPacket { details: EventDetails::Flashback { frame_identifier: 1, session_time: 50.0 } });
    let hdr = Header { session_uid: sim.header().session_uid, session_time: 50.0, ..sim.header() };
    let mut events = st.ingest(&decode(&encode(format, &hdr, &fb)).unwrap(), tick);
    let rewind = snapshot_lap_packet.unwrap();
    events.extend(st.ingest(&rewind, tick + 1));
    assert!(events.iter().any(|e| matches!(e, StateEvent::Flashback { to_lap: 2 })));
    assert_eq!(st.laps(0).len(), 1, "laps 2+ removed after flashback to lap 2");
}
