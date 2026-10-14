//! Real-time safe sound generators. Nothing in here allocates.

use crate::model::{drum_note, SynthParams, Waveform};
use std::f32::consts::{PI, TAU};

const MAX_VOICES: usize = 16;

pub fn midi_to_freq(pitch: u8) -> f32 {
    440.0 * 2f32.powf((pitch as f32 - 69.0) / 12.0)
}

/// PolyBLEP residual used to band-limit the saw and square discontinuities.
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

/// Topology-preserving-transform state variable filter (low-pass output).
#[derive(Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    fn lowpass(&mut self, x: f32, g: f32, k: f32) -> f32 {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        v2
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Gate length meaning "hold until note-off", for notes played live.
pub const HELD: u32 = 0;

#[derive(Clone, Copy)]
struct Voice {
    stage: Stage,
    pitch: u8,
    /// Sounding until a note-off arrives instead of for a fixed gate.
    held: bool,
    velocity: f32,
    phase: f32,
    phase_inc: f32,
    level: f32,
    release_step: f32,
    gate_remaining: u32,
    age: u64,
    filter: Svf,
}

impl Voice {
    const IDLE: Voice = Voice {
        stage: Stage::Idle,
        pitch: 0,
        held: false,
        velocity: 0.0,
        phase: 0.0,
        phase_inc: 0.0,
        level: 0.0,
        release_step: 0.0,
        gate_remaining: 0,
        age: 0,
        filter: Svf { ic1: 0.0, ic2: 0.0 },
    };

    fn release(&mut self, params: &SynthParams, sr: f32) {
        self.held = false;
        if self.stage != Stage::Idle && self.stage != Stage::Release {
            self.stage = Stage::Release;
            self.release_step = self.level / (params.release.max(0.001) * sr);
        }
    }
}

pub struct PolySynth {
    params: SynthParams,
    voices: [Voice; MAX_VOICES],
    sample_rate: f32,
    note_counter: u64,
}

impl PolySynth {
    pub fn new(params: SynthParams, sample_rate: f32) -> Self {
        Self {
            params,
            voices: [Voice::IDLE; MAX_VOICES],
            sample_rate,
            note_counter: 0,
        }
    }

    pub fn set_params(&mut self, params: SynthParams) {
        self.params = params;
    }

