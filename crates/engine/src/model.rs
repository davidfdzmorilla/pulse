//! Plain data types shared between the UI and the audio thread.

use serde::{Deserialize, Serialize};

/// A MIDI note inside a clip. Times are in beats, relative to the clip start.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub start: f64,
    pub length: f64,
    pub pitch: u8,
    pub velocity: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MidiClip {
    pub name: String,
    /// Loop length in beats.
    pub length: f64,
    pub notes: Vec<Note>,
    pub color: [u8; 3],
}

impl MidiClip {
    pub fn new(name: impl Into<String>, length: f64, color: [u8; 3]) -> Self {
        Self {
            name: name.into(),
            length,
            notes: Vec::new(),
            color,
        }
    }
}

/// How an audio clip follows the project tempo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarpMode {
    /// Plays at the file's own speed.
    Off,
    /// Speed follows the tempo, so pitch changes with it (like a turntable).
    Repitch,
    /// Follows the tempo while keeping the pitch (granular time-stretch).
    Stretch,
}

impl WarpMode {
    pub const ALL: [WarpMode; 3] = [Self::Off, Self::Repitch, Self::Stretch];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Repitch => "Re-Pitch",
            Self::Stretch => "Stretch",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioClip {
    pub name: String,
    pub color: [u8; 3],
    /// Path of the audio file.
    pub sample: String,
    /// Loop length in beats.
    pub length: f64,
    /// Where the clip starts inside the file, in seconds.
    pub offset: f64,
    pub warp: WarpMode,
    /// Tempo the file was recorded at; used by Re-Pitch and Stretch.
    pub original_bpm: f64,
    pub gain_db: f32,
    /// Semitones; applies in Stretch mode.
    pub transpose: f32,
}

/// A session or arrangement clip. Serialized untagged so projects saved before
/// audio clips existed (plain MIDI clips) still load.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Clip {
    Midi(MidiClip),
    Audio(AudioClip),
}

impl Clip {
    pub fn name(&self) -> &str {
        match self {
            Clip::Midi(c) => &c.name,
            Clip::Audio(c) => &c.name,
        }
    }

    pub fn name_mut(&mut self) -> &mut String {
        match self {
            Clip::Midi(c) => &mut c.name,
            Clip::Audio(c) => &mut c.name,
        }
    }

    pub fn color(&self) -> [u8; 3] {
        match self {
            Clip::Midi(c) => c.color,
            Clip::Audio(c) => c.color,
        }
    }

    pub fn color_mut(&mut self) -> &mut [u8; 3] {
        match self {
            Clip::Midi(c) => &mut c.color,
            Clip::Audio(c) => &mut c.color,
        }
    }

    /// Loop length in beats.
    pub fn length(&self) -> f64 {
        match self {
            Clip::Midi(c) => c.length,
            Clip::Audio(c) => c.length,
        }
    }

    pub fn as_midi(&self) -> Option<&MidiClip> {
        match self {
            Clip::Midi(c) => Some(c),
            Clip::Audio(_) => None,
        }
    }
}

/// A clip placed on the arrangement timeline. Its content loops if `length`
/// is longer than the clip itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArrangementClip {
    pub start: f64,
    pub length: f64,
    pub clip: Clip,
}

impl ArrangementClip {
    pub fn end(&self) -> f64 {
        self.start + self.length
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Waveform {
    Sine,
    Triangle,
    Saw,
    Square,
}

impl Waveform {
    pub const ALL: [Waveform; 4] = [Self::Sine, Self::Triangle, Self::Saw, Self::Square];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sine => "Sine",
            Self::Triangle => "Triangle",
            Self::Saw => "Saw",
            Self::Square => "Square",
        }
    }
}

/// Parameters of the subtractive polyphonic synth. Envelope times are in seconds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SynthParams {
    pub waveform: Waveform,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    /// Low-pass cutoff in Hz.
    pub cutoff: f32,
    /// 0.0 (none) .. 1.0 (self-oscillating edge).
    pub resonance: f32,
    pub gain: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            waveform: Waveform::Saw,
            attack: 0.005,
            decay: 0.2,
            sustain: 0.6,
            release: 0.2,
            cutoff: 4000.0,
            resonance: 0.2,
            gain: 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum InstrumentKind {
    Synth(SynthParams),
    /// The first drum kit, without parameters. Loaded projects are upgraded to
    /// [`InstrumentKind::DrumMachine`]; the engine plays it as one with defaults.
    Drums,
    DrumMachine(DrumParams),
    SuperSaw(SuperSawParams),
    Acid(AcidParams),
    Fm(FmParams),
    /// Plays one sample chromatically; the file is the track's first sample.
    Sampler(SamplerParams),
    /// Sixteen pads, each playing one of the track's samples.
    DrumRack(RackParams),
}

/// Parameters of the sampler. The sample itself lives on the track.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SamplerParams {
    /// Plays the whole sample regardless of note length.
    pub one_shot: bool,
    /// Loops from the start point while the note is held.
    pub looping: bool,
    /// MIDI note at which the sample plays at its original pitch.
    pub root: f32,
    pub tune: f32,
    /// Start point as a fraction of the sample length.
    pub start: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub cutoff: f32,
    pub resonance: f32,
    pub gain: f32,
}

impl Default for SamplerParams {
    fn default() -> Self {
        Self {
            one_shot: false,
            looping: false,
            root: 60.0,
            tune: 0.0,
            start: 0.0,
            attack: 0.001,
            decay: 1.0,
            sustain: 1.0,
            release: 0.1,
            cutoff: 20000.0,
            resonance: 0.0,
            gain: 0.7,
        }
    }
}

