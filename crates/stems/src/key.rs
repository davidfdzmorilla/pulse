//! Keys: finding one from how much each pitch class sounds (Krumhansl–Kessler
//! profiles), and its scale.

/// How strongly each degree of a major and a minor key is felt (Krumhansl &
/// Kessler, 1982), from the tonic up.
const MAJOR: [f64; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR: [f64; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];
const NAMES: [&str; 12] = ["Do", "Do#", "Re", "Mib", "Mi", "Fa", "Fa#", "Sol", "Lab", "La", "Sib", "Si"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Key {
    /// Pitch class of the tonic (0 = C).
    pub tonic: u8,
    pub minor: bool,
}

impl Key {
    /// The key whose profile best matches `weights` (how much each pitch class,
    /// C first, sounds); None if nothing sounds.
    pub fn detect(weights: &[f64; 12]) -> Option<Key> {
        Self::detect_clearly(weights).map(|(key, _)| key)
    }

    /// The best key and how clearly it fits (the correlation, -1..1: from about
    /// 0.6 the key is plain; drums and loops of mixed keys stay lower).
    pub fn detect_clearly(weights: &[f64; 12]) -> Option<(Key, f64)> {
        if weights.iter().sum::<f64>() <= 0.0 {
            return None;
        }
        Key::all().map(|key| (key, key.fit(weights))).max_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// How well `weights` (C first) fit this key: the correlation with its
    /// profile, -1..1.
    pub fn fit(&self, weights: &[f64; 12]) -> f64 {
        let profile = if self.minor { &MINOR } else { &MAJOR };
        let x: Vec<f64> = (0..12).map(|i| weights[(i + self.tonic as usize) % 12]).collect();
        let (mx, my) = (x.iter().sum::<f64>() / 12.0, profile.iter().sum::<f64>() / 12.0);
        let (mut num, mut sx, mut sy) = (0.0, 0.0, 0.0);
        for i in 0..12 {
            num += (x[i] - mx) * (profile[i] - my);
            sx += (x[i] - mx).powi(2);
            sy += (profile[i] - my).powi(2);
        }
        num / (sx * sy).sqrt().max(1e-12)
    }

    /// Whether each pitch class (C first) is in the scale (natural minor).
    pub fn scale(&self) -> [bool; 12] {
        let steps: [usize; 7] = if self.minor { [0, 2, 3, 5, 7, 8, 10] } else { [0, 2, 4, 5, 7, 9, 11] };
        let mut scale = [false; 12];
        for s in steps {
            scale[(self.tonic as usize + s) % 12] = true;
        }
        scale
    }

    /// The scale note nearest to `pitch` (fractional MIDI).
    pub fn snap(&self, pitch: f64) -> u8 {
        let scale = self.scale();
        let below = pitch.floor() as i32;
        (below - 2..=below + 3)
            .filter(|&p| (0..=127).contains(&p) && scale[p.rem_euclid(12) as usize])
            .min_by(|a, b| (*a as f64 - pitch).abs().total_cmp(&(*b as f64 - pitch).abs()))
            .unwrap_or(pitch.round().clamp(0.0, 127.0) as i32) as u8
    }

    /// The pitch class (C first) a scale `step` up from the tonic (0..7, the
    /// scale's degrees), wrapping round the octave.
    pub fn degree(&self, step: usize) -> u8 {
        let steps: [u8; 7] = if self.minor { [0, 2, 3, 5, 7, 8, 10] } else { [0, 2, 4, 5, 7, 9, 11] };
        (self.tonic + steps[step % 7]) % 12
    }

    /// `pitch` moved `steps` notes of the scale up (or down); a note off the
    /// scale counts from its neighbour on the other side (C# up one is D, down
    /// one C). None if it would leave 0..=127.
    pub fn step(&self, pitch: u8, steps: i32) -> Option<u8> {
        let scale = self.scale();
        let mut p = pitch as i32;
        if !scale[p.rem_euclid(12) as usize] {
            let back = if steps < 0 { 1 } else { -1 };
            while !scale[p.rem_euclid(12) as usize] {
                p += back;
            }
        }
        for _ in 0..steps.abs() {
            p += steps.signum();
            while !scale[p.rem_euclid(12) as usize] {
                p += steps.signum();
            }
        }
        (0..=127).contains(&p).then_some(p as u8)
    }

    /// Every key, majors first, from C.
    pub fn all() -> impl Iterator<Item = Key> {
        [false, true].into_iter().flat_map(|minor| (0..12).map(move |tonic| Key { tonic, minor }))
    }

    /// «Mi menor», «Do mayor».
    pub fn name(&self) -> String {
        format!("{} {}", NAMES[self.tonic as usize % 12], if self.minor { "menor" } else { "mayor" })
    }
}

/// How much each pitch class (C first) sounds in `mono` at `rate` Hz: the
/// spectrum's energy from 60 Hz to 2 kHz folded onto the twelve classes (a
/// chromagram), over its first minute.
pub fn chroma(mono: &[f32], rate: f32) -> [f64; 12] {
    use rustfft::num_complex::Complex32;
    const SIZE: usize = 8192;
    let mut weights = [0f64; 12];
    if mono.len() < SIZE {
        return weights;
    }
    let fft = rustfft::FftPlanner::<f32>::new().plan_fft_forward(SIZE);
    let window: Vec<f32> = (0..SIZE).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / SIZE as f32).cos()).collect();
    // The pitch class of each bin, if it is in range.
    let class: Vec<Option<usize>> = (0..SIZE / 2)
        .map(|b| {
            let f = b as f64 * rate as f64 / SIZE as f64;
            (60.0..2000.0).contains(&f).then(|| ((69.0 + 12.0 * (f / 440.0).log2()).round() as i64).rem_euclid(12) as usize)
        })
        .collect();
    let end = mono.len().min((rate * 60.0) as usize);
    let mut start = 0;
    while start + SIZE <= end {
        let mut buffer: Vec<Complex32> = mono[start..start + SIZE].iter().zip(&window).map(|(x, w)| Complex32::new(x * w, 0.0)).collect();
        fft.process(&mut buffer);
        for (b, c) in class.iter().enumerate() {
            if let Some(c) = c {
                weights[*c] += buffer[b].norm() as f64;
            }
        }
        start += SIZE / 2;
    }
    weights
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weights(pitches: &[u8]) -> [f64; 12] {
        let mut w = [0.0; 12];
        for p in pitches {
            w[*p as usize % 12] += 1.0;
        }
        w
    }

    #[test]
    fn keys_come_from_the_notes_that_sound() {
        // C major scale, tonic and fifth stressed.
        let c = weights(&[60, 62, 64, 65, 67, 69, 71, 60, 67, 60, 64]);
        assert_eq!(Key::detect(&c), Some(Key { tonic: 0, minor: false }));
        // E minor arpeggio and scale: E G B, F# A C D.
        let e = weights(&[64, 67, 71, 64, 67, 71, 66, 69, 72, 74, 64, 76]);
        assert_eq!(Key::detect(&e).map(|k| k.name()), Some("Mi menor".into()));
        assert_eq!(Key::detect(&[0.0; 12]), None);
    }

    #[test]
    fn notes_snap_to_the_scale() {
        let e_minor = Key { tonic: 4, minor: true };
        // Between F (not in E minor) and F#: F#. Just above G: G. A hair under E: E.
        assert_eq!(e_minor.snap(65.4), 66);
        assert_eq!(e_minor.snap(67.3), 67);
        assert_eq!(e_minor.snap(63.8), 64);
        // G# is not in the scale: nearest of G or A.
        assert_eq!(e_minor.snap(67.9), 67);
        assert_eq!(e_minor.snap(68.7), 69);
    }

    #[test]
    fn steps_walk_the_scale() {
        let c = Key { tonic: 0, minor: false };
        // E up one is F, B up one is C, C down one is B.
        assert_eq!(c.step(64, 1), Some(65));
        assert_eq!(c.step(71, 1), Some(72));
        assert_eq!(c.step(60, -1), Some(59));
        // C# (not in the scale) counts from C: up one is D.
        assert_eq!(c.step(61, 1), Some(62));
        assert_eq!(c.step(61, -1), Some(60));
        assert_eq!(c.step(60, 7), Some(72));
        assert_eq!(c.step(127, 1), None);
        assert_eq!(Key { tonic: 4, minor: true }.degree(2), 7, "E minor's third is G");
        assert_eq!(Key::all().count(), 24);
    }

    #[test]
    fn a_chord_played_long_gives_its_key() {
        // An E minor chord (E2 B2 E3 G3 B3) with harmonics, two seconds.
        let rate = 22_050.0;
        let audio: Vec<f32> = (0..44_100)
            .map(|i| {
                let t = i as f32 / rate;
                [40, 47, 52, 55, 59].iter().map(|&p| {
                    let f = 440.0 * 2f32.powf((p as f32 - 69.0) / 12.0);
                    (1..4).map(|h| (2.0 * std::f32::consts::PI * f * h as f32 * t).sin() / h as f32).sum::<f32>()
                }).sum::<f32>() * 0.1
            })
            .collect();
        let w = chroma(&audio, rate);
        assert_eq!(Key::detect(&w).map(|k| k.name()), Some("Mi menor".into()), "{w:?}");
    }
}
