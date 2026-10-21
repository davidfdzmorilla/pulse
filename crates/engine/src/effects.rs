//! Stereo audio effects. Constructors allocate (delay lines, reverb buffers) and
//! must run off the audio thread; `set_params` and `process` never allocate.

use crate::model::{
    db_to_gain, AutoFilterParams, BitcrusherParams, ChorusParams, CompressorParams, DelayParams, DuckerParams, Effect,
    Eq3Params, FilterMode, KickDrive, LimiterParams, PhaserParams, PluginId, ReverbParams, SaturatorParams, UtilityParams,
};
use crate::plugin::PluginProcessor;
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
    key: Key,
}

/// The sound of a sidechain's source for this block (the louder channel, and
/// the mono sum), when the engine has fed it.
pub struct Key {
    level: Vec<f32>,
    mono: Vec<f32>,
    fed: bool,
}

impl Key {
    /// Room for a block; call off the audio thread.
    fn new() -> Self {
        Self { level: vec![0.0; 4096], mono: vec![0.0; 4096], fed: false }
    }

    fn feed(&mut self, left: &[f32], right: &[f32]) {
        let n = left.len().min(right.len()).min(self.level.len());
        for i in 0..n {
            self.level[i] = left[i].abs().max(right[i].abs());
            self.mono[i] = 0.5 * (left[i] + right[i]);
        }
        self.fed = true;
    }

    /// Whether it was fed for this block of `n` (once: the next block needs
    /// feeding again).
    fn take(&mut self, n: usize) -> bool {
        std::mem::replace(&mut self.fed, false) && n <= self.level.len()
    }
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
        let keyed = self.key.take(l.len());
        for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            // Stereo-linked peak detection, of the sidechain's sound if it has one.
            let peak = if keyed { self.key.level[i] } else { a.abs().max(b.abs()) }.max(1e-6);
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
    /// Per channel, for the 4× oversampling.
    over: [Oversampler; 2],
    /// The dry signal, as late as the oversampled wet (its filters' delay).
    dry: [[f32; Oversampler::LATENCY]; 2],
    at: usize,
}

impl Saturator {
    fn new(params: SaturatorParams) -> Self {
        Self { params, over: [Oversampler::new(), Oversampler::new()], dry: [[0.0; Oversampler::LATENCY]; 2], at: 0 }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = self.params;
        let drive = db_to_gain(p.drive_db);
        let out = db_to_gain(p.output_db);
        if !p.oversample {
            for s in l.iter_mut().chain(r.iter_mut()) {
                let wet = shape(*s, p.kind, drive, p.drive_db) * out;
                *s += (wet - *s) * p.mix;
            }
            return;
        }
        for i in 0..l.len() {
            for (ch, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                let x = buf[i];
                let mut up = self.over[ch].up(x);
                for y in &mut up {
                    *y = shape(*y, p.kind, drive, p.drive_db);
                }
                let wet = self.over[ch].down(up) * out;
                let dry = std::mem::replace(&mut self.dry[ch][self.at], x);
                buf[i] = dry + (wet - dry) * p.mix;
            }
            self.at = (self.at + 1) % Oversampler::LATENCY;
        }
    }
}

/// A sample through a saturation `kind` at gain `drive` (`drive_db` in dB).
/// Soft is the tanh the Saturator always had.
fn shape(x: f32, kind: KickDrive, drive: f32, drive_db: f32) -> f32 {
    let x = x * drive;
    match kind {
        KickDrive::Soft => x.tanh(),
        KickDrive::Hard => x.clamp(-1.0, 1.0),
        KickDrive::Fold => (x * PI * 0.5).sin(),
        // The negative half saturates later: even harmonics, warm.
        KickDrive::Tube => {
            if x >= 0.0 { x.tanh() } else { 2.0 * (x * 0.5).tanh() }
        }
        KickDrive::Crush => {
            let steps = 2f32.powf(12.0 - (drive_db / 36.0).clamp(0.0, 1.0) * 10.0);
            (x.clamp(-1.0, 1.0) * steps).round() / steps
        }
    }
}

/// 4× oversampling for a nonlinearity: a 145-tap windowed-sinc low-pass (Kaiser,
/// flat to about 18 kHz, 70 dB down by the base rate's Nyquist) to interpolate
/// up, and the same to filter before coming down. Its delay is a whole number of
/// samples ([`Oversampler::LATENCY`]).
pub struct Oversampler {
    /// The last inputs (a ring).
    input: [f32; Self::PHASE_TAPS],
    input_at: usize,
    /// The last oversampled values (a ring).
    output: [f32; Self::TAPS],
    output_at: usize,
}

