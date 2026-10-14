//! Modular synthesizer: up to eight generators, each with a chain of four
//! modifiers (filters, distortion, ring/FM/PM from another generator, phase
//! distortion, bit and rate reduction, gain), playing into three effect lanes,
//! and up to sixteen free modulators that can target any setting.
//!
//! Built on the UI thread (lane effects and any comb filters allocate there);
//! rendering never allocates.

use crate::effects::{EffectProcessor, ProcessContext};
use crate::instruments::{saw, Gate};
use crate::model::{
    GenKind, KickDrive, LfoMode, ModifierKind, ModularParams, ModulatorKind, ParamInfo, WaveEnv, WaveFilter, CurveInput, MODULAR_PARAMS, MOD_AMP,
    MOD_GENS, MOD_GLIDE, MOD_LANES, MOD_LANE_GAIN, MOD_MACRO_PARAMS, MOD_MASTER, MOD_MODIFIERS, MOD_MODULATORS, MOD_NO_LANE,
};
use crate::sample::Sample;
use crate::wavesynth::{build_harmonic, note_freq, Env, FilterCoefs, FilterState, Stage, HARMONIC_TABLE, SAMPLE_ROOT_HZ};
use crate::wavetable::Wavetable;
use std::f32::consts::TAU;
use std::sync::Arc;

const VOICES: usize = 12;
const UNISON: usize = 8;
const CONTROL: usize = 32;
const PARAMS: usize = MODULAR_PARAMS.len();
const HARMONIC_EVERY: u32 = 4;

/// Settings of modifier `m` of generator `g` in the parameter table.
fn modifier_index(g: usize, m: usize) -> usize {
    ModularParams::modifier_param(g, m, 0)
}

/// What a modulator envelope's 0..1 settings mean in seconds.
fn envelope(a: f32, b: f32, c: f32, d: f32) -> WaveEnv {
    WaveEnv { attack: 0.0005 * 20_000f32.powf(a), hold: 0.0, decay: 0.001 * 10_000f32.powf(b), sustain: c, release: 0.001 * 10_000f32.powf(d) }
}

/// `x` wrapped into 0..1, for `x` in 0..2 (cheaper than `%`).
#[inline]
fn wrap(x: f32) -> f32 {
    if x >= 1.0 { x - 1.0 } else { x }
}

fn analog(ph: f32, inc: f32, shape: f32) -> f32 {
    // Only the two waves being blended are computed: this runs for every unison voice.
    let morph = shape.clamp(0.0, 1.0) * 3.0;
    let triangle = |ph: f32| 1.0 - 4.0 * (wrap(ph + 0.25) - 0.5).abs();
    let saw_wave = |ph: f32| saw(wrap(ph + 0.5), inc);
    match morph {
        m if m < 1.0 => {
            let sine = (ph * TAU).sin();
            sine + (triangle(ph) - sine) * m
        }
        m if m < 2.0 => {
            let t = triangle(ph);
            t + (saw_wave(ph) - t) * (m - 1.0)
        }
        m if m >= 2.999 => saw(ph, inc) - saw(wrap(ph + 0.5), inc),
        m => {
            let s = saw_wave(ph);
            let pulse = saw(ph, inc) - s;
            s + (pulse - s) * (m - 2.0)
        }
    }
}

/// Casio-style phase distortion: the first part of the cycle speeds up.
fn phase_distort(phase: f32, amount: f32) -> f32 {
    let knee = 0.5 - amount.clamp(0.0, 1.0) * 0.49;
    if phase < knee { phase * 0.5 / knee } else { 0.5 + (phase - knee) * 0.5 / (1.0 - knee) }
}

