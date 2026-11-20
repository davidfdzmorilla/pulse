//! Built-in instruments for electronic music: drum machine, unison supersaw,
//! acid bassline and two-operator FM. All are fixed-size (no allocation), so they
//! can be created and played on the audio thread.

use crate::model::{drum_note, AcidParams, DrumParams, FmParams, SuperSawParams};
use crate::synth::{midi_to_freq, HELD};
use std::f32::consts::{PI, TAU};

pub(crate) fn semitones(st: f32) -> f32 {
    2f32.powf(st / 12.0)
}

/// Per-sample decay multiplier for a time constant in seconds.
fn decay_coef(seconds: f32, sample_rate: f32) -> f32 {
    (-1.0 / (seconds.max(1e-4) * sample_rate)).exp()
}

/// PolyBLEP residual for band-limited saw and square waves.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// What a step leaves around itself once its top is taken off, two samples
/// either side of it (`x`, in samples from the step): a wider and smoother
/// correction than [`poly_blep`]'s one sample, which folds far less back.
fn step_residue(x: f32) -> f32 {
    // The integral of a cubic B-spline up to `-|x|`.
    let a = x.abs();
    let below = if a >= 2.0 {
        0.0
    } else if a >= 1.0 {
        (2.0 - a).powi(4) / 24.0
    } else {
        0.5 - 2.0 * a / 3.0 + a * a * a / 3.0 - a.powi(4) / 8.0
    };
    if x < 0.0 { below } else { -below }
}

/// A saw as [`saw`], its step corrected over four samples (see [`step_residue`]).
pub(crate) fn saw_clean(phase: f32, inc: f32) -> f32 {
    let mut out = 2.0 * phase - 1.0;
    if phase < 2.0 * inc {
        out -= 2.0 * step_residue(phase / inc);
    }
    if phase > 1.0 - 2.0 * inc {
        out -= 2.0 * step_residue((phase - 1.0) / inc);
    }
    out
}

pub(crate) fn saw(phase: f32, inc: f32) -> f32 {
    2.0 * phase - 1.0 - poly_blep(phase, inc)
}

pub(crate) fn square(phase: f32, inc: f32) -> f32 {
    let raw = if phase < 0.5 { 1.0 } else { -1.0 };
    raw + poly_blep(phase, inc) - poly_blep((phase + 0.5) % 1.0, inc)
}

/// Topology-preserving-transform state variable filter returning (low, band, high).
#[derive(Clone, Copy, Default)]
pub(crate) struct Svf {
    pub(crate) ic1: f32,
    pub(crate) ic2: f32,
}

impl Svf {
    pub(crate) fn tick(&mut self, x: f32, g: f32, k: f32) -> (f32, f32, f32) {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, x - k * v1 - v2)
    }
}

pub(crate) fn svf_g(cutoff: f32, sample_rate: f32) -> f32 {
    (PI * cutoff.clamp(20.0, sample_rate * 0.45) / sample_rate).tan()
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Linear ADSR envelope.
#[derive(Clone, Copy)]
pub(crate) struct Adsr {
    pub(crate) stage: Stage,
    pub(crate) level: f32,
    release_step: f32,
}

impl Adsr {
    pub(crate) const IDLE: Adsr = Adsr { stage: Stage::Idle, level: 0.0, release_step: 0.0 };

    pub(crate) fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }

    pub(crate) fn release(&mut self, release: f32, sample_rate: f32) {
        if !matches!(self.stage, Stage::Idle | Stage::Release) {
            self.stage = Stage::Release;
            self.release_step = self.level / (release.max(0.001) * sample_rate);
        }
    }

    /// Fades out over `seconds` from wherever it is, a release under way too.
    pub(crate) fn cut(&mut self, seconds: f32, sample_rate: f32) {
        let step = self.level / (seconds.max(0.0005) * sample_rate);
        match self.stage {
            Stage::Idle => {}
            // Already fading: no slower than that.
            Stage::Release => self.release_step = self.release_step.max(step),
            _ => {
                self.stage = Stage::Release;
                self.release_step = step;
            }
        }
    }

    pub(crate) fn tick(&mut self, attack: f32, decay: f32, sustain: f32, sample_rate: f32) -> f32 {
        match self.stage {
            Stage::Attack => {
                self.level += 1.0 / (attack.max(0.0005) * sample_rate);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level -= (1.0 - sustain) / (decay.max(0.001) * sample_rate);
                if self.level <= sustain {
                    self.level = sustain;
                    self.stage = if sustain <= 0.0 { Stage::Idle } else { Stage::Sustain };
                }
            }
            Stage::Sustain => self.level = sustain,
            Stage::Release => {
                self.level -= self.release_step;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
            Stage::Idle => {}
        }
        self.level
    }
}

/// Takes the click out of a voice that starts a new note while it still
/// sounds (stolen, or a mono voice played again): its output would jump from
/// where the old note was to where the new one starts. That jump is added
/// back and faded out over `DECLICK` seconds, so the sound stays continuous.
#[derive(Clone, Copy, Default)]
pub(crate) struct Declick {
    /// The jump being faded, per channel.
    jump: [f32; 2],
    /// Samples of the fade done.
    at: u32,
}

/// How long a jump takes to fade, in seconds.
const DECLICK: f32 = 0.003;

impl Declick {
    fn shape(&self, total: f32) -> f32 {
        0.5 + 0.5 * (PI * (self.at as f32 / total).min(1.0)).cos()
    }

    /// How much of a jump is left `at` samples into its fade, 1 to 0: a
    /// voice that can fade its new note in by the rest (see [`Taken::cut`])
    /// cross-fades, and never sounds louder than the louder of the two.
    pub(crate) fn weight(at: u32, sample_rate: f32) -> f32 {
        0.5 + 0.5 * (PI * (at as f32 / (DECLICK * sample_rate)).min(1.0)).cos()
    }

    /// A voice jumped by `-jump` on the sample about to be played.
    pub(crate) fn push(&mut self, jump: [f32; 2], sample_rate: f32) {
        let now = self.shape(DECLICK * sample_rate);
        self.jump = [self.jump[0] * now + jump[0], self.jump[1] * now + jump[1]];
        self.at = 0;
    }

    /// What it adds to the next sample.
    fn next(&mut self, total: f32) -> [f32; 2] {
        let shape = self.shape(total);
        let add = [self.jump[0] * shape, self.jump[1] * shape];
        self.at += 1;
        if self.at as f32 >= total {
            *self = Self::default();
        }
        add
    }

    pub(crate) fn render(&mut self, left: &mut [f32], right: &mut [f32], sample_rate: f32) {
        let total = DECLICK * sample_rate;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.jump == [0.0; 2] {
                return;
            }
            let add = self.next(total);
            *l += add[0];
            *r += add[1];
        }
    }

    /// The same for an instrument with one channel.
    pub(crate) fn render_mono(&mut self, out: &mut [f32], sample_rate: f32) {
        let total = DECLICK * sample_rate;
        for sample in out.iter_mut() {
            if self.jump == [0.0; 2] {
                return;
            }
            *sample += self.next(total)[0];
        }
    }
}

/// What a voice needs to be taken without a click: its last output, and
/// whether a new note just took it while it sounded.
#[derive(Clone, Copy, Default)]
pub(crate) struct Taken {
    out: [f32; 2],
    taken: bool,
}

impl Taken {
    pub(crate) const NONE: Taken = Taken { out: [0.0; 2], taken: false };

    /// A new note starts on the voice, which was `sounding` or not.
    pub(crate) fn take(&mut self, sounding: bool) {
        self.taken = sounding;
        if !sounding {
            self.out = [0.0; 2];
        }
    }

    /// After the voice rendered into `left`/`right`, watched from before:
    /// the jump of a voice just taken goes to `declick`.
    pub(crate) fn rendered(&mut self, watch: Watch, left: &[f32], right: &[f32], declick: &mut Declick, sample_rate: f32) {
        if left.is_empty() {
            return;
        }
        let (first, last) = watch.after(left, right);
        self.seen(first, last, declick, sample_rate);
    }

