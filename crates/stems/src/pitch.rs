//! The pitch of one voice, and its notes: pYIN (Mauch & Dixon, 2014), as
//! `librosa.pyin` computes it, then the voiced stretches cut into notes where the
//! pitch moves to another semitone or the voice starts again.
//!
//! Made for singing and humming (what Basic Pitch, built for instruments and
//! chords, gets wrong: low voices, vowels' formants taken for notes).

use crate::key::Key;
use crate::resample;
use crate::transcribe::NoteEvent;
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

const RATE: f32 = 22050.0;
const FRAME: usize = 2048;
const HOP: usize = 256;
/// Lowest and highest pitch looked for: A1 (a deep voice) to about B5.
const FMIN: f64 = 55.0;
const FMAX: f64 = 1000.0;
const THRESHOLDS: usize = 100;
const BINS_PER_SEMITONE: usize = 10;
const BOLTZMANN: f64 = 2.0;
const NO_TROUGH_PROB: f64 = 0.01;
const SWITCH_PROB: f64 = 0.01;
const MAX_TRANSITION_RATE: f64 = 35.92;
/// Samples of the short loudness window.
const SHORT: usize = 512;

/// The pitch track: per frame (every [`HOP`] samples at 22050 Hz, centred), the
/// frequency in Hz if voiced.
pub struct Track {
    pub f0: Vec<Option<f64>>,
    /// Loudness per frame (RMS of the frame).
    pub rms: Vec<f32>,
    /// Loudness over a short window (23 ms) around each frame: dips between
    /// notes sung on one breath show there.
    pub short: Vec<f32>,
}

/// Seconds of frame `i`.
pub fn frame_time(i: usize) -> f64 {
    (i * HOP) as f64 / RATE as f64
}

/// Beta(2, 18)'s CDF at `x` (integer parameters: a binomial tail).
fn beta_cdf(x: f64) -> f64 {
    1.0 - (1.0 - x).powi(19) - 19.0 * x * (1.0 - x).powi(18)
}

