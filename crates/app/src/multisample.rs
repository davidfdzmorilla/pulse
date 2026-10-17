//! Multisampled sampler: several files spread over the keyboard, each a zone of
//! notes (and velocities) that plays it from the note it was recorded at.
//!
//! Loading several files maps them by the note in their names ("Piano C3.wav",
//! "Rhodes_F#2_soft.wav"; C3 is middle C, as everywhere in Pulse): each covers
//! the keys halfway to its neighbours, and files of the same note become
//! velocity layers, the quietest at the bottom. Files without a note go one per
//! key from C3 up.

use crate::app::PulseApp;
use engine::model::{InstrumentKind, SamplerParams, Zone, SAMPLER_ZONES};
use std::path::PathBuf;

/// The note named in a file's name (its last one), if any: a letter A-G on its
/// own, maybe a sharp (# or s) or flat (b), and an octave (-2 to 8).
pub fn note_in_name(stem: &str) -> Option<u8> {
    let chars: Vec<char> = stem.chars().collect();
    let mut found = None;
    for i in 0..chars.len() {
        let pitch_class = match chars[i].to_ascii_uppercase() {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => continue,
        };
        if i > 0 && chars[i - 1].is_alphanumeric() {
            continue;
        }
        let mut j = i + 1;
        let mut shift = 0i32;
        match chars.get(j) {
            Some('#') | Some('s') => {
                shift = 1;
                j += 1;
            }
            Some('b') => {
                shift = -1;
                j += 1;
            }
            _ => {}
        }
        let negative = chars.get(j) == Some(&'-');
        if negative {
            j += 1;
        }
        let Some(octave) = chars.get(j).and_then(|c| c.to_digit(10)) else { continue };
        if chars.get(j + 1).is_some_and(|c| c.is_alphanumeric()) {
            continue;
        }
        let octave = if negative { -(octave as i32) } else { octave as i32 };
        let note = (octave + 2) * 12 + pitch_class + shift;
        if (0..=127).contains(&note) {
            found = Some(note as u8);
        }
    }
    found
}

/// The zones for `files` (name, loudness), and the order the files go in (zone
/// i plays file `order[i]`). At most [`SAMPLER_ZONES`].
pub fn map(files: &[(String, f32)]) -> (Vec<usize>, Vec<Zone>) {
    let named: Vec<Option<u8>> = files.iter().map(|(name, _)| note_in_name(name)).collect();
    let mut next_free = 60u8;
    let roots: Vec<u8> = named
        .iter()
        .map(|n| {
            n.unwrap_or_else(|| {
                let key = next_free;
                next_free = next_free.saturating_add(1).min(127);
                key
            })
        })
        .collect();
    // Files by note, then quietest first.
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by(|&a, &b| roots[a].cmp(&roots[b]).then(files[a].1.total_cmp(&files[b].1)));
    order.truncate(SAMPLER_ZONES);
    let mut keys: Vec<u8> = order.iter().map(|&i| roots[i]).collect();
    keys.dedup();
    let mut zones = Vec::new();
    for (k, &root) in keys.iter().enumerate() {
        // Halfway to the neighbours; the ends reach the keyboard's.
        let lo = if k == 0 { 0 } else { (keys[k - 1] as u16 + root as u16) as u8 / 2 + 1 };
        let hi = keys.get(k + 1).map_or(127, |&next| ((root as u16 + next as u16) / 2) as u8);
        let layers = order.iter().filter(|&&i| roots[i] == root).count() as u16;
        for layer in 0..layers {
            let vel_lo = (1 + layer * 127 / layers) as u8;
            let vel_hi = ((layer + 1) * 127 / layers) as u8;
            zones.push(Zone { lo, hi, root, vel_lo, vel_hi });
        }
    }
    (order, zones)
}

impl PulseApp {
    /// A sampler track plays `paths` spread over the keyboard (see `map`).
    pub fn load_multisample(&mut self, track: usize, paths: Vec<PathBuf>) {
        let InstrumentKind::Sampler(params) = self.project.tracks[track].instrument else { return };
        let mut files = Vec::new();
        for path in paths {
            let key = path.to_string_lossy().into_owned();
            let sample = self.sample(&key);
            if sample.frames() == 0 {
                continue;
            }
            let loudness = sample.peaks.iter().map(|&(lo, hi)| hi.max(-lo)).fold(0.0, f32::max);
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            files.push((key, stem, loudness));
        }
        if files.is_empty() {
            return;
        }
        let named: Vec<(String, f32)> = files.iter().map(|(_, stem, loudness)| (stem.clone(), *loudness)).collect();
        let (order, zones) = map(&named);
        let mut params = SamplerParams { zone_count: zones.len() as u8, ..params };
        params.zones = Default::default();
        params.zones[..zones.len()].copy_from_slice(&zones);
        let unnamed = named.iter().filter(|(name, _)| note_in_name(name).is_none()).count();
        self.project.tracks[track].samples = order.iter().map(|&i| files[i].0.clone()).collect();
        self.set_instrument(track, InstrumentKind::Sampler(params));
        self.sync_samples(track);
        let skipped = files.len().saturating_sub(SAMPLER_ZONES);
        let mut status = format!("{} muestras repartidas por el teclado", zones.len());
        if unnamed > 0 {
            status.push_str(&format!(" ({unnamed} sin nota en el nombre: desde C3, una por tecla)"));
        }
        if skipped > 0 {
            status.push_str(&format!(" · {skipped} no caben (máximo {SAMPLER_ZONES})"));
        }
        self.status = Some(status);
    }

