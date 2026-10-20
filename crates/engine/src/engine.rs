//! The audio-thread side: transport, clip launching, arrangement playback,
//! device chains and mixing.
//!
//! The UI talks to the engine only through a lock-free command queue, and reads
//! back transport and clip state through atomics in [`SharedState`]. Nothing in
//! [`Engine::process`] allocates or locks; anything the engine replaces is
//! handed back to the UI thread as [`Garbage`] to be freed there.

use crate::effects::{EffectProcessor, ProcessContext};
use crate::model::{db_to_gain, AudioClip, AutomationLane, Device, Effect, Groove, InstrumentKind, MidiClip, Param, PluginId, WarpMode};
use crate::plugin::PluginProcessor;
use crate::sample::Sample;
use crate::instruments::{AcidBass, DrumMachine, FmSynth, SuperSaw};
use crate::kick::KickSynth;
use crate::sampler::{DrumRack, Sampler};
use crate::modsynth::ModSynth;
use crate::wavesynth::WaveSynth;
use crate::wavetable::Wavetable;
use crate::input::MonitorInput;
use crate::synth::{Click, PolySynth, HELD};
use rtrb::{Consumer, Producer};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

pub const MAX_TRACKS: usize = 64;
pub const NUM_SCENES: usize = 8;
pub const NUM_RETURNS: usize = 2;
/// Devices per chain; insertions beyond this are rejected so the chain never reallocates.
pub const MAX_CHAIN: usize = 16;
const MAX_BLOCK: usize = 1024;
const MAX_EVENTS: usize = 512;

/// `TrackStatus::queued` value meaning "stop at the next quantization boundary".
pub const QUEUED_STOP: i32 = -2;
/// Parameters a clip's envelopes can hold at once on one track.
const MAX_HELD: usize = 16;

pub const NONE: i32 = -1;

/// A mixer channel: a track, a return track or the master.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Target {
    Track(usize),
    Return(usize),
    Master,
}

/// A device ready to run on the audio thread. Build it with [`ChainSlot::new`]
/// on the UI thread, since that allocates.
pub struct ChainSlot {
    processor: Box<EffectProcessor>,
    enabled: bool,
}

impl ChainSlot {
    pub fn new(device: &Device, sample_rate: f32) -> Self {
        Self {
            processor: Box::new(EffectProcessor::new(&device.effect, sample_rate)),
            enabled: device.enabled,
        }
    }

    /// A plugin effect, loaded with [`crate::plugin::instantiate`].
    pub fn plugin(device: &Device, plugin: PluginProcessor) -> Self {
        let id = match device.effect {
            Effect::Plugin(id) => id,
            _ => PluginId(0),
        };
        Self { processor: Box::new(EffectProcessor::Plugin(id, Some(Box::new(plugin)))), enabled: device.enabled }
    }
}

/// Builds a ready-to-run chain from device descriptions (allocates; UI thread only).
pub fn build_chain(devices: &[Device], sample_rate: f32) -> Box<Vec<ChainSlot>> {
    let mut chain = Vec::with_capacity(MAX_CHAIN);
    chain.extend(devices.iter().take(MAX_CHAIN).map(|d| ChainSlot::new(d, sample_rate)));
    Box::new(chain)
}

/// A clip ready to play: MIDI notes, or an audio clip with its decoded file.
pub enum PlayClip {
    Midi(MidiClip),
    Audio(AudioClip, Arc<Sample>),
}

impl PlayClip {
    pub fn length(&self) -> f64 {
        match self {
            PlayClip::Midi(c) => c.length,
            PlayClip::Audio(c, _) => c.length,
        }
    }

    /// The clip's automation, in clip time.
    pub fn envelopes(&self) -> &[AutomationLane] {
        match self {
            PlayClip::Midi(c) => &c.envelopes,
            PlayClip::Audio(c, _) => &c.envelopes,
        }
    }
}

/// An arrangement clip ready to play; its content loops over `length` beats.
pub struct PlacedClip {
    pub start: f64,
    pub length: f64,
    pub clip: PlayClip,
    /// Audio clips that another one starts right where they end (before) or
    /// begins right where they start (after) cross-fade with it there, playing on
    /// into each other's side, instead of each fading to silence at the cut.
    pub joined: (bool, bool),
}

impl PlacedClip {
    pub fn new(start: f64, length: f64, clip: PlayClip) -> Self {
        Self { start, length, clip, joined: (false, false) }
    }

    fn end(&self) -> f64 {
        self.start + self.length
    }

    /// An audio clip that plays its content once (not looping), which can go on
    /// past its edges into the file.
    fn plays_once(&self) -> bool {
        matches!(&self.clip, PlayClip::Audio(c, _) if self.length <= c.length + 1e-9)
    }
}

/// Marks the audio clips of an arrangement that butt onto another one (pieces of
/// a split clip, or of takes), so they cross-fade there.
pub fn link_neighbours(clips: &mut [PlacedClip]) {
    for i in 0..clips.len() {
        let joined = if clips[i].plays_once() {
            let touches = |at: f64, edge: fn(&PlacedClip) -> f64| (0..clips.len()).any(|j| j != i && clips[j].plays_once() && (edge(&clips[j]) - at).abs() < 1e-6);
            (touches(clips[i].start, PlacedClip::end), touches(clips[i].end(), |c| c.start))
        } else {
            (false, false)
        };
        clips[i].joined = joined;
    }
}

/// A note played live (MIDI keyboard or computer keyboard). Velocity 0 is a note-off.
/// With a `controller`, it is a controller move instead: `velocity` holds its value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveNote {
    pub pitch: u8,
    pub velocity: f32,
    pub controller: Option<Controller>,
}

impl LiveNote {
    pub fn note(pitch: u8, velocity: f32) -> Self {
        Self { pitch, velocity, controller: None }
    }

    /// A controller move; `pitch` is the key for polyphonic pressure.
    pub fn control(controller: Controller, pitch: u8, value: f32) -> Self {
        Self { pitch, velocity: value, controller: Some(controller) }
    }
}

/// MIDI controllers that instruments can follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controller {
    /// A continuous controller, 0..1.
    Cc(u8),
    /// -1..1.
    PitchBend,
    /// Channel pressure, 0..1.
    Pressure,
    /// Polyphonic pressure on one key, 0..1.
    PolyPressure,
}

/// A note captured while recording, in transport beats. Sent to the UI on note-off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecordedNote {
    pub track: usize,
    pub pitch: u8,
    pub velocity: f32,
    pub start: f64,
    pub length: f64,
}

/// Level of the browser's preview (-6 dB).
const PREVIEW_GAIN: f32 = 0.5;

/// Things the audio thread no longer needs; the UI thread drops them.
/// The fields are only held so they are freed off the audio thread.
#[allow(dead_code)]
pub enum Garbage {
    Clip(Box<PlayClip>),
    Device(ChainSlot),
    Chain(Box<Vec<ChainSlot>>),
    Arrangement(Box<Vec<PlacedClip>>),
    Automation(Box<Vec<AutomationLane>>),
    Samples(Box<Vec<Arc<Sample>>>),
    Wavetables(Box<Vec<Arc<Wavetable>>>),
    Instrument(Box<PreparedInstrument>),
    Monitor(Box<MonitorInput>),
}

/// An instrument built off the audio thread, ready to replace a track's one.
pub struct PreparedInstrument {
    kind: InstrumentKind,
    built: Option<Instrument>,
}

pub enum Command {
    Play,
    Stop,
    /// Moves the playhead and the start marker that Stop returns to.
    SetPosition(f64),
    SetTempo(f64),
    SetMetronome(bool),
    /// Launch quantization in beats; 0 launches immediately.
    SetQuantize(f64),
    SetLoop {
        enabled: bool,
        start: f64,
        end: f64,
    },
    SetClip {
        track: usize,
        slot: usize,
        clip: Option<Box<PlayClip>>,
    },
    LaunchClip {
        track: usize,
        slot: usize,
    },
    StopTrack(usize),
    /// Puts the tracks in a new order: track `i` becomes the one that was at
    /// `order[i]`. Everything moves with them (clips playing, instruments, plugins).
    ReorderTracks([usize; MAX_TRACKS]),
    /// Which group track each track plays into (None: the master). A group
    /// mixes its tracks through its own chain and fader, like a bus.
    SetGroups([Option<u8>; MAX_TRACKS]),
    /// Each track's id (what sidechains name their source by; 0 for none).
    SetTrackIds([u32; MAX_TRACKS]),
    /// A MIDI track's MIDI effects, in order (keys already filled in).
    SetMidiEffects { track: usize, devices: [Option<crate::model::MidiDevice>; crate::model::MAX_MIDI_EFFECTS] },
    LaunchScene(usize),
    StopAllClips,
    /// Returns every track from session clips to its arrangement.
    BackToArrangement,
    SetArrangement {
        track: usize,
        clips: Box<Vec<PlacedClip>>,
    },
    /// Use [`Command::set_instrument`]: it builds the instrument on the calling thread.
    SetInstrument {
        track: usize,
        instrument: Box<PreparedInstrument>,
    },
    /// The wavetables of a track's wavetable synth, by oscillator.
    SetWavetables {
        track: usize,
        tables: Box<Vec<Arc<Wavetable>>>,
    },
    /// The samples a track's sampler or drum rack plays, by slot (pad).
    SetSamples {
        track: usize,
        samples: Box<Vec<Arc<Sample>>>,
    },
    SetGain {
        target: Target,
        gain: f32,
    },
    SetPan {
        target: Target,
        pan: f32,
    },
    SetMute {
        target: Target,
        mute: bool,
    },
    SetSolo {
        track: usize,
        solo: bool,
    },
    SetSend {
        track: usize,
        send: usize,
        amount: f32,
    },
    InsertDevice {
        target: Target,
        index: usize,
        slot: ChainSlot,
    },
    RemoveDevice {
        target: Target,
        index: usize,
    },
    UpdateDevice {
        target: Target,
        index: usize,
        device: Device,
    },
    /// Replaces a whole chain; build it with [`build_chain`].
    ReplaceChain {
        target: Target,
        chain: Box<Vec<ChainSlot>>,
    },
    /// Replaces a channel's active automation lanes (the UI leaves out overridden ones).
    SetAutomation {
        target: Target,
        lanes: Box<Vec<AutomationLane>>,
    },
    /// Delays notes on the off-beats of `grid` beats (0.5 = eighths, 0.25 = sixteenths).
    SetSwing {
        amount: f32,
        grid: f64,
    },
    /// Whether the master output is clipped to ±1 (on for speakers; off when
    /// rendering to a file so peaks can be measured and normalized).
    SetOutputLimit(bool),
    /// Arming routes live notes to a track and makes it record.
    SetArm {
        track: usize,
        armed: bool,
    },
    /// Track that plays live notes when no track is armed (the selected one).
    SetInputTrack(Option<usize>),
    /// The audio input monitored tracks play (none: silence).
    SetMonitorInput(Option<Box<MonitorInput>>),
    /// Whether an audio track plays the audio input instead of its clips.
    SetMonitor {
        track: usize,
        on: bool,
    },
    /// Starts recording a new clip into an empty slot at the next quantization boundary.
    RecordClip {
        track: usize,
        slot: usize,
    },
    /// Ends a session recording; the slot keeps looping once the UI sends its clip.
    StopRecording(usize),
    /// Records armed tracks onto the arrangement while the transport runs.
    SetArrangementRecord(bool),
    /// Beats the metronome counts before a recording starts the transport (0: none).
    SetCountIn(f64),
    /// Plays an audio file once over the output (the browser's preview), or stops
    /// the one playing.
    PreviewSample(Option<Arc<Sample>>),
    /// Audition a note on a track, e.g. while editing in the piano roll.
    PreviewNote {
        track: usize,
        pitch: u8,
        velocity: f32,
    },
}

/// A float published through an atomic.
#[derive(Default)]
struct AtomicF64(AtomicU64);

impl AtomicF64 {
    fn load(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn store(&self, value: f64) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }
}

/// Peak level since the UI last read it (f32 bits; non-negative floats order like u32).
#[derive(Default)]
pub struct Peak(AtomicU32);

impl Peak {
    fn publish(&self, peak: f32) {
        self.0.fetch_max(peak.to_bits(), Ordering::Relaxed);
    }

    pub fn take(&self) -> f32 {
        f32::from_bits(self.0.swap(0, Ordering::Relaxed))
    }
}

pub struct TrackStatus {
    pub playing: AtomicI32,
    pub queued: AtomicI32,
    /// Whether `queued` is a recording waiting to start rather than a launch.
    pub queued_record: AtomicBool,
    /// Slot being recorded into, or [`NONE`].
    pub recording: AtomicI32,
    record_start: AtomicF64,
    pub follows_arrangement: AtomicBool,
    clip_position: AtomicF64,
    launch_beat: AtomicF64,
    switch_beat: AtomicF64,
    pub peak: Peak,
}

impl TrackStatus {
    /// Playhead inside the playing session clip, in beats.
    pub fn clip_position(&self) -> f64 {
        self.clip_position.load()
    }

    /// Transport beat where the current session recording started.
    pub fn record_start(&self) -> f64 {
        self.record_start.load()
    }

    /// Transport beat where the playing session clip's beat 0 falls (it loops
    /// from there).
    pub fn launch_beat(&self) -> f64 {
        self.launch_beat.load()
    }

    /// Transport beat where the session clip last changed (launched or stopped).
    pub fn switch_beat(&self) -> f64 {
        self.switch_beat.load()
    }
}

/// State the audio thread publishes for the UI.
pub struct SharedState {
    pub playing: AtomicBool,
    pub arrangement_recording: AtomicBool,
    position: AtomicF64,
    pub tracks: [TrackStatus; MAX_TRACKS],
    pub returns: [Peak; NUM_RETURNS],
    pub master: Peak,
    /// Beats of count-in left before the transport starts (0: not counting in).
    count_in: AtomicF64,
    /// Seconds from rendering a sample to hearing it (the output device's latency).
    output_latency: AtomicF64,
    /// How many times a channel's sound stopped being valid numbers (NaN or
    /// infinite) and was silenced, and the last one: see [`Fault`].
    pub faults: AtomicU32,
    last_fault: AtomicI32,
}

/// Where a channel's sound stopped being valid numbers (see [`SharedState::faults`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Track(usize),
    Return(usize),
    Master,
}

impl Fault {
    fn code(self) -> i32 {
        match self {
            Fault::Track(t) => t as i32,
            Fault::Return(r) => -2 - r as i32,
            Fault::Master => -1,
        }
    }

    fn from_code(code: i32) -> Self {
        match code {
            -1 => Fault::Master,
            c if c < -1 => Fault::Return((-2 - c) as usize),
            c => Fault::Track(c as usize),
        }
    }
}

impl SharedState {
    fn new() -> Self {
        Self {
            playing: AtomicBool::new(false),
            arrangement_recording: AtomicBool::new(false),
            position: AtomicF64::default(),
            tracks: std::array::from_fn(|_| TrackStatus {
                playing: AtomicI32::new(NONE),
                queued: AtomicI32::new(NONE),
                queued_record: AtomicBool::new(false),
                recording: AtomicI32::new(NONE),
                record_start: AtomicF64::default(),
                follows_arrangement: AtomicBool::new(true),
                clip_position: AtomicF64::default(),
                launch_beat: AtomicF64::default(),
                switch_beat: AtomicF64::default(),
                peak: Peak::default(),
            }),
            returns: Default::default(),
            master: Peak::default(),
            count_in: AtomicF64::default(),
            output_latency: AtomicF64::default(),
            faults: AtomicU32::new(0),
            last_fault: AtomicI32::new(0),
        }
    }

    /// The channel that went invalid last (see [`SharedState::faults`]).
    pub fn last_fault(&self) -> Fault {
        Fault::from_code(self.last_fault.load(Ordering::Relaxed))
    }

    fn fault(&self, at: Fault) {
        self.last_fault.store(at.code(), Ordering::Relaxed);
        self.faults.fetch_add(1, Ordering::Relaxed);
    }

    /// Beats of count-in left before recording starts (0: none).
    pub fn count_in(&self) -> f64 {
        self.count_in.load()
    }

    /// Seconds between rendering audio and hearing it.
    pub fn output_latency(&self) -> f64 {
        self.output_latency.load()
    }

    pub fn set_output_latency(&self, seconds: f64) {
        self.output_latency.store(seconds);
    }

