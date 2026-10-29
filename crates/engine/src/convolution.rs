//! The Convolution Reverb («Espacio»): impulse responses that Pulse synthesizes
//! (no third-party files), and a partitioned convolution that adds no latency.
//!
//! Responses ([`ConvKernel`]) are built off the audio thread and shared by every
//! instance with the same settings. When a running device's space, decay or
//! size change, the engine's handle has a background thread build the new one
//! ([`KernelBuilder`]) and hands it to the engine, which crossfades to it.
//!
//! The input, mixed to mono, is convolved with a stereo response:
//! - samples 0..64 in direct form;
//! - 64..1024 in 64-sample partitions (FFT 128), each computed as its block ends;
//! - 1024..8192 in 512-sample partitions, their work spread over 8 ticks;
//! - from 8192 on in 4096-sample partitions, spread over 64 ticks.
//!
//! A partition holds the FFT of left + i·right: one complex product per bin
//! gives both channels. All of it runs on its own 64-sample clock, so the
//! sound doesn't depend on the host's buffer size.

use crate::effects::{Svf, SvfCoefs, BUTTERWORTH_K};
use crate::engine::Target;
use crate::model::{ConvolutionParams, ReverbSpace};
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::{Arc, Mutex, Weak};

type C32 = Complex<f32>;

/// The convolution's clock: work happens every this many samples.
const TICK: usize = 64;
/// Samples of the response done in direct form.
const HEAD: usize = 64;
/// Per level: partition size, and the part of the response it covers.
const LEVELS: [(usize, usize, usize); 3] = [(64, 64, 1024), (512, 1024, 8192), (4096, 8192, usize::MAX)];
/// Input kept for the FFT windows (twice the largest partition).
const HISTORY: usize = 8192;
/// A new response starts on a boundary of the largest partitions, and is heard
/// once all its levels have output (one such partition later).
const SWITCH: usize = 4096;
/// Samples of the crossfade from one response to the next.
const FADE: usize = 2048;
/// The longest response.
pub const MAX_SECONDS: f32 = 10.0;
/// Energy of a response (per channel): a steady sound comes out of the reverb
/// about as loud as it went in.
const ENERGY: f32 = 1.0;
/// Octave bands the decay times are given at.
const BANDS: [f32; 8] = [125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
const SOUND: f32 = 343.0;

/// What a response is built from (decay and size to the hundredth).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KernelKey {
    pub space: ReverbSpace,
    decay: u16,
    size: u16,
    rate: u32,
}

impl KernelKey {
    pub fn new(p: &ConvolutionParams, sample_rate: f32) -> Self {
        let hundredths = |v: f32, range: std::ops::RangeInclusive<f32>| {
            let v = if v.is_nan() { 1.0 } else { v.clamp(*range.start(), *range.end()) };
            (v * 100.0).round() as u16
        };
        Self { space: p.space, decay: hundredths(p.decay, ConvolutionParams::DECAY), size: hundredths(p.size, ConvolutionParams::SIZE), rate: sample_rate.round().max(8000.0) as u32 }
    }

    fn decay(&self) -> f32 {
        self.decay as f32 / 100.0
    }

    fn size(&self) -> f32 {
        self.size as f32 / 100.0
    }
}

// ----- The spaces ------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Room,
    Plate,
    Spring,
}

/// How a space is built.
struct Spec {
    kind: Kind,
    /// Decay times (s) at the octave bands, 125 Hz to 16 kHz.
    t60: [f32; 8],
    /// Shoebox dimensions (m) for the early reflections; zero: none.
    room: [f32; 3],
    /// Wall reflection (amplitude).
    reflect: f32,
    /// Highest reflection order traced.
    order: i32,
    /// Early reflections are traced up to this long after the direct sound.
    early_ms: f32,
    /// Their share of the response's energy.
    early: f32,
    /// The tail goes from sparse echoes to dense noise over this time.
    mix_ms: f32,
    /// And fades in over this one.
    rise_ms: f32,
    /// Lowest and highest frequencies it has.
    band: (f32, f32),
    seed: u64,
}

fn spec(space: ReverbSpace) -> Spec {
    let room = |t60, room, reflect, order, early_ms, early, mix_ms, rise_ms, seed| Spec { kind: Kind::Room, t60, room, reflect, order, early_ms, early, mix_ms, rise_ms, band: (25.0, 20_000.0), seed };
    match space {
        ReverbSpace::Ambience => room([0.30, 0.28, 0.26, 0.25, 0.22, 0.18, 0.13, 0.09], [7.0, 5.0, 3.2], 0.78, 6, 70.0, 0.6, 25.0, 6.0, 11),
        ReverbSpace::Room => room([0.50, 0.48, 0.45, 0.42, 0.38, 0.32, 0.24, 0.15], [4.5, 3.8, 2.7], 0.82, 6, 55.0, 0.35, 30.0, 5.0, 12),
        ReverbSpace::Studio => room([0.75, 0.70, 0.65, 0.62, 0.58, 0.50, 0.38, 0.24], [9.0, 6.5, 3.6], 0.80, 5, 75.0, 0.3, 45.0, 8.0, 13),
        ReverbSpace::Hall => room([2.3, 2.1, 1.9, 1.8, 1.6, 1.3, 0.95, 0.55], [26.0, 17.0, 12.0], 0.85, 4, 110.0, 0.14, 90.0, 20.0, 14),
        ReverbSpace::BigHall => room([3.4, 3.2, 2.9, 2.7, 2.4, 2.0, 1.45, 0.8], [38.0, 25.0, 17.0], 0.87, 3, 150.0, 0.1, 120.0, 30.0, 15),
        ReverbSpace::Cathedral => room([6.2, 5.8, 5.2, 4.6, 3.8, 2.9, 1.9, 1.0], [70.0, 30.0, 32.0], 0.9, 3, 220.0, 0.07, 180.0, 50.0, 16),
        ReverbSpace::Plate => Spec { kind: Kind::Plate, t60: [1.7, 2.0, 2.2, 2.2, 2.1, 1.9, 1.6, 1.2], room: [0.0; 3], reflect: 0.0, order: 0, early_ms: 0.0, early: 0.0, mix_ms: 2.0, rise_ms: 0.5, band: (40.0, 20_000.0), seed: 17 },
        ReverbSpace::Spring => Spec { kind: Kind::Spring, t60: [1.2, 1.8, 2.0, 1.8, 1.4, 1.0, 0.6, 0.35], room: [0.0; 3], reflect: 0.0, order: 0, early_ms: 0.0, early: 0.0, mix_ms: 10.0, rise_ms: 1.0, band: (80.0, 5_500.0), seed: 18 },
        ReverbSpace::Infinite => Spec { kind: Kind::Room, t60: [7.0, 7.5, 7.5, 7.2, 6.8, 6.0, 4.8, 3.2], room: [0.0; 3], reflect: 0.0, order: 0, early_ms: 0.0, early: 0.0, mix_ms: 0.0, rise_ms: 350.0, band: (25.0, 20_000.0), seed: 19 },
    }
}

/// Decay time at `f` Hz, between the bands' (in octaves).
fn t60_at(t60: &[f32; 8], f: f32) -> f32 {
    let x = (f.max(1.0) / BANDS[0]).log2();
    if x <= 0.0 {
        return t60[0];
    }
    if x >= 7.0 {
        return t60[7];
    }
    let i = x.floor() as usize;
    let frac = x - i as f32;
    t60[i] + (t60[i + 1] - t60[i]) * frac
}

/// xorshift64*: the same noise on every computer.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in (0, 1).
    fn uniform(&mut self) -> f32 {
        ((self.next() >> 40) as f32 + 0.5) / (1u64 << 24) as f32
    }

    fn gauss(&mut self) -> f32 {
        let (a, b) = (self.uniform(), self.uniform());
        (-2.0 * a.ln()).sqrt() * (std::f32::consts::TAU * b).cos()
    }
}

/// An early reflection: when (s after the direct sound), how loud in each
/// channel, and how many walls it met.
#[derive(Clone, Copy, Debug)]
pub struct Reflection {
    pub delay: f32,
    pub gain: [f32; 2],
    pub order: i32,
}

