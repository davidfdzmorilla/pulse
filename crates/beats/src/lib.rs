//! Finding the tempo and the bars of a piece of audio, so it can follow the
//! project's tempo (auto-warp).
//!
//! Beats and downbeats come from Beat This! (Institute of Computational
//! Perception, JKU Linz), through its Rust port `beat-this`, with the small model,
//! which ships inside Pulse (models/, MIT). [`fit`] turns them into a steady grid:
//! one tempo for the whole piece (least squares over every beat, so long songs
//! stay in time) and where its bars fall.

use std::path::{Path, PathBuf};

const MEL_MODEL: &[u8] = include_bytes!("../models/mel_spectrogram.onnx");
const BEAT_MODEL: &[u8] = include_bytes!("../models/beat_this_small.onnx");
/// Beats per bar: 4/4 is assumed.
pub const BEATS_PER_BAR: f64 = 4.0;

/// A piece's steady tempo and where its bars fall.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub bpm: f64,
    /// Time of the first beat that starts a bar, in seconds (may be in the piece
    /// or the first one after it starts).
    pub downbeat: f64,
    /// How many beats were found (a measure of how sure the grid is).
    pub beats: usize,
}

impl Grid {
    pub fn beat_seconds(&self) -> f64 {
        60.0 / self.bpm
    }

    pub fn bar_seconds(&self) -> f64 {
        self.beat_seconds() * BEATS_PER_BAR
    }

    /// The bar line at or before the piece's start: where a clip of it should
    /// begin so its bars land on the project's (≤ 0: before the file, silence).
    pub fn start(&self) -> f64 {
        let bar = self.bar_seconds();
        // A downbeat this close to the start is the start (the detector's times
        // come in 20 ms steps): a loop cut on the beat starts right away.
        if self.downbeat < (0.06f64).min(self.beat_seconds() / 8.0) {
            return 0.0;
        }
        self.downbeat - (self.downbeat / bar - 1e-6).ceil().max(0.0) * bar
    }
}

/// Beats and downbeats of a piece (mono or stereo mix), in seconds.
pub fn detect(left: &[f32], right: &[f32], sample_rate: f32) -> Result<(Vec<f64>, Vec<f64>), String> {
    let mono: Vec<f32> = left.iter().zip(right).map(|(l, r)| 0.5 * (l + r)).collect();
    let (mel, beat) = models()?;
    let mut tracker = beat_this::BeatThis::new(&beat_this::RtenRuntime, &mel, &beat).map_err(|e| format!("No se pudo cargar el detector de ritmo: {e}"))?;
    let analysis = tracker.analyze_owned(mono, sample_rate as u32).map_err(|e| format!("No se pudo analizar el ritmo: {e}"))?;
    Ok((analysis.beats.iter().map(|&t| t as f64).collect(), analysis.downbeats.iter().map(|&t| t as f64).collect()))
}

/// The grid of a piece: [`detect`] then [`fit`].
pub fn analyze(left: &[f32], right: &[f32], sample_rate: f32) -> Result<Option<Grid>, String> {
    let (beats, downbeats) = detect(left, right, sample_rate)?;
    Ok(fit(&beats, &downbeats))
}

/// The models, written once to the cache folder (the runtime loads files).
fn models() -> Result<(PathBuf, PathBuf), String> {
    let dir = match std::env::var_os("HOME") {
        Some(home) => Path::new(&home).join("Library/Caches/Pulse/beat-this"),
        None => std::env::temp_dir().join("pulse-beat-this"),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("No se pudo preparar {}: {e}", dir.display()))?;
    let write = |name: &str, bytes: &[u8]| -> Result<PathBuf, String> {
        let path = dir.join(name);
        if std::fs::metadata(&path).map(|m| m.len()).ok() != Some(bytes.len() as u64) {
            let partial = path.with_extension("part");
            std::fs::write(&partial, bytes).and_then(|_| std::fs::rename(&partial, &path)).map_err(|e| format!("No se pudo escribir {}: {e}", path.display()))?;
        }
        Ok(path)
    };
    Ok((write("mel_spectrogram.onnx", MEL_MODEL)?, write("beat_this_small.onnx", BEAT_MODEL)?))
}

