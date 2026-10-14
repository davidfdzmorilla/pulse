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
    /// Kick drum synthesizer; an optional click sample is the track's first sample.
    Kick(KickParams),
    /// Wavetable synthesizer; user tables are the track's samples 0 (A) and 1 (B).
    Wavetable(WaveParams),
}

/// How the kick designer distorts its body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KickDrive {
    /// Smooth saturation (tanh).
    Soft,
    /// Hard clipping: squarer, louder.
    Hard,
    /// Wave folding: bright, metallic overtones.
    Fold,
    /// Asymmetric saturation: warm, with even harmonics.
    Tube,
    /// Bit reduction: gritty, digital.
    Crush,
}

/// Waveform of the kick designer's body oscillator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum KickWave {
    #[default]
    Sine,
    Triangle,
    Square,
    Saw,
}

impl KickWave {
    pub const ALL: [KickWave; 4] = [Self::Sine, Self::Triangle, Self::Square, Self::Saw];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sine => "Sine",
            Self::Triangle => "Triangle",
            Self::Square => "Square",
            Self::Saw => "Saw",
        }
    }
}

/// The kick designer's built-in clicks, all synthesized.
pub const KICK_CLICKS: [&str; 12] = ["Noise", "Snap", "Tick", "Square", "Zap", "Metal", "Thump", "Pulse", "Wood", "Air", "FM", "Crackle"];

/// Lengths the kick designer can lock to, in beats (0 is free).
pub const KICK_SYNC_BEATS: [f32; 6] = [0.0, 0.25, 0.5, 1.0, 2.0, 4.0];

impl KickDrive {
    pub const ALL: [KickDrive; 5] = [Self::Soft, Self::Hard, Self::Fold, Self::Tube, Self::Crush];

    pub fn label(self) -> &'static str {
        match self {
            Self::Soft => "Soft",
            Self::Hard => "Hard",
            Self::Fold => "Fold",
            Self::Tube => "Tube",
            Self::Crush => "Crush",
        }
    }
}

/// Kick drum synthesizer: a tuned sine body with pitch and amplitude envelopes,
/// a click layer and distortion. Times are in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct KickParams {
    /// The played note sets the body's pitch instead of `tune`.
    pub key_follow: bool,
    pub drive_mode: KickDrive,
    /// MIDI note the body settles on.
    pub tune: f32,
    /// Pitch at the start, in semitones above `tune`.
    pub pitch_amount: f32,
    pub pitch_time: f32,
    /// 0 sweeps linearly, 1 drops fast and settles slowly.
    pub pitch_curve: f32,
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    /// 0 fades linearly, 1 drops fast with a long, quiet tail.
    pub decay_curve: f32,
    /// Shapes the sine towards a triangle-like wave with more overtones.
    pub harmonics: f32,
    pub click: f32,
    /// Centre frequency of the synthesized click (ignored with a click sample).
    pub click_tone: f32,
    pub click_length: f32,
    pub drive: f32,
    pub gain: f32,
    /// Hand-drawn pitch sweep over `pitch_time`, scaled by `pitch_amount`.
    #[serde(default = "Shape::falling")]
    pub pitch_shape: Shape,
    /// Hand-drawn level over the decay, after attack and hold.
    #[serde(default = "Shape::falling")]
    pub amp_shape: Shape,
    /// Hand-drawn drive amount over the whole hit (1 = the Drive knob).
    #[serde(default = "Shape::flat")]
    pub drive_shape: Shape,
    #[serde(default)]
    pub wave: KickWave,
    /// Start phase of the body in degrees (90 starts at the peak: a harder attack).
    #[serde(default)]
    pub phase: f32,
    /// Clean sine layer under the body, untouched by distortion.
    #[serde(default)]
    pub sub_level: f32,
    /// Sub pitch relative to the body, in semitones. The sub follows the body's
    /// sweep in lockstep, so the two never cancel each other.
    #[serde(default)]
    pub sub_tune: f32,
    /// Index into [`KICK_CLICKS`] (used when there is no click sample).
    #[serde(default)]
    pub click_type: u8,
    /// Transposes the click, synthesized or sampled.
    #[serde(default)]
    pub click_pitch: f32,
    #[serde(default)]
    pub eq_low: f32,
    #[serde(default)]
    pub eq_mid: f32,
    #[serde(default = "default_eq_mid_freq")]
    pub eq_mid_freq: f32,
    #[serde(default)]
    pub eq_high: f32,
    /// Compression for more punch, 0 (off) .. 1.
    #[serde(default)]
    pub punch: f32,
    /// Keeps the output under -0.3 dBFS.
    #[serde(default)]
    pub limiter: bool,
    /// How much velocity changes the level, 0..1.
    #[serde(default = "default_velocity")]
    pub velocity: f32,
    /// Locks the hit length to this many beats (0: attack + hold + decay).
    #[serde(default)]
    pub sync_beats: f32,
}

fn default_eq_mid_freq() -> f32 {
    800.0
}

fn default_velocity() -> f32 {
    1.0
}

pub const SHAPE_POINTS: usize = 8;

/// A breakpoint of a [`Shape`]. `bend` curves the segment that ends here:
/// positive changes fast at first, negative slowly at first.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShapePoint {
    pub time: f32,
    pub value: f32,
    pub bend: f32,
}

/// A hand-drawn envelope: up to [`SHAPE_POINTS`] breakpoints over time 0..1,
/// values 0..1. The first point is at time 0 and the last at time 1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub points: [ShapePoint; SHAPE_POINTS],
    pub count: u8,
}

impl Shape {
    /// A straight line from 1 down to 0.
    pub fn falling() -> Self {
        Self::from_points(&[(0.0, 1.0, 0.0), (1.0, 0.0, 0.0)])
    }

    /// Constant at 1.
    pub fn flat() -> Self {
        Self::from_points(&[(0.0, 1.0, 0.0), (1.0, 1.0, 0.0)])
    }

    /// Builds a shape from (time, value, bend) points, sorted and clamped.
    pub fn from_points(points: &[(f32, f32, f32)]) -> Self {
        let mut shape = Shape { points: [ShapePoint { time: 1.0, value: 0.0, bend: 0.0 }; SHAPE_POINTS], count: 0 };
        for &(time, value, bend) in points.iter().take(SHAPE_POINTS) {
            shape.points[shape.count as usize] = ShapePoint { time: time.clamp(0.0, 1.0), value: value.clamp(0.0, 1.0), bend: bend.clamp(-1.0, 1.0) };
            shape.count += 1;
        }
        shape.points[..shape.count as usize].sort_by(|a, b| a.time.total_cmp(&b.time));
        shape.fix_ends();
        shape
    }

    pub fn points(&self) -> &[ShapePoint] {
        &self.points[..(self.count as usize).clamp(2, SHAPE_POINTS)]
    }

    fn fix_ends(&mut self) {
        if self.count < 2 {
            *self = Self::falling();
            return;
        }
        self.points[0].time = 0.0;
        self.points[self.count as usize - 1].time = 1.0;
    }

    /// Eases `u` (0..1) along a segment with `bend`.
    pub fn ease(u: f32, bend: f32) -> f32 {
        let u = u.clamp(0.0, 1.0);
        let power = 1.0 + bend.abs() * 5.0;
        if bend >= 0.0 { 1.0 - (1.0 - u).powf(power) } else { u.powf(power) }
    }