/// The early reflections of a space (image sources in a shoebox, heard by two
/// spaced, slightly directional microphones), first first.
fn reflections(s: &Spec, size: f32) -> Vec<Reflection> {
    if s.order == 0 {
        return Vec::new();
    }
    let dims = s.room.map(|d| d * size);
    let source = [0.3 * dims[0], 0.62 * dims[1], (0.45 * dims[2]).min(1.7)];
    let centre = [0.7 * dims[0], 0.42 * dims[1], (0.5 * dims[2]).min(1.8)];
    let mics = [[centre[0], centre[1] + 0.3, centre[2]], [centre[0], centre[1] - 0.3, centre[2]]];
    let distance = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    let direct = distance(source, centre);
    let image = |n: i32, x: f32, d: f32| n as f32 * d + if n % 2 == 0 { x } else { d - x };
    let window = s.early_ms * 0.001 * size;
    let mut rng = Rng::new(s.seed ^ 0xE4);
    let k = s.order;
    let mut out = Vec::new();
    for nx in -k..=k {
        for ny in -k..=k {
            for nz in -k..=k {
                let order = nx.abs() + ny.abs() + nz.abs();
                if order == 0 || order > k {
                    continue;
                }
                let at = [image(nx, source[0], dims[0]), image(ny, source[1], dims[1]), image(nz, source[2], dims[2])];
                // Walls aren't perfect mirrors: later reflections land a little off.
                let jitter = (rng.uniform() - 0.5) * 0.0004 * (order - 1) as f32;
                // Each microphone hears it at its own time, louder from its side.
                for (m, mic) in mics.iter().enumerate() {
                    let d = distance(at, *mic);
                    let delay = (d - direct) / SOUND + jitter;
                    if delay <= 0.0 || delay > window {
                        continue;
                    }
                    let side = (at[1] - mic[1]) / d * if m == 0 { 1.0 } else { -1.0 };
                    let mut gain = [0.0; 2];
                    gain[m] = s.reflect.powi(order) * direct / d * (0.65 + 0.35 * side);
                    out.push(Reflection { delay, gain, order });
                }
            }
        }
    }
    out.sort_by(|a, b| a.delay.total_cmp(&b.delay));
    out
}

/// The early reflections as two channels of `n` samples, each order through
/// its own low-pass (walls and air take more highs at every bounce).
fn early_response(s: &Spec, size: f32, sr: f32, n: usize) -> [Vec<f32>; 2] {
    let list = reflections(s, size);
    let orders = s.order.max(1) as usize;
    let mut buckets = vec![[vec![0.0f32; n], vec![0.0f32; n]]; orders];
    for r in &list {
        let at = r.delay * sr;
        let i = at.floor() as usize;
        let frac = at - i as f32;
        for ch in 0..2 {
            if r.gain[ch] != 0.0 && i + 1 < n {
                let bucket = &mut buckets[(r.order as usize - 1).min(orders - 1)][ch];
                bucket[i] += r.gain[ch] * (1.0 - frac);
                bucket[i + 1] += r.gain[ch] * frac;
            }
        }
    }
    let mut out = [vec![0.0f32; n], vec![0.0f32; n]];
    for (o, bucket) in buckets.iter().enumerate() {
        let cutoff = (14_000.0 / ((o + 1) as f32).powf(0.7)).min(sr * 0.45);
        let k = 1.0 - (-std::f32::consts::TAU * cutoff / sr).exp();
        for ch in 0..2 {
            let mut z = 0.0;
            for (y, &x) in out[ch].iter_mut().zip(&bucket[ch]) {
                z += (x - z) * k;
                *y += z;
            }
        }
    }
    out
}

/// One channel of a spring tank: two springs, each a delay loop through a
/// chain of allpasses (low frequencies travel slower: each pass smears an
/// echo into the falling «drip» chirp, and more so every time round).
fn spring(n: usize, sr: f32, size: f32, t60: f32, channel: usize) -> Vec<f32> {
    const STAGES: usize = 40;
    let delays = if channel == 0 { [0.031, 0.0375] } else { [0.0335, 0.0405] };
    let mut out = vec![0.0f32; n];
    for (j, &seconds) in delays.iter().enumerate() {
        let len = ((seconds * size * sr) as usize).max(1);
        let g = 10f32.powf(-3.0 * len as f32 / sr / t60);
        let a = 0.6 + 0.04 * j as f32;
        let mut line = vec![0.0f32; len];
        let mut chain = [0.0f32; STAGES];
        let low = 1.0 - (-std::f32::consts::TAU * 4500.0 / sr).exp();
        let high = 1.0 - (-std::f32::consts::TAU * 90.0 / sr).exp();
        let (mut lz, mut hz) = (0.0f32, 0.0f32);
        for (i, y) in out.iter_mut().enumerate() {
            let at = i % len;
            let mut v = line[at];
            for state in chain.iter_mut() {
                let w = -a * v + *state;
                *state = v + a * w;
                v = w;
            }
            lz += (v - lz) * low;
            hz += (lz - hz) * high;
            let v = lz - hz;
            *y += v;
            line[at] = if i == 0 { 1.0 } else { 0.0 } + g * v;
        }
    }
    out
}

/// Shapes a stereo tail in short frames: each frequency decays at its own
/// rate (relative to `mid`'s, which the tail already has), and only the
/// space's band is kept. Left and right go through one FFT as left + i·right.
fn shape(x: &mut [Vec<f32>; 2], t60: &[f32; 8], mid: f32, band: (f32, f32), sr: f32) {
    const N: usize = 512;
    const HOP: usize = 128;
    let n = x[0].len();
    let mut planner = FftPlanner::<f32>::new();
    let (forward, inverse) = (planner.plan_fft_forward(N), planner.plan_fft_inverse(N));
    let window: Vec<f32> = (0..N).map(|i| (std::f32::consts::PI * i as f32 / N as f32).sin()).collect();
    // Per bin: decay relative to the middle (log10 of amplitude per second), and the band.
    let (rate, mask): (Vec<f32>, Vec<f32>) = (0..=N / 2)
        .map(|k| {
            let f = k as f32 * sr / N as f32;
            let rate = -3.0 * (1.0 / t60_at(t60, f) - 1.0 / mid);
            let (lo, hi) = band;
            let high_pass = { let r = (f / lo).powi(4); r / (1.0 + r) };
            let low_pass = 1.0 / (1.0 + (f / hi).powi(4));
            (rate, (high_pass * low_pass).sqrt())
        })
        .unzip();
    let mut out = [vec![0.0f32; n + 2 * N], vec![0.0f32; n + 2 * N]];
    let mut frame = vec![C32::default(); N];
    let mut scratch = vec![C32::default(); forward.get_inplace_scratch_len().max(inverse.get_inplace_scratch_len())];
    // Frames start a frame before the response (padded): every sample gets the full overlap.
    let mut start: isize = -((N - HOP) as isize);
    while start < n as isize {
        for (i, c) in frame.iter_mut().enumerate() {
            let at = start + i as isize;
            let (l, r) = if at >= 0 && (at as usize) < n { (x[0][at as usize], x[1][at as usize]) } else { (0.0, 0.0) };
            *c = C32::new(l * window[i], r * window[i]);
        }
        forward.process_with_scratch(&mut frame, &mut scratch);
        let time = ((start + N as isize / 2).max(0)) as f32 / sr;
        for k in 0..=N / 2 {
            let g = mask[k] * 10f32.powf(rate[k] * time);
            frame[k] *= g;
            if k > 0 && k < N / 2 {
                frame[N - k] *= g;
            }
        }
        inverse.process_with_scratch(&mut frame, &mut scratch);
        // Sine windows twice at a quarter-frame hop add up to 2.
        let scale = 1.0 / (N as f32 * 2.0);
        for (i, c) in frame.iter().enumerate() {
            let at = start + i as isize + (N - HOP) as isize;
            if at >= 0 {
                out[0][at as usize] += c.re * window[i] * scale;
                out[1][at as usize] += c.im * window[i] * scale;
            }
        }
        start += HOP as isize;
    }
    let offset = N - HOP;
    for ch in 0..2 {
        x[ch].copy_from_slice(&out[ch][offset..offset + n]);
    }
}

fn energy(x: &[f32]) -> f32 {
    x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() as f32
}

