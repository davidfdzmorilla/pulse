//! Tuner: pitch correction for a voice (or any sound that plays one note at a
//! time), as Auto-Tune does. It hears the pitch coming in (YIN, every few
//! milliseconds), picks the note of the scale it is nearest and moves the
//! sound there: at once (the robotic effect) or gliding, `speed_ms` to get
//! there; with `humanize`, a note held long is left freer (its vibrato
//! stays).
//!
//! The sound is moved by reading a short delay line faster or slower. Where
//! the read falls too far behind or ahead it jumps by exactly one period of
//! the voice, fading across: the two places are in phase, so the jump is not
//! heard. It sounds about a period late (some 5 ms for a voice around A3).

use crate::model::{TunerParams, TunerScale};
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

/// The pitches it follows (Hz): a bass voice to a high soprano.
const FMIN: f32 = 65.0;
const FMAX: f32 = 1_100.0;
/// What is heard each time (seconds: two periods of the lowest voice) and how
/// often (a hop, in seconds).
const WINDOW_SECONDS: f32 = 0.032;
const HOP_SECONDS: f32 = 0.005;
/// Below this (dBFS, RMS of what is heard), no voice.
const GATE_DB: f32 = -50.0;
/// How unsure a pitch may be and count (YIN's dip).
const THRESHOLD: f32 = 0.15;
/// The note playing stays until another is this much (semitones) nearer: a
/// voice between two notes doesn't flip between them.
const HYSTERESIS: f32 = 0.25;
/// Humanize at full: a note held `HUMAN_AFTER` seconds glides this much slower
/// (seconds more).
const HUMAN_SLOWER: f32 = 0.3;
const HUMAN_AFTER: f32 = 0.3;
/// Without a voice, the correction goes back to none over this (seconds).
const UNVOICED_RETURN: f32 = 0.03;
/// The voice its latency is told for (see [`Tuner::latency`]): A3.
const TYPICAL_HZ: f32 = 220.0;
/// The delay line (samples, a power of two).
const LINE: usize = 1 << 13;

/// The pitch classes (C first) of a scale, as bits.
pub fn scale_bits(tonic: u8, minor: bool) -> u16 {
    let steps: [u8; 7] = if minor { [0, 2, 3, 5, 7, 8, 10] } else { [0, 2, 4, 5, 7, 9, 11] };
    steps.iter().fold(0, |bits, s| bits | 1 << ((tonic + s) % 12))
}

const CHROMATIC: u16 = 0xFFF;

/// YIN on the newest stretch of input, with FFTs, without allocating.
struct Detector {
    len: usize,
    lag_min: usize,
    lag_max: usize,
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    a: Vec<Complex<f32>>,
    b: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    squares: Vec<f64>,
    d: Vec<f32>,
}

impl Detector {
    fn new(rate: f32) -> Self {
        let len = (WINDOW_SECONDS * rate) as usize;
        let (lag_min, lag_max) = ((rate / FMAX).floor() as usize, (rate / FMIN).ceil() as usize);
        let size = len.next_power_of_two();
        let mut planner = FftPlanner::<f32>::new();
        let (forward, inverse) = (planner.plan_fft_forward(size), planner.plan_fft_inverse(size));
        let scratch = vec![Complex::default(); forward.get_inplace_scratch_len().max(inverse.get_inplace_scratch_len())];
        Self { len, lag_min, lag_max, forward, inverse, a: vec![Complex::default(); size], b: vec![Complex::default(); size], scratch, squares: vec![0.0; len + 1], d: vec![0.0; lag_max + 2] }
    }

