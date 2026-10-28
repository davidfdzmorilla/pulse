//! Plain data types shared between the UI and the audio thread.

use serde::{Deserialize, Serialize};

/// A MIDI note inside a clip. Times are in beats, relative to the clip start.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub start: f64,
    pub length: f64,
    pub pitch: u8,
    pub velocity: f32,
    /// How likely it is to play each time round, 0..1 (see `Note::plays`).
    #[serde(default = "always", skip_serializing_if = "is_always")]
    pub chance: f32,
}

fn always() -> f32 {
    1.0
}

fn is_always(chance: &f32) -> bool {
    *chance >= 1.0
}

impl Note {
    pub fn new(start: f64, length: f64, pitch: u8, velocity: f32) -> Self {
        Note { start, length, pitch, velocity, chance: 1.0 }
    }

    /// Whether the note plays on lap `lap` of its clip, as note `index`: by
    /// chance, but the same chance every time (playing, exporting, freezing).
    pub fn plays(&self, index: usize, lap: i64) -> bool {
        if self.chance >= 1.0 {
            return true;
        }
        let mut x = (lap as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ ((index as u64) << 32) ^ ((self.pitch as u64) << 56) ^ self.start.to_bits();
        // splitmix64's finish.
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        x ^= x >> 31;
        ((x >> 40) as f32 / (1u64 << 24) as f32) < self.chance
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MidiClip {
    pub name: String,
    /// Loop length in beats.
    pub length: f64,
    pub notes: Vec<Note>,
    pub color: [u8; 3],
    /// Clip automation: parameter lanes in clip time, looping with the clip.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub envelopes: Vec<AutomationLane>,
    /// The clip's own groove; none: the project's swing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groove: Option<Groove>,
    /// Switched off: it stays where it is, silent (see [`Clip::muted`]).
    #[serde(default, skip_serializing_if = "is_default")]
    pub muted: bool,
}

/// How a clip's notes are played off the grid, as Live's grooves: swing on the
/// off-beats of `grid`, softer off-beats (`accent`) and small random shifts in
/// time and velocity (`humanize`). Applied on playback; the notes stay as written.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Groove {
    /// Beats of the grid swung (0.5 eighths, 0.25 sixteenths).
    pub grid: f64,
    /// 0..1: how late the off-beats come (1: a triplet shuffle).
    pub swing: f32,
    /// 0..1: how much softer the off-beats are.
    pub accent: f32,
    /// 0..1: random shifts, up to a 32nd of a beat in time and 15 % in velocity.
    pub humanize: f32,
    /// A groove taken from a recording, step by step: then `swing` is how
    /// much of its timing plays and `accent` how much of its dynamics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<GrooveTemplate>,
}

/// Steps in a [`GrooveTemplate`]: a bar of sixteenths (two of eighths).
pub const GROOVE_STEPS: usize = 16;

/// How a recording plays the grid, step by step over [`GROOVE_STEPS`] steps:
/// how far off each its hits come (a fraction of a step, later if positive)
/// and how hard (1 the loudest).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GrooveTemplate {
    pub shift: [f32; GROOVE_STEPS],
    pub velocity: [f32; GROOVE_STEPS],
}

impl Default for Groove {
    fn default() -> Self {
        Self { grid: 0.25, swing: 0.0, accent: 0.0, humanize: 0.0, template: None }
    }
}

/// A number in -1..1 that is always the same for `seed` (a note on a lap).
fn jitter(seed: u64) -> f32 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f32 / (1u64 << 53) as f32 * 2.0 - 1.0
}

impl Groove {
    /// The step of the grid `start` falls on, if it falls on one.
    fn step(&self, start: f64) -> Option<i64> {
        let step = start / self.grid.max(1.0 / 64.0);
        let nearest = step.round();
        ((step - nearest).abs() < 1e-6).then_some(nearest as i64)
    }

    /// Whether some notes may play before where they are written (humanized,
    /// or a template's hits early).
    pub fn may_come_early(&self) -> bool {
        self.humanize > 0.0 || self.template.is_some_and(|t| t.shift.iter().any(|&s| s < 0.0))
    }

    /// Whether `start` falls on an off-beat of the grid (its 2nd, 4th… step).
    fn off_beat(&self, start: f64) -> bool {
        let step = start / self.grid.max(1.0 / 64.0);
        let nearest = step.round();
        (step - nearest).abs() < 1e-6 && (nearest as i64).rem_euclid(2) == 1
    }

    /// When note `index` (starting at `start` in the clip) plays on lap `lap`:
    /// beats later (or earlier, humanized), and its velocity.
    pub fn apply(&self, index: usize, lap: i64, start: f64, velocity: f32) -> (f64, f32) {
        let (mut shift, mut velocity) = match &self.template {
            // On a step of the grid: as the recording played it.
            Some(template) => match self.step(start) {
                Some(k) => {
                    let k = k.rem_euclid(GROOVE_STEPS as i64) as usize;
                    let timing = (template.shift[k].clamp(-0.5, 0.5) * self.swing.clamp(0.0, 1.0)) as f64 * self.grid;
                    (timing, velocity * (1.0 - self.accent.clamp(0.0, 1.0) * (1.0 - template.velocity[k].clamp(0.0, 1.0))))
                }
                None => (0.0, velocity),
            },
            None => {
                let off = self.off_beat(start);
                let shift = if off { self.grid * self.swing.clamp(0.0, 1.0) as f64 / 3.0 } else { 0.0 };
                (shift, velocity * if off { 1.0 - 0.6 * self.accent.clamp(0.0, 1.0) } else { 1.0 })
            }
        };
        if self.humanize > 0.0 {
            let seed = (index as u64) << 32 ^ lap as u64;
            shift += jitter(seed) as f64 * self.humanize as f64 / 32.0;
            velocity *= 1.0 + 0.15 * self.humanize * jitter(seed ^ 0xA5A5_A5A5);
        }
        (shift, velocity.clamp(0.01, 1.0))
    }
}

impl MidiClip {
    pub fn new(name: impl Into<String>, length: f64, color: [u8; 3]) -> Self {
        Self {
            groove: None,
            envelopes: Vec::new(),
            name: name.into(),
            length,
            notes: Vec::new(),
            color, muted: false,
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
    /// For drums: each hit starts where the tempo puts it and plays at the
    /// file's speed, fading out before the next (no grains smearing the hits).
    Beats,
}

impl WarpMode {
    pub const ALL: [WarpMode; 4] = [Self::Off, Self::Repitch, Self::Stretch, Self::Beats];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Repitch => "Re-Pitch",
            Self::Stretch => "Stretch",
            Self::Beats => "Beats",
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
    /// Clip automation: parameter lanes in clip time, looping with the clip.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub envelopes: Vec<AutomationLane>,
    /// Warp markers: which point of the file plays on which beat, for files whose
    /// tempo varies. Sorted, both beats and seconds rising. With fewer than two the
    /// file follows `original_bpm` throughout.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warp_markers: Vec<WarpMarker>,
    /// The other takes of a loop recording (files like `sample`, which is the one
    /// playing); the clip's menu swaps them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub takes: Vec<String>,
    /// The file's key, if it has one (from its name or heard), and whether the
    /// clip is transposed to follow the project's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<ClipKey>,
    /// Switched off: it stays where it is, silent.
    #[serde(default, skip_serializing_if = "is_default")]
    pub muted: bool,
    /// Its first hit plays untouched, with no fade-in: the file starts where
    /// the sound starts (a frozen track, rendered from the song's start).
    #[serde(default, skip_serializing_if = "is_default")]
    pub hard_start: bool,
    /// Its fade-in and fade-out (see [`Fades`]).
    #[serde(default, skip_serializing_if = "is_default")]
    pub fades: Fades,
}

/// How long an audio clip takes to come in from silence at its start and to
/// go back to it at its end, in beats (they follow the tempo). Equal-power
/// curves: no dip in the middle. In the Arrangement, at the clip's edges as
/// placed; in the Session, on every lap of its loop.
///
/// A fade may be a piece of its curve (a clip split in the middle of one keeps
/// its sound): the fade-in goes from `in_from` to `in_to` along it (0 is
/// silence, 1 full), the fade-out from `out_from` where it begins to `out_to`
/// at the clip's end. A whole fade goes 0 → 1, and 1 → 0.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fades {
    #[serde(default, rename = "in", skip_serializing_if = "is_default")]
    pub fade_in: f64,
    #[serde(default, rename = "out", skip_serializing_if = "is_default")]
    pub fade_out: f64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub in_from: f64,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub in_to: f64,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub out_from: f64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub out_to: f64,
}

fn one() -> f64 {
    1.0
}

fn is_one(x: &f64) -> bool {
    *x == 1.0
}

impl Default for Fades {
    fn default() -> Self {
        Self::NONE
    }
}

impl Fades {
    pub const NONE: Fades = Fades { fade_in: 0.0, fade_out: 0.0, in_from: 0.0, in_to: 1.0, out_from: 1.0, out_to: 0.0 };

    /// Whole fades of these lengths.
    pub const fn new(fade_in: f64, fade_out: f64) -> Fades {
        Fades { fade_in, fade_out, ..Fades::NONE }
    }

    /// The curve's gain at `x` (0..1) along it.
    pub fn curve(x: f64) -> f64 {
        (x.clamp(0.0, 1.0) * std::f64::consts::FRAC_PI_2).sin()
    }

    /// Where along its curve the fade-in is `since` beats after the start.
    pub fn in_at(&self, since: f64) -> f64 {
        self.in_from + (self.in_to - self.in_from) * (since / self.fade_in).clamp(0.0, 1.0)
    }

    /// Where along its curve the fade-out is `until` beats before the end.
    pub fn out_at(&self, until: f64) -> f64 {
        self.out_to + (self.out_from - self.out_to) * (until / self.fade_out).clamp(0.0, 1.0)
    }

    /// The gain `since` beats after the start and `until` beats before the end.
    pub fn gain(&self, since: f64, until: f64) -> f32 {
        let fade_in = if self.fade_in > 0.0 { Self::curve(self.in_at(since)) } else { 1.0 };
        let fade_out = if self.fade_out > 0.0 { Self::curve(self.out_at(until)) } else { 1.0 };
        (fade_in * fade_out) as f32
    }

    /// Kept inside a clip `length` beats long: each at most all of it, both
    /// together too.
    pub fn fitted(self, length: f64) -> Fades {
        let fade_in = self.fade_in.clamp(0.0, length.max(0.0));
        Fades { fade_in, fade_out: self.fade_out.clamp(0.0, (length - fade_in).max(0.0)), ..self }
    }

    /// Whether one is only a piece of its curve (see the type's notes).
    pub fn partial(&self) -> bool {
        (self.fade_in > 0.0 && (self.in_from != 0.0 || self.in_to != 1.0)) || (self.fade_out > 0.0 && (self.out_from != 1.0 || self.out_to != 0.0))
    }

    /// Without a fade-in (and the next one whole).
    pub fn without_in(self) -> Fades {
        Fades { fade_in: 0.0, in_from: 0.0, in_to: 1.0, ..self }
    }

    /// Without a fade-out (and the next one whole).
    pub fn without_out(self) -> Fades {
        Fades { fade_out: 0.0, out_from: 1.0, out_to: 0.0, ..self }
    }

    /// The two parts of these fades (fitted to a clip `length` long) when the
    /// clip is cut `at` beats from its start: each part with the piece of each
    /// curve it has, so together they sound as the whole.
    pub fn split(self, length: f64, at: f64) -> (Fades, Fades) {
        let f = self.fitted(length);
        let (mut left, mut right) = (f.without_out(), f.without_in());
        if f.fade_in > at {
            let cut = f.in_at(at);
            left = Fades { fade_in: at, in_to: cut, ..left };
            right = Fades { fade_in: f.fade_in - at, in_from: cut, in_to: f.in_to, ..right };
        } else {
            right = right.without_in();
        }
        let begins = length - f.fade_out;
        if f.fade_out > 0.0 && at > begins {
            let cut = f.out_at(length - at);
            left = Fades { fade_out: at - begins, out_from: f.out_from, out_to: cut, ..left };
            right = Fades { fade_out: length - at, out_from: cut, ..right };
        }
        (left, right)
    }
}

/// The key an audio file is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipKey {
    /// Pitch class of the tonic (0 = C).
    pub tonic: u8,
    pub minor: bool,
    /// Transposed to the project's key (see the app's `loop_key`).
    pub follow: bool,
}

/// A point of an audio file (`seconds`) pinned to a beat of the file's own beat
/// time (`beat`); between two markers the file plays evenly.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WarpMarker {
    pub beat: f64,
    pub seconds: f64,
}

impl AudioClip {
    /// Whether the clip plays between warp markers.
    pub fn has_markers(&self) -> bool {
        self.warp_markers.len() >= 2
    }

    /// The file's time at a beat of its own beat time: between the markers around
    /// it, or past the outer ones at the tempo of the nearest stretch. Without
    /// markers, at `original_bpm` from beat 0 at the file's start.
    pub fn file_seconds(&self, beat: f64) -> f64 {
        let m = &self.warp_markers;
        if m.len() < 2 {
            return beat * 60.0 / self.original_bpm.max(1.0);
        }
        let i = m.partition_point(|x| x.beat <= beat).clamp(1, m.len() - 1);
        let (a, b) = (m[i - 1], m[i]);
        a.seconds + (beat - a.beat) * (b.seconds - a.seconds) / (b.beat - a.beat).max(1e-9)
    }

    /// The beat (of the file's own beat time) at a time of the file; the inverse
    /// of [`AudioClip::file_seconds`].
    pub fn file_beat(&self, seconds: f64) -> f64 {
        let m = &self.warp_markers;
        if m.len() < 2 {
            return seconds * self.original_bpm.max(1.0) / 60.0;
        }
        let i = m.partition_point(|x| x.seconds <= seconds).clamp(1, m.len() - 1);
        let (a, b) = (m[i - 1], m[i]);
        a.beat + (seconds - a.seconds) * (b.beat - a.beat) / (b.seconds - a.seconds).max(1e-9)
    }

    /// Where in the file (seconds) the clip is `local` beats after its start, at
    /// project `tempo` (which only matters with warp off).
    pub fn source_seconds(&self, local: f64, tempo: f64) -> f64 {
        match self.warp {
            WarpMode::Off => self.offset + local * 60.0 / tempo.max(1.0),
            _ => self.file_seconds(self.file_beat(self.offset) + local),
        }
    }

    /// How many beats of the clip the file has from its start to `seconds`.
    pub fn beats_until(&self, seconds: f64, tempo: f64) -> f64 {
        match self.warp {
            WarpMode::Off => (seconds - self.offset) * tempo / 60.0,
            _ => self.file_beat(seconds) - self.file_beat(self.offset),
        }
    }

    /// Sets the original tempo; with markers, the file's beats are counted again
    /// at it (twice the tempo: twice the beats), so the markers keep their points.
    pub fn set_original_bpm(&mut self, bpm: f64) {
        let ratio = bpm / self.original_bpm.max(1.0);
        for m in &mut self.warp_markers {
            m.beat *= ratio;
        }
        self.original_bpm = bpm;
    }
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

    /// The clip's own automation (see [`MidiClip::envelopes`]).
    pub fn envelopes(&self) -> &[AutomationLane] {
        match self {
            Clip::Midi(c) => &c.envelopes,
            Clip::Audio(c) => &c.envelopes,
        }
    }

    pub fn envelopes_mut(&mut self) -> &mut Vec<AutomationLane> {
        match self {
            Clip::Midi(c) => &mut c.envelopes,
            Clip::Audio(c) => &mut c.envelopes,
        }
    }

    /// Switched off: it stays, silent (its notes, audio and envelopes).
    pub fn muted(&self) -> bool {
        match self {
            Clip::Midi(c) => c.muted,
            Clip::Audio(c) => c.muted,
        }
    }

    pub fn set_muted(&mut self, muted: bool) {
        match self {
            Clip::Midi(c) => c.muted = muted,
            Clip::Audio(c) => c.muted = muted,
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

impl SynthParams {
    /// Factory presets: (name as «Categoría · Nombre», settings).
    pub fn presets() -> Vec<(&'static str, SynthParams)> {
        use Waveform::*;
        let base = SynthParams::default();
        // The synth has no filter envelope: plucks and stabs come from the amplitude
        // decay (sustain 0 or low), the tone from the waveform and the fixed cutoff.
        let tone = |waveform, cutoff, resonance, gain| SynthParams { waveform, cutoff, resonance, gain, ..base };
        vec![
            ("Init", base),
            ("Bajo · Sub puro", SynthParams { attack: 0.002, decay: 0.3, sustain: 1.0, release: 0.08, ..tone(Sine, 18000.0, 0.0, 0.22) }),
            ("Bajo · Triángulo cálido", SynthParams { attack: 0.002, decay: 0.4, sustain: 0.8, release: 0.1, ..tone(Triangle, 900.0, 0.1, 0.33) }),
            ("Bajo · Sierra con mordida", SynthParams { attack: 0.001, decay: 0.25, sustain: 0.55, release: 0.06, ..tone(Saw, 850.0, 0.55, 0.29) }),
            ("Bajo · Pulso corto", SynthParams { attack: 0.001, decay: 0.22, sustain: 0.3, release: 0.06, ..tone(Square, 700.0, 0.2, 0.29) }),
            ("Lead · Sierra brillante", SynthParams { attack: 0.004, decay: 0.3, sustain: 0.8, release: 0.25, ..tone(Saw, 7000.0, 0.15, 0.27) }),
            ("Lead · Chip cuadrado", SynthParams { attack: 0.001, decay: 0.05, sustain: 1.0, release: 0.03, ..tone(Square, 18000.0, 0.0, 0.16) }),
            ("Lead · Flauta suave", SynthParams { attack: 0.04, decay: 0.3, sustain: 0.9, release: 0.3, ..tone(Triangle, 3000.0, 0.0, 0.27) }),
            ("Lead · Resonante", SynthParams { attack: 0.003, decay: 0.4, sustain: 0.75, release: 0.2, ..tone(Saw, 1800.0, 0.65, 0.22) }),
            ("Pluck · Corto", SynthParams { attack: 0.001, decay: 0.22, sustain: 0.0, release: 0.2, ..tone(Saw, 3000.0, 0.3, 0.35) }),
            ("Pluck · Marimba", SynthParams { attack: 0.001, decay: 0.35, sustain: 0.0, release: 0.3, ..tone(Triangle, 5000.0, 0.0, 0.35) }),
            ("Pluck · Arpegio de 8 bits", SynthParams { attack: 0.001, decay: 0.12, sustain: 0.0, release: 0.05, ..tone(Square, 6000.0, 0.0, 0.3) }),
            ("Pad · Cálido", SynthParams { attack: 0.8, decay: 1.0, sustain: 0.8, release: 1.5, ..tone(Saw, 1200.0, 0.1, 0.35) }),
            ("Pad · Hueco", SynthParams { attack: 1.2, decay: 1.0, sustain: 0.85, release: 2.0, ..tone(Square, 900.0, 0.2, 0.23) }),
            ("Pad · Niebla", SynthParams { attack: 1.5, decay: 1.0, sustain: 1.0, release: 2.5, ..tone(Triangle, 1500.0, 0.0, 0.36) }),
            ("Acordes · Stab de house", SynthParams { attack: 0.001, decay: 0.3, sustain: 0.15, release: 0.15, ..tone(Saw, 2200.0, 0.35, 0.33) }),
            ("Acordes · Órgano cuadrado", SynthParams { attack: 0.005, decay: 0.1, sustain: 1.0, release: 0.05, ..tone(Square, 2500.0, 0.0, 0.13) }),
        ]
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
    /// The same engine laid out around its analog, harmonic and sample engines,
    /// two filters and the step sequencer.
    Polychrome(WaveParams),
    /// Free-form synth: generators with modifier chains, effect lanes and modulators.
    Modular(ModularParams),
    /// Pulse Keys: synthesized piano, electric pianos and organ.
    Keys(KeysParams),
    /// A VST3 plugin: which one, and its state, is in the project's plugin list.
    Plugin(PluginId),
}

/// A plugin instance in the project (see `Project::plugins` in the app).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginId(pub u64);

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
                    pitch_time: 500.0,
                    pitch_curve: 0.0,
                    pitch_shape: shape(&[(0.0, 1.0, 0.0), (0.056, 0.6, 0.8), (1.0, 0.0, -0.3)]),
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
            (
                // A worn sample: tape-warm, boxy, no highs, a soft thud of noise for the beater.
                "Lo-Fi · Polvoriento",
                KickParams { tune: 30.0, pitch_amount: 18.0, pitch_time: 40.0, pitch_curve: 0.4, hold: 20.0, decay: 200.0, decay_curve: 0.5, harmonics: 0.15, click: 0.3, click_tone: 2000.0, click_length: 20.0, click_type: 6, drive: 0.35, drive_mode: KickDrive::Tube, eq_mid: 3.0, eq_mid_freq: 250.0, eq_high: -10.0, gain: 0.7, velocity: 0.6, limiter: true, ..base },
            ),
            (
                // Round and warm, with a woody knock: sits under congas and shakers.
                "Afro House · Cálido",
                KickParams { tune: 31.0, pitch_amount: 28.0, pitch_time: 50.0, hold: 50.0, decay: 280.0, harmonics: 0.1, click: 0.35, click_tone: 1200.0, click_type: 7, drive: 0.2, sub_level: 0.2, eq_mid: -2.0, eq_mid_freq: 300.0, punch: 0.25, limiter: true, ..base },
            ),
            ("Reggaetón · Dembow", KickParams { tune: 31.0, pitch_amount: 30.0, pitch_time: 40.0, hold: 60.0, decay: 380.0, decay_curve: 0.35, click: 0.45, click_tone: 3000.0, click_type: 2, drive: 0.3, drive_mode: KickDrive::Tube, sub_level: 0.35, punch: 0.35, limiter: true, ..base }),
            (
                // Tight and loud: a hard knock, the low mids scooped, compressed hard.
                "Dubstep · Pesado",
                KickParams { tune: 33.0, pitch_amount: 48.0, pitch_time: 35.0, pitch_curve: 0.8, hold: 40.0, decay: 300.0, decay_curve: 0.5, harmonics: 0.2, click: 0.75, click_tone: 5000.0, click_type: 8, drive: 0.5, gain: 0.6, sub_level: 0.3, eq_mid: -4.0, eq_mid_freq: 350.0, eq_high: 3.0, phase: 90.0, punch: 0.5, limiter: true, ..base },
            ),
            (
                // A slow attack and a gentle sweep: a felt-like pulse with no click to speak of.
                "Ambient · Suave",
                KickParams { tune: 29.0, pitch_amount: 14.0, pitch_time: 80.0, pitch_curve: 0.3, attack: 3.0, hold: 40.0, decay: 600.0, decay_curve: 0.5, click: 0.1, click_tone: 2500.0, click_length: 20.0, click_type: 9, drive: 0.0, gain: 0.85, sub_level: 0.25, eq_high: -6.0, velocity: 0.8, ..base },
            ),
            (
                // Higher and bouncy, with a little pitch blip: skippy two-step grooves.
                "UK Garage · Rebotado",
                KickParams { tune: 36.0, pitch_amount: 30.0, pitch_time: 30.0, pitch_curve: 0.7, hold: 20.0, decay: 170.0, decay_curve: 0.55, harmonics: 0.2, click: 0.5, click_tone: 2200.0, click_length: 5.0, click_type: 4, drive: 0.25, phase: 90.0, punch: 0.35, eq_low: 2.0, limiter: true, ..base },
            ),
            (
                // An acoustic kick through an old sampler: the beater's knock, boxiness scooped, crunchy.
                "Hip Hop · Boom Bap",
                KickParams { tune: 33.0, pitch_amount: 20.0, pitch_time: 30.0, pitch_curve: 0.5, hold: 20.0, decay: 220.0, decay_curve: 0.55, harmonics: 0.3, click: 0.55, click_tone: 2500.0, click_length: 8.0, click_type: 7, drive: 0.35, drive_mode: KickDrive::Tube, eq_mid: -3.0, eq_mid_freq: 400.0, eq_high: 2.0, punch: 0.45, velocity: 0.8, limiter: true, ..base },
            ),
            (
                // Gated: it holds full, then shuts fast; a tom-like sweep and a metallic edge.
                "Synthwave · Ochentero",
                KickParams { tune: 33.0, pitch_amount: 30.0, pitch_time: 60.0, pitch_curve: 0.5, hold: 100.0, decay: 120.0, decay_curve: 0.1, harmonics: 0.15, click: 0.5, click_tone: 3000.0, click_type: 5, drive: 0.2, eq_high: 2.0, punch: 0.3, limiter: true, ..base },
            ),
            (
                // A long, hot 808: punchy start and a saturated tail that cuts through on small speakers.
                "808 · Saturado",
                KickParams { tune: 30.0, pitch_amount: 36.0, pitch_time: 30.0, pitch_curve: 0.7, hold: 150.0, decay: 700.0, decay_curve: 0.4, click: 0.4, click_tone: 2500.0, click_type: 2, drive: 0.45, sub_level: 0.3, punch: 0.3, limiter: true, ..base },
            ),
        ]
    }
}

/// Most zones a multisampled sampler holds.
pub const SAMPLER_ZONES: usize = 32;

/// A zone of a multisampled sampler: which notes and velocities (1-127) play
/// the track's sample of the same index, and the note it was recorded at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub lo: u8,
    pub hi: u8,
    pub root: u8,
    pub vel_lo: u8,
    pub vel_hi: u8,
}

impl Zone {
    pub fn plays(&self, pitch: u8, velocity: f32) -> bool {
        let velocity = (velocity * 127.0).round().clamp(1.0, 127.0) as u8;
        (self.lo..=self.hi).contains(&pitch) && (self.vel_lo..=self.vel_hi).contains(&velocity)
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
    /// Multisampled: the first `zone_count` zones map notes to the track's
    /// samples (zone i plays sample i). None: sample 0 over the whole keyboard,
    /// from `root`.
    #[serde(default, skip_serializing_if = "no_zones")]
    pub zones: [Zone; SAMPLER_ZONES],
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub zone_count: u8,
}

fn no_zones(zones: &[Zone; SAMPLER_ZONES]) -> bool {
    zones.iter().all(|z| *z == Zone::default())
}

fn is_zero_u8(value: &u8) -> bool {
    *value == 0
}

impl SamplerParams {
    /// The zones in use.
    pub fn zones(&self) -> &[Zone] {
        &self.zones[..(self.zone_count as usize).min(SAMPLER_ZONES)]
    }
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
            zones: [Zone::default(); SAMPLER_ZONES],
            zone_count: 0,
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
    pub const MID_TOM: u8 = 48;
    pub const HIGH_TOM: u8 = 50;
    pub const CRASH: u8 = 49;
    pub const RIDE: u8 = 51;
    pub const SHAKER: u8 = 70;
    pub const TAMBOURINE: u8 = 54;
    pub const CONGA: u8 = 63;
    pub const CLAVE: u8 = 75;
}

/// How many sounds the drum machine has.
pub const DRUM_SOUNDS: usize = 16;

/// The drum machine's sounds, in parameter order, with their General MIDI notes.
/// The first eight were its sounds before it had sixteen, and keep their places.
pub const DRUM_VOICES: [(u8, &str); DRUM_SOUNDS] = [
    (drum_note::KICK, "Kick"),
    (drum_note::RIM, "Rim"),
    (drum_note::SNARE, "Snare"),
    (drum_note::CLAP, "Clap"),
    (drum_note::CLOSED_HAT, "Closed Hat"),
    (drum_note::OPEN_HAT, "Open Hat"),
    (drum_note::LOW_TOM, "Low Tom"),
    (drum_note::COWBELL, "Cowbell"),
    (drum_note::MID_TOM, "Mid Tom"),
    (drum_note::HIGH_TOM, "High Tom"),
    (drum_note::CRASH, "Crash"),
    (drum_note::RIDE, "Ride"),
    (drum_note::SHAKER, "Shaker"),
    (drum_note::TAMBOURINE, "Tambourine"),
    (drum_note::CONGA, "Conga"),
    (drum_note::CLAVE, "Clave"),
];

/// Per-sound tuning (semitones), decay (multiplier) and level of the drum machine.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrumParams {
    #[serde(deserialize_with = "drum_tunes")]
    pub tune: [f32; DRUM_SOUNDS],
    #[serde(deserialize_with = "drum_decays")]
    pub decay: [f32; DRUM_SOUNDS],
    #[serde(deserialize_with = "drum_levels")]
    pub level: [f32; DRUM_SOUNDS],
    /// How hard each sound is driven into a soft saturation (0: clean).
    #[serde(default, skip_serializing_if = "clean")]
    pub drive: [f32; DRUM_SOUNDS],
    /// Which machine it sounds like.
    #[serde(default, skip_serializing_if = "is_classic")]
    pub model: DrumModel,
}

impl Default for DrumParams {
    fn default() -> Self {
        Self { tune: [0.0; DRUM_SOUNDS], decay: [1.0; DRUM_SOUNDS], level: [0.8; DRUM_SOUNDS], drive: [0.0; DRUM_SOUNDS], model: DrumModel::Classic }
    }
}