    /// The same, told the voice's `first` and `last` samples.
    pub(crate) fn seen(&mut self, first: [f32; 2], last: [f32; 2], declick: &mut Declick, sample_rate: f32) {
        if self.taken {
            declick.push([self.out[0] - first[0], self.out[1] - first[1]], sample_rate);
            self.taken = false;
        }
        self.out = last;
    }

    /// The voice is cut off where it is, with no note taking it: what it
    /// was playing fades out through `declick`.
    pub(crate) fn cut(&mut self, declick: &mut Declick, sample_rate: f32) {
        if self.out != [0.0; 2] {
            declick.push(self.out, sample_rate);
        }
        *self = Self::NONE;
    }
}

/// A voice's output watched from outside its render: what it added to the
/// first and last samples of a buffer the voices add into.
pub(crate) struct Watch([f32; 2], [f32; 2]);

impl Watch {
    pub(crate) fn before(left: &[f32], right: &[f32]) -> Self {
        let n = left.len().max(1) - 1;
        Self([left.first().copied().unwrap_or(0.0), right.first().copied().unwrap_or(0.0)], [left.get(n).copied().unwrap_or(0.0), right.get(n).copied().unwrap_or(0.0)])
    }

    /// After the voice rendered: (its first sample, its last).
    pub(crate) fn after(&self, left: &[f32], right: &[f32]) -> ([f32; 2], [f32; 2]) {
        let now = Self::before(left, right);
        ([now.0[0] - self.0[0], now.0[1] - self.0[1]], [now.1[0] - self.1[0], now.1[1] - self.1[1]])
    }
}

/// Note bookkeeping shared by the polyphonic instruments.
#[derive(Clone, Copy)]
pub(crate) struct Gate {
    pub(crate) pitch: u8,
    pub(crate) velocity: f32,
    pub(crate) held: bool,
    remaining: u32,
    pub(crate) age: u64,
}

impl Gate {
    pub(crate) const OFF: Gate = Gate { pitch: 0, velocity: 0.0, held: false, remaining: 0, age: 0 };

    pub(crate) fn new(pitch: u8, velocity: f32, gate: u32, age: u64) -> Self {
        Self { pitch, velocity, held: gate == HELD, remaining: gate, age }
    }

    /// Still open, held or timed, for more than `margin` samples: the next
    /// note overlaps this one (a margin keeps notes that only touch apart).
    pub(crate) fn open_for(&self, margin: u32) -> bool {
        self.held || self.remaining > margin
    }

    /// Let go now, held or timed (also a mono voice cut by the next note).
    pub(crate) fn close(&mut self) {
        self.held = false;
        self.remaining = 0;
    }

    /// Advances one sample; true when a timed gate just closed.
    pub(crate) fn tick(&mut self) -> bool {
        if self.held || self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        self.remaining == 0
    }
}

/// Picks the voice to (re)use: an idle one, else the oldest already let go,
/// else the oldest. Each voice says (index, idle, still held, age).
pub(crate) fn steal_voice(voices: impl Iterator<Item = (usize, bool, bool, u64)>) -> usize {
    let mut best = (0, true, u64::MAX);
    for (index, is_idle, held, age) in voices {
        if is_idle {
            return index;
        }
        if (held, age) < (best.1, best.2) {
            best = (index, held, age);
        }
    }
    best.0
}

// ----- Super Saw ------------------------------------------------------------------------

const SAW_VOICES: usize = 8;
const UNISON: usize = 7;
/// The Super Saw's filter follows its envelope every `CONTROL` samples,
/// counted from the synth's start (not from each buffer's), so the sound
/// doesn't depend on the buffer.
const CONTROL: usize = 32;

/// How far a saw sits from the note, -1..1, for its place in the row
/// (-1..1, evenly). Uneven: closer towards the middle, and by a different
/// curve and a little short above it, so no two pairs of saws beat at the
/// same rate and the beating doesn't come round. (The curves are the ones
/// that repeat least, of those tried, with four to seven saws.)
fn saw_offset(position: f32, uneven: bool) -> f32 {
    if !uneven {
        position
    } else if position > 0.0 {
        0.93 * position.powf(1.2)
    } else {
        -(-position).powf(1.3)
    }
}

#[derive(Clone, Copy)]
struct SawVoice {
    gate: Gate,
    env: Adsr,
    phases: [f32; UNISON],
    filters: [Svf; 2],
    /// The filter's coefficient for its control period (below zero: not found yet).
    g: f32,
    taken: Taken,
}

pub struct SuperSaw {
    params: SuperSawParams,
    voices: [SawVoice; SAW_VOICES],
    sample_rate: f32,
    counter: u64,
    seed: u32,
    /// Pitch bend and vibrato, as a frequency ratio.
    pitch: f32,
    /// What the controllers do to the cutoff, as a ratio.
    bright: f32,
    /// How far into a control period the last buffer ended.
    period_at: usize,
    declick: Declick,
}

impl SuperSaw {
    pub fn new(params: SuperSawParams, sample_rate: f32) -> Self {
        let voice = SawVoice { gate: Gate::OFF, env: Adsr::IDLE, phases: [0.0; UNISON], filters: [Svf::default(); 2], g: -1.0, taken: Taken::NONE };
        Self { params, voices: [voice; SAW_VOICES], sample_rate, counter: 0, seed: 0x2545_F491, pitch: 1.0, bright: 1.0, period_at: 0, declick: Declick::default() }
    }

    /// Starts its luck over (the transport starts): the song's notes start
    /// with the same phases every time it is played from the same place.
    pub fn reseed(&mut self) {
        self.seed = 0x2545_F491;
        self.counter = 0;
    }

    /// Bends every note by `ratio` (a frequency multiplier: pitch bend and vibrato).
    pub fn set_pitch(&mut self, ratio: f32) {
        self.pitch = ratio;
    }

    /// Opens (above 1) or closes the filter by `ratio` (brightness, CC 74, and pressure).
    pub fn set_brightness(&mut self, ratio: f32) {
        self.bright = ratio;
    }

    pub fn params(&self) -> SuperSawParams {
        self.params
    }

    pub fn set_params(&mut self, params: SuperSawParams) {
        self.params = params;
    }

    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32, gate: u32) {
        self.counter += 1;
        let index = steal_voice(self.voices.iter().enumerate().map(|(i, v)| (i, v.env.stage == Stage::Idle, v.env.stage != Stage::Release, v.gate.age)));
        // Free-running, random phases are what makes unison saws sound wide.
        let mut phases = [0.0; UNISON];
        for phase in &mut phases {
            *phase = self.random();
        }
        let voice = &mut self.voices[index];
        // A voice at rest starts clean: its filters stopped where its last
        // note faded out (and would color this one's start).
        if voice.env.stage == Stage::Idle {
            voice.filters = [Svf::default(); 2];
        }
        voice.taken.take(voice.env.stage != Stage::Idle);
        voice.gate = Gate::new(pitch, velocity, gate, self.counter);
        voice.phases = phases;
        voice.g = -1.0;
        voice.env.trigger();
    }

    pub fn note_off(&mut self, pitch: u8) {
        let (release, sr) = (self.params.release, self.sample_rate);
        for v in self.voices.iter_mut().filter(|v| v.gate.held && v.gate.pitch == pitch) {
            v.gate.held = false;
            v.env.release(release, sr);
        }
    }

    pub fn all_notes_off(&mut self) {
        let (release, sr) = (self.params.release, self.sample_rate);
        for v in &mut self.voices {
            v.gate.held = false;
            v.env.release(release, sr);
        }
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = self.params;
        let sr = self.sample_rate;
        let count = (p.voices.round() as usize).clamp(1, UNISON);
        let norm = 1.0 / (count as f32).sqrt();
        let k = 2.0 - 1.95 * p.resonance.clamp(0.0, 1.0);
        let pitch = self.pitch;
        for voice in self.voices.iter_mut().filter(|v| v.env.stage != Stage::Idle) {
            let freq = midi_to_freq(voice.gate.pitch) * pitch;
            // Saws spread symmetrically in pitch (up to ±50 cents) and across the stereo field.
            let mut incs = [0f32; UNISON];
            let mut gains = [(0f32, 0f32); UNISON];
            for u in 0..count {
                let position = if count == 1 { 0.0 } else { u as f32 / (count - 1) as f32 * 2.0 - 1.0 };
                incs[u] = freq * semitones(saw_offset(position, p.uneven) * p.detune * 0.5) / sr;
                let angle = (position * p.spread + 1.0) * std::f32::consts::FRAC_PI_4;
                gains[u] = (angle.cos() * norm, angle.sin() * norm);
            }
            let watch = Watch::before(left, right);
            let (mut offset, mut at) = (0, self.period_at);
            while offset < left.len() {
                let n = (left.len() - offset).min(CONTROL - at);
                // The cutoff moves at the start of a period (or of a note); a
                // period cut in two by the end of a buffer keeps it.
                if at == 0 || voice.g < 0.0 {
                    let cutoff = p.cutoff * self.bright * 2f32.powf(p.env_amount * voice.env.level);
                    voice.g = svf_g(cutoff, sr);
                }
                let g = voice.g;
                let (block_l, block_r) = (&mut left[offset..offset + n], &mut right[offset..offset + n]);
                offset += n;
                at = (at + n) % CONTROL;
                for (out_l, out_r) in block_l.iter_mut().zip(block_r.iter_mut()) {
                    if voice.gate.tick() {
                        voice.env.release(p.release, sr);
                    }
                    let level = voice.env.tick(p.attack, p.decay, p.sustain, sr);
                    let (mut l, mut r) = (0.0, 0.0);
                    for u in 0..count {
                        let s = saw(voice.phases[u], incs[u]);
                        voice.phases[u] += incs[u];
                        if voice.phases[u] >= 1.0 {
                            voice.phases[u] -= 1.0;
                        }
                        l += s * gains[u].0;
                        r += s * gains[u].1;
                    }
                    let (fl, _, _) = voice.filters[0].tick(l, g, k);
                    let (fr, _, _) = voice.filters[1].tick(r, g, k);
                    let amp = level * voice.gate.velocity * p.gain;
                    *out_l += fl * amp;
                    *out_r += fr * amp;
                }
            }
            voice.taken.rendered(watch, left, right, &mut self.declick, sr);
        }
        self.declick.render(left, right, sr);
        self.period_at = (self.period_at + left.len()) % CONTROL;
    }
}

