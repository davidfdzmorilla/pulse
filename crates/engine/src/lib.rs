//! Pulse audio engine: a real-time clip launcher and sequencer.

mod audio;
mod engine;
pub mod model;
mod synth;

pub use audio::{start, AudioOutput, EngineHandle};
pub use engine::{Command, SharedState, TrackStatus, MAX_TRACKS, NONE, NUM_SCENES, QUEUED_STOP};
