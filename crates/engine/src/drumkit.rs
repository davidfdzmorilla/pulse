//! The drum machine's 808 and 909 models: each sound built as those machines
//! built it.
//!
//! - TR-808: a bridged-T resonator for the kick (a long, round sine that drops
//!   a little in pitch), two tuned resonators and filtered noise for the snare,
//!   four quick noise bursts and a tail through a band-pass for the clap, six
//!   square oscillators through two band-passes and a high-pass for the hats, two
//!   squares at 540 and 800 Hz for the cowbell.
//! - TR-909: a kick whose pitch falls fast from high, with a click, driven; a
//!   brighter snare with more noise; hats brighter and noisier.
//!
//! - Both: three toms; a crash and a ride from a second, higher set of squares
//!   with noise; a shaker of band-passed noise; a tambourine, jingles in three
//!   quick strokes; a conga, a sine that drops a touch; a clave, a short high
//!   sine. The classic machine plays these eight as the 808 does.
//!
//! The sounds and their order are the drum machine's (`DRUM_VOICES`), and so
//! are its tune, decay and level.

use crate::model::{DrumModel, DrumParams, DRUM_SOUNDS};
use std::f32::consts::TAU;

/// A biquad filter (RBJ's cookbook), its coefficients set when a hit starts.
#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl Biquad {
    fn new(kind: u8, freq: f32, q: f32, sr: f32) -> Self {
        let w = TAU * (freq / sr).clamp(1e-4, 0.49);
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q.max(0.05));
        let a0 = 1.0 + alpha;
        let b = match kind {
            // Band-pass (constant peak gain).
            0 => [alpha, 0.0, -alpha],
            // High-pass.
            1 => [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0],
            // Low-pass.
            _ => [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0],
        };
        Self { b: b.map(|v| v / a0), a: [-2.0 * cos / a0, (1.0 - alpha) / a0], ..Default::default() }
    }

    fn band(freq: f32, q: f32, sr: f32) -> Self {
        Self::new(0, freq, q, sr)
    }

    fn high(freq: f32, q: f32, sr: f32) -> Self {
        Self::new(1, freq, q, sr)
    }

    fn low(freq: f32, q: f32, sr: f32) -> Self {
        Self::new(2, freq, q, sr)
    }

    fn tick(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// The six square oscillators of the 808's hats and cowbell (Hz).
const METAL: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
/// A higher, more clashing set for the cymbals and the tambourine's jingles.
const CYMBAL: [f32; 6] = [507.0, 751.9, 912.9, 1291.0, 1333.8, 1976.0];

/// The pitch each tom (low, mid, high) is tuned to, against the low one.
const TOM_PITCH: [(usize, f32); 3] = [(6, 1.0), (8, 1.4), (9, 1.85)];

/// One sound's state while it rings.
#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    /// Seconds since it was hit.
    t: f32,
    velocity: f32,
    /// The pitch against the sound's own (a low conga, 0.75).
    base: f32,
    phases: [f32; 2],
    filters: [Biquad; 3],
}

pub struct ModelKit {
    model: DrumModel,
    sample_rate: f32,
    voices: [Voice; DRUM_SOUNDS],
    metal: [f32; 6],
    cymbal: [f32; 6],
    seed: u32,
}

/// An exponential decay of `tau` seconds at time `t`.
fn decay(t: f32, tau: f32) -> f32 {
    (-t / tau.max(1e-4)).exp()
}

fn square(phase: f32) -> f32 {
    if phase < 0.5 { 1.0 } else { -1.0 }
}

impl ModelKit {
    pub fn new(model: DrumModel, sample_rate: f32) -> Self {
        Self { model, sample_rate, voices: [Voice::default(); DRUM_SOUNDS], metal: [0.0; 6], cymbal: [0.0; 6], seed: 0x1234_5679 }
    }

    pub fn set_model(&mut self, model: DrumModel) {
        self.model = model;
    }

