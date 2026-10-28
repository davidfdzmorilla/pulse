//! Pulse audio engine: clip launcher, arrangement sequencer, device chains and mixer.

mod audio;
pub mod dsp;
mod effects;
mod engine;
mod input;
mod instruments;
mod drumkit;
mod keys;
mod kick;
pub mod meter;
mod midi_fx;
mod offline;
mod piano;
pub mod model;
pub mod plugin;
mod clap_host;
mod sample;
mod sampler;
mod synth;
mod tuner;
pub use tuner::scale_bits;
pub mod wavetable;
mod wavesynth;
mod modsynth;
pub use wavesynth::preview_cycle;

pub use audio::{detached, input_devices, output_devices, start, start_with, AudioOutput, AudioSettings, DeviceInfo, EngineHandle, LiveInput};
pub use dsp::fft;
pub use effects::{eq_band_response_db, eq_eight_response_db};
pub use instruments::render_drum_hit;
pub use kick::{amplitude as kick_amplitude, drive_at as kick_drive_at, effective as kick_effective, length_ms as kick_length_ms, pitch_offset as kick_pitch_offset, render_kick};
pub use input::{detached_input, detached_input_with_voice, start_input, start_input_with, AudioInput, InputHandle, Marker, MonitorInput, VoiceInput};
pub use offline::OfflineRenderer;
pub use sample::{guess_loop_bpm, Sample, PEAK_BLOCK};
pub use engine::{
    build_chain, copy_salt, render_audio_handles, HANDLE_SECONDS, Analyzer, ANALYZER_SAMPLES, link_neighbours, render_audio, render_placed, swing_offset, ChainSlot, Command, Engine, PreparedInstrument, LiveNote, Controller, PlacedClip, PlayClip, RecordedNote, SharedState, Fault, Target, TrackStatus, MAX_CHAIN, MAX_TRACKS, NONE,
    MAX_RETURNS, NUM_SCENES, QUEUED_STOP,
};
