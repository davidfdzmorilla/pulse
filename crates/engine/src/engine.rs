//! The audio-thread side: transport, clip launching, arrangement playback,
//! device chains and mixing.
//!
//! The UI talks to the engine only through a lock-free command queue, and reads
//! back transport and clip state through atomics in [`SharedState`]. Nothing in
//! [`Engine::process`] allocates or locks; anything the engine replaces is
//! handed back to the UI thread as [`Garbage`] to be freed there.

use crate::effects::{EffectProcessor, ProcessContext};
use crate::model::{db_to_gain, AudioClip, AutomationLane, Device, InstrumentKind, MidiClip, Param, WarpMode};
use crate::sample::Sample;
use crate::instruments::{AcidBass, DrumMachine, FmSynth, SuperSaw};
use crate::kick::KickSynth;
use crate::sampler::{DrumRack, Sampler};
use crate::synth::{Click, PolySynth, HELD};
use rtrb::{Consumer, Producer};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

pub const MAX_TRACKS: usize = 8;
pub const NUM_SCENES: usize = 8;
pub const NUM_RETURNS: usize = 2;
/// Devices per chain; insertions beyond this are rejected so the chain never reallocates.
pub const MAX_CHAIN: usize = 16;
const MAX_BLOCK: usize = 1024;
const MAX_EVENTS: usize = 512;

/// `TrackStatus::queued` value meaning "stop at the next quantization boundary".
pub const QUEUED_STOP: i32 = -2;
pub const NONE: i32 = -1;

/// A mixer channel: a track, a return track or the master.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
}

/// An arrangement clip ready to play; its content loops over `length` beats.
pub struct PlacedClip {
    pub start: f64,
    pub length: f64,
    pub clip: PlayClip,
}

impl PlacedClip {
    fn end(&self) -> f64 {
        self.start + self.length
    }
}

/// A note played live (MIDI keyboard or computer keyboard). Velocity 0 is a note-off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveNote {
    pub pitch: u8,
    pub velocity: f32,
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
    LaunchScene(usize),
    StopAllClips,
    /// Returns every track from session clips to its arrangement.
    BackToArrangement,
    SetArrangement {
        track: usize,
        clips: Box<Vec<PlacedClip>>,
    },
    SetInstrument {
        track: usize,
        kind: InstrumentKind,
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
    /// Starts recording a new clip into an empty slot at the next quantization boundary.
    RecordClip {
        track: usize,
        slot: usize,
    },
    /// Ends a session recording; the slot keeps looping once the UI sends its clip.
    StopRecording(usize),
    /// Records armed tracks onto the arrangement while the transport runs.
    SetArrangementRecord(bool),
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
}

/// State the audio thread publishes for the UI.
pub struct SharedState {
    pub playing: AtomicBool,
    pub arrangement_recording: AtomicBool,
    position: AtomicF64,
    pub tracks: [TrackStatus; MAX_TRACKS],
    pub returns: [Peak; NUM_RETURNS],
    pub master: Peak,
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
                peak: Peak::default(),
            }),
            returns: Default::default(),
            master: Peak::default(),
        }
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

enum Instrument {
    Synth(PolySynth),
    Drums(DrumMachine),
    SuperSaw(SuperSaw),
    Acid(AcidBass),
    Fm(FmSynth),
    Sampler(Sampler),
    Rack(DrumRack),
    Kick(KickSynth),
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
            _ => return false,
        }
        true
    }

    /// Instruments with tempo-synced settings follow the transport.
    fn set_tempo(&mut self, tempo: f64) {
        if let Self::Kick(k) = self {
            k.set_tempo(tempo);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
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
        }
    }

    /// Adds the instrument's output to `left`/`right`.
    fn render(&mut self, samples: &[Arc<Sample>], left: &mut [f32], right: &mut [f32]) {
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
        }
    }
}

#[derive(Clone, Copy)]
enum Event {
    AllNotesOff,
    NoteOn { pitch: u8, velocity: f32, gate: u32 },
    NoteOff { pitch: u8 },
}

/// (frame offset, sort rank, event). Pre-allocated; never grows on the audio thread.
type EventQueue = Vec<(usize, u8, Event)>;

fn push_event(events: &mut EventQueue, frame: usize, event: Event) {
    if events.len() < MAX_EVENTS {
        let rank = match event {
            Event::AllNotesOff => 0,
            Event::NoteOff { .. } => 1,
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
    for lap in first_lap..=last_lap {
        let lap_start = lap as f64 * clip.length;
        for note in clip.notes.iter().filter(|n| n.start < clip.length) {
            let (grid, amount) = timing.swing;
            let t = lap_start + note.start + swing_offset(note.start, grid, amount);
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
                    velocity: note.velocity,
                    gate,
                },
            );
        }
    }
}

