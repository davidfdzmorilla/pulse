//! Pulse audio engine: clip launcher, arrangement sequencer, device chains and mixer.

mod audio;
pub mod dsp;
mod effects;
mod engine;
mod input;
mod instruments;
mod kick;
mod offline;
pub mod model;
pub mod plugin;
mod clap_host;
mod sample;
mod sampler;
mod synth;
pub mod wavetable;
mod wavesynth;
mod modsynth;
pub use wavesynth::preview_cycle;

pub use audio::{input_devices, output_devices, start, start_with, AudioOutput, AudioSettings, DeviceInfo, EngineHandle, LiveInput};
pub use dsp::fft;
pub use instruments::render_drum_hit;
pub use kick::{amplitude as kick_amplitude, drive_at as kick_drive_at, effective as kick_effective, length_ms as kick_length_ms, pitch_offset as kick_pitch_offset, render_kick};
pub use input::{start_input, start_input_with, AudioInput, InputHandle, Marker};
pub use offline::OfflineRenderer;
pub use sample::{guess_loop_bpm, Sample, PEAK_BLOCK};
pub use engine::{
    build_chain, swing_offset, ChainSlot, Command, PreparedInstrument, LiveNote, Controller, PlacedClip, PlayClip, RecordedNote, SharedState, Target, TrackStatus, MAX_CHAIN, MAX_TRACKS, NONE,
    NUM_RETURNS, NUM_SCENES, QUEUED_STOP,
};
