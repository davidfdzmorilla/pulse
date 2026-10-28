//! Kick drum synthesizer. Each hit has three layers: a tuned body (sine, triangle,
//! square or saw) swept down by a pitch envelope and distorted by a drive envelope,
//! a clean sine sub, and a click (one of twelve synthesized ones, or a sample).
//! The mix goes through a three-band EQ with a mid bell, a punch compressor and an
//! optional lookahead limiter. Monophonic: a new hit fades the previous one out.
//! Everything is fixed-size, so it can be created and played on the audio thread.

use crate::instruments::{saw, semitones, square, svf_g, Svf};
use crate::model::{db_to_gain, KickDrive, KickParams, KickWave, Shape};
use crate::sample::Sample;
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

/// Output scale that leaves headroom for the click on top of a full-scale body.
const OUTPUT: f32 = 0.7;
/// Fade applied to a hit cut off by the next one.
const RETRIGGER_FADE_SECONDS: f32 = 0.003;
/// Limiter ceiling (-0.3 dBFS) and lookahead.
const CEILING: f32 = 0.966;
const LOOKAHEAD_SECONDS: f32 = 0.0015;
const MAX_LOOKAHEAD: usize = 512;
const BLOCK: usize = 64;

fn note_freq(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// A drawn shape at `t` (0..1) with the curve knob's tension on top: `curve` 0
/// keeps the drawing, 1 makes it drop fast and linger. On the default straight
/// line this is the classic (1 - t)^n kick envelope.
fn shaped(shape: &Shape, t: f32, curve: f32) -> f32 {
    shape.value_at(t).powf(1.0 + curve.clamp(0.0, 1.0) * 6.0)
}

/// The settings a hit actually plays with: with a synced length, the decay
/// stretches so the hit lasts `sync_beats` at `tempo`.
pub fn effective(p: &KickParams, tempo: f64) -> KickParams {
    if p.sync_beats <= 0.0 || tempo <= 0.0 {
        return *p;
    }
    let total = (p.sync_beats as f64 * 60_000.0 / tempo) as f32;
    KickParams { decay: (total - p.attack.max(0.05) - p.hold).max(1.0), ..*p }
}

/// Pitch offset in semitones `ms` milliseconds into a hit.
pub fn pitch_offset(p: &KickParams, ms: f32) -> f32 {
    let t = ms / p.pitch_time.max(0.1);
    // Past the sweep the shape holds its last value.
    p.pitch_amount * shaped(&p.pitch_shape, t.min(1.0), p.pitch_curve)
}

/// Amplitude envelope `ms` milliseconds into a hit.
pub fn amplitude(p: &KickParams, ms: f32) -> f32 {
    let attack = p.attack.max(0.05);
    if ms < attack {
        ms / attack
    } else if ms < attack + p.hold {
        1.0
    } else {
        // A drawn curve may not end at zero: fade the last 2 ms so the hit never clicks off.
        let end_fade = ((length_ms(p) - ms) / 2.0).clamp(0.0, 1.0);
        shaped(&p.amp_shape, (ms - attack - p.hold) / p.decay.max(1.0), p.decay_curve) * end_fade
    }
}

/// Drive amount `ms` milliseconds into a hit.
pub fn drive_at(p: &KickParams, ms: f32) -> f32 {
    p.drive * p.drive_shape.value_at(ms / length_ms(p))
}

/// Total length of a hit in milliseconds.
pub fn length_ms(p: &KickParams) -> f32 {
    p.attack.max(0.05) + p.hold + p.decay.max(1.0)
}

pub(crate) fn distort(x: f32, mode: KickDrive, drive: f32) -> f32 {
    if drive <= 0.0 {
        return x;
    }
    let g = 1.0 + drive * 15.0;
    match mode {
        // Normalised so a full-scale sine keeps its peak.
        KickDrive::Soft => (x * g).tanh() / g.tanh(),
        KickDrive::Hard => (x * g).clamp(-1.0, 1.0),
        KickDrive::Fold => (x * g * PI * 0.5).sin(),
        // The negative half saturates later than the positive one.
        KickDrive::Tube => {
            if x >= 0.0 { (x * g).tanh() / g.tanh() } else { (x * g * 0.5).tanh() / (g * 0.5).tanh() }
        }
        KickDrive::Crush => {
            let steps = 2f32.powf(12.0 - drive * 10.0);
            ((x * (1.0 + drive * 3.0)).clamp(-1.0, 1.0) * steps).round() / steps
        }
    }
}

/// The body oscillator at `phase` (0..1), aligned so every wave starts like a sine.
fn wave(kind: KickWave, phase: f32, inc: f32) -> f32 {
    match kind {
        KickWave::Sine => (phase * TAU).sin(),
        KickWave::Triangle => 1.0 - 4.0 * (((phase + 0.25) % 1.0) - 0.5).abs(),
        KickWave::Square => square(phase, inc),
        KickWave::Saw => saw((phase + 0.5) % 1.0, inc),
    }
}

#[derive(Clone, Copy)]
struct Click {
    seed: u32,
    filter: Svf,
    phases: [f32; 3],
    /// Position in a click sample, in its frames.
    position: f64,
}

impl Click {
    const NEW: Click = Click { seed: 0x9E37_79B9, filter: Svf { ic1: 0.0, ic2: 0.0 }, phases: [0.0; 3], position: 0.0 };

    fn noise(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    fn sine(&mut self, index: usize, freq: f32, sr: f32) -> f32 {
        let value = (self.phases[index] * TAU).sin();
        self.phases[index] = (self.phases[index] + freq / sr).fract();
        value
    }

    /// One sample of synthesized click `kind`, `n` samples into the hit.
    fn synth(&mut self, kind: u8, n: u32, tone: f32, frames: f32, sr: f32) -> f32 {
        let n_f = n as f32;
        if n_f > frames * 12.0 {
            return 0.0;
        }
        let env = |scale: f32| (-n_f / (frames * scale).max(1.0)).exp();
        let g = svf_g(tone, sr);
        match kind {
            // Band-passed noise.
            0 => {
                let x = self.noise();
                self.filter.tick(x, g, 1.2).1 * env(1.0) * 2.5
            }
            // High-passed noise, very short.
            1 => {
                let x = self.noise();
                self.filter.tick(x, g, 1.4).2 * env(0.5) * 1.5
            }
            // Sine blip.
            2 => self.sine(0, tone, sr) * env(1.0),
            // Square blip an octave down.
            3 => {
                let s = if self.sine(0, tone * 0.5, sr) >= 0.0 { 1.0 } else { -1.0 };
                s * env(1.0) * 0.6
            }
            // Sine whose pitch dives into place.
            4 => {
                let e = env(1.0);
                self.sine(0, tone * (1.0 + 3.0 * e), sr) * e
            }
            // Three inharmonic partials.
            5 => {
                let e = env(1.0);
                (self.sine(0, tone, sr) + self.sine(1, tone * 1.47, sr) + self.sine(2, tone * 2.09, sr)) * e * 0.4
            }
            // Low, longer noise.
            6 => {
                let x = self.noise();
                self.filter.tick(x, svf_g(tone * 0.25, sr), 1.0).0 * env(2.0) * 3.0
            }
            // An impulse ringing a filter.
            7 => {
                let x = if n == 0 { 1.0 } else { 0.0 };
                self.filter.tick(x, g, 0.3).1 * 4.0
            }
            // A sharper, more resonant knock.
            8 => {
                let x = if n == 0 { 1.0 } else { 0.0 };
                self.filter.tick(x, g, 0.08).1 * env(1.0) * 2.0
            }
            // Bright breath.
            9 => {
                let x = self.noise();
                self.filter.tick(x, g, 1.4).2 * env(3.0) * 0.8
            }
            // FM: the modulation index fades with the click.
            10 => {
                let e = env(1.0);
                let modulator = self.sine(1, tone * 2.7, sr);
                let value = ((self.phases[0] + modulator * 4.0 * e / TAU) * TAU).sin();
                self.phases[0] = (self.phases[0] + tone / sr).fract();
                value * e
            }
            // Sparse random ticks through a band-pass.
            _ => {
                let x = if self.noise() > 0.94 { self.noise().signum() } else { 0.0 };
                self.filter.tick(x, g, 0.6).1 * env(2.0) * 3.0
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Hit {
    active: bool,
    /// Samples since the hit started.
    elapsed: u32,
    phase: f32,
    sub_phase: f32,
    level: f32,
    note: f32,
    click: Click,
    fade: f32,
    fading: bool,
    /// DC blocker after the (possibly asymmetric) distortion.
    dc: (f32, f32),
}

impl Hit {
    const IDLE: Hit = Hit {
        active: false,
        elapsed: 0,
        phase: 0.0,
        sub_phase: 0.0,
        level: 0.0,
        note: 0.0,
        click: Click::NEW,
        fade: 1.0,
        fading: false,
        dc: (0.0, 0.0),
    };
}

pub struct KickSynth {
    params: KickParams,
    sample_rate: f32,
    tempo: f64,
    /// The sounding hit and the one fading out after a retrigger.
    hits: [Hit; 2],
    eq: [Svf; 3],
    punch_db: f32,
    lookahead: [f32; MAX_LOOKAHEAD],
    lookahead_len: usize,
    lookahead_pos: usize,
    limiter_gain: f32,
}

impl KickSynth {
    pub fn new(params: KickParams, sample_rate: f32) -> Self {
        let lookahead_len = ((LOOKAHEAD_SECONDS * sample_rate) as usize).clamp(1, MAX_LOOKAHEAD);
        Self {
            params,
            sample_rate,
            tempo: 120.0,
            hits: [Hit::IDLE; 2],
            eq: [Svf::default(); 3],
            punch_db: 0.0,
            lookahead: [0.0; MAX_LOOKAHEAD],
            lookahead_len,
            lookahead_pos: 0,
            limiter_gain: 1.0,
        }
    }

    /// How late it sounds (samples): its limiter's lookahead, when on.
    pub fn latency(&self) -> usize {
        if self.params.limiter { self.lookahead_len } else { 0 }
    }

    pub fn params(&self) -> KickParams {
        self.params
    }

    pub fn set_params(&mut self, params: KickParams) {
        self.params = params;
    }

    pub fn set_tempo(&mut self, tempo: f64) {
        self.tempo = tempo;
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32) {
        // The current hit becomes the fading one.
        if self.hits[0].active {
            self.hits[1] = self.hits[0];
            self.hits[1].fading = true;
        }
        let p = self.params;
        let note = if p.key_follow { pitch as f32 - 24.0 } else { p.tune };
        let level = 1.0 - p.velocity.clamp(0.0, 1.0) * (1.0 - velocity.clamp(0.0, 1.0));
        let start = (p.phase / 360.0).rem_euclid(1.0);
        let mut click = Click::NEW;
        click.seed = self.hits[0].click.seed ^ 0x5bd1_e995;
        self.hits[0] = Hit { active: true, level, note, phase: start, sub_phase: start, click, ..Hit::IDLE };
    }

    /// Its click noise from the start again: each hit's comes from the last.
    pub fn reseed(&mut self) {
        self.hits[0].click.seed = Click::NEW.seed;
    }

    pub fn all_notes_off(&mut self) {
        for hit in &mut self.hits {
            if hit.active {
                hit.fading = true;
            }
        }
    }

    pub fn render(&mut self, samples: &[Arc<Sample>], left: &mut [f32], right: &mut [f32]) {
        let mut scratch = [0f32; BLOCK];
        let mut offset = 0;
        while offset < left.len() {
            let n = (left.len() - offset).min(BLOCK);
            scratch[..n].fill(0.0);
            self.render_hits(samples, &mut scratch[..n]);
            self.process(&mut scratch[..n]);
            for i in 0..n {
                left[offset + i] += scratch[i];
                right[offset + i] += scratch[i];
            }
            offset += n;
        }
    }

    fn render_hits(&mut self, samples: &[Arc<Sample>], out: &mut [f32]) {
        let p = effective(&self.params, self.tempo);
        let sr = self.sample_rate;
        let ms_per_sample = 1000.0 / sr;
        let length = length_ms(&p);
        let fade_step = 1.0 / (RETRIGGER_FADE_SECONDS * sr);
        let click_sample = samples.first().filter(|s| s.frames() > 0);
        let click_tone = p.click_tone * semitones(p.click_pitch);
        let click_frames = p.click_length.max(0.5) * sr / 1000.0;
        let dc_coef = 1.0 - 20.0 * TAU / sr;
        let sub_ratio = semitones(p.sub_tune);
        for hit in self.hits.iter_mut().filter(|h| h.active) {
            let base = note_freq(hit.note);
            for sample in out.iter_mut() {
                let ms = hit.elapsed as f32 * ms_per_sample;
                if ms >= length || (hit.fading && hit.fade <= 0.0) {
                    hit.active = false;
                    break;
                }
                let sweep = pitch_offset(&p, ms);
                let amp = amplitude(&p, ms);

                // Body: the chosen wave (bent towards a triangle by Harmonics), swept to the note.
                let inc = base * semitones(sweep) / sr;
                let raw = wave(p.wave, hit.phase, inc);
                let triangle = wave(KickWave::Triangle, hit.phase, inc);
                let body = raw + (triangle * 1.4 - raw) * p.harmonics;
                hit.phase = (hit.phase + inc).fract();
                let mut body = distort(body * amp, p.drive_mode, drive_at(&p, ms));
                if p.drive_mode == KickDrive::Tube {
                    let (x1, y1) = hit.dc;
                    let y = body - x1 + dc_coef * y1;
                    hit.dc = (body, y);
                    body = y;
                }

                // Sub: a clean sine moving in lockstep with the body (same sweep, fixed
                // interval), so it reinforces the fundamental instead of beating against it.
                let value = (hit.sub_phase * TAU).sin();
                hit.sub_phase = (hit.sub_phase + inc * sub_ratio).fract();
                let sub = value * amp * p.sub_level;

                let click = match click_sample {
                    Some(sample) => {
                        let (l, r) = sample.read(hit.click.position);
                        hit.click.position += (sample.sample_rate / sr * semitones(p.click_pitch)) as f64;
                        (l + r) * 0.5
                    }
                    None => hit.click.synth(p.click_type, hit.elapsed, click_tone, click_frames, sr),
                };

                let mut value = (body + sub + click * p.click) * p.gain * hit.level * OUTPUT;
                if hit.fading {
                    hit.fade -= fade_step;
                    value *= hit.fade.max(0.0);
                }
                *sample += value;
                hit.elapsed += 1;
            }
        }
    }

    /// EQ, punch compressor and limiter on the mixed hits.
    fn process(&mut self, out: &mut [f32]) {
        let p = self.params;
        let sr = self.sample_rate;
        let eq_on = p.eq_low != 0.0 || p.eq_mid != 0.0 || p.eq_high != 0.0;
        let (g_low, g_high) = (svf_g(120.0, sr), svf_g(4000.0, sr));
        let g_mid = svf_g(p.eq_mid_freq, sr);
        let (low_gain, mid_gain, high_gain) = (db_to_gain(p.eq_low), db_to_gain(p.eq_mid), db_to_gain(p.eq_high));
        let butterworth = std::f32::consts::SQRT_2;
        // Punch: a fast 4:1 compressor with makeup gain. The peak stays about where
        // it was while the body and the tail come up, so the kick sounds denser.
        let threshold = -p.punch * 24.0;
        let makeup = db_to_gain(p.punch * 12.0);
        let attack = (-1.0 / (0.0001 * sr)).exp();
        let release = (-1.0 / (0.080 * sr)).exp();
        let limiter_release = (-1.0 / (0.050 * sr)).exp();
        for x in out.iter_mut() {
            let mut y = *x;
            if eq_on {
                let low = self.eq[0].tick(y, g_low, butterworth).0;
                let high = self.eq[1].tick(y, g_high, butterworth).2;
                y = low * low_gain + (y - low - high) + high * high_gain;
                let band = self.eq[2].tick(y, g_mid, 1.0).1;
                y += (mid_gain - 1.0) * band;
            }
            if p.punch > 0.0 {
                let level = 20.0 * y.abs().max(1e-6).log10();
                let target = ((level - threshold).max(0.0)) * (1.0 / 4.0 - 1.0);
                let coef = if target < self.punch_db { attack } else { release };
                self.punch_db = target + coef * (self.punch_db - target);
                y *= db_to_gain(self.punch_db) * makeup;
            }
            if p.limiter {
                let len = self.lookahead_len;
                let delayed = self.lookahead[self.lookahead_pos];
                self.lookahead[self.lookahead_pos] = y;
                self.lookahead_pos = (self.lookahead_pos + 1) % len;
                let peak = self.lookahead[..len].iter().fold(delayed.abs(), |m, s| m.max(s.abs()));
                let target = if peak > CEILING { CEILING / peak } else { 1.0 };
                self.limiter_gain = if target < self.limiter_gain { target } else { target + (self.limiter_gain - target) * limiter_release };
                y = (delayed * self.limiter_gain).clamp(-CEILING, CEILING);
            }
            *x = y;
        }
    }
}

/// Renders one hit (mono) at `tempo`, for previews, displays and export.
pub fn render_kick(params: KickParams, click: Option<Arc<Sample>>, sample_rate: f32, tempo: f64) -> Vec<f32> {
    let mut kick = KickSynth::new(params, sample_rate);
    kick.set_tempo(tempo);
    kick.note_on(60, 1.0);
    let length = length_ms(&effective(&params, tempo));
    let frames = ((length / 1000.0 + 0.01) * sample_rate) as usize;
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    let samples: Vec<Arc<Sample>> = click.into_iter().collect();
    for (l, r) in left.chunks_mut(512).zip(right.chunks_mut(512)) {
        kick.render(&samples, l, r);
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Frequency from the zero crossings in a window of a rendered hit.
    fn frequency(signal: &[f32], from: usize, to: usize) -> f32 {
        let crossings = signal[from..to].windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        crossings as f32 * SR / (to - from) as f32
    }

    #[test]
    fn the_body_sweeps_down_to_the_tuned_note() {
        let params = KickParams { click: 0.0, drive: 0.0, tune: 33.0, hold: 1000.0, ..Default::default() };
        let hit = render_kick(params, None, SR, 120.0);
        // Zero crossings over a second give the frequency to within 1 Hz.
        let early = frequency(&hit, 0, 480);
        let late = frequency(&hit, 9_600, 57_600);
        let target = note_freq(33.0);
        assert!(early > target * 2.5, "starts high: {early} Hz");
        assert!((late - target).abs() < 3.0, "settles at {target:.1} Hz, got {late}");
    }

    #[test]
    fn the_hit_lasts_attack_hold_and_decay() {
        let params = KickParams { attack: 1.0, hold: 50.0, decay: 100.0, ..Default::default() };
        let hit = render_kick(params, None, SR, 120.0);
        let at = |ms: f32| hit[(ms * SR / 1000.0) as usize].abs();
        let window_peak = |from_ms: f32, to_ms: f32| {
            hit[(from_ms * 48.0) as usize..(to_ms * 48.0) as usize].iter().fold(0f32, |m, s| m.max(s.abs()))
        };
        assert!(window_peak(30.0, 50.0) > 0.35, "full level while holding");
        assert!(window_peak(140.0, 151.0) < 0.05, "faded by the end of the decay");
        assert_eq!(at(152.0), 0.0);
    }

    #[test]
    fn key_follow_tunes_to_the_played_note() {
        let params = KickParams { key_follow: true, pitch_amount: 0.0, click: 0.0, drive: 0.0, hold: 1500.0, ..Default::default() };
        let mut kick = KickSynth::new(params, SR);
        kick.note_on(60, 1.0);
        let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        kick.render(&[], &mut l, &mut r);
        // Note 60 plays the body two octaves down, at note 36.
        let found = frequency(&l, 0, 48_000);
        assert!((found - note_freq(36.0)).abs() < 1.5, "{found} Hz");
    }

    #[test]
    fn a_retrigger_fades_the_previous_hit_without_a_jump() {
        let params = KickParams { click: 0.0, ..Default::default() };
        let mut kick = KickSynth::new(params, SR);
        kick.note_on(60, 1.0);
        let (mut l, mut r) = (vec![0.0; 2000], vec![0.0; 2000]);
        kick.render(&[], &mut l[..1000], &mut r[..1000]);
        kick.note_on(60, 1.0);
        kick.render(&[], &mut l[1000..], &mut r[1000..]);
        let jump = l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(jump < 0.5, "largest step {jump}");
        assert!(l.iter().all(|s| s.is_finite() && s.abs() < 2.0));
    }

    #[test]
    fn every_preset_and_drive_mode_stays_in_range() {
        for (name, preset) in KickParams::presets() {
            for mode in KickDrive::ALL {
                let hit = render_kick(KickParams { drive_mode: mode, ..preset }, None, SR, 120.0);
                let peak = hit.iter().fold(0f32, |m, s| m.max(s.abs()));
                assert!(peak > 0.2 && peak < 1.0, "{name} {mode:?}: peak {peak}");
            }
        }
    }

    #[test]
    fn the_808_glide_keeps_gliding_after_the_hit() {
        let (_, p) = KickParams::presets().into_iter().find(|(n, _)| *n == "808 · Glide").unwrap();
        // Still well above its note a quarter of a second in, and on it once the sweep is over.
        assert!(pitch_offset(&p, 250.0) > 10.0 && pitch_offset(&p, 250.0) < pitch_offset(&p, 30.0));
        assert!(pitch_offset(&p, p.pitch_time + 20.0).abs() < 0.01);
    }

    #[test]
    fn a_click_sample_replaces_the_synthesized_click() {
        let click = Arc::new(Sample::from_channels(vec![0.9; 100], vec![0.9; 100], SR));
        let params = KickParams { click: 1.0, pitch_amount: 0.0, drive: 0.0, attack: 20.0, ..Default::default() };
        let hit = render_kick(params, Some(click), SR, 120.0);
        let body = render_kick(KickParams { click: 0.0, ..params }, None, SR, 120.0);
        assert!((hit[50] - body[50] - 0.9 * params.gain * OUTPUT).abs() < 1e-3, "the sample is the click: {}", hit[50] - body[50]);
        assert_eq!(hit[150], body[150], "the click sample is over");
    }

    #[test]
    fn drawn_shapes_drive_the_envelopes() {
        // Level dips to silence half way through the decay and comes back: a double hit.
        let amp_shape = Shape::from_points(&[(0.0, 1.0, 0.0), (0.4, 0.0, 0.0), (0.5, 0.0, 0.0), (0.6, 1.0, 0.0), (1.0, 1.0, 0.0)]);
        let params = KickParams { amp_shape, attack: 0.1, hold: 0.0, decay: 200.0, decay_curve: 0.0, click: 0.0, ..Default::default() };
        let hit = render_kick(params, None, SR, 120.0);
        let peak = |from_ms: f32, to_ms: f32| hit[(from_ms * 48.0) as usize..(to_ms * 48.0) as usize].iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak(88.0, 92.0) < 0.05, "silent in the dip");
        assert!(peak(150.0, 190.0) > 0.3, "back up after it");
        assert!(hit.last().unwrap().abs() < 0.01, "a curve ending high still fades out");

        // A pitch shape that stays at half the amount keeps the body an octave up when Pitch is 24.
        let pitch_shape = Shape::from_points(&[(0.0, 1.0, 0.0), (1.0, 0.5, 0.0)]);
        let params = KickParams { pitch_shape, pitch_amount: 24.0, pitch_curve: 0.0, pitch_time: 10.0, hold: 1000.0, click: 0.0, drive: 0.0, ..Default::default() };
        let hit = render_kick(params, None, SR, 120.0);
        let found = frequency(&hit, 4_800, 52_800);
        assert!((found - note_freq(params.tune + 12.0)).abs() < 1.5, "{found} Hz");
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0f32, |m, s| m.max(s.abs()))
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// Energy below `cutoff` Hz, via a one-pole low-pass.
    fn low_energy(x: &[f32], cutoff: f32) -> f32 {
        let a = (-TAU * cutoff / SR).exp();
        let mut y = 0.0;
        let low: Vec<f32> = x.iter().map(|&s| { y = s + a * (y - s); y }).collect();
        rms(&low)
    }

    fn quiet() -> KickParams {
        KickParams { click: 0.0, drive: 0.0, ..Default::default() }
    }

    #[test]
    fn every_click_sounds_and_dies_away() {
        for (index, name) in crate::model::KICK_CLICKS.iter().enumerate() {
            let params = KickParams { click: 1.0, click_type: index as u8, gain: 1.0, attack: 20.0, hold: 0.0, decay: 300.0, ..Default::default() };
            let with = render_kick(params, None, SR, 120.0);
            let without = render_kick(KickParams { click: 0.0, ..params }, None, SR, 120.0);
            let click: Vec<f32> = with.iter().zip(&without).map(|(a, b)| a - b).collect();
            assert!(peak(&click[..480]) > 0.05, "{name} is audible: {}", peak(&click[..480]));
            assert!(peak(&click[9600..]) < 0.01, "{name} is over after 200 ms: {}", peak(&click[9600..]));
            assert!(click.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn the_sub_adds_clean_low_end() {
        let body = render_kick(quiet(), None, SR, 120.0);
        let with_sub = render_kick(KickParams { sub_level: 1.0, ..quiet() }, None, SR, 120.0);
        assert!(low_energy(&with_sub, 80.0) > low_energy(&body, 80.0) * 1.5);
    }

    #[test]
    fn eq_bands_change_their_range() {
        let flat = render_kick(KickParams { harmonics: 0.6, ..quiet() }, None, SR, 120.0);
        let boosted = render_kick(KickParams { harmonics: 0.6, eq_low: 12.0, ..quiet() }, None, SR, 120.0);
        assert!(low_energy(&boosted, 100.0) > low_energy(&flat, 100.0) * 2.0);
        // A bright click shows the high band.
        let bright = KickParams { click: 1.0, click_type: 1, click_tone: 9000.0, ..quiet() };
        let highs = |x: &[f32]| {
            let low_part = { let a = (-TAU * 6000.0 / SR).exp(); let mut y = 0.0; x.iter().map(|&s| { y = s + a * (y - s); s - y }).collect::<Vec<_>>() };
            rms(&low_part[..2400])
        };
        let flat = render_kick(bright, None, SR, 120.0);
        let cut = render_kick(KickParams { eq_high: -12.0, ..bright }, None, SR, 120.0);
        assert!(highs(&cut) < highs(&flat) * 0.5, "{} vs {}", highs(&cut), highs(&flat));
    }

    #[test]
    fn punch_raises_the_body_and_the_limiter_holds_the_ceiling() {
        let dry = render_kick(quiet(), None, SR, 120.0);
        let punched = render_kick(KickParams { punch: 1.0, ..quiet() }, None, SR, 120.0);
        assert!(rms(&punched[4800..]) > rms(&dry[4800..]) * 1.2, "the tail comes up");

        let hot = KickParams { gain: 1.0, drive: 1.0, drive_mode: KickDrive::Hard, click: 1.0, eq_low: 12.0, punch: 1.0, sub_level: 1.0, ..Default::default() };
        assert!(peak(&render_kick(hot, None, SR, 120.0)) > 1.0, "too hot without the limiter");
        let limited = render_kick(KickParams { limiter: true, ..hot }, None, SR, 120.0);
        assert!(peak(&limited) <= CEILING + 1e-6, "{}", peak(&limited));
    }

    #[test]
    fn synced_length_follows_the_tempo() {
        let params = KickParams { sync_beats: 1.0, ..Default::default() };
        assert_eq!(length_ms(&effective(&params, 120.0)), 500.0);
        assert_eq!(length_ms(&effective(&params, 150.0)), 400.0);
        let hit = render_kick(params, None, SR, 120.0);
        assert!((hit.len() as f32 / SR - 0.51).abs() < 0.002);
    }

    #[test]
    fn velocity_sensitivity_and_start_phase() {
        let level = |velocity_amount: f32, velocity: f32| {
            let mut kick = KickSynth::new(KickParams { velocity: velocity_amount, ..quiet() }, SR);
            kick.note_on(60, velocity);
            let (mut l, mut r) = (vec![0.0; 4800], vec![0.0; 4800]);
            kick.render(&[], &mut l, &mut r);
            peak(&l)
        };
        assert!((level(1.0, 0.5) / level(1.0, 1.0) - 0.5).abs() < 0.01);
        assert!((level(0.0, 0.2) / level(0.0, 1.0) - 1.0).abs() < 1e-4, "no sensitivity ignores velocity");

        // At 90 degrees the body starts at its peak instead of at zero.
        let at_peak = render_kick(KickParams { phase: 90.0, attack: 0.05, ..quiet() }, None, SR, 120.0);
        let at_zero = render_kick(KickParams { phase: 0.0, attack: 0.05, ..quiet() }, None, SR, 120.0);
        assert!(at_peak[3].abs() > 0.3 && at_zero[3].abs() < 0.2, "{} {}", at_peak[3], at_zero[3]);
    }

    #[test]
    fn the_drive_envelope_shapes_the_distortion() {
        // Drive only in the first tenth: later the body is a clean sine again.
        let drive_shape = Shape::from_points(&[(0.0, 1.0, 0.0), (0.1, 1.0, 0.0), (0.12, 0.0, 0.0), (1.0, 0.0, 0.0)]);
        let params = KickParams { drive: 1.0, drive_mode: KickDrive::Hard, drive_shape, click: 0.0, pitch_amount: 0.0, hold: 400.0, ..Default::default() };
        let hit = render_kick(params, None, SR, 120.0);
        let clean = render_kick(KickParams { drive: 0.0, ..params }, None, SR, 120.0);
        let late = 9600..14_400;
        let difference: f32 = hit[late.clone()].iter().zip(&clean[late]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(difference < 1e-3, "no drive after the envelope drops: {difference}");
        assert!(rms(&hit[..1000]) > rms(&clean[..1000]) * 1.2, "driven at the start");
    }

    #[test]
    fn every_wave_and_drive_mode_is_stable() {
        for wave in KickWave::ALL {
            for mode in KickDrive::ALL {
                let hit = render_kick(KickParams { wave, drive_mode: mode, drive: 0.7, harmonics: 0.5, sub_level: 0.5, ..Default::default() }, None, SR, 120.0);
                assert!(hit.iter().all(|s| s.is_finite() && s.abs() < 2.0), "{wave:?} {mode:?}");
                assert!(peak(&hit) > 0.1);
            }
        }
    }
}
