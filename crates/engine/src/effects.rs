//! Stereo audio effects. Constructors allocate (delay lines, reverb buffers) and
//! must run off the audio thread; `set_params` and `process` never allocate.

use crate::model::{
    db_to_gain, AutoFilterParams, BitcrusherParams, ChorusParams, CompressorParams, DelayParams, DuckerParams, Effect,
    Eq3Params, FilterMode, LimiterParams, PhaserParams, ReverbParams, SaturatorParams, UtilityParams,
};
use std::f32::consts::{PI, TAU};

/// Timing information effects may need (tempo-synced delay, LFOs).
#[derive(Clone, Copy)]
pub struct ProcessContext {
    pub sample_rate: f32,
    pub tempo: f64,
    /// Transport position at the first sample of the block.
    pub beat: f64,
    /// Beats advanced per sample; 0 while the transport is stopped.
    pub beats_per_sample: f64,
}

/// Topology-preserving-transform state variable filter returning (low, band, high).
#[derive(Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

#[derive(Clone, Copy)]
struct SvfCoefs {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl SvfCoefs {
    fn new(cutoff: f32, k: f32, sample_rate: f32) -> Self {
        let g = (PI * cutoff.clamp(20.0, sample_rate * 0.45) / sample_rate).tan();
        let a1 = 1.0 / (1.0 + g * (g + k));
        Self {
            a1,
            a2: g * a1,
            a3: g * g * a1,
            k,
        }
    }
}

impl Svf {
    fn tick(&mut self, x: f32, c: &SvfCoefs) -> (f32, f32, f32) {
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, x - c.k * v1 - v2)
    }
}

const BUTTERWORTH_K: f32 = std::f32::consts::SQRT_2;

pub struct Eq3 {
    params: Eq3Params,
    low: [Svf; 2],
    high: [Svf; 2],
}

impl Eq3 {
    fn process(&mut self, l: &mut [f32], r: &mut [f32], sr: f32) {
        let p = self.params;
        let low_c = SvfCoefs::new(p.low_freq, BUTTERWORTH_K, sr);
        let high_c = SvfCoefs::new(p.high_freq, BUTTERWORTH_K, sr);
        // At the bottom of the range a band is fully killed, as on a DJ mixer.
        let gain = |db: f32| if db <= -40.0 { 0.0 } else { db_to_gain(db) };
        let (gl, gm, gh) = (gain(p.low_db), gain(p.mid_db), gain(p.high_db));
        for (ch, buf) in [l, r].into_iter().enumerate() {
            for s in buf.iter_mut() {
                let x = *s;
                let (low, _, _) = self.low[ch].tick(x, &low_c);
                let (_, _, high) = self.high[ch].tick(x, &high_c);
                let mid = x - low - high;
                *s = low * gl + mid * gm + high * gh;
            }
        }
    }
}

pub struct Compressor {
    params: CompressorParams,
    envelope_db: f32,
}

impl Compressor {
    const KNEE_DB: f32 = 6.0;

    fn gain_reduction_db(&self, level_db: f32) -> f32 {
        let p = &self.params;
        let over = level_db - p.threshold_db;
        let slope = 1.0 / p.ratio.max(1.0) - 1.0;
        let knee = Self::KNEE_DB;
        if 2.0 * over < -knee {
            0.0
        } else if 2.0 * over.abs() <= knee {
            slope * (over + knee / 2.0).powi(2) / (2.0 * knee)
        } else {
            slope * over
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], sr: f32) {
        let attack = (-1.0 / (self.params.attack_ms.max(0.05) * 0.001 * sr)).exp();
        let release = (-1.0 / (self.params.release_ms.max(1.0) * 0.001 * sr)).exp();
        let makeup = db_to_gain(self.params.makeup_db);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            // Stereo-linked peak detection.
            let peak = a.abs().max(b.abs()).max(1e-6);
            let target = self.gain_reduction_db(20.0 * peak.log10());
            let coef = if target < self.envelope_db { attack } else { release };
            self.envelope_db = target + coef * (self.envelope_db - target);
            let g = db_to_gain(self.envelope_db) * makeup;
            *a *= g;
            *b *= g;
        }
    }
}

pub struct Delay {
    params: DelayParams,
    lines: [Vec<f32>; 2],
    write: usize,
    /// Smoothed delay time in samples, to avoid zipper noise on tempo changes.
    current: f32,
    damping: [f32; 2],
}

