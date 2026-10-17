//! Audio to MIDI notes with Basic Pitch (Spotify, Apache 2.0; see
//! `models/BASIC_PITCH_NOTICE`), a small network that finds the notes of any
//! instrument or voice, several at once. The model is built into the app.
//!
//! A port of `basic_pitch.inference.predict` with its default settings: the
//! audio at 22050 Hz in overlapping two-second windows, the network's note,
//! onset and contour activations per frame (86 a second, 88 piano keys), and
//! `output_to_notes_polyphonic` turning them into notes.

use crate::{environment, resample};

const MODEL: &[u8] = include_bytes!("../models/basic_pitch.onnx");

const RATE: f32 = 22050.0;
const HOP: usize = 256;
/// Samples the network takes (two seconds less a hop).
const WINDOW: usize = 22050 * 2 - HOP;
/// Frames it gives for them, and how many overlap between windows.
const FRAMES: usize = 172;
const OVERLAP_FRAMES: usize = 30;
const KEYS: usize = 88;
/// MIDI note of the first key.
const LOWEST: u8 = 21;

const ONSET_THRESHOLD: f32 = 0.5;
const FRAME_THRESHOLD: f32 = 0.3;
/// 127.7 ms in frames.
const MIN_NOTE_FRAMES: usize = 11;
/// Frames below the threshold a note may have before it ends.
const ENERGY_TOLERANCE: usize = 11;

/// A note found: from `start` to `end` seconds, its MIDI pitch and how strong
/// it was (0..1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteEvent {
    pub start: f64,
    pub end: f64,
    pub pitch: u8,
    pub amplitude: f32,
}

pub struct Transcriber {
    _environment: ort::environment::Environment,
    session: ort::session::Session,
}

/// Frames × keys activations.
type Activations = Vec<[f32; KEYS]>;

impl Transcriber {
    pub fn load() -> Result<Self, String> {
        let build = || -> ort::Result<(ort::environment::Environment, ort::session::Session)> {
            let environment = environment()?;
            let session = ort::session::Session::builder(&environment)?
                .with_auto_device(ort::session::builder::AutoDevicePolicy::PreferCPU)?
                .with_intra_threads(2)
                .unwrap_or_else(|e| e.recover())
                .commit_from_memory(MODEL)?;
            Ok((environment, session))
        };
        let (_environment, session) = build().map_err(|e| format!("No se pudo cargar el modelo de notas: {e}"))?;
        Ok(Self { _environment, session })
    }

    /// The notes in `mono` (at `rate` Hz). `progress` gets the fraction done and
    /// returns false to stop (then the result is an error).
    pub fn notes(&mut self, mono: &[f32], rate: f32, mut progress: impl FnMut(f32) -> bool) -> Result<Vec<NoteEvent>, String> {
        let audio = resample(mono, rate, RATE);
        let (frames, onsets) = self.activations(&audio, &mut progress)?;
        Ok(to_notes(&frames, onsets))
    }

    /// The network's note and onset activations over the whole audio.
    fn activations(&mut self, audio: &[f32], progress: &mut impl FnMut(f32) -> bool) -> Result<(Activations, Activations), String> {
        let overlap = OVERLAP_FRAMES * HOP;
        let hop = WINDOW - overlap;
        // Half an overlap of silence first, so the first window's trimmed frames are padding.
        let mut padded = vec![0.0f32; overlap / 2];
        padded.extend_from_slice(audio);
        let windows = padded.len().div_ceil(hop).max(1);
        let trim = OVERLAP_FRAMES / 2;
        let (mut frames, mut onsets) = (Vec::new(), Vec::new());
        for w in 0..windows {
            let start = w * hop;
            let mut window: Vec<f32> = padded.get(start..(start + WINDOW).min(padded.len())).unwrap_or(&[]).to_vec();
            window.resize(WINDOW, 0.0);
            let input = ort::value::Tensor::from_array(([1usize, WINDOW, 1], window)).map_err(|e| e.to_string())?;
            let outputs = self.session.run(ort::inputs!["serving_default_input_2:0" => input]).map_err(|e| format!("Error del modelo de notas: {e}"))?;
            let read = |name: &str| -> Result<Vec<f32>, String> {
                let (_, data) = outputs[name].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
                Ok(data.to_vec())
            };
            let (note, onset) = (read("StatefulPartitionedCall:1")?, read("StatefulPartitionedCall:2")?);
            for f in trim..FRAMES - trim {
                let row = |data: &[f32]| -> [f32; KEYS] { std::array::from_fn(|k| data[f * KEYS + k]) };
                frames.push(row(&note));
                onsets.push(row(&onset));
            }
            if !progress((w + 1) as f32 / windows as f32) {
                return Err("Cancelado".into());
            }
        }
        // As long as the audio.
        let length = (audio.len() as f64 * (RATE as usize / HOP) as f64 / RATE as f64).floor() as usize;
        frames.truncate(length);
        onsets.truncate(length);
        Ok((frames, onsets))
    }
}

