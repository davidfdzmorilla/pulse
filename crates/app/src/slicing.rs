//! Slicing a loop into drum rack pads: finding where each hit starts, writing the
//! hits as files and building the MIDI clip that plays them back in order.

use crate::i18n::{fill, tr};
use crate::capture::write_wav;
use engine::model::{MidiClip, Note, RackParams, RACK_FIRST_NOTE, RACK_PADS};
use engine::Sample;
use std::path::{Path, PathBuf};
use stems::key::Key;

/// How to cut the loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SliceMode {
    /// At each detected hit.
    Transients,
    /// Every `n` beats.
    Grid(f64),
}

impl SliceMode {
    pub const ALL: [SliceMode; 4] = [Self::Transients, Self::Grid(0.25), Self::Grid(0.5), Self::Grid(1.0)];

    pub fn label(self) -> &'static str {
        match self {
            Self::Transients => tr("Transitorios"),
            Self::Grid(b) if b == 0.25 => "1/16",
            Self::Grid(b) if b == 0.5 => "1/8",
            Self::Grid(b) if b == 1.0 => "1/4",
            Self::Grid(_) => tr("Rejilla"),
        }
    }
}

/// How sensitive «Transitorios» is by default (0..1): as it always was.
pub const DEFAULT_SENSITIVITY: f32 = 0.5;

/// A grid of `beats` as a note value: «1/16», «1/4», «1 compás».
pub fn grid_label(beats: f64) -> String {
    match beats {
        b if (b - 0.25).abs() < 1e-9 => "1/16".into(),
        b if (b - 0.5).abs() < 1e-9 => "1/8".into(),
        b if (b - 1.0).abs() < 1e-9 => "1/4".into(),
        b if (b - 2.0).abs() < 1e-9 => "1/2".into(),
        b if (b - 4.0).abs() < 1e-9 => tr("1 compás").into(),
        b if (b / 4.0).fract().abs() < 1e-9 => fill(tr("{} compases"), &[&(b / 4.0)]),
        b => fill(tr("{} tiempos"), &[&b]),
    }
}

/// «1 trozo», «16 trozos».
pub fn slices_text(n: usize) -> String {
    if n == 1 { tr("1 trozo").into() } else { fill(tr("{} trozos"), &[&n]) }
}

/// Analysis hop (about 5 ms at 48 kHz) and spectral frame size.
const HOP: usize = 256;
const FRAME: usize = 1024;
/// Hits closer than this are treated as one.
const MIN_GAP_SECONDS: f32 = 0.06;

pub(crate) use engine::fft;

/// Log-spaced analysis bands, as FFT bin ranges.
const BANDS: usize = 24;

/// Onset strength per hop: how much the level in each band rises from one frame
/// to the next, in dB, summed over the bands. Summing bins into bands averages out
/// the random bin-to-bin fluctuation of noisy sounds (snares, hats), so a hat on
/// top of a snare's tail still stands out.
fn onset_strength(left: &[f32], right: &[f32]) -> Vec<f32> {
    let frames = left.len().min(right.len());
    let window: Vec<f32> = (0..FRAME).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FRAME as f32).cos()).collect();
    let edges: Vec<usize> = (0..=BANDS).map(|b| (2.0 * (256f32).powf(b as f32 / BANDS as f32)).round() as usize).collect();
    let (mut re, mut im) = (vec![0f32; FRAME], vec![0f32; FRAME]);
    let mut levels: Vec<[f32; BANDS]> = Vec::with_capacity(frames / HOP + 1);
    // Frames are centred on each hop, so a hit shows up at the hop where it starts.
    for h in 0..frames.div_ceil(HOP) {
        let centre = (h * HOP) as isize;
        for i in 0..FRAME {
            let at = centre - (FRAME / 2) as isize + i as isize;
            let x = if at >= 0 && (at as usize) < frames { (left[at as usize] + right[at as usize]) * 0.5 } else { 0.0 };
            re[i] = x * window[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im);
        let mut bands = [0f32; BANDS];
        for (b, band) in bands.iter_mut().enumerate() {
            let (lo, hi) = (edges[b], edges[b + 1].max(edges[b] + 1).min(FRAME / 2));
            *band = (lo..hi).map(|k| re[k] * re[k] + im[k] * im[k]).sum::<f32>() / (hi - lo) as f32;
        }
        levels.push(bands);
    }
    // dB with a floor 60 dB under the loudest band, so silence does not count.
    let loudest = levels.iter().flatten().copied().fold(1e-20, f32::max);
    let floor = loudest * 1e-6;
    let db = |e: f32| 10.0 * (e.max(floor) / loudest).log10();
    let mut flux = vec![0.0; levels.len()];
    for h in 1..levels.len() {
        // Treble bands count more: hats and cymbals carry little energy but sharp attacks.
        flux[h] = (0..BANDS).map(|b| (db(levels[h][b]) - db(levels[h - 1][b])).max(0.0) * (0.5 + 1.5 * b as f32 / BANDS as f32)).sum();
    }
    // The very start counts as a hit when it is not silent (an empty file, a
    // missing one, has none).
    if let Some(first) = levels.first() {
        flux[0] = (0..BANDS).map(|b| db(first[b]) + 60.0).sum();
    }
    flux
}