    /// The period (samples) of `frame` (`len` samples, oldest first), if it
    /// has one: each of its newest samples compared with the one a period
    /// before.
    fn period(&mut self, frame: &[f32]) -> Option<f32> {
        let (lag_min, lag_max) = (self.lag_min.max(2), self.lag_max);
        let width = self.len - lag_max;
        for (i, slot) in self.a.iter_mut().enumerate() {
            *slot = Complex::new(frame.get(i).copied().unwrap_or(0.0), 0.0);
        }
        for (i, slot) in self.b.iter_mut().enumerate() {
            *slot = Complex::new(if i < width { frame[lag_max + i] } else { 0.0 }, 0.0);
        }
        self.forward.process_with_scratch(&mut self.a, &mut self.scratch);
        self.forward.process_with_scratch(&mut self.b, &mut self.scratch);
        for (a, b) in self.a.iter_mut().zip(&self.b) {
            *a *= b.conj();
        }
        self.inverse.process_with_scratch(&mut self.a, &mut self.scratch);
        let scale = 1.0 / self.a.len() as f64;
        for (i, &x) in frame.iter().enumerate() {
            self.squares[i + 1] = self.squares[i] + (x as f64) * (x as f64);
        }
        let energy = |from: usize| self.squares[from + width] - self.squares[from];
        let newest = energy(lag_max);
        if newest <= 0.0 {
            return None;
        }
        // The cumulative mean normalized difference, as it goes.
        let mut running = 0.0f64;
        self.d[0] = 1.0;
        for tau in 1..=lag_max {
            let cross = self.a[lag_max - tau].re as f64 * scale;
            let d = (newest + energy(lag_max - tau) - 2.0 * cross).max(0.0);
            running += d;
            self.d[tau] = if running > 0.0 { (d * tau as f64 / running) as f32 } else { 1.0 };
        }
        self.d[lag_max + 1] = 1.0;
        let mut tau = lag_min;
        while tau <= lag_max {
            if self.d[tau] < THRESHOLD {
                while tau < lag_max && self.d[tau + 1] < self.d[tau] {
                    tau += 1;
                }
                let (a, b, c) = (self.d[tau - 1], self.d[tau], self.d[tau + 1]);
                let shift = if a + c - 2.0 * b != 0.0 { 0.5 * (a - c) / (a + c - 2.0 * b) } else { 0.0 };
                return Some(tau as f32 + shift.clamp(-1.0, 1.0));
            }
            tau += 1;
        }
        None
    }
}

pub struct Tuner {
    pub(crate) params: TunerParams,
    rate: f32,
    detector: Detector,
    /// The newest input (mono), as a ring, and it in order for the detector.
    heard: Vec<f32>,
    frame: Vec<f32>,
    at: usize,
    filled: usize,
    since: usize,
    hop: usize,
    /// The note it goes to, how long (seconds) it has, and the correction
    /// wanted and now (semitones).
    note: Option<u8>,
    held: f32,
    target: f32,
    shift: f32,
    /// Whether there is a voice, and its period (samples).
    voiced: bool,
    period: f32,
    /// The delay line (per channel), where it is written, where it is read
    /// (in samples since it started: written minus read is the delay) and,
    /// jumping, where it read before and how far into the fade.
    line: [Vec<f32>; 2],
    written: u64,
    read: f64,
    fade: Option<(f64, f32, f32)>,
}

impl Tuner {
    /// Allocates; off the audio thread.
    pub fn new(params: TunerParams, rate: f32) -> Self {
        let detector = Detector::new(rate);
        let len = detector.len;
        let period = 0.005 * rate;
        let mut tuner = Self {
            params,
            rate,
            detector,
            heard: vec![0.0; len],
            frame: vec![0.0; len],
            at: 0,
            filled: 0,
            since: 0,
            hop: ((HOP_SECONDS * rate) as usize).max(1),
            note: None,
            held: 0.0,
            target: 0.0,
            shift: 0.0,
            voiced: false,
            period,
            line: [vec![0.0; LINE], vec![0.0; LINE]],
            written: 0,
            read: 0.0,
            fade: None,
        };
        tuner.reset();
        tuner
    }

    /// Forgets what it heard, without allocating.
    pub fn reset(&mut self) {
        self.heard.fill(0.0);
        self.at = 0;
        self.filled = 0;
        self.since = 0;
        self.note = None;
        self.held = 0.0;
        self.target = 0.0;
        self.shift = 0.0;
        self.voiced = false;
        self.period = 0.005 * self.rate;
        self.line.iter_mut().for_each(|l| l.fill(0.0));
        self.written = LINE as u64;
        self.fade = None;
        let (low, high) = self.range();
        self.read = self.written as f64 - ((low + high) * 0.5) as f64;
    }