    /// A sampler with no samples (the multisample's zones go too).
    pub fn clear_sampler(&mut self, track: usize) {
        if let InstrumentKind::Sampler(params) = self.project.tracks[track].instrument {
            self.set_instrument(track, InstrumentKind::Sampler(SamplerParams { zone_count: 0, zones: Default::default(), ..params }));
        }
        self.project.tracks[track].samples.clear();
        self.sync_samples(track);
    }

    /// Moves a multisample's zones `semitones` up or down the keyboard (files
    /// named an octave off, as some libraries do).
    pub fn shift_zones(&mut self, track: usize, semitones: i32) {
        let InstrumentKind::Sampler(mut params) = self.project.tracks[track].instrument else { return };
        let count = params.zone_count as usize;
        let fits = params.zones[..count].iter().all(|z| (0..=127).contains(&(z.root as i32 + semitones)));
        if !fits {
            self.status = Some("No cabe más allá: el mapa llegaría fuera del teclado".into());
            return;
        }
        let at = |n: u8| (n as i32 + semitones).clamp(0, 127) as u8;
        for zone in &mut params.zones[..count] {
            // The keyboard's ends stay reached.
            zone.lo = if zone.lo == 0 { 0 } else { at(zone.lo) };
            zone.hi = if zone.hi == 127 { 127 } else { at(zone.hi) };
            zone.root = at(zone.root);
        }
        self.set_instrument(track, InstrumentKind::Sampler(params));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_read_from_file_names() {
        assert_eq!(note_in_name("Piano C3"), Some(60));
        assert_eq!(note_in_name("Rhodes_F#2_soft"), Some(54));
        assert_eq!(note_in_name("Strings-Db4"), Some(73));
        assert_eq!(note_in_name("Bass A-1"), Some(21));
        assert_eq!(note_in_name("cello_as1"), Some(46));
        // Words with those letters aren't notes; the last note wins.
        assert_eq!(note_in_name("Bright Cello"), None);
        assert_eq!(note_in_name("Grand E2 take 3"), Some(52));
        assert_eq!(note_in_name("kick 01"), None);
    }

    #[test]
    fn named_files_split_the_keyboard_and_same_notes_layer_by_loudness() {
        let files = vec![
            ("Piano C4 hard".to_string(), 0.9),
            ("Piano C3".to_string(), 0.5),
            ("Piano C4 soft".to_string(), 0.2),
            ("Piano G3".to_string(), 0.5),
        ];
        let (order, zones) = map(&files);
        assert_eq!(order, vec![1, 3, 2, 0], "by note, quietest first");
        let ranges: Vec<(u8, u8, u8, u8, u8)> = zones.iter().map(|z| (z.lo, z.hi, z.root, z.vel_lo, z.vel_hi)).collect();
        assert_eq!(ranges, vec![(0, 63, 60, 1, 127), (64, 69, 67, 1, 127), (70, 127, 72, 1, 63), (70, 127, 72, 64, 127)]);
        // Every note and velocity plays something.
        for pitch in 0..=127u8 {
            for velocity in [0.01, 0.5, 1.0] {
                assert!(zones.iter().any(|z| z.plays(pitch, velocity)), "{pitch} {velocity}");
            }
        }
    }

    #[test]
    fn files_without_notes_go_one_per_key_from_middle_c() {
        let files = vec![("hit one".to_string(), 0.5), ("hit two".to_string(), 0.5)];
        let (_, zones) = map(&files);
        assert_eq!(zones.iter().map(|z| z.root).collect::<Vec<_>>(), vec![60, 61]);
    }

    /// Several files loaded on a sampler track: spread by their names, kept in
    /// the project; one file again: a single sample over the keyboard.
    #[test]
    fn loading_several_files_makes_a_multisample() {
        let dir = std::env::temp_dir().join(format!("pulse-multisample-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, level: f32| {
            let path = dir.join(name);
            let wave: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * level).collect();
            crate::capture::write_wav(&path, &wave, &wave, 48_000.0).unwrap();
            path
        };
        let paths = vec![write("Keys G3.wav", 0.5), write("Keys C3.wav", 0.5), write("Keys C4 loud.wav", 0.9), write("Keys C4 soft.wav", 0.2)];
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let t = 0;
        app.set_instrument(t, InstrumentKind::Sampler(SamplerParams::default()));
        app.set_instrument_samples(t, 0, paths.clone());
        let InstrumentKind::Sampler(params) = app.project.tracks[t].instrument else { panic!() };
        assert_eq!(params.zone_count, 4, "{:?}", app.status);
        let names: Vec<String> = (0..4).map(|i| app.project.tracks[t].sample_name(i).unwrap_or_default()).collect();
        assert_eq!(names, ["Keys C3", "Keys G3", "Keys C4 soft", "Keys C4 loud"]);
        // Saved and read back, the map is the same.
        let json = serde_json::to_string(&app.project).unwrap();
        let back: crate::project::Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tracks[t].instrument, app.project.tracks[t].instrument);
        // An octave up, then a single file again.
        app.shift_zones(t, 12);
        let InstrumentKind::Sampler(shifted) = app.project.tracks[t].instrument else { panic!() };
        assert_eq!(shifted.zones[0].root, 72);
        app.set_instrument_samples(t, 0, vec![paths[0].clone()]);
        let InstrumentKind::Sampler(single) = app.project.tracks[t].instrument else { panic!() };
        assert_eq!((single.zone_count, app.project.tracks[t].samples.len()), (0, 1));
        std::fs::remove_dir_all(&dir).ok();
    }
}