/// When frame `n` happens, in seconds (`model_frames_to_time`, its window offset included).
fn frame_time(n: usize) -> f64 {
    let window_offset = (HOP as f64 / RATE as f64) * (FRAMES as f64 - WINDOW as f64 / HOP as f64) + 0.0018;
    n as f64 * HOP as f64 / RATE as f64 - window_offset * (n / FRAMES) as f64
}

/// Onsets where the predicted ones are, or where a key's activation jumps
/// (`get_infered_onsets`).
fn inferred_onsets(onsets: &Activations, frames: &Activations) -> Activations {
    let n = frames.len();
    let mut diff: Activations = vec![[0.0; KEYS]; n];
    for t in 0..n {
        for k in 0..KEYS {
            let back = |d: usize| if t >= d { frames[t - d][k] } else { 0.0 };
            let smallest = (frames[t][k] - back(1)).min(frames[t][k] - back(2));
            diff[t][k] = if t < 2 { 0.0 } else { smallest.max(0.0) };
        }
    }
    let max_onset = onsets.iter().flatten().copied().fold(0.0f32, f32::max);
    let max_diff = diff.iter().flatten().copied().fold(0.0f32, f32::max);
    let scale = if max_diff > 0.0 { max_onset / max_diff } else { 0.0 };
    onsets.iter().zip(&diff).map(|(o, d)| std::array::from_fn(|k| o[k].max(d[k] * scale))).collect()
}

