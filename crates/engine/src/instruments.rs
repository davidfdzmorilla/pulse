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

    /// Advances one sample; true when a timed gate just closed.
    pub(crate) fn tick(&mut self) -> bool {
        if self.held || self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        self.remaining == 0
    }
}

/// Picks the voice to (re)use: an idle one, else the oldest.
pub(crate) fn free_voice(idle: impl Iterator<Item = (usize, bool, u64)>) -> usize {
    let mut best = (0, false, u64::MAX);
    for (index, is_idle, age) in idle {
        if is_idle {
            return index;
        }
        if age < best.2 {
            best = (index, false, age);
        }
    }
    best.0
}

// ----- Super Saw ------------------------------------------------------------------------

const SAW_VOICES: usize = 8;
const UNISON: usize = 7;
const CONTROL: usize = 32;

#[derive(Clone, Copy)]
struct SawVoice {
    gate: Gate,
    env: Adsr,
    phases: [f32; UNISON],
    filters: [Svf; 2],
}

pub struct SuperSaw {
    params: SuperSawParams,
    voices: [SawVoice; SAW_VOICES],
    sample_rate: f32,
    counter: u64,
    seed: u32,
    /// Pitch bend and vibrato, as a frequency ratio.
    pitch: f32,
}

impl SuperSaw {
    pub fn new(params: SuperSawParams, sample_rate: f32) -> Self {
        let voice = SawVoice { gate: Gate::OFF, env: Adsr::IDLE, phases: [0.0; UNISON], filters: [Svf::default(); 2] };
        Self { params, voices: [voice; SAW_VOICES], sample_rate, counter: 0, seed: 0x2545_F491, pitch: 1.0 }
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
        let index = free_voice(self.voices.iter().enumerate().map(|(i, v)| (i, v.env.stage == Stage::Idle, v.gate.age)));
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
        voice.gate = Gate::new(pitch, velocity, gate, self.counter);
        voice.phases = phases;
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
                incs[u] = freq * semitones(position * p.detune * 0.5) / sr;
                let angle = (position * p.spread + 1.0) * std::f32::consts::FRAC_PI_4;
                gains[u] = (angle.cos() * norm, angle.sin() * norm);
            }
            for (block_l, block_r) in left.chunks_mut(CONTROL).zip(right.chunks_mut(CONTROL)) {
                let cutoff = p.cutoff * 2f32.powf(p.env_amount * voice.env.level);
                let g = svf_g(cutoff, sr);
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
        }
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
        }
    }

    /// Bends every note by `ratio` (a frequency multiplier: pitch bend and vibrato).
    pub fn set_pitch(&mut self, ratio: f32) {
        self.pitch = ratio;
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

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        if !self.gate_open && self.amp < 1e-5 {
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
            let cutoff = (p.cutoff * 2f32.powf(sweep)).clamp(20.0, sr * 0.4);
            let g = 1.0 - (-TAU * cutoff / sr).exp();
            // Four one-pole stages with resonant feedback and a saturating input.
            let input = (osc * drive - feedback * self.ladder[3]).tanh();
            self.ladder[0] += g * (input - self.ladder[0]);
            self.ladder[1] += g * (self.ladder[0] - self.ladder[1]);
            self.ladder[2] += g * (self.ladder[1] - self.ladder[2]);
            self.ladder[3] += g * (self.ladder[2] - self.ladder[3]);
            // Resonance thins the low end; compensate a little.
            let out = self.ladder[3] * (1.0 + p.resonance) * self.amp * level;
            *out_l += out;
            *out_r += out;
        }
    }
}

// ----- FM Synth -------------------------------------------------------------------------

const FM_VOICES: usize = 8;

#[derive(Clone, Copy)]
struct FmVoice {
    gate: Gate,
    env: Adsr,
    carrier: f32,
    modulator: f32,
    last_mod: f32,
    mod_env: f32,
}

pub struct FmSynth {
    params: FmParams,
    voices: [FmVoice; FM_VOICES],
    sample_rate: f32,
    counter: u64,
    /// Pitch bend and vibrato, as a frequency ratio.
    pitch: f32,
}

