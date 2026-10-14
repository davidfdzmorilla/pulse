//! Wavetable synthesizer: two wavetable oscillators with unison and warp modes,
//! a sub and a noise oscillator, a multimode filter, three envelopes, four
//! hand-drawn LFOs and a sixteen-slot modulation matrix evaluated per voice.
//!
//! The synth is built on the UI thread (it owns per-voice comb-filter buffers)
//! and handed to the audio thread ready to play; rendering never allocates.

use crate::instruments::{saw, square, svf_g, Gate, Svf};
use crate::model::{
    KickWave, LfoMode, ModSource, OscWarp, ParamInfo, WaveEnv, WaveFilter, WaveParams, WAVE_MOD_TARGETS, WAVE_PARAMS, WAVE_SLOTS,
};
use crate::wavetable::Wavetable;
use std::f32::consts::TAU;
use std::sync::Arc;

const VOICES: usize = 16;
const UNISON: usize = 16;
/// Modulation is evaluated every `CONTROL` samples.
const CONTROL: usize = 32;
const COMB: usize = 2048;
/// Indices into the parameter table (see `WAVE_PARAMS`).
const OSC_PARAMS: usize = 13;
const SUB: usize = 26;
const NOISE: usize = 29;
const FILTER: usize = 32;
const LFOS: usize = 52;
const MACROS: usize = 60;
const GLIDE: usize = 64;
const MASTER: usize = 65;