/// Finds where `sample`'s hits start, once, for the Beats warp mode (see
/// `engine::Sample::onsets`).
pub fn find_onsets(sample: &engine::Sample) {
    sample.onsets.get_or_init(|| transients(&sample.left, &sample.right, sample.sample_rate, usize::MAX).into_iter().map(|f| f as f64 / sample.sample_rate as f64).collect());
}

/// Frame offsets where hits start in `left`/`right`, at most `max`, always
/// including 0. Peaks of the spectral flux above a local, adaptive threshold.
pub fn transients(left: &[f32], right: &[f32], sample_rate: f32, max: usize) -> Vec<usize> {
    transients_at(left, right, sample_rate, max, DEFAULT_SENSITIVITY)
}

/// [`transients`] with a `sensitivity` (0..1): at 1 the threshold is a quarter
/// of the usual one (softer hits count), at 0 four times it (only the hardest).
pub fn transients_at(left: &[f32], right: &[f32], sample_rate: f32, max: usize, sensitivity: f32) -> Vec<usize> {
    let flux = onset_strength(left, right);
    let scale = 4f32.powf(1.0 - 2.0 * sensitivity.clamp(0.0, 1.0));
    if flux.len() < 4 || max <= 1 {
        return vec![0];
    }
    let loudest = flux.iter().copied().fold(0.0, f32::max);
    let mean = flux.iter().sum::<f32>() / flux.len() as f32;
    let threshold = |h: usize| {
        let around = &flux[h.saturating_sub(10)..(h + 10).min(flux.len())];
        let mut sorted = around.to_vec();
        sorted.sort_by(f32::total_cmp);
        (sorted[sorted.len() / 2] * 1.5 + mean * 0.5 + loudest * 0.05) * scale
    };
    let mut candidates: Vec<(usize, f32)> = (1..flux.len() - 1)
        .filter(|&h| flux[h] > threshold(h) && (h.saturating_sub(3)..=(h + 3).min(flux.len() - 1)).all(|k| flux[k] <= flux[h]))
        .map(|h| (h, flux[h]))
        .collect();
    // Strongest first; drop hits too close to a stronger one, then keep the best.
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    let gap = (MIN_GAP_SECONDS * sample_rate / HOP as f32).ceil() as usize;
    let mut kept: Vec<usize> = Vec::new();
    for (h, _) in candidates {
        if h >= gap && kept.iter().all(|&k| k.abs_diff(h) >= gap) {
            kept.push(h);
            if kept.len() + 1 >= max {
                break;
            }
        }
    }
    let mut starts: Vec<usize> = kept.into_iter().map(|h| refine_onset(left, right, h)).collect();
    starts.push(0);
    starts.sort_unstable();
    starts.dedup();
    starts
}

/// Moves a hop-resolution onset to the biggest rise in level around it, measured
/// on the high-passed signal so a kick's low swing does not mask a hat's attack.
fn refine_onset(left: &[f32], right: &[f32], hop: usize) -> usize {
    const BLOCK: usize = 32;
    let from = (hop * HOP).saturating_sub(FRAME / 2);
    let to = (hop * HOP + FRAME / 2).min(left.len());
    let energy = |b: usize| -> f32 {
        let start = from + b * BLOCK;
        (start.max(1)..(start + BLOCK).min(to))
            .map(|i| {
                let (l, r) = (left[i] - left[i - 1], right[i] - right[i - 1]);
                l * l + r * r
            })
            .sum()
    };
    let blocks = (to - from) / BLOCK;
    let rise = |k: usize| energy(k) - energy(k - 1);
    let best = (1..blocks).max_by(|&a, &b| rise(a).total_cmp(&rise(b)));
    best.map_or(hop * HOP, |b| from + b * BLOCK).saturating_sub(BLOCK / 2)
}

/// Cuts every `grid` beats; the grid doubles until the loop fits in `max` slices.
/// Returns the frame offsets and the grid actually used.
pub fn grid(frames: usize, frames_per_beat: f64, mut grid: f64, max: usize) -> (Vec<usize>, f64) {
    let beats = frames as f64 / frames_per_beat;
    while (beats / grid).ceil() as usize > max {
        grid *= 2.0;
    }
    let starts = (0..(beats / grid).ceil() as usize).map(|i| (i as f64 * grid * frames_per_beat).round() as usize).filter(|&f| f < frames).collect();
    (starts, grid)
}

/// The result of slicing: files for the pads, rack settings and a clip that plays the loop.
pub struct Sliced {
    pub files: Vec<String>,
    pub params: RackParams,
    pub clip: MidiClip,
}

