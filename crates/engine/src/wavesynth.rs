//! Wavetable synthesizer: two wavetable oscillators with unison and warp modes,
//! a sub and a noise oscillator, a multimode filter, three envelopes, four
//! hand-drawn LFOs and a sixteen-slot modulation matrix evaluated per voice.
//!
//! The synth is built on the UI thread (it owns per-voice comb-filter buffers)
//! and handed to the audio thread ready to play; rendering never allocates.

use crate::effects::{EffectProcessor, ProcessContext};
use crate::instruments::{saw, square, svf_g, Gate, Svf};
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
/// Modulation is evaluated every `CONTROL` samples.
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
}

impl FilterState {
    pub(crate) fn new() -> Self {
        Self { svf: [Svf::default(); 2], ladder: [0.0; 4], comb: Some(Box::new([0.0; COMB])), comb_pos: 0, formant: [Svf::default(); 3] }
    }

    pub(crate) fn clear(&mut self) {
        self.svf = [Svf::default(); 2];
        self.ladder = [0.0; 4];
        if let Some(comb) = &mut self.comb {
            comb.fill(0.0);
        }
        self.formant = [Svf::default(); 3];
    }
}

/// Filter settings for a control block.
pub(crate) struct FilterCoefs {
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
            comb_delay: ((sr / cutoff) as usize).clamp(1, COMB - 1),
            comb_feedback: resonance * 0.97,
            formants,
            formant_k: 0.6 - resonance * 0.55,
            drive,
            mix,
        }
    }

    pub(crate) fn process(&self, s: &mut FilterState, x: f32) -> f32 {
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
            WaveFilter::Comb => match &mut s.comb {
                Some(comb) => {
                    let read = (s.comb_pos + COMB - self.comb_delay) % COMB;
                    let y = input + self.comb_feedback * comb[read];
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
    /// The synth's effects rack, built with the synth (allocation happens here).
    fx: Vec<Option<(EffectProcessor, bool)>>,
    scratch: [[f32; CONTROL]; 2],
    arp: Arp,
    seq: Seq,
    /// One sine cycle, for building harmonic tables without calling sin().
    sine: Vec<f32>,
    /// The voice the last note started on (for sequencer slides).
    last_voice: usize,
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

impl WaveSynth {
    pub fn new(params: WaveParams, sample_rate: f32) -> Self {
        let voices = (0..VOICES).map(|i| Voice::new(0x9E37_79B9 ^ (i as u32 * 0x85EB_CA6B))).collect();
        let fx = params.fx.iter().map(|slot| slot.map(|d| (EffectProcessor::new(&d.effect, sample_rate), d.enabled))).collect();
        Self {
            params,
            sample_rate,
            tempo: 120.0,
            voices,
            counter: 0,
            clock: 0.0,
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
            if let (Some((processor, enabled)), Some(device)) = (slot.as_mut(), device) {
                processor.set_params(&device.effect);
                *enabled = device.enabled;
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

    fn play_on(&mut self, pitch: u8, velocity: f32, gate: u32) {
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

    pub fn render(&mut self, samples: &[Arc<Sample>], tables: &[Arc<Wavetable>], left: &mut [f32], right: &mut [f32]) {
        let mut base = [0f32; PARAMS];
        let mut params = self.params;
        for (i, value) in base.iter_mut().enumerate() {
            *value = params.param_mut(i).map_or(0.0, |v| *v);
        }
        let amounts: [f32; WAVE_SLOTS] = std::array::from_fn(|s| self.params.matrix[s].amount);
        let has_fx = self.fx.iter().any(|slot| matches!(slot, Some((_, true))));
        let mut offset = 0;
        while offset < left.len() {
            let n = (left.len() - offset).min(CONTROL);
            self.arp_tick(n);
            self.seq_tick(n);
            let mut scratch = self.scratch;
            scratch[0][..n].fill(0.0);
            scratch[1][..n].fill(0.0);
            {
                let [l, r] = &mut scratch;
                for v in 0..VOICES {
                    if self.voices[v].active {
                        self.render_voice(v, samples, tables, &base, &amounts, &mut l[..n], &mut r[..n]);
                    }
                }
            }
            if has_fx {
                let tempo = self.tempo;
                let ctx = ProcessContext {
                    sample_rate: self.sample_rate,
                    tempo,
                    beat: self.clock * tempo / 60.0,
                    beats_per_sample: tempo / 60.0 / self.sample_rate as f64,
                };
                let [l, r] = &mut scratch;
                for (processor, _) in self.fx.iter_mut().flatten().filter(|(_, enabled)| *enabled) {
                    processor.process(&mut l[..n], &mut r[..n], &ctx);
                }
            }
            for i in 0..n {
                left[offset + i] += scratch[0][i];
                right[offset + i] += scratch[1][i];
            }
            self.scratch = scratch;
            self.clock += n as f64 / self.sample_rate as f64;
            offset += n;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_voice(&mut self, v: usize, samples: &[Arc<Sample>], tables: &[Arc<Wavetable>], base: &[f32; PARAMS], amounts: &[f32; WAVE_SLOTS], left: &mut [f32], right: &mut [f32]) {
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
        let freq = note_freq(voice.note);

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
                    let (current, target) = voice.drift[o][u];
                    let current = current + (target - current) * (1.0 - (-block * 2.0).exp());
                    let target = if (current - target).abs() < 0.05 { voice.random() * 2.0 - 1.0 } else { target };
                    voice.drift[o][u] = (current, target);
                    drift = current * drift_cents;
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
            if osc.kind == OscKind::Harmonic && voice.harmonic_wait == 0 {
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

        let cutoff = m[FILTER] * 2f32.powf((voice.note - 60.0) / 12.0 * m[FILTER + 4]);
        let coefs = p.filter.on.then(|| FilterCoefs::new(p.filter.mode, cutoff, m[FILTER + 1], m[FILTER + 2], m[FILTER + 3], sr));
        let routes = p.filter.routes;
        let cutoff2 = m[WAVE_FILTER2] * 2f32.powf((voice.note - 60.0) / 12.0 * m[WAVE_FILTER2 + 4]);
        let coefs2 = p.filter2.on.then(|| FilterCoefs::new(p.filter2.mode, cutoff2, m[WAVE_FILTER2 + 1], m[WAVE_FILTER2 + 2], m[WAVE_FILTER2 + 3], sr));
        let balance = m[WAVE_BALANCE].clamp(0.0, 1.0);
        let parallel = p.filter_parallel && coefs.is_some() && coefs2.is_some();
        voice.harmonic_wait = if voice.harmonic_wait == 0 { HARMONIC_EVERY } else { voice.harmonic_wait - 1 };
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
                            l += a * block.gains[u].0;
                            r += b * block.gains[u].1;
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
                        let fm = if block.warp == OscWarp::Fm { voice.last[1 - o] * block.amount * 0.5 } else { 0.0 };
                        let morph = block.position.clamp(0.0, 1.0) * 3.0;
                        let width = 0.5 - block.amount.clamp(0.0, 1.0) * 0.45;
                        for u in 0..block.count {
                            let phase = voice.phases[o][u];
                            let inc = block.incs[u];
                            let ph = (phase + fm).rem_euclid(1.0);
                            let sine = (ph * TAU).sin();
                            let triangle = 1.0 - 4.0 * (((ph + 0.25) % 1.0) - 0.5).abs();
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
                            l += value * block.gains[u].0;
                            r += value * block.gains[u].1;
                            mono += value;
                        }
                    }
                    OscKind::Harmonic => {
                        let fm = if block.warp == OscWarp::Fm { voice.last[1 - o] * block.amount * 0.5 } else { 0.0 };
                        let table = &voice.harmonic[o];
                        for u in 0..block.count {
                            let phase = voice.phases[o][u];
                            let x = (phase + fm).rem_euclid(1.0) * HARMONIC_TABLE as f32;
                            let i = x as usize % HARMONIC_TABLE;
                            let a = table[i];
                            let value = a + (table[(i + 1) % HARMONIC_TABLE] - a) * (x - x.floor());
                            let next = phase + block.incs[u];
                            voice.phases[o][u] = next - next.floor();
                            l += value * block.gains[u].0;
                            r += value * block.gains[u].1;
                            mono += value;
                        }
                    }
                    OscKind::Wavetable | OscKind::Spectral => {
                let table = &tables[o];
                let fm = if block.warp == OscWarp::Fm { voice.last[1 - o] * block.amount * 0.5 } else { 0.0 };
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
                let (c1, c2) = (coefs.as_ref().unwrap(), coefs2.as_ref().unwrap());
                let (a_l, a_r) = (c1.process(&mut voice.filter[0], wet_l), c1.process(&mut voice.filter[1], wet_r));
                let (b_l, b_r) = (c2.process(&mut voice.filter2[0], wet_l), c2.process(&mut voice.filter2[1], wet_r));
                wet_l = a_l + (b_l - a_l) * balance;
                wet_r = a_r + (b_r - a_r) * balance;
            } else {
                if let Some(c) = &coefs {
                    wet_l = c.process(&mut voice.filter[0], wet_l);
                    wet_r = c.process(&mut voice.filter[1], wet_r);
                }
                if let Some(c) = &coefs2 {
                    wet_l = c.process(&mut voice.filter2[0], wet_l);
                    wet_r = c.process(&mut voice.filter2[1], wet_r);
                }
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
            synth.render(&[], &t, a, b);
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
