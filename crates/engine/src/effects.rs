//! Stereo audio effects. Constructors allocate (delay lines, reverb buffers) and
//! must run off the audio thread; `set_params` and `process` never allocate.

use crate::model::{
    db_to_gain, AutoFilterParams, BitcrusherParams, ChorusParams, CompressorParams, DelayParams, DuckerParams, Effect,
    DeEsserParams, EqBand, EqEightParams, EqShape, Eq3Params, FilterMode, GateParams, TransientParams, EQ_BANDS, KickDrive, LimiterParams, PhaserParams, PluginId, ReverbParams, SaturatorParams, UtilityParams,
};
use crate::plugin::PluginProcessor;
use crate::tuner::Tuner;
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
    /// The project's scale (its pitch classes as bits, C first; 0: none),
    /// for a Tuner.
    pub scale: u16,
    /// The song's time signatures (plugins are told the bar and signature).
    pub meter: crate::meter::Meter,
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

/// A gain that glides to a new value over [`Glide::SAMPLES`] samples, so
/// turning it doesn't click, the same whatever the blocks are cut into (the
/// export must sound like the playback).
#[derive(Clone, Copy)]
struct Glide {
    value: f32,
    target: f32,
    step: f32,
    left: u32,
}

impl Glide {
    const SAMPLES: u32 = 64;

    fn new(value: f32) -> Self {
        Self { value, target: value, step: 0.0, left: 0 }
    }

    #[inline]
    fn next(&mut self, target: f32) -> f32 {
        if target != self.target {
            self.target = target;
            self.step = (target - self.value) / Self::SAMPLES as f32;
            self.left = Self::SAMPLES;
        }
        if self.left > 0 {
            self.left -= 1;
            self.value = if self.left == 0 { self.target } else { self.value + self.step };
        }
        self.value
    }

    /// Settled at `value` (nothing to glide).
    fn at(&self, value: f32) -> bool {
        self.left == 0 && self.value == value
    }
}

/// Second-order filter coefficients, normalized so a0 is 1.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    const IDENTITY: Biquad = Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    fn normalized(b: [f64; 3], a: [f64; 3]) -> Self {
        Self { b0: b[0] / a[0], b1: b[1] / a[0], b2: b[2] / a[0], a1: a[1] / a[0], a2: a[2] / a[0] }
    }

    /// Gain at `w` radians a sample.
    fn magnitude(&self, w: f64) -> f64 {
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let (nr, ni) = (self.b0 + self.b1 * c1 + self.b2 * c2, -(self.b1 * s1 + self.b2 * s2));
        let (dr, di) = (1.0 + self.a1 * c1 + self.a2 * c2, -(self.a1 * s1 + self.a2 * s2));
        ((nr * nr + ni * ni) / (dr * dr + di * di).max(1e-30)).sqrt()
    }
}

/// Transposed direct form II state.
#[derive(Clone, Copy, Default)]
struct BiquadState {
    s1: f64,
    s2: f64,
}

/// Second-order sections a band needs at most (a 48 dB cut).
const EQ_STAGES: usize = 4;

/// The Q a shelf gets at the Q knob's top: a bump of about 3 dB at its edge
/// (the knob's 18 would make one of over 20 dB).
const SHELF_MAX_Q: f64 = 1.4;

/// The Q a shelf uses for knob value `q`: the same up to 0.71 (no bump), then
/// the rest of the knob's travel spread up to [`SHELF_MAX_Q`].
fn shelf_q(q: f64) -> f64 {
    let flat = std::f64::consts::FRAC_1_SQRT_2;
    if q <= flat {
        return q;
    }
    let top = 18.0 / flat;
    flat * (q / flat).min(top).powf((SHELF_MAX_Q / flat).ln() / top.ln())
}