    fn noise(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// Strikes sound `voice` (the drum machine's order), at `base` pitch.
    pub fn hit(&mut self, voice: usize, velocity: f32, base: f32, p: &DrumParams) {
        let sr = self.sample_rate;
        let tune = |v: usize| 2f32.powf(p.tune[v] / 12.0);
        let is_909 = self.model == DrumModel::Tr909;
        let filters = match voice {
            // Rim: a ring, high-passed.
            1 => [Biquad::high(if is_909 { 1200.0 } else { 600.0 } * tune(1), 0.7, sr), Biquad::default(), Biquad::default()],
            // Snare: the noise, high- and low-passed.
            2 => [
                Biquad::high(if is_909 { 2200.0 } else { 1800.0 } * tune(2), 0.7, sr),
                Biquad::low(if is_909 { 11_000.0 } else { 8000.0 }, 0.7, sr),
                Biquad::default(),
            ],
            // Clap: a band of noise.
            3 => [Biquad::band(if is_909 { 1300.0 } else { 1100.0 } * tune(3), 1.4, sr), Biquad::high(500.0, 0.7, sr), Biquad::default()],
            // Hats: two band-passes and a high-pass (808), or brighter (909).
            4 | 5 => {
                let t = tune(voice);
                if is_909 {
                    [Biquad::high(7800.0 * t, 0.8, sr), Biquad::band(10_000.0 * t, 1.2, sr), Biquad::default()]
                } else {
                    [Biquad::band(3440.0 * t, 1.5, sr), Biquad::band(7100.0 * t, 1.5, sr), Biquad::high(6500.0 * t, 0.7, sr)]
                }
            }
            // Toms: a breath of noise, low-passed.
            6 | 8 | 9 => {
                let pitch = TOM_PITCH.iter().find(|t| t.0 == voice).map_or(1.0, |t| t.1);
                [Biquad::low(900.0 * pitch * tune(voice), 0.7, sr), Biquad::default(), Biquad::default()]
            }
            // Cowbell: a band round its tones.
            7 => [Biquad::band(2640.0 * tune(7), 2.0, sr), Biquad::default(), Biquad::default()],
            // Crash: high-passed and given its sizzle band.
            10 => [Biquad::high(3800.0 * tune(10), 0.7, sr), Biquad::band(if is_909 { 9000.0 } else { 7500.0 } * tune(10), 0.7, sr), Biquad::default()],
            // Ride: a narrower ping and a little wash.
            11 => [Biquad::band(5200.0 * tune(11), 1.6, sr), Biquad::high(3000.0 * tune(11), 0.7, sr), Biquad::default()],
            // Shaker: a band of noise, high up.
            12 => [Biquad::band(7000.0 * tune(12), 1.2, sr), Biquad::high(4500.0 * tune(12), 0.7, sr), Biquad::default()],
            // Tambourine: the jingles, very high.
            13 => [Biquad::high(7000.0 * tune(13), 0.7, sr), Biquad::band(10_000.0 * tune(13), 1.0, sr), Biquad::default()],
            // Conga: the slap of the hand.
            14 => [Biquad::band(1800.0 * tune(14), 1.0, sr), Biquad::default(), Biquad::default()],
            _ => [Biquad::default(); 3],
        };
        if voice == 4 {
            // A closed hat chokes the open one, as on the machines.
            self.voices[5].active = false;
        }
        self.voices[voice] = Voice { active: true, t: 0.0, velocity, base, phases: [0.0; 2], filters };
    }

    pub fn render(&mut self, p: &DrumParams, left: &mut [f32], right: &mut [f32]) {
        let sr = self.sample_rate;
        let dt = 1.0 / sr;
        let tune = |v: usize| 2f32.powf(p.tune[v] / 12.0);
        let length = |v: usize, seconds: f32| seconds * p.decay[v].max(0.05);
        let is_909 = self.model == DrumModel::Tr909;
        let metal_on = self.voices[4].active || self.voices[5].active || self.voices[7].active;
        let cymbal_on = self.voices[10].active || self.voices[11].active || self.voices[13].active;
        for (out_l, out_r) in left.iter_mut().zip(right.iter_mut()) {
            let mut s = 0.0;
            let noise = self.noise();
            // The six squares, shared by hats and cowbell.
            let mut metal = [0f32; 6];
            if metal_on {
                for (i, phase) in self.metal.iter_mut().enumerate() {
                    *phase = (*phase + METAL[i] * dt) % 1.0;
                    metal[i] = square(*phase);
                }
            }
            let mut cymbal = 0.0;
            if cymbal_on {
                for (i, phase) in self.cymbal.iter_mut().enumerate() {
                    *phase = (*phase + CYMBAL[i] * dt) % 1.0;
                    cymbal += square(*phase);
                }
                cymbal /= 6.0;
            }

            // Kick.
            let v = &mut self.voices[0];
            if v.active {
                let t = v.t;
                let out = if is_909 {
                    // From about 230 Hz down to 50 in a few tens of milliseconds, driven.
                    let freq = (50.0 + 180.0 * decay(t, 0.012) + 25.0 * decay(t, 0.06)) * tune(0);
                    v.phases[0] = (v.phases[0] + freq * dt) % 1.0;
                    let body = ((v.phases[0] * TAU).sin() * 2.2).tanh() * decay(t, length(0, 0.32));
                    let click = if t < 0.004 { noise * 0.6 * (1.0 - t / 0.004) + if t < 0.0008 { 0.8 } else { 0.0 } } else { 0.0 };
                    body + click
                } else {
                    // A round 49 Hz sine, a touch higher at the start, ringing long.
                    let freq = 49.0 * tune(0) * (1.0 + 0.18 * decay(t, 0.01));
                    v.phases[0] = (v.phases[0] + freq * dt) % 1.0;
                    let body = ((v.phases[0] * TAU).sin() * 1.3).tanh() * decay(t, length(0, 0.4));
                    let click = if t < 0.001 { 0.7 } else { 0.0 };
                    body + click
                };
                s += out * v.velocity * p.level[0] * 1.1;
                v.t += dt;
                v.active = v.t < length(0, if is_909 { 0.32 } else { 0.4 }) * 7.0;
            }

            // Rim: two tones rung very short.
            let v = &mut self.voices[1];
            if v.active {
                let t = v.t;
                let (f1, f2) = if is_909 { (1800.0, 520.0) } else { (1667.0, 455.0) };
                v.phases[0] = (v.phases[0] + f1 * tune(1) * dt) % 1.0;
                v.phases[1] = (v.phases[1] + f2 * tune(1) * dt) % 1.0;
                let tone = (v.phases[0] * TAU).sin() * 0.7 + (v.phases[1] * TAU).sin() * 0.5;
                let ring = v.filters[0].tick(tone + noise * 0.2) * decay(t, length(1, if is_909 { 0.006 } else { 0.009 }));
                s += ring * v.velocity * p.level[1] * 1.4;
                v.t += dt;
                v.active = v.t < length(1, 0.01) * 8.0;
            }

            // Snare: two tones and the snappy noise.
            let v = &mut self.voices[2];
            if v.active {
                let t = v.t;
                let (f1, f2, sweep) = if is_909 { (185.0, 330.0, 0.15) } else { (238.0, 476.0, 0.0) };
                let drop = 1.0 + sweep * decay(t, 0.01);
                v.phases[0] = (v.phases[0] + f1 * tune(2) * drop * dt) % 1.0;
                v.phases[1] = (v.phases[1] + f2 * tune(2) * drop * dt) % 1.0;
                let tone = (v.phases[0] * TAU).sin() * decay(t, 0.08) * 0.55 + (v.phases[1] * TAU).sin() * decay(t, 0.05) * 0.35;
                let shaped = v.filters[0].tick(noise);
                let snappy = v.filters[1].tick(shaped) * decay(t, length(2, if is_909 { 0.16 } else { 0.12 })) * if is_909 { 0.9 } else { 0.7 };
                s += (tone + snappy) * v.velocity * p.level[2] * 1.1;
                v.t += dt;
                v.active = v.t < length(2, 0.16) * 7.0;
            }

            // Clap: four bursts ten milliseconds apart, then the tail.
            let v = &mut self.voices[3];
            if v.active {
                let t = v.t;
                let bursts = if t < 0.04 { decay(t % 0.01, 0.003) } else { 0.0 };
                let tail = if t >= 0.03 { decay(t - 0.03, length(3, if is_909 { 0.19 } else { 0.14 })) * 0.7 } else { 0.0 };
                let band = v.filters[0].tick(noise);
                let clap = v.filters[1].tick(band) * (bursts + tail);
                s += clap * v.velocity * p.level[3] * 2.4;
                v.t += dt;
                v.active = v.t < 0.03 + length(3, 0.19) * 7.0;
            }

            // Hats: the squares (and, on the 909, noise) through their filters.
            for voice in [4, 5] {
                let v = &mut self.voices[voice];
                if !v.active {
                    continue;
                }
                let t = v.t;
                let tau = if voice == 4 { length(4, if is_909 { 0.03 } else { 0.038 }) } else { length(5, if is_909 { 0.32 } else { 0.28 }) };
                let source = metal.iter().sum::<f32>() / 6.0;
                let out = if is_909 {
                    let mix = source * 0.45 + noise * 0.55;
                    let high = v.filters[0].tick(mix);
                    v.filters[1].tick(high) * 2.2
                } else {
                    let banded = v.filters[0].tick(source) * 0.35 + v.filters[1].tick(source);
                    v.filters[2].tick(banded) * 2.6
                };
                // A quick start, then the decay.
                let attack = (t / 0.0008).min(1.0);
                s += out * attack * decay(t, tau) * v.velocity * p.level[voice];
                v.t += dt;
                v.active = v.t < tau * 7.0;
            }

            // Toms: a falling sine and a breath of noise.
            for (voice, pitch) in TOM_PITCH {
                let v = &mut self.voices[voice];
                if !v.active {
                    continue;
                }
                let t = v.t;
                let base = if is_909 { 100.0 } else { 85.0 } * pitch * v.base * tune(voice);
                let sweep = if is_909 { 0.5 } else { 0.2 };
                let freq = base * (1.0 + sweep * decay(t, 0.03));
                v.phases[0] = (v.phases[0] + freq * dt) % 1.0;
                let tau = length(voice, 0.24 / pitch);
                let body = (v.phases[0] * TAU).sin() * decay(t, tau);
                let breath = v.filters[0].tick(noise) * decay(t, 0.03) * 0.25;
                s += (body + breath) * v.velocity * p.level[voice];
                v.t += dt;
                v.active = v.t < tau * 7.0;
            }

            // Cowbell: two squares, a band-pass, a sharp first drop then a ring.
            let v = &mut self.voices[7];
            if v.active {
                let t = v.t;
                v.phases[0] = (v.phases[0] + 540.0 * tune(7) * dt) % 1.0;
                v.phases[1] = (v.phases[1] + 800.0 * tune(7) * dt) % 1.0;
                let tone = square(v.phases[0]) * 0.5 + square(v.phases[1]) * 0.5;
                let env = 0.7 * decay(t, 0.012) + 0.3 * decay(t, length(7, 0.22));
                s += v.filters[0].tick(tone) * env * v.velocity * p.level[7] * 1.4;
                v.t += dt;
                v.active = v.t < length(7, 0.22) * 7.0;
            }

            // Crash: the high squares and noise, a burst and a long wash.
            let v = &mut self.voices[10];
            if v.active {
                let t = v.t;
                let tau = length(10, if is_909 { 0.5 } else { 0.45 });
                let source = cymbal * 0.6 + noise * 0.6;
                let high = v.filters[0].tick(source);
                let out = high * 0.6 + v.filters[1].tick(high) * 1.2;
                let env = (t / 0.001).min(1.0) * (0.5 * decay(t, 0.04) + 0.5 * decay(t, tau));
                s += out * env * v.velocity * p.level[10];
                v.t += dt;
                v.active = v.t < tau * 7.0;
            }

            // Ride: mostly the squares, a ping that rings on.
            let v = &mut self.voices[11];
            if v.active {
                let t = v.t;
                let tau = length(11, 0.45);
                let source = cymbal * 0.85 + noise * 0.15;
                let out = v.filters[0].tick(source) * 1.4 + v.filters[1].tick(source) * 0.3;
                let env = (t / 0.0008).min(1.0) * (0.45 * decay(t, 0.015) + 0.55 * decay(t, tau));
                s += out * env * v.velocity * p.level[11] * 1.2;
                v.t += dt;
                v.active = v.t < tau * 7.0;
            }

            // Shaker: noise that swells for a few milliseconds and falls.
            let v = &mut self.voices[12];
            if v.active {
                let t = v.t;
                let tau = length(12, 0.045);
                let banded = v.filters[0].tick(noise);
                let out = v.filters[1].tick(banded);
                let env = (t / 0.006).min(1.0) * decay(t, tau);
                s += out * env * v.velocity * p.level[12] * 2.2;
                v.t += dt;
                v.active = v.t < 0.006 + tau * 7.0;
            }

            // Tambourine: jingles shaken three times in quick succession.
            let v = &mut self.voices[13];
            if v.active {
                let t = v.t;
                let tau = length(13, 0.12);
                let strokes: f32 = [0.0, 0.007, 0.016].iter().filter(|&&k| t >= k).map(|&k| decay(t - k, 0.004)).sum();
                let high = v.filters[0].tick(cymbal * 0.5 + noise * 0.5);
                let out = v.filters[1].tick(high);
                s += out * (strokes * 0.6 + decay(t, tau) * 0.5) * v.velocity * p.level[13] * 2.4;
                v.t += dt;
                v.active = v.t < tau * 7.0;
            }

            // Conga: a sine a touch high at first, and the slap.
            let v = &mut self.voices[14];
            if v.active {
                let t = v.t;
                let tau = length(14, 0.16);
                let freq = if is_909 { 350.0 } else { 330.0 } * v.base * tune(14) * (1.0 + 0.12 * decay(t, 0.01));
                v.phases[0] = (v.phases[0] + freq * dt) % 1.0;
                let body = (v.phases[0] * TAU).sin() * decay(t, tau);
                let slap = v.filters[0].tick(noise) * decay(t, 0.004) * 0.4;
                s += (body + slap) * v.velocity * p.level[14];
                v.t += dt;
                v.active = v.t < tau * 7.0;
            }

            // Clave: a short, high wooden sine.
            let v = &mut self.voices[15];
            if v.active {
                let t = v.t;
                let tau = length(15, 0.025);
                v.phases[0] = (v.phases[0] + 2500.0 * tune(15) * dt) % 1.0;
                s += (v.phases[0] * TAU).sin() * decay(t, tau) * v.velocity * p.level[15] * 0.9;
                v.t += dt;
                v.active = v.t < tau * 7.0;
            }

            // Headroom: kick and clap often land together.
            let out = (s * 0.55).clamp(-1.5, 1.5);
            *out_l += out;
            *out_r += out;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn render(model: DrumModel, voice: usize, base: f32, seconds: f32) -> Vec<f32> {
        let p = DrumParams::default();
        let mut kit = ModelKit::new(model, SR);
        kit.hit(voice, 1.0, base, &p);
        let frames = (SR * seconds) as usize;
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            kit.render(&p, a, b);
        }
        l
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// The share of energy above `hz` (a second-order high-pass's output
    /// against the whole).
    fn above(x: &[f32], hz: f32) -> f32 {
        let mut filter = Biquad::high(hz, 0.707, SR);
        let (mut total, mut high) = (0.0, 0.0);
        for &v in x {
            let h = filter.tick(v);
            high += h * h;
            total += v * v;
        }
        high / total.max(1e-12)
    }

    /// Zero crossings a second over a stretch: twice the frequency of a tone.
    fn pitch(x: &[f32]) -> f32 {
        let crossings = x.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        crossings as f32 / 2.0 / (x.len() as f32 / SR)
    }

    #[test]
    fn every_sound_of_both_models_plays_and_dies_away() {
        for model in [DrumModel::Tr808, DrumModel::Tr909] {
            for voice in 0..DRUM_SOUNDS {
                let out = render(model, voice, 1.0, 4.0);
                assert!(out.iter().all(|v| v.is_finite() && v.abs() <= 1.5));
                let start = rms(&out[..(SR * 0.05) as usize]);
                assert!(start > 0.01, "{model:?} {voice}: silent");
                assert!(rms(&out[(SR * 3.5) as usize..]) < start * 1e-3, "{model:?} {voice}: rings on");
            }
        }
    }

    #[test]
    fn the_kicks_are_an_808s_and_a_909s() {
        // 808: a round tone near 49 Hz that rings long.
        let k808 = render(DrumModel::Tr808, 0, 1.0, 1.5);
        let f = pitch(&k808[(SR * 0.05) as usize..(SR * 0.4) as usize]);
        assert!((44.0..56.0).contains(&f), "{f}");
        assert!(rms(&k808[(SR * 0.4) as usize..(SR * 0.5) as usize]) > 0.15 * rms(&k808[..(SR * 0.1) as usize]), "it rings");
        // 909: high at first, falling fast, shorter.
        let k909 = render(DrumModel::Tr909, 0, 1.0, 1.0);
        let (early, late) = (pitch(&k909[48..(SR * 0.012) as usize]), pitch(&k909[(SR * 0.08) as usize..(SR * 0.2) as usize]));
        assert!(early > 2.0 * late && (45.0..65.0).contains(&late), "{early} → {late}");
        assert!(rms(&k909[(SR * 0.6) as usize..(SR * 0.7) as usize]) < rms(&k808[(SR * 0.6) as usize..(SR * 0.7) as usize]));
    }

    #[test]
    fn hats_are_bright_and_the_open_one_longer() {
        for model in [DrumModel::Tr808, DrumModel::Tr909] {
            let closed = render(model, 4, 1.0, 0.5);
            let open = render(model, 5, 1.0, 1.0);
            assert!(above(&closed, 5000.0) > 0.5, "{model:?}: {}", above(&closed, 5000.0));
            let tail = |x: &[f32]| rms(&x[(SR * 0.15) as usize..(SR * 0.25) as usize]);
            assert!(tail(&open) > 10.0 * tail(&closed), "{model:?}");
        }
    }

    #[test]
    fn the_clap_claps_four_times_and_the_snare_has_its_snap() {
        let clap = render(DrumModel::Tr808, 3, 1.0, 0.1);
        // The level every 2.5 ms: four peaks in the first 40 ms.
        let window = (SR * 0.0025) as usize;
        let levels: Vec<f32> = clap[..(SR * 0.04) as usize].chunks(window).map(rms).collect();
        let peaks = (1..levels.len() - 1).filter(|&i| levels[i] > levels[i - 1] && levels[i] >= levels[i + 1]).count() + usize::from(levels[0] > levels[1]);
        assert!(peaks >= 3, "{levels:?}");
        // The 808's snare is mostly its two tones; the 909's, its noise.
        for (model, share) in [(DrumModel::Tr808, 0.12), (DrumModel::Tr909, 0.3)] {
            let snare = render(model, 2, 1.0, 0.3);
            assert!(above(&snare, 2000.0) > share, "{model:?}: noise above 2 kHz: {}", above(&snare, 2000.0));
        }
    }

    #[test]
    fn the_new_sounds_sound_like_themselves() {
        for model in [DrumModel::Tr808, DrumModel::Tr909] {
            // Cymbals bright and long, the crash longer than the ride's ping.
            let crash = render(model, 10, 1.0, 2.0);
            let ride = render(model, 11, 1.0, 2.0);
            let hat = render(model, 4, 1.0, 2.0);
            let at = |x: &[f32], from: f32| rms(&x[(SR * from) as usize..(SR * (from + 0.1)) as usize]);
            assert!(above(&crash, 3000.0) > 0.5 && above(&ride, 3000.0) > 0.5, "{model:?}");
            assert!(at(&crash, 0.8) > 10.0 * at(&hat, 0.8) && at(&ride, 0.8) > 10.0 * at(&hat, 0.8), "{model:?}: cymbals ring");
            // Shaker short and high.
            let shaker = render(model, 12, 1.0, 0.5);
            assert!(above(&shaker, 4000.0) > 0.5 && at(&shaker, 0.3) < 0.01 * at(&shaker, 0.0), "{model:?}");
            // Toms go up low, mid, high.
            let tom = |v| pitch(&render(model, v, 1.0, 0.3)[(SR * 0.05) as usize..(SR * 0.15) as usize]);
            assert!(tom(6) < tom(8) && tom(8) < tom(9), "{model:?}: {} {} {}", tom(6), tom(8), tom(9));
            // Conga and clave at their pitches.
            let conga = pitch(&render(model, 14, 1.0, 0.2)[(SR * 0.03) as usize..(SR * 0.12) as usize]);
            assert!((300.0..380.0).contains(&conga), "{model:?}: conga {conga}");
            let clave = pitch(&render(model, 15, 1.0, 0.05)[48..(SR * 0.02) as usize]);
            assert!((2200.0..2800.0).contains(&clave), "{model:?}: clave {clave}");
        }
    }
}