/// How long the response of `p` is (s).
fn length(s: &Spec, decay: f32, size: f32) -> f32 {
    let longest = s.t60.iter().fold(0.0f32, |a, &b| a.max(b)) * decay;
    (longest + s.rise_ms * 0.003 * size + s.early_ms * 0.001 * size).min(MAX_SECONDS)
}

/// The stereo impulse response of a space (reverb only, without the direct sound).
fn synthesize(key: KernelKey) -> [Vec<f32>; 2] {
    let sr = key.rate as f32;
    let s = spec(key.space);
    let (decay, size) = (key.decay(), key.size());
    let t60 = s.t60.map(|t| t * decay);
    let mid = t60_at(&t60, 1000.0);
    let n = ((length(&s, decay, size) * sr) as usize).max(HEAD);
    let rise = s.rise_ms * 0.001 * size;
    let mix = s.mix_ms * 0.001 * size * sr;
    let mut tail = [vec![0.0f32; n], vec![0.0f32; n]];
    for (ch, out) in tail.iter_mut().enumerate() {
        // Echoes, sparse at first (fewer paths have arrived) and then dense noise.
        let mut rng = Rng::new(s.seed.wrapping_mul(31).wrapping_add(ch as u64 + 1));
        for (i, v) in out.iter_mut().enumerate() {
            let t = i as f32 / sr;
            let density = if mix > 0.0 { ((i as f32 + 1.0) / mix).powi(2).clamp(0.002, 1.0) } else { 1.0 };
            let x = if density >= 1.0 {
                rng.gauss()
            } else if rng.uniform() < density {
                rng.gauss() / density.sqrt()
            } else {
                0.0
            };
            let swell = if rise > 0.0 { 1.0 - (-t / rise).exp() } else { 1.0 };
            *v = x * swell * 10f32.powf(-3.0 * t / mid);
        }
    }
    if s.kind == Kind::Spring {
        // Mostly the springs, with a little diffuse noise for body.
        for (ch, out) in tail.iter_mut().enumerate() {
            let springs = spring(n, sr, size, mid, ch);
            let (e_noise, e_spring) = (energy(out).max(1e-20), energy(&springs).max(1e-20));
            let (a, b) = ((0.2 / e_noise).sqrt(), (0.8 / e_spring).sqrt());
            for (v, s) in out.iter_mut().zip(&springs) {
                *v = *v * a + s * b;
            }
        }
    }
    shape(&mut tail, &t60, mid, s.band, sr);
    if s.early > 0.0 {
        let early = early_response(&s, size, sr, n);
        let e_tail = energy(&tail[0]) + energy(&tail[1]);
        let e_early = (energy(&early[0]) + energy(&early[1])).max(1e-20);
        let gain = (s.early / (1.0 - s.early) * e_tail / e_early).sqrt();
        for ch in 0..2 {
            for (v, e) in tail[ch].iter_mut().zip(&early[ch]) {
                *v += e * gain;
            }
        }
    }
    // The end fades out (a response cut short at its longest).
    let fade = n * 3 / 20;
    for ch in 0..2 {
        for i in 0..fade {
            let g = 0.5 - 0.5 * (std::f32::consts::PI * (i as f32 + 0.5) / fade as f32).cos();
            tail[ch][n - 1 - i] *= g;
        }
    }
    let total = (energy(&tail[0]) + energy(&tail[1])) * 0.5;
    // A spring has all its energy in the mids, where voices and guitars are: a little less.
    let target = if s.kind == Kind::Spring { ENERGY * 0.6 } else { ENERGY };
    let gain = (target / total.max(1e-20)).sqrt();
    for ch in 0..2 {
        tail[ch].iter_mut().for_each(|v| *v *= gain);
    }
    tail
}

/// The impulse response of `p` at `sample_rate` (left, right): reverb only.
pub fn impulse_response(p: &ConvolutionParams, sample_rate: f32) -> [Vec<f32>; 2] {
    synthesize(KernelKey::new(p, sample_rate))
}

/// What the device draws: the early reflections and how the tail dies.
pub struct SpaceSketch {
    /// Seconds after the direct sound (pre-delay left out) and level (dB, the loudest 0).
    pub reflections: Vec<(f32, f32)>,
    /// Decay times (s) of the lows (250 Hz), mids (1 kHz) and highs (8 kHz).
    pub t60: [f32; 3],
    /// When the tail is at its loudest (s).
    pub tail_peak: f32,
    /// The response's length (s).
    pub length: f32,
}

/// Cheap (no synthesis): fine to call every frame.
pub fn space_sketch(p: &ConvolutionParams) -> SpaceSketch {
    let key = KernelKey::new(p, 48_000.0);
    let s = spec(p.space);
    let (decay, size) = (key.decay(), key.size());
    let t60 = s.t60.map(|t| t * decay);
    let list = reflections(&s, size);
    let loudest = list.iter().map(|r| r.gain[0].abs().max(r.gain[1].abs())).fold(1e-9f32, f32::max);
    let reflections = list.iter().map(|r| (r.delay, 20.0 * (r.gain[0].abs().max(r.gain[1].abs()) / loudest).max(1e-4).log10())).collect();
    let mid = t60_at(&t60, 1000.0);
    // The swell's peak against the decay: where (1 - e^(-t/rise))·10^(-3t/T) is highest.
    let rise = s.rise_ms * 0.001 * size;
    let tail_peak = if rise > 0.0 { rise * (1.0 + 3.0 * std::f32::consts::LN_10 * rise / mid).ln() } else { 0.0 };
    SpaceSketch { reflections, t60: [t60_at(&t60, 250.0), mid, t60_at(&t60, 8000.0)], tail_peak, length: length(&s, decay, size) }
}

// ----- Kernels ----------------------------------------------------------------------------

/// A response ready to convolve with: its first samples as they are, the rest
/// as partition spectra (left + i·right, scaled for the inverse FFT).
pub struct ConvKernel {
    key: KernelKey,
    head: [[f32; HEAD]; 2],
    parts: [Vec<C32>; 3],
    /// Partitions per level.
    count: [usize; 3],
    /// Samples.
    len: usize,
}

impl ConvKernel {
    fn from_response(key: KernelKey, response: &[Vec<f32>; 2]) -> Self {
        let len = response[0].len().min(response[1].len()).min((MAX_SECONDS * key.rate as f32) as usize);
        let at = |ch: usize, i: usize| if i < len { response[ch][i] } else { 0.0 };
        let head = [std::array::from_fn(|i| at(0, i)), std::array::from_fn(|i| at(1, i))];
        let mut planner = FftPlanner::<f32>::new();
        let mut count = [0; 3];
        let parts = std::array::from_fn(|level| {
            let (p, start, end) = LEVELS[level];
            let covered = len.min(end);
            count[level] = if covered > start { (covered - start).div_ceil(p) } else { 0 };
            let fft = planner.plan_fft_forward(2 * p);
            let mut spectra = vec![C32::default(); count[level] * 2 * p];
            let scale = 1.0 / (2 * p) as f32;
            for (k, part) in spectra.chunks_mut(2 * p).enumerate() {
                for (j, c) in part.iter_mut().take(p).enumerate() {
                    let i = start + k * p + j;
                    *c = C32::new(at(0, i) * scale, at(1, i) * scale);
                }
                fft.process(part);
            }
            spectra
        });
        Self { key, head, parts, count, len }
    }

    pub fn key(&self) -> KernelKey {
        self.key
    }
}

/// The responses alive somewhere (instances with the same settings share theirs).
static KERNELS: Mutex<Vec<(KernelKey, Weak<ConvKernel>)>> = Mutex::new(Vec::new());

/// The response for `key`, built if no instance has it. Allocates and can take
/// tens of milliseconds: never on the audio thread.
pub fn kernel(key: KernelKey) -> Arc<ConvKernel> {
    if let Some(found) = KERNELS.lock().ok().and_then(|cache| cache.iter().find(|(k, _)| *k == key).and_then(|(_, w)| w.upgrade())) {
        return found;
    }
    let built = Arc::new(ConvKernel::from_response(key, &synthesize(key)));
    if let Ok(mut cache) = KERNELS.lock() {
        cache.retain(|(k, w)| w.strong_count() > 0 && *k != key);
        cache.push((key, Arc::downgrade(&built)));
    }
    built
}

// ----- The convolution --------------------------------------------------------------------

