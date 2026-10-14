//! Wavetables: sequences of single-cycle frames, scanned by an oscillator's
//! position. Each frame is stored at several band-limited levels (mipmaps), so a
//! note only plays the harmonics that fit under Nyquist and never aliases.
//!
//! Tables are built on the UI thread (FFTs and allocation) and shared with the
//! audio thread through `Arc`.

use crate::dsp::{fft, ifft};
use std::f32::consts::{PI, TAU};

/// Samples per frame (the common wavetable format).
pub const FRAME: usize = 2048;
/// Band-limited copies: level `l` keeps harmonics up to `FRAME / 2 >> l`.
pub const LEVELS: usize = 10;
pub const MAX_FRAMES: usize = 256;
/// Points per frame kept for drawing.
pub const PREVIEW_POINTS: usize = 128;

pub struct Wavetable {
    pub name: String,
    frames: usize,
    /// `levels[l][frame * FRAME + i]`.
    levels: Vec<Vec<f32>>,
    /// Each frame reduced to [`PREVIEW_POINTS`] points, for displays.
    pub preview: Vec<Vec<f32>>,
}

impl std::fmt::Debug for Wavetable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wavetable({}, {} frames)", self.name, self.frames)
    }
}

/// Harmonics kept at mip level `level`.
fn harmonics(level: usize) -> usize {
    (FRAME / 2) >> level
}

impl Wavetable {
    /// Builds a table from time-domain frames of any length (each is resampled to
    /// [`FRAME`] samples). The whole table is normalised to a peak of 1.
    pub fn from_frames(name: impl Into<String>, frames: &[Vec<f32>]) -> Self {
        let spectra: Vec<(Vec<f32>, Vec<f32>)> = frames
            .iter()
            .take(MAX_FRAMES)
            .map(|frame| {
                let (mut re, mut im) = (resample(frame, FRAME), vec![0.0; FRAME]);
                fft(&mut re, &mut im);
                (re, im)
            })
            .collect();
        Self::from_spectra(name, spectra)
    }

    /// Builds a table from harmonic amplitudes per frame (index 1 is the
    /// fundamental), with sine phase.
    pub fn from_harmonics(name: impl Into<String>, frames: &[Vec<f32>]) -> Self {
        let spectra = frames
            .iter()
            .take(MAX_FRAMES)
            .map(|amplitudes| {
                let (mut re, mut im) = (vec![0.0; FRAME], vec![0.0; FRAME]);
                for (n, &a) in amplitudes.iter().enumerate().take(FRAME / 2).skip(1) {
                    // sin(nθ) has a spectrum of -i·n/2 at bin n and +i·n/2 at bin -n.
                    im[n] = -a * FRAME as f32 / 2.0;
                    im[FRAME - n] = a * FRAME as f32 / 2.0;
                    re[n] = 0.0;
                }
                (re, im)
            })
            .collect();
        Self::from_spectra(name, spectra)
    }

    fn from_spectra(name: impl Into<String>, spectra: Vec<(Vec<f32>, Vec<f32>)>) -> Self {
        let frames = spectra.len().max(1);
        let mut levels = vec![vec![0.0; frames * FRAME]; LEVELS];
        for (f, (re, im)) in spectra.iter().enumerate() {
            for (level, data) in levels.iter_mut().enumerate() {
                let keep = harmonics(level);
                let (mut lr, mut li) = (vec![0.0; FRAME], vec![0.0; FRAME]);
                // No DC; harmonics 1..=keep and their mirror images.
                for n in 1..=keep.min(FRAME / 2 - 1) {
                    lr[n] = re[n];
                    li[n] = im[n];
                    lr[FRAME - n] = re[FRAME - n];
                    li[FRAME - n] = im[FRAME - n];
                }
                ifft(&mut lr, &mut li);
                data[f * FRAME..(f + 1) * FRAME].copy_from_slice(&lr);
            }
        }
        let peak = levels[0].iter().fold(0f32, |m, s| m.max(s.abs()));
        if peak > 1e-9 {
            for data in &mut levels {
                for s in data.iter_mut() {
                    *s /= peak;
                }
            }
        }
        let preview = (0..frames)
            .map(|f| (0..PREVIEW_POINTS).map(|i| levels[0][f * FRAME + i * FRAME / PREVIEW_POINTS]).collect())
            .collect();
        Self { name: name.into(), frames, levels, preview }
    }