fn note_freq(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
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
struct Env {
    stage: Stage,
    level: f32,
    held_for: f32,
}

impl Env {
    const IDLE: Env = Env { stage: Stage::Idle, level: 0.0, held_for: 0.0 };

    /// Starts again from the current level, so retriggers do not click.
    fn trigger(&mut self) {
        self.stage = Stage::Attack;
        self.held_for = 0.0;
    }

    fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    fn tick(&mut self, p: &WaveEnv, dt: f32) -> f32 {
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
struct FilterState {
    svf: [Svf; 2],
    ladder: [f32; 4],
    comb: Box<[f32; COMB]>,
    comb_pos: usize,
    formant: [Svf; 3],
}

impl FilterState {
    fn new() -> Self {
        Self { svf: [Svf::default(); 2], ladder: [0.0; 4], comb: Box::new([0.0; COMB]), comb_pos: 0, formant: [Svf::default(); 3] }
    }

    fn clear(&mut self) {
        self.svf = [Svf::default(); 2];
        self.ladder = [0.0; 4];
        self.comb.fill(0.0);
        self.formant = [Svf::default(); 3];
    }
}

/// Filter settings for a control block.
struct FilterCoefs {
    mode: WaveFilter,
    g: f32,
    k: f32,
    ladder_g: f32,
    ladder_feedback: f32,
    comb_delay: usize,
    comb_feedback: f32,
    formants: [(f32, f32); 3],
    formant_k: f32,
    drive: f32,
    mix: f32,
}

const VOWELS: [[f32; 3]; 5] = [[800.0, 1150.0, 2900.0], [400.0, 1600.0, 2700.0], [300.0, 2300.0, 3000.0], [450.0, 800.0, 2830.0], [325.0, 700.0, 2530.0]];

impl FilterCoefs {
    fn new(mode: WaveFilter, cutoff: f32, resonance: f32, drive: f32, mix: f32, sr: f32) -> Self {
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
            comb_delay: ((sr / cutoff) as usize).clamp(1, COMB - 1),
            comb_feedback: resonance * 0.97,
            formants,
            formant_k: 0.6 - resonance * 0.55,
            drive,
            mix,
        }
    }

    fn process(&self, s: &mut FilterState, x: f32) -> f32 {
        let input = if self.drive > 0.0 {
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
            WaveFilter::Comb => {
                let read = (s.comb_pos + COMB - self.comb_delay) % COMB;
                let y = input + self.comb_feedback * s.comb[read];
                s.comb[s.comb_pos] = y.clamp(-4.0, 4.0);
                s.comb_pos = (s.comb_pos + 1) % COMB;
                y * (1.0 - self.comb_feedback * 0.5)
            }
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
        }
    }
}

/// Per-block settings of one oscillator.
struct OscBlock {
    count: usize,
    incs: [f32; UNISON],
    gains: [(f32, f32); UNISON],
    level: usize,
    position: f32,
    warp: OscWarp,
    amount: f32,
}

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
    /// Seconds since the synth started, for free-running LFOs.
    clock: f64,
    /// Notes held in mono mode, oldest first.
    held: [u8; 16],
    held_count: usize,
    last_note: Option<f32>,
    seed: u32,
}

impl WaveSynth {
    pub fn new(params: WaveParams, sample_rate: f32) -> Self {
        let voices = (0..VOICES).map(|i| Voice::new(0x9E37_79B9 ^ (i as u32 * 0x85EB_CA6B))).collect();
        Self { params, sample_rate, tempo: 120.0, voices, counter: 0, clock: 0.0, held: [0; 16], held_count: 0, last_note: None, seed: 0x1357_9BDF }
    }

    pub fn params(&self) -> WaveParams {
        self.params
    }

    pub fn set_params(&mut self, params: WaveParams) {
        self.params = params;
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
        let glide_from = if p.glide > 0.0 { self.last_note } else { None };
        let mut randoms = [[0f32; UNISON]; 2];
        for osc in &mut randoms {
            for r in osc.iter_mut() {
                *r = self.random();
            }
        }
        let random = self.random();
        let voice = &mut self.voices[index];
        let was_active = voice.active;
        voice.gate = Gate::new(pitch, velocity, gate, self.counter);
        voice.target = pitch as f32;
        voice.velocity = velocity;
        if !was_active || retrigger {
            voice.note = glide_from.unwrap_or(pitch as f32);
            voice.random = random;
            voice.age = 0.0;
            for (o, phases) in voice.phases.iter_mut().enumerate() {
                for (u, phase) in phases.iter_mut().enumerate() {
                    *phase = (p.osc[o].phase + p.osc[o].random_phase * randoms[o][u]).fract();
                }
            }
            voice.sub_phase = 0.0;
            for (l, phase) in voice.lfo_phase.iter_mut().enumerate() {
                if p.lfo[l].mode != LfoMode::Free {
                    *phase = 0.0;
                }
            }
            if !was_active {
                voice.filter[0].clear();
                voice.filter[1].clear();
                voice.last = [0.0; 2];
            }
            for env in &mut voice.envs {
                env.trigger();
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

    pub fn note_off(&mut self, pitch: u8) {
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
        for v in &mut self.voices {
            v.gate.held = false;
            for env in &mut v.envs {
                env.release();
            }
        }
    }

    /// Value of a modulation source for a voice, 0..1.
    fn source(&self, voice: &Voice, source: ModSource, lfos: &[f32; 4], macros: &[f32]) -> f32 {
        match source {
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

    pub fn render(&mut self, tables: &[Arc<Wavetable>], left: &mut [f32], right: &mut [f32]) {
        let mut base = [0f32; WAVE_MOD_TARGETS];
        let mut params = self.params;
        for (i, value) in base.iter_mut().enumerate() {
            *value = params.param_mut(i).map_or(0.0, |v| *v);
        }
        let amounts: [f32; WAVE_SLOTS] = std::array::from_fn(|s| self.params.matrix[s].amount);
        let mut offset = 0;
        while offset < left.len() {
            let n = (left.len() - offset).min(CONTROL);
            for v in 0..VOICES {
                if self.voices[v].active {
                    self.render_voice(v, tables, &base, &amounts, &mut left[offset..offset + n], &mut right[offset..offset + n]);
                }
            }
            self.clock += n as f64 / self.sample_rate as f64;
            offset += n;
        }
    }

    fn render_voice(&mut self, v: usize, tables: &[Arc<Wavetable>], base: &[f32; WAVE_MOD_TARGETS], amounts: &[f32; WAVE_SLOTS], left: &mut [f32], right: &mut [f32]) {
        let p = self.params;
        let sr = self.sample_rate;
        let dt = 1.0 / sr;
        let block = left.len() as f32 * dt;

        // Modulation sources at the start of the block.
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
        }
        let macros = &base[MACROS..MACROS + 4];
        let mut m = *base;
        for (s, slot) in p.matrix.iter().enumerate() {
            let dest = slot.dest as usize;
            if slot.source == ModSource::None || dest >= WAVE_MOD_TARGETS {
                continue;
            }
            let mut value = self.source(&self.voices[v], slot.source, &lfos, macros);
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
        let freq = note_freq(voice.note);

        let mut oscs: [Option<OscBlock>; 2] = [None, None];
        for (o, slot) in oscs.iter_mut().enumerate() {
            let osc = &p.osc[o];
            if !osc.on || !tables.get(o).is_some_and(|t| t.frames() > 0) {
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
            for u in 0..count {
                let s = if count == 1 { 0.0 } else { u as f32 / (count - 1) as f32 * 2.0 - 1.0 };
                incs[u] = f * 2f32.powf(s * spread_cents / 1200.0) / sr;
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
            *slot = Some(OscBlock { count, incs, gains, level: Wavetable::level_for(top, sr), position: at(0), warp: osc.warp, amount: at(12) });
        }

        let cutoff = m[FILTER] * 2f32.powf((voice.note - 60.0) / 12.0 * m[FILTER + 4]);
        let coefs = p.filter.on.then(|| FilterCoefs::new(p.filter.mode, cutoff, m[FILTER + 1], m[FILTER + 2], m[FILTER + 3], sr));
        let routes = p.filter.routes;
        let sub_inc = freq * 2f32.powf(m[SUB].round()) / sr;
        let sub_angle = (m[SUB + 2].clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        let sub_gain = (sub_angle.cos() * m[SUB + 1] * std::f32::consts::SQRT_2, sub_angle.sin() * m[SUB + 1] * std::f32::consts::SQRT_2);
        let noise_a = (-TAU * m[NOISE + 1] / sr).exp();
        let noise_angle = (m[NOISE + 2].clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        let noise_gain = (noise_angle.cos() * m[NOISE] * std::f32::consts::SQRT_2, noise_angle.sin() * m[NOISE] * std::f32::consts::SQRT_2);
        let master = m[MASTER];

        for (out_l, out_r) in left.iter_mut().zip(right.iter_mut()) {
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
                let table = &tables[o];
                let fm = if block.warp == OscWarp::Fm { voice.last[1 - o] * block.amount * 0.5 } else { 0.0 };
                let (mut l, mut r, mut mono) = (0.0, 0.0, 0.0);
                for u in 0..block.count {
                    let phase = voice.phases[o][u];
                    let read = warp_phase(block.warp, block.amount, (phase + fm).rem_euclid(1.0));
                    let mut s = table.read(block.level, block.position, read);
                    if block.warp == OscWarp::Quantize && block.amount > 0.0 {
                        let steps = 2f32.powf(1.0 + (1.0 - block.amount) * 7.0);
                        s = (s * steps).round() / steps;
                    }
                    let next = phase + block.incs[u];
                    voice.phases[o][u] = next - next.floor();
                    l += s * block.gains[u].0;
                    r += s * block.gains[u].1;
                    mono += s;
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
            if let Some(c) = &coefs {
                wet_l = c.process(&mut voice.filter[0], wet_l);
                wet_r = c.process(&mut voice.filter[1], wet_r);
            }
            let gain = amp * voice.velocity * master;
            *out_l += (wet_l + dry_l) * gain;
            *out_r += (wet_r + dry_r) * gain;
        }
        voice.age += block;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModSlot, Shape, WaveFilter};
    use crate::synth::HELD;
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
            synth.render(&t, a, b);
        }
        (l, r)
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
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
        synth.render(&t, &mut l, &mut r);
        assert!((frequency(&l[4800..]) - 440.0).abs() < 2.0, "{}", frequency(&l[4800..]));
        synth.note_off(69);
        let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
        synth.render(&t, &mut l, &mut r);
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
        synth.render(&t, &mut l, &mut r);
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
        synth.render(&t, &mut l, &mut r);
        let level_before = synth.voices[0].envs[0].level;
        synth.note_on(69, 1.0, HELD);
        let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        synth.render(&t, &mut l, &mut r);
        assert!(synth.voices[0].envs[0].level >= level_before, "the envelope kept going");
        assert!((frequency(&l[2400..4800]) - 440.0).abs() > 30.0, "still gliding at first");
        assert!((frequency(&l[24_000..]) - 440.0).abs() < 3.0, "arrived");
        assert_eq!(synth.voices.iter().filter(|v| v.active).count(), 1);
        // Releasing the top note goes back to the held one.
        synth.note_off(69);
        l.fill(0.0);
        r.fill(0.0);
        synth.render(&t, &mut l, &mut r);
        assert!((frequency(&l[24_000..]) - 220.0).abs() < 3.0);
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
        synth.render(&t, &mut l, &mut r);
        assert!(rms(&l[..4800]) > 0.05 && rms(&l[19_200..]) < 1e-4);
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
                synth.render(&tables, a, b);
            }
            let peak = l.iter().chain(&r).fold(0f32, |m, s| m.max(s.abs()));
            let rms = (l.iter().map(|s| s * s).sum::<f32>() / l.len() as f32).sqrt();
            assert!(l.iter().chain(&r).all(|s| s.is_finite()), "{name}");
            assert!(rms > 0.01 && peak < 2.0, "{name}: rms {rms}, peak {peak}");
        }
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
            synth.render(&tables, &mut l, &mut r);
        }
        let seconds = start.elapsed().as_secs_f64();
        println!("1 s de audio en {:.1} ms ({:.0} % de tiempo real)", seconds * 1000.0, seconds * 100.0);
    }
}
