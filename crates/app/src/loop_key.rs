//! Loops in the song's key: each audio file's key is found when it comes in
//! (from its name, as loop libraries write it, «Pad_120_Am», or heard, when it
//! is clear), and its clips are transposed to the project's scale, by the
//! shortest way, to the relative key if their modes differ. When the project's
//! key changes they follow. The first loop with a key gives the project its key
//! if it had none. Drums are left as they are.

use crate::app::{ClipRef, PulseApp};
use engine::model::{AudioClip, Clip, ClipKey, WarpMode};
use stems::key::Key;

/// How clearly a file must sound in a key for it to count, without one in its name.
const CLEAR: f64 = 0.6;

/// How the words in a file's name that say it is drums begin (drums have no key
/// to follow).
const DRUM_WORDS: [&str; 15] =
    ["kick", "snare", "hat", "hihat", "clap", "perc", "drum", "tom", "cymbal", "ride", "crash", "break", "shaker", "rimshot", "bombo"];

/// The key written in a file's name: a word like «Am», «F#m», «Ebmin», «Cmaj»,
/// «E», or one after its tempo («120Am»). The mode, if the name says it.
pub fn from_name(name: &str) -> Option<(u8, Option<bool>)> {
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric() && c != '#').filter(|w| !w.is_empty()).collect();
    // The name's first word is its pack or instrument, never its key.
    words.iter().skip(1).rev().find_map(|word| {
        // A tempo before it: «120Am».
        let digits = word.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && !(2..=3).contains(&digits) {
            return None;
        }
        if digits > 0 && !(60..=200).contains(&word[..digits].parse::<u32>().unwrap_or(0)) {
            return None;
        }
        parse_key(&word[digits..])
    })
}

fn parse_key(word: &str) -> Option<(u8, Option<bool>)> {
    let mut chars = word.chars();
    let letter = chars.next()?;
    let base: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (shift, rest) = match rest.chars().next() {
        Some('#') => (1, &rest[1..]),
        Some('b') => (-1, &rest[1..]),
        _ => (0, rest),
    };
    let minor = match rest {
        "" => None,
        "m" | "min" | "Min" | "minor" | "Minor" => Some(true),
        "maj" | "Maj" | "major" | "Major" | "M" => Some(false),
        _ => return None,
    };
    Some(((base + shift).rem_euclid(12) as u8, minor))
}

/// Whether a file's name says it is drums: one of its words (split at
/// capitals too: «KickLoopB») is one of [`DRUM_WORDS`] or begins like it.
fn is_drums(name: &str) -> bool {
    let mut words = Vec::new();
    let mut word = String::new();
    for c in name.chars() {
        if !c.is_alphabetic() || (c.is_uppercase() && word.chars().last().is_some_and(|l| l.is_lowercase())) {
            words.push(std::mem::take(&mut word));
        }
        if c.is_alphabetic() {
            word.push(c);
        }
    }
    words.push(word);
    words.iter().map(|w| w.to_lowercase()).any(|w| w == "beat" || DRUM_WORDS.iter().any(|d| w.starts_with(d)))
}

/// The semitones (-6..=5) that move a loop in `from` into `to`: to its tonic,
/// or to its relative's if one is minor and the other major.
pub fn shift(from: Key, to: Key) -> i32 {
    let target = match (from.minor, to.minor) {
        (true, false) => to.tonic + 9,
        (false, true) => to.tonic + 3,
        _ => to.tonic,
    } as i32;
    let up = (target - from.tonic as i32).rem_euclid(12);
    if up > 5 { up - 12 } else { up }
}

/// The key of a file called `name`, from its name or heard in `mono`; None for
/// drums or when it is not clear. With a name that does not say the mode, the
/// one that fits the project's `key` best (so a loop «in E» in G major is E
/// minor), else the one that sounds.
pub fn file_key(name: &str, mono: &[f32], rate: f32, project: Option<Key>) -> Option<Key> {
    if is_drums(name) {
        return None;
    }
    let chroma = stems::key::chroma(mono, rate);
    let heard = stems::key::Key::detect_clearly(&chroma).filter(|(_, clarity)| *clarity >= CLEAR);
    // A name's key, unless the sound clearly says another and hardly fits it
    // (names have numbers and letters that are not keys: «150A»).
    let named = from_name(name).filter(|&(tonic, minor)| {
        let fits = [true, false].iter().filter(|&&m| minor.is_none_or(|n| n == m)).map(|&m| Key { tonic, minor: m }.fit(&chroma)).fold(f64::MIN, f64::max);
        heard.is_none() || fits >= 0.3
    });
    match named {
        Some((tonic, Some(minor))) => Some(Key { tonic, minor }),
        Some((tonic, None)) => {
            let options = [Key { tonic, minor: true }, Key { tonic, minor: false }];
            Some(match project {
                Some(p) => *options.iter().min_by_key(|k| shift(**k, p).abs()).expect("two"),
                // The mode whose third sounds more.
                None => {
                    if chroma[(tonic as usize + 3) % 12] >= chroma[(tonic as usize + 4) % 12] { options[0] } else { options[1] }
                }
            })
        }
        None => heard.map(|(k, _)| k),
    }
}

