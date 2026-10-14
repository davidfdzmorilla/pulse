//! Modular synthesizer built as a stack of modules, read top to bottom inside
//! groups (see `ModularParams`):
//!
//! - a Group starts a new signal; nothing flows past a group header;
//! - generators add their sound to the signal from above;
//! - effects (Distortion, Filter) process the signal so far;
//! - Mix passes the signal on scaled (and exposes it as a modulation source);
//! - Aux injects whatever audio-rate connections route into it;
//! - Output modules shape the signal with their envelope and send it to an
//!   effect lane or the master (or nowhere, as a pure modulation source).
//!
//! Any module's output can modulate another module's phase, harmonic, shift,
//! level or pitch at audio rate; sources below their target are read one
//! sample late. Lane effects are allocated when the synth is built, on the UI
//! thread; rendering never allocates.

use crate::effects::{EffectProcessor, ProcessContext};
use crate::instruments::{saw, Gate};
use crate::model::{
    module_slot, slot, AudioParam, CurveInput, LfoMode, ModularParams, ModuleKind, ModulatorKind, ParamInfo, WaveEnv, MODULAR_PARAMS,
    MOD_AUDIO, MOD_AUDIO_AMOUNT, MOD_GLIDE, MOD_LANES, MOD_LANE_GAIN, MOD_MACRO_PARAMS, MOD_MASTER, MOD_MODULATORS, MOD_MODULES, MOD_NO_LANE,
    MOD_NO_MODULE, MOD_SLOTS,
};
use crate::sample::Sample;
use crate::wavesynth::{note_freq, Env, Stage, SAMPLE_ROOT_HZ};
use crate::wavetable::Wavetable;
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

const VOICES: usize = 12;
const UNISON: usize = 8;
const CONTROL: usize = 32;
const PARAMS: usize = MODULAR_PARAMS.len();

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
    positions: [f64; UNISON],
    seed: u32,
    noise: [f32; 2],
    /// Filter memory: [channel][stage].
    biquad: [[[f32; 2]; 2]; 2],
    env: Env,
}

impl ModuleState {
    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32
    }
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
    envs: [Env; MOD_MODULATORS],
    lfo: [f32; MOD_MODULATORS],
    random_state: [(f32, f32, f32); MOD_MODULATORS],
    modules: [ModuleState; MOD_MODULES],
    /// Each module's latest output (mono), the source of audio-rate modulation.
    outputs: [f32; MOD_MODULES],
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
    rates: [f64; UNISON],
    start: f64,
    noise_a: f32,
    stereo: f32,
    biquad: Biquad,
    stages: usize,
    drive: f32,
    bias: f32,
    spread: f32,
    mix: f32,
    steps: f32,
    pan: (f32, f32),
    envelope: WaveEnv,
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
        noise_a: 0.0,
        stereo: 0.0,
        biquad: Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 },
        stages: 1,
        drive: 1.0,
        bias: 0.0,
        spread: 0.0,
        mix: 1.0,
        steps: 16.0,
        pan: (1.0, 1.0),
        envelope: WaveEnv { attack: 0.001, hold: 0.0, decay: 1.0, sustain: 1.0, release: 0.1 },
        lane: None,
    };
}

pub struct ModSynth {
    params: ModularParams,
    sample_rate: f32,
    tempo: f64,
    voices: Vec<Voice>,
    counter: u64,
    clock: f64,
    held: [u8; 16],
    held_count: usize,
    last_note: Option<f32>,
    seed: u32,
    lanes: Vec<Vec<Option<(EffectProcessor, bool)>>>,
    bus: [[[f32; CONTROL]; 2]; MOD_LANES],
    master: [[f32; CONTROL]; 2],
}

