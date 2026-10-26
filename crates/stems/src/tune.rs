//! Tuning a recorded voice note by note, as Melodyne does: its notes and the
//! pitch it sings (see [`pitch`]), and the voice moved to where each note is
//! taken (another pitch, less vibrato) with TD-PSOLA.
//!
//! TD-PSOLA cuts the voice into grains two periods long, one per period, and
//! lays them out again closer together (higher) or further apart (lower):
//! each grain keeps its shape, so the voice keeps its vowels (its formants)
//! instead of sounding like a chipmunk, and stays as long as it was. Where
//! nothing is moved the grains go back where they were, so the audio there is
//! the same, sample for sample.

use crate::pitch;

/// A note as sung: where (seconds) and its pitch (fractional MIDI, the middle
/// of what is sung).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SungNote {
    pub start: f64,
    pub end: f64,
    pub pitch: f32,
}

/// What a voice sings: its pitch over time and its notes.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    /// Seconds between the values of `curve`.
    pub hop: f64,
    /// The pitch (fractional MIDI) at each step, where there is one.
    pub curve: Vec<Option<f32>>,
    pub notes: Vec<SungNote>,
}

fn midi(hz: f64) -> f32 {
    (69.0 + 12.0 * (hz / 440.0).log2()) as f32
}

/// The notes and pitch of the voice in `mono`.
pub fn analyse(mono: &[f32], rate: f32) -> Analysis {
    let track = pitch::track(mono, rate);
    let hop = pitch::frame_time(1);
    // pYIN's pitch is in tenths of a semitone: each step refined (a vibrato
    // taken away by tenths would go in steps).
    let curve: Vec<Option<f32>> = track.f0.iter().enumerate().map(|(i, f)| f.map(|f| midi(refine(mono, rate, i as f64 * hop, f)))).collect();
    let notes = pitch::notes(&track, None)
        .into_iter()
        .filter_map(|n| {
            let (from, to) = ((n.start / hop).round() as usize, ((n.end / hop).round() as usize).min(curve.len()));
            let mut sung: Vec<f32> = curve.get(from..to)?.iter().flatten().copied().collect();
            if sung.is_empty() {
                return None;
            }
            sung.sort_by(f32::total_cmp);
            Some(SungNote { start: n.start, end: n.end, pitch: sung[sung.len() / 2] })
        })
        .collect();
    Analysis { hop, curve, notes }
}

/// The frequency at `seconds` of `mono`, near `hz`: the period (within 6 %
/// of it) whose two periods around there differ least (YIN's difference, between
/// lags).
fn refine(mono: &[f32], rate: f32, seconds: f64, hz: f64) -> f64 {
    let lag = rate as f64 / hz;
    let (low, high) = ((lag * 0.94).floor() as usize, (lag * 1.06).ceil() as usize + 1);
    let width = high;
    let centre = (seconds * rate as f64) as usize;
    let Some(start) = centre.checked_sub(width / 2 + high).filter(|s| s + high + width <= mono.len()) else { return hz };
    let d: Vec<f64> = (low.max(2)..=high)
        .map(|tau| (0..width).map(|j| (mono[start + high + j] as f64 - mono[start + high + j - tau] as f64).powi(2)).sum())
        .collect();
    let i = (1..d.len() - 1).min_by(|&a, &b| d[a].total_cmp(&d[b])).unwrap_or(0);
    if i == 0 {
        return hz;
    }
    let (a, b, c) = (d[i - 1], d[i], d[i + 1]);
    let shift = if a + c - 2.0 * b > 0.0 { 0.5 * (a - c) / (a + c - 2.0 * b) } else { 0.0 };
    rate as f64 / ((low.max(2) + i) as f64 + shift.clamp(-1.0, 1.0))
}

impl Analysis {
    /// The pitch at `seconds`, between steps, if the voice has one there.
    pub fn pitch_at(&self, seconds: f64) -> Option<f32> {
        let x = seconds / self.hop;
        if x < 0.0 {
            return None;
        }
        let i = x.floor() as usize;
        let t = (x - i as f64) as f32;
        match (self.curve.get(i).copied().flatten(), self.curve.get(i + 1).copied().flatten()) {
            (Some(a), Some(b)) => Some(a + (b - a) * t),
            (Some(a), None) if t < 0.5 => Some(a),
            (None, Some(b)) if t >= 0.5 => Some(b),
            _ => None,
        }
    }
}

