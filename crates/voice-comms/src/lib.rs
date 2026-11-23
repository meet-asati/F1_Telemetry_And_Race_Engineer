//! Radio audio: the team-radio DSP effect, resampling, a TTS disk cache,
//! local playback and microphone capture through `cpal`, plus text
//! normalisation for speech.

pub mod cache;
pub mod dsp;
pub mod mic;
pub mod player;
pub mod speech;

pub use cache::TtsCache;
pub use dsp::{resample, RadioFx};
pub use mic::{list_input_devices, MicCapture};
pub use player::{list_output_devices, AudioPlayer};
pub use speech::{chirp, speechify};