/// The transposition a following clip gets in a project in `key`.
pub fn transpose_for(clip: &AudioClip, key: Option<Key>) -> f32 {
    match (clip.key, key) {
        (Some(own), Some(project)) => shift(Key { tonic: own.tonic, minor: own.minor }, project) as f32,
        _ => 0.0,
    }
}

impl PulseApp {
    /// Finds the key of new audio clips and, if they follow the project's,
    /// transposes them; the first with a key gives it to a project without one.
    pub fn find_clip_keys(&mut self, clips: &[ClipRef]) {
        let mut found = Vec::new();
        for &clip in clips {
            let Some(Clip::Audio(audio)) = self.clip(clip).cloned() else { continue };
            let sample = self.sample(&audio.sample);
            if sample.frames() == 0 {
                continue;
            }
            let mono: Vec<f32> = sample.left.iter().zip(&sample.right).map(|(l, r)| (l + r) * 0.5).collect();
            let name = std::path::Path::new(&audio.sample).file_stem().map_or(audio.name.clone(), |s| s.to_string_lossy().into_owned());
            let Some(key) = file_key(&name, &mono, sample.sample_rate, self.project.key) else { continue };
            let follow = self.settings.loops_follow_key;
            self.edit_audio_clip(clip, |a| a.key = Some(ClipKey { tonic: key.tonic, minor: key.minor, follow }));
            found.push((clip, key, audio.name));
        }
        let Some((_, first, name)) = found.first().cloned() else { return };
        if self.project.key.is_none() && self.settings.loops_follow_key {
            self.project.key = Some(first);
            self.status = Some(format!("«{name}» está en {}: es ahora la escala del proyecto, y los loops que añadas se ajustan a ella", first.name()));
        } else if let (Some(project), true) = (self.project.key, self.settings.loops_follow_key) {
            let moved: Vec<String> = found
                .iter()
                .filter(|(_, k, _)| shift(*k, project) != 0)
                .map(|(_, k, n)| format!("«{n}» ({} → {:+} st)", k.name(), shift(*k, project)))
                .collect();
            if !moved.is_empty() {
                self.status = Some(format!("Ajustado a {}: {}", project.name(), moved.join(", ")));
            }
        }
        self.follow_key();
    }

    /// Sets the project's key; the loops that follow it move with it.
    pub fn set_key(&mut self, key: Option<Key>) {
        self.project.key = key;
        self.follow_key();
        self.sync_all_midi_effects();
    }

    /// Transposes every clip that follows the project's key to it.
    pub fn follow_key(&mut self) {
        let key = self.project.key;
        let mut changed = Vec::new();
        for (t, track) in self.project.tracks.iter_mut().enumerate() {
            let clips = track.clips.iter_mut().flatten().chain(track.arrangement.iter_mut().map(|a| &mut a.clip));
            for clip in clips {
                let Clip::Audio(audio) = clip else { continue };
                if !audio.key.is_some_and(|k| k.follow) {
                    continue;
                }
                let semitones = transpose_for(audio, key);
                if audio.transpose != semitones {
                    audio.transpose = semitones;
                    // Transposing needs Stretch (Off plays the file as it is).
                    if audio.warp == WarpMode::Off && semitones != 0.0 {
                        audio.warp = WarpMode::Stretch;
                    }
                    changed.push(t);
                }
            }
        }
        changed.dedup();
        for t in changed {
            self.sync_track_clips(t);
        }
    }

    /// Follows the project's key or not (its transposition goes back to none).
    pub fn set_follow_key(&mut self, clip: ClipRef, follow: bool) {
        self.edit_audio_clip(clip, |a| {
            if let Some(k) = a.key.as_mut() {
                k.follow = follow;
            }
            if !follow {
                a.transpose = 0.0;
            }
        });
        self.follow_key();
    }