    pub fn value_at(&self, time: f32) -> f32 {
        let points = self.points();
        let time = time.clamp(0.0, 1.0);
        let next = points.iter().position(|p| p.time > time).unwrap_or(points.len() - 1).max(1);
        let (a, b) = (points[next - 1], points[next]);
        let span = b.time - a.time;
        let u = if span > 0.0 { (time - a.time) / span } else { 1.0 };
        a.value + (b.value - a.value) * Self::ease(u, b.bend)
    }

    /// Adds a point; returns its index, or None when the shape is full.
    pub fn insert(&mut self, time: f32, value: f32) -> Option<usize> {
        if self.count as usize >= SHAPE_POINTS || time <= 0.0 || time >= 1.0 {
            return None;
        }
        let count = self.count as usize;
        let index = self.points[..count].iter().position(|p| p.time > time)?;
        self.points.copy_within(index..count, index + 1);
        self.points[index] = ShapePoint { time, value: value.clamp(0.0, 1.0), bend: 0.0 };
        self.count += 1;
        Some(index)
    }

    /// Removes an inner point (the ends stay).
    pub fn remove(&mut self, index: usize) {
        let count = self.count as usize;
        if index == 0 || index + 1 >= count {
            return;
        }
        self.points.copy_within(index + 1..count, index);
        self.count -= 1;
    }

    /// Moves a point, keeping it between its neighbours; the ends only move up and down.
    pub fn move_point(&mut self, index: usize, time: f32, value: f32) {
        let count = self.count as usize;
        if index >= count {
            return;
        }
        self.points[index].value = value.clamp(0.0, 1.0);
        if index > 0 && index + 1 < count {
            let (lo, hi) = (self.points[index - 1].time, self.points[index + 1].time);
            self.points[index].time = time.clamp(lo + 0.001, (hi - 0.001).max(lo + 0.001));
        }
    }
}

impl Default for KickParams {
    fn default() -> Self {
        Self {
            key_follow: false,
            drive_mode: KickDrive::Soft,
            tune: 31.0,
            pitch_amount: 40.0,
            pitch_time: 60.0,
            pitch_curve: 0.6,
            attack: 0.5,
            hold: 60.0,
            decay: 300.0,
            decay_curve: 0.4,
            harmonics: 0.0,
            click: 0.4,
            click_tone: 3000.0,
            click_length: 6.0,
            drive: 0.2,
            gain: 0.8,
            pitch_shape: Shape::falling(),
            amp_shape: Shape::falling(),
            drive_shape: Shape::flat(),
            wave: KickWave::Sine,
            phase: 0.0,
            sub_level: 0.0,
            sub_tune: 0.0,
            click_type: 0,
            click_pitch: 0.0,
            eq_low: 0.0,
            eq_mid: 0.0,
            eq_mid_freq: default_eq_mid_freq(),
            eq_high: 0.0,
            punch: 0.0,
            limiter: false,
            velocity: default_velocity(),
            sync_beats: 0.0,
        }
    }
}