/// The sections that make band `band` at `sample_rate`, and how many it uses
/// (none: it changes nothing, a bell or shelf at 0 dB).
fn band_sections(band: &EqBand, sample_rate: f32) -> ([Biquad; EQ_STAGES], usize) {
    if band.shape.has_gain() && band.gain_db == 0.0 {
        return ([Biquad::IDENTITY; EQ_STAGES], 0);
    }
    let sr = sample_rate as f64;
    let freq = (band.freq as f64).clamp(10.0, sr * 0.49);
    let q = (band.q as f64).clamp(0.05, 40.0);
    let w0 = std::f64::consts::TAU * freq / sr;
    let (cos, sin) = (w0.cos(), w0.sin());
    let low_pass = |q: f64| {
        let alpha = sin / (2.0 * q);
        Biquad::normalized([(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0], [1.0 + alpha, -2.0 * cos, 1.0 - alpha])
    };
    let high_pass = |q: f64| {
        let alpha = sin / (2.0 * q);
        Biquad::normalized([(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0], [1.0 + alpha, -2.0 * cos, 1.0 - alpha])
    };
    // An eighth-order Butterworth in four sections; the band's Q above 0.71
    // raises the resonance of the last one.
    let steep = |section: &dyn Fn(f64) -> Biquad| {
        let resonance = q / std::f64::consts::FRAC_1_SQRT_2;
        let qs = [0.5098, 0.6013, 0.9000, 2.5629 * resonance.max(0.2)];
        (qs.map(|q| section(q)), EQ_STAGES)
    };
    let one = |section: Biquad| {
        let mut sections = [Biquad::IDENTITY; EQ_STAGES];
        sections[0] = section;
        (sections, 1)
    };
    let a = 10f64.powf(band.gain_db as f64 / 40.0);
    let alpha = sin / (2.0 * q);
    let shelf_alpha = sin / (2.0 * shelf_q(q));
    match band.shape {
        EqShape::LowCut48 => steep(&high_pass),
        EqShape::HighCut48 => steep(&low_pass),
        EqShape::LowCut12 => one(high_pass(q)),
        EqShape::HighCut12 => one(low_pass(q)),
        EqShape::Bell => one(Biquad::normalized([1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a], [1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a])),
        EqShape::Notch => one(Biquad::normalized([1.0, -2.0 * cos, 1.0], [1.0 + alpha, -2.0 * cos, 1.0 - alpha])),
        EqShape::LowShelf => {
            let k = 2.0 * a.sqrt() * shelf_alpha;
            one(Biquad::normalized(
                [a * ((a + 1.0) - (a - 1.0) * cos + k), 2.0 * a * ((a - 1.0) - (a + 1.0) * cos), a * ((a + 1.0) - (a - 1.0) * cos - k)],
                [(a + 1.0) + (a - 1.0) * cos + k, -2.0 * ((a - 1.0) + (a + 1.0) * cos), (a + 1.0) + (a - 1.0) * cos - k],
            ))
        }
        EqShape::HighShelf => {
            let k = 2.0 * a.sqrt() * shelf_alpha;
            one(Biquad::normalized(
                [a * ((a + 1.0) + (a - 1.0) * cos + k), -2.0 * a * ((a - 1.0) + (a + 1.0) * cos), a * ((a + 1.0) + (a - 1.0) * cos - k)],
                [(a + 1.0) - (a - 1.0) * cos + k, 2.0 * ((a - 1.0) - (a + 1.0) * cos), (a + 1.0) - (a - 1.0) * cos - k],
            ))
        }
    }
}

/// What band `band` does at `freq` Hz, in dB (whether it is on or not).
pub fn eq_band_response_db(band: &EqBand, sample_rate: f32, freq: f32) -> f32 {
    let (sections, used) = band_sections(band, sample_rate);
    let w = std::f64::consts::TAU * freq as f64 / sample_rate as f64;
    let gain: f64 = sections[..used].iter().map(|s| s.magnitude(w)).product();
    (20.0 * gain.max(1e-12).log10()) as f32
}

/// What the bands that are on do together at `freq` Hz, in dB (the output gain apart).
pub fn eq_eight_response_db(params: &EqEightParams, sample_rate: f32, freq: f32) -> f32 {
    params.bands.iter().filter(|b| b.on).map(|b| eq_band_response_db(b, sample_rate, freq)).sum()
}

pub struct EqEight {
    params: EqEightParams,
    /// The settings the sections were made from (None: make them).
    made: Option<(EqEightParams, f32)>,
    sections: [([Biquad; EQ_STAGES], usize); EQ_BANDS],
    states: [[[BiquadState; EQ_STAGES]; EQ_BANDS]; 2],
    /// The output gain.
    gain: Glide,
}

impl EqEight {
    fn new(params: EqEightParams) -> Self {
        Self {
            params,
            made: None,
            sections: [([Biquad::IDENTITY; EQ_STAGES], 0); EQ_BANDS],
            states: Default::default(),
            gain: Glide::new(db_to_gain(params.output_db)),
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], sr: f32) {
        let p = self.params;
        if self.made != Some((p, sr)) {
            for (b, band) in p.bands.iter().enumerate() {
                let before = self.made.map(|(m, _)| m.bands[b]);
                self.sections[b] = if band.on { band_sections(band, sr) } else { ([Biquad::IDENTITY; EQ_STAGES], 0) };
                // A band that comes on, or changes shape, starts from silence; so
                // does one left out (flat: a flat filter's state is 0 anyway).
                if before.is_none_or(|m| m.on != band.on || m.shape != band.shape) || self.sections[b].1 == 0 {
                    self.states[0][b] = Default::default();
                    self.states[1][b] = Default::default();
                }
            }
            self.made = Some((p, sr));
        }
        for (ch, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
            for (b, (sections, used)) in self.sections.iter().enumerate() {
                let state = &mut self.states[ch][b];
                for (c, s) in sections[..*used].iter().zip(state.iter_mut()) {
                    // In locals: kept in registers, not written back every sample.
                    let (mut s1, mut s2) = (s.s1, s.s2);
                    for x in buf.iter_mut() {
                        let input = *x as f64;
                        let y = c.b0 * input + s1;
                        s1 = c.b1 * input - c.a1 * y + s2;
                        s2 = c.b2 * input - c.a2 * y;
                        *x = y as f32;
                    }
                    *s = BiquadState { s1, s2 };
                }
            }
        }
        let target = db_to_gain(p.output_db);
        if !(self.gain.at(1.0) && target == 1.0) {
            for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                let g = self.gain.next(target);
                *a *= g;
                *b *= g;
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
        let steps = crush_steps(p.drive_db);
        if !p.oversample {
            for s in l.iter_mut().chain(r.iter_mut()) {
                let wet = shape(*s, p.kind, drive, steps) * out;
                *s += (wet - *s) * p.mix;
            }
            return;
        }
        for i in 0..l.len() {
            for (ch, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                let x = buf[i];
                let mut up = self.over[ch].up(x);
                for y in &mut up {
                    *y = shape(*y, p.kind, drive, steps);
                }
                let wet = self.over[ch].down(up) * out;
                let dry = std::mem::replace(&mut self.dry[ch][self.at], x);
                buf[i] = dry + (wet - dry) * p.mix;
            }
            self.at = (self.at + 1) % Oversampler::LATENCY;
        }
    }
}

/// Crush's levels at `drive_db`: the same for a whole block.
fn crush_steps(drive_db: f32) -> f32 {
    2f32.powf(12.0 - (drive_db / 36.0).clamp(0.0, 1.0) * 10.0)
}

/// A sample through a saturation `kind` at gain `drive`, with Crush's
/// levels (see [`crush_steps`]). Soft is the tanh the Saturator always had.
#[inline]
fn shape(x: f32, kind: KickDrive, drive: f32, steps: f32) -> f32 {
    let x = x * drive;
    match kind {
        KickDrive::Soft => x.tanh(),
        KickDrive::Hard => x.clamp(-1.0, 1.0),
        KickDrive::Fold => (x * PI * 0.5).sin(),
        // The negative half saturates later: even harmonics, warm.
        KickDrive::Tube => {
            if x >= 0.0 { x.tanh() } else { 2.0 * (x * 0.5).tanh() }
        }
        KickDrive::Crush => (x.clamp(-1.0, 1.0) * steps).round() / steps,
    }
}

/// 4× oversampling for a nonlinearity: a 145-tap windowed-sinc low-pass (Kaiser,
/// flat to about 18 kHz, 70 dB down by the base rate's Nyquist) to interpolate
/// up, and the same to filter before coming down. Its delay is a whole number of
/// samples ([`Oversampler::LATENCY`]).
pub struct Oversampler {
    /// The last inputs, twice over (each written at its place and one ring
    /// further), so the last ones are always side by side: no modulo per tap.
    input: [f32; 2 * Self::PHASE_TAPS],
    input_at: usize,
    /// The last oversampled values, the same way.
    output: [f32; 2 * Self::TAPS],
    output_at: usize,
    /// The low-pass split in its four phases (for going up), and its first
    /// half (it is symmetric: going down adds each pair of samples first).
    phases: &'static Phases,
}

/// [`Oversampler::taps`] as the oversampler uses them.
struct Phases {
    up: [[f32; Oversampler::PHASE_TAPS]; Oversampler::FACTOR],
    half: [f32; Oversampler::TAPS / 2 + 1],
}

/// The dot product of two slices of the same length, in eight lanes the
/// compiler keeps in vector registers.
#[inline]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut lanes = [0f32; 8];
    let (ca, cb) = (a.chunks_exact(8), b.chunks_exact(8));
    let (ra, rb) = (ca.remainder(), cb.remainder());
    for (x, y) in ca.zip(cb) {
        for k in 0..8 {
            lanes[k] += x[k] * y[k];
        }
    }
    let mut sum = ((lanes[0] + lanes[4]) + (lanes[1] + lanes[5])) + ((lanes[2] + lanes[6]) + (lanes[3] + lanes[7]));
    for (x, y) in ra.iter().zip(rb) {
        sum += x * y;
    }
    sum
}

impl Oversampler {
    const FACTOR: usize = 4;
    const TAPS: usize = 145;
    const PHASE_TAPS: usize = 37;
    /// Samples (at the base rate) between a sample going in and coming out:
    /// each filter's middle, 72 oversampled samples, twice.
    pub const LATENCY: usize = 36;

    fn new() -> Self {
        Self { input: [0.0; 2 * Self::PHASE_TAPS], input_at: 0, output: [0.0; 2 * Self::TAPS], output_at: 0, phases: Self::phases() }
    }

    fn phases() -> &'static Phases {
        static PHASES: std::sync::OnceLock<Phases> = std::sync::OnceLock::new();
        PHASES.get_or_init(|| {
            let taps = Self::taps();
            let mut up = [[0f32; Self::PHASE_TAPS]; Self::FACTOR];
            for (phase, row) in up.iter_mut().enumerate() {
                for (j, tap) in row.iter_mut().enumerate() {
                    // Zero-stuffing leaves a quarter of the level: four times the taps.
                    *tap = taps.get(phase + Self::FACTOR * j).map_or(0.0, |t| t * Self::FACTOR as f32);
                }
            }
            let mut half = [0f32; Self::TAPS / 2 + 1];
            half.copy_from_slice(&taps[..Self::TAPS / 2 + 1]);
            Phases { up, half }
        })
    }

    /// The low-pass, at about 21 kHz for 48 kHz (0.44 of the base Nyquist), with
    /// a Kaiser window (β 7).
    fn taps() -> &'static [f32; 145] {
        static TAPS: std::sync::OnceLock<[f32; 145]> = std::sync::OnceLock::new();
        TAPS.get_or_init(Self::design)
    }

    fn design() -> [f32; 145] {
        {
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
        }
    }

    /// One input sample as four at the higher rate.
    #[inline]
    fn up(&mut self, x: f32) -> [f32; 4] {
        const P: usize = Oversampler::PHASE_TAPS;
        self.input_at = (self.input_at + P - 1) % P;
        self.input[self.input_at] = x;
        self.input[self.input_at + P] = x;
        let window = &self.input[self.input_at..self.input_at + P];
        let up = &self.phases.up;
        [dot(&up[0], window), dot(&up[1], window), dot(&up[2], window), dot(&up[3], window)]
    }

    /// Four samples at the higher rate as one at the base rate: taken at the
    /// first of them, so the delay is whole base samples.
    #[inline]
    fn down(&mut self, xs: [f32; 4]) -> f32 {
        const T: usize = Oversampler::TAPS;
        let push = |o: &mut Self, x: f32| {
            o.output_at = (o.output_at + T - 1) % T;
            o.output[o.output_at] = x;
            o.output[o.output_at + T] = x;
        };
        // The first one in, and the window ending at it, before the other three go in.
        push(self, xs[0]);
        let window = &self.output[self.output_at..self.output_at + T];
        // Symmetric taps: each pair from the two ends shares one.
        let mut pairs = [0f32; T / 2 + 1];
        for k in 0..T / 2 {
            pairs[k] = window[k] + window[T - 1 - k];
        }
        pairs[T / 2] = window[T / 2];
        let out = dot(&self.phases.half, &pairs);
        for &x in &xs[1..] {
            push(self, x);
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

/// One-pole smoothing coefficient for `ms` milliseconds at `sr`.
fn smoothing(ms: f32, sr: f32) -> f32 {
    (-1.0 / (ms.max(0.001) * 0.001 * sr)).exp()
}

pub struct Gate {
    params: GateParams,
    key: Key,
    /// The level it listens to, a peak that falls in a few ms (it doesn't
    /// close between the waves of a low note).
    level: f32,
    open: bool,
    /// Samples left to stay open after the sound fell under.
    hold: usize,
    gain: f32,
}

impl Gate {
    fn new(params: GateParams) -> Self {
        Self { params, key: Key::new(), level: 0.0, open: false, hold: 0, gain: 0.0 }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], sr: f32) {
        let p = self.params;
        let open_at = db_to_gain(p.threshold_db);
        let close_at = db_to_gain(p.threshold_db - p.return_db.max(0.0));
        let floor = if p.floor_db <= -80.0 { 0.0 } else { db_to_gain(p.floor_db) };
        // Attack and release are how long it takes to open and close all the
        // way (60 dB), not a time constant.
        let (attack, release, fall) = (smoothing(p.attack_ms / 6.9, sr), smoothing(p.release_ms / 6.9, sr), smoothing(5.0, sr));
        let hold = (p.hold_ms.max(0.0) * 0.001 * sr) as usize;
        let keyed = self.key.take(l.len());
        for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            let peak = if keyed { self.key.level[i] } else { a.abs().max(b.abs()) };
            self.level = peak.max(self.level * fall);
            if self.level >= open_at {
                self.open = true;
                self.hold = hold;
            } else if self.open && self.level < close_at {
                if self.hold > 0 {
                    self.hold -= 1;
                } else {
                    self.open = false;
                }
            } else if self.open {
                self.hold = hold;
            }
            let (target, coef) = if self.open { (1.0, attack) } else { (floor, release) };
            self.gain = target + coef * (self.gain - target);
            *a *= self.gain;
            *b *= self.gain;
        }
    }
}

pub struct Transient {
    params: TransientParams,
    /// The sound's level, smooth (mean square over a few ms).
    power: f32,
    /// The level rising slowly and falling at once: under the level at the
    /// start of a hit.
    rising: f32,
    /// The level rising at once and falling slowly: over it in a hit's tail.
    falling: f32,
    /// The gain of the last sample, dB (smoothed so it doesn't crackle).
    gain_db: f32,
    output: Glide,
}

impl Transient {
    /// The most it raises or lowers, dB.
    const RANGE_DB: f32 = 18.0;
    /// dB of change for each dB of attack or tail, at ±100 %.
    const DEPTH: f32 = 0.8;

    fn new(params: TransientParams) -> Self {
        Self { params, power: 0.0, rising: 0.0, falling: 0.0, gain_db: 0.0, output: Glide::new(db_to_gain(params.output_db)) }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], sr: f32) {
        let p = self.params;
        let (mean, rise, fall, glide) = (smoothing(5.0, sr), smoothing(25.0, sr), smoothing(350.0, sr), smoothing(1.0, sr));
        let target_output = db_to_gain(p.output_db);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let x = 0.5 * (*a * *a + *b * *b);
            self.power = x + mean * (self.power - x);
            let level = self.power;
            self.rising = if level < self.rising { level } else { level + rise * (self.rising - level) };
            self.falling = if level > self.falling { level } else { level + fall * (self.falling - level) };
            // Power ratios in dB (10 log10).
            let db = |x: f32| 10.0 * x.max(1e-10).log10();
            let hit = db(level) - db(self.rising);
            // Near silence there is no tail to lengthen (only noise).
            let tail = if self.falling > 1e-7 { db(self.falling) - db(level) } else { 0.0 };
            let target = ((p.attack * hit + p.sustain * tail) * Self::DEPTH).clamp(-Self::RANGE_DB, Self::RANGE_DB);
            self.gain_db = target + glide * (self.gain_db - target);
            let g = db_to_gain(self.gain_db) * self.output.next(target_output);
            *a *= g;
            *b *= g;
        }
    }
}

