//! The Sampler's factory presets: sounds of «Sonidos de Pulse» (see
//! `factory_sounds`) set up to play, the 808s for trap first. A preset names
//! its sound by folder and name; the file is made the first time a preset
//! needs it (see [`make_missing`]), as the kits' are.

use crate::factory_recipes::{recipes, C1, SUBS};
use crate::presets::Preset;
use engine::model::{InstrumentKind, SamplerParams};
use std::path::Path;

/// A preset: its name, the sound it plays (folder and name in the library)
/// and the Sampler's settings.
pub struct SamplerPreset {
    pub name: &'static str,
    pub folder: &'static str,
    pub sound: &'static str,
    pub params: SamplerParams,
}

/// Every preset, in the menu's order.
pub fn all() -> Vec<SamplerPreset> {
    // The library's 808s are in C at C1: that note plays them at their pitch.
    let base = SamplerParams { root: C1 as f32, attack: 0.001, decay: 1.0, sustain: 1.0, release: 0.12, ..SamplerParams::default() };
    vec![
        SamplerPreset {
            // Long and punchy, sliding into the notes written over the last
            // one (FL's slide notes): the trap 808. After a rest a note
            // starts on its own pitch, with its punch.
            name: "808 · Largo con glide",
            folder: SUBS,
            sound: "808 largo con pegada (Do)",
            params: SamplerParams { mono: true, legato: true, glide: 90.0, gain: 0.7, ..base },
        },
        SamplerPreset {
            // Saturated and short: it dies away under a held note, slides
            // quickly when tied, and the tails never pile up (drill, rage).
            name: "808 · Corto y sucio",
            folder: SUBS,
            sound: "808 saturado (Do)",
            params: SamplerParams { mono: true, legato: true, glide: 45.0, decay: 0.3, sustain: 0.45, release: 0.06, cutoff: 9000.0, gain: 1.0, ..base },
        },
        SamplerPreset {
            // Each note a new hit with its click, cutting the last: a bouncing
            // 808 line without slides (boom bap, old trap).
            name: "808 · Con golpe",
            folder: SUBS,
            sound: "808 con golpe (Do)",
            params: SamplerParams { mono: true, release: 0.08, gain: 0.7, ..base },
        },
    ]
}

/// The presets as the menus and the search list them, their files under `root`.
pub fn presets(root: &Path) -> Vec<Preset> {
    all()
        .into_iter()
        .map(|p| Preset { name: p.name.to_string(), instrument: InstrumentKind::Sampler(p.params), samples: vec![root.join(p.folder).join(format!("{}.wav", p.sound)).to_string_lossy().into_owned()] })
        .collect()
}