/// The frames of `sample` from `offset` for `seconds` (clamped to the file), if
/// long enough to slice.
fn region(sample: &Sample, (offset, seconds): (f64, f64)) -> Result<(usize, usize), String> {
    let rate = sample.sample_rate as f64;
    let first = ((offset.max(0.0) * rate) as usize).min(sample.frames());
    let last = (((offset + seconds) * rate) as usize).clamp(first, sample.frames());
    if last - first < 64 {
        return Err(tr("La región es demasiado corta para trocearla").into());
    }
    Ok((first, last))
}

/// Where `mode` cuts `seconds` of `sample` from `offset` (tempo `bpm`): points of
/// the file in seconds, and the grid actually used (0 for transients). By
/// transients, with `sensitivity` (0..1), and where the chord changes.
pub fn find_cuts(sample: &Sample, (offset, seconds): (f64, f64), bpm: f64, mode: SliceMode, sensitivity: f32) -> Result<(Vec<f64>, f64), String> {
    let (first, last) = region(sample, (offset, seconds))?;
    let rate = sample.sample_rate as f64;
    let (left, right) = (&sample.left[first..last], &sample.right[first..last]);
    let (starts, used) = match mode {
        SliceMode::Transients => {
            let mut starts = transients_at(left, right, sample.sample_rate, RACK_PADS, sensitivity);
            let gap = (MIN_GAP_SECONDS as f64 * rate) as usize;
            for change in chord_changes(left, right, sample.sample_rate, rate * 60.0 / bpm) {
                if starts.len() < RACK_PADS && starts.iter().all(|&s| s.abs_diff(change) >= gap) {
                    starts.push(change);
                }
            }
            starts.sort_unstable();
            (starts, 0.0)
        }
        SliceMode::Grid(beats) => grid(left.len(), rate * 60.0 / bpm, beats, RACK_PADS),
    };
    Ok((starts.into_iter().map(|f| (first + f) as f64 / rate).collect(), used))
}

/// Where the chord changes in a tonal loop (frame offsets, on beats, moved to
/// the attack right there): a pad held across a bar line has no hit, but the
/// new chord is a slice of its own. None in drums.
fn chord_changes(left: &[f32], right: &[f32], sample_rate: f32, frames_per_beat: f64) -> Vec<usize> {
    let mono: Vec<f32> = left.iter().zip(right).map(|(l, r)| (l + r) * 0.5).collect();
    let Some(key) = tonal_key(&mono, sample_rate) else { return Vec::new() };
    let gram = stems::chord::Chromagram::new(&mono, sample_rate);
    let rate = sample_rate as f64;
    let beats = (mono.len() as f64 / frames_per_beat).floor() as usize;
    let stretches: Vec<(f64, f64)> = (0..beats).map(|b| (b as f64 * frames_per_beat / rate, (b + 1) as f64 * frames_per_beat / rate)).collect();
    let runs = gram.chords(&stretches, Some(key));
    // A chord that holds two beats at least: not a passing note.
    let beat_seconds = frames_per_beat / rate;
    runs.windows(2)
        .filter(|w| w[0].2.is_some() && w[1].2.is_some() && w[1].1 - w[1].0 >= 2.0 * beat_seconds - 1e-6)
        .map(|w| refine_onset(left, right, ((w[1].0 * rate) as usize / HOP).max(1)))
        .filter(|&f| f > 0 && f < mono.len())
        .collect()
}

/// The key of `mono` if it plainly has one (a chord loop, a melody), not drums.
fn tonal_key(mono: &[f32], sample_rate: f32) -> Option<Key> {
    Key::detect_clearly(&stems::key::chroma(mono, sample_rate)).filter(|(_, fit)| *fit >= TONAL_FIT).map(|(key, _)| key)
}

/// How clearly (see `Key::detect_clearly`) a loop must fit a key to be tonal.
const TONAL_FIT: f64 = 0.6;

/// Slices `seconds` of `sample` from `offset`, whose tempo is `bpm`, into files
/// in `dir`. `key` (the project's) spells the chords in the pads' names.
#[allow(clippy::too_many_arguments)]
pub fn slice(sample: &Sample, region: (f64, f64), bpm: f64, mode: SliceMode, sensitivity: f32, name: &str, dir: &Path, color: [u8; 3], key: Option<Key>) -> Result<(Sliced, f64), String> {
    let (cuts, used) = find_cuts(sample, region, bpm, mode, sensitivity)?;
    Ok((slice_at(sample, region, bpm, &cuts, name, dir, color, key)?, used))
}

/// A beat near a sixteenth (within 20 ms and a 25th of a beat) put on it: the
/// cut's attack was found a few milliseconds off, or the file's tempo is not exact.
fn snap_to_grid(beat: f64, bpm: f64) -> f64 {
    let sixteenth = (beat * 4.0).round() / 4.0;
    let near = (0.04f64).min(0.02 * bpm / 60.0);
    if (beat - sixteenth).abs() <= near { sixteenth } else { beat }
}