/// Splits the sound at the esses' frequency (a low-pass, and what it leaves),
/// listens to the top part and takes it down while it is over the threshold,
/// up to the range. The two parts add up to the sound: while it takes nothing,
/// it changes nothing.
pub struct DeEsser {
    params: DeEsserParams,
    /// The low-pass the top part is taken from, and what it was made for.
    split: Biquad,
    made: Option<(f32, f32)>,
    states: [BiquadState; 2],
    /// The top part's level (a peak falling in 60 ms), and the gain taken off it (dB).
    level: f32,
    cut_db: f32,
}

impl DeEsser {
    fn new(params: DeEsserParams) -> Self {
        Self { params, split: Biquad::IDENTITY, made: None, states: Default::default(), level: 0.0, cut_db: 0.0 }
    }

    /// How much it takes off the top part (dB, ≤ 0) for its level.
    fn cut_for(&self, level: f32) -> f32 {
        let over = 20.0 * level.max(1e-9).log10() - self.params.threshold_db;
        -over.clamp(0.0, self.params.range_db.max(0.0))
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], sr: f32) {
        let freq = self.params.freq_hz.clamp(1000.0, sr * 0.45);
        if self.made != Some((freq, sr)) {
            let band = EqBand { on: true, shape: EqShape::HighCut12, freq, gain_db: 0.0, q: std::f32::consts::FRAC_1_SQRT_2 };
            self.split = band_sections(&band, sr).0[0];
            self.made = Some((freq, sr));
        }
        let (fall, attack, release) = (smoothing(60.0, sr), smoothing(0.5, sr), smoothing(60.0, sr));
        let s = self.split;
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let mut tops = [0f32; 2];
            for (c, x) in [&mut *a, &mut *b].into_iter().enumerate() {
                let st = &mut self.states[c];
                let input = *x as f64;
                let low = s.b0 * input + st.s1;
                st.s1 = s.b1 * input - s.a1 * low + st.s2;
                st.s2 = s.b2 * input - s.a2 * low;
                tops[c] = (input - low) as f32;
            }
            self.level = tops[0].abs().max(tops[1].abs()).max(self.level * fall);
            let target = self.cut_for(self.level);
            let coef = if target < self.cut_db { attack } else { release };
            self.cut_db = target + coef * (self.cut_db - target);
            // The sound less what the top part loses.
            let keep = db_to_gain(self.cut_db) - 1.0;
            *a += keep * tops[0];
            *b += keep * tops[1];
        }
    }
}

