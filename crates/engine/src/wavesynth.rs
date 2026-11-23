//! Wavetable synthesizer: two wavetable oscillators with unison and warp modes,
//! a sub and a noise oscillator, a multimode filter, three envelopes, four
//! hand-drawn LFOs and a sixteen-slot modulation matrix evaluated per voice.
//!
//! The synth is built on the UI thread (it owns per-voice comb-filter buffers)
//! and handed to the audio thread ready to play; rendering never allocates.

use crate::effects::{ProcessContext, Switched};
use crate::instruments::{saw, saw_clean, square, svf_g, Declick, Gate, Svf, Taken, Watch};
use crate::synth::HELD;
use crate::model::{
    wave_is_target, ArpMode, SEQ_STEPS, WAVE_BALANCE, WAVE_FILTER2, WAVE_OSC_EXTRA, WAVE_RANDOM_RATE, KickWave, LfoMode, ModSource, OscKind, OscWarp, ParamInfo, WaveEnv, WaveFilter, WaveParams,
    WAVE_GRAIN, WAVE_PARAMS, WAVE_SLOTS,
};
use crate::sample::Sample;
use crate::wavetable::Wavetable;
use std::f32::consts::TAU;
use std::sync::Arc;

const VOICES: usize = 16;
/// Every parameter of the synth, modulated or not.
const PARAMS: usize = WAVE_PARAMS.len();
const GRAINS: usize = 24;
/// Size of the single-cycle table a harmonic oscillator rebuilds as it plays.
pub(crate) const HARMONIC_TABLE: usize = 256;
/// The harmonic table is rebuilt every this many control blocks.
const HARMONIC_EVERY: u32 = 4;
/// Sample and granular oscillators play their file at its own pitch on C3.
pub(crate) const SAMPLE_ROOT_HZ: f32 = 261.625_58;
const UNISON: usize = 16;
/// Modulation is evaluated every `CONTROL` samples, counted from the synth's
/// start (not from each buffer's), so the sound doesn't depend on the buffer.
const CONTROL: usize = 32;
pub(crate) const COMB: usize = 2048;
/// Indices into the parameter table (see `WAVE_PARAMS`).
const OSC_PARAMS: usize = 13;
const SUB: usize = 26;
const NOISE: usize = 29;
const FILTER: usize = 32;
const LFOS: usize = 52;
const MACROS: usize = 60;
const GLIDE: usize = 64;
const MASTER: usize = 65;

