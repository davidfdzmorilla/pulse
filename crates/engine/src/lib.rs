//! Pulse audio engine: clip launcher, arrangement sequencer, device chains and mixer.

mod audio;
mod effects;
mod engine;
pub mod model;
mod sample;
mod synth;

pub use audio::{start, AudioOutput, EngineHandle, LiveInput};
pub use sample::{guess_loop_bpm, Sample, PEAK_BLOCK};
pub use engine::{
    build_chain, ChainSlot, Command, LiveNote, PlacedClip, PlayClip, RecordedNote, SharedState, Target, TrackStatus, MAX_CHAIN, MAX_TRACKS, NONE,
    NUM_RETURNS, NUM_SCENES, QUEUED_STOP,
};