/// Cumulative mean normalized difference of a frame, for periods min..=max.
fn yin_frame(frame: &[f32], acf: &[f64], min_period: usize, max_period: usize) -> Vec<f64> {
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
fn observe(yin: &[f64], thresholds: &[f64], beta: &[f64], min_period: usize, bins: usize) -> Vec<f64> {
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
fn viterbi(observations: &[Vec<f64>], bins: usize) -> Vec<usize> {
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

/// Elsewhere for this many frames (93 ms) starts another note.
const MOVE_FRAMES: usize = 8;
/// How far (semitones) the voice may stray from its note and still be on it.
const STRAY: f64 = 0.75;
/// Notes shorter than this (seconds) are left out: slides and blips.
const MIN_NOTE: f64 = 0.08;
/// Within this (semitones) of a semitone, a note is in tune: it stays there.
const IN_TUNE: f64 = 0.3;

fn midi(f0: f64) -> f64 {
    69.0 + 12.0 * (f0 / 440.0).log2()
}

/// How much each pitch class sounds in the track (voiced frames, by loudness).
pub fn weights(track: &Track) -> [f64; 12] {
    let mut w = [0f64; 12];
    for (f0, level) in track.f0.iter().zip(&track.short) {
        if let Some(f0) = f0 {
            w[(midi(*f0).round() as i64).rem_euclid(12) as usize] += *level as f64;
        }
    }
    w
}

/// The notes of a pitch track: each voiced stretch, cut where the voice settles
/// on another note (for [`MOVE_FRAMES`]) or comes in again after a dip. With a
/// `key`, the voice is heard as the scale note it is nearest to, so singing a bit
/// sharp or flat doesn't jump to the next semitone; without, the nearest semitone.
pub fn notes(track: &Track, key: Option<Key>) -> Vec<NoteEvent> {
    let n = track.f0.len();
    let peak = track.rms.iter().copied().fold(0f32, f32::max).max(1e-9);
    // In tune (close to a semitone): that semitone, in the scale or not. Between
    // two: the key's scale decides.
    let nearest = |m: f64| -> u8 {
        let semitone = m.round().clamp(0.0, 127.0);
        match key {
            Some(k) if (m - semitone).abs() > IN_TUNE => k.snap(m),
            _ => semitone as u8,
        }
    };
    // The note the voice is on at a frame, given the one it was on.
    let heard = |m: f64, current: u8| -> u8 { if (m - current as f64).abs() <= STRAY { current } else { nearest(m) } };
    let mut notes = Vec::new();
    let mut close = |from: usize, to: usize, pitch: u8| {
        if to <= from || frame_time(to) - frame_time(from) < MIN_NOTE {
            return;
        }
        let level = track.rms[from..to].iter().copied().fold(0f32, f32::max) / peak;
        notes.push(NoteEvent { start: frame_time(from), end: frame_time(to), pitch, amplitude: level.sqrt() });
    };
    let level = &track.short;
    let mut i = 0;
    while i < n {
        if track.f0[i].is_none() {
            i += 1;
            continue;
        }
        let mut end = i;
        while end < n && track.f0[end].is_some() {
            end += 1;
        }
        let m: Vec<f64> = (i..end).map(|t| midi(track.f0[t].unwrap_or(FMIN))).collect();
        // A note starts on what the next few frames mostly are.
        let settle = |from: usize| -> u8 {
            let mut votes = [0usize; 128];
            for &x in &m[from - i..(from + MOVE_FRAMES).min(end) - i] {
                votes[nearest(x) as usize] += 1;
            }
            (0..128).max_by_key(|&p| votes[p]).unwrap_or(0) as u8
        };
        let (mut start, mut current) = (i, settle(i));
        let mut away = 0;
        let mut t = i;
        while t < end {
            let here = heard(m[t - i], current);
            away = if here != current { away + 1 } else { 0 };
            let before = level[start..t].iter().copied().fold(0f32, f32::max);
            let after = level[(t + 1).min(end)..(t + 8).min(end)].iter().copied().fold(0f32, f32::max);
            let again = t > start + 3 && t + 1 < end && level[t] <= level[t - 1] && level[t] <= level[t + 1] && level[t] < 0.35 * before && after > 2.5 * level[t];
            if away >= MOVE_FRAMES {
                // On another note for a while: it began where it first went there.
                let from = t + 1 - away;
                close(start, from, current);
                start = from;
                current = settle(from);
                away = 0;
            } else if again {
                close(start, t, current);
                start = t + 1;
                current = settle(start.min(end - 1));
                away = 0;
            }
            t += 1;
        }
        close(start, end, current);
        i = end;
    }
    notes
}

/// The notes sung or played in `mono` (one at a time), and the key they were
/// heard in: the voice's own, or with `context` (how much each pitch class
/// sounds in the rest of the song) added. `snap`: to that key's scale.
pub fn melody(mono: &[f32], rate: f32, context: [f64; 12], snap: bool) -> (Vec<NoteEvent>, Option<Key>) {
    let track = track(mono, rate);
    // The voice's own notes, each by how long and loud it is.
    let mut voice = [0f64; 12];
    for n in notes(&track, None) {
        voice[n.pitch as usize % 12] += (n.end - n.start) * n.amplitude as f64;
    }
    let total: f64 = voice.iter().sum::<f64>().max(1e-9);
    let context_total: f64 = context.iter().sum::<f64>();
    // The song and the voice count the same.
    let combined: [f64; 12] = std::array::from_fn(|i| voice[i] / total + if context_total > 0.0 { context[i] / context_total } else { 0.0 });
    let key = Key::detect(&combined);
    (notes(&track, key.filter(|_| snap)), key)
}

/// The pitch of a short stretch of a voice, as it comes in (YIN, without the
/// look back and ahead of [`track`]): its frequency and how unsure it is (the
/// aperiodicity, 0 a perfect tone), or None if it has none between `fmin` and
/// `fmax` Hz. `frame` needs at least two periods of `fmin` and `fmax`'s lag.
/// (One call; [`LiveTracker`] for one after another.)
pub fn live(frame: &[f32], rate: f32, fmin: f64, fmax: f64) -> Option<(f64, f64)> {
    LiveTracker::new(frame.len(), rate, fmin, fmax).pitch(frame)
}

/// [`live`] for one stretch after another of the same length, as the voice
/// comes in. It listens to the newest part of each (each sample of it compared
/// with the one a period before), so a new note shows as soon as it is there;
/// with FFTs and its buffers kept, it can run every few milliseconds.
pub struct LiveTracker {
    rate: f32,
    len: usize,
    lag_min: usize,
    lag_max: usize,
    forward: std::sync::Arc<dyn rustfft::Fft<f64>>,
    inverse: std::sync::Arc<dyn rustfft::Fft<f64>>,
    a: Vec<Complex64>,
    b: Vec<Complex64>,
    scratch: Vec<Complex64>,
    /// Running sums of the squares: `squares[i]` is that of the first `i` samples.
    squares: Vec<f64>,
    d: Vec<f64>,
    cmnd: Vec<f64>,
}

impl LiveTracker {
    /// For stretches of `len` samples at `rate`, pitches between `fmin` and `fmax` Hz.
    pub fn new(len: usize, rate: f32, fmin: f64, fmax: f64) -> Self {
        let (lag_min, lag_max) = ((rate as f64 / fmax).floor() as usize, (rate as f64 / fmin).ceil() as usize);
        // The correlation needs no wrap: what is compared fits in `len`.
        let size = len.max(2).next_power_of_two();
        let mut planner = rustfft::FftPlanner::<f64>::new();
        let (forward, inverse) = (planner.plan_fft_forward(size), planner.plan_fft_inverse(size));
        let scratch = vec![Complex64::default(); forward.get_inplace_scratch_len().max(inverse.get_inplace_scratch_len())];
        Self {
            rate,
            len,
            lag_min,
            lag_max,
            forward,
            inverse,
            a: vec![Complex64::default(); size],
            b: vec![Complex64::default(); size],
            scratch,
            squares: vec![0.0; len + 1],
            d: vec![0.0; lag_max + 2],
            cmnd: vec![1.0; lag_max + 2],
        }
    }

    /// The samples it takes each time.
    pub fn len(&self) -> usize {
        self.len
    }

    /// The pitch of `frame` (`len` samples, oldest first), as [`live`].
    pub fn pitch(&mut self, frame: &[f32]) -> Option<(f64, f64)> {
        let (lag_min, lag_max) = (self.lag_min, self.lag_max);
        if lag_min < 2 || frame.len() != self.len || frame.len() < 2 * lag_max {
            return None;
        }
        // The newest `width` samples, each compared with the one `tau` before:
        // d(tau) = Σ x[j]² + Σ x[j - tau]² - 2 Σ x[j]·x[j - tau].
        let width = self.len - lag_max;
        for (i, slot) in self.a.iter_mut().enumerate() {
            *slot = Complex64::new(frame.get(i).map_or(0.0, |&x| x as f64), 0.0);
        }
        for (i, slot) in self.b.iter_mut().enumerate() {
            *slot = Complex64::new(if i < width { frame[lag_max + i] as f64 } else { 0.0 }, 0.0);
        }
        self.forward.process_with_scratch(&mut self.a, &mut self.scratch);
        self.forward.process_with_scratch(&mut self.b, &mut self.scratch);
        for (a, b) in self.a.iter_mut().zip(&self.b) {
            *a *= b.conj();
        }
        self.inverse.process_with_scratch(&mut self.a, &mut self.scratch);
        // a[m] / size: Σ frame[k + m]·frame[lag_max + k], over the window.
        let scale = 1.0 / self.a.len() as f64;
        for (i, &x) in frame.iter().enumerate() {
            self.squares[i + 1] = self.squares[i] + (x as f64) * (x as f64);
        }
        let energy = |from: usize| self.squares[from + width] - self.squares[from];
        let newest = energy(lag_max);
        self.d[0] = 0.0;
        for tau in 1..=lag_max {
            let cross = self.a[lag_max - tau].re * scale;
            self.d[tau] = (newest + energy(lag_max - tau) - 2.0 * cross).max(0.0);
        }
        self.d[lag_max + 1] = 0.0;
        yin_pick(&self.d, &mut self.cmnd, lag_min, lag_max, self.rate)
    }
}

type Complex64 = rustfft::num_complex::Complex<f64>;

/// YIN's last steps on a difference function `d` (from lag 0 to `lag_max + 1`):
/// the first dip of its cumulative mean normalized form under the threshold,
/// between lags. `cmnd` is room for that form, as long as `d`.
fn yin_pick(d: &[f64], cmnd: &mut [f64], lag_min: usize, lag_max: usize, rate: f32) -> Option<(f64, f64)> {
    const THRESHOLD: f64 = 0.15;
    cmnd[0] = 1.0;
    let mut running = 0.0;
    for tau in 1..d.len() {
        running += d[tau];
        cmnd[tau] = if running > 0.0 { d[tau] * tau as f64 / running } else { 1.0 };
    }
    let mut tau = lag_min;
    while tau <= lag_max {
        if cmnd[tau] < THRESHOLD {
            while tau + 1 <= lag_max && cmnd[tau + 1] < cmnd[tau] {
                tau += 1;
            }
            // Between the lags around the dip.
            let (a, b, c) = (cmnd[tau - 1], cmnd[tau], cmnd[(tau + 1).min(d.len() - 1)]);
            let shift = if a + c - 2.0 * b != 0.0 { 0.5 * (a - c) / (a + c - 2.0 * b) } else { 0.0 };
            let period = tau as f64 + shift.clamp(-1.0, 1.0);
            return Some((rate as f64 / period, b));
        }
        tau += 1;
    }
    None
}

/// A melody's notes in a key already known (the project's), each note that is
/// not in tune going to its scale.
pub fn melody_in(mono: &[f32], rate: f32, key: Key) -> Vec<NoteEvent> {
    notes(&track(mono, rate), Some(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: f32, parts: &[(f64, f64, f64)]) -> Vec<f32> {
        // (start s, length s, MIDI pitch), with a few harmonics, like a voice.
        let total = parts.iter().map(|p| p.0 + p.1).fold(0.0, f64::max) + 0.3;
        let mut out = vec![0f32; (total * rate as f64) as usize];
        for &(start, length, pitch) in parts {
            let f = 440.0 * 2f64.powf((pitch - 69.0) / 12.0);
            let s = (start * rate as f64) as usize;
            for i in 0..(length * rate as f64) as usize {
                let t = i as f64 / rate as f64;
                let env = (t / 0.02).min(1.0) * ((length - t) / 0.02).min(1.0);
                let v: f64 = [1.0, 0.6, 0.4, 0.3].iter().enumerate().map(|(h, a)| a * (2.0 * std::f64::consts::PI * f * (h + 1) as f64 * t).sin()).sum();
                out[s + i] += (v * env * 0.1) as f32;
            }
        }
        out
    }

    #[test]
    fn a_low_sung_line_comes_out_note_by_note() {
        // Low, like a deep voice humming: C#2 D2 E2 A2, then a held G#2.
        let parts = [(0.2, 0.4, 37.0), (0.7, 0.4, 38.0), (1.2, 0.3, 40.0), (1.6, 0.5, 45.0), (2.3, 1.0, 44.0)];
        let audio = tone(44_100.0, &parts);
        let found = melody(&audio, 44_100.0, [0.0; 12], false).0;
        let got: Vec<(f64, u8)> = found.iter().map(|n| ((n.start * 20.0).round() / 20.0, n.pitch)).collect();
        assert_eq!(got.iter().map(|g| g.1).collect::<Vec<_>>(), vec![37, 38, 40, 45, 44], "{got:?}");
        for (note, part) in found.iter().zip(&parts) {
            assert!((note.start - part.0).abs() < 0.05, "{note:?} vs {part:?}");
            assert!((note.end - (part.0 + part.1)).abs() < 0.08, "{note:?} vs {part:?}");
        }
    }

    /// A note played in tune stays as it is even out of the key's scale; one
    /// sung between two notes goes to the scale's.
    #[test]
    fn in_tune_notes_stay_and_wavering_ones_go_to_the_scale() {
        let g_major = Key { tonic: 7, minor: false };
        // An F in tune (not in G major), then a note 45 cents above F (between F and F#).
        let audio = tone(44_100.0, &[(0.2, 0.5, 65.0), (0.9, 0.5, 65.45)]);
        let found = notes(&track(&audio, 44_100.0), Some(g_major));
        assert_eq!(found.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![65, 66], "{found:?}");
    }

    #[test]
    fn the_same_note_sung_twice_is_two_notes() {
        let parts = [(0.2, 0.4, 57.0), (0.65, 0.4, 57.0)];
        let found = melody(&tone(44_100.0, &parts), 44_100.0, [0.0; 12], false).0;
        assert_eq!(found.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![57, 57], "{found:?}");
    }

    #[test]
    fn live_pitch_follows_a_voice_as_it_comes_in() {
        let rate = 48_000.0;
        for (pitch, hz) in [(45.0, 110.0), (57.0, 220.0), (69.0, 440.0), (76.0, 659.26)] {
            let audio = tone(rate, &[(0.0, 0.1, pitch)]);
            let frame = &audio[1000..1000 + 2048];
            let (found, unsure) = live(frame, rate, 65.0, 1100.0).expect("a pitch");
            assert!((found / hz - 1.0).abs() < 0.005, "{hz}: {found}");
            assert!(unsure < 0.1);
        }
        let silence = vec![0f32; 2048];
        assert!(live(&silence, rate, 65.0, 1100.0).is_none());
        // Noise has no pitch.
        let mut seed = 7u32;
        let noise: Vec<f32> = (0..2048).map(|_| { seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345); (seed >> 9) as f32 / (1u32 << 23) as f32 - 1.0 }).collect();
        assert!(live(&noise, rate, 65.0, 1100.0).is_none());
    }

    /// The FFT's sums are the direct ones: the same pitch and certainty as
    /// working them out sample by sample, on voices, noise and odd lengths.
    #[test]
    fn the_live_tracker_matches_the_direct_sums() {
        let direct = |frame: &[f32], rate: f32, fmin: f64, fmax: f64| {
            let (lag_min, lag_max) = ((rate as f64 / fmax).floor() as usize, (rate as f64 / fmin).ceil() as usize);
            let width = frame.len() - lag_max;
            let mut d = vec![0f64; lag_max + 2];
            for tau in 1..=lag_max {
                d[tau] = (lag_max..lag_max + width).map(|j| ((frame[j] - frame[j - tau]) as f64).powi(2)).sum();
            }
            yin_pick(&d, &mut vec![0.0; d.len()], lag_min, lag_max, rate)
        };
        let mut seed = 3u32;
        let mut noise = || { seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345); (seed >> 9) as f32 / (1u32 << 23) as f32 - 1.0 };
        for (rate, len) in [(48_000.0f32, 2064usize), (44_100.0, 1896), (96_000.0, 4128)] {
            let mut tracker = LiveTracker::new(len, rate, 65.0, 1600.0);
            for pitch in [40.0, 52.5, 64.0, 81.0] {
                let voice: Vec<f32> = tone(rate, &[(0.0, 0.2, pitch)]).iter().map(|x| x + 0.02 * noise()).collect();
                let frame = &voice[500..500 + len];
                let (fast, slow) = (tracker.pitch(frame), direct(frame, rate, 65.0, 1600.0));
                let (a, b) = (fast.expect("a pitch"), slow.expect("a pitch"));
                assert!((a.0 / b.0 - 1.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9, "{pitch} at {rate}: {a:?} vs {b:?}");
            }
            let frame: Vec<f32> = (0..len).map(|_| noise()).collect();
            assert_eq!(tracker.pitch(&frame).is_some(), direct(&frame, rate, 65.0, 1600.0).is_some());
        }
    }

    /// It listens to the newest part of what it gets: a note that started 30
    /// ms ago is the pitch, not the one before it (the oldest 28 ms of the 43
    /// are still the last note).
    #[test]
    fn live_pitch_hears_the_newest_note() {
        let rate = 48_000.0;
        let len = (0.043 * rate) as usize;
        // A3 until 0.3 s, then E4.
        let audio = tone(rate, &[(0.0, 0.3, 57.0), (0.3, 0.3, 64.0)]);
        let end = (0.33 * rate) as usize;
        let (hz, _) = live(&audio[end - len..end], rate, 65.0, 1600.0).expect("a pitch");
        assert!((hz / 329.63 - 1.0).abs() < 0.01, "30 ms into E4 it hears {hz:.1} Hz");
    }

    #[test]
    fn silence_has_no_notes() {
        assert!(melody(&vec![0.0; 44_100], 44_100.0, [0.0; 12], true).0.is_empty());
    }

    /// Writes the track and notes of the file in PULSE_PITCH_IN to PULSE_PITCH_OUT
    /// (to compare with librosa.pyin by hand).
    #[test]
    #[ignore = "needs PULSE_PITCH_IN"]
    fn dump_track() {
        let (Ok(input), Ok(output)) = (std::env::var("PULSE_PITCH_IN"), std::env::var("PULSE_PITCH_OUT")) else { return };
        let mut reader = hound::WavReader::open(&input).unwrap();
        let spec = reader.spec();
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
            hound::SampleFormat::Int => reader.samples::<i32>().map(|s| s.unwrap() as f32 / (1i64 << (spec.bits_per_sample - 1)) as f32).collect(),
        };
        let channels = spec.channels as usize;
        let mono: Vec<f32> = samples.chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32).collect();
        let started = std::time::Instant::now();
        let track = track(&mono, spec.sample_rate as f32);
        let took = started.elapsed();
        let mut text = String::new();
        for f in &track.f0 {
            text.push_str(&format!("{}\n", f.unwrap_or(0.0)));
        }
        std::fs::write(&output, text).unwrap();
        let (found, key) = melody(&mono, spec.sample_rate as f32, [0.0; 12], true);
        eprintln!("key {:?}", key.map(|k| k.name()));
        eprintln!("{} frames in {took:?}; {} notes:", track.f0.len(), found.len());
        for n in &found {
            eprintln!("  {:.2} {:.2} {} amp {:.2}", n.start, n.end - n.start, n.pitch, n.amplitude);
        }
        eprintln!("free: {:?}", notes(&track, None).iter().map(|n| (format!("{:.2}", n.start), n.pitch)).collect::<Vec<_>>());
    }
}