impl Delay {
    const MAX_SECONDS: f32 = 4.0;

    fn new(params: DelayParams, sr: f32) -> Self {
        let len = (Self::MAX_SECONDS * sr) as usize + 2;
        Self {
            params,
            lines: [vec![0.0; len], vec![0.0; len]],
            write: 0,
            current: 0.0,
            damping: [0.0; 2],
        }
    }

    fn read(line: &[f32], write: usize, delay: f32) -> f32 {
        let len = line.len();
        let pos = write as f32 + len as f32 - delay;
        let i = pos.floor() as usize;
        let frac = pos - pos.floor();
        let a = line[i % len];
        let b = line[(i + 1) % len];
        a + (b - a) * frac
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessContext) {
        let p = self.params;
        let sr = ctx.sample_rate;
        let len = self.lines[0].len();
        let target = ((p.time_beats * 60.0 / ctx.tempo) as f32 * sr).clamp(1.0, (len - 2) as f32);
        if self.current <= 0.0 {
            self.current = target;
        }
        let smooth = 1.0 - (-1.0 / (0.05 * sr)).exp();
        let damp = (-TAU * p.filter_hz.clamp(200.0, 20000.0) / sr).exp();
        let feedback = p.feedback.clamp(0.0, 0.95);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            self.current += (target - self.current) * smooth;
            let wet_l = Self::read(&self.lines[0], self.write, self.current);
            let wet_r = Self::read(&self.lines[1], self.write, self.current);
            self.damping[0] = wet_l + damp * (self.damping[0] - wet_l);
            self.damping[1] = wet_r + damp * (self.damping[1] - wet_r);
            let (fb_l, fb_r) = (self.damping[0] * feedback, self.damping[1] * feedback);
            let (in_l, in_r) = if p.ping_pong {
                // Mono input enters the left line and bounces between sides.
                (0.5 * (*a + *b) + fb_r, fb_l)
            } else {
                (*a + fb_l, *b + fb_r)
            };
            self.lines[0][self.write] = in_l;
            self.lines[1][self.write] = in_r;
            self.write = (self.write + 1) % len;
            *a += (wet_l - *a) * p.mix;
            *b += (wet_r - *b) * p.mix;
        }
    }
}

struct Comb {
    buffer: Vec<f32>,
    index: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)],
            index: 0,
            store: 0.0,
        }
    }

    fn tick(&mut self, input: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buffer[self.index];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buffer[self.index] = input + self.store * feedback;
        self.index = (self.index + 1) % self.buffer.len();
        out
    }
}

struct Allpass {
    buffer: Vec<f32>,
    index: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)],
            index: 0,
        }
    }

    fn tick(&mut self, input: f32) -> f32 {
        let delayed = self.buffer[self.index];
        self.buffer[self.index] = input + delayed * 0.5;
        self.index = (self.index + 1) % self.buffer.len();
        delayed - input
    }
}

/// Freeverb-style Schroeder/Moorer reverb.
pub struct Reverb {
    params: ReverbParams,
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
}

impl Reverb {
    const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
    const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
    const SPREAD: usize = 23;

    fn new(params: ReverbParams, sr: f32) -> Self {
        let scale = sr / 44100.0;
        let len = |n: usize, spread: usize| ((n + spread) as f32 * scale) as usize;
        let combs = [0, Self::SPREAD].map(|s| Self::COMBS.iter().map(|&n| Comb::new(len(n, s))).collect());
        let allpasses = [0, Self::SPREAD].map(|s| Self::ALLPASSES.iter().map(|&n| Allpass::new(len(n, s))).collect());
        Self {
            params,
            combs,
            allpasses,
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = self.params;
        let feedback = 0.7 + 0.28 * p.size.clamp(0.0, 1.0);
        let damp = 0.4 * p.damping.clamp(0.0, 1.0);
        let wet1 = 0.5 * (1.0 + p.width);
        let wet2 = 0.5 * (1.0 - p.width);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let input = (*a + *b) * 0.015;
            let mut outs = [0.0f32; 2];
            for ch in 0..2 {
                let mut acc = 0.0;
                for comb in &mut self.combs[ch] {
                    acc += comb.tick(input, feedback, damp);
                }
                for ap in &mut self.allpasses[ch] {
                    acc = ap.tick(acc);
                }
                outs[ch] = acc * 3.0;
            }
            let wet_l = outs[0] * wet1 + outs[1] * wet2;
            let wet_r = outs[1] * wet1 + outs[0] * wet2;
            *a += (wet_l - *a) * p.mix;
            *b += (wet_r - *b) * p.mix;
        }
    }
}