impl DrumParams {
    /// Factory presets: (name as «Categoría · Nombre», settings).
    pub fn presets() -> Vec<(&'static str, DrumParams)> {
        let base = DrumParams::default();
        // Each kit's tune, decay, level and drive, a value a sound in `DRUM_VOICES` order:
        // kick, rim, snare, clap, closed and open hat, low tom, cowbell, mid and high tom,
        // crash, ride, shaker, tambourine, conga, clave. The sounds a style doesn't use
        // stay low but playable. (The 808's hats are soft: a touch of drive lifts them.)
        let kit = |model: DrumModel, tune: [f32; DRUM_SOUNDS], decay: [f32; DRUM_SOUNDS], level: [f32; DRUM_SOUNDS], drive: [f32; DRUM_SOUNDS]| DrumParams { tune, decay, level, drive, model };
        vec![
            ("Init", base),
            (
                // Four on the floor: a round 909 kick, a long clap and the open hat on the off-beat.
                "House · Clásico",
                kit(
                    DrumModel::Tr909,
                    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    [0.9, 1.0, 1.0, 1.2, 0.8, 0.7, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                    [0.62, 0.4, 0.35, 0.7, 0.55, 0.5, 0.35, 0.3, 0.35, 0.35, 0.4, 0.8, 0.45, 0.45, 0.4, 0.35],
                    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Soft and warm: a rounder 808 kick, a rim, a dark long clap and soft hats.
                "House · Deep",
                kit(
                    DrumModel::Tr808,
                    [1.0, 0.0, 0.0, -1.0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -2.0, 0.0, 0.0, 0.0],
                    [1.1, 1.0, 1.0, 1.4, 1.2, 0.9, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.2, 1.0, 1.2, 1.0],
                    [0.85, 0.6, 0.35, 0.65, 0.9, 0.7, 0.35, 0.3, 0.35, 0.35, 0.35, 0.6, 0.55, 0.35, 0.5, 0.4],
                    [0.0, 0.0, 0.0, 0.0, 0.08, 0.06, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Tight and dry: a short, slightly clipped kick, a quick clap, short hats and bongo-like congas.
                "Tech House · Seco",
                kit(
                    DrumModel::Tr909,
                    [1.0, 2.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 3.0, 0.0],
                    [0.7, 1.0, 1.0, 0.9, 0.6, 0.55, 1.0, 1.0, 1.0, 1.0, 1.0, 0.8, 0.9, 0.8, 0.8, 1.0],
                    [0.62, 0.55, 0.35, 0.7, 0.6, 0.55, 0.35, 0.3, 0.35, 0.35, 0.35, 0.5, 0.55, 0.5, 0.5, 0.4],
                    [0.03, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Lower and longer: a deep kick with a little grit, a dark clap, rim and ride.
                "Techno · Oscuro",
                kit(
                    DrumModel::Tr909,
                    [-2.0, 0.0, -2.0, -2.0, 0.0, 1.0, -2.0, 0.0, -2.0, -2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    [1.1, 1.0, 1.0, 1.1, 0.8, 0.6, 1.0, 1.0, 1.0, 1.0, 1.0, 1.2, 1.0, 1.0, 1.0, 1.0],
                    [0.6, 0.55, 0.4, 0.65, 0.6, 0.55, 0.4, 0.3, 0.4, 0.4, 0.4, 0.85, 0.45, 0.35, 0.35, 0.35],
                    [0.04, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Distorted: a long, clipped kick, a crushed clap and snare, and dirty low toms.
                "Techno · Industrial",
                kit(
                    DrumModel::Tr909,
                    [-3.0, -2.0, -3.0, -3.0, 1.0, 0.0, -4.0, 0.0, -4.0, -4.0, -2.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    [1.5, 1.0, 1.2, 1.4, 0.7, 0.7, 1.2, 1.0, 1.2, 1.2, 1.2, 1.0, 1.0, 1.0, 1.0, 1.0],
                    [0.48, 0.4, 0.42, 0.5, 0.45, 0.42, 0.32, 0.3, 0.32, 0.32, 0.36, 0.5, 0.35, 0.3, 0.3, 0.3],
                    [0.35, 0.15, 0.12, 0.12, 0.1, 0.08, 0.1, 0.0, 0.1, 0.1, 0.15, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Short and clicky: a short kick, a bright rim, a small clap, shaker, clave and high congas.
                "Minimal · Clic",
                kit(
                    DrumModel::Tr909,
                    [1.0, 3.0, 0.0, 1.0, 2.0, 1.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 2.0, 0.0],
                    [0.55, 0.8, 1.0, 0.8, 0.5, 0.5, 1.0, 0.8, 1.0, 1.0, 1.0, 0.8, 0.7, 0.8, 0.7, 1.0],
                    [0.6, 0.65, 0.3, 0.55, 0.55, 0.35, 0.3, 0.35, 0.3, 0.3, 0.3, 0.45, 0.6, 0.45, 0.5, 0.55],
                    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // A long, slightly saturated 808 boom, a tight snare and clap, and bright hats for rolls.
                "Trap · 808 largo",
                kit(
                    DrumModel::Tr808,
                    [-2.0, 0.0, 2.0, 0.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    [3.2, 1.0, 0.9, 1.0, 0.6, 0.8, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                    [0.78, 0.5, 0.75, 0.8, 0.9, 0.6, 0.35, 0.3, 0.35, 0.35, 0.35, 0.35, 0.4, 0.35, 0.35, 0.35],
                    [0.04, 0.0, 0.0, 0.0, 0.08, 0.06, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Knocking: a full, gritty kick, a fat low snare with some crunch, dull hats, rim and shaker.
                "Hip Hop · Boom Bap",
                kit(
                    DrumModel::Classic,
                    [-1.0, -1.0, -2.0, -1.0, -1.0, -2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0],
                    [1.2, 1.0, 1.4, 1.0, 1.0, 0.8, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                    [0.95, 0.6, 0.85, 0.45, 0.85, 0.6, 0.35, 0.3, 0.35, 0.35, 0.35, 0.4, 0.5, 0.45, 0.35, 0.35],
                    [0.03, 0.0, 0.05, 0.0, 0.15, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Muffled and soft: everything tuned down and shortened, a gentle kick, a low snare and rim, dark hats.
                "Lo-Fi · Polvoriento",
                kit(
                    DrumModel::Tr808,
                    [-1.0, -2.0, -3.0, -2.0, -4.0, -4.0, -2.0, -2.0, -2.0, -2.0, -3.0, -3.0, -3.0, -3.0, -2.0, -2.0],
                    [0.8, 1.0, 0.8, 0.9, 0.8, 0.7, 0.8, 1.0, 0.8, 0.8, 0.7, 0.8, 0.9, 0.8, 1.0, 1.0],
                    [0.9, 0.65, 0.75, 0.35, 0.85, 0.5, 0.35, 0.3, 0.35, 0.35, 0.3, 0.35, 0.55, 0.35, 0.45, 0.35],
                    [0.02, 0.0, 0.0, 0.0, 0.06, 0.04, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Dembow: a booming 808 kick, a high, tight snare and rim, hats, tambourine, conga and clave.
                "Reggaetón · Dembow",
                kit(
                    DrumModel::Tr808,
                    [0.0, 2.0, 3.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    [1.3, 1.0, 0.7, 0.9, 0.7, 0.6, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                    [0.9, 0.6, 0.8, 0.6, 0.85, 0.4, 0.4, 0.3, 0.4, 0.4, 0.35, 0.35, 0.5, 0.55, 0.5, 0.45],
                    [0.0, 0.0, 0.0, 0.0, 0.06, 0.04, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Fast breaks: a short, high kick, a cracking high snare, tight hats and a ride.
                "Drum & Bass · Rodante",
                kit(
                    DrumModel::Tr909,
                    [2.0, 2.0, 4.0, 2.0, 2.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0],
                    [0.6, 1.0, 0.8, 0.9, 0.6, 0.6, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.8, 1.0, 1.0, 1.0],
                    [0.6, 0.5, 0.75, 0.4, 0.6, 0.4, 0.35, 0.3, 0.35, 0.35, 0.45, 0.9, 0.5, 0.35, 0.3, 0.3],
                    [0.0, 0.0, 0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Half-time weight: a driven kick, a huge low snare and clap, hats kept back.
                "Dubstep · Pesado",
                kit(
                    DrumModel::Tr909,
                    [-1.0, 0.0, -2.0, -1.0, 0.0, 0.0, -3.0, 0.0, -3.0, -3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    [1.0, 1.0, 1.5, 1.4, 0.8, 0.7, 1.2, 1.0, 1.2, 1.2, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                    [0.6, 0.45, 0.75, 0.7, 0.55, 0.4, 0.45, 0.3, 0.45, 0.45, 0.45, 0.4, 0.35, 0.3, 0.3, 0.3],
                    [0.08, 0.0, 0.06, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Skippy: a short, bouncy kick, a crisp rim and snare, quick hats, shaker and tambourine.
                "UK Garage · Shuffle",
                kit(
                    DrumModel::Tr909,
                    [1.0, 2.0, 2.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0],
                    [0.75, 1.0, 0.8, 0.9, 0.7, 0.6, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.8, 0.8, 1.0, 1.0],
                    [0.62, 0.65, 0.7, 0.6, 0.6, 0.45, 0.35, 0.3, 0.35, 0.35, 0.35, 0.4, 0.6, 0.5, 0.35, 0.35],
                    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Electro: a punchy 808 kick, snappy snare and clap, cowbell, clave and high toms up front.
                "Electro · Robótico",
                kit(
                    DrumModel::Tr808,
                    [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 2.0, 0.0, 2.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    [0.7, 1.0, 0.6, 0.8, 0.6, 0.7, 0.9, 1.0, 0.9, 0.9, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                    [0.9, 0.6, 0.85, 0.75, 0.9, 0.5, 0.6, 0.75, 0.6, 0.6, 0.35, 0.35, 0.4, 0.35, 0.5, 0.6],
                    [0.02, 0.0, 0.0, 0.0, 0.08, 0.06, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Eighties: a big, long snare and clap, a solid kick, deep long toms and a crash.
                "Synthwave · Ochentera",
                kit(
                    DrumModel::Tr909,
                    [-1.0, 0.0, -1.0, -1.0, 0.0, 0.0, -2.0, 0.0, -2.0, -2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    [0.8, 1.0, 1.6, 1.3, 0.8, 0.8, 1.6, 1.0, 1.6, 1.6, 1.2, 1.0, 1.0, 1.0, 1.0, 1.0],
                    [0.6, 0.4, 0.8, 0.65, 0.55, 0.45, 0.6, 0.3, 0.6, 0.6, 0.5, 0.4, 0.35, 0.4, 0.3, 0.3],
                    [0.0, 0.0, 0.03, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Percussive: a round kick, toms, congas, shaker, clave and cowbell carry the groove.
                "Afro House · Tribal",
                kit(
                    DrumModel::Tr808,
                    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -2.0, 0.0, 0.0, 0.0, 0.0],
                    [1.0, 1.0, 1.0, 1.0, 0.8, 0.8, 1.2, 1.0, 1.2, 1.2, 1.0, 1.0, 1.0, 1.0, 1.3, 1.0],
                    [0.88, 0.55, 0.4, 0.6, 0.8, 0.5, 0.7, 0.45, 0.7, 0.65, 0.3, 0.4, 0.7, 0.55, 0.8, 0.6],
                    [0.0, 0.0, 0.0, 0.0, 0.06, 0.04, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
            (
                // Latin percussion: congas, clave, cowbell and high, short toms like timbales over a light kick.
                "Latino · Percusión",
                kit(
                    DrumModel::Tr808,
                    [0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 4.0, 2.0, 4.0, 4.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0],
                    [0.8, 1.0, 0.8, 1.0, 0.7, 0.7, 0.6, 1.0, 0.6, 0.6, 1.0, 1.0, 1.0, 1.0, 1.2, 1.0],
                    [0.7, 0.65, 0.45, 0.35, 0.7, 0.4, 0.65, 0.7, 0.65, 0.65, 0.3, 0.35, 0.7, 0.6, 0.85, 0.7],
                    [0.0, 0.0, 0.0, 0.0, 0.05, 0.03, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ),
        ]
    }
}

fn clean(drive: &[f32; DRUM_SOUNDS]) -> bool {
    drive.iter().all(|&d| d == 0.0)
}

/// A drum machine's values for its sounds; projects from when it had eight get
/// `default` for the new ones.
fn drum_values<'de, D: serde::Deserializer<'de>>(d: D, default: f32) -> Result<[f32; DRUM_SOUNDS], D::Error> {
    let saved = Vec::<f32>::deserialize(d)?;
    let mut values = [default; DRUM_SOUNDS];
    for (value, saved) in values.iter_mut().zip(saved) {
        *value = saved;
    }
    Ok(values)
}

fn drum_tunes<'de, D: serde::Deserializer<'de>>(d: D) -> Result<[f32; DRUM_SOUNDS], D::Error> {
    drum_values(d, 0.0)
}

fn drum_decays<'de, D: serde::Deserializer<'de>>(d: D) -> Result<[f32; DRUM_SOUNDS], D::Error> {
    drum_values(d, 1.0)
}

/// The sounds a drum machine of eight didn't have are silent in its projects:
/// the notes it ignored stay unheard, so they sound as they did.
fn drum_levels<'de, D: serde::Deserializer<'de>>(d: D) -> Result<[f32; DRUM_SOUNDS], D::Error> {
    drum_values(d, 0.0)
}

/// The sound of the drum machine: its own first sounds, or modelled on the
/// Roland TR-808 or TR-909 (see the engine's `drumkit`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrumModel {
    #[default]
    Classic,
    Tr808,
    Tr909,
}

impl DrumModel {
    pub const ALL: [DrumModel; 3] = [DrumModel::Tr808, DrumModel::Tr909, DrumModel::Classic];

    pub fn label(self) -> &'static str {
        match self {
            DrumModel::Classic => "Clásica",
            DrumModel::Tr808 => "808",
            DrumModel::Tr909 => "909",
        }
    }
}

fn is_classic(model: &DrumModel) -> bool {
    *model == DrumModel::Classic
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

impl SuperSawParams {
    /// Factory presets: (name as «Categoría · Nombre», settings).
    pub fn presets() -> Vec<(&'static str, SuperSawParams)> {
        let base = SuperSawParams::default();
        // The filter envelope follows the amplitude envelope: with sustain 0 a note
        // closes its filter as it fades (plucks), a slow attack opens it (swells).
        let saws = |voices, detune, spread, gain| SuperSawParams { voices, detune, spread, gain, ..base };
        vec![
            ("Init", base),
            ("Lead · Trance clásico", SuperSawParams { cutoff: 5000.0, resonance: 0.1, env_amount: 0.0, attack: 0.005, decay: 0.5, sustain: 0.85, release: 0.3, ..saws(7.0, 0.4, 0.8, 0.45) }),
            ("Lead · Estrecho", SuperSawParams { cutoff: 3500.0, resonance: 0.25, env_amount: 1.0, attack: 0.003, decay: 0.4, sustain: 0.7, release: 0.2, ..saws(5.0, 0.2, 0.3, 0.42) }),
            ("Lead · Rave desafinado", SuperSawParams { cutoff: 8000.0, resonance: 0.05, env_amount: 0.0, attack: 0.002, decay: 0.3, sustain: 0.9, release: 0.25, ..saws(7.0, 0.9, 1.0, 0.4) }),
            ("Pad · Ancho", SuperSawParams { cutoff: 2500.0, resonance: 0.05, env_amount: 0.0, attack: 1.2, decay: 1.0, sustain: 0.9, release: 2.0, ..saws(7.0, 0.3, 1.0, 0.45) }),
            ("Pad · Amanecer", SuperSawParams { cutoff: 350.0, resonance: 0.15, env_amount: 3.5, attack: 1.2, decay: 1.0, sustain: 1.0, release: 2.5, ..saws(7.0, 0.35, 0.9, 0.43) }),
            ("Pad · Oscuro", SuperSawParams { cutoff: 600.0, resonance: 0.3, env_amount: 0.0, attack: 0.8, decay: 1.0, sustain: 1.0, release: 2.0, ..saws(6.0, 0.15, 0.7, 0.4) }),
            ("Acordes · Progressive", SuperSawParams { cutoff: 1500.0, resonance: 0.1, env_amount: 1.5, attack: 0.002, decay: 0.35, sustain: 0.4, release: 0.3, ..saws(7.0, 0.3, 0.8, 0.45) }),
            ("Acordes · Stab de house", SuperSawParams { cutoff: 1200.0, resonance: 0.3, env_amount: 2.0, attack: 0.001, decay: 0.25, sustain: 0.2, release: 0.15, ..saws(5.0, 0.25, 0.6, 0.44) }),
            ("Acordes · Rave de los 90", SuperSawParams { cutoff: 2500.0, resonance: 0.2, env_amount: 1.0, attack: 0.001, decay: 0.3, sustain: 0.6, release: 0.2, ..saws(7.0, 0.6, 0.8, 0.38) }),
            ("Pluck · Trance", SuperSawParams { cutoff: 600.0, resonance: 0.25, env_amount: 3.0, attack: 0.001, decay: 0.25, sustain: 0.0, release: 0.25, ..saws(7.0, 0.35, 0.8, 0.48) }),
            ("Pluck · Corto y brillante", SuperSawParams { cutoff: 2000.0, resonance: 0.15, env_amount: 2.5, attack: 0.001, decay: 0.16, sustain: 0.0, release: 0.1, ..saws(3.0, 0.2, 0.6, 0.5) }),
            ("Pluck · Largo", SuperSawParams { cutoff: 900.0, resonance: 0.15, env_amount: 2.5, attack: 0.001, decay: 0.8, sustain: 0.0, release: 0.5, ..saws(5.0, 0.3, 0.9, 0.33) }),
            ("Bajo · Sierra única", SuperSawParams { cutoff: 700.0, resonance: 0.3, env_amount: 1.5, attack: 0.001, decay: 0.3, sustain: 0.5, release: 0.08, ..saws(1.0, 0.0, 0.0, 0.52) }),
            ("Bajo · Progressive", SuperSawParams { cutoff: 400.0, resonance: 0.2, env_amount: 2.0, attack: 0.001, decay: 0.2, sustain: 0.5, release: 0.1, ..saws(3.0, 0.12, 0.2, 0.57) }),
            ("Bajo · Reese", SuperSawParams { cutoff: 500.0, resonance: 0.1, env_amount: 0.5, attack: 0.003, decay: 0.5, sustain: 1.0, release: 0.15, ..saws(2.0, 0.25, 0.0, 0.4) }),
            ("Bajo · Rebotado", SuperSawParams { cutoff: 250.0, resonance: 0.35, env_amount: 3.0, attack: 0.001, decay: 0.22, sustain: 0.0, release: 0.08, ..saws(2.0, 0.06, 0.0, 0.75) }),
            ("Textura · Enjambre", SuperSawParams { cutoff: 1800.0, resonance: 0.5, env_amount: 0.0, attack: 1.2, decay: 1.0, sustain: 1.0, release: 3.0, ..saws(7.0, 1.0, 1.0, 0.36) }),
            ("FX · Barrido", SuperSawParams { cutoff: 150.0, resonance: 0.6, env_amount: 4.0, attack: 1.5, decay: 1.0, sustain: 1.0, release: 1.5, ..saws(7.0, 0.5, 1.0, 0.41) }),
        ]
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

impl AcidParams {
    /// Factory presets: (name as «Categoría · Nombre», settings).
    pub fn presets() -> Vec<(&'static str, AcidParams)> {
        let base = AcidParams::default();
        vec![
            ("Init", base),
            ("Acid · Clásico", AcidParams { cutoff: 320.0, resonance: 0.8, env_mod: 0.6, decay: 0.35, accent: 0.7, glide_ms: 60.0, drive: 0.25, gain: 0.5, ..base }),
            ("Acid · Squelch", AcidParams { cutoff: 180.0, resonance: 0.92, env_mod: 0.85, decay: 0.22, accent: 0.9, glide_ms: 50.0, drive: 0.35, gain: 0.445, ..base }),
            ("Acid · Cuadrada hueca", AcidParams { square: true, cutoff: 260.0, resonance: 0.78, env_mod: 0.55, decay: 0.3, accent: 0.6, glide_ms: 60.0, drive: 0.2, gain: 0.4 }),
            ("Acid · Gritón", AcidParams { cutoff: 400.0, resonance: 0.9, env_mod: 0.7, decay: 0.4, accent: 1.0, glide_ms: 60.0, drive: 1.0, gain: 0.25, ..base }),
            ("Acid · Glide largo", AcidParams { cutoff: 300.0, resonance: 0.85, env_mod: 0.5, decay: 0.6, accent: 0.6, glide_ms: 250.0, drive: 0.3, gain: 0.5, ..base }),
            ("Acid · Burbuja", AcidParams { square: true, cutoff: 500.0, resonance: 0.95, env_mod: 0.4, decay: 0.12, accent: 0.5, glide_ms: 30.0, drive: 0.15, gain: 0.4 }),
            ("Acid · Barrido lento", AcidParams { cutoff: 120.0, resonance: 0.88, env_mod: 0.8, decay: 1.5, accent: 0.5, glide_ms: 80.0, drive: 0.3, gain: 0.5, ..base }),
            ("Acid · Solo acentos", AcidParams { cutoff: 250.0, resonance: 0.85, env_mod: 0.25, decay: 0.3, accent: 1.0, glide_ms: 60.0, drive: 0.4, gain: 0.42, ..base }),
            ("Bajo · Sub oscuro", AcidParams { cutoff: 80.0, resonance: 0.6, env_mod: 0.25, decay: 0.2, accent: 0.4, glide_ms: 40.0, drive: 0.2, gain: 0.8, ..base }),
            ("Bajo · Techno rodante", AcidParams { cutoff: 220.0, resonance: 0.55, env_mod: 0.45, decay: 0.15, accent: 0.5, glide_ms: 20.0, drive: 0.5, gain: 0.5, ..base }),
            ("Bajo · Minimal seco", AcidParams { square: true, cutoff: 350.0, resonance: 0.4, env_mod: 0.35, decay: 0.08, accent: 0.4, glide_ms: 5.0, drive: 0.1, gain: 0.5 }),
            ("Bajo · Gordo saturado", AcidParams { cutoff: 600.0, resonance: 0.35, env_mod: 0.3, decay: 0.3, accent: 0.5, glide_ms: 30.0, drive: 0.8, gain: 0.32, ..base }),
            ("Bajo · Pulso hueco", AcidParams { square: true, cutoff: 700.0, resonance: 0.15, env_mod: 0.1, decay: 0.3, accent: 0.3, glide_ms: 30.0, drive: 0.1, gain: 0.42 }),
            ("Lead · Chillón", AcidParams { cutoff: 1200.0, resonance: 0.88, env_mod: 0.6, decay: 0.5, accent: 0.8, glide_ms: 80.0, drive: 0.7, gain: 0.32, ..base }),
            ("Lead · Silbido", AcidParams { square: true, cutoff: 900.0, resonance: 0.96, env_mod: 0.2, decay: 0.8, accent: 0.4, glide_ms: 120.0, drive: 0.1, gain: 0.5 }),
            ("FX · Láser", AcidParams { cutoff: 60.0, resonance: 0.95, env_mod: 1.0, decay: 0.08, accent: 1.0, glide_ms: 400.0, drive: 0.5, gain: 0.4, ..base }),
        ]
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

impl FmParams {
    /// Factory presets: (name as «Categoría · Nombre», settings).
    pub fn presets() -> Vec<(&'static str, FmParams)> {
        let base = FmParams::default();
        vec![
            ("Init", base),
            ("Teclas · Piano eléctrico", FmParams { ratio: 1.0, index: 1.8, mod_decay: 0.8, mod_sustain: 0.1, feedback: 0.0, attack: 0.002, decay: 3.0, sustain: 0.0, release: 0.4, gain: 0.3 }),
            ("Teclas · Púa brillante", FmParams { ratio: 14.0, index: 1.2, mod_decay: 0.08, mod_sustain: 0.0, feedback: 0.0, attack: 0.002, decay: 2.5, sustain: 0.0, release: 0.4, gain: 0.24 }),
            ("Teclas · Funky percusivo", FmParams { ratio: 1.0, index: 3.0, mod_decay: 0.15, mod_sustain: 0.25, feedback: 0.4, attack: 0.001, decay: 0.8, sustain: 0.2, release: 0.12, gain: 0.38 }),
            ("Teclas · Órgano FM", FmParams { ratio: 2.0, index: 1.0, mod_decay: 4.0, mod_sustain: 1.0, feedback: 0.0, attack: 0.005, decay: 0.1, sustain: 1.0, release: 0.08, gain: 0.18 }),
            ("Teclas · Órgano saturado", FmParams { ratio: 1.0, index: 1.5, mod_decay: 1.0, mod_sustain: 0.8, feedback: 0.35, attack: 0.005, decay: 0.1, sustain: 1.0, release: 0.1, gain: 0.27 }),
            ("Campanas · Tubular", FmParams { ratio: 3.5, index: 4.0, mod_decay: 2.5, mod_sustain: 0.2, feedback: 0.0, attack: 0.001, decay: 6.0, sustain: 0.0, release: 2.0, gain: 0.17 }),
            ("Campanas · Iglesia", FmParams { ratio: 1.4, index: 5.0, mod_decay: 3.0, mod_sustain: 0.15, feedback: 0.0, attack: 0.001, decay: 7.0, sustain: 0.0, release: 3.0, gain: 0.15 }),
            ("Campanas · Cristal", FmParams { ratio: 7.07, index: 2.0, mod_decay: 0.6, mod_sustain: 0.05, feedback: 0.0, attack: 0.001, decay: 2.5, sustain: 0.0, release: 1.5, gain: 0.19 }),
            ("Campanas · Caja de música", FmParams { ratio: 5.0, index: 1.5, mod_decay: 0.3, mod_sustain: 0.0, feedback: 0.0, attack: 0.001, decay: 1.5, sustain: 0.0, release: 0.8, gain: 0.24 }),
            ("Pluck · Marimba", FmParams { ratio: 4.0, index: 3.0, mod_decay: 0.03, mod_sustain: 0.0, feedback: 0.0, attack: 0.001, decay: 0.6, sustain: 0.0, release: 0.3, gain: 0.3 }),
            ("Pluck · Vibráfono", FmParams { ratio: 4.0, index: 1.5, mod_decay: 0.25, mod_sustain: 0.05, feedback: 0.0, attack: 0.001, decay: 2.5, sustain: 0.0, release: 0.8, gain: 0.17 }),
            ("Pluck · Metálico", FmParams { ratio: 3.17, index: 6.0, mod_decay: 0.12, mod_sustain: 0.0, feedback: 0.2, attack: 0.001, decay: 0.4, sustain: 0.0, release: 0.2, gain: 0.33 }),
            ("Pluck · Arpegio digital", FmParams { ratio: 2.0, index: 4.0, mod_decay: 0.1, mod_sustain: 0.0, feedback: 0.0, attack: 0.001, decay: 0.35, sustain: 0.0, release: 0.15, gain: 0.31 }),
            ("Bajo · FM de los 80", FmParams { ratio: 1.0, index: 4.0, mod_decay: 0.18, mod_sustain: 0.2, feedback: 0.3, attack: 0.001, decay: 0.6, sustain: 0.5, release: 0.08, gain: 0.31 }),
            ("Bajo · Slap", FmParams { ratio: 2.0, index: 5.0, mod_decay: 0.04, mod_sustain: 0.1, feedback: 0.2, attack: 0.001, decay: 0.5, sustain: 0.3, release: 0.08, gain: 0.31 }),
            ("Bajo · Sub FM", FmParams { ratio: 1.0, index: 0.6, mod_decay: 0.3, mod_sustain: 0.1, feedback: 0.0, attack: 0.002, decay: 0.5, sustain: 0.8, release: 0.1, gain: 0.31 }),
            ("Bajo · Gruñido", FmParams { ratio: 1.0, index: 6.0, mod_decay: 0.5, mod_sustain: 0.6, feedback: 0.7, attack: 0.002, decay: 0.3, sustain: 0.8, release: 0.1, gain: 0.28 }),
            ("Lead · Feedback", FmParams { ratio: 1.0, index: 2.5, mod_decay: 1.0, mod_sustain: 0.8, feedback: 0.85, attack: 0.005, decay: 0.3, sustain: 0.8, release: 0.2, gain: 0.27 }),
            ("Lead · Flauta FM", FmParams { ratio: 1.0, index: 0.8, mod_decay: 0.15, mod_sustain: 0.3, feedback: 0.0, attack: 0.04, decay: 0.3, sustain: 0.9, release: 0.2, gain: 0.32 }),
            ("Metales · Stab FM", FmParams { ratio: 1.0, index: 3.5, mod_decay: 0.25, mod_sustain: 0.45, feedback: 0.4, attack: 0.015, decay: 0.4, sustain: 0.6, release: 0.15, gain: 0.37 }),
            ("Pad · Cristal lento", FmParams { ratio: 3.0, index: 1.2, mod_decay: 4.0, mod_sustain: 0.6, feedback: 0.0, attack: 1.2, decay: 2.0, sustain: 0.8, release: 2.5, gain: 0.23 }),
            ("Textura · Inarmónica", FmParams { ratio: 2.71, index: 5.0, mod_decay: 3.0, mod_sustain: 0.5, feedback: 0.0, attack: 0.6, decay: 2.0, sustain: 0.7, release: 2.0, gain: 0.183 }),
            ("Textura · Metal oxidado", FmParams { ratio: 0.73, index: 7.0, mod_decay: 2.0, mod_sustain: 0.6, feedback: 0.5, attack: 0.3, decay: 3.0, sustain: 0.6, release: 2.0, gain: 0.163 }),
            ("FX · Gong", FmParams { ratio: 0.586, index: 8.0, mod_decay: 4.0, mod_sustain: 0.1, feedback: 0.0, attack: 0.001, decay: 8.0, sustain: 0.0, release: 4.0, gain: 0.125 }),
            ("FX · Ruido digital", FmParams { ratio: 13.7, index: 10.0, mod_decay: 0.3, mod_sustain: 0.2, feedback: 1.0, attack: 0.001, decay: 0.3, sustain: 0.0, release: 0.2, gain: 0.33 }),
        ]
    }
}

/// What Pulse Keys plays: which instrument its synthesis models.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeysModel {
    /// Hammered strings: inharmonic partials, two strings a note, a soundboard.
    #[default]
    Piano,
    /// Electric piano with struck tines and a magnetic pickup (Rhodes-like).
    Tines,
    /// Electric piano with struck steel reeds and an electrostatic pickup (Wurlitzer-like).
    Reed,
    /// Tonewheel organ with drawbars, percussion, key click and a rotary speaker.
    Organ,
}

impl KeysModel {
    pub const ALL: [KeysModel; 4] = [Self::Piano, Self::Tines, Self::Reed, Self::Organ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Piano => "Piano",
            Self::Tines => "Púas",
            Self::Reed => "Lengüetas",
            Self::Organ => "Órgano",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Piano => "Piano acústico: cuerdas golpeadas por martillos, con su tabla armónica",
            Self::Tines => "Piano eléctrico de púas, como un Rhodes: redondo, con campana al atacar",
            Self::Reed => "Piano eléctrico de lengüetas, como un Wurlitzer: más nasal y con mordida",
            Self::Organ => "Órgano de ruedas fónicas, como un Hammond: registros, percusión y altavoz giratorio",
        }
    }

    /// True when parameter `index` of [`KEYS_PARAMS`] changes this model's sound.
    pub fn uses(self, index: usize) -> bool {
        match index {
            KEYS_TONE | KEYS_DRIVE | KEYS_WIDTH | KEYS_GAIN => true,
            // The organ sustains, stops at once and is not touch-sensitive.
            KEYS_LENGTH | KEYS_RELEASE | KEYS_DYNAMICS | KEYS_HAMMER => self != Self::Organ,
            KEYS_STRINGS | KEYS_BODY => self == Self::Piano,
            KEYS_BELL | KEYS_BARK | KEYS_TREMOLO_RATE | KEYS_TREMOLO => matches!(self, Self::Tines | Self::Reed),
            _ => self == Self::Organ && index < KEYS_PARAMS.len(),
        }
    }
}

/// Indices of Pulse Keys' parameters (see [`KEYS_PARAMS`]).
pub const KEYS_TONE: usize = 0;
pub const KEYS_LENGTH: usize = 1;
pub const KEYS_RELEASE: usize = 2;
pub const KEYS_DYNAMICS: usize = 3;
pub const KEYS_HAMMER: usize = 4;
pub const KEYS_STRINGS: usize = 5;
pub const KEYS_BODY: usize = 6;
pub const KEYS_BELL: usize = 7;
pub const KEYS_BARK: usize = 8;
pub const KEYS_TREMOLO_RATE: usize = 9;
pub const KEYS_TREMOLO: usize = 10;
/// The first of the nine drawbars, 16' (then 5 1/3', 8', 4', 2 2/3', 2', 1 3/5', 1 1/3', 1').
pub const KEYS_DRAWBAR: usize = 11;
pub const KEYS_PERCUSSION: usize = 20;
pub const KEYS_PERC_DECAY: usize = 21;
pub const KEYS_CLICK: usize = 22;
pub const KEYS_LESLIE: usize = 23;
pub const KEYS_LESLIE_FAST: usize = 24;
pub const KEYS_DRIVE: usize = 25;
pub const KEYS_WIDTH: usize = 26;
pub const KEYS_GAIN: usize = 27;

/// Pulse Keys: synthesized piano, electric pianos and organ. Every model keeps all
/// the settings; each uses the ones [`KeysModel::uses`] says.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeysParams {
    pub model: KeysModel,
    /// Brightness, 0..1: how hard the hammers are, how open the pickup sounds.
    pub tone: f32,
    /// How long notes ring, as a multiple of the model's own.
    pub length: f32,
    /// Seconds a released note takes to fall 60 dB (the dampers).
    pub release: f32,
    /// How much velocity changes loudness (and brightness), 0..1.
    pub dynamics: f32,
    /// Hammer knock at the start of a note, 0..1.
    pub hammer: f32,
    /// How far apart a piano note's strings are tuned, 0..1 (up to 3 cents).
    pub strings: f32,
    /// Soundboard and cabinet resonance, 0..1.
    pub body: f32,
    /// Bell-like attack of the tines or reeds, 0..1.
    pub bell: f32,
    /// How much the pickup growls on hard notes, 0..1.
    pub bark: f32,
    pub tremolo_rate: f32,
    /// Tremolo depth, 0..1 (the tines pan side to side, the reeds pulse).
    pub tremolo: f32,
    /// Organ drawbars, 0 (in) .. 8 (out): 16', 5 1/3', 8', 4', 2 2/3', 2', 1 3/5', 1 1/3', 1'.
    pub drawbars: [f32; 9],
    /// Level of the organ's percussion, 0 (off) .. 1.
    pub percussion: f32,
    /// Seconds the percussion takes to fall 60 dB.
    pub perc_decay: f32,
    /// The percussion sounds the third harmonic (else the second).
    pub perc_third: bool,
    pub click: f32,
    /// Rotary speaker mix, 0 (dry) .. 1.
    pub leslie: f32,
    /// Rotary speaker speed, 0 (slow) .. 1 (fast); the rotors take time to get there.
    pub leslie_fast: f32,
    pub drive: f32,
    pub width: f32,
    pub gain: f32,
}

impl Default for KeysParams {
    fn default() -> Self {
        Self {
            model: KeysModel::Piano,
            tone: 0.5,
            length: 1.0,
            release: 0.35,
            dynamics: 0.8,
            hammer: 0.4,
            strings: 0.4,
            body: 0.5,
            bell: 0.4,
            bark: 0.4,
            tremolo_rate: 4.5,
            tremolo: 0.0,
            drawbars: [8.0, 8.0, 8.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            percussion: 0.0,
            perc_decay: 0.5,
            perc_third: false,
            click: 0.3,
            leslie: 0.6,
            leslie_fast: 0.0,
            drive: 0.0,
            width: 0.5,
            gain: 0.5,
        }
    }
}

impl KeysParams {
    /// The starting sound of `model` (what choosing it in the device gives).
    pub fn model_default(model: KeysModel) -> Self {
        let base = KeysParams::default();
        match model {
            KeysModel::Piano => base,
            KeysModel::Tines => KeysParams { model, tone: 0.45, release: 0.25, hammer: 0.2, bell: 0.45, bark: 0.35, ..base },
            KeysModel::Reed => KeysParams { model, tone: 0.5, release: 0.2, hammer: 0.25, bell: 0.35, bark: 0.5, tremolo_rate: 5.5, ..base },
            KeysModel::Organ => KeysParams { model, release: 0.05, dynamics: 0.0, percussion: 0.6, perc_decay: 0.4, perc_third: true, leslie: 0.6, ..base },
        }
    }

    /// Factory presets: (name, settings), grouped by model.
    pub fn presets() -> Vec<(&'static str, KeysParams)> {
        let piano = Self::model_default(KeysModel::Piano);
        let tines = Self::model_default(KeysModel::Tines);
        let reed = Self::model_default(KeysModel::Reed);
        let organ = Self::model_default(KeysModel::Organ);
        let bars = |s: &str| {
            let mut d = [0.0; 9];
            for (slot, c) in d.iter_mut().zip(s.chars()) {
                *slot = c.to_digit(10).unwrap_or(0) as f32;
            }
            d
        };
        vec![
            ("Piano · De cola", piano),
            ("Piano · Brillante", KeysParams { tone: 0.85, hammer: 0.5, body: 0.35, width: 0.6, ..piano }),
            ("Piano · Suave", KeysParams { tone: 0.22, dynamics: 0.9, hammer: 0.2, body: 0.65, release: 0.5, ..piano }),
            ("Piano · De pared", KeysParams { tone: 0.6, strings: 0.95, body: 0.75, length: 0.6, width: 0.3, ..piano }),
            ("Púas · Clásico", tines),
            ("Púas · Trémolo", KeysParams { tremolo: 0.6, tremolo_rate: 4.2, bell: 0.35, width: 0.7, gain: 0.4, ..tines }),
            ("Púas · Con ladrido", KeysParams { tone: 0.65, bark: 0.85, bell: 0.5, drive: 0.25, ..tines }),
            ("Púas · Campana", KeysParams { tone: 0.6, bell: 0.85, bark: 0.2, length: 1.4, ..tines }),
            ("Lengüetas · Clásico", reed),
            ("Lengüetas · Trémolo", KeysParams { tremolo: 0.5, ..reed }),
            ("Lengüetas · Saturado", KeysParams { bark: 0.8, drive: 0.45, tone: 0.65, gain: 0.36, ..reed }),
            ("Órgano · Jazz", KeysParams { drawbars: bars("888000000"), percussion: 0.7, perc_decay: 0.35, leslie_fast: 0.0, ..organ }),
            ("Órgano · Gospel", KeysParams { drawbars: bars("888800000"), percussion: 0.0, leslie_fast: 1.0, click: 0.45, ..organ }),
            ("Órgano · Rock", KeysParams { drawbars: bars("888000000"), percussion: 0.0, drive: 0.6, leslie: 0.75, click: 0.5, ..organ }),
            ("Órgano · Balada", KeysParams { drawbars: bars("006802000"), percussion: 0.0, click: 0.1, leslie: 0.5, ..organ }),
            ("Órgano · Completo", KeysParams { drawbars: bars("888888888"), percussion: 0.0, leslie_fast: 1.0, gain: 0.4, ..organ }),
            // Pianos for songs: pop cuts through, the ballad sings on, the dark one
            // under film cues, the worn-out one for lo-fi beats, the touchy jazz one.
            ("Piano · Pop", KeysParams { tone: 0.72, hammer: 0.5, dynamics: 0.6, body: 0.4, length: 0.8, release: 0.3, width: 0.7, gain: 0.5, ..piano }),
            ("Piano · Balada", KeysParams { tone: 0.42, hammer: 0.25, dynamics: 0.85, strings: 0.5, body: 0.7, length: 1.6, release: 0.9, width: 0.8, gain: 0.45, ..piano }),
            ("Piano · Cine oscuro", KeysParams { tone: 0.08, hammer: 0.1, dynamics: 1.0, strings: 0.6, body: 0.85, length: 2.5, release: 1.6, width: 0.9, gain: 0.42, ..piano }),
            ("Piano · Lo-fi", KeysParams { tone: 0.3, hammer: 0.6, dynamics: 0.4, strings: 1.0, body: 0.9, length: 0.45, release: 0.18, drive: 0.35, width: 0.25, gain: 0.47, ..piano }),
            ("Piano · Jazz", KeysParams { tone: 0.55, hammer: 0.35, dynamics: 0.95, strings: 0.3, body: 0.55, length: 1.1, release: 0.25, width: 0.45, gain: 0.5, ..piano }),
            // Tines: warm soul, dark neo-soul with a lazy tremolo, the glassy seventies
            // ballad, a short barking funk comp and a slow wide chill tremolo.
            ("Púas · Soul", KeysParams { tone: 0.4, hammer: 0.15, dynamics: 0.85, bell: 0.3, bark: 0.45, length: 1.2, release: 0.35, gain: 0.53, ..tines }),
            ("Púas · Neo-soul", KeysParams { tone: 0.26, hammer: 0.1, dynamics: 0.9, bell: 0.22, bark: 0.2, tremolo: 0.35, tremolo_rate: 3.0, length: 1.3, release: 0.45, width: 0.85, gain: 0.46, ..tines }),
            ("Púas · Balada de los 70", KeysParams { tone: 0.55, dynamics: 0.8, bell: 0.65, bark: 0.25, tremolo: 0.25, tremolo_rate: 6.0, length: 1.5, release: 0.5, width: 0.8, gain: 0.46, ..tines }),
            ("Púas · Funk", KeysParams { tone: 0.7, hammer: 0.4, dynamics: 1.0, bell: 0.4, bark: 0.8, drive: 0.3, length: 0.6, release: 0.12, width: 0.4, gain: 0.5, ..tines }),
            ("Púas · Chill", KeysParams { tone: 0.32, hammer: 0.1, dynamics: 0.75, bell: 0.3, bark: 0.15, tremolo: 0.6, tremolo_rate: 1.2, length: 1.6, release: 0.6, width: 0.75, gain: 0.34, ..tines }),
            // Reeds: a soul one that growls when hit hard, a crunchy rock one.
            ("Lengüetas · Soul", KeysParams { tone: 0.35, dynamics: 0.95, bell: 0.45, bark: 0.6, tremolo: 0.2, tremolo_rate: 6.0, length: 1.1, gain: 0.55, ..reed }),
            ("Lengüetas · Rock", KeysParams { tone: 0.85, hammer: 0.4, dynamics: 0.9, bell: 0.5, bark: 1.0, drive: 0.75, length: 0.7, release: 0.15, gain: 0.36, ..reed }),
            // Organs by style and registration.
            ("Órgano · Blues", KeysParams { drawbars: bars("838000000"), percussion: 0.8, perc_decay: 0.2, perc_third: true, click: 0.5, leslie: 0.7, leslie_fast: 0.0, drive: 0.25, gain: 0.55, ..organ }),
            ("Órgano · Reggae", KeysParams { drawbars: bars("008400000"), percussion: 0.8, perc_decay: 0.15, perc_third: true, click: 0.6, leslie: 0.3, leslie_fast: 0.0, tone: 0.55, gain: 0.89, ..organ }),
            ("Órgano · Iglesia", KeysParams { drawbars: bars("886000000"), percussion: 0.0, click: 0.05, leslie: 0.55, leslie_fast: 0.0, tone: 0.35, width: 0.7, gain: 0.6, ..organ }),
            ("Órgano · Silbante", KeysParams { drawbars: bars("800000888"), percussion: 0.0, click: 0.35, leslie: 0.8, leslie_fast: 1.0, tone: 0.6, gain: 0.37, ..organ }),
            ("Órgano · Rugido", KeysParams { drawbars: bars("888500000"), percussion: 0.0, click: 0.6, leslie: 1.0, leslie_fast: 1.0, drive: 0.85, tone: 0.8, width: 0.8, gain: 0.38, ..organ }),
            ("Órgano · House", KeysParams { drawbars: bars("006060300"), percussion: 1.0, perc_decay: 0.25, perc_third: false, click: 0.7, leslie: 0.0, tone: 0.7, drive: 0.15, gain: 0.95, ..organ }),
        ]
    }

    fn param_mut(&mut self, index: usize) -> Option<&mut f32> {
        Some(match index {
            KEYS_TONE => &mut self.tone,
            KEYS_LENGTH => &mut self.length,
            KEYS_RELEASE => &mut self.release,
            KEYS_DYNAMICS => &mut self.dynamics,
            KEYS_HAMMER => &mut self.hammer,
            KEYS_STRINGS => &mut self.strings,
            KEYS_BODY => &mut self.body,
            KEYS_BELL => &mut self.bell,
            KEYS_BARK => &mut self.bark,
            KEYS_TREMOLO_RATE => &mut self.tremolo_rate,
            KEYS_TREMOLO => &mut self.tremolo,
            i if (KEYS_DRAWBAR..KEYS_DRAWBAR + 9).contains(&i) => &mut self.drawbars[i - KEYS_DRAWBAR],
            KEYS_PERCUSSION => &mut self.percussion,
            KEYS_PERC_DECAY => &mut self.perc_decay,
            KEYS_CLICK => &mut self.click,
            KEYS_LESLIE => &mut self.leslie,
            KEYS_LESLIE_FAST => &mut self.leslie_fast,
            KEYS_DRIVE => &mut self.drive,
            KEYS_WIDTH => &mut self.width,
            KEYS_GAIN => &mut self.gain,
            _ => return None,
        })
    }
}

pub const KEYS_PARAMS: [ParamInfo; 28] = [
    ParamInfo::linear("Tono", 0.0, 1.0, 0.5, Percent),
    ParamInfo::log("Duración", 0.25, 4.0, 1.0, Times),
    ParamInfo::log("Apagado", 0.02, 3.0, 0.35, Seconds),
    ParamInfo::linear("Dinámica", 0.0, 1.0, 0.8, Percent),
    ParamInfo::linear("Martillo", 0.0, 1.0, 0.4, Percent),
    ParamInfo::linear("Cuerdas", 0.0, 1.0, 0.4, Percent),
    ParamInfo::linear("Cuerpo", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Campana", 0.0, 1.0, 0.4, Percent),
    ParamInfo::linear("Ladrido", 0.0, 1.0, 0.4, Percent),
    ParamInfo::log("Vel. trémolo", 0.5, 12.0, 4.5, Hertz),
    ParamInfo::linear("Trémolo", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("16'", 0.0, 8.0, 8.0, Plain),
    ParamInfo::linear("5 1/3'", 0.0, 8.0, 8.0, Plain),
    ParamInfo::linear("8'", 0.0, 8.0, 8.0, Plain),
    ParamInfo::linear("4'", 0.0, 8.0, 0.0, Plain),
    ParamInfo::linear("2 2/3'", 0.0, 8.0, 0.0, Plain),
    ParamInfo::linear("2'", 0.0, 8.0, 0.0, Plain),
    ParamInfo::linear("1 3/5'", 0.0, 8.0, 0.0, Plain),
    ParamInfo::linear("1 1/3'", 0.0, 8.0, 0.0, Plain),
    ParamInfo::linear("1'", 0.0, 8.0, 0.0, Plain),
    ParamInfo::linear("Percusión", 0.0, 1.0, 0.0, Percent),
    ParamInfo::log("Perc. caída", 0.1, 2.0, 0.5, Seconds),
    ParamInfo::linear("Click", 0.0, 1.0, 0.3, Percent),
    ParamInfo::linear("Leslie", 0.0, 1.0, 0.6, Percent),
    ParamInfo::linear("Leslie rápido", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Drive", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Ancho", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Volumen", 0.0, 1.0, 0.5, Percent),
];

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

/// What one band of an EQ Eight does to the sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EqShape {
    /// Takes away what is below the frequency, 48 dB an octave.
    LowCut48,
    LowCut12,
    LowShelf,
    Bell,
    Notch,
    HighShelf,
    HighCut12,
    /// Takes away what is above the frequency, 48 dB an octave.
    HighCut48,
}

impl EqShape {
    pub const ALL: [EqShape; 8] = [Self::LowCut48, Self::LowCut12, Self::LowShelf, Self::Bell, Self::Notch, Self::HighShelf, Self::HighCut12, Self::HighCut48];

    pub fn label(self) -> &'static str {
        match self {
            Self::LowCut48 => "Corte graves 48",
            Self::LowCut12 => "Corte graves 12",
            Self::LowShelf => "Estante graves",
            Self::Bell => "Campana",
            Self::Notch => "Notch",
            Self::HighShelf => "Estante agudos",
            Self::HighCut12 => "Corte agudos 12",
            Self::HighCut48 => "Corte agudos 48",
        }
    }

    /// Whether its gain does something (the cuts and the notch ignore it).
    pub fn has_gain(self) -> bool {
        matches!(self, Self::LowShelf | Self::Bell | Self::HighShelf)
    }
}

/// One band of an EQ Eight.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    pub on: bool,
    pub shape: EqShape,
    pub freq: f32,
    pub gain_db: f32,
    pub q: f32,
}

pub const EQ_BANDS: usize = 8;

/// The parameters of each band (frequency, gain, Q) in [`Effect::params`].
pub const EQ_BAND_PARAMS: usize = 3;

/// An eight-band parametric EQ (like Live's EQ Eight).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EqEightParams {
    pub bands: [EqBand; EQ_BANDS],
    #[serde(default, skip_serializing_if = "is_default")]
    pub output_db: f32,
}

impl EqEightParams {
    /// Each band's frequency when it is new (see [`EQ_EIGHT_PARAMS`]).
    const FREQS: [f32; EQ_BANDS] = [40.0, 100.0, 250.0, 800.0, 2000.0, 5000.0, 10000.0, 18000.0];
}

impl EqEightParams {
    fn param_mut(&mut self, index: usize) -> Option<&mut f32> {
        if index == EQ_BANDS * EQ_BAND_PARAMS {
            return Some(&mut self.output_db);
        }
        let band = self.bands.get_mut(index / EQ_BAND_PARAMS)?;
        Some(match index % EQ_BAND_PARAMS {
            0 => &mut band.freq,
            1 => &mut band.gain_db,
            _ => &mut band.q,
        })
    }
}

impl Default for EqEightParams {
    fn default() -> Self {
        let shapes = [EqShape::LowCut48, EqShape::LowShelf, EqShape::Bell, EqShape::Bell, EqShape::Bell, EqShape::Bell, EqShape::HighShelf, EqShape::HighCut12];
        Self {
            // The cuts start off: a new EQ doesn't change the sound.
            bands: std::array::from_fn(|b| EqBand { on: b != 0 && b != EQ_BANDS - 1, shape: shapes[b], freq: Self::FREQS[b], gain_db: 0.0, q: 0.71 }),
            output_db: 0.0,
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
    /// Sidechain: the track (by its id) whose sound it listens to, instead of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidechain: Option<u32>,
}

impl Default for CompressorParams {
    fn default() -> Self {
        Self {
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 120.0,
            makeup_db: 0.0,
            sidechain: None,
        }
    }
}

/// A noise gate: closes (down to `floor_db`) while the sound is under the threshold.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GateParams {
    pub threshold_db: f32,
    /// How far under the threshold the sound must fall for it to close again, dB
    /// (hysteresis: it doesn't flutter on a level near the threshold).
    pub return_db: f32,
    pub attack_ms: f32,
    /// How long it stays open after the sound falls under, ms.
    pub hold_ms: f32,
    pub release_ms: f32,
    /// How much it lets through when closed, dB (-80: nothing).
    pub floor_db: f32,
    /// Sidechain: the track (by its id) whose sound opens it, instead of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidechain: Option<u32>,
}

impl Default for GateParams {
    fn default() -> Self {
        Self { threshold_db: -40.0, return_db: 3.0, attack_ms: 0.5, hold_ms: 20.0, release_ms: 120.0, floor_db: -80.0, sidechain: None }
    }
}

/// The notes a Tuner tunes to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TunerScale {
    /// The project's scale (chromatic if it has none).
    #[default]
    Project,
    Chromatic,
    /// Its own: a major or (natural) minor scale on `tonic` (0 = C).
    Key { tonic: u8, minor: bool },
}

/// Pitch correction for a voice (see `tuner`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TunerParams {
    /// How long it takes to get to the note (0: at once, robotic).
    pub speed_ms: f32,
    /// 0..1: notes held long are left freer (their vibrato stays).
    pub humanize: f32,
    /// 0..1: how much of the way to the note.
    pub amount: f32,
    #[serde(default)]
    pub scale: TunerScale,
}

impl Default for TunerParams {
    fn default() -> Self {
        Self { speed_ms: 20.0, humanize: 0.0, amount: 1.0, scale: TunerScale::Project }
    }
}

/// Takes down the voice's esses and hisses when they jump out: what is over
/// `freq_hz`, while it is over the threshold, at most `range_db`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeEsserParams {
    pub freq_hz: f32,
    pub threshold_db: f32,
    pub range_db: f32,
}

impl Default for DeEsserParams {
    fn default() -> Self {
        Self { freq_hz: 6000.0, threshold_db: -30.0, range_db: 8.0 }
    }
}

/// Makes the start of each hit stronger or softer, and its tail longer or shorter.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransientParams {
    /// -1..1: the hits' attack, softer to harder.
    pub attack: f32,
    /// -1..1: what follows the hit, shorter to longer.
    pub sustain: f32,
    pub output_db: f32,
}

impl Default for TransientParams {
    fn default() -> Self {
        Self { attack: 0.0, sustain: 0.0, output_db: 0.0 }
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
    /// How it shapes the sound: Soft (the tanh it always had), Hard, Fold, Tube, Crush.
    #[serde(default = "soft_drive", skip_serializing_if = "is_soft_drive")]
    pub kind: KickDrive,
    /// Works at four times the sample rate: high drives without aliasing.
    #[serde(default, skip_serializing_if = "is_default")]
    pub oversample: bool,
}

fn soft_drive() -> KickDrive {
    KickDrive::Soft
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

fn is_soft_drive(kind: &KickDrive) -> bool {
    *kind == KickDrive::Soft
}

impl Default for SaturatorParams {
    fn default() -> Self {
        Self {
            drive_db: 6.0,
            output_db: -3.0,
            mix: 1.0,
            kind: KickDrive::Soft,
            oversample: false,
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
    /// Into the filter through a saturation, dB (0: clean).
    #[serde(default, skip_serializing_if = "is_default")]
    pub drive_db: f32,
    /// 24 dB per octave (two stages) instead of 12.
    #[serde(default, skip_serializing_if = "is_default")]
    pub steep: bool,
}

impl Default for AutoFilterParams {
    fn default() -> Self {
        Self {
            mode: FilterMode::LowPass,
            cutoff: 1500.0,
            resonance: 0.3,
            lfo_beats: 4.0,
            lfo_amount: 0.0,
            drive_db: 0.0,
            steep: false,
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
    /// Sidechain: the track (by its id) whose hits make it pump, instead of the
    /// beat: each hit ducks it and it comes back over `release` of the period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidechain: Option<u32>,
    /// With a sidechain, the quietest hit that still makes it pump (fully from 12 dB above).
    #[serde(default = "ducker_threshold")]
    pub threshold_db: f32,
    /// With a sidechain, listen only below 600 Hz: the kick, not the hats or claps.
    #[serde(default = "yes")]
    pub lows: bool,
}

fn ducker_threshold() -> f32 {
    -36.0
}

impl Default for DuckerParams {
    fn default() -> Self {
        Self { depth: 0.7, release: 0.6, rate_beats: 1.0, sidechain: None, threshold_db: ducker_threshold(), lows: true }
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
    /// Under this (Hz), the sound in mono: the low end of a club (0: off).
    #[serde(default, skip_serializing_if = "is_default")]
    pub bass_mono_hz: f32,
    /// The right channel upside down: a track recorded with one channel
    /// reversed (it vanishes in mono) put right.
    #[serde(default, skip_serializing_if = "is_default")]
    pub flip_right: bool,
}

impl Default for UtilityParams {
    fn default() -> Self {
        Self { gain_db: 0.0, width: 1.0, bass_mono_hz: 0.0, flip_right: false }
    }
}

/// A key, for the MIDI effects (pitch class of the tonic, 0 = C).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScaleKey {
    pub tonic: u8,
    pub minor: bool,
}

impl ScaleKey {
    fn steps(self) -> [u8; 7] {
        if self.minor { [0, 2, 3, 5, 7, 8, 10] } else { [0, 2, 4, 5, 7, 9, 11] }
    }

    /// Whether a pitch is in the scale.
    pub fn contains(self, pitch: u8) -> bool {
        self.steps().contains(&((pitch + 12 - self.tonic % 12) % 12))
    }

    /// The scale's nearest note (the lower of two as near).
    pub fn snap(self, pitch: u8) -> u8 {
        (0..=6u8)
            .flat_map(|d| [pitch.saturating_sub(d), pitch.saturating_add(d).min(127)])
            .find(|&p| self.contains(p))
            .unwrap_or(pitch)
    }

    /// The scale note `steps` scale notes above `pitch` (snapped first).
    pub fn up(self, pitch: u8, steps: usize) -> u8 {
        let mut p = self.snap(pitch);
        for _ in 0..steps {
            p = (p + 1..=127).find(|&q| self.contains(q)).unwrap_or(p);
        }
        p
    }
}

/// The arpeggiator MIDI effect (see `midi_fx`): plays the held notes one at a
/// time, in time with the transport.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArpeggiatorParams {
    pub mode: ArpMode,
    /// Beats between notes (0.25 = sixteenths).
    pub rate: f64,
    /// Octaves it climbs through, 1..=4.
    pub octaves: u8,
    /// How long each note lasts, as a fraction of a step.
    pub gate: f32,
}

impl Default for ArpeggiatorParams {
    fn default() -> Self {
        Self { mode: ArpMode::Up, rate: 0.25, octaves: 1, gate: 0.6 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChordParams {
    /// In the key's scale: each note gets its third and fifth (and seventh) in
    /// the scale, so the chords fit the key. Otherwise `intervals`.
    pub in_key: bool,
    pub seventh: bool,
    /// Semitones added above each note (0: none).
    pub intervals: [i8; 3],
    /// How loud the added notes are, relative to the played one.
    pub velocity: f32,
    /// The key for `in_key`: the project's (filled in by the app) or its own.
    pub key: Option<ScaleKey>,
    pub follow_project: bool,
}

impl Default for ChordParams {
    fn default() -> Self {
        Self { in_key: true, seventh: false, intervals: [4, 7, 0], velocity: 0.85, key: None, follow_project: true }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScaleParams {
    pub key: Option<ScaleKey>,
    pub follow_project: bool,
}

impl Default for ScaleParams {
    fn default() -> Self {
        Self { key: None, follow_project: true }
    }
}

/// What a MIDI track's notes go through before its instrument.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum MidiEffect {
    Arpeggiator(ArpeggiatorParams),
    Chord(ChordParams),
    Scale(ScaleParams),
}

impl MidiEffect {
    pub fn catalog() -> [MidiEffect; 3] {
        [MidiEffect::Arpeggiator(Default::default()), MidiEffect::Chord(Default::default()), MidiEffect::Scale(Default::default())]
    }

    pub fn label(&self) -> &'static str {
        match self {
            MidiEffect::Arpeggiator(_) => "Arpegiador",
            MidiEffect::Chord(_) => "Acorde",
            MidiEffect::Scale(_) => "Escala",
        }
    }

    /// Fills in the project's key where the effect follows it.
    pub fn with_project_key(mut self, key: Option<ScaleKey>) -> Self {
        match &mut self {
            MidiEffect::Chord(p) if p.follow_project => p.key = key,
            MidiEffect::Scale(p) if p.follow_project => p.key = key,
            _ => {}
        }
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MidiDevice {
    pub effect: MidiEffect,
    pub enabled: bool,
}

impl MidiDevice {
    pub fn new(effect: MidiEffect) -> Self {
        Self { effect, enabled: true }
    }
}

/// How many MIDI effects a track can chain.
pub const MAX_MIDI_EFFECTS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Eq3(Eq3Params),
    EqEight(EqEightParams),
    Compressor(CompressorParams),
    Gate(GateParams),
    Transient(TransientParams),
    Tuner(TunerParams),
    DeEsser(DeEsserParams),
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
    /// A VST3 effect plugin (see [`InstrumentKind::Plugin`]).
    Plugin(PluginId),
}

impl Effect {
    /// Every effect type with default settings, in browser order.
    pub fn catalog() -> [Effect; 17] {
        [
            Effect::Eq3(Default::default()),
            Effect::EqEight(Default::default()),
            Effect::Compressor(Default::default()),
            Effect::Limiter(Default::default()),
            Effect::Gate(Default::default()),
            Effect::Transient(Default::default()),
            Effect::Tuner(Default::default()),
            Effect::DeEsser(Default::default()),
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

    /// The track (by its id) whose sound drives it, if it has a sidechain.
    pub fn sidechain(&self) -> Option<u32> {
        match self {
            Effect::Compressor(p) => p.sidechain,
            Effect::Gate(p) => p.sidechain,
            Effect::Ducker(p) => p.sidechain,
            _ => None,
        }
    }

    /// Its sidechain, to point it at another track (None: it has none).
    pub fn sidechain_mut(&mut self) -> Option<&mut Option<u32>> {
        match self {
            Effect::Compressor(p) => Some(&mut p.sidechain),
            Effect::Gate(p) => Some(&mut p.sidechain),
            Effect::Ducker(p) => Some(&mut p.sidechain),
            _ => None,
        }
    }

    /// Whether it can go among a synth's own effects (the Wavetable's, a
    /// Modular's lanes): those show and modulate [`MOD_FX_SLOTS`] settings an
    /// effect at most.
    pub fn fits_in_synths(&self) -> bool {
        // A Tuner tunes a voice coming in, not a synth (which plays in tune).
        self.params().len() <= MOD_FX_SLOTS && !matches!(self, Effect::Tuner(_))
    }

    pub fn label(&self) -> &'static str {
        match self {
            Effect::Eq3(_) => "EQ Three",
            Effect::EqEight(_) => "EQ Eight",
            Effect::Compressor(_) => "Compressor",
            Effect::Gate(_) => "Gate",
            Effect::Transient(_) => "Transient",
            Effect::Tuner(_) => "Tuner",
            Effect::DeEsser(_) => "De-esser",
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
            Effect::Plugin(_) => "Plugin",
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
    /// Beats a minute.
    Bpm,
}

/// Tempo automation's range: what a tempo lane spans.
pub const TEMPO_PARAM: ParamInfo = ParamInfo::linear("Tempo", 40.0, 240.0, 120.0, Unit::Bpm);

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
    pub const fn linear(name: &'static str, min: f32, max: f32, default: f32, unit: Unit) -> Self {
        Self { name, min, max, default, log: false, unit }
    }

    pub const fn log(name: &'static str, min: f32, max: f32, default: f32, unit: Unit) -> Self {
        Self { name, min, max, default, log: true, unit }
    }

    /// `value` within the range (not a number: the default).
    pub fn clamp(&self, value: f32) -> f32 {
        if value.is_nan() { self.default } else { value.clamp(self.min, self.max) }
    }

    /// Position of `value` along the control's travel, 0..1 (not a number: the default's).
    pub fn normalize(&self, value: f32) -> f32 {
        let value = if value.is_nan() { self.default } else { value };
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
/// Frequency, gain and Q of each band, then the output.
pub const EQ_EIGHT_PARAMS: [ParamInfo; EQ_BANDS * EQ_BAND_PARAMS + 1] = [
    ParamInfo::log("Freq 1", 20.0, 20000.0, 40.0, Hertz),
    ParamInfo::linear("Gain 1", -15.0, 15.0, 0.0, Decibels),
    ParamInfo::log("Q 1", 0.1, 18.0, 0.71, Plain),
    ParamInfo::log("Freq 2", 20.0, 20000.0, 100.0, Hertz),
    ParamInfo::linear("Gain 2", -15.0, 15.0, 0.0, Decibels),
    ParamInfo::log("Q 2", 0.1, 18.0, 0.71, Plain),
    ParamInfo::log("Freq 3", 20.0, 20000.0, 250.0, Hertz),
    ParamInfo::linear("Gain 3", -15.0, 15.0, 0.0, Decibels),
    ParamInfo::log("Q 3", 0.1, 18.0, 0.71, Plain),
    ParamInfo::log("Freq 4", 20.0, 20000.0, 800.0, Hertz),
    ParamInfo::linear("Gain 4", -15.0, 15.0, 0.0, Decibels),
    ParamInfo::log("Q 4", 0.1, 18.0, 0.71, Plain),
    ParamInfo::log("Freq 5", 20.0, 20000.0, 2000.0, Hertz),
    ParamInfo::linear("Gain 5", -15.0, 15.0, 0.0, Decibels),
    ParamInfo::log("Q 5", 0.1, 18.0, 0.71, Plain),
    ParamInfo::log("Freq 6", 20.0, 20000.0, 5000.0, Hertz),
    ParamInfo::linear("Gain 6", -15.0, 15.0, 0.0, Decibels),
    ParamInfo::log("Q 6", 0.1, 18.0, 0.71, Plain),
    ParamInfo::log("Freq 7", 20.0, 20000.0, 10000.0, Hertz),
    ParamInfo::linear("Gain 7", -15.0, 15.0, 0.0, Decibels),
    ParamInfo::log("Q 7", 0.1, 18.0, 0.71, Plain),
    ParamInfo::log("Freq 8", 20.0, 20000.0, 18000.0, Hertz),
    ParamInfo::linear("Gain 8", -15.0, 15.0, 0.0, Decibels),
    ParamInfo::log("Q 8", 0.1, 18.0, 0.71, Plain),
    ParamInfo::linear("Output", -12.0, 12.0, 0.0, Decibels),
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
const AUTO_FILTER_PARAMS: [ParamInfo; 4] = [
    ParamInfo::log("Freq", 30.0, 18000.0, 1500.0, Hertz),
    // Past 95 % it rings on its own (self-oscillation).
    ParamInfo::linear("Res", 0.0, 1.2, 0.3, Percent),
    ParamInfo::linear("LFO Amount", 0.0, 4.0, 0.0, Octaves),
    ParamInfo::linear("Drive", 0.0, 24.0, 0.0, Decibels),
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
const DUCKER_PARAMS: [ParamInfo; 3] = [
    ParamInfo::linear("Depth", 0.0, 1.0, 0.7, Percent),
    ParamInfo::linear("Release", 0.05, 1.0, 0.6, Percent),
    ParamInfo::linear("Threshold", -60.0, 0.0, -36.0, Decibels),
];
const GATE_PARAMS: [ParamInfo; 6] = [
    ParamInfo::linear("Threshold", -80.0, 0.0, -40.0, Decibels),
    ParamInfo::linear("Return", 0.0, 24.0, 3.0, Decibels),
    ParamInfo::log("Attack", 0.02, 100.0, 0.5, Milliseconds),
    ParamInfo::linear("Hold", 0.0, 500.0, 20.0, Milliseconds),
    ParamInfo::log("Release", 1.0, 2000.0, 120.0, Milliseconds),
    ParamInfo::linear("Floor", -80.0, 0.0, -80.0, Decibels),
];
const TUNER_PARAMS: [ParamInfo; 3] = [
    ParamInfo::linear("Speed", 0.0, 400.0, 20.0, Milliseconds),
    ParamInfo::linear("Humanize", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Amount", 0.0, 1.0, 1.0, Percent),
];
const DE_ESSER_PARAMS: [ParamInfo; 3] = [
    ParamInfo::log("Frequency", 2000.0, 16000.0, 6000.0, Hertz),
    ParamInfo::linear("Threshold", -60.0, 0.0, -30.0, Decibels),
    ParamInfo::linear("Range", 0.0, 24.0, 8.0, Decibels),
];
const TRANSIENT_PARAMS: [ParamInfo; 3] = [
    ParamInfo::linear("Attack", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Sustain", -1.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Output", -12.0, 12.0, 0.0, Decibels),
];
const LIMITER_PARAMS: [ParamInfo; 3] = [
    ParamInfo::linear("Gain", 0.0, 24.0, 0.0, Decibels),
    ParamInfo::linear("Ceiling", -12.0, 0.0, -0.3, Decibels),
    ParamInfo::log("Release", 10.0, 1000.0, 120.0, Milliseconds),
];
const UTILITY_PARAMS: [ParamInfo; 3] = [
    ParamInfo::linear("Gain", -36.0, 12.0, 0.0, Decibels),
    ParamInfo::linear("Width", 0.0, 2.0, 1.0, Percent),
    ParamInfo::linear("Bass Mono", 0.0, 500.0, 0.0, Hertz),
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
/// Tune, decay and level of each sound in turn, then the drives (added later:
/// the first ones keep their numbers in saved automation).
const DRUM_PARAMS: [ParamInfo; DRUM_SOUNDS * 4] = {
    let names = [
        ["Kick Tune", "Kick Decay", "Kick Level", "Kick Drive"],
        ["Rim Tune", "Rim Decay", "Rim Level", "Rim Drive"],
        ["Snare Tune", "Snare Decay", "Snare Level", "Snare Drive"],
        ["Clap Tune", "Clap Decay", "Clap Level", "Clap Drive"],
        ["CH Tune", "CH Decay", "CH Level", "CH Drive"],
        ["OH Tune", "OH Decay", "OH Level", "OH Drive"],
        ["Tom Tune", "Tom Decay", "Tom Level", "Tom Drive"],
        ["Bell Tune", "Bell Decay", "Bell Level", "Bell Drive"],
        ["Mid Tom Tune", "Mid Tom Decay", "Mid Tom Level", "Mid Tom Drive"],
        ["High Tom Tune", "High Tom Decay", "High Tom Level", "High Tom Drive"],
        ["Crash Tune", "Crash Decay", "Crash Level", "Crash Drive"],
        ["Ride Tune", "Ride Decay", "Ride Level", "Ride Drive"],
        ["Shaker Tune", "Shaker Decay", "Shaker Level", "Shaker Drive"],
        ["Tamb Tune", "Tamb Decay", "Tamb Level", "Tamb Drive"],
        ["Conga Tune", "Conga Decay", "Conga Level", "Conga Drive"],
        ["Clave Tune", "Clave Decay", "Clave Level", "Clave Drive"],
    ];
    let mut table = [ParamInfo::linear("", 0.0, 1.0, 0.0, Plain); DRUM_SOUNDS * 4];
    let mut voice = 0;
    while voice < DRUM_SOUNDS {
        table[voice * 3] = ParamInfo::linear(names[voice][0], -12.0, 12.0, 0.0, Semitones);
        table[voice * 3 + 1] = ParamInfo::log(names[voice][1], 0.25, 4.0, 1.0, Times);
        table[voice * 3 + 2] = ParamInfo::linear(names[voice][2], 0.0, 1.0, 0.8, Percent);
        table[DRUM_SOUNDS * 3 + voice] = ParamInfo::linear(names[voice][3], 0.0, 1.0, 0.0, Percent);
        voice += 1;
    }
    table
};

impl InstrumentKind {
    /// The effects inside the synth (the Wavetable's and Polychrome's, a
    /// Modular's lanes').
    pub fn own_effects(&self) -> Vec<Effect> {
        let devices: Vec<Option<Device>> = match self {
            InstrumentKind::Wavetable(p) | InstrumentKind::Polychrome(p) => p.fx.to_vec(),
            InstrumentKind::Modular(p) => p.lanes.iter().flat_map(|l| l.fx).collect(),
            _ => Vec::new(),
        };
        devices.into_iter().flatten().map(|d| d.effect).collect()
    }

    /// Every instrument, in browser order.
    pub fn catalog() -> [InstrumentKind; 12] {
        [
            InstrumentKind::Synth(SynthParams::default()),
            InstrumentKind::Keys(KeysParams::default()),
            InstrumentKind::SuperSaw(SuperSawParams::default()),
            InstrumentKind::Acid(AcidParams::default()),
            InstrumentKind::Fm(FmParams::default()),
            InstrumentKind::Wavetable(WaveParams::default()),
            InstrumentKind::Polychrome(WaveParams::polychrome()),
            InstrumentKind::Modular(ModularParams::default()),
            InstrumentKind::Sampler(SamplerParams::default()),
            InstrumentKind::Kick(KickParams::default()),
            InstrumentKind::DrumMachine(DrumParams::default()),
            InstrumentKind::DrumRack(RackParams::default()),
        ]
    }

    /// The presets that ship with this kind of instrument, each as a whole instrument.
    pub fn factory_presets(&self) -> Vec<(&'static str, InstrumentKind)> {
        fn all<P>(list: Vec<(&'static str, P)>, wrap: fn(P) -> InstrumentKind) -> Vec<(&'static str, InstrumentKind)> {
            list.into_iter().map(|(name, p)| (name, wrap(p))).collect()
        }
        match self {
            InstrumentKind::Synth(_) => all(SynthParams::presets(), InstrumentKind::Synth),
            InstrumentKind::Drums | InstrumentKind::DrumMachine(_) => all(DrumParams::presets(), InstrumentKind::DrumMachine),
            InstrumentKind::SuperSaw(_) => all(SuperSawParams::presets(), InstrumentKind::SuperSaw),
            InstrumentKind::Acid(_) => all(AcidParams::presets(), InstrumentKind::Acid),
            InstrumentKind::Fm(_) => all(FmParams::presets(), InstrumentKind::Fm),
            InstrumentKind::Kick(_) => all(KickParams::presets(), InstrumentKind::Kick),
            InstrumentKind::Wavetable(_) => all(WaveParams::presets(), InstrumentKind::Wavetable),
            InstrumentKind::Polychrome(_) => all(WaveParams::polychrome_presets(), InstrumentKind::Polychrome),
            InstrumentKind::Modular(_) => all(ModularParams::presets(), InstrumentKind::Modular),
            InstrumentKind::Keys(_) => all(KeysParams::presets(), InstrumentKind::Keys),
            InstrumentKind::Sampler(_) | InstrumentKind::DrumRack(_) | InstrumentKind::Plugin(_) => Vec::new(),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            InstrumentKind::Synth(_) => "Pulse Synth",
            InstrumentKind::Drums | InstrumentKind::DrumMachine(_) => "Drum Machine",
            InstrumentKind::SuperSaw(_) => "Super Saw",
            InstrumentKind::Acid(_) => "Acid Bass",
            InstrumentKind::Fm(_) => "FM Synth",
            InstrumentKind::Sampler(_) => "Sampler",
            InstrumentKind::DrumRack(_) => "Drum Pads",
            InstrumentKind::Kick(_) => "Kick Designer",
            InstrumentKind::Wavetable(_) => "Wavetable Synth",
            InstrumentKind::Polychrome(_) => "Polychrome Synth",
            InstrumentKind::Modular(_) => "Modular Synth",
            InstrumentKind::Keys(_) => "Pulse Keys",
            InstrumentKind::Plugin(_) => "Plugin",
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
            (InstrumentKind::Wavetable(a), InstrumentKind::Wavetable(b)) | (InstrumentKind::Polychrome(a), InstrumentKind::Polychrome(b)) => {
                let layout = |p: &WaveParams| p.fx.map(|d| d.map(|d| std::mem::discriminant(&d.effect)));
                layout(a) != layout(b)
            }
            (InstrumentKind::Modular(a), InstrumentKind::Modular(b)) => a.layout_differs(b),
            (InstrumentKind::Plugin(a), InstrumentKind::Plugin(b)) => a != b,
            _ => false,
        }
    }

    /// The settings of either synth built on the wavetable engine.
    pub fn wave(&self) -> Option<&WaveParams> {
        match self {
            InstrumentKind::Wavetable(p) | InstrumentKind::Polychrome(p) => Some(p),
            _ => None,
        }
    }

    /// The same kind of engine synth with other settings.
    pub fn with_wave(&self, params: WaveParams) -> InstrumentKind {
        match self {
            InstrumentKind::Polychrome(_) => InstrumentKind::Polychrome(params),
            _ => InstrumentKind::Wavetable(params),
        }
    }

    /// True for instruments that play the track's samples.
    pub fn uses_samples(&self) -> bool {
        matches!(self, InstrumentKind::Sampler(_) | InstrumentKind::DrumRack(_) | InstrumentKind::Kick(_) | InstrumentKind::Wavetable(_) | InstrumentKind::Polychrome(_) | InstrumentKind::Modular(_))
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
            InstrumentKind::Drums | InstrumentKind::Plugin(_) => &[],
            InstrumentKind::DrumMachine(_) => &DRUM_PARAMS,
            InstrumentKind::SuperSaw(_) => &SUPERSAW_PARAMS,
            InstrumentKind::Acid(_) => &ACID_PARAMS,
            InstrumentKind::Fm(_) => &FM_PARAMS,
            InstrumentKind::Sampler(_) => &SAMPLER_PARAMS,
            InstrumentKind::DrumRack(_) => &RACK_PARAMS,
            InstrumentKind::Kick(_) => &KICK_PARAMS,
            InstrumentKind::Wavetable(_) | InstrumentKind::Polychrome(_) => &WAVE_PARAMS,
            InstrumentKind::Modular(_) => &MODULAR_PARAMS,
            InstrumentKind::Keys(_) => &KEYS_PARAMS,
        }
    }

    fn param_mut(&mut self, index: usize) -> Option<&mut f32> {
        Some(match self {
            InstrumentKind::Synth(p) => return p.param_mut(index),
            InstrumentKind::Drums | InstrumentKind::Plugin(_) => return None,
            InstrumentKind::DrumMachine(p) => match index % 3 {
                _ if index >= DRUM_SOUNDS * 3 => p.drive.get_mut(index - DRUM_SOUNDS * 3)?,
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
            InstrumentKind::Wavetable(p) | InstrumentKind::Polychrome(p) => return p.param_mut(index),
            InstrumentKind::Modular(p) => return p.param_mut(index),
            InstrumentKind::Keys(p) => return p.param_mut(index),
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
        let info = self.param_info(index);
        if let (Some(info), Some(slot)) = (info, self.param_mut(index)) {
            *slot = info.clamp(value);
        }
    }

    /// The range of parameter `index` as it is now (a modular synth's lane effect
    /// settings take their effect's), or None if there is no such parameter.
    pub fn param_info(&self, index: usize) -> Option<ParamInfo> {
        match self {
            InstrumentKind::Modular(p) => p.param_info(index),
            _ => self.params().get(index).copied(),
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
            Effect::EqEight(_) => &EQ_EIGHT_PARAMS,
            Effect::Compressor(_) => &COMPRESSOR_PARAMS,
            Effect::Gate(_) => &GATE_PARAMS,
            Effect::Transient(_) => &TRANSIENT_PARAMS,
            Effect::Tuner(_) => &TUNER_PARAMS,
            Effect::DeEsser(_) => &DE_ESSER_PARAMS,
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
            Effect::Plugin(_) => &[],
        }
    }

    fn param_mut(&mut self, index: usize) -> Option<&mut f32> {
        Some(match (self, index) {
            (Effect::Eq3(p), 0) => &mut p.low_db,
            (Effect::Eq3(p), 1) => &mut p.mid_db,
            (Effect::Eq3(p), 2) => &mut p.high_db,
            (Effect::Eq3(p), 3) => &mut p.low_freq,
            (Effect::Eq3(p), 4) => &mut p.high_freq,
            (Effect::EqEight(p), i) => return p.param_mut(i),
            (Effect::Compressor(p), 0) => &mut p.threshold_db,
            (Effect::Compressor(p), 1) => &mut p.ratio,
            (Effect::Compressor(p), 2) => &mut p.attack_ms,
            (Effect::Compressor(p), 3) => &mut p.release_ms,
            (Effect::Compressor(p), 4) => &mut p.makeup_db,
            (Effect::Gate(p), 0) => &mut p.threshold_db,
            (Effect::Gate(p), 1) => &mut p.return_db,
            (Effect::Gate(p), 2) => &mut p.attack_ms,
            (Effect::Gate(p), 3) => &mut p.hold_ms,
            (Effect::Gate(p), 4) => &mut p.release_ms,
            (Effect::Gate(p), 5) => &mut p.floor_db,
            (Effect::Transient(p), 0) => &mut p.attack,
            (Effect::Transient(p), 1) => &mut p.sustain,
            (Effect::Transient(p), 2) => &mut p.output_db,
            (Effect::Tuner(p), 0) => &mut p.speed_ms,
            (Effect::Tuner(p), 1) => &mut p.humanize,
            (Effect::Tuner(p), 2) => &mut p.amount,
            (Effect::DeEsser(p), 0) => &mut p.freq_hz,
            (Effect::DeEsser(p), 1) => &mut p.threshold_db,
            (Effect::DeEsser(p), 2) => &mut p.range_db,
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
            (Effect::AutoFilter(p), 3) => &mut p.drive_db,
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
            (Effect::Ducker(p), 2) => &mut p.threshold_db,
            (Effect::Limiter(p), 0) => &mut p.gain_db,
            (Effect::Limiter(p), 1) => &mut p.ceiling_db,
            (Effect::Limiter(p), 2) => &mut p.release_ms,
            (Effect::Utility(p), 0) => &mut p.gain_db,
            (Effect::Utility(p), 1) => &mut p.width,
            (Effect::Utility(p), 2) => &mut p.bass_mono_hz,
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
            *slot = info.clamp(value);
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
            *slot = info.clamp(value);
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
    /// The song's tempo (BPM), on the master: tempo automation.
    Tempo,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationPoint {
    pub beat: f64,
    pub value: f32,
    /// How the stretch from this point to the next bends, -1..1: 0 is a straight
    /// line; above, it moves quickly at first and settles; below, it starts slowly
    /// and speeds up (as dragging a segment with ⌥ in Live).
    #[serde(default, skip_serializing_if = "is_straight")]
    pub curve: f32,
}

fn is_straight(curve: &f32) -> bool {
    *curve == 0.0
}

/// Where a bent stretch is, `u` (0..1) of the way along it: see [`AutomationPoint::curve`].
pub fn bend(u: f32, curve: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    let power = 1.0 + curve.abs() * 5.0;
    if curve >= 0.0 {
        1.0 - (1.0 - u).powf(power)
    } else {
        u.powf(power)
    }
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
        Some(a.value + (b.value - a.value) * bend(t, a.curve))
    }

    pub fn sort(&mut self) {
        self.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    }

    /// The bend of the stretch at `beat` (for a point added there, so the rest of
    /// the stretch keeps much of its shape).
    pub fn curve_at(&self, beat: f64) -> f32 {
        let next = self.points.partition_point(|p| p.beat <= beat);
        next.checked_sub(1).and_then(|i| self.points.get(i)).filter(|_| next < self.points.len()).map_or(0.0, |p| p.curve)
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
/// Analog drift and harmonic settings of oscillator A (B's follow).
pub const WAVE_OSC_EXTRA: usize = WAVE_ARP_GATE + 1;
/// Filter 2 cutoff, resonance, drive, mix and key tracking, then the filter balance.
pub const WAVE_FILTER2: usize = WAVE_OSC_EXTRA + 8;
pub const WAVE_BALANCE: usize = WAVE_FILTER2 + 5;
pub const WAVE_RANDOM_RATE: usize = WAVE_BALANCE + 1;
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
    /// Virtual analog: Position morphs sine → triangle → saw → square, Amount is
    /// the pulse width, Drift the slow pitch wander of analog circuits.
    Analog,
    /// Additive: a series of partials shaped by count, tilt and odd/even balance,
    /// with a formant peak at Position (gain: Amount).
    Harmonic,
}

impl OscKind {
    pub const ALL: [OscKind; 6] = [Self::Wavetable, Self::Analog, Self::Harmonic, Self::Sample, Self::Granular, Self::Spectral];

    pub fn label(self) -> &'static str {
        match self {
            Self::Wavetable => "Wavetable",
            Self::Sample => "Sample",
            Self::Granular => "Granular",
            Self::Spectral => "Spectral",
            Self::Analog => "Analog",
            Self::Harmonic => "Harmonic",
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
    /// A new random value at the random rate, held in between (sample and hold).
    SampleHold,
    /// A random value gliding smoothly to the next one.
    Smooth,
    Combo1,
    Combo2,
    /// The current step's value in the sequencer.
    Sequencer,
}

impl ModSource {
    pub const ALL: [ModSource; 20] = [
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
        Self::SampleHold,
        Self::Smooth,
        Self::Combo1,
        Self::Combo2,
        Self::Sequencer,
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
            Self::SampleHold => "S&H",
            Self::Smooth => "Smooth Rnd",
            Self::Combo1 => "Combo 1",
            Self::Combo2 => "Combo 2",
            Self::Sequencer => "Seq Mod",
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
    /// Analog: slow random pitch wander, 0..1 (up to ±15 cents).
    #[serde(default)]
    pub drift: f32,
    /// Harmonic: number of partials, 1..64.
    #[serde(default = "default_partials")]
    pub partials: f32,
    /// Harmonic: spectral slope, -1 (dark) .. 1 (bright); 0 is a saw's 1/n.
    #[serde(default)]
    pub tilt: f32,
    /// Harmonic: -1 odd partials only .. 1 even partials only.
    #[serde(default)]
    pub parity: f32,
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

fn default_partials() -> f32 {
    32.0
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
            drift: 0.0,
            partials: default_partials(),
            tilt: 0.0,
            parity: 0.0,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComboOp {
    #[default]
    Multiply,
    Add,
    Min,
    Max,
    Average,
}

impl ComboOp {
    pub const ALL: [ComboOp; 5] = [Self::Multiply, Self::Add, Self::Min, Self::Max, Self::Average];

    pub fn label(self) -> &'static str {
        match self {
            Self::Multiply => "×",
            Self::Add => "+",
            Self::Min => "Min",
            Self::Max => "Max",
            Self::Average => "Media",
        }
    }

    pub fn apply(self, a: f32, b: f32) -> f32 {
        match self {
            Self::Multiply => a * b,
            Self::Add => (a + b).min(1.0),
            Self::Min => a.min(b),
            Self::Max => a.max(b),
            Self::Average => (a + b) * 0.5,
        }
    }
}

/// Combines two modulation sources into a new one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Combo {
    pub a: ModSource,
    pub b: ModSource,
    pub op: ComboOp,
}

pub const SEQ_STEPS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeqStep {
    /// Off steps are rests.
    pub on: bool,
    /// Semitones from the held key.
    pub pitch: f32,
    pub velocity: f32,
    /// Note length, as a fraction of a step.
    pub gate: f32,
    /// Glides into the next step without a new attack.
    pub slide: bool,
    /// Chance the step plays, 0..1.
    pub probability: f32,
    /// The step's value on the sequencer's modulation lane, 0..1.
    pub value: f32,
}

impl Default for SeqStep {
    fn default() -> Self {
        Self { on: true, pitch: 0.0, velocity: 0.8, gate: 0.5, slide: false, probability: 1.0, value: 0.0 }
    }
}

/// Step sequencer: while a key is held, plays its steps transposed to that key.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeqParams {
    pub on: bool,
    pub steps: [SeqStep; SEQ_STEPS],
    /// Steps in the loop, 1..16.
    pub length: u8,
    pub rate_beats: f32,
}

impl Default for SeqParams {
    fn default() -> Self {
        Self { on: false, steps: [SeqStep::default(); SEQ_STEPS], length: 16, rate_beats: 0.25 }
    }
}

fn default_filter2() -> WaveFilterParams {
    WaveFilterParams { on: false, mode: WaveFilter::Hp12, routes: [true; 4], cutoff: 120.0, resonance: 0.1, drive: 0.0, mix: 1.0, key_track: 0.0 }
}

fn default_random_rate() -> f32 {
    4.0
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
    /// Second filter; its `routes` are unused (it takes what goes through filter 1).
    #[serde(default = "default_filter2")]
    pub filter2: WaveFilterParams,
    /// Filters side by side (mixed by `filter_balance`) instead of one after the other.
    #[serde(default)]
    pub filter_parallel: bool,
    #[serde(default = "half")]
    pub filter_balance: f32,
    /// Rate of the random sources, in Hz.
    #[serde(default = "default_random_rate")]
    pub random_rate: f32,
    #[serde(default)]
    pub combos: [Combo; 2],
    #[serde(default)]
    pub seq: SeqParams,
}

fn half() -> f32 {
    0.5
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
            filter2: default_filter2(),
            filter_parallel: false,
            filter_balance: 0.5,
            random_rate: default_random_rate(),
            combos: [Combo::default(); 2],
            seq: SeqParams::default(),
        }
    }
}

impl WaveParams {
    /// Starting point of the polychrome synth: an analog engine through a ladder filter.
    pub fn polychrome() -> WaveParams {
        let mut p = WaveParams::default();
        p.osc[0].kind = OscKind::Analog;
        p.osc[0].position = 2.0 / 3.0;
        p.osc[0].drift = 0.2;
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Ladder, cutoff: 6000.0, resonance: 0.15, ..p.filter };
        p
    }

    /// Factory presets of the polychrome synth.
    pub fn polychrome_presets() -> Vec<(&'static str, WaveParams)> {
        let init = WaveParams::polychrome();
        let saw = 2.0 / 3.0;
        let slot = |source, dest: usize, amount| ModSlot { source, dest: dest as u8, amount, bipolar: false };
        let env = |attack, decay, sustain, release| WaveEnv { attack, hold: 0.0, decay, sustain, release };
        let fx = |effect: Effect| Some(Device::new(effect));
        let delay = |time_beats: f64, mix: f32| fx(Effect::Delay(DelayParams { time_beats, feedback: 0.35, mix, ping_pong: true, ..DelayParams::default() }));
        let reverb = |size: f32, mix: f32| fx(Effect::Reverb(ReverbParams { size, mix, ..ReverbParams::default() }));
        let chorus = fx(Effect::Chorus(ChorusParams::default()));
        let analog = |position: f32| WaveOsc { kind: OscKind::Analog, position, random_phase: 1.0, ..WaveOsc::default() };
        let harmonic = |partials: f32, tilt: f32, parity: f32| WaveOsc { kind: OscKind::Harmonic, partials, tilt, parity, random_phase: 0.0, ..WaveOsc::default() };
        let steps = |pattern: &[(f32, bool, f32, bool)]| {
            let mut steps = [SeqStep::default(); SEQ_STEPS];
            for (i, &(pitch, on, velocity, slide)) in pattern.iter().enumerate().take(SEQ_STEPS) {
                steps[i] = SeqStep { on, pitch, velocity, slide, gate: 0.55, probability: 1.0, value: velocity };
            }
            steps
        };
        let triangle = Shape::from_points(&[(0.0, 0.0, 0.0), (0.5, 1.0, 0.0), (1.0, 0.0, 0.0)]);
        let mut list = vec![("Init", init)];

        let mut p = init;
        p.osc[0] = WaveOsc { unison: 4.0, detune: 0.2, drift: 0.4, ..analog(saw) };
        p.osc[1] = WaveOsc { on: true, octave: -1.0, level: 0.4, drift: 0.3, warp_amount: 0.3, ..analog(1.0) };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Ladder, cutoff: 1800.0, resonance: 0.2, ..init.filter };
        p.env[0] = env(0.6, 1.0, 0.9, 1.2);
        p.lfo[0] = WaveLfo { rate: 0.2, shape: triangle, ..init.lfo[0] };
        p.matrix[0] = slot(ModSource::Lfo1, 32, 0.2);
        p.fx[0] = chorus;
        p.fx[1] = reverb(0.8, 0.3);
        p.gain = 0.45;
        list.push(("Analog · Warm Pad", p));

        let mut p = init;
        p.osc[0] = WaveOsc { unison: 2.0, detune: 0.12, drift: 0.3, ..analog(saw) };
        p.osc[1] = WaveOsc { on: true, unison: 2.0, detune: 0.15, fine: 7.0, level: 0.5, ..analog(saw) };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Ladder, cutoff: 500.0, resonance: 0.15, ..init.filter };
        p.env[0] = env(0.03, 0.3, 0.8, 0.2);
        p.env[1] = env(0.06, 0.4, 0.3, 0.2);
        p.matrix[0] = slot(ModSource::Env2, 32, 0.5);
        p.fx[0] = reverb(0.5, 0.15);
        p.gain = 0.5;
        list.push(("Analog · Brass", p));

        let mut p = init;
        p.osc[0] = WaveOsc { drift: 0.8, unison: 2.0, detune: 0.08, ..analog(saw) };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Lp24, cutoff: 4000.0, resonance: 0.2, ..init.filter };
        p.mono = true;
        p.legato = true;
        p.glide = 50.0;
        p.lfo[0] = WaveLfo { rate: 5.5, rise: 0.4, ..init.lfo[0] };
        p.matrix[0] = slot(ModSource::Lfo1, 3, 0.04);
        p.fx[0] = delay(0.75, 0.2);
        p.gain = 0.85;
        list.push(("Analog · Drift Lead", p));

        let mut p = init;
        p.osc[0] = harmonic(24.0, 0.3, 0.4);
        p.filter.on = false;
        p.env[0] = env(0.002, 1.5, 0.0, 0.8);
        p.env[1] = env(0.002, 0.6, 0.0, 0.4);
        p.osc[0].warp_amount = 0.5;
        p.matrix[0] = slot(ModSource::Env2, 0, 0.6);
        p.fx[0] = reverb(0.7, 0.25);
        p.gain = 0.6;
        list.push(("Harmonic · Glass Keys", p));

        let mut p = init;
        p.osc[0] = harmonic(16.0, 0.5, -0.3);
        p.filter.on = false;
        p.env[0] = env(0.004, 0.2, 1.0, 0.05);
        p.fx[0] = chorus;
        p.gain = 0.55;
        list.push(("Harmonic · Drawbar Organ", p));

        let mut p = init;
        p.osc[0] = WaveOsc { unison: 4.0, detune: 0.18, warp_amount: 0.8, position: 0.2, ..harmonic(40.0, -0.2, 0.0) };
        p.filter.on = false;
        p.lfo[0] = WaveLfo { rate: 0.15, shape: triangle, ..init.lfo[0] };
        p.matrix[0] = slot(ModSource::Lfo1, 0, 0.35);
        p.env[0] = env(0.5, 1.0, 1.0, 1.5);
        p.fx[0] = reverb(0.85, 0.35);
        p.gain = 0.3;
        list.push(("Harmonic · Choir", p));

        let mut p = init;
        p.osc[0] = analog(saw);
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Ladder, cutoff: 350.0, resonance: 0.75, drive: 0.4, ..init.filter };
        p.env[0] = env(0.002, 0.3, 0.7, 0.05);
        p.env[1] = env(0.001, 0.2, 0.0, 0.1);
        p.matrix[0] = slot(ModSource::Env2, 32, 0.35);
        p.matrix[1] = slot(ModSource::Sequencer, 32, 0.35);
        p.mono = true;
        p.legato = true;
        p.glide = 40.0;
        p.seq = SeqParams {
            on: true,
            length: 16,
            rate_beats: 0.25,
            steps: steps(&[
                (0.0, true, 0.9, false),
                (0.0, true, 0.5, false),
                (12.0, true, 0.6, true),
                (0.0, true, 0.5, false),
                (3.0, true, 0.9, false),
                (0.0, false, 0.5, false),
                (7.0, true, 0.6, true),
                (12.0, true, 0.5, false),
                (0.0, true, 0.9, false),
                (0.0, true, 0.5, false),
                (10.0, true, 0.6, false),
                (0.0, false, 0.5, false),
                (5.0, true, 0.9, false),
                (0.0, true, 0.5, true),
                (12.0, true, 0.6, false),
                (7.0, true, 0.5, false),
            ]),
        };
        p.fx[0] = delay(0.75, 0.15);
        p.gain = 0.55;
        list.push(("Seq · Acid Line", p));

        let mut p = init;
        p.osc[0] = WaveOsc { unison: 3.0, detune: 0.2, warp_amount: 0.2, ..analog(1.0) };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Lp24, cutoff: 2000.0, resonance: 0.3, ..init.filter };
        p.env[0] = env(0.002, 0.15, 0.0, 0.1);
        let mut stab = steps(&[(0.0, true, 0.9, false), (0.0, false, 0.5, false), (0.0, true, 0.6, false), (0.0, false, 0.5, false)].repeat(4));
        for (i, step) in stab.iter_mut().enumerate() {
            if i % 4 == 2 {
                step.probability = 0.5;
            }
            if i == 14 {
                step.pitch = 5.0;
                step.on = true;
            }
        }
        p.seq = SeqParams { on: true, length: 16, rate_beats: 0.25, steps: stab };
        p.fx[0] = delay(0.75, 0.25);
        p.fx[1] = reverb(0.6, 0.2);
        p.gain = 0.95;
        list.push(("Seq · Techno Stab", p));

        let mut p = init;
        p.osc[0] = harmonic(20.0, 0.1, 0.2);
        p.filter.on = false;
        p.env[0] = env(0.002, 0.35, 0.0, 0.25);
        p.seq = SeqParams {
            on: true,
            length: 8,
            rate_beats: 0.5,
            steps: steps(&[
                (0.0, true, 0.8, false),
                (3.0, true, 0.6, false),
                (7.0, true, 0.7, false),
                (10.0, true, 0.6, false),
                (12.0, true, 0.8, false),
                (10.0, true, 0.6, false),
                (7.0, true, 0.7, false),
                (5.0, true, 0.6, false),
            ]),
        };
        p.fx[0] = delay(0.75, 0.3);
        p.fx[1] = reverb(0.7, 0.25);
        p.gain = 1.0;
        list.push(("Seq · Melodic Pluck", p));

        let mut p = init;
        p.osc[0] = WaveOsc { drift: 0.15, ..analog(0.15) };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Lp12, cutoff: 800.0, ..init.filter };
        p.mono = true;
        p.glide = 25.0;
        p.env[0] = env(0.003, 1.0, 1.0, 0.08);
        p.gain = 0.9;
        list.push(("Bass · Analog Sub", p));

        let mut p = init;
        p.osc[0] = WaveOsc { kind: OscKind::Wavetable, table: 3, unison: 3.0, detune: 0.2, ..WaveOsc::default() };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Lp24, cutoff: 1200.0, resonance: 0.3, ..init.filter };
        p.filter2 = WaveFilterParams { on: true, mode: WaveFilter::Bp12, cutoff: 900.0, resonance: 0.5, ..p.filter2 };
        p.filter_parallel = true;
        p.random_rate = 1.5;
        p.matrix[0] = slot(ModSource::Smooth, 32, 0.35);
        p.matrix[1] = slot(ModSource::Smooth, 0, 0.6);
        p.matrix[2] = slot(ModSource::SampleHold, 99, 0.3);
        p.env[0] = env(0.8, 1.0, 1.0, 1.8);
        p.fx[0] = reverb(0.9, 0.35);
        p.gain = 0.55;
        list.push(("Pad · Random Motion", p));

        let mut p = init;
        p.osc[0] = WaveOsc { unison: 3.0, detune: 0.15, ..analog(saw) };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Formant, cutoff: 300.0, resonance: 0.5, ..init.filter };
        p.filter2 = WaveFilterParams { on: true, mode: WaveFilter::Lp24, cutoff: 900.0, resonance: 0.3, ..p.filter2 };
        p.filter_parallel = true;
        p.lfo[0] = WaveLfo { shape: triangle, sync_beats: 4.0, ..init.lfo[0] };
        p.matrix[0] = slot(ModSource::Lfo1, 104, 1.0);
        p.lfo[1] = WaveLfo { shape: triangle, sync_beats: 8.0, ..init.lfo[1] };
        p.matrix[1] = slot(ModSource::Lfo2, 32, 0.4);
        p.gain = 0.3;
        list.push(("Dual Filter · Vowel Sweep", p));

        let mut p = init;
        p.osc[0] = WaveOsc { unison: 2.0, detune: 0.1, ..analog(1.0) };
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Lp24, cutoff: 300.0, resonance: 0.4, ..init.filter };
        p.lfo[0] = WaveLfo { shape: triangle, sync_beats: 0.25, mode: LfoMode::Retrig, ..init.lfo[0] };
        p.env[1] = env(0.5, 2.0, 0.0, 0.5);
        p.combos[0] = Combo { a: ModSource::Lfo1, b: ModSource::Env2, op: ComboOp::Multiply };
        p.matrix[0] = slot(ModSource::Combo1, 32, 0.7);
        p.env[0] = env(0.01, 2.5, 0.0, 0.3);
        p.fx[0] = delay(0.5, 0.2);
        p.gain = 0.45;
        list.push(("FX · Combo Pulse", p));
        list
    }

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
                91 => &mut p.osc[0].drift,
                92 => &mut p.osc[0].partials,
                93 => &mut p.osc[0].tilt,
                94 => &mut p.osc[0].parity,
                95 => &mut p.osc[1].drift,
                96 => &mut p.osc[1].partials,
                97 => &mut p.osc[1].tilt,
                98 => &mut p.osc[1].parity,
                99 => &mut p.filter2.cutoff,
                100 => &mut p.filter2.resonance,
                101 => &mut p.filter2.drive,
                102 => &mut p.filter2.mix,
                103 => &mut p.filter2.key_track,
                104 => &mut p.filter_balance,
                105 => &mut p.random_rate,
            _ => return None,
        })
    }
}

pub const WAVE_PARAMS: [ParamInfo; 106] = [
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
    ParamInfo::linear("A Drift", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("A Partials", 1.0, 64.0, 32.0, Count),
    ParamInfo::linear("A Tilt", -1.0, 1.0, 0.0, Plain),
    ParamInfo::linear("A Parity", -1.0, 1.0, 0.0, Plain),
    ParamInfo::linear("B Drift", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("B Partials", 1.0, 64.0, 32.0, Count),
    ParamInfo::linear("B Tilt", -1.0, 1.0, 0.0, Plain),
    ParamInfo::linear("B Parity", -1.0, 1.0, 0.0, Plain),
    ParamInfo::log("F2 Cutoff", 20.0, 20000.0, 120.0, Hertz),
    ParamInfo::linear("F2 Res", 0.0, 1.0, 0.1, Percent),
    ParamInfo::linear("F2 Drive", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("F2 Mix", 0.0, 1.0, 1.0, Percent),
    ParamInfo::linear("F2 Key", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("F Balance", 0.0, 1.0, 0.5, Percent),
    ParamInfo::log("Rnd Rate", 0.1, 40.0, 4.0, Hertz),
];

// ----- Modular synth ------------------------------------------------------------------
//
// A stack of modules, read top to bottom inside groups: generators add their
// sound to the signal from above, effects process it, Mix and Aux tap or inject
// it, and output modules shape it with an envelope and send it to an effect
// lane. Any module's audio can modulate another's phase, frequency, level or
// pitch at audio rate. Every module setting is a 0..1 slot whose meaning
// depends on the module (see [`module_slot`]).

pub const MOD_MODULES: usize = 32;
pub const MOD_SLOTS: usize = 16;
pub const MOD_AUDIO: usize = 32;
pub const MOD_LANES: usize = 3;
pub const MOD_LANE_FX: usize = 8;
/// Effects per lane in the first layout.
pub const MOD_FIRST_LANE_FX: usize = 4;
pub const MOD_MODULATORS: usize = 32;
/// Modulators whose first four settings have numbers in the first layout.
pub const MOD_FIRST_MODULATORS: usize = 16;
pub const MOD_TARGETS: usize = 8;
pub const MOD_MACROS: usize = 8;
pub const MOD_AUDIO_AMOUNT: usize = 512;
pub const MOD_LANE_GAIN: usize = 544;
pub const MOD_MODULATOR_PARAMS: usize = 547;
pub const MOD_MACRO_PARAMS: usize = 611;
pub const MOD_GLIDE: usize = 619;
pub const MOD_MASTER: usize = 620;
/// Slots 16..24 of every module live here, 8 per module (added after the first layout).
pub const MOD_EXTRA: usize = 621;
pub const MOD_EXTRA_SLOTS: usize = 8;
/// All the slots of a module.
pub const MOD_MODULE_SLOTS: usize = MOD_SLOTS + MOD_EXTRA_SLOTS;
/// Global unison: detune, spread, blend, bias (range, center or balance by mode).
pub const MOD_UNISON: usize = MOD_EXTRA + MOD_MODULES * MOD_EXTRA_SLOTS;
pub const MOD_UNISON_VOICES: usize = 8;
/// Every modulator's eight settings (the first 16 modulators' first four also have
/// their numbers from the first layout, at [`MOD_MODULATOR_PARAMS`]).
pub const MOD_MODULATOR_SLOTS_AT: usize = MOD_UNISON + 4;
pub const MOD_MODULATOR_SLOTS: usize = 8;
/// The continuous settings of every lane effect, [`MOD_FX_SLOTS`] per effect
/// (in the effect's own units, see [`ModularParams::param_info`]).
pub const MOD_LANE_FX_PARAMS: usize = MOD_MODULATOR_SLOTS_AT + MOD_MODULATORS * MOD_MODULATOR_SLOTS;
pub const MOD_FX_SLOTS: usize = 6;
/// Dry/wet of each lane.
pub const MOD_LANE_MIX: usize = MOD_LANE_FX_PARAMS + MOD_LANES * MOD_LANE_FX * MOD_FX_SLOTS;
/// Master pitch in semitones and its fine tuning in cents.
pub const MOD_PITCH: usize = MOD_LANE_MIX + MOD_LANES;
pub const MOD_FINE: usize = MOD_PITCH + 1;
pub const MOD_PARAM_COUNT: usize = MOD_FINE + 1;
/// An output's send (`Module::choice[0]`) to the sideband bus: heard only by the
/// audio follower and pitch tracker.
pub const MOD_SEND_SIDEBAND: u8 = 4;
pub const MOD_POLYPHONY: u8 = 32;
/// `Modulator::source` and `trigger_source` when nothing is connected.
pub const MOD_NO_MODULATOR: u8 = 255;
/// `ModTarget::dest` of an unused target.
pub const MOD_NO_TARGET: u16 = u16::MAX;
/// `Lane::dest` (and an output's send) meaning the master bus.
pub const MOD_NO_LANE: u8 = 255;
/// `AudioMod::source` of an unused connection.
pub const MOD_NO_MODULE: u8 = 255;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModuleKind {
    /// A free place in the stack.
    #[default]
    Empty,
    /// Starts a group: the signal from above does not continue past it.
    Group,
    Analog,
    Wavetable,
    Sample,
    Noise,
    Distortion,
    Filter,
    /// Grains of a sample, spawned at a rate, with randomisation and chords.
    Granular,
    /// A resonant filter with saturation inside its feedback loop.
    NonlinearFilter,
    /// Like Output, but the level follows a drawn curve instead of an envelope.
    CurveOutput,
    /// Passes on (and exposes, for audio-rate modulation) the sum of what is above.
    Mix,
    /// Adds the audio routed to its input from anywhere in the stack.
    Aux,
    /// Shapes the signal with an envelope and sends it to a lane or the master.
    Output,
}

impl ModuleKind {
    /// Modules that can be added, in menu order.
    pub const ALL: [ModuleKind; 13] = [
        Self::Group,
        Self::Analog,
        Self::Wavetable,
        Self::Sample,
        Self::Granular,
        Self::Noise,
        Self::Distortion,
        Self::Filter,
        Self::NonlinearFilter,
        Self::Mix,
        Self::Aux,
        Self::Output,
        Self::CurveOutput,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "—",
            Self::Group => "Group",
            Self::Analog => "Analog",
            Self::Wavetable => "Wavetable",
            Self::Sample => "Sample",
            Self::Noise => "Noise",
            Self::Distortion => "Distortion",
            Self::Filter => "Filter",
            Self::Granular => "Granular",
            Self::NonlinearFilter => "Nonlinear Filter",
            Self::CurveOutput => "Curve Output",
            Self::Mix => "Mix",
            Self::Aux => "Aux",
            Self::Output => "Output",
        }
    }

    pub fn is_generator(self) -> bool {
        matches!(self, Self::Analog | Self::Wavetable | Self::Sample | Self::Noise | Self::Granular)
    }

    /// Modules that send the signal out (and end the voice when they fall silent).
    pub fn is_output(self) -> bool {
        matches!(self, Self::Output | Self::CurveOutput)
    }

    /// Modules that play an audio file.
    pub fn uses_sample(self) -> bool {
        matches!(self, Self::Sample | Self::Granular)
    }

    /// Generators with their own oscillator unison.
    pub fn has_unison(self) -> bool {
        matches!(self, Self::Analog | Self::Wavetable | Self::Sample)
    }
}

/// Names of a module's discrete choices.
pub const ANALOG_WAVES: [&str; 4] = ["Saw", "Pulse", "Triangle", "Sine"];
pub const DISTORTION_TYPES: [&str; 6] = ["Overdrive", "Saturate", "Foldback", "Sine", "Hard Clip", "Quantize"];
pub const FILTER_TYPES: [&str; 7] = ["Low Pass", "Band Pass", "High Pass", "Notch", "Low Shelf", "Peak", "High Shelf"];
pub const NONLINEAR_TYPES: [&str; 5] = ["Low Pass", "Band Pass", "High Pass", "Notch", "All Pass"];
/// Flavours of the nonlinear filter's saturation ("Clean" adds none).
pub const NONLINEAR_MODES: [&str; 5] = ["Clean", "Warm", "Tape", "Diode", "Fuzz"];
pub const SAMPLE_LOOPS: [&str; 5] = ["Off", "Infinite", "Sustain", "Ping Pong", "Reverse"];
pub const NOISE_TYPES: [&str; 3] = ["Colored", "Stepped", "Smooth"];
pub const NOISE_SEEDS: [&str; 2] = ["Random", "Stable"];
pub const GRAIN_SPAWN: [&str; 3] = ["Free Rate", "Synced", "Density"];
/// Synced grain rates, in beats per grain, and their names.
pub const GRAIN_SYNC: [(f32, &str); 8] =
    [(4.0, "1/1"), (2.0, "1/2"), (1.0, "1/4"), (0.5, "1/8"), (1.0 / 3.0, "1/8T"), (0.25, "1/16"), (0.125, "1/32"), (0.0625, "1/64")];
pub const CHORD_PICKS: [&str; 4] = ["Up", "Down", "Up-Down", "Random"];
pub const CURVE_LOOPS: [&str; 4] = ["Off", "Loop", "Sustain", "Ping Pong"];
/// Chords for grains and the global unison: names and intervals in semitones.
pub const CHORDS: [(&str, &[i8]); 15] = [
    ("None", &[0]),
    ("Octaves", &[0]),
    ("Fifths", &[0, 7]),
    ("Major", &[0, 4, 7]),
    ("Minor", &[0, 3, 7]),
    ("Sus2", &[0, 2, 7]),
    ("Sus4", &[0, 5, 7]),
    ("Major 7", &[0, 4, 7, 11]),
    ("Minor 7", &[0, 3, 7, 10]),
    ("Dominant 7", &[0, 4, 7, 10]),
    ("Diminished", &[0, 3, 6, 9]),
    ("Augmented", &[0, 4, 8]),
    ("Major 9", &[0, 4, 7, 11, 14]),
    ("Pentatonic", &[0, 2, 4, 7, 9]),
    ("Minor Pentatonic", &[0, 3, 5, 7, 10]),
];

/// The `step`th note (from 0, upwards) of `chord`, in semitones above the root.
pub fn chord_note(chord: usize, step: usize) -> f32 {
    let intervals = CHORDS.get(chord).map_or(&[0i8][..], |c| c.1);
    let octave = if chord == 0 { 0 } else { step / intervals.len() };
    (intervals[step % intervals.len()] as f32) + 12.0 * octave as f32
}

/// How many notes `chord` has in `octaves` octaves (at least one).
pub fn chord_size(chord: usize, octaves: f32) -> usize {
    let intervals = CHORDS.get(chord).map_or(1, |c| c.1.len());
    if chord == 0 {
        return 1;
    }
    let span = octaves.max(0.0) * 12.0;
    (0..64).take_while(|&k| chord_note(chord, k) <= span + 0.01).count().max(1).min(intervals * 8)
}

/// Global unison modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnisonMode {
    /// Slight detune, every voice starting at the same phase.
    Hard,
    /// Slight detune, voices starting at different phases.
    #[default]
    Smooth,
    /// Slight detune, phases evenly spread.
    Synthetic,
    /// Voices at multiples of the note's frequency.
    FrequencyStack,
    /// Voices a pitch interval apart.
    PitchStack,
    /// Voices an octave apart, quieter away from the center: an endless rise or fall.
    Shepard,
    /// Voices tuned to the notes of a chord.
    Chords,
}

impl UnisonMode {
    pub const ALL: [UnisonMode; 7] = [Self::Hard, Self::Smooth, Self::Synthetic, Self::FrequencyStack, Self::PitchStack, Self::Shepard, Self::Chords];

    pub fn label(self) -> &'static str {
        match self {
            Self::Hard => "Hard",
            Self::Smooth => "Smooth",
            Self::Synthetic => "Synthetic",
            Self::FrequencyStack => "Frequency Stack",
            Self::PitchStack => "Pitch Stack",
            Self::Shepard => "Shepard",
            Self::Chords => "Chords",
        }
    }

    /// What the fourth unison knob does in this mode.
    pub fn bias_label(self) -> &'static str {
        match self {
            Self::FrequencyStack | Self::PitchStack => "Range",
            Self::Shepard => "Center",
            Self::Chords => "Balance",
            _ => "Bias",
        }
    }
}

/// Unison of the whole voice: every module plays once per unison voice.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Unison {
    pub voices: u8,
    pub mode: UnisonMode,
    /// Chords mode: an index into [`CHORDS`].
    pub chord: u8,
    pub detune: f32,
    pub spread: f32,
    pub blend: f32,
    /// -1..1: bias, range, center or balance depending on the mode.
    pub bias: f32,
}

impl Default for Unison {
    fn default() -> Self {
        Self { voices: 1, mode: UnisonMode::Smooth, chord: 3, detune: 0.25, spread: 0.7, blend: 0.5, bias: 0.0 }
    }
}

/// Module slot indices.
pub mod slot {
    pub const LEVEL: usize = 0;
    pub const SEMI: usize = 1;
    pub const CENT: usize = 2;
    pub const HARMONIC: usize = 3;
    pub const SHIFT: usize = 4;
    pub const PHASE: usize = 5;
    pub const PHASE_RANDOM: usize = 6;
    /// First generator-specific setting (sync, frame, offset, slope…).
    pub const A: usize = 7;
    pub const B: usize = 8;
    pub const UNISON: usize = 12;
    pub const DETUNE: usize = 13;
    pub const SPREAD: usize = 14;
    pub const BLEND: usize = 15;
    /// Wavetable.
    pub const BANDLIMIT: usize = 8;
    /// Sample (A: offset, B: root).
    pub const LOOP_START: usize = 9;
    pub const LOOP_LENGTH: usize = 10;
    pub const XFADE: usize = 11;
    /// Noise.
    pub const SLOPE: usize = 7;
    pub const NOISE_STEREO: usize = 8;
    /// Granular.
    pub const POSITION: usize = 7;
    pub const LENGTH: usize = 8;
    pub const RATE: usize = 9;
    pub const DENSITY: usize = 10;
    pub const SYNC: usize = 11;
    pub const GRAIN_ATTACK: usize = 12;
    pub const GRAIN_DECAY: usize = 13;
    pub const GRAIN_CURVE: usize = 14;
    pub const RANDOM_POSITION: usize = 15;
    pub const RANDOM_TIMING: usize = 16;
    pub const RANDOM_PITCH: usize = 17;
    pub const RANDOM_LEVEL: usize = 18;
    pub const RANDOM_PAN: usize = 19;
    pub const RANDOM_REVERSE: usize = 20;
    pub const CHORD_RANGE: usize = 21;
    pub const ROOT: usize = 22;
    pub const LENGTH_TRACK: usize = 23;
    // Effects and outputs number their settings from 0.
    pub const DRIVE: usize = 0;
    pub const BIAS: usize = 1;
    pub const STEREO: usize = 2;
    pub const MIX: usize = 3;
    pub const CUTOFF: usize = 0;
    pub const Q: usize = 1;
    pub const GAIN: usize = 2;
    pub const PAN: usize = 1;
    pub const ATTACK: usize = 2;
    pub const HOLD: usize = 3;
    pub const DECAY: usize = 4;
    pub const SUSTAIN: usize = 5;
    pub const RELEASE: usize = 6;
    /// Nonlinear filter (also CUTOFF, Q).
    pub const NL_DRIVE: usize = 2;
    /// Curve output (also LEVEL, PAN).
    pub const CURVE_LENGTH: usize = 2;
    pub const CURVE_START: usize = 3;
    pub const CURVE_END: usize = 4;
}

/// What slot `index` of a `kind` module means, in real units: `info.denormalize`
/// turns the stored 0..1 value into the setting. None for unused slots.
pub fn module_slot(kind: ModuleKind, index: usize) -> Option<ParamInfo> {
    use slot::*;
    use ModuleKind as K;
    let generator = |i: usize| match i {
        LEVEL => Some(ParamInfo::linear("Level", 0.0, 2.0, 1.0, Percent)),
        SEMI => Some(ParamInfo::linear("Semi", -48.0, 48.0, 0.0, Semitones)),
        CENT => Some(ParamInfo::linear("Cent", -100.0, 100.0, 0.0, Plain)),
        HARMONIC => Some(ParamInfo::linear("Harmonic", 0.0, 16.0, 1.0, Times)),
        SHIFT => Some(ParamInfo::linear("Shift", -1000.0, 1000.0, 0.0, Hertz)),
        PHASE => Some(ParamInfo::linear("Phase", 0.0, 360.0, 0.0, Degrees)),
        PHASE_RANDOM => Some(ParamInfo::linear("Random", 0.0, 180.0, 0.0, Degrees)),
        _ => None,
    };
    let unison = |i: usize| match i {
        UNISON => Some(ParamInfo::linear("Voices", 1.0, 8.0, 1.0, Count)),
        DETUNE => Some(ParamInfo::linear("Detune", 0.0, 1.0, 0.25, Percent)),
        SPREAD => Some(ParamInfo::linear("Spread", 0.0, 1.0, 1.0, Percent)),
        BLEND => Some(ParamInfo::linear("Blend", 0.0, 1.0, 1.0, Percent)),
        _ => None,
    };
    let envelope = |i: usize| match i {
        ATTACK => Some(ParamInfo::log("Attack", 0.0005, 10.0, 0.002, Seconds)),
        HOLD => Some(ParamInfo::linear("Hold", 0.0, 5.0, 0.0, Seconds)),
        DECAY => Some(ParamInfo::log("Decay", 0.001, 10.0, 1.0, Seconds)),
        SUSTAIN => Some(ParamInfo::linear("Sustain", 0.0, 1.0, 1.0, Percent)),
        RELEASE => Some(ParamInfo::log("Release", 0.001, 10.0, 0.15, Seconds)),
        _ => None,
    };
    match (kind, index) {
        (K::Analog, A) => Some(ParamInfo::linear("Sync", 1.0, 16.0, 1.0, Times)),
        (K::Analog, B) => Some(ParamInfo::linear("PW", 0.02, 0.98, 0.5, Percent)),
        (K::Wavetable, A) => Some(ParamInfo::linear("Frame", 0.0, 255.0, 0.0, Count)),
        (K::Wavetable, BANDLIMIT) => Some(ParamInfo::log("Bandlimit", 100.0, 22_000.0, 22_000.0, Hertz)),
        (K::Sample, A) => Some(ParamInfo::linear("Offset", 0.0, 1.0, 0.0, Percent)),
        (K::Sample, B) => Some(ParamInfo::linear("Root", 0.0, 127.0, 60.0, Note)),
        (K::Sample, LOOP_START) => Some(ParamInfo::linear("Loop Start", 0.0, 1.0, 0.0, Percent)),
        (K::Sample, LOOP_LENGTH) => Some(ParamInfo::linear("Length", 0.0, 1.0, 1.0, Percent)),
        (K::Sample, XFADE) => Some(ParamInfo::linear("X-Fade", 0.0, 0.5, 0.0, Percent)),
        (K::Noise, SLOPE) => Some(ParamInfo::linear("Slope", -6.0, 6.0, 0.0, Decibels)),
        (K::Noise, NOISE_STEREO) => Some(ParamInfo::linear("Stereo", 0.0, 1.0, 1.0, Percent)),
        (K::Granular, POSITION) => Some(ParamInfo::linear("Position", 0.0, 1.0, 0.0, Percent)),
        (K::Granular, LENGTH) => Some(ParamInfo::log("Length", 5.0, 2000.0, 100.0, Milliseconds)),
        (K::Granular, RATE) => Some(ParamInfo::log("Rate", 0.5, 500.0, 30.0, Hertz)),
        (K::Granular, DENSITY) => Some(ParamInfo::linear("Density", 1.0, 16.0, 4.0, Count)),
        (K::Granular, SYNC) => Some(ParamInfo::linear("Sync", 0.0, 7.0, 5.0, Count)),
        (K::Granular, GRAIN_ATTACK) => Some(ParamInfo::linear("Attack", 0.0, 1.0, 0.5, Percent)),
        (K::Granular, GRAIN_DECAY) => Some(ParamInfo::linear("Decay", 0.0, 1.0, 0.5, Percent)),
        (K::Granular, GRAIN_CURVE) => Some(ParamInfo::linear("Curve", -1.0, 1.0, 0.0, Plain)),
        (K::Granular, RANDOM_POSITION) => Some(ParamInfo::linear("Rnd Pos", 0.0, 1.0, 0.0, Percent)),
        (K::Granular, RANDOM_TIMING) => Some(ParamInfo::linear("Rnd Time", 0.0, 1.0, 0.0, Percent)),
        (K::Granular, RANDOM_PITCH) => Some(ParamInfo::linear("Rnd Pitch", 0.0, 24.0, 0.0, Semitones)),
        (K::Granular, RANDOM_LEVEL) => Some(ParamInfo::linear("Rnd Level", 0.0, 1.0, 0.0, Percent)),
        (K::Granular, RANDOM_PAN) => Some(ParamInfo::linear("Rnd Pan", 0.0, 1.0, 0.0, Percent)),
        (K::Granular, RANDOM_REVERSE) => Some(ParamInfo::linear("Reverse", 0.0, 1.0, 0.0, Percent)),
        (K::Granular, CHORD_RANGE) => Some(ParamInfo::linear("Chord Range", 0.0, 4.0, 1.0, Octaves)),
        (K::Granular, ROOT) => Some(ParamInfo::linear("Root", 0.0, 127.0, 60.0, Note)),
        (K::Granular, LENGTH_TRACK) => Some(ParamInfo::linear("Len Track", 0.0, 1.0, 0.0, Percent)),
        (k, i) if k.is_generator() && i < A => generator(i),
        (k, i) if k.has_unison() && i >= UNISON => unison(i),
        (K::Distortion, DRIVE) => Some(ParamInfo::linear("Drive", 0.0, 48.0, 12.0, Decibels)),
        (K::Distortion, BIAS) => Some(ParamInfo::linear("Bias", -1.0, 1.0, 0.0, Plain)),
        (K::Distortion, STEREO) => Some(ParamInfo::linear("Spread", 0.0, 1.0, 0.0, Percent)),
        (K::Distortion, MIX) => Some(ParamInfo::linear("Mix", 0.0, 1.0, 1.0, Percent)),
        (K::Filter, CUTOFF) => Some(ParamInfo::log("Cutoff", 20.0, 20_000.0, 1000.0, Hertz)),
        (K::Filter, Q) => Some(ParamInfo::log("Q", 0.1, 20.0, 0.707, Plain)),
        (K::Filter, GAIN) => Some(ParamInfo::linear("Gain", -24.0, 24.0, 0.0, Decibels)),
        (K::NonlinearFilter, CUTOFF) => Some(ParamInfo::log("Cutoff", 20.0, 20_000.0, 1000.0, Hertz)),
        (K::NonlinearFilter, Q) => Some(ParamInfo::log("Q", 0.5, 25.0, 0.707, Plain)),
        (K::NonlinearFilter, NL_DRIVE) => Some(ParamInfo::linear("Drive", 0.0, 36.0, 6.0, Decibels)),
        (K::CurveOutput, LEVEL) => Some(ParamInfo::linear("Gain", 0.0, 2.0, 1.0, Percent)),
        (K::CurveOutput, PAN) => Some(ParamInfo::linear("Pan", -1.0, 1.0, 0.0, Pan)),
        (K::CurveOutput, CURVE_LENGTH) => Some(ParamInfo::log("Length", 0.01, 30.0, 1.0, Seconds)),
        (K::CurveOutput, CURVE_START) => Some(ParamInfo::linear("Loop Start", 0.0, 1.0, 0.0, Percent)),
        (K::CurveOutput, CURVE_END) => Some(ParamInfo::linear("Loop End", 0.0, 1.0, 1.0, Percent)),
        (K::Mix | K::Aux, LEVEL) => Some(ParamInfo::linear("Level", 0.0, 2.0, 1.0, Percent)),
        (K::Output, LEVEL) => Some(ParamInfo::linear("Gain", 0.0, 2.0, 1.0, Percent)),
        (K::Output, PAN) => Some(ParamInfo::linear("Pan", -1.0, 1.0, 0.0, Pan)),
        (K::Output, i) => envelope(i),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Module {
    pub kind: ModuleKind,
    pub on: bool,
    /// Waveform, effect type, filter slope or output send, by kind.
    pub choice: [u8; 2],
    /// Sample: loop (first layout; now `choice[0]`). Granular: warm start.
    /// Mix/Aux: invert. Output, Curve Output: send out (off: only a modulation source).
    pub flag: bool,
    /// Wavetable: built-in table, or [`WAVE_USER_TABLE`] for the module's file.
    /// Granular: chord picking pattern ([`CHORD_PICKS`]).
    pub table: u8,
    pub values: [f32; MOD_SLOTS],
    /// Slots 16..24.
    #[serde(default)]
    pub extra: [f32; MOD_EXTRA_SLOTS],
    /// Granular: align grains to the root's phase.
    #[serde(default)]
    pub alt: bool,
    /// Curve Output: its level over time.
    #[serde(default = "Shape::falling")]
    pub curve: Shape,
}

impl Module {
    pub const EMPTY: Module = Module {
        kind: ModuleKind::Empty,
        on: true,
        choice: [0; 2],
        flag: false,
        table: 0,
        values: [0.0; MOD_SLOTS],
        extra: [0.0; MOD_EXTRA_SLOTS],
        alt: false,
        curve: Shape { points: [ShapePoint { time: 0.0, value: 0.0, bend: 0.0 }; SHAPE_POINTS], count: 0 },
    };

    /// A module of `kind` with every slot at its default.
    pub fn new(kind: ModuleKind) -> Self {
        let mut module = Module { kind, flag: kind.is_output(), curve: Shape::falling(), ..Self::EMPTY };
        for i in 0..MOD_MODULE_SLOTS {
            if let Some(info) = module_slot(kind, i) {
                *module.value_mut(i) = info.normalize(info.default);
            }
        }
        match kind {
            // Infinite loop.
            ModuleKind::Sample => module.choice[0] = 1,
            ModuleKind::CurveOutput => module.curve = Shape::from_points(&[(0.0, 0.0, 0.0), (0.02, 1.0, 0.0), (1.0, 0.0, 0.6)]),
            _ => {}
        }
        module
    }

    /// Slot `i`, normalised.
    pub fn value(&self, i: usize) -> f32 {
        if i < MOD_SLOTS { self.values[i] } else { self.extra.get(i - MOD_SLOTS).copied().unwrap_or(0.0) }
    }

    pub fn value_mut(&mut self, i: usize) -> &mut f32 {
        if i < MOD_SLOTS { &mut self.values[i] } else { &mut self.extra[(i - MOD_SLOTS).min(MOD_EXTRA_SLOTS - 1)] }
    }

    /// Slot `i` in real units.
    pub fn get(&self, i: usize) -> f32 {
        module_slot(self.kind, i).map_or(0.0, |info| info.denormalize(self.value(i)))
    }

    /// Sets slot `i` from a value in real units.
    pub fn set(&mut self, i: usize, value: f32) {
        if let Some(info) = module_slot(self.kind, i) {
            *self.value_mut(i) = info.normalize(value);
        }
    }

    /// The sample's loop mode (an index into [`SAMPLE_LOOPS`]), reading the first layout's flag too.
    pub fn sample_loop(&self) -> u8 {
        if self.choice[0] == 0 && self.flag { 1 } else { self.choice[0] }
    }
}

/// What an audio-rate connection modulates on its target module.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioParam {
    /// Classic FM: moves the phase, never detunes.
    #[default]
    Phase,
    /// Linear FM, proportional to the frequency: the same timbre at every pitch.
    Harmonic,
    /// Linear FM in Hz.
    Shift,
    /// Ring modulation: multiplies the target by the source.
    Level,
    /// Exponential FM in semitones (vibrato from slow sources).
    Pitch,
    /// Into an Aux module.
    Input,
}

impl AudioParam {
    pub const ALL: [AudioParam; 6] = [Self::Phase, Self::Harmonic, Self::Shift, Self::Level, Self::Pitch, Self::Input];

    pub fn label(self) -> &'static str {
        match self {
            Self::Phase => "Phase",
            Self::Harmonic => "Harmonic",
            Self::Shift => "Shift",
            Self::Level => "Level",
            Self::Pitch => "Pitch",
            Self::Input => "Input",
        }
    }
}

/// An audio-rate connection from one module's output to another's setting.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioMod {
    pub source: u8,
    pub target: u8,
    pub param: AudioParam,
    /// -1..1.
    pub amount: f32,
}

impl AudioMod {
    pub const NONE: AudioMod = AudioMod { source: MOD_NO_MODULE, target: MOD_NO_MODULE, param: AudioParam::Phase, amount: 0.0 };
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lane {
    pub gain: f32,
    #[serde(deserialize_with = "padded_lane_fx")]
    pub fx: [Option<Device>; MOD_LANE_FX],
    /// Where the lane goes: [`MOD_NO_LANE`] is the master, otherwise a later lane.
    pub dest: u8,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    /// Effects run once per voice (only from the first lane on, without gaps).
    #[serde(default)]
    pub poly: bool,
    /// Dry/wet, 0..1.
    #[serde(default = "full")]
    pub mix: f32,
}

fn full() -> f32 {
    1.0
}

/// Reads up to [`MOD_LANE_FX`] effects (lanes from the first layout have 4).
fn padded_lane_fx<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<[Option<Device>; MOD_LANE_FX], D::Error> {
    let list: Vec<Option<Device>> = Vec::deserialize(deserializer)?;
    let mut fx = [None; MOD_LANE_FX];
    for (slot, d) in fx.iter_mut().zip(list) {
        *slot = d;
    }
    Ok(fx)
}

impl Lane {
    pub const EMPTY: Lane = Lane { gain: 1.0, fx: [None; MOD_LANE_FX], dest: MOD_NO_LANE, mute: false, solo: false, poly: false, mix: 1.0 };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModulatorKind {
    #[default]
    Off,
    /// Delay, attack, hold, decay, sustain, release.
    Envelope,
    /// A drawn cycle, free or synced.
    Lfo,
    /// Scans one frame of a wavetable as its cycle.
    LfoTable,
    /// A drawn curve played over time, with loop points.
    Timeline,
    /// Random steps with jitter, smoothing and chaos.
    Random,
    /// The level of an effect lane or the master.
    AudioFollower,
    /// The pitch of an effect lane or the master.
    PitchTracker,
    Note,
    /// Velocity, note or a random value per note, mapped through a drawn curve.
    Curve,
    Velocity,
    /// 1 while the note is held.
    NoteGate,
    /// Channel or polyphonic aftertouch.
    Pressure,
    PitchWheel,
    MidiCc,
    /// One of the eight macros.
    Macro,
    /// Another modulator through a drawn curve.
    Remap,
    /// Another modulator, kept above a limit.
    LowerLimit,
    /// Another modulator, kept below a limit.
    UpperLimit,
    /// Another modulator, multiplied.
    Scale,
    /// Another modulator, sampled when the trigger rises.
    SampleHold,
}

impl ModulatorKind {
    pub const ALL: [ModulatorKind; 20] = [
        Self::Envelope,
        Self::Lfo,
        Self::LfoTable,
        Self::Timeline,
        Self::Random,
        Self::AudioFollower,
        Self::PitchTracker,
        Self::Note,
        Self::Curve,
        Self::Velocity,
        Self::NoteGate,
        Self::Pressure,
        Self::PitchWheel,
        Self::MidiCc,
        Self::Macro,
        Self::Remap,
        Self::LowerLimit,
        Self::UpperLimit,
        Self::Scale,
        Self::SampleHold,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "—",
            Self::Envelope => "Envelope",
            Self::Lfo => "LFO",
            Self::LfoTable => "LFO Table",
            Self::Timeline => "Curve",
            Self::Random => "Random",
            Self::AudioFollower => "Audio Follower",
            Self::PitchTracker => "Pitch Tracker",
            Self::Note => "Note",
            Self::Curve => "Note Curve",
            Self::Velocity => "Velocity",
            Self::NoteGate => "Note Gate",
            Self::Pressure => "Pressure",
            Self::PitchWheel => "Pitch Wheel",
            Self::MidiCc => "MIDI CC",
            Self::Macro => "Macro",
            Self::Remap => "Remap",
            Self::LowerLimit => "Lower Limit",
            Self::UpperLimit => "Upper Limit",
            Self::Scale => "Scale",
            Self::SampleHold => "Sample & Hold",
        }
    }

    /// Reads another modulator (its `source`).
    pub fn takes_input(self) -> bool {
        matches!(self, Self::Remap | Self::LowerLimit | Self::UpperLimit | Self::Scale | Self::SampleHold)
    }

    /// Can be (re)started by notes or by a trigger input.
    pub fn triggered(self) -> bool {
        matches!(self, Self::Envelope | Self::Lfo | Self::LfoTable | Self::Timeline | Self::Random | Self::SampleHold)
    }

    /// The same for every voice: computed once per block.
    pub fn is_global(self) -> bool {
        matches!(self, Self::AudioFollower | Self::PitchTracker | Self::PitchWheel | Self::MidiCc)
    }
}

/// When notes restart a modulator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerMode {
    /// Never with a trigger input connected; otherwise Legato in legato mode, Always elsewhere.
    #[default]
    Auto,
    /// Free running.
    Never,
    /// Every note-on.
    Always,
    /// The first note of a legato phrase only.
    Legato,
}

impl TriggerMode {
    pub const ALL: [TriggerMode; 4] = [Self::Auto, Self::Never, Self::Always, Self::Legato];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Never => "Never",
            Self::Always => "Always",
            Self::Legato => "Legato",
        }
    }
}

/// How a modulator's 0..1 value reaches its targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputRange {
    /// 0..1.
    Unipolar,
    /// -1..1.
    Bipolar,
    /// 1..0.
    Inverted,
}

impl OutputRange {
    pub const ALL: [OutputRange; 3] = [Self::Unipolar, Self::Bipolar, Self::Inverted];

    pub fn label(self) -> &'static str {
        match self {
            Self::Unipolar => "Unipolar",
            Self::Bipolar => "Bipolar",
            Self::Inverted => "Inverted",
        }
    }

    pub fn apply(self, value: f32) -> f32 {
        match self {
            Self::Unipolar => value,
            Self::Bipolar => value * 2.0 - 1.0,
            Self::Inverted => 1.0 - value,
        }
    }
}

/// Audio Follower and Pitch Tracker inputs.
pub const MOD_LISTEN: [&str; 5] = ["Lane 1", "Lane 2", "Lane 3", "Master", "Sideband"];
/// Scale modulator multipliers.
pub const SCALE_MULTIPLIERS: [f32; 5] = [0.01, 0.1, 1.0, 10.0, 100.0];

/// What a curve modulator reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurveInput {
    #[default]
    Velocity,
    Note,
    Random,
}

impl CurveInput {
    pub const ALL: [CurveInput; 3] = [Self::Velocity, Self::Note, Self::Random];

    pub fn label(self) -> &'static str {
        match self {
            Self::Velocity => "Velocity",
            Self::Note => "Note",
            Self::Random => "Random",
        }
    }
}

/// Modulator settings: each kind uses some of the eight 0..1 slots (see [`modulator_slot`]).
pub mod mslot {
    pub const RATE: usize = 0;
    pub const PHASE: usize = 1;
    pub const FRAME: usize = 2;
    pub const SMOOTH_TABLE: usize = 3;
    pub const ATTACK: usize = 0;
    pub const DECAY: usize = 1;
    pub const SUSTAIN: usize = 2;
    pub const RELEASE: usize = 3;
    pub const DELAY: usize = 4;
    pub const HOLD: usize = 6;
    pub const SMOOTH: usize = 1;
    pub const JITTER: usize = 4;
    pub const CHAOS: usize = 5;
    pub const LENGTH: usize = 0;
    pub const LOOP_START: usize = 1;
    pub const LOOP_END: usize = 2;
    pub const GAIN: usize = 0;
    pub const FOLLOW_ATTACK: usize = 1;
    pub const FOLLOW_RELEASE: usize = 2;
    pub const LOW: usize = 0;
    pub const HIGH: usize = 1;
    pub const SENSITIVITY: usize = 2;
    pub const LIMIT: usize = 0;
    pub const FACTOR: usize = 0;
    pub const THRESHOLD: usize = 0;
    /// Output depth, for every kind.
    pub const DEPTH: usize = 7;
}

/// What slot `index` of a `kind` modulator means, or None when it is unused.
pub fn modulator_slot(kind: ModulatorKind, index: usize) -> Option<ParamInfo> {
    use mslot::*;
    use ModulatorKind as K;
    if kind == K::Off {
        return None;
    }
    if index == DEPTH {
        return Some(ParamInfo::linear("Depth", 0.0, 1.0, 1.0, Percent));
    }
    let rate = ParamInfo::log("Rate", 0.02, 40.0, 1.0, Hertz);
    match (kind, index) {
        (K::Envelope, ATTACK) => Some(ParamInfo::log("Attack", 0.0005, 10.0, 0.01, Seconds)),
        (K::Envelope, DECAY) => Some(ParamInfo::log("Decay", 0.001, 10.0, 0.1, Seconds)),
        (K::Envelope, SUSTAIN) => Some(ParamInfo::linear("Sustain", 0.0, 1.0, 0.0, Percent)),
        (K::Envelope, RELEASE) => Some(ParamInfo::log("Release", 0.001, 10.0, 0.1, Seconds)),
        (K::Envelope, DELAY) => Some(ParamInfo::linear("Delay", 0.0, 4.0, 0.0, Seconds)),
        (K::Envelope, HOLD) => Some(ParamInfo::linear("Hold", 0.0, 4.0, 0.0, Seconds)),
        (K::Lfo | K::LfoTable, RATE) => Some(rate),
        (K::Lfo | K::LfoTable, PHASE) => Some(ParamInfo::linear("Phase", 0.0, 1.0, 0.0, Percent)),
        (K::LfoTable, FRAME) => Some(ParamInfo::linear("Frame", 0.0, 1.0, 0.0, Percent)),
        (K::LfoTable, SMOOTH_TABLE) => Some(ParamInfo::linear("Smooth", 0.0, 1.0, 0.0, Percent)),
        (K::Timeline, LENGTH) => Some(ParamInfo::log("Length", 0.01, 30.0, 1.0, Seconds)),
        (K::Timeline, LOOP_START) => Some(ParamInfo::linear("Loop Start", 0.0, 1.0, 0.0, Percent)),
        (K::Timeline, LOOP_END) => Some(ParamInfo::linear("Loop End", 0.0, 1.0, 1.0, Percent)),
        (K::Random, RATE) => Some(ParamInfo::log("Rate", 0.1, 40.0, 2.0, Hertz)),
        (K::Random, SMOOTH) => Some(ParamInfo::linear("Smooth", 0.0, 1.0, 0.0, Percent)),
        (K::Random, JITTER) => Some(ParamInfo::linear("Jitter", 0.0, 1.0, 0.0, Percent)),
        (K::Random, CHAOS) => Some(ParamInfo::linear("Chaos", 0.0, 1.0, 1.0, Percent)),
        (K::AudioFollower, GAIN) => Some(ParamInfo::linear("Gain", -24.0, 24.0, 0.0, Decibels)),
        (K::AudioFollower, FOLLOW_ATTACK) => Some(ParamInfo::log("Attack", 0.0001, 1.0, 0.005, Seconds)),
        (K::AudioFollower, FOLLOW_RELEASE) => Some(ParamInfo::log("Release", 0.001, 5.0, 0.15, Seconds)),
        (K::PitchTracker, LOW) => Some(ParamInfo::linear("Low", 0.0, 127.0, 36.0, Note)),
        (K::PitchTracker, HIGH) => Some(ParamInfo::linear("High", 0.0, 127.0, 84.0, Note)),
        (K::PitchTracker, SENSITIVITY) => Some(ParamInfo::linear("Sensitivity", 0.0, 1.0, 0.5, Percent)),
        (K::LowerLimit | K::UpperLimit, LIMIT) => Some(ParamInfo::linear("Limit", 0.0, 1.0, 0.5, Percent)),
        (K::Scale, FACTOR) => Some(ParamInfo::linear("Factor", -1.0, 1.0, 1.0, Plain)),
        (K::SampleHold, THRESHOLD) => Some(ParamInfo::linear("Threshold", 0.0, 1.0, 0.5, Percent)),
        _ => None,
    }
}

/// One connection from a modulator to a parameter.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModTarget {
    pub dest: u16,
    /// -1..1 of the parameter's range.
    pub depth: f32,
    /// Off: kept but not applied.
    #[serde(default = "yes")]
    pub on: bool,
    /// Bends the modulator's value before it is applied (-1..1, 0: straight).
    #[serde(default)]
    pub curve: f32,
    /// Added to the parameter whatever the modulator does; with the depth it sets
    /// the lower and upper bounds the modulation moves between.
    #[serde(default)]
    pub offset: f32,
}

fn no_modulator() -> u8 {
    MOD_NO_MODULATOR
}

impl ModTarget {
    pub const NONE: ModTarget = ModTarget { dest: MOD_NO_TARGET, depth: 0.0, on: true, curve: 0.0, offset: 0.0 };

    pub fn new(dest: usize, depth: f32) -> Self {
        ModTarget { dest: dest as u16, depth, ..Self::NONE }
    }

    /// How far (in 0..1 of the parameter's travel) a modulator value `value` moves the target.
    pub fn amount(&self, value: f32) -> f32 {
        if !self.on {
            return 0.0;
        }
        let a = value.abs();
        let bent = if a <= 1.0 && self.curve != 0.0 { Shape::ease(a, self.curve) } else { a };
        self.offset + self.depth * bent.copysign(value)
    }

    /// The lowest and highest amount for a modulator with output `range`.
    pub fn bounds(&self, range: OutputRange) -> (f32, f32) {
        let low = if range == OutputRange::Bipolar { -1.0 } else { 0.0 };
        let (a, b) = (self.amount(low), self.amount(1.0));
        (a.min(b), a.max(b))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Modulator {
    pub kind: ModulatorKind,
    /// LFO and Curve: the cycle; Note Curve and Remap: the mapping.
    pub shape: Shape,
    pub mode: LfoMode,
    /// LFO period, or Curve length, in beats when synced (0: free rate).
    pub sync_beats: f32,
    pub input: CurveInput,
    /// Macro, MIDI CC number, LFO Table's table, Follower and Tracker input, Scale multiplier.
    pub index: u8,
    pub bipolar: bool,
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub targets: [ModTarget; MOD_TARGETS],
    /// Slots 4..8 (7 is the output depth).
    #[serde(default = "Modulator::more_default")]
    pub more: [f32; 4],
    /// Output from 1 down to 0 (wins over `bipolar`).
    #[serde(default)]
    pub inverted: bool,
    #[serde(default)]
    pub trigger: TriggerMode,
    /// A modulator whose rises restart this one (and whose falls release it).
    #[serde(default = "no_modulator")]
    pub trigger_source: u8,
    /// The modulator that Remap, the limits, Scale and Sample & Hold read.
    #[serde(default = "no_modulator")]
    pub source: u8,
    /// Random: one sequence for all voices (Unison) instead of one each.
    #[serde(default)]
    pub shared: bool,
    /// Envelope: a retrigger starts from the current level instead of zero.
    #[serde(default = "yes")]
    pub seamless: bool,
    /// Curve: an index into [`CURVE_LOOPS`].
    #[serde(default)]
    pub loop_mode: u8,
}

impl Modulator {
    pub const OFF: Modulator = Modulator {
        kind: ModulatorKind::Off,
        shape: Shape { points: [ShapePoint { time: 0.0, value: 0.0, bend: 0.0 }; SHAPE_POINTS], count: 0 },
        mode: LfoMode::Free,
        sync_beats: 0.0,
        input: CurveInput::Velocity,
        index: 0,
        bipolar: false,
        a: 0.5,
        b: 0.5,
        c: 0.5,
        d: 0.5,
        targets: [ModTarget::NONE; MOD_TARGETS],
        more: Self::MORE,
        inverted: false,
        trigger: TriggerMode::Auto,
        trigger_source: MOD_NO_MODULATOR,
        source: MOD_NO_MODULATOR,
        shared: false,
        seamless: false,
        loop_mode: 0,
    };

    /// Slots 4..8 of modulators saved before they existed: no delay or jitter, full chaos and depth.
    const MORE: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

    fn more_default() -> [f32; 4] {
        Self::MORE
    }

    /// A new modulator of `kind` with every setting at its default.
    pub fn new(kind: ModulatorKind) -> Self {
        let mut m = Self { kind, ..Self::OFF };
        for i in 0..MOD_MODULATOR_SLOTS {
            if let Some(info) = modulator_slot(kind, i) {
                *m.value_mut(i) = info.normalize(info.default);
            }
        }
        let triangle = Shape::from_points(&[(0.0, 0.5, 0.0), (0.25, 1.0, 0.0), (0.75, 0.0, 0.0), (1.0, 0.5, 0.0)]);
        let rising = Shape::from_points(&[(0.0, 0.0, 0.0), (1.0, 1.0, 0.0)]);
        match kind {
            ModulatorKind::Envelope => {
                m.a = 0.1;
                m.b = 0.4;
                m.c = 0.0;
                m.d = 0.3;
            }
            ModulatorKind::Lfo => {
                m.shape = triangle;
                m.a = 0.45;
            }
            ModulatorKind::LfoTable => m.a = 0.45,
            ModulatorKind::Timeline => m.shape = Shape::from_points(&[(0.0, 0.0, 0.0), (0.1, 1.0, 0.0), (1.0, 0.0, 0.5)]),
            ModulatorKind::Random => m.a = 0.5,
            ModulatorKind::Curve | ModulatorKind::Remap => m.shape = rising,
            ModulatorKind::AudioFollower | ModulatorKind::PitchTracker => m.index = 3,
            ModulatorKind::MidiCc => m.index = 1,
            ModulatorKind::Scale => m.index = 2,
            ModulatorKind::PitchWheel => m.bipolar = true,
            _ => {}
        }
        m
    }

    pub fn range(&self) -> OutputRange {
        if self.inverted {
            OutputRange::Inverted
        } else if self.bipolar {
            OutputRange::Bipolar
        } else {
            OutputRange::Unipolar
        }
    }

    pub fn set_range(&mut self, range: OutputRange) {
        self.inverted = range == OutputRange::Inverted;
        self.bipolar = range == OutputRange::Bipolar;
    }

    /// Slot `i`, normalised.
    pub fn value(&self, i: usize) -> f32 {
        match i {
            0 => self.a,
            1 => self.b,
            2 => self.c,
            3 => self.d,
            _ => self.more[(i - 4).min(3)],
        }
    }

    pub fn value_mut(&mut self, i: usize) -> &mut f32 {
        match i {
            0 => &mut self.a,
            1 => &mut self.b,
            2 => &mut self.c,
            3 => &mut self.d,
            _ => &mut self.more[(i - 4).min(3)],
        }
    }

    /// Slot `i` in real units.
    pub fn get(&self, i: usize) -> f32 {
        modulator_slot(self.kind, i).map_or(0.0, |info| info.denormalize(self.value(i)))
    }

    /// Sets slot `i` from a value in real units.
    pub fn set(&mut self, i: usize, value: f32) {
        if let Some(info) = modulator_slot(self.kind, i) {
            *self.value_mut(i) = info.normalize(value);
        }
    }

    /// When notes restart it, given the synth's voicing.
    pub fn note_trigger(&self, legato: bool) -> TriggerMode {
        match self.trigger {
            TriggerMode::Auto if self.trigger_source != MOD_NO_MODULATOR => TriggerMode::Never,
            TriggerMode::Auto if legato => TriggerMode::Legato,
            TriggerMode::Auto => TriggerMode::Always,
            mode => mode,
        }
    }
}

/// Reads up to [`MOD_MODULATORS`] modulators (patches from the first layout have 16).
fn padded_modulators<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<[Modulator; MOD_MODULATORS], D::Error> {
    let list: Vec<Modulator> = Vec::deserialize(deserializer)?;
    let mut modulators = [Modulator::OFF; MOD_MODULATORS];
    for (slot, m) in modulators.iter_mut().zip(list) {
        *slot = m;
    }
    Ok(modulators)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModularParams {
    pub modules: [Module; MOD_MODULES],
    pub audio: [AudioMod; MOD_AUDIO],
    pub lanes: [Lane; MOD_LANES],
    #[serde(deserialize_with = "padded_modulators")]
    pub modulators: [Modulator; MOD_MODULATORS],
    pub macros: [f32; MOD_MACROS],
    pub mono: bool,
    pub legato: bool,
    pub glide: f32,
    pub gain: f32,
    #[serde(default)]
    pub unison: Unison,
    /// Notes that can sound at once (each takes one voice per unison voice).
    #[serde(default = "default_polyphony")]
    pub polyphony: u8,
    /// Pitch wheel range in semitones.
    #[serde(default = "default_bend_range")]
    pub bend_range: u8,
    /// Master pitch (semitones) and fine tuning (cents).
    #[serde(default)]
    pub pitch: f32,
    #[serde(default)]
    pub fine: f32,
}

fn default_polyphony() -> u8 {
    12
}

fn default_bend_range() -> u8 {
    2
}

impl Default for ModularParams {
    fn default() -> Self {
        let mut modules = [Module::EMPTY; MOD_MODULES];
        modules[0] = Module::new(ModuleKind::Group);
        modules[1] = Module::new(ModuleKind::Analog);
        modules[2] = Module::new(ModuleKind::Output);
        Self {
            modules,
            audio: [AudioMod::NONE; MOD_AUDIO],
            lanes: [Lane::EMPTY; MOD_LANES],
            modulators: [Modulator::OFF; MOD_MODULATORS],
            macros: [0.0; MOD_MACROS],
            mono: false,
            legato: false,
            glide: 0.0,
            gain: 0.35,
            unison: Unison::default(),
            polyphony: default_polyphony(),
            bend_range: default_bend_range(),
            pitch: 0.0,
            fine: 0.0,
        }
    }
}

impl ModularParams {
    /// Parameter index of slot `slot` of module `module`.
    pub fn module_param(module: usize, slot: usize) -> usize {
        if slot < MOD_SLOTS { module * MOD_SLOTS + slot } else { MOD_EXTRA + module * MOD_EXTRA_SLOTS + slot - MOD_SLOTS }
    }

    /// The module and slot behind parameter `index`, if it is a module slot.
    pub fn param_module(index: usize) -> Option<(usize, usize)> {
        if index < MOD_AUDIO_AMOUNT {
            Some((index / MOD_SLOTS, index % MOD_SLOTS))
        } else if (MOD_EXTRA..MOD_UNISON).contains(&index) {
            Some(((index - MOD_EXTRA) / MOD_EXTRA_SLOTS, MOD_SLOTS + (index - MOD_EXTRA) % MOD_EXTRA_SLOTS))
        } else {
            None
        }
    }

    /// Parameter index of slot `i` of modulator `m`.
    pub fn modulator_param(m: usize, i: usize) -> usize {
        if m < MOD_FIRST_MODULATORS && i < 4 { MOD_MODULATOR_PARAMS + m * 4 + i } else { MOD_MODULATOR_SLOTS_AT + m * MOD_MODULATOR_SLOTS + i }
    }

    /// The modulator and slot behind parameter `index`, if it is a modulator setting.
    pub fn param_modulator(index: usize) -> Option<(usize, usize)> {
        if (MOD_MODULATOR_PARAMS..MOD_MACRO_PARAMS).contains(&index) {
            Some(((index - MOD_MODULATOR_PARAMS) / 4, (index - MOD_MODULATOR_PARAMS) % 4))
        } else if (MOD_MODULATOR_SLOTS_AT..MOD_LANE_FX_PARAMS).contains(&index) {
            let k = index - MOD_MODULATOR_SLOTS_AT;
            Some((k / MOD_MODULATOR_SLOTS, k % MOD_MODULATOR_SLOTS))
        } else {
            None
        }
    }

    /// Moves modulation aimed at lane effect settings: `to` maps a lane and slot to
    /// their new place, or None when the effect goes (and its modulation with it).
    fn retarget_lane_fx(&mut self, to: impl Fn(usize, usize) -> Option<(usize, usize)>) {
        for modulator in &mut self.modulators {
            for target in &mut modulator.targets {
                if let Some((lane, slot, k)) = Self::param_lane_fx(target.dest as usize) {
                    *target = match to(lane, slot) {
                        Some((lane, slot)) => ModTarget { dest: Self::lane_fx_param(lane, slot, k) as u16, ..*target },
                        None => ModTarget::NONE,
                    };
                }
            }
        }
    }

    /// Takes the effect in slot `slot` of lane `l` out, closing the gap.
    pub fn remove_lane_fx(&mut self, l: usize, slot: usize) {
        let fx = &mut self.lanes[l].fx;
        fx.copy_within(slot + 1.., slot);
        fx[MOD_LANE_FX - 1] = None;
        self.retarget_lane_fx(|lane, s| match s {
            _ if lane != l || s < slot => Some((lane, s)),
            _ if s == slot => None,
            _ => Some((lane, s - 1)),
        });
    }

    /// Swaps the effect in slot `slot` of lane `l` with the one above (`up`) or below.
    pub fn move_lane_fx(&mut self, l: usize, slot: usize, up: bool) {
        let other = if up { slot.checked_sub(1) } else { Some(slot + 1).filter(|&o| o < MOD_LANE_FX) };
        let Some(other) = other.filter(|&o| self.lanes[l].fx[o].is_some()) else { return };
        self.lanes[l].fx.swap(slot, other);
        self.retarget_lane_fx(|lane, s| match s {
            _ if lane != l => Some((lane, s)),
            _ if s == slot => Some((lane, other)),
            _ if s == other => Some((lane, slot)),
            _ => Some((lane, s)),
        });
    }

    /// Takes modulator `m` out, with everything connected to it.
    pub fn remove_modulator(&mut self, m: usize) {
        self.modulators[m] = Modulator::OFF;
        for other in &mut self.modulators {
            if other.source as usize == m {
                other.source = MOD_NO_MODULATOR;
            }
            if other.trigger_source as usize == m {
                other.trigger_source = MOD_NO_MODULATOR;
            }
            for target in &mut other.targets {
                if Self::param_modulator(target.dest as usize).is_some_and(|(owner, _)| owner == m) {
                    *target = ModTarget::NONE;
                }
            }
        }
    }

    /// Parameter index of setting `k` of the effect in slot `slot` of lane `lane`.
    pub fn lane_fx_param(lane: usize, slot: usize, k: usize) -> usize {
        MOD_LANE_FX_PARAMS + (lane * MOD_LANE_FX + slot) * MOD_FX_SLOTS + k
    }

    /// The lane, effect slot and setting behind parameter `index`, if it is a lane effect's.
    pub fn param_lane_fx(index: usize) -> Option<(usize, usize, usize)> {
        if !(MOD_LANE_FX_PARAMS..MOD_LANE_MIX).contains(&index) {
            return None;
        }
        let k = index - MOD_LANE_FX_PARAMS;
        let (fx, setting) = (k / MOD_FX_SLOTS, k % MOD_FX_SLOTS);
        Some((fx / MOD_LANE_FX, fx % MOD_LANE_FX, setting))
    }

    /// The range of parameter `index`: lane effect settings have their effect's.
    pub fn param_info(&self, index: usize) -> Option<ParamInfo> {
        match Self::param_lane_fx(index) {
            Some((lane, slot, k)) => self.lanes[lane].fx[slot].as_ref()?.effect.params().get(k).copied(),
            None => MODULAR_PARAMS.get(index).copied(),
        }
    }

    /// Whether lane `l` runs its effects per voice: it and every lane before it are poly.
    pub fn lane_poly(&self, l: usize) -> bool {
        self.lanes[..=l.min(MOD_LANES - 1)].iter().all(|lane| lane.poly)
    }

    /// Whether lane `l` is heard: not muted, and soloed if any lane is.
    pub fn lane_audible(&self, l: usize) -> bool {
        let lane = &self.lanes[l];
        !lane.mute && (lane.solo || !self.lanes.iter().any(|other| other.solo))
    }

    pub fn param_mut(&mut self, index: usize) -> Option<&mut f32> {
        Some(match index {
            i if i < MOD_AUDIO_AMOUNT => &mut self.modules[i / MOD_SLOTS].values[i % MOD_SLOTS],
            i if i < MOD_LANE_GAIN => &mut self.audio[i - MOD_AUDIO_AMOUNT].amount,
            i if i < MOD_MODULATOR_PARAMS => &mut self.lanes[i - MOD_LANE_GAIN].gain,
            i if i < MOD_MACRO_PARAMS => {
                let m = &mut self.modulators[(i - MOD_MODULATOR_PARAMS) / 4];
                match (i - MOD_MODULATOR_PARAMS) % 4 {
                    0 => &mut m.a,
                    1 => &mut m.b,
                    2 => &mut m.c,
                    _ => &mut m.d,
                }
            }
            i if i < MOD_GLIDE => &mut self.macros[i - MOD_MACRO_PARAMS],
            MOD_GLIDE => &mut self.glide,
            MOD_MASTER => &mut self.gain,
            i if i < MOD_UNISON => {
                let k = i - MOD_EXTRA;
                &mut self.modules[k / MOD_EXTRA_SLOTS].extra[k % MOD_EXTRA_SLOTS]
            }
            i if i == MOD_UNISON => &mut self.unison.detune,
            i if i == MOD_UNISON + 1 => &mut self.unison.spread,
            i if i == MOD_UNISON + 2 => &mut self.unison.blend,
            i if i == MOD_UNISON + 3 => &mut self.unison.bias,
            i if i < MOD_LANE_FX_PARAMS => {
                let k = i - MOD_MODULATOR_SLOTS_AT;
                self.modulators[k / MOD_MODULATOR_SLOTS].value_mut(k % MOD_MODULATOR_SLOTS)
            }
            i if i < MOD_LANE_MIX => {
                let (lane, slot, k) = Self::param_lane_fx(i)?;
                return self.lanes[lane].fx[slot].as_mut()?.effect.param_mut(k);
            }
            i if i < MOD_PITCH => &mut self.lanes[i - MOD_LANE_MIX].mix,
            MOD_PITCH => &mut self.pitch,
            MOD_FINE => &mut self.fine,
            _ => return None,
        })
    }

    /// A readable name for parameter `index`, e.g. "3 Analog · Level".
    pub fn param_name(&self, index: usize) -> String {
        if let Some((m, s)) = Self::param_module(index) {
            let module = &self.modules[m];
            let name = module_slot(module.kind, s).map_or("?", |i| i.name);
            return format!("{} {} · {name}", m + 1, module.kind.label());
        }
        if let Some((m, k)) = Self::param_modulator(index) {
            let modulator = &self.modulators[m];
            let name = modulator_slot(modulator.kind, k).map_or("?", |i| i.name);
            return format!("Mod {} {} · {name}", m + 1, modulator.kind.label());
        }
        if let Some((lane, slot, k)) = Self::param_lane_fx(index) {
            let effect = self.lanes[lane].fx[slot].map(|d| d.effect);
            let name = effect.and_then(|e| e.params().get(k)).map_or("?", |i| i.name);
            return format!("Lane {} {} · {name}", lane + 1, effect.map_or("?", |e| e.label()));
        }
        MODULAR_PARAMS.get(index).map_or_else(|| "?".into(), |p| p.name.to_string())
    }

    /// The modules of the group that starts at or contains `module`.
    pub fn group_of(&self, module: usize) -> std::ops::Range<usize> {
        let start = (0..=module.min(MOD_MODULES - 1)).rev().find(|&i| self.modules[i].kind == ModuleKind::Group).unwrap_or(0);
        let end = (start + 1..MOD_MODULES).find(|&i| self.modules[i].kind == ModuleKind::Group).unwrap_or(MOD_MODULES);
        start..end
    }

    /// Factory presets, each showing a way of building the stack.
    pub fn presets() -> Vec<(&'static str, ModularParams)> {
        use ModuleKind as K;
        let module = |kind: K, settings: &[(usize, f32)], choice: u8| {
            let mut m = Module::new(kind);
            for &(slot, value) in settings {
                m.set(slot, value);
            }
            m.choice[0] = choice;
            m
        };
        let build = |modules: &[Module]| {
            let mut p = ModularParams::default();
            p.modules = [Module::EMPTY; MOD_MODULES];
            p.modules[..modules.len()].copy_from_slice(modules);
            p
        };
        let group = Module::new(K::Group);
        let out = |attack: f32, decay: f32, sustain: f32, release: f32| {
            module(K::Output, &[(slot::ATTACK, attack), (slot::DECAY, decay), (slot::SUSTAIN, sustain), (slot::RELEASE, release)], 0)
        };
        let sine = |settings: &[(usize, f32)]| module(K::Analog, settings, 3);
        let saw = |settings: &[(usize, f32)]| module(K::Analog, settings, 0);
        let filter = |kind: u8, cutoff: f32, q: f32, four_pole: bool| {
            let mut f = module(K::Filter, &[(slot::CUTOFF, cutoff), (slot::Q, q)], kind);
            f.choice[1] = four_pole as u8;
            f
        };
        let fx = |effect: Effect| Some(Device::new(effect));
        let reverb = |size: f32, mix: f32| fx(Effect::Reverb(ReverbParams { size, mix, ..ReverbParams::default() }));
        let delay = |time_beats: f64, mix: f32| fx(Effect::Delay(DelayParams { time_beats, feedback: 0.35, mix, ping_pong: true, ..DelayParams::default() }));
        let lfo_rate = |hz: f32| (hz / 0.02).ln() / 2000f32.ln();
        let target = |m: usize, s: usize, depth: f32| ModTarget::new(ModularParams::module_param(m, s), depth);
        let mut list = vec![("Init", ModularParams::default())];

        // Classic two-operator FM: the modulator sits in its own group, unheard.
        let mut p = build(&[group, sine(&[]), out(0.002, 1.5, 0.3, 0.6), group, sine(&[(slot::HARMONIC, 2.0)])]);
        let c = p.connect_audio(4, 1, AudioParam::Phase).unwrap();
        p.audio[c].amount = 0.35;
        p.gain = 0.4;
        list.push(("FM · Two Groups", p));

        let mut p = build(&[group, sine(&[]), out(0.002, 3.0, 0.0, 2.0), group, sine(&[(slot::HARMONIC, 3.5)])]);
        let c = p.connect_audio(4, 1, AudioParam::Phase).unwrap();
        p.audio[c].amount = 0.4;
        let mut decay = Modulator::new(ModulatorKind::Envelope);
        decay.a = 0.0;
        decay.b = 0.6;
        decay.c = 0.0;
        decay.targets[0] = ModTarget::new(MOD_AUDIO_AMOUNT + c, 0.25);
        p.modulators[0] = decay;
        p.lanes[0].fx[0] = reverb(0.75, 0.3);
        p.gain = 0.3;
        list.push(("FM · Bell", p));

        let mut p = build(&[group, sine(&[]), out(0.002, 1.2, 0.2, 0.8), group, sine(&[(slot::HARMONIC, 1.0), (slot::SEMI, 7.0), (slot::CENT, 13.0)])]);
        let c = p.connect_audio(4, 1, AudioParam::Level).unwrap();
        p.audio[c].amount = 1.0;
        p.lanes[0].fx[0] = delay(0.75, 0.3);
        p.gain = 0.7;
        list.push(("Ring · Metallic", p));

        let mut p = build(&[
            group,
            saw(&[(slot::UNISON, 5.0), (slot::DETUNE, 0.3), (slot::PHASE_RANDOM, 180.0)]),
            saw(&[(slot::SEMI, -12.0), (slot::UNISON, 3.0), (slot::DETUNE, 0.2), (slot::LEVEL, 0.6), (slot::PHASE_RANDOM, 180.0)]),
            filter(0, 900.0, 0.9, true),
            module(K::Distortion, &[(slot::DRIVE, 9.0), (slot::MIX, 0.6)], 1),
            out(0.01, 0.6, 0.8, 0.4),
        ]);
        let mut env = Modulator::new(ModulatorKind::Envelope);
        env.a = 0.15;
        env.b = 0.55;
        env.c = 0.2;
        env.targets[0] = target(3, slot::CUTOFF, 0.3);
        p.modulators[0] = env;
        p.lanes[0].fx[0] = reverb(0.5, 0.15);
        p.gain = 0.4;
        list.push(("Stack · Detuned Saws", p));

        // Group 2's oscillator reaches group 1's filter through an Aux.
        let mut p = build(&[
            group,
            saw(&[(slot::LEVEL, 0.7)]),
            module(K::Aux, &[], 0),
            filter(0, 1500.0, 2.0, false),
            out(0.005, 0.8, 0.7, 0.3),
            group,
            module(K::Analog, &[(slot::CENT, 12.0)], 1),
        ]);
        let c = p.connect_audio(6, 2, AudioParam::Input).unwrap();
        p.audio[c].amount = 0.6;
        p.gain = 0.25;
        list.push(("Aux · Cross Feed", p));

        let mut p = build(&[group, module(K::Noise, &[(slot::SLOPE, -4.0), (slot::LEVEL, 2.0)], 0), filter(1, 800.0, 6.0, false), out(0.8, 1.0, 1.0, 1.5)]);
        let mut wind = Modulator::new(ModulatorKind::Lfo);
        wind.a = lfo_rate(0.15);
        wind.targets[0] = target(2, slot::CUTOFF, 0.3);
        p.modulators[0] = wind;
        p.lanes[0].fx[0] = reverb(0.9, 0.3);
        p.gain = 0.8;
        list.push(("Noise · Wind", p));

        let mut p = build(&[group, sine(&[]), saw(&[(slot::HARMONIC, 2.0), (slot::LEVEL, 0.3)]), filter(0, 700.0, 0.7, true), out(0.003, 1.0, 1.0, 0.1)]);
        p.mono = true;
        p.glide = 25.0;
        p.gain = 0.7;
        list.push(("Bass · Sub", p));

        let mut p = build(&[group, saw(&[(slot::A, 2.0)]), filter(0, 5000.0, 0.8, false), out(0.005, 0.5, 0.9, 0.2)]);
        let mut sweep = Modulator::new(ModulatorKind::Envelope);
        sweep.a = 0.0;
        sweep.b = 0.55;
        sweep.c = 0.1;
        sweep.targets[0] = target(1, slot::A, 0.5);
        p.modulators[0] = sweep;
        p.mono = true;
        p.legato = true;
        p.glide = 50.0;
        p.lanes[0].fx[0] = delay(0.5, 0.2);
        p.gain = 0.9;
        list.push(("Lead · Sync Sweep", p));

        // Two layers in two lanes: a pad to the reverb, a pluck to the delay.
        let mut pluck = out(0.002, 0.35, 0.0, 0.3);
        pluck.choice[0] = 1;
        let mut p = build(&[
            group,
            module(K::Wavetable, &[(slot::UNISON, 4.0), (slot::DETUNE, 0.2), (slot::PHASE_RANDOM, 180.0)], 0),
            out(0.6, 1.0, 1.0, 1.5),
            group,
            sine(&[(slot::HARMONIC, 2.0)]),
            pluck,
        ]);
        p.modules[1].table = 3;
        let mut lfo = Modulator::new(ModulatorKind::Lfo);
        lfo.a = lfo_rate(0.1);
        lfo.targets[0] = target(1, slot::A, 0.4);
        p.modulators[0] = lfo;
        p.lanes[0].fx[0] = reverb(0.85, 0.35);
        p.lanes[1].fx[0] = delay(0.75, 0.35);
        p.gain = 0.32;
        list.push(("Layers · Pad + Pluck", p));

        // ----- Phase 2: global unison, nonlinear filter, curve output, keytracked noise -----
        let unison = |voices: u8, mode: UnisonMode, detune: f32, spread: f32, blend: f32, bias: f32| Unison { voices, mode, detune, spread, blend, bias, ..Unison::default() };

        let mut p = build(&[group, saw(&[]), filter(0, 7000.0, 0.7, false), out(0.01, 1.0, 0.85, 0.5)]);
        p.unison = unison(7, UnisonMode::Smooth, 0.4, 1.0, 0.6, 0.0);
        p.lanes[0].fx[0] = reverb(0.6, 0.2);
        p.gain = 0.31;
        list.push(("Unison · Supersaw", p));

        let mut p = build(&[group, saw(&[(slot::LEVEL, 0.8)]), sine(&[(slot::SEMI, -12.0)]), filter(0, 3000.0, 0.7, false), out(0.02, 1.0, 0.9, 0.6)]);
        p.unison = unison(3, UnisonMode::PitchStack, 0.1, 0.8, 0.5, 0.0);
        p.lanes[0].fx[0] = delay(0.75, 0.25);
        p.gain = 0.25;
        list.push(("Unison · Octave Stack", p));

        let mut p = build(&[group, saw(&[]), filter(0, 2500.0, 1.2, true), out(0.003, 0.5, 0.0, 0.3)]);
        p.unison = unison(4, UnisonMode::Chords, 0.15, 0.7, 0.5, -0.2);
        p.unison.chord = 8;
        let mut env = Modulator::new(ModulatorKind::Envelope);
        env.a = 0.0;
        env.b = 0.5;
        env.c = 0.0;
        env.targets[0] = target(2, slot::CUTOFF, 0.25);
        p.modulators[0] = env;
        p.lanes[0].fx[0] = delay(0.75, 0.3);
        p.lanes[1].fx[0] = reverb(0.7, 0.25);
        p.lanes[0].dest = 1;
        p.gain = 0.45;
        list.push(("Chords · Minor 7 Stab", p));

        // Octaves of the pitch class, loudest around a center that drifts.
        let mut p = build(&[group, module(K::Analog, &[], 2), out(0.5, 1.0, 1.0, 1.5)]);
        p.unison = unison(8, UnisonMode::Shepard, 0.05, 0.6, 0.5, 0.0);
        let mut drift = Modulator::new(ModulatorKind::Lfo);
        drift.a = lfo_rate(0.05);
        drift.bipolar = true;
        drift.targets[0] = ModTarget::new(MOD_UNISON + 3, 0.35);
        p.modulators[0] = drift;
        p.lanes[0].fx[0] = reverb(0.85, 0.35);
        p.gain = 0.2;
        list.push(("Shepard · Drifting Organ", p));

        let mut acid = module(K::NonlinearFilter, &[(slot::CUTOFF, 400.0), (slot::Q, 9.0), (slot::NL_DRIVE, 15.0)], 0);
        acid.choice[1] = 1;
        let mut loud = out(0.002, 0.4, 0.6, 0.08);
        loud.set(slot::LEVEL, 1.8);
        let mut p = build(&[group, saw(&[]), acid, loud]);
        let mut env = Modulator::new(ModulatorKind::Envelope);
        env.a = 0.0;
        env.b = 0.45;
        env.c = 0.0;
        env.targets[0] = target(2, slot::CUTOFF, 0.45);
        p.modulators[0] = env;
        p.mono = true;
        p.legato = true;
        p.glide = 60.0;
        p.lanes[0].fx[0] = delay(0.75, 0.2);
        p.gain = 0.9;
        list.push(("Nonlinear · Acid", p));

        // A drawn rhythmic gate that loops while the note is held.
        let mut gate = module(K::CurveOutput, &[(slot::CURVE_LENGTH, 0.5), (slot::CURVE_START, 0.0), (slot::CURVE_END, 1.0)], 0);
        gate.choice[1] = 1;
        gate.set(slot::LEVEL, 1.7);
        // Opening over the first 2% (a few ms): straight to 1 it clicked.
        gate.curve = Shape::from_points(&[(0.0, 0.0, 0.0), (0.02, 1.0, 0.0), (0.4, 0.9, 0.0), (0.42, 0.0, 0.0), (0.5, 0.0, 0.0), (0.52, 1.0, 0.0), (0.7, 0.8, 0.0), (0.72, 0.0, 0.0), (1.0, 0.0, 0.0)]);
        let mut p = build(&[group, saw(&[(slot::UNISON, 3.0), (slot::DETUNE, 0.25), (slot::PHASE_RANDOM, 180.0)]), filter(0, 2200.0, 1.0, false), gate]);
        p.lanes[0].fx[0] = reverb(0.7, 0.25);
        p.gain = 1.0;
        list.push(("Curve · Gated Pad", p));

        // Smooth keytracked noise is pitched: a breathy tone that follows the keyboard.
        let mut breath = module(K::Noise, &[(slot::LEVEL, 0.8)], 2);
        breath.choice[1] = 1;
        let mut p = build(&[group, breath, sine(&[(slot::LEVEL, 0.35)]), filter(0, 2500.0, 0.9, false), out(0.08, 1.0, 0.8, 0.6)]);
        p.lanes[0].fx[0] = reverb(0.8, 0.3);
        p.gain = 0.27;
        list.push(("Noise · Pitched Breath", p));

        // FM into a wavetable; its bandlimit (macro 1) tames the harshness.
        let mut p = build(&[group, module(K::Wavetable, &[(slot::A, 60.0), (slot::BANDLIMIT, 5000.0)], 0), out(0.005, 1.2, 0.5, 0.5), group, sine(&[(slot::HARMONIC, 2.0)])]);
        p.modules[1].table = 2;
        let c = p.connect_audio(4, 1, AudioParam::Phase).unwrap();
        p.audio[c].amount = 0.45;
        let mut knob = Modulator::new(ModulatorKind::Macro);
        knob.targets[0] = target(1, slot::BANDLIMIT, 0.5);
        p.modulators[0] = knob;
        p.lanes[0].fx[0] = reverb(0.5, 0.2);
        p.gain = 0.4;
        list.push(("Wavetable · Bandlimited FM", p));

        // Modulation chains. Rates and depths are set in real units through `set`.
        let modulator = |kind: ModulatorKind, settings: &[(usize, f32)]| {
            let mut m = Modulator::new(kind);
            for &(k, value) in settings {
                m.set(k, value);
            }
            m
        };
        let modulator_target = |m: usize, k: usize, depth: f32| ModTarget::new(ModularParams::modulator_param(m, k), depth);
        let tri = Shape::from_points(&[(0.0, 0.0, 0.0), (0.5, 1.0, 0.0), (1.0, 0.0, 0.0)]);

        // An eighth-note LFO, remapped so the filter snaps open, on a saw and its sub.
        let mut p = build(&[group, saw(&[]), sine(&[(slot::SEMI, -12.0), (slot::LEVEL, 0.8)]), filter(0, 120.0, 3.0, true), out(0.003, 1.0, 1.0, 0.15)]);
        let mut wobble = modulator(ModulatorKind::Lfo, &[]);
        wobble.shape = tri;
        wobble.mode = LfoMode::Retrig;
        wobble.sync_beats = 0.5;
        let mut remap = modulator(ModulatorKind::Remap, &[]);
        remap.source = 0;
        remap.shape = Shape::from_points(&[(0.0, 0.0, 0.0), (1.0, 1.0, -0.6)]);
        remap.targets[0] = target(3, slot::CUTOFF, 0.55);
        p.modulators[0] = wobble;
        p.modulators[1] = remap;
        p.mono = true;
        p.gain = 0.14;
        list.push(("Mod · Wobble Bass", p));

        // Fast random values, sampled every sixteenth by a square LFO.
        let mut p = build(&[group, saw(&[(slot::UNISON, 2.0), (slot::DETUNE, 0.1)]), filter(0, 400.0, 4.0, false), out(0.002, 0.3, 0.6, 0.2)]);
        let source = modulator(ModulatorKind::Random, &[(mslot::RATE, 40.0)]);
        let mut clock = modulator(ModulatorKind::Lfo, &[]);
        clock.shape = Shape::from_points(&[(0.0, 1.0, 0.0), (0.5, 1.0, 0.0), (0.501, 0.0, 0.0), (1.0, 0.0, 0.0)]);
        clock.sync_beats = 0.25;
        let mut sh = modulator(ModulatorKind::SampleHold, &[]);
        sh.source = 0;
        sh.trigger_source = 1;
        sh.targets[0] = target(2, slot::CUTOFF, 0.45);
        p.modulators[..3].copy_from_slice(&[source, clock, sh]);
        p.lanes[0].fx[0] = delay(0.75, 0.25);
        p.gain = 0.35;
        list.push(("Mod · Sample & Hold Filter", p));

        // Vibrato that fades in: a delayed envelope opens the LFO's depth.
        let mut p = build(&[group, saw(&[]), filter(0, 2500.0, 0.8, false), out(0.02, 1.0, 1.0, 0.3)]);
        let mut vibrato = modulator(ModulatorKind::Lfo, &[(mslot::RATE, 5.5), (mslot::DEPTH, 0.0)]);
        vibrato.shape = tri;
        vibrato.mode = LfoMode::Retrig;
        vibrato.bipolar = true;
        vibrato.targets[0] = target(1, slot::CENT, 0.2);
        let mut fade = modulator(ModulatorKind::Envelope, &[(mslot::DELAY, 0.35), (mslot::ATTACK, 0.8), (mslot::SUSTAIN, 1.0), (mslot::RELEASE, 0.3)]);
        fade.targets[0] = modulator_target(0, mslot::DEPTH, 1.0);
        p.modulators[..2].copy_from_slice(&[vibrato, fade]);
        p.lanes[0].fx[0] = reverb(0.6, 0.2);
        p.mono = true;
        p.legato = true;
        p.glide = 60.0;
        p.gain = 0.75;
        list.push(("Mod · Delayed Vibrato", p));

        // A one-bar drawn filter rhythm that loops while the chord is held.
        let mut p = build(&[group, saw(&[(slot::UNISON, 4.0), (slot::DETUNE, 0.3), (slot::PHASE_RANDOM, 180.0)]), filter(0, 300.0, 2.0, true), out(0.01, 1.0, 1.0, 0.5)]);
        let mut rhythm = modulator(ModulatorKind::Timeline, &[]);
        rhythm.shape = Shape::from_points(&[(0.0, 1.0, 0.0), (0.2, 0.1, 0.5), (0.25, 0.8, 0.0), (0.45, 0.1, 0.5), (0.5, 1.0, 0.0), (0.62, 0.3, 0.0), (0.75, 0.9, 0.0), (1.0, 0.0, 0.5)]);
        rhythm.sync_beats = 4.0;
        rhythm.loop_mode = 2;
        rhythm.targets[0] = target(2, slot::CUTOFF, 0.5);
        p.modulators[0] = rhythm;
        p.lanes[0].fx[0] = reverb(0.7, 0.25);
        p.gain = 0.14;
        list.push(("Mod · Rhythmic Curve", p));

        // Slow, uneven random drift of pitch and pan, different on every note.
        let mut p = build(&[group, saw(&[(slot::LEVEL, 0.7)]), saw(&[(slot::CENT, 9.0), (slot::LEVEL, 0.7)]), filter(0, 1800.0, 0.7, false), out(0.6, 1.0, 1.0, 1.2)]);
        let mut pitch = modulator(ModulatorKind::Random, &[(mslot::RATE, 0.6), (mslot::SMOOTH, 1.0), (mslot::JITTER, 0.6), (mslot::CHAOS, 0.4)]);
        pitch.bipolar = true;
        pitch.targets[0] = target(1, slot::CENT, 0.08);
        pitch.targets[1] = target(2, slot::CENT, -0.08);
        let mut pan = modulator(ModulatorKind::Random, &[(mslot::RATE, 0.3), (mslot::SMOOTH, 1.0), (mslot::JITTER, 0.8), (mslot::CHAOS, 0.7)]);
        pan.bipolar = true;
        pan.targets[0] = target(4, slot::PAN, 0.3);
        p.modulators[..2].copy_from_slice(&[pitch, pan]);
        p.lanes[0].fx[0] = reverb(0.85, 0.35);
        p.gain = 0.2;
        list.push(("Mod · Drifting Pad", p));

        // For a MIDI keyboard: the mod wheel adds vibrato, aftertouch opens the filter.
        let mut p = build(&[group, saw(&[]), module(K::Analog, &[(slot::SEMI, 12.0), (slot::LEVEL, 0.3)], 1), filter(0, 900.0, 1.5, false), out(0.005, 1.0, 0.9, 0.25)]);
        let mut vibrato = modulator(ModulatorKind::Lfo, &[(mslot::RATE, 5.0), (mslot::DEPTH, 0.0)]);
        vibrato.shape = tri;
        vibrato.bipolar = true;
        vibrato.targets[0] = target(1, slot::CENT, 0.25);
        vibrato.targets[1] = target(2, slot::CENT, 0.25);
        let mut wheel = modulator(ModulatorKind::MidiCc, &[]);
        wheel.index = 1;
        wheel.targets[0] = modulator_target(0, mslot::DEPTH, 1.0);
        let mut pressure = modulator(ModulatorKind::Pressure, &[]);
        pressure.targets[0] = target(3, slot::CUTOFF, 0.4);
        let mut velocity = modulator(ModulatorKind::Velocity, &[]);
        velocity.targets[0] = ModTarget { curve: 0.4, ..target(3, slot::CUTOFF, 0.2) };
        p.modulators[..4].copy_from_slice(&[vibrato, wheel, pressure, velocity]);
        p.lanes[0].fx[0] = delay(0.75, 0.2);
        p.mono = true;
        p.legato = true;
        p.glide = 40.0;
        p.gain = 0.55;
        list.push(("Mod · Expressive Lead", p));

        // Poly lane: each note of the chord is driven on its own, so chords stay clean
        // (no intermodulation); velocity drives harder. A shared reverb lane after it.
        let mut p = build(&[group, saw(&[(slot::UNISON, 3.0), (slot::DETUNE, 0.2)]), filter(0, 3200.0, 0.9, false), out(0.01, 0.8, 0.8, 0.5)]);
        p.lanes[0].fx[0] = fx(Effect::Saturator(SaturatorParams { drive_db: 16.0, output_db: -10.0, mix: 1.0, ..Default::default() }));
        p.lanes[0].poly = true;
        p.lanes[0].dest = 2;
        p.lanes[2].fx[0] = reverb(0.8, 0.3);
        let mut velocity = modulator(ModulatorKind::Velocity, &[]);
        velocity.targets[0] = ModTarget::new(ModularParams::lane_fx_param(0, 0, 0), 0.25);
        p.modulators[0] = velocity;
        p.gain = 0.42;
        list.push(("Lanes · Poly Drive Chords", p));

        // Parallel lanes: the same saw clean with chorus in lane 1, crushed in lane 2.
        let mut p = build(&[group, saw(&[]), filter(0, 5000.0, 0.7, false), out(0.005, 0.6, 0.6, 0.3), group, saw(&[]), out(0.005, 0.6, 0.6, 0.3)]);
        p.modules[6].choice[0] = 1;
        p.lanes[0].fx[0] = fx(Effect::Chorus(ChorusParams::default()));
        p.lanes[1].fx[0] = fx(Effect::Bitcrusher(BitcrusherParams { bits: 5.0, downsample: 6.0, mix: 1.0 }));
        p.lanes[1].mix = 0.6;
        p.lanes[1].gain = 0.5;
        p.gain = 0.37;
        list.push(("Lanes · Parallel Crush", p));

        // Sidechain pumping inside the patch: a looping curve beats quarter notes into the
        // sideband, unheard; a follower listening to it ducks the pad.
        let mut beat = module(K::CurveOutput, &[(slot::CURVE_LENGTH, 0.5), (slot::CURVE_START, 0.0), (slot::CURVE_END, 1.0)], 0);
        beat.choice[0] = MOD_SEND_SIDEBAND;
        beat.choice[1] = 1;
        beat.curve = Shape::from_points(&[(0.0, 1.0, 0.0), (0.25, 0.0, 0.5), (1.0, 0.0, 0.0)]);
        let mut p = build(&[group, saw(&[(slot::UNISON, 4.0), (slot::DETUNE, 0.25), (slot::PHASE_RANDOM, 180.0)]), filter(0, 2600.0, 0.8, false), out(0.05, 1.0, 1.0, 0.6), group, sine(&[]), beat]);
        let mut follower = modulator(ModulatorKind::AudioFollower, &[(mslot::FOLLOW_ATTACK, 0.002), (mslot::FOLLOW_RELEASE, 0.15)]);
        follower.index = (MOD_LANES + 1) as u8;
        follower.targets[0] = target(3, slot::LEVEL, -0.45);
        p.modulators[0] = follower;
        p.lanes[0].fx[0] = reverb(0.6, 0.2);
        p.gain = 0.3;
        list.push(("Lanes · Sideband Pump", p));

        // Poly echoes: every note has its own delay, so echoes of one note never
        // smear into the next; a random modulator sets each note's feedback.
        let mut p = build(&[group, sine(&[]), filter(0, 4000.0, 0.7, false), out(0.002, 0.25, 0.0, 0.2)]);
        p.modules[1].set(slot::HARMONIC, 1.0);
        let mut pluck = Module::new(K::Analog);
        pluck.choice[0] = 1;
        pluck.set(slot::LEVEL, 0.4);
        p.modules[2] = pluck;
        p.modules[3] = filter(0, 4000.0, 0.7, false);
        p.modules[4] = out(0.002, 0.25, 0.0, 0.2);
        p.lanes[0].fx[0] = fx(Effect::Delay(DelayParams { time_beats: 0.75, feedback: 0.4, mix: 0.45, ping_pong: true, ..DelayParams::default() }));
        p.lanes[0].poly = true;
        p.lanes[0].dest = 1;
        p.lanes[1].fx[0] = reverb(0.6, 0.2);
        let mut random = modulator(ModulatorKind::Random, &[]);
        random.targets[0] = ModTarget::new(ModularParams::lane_fx_param(0, 0, 0), 0.3);
        p.modulators[0] = random;
        p.modules[4].set(slot::LEVEL, 1.8);
        p.gain = 1.0;
        list.push(("Lanes · Poly Echo Pluck", p));
        list
    }

    /// Swaps two modules, keeping every connection pointing at the same modules.
    pub fn swap_modules(&mut self, a: usize, b: usize) {
        if a == b || a >= MOD_MODULES || b >= MOD_MODULES {
            return;
        }
        self.modules.swap(a, b);
        let swap = |i: u8| if i as usize == a { b as u8 } else if i as usize == b { a as u8 } else { i };
        for link in &mut self.audio {
            if link.source != MOD_NO_MODULE {
                link.source = swap(link.source);
                link.target = swap(link.target);
            }
        }
        for modulator in &mut self.modulators {
            for target in &mut modulator.targets {
                if let Some((m, s)) = Self::param_module(target.dest as usize) {
                    let m = if m == a { b } else if m == b { a } else { m };
                    target.dest = Self::module_param(m, s) as u16;
                }
            }
        }
    }

    /// Puts `module` right below `after` (at the top for None), moving the rest down.
    /// Returns where it went, or None when the stack is full.
    pub fn insert_module(&mut self, after: Option<usize>, module: Module) -> Option<usize> {
        let free = self.modules.iter().rposition(|m| m.kind != ModuleKind::Empty).map_or(0, |last| last + 1);
        if free >= MOD_MODULES {
            return None;
        }
        self.modules[free] = module;
        let at = after.map_or(0, |a| a + 1).min(free);
        let mut i = free;
        while i > at {
            self.swap_modules(i, i - 1);
            i -= 1;
        }
        Some(at)
    }

    /// Removes a module and everything connected to it; the modules below move up.
    pub fn remove_module(&mut self, index: usize) {
        if index >= MOD_MODULES {
            return;
        }
        for link in &mut self.audio {
            if link.source as usize == index || link.target as usize == index {
                *link = AudioMod::NONE;
            }
        }
        for modulator in &mut self.modulators {
            for target in &mut modulator.targets {
                if Self::param_module(target.dest as usize).is_some_and(|(m, _)| m == index) {
                    *target = ModTarget::NONE;
                }
            }
        }
        self.modules[index] = Module::EMPTY;
        let last = self.modules.iter().rposition(|m| m.kind != ModuleKind::Empty).unwrap_or(0);
        for i in index..last {
            self.swap_modules(i, i + 1);
        }
    }

    /// Adds an audio-rate connection (or returns the existing one); None when all are used.
    pub fn connect_audio(&mut self, source: usize, target: usize, param: AudioParam) -> Option<usize> {
        if let Some(existing) = self.audio.iter().position(|l| l.source as usize == source && l.target as usize == target && l.param == param) {
            return Some(existing);
        }
        let free = self.audio.iter().position(|l| l.source == MOD_NO_MODULE)?;
        self.audio[free] = AudioMod { source: source as u8, target: target as u8, param, amount: 0.5 };
        Some(free)
    }

    /// True when going from `self` to `other` changes what the synth allocates (lane effects).
    pub fn layout_differs(&self, other: &ModularParams) -> bool {
        let fx = |p: &ModularParams| p.lanes.map(|l| l.fx.map(|d| d.map(|d| std::mem::discriminant(&d.effect))));
        let poly = |p: &ModularParams| [0, 1, 2].map(|l| p.lane_poly(l));
        fx(self) != fx(other) || poly(self) != poly(other)
    }
}

pub const MODULAR_PARAMS: [ParamInfo; MOD_PARAM_COUNT] = [
    ParamInfo::linear("M1·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·9", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·10", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·11", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·12", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·13", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·14", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·15", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·16", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Audio 1", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 2", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 3", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 4", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 5", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 6", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 7", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 8", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 9", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 10", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 11", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 12", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 13", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 14", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 15", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 16", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 17", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 18", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 19", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 20", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 21", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 22", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 23", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 24", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 25", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 26", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 27", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 28", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 29", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 30", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 31", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Audio 32", -1.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Lane A", 0.0, 1.5, 1.0, Percent),
    ParamInfo::linear("Lane B", 0.0, 1.5, 1.0, Percent),
    ParamInfo::linear("Lane C", 0.0, 1.5, 1.0, Percent),
    ParamInfo::linear("Mod1 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod1 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod1 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod1 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod2 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod2 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod2 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod2 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod3 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod3 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod3 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod3 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod4 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod4 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod4 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod4 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod5 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod5 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod5 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod5 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod6 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod6 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod6 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod6 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod7 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod7 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod7 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod7 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod8 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod8 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod8 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod8 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod9 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod9 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod9 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod9 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod10 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod10 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod10 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod10 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod11 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod11 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod11 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod11 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod12 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod12 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod12 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod12 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod13 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod13 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod13 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod13 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod14 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod14 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod14 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod14 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod15 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod15 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod15 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod15 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod16 A", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod16 B", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod16 C", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Mod16 D", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Macro 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Macro 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Macro 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Macro 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Macro 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Macro 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Macro 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Macro 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Glide", 0.0, 2000.0, 0.0, Milliseconds),
    ParamInfo::linear("Master", 0.0, 1.0, 0.35, Percent),
    ParamInfo::linear("M1·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M1·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M2·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M3·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M4·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M5·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M6·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M7·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M8·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M9·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M10·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M11·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M12·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M13·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M14·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M15·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M16·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M17·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M18·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M19·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M20·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M21·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M22·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M23·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M24·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M25·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M26·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M27·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M28·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M29·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M30·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M31·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·17", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·18", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·19", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·20", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·21", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·22", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·23", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("M32·24", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Unison Detune", 0.0, 1.0, 0.25, Percent),
    ParamInfo::linear("Unison Spread", 0.0, 1.0, 0.7, Percent),
    ParamInfo::linear("Unison Blend", 0.0, 1.0, 0.5, Percent),
    ParamInfo::linear("Unison Bias", -1.0, 1.0, 0.0, Plain),
    ParamInfo::linear("Mod1 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod1 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod1 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod1 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod1 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod1 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod1 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod1 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod2 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod2 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod2 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod2 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod2 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod2 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod2 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod2 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod3 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod3 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod3 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod3 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod3 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod3 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod3 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod3 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod4 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod4 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod4 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod4 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod4 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod4 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod4 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod4 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod5 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod5 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod5 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod5 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod5 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod5 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod5 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod5 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod6 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod6 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod6 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod6 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod6 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod6 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod6 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod6 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod7 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod7 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod7 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod7 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod7 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod7 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod7 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod7 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod8 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod8 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod8 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod8 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod8 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod8 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod8 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod8 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod9 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod9 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod9 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod9 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod9 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod9 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod9 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod9 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod10 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod10 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod10 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod10 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod10 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod10 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod10 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod10 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod11 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod11 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod11 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod11 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod11 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod11 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod11 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod11 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod12 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod12 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod12 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod12 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod12 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod12 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod12 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod12 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod13 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod13 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod13 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod13 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod13 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod13 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod13 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod13 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod14 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod14 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod14 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod14 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod14 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod14 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod14 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod14 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod15 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod15 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod15 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod15 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod15 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod15 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod15 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod15 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod16 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod16 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod16 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod16 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod16 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod16 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod16 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod16 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod17 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod17 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod17 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod17 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod17 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod17 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod17 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod17 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod18 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod18 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod18 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod18 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod18 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod18 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod18 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod18 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod19 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod19 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod19 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod19 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod19 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod19 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod19 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod19 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod20 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod20 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod20 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod20 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod20 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod20 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod20 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod20 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod21 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod21 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod21 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod21 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod21 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod21 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod21 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod21 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod22 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod22 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod22 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod22 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod22 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod22 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod22 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod22 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod23 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod23 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod23 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod23 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod23 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod23 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod23 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod23 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod24 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod24 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod24 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod24 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod24 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod24 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod24 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod24 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod25 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod25 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod25 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod25 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod25 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod25 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod25 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod25 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod26 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod26 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod26 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod26 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod26 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod26 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod26 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod26 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod27 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod27 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod27 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod27 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod27 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod27 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod27 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod27 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod28 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod28 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod28 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod28 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod28 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod28 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod28 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod28 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod29 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod29 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod29 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod29 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod29 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod29 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod29 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod29 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod30 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod30 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod30 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod30 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod30 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod30 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod30 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod30 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod31 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod31 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod31 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod31 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod31 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod31 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod31 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod31 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod32 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod32 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod32 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod32 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod32 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod32 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod32 7", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Mod32 8", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX1 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX1 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX1 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX1 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX1 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX1 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX2 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX2 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX2 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX2 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX2 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX2 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX3 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX3 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX3 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX3 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX3 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX3 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX4 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX4 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX4 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX4 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX4 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX4 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX5 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX5 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX5 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX5 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX5 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX5 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX6 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX6 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX6 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX6 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX6 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX6 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX7 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX7 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX7 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX7 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX7 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX7 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX8 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX8 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX8 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX8 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX8 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L1 FX8 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX1 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX1 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX1 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX1 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX1 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX1 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX2 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX2 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX2 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX2 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX2 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX2 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX3 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX3 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX3 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX3 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX3 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX3 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX4 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX4 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX4 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX4 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX4 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX4 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX5 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX5 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX5 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX5 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX5 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX5 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX6 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX6 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX6 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX6 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX6 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX6 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX7 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX7 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX7 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX7 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX7 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX7 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX8 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX8 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX8 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX8 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX8 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L2 FX8 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX1 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX1 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX1 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX1 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX1 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX1 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX2 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX2 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX2 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX2 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX2 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX2 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX3 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX3 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX3 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX3 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX3 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX3 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX4 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX4 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX4 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX4 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX4 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX4 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX5 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX5 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX5 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX5 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX5 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX5 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX6 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX6 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX6 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX6 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX6 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX6 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX7 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX7 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX7 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX7 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX7 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX7 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX8 1", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX8 2", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX8 3", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX8 4", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX8 5", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("L3 FX8 6", 0.0, 1.0, 0.0, Percent),
    ParamInfo::linear("Lane 1 Mix", 0.0, 1.0, 1.0, Percent),
    ParamInfo::linear("Lane 2 Mix", 0.0, 1.0, 1.0, Percent),
    ParamInfo::linear("Lane 3 Mix", 0.0, 1.0, 1.0, Percent),
    ParamInfo::linear("Pitch", -24.0, 24.0, 0.0, Semitones),
    ParamInfo::linear("Fine", -100.0, 100.0, 0.0, Plain),
];

#[cfg(test)]
mod tests {
    #[test]
    fn a_drum_machine_from_when_it_had_eight_sounds_opens_with_sixteen() {
        let old = r#"{"tune":[1,2,3,4,5,6,7,8],"decay":[2,2,2,2,2,2,2,2],"level":[0.5,0.5,0.5,0.5,0.5,0.5,0.5,0.5],"model":"Tr909"}"#;
        let p: super::DrumParams = serde_json::from_str(old).unwrap();
        assert_eq!(p.tune[..8], [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        // The new sounds are silent: the notes an old machine ignored stay unheard.
        assert_eq!((p.tune[8], p.decay[15], p.level[12], p.decay[0]), (0.0, 1.0, 0.0, 2.0));
        assert_eq!(p.model, super::DrumModel::Tr909);
        let again: super::DrumParams = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(again, p);
    }

    use super::*;

    #[test]
    fn warp_markers_map_beats_to_the_file_and_back() {
        let mut clip = AudioClip { hard_start: false, fades: Default::default(),
            name: "x".into(),
            color: [0; 3],
            sample: "x.wav".into(),
            length: 8.0,
            offset: 0.3,
            warp: WarpMode::Stretch,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(), muted: false,
        };
        // Without markers: the original tempo.
        assert!((clip.source_seconds(2.0, 90.0) - 1.3).abs() < 1e-12);
        clip.warp_markers = [(0.0, 0.5), (4.0, 2.5), (8.0, 4.0)].map(|(beat, seconds)| WarpMarker { beat, seconds }).to_vec();
        assert!((clip.file_seconds(2.0) - 1.5).abs() < 1e-12);
        assert!((clip.file_seconds(6.0) - 3.25).abs() < 1e-12);
        // Past the outer markers, at the nearest stretch's tempo.
        assert!((clip.file_seconds(-1.0) - 0.0).abs() < 1e-12);
        assert!((clip.file_seconds(10.0) - 4.75).abs() < 1e-12);
        for beat in [-1.0, 0.0, 1.3, 4.0, 7.9, 12.0] {
            assert!((clip.file_beat(clip.file_seconds(beat)) - beat).abs() < 1e-9);
        }
        // The clip starts 0.3 s in, beat -0.4 of the file.
        assert!((clip.source_seconds(4.4, 90.0) - 2.5).abs() < 1e-9);
        assert!((clip.beats_until(2.5, 90.0) - 4.4).abs() < 1e-9);
        // Twice the original tempo: twice the beats, the same points.
        clip.set_original_bpm(240.0);
        assert!((clip.file_seconds(8.0) - 2.5).abs() < 1e-12);
        // Warp off: the project's tempo.
        clip.warp = WarpMode::Off;
        assert!((clip.source_seconds(3.0, 90.0) - 2.3).abs() < 1e-12);
    }

    #[test]
    fn lanes_interpolate_and_hold_their_ends() {
        let mut lane = AutomationLane::new(Param::Volume);
        assert_eq!(lane.value_at(0.0), None);
        lane.points = vec![AutomationPoint { beat: 4.0, value: 0.0, curve: 0.0 }, AutomationPoint { beat: 8.0, value: 1.0, curve: 0.0 }];
        assert_eq!(lane.value_at(0.0), Some(0.0));
        assert_eq!(lane.value_at(6.0), Some(0.5));
        assert_eq!(lane.value_at(8.0), Some(1.0));
        assert_eq!(lane.value_at(100.0), Some(1.0));
    }

    #[test]
    fn grooves_swing_soften_and_humanize_the_off_beats() {
        let groove = Groove { grid: 0.5, swing: 0.6, accent: 0.5, humanize: 0.0, template: None };
        assert_eq!(groove.apply(0, 0, 0.0, 1.0), (0.0, 1.0), "on the beat: as written");
        let (shift, velocity) = groove.apply(1, 0, 0.5, 1.0);
        assert!((shift - 0.5 * 0.6 / 3.0).abs() < 1e-6 && (velocity - 0.7).abs() < 1e-6, "{shift} {velocity}");
        assert_eq!(groove.apply(2, 0, 0.6, 0.8), (0.0, 0.8), "off the grid: left alone");
        // Humanized: small, different per note and lap, the same every time.
        let human = Groove { humanize: 1.0, ..Groove::default() };
        let a = human.apply(3, 0, 1.0, 0.8);
        assert_eq!(a, human.apply(3, 0, 1.0, 0.8));
        assert_ne!(a, human.apply(3, 1, 1.0, 0.8));
        let shifts: Vec<f64> = (0..200).map(|i| human.apply(i, 0, 1.0, 0.8).0).collect();
        assert!(shifts.iter().all(|s| s.abs() <= 1.0 / 32.0 + 1e-9));
        assert!(shifts.iter().any(|&s| s > 0.01) && shifts.iter().any(|&s| s < -0.01), "both ways");
        let json = serde_json::to_string(&MidiClip::new("a", 4.0, [0; 3])).unwrap();
        assert!(!json.contains("groove"), "clips without one save as before");
    }

    #[test]
    fn bent_stretches_curve_and_keep_their_ends() {
        let mut lane = AutomationLane::new(Param::Volume);
        lane.points = vec![AutomationPoint { beat: 4.0, value: 0.0, curve: 0.6 }, AutomationPoint { beat: 8.0, value: 1.0, curve: 0.0 }];
        // Fast first, then settling: above the straight line in the middle.
        assert!(lane.value_at(6.0).unwrap() > 0.8);
        assert_eq!((lane.value_at(4.0), lane.value_at(8.0)), (Some(0.0), Some(1.0)));
        lane.points[0].curve = -0.6;
        assert!(lane.value_at(6.0).unwrap() < 0.2, "slow first");
        // Straight ones don't write the field (older projects read back the same).
        let json = serde_json::to_string(&AutomationPoint { beat: 1.0, value: 0.5, curve: 0.0 }).unwrap();
        assert!(!json.contains("curve"), "{json}");
        let back: AutomationPoint = serde_json::from_str(r#"{"beat":1.0,"value":0.5}"#).unwrap();
        assert_eq!(back.curve, 0.0);
        assert_eq!(lane.curve_at(5.0), -0.6);
        assert_eq!(lane.curve_at(9.0), 0.0, "past the last point there is no stretch");
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
            // A modular synth's lane effect settings exist only while the effect does.
            let modular = matches!(kind, InstrumentKind::Modular(_));
            for (i, info) in kind.params().iter().enumerate().filter(|&(i, _)| !modular || ModularParams::param_lane_fx(i).is_none()) {
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

    #[test]
    fn modular_patches_from_the_first_layout_still_load() {
        // Saved before the extra slots, the curve output and the global unison existed.
        let mut json = serde_json::to_value(ModularParams::presets()[1].1).unwrap();
        json.as_object_mut().unwrap().remove("unison");
        for module in json["modules"].as_array_mut().unwrap() {
            let module = module.as_object_mut().unwrap();
            module.remove("extra");
            module.remove("alt");
            module.remove("curve");
        }
        let p: ModularParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.unison, Unison::default());
        assert_eq!(p.modules[1].kind, ModuleKind::Analog);
        // An old looping sample keeps looping.
        let mut old = Module::new(ModuleKind::Sample);
        old.choice[0] = 0;
        old.flag = true;
        assert_eq!(old.sample_loop(), 1);
        // Extra slots have their own parameter numbers.
        let index = ModularParams::module_param(5, slot::ROOT);
        assert!(index >= MOD_EXTRA && index < MOD_UNISON);
        assert_eq!(ModularParams::param_module(index), Some((5, slot::ROOT)));
        let mut p = ModularParams::default();
        p.modules[5] = Module::new(ModuleKind::Granular);
        *p.param_mut(index).unwrap() = 0.25;
        assert_eq!(p.modules[5].value(slot::ROOT), 0.25);
        assert!(p.param_name(index).contains("Root"));
    }

    #[test]
    fn moving_and_removing_modules_keeps_connections_on_the_same_modules() {
        let mut p = ModularParams::default();
        p.modules[3] = Module::new(ModuleKind::Group);
        p.modules[4] = Module::new(ModuleKind::Analog);
        p.connect_audio(4, 1, AudioParam::Phase);
        p.modulators[0] = Modulator::new(ModulatorKind::Lfo);
        p.modulators[0].targets[0] = ModTarget::new(ModularParams::module_param(4, slot::LEVEL), 0.5);

        // Insert a filter under the first oscillator: everything below moves down one.
        let at = p.insert_module(Some(1), Module::new(ModuleKind::Filter)).unwrap();
        assert_eq!(at, 2);
        assert_eq!(p.modules[2].kind, ModuleKind::Filter);
        assert_eq!(p.modules[5].kind, ModuleKind::Analog);
        assert_eq!((p.audio[0].source, p.audio[0].target), (5, 1));
        assert_eq!(p.modulators[0].targets[0].dest as usize, ModularParams::module_param(5, slot::LEVEL));

        // Removing the modulator source drops its connections.
        p.remove_module(5);
        assert_eq!(p.audio[0].source, MOD_NO_MODULE);
        assert_eq!(p.modulators[0].targets[0].dest, MOD_NO_TARGET);
        assert_eq!(p.modules[4].kind, ModuleKind::Group);
        assert_eq!(p.modules[5].kind, ModuleKind::Empty);
        assert!(p.connect_audio(1, 2, AudioParam::Level).is_some());
    }
}
