//! Pulse audio engine: clip launcher, arrangement sequencer, device chains and mixer.

mod audio;
mod effects;
mod engine;
pub mod model;
mod synth;

pub use audio::{start, AudioOutput, EngineHandle};
pub use engine::{
    build_chain, ChainSlot, Command, SharedState, Target, TrackStatus, MAX_CHAIN, MAX_TRACKS, NONE,
    NUM_RETURNS, NUM_SCENES, QUEUED_STOP,
};