/// One size of partitions: its input spectra (newest at `newest`), and per
/// slot (0: the response playing, 1: the one fading out) what it adds up and
/// outputs (left in re, right in im).
struct Level {
    p: usize,
    ticks: usize,
    cap: usize,
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
    fdl: Vec<C32>,
    newest: usize,
    acc: [Vec<C32>; 2],
    out: [Vec<C32>; 2],
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum State {
    Steady,
    /// Both responses run; the old one is heard until this sample.
    Warming(usize),
    /// Crossfading: samples done.
    Fading(usize),
}

pub struct Convolution {
    pub(crate) params: ConvolutionParams,
    rate: f32,
    kernel: Arc<ConvKernel>,
    /// The response fading out (or faded out, waiting to be handed back).
    old: Option<Arc<ConvKernel>>,
    /// The next one, waiting for a boundary.
    next: Option<Arc<ConvKernel>>,
    state: State,
    /// Equal-gain crossfade (the same space: correlated tails), else equal-power.
    linear: bool,
    clock: usize,
    pre: Vec<f32>,
    pre_at: usize,
    pre_delay: f32,
    /// The newest input first, twice over (a contiguous window at any start).
    recent: [f32; 2 * HEAD],
    recent_at: usize,
    history: Vec<f32>,
    levels: [Level; 3],
    scratch: Vec<C32>,
    /// Samples since the input last made a sound; past the response's length
    /// the convolution rests.
    quiet: usize,
    idle: bool,
    low: [Svf; 2],
    high: [Svf; 2],
    low_coefs: SvfCoefs,
    high_coefs: SvfCoefs,
    low_hz: f32,
    high_hz: f32,
    mix: f32,
    width: f32,
}

impl Convolution {
    /// Builds (or shares) its response and allocates everything: off the audio thread.
    pub fn new(params: ConvolutionParams, rate: f32) -> Self {
        Self::with_kernel(params, rate, kernel(KernelKey::new(&params, rate)))
    }

    fn with_kernel(params: ConvolutionParams, rate: f32, kernel: Arc<ConvKernel>) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let longest = (MAX_SECONDS * rate) as usize;
        let mut scratch_len = 0;
        let levels = LEVELS.map(|(p, start, end)| {
            let covered = longest.min(end);
            let cap = if covered > start { (covered - start).div_ceil(p) } else { 0 }.max(1);
            let (fft, ifft) = (planner.plan_fft_forward(2 * p), planner.plan_fft_inverse(2 * p));
            scratch_len = scratch_len.max(fft.get_inplace_scratch_len()).max(ifft.get_inplace_scratch_len());
            Level { p, ticks: p / TICK, cap, fft, ifft, fdl: touched(cap * 2 * p), newest: 0, acc: [touched(2 * p), touched(2 * p)], out: [touched(p), touched(p)] }
        });
        let low_hz = params.low_cut_hz.clamp(20.0, 2000.0);
        let high_hz = params.high_cut_hz.clamp(1000.0, 20000.0);
        Self {
            params,
            rate,
            kernel,
            old: None,
            next: None,
            state: State::Steady,
            linear: false,
            clock: 0,
            pre: touched((0.25 * rate) as usize + 4),
            pre_at: 0,
            pre_delay: params.predelay_ms.clamp(0.0, 250.0) * 0.001 * rate,
            recent: [0.0; 2 * HEAD],
            recent_at: 0,
            history: touched(HISTORY),
            levels,
            scratch: touched(scratch_len),
            quiet: 0,
            idle: false,
            low: Default::default(),
            high: Default::default(),
            low_coefs: SvfCoefs::new(low_hz, BUTTERWORTH_K, rate),
            high_coefs: SvfCoefs::new(high_hz, BUTTERWORTH_K, rate),
            low_hz,
            high_hz,
            mix: params.mix.clamp(0.0, 1.0),
            width: params.width.clamp(0.0, 1.0),
        }
    }

    /// The response its space, decay and size ask for.
    pub fn wanted(&self) -> KernelKey {
        KernelKey::new(&self.params, self.rate)
    }

    /// Takes a new response, to start at the next boundary. Gives back what it
    /// won't use (a stale or repeated response, or one it was keeping), to be
    /// freed off the audio thread.
    pub fn offer(&mut self, kernel: Arc<ConvKernel>) -> Option<Arc<ConvKernel>> {
        let playing = self.next.as_ref().unwrap_or(&self.kernel).key;
        if kernel.key != self.wanted() || kernel.key == playing {
            return Some(kernel);
        }
        self.next.replace(kernel)
    }

    /// A response done fading out, to be freed off the audio thread.
    pub fn collect(&mut self) -> Option<Arc<ConvKernel>> {
        if self.state == State::Steady { self.old.take() } else { None }
    }

    /// Whether it still has a response to start or to hand back.
    pub fn busy(&self) -> bool {
        self.next.is_some() || self.old.is_some() || self.state != State::Steady
    }

    /// The key of the response heard now (tests).
    pub fn playing(&self) -> KernelKey {
        self.kernel.key
    }

    pub fn reset(&mut self) {
        self.pre.fill(0.0);
        self.recent = [0.0; 2 * HEAD];
        self.history.fill(0.0);
        for level in &mut self.levels {
            level.fdl.fill(C32::default());
            level.acc.iter_mut().chain(level.out.iter_mut()).for_each(|b| b.fill(C32::default()));
        }
        self.low = Default::default();
        self.high = Default::default();
        if self.state != State::Steady {
            self.state = State::Steady;
        }
    }

    /// The reverb of the current sample, from response slot `s`.
    #[inline]
    fn wet(&self, kernel: &ConvKernel, s: usize) -> (f32, f32) {
        let recent = &self.recent[self.recent_at..self.recent_at + HEAD];
        let (mut l, mut r) = (0.0f32, 0.0f32);
        for ((x, hl), hr) in recent.iter().zip(&kernel.head[0]).zip(&kernel.head[1]) {
            l += x * hl;
            r += x * hr;
        }
        for level in &self.levels {
            let o = level.out[s][self.clock % level.p];
            l += o.re;
            r += o.im;
        }
        (l, r)
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = self.params;
        let rate = self.rate;
        let pre_target = p.predelay_ms.clamp(0.0, 250.0) * 0.001 * rate;
        let glide = 1.0 - (-1.0 / (0.02 * rate)).exp();
        let (mix, width) = (p.mix.clamp(0.0, 1.0), p.width.clamp(0.0, 1.0));
        let len = self.pre.len();
        for (a, b) in left.iter_mut().zip(right.iter_mut()) {
            // Pre-delay, gliding when it changes.
            self.pre[self.pre_at] = 0.5 * (*a + *b);
            self.pre_delay += (pre_target - self.pre_delay) * glide;
            let whole = self.pre_delay.floor();
            let frac = self.pre_delay - whole;
            let at = self.pre_at + len - whole as usize;
            let x = self.pre[at % len] + (self.pre[(at + len - 1) % len] - self.pre[at % len]) * frac;
            self.pre_at = (self.pre_at + 1) % len;

            self.recent_at = (self.recent_at + HEAD - 1) % HEAD;
            self.recent[self.recent_at] = x;
            self.recent[self.recent_at + HEAD] = x;
            self.history[self.clock % HISTORY] = x;
            if x.abs() > 1e-7 {
                self.quiet = 0;
                self.idle = false;
            } else {
                self.quiet = self.quiet.saturating_add(1);
            }

            let (mut wl, mut wr) = match self.state {
                _ if self.idle => (0.0, 0.0),
                State::Steady => self.wet(&self.kernel, 0),
                State::Warming(_) => self.old.as_deref().map_or((0.0, 0.0), |old| self.wet(old, 1)),
                State::Fading(done) => {
                    let (nl, nr) = self.wet(&self.kernel, 0);
                    let (ol, or) = self.old.as_deref().map_or((0.0, 0.0), |old| self.wet(old, 1));
                    let t = (done as f32 + 0.5) / FADE as f32;
                    let (g_new, g_old) = if self.linear { (t, 1.0 - t) } else { ((t * std::f32::consts::FRAC_PI_2).sin(), (t * std::f32::consts::FRAC_PI_2).cos()) };
                    self.state = if done + 1 >= FADE { State::Steady } else { State::Fading(done + 1) };
                    (nl * g_new + ol * g_old, nr * g_new + or * g_old)
                }
            };

            // At the ends of their ranges the cuts are off.
            let (low_on, high_on) = (self.low_hz > 20.5, self.high_hz < 19_990.0);
            for (ch, w) in [&mut wl, &mut wr].into_iter().enumerate() {
                if low_on {
                    *w = self.low[ch].tick(*w, &self.low_coefs).2;
                }
                if high_on {
                    *w = self.high[ch].tick(*w, &self.high_coefs).0;
                }
            }
            self.mix += (mix - self.mix) * 0.002;
            self.width += (width - self.width) * 0.002;
            let mid = 0.5 * (wl + wr);
            let side = 0.5 * (wl - wr) * self.width;
            *a += (mid + side - *a) * self.mix;
            *b += (mid - side - *b) * self.mix;

            self.clock += 1;
            if self.clock % TICK == 0 {
                self.tick();
            }
        }
    }