impl FmSynth {
    pub fn new(params: FmParams, sample_rate: f32) -> Self {
        let voice = FmVoice { gate: Gate::OFF, env: Adsr::IDLE, carrier: 0.0, modulator: 0.0, last_mod: 0.0, mod_env: 0.0 };
        Self { params, voices: [voice; FM_VOICES], sample_rate, counter: 0, pitch: 1.0 }
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
        let index = free_voice(self.voices.iter().enumerate().map(|(i, v)| (i, v.env.stage == Stage::Idle, v.gate.age)));
        let voice = &mut self.voices[index];
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
        for voice in self.voices.iter_mut().filter(|v| v.env.stage != Stage::Idle) {
            let freq = midi_to_freq(voice.gate.pitch) * pitch;
            let (carrier_inc, mod_inc) = (freq / sr, freq * p.ratio / sr);
            for (out_l, out_r) in left.iter_mut().zip(right.iter_mut()) {
                if voice.gate.tick() {
                    voice.env.release(p.release, sr);
                }
                let level = voice.env.tick(p.attack, p.decay, p.sustain, sr);
                if voice.env.stage == Stage::Idle {
                    break;
                }
                voice.mod_env = p.mod_sustain + (voice.mod_env - p.mod_sustain) * mod_decay;
                let modulator = (TAU * voice.modulator + p.feedback * PI * voice.last_mod).sin();
                voice.last_mod = modulator;
                let out = (TAU * voice.carrier + p.index * voice.mod_env * modulator).sin();
                voice.carrier = (voice.carrier + carrier_inc) % 1.0;
                voice.modulator = (voice.modulator + mod_inc) % 1.0;
                let s = out * level * voice.gate.velocity * p.gain;
                *out_l += s;
                *out_r += s;
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
    bell_filter: Svf,
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
            bell_filter: Svf::default(),
        }
    }

    pub fn params(&self) -> DrumParams {
        self.params
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
        // The classic machine has its own first eight; the rest are the models'.
        if self.params.model != crate::model::DrumModel::Classic || voice >= 8 {
            let params = self.params;
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
        let params = self.params;
        self.kit.render(&params, left, right);
        let sr = self.sample_rate;
        let dt = 1.0 / sr;
        let p = self.params;
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

        for (out_l, out_r) in left.iter_mut().zip(right.iter_mut()) {
            let mut per = [0f32; crate::model::DRUM_SOUNDS];
            let noise = self.noise();

            // Shared metallic oscillators for the hats and the cowbell.
            let mut metal = 0.0;
            if any_metal {
                for (phase, &f) in self.metal_phases.iter_mut().zip(&METAL) {
                    *phase = (*phase + f * dt * 1.5) % 1.0;
                    metal += if *phase < 0.5 { 1.0 } else { -1.0 };
                }
                metal /= 6.0;
            }

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

            let mut hats = 0.0;
            for voice in [4, 5] {
                let h = &mut self.hits[voice];
                if h.active {
                    hats += (metal * 0.7 + noise * 0.3) * h.env * h.velocity * p.level[voice];
                    h.env *= decays[voice];
                    h.active = h.env > 1e-4;
                }
            }
            if hats != 0.0 || any_metal {
                let (_, _, high) = self.hat_filter.tick(hats, hat_g, 1.2);
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

            let s = crate::drumkit::driven(&per, &p);
            // -6 dB of headroom: kick and clap often land together.
            let out = s * 0.5;
            *out_l += out;
            *out_r += out;
        }
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

    #[test]
    fn held_notes_stop_on_note_off() {
        let mut synth = SuperSaw::new(SuperSawParams::default(), SR);
        synth.note_on(64, 1.0, HELD);
        run(&mut |l, r| synth.render(l, r), 4_800);
        synth.note_off(64);
        let (tail, _) = run(&mut |l, r| synth.render(l, r), 48_000);
        assert!(energy(&tail[tail.len() - 2400..]) < 1e-6);
    }
}
