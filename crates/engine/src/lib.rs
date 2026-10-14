//! Pulse audio engine: clip launcher, arrangement sequencer, device chains and mixer.

mod audio;
mod effects;
mod engine;
mod input;
mod instruments;
mod offline;
pub mod model;
mod sample;
mod sampler;
mod synth;

pub use audio::{start, AudioOutput, EngineHandle, LiveInput};
pub use instruments::render_drum_hit;
pub use input::{start_input, AudioInput, InputHandle, Marker};
pub use offline::OfflineRenderer;
pub use sample::{guess_loop_bpm, Sample, PEAK_BLOCK};
pub use engine::{
    build_chain, swing_offset, ChainSlot, Command, LiveNote, PlacedClip, PlayClip, RecordedNote, SharedState, Target, TrackStatus, MAX_CHAIN, MAX_TRACKS, NONE,
    NUM_RETURNS, NUM_SCENES, QUEUED_STOP,
};
