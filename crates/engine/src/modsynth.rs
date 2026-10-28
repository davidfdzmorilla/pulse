//! Modular synthesizer built as a stack of modules, read top to bottom inside
//! groups (see `ModularParams`):
//!
//! - a Group starts a new signal; nothing flows past a group header;
//! - generators (Analog, Wavetable, Sample, Granular, Noise) add their sound to
//!   the signal from above;
//! - effects (Distortion, Filter, Nonlinear Filter) process the signal so far;
//! - Mix passes the signal on scaled (and exposes it as a modulation source);
//! - Aux injects whatever audio-rate connections route into it;
//! - Output and Curve Output modules shape the signal with an envelope or a
//!   drawn curve and send it to an effect lane or the master (or nowhere, as a
//!   pure modulation source).
//!
//! Any module's output can modulate another module's phase, harmonic, shift,
//! level or pitch at audio rate; sources below their target are read one
//! sample late. Up to 32 modulators run at control rate (every 32 samples):
//! per voice, or once for all voices for those that listen to the lanes, the
//! master or MIDI controllers. Notes, or another modulator's rises, trigger
//! them; Remap, the limits, Scale and Sample & Hold process another modulator,
//! and any modulator can modulate another's settings, including its depth.
//! The global unison plays the whole stack once per unison voice,
//! each voice detuned, stacked or tuned to a chord. Lane effects are allocated
//! when the synth is built, on the UI thread; rendering never allocates.

use crate::effects::{EffectProcessor, ProcessContext};
use crate::model::Effect;
use crate::instruments::{saw, Gate};
use crate::engine::Controller;
use crate::model::{
    chord_note, chord_size, module_slot, modulator_slot, mslot, slot, AudioParam, CurveInput, LfoMode, ModularParams, Modulator, ModuleKind, ModulatorKind,
    TriggerMode, UnisonMode, WaveEnv, GRAIN_SYNC, MODULAR_PARAMS, MOD_AUDIO, MOD_AUDIO_AMOUNT, MOD_GLIDE, MOD_LANES, MOD_LANE_GAIN, MOD_MACRO_PARAMS,
    MOD_MASTER, MOD_MODULATORS, MOD_MODULES, MOD_NO_LANE, MOD_NO_MODULATOR, MOD_NO_MODULE, MOD_UNISON, MOD_UNISON_VOICES, SCALE_MULTIPLIERS,
    MOD_FINE, MOD_LANE_FX_PARAMS, MOD_LANE_MIX, MOD_PITCH, MOD_SEND_SIDEBAND,
};
use crate::sample::Sample;
use crate::wavesynth::{note_freq, Env, Stage, SAMPLE_ROOT_HZ};
use crate::wavetable::Wavetable;
use std::f32::consts::{FRAC_PI_4, PI, SQRT_2, TAU};
use std::sync::Arc;

/// Voices allocated; each note takes one per global unison voice.
const VOICES: usize = 32;
/// Oscillator unison inside a generator.
const UNISON: usize = 8;
/// Grains per voice, shared by its granular modules.
const GRAINS: usize = 32;
const NO_GRAIN: u8 = u8::MAX;
const CONTROL: usize = 32;
const PARAMS: usize = MODULAR_PARAMS.len();
/// How long a looping curve output takes to fade out after the note ends.
const CURVE_FADE: f32 = 0.02;
/// Parameters that lanes read from the newest voice: their effects' settings and mixes.
const LANE_VALUES: usize = MOD_PITCH - MOD_LANE_FX_PARAMS;
/// A poly lane's tail ends once it stays below this for longer than its longest delay
/// (plus `TAIL_HOLD` seconds), or after `TAIL_MAX` seconds.
const TAIL_SILENCE: f32 = 1e-5;
const TAIL_HOLD: f32 = 0.05;
const TAIL_MAX: f32 = 30.0;
/// What followers and trackers can listen to: the three lanes, the master and the sideband.
const LISTEN: usize = MOD_LANES + 2;
/// `Block::lane` of an output sent to the sideband bus.
const SIDEBAND: usize = usize::MAX - 1;
/// Pitch tracking: the input is decimated by this much into a ring this long, and
/// analysed over a window every `TRACK_EVERY` blocks.
const TRACK_DECIMATE: usize = 4;
const TRACK_RING: usize = 1024;
const TRACK_WINDOW: usize = 384;
const TRACK_EVERY: u32 = 8;

/// What a modulator envelope's 0..1 settings mean in seconds.
fn envelope(a: f32, b: f32, c: f32, d: f32) -> WaveEnv {
    WaveEnv { attack: 0.0005 * 20_000f32.powf(a), hold: 0.0, decay: 0.001 * 10_000f32.powf(b), sustain: c, release: 0.001 * 10_000f32.powf(d) }
}

/// `x` wrapped into 0..1, for `x` in -1..2 (cheaper than `rem_euclid`).
#[inline]
fn wrap(x: f32) -> f32 {
    if x >= 1.0 {
        x - 1.0
    } else if x < 0.0 {
        x + 1.0
    } else {
        x
    }
}

/// Band-limited analog waves: 0 saw, 1 pulse, 2 triangle, 3 sine.
#[inline]
fn analog(wave: u8, ph: f32, inc: f32, width: f32) -> f32 {
    match wave {
        0 => saw(wrap(ph + 0.5), inc),
        1 => (saw(ph, inc) - saw(wrap(ph + 1.0 - width), inc)) * 0.5 / (1.0 - width.min(1.0 - width)),
        2 => 1.0 - 4.0 * (wrap(ph + 0.25) - 0.5).abs(),
        _ => (ph * TAU).sin(),
    }
}

fn shape(kind: u8, x: f32, steps: f32) -> f32 {
    match kind {
        // Overdrive, saturate, foldback, sine, hard clip, quantize.
        0 => x.tanh(),
        1 => x.atan() * (2.0 / PI),
        2 => {
            let folded = (x + 1.0).rem_euclid(4.0);
            if folded < 2.0 { folded - 1.0 } else { 3.0 - folded }
        }
        3 => x.sin(),
        4 => x.clamp(-1.0, 1.0),
        _ => (x.clamp(-1.0, 1.0) * steps).round() / steps,
    }
}

/// The nonlinear filter's saturation flavours: clean, warm, tape, diode, fuzz.
#[inline]
fn saturate(mode: u8, x: f32) -> f32 {
    match mode {
        0 => x,
        1 => x.tanh(),
        2 => x / (1.0 + x.abs()),
        // Asymmetric: even harmonics.
        3 => {
            if x > 0.0 {
                (x * 1.5).tanh() / 1.5
            } else {
                (x * 0.6).tanh() / 0.6
            }
        }
        _ => x.signum() * (1.0 - (-2.5 * x.abs()).exp()) * 0.8,
    }
}

/// Normalised biquad coefficients (RBJ cookbook).
#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    fn new(kind: u8, cutoff: f32, q: f32, gain_db: f32, sr: f32) -> Self {
        let w0 = TAU * cutoff.clamp(10.0, sr * 0.45) / sr;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q.max(0.05));
        let a = 10f32.powf(gain_db / 40.0);
        let sq = 2.0 * a.sqrt() * alpha;
        let (b0, b1, b2, a0, a1, a2) = match kind {
            0 => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            1 => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            2 => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            3 => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            4 => (
                a * ((a + 1.0) - (a - 1.0) * cos + sq),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - sq),
                (a + 1.0) + (a - 1.0) * cos + sq,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - sq,
            ),
            5 => (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a),
            _ => (
                a * ((a + 1.0) + (a - 1.0) * cos + sq),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - sq),
                (a + 1.0) - (a - 1.0) * cos + sq,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - sq,
            ),
        };
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    /// Transposed direct form II.
    #[inline]
    fn tick(&self, z: &mut [f32; 2], x: f32) -> f32 {
        let y = self.b0 * x + z[0];
        z[0] = self.b1 * x - self.a1 * y + z[1];
        z[1] = self.b2 * x - self.a2 * y;
        y
    }
}

/// Per-voice state of one module.
#[derive(Clone, Copy)]
struct ModuleState {
    /// Oscillator phase (slave, under sync) and sync master phase, per unison voice.
    phases: [f32; UNISON],
    masters: [f32; UNISON],
    /// Sample play heads, and whether each is going backwards (ping pong, reverse).
    positions: [f64; UNISON],
    back: [bool; UNISON],
    seed: u32,
    /// Noise colouring: pink filter, brown integrator, previous white and pink, per channel.
    pink: [[f32; 7]; 2],
    brown: [f32; 2],
    previous: [[f32; 2]; 2],
    /// Keytracked noise: phase through the note's cycle, and the old and new value per channel.
    keyed_phase: f32,
    keyed: [[f32; 2]; 2],
    /// Filter memory: [channel][stage].
    biquad: [[[f32; 2]; 2]; 2],
    /// Nonlinear filter integrators, per channel.
    svf: [[f32; 2]; 2],
    env: Env,
    /// Granular: progress to the next grain (spawns at 1), chord step, warm start pending.
    spawn: f32,
    pick: u32,
    warm: bool,
    /// Curve output: position 0..1, direction, release fade, finished.
    curve: f32,
    curve_back: bool,
    fade: f32,
    done: bool,
}

impl ModuleState {
    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32
    }
}

#[derive(Clone, Copy)]
struct Grain {
    /// The granular module playing it, or [`NO_GRAIN`].
    module: u8,
    position: f64,
    rate: f64,
    age: f32,
    length: f32,
    gains: (f32, f32),
}

impl Grain {
    const FREE: Grain = Grain { module: NO_GRAIN, position: 0.0, rate: 0.0, age: 0.0, length: 1.0, gains: (0.0, 0.0) };
}

/// The state of one modulator (per voice, or shared by all voices).
#[derive(Clone, Copy)]
struct ModState {
    env: Env,
    /// Seconds an envelope still waits before its attack; below zero when not waiting.
    delay: f32,
    /// LFO cycles, curve position (0..1) or random step progress.
    phase: f32,
    /// Curve: going backwards (ping pong).
    back: bool,
    /// Held by its note or trigger input.
    held: bool,
    /// The trigger input was above its threshold last block.
    gate: bool,
    /// Random: the step being left and the one being reached, and the step's length factor.
    from: f32,
    to: f32,
    step: f32,
    /// Sample & Hold: the held value, and whether to take one this block.
    sample: f32,
    pending: bool,
    /// The latest output (read by modulators listed before it, one block late).
    out: f32,
}

impl ModState {
    const IDLE: ModState = ModState {
        env: Env::IDLE,
        delay: -1.0,
        phase: 0.0,
        back: false,
        held: false,
        gate: false,
        from: 0.5,
        to: 0.5,
        step: 1.0,
        sample: 0.0,
        pending: false,
        out: 0.0,
    };

    /// Restarts it from the beginning; `random` is a fresh 0..1 value.
    fn trigger(&mut self, modulator: &Modulator, delay: f32, random: f32) {
        self.held = true;
        match modulator.kind {
            ModulatorKind::Envelope => {
                if !modulator.seamless {
                    self.env = Env::IDLE;
                }
                if delay > 0.0 {
                    self.delay = delay;
                } else {
                    self.delay = -1.0;
                    self.env.trigger();
                }
            }
            ModulatorKind::Lfo | ModulatorKind::LfoTable | ModulatorKind::Timeline => {
                self.phase = 0.0;
                self.back = false;
            }
            ModulatorKind::Random => {
                self.phase = 0.0;
                self.from = random;
                self.to = random;
            }
            ModulatorKind::SampleHold => self.pending = true,
            _ => {}
        }
    }

    fn release(&mut self) {
        self.held = false;
        self.delay = -1.0;
        self.env.release();
    }
}

/// Slot `k` of modulator `i` in real units, from the (modulated) parameters `m`.
fn modulator_value(modulator: &Modulator, m: &[f32; PARAMS], i: usize, k: usize) -> f32 {
    modulator_slot(modulator.kind, k).map_or(0.0, |info| info.denormalize(m[ModularParams::modulator_param(i, k)]))
}

/// One step of a random modulator: walks to a new value at `rate`, with `jitter` on
/// the step lengths, `chaos` limiting how far each step goes and `smooth` gliding.
fn random_tick(state: &mut ModState, rate: f32, smooth: f32, jitter: f32, chaos: f32, dt: f32, mut random: impl FnMut() -> f32) -> f32 {
    state.phase += rate * dt / state.step;
    if state.phase >= 1.0 {
        state.phase = (state.phase - 1.0).min(0.999);
        // Each step starts where the last one arrived.
        state.from = state.to;
        state.to += chaos * (random() - state.to);
        state.step = 1.0 + jitter * (random() * 2.0 - 1.0) * 0.9;
    }
    random_value(state, smooth)
}

fn random_value(state: &ModState, smooth: f32) -> f32 {
    let k = if smooth > 0.001 { (state.phase / smooth).min(1.0) } else { 1.0 };
    state.from + (state.to - state.from) * k * k * (3.0 - 2.0 * k)
}

/// A drawn curve played over time: `speed` is its share per second, `mode` an index into `CURVE_LOOPS`.
fn timeline_tick(state: &mut ModState, mode: u8, (start, end): (f32, f32), speed: f32, dt: f32) -> f32 {
    let (lo, hi) = (start.min(end), start.max(end));
    let looping = hi - lo > 0.001 && match mode {
        1 | 3 => true,
        2 => state.held,
        _ => false,
    };
    let step = speed * dt;
    if state.back {
        state.phase -= step;
        if state.phase <= lo {
            state.phase = lo + (lo - state.phase).min(hi - lo);
            state.back = false;
        }
    } else {
        state.phase += step;
        if looping && state.phase >= hi {
            if mode == 3 {
                state.phase = hi - (state.phase - hi).min(hi - lo);
                state.back = true;
            } else {
                state.phase = lo + (state.phase - hi) % (hi - lo);
            }
        }
    }
    state.phase = state.phase.clamp(0.0, 1.0);
    state.phase
}

/// The global sources modulators read, for one block.
struct ModInputs<'a> {
    tempo: f32,
    clock: f64,
    legato: bool,
    globals: &'a [f32; MOD_MODULATORS],
    tables: &'a [Arc<Wavetable>],
    bend: f32,
    pressure: f32,
}

#[derive(Clone)]
struct Voice {
    active: bool,
    gate: Gate,
    note: f32,
    target: f32,
    velocity: f32,
    random: f32,
    age: f32,
    /// Which global unison voice this is, of how many.
    unison: usize,
    unisons: usize,
    mods: [ModState; MOD_MODULATORS],
    /// Random numbers for its modulators.
    seed: u32,
    /// Polyphonic pressure on its key.
    pressure: f32,
    modules: [ModuleState; MOD_MODULES],
    grains: [Grain; GRAINS],
    /// Each module's latest output (mono), the source of audio-rate modulation.
    outputs: [f32; MOD_MODULES],
    /// Its modules have finished; only its poly lanes' effects still ring.
    tail: bool,
    /// Seconds its tail has been silent.
    quiet: f32,
    /// Lane gains, effect settings and mixes as this voice's modulators set them.
    lane_gains: [f32; MOD_LANES],
    lane_values: [f32; LANE_VALUES],
}

/// Per-block settings of one module.
#[derive(Clone, Copy)]
struct Block {
    count: usize,
    incs: [f32; UNISON],
    gains: [(f32, f32); UNISON],
    level: f32,
    /// Base frequency in Hz (for audio-rate shift modulation).
    freq: f32,
    sync: f32,
    width: f32,
    frame: f32,
    mip: usize,
    /// Sample: playback rate per unison voice, offset, loop mode and loop points (frames).
    rates: [f64; UNISON],
    start: f64,
    loop_mode: u8,
    loop_start: f64,
    loop_end: f64,
    xfade: f64,
    /// Noise: mix of white, pink, brown, blue and violet; stereo; keytracked phase step.
    colors: [f32; 5],
    stereo: f32,
    keyed_inc: f32,
    /// Granular.
    grain_position: f32,
    grain_length: f32,
    grain_spawn: f32,
    grain_speed: f64,
    grain_attack: f32,
    grain_decay: f32,
    grain_power: f32,
    grain_norm: f32,
    random_position: f32,
    random_timing: f32,
    random_pitch: f32,
    random_level: f32,
    random_pan: f32,
    random_reverse: f32,
    chord: usize,
    chord_notes: usize,
    period: f64,
    biquad: Biquad,
    stages: usize,
    /// Nonlinear filter: TPT state-variable coefficients and its drive.
    svf: (f32, f32, f32, f32),
    drive: f32,
    bias: f32,
    spread: f32,
    mix: f32,
    steps: f32,
    pan: (f32, f32),
    envelope: WaveEnv,
    /// Curve output: progress per sample and loop points.
    curve_speed: f32,
    curve_start: f32,
    curve_end: f32,
    /// Output destination: a lane, `usize::MAX` for the master, None for nowhere.
    lane: Option<usize>,
}

impl Block {
    const NONE: Block = Block {
        count: 0,
        incs: [0.0; UNISON],
        gains: [(0.0, 0.0); UNISON],
        level: 0.0,
        freq: 0.0,
        sync: 1.0,
        width: 0.5,
        frame: 0.0,
        mip: 0,
        rates: [0.0; UNISON],
        start: 0.0,
        loop_mode: 0,
        loop_start: 0.0,
        loop_end: 0.0,
        xfade: 0.0,
        colors: [1.0, 0.0, 0.0, 0.0, 0.0],
        stereo: 0.0,
        keyed_inc: 0.0,
        grain_position: 0.0,
        grain_length: 1.0,
        grain_spawn: 0.0,
        grain_speed: 0.0,
        grain_attack: 0.5,
        grain_decay: 0.5,
        grain_power: 1.0,
        grain_norm: 1.0,
        random_position: 0.0,
        random_timing: 0.0,
        random_pitch: 0.0,
        random_level: 0.0,
        random_pan: 0.0,
        random_reverse: 0.0,
        chord: 0,
        chord_notes: 1,
        period: 0.0,
        biquad: Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 },
        stages: 1,
        svf: (1.0, 0.0, 0.0, 1.0),
        drive: 1.0,
        bias: 0.0,
        spread: 0.0,
        mix: 1.0,
        steps: 16.0,
        pan: (1.0, 1.0),
        envelope: WaveEnv { attack: 0.001, hold: 0.0, decay: 1.0, sustain: 1.0, release: 0.1 },
        curve_speed: 0.0,
        curve_start: 0.0,
        curve_end: 1.0,
        lane: None,
    };
}

/// Equal-power gains for a pan of -1..1, unity in the middle.
fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
    (angle.cos() * SQRT_2, angle.sin() * SQRT_2)
}

/// Tuning and level of global unison voice `j` of `count`: (semitones, frequency
/// multiplier, gain, pan). `settings` are detune, spread, blend and bias.
fn unison_voice(mode: UnisonMode, chord: usize, j: usize, count: usize, settings: [f32; 4], note: f32) -> (f32, f32, f32, f32) {
    if count <= 1 {
        return (0.0, 1.0, 1.0, 0.0);
    }
    let [detune, spread, blend, bias] = settings;
    let bias = bias.clamp(-1.0, 1.0);
    let cents = detune.clamp(0.0, 1.0).powf(1.5);
    let (main, other) = ((2.0 * (1.0 - blend)).clamp(0.0, 1.0), (2.0 * blend).clamp(0.0, 1.0));
    let step = 1.0 / (count - 1) as f32;
    let mut gains = [0f32; MOD_UNISON_VOICES];
    let mut tuning = (0.0, 1.0, 0.0);
    for (k, gain) in gains.iter_mut().enumerate().take(count) {
        let s = k as f32 * step * 2.0 - 1.0;
        // A slight detune on top of every mode.
        let fine = s * cents;
        let (semis, mult, g, pan) = match mode {
            UnisonMode::Hard | UnisonMode::Smooth | UnisonMode::Synthetic => {
                // Bias pushes the outer voices out (negative) or pulls them in.
                let warped = s.signum() * s.abs().powf(2f32.powf(bias * 1.5));
                let centre = s.abs() <= step + 1e-4 && (count % 2 == 0 || s.abs() < 1e-4);
                (warped * cents, 1.0, if centre { main } else { other }, s * spread)
            }
            UnisonMode::FrequencyStack => (fine, 1.0 + k as f32 * (1.0 + bias), if k == 0 { main } else { other }, if k == 0 { 0.0 } else { s * spread }),
            UnisonMode::PitchStack => (k as f32 * 12.0 * (1.0 + bias) + fine, 1.0, if k == 0 { main } else { other }, if k == 0 { 0.0 } else { s * spread }),
            UnisonMode::Shepard => {
                // Octaves of the played pitch class around the center, loudest at the center.
                let centre = 60.0 + bias * 36.0;
                let lowest = centre - count as f32 * 6.0;
                let class = note.rem_euclid(12.0);
                let pitch = class + 12.0 * ((lowest - class) / 12.0).ceil() + 12.0 * k as f32;
                let width = count as f32 * 12.0 / 4.5;
                (pitch - note + fine, 1.0, (-((pitch - centre) / width).powi(2)).exp(), s * spread)
            }
            UnisonMode::Chords => (chord_note(chord, k) + fine, 1.0, 2f32.powf(bias * 2.0 * s) * if k == 0 { main } else { other }, s * spread),
        };
        *gain = g;
        if k == j {
            tuning = (semis, mult, pan);
        }
    }
    // The same overall level whatever the count.
    let power: f32 = gains[..count].iter().map(|g| g * g).sum();
    let scale = 1.0 / power.sqrt().max(1e-4);
    (tuning.0, tuning.1, gains[j] * scale, tuning.2)
}