impl Oversampler {
    const FACTOR: usize = 4;
    const TAPS: usize = 145;
    const PHASE_TAPS: usize = 37;
    /// Samples (at the base rate) between a sample going in and coming out:
    /// each filter's middle, 72 oversampled samples, twice.
    pub const LATENCY: usize = 36;

    fn new() -> Self {
        Self { input: [0.0; Self::PHASE_TAPS], input_at: 0, output: [0.0; Self::TAPS], output_at: 0 }
    }

    /// The low-pass, at about 21 kHz for 48 kHz (0.44 of the base Nyquist), with
    /// a Kaiser window (β 7).
    fn taps() -> &'static [f32; 145] {
        static TAPS: std::sync::OnceLock<[f32; 145]> = std::sync::OnceLock::new();
        TAPS.get_or_init(|| {
            // The modified Bessel function I0, by its series.
            let bessel = |x: f64| {
                let (mut sum, mut term) = (1.0, 1.0);
                for k in 1..40 {
                    term *= (x / 2.0) / k as f64;
                    sum += term * term;
                }
                sum
            };
            let beta = 7.0;
            let cutoff = 0.875 / 2.0 / Oversampler::FACTOR as f64;
            let middle = (Oversampler::TAPS - 1) as f64 / 2.0;
            let mut taps = [0f32; 145];
            for (n, tap) in taps.iter_mut().enumerate() {
                let t = n as f64 - middle;
                let sinc = if t == 0.0 { 2.0 * cutoff } else { (std::f64::consts::TAU * cutoff * t).sin() / (std::f64::consts::PI * t) };
                let ratio = t / middle;
                let window = bessel(beta * (1.0 - ratio * ratio).max(0.0).sqrt()) / bessel(beta);
                *tap = (sinc * window) as f32;
            }
            // Unity gain at DC.
            let sum: f32 = taps.iter().sum();
            taps.iter_mut().for_each(|t| *t /= sum);
            taps
        })
    }

    /// One input sample as four at the higher rate.
    fn up(&mut self, x: f32) -> [f32; 4] {
        self.input_at = (self.input_at + Self::PHASE_TAPS - 1) % Self::PHASE_TAPS;
        self.input[self.input_at] = x;
        let taps = Self::taps();
        let mut out = [0f32; 4];
        for (phase, y) in out.iter_mut().enumerate() {
            let mut sum = 0.0;
            for j in 0..Self::PHASE_TAPS {
                let k = phase + Self::FACTOR * j;
                if k < Self::TAPS {
                    sum += taps[k] * self.input[(self.input_at + j) % Self::PHASE_TAPS];
                }
            }
            // Zero-stuffing leaves a quarter of the level.
            *y = sum * Self::FACTOR as f32;
        }
        out
    }

    /// Four samples at the higher rate as one at the base rate: taken at the
    /// first of them, so the delay is whole base samples.
    fn down(&mut self, xs: [f32; 4]) -> f32 {
        let mut out = 0.0;
        for (i, x) in xs.into_iter().enumerate() {
            self.output_at = (self.output_at + Self::TAPS - 1) % Self::TAPS;
            self.output[self.output_at] = x;
            if i == 0 {
                let taps = Self::taps();
                out = (0..Self::TAPS).map(|k| taps[k] * self.output[(self.output_at + k) % Self::TAPS]).sum();
            }
        }
        out
    }
}

/// How far the low-pass state of a self-oscillating filter goes (it saturates
/// softly towards it).
const RING_LIMIT: f32 = 2.0;

pub struct AutoFilter {
    params: AutoFilterParams,
    /// Per channel: the resonant stage, then the first of a 24 dB filter.
    filters: [Svf; 2],
    first: [Svf; 2],
    /// The coefficients in use (the resonant stage's, the first stage's) and
    /// samples until the LFO moves them again.
    coefs: Option<(SvfCoefs, Option<SvfCoefs>)>,
    until: usize,
}

impl AutoFilter {
    const CONTROL_RATE: usize = 16;
    /// Resonance from which the filter rings by itself.
    const SELF_OSCILLATION: f32 = 0.95;