fn distort(x: f32, mode: KickDrive, drive: f32) -> f32 {
    let g = 1.0 + drive * 15.0;
    match mode {
        KickDrive::Soft => (x * g).tanh() / g.tanh(),
        KickDrive::Hard => (x * g).clamp(-1.0, 1.0),
        KickDrive::Fold => (x * g * std::f32::consts::FRAC_PI_2).sin(),
        KickDrive::Tube => {
            if x >= 0.0 { (x * g).tanh() / g.tanh() } else { (x * g * 0.5).tanh() / (g * 0.5).tanh() }
        }
        KickDrive::Crush => {
            let steps = 2f32.powf(12.0 - drive * 10.0);
            ((x * (1.0 + drive * 3.0)).clamp(-1.0, 1.0) * steps).round() / steps
        }
    }
}

#[derive(Clone)]
struct ModifierState {
    filter: [FilterState; 2],
    tone: [f32; 2],
    held: (f32, f32),
    counter: f32,
}

#[derive(Clone)]
struct GenState {
    phases: [f32; UNISON],
    positions: [f64; UNISON],
    seed: u32,
    noise: [f32; 2],
    harmonic: [f32; HARMONIC_TABLE],
    /// Last output (mono), read by other generators' ring/FM/PM modifiers.
    last: f32,
    modifiers: Vec<ModifierState>,
}

impl GenState {
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
    amp: Env,
    envs: [Env; MOD_MODULATORS],
    lfo: [f32; MOD_MODULATORS],
    /// Random modulators: held value, smoothed value, phase towards the next.
    random_state: [(f32, f32, f32); MOD_MODULATORS],
    gens: Vec<GenState>,
    harmonic_wait: u32,
}

/// Per-block settings of one generator.
struct GenBlock {
    kind: GenKind,
    count: usize,
    incs: [f32; UNISON],
    gains: [(f32, f32); UNISON],
    shape: f32,
    level: usize,
    rates: [f64; UNISON],
    start: f64,
    noise_a: f32,
    /// None: heard only through the generators that read it.
    lane: Option<usize>,
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
    sine: Vec<f32>,
}