pub struct Saturator {
    params: SaturatorParams,
}

impl Saturator {
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = self.params;
        let drive = db_to_gain(p.drive_db);
        let out = db_to_gain(p.output_db);
        for s in l.iter_mut().chain(r.iter_mut()) {
            let wet = (*s * drive).tanh() * out;
            *s += (wet - *s) * p.mix;
        }
    }
}

pub struct AutoFilter {
    params: AutoFilterParams,
    filters: [Svf; 2],
}

impl AutoFilter {
    const CONTROL_RATE: usize = 16;

    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessContext) {
        let p = self.params;
        let k = 2.0 - 1.95 * p.resonance.clamp(0.0, 1.0);
        let mut offset = 0;
        for (ca, cb) in l.chunks_mut(Self::CONTROL_RATE).zip(r.chunks_mut(Self::CONTROL_RATE)) {
            // LFO locked to the transport so it stays in time with the music.
            let beat = ctx.beat + offset as f64 * ctx.beats_per_sample;
            let phase = (beat / p.lfo_beats.max(0.0625)).fract() as f32;
            let lfo = (phase * TAU).sin();
            let cutoff = p.cutoff * 2f32.powf(lfo * p.lfo_amount);
            let c = SvfCoefs::new(cutoff, k, ctx.sample_rate);
            for (ch, buf) in [ca, cb].into_iter().enumerate() {
                for s in buf.iter_mut() {
                    let (low, band, high) = self.filters[ch].tick(*s, &c);
                    *s = match p.mode {
                        FilterMode::LowPass => low,
                        FilterMode::HighPass => high,
                        FilterMode::BandPass => band,
                    };
                }
            }
            offset += Self::CONTROL_RATE;
        }
    }
}

pub struct Chorus {
    params: ChorusParams,
    lines: [Vec<f32>; 2],
    write: usize,
    phase: f32,
}

impl Chorus {
    fn new(params: ChorusParams, sr: f32) -> Self {
        let len = (0.05 * sr) as usize + 4;
        Self { params, lines: [vec![0.0; len], vec![0.0; len]], write: 0, phase: 0.0 }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], sr: f32) {
        let p = self.params;
        let len = self.lines[0].len();
        let base = (p.delay_ms * 0.001 * sr).clamp(1.0, len as f32 - 2.0);
        let depth = p.depth * (base * 0.9).min(0.005 * sr);
        let inc = p.rate / sr;
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            self.lines[0][self.write] = *a;
            self.lines[1][self.write] = *b;
            self.phase = (self.phase + inc) % 1.0;
            // Left and right LFOs a quarter cycle apart give the stereo ensemble.
            let mod_l = (self.phase * TAU).sin();
            let mod_r = ((self.phase + 0.25) * TAU).sin();
            let wet_l = Delay::read(&self.lines[0], self.write, base + depth * mod_l);
            let wet_r = Delay::read(&self.lines[1], self.write, base + depth * mod_r);
            self.write = (self.write + 1) % len;
            *a += (wet_l - *a) * p.mix;
            *b += (wet_r - *b) * p.mix;
        }
    }
}

const PHASER_STAGES: usize = 6;

pub struct Phaser {
    params: PhaserParams,
    /// First-order allpass state (x1, y1) per channel and stage.
    stages: [[(f32, f32); PHASER_STAGES]; 2],
    feedback: [f32; 2],
    phase: f32,
}

impl Phaser {
    fn process(&mut self, l: &mut [f32], r: &mut [f32], sr: f32) {
        let p = self.params;
        let inc = p.rate / sr;
        let mut coefs = [0f32; 2];
        for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            if i % 16 == 0 {
                // The notches sweep up to two octaves either side of the center.
                for (ch, coef) in coefs.iter_mut().enumerate() {
                    let lfo = ((self.phase + ch as f32 * 0.25) * TAU).sin();
                    let freq = (p.center * 2f32.powf(2.0 * p.depth * lfo)).clamp(20.0, sr * 0.45);
                    let t = (PI * freq / sr).tan();
                    *coef = (t - 1.0) / (t + 1.0);
                }
            }
            self.phase = (self.phase + inc) % 1.0;
            for (ch, sample) in [a, b].into_iter().enumerate() {
                let mut x = *sample + self.feedback[ch] * p.feedback;
                for (x1, y1) in self.stages[ch].iter_mut() {
                    let y = coefs[ch] * x + *x1 - coefs[ch] * *y1;
                    *x1 = x;
                    *y1 = y;
                    x = y;
                }
                self.feedback[ch] = x;
                *sample += (x - *sample) * p.mix;
            }
        }
    }
}

