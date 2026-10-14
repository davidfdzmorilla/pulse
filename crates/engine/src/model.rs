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
    /// Synthesized drum kit, General MIDI mapping (36 kick, 38 snare, 39 clap, 42/46 hats).
    Drums,
}

pub mod drum_note {
    pub const KICK: u8 = 36;
    pub const SNARE: u8 = 38;
    pub const CLAP: u8 = 39;
    pub const CLOSED_HAT: u8 = 42;
    pub const OPEN_HAT: u8 = 46;
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
pub enum Effect {
    Eq3(Eq3Params),
    Compressor(CompressorParams),
    Delay(DelayParams),
    Reverb(ReverbParams),
    Saturator(SaturatorParams),
    AutoFilter(AutoFilterParams),
}

impl Effect {
    /// Every effect type with default settings, in browser order.
    pub fn catalog() -> [Effect; 6] {
        [
            Effect::Eq3(Default::default()),
            Effect::Compressor(Default::default()),
            Effect::Saturator(Default::default()),
            Effect::AutoFilter(Default::default()),
            Effect::Delay(Default::default()),
            Effect::Reverb(Default::default()),
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