impl ModSynth {
    pub fn new(params: ModularParams, sample_rate: f32) -> Self {
        let voice = |i: usize| {
            let gens = (0..MOD_GENS)
                .map(|g| GenState {
                    phases: [0.0; UNISON],
                    positions: [-1.0; UNISON],
                    seed: 0x9E37_79B9 ^ ((i * MOD_GENS + g) as u32).wrapping_mul(0x85EB_CA6B),
                    noise: [0.0; 2],
                    harmonic: [0.0; HARMONIC_TABLE],
                    last: 0.0,
                    modifiers: (0..MOD_MODIFIERS)
                        .map(|m| {
                            let modifier = params.gens[g].modifiers[m];
                            // Delay lines only where a comb filter is set (changing that rebuilds the synth).
                            let comb = modifier.kind == ModifierKind::Filter && modifier.filter == WaveFilter::Comb;
                            let filter = || if comb { FilterState::new() } else { FilterState::without_comb() };
                            ModifierState { filter: [filter(), filter()], tone: [0.0; 2], held: (0.0, 0.0), counter: 0.0 }
                        })
                        .collect(),
                })
                .collect();
            Voice {
                active: false,
                gate: Gate::OFF,
                note: 60.0,
                target: 60.0,
                velocity: 0.0,
                random: 0.0,
                age: 0.0,
                amp: Env::IDLE,
                envs: [Env::IDLE; MOD_MODULATORS],
                lfo: [0.0; MOD_MODULATORS],
                random_state: [(0.5, 0.5, 0.0); MOD_MODULATORS],
                gens,
                harmonic_wait: 0,
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
            sine: (0..HARMONIC_TABLE).map(|i| (i as f32 / HARMONIC_TABLE as f32 * TAU).sin()).collect(),
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
        let mut phases = [[0f32; UNISON]; MOD_GENS];
        for generator in &mut phases {
            for p in generator.iter_mut() {
                *p = self.random();
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
            voice.harmonic_wait = 0;
            for (g, state) in voice.gens.iter_mut().enumerate() {
                state.phases = phases[g];
                state.positions = [-1.0; UNISON];
                if !was_active {
                    state.last = 0.0;
                    for m in &mut state.modifiers {
                        m.filter[0].clear();
                        m.filter[1].clear();
                        m.tone = [0.0; 2];
                    }
                }
            }
            for (m, modulator) in p.modulators.iter().enumerate() {
                if modulator.kind == ModulatorKind::Lfo && modulator.mode != LfoMode::Free {
                    voice.lfo[m] = 0.0;
                }
                voice.envs[m].trigger();
            }
            voice.amp.trigger();
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
                    voice.gate.held = false;
                    voice.amp.release();
                    voice.envs.iter_mut().for_each(Env::release);
                }
            }
            return;
        }
        for v in self.voices.iter_mut().filter(|v| v.active && v.gate.held && v.gate.pitch == pitch) {
            v.gate.held = false;
            v.amp.release();
            v.envs.iter_mut().for_each(Env::release);
        }
    }

    pub fn all_notes_off(&mut self) {
        self.held_count = 0;
        for v in &mut self.voices {
            v.gate.held = false;
            v.amp.release();
            v.envs.iter_mut().for_each(Env::release);
        }
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
            let mut bus = self.bus;
            for lane in &mut bus {
                lane[0][..n].fill(0.0);
                lane[1][..n].fill(0.0);
            }
            for v in 0..VOICES {
                if self.voices[v].active {
                    self.render_voice(v, samples, tables, &base, &mut bus, n);
                }
            }
            let tempo = self.tempo;
            let ctx = ProcessContext { sample_rate: self.sample_rate, tempo, beat: self.clock * tempo / 60.0, beats_per_sample: tempo / 60.0 / self.sample_rate as f64 };
            let master_gain = base[MOD_MASTER];
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
                if self.params.lanes[lane].dest != MOD_NO_LANE && dest > lane && dest < MOD_LANES {
                    for i in 0..n {
                        let (a, b) = (bus[lane][0][i] * gain, bus[lane][1][i] * gain);
                        bus[dest][0][i] += a;
                        bus[dest][1][i] += b;
                    }
                } else {
                    for i in 0..n {
                        left[offset + i] += bus[lane][0][i] * gain * master_gain;
                        right[offset + i] += bus[lane][1][i] * gain * master_gain;
                    }
                }
            }
            self.bus = bus;
            self.clock += n as f64 / self.sample_rate as f64;
            offset += n;
        }
    }

    fn render_voice(&mut self, v: usize, samples: &[Arc<Sample>], tables: &[Arc<Wavetable>], base: &[f32; PARAMS], bus: &mut [[[f32; CONTROL]; 2]; MOD_LANES], n: usize) {
        let p = self.params;
        let sr = self.sample_rate;
        let dt = 1.0 / sr;
        let block = n as f32 * dt;
        let tempo = self.tempo;

        // Modulators, evaluated once per block.
        let mut m = *base;
        {
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
                            LfoMode::Free => ((self.clock * rate as f64).fract()) as f32,
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
                            hold = voice.gens[0].random();
                        }
                        let k = if at(1) < 0.01 { 1.0 } else { 1.0 - (-block * rate * 4.0 / at(1)).exp() };
                        smooth += (hold - smooth) * k;
                        voice.random_state[i] = (hold, smooth, phase);
                        smooth
                    }
                    ModulatorKind::Curve => {
                        let input = match modulator.input {
                            CurveInput::Velocity => voice.velocity,
                            CurveInput::Note => voice.note / 127.0,
                            CurveInput::Random => voice.random,
                        };
                        modulator.shape.value_at(input)
                    }
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
        let freq = note_freq(voice.note);
        let rebuild = voice.harmonic_wait == 0;
        voice.harmonic_wait = if rebuild { HARMONIC_EVERY } else { voice.harmonic_wait - 1 };

        // Generator settings for the block.
        let mut blocks: [Option<GenBlock>; MOD_GENS] = std::array::from_fn(|_| None);
        let mut coefs: [[Option<FilterCoefs>; MOD_MODIFIERS]; MOD_GENS] = std::array::from_fn(|_| std::array::from_fn(|_| None));
        // Per-modifier constants for the block (tone coefficient, steps, hold length, gain).
        let mut constants = [[0f32; MOD_MODIFIERS]; MOD_GENS];
        for g in 0..MOD_GENS {
            let generator = &p.gens[g];
            let ready = match generator.kind {
                GenKind::Off => false,
                GenKind::Wavetable => tables.get(g).is_some_and(|t| t.frames() > 0),
                GenKind::Sample => samples.get(g).is_some_and(|s| s.frames() > 0),
                _ => true,
            };
            // Generators without an output still run: other generators can read them.
            if !ready {
                continue;
            }
            let at = |i: usize| m[ModularParams::gen_param(g, i)];
            let f = freq * 2f32.powf(at(2).round() + at(3).round() / 12.0 + at(4) / 1200.0);
            let count = (at(6).round() as usize).clamp(1, UNISON);
            let spread_cents = at(7).powf(1.5) * 100.0;
            let mut incs = [0f32; UNISON];
            let mut gains = [(0f32, 0f32); UNISON];
            for u in 0..count {
                let s = if count == 1 { 0.0 } else { u as f32 / (count - 1) as f32 * 2.0 - 1.0 };
                incs[u] = f * 2f32.powf(s * spread_cents / 1200.0) / sr;
                let pan = (s * at(8) + at(1)).clamp(-1.0, 1.0);
                let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                let g = at(0) * std::f32::consts::SQRT_2 / (count as f32).sqrt();
                gains[u] = (angle.cos() * g, angle.sin() * g);
            }
            let top = f * 2f32.powf(spread_cents / 1200.0);
            let mut rates = [0f64; UNISON];
            let mut start = 0.0;
            if let Some(sample) = samples.get(g).filter(|_| generator.kind == GenKind::Sample) {
                let native = if sample.sample_rate > 0.0 { sample.sample_rate } else { sr };
                for u in 0..count {
                    rates[u] = (incs[u] * sr / SAMPLE_ROOT_HZ * native / sr) as f64;
                }
                start = at(5).clamp(0.0, 0.999) as f64 * sample.frames() as f64;
            }
            if generator.kind == GenKind::Harmonic && rebuild {
                let max_harmonic = (sr * 0.5 / top.max(1.0)) as usize;
                build_harmonic(&mut voice.gens[g].harmonic, &self.sine, 48.0, at(5) * 2.0 - 1.0, 0.0, 0.0, 0.0, max_harmonic);
            }
            for (k, modifier) in generator.modifiers.iter().enumerate() {
                let x = modifier_index(g, k);
                let (a, b) = (m[x], m[x + 1]);
                constants[g][k] = match modifier.kind {
                    ModifierKind::Filter => {
                        coefs[g][k] = Some(FilterCoefs::new(modifier.filter, 20.0 * 1000f32.powf(a), b, 0.0, m[x + 2], sr));
                        0.0
                    }
                    ModifierKind::Distortion => (-TAU * 200.0 * 100f32.powf(b) / sr).exp(),
                    ModifierKind::Bitcrush => 2f32.powf(16.0 - a * 15.0),
                    ModifierKind::Downsample => 64f32.powf(a),
                    ModifierKind::Gain => 10f32.powf((a * 48.0 - 24.0) / 20.0),
                    _ => 0.0,
                };
            }
            blocks[g] = Some(GenBlock {
                kind: generator.kind,
                count,
                incs,
                gains,
                shape: at(5),
                level: Wavetable::level_for(top, sr),
                rates,
                start,
                noise_a: (-TAU * 200.0 * 100f32.powf(at(5)) / sr).exp(),
                lane: (generator.output != MOD_NO_LANE).then(|| (generator.output as usize).min(MOD_LANES - 1)),
            });
        }

        let amp_env = WaveEnv { attack: m[MOD_AMP], hold: m[MOD_AMP + 1], decay: m[MOD_AMP + 2], sustain: m[MOD_AMP + 3], release: m[MOD_AMP + 4] };
        for i in 0..n {
            if voice.gate.tick() {
                voice.amp.release();
                voice.envs.iter_mut().for_each(Env::release);
            }
            let amp = voice.amp.tick(&amp_env, dt);
            if voice.amp.stage == Stage::Idle {
                voice.active = false;
                break;
            }
            let gain = amp * voice.velocity;
            for g in 0..MOD_GENS {
                let Some(block) = &blocks[g] else { continue };
                let generator = &p.gens[g];
                // Phase modifiers, from the other generators' last outputs.
                let (mut fm, mut pm, mut pd) = (0.0f32, 0.0f32, None);
                for (k, modifier) in generator.modifiers.iter().enumerate() {
                    let x = modifier_index(g, k);
                    let source = voice.gens[(modifier.source as usize).min(MOD_GENS - 1)].last;
                    match modifier.kind {
                        ModifierKind::FmFrom => fm += m[x] * 4.0 * source,
                        ModifierKind::PmFrom => pm += m[x] * 0.5 * source,
                        ModifierKind::PhaseDistort => pd = Some(m[x]),
                        _ => {}
                    }
                }
                let state = &mut voice.gens[g];
                let (mut l, mut r) = (0.0f32, 0.0f32);
                match block.kind {
                    GenKind::Analog | GenKind::Wavetable | GenKind::Harmonic => {
                        for u in 0..block.count {
                            let mut ph = (state.phases[u] + pm).rem_euclid(1.0);
                            if let Some(amount) = pd {
                                ph = phase_distort(ph, amount);
                            }
                            let inc = block.incs[u];
                            let s = match block.kind {
                                GenKind::Analog => analog(ph, inc, block.shape),
                                GenKind::Wavetable => tables[g].read(block.level, block.shape, ph),
                                _ => {
                                    let x = ph * HARMONIC_TABLE as f32;
                                    let j = x as usize % HARMONIC_TABLE;
                                    let a = state.harmonic[j];
                                    a + (state.harmonic[(j + 1) % HARMONIC_TABLE] - a) * (x - x.floor())
                                }
                            };
                            let next = state.phases[u] + inc * (1.0 + fm);
                            state.phases[u] = next - next.floor();
                            l += s * block.gains[u].0;
                            r += s * block.gains[u].1;
                        }
                    }
                    GenKind::Sample => {
                        let sample = &samples[g];
                        let frames = sample.frames() as f64;
                        for u in 0..block.count {
                            let position = &mut state.positions[u];
                            if *position < 0.0 {
                                *position = block.start;
                            }
                            if *position >= frames {
                                if frames - block.start < 2.0 {
                                    continue;
                                }
                                *position = block.start + (*position - frames) % (frames - block.start);
                            }
                            let (a, b) = sample.read(*position);
                            *position += block.rates[u] * (1.0 + fm as f64);
                            l += a * block.gains[u].0;
                            r += b * block.gains[u].1;
                        }
                    }
                    GenKind::Noise => {
                        let (a, b) = (state.random() * 2.0 - 1.0, state.random() * 2.0 - 1.0);
                        state.noise[0] = a + block.noise_a * (state.noise[0] - a);
                        state.noise[1] = b + block.noise_a * (state.noise[1] - b);
                        l = state.noise[0] * block.gains[0].0;
                        r = state.noise[1] * block.gains[0].1;
                    }
                    GenKind::Off => {}
                }
                // Audio modifiers, in order.
                for k in 0..MOD_MODIFIERS {
                    let modifier = generator.modifiers[k];
                    let x = modifier_index(g, k);
                    let (a, mix) = (m[x], m[x + 2]);
                    let wet = |dry: f32, wet: f32| dry + (wet - dry) * mix;
                    match modifier.kind {
                        ModifierKind::Filter => {
                            if let Some(c) = &coefs[g][k] {
                                let s = &mut voice.gens[g].modifiers[k];
                                l = c.process(&mut s.filter[0], l);
                                r = c.process(&mut s.filter[1], r);
                            }
                        }
                        ModifierKind::Distortion => {
                            let s = &mut voice.gens[g].modifiers[k];
                            let tone = constants[g][k];
                            let (dl, dr) = (distort(l, modifier.drive, a), distort(r, modifier.drive, a));
                            s.tone[0] = dl + tone * (s.tone[0] - dl);
                            s.tone[1] = dr + tone * (s.tone[1] - dr);
                            l = wet(l, s.tone[0]);
                            r = wet(r, s.tone[1]);
                        }
                        ModifierKind::RingMod => {
                            let source = voice.gens[(modifier.source as usize).min(MOD_GENS - 1)].last * 2.0;
                            l = wet(l, l * source);
                            r = wet(r, r * source);
                        }
                        ModifierKind::Bitcrush => {
                            let steps = constants[g][k];
                            l = wet(l, (l * steps).round() / steps);
                            r = wet(r, (r * steps).round() / steps);
                        }
                        ModifierKind::Downsample => {
                            let s = &mut voice.gens[g].modifiers[k];
                            s.counter -= 1.0;
                            if s.counter <= 0.0 {
                                s.counter += constants[g][k];
                                s.held = (l, r);
                            }
                            l = wet(l, s.held.0);
                            r = wet(r, s.held.1);
                        }
                        ModifierKind::Gain => {
                            l *= constants[g][k];
                            r *= constants[g][k];
                        }
                        ModifierKind::Off | ModifierKind::FmFrom | ModifierKind::PmFrom | ModifierKind::PhaseDistort => {}
                    }
                }
                let state = &mut voice.gens[g];
                // Normalised so FM depths behave alike whatever the generator's level.
                state.last = ((l + r) * 0.5 / p.gens[g].level.max(0.05)).clamp(-2.0, 2.0);
                if let Some(lane) = block.lane {
                    bus[lane][0][i] += l * gain;
                    bus[lane][1][i] += r * gain;
                }
            }
        }
        voice.age += block;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Device, Effect, Generator, Lane, ModTarget, Modifier, Modulator, UtilityParams, MOD_NO_TARGET};
    use crate::synth::HELD;

    const SR: f32 = 48_000.0;

    fn play(p: ModularParams, notes: &[u8], frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut synth = ModSynth::new(p, SR);
        for &n in notes {
            synth.note_on(n, 1.0, HELD);
        }
        let tables: Vec<Arc<Wavetable>> = (0..MOD_GENS).map(|_| Arc::new(crate::wavetable::builtin(0))).collect();
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

    fn sine_gen() -> Generator {
        Generator { position: 0.0, ..Generator::new(GenKind::Analog) }
    }

    #[test]
    fn generators_add_up_and_follow_their_tuning() {
        let mut p = ModularParams::default();
        p.gens[0] = sine_gen();
        let (one, _) = play(p, &[69], 24_000);
        assert!((frequency(&one[2400..]) - 440.0).abs() < 2.0);
        p.gens[1] = Generator { octave: 1.0, ..sine_gen() };
        let (two, _) = play(p, &[69], 24_000);
        assert!(rms(&two[2400..]) > rms(&one[2400..]) * 1.3, "a second generator adds");
        p.gens[0].output = MOD_NO_LANE;
        let (octave, _) = play(p, &[69], 24_000);
        assert!((frequency(&octave[2400..]) - 880.0).abs() < 3.0, "only the octave generator is heard");
    }

    #[test]
    fn every_generator_kind_plays() {
        for kind in GenKind::ALL {
            let mut p = ModularParams::default();
            p.gens[0] = Generator::new(kind);
            let (l, _) = play(p, &[60], 9600);
            if kind == GenKind::Sample {
                assert!(rms(&l) < 1e-6, "no sample, no sound");
            } else {
                assert!(rms(&l[2400..]) > 0.05 && l.iter().all(|s| s.is_finite()), "{kind:?}");
            }
        }
    }

    #[test]
    fn fm_and_ring_modifiers_read_another_generator() {
        let mut p = ModularParams::default();
        p.gens[0] = sine_gen();
        p.gens[1] = Generator { output: MOD_NO_LANE, octave: 1.0, ..sine_gen() };
        let (clean, _) = play(p, &[57], 24_000);
        p.gens[0].modifiers[0] = Modifier { kind: ModifierKind::FmFrom, source: 1, a: 0.5, ..Modifier::OFF };
        let (fm, _) = play(p, &[57], 24_000);
        let highs = |x: &[f32]| rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>());
        let brightness = |x: &[f32]| highs(x) / rms(x);
        assert!(brightness(&fm[2400..]) > brightness(&clean[2400..]) * 1.8, "FM adds sidebands");

        p.gens[0].modifiers[0] = Modifier { kind: ModifierKind::RingMod, source: 1, mix: 1.0, ..Modifier::OFF };
        let (ring, _) = play(p, &[57], 48_000);
        // 220 Hz × 440 Hz gives 220 + 660 Hz: the zero crossings follow the sum.
        assert!(frequency(&ring[4800..]) > 300.0, "{}", frequency(&ring[4800..]));
    }

    #[test]
    fn audio_modifiers_change_the_sound_in_order() {
        let base = {
            let mut p = ModularParams::default();
            p.gens[0] = Generator::new(GenKind::Analog);
            p
        };
        let (dry, _) = play(base, &[48], 24_000);
        for kind in [ModifierKind::Filter, ModifierKind::Distortion, ModifierKind::Bitcrush, ModifierKind::Downsample, ModifierKind::PhaseDistort, ModifierKind::Gain] {
            let mut p = base;
            let a = if matches!(kind, ModifierKind::Bitcrush | ModifierKind::Downsample) { 0.85 } else { 0.2 };
            p.gens[0].modifiers[0] = Modifier { kind, a, b: 0.3, ..Modifier::OFF };
            let (wet, _) = play(p, &[48], 24_000);
            let difference: f32 = wet.iter().zip(&dry).map(|(a, b)| (a - b).abs()).sum::<f32>() / wet.len() as f32;
            assert!(difference > 0.01 && wet.iter().all(|s| s.is_finite() && s.abs() < 4.0), "{kind:?}: {difference}");
        }
        // A low cutoff darkens.
        let mut p = base;
        p.gens[0].modifiers[0] = Modifier { kind: ModifierKind::Filter, a: 0.2, b: 0.1, ..Modifier::OFF };
        let (dark, _) = play(p, &[48], 24_000);
        let highs = |x: &[f32]| rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>());
        assert!(highs(&dark) < highs(&dry) * 0.5);
    }

    #[test]
    fn lanes_process_and_route_to_later_lanes() {
        let mut p = ModularParams::default();
        p.gens[0] = sine_gen();
        let (plain, _) = play(p, &[60], 9600);
        p.lanes[0].fx[0] = Some(Device::new(Effect::Utility(UtilityParams { gain_db: -12.0, ..Default::default() })));
        let (quieter, _) = play(p, &[60], 9600);
        assert!((rms(&quieter) / rms(&plain) - 0.251).abs() < 0.02);
        // Lane A into lane B, which halves it again.
        p.lanes[0].dest = 1;
        p.lanes[1] = Lane { gain: 0.5, ..Lane::EMPTY };
        let (routed, _) = play(p, &[60], 9600);
        assert!((rms(&routed) / rms(&plain) - 0.125).abs() < 0.015);
        // A lane pointing back never loops: it goes to the master.
        p.lanes[1].dest = 0;
        let (safe, _) = play(p, &[60], 9600);
        assert!(safe.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn modulators_reach_any_setting() {
        let mut p = ModularParams::default();
        p.gens[0] = sine_gen();
        // An LFO that is 1 for the first half second, then 0, turning the level down.
        let mut lfo = Modulator::new(ModulatorKind::Lfo);
        lfo.shape = crate::model::Shape::from_points(&[(0.0, 1.0, 0.0), (0.5, 1.0, 0.0), (0.501, 0.0, 0.0), (1.0, 0.0, 0.0)]);
        lfo.mode = LfoMode::Retrig;
        lfo.a = (1.0f32 / 0.02).ln() / 2000f32.ln(); // 1 Hz
        lfo.targets[0] = ModTarget { dest: ModularParams::gen_param(0, 0) as u16, depth: -1.0 };
        p.modulators[0] = lfo;
        let (l, _) = play(p, &[60], 48_000);
        assert!(rms(&l[4800..19_200]) < 0.01 && rms(&l[28_800..43_200]) > 0.1, "{} {}", rms(&l[4800..19_200]), rms(&l[28_800..43_200]));

        // A velocity curve sends low velocities to silence.
        let mut p = ModularParams::default();
        p.gens[0] = sine_gen();
        let mut curve = Modulator::new(ModulatorKind::Curve);
        curve.shape = crate::model::Shape::from_points(&[(0.0, 1.0, 0.0), (0.5, 0.0, 0.0), (1.0, 0.0, 0.0)]);
        curve.targets[0] = ModTarget { dest: ModularParams::gen_param(0, 0) as u16, depth: -1.0 };
        p.modulators[0] = curve;
        let mut synth = ModSynth::new(p, SR);
        synth.note_on(60, 0.1, HELD);
        let (mut l, mut r) = (vec![0.0; 4800], vec![0.0; 4800]);
        synth.render(&[], &[], &mut l, &mut r);
        assert!(rms(&l[2400..]) < 0.01);
        assert_eq!(MOD_NO_TARGET, u16::MAX);
    }

    #[test]
    fn mono_glide_and_polyphony() {
        let mut p = ModularParams::default();
        p.gens[0] = sine_gen();
        let (chord, _) = play(p, &[48, 52, 55], 4800);
        assert!(rms(&chord) > 0.1);
        p.mono = true;
        p.glide = 100.0;
        let mut synth = ModSynth::new(p, SR);
        synth.note_on(57, 1.0, HELD);
        synth.note_on(69, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        synth.render(&[], &[], &mut l, &mut r);
        assert_eq!(synth.voices.iter().filter(|v| v.active).count(), 1);
        assert!((frequency(&l[24_000..]) - 440.0).abs() < 3.0);
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    use crate::model::{Generator, Modifier};
    use crate::synth::HELD;

    fn cost(gens: usize, unison: f32, modifiers: usize, voices: usize) -> f64 {
        let mut p = ModularParams::default();
        for g in 0..gens {
            p.gens[g] = Generator { unison, ..Generator::new(GenKind::Analog) };
            let chain = [
                Modifier { kind: ModifierKind::FmFrom, source: ((g + 1) % gens) as u8, ..Modifier::OFF },
                Modifier { kind: ModifierKind::Filter, filter: WaveFilter::Ladder, ..Modifier::OFF },
                Modifier { kind: ModifierKind::Distortion, ..Modifier::OFF },
                Modifier { kind: ModifierKind::Filter, ..Modifier::OFF },
            ];
            for k in 0..modifiers {
                p.gens[g].modifiers[k] = chain[k];
            }
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
        start.elapsed().as_secs_f64()
    }

    #[test]
    #[ignore = "benchmark: prints how much of real time a worst-case patch takes"]
    fn worst_case_cost() {
        let worst = cost(MOD_GENS, 8.0, 4, VOICES);
        let heavy = cost(4, 4.0, 2, 8);
        println!("peor caso (8 gen × 8 unísono × 4 mods, 12 voces): {:.0} % de tiempo real", worst * 100.0);
        println!("patch pesado (4 gen × 4 unísono × 2 mods, 8 voces): {:.0} % de tiempo real", heavy * 100.0);
    }
}