pub const RACK_PADS: usize = 16;
/// MIDI note of the first pad (C1, the General MIDI kick); pads go up chromatically.
pub const RACK_FIRST_NOTE: u8 = 36;
/// Pad lengths at the top of the range play the whole sample.
pub const RACK_FULL_LENGTH: f32 = 10.0;

/// Per-pad settings of the drum rack.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RackParams {
    pub tune: [f32; RACK_PADS],
    /// Seconds before the pad fades out; [`RACK_FULL_LENGTH`] plays the whole sample.
    pub length: [f32; RACK_PADS],
    pub level: [f32; RACK_PADS],
    pub pan: [f32; RACK_PADS],
    /// Pads sharing a non-zero group cut each other off (like open and closed hats).
    pub choke: [u8; RACK_PADS],
}

impl Default for RackParams {
    fn default() -> Self {
        Self {
            tune: [0.0; RACK_PADS],
            length: [RACK_FULL_LENGTH; RACK_PADS],
            level: [0.8; RACK_PADS],
            pan: [0.0; RACK_PADS],
            choke: [0; RACK_PADS],
        }
    }
}

/// The drum rack pad that `pitch` triggers.
pub fn rack_pad(pitch: u8) -> Option<usize> {
    let pad = pitch.checked_sub(RACK_FIRST_NOTE)? as usize;
    (pad < RACK_PADS).then_some(pad)
}

pub mod drum_note {
    pub const KICK: u8 = 36;
    pub const RIM: u8 = 37;
    pub const SNARE: u8 = 38;
    pub const CLAP: u8 = 39;
    pub const CLOSED_HAT: u8 = 42;
    pub const LOW_TOM: u8 = 45;
    pub const OPEN_HAT: u8 = 46;
    pub const COWBELL: u8 = 56;
}

/// The drum machine's sounds, in parameter order, with their General MIDI notes.
pub const DRUM_VOICES: [(u8, &str); 8] = [
    (drum_note::KICK, "Kick"),
    (drum_note::RIM, "Rim"),
    (drum_note::SNARE, "Snare"),
    (drum_note::CLAP, "Clap"),
    (drum_note::CLOSED_HAT, "Closed Hat"),
    (drum_note::OPEN_HAT, "Open Hat"),
    (drum_note::LOW_TOM, "Tom"),
    (drum_note::COWBELL, "Cowbell"),
];

/// Per-sound tuning (semitones), decay (multiplier) and level of the drum machine.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrumParams {
    pub tune: [f32; 8],
    pub decay: [f32; 8],
    pub level: [f32; 8],
}

impl Default for DrumParams {
    fn default() -> Self {
        Self { tune: [0.0; 8], decay: [1.0; 8], level: [0.8; 8] }
    }
}

/// Unison sawtooth polysynth.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SuperSawParams {
    /// Saws per note, 1..7.
    pub voices: f32,
    /// Spread between the saws, 0..1 (up to ±50 cents).
    pub detune: f32,
    /// Stereo width of the saws, 0..1.
    pub spread: f32,
    pub cutoff: f32,
    pub resonance: f32,
    /// Filter envelope depth in octaves.
    pub env_amount: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub gain: f32,
}

impl Default for SuperSawParams {
    fn default() -> Self {
        Self {
            voices: 7.0,
            detune: 0.35,
            spread: 0.8,
            cutoff: 6000.0,
            resonance: 0.1,
            env_amount: 0.0,
            attack: 0.01,
            decay: 0.4,
            sustain: 0.8,
            release: 0.35,
            gain: 0.35,
        }
    }
}

/// Monophonic acid bassline synth with a resonant 4-pole filter and slides.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcidParams {
    pub square: bool,
    pub cutoff: f32,
    pub resonance: f32,
    /// Filter envelope depth, 0..1 (up to 4 octaves).
    pub env_mod: f32,
    /// Filter envelope decay in seconds.
    pub decay: f32,
    /// Extra punch for notes with velocity above 80%.
    pub accent: f32,
    /// Slide time in milliseconds between overlapping notes.
    pub glide_ms: f32,
    pub drive: f32,
    pub gain: f32,
}

impl Default for AcidParams {
    fn default() -> Self {
        Self {
            square: false,
            cutoff: 400.0,
            resonance: 0.75,
            env_mod: 0.6,
            decay: 0.3,
            accent: 0.6,
            glide_ms: 60.0,
            drive: 0.3,
            gain: 0.5,
        }
    }
}

/// Two-operator FM synth: a modulator with feedback driving a sine carrier.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FmParams {
    /// Modulator frequency as a multiple of the note's.
    pub ratio: f32,
    /// Modulation depth.
    pub index: f32,
    pub mod_decay: f32,
    pub mod_sustain: f32,
    pub feedback: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub gain: f32,
}