/// A pad's name: its number and the chord it plays («05 · Mib»), or where it
/// starts in the loop, bar and beat in 4/4 («05 · 2.1», «06 · 2.1.3» on the
/// third sixteenth).
fn pad_name(pad: usize, beat: f64, chord: Option<String>) -> String {
    if let Some(chord) = chord {
        return format!("{:02} · {chord}", pad + 1);
    }
    let sixteenths = (beat * 4.0).round().max(0.0) as usize;
    let (bar, beat, sixteenth) = (sixteenths / 16 + 1, sixteenths / 4 % 4 + 1, sixteenths % 4 + 1);
    if sixteenth == 1 { format!("{:02} · {bar}.{beat}", pad + 1) } else { format!("{:02} · {bar}.{beat}.{sixteenth}", pad + 1) }
}

/// Slices `seconds` of `sample` from `offset` at `cuts` (points of the file, in
/// seconds; at most one per pad), each slice up to the next cut. The pads are
/// named by number and chord (spelt in `key`, or the loop's own) or position.
#[allow(clippy::too_many_arguments)]
pub fn slice_at(sample: &Sample, (offset, seconds): (f64, f64), bpm: f64, cuts: &[f64], name: &str, dir: &Path, color: [u8; 3], key: Option<Key>) -> Result<Sliced, String> {
    let (first, last) = region(sample, (offset, seconds))?;
    let rate = sample.sample_rate as f64;
    let (left, right) = (&sample.left[first..last], &sample.right[first..last]);
    let frames_per_beat = rate * 60.0 / bpm;
    let mut starts: Vec<usize> = cuts.iter().map(|&c| ((c * rate) as usize).saturating_sub(first)).filter(|&f| f < left.len()).collect();
    starts.sort_unstable();
    starts.dedup();
    starts.truncate(RACK_PADS);
    if starts.is_empty() {
        return Err(tr("No hay ningún corte dentro de la región del clip").into());
    }

    std::fs::create_dir_all(dir).map_err(|e| fill(tr("No se pudo crear {}: {}"), &[&dir.display(), &e]))?;
    // A chord loop's pads are named by their chord, spelt in the project's key.
    let mono: Vec<f32> = left.iter().zip(right).map(|(l, r)| (l + r) * 0.5).collect();
    let tonal = tonal_key(&mono, sample.sample_rate);
    let gram = tonal.map(|_| stems::chord::Chromagram::new(&mono, sample.sample_rate));
    let spelling = key.or(tonal);
    let beat_of = |frame: usize| snap_to_grid(frame as f64 / frames_per_beat, bpm);
    let fade = (rate * 0.003) as usize;
    let mut files = vec![String::new(); RACK_PADS];
    let length = (left.len() as f64 / frames_per_beat * 4.0).round().max(1.0) / 4.0;
    let mut clip = MidiClip::new(name, length, color);
    for (pad, &start) in starts.iter().enumerate() {
        let end = starts.get(pad + 1).copied().unwrap_or(left.len());
        let (mut l, mut r) = (left[start..end].to_vec(), right[start..end].to_vec());
        // A short fade so the cut where the next hit begins does not click.
        let n = l.len();
        for i in 0..fade.min(n) {
            let gain = i as f32 / fade as f32;
            l[n - 1 - i] *= gain;
            r[n - 1 - i] *= gain;
        }
        let beat = beat_of(start);
        let next = beat_of(end);
        let chord = gram.as_ref().and_then(|g| {
            let (weights, bass) = g.between(start as f64 / rate, end as f64 / rate);
            // Without its bass (short, and no `/` in a file name).
            stems::chord::Chord::detect(&weights, bass, tonal).map(|c| stems::chord::Chord { bass: None, ..c }.name_in(spelling))
        });
        // In the folder of the clip's slices, named so the rows of the editor
        // tell them apart (the clip's name is the project's text: kept out).
        let path: PathBuf = dir.join(format!("{}.wav", crate::files::safe_name(&pad_name(pad, beat, chord))));
        write_wav(&path, &l, &r, sample.sample_rate)?;
        files[pad] = path.to_string_lossy().into_owned();
        clip.notes.push(Note { start: beat, length: (next - beat).max(1.0 / 32.0), pitch: RACK_FIRST_NOTE + pad as u8, velocity: 1.0, chance: 1.0 });
    }
    while files.last().is_some_and(String::is_empty) {
        files.pop();
    }
    let mut params = RackParams::default();
    // Slices are one voice: a new hit cuts the previous one, like the loop itself.
    params.choke = [1; RACK_PADS];
    params.level = [1.0; RACK_PADS];
    Ok(Sliced { files, params, clip })
}