// ----- Acid Bass ------------------------------------------------------------------------

/// Monophonic bassline voice: overlapping notes slide in pitch without retriggering.
pub struct AcidBass {
    params: AcidParams,
    sample_rate: f32,
    gate: Gate,
    gate_open: bool,
    freq: f32,
    target: f32,
    phase: f32,
    amp: f32,
    filter_env: f32,
    accent: f32,
    ladder: [f32; 4],
    /// Pitch bend and vibrato, as a frequency ratio.
    pitch: f32,
    /// What the controllers do to the cutoff, as a ratio.
    bright: f32,
}

impl AcidBass {
    pub fn new(params: AcidParams, sample_rate: f32) -> Self {
        Self {
            params,
            sample_rate,
            gate: Gate::OFF,
            gate_open: false,
            freq: 110.0,
            target: 110.0,
            phase: 0.0,
            amp: 0.0,
            filter_env: 0.0,
            accent: 0.0,
            ladder: [0.0; 4],
            pitch: 1.0,
            bright: 1.0,
        }
    }

    /// Bends every note by `ratio` (a frequency multiplier: pitch bend and vibrato).
    pub fn set_pitch(&mut self, ratio: f32) {
        self.pitch = ratio;
    }

    /// Opens (above 1) or closes the filter by `ratio` (brightness, CC 74, and pressure).
    pub fn set_brightness(&mut self, ratio: f32) {
        self.bright = ratio;
    }

    pub fn params(&self) -> AcidParams {
        self.params
    }

    pub fn set_params(&mut self, params: AcidParams) {
        self.params = params;
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32, gate: u32) {
        let slide = self.gate_open;
        self.target = midi_to_freq(pitch);
        if !slide {
            self.freq = self.target;
            self.filter_env = 1.0;
        }
        self.accent = if velocity > 0.8 { self.params.accent } else { 0.0 };
        self.gate = Gate::new(pitch, velocity, gate, 0);
        self.gate_open = true;
    }

    pub fn note_off(&mut self, pitch: u8) {
        if self.gate.held && self.gate.pitch == pitch {
            self.gate.held = false;
            self.gate_open = false;
        }
    }

    pub fn all_notes_off(&mut self) {
        self.gate.held = false;
        self.gate_open = false;
    }

    /// Silent: the gate closed and the release faded out.
    fn silent(&self) -> bool {
        !self.gate_open && self.amp < 1e-5
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.silent() {
            return;
        }
        let p = self.params;
        let sr = self.sample_rate;
        let glide = 1.0 - decay_coef(p.glide_ms * 0.001, sr);
        let env_decay = decay_coef(p.decay * if self.accent > 0.0 { 0.5 } else { 1.0 }, sr);
        let attack = 1.0 - decay_coef(0.003, sr);
        let release = 1.0 - decay_coef(0.008, sr);
        let feedback = 4.0 * p.resonance.clamp(0.0, 0.99);
        let drive = 1.0 + p.drive * 4.0;
        let level = p.gain * (1.0 + self.accent * 0.6);
        for (out_l, out_r) in left.iter_mut().zip(right.iter_mut()) {
            if self.gate.tick() {
                self.gate_open = false;
            }
            let target_amp = if self.gate_open { 1.0 } else { 0.0 };
            self.amp += (target_amp - self.amp) * if self.gate_open { attack } else { release };
            self.freq += (self.target - self.freq) * glide;
            let inc = self.freq * self.pitch / sr;
            let osc = if p.square { square(self.phase, inc) } else { saw(self.phase, inc) };
            self.phase = (self.phase + inc) % 1.0;

            self.filter_env *= env_decay;
            let sweep = 4.0 * p.env_mod * (1.0 + self.accent) * self.filter_env;
            let cutoff = (p.cutoff * self.bright * 2f32.powf(sweep)).clamp(20.0, sr * 0.4);
            // Four one-pole stages with resonant feedback and a saturating input.
            let filtered = if p.tuned {
                // The feedback takes what the stages give this very sample,
                // so the resonance stays on the cutoff all the way up.
                let g = (PI * cutoff / sr).tan();
                let (share, keep) = (g / (1.0 + g), 1.0 / (1.0 + g));
                let held = keep * (share * (share * (share * self.ladder[0] + self.ladder[1]) + self.ladder[2]) + self.ladder[3]);
                let mut y = ((osc * drive - feedback * held) / (1.0 + feedback * share.powi(4))).tanh();
                for stage in &mut self.ladder {
                    let v = (y - *stage) * share;
                    y = v + *stage;
                    *stage = y + v;
                }
                y
            } else {
                let g = 1.0 - (-TAU * cutoff / sr).exp();
                let input = (osc * drive - feedback * self.ladder[3]).tanh();
                self.ladder[0] += g * (input - self.ladder[0]);
                self.ladder[1] += g * (self.ladder[0] - self.ladder[1]);
                self.ladder[2] += g * (self.ladder[1] - self.ladder[2]);
                self.ladder[3] += g * (self.ladder[2] - self.ladder[3]);
                self.ladder[3]
            };
            // Resonance thins the low end; compensate a little.
            let out = filtered * (1.0 + p.resonance) * self.amp * level;
            *out_l += out;
            *out_r += out;
            // Stopping on the sample it falls silent, not at the end of
            // whatever block that was, and from a known state: the next note
            // starts the same with any buffer size.
            if self.silent() {
                self.amp = 0.0;
                self.phase = 0.0;
                self.ladder = [0.0; 4];
                self.filter_env = 0.0;
                break;
            }
        }
    }
}

// ----- FM Synth -------------------------------------------------------------------------

const FM_VOICES: usize = 8;

/// How much of the modulation index velocity takes away: a full hit plays the
/// index as set, the softest about half of it (darker, as on a DX7).
const FM_VELOCITY_INDEX: f32 = 0.5;
/// Octaves of modulation index the brightness controller (CC 74) moves either way
/// and full pressure moves up; the index CC 74 at the top and full pressure add,
/// on top of the envelope (so held notes and soft patches brighten too), over
/// the ratio (a high ratio's sidebands are far apart: the same index there is
/// a much brighter sound). Measured on the factory presets, CC 74 at the top
/// lifts the spectral centroid some 80-130 %, and full pressure 60-90 %.
const FM_BRIGHT_OCTAVES: f32 = 1.0;
const FM_PRESSURE_OCTAVES: f32 = 0.7;
const FM_BRIGHT_INDEX: f32 = 1.5;
const FM_PRESSURE_INDEX: f32 = 1.0;
/// How fast the controllers move the index: about 5 ms, so it never clicks.
const FM_GLIDE: f32 = 0.005;

