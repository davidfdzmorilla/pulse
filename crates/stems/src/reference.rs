//! The analyses as they were before they were made faster, kept to check
//! that the fast ones give the same results.

use crate::pitch::Track;
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

const RATE: f32 = 22050.0;
const FRAME: usize = 2048;
const HOP: usize = 256;
const FMIN: f64 = 55.0;
const FMAX: f64 = 1000.0;
const THRESHOLDS: usize = 100;
const BINS_PER_SEMITONE: usize = 10;
const BOLTZMANN: f64 = 2.0;
const NO_TROUGH_PROB: f64 = 0.01;
const SWITCH_PROB: f64 = 0.01;
const MAX_TRANSITION_RATE: f64 = 35.92;
const SHORT: usize = 512;

fn beta_cdf(x: f64) -> f64 {
    1.0 - (1.0 - x).powi(19) - 19.0 * x * (1.0 - x).powi(18)
}

/// The resampling as it was, one output sample at a time with every weight
/// worked out again.
pub fn resample(x: &[f32], from: f32, to: f32) -> Vec<f32> {
    const HALF: isize = 32;
    let ratio = from as f64 / to as f64;
    let cutoff = (1.0 / ratio).min(1.0) * 0.97;
    let length = ((x.len() as f64) / ratio).round() as usize;
    let reach = (HALF as f64 / cutoff).ceil() as isize;
    (0..length)
        .map(|i| {
            let center = i as f64 * ratio;
            let base = center.floor() as isize;
            let mut sum = 0.0f64;
            for k in base - reach + 1..=base + reach {
                let Some(&sample) = usize::try_from(k).ok().and_then(|k| x.get(k)) else { continue };
                let t = (k as f64 - center) * cutoff;
                let w = t / HALF as f64;
                if w.abs() >= 1.0 {
                    continue;
                }
                let sinc = if t.abs() < 1e-9 { 1.0 } else { (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t) };
                let blackman = 0.42 + 0.5 * (std::f64::consts::PI * w).cos() + 0.08 * (2.0 * std::f64::consts::PI * w).cos();
                sum += sample as f64 * sinc * blackman * cutoff;
            }
            sum as f32
        })
        .collect()
}

/// Cumulative mean normalized difference of a frame, for periods min..=max.
pub(crate) fn yin_frame(frame: &[f32], acf: &[f64], min_period: usize, max_period: usize) -> Vec<f64> {
    // Running energy of the frame's start, as librosa subtracts it.
    let mut cumulative = vec![0f64; max_period + 1];
    let mut sum = 0.0;
    for k in 0..=max_period {
        sum += (frame[k] as f64).powi(2);
        cumulative[k] = sum;
    }
    let mut raw = vec![0f64; max_period + 1];
    for k in 1..=max_period {
        raw[k] = 2.0 * (acf[0] - acf[k]) - cumulative[k - 1];
    }
    let mut mean = 0.0;
    let mut out = Vec::with_capacity(max_period - min_period + 1);
    for k in 1..=max_period {
        mean += raw[k];
        if k >= min_period {
            out.push(raw[k] / (mean / k as f64 + f64::MIN_POSITIVE));
        }
    }
    out
}