/// The cuts shown (and edited by hand) for a clip: points of its file, in
/// seconds. `key` is what they were found for (the file, its region, the mode):
/// when it changes they are found again.
pub struct SliceCuts {
    pub clip: crate::app::ClipRef,
    key: (String, u64, u64, &'static str, u32),
    pub seconds: Vec<f64>,
}

/// The part of the file an audio clip plays (offset, seconds) and its average tempo.
pub fn clip_region(audio: &engine::model::AudioClip, tempo: f64) -> ((f64, f64), f64) {
    let region = (audio.offset, audio.source_seconds(audio.length, tempo) - audio.offset);
    (region, audio.length * 60.0 / region.1.max(1e-6))
}

impl crate::app::PulseApp {
    /// Where the clip will be sliced: found by the slicing mode, or as edited.
    pub fn cuts_for(&mut self, clip_ref: crate::app::ClipRef) -> Option<Vec<f64>> {
        let Some(engine::model::Clip::Audio(audio)) = self.clip(clip_ref).cloned() else { return None };
        let (region, bpm) = clip_region(&audio, self.project.tempo);
        let key = (audio.sample.clone(), region.0.to_bits(), region.1.to_bits(), self.slice_mode.label(), self.slice_sensitivity.to_bits());
        if let Some(cuts) = self.slice_cuts.as_ref().filter(|c| c.clip == clip_ref && c.key == key) {
            return Some(cuts.seconds.clone());
        }
        let sample = self.sample(&audio.sample);
        let (seconds, _) = find_cuts(&sample, region, bpm, self.slice_mode, self.slice_sensitivity).ok()?;
        self.slice_cuts = Some(SliceCuts { clip: clip_ref, key, seconds: seconds.clone() });
        Some(seconds)
    }

