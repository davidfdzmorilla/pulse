//! Kick drum synthesizer: a tuned sine body swept down by a pitch envelope, shaped
//! by an attack-hold-decay envelope, with a click layer (synthesized or a sample)
//! and distortion. Monophonic: a new hit fades the previous one out quickly.

use crate::instruments::{Svf, semitones, svf_g};
use crate::model::{KickDrive, KickParams};
use crate::sample::Sample;
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

/// Output scale that leaves headroom for the click on top of a full-scale body.
const OUTPUT: f32 = 0.7;
/// Fade applied to a hit cut off by the next one.
const RETRIGGER_FADE_SECONDS: f32 = 0.003;

fn note_freq(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// Power curve used by both envelopes: `x` falls from 1 to 0 as `t` goes 0..1;
/// `curve` 0 is linear, 1 drops fast and lingers.
fn falling(t: f32, curve: f32) -> f32 {
    (1.0 - t.clamp(0.0, 1.0)).powf(1.0 + curve.clamp(0.0, 1.0) * 6.0)
}

/// Pitch offset in semitones `ms` milliseconds into a hit.
pub fn pitch_offset(p: &KickParams, ms: f32) -> f32 {
    p.pitch_amount * falling(ms / p.pitch_time.max(0.1), p.pitch_curve)
}

/// Amplitude envelope `ms` milliseconds into a hit.
pub fn amplitude(p: &KickParams, ms: f32) -> f32 {
    let attack = p.attack.max(0.05);
    if ms < attack {
        ms / attack
    } else if ms < attack + p.hold {
        1.0
    } else {
        falling((ms - attack - p.hold) / p.decay.max(1.0), p.decay_curve)
    }
}

/// Total length of a hit in milliseconds.
pub fn length_ms(p: &KickParams) -> f32 {
    p.attack.max(0.05) + p.hold + p.decay.max(1.0)
}

fn distort(x: f32, mode: KickDrive, drive: f32) -> f32 {
    if drive <= 0.0 {
        return x;
    }
    let g = 1.0 + drive * 15.0;
    match mode {
        // Normalised so a full-scale sine keeps its peak.
        KickDrive::Soft => (x * g).tanh() / g.tanh(),
        KickDrive::Hard => (x * g).clamp(-1.0, 1.0),
        KickDrive::Fold => (x * g * PI * 0.5).sin(),
    }
}

#[derive(Clone, Copy)]
struct Hit {
    active: bool,
    /// Samples since the hit started.
    elapsed: u32,
    phase: f32,
    velocity: f32,
    note: f32,
    /// Position in the click sample, in sample frames.
    click_position: f64,
    fade: f32,
    fading: bool,
    seed: u32,
    click_filter: Svf,
}

impl Hit {
    const IDLE: Hit = Hit {
        active: false,
        elapsed: 0,
        phase: 0.0,
        velocity: 0.0,
        note: 0.0,
        click_position: 0.0,
        fade: 1.0,
        fading: false,
        seed: 0x9E37_79B9,
        click_filter: Svf { ic1: 0.0, ic2: 0.0 },
    };

    fn noise(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

pub struct KickSynth {
    params: KickParams,
    sample_rate: f32,
    /// The sounding hit and the one fading out after a retrigger.
    hits: [Hit; 2],
}

impl KickSynth {
    pub fn new(params: KickParams, sample_rate: f32) -> Self {
        Self { params, sample_rate, hits: [Hit::IDLE; 2] }
    }

    pub fn params(&self) -> KickParams {
        self.params
    }

    pub fn set_params(&mut self, params: KickParams) {
        self.params = params;
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32) {
        // The current hit becomes the fading one.
        if self.hits[0].active {
            self.hits[1] = self.hits[0];
            self.hits[1].fading = true;
        }
        let note = if self.params.key_follow { pitch as f32 - 24.0 } else { self.params.tune };
        self.hits[0] = Hit { active: true, velocity, note, seed: self.hits[0].seed ^ 0x5bd1_e995, ..Hit::IDLE };
    }

    pub fn all_notes_off(&mut self) {
        for hit in &mut self.hits {
            if hit.active {
                hit.fading = true;
            }
        }
    }

    pub fn render(&mut self, samples: &[Arc<Sample>], left: &mut [f32], right: &mut [f32]) {
        let p = self.params;
        let sr = self.sample_rate;
        let ms_per_sample = 1000.0 / sr;
        let length = length_ms(&p);
        let fade_step = 1.0 / (RETRIGGER_FADE_SECONDS * sr);
        let click_sample = samples.first().filter(|s| s.frames() > 0);
        let click_g = svf_g(p.click_tone, sr);
        let click_frames = (p.click_length.max(0.5) * sr / 1000.0) as u32;
        for hit in self.hits.iter_mut().filter(|h| h.active) {
            let base = note_freq(hit.note);
            for (out_l, out_r) in left.iter_mut().zip(right.iter_mut()) {
                let ms = hit.elapsed as f32 * ms_per_sample;
                if ms >= length || (hit.fading && hit.fade <= 0.0) {
                    hit.active = false;
                    break;
                }
                // Body: a sine (bent towards a triangle by Harmonics) swept down to the note.
                let freq = base * semitones(pitch_offset(&p, ms));
                let sine = (hit.phase * TAU).sin();
                let triangle = 1.0 - 4.0 * (hit.phase - 0.5).abs();
                let body = sine + (triangle * 1.4 - sine) * p.harmonics;
                hit.phase = (hit.phase + freq / sr).fract();
                let body = distort(body * amplitude(&p, ms), p.drive_mode, p.drive);

                // Click: the sample if there is one, else filtered noise with a quick decay.
                let click = match click_sample {
                    Some(sample) => {
                        let (l, r) = sample.read(hit.click_position);
                        hit.click_position += sample.sample_rate as f64 / sr as f64;
                        (l + r) * 0.5
                    }
                    None if hit.elapsed < click_frames * 4 => {
                        let envelope = (-(hit.elapsed as f32) / click_frames.max(1) as f32).exp();
                        let noise = hit.noise();
                        let (_, band, _) = hit.click_filter.tick(noise, click_g, 1.2);
                        band * envelope * 2.5
                    }
                    None => 0.0,
                };

                let mut out = (body + click * p.click) * p.gain * hit.velocity * OUTPUT;
                if hit.fading {
                    hit.fade -= fade_step;
                    out *= hit.fade.max(0.0);
                }
                *out_l += out;
                *out_r += out;
                hit.elapsed += 1;
            }
        }
    }
}

/// Renders one hit (mono) for previews and displays.
pub fn render_kick(params: KickParams, click: Option<Arc<Sample>>, sample_rate: f32) -> Vec<f32> {
    let mut kick = KickSynth::new(params, sample_rate);
    kick.note_on(60, 1.0);
    let frames = ((length_ms(&params) / 1000.0 + 0.01) * sample_rate) as usize;
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
        let hit = render_kick(params, None, SR);
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
        let hit = render_kick(params, None, SR);
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
                let hit = render_kick(KickParams { drive_mode: mode, ..preset }, None, SR);
                let peak = hit.iter().fold(0f32, |m, s| m.max(s.abs()));
                assert!(peak > 0.2 && peak < 1.0, "{name} {mode:?}: peak {peak}");
            }
        }
    }

    #[test]
    fn a_click_sample_replaces_the_synthesized_click() {
        let click = Arc::new(Sample::from_channels(vec![0.9; 100], vec![0.9; 100], SR));
        let params = KickParams { click: 1.0, pitch_amount: 0.0, drive: 0.0, attack: 20.0, ..Default::default() };
        let hit = render_kick(params, Some(click), SR);
        let body = render_kick(KickParams { click: 0.0, ..params }, None, SR);
        assert!((hit[50] - body[50] - 0.9 * params.gain * OUTPUT).abs() < 1e-3, "the sample is the click: {}", hit[50] - body[50]);
        assert_eq!(hit[150], body[150], "the click sample is over");
    }
}