impl ModSynth {
    pub fn new(params: ModularParams, sample_rate: f32) -> Self {
        let voice = |i: usize| {
            let module = |m: usize| ModuleState {
                phases: [0.0; UNISON],
                masters: [0.0; UNISON],
                positions: [-1.0; UNISON],
                seed: 0x9E37_79B9 ^ ((i * MOD_MODULES + m) as u32).wrapping_mul(0x85EB_CA6B),
                noise: [0.0; 2],
                biquad: [[[0.0; 2]; 2]; 2],
                env: Env::IDLE,
            };
            Voice {
                active: false,
                gate: Gate::OFF,
                note: 60.0,
                target: 60.0,
                velocity: 0.0,
                random: 0.0,
                age: 0.0,
                envs: [Env::IDLE; MOD_MODULATORS],
                lfo: [0.0; MOD_MODULATORS],
                random_state: [(0.5, 0.5, 0.0); MOD_MODULATORS],
                modules: std::array::from_fn(module),
                outputs: [0.0; MOD_MODULES],
            }
        };
        let lanes = params.lanes.iter().map(|lane| lane.fx.iter().map(|slot| slot.map(|d| (EffectProcessor::new(&d.effect, sample_rate), d.enabled))).collect()).collect();
        Self {
            params,
            sample_rate,
            tempo: 120.0,
            voices: (0..VOICES).map(voice).collect(),
            counter: 0,
            clock: 0.0,
            held: [0; 16],
            held_count: 0,
            last_note: None,
            seed: 0x2468_ACE1,
            lanes,
            bus: [[[0.0; CONTROL]; 2]; MOD_LANES],
            master: [[0.0; CONTROL]; 2],
        }
    }

    pub fn params(&self) -> ModularParams {
        self.params
    }

