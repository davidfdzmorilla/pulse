//! Slicing a loop into drum rack pads: finding where each hit starts, writing the
//! hits as files and building the MIDI clip that plays them back in order.

use crate::capture::write_wav;
use engine::model::{MidiClip, Note, RackParams, RACK_FIRST_NOTE, RACK_PADS};
use engine::Sample;
use std::path::{Path, PathBuf};

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
            Self::Transients => "Transitorios",
            Self::Grid(b) if b == 0.25 => "1/16",
            Self::Grid(b) if b == 0.5 => "1/8",
            Self::Grid(b) if b == 1.0 => "1/4",
            Self::Grid(_) => "Rejilla",
        }
    }
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
    // The very start counts as a hit when it is not silent.
    flux[0] = (0..BANDS).map(|b| db(levels[0][b]) + 60.0).sum();
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
    let flux = onset_strength(left, right);
    if flux.len() < 4 || max <= 1 {
        return vec![0];
    }
    let loudest = flux.iter().copied().fold(0.0, f32::max);
    let mean = flux.iter().sum::<f32>() / flux.len() as f32;
    let threshold = |h: usize| {
        let around = &flux[h.saturating_sub(10)..(h + 10).min(flux.len())];
        let mut sorted = around.to_vec();
        sorted.sort_by(f32::total_cmp);
        sorted[sorted.len() / 2] * 1.5 + mean * 0.5 + loudest * 0.05
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
        return Err("La región es demasiado corta para trocearla".into());
    }
    Ok((first, last))
}

/// Where `mode` cuts `seconds` of `sample` from `offset` (tempo `bpm`): points of
/// the file in seconds, and the grid actually used (0 for transients).
pub fn find_cuts(sample: &Sample, (offset, seconds): (f64, f64), bpm: f64, mode: SliceMode) -> Result<(Vec<f64>, f64), String> {
    let (first, last) = region(sample, (offset, seconds))?;
    let rate = sample.sample_rate as f64;
    let (left, right) = (&sample.left[first..last], &sample.right[first..last]);
    let (starts, used) = match mode {
        SliceMode::Transients => (transients(left, right, sample.sample_rate, RACK_PADS), 0.0),
        SliceMode::Grid(beats) => grid(left.len(), rate * 60.0 / bpm, beats, RACK_PADS),
    };
    Ok((starts.into_iter().map(|f| (first + f) as f64 / rate).collect(), used))
}

/// Slices `seconds` of `sample` from `offset`, whose tempo is `bpm`, into files in `dir`.
pub fn slice(sample: &Sample, region: (f64, f64), bpm: f64, mode: SliceMode, name: &str, dir: &Path, color: [u8; 3]) -> Result<(Sliced, f64), String> {
    let (cuts, used) = find_cuts(sample, region, bpm, mode)?;
    Ok((slice_at(sample, region, bpm, &cuts, name, dir, color)?, used))
}

/// Slices `seconds` of `sample` from `offset` at `cuts` (points of the file, in
/// seconds; at most one per pad), each slice up to the next cut.
pub fn slice_at(sample: &Sample, (offset, seconds): (f64, f64), bpm: f64, cuts: &[f64], name: &str, dir: &Path, color: [u8; 3]) -> Result<Sliced, String> {
    let (first, last) = region(sample, (offset, seconds))?;
    let rate = sample.sample_rate as f64;
    let (left, right) = (&sample.left[first..last], &sample.right[first..last]);
    let frames_per_beat = rate * 60.0 / bpm;
    let mut starts: Vec<usize> = cuts.iter().map(|&c| ((c * rate) as usize).saturating_sub(first)).filter(|&f| f < left.len()).collect();
    starts.sort_unstable();
    starts.dedup();
    starts.truncate(RACK_PADS);
    if starts.is_empty() {
        return Err("No hay ningún corte dentro de la región del clip".into());
    }

    std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
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
        // The clip's name is the project's text: no `/` or `..` out of `dir`.
        let path: PathBuf = dir.join(format!("{} {:02}.wav", crate::files::safe_name(name), pad + 1));
        write_wav(&path, &l, &r, sample.sample_rate)?;
        files[pad] = path.to_string_lossy().into_owned();
        let beat = start as f64 / frames_per_beat;
        let next = end as f64 / frames_per_beat;
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
    key: (String, u64, u64, &'static str),
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
        let key = (audio.sample.clone(), region.0.to_bits(), region.1.to_bits(), self.slice_mode.label());
        if let Some(cuts) = self.slice_cuts.as_ref().filter(|c| c.clip == clip_ref && c.key == key) {
            return Some(cuts.seconds.clone());
        }
        let sample = self.sample(&audio.sample);
        let (seconds, _) = find_cuts(&sample, region, bpm, self.slice_mode).ok()?;
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
            let sliced = slice_at(&sample, (0.0, 1.0), 120.0, &[0.0, 0.5], name, &dir, [1, 2, 3]).unwrap();
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
        let (sliced, _) = slice(&sample, (0.0, 2.0), 120.0, SliceMode::Transients, "Break", &dir, [1, 2, 3]).unwrap();
        assert_eq!(sliced.files.len(), 4);
        assert!(sliced.files[3].ends_with("Break 04.wav"));
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
        app.project.tracks[t].clips[0] = Some(Clip::Audio(AudioClip { hard_start: false,
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
        // Another mode finds its own cuts again.
        app.slice_mode = SliceMode::Grid(1.0);
        assert_eq!(app.cuts_for(clip).unwrap().len(), 4);
        std::fs::remove_dir_all(&dir).ok();
    }
}