    /// Finds the clip's key now (asked for from its editor), and follows the
    /// project's with it.
    pub fn detect_clip_key(&mut self, clip: ClipRef) {
        let Some(Clip::Audio(audio)) = self.clip(clip).cloned() else { return };
        let sample = self.sample(&audio.sample);
        let mono: Vec<f32> = sample.left.iter().zip(&sample.right).map(|(l, r)| (l + r) * 0.5).collect();
        let name = std::path::Path::new(&audio.sample).file_stem().map_or(audio.name.clone(), |s| s.to_string_lossy().into_owned());
        // Asked for, even drums' names are no reason not to listen.
        let key = file_key(&name, &mono, sample.sample_rate, self.project.key)
            .or_else(|| stems::key::Key::detect_clearly(&stems::key::chroma(&mono, sample.sample_rate)).filter(|(_, c)| *c >= CLEAR).map(|(k, _)| k));
        match key {
            Some(key) => {
                self.edit_audio_clip(clip, |a| a.key = Some(ClipKey { tonic: key.tonic, minor: key.minor, follow: true }));
                self.status = Some(format!("«{}» está en {}", audio.name, key.name()));
                self.follow_key();
            }
            None => self.status = Some(format!("«{}» no tiene una tonalidad clara", audio.name)),
        }
    }

    /// Changes an audio clip in place and sends it to the engine.
    fn edit_audio_clip(&mut self, clip: ClipRef, edit: impl FnOnce(&mut AudioClip)) {
        let target = match clip {
            ClipRef::Session { track, slot } => self.project.tracks.get_mut(track).and_then(|t| t.clips[slot].as_mut()),
            ClipRef::Arrangement { track, index } => self.project.tracks.get_mut(track).and_then(|t| t.arrangement.get_mut(index)).map(|a| &mut a.clip),
        };
        if let Some(Clip::Audio(audio)) = target {
            edit(audio);
            self.sync_clip_ref(clip);
        }
    }

    /// Sends all of a track's clips to the engine.
    fn sync_track_clips(&mut self, track: usize) {
        for slot in 0..engine::NUM_SCENES {
            if self.project.tracks[track].clips[slot].is_some() {
                self.sync_clip(track, slot);
            }
        }
        self.sync_arrangement(track);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prints the key each file in `PULSE_LOOPS` (a folder) is taken to be in,
    /// and its move into G major.
    #[test]
    #[ignore]
    fn dump_loop_keys() {
        let Ok(dir) = std::env::var("PULSE_LOOPS") else { return };
        let g = Key { tonic: 7, minor: false };
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let Ok(sample) = engine::Sample::load(&path) else { continue };
            let mono: Vec<f32> = sample.left.iter().zip(&sample.right).map(|(l, r)| (l + r) * 0.5).collect();
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let alone = file_key(&name, &mono, sample.sample_rate, None);
            let in_g = file_key(&name, &mono, sample.sample_rate, Some(g));
            println!("{name:30} {:12} en Sol mayor: {:?}", alone.map_or("—".into(), |k| k.name()), in_g.map(|k| (k.name(), shift(k, g))));
        }
    }

    #[test]
    fn keys_are_read_from_names() {
        assert_eq!(from_name("TB_BassArp134_E"), Some((4, None)));
        assert_eq!(from_name("Pad_120_Am"), Some((9, Some(true))));
        assert_eq!(from_name("Keys 90 F#m"), Some((6, Some(true))));
        assert_eq!(from_name("Strings_Ebmin_01"), Some((3, Some(true))));
        assert_eq!(from_name("Lead_Bb_128"), Some((10, None)));
        assert_eq!(from_name("Guitar_Cmaj"), Some((0, Some(false))));
        assert_eq!(from_name("Pluck-124Gm"), Some((7, Some(true))));
        assert_eq!(from_name("PadK03-150A-01"), Some((9, None)), "a key after the tempo");
        // No key: the first word is the instrument, and words are not keys.
        assert_eq!(from_name("Bass_Deep_01"), None);
        assert_eq!(from_name("E_Piano_Loop"), None);
        assert_eq!(from_name("Vox_Ahh_Dry"), None);
        assert_eq!(from_name("Synth_Big"), None);
    }

    #[test]
    fn drum_files_are_told_by_their_names() {
        for drums in ["WT_KickLoopB_125-01", "Jomox_Hat07", "Snares_Pack", "Breakbeat 170", "Top_Perc_Loop", "Beat_01"] {
            assert!(is_drums(drums), "{drums}");
        }
        for tonal in ["Bottom_Bass_Em", "What_A_Pad", "Atmosphere_Cm", "Primal_Lead", "IND_PadK03-150A-01", "TB_BassArp134_E"] {
            assert!(!is_drums(tonal), "{tonal}");
        }
    }