impl Default for FmParams {
    fn default() -> Self {
        Self {
            ratio: 2.0,
            index: 3.0,
            mod_decay: 0.6,
            mod_sustain: 0.2,
            feedback: 0.0,
            attack: 0.002,
            decay: 1.2,
            sustain: 0.0,
            release: 0.6,
            gain: 0.45,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Eq3Params {
    pub low_db: f32,
    pub mid_db: f32,
    pub high_db: f32,
    pub low_freq: f32,
    pub high_freq: f32,
}

impl Default for Eq3Params {
    fn default() -> Self {
        Self {
            low_db: 0.0,
            mid_db: 0.0,
            high_db: 0.0,
            low_freq: 250.0,
            high_freq: 2500.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompressorParams {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
}

impl Default for CompressorParams {
    fn default() -> Self {
        Self {
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 120.0,
            makeup_db: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DelayParams {
    /// Delay time in beats, synced to the tempo.
    pub time_beats: f64,
    pub feedback: f32,
    pub mix: f32,
    pub ping_pong: bool,
    /// Low-pass in the feedback path, Hz.
    pub filter_hz: f32,
}

impl Default for DelayParams {
    fn default() -> Self {
        Self {
            time_beats: 0.75,
            feedback: 0.4,
            mix: 0.3,
            ping_pong: true,
            filter_hz: 5000.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReverbParams {
    pub size: f32,
    pub damping: f32,
    pub width: f32,
    pub mix: f32,
}

impl Default for ReverbParams {
    fn default() -> Self {
        Self {
            size: 0.75,
            damping: 0.5,
            width: 1.0,
            mix: 0.3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SaturatorParams {
    pub drive_db: f32,
    pub output_db: f32,
    pub mix: f32,
}

impl Default for SaturatorParams {
    fn default() -> Self {
        Self {
            drive_db: 6.0,
            output_db: -3.0,
            mix: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterMode {
    LowPass,
    HighPass,
    BandPass,
}

impl FilterMode {
    pub const ALL: [FilterMode; 3] = [Self::LowPass, Self::HighPass, Self::BandPass];

    pub fn label(self) -> &'static str {
        match self {
            Self::LowPass => "Low-pass",
            Self::HighPass => "High-pass",
            Self::BandPass => "Band-pass",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutoFilterParams {
    pub mode: FilterMode,
    pub cutoff: f32,
    pub resonance: f32,
    /// LFO period in beats, synced to the transport.
    pub lfo_beats: f64,
    /// LFO depth in octaves.
    pub lfo_amount: f32,
}

impl Default for AutoFilterParams {
    fn default() -> Self {
        Self {
            mode: FilterMode::LowPass,
            cutoff: 1500.0,
            resonance: 0.3,
            lfo_beats: 4.0,
            lfo_amount: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChorusParams {
    pub rate: f32,
    pub depth: f32,
    pub delay_ms: f32,
    pub mix: f32,
}

impl Default for ChorusParams {
    fn default() -> Self {
        Self { rate: 0.6, depth: 0.5, delay_ms: 8.0, mix: 0.5 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhaserParams {
    pub rate: f32,
    pub depth: f32,
    pub center: f32,
    pub feedback: f32,
    pub mix: f32,
}

impl Default for PhaserParams {
    fn default() -> Self {
        Self { rate: 0.4, depth: 0.7, center: 800.0, feedback: 0.5, mix: 0.5 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BitcrusherParams {
    pub bits: f32,
    /// Holds each sample this many times (sample-rate reduction).
    pub downsample: f32,
    pub mix: f32,
}

impl Default for BitcrusherParams {
    fn default() -> Self {
        Self { bits: 8.0, downsample: 4.0, mix: 1.0 }
    }
}

/// Tempo-synced volume ducking, the "sidechain pump" without a sidechain.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DuckerParams {
    pub depth: f32,
    /// Fraction of each period spent recovering, 0.05..1.
    pub release: f32,
    /// Ducking period in beats (1 = every quarter note).
    pub rate_beats: f64,
}

impl Default for DuckerParams {
    fn default() -> Self {
        Self { depth: 0.7, release: 0.6, rate_beats: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LimiterParams {
    pub gain_db: f32,
    pub ceiling_db: f32,
    pub release_ms: f32,
}

impl Default for LimiterParams {
    fn default() -> Self {
        Self { gain_db: 0.0, ceiling_db: -0.3, release_ms: 120.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UtilityParams {
    pub gain_db: f32,
    /// 0 = mono, 1 = unchanged, 2 = extra wide.
    pub width: f32,
}

impl Default for UtilityParams {
    fn default() -> Self {
        Self { gain_db: 0.0, width: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Eq3(Eq3Params),
    Compressor(CompressorParams),
    Delay(DelayParams),
    Reverb(ReverbParams),
    Saturator(SaturatorParams),
    AutoFilter(AutoFilterParams),
    Chorus(ChorusParams),
    Phaser(PhaserParams),
    Bitcrusher(BitcrusherParams),
    Ducker(DuckerParams),
    Limiter(LimiterParams),
    Utility(UtilityParams),
}

impl Effect {
    /// Every effect type with default settings, in browser order.
    pub fn catalog() -> [Effect; 12] {
        [
            Effect::Eq3(Default::default()),
            Effect::Compressor(Default::default()),
            Effect::Limiter(Default::default()),
            Effect::Saturator(Default::default()),
            Effect::Bitcrusher(Default::default()),
            Effect::AutoFilter(Default::default()),
            Effect::Ducker(Default::default()),
            Effect::Chorus(Default::default()),
            Effect::Phaser(Default::default()),
            Effect::Delay(Default::default()),
            Effect::Reverb(Default::default()),
            Effect::Utility(Default::default()),
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Effect::Eq3(_) => "EQ Three",
            Effect::Compressor(_) => "Compressor",
            Effect::Delay(_) => "Delay",
            Effect::Reverb(_) => "Reverb",
            Effect::Saturator(_) => "Saturator",
            Effect::AutoFilter(_) => "Auto Filter",
            Effect::Chorus(_) => "Chorus",
            Effect::Phaser(_) => "Phaser",
            Effect::Bitcrusher(_) => "Bitcrusher",
            Effect::Ducker(_) => "Ducker",
            Effect::Limiter(_) => "Limiter",
            Effect::Utility(_) => "Utility",
        }
    }
}

/// An effect in a device chain.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub effect: Effect,
    pub enabled: bool,
}

impl Device {
    pub fn new(effect: Effect) -> Self {
        Self {
            effect,
            enabled: true,
        }
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    if db <= -70.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

pub fn gain_to_db(gain: f32) -> f32 {
    if gain <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * gain.log10()
    }
}

// ----- Parameters and automation -----------------------------------------------------

/// How a parameter value is displayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Decibels,
    Hertz,
    Percent,
    Seconds,
    Milliseconds,
    Ratio,
    Octaves,
    Pan,
    Semitones,
    /// A multiplier, shown as "×1.5".
    Times,
    Bits,
    Count,
    /// A MIDI note number, shown as "C3".
    Note,
    Plain,
}

/// Description of a continuous parameter: what knobs and automation lanes need to know.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamInfo {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// Logarithmic travel (frequencies, times, ratios).
    pub log: bool,
    pub unit: Unit,
}

impl ParamInfo {
    const fn linear(name: &'static str, min: f32, max: f32, default: f32, unit: Unit) -> Self {
        Self { name, min, max, default, log: false, unit }
    }

    const fn log(name: &'static str, min: f32, max: f32, default: f32, unit: Unit) -> Self {
        Self { name, min, max, default, log: true, unit }
    }

    /// Position of `value` along the control's travel, 0..1.
    pub fn normalize(&self, value: f32) -> f32 {
        let n = if self.log {
            (value / self.min).ln() / (self.max / self.min).ln()
        } else {
            (value - self.min) / (self.max - self.min)
        };
        n.clamp(0.0, 1.0)
    }

    pub fn denormalize(&self, n: f32) -> f32 {
        let n = n.clamp(0.0, 1.0);
        if self.log {
            self.min * (self.max / self.min).powf(n)
        } else {
            self.min + n * (self.max - self.min)
        }
    }
}

use Unit::*;

const EQ3_PARAMS: [ParamInfo; 5] = [
    ParamInfo::linear("Low", -40.0, 6.0, 0.0, Decibels),
    ParamInfo::linear("Mid", -40.0, 6.0, 0.0, Decibels),
    ParamInfo::linear("High", -40.0, 6.0, 0.0, Decibels),
    ParamInfo::log("FreqLo", 50.0, 1000.0, 250.0, Hertz),
    ParamInfo::log("FreqHi", 800.0, 12000.0, 2500.0, Hertz),
];
const COMPRESSOR_PARAMS: [ParamInfo; 5] = [
    ParamInfo::linear("Threshold", -60.0, 0.0, -18.0, Decibels),
    ParamInfo::log("Ratio", 1.0, 20.0, 4.0, Ratio),
    ParamInfo::log("Attack", 0.1, 200.0, 10.0, Milliseconds),
    ParamInfo::log("Release", 5.0, 2000.0, 120.0, Milliseconds),
    ParamInfo::linear("Makeup", 0.0, 24.0, 0.0, Decibels),
];
const DELAY_PARAMS: [ParamInfo; 3] = [
    ParamInfo::linear("Feedback", 0.0, 0.95, 0.4, Percent),
    ParamInfo::log("Filter", 500.0, 18000.0, 5000.0, Hertz),
    ParamInfo::linear("Dry/Wet", 0.0, 1.0, 0.3, Percent),
];
const REVERB_PARAMS: [ParamInfo; 4] = [
    ParamInfo::linear("Size", 0.0, 1.0, 0.75, Percent),
    ParamInfo::linear("Damping", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Width", 0.0, 1.0, 1.0, Percent),
    ParamInfo::linear("Dry/Wet", 0.0, 1.0, 0.3, Percent),
];
const SATURATOR_PARAMS: [ParamInfo; 3] = [
    ParamInfo::linear("Drive", 0.0, 36.0, 6.0, Decibels),
    ParamInfo::linear("Output", -36.0, 6.0, -3.0, Decibels),
    ParamInfo::linear("Dry/Wet", 0.0, 1.0, 1.0, Percent),
];
const AUTO_FILTER_PARAMS: [ParamInfo; 3] = [
    ParamInfo::log("Freq", 30.0, 18000.0, 1500.0, Hertz),
    ParamInfo::linear("Res", 0.0, 0.95, 0.3, Percent),
    ParamInfo::linear("LFO Amount", 0.0, 4.0, 0.0, Octaves),
];
const CHORUS_PARAMS: [ParamInfo; 4] = [
    ParamInfo::log("Rate", 0.05, 5.0, 0.6, Hertz),
    ParamInfo::linear("Depth", 0.0, 1.0, 0.5, Percent),
    ParamInfo::log("Delay", 2.0, 20.0, 8.0, Milliseconds),
    ParamInfo::linear("Dry/Wet", 0.0, 1.0, 0.5, Percent),
];
const PHASER_PARAMS: [ParamInfo; 5] = [
    ParamInfo::log("Rate", 0.05, 8.0, 0.4, Hertz),
    ParamInfo::linear("Depth", 0.0, 1.0, 0.7, Percent),
    ParamInfo::log("Freq", 100.0, 4000.0, 800.0, Hertz),
    ParamInfo::linear("Feedback", 0.0, 0.9, 0.5, Percent),
    ParamInfo::linear("Dry/Wet", 0.0, 1.0, 0.5, Percent),
];
const BITCRUSHER_PARAMS: [ParamInfo; 3] = [
    ParamInfo::linear("Bits", 2.0, 16.0, 8.0, Bits),
    ParamInfo::log("Downsample", 1.0, 40.0, 4.0, Times),
    ParamInfo::linear("Dry/Wet", 0.0, 1.0, 1.0, Percent),
];
const DUCKER_PARAMS: [ParamInfo; 2] = [
    ParamInfo::linear("Depth", 0.0, 1.0, 0.7, Percent),
    ParamInfo::linear("Release", 0.05, 1.0, 0.6, Percent),
];
const LIMITER_PARAMS: [ParamInfo; 3] = [
    ParamInfo::linear("Gain", 0.0, 24.0, 0.0, Decibels),
    ParamInfo::linear("Ceiling", -12.0, 0.0, -0.3, Decibels),
    ParamInfo::log("Release", 10.0, 1000.0, 120.0, Milliseconds),
];
const UTILITY_PARAMS: [ParamInfo; 2] = [
    ParamInfo::linear("Gain", -36.0, 12.0, 0.0, Decibels),
    ParamInfo::linear("Width", 0.0, 2.0, 1.0, Percent),
];
const SUPERSAW_PARAMS: [ParamInfo; 11] = [
    ParamInfo::linear("Voices", 1.0, 7.0, 7.0, Count),
    ParamInfo::linear("Detune", 0.0, 1.0, 0.35, Percent),
    ParamInfo::linear("Spread", 0.0, 1.0, 0.8, Percent),
    ParamInfo::log("Filter", 40.0, 18000.0, 6000.0, Hertz),
    ParamInfo::linear("Res", 0.0, 0.95, 0.1, Percent),
    ParamInfo::linear("Env Amt", 0.0, 4.0, 0.0, Octaves),
    ParamInfo::log("Attack", 0.001, 4.0, 0.01, Seconds),
    ParamInfo::log("Decay", 0.005, 4.0, 0.4, Seconds),
    ParamInfo::linear("Sustain", 0.0, 1.0, 0.8, Percent),
    ParamInfo::log("Release", 0.005, 4.0, 0.35, Seconds),
    ParamInfo::linear("Volume", 0.0, 1.0, 0.35, Percent),
];
const ACID_PARAMS: [ParamInfo; 8] = [
    ParamInfo::log("Cutoff", 50.0, 5000.0, 400.0, Hertz),
    ParamInfo::linear("Res", 0.0, 0.97, 0.75, Percent),
    ParamInfo::linear("Env Mod", 0.0, 1.0, 0.6, Percent),
    ParamInfo::log("Decay", 0.05, 2.0, 0.3, Seconds),
    ParamInfo::linear("Accent", 0.0, 1.0, 0.6, Percent),
    ParamInfo::log("Glide", 1.0, 500.0, 60.0, Milliseconds),
    ParamInfo::linear("Drive", 0.0, 1.0, 0.3, Percent),
    ParamInfo::linear("Volume", 0.0, 1.0, 0.5, Percent),
];
const FM_PARAMS: [ParamInfo; 10] = [
    ParamInfo::log("Ratio", 0.25, 16.0, 2.0, Times),
    ParamInfo::linear("Index", 0.0, 10.0, 3.0, Plain),
    ParamInfo::log("Mod Decay", 0.01, 4.0, 0.6, Seconds),
    ParamInfo::linear("Mod Sus", 0.0, 1.0, 0.2, Percent),
    ParamInfo::linear("Feedback", 0.0, 1.0, 0.0, Percent),
    ParamInfo::log("Attack", 0.001, 4.0, 0.002, Seconds),
    ParamInfo::log("Decay", 0.005, 8.0, 1.2, Seconds),
    ParamInfo::linear("Sustain", 0.0, 1.0, 0.0, Percent),
    ParamInfo::log("Release", 0.005, 4.0, 0.6, Seconds),
    ParamInfo::linear("Volume", 0.0, 1.0, 0.45, Percent),
];
const SAMPLER_PARAMS: [ParamInfo; 10] = [
    ParamInfo::linear("Root", 0.0, 127.0, 60.0, Note),
    ParamInfo::linear("Tune", -24.0, 24.0, 0.0, Semitones),
    ParamInfo::linear("Start", 0.0, 0.99, 0.0, Percent),
    ParamInfo::log("Attack", 0.001, 4.0, 0.001, Seconds),
    ParamInfo::log("Decay", 0.005, 8.0, 1.0, Seconds),
    ParamInfo::linear("Sustain", 0.0, 1.0, 1.0, Percent),
    ParamInfo::log("Release", 0.005, 8.0, 0.1, Seconds),
    ParamInfo::log("Filter", 40.0, 20000.0, 20000.0, Hertz),
    ParamInfo::linear("Res", 0.0, 0.95, 0.0, Percent),
    ParamInfo::linear("Volume", 0.0, 1.0, 0.7, Percent),
];
/// Pad parameters, four per pad: "Pad 1 Tune", "Pad 1 Length", …
const RACK_PARAMS: [ParamInfo; RACK_PADS * 4] = {
    const NAMES: [[&str; 4]; RACK_PADS] = {
        // Built by hand because `format!` is not const.
        [
            ["Pad 1 Tune", "Pad 1 Length", "Pad 1 Level", "Pad 1 Pan"],
            ["Pad 2 Tune", "Pad 2 Length", "Pad 2 Level", "Pad 2 Pan"],
            ["Pad 3 Tune", "Pad 3 Length", "Pad 3 Level", "Pad 3 Pan"],
            ["Pad 4 Tune", "Pad 4 Length", "Pad 4 Level", "Pad 4 Pan"],
            ["Pad 5 Tune", "Pad 5 Length", "Pad 5 Level", "Pad 5 Pan"],
            ["Pad 6 Tune", "Pad 6 Length", "Pad 6 Level", "Pad 6 Pan"],
            ["Pad 7 Tune", "Pad 7 Length", "Pad 7 Level", "Pad 7 Pan"],
            ["Pad 8 Tune", "Pad 8 Length", "Pad 8 Level", "Pad 8 Pan"],
            ["Pad 9 Tune", "Pad 9 Length", "Pad 9 Level", "Pad 9 Pan"],
            ["Pad 10 Tune", "Pad 10 Length", "Pad 10 Level", "Pad 10 Pan"],
            ["Pad 11 Tune", "Pad 11 Length", "Pad 11 Level", "Pad 11 Pan"],
            ["Pad 12 Tune", "Pad 12 Length", "Pad 12 Level", "Pad 12 Pan"],
            ["Pad 13 Tune", "Pad 13 Length", "Pad 13 Level", "Pad 13 Pan"],
            ["Pad 14 Tune", "Pad 14 Length", "Pad 14 Level", "Pad 14 Pan"],
            ["Pad 15 Tune", "Pad 15 Length", "Pad 15 Level", "Pad 15 Pan"],
            ["Pad 16 Tune", "Pad 16 Length", "Pad 16 Level", "Pad 16 Pan"],
        ]
    };
    let mut table = [ParamInfo::linear("", 0.0, 1.0, 0.0, Plain); RACK_PADS * 4];
    let mut pad = 0;
    while pad < RACK_PADS {
        table[pad * 4] = ParamInfo::linear(NAMES[pad][0], -24.0, 24.0, 0.0, Semitones);
        table[pad * 4 + 1] = ParamInfo::log(NAMES[pad][1], 0.02, RACK_FULL_LENGTH, RACK_FULL_LENGTH, Seconds);
        table[pad * 4 + 2] = ParamInfo::linear(NAMES[pad][2], 0.0, 1.0, 0.8, Percent);
        table[pad * 4 + 3] = ParamInfo::linear(NAMES[pad][3], -1.0, 1.0, 0.0, Pan);
        pad += 1;
    }
    table
};
const DRUM_PARAMS: [ParamInfo; 24] = {
    let names = [
        ["Kick Tune", "Kick Decay", "Kick Level"],
        ["Rim Tune", "Rim Decay", "Rim Level"],
        ["Snare Tune", "Snare Decay", "Snare Level"],
        ["Clap Tune", "Clap Decay", "Clap Level"],
        ["CH Tune", "CH Decay", "CH Level"],
        ["OH Tune", "OH Decay", "OH Level"],
        ["Tom Tune", "Tom Decay", "Tom Level"],
        ["Bell Tune", "Bell Decay", "Bell Level"],
    ];
    let mut table = [ParamInfo::linear("", 0.0, 1.0, 0.0, Plain); 24];
    let mut voice = 0;
    while voice < 8 {
        table[voice * 3] = ParamInfo::linear(names[voice][0], -12.0, 12.0, 0.0, Semitones);
        table[voice * 3 + 1] = ParamInfo::log(names[voice][1], 0.25, 4.0, 1.0, Times);
        table[voice * 3 + 2] = ParamInfo::linear(names[voice][2], 0.0, 1.0, 0.8, Percent);
        voice += 1;
    }
    table
};

impl InstrumentKind {
    /// Every instrument, in browser order.
    pub fn catalog() -> [InstrumentKind; 7] {
        [
            InstrumentKind::Synth(SynthParams::default()),
            InstrumentKind::SuperSaw(SuperSawParams::default()),
            InstrumentKind::Acid(AcidParams::default()),
            InstrumentKind::Fm(FmParams::default()),
            InstrumentKind::Sampler(SamplerParams::default()),
            InstrumentKind::DrumMachine(DrumParams::default()),
            InstrumentKind::DrumRack(RackParams::default()),
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            InstrumentKind::Synth(_) => "Pulse Synth",
            InstrumentKind::Drums | InstrumentKind::DrumMachine(_) => "Drum Machine",
            InstrumentKind::SuperSaw(_) => "Super Saw",
            InstrumentKind::Acid(_) => "Acid Bass",
            InstrumentKind::Fm(_) => "FM Synth",
            InstrumentKind::Sampler(_) => "Sampler",
            InstrumentKind::DrumRack(_) => "Drum Rack",
        }
    }

    pub fn is_drums(&self) -> bool {
        matches!(self, InstrumentKind::Drums | InstrumentKind::DrumMachine(_) | InstrumentKind::DrumRack(_))
    }

    /// True for instruments that play the track's samples.
    pub fn uses_samples(&self) -> bool {
        matches!(self, InstrumentKind::Sampler(_) | InstrumentKind::DrumRack(_))
    }

    /// Replaces the parameterless first drum kit with the drum machine.
    pub fn upgraded(self) -> Self {
        match self {
            InstrumentKind::Drums => InstrumentKind::DrumMachine(DrumParams::default()),
            other => other,
        }
    }

    pub fn params(&self) -> &'static [ParamInfo] {
        match self {
            InstrumentKind::Synth(_) => &SYNTH_PARAMS,
            InstrumentKind::Drums => &[],
            InstrumentKind::DrumMachine(_) => &DRUM_PARAMS,
            InstrumentKind::SuperSaw(_) => &SUPERSAW_PARAMS,
            InstrumentKind::Acid(_) => &ACID_PARAMS,
            InstrumentKind::Fm(_) => &FM_PARAMS,
            InstrumentKind::Sampler(_) => &SAMPLER_PARAMS,
            InstrumentKind::DrumRack(_) => &RACK_PARAMS,
        }
    }

    fn param_mut(&mut self, index: usize) -> Option<&mut f32> {
        Some(match self {
            InstrumentKind::Synth(p) => return p.param_mut(index),
            InstrumentKind::Drums => return None,
            InstrumentKind::DrumMachine(p) => match index % 3 {
                0 => p.tune.get_mut(index / 3)?,
                1 => p.decay.get_mut(index / 3)?,
                _ => p.level.get_mut(index / 3)?,
            },
            InstrumentKind::SuperSaw(p) => match index {
                0 => &mut p.voices,
                1 => &mut p.detune,
                2 => &mut p.spread,
                3 => &mut p.cutoff,
                4 => &mut p.resonance,
                5 => &mut p.env_amount,
                6 => &mut p.attack,
                7 => &mut p.decay,
                8 => &mut p.sustain,
                9 => &mut p.release,
                10 => &mut p.gain,
                _ => return None,
            },
            InstrumentKind::Acid(p) => match index {
                0 => &mut p.cutoff,
                1 => &mut p.resonance,
                2 => &mut p.env_mod,
                3 => &mut p.decay,
                4 => &mut p.accent,
                5 => &mut p.glide_ms,
                6 => &mut p.drive,
                7 => &mut p.gain,
                _ => return None,
            },
            InstrumentKind::Fm(p) => match index {
                0 => &mut p.ratio,
                1 => &mut p.index,
                2 => &mut p.mod_decay,
                3 => &mut p.mod_sustain,
                4 => &mut p.feedback,
                5 => &mut p.attack,
                6 => &mut p.decay,
                7 => &mut p.sustain,
                8 => &mut p.release,
                9 => &mut p.gain,
                _ => return None,
            },
            InstrumentKind::Sampler(p) => match index {
                0 => &mut p.root,
                1 => &mut p.tune,
                2 => &mut p.start,
                3 => &mut p.attack,
                4 => &mut p.decay,
                5 => &mut p.sustain,
                6 => &mut p.release,
                7 => &mut p.cutoff,
                8 => &mut p.resonance,
                9 => &mut p.gain,
                _ => return None,
            },
            InstrumentKind::DrumRack(p) => match index % 4 {
                0 => p.tune.get_mut(index / 4)?,
                1 => p.length.get_mut(index / 4)?,
                2 => p.level.get_mut(index / 4)?,
                _ => p.pan.get_mut(index / 4)?,
            },
        })
    }

    pub fn get(&self, index: usize) -> f32 {
        let mut copy = *self;
        copy.param_mut(index).map_or(0.0, |v| *v)
    }

    pub fn set(&mut self, index: usize, value: f32) {
        let info = self.params().get(index).copied();
        if let (Some(info), Some(slot)) = (info, self.param_mut(index)) {
            *slot = value.clamp(info.min, info.max);
        }
    }
}

const SYNTH_PARAMS: [ParamInfo; 7] = [
    ParamInfo::log("Attack", 0.001, 4.0, 0.005, Seconds),
    ParamInfo::log("Decay", 0.005, 4.0, 0.2, Seconds),
    ParamInfo::linear("Sustain", 0.0, 1.0, 0.6, Percent),
    ParamInfo::log("Release", 0.005, 4.0, 0.2, Seconds),
    ParamInfo::log("Filter", 40.0, 18000.0, 4000.0, Hertz),
    ParamInfo::linear("Res", 0.0, 0.95, 0.2, Percent),
    ParamInfo::linear("Volume", 0.0, 1.0, 0.5, Percent),
];

impl Effect {
    /// The continuous (knob and automation) parameters of this effect.
    pub fn params(&self) -> &'static [ParamInfo] {
        match self {
            Effect::Eq3(_) => &EQ3_PARAMS,
            Effect::Compressor(_) => &COMPRESSOR_PARAMS,
            Effect::Delay(_) => &DELAY_PARAMS,
            Effect::Reverb(_) => &REVERB_PARAMS,
            Effect::Saturator(_) => &SATURATOR_PARAMS,
            Effect::AutoFilter(_) => &AUTO_FILTER_PARAMS,
            Effect::Chorus(_) => &CHORUS_PARAMS,
            Effect::Phaser(_) => &PHASER_PARAMS,
            Effect::Bitcrusher(_) => &BITCRUSHER_PARAMS,
            Effect::Ducker(_) => &DUCKER_PARAMS,
            Effect::Limiter(_) => &LIMITER_PARAMS,
            Effect::Utility(_) => &UTILITY_PARAMS,
        }
    }

    fn param_mut(&mut self, index: usize) -> Option<&mut f32> {
        Some(match (self, index) {
            (Effect::Eq3(p), 0) => &mut p.low_db,
            (Effect::Eq3(p), 1) => &mut p.mid_db,
            (Effect::Eq3(p), 2) => &mut p.high_db,
            (Effect::Eq3(p), 3) => &mut p.low_freq,
            (Effect::Eq3(p), 4) => &mut p.high_freq,
            (Effect::Compressor(p), 0) => &mut p.threshold_db,
            (Effect::Compressor(p), 1) => &mut p.ratio,
            (Effect::Compressor(p), 2) => &mut p.attack_ms,
            (Effect::Compressor(p), 3) => &mut p.release_ms,
            (Effect::Compressor(p), 4) => &mut p.makeup_db,
            (Effect::Delay(p), 0) => &mut p.feedback,
            (Effect::Delay(p), 1) => &mut p.filter_hz,
            (Effect::Delay(p), 2) => &mut p.mix,
            (Effect::Reverb(p), 0) => &mut p.size,
            (Effect::Reverb(p), 1) => &mut p.damping,
            (Effect::Reverb(p), 2) => &mut p.width,
            (Effect::Reverb(p), 3) => &mut p.mix,
            (Effect::Saturator(p), 0) => &mut p.drive_db,
            (Effect::Saturator(p), 1) => &mut p.output_db,
            (Effect::Saturator(p), 2) => &mut p.mix,
            (Effect::AutoFilter(p), 0) => &mut p.cutoff,
            (Effect::AutoFilter(p), 1) => &mut p.resonance,
            (Effect::AutoFilter(p), 2) => &mut p.lfo_amount,
            (Effect::Chorus(p), 0) => &mut p.rate,
            (Effect::Chorus(p), 1) => &mut p.depth,
            (Effect::Chorus(p), 2) => &mut p.delay_ms,
            (Effect::Chorus(p), 3) => &mut p.mix,
            (Effect::Phaser(p), 0) => &mut p.rate,
            (Effect::Phaser(p), 1) => &mut p.depth,
            (Effect::Phaser(p), 2) => &mut p.center,
            (Effect::Phaser(p), 3) => &mut p.feedback,
            (Effect::Phaser(p), 4) => &mut p.mix,
            (Effect::Bitcrusher(p), 0) => &mut p.bits,
            (Effect::Bitcrusher(p), 1) => &mut p.downsample,
            (Effect::Bitcrusher(p), 2) => &mut p.mix,
            (Effect::Ducker(p), 0) => &mut p.depth,
            (Effect::Ducker(p), 1) => &mut p.release,
            (Effect::Limiter(p), 0) => &mut p.gain_db,
            (Effect::Limiter(p), 1) => &mut p.ceiling_db,
            (Effect::Limiter(p), 2) => &mut p.release_ms,
            (Effect::Utility(p), 0) => &mut p.gain_db,
            (Effect::Utility(p), 1) => &mut p.width,
            _ => return None,
        })
    }

    pub fn get(&self, index: usize) -> f32 {
        let mut copy = *self;
        copy.param_mut(index).map_or(0.0, |v| *v)
    }

    pub fn set(&mut self, index: usize, value: f32) {
        let info = self.params().get(index).copied();
        if let (Some(slot), Some(info)) = (self.param_mut(index), info) {
            *slot = value.clamp(info.min, info.max);
        }
    }
}

impl SynthParams {
    pub fn params() -> &'static [ParamInfo] {
        &SYNTH_PARAMS
    }

    fn param_mut(&mut self, index: usize) -> Option<&mut f32> {
        Some(match index {
            0 => &mut self.attack,
            1 => &mut self.decay,
            2 => &mut self.sustain,
            3 => &mut self.release,
            4 => &mut self.cutoff,
            5 => &mut self.resonance,
            6 => &mut self.gain,
            _ => return None,
        })
    }

    pub fn get(&self, index: usize) -> f32 {
        let mut copy = *self;
        copy.param_mut(index).map_or(0.0, |v| *v)
    }

    pub fn set(&mut self, index: usize, value: f32) {
        if let (Some(info), Some(slot)) = (SYNTH_PARAMS.get(index).copied(), self.param_mut(index)) {
            *slot = value.clamp(info.min, info.max);
        }
    }
}

/// Mixer parameters of a channel.
pub const VOLUME_PARAM: ParamInfo = ParamInfo::linear("Volumen", 0.0, 2.0, 1.0, Decibels);
pub const PAN_PARAM: ParamInfo = ParamInfo::linear("Pan", -1.0, 1.0, 0.0, Pan);
pub const SEND_PARAM: ParamInfo = ParamInfo::linear("Envío", 0.0, 1.0, 0.0, Percent);

/// An automatable parameter of a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Param {
    /// Fader gain (linear).
    Volume,
    Pan,
    Send(usize),
    /// Parameter `param` of the device at `device` in the channel's chain.
    Device { device: usize, param: usize },
    /// Parameter of the track's synth.
    Instrument(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationPoint {
    pub beat: f64,
    pub value: f32,
}

/// Breakpoints for one parameter along the arrangement, kept sorted by beat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationLane {
    pub param: Param,
    pub points: Vec<AutomationPoint>,
}

impl AutomationLane {
    pub fn new(param: Param) -> Self {
        Self { param, points: Vec::new() }
    }

    /// Value at `beat`, linear between points and held before the first and after the last.
    pub fn value_at(&self, beat: f64) -> Option<f32> {
        let first = self.points.first()?;
        if beat <= first.beat {
            return Some(first.value);
        }
        let next = self.points.partition_point(|p| p.beat <= beat);
        let Some(b) = self.points.get(next) else {
            return self.points.last().map(|p| p.value);
        };
        let a = self.points[next - 1];
        let t = ((beat - a.beat) / (b.beat - a.beat).max(1e-9)) as f32;
        Some(a.value + (b.value - a.value) * t)
    }

    pub fn sort(&mut self) {
        self.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lanes_interpolate_and_hold_their_ends() {
        let mut lane = AutomationLane::new(Param::Volume);
        assert_eq!(lane.value_at(0.0), None);
        lane.points = vec![AutomationPoint { beat: 4.0, value: 0.0 }, AutomationPoint { beat: 8.0, value: 1.0 }];
        assert_eq!(lane.value_at(0.0), Some(0.0));
        assert_eq!(lane.value_at(6.0), Some(0.5));
        assert_eq!(lane.value_at(8.0), Some(1.0));
        assert_eq!(lane.value_at(100.0), Some(1.0));
    }

    #[test]
    fn every_parameter_round_trips_through_get_and_set() {
        for mut effect in Effect::catalog() {
            for (i, info) in effect.params().iter().enumerate() {
                effect.set(i, info.max);
                assert_eq!(effect.get(i), info.max, "{} {}", effect.label(), info.name);
                effect.set(i, info.max * 10.0 + 10.0);
                assert_eq!(effect.get(i), info.max, "values are clamped to the range");
            }
        }
        for mut kind in InstrumentKind::catalog() {
            for (i, info) in kind.params().iter().enumerate() {
                kind.set(i, info.max);
                assert_eq!(kind.get(i), info.max, "{} {}", kind.label(), info.name);
                kind.set(i, info.min);
                assert_eq!(kind.get(i), info.min, "{} {}", kind.label(), info.name);
            }
        }
        let mut synth = SynthParams::default();
        for (i, info) in SynthParams::params().iter().enumerate() {
            synth.set(i, info.min);
            assert_eq!(synth.get(i), info.min);
        }
    }

    #[test]
    fn log_parameters_normalize_symmetrically() {
        let info = EQ3_PARAMS[3];
        let mid = info.denormalize(0.5);
        assert!((info.normalize(mid) - 0.5).abs() < 1e-5);
        assert!((mid - (50.0f32 * 1000.0).sqrt()).abs() < 0.1);
    }
}