/// A grain's envelope at `x` (0..1 of its life).
#[inline]
fn grain_envelope(x: f32, attack: f32, decay: f32, power: f32) -> f32 {
    let level = if x < attack {
        x / attack
    } else if x > 1.0 - decay {
        (1.0 - x) / decay
    } else {
        1.0
    };
    if power == 1.0 { level } else { level.max(0.0).powf(power) }
}

/// Starts a grain of module `k` that is already `age` samples old (warm start).
fn spawn_grain(grains: &mut [Grain; GRAINS], state: &mut ModuleState, b: &Block, k: usize, pattern: u8, align: bool, frames: f64, age: f32) {
    let Some(free) = grains.iter().position(|g| g.module == NO_GRAIN) else { return };
    let n = b.chord_notes.max(1);
    let i = state.pick as usize;
    let step = match pattern {
        0 => i % n,
        1 => n - 1 - i % n,
        2 if n > 1 => {
            let period = 2 * n - 2;
            let at = i % period;
            if at < n { at } else { period - at }
        }
        2 => 0,
        _ => ((state.random() * n as f32) as usize).min(n - 1),
    };
    state.pick = state.pick.wrapping_add(1);
    let semis = chord_note(b.chord, step) + b.random_pitch * (state.random() * 2.0 - 1.0);
    let mut rate = b.grain_speed * 2f64.powf(semis as f64 / 12.0);
    if state.random() < b.random_reverse {
        rate = -rate;
    }
    let mut start = (b.grain_position + (state.random() - 0.5) * b.random_position).clamp(0.0, 1.0) as f64 * (frames - 1.0);
    if align && b.period > 1.0 {
        start = (start / b.period).round() * b.period;
    }
    let level = 1.0 - b.random_level * state.random();
    let (gl, gr) = pan_gains(b.random_pan * (state.random() * 2.0 - 1.0));
    grains[free] = Grain { module: k as u8, position: start + rate * age as f64, rate, age, length: b.grain_length, gains: (gl * level, gr * level) };
}

/// A lane's effects, each with whether it is on.
type FxChain = Vec<Option<(EffectProcessor, bool)>>;

/// Allocates lane `l`'s effects; call off the audio thread.
fn lane_processors(params: &ModularParams, l: usize, sample_rate: f32) -> FxChain {
    params.lanes[l].fx.iter().map(|slot| slot.map(|d| (EffectProcessor::new(&d.effect, sample_rate), d.enabled))).collect()
}

/// Runs lane `l`'s effects over `buf` with its settings `values` (see [`LANE_VALUES`]),
/// then blends in the dry signal by the lane's mix.
fn process_lane(chain: &mut FxChain, l: usize, values: &[f32; LANE_VALUES], buf: &mut [[f32; CONTROL]; 2], n: usize, ctx: &ProcessContext) {
    if chain.iter().flatten().all(|(_, on)| !on) {
        return;
    }
    let dry = *buf;
    let [left, right] = buf;
    for (slot, (processor, _)) in chain.iter_mut().enumerate().filter_map(|(k, fx)| fx.as_mut().filter(|(_, on)| *on).map(|fx| (k, fx))) {
        let at = ModularParams::lane_fx_param(l, slot, 0) - MOD_LANE_FX_PARAMS;
        let settings = processor.effect().params().len();
        for k in 0..settings {
            processor.set_param(k, values[at + k]);
        }
        processor.process(&mut left[..n], &mut right[..n], ctx);
    }
    let mix = values[MOD_LANE_MIX - MOD_LANE_FX_PARAMS + l].clamp(0.0, 1.0);
    if mix < 1.0 {
        for (wet, dry) in buf.iter_mut().zip(dry.iter()) {
            for (w, d) in wet[..n].iter_mut().zip(&dry[..n]) {
                *w = d + (*w - d) * mix;
            }
        }
    }
}

pub struct ModSynth {
    params: ModularParams,
    sample_rate: f32,
    tempo: f64,
    voices: Vec<Voice>,
    /// Per-block module settings, reused from voice to voice.
    blocks: Box<[Block; MOD_MODULES]>,
    counter: u64,
    clock: f64,
    held: [u8; 16],
    held_count: usize,
    last_note: Option<f32>,
    seed: u32,
    lanes: Vec<FxChain>,
    /// Per voice, the effects of the poly lanes (empty for the others).
    poly: Vec<Vec<FxChain>>,
    /// Values of the modulators that are the same for every voice, this block.
    globals: [f32; MOD_MODULATORS],
    /// Random modulators in Unison mode: one sequence for all voices.
    shared: [ModState; MOD_MODULATORS],
    /// Audio follower levels and tracked pitches (notes), per modulator.
    follow: [f32; MOD_MODULATORS],
    tracked: [f32; MOD_MODULATORS],
    tracker: Box<Tracker>,
    cc: [f32; 128],
    bend: f32,
    pressure: f32,
    /// Another note was held when the latest note started (legato triggers).
    phrase: bool,
}

/// The recent audio of each lane and the master, decimated, for pitch tracking.
struct Tracker {
    rings: [[f32; TRACK_RING]; LISTEN],
    pos: usize,
    sums: [f32; LISTEN],
    summed: usize,
    blocks: u32,
    window: [f32; TRACK_RING],
    diff: [f32; TRACK_RING],
}

impl Tracker {
    fn push(&mut self, input: [f32; LISTEN]) {
        for (sum, x) in self.sums.iter_mut().zip(input) {
            *sum += x;
        }
        self.summed += 1;
        if self.summed == TRACK_DECIMATE {
            for (ring, sum) in self.rings.iter_mut().zip(self.sums.iter_mut()) {
                ring[self.pos] = *sum / TRACK_DECIMATE as f32;
                *sum = 0.0;
            }
            self.pos = (self.pos + 1) % TRACK_RING;
            self.summed = 0;
        }
    }

    /// The pitch (a note number) of `input` between notes `low` and `high`, if one is
    /// clear enough (YIN's cumulative mean difference, below `threshold`).
    fn detect(&mut self, input: usize, sample_rate: f32, low: f32, high: f32, threshold: f32) -> Option<f32> {
        let rate = sample_rate / TRACK_DECIMATE as f32;
        let longest = ((rate / note_freq(low.min(high))) as usize + 2).min(TRACK_RING - TRACK_WINDOW - 1);
        let shortest = ((rate / note_freq(low.max(high))) as usize).max(2).min(longest);
        let ring = &self.rings[input.min(LISTEN - 1)];
        // Oldest first: the window and the lags reach back from the newest sample.
        let count = TRACK_WINDOW + longest;
        let start = (self.pos + TRACK_RING - count) % TRACK_RING;
        for (j, x) in self.window[..count].iter_mut().enumerate() {
            *x = ring[(start + j) % TRACK_RING];
        }
        let x = &self.window[..count];
        let energy: f32 = x[..TRACK_WINDOW].iter().map(|v| v * v).sum::<f32>() / TRACK_WINDOW as f32;
        if energy < 1e-6 {
            return None;
        }
        let mut total = 0.0;
        self.diff[0] = 1.0;
        for tau in 1..=longest {
            let mut d = 0.0;
            for j in 0..TRACK_WINDOW {
                let e = x[j] - x[j + tau];
                d += e * e;
            }
            total += d;
            self.diff[tau] = if total > 0.0 { d * tau as f32 / total } else { 1.0 };
        }
        let mut tau = (shortest..longest).find(|&t| self.diff[t] < threshold)?;
        while tau + 1 < longest && self.diff[tau + 1] < self.diff[tau] {
            tau += 1;
        }
        // Parabolic interpolation between the neighbouring lags.
        let (a, b, c) = (self.diff[tau - 1], self.diff[tau], self.diff[tau + 1]);
        let bottom = a - 2.0 * b + c;
        let offset = if bottom.abs() > 1e-9 { (0.5 * (a - c) / bottom).clamp(-1.0, 1.0) } else { 0.0 };
        let freq = rate / (tau as f32 + offset);
        Some(69.0 + 12.0 * (freq / 440.0).log2())
    }
}

/// The modulators for a voice, applied to the parameter values `m`: first the
/// modulation of modulator settings (with last block's values), then every
/// modulator in order, then their modulation of everything else.
fn modulate(p: &ModularParams, voice: &mut Voice, inputs: &ModInputs, m: &mut [f32; PARAMS], block: f32) {
    let apply = |m: &mut [f32; PARAMS], dest: usize, amount: f32| {
        let Some(info) = p.param_info(dest) else { return };
        m[dest] = info.denormalize(info.normalize(m[dest]) + amount);
    };
    for (i, modulator) in p.modulators.iter().enumerate().filter(|(_, md)| md.kind != ModulatorKind::Off) {
        for target in modulator.targets.iter().filter(|t| ModularParams::param_modulator(t.dest as usize).is_some()) {
            apply(m, target.dest as usize, target.amount(voice.mods[i].out));
        }
    }
    let mut seed = voice.seed;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    for (i, modulator) in p.modulators.iter().enumerate() {
        if modulator.kind == ModulatorKind::Off {
            continue;
        }
        let at = |k: usize| modulator_value(modulator, m, i, k);
        let input = |source: u8| if source == MOD_NO_MODULATOR || source as usize >= MOD_MODULATORS { 0.0 } else { voice.mods[source as usize].out };
        let (trigger, source) = (input(modulator.trigger_source), input(modulator.source));
        let state = &mut voice.mods[i];
        // A trigger input restarts it when it rises and releases it when it falls.
        if modulator.trigger_source != MOD_NO_MODULATOR && modulator.kind.triggered() {
            let threshold = if modulator.kind == ModulatorKind::SampleHold { at(mslot::THRESHOLD) } else { 0.5 };
            let gate = trigger > threshold;
            if gate && !state.gate {
                state.trigger(modulator, at(mslot::DELAY), random());
            } else if !gate && state.gate {
                state.release();
            }
            state.gate = gate;
        }
        let cycle = |state: &mut ModState, rate: f32| -> f32 {
            let free = modulator.mode == LfoMode::Free || modulator.note_trigger(inputs.legato) == TriggerMode::Never && modulator.trigger_source == MOD_NO_MODULATOR;
            let offset = at(mslot::PHASE);
            let x = if free {
                ((inputs.clock * rate as f64 + offset as f64).fract()) as f32
            } else if modulator.mode == LfoMode::Envelope {
                if state.phase >= 1.0 { 1.0 } else { (state.phase + offset).fract() }
            } else {
                (state.phase + offset).fract()
            };
            state.phase += rate * block;
            if modulator.mode != LfoMode::Envelope {
                state.phase = state.phase.fract();
            }
            x
        };
        let synced = |beats: f32| inputs.tempo / 60.0 / beats;
        let raw = match modulator.kind {
            ModulatorKind::Off => 0.0,
            ModulatorKind::Envelope => {
                if state.delay >= 0.0 {
                    state.delay -= block;
                    if state.delay < 0.0 {
                        state.env.trigger();
                    }
                }
                let mut env = envelope(m[ModularParams::modulator_param(i, 0)], m[ModularParams::modulator_param(i, 1)], m[ModularParams::modulator_param(i, 2)], m[ModularParams::modulator_param(i, 3)]);
                env.hold = at(mslot::HOLD);
                state.env.tick(&env, block)
            }
            ModulatorKind::Lfo => {
                let rate = if modulator.sync_beats > 0.0 { synced(modulator.sync_beats) } else { at(mslot::RATE) };
                let x = cycle(state, rate);
                modulator.shape.value_at(x)
            }
            ModulatorKind::LfoTable => {
                let rate = if modulator.sync_beats > 0.0 { synced(modulator.sync_beats) } else { at(mslot::RATE) };
                let x = cycle(state, rate);
                match inputs.tables.get(MOD_MODULES + i) {
                    // Smoothing reads a mip level with fewer harmonics.
                    Some(table) if table.frames() > 0 => {
                        let level = (at(mslot::SMOOTH_TABLE) * (crate::wavetable::LEVELS - 1) as f32).round() as usize;
                        (table.read(level, at(mslot::FRAME), x) * 0.5 + 0.5).clamp(0.0, 1.0)
                    }
                    _ => 0.5,
                }
            }
            ModulatorKind::Timeline => {
                let length = if modulator.sync_beats > 0.0 { 1.0 / synced(modulator.sync_beats) } else { at(mslot::LENGTH) };
                let x = timeline_tick(state, modulator.loop_mode, (at(mslot::LOOP_START), at(mslot::LOOP_END)), 1.0 / length.max(1e-3), block);
                modulator.shape.value_at(x)
            }
            ModulatorKind::Random if modulator.shared => inputs.globals[i],
            ModulatorKind::Random => random_tick(state, at(mslot::RATE), at(mslot::SMOOTH), at(mslot::JITTER), at(mslot::CHAOS), block, &mut random),
            ModulatorKind::Note => ((voice.note + inputs.bend * p.bend_range as f32) / 127.0).clamp(0.0, 1.0),
            ModulatorKind::Curve => modulator.shape.value_at(match modulator.input {
                CurveInput::Velocity => voice.velocity,
                CurveInput::Note => voice.note / 127.0,
                CurveInput::Random => voice.random,
            }),
            ModulatorKind::Velocity => voice.velocity,
            ModulatorKind::NoteGate => voice.gate.held as u8 as f32,
            ModulatorKind::Pressure => voice.pressure.max(inputs.pressure),
            ModulatorKind::Macro => m[MOD_MACRO_PARAMS + (modulator.index as usize).min(7)],
            ModulatorKind::Remap => modulator.shape.value_at(source.clamp(0.0, 1.0)),
            ModulatorKind::LowerLimit => source.max(at(mslot::LIMIT)),
            ModulatorKind::UpperLimit => source.min(at(mslot::LIMIT)),
            ModulatorKind::Scale => source * at(mslot::FACTOR) * SCALE_MULTIPLIERS[(modulator.index as usize).min(SCALE_MULTIPLIERS.len() - 1)],
            ModulatorKind::SampleHold => {
                if state.pending {
                    state.sample = source;
                    state.pending = false;
                }
                state.sample
            }
            ModulatorKind::AudioFollower | ModulatorKind::PitchTracker | ModulatorKind::PitchWheel | ModulatorKind::MidiCc => inputs.globals[i],
        };
        state.out = modulator.range().apply(raw) * at(mslot::DEPTH);
    }
    voice.seed = voice.seed.wrapping_mul(0x2C1B_3C6D).wrapping_add(0x297A_2D39);
    for (i, modulator) in p.modulators.iter().enumerate().filter(|(_, md)| md.kind != ModulatorKind::Off) {
        let value = voice.mods[i].out;
        for target in modulator.targets.iter().filter(|t| (t.dest as usize) < PARAMS && ModularParams::param_modulator(t.dest as usize).is_none()) {
            apply(m, target.dest as usize, target.amount(value));
        }
    }
}

/// Voice `i`'s first seed, for its modulators.
fn voice_seed(i: usize) -> u32 {
    0x6C07_8965 ^ (i as u32 + 1).wrapping_mul(0x2545_F491)
}

/// Module `m` of voice `i`'s first noise seed.
fn module_seed(i: usize, m: usize) -> u32 {
    0x9E37_79B9 ^ ((i * MOD_MODULES + m) as u32).wrapping_mul(0x85EB_CA6B)
}

impl ModSynth {
    /// Its random numbers from the start again (the transport starts: it
    /// plays the same every time, and as exported).
    pub fn reseed(&mut self) {
        self.seed = 0x2468_ACE1;
        for (i, voice) in self.voices.iter_mut().enumerate() {
            voice.seed = voice_seed(i);
            for (m, module) in voice.modules.iter_mut().enumerate() {
                module.seed = module_seed(i, m);
            }
        }
    }

    pub fn new(params: ModularParams, sample_rate: f32) -> Self {
        let voice = |i: usize| {
            let module = |m: usize| ModuleState {
                phases: [0.0; UNISON],
                masters: [0.0; UNISON],
                positions: [-1.0; UNISON],
                back: [false; UNISON],
                seed: module_seed(i, m),
                pink: [[0.0; 7]; 2],
                brown: [0.0; 2],
                previous: [[0.0; 2]; 2],
                keyed_phase: 0.0,
                keyed: [[0.0; 2]; 2],
                biquad: [[[0.0; 2]; 2]; 2],
                svf: [[0.0; 2]; 2],
                env: Env::IDLE,
                spawn: 1.0,
                pick: 0,
                warm: false,
                curve: 0.0,
                curve_back: false,
                fade: 1.0,
                done: false,
            };
            Voice {
                active: false,
                gate: Gate::OFF,
                note: 60.0,
                target: 60.0,
                velocity: 0.0,
                random: 0.0,
                age: 0.0,
                unison: 0,
                unisons: 1,
                mods: [ModState::IDLE; MOD_MODULATORS],
                seed: voice_seed(i),
                pressure: 0.0,
                modules: std::array::from_fn(module),
                grains: [Grain::FREE; GRAINS],
                outputs: [0.0; MOD_MODULES],
                tail: false,
                quiet: 0.0,
                lane_gains: [1.0; MOD_LANES],
                lane_values: [0.0; LANE_VALUES],
            }
        };
        let lanes = (0..MOD_LANES).map(|l| lane_processors(&params, l, sample_rate)).collect();
        // Poly lanes have their effects once per voice.
        let poly = (0..VOICES).map(|_| (0..MOD_LANES).map(|l| if params.lane_poly(l) { lane_processors(&params, l, sample_rate) } else { Vec::new() }).collect()).collect();
        Self {
            params,
            sample_rate,
            tempo: 120.0,
            voices: (0..VOICES).map(voice).collect(),
            blocks: Box::new([Block::NONE; MOD_MODULES]),
            counter: 0,
            clock: 0.0,
            held: [0; 16],
            held_count: 0,
            last_note: None,
            seed: 0x2468_ACE1,
            lanes,
            poly,
            globals: [0.0; MOD_MODULATORS],
            shared: [ModState::IDLE; MOD_MODULATORS],
            follow: [0.0; MOD_MODULATORS],
            tracked: [60.0; MOD_MODULATORS],
            tracker: Box::new(Tracker { rings: [[0.0; TRACK_RING]; LISTEN], pos: 0, sums: [0.0; LISTEN], summed: 0, blocks: 0, window: [0.0; TRACK_RING], diff: [0.0; TRACK_RING] }),
            cc: [0.0; 128],
            bend: 0.0,
            pressure: 0.0,
            phrase: false,
        }
    }