    /// Cuts a mono file into frames: `frame_size` samples each (wavetable files),
    /// or the whole file as one cycle when it is shorter than a frame.
    pub fn from_samples(name: impl Into<String>, data: &[f32], frame_size: usize) -> Self {
        let frame_size = frame_size.max(16);
        let frames: Vec<Vec<f32>> = if data.len() < frame_size {
            vec![data.to_vec()]
        } else {
            data.chunks_exact(frame_size).take(MAX_FRAMES).map(<[f32]>::to_vec).collect()
        };
        Self::from_frames(name, &frames)
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The mip level for a note at `freq` Hz: the most harmonics that stay under Nyquist.
    pub fn level_for(freq: f32, sample_rate: f32) -> usize {
        let allowed = sample_rate * 0.5 / freq.max(1.0);
        (0..LEVELS).find(|&l| harmonics(l) as f32 <= allowed).unwrap_or(LEVELS - 1)
    }

    /// Reads at `phase` (0..1) and `position` (0..1 across the frames), interpolating
    /// between samples and between neighbouring frames.
    #[inline]
    pub fn read(&self, level: usize, position: f32, phase: f32) -> f32 {
        let data = &self.levels[level.min(LEVELS - 1)];
        let place = position.clamp(0.0, 1.0) * (self.frames - 1) as f32;
        let f0 = place as usize;
        let f1 = (f0 + 1).min(self.frames - 1);
        let mix = place - f0 as f32;
        let x = phase.rem_euclid(1.0) * FRAME as f32;
        let i = x as usize % FRAME;
        let j = (i + 1) % FRAME;
        let t = x - x.floor();
        let at = |f: usize| {
            let base = f * FRAME;
            data[base + i] + (data[base + j] - data[base + i]) * t
        };
        let a = at(f0);
        if mix > 0.0 { a + (at(f1) - a) * mix } else { a }
    }
}

fn resample(frame: &[f32], size: usize) -> Vec<f32> {
    if frame.len() == size {
        return frame.to_vec();
    }
    if frame.is_empty() {
        return vec![0.0; size];
    }
    (0..size)
        .map(|i| {
            let x = i as f32 * frame.len() as f32 / size as f32;
            let a = x as usize % frame.len();
            let b = (a + 1) % frame.len();
            frame[a] + (frame[b] - frame[a]) * (x - x.floor())
        })
        .collect()
}

/// The fundamental of `data` by autocorrelation, if it has a clear pitch (30–2000 Hz).
fn detect_pitch(data: &[f32], sample_rate: f32) -> Option<f32> {
    let n = data.len().min(8192);
    let start = (data.len() - n) / 2;
    let x = &data[start..start + n];
    let energy: f32 = x.iter().map(|s| s * s).sum();
    if energy < 1e-6 {
        return None;
    }
    let (min_lag, max_lag) = ((sample_rate / 2000.0) as usize, ((sample_rate / 30.0) as usize).min(n / 2));
    let correlation = |lag: usize| x[..n - lag].iter().zip(&x[lag..]).map(|(a, b)| a * b).sum::<f32>() / energy;
    // The first strong peak, so octave errors pick the true period.
    let values: Vec<f32> = (min_lag..max_lag).map(correlation).collect();
    let best = values.iter().copied().fold(0.0, f32::max);
    if best < 0.5 {
        return None;
    }
    let index = (1..values.len().saturating_sub(1)).find(|&i| values[i] >= best * 0.9 && values[i] >= values[i - 1] && values[i] >= values[i + 1])?;
    // Parabolic interpolation for a fractional period.
    let (a, b, c) = (values[index - 1], values[index], values[index + 1]);
    let shift = 0.5 * (a - c) / (a - 2.0 * b + c).min(-1e-9);
    Some(sample_rate / (min_lag as f32 + index as f32 + shift))
}

/// Turns any audio into a wavetable by its spectrum: frames step through the
/// file, and each becomes one cycle holding that moment's harmonics. Pitched
/// sounds are resynthesised on their detected harmonic series; unpitched ones
/// map the spectrum straight onto the harmonics.
pub fn spectral(name: impl Into<String>, data: &[f32], sample_rate: f32) -> Wavetable {
    const WINDOW: usize = 4096;
    const FRAMES: usize = 64;
    if data.is_empty() {
        return builtin(0);
    }
    let pitch = detect_pitch(data, sample_rate);
    let hann: Vec<f32> = (0..WINDOW).map(|i| 0.5 - 0.5 * (TAU * i as f32 / WINDOW as f32).cos()).collect();
    let span = data.len().saturating_sub(WINDOW);
    let frames: Vec<Vec<f32>> = (0..FRAMES)
        .map(|f| {
            let start = span * f / (FRAMES - 1);
            let (mut re, mut im) = (vec![0.0; WINDOW], vec![0.0; WINDOW]);
            for i in 0..WINDOW {
                re[i] = data.get(start + i).copied().unwrap_or(0.0) * hann[i];
            }
            fft(&mut re, &mut im);
            let magnitude = |bin: f32| {
                let k = bin.floor() as usize;
                let t = bin - bin.floor();
                let at = |k: usize| if k < WINDOW / 2 { re[k].hypot(im[k]) } else { 0.0 };
                at(k) + (at(k + 1) - at(k)) * t
            };
            (0..=512)
                .map(|h| match (h, pitch) {
                    (0, _) => 0.0,
                    (h, Some(f0)) => {
                        let bin = h as f32 * f0 * WINDOW as f32 / sample_rate;
                        if bin < (WINDOW / 2 - 1) as f32 { magnitude(bin) } else { 0.0 }
                    }
                    (h, None) => magnitude(h as f32),
                })
                .collect()
        })
        .collect();
    Wavetable::from_harmonics(name, &frames)
}

/// Frame size declared in a wavetable WAV's `clm ` chunk ("<!>2048 …"), if any.
pub fn declared_frame_size(wav: &[u8]) -> Option<usize> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12;
    while at + 8 <= wav.len() {
        let id = &wav[at..at + 4];
        let size = u32::from_le_bytes(wav[at + 4..at + 8].try_into().ok()?) as usize;
        let body = wav.get(at + 8..(at + 8 + size).min(wav.len()))?;
        if id == b"clm " {
            let text = String::from_utf8_lossy(body);
            let digits: String = text.trim_start_matches("<!>").chars().take_while(char::is_ascii_digit).collect();
            return digits.parse().ok().filter(|&n: &usize| n >= 16);
        }
        at += 8 + size + (size & 1);
    }
    None
}