    /// Transport position in beats.
    pub fn position(&self) -> f64 {
        self.position.load()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Queued {
    Nothing,
    Slot(usize),
    Record(usize),
    Stop,
}

impl Command {
    /// Sets a track's instrument. A new kind of instrument is built here, so the
    /// audio thread only swaps it in; the same kind just takes the new settings.
    pub fn set_instrument(track: usize, kind: InstrumentKind, sample_rate: f32) -> Command {
        Command::SetInstrument { track, instrument: Box::new(PreparedInstrument { kind, built: Some(Instrument::new(kind, sample_rate)) }) }
    }

    /// Changes the settings of the instrument already on a track (same kind).
    pub fn update_instrument(track: usize, kind: InstrumentKind) -> Command {
        Command::SetInstrument { track, instrument: Box::new(PreparedInstrument { kind, built: None }) }
    }

    /// Puts a loaded plugin instrument (see [`crate::plugin::instantiate`]) on a track.
    pub fn set_plugin(track: usize, id: PluginId, plugin: PluginProcessor) -> Command {
        let kind = InstrumentKind::Plugin(id);
        Command::SetInstrument { track, instrument: Box::new(PreparedInstrument { kind, built: Some(Instrument::Plugin(id, Some(Box::new(plugin)))) }) }
    }
}

enum Instrument {
    Synth(PolySynth),
    Drums(DrumMachine),
    SuperSaw(SuperSaw),
    Acid(AcidBass),
    Fm(FmSynth),
    Sampler(Sampler),
    Rack(DrumRack),
    Kick(KickSynth),
    /// The wavetable engine; true when it is the polychrome synth.
    Wavetable(Box<WaveSynth>, bool),
    Modular(Box<ModSynth>),
    /// A VST3 instrument; None (silent) until the app loads it.
    Plugin(PluginId, Option<Box<PluginProcessor>>),
}

impl Instrument {
    fn new(kind: InstrumentKind, sample_rate: f32) -> Self {
        match kind.upgraded() {
            InstrumentKind::Synth(p) => Self::Synth(PolySynth::new(p, sample_rate)),
            InstrumentKind::DrumMachine(p) => Self::Drums(DrumMachine::new(p, sample_rate)),
            InstrumentKind::SuperSaw(p) => Self::SuperSaw(SuperSaw::new(p, sample_rate)),
            InstrumentKind::Acid(p) => Self::Acid(AcidBass::new(p, sample_rate)),
            InstrumentKind::Fm(p) => Self::Fm(FmSynth::new(p, sample_rate)),
            InstrumentKind::Sampler(p) => Self::Sampler(Sampler::new(p, sample_rate)),
            InstrumentKind::DrumRack(p) => Self::Rack(DrumRack::new(p, sample_rate)),
            InstrumentKind::Kick(p) => Self::Kick(KickSynth::new(p, sample_rate)),
            InstrumentKind::Wavetable(p) => Self::Wavetable(Box::new(WaveSynth::new(p, sample_rate)), false),
            InstrumentKind::Polychrome(p) => Self::Wavetable(Box::new(WaveSynth::new(p, sample_rate)), true),
            InstrumentKind::Modular(p) => Self::Modular(Box::new(ModSynth::new(p, sample_rate))),
            InstrumentKind::Plugin(id) => Self::Plugin(id, None),
            InstrumentKind::Drums => unreachable!("upgraded above"),
        }
    }

    fn kind(&self) -> InstrumentKind {
        match self {
            Self::Synth(s) => InstrumentKind::Synth(s.params()),
            Self::Drums(d) => InstrumentKind::DrumMachine(d.params()),
            Self::SuperSaw(s) => InstrumentKind::SuperSaw(s.params()),
            Self::Acid(a) => InstrumentKind::Acid(a.params()),
            Self::Fm(f) => InstrumentKind::Fm(f.params()),
            Self::Sampler(s) => InstrumentKind::Sampler(s.params()),
            Self::Rack(r) => InstrumentKind::DrumRack(r.params()),
            Self::Kick(k) => InstrumentKind::Kick(k.params()),
            Self::Wavetable(w, false) => InstrumentKind::Wavetable(w.params()),
            Self::Wavetable(w, true) => InstrumentKind::Polychrome(w.params()),
            Self::Modular(m) => InstrumentKind::Modular(m.params()),
            Self::Plugin(id, _) => InstrumentKind::Plugin(*id),
        }
    }

    /// Updates parameters in place, keeping sounding notes. False if `kind` is another instrument.
    fn update(&mut self, kind: InstrumentKind) -> bool {
        match (self, kind.upgraded()) {
            (Self::Synth(s), InstrumentKind::Synth(p)) => s.set_params(p),
            (Self::Drums(d), InstrumentKind::DrumMachine(p)) => d.set_params(p),
            (Self::SuperSaw(s), InstrumentKind::SuperSaw(p)) => s.set_params(p),
            (Self::Acid(a), InstrumentKind::Acid(p)) => a.set_params(p),
            (Self::Fm(f), InstrumentKind::Fm(p)) => f.set_params(p),
            (Self::Sampler(s), InstrumentKind::Sampler(p)) => s.set_params(p),
            (Self::Rack(r), InstrumentKind::DrumRack(p)) => r.set_params(p),
            (Self::Kick(k), InstrumentKind::Kick(p)) => k.set_params(p),
            (Self::Wavetable(w, false), InstrumentKind::Wavetable(p)) | (Self::Wavetable(w, true), InstrumentKind::Polychrome(p)) => w.set_params(p),
            (Self::Modular(m), InstrumentKind::Modular(p)) => m.set_params(p),
            (Self::Plugin(id, _), InstrumentKind::Plugin(other)) => return *id == other,
            _ => return false,
        }
        true
    }

    /// Instruments with tempo-synced settings follow the transport.
    fn set_tempo(&mut self, tempo: f64) {
        match self {
            Self::Kick(k) => k.set_tempo(tempo),
            Self::Wavetable(w, _) => w.set_tempo(tempo),
            Self::Modular(m) => m.set_tempo(tempo),
            Self::Plugin(_, Some(p)) => p.set_tempo(tempo),
            _ => {}
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        // A plugin's parameters are its own ids, normalized.
        if let Self::Plugin(_, plugin) = self {
            if let Some(p) = plugin {
                p.set_normalized(index as u32, value as f64);
            }
            return;
        }
        let mut kind = self.kind();
        kind.set(index, value);
        self.update(kind);
    }

    fn note_on(&mut self, samples: &[Arc<Sample>], pitch: u8, velocity: f32, gate_samples: u32) {
        match self {
            Self::Synth(s) => s.note_on(pitch, velocity, gate_samples),
            Self::Drums(d) => d.note_on(pitch, velocity),
            Self::SuperSaw(s) => s.note_on(pitch, velocity, gate_samples),
            Self::Acid(a) => a.note_on(pitch, velocity, gate_samples),
            Self::Fm(f) => f.note_on(pitch, velocity, gate_samples),
            Self::Sampler(s) => s.note_on(samples, pitch, velocity, gate_samples),
            Self::Rack(r) => r.note_on(samples, pitch, velocity),
            Self::Kick(k) => k.note_on(pitch, velocity),
            Self::Wavetable(w, _) => w.note_on(pitch, velocity, gate_samples),
            Self::Modular(m) => m.note_on(pitch, velocity, gate_samples),
            Self::Plugin(_, Some(p)) => p.note_on(pitch, velocity, gate_samples),
            Self::Plugin(_, None) => {}
        }
    }

    fn note_off(&mut self, pitch: u8) {
        match self {
            Self::Synth(s) => s.note_off(pitch),
            Self::Drums(_) => {}
            Self::SuperSaw(s) => s.note_off(pitch),
            Self::Acid(a) => a.note_off(pitch),
            Self::Fm(f) => f.note_off(pitch),
            Self::Sampler(s) => s.note_off(pitch),
            Self::Rack(_) | Self::Kick(_) => {}
            Self::Wavetable(w, _) => w.note_off(pitch),
            Self::Modular(m) => m.note_off(pitch),
            Self::Plugin(_, Some(p)) => p.note_off(pitch),
            Self::Plugin(_, None) => {}
        }
    }

    /// Bends the built-in pitched instruments by `ratio`. False for the ones that take
    /// pitch bend themselves (modular synth, plugins) or play drums.
    fn set_pitch(&mut self, ratio: f32) -> bool {
        match self {
            Self::Synth(s) => s.set_pitch(ratio),
            Self::SuperSaw(s) => s.set_pitch(ratio),
            Self::Acid(a) => a.set_pitch(ratio),
            Self::Fm(f) => f.set_pitch(ratio),
            Self::Sampler(s) => s.set_pitch(ratio),
            Self::Wavetable(w, _) => w.set_pitch(ratio),
            Self::Drums(_) | Self::Rack(_) | Self::Kick(_) | Self::Modular(_) | Self::Plugin(..) => return false,
        }
        true
    }

    /// The modular synth and plugins follow controllers.
    fn control(&mut self, controller: Controller, pitch: u8, value: f32) {
        match self {
            Self::Modular(m) => m.control(controller, pitch, value),
            Self::Plugin(_, Some(p)) => p.control(controller, pitch, value),
            _ => {}
        }
    }

    fn all_notes_off(&mut self) {
        match self {
            Self::Synth(s) => s.all_notes_off(),
            Self::Drums(_) => {}
            Self::SuperSaw(s) => s.all_notes_off(),
            Self::Acid(a) => a.all_notes_off(),
            Self::Fm(f) => f.all_notes_off(),
            Self::Sampler(s) => s.all_notes_off(),
            Self::Rack(_) => {}
            Self::Kick(k) => k.all_notes_off(),
            Self::Wavetable(w, _) => w.all_notes_off(),
            Self::Modular(m) => m.all_notes_off(),
            Self::Plugin(_, Some(p)) => p.all_notes_off(),
            Self::Plugin(_, None) => {}
        }
    }

    /// Adds the instrument's output to `left`/`right`.
    fn render(&mut self, samples: &[Arc<Sample>], tables: &[Arc<Wavetable>], left: &mut [f32], right: &mut [f32]) {
        match self {
            Self::Synth(s) => {
                // Mono synth: render once and copy.
                s.render(left);
                right.copy_from_slice(left);
            }
            Self::Drums(d) => d.render(left, right),
            Self::SuperSaw(s) => s.render(left, right),
            Self::Acid(a) => a.render(left, right),
            Self::Fm(f) => f.render(left, right),
            Self::Sampler(s) => s.render(samples, left, right),
            Self::Rack(r) => r.render(samples, left, right),
            Self::Kick(k) => k.render(samples, left, right),
            Self::Wavetable(w, _) => w.render(samples, tables, left, right),
            Self::Modular(m) => m.render(samples, tables, left, right),
            Self::Plugin(_, Some(p)) => p.render(left, right),
            Self::Plugin(_, None) => {}
        }
    }
}

/// Pitch bend range of the built-in instruments, in semitones.
const BEND_SEMITONES: f32 = 2.0;
/// The modulation wheel's vibrato: its rate, its depth at full wheel (semitones) and how
/// often its pitch is updated (frames).
const VIBRATO_HZ: f32 = 5.5;
const VIBRATO_SEMITONES: f32 = 0.5;
const VIBRATO_STEP: usize = 64;

/// What the pitch bend and the modulation wheel do to a track's built-in instrument:
/// bend it up to two semitones either way, and add vibrato. (The modular synth and
/// plugins take the controllers themselves.)
#[derive(Clone, Copy, Debug, Default)]
struct Expression {
    /// -1..1.
    bend: f32,
    /// 0..1.
    wheel: f32,
    /// The vibrato's phase, 0..1.
    phase: f32,
}

impl Expression {
    fn control(&mut self, controller: Controller, value: f32) {
        match controller {
            Controller::PitchBend => self.bend = value.clamp(-1.0, 1.0),
            Controller::Cc(1) => self.wheel = value.clamp(0.0, 1.0),
            // All notes off and reset controllers bring the wheels back.
            Controller::Cc(121) => *self = Expression { phase: self.phase, ..Default::default() },
            _ => {}
        }
    }

    /// The frequency ratio now.
    fn ratio(&self) -> f32 {
        let vibrato = (self.phase * std::f32::consts::TAU).sin() * self.wheel * VIBRATO_SEMITONES;
        2f32.powf((self.bend * BEND_SEMITONES + vibrato) / 12.0)
    }

    /// Renders `instrument`, bent and with vibrato, updating the pitch every few frames.
    fn render(&mut self, instrument: &mut Instrument, sr: f32, samples: &[Arc<Sample>], tables: &[Arc<Wavetable>], left: &mut [f32], right: &mut [f32]) {
        if !instrument.set_pitch(1.0) {
            instrument.render(samples, tables, left, right);
            self.phase = (self.phase + left.len() as f32 * VIBRATO_HZ / sr).fract();
            return;
        }
        let step = if self.wheel > 0.0 { VIBRATO_STEP } else { left.len().max(1) };
        for (l, r) in left.chunks_mut(step).zip(right.chunks_mut(step)) {
            instrument.set_pitch(self.ratio());
            instrument.render(samples, tables, l, r);
            self.phase = (self.phase + l.len() as f32 * VIBRATO_HZ / sr).fract();
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Event {
    AllNotesOff,
    NoteOn { pitch: u8, velocity: f32, gate: u32 },
    NoteOff { pitch: u8 },
    Control { controller: Controller, pitch: u8, value: f32 },
}

/// (frame offset, sort rank, event). Pre-allocated; never grows on the audio thread.
pub(crate) type EventQueue = Vec<(usize, u8, Event)>;

pub(crate) fn push_event(events: &mut EventQueue, frame: usize, event: Event) {
    if events.len() < MAX_EVENTS {
        let rank = match event {
            Event::AllNotesOff => 0,
            Event::NoteOff { .. } | Event::Control { .. } => 1,
            Event::NoteOn { .. } => 2,
        };
        events.push((frame, rank, event));
    }
}

/// Maps beats inside the current segment of a block to frame offsets.
#[derive(Clone, Copy)]
struct Timing {
    segment_beat: f64,
    segment_frame: usize,
    /// First frame after the segment.
    segment_end: usize,
    beats_per_sample: f64,
    frames: usize,
    /// Swing grid (beats) and amount applied to MIDI notes.
    swing: (f64, f32),
    /// Where the transport jumps back (the loop's end), if it does from here:
    /// arrangement notes end there.
    loop_end: f64,
}

impl Timing {
    fn frame(&self, beat: f64) -> usize {
        let offset = ((beat - self.segment_beat) / self.beats_per_sample).max(0.0) as usize;
        (self.segment_frame + offset).min(self.frames - 1)
    }
}

/// Delay for a note starting at `start` (clip beats): notes exactly on the off-beats
/// of `grid` (its 2nd, 4th… steps) move later. At amount 1 the delay is a third of a
/// grid step, which turns straight eighths/sixteenths into a triplet shuffle.
pub fn swing_offset(start: f64, grid: f64, amount: f32) -> f64 {
    if amount <= 0.0 || grid <= 0.0 {
        return 0.0;
    }
    let step = start / grid;
    let nearest = step.round();
    if (step - nearest).abs() > 1e-6 || (nearest as i64).rem_euclid(2) == 0 {
        return 0.0;
    }
    grid * amount as f64 / 3.0
}

/// Schedules note-ons of `clip` (looping, local time 0 at `origin`) that start in
/// `[from, to)`, with swing applied. Notes are cut short at `limit`.
fn schedule_notes(
    clip: &MidiClip,
    origin: f64,
    from: f64,
    to: f64,
    limit: f64,
    timing: &Timing,
    events: &mut EventQueue,
) {
    if clip.length <= 0.0 || to <= from {
        return;
    }
    let local_from = from - origin;
    let local_to = to - origin;
    let first_lap = (local_from / clip.length).floor() as i64;
    let last_lap = (local_to / clip.length).floor() as i64;
    // The clip's own groove, or the project's swing.
    let (grid, amount) = timing.swing;
    let groove = clip.groove.unwrap_or(Groove { grid, swing: amount, accent: 0.0, humanize: 0.0 });
    // Humanized notes may come a hair early: the next lap's first ones too.
    let last_lap = if groove.humanize > 0.0 { last_lap + 1 } else { last_lap };
    for lap in first_lap..=last_lap {
        let lap_start = lap as f64 * clip.length;
        for (index, note) in clip.notes.iter().enumerate().filter(|(_, n)| n.start < clip.length) {
            let (shift, velocity) = groove.apply(index, lap, note.start, note.velocity);
            // Never before the clip starts.
            let t = (lap_start + note.start + shift).max(0.0);
            if t < local_from || t >= local_to {
                continue;
            }
            let beat = origin + t;
            let length = note.length.min(limit - beat);
            if length <= 0.0 {
                continue;
            }
            let gate = (length / timing.beats_per_sample).max(1.0) as u32;
            push_event(
                events,
                timing.frame(beat),
                Event::NoteOn {
                    pitch: note.pitch,
                    velocity,
                    gate,
                },
            );
        }
    }
}

/// Length of the fades applied at clip edges and loop points, in seconds.
const EDGE_FADE: f64 = 0.003;
/// Length of the cross-fade between two audio clips that follow on, in seconds.
const CROSSFADE: f64 = 0.010;
/// Grain length of the Stretch warp mode, in seconds.
const GRAIN: f64 = 0.06;
/// Monitoring: input frames queued beyond a block that count as late, and how many
/// are kept when the rest is skipped.
const MONITOR_SLACK: usize = 1024;
const MONITOR_CUSHION: usize = 256;

/// What an audio clip sounds like over its first `beats` (looping), at `tempo`,
/// as audio at `rate`: its warp, transposition and gain applied, as playback does
/// (consolidating a clip into a file of its own).
pub fn render_audio(clip: &AudioClip, sample: &Sample, beats: f64, tempo: f64, rate: f32) -> (Vec<f32>, Vec<f32>) {
    let frames = (beats * 60.0 / tempo * rate as f64).round() as usize;
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    let beats_per_sample = tempo / 60.0 / rate as f64;
    render_audio_clip(clip, sample, 0.0, beats, (false, false), (0, frames), 0.0, beats_per_sample, tempo, &mut left, &mut right);
    (left, right)
}

/// What an arrangement audio clip plays over `frames` frames from `beat`, at
/// `tempo`, as audio at `rate`: silent outside it, cross-fading where it is
/// joined (joining clips into one file).
pub fn render_placed(placed: &PlacedClip, beat: f64, frames: usize, tempo: f64, rate: f32) -> (Vec<f32>, Vec<f32>) {
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    if let PlayClip::Audio(clip, sample) = &placed.clip {
        let beats_per_sample = tempo / 60.0 / rate as f64;
        render_audio_clip(clip, sample, placed.start, placed.end(), placed.joined, (0, frames), beat, beats_per_sample, tempo, &mut left, &mut right);
    }
    (left, right)
}

/// Adds an audio clip (looping, local time 0 at `origin`, silent from `limit`) to
/// `left`/`right` over frames `[from, to)`; `beat` is the transport position at `from`.
/// `joined` (start, end): the clip cross-fades with a neighbour there, playing on
/// half the cross-fade past that edge (only for clips that play once).
#[allow(clippy::too_many_arguments)]
fn render_audio_clip(
    clip: &AudioClip,
    sample: &Sample,
    origin: f64,
    limit: f64,
    joined: (bool, bool),
    (from, to): (usize, usize),
    beat: f64,
    beats_per_sample: f64,
    tempo: f64,
    left: &mut [f32],
    right: &mut [f32],
) {
    if sample.frames() == 0 || clip.length <= 0.0 {
        return;
    }
    // Warped, beats inside the clip map to file time through its warp markers (or
    // at its original tempo): the clip starts at this beat of the file's.
    let first_beat = clip.file_beat(clip.offset);
    let rate = sample.sample_rate as f64;
    let gain = db_to_gain(clip.gain_db);
    let fade = EDGE_FADE * tempo / 60.0;
    let crossing = joined.0 || joined.1;
    let half = CROSSFADE * tempo / 60.0 / 2.0;
    let lo = if joined.0 { origin - half } else { origin };
    let hi = if joined.1 { limit + half } else { limit };
    let pitch = 2f64.powf(clip.transpose as f64 / 12.0);
    let hop = GRAIN / 2.0;

    for k in from..to {
        let b = beat + (k - from) as f64 * beats_per_sample;
        if b < lo || b >= hi {
            continue;
        }
        let envelope;
        let local;
        if crossing {
            // Plays once: past an edge it reads on into the file.
            local = b - origin;
            let fade_in = if joined.0 { (local + half) / (2.0 * half) } else { local / fade };
            let fade_out = if joined.1 { (limit + half - b) / (2.0 * half) } else { (limit - b) / fade };
            envelope = (fade_in.clamp(0.0, 1.0) * fade_out.clamp(0.0, 1.0)) as f32;
        } else {
            local = (b - origin).rem_euclid(clip.length);
            let remaining = (clip.length - local).min(limit - b);
            envelope = ((local / fade).min(1.0) * (remaining / fade).min(1.0)) as f32;
        }
        let (l, r) = match clip.warp {
            WarpMode::Off => sample.read((clip.offset + local * 60.0 / tempo) * rate),
            WarpMode::Repitch => sample.read(clip.file_seconds(first_beat + local) * rate),
            WarpMode::Stretch => {
                // Two overlapping Hann-windowed grains read the file at its own speed
                // (times the transposition) from positions that follow the tempo.
                let t = local * 60.0 / tempo;
                let newest = (t / hop).floor();
                let (mut l, mut r) = (0.0, 0.0);
                for grain in [newest, newest - 1.0] {
                    let grain_start = grain * hop;
                    let phase = (t - grain_start) / GRAIN;
                    let window = (0.5 - 0.5 * (std::f64::consts::TAU * phase).cos()) as f32;
                    let source = clip.file_seconds(first_beat + grain_start * tempo / 60.0) + (t - grain_start) * pitch;
                    let (gl, gr) = sample.read(source * rate);
                    l += gl * window;
                    r += gr * window;
                }
                (l, r)
            }
        };
        left[k] += l * gain * envelope;
        right[k] += r * gain * envelope;
    }
}

struct Channel {
    chain: Vec<ChainSlot>,
    gain: f32,
    pan: f32,
    /// The fader and pan as set (automation and clip envelopes move `gain` and
    /// `pan` from these, block by block, and they come back when those stop).
    base_gain: f32,
    base_pan: f32,
    mute: bool,
    left: Vec<f32>,
    right: Vec<f32>,
    automation: Box<Vec<AutomationLane>>,
    /// Pan/fader gains reached at the end of the last block, ramped from to avoid
    /// zipper noise. None until the first block, which starts at its gains: a new
    /// channel has nothing sounding yet, and a ramp from silence would soften the
    /// first hit of every export.
    last_gains: Option<(f32, f32)>,
}

impl Channel {
    fn new() -> Self {
        Self {
            chain: Vec::with_capacity(MAX_CHAIN),
            gain: 1.0,
            pan: 0.0,
            base_gain: 1.0,
            base_pan: 0.0,
            mute: false,
            left: vec![0.0; MAX_BLOCK],
            right: vec![0.0; MAX_BLOCK],
            automation: Box::default(),
            last_gains: None,
        }
    }

    /// Applies automation for the mixer and device parameters at `beat`.
    /// Returns values of parameters the channel does not own itself (sends, instrument).
    fn automate(&mut self, beat: f64, mut other: impl FnMut(Param, f32)) {
        self.gain = self.base_gain;
        self.pan = self.base_pan;
        for lane in self.automation.iter() {
            if let Some(value) = lane.value_at(beat) {
                Self::set(lane.param, value, &mut self.gain, &mut self.pan, &mut self.chain, &mut other);
            }
        }
    }

    /// Like [`Channel::automate`], with lanes from elsewhere (a clip's envelopes).
    fn automate_with(&mut self, lanes: &[AutomationLane], beat: f64, mut other: impl FnMut(Param, f32)) {
        for lane in lanes {
            if let Some(value) = lane.value_at(beat) {
                Self::set(lane.param, value, &mut self.gain, &mut self.pan, &mut self.chain, &mut other);
            }
        }
    }

    fn set(param: Param, value: f32, gain: &mut f32, pan: &mut f32, chain: &mut [ChainSlot], other: &mut impl FnMut(Param, f32)) {
        match param {
            Param::Volume => *gain = value.max(0.0),
            Param::Pan => *pan = value.clamp(-1.0, 1.0),
            Param::Device { device, param } => {
                if let Some(slot) = chain.get_mut(device) {
                    slot.processor.set_param(param, value);
                }
            }
            param @ (Param::Send(_) | Param::Instrument(_) | Param::Tempo) => other(param, value),
        }
    }

    /// Mixes this channel into `left`/`right` with its pan and `gain`, ramping from
    /// the previous block's gains. Returns the output peak.
    fn mix_into(&mut self, n: usize, gain: f32, left: &mut [f32], right: &mut [f32]) -> f32 {
        let (gl, gr) = self.pan_gains(gain);
        let (l0, r0) = self.last_gains.unwrap_or((gl, gr));
        self.last_gains = Some((gl, gr));
        let step = 1.0 / n as f32;
        let mut peak = 0f32;
        for i in 0..n {
            let t = (i + 1) as f32 * step;
            let out_l = self.left[i] * (l0 + (gl - l0) * t);
            let out_r = self.right[i] * (r0 + (gr - r0) * t);
            peak = peak.max(out_l.abs()).max(out_r.abs());
            left[i] += out_l;
            right[i] += out_r;
        }
        peak
    }

    /// Runs the effects. One NaN or infinity (a plugin, a filter that blew up, a
    /// damaged file) would stay in reverbs and delays and reach everything after
    /// them for good: then the block is silenced and the effects' memory cleared.
    /// False if that happened.
    fn run_chain(&mut self, n: usize, ctx: &ProcessContext) -> bool {
        let (left, right) = (&mut self.left[..n], &mut self.right[..n]);
        for slot in self.chain.iter_mut().filter(|s| s.enabled) {
            slot.processor.process(left, right, ctx);
        }
        if left.iter().chain(right.iter()).all(|s| s.is_finite()) {
            return true;
        }
        left.fill(0.0);
        right.fill(0.0);
        for slot in &mut self.chain {
            slot.processor.reset();
        }
        false
    }

    /// Constant-power pan law, unity at center, combined with the fader gain.
    fn pan_gains(&self, gain: f32) -> (f32, f32) {
        let angle = (self.pan + 1.0) * std::f32::consts::FRAC_PI_4;
        let unity = std::f32::consts::SQRT_2;
        (angle.cos() * gain * unity, angle.sin() * gain * unity)
    }
}

struct Track {
    channel: Channel,
    slots: [Option<Box<PlayClip>>; NUM_SCENES],
    playing: Option<usize>,
    launch_beat: f64,
    /// Transport beat of the last change of session clip (a launch or a stop).
    switch_beat: f64,
    queued: Queued,
    instrument: Instrument,
    /// What the sampler or drum rack plays.
    samples: Box<Vec<Arc<Sample>>>,
    /// What the wavetable synth's oscillators read.
    tables: Box<Vec<Arc<Wavetable>>>,
    solo: bool,
    sends: [f32; NUM_RETURNS],
    /// The sends as set (see `Channel::base_gain`).
    base_sends: [f32; NUM_RETURNS],
    /// Device and instrument parameters a clip's envelopes hold, with the value
    /// they had before: it comes back when the clip ends.
    envelope_held: [Option<(Param, f32)>; MAX_HELD],
    arrangement: Box<Vec<PlacedClip>>,
    follows_arrangement: bool,
    buffer: Vec<f32>,
    buffer_right: Vec<f32>,
    /// Audio clip output for the current block, mixed after the instrument.
    audio_left: Vec<f32>,
    audio_right: Vec<f32>,
    events: EventQueue,
    /// What its notes go through before the instrument.
    midi_fx: crate::midi_fx::MidiChain,
    /// Pitch bend and modulation wheel for the built-in instruments.
    expression: Expression,
    armed: bool,
    /// Plays the audio input instead of its clips.
    monitor: bool,
    /// Session slot being recorded into.
    recording: Option<usize>,
    record_start: f64,
    /// Live notes being captured: (start beat, velocity) per pitch.
    held: [Option<(f64, f32)>; 128],
}

fn is_permutation(order: &[usize]) -> bool {
    let mut seen = [false; MAX_TRACKS];
    order.len() == MAX_TRACKS && order.iter().all(|&o| o < MAX_TRACKS && !std::mem::replace(&mut seen[o], true))
}

/// Sends the notes still held on a track to the UI, ending them at `end`.
fn flush_held(track: &mut Track, index: usize, end: f64, recorded: &mut Producer<RecordedNote>) {
    for (pitch, held) in track.held.iter_mut().enumerate() {
        if let Some((start, velocity)) = held.take() {
            let length = (end - start).max(0.05);
            let _ = recorded.push(RecordedNote { track: index, pitch: pitch as u8, velocity, start, length });
        }
    }
}

impl Track {
    fn new(sample_rate: f32) -> Self {
        Self {
            channel: Channel::new(),
            slots: Default::default(),
            playing: None,
            launch_beat: 0.0,
            switch_beat: 0.0,
            queued: Queued::Nothing,
            instrument: Instrument::new(InstrumentKind::Synth(Default::default()), sample_rate),
            samples: Box::default(),
            tables: Box::default(),
            solo: false,
            sends: [0.0; NUM_RETURNS],
            base_sends: [0.0; NUM_RETURNS],
            envelope_held: [None; MAX_HELD],
            arrangement: Box::default(),
            follows_arrangement: true,
            buffer: vec![0.0; MAX_BLOCK],
            buffer_right: vec![0.0; MAX_BLOCK],
            audio_left: vec![0.0; MAX_BLOCK],
            audio_right: vec![0.0; MAX_BLOCK],
            events: Vec::with_capacity(MAX_EVENTS),
            midi_fx: crate::midi_fx::MidiChain::new(MAX_EVENTS),
            expression: Expression::default(),
            armed: false,
            monitor: false,
            recording: None,
            record_start: 0.0,
            held: [None; 128],
        }
    }

    /// Queues the note-ons that start in the beat range `[from, to)`.
    fn collect_notes(&mut self, from: f64, to: f64, timing: &Timing) {
        if let Some(slot) = self.playing {
            if let Some(PlayClip::Midi(clip)) = self.slots[slot].as_deref() {
                schedule_notes(clip, self.launch_beat, from, to, f64::INFINITY, timing, &mut self.events);
            }
        } else if self.follows_arrangement {
            for placed in self.arrangement.iter() {
                let PlayClip::Midi(clip) = &placed.clip else { continue };
                let (start, end) = (from.max(placed.start), to.min(placed.end()));
                if start < end {
                    // Cut at the loop's end, where the transport jumps back: they end by
                    // themselves, and notes played live go on sounding across it.
                    schedule_notes(clip, placed.start, start, end, placed.end().min(timing.loop_end), timing, &mut self.events);
                }
            }
        }
    }

    /// Renders the audio clips sounding during frames `[from, to)`, where `beat`
    /// is the transport position at `from`.
    fn render_audio(&mut self, (from, to): (usize, usize), beat: f64, beats_per_sample: f64, tempo: f64) {
        if to <= from {
            return;
        }
        let (left, right) = (&mut self.audio_left, &mut self.audio_right);
        if let Some(slot) = self.playing {
            if let Some(PlayClip::Audio(clip, sample)) = self.slots[slot].as_deref() {
                let frames = (from, to);
                render_audio_clip(clip, sample, self.launch_beat, f64::INFINITY, (false, false), frames, beat, beats_per_sample, tempo, left, right);
            }
        } else if self.follows_arrangement {
            let end = beat + (to - from) as f64 * beats_per_sample;
            // Half a cross-fade either side, for clips playing on past their edges.
            let margin = CROSSFADE * tempo / 60.0;
            for placed in self.arrangement.iter() {
                let PlayClip::Audio(clip, sample) = &placed.clip else { continue };
                if placed.start - margin < end && placed.end() + margin > beat {
                    let frames = (from, to);
                    render_audio_clip(clip, sample, placed.start, placed.end(), placed.joined, frames, beat, beats_per_sample, tempo, left, right);
                }
            }
        }
    }

    fn stop_session_clip(&mut self) {
        self.playing = None;
        self.queued = Queued::Nothing;
        self.instrument.all_notes_off();
    }
}

pub struct Engine {
    commands: Consumer<Command>,
    live: Consumer<LiveNote>,
    recorded: Producer<RecordedNote>,
    garbage: Producer<Garbage>,
    shared: Arc<SharedState>,
    sample_rate: f32,
    tracks: Vec<Track>,
    returns: Vec<Channel>,
    master: Channel,
    playing: bool,
    position: f64,
    start_marker: f64,
    tempo: f64,
    quantize: f64,
    loop_enabled: bool,
    loop_start: f64,
    loop_end: f64,
    metronome: bool,
    count_in_beats: f64,
    /// Beats counted in so far, while counting in.
    counting: Option<f64>,
    click: Click,
    click_buffer: Vec<f32>,
    input_track: Option<usize>,
    arrangement_record: bool,
    limit_output: bool,
    swing: (f64, f32),
    /// The file the browser is previewing, and where in it (frames of the file).
    preview: Option<(Arc<Sample>, f64)>,
    preview_buffer: Vec<f32>,
    monitor_input: Option<Box<MonitorInput>>,
    /// This block of input, as stereo, for the monitored tracks.
    monitor_left: Vec<f32>,
    monitor_right: Vec<f32>,
    /// The group each track plays into (see [`Command::SetGroups`]), and the
    /// order tracks are processed in: the deepest first, so a group has all its
    /// tracks before it runs its own chain.
    parents: [Option<u8>; MAX_TRACKS],
    order: [usize; MAX_TRACKS],
    /// A track's output on its way into its group.
    bus_left: Vec<f32>,
    bus_right: Vec<f32>,
    /// Each track's id, and its sound after its effects (before its fader) in
    /// the last block it was processed: what sidechains listen to.
    ids: [u32; MAX_TRACKS],
    key_left: Vec<Vec<f32>>,
    key_right: Vec<Vec<f32>>,
}

/// Gives the sidechained effects of a chain their sources' sound.
fn feed_keys(chain: &mut [ChainSlot], ids: &[u32; MAX_TRACKS], left: &[Vec<f32>], right: &[Vec<f32>], n: usize) {
    for slot in chain.iter_mut() {
        let Some(id) = slot.processor.sidechain() else { continue };
        if let Some(source) = ids.iter().position(|&x| x == id && id != 0) {
            slot.processor.feed_key(&left[source][..n], &right[source][..n]);
        }
    }
}

/// The order to process tracks in: every track before the group it plays
/// into, and, where it can be, a sidechain's source before the track that
/// listens to it (so it hears this block, not the last). `wants[i]` has bit `j`
/// set if track `i` listens to track `j`.
fn processing_order(parents: &[Option<u8>; MAX_TRACKS], wants: &[u64; MAX_TRACKS]) -> [usize; MAX_TRACKS] {
    let mut children = [0u64; MAX_TRACKS];
    for (i, p) in parents.iter().enumerate() {
        if let Some(p) = p {
            children[*p as usize] |= 1 << i;
        }
    }
    let ancestor = |a: usize, mut of: usize| {
        let mut steps = 0;
        while let Some(p) = parents[of] {
            if p as usize == a {
                return true;
            }
            of = p as usize;
            steps += 1;
            if steps > MAX_TRACKS {
                break;
            }
        }
        false
    };
    struct Walk {
        state: [u8; MAX_TRACKS],
        order: [usize; MAX_TRACKS],
        len: usize,
    }
    fn visit(i: usize, walk: &mut Walk, children: &[u64; MAX_TRACKS], wants: &[u64; MAX_TRACKS], ancestor: &dyn Fn(usize, usize) -> bool) {
        walk.state[i] = 1;
        // What it needs first: its tracks (always), then what it listens to
        // (unless that holds it: a group can't come before its own tracks).
        for (mask, required) in [(children[i], true), (wants[i], false)] {
            let mut bits = mask;
            while bits != 0 {
                let j = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                if walk.state[j] == 0 && (required || !ancestor(j, i)) {
                    visit(j, walk, children, wants, ancestor);
                }
            }
        }
        walk.state[i] = 2;
        walk.order[walk.len] = i;
        walk.len += 1;
    }
    let mut walk = Walk { state: [0; MAX_TRACKS], order: [0; MAX_TRACKS], len: 0 };
    for i in 0..MAX_TRACKS {
        if walk.state[i] == 0 {
            visit(i, &mut walk, &children, wants, &ancestor);
        }
    }
    walk.order
}

/// Whether each track sounds while some are soloed: soloed itself, inside a
/// soloed group, or a group holding a soloed track.
fn soloed(solo: &[bool; MAX_TRACKS], parents: &[Option<u8>; MAX_TRACKS], order: &[usize; MAX_TRACKS]) -> [bool; MAX_TRACKS] {
    let mut on = *solo;
    // Down: the shallowest first, so a group's solo reaches all its depths.
    for &i in order.iter().rev() {
        if let Some(p) = parents[i] {
            on[i] |= on[p as usize];
        }
    }
    // Up: a soloed track's groups play too (only them, not their other tracks).
    let mut up = [false; MAX_TRACKS];
    for &i in order.iter() {
        if solo[i] || up[i] {
            if let Some(p) = parents[i] {
                up[p as usize] = true;
            }
        }
    }
    std::array::from_fn(|i| on[i] || up[i])
}

impl Engine {
    pub fn new(
        sample_rate: f32,
        commands: Consumer<Command>,
        live: Consumer<LiveNote>,
        recorded: Producer<RecordedNote>,
        garbage: Producer<Garbage>,
    ) -> (Self, Arc<SharedState>) {
        let shared = Arc::new(SharedState::new());
        let engine = Self {
            commands,
            live,
            recorded,
            garbage,
            shared: shared.clone(),
            sample_rate,
            tracks: (0..MAX_TRACKS).map(|_| Track::new(sample_rate)).collect(),
            returns: (0..NUM_RETURNS).map(|_| Channel::new()).collect(),
            master: Channel::new(),
            playing: false,
            position: 0.0,
            start_marker: 0.0,
            tempo: 120.0,
            quantize: 4.0,
            loop_enabled: false,
            loop_start: 0.0,
            loop_end: 16.0,
            metronome: false,
            count_in_beats: 0.0,
            counting: None,
            click: Click::new(sample_rate),
            click_buffer: vec![0.0; MAX_BLOCK],
            preview: None,
            preview_buffer: vec![0.0; 2 * MAX_BLOCK],
            monitor_input: None,
            monitor_left: vec![0.0; MAX_BLOCK],
            monitor_right: vec![0.0; MAX_BLOCK],
            input_track: None,
            arrangement_record: false,
            limit_output: true,
            swing: (0.25, 0.0),
            parents: [None; MAX_TRACKS],
            order: std::array::from_fn(|i| i),
            bus_left: vec![0.0; MAX_BLOCK],
            bus_right: vec![0.0; MAX_BLOCK],
            ids: [0; MAX_TRACKS],
            key_left: (0..MAX_TRACKS).map(|_| vec![0.0; MAX_BLOCK]).collect(),
            key_right: (0..MAX_TRACKS).map(|_| vec![0.0; MAX_BLOCK]).collect(),
        };
        (engine, shared)
    }

    /// Monitored tracks play this block of the audio input instead of their clips,
    /// resampled to the output's rate if the input runs at another.
    fn monitor(&mut self, n: usize) {
        let (left, right) = (&mut self.monitor_left[..n], &mut self.monitor_right[..n]);
        left.fill(0.0);
        right.fill(0.0);
        let Some(input) = self.monitor_input.as_mut() else { return };
        let channels = input.channels.max(1);
        let ratio = (input.sample_rate / self.sample_rate) as f64;
        let wanted = (n as f64 * ratio).ceil() as usize;
        let queued = input.samples.slots() / channels + input.pending.len();
        // Input that piled up while nothing listened (or the devices drifted apart)
        // would be heard late: keep only a small cushion against callback jitter.
        if queued > wanted + MONITOR_SLACK {
            let mut skip = queued - wanted - MONITOR_CUSHION;
            let from_pending = skip.min(input.pending.len());
            input.pending.drain(..from_pending);
            skip -= from_pending;
            if let Ok(chunk) = input.samples.read_chunk(skip * channels) {
                chunk.commit_all();
            }
        }
        if !self.tracks.iter().any(|t| t.monitor) {
            return;
        }
        // Frames this block reads: up to the one after its last position.
        let needed = (input.position + n as f64 * ratio).floor() as usize + 2;
        let missing = needed.saturating_sub(input.pending.len()).min(input.pending.capacity() - input.pending.len());
        let take = missing.min(input.samples.slots() / channels);
        if let Ok(chunk) = input.samples.read_chunk(take * channels) {
            let (a, b) = chunk.as_slices();
            let mut frame = (0.0, 0.0);
            for (k, &value) in a.iter().chain(b).enumerate() {
                match k % channels {
                    0 => frame = (value, value),
                    1 => frame.1 = value,
                    _ => {}
                }
                if k % channels == channels - 1 {
                    input.pending.push(frame);
                }
            }
            chunk.commit_all();
        }
        let pending = &input.pending;
        for i in 0..n {
            let x = input.position + i as f64 * ratio;
            let j = x as usize;
            let t = (x - j as f64) as f32;
            let (Some(&(l0, r0)), Some(&(l1, r1))) = (pending.get(j), pending.get(j + 1).or(pending.get(j))) else { break };
            left[i] = l0 + (l1 - l0) * t;
            right[i] = r0 + (r1 - r0) * t;
        }
        let advanced = input.position + n as f64 * ratio;
        let used = (advanced.floor() as usize).min(input.pending.len());
        input.pending.drain(..used);
        input.position = if used < advanced.floor() as usize { 0.0 } else { advanced - used as f64 };
        for track in self.tracks.iter_mut().filter(|t| t.monitor) {
            track.audio_left[..n].copy_from_slice(left);
            track.audio_right[..n].copy_from_slice(right);
        }
    }

    pub fn shared(&self) -> &SharedState {
        &self.shared
    }

    /// Hands something back to the UI thread so it is freed there.
    fn discard(&mut self, garbage: Garbage) {
        // If the queue is full it is freed here; rare, and only costs a free().
        let _ = self.garbage.push(garbage);
    }

    fn channel(&mut self, target: Target) -> Option<&mut Channel> {
        match target {
            Target::Track(i) => self.tracks.get_mut(i).map(|t| &mut t.channel),
            Target::Return(i) => self.returns.get_mut(i),
            Target::Master => Some(&mut self.master),
        }
    }

    fn start_transport(&mut self) {
        // A count-in in progress starts it when it ends.
        if !self.playing && self.counting.is_none() {
            self.playing = true;
            self.position = self.start_marker;
        }
    }

    /// Starts the transport for a recording: after the count-in, if there is one.
    fn start_recording_transport(&mut self) {
        if self.playing || self.counting.is_some() {
            return;
        }
        if self.count_in_beats > 0.0 {
            self.counting = Some(0.0);
        } else {
            self.start_transport();
        }
    }

    /// The count-in's clicks in a block of `n` frames; the transport starts right
    /// after it (`process` ends the block where the count-in does).
    fn count_in(&mut self, n: usize, beats_per_sample: f64) {
        let Some(done) = self.counting else { return };
        let end = done + n as f64 * beats_per_sample;
        let beat = (done - 1e-9).ceil();
        if beat < end && beat < self.count_in_beats - 1e-9 {
            let frame = ((beat - done) / beats_per_sample).round() as usize;
            self.click.trigger((beat as i64).rem_euclid(4) == 0, frame.min(n - 1));
        }
        if end >= self.count_in_beats - 1e-9 {
            self.counting = None;
            self.start_transport();
        } else {
            self.counting = Some(end);
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Play => self.start_transport(),
            Command::Stop => {
                let end = self.position;
                for (index, track) in self.tracks.iter_mut().enumerate() {
                    flush_held(track, index, end, &mut self.recorded);
                    track.recording = None;
                    track.stop_session_clip();
                }
                self.arrangement_record = false;
                self.playing = false;
                self.counting = None;
                self.position = self.start_marker;
            }
            Command::SetPosition(beat) => {
                self.start_marker = beat.max(0.0);
                self.position = self.start_marker;
                for track in &mut self.tracks {
                    push_event(&mut track.events, 0, Event::AllNotesOff);
                }
            }
            Command::SetTempo(bpm) => self.tempo = bpm.clamp(20.0, 999.0),
            Command::SetMetronome(on) => self.metronome = on,
            Command::SetQuantize(beats) => self.quantize = beats.max(0.0),
            Command::SetLoop { enabled, start, end } => {
                self.loop_enabled = enabled && end > start;
                self.loop_start = start.max(0.0);
                self.loop_end = end;
            }
            Command::SetClip { track, slot, clip } => {
                let Some(t) = self.tracks.get_mut(track) else { return };
                if slot >= NUM_SCENES {
                    return;
                }
                let removed = clip.is_none();
                let old = std::mem::replace(&mut t.slots[slot], clip);
                if removed && t.playing == Some(slot) {
                    t.playing = None;
                    t.instrument.all_notes_off();
                }
                if removed && t.queued == Queued::Slot(slot) {
                    t.queued = Queued::Nothing;
                }
                if let Some(old) = old {
                    self.discard(Garbage::Clip(old));
                }
            }
            Command::LaunchClip { track, slot } => {
                let Some(t) = self.tracks.get_mut(track) else { return };
                if slot >= NUM_SCENES {
                    return;
                }
                // Launching an empty slot acts as that track's stop button.
                t.queued = if t.slots[slot].is_some() {
                    Queued::Slot(slot)
                } else {
                    Queued::Stop
                };
                if t.queued != Queued::Stop {
                    self.start_transport();
                }
            }
            Command::StopTrack(track) => {
                if let Some(t) = self.tracks.get_mut(track) {
                    t.queued = Queued::Stop;
                }
            }
            Command::ReorderTracks(order) => {
                if !is_permutation(&order[..]) {
                    return;
                }
                // Follow each cycle of the permutation with swaps: nothing allocates.
                let mut done = [false; MAX_TRACKS];
                for start in 0..MAX_TRACKS {
                    let mut i = start;
                    while !done[i] {
                        done[i] = true;
                        let from = order[i];
                        if done[from] {
                            break;
                        }
                        self.tracks.swap(i, from);
                        i = from;
                    }
                }
                self.input_track = self.input_track.and_then(|t| order.iter().position(|&o| o == t));
                // The groups follow their tracks.
                let mut moved_to = [0usize; MAX_TRACKS];
                for (new, &old) in order.iter().enumerate() {
                    moved_to[old] = new;
                }
                let old = self.parents;
                self.parents = std::array::from_fn(|i| old[order[i]].map(|p| moved_to[p as usize] as u8));
                let ids = self.ids;
                self.ids = std::array::from_fn(|i| ids[order[i]]);
            }
            Command::SetGroups(parents) => {
                // Only groups that come before their tracks, so nothing loops.
                let valid = parents.iter().enumerate().all(|(i, p)| p.is_none_or(|p| (p as usize) < i));
                if valid {
                    self.parents = parents;
                }
            }
            Command::SetTrackIds(ids) => self.ids = ids,
            Command::SetMidiEffects { track, devices } => {
                if let Some(t) = self.tracks.get_mut(track) {
                    t.midi_fx.set(devices);
                }
            }
            Command::LaunchScene(slot) => {
                if slot >= NUM_SCENES {
                    return;
                }
                for t in &mut self.tracks {
                    t.queued = if t.slots[slot].is_some() {
                        Queued::Slot(slot)
                    } else {
                        Queued::Stop
                    };
                }
                self.start_transport();
            }
            Command::StopAllClips => {
                for t in &mut self.tracks {
                    t.queued = Queued::Stop;
                }
            }
            Command::BackToArrangement => {
                for t in &mut self.tracks {
                    if t.playing.is_some() {
                        t.switch_beat = self.position;
                    }
                    t.stop_session_clip();
                    t.follows_arrangement = true;
                }
            }
            Command::SetArrangement { track, mut clips } => {
                let Some(t) = self.tracks.get_mut(track) else { return };
                std::mem::swap(&mut t.arrangement, &mut clips);
                if t.playing.is_none() {
                    // Edits under the playhead must not leave hanging notes.
                    t.instrument.all_notes_off();
                }
                self.discard(Garbage::Arrangement(clips));
            }
            Command::SetInstrument { track, mut instrument } => {
                self.forget_held(track, |p| matches!(p, Param::Instrument(_)));
                let Some(t) = self.tracks.get_mut(track) else { return };
                // A new instance is taken whenever one was built for a reason: a
                // plugin's (the UI's handle controls that one, not the running
                // one) or another layout of the same synth (an effect added inside
                // a Wavetable). Otherwise sounding voices are kept and only the
                // parameters change.
                let rebuilt = match &instrument.built {
                    Some(Instrument::Plugin(..)) => true,
                    Some(_) => t.instrument.kind().needs_rebuild(&instrument.kind),
                    None => false,
                };
                if rebuilt || !t.instrument.update(instrument.kind) {
                    let built = match instrument.built.take() {
                        Some(built) => built,
                        // Only reached if a caller skipped `set_instrument`.
                        None => Instrument::new(instrument.kind, self.sample_rate),
                    };
                    // The old instrument leaves in the box, to be freed off this thread.
                    instrument.built = Some(std::mem::replace(&mut t.instrument, built));
                }
                self.discard(Garbage::Instrument(instrument));
            }
            Command::SetWavetables { track, mut tables } => {
                let Some(t) = self.tracks.get_mut(track) else { return };
                std::mem::swap(&mut t.tables, &mut tables);
                self.discard(Garbage::Wavetables(tables));
            }
            Command::SetSamples { track, mut samples } => {
                let Some(t) = self.tracks.get_mut(track) else { return };
                std::mem::swap(&mut t.samples, &mut samples);
                self.discard(Garbage::Samples(samples));
            }
            Command::SetGain { target, gain } => {
                if let Some(c) = self.channel(target) {
                    c.gain = gain.max(0.0);
                    c.base_gain = c.gain;
                }
            }
            Command::SetPan { target, pan } => {
                if let Some(c) = self.channel(target) {
                    c.pan = pan.clamp(-1.0, 1.0);
                    c.base_pan = c.pan;
                }
            }
            Command::SetMute { target, mute } => {
                if let Some(c) = self.channel(target) {
                    c.mute = mute;
                }
            }
            Command::SetSolo { track, solo } => {
                if let Some(t) = self.tracks.get_mut(track) {
                    t.solo = solo;
                }
            }
            Command::SetSend { track, send, amount } => {
                if let Some(t) = self.tracks.get_mut(track) {
                    if let (Some(s), Some(base)) = (t.sends.get(send).copied(), t.base_sends.get_mut(send)) {
                        let _ = s;
                        *base = amount.clamp(0.0, 1.0);
                        t.sends[send] = *base;
                    }
                }
            }
            Command::InsertDevice { target, index, slot } => {
                let rejected = match self.channel(target) {
                    Some(c) if c.chain.len() < c.chain.capacity() => {
                        let index = index.min(c.chain.len());
                        c.chain.insert(index, slot);
                        None
                    }
                    _ => Some(slot),
                };
                if let Some(slot) = rejected {
                    self.discard(Garbage::Device(slot));
                }
            }
            Command::RemoveDevice { target, index } => {
                let removed = self
                    .channel(target)
                    .filter(|c| index < c.chain.len())
                    .map(|c| c.chain.remove(index));
                if let Some(slot) = removed {
                    self.discard(Garbage::Device(slot));
                }
            }
            Command::UpdateDevice { target, index, device } => {
                // New settings: what a clip's envelope comes back to.
                if let Target::Track(t) = target {
                    self.forget_held(t, |p| matches!(p, Param::Device { device, .. } if device == index));
                }
                if let Some(slot) = self.channel(target).and_then(|c| c.chain.get_mut(index)) {
                    slot.enabled = device.enabled;
                    slot.processor.set_params(&device.effect);
                }
            }
            Command::ReplaceChain { target, mut chain } => {
                if let Target::Track(t) = target {
                    self.forget_held(t, |p| matches!(p, Param::Device { .. }));
                }
                let Some(c) = self.channel(target) else { return };
                std::mem::swap(&mut c.chain, &mut chain);
                self.discard(Garbage::Chain(chain));
            }
            Command::SetAutomation { target, mut lanes } => {
                let Some(c) = self.channel(target) else { return };
                std::mem::swap(&mut c.automation, &mut lanes);
                self.discard(Garbage::Automation(lanes));
            }
            Command::SetOutputLimit(on) => self.limit_output = on,
            Command::SetSwing { amount, grid } => self.swing = (grid.max(1.0 / 64.0), amount.clamp(0.0, 1.0)),
            Command::SetArm { track, armed } => {
                let end = self.position;
                if let Some(t) = self.tracks.get_mut(track) {
                    t.armed = armed;
                    if !armed && t.recording.is_none() {
                        flush_held(t, track, end, &mut self.recorded);
                    }
                }
            }
            Command::SetInputTrack(track) => self.input_track = track,
            Command::SetMonitorInput(mut input) => {
                std::mem::swap(&mut self.monitor_input, &mut input);
                if let Some(old) = input {
                    self.discard(Garbage::Monitor(old));
                }
            }
            Command::SetMonitor { track, on } => {
                if let Some(t) = self.tracks.get_mut(track) {
                    t.monitor = on;
                }
            }
            Command::RecordClip { track, slot } => {
                if let Some(t) = self.tracks.get_mut(track) {
                    if slot < NUM_SCENES && t.slots[slot].is_none() {
                        t.queued = Queued::Record(slot);
                        self.start_recording_transport();
                    }
                }
            }
            Command::StopRecording(track) => {
                let end = self.position;
                if let Some(t) = self.tracks.get_mut(track) {
                    if let Some(slot) = t.recording.take() {
                        flush_held(t, track, end, &mut self.recorded);
                        // Keep looping from where the take began once its clip arrives.
                        t.playing = Some(slot);
                        t.launch_beat = t.record_start;
                    }
                }
            }
            Command::SetArrangementRecord(on) => {
                if !on {
                    let end = self.position;
                    for (index, t) in self.tracks.iter_mut().enumerate() {
                        if t.recording.is_none() {
                            flush_held(t, index, end, &mut self.recorded);
                        }
                    }
                }
                self.arrangement_record = on;
                if on {
                    self.start_recording_transport();
                }
            }
            Command::SetCountIn(beats) => self.count_in_beats = beats.max(0.0),
            Command::PreviewSample(sample) => {
                if let Some((old, _)) = self.preview.take() {
                    self.discard(Garbage::Samples(Box::new(vec![old])));
                }
                self.preview = sample.map(|s| (s, 0.0));
            }
            Command::PreviewNote {
                track,
                pitch,
                velocity,
            } => {
                let gate = (0.3 * self.sample_rate) as u32;
                if let Some(t) = self.tracks.get_mut(track) {
                    push_event(&mut t.events, 0, Event::NoteOn { pitch, velocity, gate });
                }
            }
        }
    }

    /// Plays a live note on the armed tracks (or the input track if none is armed)
    /// and captures it on tracks that are recording.
    fn handle_live(&mut self, note: LiveNote) {
        let any_armed = self.tracks.iter().any(|t| t.armed);
        let input = self.input_track;
        let beat = self.position;
        let arrangement_record = self.playing && self.arrangement_record;
        let pitch = note.pitch.min(127);
        for (index, track) in self.tracks.iter_mut().enumerate() {
            let routed = if any_armed { track.armed } else { input == Some(index) };
            if !routed {
                continue;
            }
            if let Some(controller) = note.controller {
                push_event(&mut track.events, 0, Event::Control { controller, pitch, value: note.velocity });
                continue;
            }
            if note.velocity > 0.0 {
                push_event(&mut track.events, 0, Event::NoteOn { pitch, velocity: note.velocity, gate: HELD });
                let capturing = self.playing && (track.recording.is_some() || (arrangement_record && track.armed));
                if capturing {
                    track.held[pitch as usize] = Some((beat, note.velocity));
                }
            } else {
                push_event(&mut track.events, 0, Event::NoteOff { pitch });
                if let Some((start, velocity)) = track.held[pitch as usize].take() {
                    let length = (beat - start).max(0.05);
                    let _ = self.recorded.push(RecordedNote { track: index, pitch, velocity, start, length });
                }
            }
        }
    }

    /// Renders interleaved frames into `out`.
    pub fn process(&mut self, out: &mut [f32], channels: usize) {
        for t in &mut self.tracks {
            t.events.clear();
        }
        while let Ok(command) = self.commands.pop() {
            self.handle(command);
        }
        while let Ok(note) = self.live.pop() {
            self.handle_live(note);
        }

        let frames = out.len() / channels;
        let mut done = 0;
        while done < frames {
            let mut n = (frames - done).min(MAX_BLOCK);
            // A count-in ends a block where it ends, so the transport starts on time.
            if let Some(counted) = self.counting {
                let bps = self.tempo / 60.0 / self.sample_rate as f64;
                let left = ((self.count_in_beats - counted) / bps).ceil() as usize;
                n = n.min(left.max(1));
            }
            self.render_block(n);
            let (left, right) = (&self.master.left, &self.master.right);
            for i in 0..n {
                let frame = &mut out[(done + i) * channels..(done + i + 1) * channels];
                match channels {
                    1 => frame[0] = 0.5 * (left[i] + right[i]),
                    _ => {
                        frame[0] = left[i];
                        frame[1] = right[i];
                        frame[2..].fill(0.0);
                    }
                }
            }
            done += n;
        }
        self.publish();
    }

    fn render_block(&mut self, n: usize) {
        let beats_per_sample = self.tempo / 60.0 / self.sample_rate as f64;
        let ctx = ProcessContext {
            sample_rate: self.sample_rate,
            tempo: self.tempo,
            beat: self.position,
            beats_per_sample: if self.playing { beats_per_sample } else { 0.0 },
        };
        for track in &mut self.tracks {
            track.audio_left[..n].fill(0.0);
            track.audio_right[..n].fill(0.0);
        }
        // A transport the count-in starts at the end of this block plays from the next.
        let playing = self.playing;
        self.count_in(n, beats_per_sample);
        if playing {
            self.apply_automation();
            self.sequence(n, beats_per_sample);
        }
        self.monitor(n);

        for channel in self.returns.iter_mut().chain([&mut self.master]) {
            channel.left[..n].fill(0.0);
            channel.right[..n].fill(0.0);
        }

        // Which tracks each listens to (sidechains), to process those first.
        let mut wants = [0u64; MAX_TRACKS];
        for (i, track) in self.tracks.iter().enumerate() {
            for id in track.channel.chain.iter().filter_map(|slot| slot.processor.sidechain()) {
                if let Some(source) = self.ids.iter().position(|&x| x == id && id != 0) {
                    wants[i] |= 1 << source;
                }
            }
        }
        self.order = processing_order(&self.parents, &wants);
        let any_solo = self.tracks.iter().any(|t| t.solo);
        let solo: [bool; MAX_TRACKS] = std::array::from_fn(|i| self.tracks[i].solo);
        let soloed = soloed(&solo, &self.parents, &self.order);
        // A track in a muted group (at any depth) is silent, its sends too: muting
        // a bus silences all of it.
        let silenced: [bool; MAX_TRACKS] = std::array::from_fn(|i| {
            let mut parent = self.parents[i];
            let mut depth = 0;
            while let Some(group) = parent.filter(|_| depth < MAX_TRACKS) {
                if self.tracks[group as usize].channel.mute {
                    return true;
                }
                parent = self.parents[group as usize];
                depth += 1;
            }
            false
        });
        let sr = self.sample_rate;
        for k in 0..MAX_TRACKS {
            let index = self.order[k];
            let track = &mut self.tracks[index];
            let (left, right) = (&mut track.buffer[..n], &mut track.buffer_right[..n]);
            left.fill(0.0);
            right.fill(0.0);
            track.events.sort_unstable_by_key(|&(frame, rank, _)| (frame, rank));
            if !track.midi_fx.is_empty() {
                let timing = crate::midi_fx::Timing { sample_rate: sr, tempo: self.tempo, beat: ctx.beat, beats_per_sample: ctx.beats_per_sample };
                track.midi_fx.process(&mut track.events, n, timing);
            }
            track.instrument.set_tempo(self.tempo);
            let mut cursor = 0;
            for &(frame, _, event) in &track.events {
                let frame = frame.min(n - 1);
                if frame > cursor {
                    track.expression.render(&mut track.instrument, sr, &track.samples, &track.tables, &mut left[cursor..frame], &mut right[cursor..frame]);
                    cursor = frame;
                }
                match event {
                    Event::AllNotesOff => track.instrument.all_notes_off(),
                    Event::NoteOff { pitch } => track.instrument.note_off(pitch),
                    Event::NoteOn { pitch, velocity, gate } => track.instrument.note_on(&track.samples, pitch, velocity, gate),
                    Event::Control { controller, pitch, value } => {
                        track.expression.control(controller, value);
                        track.instrument.control(controller, pitch, value);
                    }
                }
            }
            track.expression.render(&mut track.instrument, sr, &track.samples, &track.tables, &mut left[cursor..], &mut right[cursor..]);
            track.events.clear();

            let channel = &mut track.channel;
            for i in 0..n {
                channel.left[i] = left[i] + track.audio_left[i];
                channel.right[i] = right[i] + track.audio_right[i];
            }
            feed_keys(&mut channel.chain, &self.ids, &self.key_left, &self.key_right, n);
            if !channel.run_chain(n, &ctx) {
                self.shared.fault(Fault::Track(index));
            }
            // What sidechains listening to it hear: after its effects, before its fader.
            self.key_left[index][..n].copy_from_slice(&channel.left[..n]);
            self.key_right[index][..n].copy_from_slice(&channel.right[..n]);

            let audible = !channel.mute && !silenced[index] && (!any_solo || soloed[index]);
            let gain = if audible { channel.gain } else { 0.0 };
            // Post-fader sends, taken before panning.
            for (ret, &amount) in self.returns.iter_mut().zip(&track.sends) {
                if amount > 0.0 {
                    for i in 0..n {
                        ret.left[i] += channel.left[i] * gain * amount;
                        ret.right[i] += channel.right[i] * gain * amount;
                    }
                }
            }
            // Into its group (which comes later in the order), or the master.
            match self.parents[index] {
                Some(group) => {
                    self.bus_left[..n].fill(0.0);
                    self.bus_right[..n].fill(0.0);
                    let peak = channel.mix_into(n, gain, &mut self.bus_left, &mut self.bus_right);
                    self.shared.tracks[index].peak.publish(peak);
                    let into = &mut self.tracks[group as usize];
                    for i in 0..n {
                        into.audio_left[i] += self.bus_left[i];
                        into.audio_right[i] += self.bus_right[i];
                    }
                }
                None => {
                    let peak = channel.mix_into(n, gain, &mut self.master.left, &mut self.master.right);
                    self.shared.tracks[index].peak.publish(peak);
                }
            }
        }

        for (index, ret) in self.returns.iter_mut().enumerate() {
            feed_keys(&mut ret.chain, &self.ids, &self.key_left, &self.key_right, n);
            if !ret.run_chain(n, &ctx) {
                self.shared.fault(Fault::Return(index));
            }
            let gain = if ret.mute { 0.0 } else { ret.gain };
            let peak = ret.mix_into(n, gain, &mut self.master.left, &mut self.master.right);
            self.shared.returns[index].publish(peak);
        }

        // The browser's preview, at the file's own speed, after the master's
        // settings (it is a preview, not part of the song).
        let preview = &mut self.preview_buffer[..2 * n];
        preview.fill(0.0);
        let mut finished = false;
        if let Some((sample, position)) = &mut self.preview {
            let step = sample.sample_rate as f64 / self.sample_rate as f64;
            for i in 0..n {
                let (l, r) = sample.read(*position);
                preview[2 * i] = l * PREVIEW_GAIN;
                preview[2 * i + 1] = r * PREVIEW_GAIN;
                *position += step;
            }
            finished = *position >= sample.frames() as f64;
        }
        if finished {
            if let Some((old, _)) = self.preview.take() {
                self.discard(Garbage::Samples(Box::new(vec![old])));
            }
        }

        let master = &mut self.master;
        feed_keys(&mut master.chain, &self.ids, &self.key_left, &self.key_right, n);
        if !master.run_chain(n, &ctx) {
            self.shared.fault(Fault::Master);
        }
        let (gl, gr) = master.pan_gains(master.gain);
        let (l0, r0) = master.last_gains.unwrap_or((gl, gr));
        master.last_gains = Some((gl, gr));
        let click = &mut self.click_buffer[..n];
        click.fill(0.0);
        self.click.render(click);
        let preview = &self.preview_buffer;
        let mut peak = 0f32;
        for i in 0..n {
            let t = (i + 1) as f32 / n as f32;
            let l = master.left[i] * (l0 + (gl - l0) * t) + click[i] + preview[2 * i];
            let r = master.right[i] * (r0 + (gr - r0) * t) + click[i] + preview[2 * i + 1];
            peak = peak.max(l.abs()).max(r.abs());
            // Safety limit only; the meter shows the real level.
            let (l, r) = if self.limit_output { (l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0)) } else { (l, r) };
            master.left[i] = l;
            master.right[i] = r;
        }
        self.shared.master.publish(peak);
    }

    /// Sets automated parameters to their values at the current position.
    fn apply_automation(&mut self) {
        let beat = self.position;
        for track in &mut self.tracks {
            let Track { channel, sends, base_sends, envelope_held: held, instrument, slots, playing, launch_beat, follows_arrangement, arrangement, .. } = track;
            // The sounding clip's envelopes, in its own time (looping with it), win
            // over the track's automation.
            let clip = if *follows_arrangement {
                arrangement.iter().find(|c| beat >= c.start && beat < c.end()).map(|c| (&c.clip, beat - c.start))
            } else {
                playing.and_then(|p| slots[p].as_deref()).map(|c| (c, beat - *launch_beat))
            };
            let envelopes = clip.and_then(|(clip, since)| {
                let length = clip.length();
                (length > 0.0 && !clip.envelopes().is_empty()).then(|| (clip.envelopes(), since.rem_euclid(length)))
            });
            // What the envelopes take over, as it was before them.
            let mut touched = [false; MAX_HELD];
            for lane in envelopes.iter().flat_map(|(lanes, at)| lanes.iter().filter(move |l| l.value_at(*at).is_some())) {
                let before = match lane.param {
                    Param::Device { device, param } => channel.chain.get(device).filter(|s| !matches!(*s.processor, EffectProcessor::Plugin(..))).map(|s| s.processor.effect().get(param)),
                    Param::Instrument(i) if !matches!(instrument, Instrument::Plugin(..)) => Some(instrument.kind().get(i)),
                    _ => None,
                };
                let Some(before) = before else { continue };
                let slot = held.iter().position(|h| h.is_some_and(|(p, _)| p == lane.param)).or_else(|| {
                    let free = held.iter().position(|h| h.is_none())?;
                    held[free] = Some((lane.param, before));
                    Some(free)
                });
                if let Some(slot) = slot {
                    touched[slot] = true;
                }
            }
            *sends = *base_sends;
            let mut other = |param: Param, value: f32| match param {
                Param::Send(i) => {
                    if let Some(send) = sends.get_mut(i) {
                        *send = value.clamp(0.0, 1.0);
                    }
                }
                Param::Instrument(i) => instrument.set_param(i, value),
                _ => {}
            };
            channel.automate(beat, &mut other);
            if let Some((lanes, at)) = envelopes {
                channel.automate_with(lanes, at, &mut other);
            }
            // Clip over: what its envelopes held comes back.
            for (slot, touched) in held.iter_mut().zip(touched) {
                if let (Some((param, value)), false) = (*slot, touched) {
                    Channel::set(param, value, &mut channel.gain, &mut channel.pan, &mut channel.chain, &mut other);
                    *slot = None;
                }
            }
        }
        for channel in self.returns.iter_mut() {
            channel.automate(beat, |_, _| {});
        }
        // The master's lanes can hold the tempo.
        let tempo = &mut self.tempo;
        self.master.automate(beat, |param, value| {
            if param == Param::Tempo {
                *tempo = (value as f64).clamp(20.0, 999.0);
            }
        });
    }

    /// Lets go of held parameters of `track` matching `which` (their settings
    /// changed: the new ones are what an envelope comes back to).
    fn forget_held(&mut self, track: usize, which: impl Fn(Param) -> bool) {
        if let Some(t) = self.tracks.get_mut(track) {
            for slot in &mut t.envelope_held {
                if slot.is_some_and(|(p, _)| which(p)) {
                    *slot = None;
                }
            }
        }
    }

    /// Advances the transport over `n` frames, wrapping at the loop end.
    fn sequence(&mut self, n: usize, beats_per_sample: f64) {
        let mut frame = 0;
        while frame < n {
            let remaining = n - frame;
            let start = self.position;
            let wraps = self.loop_enabled
                && start < self.loop_end
                && start + remaining as f64 * beats_per_sample > self.loop_end;
            let (frames, end) = if wraps {
                let frames = (((self.loop_end - start) / beats_per_sample).ceil() as usize).clamp(1, remaining);
                (frames, self.loop_end)
            } else {
                (remaining, start + remaining as f64 * beats_per_sample)
            };
            let timing = Timing {
                segment_beat: start,
                segment_frame: frame,
                segment_end: frame + frames,
                beats_per_sample,
                frames: n,
                swing: self.swing,
                loop_end: if self.loop_enabled && start < self.loop_end { self.loop_end } else { f64::INFINITY },
            };
            self.sequence_segment(start, end, &timing);
            frame += frames;
            self.position = end;

            if wraps {
                let length = self.loop_end - self.loop_start;
                self.position = self.loop_start;
                // Arrangement notes were cut at the loop's end; notes held on a
                // keyboard keep sounding.
                for track in &mut self.tracks {
                    // Session clips keep their phase across the jump.
                    track.launch_beat -= length;
                }
            }
        }
    }

    /// Applies quantized launches and schedules the notes in `[start, end)`.
    fn sequence_segment(&mut self, start: f64, end: f64, timing: &Timing) {
        let boundary = if self.quantize <= 0.0 {
            start
        } else {
            // Small epsilon so a launch exactly on a boundary is not pushed a bar later.
            ((start - 1e-9) / self.quantize).ceil() * self.quantize
        };

        let segment = (timing.segment_frame, timing.segment_end);
        let bps = timing.beats_per_sample;
        let tempo = self.tempo;
        for (index, track) in self.tracks.iter_mut().enumerate() {
            if track.queued == Queued::Nothing || boundary >= end {
                track.collect_notes(start, end, timing);
                track.render_audio(segment, start, bps, tempo);
                continue;
            }
            track.collect_notes(start, boundary, timing);
            let switch = (segment.0 + ((boundary - start) / bps).ceil() as usize).clamp(segment.0, segment.1);
            track.render_audio((segment.0, switch), start, bps, tempo);
            // Any switch on a track ends the take it was recording.
            if track.recording.take().is_some() {
                flush_held(track, index, boundary, &mut self.recorded);
            }
            track.switch_beat = boundary;
            match track.queued {
                Queued::Nothing => {}
                Queued::Stop => track.playing = None,
                Queued::Slot(slot) => {
                    track.playing = Some(slot);
                    track.launch_beat = boundary;
                }
                Queued::Record(slot) => {
                    track.playing = None;
                    track.recording = Some(slot);
                    track.record_start = boundary;
                }
            }
            // Session activity on a track overrides its arrangement until "Back to Arrangement".
            track.follows_arrangement = false;
            track.queued = Queued::Nothing;
            push_event(&mut track.events, timing.frame(boundary), Event::AllNotesOff);
            track.collect_notes(boundary, end, timing);
            let switch_beat = start + (switch - segment.0) as f64 * bps;
            track.render_audio((switch, segment.1), switch_beat, bps, tempo);
        }

        if self.metronome {
            let beat = (start - 1e-9).ceil();
            if beat < end {
                self.click.trigger((beat as i64).rem_euclid(4) == 0, timing.frame(beat));
            }
        }
    }

    fn publish(&self) {
        let s = &self.shared;
        s.playing.store(self.playing, Ordering::Relaxed);
        s.arrangement_recording.store(self.arrangement_record, Ordering::Relaxed);
        s.count_in.store(self.counting.map_or(0.0, |done| (self.count_in_beats - done).max(0.0)));
        s.position.store(self.position);
        for (track, status) in self.tracks.iter().zip(&s.tracks) {
            let queued = match track.queued {
                Queued::Nothing => NONE,
                Queued::Stop => QUEUED_STOP,
                Queued::Slot(slot) | Queued::Record(slot) => slot as i32,
            };
            status.queued_record.store(matches!(track.queued, Queued::Record(_)), Ordering::Relaxed);
            status.recording.store(track.recording.map_or(NONE, |r| r as i32), Ordering::Relaxed);
            status.record_start.store(track.record_start);
            status.playing.store(track.playing.map_or(NONE, |p| p as i32), Ordering::Relaxed);
            status.queued.store(queued, Ordering::Relaxed);
            status.follows_arrangement.store(track.follows_arrangement, Ordering::Relaxed);
            let clip_position = match track.playing.and_then(|p| track.slots[p].as_ref()) {
                Some(clip) if clip.length() > 0.0 => (self.position - track.launch_beat).rem_euclid(clip.length()),
                _ => 0.0,
            };
            status.clip_position.store(clip_position);
            status.launch_beat.store(track.launch_beat);
            status.switch_beat.store(track.switch_beat);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AudioClip, Device, Effect, InstrumentKind, MidiClip, Note, WarpMarker, WarpMode};
    use crate::model::AutomationLane;
    use rtrb::RingBuffer;

    const SR: f32 = 48_000.0;

    struct Harness {
        engine: Engine,
        commands: Producer<Command>,
        live: Producer<LiveNote>,
        recorded: Consumer<RecordedNote>,
        shared: Arc<SharedState>,
        _garbage: Consumer<Garbage>,
    }

    impl Harness {
        fn new() -> Self {
            let (commands, rx) = RingBuffer::new(64);
            let (live, live_rx) = RingBuffer::new(64);
            let (recorded_tx, recorded) = RingBuffer::new(64);
            let (gtx, grx) = RingBuffer::new(64);
            let (engine, shared) = Engine::new(SR, rx, live_rx, recorded_tx, gtx);
            Self { engine, commands, live, recorded, shared, _garbage: grx }
        }

        fn send(&mut self, c: Command) {
            assert!(self.commands.push(c).is_ok());
        }

        fn play_note(&mut self, pitch: u8, velocity: f32) {
            assert!(self.live.push(LiveNote::note(pitch, velocity)).is_ok());
        }

        fn recorded(&mut self) -> Vec<RecordedNote> {
            std::iter::from_fn(|| self.recorded.pop().ok()).collect()
        }

        /// Renders `beats` worth of stereo audio at 120 BPM and returns it.
        fn run_beats(&mut self, beats: f64) -> Vec<f32> {
            let frames = (beats * 0.5 * SR as f64) as usize;
            let mut out = vec![0.0; frames * 2];
            for chunk in out.chunks_mut(512 * 2) {
                self.engine.process(chunk, 2);
            }
            out
        }

        fn playing(&self, track: usize) -> i32 {
            self.shared.tracks[track].playing.load(Ordering::Relaxed)
        }
    }

    fn one_note_clip() -> Box<PlayClip> {
        let mut clip = MidiClip::new("test", 4.0, [0, 0, 0]);
        clip.notes.push(Note { start: 0.0, length: 1.0, pitch: 60, velocity: 1.0 });
        Box::new(PlayClip::Midi(clip))
    }

    fn energy(samples: &[f32]) -> f32 {
        samples.iter().map(|s| s * s).sum()
    }

    #[test]
    fn launching_a_clip_starts_the_transport_and_plays_notes() {
        let mut h = Harness::new();
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        let audio = h.run_beats(1.0);
        assert!(h.shared.playing.load(Ordering::Relaxed));
        assert_eq!(h.playing(0), 0);
        assert!(energy(&audio) > 1.0, "clip should be audible");
    }

    #[test]
    fn reordering_tracks_moves_what_they_play() {
        let mut h = Harness::new();
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::SetClip { track: 2, slot: 1, clip: Some(one_note_clip()) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        h.send(Command::LaunchClip { track: 2, slot: 1 });
        h.send(Command::SetInputTrack(Some(2)));
        h.run_beats(1.0);
        // New order: 2, 0, 1, 3, 4…
        let mut order: [usize; MAX_TRACKS] = std::array::from_fn(|i| i);
        order[..3].copy_from_slice(&[2, 0, 1]);
        h.send(Command::ReorderTracks(order));
        h.run_beats(0.5);
        assert_eq!((h.playing(0), h.playing(1), h.playing(2)), (1, 0, NONE));
        assert_eq!(h.engine.input_track, Some(0));
        // Not a permutation: ignored.
        h.send(Command::ReorderTracks([0; MAX_TRACKS]));
        h.run_beats(0.5);
        assert_eq!((h.playing(0), h.playing(1)), (1, 0));
        // A long cycle and its inverse put everything back.
        let shift: [usize; MAX_TRACKS] = std::array::from_fn(|i| (i + 1) % MAX_TRACKS);
        let back: [usize; MAX_TRACKS] = std::array::from_fn(|i| (i + MAX_TRACKS - 1) % MAX_TRACKS);
        h.send(Command::ReorderTracks(shift));
        h.run_beats(0.25);
        assert_eq!((h.playing(0), h.playing(MAX_TRACKS - 1)), (0, 1));
        h.send(Command::ReorderTracks(back));
        h.run_beats(0.25);
        assert_eq!((h.playing(0), h.playing(1)), (1, 0));
    }

    /// How long the engine takes for 10 s of audio with one clip playing (the
    /// other tracks idle), to see what each possible track costs.
    #[test]
    #[ignore = "timing"]
    fn ten_seconds_of_audio() {
        let mut h = Harness::new();
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        let started = std::time::Instant::now();
        h.run_beats(20.0);
        println!("{MAX_TRACKS} pistas: 10 s de audio en {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);
    }

    #[test]
    fn launches_wait_for_the_next_bar() {
        let mut h = Harness::new();
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::SetClip { track: 0, slot: 1, clip: Some(one_note_clip()) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        h.run_beats(1.0);
        h.send(Command::LaunchClip { track: 0, slot: 1 });
        h.run_beats(2.0); // now at beat 3: still in the first bar
        assert_eq!(h.playing(0), 0);
        assert_eq!(h.shared.tracks[0].queued.load(Ordering::Relaxed), 1);
        h.run_beats(1.5); // past beat 4
        assert_eq!(h.playing(0), 1);
        assert_eq!(h.shared.tracks[0].queued.load(Ordering::Relaxed), NONE);
    }

    #[test]
    fn scene_launch_stops_tracks_with_empty_slots() {
        let mut h = Harness::new();
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::SetClip { track: 1, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::SetClip { track: 0, slot: 1, clip: Some(one_note_clip()) });
        h.send(Command::LaunchScene(0));
        h.run_beats(1.0);
        h.send(Command::LaunchScene(1));
        h.run_beats(4.0);
        assert_eq!(h.playing(0), 1);
        assert_eq!(h.playing(1), NONE);
    }

    #[test]
    fn drums_and_clip_loop_keep_playing() {
        let mut h = Harness::new();
        h.send(Command::set_instrument(0, InstrumentKind::Drums, 48_000.0));
        let mut clip = MidiClip::new("beat", 1.0, [0, 0, 0]);
        clip.notes.push(Note { start: 0.0, length: 0.25, pitch: 36, velocity: 1.0 });
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Midi(clip))) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        h.run_beats(8.0);
        let late = h.run_beats(1.0);
        assert!(energy(&late) > 1.0);
        assert!((0.0..1.0).contains(&h.shared.tracks[0].clip_position()));
    }

    #[test]
    fn stop_silences_and_returns_to_the_start_marker() {
        let mut h = Harness::new();
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        h.run_beats(0.5);
        h.send(Command::Stop);
        h.run_beats(1.0); // let the release tail finish
        let tail = h.run_beats(0.5);
        assert!(!h.shared.playing.load(Ordering::Relaxed));
        assert_eq!(h.shared.position(), 0.0);
        assert!(energy(&tail) < 1e-6);
    }

    fn arrangement_with_note_at(beat: f64) -> Box<Vec<PlacedClip>> {
        let mut clip = MidiClip::new("arr", 4.0, [0, 0, 0]);
        clip.notes.push(Note { start: 0.0, length: 1.0, pitch: 60, velocity: 1.0 });
        Box::new(vec![PlacedClip::new(beat, 4.0, PlayClip::Midi(clip))])
    }

    /// One second of a 440 Hz sine at 48 kHz: two beats at 120 BPM.
    fn sine_sample() -> Arc<Sample> {
        let left: Vec<f32> = (0..48_000).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5).collect();
        Arc::new(Sample::from_channels(left.clone(), left, 48_000.0))
    }

    fn audio_clip(warp: WarpMode) -> AudioClip {
        AudioClip {
            envelopes: Vec::new(),
            name: "sine".into(),
            color: [0, 0, 0],
            sample: "sine.wav".into(),
            length: 2.0,
            offset: 0.0,
            warp,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: 0.0,
            transpose: 0.0,
        }
    }

    /// Counts upward zero crossings: a pitch estimate over a known duration.
    fn crossings(samples: &[f32]) -> usize {
        samples.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    }

    fn left_channel(audio: &[f32]) -> Vec<f32> {
        audio.chunks(2).map(|f| f[0]).collect()
    }

    #[test]
    fn live_notes_play_the_input_track_and_stop_on_note_off() {
        let mut h = Harness::new();
        h.send(Command::SetInputTrack(Some(2)));
        h.play_note(60, 0.9);
        h.run_beats(0.5);
        assert!(h.shared.tracks[2].peak.take() > 0.0, "the input track should sound");
        assert_eq!(h.shared.tracks[0].peak.take(), 0.0);
        h.play_note(60, 0.0);
        h.run_beats(1.0); // release tail
        h.shared.tracks[2].peak.take();
        h.run_beats(0.5);
        assert_eq!(h.shared.tracks[2].peak.take(), 0.0, "note-off should silence the held note");
    }

    #[test]
    fn live_controllers_reach_the_modular_synth() {
        use crate::model::{mslot, slot, ModTarget, ModularParams, Modulator, ModulatorKind};
        // CC 1 turns the oscillator down: all the way at 1.
        let mut p = ModularParams::default();
        let mut cc = Modulator::new(ModulatorKind::MidiCc);
        cc.index = 1;
        cc.set(mslot::DEPTH, 1.0);
        cc.targets[0] = ModTarget::new(ModularParams::module_param(1, slot::LEVEL), -1.0);
        p.modulators[0] = cc;
        let mut h = Harness::new();
        h.send(Command::set_instrument(2, InstrumentKind::Modular(p), SR));
        h.send(Command::SetInputTrack(Some(2)));
        h.play_note(60, 0.9);
        h.run_beats(0.5);
        assert!(h.shared.tracks[2].peak.take() > 0.0);
        assert!(h.live.push(LiveNote::control(Controller::Cc(1), 0, 1.0)).is_ok());
        h.run_beats(0.5);
        h.shared.tracks[2].peak.take();
        h.run_beats(0.5);
        assert!(h.shared.tracks[2].peak.take() < 1e-3, "CC 1 silenced it");
    }

    #[test]
    fn plugin_instruments_and_effects_play_in_the_engine() {
        use crate::model::{Device, Effect, PluginId};
        use crate::plugin::{instantiate, scan, tests::test_bundle};
        let found = scan(&test_bundle()).unwrap();
        let (synth, gain) = (&found[0], &found[1]);
        let (processor, _handle) = instantiate(&synth.path, &synth.cid, SR, None).unwrap();
        let mut h = Harness::new();
        h.send(Command::set_plugin(2, PluginId(7), processor));
        h.send(Command::SetInputTrack(Some(2)));
        h.play_note(69, 1.0);
        h.run_beats(0.5);
        let plain = h.shared.tracks[2].peak.take();
        assert!(plain > 0.2, "the plugin plays the live note: {plain}");

        // A plugin effect in the chain: gain 0.5×.
        let (effect, handle) = instantiate(&gain.path, &gain.cid, SR, None).unwrap();
        handle.set(0, 0.25);
        let device = Device::new(Effect::Plugin(PluginId(8)));
        h.send(Command::InsertDevice { target: Target::Track(2), index: 0, slot: ChainSlot::plugin(&device, effect) });
        h.run_beats(0.5);
        h.shared.tracks[2].peak.take();
        h.run_beats(0.5);
        let halved = h.shared.tracks[2].peak.take();
        assert!((halved / plain - 0.5).abs() < 0.02, "{halved} vs {plain}");
    }

    /// The fundamental of `x` (one channel), by autocorrelation between 200 and 1000 Hz.
    fn pitch_of(x: &[f32]) -> f32 {
        let (shortest, longest) = ((SR / 1000.0) as usize, (SR / 200.0) as usize);
        let n = x.len() - longest - 1;
        let correlation = |lag: usize| -> f32 { (0..n).map(|i| x[i] * x[i + lag]).sum() };
        let values: Vec<f32> = (shortest..=longest).map(correlation).collect();
        let max = values.iter().copied().fold(f32::MIN, f32::max);
        // The first peak near the highest: later ones are whole multiples of the period.
        let k = (1..values.len() - 1).find(|&k| values[k] >= 0.9 * max && values[k] >= values[k - 1] && values[k] >= values[k + 1]).unwrap_or(0);
        let best = shortest + k;
        // Parabolic interpolation around the peak.
        let (a, b, c) = (correlation(best - 1), correlation(best), correlation(best + 1));
        let offset = 0.5 * (a - c) / (a - 2.0 * b + c);
        SR / (best as f32 + offset)
    }

    #[test]
    fn pitch_bend_and_the_wheel_bend_and_vibrate_the_built_in_instruments() {
        use crate::model::{AcidParams, FmParams, SuperSawParams, SynthParams, WaveParams};
        let instruments = [
            ("Pulse Synth", InstrumentKind::Synth(SynthParams { cutoff: 8000.0, ..Default::default() })),
            ("Super Saw", InstrumentKind::SuperSaw(SuperSawParams { voices: 1.0, ..Default::default() })),
            ("Acid Bass", InstrumentKind::Acid(AcidParams { cutoff: 3000.0, env_mod: 0.0, resonance: 0.0, ..Default::default() })),
            ("FM Synth", InstrumentKind::Fm(FmParams { index: 0.0, feedback: 0.0, sustain: 1.0, ..Default::default() })),
            ("Wavetable", InstrumentKind::Wavetable(WaveParams::default())),
            ("Polychrome", InstrumentKind::Polychrome(WaveParams::polychrome())),
        ];
        for (name, kind) in instruments {
            let mut h = Harness::new();
            h.send(Command::set_instrument(2, kind, SR));
            let table = Arc::new(crate::wavetable::builtin(0));
            h.send(Command::SetWavetables { track: 2, tables: Box::new(vec![table.clone(), table]) });
            h.send(Command::SetInputTrack(Some(2)));
            h.play_note(69, 1.0);
            let left = |out: &[f32]| out.iter().step_by(2).copied().collect::<Vec<f32>>();
            let plain = pitch_of(&left(&h.run_beats(0.5))[4800..]);
            assert!((plain - 440.0).abs() < 4.0, "{name}: {plain} Hz");
            assert!(h.live.push(LiveNote::control(Controller::PitchBend, 0, 1.0)).is_ok());
            let bent = pitch_of(&left(&h.run_beats(0.5))[2400..]);
            assert!((bent - 493.9).abs() < 4.0, "{name}: bent up to B4, {bent} Hz");
            assert!(h.live.push(LiveNote::control(Controller::PitchBend, 0, -1.0)).is_ok());
            let down = pitch_of(&left(&h.run_beats(0.5))[2400..]);
            assert!((down - 392.0).abs() < 4.0, "{name}: bent down to G4, {down} Hz");
            // Back to the middle, then the wheel all the way: the pitch wavers ±half a semitone.
            assert!(h.live.push(LiveNote::control(Controller::PitchBend, 0, 0.0)).is_ok());
            assert!(h.live.push(LiveNote::control(Controller::Cc(1), 0, 1.0)).is_ok());
            let out = left(&h.run_beats(1.0));
            let pitches: Vec<f32> = out[4800..].chunks(1200).filter(|c| c.len() == 1200).map(pitch_of).collect();
            let (low, high) = pitches.iter().fold((f32::MAX, 0f32), |(lo, hi), &p| (lo.min(p), hi.max(p)));
            assert!(low < 440.0 * 0.985 && high > 440.0 * 1.015, "{name}: vibrato {low}..{high} Hz");
            assert!(high < 440.0 * 1.04 && low > 440.0 * 0.96, "{name}: not more than half a semitone: {low}..{high}");
        }
    }

    #[test]
    fn the_sampler_bends_too() {
        use crate::model::SamplerParams;
        // Five seconds of a C4 sine, played as A4.
        let c4 = 261.63f32;
        let sine: Vec<f32> = (0..240_000).map(|i| (i as f32 * c4 / SR * std::f32::consts::TAU).sin() * 0.5).collect();
        let mut h = Harness::new();
        h.send(Command::set_instrument(2, InstrumentKind::Sampler(SamplerParams::default()), SR));
        h.send(Command::SetSamples { track: 2, samples: Box::new(vec![Arc::new(Sample::from_channels(sine.clone(), sine, SR))]) });
        h.send(Command::SetInputTrack(Some(2)));
        h.play_note(69, 1.0);
        let left = |out: &[f32]| out.iter().step_by(2).copied().collect::<Vec<f32>>();
        let plain = pitch_of(&left(&h.run_beats(0.5))[4800..]);
        assert!((plain - 440.0).abs() < 4.0, "{plain}");
        assert!(h.live.push(LiveNote::control(Controller::PitchBend, 0, 1.0)).is_ok());
        let bent = pitch_of(&left(&h.run_beats(0.5))[2400..]);
        assert!((bent - 493.9).abs() < 4.0, "{bent}");
    }

    #[test]
    fn drums_ignore_pitch_bend() {
        let mut h = Harness::new();
        h.send(Command::set_instrument(2, InstrumentKind::DrumMachine(Default::default()), SR));
        h.send(Command::SetInputTrack(Some(2)));
        h.play_note(36, 1.0);
        let plain = h.run_beats(0.5);
        let mut h2 = Harness::new();
        h2.send(Command::set_instrument(2, InstrumentKind::DrumMachine(Default::default()), SR));
        h2.send(Command::SetInputTrack(Some(2)));
        assert!(h2.live.push(LiveNote::control(Controller::PitchBend, 0, 1.0)).is_ok());
        h2.play_note(36, 1.0);
        assert_eq!(h2.run_beats(0.5), plain);
    }

    /// Audio input as the input callback would deliver it, for monitoring.
    fn monitor_input(channels: usize) -> (Producer<f32>, Box<MonitorInput>) {
        let (tx, rx) = RingBuffer::new(SR as usize * channels);
        (tx, Box::new(MonitorInput::new(rx, channels, SR)))
    }

    #[test]
    fn monitored_tracks_play_the_audio_input_with_little_delay() {
        let mut h = Harness::new();
        let (mut input, monitor) = monitor_input(2);
        h.send(Command::SetMonitorInput(Some(monitor)));
        // Input that piled up before anything monitored is not heard late.
        for _ in 0..10_000 * 2 {
            input.push(0.9).unwrap();
        }
        h.run_beats(0.02);
        assert!(input.slots() > SR as usize * 2 - (MONITOR_CUSHION + 512) * 2, "stale input skipped");
        assert_eq!(energy(&h.run_beats(0.02)), 0.0, "nothing monitors yet");

        h.send(Command::SetMonitor { track: 0, on: true });
        let mut heard = Vec::new();
        for _ in 0..20 {
            for _ in 0..512 {
                input.push(0.25).unwrap();
                input.push(-0.25).unwrap();
            }
            heard.extend(h.run_beats(512.0 / (0.5 * SR as f64)));
        }
        // Once the fader has faded in, left and right come out as they went in.
        let last = &heard[heard.len() - 2..];
        assert!((last[0] - 0.25).abs() < 0.05 && (last[1] + 0.25).abs() < 0.05, "{last:?}");

        h.send(Command::SetMonitor { track: 0, on: false });
        h.run_beats(0.05);
        for _ in 0..512 * 2 {
            input.push(0.25).unwrap();
        }
        assert!(energy(&h.run_beats(0.05)) < 1e-6, "monitoring off: silent");
    }

    #[test]
    fn monitoring_resamples_an_input_at_another_rate() {
        let mut h = Harness::new();
        let (tx, rx) = RingBuffer::new(SR as usize);
        let mut input = tx;
        h.send(Command::SetMonitorInput(Some(Box::new(MonitorInput::new(rx, 1, 44_100.0)))));
        h.send(Command::SetMonitor { track: 0, on: true });
        // A second of a 1 kHz tone from a 44.1 kHz microphone, as its callbacks deliver it.
        let mut heard = Vec::new();
        let mut k = 0usize;
        for _ in 0..(SR as usize / 512) {
            for _ in 0..470 {
                let _ = input.push((std::f64::consts::TAU * 1000.0 * k as f64 / 44_100.0).sin() as f32 * 0.5);
                k += 1;
            }
            heard.extend(h.run_beats(512.0 / (0.5 * SR as f64)).chunks(2).map(|f| f[0]));
        }
        // Past the fade-in, a 1 kHz tone at 48 kHz: 2000 zero crossings a second.
        let tail = &heard[heard.len() / 2..];
        let crossings = tail.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count() as f32;
        let seconds = tail.len() as f32 / SR;
        assert!((crossings / seconds - 2000.0).abs() < 40.0, "{} crossings a second", crossings / seconds);
        assert!(tail.iter().fold(0f32, |m, v| m.max(v.abs())) > 0.4, "the tone comes through");
        let gaps = tail.windows(4).filter(|w| w.iter().all(|v| v.abs() < 0.01)).count();
        assert_eq!(gaps, 0, "no dropouts");
    }

    #[test]
    fn recording_counts_in_before_the_transport_starts() {
        let mut h = Harness::new();
        h.send(Command::SetCountIn(4.0));
        h.send(Command::SetPosition(8.0));
        h.send(Command::SetArm { track: 0, armed: true });
        h.send(Command::SetArrangementRecord(true));
        h.send(Command::Play);
        // One bar at 120 BPM is two seconds: the clicks, and the transport still.
        let count = h.run_beats(3.9);
        assert!(!h.shared.playing.load(Ordering::Relaxed));
        assert_eq!(h.shared.position(), 8.0);
        assert!(h.shared.count_in() > 0.0 && h.shared.count_in() <= 0.2, "{}", h.shared.count_in());
        let clicks = count.chunks(2).map(|f| f[0]).collect::<Vec<_>>();
        let starts = (0..4).filter(|&b| energy(&clicks[b * 24_000..b * 24_000 + 2400]) > 0.5).count();
        assert_eq!(starts, 4, "a click on each beat");
        // Then it plays and records from where it was.
        h.run_beats(0.2);
        assert!(h.shared.playing.load(Ordering::Relaxed));
        assert_eq!(h.shared.count_in(), 0.0);
        let beat = h.shared.position();
        assert!(beat > 8.0 && beat < 8.2, "{beat}");

        // Play alone does not count in, and Stop cancels a count-in.
        h.send(Command::Stop);
        h.send(Command::SetArrangementRecord(false));
        h.send(Command::Play);
        h.run_beats(0.1);
        assert!(h.shared.playing.load(Ordering::Relaxed));
        h.send(Command::Stop);
        h.send(Command::RecordClip { track: 0, slot: 3 });
        h.run_beats(1.0);
        assert!(!h.shared.playing.load(Ordering::Relaxed), "a session take counts in too");
        h.send(Command::Stop);
        h.run_beats(4.0);
        assert!(!h.shared.playing.load(Ordering::Relaxed));
        assert_eq!(h.shared.count_in(), 0.0);
    }

    #[test]
    fn armed_tracks_take_live_input_over_the_input_track() {
        let mut h = Harness::new();
        h.send(Command::SetInputTrack(Some(0)));
        h.send(Command::SetArm { track: 1, armed: true });
        h.play_note(64, 1.0);
        h.run_beats(0.5);
        assert!(h.shared.tracks[1].peak.take() > 0.0);
        assert_eq!(h.shared.tracks[0].peak.take(), 0.0);
    }

    #[test]
    fn session_recording_starts_quantized_and_captures_notes() {
        let mut h = Harness::new();
        h.send(Command::SetArm { track: 0, armed: true });
        h.send(Command::SetClip { track: 1, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::LaunchClip { track: 1, slot: 0 });
        h.run_beats(1.0);
        h.send(Command::RecordClip { track: 0, slot: 0 });
        h.run_beats(1.0); // beat 2: still waiting for the bar
        assert!(h.shared.tracks[0].queued_record.load(Ordering::Relaxed));
        h.play_note(60, 1.0); // before the take starts: not captured
        h.run_beats(0.1);
        h.play_note(60, 0.0);
        h.run_beats(2.5); // past beat 4: recording
        assert_eq!(h.shared.tracks[0].recording.load(Ordering::Relaxed), 0);
        assert_eq!(h.shared.tracks[0].record_start(), 4.0);
        h.play_note(62, 0.8);
        h.run_beats(1.0);
        h.play_note(62, 0.0);
        h.run_beats(0.1);
        let notes = h.recorded();
        assert_eq!(notes.len(), 1, "only the note played during the take is recorded");
        assert_eq!(notes[0].pitch, 62);
        assert!(notes[0].start >= 4.0 && (notes[0].length - 1.0).abs() < 0.05);

        h.send(Command::StopRecording(0));
        h.run_beats(0.1);
        assert_eq!(h.shared.tracks[0].recording.load(Ordering::Relaxed), NONE);
        assert_eq!(h.playing(0), 0, "the recorded slot keeps playing");
    }

    #[test]
    fn arrangement_recording_captures_armed_tracks_and_flushes_on_stop() {
        let mut h = Harness::new();
        h.send(Command::SetArm { track: 3, armed: true });
        h.send(Command::SetArrangementRecord(true));
        h.send(Command::Play);
        h.run_beats(1.0);
        h.play_note(48, 0.7);
        h.run_beats(1.0);
        h.send(Command::Stop); // the held note is closed at the stop position
        h.run_beats(0.1);
        let notes = h.recorded();
        assert_eq!(notes.len(), 1);
        assert_eq!((notes[0].track, notes[0].pitch), (3, 48));
        assert!((notes[0].start - 1.0).abs() < 0.05 && (notes[0].length - 1.0).abs() < 0.05);
        assert!(!h.shared.arrangement_recording.load(Ordering::Relaxed));
    }

    #[test]
    fn automation_drives_mixer_and_device_parameters() {
        use crate::model::{AutomationPoint, Param};
        let mut h = Harness::new();
        // Volume fades from silence to full over the first bar; a send opens at beat 2.
        let lanes = vec![
            AutomationLane {
                param: Param::Volume,
                points: vec![AutomationPoint { beat: 0.0, value: 0.0, curve: 0.0 }, AutomationPoint { beat: 4.0, value: 1.0, curve: 0.0 }],
            },
            AutomationLane {
                param: Param::Send(0),
                points: vec![AutomationPoint { beat: 1.99, value: 0.0, curve: 0.0 }, AutomationPoint { beat: 2.0, value: 1.0, curve: 0.0 }],
            },
            AutomationLane {
                param: Param::Device { device: 0, param: 0 },
                points: vec![AutomationPoint { beat: 0.0, value: 100.0, curve: 0.0 }],
            },
        ];
        h.send(Command::InsertDevice {
            target: Target::Track(0),
            index: 0,
            slot: ChainSlot::new(&Device::new(Effect::AutoFilter(Default::default())), SR),
        });
        h.send(Command::SetAutomation { target: Target::Track(0), lanes: Box::new(lanes) });
        let mut clip = MidiClip::new("drone", 4.0, [0, 0, 0]);
        clip.notes.push(Note { start: 0.0, length: 4.0, pitch: 48, velocity: 1.0 });
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Midi(clip))) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });

        let early = energy(&h.run_beats(0.25));
        h.run_beats(1.5);
        assert_eq!(h.shared.returns[0].take(), 0.0, "the send is still closed");
        let late = energy(&h.run_beats(0.5));
        assert!(late > early * 10.0, "the fade-in should raise the level ({early} -> {late})");
        h.run_beats(0.5);
        assert!(h.shared.returns[0].take() > 0.0, "the send opened at beat 2");
        assert_eq!(h.engine.tracks[0].channel.automation.len(), 3);
        let Effect::AutoFilter(p) = h.engine.tracks[0].channel.chain[0].processor.effect() else { panic!() };
        assert_eq!(p.cutoff, 100.0, "device parameters follow their lane");
    }

    #[test]
    fn clip_envelopes_follow_the_clip_and_win_over_track_automation() {
        use crate::model::{AutomationPoint, Param};
        let ramp = |param| AutomationLane {
            param,
            points: vec![AutomationPoint { beat: 0.0, value: 0.0, curve: 0.0 }, AutomationPoint { beat: 2.0, value: 1.0, curve: 0.0 }],
        };
        let near = |a: f32, b: f32| (a - b).abs() < 0.05;
        // A session clip, 4 beats, its volume rising over its first 2; the track's
        // own automation says 0.25 throughout.
        let mut h = Harness::new();
        h.send(Command::SetAutomation {
            target: Target::Track(0),
            lanes: Box::new(vec![AutomationLane { param: Param::Volume, points: vec![AutomationPoint { beat: 0.0, value: 0.25, curve: 0.0 }] }]),
        });
        let mut clip = MidiClip::new("env", 4.0, [0, 0, 0]);
        clip.envelopes.push(ramp(Param::Volume));
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Midi(clip.clone()))) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        h.run_beats(1.0);
        let gain = h.engine.tracks[0].channel.gain;
        assert!(near(gain, 0.5), "half way up its ramp, over the track's 0.25: {gain}");
        h.run_beats(2.0);
        assert!(near(h.engine.tracks[0].channel.gain, 1.0), "held after the last point");
        // The clip loops: its envelope starts again.
        h.run_beats(2.0);
        let gain = h.engine.tracks[0].channel.gain;
        assert!(near(gain, 0.5), "a loop later, the same place in the clip: {gain}");
        // Stopped: the track's automation again.
        h.send(Command::BackToArrangement);
        h.run_beats(0.25);
        assert!(near(h.engine.tracks[0].channel.gain, 0.25));

        // On the arrangement, from where the clip is placed.
        let mut h = Harness::new();
        let placed = PlacedClip::new(8.0, 8.0, PlayClip::Midi(clip));
        h.send(Command::SetArrangement { track: 0, clips: Box::new(vec![placed]) });
        h.send(Command::Play);
        h.run_beats(6.0);
        let before = h.engine.tracks[0].channel.gain;
        assert!(near(before, 1.0), "before the clip, untouched: {before}");
        h.run_beats(3.0);
        let gain = h.engine.tracks[0].channel.gain;
        assert!(near(gain, 0.5), "a beat into the clip: {gain}");
        h.run_beats(4.0);
        let gain = h.engine.tracks[0].channel.gain;
        assert!(near(gain, 0.5), "a beat into its second loop: {gain}");
    }

    #[test]
    fn swing_delays_only_off_beat_notes() {
        assert_eq!(swing_offset(0.0, 0.25, 1.0), 0.0);
        assert!((swing_offset(0.25, 0.25, 1.0) - 0.25 / 3.0).abs() < 1e-12);
        assert_eq!(swing_offset(0.5, 0.25, 1.0), 0.0);
        assert!((swing_offset(0.75, 0.25, 0.5) - 0.25 / 6.0).abs() < 1e-12);
        assert_eq!(swing_offset(0.3, 0.25, 1.0), 0.0, "notes off the grid are left alone");
        assert_eq!(swing_offset(0.25, 0.25, 0.0), 0.0);
    }

    #[test]
    fn swing_moves_the_off_beat_note_when_playing() {
        use crate::model::SynthParams;
        // Two short sine blips on the first two sixteenths; find when the second starts.
        let onset = |amount: f32| {
            let mut h = Harness::new();
            let params = SynthParams { waveform: crate::model::Waveform::Sine, attack: 0.0005, release: 0.002, ..Default::default() };
            h.send(Command::update_instrument(0, InstrumentKind::Synth(params)));
            h.send(Command::SetSwing { amount, grid: 0.25 });
            let mut clip = MidiClip::new("blips", 4.0, [0, 0, 0]);
            for start in [0.0, 0.25] {
                clip.notes.push(Note { start, length: 0.05, pitch: 72, velocity: 1.0 });
            }
            h.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Midi(clip))) });
            h.send(Command::LaunchClip { track: 0, slot: 0 });
            let audio = left_channel(&h.run_beats(1.0));
            // Skip the first blip (0.05 beats = 1200 frames, plus its release).
            let from = 3000;
            from + audio[from..].iter().position(|s| s.abs() > 0.001).expect("second note")
        };
        let straight = onset(0.0);
        let swung = onset(1.0);
        // A sixteenth at 120 BPM is 6000 frames; full swing delays it by a third: 2000 frames.
        assert!((straight as i64 - 6000).abs() < 64, "straight onset at {straight}");
        assert!((swung as i64 - 8000).abs() < 64, "swung onset at {swung}");
    }

    /// A clip's own groove wins over the project's swing: it swings with none
    /// set, and plays straight (its groove at 0) with the project's at full.
    #[test]
    fn a_clips_groove_overrides_the_projects_swing() {
        use crate::model::{Groove, SynthParams};
        let onset = |project: f32, groove: Option<Groove>| {
            let mut h = Harness::new();
            let params = SynthParams { waveform: crate::model::Waveform::Sine, attack: 0.0005, release: 0.002, ..Default::default() };
            h.send(Command::update_instrument(0, InstrumentKind::Synth(params)));
            h.send(Command::SetSwing { amount: project, grid: 0.25 });
            let mut clip = MidiClip::new("blips", 4.0, [0, 0, 0]);
            for start in [0.0, 0.25] {
                clip.notes.push(Note { start, length: 0.05, pitch: 72, velocity: 1.0 });
            }
            clip.groove = groove;
            h.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Midi(clip))) });
            h.send(Command::LaunchClip { track: 0, slot: 0 });
            let audio = left_channel(&h.run_beats(1.0));
            3000 + audio[3000..].iter().position(|s| s.abs() > 0.001).expect("second note")
        };
        let swing = Groove { grid: 0.25, swing: 1.0, ..Default::default() };
        assert!((onset(0.0, Some(swing)) as i64 - 8000).abs() < 64);
        assert!((onset(1.0, Some(Groove::default())) as i64 - 6000).abs() < 64);
        assert!((onset(1.0, None) as i64 - 8000).abs() < 64, "without one, the project's");
    }

    /// When a clip with envelopes ends, what they moved comes back: the fader,
    /// a send and a device's parameter (before, they stayed as the clip left
    /// them, and the next clip played 17 dB down).
    #[test]
    fn a_clips_envelopes_end_with_it() {
        use crate::model::{AutomationPoint, UtilityParams};
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        let mut h = Harness::new();
        let mut quiet = audio_clip(WarpMode::Off);
        let lane = |param, value| AutomationLane { param, points: vec![AutomationPoint { beat: 0.0, value, curve: 0.0 }] };
        quiet.envelopes = vec![
            lane(Param::Volume, 0.1),
            lane(Param::Send(0), 1.0),
            lane(Param::Device { device: 0, param: 0 }, -30.0),
        ];
        h.send(Command::ReplaceChain { target: Target::Track(0), chain: build_chain(&[Device::new(Effect::Utility(UtilityParams { gain_db: 0.0, ..Default::default() }))], SR) });
        h.send(Command::SetGain { target: Target::Track(0), gain: 0.8 });
        h.send(Command::SetSend { track: 0, send: 0, amount: 0.25 });
        let clips = vec![PlacedClip::new(0.0, 2.0, PlayClip::Audio(quiet, sine_sample())), PlacedClip::new(2.0, 2.0, PlayClip::Audio(audio_clip(WarpMode::Off), sine_sample()))];
        h.send(Command::SetArrangement { track: 0, clips: Box::new(clips) });
        h.send(Command::Play);
        h.run_beats(1.0);
        let t = &h.engine.tracks[0];
        assert!(near(t.channel.gain, 0.1) && near(t.sends[0], 1.0), "the envelopes rule inside the clip");
        h.run_beats(2.0);
        let t = &h.engine.tracks[0];
        assert!(near(t.channel.gain, 0.8), "the fader again: {}", t.channel.gain);
        assert!(near(t.sends[0], 0.25), "the send again: {}", t.sends[0]);
        assert!(near(t.channel.chain[0].processor.effect().get(0), 0.0), "the device again: {}", t.channel.chain[0].processor.effect().get(0));
    }

    /// Muting a group silences what its tracks send to the returns too (before,
    /// a muted bus still fed the reverb).
    #[test]
    fn a_muted_group_silences_its_tracks_sends() {
        let level = |muted: bool| {
            let mut h = Harness::new();
            let mut parents = [None; MAX_TRACKS];
            // Track 1 in the group on track 0 (groups come before their tracks).
            parents[1] = Some(0);
            h.send(Command::SetGroups(parents));
            h.send(Command::SetSend { track: 1, send: 0, amount: 1.0 });
            h.send(Command::SetMute { target: Target::Track(0), mute: muted });
            h.send(Command::SetClip { track: 1, slot: 0, clip: Some(Box::new(PlayClip::Audio(audio_clip(WarpMode::Off), sine_sample()))) });
            h.send(Command::LaunchClip { track: 1, slot: 0 });
            energy(&h.run_beats(2.0))
        };
        assert!(level(false) > 100.0);
        assert_eq!(level(true), 0.0, "nothing of the group is heard");
    }

    /// A render starts at full level: the first hit of an export or a freeze is
    /// not faded in (it was 30 dB down for its first 5 ms).
    #[test]
    fn the_first_block_is_not_faded_in() {
        let mut h = Harness::new();
        h.send(Command::SetGain { target: Target::Track(0), gain: 0.5 });
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Audio(audio_clip(WarpMode::Off), sine_sample()))) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        let mut first = vec![0.0; 512 * 2];
        h.engine.process(&mut first, 2);
        // After the clip's own 3 ms fade: what comes out over what goes in stays
        // the same through the block (a ramp from silence would make it grow).
        let input = |i: usize| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5;
        let ratios: Vec<f32> = (150..512).filter(|&i| input(i).abs() > 0.3).map(|i| first[2 * i] / input(i)).collect();
        let (low, high) = ratios.iter().fold((f32::MAX, 0f32), |(l, h), &r| (l.min(r), h.max(r)));
        assert!(high > 0.1 && (high - low) / high < 0.02, "{low} … {high}");
    }

    /// An effect added inside a Wavetable synth while it plays changes its layout:
    /// the app sends a rebuilt synth, and that is the one heard (before, the
    /// running one only took the parameters and ignored the new slot).
    #[test]
    fn an_effect_added_inside_a_synth_is_heard_at_once() {
        use crate::model::{UtilityParams, WaveParams};
        use crate::wavetable::builtin;
        let level = |with_fx: bool| -> f32 {
            let mut h = Harness::new();
            let base = InstrumentKind::Wavetable(WaveParams::default());
            h.send(Command::set_instrument(0, base, SR));
            h.send(Command::SetWavetables { track: 0, tables: Box::new(vec![Arc::new(builtin(0)), Arc::new(builtin(3))]) });
            let mut clip = MidiClip::new("A", 4.0, [0, 0, 0]);
            clip.notes.push(Note { start: 0.0, length: 4.0, pitch: 60, velocity: 1.0 });
            h.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Midi(clip))) });
            h.send(Command::LaunchClip { track: 0, slot: 0 });
            h.run_beats(0.5);
            if with_fx {
                let mut p = WaveParams::default();
                p.fx[0] = Some(Device::new(Effect::Utility(UtilityParams { gain_db: -40.0, ..Default::default() })));
                let kind = InstrumentKind::Wavetable(p);
                assert!(base.needs_rebuild(&kind));
                h.send(Command::set_instrument(0, kind, SR));
            }
            energy(&h.run_beats(1.0))
        };
        let (plain, with) = (level(false), level(true));
        assert!(plain > 1.0, "{plain}");
        assert!(with < plain * 0.01, "the added −40 dB is heard: {with} vs {plain}");
    }

    /// One NaN in a track's audio (a damaged float WAV, a plugin that blew up),
    /// sent to a reverb: the track is silenced for that block and the rest plays
    /// on. Before, the reverb kept it and the master was NaN for good.
    #[test]
    fn an_invalid_sample_never_poisons_the_mix() {
        let mut h = Harness::new();
        let mut bad: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        bad[1000] = f32::NAN;
        let bad = Arc::new(Sample::from_channels(bad.clone(), bad, SR));
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Audio(audio_clip(WarpMode::Off), bad))) });
        h.send(Command::SetSend { track: 0, send: 0, amount: 0.5 });
        h.send(Command::ReplaceChain { target: Target::Return(0), chain: build_chain(&[Device::new(Effect::Reverb(Default::default()))], SR) });
        h.send(Command::SetClip { track: 1, slot: 0, clip: Some(Box::new(PlayClip::Audio(audio_clip(WarpMode::Off), sine_sample()))) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        h.send(Command::LaunchClip { track: 1, slot: 0 });
        let audio = h.run_beats(8.0);
        assert!(audio.iter().all(|s| s.is_finite()), "the output stays valid");
        assert!(energy(&audio[audio.len() / 2..]) > 100.0, "the healthy track still sounds");
        assert!(h.shared.faults.load(Ordering::Relaxed) > 0);
        assert_eq!(h.shared.last_fault(), Fault::Track(0), "and the app is told which one");
    }

    #[test]
    fn audio_clips_play_in_every_warp_mode() {
        for warp in WarpMode::ALL {
            let mut h = Harness::new();
            h.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Audio(audio_clip(warp), sine_sample()))) });
            h.send(Command::LaunchClip { track: 0, slot: 0 });
            let audio = h.run_beats(4.0);
            assert!(audio.iter().all(|s| s.is_finite()));
            assert!(energy(&audio) > 100.0, "{warp:?} should be audible");
        }
    }

    #[test]
    fn repitch_follows_tempo_and_stretch_keeps_pitch() {
        let pitch_at_double_tempo = |warp| {
            let mut h = Harness::new();
            h.send(Command::SetTempo(240.0));
            h.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Audio(audio_clip(warp), sine_sample()))) });
            h.send(Command::LaunchClip { track: 0, slot: 0 });
            // At 240 BPM, run_beats(1.0) renders 0.5 s of audio; skip the first moments.
            h.run_beats(0.2);
            let audio = left_channel(&h.run_beats(1.0));
            crossings(&audio) as f64 / 0.5
        };
        let repitch = pitch_at_double_tempo(WarpMode::Repitch);
        let stretch = pitch_at_double_tempo(WarpMode::Stretch);
        assert!((repitch - 880.0).abs() < 20.0, "Re-Pitch at double tempo should sound an octave up, got {repitch} Hz");
        assert!((stretch - 440.0).abs() < 20.0, "Stretch should keep 440 Hz, got {stretch} Hz");
    }

    /// The browser's preview plays a file once, transport stopped, then is silent.
    #[test]
    fn a_previewed_file_plays_once() {
        let mut h = Harness::new();
        h.send(Command::PreviewSample(Some(sine_sample())));
        // The 1 s sine: 2 beats at 120 BPM.
        let during = left_channel(&h.run_beats(1.6));
        h.run_beats(0.6);
        let after = h.run_beats(1.0);
        assert!((crossings(&during) as f64 / 0.8 - 440.0).abs() < 10.0, "at its own pitch");
        assert!(energy(&during) > 100.0);
        assert!(energy(&after) < 1e-6, "once, not looping");
        // Stopped halfway.
        h.send(Command::PreviewSample(Some(sine_sample())));
        h.run_beats(0.5);
        h.send(Command::PreviewSample(None));
        assert!(energy(&h.run_beats(0.5)) < 1e-6);
    }

    /// Between warp markers the file plays at each stretch's own speed.
    #[test]
    fn warp_markers_set_the_speed_of_each_stretch() {
        // At 120 BPM a beat is 0.5 s: the first beat plays 0.8 s of the file (×1.6),
        // the second 0.2 s (×0.4).
        let mut clip = audio_clip(WarpMode::Repitch);
        clip.warp_markers = [(0.0, 0.0), (1.0, 0.8), (2.0, 1.0)].map(|(beat, seconds)| WarpMarker { beat, seconds }).to_vec();
        let mut h = Harness::new();
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Audio(clip.clone(), sine_sample()))) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        h.run_beats(0.1);
        let first = crossings(&left_channel(&h.run_beats(0.8))) as f64 / 0.4;
        h.run_beats(0.2);
        let second = crossings(&left_channel(&h.run_beats(0.8))) as f64 / 0.4;
        assert!((first - 704.0).abs() < 20.0, "{first} Hz");
        assert!((second - 176.0).abs() < 20.0, "{second} Hz");

        // In Stretch the pitch stays and the file's point at 0.8 s lands on beat 1.
        let mut click: Vec<f32> = vec![0.0; 48_000];
        for (i, s) in click[38_400..39_400].iter_mut().enumerate() {
            *s = (i as f32 * 0.3).sin() * 0.8;
        }
        let sample = Arc::new(Sample::from_channels(click.clone(), click, 48_000.0));
        clip.warp = WarpMode::Stretch;
        let mut h = Harness::new();
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Audio(clip, sample))) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        let audio = left_channel(&h.run_beats(2.0));
        let onset = audio.iter().position(|s| s.abs() > 0.05).expect("the click");
        assert!((onset as i64 - 24_000).abs() < 1_500, "on beat 1 (24000), got {onset}");
    }

    /// Two pieces that follow on cross-fade at the cut: no dip to silence there,
    /// as each one fading out on its own would leave.
    #[test]
    fn butting_audio_clips_cross_fade_without_a_gap() {
        let level = |v: f32| Arc::new(Sample::from_channels(vec![v; 96_000], vec![v; 96_000], 48_000.0));
        let (first, second) = (level(0.5), level(0.5));
        let mut clip = audio_clip(WarpMode::Off);
        clip.length = 1.0;
        let mut later = clip.clone();
        later.offset = 0.5;
        let mut clips = vec![
            PlacedClip::new(0.0, 1.0, PlayClip::Audio(clip.clone(), first.clone())),
            PlacedClip::new(1.0, 1.0, PlayClip::Audio(later.clone(), second.clone())),
            // Looping: no cross-fade (it repeats, there's nothing to play on into).
            PlacedClip::new(4.0, 2.0, PlayClip::Audio(clip.clone(), first.clone())),
        ];
        link_neighbours(&mut clips);
        assert_eq!(clips.iter().map(|c| c.joined).collect::<Vec<_>>(), vec![(false, true), (true, false), (false, false)]);

        let render = |joined: [(bool, bool); 2]| {
            let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
            let per_sample = 2.0 / 48_000.0;
            render_audio_clip(&clip, &first, 0.0, 1.0, joined[0], (0, 48_000), 0.0, per_sample, 120.0, &mut l, &mut r);
            render_audio_clip(&later, &second, 1.0, 2.0, joined[1], (0, 48_000), 0.0, per_sample, 120.0, &mut l, &mut r);
            l
        };
        // Around the cut (beat 1 = frame 24000).
        let lowest = |audio: &[f32]| audio[23_500..24_500].iter().copied().fold(f32::INFINITY, f32::min);
        assert!(lowest(&render([(false, false); 2])) < 0.01, "each fading alone dips to silence");
        let crossed = render([(false, true), (true, false)]);
        assert!((lowest(&crossed) - 0.5).abs() < 1e-3, "{}", lowest(&crossed));
        assert!(crossed[23_500..24_500].iter().all(|s| (s - 0.5).abs() < 1e-3), "and never goes above either");
        // The outer edges still fade.
        assert!(crossed[0].abs() < 0.01 && crossed[47_999].abs() < 0.01);
    }

    #[test]
    fn arrangement_audio_clips_stop_at_their_end() {
        let mut h = Harness::new();
        let placed = PlacedClip::new(0.0, 1.0, PlayClip::Audio(audio_clip(WarpMode::Repitch), sine_sample()));
        h.send(Command::SetArrangement { track: 0, clips: Box::new(vec![placed]) });
        h.send(Command::Play);
        let during = h.run_beats(1.0);
        let after = h.run_beats(1.0);
        assert!(energy(&during) > 100.0);
        assert!(energy(&after) < 1e-6);
    }

    #[test]
    fn arrangement_clips_play_at_their_position() {
        let mut h = Harness::new();
        h.send(Command::SetArrangement { track: 0, clips: arrangement_with_note_at(4.0) });
        h.send(Command::Play);
        let before = h.run_beats(3.5);
        assert!(energy(&before) < 1e-6, "nothing should sound before the clip");
        let during = h.run_beats(1.5);
        assert!(energy(&during) > 1.0, "the arrangement clip should sound");
    }

    #[test]
    fn session_launch_overrides_arrangement_until_back_to_arrangement() {
        let mut h = Harness::new();
        h.send(Command::SetArrangement { track: 0, clips: arrangement_with_note_at(0.0) });
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        h.run_beats(0.5);
        assert!(!h.shared.tracks[0].follows_arrangement.load(Ordering::Relaxed));
        h.send(Command::BackToArrangement);
        h.run_beats(0.1);
        assert!(h.shared.tracks[0].follows_arrangement.load(Ordering::Relaxed));
        assert_eq!(h.playing(0), NONE);
    }

    /// A note held on a keyboard goes on sounding when the loop jumps back.
    #[test]
    fn a_held_live_note_sounds_across_the_loop_jump() {
        let mut h = Harness::new();
        h.send(Command::SetInputTrack(Some(2)));
        h.send(Command::SetLoop { enabled: true, start: 0.0, end: 2.0 });
        h.send(Command::Play);
        h.play_note(60, 0.9);
        h.run_beats(1.5);
        h.shared.tracks[2].peak.take();
        // Across the jump (at beat 2) and well after it (past any release), still held.
        h.run_beats(1.5);
        h.shared.tracks[2].peak.take();
        h.run_beats(0.5);
        assert!(h.shared.tracks[2].peak.take() > 0.05, "the held note was cut at the loop");
        h.play_note(60, 0.0);
        h.run_beats(1.0);
        h.shared.tracks[2].peak.take();
        h.run_beats(0.5);
        assert_eq!(h.shared.tracks[2].peak.take(), 0.0, "and it stops on its note-off");
    }

    /// An arrangement note that goes past the loop's end stops there: nothing
    /// hangs on after the jump.
    #[test]
    fn arrangement_notes_end_at_the_loop_jump() {
        let mut h = Harness::new();
        let mut clip = MidiClip::new("long", 16.0, [0, 0, 0]);
        clip.notes.push(Note { start: 1.0, length: 8.0, pitch: 60, velocity: 1.0 });
        h.send(Command::SetArrangement { track: 0, clips: Box::new(vec![PlacedClip::new(0.0, 16.0, PlayClip::Midi(clip))]) });
        h.send(Command::SetLoop { enabled: true, start: 0.0, end: 2.0 });
        h.send(Command::Play);
        h.run_beats(1.5);
        assert!(h.shared.tracks[0].peak.take() > 0.05, "the note plays");
        // After the jump, before it starts again at beat 1 (release tail aside).
        h.run_beats(1.0);
        h.shared.tracks[0].peak.take();
        h.run_beats(0.3);
        assert_eq!(h.shared.tracks[0].peak.take(), 0.0, "the note hung on past the loop's end");
    }

    #[test]
    fn loop_wraps_the_playhead() {
        let mut h = Harness::new();
        h.send(Command::SetLoop { enabled: true, start: 4.0, end: 8.0 });
        h.send(Command::SetPosition(4.0));
        h.send(Command::Play);
        h.run_beats(5.0);
        let pos = h.shared.position();
        assert!((4.9..5.1).contains(&pos), "expected about beat 5 after wrapping, got {pos}");
    }

    #[test]
    fn solo_mutes_other_tracks_and_sends_reach_returns() {
        let mut h = Harness::new();
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::SetClip { track: 1, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::SetSolo { track: 1, solo: true });
        h.send(Command::SetSend { track: 1, send: 0, amount: 1.0 });
        h.send(Command::LaunchScene(0));
        h.run_beats(1.0);
        assert_eq!(h.shared.tracks[0].peak.take(), 0.0);
        assert!(h.shared.tracks[1].peak.take() > 0.0);
        assert!(h.shared.returns[0].take() > 0.0);
    }

    /// Tracks 1 and 2 in group 0, track 3 outside.
    fn grouped() -> Harness {
        let mut h = Harness::new();
        for t in 1..=3 {
            h.send(Command::SetClip { track: t, slot: 0, clip: Some(one_note_clip()) });
        }
        let mut parents = [None; MAX_TRACKS];
        parents[1] = Some(0);
        parents[2] = Some(0);
        h.send(Command::SetGroups(parents));
        h.send(Command::LaunchScene(0));
        h
    }

    fn peaks(h: &Harness) -> Vec<f32> {
        (0..4).map(|t| h.shared.tracks[t].peak.take()).collect()
    }

    #[test]
    fn a_group_plays_its_tracks_through_its_fader() {
        let mut h = grouped();
        h.run_beats(0.5);
        let p = peaks(&h);
        assert!(p.iter().all(|&x| x > 0.0), "the group sounds with its tracks: {p:?}");
        // The group's fader down: its tracks go quiet in the master, the other doesn't.
        let mut alone = grouped();
        alone.send(Command::SetMute { target: Target::Track(3), mute: true });
        let with_group = energy(&alone.run_beats(0.5));
        let mut h = grouped();
        h.send(Command::SetMute { target: Target::Track(3), mute: true });
        h.send(Command::SetMute { target: Target::Track(0), mute: true });
        let without = energy(&h.run_beats(0.5));
        assert!(with_group > 1.0 && without < 1e-6, "{with_group} {without}");
    }

    #[test]
    fn soloing_inside_and_outside_a_group() {
        // A track inside: it and its group sound, not its sibling nor the rest.
        let mut h = grouped();
        h.send(Command::SetSolo { track: 1, solo: true });
        h.run_beats(0.5);
        let p = peaks(&h);
        assert!(p[0] > 0.0 && p[1] > 0.0 && p[2] == 0.0 && p[3] == 0.0, "{p:?}");
        // The group: all its tracks.
        let mut h = grouped();
        h.send(Command::SetSolo { track: 0, solo: true });
        h.run_beats(0.5);
        let p = peaks(&h);
        assert!(p[0] > 0.0 && p[1] > 0.0 && p[2] > 0.0 && p[3] == 0.0, "{p:?}");
    }

    #[test]
    fn groups_follow_their_tracks_when_reordered_and_nest() {
        assert_eq!(processing_order(&std::array::from_fn(|i| match i { 1 => Some(0), 2 => Some(1), _ => None }), &[0; MAX_TRACKS])[..3], [2, 1, 0]);
        let mut h = grouped();
        // Group and tracks moved to the end of the first four: 3, 0, 1, 2.
        let mut order: [usize; MAX_TRACKS] = std::array::from_fn(|i| i);
        order[..4].copy_from_slice(&[3, 0, 1, 2]);
        h.send(Command::ReorderTracks(order));
        h.run_beats(0.1);
        assert_eq!(&h.engine.parents[..4], &[None, None, Some(1), Some(1)]);
        // A group after its tracks is refused.
        let mut wrong = [None; MAX_TRACKS];
        wrong[0] = Some(3);
        h.send(Command::SetGroups(wrong));
        h.run_beats(0.1);
        assert_eq!(h.engine.parents[0], None);
    }

    /// Track 1 plays on, with a ducker; track 0 (muted, as a sidechain source
    /// needs not be heard) plays a note now and then.
    fn sidechained(sidechain: Option<u32>) -> f32 {
        let mut h = Harness::new();
        let mut ids = [0u32; MAX_TRACKS];
        ids[0] = 7;
        ids[1] = 8;
        h.send(Command::SetTrackIds(ids));
        let mut held = MidiClip::new("pad", 4.0, [0, 0, 0]);
        held.notes.push(Note { start: 0.0, length: 4.0, pitch: 48, velocity: 1.0 });
        h.send(Command::SetClip { track: 1, slot: 0, clip: Some(Box::new(PlayClip::Midi(held))) });
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::SetMute { target: Target::Track(0), mute: true });
        let ducker = Device::new(Effect::Ducker(crate::model::DuckerParams { depth: 1.0, sidechain, ..Default::default() }));
        h.send(Command::InsertDevice { target: Target::Track(1), index: 0, slot: ChainSlot::new(&ducker, SR) });
        h.send(Command::LaunchScene(0));
        // While track 0's note sounds (its first beat).
        energy(&h.run_beats(0.9))
    }

    #[test]
    fn a_sidechain_ducks_when_its_source_sounds() {
        let free = {
            let mut h = Harness::new();
            let mut held = MidiClip::new("pad", 4.0, [0, 0, 0]);
            held.notes.push(Note { start: 0.0, length: 4.0, pitch: 48, velocity: 1.0 });
            h.send(Command::SetClip { track: 1, slot: 0, clip: Some(Box::new(PlayClip::Midi(held))) });
            h.send(Command::LaunchScene(0));
            energy(&h.run_beats(0.9))
        };
        let ducked = sidechained(Some(7));
        // A source that does not exist: nothing to listen to, so the tempo-synced
        // pump instead (it ducks at the start of each beat, much less overall).
        let missing = sidechained(Some(99));
        assert!(ducked < free * 0.3, "ducked by the source: {ducked} vs {free}");
        assert!(missing > ducked * 2.0, "{missing} vs {ducked}");
    }

    #[test]
    fn a_sidechained_compressor_listens_to_its_source() {
        let run = |sidechain: Option<u32>| {
            let mut h = Harness::new();
            let mut ids = [0u32; MAX_TRACKS];
            ids[0] = 7;
            h.send(Command::SetTrackIds(ids));
            // A soft pad that alone stays under the threshold; a loud note on track 0.
            let mut held = MidiClip::new("pad", 4.0, [0, 0, 0]);
            held.notes.push(Note { start: 0.0, length: 4.0, pitch: 48, velocity: 0.3 });
            h.send(Command::SetClip { track: 1, slot: 0, clip: Some(Box::new(PlayClip::Midi(held))) });
            h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
            h.send(Command::SetMute { target: Target::Track(0), mute: true });
            let params = crate::model::CompressorParams { threshold_db: -30.0, ratio: 20.0, attack_ms: 1.0, sidechain, ..Default::default() };
            let device = Device::new(Effect::Compressor(params));
            h.send(Command::InsertDevice { target: Target::Track(1), index: 0, slot: ChainSlot::new(&device, SR) });
            h.send(Command::LaunchScene(0));
            energy(&h.run_beats(0.9))
        };
        let (own, keyed) = (run(None), run(Some(7)));
        assert!(keyed < own * 0.5, "{keyed} vs {own}");
    }

    #[test]
    fn sidechain_sources_are_processed_first_and_follow_reordering() {
        // Track 0 listens to 3: 3 first.
        let mut wants = [0u64; MAX_TRACKS];
        wants[0] = 1 << 3;
        assert_eq!(&processing_order(&[None; MAX_TRACKS], &wants)[..2], &[3, 0]);
        // A bass inside group 1 listening to a kick outside it (track 4): kick,
        // bass, then the group.
        let mut parents = [None; MAX_TRACKS];
        parents[2] = Some(1);
        let mut wants = [0u64; MAX_TRACKS];
        wants[2] = 1 << 4;
        let order = processing_order(&parents, &wants);
        let at = |t: usize| order.iter().position(|&o| o == t).unwrap();
        assert!(at(4) < at(2) && at(2) < at(1), "{:?}", &order[..5]);
        // A track listening to its own group: the group still comes after it.
        let mut wants = [0u64; MAX_TRACKS];
        wants[2] = 1 << 1;
        let order = processing_order(&parents, &wants);
        let at = |t: usize| order.iter().position(|&o| o == t).unwrap();
        assert!(at(2) < at(1));
        // Each track once.
        let mut seen = order.to_vec();
        seen.sort_unstable();
        assert_eq!(seen, (0..MAX_TRACKS).collect::<Vec<_>>());
        let mut h = Harness::new();
        let mut ids = [0u32; MAX_TRACKS];
        ids[0] = 5;
        h.send(Command::SetTrackIds(ids));
        let mut order: [usize; MAX_TRACKS] = std::array::from_fn(|i| i);
        order[..2].copy_from_slice(&[1, 0]);
        h.send(Command::ReorderTracks(order));
        h.run_beats(0.05);
        assert_eq!(&h.engine.ids[..2], &[0, 5]);
    }

    #[test]
    fn the_masters_tempo_lane_sets_the_tempo() {
        // 120 BPM until beat 4, then 60.
        let mut h = Harness::new();
        let mut lane = AutomationLane::new(Param::Tempo);
        for (beat, value) in [(0.0, 120.0), (3.99, 120.0), (4.0, 60.0)] {
            lane.points.push(crate::model::AutomationPoint { beat, value, curve: 0.0 });
        }
        h.send(Command::SetAutomation { target: Target::Master, lanes: Box::new(vec![lane]) });
        h.send(Command::Play);
        // Two seconds at 120: four beats.
        let frames = (2.0 * SR) as usize;
        let mut out = vec![0.0; frames * 2];
        for chunk in out.chunks_mut(512 * 2) {
            h.engine.process(chunk, 2);
        }
        let at = h.shared.position();
        assert!((at - 4.0).abs() < 0.05, "{at}");
        // Two more seconds at 60: two beats more.
        for chunk in out.chunks_mut(512 * 2) {
            h.engine.process(chunk, 2);
        }
        let at = h.shared.position();
        assert!((at - 6.0).abs() < 0.05, "{at}");
    }

    #[test]
    fn device_chain_processes_and_can_be_edited() {
        let mut h = Harness::new();
        let device = Device::new(Effect::Reverb(Default::default()));
        h.send(Command::InsertDevice { target: Target::Master, index: 0, slot: ChainSlot::new(&device, SR) });
        let mut off = device;
        off.enabled = false;
        h.send(Command::UpdateDevice { target: Target::Master, index: 0, device: off });
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        let audio = h.run_beats(1.0);
        assert!(audio.iter().all(|s| s.is_finite()));
        h.send(Command::RemoveDevice { target: Target::Master, index: 0 });
        h.run_beats(0.1);
        assert!(h.engine.master.chain.is_empty());
    }
}