    /// A MIDI controller moved.
    pub fn control(&mut self, controller: Controller, pitch: u8, value: f32) {
        match controller {
            Controller::Cc(n) => self.cc[(n & 127) as usize] = value.clamp(0.0, 1.0),
            Controller::PitchBend => self.bend = value.clamp(-1.0, 1.0),
            Controller::Pressure => self.pressure = value.clamp(0.0, 1.0),
            Controller::PolyPressure => {
                for v in self.voices.iter_mut().filter(|v| v.active && v.gate.pitch == pitch) {
                    v.pressure = value.clamp(0.0, 1.0);
                }
            }
        }
    }

    pub fn params(&self) -> ModularParams {
        self.params
    }

    pub fn set_params(&mut self, params: ModularParams) {
        self.params = params;
        for lanes in std::iter::once(&mut self.lanes).chain(self.poly.iter_mut()) {
            for (lane, settings) in lanes.iter_mut().zip(params.lanes.iter()) {
                for (slot, device) in lane.iter_mut().zip(settings.fx.iter()) {
                    if let (Some((processor, enabled)), Some(device)) = (slot.as_mut(), device) {
                        processor.set_params(&device.effect);
                        *enabled = device.enabled;
                    }
                }
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

    /// Global unison voices per note.
    fn unisons(&self) -> usize {
        (self.params.unison.voices as usize).clamp(1, MOD_UNISON_VOICES)
    }

    /// Random start phases for a note: per module and oscillator unison voice.
    fn note_randoms(&mut self) -> [[f32; UNISON]; MOD_MODULES] {
        let mut randoms = [[0f32; UNISON]; MOD_MODULES];
        for module in &mut randoms {
            for r in module.iter_mut() {
                *r = self.random() * 2.0 - 1.0;
            }
        }
        randoms
    }

    #[allow(clippy::too_many_arguments)]
    fn start_voice(&mut self, index: usize, pitch: u8, velocity: f32, gate: u32, retrigger: bool, (j, count): (usize, usize), randoms: &[[f32; UNISON]; MOD_MODULES]) {
        let glide_from = if self.params.glide > 0.0 { self.last_note } else { None };
        let random = self.random();
        // Where this unison voice starts in every oscillator's cycle.
        let offset = match self.params.unison.mode {
            _ if count <= 1 => 0.0,
            UnisonMode::Hard => 0.0,
            UnisonMode::Synthetic => j as f32 / count as f32,
            _ => self.random(),
        };
        let p = self.params;
        let counter = self.counter;
        let mut mod_randoms = [0f32; MOD_MODULATORS];
        for r in &mut mod_randoms {
            *r = self.random();
        }
        // In a legato phrase, untriggered modulators carry on from the newest held note.
        let donor = self.voices.iter().enumerate().filter(|&(k, v)| k != index && v.active && v.gate.held).max_by_key(|(_, v)| v.gate.age).map(|(_, v)| v.mods);
        let phrase = self.phrase;
        if !self.voices[index].active {
            for (processor, _) in self.poly[index].iter_mut().flatten().flatten() {
                processor.reset();
            }
        }
        let voice = &mut self.voices[index];
        let was_active = voice.active;
        voice.tail = false;
        voice.gate = Gate::new(pitch, velocity, gate, counter);
        voice.target = pitch as f32;
        voice.velocity = velocity;
        voice.unison = j;
        voice.unisons = count;
        if !was_active || retrigger {
            voice.note = glide_from.unwrap_or(pitch as f32);
            voice.random = random;
            voice.age = 0.0;
            for (m, state) in voice.modules.iter_mut().enumerate() {
                let module = &p.modules[m];
                // Start phase plus a random variation per note and unison voice.
                let phase = module.get(slot::PHASE) / 360.0 + offset;
                let spread = module.get(slot::PHASE_RANDOM) / 360.0;
                for u in 0..UNISON {
                    state.phases[u] = (phase + spread * randoms[m][u]).rem_euclid(1.0);
                    state.masters[u] = 0.0;
                }
                state.positions = [-1.0; UNISON];
                state.back = [false; UNISON];
                if !was_active {
                    state.biquad = [[[0.0; 2]; 2]; 2];
                    state.svf = [[0.0; 2]; 2];
                    state.pink = [[0.0; 7]; 2];
                    state.brown = [0.0; 2];
                    state.previous = [[0.0; 2]; 2];
                }
                if module.kind == ModuleKind::Noise && module.choice[1] == 1 {
                    // Stable seed: the same noise on every note.
                    state.seed = 0x1234_5678 ^ (m as u32 + 1).wrapping_mul(0x9E37_79B9);
                }
                state.keyed_phase = 1.0;
                state.spawn = 1.0;
                state.pick = 0;
                state.warm = module.kind == ModuleKind::Granular && module.flag;
                state.curve = 0.0;
                state.curve_back = false;
                state.fade = 1.0;
                state.done = false;
                state.env.trigger();
            }
            if !was_active {
                voice.outputs = [0.0; MOD_MODULES];
                voice.grains = [Grain::FREE; GRAINS];
            }
            voice.pressure = 0.0;
        }
        for (k, modulator) in p.modulators.iter().enumerate() {
            if modulator.kind == ModulatorKind::Off {
                continue;
            }
            let mode = modulator.note_trigger(p.mono && p.legato);
            let fire = match mode {
                TriggerMode::Always => true,
                TriggerMode::Legato => !phrase,
                _ => false,
            };
            if fire {
                voice.mods[k].trigger(modulator, modulator.get(mslot::DELAY), mod_randoms[k]);
            } else if !was_active {
                voice.mods[k] = match donor {
                    Some(donor) if mode == TriggerMode::Legato => donor[k],
                    _ => ModState { from: mod_randoms[k], to: mod_randoms[k], ..ModState::IDLE },
                };
            }
        }
        voice.active = true;
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32, gate: u32) {
        let count = self.unisons();
        let randoms = self.note_randoms();
        self.counter += 1;
        if self.params.mono {
            self.phrase = self.held_count > 0;
            let legato = self.params.legato && self.voices[0].active && self.voices[0].gate.held && self.held_count > 0;
            if self.held_count < self.held.len() {
                self.held[self.held_count] = pitch;
                self.held_count += 1;
            }
            for j in 0..count {
                self.start_voice(j, pitch, velocity, gate, !legato, (j, count), &randoms);
            }
            for v in &mut self.voices[count..] {
                v.active = false;
            }
            self.last_note = Some(pitch as f32);
            return;
        }
        self.phrase = self.voices.iter().any(|v| v.active && v.gate.held);
        let limit = (VOICES / count).min((self.params.polyphony as usize).max(1));
        loop {
            let free = self.voices.iter().filter(|v| !v.active).count();
            let mut notes = 0;
            for (i, v) in self.voices.iter().enumerate().filter(|(_, v)| v.active) {
                if !self.voices[..i].iter().any(|w| w.active && w.gate.age == v.gate.age) {
                    notes += 1;
                }
            }
            if free >= count && notes < limit {
                break;
            }
            // Steal the oldest note, with all its unison voices.
            let Some(oldest) = self.voices.iter().filter(|v| v.active).map(|v| v.gate.age).min() else { break };
            for v in self.voices.iter_mut().filter(|v| v.active && v.gate.age == oldest) {
                v.active = false;
            }
        }
        let mut j = 0;
        for index in 0..VOICES {
            if j < count && !self.voices[index].active {
                self.start_voice(index, pitch, velocity, gate, true, (j, count), &randoms);
                j += 1;
            }
        }
        self.last_note = Some(pitch as f32);
    }

    fn release(voice: &mut Voice, p: &ModularParams) {
        voice.gate.held = false;
        // Modulators with a trigger input are released by it instead.
        for (state, modulator) in voice.mods.iter_mut().zip(p.modulators.iter()) {
            if modulator.trigger_source == MOD_NO_MODULATOR {
                state.release();
            }
        }
        voice.modules.iter_mut().for_each(|m| m.env.release());
    }

    pub fn note_off(&mut self, pitch: u8) {
        if self.params.mono {
            if let Some(at) = self.held[..self.held_count].iter().position(|&n| n == pitch) {
                self.held.copy_within(at + 1..self.held_count, at);
                self.held_count -= 1;
            }
            let previous = (self.held_count > 0).then(|| self.held[self.held_count - 1]);
            let count = self.unisons();
            let p = self.params;
            for voice in &mut self.voices[..count] {
                if voice.gate.held && voice.gate.pitch == pitch {
                    match previous {
                        Some(previous) => {
                            voice.gate.pitch = previous;
                            voice.target = previous as f32;
                        }
                        None => Self::release(voice, &p),
                    }
                }
            }
            return;
        }
        let p = self.params;
        for v in self.voices.iter_mut().filter(|v| v.active && v.gate.held && v.gate.pitch == pitch) {
            Self::release(v, &p);
        }
    }

    pub fn all_notes_off(&mut self) {
        self.held_count = 0;
        let p = self.params;
        self.voices.iter_mut().for_each(|v| Self::release(v, &p));
    }

    pub fn render(&mut self, samples: &[Arc<Sample>], tables: &[Arc<Wavetable>], left: &mut [f32], right: &mut [f32]) {
        let mut base = [0f32; PARAMS];
        let mut params = self.params;
        for (i, value) in base.iter_mut().enumerate() {
            *value = params.param_mut(i).map_or(0.0, |v| *v);
        }
        let p = self.params;
        // Poly lanes come first, from lane 1 on.
        let polys = (0..MOD_LANES).take_while(|&l| p.lane_poly(l)).count();
        let poly_fx = (0..polys).any(|l| p.lanes[l].fx.iter().flatten().any(|d| d.enabled));
        // Echoes can come after a silence as long as the delay.
        let hold = TAIL_HOLD
            + (0..polys)
                .flat_map(|l| p.lanes[l].fx.iter().flatten())
                .filter_map(|d| match d.effect {
                    Effect::Delay(delay) if d.enabled => Some((delay.time_beats * 60.0 / self.tempo.max(1.0)) as f32),
                    _ => None,
                })
                .fold(0f32, f32::max);
        let dt = 1.0 / self.sample_rate;
        let mut offset = 0;
        while offset < left.len() {
            let n = (left.len() - offset).min(CONTROL);
            let mut bus = [[[0f32; CONTROL]; 2]; MOD_LANES];
            let mut master = [[0f32; CONTROL]; 2];
            let mut sideband = [[0f32; CONTROL]; 2];
            self.update_globals(&base, n as f32 / self.sample_rate);
            let tempo = self.tempo;
            let ctx = ProcessContext { sample_rate: self.sample_rate, tempo, beat: self.clock * tempo / 60.0, beats_per_sample: tempo / 60.0 / self.sample_rate as f64, scale: 0 };
            let mut newest: Option<(usize, u64)> = None;
            for v in 0..VOICES {
                if !self.voices[v].active {
                    continue;
                }
                let mut own = [[[0f32; CONTROL]; 2]; MOD_LANES];
                let alive = !self.voices[v].tail && self.render_voice(v, samples, tables, &base, &mut own, &mut master, &mut sideband, n);
                if !self.voices[v].tail && newest.is_none_or(|(_, age)| self.voices[v].gate.age > age) {
                    newest = Some((v, self.voices[v].gate.age));
                }
                let voice = &mut self.voices[v];
                let mut peak = 0f32;
                for l in 0..MOD_LANES {
                    if l >= polys {
                        for (to, from) in bus[l].iter_mut().zip(own[l].iter()) {
                            for (a, b) in to[..n].iter_mut().zip(&from[..n]) {
                                *a += b;
                            }
                        }
                        continue;
                    }
                    process_lane(&mut self.poly[v][l], l, &voice.lane_values, &mut own[l], n, &ctx);
                    if !p.lane_audible(l) {
                        continue;
                    }
                    let gain = voice.lane_gains[l];
                    let dest = p.lanes[l].dest as usize;
                    let into = (p.lanes[l].dest != MOD_NO_LANE && dest > l && dest < MOD_LANES).then_some(dest);
                    for i in 0..n {
                        let (a, b) = (own[l][0][i] * gain, own[l][1][i] * gain);
                        peak = peak.max(a.abs()).max(b.abs());
                        // What the lane plays, for followers and trackers listening to it.
                        bus[l][0][i] += a;
                        bus[l][1][i] += b;
                        let out = match into {
                            Some(d) if d < polys => &mut own[d],
                            Some(d) => &mut bus[d],
                            None => &mut master,
                        };
                        out[0][i] += a;
                        out[1][i] += b;
                    }
                }
                if voice.tail {
                    voice.age += n as f32 * dt;
                    voice.quiet = if peak < TAIL_SILENCE { voice.quiet + n as f32 * dt } else { 0.0 };
                    if voice.quiet > hold || voice.age > TAIL_MAX {
                        voice.tail = false;
                        voice.active = false;
                    }
                } else if !alive {
                    // Its effects ring on after the modules finish, as long as they sound.
                    if poly_fx {
                        voice.tail = true;
                        voice.age = 0.0;
                        voice.quiet = 0.0;
                    } else {
                        voice.active = false;
                    }
                }
            }
            // Shared lanes follow the modulation of the newest note.
            let (gains, values) = match newest {
                Some((v, _)) => (self.voices[v].lane_gains, self.voices[v].lane_values),
                None => {
                    let mut values = [0f32; LANE_VALUES];
                    values.copy_from_slice(&base[MOD_LANE_FX_PARAMS..MOD_PITCH]);
                    ([0, 1, 2].map(|l| base[MOD_LANE_GAIN + l]), values)
                }
            };
            for lane in polys..MOD_LANES {
                process_lane(&mut self.lanes[lane], lane, &values, &mut bus[lane], n, &ctx);
                if !p.lane_audible(lane) {
                    bus[lane] = [[0.0; CONTROL]; 2];
                    continue;
                }
                let gain = gains[lane];
                let dest = p.lanes[lane].dest as usize;
                // Lanes only feed later lanes, so routing can never loop.
                let into = (p.lanes[lane].dest != MOD_NO_LANE && dest > lane && dest < MOD_LANES).then_some(dest);
                for i in 0..n {
                    let (a, b) = (bus[lane][0][i] * gain, bus[lane][1][i] * gain);
                    match into {
                        Some(d) => {
                            bus[d][0][i] += a;
                            bus[d][1][i] += b;
                        }
                        None => {
                            master[0][i] += a;
                            master[1][i] += b;
                        }
                    }
                }
            }
            self.listen(&base, &bus, &master, &sideband, n);
            let gain = base[MOD_MASTER];
            for i in 0..n {
                left[offset + i] += master[0][i] * gain;
                right[offset + i] += master[1][i] * gain;
            }
            self.clock += n as f64 / self.sample_rate as f64;
            offset += n;
        }
    }

    /// The modulators that are the same for every voice, for this block.
    fn update_globals(&mut self, base: &[f32; PARAMS], block: f32) {
        let Self { params, globals, shared, follow, tracked, cc, bend, seed, .. } = self;
        let mut random = || {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 17;
            *seed ^= *seed << 5;
            *seed as f32 / u32::MAX as f32
        };
        for (i, modulator) in params.modulators.iter().enumerate() {
            let at = |k: usize| modulator_value(modulator, base, i, k);
            globals[i] = match modulator.kind {
                ModulatorKind::AudioFollower => follow[i].min(1.0),
                ModulatorKind::PitchTracker => {
                    let (low, high) = (at(mslot::LOW), at(mslot::HIGH));
                    let (low, high) = (low.min(high), low.max(high).max(low.min(high) + 1.0));
                    ((tracked[i] - low) / (high - low)).clamp(0.0, 1.0)
                }
                ModulatorKind::PitchWheel => *bend * 0.5 + 0.5,
                ModulatorKind::MidiCc => cc[(modulator.index & 127) as usize],
                ModulatorKind::Random if modulator.shared => random_tick(&mut shared[i], at(mslot::RATE), at(mslot::SMOOTH), at(mslot::JITTER), at(mslot::CHAOS), block, &mut random),
                _ => 0.0,
            };
        }
    }

    /// Follows the level and pitch of the lanes, the master and the sideband (this block's).
    fn listen(&mut self, base: &[f32; PARAMS], bus: &[[[f32; CONTROL]; 2]; MOD_LANES], master: &[[f32; CONTROL]; 2], sideband: &[[f32; CONTROL]; 2], n: usize) {
        let sr = self.sample_rate;
        let channel = |input: usize| match input {
            i if i < MOD_LANES => &bus[i],
            i if i == MOD_LANES => master,
            _ => sideband,
        };
        let mut tracking = false;
        for (i, modulator) in self.params.modulators.iter().enumerate() {
            match modulator.kind {
                ModulatorKind::AudioFollower => {
                    let at = |k: usize| modulator_value(modulator, base, i, k);
                    let gain = 10f32.powf(at(mslot::GAIN) / 20.0);
                    let attack = 1.0 - (-1.0 / (at(mslot::FOLLOW_ATTACK) * sr)).exp();
                    let release = 1.0 - (-1.0 / (at(mslot::FOLLOW_RELEASE) * sr)).exp();
                    let [l, r] = channel(modulator.index as usize);
                    let level = &mut self.follow[i];
                    for j in 0..n {
                        let x = l[j].abs().max(r[j].abs()) * gain;
                        *level += (x - *level) * if x > *level { attack } else { release };
                    }
                }
                ModulatorKind::PitchTracker => tracking = true,
                _ => {}
            }
        }
        if !tracking {
            return;
        }
        for j in 0..n {
            let mono = |input: usize| {
                let [l, r] = channel(input);
                (l[j] + r[j]) * 0.5
            };
            self.tracker.push(std::array::from_fn(mono));
        }
        self.tracker.blocks += 1;
        if self.tracker.blocks < TRACK_EVERY {
            return;
        }
        self.tracker.blocks = 0;
        for (i, modulator) in self.params.modulators.iter().enumerate().filter(|(_, md)| md.kind == ModulatorKind::PitchTracker) {
            let at = |k: usize| modulator_value(modulator, base, i, k);
            let sensitivity = at(mslot::SENSITIVITY);
            // Sensitive: accepts less clear pitches and follows them faster.
            if let Some(note) = self.tracker.detect(modulator.index as usize, sr, at(mslot::LOW) - 1.0, at(mslot::HIGH) + 1.0, 0.08 + 0.3 * sensitivity) {
                self.tracked[i] += (note - self.tracked[i]) * (0.15 + 0.85 * sensitivity);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_voice(
        &mut self,
        v: usize,
        samples: &[Arc<Sample>],
        tables: &[Arc<Wavetable>],
        base: &[f32; PARAMS],
        bus: &mut [[[f32; CONTROL]; 2]; MOD_LANES],
        master: &mut [[f32; CONTROL]; 2],
        sideband: &mut [[f32; CONTROL]; 2],
        n: usize,
    ) -> bool {
        let p = self.params;
        let sr = self.sample_rate;
        let tempo = self.tempo as f32;
        let dt = 1.0 / sr;
        let block = n as f32 * dt;
        let mut m = *base;
        let inputs = ModInputs { tempo, clock: self.clock, legato: p.mono && p.legato, globals: &self.globals, tables, bend: self.bend, pressure: self.pressure };
        modulate(&p, &mut self.voices[v], &inputs, &mut m, block);
        let bend = self.bend * p.bend_range as f32 + m[MOD_PITCH] + m[MOD_FINE] / 100.0;

        let Self { voices, blocks, .. } = self;
        let voice = &mut voices[v];
        voice.lane_values.copy_from_slice(&m[MOD_LANE_FX_PARAMS..MOD_PITCH]);
        voice.lane_gains = [0, 1, 2].map(|l| m[MOD_LANE_GAIN + l]);
        let glide = m[MOD_GLIDE];
        if glide > 0.0 && voice.note != voice.target {
            voice.note += (voice.target - voice.note) * (1.0 - (-block * 3000.0 / glide).exp());
            if (voice.note - voice.target).abs() < 0.001 {
                voice.note = voice.target;
            }
        } else {
            voice.note = voice.target;
        }

        // This voice's place in the global unison.
        let unison = [m[MOD_UNISON], m[MOD_UNISON + 1], m[MOD_UNISON + 2], m[MOD_UNISON + 3]];
        let (unison_semis, unison_mult, unison_gain, unison_pan) = unison_voice(p.unison.mode, p.unison.chord as usize, voice.unison, voice.unisons, unison, voice.note);
        let unison_pan = pan_gains(unison_pan);
        let unison_out = (unison_pan.0 * unison_gain, unison_pan.1 * unison_gain);
        let note = voice.note + unison_semis + bend;

        // Module settings for the block, in real units.
        let last = p.modules.iter().rposition(|m| m.kind != ModuleKind::Empty).map_or(0, |l| l + 1);
        let mut has_output = false;
        for i in 0..last {
            let module = &p.modules[i];
            if !module.on || matches!(module.kind, ModuleKind::Empty | ModuleKind::Group) {
                continue;
            }
            let at = |s: usize| module_slot(module.kind, s).map_or(0.0, |info| info.denormalize(m[ModularParams::module_param(i, s)]));
            let b = &mut blocks[i];
            *b = Block::NONE;
            match module.kind {
                kind if kind.is_generator() => {
                    b.level = at(slot::LEVEL);
                    // Harmonic multiplies the note's frequency (0: no keytracking); Shift adds Hz.
                    let freq = note_freq(note + at(slot::SEMI) + at(slot::CENT) / 100.0) * at(slot::HARMONIC) * unison_mult + at(slot::SHIFT);
                    b.freq = freq;
                    let count = if kind.has_unison() { (at(slot::UNISON).round() as usize).clamp(1, UNISON) } else { 1 };
                    let spread_cents = if kind.has_unison() { at(slot::DETUNE).powf(1.5) * 100.0 } else { 0.0 };
                    let (spread, blend) = if kind.has_unison() { (at(slot::SPREAD), at(slot::BLEND)) } else { (0.0, 1.0) };
                    let mut norm = 0.0;
                    for u in 0..count {
                        let s = if count == 1 { 0.0 } else { u as f32 / (count - 1) as f32 * 2.0 - 1.0 };
                        b.incs[u] = freq * 2f32.powf(s * spread_cents / 1200.0) / sr;
                        let g = if count == 1 || s.abs() < 0.01 { 1.0 } else { blend.max(0.0) };
                        norm += g * g;
                        let angle = ((s * spread).clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
                        b.gains[u] = (angle.cos() * g, angle.sin() * g);
                    }
                    let scale = SQRT_2 / norm.sqrt().max(1e-6);
                    for g in &mut b.gains[..count] {
                        g.0 *= scale;
                        g.1 *= scale;
                    }
                    b.count = count;
                    match kind {
                        ModuleKind::Analog => {
                            b.sync = at(slot::A);
                            b.width = at(slot::B);
                        }
                        ModuleKind::Wavetable => {
                            b.frame = at(slot::A) / 255.0;
                            let top = freq.abs() * 2f32.powf(spread_cents / 1200.0);
                            // Bandlimit: no harmonics above it (a sharp low pass before any phase modulation).
                            let limit = at(slot::BANDLIMIT);
                            b.mip = Wavetable::level_for(top, sr).max(Wavetable::level_for(top * sr * 0.5 / limit.max(1.0), sr));
                        }
                        ModuleKind::Sample | ModuleKind::Granular => {
                            let Some(sample) = samples.get(i).filter(|s| s.frames() > 1) else {
                                b.count = 0;
                                continue;
                            };
                            let frames = sample.frames() as f64;
                            let native = if sample.sample_rate > 0.0 { sample.sample_rate } else { sr };
                            let root_slot = if kind == ModuleKind::Sample { slot::B } else { slot::ROOT };
                            let root = note_freq(at(root_slot)) / note_freq(60.0) * SAMPLE_ROOT_HZ;
                            if kind == ModuleKind::Sample {
                                for u in 0..count {
                                    b.rates[u] = (b.incs[u] * sr / root * native / sr) as f64;
                                }
                                b.start = at(slot::A).clamp(0.0, 0.999) as f64 * frames;
                                b.loop_mode = module.sample_loop();
                                b.loop_start = (at(slot::LOOP_START) as f64 * frames).min(frames - 2.0);
                                let length = (at(slot::LOOP_LENGTH) as f64 * (frames - b.loop_start)).max(2.0);
                                b.loop_end = (b.loop_start + length).min(frames);
                                b.xfade = at(slot::XFADE) as f64 * (b.loop_end - b.loop_start);
                            } else {
                                b.grain_speed = (freq / root * native / sr) as f64;
                                b.period = (native / root) as f64;
                                // Length keytracking: shorter grains for higher notes.
                                let length_ms = at(slot::LENGTH) * 2f32.powf(-(note - 60.0) / 12.0 * at(slot::LENGTH_TRACK));
                                b.grain_length = (length_ms * sr / 1000.0).max(4.0);
                                let rate = match module.choice[0] {
                                    1 => tempo / 60.0 / GRAIN_SYNC[(at(slot::SYNC).round() as usize).min(GRAIN_SYNC.len() - 1)].0,
                                    2 => at(slot::DENSITY) / (length_ms / 1000.0),
                                    _ => at(slot::RATE),
                                };
                                b.grain_spawn = rate / sr;
                                let (attack, decay) = (at(slot::GRAIN_ATTACK), at(slot::GRAIN_DECAY));
                                let total = (attack + decay).max(1.0);
                                b.grain_attack = attack / total;
                                b.grain_decay = decay / total;
                                b.grain_power = 2f32.powf(at(slot::GRAIN_CURVE) * 2.0);
                                // About the same level however many grains overlap.
                                b.grain_norm = 1.0 / (b.grain_spawn * b.grain_length).max(1.0).sqrt();
                                b.grain_position = at(slot::POSITION);
                                b.random_position = at(slot::RANDOM_POSITION);
                                b.random_timing = at(slot::RANDOM_TIMING);
                                b.random_pitch = at(slot::RANDOM_PITCH);
                                b.random_level = at(slot::RANDOM_LEVEL);
                                b.random_pan = at(slot::RANDOM_PAN);
                                b.random_reverse = at(slot::RANDOM_REVERSE);
                                b.chord = module.choice[1] as usize;
                                b.chord_notes = chord_size(b.chord, at(slot::CHORD_RANGE));
                            }
                        }
                        _ => {
                            // Noise: a slope of -6..6 dB per octave, from brown through pink,
                            // white and blue to violet.
                            let slope = at(slot::SLOPE);
                            let t = (slope.abs() % 3.0) / 3.0;
                            b.colors = match slope {
                                s if s <= -3.0 => [0.0, 1.0 - t, t, 0.0, 0.0],
                                s if s < 0.0 => [1.0 - t, t, 0.0, 0.0, 0.0],
                                s if s < 3.0 => [1.0 - t, 0.0, 0.0, t, 0.0],
                                _ => [0.0, 0.0, 0.0, 1.0 - t, t],
                            };
                            if slope.abs() >= 6.0 {
                                b.colors = if slope < 0.0 { [0.0, 0.0, 1.0, 0.0, 0.0] } else { [0.0, 0.0, 0.0, 0.0, 1.0] };
                            }
                            b.stereo = at(slot::NOISE_STEREO);
                            b.keyed_inc = (freq.abs() / sr).min(1.0);
                        }
                    }
                }
                ModuleKind::Distortion => {
                    b.drive = 10f32.powf(at(slot::DRIVE) / 20.0);
                    b.bias = at(slot::BIAS);
                    b.spread = at(slot::STEREO);
                    b.mix = at(slot::MIX);
                    b.steps = (2f32.powf(8.0 - at(slot::DRIVE) / 48.0 * 7.0)).max(1.0);
                }
                ModuleKind::Filter => {
                    b.biquad = Biquad::new(module.choice[0], at(slot::CUTOFF), at(slot::Q), at(slot::GAIN), sr);
                    b.stages = if module.choice[1] == 1 { 2 } else { 1 };
                }
                ModuleKind::NonlinearFilter => {
                    let g = (PI * at(slot::CUTOFF).clamp(10.0, sr * 0.45) / sr).tan();
                    let k = 1.0 / at(slot::Q).max(0.05);
                    let a1 = 1.0 / (1.0 + g * (g + k));
                    b.svf = (a1, g * a1, g * g * a1, k);
                    b.drive = 10f32.powf(at(slot::NL_DRIVE) / 20.0);
                }
                ModuleKind::Mix | ModuleKind::Aux => b.level = at(slot::LEVEL) * if module.flag { -1.0 } else { 1.0 },
                ModuleKind::Output | ModuleKind::CurveOutput => {
                    has_output = true;
                    b.level = at(slot::LEVEL);
                    b.pan = pan_gains(at(slot::PAN));
                    if module.kind == ModuleKind::Output {
                        b.envelope = WaveEnv { attack: at(slot::ATTACK), hold: at(slot::HOLD), decay: at(slot::DECAY), sustain: at(slot::SUSTAIN), release: at(slot::RELEASE) };
                    } else {
                        b.curve_speed = 1.0 / (at(slot::CURVE_LENGTH).max(0.001) * sr);
                        b.curve_start = at(slot::CURVE_START);
                        b.curve_end = at(slot::CURVE_END);
                    }
                    let send = module.choice[0] as usize;
                    b.lane = module.flag.then_some(match send {
                        s if s < MOD_LANES => s,
                        s if s == MOD_SEND_SIDEBAND as usize => SIDEBAND,
                        _ => usize::MAX,
                    });
                }
                _ => {}
            }
        }
        if !has_output {
            return false;
        }

        // Audio-rate connections with their (modulated) amounts.
        let mut connections = [(0usize, 0usize, AudioParam::Phase, 0f32); MOD_AUDIO];
        let mut count = 0;
        for (c, link) in p.audio.iter().enumerate() {
            if link.source == MOD_NO_MODULE || link.target == MOD_NO_MODULE || (link.source as usize) >= MOD_MODULES || (link.target as usize) >= MOD_MODULES {
                continue;
            }
            connections[count] = (link.source as usize, link.target as usize, link.param, m[MOD_AUDIO_AMOUNT + c]);
            count += 1;
        }
        let connections = &connections[..count];

        for i in 0..n {
            if voice.gate.tick() {
                Self::release(voice, &p);
            }
            let held = voice.gate.held;
            let mut sounding = false;
            let (mut l, mut r) = (0f32, 0f32);
            for k in 0..last {
                let module = &p.modules[k];
                if !module.on || module.kind == ModuleKind::Empty {
                    continue;
                }
                if module.kind == ModuleKind::Group {
                    l = 0.0;
                    r = 0.0;
                    continue;
                }
                let b = &blocks[k];
                // Audio-rate modulation into this module.
                let (mut phase, mut harmonic, mut shift, mut ring, mut pitch, mut input) = (0f32, 0f32, 0f32, 1f32, 0f32, 0f32);
                for &(source, target, param, amount) in connections {
                    if target != k {
                        continue;
                    }
                    let s = voice.outputs[source];
                    match param {
                        AudioParam::Phase => phase += amount * s,
                        AudioParam::Harmonic => harmonic += amount * s,
                        AudioParam::Shift => shift += amount * s * 1000.0,
                        AudioParam::Level => ring *= 1.0 + amount * (s - 1.0),
                        AudioParam::Pitch => pitch += amount * s * 24.0,
                        AudioParam::Input => input += amount * s,
                    }
                }
                // Frequency modulation scales every unison voice's increment alike.
                let mut scale = 1.0 + harmonic;
                if shift != 0.0 && b.freq.abs() > 1e-3 {
                    scale += shift / b.freq;
                }
                if pitch != 0.0 {
                    scale *= 2f32.powf(pitch / 12.0);
                }
                let state = &mut voice.modules[k];
                match module.kind {
                    ModuleKind::Analog | ModuleKind::Wavetable | ModuleKind::Sample => {
                        let (mut gl, mut gr) = (0f32, 0f32);
                        for u in 0..b.count {
                            let inc = b.incs[u] * scale;
                            match module.kind {
                                ModuleKind::Analog => {
                                    // Hard sync: the wave restarts each time the note's own cycle does.
                                    let read = (state.phases[u] + phase).rem_euclid(1.0);
                                    let value = analog(module.choice[0], read, (inc * b.sync).abs(), b.width);
                                    state.masters[u] += inc;
                                    state.phases[u] += inc * b.sync;
                                    if state.masters[u] >= 1.0 || state.masters[u] < 0.0 {
                                        state.masters[u] -= state.masters[u].floor();
                                        if b.sync > 1.0 {
                                            state.phases[u] = state.masters[u] * b.sync;
                                        }
                                    }
                                    state.phases[u] -= state.phases[u].floor();
                                    gl += value * b.gains[u].0;
                                    gr += value * b.gains[u].1;
                                }
                                ModuleKind::Wavetable => {
                                    let Some(table) = tables.get(k).filter(|t| t.frames() > 0) else { continue };
                                    let value = table.read(b.mip, b.frame, (state.phases[u] + phase).rem_euclid(1.0));
                                    state.phases[u] = (state.phases[u] + inc).rem_euclid(1.0);
                                    gl += value * b.gains[u].0;
                                    gr += value * b.gains[u].1;
                                }
                                _ => {
                                    let sample = &samples[k];
                                    let (a, c) = Self::sample_tick(sample, state, u, b, scale, phase, held);
                                    gl += a * b.gains[u].0;
                                    gr += c * b.gains[u].1;
                                }
                            }
                        }
                        let level = b.level * ring;
                        l += gl * level;
                        r += gr * level;
                        voice.outputs[k] = (gl + gr) * 0.5 * level;
                    }
                    ModuleKind::Granular => {
                        if b.count == 0 {
                            voice.outputs[k] = 0.0;
                            continue;
                        }
                        let sample = &samples[k];
                        let frames = sample.frames() as f64;
                        if state.warm {
                            // Warm start: a full set of grains, already under way.
                            state.warm = false;
                            let overlap = (b.grain_spawn * b.grain_length).clamp(1.0, (GRAINS / 2) as f32) as usize;
                            for g in 1..overlap {
                                let age = g as f32 / overlap as f32 * b.grain_length;
                                spawn_grain(&mut voice.grains, state, b, k, module.table, module.alt, frames, age);
                            }
                        }
                        state.spawn += b.grain_spawn;
                        while state.spawn >= 1.0 {
                            // Random timing delays the next grain by up to one period.
                            state.spawn -= 1.0 + b.random_timing * state.random();
                            spawn_grain(&mut voice.grains, state, b, k, module.table, module.alt, frames, 0.0);
                        }
                        let (mut gl, mut gr) = (0f32, 0f32);
                        for grain in voice.grains.iter_mut().filter(|g| g.module as usize == k) {
                            let x = grain.age / grain.length;
                            if x >= 1.0 || grain.position < 0.0 || grain.position >= frames {
                                grain.module = NO_GRAIN;
                                continue;
                            }
                            let env = grain_envelope(x, b.grain_attack, b.grain_decay, b.grain_power);
                            let (a, c) = sample.read(grain.position + phase as f64 * 64.0);
                            gl += a * env * grain.gains.0;
                            gr += c * env * grain.gains.1;
                            grain.position += grain.rate * scale as f64;
                            grain.age += 1.0;
                        }
                        let level = b.level * ring * b.grain_norm;
                        l += gl * level;
                        r += gr * level;
                        voice.outputs[k] = (gl + gr) * 0.5 * level;
                    }
                    ModuleKind::Noise => {
                        let (nl, nr) = Self::noise_tick(state, module.choice[0], b, scale);
                        let level = b.level * ring;
                        l += nl * level;
                        r += nr * level;
                        voice.outputs[k] = (nl + nr) * 0.5 * level;
                    }
                    ModuleKind::Distortion => {
                        let kind = module.choice[0];
                        let dist = |x: f32, bias: f32| shape(kind, (x + bias) * b.drive, b.steps) - shape(kind, bias * b.drive, b.steps);
                        let (dl, dr) = (dist(l, b.bias + b.spread), dist(r, b.bias - b.spread));
                        l = (l + (dl - l) * b.mix) * ring;
                        r = (r + (dr - r) * b.mix) * ring;
                        voice.outputs[k] = (l + r) * 0.5;
                    }
                    ModuleKind::Filter => {
                        for stage in 0..b.stages {
                            l = b.biquad.tick(&mut state.biquad[0][stage], l);
                            r = b.biquad.tick(&mut state.biquad[1][stage], r);
                        }
                        l *= ring;
                        r *= ring;
                        voice.outputs[k] = (l + r) * 0.5;
                    }
                    ModuleKind::NonlinearFilter => {
                        let (a1, a2, a3, damp) = b.svf;
                        let (kind, mode) = (module.choice[0], module.choice[1]);
                        // Clean has no saturation, so its drive cancels out; the others gain some level.
                        let makeup = if mode == 0 { 1.0 / b.drive } else { 1.0 / b.drive.sqrt().sqrt() };
                        let tick = |x: f32, z: &mut [f32; 2]| {
                            let x = x * b.drive;
                            let v3 = x - z[1];
                            let v1 = a1 * z[0] + a2 * v3;
                            let v2 = z[1] + a2 * z[0] + a3 * v3;
                            // Saturating the integrators bounds the resonance.
                            z[0] = saturate(mode, 2.0 * v1 - z[0]);
                            z[1] = saturate(mode, 2.0 * v2 - z[1]);
                            let y = match kind {
                                0 => v2,
                                1 => v1,
                                2 => x - damp * v1 - v2,
                                3 => x - damp * v1,
                                _ => x - 2.0 * damp * v1,
                            };
                            y * makeup
                        };
                        let [zl, zr] = &mut state.svf;
                        l = tick(l, zl) * ring;
                        r = tick(r, zr) * ring;
                        voice.outputs[k] = (l + r) * 0.5;
                    }
                    ModuleKind::Mix => {
                        l *= b.level * ring;
                        r *= b.level * ring;
                        voice.outputs[k] = (l + r) * 0.5;
                    }
                    ModuleKind::Aux => {
                        let a = input * b.level * ring;
                        l += a;
                        r += a;
                        voice.outputs[k] = a;
                    }
                    ModuleKind::Output | ModuleKind::CurveOutput => {
                        let env = if module.kind == ModuleKind::Output {
                            let env = state.env.tick(&b.envelope, dt);
                            if state.env.stage != Stage::Idle {
                                sounding = true;
                            }
                            env
                        } else {
                            let level = Self::curve_tick(state, &module.curve, module.choice[1], b, held, dt);
                            if !state.done {
                                sounding = true;
                            }
                            level
                        };
                        let g = env * b.level * ring * voice.velocity;
                        let (ol, or) = (l * g * b.pan.0 * unison_out.0, r * g * b.pan.1 * unison_out.1);
                        voice.outputs[k] = (ol + or) * 0.5;
                        match b.lane {
                            Some(lane) if lane < MOD_LANES => {
                                bus[lane][0][i] += ol;
                                bus[lane][1][i] += or;
                            }
                            Some(SIDEBAND) => {
                                sideband[0][i] += ol;
                                sideband[1][i] += or;
                            }
                            Some(_) => {
                                master[0][i] += ol;
                                master[1][i] += or;
                            }
                            None => {}
                        }
                    }
                    ModuleKind::Empty | ModuleKind::Group => {}
                }
            }
            if !sounding {
                voice.age += block;
                return false;
            }
        }
        voice.age += block;
        true
    }

    /// One frame of a sample generator's unison voice `u`, moving its play head
    /// through the loop in the module's loop mode.
    fn sample_tick(sample: &Sample, state: &mut ModuleState, u: usize, b: &Block, scale: f32, phase: f32, held: bool) -> (f32, f32) {
        let position = &mut state.positions[u];
        if *position < 0.0 {
            *position = b.start;
        }
        let offset = phase as f64 * 64.0;
        // Sustain loops only while the note is held, then plays on to the end.
        let looping = match b.loop_mode {
            1 | 3 | 4 => true,
            2 => held,
            _ => false,
        };
        let forward_loop = looping && matches!(b.loop_mode, 1 | 2);
        let fade_from = b.loop_end - b.xfade;
        let value = if forward_loop && b.xfade > 1.0 && *position >= fade_from && *position < b.loop_end {
            // Crossfade into the start of the loop; the jump then lands where the fade ends.
            let t = ((*position - fade_from) / b.xfade) as f32;
            let (a0, c0) = sample.read(*position + offset);
            let (a1, c1) = sample.read(b.loop_start + (*position - fade_from) + offset);
            (a0 + (a1 - a0) * t, c0 + (c1 - c0) * t)
        } else if forward_loop && *position + 1.0 >= b.loop_end {
            // The frame after the loop's last one is its first.
            let t = (*position - position.floor()) as f32;
            let (a0, c0) = sample.read(position.floor() + offset);
            let (a1, c1) = sample.read(b.loop_start + b.xfade + offset);
            (a0 + (a1 - a0) * t, c0 + (c1 - c0) * t)
        } else if looping {
            sample.read(position.min(b.loop_end - 1.0) + offset)
        } else {
            sample.read(*position + offset)
        };
        let step = b.rates[u] * scale as f64;
        let back = &mut state.back[u];
        *position += if *back { -step } else { step };
        if looping {
            let (start, end) = (b.loop_start, b.loop_end);
            match b.loop_mode {
                1 | 2 => {
                    if *position >= end {
                        *position = start + b.xfade + (*position - end);
                    }
                }
                3 => {
                    if !*back && *position >= end {
                        *position = (2.0 * end - *position).max(start);
                        *back = true;
                    } else if *back && *position < start {
                        *position = (2.0 * start - *position).min(end);
                        *back = false;
                    }
                }
                _ => {
                    // Reverse: once at the loop's end, play it backwards over and over.
                    if !*back && *position >= end {
                        *position = (2.0 * end - *position).max(start);
                        *back = true;
                    } else if *back && *position < start {
                        *position += end - start;
                    }
                }
            }
        }
        value
    }

    /// One frame of noise: coloured by the slope, or stepped/smooth at the note's frequency.
    fn noise_tick(state: &mut ModuleState, kind: u8, b: &Block, scale: f32) -> (f32, f32) {
        let white = state.random() * 2.0 - 1.0;
        let (a, c) = (state.random() * 2.0 - 1.0, state.random() * 2.0 - 1.0);
        let fresh = [white + (a - white) * b.stereo, white + (c - white) * b.stereo];
        if kind != 0 {
            // Keytracked: a new random value every cycle of the note.
            state.keyed_phase += b.keyed_inc * scale;
            if state.keyed_phase >= 1.0 {
                state.keyed_phase -= state.keyed_phase.floor();
                for (ch, value) in fresh.iter().enumerate() {
                    state.keyed[ch] = [state.keyed[ch][1], *value];
                }
            }
            let t = if kind == 1 { 1.0 } else { 0.5 - 0.5 * (PI * state.keyed_phase).cos() };
            let at = |ch: usize| state.keyed[ch][0] + (state.keyed[ch][1] - state.keyed[ch][0]) * t;
            return (at(0) * 1.4, at(1) * 1.4);
        }
        let mut out = [0f32; 2];
        for (ch, &w) in fresh.iter().enumerate() {
            // Paul Kellet's pink filter, a leaky integrator for brown, differences for blue and violet.
            let pk = &mut state.pink[ch];
            pk[0] = 0.99886 * pk[0] + w * 0.055_517_9;
            pk[1] = 0.99332 * pk[1] + w * 0.075_075_9;
            pk[2] = 0.969 * pk[2] + w * 0.153_852;
            pk[3] = 0.8665 * pk[3] + w * 0.310_485_6;
            pk[4] = 0.55 * pk[4] + w * 0.532_952_2;
            pk[5] = -0.7616 * pk[5] - w * 0.016_898;
            let pink = (pk[0] + pk[1] + pk[2] + pk[3] + pk[4] + pk[5] + pk[6] + w * 0.5362) * 0.32;
            pk[6] = w * 0.115_926;
            state.brown[ch] = (state.brown[ch] + 0.02 * w) / 1.02;
            let brown = state.brown[ch] * 10.0;
            let [previous_white, previous_pink] = state.previous[ch];
            let blue = (pink - previous_pink) * 3.4;
            let violet = (w - previous_white) * 0.7;
            state.previous[ch] = [w, pink];
            let [cw, cp, cb, cu, cv] = b.colors;
            out[ch] = w * cw + pink * cp + brown * cb + blue * cu + violet * cv;
        }
        (out[0], out[1])
    }

    /// The curve output's level now, moving along the curve in its loop mode.
    fn curve_tick(state: &mut ModuleState, curve: &crate::model::Shape, mode: u8, b: &Block, held: bool, dt: f32) -> f32 {
        if state.done {
            return 0.0;
        }
        let (start, end) = (b.curve_start.min(b.curve_end), b.curve_end.max(b.curve_start));
        let can_loop = end - start > 0.001;
        // Loop and ping pong fade out once the note ends; sustain plays on to the end.
        if !held && matches!(mode, 1 | 3) {
            state.fade -= dt / CURVE_FADE;
            if state.fade <= 0.0 {
                state.done = true;
                return 0.0;
            }
        }
        let level = curve.value_at(state.curve) * state.fade;
        let looping = can_loop && match mode {
            1 | 3 => true,
            2 => held,
            _ => false,
        };
        if state.curve_back {
            state.curve -= b.curve_speed;
            if state.curve <= start {
                state.curve = start + (start - state.curve);
                state.curve_back = false;
            }
        } else {
            state.curve += b.curve_speed;
            if looping && state.curve >= end {
                if mode == 3 {
                    state.curve = end - (state.curve - end);
                    state.curve_back = true;
                } else {
                    state.curve = start + (state.curve - end);
                }
            } else if state.curve >= 1.0 {
                state.curve = 1.0;
                state.done = true;
            }
        }
        level
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AudioMod, Device, Effect, Lane, ModTarget, Module, Modulator, UtilityParams};
    use crate::synth::HELD;

    pub(super) const SR: f32 = 48_000.0;

    pub(super) fn play(p: ModularParams, notes: &[u8], frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut synth = ModSynth::new(p, SR);
        for &n in notes {
            synth.note_on(n, 1.0, HELD);
        }
        let table = Arc::new(crate::wavetable::builtin(0));
        let tables: Vec<Arc<Wavetable>> = (0..MOD_MODULES).map(|_| table.clone()).collect();
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            synth.render(&[], &tables, a, b);
        }
        (l, r)
    }

    pub(super) fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    pub(super) fn frequency(signal: &[f32]) -> f32 {
        signal.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f32 * SR / signal.len() as f32
    }

    pub(super) fn sine() -> Module {
        let mut m = Module::new(ModuleKind::Analog);
        m.choice[0] = 3;
        m
    }

    /// A stack from a list of modules.
    fn stack(modules: &[Module]) -> ModularParams {
        let mut p = ModularParams::default();
        p.modules = [Module::EMPTY; MOD_MODULES];
        p.modules[..modules.len()].copy_from_slice(modules);
        p
    }

    pub(super) fn group() -> Module {
        Module::new(ModuleKind::Group)
    }

    pub(super) fn output() -> Module {
        Module::new(ModuleKind::Output)
    }

    #[test]
    fn the_default_stack_plays_a_saw() {
        let (l, _) = play(ModularParams::default(), &[69], 24_000);
        assert!((frequency(&l[2400..]) - 440.0).abs() < 3.0, "{}", frequency(&l[2400..]));
        assert!(rms(&l[2400..]) > 0.1);
    }

    #[test]
    fn nothing_sounds_without_an_output_module() {
        let (l, _) = play(stack(&[group(), sine()]), &[60], 4800);
        assert!(rms(&l) == 0.0);
    }

    #[test]
    fn generators_stack_and_groups_separate_signals() {
        let (one, _) = play(stack(&[group(), sine(), output()]), &[57], 24_000);
        let (two, _) = play(stack(&[group(), sine(), sine(), output()]), &[57], 24_000);
        assert!(rms(&two[2400..]) > rms(&one[2400..]) * 1.3, "generators add up");
        // A group header stops the signal: the first group has no output here.
        let (split, _) = play(stack(&[group(), sine(), group(), output()]), &[57], 24_000);
        assert!(rms(&split) < 1e-6, "nothing flows past a group header");
    }

    #[test]
    fn harmonic_shift_and_pitch_tune_a_generator() {
        let mut generator = sine();
        generator.set(slot::HARMONIC, 2.0);
        let (l, _) = play(stack(&[group(), generator, output()]), &[57], 48_000);
        assert!((frequency(&l[2400..]) - 440.0).abs() < 2.0, "harmonic ×2");
        let mut generator = sine();
        generator.set(slot::HARMONIC, 0.0);
        generator.set(slot::SHIFT, 100.0);
        let (l, _) = play(stack(&[group(), generator, output()]), &[57], 48_000);
        assert!((frequency(&l[2400..]) - 100.0).abs() < 2.0, "harmonic 0 keeps only the shift");
        let mut generator = sine();
        generator.set(slot::SEMI, 12.0);
        let (l, _) = play(stack(&[group(), generator, output()]), &[57], 48_000);
        assert!((frequency(&l[2400..]) - 440.0).abs() < 2.0);
    }

    #[test]
    fn effects_process_everything_above_them() {
        let mut filter = Module::new(ModuleKind::Filter);
        filter.set(slot::CUTOFF, 200.0);
        filter.choice[1] = 1;
        let saw = Module::new(ModuleKind::Analog);
        let (dry, _) = play(stack(&[group(), saw, output()]), &[48], 24_000);
        let (wet, _) = play(stack(&[group(), saw, filter, output()]), &[48], 24_000);
        let highs = |x: &[f32]| rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>());
        assert!(highs(&wet) < highs(&dry) * 0.3, "the low pass darkens");
        // A generator below the filter is not filtered.
        let (after, _) = play(stack(&[group(), saw, filter, saw, output()]), &[48], 24_000);
        assert!(highs(&after) > highs(&dry) * 0.8);

        let (clean, _) = play(stack(&[group(), sine(), output()]), &[48], 24_000);
        for kind in 0..6u8 {
            let mut distortion = Module::new(ModuleKind::Distortion);
            distortion.choice[0] = kind;
            let (out, _) = play(stack(&[group(), sine(), distortion, output()]), &[48], 24_000);
            let difference: f32 = out.iter().zip(&clean).map(|(a, b)| (a - b).abs()).sum::<f32>() / out.len() as f32;
            assert!(difference > 0.01 && out.iter().all(|s| s.is_finite() && s.abs() < 4.0), "type {kind}: {difference}");
        }
        for kind in 0..7u8 {
            let mut f = Module::new(ModuleKind::Filter);
            f.choice[0] = kind;
            f.set(slot::GAIN, 12.0);
            f.set(slot::Q, 5.0);
            let (out, _) = play(stack(&[group(), Module::new(ModuleKind::Analog), f, output()]), &[48], 9600);
            assert!(out.iter().all(|s| s.is_finite() && s.abs() < 8.0), "filter type {kind}");
        }
    }

    #[test]
    fn audio_rate_phase_modulation_from_another_group() {
        // Group 1: the carrier and its output. Group 2: the modulator, unheard.
        let mut p = stack(&[group(), sine(), output(), group(), sine()]);
        p.modules[4].set(slot::HARMONIC, 2.0);
        let (clean, _) = play(p, &[57], 24_000);
        p.audio[0] = AudioMod { source: 4, target: 1, param: AudioParam::Phase, amount: 0.5 };
        let (fm, _) = play(p, &[57], 24_000);
        let brightness = |x: &[f32]| rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>()) / rms(x);
        assert!(brightness(&fm[2400..]) > brightness(&clean[2400..]) * 1.8, "sidebands");
        assert!(rms(&fm[2400..]) > 0.1, "the modulator group stays silent but the carrier plays");
    }

    #[test]
    fn level_modulation_is_ring_modulation_and_aux_injects_audio() {
        let mut p = stack(&[group(), sine(), output(), group(), sine()]);
        p.modules[4].set(slot::HARMONIC, 3.0);
        p.audio[0] = AudioMod { source: 4, target: 1, param: AudioParam::Level, amount: 1.0 };
        let (ring, _) = play(p, &[57], 48_000);
        assert!(frequency(&ring[4800..]) > 300.0, "{}", frequency(&ring[4800..]));

        // An aux module in group 1 picks up the oscillator of group 2.
        let mut p = stack(&[group(), Module::new(ModuleKind::Aux), output(), group(), sine()]);
        let (silent, _) = play(p, &[57], 9600);
        p.audio[0] = AudioMod { source: 4, target: 1, param: AudioParam::Input, amount: 1.0 };
        let (fed, _) = play(p, &[57], 9600);
        assert!(rms(&silent) < 1e-6 && rms(&fed[2400..]) > 0.1);
    }

    #[test]
    fn outputs_send_to_lanes_or_to_nowhere() {
        let mut p = stack(&[group(), sine(), output()]);
        let (plain, _) = play(p, &[60], 9600);
        p.lanes[0].fx[0] = Some(Device::new(Effect::Utility(UtilityParams { gain_db: -12.0, ..Default::default() })));
        let (lane, _) = play(p, &[60], 9600);
        assert!((rms(&lane) / rms(&plain) - 0.251).abs() < 0.02, "lane 1 has the effect");
        p.modules[2].choice[0] = 3;
        let (direct, _) = play(p, &[60], 9600);
        assert!((rms(&direct) / rms(&plain) - 1.0).abs() < 0.02, "sent to the master, skipping the lanes");
        p.lanes[0].dest = 1;
        p.lanes[1] = Lane { gain: 0.5, ..Lane::EMPTY };
        p.modules[2].choice[0] = 0;
        let (chained, _) = play(p, &[60], 9600);
        assert!((rms(&chained) / rms(&plain) - 0.125).abs() < 0.015);
        p.modules[2].flag = false;
        let (muted, _) = play(p, &[60], 9600);
        assert!(rms(&muted) == 0.0, "an output switched off only modulates");
    }

    #[test]
    fn the_output_envelope_shapes_and_ends_the_note() {
        let mut out = output();
        out.set(slot::ATTACK, 0.1);
        let mut synth = ModSynth::new(stack(&[group(), sine(), out]), SR);
        synth.note_on(60, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; 9600], vec![0.0; 9600]);
        synth.render(&[], &[], &mut l, &mut r);
        assert!(rms(&l[..1200]) < rms(&l[4800..]) * 0.5, "attack ramps up");
        synth.note_off(60);
        let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        synth.render(&[], &[], &mut l, &mut r);
        assert!(!synth.voices.iter().any(|v| v.active));
    }

    #[test]
    fn unison_sync_and_noise() {
        let mut wide = Module::new(ModuleKind::Analog);
        wide.set(slot::UNISON, 6.0);
        wide.set(slot::PHASE_RANDOM, 180.0);
        let (l, r) = play(stack(&[group(), wide, output()]), &[57], 24_000);
        let side = rms(&l.iter().zip(&r).map(|(a, b)| a - b).collect::<Vec<_>>());
        assert!(side > 0.05, "spread unison is wide");

        let mut synced = Module::new(ModuleKind::Analog);
        synced.set(slot::A, 3.5);
        let (s, _) = play(stack(&[group(), synced, output()]), &[45], 48_000);
        assert!(s.iter().all(|x| x.is_finite()));
        let (plain, _) = play(stack(&[group(), Module::new(ModuleKind::Analog), output()]), &[45], 48_000);
        let difference: f32 = s.iter().zip(&plain).map(|(a, b)| (a - b).abs()).sum();
        assert!(difference > 100.0, "sync changes the wave");

        let (noise, _) = play(stack(&[group(), Module::new(ModuleKind::Noise), output()]), &[60], 9600);
        assert!(rms(&noise[2400..]) > 0.1);
    }

    #[test]
    fn modulators_reach_module_settings() {
        let mut p = stack(&[group(), sine(), output()]);
        let mut lfo = Modulator::new(ModulatorKind::Lfo);
        lfo.shape = crate::model::Shape::from_points(&[(0.0, 1.0, 0.0), (0.5, 1.0, 0.0), (0.501, 0.0, 0.0), (1.0, 0.0, 0.0)]);
        lfo.mode = LfoMode::Retrig;
        lfo.a = (1.0f32 / 0.02).ln() / 2000f32.ln();
        lfo.targets[0] = ModTarget::new(ModularParams::module_param(1, slot::LEVEL), -0.5);
        p.modulators[0] = lfo;
        let (l, _) = play(p, &[60], 48_000);
        assert!(rms(&l[4800..19_200]) < 0.01 && rms(&l[28_800..43_200]) > 0.1, "{} {}", rms(&l[4800..19_200]), rms(&l[28_800..43_200]));
    }

    #[test]
    fn mono_glide_and_voice_stealing() {
        let mut p = stack(&[group(), sine(), output()]);
        p.mono = true;
        p.glide = 100.0;
        let mut synth = ModSynth::new(p, SR);
        synth.note_on(57, 1.0, HELD);
        synth.note_on(69, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        synth.render(&[], &[], &mut l, &mut r);
        assert_eq!(synth.voices.iter().filter(|v| v.active).count(), 1);
        assert!((frequency(&l[24_000..]) - 440.0).abs() < 3.0);
        let mut poly = ModSynth::new(stack(&[group(), sine(), output()]), SR);
        for k in 0..20 {
            poly.note_on(40 + k, 1.0, HELD);
        }
        assert_eq!(poly.voices.iter().filter(|v| v.active).count(), ModularParams::default().polyphony as usize);
    }

    // ----- Phase 2: generators, nonlinear filter, curve output, global unison -----

    use crate::model::{Shape, UnisonMode, MOD_UNISON};

    /// Renders with audio files: `samples[k]` is the file of module `k`.
    fn play_files(p: ModularParams, files: &[(usize, Arc<Sample>)], notes: &[u8], frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut samples: Vec<Arc<Sample>> = (0..MOD_MODULES).map(|_| Arc::new(Sample::from_channels(vec![], vec![], SR))).collect();
        for (k, s) in files {
            samples[*k] = s.clone();
        }
        let mut synth = ModSynth::new(p, SR);
        for &n in notes {
            synth.note_on(n, 1.0, HELD);
        }
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            synth.render(&samples, &[], a, b);
        }
        (l, r)
    }

    /// A 0..1 ramp over `frames` frames: the value says where the play head is.
    fn ramp(frames: usize) -> Arc<Sample> {
        let data: Vec<f32> = (0..frames).map(|i| i as f32 / frames as f32).collect();
        Arc::new(Sample::from_channels(data.clone(), data, SR))
    }

    fn sine_file(freq: f32, seconds: f32) -> Arc<Sample> {
        let data: Vec<f32> = (0..(SR * seconds) as usize).map(|i| (TAU * freq * i as f32 / SR).sin()).collect();
        Arc::new(Sample::from_channels(data.clone(), data, SR))
    }

    /// Amplitude of `freq` in `x` (Goertzel).
    fn tone(x: &[f32], freq: f32) -> f32 {
        let w = TAU * freq / SR;
        let (mut s1, mut s2) = (0f32, 0f32);
        for &v in x {
            let s = v + 2.0 * w.cos() * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        (s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2).max(0.0).sqrt() * 2.0 / x.len() as f32
    }

    /// Like [`tone`], averaged over short windows: grains start at unrelated phases,
    /// so a long window would let them cancel.
    fn grain_tone(x: &[f32], freq: f32) -> f32 {
        let windows: Vec<f32> = x.chunks_exact(2400).map(|w| tone(w, freq)).collect();
        windows.iter().sum::<f32>() / windows.len().max(1) as f32
    }

    fn highs(x: &[f32]) -> f32 {
        rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>()) / rms(x).max(1e-9)
    }

    fn sample_module(mode: u8, start: f32, length: f32, xfade: f32) -> Module {
        let mut s = Module::new(ModuleKind::Sample);
        s.choice[0] = mode;
        s.set(slot::LOOP_START, start);
        s.set(slot::LOOP_LENGTH, length);
        s.set(slot::XFADE, xfade);
        s
    }

    #[test]
    fn sample_loop_modes() {
        let file = ramp(4800);
        let run = |module: Module| play_files(stack(&[group(), module, output()]), &[(1, file.clone())], &[60], 24_000).0;
        let gain = 0.35;
        // Off: plays once.
        let off = run(sample_module(0, 0.0, 1.0, 0.0));
        assert!(rms(&off[200..4700]) > 0.1 && rms(&off[5000..]) == 0.0);
        // Infinite: loops the second half forever.
        let infinite = run(sample_module(1, 0.5, 1.0, 0.0));
        let tail = &infinite[9600..];
        assert!(tail.iter().all(|&v| v >= 0.49 * gain && v <= 1.01 * gain), "only the loop plays");
        // Ping pong goes back down after the loop's end; reverse too, then jumps up.
        for mode in [3, 4] {
            let out = run(sample_module(mode, 0.5, 1.0, 0.0));
            let falling = out[5000..7000].windows(2).filter(|w| w[1] < w[0]).count();
            assert!(falling > 1900, "mode {mode}: {falling}");
            assert!(out[9600..].iter().all(|&v| v >= 0.49 * gain && v <= 1.01 * gain));
        }
        let pp = run(sample_module(3, 0.5, 1.0, 0.0));
        assert!(pp[7600..9000].windows(2).filter(|w| w[1] > w[0]).count() > 1300, "ping pong turns around at the loop start");
        // The crossfade removes the jump at the loop point.
        let jump = |x: &[f32]| x[9600..].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
        let hard = run(sample_module(1, 0.0, 1.0, 0.0));
        let smooth = run(sample_module(1, 0.0, 1.0, 0.25));
        assert!(jump(&hard) > 0.3 && jump(&smooth) < 0.01, "{} {}", jump(&hard), jump(&smooth));
    }

    #[test]
    fn sample_sustain_loop_plays_out_after_the_release() {
        let mut out = output();
        out.set(slot::RELEASE, 10.0);
        let p = stack(&[group(), sample_module(2, 0.25, 0.25, 0.0), out]);
        let samples: Vec<Arc<Sample>> = (0..MOD_MODULES).map(|k| if k == 1 { ramp(4800) } else { Arc::new(Sample::from_channels(vec![], vec![], SR)) }).collect();
        let mut synth = ModSynth::new(p, SR);
        synth.note_on(60, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
        synth.render(&samples, &[], &mut l, &mut r);
        assert!(l[9600..].iter().all(|&v| v < 0.51 * 0.35), "held: loops 25..50 %");
        synth.note_off(60);
        let (mut l, mut r) = (vec![0.0; 9600], vec![0.0; 9600]);
        synth.render(&samples, &[], &mut l, &mut r);
        assert!(l[..4800].iter().any(|&v| v > 0.9 * 0.35), "released: plays on to the end");
        assert!(rms(&l[6000..]) == 0.0);
    }

    fn granular(settings: &[(usize, f32)]) -> Module {
        let mut g = Module::new(ModuleKind::Granular);
        g.set(slot::ROOT, 57.0);
        for &(s, v) in settings {
            g.set(s, v);
        }
        g
    }

    #[test]
    fn granular_plays_grains_at_the_note() {
        let file = sine_file(220.0, 2.0);
        let run = |g: Module, note: u8| play_files(stack(&[group(), g, output()]), &[(1, file.clone())], &[note], 48_000).0;
        let plain = run(granular(&[(slot::POSITION, 0.3)]), 57);
        assert!(rms(&plain[4800..]) > 0.05);
        assert!(grain_tone(&plain[4800..], 220.0) > grain_tone(&plain[4800..], 330.0) * 5.0, "the root plays the file as it is");
        let up = run(granular(&[(slot::POSITION, 0.3)]), 69);
        assert!(grain_tone(&up[4800..], 440.0) > grain_tone(&up[4800..], 220.0) * 5.0, "an octave up");
        assert!(plain.iter().all(|s| s.is_finite() && s.abs() < 1.5));
        // A major chord adds the third and the fifth.
        let mut chord = granular(&[(slot::POSITION, 0.3), (slot::CHORD_RANGE, 1.0)]);
        chord.choice[1] = 3;
        let chord = run(chord, 57);
        let third = 220.0 * 2f32.powf(4.0 / 12.0);
        assert!(grain_tone(&chord[4800..], third) > grain_tone(&plain[4800..], third) * 4.0);
        // Randomness and reverse keep it sounding.
        let wild = run(
            granular(&[(slot::RANDOM_POSITION, 1.0), (slot::RANDOM_PITCH, 12.0), (slot::RANDOM_PAN, 1.0), (slot::RANDOM_REVERSE, 1.0), (slot::RANDOM_TIMING, 1.0), (slot::POSITION, 0.5)]),
            57,
        );
        assert!(rms(&wild[4800..]) > 0.02 && wild.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn granular_spawn_modes_and_warm_start() {
        let file = sine_file(220.0, 2.0);
        let synth_grains = |g: Module| {
            let samples: Vec<Arc<Sample>> = (0..MOD_MODULES).map(|k| if k == 1 { file.clone() } else { Arc::new(Sample::from_channels(vec![], vec![], SR)) }).collect();
            let mut synth = ModSynth::new(stack(&[group(), g, output()]), SR);
            synth.set_tempo(120.0);
            synth.note_on(57, 1.0, HELD);
            let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
            synth.render(&samples, &[], &mut l, &mut r);
            (synth.voices[0].grains.iter().filter(|g| g.module == 1).count(), l)
        };
        // Density: as many grains at once as asked, whatever their length.
        let mut dense = granular(&[(slot::DENSITY, 6.0), (slot::LENGTH, 200.0)]);
        dense.choice[0] = 2;
        let (count, _) = synth_grains(dense);
        assert!((5..=7).contains(&count), "{count}");
        // Synced 1/16 at 120 BPM: 8 grains a second, 100 ms long → about one at a time.
        let mut synced = granular(&[(slot::SYNC, 5.0), (slot::LENGTH, 100.0)]);
        synced.choice[0] = 1;
        let (count, _) = synth_grains(synced);
        assert!(count <= 2, "{count}");
        // Warm start: full from the first moment.
        let mut warm = granular(&[(slot::LENGTH, 400.0), (slot::RATE, 20.0)]);
        let (_, cold) = synth_grains(warm);
        warm.flag = true;
        let (_, hot) = synth_grains(warm);
        assert!(rms(&hot[..2400]) > rms(&cold[..2400]) * 2.0, "{} {}", rms(&hot[..2400]), rms(&cold[..2400]));
    }

    fn noise(slope: f32) -> Module {
        let mut n = Module::new(ModuleKind::Noise);
        n.set(slot::SLOPE, slope);
        n
    }

    #[test]
    fn noise_slopes_keytracking_and_seed() {
        let (white, _) = play(stack(&[group(), noise(0.0), output()]), &[60], 24_000);
        let mut brightness = vec![];
        for slope in [-6.0, -4.5, -3.0, -1.5, 0.0, 1.5, 3.0, 4.5, 6.0] {
            let (out, _) = play(stack(&[group(), noise(slope), output()]), &[60], 24_000);
            let level = rms(&out[4800..]) / rms(&white[4800..]);
            assert!((0.4..2.5).contains(&level), "slope {slope}: level {level}");
            brightness.push(highs(&out[4800..]));
        }
        assert!(brightness.windows(2).all(|w| w[1] > w[0]), "{brightness:?}");
        // Keytracked noise changes value once per cycle of the note.
        let mut stepped = noise(0.0);
        stepped.choice[0] = 1;
        let (low, _) = play(stack(&[group(), stepped, output()]), &[36], 24_000);
        let (high, _) = play(stack(&[group(), stepped, output()]), &[84], 24_000);
        assert!(highs(&low[4800..]) < highs(&high[4800..]) * 0.3);
        stepped.choice[0] = 2;
        let (smooth, _) = play(stack(&[group(), stepped, output()]), &[84], 24_000);
        assert!(highs(&smooth[4800..]) < highs(&high[4800..]));
        // Stable seed: the same noise on every note; random: different each time.
        let twice = |seed: u8| {
            let mut n = noise(0.0);
            n.choice[1] = seed;
            let mut synth = ModSynth::new(stack(&[group(), n, output()]), SR);
            let take = |synth: &mut ModSynth| {
                synth.note_on(60, 1.0, HELD);
                let (mut l, mut r) = (vec![0.0; 2400], vec![0.0; 2400]);
                synth.render(&[], &[], &mut l, &mut r);
                synth.all_notes_off();
                let (mut a, mut b) = (vec![0.0; 48_000], vec![0.0; 48_000]);
                synth.render(&[], &[], &mut a, &mut b);
                l
            };
            (take(&mut synth), take(&mut synth))
        };
        let (a, b) = twice(1);
        assert_eq!(a, b);
        let (a, b) = twice(0);
        assert_ne!(a, b);
    }

    #[test]
    fn wavetable_bandlimit_removes_highs() {
        let open = Module::new(ModuleKind::Wavetable);
        let mut closed = open;
        closed.set(slot::BANDLIMIT, 400.0);
        // A saw: frame 0 of "Saw → Pulse".
        let saw = Arc::new(crate::wavetable::builtin(1));
        let tables: Vec<Arc<Wavetable>> = (0..MOD_MODULES).map(|_| saw.clone()).collect();
        let render = |module: Module| {
            let mut synth = ModSynth::new(stack(&[group(), module, output()]), SR);
            synth.note_on(45, 1.0, HELD);
            let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
            synth.render(&[], &tables, &mut l, &mut r);
            l
        };
        let (bright, dark) = (render(open), render(closed));
        assert!(highs(&dark[2400..]) < highs(&bright[2400..]) * 0.5, "{} {}", highs(&dark[2400..]), highs(&bright[2400..]));
        assert!(rms(&dark[2400..]) > 0.05);
    }

    #[test]
    fn nonlinear_filter_filters_saturates_and_stays_bounded() {
        let saw = Module::new(ModuleKind::Analog);
        let (dry, _) = play(stack(&[group(), saw, output()]), &[45], 24_000);
        let mut f = Module::new(ModuleKind::NonlinearFilter);
        f.set(slot::CUTOFF, 300.0);
        f.set(slot::NL_DRIVE, 0.0);
        let (lp, _) = play(stack(&[group(), saw, f, output()]), &[45], 24_000);
        assert!(highs(&lp[2400..]) < highs(&dry[2400..]) * 0.5, "low pass");
        // Clean has no saturation: drive changes nothing.
        f.choice[1] = 0;
        let (clean, _) = play(stack(&[group(), saw, f, output()]), &[45], 24_000);
        f.set(slot::NL_DRIVE, 24.0);
        let (clean_driven, _) = play(stack(&[group(), saw, f, output()]), &[45], 24_000);
        let diff = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / a.len() as f32;
        assert!(diff(&clean, &clean_driven) < 1e-3);
        f.choice[1] = 1;
        let (warm, _) = play(stack(&[group(), saw, f, output()]), &[45], 24_000);
        assert!(diff(&clean, &warm) > 0.01, "the other modes distort");
        for kind in 0..5u8 {
            for mode in 0..5u8 {
                let mut f = Module::new(ModuleKind::NonlinearFilter);
                f.choice = [kind, mode];
                f.set(slot::Q, 25.0);
                f.set(slot::NL_DRIVE, 36.0);
                let (out, _) = play(stack(&[group(), saw, f, output()]), &[45], 9600);
                assert!(out.iter().all(|s| s.is_finite() && s.abs() < 8.0), "type {kind} mode {mode}");
            }
        }
    }

    #[test]
    fn curve_output_follows_its_curve() {
        let curve_out = |mode: u8, length: f32| {
            let mut c = Module::new(ModuleKind::CurveOutput);
            c.curve = Shape::from_points(&[(0.0, 1.0, 0.0), (1.0, 0.0, 0.0)]);
            c.choice[1] = mode;
            c.set(slot::CURVE_LENGTH, length);
            c
        };
        // One shot: a falling line over 0.5 s, then the voice ends.
        let mut synth = ModSynth::new(stack(&[group(), sine(), curve_out(0, 0.5)]), SR);
        synth.note_on(60, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        synth.render(&[], &[], &mut l, &mut r);
        assert!(rms(&l[..4800]) > rms(&l[14_400..19_200]) * 1.5);
        assert!(rms(&l[24_500..]) == 0.0 && !synth.voices.iter().any(|v| v.active));
        // Loop: keeps going while held, fades out quickly after.
        let mut synth = ModSynth::new(stack(&[group(), sine(), curve_out(1, 0.25)]), SR);
        synth.note_on(60, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        synth.render(&[], &[], &mut l, &mut r);
        assert!(rms(&l[36_000..]) > 0.05, "still looping");
        synth.note_off(60);
        let (mut l, mut r) = (vec![0.0; 4800], vec![0.0; 4800]);
        synth.render(&[], &[], &mut l, &mut r);
        assert!(!synth.voices.iter().any(|v| v.active));
        // Ping pong is continuous: no jumps in level at the turns.
        let mut synth = ModSynth::new(stack(&[group(), sine(), curve_out(3, 0.1)]), SR);
        synth.note_on(60, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        synth.render(&[], &[], &mut l, &mut r);
        assert!(rms(&l[40_000..]) > 0.05);
    }

    fn with_unison(mut p: ModularParams, voices: u8, mode: UnisonMode, settings: [f32; 4]) -> ModularParams {
        p.unison.voices = voices;
        p.unison.mode = mode;
        p.unison.detune = settings[0];
        p.unison.spread = settings[1];
        p.unison.blend = settings[2];
        p.unison.bias = settings[3];
        p
    }

    #[test]
    fn global_unison_detunes_and_widens_the_whole_stack() {
        let base = stack(&[group(), Module::new(ModuleKind::Analog), output()]);
        let (one, _) = play(base, &[57], 24_000);
        let (l, r) = play(with_unison(base, 5, UnisonMode::Smooth, [0.4, 1.0, 0.5, 0.0]), &[57], 24_000);
        let side = rms(&l.iter().zip(&r).map(|(a, b)| a - b).collect::<Vec<_>>());
        assert!(side > 0.05, "wide");
        let level = rms(&l[2400..]) / rms(&one[2400..]);
        assert!((0.5..2.0).contains(&level), "about the same level: {level}");
        // Hard starts every voice in phase (they add up); synthetic spreads phases evenly
        // (two sines half a cycle apart cancel).
        let sines = stack(&[group(), sine(), output()]);
        let (hard, _) = play(with_unison(sines, 4, UnisonMode::Hard, [0.0, 0.0, 0.5, 0.0]), &[57], 9600);
        let (smooth, _) = play(with_unison(sines, 4, UnisonMode::Smooth, [0.0, 0.0, 0.5, 0.0]), &[57], 9600);
        let (synthetic, _) = play(with_unison(sines, 2, UnisonMode::Synthetic, [0.0, 0.0, 0.5, 0.0]), &[57], 9600);
        assert!(rms(&hard) > rms(&smooth) * 1.2, "{} {}", rms(&hard), rms(&smooth));
        assert!(rms(&synthetic) < 0.01, "{}", rms(&synthetic));
        // Voices per note come out of the same pool.
        let mut synth = ModSynth::new(with_unison(sines, 4, UnisonMode::Smooth, [0.2, 0.5, 0.5, 0.0]), SR);
        for k in 0..20 {
            synth.note_on(40 + k, 1.0, HELD);
        }
        assert_eq!(synth.voices.iter().filter(|v| v.active).count(), VOICES);
    }

    #[test]
    fn unison_stacks_shepard_and_chords() {
        let sines = stack(&[group(), sine(), output()]);
        let (pitch, _) = play(with_unison(sines, 2, UnisonMode::PitchStack, [0.0, 0.0, 0.5, 0.0]), &[57], 24_000);
        assert!(tone(&pitch[2400..], 440.0) > 0.1 && tone(&pitch[2400..], 220.0) > 0.1, "an octave apart");
        let (freq, _) = play(with_unison(sines, 3, UnisonMode::FrequencyStack, [0.0, 0.0, 0.5, 0.0]), &[57], 24_000);
        assert!(tone(&freq[2400..], 660.0) > 0.08, "×1, ×2, ×3");
        let mut chords = with_unison(sines, 3, UnisonMode::Chords, [0.0, 0.0, 0.5, 0.0]);
        chords.unison.chord = 4;
        let (minor, _) = play(chords, &[57], 24_000);
        assert!(tone(&minor[2400..], 220.0 * 2f32.powf(3.0 / 12.0)) > 0.08, "minor third");
        assert!(tone(&minor[2400..], 220.0 * 2f32.powf(4.0 / 12.0)) < 0.02);
        // Shepard: every C sounds the same, whichever octave is played.
        let shepard = with_unison(sines, 6, UnisonMode::Shepard, [0.0, 0.0, 0.5, 0.0]);
        let (c3, _) = play(shepard, &[48], 24_000);
        let (c5, _) = play(shepard, &[72], 24_000);
        for f in [65.4, 130.8, 261.6, 523.3, 1046.5] {
            let (a, b) = (tone(&c3[2400..], f), tone(&c5[2400..], f));
            assert!((a - b).abs() < 0.01 + 0.1 * a.max(b), "{f} Hz: {a} vs {b}");
        }
        // Its center can be modulated: unison settings are parameters like any other.
        let mut moving = shepard;
        let mut lfo = Modulator::new(ModulatorKind::Lfo);
        lfo.targets[0] = ModTarget::new(MOD_UNISON + 3, 0.5);
        moving.modulators[0] = lfo;
        let (out, _) = play(moving, &[48], 24_000);
        assert!(out.iter().all(|s| s.is_finite()) && rms(&out) > 0.05);
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    use crate::model::{AudioMod, Module};
    use crate::synth::HELD;

    #[test]
    #[ignore = "benchmark: prints how much of real time big patches take"]
    fn cost() {
        let run = |groups: usize, unison: f32, voices: usize| {
            let mut p = ModularParams::default();
            p.modules = [Module::EMPTY; MOD_MODULES];
            let mut i = 0;
            for g in 0..groups {
                p.modules[i] = Module::new(ModuleKind::Group);
                let mut osc = Module::new(ModuleKind::Analog);
                osc.set(slot::UNISON, unison);
                p.modules[i + 1] = osc;
                p.modules[i + 2] = osc;
                p.modules[i + 3] = Module::new(ModuleKind::Filter);
                p.modules[i + 4] = Module::new(ModuleKind::Distortion);
                p.modules[i + 5] = Module::new(ModuleKind::Output);
                p.audio[g] = AudioMod { source: (i + 2) as u8, target: (i + 1) as u8, param: AudioParam::Phase, amount: 0.3 };
                i += 6;
            }
            let mut synth = ModSynth::new(p, 48_000.0);
            for n in 0..voices as u8 {
                synth.note_on(40 + n * 2, 1.0, HELD);
            }
            let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
            let start = std::time::Instant::now();
            for _ in 0..(48_000 / 512) {
                synth.render(&[], &[], &mut l, &mut r);
            }
            start.elapsed().as_secs_f64() * 100.0
        };
        println!("2 grupos × 2 osc (unísono 4) + filtro + dist, 8 voces: {:.0} % de tiempo real", run(2, 4.0, 8));
        println!("5 grupos × 2 osc (unísono 8) + filtro + dist, 12 voces: {:.0} % de tiempo real", run(5, 8.0, 12));

        // Global unison: the whole stack per unison voice.
        let mut p = ModularParams::default();
        p.modules = [Module::EMPTY; MOD_MODULES];
        for (i, kind) in [ModuleKind::Group, ModuleKind::Analog, ModuleKind::Analog, ModuleKind::NonlinearFilter, ModuleKind::Output].into_iter().enumerate() {
            p.modules[i] = Module::new(kind);
        }
        p.unison.voices = 8;
        let mut synth = ModSynth::new(p, 48_000.0);
        for n in 0..4u8 {
            synth.note_on(48 + n * 4, 1.0, HELD);
        }
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        let start = std::time::Instant::now();
        for _ in 0..(48_000 / 512) {
            synth.render(&[], &[], &mut l, &mut r);
        }
        println!("unísono global 8 × 4 notas (2 osc + filtro no lineal): {:.0} % de tiempo real", start.elapsed().as_secs_f64() * 100.0);

        // Granular: dense clouds.
        let data: Vec<f32> = (0..96_000).map(|i| (i as f32 * 0.03).sin()).collect();
        let file = Arc::new(Sample::from_channels(data.clone(), data, 48_000.0));
        let samples: Vec<Arc<Sample>> = (0..MOD_MODULES).map(|_| file.clone()).collect();
        let mut p = ModularParams::default();
        let mut g = Module::new(ModuleKind::Granular);
        g.choice[0] = 2;
        g.set(slot::DENSITY, 16.0);
        p.modules[1] = g;
        let mut synth = ModSynth::new(p, 48_000.0);
        for n in 0..8u8 {
            synth.note_on(48 + n * 3, 1.0, HELD);
        }
        let start = std::time::Instant::now();
        for _ in 0..(48_000 / 512) {
            synth.render(&samples, &[], &mut l, &mut r);
        }
        println!("granular, 16 granos × 8 notas: {:.0} % de tiempo real", start.elapsed().as_secs_f64() * 100.0);

        // 32 modulators of every kind, each with 4 targets, on 12 notes; one tracks pitch.
        use crate::model::{ModTarget, Modulator, ModulatorKind};
        let mut p = ModularParams::default();
        for i in 0..MOD_MODULATORS {
            let mut m = Modulator::new(ModulatorKind::ALL[i % ModulatorKind::ALL.len()]);
            m.source = ((i + 1) % MOD_MODULATORS) as u8;
            for k in 0..4 {
                m.targets[k] = ModTarget::new(ModularParams::module_param(1, k), 0.1);
            }
            p.modulators[i] = m;
        }
        let mut synth = ModSynth::new(p, 48_000.0);
        for n in 0..12u8 {
            synth.note_on(48 + n * 2, 1.0, HELD);
        }
        let start = std::time::Instant::now();
        for _ in 0..(48_000 / 512) {
            synth.render(&[], &[], &mut l, &mut r);
        }
        println!("32 moduladores × 4 destinos, 12 notas: {:.0} % de tiempo real", start.elapsed().as_secs_f64() * 100.0);

        // Poly lanes: a saturator, a chorus and a reverb per voice, an LFO on each.
        use crate::model::{Device, Effect};
        let mut p = ModularParams::default();
        for (slot, effect) in [Effect::Saturator(Default::default()), Effect::Chorus(Default::default()), Effect::Reverb(Default::default())].into_iter().enumerate() {
            p.lanes[0].fx[slot] = Some(Device::new(effect));
        }
        p.lanes[0].poly = true;
        let mut lfo = Modulator::new(ModulatorKind::Lfo);
        for k in 0..3 {
            lfo.targets[k] = ModTarget::new(ModularParams::lane_fx_param(0, k, 0), 0.2);
        }
        p.modulators[0] = lfo;
        let start = std::time::Instant::now();
        let mut synth = ModSynth::new(p, 48_000.0);
        println!("crear 32 voces × 3 efectos: {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
        for n in 0..12u8 {
            synth.note_on(48 + n * 2, 1.0, HELD);
        }
        let start = std::time::Instant::now();
        for _ in 0..(48_000 / 512) {
            synth.render(&[], &[], &mut l, &mut r);
        }
        println!("carril poly (saturador + chorus + reverb), 12 notas: {:.0} % de tiempo real", start.elapsed().as_secs_f64() * 100.0);
    }
}

#[cfg(test)]
mod modulation_tests {
    use super::*;
    use crate::model::{ModTarget, Module, OutputRange, Shape, MOD_NO_TARGET};
    use crate::synth::HELD;

    const SR: f32 = 48_000.0;
    /// Seconds per control block.
    const BLOCK: f32 = CONTROL as f32 / SR;

    /// A sine on the master, plus `modulators`.
    pub(super) fn patch(modulators: &[Modulator]) -> ModularParams {
        let mut p = ModularParams::default();
        p.modules = [Module::EMPTY; MOD_MODULES];
        let mut sine = Module::new(ModuleKind::Analog);
        sine.choice[0] = 3;
        p.modules[0] = Module::new(ModuleKind::Group);
        p.modules[1] = sine;
        p.modules[2] = Module::new(ModuleKind::Output);
        p.modules[2].set(slot::SUSTAIN, 1.0);
        p.modules[2].set(slot::RELEASE, 0.01);
        p.modulators[..modulators.len()].copy_from_slice(modulators);
        p
    }

    /// Renders `blocks` control blocks; `each` runs before every block with its index.
    pub(super) fn run(p: ModularParams, blocks: usize, mut each: impl FnMut(&mut ModSynth, usize)) -> (ModSynth, Vec<f32>) {
        let mut synth = ModSynth::new(p, SR);
        let table = Arc::new(crate::wavetable::builtin(0));
        let tables: Vec<Arc<Wavetable>> = (0..MOD_MODULES + MOD_MODULATORS).map(|_| table.clone()).collect();
        let mut out = Vec::new();
        let (mut l, mut r) = ([0.0; CONTROL], [0.0; CONTROL]);
        for b in 0..blocks {
            each(&mut synth, b);
            l.fill(0.0);
            r.fill(0.0);
            synth.render(&[], &tables, &mut l, &mut r);
            out.extend_from_slice(&l);
        }
        (synth, out)
    }

    /// Modulator `k`'s output on voice 0, block by block.
    fn trace(p: ModularParams, k: usize, blocks: usize, mut each: impl FnMut(&mut ModSynth, usize)) -> Vec<f32> {
        let mut values = Vec::new();
        run(p, blocks, |synth, b| {
            each(synth, b);
            if b > 0 {
                values.push(synth.voices[0].mods[k].out);
            }
        });
        values.push(0.0);
        values.pop();
        values
    }

    fn note_at_start(synth: &mut ModSynth, b: usize) {
        if b == 0 {
            synth.note_on(60, 1.0, HELD);
        }
    }

    pub(super) fn blocks(seconds: f32) -> usize {
        (seconds / BLOCK) as usize
    }

    fn envelope_mod(attack: f32, decay: f32, sustain: f32, release: f32) -> Modulator {
        let mut env = Modulator::new(ModulatorKind::Envelope);
        env.set(mslot::ATTACK, attack);
        env.set(mslot::DECAY, decay);
        env.set(mslot::SUSTAIN, sustain);
        env.set(mslot::RELEASE, release);
        env
    }

    fn lfo(hz: f32, shape: Shape) -> Modulator {
        let mut lfo = Modulator::new(ModulatorKind::Lfo);
        lfo.mode = LfoMode::Retrig;
        lfo.shape = shape;
        lfo.set(mslot::RATE, hz);
        lfo
    }

    fn square() -> Shape {
        Shape::from_points(&[(0.0, 1.0, 0.0), (0.5, 1.0, 0.0), (0.501, 0.0, 0.0), (1.0, 0.0, 0.0)])
    }

    #[test]
    fn envelopes_wait_hold_and_retrigger_from_zero_or_seamlessly() {
        let mut env = envelope_mod(0.001, 0.05, 0.0, 0.05);
        env.set(mslot::DELAY, 0.1);
        env.set(mslot::HOLD, 0.2);
        let v = trace(patch(&[env]), 0, blocks(0.5), note_at_start);
        assert!(v[blocks(0.09)] < 0.01, "waits for the delay: {}", v[blocks(0.09)]);
        assert!(v[blocks(0.15)] > 0.99 && v[blocks(0.28)] > 0.99, "attacks after it and holds: {} {}", v[blocks(0.15)], v[blocks(0.28)]);
        assert!(v[blocks(0.45)] < 0.2, "then decays: {}", v[blocks(0.45)]);

        // Mono retrigger halfway through a slow attack.
        for seamless in [false, true] {
            let mut env = envelope_mod(0.2, 1.0, 1.0, 0.1);
            env.seamless = seamless;
            let mut p = patch(&[env]);
            p.mono = true;
            let v = trace(p, 0, blocks(0.2), |s, b| {
                if b == 0 {
                    s.note_on(60, 1.0, HELD);
                }
                if b == blocks(0.1) {
                    s.note_on(62, 1.0, HELD);
                }
            });
            let after = v[blocks(0.1) + 1];
            if seamless {
                assert!(after > 0.4, "seamless keeps going from where it was: {after}");
            } else {
                assert!(after < 0.05, "starts again from zero: {after}");
            }
        }
    }

    #[test]
    fn output_range_depth_and_connection_bounds() {
        let target = ModTarget { curve: 0.0, offset: 0.2, ..ModTarget::new(0, 0.5) };
        assert_eq!(target.bounds(OutputRange::Unipolar), (0.2, 0.7));
        assert_eq!(target.bounds(OutputRange::Bipolar), (-0.3, 0.7));
        let off = ModTarget { on: false, ..target };
        assert_eq!(off.amount(1.0), 0.0, "a switched off connection does nothing");
        let bent = ModTarget { curve: 1.0, ..ModTarget::new(0, 1.0) };
        assert!(bent.amount(0.25) > 0.6, "a positive curve rises early: {}", bent.amount(0.25));
        let bent = ModTarget { curve: -1.0, ..ModTarget::new(0, 1.0) };
        assert!(bent.amount(0.5) < 0.1 && bent.amount(-0.5) > -0.1, "and a negative one late, symmetrically: {}", bent.amount(0.5));

        let mut env = envelope_mod(0.0005, 1.0, 1.0, 0.1);
        env.set(mslot::DEPTH, 0.5);
        let v = trace(patch(&[env]), 0, blocks(0.1), note_at_start);
        assert!((v[blocks(0.05)] - 0.5).abs() < 0.01, "depth scales the output: {}", v[blocks(0.05)]);
        env.set_range(OutputRange::Inverted);
        env.set(mslot::DEPTH, 1.0);
        let v = trace(patch(&[env]), 0, blocks(0.1), note_at_start);
        assert!(v[blocks(0.05)].abs() < 0.01, "inverted: {}", v[blocks(0.05)]);
        let mut half = envelope_mod(0.0005, 0.001, 0.5, 0.1);
        half.set_range(OutputRange::Bipolar);
        let v = trace(patch(&[half]), 0, blocks(0.1), note_at_start);
        assert!(v[blocks(0.05)].abs() < 0.01, "bipolar: half way is the middle: {}", v[blocks(0.05)]);
    }

    fn random(rate: f32, smooth: f32, jitter: f32, chaos: f32) -> Modulator {
        let mut r = Modulator::new(ModulatorKind::Random);
        r.set(mslot::RATE, rate);
        r.set(mslot::SMOOTH, smooth);
        r.set(mslot::JITTER, jitter);
        r.set(mslot::CHAOS, chaos);
        r
    }

    fn steps(v: &[f32]) -> Vec<f32> {
        let mut values: Vec<f32> = v.windows(2).filter(|w| (w[1] - w[0]).abs() > 1e-6).map(|w| w[1]).collect();
        values.dedup();
        values
    }

    #[test]
    fn random_chaos_smoothing_jitter_and_shared_sequences() {
        let length = blocks(1.0);
        let wild = trace(patch(&[random(20.0, 0.0, 0.0, 1.0)]), 0, length, note_at_start);
        let calm = trace(patch(&[random(20.0, 0.0, 0.0, 0.1)]), 0, length, note_at_start);
        let frozen = trace(patch(&[random(20.0, 0.0, 0.0, 0.0)]), 0, length, note_at_start);
        let moves = |v: &[f32]| v.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(steps(&wild).len() >= 15, "about 20 steps a second: {}", steps(&wild).len());
        assert!(moves(&calm) < 0.12 && moves(&wild) > 0.3, "low chaos takes small steps: {} vs {}", moves(&calm), moves(&wild));
        assert!(steps(&frozen).is_empty(), "no chaos: only triggers change it");

        let smooth = trace(patch(&[random(20.0, 1.0, 0.0, 1.0)]), 0, length, note_at_start);
        assert!(moves(&smooth) < moves(&wild) * 0.4, "smoothing glides: {} vs {}", moves(&smooth), moves(&wild));

        // Jitter: uneven step lengths.
        let lengths = |v: &[f32]| {
            let changes: Vec<usize> = (1..v.len()).filter(|&i| (v[i] - v[i - 1]).abs() > 1e-6).collect();
            let gaps: Vec<f32> = changes.windows(2).map(|w| (w[1] - w[0]) as f32).collect();
            let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
            (gaps.iter().map(|g| (g - mean) * (g - mean)).sum::<f32>() / gaps.len() as f32).sqrt() / mean
        };
        let even = lengths(&wild);
        let jittery = lengths(&trace(patch(&[random(20.0, 0.0, 1.0, 1.0)]), 0, length, note_at_start));
        assert!(jittery > even + 0.2, "jitter varies the steps: {even} → {jittery}");

        // Two notes: each voice has its own sequence unless shared.
        for shared in [false, true] {
            let mut r = random(20.0, 0.0, 0.0, 1.0);
            r.shared = shared;
            let mut pairs = Vec::new();
            run(patch(&[r]), length, |s, b| {
                if b == 0 {
                    s.note_on(60, 1.0, HELD);
                    s.note_on(64, 1.0, HELD);
                }
                if b > 5 {
                    pairs.push((s.voices[0].mods[0].out, s.voices[1].mods[0].out));
                }
            });
            let same = pairs.iter().all(|(a, b)| a == b);
            assert_eq!(same, shared, "shared {shared}");
        }
    }

    #[test]
    fn trigger_modes_and_trigger_inputs() {
        // Never: notes do not start it.
        let mut env = envelope_mod(0.001, 1.0, 1.0, 0.1);
        env.trigger = TriggerMode::Never;
        let v = trace(patch(&[env]), 0, blocks(0.1), note_at_start);
        assert!(v.iter().all(|&x| x == 0.0));

        // Legato (poly): a note played over a held one carries on its envelope.
        let mut env = envelope_mod(0.2, 1.0, 1.0, 0.1);
        env.trigger = TriggerMode::Legato;
        let mut second = Vec::new();
        run(patch(&[env]), blocks(0.25), |s, b| {
            if b == 0 {
                s.note_on(60, 1.0, HELD);
            }
            if b == blocks(0.1) {
                s.note_on(64, 1.0, HELD);
            }
            if b == blocks(0.1) + 2 {
                second.push((s.voices[0].mods[0].out, s.voices[1].mods[0].out));
            }
        });
        let (first, other) = second[0];
        assert!(other > 0.4 && (first - other).abs() < 0.05, "legato continues: {first} {other}");

        // Always (the default in poly) starts the second note from zero.
        let env = envelope_mod(0.2, 1.0, 1.0, 0.1);
        let mut second = Vec::new();
        run(patch(&[env]), blocks(0.25), |s, b| {
            if b == 0 {
                s.note_on(60, 1.0, HELD);
            }
            if b == blocks(0.1) {
                s.note_on(64, 1.0, HELD);
            }
            if b == blocks(0.1) + 2 {
                second.push(s.voices[1].mods[0].out);
            }
        });
        assert!(second[0] < 0.05, "{}", second[0]);

        // A 4 Hz square retriggers a short envelope: four peaks a second, released in between.
        let gate = lfo(4.0, square());
        let mut env = envelope_mod(0.001, 1.0, 1.0, 0.02);
        env.trigger_source = 0;
        let v = trace(patch(&[gate, env]), 1, blocks(1.0), note_at_start);
        let rises = v.windows(2).filter(|w| w[0] < 0.5 && w[1] >= 0.5).count();
        assert!((3..=5).contains(&rises), "{rises} retriggers");
    }

    #[test]
    fn chained_modulators_reshape_clamp_scale_and_sample() {
        let ramp = |hz: f32| lfo(hz, Shape::from_points(&[(0.0, 0.0, 0.0), (1.0, 1.0, 0.0)]));
        let chained = |kind: ModulatorKind, setting: f32| {
            let mut m = Modulator::new(kind);
            m.source = 0;
            if kind != ModulatorKind::Remap {
                m.set(0, setting);
            }
            m
        };
        let mut remap = chained(ModulatorKind::Remap, 0.0);
        remap.shape = Shape::falling();
        let pairs = |second: Modulator| {
            let mut out = Vec::new();
            run(patch(&[ramp(2.0), second]), blocks(0.5), |s, b| {
                note_at_start(s, b);
                if b > 2 {
                    out.push((s.voices[0].mods[0].out, s.voices[0].mods[1].out));
                }
            });
            out
        };
        assert!(pairs(remap).iter().all(|(a, b)| (a + b - 1.0).abs() < 1e-4), "remapped through a falling line");
        assert!(pairs(chained(ModulatorKind::LowerLimit, 0.3)).iter().all(|&(a, b)| b == a.max(0.3)));
        assert!(pairs(chained(ModulatorKind::UpperLimit, 0.3)).iter().all(|&(a, b)| b == a.min(0.3)));
        let mut scale = chained(ModulatorKind::Scale, -0.5);
        scale.index = 3;
        assert!(pairs(scale).iter().all(|&(a, b)| (b + 5.0 * a).abs() < 1e-4), "×(-0.5)×10");

        // Sample & hold: a fast ramp sampled by a 4 Hz square holds four values a second.
        let mut sh = chained(ModulatorKind::SampleHold, 0.5);
        sh.trigger_source = 2;
        let v = trace(patch(&[ramp(13.0), sh, lfo(4.0, square())]), 1, blocks(1.0), note_at_start);
        let held = steps(&v);
        assert!((3..=5).contains(&held.len()), "held values: {held:?}");
    }

    #[test]
    fn modulators_modulate_each_others_settings() {
        // An envelope opens the depth of a fast LFO (a delayed vibrato), and a
        // modulator listed later reaches an earlier one's settings one block late.
        let mut vibrato = lfo(6.0, Shape::from_points(&[(0.0, 0.0, 0.0), (0.5, 1.0, 0.0), (1.0, 0.0, 0.0)]));
        vibrato.set(mslot::DEPTH, 0.0);
        let mut env = envelope_mod(0.5, 1.0, 1.0, 0.1);
        env.targets[0] = ModTarget::new(ModularParams::modulator_param(0, mslot::DEPTH), 1.0);
        let v = trace(patch(&[vibrato, env]), 0, blocks(1.0), note_at_start);
        let peak = |from: f32, to: f32| v[blocks(from)..blocks(to)].iter().fold(0.0f32, |a, &b| a.max(b));
        assert!(peak(0.0, 0.17) < 0.35 && peak(0.6, 0.99) > 0.95, "{} {}", peak(0.0, 0.17), peak(0.6, 0.99));
    }

    #[test]
    fn lfo_table_and_timed_curves() {
        let mut table = Modulator::new(ModulatorKind::LfoTable);
        table.mode = LfoMode::Retrig;
        table.set(mslot::RATE, 5.0);
        let v = trace(patch(&[table]), 0, blocks(1.0), note_at_start);
        let crossings = v.windows(2).filter(|w| w[0] < 0.5 && w[1] >= 0.5).count();
        assert!((4..=6).contains(&crossings), "5 cycles of the table's first frame: {crossings}");
        let range = |v: &[f32]| v.iter().fold(f32::MAX, |a, &b| a.min(b))..v.iter().fold(f32::MIN, |a, &b| a.max(b));
        assert!(range(&v).start < 0.1 && range(&v).end > 0.9);

        // A curve rising over 0.5 s, played once or looped over its second half.
        let mut curve = Modulator::new(ModulatorKind::Timeline);
        curve.shape = Shape::from_points(&[(0.0, 0.0, 0.0), (1.0, 1.0, 0.0)]);
        curve.set(mslot::LENGTH, 0.5);
        let once = trace(patch(&[curve]), 0, blocks(1.0), note_at_start);
        assert!((once[blocks(0.25)] - 0.5).abs() < 0.03 && once[blocks(0.9)] == 1.0, "{} {}", once[blocks(0.25)], once[blocks(0.9)]);
        curve.loop_mode = 1;
        curve.set(mslot::LOOP_START, 0.5);
        let looped = trace(patch(&[curve]), 0, blocks(1.0), note_at_start);
        assert!(looped[blocks(0.3)..].iter().all(|&x| (0.49..=1.0).contains(&x)), "stays inside the loop");
        assert!(looped[blocks(0.3)..].windows(2).any(|w| w[1] < w[0] - 0.3), "and jumps back");
    }

    #[test]
    fn followers_trackers_and_controllers() {
        // The sine at A4 on the master: its level and its pitch.
        let mut follower = Modulator::new(ModulatorKind::AudioFollower);
        follower.index = 3;
        follower.set(mslot::FOLLOW_RELEASE, 0.05);
        let mut tracker = Modulator::new(ModulatorKind::PitchTracker);
        tracker.index = 3;
        let mut values = Vec::new();
        run(patch(&[follower, tracker]), blocks(1.0), |s, b| {
            if b == 0 {
                s.note_on(69, 1.0, HELD);
            }
            if b == blocks(0.6) {
                s.note_off(69);
            }
            values.push((s.globals[0], s.globals[1], s.voices.iter().any(|v| v.active)));
        });
        let (level, pitch, _) = values[blocks(0.5)];
        assert!(level > 0.2, "follows the level: {level}");
        let expected = (69.0 - 36.0) / (84.0 - 36.0);
        assert!((pitch - expected).abs() < 0.01, "tracks A4: {pitch} vs {expected}");
        assert!(values.last().unwrap().0 < 0.01, "and the level falls after the note: {:?}", values.last());

        // MIDI controllers.
        let mut cc = Modulator::new(ModulatorKind::MidiCc);
        cc.index = 74;
        let wheel = Modulator::new(ModulatorKind::PitchWheel);
        let pressure = Modulator::new(ModulatorKind::Pressure);
        let gate = Modulator::new(ModulatorKind::NoteGate);
        let mut seen = Vec::new();
        let (_, audio) = run(patch(&[cc, wheel, pressure, gate]), blocks(0.5), |s, b| {
            if b == 0 {
                s.note_on(69, 1.0, HELD);
                s.control(Controller::Cc(74), 0, 0.7);
                s.control(Controller::PitchBend, 0, 1.0);
                s.control(Controller::PolyPressure, 69, 0.4);
            }
            if b == blocks(0.4) {
                s.note_off(69);
            }
            seen.push(s.voices[0].mods.map(|m| m.out));
        });
        let held = seen[blocks(0.2)];
        assert_eq!((held[0], held[1], held[2], held[3]), (0.7, 1.0, 0.4, 1.0));
        assert_eq!(seen.last().unwrap()[3], 0.0, "the note gate closes");
        // Bent up a whole tone: A4 becomes B4.
        let crossings = audio[blocks(0.1) * CONTROL..blocks(0.3) * CONTROL].windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f32 / 0.2;
        assert!((crossings - 493.9).abs() < 8.0, "{crossings} Hz");
    }

    #[test]
    fn thirty_two_modulators_and_patches_with_sixteen() {
        let mut p = patch(&[]);
        let mut env = envelope_mod(0.001, 1.0, 1.0, 0.1);
        env.targets[0] = ModTarget::new(ModularParams::module_param(1, slot::LEVEL), -1.0);
        p.modulators[31] = env;
        let (_, quiet) = run(p, blocks(0.2), note_at_start);
        assert!(quiet[blocks(0.1) * CONTROL..].iter().all(|x| x.abs() < 1e-3), "the 32nd modulator works");

        let index = ModularParams::modulator_param(20, mslot::DEPTH);
        assert_eq!(ModularParams::param_modulator(index), Some((20, mslot::DEPTH)));
        assert_eq!(ModularParams::param_modulator(ModularParams::modulator_param(3, 2)), Some((3, 2)));
        *p.param_mut(index).unwrap() = 0.25;
        assert_eq!(p.modulators[20].value(mslot::DEPTH), 0.25);

        // A patch from the first layout: 16 modulators without the new fields.
        let mut old = serde_json::to_value(patch(&[lfo(1.0, square())])).unwrap();
        let list = old["modulators"].as_array_mut().unwrap();
        list.truncate(16);
        for m in list.iter_mut() {
            let m = m.as_object_mut().unwrap();
            for key in ["more", "inverted", "trigger", "trigger_source", "source", "shared", "seamless", "loop_mode"] {
                m.remove(key);
            }
            for t in m["targets"].as_array_mut().unwrap() {
                let t = t.as_object_mut().unwrap();
                t.remove("on");
                t.remove("curve");
                t.remove("offset");
            }
        }
        let loaded: ModularParams = serde_json::from_value(old).unwrap();
        assert_eq!(loaded.modulators[0].kind, ModulatorKind::Lfo);
        assert_eq!(loaded.modulators[0].value(mslot::DEPTH), 1.0);
        assert_eq!(loaded.modulators[0].targets[0].dest, MOD_NO_TARGET);
        assert!(loaded.modulators[0].targets[0].on);
        assert_eq!(loaded.modulators[31].kind, ModulatorKind::Off);
        assert_eq!(loaded.modulators[0].trigger_source, MOD_NO_MODULATOR);
    }

    /// High-frequency energy of `x` in windows of `size` samples.
    fn brightness(x: &[f32], size: usize) -> Vec<f32> {
        x.chunks(size).map(|c| (c.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum::<f32>() / c.len() as f32).sqrt()).collect()
    }

    #[test]
    fn modulation_presets_move_their_filters() {
        let preset = |name: &str| ModularParams::presets().into_iter().find(|(n, _)| *n == name).unwrap().1;
        // At 120 BPM: the wobble opens every eighth note (6000 samples).
        let (_, audio) = run(preset("Mod · Wobble Bass"), blocks(2.0), |s, b| {
            if b == 0 {
                s.note_on(36, 1.0, HELD);
            }
        });
        let b = brightness(&audio[24_000..], 1500);
        let (low, high) = (b.iter().fold(f32::MAX, |a, &x| a.min(x)), b.iter().fold(0.0f32, |a, &x| a.max(x)));
        assert!(high > low * 3.0, "wobbles: {low} .. {high}");
        // Sample & hold: a different filter setting every sixteenth (6000 samples), flat in between.
        let (_, audio) = run(preset("Mod · Sample & Hold Filter"), blocks(2.0), |s, b| {
            if b == 0 {
                s.note_on(48, 1.0, HELD);
            }
        });
        let b = brightness(&audio[24_000..72_000], 3000);
        let across = b.windows(2).skip(1).step_by(2).filter(|w| (w[1] / w[0] - 1.0).abs() > 0.05).count();
        let within = b.chunks(2).filter(|w| (w[1] / w[0] - 1.0).abs() > 0.05).count();
        assert!(across >= 4 && within <= 2, "steps: {b:?}");
    }

    #[test]
    fn harmonic_arpeggio_bouncing_curve_and_feedback_presets() {
        use super::tests::{frequency, rms};
        let preset = |name: &str| ModularParams::presets().into_iter().find(|(n, _)| *n == name).unwrap().1;
        // One held key climbs the harmonics 1, 2, 3, 4, a sixteenth (6000 samples) each.
        let mut p = preset("Mod · Arpegio armónico");
        p.lanes[0].fx[0] = None;
        let (_, audio) = run(p, blocks(0.6), |s, b| {
            if b == 0 {
                s.note_on(48, 1.0, HELD);
            }
        });
        let root = crate::wavesynth::note_freq(48.0);
        for step in 0..4 {
            let at = step * 6000 + 300;
            let f = frequency(&audio[at..at + 3000]);
            let harmonic = (step + 1) as f32;
            assert!((f / (root * harmonic) - 1.0).abs() < 0.06, "step {step}: {f} Hz");
        }
        // The drawn level falls to nothing and hits again, twice.
        let mut p = preset("Curve · Pelota que rebota");
        p.lanes[0].fx[0] = None;
        let (_, audio) = run(p, blocks(1.3), |s, b| {
            if b == 0 {
                s.note_on(60, 1.0, 4800);
            }
        });
        let level = |t: f32| rms(&audio[(t * SR) as usize..(t * SR) as usize + 480]);
        for (gap, hit) in [(0.285, 0.315), (0.515, 0.542)] {
            assert!(level(hit) > level(gap) * 3.0, "bounce at {hit} s: {} after {}", level(hit), level(gap));
        }
        // Full feedback on a high note stays bounded.
        let mut p = preset("Aux · Realimentación");
        p.macros[0] = 1.0;
        let (_, audio) = run(p, blocks(1.0), |s, b| {
            if b == 0 {
                s.note_on(96, 1.0, HELD);
            }
        });
        assert!(audio.iter().all(|x| x.is_finite() && x.abs() < 1.0), "peak {}", audio.iter().fold(0f32, |m, x| m.max(x.abs())));
    }

    #[test]
    fn removing_a_modulator_disconnects_it() {
        let mut p = patch(&[]);
        p.modulators[0] = Modulator::new(ModulatorKind::Lfo);
        let mut remap = Modulator::new(ModulatorKind::Remap);
        remap.source = 0;
        remap.trigger_source = 0;
        p.modulators[1] = remap;
        let mut env = Modulator::new(ModulatorKind::Envelope);
        env.targets[0] = ModTarget::new(ModularParams::modulator_param(0, mslot::DEPTH), 0.5);
        env.targets[1] = ModTarget::new(ModularParams::module_param(1, slot::LEVEL), 0.5);
        p.modulators[2] = env;
        p.remove_modulator(0);
        assert_eq!((p.modulators[1].source, p.modulators[1].trigger_source), (MOD_NO_MODULATOR, MOD_NO_MODULATOR));
        assert_eq!(p.modulators[2].targets[0], ModTarget::NONE);
        assert_eq!(p.modulators[2].targets[1].dest as usize, ModularParams::module_param(1, slot::LEVEL));
    }
}

#[cfg(test)]
mod lane_tests {
    use super::*;
    use super::modulation_tests::{blocks, patch, run};
    use super::tests::{frequency, group, output, play, rms, sine, SR};
    use crate::model::{Device, Effect, ModTarget, UtilityParams, MOD_NO_TARGET};
    use crate::synth::HELD;

    fn utility(gain_db: f32) -> Option<Device> {
        Some(Device::new(Effect::Utility(UtilityParams { gain_db, ..Default::default() })))
    }

    fn saturator() -> Option<Device> {
        Some(Device::new(Effect::Saturator(crate::model::SaturatorParams { drive_db: 30.0, output_db: -12.0, mix: 1.0, ..Default::default() })))
    }

    #[test]
    fn poly_lanes_distort_each_voice_on_its_own() {
        let mut p = patch(&[]);
        p.lanes[0].fx[0] = saturator();
        let render = |p: ModularParams, notes: &[u8]| play(p, notes, 9600).0;
        let error = |p: ModularParams| {
            let (chord, a, b) = (render(p, &[57, 64]), render(p, &[57]), render(p, &[64]));
            let sum: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
            let diff: Vec<f32> = chord.iter().zip(&sum).map(|(x, y)| x - y).collect();
            rms(&diff) / rms(&chord)
        };
        // Shared: the two notes distort together (intermodulation).
        assert!(error(p) > 0.1, "{}", error(p));
        p.lanes[0].poly = true;
        // Poly: the chord is just the two notes, each distorted alone.
        assert!(error(p) < 1e-4, "{}", error(p));
    }

    #[test]
    fn only_lanes_from_the_first_on_can_be_poly() {
        let mut p = ModularParams::default();
        p.lanes[1].poly = true;
        assert!(!p.lane_poly(1), "lane 2 needs lane 1 poly too");
        p.lanes[0].poly = true;
        assert!(p.lane_poly(0) && p.lane_poly(1) && !p.lane_poly(2));
    }

    #[test]
    fn poly_lane_tails_ring_after_the_note() {
        let mut p = patch(&[]);
        let delay = crate::model::DelayParams { time_beats: 0.5, feedback: 0.5, mix: 0.5, ping_pong: false, filter_hz: 12_000.0 };
        p.lanes[0].fx[0] = Some(Device::new(Effect::Delay(delay)));
        p.lanes[0].poly = true;
        let mut active = Vec::new();
        let (_, audio) = run(p, blocks(6.0), |s, b| {
            if b == 0 {
                s.note_on(69, 1.0, HELD);
            }
            if b == blocks(0.1) {
                s.note_off(69);
            }
            active.push(s.voices.iter().filter(|v| v.active).count());
        });
        // The echo at 120 BPM comes a quarter of a second later, after the note.
        let echo = &audio[blocks(0.3) * CONTROL..blocks(0.4) * CONTROL];
        assert!(rms(echo) > 0.01, "the echo rings in the voice's own delay: {}", rms(echo));
        assert_eq!(active[blocks(0.3)], 1, "the voice stays for its tail");
        assert_eq!(*active.last().unwrap(), 0, "and ends when the tail dies away");
    }

    #[test]
    fn mute_solo_and_mix() {
        let mut p = patch(&[]);
        let level = |p: ModularParams| rms(&play(p, &[60], 9600).0[2400..]);
        let plain = level(p);
        p.lanes[0].fx[0] = utility(-12.0);
        assert!((level(p) / plain - 0.251).abs() < 0.02);
        p.lanes[0].mix = 0.0;
        assert!((level(p) / plain - 1.0).abs() < 0.02, "mix 0: dry");
        p.lanes[0].mix = 0.5;
        assert!((level(p) / plain - 0.626).abs() < 0.02, "mix 50 %: half way");
        p.lanes[0].mute = true;
        assert_eq!(level(p), 0.0, "muted");
        p.lanes[0].mute = false;
        p.lanes[1].solo = true;
        assert_eq!(level(p), 0.0, "another lane soloed");
        p.lanes[0].solo = true;
        assert!(level(p) > 0.0, "soloed too");
        // The same in a poly lane.
        p.lanes[0].poly = true;
        p.lanes[1].poly = true;
        assert!((level(p) / plain - 0.626).abs() < 0.02);
        p.lanes[0].mute = true;
        assert_eq!(level(p), 0.0, "a muted poly lane");
    }

    #[test]
    fn modulators_move_lane_effect_settings() {
        // Velocity turns a lane's utility gain from -24 dB up to 0 dB.
        let mut velocity = Modulator::new(ModulatorKind::Velocity);
        let gain = ModularParams::lane_fx_param(0, 0, 0);
        velocity.targets[0] = ModTarget::new(gain, 0.5);
        let mut p = patch(&[velocity]);
        p.lanes[0].fx[0] = utility(-24.0);
        assert_eq!(p.param_info(gain).map(|i| i.name), Some("Gain"));
        let level = |p: ModularParams, velocity: f32| {
            let mut synth = ModSynth::new(p, SR);
            synth.note_on(60, velocity, HELD);
            let (mut l, mut r) = (vec![0.0; 9600], vec![0.0; 9600]);
            synth.render(&[], &[], &mut l, &mut r);
            rms(&l[2400..]) / velocity
        };
        for poly in [false, true] {
            p.lanes[0].poly = poly;
            let (soft, hard) = (level(p, 0.1), level(p, 1.0));
            assert!(hard > soft * 4.0, "poly {poly}: {soft} {hard}");
        }
    }

    #[test]
    fn moving_and_removing_lane_effects_keeps_their_modulation() {
        let mut lfo = Modulator::new(ModulatorKind::Lfo);
        lfo.targets[0] = ModTarget::new(ModularParams::lane_fx_param(0, 0, 1), 0.3);
        lfo.targets[1] = ModTarget::new(ModularParams::lane_fx_param(0, 1, 0), 0.4);
        lfo.targets[2] = ModTarget::new(ModularParams::lane_fx_param(1, 0, 0), 0.5);
        let mut p = patch(&[lfo]);
        p.lanes[0].fx[0] = utility(-6.0);
        p.lanes[0].fx[1] = saturator();
        p.lanes[1].fx[0] = utility(-3.0);
        p.move_lane_fx(0, 1, true);
        assert_eq!(p.lanes[0].fx[0], saturator());
        let dests = |p: &ModularParams| [0, 1, 2].map(|k| p.modulators[0].targets[k].dest);
        let at = |l, s, k| ModularParams::lane_fx_param(l, s, k) as u16;
        assert_eq!(dests(&p), [at(0, 1, 1), at(0, 0, 0), at(1, 0, 0)], "the modulation follows its effect");
        p.remove_lane_fx(0, 0);
        assert_eq!(p.lanes[0].fx[0], utility(-6.0), "the gap closes");
        assert_eq!(dests(&p), [at(0, 0, 1), MOD_NO_TARGET, at(1, 0, 0)], "the removed effect's modulation goes with it");
    }

    #[test]
    fn outputs_can_feed_the_sideband() {
        // The first output goes to the sideband, unheard; a follower listening to it
        // opens a second, silent-by-default output.
        let mut follower = Modulator::new(ModulatorKind::AudioFollower);
        follower.index = (MOD_LANES + 1) as u8;
        follower.targets[0] = ModTarget::new(ModularParams::module_param(5, slot::LEVEL), 1.0);
        let mut p = patch(&[follower]);
        p.modules[2].choice[0] = MOD_SEND_SIDEBAND;
        p.modules[3] = group();
        p.modules[4] = sine();
        p.modules[5] = output();
        p.modules[5].set(slot::LEVEL, 0.0);
        let (l, _) = play(p, &[69], 9600);
        assert!(rms(&l[4800..]) > 0.05, "the follower hears the sideband: {}", rms(&l[4800..]));
        p.modulators[0].kind = ModulatorKind::Off;
        let (l, _) = play(p, &[69], 9600);
        assert!(rms(&l) < 1e-6, "and the sideband itself is silent");
    }

    #[test]
    fn polyphony_bend_range_and_master_pitch() {
        let mut p = ModularParams::default();
        p.polyphony = 3;
        let mut synth = ModSynth::new(p, SR);
        for n in 60..66 {
            synth.note_on(n, 1.0, HELD);
        }
        assert_eq!(synth.voices.iter().filter(|v| v.active).count(), 3);

        let mut p = patch(&[]);
        p.pitch = 12.0;
        let (l, _) = play(p, &[57], 24_000);
        assert!((frequency(&l[2400..]) - 440.0).abs() < 3.0, "an octave up: {}", frequency(&l[2400..]));
        p.pitch = 0.0;
        p.fine = 100.0;
        let (l, _) = play(p, &[69], 24_000);
        assert!((frequency(&l[2400..]) - 466.2).abs() < 3.0, "a semitone of fine tuning: {}", frequency(&l[2400..]));
        p.fine = 0.0;
        p.bend_range = 12;
        let (_, audio) = run(p, blocks(0.4), |s, b| {
            if b == 0 {
                s.note_on(57, 1.0, HELD);
                s.control(Controller::PitchBend, 0, 1.0);
            }
        });
        let f = frequency(&audio[blocks(0.1) * CONTROL..]);
        assert!((f - 440.0).abs() < 5.0, "bent up an octave: {f}");
    }

    #[test]
    fn the_sideband_pump_preset_ducks_on_every_beat() {
        let (_, preset) = ModularParams::presets().into_iter().find(|(name, _)| name.contains("Sideband Pump")).unwrap();
        // The level just after each beat (every half second) against the level before the next.
        let dip = |p: ModularParams| {
            let (l, _) = play(p, &[60, 64, 67], 144_000);
            let at = |from: f32, to: f32| rms(&l[(from * SR) as usize..(to * SR) as usize]);
            let ducked: f32 = (2..6).map(|k| at(k as f32 * 0.5 + 0.03, k as f32 * 0.5 + 0.12)).sum();
            let open: f32 = (2..6).map(|k| at(k as f32 * 0.5 + 0.35, k as f32 * 0.5 + 0.48)).sum();
            ducked / open
        };
        assert!(dip(preset) < 0.6, "ducks on the beat: {}", dip(preset));
        let mut flat = preset;
        flat.modulators[0].kind = ModulatorKind::Off;
        assert!(dip(flat) > 0.85, "steady without the follower: {}", dip(flat));
    }

    #[test]
    fn patches_with_four_lane_effects_still_load() {
        let mut p = ModularParams::default();
        p.lanes[0].fx[0] = utility(-6.0);
        let json = serde_json::to_value(p).unwrap();
        let mut old = json.clone();
        for lane in old["lanes"].as_array_mut().unwrap() {
            let lane = lane.as_object_mut().unwrap();
            lane["fx"].as_array_mut().unwrap().truncate(4);
            for key in ["mute", "solo", "poly", "mix"] {
                lane.remove(key);
            }
        }
        for key in ["polyphony", "bend_range", "pitch", "fine"] {
            old.as_object_mut().unwrap().remove(key);
        }
        let loaded: ModularParams = serde_json::from_value(old).unwrap();
        assert_eq!(loaded, p);
    }
}