/// Length of the fades applied at clip edges and loop points, in seconds.
const EDGE_FADE: f64 = 0.003;
/// Grain length of the Stretch warp mode, in seconds.
const GRAIN: f64 = 0.06;

/// Adds an audio clip (looping, local time 0 at `origin`, silent from `limit`) to
/// `left`/`right` over frames `[from, to)`; `beat` is the transport position at `from`.
#[allow(clippy::too_many_arguments)]
fn render_audio_clip(
    clip: &AudioClip,
    sample: &Sample,
    origin: f64,
    limit: f64,
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
    // Beats inside the clip map to file time at this reference tempo.
    let reference_bpm = match clip.warp {
        WarpMode::Off => tempo,
        WarpMode::Repitch | WarpMode::Stretch => clip.original_bpm.max(1.0),
    };
    let rate = sample.sample_rate as f64;
    let gain = db_to_gain(clip.gain_db);
    let fade = EDGE_FADE * tempo / 60.0;
    let pitch = 2f64.powf(clip.transpose as f64 / 12.0);
    let hop = GRAIN / 2.0;

    for k in from..to {
        let b = beat + (k - from) as f64 * beats_per_sample;
        if b < origin || b >= limit {
            continue;
        }
        let local = (b - origin).rem_euclid(clip.length);
        let remaining = (clip.length - local).min(limit - b);
        let envelope = ((local / fade).min(1.0) * (remaining / fade).min(1.0)) as f32;
        let (l, r) = match clip.warp {
            WarpMode::Off | WarpMode::Repitch => {
                let seconds = clip.offset + local * 60.0 / reference_bpm;
                sample.read(seconds * rate)
            }
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
                    let source = clip.offset + grain_start * tempo / reference_bpm + (t - grain_start) * pitch;
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
    mute: bool,
    left: Vec<f32>,
    right: Vec<f32>,
    automation: Box<Vec<AutomationLane>>,
    /// Pan/fader gains reached at the end of the last block, ramped from to avoid zipper
    /// noise. Starts silent so a channel fades in instead of popping.
    last_gains: (f32, f32),
}

impl Channel {
    fn new() -> Self {
        Self {
            chain: Vec::with_capacity(MAX_CHAIN),
            gain: 1.0,
            pan: 0.0,
            mute: false,
            left: vec![0.0; MAX_BLOCK],
            right: vec![0.0; MAX_BLOCK],
            automation: Box::default(),
            last_gains: (0.0, 0.0),
        }
    }

    /// Applies automation for the mixer and device parameters at `beat`.
    /// Returns values of parameters the channel does not own itself (sends, instrument).
    fn automate(&mut self, beat: f64, mut other: impl FnMut(Param, f32)) {
        for lane in self.automation.iter() {
            let Some(value) = lane.value_at(beat) else { continue };
            match lane.param {
                Param::Volume => self.gain = value.max(0.0),
                Param::Pan => self.pan = value.clamp(-1.0, 1.0),
                Param::Device { device, param } => {
                    if let Some(slot) = self.chain.get_mut(device) {
                        slot.processor.set_param(param, value);
                    }
                }
                param @ (Param::Send(_) | Param::Instrument(_)) => other(param, value),
            }
        }
    }

    /// Mixes this channel into `left`/`right` with its pan and `gain`, ramping from
    /// the previous block's gains. Returns the output peak.
    fn mix_into(&mut self, n: usize, gain: f32, left: &mut [f32], right: &mut [f32]) -> f32 {
        let (gl, gr) = self.pan_gains(gain);
        let (l0, r0) = self.last_gains;
        self.last_gains = (gl, gr);
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

    fn run_chain(&mut self, n: usize, ctx: &ProcessContext) {
        let (left, right) = (&mut self.left[..n], &mut self.right[..n]);
        for slot in self.chain.iter_mut().filter(|s| s.enabled) {
            slot.processor.process(left, right, ctx);
        }
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
    queued: Queued,
    instrument: Instrument,
    /// What the sampler or drum rack plays.
    samples: Box<Vec<Arc<Sample>>>,
    solo: bool,
    sends: [f32; NUM_RETURNS],
    arrangement: Box<Vec<PlacedClip>>,
    follows_arrangement: bool,
    buffer: Vec<f32>,
    buffer_right: Vec<f32>,
    /// Audio clip output for the current block, mixed after the instrument.
    audio_left: Vec<f32>,
    audio_right: Vec<f32>,
    events: EventQueue,
    armed: bool,
    /// Session slot being recorded into.
    recording: Option<usize>,
    record_start: f64,
    /// Live notes being captured: (start beat, velocity) per pitch.
    held: [Option<(f64, f32)>; 128],
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
            queued: Queued::Nothing,
            instrument: Instrument::new(InstrumentKind::Synth(Default::default()), sample_rate),
            samples: Box::default(),
            solo: false,
            sends: [0.0; NUM_RETURNS],
            arrangement: Box::default(),
            follows_arrangement: true,
            buffer: vec![0.0; MAX_BLOCK],
            buffer_right: vec![0.0; MAX_BLOCK],
            audio_left: vec![0.0; MAX_BLOCK],
            audio_right: vec![0.0; MAX_BLOCK],
            events: Vec::with_capacity(MAX_EVENTS),
            armed: false,
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
                    schedule_notes(clip, placed.start, start, end, placed.end(), timing, &mut self.events);
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
                render_audio_clip(clip, sample, self.launch_beat, f64::INFINITY, frames, beat, beats_per_sample, tempo, left, right);
            }
        } else if self.follows_arrangement {
            let end = beat + (to - from) as f64 * beats_per_sample;
            for placed in self.arrangement.iter() {
                let PlayClip::Audio(clip, sample) = &placed.clip else { continue };
                if placed.start < end && placed.end() > beat {
                    let frames = (from, to);
                    render_audio_clip(clip, sample, placed.start, placed.end(), frames, beat, beats_per_sample, tempo, left, right);
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
    click: Click,
    click_buffer: Vec<f32>,
    input_track: Option<usize>,
    arrangement_record: bool,
    limit_output: bool,
    swing: (f64, f32),
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
            click: Click::new(sample_rate),
            click_buffer: vec![0.0; MAX_BLOCK],
            input_track: None,
            arrangement_record: false,
            limit_output: true,
            swing: (0.25, 0.0),
        };
        (engine, shared)
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
        if !self.playing {
            self.playing = true;
            self.position = self.start_marker;
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
            Command::SetInstrument { track, kind } => {
                let Some(t) = self.tracks.get_mut(track) else { return };
                // Keep sounding voices when only parameters change.
                if !t.instrument.update(kind) {
                    t.instrument = Instrument::new(kind, self.sample_rate);
                }
            }
            Command::SetSamples { track, mut samples } => {
                let Some(t) = self.tracks.get_mut(track) else { return };
                std::mem::swap(&mut t.samples, &mut samples);
                self.discard(Garbage::Samples(samples));
            }
            Command::SetGain { target, gain } => {
                if let Some(c) = self.channel(target) {
                    c.gain = gain.max(0.0);
                }
            }
            Command::SetPan { target, pan } => {
                if let Some(c) = self.channel(target) {
                    c.pan = pan.clamp(-1.0, 1.0);
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
                if let Some(s) = self.tracks.get_mut(track).and_then(|t| t.sends.get_mut(send)) {
                    *s = amount.clamp(0.0, 1.0);
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
                if let Some(slot) = self.channel(target).and_then(|c| c.chain.get_mut(index)) {
                    slot.enabled = device.enabled;
                    slot.processor.set_params(&device.effect);
                }
            }
            Command::ReplaceChain { target, mut chain } => {
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
            Command::RecordClip { track, slot } => {
                if let Some(t) = self.tracks.get_mut(track) {
                    if slot < NUM_SCENES && t.slots[slot].is_none() {
                        t.queued = Queued::Record(slot);
                        self.start_transport();
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
            let n = (frames - done).min(MAX_BLOCK);
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
        if self.playing {
            self.apply_automation();
            self.sequence(n, beats_per_sample);
        }

        for channel in self.returns.iter_mut().chain([&mut self.master]) {
            channel.left[..n].fill(0.0);
            channel.right[..n].fill(0.0);
        }

        let any_solo = self.tracks.iter().any(|t| t.solo);
        for (index, track) in self.tracks.iter_mut().enumerate() {
            let (left, right) = (&mut track.buffer[..n], &mut track.buffer_right[..n]);
            left.fill(0.0);
            right.fill(0.0);
            track.events.sort_unstable_by_key(|&(frame, rank, _)| (frame, rank));
            track.instrument.set_tempo(self.tempo);
            let mut cursor = 0;
            for &(frame, _, event) in &track.events {
                let frame = frame.min(n - 1);
                if frame > cursor {
                    track.instrument.render(&track.samples, &mut left[cursor..frame], &mut right[cursor..frame]);
                    cursor = frame;
                }
                match event {
                    Event::AllNotesOff => track.instrument.all_notes_off(),
                    Event::NoteOff { pitch } => track.instrument.note_off(pitch),
                    Event::NoteOn { pitch, velocity, gate } => track.instrument.note_on(&track.samples, pitch, velocity, gate),
                }
            }
            track.instrument.render(&track.samples, &mut left[cursor..], &mut right[cursor..]);
            track.events.clear();

            let channel = &mut track.channel;
            for i in 0..n {
                channel.left[i] = left[i] + track.audio_left[i];
                channel.right[i] = right[i] + track.audio_right[i];
            }
            channel.run_chain(n, &ctx);

            let audible = !channel.mute && (!any_solo || track.solo);
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
            let peak = channel.mix_into(n, gain, &mut self.master.left, &mut self.master.right);
            self.shared.tracks[index].peak.publish(peak);
        }

        for (index, ret) in self.returns.iter_mut().enumerate() {
            ret.run_chain(n, &ctx);
            let gain = if ret.mute { 0.0 } else { ret.gain };
            let peak = ret.mix_into(n, gain, &mut self.master.left, &mut self.master.right);
            self.shared.returns[index].publish(peak);
        }

        let master = &mut self.master;
        master.run_chain(n, &ctx);
        let (gl, gr) = master.pan_gains(master.gain);
        let (l0, r0) = master.last_gains;
        master.last_gains = (gl, gr);
        let click = &mut self.click_buffer[..n];
        click.fill(0.0);
        self.click.render(click);
        let mut peak = 0f32;
        for i in 0..n {
            let t = (i + 1) as f32 / n as f32;
            let l = master.left[i] * (l0 + (gl - l0) * t) + click[i];
            let r = master.right[i] * (r0 + (gr - r0) * t) + click[i];
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
            let (sends, instrument) = (&mut track.sends, &mut track.instrument);
            track.channel.automate(beat, |param, value| match param {
                Param::Send(i) => {
                    if let Some(send) = sends.get_mut(i) {
                        *send = value.clamp(0.0, 1.0);
                    }
                }
                Param::Instrument(i) => instrument.set_param(i, value),
                _ => {}
            });
        }
        for channel in self.returns.iter_mut().chain([&mut self.master]) {
            channel.automate(beat, |_, _| {});
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
            };
            self.sequence_segment(start, end, &timing);
            frame += frames;
            self.position = end;

            if wraps {
                let length = self.loop_end - self.loop_start;
                self.position = self.loop_start;
                let wrap_frame = timing.frame(end);
                for track in &mut self.tracks {
                    // Session clips keep their phase across the jump.
                    track.launch_beat -= length;
                    if track.playing.is_none() {
                        push_event(&mut track.events, wrap_frame, Event::AllNotesOff);
                    }
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AudioClip, Device, Effect, InstrumentKind, MidiClip, Note, WarpMode};
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
            assert!(self.live.push(LiveNote { pitch, velocity }).is_ok());
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
        h.send(Command::SetInstrument { track: 0, kind: InstrumentKind::Drums });
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
        Box::new(vec![PlacedClip { start: beat, length: 4.0, clip: PlayClip::Midi(clip) }])
    }

    /// One second of a 440 Hz sine at 48 kHz: two beats at 120 BPM.
    fn sine_sample() -> Arc<Sample> {
        let left: Vec<f32> = (0..48_000).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5).collect();
        Arc::new(Sample::from_channels(left.clone(), left, 48_000.0))
    }

    fn audio_clip(warp: WarpMode) -> AudioClip {
        AudioClip {
            name: "sine".into(),
            color: [0, 0, 0],
            sample: "sine.wav".into(),
            length: 2.0,
            offset: 0.0,
            warp,
            original_bpm: 120.0,
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
                points: vec![AutomationPoint { beat: 0.0, value: 0.0 }, AutomationPoint { beat: 4.0, value: 1.0 }],
            },
            AutomationLane {
                param: Param::Send(0),
                points: vec![AutomationPoint { beat: 1.99, value: 0.0 }, AutomationPoint { beat: 2.0, value: 1.0 }],
            },
            AutomationLane {
                param: Param::Device { device: 0, param: 0 },
                points: vec![AutomationPoint { beat: 0.0, value: 100.0 }],
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
            h.send(Command::SetInstrument { track: 0, kind: InstrumentKind::Synth(params) });
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

    #[test]
    fn arrangement_audio_clips_stop_at_their_end() {
        let mut h = Harness::new();
        let placed = PlacedClip { start: 0.0, length: 1.0, clip: PlayClip::Audio(audio_clip(WarpMode::Repitch), sine_sample()) };
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