/// A steady grid through the detected beats: the tempo from a straight line
/// through all of them, and the bars from the most common position of the
/// downbeats among the beats. Each beat is numbered from the one before it (one
/// beat on, two after a missed one, none for an extra one), since the detector's
/// times come in 20 ms steps and numbering by a rounded beat length would drift.
/// Two passes: the second leaves out beats far from the first line. None with
/// too few beats.
pub fn fit(beats: &[f64], downbeats: &[f64]) -> Option<Grid> {
    if beats.len() < 8 {
        return None;
    }
    let mut intervals: Vec<f64> = beats.windows(2).map(|w| w[1] - w[0]).filter(|&d| d > 0.2 && d < 2.0).collect();
    if intervals.len() < 4 {
        return None;
    }
    intervals.sort_by(f64::total_cmp);
    let median = intervals[intervals.len() / 2];
    let mut numbered = vec![(0i64, beats[0])];
    for w in beats.windows(2) {
        let step = ((w[1] - w[0]) / median).round() as i64;
        let k = numbered.last().map_or(0, |&(k, _)| k) + step;
        numbered.push((k, w[1]));
    }
    let (mut origin, mut period) = line(&numbered)?;
    // Again without the beats more than a fifth of a beat off the line.
    let kept: Vec<(i64, f64)> = numbered.iter().copied().filter(|&(k, t)| (t - (origin + k as f64 * period)).abs() < 0.2 * period).collect();
    if kept.len() >= 8 {
        (origin, period) = line(&kept)?;
    }
    let mut bpm = 60.0 / period;
    // Produced music is usually at a whole tempo: keep it when it is that close.
    if (bpm - bpm.round()).abs() < 0.05 {
        let whole = 60.0 / bpm.round();
        // Through the same centre of mass, at the whole tempo.
        let (sk, st, n) = kept.iter().fold((0.0, 0.0, 0.0), |(a, b, n), &(k, t)| (a + k as f64, b + t, n + 1.0));
        origin = st / n - sk / n * whole;
        period = whole;
        bpm = bpm.round();
    }
    let index = |t: f64| ((t - origin) / period).round() as i64;
    // Downbeats vote for which beat of four starts the bar; without any, the first beat.
    let k0 = index(beats[0]);
    let mut votes = [0usize; 4];
    for &d in downbeats {
        votes[index(d).rem_euclid(4) as usize] += 1;
    }
    let best = votes.iter().copied().max().unwrap_or(0);
    let phase = if best == 0 { k0.rem_euclid(4) } else { votes.iter().position(|&v| v == best).unwrap_or(0) as i64 };
    // The first downbeat at or after the first detected beat.
    let k = k0 + (phase - k0).rem_euclid(4);
    Some(Grid { bpm, downbeat: origin + k as f64 * period, beats: beats.len() })
}

