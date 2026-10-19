//! A starter kit for the drum rack, rendered from the drum machine's own sounds,
//! so the rack is playable before the user has any samples of their own.

use crate::capture::write_wav;
use engine::model::{drum_note::*, DrumParams, RackParams, RACK_FIRST_NOTE, RACK_PADS};
use engine::render_drum_hit;
use std::path::{Path, PathBuf};

/// (pad note, file name, drum machine sound, tuning in semitones).
const BASIC_KIT: [(u8, &str, u8, f32); 13] = [
    (36, "Kick", KICK, 0.0),
    (37, "Rim", RIM, 0.0),
    (38, "Snare", SNARE, 0.0),
    (39, "Clap", CLAP, 0.0),
    (41, "Tom", LOW_TOM, -7.0),
    (42, "Closed Hat", CLOSED_HAT, 0.0),
    (43, "Tom", LOW_TOM, -4.0),
    (44, "Cowbell", COWBELL, 0.0),
    (45, "Tom", LOW_TOM, 0.0),
    (46, "Open Hat", OPEN_HAT, 0.0),
    (47, "Tom", LOW_TOM, 3.0),
    (48, "Tom", LOW_TOM, 5.0),
    (50, "Tom", LOW_TOM, 8.0),
];

/// Where generated kits are kept.
pub fn kits_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => Path::new(&home).join("Library/Application Support/Pulse/Kits"),
        None => std::env::temp_dir().join("Pulse Kits"),
    }
}

/// Writes the basic kit's sounds to `dir` (once) and returns the rack settings and
/// the file for each pad.
pub fn basic_kit(dir: &Path) -> Result<(RackParams, Vec<String>), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    const SAMPLE_RATE: f32 = 48_000.0;
    let mut params = RackParams::default();
    let mut files = vec![String::new(); RACK_PADS];
    let full_level = DrumParams { level: [1.0; engine::model::DRUM_SOUNDS], ..DrumParams::default() };
    for (note, name, sound, tune) in BASIC_KIT {
        let pad = (note - RACK_FIRST_NOTE) as usize;
        let path = dir.join(format!("{name}.wav"));
        if !path.exists() {
            let (left, right) = render_drum_hit(full_level, sound, SAMPLE_RATE);
            write_wav(&path, &left, &right, SAMPLE_RATE)?;
        }
        files[pad] = path.to_string_lossy().into_owned();
        params.tune[pad] = tune;
    }
    // The closed hat cuts the open one, as on drum machines.
    for note in [CLOSED_HAT, OPEN_HAT] {
        params.choke[(note - RACK_FIRST_NOTE) as usize] = 1;
    }
    Ok((params, files))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_basic_kit_fills_its_pads_with_sounding_files() {
        let dir = std::env::temp_dir().join(format!("pulse-kit-{}", std::process::id()));
        let (params, files) = basic_kit(&dir).unwrap();
        let used: Vec<_> = files.iter().filter(|f| !f.is_empty()).collect();
        assert_eq!(used.len(), BASIC_KIT.len());
        for file in used {
            let sample = engine::Sample::load(Path::new(file)).unwrap();
            let peak = sample.left.iter().fold(0f32, |m, s| m.max(s.abs()));
            assert!(peak > 0.05 && peak <= 1.0, "{file}: peak {peak}");
            assert!(sample.duration() < 4.0, "{file} is trimmed");
        }
        assert_eq!(params.choke[6], params.choke[10]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