impl KickParams {
    /// Factory presets: (name, settings), grouped by style.
    pub fn presets() -> Vec<(&'static str, KickParams)> {
        let base = KickParams::default();
        let shape = Shape::from_points;
        vec![
            ("Init", base),
            ("House · Punch", KickParams { tune: 33.0, pitch_amount: 36.0, pitch_time: 45.0, hold: 40.0, decay: 220.0, click: 0.5, click_tone: 4000.0, drive: 0.25, phase: 90.0, punch: 0.3, click_type: 2, limiter: true, ..base }),
            ("House · Deep", KickParams { tune: 31.0, pitch_amount: 30.0, pitch_time: 55.0, pitch_curve: 0.5, hold: 60.0, decay: 320.0, decay_curve: 0.3, click: 0.2, click_tone: 1800.0, drive: 0.1, sub_level: 0.3, eq_high: -3.0, ..base }),
            ("Tech House · Tight", KickParams { tune: 34.0, pitch_amount: 40.0, pitch_time: 30.0, pitch_curve: 0.7, hold: 25.0, decay: 160.0, decay_curve: 0.5, click: 0.55, click_tone: 5000.0, click_length: 4.0, drive: 0.35, phase: 90.0, punch: 0.4, click_type: 1, eq_mid: -3.0, eq_mid_freq: 400.0, limiter: true, ..base }),
            ("Techno · Rumble", KickParams { tune: 29.0, pitch_amount: 44.0, pitch_time: 70.0, hold: 90.0, decay: 650.0, decay_curve: 0.6, harmonics: 0.15, drive: 0.55, sub_level: 0.35, eq_low: 3.0, punch: 0.3, gain: 0.6, ..base }),
            (
                "Techno · Industrial",
                KickParams { tune: 30.0, pitch_amount: 50.0, pitch_time: 60.0, hold: 70.0, decay: 420.0, harmonics: 0.25, click: 0.6, click_tone: 7000.0, drive: 0.6, drive_mode: KickDrive::Fold, gain: 0.6, click_type: 5, eq_mid: 3.0, eq_mid_freq: 1500.0, limiter: true, ..base },
            ),
            (
                "Hard · Kick",
                KickParams {
                    tune: 34.0,
                    pitch_amount: 52.0,
                    pitch_time: 90.0,
                    pitch_curve: 0.75,
                    hold: 120.0,
                    decay: 400.0,
                    harmonics: 0.3,
                    click: 0.7,
                    click_tone: 6000.0,
                    drive: 0.8,
                    drive_mode: KickDrive::Hard,
                    gain: 0.6,
                    click_type: 4, punch: 0.4, limiter: true, ..base
                },
            ),
            (
                // A fast "tok" then a pitch plateau that bends down slowly: the long, tonal tail.
                "Hard · Hardstyle Tail",
                KickParams {
                    tune: 36.0,
                    pitch_amount: 48.0,
                    pitch_time: 300.0,
                    pitch_curve: 0.0,
                    pitch_shape: shape(&[(0.0, 1.0, 0.0), (0.06, 0.25, 0.8), (0.7, 0.12, 0.0), (1.0, 0.0, -0.5)]),
                    hold: 150.0,
                    decay: 350.0,
                    decay_curve: 0.2,
                    harmonics: 0.4,
                    click: 0.6,
                    click_tone: 5500.0,
                    drive: 0.75,
                    drive_mode: KickDrive::Fold,
                    gain: 0.55,
                    click_type: 4, sub_level: 0.25, limiter: true, ..base
                },
            ),
            (
                "Hard · Gabber",
                KickParams { tune: 38.0, pitch_amount: 40.0, pitch_time: 25.0, pitch_curve: 0.8, hold: 90.0, decay: 250.0, harmonics: 0.5, click: 0.8, click_tone: 8000.0, drive: 1.0, drive_mode: KickDrive::Hard, gain: 0.5, click_type: 3, eq_mid: 4.0, eq_mid_freq: 900.0, limiter: true, ..base },
            ),
            ("Trance · Thud", KickParams { tune: 32.0, pitch_amount: 48.0, pitch_time: 35.0, pitch_curve: 0.8, hold: 30.0, decay: 180.0, decay_curve: 0.6, click: 0.6, click_tone: 2500.0, drive: 0.35, click_type: 2, phase: 90.0, punch: 0.35, limiter: true, ..base }),
            ("Psytrance · Click", KickParams { tune: 34.0, pitch_amount: 56.0, pitch_time: 20.0, pitch_curve: 0.85, hold: 15.0, decay: 140.0, decay_curve: 0.7, click: 0.8, click_tone: 6500.0, click_length: 3.0, drive: 0.3, gain: 0.6, click_type: 1, punch: 0.3, eq_low: -2.0, limiter: true, ..base }),
            ("Drum & Bass · Punch", KickParams { tune: 38.0, pitch_amount: 36.0, pitch_time: 25.0, pitch_curve: 0.7, hold: 20.0, decay: 180.0, decay_curve: 0.5, harmonics: 0.1, click: 0.7, click_tone: 4500.0, drive: 0.45, gain: 0.7, click_type: 8, phase: 90.0, punch: 0.5, limiter: true, ..base }),
            ("808 · Long", KickParams { tune: 28.0, pitch_amount: 24.0, pitch_time: 40.0, hold: 200.0, decay: 1600.0, decay_curve: 0.3, click: 0.15, drive: 0.1, key_follow: true, sub_level: 0.4, ..base }),
            (
                // After the hit the pitch keeps gliding down an octave and a half.
                "808 · Glide",
                KickParams {
                    tune: 28.0,
                    pitch_amount: 30.0,
                    pitch_time: 700.0,
                    pitch_curve: 0.0,
                    pitch_shape: shape(&[(0.0, 1.0, 0.0), (0.04, 0.6, 0.8), (1.0, 0.0, -0.3)]),
                    hold: 300.0,
                    decay: 1200.0,
                    decay_curve: 0.3,
                    click: 0.1,
                    drive: 0.25,
                    key_follow: true,
                    sub_level: 0.3, drive_mode: KickDrive::Tube, ..base
                },
            ),
            ("Trap · Boom", KickParams { tune: 26.0, pitch_amount: 28.0, pitch_time: 50.0, hold: 250.0, decay: 1100.0, decay_curve: 0.2, harmonics: 0.1, click: 0.3, click_tone: 2000.0, drive: 0.5, sub_level: 0.5, drive_mode: KickDrive::Tube, limiter: true, ..base }),
            ("Lo-Fi · Thump", KickParams { tune: 30.0, pitch_amount: 26.0, pitch_time: 50.0, pitch_curve: 0.4, hold: 40.0, decay: 260.0, harmonics: 0.35, click: 0.35, click_tone: 900.0, click_length: 12.0, drive: 0.4, click_type: 11, drive_mode: KickDrive::Crush, eq_high: -6.0, ..base }),
            (
                // The level dips and comes back: two punches from one note.
                "FX · Double Punch",
                KickParams {
                    tune: 33.0,
                    pitch_amount: 40.0,
                    pitch_time: 50.0,
                    amp_shape: shape(&[(0.0, 1.0, 0.0), (0.18, 0.05, 0.6), (0.24, 0.9, -0.2), (1.0, 0.0, 0.5)]),
                    hold: 20.0,
                    decay: 380.0,
                    decay_curve: 0.0,
                    click: 0.5,
                    drive: 0.3,
                    ..base
                },
            ),
            ("FX · Zap", KickParams { tune: 33.0, pitch_amount: 72.0, pitch_time: 120.0, pitch_curve: 0.9, hold: 30.0, decay: 200.0, decay_curve: 0.5, click: 0.2, click_tone: 9000.0, drive: 0.2, click_type: 10, ..base }),
        ]
    }
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
    Degrees,
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
const KICK_PARAMS: [ParamInfo; 24] = [
    ParamInfo::linear("Tune", 16.0, 52.0, 31.0, Note),
    ParamInfo::linear("Pitch", 0.0, 72.0, 40.0, Semitones),
    ParamInfo::log("P. Time", 5.0, 500.0, 60.0, Milliseconds),
    ParamInfo::linear("P. Curve", 0.0, 1.0, 0.6, Percent),
    ParamInfo::log("Attack", 0.1, 20.0, 0.5, Milliseconds),
    ParamInfo::linear("Hold", 0.0, 500.0, 60.0, Milliseconds),
    ParamInfo::log("Decay", 10.0, 2000.0, 300.0, Milliseconds),
    ParamInfo::linear("D. Curve", 0.0, 1.0, 0.4, Percent),
    ParamInfo::linear("Harmonics", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Click", 0.0, 1.0, 0.4, Percent),
    ParamInfo::log("Click Tone", 500.0, 12000.0, 3000.0, Hertz),
    ParamInfo::log("Click Len", 1.0, 50.0, 6.0, Milliseconds),
    ParamInfo::linear("Drive", 0.0, 1.0, 0.2, Percent),
    ParamInfo::linear("Volume", 0.0, 1.0, 0.8, Percent),
    ParamInfo::linear("Phase", 0.0, 360.0, 0.0, Degrees),
    ParamInfo::linear("Sub", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Sub Tune", -12.0, 12.0, 0.0, Semitones),
    ParamInfo::linear("Click Pitch", -24.0, 24.0, 0.0, Semitones),
    ParamInfo::linear("Low", -12.0, 12.0, 0.0, Decibels),
    ParamInfo::linear("Mid", -12.0, 12.0, 0.0, Decibels),
    ParamInfo::log("Mid Freq", 100.0, 5000.0, 800.0, Hertz),
    ParamInfo::linear("High", -12.0, 12.0, 0.0, Decibels),
    ParamInfo::linear("Punch", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Velocity", 0.0, 1.0, 1.0, Percent),
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
    pub fn catalog() -> [InstrumentKind; 9] {
        [
            InstrumentKind::Synth(SynthParams::default()),
            InstrumentKind::SuperSaw(SuperSawParams::default()),
            InstrumentKind::Acid(AcidParams::default()),
            InstrumentKind::Fm(FmParams::default()),
            InstrumentKind::Wavetable(WaveParams::default()),
            InstrumentKind::Sampler(SamplerParams::default()),
            InstrumentKind::Kick(KickParams::default()),
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
            InstrumentKind::Kick(_) => "Kick Designer",
            InstrumentKind::Wavetable(_) => "Wavetable Synth",
        }
    }

    pub fn is_drums(&self) -> bool {
        matches!(self, InstrumentKind::Drums | InstrumentKind::DrumMachine(_) | InstrumentKind::DrumRack(_))
    }

    /// True when switching from `self` to `other` needs a newly built instrument
    /// (another kind, or a different set of effects in the wavetable synth's rack).
    pub fn needs_rebuild(&self, other: &InstrumentKind) -> bool {
        if self.label() != other.label() {
            return true;
        }
        match (self, other) {
            (InstrumentKind::Wavetable(a), InstrumentKind::Wavetable(b)) => {
                let layout = |p: &WaveParams| p.fx.map(|d| d.map(|d| std::mem::discriminant(&d.effect)));
                layout(a) != layout(b)
            }
            _ => false,
        }
    }

    /// True for instruments that play the track's samples.
    pub fn uses_samples(&self) -> bool {
        matches!(self, InstrumentKind::Sampler(_) | InstrumentKind::DrumRack(_) | InstrumentKind::Kick(_) | InstrumentKind::Wavetable(_))
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
            InstrumentKind::Kick(_) => &KICK_PARAMS,
            InstrumentKind::Wavetable(_) => &WAVE_PARAMS,
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
            InstrumentKind::Wavetable(p) => return p.param_mut(index),
            InstrumentKind::Kick(p) => match index {
                0 => &mut p.tune,
                1 => &mut p.pitch_amount,
                2 => &mut p.pitch_time,
                3 => &mut p.pitch_curve,
                4 => &mut p.attack,
                5 => &mut p.hold,
                6 => &mut p.decay,
                7 => &mut p.decay_curve,
                8 => &mut p.harmonics,
                9 => &mut p.click,
                10 => &mut p.click_tone,
                11 => &mut p.click_length,
                12 => &mut p.drive,
                13 => &mut p.gain,
                14 => &mut p.phase,
                15 => &mut p.sub_level,
                16 => &mut p.sub_tune,
                17 => &mut p.click_pitch,
                18 => &mut p.eq_low,
                19 => &mut p.eq_mid,
                20 => &mut p.eq_mid_freq,
                21 => &mut p.eq_high,
                22 => &mut p.punch,
                23 => &mut p.velocity,
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

// ----- Wavetable synth ----------------------------------------------------------------

pub const WAVE_OSCS: usize = 2;
pub const WAVE_ENVS: usize = 3;
pub const WAVE_LFOS: usize = 4;
pub const WAVE_MACROS: usize = 4;
pub const WAVE_SLOTS: usize = 16;
/// `WaveOsc::table` for a table loaded from a file (the track's sample slot of that oscillator).
pub const WAVE_USER_TABLE: u8 = 255;
/// Index of the first matrix slot amount; the 16 amounts follow. Every other
/// parameter can be a modulation destination (see [`wave_is_target`]).
pub const WAVE_MOD_TARGETS: usize = 66;
/// Grain parameters of oscillator A (B's follow), after the slot amounts.
pub const WAVE_GRAIN: usize = WAVE_MOD_TARGETS + WAVE_SLOTS;
pub const WAVE_ARP_GATE: usize = WAVE_GRAIN + 8;
/// Effects in the synth's own rack.
pub const WAVE_FX: usize = 6;

/// True for parameters the matrix can modulate (all but its own amounts).
pub fn wave_is_target(index: usize) -> bool {
    index < WAVE_MOD_TARGETS || (WAVE_GRAIN..WAVE_PARAMS.len()).contains(&index)
}

/// What an oscillator plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OscKind {
    /// A wavetable, scanned by Position.
    #[default]
    Wavetable,
    /// The oscillator's sample, played chromatically from C3 (Position: start).
    Sample,
    /// Grains of the oscillator's sample (Position: where they are taken from).
    Granular,
    /// The sample's spectrum turned into a wavetable (Position: time in the sample).
    Spectral,
}

impl OscKind {
    pub const ALL: [OscKind; 4] = [Self::Wavetable, Self::Sample, Self::Granular, Self::Spectral];

    pub fn label(self) -> &'static str {
        match self {
            Self::Wavetable => "Wavetable",
            Self::Sample => "Sample",
            Self::Granular => "Granular",
            Self::Spectral => "Spectral",
        }
    }

    /// Kinds that play the oscillator's sample file.
    pub fn uses_sample(self) -> bool {
        matches!(self, Self::Sample | Self::Granular | Self::Spectral)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArpMode {
    #[default]
    Up,
    Down,
    UpDown,
    Random,
    /// In the order the keys were pressed.
    Played,
}

impl ArpMode {
    pub const ALL: [ArpMode; 5] = [Self::Up, Self::Down, Self::UpDown, Self::Random, Self::Played];

    pub fn label(self) -> &'static str {
        match self {
            Self::Up => "Up",
            Self::Down => "Down",
            Self::UpDown => "Up/Down",
            Self::Random => "Random",
            Self::Played => "Played",
        }
    }
}

/// Arpeggiator: plays the held notes one at a time, in time with the tempo.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArpParams {
    pub on: bool,
    pub mode: ArpMode,
    /// Step length in beats (0.25: sixteenths).
    pub rate_beats: f32,
    /// Octaves the pattern spans, 1..4.
    pub octaves: u8,
}

impl Default for ArpParams {
    fn default() -> Self {
        Self { on: false, mode: ArpMode::Up, rate_beats: 0.25, octaves: 1 }
    }
}

fn yes() -> bool {
    true
}
/// `ModSlot::dest` of an unused slot.
pub const WAVE_NO_TARGET: u8 = 255;

/// How an oscillator bends its reading of the wavetable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OscWarp {
    #[default]
    Off,
    /// Hard sync: the cycle restarts faster than it plays.
    Sync,
    /// Squeezes the cycle towards its start.
    BendPlus,
    /// Squeezes the cycle towards its end.
    BendMinus,
    /// Moves the middle of the cycle, like pulse width.
    Pwm,
    /// Plays the first half forwards and then backwards.
    Mirror,
    /// Reduces the output to fewer levels.
    Quantize,
    /// Frequency modulation by the other oscillator.
    Fm,
}

impl OscWarp {
    pub const ALL: [OscWarp; 8] = [Self::Off, Self::Sync, Self::BendPlus, Self::BendMinus, Self::Pwm, Self::Mirror, Self::Quantize, Self::Fm];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Sync => "Sync",
            Self::BendPlus => "Bend +",
            Self::BendMinus => "Bend -",
            Self::Pwm => "PWM",
            Self::Mirror => "Mirror",
            Self::Quantize => "Quantize",
            Self::Fm => "FM",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaveFilter {
    #[default]
    Lp12,
    Lp24,
    Hp12,
    Hp24,
    Bp12,
    Notch,
    /// Four-pole ladder low-pass with saturating feedback.
    Ladder,
    /// Short feedback delay tuned by the cutoff.
    Comb,
    /// Vowel formants; the cutoff moves between them.
    Formant,
}

impl WaveFilter {
    pub const ALL: [WaveFilter; 9] = [Self::Lp12, Self::Lp24, Self::Hp12, Self::Hp24, Self::Bp12, Self::Notch, Self::Ladder, Self::Comb, Self::Formant];

    pub fn label(self) -> &'static str {
        match self {
            Self::Lp12 => "Low 12",
            Self::Lp24 => "Low 24",
            Self::Hp12 => "High 12",
            Self::Hp24 => "High 24",
            Self::Bp12 => "Band 12",
            Self::Notch => "Notch",
            Self::Ladder => "Ladder",
            Self::Comb => "Comb",
            Self::Formant => "Formant",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LfoMode {
    /// Runs continuously; every voice sees the same phase.
    #[default]
    Free,
    /// Restarts with each note.
    Retrig,
    /// Plays its shape once per note, like an envelope.
    Envelope,
}

impl LfoMode {
    pub const ALL: [LfoMode; 3] = [Self::Free, Self::Retrig, Self::Envelope]; 

    pub fn label(self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Retrig => "Retrig",
            Self::Envelope => "Env",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModSource {
    #[default]
    None,
    Env1,
    Env2,
    Env3,
    Lfo1,
    Lfo2,
    Lfo3,
    Lfo4,
    Velocity,
    /// The played note, 0 at C-2 up to 1 at G8.
    Note,
    /// A random value per note.
    Random,
    Macro1,
    Macro2,
    Macro3,
    Macro4,
}

impl ModSource {
    pub const ALL: [ModSource; 15] = [
        Self::None,
        Self::Env1,
        Self::Env2,
        Self::Env3,
        Self::Lfo1,
        Self::Lfo2,
        Self::Lfo3,
        Self::Lfo4,
        Self::Velocity,
        Self::Note,
        Self::Random,
        Self::Macro1,
        Self::Macro2,
        Self::Macro3,
        Self::Macro4,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "—",
            Self::Env1 => "Env 1",
            Self::Env2 => "Env 2",
            Self::Env3 => "Env 3",
            Self::Lfo1 => "LFO 1",
            Self::Lfo2 => "LFO 2",
            Self::Lfo3 => "LFO 3",
            Self::Lfo4 => "LFO 4",
            Self::Velocity => "Velocity",
            Self::Note => "Note",
            Self::Random => "Random",
            Self::Macro1 => "Macro 1",
            Self::Macro2 => "Macro 2",
            Self::Macro3 => "Macro 3",
            Self::Macro4 => "Macro 4",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaveOsc {
    pub on: bool,
    #[serde(default)]
    pub kind: OscKind,
    /// Sample mode: loop the sample instead of playing it once.
    #[serde(default = "yes")]
    pub sample_loop: bool,
    /// Granular: grain length in milliseconds.
    #[serde(default = "default_grain_size")]
    pub grain_size: f32,
    /// Granular: grains per second.
    #[serde(default = "default_density")]
    pub density: f32,
    /// Granular: random spread of where grains start, 0..1 of the sample.
    #[serde(default)]
    pub spray: f32,
    /// Granular: random pitch per grain, in semitones.
    #[serde(default)]
    pub grain_pitch: f32,
    /// Index into the built-in tables, or [`WAVE_USER_TABLE`].
    pub table: u8,
    pub warp: OscWarp,
    pub position: f32,
    pub octave: f32,
    pub semi: f32,
    /// Cents.
    pub fine: f32,
    pub level: f32,
    pub pan: f32,
    /// Unison voices, 1..16.
    pub unison: f32,
    pub detune: f32,
    /// Level of the outer unison voices against the centre ones.
    pub blend: f32,
    /// Stereo spread of the unison voices.
    pub width: f32,
    /// Start phase, 0..1 of a cycle.
    pub phase: f32,
    /// How much the start phase is randomised per voice.
    pub random_phase: f32,
    pub warp_amount: f32,
}

fn default_grain_size() -> f32 {
    80.0
}

fn default_density() -> f32 {
    30.0
}

impl Default for WaveOsc {
    fn default() -> Self {
        Self {
            on: true,
            kind: OscKind::Wavetable,
            sample_loop: true,
            grain_size: default_grain_size(),
            density: default_density(),
            spray: 0.0,
            grain_pitch: 0.0,
            table: 0,
            warp: OscWarp::Off,
            position: 0.0,
            octave: 0.0,
            semi: 0.0,
            fine: 0.0,
            level: 0.75,
            pan: 0.0,
            unison: 1.0,
            detune: 0.25,
            blend: 0.75,
            width: 1.0,
            phase: 0.0,
            random_phase: 1.0,
            warp_amount: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaveSub {
    pub on: bool,
    pub wave: KickWave,
    pub octave: f32,
    pub level: f32,
    pub pan: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaveNoise {
    pub on: bool,
    pub level: f32,
    /// Low-pass on the noise, in Hz.
    pub color: f32,
    pub pan: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaveFilterParams {
    pub on: bool,
    pub mode: WaveFilter,
    /// Which sources go through the filter: oscillator A, B, sub, noise.
    pub routes: [bool; 4],
    pub cutoff: f32,
    pub resonance: f32,
    pub drive: f32,
    pub mix: f32,
    /// How much the cutoff follows the played note (1: one octave per octave).
    pub key_track: f32,
}

/// Attack-hold-decay-sustain-release envelope; times in seconds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaveEnv {
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaveLfo {
    /// One cycle, drawn by hand (values 0..1).
    pub shape: Shape,
    pub mode: LfoMode,
    /// Period in beats when synced to the tempo; 0 uses `rate`.
    pub sync_beats: f32,
    pub rate: f32,
    /// Seconds to fade in after a note starts.
    pub rise: f32,
}

/// A modulation matrix row: `source` moves parameter `dest` by `amount` of its range.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModSlot {
    pub source: ModSource,
    pub dest: u8,
    pub amount: f32,
    /// Centres the source on zero, so it moves the parameter both ways.
    pub bipolar: bool,
}

impl ModSlot {
    pub const EMPTY: ModSlot = ModSlot { source: ModSource::None, dest: WAVE_NO_TARGET, amount: 0.0, bipolar: false };
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaveParams {
    pub osc: [WaveOsc; WAVE_OSCS],
    pub sub: WaveSub,
    pub noise: WaveNoise,
    pub filter: WaveFilterParams,
    /// Env 1 shapes the volume; 2 and 3 are free for modulation.
    pub env: [WaveEnv; WAVE_ENVS],
    pub lfo: [WaveLfo; WAVE_LFOS],
    pub macros: [f32; WAVE_MACROS],
    pub matrix: [ModSlot; WAVE_SLOTS],
    pub mono: bool,
    /// In mono, overlapping notes glide without restarting the envelopes.
    pub legato: bool,
    /// Glide time in milliseconds (0: off).
    pub glide: f32,
    pub gain: f32,
    /// The synth's own effects, applied after the voices are mixed.
    #[serde(default)]
    pub fx: [Option<Device>; WAVE_FX],
    #[serde(default)]
    pub arp: ArpParams,
    /// Length of each arpeggiated note, as a fraction of a step.
    #[serde(default = "default_arp_gate")]
    pub arp_gate: f32,
}

fn default_arp_gate() -> f32 {
    0.5
}

impl Default for WaveParams {
    fn default() -> Self {
        let env = WaveEnv { attack: 0.001, hold: 0.0, decay: 1.0, sustain: 1.0, release: 0.1 };
        let mod_env = WaveEnv { attack: 0.001, hold: 0.0, decay: 0.5, sustain: 0.0, release: 0.1 };
        let triangle = Shape::from_points(&[(0.0, 0.5, 0.0), (0.25, 1.0, 0.0), (0.75, 0.0, 0.0), (1.0, 0.5, 0.0)]);
        let lfo = WaveLfo { shape: triangle, mode: LfoMode::Free, sync_beats: 0.0, rate: 2.0, rise: 0.0 };
        Self {
            osc: [WaveOsc::default(), WaveOsc { on: false, ..WaveOsc::default() }],
            sub: WaveSub { on: false, wave: KickWave::Sine, octave: -1.0, level: 0.75, pan: 0.0 },
            noise: WaveNoise { on: false, level: 0.5, color: 20000.0, pan: 0.0 },
            filter: WaveFilterParams { on: false, mode: WaveFilter::Lp12, routes: [true; 4], cutoff: 18000.0, resonance: 0.1, drive: 0.0, mix: 1.0, key_track: 0.0 },
            env: [env, mod_env, mod_env],
            lfo: [lfo; WAVE_LFOS],
            macros: [0.0; WAVE_MACROS],
            matrix: [ModSlot::EMPTY; WAVE_SLOTS],
            mono: false,
            legato: false,
            glide: 0.0,
            gain: 0.5,
            fx: [None; WAVE_FX],
            arp: ArpParams::default(),
            arp_gate: default_arp_gate(),
        }
    }
}

impl WaveParams {
    /// Factory presets: (name, settings), grouped by use.
    pub fn presets() -> Vec<(&'static str, WaveParams)> {
        let init = WaveParams::default();
        let saw = 2.0 / 3.0;
        let slot = |source, dest: usize, amount| ModSlot { source, dest: dest as u8, amount, bipolar: false };
        let env = |attack, decay, sustain, release| WaveEnv { attack, hold: 0.0, decay, sustain, release };
        let ramp = Shape::from_points(&[(0.0, 0.0, 0.0), (1.0, 1.0, 0.0)]);
        let triangle = Shape::from_points(&[(0.0, 0.0, 0.0), (0.5, 1.0, 0.0), (1.0, 0.0, 0.0)]);
        let mut list = vec![("Init", init)];
        let fx = |effect: Effect| Some(Device::new(effect));
        let delay = |time_beats: f64, mix: f32| fx(Effect::Delay(DelayParams { time_beats, feedback: 0.35, mix, ping_pong: true, ..DelayParams::default() }));
        let reverb = |size: f32, mix: f32| fx(Effect::Reverb(ReverbParams { size, mix, ..ReverbParams::default() }));
        let chorus = fx(Effect::Chorus(ChorusParams::default()));
        let drive = |drive_db: f32| fx(Effect::Saturator(SaturatorParams { drive_db, output_db: -drive_db * 0.5, ..SaturatorParams::default() }));

        let mut p = init;
        p.osc[0] = WaveOsc { position: saw, unison: 7.0, detune: 0.35, blend: 0.7, ..WaveOsc::default() };
        p.osc[1] = WaveOsc { position: saw, octave: 1.0, unison: 5.0, detune: 0.3, level: 0.35, ..WaveOsc::default() };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Lp24, cutoff: 9000.0, resonance: 0.15, ..init.filter };
        p.env[0] = env(0.005, 0.3, 0.8, 0.3);
        p.gain = 0.5;
                p.fx[0] = delay(0.75, 0.2);
        p.fx[1] = reverb(0.6, 0.2);
        list.push(("Lead · Supersaw", p));

        let mut p = init;
        p.osc[0] = WaveOsc { table: 4, position: 0.3, unison: 3.0, detune: 0.15, ..WaveOsc::default() };
        p.lfo[0] = WaveLfo { shape: triangle, mode: LfoMode::Retrig, sync_beats: 2.0, ..init.lfo[0] };
        p.matrix[0] = slot(ModSource::Lfo1, 0, 0.6);
        p.mono = true;
        p.legato = true;
        p.glide = 60.0;
        p.env[0] = env(0.005, 0.5, 0.9, 0.2);
        p.gain = 0.9;
                p.fx[0] = drive(9.0);
        p.fx[1] = delay(0.5, 0.15);
        list.push(("Lead · Sync Scream", p));

        let mut p = init;
        p.osc[0] = WaveOsc { position: saw, unison: 2.0, detune: 0.18, width: 0.3, ..WaveOsc::default() };
        p.osc[1] = WaveOsc { position: saw, octave: -1.0, unison: 2.0, detune: 0.12, width: 0.2, level: 0.5, ..WaveOsc::default() };
        p.sub = WaveSub { on: true, level: 0.6, ..init.sub };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Ladder, cutoff: 900.0, resonance: 0.25, drive: 0.3, routes: [true, true, false, true], ..init.filter };
        p.lfo[0] = WaveLfo { rate: 0.3, ..init.lfo[0] };
        p.matrix[0] = slot(ModSource::Lfo1, 7, 0.15);
        p.mono = true;
        p.legato = true;
        p.glide = 40.0;
        p.gain = 0.55;
                p.fx[0] = chorus;
        list.push(("Bass · Reese", p));

        let mut p = init;
        p.osc[0] = WaveOsc { table: 9, position: 0.2, unison: 2.0, detune: 0.1, width: 0.4, ..WaveOsc::default() };
        p.sub = WaveSub { on: true, level: 0.55, ..init.sub };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Formant, cutoff: 400.0, resonance: 0.5, drive: 0.4, routes: [true, true, false, true], ..init.filter };
        p.lfo[0] = WaveLfo { shape: triangle, mode: LfoMode::Retrig, sync_beats: 0.5, ..init.lfo[0] };
        p.matrix[0] = slot(ModSource::Lfo1, 0, 0.7);
        p.matrix[1] = slot(ModSource::Lfo1, 32, 0.45);
        p.mono = true;
        p.gain = 0.45;
                p.fx[0] = drive(6.0);
        list.push(("Bass · Growl", p));

        let mut p = init;
        p.osc[0] = WaveOsc { position: saw, unison: 3.0, detune: 0.12, width: 0.5, ..WaveOsc::default() };
        p.sub = WaveSub { on: true, level: 0.7, ..init.sub };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Lp24, cutoff: 150.0, resonance: 0.55, drive: 0.5, routes: [true, true, false, true], ..init.filter };
        p.lfo[0] = WaveLfo { shape: triangle, mode: LfoMode::Retrig, sync_beats: 0.5, ..init.lfo[0] };
        p.matrix[0] = slot(ModSource::Lfo1, 32, 0.75);
        p.mono = true;
        p.gain = 0.6;
        list.push(("Bass · Wobble 1/8", p));

        let mut p = init;
        p.osc[0] = WaveOsc { position: 0.0, level: 0.3, ..WaveOsc::default() };
        p.sub = WaveSub { on: true, level: 0.9, ..init.sub };
        p.mono = true;
        p.glide = 30.0;
        p.env[0] = env(0.003, 1.0, 1.0, 0.08);
        p.gain = 0.7;
        list.push(("Bass · Sub", p));

        let mut p = init;
        p.osc[0] = WaveOsc { table: 5, position: 0.55, ..WaveOsc::default() };
        p.osc[1] = WaveOsc { on: true, position: 0.0, octave: 1.0, level: 0.0, ..WaveOsc::default() };
        p.osc[0].warp = OscWarp::Fm;
        p.osc[0].warp_amount = 0.4;
        p.env[1] = env(0.001, 0.25, 0.1, 0.1);
        p.matrix[0] = slot(ModSource::Env2, 12, 0.5);
        p.sub = WaveSub { on: true, level: 0.5, ..init.sub };
        p.mono = true;
        p.gain = 0.75;
        list.push(("Bass · FM Metal", p));

        let mut p = init;
        p.osc[0] = WaveOsc { position: saw, unison: 3.0, detune: 0.2, ..WaveOsc::default() };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Lp24, cutoff: 300.0, resonance: 0.2, ..init.filter };
        p.env[0] = env(0.002, 0.45, 0.0, 0.3);
        p.env[1] = env(0.001, 0.25, 0.0, 0.2);
        p.matrix[0] = slot(ModSource::Env2, 32, 0.55);
        p.gain = 0.85;
                p.fx[0] = delay(0.75, 0.25);
        p.fx[1] = reverb(0.5, 0.18);
        list.push(("Pluck · Bright", p));

        let mut p = init;
        p.osc[0] = WaveOsc { table: 7, position: 0.2, unison: 2.0, detune: 0.1, ..WaveOsc::default() };
        p.env[0] = env(0.001, 0.35, 0.0, 0.25);
        p.env[1] = env(0.001, 0.3, 0.0, 0.2);
        p.matrix[0] = slot(ModSource::Env2, 0, 0.6);
        p.gain = 0.6;
        list.push(("Pluck · Digital", p));

        let mut p = init;
        p.osc[0] = WaveOsc { position: saw, unison: 5.0, detune: 0.3, ..WaveOsc::default() };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Lp24, cutoff: 1200.0, resonance: 0.3, ..init.filter };
        p.env[0] = env(0.002, 0.2, 0.0, 0.15);
        p.env[1] = env(0.001, 0.15, 0.0, 0.1);
        p.matrix[0] = slot(ModSource::Env2, 32, 0.4);
        p.arp = ArpParams { on: true, mode: ArpMode::UpDown, rate_beats: 0.25, octaves: 2 };
        p.arp_gate = 0.6;
        p.fx[0] = delay(0.75, 0.3);
        p.fx[1] = reverb(0.6, 0.2);
        p.gain = 0.95;
        list.push(("Arp · Trance Pluck", p));

        let mut p = init;
        p.osc[0] = WaveOsc { table: 3, position: 0.0, unison: 4.0, detune: 0.2, ..WaveOsc::default() };
        p.osc[1] = WaveOsc { table: 3, position: 0.5, octave: -1.0, unison: 3.0, detune: 0.25, level: 0.4, ..WaveOsc::default() };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Lp12, cutoff: 4000.0, ..init.filter };
        p.lfo[0] = WaveLfo { rate: 0.12, shape: triangle, ..init.lfo[0] };
        p.matrix[0] = slot(ModSource::Lfo1, 0, 0.8);
        p.matrix[1] = slot(ModSource::Lfo1, 13, 0.5);
        p.env[0] = env(0.8, 1.0, 1.0, 1.5);
        p.gain = 0.5;
                p.fx[0] = chorus;
        p.fx[1] = reverb(0.85, 0.35);
        list.push(("Pad · Vowel", p));

        let mut p = init;
        p.osc[0] = WaveOsc { table: 10, position: 0.1, unison: 6.0, detune: 0.25, ..WaveOsc::default() };
        p.lfo[0] = WaveLfo { rate: 0.08, shape: triangle, ..init.lfo[0] };
        p.matrix[0] = slot(ModSource::Lfo1, 0, 0.7);
        p.env[0] = env(1.2, 1.0, 1.0, 2.0);
        p.gain = 0.5;
                p.fx[0] = reverb(0.9, 0.4);
        list.push(("Pad · Spectral", p));

        let mut p = init;
        p.osc[0] = WaveOsc { table: 8, position: 0.7, random_phase: 0.0, ..WaveOsc::default() };
        p.env[0] = env(0.003, 0.2, 1.0, 0.06);
        p.gain = 0.55;
        list.push(("Keys · Organ", p));

        let mut p = init;
        p.osc[0] = WaveOsc { table: 5, position: 0.35, random_phase: 0.0, ..WaveOsc::default() };
        p.env[0] = env(0.001, 2.0, 0.0, 1.5);
        p.env[1] = env(0.001, 0.8, 0.0, 0.5);
        p.matrix[0] = slot(ModSource::Env2, 0, 0.35);
        p.gain = 0.55;
                p.fx[0] = reverb(0.7, 0.25);
        list.push(("Keys · Bell", p));

        let mut p = init;
        p.osc[0] = WaveOsc { table: 2, position: 0.0, unison: 5.0, detune: 0.3, ..WaveOsc::default() };
        p.noise = WaveNoise { on: true, level: 0.35, ..init.noise };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Lp24, cutoff: 200.0, resonance: 0.3, ..init.filter };
        p.lfo[0] = WaveLfo { shape: ramp, mode: LfoMode::Envelope, sync_beats: 16.0, ..init.lfo[0] };
        p.matrix[0] = slot(ModSource::Lfo1, 0, 1.0);
        p.matrix[1] = slot(ModSource::Lfo1, 32, 0.8);
        p.env[0] = env(0.5, 1.0, 1.0, 1.0);
        p.gain = 0.5;
                p.fx[0] = reverb(0.9, 0.3);
        list.push(("FX · Riser 4 bars", p));
        list
    }

    pub fn param_mut(&mut self, index: usize) -> Option<&mut f32> {
        let p = self;
        Some(match index {
                0 => &mut p.osc[0].position,
                1 => &mut p.osc[0].octave,
                2 => &mut p.osc[0].semi,
                3 => &mut p.osc[0].fine,
                4 => &mut p.osc[0].level,
                5 => &mut p.osc[0].pan,
                6 => &mut p.osc[0].unison,
                7 => &mut p.osc[0].detune,
                8 => &mut p.osc[0].blend,
                9 => &mut p.osc[0].width,
                10 => &mut p.osc[0].phase,
                11 => &mut p.osc[0].random_phase,
                12 => &mut p.osc[0].warp_amount,
                13 => &mut p.osc[1].position,
                14 => &mut p.osc[1].octave,
                15 => &mut p.osc[1].semi,
                16 => &mut p.osc[1].fine,
                17 => &mut p.osc[1].level,
                18 => &mut p.osc[1].pan,
                19 => &mut p.osc[1].unison,
                20 => &mut p.osc[1].detune,
                21 => &mut p.osc[1].blend,
                22 => &mut p.osc[1].width,
                23 => &mut p.osc[1].phase,
                24 => &mut p.osc[1].random_phase,
                25 => &mut p.osc[1].warp_amount,
                26 => &mut p.sub.octave,
                27 => &mut p.sub.level,
                28 => &mut p.sub.pan,
                29 => &mut p.noise.level,
                30 => &mut p.noise.color,
                31 => &mut p.noise.pan,
                32 => &mut p.filter.cutoff,
                33 => &mut p.filter.resonance,
                34 => &mut p.filter.drive,
                35 => &mut p.filter.mix,
                36 => &mut p.filter.key_track,
                37 => &mut p.env[0].attack,
                38 => &mut p.env[0].hold,
                39 => &mut p.env[0].decay,
                40 => &mut p.env[0].sustain,
                41 => &mut p.env[0].release,
                42 => &mut p.env[1].attack,
                43 => &mut p.env[1].hold,
                44 => &mut p.env[1].decay,
                45 => &mut p.env[1].sustain,
                46 => &mut p.env[1].release,
                47 => &mut p.env[2].attack,
                48 => &mut p.env[2].hold,
                49 => &mut p.env[2].decay,
                50 => &mut p.env[2].sustain,
                51 => &mut p.env[2].release,
                52 => &mut p.lfo[0].rate,
                53 => &mut p.lfo[0].rise,
                54 => &mut p.lfo[1].rate,
                55 => &mut p.lfo[1].rise,
                56 => &mut p.lfo[2].rate,
                57 => &mut p.lfo[2].rise,
                58 => &mut p.lfo[3].rate,
                59 => &mut p.lfo[3].rise,
                60 => &mut p.macros[0],
                61 => &mut p.macros[1],
                62 => &mut p.macros[2],
                63 => &mut p.macros[3],
                64 => &mut p.glide,
                65 => &mut p.gain,
                66 => &mut p.matrix[0].amount,
                67 => &mut p.matrix[1].amount,
                68 => &mut p.matrix[2].amount,
                69 => &mut p.matrix[3].amount,
                70 => &mut p.matrix[4].amount,
                71 => &mut p.matrix[5].amount,
                72 => &mut p.matrix[6].amount,
                73 => &mut p.matrix[7].amount,
                74 => &mut p.matrix[8].amount,
                75 => &mut p.matrix[9].amount,
                76 => &mut p.matrix[10].amount,
                77 => &mut p.matrix[11].amount,
                78 => &mut p.matrix[12].amount,
                79 => &mut p.matrix[13].amount,
                80 => &mut p.matrix[14].amount,
                81 => &mut p.matrix[15].amount,
                82 => &mut p.osc[0].grain_size,
                83 => &mut p.osc[0].density,
                84 => &mut p.osc[0].spray,
                85 => &mut p.osc[0].grain_pitch,
                86 => &mut p.osc[1].grain_size,
                87 => &mut p.osc[1].density,
                88 => &mut p.osc[1].spray,
                89 => &mut p.osc[1].grain_pitch,
                90 => &mut p.arp_gate,
            _ => return None,
        })
    }
}

pub const WAVE_PARAMS: [ParamInfo; 91] = [
    ParamInfo::linear("A Pos", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("A Oct", -4.0, 4.0, 0.0, Count),
    ParamInfo::linear("A Semi", -12.0, 12.0, 0.0, Semitones),
    ParamInfo::linear("A Fine", -100.0, 100.0, 0.0, Plain),
    ParamInfo::linear("A Level", 0.0, 1.0, 0.75, Percent),
    ParamInfo::linear("A Pan", -1.0, 1.0, 0.0, Pan),
    ParamInfo::linear("A Unison", 1.0, 16.0, 1.0, Count),
    ParamInfo::linear("A Detune", 0.0, 1.0, 0.25, Percent),
    ParamInfo::linear("A Blend", 0.0, 1.0, 0.75, Percent),
    ParamInfo::linear("A Width", 0.0, 1.0, 1.0, Percent),
    ParamInfo::linear("A Phase", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("A Rand", 0.0, 1.0, 1.0, Percent),
    ParamInfo::linear("A Warp", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("B Pos", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("B Oct", -4.0, 4.0, 0.0, Count),
    ParamInfo::linear("B Semi", -12.0, 12.0, 0.0, Semitones),
    ParamInfo::linear("B Fine", -100.0, 100.0, 0.0, Plain),
    ParamInfo::linear("B Level", 0.0, 1.0, 0.75, Percent),
    ParamInfo::linear("B Pan", -1.0, 1.0, 0.0, Pan),
    ParamInfo::linear("B Unison", 1.0, 16.0, 1.0, Count),
    ParamInfo::linear("B Detune", 0.0, 1.0, 0.25, Percent),
    ParamInfo::linear("B Blend", 0.0, 1.0, 0.75, Percent),
    ParamInfo::linear("B Width", 0.0, 1.0, 1.0, Percent),
    ParamInfo::linear("B Phase", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("B Rand", 0.0, 1.0, 1.0, Percent),
    ParamInfo::linear("B Warp", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Sub Oct", -2.0, 0.0, -1.0, Count),
    ParamInfo::linear("Sub Level", 0.0, 1.0, 0.75, Percent),
    ParamInfo::linear("Sub Pan", -1.0, 1.0, 0.0, Pan),
    ParamInfo::linear("Noise Level", 0.0, 1.0, 0.5, Percent),
    ParamInfo::log("Noise Color", 200.0, 20000.0, 20000.0, Hertz),
    ParamInfo::linear("Noise Pan", -1.0, 1.0, 0.0, Pan),
    ParamInfo::log("Cutoff", 20.0, 20000.0, 18000.0, Hertz),
    ParamInfo::linear("Res", 0.0, 1.0, 0.1, Percent),
    ParamInfo::linear("Drive", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Fil Mix", 0.0, 1.0, 1.0, Percent),
    ParamInfo::linear("Key Track", 0.0, 1.0, 0.0, Percent),
    ParamInfo::log("Env1 A", 0.0005, 10.0, 0.001, Seconds),
    ParamInfo::linear("Env1 H", 0.0, 5.0, 0.0, Seconds),
    ParamInfo::log("Env1 D", 0.001, 10.0, 1.0, Seconds),
    ParamInfo::linear("Env1 S", 0.0, 1.0, 1.0, Percent),
    ParamInfo::log("Env1 R", 0.001, 10.0, 0.1, Seconds),
    ParamInfo::log("Env2 A", 0.0005, 10.0, 0.001, Seconds),
    ParamInfo::linear("Env2 H", 0.0, 5.0, 0.0, Seconds),
    ParamInfo::log("Env2 D", 0.001, 10.0, 1.0, Seconds),
    ParamInfo::linear("Env2 S", 0.0, 1.0, 1.0, Percent),
    ParamInfo::log("Env2 R", 0.001, 10.0, 0.1, Seconds),
    ParamInfo::log("Env3 A", 0.0005, 10.0, 0.001, Seconds),
    ParamInfo::linear("Env3 H", 0.0, 5.0, 0.0, Seconds),
    ParamInfo::log("Env3 D", 0.001, 10.0, 1.0, Seconds),
    ParamInfo::linear("Env3 S", 0.0, 1.0, 1.0, Percent),
    ParamInfo::log("Env3 R", 0.001, 10.0, 0.1, Seconds),
    ParamInfo::log("LFO1 Rate", 0.01, 30.0, 2.0, Hertz),
    ParamInfo::linear("LFO1 Rise", 0.0, 5.0, 0.0, Seconds),
    ParamInfo::log("LFO2 Rate", 0.01, 30.0, 2.0, Hertz),
    ParamInfo::linear("LFO2 Rise", 0.0, 5.0, 0.0, Seconds),
    ParamInfo::log("LFO3 Rate", 0.01, 30.0, 2.0, Hertz),
    ParamInfo::linear("LFO3 Rise", 0.0, 5.0, 0.0, Seconds),
    ParamInfo::log("LFO4 Rate", 0.01, 30.0, 2.0, Hertz),
    ParamInfo::linear("LFO4 Rise", 0.0, 5.0, 0.0, Seconds),
    ParamInfo::linear("Macro 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Macro 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Macro 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Macro 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Glide", 0.0, 2000.0, 0.0, Milliseconds),
    ParamInfo::linear("Master", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod 1", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 2", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 3", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 4", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 5", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 6", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 7", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 8", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 9", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 10", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 11", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 12", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 13", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 14", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 15", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod 16", -1.0, 1.0, 0.0, Percent),
    ParamInfo::log("A Grain", 5.0, 500.0, 80.0, Milliseconds),
    ParamInfo::log("A Density", 1.0, 200.0, 30.0, Count),
    ParamInfo::linear("A Spray", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("A G.Pitch", 0.0, 24.0, 0.0, Semitones),
    ParamInfo::log("B Grain", 5.0, 500.0, 80.0, Milliseconds),
    ParamInfo::log("B Density", 1.0, 200.0, 30.0, Count),
    ParamInfo::linear("B Spray", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("B G.Pitch", 0.0, 24.0, 0.0, Semitones),
    ParamInfo::linear("Arp Gate", 0.05, 1.0, 0.5, Percent),
];

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

    #[test]
    fn shapes_interpolate_bend_and_edit() {
        let mut shape = Shape::falling();
        assert_eq!(shape.value_at(0.0), 1.0);
        assert!((shape.value_at(0.25) - 0.75).abs() < 1e-6);
        assert_eq!(shape.value_at(1.0), 0.0);

        let index = shape.insert(0.5, 0.8).unwrap();
        assert_eq!((index, shape.count), (1, 3));
        assert!((shape.value_at(0.5) - 0.8).abs() < 1e-6);
        shape.points[2].bend = 1.0;
        assert!(shape.value_at(0.6) < 0.5, "a positive bend drops fast: {}", shape.value_at(0.6));

        shape.move_point(1, 2.0, 0.3);
        assert!(shape.points[1].time < 1.0, "inner points stay between their neighbours");
        shape.move_point(0, 0.5, 0.9);
        assert_eq!(shape.points[0].time, 0.0, "the first point stays at the start");
        shape.remove(0);
        assert_eq!(shape.count, 3, "ends cannot be removed");
        shape.remove(1);
        assert_eq!(shape.count, 2);
        for _ in 0..10 {
            shape.insert(0.5, 0.5);
        }
        assert_eq!(shape.count as usize, SHAPE_POINTS);
    }

    #[test]
    fn kick_projects_saved_before_shapes_load_with_straight_lines() {
        let json = serde_json::to_string(&KickParams::default()).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value.as_object_mut().unwrap().remove("pitch_shape");
        value.as_object_mut().unwrap().remove("amp_shape");
        let loaded: KickParams = serde_json::from_value(value).unwrap();
        assert_eq!(loaded, KickParams::default());
    }
}