#[derive(Clone, Copy)]
struct FmVoice {
    gate: Gate,
    env: Adsr,
    carrier: f32,
    modulator: f32,
    last_mod: f32,
    /// The modulator's sample before `last_mod`.
    older_mod: f32,
    mod_env: f32,
    taken: Taken,
}

pub struct FmSynth {
    params: FmParams,
    voices: [FmVoice; FM_VOICES],
    sample_rate: f32,
    counter: u64,
    /// Pitch bend and vibrato, as a frequency ratio.
    pitch: f32,
    /// What the controllers do to the index: a ratio and an amount added, as
    /// asked (see [`Self::set_expression`]) and as now, gliding to it.
    target: (f32, f32),
    depth: (f32, f32),
    declick: Declick,
}

impl FmSynth {
    pub fn new(params: FmParams, sample_rate: f32) -> Self {
        let voice = FmVoice { gate: Gate::OFF, env: Adsr::IDLE, carrier: 0.0, modulator: 0.0, last_mod: 0.0, older_mod: 0.0, mod_env: 0.0, taken: Taken::default() };
        Self { params, voices: [voice; FM_VOICES], sample_rate, counter: 0, pitch: 1.0, target: (1.0, 0.0), depth: (1.0, 0.0), declick: Declick::default() }
    }

    /// Brightness (`tone`, -1..1, at rest 0: CC 74) scales the modulation index
    /// (and above rest adds to it), and `pressure` (0..1) adds to it: both
    /// brighten the sound.
    pub fn set_expression(&mut self, tone: f32, pressure: f32) {
        let (tone, pressure) = (tone.clamp(-1.0, 1.0), pressure.clamp(0.0, 1.0));
        self.target = (2f32.powf(tone * FM_BRIGHT_OCTAVES + pressure * FM_PRESSURE_OCTAVES), tone.max(0.0) * FM_BRIGHT_INDEX + pressure * FM_PRESSURE_INDEX);
    }

    /// Bends every note by `ratio` (a frequency multiplier: pitch bend and vibrato).
    pub fn set_pitch(&mut self, ratio: f32) {
        self.pitch = ratio;
    }

    pub fn params(&self) -> FmParams {
        self.params
    }

    pub fn set_params(&mut self, params: FmParams) {
        self.params = params;
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32, gate: u32) {
        self.counter += 1;
        let index = steal_voice(self.voices.iter().enumerate().map(|(i, v)| (i, v.env.stage == Stage::Idle, v.env.stage != Stage::Release, v.gate.age)));
        let voice = &mut self.voices[index];
        voice.taken.take(voice.env.stage != Stage::Idle);
        *voice = FmVoice { gate: Gate::new(pitch, velocity, gate, self.counter), mod_env: 1.0, ..*voice };
        voice.env.trigger();
    }

    pub fn note_off(&mut self, pitch: u8) {
        let (release, sr) = (self.params.release, self.sample_rate);
        for v in self.voices.iter_mut().filter(|v| v.gate.held && v.gate.pitch == pitch) {
            v.gate.held = false;
            v.env.release(release, sr);
        }
    }

    pub fn all_notes_off(&mut self) {
        let (release, sr) = (self.params.release, self.sample_rate);
        for v in &mut self.voices {
            v.gate.held = false;
            v.env.release(release, sr);
        }
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = self.params;
        let sr = self.sample_rate;
        let mod_decay = decay_coef(p.mod_decay, sr);
        let pitch = self.pitch;
        let (start, target) = (self.depth, self.target);
        // At rest the index is the preset's, untouched.
        let still = start == target && target == (1.0, 0.0);
        let spread = 1.0 / p.ratio.max(1.0);
        let glide = 1.0 - (-1.0 / (FM_GLIDE * sr)).exp();
        let step = |(scale, add): (f32, f32)| -> (f32, f32) {
            let next = (scale + (target.0 - scale) * glide, add + (target.1 - add) * glide);
            let settle = |x: f32, to: f32| if (x - to).abs() < 1e-6 { to } else { x };
            (settle(next.0, target.0), settle(next.1, target.1))
        };
        for voice in self.voices.iter_mut().filter(|v| v.env.stage != Stage::Idle) {
            let freq = midi_to_freq(voice.gate.pitch) * pitch;
            let (carrier_inc, mod_inc) = (freq / sr, freq * p.ratio / sr);
            let index = p.index * (1.0 - FM_VELOCITY_INDEX * (1.0 - voice.gate.velocity.clamp(0.0, 1.0)));
            let mut depth = start;
            let watch = Watch::before(left, right);
            for (out_l, out_r) in left.iter_mut().zip(right.iter_mut()) {
                if voice.gate.tick() {
                    voice.env.release(p.release, sr);
                }
                let level = voice.env.tick(p.attack, p.decay, p.sustain, sr);
                if voice.env.stage == Stage::Idle {
                    break;
                }
                voice.mod_env = p.mod_sustain + (voice.mod_env - p.mod_sustain) * mod_decay;
                // The mean of its last two samples: a feedback loop of one
                // sample turns to noise at half the sample rate.
                let fed = if p.feedback_smooth { 0.5 * (voice.last_mod + voice.older_mod) } else { voice.last_mod };
                let modulator = (TAU * voice.modulator + p.feedback * PI * fed).sin();
                voice.older_mod = voice.last_mod;
                voice.last_mod = modulator;
                let amount = if still {
                    index * voice.mod_env
                } else {
                    depth = step(depth);
                    index * voice.mod_env * depth.0 + depth.1 * spread
                };
                let out = (TAU * voice.carrier + amount * modulator).sin();
                voice.carrier = (voice.carrier + carrier_inc) % 1.0;
                voice.modulator = (voice.modulator + mod_inc) % 1.0;
                let s = out * level * voice.gate.velocity * p.gain;
                *out_l += s;
                *out_r += s;
            }
            voice.taken.rendered(watch, left, right, &mut self.declick, sr);
        }
        self.declick.render(left, right, sr);
        // The glide moves on with the frames, sounding or not.
        if !still {
            for _ in 0..left.len() {
                self.depth = step(self.depth);
            }
        }
    }
}

// ----- Drum Machine ---------------------------------------------------------------------

/// Hat/cowbell oscillator frequencies of classic analog drum machines (a "metallic" chord).
const METAL: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

#[derive(Clone, Copy, Default)]
struct Hit {
    active: bool,
    t: f32,
    env: f32,
    velocity: f32,
    phase: f32,
    phase2: f32,
    /// Base pitch, used by the toms to cover low, mid and high.
    base: f32,
    hp_in: f32,
    hp_out: f32,
}

impl Hit {
    fn trigger(&mut self, velocity: f32, base: f32) {
        *self = Hit { active: true, env: 1.0, velocity, base, ..Default::default() };
    }

    fn highpass(&mut self, x: f32, coef: f32) -> f32 {
        let y = coef * (self.hp_out + x - self.hp_in);
        self.hp_in = x;
        self.hp_out = y;
        y
    }
}

pub struct DrumMachine {
    params: DrumParams,
    /// The 808 and 909 models (the classic sounds are below).
    kit: crate::drumkit::ModelKit,
    sample_rate: f32,
    seed: u32,
    hits: [Hit; 8],
    metal_phases: [f32; 6],
    hat_filter: Svf,
    /// The open hat's, when it plays apart from the closed one (see `apart`).
    open_hat_filter: Svf,
    bell_filter: Svf,
    /// The sounds other tracks take, out of its own output.
    apart: crate::drumkit::Apart,
    /// Pitch bend now, -1..1.
    bend: f32,
    /// Semitones each sound's last hit was bent by (each sound has one voice).
    hit_tune: [f32; crate::model::DRUM_SOUNDS],
}