/// Where a note is taken: to `target` (fractional MIDI) from where it was sung
/// (`pitch`), with `modulation` of its wavering (1: as sung, 0: none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteEdit {
    pub start: f64,
    pub end: f64,
    pub pitch: f32,
    pub target: f32,
    pub modulation: f32,
}

impl NoteEdit {
    fn changes(&self) -> bool {
        (self.target - self.pitch).abs() > 1e-4 || (self.modulation - 1.0).abs() > 1e-4
    }

    /// The pitch it sings at `seconds` (sung `p` there), moved.
    pub fn moved(&self, p: f32) -> f32 {
        self.target + self.modulation * (p - self.pitch)
    }
}

/// Seconds a note's move takes to come in and go at its ends (no step between
/// notes).
const RAMP: f64 = 0.02;
/// Seconds around what is moved that are worked on too (the grains settle
/// back where they were there).
const MARGIN: f64 = 0.04;
/// Grains where there is no pitch (breaths, consonants): this long a period.
const UNVOICED: f64 = 0.005;
/// The periods it works with (Hz).
const FMIN: f64 = 50.0;
const FMAX: f64 = 1_200.0;

/// `channels` (at `rate`) with each note moved as `edits` say. The same length;
/// where nothing is moved, the same samples.
pub fn render(channels: &[&[f32]], rate: f32, analysis: &Analysis, edits: &[NoteEdit]) -> Vec<Vec<f32>> {
    let mut out: Vec<Vec<f32>> = channels.iter().map(|c| c.to_vec()).collect();
    let len = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    let edits: Vec<NoteEdit> = edits.iter().copied().filter(NoteEdit::changes).collect();
    if edits.is_empty() || len == 0 {
        return out;
    }
    let sr = rate as f64;
    // The shift (semitones) at `seconds`, and the period (samples) there if the voice has one.
    let shift = |seconds: f64| -> f64 {
        let Some(e) = edits.iter().find(|e| e.start <= seconds && seconds < e.end) else { return 0.0 };
        let ramp = ((seconds - e.start) / RAMP).min((e.end - seconds) / RAMP).clamp(0.0, 1.0);
        let moved = match analysis.pitch_at(seconds) {
            Some(p) => e.moved(p) - p,
            None => e.target - e.pitch,
        };
        moved as f64 * ramp
    };
    let period = |at: f64| -> Option<f64> { analysis.pitch_at(at / sr).map(|p| sr / (440.0 * 2f64.powf((p as f64 - 69.0) / 12.0)).clamp(FMIN, FMAX)) };
    let step = |at: f64| period(at).unwrap_or(UNVOICED * sr);
    // What is moved, with room around, joined where they touch.
    let mut regions: Vec<(usize, usize)> = Vec::new();
    for e in &edits {
        let a = ((e.start - MARGIN) * sr).max(0.0) as usize;
        let b = (((e.end + MARGIN) * sr) as usize).min(len);
        match regions.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => regions.push((a, b)),
        }
    }
    for (a, b) in regions {
        // The grains as they are: a period apart.
        let mut marks = vec![a as f64];
        while *marks.last().unwrap() < (b as f64) + sr / FMIN {
            let m = *marks.last().unwrap();
            marks.push(m + step(m));
        }
        let mut sum: Vec<Vec<f64>> = vec![vec![0.0; b - a]; channels.len()];
        let mut weight = vec![0f64; b - a];
        let mut o = a as f64;
        while o < b as f64 {
            // The grain nearest in time.
            let i = marks.partition_point(|&m| m < o);
            let m = match (i.checked_sub(1).map(|j| marks[j]), marks.get(i)) {
                (Some(x), Some(&y)) => if o - x <= y - o { x } else { y },
                (Some(x), None) => x,
                (None, Some(&y)) => y,
                (None, None) => break,
            };
            let half = step(m);
            let (mi, oi, h) = (m.round() as i64, o.round() as i64, half.round() as i64);
            for j in -h + 1..h {
                let (from, to) = (mi + j, oi + j);
                if from < 0 || from as usize >= len || to < a as i64 || to >= b as i64 {
                    continue;
                }
                let w = 0.5 + 0.5 * (std::f64::consts::PI * j as f64 / h as f64).cos();
                let k = (to - a as i64) as usize;
                for (c, channel) in channels.iter().enumerate() {
                    sum[c][k] += channel[from as usize] as f64 * w;
                }
                weight[k] += w;
            }
            let s = shift(o / sr);
            o = if s == 0.0 || period(o).is_none() {
                // Nothing moved here: back onto the grains as they are, so the
                // sound is the same again.
                let next = marks.partition_point(|&x| x <= o + 0.5 * step(o));
                match marks.get(next) {
                    Some(&x) => x,
                    None => break,
                }
            } else {
                o + step(o) / 2f64.powf(s / 12.0)
            };
        }
        for k in 0..b - a {
            if weight[k] > 1e-3 {
                for c in 0..channels.len() {
                    out[c][a + k] = (sum[c][k] / weight[k]) as f32;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// A voice-like tone: `hz(t)` with harmonics, as a vowel through a mouth
    /// that rings at `formant` Hz (each harmonic louder near it).
    fn voice(seconds: f32, hz: impl Fn(f32) -> f32, formant: f32) -> Vec<f32> {
        let mut phase = 0f32;
        let mut f = 0f32;
        (0..(seconds * RATE) as usize)
            .map(|i| {
                f = hz(i as f32 / RATE);
                phase = (phase + f / RATE).fract();
                let mut x = 0.0;
                let mut k = 1;
                while k as f32 * f < 5000.0 {
                    let h = k as f32 * f;
                    let gain = 1.0 / (1.0 + ((h - formant) / 150.0).powi(2));
                    x += gain * (std::f32::consts::TAU * phase * k as f32).sin();
                    k += 1;
                }
                0.15 * x
            })
            .collect()
    }

    /// The pitch (fractional MIDI) of `x` every 10 ms between `from` and
    /// `to` seconds (YIN over 40 ms).
    fn pitches(x: &[f32], from: f32, to: f32) -> Vec<f32> {
        let window = (0.04 * RATE) as usize;
        let mut out = Vec::new();
        let mut at = (from * RATE) as usize;
        while at + window <= (to * RATE) as usize {
            if let Some((hz, _)) = pitch::live(&x[at..at + window], RATE, 60.0, 1000.0) {
                out.push(midi(hz));
            }
            at += (0.01 * RATE) as usize;
        }
        out
    }

    fn sung(x: &[f32], from: f32, to: f32) -> f32 {
        let mut v = pitches(x, from, to);
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    }

    #[test]
    fn a_voice_has_its_notes() {
        let x = voice(1.2, |t| if t < 0.6 { 220.0 * 2f32.powf(0.3 / 12.0) } else { 293.66 }, 700.0);
        let a = analyse(&x, RATE);
        let pitches: Vec<f32> = a.notes.iter().map(|n| (n.pitch * 10.0).round() / 10.0).collect();
        println!("{:?}", a.notes);
        assert_eq!(pitches, vec![57.3, 62.0]);
        assert!(a.notes[0].start < 0.05 && (a.notes[0].end - 0.6).abs() < 0.05);
    }

    /// A note moved a semitone up sounds a semitone up, and the one after it
    /// (untouched) is the same samples.
    #[test]
    fn a_note_moved_sounds_moved_and_the_rest_is_the_same() {
        let x = voice(1.4, |t| if t < 0.7 { 220.0 } else { 261.63 }, 700.0);
        let a = analyse(&x, RATE);
        let first = a.notes[0];
        let edit = NoteEdit { start: first.start, end: first.end, pitch: first.pitch, target: first.pitch + 1.0, modulation: 1.0 };
        let out = render(&[&x], RATE, &a, &[edit]).remove(0);
        assert_eq!(out.len(), x.len());
        let (before, after) = (sung(&x, 0.2, 0.5), sung(&out, 0.2, 0.5));
        println!("{before:.2} → {after:.2}");
        assert!((after - before - 1.0).abs() < 0.03, "{before:.2} → {after:.2}");
        let untouched = ((first.end + MARGIN) * RATE as f64) as usize + 1;
        assert_eq!(&out[untouched..], &x[untouched..], "the second note is untouched");
        // Nothing moved: the same, every sample.
        let same = NoteEdit { target: first.pitch, ..edit };
        assert_eq!(render(&[&x], RATE, &a, &[same])[0], x);
    }

    /// Moved up three semitones, the vowel stays: the loudest harmonic is still
    /// the one nearest its formant (800 Hz), not one moved up with the voice
    /// (as speeding the sound up would: 950 Hz).
    #[test]
    fn the_vowel_stays_where_it_was() {
        let x = voice(1.0, |_| 150.0, 800.0);
        let a = analyse(&x, RATE);
        let n = a.notes[0];
        let out = render(&[&x], RATE, &a, &[NoteEdit { start: n.start, end: n.end, pitch: n.pitch, target: n.pitch + 3.0, modulation: 1.0 }]).remove(0);
        let f0 = 150.0 * 2f32.powf(3.0 / 12.0);
        // Each harmonic's level, by a DFT at it over 0.3 s.
        let part = &out[(0.3 * RATE) as usize..(0.6 * RATE) as usize];
        let level = |hz: f32| {
            let (mut re, mut im) = (0f32, 0f32);
            for (i, s) in part.iter().enumerate() {
                let p = std::f32::consts::TAU * hz * i as f32 / RATE;
                re += s * p.cos();
                im += s * p.sin();
            }
            (re * re + im * im).sqrt()
        };
        let loudest = (1..20).max_by(|&a, &b| level(a as f32 * f0).total_cmp(&level(b as f32 * f0))).unwrap();
        let hz = loudest as f32 * f0;
        println!("loudest harmonic {loudest} at {hz:.0} Hz");
        assert!((hz - 800.0).abs() < f0 * 0.6, "the formant moved to {hz:.0} Hz");
    }

    /// Modulation 0: the vibrato goes (to under a quarter: each grain keeps a
    /// little of the period it had) and the note stays where it was sung.
    #[test]
    fn modulation_takes_the_vibrato_away() {
        let x = voice(1.4, |t| 220.0 * 2f32.powf(0.5 * (std::f32::consts::TAU * 5.5 * t).sin() / 12.0), 700.0);
        let a = analyse(&x, RATE);
        let n = a.notes[0];
        let out = render(&[&x], RATE, &a, &[NoteEdit { start: n.start, end: n.end, pitch: n.pitch, target: n.pitch, modulation: 0.0 }]).remove(0);
        let swing = |y: &[f32]| {
            let v = pitches(y, 0.3, 1.1);
            v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min)
        };
        let (sung, flat) = (swing(&x), swing(&out));
        println!("vibrato {sung:.2} → {flat:.2} semitones");
        assert!(sung > 0.9 && flat < sung / 4.0);
    }

    /// No clicks, the same loudness, and both sides of a stereo voice moved
    /// alike.
    #[test]
    fn no_clicks_and_the_same_level() {
        for hz in [110.0, 196.0, 440.0] {
            let x = voice(1.0, |_| hz, 900.0);
            let right: Vec<f32> = x.iter().map(|s| s * 0.5).collect();
            let a = analyse(&x, RATE);
            let n = a.notes[0];
            let out = render(&[&x, &right], RATE, &a, &[NoteEdit { start: n.start, end: n.end, pitch: n.pitch, target: n.pitch - 2.0, modulation: 1.0 }]);
            let from = (0.2 * RATE) as usize;
            let to = (0.8 * RATE) as usize;
            let rms = |y: &[f32]| (y.iter().map(|s| s * s).sum::<f32>() / y.len() as f32).sqrt();
            let level = 20.0 * (rms(&out[0][from..to]) / rms(&x[from..to])).log10();
            // (To 0.95 s: the voice stops dead where the file does.)
            let step = |y: &[f32]| y[..(0.95 * RATE) as usize].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
            println!("{hz} Hz: level {level:+.2} dB, steps {:.3} / {:.3}", step(&out[0]), step(&x));
            assert!(level.abs() < 1.0, "{hz} Hz: {level:+.2} dB");
            assert!(step(&out[0]) < step(&x) * 1.2, "{hz} Hz: a click");
            for (l, r) in out[0].iter().zip(&out[1]) {
                assert!((l * 0.5 - r).abs() < 1e-5);
            }
        }
    }
}

#[cfg(test)]
mod timing {
    /// How long a minute of voice takes to analyse and to render (run by hand).
    #[test]
    #[ignore]
    fn a_minute_of_voice() {
        let rate = 48_000.0;
        let x: Vec<f32> = (0..(60.0 * rate) as usize).map(|i| { let t = i as f32 / rate; 0.2 * (std::f32::consts::TAU * (200.0 + 40.0 * (t * 0.7).sin()) * t).sin() }).collect();
        let started = std::time::Instant::now();
        let a = super::analyse(&x, rate);
        let analysed = started.elapsed();
        let edits: Vec<super::NoteEdit> = a.notes.iter().map(|n| super::NoteEdit { start: n.start, end: n.end, pitch: n.pitch, target: n.pitch.round(), modulation: 0.5 }).collect();
        let started = std::time::Instant::now();
        let _ = super::render(&[&x, &x], rate, &a, &edits);
        println!("1 min: analysed in {analysed:?}, {} notes all rendered in {:?}", a.notes.len(), started.elapsed());
    }
}