/// Makes the sounds of the library among `samples` (files under `root`)
/// that aren't made yet; other files are left as they are.
pub fn make_missing(root: &Path, samples: &[String]) -> Result<(), String> {
    let missing: Vec<&String> = samples.iter().filter(|f| Path::new(f.as_str()).starts_with(root) && !Path::new(f.as_str()).exists()).collect();
    if missing.is_empty() {
        return Ok(());
    }
    for recipe in recipes() {
        if missing.iter().any(|f| Path::new(f.as_str()) == crate::factory_sounds::file(root, &recipe)) {
            crate::factory_sounds::make(root, &recipe)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factory_recipes::{render_project, RATE};
    use crate::project::{Project, Track};
    use engine::model::{ArrangementClip, Clip, MidiClip, Note};

    /// A trap 808 line on the preset at 140 BPM: (beat, length, pitch). Two
    /// tied notes (the second slides an octave up), notes after rests.
    const LINE: [(f64, f64, u8); 5] = [(0.0, 1.75, 36), (1.5, 1.0, 48), (3.0, 0.75, 39), (4.0, 2.0, 34), (6.5, 1.0, 36)];

    fn render(preset: &Preset, notes: &[(f64, f64, u8)], beats: f64) -> (Vec<f32>, Vec<f32>) {
        let mut project = Project::empty();
        project.tempo = 140.0;
        let mut track = Track::new("808", [0, 0, 0], preset.instrument);
        track.samples = preset.samples.clone();
        let mut clip = MidiClip::new("808", beats, [0, 0, 0]);
        for &(start, length, pitch) in notes {
            clip.notes.push(Note::new(start, length, pitch, 0.9));
        }
        track.arrangement.push(ArrangementClip { start: 0.0, length: beats, clip: Clip::Midi(clip) });
        project.tracks = vec![track];
        render_project(&project, beats)
    }

    /// Where the energy of the fundamental is, in MIDI notes (a DFT around
    /// `near`, ±3 semitones).
    fn note_of(x: &[f32], near: f32) -> f32 {
        let n = x.len();
        let window = |i: usize| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
        let power = |note: f32| {
            let hz = 440.0 * 2f32.powf((note - 69.0) / 12.0);
            let w = std::f32::consts::TAU * hz / RATE;
            let (re, im) = x.iter().enumerate().fold((0f32, 0f32), |(re, im), (i, s)| (re + s * window(i) * (w * i as f32).cos(), im + s * window(i) * (w * i as f32).sin()));
            re * re + im * im
        };
        let steps: Vec<f32> = (0..=120).map(|k| near - 3.0 + k as f32 * 0.05).collect();
        steps.into_iter().max_by(|a, b| power(*a).total_cmp(&power(*b))).unwrap()
    }

    fn at(beat: f64, ms: f64) -> usize {
        ((beat * 60.0 / 140.0 + ms * 0.001) * RATE as f64) as usize
    }

    /// Every preset is found, its sound made, it plays the line in tune and
    /// leaves headroom, at a bass's level: and «808» finds them all.
    #[test]
    fn every_sampler_preset_sounds_in_tune_and_leaves_headroom() {
        let root = crate::factory_sounds::root();
        let presets = presets(&root);
        assert_eq!(presets.len(), all().len());
        for preset in &presets {
            assert!(preset.name.starts_with("808 · "), "{}", preset.name);
            make_missing(&root, &preset.samples).unwrap();
            assert!(Path::new(&preset.samples[0]).exists(), "{}: its sound is made", preset.name);
            let (l, r) = render(preset, &LINE, 8.0);
            assert!(l.iter().chain(&r).all(|s| s.is_finite()), "{}", preset.name);
            let peak = l.iter().chain(&r).fold(0f32, |m, s| m.max(s.abs()));
            let rms = (l.iter().chain(&r).map(|s| s * s).sum::<f32>() / (2 * l.len()) as f32).sqrt();
            let (peak_db, rms_db) = (20.0 * peak.log10(), 20.0 * rms.log10());
            // A loose note's pitch, past its punch: what it is written as.
            let note = note_of(&l[at(4.0, 120.0)..at(4.0, 420.0)], 34.0);
            println!("{}: peak {peak_db:.1} dBFS, {rms_db:.1} dB RMS; 34 plays as {note:.2}", preset.name);
            assert!(peak_db <= -0.5, "{}: peak {peak_db}", preset.name);
            assert!((-24.0..=-9.0).contains(&rms_db), "{}: {rms_db} dB RMS", preset.name);
            assert!((note - 34.0).abs() < 0.15, "{}: 34 plays as {note}", preset.name);
        }
        // The search finds them by «808».
        let items = crate::search::catalog(&crate::search::Sources { presets: Path::new("/nonexistent"), chains: Vec::new(), plugins: &[], factory: &root });
        let found: Vec<String> = crate::search::find(&[&items], "808", 200).into_iter().flat_map(|(_, g)| g.into_iter().map(|i| i.name.clone())).collect();
        for preset in &presets {
            assert!(found.contains(&preset.name), "«808» finds {}: {found:?}", preset.name);
        }
    }

    /// «808 · Largo con glide» slides into the tied note, without a new
    /// punch, and starts each note after a rest on its pitch.
    #[test]
    fn the_long_808_slides_only_into_tied_notes() {
        let root = crate::factory_sounds::root();
        let preset = presets(&root).into_iter().find(|p| p.name == "808 · Largo con glide").unwrap();
        make_missing(&root, &preset.samples).unwrap();
        let (l, _) = render(&preset, &LINE, 8.0);
        let start = note_of(&l[at(0.0, 150.0)..at(0.0, 450.0)], 36.0);
        // 48 tied over 36 at beat 1.5: on its way up 20 ms in, there by 250 ms.
        let sliding = note_of(&l[at(1.5, 5.0)..at(1.5, 45.0)], 42.0);
        let tied = note_of(&l[at(1.5, 250.0)..at(1.5, 400.0)], 48.0);
        let after_rest = note_of(&l[at(3.0, 120.0)..at(3.0, 300.0)], 39.0);
        println!("808 · Largo con glide: 36 → {start:.2}; tied 48: sliding {sliding:.2}, then {tied:.2}; 39 after a rest → {after_rest:.2}");
        assert!((start - 36.0).abs() < 0.15, "{start}");
        assert!(sliding > 37.0 && sliding < 46.0, "{sliding}");
        assert!((tied - 48.0).abs() < 0.15, "{tied}");
        assert!((after_rest - 39.0).abs() < 0.15, "{after_rest}");
        // No new punch on the tie: the level round it doesn't jump.
        let level = |from: usize, to: usize| (l[from..to].iter().map(|s| s * s).sum::<f32>() / (to - from) as f32).sqrt();
        let (before, after) = (level(at(1.5, -40.0), at(1.5, 0.0)), level(at(1.5, 0.0), at(1.5, 40.0)));
        assert!(after < before * 1.5, "tied: {before} → {after}");
    }

    /// WAVs of each preset playing the line:
    /// `PULSE_WAV=dir cargo test --release -p app sampler_preset_wavs -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn sampler_preset_wavs() {
        let Some(dir) = std::env::var("PULSE_WAV").ok() else { return };
        let root = crate::factory_sounds::root();
        for preset in presets(&root) {
            make_missing(&root, &preset.samples).unwrap();
            let (l, r) = render(&preset, &LINE, 8.0);
            let name = preset.name.replace(" · ", " - ");
            crate::factory_sounds::write_wav(&Path::new(&dir).join(format!("{name}.wav")), &l, &r).unwrap();
        }
        // The sound the long one plays, as it is.
        let sound = Path::new(&presets(&root)[0].samples[0]).to_path_buf();
        std::fs::copy(&sound, Path::new(&dir).join(sound.file_name().unwrap())).unwrap();
    }
}