impl DrumMachine {
    pub fn new(params: DrumParams, sample_rate: f32) -> Self {
        Self {
            params,
            kit: crate::drumkit::ModelKit::new(params.model, sample_rate),
            sample_rate,
            seed: 0x9E37_79B9,
            hits: [Hit::default(); 8],
            metal_phases: [0.0; 6],
            hat_filter: Svf::default(),
            open_hat_filter: Svf::default(),
            bell_filter: Svf::default(),
            apart: crate::drumkit::Apart::new(),
            bend: 0.0,
            hit_tune: [0.0; crate::model::DRUM_SOUNDS],
        }
    }

    /// Pitch bend, -1..1: the hits that start from now on take it, each sound
    /// as far as its bend range (`DrumParams::bend`) says.
    pub fn set_bend(&mut self, bend: f32) {
        self.bend = bend.clamp(-1.0, 1.0);
    }

    /// Its settings with each sound tuned as its last hit was (bent).
    fn played(&self) -> DrumParams {
        let mut p = self.params;
        for (tune, bent) in p.tune.iter_mut().zip(self.hit_tune) {
            *tune += bent;
        }
        p
    }

    /// A new block, the sounds in `mask` (a bit each, in `DRUM_VOICES` order)
    /// apart from its output: for other tracks to play (see `sound`).
    pub fn begin(&mut self, mask: u16) {
        self.apart.begin(mask);
    }

    /// Sound `voice` this block, if it is apart.
    pub fn sound(&self, voice: usize) -> &[f32] {
        self.apart.sound(voice)
    }

    pub fn params(&self) -> DrumParams {
        self.params
    }

    /// Its noise from the start again (see `ModelKit::reseed`).
    pub fn reseed(&mut self) {
        self.seed = 0x9E37_79B9;
        if !(self.hits[4].active || self.hits[5].active || self.hits[7].active) {
            self.metal_phases = [0.0; 6];
        }
        self.kit.reseed();
    }

    pub fn set_params(&mut self, params: DrumParams) {
        self.params = params;
        self.kit.set_model(params.model);
    }