    /// How late it sounds (samples), as the mixer makes up for it: the middle
    /// of its range for a voice around A3. It reads a period nearer or
    /// further as it goes (that is how it moves the pitch), so a voice sounds
    /// a few milliseconds earlier or later than this.
    pub fn latency(&self) -> usize {
        let period = self.rate / TYPICAL_HZ;
        (1.1 * period + 4.0).round() as usize
    }

    /// How far behind the input it may read (samples): about a period.
    fn range(&self) -> (f32, f32) {
        let low = 0.6 * self.period + 4.0;
        (low, low + self.period)
    }

    /// The pitch classes it tunes to: `project` (bits, 0 for none) or its own.
    fn scale(&self, project: u16) -> u16 {
        match self.params.scale {
            TunerScale::Project if project != 0 => project,
            TunerScale::Project | TunerScale::Chromatic => CHROMATIC,
            TunerScale::Key { tonic, minor } => scale_bits(tonic % 12, minor),
        }
    }

    /// Hears the newest input: the note to go to and the correction wanted.
    fn listen(&mut self, scale: u16) {
        let len = self.heard.len();
        for i in 0..len {
            self.frame[i] = self.heard[(self.at + i) % len];
        }
        let rms = (self.frame.iter().map(|x| x * x).sum::<f32>() / len as f32).sqrt();
        let period = if self.filled >= len && 20.0 * rms.max(1e-9).log10() > GATE_DB { self.detector.period(&self.frame) } else { None };
        let dt = self.hop as f32 / self.rate;
        let Some(period) = period else {
            self.voiced = false;
            self.note = None;
            self.held = 0.0;
            self.target = 0.0;
            return;
        };
        self.voiced = true;
        self.period = period;
        let m = 69.0 + 12.0 * (self.rate / period / 440.0).log2();
        // The nearest note of the scale.
        let near = |m: f32| {
            let r = m.round() as i32;
            (r - 6..=r + 6).filter(|n| (0..128).contains(n) && scale & 1 << n.rem_euclid(12) != 0).min_by(|a, b| (*a as f32 - m).abs().total_cmp(&(*b as f32 - m).abs()))
        };
        let Some(nearest) = near(m) else { return };
        let note = match self.note {
            Some(c) if scale & 1 << (c % 12) != 0 && (c as f32 - m).abs() < (nearest as f32 - m).abs() + HYSTERESIS => c,
            _ => nearest as u8,
        };
        if Some(note) == self.note {
            self.held += dt;
        } else {
            self.note = Some(note);
            self.held = 0.0;
        }
        self.target = (note as f32 - m) * self.params.amount.clamp(0.0, 1.0);
    }