/// Names of the built-in tables, in [`builtin`] order.
pub const BUILTIN: [&str; 12] = [
    "Basic Shapes",
    "Saw → Pulse",
    "Harmonic Sweep",
    "Formant Vowels",
    "Sync Sweep",
    "FM Sweep",
    "Fold",
    "Digital Steps",
    "Organ",
    "Growl",
    "Spectral Comb",
    "Noise Spectra",
];

const BUILTIN_FRAMES: usize = 64;

/// Generates built-in table `index` (see [`BUILTIN`]).
pub fn builtin(index: usize) -> Wavetable {
    let name = BUILTIN.get(index).copied().unwrap_or(BUILTIN[0]);
    let n = BUILTIN_FRAMES;
    let t = |f: usize| f as f32 / (n - 1) as f32;
    let time = |shape: &dyn Fn(f32, f32) -> f32| -> Vec<Vec<f32>> {
        (0..n).map(|f| (0..FRAME).map(|i| shape(i as f32 / FRAME as f32, t(f))).collect()).collect()
    };
    let harmonic = |amp: &dyn Fn(usize, f32) -> f32| -> Vec<Vec<f32>> {
        (0..n).map(|f| (0..=512).map(|h| if h == 0 { 0.0 } else { amp(h, t(f)) }).collect()).collect()
    };
    let sine = |p: f32| (p * TAU).sin();
    let saw = |p: f32| 2.0 * ((p + 0.5) % 1.0) - 1.0;
    let triangle = |p: f32| 1.0 - 4.0 * (((p + 0.25) % 1.0) - 0.5).abs();
    let square = |p: f32, width: f32| if p < width { 1.0 } else { -1.0 };
    match index {
        // Sine → triangle → saw → square.
        0 => Wavetable::from_frames(
            name,
            &time(&|p, m| {
                let m = m * 3.0;
                let (a, b, k) = match m {
                    m if m < 1.0 => (sine(p), triangle(p), m),
                    m if m < 2.0 => (triangle(p), saw(p), m - 1.0),
                    m => (saw(p), square(p, 0.5), m - 2.0),
                };
                a + (b - a) * k
            }),
        ),
        // A saw that turns into an ever narrower pulse.
        1 => Wavetable::from_frames(
            name,
            &time(&|p, m| if m < 0.15 { saw(p) + (square(p, 0.5) - saw(p)) * (m / 0.15) } else { square(p, 0.5 - (m - 0.15) / 0.85 * 0.47) }),
        ),
        // From the fundamental alone to a full saw spectrum.
        2 => Wavetable::from_harmonics(name, &harmonic(&|h, m| if (h as f32) <= 1.0 + m * m * 255.0 { 1.0 / h as f32 } else { 0.0 })),
        // Vowels A, E, I, O, U on a 110 Hz voice.
        3 => {
            const VOWELS: [[f32; 3]; 5] = [[800.0, 1150.0, 2900.0], [400.0, 1600.0, 2700.0], [300.0, 2300.0, 3000.0], [450.0, 800.0, 2830.0], [325.0, 700.0, 2530.0]];
            Wavetable::from_harmonics(
                name,
                &harmonic(&|h, m| {
                    let place = m * 4.0;
                    let (a, b) = (place.floor() as usize, (place.floor() as usize + 1).min(4));
                    let k = place - place.floor();
                    let freq = h as f32 * 110.0;
                    let peak = |f: f32, width: f32| (-((freq - f) / width).powi(2)).exp();
                    (0..3)
                        .map(|i| {
                            let f = VOWELS[a][i] + (VOWELS[b][i] - VOWELS[a][i]) * k;
                            peak(f, 90.0 + 30.0 * i as f32) * [1.0, 0.6, 0.3][i]
                        })
                        .sum::<f32>()
                        / (h as f32).sqrt()
                }),
            )
        }
        // A saw hard-synced at a rising ratio.
        4 => Wavetable::from_frames(name, &time(&|p, m| saw((p * (1.0 + m * 7.0)) % 1.0) * (1.0 - p * 0.3))),
        // Phase modulation with a rising index.
        5 => Wavetable::from_frames(name, &time(&|p, m| (p * TAU + m * 8.0 * (p * TAU * 2.0).sin()).sin())),
        // A sine driven into a wave folder.
        6 => Wavetable::from_frames(name, &time(&|p, m| (sine(p) * (1.0 + m * 7.0) * PI * 0.5).sin())),
        // Fewer and fewer steps of a sine.
        7 => Wavetable::from_frames(
            name,
            &time(&|p, m| {
                let steps = 32.0 - m * 30.0;
                (sine(p) * steps).round() / steps
            }),
        ),
        // Drawbar registrations, from flute to full organ.
        8 => Wavetable::from_harmonics(
            name,
            &harmonic(&|h, m| {
                let bars = [(1, 1.0), (2, 0.8 * m.min(0.5) * 2.0), (3, 0.6 * m), (4, 0.7 * m), (6, 0.4 * m * m), (8, 0.5 * m * m)];
                bars.iter().find(|(n, _)| *n == h).map_or(0.0, |(_, a)| *a)
            }),
        ),
        // Odd harmonics with a resonant peak sweeping through them: a talking bass.
        9 => Wavetable::from_harmonics(
            name,
            &harmonic(&|h, m| {
                let centre = 2.0 + m * 30.0;
                let peak = (-((h as f32 - centre) / 3.0).powi(2)).exp() * 3.0;
                let odd = if h % 2 == 1 { 1.0 } else { 0.35 };
                odd * (1.0 + peak) / h as f32
            }),
        ),
        // A saw spectrum with notches moving through it.
        10 => Wavetable::from_harmonics(name, &harmonic(&|h, m| (1.0 / h as f32) * ((h as f32 * (0.15 + m * 0.6)).cos().powi(2)))),
        // Random spectra, darker to brighter.
        _ => {
            let mut seed = 0x1234_5678u32;
            let mut random = move || {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as f32 / u32::MAX as f32
            };
            let frames: Vec<Vec<f32>> = (0..n)
                .map(|f| {
                    let bright = 0.5 + t(f) * 2.5;
                    (0..=512).map(|h| if h == 0 { 0.0 } else { random() / (h as f32).powf(2.0 / bright) }).collect()
                })
                .collect();
            Wavetable::from_harmonics(name, &frames)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Energy of `signal` above `cutoff` Hz, as a fraction of the total.
    fn energy_above(signal: &[f32], cutoff: f32, sample_rate: f32) -> f32 {
        let n = signal.len().next_power_of_two() / 2;
        let (mut re, mut im) = (signal[..n].to_vec(), vec![0.0; n]);
        fft(&mut re, &mut im);
        let bin = |k: usize| re[k] * re[k] + im[k] * im[k];
        let total: f32 = (1..n / 2).map(bin).sum();
        let above: f32 = (1..n / 2).filter(|&k| k as f32 * sample_rate / n as f32 > cutoff).map(bin).sum();
        above / total
    }

    #[test]
    fn a_single_cycle_reads_back() {
        let table = Wavetable::from_frames("sine", &[(0..FRAME).map(|i| (i as f32 / FRAME as f32 * TAU).sin()).collect()]);
        assert_eq!(table.frames(), 1);
        assert!((table.read(0, 0.0, 0.25) - 1.0).abs() < 1e-3);
        assert!(table.read(0, 0.0, 0.75) + 1.0 < 1e-3);
    }

    #[test]
    fn high_notes_play_band_limited_levels_without_aliasing() {
        // A saw at 5 kHz, 48 kHz: only 4 harmonics fit; a naive saw would alias badly.
        let table = builtin(0);
        let (sr, freq) = (48_000.0, 5_000.0);
        let level = Wavetable::level_for(freq, sr);
        assert!(harmonics(level) as f32 * freq <= sr * 0.5);
        let mut phase = 0.0;
        let signal: Vec<f32> = (0..8192)
            .map(|_| {
                let s = table.read(level, 2.0 / 3.0, phase);
                phase = (phase + freq / sr) % 1.0;
                s
            })
            .collect();
        // Energy that is not a harmonic of 5 kHz (aliases) stays tiny: check the band
        // between the 4th harmonic (20 kHz) and Nyquist, and below the fundamental.
        assert!(energy_above(&signal, 20_500.0, sr) < 1e-3, "{}", energy_above(&signal, 20_500.0, sr));
    }

    #[test]
    fn position_morphs_between_frames() {
        let table = builtin(0);
        // Sine at the start, square at the end.
        let at = |position: f32, phase: f32| table.read(0, position, phase);
        assert!((at(0.0, 0.125) - (TAU * 0.125).sin()).abs() < 0.05);
        assert!(at(1.0, 0.125) > 0.9 && at(1.0, 0.125) < 1.2);
    }

    #[test]
    fn every_builtin_is_normalised_and_finite() {
        for index in 0..BUILTIN.len() {
            let table = builtin(index);
            assert_eq!(table.frames(), BUILTIN_FRAMES, "{}", BUILTIN[index]);
            let peak = (0..FRAME).flat_map(|i| (0..4).map(move |f| (i, f as f32 / 3.0))).map(|(i, p)| table.read(0, p, i as f32 / FRAME as f32).abs()).fold(0f32, f32::max);
            assert!(peak > 0.2 && peak <= 1.0001, "{}: {peak}", BUILTIN[index]);
        }
    }

    #[test]
    fn wavetable_files_declare_their_frame_size() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&0u32.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&[0; 16]);
        let text = b"<!>1024 10000000 wavetable (www.example.com)";
        wav.extend_from_slice(b"clm ");
        wav.extend_from_slice(&(text.len() as u32).to_le_bytes());
        wav.extend_from_slice(text);
        assert_eq!(declared_frame_size(&wav), Some(1024));
        assert_eq!(declared_frame_size(b"RIFF\0\0\0\0WAVE"), None);

        let data: Vec<f32> = (0..4096).map(|i| if i < 2048 { (i as f32 / 2048.0 * TAU).sin() } else { 0.5 }).collect();
        assert_eq!(Wavetable::from_samples("two", &data, 2048).frames(), 2);
        assert_eq!(Wavetable::from_samples("one cycle", &data[..600], 2048).frames(), 1);
    }

    #[test]
    fn spectral_tables_follow_the_sound_over_time() {
        let sr = 48_000.0;
        // Half a second of sine, then half a second of square, at 220 Hz.
        let data: Vec<f32> = (0..48_000)
            .map(|i| {
                let s = (i as f32 * 220.0 * TAU / sr).sin();
                if i < 24_000 { s } else { s.signum() * 0.8 }
            })
            .collect();
        assert!((detect_pitch(&data, sr).unwrap() - 220.0).abs() < 1.0);
        let table = spectral("test", &data, sr);
        assert_eq!(table.frames(), 64);
        // The first frame is a pure sine: it matches sin() closely.
        let sine_error: f32 = (0..64).map(|i| {
            let p = i as f32 / 64.0;
            (table.read(0, 0.0, p).abs() - (p * TAU).sin().abs()).abs()
        }).sum::<f32>() / 64.0;
        assert!(sine_error < 0.05, "{sine_error}");
        // The last is square-like: flat near the middle of each half.
        let (a, b) = (table.read(0, 1.0, 0.2).abs(), table.read(0, 1.0, 0.3).abs());
        assert!(a > 0.6 && (a - b).abs() < 0.2, "{a} {b}");
        // Noise has no pitch and still makes a table.
        let mut seed = 7u32;
        let noise: Vec<f32> = (0..20_000).map(|_| { seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5; seed as f32 / u32::MAX as f32 - 0.5 }).collect();
        assert!(detect_pitch(&noise, sr).is_none());
        assert_eq!(spectral("noise", &noise, sr).frames(), 64);
    }
}