    /// Starts a note that releases on its own after `gate_samples`, or on
    /// [`Self::note_off`] when the gate is [`HELD`].
    pub fn note_on(&mut self, pitch: u8, velocity: f32, gate_samples: u32) {
        self.note_counter += 1;
        // Prefer an idle voice, then the oldest releasing one, then the oldest overall.
        let index = self
            .voices
            .iter()
            .position(|v| v.stage == Stage::Idle)
            .or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.stage == Stage::Release)
                    .min_by_key(|(_, v)| v.age)
                    .map(|(i, _)| i)
            })
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.age)
                    .map(|(i, _)| i)
                    .unwrap_or(0)
            });
        let v = &mut self.voices[index];
        let retrigger = v.stage != Stage::Idle;
        *v = Voice {
            stage: Stage::Attack,
            pitch,
            held: gate_samples == HELD,
            velocity,
            phase: 0.0,
            phase_inc: midi_to_freq(pitch) / self.sample_rate,
            // Start from the current level on a stolen voice to avoid clicks.
            level: if retrigger { v.level } else { 0.0 },
            release_step: 0.0,
            gate_remaining: gate_samples,
            age: self.note_counter,
            filter: if retrigger { v.filter } else { Svf::default() },
        };
    }

    /// Releases held voices playing `pitch`.
    pub fn note_off(&mut self, pitch: u8) {
        let (params, sr) = (self.params, self.sample_rate);
        for v in self.voices.iter_mut().filter(|v| v.held && v.pitch == pitch) {
            v.held = false;
            v.release(&params, sr);
        }
    }

    pub fn all_notes_off(&mut self) {
        let (params, sr) = (self.params, self.sample_rate);
        for v in &mut self.voices {
            v.release(&params, sr);
        }
    }

    /// Adds the synth output to `out` (mono).
    pub fn render(&mut self, out: &mut [f32]) {
        let p = self.params;
        let sr = self.sample_rate;
        let attack_step = 1.0 / (p.attack.max(0.0005) * sr);
        let decay_step = (1.0 - p.sustain) / (p.decay.max(0.001) * sr);
        let cutoff = p.cutoff.clamp(20.0, sr * 0.45);
        let g = (PI * cutoff / sr).tan();
        let k = 2.0 - 1.98 * p.resonance.clamp(0.0, 1.0);

        for v in self.voices.iter_mut().filter(|v| v.stage != Stage::Idle) {
            for sample in out.iter_mut() {
                if !v.held && v.gate_remaining > 0 {
                    v.gate_remaining -= 1;
                    if v.gate_remaining == 0 {
                        v.release(&p, sr);
                    }
                }
                match v.stage {
                    Stage::Attack => {
                        v.level += attack_step;
                        if v.level >= 1.0 {
                            v.level = 1.0;
                            v.stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => {
                        v.level -= decay_step;
                        if v.level <= p.sustain {
                            v.level = p.sustain;
                            v.stage = Stage::Sustain;
                        }
                    }
                    Stage::Sustain => v.level = p.sustain,
                    Stage::Release => {
                        v.level -= v.release_step;
                        if v.level <= 0.0 {
                            v.level = 0.0;
                            v.stage = Stage::Idle;
                        }
                    }
                    Stage::Idle => {}
                }
                if v.stage == Stage::Idle {
                    break;
                }

                let t = v.phase;
                let dt = v.phase_inc;
                let osc = match p.waveform {
                    Waveform::Sine => (t * TAU).sin(),
                    Waveform::Triangle => 1.0 - 4.0 * (t - 0.5).abs(),
                    Waveform::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
                    Waveform::Square => {
                        let sq = if t < 0.5 { 1.0 } else { -1.0 };
                        sq + poly_blep(t, dt) - poly_blep((t + 0.5) % 1.0, dt)
                    }
                };
                v.phase += dt;
                if v.phase >= 1.0 {
                    v.phase -= 1.0;
                }
                let filtered = v.filter.lowpass(osc, g, k);
                *sample += filtered * v.level * v.velocity * p.gain;
            }
        }
    }
}

/// Small xorshift noise source.
#[derive(Clone, Copy)]
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

#[derive(Clone, Copy, Default)]
struct DrumVoice {
    active: bool,
    t: f32,
    velocity: f32,
    env: f32,
    phase: f32,
    hp_prev_in: f32,
    hp_prev_out: f32,
}

impl DrumVoice {
    fn trigger(&mut self, velocity: f32) {
        *self = DrumVoice {
            active: true,
            velocity,
            env: 1.0,
            ..Default::default()
        };
    }

    /// One-pole high-pass, used to thin out noise for snares and hats.
    fn highpass(&mut self, x: f32, coef: f32) -> f32 {
        let y = coef * (self.hp_prev_out + x - self.hp_prev_in);
        self.hp_prev_in = x;
        self.hp_prev_out = y;
        y
    }
}

/// Synthesized drum kit: kick, snare, clap, closed and open hat.
pub struct DrumKit {
    sample_rate: f32,
    noise: Noise,
    kick: DrumVoice,
    snare: DrumVoice,
    clap: DrumVoice,
    closed_hat: DrumVoice,
    open_hat: DrumVoice,
}

impl DrumKit {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            noise: Noise(0x9E37_79B9),
            kick: DrumVoice::default(),
            snare: DrumVoice::default(),
            clap: DrumVoice::default(),
            closed_hat: DrumVoice::default(),
            open_hat: DrumVoice::default(),
        }
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32) {
        match pitch {
            drum_note::KICK | 35 => self.kick.trigger(velocity),
            drum_note::SNARE | 40 => self.snare.trigger(velocity),
            drum_note::CLAP => self.clap.trigger(velocity),
            drum_note::CLOSED_HAT | 44 => {
                // A closed hat chokes the open one, as on a real kit.
                self.open_hat.active = false;
                self.closed_hat.trigger(velocity);
            }
            drum_note::OPEN_HAT => self.open_hat.trigger(velocity),
            _ => {}
        }
    }

    pub fn render(&mut self, out: &mut [f32]) {
        let sr = self.sample_rate;
        let dt = 1.0 / sr;
        let decay = |seconds: f32| (-1.0 / (seconds * sr)).exp();
        let (kick_decay, snare_decay, clap_decay) = (decay(0.35), decay(0.18), decay(0.12));
        let (closed_decay, open_decay) = (decay(0.04), decay(0.35));
        let noise = &mut self.noise;

        for sample in out.iter_mut() {
            let mut s = 0.0;

            let k = &mut self.kick;
            if k.active {
                let freq = 45.0 + 130.0 * (-k.t * 35.0).exp();
                k.phase = (k.phase + freq * dt) % 1.0;
                s += (k.phase * TAU).sin() * k.env * k.velocity * 0.9;
                k.env *= kick_decay;
                k.t += dt;
                k.active = k.env > 1e-4;
            }

            let sn = &mut self.snare;
            if sn.active {
                sn.phase = (sn.phase + 185.0 * dt) % 1.0;
                let tone = (sn.phase * TAU).sin() * (-sn.t * 25.0).exp() * 0.4;
                let n = sn.highpass(noise.next(), 0.85) * sn.env * 0.5;
                s += (tone + n) * sn.velocity;
                sn.env *= snare_decay;
                sn.t += dt;
                sn.active = sn.env > 1e-4;
            }

            let c = &mut self.clap;
            if c.active {
                // Three quick bursts followed by a short tail.
                let burst = if c.t < 0.03 {
                    (1.0 - ((c.t * 100.0) % 1.0)).powi(2)
                } else {
                    1.0
                };
                s += c.highpass(noise.next(), 0.8) * c.env * burst * c.velocity * 0.5;
                c.env *= clap_decay;
                c.t += dt;
                c.active = c.env > 1e-4;
            }

            for (hat, hat_decay) in [
                (&mut self.closed_hat, closed_decay),
                (&mut self.open_hat, open_decay),
            ] {
                if hat.active {
                    s += hat.highpass(noise.next(), 0.6) * hat.env * hat.velocity * 0.3;
                    hat.env *= hat_decay;
                    hat.active = hat.env > 1e-4;
                }
            }

            *sample += s;
        }
    }
}

/// Metronome click.
pub struct Click {
    sample_rate: f32,
    phase: f32,
    freq: f32,
    env: f32,
    /// Samples to wait before the click starts, so it lands on the beat.
    delay: usize,
}

impl Click {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
            freq: 1000.0,
            env: 0.0,
            delay: 0,
        }
    }

    /// Starts a click `delay` samples into the next rendered block.
    pub fn trigger(&mut self, accent: bool, delay: usize) {
        self.freq = if accent { 1600.0 } else { 1000.0 };
        self.phase = 0.0;
        self.env = 1.0;
        self.delay = delay;
    }

    pub fn render(&mut self, out: &mut [f32]) {
        if self.env < 1e-4 {
            return;
        }
        let decay = (-1.0 / (0.015 * self.sample_rate)).exp();
        let start = std::mem::take(&mut self.delay).min(out.len());
        for sample in out[start..].iter_mut() {
            self.phase = (self.phase + self.freq / self.sample_rate) % 1.0;
            *sample += (self.phase * TAU).sin() * self.env * 0.35;
            self.env *= decay;
        }
    }
}