pub struct Bitcrusher {
    params: BitcrusherParams,
    counter: f32,
    held: [f32; 2],
}

impl Bitcrusher {
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = self.params;
        let levels = 2f32.powf(p.bits.clamp(1.0, 24.0) - 1.0);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            self.counter += 1.0;
            if self.counter >= p.downsample.max(1.0) {
                self.counter -= p.downsample.max(1.0);
                self.held = [(*a * levels).round() / levels, (*b * levels).round() / levels];
            }
            *a += (self.held[0] - *a) * p.mix;
            *b += (self.held[1] - *b) * p.mix;
        }
    }
}

pub struct Ducker {
    params: DuckerParams,
}

impl Ducker {
    /// Fraction of the period used to duck down, short enough to feel instant without clicking.
    const ATTACK: f64 = 0.015;

    fn gain(&self, phase: f64) -> f32 {
        let p = self.params;
        let duck = if phase < Self::ATTACK {
            phase / Self::ATTACK
        } else {
            let t = ((phase - Self::ATTACK) / p.release.max(0.05) as f64).min(1.0);
            1.0 - t * t * (3.0 - 2.0 * t)
        };
        1.0 - p.depth * duck as f32
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessContext) {
        // Synced to the transport: nothing to pump while stopped.
        if ctx.beats_per_sample <= 0.0 {
            return;
        }
        let period = self.params.rate_beats.max(0.0625);
        for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            let beat = ctx.beat + i as f64 * ctx.beats_per_sample;
            let g = self.gain((beat / period).rem_euclid(1.0));
            *a *= g;
            *b *= g;
        }
    }
}

/// Lookahead peak limiter: the gain drops before a peak arrives, so the output never
/// exceeds the ceiling. Adds 1.5 ms of latency to its channel.
pub struct Limiter {
    params: LimiterParams,
    delay: [Vec<f32>; 2],
    position: usize,
    gain: f32,
}

impl Limiter {
    fn new(params: LimiterParams, sr: f32) -> Self {
        let lookahead = ((0.0015 * sr) as usize).max(1);
        Self { params, delay: [vec![0.0; lookahead], vec![0.0; lookahead]], position: 0, gain: 1.0 }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], sr: f32) {
        let p = self.params;
        let input = db_to_gain(p.gain_db);
        let ceiling = db_to_gain(p.ceiling_db).max(1e-4);
        let release = (-1.0 / (p.release_ms.max(1.0) * 0.001 * sr)).exp();
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let (out_l, out_r) = (self.delay[0][self.position], self.delay[1][self.position]);
            self.delay[0][self.position] = *a * input;
            self.delay[1][self.position] = *b * input;
            self.position = (self.position + 1) % self.delay[0].len();
            // The loudest sample still to come (or going out now) sets the gain.
            let peak = self.delay[0]
                .iter()
                .chain(&self.delay[1])
                .fold(out_l.abs().max(out_r.abs()), |m, s| m.max(s.abs()));
            let target = if peak > ceiling { ceiling / peak } else { 1.0 };
            self.gain = if target < self.gain { target } else { target + (self.gain - target) * release };
            *a = (out_l * self.gain).clamp(-ceiling, ceiling);
            *b = (out_r * self.gain).clamp(-ceiling, ceiling);
        }
    }
}

pub struct Utility {
    params: UtilityParams,
}

impl Utility {
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let gain = db_to_gain(self.params.gain_db);
        let width = self.params.width.clamp(0.0, 2.0);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let mid = (*a + *b) * 0.5;
            let side = (*a - *b) * 0.5 * width;
            *a = (mid + side) * gain;
            *b = (mid - side) * gain;
        }
    }
}

pub enum EffectProcessor {
    Eq3(Eq3),
    Compressor(Compressor),
    Delay(Delay),
    Reverb(Reverb),
    Saturator(Saturator),
    AutoFilter(AutoFilter),
    Chorus(Chorus),
    Phaser(Phaser),
    Bitcrusher(Bitcrusher),
    Ducker(Ducker),
    Limiter(Limiter),
    Utility(Utility),
}

