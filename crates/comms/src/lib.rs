//! Race-engineer comms: a deterministic [`rules::RuleEngine`] that turns
//! telemetry into callouts, an [`arbitrator::Arbitrator`] that decides what
//! is spoken and when, and [`answers`] for the driver's radio questions.

pub mod answers;
pub mod arbitrator;
pub mod rules;

pub use arbitrator::{ArbConfig, Arbitrator, GateCtx, Utterance};
pub use rules::{Alert, RuleConfig, RuleEngine};
