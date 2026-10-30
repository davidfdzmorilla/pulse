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
#[derive(Clone, Debug, Default)]
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

/// Cumulative mean normalized difference of a frame, for periods min..=max,
/// into `out` (`cumulative` and `raw` are room for the sums, `max + 1` long).
fn yin_frame(frame: &[f32], acf: &[f64], min_period: usize, max_period: usize, cumulative: &mut [f64], raw: &mut [f64], out: &mut Vec<f64>) {
    // Running energy of the frame's start, as librosa subtracts it.
    let mut sum = 0.0;
    for k in 0..=max_period {
        sum += (frame[k] as f64).powi(2);
        cumulative[k] = sum;
    }
    raw[0] = 0.0;
    for k in 1..=max_period {
        raw[k] = 2.0 * (acf[0] - acf[k]) - cumulative[k - 1];
    }
    let mut mean = 0.0;
    out.clear();
    for k in 1..=max_period {
        mean += raw[k];
        if k >= min_period {
            out.push(raw[k] / (mean / k as f64 + f64::MIN_POSITIVE));
        }
    }
}

/// What a frame tells the Viterbi search: the voiced bins with some
/// probability (the rest have none) and each unvoiced bin's.
struct Observation {
    voiced: Vec<(u16, f64)>,
    unvoiced: f64,
}

/// What [`observe`] needs worked out once: the thresholds, the Beta(2, 18)
/// weight of each, and the parts of the Boltzmann prior.
struct Priors {
    thresholds: Vec<f64>,
    beta: Vec<f64>,
    /// exp(-B·position), and 1 - exp(-B·count).
    decay: Vec<f64>,
    norm: Vec<f64>,
}

impl Priors {
    fn new(lags: usize) -> Self {
        let thresholds: Vec<f64> = (0..=THRESHOLDS).map(|i| i as f64 / THRESHOLDS as f64).collect();
        let beta = thresholds.windows(2).map(|w| beta_cdf(w[1]) - beta_cdf(w[0])).collect();
        let decay = (0..=lags).map(|p| (-BOLTZMANN * p as f64).exp()).collect();
        let norm = (0..=lags).map(|c| 1.0 - (-BOLTZMANN * c as f64).exp()).collect();
        Self { thresholds, beta, decay, norm }
    }
}

/// Room one thread reuses frame after frame.
struct Scratch {
    spectrum: Vec<Complex32>,
    fft: Vec<Complex32>,
    acf: Vec<f64>,
    cumulative: Vec<f64>,
    raw: Vec<f64>,
    yin: Vec<f64>,
    obs: Vec<f64>,
    troughs: Vec<usize>,
    probs: Vec<f64>,
}

/// One frame's loudness and observation.
struct Frame {
    rms: f32,
    short: f32,
    observation: Observation,
}

/// The pitch track of `mono` at `rate` Hz. The same audio again (one of the
/// last few analysed) comes from memory.
pub fn track(mono: &[f32], rate: f32) -> Track {
    track_with(mono, rate, &|_| true).expect("never stopped")
}

/// [`track`], telling `progress` the fraction done now and then (from any
/// thread); it returns false to stop, and then this gives None.
pub fn track_with(mono: &[f32], rate: f32, progress: &(dyn Fn(f32) -> bool + Sync)) -> Option<Track> {
    let key = crate::fingerprint(mono, rate);
    if let Some(found) = recall(key) {
        return Some(found);
    }
    let track = compute(&resample(mono, rate, RATE), progress)?;
    remember(key, &track);
    Some(track)
}

/// How many tracks are kept, and up to how many frames each (20 minutes).
const KEPT: usize = 6;
const KEPT_FRAMES: usize = 86 * 60 * 20;

static MEMORY: std::sync::Mutex<Vec<(crate::Fingerprint, std::sync::Arc<Track>)>> = std::sync::Mutex::new(Vec::new());