impl EffectProcessor {
    /// Allocates the effect's buffers; call off the audio thread.
    pub fn new(effect: &Effect, sample_rate: f32) -> Self {
        match *effect {
            Effect::Eq3(params) => Self::Eq3(Eq3 {
                params,
                low: Default::default(),
                high: Default::default(),
            }),
            Effect::Compressor(params) => Self::Compressor(Compressor {
                params,
                envelope_db: 0.0,
            }),
            Effect::Delay(params) => Self::Delay(Delay::new(params, sample_rate)),
            Effect::Reverb(params) => Self::Reverb(Reverb::new(params, sample_rate)),
            Effect::Saturator(params) => Self::Saturator(Saturator { params }),
            Effect::AutoFilter(params) => Self::AutoFilter(AutoFilter {
                params,
                filters: Default::default(),
            }),
            Effect::Chorus(params) => Self::Chorus(Chorus::new(params, sample_rate)),
            Effect::Phaser(params) => Self::Phaser(Phaser {
                params,
                stages: [[(0.0, 0.0); PHASER_STAGES]; 2],
                feedback: [0.0; 2],
                phase: 0.0,
            }),
            Effect::Bitcrusher(params) => Self::Bitcrusher(Bitcrusher { params, counter: 0.0, held: [0.0; 2] }),
            Effect::Ducker(params) => Self::Ducker(Ducker { params }),
            Effect::Limiter(params) => Self::Limiter(Limiter::new(params, sample_rate)),
            Effect::Utility(params) => Self::Utility(Utility { params }),
        }
    }

    /// Updates parameters in place. Returns false if `effect` is a different type.
    pub fn set_params(&mut self, effect: &Effect) -> bool {
        match (self, *effect) {
            (Self::Eq3(fx), Effect::Eq3(p)) => fx.params = p,
            (Self::Compressor(fx), Effect::Compressor(p)) => fx.params = p,
            (Self::Delay(fx), Effect::Delay(p)) => fx.params = p,
            (Self::Reverb(fx), Effect::Reverb(p)) => fx.params = p,
            (Self::Saturator(fx), Effect::Saturator(p)) => fx.params = p,
            (Self::AutoFilter(fx), Effect::AutoFilter(p)) => fx.params = p,
            (Self::Chorus(fx), Effect::Chorus(p)) => fx.params = p,
            (Self::Phaser(fx), Effect::Phaser(p)) => fx.params = p,
            (Self::Bitcrusher(fx), Effect::Bitcrusher(p)) => fx.params = p,
            (Self::Ducker(fx), Effect::Ducker(p)) => fx.params = p,
            (Self::Limiter(fx), Effect::Limiter(p)) => fx.params = p,
            (Self::Utility(fx), Effect::Utility(p)) => fx.params = p,
            _ => return false,
        }
        true
    }

    /// Current parameters as a model value.
    pub(crate) fn effect(&self) -> Effect {
        match self {
            Self::Eq3(fx) => Effect::Eq3(fx.params),
            Self::Compressor(fx) => Effect::Compressor(fx.params),
            Self::Delay(fx) => Effect::Delay(fx.params),
            Self::Reverb(fx) => Effect::Reverb(fx.params),
            Self::Saturator(fx) => Effect::Saturator(fx.params),
            Self::AutoFilter(fx) => Effect::AutoFilter(fx.params),
            Self::Chorus(fx) => Effect::Chorus(fx.params),
            Self::Phaser(fx) => Effect::Phaser(fx.params),
            Self::Bitcrusher(fx) => Effect::Bitcrusher(fx.params),
            Self::Ducker(fx) => Effect::Ducker(fx.params),
            Self::Limiter(fx) => Effect::Limiter(fx.params),
            Self::Utility(fx) => Effect::Utility(fx.params),
        }
    }