    fn noise(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32) {
        use drum_note::*;
        let (voice, base) = match pitch {
            KICK | 35 => (0, 1.0),
            RIM => (1, 1.0),
            SNARE | 40 => (2, 1.0),
            CLAP => (3, 1.0),
            CLOSED_HAT | 44 => (4, 1.0),
            OPEN_HAT => (5, 1.0),
            LOW_TOM | 41 | 43 => (6, 1.0),
            COWBELL => (7, 1.0),
            MID_TOM | 47 => (8, 1.0),
            HIGH_TOM => (9, 1.0),
            CRASH | 57 | 55 | 52 => (10, 1.0),
            RIDE | 59 | 53 => (11, 1.0),
            SHAKER | 69 | 82 => (12, 1.0),
            TAMBOURINE => (13, 1.0),
            CONGA | 62 => (14, 1.0),
            64 => (14, 0.75),
            CLAVE | 76 | 77 => (15, 1.0),
            _ => return,
        };
        self.hit_tune[voice] = self.bend * self.params.bend[voice];
        // The classic machine has its own first eight; the rest are the models'.
        if self.params.model != crate::model::DrumModel::Classic || voice >= 8 {
            let params = self.played();
            self.kit.hit(voice, velocity, base, &params);
            return;
        }
        if voice == 4 {
            // A closed hat chokes the open one, as on the real machines.
            self.hits[5].active = false;
        }
        self.hits[voice].trigger(velocity, base);
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        // The models ring out even if the model changes meanwhile.
        let params = self.played();
        self.kit.render(&params, &mut self.apart, left, right);
        let sr = self.sample_rate;
        let dt = 1.0 / sr;
        let p = params;
        let tune = |v: usize| semitones(p.tune[v]);
        let coef = |v: usize, seconds: f32| decay_coef(seconds * p.decay[v], sr);
        let decays = [
            coef(0, 0.45),
            coef(1, 0.03),
            coef(2, 0.16),
            coef(3, 0.12),
            coef(4, 0.045),
            coef(5, 0.35),
            coef(6, 0.25),
            coef(7, 0.12),
        ];
        let hat_g = svf_g(7500.0 * tune(4), sr);
        let bell_g = svf_g(1200.0 * tune(7), sr);
        let any_metal = self.hits[4].active || self.hits[5].active || self.hits[7].active;

        // The open hat on its own when it is apart (filtering both together, as
        // the machine does, is the same: the filter is linear).
        let open_apart = self.apart.takes(5);
        for (i, (out_l, out_r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let mut per = [0f32; crate::model::DRUM_SOUNDS];
            let noise = self.noise();

            // Shared metallic oscillators for the hats and the cowbell.
            // They run all the time (not only while a hat sounds), so where
            // they are when one starts doesn't depend on how the audio was
            // cut into blocks: the export sounds like the playback.
            let mut metal = 0.0;
            for (phase, &f) in self.metal_phases.iter_mut().zip(&METAL) {
                *phase = (*phase + f * dt * 1.5) % 1.0;
                metal += if *phase < 0.5 { 1.0 } else { -1.0 };
            }
            metal /= 6.0;
            let metal = if any_metal { metal } else { 0.0 };

            let k = &mut self.hits[0];
            if k.active {
                let freq = 48.0 * tune(0) + 120.0 * tune(0) * (-k.t * 28.0).exp();
                k.phase = (k.phase + freq * dt) % 1.0;
                let click = if k.t < 0.003 { noise * 0.4 } else { 0.0 };
                let body = ((k.phase * TAU).sin() * 1.4).tanh();
                per[0] += (body + click) * k.env * k.velocity * p.level[0];
                k.env *= decays[0];
                k.t += dt;
                k.active = k.env > 1e-4;
            }

            let r = &mut self.hits[1];
            if r.active {
                r.phase = (r.phase + 1700.0 * tune(1) * dt) % 1.0;
                r.phase2 = (r.phase2 + 470.0 * tune(1) * dt) % 1.0;
                let tone = (r.phase * TAU).sin() * 0.6 + (r.phase2 * TAU).sin() * 0.4;
                per[1] += (tone + r.highpass(noise, 0.7) * 0.3) * r.env * r.velocity * p.level[1];
                r.env *= decays[1];
                r.active = r.env > 1e-4;
            }

            let sn = &mut self.hits[2];
            if sn.active {
                sn.phase = (sn.phase + 185.0 * tune(2) * dt) % 1.0;
                sn.phase2 = (sn.phase2 + 330.0 * tune(2) * dt) % 1.0;
                let tone = ((sn.phase * TAU).sin() + (sn.phase2 * TAU).sin() * 0.5) * (-sn.t * 22.0).exp() * 0.45;
                let snappy = sn.highpass(noise, 0.85) * 0.6;
                per[2] += (tone + snappy) * sn.env * sn.velocity * p.level[2];
                sn.env *= decays[2];
                sn.t += dt;
                sn.active = sn.env > 1e-4;
            }

            let c = &mut self.hits[3];
            if c.active {
                // Three quick bursts followed by a tail.
                let burst = if c.t < 0.03 { (1.0 - ((c.t * 100.0) % 1.0)).powi(2) } else { 1.0 };
                per[3] += c.highpass(noise, 0.75 + 0.1 * (tune(3) - 1.0).clamp(-1.0, 1.0)) * c.env * burst * c.velocity * p.level[3] * 0.7;
                c.env *= decays[3];
                c.t += dt;
                c.active = c.env > 1e-4;
            }

            let mut hats = [0.0; 2];
            for voice in [4, 5] {
                let h = &mut self.hits[voice];
                if h.active {
                    hats[voice - 4] += (metal * 0.7 + noise * 0.3) * h.env * h.velocity * p.level[voice];
                    h.env *= decays[voice];
                    h.active = h.env > 1e-4;
                }
            }
            if open_apart {
                let (_, _, open) = self.open_hat_filter.tick(hats[1], hat_g, 1.2);
                per[5] += open * 0.5;
                hats[1] = 0.0;
            }
            if hats != [0.0; 2] || any_metal {
                let (_, _, high) = self.hat_filter.tick(hats[0] + hats[1], hat_g, 1.2);
                // Both hats, through the closed one's drive.
                per[4] += high * 0.5;
            }

            let tom = &mut self.hits[6];
            if tom.active {
                let freq = 95.0 * tom.base * tune(6) * (1.0 + 0.6 * (-tom.t * 18.0).exp());
                tom.phase = (tom.phase + freq * dt) % 1.0;
                per[6] += (tom.phase * TAU).sin() * tom.env * tom.velocity * p.level[6] * 0.9;
                tom.env *= decays[6];
                tom.t += dt;
                tom.active = tom.env > 1e-4;
            }

            let bell = &mut self.hits[7];
            if bell.active {
                bell.phase = (bell.phase + 540.0 * tune(7) * dt) % 1.0;
                bell.phase2 = (bell.phase2 + 800.0 * tune(7) * dt) % 1.0;
                let tone = (if bell.phase < 0.5 { 1.0 } else { -1.0 }) + (if bell.phase2 < 0.5 { 1.0 } else { -1.0 });
                let (_, band, _) = self.bell_filter.tick(tone * 0.5, bell_g, 0.8);
                per[7] += band * bell.env * bell.velocity * p.level[7];
                bell.env *= decays[7];
                bell.active = bell.env > 1e-4;
            }

            // -6 dB of headroom: kick and clap often land together.
            let out = self.apart.mix(&per, &p, i, 0.5, f32::INFINITY);
            *out_l += out;
            *out_r += out;
        }
        self.apart.advance(left.len());
    }
}

/// Renders one drum machine hit, trimmed once it has decayed, for building sample kits.
pub fn render_drum_hit(params: DrumParams, pitch: u8, sample_rate: f32) -> (Vec<f32>, Vec<f32>) {
    let mut drums = DrumMachine::new(params, sample_rate);
    drums.note_on(pitch, 1.0);
    let frames = (sample_rate * 4.0) as usize;
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    for (l, r) in left.chunks_mut(512).zip(right.chunks_mut(512)) {
        drums.render(l, r);
    }
    // Keep everything up to the last sample above -80 dB, plus a few milliseconds.
    let last = left.iter().zip(&right).rposition(|(l, r)| l.abs().max(r.abs()) > 1e-4).unwrap_or(0);
    let end = (last + (sample_rate * 0.005) as usize).min(frames);
    left.truncate(end);
    right.truncate(end);
    (left, right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DRUM_VOICES;

    const SR: f32 = 48_000.0;

    fn energy(buffer: &[f32]) -> f32 {
        buffer.iter().map(|s| s * s).sum()
    }

    fn run(render: &mut dyn FnMut(&mut [f32], &mut [f32]), frames: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            render(a, b);
        }
        assert!(l.iter().chain(&r).all(|s| s.is_finite() && s.abs() < 4.0));
        (l, r)
    }

    /// With every voice busy, the note that takes one doesn't click: the
    /// sound moves no faster across that sample than within the notes. And
    /// it takes a voice already let go before a key still held.
    #[test]
    fn a_stolen_voice_does_not_click_and_held_keys_stay() {
        let steepest = |x: &[f32]| x.windows(2).fold(0f32, |m, w| m.max((w[1] - w[0]).abs()));
        let frames = SR as usize / 4;
        let check = |name: &str, before: &[f32], after: &[f32]| {
            let across = (after[0] - before[frames - 1]).abs();
            let within = steepest(&before[frames / 2..]).max(steepest(&after[frames / 2..]));
            println!("{name}: {across:.4} across the new note, {within:.4} within the notes");
            assert!(within > 0.0 && across <= within * 1.5, "{name}: {across} {within}");
        };
        // Sines once the modulation has died away, so the notes themselves
        // move slowly; a new note brings the modulation back at once.
        let mut fm = FmSynth::new(FmParams { index: 3.0, mod_decay: 0.02, mod_sustain: 0.0, sustain: 1.0, ..Default::default() }, SR);
        for k in 0..FM_VOICES as u8 {
            fm.note_on(40 + k, 1.0, HELD);
        }
        let (before, _) = run(&mut |l, r| fm.render(l, r), frames);
        fm.note_on(41, 1.0, HELD);
        let (after, _) = run(&mut |l, r| fm.render(l, r), frames);
        check("fm", &before, &after);

        // One saw a voice, far under the filter: close to a sine.
        let mut saws = SuperSaw::new(SuperSawParams { voices: 1.0, cutoff: 150.0, env_amount: 0.0, sustain: 1.0, ..Default::default() }, SR);
        for k in 0..SAW_VOICES as u8 {
            saws.note_on(64 + k, 1.0, HELD);
        }
        let (before, _) = run(&mut |l, r| saws.render(l, r), frames);
        saws.note_on(65, 1.0, HELD);
        let (after, _) = run(&mut |l, r| saws.render(l, r), frames);
        check("super saw", &before, &after);

        let params = crate::model::SynthParams { waveform: crate::model::Waveform::Sine, sustain: 1.0, cutoff: 20_000.0, ..Default::default() };
        let mut synth = crate::synth::PolySynth::new(params, SR);
        let mono = |synth: &mut crate::synth::PolySynth| {
            let mut out = vec![0.0; frames];
            out.chunks_mut(512).for_each(|chunk| synth.render(chunk));
            out
        };
        for k in 0..16 {
            synth.note_on(40 + k, 1.0, HELD);
        }
        let before = mono(&mut synth);
        synth.note_on(41, 1.0, HELD);
        check("synth", &before, &mono(&mut synth));

        // The oldest key held, the rest let go and still ringing.
        let mut fm = FmSynth::new(FmParams { release: 2.0, ..Default::default() }, SR);
        for k in 0..FM_VOICES as u8 {
            fm.note_on(40 + k, 1.0, HELD);
        }
        run(&mut |l, r| fm.render(l, r), 512);
        for k in 1..FM_VOICES as u8 {
            fm.note_off(40 + k);
        }
        fm.note_on(100, 1.0, HELD);
        assert!(fm.voices.iter().any(|v| v.gate.held && v.gate.pitch == 40), "the held note went");
    }

    /// The FM's feedback stays a clean pitched sound half of the way up (a
    /// sine that turns to a saw), and from there gets rougher little by
    /// little, instead of turning to noise a third of the way: the wave
    /// repeats from one cycle to the next. A sound made before keeps the
    /// feedback it had.
    #[test]
    fn fm_feedback_stays_pitched() {
        // A2 at 48 kHz: 110 Hz, a cycle of 436.4 samples; five cycles are 2182.
        let lag = 2182;
        let repeats = |feedback: f32, feedback_smooth: bool| {
            let params = FmParams { ratio: 1.0, index: 2.0, mod_decay: 5.0, mod_sustain: 1.0, sustain: 1.0, feedback, feedback_smooth, ..Default::default() };
            let mut fm = FmSynth::new(params, SR);
            fm.note_on(45, 1.0, HELD);
            let (l, _) = run(&mut |l, r| fm.render(l, r), SR as usize);
            let x = &l[SR as usize / 2..];
            let (mut ab, mut aa, mut bb) = (0f64, 0f64, 0f64);
            for i in 0..x.len() - lag {
                let (a, b) = (x[i] as f64, x[i + lag] as f64);
                ab += a * b;
                aa += a * a;
                bb += b * b;
            }
            (ab / (aa * bb).sqrt()) as f32
        };
        for feedback in [0.0, 0.2, 0.35, 0.5, 0.7, 0.85, 1.0] {
            println!("feedback {feedback}: repeats {:.3} (before: {:.3})", repeats(feedback, true), repeats(feedback, false));
        }
        for feedback in [0.2, 0.35, 0.5] {
            assert!(repeats(feedback, true) > 0.98, "{feedback}: {}", repeats(feedback, true));
        }
        assert!(repeats(0.7, true) > 0.9 && repeats(1.0, true) > 0.8, "rougher, still pitched");
        assert!(repeats(0.5, false) < 0.85 && repeats(0.7, false) < 0.65, "the old feedback was noise by then");
        // A project saved before has no such setting: the old feedback. So
        // do the factory sounds, made with it.
        let mut saved = serde_json::to_value(FmParams::default()).unwrap();
        assert_eq!(saved.as_object_mut().unwrap().remove("feedback_smooth"), Some(true.into()));
        assert!(!serde_json::from_value::<FmParams>(saved).unwrap().feedback_smooth);
        assert!(FmParams::presets().iter().all(|(name, p)| !p.feedback_smooth || *name == "Init"));
    }

    #[test]
    fn every_drum_sound_plays_and_dies_away() {
        for (pitch, name) in DRUM_VOICES {
            let mut drums = DrumMachine::new(DrumParams::default(), SR);
            drums.note_on(pitch, 1.0);
            let (attack, _) = run(&mut |l, r| drums.render(l, r), 4800);
            assert!(energy(&attack) > 0.1, "{name} should sound");
            let (tail, _) = run(&mut |l, r| drums.render(l, r), 48_000 * 6);
            assert!(energy(&tail[tail.len() - 4800..]) < 1e-6, "{name} should fade out");
        }
    }

    /// A new Super Saw's saws sit at uneven distances, so a held note's
    /// beating never comes round; evenly spaced (the factory sounds and a
    /// saved one that says nothing), it repeats in a loop.
    #[test]
    fn a_new_supersaw_does_not_beat_in_a_loop() {
        // How alike a held note's loudness curve is to itself some time
        // later, at the lag where it is most alike (0.2 to 3 s): 1 is a loop.
        let looping = |uneven: bool, voices: f32| {
            let mut synth = SuperSaw::new(SuperSawParams { voices, uneven, attack: 0.001, sustain: 1.0, cutoff: 18_000.0, resonance: 0.0, ..Default::default() }, SR);
            synth.note_on(69, 1.0, 480_000);
            let (l, r) = run(&mut |l, r| synth.render(l, r), 432_000);
            let curve: Vec<f32> = l.chunks(480).zip(r.chunks(480)).skip(10).map(|(l, r)| (energy(l) + energy(r)).sqrt()).collect();
            let mean = curve.iter().sum::<f32>() / curve.len() as f32;
            let curve: Vec<f32> = curve.iter().map(|v| v - mean).collect();
            let span = curve.len() - 300;
            let alike = |lag: usize| curve[..span].iter().zip(&curve[lag..]).map(|(a, b)| a * b).sum::<f32>() / (energy(&curve[..span]) * energy(&curve[lag..lag + span])).sqrt();
            (20..=300).map(alike).fold(f32::MIN, f32::max)
        };
        for voices in [3.0f32, 4.0, 5.0, 6.0, 7.0] {
            let (before, now) = (looping(false, voices), looping(true, voices));
            println!("{voices} sierras: se parece a sí misma {before:.2} antes y {now:.2} ahora");
            assert!(before > 0.9, "{voices} saws evenly spaced: {before}");
            // Three saws only have two distances to tell apart: they still
            // come round, slowly changing.
            assert!(voices < 4.0 || now < 0.75, "{voices} saws: {now}");
        }
        // As wide as before, and the same with one saw.
        assert_eq!((saw_offset(-1.0, true), saw_offset(0.0, true)), (-1.0, 0.0));
        assert!((0.9..1.0).contains(&saw_offset(1.0, true)));

        // Who has it.
        assert!(SuperSawParams::default().uneven && !SuperSawParams::old().uneven);
        for (name, p) in SuperSawParams::presets() {
            assert_eq!(p.uneven, name == "Init", "{name}");
        }
        let mut old = serde_json::to_value(SuperSawParams::default()).unwrap();
        old.as_object_mut().unwrap().remove("uneven");
        let old: SuperSawParams = serde_json::from_value(old).unwrap();
        assert_eq!(old, SuperSawParams::old());
        let again: SuperSawParams = serde_json::from_str(&serde_json::to_string(&SuperSawParams::default()).unwrap()).unwrap();
        assert!(again.uneven);
    }

    #[test]
    fn supersaw_is_stereo_when_spread() {
        let mut synth = SuperSaw::new(SuperSawParams::default(), SR);
        synth.note_on(60, 1.0, 24_000);
        let (l, r) = run(&mut |l, r| synth.render(l, r), 24_000);
        assert!(energy(&l) > 1.0);
        let difference: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
        assert!(difference > 10.0, "spread saws should differ between channels");
        let (tail, _) = run(&mut |l, r| synth.render(l, r), 48_000);
        assert!(energy(&tail[tail.len() - 2400..]) < 1e-6, "released voices go silent");
    }

    #[test]
    fn acid_slides_between_overlapping_notes_and_retriggers_otherwise() {
        let mut bass = AcidBass::new(AcidParams::default(), SR);
        bass.note_on(36, 0.5, 12_000);
        run(&mut |l, r| bass.render(l, r), 6_000);
        // Overlapping note: slide from C1 towards C2 without restarting the filter envelope.
        let env_before = bass.filter_env;
        bass.note_on(48, 0.5, 12_000);
        assert_eq!(bass.filter_env, env_before);
        assert!(bass.freq < midi_to_freq(40), "the slide starts from the previous pitch");
        run(&mut |l, r| bass.render(l, r), 24_000);
        assert!((bass.freq - midi_to_freq(48)).abs() < 1.0, "and reaches the new one");
        // After the gate closes, the next note retriggers.
        run(&mut |l, r| bass.render(l, r), 4_800);
        bass.note_on(40, 1.0, 1_000);
        assert_eq!(bass.filter_env, 1.0);
        assert_eq!(bass.freq, midi_to_freq(40));
    }

    fn acid_preset(name: &str) -> AcidParams {
        AcidParams::presets().into_iter().find(|(n, _)| *n == name).unwrap().1
    }

    /// The presets built on accent and slide really play them: «Solo acentos» only
    /// squelches on hard notes, «Glide largo» is still on its way a tenth of a second in.
    #[test]
    fn acid_presets_answer_to_accent_and_glide() {
        let first_tenth = |velocity: f32| {
            let mut bass = AcidBass::new(acid_preset("Acid · Solo acentos"), SR);
            bass.note_on(36, velocity, 4_800);
            energy(&run(&mut |l, r| bass.render(l, r), 4_800).0)
        };
        assert!(first_tenth(1.0) > 2.0 * first_tenth(0.6), "accented notes stand out");

        let mut bass = AcidBass::new(acid_preset("Acid · Glide largo"), SR);
        bass.note_on(36, 0.7, 24_000);
        run(&mut |l, r| bass.render(l, r), 2_400);
        bass.note_on(48, 0.7, 24_000);
        run(&mut |l, r| bass.render(l, r), 4_800);
        assert!(bass.freq < midi_to_freq(46), "a long glide is still sliding after 100 ms");
    }

    /// A new Acid's resonance sits on its cutoff wherever the sweep takes it;
    /// the one before went sharp on the way up and, with the cutoff at the
    /// top, rang by itself at the edge of hearing. The factory sounds and a
    /// saved one that says nothing keep that.
    #[test]
    fn a_new_acid_resonates_on_its_cutoff() {
        // The loudest harmonic of a low held note, the saw's own slope taken
        // out, against the cutoff: in cents.
        let peak = |tuned: bool, cutoff: f32| {
            let mut bass = AcidBass::new(AcidParams { cutoff, resonance: 0.95, env_mod: 0.0, accent: 0.0, drive: 0.0, tuned, ..Default::default() }, SR);
            bass.note_on(24, 0.5, 48_000);
            let heard = run(&mut |l, r| bass.render(l, r), 24_000).0;
            let f0 = midi_to_freq(24);
            let level = |n: usize| {
                let (re, im) = heard[4_800..].iter().enumerate().fold((0f64, 0f64), |(re, im), (i, v)| {
                    let a = (TAU * f0 * n as f32 / SR) as f64 * i as f64;
                    (re + *v as f64 * a.cos(), im + *v as f64 * a.sin())
                });
                (re * re + im * im).sqrt() * n as f64
            };
            let (low, high) = ((cutoff * 0.5 / f0) as usize, ((cutoff * 2.0).min(SR * 0.49) / f0) as usize);
            let n = (low.max(1)..=high).max_by(|a, b| level(*a).total_cmp(&level(*b))).unwrap();
            1200.0 * (n as f32 * f0 / cutoff).log2()
        };
        let mut worst_before = 0f32;
        for cutoff in [300.0f32, 1000.0, 3000.0, 6000.0, 10_000.0] {
            let (before, now) = (peak(false, cutoff), peak(true, cutoff));
            println!("corte {cutoff} Hz: resuena a {before:+.0} cents antes y {now:+.0} ahora");
            assert!(now.abs() <= 60.0, "{cutoff} Hz: {now} cents off");
            worst_before = worst_before.max(before.abs());
        }
        assert!(worst_before > 250.0, "the one before was off by {worst_before} cents");

        // An accented note of «Gritón» throws the cutoff to the top: how much
        // of its first 10 ms is at the very top of the band (the share of
        // the energy that changes sign from one sample to the next).
        let top = |tuned: bool| {
            let mut bass = AcidBass::new(AcidParams { tuned, ..acid_preset("Acid · Gritón") }, SR);
            bass.note_on(36, 1.0, 24_000);
            let heard = run(&mut |l, r| bass.render(l, r), 480).0;
            let jumps: f32 = heard.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
            assert!(heard.iter().all(|v| v.is_finite() && v.abs() < 4.0));
            jumps / (4.0 * energy(&heard))
        };
        let (before, now) = (top(false), top(true));
        println!("«Gritón» acentuado, parte en lo más alto: {before:.3} antes, {now:.3} ahora");
        assert!(before > 0.1 && now < 0.2 * before, "{before} before, {now} now");

        // Who has it.
        assert!(AcidParams::default().tuned && !AcidParams::old().tuned);
        for (name, p) in AcidParams::presets() {
            assert_eq!(p.tuned, name == "Init", "{name}");
        }
        let mut old = serde_json::to_value(AcidParams::default()).unwrap();
        old.as_object_mut().unwrap().remove("tuned");
        let old: AcidParams = serde_json::from_value(old).unwrap();
        assert_eq!(old, AcidParams::old());
        let again: AcidParams = serde_json::from_str(&serde_json::to_string(&AcidParams::default()).unwrap()).unwrap();
        assert!(again.tuned);
    }

    #[test]
    fn fm_and_acid_render_audible_bounded_audio() {
        let mut fm = FmSynth::new(FmParams::default(), SR);
        fm.note_on(72, 1.0, 12_000);
        let (l, _) = run(&mut |l, r| fm.render(l, r), 12_000);
        assert!(energy(&l) > 1.0);
        let mut acid = AcidBass::new(AcidParams { resonance: 0.97, drive: 1.0, ..Default::default() }, SR);
        acid.note_on(36, 1.0, 24_000);
        let (l, _) = run(&mut |l, r| acid.render(l, r), 24_000);
        assert!(energy(&l) > 1.0);
    }

    /// Playing harder brightens the FM Synth, not only louder: velocity
    /// scales the modulation index (a full hit plays the index as set).
    #[test]
    fn fm_velocity_brightens() {
        let brightness = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum::<f32>() / energy(s).max(1e-9);
        let play = |velocity: f32| {
            let mut fm = FmSynth::new(FmParams { mod_decay: 5.0, mod_sustain: 1.0, ..FmParams::default() }, SR);
            fm.note_on(57, velocity, 12_000);
            run(&mut |l, r| fm.render(l, r), 12_000).0
        };
        let (soft, hard) = (play(0.3), play(1.0));
        let (soft, hard) = (&soft[2_400..], &hard[2_400..]);
        assert!(brightness(hard) > brightness(soft) * 1.5, "{} against {}", brightness(hard), brightness(soft));
        // Its loudness still follows the velocity (a change of index moves it a little).
        let ratio = (energy(hard) / energy(soft)).sqrt() * 0.3;
        assert!((0.8..1.25).contains(&ratio), "{ratio}");
    }

    /// The swelling Super Saw presets open their filter with the slow attack (the
    /// audition's short notes barely show it): later in a held note the sound is
    /// brighter, measured as the share of energy in its sample-to-sample changes.
    #[test]
    fn supersaw_swell_presets_open_their_filter() {
        let brightness = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum::<f32>() / energy(s).max(1e-9);
        for name in ["Pad · Amanecer", "FX · Barrido"] {
            let (_, preset) = SuperSawParams::presets().into_iter().find(|(n, _)| *n == name).unwrap();
            let mut synth = SuperSaw::new(preset, SR);
            synth.note_on(48, 1.0, HELD);
            let (l, _) = run(&mut |l, r| synth.render(l, r), 48_000 * 2);
            let (early, late) = (brightness(&l[9_600..14_400]), brightness(&l[86_400..91_200]));
            assert!(late > early * 4.0, "{name}: the filter should open ({early} → {late})");
        }
    }

    /// A chord held long enough for the slow attacks and filter swells to arrive
    /// still leaves headroom (the audition's notes are too short to show it).
    #[test]
    fn synth_and_supersaw_presets_leave_headroom_on_a_held_chord() {
        let chord = [48, 55, 60, 64];
        let frames = 48_000 * 4;
        let peak = |l: &[f32], r: &[f32]| l.iter().chain(r).fold(0.0f32, |m, s| m.max(s.abs()));
        let mut problems = Vec::new();
        for (name, preset) in crate::model::SynthParams::presets().into_iter().skip(1) {
            let mut synth = crate::synth::PolySynth::new(preset, SR);
            chord.iter().for_each(|&p| synth.note_on(p, 0.75, HELD));
            let mut l = vec![0.0; frames];
            synth.render(&mut l);
            let peak = peak(&l, &[]);
            if peak > 1.0 {
                problems.push(format!("Pulse Synth {name}: peak {:.1} dB", 20.0 * peak.log10()));
            }
        }
        for (name, preset) in SuperSawParams::presets().into_iter().skip(1) {
            let mut synth = SuperSaw::new(preset, SR);
            chord.iter().for_each(|&p| synth.note_on(p, 0.75, HELD));
            let (l, r) = run(&mut |l, r| synth.render(l, r), frames);
            let peak = peak(&l, &r);
            if peak > 1.0 {
                problems.push(format!("Super Saw {name}: peak {:.1} dB", 20.0 * peak.log10()));
            }
        }
        assert!(problems.is_empty(), "held chords clip:\n{}", problems.join("\n"));
    }

    #[test]
    fn held_notes_stop_on_note_off() {
        let mut synth = SuperSaw::new(SuperSawParams::default(), SR);
        synth.note_on(64, 1.0, HELD);
        run(&mut |l, r| synth.render(l, r), 4_800);
        synth.note_off(64);
        let (tail, _) = run(&mut |l, r| synth.render(l, r), 48_000);
        assert!(energy(&tail[tail.len() - 2400..]) < 1e-6);
    }

    /// The drum machine's sounds follow pitch bend as far as their range,
    /// each hit taking the bend it starts with: a tom bent fully with a range
    /// of 12 sounds an octave up, on every model; one without range doesn't move.
    #[test]
    fn drum_hits_take_the_bend_they_start_with() {
        use crate::model::DrumModel;
        // Rising zero crossings over a stretch after the attack, as Hz.
        let pitch = |x: &[f32]| {
            let c: Vec<usize> = (1..x.len()).filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0).collect();
            SR * (c.len() - 1) as f32 / (c[c.len() - 1] - c[0]) as f32
        };
        for model in [DrumModel::Classic, DrumModel::Tr808, DrumModel::Tr909] {
            let hit = |range: f32, bend: f32| {
                let mut params = DrumParams { model, ..DrumParams::default() };
                params.bend[6] = range;
                let mut drums = DrumMachine::new(params, SR);
                drums.set_bend(bend);
                drums.note_on(drum_note::LOW_TOM, 1.0);
                drums.set_bend(0.0);
                let (l, _) = run(&mut |l, r| drums.render(l, r), 9600);
                pitch(&l[2400..9600])
            };
            let (flat, up, ignored) = (hit(12.0, 0.0), hit(12.0, 1.0), hit(0.0, 1.0));
            let cents = 1200.0 * (up / flat).log2();
            println!("{model:?} low tom: {flat:.1} Hz, bent {up:.1} Hz ({cents:+.0} cents)");
            assert!((cents - 1200.0).abs() < 40.0, "{model:?}: {cents}");
            assert!((ignored - flat).abs() < 0.5, "{model:?}: no range, no bend");
        }
    }
}