    /// Every 64 samples: the filters' glide, a new response, and the partitions' work.
    fn tick(&mut self) {
        let clock = self.clock;
        let p = self.params;
        let low = p.low_cut_hz.clamp(20.0, 2000.0);
        let high = p.high_cut_hz.clamp(1000.0, 20000.0);
        if low != self.low_hz || high != self.high_hz {
            // About 20 ms to get there, in steps of a tick.
            let glide = |from: f32, to: f32| if (to / from - 1.0).abs() < 0.001 { to } else { from * (to / from).powf(0.25) };
            self.low_hz = glide(self.low_hz, low);
            self.high_hz = glide(self.high_hz, high);
            self.low_coefs = SvfCoefs::new(self.low_hz, BUTTERWORTH_K, self.rate);
            self.high_coefs = SvfCoefs::new(self.high_hz, BUTTERWORTH_K, self.rate);
        }
        if let State::Warming(until) = self.state {
            if clock >= until {
                self.state = State::Fading(0);
            }
        }
        if clock % SWITCH == 0 && self.state == State::Steady && self.old.is_none() {
            if let Some(next) = self.next.take() {
                let previous = std::mem::replace(&mut self.kernel, next);
                self.linear = previous.key.space == self.kernel.key.space;
                self.old = Some(previous);
                if !self.idle {
                    for level in &mut self.levels {
                        level.acc.swap(0, 1);
                        level.out.swap(0, 1);
                    }
                    self.state = State::Warming(clock + SWITCH);
                }
            }
        }
        let span = self.kernel.len.max(self.old.as_ref().map_or(0, |k| k.len)) + 3 * SWITCH;
        if !self.idle && self.quiet > span && self.state == State::Steady && self.next.is_none() {
            // Silence for longer than the response: nothing left to add up.
            self.idle = true;
            for level in &mut self.levels {
                level.acc.iter_mut().chain(level.out.iter_mut()).for_each(|b| b.fill(C32::default()));
            }
        }
        let slots = if self.state == State::Steady { 1 } else { 2 };
        let kernels = [Some(&*self.kernel), self.old.as_deref()];
        for (index, level) in self.levels.iter_mut().enumerate() {
            let two = 2 * level.p;
            let phase = (clock / TICK) % level.ticks;
            if phase == 0 {
                if level.ticks > 1 && !self.idle {
                    for s in 0..slots {
                        finish(level, s, &mut self.scratch);
                    }
                }
                level.newest = (level.newest + 1) % level.cap;
                let at = level.newest * two;
                let x = &mut level.fdl[at..at + two];
                if self.idle {
                    x.fill(C32::default());
                    continue;
                }
                for (j, c) in x.iter_mut().enumerate() {
                    *c = C32::new(self.history[(clock + HISTORY - two + j) % HISTORY], 0.0);
                }
                level.fft.process_with_scratch(x, &mut self.scratch);
                for s in 0..slots {
                    level.acc[s].fill(C32::default());
                }
            }
            if self.idle {
                continue;
            }
            for (s, kernel) in kernels.iter().enumerate().take(slots) {
                let Some(kernel) = kernel else { continue };
                let count = kernel.count[index];
                let (from, to) = (count * phase / level.ticks, count * (phase + 1) / level.ticks);
                let acc = &mut level.acc[s];
                for k in from..to {
                    let x = (level.newest + level.cap - k % level.cap) % level.cap;
                    let x = &level.fdl[x * two..(x + 1) * two];
                    let h = &kernel.parts[index][k * two..(k + 1) * two];
                    for ((a, x), h) in acc.iter_mut().zip(x).zip(h) {
                        *a += x * h;
                    }
                }
            }
            if level.ticks == 1 {
                for s in 0..slots {
                    finish(level, s, &mut self.scratch);
                }
            }
        }
    }
}

/// Zeros, written here: the system hands memory out lazily, and the first
/// write to each page would otherwise stall the audio thread.
fn touched<T: Clone + Default>(n: usize) -> Vec<T> {
    let mut v = vec![T::default(); n];
    std::hint::black_box(&mut v[..]);
    v.fill(T::default());
    v
}

/// A block's sum back to samples: the second half is the output (overlap-save).
fn finish(level: &mut Level, s: usize, scratch: &mut [C32]) {
    let p = level.p;
    level.ifft.process_with_scratch(&mut level.acc[s], scratch);
    let (acc, out) = (&level.acc[s], &mut level.out[s]);
    out.copy_from_slice(&acc[p..2 * p]);
}

// ----- Building responses for a running engine ---------------------------------------------

/// A response built for the device at `index` of `target`'s chain.
pub struct KernelDelivery {
    pub(crate) target: Target,
    pub(crate) index: usize,
    pub(crate) kernel: Arc<ConvKernel>,
}

struct Request {
    target: Target,
    index: usize,
    key: KernelKey,
}

/// Builds new responses on a thread of its own (started the first time one is
/// asked for) and hands them straight to the engine.
pub(crate) struct KernelBuilder {
    requests: Option<std::sync::mpsc::Sender<Request>>,
    queue: Option<rtrb::Producer<KernelDelivery>>,
    pending: Arc<std::sync::atomic::AtomicUsize>,
}

/// Responses on their way to the engine at once (each up to a few MB).
pub(crate) const DELIVERIES: usize = 8;

impl KernelBuilder {
    pub(crate) fn new(queue: rtrb::Producer<KernelDelivery>) -> Self {
        Self { requests: None, queue: Some(queue), pending: Default::default() }
    }