/// Notes from the activations (`output_to_notes_polyphonic`, melodia trick included).
fn to_notes(frames: &Activations, onsets: Activations) -> Vec<NoteEvent> {
    let n = frames.len();
    if n < 3 {
        return Vec::new();
    }
    let onsets = inferred_onsets(&onsets, frames);
    // Peaks in time of each key's onsets, latest first (then highest key first).
    let mut peaks = Vec::new();
    for t in 1..n - 1 {
        for k in 0..KEYS {
            let o = onsets[t][k];
            if o > onsets[t - 1][k] && o > onsets[t + 1][k] && o >= ONSET_THRESHOLD {
                peaks.push((t, k));
            }
        }
    }
    peaks.reverse();
    let mut energy = frames.clone();
    let mut events: Vec<(usize, usize, usize)> = Vec::new();
    let clear = |energy: &mut Activations, t: usize, k: usize| {
        energy[t][k] = 0.0;
        if k + 1 < KEYS {
            energy[t][k + 1] = 0.0;
        }
        if k > 0 {
            energy[t][k - 1] = 0.0;
        }
    };
    for (start, k) in peaks {
        if start >= n - 1 {
            continue;
        }
        let (mut i, mut below) = (start + 1, 0);
        while i < n - 1 && below < ENERGY_TOLERANCE {
            below = if energy[i][k] < FRAME_THRESHOLD { below + 1 } else { 0 };
            i += 1;
        }
        i -= below;
        if i - start <= MIN_NOTE_FRAMES {
            continue;
        }
        for t in start..i {
            clear(&mut energy, t, k);
        }
        events.push((start, i, k));
    }
    // The melodia trick: notes from what energy is left, strongest first.
    loop {
        let mut best = (0usize, 0usize, FRAME_THRESHOLD);
        for (t, row) in energy.iter().enumerate() {
            for (k, &e) in row.iter().enumerate() {
                if e > best.2 {
                    best = (t, k, e);
                }
            }
        }
        if best.2 <= FRAME_THRESHOLD {
            break;
        }
        let (middle, k) = (best.0, best.1);
        energy[middle][k] = 0.0;
        let (mut i, mut below) = (middle + 1, 0);
        while i < n - 1 && below < ENERGY_TOLERANCE {
            below = if energy[i][k] < FRAME_THRESHOLD { below + 1 } else { 0 };
            clear(&mut energy, i, k);
            i += 1;
        }
        let end = i - 1 - below;
        let (mut i, mut below) = (middle as isize - 1, 0);
        while i > 0 && below < ENERGY_TOLERANCE {
            below = if energy[i as usize][k] < FRAME_THRESHOLD { below + 1 } else { 0 };
            clear(&mut energy, i as usize, k);
            i -= 1;
        }
        let start = (i + 1 + below as isize) as usize;
        if end <= start || end - start <= MIN_NOTE_FRAMES {
            continue;
        }
        events.push((start, end, k));
    }
    let mut notes: Vec<NoteEvent> = events
        .into_iter()
        .map(|(start, end, k)| NoteEvent {
            start: frame_time(start),
            end: frame_time(end),
            pitch: LOWEST + k as u8,
            amplitude: frames[start..end].iter().map(|row| row[k]).sum::<f32>() / (end - start) as f32,
        })
        .collect();
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn melody() -> (Vec<f32>, f32) {
        let mut reader = hound::WavReader::open(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/melody.wav")).unwrap();
        let rate = reader.spec().sample_rate as f32;
        let samples = reader.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect();
        (samples, rate)
    }

    /// The same notes as Basic Pitch itself (scripts/transcribe/make_reference.py).
    #[test]
    fn finds_the_notes_basic_pitch_finds() {
        let (audio, rate) = melody();
        let mut transcriber = Transcriber::load().unwrap();
        let notes = transcriber.notes(&audio, rate, |_| true).unwrap();
        let reference: Vec<(f64, f64, u8, f32)> = serde_json_reference();
        let found: Vec<(f64, u8)> = notes.iter().map(|n| (n.start, n.pitch)).collect();
        // Every reference note, at the same pitch and times, as strong; nothing else.
        for &(start, end, pitch, amplitude) in &reference {
            let hit = notes.iter().find(|n| n.pitch == pitch && (n.start - start).abs() < 0.001);
            let Some(hit) = hit else { panic!("missing {pitch} at {start}: found {found:?}") };
            assert!((hit.end - end).abs() < 0.001, "{pitch} ends at {} (reference {end})", hit.end);
            assert!((hit.amplitude - amplitude).abs() < 0.005, "{pitch}: {} vs {amplitude}", hit.amplitude);
        }
        assert_eq!(notes.len(), reference.len(), "{found:?}");
        // The melody itself, in order.
        let melody: Vec<u8> = notes.iter().filter(|n| n.start < 2.4).map(|n| n.pitch).collect();
        assert_eq!(melody, vec![60, 64, 67, 72]);
    }

    /// The reference file, read by hand (no JSON crate here): rows of
    /// `[start, end, pitch, amplitude]`.
    fn serde_json_reference() -> Vec<(f64, f64, u8, f32)> {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/melody_notes.json")).unwrap();
        let numbers: Vec<f64> = text.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|s| !s.is_empty()).map(|s| s.parse().unwrap()).collect();
        numbers.chunks(4).map(|c| (c[0], c[1], c[2] as u8, c[3] as f32)).collect()
    }

    #[test]
    fn frame_times_follow_the_models_windows() {
        assert_eq!(frame_time(0), 0.0);
        assert!((frame_time(86) - 86.0 * 256.0 / 22050.0).abs() < 1e-12);
        // From frame 172 on, the window offset is taken off.
        assert!(frame_time(172) < 172.0 * 256.0 / 22050.0);
    }
}
