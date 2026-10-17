//! The pitch of one voice, and its notes: pYIN (Mauch & Dixon, 2014), as
//! `librosa.pyin` computes it, then the voiced stretches cut into notes where the
//! pitch moves to another semitone or the voice starts again.
//!
//! Made for singing and humming (what Basic Pitch, built for instruments and
//! chords, gets wrong: low voices, vowels' formants taken for notes).

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

/// A semitone away from the note for this many frames (46 ms) starts another.
const MOVE_FRAMES: usize = 4;
/// Notes shorter than this (seconds) are left out: slides and blips.
const MIN_NOTE: f64 = 0.08;

fn midi(f0: f64) -> f64 {
    69.0 + 12.0 * (f0 / 440.0).log2()
}

/// The notes of a pitch track: each voiced stretch, cut where the pitch settles
/// on another semitone or the voice comes in again louder; each at its median
/// pitch.
pub fn notes(track: &Track) -> Vec<NoteEvent> {
    let n = track.f0.len();
    let peak = track.rms.iter().copied().fold(0f32, f32::max).max(1e-9);
    let mut notes = Vec::new();
    let mut close = |from: usize, to: usize, pitches: &[f64]| {
        if to <= from || frame_time(to) - frame_time(from) < MIN_NOTE {
            return;
        }
        let mut sorted = pitches.to_vec();
        sorted.sort_by(f64::total_cmp);
        let pitch = sorted[sorted.len() / 2].round().clamp(0.0, 127.0) as u8;
        let level = track.rms[from..to].iter().copied().fold(0f32, f32::max) / peak;
        notes.push(NoteEvent { start: frame_time(from), end: frame_time(to), pitch, amplitude: level.sqrt() });
    };
    let mut i = 0;
    while i < n {
        if track.f0[i].is_none() {
            i += 1;
            continue;
        }
        // A voiced stretch: [i, end).
        let mut end = i;
        while end < n && track.f0[end].is_some() {
            end += 1;
        }
        let pitch: Vec<f64> = (i..end).map(|t| midi(track.f0[t].unwrap_or(FMIN))).collect();
        let mut start = i;
        let mut t = i;
        while t < end {
            let here = &pitch[start - i..=t - i];
            let mut sorted = here.to_vec();
            sorted.sort_by(f64::total_cmp);
            let centre = sorted[sorted.len() / 2];
            // Settled a semitone or more away for a while: a new note from there.
            let moved = (t..(t + MOVE_FRAMES).min(end)).all(|u| (pitch[u - i] - centre).abs() > 0.7) && t + MOVE_FRAMES <= end;
            // Sung again: a dip in the level, and it comes back up.
            let level = &track.short;
            let before = level[start..t].iter().copied().fold(0f32, f32::max);
            let after = level[t + 1..(t + 8).min(end)].iter().copied().fold(0f32, f32::max);
            let again = t > start + 3 && t + 1 < end && level[t] <= level[t - 1] && level[t] <= level[t + 1] && level[t] < 0.35 * before && after > 2.5 * level[t];
            if (moved || again) && t > start {
                close(start, t, &pitch[start - i..t - i]);
                start = if again { t + 1 } else { t };
            }
            t += 1;
        }
        close(start, end, &pitch[start - i..end - i]);
        i = end;
    }
    notes
}

/// The notes sung or played in `mono` (one at a time).
pub fn melody(mono: &[f32], rate: f32) -> Vec<NoteEvent> {
    notes(&track(mono, rate))
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
        let found = melody(&audio, 44_100.0);
        let got: Vec<(f64, u8)> = found.iter().map(|n| ((n.start * 20.0).round() / 20.0, n.pitch)).collect();
        assert_eq!(got.iter().map(|g| g.1).collect::<Vec<_>>(), vec![37, 38, 40, 45, 44], "{got:?}");
        for (note, part) in found.iter().zip(&parts) {
            assert!((note.start - part.0).abs() < 0.05, "{note:?} vs {part:?}");
            assert!((note.end - (part.0 + part.1)).abs() < 0.08, "{note:?} vs {part:?}");
        }
    }

    #[test]
    fn the_same_note_sung_twice_is_two_notes() {
        let parts = [(0.2, 0.4, 57.0), (0.65, 0.4, 57.0)];
        let found = melody(&tone(44_100.0, &parts), 44_100.0);
        assert_eq!(found.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![57, 57], "{found:?}");
    }

    #[test]
    fn silence_has_no_notes() {
        assert!(melody(&vec![0.0; 44_100], 44_100.0).is_empty());
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
        let found = notes(&track);
        eprintln!("{} frames in {took:?}; {} notes:", track.f0.len(), found.len());
        for n in &found {
            eprintln!("  {:.2} {:.2} {}", n.start, n.end - n.start, n.pitch);
        }
    }
}