pub struct Utility {
    params: UtilityParams,
    /// The crossover's halves (each run twice), and what they were made for.
    made: Option<(f32, f32)>,
    low: Biquad,
    high: Biquad,
    /// The middle's low and high halves, the side's high one.
    states: [[BiquadState; 2]; 3],
}

impl Utility {
    fn new(params: UtilityParams) -> Self {
        Self { params, made: None, low: Biquad::IDENTITY, high: Biquad::IDENTITY, states: Default::default() }
    }

    /// Runs `x` through two of `s` (a 24 dB Linkwitz-Riley), with their states.
    fn twice(s: &Biquad, states: &mut [BiquadState], mut x: f64) -> f64 {
        for st in states {
            let y = s.b0 * x + st.s1;
            st.s1 = s.b1 * x - s.a1 * y + st.s2;
            st.s2 = s.b2 * x - s.a2 * y;
            x = y;
        }
        x
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], sr: f32) {
        let gain = db_to_gain(self.params.gain_db);
        let width = self.params.width.clamp(0.0, 2.0);
        let flip = if self.params.flip_right { -1.0 } else { 1.0 };
        // The lows in mono: a Linkwitz-Riley crossover. The side keeps only
        // its highs; the middle goes through both halves, which add up to the
        // same phase the side's highs have, so over the crossover left and
        // right are what they were.
        let mono = self.params.bass_mono_hz;
        let bass = mono >= 10.0;
        if bass && self.made != Some((mono, sr)) {
            let band = |shape| band_sections(&EqBand { on: true, shape, freq: mono, gain_db: 0.0, q: std::f32::consts::FRAC_1_SQRT_2 }, sr).0[0];
            self.low = band(EqShape::HighCut12);
            self.high = band(EqShape::LowCut12);
            self.made = Some((mono, sr));
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let right = *b * flip;
            let mut mid = (*a + right) * 0.5;
            let mut side = (*a - right) * 0.5;
            if bass {
                let [mid_low, mid_high, side_high] = &mut self.states;
                let (m, x) = (mid as f64, side as f64);
                mid = (Self::twice(&self.low, mid_low, m) + Self::twice(&self.high, mid_high, m)) as f32;
                side = Self::twice(&self.high, side_high, x) as f32;
            }
            side *= width;
            *a = (mid + side) * gain;
            *b = (mid - side) * gain;
        }
    }
}