    /// The SVF damping for `resonance`: as it always was up to 95 %, then on
    /// down to none and a little under (it rings on its own; its saturation
    /// keeps that level).
    fn damping(resonance: f32) -> f32 {
        let r = resonance.clamp(0.0, 1.2);
        if r <= Self::SELF_OSCILLATION { 2.0 - 1.95 * r } else { 2.0 - 1.95 * Self::SELF_OSCILLATION - (r - Self::SELF_OSCILLATION) * 2.0 }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessContext) {
        let p = self.params;
        let k = Self::damping(p.resonance);
        let ringing = p.resonance > Self::SELF_OSCILLATION;
        let drive = (p.drive_db > 0.0).then(|| db_to_gain(p.drive_db));
        for i in 0..l.len().min(r.len()) {
            // Every CONTROL_RATE samples of its own count (not of the block's,
            // so any buffer size sounds the same): the LFO moves the cutoff.
            if self.until == 0 || self.coefs.is_none() {
                // LFO locked to the transport so it stays in time with the music.
                let beat = ctx.beat + i as f64 * ctx.beats_per_sample;
                let phase = (beat / p.lfo_beats.max(0.0625)).fract() as f32;
                let lfo = (phase * TAU).sin();
                let cutoff = p.cutoff * 2f32.powf(lfo * p.lfo_amount);
                // 24 dB: a first stage without resonance (a fourth-order Butterworth's).
                let first = p.steep.then(|| SvfCoefs::new(cutoff, 1.848, ctx.sample_rate));
                self.coefs = Some((SvfCoefs::new(cutoff, k, ctx.sample_rate), first));
                self.until = Self::CONTROL_RATE;
            }
            self.until -= 1;
            let Some((c, first)) = self.coefs else { continue };
            let first = first.filter(|_| p.steep);
            for (ch, s) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let mut x = *s;
                if let Some(g) = drive {
                    // Driven: louder into a soft clip, about as loud out.
                    x = (x * g).tanh() / g.sqrt();
                }
                if let Some(first) = &first {
                    let (low, band, high) = self.first[ch].tick(x, first);
                    x = match p.mode {
                        FilterMode::LowPass => low,
                        FilterMode::HighPass => high,
                        FilterMode::BandPass => band,
                    };
                }
                let (low, band, high) = self.filters[ch].tick(x, &c);
                if ringing {
                    // Ringing on its own, the resonance's level stays bounded:
                    // both of its states (bounding one alone runs away near the top).
                    let f = &mut self.filters[ch];
                    f.ic1 = f.ic1.tanh();
                    f.ic2 = RING_LIMIT * (f.ic2 / RING_LIMIT).tanh();
                }
                *s = match p.mode {
                    FilterMode::LowPass => low,
                    FilterMode::HighPass => high,
                    FilterMode::BandPass => band,
                };
            }
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
    /// The allpasses' coefficients, and samples until the LFO moves them
    /// again (its own count: any buffer size sounds the same).
    coefs: [f32; 2],
    until: usize,
}

impl Phaser {
    fn process(&mut self, l: &mut [f32], r: &mut [f32], sr: f32) {
        let p = self.params;
        let inc = p.rate / sr;
        let mut coefs = self.coefs;
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            if self.until == 0 {
                self.until = 16;
                // The notches sweep up to two octaves either side of the center.
                for (ch, coef) in coefs.iter_mut().enumerate() {
                    let lfo = ((self.phase + ch as f32 * 0.25) * TAU).sin();
                    let freq = (p.center * 2f32.powf(2.0 * p.depth * lfo)).clamp(20.0, sr * 0.45);
                    let t = (PI * freq / sr).tan();
                    *coef = (t - 1.0) / (t + 1.0);
                }
            }
            self.until -= 1;
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
        self.coefs = coefs;
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
    key: Key,
    hits: Hits,
}

/// Finds the hits in a sidechain: a band of it that jumps well above where
/// it was a moment ago. A new kick shows in its pitch sweep and click even
/// when the last one's tail is still loud, so each kick pumps once.
#[derive(Clone, Copy)]
struct Hits {
    /// Butterworth filters that split the key into four bands: under 150 Hz,
    /// 150–600, 600–3000 and over (the middle ones fall at 24 dB an octave
    /// below, so a loud kick tail stays out of them).
    split: [Svf; 8],
    /// Each band's power, quick and slow.
    fast: [f32; 4],
    slow: [f32; 4],
    /// The key's peak level, for the threshold.
    peak: f32,
    /// The pitch under 600 Hz, from its zero crossings: which side it is on
    /// (past half its level), samples since the last crossing, and the
    /// usual time between them. A kick starts with a sweep from high up, so the
    /// crossings come several times quicker, even when a distorted tail is
    /// as loud in every band as the hit.
    side: f32,
    since_crossing: f32,
    half_period: f32,
    /// Short half periods in a row (two, a whole cycle, make a hit: not one noisy crossing).
    quick: u8,
    armed: bool,
    /// Seconds since the last hit (infinite before the first).
    since: f32,
    /// How far down it already was when the hit came, so a hit during the
    /// release goes on from there instead of jumping up.
    from: f32,
    /// How strong the hit is: 0 at the threshold, 1 from 12 dB above.
    amount: f32,
}

impl Hits {
    const IDLE: Self = Self {
        split: [Svf { ic1: 0.0, ic2: 0.0 }; 8],
        fast: [0.0; 4],
        slow: [0.0; 4],
        peak: 0.0,
        side: 0.0,
        since_crossing: 0.0,
        half_period: f32::INFINITY,
        quick: 0,
        armed: true,
        since: f32::INFINITY,
        from: 0.0,
        amount: 0.0,
    };
    const SPLITS: [f32; 3] = [150.0, 600.0, 3000.0];
    /// A hit is a band's power at least this many times what it was (+6 dB).
    const JUMP: f32 = 4.0;
    /// Seconds to wait before another hit can count.
    const GAP: f32 = 0.05;
    /// Seconds during which a hit's strength can still grow.
    const RISE: f32 = 0.01;
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

