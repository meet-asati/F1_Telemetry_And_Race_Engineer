//! Human-readable names for enum-like IDs in the UDP spec.

use crate::Format;

pub fn track_name(id: i8) -> &'static str {
    match id {
        0 => "Melbourne",
        1 => "Paul Ricard",
        2 => "Shanghai",
        3 => "Sakhir (Bahrain)",
        4 => "Catalunya",
        5 => "Monaco",
        6 => "Montreal",
        7 => "Silverstone",
        8 => "Hockenheim",
        9 => "Hungaroring",
        10 => "Spa",
        11 => "Monza",
        12 => "Singapore",
        13 => "Suzuka",
        14 => "Abu Dhabi",
        15 => "Texas (COTA)",
        16 => "Brazil",
        17 => "Austria",
        18 => "Sochi",
        19 => "Mexico",
        20 => "Baku",
        21 => "Sakhir Short",
        22 => "Silverstone Short",
        23 => "Texas Short",
        24 => "Suzuka Short",
        25 => "Hanoi",
        26 => "Zandvoort",
        27 => "Imola",
        28 => "Portimão",
        29 => "Jeddah",
        30 => "Miami",
        31 => "Las Vegas",
        32 => "Losail",
        39 => "Silverstone (Reverse)",
        40 => "Austria (Reverse)",
        41 => "Zandvoort (Reverse)",
        _ => "Unknown Track",
    }
}

/// Session type label. The numbering changed in the 2024 format when sprint
/// shootout sessions were inserted before the race sessions.
pub fn session_type_name(format: Format, t: u8) -> &'static str {
    if format >= Format::F2024 {
        match t {
            1 => "P1",
            2 => "P2",
            3 => "P3",
            4 => "Short Practice",
            5 => "Q1",
            6 => "Q2",
            7 => "Q3",
            8 => "Short Quali",
            9 => "One-Shot Quali",
            10 => "SQ1",
            11 => "SQ2",
            12 => "SQ3",
            13 => "Short Sprint Quali",
            14 => "One-Shot Sprint Quali",
            15 => "Race",
            16 => "Race 2",
            17 => "Race 3",
            18 => "Time Trial",
            _ => "Unknown",
        }
    } else {
        match t {
            1 => "P1",
            2 => "P2",
            3 => "P3",
            4 => "Short Practice",
            5 => "Q1",
            6 => "Q2",
            7 => "Q3",
            8 => "Short Quali",
            9 => "One-Shot Quali",
            10 => "Race",
            11 => "Race 2",
            12 => "Race 3",
            13 => "Time Trial",
            _ => "Unknown",
        }
    }
}

pub fn is_race_session(format: Format, t: u8) -> bool {
    if format >= Format::F2024 {
        (15..=17).contains(&t)
    } else {
        (10..=12).contains(&t)
    }
}

pub fn weather_name(w: u8) -> &'static str {
    match w {
        0 => "clear",
        1 => "lightCloud",
        2 => "overcast",
        3 => "lightRain",
        4 => "heavyRain",
        5 => "storm",
        _ => "unknown",
    }
}

/// Visual compound letter: S / M / H / I / W.
pub fn visual_compound(v: u8) -> &'static str {
    match v {
        16 | 19 | 20 => "S",
        17 | 21 => "M",
        18 | 22 => "H",
        7 => "I",
        8 | 10 | 15 => "W",
        9 => "D",
        _ => "?",
    }
}

pub fn actual_compound(v: u8) -> &'static str {
    match v {
        22 => "C6",
        16 => "C5",
        17 => "C4",
        18 => "C3",
        19 => "C2",
        20 => "C1",
        21 => "C0",
        7 => "Inter",
        8 => "Wet",
        9 => "Dry",
        10 => "Wet",
        11 => "SuperSoft",
        12 => "Soft",
        13 => "Medium",
        14 => "Hard",
        15 => "Wet",
        _ => "?",
    }
}

pub fn ers_mode_name(m: u8) -> &'static str {
    match m {
        0 => "None",
        1 => "Medium",
        2 => "Hotlap",
        3 => "Overtake",
        _ => "Unknown",
    }
}

pub fn safety_car_name(s: u8) -> &'static str {
    match s {
        1 => "full",
        2 => "virtual",
        3 => "formation",
        _ => "none",
    }
}

pub fn marshal_flag_name(f: i8) -> &'static str {
    match f {
        1 => "green",
        2 => "blue",
        3 => "yellow",
        4 => "red",
        _ => "none",
    }
}

pub fn driver_status_name(s: u8) -> &'static str {
    match s {
        0 => "garage",
        1 => "flyingLap",
        2 => "inLap",
        3 => "outLap",
        _ => "onTrack",
    }
}

pub fn result_status_name(s: u8) -> &'static str {
    match s {
        1 => "inactive",
        2 => "active",
        3 => "finished",
        4 => "dnf",
        5 => "dsq",
        6 => "notClassified",
        7 => "retired",
        _ => "inactive",
    }
}

pub fn pit_status_name(s: u8) -> &'static str {
    match s {
        1 => "pitting",
        2 => "inPitArea",
        _ => "none",
    }
}

/// Short penalty description for the `PENA` event.
pub fn penalty_type_name(t: u8) -> &'static str {
    match t {
        0 => "Drive through",
        1 => "Stop go",
        2 => "Grid penalty",
        3 => "Penalty reminder",
        4 => "Time penalty",
        5 => "Warning",
        6 => "Disqualified",
        7 => "Removed from formation lap",
        8 => "Parked too long timer",
        9 => "Tyre regulations",
        10 => "This lap invalidated",
        11 => "This and next lap invalidated",
        12 => "This lap invalidated without reason",
        13 => "This and next lap invalidated without reason",
        14 => "This and previous lap invalidated",
        15 => "This and previous lap invalidated without reason",
        16 => "Retired",
        17 => "Black flag timer",
        _ => "Penalty",
    }
}

/// Penalty types worth a radio call (excludes lap invalidations and warnings).
pub fn penalty_is_significant(t: u8) -> bool {
    matches!(t, 0 | 1 | 2 | 4 | 6 | 9 | 17)
}