    /// Reads channel `c` at `at` (samples since it started), between samples.
    fn sample(&self, c: usize, at: f64) -> f32 {
        let i = at.floor();
        let t = (at - i) as f32;
        let i = i as i64;
        let line = &self.line[c];
        let x = |k: i64| line[(k as usize) & (LINE - 1)];
        let (y0, y1, y2, y3) = (x(i - 1), x(i), x(i + 1), x(i + 2));
        // Cubic Hermite.
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], project_scale: u16) {
        let scale = self.scale(project_scale);
        let p = self.params;
        let len = self.heard.len();
        for (left, right) in l.iter_mut().zip(r.iter_mut()) {
            // Hear.
            self.heard[self.at] = 0.5 * (*left + *right);
            self.at = (self.at + 1) % len;
            self.filled = (self.filled + 1).min(len);
            self.since += 1;
            if self.since >= self.hop {
                self.since = 0;
                self.listen(scale);
            }
            // Glide towards the correction wanted.
            let seconds = if self.voiced {
                p.speed_ms.max(0.0) / 1000.0 + p.humanize.clamp(0.0, 1.0) * HUMAN_SLOWER * (self.held / HUMAN_AFTER).min(1.0)
            } else {
                UNVOICED_RETURN
            };
            self.shift = if seconds < 0.0005 { self.target } else { self.shift + (self.target - self.shift) * (1.0 - (-1.0 / (seconds * self.rate)).exp()) };
            let ratio = 2f64.powf(self.shift as f64 / 12.0);
            // Write, and read faster or slower.
            let w = (self.written as usize) & (LINE - 1);
            self.line[0][w] = *left;
            self.line[1][w] = *right;
            self.written += 1;
            self.read += ratio;
            let (low, high) = self.range();
            let behind = (self.written as f64 - 1.0 - self.read) as f32;
            if self.fade.is_none() {
                // Too near (reading faster) or too far (slower): a period back or on.
                let jump = if behind < low { -self.period } else if behind > high { self.period } else { 0.0 };
                if jump != 0.0 {
                    self.fade = Some((self.read, 0.0, self.period.max(16.0)));
                    self.read += jump as f64;
                }
            }
            // Way off (the period changed a lot, or it read past the input): back in the middle.
            let behind = (self.written as f64 - 1.0 - self.read) as f32;
            if !(2.0..(LINE / 2) as f32).contains(&behind) {
                self.read = self.written as f64 - 1.0 - ((low + high) * 0.5) as f64;
                self.fade = None;
            }
            let now = |c: usize| self.sample(c, self.read);
            let (mut a, mut b) = (now(0), now(1));
            if let Some((before, done, length)) = self.fade {
                let t = done / length;
                a = a * t + self.sample(0, before) * (1.0 - t);
                b = b * t + self.sample(1, before) * (1.0 - t);
                self.fade = if done + 1.0 >= length { None } else { Some((before + ratio, done + 1.0, length)) };
            }
            *left = a;
            *right = b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn params(speed_ms: f32, humanize: f32, amount: f32, scale: TunerScale) -> TunerParams {
        TunerParams { speed_ms, humanize, amount, scale }
    }

    /// A tone at `hz(t)`, with `harmonics` (their levels, from the 2nd).
    fn tone(seconds: f32, hz: impl Fn(f32) -> f32, harmonics: &[f32]) -> Vec<f32> {
        let mut phase = 0f32;
        (0..(seconds * RATE) as usize)
            .map(|i| {
                phase = (phase + hz(i as f32 / RATE) / RATE).fract();
                let mut x = (std::f32::consts::TAU * phase).sin();
                for (k, level) in harmonics.iter().enumerate() {
                    x += level * (std::f32::consts::TAU * phase * (k + 2) as f32).sin();
                }
                0.25 * x
            })
            .collect()
    }

    fn run(tuner: &mut Tuner, input: &[f32], block: usize, scale: u16) -> Vec<f32> {
        let mut out = Vec::new();
        for chunk in input.chunks(block) {
            let (mut l, mut r) = (chunk.to_vec(), chunk.to_vec());
            tuner.process(&mut l, &mut r, scale);
            out.extend(l);
        }
        out
    }

    /// The frequency of a sine-like `x` (Hz), from its rising zero crossings.
    fn frequency(x: &[f32]) -> f32 {
        let mut crossings = Vec::new();
        for i in 1..x.len() {
            if x[i - 1] < 0.0 && x[i] >= 0.0 {
                crossings.push(i as f32 - 1.0 + x[i - 1] / (x[i - 1] - x[i]));
            }
        }
        (crossings.len() - 1) as f32 / (crossings[crossings.len() - 1] - crossings[0]) * RATE
    }

    fn cents(a: f32, b: f32) -> f32 {
        1200.0 * (a / b).log2()
    }

    /// Out of tune by 30 cents, sharp or flat, high or low: tuned at once
    /// (speed 0) to the note.
    #[test]
    fn an_out_of_tune_voice_comes_out_in_tune() {
        for (hz, note) in [(440.0 * 2f32.powf(0.3 / 12.0), 440.0), (220.0 * 2f32.powf(-0.3 / 12.0), 220.0), (110.0 * 2f32.powf(0.35 / 12.0), 110.0), (880.0 * 2f32.powf(-0.4 / 12.0), 880.0)] {
            let mut tuner = Tuner::new(params(0.0, 0.0, 1.0, TunerScale::Chromatic), RATE);
            let out = run(&mut tuner, &tone(1.0, |_| hz, &[]), 256, 0);
            let got = frequency(&out[(0.3 * RATE) as usize..]);
            println!("{hz:.1} Hz → {got:.2} Hz ({:+.1} cents)", cents(got, note));
            assert!(cents(got, note).abs() < 3.0, "{hz:.1} Hz came out at {got:.2} ({:+.1} cents off {note})", cents(got, note));
        }
    }

    /// In a scale: a F# 40 cents flat goes to G in C major (F# is not in it),
    /// to F# in D major; the project's scale if it has one, chromatic if not.
    #[test]
    fn it_tunes_to_the_scale() {
        let hz = 440.0 * 2f32.powf((66.4 - 69.0) / 12.0);
        let note = |scale: TunerScale, project: u16| {
            let mut tuner = Tuner::new(params(0.0, 0.0, 1.0, scale), RATE);
            let out = run(&mut tuner, &tone(0.8, |_| hz, &[]), 512, project);
            let got = frequency(&out[(0.3 * RATE) as usize..]);
            (69.0 + 12.0 * (got / 440.0).log2()).round() as i32
        };
        assert_eq!(note(TunerScale::Key { tonic: 0, minor: false }, 0), 67);
        assert_eq!(note(TunerScale::Key { tonic: 2, minor: false }, 0), 66);
        assert_eq!(note(TunerScale::Project, scale_bits(0, false)), 67);
        assert_eq!(note(TunerScale::Project, 0), 66);
        assert_eq!(note(TunerScale::Chromatic, scale_bits(0, false)), 66);
    }

    /// Speed: at 0 it is there at once; at 200 ms, 60 ms in it is part of the
    /// way and 1 s in, there.
    #[test]
    fn the_speed_is_how_fast_it_gets_there() {
        let hz = 440.0 * 2f32.powf(0.45 / 12.0);
        let off = |speed: f32, from: f32, to: f32| {
            let mut tuner = Tuner::new(params(speed, 0.0, 1.0, TunerScale::Chromatic), RATE);
            let out = run(&mut tuner, &tone(1.2, |_| hz, &[]), 128, 0);
            cents(frequency(&out[(from * RATE) as usize..(to * RATE) as usize]), 440.0)
        };
        let (instant, slow_early, slow_late) = (off(0.0, 0.05, 0.1), off(200.0, 0.05, 0.1), off(200.0, 1.0, 1.2));
        println!("instant {instant:+.1}, 200 ms early {slow_early:+.1}, late {slow_late:+.1} cents");
        assert!(instant.abs() < 4.0 && slow_early > 15.0 && slow_late.abs() < 4.0);
    }

    /// Amount 0: the voice as it was (a little late); 50 %: halfway.
    #[test]
    fn the_amount_is_how_much() {
        let hz = 440.0 * 2f32.powf(0.4 / 12.0);
        let input = tone(0.8, |_| hz, &[0.4, 0.2]);
        let mut tuner = Tuner::new(params(0.0, 0.0, 0.0, TunerScale::Chromatic), RATE);
        let out = run(&mut tuner, &input, 256, 0);
        // The same sound, late by some samples.
        let at = (0.4 * RATE) as usize;
        let late = (0..400).min_by(|&a, &b| {
            let e = |d: usize| (0..2000).map(|i| (out[at + i] - input[at + i - d]).powi(2)).sum::<f32>();
            e(a).total_cmp(&e(b))
        }).unwrap();
        let error: f32 = (0..2000).map(|i| (out[at + i] - input[at + i - late]).powi(2)).sum::<f32>() / (0..2000).map(|i| input[at + i].powi(2)).sum::<f32>();
        println!("late {late} samples ({:.1} ms), error {:.1} dB", late as f32 / RATE * 1000.0, 10.0 * error.log10());
        assert!(10.0 * error.log10() < -40.0 && late < (0.012 * RATE) as usize);
        let mut tuner = Tuner::new(params(0.0, 0.0, 0.5, TunerScale::Chromatic), RATE);
        let half = cents(frequency(&run(&mut tuner, &tone(0.8, |_| hz, &[]), 256, 0)[(0.3 * RATE) as usize..]), 440.0);
        assert!((half - 20.0).abs() < 4.0, "{half:+.1} cents");
    }

    /// A vibrato (±40 cents, 5.5 Hz) on a held note: flattened to under half
    /// without humanize (what is heard is some 10 ms old: a quick vibrato is
    /// followed a little late), left through with it once the note has held a
    /// moment.
    #[test]
    fn humanize_leaves_a_held_notes_vibrato() {
        let vibrato = |t: f32| 440.0 * 2f32.powf(0.4 * (std::f32::consts::TAU * 5.5 * t).sin() / 12.0);
        let swing = |humanize: f32| {
            let mut tuner = Tuner::new(params(5.0, humanize, 1.0, TunerScale::Chromatic), RATE);
            let out = run(&mut tuner, &tone(1.6, vibrato, &[]), 256, 0);
            // Pitch over 20 ms stretches, from 0.8 s.
            let stretch = (0.02 * RATE) as usize;
            let pitches: Vec<f32> = out[(0.8 * RATE) as usize..].chunks(stretch).filter(|c| c.len() == stretch).map(|c| cents(frequency(c), 440.0)).collect();
            pitches.iter().cloned().fold(f32::MIN, f32::max) - pitches.iter().cloned().fold(f32::MAX, f32::min)
        };
        let (flat, human) = (swing(0.0), swing(1.0));
        println!("vibrato swing: {flat:.0} cents tuned, {human:.0} humanized");
        assert!(flat < 40.0 && human > 70.0);
    }

    /// The jumps are not heard: a voice with harmonics, moved half a
    /// semitone, keeps its level and has no clicks (no sample-to-sample step
    /// bigger than the input's).
    #[test]
    fn the_jumps_are_not_heard() {
        for hz in [110.0, 233.0, 587.0] {
            let hz = hz * 2f32.powf(0.5 / 12.0) * 0.999;
            let input = tone(1.0, |_| hz, &[0.5, 0.3, 0.2]);
            let mut tuner = Tuner::new(params(0.0, 0.0, 1.0, TunerScale::Chromatic), RATE);
            let out = run(&mut tuner, &input, 256, 0);
            let from = (0.3 * RATE) as usize;
            let rms = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt();
            let level = 20.0 * (rms(&out[from..]) / rms(&input[from..])).log10();
            let step = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
            // A moved voice's steps are bigger by as much as it is higher.
            let (steps_in, steps_out) = (step(&input[from..]), step(&out[from..]));
            println!("{hz:.0} Hz: level {level:+.2} dB, steps {steps_out:.4} / {steps_in:.4}");
            assert!(level.abs() < 0.5, "{hz} Hz: {level:+.2} dB");
            assert!(steps_out < steps_in * 1.1, "{hz} Hz: a click ({steps_out:.4} against {steps_in:.4})");
        }
    }

    /// Cut into blocks of any size it sounds the same (the export must sound
    /// as the playback), and silence, noise and a jump between notes give
    /// nothing odd.
    #[test]
    fn blocks_silence_and_noise() {
        let mut input = tone(0.5, |t| if t < 0.25 { 300.0 } else { 450.0 }, &[0.3]);
        input.extend(vec![0.0; 4800]);
        let mut seed = 1u32;
        input.extend((0..9600).map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            0.3 * ((seed >> 8) as f32 / (1 << 23) as f32 - 1.0)
        }));
        let reference = run(&mut Tuner::new(params(20.0, 0.5, 1.0, TunerScale::Chromatic), RATE), &input, 128, 0);
        for block in [1, 77, 512, 4096] {
            assert_eq!(run(&mut Tuner::new(params(20.0, 0.5, 1.0, TunerScale::Chromatic), RATE), &input, block, 0), reference, "blocks of {block}");
        }
        assert!(reference.iter().all(|x| x.is_finite() && x.abs() < 1.0));
    }
}