    pub fn set_params(&mut self, params: ModularParams) {
        self.params = params;
        for (lane, settings) in self.lanes.iter_mut().zip(params.lanes.iter()) {
            for (slot, device) in lane.iter_mut().zip(settings.fx.iter()) {
                if let (Some((processor, enabled)), Some(device)) = (slot.as_mut(), device) {
                    processor.set_params(&device.effect);
                    *enabled = device.enabled;
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

    fn start_voice(&mut self, index: usize, pitch: u8, velocity: f32, gate: u32, retrigger: bool) {
        let glide_from = if self.params.glide > 0.0 { self.last_note } else { None };
        self.counter += 1;
        let random = self.random();
        let mut randoms = [[0f32; UNISON]; MOD_MODULES];
        for module in &mut randoms {
            for r in module.iter_mut() {
                *r = self.random() * 2.0 - 1.0;
            }
        }
        let p = self.params;
        let voice = &mut self.voices[index];
        let was_active = voice.active;
        voice.gate = Gate::new(pitch, velocity, gate, self.counter);
        voice.target = pitch as f32;
        voice.velocity = velocity;
        if !was_active || retrigger {
            voice.note = glide_from.unwrap_or(pitch as f32);
            voice.random = random;
            voice.age = 0.0;
            for (m, state) in voice.modules.iter_mut().enumerate() {
                let module = &p.modules[m];
                // Start phase plus a random variation per note and unison voice.
                let phase = module.get(slot::PHASE) / 360.0;
                let spread = module.get(slot::PHASE_RANDOM) / 360.0;
                for u in 0..UNISON {
                    state.phases[u] = (phase + spread * randoms[m][u]).rem_euclid(1.0);
                    state.masters[u] = 0.0;
                }
                state.positions = [-1.0; UNISON];
                if !was_active {
                    state.biquad = [[[0.0; 2]; 2]; 2];
                    state.noise = [0.0; 2];
                }
                state.env.trigger();
            }
            if !was_active {
                voice.outputs = [0.0; MOD_MODULES];
            }
            for (m, modulator) in p.modulators.iter().enumerate() {
                if modulator.kind == ModulatorKind::Lfo && modulator.mode != LfoMode::Free {
                    voice.lfo[m] = 0.0;
                }
                voice.envs[m].trigger();
            }
        }
        voice.active = true;
        self.last_note = Some(pitch as f32);
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32, gate: u32) {
        if self.params.mono {
            let legato = self.params.legato && self.voices[0].active && self.voices[0].gate.held && self.held_count > 0;
            if self.held_count < self.held.len() {
                self.held[self.held_count] = pitch;
                self.held_count += 1;
            }
            self.start_voice(0, pitch, velocity, gate, !legato);
            return;
        }
        let index = (0..VOICES)
            .find(|&i| !self.voices[i].active)
            .unwrap_or_else(|| (0..VOICES).min_by_key(|&i| self.voices[i].gate.age).unwrap_or(0));
        self.start_voice(index, pitch, velocity, gate, true);
    }

    fn release(voice: &mut Voice) {
        voice.gate.held = false;
        voice.envs.iter_mut().for_each(Env::release);
        voice.modules.iter_mut().for_each(|m| m.env.release());
    }

    pub fn note_off(&mut self, pitch: u8) {
        if self.params.mono {
            if let Some(at) = self.held[..self.held_count].iter().position(|&n| n == pitch) {
                self.held.copy_within(at + 1..self.held_count, at);
                self.held_count -= 1;
            }
            let voice = &mut self.voices[0];
            if voice.gate.held && voice.gate.pitch == pitch {
                if self.held_count > 0 {
                    let previous = self.held[self.held_count - 1];
                    voice.gate.pitch = previous;
                    voice.target = previous as f32;
                } else {
                    Self::release(voice);
                }
            }
            return;
        }
        for v in self.voices.iter_mut().filter(|v| v.active && v.gate.held && v.gate.pitch == pitch) {
            Self::release(v);
        }
    }

    pub fn all_notes_off(&mut self) {
        self.held_count = 0;
        self.voices.iter_mut().for_each(Self::release);
    }

    pub fn render(&mut self, samples: &[Arc<Sample>], tables: &[Arc<Wavetable>], left: &mut [f32], right: &mut [f32]) {
        let mut base = [0f32; PARAMS];
        let mut params = self.params;
        for (i, value) in base.iter_mut().enumerate() {
            *value = params.param_mut(i).map_or(0.0, |v| *v);
        }
        let mut offset = 0;
        while offset < left.len() {
            let n = (left.len() - offset).min(CONTROL);
            let (mut bus, mut master) = (self.bus, self.master);
            for lane in &mut bus {
                lane[0][..n].fill(0.0);
                lane[1][..n].fill(0.0);
            }
            master[0][..n].fill(0.0);
            master[1][..n].fill(0.0);
            for v in 0..VOICES {
                if self.voices[v].active {
                    self.render_voice(v, samples, tables, &base, &mut bus, &mut master, n);
                }
            }
            let tempo = self.tempo;
            let ctx = ProcessContext { sample_rate: self.sample_rate, tempo, beat: self.clock * tempo / 60.0, beats_per_sample: tempo / 60.0 / self.sample_rate as f64 };
            for lane in 0..MOD_LANES {
                {
                    let [l, r] = &mut bus[lane];
                    for (processor, _) in self.lanes[lane].iter_mut().flatten().filter(|(_, on)| *on) {
                        processor.process(&mut l[..n], &mut r[..n], &ctx);
                    }
                }
                let gain = base[MOD_LANE_GAIN + lane];
                let dest = self.params.lanes[lane].dest as usize;
                // Lanes only feed later lanes, so routing can never loop.
                let into = (self.params.lanes[lane].dest != MOD_NO_LANE && dest > lane && dest < MOD_LANES).then_some(dest);
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
            let gain = base[MOD_MASTER];
            for i in 0..n {
                left[offset + i] += master[0][i] * gain;
                right[offset + i] += master[1][i] * gain;
            }
            self.bus = bus;
            self.master = master;
            self.clock += n as f64 / self.sample_rate as f64;
            offset += n;
        }
    }

    /// The control-rate modulators for a voice, applied to the parameter values `m`.
    fn modulate(&mut self, v: usize, base: &[f32; PARAMS], m: &mut [f32; PARAMS], block: f32) {
        let p = &self.params;
        let tempo = self.tempo;
        let clock = self.clock;
        let voice = &mut self.voices[v];
        let mut values = [0f32; MOD_MODULATORS];
        for (i, modulator) in p.modulators.iter().enumerate() {
            let at = |k: usize| base[ModularParams::modulator_param(i, k)];
            values[i] = match modulator.kind {
                ModulatorKind::Off => continue,
                ModulatorKind::Envelope => voice.envs[i].tick(&envelope(at(0), at(1), at(2), at(3)), block),
                ModulatorKind::Lfo => {
                    let rate = if modulator.sync_beats > 0.0 { (tempo / 60.0) as f32 / modulator.sync_beats } else { 0.02 * 2000f32.powf(at(0)) };
                    let phase = match modulator.mode {
                        LfoMode::Free => ((clock * rate as f64).fract()) as f32,
                        LfoMode::Retrig => voice.lfo[i].fract(),
                        LfoMode::Envelope => voice.lfo[i].min(1.0),
                    };
                    voice.lfo[i] += rate * block;
                    modulator.shape.value_at((phase + at(1)).fract())
                }
                ModulatorKind::Random => {
                    let rate = 0.1 * 400f32.powf(at(0));
                    let (mut hold, mut smooth, mut phase) = voice.random_state[i];
                    phase += rate * block;
                    if phase >= 1.0 {
                        phase -= phase.floor();
                        hold = voice.modules[0].random();
                    }
                    let k = if at(1) < 0.01 { 1.0 } else { 1.0 - (-block * rate * 4.0 / at(1)).exp() };
                    smooth += (hold - smooth) * k;
                    voice.random_state[i] = (hold, smooth, phase);
                    smooth
                }
                ModulatorKind::Curve => modulator.shape.value_at(match modulator.input {
                    CurveInput::Velocity => voice.velocity,
                    CurveInput::Note => voice.note / 127.0,
                    CurveInput::Random => voice.random,
                }),
                ModulatorKind::Note => voice.note / 127.0,
                ModulatorKind::Velocity => voice.velocity,
                ModulatorKind::Macro => base[MOD_MACRO_PARAMS + (modulator.index as usize).min(7)],
            };
        }
        for (i, modulator) in p.modulators.iter().enumerate() {
            if modulator.kind == ModulatorKind::Off {
                continue;
            }
            let value = if modulator.bipolar { values[i] * 2.0 - 1.0 } else { values[i] };
            for target in modulator.targets.iter().filter(|t| (t.dest as usize) < PARAMS) {
                let dest = target.dest as usize;
                let info: &ParamInfo = &MODULAR_PARAMS[dest];
                m[dest] = info.denormalize(info.normalize(m[dest]) + target.depth * value);
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
        n: usize,
    ) {
        let p = self.params;
        let sr = self.sample_rate;
        let dt = 1.0 / sr;
        let block = n as f32 * dt;
        let mut m = *base;
        self.modulate(v, base, &mut m, block);

        let voice = &mut self.voices[v];
        let glide = m[MOD_GLIDE];
        if glide > 0.0 && voice.note != voice.target {
            voice.note += (voice.target - voice.note) * (1.0 - (-block * 3000.0 / glide).exp());
            if (voice.note - voice.target).abs() < 0.001 {
                voice.note = voice.target;
            }
        } else {
            voice.note = voice.target;
        }

        // Module settings for the block, in real units.
        let mut blocks = [Block::NONE; MOD_MODULES];
        let mut has_output = false;
        for i in 0..MOD_MODULES {
            let module = &p.modules[i];
            if !module.on || matches!(module.kind, ModuleKind::Empty | ModuleKind::Group) {
                continue;
            }
            let at = |s: usize| module_slot(module.kind, s).map_or(0.0, |info| info.denormalize(m[i * MOD_SLOTS + s]));
            let b = &mut blocks[i];
            match module.kind {
                kind if kind.is_generator() => {
                    b.level = at(slot::LEVEL);
                    // Harmonic multiplies the note's frequency (0: no keytracking); Shift adds Hz.
                    let freq = note_freq(voice.note + at(slot::SEMI) + at(slot::CENT) / 100.0) * at(slot::HARMONIC) + at(slot::SHIFT);
                    b.freq = freq;
                    let count = if kind.has_unison() { (at(slot::UNISON).round() as usize).clamp(1, UNISON) } else { 1 };
                    let spread_cents = at(slot::DETUNE).powf(1.5) * 100.0;
                    let (spread, blend) = (at(slot::SPREAD), at(slot::BLEND));
                    let mut norm = 0.0;
                    for u in 0..count {
                        let s = if count == 1 { 0.0 } else { u as f32 / (count - 1) as f32 * 2.0 - 1.0 };
                        b.incs[u] = freq * 2f32.powf(s * spread_cents / 1200.0) / sr;
                        let g = if count == 1 || s.abs() < 0.01 { 1.0 } else { blend.max(0.0) };
                        norm += g * g;
                        let angle = ((s * spread).clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
                        b.gains[u] = (angle.cos() * g, angle.sin() * g);
                    }
                    let scale = std::f32::consts::SQRT_2 / norm.sqrt().max(1e-6);
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
                            b.mip = Wavetable::level_for(freq.abs() * 2f32.powf(spread_cents / 1200.0), sr);
                        }
                        ModuleKind::Sample => {
                            if let Some(sample) = samples.get(i).filter(|s| s.frames() > 0) {
                                let native = if sample.sample_rate > 0.0 { sample.sample_rate } else { sr };
                                let root = note_freq(at(slot::B)) / note_freq(60.0) * SAMPLE_ROOT_HZ;
                                for u in 0..count {
                                    b.rates[u] = (b.incs[u] * sr / root * native / sr) as f64;
                                }
                                b.start = at(slot::A).clamp(0.0, 0.999) as f64 * sample.frames() as f64;
                            } else {
                                b.count = 0;
                            }
                        }
                        _ => {
                            // Noise: slope from white (no filtering) towards brown.
                            b.noise_a = at(slot::A).powf(0.5) * 0.997;
                            b.stereo = at(slot::B);
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
                ModuleKind::Mix | ModuleKind::Aux => b.level = at(slot::LEVEL) * if module.flag { -1.0 } else { 1.0 },
                ModuleKind::Output => {
                    has_output = true;
                    b.level = at(slot::LEVEL);
                    let angle = (at(slot::PAN) + 1.0) * std::f32::consts::FRAC_PI_4;
                    b.pan = (angle.cos() * std::f32::consts::SQRT_2, angle.sin() * std::f32::consts::SQRT_2);
                    b.envelope = WaveEnv { attack: at(slot::ATTACK), hold: at(slot::HOLD), decay: at(slot::DECAY), sustain: at(slot::SUSTAIN), release: at(slot::RELEASE) };
                    let send = module.choice[0] as usize;
                    b.lane = module.flag.then_some(if send < MOD_LANES { send } else { usize::MAX });
                }
                _ => {}
            }
        }
        if !has_output {
            voice.active = false;
            return;
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
                Self::release(voice);
            }
            let mut sounding = false;
            let (mut l, mut r) = (0f32, 0f32);
            for k in 0..MOD_MODULES {
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
                let state = &mut voice.modules[k];
                match module.kind {
                    ModuleKind::Analog | ModuleKind::Wavetable | ModuleKind::Sample => {
                        // Frequency modulation scales every unison voice's increment alike.
                        let mut scale = 1.0 + harmonic;
                        if shift != 0.0 && b.freq.abs() > 1e-3 {
                            scale += shift / b.freq;
                        }
                        if pitch != 0.0 {
                            scale *= 2f32.powf(pitch / 12.0);
                        }
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
                                    let frames = sample.frames() as f64;
                                    let position = &mut state.positions[u];
                                    if *position < 0.0 {
                                        *position = b.start;
                                    }
                                    if *position >= frames {
                                        if !module.flag || frames - b.start < 2.0 {
                                            continue;
                                        }
                                        *position = b.start + (*position - frames) % (frames - b.start);
                                    }
                                    let (a, c) = sample.read(*position + phase as f64 * 64.0);
                                    *position += b.rates[u] * scale as f64;
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
                    ModuleKind::Noise => {
                        let (white, a, c) = (state.random() * 2.0 - 1.0, state.random() * 2.0 - 1.0, state.random() * 2.0 - 1.0);
                        let (nl, nr) = (white + (a - white) * b.stereo, white + (c - white) * b.stereo);
                        let coef = b.noise_a;
                        state.noise[0] = nl + coef * (state.noise[0] - nl);
                        state.noise[1] = nr + coef * (state.noise[1] - nr);
                        // Filtering darker loses level; give some of it back.
                        let level = b.level * ring * (1.0 + coef * 6.0);
                        l += state.noise[0] * level;
                        r += state.noise[1] * level;
                        voice.outputs[k] = (state.noise[0] + state.noise[1]) * 0.5 * level;
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
                    ModuleKind::Output => {
                        let env = state.env.tick(&b.envelope, dt);
                        if state.env.stage != Stage::Idle {
                            sounding = true;
                        }
                        let g = env * b.level * ring * voice.velocity;
                        let (ol, or) = (l * g * b.pan.0, r * g * b.pan.1);
                        voice.outputs[k] = (ol + or) * 0.5;
                        match b.lane {
                            Some(lane) if lane < MOD_LANES => {
                                bus[lane][0][i] += ol;
                                bus[lane][1][i] += or;
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
                voice.active = false;
                break;
            }
        }
        voice.age += block;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AudioMod, Device, Effect, Lane, ModTarget, Module, Modulator, UtilityParams};
    use crate::synth::HELD;

    const SR: f32 = 48_000.0;

    fn play(p: ModularParams, notes: &[u8], frames: usize) -> (Vec<f32>, Vec<f32>) {
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

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn frequency(signal: &[f32]) -> f32 {
        signal.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f32 * SR / signal.len() as f32
    }

    fn sine() -> Module {
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

    fn group() -> Module {
        Module::new(ModuleKind::Group)
    }

    fn output() -> Module {
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
        lfo.targets[0] = ModTarget { dest: ModularParams::module_param(1, slot::LEVEL) as u16, depth: -0.5 };
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
        assert_eq!(poly.voices.iter().filter(|v| v.active).count(), VOICES);
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
    }
}