    /// Asks for the response `params` need at that device.
    pub(crate) fn request(&mut self, target: Target, index: usize, params: &ConvolutionParams, sample_rate: f32) {
        use std::sync::atomic::Ordering;
        if self.requests.is_none() {
            let Some(mut queue) = self.queue.take() else { return };
            let (tx, rx) = std::sync::mpsc::channel::<Request>();
            let pending = self.pending.clone();
            let spawned = std::thread::Builder::new().name("pulse-reverb".into()).spawn(move || {
                let mut waiting: Vec<Request> = Vec::new();
                loop {
                    if waiting.is_empty() {
                        match rx.recv() {
                            Ok(r) => waiting.push(r),
                            Err(_) => return,
                        }
                    }
                    // Only the latest settings of each device matter.
                    while let Ok(r) = rx.try_recv() {
                        match waiting.iter_mut().find(|w| w.target == r.target && w.index == r.index) {
                            Some(w) => {
                                w.key = r.key;
                                pending.fetch_sub(1, Ordering::Relaxed);
                            }
                            None => waiting.push(r),
                        }
                    }
                    let r = waiting.remove(0);
                    let mut delivery = KernelDelivery { target: r.target, index: r.index, kernel: kernel(r.key) };
                    loop {
                        match queue.push(delivery) {
                            Ok(()) => break,
                            Err(rtrb::PushError::Full(back)) => {
                                if queue.is_abandoned() {
                                    return;
                                }
                                delivery = back;
                                std::thread::sleep(std::time::Duration::from_millis(5));
                            }
                        }
                    }
                    pending.fetch_sub(1, Ordering::Relaxed);
                }
            });
            if spawned.is_err() {
                return;
            }
            self.requests = Some(tx);
        }
        if let Some(tx) = &self.requests {
            self.pending.fetch_add(1, Ordering::Relaxed);
            if tx.send(Request { target, index, key: KernelKey::new(params, sample_rate) }).is_err() {
                self.pending.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    /// Requests not yet built and handed over.
    pub(crate) fn pending(&self) -> usize {
        self.pending.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn params(space: ReverbSpace, decay: f32) -> ConvolutionParams {
        // No pre-delay, cuts off, all wet: the output is the convolution itself.
        ConvolutionParams { space, decay, size: 1.0, predelay_ms: 0.0, low_cut_hz: 20.0, high_cut_hz: 20_000.0, width: 1.0, mix: 1.0 }
    }

    fn noise(n: usize, seed: u64) -> Vec<f32> {
        let mut rng = Rng::new(seed);
        (0..n).map(|_| rng.uniform() * 2.0 - 1.0).collect()
    }

    fn run(fx: &mut Convolution, input: &[f32], block: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (input.to_vec(), input.to_vec());
        for (a, b) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
            fx.process(a, b);
        }
        (l, r)
    }

    /// Linear convolution through one big FFT, in f64.
    fn reference(x: &[f32], h: &[f32], n: usize) -> Vec<f32> {
        let size = (x.len() + h.len()).next_power_of_two();
        let mut planner = FftPlanner::<f64>::new();
        let (f, i) = (planner.plan_fft_forward(size), planner.plan_fft_inverse(size));
        let mut a: Vec<Complex<f64>> = (0..size).map(|k| Complex::new(x.get(k).copied().unwrap_or(0.0) as f64, 0.0)).collect();
        let mut b: Vec<Complex<f64>> = (0..size).map(|k| Complex::new(h.get(k).copied().unwrap_or(0.0) as f64, 0.0)).collect();
        f.process(&mut a);
        f.process(&mut b);
        for (a, b) in a.iter_mut().zip(&b) {
            *a *= b;
        }
        i.process(&mut a);
        a.iter().take(n).map(|c| (c.re / size as f64) as f32).collect()
    }

    #[test]
    fn it_is_the_convolution_with_its_response_with_no_latency() {
        for space in [ReverbSpace::Room, ReverbSpace::Hall] {
            let p = params(space, 1.0);
            let [hl, hr] = impulse_response(&p, SR);
            let n = 30_000;
            // A burst, then silence: the tail too.
            let mut x = noise(n, 3);
            x[12_000..].fill(0.0);
            let mut fx = Convolution::new(p, SR);
            let (l, r) = run(&mut fx, &x, 128);
            for (out, h) in [(&l, &hl), (&r, &hr)] {
                let want = reference(&x, h, n);
                let peak = want.iter().fold(0f32, |m, v| m.max(v.abs()));
                let error = out.iter().zip(&want).fold(0f32, |m, (a, b)| m.max((a - b).abs()));
                assert!(error < peak * 1e-4, "{space:?}: error {error} against {peak}");
            }
        }
        // An impulse comes out as the response from its first sample.
        let p = params(ReverbSpace::Plate, 1.0);
        let [hl, _] = impulse_response(&p, SR);
        let mut x = vec![0.0; 2000];
        x[0] = 1.0;
        let (l, _) = run(&mut Convolution::new(p, SR), &x, 100);
        for i in 0..2000 {
            assert!((l[i] - hl[i]).abs() < 1e-5, "sample {i}: {} against {}", l[i], hl[i]);
        }
    }

    #[test]
    fn the_sound_does_not_depend_on_the_buffer_size() {
        let mut p = params(ReverbSpace::Hall, 0.6);
        p.predelay_ms = 23.0;
        p.low_cut_hz = 150.0;
        p.high_cut_hz = 9000.0;
        p.width = 0.7;
        p.mix = 0.4;
        let mut x = noise(40_000, 5);
        x[20_000..].fill(0.0);
        let whole = run(&mut Convolution::new(p, SR), &x, 4096);
        for block in [1, 7, 64, 128, 333, 1000] {
            let parts = run(&mut Convolution::new(p, SR), &x, block);
            assert!(parts == whole, "block {block}");
        }
    }

    #[test]
    fn the_pre_delay_delays_the_reverb_only() {
        let mut p = params(ReverbSpace::Room, 1.0);
        let mut x = vec![0.0; 6000];
        x[0] = 1.0;
        let (dry, _) = run(&mut Convolution::new(p, SR), &x, 64);
        p.predelay_ms = 50.0;
        let (late, _) = run(&mut Convolution::new(p, SR), &x, 64);
        let shift = 2400;
        for i in 0..6000 - shift {
            assert!((late[i + shift] - dry[i]).abs() < 1e-6);
        }
        assert!(late[1..shift].iter().all(|v| v.abs() < 1e-9));
        // Mixed: the dry sound is there, on time.
        p.mix = 0.5;
        let (mixed, _) = run(&mut Convolution::new(p, SR), &x, 64);
        assert!((mixed[0] - 0.5).abs() < 0.01);
    }

    /// Short-term level (dB) in 5 ms windows.
    fn levels(x: &[f32]) -> Vec<f32> {
        x.chunks(240).map(|w| 10.0 * (w.iter().map(|v| v * v).sum::<f32>() / w.len() as f32 + 1e-12).log10()).collect()
    }

    #[test]
    fn a_new_response_crossfades_in_without_a_click() {
        for (from, to) in [(ReverbSpace::Hall, ReverbSpace::Plate), (ReverbSpace::Hall, ReverbSpace::Hall)] {
            let mut p = params(from, 1.0);
            let mut fx = Convolution::new(p, SR);
            let x = noise(3 * SR as usize, 9);
            let (mut l, mut r) = (x.clone(), x.clone());
            let at = SR as usize + 1000;
            fx.process(&mut l[..at], &mut r[..at]);
            p.space = to;
            if from == to {
                p.decay = 0.5;
            }
            fx.params = p;
            // A response for other settings is handed back unused.
            let stale = kernel(KernelKey::new(&params(ReverbSpace::Cathedral, 1.0), SR));
            assert!(fx.offer(stale).is_some());
            assert!(fx.offer(kernel(KernelKey::new(&p, SR))).is_none());
            let mut done = at;
            while done < x.len() {
                let end = (done + 128).min(x.len());
                fx.process(&mut l[done..end], &mut r[done..end]);
                done = end;
                if fx.playing() == fx.wanted() && fx.state == State::Steady && done < at + 3 * SWITCH {
                    break;
                }
            }
            assert_eq!(fx.playing(), fx.wanted());
            assert!(done - at <= 2 * SWITCH + FADE + 128, "it took {} samples", done - at);
            fx.process(&mut l[done..], &mut r[done..]);
            assert!(fx.collect().is_some(), "the old one goes back");
            assert!(!fx.busy());
            // The level glides: no 5 ms window jumps more than a few dB from the one before.
            let level = levels(&l[(0.5 * SR) as usize..]);
            let jump = level.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
            assert!(jump < 4.0, "{from:?} → {to:?}: a jump of {jump:.1} dB");
            // And no sample jumps further than the noise itself does.
            let steps = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
            let around = steps(&l[at..at + 2 * SWITCH + FADE]);
            assert!(around < 1.3 * steps(&l[(0.5 * SR) as usize..at]).max(steps(&l[at + 3 * SWITCH..])), "{from:?} → {to:?}");
        }
    }

    #[test]
    fn it_rests_in_silence_and_wakes_up_at_once() {
        let p = params(ReverbSpace::Room, 1.0);
        let mut x = vec![0.0; 3 * SR as usize];
        x[100] = 1.0;
        let back = 2 * SR as usize + 77;
        x[back] = 1.0;
        let mut fx = Convolution::new(p, SR);
        let (l, _) = run(&mut fx, &x, 128);
        assert!(fx.idle || fx.quiet < 3 * SWITCH, "it rested");
        let [h, _] = impulse_response(&p, SR);
        for i in 0..h.len().min(x.len() - back) {
            assert!((l[back + i] - h[i]).abs() < 1e-5, "sample {i} after waking");
        }
    }

    fn rms_db(x: &[f32]) -> f32 {
        10.0 * (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).log10() as f32
    }

    #[test]
    fn a_steady_sound_comes_out_about_as_loud_as_it_went_in() {
        use crate::effects::{EffectProcessor, ProcessContext};
        use crate::model::{Effect, ReverbParams};
        let ctx = ProcessContext { sample_rate: SR, tempo: 120.0, beat: 0.0, beats_per_sample: 0.0, scale: 0, meter: Default::default() };
        let x = noise(4 * SR as usize, 2);
        let tail = 2 * SR as usize..;
        let dry = rms_db(&x[tail.clone()]);
        let mut old = EffectProcessor::new(&Effect::Reverb(ReverbParams { mix: 1.0, ..Default::default() }), SR);
        let (mut l, mut r) = (x.clone(), x.clone());
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            old.process(a, b, &ctx);
        }
        let freeverb = rms_db(&l[tail.clone()]) - dry;
        for space in ReverbSpace::ALL {
            let (l, _) = run(&mut Convolution::new(ConvolutionParams { mix: 1.0, ..Default::default() }.with_space(space), SR), &x, 256);
            let level = rms_db(&l[tail.clone()]) - dry;
            println!("{:<12} {level:+.1} dB (la Reverb antigua: {freeverb:+.1} dB)", space.label());
            assert!(level.abs() < 2.5, "{space:?}: {level:+.1} dB");
        }
    }

    /// Dry test sounds, the spaces' responses and the sounds through them, for
    /// listening: PULSE_REVERB_WAVS=folder cargo test --release -p engine --lib render_reverb_wavs -- --ignored
    #[test]
    #[ignore = "writes WAVs to listen to"]
    fn render_reverb_wavs() {
        let Ok(dir) = std::env::var("PULSE_REVERB_WAVS") else { return };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, l: &[f32], r: &[f32]| {
            let mut data = Vec::with_capacity(l.len() * 4);
            for (a, b) in l.iter().zip(r) {
                for s in [a, b] {
                    data.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
                }
            }
            let mut out = Vec::new();
            out.extend_from_slice(b"RIFF");
            out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
            out.extend_from_slice(b"WAVEfmt ");
            out.extend_from_slice(&16u32.to_le_bytes());
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&2u16.to_le_bytes());
            out.extend_from_slice(&(SR as u32).to_le_bytes());
            out.extend_from_slice(&(SR as u32 * 4).to_le_bytes());
            out.extend_from_slice(&4u16.to_le_bytes());
            out.extend_from_slice(&16u16.to_le_bytes());
            out.extend_from_slice(b"data");
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&data);
            let temp = dir.join(format!(".{name}.tmp"));
            std::fs::write(&temp, out).unwrap();
            std::fs::rename(&temp, dir.join(format!("{name}.wav"))).unwrap();
        };
        let n = 7 * SR as usize;
        let mut rng = Rng::new(77);
        // A snare: a tone and a noise burst, twice a second for 3 s.
        let mut snare = vec![0.0f32; n];
        for hit in 0..6 {
            let at = hit * SR as usize / 2;
            let (mut z, mut y) = (0.0f32, 0.0f32);
            for i in 0..(0.25 * SR) as usize {
                let t = i as f32 / SR;
                z += (rng.gauss() - z) * 0.5;
                y = 0.55 * z * (-t / 0.045).exp() + 0.45 * (std::f32::consts::TAU * 185.0 * t).sin() * (-t / 0.03).exp();
                snare[at + i] += y * 0.7;
            }
            let _ = y;
        }
        // Something like a voice: a sawtooth with vibrato through two formants, in phrases.
        let mut voice = vec![0.0f32; n];
        let mut formants = [Svf::default(); 2];
        let coefs = [SvfCoefs::new(700.0, 0.25, SR), SvfCoefs::new(1150.0, 0.3, SR)];
        let mut phase = 0.0f32;
        for (i, v) in voice.iter_mut().enumerate() {
            let t = i as f32 / SR;
            let phrase = t % 1.2;
            if t > 4.8 || phrase > 0.75 {
                continue;
            }
            let note = [220.0, 246.9, 261.6, 293.7][((t / 1.2) as usize) % 4];
            phase = (phase + note * (1.0 + 0.006 * (std::f32::consts::TAU * 5.5 * t).sin()) / SR) % 1.0;
            let saw = 2.0 * phase - 1.0;
            let envelope = (phrase / 0.04).min(1.0) * ((0.75 - phrase) / 0.08).min(1.0);
            let sound = formants[0].tick(saw, &coefs[0]).1 + 0.6 * formants[1].tick(saw, &coefs[1]).1;
            *v = sound * envelope * 0.35;
        }
        // A pad: an A minor chord of detuned saws, slow in and out, then silence.
        let mut pad = vec![0.0f32; n];
        let mut phases = [0.0f32; 6];
        let mut low = Svf::default();
        let lc = SvfCoefs::new(2500.0, BUTTERWORTH_K, SR);
        for (i, v) in pad.iter_mut().enumerate() {
            let t = i as f32 / SR;
            if t > 3.0 {
                break;
            }
            let mut s = 0.0;
            for (k, f) in [220.0f32, 261.6, 329.6].into_iter().enumerate() {
                for (d, detune) in [0.997f32, 1.003].into_iter().enumerate() {
                    let ph = &mut phases[k * 2 + d];
                    *ph = (*ph + f * detune / SR) % 1.0;
                    s += 2.0 * *ph - 1.0;
                }
            }
            let envelope = (t / 0.6).min(1.0) * ((3.0 - t) / 0.8).min(1.0);
            *v = low.tick(s, &lc).0 * envelope * 0.12;
        }
        write("seco - caja", &snare, &snare);
        write("seco - voz", &voice, &voice);
        write("seco - pad", &pad, &pad);
        for space in ReverbSpace::ALL {
            let [l, r] = impulse_response(&ConvolutionParams::default().with_space(space), SR);
            let peak = l.iter().chain(&r).fold(0f32, |m, v| m.max(v.abs()));
            let scale = 0.9 / peak;
            let (l, r): (Vec<f32>, Vec<f32>) = l.iter().zip(&r).map(|(a, b)| (a * scale, b * scale)).unzip();
            write(&format!("respuesta - {}", space.label()), &l, &r);
        }
        use ReverbSpace::*;
        let through = |dry: &[f32], name: &str, spaces: &[ReverbSpace], mix: f32| {
            for &space in spaces {
                let p = ConvolutionParams { space, mix, predelay_ms: 10.0, low_cut_hz: 120.0, ..Default::default() };
                let (l, r) = run(&mut Convolution::new(p, SR), dry, 128);
                write(&format!("{name} - {}", space.label()), &l, &r);
            }
        };
        through(&snare, "caja", &[Ambience, Room, Studio, Plate, Hall], 0.35);
        through(&voice, "voz", &[Plate, Hall, Spring, BigHall], 0.3);
        through(&pad, "pad", &[Cathedral, BigHall, Infinite], 0.45);
        // The old Reverb, to compare.
        use crate::effects::{EffectProcessor, ProcessContext};
        let ctx = ProcessContext { sample_rate: SR, tempo: 120.0, beat: 0.0, beats_per_sample: 0.0, scale: 0, meter: Default::default() };
        for (dry, name, mix) in [(&snare, "caja", 0.35), (&voice, "voz", 0.3), (&pad, "pad", 0.45)] {
            let mut old = EffectProcessor::new(&crate::model::Effect::Reverb(crate::model::ReverbParams { mix, ..Default::default() }), SR);
            let (mut l, mut r) = (dry.clone(), dry.clone());
            for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
                old.process(a, b, &ctx);
            }
            write(&format!("{name} - Reverb antigua (Freeverb)"), &l, &r);
        }
    }

    #[test]
    #[ignore = "benchmark: cargo test --release -p engine --lib bench_convolution -- --ignored --nocapture"]
    fn bench_convolution() {
        for (space, decay) in [(ReverbSpace::Hall, 0.8), (ReverbSpace::Cathedral, 0.6), (ReverbSpace::Cathedral, 1.25), (ReverbSpace::Infinite, 2.0)] {
            let p = ConvolutionParams { mix: 1.0, ..params(space, decay) };
            let started = std::time::Instant::now();
            let mut fx = Convolution::new(p, SR);
            let build = started.elapsed();
            let seconds = fx.kernel.len as f32 / SR;
            let x = noise(10 * SR as usize, 1);
            let (mut l, mut r) = (x.clone(), x.clone());
            let mut worst = std::time::Duration::ZERO;
            let started = std::time::Instant::now();
            for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
                let t = std::time::Instant::now();
                fx.process(a, b);
                worst = worst.max(t.elapsed());
            }
            let blocks = x.len() / 128;
            let mean = started.elapsed().as_secs_f64() * 1e6 / blocks as f64;
            println!("{} {decay}× · respuesta de {seconds:.1} s (hecha en {:.0} ms): {mean:.1} µs por bloque de 128 (peor {:.0} µs) · {:.2} % de un núcleo", space.label(), build.as_secs_f64() * 1e3, worst.as_secs_f64() * 1e6, mean / (128.0 / SR as f64 * 1e6) * 100.0);
        }
    }

    #[test]
    fn responses_are_the_same_every_time() {
        let p = params(ReverbSpace::Cathedral, 0.5);
        assert!(impulse_response(&p, SR) == impulse_response(&p, SR));
        assert!(impulse_response(&p, SR) != impulse_response(&params(ReverbSpace::Cathedral, 0.6), SR));
    }

    // ----- Measures of a response ---------------------------------------------------------

    /// An octave band of `x` around `f` (two band-pass biquads).
    fn octave(x: &[f32], f: f32) -> Vec<f32> {
        let w = std::f32::consts::TAU * f / SR;
        let alpha = w.sin() / (2.0 * std::f32::consts::SQRT_2);
        let (b0, a0, a1, a2) = (alpha, 1.0 + alpha, -2.0 * w.cos(), 1.0 - alpha);
        let mut y = x.to_vec();
        for _ in 0..2 {
            let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for v in y.iter_mut() {
                let x0 = *v;
                let out = (b0 * x0 - b0 * x2 - a1 * y1 - a2 * y2) / a0;
                x2 = x1;
                x1 = x0;
                y2 = y1;
                y1 = out;
                *v = out;
            }
        }
        y
    }

    /// Decay time from the Schroeder curve's -5 to -25 dB (T20).
    fn t60(x: &[f32]) -> f32 {
        let mut edc = vec![0.0f64; x.len() + 1];
        for i in (0..x.len()).rev() {
            edc[i] = edc[i + 1] + (x[i] as f64).powi(2);
        }
        let db = |i: usize| 10.0 * (edc[i] / edc[0]).log10();
        let a = (0..x.len()).find(|&i| db(i) <= -5.0).unwrap_or(0);
        let b = (0..x.len()).find(|&i| db(i) <= -25.0).unwrap_or(x.len());
        (b - a) as f32 / SR * 3.0
    }

    /// Normalized echo density (Abel & Huang) in 20 ms windows: 1 is Gaussian noise.
    fn echo_density(x: &[f32], at: usize) -> f32 {
        let half = 480;
        let w = &x[at.saturating_sub(half)..(at + half).min(x.len())];
        let sigma = (w.iter().map(|v| v * v).sum::<f32>() / w.len() as f32).sqrt();
        w.iter().filter(|v| v.abs() > sigma).count() as f32 / w.len() as f32 / 0.3173
    }

    fn correlation(l: &[f32], r: &[f32]) -> f32 {
        let (lr, ll, rr) = l.iter().zip(r).fold((0.0f64, 0.0f64, 0.0f64), |(a, b, c), (&x, &y)| (a + (x * y) as f64, b + (x * x) as f64, c + (y * y) as f64));
        (lr / (ll * rr).sqrt()) as f32
    }

    /// How far the tail's spectrum (each frame at the same level, averaged)
    /// stands above its own surroundings (±1/6 octave), at most, 150 Hz-10 kHz:
    /// a resonance, a metallic ring.
    fn ringing(x: &[f32]) -> f32 {
        const N: usize = 4096;
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(N);
        let mut power = vec![0.0f64; N / 2];
        let mut start = 0;
        while start + N <= x.len() {
            let mut frame: Vec<C32> = (0..N).map(|i| C32::new(x[start + i] * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos()), 0.0)).collect();
            fft.process(&mut frame);
            let e: f64 = frame[..N / 2].iter().map(|c| c.norm_sqr() as f64).sum::<f64>().max(1e-30);
            for (p, c) in power.iter_mut().zip(&frame) {
                *p += c.norm_sqr() as f64 / e;
            }
            start += N / 2;
        }
        let bin = |f: f32| (f / SR * N as f32) as usize;
        let mut worst = 0f32;
        for k in bin(150.0)..bin(10_000.0) {
            let (lo, hi) = ((k as f32 / 1.122) as usize, (k as f32 * 1.122) as usize + 1);
            let mut around: Vec<f64> = power[lo..hi].to_vec();
            around.sort_by(|a, b| a.total_cmp(b));
            let median = around[around.len() / 2].max(1e-30);
            worst = worst.max(10.0 * (power[k] / median).log10() as f32);
        }
        worst
    }

    struct Measures {
        t60: [f32; 8],
        dense_ms: f32,
        density: f32,
        correlation: f32,
        ringing: f32,
    }

    fn measure(space: ReverbSpace, decay: f32) -> Measures {
        let [l, r] = impulse_response(&params(space, decay), SR);
        let mono: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a + b).collect();
        let t60 = BANDS.map(|f| if f < SR * 0.4 { t60(&octave(&l, f)) * 0.5 + t60(&octave(&r, f)) * 0.5 } else { 0.0 });
        let s = spec(space);
        let start = ((s.early_ms.max(40.0) + 10.0) * 0.001 * SR) as usize;
        let end = (l.len() * 6 / 10).max(start + 8192).min(l.len());
        let dense = (480..l.len() / 2).step_by(240).find(|&i| echo_density(&mono, i) > 0.9).unwrap_or(l.len());
        let density = (start..end).step_by(960).map(|i| echo_density(&l, i)).sum::<f32>() / ((end - start) / 960).max(1) as f32;
        Measures { t60, dense_ms: dense as f32 / SR * 1000.0, density, correlation: correlation(&l[start..end], &r[start..end]), ringing: ringing(&l[start..end]) }
    }

    #[test]
    fn spaces_decay_as_designed_dense_wide_and_without_ringing() {
        for space in ReverbSpace::ALL {
            let m = measure(space, 1.0);
            let s = spec(space);
            println!(
                "{:<12} T60 {:?} · densa a {:.0} ms · densidad {:.2} · correlación {:+.2} · pico {:.1} dB",
                space.label(),
                m.t60.map(|t| (t * 100.0).round() / 100.0),
                m.dense_ms,
                m.density,
                m.correlation,
                m.ringing
            );
            // Decay times where the tail rules (the shortest spaces are mostly early reflections).
            if !matches!(space, ReverbSpace::Ambience | ReverbSpace::Spring) {
                for (band, (&got, &want)) in m.t60.iter().zip(&s.t60).enumerate().take(7) {
                    assert!((got / want - 1.0).abs() < 0.25, "{space:?} band {}: T60 {got:.2} s, designed {want:.2}", BANDS[band]);
                }
                // Highs die first.
                assert!(m.t60[6] < m.t60[3], "{space:?}");
            }
            assert!(m.correlation.abs() < 0.3, "{space:?}: correlation {}", m.correlation);
            if space != ReverbSpace::Spring {
                assert!(m.density > 0.85, "{space:?}: echo density {}", m.density);
                assert!(m.ringing < 9.0, "{space:?}: a peak {} dB over its surroundings", m.ringing);
            }
        }
    }

    #[test]
    fn decay_scales_the_decay_times() {
        let short = measure(ReverbSpace::Hall, 0.5);
        let long = measure(ReverbSpace::Hall, 1.5);
        for band in 1..6 {
            let ratio = long.t60[band] / short.t60[band];
            assert!((ratio / 3.0 - 1.0).abs() < 0.2, "band {}: ×{ratio:.2}", BANDS[band]);
        }
    }

    #[test]
    fn a_spring_drips_highs_first() {
        // In each echo the highs arrive before the lows (the chirp falls).
        let [l, _] = impulse_response(&params(ReverbSpace::Spring, 1.0), SR);
        let centre = |x: &[f32]| {
            let w = &x[(0.025 * SR) as usize..(0.06 * SR) as usize];
            let e: f32 = w.iter().map(|v| v * v).sum();
            w.iter().enumerate().map(|(i, v)| i as f32 * v * v).sum::<f32>() / e
        };
        let (high, low) = (centre(&octave(&l, 3000.0)), centre(&octave(&l, 250.0)));
        assert!(high + 0.002 * SR < low, "highs at {high}, lows at {low}");
    }
}