    /// The clip's cuts as edited by hand.
    pub fn set_cuts(&mut self, clip_ref: crate::app::ClipRef, mut seconds: Vec<f64>) {
        seconds.sort_by(f64::total_cmp);
        if let Some(cuts) = self.slice_cuts.as_mut().filter(|c| c.clip == clip_ref) {
            cuts.seconds = seconds;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A missing or empty file has no hits, and no crash (a drum loop in Beats
    /// whose file is gone).
    #[test]
    fn an_empty_file_has_one_hit_at_the_start() {
        assert_eq!(transients(&[], &[], 48_000.0, usize::MAX), vec![0]);
        assert!(onset_strength(&[], &[]).is_empty());
    }

    const SR: f32 = 48_000.0;

    /// A clip's name (text of the project, maybe someone else's) never takes
    /// the slices out of their folder.
    #[test]
    fn slices_stay_in_their_folder_whatever_the_name() {
        let root = std::env::temp_dir().join(format!("pulse-slices-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("Proyecto/Trozos");
        let sample = Sample::from_channels(hits(&[0.0, 0.5], 1.0), hits(&[0.0, 0.5], 1.0), SR);
        for name in ["../../Fuera/escrito", "..", "a\\..\\..\\b", "con: raros*?"] {
            let sliced = slice_at(&sample, (0.0, 1.0), 120.0, &[0.0, 0.5], name, &dir, [1, 2, 3], None).unwrap();
            for path in &sliced.files {
                assert_eq!(Path::new(path).parent(), Some(dir.as_path()), "{name}: {path}");
            }
        }
        assert!(!root.join("Fuera").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    /// Decaying noise bursts at the given seconds, over a quiet sine "bass".
    fn hits(at: &[f32], seconds: f32) -> Vec<f32> {
        let mut seed = 1u32;
        let mut noise = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32 * 2.0 - 1.0
        };
        let mut out: Vec<f32> = (0..(seconds * SR) as usize).map(|i| (i as f32 * 55.0 * std::f32::consts::TAU / SR).sin() * 0.1).collect();
        for &t in at {
            let start = (t * SR) as usize;
            for i in 0..(0.15 * SR) as usize {
                if let Some(s) = out.get_mut(start + i) {
                    *s += noise() * 0.8 * (-(i as f32) / (0.03 * SR)).exp();
                }
            }
        }
        out
    }

    #[test]
    fn transients_find_each_hit() {
        let at = [0.0, 0.25, 0.5, 0.625, 1.0, 1.5];
        let audio = hits(&at, 2.0);
        let found = transients(&audio, &audio, SR, 16);
        assert_eq!(found.len(), at.len(), "{found:?}");
        for (&frame, &t) in found.iter().zip(&at) {
            let error = (frame as f32 / SR - t).abs();
            assert!(error < 0.004, "hit at {t} s found at {} s", frame as f32 / SR);
        }
    }

    #[test]
    fn transients_keep_the_strongest_when_there_are_too_many() {
        let at: Vec<f32> = (0..24).map(|i| i as f32 * 0.125).collect();
        let audio = hits(&at, 3.0);
        let found = transients(&audio, &audio, SR, 16);
        assert_eq!(found.len(), 16);
        assert_eq!(found[0], 0);
    }

    /// Held chords, `bar` seconds each, gliding from one to the next with no
    /// attack (a pad): each chord's MIDI notes.
    fn pad(chords: &[&[u8]], bar: f32) -> Vec<f32> {
        let len = (bar * SR) as usize;
        let fade = (0.03 * SR) as usize;
        let mut out = vec![0f32; len * chords.len()];
        for (c, notes) in chords.iter().enumerate() {
            for i in 0..len + fade {
                let at = c * len + i;
                let Some(s) = out.get_mut(at) else { break };
                // In and out over the crossfade, but the first chord's start.
                let gain = if c > 0 && i < fade { i as f32 / fade as f32 } else if i >= len { 1.0 - (i - len) as f32 / fade as f32 } else { 1.0 };
                let t = at as f32 / SR;
                for &n in notes.iter() {
                    let hz = 440.0 * 2f32.powf((n as f32 - 69.0) / 12.0);
                    for h in 1..=3 {
                        *s += gain * 0.08 / h as f32 * (std::f32::consts::TAU * hz * h as f32 * t).sin();
                    }
                }
            }
        }
        out
    }

    /// A pad of four chords with no hits: cut where each chord comes in, its
    /// pads named after them in F minor (Reb, not Do#), on the beat exactly.
    #[test]
    fn a_chord_loop_is_cut_where_the_chord_changes() {
        // Fm, Eb, Db, C (the Andalusian cadence), a bar each at 120 BPM.
        let audio = pad(&[&[53, 56, 60], &[51, 55, 58], &[49, 53, 56], &[48, 52, 55]], 2.0);
        let sample = Sample::from_channels(audio.clone(), audio, SR);
        let (cuts, _) = find_cuts(&sample, (0.0, 8.0), 120.0, SliceMode::Transients, DEFAULT_SENSITIVITY).unwrap();
        let bars: Vec<f64> = cuts.iter().map(|c| (c / 2.0 * 100.0).round() / 100.0).collect();
        assert_eq!(bars, [0.0, 1.0, 2.0, 3.0], "{cuts:?}");
        let dir = std::env::temp_dir().join(format!("pulse-slices-chords-{}", std::process::id()));
        let f_minor = Some(Key { tonic: 5, minor: true });
        let (sliced, _) = slice(&sample, (0.0, 8.0), 120.0, SliceMode::Transients, DEFAULT_SENSITIVITY, "Acordes", &dir, [1, 2, 3], f_minor).unwrap();
        let names: Vec<String> = sliced.files.iter().map(|f| Path::new(f).file_stem().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["01 · Fam", "02 · Mib", "03 · Reb", "04 · Do"]);
        let starts: Vec<f64> = sliced.clip.notes.iter().map(|n| n.start).collect();
        assert_eq!(starts, [0.0, 4.0, 8.0, 12.0], "on the grid exactly");
        assert!(sliced.clip.notes.iter().all(|n| n.length == 4.0));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Less sensitive, only the hardest hits and the chord changes cut; more,
    /// the softer ones too (and, all the way up, the smallest changes).
    #[test]
    fn the_sensitivity_decides_how_soft_a_hit_still_cuts() {
        // Two held chords, a bar each at 120 BPM, and soft hits on the other beats.
        let mut audio = pad(&[&[53, 56, 60], &[51, 55, 58]], 2.0);
        let soft = hits(&[0.5, 1.0, 1.5, 2.5, 3.0, 3.5], 4.0);
        for (a, s) in audio.iter_mut().zip(&soft) {
            *a += s * 0.02;
        }
        let sample = Sample::from_channels(audio.clone(), audio, SR);
        let cuts = |s: f32| find_cuts(&sample, (0.0, 4.0), 120.0, SliceMode::Transients, s).unwrap().0;
        let beats = |cuts: Vec<f64>| cuts.iter().map(|c| (c * 2.0 * 10.0).round() / 10.0).collect::<Vec<f64>>();
        assert_eq!(beats(cuts(0.0)), [0.0, 4.0], "the chords only");
        assert_eq!(beats(cuts(DEFAULT_SENSITIVITY)), [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0], "and the soft hits");
        assert!(cuts(1.0).len() > 8);
    }

    /// Cut on a grid at a tempo whose beats are no whole number of samples: the
    /// notes are on the grid all the same, and the status says «tiempo», not «beats».
    #[test]
    fn slices_land_on_the_grid() {
        let audio = hits(&[0.0], 60.0 / 140.0 * 16.0);
        let sample = Sample::from_channels(audio.clone(), audio, SR);
        let dir = std::env::temp_dir().join(format!("pulse-slices-grid-{}", std::process::id()));
        let seconds = sample.duration();
        let (sliced, used) = slice(&sample, (0.0, seconds), 140.0, SliceMode::Grid(1.0), DEFAULT_SENSITIVITY, "Loop", &dir, [1, 2, 3], None).unwrap();
        assert_eq!(used, 1.0);
        let starts: Vec<f64> = sliced.clip.notes.iter().map(|n| n.start).collect();
        assert_eq!(starts, (0..16).map(f64::from).collect::<Vec<_>>());
        assert!(sliced.clip.notes.iter().all(|n| n.length == 1.0), "{:?}", sliced.clip.notes);
        let names: Vec<String> = sliced.files.iter().take(6).map(|f| Path::new(f).file_stem().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["01 · 1.1", "02 · 1.2", "03 · 1.3", "04 · 1.4", "05 · 2.1", "06 · 2.2"]);
        assert_eq!((grid_label(0.5), grid_label(4.0), grid_label(8.0)), ("1/8".to_string(), "1 compás".to_string(), "2 compases".to_string()));
        assert_eq!((slices_text(1), slices_text(16)), ("1 trozo".to_string(), "16 trozos".to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_grid_widens_to_fit_the_pads() {
        // Two bars at 120 BPM: 32 sixteenths don't fit, 16 eighths do.
        let (starts, used) = grid(192_000, 24_000.0, 0.25, 16);
        assert_eq!(used, 0.5);
        assert_eq!(starts.len(), 16);
        assert_eq!(starts[1], 12_000);
    }

    #[test]
    fn slicing_writes_pads_and_a_clip_that_replays_the_loop() {
        let audio = hits(&[0.0, 0.5, 1.0, 1.5], 2.0);
        let sample = Sample::from_channels(audio.clone(), audio, SR);
        let dir = std::env::temp_dir().join(format!("pulse-slices-{}", std::process::id()));
        let (sliced, _) = slice(&sample, (0.0, 2.0), 120.0, SliceMode::Transients, DEFAULT_SENSITIVITY, "Break", &dir, [1, 2, 3], None).unwrap();
        assert_eq!(sliced.files.len(), 4);
        // Named by number and where each starts (a break has no chords).
        assert!(sliced.files[3].ends_with("04 · 1.4.wav"), "{}", sliced.files[3]);
        assert_eq!(sliced.clip.length, 4.0, "2 s at 120 BPM");
        for (i, note) in sliced.clip.notes.iter().enumerate() {
            assert!((note.start - i as f64).abs() < 0.02, "slice {i} at beat {}", note.start);
            assert_eq!(note.pitch, RACK_FIRST_NOTE + i as u8);
        }
        let pad = Sample::load(Path::new(&sliced.files[1])).unwrap();
        assert!((pad.duration() - 0.5).abs() < 0.01);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn transients_find_a_drum_machine_beat() {
        use engine::model::{drum_note::*, DrumParams};
        // One bar at 120 BPM in eighths. A hat right after the snare is left out: it
        // is as loud as the snare's tail and indistinguishable from it.
        let pattern = [KICK, CLOSED_HAT, SNARE, CLAP, KICK, CLOSED_HAT, OPEN_HAT, SNARE];
        let eighth = 12_000;
        let (mut left, mut right) = (vec![0.0f32; eighth * 8], vec![0.0f32; eighth * 8]);
        for (step, &sound) in pattern.iter().enumerate() {
            let (l, r) = engine::render_drum_hit(DrumParams::default(), sound, SR);
            for (i, (a, b)) in l.iter().zip(&r).enumerate().take(left.len() - step * eighth) {
                left[step * eighth + i] += a;
                right[step * eighth + i] += b;
            }
        }
        let found = transients(&left, &right, SR, 16);
        let steps: Vec<usize> = found.iter().map(|&f| (f as f32 / eighth as f32).round() as usize).collect();
        assert_eq!(steps, (0..8).collect::<Vec<_>>(), "found at {found:?}");
        for (&frame, step) in found.iter().zip(0..) {
            assert!((frame as i64 - step * eighth as i64).abs() < 200, "step {step} at {frame}");
        }
    }

    /// Cuts edited by hand: one taken out, one moved; slicing then follows them.
    #[test]
    fn hand_edited_cuts_decide_the_slices() {
        use crate::app::{ClipRef, PulseApp};
        use engine::model::{AudioClip, Clip, WarpMode};
        let audio = hits(&[0.0, 0.5, 1.0, 1.5], 2.0);
        let dir = std::env::temp_dir().join(format!("pulse-cuts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Break.wav");
        write_wav(&path, &audio, &audio, SR).unwrap();
        let mut app = PulseApp::headless(crate::project::Project::demo());
        app.project.tempo = 120.0;
        let t = app.add_audio_track().unwrap();
        app.project.tracks[t].clips[0] = Some(Clip::Audio(AudioClip { hard_start: false, fades: Default::default(),
            name: "Break".into(),
            color: [1, 2, 3],
            sample: path.to_string_lossy().into_owned(),
            length: 4.0,
            offset: 0.0,
            warp: WarpMode::Repitch,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(), muted: false,
        }));
        let clip = ClipRef::Session { track: t, slot: 0 };
        app.show_cuts = true;
        let found = app.cuts_for(clip).unwrap();
        assert_eq!(found.len(), 4, "{found:?}");
        // Without the second hit, and the third a quarter of a second later.
        app.set_cuts(clip, vec![found[0], found[2] + 0.25, found[3]]);
        assert_eq!(app.cuts_for(clip).unwrap().len(), 3, "the edit is kept");
        app.slice_clip(clip);
        let rack = app.project.tracks.len() - 1;
        let Some(Clip::Midi(midi)) = &app.project.tracks[rack].clips[0] else { panic!("{:?}", app.status) };
        let starts: Vec<f64> = midi.notes.iter().map(|n| (n.start * 4.0).round() / 4.0).collect();
        assert_eq!(starts, vec![0.0, 2.5, 3.0], "beats at 120 BPM");
        assert_eq!(app.project.tracks[rack].name, "Break troceado");
        assert_eq!(app.status.as_deref(), Some("Break: 3 trozos en un Drum Pads nuevo"));
        // Another mode finds its own cuts again.
        app.slice_mode = SliceMode::Grid(1.0);
        assert_eq!(app.cuts_for(clip).unwrap().len(), 4);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A chord loop in F minor sliced by quarters, its pads in the editor, and
    /// the audio editor with the sensitivity:
    /// `PULSE_SHOTS=dir cargo test --release -p app slice_shots -- --ignored`.
    #[test]
    #[ignore]
    fn slice_shots() {
        use crate::app::{ClipRef, PulseApp};
        use engine::model::{AudioClip, Clip, WarpMode};
        let Some(dir) = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from) else { return };
        let media = std::env::temp_dir().join(format!("pulse-slice-shots-{}", std::process::id()));
        std::fs::create_dir_all(&media).unwrap();
        let path = media.join("Fam Mib Reb Do.wav");
        let audio = pad(&[&[53, 56, 60], &[51, 55, 58], &[49, 53, 56], &[48, 52, 55]], 2.0);
        write_wav(&path, &audio, &audio, SR).unwrap();
        let mut app = PulseApp::headless(crate::project::Project::empty());
        app.project.tempo = 120.0;
        app.project.key = Some(Key { tonic: 5, minor: true });
        let t = app.add_audio_track().unwrap();
        app.project.tracks[t].clips[0] = Some(Clip::Audio(AudioClip { hard_start: false, fades: Default::default(),
            name: "Fam Mib Reb Do".into(), color: [180, 200, 40], sample: path.to_string_lossy().into_owned(), length: 16.0, offset: 0.0,
            warp: WarpMode::Repitch, original_bpm: 120.0, warp_markers: Vec::new(), takes: Vec::new(), key: None, gain_db: 0.0, transpose: 0.0,
            envelopes: Vec::new(), muted: false,
        }));
        app.sync_all();
        let clip = ClipRef::Session { track: t, slot: 0 };
        app.select_clip(clip);
        let mut s = crate::produccion::Studio::new(app, dir);
        s.frames(6);
        s.shot("editor-de-audio-trocear");
        s.app.slice_mode = SliceMode::Grid(1.0);
        s.app.slice_clip(clip);
        s.frames(6);
        s.shot("trozos-en-el-editor");
        std::fs::remove_dir_all(&media).ok();
    }

    /// How a file is cut at each sensitivity, and the pads' names:
    /// `PULSE_SLICE=file.wav PULSE_BPM=140 cargo test --release -p app slice_a_file -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn slice_a_file() {
        let Some(path) = std::env::var_os("PULSE_SLICE") else { return };
        let bpm: f64 = std::env::var("PULSE_BPM").ok().and_then(|b| b.parse().ok()).unwrap_or(120.0);
        let sample = Sample::load(Path::new(&path)).unwrap();
        let region = (0.0, sample.duration());
        let key = Some(Key { tonic: 5, minor: true });
        for sensitivity in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let (cuts, _) = find_cuts(&sample, region, bpm, SliceMode::Transients, sensitivity).unwrap();
            let beats: Vec<String> = cuts.iter().map(|c| format!("{:.2}", c * bpm / 60.0)).collect();
            println!("sensibilidad {sensitivity}: {} cortes en {}", cuts.len(), beats.join(" "));
        }
        let dir = std::env::temp_dir().join(format!("pulse-slice-file-{}", std::process::id()));
        for mode in [SliceMode::Transients, SliceMode::Grid(1.0)] {
            let (sliced, _) = slice(&sample, region, bpm, mode, DEFAULT_SENSITIVITY, "Loop", &dir.join(mode.label().replace('/', "-")), [1, 2, 3], key).unwrap();
            let names: Vec<String> = sliced.files.iter().map(|f| Path::new(f).file_stem().unwrap().to_string_lossy().into_owned()).collect();
            let starts: Vec<f64> = sliced.clip.notes.iter().map(|n| n.start).collect();
            println!("{}: {names:?}\n  {starts:?}", mode.label());
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
