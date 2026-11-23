//! Live race state for the pit wall.
//!
//! * [`state::RaceState`] ingests decoded [`f1_udp::ParsedPacket`]s and emits
//!   [`state::StateEvent`]s (laps, flags, penalties, flashbacks, outlines).
//! * [`snapshot`] holds the JSON wire contract shared with the UI (`UI.md` §5).
//! * [`db::Db`] persists sessions, laps, radio events and track outlines.

pub mod db;
pub mod metrics;
pub mod snapshot;
pub mod state;
pub mod timing;

pub use state::{LapRow, RaceState, SessionRow, StateEvent, Thresholds};