fn recall(key: crate::Fingerprint) -> Option<Track> {
    let mut kept = MEMORY.lock().unwrap_or_else(|e| e.into_inner());
    let at = kept.iter().position(|(k, _)| *k == key)?;
    // The one used last goes last: the oldest is forgotten first.
    let entry = kept.remove(at);
    let track = (*entry.1).clone();
    kept.push(entry);
    Some(track)
}

fn remember(key: crate::Fingerprint, track: &Track) {
    if track.f0.len() > KEPT_FRAMES {
        return;
    }
    let mut kept = MEMORY.lock().unwrap_or_else(|e| e.into_inner());
    kept.retain(|(k, _)| *k != key);
    if kept.len() >= KEPT {
        kept.remove(0);
    }
    kept.push((key, std::sync::Arc::new(track.clone())));
}

/// pYIN on `audio` (at [`RATE`]): the frames observed on every core, then the
/// likeliest path through them.
fn compute(audio: &[f32], progress: &(dyn Fn(f32) -> bool + Sync)) -> Option<Track> {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let sr = RATE as f64;
    // Centred frames: half a frame of silence each side.
    let mut padded = vec![0f32; FRAME / 2];
    padded.extend_from_slice(audio);
    padded.extend(std::iter::repeat_n(0.0, FRAME / 2));
    let frames = if padded.len() >= FRAME { 1 + (padded.len() - FRAME) / HOP } else { 0 };
    let min_period = (sr / FMAX).floor() as usize;
    let max_period = ((sr / FMIN).ceil() as usize).min(FRAME - 1);
    let bins = (12.0 * BINS_PER_SEMITONE as f64 * (FMAX / FMIN).log2()).floor() as usize + 1;
    let priors = Priors::new(max_period - min_period + 1);

    // Autocorrelation by FFT (twice the frame, so it doesn't wrap).
    let size = FRAME * 2;
    let mut planner = FftPlanner::<f32>::new();
    let (forward, inverse) = (planner.plan_fft_forward(size), planner.plan_fft_inverse(size));
    let scratch_len = forward.get_inplace_scratch_len().max(inverse.get_inplace_scratch_len());

    // Each frame on its own: the frames are shared out among the cores, which
    // tell the progress (the first 90 %) and listen for a stop as they go.
    const EVERY: usize = 128;
    let (stopped, done) = (AtomicBool::new(false), AtomicUsize::new(0));
    let mut slots: Vec<Option<Frame>> = (0..frames).map(|_| None).collect();
    crate::parallel(&mut slots, 256, |first, chunk| {
        let mut s = Scratch {
            spectrum: vec![Complex32::new(0.0, 0.0); size],
            fft: vec![Complex32::new(0.0, 0.0); scratch_len],
            acf: vec![0.0; max_period + 1],
            cumulative: vec![0.0; max_period + 1],
            raw: vec![0.0; max_period + 1],
            yin: Vec::with_capacity(max_period),
            obs: vec![0.0; 2 * bins],
            troughs: Vec::new(),
            probs: Vec::new(),
        };
        for (n, slot) in chunk.iter_mut().enumerate() {
            if n % EVERY == 0 && n > 0 {
                let so_far = done.fetch_add(EVERY, Ordering::Relaxed) + EVERY;
                if stopped.load(Ordering::Relaxed) || !progress(0.9 * so_far as f32 / frames as f32) {
                    stopped.store(true, Ordering::Relaxed);
                    return;
                }
            }
            let f = first + n;
            let frame = &padded[f * HOP..f * HOP + FRAME];
            let rms = (frame.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt();
            let middle = &frame[FRAME / 2 - SHORT / 2..FRAME / 2 + SHORT / 2];
            let short = (middle.iter().map(|x| x * x).sum::<f32>() / SHORT as f32).sqrt();
            let (head, tail) = s.spectrum.split_at_mut(FRAME);
            for (c, &x) in head.iter_mut().zip(frame) {
                *c = Complex32::new(x, 0.0);
            }
            tail.fill(Complex32::new(0.0, 0.0));
            forward.process_with_scratch(&mut s.spectrum, &mut s.fft);
            for c in &mut s.spectrum {
                *c = Complex32::new(c.norm_sqr(), 0.0);
            }
            inverse.process_with_scratch(&mut s.spectrum, &mut s.fft);
            for (a, c) in s.acf.iter_mut().zip(&s.spectrum) {
                *a = c.re as f64 / size as f64;
            }
            yin_frame(frame, &s.acf, min_period, max_period, &mut s.cumulative, &mut s.raw, &mut s.yin);
            let observation = observe(&s.yin, &priors, min_period, bins, &mut s.obs, &mut s.troughs, &mut s.probs);
            *slot = Some(Frame { rms, short, observation });
        }
    });
    if stopped.into_inner() {
        return None;
    }
    let (mut rms, mut short, mut observations) = (Vec::with_capacity(frames), Vec::with_capacity(frames), Vec::with_capacity(frames));
    for frame in slots.into_iter().flatten() {
        rms.push(frame.rms);
        short.push(frame.short);
        observations.push(frame.observation);
    }
    let states = viterbi(&observations, bins, &|fraction| progress(0.9 + 0.1 * fraction))?;
    let f0 = states.into_iter().map(|s| (s < bins).then(|| FMIN * 2f64.powf(s as f64 / (12 * BINS_PER_SEMITONE) as f64))).collect();
    Some(Track { f0, rms, short })
}

/// The probability of each pitch bin (voiced) and of no pitch (unvoiced, spread
/// over the second half) for one frame (`__pyin_helper`). `obs`, `troughs`
/// and `probs` are room to work in.
fn observe(yin: &[f64], priors: &Priors, min_period: usize, bins: usize, obs: &mut [f64], troughs: &mut Vec<usize>, probs: &mut Vec<f64>) -> Observation {
    let (thresholds, beta) = (&priors.thresholds, &priors.beta);
    let n = yin.len();
    obs.fill(0.0);
    troughs.clear();
    troughs.extend((0..n).filter(|&t| match t {
        0 => n > 1 && yin[0] < yin[1],
        t if t == n - 1 => yin[t] < yin[t - 1],
        t => yin[t] < yin[t - 1] && yin[t] <= yin[t + 1],
    }));
    let mut voiced = Vec::new();
    if !troughs.is_empty() {
        probs.clear();
        probs.resize(troughs.len(), 0.0);
        let scale = 1.0 - (-BOLTZMANN).exp();
        for (k, &b) in beta.iter().enumerate() {
            let threshold = thresholds[k + 1];
            let count = troughs.iter().filter(|&&t| yin[t] < threshold).count();
            let mut position = 0;
            for (i, &t) in troughs.iter().enumerate() {
                if yin[t] < threshold {
                    // Boltzmann prior: earlier troughs (shorter periods) first.
                    let prior = scale * priors.decay[position] / priors.norm[count];
                    probs[i] += prior * b;
                    position += 1;
                }
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
        voiced.extend(obs[..bins].iter().enumerate().filter(|(_, p)| **p != 0.0).map(|(b, &p)| (b as u16, p)));
    }
    let total = obs[..bins].iter().sum::<f64>().clamp(0.0, 1.0);
    Observation { voiced, unvoiced: (1.0 - total) / bins as f64 }
}

/// The likeliest path through the states (voiced bins, then unvoiced bins): the
/// pitch moves at most a few semitones a frame, voicing switches rarely.
/// `progress` as in [`track_with`].
fn viterbi(observations: &[Observation], bins: usize, progress: &dyn Fn(f32) -> bool) -> Option<Vec<usize>> {
    let states = 2 * bins;
    if observations.is_empty() {
        return Some(Vec::new());
    }
    let semitones = (MAX_TRANSITION_RATE * 12.0 * HOP as f64 / RATE as f64).round() as usize;
    let width = semitones * BINS_PER_SEMITONE + 1;
    let half = (width / 2) as isize;
    // Triangle weights over the reachable bins, each row summing to one.
    let weight = |d: isize| 1.0 - d.unsigned_abs() as f64 / (half + 1) as f64;
    let row_sum: Vec<f64> = (0..bins as isize).map(|i| (-half..=half).filter(|d| (0..bins as isize).contains(&(i + d))).map(weight).sum()).collect();
    let tiny = f64::MIN_POSITIVE;
    let ln = |x: f64| (x + tiny).ln();
    // The ln of each move into a bin from `d` bins away, staying in the block
    // or switching, worked out once: `moves[(switched · bins + bin) · width + d + half]`.
    let mut moves = vec![f64::NAN; 2 * bins * width];
    for switched in 0..2 {
        let switch = if switched == 0 { 1.0 - SWITCH_PROB } else { SWITCH_PROB };
        for bin in 0..bins as isize {
            for d in -half..=half {
                let i = bin + d;
                if (0..bins as isize).contains(&i) {
                    // From bin i to this bin: row i's weight at distance -d.
                    moves[(switched * bins + bin as usize) * width + (d + half) as usize] = ln(switch * weight(d) / row_sum[i as usize]);
                }
            }
        }
    }
    let unheard = ln(0.0);
    let fill = |ln_obs: &mut [f64], o: &Observation| {
        ln_obs[..bins].fill(unheard);
        for &(b, p) in &o.voiced {
            ln_obs[b as usize] = ln(p);
        }
        ln_obs[bins..].fill(ln(o.unvoiced));
    };
    let mut ln_obs = vec![0f64; states];
    fill(&mut ln_obs, &observations[0]);
    let mut score: Vec<f64> = ln_obs.iter().map(|&o| ln(1.0 / states as f64) + o).collect();
    let mut next = vec![0f64; states];
    // Where each state's best path came from, frame after frame.
    let mut back = vec![0u16; (observations.len() - 1) * states];
    for (t, obs) in observations[1..].iter().enumerate() {
        if t % 1024 == 1023 && !progress(t as f32 / observations.len() as f32) {
            return None;
        }
        fill(&mut ln_obs, obs);
        let from = &mut back[t * states..(t + 1) * states];
        for to in 0..states {
            let (block, bin) = (to / bins, (to % bins) as isize);
            // The bins in reach, the edges cut off.
            let (low, high) = ((-half).max(-bin), half.min(bins as isize - 1 - bin));
            // Each source block's reachable scores and the moves from them.
            let (span, reach) = ((high - low + 1) as usize, (low + half) as usize);
            let pairs = [0, 1].map(|source_block| {
                let switched = (source_block != block) as usize;
                let row = &moves[(switched * bins + bin as usize) * width + reach..][..span];
                let first = (source_block as isize * bins as isize + bin + low) as usize;
                (&score[first..first + span], row, first)
            });
            // The search in order keeps the first of the best: four searches at
            // once, each over every fourth source (each keeping its first of
            // the best), then the best of the four, the first of them in the
            // order the sources come in.
            let mut lanes = [(f64::NEG_INFINITY, usize::MAX); 4];
            for (source, (scores, row, _)) in pairs.iter().enumerate() {
                let (a, b) = (scores.chunks_exact(4), row.chunks_exact(4));
                let (ra, rb) = (a.remainder(), b.remainder());
                for (c, (x, y)) in a.zip(b).enumerate() {
                    for j in 0..4 {
                        let candidate = x[j] + y[j];
                        if candidate > lanes[j].0 {
                            lanes[j] = (candidate, source * width + 4 * c + j);
                        }
                    }
                }
                let at = scores.len() - ra.len();
                for (j, (x, y)) in ra.iter().zip(rb).enumerate() {
                    let candidate = x + y;
                    if candidate > lanes[j].0 {
                        lanes[j] = (candidate, source * width + at + j);
                    }
                }
            }
            let (best, order) = lanes.into_iter().fold((f64::NEG_INFINITY, usize::MAX), |a, b| if b.0 > a.0 || (b.0 == a.0 && b.1 < a.1) { b } else { a });
            let best_from = pairs[order / width].2 + order % width;
            next[to] = best + ln_obs[to];
            from[to] = best_from as u16;
        }
        std::mem::swap(&mut score, &mut next);
    }
    let mut state = (0..states).max_by(|&a, &b| score[a].total_cmp(&score[b])).unwrap_or(0);
    let mut path = vec![state; observations.len()];
    for t in (0..observations.len() - 1).rev() {
        state = back[t * states + state] as usize;
        path[t] = state;
    }
    progress(1.0);
    Some(path)
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

    /// A voice singing random notes of a minor scale, with vibrato, breaths
    /// and a little noise.
    fn singing(seconds: f64, rate: f32, seed: u64) -> Vec<f32> {
        let mut state = seed;
        let mut random = || {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut out = vec![0f32; (seconds * rate as f64) as usize];
        let (mut t, mut phase) = (0.1, 0.0f64);
        while t < seconds - 0.8 {
            let length = 0.18 + 0.5 * random();
            let pitch = 45.0 + [0.0, 2.0, 3.0, 5.0, 7.0, 8.0, 10.0][(random() * 7.0) as usize] + 12.0 * (random() * 2.0).floor();
            let start = (t * rate as f64) as usize;
            for i in 0..(length * rate as f64) as usize {
                let s = i as f64 / rate as f64;
                let f = 440.0 * 2f64.powf((pitch + 0.3 * (2.0 * std::f64::consts::PI * 5.5 * s).sin() - 69.0) / 12.0);
                phase += 2.0 * std::f64::consts::PI * f / rate as f64;
                let env = (s / 0.03).min(1.0) * ((length - s) / 0.04).min(1.0);
                let v: f64 = [1.0, 0.7, 0.5, 0.35, 0.2].iter().enumerate().map(|(h, a)| a * ((h + 1) as f64 * phase).sin()).sum();
                out[start + i] += (0.12 * env * v + 0.004 * (random() - 0.5)) as f32;
            }
            t += length + if random() < 0.3 { 0.25 } else { 0.03 };
        }
        out
    }

    /// Each step of pYIN on every core, with its sums worked out once, is the
    /// step as it was: from the same frames, the same differences, the same
    /// probabilities, and the same path through them, bit for bit.
    #[test]
    fn each_step_is_the_same_as_before() {
        for seed in [1, 2] {
            let audio = singing(20.0, RATE, seed);
            let mut padded = vec![0f32; FRAME / 2];
            padded.extend_from_slice(&audio);
            padded.extend(std::iter::repeat_n(0.0, FRAME / 2));
            let frames = 1 + (padded.len() - FRAME) / HOP;
            let (min_period, max_period) = ((RATE as f64 / FMAX).floor() as usize, (RATE as f64 / FMIN).ceil() as usize);
            let bins = (12.0 * BINS_PER_SEMITONE as f64 * (FMAX / FMIN).log2()).floor() as usize + 1;
            let priors = Priors::new(max_period - min_period + 1);
            let size = FRAME * 2;
            let mut planner = FftPlanner::<f32>::new();
            let (forward, inverse) = (planner.plan_fft_forward(size), planner.plan_fft_inverse(size));
            let (mut obs, mut troughs, mut probs) = (vec![0.0; 2 * bins], Vec::new(), Vec::new());
            let (mut cumulative, mut raw, mut yin) = (vec![0.0; max_period + 1], vec![0.0; max_period + 1], Vec::new());
            let (mut dense, mut sparse) = (Vec::new(), Vec::new());
            for f in 0..frames {
                let frame = &padded[f * HOP..f * HOP + FRAME];
                let mut spectrum: Vec<Complex32> = frame.iter().map(|&x| Complex32::new(x, 0.0)).chain(std::iter::repeat_n(Complex32::new(0.0, 0.0), size - FRAME)).collect();
                forward.process(&mut spectrum);
                for c in &mut spectrum {
                    *c = Complex32::new(c.norm_sqr(), 0.0);
                }
                inverse.process(&mut spectrum);
                let acf: Vec<f64> = spectrum[..=max_period].iter().map(|c| c.re as f64 / size as f64).collect();
                let before = crate::reference::yin_frame(frame, &acf, min_period, max_period);
                yin_frame(frame, &acf, min_period, max_period, &mut cumulative, &mut raw, &mut yin);
                assert_eq!(yin, before);
                let old = crate::reference::observe(&yin, &priors.thresholds, &priors.beta, min_period, bins);
                let new = observe(&yin, &priors, min_period, bins, &mut obs, &mut troughs, &mut probs);
                let mut unpacked = vec![0.0; 2 * bins];
                for &(b, p) in &new.voiced {
                    unpacked[b as usize] = p;
                }
                unpacked[bins..].fill(new.unvoiced);
                assert_eq!(unpacked, old, "frame {f}");
                dense.push(old);
                sparse.push(new);
            }
            assert_eq!(viterbi(&sparse, bins, &|_| true).unwrap(), crate::reference::viterbi(&dense, bins), "seed {seed}");
        }
    }

    /// The whole track, against the one before, from 22.05 (nothing
    /// resampled), 44.1 and 48 kHz: the same notes, and the same pitch but in a
    /// handful of frames, there a tenth of a semitone (pYIN's step) away. The
    /// resampling's filter now comes from a table (its samples within 1e-6 of
    /// before), and the compiler may round a Beta weight differently by a
    /// last bit: that is enough to tip a frame on the edge of two steps.
    #[test]
    fn the_fast_track_hears_the_same_notes_from_any_rate() {
        for (rate, seed) in [(RATE, 1), (44_100.0, 4), (48_000.0, 5)] {
            let audio = singing(30.0, rate, seed);
            let (fast, slow) = (track(&audio, rate), crate::reference::track(&audio, rate));
            assert_eq!(fast.f0.len(), slow.f0.len());
            let mut moved = 0;
            for (a, b) in fast.f0.iter().zip(&slow.f0) {
                match (a, b) {
                    (Some(a), Some(b)) => {
                        let cents = 1200.0 * (a / b).log2().abs();
                        assert!(cents < 10.5, "{a} vs {b}");
                        moved += (cents > 1e-6) as usize;
                    }
                    (None, None) => {}
                    _ => moved += 1,
                }
            }
            assert!(moved * 200 <= fast.f0.len(), "{moved} of {} frames differ", fast.f0.len());
            let pitches = |t: &Track| notes(t, None).iter().map(|n| n.pitch).collect::<Vec<_>>();
            assert_eq!(pitches(&fast), pitches(&slow), "at {rate}");
            for (a, b) in notes(&fast, None).iter().zip(&notes(&slow, None)) {
                assert!((a.start - b.start).abs() < 0.03 && (a.end - b.end).abs() < 0.03, "{a:?} vs {b:?}");
            }
        }
    }

    /// The same audio again comes from memory; a stop asked for stops it.
    #[test]
    fn tracks_are_remembered_and_can_be_stopped() {
        let audio = singing(6.0, 44_100.0, 8);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let stop = track_with(&audio, 44_100.0, &|_| {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            false
        });
        assert!(stop.is_none());
        assert!(calls.into_inner() >= 1);
        let seen = std::sync::Mutex::new(Vec::new());
        let first = track_with(&audio, 44_100.0, &|f| {
            seen.lock().unwrap().push(f);
            true
        })
        .unwrap();
        let seen = seen.into_inner().unwrap();
        assert!(seen.iter().all(|f| (0.0..=1.0).contains(f)) && seen.last() == Some(&1.0), "{seen:?}");
        let started = std::time::Instant::now();
        let again = track(&audio, 44_100.0);
        assert!(started.elapsed().as_millis() < 50);
        assert_eq!(again.f0, first.f0);
        // Another rate is other audio.
        assert_ne!(crate::fingerprint(&audio, 44_100.0), crate::fingerprint(&audio, 48_000.0));
        assert_ne!(crate::fingerprint(&audio, 44_100.0), crate::fingerprint(&audio[1..], 44_100.0));
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