/// The pitch track of `mono` at `rate` Hz.
pub fn track(mono: &[f32], rate: f32) -> Track {
    let audio = resample(mono, rate, RATE);
    let sr = RATE as f64;
    // Centred frames: half a frame of silence each side.
    let mut padded = vec![0f32; FRAME / 2];
    padded.extend_from_slice(&audio);
    padded.extend(std::iter::repeat_n(0.0, FRAME / 2));
    let frames = if padded.len() >= FRAME { 1 + (padded.len() - FRAME) / HOP } else { 0 };
    let min_period = (sr / FMAX).floor() as usize;
    let max_period = ((sr / FMIN).ceil() as usize).min(FRAME - 1);
    let bins = (12.0 * BINS_PER_SEMITONE as f64 * (FMAX / FMIN).log2()).floor() as usize + 1;

    let thresholds: Vec<f64> = (0..=THRESHOLDS).map(|i| i as f64 / THRESHOLDS as f64).collect();
    let beta: Vec<f64> = thresholds.windows(2).map(|w| beta_cdf(w[1]) - beta_cdf(w[0])).collect();

    // Autocorrelation by FFT (twice the frame, so it doesn't wrap).
    let size = FRAME * 2;
    let mut planner = FftPlanner::<f32>::new();
    let (forward, inverse) = (planner.plan_fft_forward(size), planner.plan_fft_inverse(size));

    let mut observations: Vec<Vec<f64>> = Vec::with_capacity(frames);
    let mut rms = Vec::with_capacity(frames);
    let mut short = Vec::with_capacity(frames);
    for f in 0..frames {
        let frame = &padded[f * HOP..f * HOP + FRAME];
        rms.push((frame.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt());
        let middle = &frame[FRAME / 2 - SHORT / 2..FRAME / 2 + SHORT / 2];
        short.push((middle.iter().map(|x| x * x).sum::<f32>() / SHORT as f32).sqrt());
        let mut spectrum: Vec<Complex32> = frame.iter().map(|&x| Complex32::new(x, 0.0)).chain(std::iter::repeat_n(Complex32::new(0.0, 0.0), size - FRAME)).collect();
        forward.process(&mut spectrum);
        for c in &mut spectrum {
            *c = Complex32::new(c.norm_sqr(), 0.0);
        }
        inverse.process(&mut spectrum);
        let acf: Vec<f64> = spectrum[..=max_period].iter().map(|c| c.re as f64 / size as f64).collect();
        let yin = yin_frame(frame, &acf, min_period, max_period);
        observations.push(observe(&yin, &thresholds, &beta, min_period, bins));
    }
    let states = viterbi(&observations, bins);
    let f0 = states.into_iter().map(|s| (s < bins).then(|| FMIN * 2f64.powf(s as f64 / (12 * BINS_PER_SEMITONE) as f64))).collect();
    Track { f0, rms, short }
}

/// The probability of each pitch bin (voiced) and of no pitch (unvoiced, spread
/// over the second half) for one frame (`__pyin_helper`).
pub(crate) fn observe(yin: &[f64], thresholds: &[f64], beta: &[f64], min_period: usize, bins: usize) -> Vec<f64> {
    let n = yin.len();
    let mut obs = vec![0f64; 2 * bins];
    let troughs: Vec<usize> = (0..n)
        .filter(|&t| match t {
            0 => n > 1 && yin[0] < yin[1],
            t if t == n - 1 => yin[t] < yin[t - 1],
            t => yin[t] < yin[t - 1] && yin[t] <= yin[t + 1],
        })
        .collect();
    if !troughs.is_empty() {
        let mut probs = vec![0f64; troughs.len()];
        for (k, &b) in beta.iter().enumerate() {
            let threshold = thresholds[k + 1];
            let below: Vec<usize> = (0..troughs.len()).filter(|&i| yin[troughs[i]] < threshold).collect();
            let count = below.len() as f64;
            for (position, &i) in below.iter().enumerate() {
                // Boltzmann prior: earlier troughs (shorter periods) first.
                let prior = (1.0 - (-BOLTZMANN).exp()) * (-BOLTZMANN * position as f64).exp() / (1.0 - (-BOLTZMANN * count).exp());
                probs[i] += prior * b;
            }
        }
        let global = (0..troughs.len()).min_by(|&a, &b| yin[troughs[a]].total_cmp(&yin[troughs[b]])).unwrap_or(0);
        let not_below = thresholds[1..].iter().filter(|&&t| !(yin[troughs[global]] < t)).count();
        probs[global] += NO_TROUGH_PROB * beta[..not_below].iter().sum::<f64>();
        for (i, &t) in troughs.iter().enumerate() {
            if probs[i] == 0.0 {
                continue;
            }
            // Parabolic interpolation of the trough's period.
            let shift = if t == 0 || t == n - 1 {
                0.0
            } else {
                let a = yin[t + 1] + yin[t - 1] - 2.0 * yin[t];
                let b = (yin[t + 1] - yin[t - 1]) / 2.0;
                if b.abs() >= a.abs() { 0.0 } else { -b / a }
            };
            let f0 = RATE as f64 / (min_period as f64 + t as f64 + shift);
            let bin = (12.0 * BINS_PER_SEMITONE as f64 * (f0 / FMIN).log2()).round().clamp(0.0, bins as f64) as usize;
            obs[bin] = probs[i];
        }
    }
    let voiced = obs[..bins].iter().sum::<f64>().clamp(0.0, 1.0);
    for o in &mut obs[bins..] {
        *o = (1.0 - voiced) / bins as f64;
    }
    obs
}

/// The likeliest path through the states (voiced bins, then unvoiced bins): the
/// pitch moves at most a few semitones a frame, voicing switches rarely.
pub(crate) fn viterbi(observations: &[Vec<f64>], bins: usize) -> Vec<usize> {
    let states = 2 * bins;
    if observations.is_empty() {
        return Vec::new();
    }
    let semitones = (MAX_TRANSITION_RATE * 12.0 * HOP as f64 / RATE as f64).round() as usize;
    let width = semitones * BINS_PER_SEMITONE + 1;
    let half = (width / 2) as isize;
    // Triangle weights over the reachable bins, each row summing to one.
    let weight = |d: isize| 1.0 - d.unsigned_abs() as f64 / (half + 1) as f64;
    let row_sum: Vec<f64> = (0..bins as isize).map(|i| (-half..=half).filter(|d| (0..bins as isize).contains(&(i + d))).map(weight).sum()).collect();
    let tiny = f64::MIN_POSITIVE;
    let ln = |x: f64| (x + tiny).ln();
    let mut score: Vec<f64> = observations[0].iter().map(|&o| ln(1.0 / states as f64) + ln(o)).collect();
    let mut back: Vec<Vec<u32>> = Vec::with_capacity(observations.len());
    for obs in &observations[1..] {
        let mut next = vec![f64::NEG_INFINITY; states];
        let mut from = vec![0u32; states];
        for to in 0..states {
            let (block, bin) = (to / bins, (to % bins) as isize);
            for source_block in 0..2 {
                let switch = if source_block == block { 1.0 - SWITCH_PROB } else { SWITCH_PROB };
                for d in -half..=half {
                    let i = bin + d;
                    if !(0..bins as isize).contains(&i) {
                        continue;
                    }
                    // From bin i to this bin: row i's weight at distance -d.
                    let p = switch * weight(d) / row_sum[i as usize];
                    let s = source_block * bins + i as usize;
                    let candidate = score[s] + ln(p);
                    if candidate > next[to] {
                        next[to] = candidate;
                        from[to] = s as u32;
                    }
                }
            }
            next[to] += ln(obs[to]);
        }
        score = next;
        back.push(from);
    }
    let mut state = (0..states).max_by(|&a, &b| score[a].total_cmp(&score[b])).unwrap_or(0);
    let mut path = vec![state; observations.len()];
    for (t, from) in back.iter().enumerate().rev() {
        state = from[state] as usize;
        path[t] = state;
    }
    path
}

/// `Chromagram::new`'s frames, one after another on one thread.
pub fn chroma_frames(mono: &[f32], rate: f32) -> Vec<([f64; 12], Option<(u8, f64)>)> {
    use rustfft::num_complex::Complex32;
    const SIZE: usize = 8192;
    const HOP: usize = 2048;
    let fft = rustfft::FftPlanner::<f32>::new().plan_fft_forward(SIZE);
    let window: Vec<f32> = (0..SIZE).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / SIZE as f32).cos()).collect();
    // The pitch class of each bin, for the whole range and for the bass.
    let class_in = |low: f64, high: f64| -> Vec<Option<usize>> {
        (0..SIZE / 2)
            .map(|b| {
                let f = b as f64 * rate as f64 / SIZE as f64;
                (low..high).contains(&f).then(|| ((69.0 + 12.0 * (f / 440.0).log2()).round() as i64).rem_euclid(12) as usize)
            })
            .collect()
    };
    let all = class_in(100.0, 2000.0);
    let bin_hz = rate as f64 / SIZE as f64;
    let (low_bin, high_bin) = ((38.0 / bin_hz) as usize, (220.0 / bin_hz) as usize);
    let mut frames = Vec::new();
    let mut start = 0;
    let mut buffer = vec![Complex32::new(0.0, 0.0); SIZE];
    while start < mono.len() {
        for (i, b) in buffer.iter_mut().enumerate() {
            *b = Complex32::new(mono.get(start + i).copied().unwrap_or(0.0) * window[i], 0.0);
        }
        fft.process(&mut buffer);
        let mut a = [0f64; 12];
        for b in 0..SIZE / 2 {
            if let Some(c) = all[b] {
                a[c] += buffer[b].norm() as f64;
            }
        }
        let m = |b: usize| buffer[b].norm() as f64;
        let loudest = (low_bin..=high_bin).map(m).fold(0.0, f64::max);
        let bass = (low_bin.max(1)..high_bin).find(|&b| m(b) >= 0.5 * loudest && m(b) >= m(b - 1) && m(b) >= m(b + 1)).filter(|_| loudest > 0.0).map(|b| {
            let f = b as f64 * bin_hz;
            ((((69.0 + 12.0 * (f / 440.0).log2()).round() as i64).rem_euclid(12)) as u8, m(b))
        });
        frames.push((a, bass));
        start += HOP;
    }
    // Each class's median over about 0.4 s: what holds (a chord) stays, what
    // only hits (a kick, a snare) goes.
    const SPAN: usize = 4;
    let held: Vec<[f64; 12]> = (0..frames.len())
        .map(|i| {
            let around = &frames[i.saturating_sub(SPAN)..(i + SPAN + 1).min(frames.len())];
            std::array::from_fn(|c| {
                let mut values: Vec<f64> = around.iter().map(|f| f.0[c]).collect();
                values.sort_by(|a, b| a.total_cmp(b));
                values[values.len() / 2]
            })
        })
        .collect();
    for (frame, h) in frames.iter_mut().zip(held) {
        frame.0 = h;
    }
    frames
}