pub enum EffectProcessor {
    Eq3(Eq3),
    EqEight(Box<EqEight>),
    Compressor(Compressor),
    Gate(Gate),
    Transient(Transient),
    Tuner(Box<Tuner>),
    DeEsser(Box<DeEsser>),
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
            Effect::EqEight(params) => Self::EqEight(Box::new(EqEight::new(params))),
            Effect::Compressor(params) => Self::Compressor(Compressor {
                params,
                envelope_db: 0.0,
                key: Key::new(),
            }),
            Effect::Gate(params) => Self::Gate(Gate::new(params)),
            Effect::Transient(params) => Self::Transient(Transient::new(params)),
            Effect::Tuner(params) => Self::Tuner(Box::new(Tuner::new(params, sample_rate))),
            Effect::DeEsser(params) => Self::DeEsser(Box::new(DeEsser::new(params))),
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
            Effect::Utility(params) => Self::Utility(Utility::new(params)),
            Effect::Plugin(id) => Self::Plugin(id, None),
        }
    }

    /// How late (samples) its sound comes out: what the mixer makes up for
    /// on the other tracks (see `Engine::compensate`).
    pub fn latency(&self) -> usize {
        match self {
            Self::Saturator(fx) if fx.params.oversample => Oversampler::LATENCY,
            Self::Limiter(fx) => fx.delay[0].len(),
            Self::Tuner(fx) => fx.latency(),
            Self::Plugin(_, Some(plugin)) => plugin.latency(),
            _ => 0,
        }
    }

    /// The track (by id) whose sound drives it, if it has a sidechain.
    pub fn sidechain(&self) -> Option<u32> {
        match self {
            Self::Compressor(fx) => fx.params.sidechain,
            Self::Gate(fx) => fx.params.sidechain,
            Self::Ducker(fx) => fx.params.sidechain,
            _ => None,
        }
    }

    /// Gives a sidechained effect its source's sound for this block.
    pub fn feed_key(&mut self, left: &[f32], right: &[f32]) {
        match self {
            Self::Compressor(fx) => fx.key.feed(left, right),
            Self::Gate(fx) => fx.key.feed(left, right),
            Self::Ducker(fx) => fx.key.feed(left, right),
            _ => {}
        }
    }

    /// Updates parameters in place. Returns false if `effect` is a different type.
    pub fn set_params(&mut self, effect: &Effect) -> bool {
        match (self, *effect) {
            (Self::Eq3(fx), Effect::Eq3(p)) => fx.params = p,
            (Self::EqEight(fx), Effect::EqEight(p)) => fx.params = p,
            (Self::Compressor(fx), Effect::Compressor(p)) => fx.params = p,
            (Self::Gate(fx), Effect::Gate(p)) => fx.params = p,
            (Self::Transient(fx), Effect::Transient(p)) => fx.params = p,
            (Self::Tuner(fx), Effect::Tuner(p)) => fx.params = p,
            (Self::DeEsser(fx), Effect::DeEsser(p)) => fx.params = p,
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
            Self::EqEight(fx) => Effect::EqEight(fx.params),
            Self::Compressor(fx) => Effect::Compressor(fx.params),
            Self::Gate(fx) => Effect::Gate(fx.params),
            Self::Transient(fx) => Effect::Transient(fx.params),
            Self::Tuner(fx) => Effect::Tuner(fx.params),
            Self::DeEsser(fx) => Effect::DeEsser(fx.params),
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
            Self::EqEight(fx) => {
                fx.states = Default::default();
                fx.gain = Glide::new(db_to_gain(fx.params.output_db));
            }
            Self::Compressor(fx) => fx.envelope_db = 0.0,
            Self::Gate(fx) => {
                fx.level = 0.0;
                fx.open = false;
                fx.hold = 0;
                fx.gain = 0.0;
            }
            Self::Transient(fx) => *fx = Transient::new(fx.params),
            Self::Tuner(fx) => fx.reset(),
            Self::DeEsser(fx) => **fx = DeEsser::new(fx.params),
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
            Self::Utility(fx) => fx.states = Default::default(),
            Self::Plugin(..) => {}
        }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessContext) {
        match self {
            Self::Eq3(fx) => fx.process(l, r, ctx.sample_rate),
            Self::EqEight(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Compressor(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Gate(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Transient(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Tuner(fx) => fx.process(l, r, ctx.scale),
            Self::DeEsser(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Delay(fx) => fx.process(l, r, ctx),
            Self::Reverb(fx) => fx.process(l, r),
            Self::Saturator(fx) => fx.process(l, r),
            Self::AutoFilter(fx) => fx.process(l, r, ctx),
            Self::Chorus(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Phaser(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Bitcrusher(fx) => fx.process(l, r),
            Self::Ducker(fx) => fx.process(l, r, ctx),
            Self::Limiter(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Utility(fx) => fx.process(l, r, ctx.sample_rate),
            Self::Plugin(_, Some(plugin)) => {
                plugin.set_transport(ctx.tempo, ctx.beat, ctx.beats_per_sample > 0.0, &ctx.meter);
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
            scale: 0,
            meter: Default::default(),
        }
    }

    /// Gain (dB) of `fx` on a sine of `freq` Hz, once it has settled.
    fn sine_gain_db(fx: &mut EffectProcessor, freq: f32) -> f32 {
        let n = 48_000;
        let input: Vec<f32> = (0..n).map(|i| (TAU * freq * i as f32 / 48_000.0).sin() * 0.25).collect();
        let (mut l, mut r) = (input.clone(), input.clone());
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            fx.process(a, b, &ctx());
        }
        let rms = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt();
        let tail = n / 2;
        20.0 * (rms(&l[tail..]) / rms(&input[tail..])).log10()
    }

    /// What an EQ Eight does to the sound is what it draws, for every shape.
    #[test]
    fn eq_eight_sounds_like_its_curve() {
        for shape in EqShape::ALL {
            for (freq, gain_db, q) in [(1000.0, 9.0, 0.71), (120.0, -12.0, 3.0), (6000.0, 4.0, 0.3)] {
                let mut p = EqEightParams::default();
                p.bands = p.bands.map(|b| EqBand { on: false, ..b });
                p.bands[3] = EqBand { on: true, shape, freq, gain_db, q };
                let mut fx = EffectProcessor::new(&Effect::EqEight(p), 48_000.0);
                for probe in [50.0, 200.0, 1000.0, 3000.0, 12000.0] {
                    let drawn = eq_eight_response_db(&p, 48_000.0, probe);
                    fx.reset();
                    let heard = sine_gain_db(&mut fx, probe);
                    // Far down a cut, the residue doesn't need to match.
                    if drawn > -40.0 {
                        assert!((heard - drawn).abs() < 0.3, "{shape:?} {freq} Hz at {probe} Hz: drawn {drawn:.2} dB, heard {heard:.2} dB");
                    } else {
                        assert!(heard < -35.0, "{shape:?} {freq} Hz at {probe} Hz: {heard:.2} dB");
                    }
                }
            }
        }
    }

    /// The curves have the shapes their names say.
    #[test]
    fn eq_eight_shapes() {
        let at = |shape, freq: f32, gain_db: f32, probe: f32| eq_band_response_db(&EqBand { on: true, shape, freq, gain_db, q: 0.71 }, 48_000.0, probe);
        assert!((at(EqShape::Bell, 1000.0, 6.0, 1000.0) - 6.0).abs() < 0.01);
        assert!(at(EqShape::Bell, 1000.0, 6.0, 50.0).abs() < 0.2 && at(EqShape::Bell, 1000.0, 6.0, 15000.0).abs() < 0.3);
        // Butterworth: −3 dB at the frequency; then 48 or 12 dB an octave.
        for shape in [EqShape::LowCut48, EqShape::LowCut12, EqShape::HighCut12, EqShape::HighCut48] {
            assert!((at(shape, 1000.0, 0.0, 1000.0) + 3.01).abs() < 0.1, "{shape:?} at its frequency");
        }
        assert!((at(EqShape::LowCut48, 1000.0, 0.0, 250.0) + 96.0).abs() < 1.0, "two octaves down a 48 dB cut");
        assert!((at(EqShape::LowCut12, 1000.0, 0.0, 250.0) + 24.0).abs() < 0.5);
        assert!((at(EqShape::HighCut48, 1000.0, 0.0, 4000.0) + 96.0).abs() < 2.0);
        assert!(at(EqShape::LowCut48, 1000.0, 0.0, 8000.0).abs() < 0.01 && at(EqShape::HighCut48, 1000.0, 0.0, 125.0).abs() < 0.01);
        assert!((at(EqShape::LowShelf, 200.0, 6.0, 20.0) - 6.0).abs() < 0.1 && at(EqShape::LowShelf, 200.0, 6.0, 10000.0).abs() < 0.1);
        assert!((at(EqShape::HighShelf, 2000.0, -6.0, 20000.0) + 6.0).abs() < 0.3 && at(EqShape::HighShelf, 2000.0, -6.0, 50.0).abs() < 0.1);
        assert!(at(EqShape::Notch, 1000.0, 0.0, 1000.0) < -60.0 && at(EqShape::Notch, 1000.0, 0.0, 100.0).abs() < 0.1);
        // The gain of a cut does nothing.
        assert_eq!(at(EqShape::LowCut12, 1000.0, 12.0, 400.0), at(EqShape::LowCut12, 1000.0, -12.0, 400.0));
    }

    /// A shelf's Q makes a bump at its edge, but a small one: up to about 3 dB
    /// at the knob's top, the same up to 0.71 (none), growing with the knob.
    #[test]
    fn a_shelf_at_high_q_stays_near_its_gain() {
        let sr = 48_000.0;
        let extremes = |shape, gain_db, q| {
            let r: Vec<f32> = (0..2000).map(|k| eq_band_response_db(&EqBand { on: true, shape, freq: 1000.0, gain_db, q }, sr, 20.0 * 1000f32.powf(k as f32 / 1999.0))).collect();
            (r.iter().fold(f32::MIN, |a, &b| a.max(b)), r.iter().fold(f32::MAX, |a, &b| a.min(b)))
        };
        for shape in [EqShape::LowShelf, EqShape::HighShelf] {
            let (top, low) = extremes(shape, 15.0, 18.0);
            assert!(top < 18.5 && top > 16.0 && low > -3.5, "{shape:?} +15 dB Q 18: {top:.1} to {low:.1} dB");
            let (top, low) = extremes(shape, -15.0, 18.0);
            assert!(low > -18.5 && top < 3.5, "{shape:?} −15 dB Q 18: {top:.1} to {low:.1} dB");
            let (top, _) = extremes(shape, 15.0, 0.71);
            assert!(top < 15.05, "no bump at 0.71: {top}");
            let bumps: Vec<f32> = [1.0, 2.0, 4.0, 8.0, 18.0].map(|q| extremes(shape, 15.0, q).0).to_vec();
            assert!(bumps.windows(2).all(|w| w[1] > w[0]), "the knob does something all the way: {bumps:?}");
        }
    }

    /// Bands at 0 dB are left out, and that changes nothing: not the sound,
    /// not when one leaves 0 dB later (automated), whatever the blocks.
    #[test]
    fn flat_bands_change_nothing() {
        let sr = 48_000.0;
        let input: Vec<f32> = (0..9600).map(|i| ((i as f32 * 0.37).sin() + (i as f32 * 0.051).sin()) * 0.3).collect();
        let mut fx = EffectProcessor::new(&Effect::EqEight(EqEightParams::default()), sr);
        let (mut l, mut r) = (input.clone(), input.clone());
        fx.process(&mut l, &mut r, &ctx());
        assert_eq!(l, input, "the default EQ Eight (all at 0 dB) is the sound as it was");
        // The third band (a bell) automated from 0 dB up, in blocks of 64 or of 7.
        let run = |block: usize| {
            let mut fx = EffectProcessor::new(&Effect::EqEight(EqEightParams::default()), sr);
            let (mut l, mut r) = (input.clone(), input.clone());
            let mut at = 0;
            while at < input.len() {
                if at % 64 == 0 {
                    fx.set_param(7, if at < 3200 { 0.0 } else { (at - 3200) as f32 / 600.0 });
                }
                let n = block.min(64 - at % 64).min(input.len() - at);
                fx.process(&mut l[at..at + n], &mut r[at..at + n], &ctx());
                at += n;
            }
            l
        };
        let (a, b) = (run(64), run(7));
        assert_eq!(a[..3200], input[..3200]);
        assert!(a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6), "the same in any blocks");
        assert!(a[9000..].iter().zip(&input[9000..]).any(|(x, y)| (x - y).abs() > 0.01), "it does boost once up");
    }

    /// Not a number (a broken lane, a plugin's controller) never reaches a
    /// setting: it gets the default.
    #[test]
    fn a_parameter_set_to_nan_gets_its_default() {
        let mut e = Effect::EqEight(Default::default());
        for i in 0..e.params().len() {
            e.set(i, f32::NAN);
            assert_eq!(e.get(i), e.params()[i].default, "{i}");
        }
        let mut k = crate::model::InstrumentKind::SuperSaw(Default::default());
        k.set(0, f32::NAN);
        assert!(k.get(0).is_finite());
        let info = crate::model::ParamInfo::log("Freq", 20.0, 20_000.0, 1000.0, crate::model::Unit::Hertz);
        assert_eq!(info.denormalize(info.normalize(f32::NAN)), info.denormalize(info.normalize(1000.0)));
    }

    /// A new EQ Eight leaves the sound as it is, and its output gain is a gain.
    #[test]
    fn a_new_eq_eight_changes_nothing() {
        let mut fx = EffectProcessor::new(&Effect::EqEight(Default::default()), 48_000.0);
        let input: Vec<f32> = (0..4096).map(|i| ((i * 7919) % 2001) as f32 / 1000.0 - 1.0).collect();
        let (mut l, mut r) = (input.clone(), input.clone());
        fx.process(&mut l, &mut r, &ctx());
        assert!(l.iter().zip(&input).all(|(a, b)| (a - b).abs() < 1e-5), "flat bells and shelves are exact");
        let mut p = EqEightParams::default();
        p.output_db = -6.0;
        fx.set_param(EQ_BANDS * crate::model::EQ_BAND_PARAMS, -6.0);
        assert_eq!(fx.effect(), Effect::EqEight(p), "the last parameter is the output");
        assert!((sine_gain_db(&mut fx, 1000.0) + 6.0).abs() < 0.01);
        fx.set_param(4, -9.0);
        let Effect::EqEight(p) = fx.effect() else { unreachable!() };
        assert_eq!(p.bands[1].gain_db, -9.0, "band 2's gain");
        fx.set_param(4, -90.0);
        let Effect::EqEight(p) = fx.effect() else { unreachable!() };
        assert_eq!(p.bands[1].gain_db, -15.0, "within its range");
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn run(fx: &mut EffectProcessor, input: &[f32]) -> Vec<f32> {
        let (mut l, mut r) = (input.to_vec(), input.to_vec());
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            fx.process(a, b, &ctx());
        }
        l
    }

    /// Loud notes with quiet noise between them: the gate lets the notes
    /// through and silences the noise, down to its floor.
    #[test]
    fn the_gate_silences_whats_under_the_threshold() {
        let sr = 48_000.0;
        let mut noise = 12345u32;
        let input: Vec<f32> = (0..48_000)
            .map(|i| {
                noise = noise.wrapping_mul(1664525).wrapping_add(1013904223);
                let hiss = 0.003 * (noise as f32 / u32::MAX as f32 * 2.0 - 1.0);
                let note = if (i / 6000) % 2 == 0 { 0.3 * (TAU * 220.0 * i as f32 / sr).sin() } else { 0.0 };
                note + hiss
            })
            .collect();
        let gated = |floor_db: f32| run(&mut EffectProcessor::new(&Effect::Gate(GateParams { floor_db, release_ms: 20.0, ..Default::default() }), sr), &input);
        let out = gated(-80.0);
        // A note (from 12 000) and the gap after it (from 18 000), past the
        // edges: closing takes the fall under the threshold, hold and release.
        let (note, gap) = (12_500..17_500, 18_000 + 4000..23_500);
        assert!((rms(&out[note.clone()]) / rms(&input[note.clone()]) - 1.0).abs() < 0.01, "the notes pass as they are");
        assert!(rms(&out[gap.clone()]) < rms(&input[gap.clone()]) * 1e-3, "the hiss between them goes");
        let out = gated(-20.0);
        let ratio = rms(&out[gap.clone()]) / rms(&input[gap.clone()]);
        assert!((20.0 * ratio.log10() + 20.0).abs() < 0.5, "closed, it leaves the floor: {ratio}");
    }

    /// Return keeps it open while the sound is a little under the threshold
    /// (it doesn't flutter); hold keeps it open a while after.
    #[test]
    fn the_gate_return_and_hold() {
        let sr = 48_000.0;
        // −30 dB, then −42 (under −40, but within 3 dB of return), then −50.
        let level = |i: usize| match i / 12_000 { 0 => 0.0316, 1 => 0.0079, _ => 0.00316 };
        let input: Vec<f32> = (0..36_000).map(|i| level(i) * (TAU * 300.0 * i as f32 / sr).sin()).collect();
        let mut gate = EffectProcessor::new(&Effect::Gate(GateParams { hold_ms: 50.0, release_ms: 5.0, ..Default::default() }), sr);
        let out = run(&mut gate, &input);
        assert!((rms(&out[13_000..23_000]) / rms(&input[13_000..23_000]) - 1.0).abs() < 0.01, "still open within return");
        assert!((rms(&out[24_000..26_000]) / rms(&input[24_000..26_000]) - 1.0).abs() < 0.05, "held for 50 ms");
        assert!(rms(&out[28_000..36_000]) < rms(&input[28_000..36_000]) * 0.01, "then closed");
    }

    /// With a sidechain, the gate opens with its source's sound, not its own.
    #[test]
    fn a_keyed_gate_opens_with_its_source() {
        let sr = 48_000.0;
        let mut fx = EffectProcessor::new(&Effect::Gate(GateParams { sidechain: Some(1), hold_ms: 10.0, release_ms: 20.0, ..Default::default() }), sr);
        let pad: Vec<f32> = (0..256).map(|i| 0.5 * (TAU * 200.0 * i as f32 / sr).sin()).collect();
        let mut opened = Vec::new();
        for block in 0..40 {
            let key = if block % 20 < 10 { vec![0.5; 256] } else { vec![0.0; 256] };
            fx.feed_key(&key, &key);
            let (mut l, mut r) = (pad.clone(), pad.clone());
            fx.process(&mut l, &mut r, &ctx());
            opened.push(rms(&l[128..]) / rms(&pad[128..]));
        }
        assert!(opened[5] > 0.99 && opened[25] > 0.99, "open while the key sounds");
        // The key's level falls under the threshold in about 20 ms, then hold
        // and release: 50 ms, 10 blocks.
        assert!(opened[19] < 0.001 && opened[39] < 0.001, "closed when it doesn't, though the pad is loud: {opened:?}");
    }

    /// Hits that fall away: Attack makes their start louder or softer, Sustain
    /// their tail longer or shorter; a held note stays as it is.
    #[test]
    fn transient_shapes_attack_and_sustain() {
        let sr = 48_000.0;
        let hits: Vec<f32> = (0..48_000).map(|i| {
            let t = (i % 12_000) as f32 / sr;
            0.5 * (-t * 25.0).exp() * (TAU * 180.0 * t).sin()
        }).collect();
        let shaped = |attack, sustain| run(&mut EffectProcessor::new(&Effect::Transient(TransientParams { attack, sustain, output_db: 0.0 }), sr), &hits);
        // The third hit: its first 4 ms, and its tail at 80–160 ms.
        let (start, tail) = (24_000..24_192, 24_000 + 3840..24_000 + 7680);
        let gain = |out: &[f32], range: std::ops::Range<usize>| 20.0 * (rms(&out[range.clone()]) / rms(&hits[range])).log10();
        let flat = shaped(0.0, 0.0);
        assert!(gain(&flat, start.clone()).abs() < 0.01 && gain(&flat, tail.clone()).abs() < 0.01, "at 0 % it does nothing");
        let harder = shaped(1.0, 0.0);
        let softer = shaped(-1.0, 0.0);
        assert!(gain(&harder, start.clone()) > 4.0, "attack up: {}", gain(&harder, start.clone()));
        assert!(gain(&softer, start.clone()) < -4.0, "attack down: {}", gain(&softer, start.clone()));
        assert!(gain(&harder, tail.clone()).abs() < 1.0, "the tail stays: {}", gain(&harder, tail.clone()));
        let longer = shaped(0.0, 1.0);
        let shorter = shaped(0.0, -1.0);
        assert!(gain(&longer, tail.clone()) > 4.0, "sustain up: {}", gain(&longer, tail.clone()));
        assert!(gain(&shorter, tail.clone()) < -4.0, "sustain down: {}", gain(&shorter, tail.clone()));

        let held: Vec<f32> = (0..48_000).map(|i| 0.3 * (TAU * 440.0 * i as f32 / sr).sin()).collect();
        let mut fx = EffectProcessor::new(&Effect::Transient(TransientParams { attack: 1.0, sustain: 1.0, output_db: -6.0 }), sr);
        let out = run(&mut fx, &held);
        let g = 20.0 * (rms(&out[24_000..]) / rms(&held[24_000..])).log10();
        assert!((g + 6.0).abs() < 0.3, "a held note: only the output, {g} dB");
    }

    /// The output gain of the EQ Eight and the Transient, automated every 64
    /// samples, gives the same samples whatever blocks the audio comes in
    /// (it used to glide across each block: the export differed).
    #[test]
    fn automated_output_gain_is_the_same_in_any_blocks() {
        let input: Vec<f32> = (0..24_000).map(|i| ((i * 7919) % 2001) as f32 / 1000.0 - 1.0).collect();
        let run = |effect: Effect, index: usize, block: usize| {
            let mut fx = EffectProcessor::new(&effect, 48_000.0);
            let (mut l, mut r) = (input.clone(), input.clone());
            let mut at = 0;
            while at < input.len() {
                if at % 64 == 0 {
                    fx.set_param(index, -12.0 + 24.0 * at as f32 / input.len() as f32);
                }
                let n = block.min(64 - at % 64).min(input.len() - at);
                fx.process(&mut l[at..at + n], &mut r[at..at + n], &ctx());
                at += n;
            }
            l
        };
        for (effect, index) in [(Effect::EqEight(Default::default()), 24), (Effect::Transient(TransientParams { attack: 0.5, sustain: 0.3, output_db: 0.0 }), 2)] {
            let whole = run(effect, index, 64);
            for block in [1, 37, 100] {
                assert_eq!(run(effect, index, block), whole, "{} in blocks of {block}", effect.label());
            }
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
        let mut fx = EffectProcessor::new(&Effect::Utility(crate::model::UtilityParams { gain_db: 0.0, width: 0.0, ..Default::default() }), 48_000.0);
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
            let ctx = ProcessContext { sample_rate: rate, tempo: 120.0, beat: 0.0, beats_per_sample: 2.0 / rate as f64, scale: 0, meter: Default::default() };
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
    /// The oversampler as it first was (one tap at a time, rings read with a
    /// modulo): what the quicker one must still give.
    struct PlainOversampler {
        input: [f32; Oversampler::PHASE_TAPS],
        input_at: usize,
        output: [f32; Oversampler::TAPS],
        output_at: usize,
    }

    impl PlainOversampler {
        fn new() -> Self {
            Self { input: [0.0; Oversampler::PHASE_TAPS], input_at: 0, output: [0.0; Oversampler::TAPS], output_at: 0 }
        }

        fn up(&mut self, x: f32) -> [f32; 4] {
            const P: usize = Oversampler::PHASE_TAPS;
            self.input_at = (self.input_at + P - 1) % P;
            self.input[self.input_at] = x;
            let taps = &Oversampler::design();
            let mut out = [0f32; 4];
            for (phase, y) in out.iter_mut().enumerate() {
                let mut sum = 0.0;
                for j in 0..P {
                    let k = phase + 4 * j;
                    if k < Oversampler::TAPS {
                        sum += taps[k] * self.input[(self.input_at + j) % P];
                    }
                }
                *y = sum * 4.0;
            }
            out
        }

        fn down(&mut self, xs: [f32; 4]) -> f32 {
            const T: usize = Oversampler::TAPS;
            let taps = &Oversampler::design();
            let mut out = 0.0;
            for (i, x) in xs.into_iter().enumerate() {
                self.output_at = (self.output_at + T - 1) % T;
                self.output[self.output_at] = x;
                if i == 0 {
                    out = (0..T).map(|k| taps[k] * self.output[(self.output_at + k) % T]).sum();
                }
            }
            out
        }
    }

    /// The quick oversampler gives what the plain one did, to float rounding.
    #[test]
    fn the_quick_oversampler_sounds_as_the_plain_one() {
        let (mut quick, mut plain) = (Oversampler::new(), PlainOversampler::new());
        let mut seed = 3u32;
        let mut worst = 0f32;
        for i in 0..48_000 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 9) as f32 / (1u32 << 23) as f32 * 2.0 - 1.0;
            let x = 0.8 * (i as f32 * 0.05).sin() + 0.2 * noise;
            let (a, b) = (quick.up(x), plain.up(x));
            for k in 0..4 {
                worst = worst.max((a[k] - b[k]).abs());
            }
            // Through a hard clip, as the Saturator does.
            let clip = |v: [f32; 4]| v.map(|y| (y * 3.0).clamp(-1.0, 1.0));
            worst = worst.max((quick.down(clip(a)) - plain.down(clip(b))).abs());
        }
        assert!(worst < 2e-6, "{worst}");
    }

    /// How long the 4× Saturator takes (`cargo test --release -p engine -- --ignored --nocapture saturator_cost`).
    #[test]
    #[ignore]
    fn saturator_cost() {
        let sr = 48_000.0;
        let params = crate::model::SaturatorParams { drive_db: 12.0, kind: crate::model::KickDrive::Tube, oversample: true, ..Default::default() };
        let mut fx = EffectProcessor::new(&Effect::Saturator(params), sr);
        let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
        let seconds = 20.0;
        let blocks = (seconds * sr / 256.0) as usize;
        let start = std::time::Instant::now();
        for b in 0..blocks {
            for (i, (a, c)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                *a = ((b * 256 + i) as f32 * 0.03).sin() * 0.7;
                *c = *a;
            }
            fx.process(&mut l, &mut r, &ctx());
        }
        let share = start.elapsed().as_secs_f64() / seconds as f64 * 100.0;
        println!("Saturator 4×: {share:.3} % de un núcleo (estéreo, 48 kHz)");
    }

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

    /// Under its threshold it changes nothing; a hiss over it comes down
    /// about its range, and a low tone with it is left as it was.
    #[test]
    fn the_de_esser_takes_down_only_the_esses() {
        let sr = 48_000.0;
        let n = 48_000;
        // A 220 Hz tone, quiet: nothing to take.
        let tone: Vec<f32> = (0..n).map(|i| 0.3 * (std::f32::consts::TAU * 220.0 * i as f32 / sr).sin()).collect();
        let mut fx = EffectProcessor::new(&Effect::DeEsser(DeEsserParams { freq_hz: 6000.0, threshold_db: -30.0, range_db: 8.0 }), sr);
        let (mut l, mut r) = (tone.clone(), tone.clone());
        fx.process(&mut l, &mut r, &ctx());
        let worst = l.iter().zip(&tone).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(worst < 1e-4, "untouched: {worst}");
        // The tone and a loud 9 kHz whistle (an «s»): the whistle comes down, the tone stays.
        let mixed: Vec<f32> = (0..n).map(|i| i as f32 / sr).map(|t| 0.3 * (std::f32::consts::TAU * 220.0 * t).sin() + 0.3 * (std::f32::consts::TAU * 9000.0 * t).sin()).collect();
        let mut fx = EffectProcessor::new(&Effect::DeEsser(DeEsserParams { freq_hz: 6000.0, threshold_db: -30.0, range_db: 8.0 }), sr);
        let (mut l, mut r) = (mixed.clone(), mixed.clone());
        fx.process(&mut l, &mut r, &ctx());
        let level = |x: &[f32], hz: f32| {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, &v) in x.iter().enumerate() {
                let w = std::f64::consts::TAU * hz as f64 * i as f64 / sr as f64;
                re += v as f64 * w.cos();
                im += v as f64 * w.sin();
            }
            (2.0 * (re * re + im * im).sqrt() / x.len() as f64) as f32
        };
        let tail = &l[n / 2..];
        let whistle = 20.0 * (level(tail, 9000.0) / 0.3).log10();
        let low = 20.0 * (level(tail, 220.0) / 0.3).log10();
        println!("9 kHz {whistle:.1} dB, 220 Hz {low:.1} dB");
        // About the range: just over the split, the top part is a little louder than the sound.
        assert!((-10.5..=-7.0).contains(&whistle), "{whistle}");
        assert!(low.abs() < 0.5, "{low}");
    }

    /// A channel flipped comes back the right way; with the lows in mono, a
    /// bass only on the left is in both, and what is over it stays where it was.
    #[test]
    fn the_utility_flips_a_channel_and_puts_the_lows_in_mono() {
        let sr = 48_000.0;
        let n = 48_000;
        let wave = |hz: f32| -> Vec<f32> { (0..n).map(|i| (std::f32::consts::TAU * hz * i as f32 / sr).sin()).collect() };
        let tone = wave(440.0);
        let mut fx = EffectProcessor::new(&Effect::Utility(UtilityParams { flip_right: true, ..Default::default() }), sr);
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = (tone.clone(), tone.iter().map(|x| -x).collect());
        fx.process(&mut l, &mut r, &ctx());
        assert!(l.iter().zip(&r).all(|(a, b)| (a - b).abs() < 1e-6), "both the same way");
        // 50 Hz and 2 kHz, both only on the left.
        let (low, high) = (wave(50.0), wave(2000.0));
        let mut l: Vec<f32> = low.iter().zip(&high).map(|(a, b)| 0.5 * (a + b)).collect();
        let mut r = vec![0.0f32; n];
        let mut fx = EffectProcessor::new(&Effect::Utility(UtilityParams { bass_mono_hz: 150.0, ..Default::default() }), sr);
        fx.process(&mut l, &mut r, &ctx());
        let level = |x: &[f32], hz: f32| {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, &v) in x.iter().enumerate().skip(n / 2) {
                let w = std::f64::consts::TAU * hz as f64 * i as f64 / sr as f64;
                re += v as f64 * w.cos();
                im += v as f64 * w.sin();
            }
            (2.0 * (re * re + im * im).sqrt() / (n / 2) as f64) as f32
        };
        let (l50, r50, l2k, r2k) = (level(&l, 50.0), level(&r, 50.0), level(&l, 2000.0), level(&r, 2000.0));
        println!("50 Hz {l50:.3}/{r50:.3}, 2 kHz {l2k:.3}/{r2k:.3}");
        assert!((l50 - r50).abs() < 0.03 && (l50 - 0.25).abs() < 0.03, "the lows in both");
        assert!((l2k - 0.5).abs() < 0.02 && r2k < 0.02, "the rest where it was");
    }
}
