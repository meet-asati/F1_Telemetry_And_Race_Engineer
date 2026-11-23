//! Synthetic race generator library (used by the `f1-synth` binary and tests).

pub mod sim;
pub mod track;

pub use sim::{Sim, SimConfig};