    /// How far down (0..1) the pump is `since` seconds after a hit, for a period of `period` seconds.
    fn shape(&self, hits: &Hits, period: f32) -> f32 {
        if !hits.since.is_finite() {
            return 0.0;
        }
        let attack = (Self::ATTACK as f32 * period).min(0.005);
        if hits.since < attack {
            hits.from + (1.0 - hits.from) * hits.since / attack
        } else {
            let t = ((hits.since - attack) / (self.params.release.max(0.05) * period)).min(1.0);
            1.0 - t * t * (3.0 - 2.0 * t)
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessContext) {
        // With a sidechain each hit of that track makes it pump: down at once
        // and back up over `release` of the period, whatever the hit's tail does.
        let p = self.params;
        if self.key.take(l.len()) {
            let sr = ctx.sample_rate;
            let dt = 1.0 / sr;
            let split = Hits::SPLITS.map(|hz| SvfCoefs::new(hz, BUTTERWORTH_K, sr));
            let fast = 1.0 - (-1.0 / (0.005 * sr)).exp();
            let slow = 1.0 - (-1.0 / (0.04 * sr)).exp();
            let (peak_up, peak_down) = (1.0 - (-1.0 / (0.0005 * sr)).exp(), (-1.0 / (0.03 * sr)).exp());
            let threshold = db_to_gain(p.threshold_db);
            // With `lows` only the two bands under 600 Hz count: a kick's body and
            // sweep, not the hats or claps.
            let bands = if p.lows { 2 } else { 4 };
            let period = (p.rate_beats.max(0.0625) * 60.0 / ctx.tempo.max(1.0)) as f32;
            let mut h = self.hits;
            for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                let x = self.key.mono[i];
                let f = &mut h.split;
                let low = f[0].tick(x, &split[0]).0;
                let below_600 = f[1].tick(x, &split[1]).0;
                let mid = f[2].tick(below_600, &split[0]).2;
                let mid = f[3].tick(mid, &split[0]).2;
                let upper = f[4].tick(x, &split[2]).0;
                let upper = f[5].tick(upper, &split[1]).2;
                let upper = f[6].tick(upper, &split[1]).2;
                let high = f[7].tick(x, &split[2]).2;
                let band = [low, mid, upper, high];
                let mut jump = false;
                for k in 0..4 {
                    h.fast[k] += (band[k] * band[k] - h.fast[k]) * fast;
                    if k < bands && h.fast[k] > h.slow[k] * Hits::JUMP && h.fast[k] > threshold * threshold * 0.1 {
                        jump = true;
                    }
                    h.slow[k] += (h.fast[k] - h.slow[k]) * slow;
                }
                let level = if p.lows { below_600.abs() } else { self.key.level[i] };
                h.peak = if level > h.peak { h.peak + (level - h.peak) * peak_up } else { level + (h.peak - level) * peak_down };
                // A crossing (past half the level either way, so a quieter conga
                // riding on a kick's tail makes none): its half
                // period much shorter than they have been is a new hit's sweep.
                h.since_crossing += 1.0;
                let side = if below_600 > 0.5 * h.peak { 1.0 } else if below_600 < -0.5 * h.peak { -1.0 } else { h.side };
                if side != h.side {
                    if h.side != 0.0 {
                        h.quick = if h.since_crossing < 0.4 * h.half_period { h.quick.saturating_add(1) } else { 0 };
                        // (Listening to all of it, a high band shows the hit already.)
                        if h.quick >= 2 && p.lows {
                            jump = true;
                        }
                        h.half_period = if h.half_period.is_finite() { h.half_period + (h.since_crossing - h.half_period) * 0.2 } else { h.since_crossing };
                    }
                    (h.side, h.since_crossing) = (side, 0.0);
                }
                h.since += dt;
                let strength = ((20.0 * (h.peak.max(1e-9) / threshold).log10()) / 12.0).clamp(0.0, 1.0);
                if h.armed && jump && h.peak > threshold {
                    let down = h.amount * self.shape(&h, period);
                    h.amount = strength.max(1e-3);
                    h.from = (down / h.amount).min(1.0);
                    h.since = 0.0;
                    h.armed = false;
                } else {
                    if h.since < Hits::RISE {
                        h.amount = h.amount.max(strength);
                    }
                    if !h.armed && h.since > Hits::GAP && !jump {
                        h.armed = true;
                    }
                }
                let g = 1.0 - p.depth * h.amount * self.shape(&h, period);
                *a *= g;
                *b *= g;
            }
            self.hits = h;
            return;
        }
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
    /// A VST3 effect; None until the app loads it (it then passes the signal through).
    Plugin(PluginId, Option<Box<PluginProcessor>>),
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
                key: Key::new(),
            }),
            Effect::Delay(params) => Self::Delay(Delay::new(params, sample_rate)),
            Effect::Reverb(params) => Self::Reverb(Reverb::new(params, sample_rate)),
            Effect::Saturator(params) => Self::Saturator(Saturator::new(params)),
            Effect::AutoFilter(params) => Self::AutoFilter(AutoFilter {
                params,
                filters: Default::default(),
                first: Default::default(),
                coefs: None,
                until: 0,
            }),
            Effect::Chorus(params) => Self::Chorus(Chorus::new(params, sample_rate)),
            Effect::Phaser(params) => Self::Phaser(Phaser {
                params,
                stages: [[(0.0, 0.0); PHASER_STAGES]; 2],
                feedback: [0.0; 2],
                phase: 0.0,
                coefs: [0.0; 2],
                until: 0,
            }),
            Effect::Bitcrusher(params) => Self::Bitcrusher(Bitcrusher { params, counter: 0.0, held: [0.0; 2] }),
            Effect::Ducker(params) => Self::Ducker(Ducker { params, key: Key::new(), hits: Hits::IDLE }),
            Effect::Limiter(params) => Self::Limiter(Limiter::new(params, sample_rate)),
            Effect::Utility(params) => Self::Utility(Utility { params }),
            Effect::Plugin(id) => Self::Plugin(id, None),
        }
    }

    /// The track (by id) whose sound drives it, if it has a sidechain.
    pub fn sidechain(&self) -> Option<u32> {
        match self {
            Self::Compressor(fx) => fx.params.sidechain,
            Self::Ducker(fx) => fx.params.sidechain,
            _ => None,
        }
    }

    /// Gives a sidechained effect its source's sound for this block.
    pub fn feed_key(&mut self, left: &[f32], right: &[f32]) {
        match self {
            Self::Compressor(fx) => fx.key.feed(left, right),
            Self::Ducker(fx) => fx.key.feed(left, right),
            _ => {}
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
            (Self::Plugin(id, _), Effect::Plugin(other)) => return *id == other,
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
            Self::Plugin(id, _) => Effect::Plugin(*id),
        }
    }

    /// Sets one continuous parameter (see [`Effect::params`]); used by automation.
    pub fn set_param(&mut self, index: usize, value: f32) {
        // A plugin's parameters are its own ids, normalized.
        if let Self::Plugin(_, plugin) = self {
            if let Some(p) = plugin {
                p.set_normalized(index as u32, value as f64);
            }
            return;
        }
        let mut effect = self.effect();
        effect.set(index, value);
        self.set_params(&effect);
    }

    /// Silences the effect's memory (delay lines, filter states) without allocating.
    pub fn reset(&mut self) {
        match self {
            Self::Eq3(fx) => {
                fx.low = Default::default();
                fx.high = Default::default();
            }
            Self::Compressor(fx) => fx.envelope_db = 0.0,
            Self::Delay(fx) => {
                fx.lines.iter_mut().for_each(|line| line.fill(0.0));
                fx.write = 0;
                fx.current = 0.0;
                fx.damping = [0.0; 2];
            }
            Self::Reverb(fx) => {
                for comb in fx.combs.iter_mut().flatten() {
                    comb.buffer.fill(0.0);
                    comb.index = 0;
                    comb.store = 0.0;
                }
                for allpass in fx.allpasses.iter_mut().flatten() {
                    allpass.buffer.fill(0.0);
                    allpass.index = 0;
                }
            }
            Self::AutoFilter(fx) => {
                fx.filters = Default::default();
                fx.first = Default::default();
                fx.coefs = None;
                fx.until = 0;
            }
            Self::Chorus(fx) => {
                fx.lines.iter_mut().for_each(|line| line.fill(0.0));
                fx.write = 0;
                fx.phase = 0.0;
            }
            Self::Phaser(fx) => {
                fx.stages = [[(0.0, 0.0); PHASER_STAGES]; 2];
                fx.feedback = [0.0; 2];
                fx.phase = 0.0;
                fx.until = 0;
            }
            Self::Bitcrusher(fx) => {
                fx.counter = 0.0;
                fx.held = [0.0; 2];
            }
            Self::Limiter(fx) => {
                fx.delay.iter_mut().for_each(|line| line.fill(0.0));
                fx.position = 0;
                fx.gain = 1.0;
            }
            Self::Ducker(fx) => fx.hits = Hits::IDLE,
            Self::Saturator(fx) => {
                fx.over = [Oversampler::new(), Oversampler::new()];
                fx.dry = [[0.0; Oversampler::LATENCY]; 2];
            }
            Self::Utility(_) | Self::Plugin(..) => {}
        }
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
            Self::Plugin(_, Some(plugin)) => {
                plugin.set_transport(ctx.tempo, ctx.beat, ctx.beats_per_sample > 0.0);
                plugin.process(l, r);
            }
            Self::Plugin(_, None) => {}
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
    fn a_keyed_ducker_pumps_once_on_every_kick_designer_kick() {
        // Kicks at 150 BPM, each cutting the last one's tail as the instrument
        // does; clean, and clipped hard (a hard techno kick: its tail as loud in
        // every low band as its hit).
        let sr = 48_000.0;
        let beat = (60.0 / 150.0 * sr) as usize;
        for (name, params) in crate::model::KickParams::presets() {
            let mut kick = crate::kick::KickSynth::new(params, sr);
            kick.set_tempo(150.0);
            let (mut kl, mut kr) = (vec![0.0f32; 8 * beat], vec![0.0f32; 8 * beat]);
            for b in 0..8 {
                kick.note_on(36, 1.0);
                for (l, r) in kl[b * beat..(b + 1) * beat].chunks_mut(256).zip(kr[b * beat..(b + 1) * beat].chunks_mut(256)) {
                    kick.render(&[], l, r);
                }
            }
            // (A hardstyle tail is distorted already; a long 808 or trap sub
            // clipped is a square restarting, with no sweep to tell its hits by.)
            let sub = name.starts_with("808") || name.starts_with("Trap");
            for clipped in [false, true].into_iter().filter(|&c| !c || !(name.contains("Hardstyle") || sub)) {
            if clipped {
                for x in kl.iter_mut().chain(kr.iter_mut()) {
                    *x = (*x * 8.0).clamp(-0.9, 0.9);
                }
            }
            // A hardstyle tail is as loud under 600 Hz as its hit: it needs the whole sound.
            let lows = !name.contains("Hardstyle");
            let params = crate::model::DuckerParams { depth: 0.6, release: 0.25, sidechain: Some(1), lows, ..Default::default() };
            let mut fx = EffectProcessor::new(&Effect::Ducker(params), sr);
            let ctx = ProcessContext { tempo: 150.0, ..ctx() };
            let mut gain = Vec::new();
            for (bl, br) in kl.chunks(256).zip(kr.chunks(256)) {
                fx.feed_key(bl, br);
                let mut l = vec![1.0f32; bl.len()];
                let mut r = l.clone();
                fx.process(&mut l, &mut r, &ctx);
                gain.extend(l);
            }
            // The lowest gain in each eighth of a beat, from the second beat on.
            let step = beat / 8;
            for b in 1..8 {
                let lowest: Vec<f32> = (0..8).map(|k| gain[b * beat + k * step..][..step].iter().cloned().fold(1.0, f32::min)).collect();
                assert!(lowest[0] < 0.45, "{name} (clipped {clipped}): ducks at once on kick {b}: {lowest:.2?}");
                assert!(lowest[5..].iter().all(|&g| g > 0.98), "{name} (clipped {clipped}): back up before kick {}: {lowest:.2?}", b + 1);
            }
            }
        }
    }

    #[test]
    fn a_keyed_ducker_ignores_hats_with_lows_only() {
        let sr = 48_000.0;
        // Bright noise bursts of 30 ms (noise differenced twice: a hat's
        // highs), one on every eighth at 150 BPM.
        let eighth = (30.0 / 150.0 * sr) as usize;
        let mut seed = 1u32;
        let (mut x1, mut d1) = (0.0f32, 0.0f32);
        let hats: Vec<f32> = (0..16 * eighth)
            .map(|i| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 9) as f32 / (1u32 << 23) as f32 * 2.0 - 1.0;
                let d = noise - x1;
                let bright = 0.25 * (d - d1);
                (x1, d1) = (noise, d);
                let t = (i % eighth) as f32 / sr;
                bright * 0.8 * (-t / 0.03).exp()
            })
            .collect();
        for (lows, pumps) in [(true, false), (false, true)] {
            let params = crate::model::DuckerParams { depth: 0.6, release: 0.25, sidechain: Some(1), lows, ..Default::default() };
            let mut fx = EffectProcessor::new(&Effect::Ducker(params), sr);
            let ctx = ProcessContext { tempo: 150.0, ..ctx() };
            let mut lowest = 1.0f32;
            for block in hats.chunks(256) {
                fx.feed_key(block, block);
                let mut l = vec![1.0f32; block.len()];
                let mut r = l.clone();
                fx.process(&mut l, &mut r, &ctx);
                lowest = l.iter().cloned().fold(lowest, f32::min);
            }
            assert_eq!(lowest < 0.8, pumps, "lows only {lows}: lowest gain {lowest}");
        }
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

    fn sine(freq: f32, frames: usize, level: f32) -> Vec<f32> {
        (0..frames).map(|i| (i as f32 * freq * TAU / 48_000.0).sin() * level).collect()
    }

    /// The level of `x` at `freq` (a single-bin DFT, Hann-windowed).
    fn level_at(x: &[f32], freq: f32) -> f32 {
        let n = x.len() as f32;
        let (mut re, mut im) = (0f32, 0f32);
        for (i, s) in x.iter().enumerate() {
            let w = 0.5 - 0.5 * (TAU * i as f32 / n).cos();
            let a = TAU * freq * i as f32 / 48_000.0;
            re += s * w * a.cos();
            im += s * w * a.sin();
        }
        (re * re + im * im).sqrt() / n * 4.0
    }

    /// Past 95 % resonance the filter rings by itself after a hit, for good,
    /// at its cutoff, without running away.
    #[test]
    fn the_auto_filter_self_oscillates() {
        let mut fx = EffectProcessor::new(&Effect::AutoFilter(AutoFilterParams { cutoff: 440.0, resonance: 1.1, ..Default::default() }), 48_000.0);
        let mut l = vec![0f32; 96_000];
        l[0] = 1.0;
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            fx.process(a, b, &ctx());
        }
        let late = &l[72_000..];
        let peak = late.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05 && peak < 2.0, "still ringing, bounded: {peak}");
        assert!(level_at(late, 440.0) > 10.0 * level_at(late, 880.0), "at its cutoff");
        // As it always was below: a hit dies away.
        let mut fx = EffectProcessor::new(&Effect::AutoFilter(AutoFilterParams { cutoff: 440.0, resonance: 0.9, ..Default::default() }), 48_000.0);
        let mut l = vec![0f32; 96_000];
        l[0] = 1.0;
        let mut r = l.clone();
        fx.process(&mut l, &mut r, &ctx());
        assert!(l[72_000..].iter().all(|s| s.abs() < 1e-3));
    }

    /// However hard it is pushed (resonance past self-oscillation, the cutoff
    /// jumping across the whole range or swept fast near the top, drive, loud
    /// noise), the filter stays at sane levels: it used to run away to 1e12 and
    /// more, which the engine's NaN guard did not catch.
    #[test]
    fn the_auto_filter_never_runs_away() {
        let mut seed = 1u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        };
        let mut worst = (0f32, String::new());
        for rate in [44_100.0f32, 48_000.0] {
            let ctx = ProcessContext { sample_rate: rate, tempo: 120.0, beat: 0.0, beats_per_sample: 2.0 / rate as f64 };
            for mode in [FilterMode::LowPass, FilterMode::HighPass, FilterMode::BandPass] {
                for (steep, drive_db) in [(false, 0.0), (true, 0.0), (false, 24.0)] {
                    for resonance in [0.95f32, 1.0, 1.1, 1.2] {
                        for (cutoff, lfo_amount, jumps) in [(18_000.0f32, 0.0f32, false), (12_000.0, 1.0, false), (1_000.0, 4.0, false), (1_000.0, 0.0, true)] {
                            for level in [1.0f32, 8.0] {
                                let params = AutoFilterParams { mode, cutoff, resonance, lfo_beats: 0.0625, lfo_amount, drive_db, steep };
                                let mut fx = EffectProcessor::new(&Effect::AutoFilter(params), rate);
                                let mut peak = 0f32;
                                for block in 0..(2.0 * rate / 16.0) as usize {
                                    if jumps {
                                        let cutoff = if block % 2 == 0 { 30.0 } else { 18_000.0 };
                                        fx.set_params(&Effect::AutoFilter(AutoFilterParams { cutoff, ..params }));
                                    }
                                    let mut l: Vec<f32> = (0..16).map(|_| noise() * level).collect();
                                    let mut r = l.clone();
                                    fx.process(&mut l, &mut r, &ctx);
                                    peak = l.iter().chain(&r).fold(peak, |m, s| m.max(s.abs()));
                                }
                                let relative = peak / level;
                                if !(relative <= worst.0) {
                                    worst = (relative, format!("{rate} {mode:?} steep {steep} drive {drive_db} res {resonance} cutoff {cutoff} lfo {lfo_amount} jumps {jumps} level {level}: peak {peak}"));
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(worst.0 < 20.0, "{}", worst.1);
    }

    /// 24 dB per octave: an octave over the cutoff, about twice as far down.
    #[test]
    fn the_steep_auto_filter_cuts_twice_as_much() {
        let attenuation = |steep: bool| {
            let mut fx = EffectProcessor::new(&Effect::AutoFilter(AutoFilterParams { cutoff: 1000.0, resonance: 0.0, steep, ..Default::default() }), 48_000.0);
            let mut l = sine(2000.0, 48_000, 0.5);
            let mut r = l.clone();
            fx.process(&mut l, &mut r, &ctx());
            20.0 * (level_at(&l[24_000..], 2000.0) / 0.5).log10()
        };
        let (gentle, steep) = (attenuation(false), attenuation(true));
        println!("una octava por encima del corte: 12 dB {gentle:.1} dB, 24 dB {steep:.1} dB");
        assert!((-17.0..-10.0).contains(&gentle), "12 dB: {gentle:.1}");
        assert!((-34.0..-22.0).contains(&steep) && steep < gentle - 9.0, "24 dB: {steep:.1} (12 dB: {gentle:.1})");
    }

    /// Hard clipping a 5 kHz sine makes harmonics up past Nyquist, which fold
    /// back (to 13 and 3 kHz here); oversampled, they stay out.
    #[test]
    fn oversampling_keeps_aliases_out() {
        let aliases = |oversample: bool| {
            let p = SaturatorParams { drive_db: 30.0, output_db: -6.0, mix: 1.0, kind: KickDrive::Hard, oversample };
            let mut fx = EffectProcessor::new(&Effect::Saturator(p), 48_000.0);
            let mut l = sine(5000.0, 48_000, 0.5);
            let mut r = l.clone();
            fx.process(&mut l, &mut r, &ctx());
            let x = &l[4_800..];
            let fundamental = level_at(x, 5000.0);
            20.0 * ((level_at(x, 13_000.0) + level_at(x, 3_000.0)) / fundamental).log10()
        };
        let (plain, over) = (aliases(false), aliases(true));
        println!("alias (13 + 3 kHz) respecto a la fundamental: sin sobremuestreo {plain:.1} dB, con {over:.1} dB");
        assert!(plain > -40.0, "without: {plain:.1} dB");
        assert!(over < plain - 20.0, "oversampled: {over:.1} dB (without {plain:.1})");
    }

    /// The oversampled path is as late as its dry signal: half wet, half dry
    /// doesn't comb-filter (a quiet sine goes through almost untouched).
    #[test]
    fn oversampled_dry_and_wet_line_up() {
        let p = SaturatorParams { drive_db: 0.0, output_db: 0.0, mix: 0.5, kind: KickDrive::Soft, oversample: true };
        let mut fx = EffectProcessor::new(&Effect::Saturator(p), 48_000.0);
        for freq in [1000.0, 8000.0, 15_000.0] {
            let mut l = sine(freq, 24_000, 0.05);
            let mut r = l.clone();
            fx.process(&mut l, &mut r, &ctx());
            let level = level_at(&l[4_800..], freq) / 0.05;
            assert!((0.9..1.1).contains(&level), "{freq} Hz: {level}");
        }
    }
}