/// Least squares t = origin + k · period.
fn line(points: &[(i64, f64)]) -> Option<(f64, f64)> {
    let n = points.len() as f64;
    let (sk, st) = points.iter().fold((0.0, 0.0), |(a, b), &(k, t)| (a + k as f64, b + t));
    let (mk, mt) = (sk / n, st / n);
    let (num, den) = points.iter().fold((0.0, 0.0), |(a, b), &(k, t)| (a + (k as f64 - mk) * (t - mt), b + (k as f64 - mk) * (k as f64 - mk)));
    (den > 0.0 && num > 0.0).then(|| (mt - mk * num / den, num / den))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Beats of a piece at `bpm` from `start`, with a little timing jitter.
    fn beats(bpm: f64, start: f64, count: usize) -> Vec<f64> {
        (0..count).map(|i| start + i as f64 * 60.0 / bpm + if i % 3 == 0 { 0.004 } else { -0.003 }).collect()
    }

    #[test]
    fn a_steady_grid_from_jittery_beats() {
        let b = beats(124.0, 0.31, 400);
        let downbeats: Vec<f64> = b.iter().skip(1).step_by(4).copied().collect();
        let grid = fit(&b, &downbeats).unwrap();
        assert_eq!(grid.bpm, 124.0, "snapped to the whole tempo");
        // The second beat starts the bars.
        assert!((grid.downbeat - (0.31 + 60.0 / 124.0)).abs() < 0.01, "{grid:?}");
        // Where a clip should start: the bar line before the file (silence first).
        let start = grid.start();
        assert!(start <= 0.0 && start > -grid.bar_seconds());
        assert!(((grid.downbeat - start) / grid.bar_seconds() - (grid.downbeat - start) / grid.bar_seconds()).abs() < 1e-9);
        assert!((((grid.downbeat - start) / grid.bar_seconds()).round() - (grid.downbeat - start) / grid.bar_seconds()).abs() < 1e-6, "a whole number of bars");
    }

    #[test]
    fn missed_and_extra_beats_dont_move_the_tempo() {
        let mut b = beats(127.3, 0.0, 600);
        b.remove(100);
        b.remove(300);
        b.insert(50, b[49] + 0.1);
        let grid = fit(&b, &[]).unwrap();
        assert!((grid.bpm - 127.3).abs() < 0.02, "{}", grid.bpm);
        assert!(grid.downbeat.abs() < 0.01, "no downbeats: the first beat");
    }

    #[test]
    fn beats_in_20_ms_steps_dont_drift() {
        // What the detector gives: times rounded to its 50 frames a second.
        let b: Vec<f64> = (0..300).map(|i| ((0.5 + i as f64 * 60.0 / 128.0) * 50.0).round() / 50.0).collect();
        let grid = fit(&b, &[]).unwrap();
        assert_eq!(grid.bpm, 128.0, "{grid:?}");
        assert!((grid.downbeat - 0.5).abs() < 0.01, "{grid:?}");
    }

    #[test]
    fn a_loop_cut_on_the_beat_starts_at_once() {
        let grid = Grid { bpm: 120.0, downbeat: 0.02, beats: 32 };
        assert_eq!(grid.start(), 0.0);
        let grid = Grid { bpm: 120.0, downbeat: 0.5, beats: 32 };
        assert!((grid.start() + 1.5).abs() < 1e-9, "the bar line before: {}", grid.start());
    }

    #[test]
    fn too_few_beats_no_grid() {
        assert_eq!(fit(&beats(120.0, 0.0, 5), &[]), None);
    }

    #[test]
    fn a_click_track_is_found() {
        // Four bars per... 32 bars of clicks at 128 BPM, accented on the downbeat,
        // starting 0.5 s in.
        let rate = 44_100.0;
        let bpm = 128.0;
        let beat = 60.0 / bpm;
        let length = (0.5 + 32.0 * 4.0 * beat + 1.0) * rate as f64;
        let mut x = vec![0f32; length as usize];
        for i in 0..32 * 4 {
            let at = ((0.5 + i as f64 * beat) * rate as f64) as usize;
            let (freq, gain) = if i % 4 == 0 { (1600.0, 1.0) } else { (800.0, 0.5) };
            for j in 0..2000 {
                let env = (-(j as f32) / 300.0).exp();
                x[at + j] += gain * env * (2.0 * std::f32::consts::PI * freq * j as f32 / rate).sin();
                // A kick too, so it sounds like music to the model.
                if i % 4 == 0 || i % 4 == 2 {
                    x[at + j] += 0.8 * (-(j as f32) / 900.0).exp() * (2.0 * std::f32::consts::PI * (60.0 + 80.0 * (-(j as f32) / 200.0).exp()) * j as f32 / rate).sin();
                }
            }
        }
        let started = std::time::Instant::now();
        let grid = analyze(&x, &x, rate).unwrap().expect("a grid");
        println!("{grid:?} in {:.2} s", started.elapsed().as_secs_f32());
        assert!((grid.bpm - bpm).abs() < 0.5, "{grid:?}");
        // On a beat of the clicks (which beat starts the bar is the model's call).
        let offset = (grid.downbeat - 0.5) / beat;
        assert!((offset - offset.round()).abs() < 0.1, "{grid:?}");
    }

    /// Pulse's demo song (rendered at 120 BPM): PULSE_SONG=demo.wav.
    #[test]
    #[ignore = "needs PULSE_SONG"]
    fn a_song() {
        let path = std::env::var("PULSE_SONG").unwrap();
        let mut reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().map(Result::unwrap).collect(),
            hound::SampleFormat::Int => reader.samples::<i32>().map(|s| s.unwrap() as f32 / (1u32 << (spec.bits_per_sample - 1)) as f32).collect(),
        };
        let channels = spec.channels as usize;
        let left: Vec<f32> = samples.iter().step_by(channels).copied().collect();
        let right: Vec<f32> = samples.iter().skip(channels - 1).step_by(channels).copied().collect();
        let started = std::time::Instant::now();
        let (b, d) = detect(&left, &right, spec.sample_rate as f32).unwrap();
        let grid = fit(&b, &d);
        println!("{path}: {grid:?} ({} beats, {} downbeats) in {:.2} s", b.len(), d.len(), started.elapsed().as_secs_f32());
        println!("first beats {:?}", &b[..b.len().min(6)]);
        println!("first downbeats {:?}", &d[..d.len().min(4)]);
    }
}