    /// Sets one continuous parameter (see [`Effect::params`]); used by automation.
    pub fn set_param(&mut self, index: usize, value: f32) {
        let mut effect = self.effect();
        effect.set(index, value);
        self.set_params(&effect);
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessContext) {
        match self {
            Self::Eq3(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Compressor(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Delay(fx) => fx.process(l, r, ctx),
            Self::Reverb(fx) => fx.process(l, r),
            Self::Saturator(fx) => fx.process(l, r),
            Self::AutoFilter(fx) => fx.process(l, r, ctx),
            Self::Chorus(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Phaser(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Bitcrusher(fx) => fx.process(l, r),
            Self::Ducker(fx) => fx.process(l, r, ctx),
            Self::Limiter(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Utility(fx) => fx.process(l, r),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ProcessContext {
        ProcessContext {
            sample_rate: 48_000.0,
            tempo: 120.0,
            beat: 0.0,
            beats_per_sample: 120.0 / 60.0 / 48_000.0,
        }
    }

    /// Feeds an impulse followed by silence and checks the output stays finite and bounded.
    #[test]
    fn every_effect_is_stable_on_an_impulse() {
        for effect in Effect::catalog() {
            let mut fx = EffectProcessor::new(&effect, 48_000.0);
            let mut peak = 0f32;
            for block in 0..400 {
                let mut l = vec![0.0; 256];
                let mut r = vec![0.0; 256];
                if block == 0 {
                    l[0] = 1.0;
                    r[0] = 1.0;
                }
                fx.process(&mut l, &mut r, &ctx());
                for s in l.iter().chain(&r) {
                    assert!(s.is_finite(), "{} produced a non-finite sample", effect.label());
                    peak = peak.max(s.abs());
                }
            }
            assert!(peak < 4.0, "{} peaked at {peak}", effect.label());
        }
    }

    #[test]
    fn eq3_is_transparent_at_unity() {
        let mut fx = EffectProcessor::new(&Effect::Eq3(Default::default()), 48_000.0);
        let input: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let mut l = input.clone();
        let mut r = input.clone();
        fx.process(&mut l, &mut r, &ctx());
        let error: f32 = l.iter().zip(&input).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(error < 1e-3, "EQ Three at 0 dB should not change the signal ({error})");
    }

    #[test]
    fn limiter_never_exceeds_its_ceiling() {
        let params = crate::model::LimiterParams { gain_db: 12.0, ceiling_db: -1.0, release_ms: 50.0 };
        let mut fx = EffectProcessor::new(&Effect::Limiter(params), 48_000.0);
        let mut l: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.03).sin() * if i % 7000 < 50 { 1.0 } else { 0.4 }).collect();
        let mut r = l.clone();
        fx.process(&mut l, &mut r, &ctx());
        let ceiling = 10f32.powf(-1.0 / 20.0);
        let peak = l.iter().chain(&r).fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak <= ceiling + 1e-6, "peak {peak} over ceiling {ceiling}");
        assert!(peak > ceiling * 0.9, "the +12 dB input should reach the ceiling");
    }

    #[test]
    fn ducker_pumps_on_the_beat_and_recovers() {
        let mut fx = EffectProcessor::new(&Effect::Ducker(Default::default()), 48_000.0);
        // One beat at 120 BPM: half a second.
        let mut l = vec![1.0f32; 24_000];
        let mut r = l.clone();
        fx.process(&mut l, &mut r, &ctx());
        let depth = crate::model::DuckerParams::default().depth;
        let lowest = l.iter().copied().fold(1.0, f32::min);
        assert!((lowest - (1.0 - depth)).abs() < 0.02, "ducks to {lowest}");
        assert!((l[23_999] - 1.0).abs() < 1e-3, "back to full level by the end of the beat");
        // Stopped transport: untouched.
        let stopped = ProcessContext { beats_per_sample: 0.0, ..ctx() };
        let mut l = vec![1.0f32; 512];
        let mut r = l.clone();
        fx.process(&mut l, &mut r, &stopped);
        assert!(l.iter().all(|&s| s == 1.0));
    }

    #[test]
    fn utility_width_zero_is_mono() {
        let mut fx = EffectProcessor::new(&Effect::Utility(crate::model::UtilityParams { gain_db: 0.0, width: 0.0 }), 48_000.0);
        let (mut l, mut r) = (vec![1.0, 0.0], vec![0.0, 1.0]);
        fx.process(&mut l, &mut r, &ctx());
        assert_eq!((l, r), (vec![0.5, 0.5], vec![0.5, 0.5]));
    }

    #[test]
    fn compressor_reduces_loud_signals() {
        let mut fx = EffectProcessor::new(&Effect::Compressor(Default::default()), 48_000.0);
        let mut l = vec![0.9f32; 48_000];
        let mut r = l.clone();
        fx.process(&mut l, &mut r, &ctx());
        assert!(l[47_999] < 0.5, "expected gain reduction, got {}", l[47_999]);
    }
}