pub(crate) fn note_freq(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Stage {
    Idle,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
}

/// Attack is linear; decay and release are exponential, reaching their target in
/// about the set time.
#[derive(Clone, Copy)]
pub(crate) struct Env {
    pub(crate) stage: Stage,
    pub(crate) level: f32,
    held_for: f32,
}

impl Env {
    pub(crate) const IDLE: Env = Env { stage: Stage::Idle, level: 0.0, held_for: 0.0 };

    /// Starts again from the current level, so retriggers do not click.
    pub(crate) fn trigger(&mut self) {
        self.stage = Stage::Attack;
        self.held_for = 0.0;
    }

    pub(crate) fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub(crate) fn tick(&mut self, p: &WaveEnv, dt: f32) -> f32 {
        let towards = |level: f32, target: f32, time: f32| target + (level - target) * (-dt * 5.0 / time.max(1e-4)).exp();
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += dt / p.attack.max(1e-4);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Hold;
                }
            }
            Stage::Hold => {
                self.held_for += dt;
                if self.held_for >= p.hold {
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level = towards(self.level, p.sustain, p.decay);
                if (self.level - p.sustain).abs() < 1e-4 {
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = p.sustain,
            Stage::Release => {
                self.level = towards(self.level, 0.0, p.release);
                if self.level < 1e-5 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

/// Filter memory for one channel of one voice.
#[derive(Clone)]
pub(crate) struct FilterState {
    svf: [Svf; 2],
    ladder: [f32; 4],
    comb: Option<Box<[f32; COMB]>>,
    comb_pos: usize,
    formant: [Svf; 3],
    /// The clean Drive's last input, and the area under the curve up to it.
    driven: f64,
    driven_area: f64,
}

impl FilterState {
    pub(crate) fn new() -> Self {
        Self { svf: [Svf::default(); 2], ladder: [0.0; 4], comb: Some(Box::new([0.0; COMB])), comb_pos: 0, formant: [Svf::default(); 3], driven: 0.0, driven_area: std::f64::consts::LN_2 }
    }

    pub(crate) fn clear(&mut self) {
        self.svf = [Svf::default(); 2];
        self.ladder = [0.0; 4];
        if let Some(comb) = &mut self.comb {
            comb.fill(0.0);
        }
        self.formant = [Svf::default(); 3];
        (self.driven, self.driven_area) = (0.0, std::f64::consts::LN_2);
    }
}

/// The comb's period for `cutoff`, in samples: fractional, so it is in tune
/// high up too; at least 3 for the taps around it.
fn comb_delay(cutoff: f32, sr: f32) -> f32 {
    (sr / cutoff).clamp(3.0, (COMB - 4) as f32)
}

/// How much of each neighbour the comb's loop takes in: a gentle low pass
/// (centred, so it doesn't detune), as a string loses its highs first.
const COMB_DAMP: f32 = 0.1;

/// The comb's taps for a `delay` (fractional, samples): third-order Lagrange
/// interpolation, through the damping low pass `[d, 1 - 2d, d]` around it.
/// Tap `j` reads `floor(delay) - 2 + j` samples back.
fn comb_taps(delay: f32) -> [f32; 6] {
    let f = delay - delay.floor();
    let lagrange = [-f * (f - 1.0) * (f - 2.0) / 6.0, (f + 1.0) * (f - 1.0) * (f - 2.0) / 2.0, -(f + 1.0) * f * (f - 2.0) / 2.0, (f + 1.0) * f * (f - 1.0) / 6.0];
    let mut taps = [0.0; 6];
    for (o, w) in [COMB_DAMP, 1.0 - 2.0 * COMB_DAMP, COMB_DAMP].into_iter().enumerate() {
        for (k, h) in lagrange.iter().enumerate() {
            taps[o + k] += w * h;
        }
    }
    taps
}

/// Filter settings for a control block.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct FilterCoefs {
    mode: WaveFilter,
    g: f32,
    k: f32,
    ladder_g: f32,
    ladder_feedback: f32,
    /// The tuned Ladder (see [`Self::tuned`]): each stage's share of its
    /// input, and that to the fourth.
    ladder_tuned: bool,
    ladder_share: f32,
    ladder_loop: f32,
    /// The comb's delay in whole samples, and its taps over delays
    /// `comb_delay - 2 ..= comb_delay + 3` (see `comb_taps`).
    comb_delay: usize,
    comb_taps: [f32; 6],
    comb_feedback: f32,
    formants: [(f32, f32); 3],
    formant_k: f32,
    drive: f32,
    drive_clean: bool,
    mix: f32,
}

const VOWELS: [[f32; 3]; 5] = [[800.0, 1150.0, 2900.0], [400.0, 1600.0, 2700.0], [300.0, 2300.0, 3000.0], [450.0, 800.0, 2830.0], [325.0, 700.0, 2530.0]];

impl FilterCoefs {
    pub(crate) fn new(mode: WaveFilter, cutoff: f32, resonance: f32, drive: f32, mix: f32, sr: f32) -> Self {
        let cutoff = cutoff.clamp(20.0, sr * 0.45);
        let resonance = resonance.clamp(0.0, 1.0);
        // Vowel position from the cutoff's place on a log scale.
        let place = ((cutoff / 20.0).ln() / (1000f32).ln()).clamp(0.0, 1.0) * 4.0;
        let (a, b) = (place.floor() as usize, (place.floor() as usize + 1).min(4));
        let t = place - place.floor();
        let mut formants = [(0.0, 0.0); 3];
        for (i, f) in formants.iter_mut().enumerate() {
            let hz = VOWELS[a][i] + (VOWELS[b][i] - VOWELS[a][i]) * t;
            *f = (svf_g(hz, sr), [1.0, 0.6, 0.35][i]);
        }
        Self {
            mode,
            g: svf_g(cutoff, sr),
            k: (2.0 - 1.98 * resonance).max(0.02),
            ladder_g: 1.0 - (-TAU * cutoff / sr).exp(),
            ladder_feedback: resonance * 4.2,
            ladder_tuned: false,
            ladder_share: svf_g(cutoff, sr) / (1.0 + svf_g(cutoff, sr)),
            ladder_loop: (svf_g(cutoff, sr) / (1.0 + svf_g(cutoff, sr))).powi(4),
            comb_delay: comb_delay(cutoff, sr) as usize,
            comb_taps: comb_taps(comb_delay(cutoff, sr)),
            comb_feedback: resonance * 0.97,
            formants,
            formant_k: 0.6 - resonance * 0.55,
            drive,
            drive_clean: false,
            mix,
        }
    }

    /// The Ladder with its feedback solved within the sample (no sample of
    /// delay round the loop): the resonance sits on the cutoff wherever it
    /// is, and it starts to ring by itself at the same Res all the way up.
    /// Without it, the one made before: its resonance goes sharp as the
    /// cutoff rises (almost five semitones at 10 kHz) and it no longer rings
    /// by itself over a few hundred Hz.
    pub(crate) fn tuned(mut self, tuned: bool) -> Self {
        self.ladder_tuned = tuned;
        self
    }

    /// The Drive without what it folds back: each sample is the curve's
    /// mean between the last input and this one, not the curve at this one,
    /// so a bright wave driven hard stays in tune on high notes. Without it,
    /// the one made before.
    pub(crate) fn clean_drive(mut self, clean: bool) -> Self {
        self.drive_clean = clean;
        self
    }

    /// The filter `t` (0..1) of the way from `from` to `to`: what moves
    /// smoothly does (the comb's length, a whole number of samples, is `to`'s).
    fn between(from: &Self, to: &Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            g: mix(from.g, to.g),
            k: mix(from.k, to.k),
            ladder_g: mix(from.ladder_g, to.ladder_g),
            ladder_feedback: mix(from.ladder_feedback, to.ladder_feedback),
            ladder_share: mix(from.ladder_share, to.ladder_share),
            ladder_loop: mix(from.ladder_loop, to.ladder_loop),
            comb_feedback: mix(from.comb_feedback, to.comb_feedback),
            formants: std::array::from_fn(|i| (mix(from.formants[i].0, to.formants[i].0), to.formants[i].1)),
            formant_k: mix(from.formant_k, to.formant_k),
            drive: mix(from.drive, to.drive),
            mix: mix(from.mix, to.mix),
            ..*to
        }
    }

    pub(crate) fn process(&self, s: &mut FilterState, x: f32) -> f32 {
        let input = if self.drive > 0.0 && self.drive_clean {
            let g = 1.0 + self.drive * 8.0;
            let u = (x * g) as f64;
            // ln(2 cosh u), the area under tanh.
            let area = u.abs() + (-2.0 * u.abs()).exp().ln_1p();
            let step = u - s.driven;
            let mean = if step.abs() > 1e-5 { (area - s.driven_area) / step } else { (0.5 * (u + s.driven)).tanh() };
            (s.driven, s.driven_area) = (u, area);
            mean as f32 / g.tanh()
        } else if self.drive > 0.0 {
            let g = 1.0 + self.drive * 8.0;
            (x * g).tanh() / g.tanh()
        } else {
            x
        };
        let butterworth = std::f32::consts::SQRT_2;
        let wet = match self.mode {
            WaveFilter::Lp12 => s.svf[0].tick(input, self.g, self.k).0,
            WaveFilter::Lp24 => {
                let a = s.svf[0].tick(input, self.g, self.k).0;
                s.svf[1].tick(a, self.g, butterworth).0
            }
            WaveFilter::Hp12 => s.svf[0].tick(input, self.g, self.k).2,
            WaveFilter::Hp24 => {
                let a = s.svf[0].tick(input, self.g, self.k).2;
                s.svf[1].tick(a, self.g, butterworth).2
            }
            WaveFilter::Bp12 => s.svf[0].tick(input, self.g, self.k).1 * self.k.max(0.3),
            WaveFilter::Notch => {
                let (low, _, high) = s.svf[0].tick(input, self.g, self.k);
                low + high
            }
            WaveFilter::Ladder if self.ladder_tuned => {
                let (share, keep) = (self.ladder_share, 1.0 - self.ladder_share);
                // What the four stages hold comes out whatever goes in now.
                let held = keep * (share * (share * (share * s.ladder[0] + s.ladder[1]) + s.ladder[2]) + s.ladder[3]);
                let mut y = ((input - self.ladder_feedback * held) / (1.0 + self.ladder_feedback * self.ladder_loop)).tanh();
                for stage in &mut s.ladder {
                    let v = (y - *stage) * share;
                    y = v + *stage;
                    *stage = y + v;
                }
                // Make up for the level the feedback takes away.
                y * (1.0 + self.ladder_feedback * 0.4)
            }
            WaveFilter::Ladder => {
                let g = self.ladder_g;
                let feedback = (input - self.ladder_feedback * s.ladder[3]).tanh();
                s.ladder[0] += g * (feedback - s.ladder[0]);
                s.ladder[1] += g * (s.ladder[0] - s.ladder[1]);
                s.ladder[2] += g * (s.ladder[1] - s.ladder[2]);
                s.ladder[3] += g * (s.ladder[2] - s.ladder[3]);
                // Make up for the level the feedback takes away.
                s.ladder[3] * (1.0 + self.ladder_feedback * 0.4)
            }
            WaveFilter::Comb => match &mut s.comb {
                Some(comb) => {
                    let oldest = s.comb_pos + COMB - (self.comb_delay - 2);
                    let fed: f32 = self.comb_taps.iter().enumerate().map(|(j, tap)| tap * comb[(oldest - j) % COMB]).sum();
                    let y = input + self.comb_feedback * fed;
                    comb[s.comb_pos] = y.clamp(-4.0, 4.0);
                    s.comb_pos = (s.comb_pos + 1) % COMB;
                    y * (1.0 - self.comb_feedback * 0.5)
                }
                None => input,
            },
            WaveFilter::Formant => {
                let mut y = 0.0;
                for (state, &(g, level)) in s.formant.iter_mut().zip(&self.formants) {
                    y += state.tick(input, g, self.formant_k).1 * level;
                }
                y * 1.5
            }
        };
        x + (wet - x) * self.mix
    }
}

#[derive(Clone, Copy)]
struct Grain {
    active: bool,
    position: f64,
    rate: f64,
    age: u32,
    length: u32,
    gain: (f32, f32),
}

impl Grain {
    const IDLE: Grain = Grain { active: false, position: 0.0, rate: 1.0, age: 0, length: 1, gain: (0.0, 0.0) };
}

/// What a voice's modulation sets once per control period and is heard as
/// a step if it jumps: its levels and pans, where its oscillators are in
/// their tables and how warped, and its filters.
#[derive(Clone, Copy)]
struct Levels {
    gains: [[(f32, f32); UNISON]; 2],
    positions: [f32; 2],
    warps: [f32; 2],
    sub: (f32, f32),
    noise: (f32, f32),
    master: f32,
    filters: [Option<FilterCoefs>; 2],
}

impl Levels {
    const NONE: Levels = Levels { gains: [[(0.0, 0.0); UNISON]; 2], positions: [0.0; 2], warps: [0.0; 2], sub: (0.0, 0.0), noise: (0.0, 0.0), master: 0.0, filters: [None; 2] };
}

#[derive(Clone)]
struct Voice {
    active: bool,
    gate: Gate,
    /// Pitch now and where it glides to, in (fractional) MIDI notes.
    note: f32,
    target: f32,
    velocity: f32,
    random: f32,
    envs: [Env; 3],
    lfo_phase: [f32; 4],
    /// Seconds since the note started (LFO rise and envelope mode).
    age: f32,
    phases: [[f32; UNISON]; 2],
    sub_phase: f32,
    noise_seed: u32,
    noise_lp: f32,
    filter: [FilterState; 2],
    /// Last output of each oscillator, for FM between them.
    last: [f32; 2],
    /// Sample oscillators: read positions per unison voice (negative: not started).
    positions: [[f64; UNISON]; 2],
    grains: [[Grain; GRAINS]; 2],
    /// Samples until the next grain starts.
    grain_wait: [f32; 2],
    /// Analog drift per unison voice: current and target offset, in cents.
    drift: [[(f32, f32); UNISON]; 2],
    /// Harmonic oscillators: the cycle being played and blocks until it is rebuilt.
    harmonic: [[f32; HARMONIC_TABLE]; 2],
    harmonic_wait: u32,
    /// Random sources: held value, smoothed value and phase towards the next value.
    hold: f32,
    smooth: f32,
    random_phase: f32,
    filter2: [FilterState; 2],
    /// Its modulated parameters and frequency this control period, and the
    /// period they are of: a period played in pieces (a buffer that ends in
    /// it, a note starting in it) plays each piece with the same.
    snap: ([f32; PARAMS], f32),
    snap_period: u64,
    /// Its levels and filters when this control period started, and this
    /// period's: it moves from the ones to the others along the period.
    was: Levels,
    now: Levels,
    taken: Taken,
}

impl Voice {
    fn new(seed: u32) -> Self {
        Self {
            active: false,
            gate: Gate::OFF,
            note: 60.0,
            target: 60.0,
            velocity: 0.0,
            random: 0.0,
            envs: [Env::IDLE; 3],
            lfo_phase: [0.0; 4],
            age: 0.0,
            phases: [[0.0; UNISON]; 2],
            sub_phase: 0.0,
            noise_seed: seed,
            noise_lp: 0.0,
            filter: [FilterState::new(), FilterState::new()],
            last: [0.0; 2],
            positions: [[-1.0; UNISON]; 2],
            grains: [[Grain::IDLE; GRAINS]; 2],
            grain_wait: [0.0; 2],
            drift: [[(0.0, 0.0); UNISON]; 2],
            harmonic: [[0.0; HARMONIC_TABLE]; 2],
            harmonic_wait: 0,
            hold: 0.5,
            smooth: 0.5,
            random_phase: 0.0,
            filter2: [FilterState::new(), FilterState::new()],
            snap: ([0.0; PARAMS], 0.0),
            snap_period: u64::MAX,
            was: Levels::NONE,
            now: Levels::NONE,
            taken: Taken::default(),
        }
    }

    fn random(&mut self) -> f32 {
        self.noise_seed ^= self.noise_seed << 13;
        self.noise_seed ^= self.noise_seed >> 17;
        self.noise_seed ^= self.noise_seed << 5;
        self.noise_seed as f32 / u32::MAX as f32
    }
}

/// Per-block settings of one oscillator.
struct OscBlock {
    kind: OscKind,
    count: usize,
    incs: [f32; UNISON],
    gains: [(f32, f32); UNISON],
    level: usize,
    /// The level a clean warp reads (see [`warp_stretch`]), if it has one.
    clean: Option<usize>,
    /// The analog saw and pulse with their steps corrected over four samples.
    smooth: bool,
    position: f32,
    warp: OscWarp,
    amount: f32,
    /// Sample playback: rate per unison voice relative to the file, and the loop start.
    rates: [f64; UNISON],
    start: f64,
    looping: bool,
    /// Granular settings.
    grain_frames: u32,
    grain_every: f32,
    spray: f32,
    grain_pitch: f32,
    width: f32,
    pan: f32,
    grain_scale: f32,
}

/// Fills `table` with one cycle of `partials` harmonics: slope `tilt`, odd/even
/// balance `parity`, and a formant peak at `formant` (0..1 along the partials)
/// raised by `gain`. Harmonics above `max_harmonic` are left out (no aliasing).
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_harmonic(table: &mut [f32; HARMONIC_TABLE], sine: &[f32], partials: f32, tilt: f32, parity: f32, formant: f32, gain: f32, max_harmonic: usize) {
    let count = (partials.round() as usize).clamp(1, 64).min(max_harmonic.max(1)).min(HARMONIC_TABLE / 2 - 1);
    let centre = 1.0 + formant.clamp(0.0, 1.0) * (partials.max(1.0) - 1.0);
    table.fill(0.0);
    for n in 1..=count {
        let mut amplitude = (n as f32).powf(-1.0 + tilt.clamp(-1.0, 1.0));
        if n > 1 {
            amplitude *= if n % 2 == 1 { 1.0 - parity.max(0.0) } else { 1.0 + parity.min(0.0) };
        }
        amplitude *= 1.0 + gain * 4.0 * (-((n as f32 - centre) / 1.5).powi(2)).exp();
        if amplitude <= 1e-5 {
            continue;
        }
        for (i, value) in table.iter_mut().enumerate() {
            *value += amplitude * sine[(n * i) % HARMONIC_TABLE];
        }
    }
    let peak = table.iter().fold(0f32, |m, v| m.max(v.abs()));
    if peak > 1e-6 {
        for value in table.iter_mut() {
            *value /= peak;
        }
    }
}

/// One cycle of an analog or harmonic oscillator at its current settings, for displays.
pub fn preview_cycle(osc: &crate::model::WaveOsc, points: usize) -> Vec<f32> {
    match osc.kind {
        OscKind::Harmonic => {
            let sine: Vec<f32> = (0..HARMONIC_TABLE).map(|i| (i as f32 / HARMONIC_TABLE as f32 * TAU).sin()).collect();
            let mut table = [0f32; HARMONIC_TABLE];
            build_harmonic(&mut table, &sine, osc.partials, osc.tilt, osc.parity, osc.position, osc.warp_amount, 64);
            (0..points).map(|i| table[i * HARMONIC_TABLE / points]).collect()
        }
        _ => {
            let morph = osc.position.clamp(0.0, 1.0) * 3.0;
            let width = 0.5 - osc.warp_amount.clamp(0.0, 1.0) * 0.45;
            (0..points)
                .map(|i| {
                    let ph = i as f32 / points as f32;
                    let sine = (ph * TAU).sin();
                    let triangle = 1.0 - 4.0 * (((ph + 0.25) % 1.0) - 0.5).abs();
                    let saw_wave = 2.0 * ((ph + 0.5) % 1.0) - 1.0;
                    let pulse = if ph < width { 1.0 } else { -1.0 };
                    match morph {
                        m if m < 1.0 => sine + (triangle - sine) * m,
                        m if m < 2.0 => triangle + (saw_wave - triangle) * (m - 1.0),
                        m => saw_wave + (pulse - saw_wave) * (m - 2.0),
                    }
                })
                .collect()
        }
    }
}

/// How many times faster than the note a warp reads the table at its
/// fastest: what its harmonics are multiplied by. (Bend+ starts faster
/// still, for an instant.) The warps that bend the phase only.
fn warp_stretch(warp: OscWarp, amount: f32) -> Option<f32> {
    match warp {
        OscWarp::Sync => Some(1.0 + amount * 15.0),
        OscWarp::BendPlus | OscWarp::BendMinus => Some(1.0 + amount * 4.0),
        OscWarp::Pwm => Some(0.5 / (0.5 - amount * 0.49)),
        OscWarp::Mirror => Some(1.0 + amount),
        OscWarp::Off | OscWarp::Quantize | OscWarp::Fm => None,
    }
}

/// Reads a clean warp takes per sample, spread over a sample either side
/// of its place and weighed as a triangle: what the warp puts over the top
/// of the band is averaged out, not folded back down.
const WARP_READS: usize = 8;

fn warp_phase(warp: OscWarp, amount: f32, phase: f32) -> f32 {
    match warp {
        OscWarp::Sync => (phase * (1.0 + amount * 15.0)).fract(),
        OscWarp::BendPlus => phase.powf(1.0 / (1.0 + amount * 4.0)),
        OscWarp::BendMinus => phase.powf(1.0 + amount * 4.0),
        OscWarp::Pwm => {
            let w = 0.5 - amount * 0.49;
            if phase < w { phase * 0.5 / w } else { 0.5 + (phase - w) * 0.5 / (1.0 - w) }
        }
        OscWarp::Mirror => {
            let mirrored = if phase < 0.5 { phase * 2.0 * 0.5 } else { (1.0 - phase) * 2.0 * 0.5 };
            phase + (mirrored - phase) * amount
        }
        OscWarp::Off | OscWarp::Quantize | OscWarp::Fm => phase,
    }
}

pub struct WaveSynth {
    params: WaveParams,
    sample_rate: f32,
    tempo: f64,
    voices: Vec<Voice>,
    counter: u64,
    /// Seconds since the synth started, for free-running LFOs: `frames` over
    /// the rate (exact, however the blocks are cut).
    clock: f64,
    frames: u64,
    /// Control periods (`CONTROL` samples) since it started, and how far into
    /// the current one it is: modulation moves once a period, on the same
    /// samples whatever the buffer size.
    period: u64,
    period_at: usize,
    /// Notes held in mono mode, oldest first.
    held: [u8; 16],
    held_count: usize,
    last_note: Option<f32>,
    seed: u32,
    /// The synth's effects rack, built with the synth (allocation happens here).
    fx: Vec<Option<Switched>>,
    scratch: [[f32; CONTROL]; 2],
    arp: Arp,
    seq: Seq,
    /// One sine cycle, for building harmonic tables without calling sin().
    sine: Vec<f32>,
    /// The voice the last note started on (for sequencer slides).
    last_voice: usize,
    declick: Declick,
    /// Pitch bend and vibrato, as a frequency ratio.
    pitch: f32,
    /// What the controllers do to the filters' cutoffs, as a ratio.
    bright: f32,
}

/// Step sequencer state.
struct Seq {
    /// Keys held, latest last; the sequence follows the latest.
    keys: [(u8, f32); 16],
    count: usize,
    step: usize,
    wait: f64,
    /// The current step's modulation value.
    value: f32,
    /// The previous step slid into this one.
    sliding: bool,
}

/// Arpeggiator state: the keys held and where the pattern is.
struct Arp {
    keys: [(u8, f32); 16],
    count: usize,
    step: usize,
    /// Samples until the next step.
    wait: f64,
}

/// Voice `i`'s first noise seed.
fn voice_seed(i: usize) -> u32 {
    0x9E37_79B9 ^ (i as u32).wrapping_mul(0x85EB_CA6B)
}

impl WaveSynth {
    /// Its random numbers from the start again (the transport starts: it
    /// plays the same every time, and as exported).
    pub fn reseed(&mut self) {
        self.seed = 0x1357_9BDF;
        for (i, voice) in self.voices.iter_mut().enumerate() {
            voice.noise_seed = voice_seed(i);
        }
    }

    pub fn new(params: WaveParams, sample_rate: f32) -> Self {
        let voices = (0..VOICES).map(|i| Voice::new(voice_seed(i))).collect();
        let fx = params.fx.iter().map(|slot| slot.map(|d| Switched::new(&d.effect, d.enabled, sample_rate))).collect();
        Self {
            params,
            sample_rate,
            tempo: 120.0,
            voices,
            counter: 0,
            clock: 0.0,
            frames: 0,
            period: 0,
            period_at: 0,
            held: [0; 16],
            held_count: 0,
            last_note: None,
            seed: 0x1357_9BDF,
            fx,
            scratch: [[0.0; CONTROL]; 2],
            arp: Arp { keys: [(0, 0.0); 16], count: 0, step: 0, wait: 0.0 },
            seq: Seq { keys: [(0, 0.0); 16], count: 0, step: 0, wait: 0.0, value: 0.0, sliding: false },
            sine: (0..HARMONIC_TABLE).map(|i| (i as f32 / HARMONIC_TABLE as f32 * TAU).sin()).collect(),
            last_voice: 0,
            declick: Declick::default(),
            pitch: 1.0,
            bright: 1.0,
        }
    }

    pub fn params(&self) -> WaveParams {
        self.params
    }

    /// New settings; the effects keep their state when only their settings change
    /// (a different set of effects needs a new synth, see `InstrumentKind::needs_rebuild`).
    pub fn set_params(&mut self, params: WaveParams) {
        if !params.arp.on && self.params.arp.on {
            self.arp.count = 0;
        }
        self.params = params;
        for (slot, device) in self.fx.iter_mut().zip(params.fx.iter()) {
            if let (Some(fx), Some(device)) = (slot.as_mut(), device) {
                fx.processor.set_params(&device.effect);
                fx.on = device.enabled;
            }
        }
    }

    pub fn set_tempo(&mut self, tempo: f64) {
        self.tempo = tempo;
    }

    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32
    }

    fn start_voice(&mut self, index: usize, pitch: u8, velocity: f32, gate: u32, retrigger: bool) {
        let p = self.params;
        self.counter += 1;
        // Legato glides only into a note that overlaps one still sounding
        // (the 808 slide); Poly and Mono glide from the last note always.
        let glide_from = if p.glide > 0.0 && (!(p.mono && p.legato) || self.overlaps()) { self.last_note } else { None };
        let mut randoms = [[0f32; UNISON]; 2];
        for osc in &mut randoms {
            for r in osc.iter_mut() {
                *r = self.random();
            }
        }
        let random = self.random();
        let voice = &mut self.voices[index];
        let was_active = voice.active;
        // Its modulation is worked out again from here.
        voice.snap_period = u64::MAX;
        voice.gate = Gate::new(pitch, velocity, gate, self.counter);
        voice.target = pitch as f32;
        voice.velocity = velocity;
        if !was_active || retrigger {
            voice.taken.take(was_active);
            voice.note = glide_from.unwrap_or(pitch as f32);
            voice.random = random;
            voice.age = 0.0;
            for (o, phases) in voice.phases.iter_mut().enumerate() {
                for (u, phase) in phases.iter_mut().enumerate() {
                    *phase = (p.osc[o].phase + p.osc[o].random_phase * randoms[o][u]).fract();
                }
            }
            voice.sub_phase = 0.0;
            voice.positions = [[-1.0; UNISON]; 2];
            voice.harmonic_wait = 0;
            voice.grains = [[Grain::IDLE; GRAINS]; 2];
            voice.grain_wait = [0.0; 2];
            for (l, phase) in voice.lfo_phase.iter_mut().enumerate() {
                if p.lfo[l].mode != LfoMode::Free {
                    *phase = 0.0;
                }
            }
            if !was_active {
                voice.filter[0].clear();
                voice.filter[1].clear();
                voice.filter2[0].clear();
                voice.filter2[1].clear();
                voice.last = [0.0; 2];
            }
            for env in &mut voice.envs {
                env.trigger();
            }
        }
        voice.active = true;
        self.last_note = Some(pitch as f32);
        self.last_voice = index;
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32, gate: u32) {
        if self.params.seq.on {
            let seq = &mut self.seq;
            if seq.count < seq.keys.len() {
                seq.keys[seq.count] = (pitch, velocity);
                seq.count += 1;
                if seq.count == 1 {
                    seq.step = 0;
                    seq.wait = 0.0;
                    seq.sliding = false;
                }
            }
            return;
        }
        if self.params.arp.on {
            let arp = &mut self.arp;
            if arp.count < arp.keys.len() && !arp.keys[..arp.count].iter().any(|k| k.0 == pitch) {
                arp.keys[arp.count] = (pitch, velocity);
                arp.count += 1;
                if arp.count == 1 {
                    // The first key starts the pattern right away.
                    arp.step = 0;
                    arp.wait = 0.0;
                }
            }
            return;
        }
        self.play_on(pitch, velocity, gate);
    }

    pub fn note_off(&mut self, pitch: u8) {
        if self.params.seq.on {
            let seq = &mut self.seq;
            if let Some(at) = seq.keys[..seq.count].iter().position(|k| k.0 == pitch) {
                seq.keys.copy_within(at + 1..seq.count, at);
                seq.count -= 1;
            }
            if seq.count == 0 {
                self.release_all();
            }
            return;
        }
        if self.params.arp.on {
            let arp = &mut self.arp;
            if let Some(at) = arp.keys[..arp.count].iter().position(|k| k.0 == pitch) {
                arp.keys.copy_within(at + 1..arp.count, at);
                arp.count -= 1;
            }
            return;
        }
        self.play_off(pitch);
    }

    fn release_all(&mut self) {
        for v in &mut self.voices {
            v.gate.held = false;
            for env in &mut v.envs {
                env.release();
            }
        }
    }

    /// Plays the next step of the sequencer if one is due in the next `frames` samples.
    fn seq_tick(&mut self, frames: usize) {
        if !self.params.seq.on || self.seq.count == 0 {
            return;
        }
        self.seq.wait -= frames as f64;
        if self.seq.wait > 0.0 {
            return;
        }
        let seq = self.params.seq;
        let step_frames = seq.rate_beats.max(0.03125) as f64 * 60.0 / self.tempo.max(1.0) * self.sample_rate as f64;
        self.seq.wait += step_frames;
        let length = (seq.length as usize).clamp(1, SEQ_STEPS);
        let step = seq.steps[self.seq.step % length];
        self.seq.step += 1;
        self.seq.value = step.value;
        let roll = self.random();
        let sliding = self.seq.sliding;
        self.seq.sliding = false;
        if !step.on || roll >= step.probability {
            return;
        }
        let (key, key_velocity) = self.seq.keys[self.seq.count - 1];
        let pitch = (key as f32 + step.pitch.round()).clamp(0.0, 127.0) as u8;
        // A slide holds the note into the next step, which then glides instead of restarting.
        let gate = if step.slide { step_frames * 1.02 } else { step_frames * step.gate.clamp(0.05, 1.0) as f64 };
        let velocity = (key_velocity * step.velocity * 1.25).clamp(0.0, 1.0);
        let voice = self.last_voice;
        if sliding && self.voices[voice].active {
            self.start_voice(voice, pitch, velocity, gate.max(1.0) as u32, false);
        } else {
            self.play_on(pitch, velocity, gate.max(1.0) as u32);
        }
        self.seq.sliding = step.slide;
    }

    /// Starts the next note of the arpeggio if a step is due in the next `frames` samples.
    fn arp_tick(&mut self, frames: usize) {
        if !self.params.arp.on || self.params.seq.on || self.arp.count == 0 {
            return;
        }
        self.arp.wait -= frames as f64;
        if self.arp.wait > 0.0 {
            return;
        }
        let step_frames = self.params.arp.rate_beats.max(0.03125) as f64 * 60.0 / self.tempo.max(1.0) * self.sample_rate as f64;
        self.arp.wait += step_frames;
        let mut sorted = [(0u8, 0f32); 16];
        let count = self.arp.count;
        sorted[..count].copy_from_slice(&self.arp.keys[..count]);
        if self.params.arp.mode != ArpMode::Played {
            sorted[..count].sort_by_key(|k| k.0);
        }
        let octaves = self.params.arp.octaves.clamp(1, 4) as usize;
        let len = count * octaves;
        let step = self.arp.step;
        let index = match self.params.arp.mode {
            ArpMode::Up | ArpMode::Played => step % len,
            ArpMode::Down => len - 1 - step % len,
            ArpMode::UpDown if len > 1 => {
                let period = 2 * len - 2;
                let at = step % period;
                if at < len { at } else { period - at }
            }
            ArpMode::UpDown => 0,
            ArpMode::Random => ((self.random() * len as f32) as usize).min(len - 1),
        };
        self.arp.step += 1;
        let (pitch, velocity) = sorted[index % count];
        let pitch = (pitch as usize + 12 * (index / count)).min(127) as u8;
        let gate = (step_frames * self.params.arp_gate.clamp(0.05, 1.0) as f64).max(1.0) as u32;
        self.play_on(pitch, velocity, gate);
    }

    /// A note still sounding (held, or a clip note whose length hasn't run
    /// out): a new one overlaps it. Notes that only touch don't count.
    fn overlaps(&self) -> bool {
        let margin = (self.sample_rate * 0.002) as u32;
        self.voices.iter().any(|v| v.active && v.gate.open_for(margin))
    }

    fn play_on(&mut self, pitch: u8, velocity: f32, gate: u32) {
        if self.params.mono {
            let legato = self.params.legato && self.voices[0].active && self.overlaps();
            // Only held notes get a note-off; timed ones (from clips) end on their own.
            if gate == HELD && self.held_count < self.held.len() {
                self.held[self.held_count] = pitch;
                self.held_count += 1;
            }
            self.start_voice(0, pitch, velocity, gate, !legato);
            return;
        }
        // A free voice, else the oldest already let go, else the oldest.
        let index = (0..VOICES)
            .find(|&i| !self.voices[i].active)
            .unwrap_or_else(|| (0..VOICES).min_by_key(|&i| (self.voices[i].gate.open_for(0), self.voices[i].gate.age)).unwrap_or(0));
        self.start_voice(index, pitch, velocity, gate, true);
    }

    fn play_off(&mut self, pitch: u8) {
        if self.params.mono {
            if let Some(at) = self.held[..self.held_count].iter().position(|&n| n == pitch) {
                self.held.copy_within(at + 1..self.held_count, at);
                self.held_count -= 1;
            }
            let voice = &mut self.voices[0];
            if voice.gate.held && voice.gate.pitch == pitch {
                if self.held_count > 0 {
                    // Back to the note still held, gliding without a new attack.
                    let previous = self.held[self.held_count - 1];
                    voice.gate.pitch = previous;
                    voice.target = previous as f32;
                    if self.params.glide <= 0.0 {
                        voice.note = previous as f32;
                    }
                } else {
                    voice.gate.held = false;
                    for env in &mut voice.envs {
                        env.release();
                    }
                }
            }
            return;
        }
        for v in self.voices.iter_mut().filter(|v| v.active && v.gate.held && v.gate.pitch == pitch) {
            v.gate.held = false;
            for env in &mut v.envs {
                env.release();
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        self.held_count = 0;
        self.arp.count = 0;
        self.seq.count = 0;
        for v in &mut self.voices {
            v.gate.held = false;
            for env in &mut v.envs {
                env.release();
            }
        }
    }

    /// Every modulation source for a voice, 0..1, indexed by `ModSource as usize`.
    fn sources(&self, voice: &Voice, lfos: &[f32; 4], macros: &[f32]) -> [f32; ModSource::ALL.len()] {
        let mut values = [0f32; ModSource::ALL.len()];
        for source in ModSource::ALL {
            values[source as usize] = self.source(voice, source, lfos, macros);
        }
        for (i, combo) in self.params.combos.iter().enumerate() {
            let value = |s: ModSource| if matches!(s, ModSource::Combo1 | ModSource::Combo2) { 0.0 } else { values[s as usize] };
            let combined = combo.op.apply(value(combo.a), value(combo.b));
            values[[ModSource::Combo1, ModSource::Combo2][i] as usize] = combined;
        }
        values
    }

    /// Value of a modulation source for a voice, 0..1 (combinations come from [`Self::sources`]).
    fn source(&self, voice: &Voice, source: ModSource, lfos: &[f32; 4], macros: &[f32]) -> f32 {
        match source {
            ModSource::SampleHold => voice.hold,
            ModSource::Smooth => voice.smooth,
            ModSource::Combo1 | ModSource::Combo2 => 0.0,
            ModSource::Sequencer => self.seq.value,
            ModSource::None => 0.0,
            ModSource::Env1 => voice.envs[0].level,
            ModSource::Env2 => voice.envs[1].level,
            ModSource::Env3 => voice.envs[2].level,
            ModSource::Lfo1 => lfos[0],
            ModSource::Lfo2 => lfos[1],
            ModSource::Lfo3 => lfos[2],
            ModSource::Lfo4 => lfos[3],
            ModSource::Velocity => voice.velocity,
            ModSource::Note => voice.note / 127.0,
            ModSource::Random => voice.random,
            ModSource::Macro1 => macros[0],
            ModSource::Macro2 => macros[1],
            ModSource::Macro3 => macros[2],
            ModSource::Macro4 => macros[3],
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

    pub fn render(&mut self, samples: &[Arc<Sample>], tables: &[Arc<Wavetable>], left: &mut [f32], right: &mut [f32]) {
        let mut base = [0f32; PARAMS];
        let mut params = self.params;
        for (i, value) in base.iter_mut().enumerate() {
            *value = params.param_mut(i).map_or(0.0, |v| *v);
        }
        let amounts: [f32; WAVE_SLOTS] = std::array::from_fn(|s| self.params.matrix[s].amount);
        let has_fx = self.fx.iter().flatten().any(|fx| fx.sounds());
        let mut offset = 0;
        while offset < left.len() {
            let n = (left.len() - offset).min(CONTROL - self.period_at);
            if self.period_at == 0 {
                self.arp_tick(CONTROL);
                self.seq_tick(CONTROL);
            }
            let mut scratch = self.scratch;
            scratch[0][..n].fill(0.0);
            scratch[1][..n].fill(0.0);
            {
                let [l, r] = &mut scratch;
                for v in 0..VOICES {
                    if self.voices[v].active {
                        let watch = Watch::before(&l[..n], &r[..n]);
                        self.render_voice(v, samples, tables, &base, &amounts, &mut l[..n], &mut r[..n]);
                        self.voices[v].taken.rendered(watch, &l[..n], &r[..n], &mut self.declick, self.sample_rate);
                    }
                }
                self.declick.render(&mut l[..n], &mut r[..n], self.sample_rate);
            }
            if has_fx {
                let tempo = self.tempo;
                let ctx = ProcessContext {
                    sample_rate: self.sample_rate,
                    tempo,
                    beat: self.clock * tempo / 60.0,
                    beats_per_sample: tempo / 60.0 / self.sample_rate as f64,
                    scale: 0,
                    meter: Default::default(),
                };
                let [l, r] = &mut scratch;
                for fx in self.fx.iter_mut().flatten() {
                    fx.process(&mut l[..n], &mut r[..n], &ctx);
                }
            }
            for i in 0..n {
                left[offset + i] += scratch[0][i];
                right[offset + i] += scratch[1][i];
            }
            self.scratch = scratch;
            self.frames += n as u64;
            self.clock = self.frames as f64 / self.sample_rate as f64;
            self.period_at += n;
            if self.period_at == CONTROL {
                self.period_at = 0;
                self.period += 1;
            }
            offset += n;
        }
    }

    /// Moves voice `v`'s modulation a period (`block` seconds) on, and works
    /// out its modulated parameters and frequency for the period (kept).
    fn modulate(&mut self, v: usize, base: &[f32; PARAMS], amounts: &[f32; WAVE_SLOTS], block: f32) -> ([f32; PARAMS], f32) {
        let p = &self.params;
        let pitch = self.pitch;
        // Modulation sources at the start of the period.
        let mut lfos = [0f32; 4];
        {
            let voice = &mut self.voices[v];
            for (l, value) in lfos.iter_mut().enumerate() {
                let lfo = &p.lfo[l];
                let rate = if lfo.sync_beats > 0.0 { (self.tempo / 60.0) as f32 / lfo.sync_beats } else { base[LFOS + l * 2] };
                let phase = match lfo.mode {
                    LfoMode::Free => ((self.clock * rate as f64).fract()) as f32,
                    LfoMode::Retrig => voice.lfo_phase[l].fract(),
                    LfoMode::Envelope => voice.lfo_phase[l].min(1.0),
                };
                let rise = base[LFOS + l * 2 + 1];
                let fade = if rise > 0.0 { (voice.age / rise).min(1.0) } else { 1.0 };
                *value = lfo.shape.value_at(phase) * fade;
                voice.lfo_phase[l] += rate * block;
            }
            // Random sources: a new target at the random rate; Smooth glides to it.
            let rate = base[WAVE_RANDOM_RATE].max(0.01);
            voice.random_phase += rate * block;
            if voice.random_phase >= 1.0 {
                voice.random_phase -= voice.random_phase.floor();
                voice.hold = voice.random();
            }
            voice.smooth += (voice.hold - voice.smooth) * (1.0 - (-block * rate * 4.0).exp());
        }
        let macros = &base[MACROS..MACROS + 4];
        let values = self.sources(&self.voices[v], &lfos, macros);
        let mut m = *base;
        for (s, slot) in p.matrix.iter().enumerate() {
            let dest = slot.dest as usize;
            if slot.source == ModSource::None || !wave_is_target(dest) {
                continue;
            }
            let mut value = values[slot.source as usize];
            if slot.bipolar {
                value = value * 2.0 - 1.0;
            }
            let info: &ParamInfo = &WAVE_PARAMS[dest];
            m[dest] = info.denormalize(info.normalize(m[dest]) + amounts[s] * value);
        }

        let voice = &mut self.voices[v];
        // Glide: about three time constants to reach the target.
        let glide = m[GLIDE];
        if glide > 0.0 && voice.note != voice.target {
            let k = 1.0 - (-block * 3000.0 / glide).exp();
            voice.note += (voice.target - voice.note) * k;
            if (voice.note - voice.target).abs() < 0.001 {
                voice.note = voice.target;
            }
        } else {
            voice.note = voice.target;
        }
        let freq = note_freq(voice.note) * pitch;
        voice.snap = (m, freq);
        voice.snap_period = self.period;
        (m, freq)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_voice(&mut self, v: usize, samples: &[Arc<Sample>], tables: &[Arc<Wavetable>], base: &[f32; PARAMS], amounts: &[f32; WAVE_SLOTS], left: &mut [f32], right: &mut [f32]) {
        let sr = self.sample_rate;
        let dt = 1.0 / sr;
        // Modulation moves a whole period at a time, at the period's first
        // piece (or a note's); the rest of it plays with what was found then.
        let block = CONTROL as f32 * dt;
        let fresh = self.voices[v].snap_period != self.period;
        // A note that starts has nowhere to come from.
        let starting = self.voices[v].snap_period == u64::MAX;
        let at = self.period_at;
        let (m, freq) = if fresh { self.modulate(v, base, amounts, block) } else { self.voices[v].snap };
        // Borrowed, not copied: this runs per voice every control period.
        let p = &self.params;
        let voice = &mut self.voices[v];

        let mut oscs: [Option<OscBlock>; 2] = [None, None];
        for (o, slot) in oscs.iter_mut().enumerate() {
            let osc = &p.osc[o];
            let ready = match osc.kind {
                OscKind::Wavetable | OscKind::Spectral => tables.get(o).is_some_and(|t| t.frames() > 0),
                OscKind::Sample | OscKind::Granular => samples.get(o).is_some_and(|s| s.frames() > 0),
                OscKind::Analog | OscKind::Harmonic => true,
            };
            if !osc.on || !ready {
                continue;
            }
            let at = |i: usize| m[o * OSC_PARAMS + i];
            let f = freq * 2f32.powf(at(1).round() + at(2).round() / 12.0 + at(3) / 1200.0);
            let count = (at(6).round() as usize).clamp(1, UNISON);
            let (detune, blend, width) = (at(7), at(8), at(9));
            let spread_cents = detune.powf(1.5) * 100.0;
            let mut incs = [0f32; UNISON];
            let mut gains = [(0f32, 0f32); UNISON];
            let mut norm = 0.0;
            let drift_cents = m[WAVE_OSC_EXTRA + o * 4] * 15.0;
            for u in 0..count {
                let s = if count == 1 { 0.0 } else { u as f32 / (count - 1) as f32 * 2.0 - 1.0 };
                // Analog drift: each voice wanders slowly towards a new random offset.
                let mut drift = 0.0;
                if osc.kind == OscKind::Analog && drift_cents > 0.0 {
                    if fresh {
                        let (current, target) = voice.drift[o][u];
                        let current = current + (target - current) * (1.0 - (-block * 2.0).exp());
                        let target = if (current - target).abs() < 0.05 { voice.random() * 2.0 - 1.0 } else { target };
                        voice.drift[o][u] = (current, target);
                    }
                    drift = voice.drift[o][u].0 * drift_cents;
                }
                incs[u] = f * 2f32.powf((s * spread_cents + drift) / 1200.0) / sr;
                let centre = 1.0 - s.abs();
                let g = if count == 1 { 1.0 } else { centre * (1.0 - blend) + (1.0 - centre) * blend + 0.05 };
                norm += g * g;
                let pan = (s * width + at(5)).clamp(-1.0, 1.0);
                let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                gains[u] = (angle.cos() * g, angle.sin() * g);
            }
            let scale = at(4) * std::f32::consts::SQRT_2 / norm.sqrt().max(1e-6);
            for g in &mut gains[..count] {
                g.0 *= scale;
                g.1 *= scale;
            }
            let top = f * 2f32.powf(spread_cents / 1200.0);
            if fresh && osc.kind == OscKind::Harmonic && voice.harmonic_wait == 0 {
                let x = WAVE_OSC_EXTRA + o * 4;
                let max_harmonic = ((sr * 0.5) / top.max(1.0)) as usize;
                build_harmonic(&mut voice.harmonic[o], &self.sine, m[x + 1], m[x + 2], m[x + 3], at(0), at(12), max_harmonic);
            }
            let (mut rates, mut start, mut grain_frames, mut grain_every, mut grain_scale) = ([0f64; UNISON], 0.0, 1, 1.0, 1.0);
            if let Some(sample) = samples.get(o).filter(|_| osc.kind.uses_sample()) {
                let native = if sample.sample_rate > 0.0 { sample.sample_rate } else { sr };
                for u in 0..count {
                    rates[u] = (incs[u] * sr / SAMPLE_ROOT_HZ * native / sr) as f64;
                }
                start = at(0).clamp(0.0, 0.999) as f64 * sample.frames() as f64;
                let g = WAVE_GRAIN + o * 4;
                let (size_ms, density) = (m[g], m[g + 1].max(0.5));
                grain_frames = ((size_ms * sr / 1000.0) as u32).max(16);
                grain_every = sr / density;
                // Overlapping grains add up; keep the level about the same at any density.
                grain_scale = 1.0 / (density * size_ms / 1000.0).max(1.0).sqrt();
            }
            let g = WAVE_GRAIN + o * 4;
            *slot = Some(OscBlock {
                kind: osc.kind,
                count: if osc.kind == OscKind::Granular { 1 } else { count },
                incs,
                gains,
                level: Wavetable::level_for(top, sr),
                smooth: osc.clean,
                clean: warp_stretch(osc.warp, at(12)).filter(|_| osc.clean && at(12) > 0.0).map(|stretch| Wavetable::level_for(top * stretch, sr)),
                position: at(0),
                warp: osc.warp,
                amount: at(12),
                rates,
                start,
                looping: osc.sample_loop,
                grain_frames,
                grain_every,
                spray: m[g + 2],
                grain_pitch: m[g + 3],
                width,
                pan: at(5),
                grain_scale: grain_scale * at(4),
            });
        }

        let cutoff = m[FILTER] * self.bright * 2f32.powf((voice.note - 60.0) / 12.0 * m[FILTER + 4]);
        let coefs = p.filter.on.then(|| FilterCoefs::new(p.filter.mode, cutoff, m[FILTER + 1], m[FILTER + 2], m[FILTER + 3], sr).tuned(p.filter.tuned).clean_drive(p.filter.clean_drive));
        let routes = p.filter.routes;
        let cutoff2 = m[WAVE_FILTER2] * self.bright * 2f32.powf((voice.note - 60.0) / 12.0 * m[WAVE_FILTER2 + 4]);
        let coefs2 = p.filter2.on.then(|| FilterCoefs::new(p.filter2.mode, cutoff2, m[WAVE_FILTER2 + 1], m[WAVE_FILTER2 + 2], m[WAVE_FILTER2 + 3], sr).tuned(p.filter2.tuned).clean_drive(p.filter2.clean_drive));
        let balance = m[WAVE_BALANCE].clamp(0.0, 1.0);
        let parallel = p.filter_parallel && coefs.is_some() && coefs2.is_some();
        if fresh {
            voice.harmonic_wait = if voice.harmonic_wait == 0 { HARMONIC_EVERY } else { voice.harmonic_wait - 1 };
        }
        let sub_inc = freq * 2f32.powf(m[SUB].round()) / sr;
        let sub_angle = (m[SUB + 2].clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        let sub_gain = (sub_angle.cos() * m[SUB + 1] * std::f32::consts::SQRT_2, sub_angle.sin() * m[SUB + 1] * std::f32::consts::SQRT_2);
        let noise_a = (-TAU * m[NOISE + 1] / sr).exp();
        let noise_angle = (m[NOISE + 2].clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        let noise_gain = (noise_angle.cos() * m[NOISE] * std::f32::consts::SQRT_2, noise_angle.sin() * m[NOISE] * std::f32::consts::SQRT_2);
        let master = m[MASTER];
        let now = Levels { gains: std::array::from_fn(|o| oscs[o].as_ref().map_or([(0.0, 0.0); UNISON], |b| b.gains)), positions: std::array::from_fn(|o| oscs[o].as_ref().map_or(0.0, |b| b.position)), warps: std::array::from_fn(|o| oscs[o].as_ref().map_or(0.0, |b| b.amount)), sub: sub_gain, noise: noise_gain, master, filters: [coefs, coefs2] };
        if fresh {
            voice.was = if starting { now } else { voice.now };
        }
        voice.now = now;
        let was = voice.was;
        // How far each level has to go in the period (nothing, if nothing moves it).
        let far = |a: (f32, f32), b: (f32, f32)| (b.0 - a.0, b.1 - a.1);
        let gains_far: [[(f32, f32); UNISON]; 2] = std::array::from_fn(|o| std::array::from_fn(|u| far(was.gains[o][u], now.gains[o][u])));
        let (sub_far, noise_far, master_far) = (far(was.sub, now.sub), far(was.noise, now.noise), master - was.master);
        let moving: [Option<(FilterCoefs, FilterCoefs)>; 2] = std::array::from_fn(|f| was.filters[f].zip(now.filters[f]).filter(|(a, b)| a != b));

        // Nothing moves this period (most of them): nothing to work out
        // per sample.
        let still = moving.iter().all(|m| m.is_none())
            && gains_far.iter().flatten().all(|g| *g == (0.0, 0.0))
            && (sub_far, noise_far, master_far) == ((0.0, 0.0), (0.0, 0.0), 0.0)
            && was.positions == now.positions
            && was.warps == now.warps;
        let counts: [usize; 2] = std::array::from_fn(|o| oscs[o].as_ref().map_or(0, |b| b.count));
        // Where they are on this sample.
        let mut live = now;
        for (i, (out_l, out_r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            if !still {
                let t = (at + i + 1) as f32 / CONTROL as f32;
                for o in 0..2 {
                    for u in 0..counts[o] {
                        live.gains[o][u] = (was.gains[o][u].0 + gains_far[o][u].0 * t, was.gains[o][u].1 + gains_far[o][u].1 * t);
                    }
                    live.positions[o] = was.positions[o] + (now.positions[o] - was.positions[o]) * t;
                    live.warps[o] = was.warps[o] + (now.warps[o] - was.warps[o]) * t;
                    if let Some((a, b)) = &moving[o] {
                        live.filters[o] = Some(FilterCoefs::between(a, b, t));
                    }
                }
                live.sub = (was.sub.0 + sub_far.0 * t, was.sub.1 + sub_far.1 * t);
                live.noise = (was.noise.0 + noise_far.0 * t, was.noise.1 + noise_far.1 * t);
                live.master = was.master + master_far * t;
            }
            let (sub_gain, noise_gain, master) = (live.sub, live.noise, live.master);
            let (coefs, coefs2) = (live.filters[0].as_ref(), live.filters[1].as_ref());
            if voice.gate.tick() {
                for env in &mut voice.envs {
                    env.release();
                }
            }
            let amp = voice.envs[0].tick(&p.env[0], dt);
            voice.envs[1].tick(&p.env[1], dt);
            voice.envs[2].tick(&p.env[2], dt);
            if voice.envs[0].stage == Stage::Idle {
                voice.active = false;
                break;
            }
            // (filtered, dry) per channel.
            let (mut wet_l, mut wet_r, mut dry_l, mut dry_r) = (0.0, 0.0, 0.0, 0.0);
            for o in 0..2 {
                let Some(block) = &oscs[o] else { continue };
                let gain = |u: usize| live.gains[o][u];
                let (position, warp) = (live.positions[o], live.warps[o]);
                let (mut l, mut r, mut mono) = (0.0, 0.0, 0.0);
                match block.kind {
                    OscKind::Sample => {
                        let sample = &samples[o];
                        let frames = sample.frames() as f64;
                        for u in 0..block.count {
                            let position = &mut voice.positions[o][u];
                            if *position < 0.0 {
                                *position = block.start;
                            }
                            if *position >= frames {
                                if !block.looping || frames - block.start < 2.0 {
                                    continue;
                                }
                                *position = block.start + (*position - frames) % (frames - block.start);
                            }
                            let (a, b) = sample.read(*position);
                            *position += block.rates[u];
                            l += a * gain(u).0;
                            r += b * gain(u).1;
                            mono += (a + b) * 0.5;
                        }
                    }
                    OscKind::Granular => {
                        let sample = &samples[o];
                        let frames = sample.frames() as f64;
                        voice.grain_wait[o] -= 1.0;
                        if voice.grain_wait[o] <= 0.0 {
                            voice.grain_wait[o] += block.grain_every;
                            if let Some(free) = voice.grains[o].iter().position(|g| !g.active) {
                                let (r1, r2, r3) = (voice.random(), voice.random(), voice.random());
                                let place = (block.position + block.spray * (r1 - 0.5)).clamp(0.0, 0.999) as f64;
                                let pan = ((r2 * 2.0 - 1.0) * block.width + block.pan).clamp(-1.0, 1.0);
                                let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                                let detune = 2f64.powf((block.grain_pitch * (r3 * 2.0 - 1.0)) as f64 / 12.0);
                                voice.grains[o][free] = Grain {
                                    active: true,
                                    position: place * frames,
                                    rate: block.rates[0] * detune,
                                    age: 0,
                                    length: block.grain_frames,
                                    gain: (angle.cos() * block.grain_scale * std::f32::consts::SQRT_2, angle.sin() * block.grain_scale * std::f32::consts::SQRT_2),
                                };
                            }
                        }
                        for grain in voice.grains[o].iter_mut().filter(|g| g.active) {
                            // Hann window over the grain.
                            let env = 0.5 - 0.5 * (TAU * grain.age as f32 / grain.length as f32).cos();
                            let (a, b) = sample.read(grain.position);
                            grain.position += grain.rate;
                            grain.age += 1;
                            if grain.age >= grain.length || grain.position >= frames {
                                grain.active = false;
                            }
                            l += a * env * grain.gain.0;
                            r += b * env * grain.gain.1;
                            mono += (a + b) * 0.5 * env;
                        }
                    }
                    OscKind::Analog => {
                        let fm = if block.warp == OscWarp::Fm { voice.last[1 - o] * warp * 0.5 } else { 0.0 };
                        let morph = position.clamp(0.0, 1.0) * 3.0;
                        let width = 0.5 - warp.clamp(0.0, 1.0) * 0.45;
                        for u in 0..block.count {
                            let phase = voice.phases[o][u];
                            let inc = block.incs[u];
                            let ph = (phase + fm).rem_euclid(1.0);
                            let sine = (ph * TAU).sin();
                            let triangle = 1.0 - 4.0 * (((ph + 0.25) % 1.0) - 0.5).abs();
                            let saw = if block.smooth { saw_clean } else { saw };
                            let saw_wave = saw((ph + 0.5) % 1.0, inc);
                            // A pulse of any width from two band-limited saws.
                            let pulse = (saw(ph, inc) - saw((ph + width) % 1.0, inc)) * 0.5 / (1.0 - width.min(1.0 - width));
                            let value = match morph {
                                m if m < 1.0 => sine + (triangle - sine) * m,
                                m if m < 2.0 => triangle + (saw_wave - triangle) * (m - 1.0),
                                m => saw_wave + (pulse - saw_wave) * (m - 2.0),
                            };
                            let next = phase + inc;
                            voice.phases[o][u] = next - next.floor();
                            l += value * gain(u).0;
                            r += value * gain(u).1;
                            mono += value;
                        }
                    }
                    OscKind::Harmonic => {
                        let fm = if block.warp == OscWarp::Fm { voice.last[1 - o] * warp * 0.5 } else { 0.0 };
                        let table = &voice.harmonic[o];
                        for u in 0..block.count {
                            let phase = voice.phases[o][u];
                            let x = (phase + fm).rem_euclid(1.0) * HARMONIC_TABLE as f32;
                            let i = x as usize % HARMONIC_TABLE;
                            let a = table[i];
                            let value = a + (table[(i + 1) % HARMONIC_TABLE] - a) * (x - x.floor());
                            let next = phase + block.incs[u];
                            voice.phases[o][u] = next - next.floor();
                            l += value * gain(u).0;
                            r += value * gain(u).1;
                            mono += value;
                        }
                    }
                    OscKind::Wavetable | OscKind::Spectral => {
                let table = &tables[o];
                let fm = if block.warp == OscWarp::Fm { voice.last[1 - o] * warp * 0.5 } else { 0.0 };
                for u in 0..block.count {
                    let phase = voice.phases[o][u];
                    let mut s = match block.clean {
                        Some(level) => {
                            let mut sum = 0.0;
                            for k in 0..WARP_READS {
                                let off = (k as f32 + 0.5) / WARP_READS as f32 * 2.0 - 1.0;
                                let read = warp_phase(block.warp, warp, (phase + off * block.incs[u]).rem_euclid(1.0));
                                sum += (1.0 - off.abs()) * table.read(level, position, read);
                            }
                            // The weights add up to half the reads.
                            sum * (2.0 / WARP_READS as f32)
                        }
                        None => table.read(block.level, position, warp_phase(block.warp, warp, (phase + fm).rem_euclid(1.0))),
                    };
                    if block.warp == OscWarp::Quantize && warp > 0.0 {
                        let steps = 2f32.powf(1.0 + (1.0 - warp) * 7.0);
                        s = (s * steps).round() / steps;
                    }
                    let next = phase + block.incs[u];
                    voice.phases[o][u] = next - next.floor();
                    l += s * gain(u).0;
                    r += s * gain(u).1;
                    mono += s;
                }
                    }
                }
                voice.last[o] = mono / block.count as f32;
                if routes[o] {
                    wet_l += l;
                    wet_r += r;
                } else {
                    dry_l += l;
                    dry_r += r;
                }
            }
            if p.sub.on {
                let ph = voice.sub_phase;
                let s = match p.sub.wave {
                    KickWave::Sine => (ph * TAU).sin(),
                    KickWave::Triangle => 1.0 - 4.0 * (((ph + 0.25) % 1.0) - 0.5).abs(),
                    KickWave::Square => square(ph, sub_inc),
                    KickWave::Saw => saw(ph, sub_inc),
                };
                voice.sub_phase = (ph + sub_inc).fract();
                if routes[2] {
                    wet_l += s * sub_gain.0;
                    wet_r += s * sub_gain.1;
                } else {
                    dry_l += s * sub_gain.0;
                    dry_r += s * sub_gain.1;
                }
            }
            if p.noise.on {
                voice.noise_seed ^= voice.noise_seed << 13;
                voice.noise_seed ^= voice.noise_seed >> 17;
                voice.noise_seed ^= voice.noise_seed << 5;
                let white = voice.noise_seed as f32 / u32::MAX as f32 * 2.0 - 1.0;
                voice.noise_lp = white + noise_a * (voice.noise_lp - white);
                let s = voice.noise_lp;
                if routes[3] {
                    wet_l += s * noise_gain.0;
                    wet_r += s * noise_gain.1;
                } else {
                    dry_l += s * noise_gain.0;
                    dry_r += s * noise_gain.1;
                }
            }
            if parallel {
                let (c1, c2) = (coefs.unwrap(), coefs2.unwrap());
                let (a_l, a_r) = (c1.process(&mut voice.filter[0], wet_l), c1.process(&mut voice.filter[1], wet_r));
                let (b_l, b_r) = (c2.process(&mut voice.filter2[0], wet_l), c2.process(&mut voice.filter2[1], wet_r));
                wet_l = a_l + (b_l - a_l) * balance;
                wet_r = a_r + (b_r - a_r) * balance;
            } else {
                if let Some(c) = coefs {
                    wet_l = c.process(&mut voice.filter[0], wet_l);
                    wet_r = c.process(&mut voice.filter[1], wet_r);
                }
                if let Some(c) = coefs2 {
                    wet_l = c.process(&mut voice.filter2[0], wet_l);
                    wet_r = c.process(&mut voice.filter2[1], wet_r);
                }
            }
            let gain = amp * voice.velocity * master;
            *out_l += (wet_l + dry_l) * gain;
            *out_r += (wet_r + dry_r) * gain;
        }
        if fresh {
            voice.age += block;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModSlot, Shape, WaveFilter};
    use crate::wavetable::builtin;

    const SR: f32 = 48_000.0;

    fn tables() -> Vec<Arc<Wavetable>> {
        vec![Arc::new(builtin(0)), Arc::new(builtin(0))]
    }

    fn play(params: WaveParams, notes: &[u8], frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut synth = WaveSynth::new(params, SR);
        for &n in notes {
            synth.note_on(n, 1.0, HELD);
        }
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        let t = tables();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            synth.render(&[], &t, a, b);
        }
        (l, r)
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// A voice that starts a new note while it sounds (a mono voice played
    /// again, a voice stolen with all of them busy) doesn't click: the sound
    /// moves no faster across that sample than it does within the notes.
    #[test]
    fn a_voice_taken_while_it_sounds_does_not_click() {
        let steepest = |x: &[f32]| x.windows(2).fold(0f32, |m, w| m.max((w[1] - w[0]).abs()));
        let t = tables();
        let frames = SR as usize / 4;
        let run = |synth: &mut WaveSynth| {
            let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
            for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                synth.render(&[], &t, a, b);
            }
            l
        };
        for mono in [true, false] {
            let mut synth = WaveSynth::new(WaveParams { mono, ..Default::default() }, SR);
            let held = if mono { 1 } else { VOICES };
            for k in 0..held {
                synth.note_on(48 + k as u8 % 12, 1.0, HELD);
            }
            let before = run(&mut synth);
            synth.note_on(55, 1.0, HELD);
            let after = run(&mut synth);
            let across = (after[0] - before[frames - 1]).abs();
            let within = steepest(&before[frames / 2..]).max(steepest(&after[frames / 2..]));
            println!("mono {mono}: {across:.4} across the new note, {within:.4} within the notes");
            assert!(within > 0.0);
            assert!(across <= within * 1.5, "mono {mono}: {across} {within}");
        }
    }

    /// With every voice busy, a new note takes one already let go before a
    /// key still held.
    #[test]
    fn a_new_note_takes_a_released_voice_before_a_held_one() {
        let t = tables();
        let mut params = WaveParams::default();
        params.env[0].release = 2.0;
        let mut synth = WaveSynth::new(params, SR);
        // The oldest note stays held; the rest are let go and ring on.
        for k in 0..VOICES as u8 {
            synth.note_on(40 + k, 1.0, HELD);
        }
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        synth.render(&[], &t, &mut l, &mut r);
        for k in 1..VOICES as u8 {
            synth.note_off(40 + k);
        }
        synth.note_on(100, 1.0, HELD);
        assert!(synth.voices.iter().any(|v| v.active && v.gate.held && v.gate.pitch == 40), "the held note went");
        assert!(synth.voices.iter().any(|v| v.active && v.gate.pitch == 100));
    }

    /// The comb's ringing to an impulse, `seconds` long, tuned to `hz` at full resonance.
    fn comb_ring(hz: f32, seconds: f32) -> Vec<f32> {
        let coefs = FilterCoefs::new(WaveFilter::Comb, hz, 1.0, 0.0, 1.0, SR);
        let mut state = FilterState::new();
        (0..(SR * seconds) as usize).map(|n| coefs.process(&mut state, if n == 0 { 1.0 } else { 0.0 })).collect()
    }

    /// The magnitude of `x` at `hz` (a direct DFT).
    fn magnitude(x: &[f32], hz: f32) -> f32 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, &v) in x.iter().enumerate() {
            let w = std::f64::consts::TAU * hz as f64 * n as f64 / SR as f64;
            re += v as f64 * w.cos();
            im -= v as f64 * w.sin();
        }
        (re * re + im * im).sqrt() as f32
    }

    /// The comb rings at the note it is set to, high up too (a whole-sample
    /// delay put C6 a third of a semitone sharp): its strongest peak within
    /// a semitone of the cutoff, a quarter of a cent apart, is under 2 cents off.
    #[test]
    fn the_comb_is_in_tune() {
        let mut off = Vec::new();
        for note in [36.0f32, 48.0, 60.0, 72.0, 84.0, 91.0, 96.0] {
            let hz = note_freq(note);
            let ring = comb_ring(hz, 0.5);
            let cents = (-400..=400).map(|c| c as f32 / 4.0).max_by(|a, b| magnitude(&ring, hz * 2f32.powf(a / 1200.0)).total_cmp(&magnitude(&ring, hz * 2f32.powf(b / 1200.0)))).unwrap();
            if cents.abs() > 2.0 {
                off.push(format!("note {note}: {cents:+.2} cents"));
            }
        }
        assert!(off.is_empty(), "{}", off.join(", "));
    }

    /// A plucked comb loses its high harmonics sooner than its fundamental,
    /// as a string does (a bare comb rings every harmonic as long: metallic).
    #[test]
    fn the_comb_darkens_as_it_rings() {
        let hz = note_freq(57.0);
        let ring = comb_ring(hz, 1.0);
        let share = |x: &[f32]| magnitude(x, hz * 8.0) / magnitude(x, hz);
        let (early, late) = (share(&ring[..4800]), share(&ring[38_400..43_200]));
        assert!(late < early * 0.5, "the 8th harmonic against the first: {early} early, {late} late");
    }

    fn frequency(signal: &[f32]) -> f32 {
        let crossings = signal.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        crossings as f32 * SR / signal.len() as f32
    }

    /// Saw (position 2/3 of the basic table).
    fn saw_patch() -> WaveParams {
        let mut p = WaveParams::default();
        p.osc[0].position = 2.0 / 3.0;
        p.osc[0].random_phase = 0.0;
        p
    }

    #[test]
    fn a_note_plays_at_its_pitch_and_releases() {
        let mut synth = WaveSynth::new(saw_patch(), SR);
        synth.note_on(69, 1.0, HELD);
        let t = tables();
        let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        synth.render(&[], &t, &mut l, &mut r);
        assert!((frequency(&l[4800..]) - 440.0).abs() < 2.0, "{}", frequency(&l[4800..]));
        synth.note_off(69);
        let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
        synth.render(&[], &t, &mut l, &mut r);
        assert!(rms(&l[19_200..]) < 1e-4, "released");
        assert!(!synth.voices.iter().any(|v| v.active));
    }

    #[test]
    fn octave_semitone_and_fine_tune_the_oscillator() {
        let mut p = saw_patch();
        p.osc[0].octave = -1.0;
        p.osc[0].semi = 7.0;
        let (l, _) = play(p, &[69], 48_000);
        let expected = 440.0 * 0.5 * 2f32.powf(7.0 / 12.0);
        assert!((frequency(&l) - expected).abs() < 2.0);
    }

    #[test]
    fn unison_widens_the_stereo_image() {
        let (l, r) = play(saw_patch(), &[57], 24_000);
        let mono: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
        assert!(mono < 1e-3, "one voice is centred");
        let mut p = saw_patch();
        p.osc[0].unison = 7.0;
        p.osc[0].detune = 0.4;
        p.osc[0].random_phase = 1.0;
        let (l, r) = play(p, &[57], 24_000);
        let side = rms(&l.iter().zip(&r).map(|(a, b)| a - b).collect::<Vec<_>>());
        assert!(side > 0.05, "detuned unison is wide: {side}");
        assert!(rms(&l) < 1.0, "unison is normalised: {}", rms(&l));
    }

    /// The tuned Ladder resonates on its cutoff wherever it is, and starts
    /// ringing by itself at the same Res all the way up. (The one before goes
    /// sharp as the cutoff rises, more than a semitone by 3 kHz, and stops
    /// ringing by itself.) A new sound
    /// has it; the factory sounds and a saved one that says nothing don't.
    #[test]
    fn the_tuned_ladder_resonates_on_its_cutoff() {
        // Where the resonance peaks, just under where it rings by itself:
        // the strongest frequency of what an impulse leaves, looked for a
        // fifth either side of the cutoff.
        let peak = |tuned: bool, cutoff: f32| {
            let c = FilterCoefs::new(WaveFilter::Ladder, cutoff, 0.94, 0.0, 1.0, SR).tuned(tuned);
            let mut s = FilterState::new();
            let h: Vec<f32> = (0..48_000).map(|i| c.process(&mut s, if i == 0 { 0.01 } else { 0.0 })).collect();
            let level = |hz: f32| {
                let (re, im) = h.iter().enumerate().fold((0f64, 0f64), |(re, im), (i, v)| {
                    let a = (TAU * hz * i as f32 / SR) as f64;
                    (re + *v as f64 * a.cos(), im + *v as f64 * a.sin())
                });
                re * re + im * im
            };
            let cents = (-700..=700).step_by(10).max_by(|a, b| level(cutoff * 2f32.powf(*a as f32 / 1200.0)).total_cmp(&level(cutoff * 2f32.powf(*b as f32 / 1200.0)))).unwrap();
            cents as f32
        };
        let rings_from = |tuned: bool, cutoff: f32| {
            (60..=100).map(|r| r as f32 * 0.01).find(|&res| {
                let c = FilterCoefs::new(WaveFilter::Ladder, cutoff, res, 0.0, 1.0, SR).tuned(tuned);
                let mut s = FilterState::new();
                let h: Vec<f32> = (0..48_000).map(|i| c.process(&mut s, if i == 0 { 0.1 } else { 0.0 })).collect();
                rms(&h[40_000..]) > 1e-4
            })
        };
        let mut worst_before = 0f32;
        for cutoff in [100.0f32, 300.0, 1000.0, 3000.0, 6000.0, 10_000.0] {
            let (before, now) = (peak(false, cutoff), peak(true, cutoff));
            let (rings_before, rings) = (rings_from(false, cutoff), rings_from(true, cutoff));
            println!("corte {cutoff} Hz: resuena a {before:+.0} cents antes y {now:+.0} ahora; oscila solo desde Res {rings_before:?} antes y {rings:?} ahora");
            assert!(now.abs() <= 30.0, "{cutoff} Hz: {now} cents off");
            assert!(rings.is_some_and(|r| (0.94..=0.97).contains(&r)), "{cutoff} Hz: rings by itself from {rings:?}");
            worst_before = worst_before.max(before.abs());
        }
        assert!(worst_before > 100.0, "the one before was off by {worst_before} cents");
        // It never runs away.
        let c = FilterCoefs::new(WaveFilter::Ladder, 2000.0, 1.0, 1.0, 1.0, SR).tuned(true);
        let mut s = FilterState::new();
        let loud = (0..48_000).map(|i| c.process(&mut s, ((i * 7919) % 200) as f32 / 100.0 - 1.0)).fold(0f32, |m, v| m.max(v.abs()));
        assert!(loud.is_finite() && loud < 8.0, "{loud}");
        // Who has it.
        assert!(WaveParams::default().filter.tuned && WaveParams::polychrome().filter.tuned);
        for (name, p) in WaveParams::presets().into_iter().chain(WaveParams::polychrome_presets()) {
            assert_eq!(p.filter.tuned, name == "Init" || WaveParams::MADE_SINCE.contains(&name), "{name}");
        }
        let mut old = serde_json::to_value(WaveParams::polychrome()).unwrap();
        old["filter"].as_object_mut().unwrap().remove("tuned");
        let old: WaveParams = serde_json::from_value(old).unwrap();
        assert!(!old.filter.tuned);
        let again: WaveParams = serde_json::from_str(&serde_json::to_string(&WaveParams::polychrome()).unwrap()).unwrap();
        assert!(again.filter.tuned);
    }

    #[test]
    fn filter_modes_shape_the_spectrum() {
        let highs = |x: &[f32]| rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>());
        let open = play(saw_patch(), &[48], 24_000).0;
        for (mode, darker) in [(WaveFilter::Lp12, true), (WaveFilter::Lp24, true), (WaveFilter::Ladder, true), (WaveFilter::Hp24, false)] {
            let mut p = saw_patch();
            p.filter.on = true;
            p.filter.mode = mode;
            p.filter.cutoff = 500.0;
            let out = play(p, &[48], 24_000).0;
            assert!(out.iter().all(|s| s.is_finite()));
            if darker {
                assert!(highs(&out) < highs(&open) * 0.5, "{mode:?} darkens");
            } else {
                assert!(rms(&out) < rms(&open) * 0.8, "{mode:?} removes the low end");
            }
        }
        for mode in WaveFilter::ALL {
            let mut p = saw_patch();
            p.filter.on = true;
            p.filter.mode = mode;
            p.filter.resonance = 1.0;
            p.filter.drive = 1.0;
            let out = play(p, &[36, 48, 60], 24_000).0;
            assert!(out.iter().all(|s| s.is_finite() && s.abs() < 8.0), "{mode:?} is stable");
        }
    }

    #[test]
    fn the_matrix_moves_the_cutoff_with_an_lfo() {
        let mut p = saw_patch();
        p.filter.on = true;
        p.filter.mode = WaveFilter::Lp24;
        p.filter.cutoff = 200.0;
        // A square LFO at 2 Hz: half a second open, half closed.
        p.lfo[0].shape = Shape::from_points(&[(0.0, 1.0, 0.0), (0.5, 1.0, 0.0), (0.501, 0.0, 0.0), (1.0, 0.0, 0.0)]);
        p.lfo[0].mode = LfoMode::Retrig;
        p.lfo[0].rate = 1.0;
        p.matrix[0] = ModSlot { source: ModSource::Lfo1, dest: FILTER as u8, amount: 0.8, bipolar: false };
        let (l, _) = play(p, &[48], 48_000);
        let highs = |x: &[f32]| rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>());
        assert!(highs(&l[4800..19_200]) > highs(&l[28_800..43_200]) * 3.0, "open while the LFO is up");
    }

    /// How much of a held sine is the modulation's steps: what is left from
    /// 1 to 4 kHz (the steps come 1500 times a second), against the whole.
    fn steps(dest: usize, amount: f32, filter: bool) -> f32 {
        let l = swept(57, dest, amount, filter);
        let window = |i: usize| 0.5 - 0.5 * (TAU * i as f32 / 8192.0).cos();
        let (mut re, mut im) = (l[8_000..16_192].iter().enumerate().map(|(i, v)| v * window(i)).collect::<Vec<_>>(), vec![0.0; 8192]);
        crate::dsp::fft(&mut re, &mut im);
        let band = |from: f32, to: f32| ((from * 8192.0 / SR) as usize..(to * 8192.0 / SR) as usize).map(|k| re[k] * re[k] + im[k] * im[k]).sum::<f32>();
        20.0 * (band(1000.0, 4000.0) / band(20.0, 4000.0)).sqrt().log10()
    }

    /// A held `note` with an 8 Hz triangle LFO on `dest`.
    fn swept(note: u8, dest: usize, amount: f32, filter: bool) -> Vec<f32> {
        let mut p = WaveParams::default();
        p.osc[0].position = 0.0;
        p.osc[0].random_phase = 0.0;
        p.filter.on = filter;
        p.filter.mode = WaveFilter::Lp12;
        p.filter.cutoff = 300.0;
        p.lfo[0].shape = Shape::from_points(&[(0.0, 0.0, 0.0), (0.5, 1.0, 0.0), (1.0, 0.0, 0.0)]);
        p.lfo[0].mode = LfoMode::Retrig;
        p.lfo[0].rate = 8.0;
        p.matrix[0] = ModSlot { source: ModSource::Lfo1, dest: dest as u8, amount, bipolar: false };
        play(p, &[note], 24_000).0
    }

    /// What the modulation sets once per control period (levels, pans, the
    /// place in the table, the filters) moves along the period, not in a
    /// step at its start: the steps, 1500 a second, were a buzz 40 dB under
    /// the note.
    #[test]
    fn modulation_moves_without_steps() {
        for (name, dest, amount, filter, under) in [("nivel", 4, -1.0, false, -60.0), ("pan", 5, 1.0, false, -60.0), ("corte", FILTER, 0.5, true, -75.0), ("master", MASTER, -1.0, false, -60.0)] {
            let heard = steps(dest, amount, filter);
            println!("{name}: {heard:.1} dB");
            assert!(heard < under, "{name}: {heard} dB");
        }
        // Moving along the table changes the wave itself, so its steps are
        // looked for where they fall: how much the wave bends where a
        // period starts (every 32 samples), against anywhere else, on a
        // low note (which bends little by itself).
        let l = swept(33, 0, 0.1, false);
        let bend = |n: usize| (l[n] - 2.0 * l[n - 1] + l[n - 2]).abs();
        let (at_starts, elsewhere) = (8_000..16_000).fold((0f32, 0f32), |(a, b), n| if n % CONTROL == 0 { (a.max(bend(n)), b) } else { (a, b.max(bend(n))) });
        println!("posición: se dobla {at_starts:.5} donde empieza un periodo y {elsewhere:.5} en lo demás");
        // (In steps it bent half as much there as at its worst elsewhere.)
        assert!(at_starts < 0.2 * elsewhere, "{at_starts} where a period starts, {elsewhere} elsewhere");
    }

    #[test]
    fn tempo_synced_lfos_follow_the_tempo() {
        let mut p = saw_patch();
        p.lfo[0].shape = Shape::from_points(&[(0.0, 0.0, 0.0), (0.5, 1.0, 0.0), (1.0, 0.0, 0.0)]);
        p.lfo[0].sync_beats = 1.0;
        p.lfo[0].mode = LfoMode::Retrig;
        p.matrix[0] = ModSlot { source: ModSource::Lfo1, dest: 4, amount: -1.0, bipolar: false };
        let mut synth = WaveSynth::new(p, SR);
        synth.set_tempo(120.0);
        synth.note_on(60, 1.0, HELD);
        let t = tables();
        let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        synth.render(&[], &t, &mut l, &mut r);
        // One beat is 0.5 s: the level dips to silence at 0.25 s and 0.75 s.
        let window = |at: f32| rms(&l[(at * SR) as usize - 600..(at * SR) as usize + 600]);
        assert!(window(0.25) < window(0.5) * 0.2 && window(0.75) < window(0.5) * 0.2);
    }

    #[test]
    fn mono_legato_glides_without_a_new_attack() {
        let mut p = saw_patch();
        p.mono = true;
        p.legato = true;
        p.glide = 200.0;
        p.env[0].attack = 0.3;
        let mut synth = WaveSynth::new(p, SR);
        let t = tables();
        synth.note_on(57, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
        synth.render(&[], &t, &mut l, &mut r);
        let level_before = synth.voices[0].envs[0].level;
        synth.note_on(69, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        synth.render(&[], &t, &mut l, &mut r);
        assert!(synth.voices[0].envs[0].level >= level_before, "the envelope kept going");
        assert!((frequency(&l[2400..4800]) - 440.0).abs() > 30.0, "still gliding at first");
        assert!((frequency(&l[24_000..]) - 440.0).abs() < 3.0, "arrived");
        assert_eq!(synth.voices.iter().filter(|v| v.active).count(), 1);
        // Releasing the top note goes back to the held one.
        synth.note_off(69);
        l.fill(0.0);
        r.fill(0.0);
        synth.render(&[], &t, &mut l, &mut r);
        assert!((frequency(&l[24_000..]) - 220.0).abs() < 3.0);
    }

    /// The 808 preset glides only between notes that overlap: a note after a
    /// rest, or one that just touches the last, starts on its own pitch.
    #[test]
    fn legato_glides_only_into_overlapping_notes() {
        let p = WaveParams::polychrome_presets().into_iter().find(|(n, _)| *n == "Bajo · 808 con glide").expect("808 preset").1;
        assert!(p.mono && p.legato && p.glide > 0.0);
        let t = tables();
        let ms = |m: f32| (m * SR / 1000.0) as usize;
        let run = |synth: &mut WaveSynth, frames: usize| {
            let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
            synth.render(&[], &t, &mut l, &mut r);
        };
        let mut synth = WaveSynth::new(p, SR);
        // Overlapping: 27 still sounds when 39 starts, so 39 slides up from 27.
        synth.note_on(27, 1.0, ms(500.0) as u32);
        run(&mut synth, ms(400.0));
        let level = synth.voices[0].envs[0].level;
        synth.note_on(39, 1.0, ms(500.0) as u32);
        run(&mut synth, ms(15.0));
        let tied = synth.voices[0].note;
        assert!(tied < 36.0, "a tied note starts below its pitch and slides: {tied}");
        assert!(synth.voices[0].envs[0].level < level * 1.05, "no new attack on a tied note");
        run(&mut synth, ms(600.0));
        assert!((synth.voices[0].note - 39.0).abs() < 0.1, "the slide arrives");
        // After a rest: no glide at all.
        run(&mut synth, ms(300.0));
        synth.note_on(25, 1.0, ms(500.0) as u32);
        run(&mut synth, ms(15.0));
        let after_rest = synth.voices[0].note;
        assert!((after_rest - 25.0).abs() < 0.05, "a note after a rest starts on its pitch: {after_rest}");
        // Back to back (the next starts as this one ends): a new note, not a slide.
        run(&mut synth, ms(500.0) - ms(15.0));
        synth.note_on(32, 1.0, ms(500.0) as u32);
        run(&mut synth, ms(15.0));
        let touching = synth.voices[0].note;
        assert!((touching - 32.0).abs() < 0.05, "touching notes don't slide: {touching}");
        // Mono (not legato) keeps the classic always-on portamento.
        let mut synth = WaveSynth::new(WaveParams { legato: false, ..p }, SR);
        synth.note_on(39, 1.0, ms(200.0) as u32);
        run(&mut synth, ms(500.0));
        synth.note_on(27, 1.0, ms(200.0) as u32);
        run(&mut synth, ms(15.0));
        assert!(synth.voices[0].note > 30.0, "mono glides from the last note: {}", synth.voices[0].note);
    }

    /// The 808 preset playing a trap line (one tied slide, notes after rests),
    /// its pitch printed 15 and 120 ms into each note, Legato (as it is) and
    /// Mono (how clip notes played before glides kept to tied notes):
    /// `PULSE_WAV=dir cargo test --release -p engine glide_808_wavs -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn glide_808_wavs() {
        let Some(dir) = std::env::var("PULSE_WAV").ok() else { return };
        std::fs::create_dir_all(&dir).unwrap();
        let p = WaveParams::polychrome_presets().into_iter().find(|(n, _)| *n == "Bajo · 808 con glide").unwrap().1;
        let t = tables();
        let beat = 60.0 / 140.0 * SR as f64;
        // (beat, length, pitch): 39 slides in over 27, then notes after rests.
        let notes = [(0.0, 3.0, 27u8), (2.75, 1.0, 39), (4.0, 2.5, 27), (8.0, 3.0, 25), (12.0, 1.5, 32)];
        for (name, legato) in [("808-legato", true), ("808-mono", false)] {
            let mut synth = WaveSynth::new(WaveParams { legato, ..p }, SR);
            let total = (16.0 * beat) as usize;
            let (mut l, mut r) = (vec![0.0; total], vec![0.0; total]);
            let starts: Vec<usize> = notes.iter().map(|n| (n.0 * beat) as usize).collect();
            let probes: Vec<(usize, usize)> = starts.iter().enumerate().flat_map(|(k, &s)| [(k, s + (0.015 * SR) as usize), (k, s + (0.12 * SR) as usize)]).collect();
            let mut marks: Vec<usize> = starts.iter().copied().chain(probes.iter().map(|p| p.1)).collect();
            marks.push(total);
            marks.sort();
            marks.dedup();
            let mut at = 0;
            let mut line = String::new();
            for &mark in &marks {
                synth.render(&[], &t, &mut l[at..mark], &mut r[at..mark]);
                at = mark;
                if let Some(k) = starts.iter().position(|&s| s == mark) {
                    let (_, length, pitch) = notes[k];
                    synth.note_on(pitch, 0.9, (length * beat) as u32);
                }
                for &(k, _) in probes.iter().filter(|p| p.1 == mark) {
                    line += &format!(" {}:{:.1}", notes[k].2, synth.voices[0].note);
                }
            }
            println!("{name}:{line}");
            crate::keys::tests::write_wav(&format!("{dir}/{name}.wav"), &l, &r);
        }
    }

    /// Notes from a clip (timed gates) don't pile up in the held-notes stack.
    #[test]
    fn timed_notes_leave_the_mono_stack_alone() {
        let mut synth = WaveSynth::new(WaveParams { mono: true, ..saw_patch() }, SR);
        for n in 0..20 {
            synth.note_on(40 + n, 1.0, 100);
        }
        assert_eq!(synth.held_count, 0);
    }

    #[test]
    fn polyphony_plays_chords_and_steals_the_oldest_voice() {
        let (l, _) = play(saw_patch(), &[48, 52, 55], 4800);
        assert!(rms(&l) > 0.1);
        let mut synth = WaveSynth::new(saw_patch(), SR);
        for n in 0..20 {
            synth.note_on(40 + n, 1.0, HELD);
        }
        assert_eq!(synth.voices.iter().filter(|v| v.active).count(), VOICES);
        assert!(!synth.voices.iter().any(|v| v.gate.pitch == 40), "the first note was stolen");
    }

    #[test]
    fn every_warp_mode_is_stable_and_changes_the_sound() {
        let (plain, _) = play(saw_patch(), &[60], 9600);
        for warp in OscWarp::ALL {
            let mut p = saw_patch();
            p.osc[1].on = true;
            p.osc[0].warp = warp;
            p.osc[0].warp_amount = 0.7;
            let (out, _) = play(p, &[60], 9600);
            assert!(out.iter().all(|s| s.is_finite() && s.abs() < 4.0), "{warp:?}");
            if warp != OscWarp::Off {
                let difference: f32 = out.iter().zip(&plain).map(|(a, b)| (a - b).abs()).sum();
                assert!(difference > 1.0, "{warp:?} changes the wave");
            }
        }
    }

    #[test]
    fn timed_notes_release_on_their_own() {
        let mut synth = WaveSynth::new(saw_patch(), SR);
        synth.note_on(60, 1.0, 4800);
        let t = tables();
        let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
        synth.render(&[], &t, &mut l, &mut r);
        assert!(rms(&l[..4800]) > 0.05 && rms(&l[19_200..]) < 1e-4);
    }
}

#[cfg(test)]
mod phase2_tests {
    use super::*;
    use crate::model::{ArpParams, Device, Effect, InstrumentKind, UtilityParams};
    use crate::synth::HELD;
    use crate::wavetable::builtin;

    const SR: f32 = 48_000.0;

    fn tone(freq: f32, seconds: f32) -> Arc<Sample> {
        let data: Vec<f32> = (0..(seconds * SR) as usize).map(|i| (i as f32 * freq * TAU / SR).sin() * 0.5).collect();
        Arc::new(Sample::from_channels(data.clone(), data, SR))
    }

    fn render(synth: &mut WaveSynth, samples: &[Arc<Sample>], frames: usize) -> (Vec<f32>, Vec<f32>) {
        let tables = vec![Arc::new(builtin(0)), Arc::new(builtin(0))];
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            synth.render(samples, &tables, a, b);
        }
        (l, r)
    }

    fn frequency(signal: &[f32]) -> f32 {
        signal.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f32 * SR / signal.len() as f32
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn sample_patch(kind: OscKind) -> WaveParams {
        let mut p = WaveParams::default();
        p.osc[0].kind = kind;
        p.osc[0].random_phase = 0.0;
        p
    }

    #[test]
    fn the_sample_oscillator_plays_its_file_chromatically_from_c3() {
        let samples = [tone(440.0, 2.0)];
        for (note, expected) in [(60, 440.0), (72, 880.0), (48, 220.0)] {
            let mut synth = WaveSynth::new(sample_patch(OscKind::Sample), SR);
            synth.note_on(note, 1.0, HELD);
            let (l, _) = render(&mut synth, &samples, 24_000);
            assert!((frequency(&l[2400..]) - expected).abs() < 3.0, "note {note}: {}", frequency(&l[2400..]));
        }
    }

    #[test]
    fn a_sample_plays_once_or_loops() {
        let samples = [tone(440.0, 0.1)];
        let mut once = sample_patch(OscKind::Sample);
        once.osc[0].sample_loop = false;
        let mut synth = WaveSynth::new(once, SR);
        synth.note_on(60, 1.0, HELD);
        let (l, _) = render(&mut synth, &samples, 24_000);
        assert!(rms(&l[..4000]) > 0.05 && rms(&l[6000..]) < 1e-4, "stops after 0.1 s");
        let mut synth = WaveSynth::new(sample_patch(OscKind::Sample), SR);
        synth.note_on(60, 1.0, HELD);
        let (l, _) = render(&mut synth, &samples, 24_000);
        assert!(rms(&l[12_000..]) > 0.05, "loops");
    }

    #[test]
    fn granular_clouds_sound_at_the_note_pitch_and_spread() {
        let samples = [tone(440.0, 2.0)];
        let mut p = sample_patch(OscKind::Granular);
        p.osc[0].density = 40.0;
        p.osc[0].grain_size = 100.0;
        p.osc[0].width = 1.0;
        let mut synth = WaveSynth::new(p, SR);
        synth.note_on(60, 1.0, HELD);
        let (l, r) = render(&mut synth, &samples, 48_000);
        assert!(rms(&l[4800..]) > 0.03, "grains play: {}", rms(&l[4800..]));
        assert!((frequency(&l[4800..]) - 440.0).abs() < 40.0, "{}", frequency(&l[4800..]));
        let side = rms(&l.iter().zip(&r).map(|(a, b)| a - b).collect::<Vec<_>>());
        assert!(side > 0.005, "random pans make it wide");
        assert!(l.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn the_effects_rack_processes_the_synth() {
        let mut p = WaveParams::default();
        p.fx[0] = Some(Device::new(Effect::Utility(UtilityParams { gain_db: -12.0, ..Default::default() })));
        let mut dry = WaveSynth::new(WaveParams::default(), SR);
        let mut wet = WaveSynth::new(p, SR);
        dry.note_on(60, 1.0, HELD);
        wet.note_on(60, 1.0, HELD);
        let ratio = rms(&render(&mut wet, &[], 9600).0) / rms(&render(&mut dry, &[], 9600).0);
        assert!((ratio - 0.251).abs() < 0.02, "-12 dB: {ratio}");

        // Turning the effect off bypasses it without rebuilding.
        let mut off = p;
        off.fx[0] = off.fx[0].map(|d| Device { enabled: false, ..d });
        wet.set_params(off);
        let ratio = rms(&render(&mut wet, &[], 9600).0) / rms(&render(&mut dry, &[], 9600).0);
        assert!((ratio - 1.0).abs() < 0.05, "{ratio}");

        // Switching it cross-fades for 10 ms (no click): a fifth of the way
        // in, an effect 60 dB down still lets under a fifth of the sound through.
        let mut quiet = WaveParams::default();
        quiet.fx[0] = Some(Device::new(Effect::Utility(UtilityParams { gain_db: -60.0, ..Default::default() })));
        let (mut dry, mut wet) = (WaveSynth::new(WaveParams::default(), SR), WaveSynth::new(quiet, SR));
        dry.note_on(60, 1.0, HELD);
        wet.note_on(60, 1.0, HELD);
        render(&mut dry, &[], 9600);
        render(&mut wet, &[], 9600);
        quiet.fx[0] = quiet.fx[0].map(|d| Device { enabled: false, ..d });
        wet.set_params(quiet);
        let (plain, switched) = (render(&mut dry, &[], 9600).0, render(&mut wet, &[], 9600).0);
        let fade = (crate::effects::SWITCH_FADE * SR) as usize;
        assert!(plain[..fade / 5].iter().any(|s| s.abs() > 0.01));
        for (a, b) in switched[..fade / 5].iter().zip(&plain) {
            assert!(a.abs() <= b.abs() * 0.21 + 1e-6, "{a} {b}");
        }
        assert_eq!(switched[fade + 2..], plain[fade + 2..], "off: the sound as it came");

        // Off, it forgets its tail: a reverb turned back on in silence is silent.
        let mut hall = WaveParams::default();
        hall.fx[0] = Some(Device::new(Effect::Reverb(crate::model::ReverbParams { size: 1.0, mix: 1.0, ..Default::default() })));
        let mut synth = WaveSynth::new(hall, SR);
        synth.note_on(60, 1.0, HELD);
        render(&mut synth, &[], 9600);
        let mut hall_off = hall;
        hall_off.fx[0] = hall_off.fx[0].map(|d| Device { enabled: false, ..d });
        synth.set_params(hall_off);
        render(&mut synth, &[], 2400);
        synth.note_off(60);
        render(&mut synth, &[], SR as usize * 2);
        synth.set_params(hall);
        let back = render(&mut synth, &[], 9600).0;
        assert!(back.iter().all(|s| s.abs() < 1e-6), "no tail from before");

        let with_fx = InstrumentKind::Wavetable(p);
        assert!(InstrumentKind::Wavetable(WaveParams::default()).needs_rebuild(&with_fx), "adding an effect rebuilds");
        assert!(!with_fx.needs_rebuild(&InstrumentKind::Wavetable(off)), "toggling it does not");
    }

    #[test]
    fn the_arpeggiator_plays_held_notes_in_time() {
        let mut p = WaveParams::default();
        p.osc[0].random_phase = 0.0;
        p.arp = ArpParams { on: true, mode: ArpMode::Up, rate_beats: 0.5, octaves: 2 };
        p.arp_gate = 0.5;
        p.env[0].release = 0.005;
        let mut synth = WaveSynth::new(p, SR);
        synth.set_tempo(120.0);
        // Pressed out of order; Up sorts them.
        synth.note_on(64, 1.0, HELD);
        synth.note_on(57, 1.0, HELD);
        let (l, _) = render(&mut synth, &[], 96_000);
        // Steps every 0.25 s: 57, 64, 69, 76, then again.
        let step = 12_000;
        let expected = [57, 64, 69, 76, 57];
        for (k, &note) in expected.iter().enumerate() {
            let window = &l[k * step + 600..k * step + 5400];
            let f = 440.0 * 2f32.powf((note as f32 - 69.0) / 12.0);
            assert!((frequency(window) / f - 1.0).abs() < 0.05, "step {k}: {} Hz, wanted {f}", frequency(window));
            assert!(rms(&l[k * step + 7200..k * step + 11_500]) < 0.02, "gate closes half way through step {k}");
        }
        synth.note_off(64);
        synth.note_off(57);
        let (l, _) = render(&mut synth, &[], 48_000);
        assert!(rms(&l[24_000..]) < 1e-4, "nothing plays once the keys are up");
    }
}

#[cfg(test)]
mod polychrome_tests {
    use super::*;
    use crate::dsp::fft;
    use crate::model::{Combo, ComboOp, ModSlot, SeqStep, WaveFilterParams};
    use crate::synth::HELD;
    use crate::wavetable::builtin;

    const SR: f32 = 48_000.0;

    fn render(synth: &mut WaveSynth, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let tables = vec![Arc::new(builtin(0)), Arc::new(builtin(0))];
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            synth.render(&[], &tables, a, b);
        }
        (l, r)
    }

    fn frequency(signal: &[f32]) -> f32 {
        signal.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f32 * SR / signal.len() as f32
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// Magnitudes of harmonics 1..=n of `f0` in `signal` (8192 samples).
    fn harmonics(signal: &[f32], f0: f32, n: usize) -> Vec<f32> {
        let window = |i: usize| 0.5 - 0.5 * (TAU * i as f32 / 8192.0).cos();
        let (mut re, mut im) = (signal[..8192].iter().enumerate().map(|(i, v)| v * window(i)).collect::<Vec<_>>(), vec![0.0; 8192]);
        fft(&mut re, &mut im);
        (1..=n)
            .map(|h| {
                let bin = (h as f32 * f0 * 8192.0 / SR).round() as usize;
                (bin.saturating_sub(2)..=bin + 2).map(|k| re[k].hypot(im[k])).fold(0.0, f32::max)
            })
            .collect()
    }

    fn engine(kind: OscKind) -> WaveParams {
        let mut p = WaveParams::default();
        p.osc[0].kind = kind;
        p.osc[0].random_phase = 0.0;
        p
    }

    fn play(p: WaveParams, note: u8, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut synth = WaveSynth::new(p, SR);
        synth.note_on(note, 1.0, HELD);
        render(&mut synth, frames)
    }

    /// What of a held note at `f0` is not one of its harmonics, in dB under
    /// the whole: what folded back from over the top of the band.
    fn dirt(signal: &[f32], f0: f32) -> f32 {
        const N: usize = 32_768;
        let window = |i: usize| 0.5 - 0.5 * (TAU * i as f32 / N as f32).cos();
        let (mut re, mut im) = (signal[signal.len() - N..].iter().enumerate().map(|(i, v)| v * window(i)).collect::<Vec<_>>(), vec![0.0; N]);
        fft(&mut re, &mut im);
        let (mut clean, mut dirty) = (0f64, 0f64);
        for k in 8..N / 2 {
            let hz = k as f32 * SR / N as f32;
            let off = (hz / f0 - (hz / f0).round()).abs() * f0 / (SR / N as f32);
            let e = (re[k] * re[k] + im[k] * im[k]) as f64;
            if off <= 4.0 {
                clean += e
            } else {
                dirty += e
            }
        }
        (10.0 * (dirty / (clean + dirty)).log10()) as f32
    }

    /// A new sound's analog saw and pulse fold less back on high notes (their
    /// steps are corrected over four samples, not two); the factory sounds
    /// and a saved one that says nothing keep the two.
    #[test]
    fn a_new_sounds_analog_waves_stay_clean_up_high() {
        let analog = |position: f32, clean: bool| {
            let mut p = engine(OscKind::Analog);
            p.osc[0].position = position;
            p.osc[0].clean = clean;
            p
        };
        // How bright: what is over 8 kHz, against the whole.
        let highs = |x: &[f32]| {
            let (mut re, mut im) = (x[x.len() - 8192..].to_vec(), vec![0.0; 8192]);
            fft(&mut re, &mut im);
            let band = |from: usize, to: usize| (from..to).map(|k| re[k] * re[k] + im[k] * im[k]).sum::<f32>();
            10.0 * (band(8000 * 8192 / 48_000, 4096) / band(1, 4096)).log10()
        };
        for (name, position) in [("sierra", 2.0 / 3.0), ("pulso", 1.0)] {
            for note in [48u8, 60, 84, 96] {
                let f0 = 440.0 * 2f32.powf((note as f32 - 69.0) / 12.0);
                let (old, new) = (play(analog(position, false), note, 48_000).0, play(analog(position, true), note, 48_000).0);
                let (before, now) = (dirt(&old, f0), dirt(&new, f0));
                let (level, top) = (20.0 * (rms(&new) / rms(&old)).log10(), highs(&new) - highs(&old));
                println!("{name}, nota {note}: {before:.1} dB antes, {now:.1} ahora; nivel {level:+.2} dB; agudos {top:+.1} dB");
                assert!(now < before - 4.0 && now < -36.0, "{name}, note {note}: {now} dB ({before} before)");
                // As loud, and a little less on top (the price of the wider correction).
                assert!(level.abs() < 0.5 && (-2.5..0.0).contains(&top), "{name}, note {note}: {level} dB, {top} dB on top");
            }
        }
        // The sine and the triangle have no step: they play as they did.
        assert_eq!(play(analog(0.2, true), 72, 4_800).0, play(analog(0.2, false), 72, 4_800).0);
    }

    /// A new sound's Drive folds less back on high notes: each sample is the
    /// curve's mean since the last one. The factory sounds and a saved one
    /// that says nothing keep the curve at each sample.
    #[test]
    fn a_new_sounds_drive_stays_clean_up_high() {
        let driven = |mode: WaveFilter, drive: f32, clean: bool| {
            let mut p = engine(OscKind::Analog);
            p.osc[0].position = 2.0 / 3.0;
            p.filter.on = true;
            p.filter.mode = mode;
            p.filter.cutoff = 18_000.0;
            p.filter.resonance = 0.1;
            p.filter.drive = drive;
            p.filter.clean_drive = clean;
            p
        };
        // How bright: what is over 8 kHz, against the whole.
        let highs = |x: &[f32]| {
            let (mut re, mut im) = (x[x.len() - 8192..].to_vec(), vec![0.0; 8192]);
            fft(&mut re, &mut im);
            let band = |from: usize, to: usize| (from..to).map(|k| re[k] * re[k] + im[k] * im[k]).sum::<f32>();
            10.0 * (band(8000 * 8192 / 48_000, 4096) / band(1, 4096)).log10()
        };
        for (name, mode) in [("LP12", WaveFilter::Lp12), ("Ladder", WaveFilter::Ladder)] {
            for drive in [0.25f32, 0.5, 1.0] {
                for note in [48u8, 60, 72, 84, 96] {
                    let f0 = 440.0 * 2f32.powf((note as f32 - 69.0) / 12.0);
                    let (old, new) = (play(driven(mode, drive, false), note, 48_000).0, play(driven(mode, drive, true), note, 48_000).0);
                    let (before, now) = (dirt(&old, f0), dirt(&new, f0));
                    let (level, top) = (20.0 * (rms(&new) / rms(&old)).log10(), highs(&new) - highs(&old));
                    println!("{name}, drive {drive}, nota {note}: {before:.1} dB antes, {now:.1} ahora; nivel {level:+.2} dB; agudos {top:+.1} dB");
                    assert!(now < before - 6.0 && now < -35.0, "{name}, drive {drive}, note {note}: {now} dB ({before} before)");
                    // As loud, and a little less on top (the price of the mean).
                    assert!(level.abs() < 0.3 && (-2.0..0.0).contains(&top), "{name}, drive {drive}, note {note}: {level} dB, {top} dB on top");
                }
            }
        }
        // With no Drive there is nothing to clean: it plays as it did.
        assert_eq!(play(driven(WaveFilter::Lp12, 0.0, true), 72, 4_800).0, play(driven(WaveFilter::Lp12, 0.0, false), 72, 4_800).0);
        // Who has it.
        assert!(WaveParams::default().filter.clean_drive && WaveParams::polychrome().filter.clean_drive && WaveParams::polychrome().filter2.clean_drive);
        for (name, p) in WaveParams::presets().into_iter().chain(WaveParams::polychrome_presets()) {
            assert_eq!(p.filter.clean_drive, name == "Init" || WaveParams::MADE_SINCE.contains(&name), "{name}");
            assert_eq!(p.filter2.clean_drive, name == "Init" || WaveParams::MADE_SINCE.contains(&name), "{name}");
        }
        let mut old = serde_json::to_value(WaveParams::polychrome()).unwrap();
        old["filter"].as_object_mut().unwrap().remove("clean_drive");
        let old: WaveParams = serde_json::from_value(old).unwrap();
        assert!(!old.filter.clean_drive);
        let again: WaveParams = serde_json::from_str(&serde_json::to_string(&WaveParams::polychrome()).unwrap()).unwrap();
        assert!(again.filter.clean_drive);
    }

    /// A new sound's warps stay clean on high notes: read several times a
    /// sample, from a table with the harmonics the warped wave has room for.
    /// The factory sounds and a saved one that says nothing keep one read
    /// a sample.
    #[test]
    fn a_new_sounds_warps_stay_clean_up_high() {
        let warped = |warp: OscWarp, amount: f32, clean: bool| {
            let mut p = engine(OscKind::Wavetable);
            p.osc[0].position = 2.0 / 3.0;
            p.osc[0].warp = warp;
            p.osc[0].warp_amount = amount;
            p.osc[0].clean = clean;
            p
        };
        for (name, warp, amount) in [("Sync 0,3", OscWarp::Sync, 0.3), ("Bend+ 0,7", OscWarp::BendPlus, 0.7), ("Bend- 0,7", OscWarp::BendMinus, 0.7), ("PWM 0,7", OscWarp::Pwm, 0.7), ("Mirror 1", OscWarp::Mirror, 1.0)] {
            for note in [60u8, 84, 96] {
                let f0 = 440.0 * 2f32.powf((note as f32 - 69.0) / 12.0);
                let (before, now) = (dirt(&play(warped(warp, amount, false), note, 48_000).0, f0), dirt(&play(warped(warp, amount, true), note, 48_000).0, f0));
                let (loud_before, loud) = (rms(&play(warped(warp, amount, false), note, 48_000).0), rms(&play(warped(warp, amount, true), note, 48_000).0));
                println!("{name}, nota {note}: {before:.1} dB antes, {now:.1} ahora; nivel {:+.1} dB", 20.0 * (loud / loud_before).log10());
                assert!(now < -28.0 && now < before + 1.0, "{name}, note {note}: {now} dB ({before} before)");
            }
        }
        // Without a warp, or with none of it, it plays as it did.
        let plain = |clean: bool| {
            let mut p = warped(OscWarp::Sync, 0.0, clean);
            p.osc[0].unison = 3.0;
            play(p, 72, 4_800).0
        };
        assert_eq!(plain(true), plain(false));
        // Who has it.
        assert!(crate::model::WaveOsc::default().clean && WaveParams::default().osc[0].clean && WaveParams::polychrome().osc[1].clean);
        for (name, p) in WaveParams::presets().into_iter().chain(WaveParams::polychrome_presets()) {
            let new = name == "Init" || WaveParams::MADE_SINCE.contains(&name);
            assert_eq!((p.osc[0].clean, p.osc[1].clean), (new, new), "{name}");
        }
        let mut old = serde_json::to_value(WaveParams::default()).unwrap();
        old["osc"][0].as_object_mut().unwrap().remove("clean");
        let old: WaveParams = serde_json::from_value(old).unwrap();
        assert!(!old.osc[0].clean && old.osc[1].clean);
    }

    #[test]
    fn the_analog_engine_morphs_its_wave_and_pulse_width() {
        let mut p = engine(OscKind::Analog);
        p.osc[0].position = 2.0 / 3.0;
        let (saw, _) = play(p, 57, 16_384);
        assert!((frequency(&saw[4800..]) - 220.0).abs() < 2.0);
        let h = harmonics(&saw[4800..], 220.0, 4);
        assert!((h[1] / h[0] - 0.5).abs() < 0.08, "a saw's 2nd harmonic is half: {:?}", h);

        p.osc[0].position = 1.0;
        let (square, _) = play(p, 57, 16_384);
        let h = harmonics(&square[4800..], 220.0, 3);
        assert!(h[1] / h[0] < 0.05, "a 50 % pulse has no even harmonics: {:?}", h);
        p.osc[0].warp_amount = 0.8;
        let (narrow, _) = play(p, 57, 16_384);
        let h = harmonics(&narrow[4800..], 220.0, 3);
        assert!(h[1] / h[0] > 0.3, "a narrow pulse does: {:?}", h);
    }

    #[test]
    fn analog_drift_wanders_a_little() {
        let mut p = engine(OscKind::Analog);
        p.osc[0].drift = 1.0;
        let (l, _) = play(p, 69, 96_000);
        let f = frequency(&l[48_000..]);
        assert!((f - 440.0).abs() < 440.0 * 0.01, "within 15 cents: {f}");
        assert!(l.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn the_harmonic_engine_shapes_its_partials() {
        let mut p = engine(OscKind::Harmonic);
        p.osc[0].partials = 1.0;
        let (sine, _) = play(p, 57, 16_384);
        let h = harmonics(&sine[4800..], 220.0, 4);
        assert!(h[1] < h[0] * 0.01 && h[2] < h[0] * 0.01, "one partial is a sine: {:?}", h);

        p.osc[0].partials = 16.0;
        p.osc[0].parity = -1.0;
        let (odd, _) = play(p, 57, 16_384);
        let h = harmonics(&odd[4800..], 220.0, 4);
        assert!(h[1] < h[0] * 0.02 && h[2] > h[0] * 0.2, "odd only: {:?}", h);

        p.osc[0].parity = 0.0;
        p.osc[0].tilt = -1.0;
        let (dark, _) = play(p, 57, 16_384);
        p.osc[0].tilt = 1.0;
        let (bright, _) = play(p, 57, 16_384);
        let ratio = |x: &[f32]| { let h = harmonics(&x[4800..], 220.0, 8); h[7] / h[0] };
        assert!(ratio(&bright) > ratio(&dark) * 10.0, "tilt brightens");

        // A high note keeps only the partials below Nyquist.
        let (high, _) = play(p, 105, 16_384);
        assert!(high.iter().all(|s| s.is_finite()) && rms(&high) > 0.05);
    }

    #[test]
    fn two_filters_in_series_or_parallel() {
        let mut p = engine(OscKind::Analog);
        p.osc[0].position = 2.0 / 3.0;
        p.filter = WaveFilterParams { on: true, mode: WaveFilter::Lp24, cutoff: 300.0, ..p.filter };
        p.filter2 = WaveFilterParams { on: true, mode: WaveFilter::Hp24, cutoff: 3000.0, ..p.filter2 };
        let (series, _) = play(p, 45, 24_000);
        assert!(rms(&series[4800..]) < 0.01, "low-pass then high-pass leaves almost nothing");
        p.filter_parallel = true;
        p.filter_balance = 0.0;
        let (only_low, _) = play(p, 45, 24_000);
        p.filter_balance = 1.0;
        let (only_high, _) = play(p, 45, 24_000);
        assert!(rms(&only_low[4800..]) > 0.05 && rms(&only_high[4800..]) > 0.005);
        let highs = |x: &[f32]| rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>());
        assert!(highs(&only_high[4800..]) / rms(&only_high[4800..]) > highs(&only_low[4800..]) / rms(&only_low[4800..]) * 3.0);
    }

    #[test]
    fn random_sources_and_combinations_modulate() {
        let mut p = engine(OscKind::Analog);
        p.random_rate = 10.0;
        p.matrix[0] = ModSlot { source: ModSource::SampleHold, dest: 4, amount: -1.0, bipolar: false };
        let (l, _) = play(p, 60, 48_000);
        let levels: Vec<f32> = l.chunks(4800).map(rms).collect();
        let spread = levels.iter().fold(0f32, |m, v| m.max(*v)) - levels.iter().fold(1f32, |m, v| m.min(*v));
        assert!(spread > 0.05, "S&H changes the level every 0.1 s: {levels:?}");

        // Velocity × macro 1 at 0: the combination is 0 and the level stays.
        let mut p = engine(OscKind::Analog);
        p.combos[0] = Combo { a: ModSource::Velocity, b: ModSource::Macro1, op: ComboOp::Multiply };
        p.matrix[0] = ModSlot { source: ModSource::Combo1, dest: 4, amount: -1.0, bipolar: false };
        let quiet_macro = rms(&play(p, 60, 9600).0[2400..]);
        p.macros[0] = 1.0;
        let full_macro = rms(&play(p, 60, 9600).0[2400..]);
        assert!(quiet_macro > 0.1 && full_macro < 0.01, "{quiet_macro} {full_macro}");
    }

    #[test]
    fn the_sequencer_plays_its_steps_on_the_held_key() {
        let mut p = engine(OscKind::Analog);
        p.env[0].release = 0.005;
        p.seq.on = true;
        p.seq.length = 4;
        p.seq.rate_beats = 0.5;
        p.seq.steps[0] = SeqStep { pitch: 0.0, ..SeqStep::default() };
        p.seq.steps[1] = SeqStep { pitch: 12.0, ..SeqStep::default() };
        p.seq.steps[2] = SeqStep { on: false, ..SeqStep::default() };
        p.seq.steps[3] = SeqStep { pitch: 7.0, probability: 0.0, ..SeqStep::default() };
        let mut synth = WaveSynth::new(p, SR);
        synth.set_tempo(120.0);
        synth.note_on(57, 1.0, HELD);
        let (l, _) = render(&mut synth, 96_000);
        let step = 12_000;
        let window = |k: usize| &l[k * step + 600..k * step + 5400];
        assert!((frequency(window(0)) - 220.0).abs() < 4.0);
        assert!((frequency(window(1)) - 440.0).abs() < 6.0, "{}", frequency(window(1)));
        assert!(rms(window(2)) < 1e-3, "a rest");
        assert!(rms(window(3)) < 1e-3, "probability 0 never plays");
        assert!((frequency(window(4)) - 220.0).abs() < 4.0, "it loops");
        synth.note_off(57);
        let (l, _) = render(&mut synth, 24_000);
        assert!(rms(&l[12_000..]) < 1e-4, "stops with the key");
    }

    #[test]
    fn sequencer_slides_glide_and_its_lane_modulates() {
        let mut p = engine(OscKind::Analog);
        p.glide = 80.0;
        p.seq.on = true;
        p.seq.length = 2;
        p.seq.rate_beats = 1.0;
        p.seq.steps[0] = SeqStep { slide: true, value: 0.0, ..SeqStep::default() };
        p.seq.steps[1] = SeqStep { pitch: 12.0, value: 1.0, ..SeqStep::default() };
        p.matrix[0] = ModSlot { source: ModSource::Sequencer, dest: 4, amount: -1.0, bipolar: false };
        let mut synth = WaveSynth::new(p, SR);
        synth.set_tempo(120.0);
        synth.note_on(57, 1.0, HELD);
        let (l, _) = render(&mut synth, 48_000);
        assert_eq!(synth.voices.iter().filter(|v| v.active).count(), 1, "the slide reused the voice");
        assert!(rms(&l[2400..20_000]) > 0.05, "step 1: lane 0, full level");
        assert!(rms(&l[30_000..44_000]) < 0.01, "step 2: lane 1 turns the level down");
    }

    fn factory(name: &str) -> WaveParams {
        WaveParams::polychrome_presets().into_iter().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("no preset {name}")).1
    }

    fn play_at_120(p: WaveParams, note: u8, frames: usize) -> Vec<f32> {
        let mut synth = WaveSynth::new(p, SR);
        synth.set_tempo(120.0);
        synth.note_on(note, 1.0, HELD);
        render(&mut synth, frames).0
    }

    #[test]
    fn the_plucked_string_rings_at_the_played_note_and_dies_away() {
        let p = factory("Doble filtro · Cuerda pulsada");
        for (note, hz) in [(57, 220.0), (69, 440.0)] {
            let l = play_at_120(p, note, 48_000);
            let near = harmonics(&l[1200..], hz, 1)[0];
            let off = harmonics(&l[1200..], hz * 1.12, 1)[0];
            assert!(near > 4.0 * off, "note {note}: {near} at the note, {off} beside it");
            assert!(rms(&l[0..4800]) > 3.0 * rms(&l[38_400..43_200]), "note {note} decays");
        }
    }

    #[test]
    fn the_noise_riser_climbs_over_eight_beats() {
        let l = play_at_120(factory("FX · Subida de ruido"), 60, 180_000);
        let (early, late) = (frequency(&l[14_400..24_000]), frequency(&l[163_200..172_800]));
        assert!(late > 3.0 * early, "brighter at the end: {early} → {late}");
    }

    #[test]
    fn the_crossed_filters_move_from_low_to_high_pass() {
        let l = play_at_120(factory("Doble filtro · LP/HP cruzado"), 60, 110_000);
        let (low, high) = (frequency(&l[19_200..28_800]), frequency(&l[91_200..100_800]));
        assert!(high > 2.0 * low, "the high-pass side is brighter: {low} → {high}");
    }

    #[test]
    fn the_rolling_bass_rests_on_the_beat() {
        let l = play_at_120(factory("Seq · Bajo rodante"), 45, 24_000);
        let step = |k: usize| &l[k * 6000 + 600..k * 6000 + 5400];
        assert!(rms(step(0)) < 1e-3 && rms(step(2)) > 0.02 && rms(step(3)) > 0.02, "{} {} {}", rms(step(0)), rms(step(2)), rms(step(3)));
    }
}

#[cfg(test)]
mod preset_tests {
    use super::*;
    use crate::synth::HELD;
    use crate::wavetable::builtin;

    #[test]
    fn every_factory_preset_sounds_and_stays_in_range() {
        for (name, p) in WaveParams::presets() {
            let tables = vec![Arc::new(builtin(p.osc[0].table as usize)), Arc::new(builtin(p.osc[1].table as usize))];
            let mut synth = WaveSynth::new(p, 48_000.0);
            for n in [48, 55, 60] {
                synth.note_on(if p.mono { 36 } else { n }, 1.0, HELD);
            }
            let (mut l, mut r) = (vec![0.0; 96_000], vec![0.0; 96_000]);
            for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
                synth.render(&[], &tables, a, b);
            }
            let peak = l.iter().chain(&r).fold(0f32, |m, s| m.max(s.abs()));
            let rms = (l.iter().map(|s| s * s).sum::<f32>() / l.len() as f32).sqrt();
            assert!(l.iter().chain(&r).all(|s| s.is_finite()), "{name}");
            assert!(rms > 0.01 && peak < 2.0, "{name}: rms {rms}, peak {peak}");
        }
    }

    fn factory(name: &str) -> WaveParams {
        WaveParams::presets().into_iter().find(|(n, _)| *n == name).map(|(_, p)| p).unwrap_or_else(|| panic!("no preset {name}"))
    }

    /// Left channel of `p` holding `note` for `frames` samples, at 120 BPM.
    fn hold(p: WaveParams, note: u8, frames: usize) -> Vec<f32> {
        let tables = vec![Arc::new(builtin(p.osc[0].table as usize)), Arc::new(builtin(p.osc[1].table as usize))];
        let mut synth = WaveSynth::new(p, 48_000.0);
        synth.note_on(note, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            synth.render(&[], &tables, a, b);
        }
        l
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// Fundamental by autocorrelation, between 50 Hz and 2 kHz.
    fn pitch(x: &[f32]) -> f32 {
        let lag = (24..960).max_by(|&a, &b| {
            let corr = |lag: usize| x.iter().zip(&x[lag..]).map(|(a, b)| a * b).sum::<f32>();
            corr(a).total_cmp(&corr(b))
        });
        48_000.0 / lag.unwrap() as f32
    }

    /// Brightness: the level of the signal's slope against its own level.
    fn brightness(x: &[f32]) -> f32 {
        rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>()) / rms(x).max(1e-9)
    }

    #[test]
    fn the_plucked_string_rings_at_the_played_note() {
        for note in [48u8, 55, 60] {
            let l = hold(factory("Pluck · Cuerda pulsada"), note, 12_000);
            let (found, expected) = (pitch(&l[2_400..8_400]), note_freq(note as f32));
            assert!((found / expected - 1.0).abs() < 0.03, "{note}: {found} Hz, expected {expected}");
        }
    }

    #[test]
    fn macro_one_sets_the_depth_of_the_triplet_wobble() {
        // Spread of the brightness over 20 ms windows, across a second.
        let movement = |macro_one: f32| {
            let mut p = factory("Bajo · Wobble tresillo");
            p.macros[0] = macro_one;
            let l = hold(p, 36, 48_000);
            let levels: Vec<f32> = l[4_800..].chunks(960).map(brightness).collect();
            let (low, high) = levels.iter().fold((f32::MAX, 0f32), |(lo, hi), &v| (lo.min(v), hi.max(v)));
            high / low.max(1e-6)
        };
        let (still, wobbling) = (movement(0.0), movement(0.85));
        assert!(wobbling > still * 2.0, "macro at 0: {still}, at 0.85: {wobbling}");
    }

    #[test]
    fn the_downlifter_falls_over_two_bars() {
        let l = hold(factory("FX · Caída de ruido"), 60, 48_000 * 4);
        let (start, end) = (brightness(&l[4_800..28_800]), brightness(&l[144_000..168_000]));
        assert!(start > end * 2.0, "brightness {start} at the start, {end} at the end");
    }

    #[test]
    fn the_evolving_pad_keeps_changing_its_tone() {
        let l = hold(factory("Pad · Evolutivo"), 60, 48_000 * 8);
        let tones: Vec<f32> = l[48_000..].chunks(48_000).map(brightness).collect();
        let (low, high) = tones.iter().fold((f32::MAX, 0f32), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        assert!(high > low * 1.2, "brightness per second: {tones:?}");
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    use crate::synth::HELD;

    #[test]
    #[ignore = "benchmark: prints how much of real time a worst-case patch takes"]
    fn worst_case_cost() {
        let mut p = WaveParams::default();
        for osc in &mut p.osc {
            osc.on = true;
            osc.unison = 16.0;
        }
        p.filter.on = true;
        p.filter.mode = crate::model::WaveFilter::Ladder;
        let mut synth = WaveSynth::new(p, 48_000.0);
        for n in 0..16 {
            synth.note_on(40 + n * 2, 1.0, HELD);
        }
        let tables = vec![Arc::new(crate::wavetable::builtin(0)), Arc::new(crate::wavetable::builtin(3))];
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        let start = std::time::Instant::now();
        for _ in 0..(48_000 / 512) {
            synth.render(&[], &tables, &mut l, &mut r);
        }
        let seconds = start.elapsed().as_secs_f64();
        println!("1 s de audio en {:.1} ms ({:.0} % de tiempo real)", seconds * 1000.0, seconds * 100.0);
    }
}