    #[test]
    fn loops_move_the_shortest_way_to_the_key() {
        let k = |tonic, minor| Key { tonic, minor };
        assert_eq!(shift(k(9, true), k(4, true)), -5, "A minor to E minor: down a fourth");
        assert_eq!(shift(k(0, false), k(2, false)), 2);
        assert_eq!(shift(k(0, false), k(10, false)), -2);
        // Relatives: E minor in G major stays; C major in A minor stays.
        assert_eq!(shift(k(4, true), k(7, false)), 0);
        assert_eq!(shift(k(0, false), k(9, true)), 0);
        // F# minor into G major: to E minor, down two.
        assert_eq!(shift(k(6, true), k(7, false)), -2);
    }

    #[test]
    fn drums_have_no_key_and_names_without_a_mode_take_the_projects() {
        // A name's key that the sound clearly contradicts loses to the sound.
        let e = chord(44_100.0, &[40, 52, 55, 59, 64]);
        assert_eq!(file_key("Pad-150A-01", &e, 44_100.0, None).map(|k| k.name()).as_deref(), Some("Mi menor"));
        let silence = vec![0f32; 1000];
        assert_eq!(file_key("Kick_Loop_Am", &silence, 44_100.0, None), None);
        assert_eq!(file_key("Jomox_Hat07", &silence, 44_100.0, None), None);
        // «E» in G major is E minor (its relative: no move).
        let g = Key { tonic: 7, minor: false };
        assert_eq!(file_key("TB_BassArp134_E", &silence, 44_100.0, Some(g)), Some(Key { tonic: 4, minor: true }));
    }

    /// A chord three seconds long.
    fn chord(rate: f32, pitches: &[u8]) -> Vec<f32> {
        (0..(rate * 3.0) as usize)
            .map(|i| {
                let t = i as f32 / rate;
                pitches.iter().map(|&p| {
                    let f = 440.0 * 2f32.powf((p as f32 - 69.0) / 12.0);
                    (1..4).map(|h| (2.0 * std::f32::consts::PI * f * h as f32 * t).sin() / h as f32).sum::<f32>()
                }).sum::<f32>() * 0.05
            })
            .collect()
    }

    /// A new loop gives the project its key; another one follows it, and both
    /// move when the key changes.
    #[test]
    fn loops_follow_the_projects_key() {
        let dir = std::env::temp_dir().join(format!("pulse-loop-key-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let rate = 44_100.0;
        let write = |name: &str, pitches: &[u8]| {
            let path = dir.join(name);
            let spec = hound::WavSpec { channels: 1, sample_rate: rate as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
            let mut w = hound::WavWriter::create(&path, spec).unwrap();
            for s in chord(rate, pitches) {
                w.write_sample((s * 20_000.0) as i16).unwrap();
            }
            w.finalize().unwrap();
            path
        };
        // An E minor chord without a key in its name, and an A minor one with.
        let (pad, keys) = (write("Pad.wav", &[40, 52, 55, 59, 64]), write("Keys_Am.wav", &[45, 57, 60, 64, 69]));
        let mut app = PulseApp::headless(crate::project::Project::empty());
        app.settings.loops_follow_key = true;
        let t = app.add_audio_track().unwrap();
        app.add_samples(vec![pad], crate::app::Placement::Session { track: t, slot: 0 });
        assert_eq!(app.project.key.map(|k| k.name()).as_deref(), Some("Mi menor"), "{:?}", app.status);
        // «Am» by its name: down a fourth into E minor, in Stretch.
        app.add_samples(vec![keys], crate::app::Placement::Session { track: t, slot: 1 });
        let audio = |app: &PulseApp, slot: usize| match &app.project.tracks[t].clips[slot] {
            Some(Clip::Audio(a)) => a.clone(),
            _ => panic!(),
        };
        assert_eq!(audio(&app, 0).transpose, 0.0);
        assert_eq!(audio(&app, 1).transpose, -5.0);
        assert_eq!(audio(&app, 1).warp, WarpMode::Stretch);
        // To G minor: E minor up three, A minor down two.
        app.set_key(Some(Key { tonic: 7, minor: true }));
        assert_eq!((audio(&app, 0).transpose, audio(&app, 1).transpose), (3.0, -2.0));
        // One that stops following goes back to its own key.
        app.set_follow_key(ClipRef::Session { track: t, slot: 1 }, false);
        assert_eq!(audio(&app, 1).transpose, 0.0);
        app.set_key(None);
        assert_eq!(audio(&app, 0).transpose, 0.0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
