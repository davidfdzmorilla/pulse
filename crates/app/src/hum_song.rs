//! From a hummed tune to a song: the melody heard in an audio clip (or the
//! notes of a MIDI one), set on the grid, harmonized with the chords of its
//! key that fit it, and played on new tracks under it: the melody on a lead,
//! the chords, a bass and a beat in the style chosen.
//!
//! Harmonizing picks a chord of the scale per bar (or half bar): the one whose
//! notes are those the melody sings there, the strong beats counting more,
//! and among those that fit alike the ones that usually follow each other
//! (`chord::next_degrees`), starting and ending at home.

use crate::accompany::Part;
use crate::app::{ClipRef, PulseApp};
use crate::drum_gen::{self, Level, Style};
use engine::model::{AudioClip, Clip, InstrumentKind, MidiClip, Note, SynthParams};
use engine::{Target, MAX_TRACKS};
use stems::chord::{self, Chord};
use stems::key::Key;
use std::sync::{Arc, Mutex};

/// The grid the melody's notes go to (a sixteenth).
const GRID: f64 = 0.25;
/// Beats a chord lasts: a bar.
const CHORD_BEATS: f64 = 4.0;

/// `notes` on the grid, one at a time (a later one cuts the one before),
/// within `length` beats.
pub fn quantize(notes: &[Note], length: f64) -> Vec<Note> {
    let snap = |b: f64| (b / GRID).round() * GRID;
    let mut out: Vec<Note> = notes
        .iter()
        .map(|n| {
            let start = snap(n.start).clamp(0.0, length);
            let end = snap(n.start + n.length).max(start + GRID).min(length);
            Note { start, length: end - start, ..*n }
        })
        .filter(|n| n.length > 1e-9)
        .collect();
    out.sort_by(|a, b| a.start.total_cmp(&b.start).then(b.length.total_cmp(&a.length)));
    // One at a time: of two on one step, the longer; each cut where the next starts.
    out.dedup_by(|later, earlier| (later.start - earlier.start).abs() < 1e-9);
    for i in 0..out.len().saturating_sub(1) {
        let next = out[i + 1].start;
        if out[i].start + out[i].length > next {
            out[i].length = next - out[i].start;
        }
    }
    out
}

/// The key `notes` are in: the project's, or the one they sound in.
pub fn key_of(notes: &[Note], project: Option<Key>) -> Option<Key> {
    project.or_else(|| {
        let mut weights = [0f64; 12];
        for n in notes {
            weights[n.pitch as usize % 12] += n.length;
        }
        Key::detect(&weights)
    })
}

/// The chords for a melody `length` beats long in `key`: one per
/// [`CHORD_BEATS`], the same one running on joined (start and end beats).
pub fn harmonize(notes: &[Note], length: f64, key: Key) -> Vec<(f64, f64, Chord)> {
    let segments = (length / CHORD_BEATS).ceil().max(1.0) as usize;
    // The chords it chooses from (by degree), and in a minor key a major V too.
    let mut options: Vec<(usize, Chord)> = (0..7).map(|d| (d, chord::on_degree(key, d, false, false))).collect();
    if key.minor {
        options.push((4, chord::on_degree(key, 4, false, true)));
    }
    let scale = key.scale();
    // How well a chord fits the melody over a segment.
    let fit = |s: usize, chord: &Chord, degree: usize| -> f64 {
        let (from, to) = (s as f64 * CHORD_BEATS, (s as f64 + 1.0) * CHORD_BEATS);
        let tones = chord.classes();
        let mut score = 0.0;
        for n in notes {
            let overlap = (n.start + n.length).min(to) - n.start.max(from);
            if overlap <= 0.0 {
                continue;
            }
            // On a strong beat (the first or third of the bar), it counts more.
            let strong = n.start >= from - 1e-9 && ((n.start - from) % 2.0).abs() < 1e-9;
            let weight = overlap * if strong { 1.5 } else { 1.0 };
            let class = n.pitch % 12;
            score += weight * if tones.contains(&class) { 1.0 } else if scale[class as usize] { -0.25 } else { -0.6 };
        }
        // The usual chords a little more than the rare ones (iii, vii°).
        score + match degree {
            0 => 0.15,
            3 | 4 => 0.1,
            5 => 0.05,
            2 => -0.05,
            6 => -0.3,
            _ => 0.0,
        }
    };
    // Viterbi over the segments: the best way to each chord.
    let n = options.len();
    let mut score: Vec<f64> = options.iter().map(|(d, c)| fit(0, c, *d) + if *d == 0 { 0.3 } else { 0.0 }).collect();
    let mut back: Vec<Vec<usize>> = Vec::new();
    for s in 1..segments {
        let mut next = vec![f64::NEG_INFINITY; n];
        let mut from = vec![0; n];
        for (j, (dj, cj)) in options.iter().enumerate() {
            for (i, (di, _)) in options.iter().enumerate() {
                let step = if i == j { 0.05 } else if chord::next_degrees(key, *di).contains(dj) { 0.25 } else { 0.0 };
                if score[i] + step > next[j] {
                    next[j] = score[i] + step;
                    from[j] = i;
                }
            }
            next[j] += fit(s, cj, *dj) + if s + 1 == segments && *dj == 0 { 0.3 } else { 0.0 };
        }
        back.push(from);
        score = next;
    }
    let mut state = (0..n).max_by(|&a, &b| score[a].total_cmp(&score[b])).unwrap_or(0);
    let mut path = vec![state; segments];
    for (s, from) in back.iter().enumerate().rev() {
        state = from[state];
        path[s] = state;
    }
    // The same chord on: one.
    let mut chords: Vec<(f64, f64, Chord)> = Vec::new();
    for (s, &i) in path.iter().enumerate() {
        let (from, to) = (s as f64 * CHORD_BEATS, ((s + 1) as f64 * CHORD_BEATS).min(length));
        match chords.last_mut() {
            Some(last) if last.2 == options[i].1 => last.1 = to,
            _ => chords.push((from, to, options[i].1)),
        }
    }
    chords
}

/// The parts a style is played with: how the chords go and the bass.
fn parts(style: Style) -> (Part, Part) {
    match style {
        Style::House | Style::Techno | Style::Hardgroove | Style::HardTechno => (Part::Stabs, Part::BassOffbeat),
        Style::Reggaeton => (Part::Stabs, Part::BassRoot),
        Style::Breakbeat => (Part::Pad, Part::BassOctaves),
        Style::Trap | Style::BoomBap | Style::DrumAndBass | Style::PopRock => (Part::Pad, Part::BassRoot),
    }
}

/// A melody being heard in an audio clip.
pub struct HumJob {
    clip: ClipRef,
    audio: AudioClip,
    style: Style,
    result: Arc<Mutex<Option<(Vec<stems::transcribe::NoteEvent>, Option<Key>)>>>,
}

impl PulseApp {
    /// Starts making a song from `clip`: a hummed tune (audio, heard in the
    /// background) or a melody (MIDI, at once).
    pub fn hum_to_song(&mut self, clip: ClipRef, style: Style) {
        match self.clip(clip).cloned() {
            Some(Clip::Midi(m)) => {
                let length = m.length;
                self.build_song(clip, &m.notes, length, None, style, &m.name);
            }
            Some(Clip::Audio(audio)) => {
                if self.hum_song.is_some() {
                    self.status = Some("Ya se está escuchando un tarareo: espera a que termine".into());
                    return;
                }
                let sample = self.sample(&audio.sample);
                if sample.frames() == 0 {
                    self.status = Some("No se encuentra el audio del clip".into());
                    return;
                }
                let (tempo, key, a) = (self.project.tempo, self.project.key, audio.clone());
                let result = Arc::new(Mutex::new(None));
                let out = result.clone();
                std::thread::spawn(move || {
                    let rate = sample.sample_rate;
                    let from = a.offset.max(0.0);
                    let to = a.source_seconds(a.length, tempo).min(sample.duration());
                    let (i, j) = ((from * rate as f64) as usize, ((to * rate as f64) as usize).min(sample.frames()));
                    let mono: Vec<f32> = sample.left[i.min(j)..j].iter().zip(&sample.right[i.min(j)..j]).map(|(l, r)| 0.5 * (l + r)).collect();
                    let (mut events, heard) = match key {
                        Some(k) => (stems::pitch::melody_in(&mono, rate, k), Some(k)),
                        None => stems::pitch::melody(&mono, rate, [0.0; 12], true),
                    };
                    for e in &mut events {
                        e.start += from;
                        e.end += from;
                    }
                    *out.lock().unwrap_or_else(|e| e.into_inner()) = Some((events, heard));
                });
                self.status = Some(format!("Escuchando la melodía de «{}»…", audio.name));
                self.hum_song = Some(HumJob { clip, audio, style, result });
            }
            None => {}
        }
    }

    /// Once a frame: the song, when the melody has been heard.
    pub fn follow_hum_song(&mut self, ctx: &eframe::egui::Context) {
        let Some(job) = &self.hum_song else { return };
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let Some((events, key)) = job.result.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
        let job = self.hum_song.take().expect("checked above");
        let notes = crate::to_midi::clip_notes(&events, &job.audio, self.project.tempo, false);
        self.build_song(job.clip, &notes, job.audio.length, key, job.style, &job.audio.name);
    }

    /// Waits for the melody being heard (tests).
    #[cfg(test)]
    pub fn wait_hum_song(&mut self) {
        let ctx = eframe::egui::Context::default();
        let started = std::time::Instant::now();
        while self.hum_song.is_some() {
            self.follow_hum_song(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert!(started.elapsed().as_secs() < 60, "the melody was never heard");
        }
    }

    /// The song: the melody, its chords, a bass and a beat, on new tracks under
    /// `clip`'s, where it is (its scene, or its place on the Arrangement).
    fn build_song(&mut self, clip: ClipRef, notes: &[Note], length: f64, heard: Option<Key>, style: Style, name: &str) {
        let length = (length / 4.0).ceil().max(1.0) * 4.0;
        let melody = quantize(notes, length);
        if melody.len() < 2 {
            self.status = Some(format!("No se oye una melodía en «{name}»: tararea, canta o silba unas notas, con el metrónomo"));
            return;
        }
        let Some(key) = key_of(&melody, self.project.key.or(heard)) else {
            self.status = Some("No se sabe en qué tonalidad está la melodía: elige la escala del proyecto y vuelve a probar".into());
            return;
        };
        if self.project.tracks.len() + 4 > MAX_TRACKS {
            self.status = Some(format!("Hacen falta 4 pistas más y el máximo es {MAX_TRACKS}"));
            return;
        }
        if self.project.key.is_none() {
            self.set_key(Some(key));
        }
        let chords = harmonize(&melody, length, key);
        let (harmony, bass_part) = parts(style);
        let seed = crate::versions::now();
        let chord_notes = crate::accompany::generate(harmony, &chords, length, seed);
        let bass = crate::accompany::generate(bass_part, &chords, length, seed ^ 0x9E37);
        let mut drums = crate::project::Track::new("Batería", crate::project::PALETTE[0], InstrumentKind::DrumMachine(style.sound()));
        let kit = drum_gen::Kit::from_rows(&drums.drum_rows());
        let settings = drum_gen::Settings { style, level: Level::Medium, bars: (length / 4.0) as u32, sounds: style.sounds(), ..Default::default() };
        let (beat, groove) = drum_gen::generate(&kit, &settings, length, &[], seed);
        let colour = self.clip(clip).map_or([128; 3], |c| c.color());
        let mut lead = crate::project::Track::new("Melodía", colour, InstrumentKind::Synth(SynthParams::default()));
        let mut keys = crate::project::Track::new("Acordes", crate::project::PALETTE[4], harmony.instrument());
        let mut low = crate::project::Track::new("Bajo", crate::project::PALETTE[8], bass_part.instrument());
        let clips = [
            (&mut lead, MidiClip { notes: melody.clone(), ..MidiClip::new("Melodía", length, colour) }),
            (&mut keys, MidiClip { notes: chord_notes, ..MidiClip::new("Acordes", length, crate::project::PALETTE[4]) }),
            (&mut low, MidiClip { notes: bass, ..MidiClip::new("Bajo", length, crate::project::PALETTE[8]) }),
            (&mut drums, MidiClip { notes: beat, groove, ..MidiClip::new(style.label(), length, crate::project::PALETTE[0]) }),
        ];
        let source = clip.track();
        let placed = match clip {
            ClipRef::Arrangement { track, index } => self.project.tracks[track].arrangement.get(index).map(|a| a.start),
            ClipRef::Session { .. } => None,
        };
        let depth = self.project.tracks[source].depth;
        let mut tracks = Vec::new();
        for (track, midi) in clips {
            match (clip, placed) {
                (ClipRef::Session { slot, .. }, _) => track.clips[slot] = Some(Clip::Midi(midi)),
                (_, Some(start)) => track.arrangement.push(engine::model::ArrangementClip { start, length, clip: Clip::Midi(midi) }),
                _ => {}
            }
            track.depth = depth;
            tracks.push(track.clone());
        }
        let ids = self.track_ids();
        for (k, track) in tracks.into_iter().enumerate() {
            self.project.tracks.insert(source + 1 + k, track);
        }
        self.follow_tracks(&ids);
        self.selected_target = Target::Track(source + 1);
        self.sync_all();
        self.select_clip(match clip {
            ClipRef::Session { slot, .. } => ClipRef::Session { track: source + 1, slot },
            ClipRef::Arrangement { .. } => ClipRef::Arrangement { track: source + 1, index: 0 },
        });
        let names: Vec<String> = chords.iter().map(|c| c.2.name()).collect();
        self.status = Some(format!("Canción en {}: {} notas de melodía, acordes {}, bajo y batería {}", key.name(), melody.len(), names.join(" "), style.label()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(start: f64, length: f64, pitch: u8) -> Note {
        Note { start, length, pitch, velocity: 0.8, chance: 1.0 }
    }

    /// A tune: each bar's notes (a quarter each).
    fn tune(bars: &[[u8; 4]]) -> Vec<Note> {
        bars.iter().enumerate().flat_map(|(b, ns)| ns.iter().enumerate().map(move |(k, &p)| note(b as f64 * 4.0 + k as f64, 1.0, p))).collect()
    }

    fn names(chords: &[(f64, f64, Chord)]) -> Vec<String> {
        chords.iter().map(|c| c.2.name()).collect()
    }

    #[test]
    fn a_tune_gets_the_chords_its_notes_are() {
        let c = Key { tonic: 0, minor: false };
        // C E G E | F A C A | G B D B | C G E C.
        let melody = tune(&[[60, 64, 67, 64], [65, 69, 72, 69], [67, 71, 74, 71], [72, 67, 64, 60]]);
        let chords = harmonize(&melody, 16.0, c);
        assert_eq!(names(&chords), ["Do", "Fa", "Sol", "Do"]);
        assert_eq!(chords.iter().map(|c| (c.0, c.1)).collect::<Vec<_>>(), [(0.0, 4.0), (4.0, 8.0), (8.0, 12.0), (12.0, 16.0)]);
    }

    /// In A minor, a bar on G# (the raised seventh) gets E major, and the tune
    /// ends at home.
    #[test]
    fn a_minor_tune_has_its_major_five() {
        let a = Key { tonic: 9, minor: true };
        let melody = tune(&[[69, 72, 76, 72], [74, 77, 81, 77], [68, 71, 76, 71], [69, 64, 60, 57]]);
        assert_eq!(names(&harmonize(&melody, 16.0, a)), ["Lam", "Rem", "Mi", "Lam"]);
    }

    /// Where the tune says little (a held note that fits several, a bar of
    /// silence), the chords that usually follow: V, then home.
    #[test]
    fn where_the_tune_says_little_the_usual_way() {
        let c = Key { tonic: 0, minor: false };
        // C held a bar, G held a bar (G fits C, Em and G), silence, C.
        let melody = vec![note(0.0, 4.0, 60), note(4.0, 4.0, 67), note(12.0, 4.0, 60)];
        let chords = harmonize(&melody, 16.0, c);
        println!("{:?}", names(&chords));
        assert_eq!(names(&chords), ["Do", "Sol", "Do"]);
        assert!(chords.iter().all(|c| !c.2.name().contains('°')), "no diminished chord");
    }

    #[test]
    fn a_hummed_tune_goes_on_the_grid_one_note_at_a_time() {
        let hummed = vec![note(0.07, 0.9, 60), note(0.96, 0.6, 62), note(1.02, 0.1, 61), note(1.55, 2.9, 64), note(15.9, 1.0, 67)];
        let q = quantize(&hummed, 16.0);
        // The 61 (on the 62's step, shorter) goes; the one past the end too.
        assert_eq!(q.iter().map(|n| (n.start, n.length, n.pitch)).collect::<Vec<_>>(), [(0.0, 1.0, 60), (1.0, 0.5, 62), (1.5, 3.0, 64)]);
    }

    #[test]
    fn the_key_is_the_projects_or_the_tunes() {
        let melody = tune(&[[69, 72, 76, 72], [74, 77, 81, 77], [64, 71, 76, 71], [69, 64, 60, 57]]);
        let heard = key_of(&melody, None).unwrap();
        assert!(heard.scale() == Key { tonic: 9, minor: true }.scale(), "{}", heard.name());
        let d = Key { tonic: 2, minor: false };
        assert_eq!(key_of(&melody, Some(d)), Some(d));
    }

    /// From a MIDI melody in the demo: four tracks under it, a bar of chords
    /// each in the scale, and the key set.
    #[test]
    fn a_melody_becomes_a_song() {
        let mut project = crate::project::Project::empty();
        let mut voice = crate::project::Track::new("Tarareo", [10, 20, 30], InstrumentKind::Synth(Default::default()));
        voice.id = 1;
        let melody = tune(&[[60, 64, 67, 64], [65, 69, 72, 69], [67, 71, 74, 71], [72, 67, 64, 60]]);
        voice.arrangement = vec![engine::model::ArrangementClip { start: 8.0, length: 16.0, clip: Clip::Midi(MidiClip { notes: melody, ..MidiClip::new("Tarareo", 16.0, [10, 20, 30]) }) }];
        project.tracks = vec![voice];
        let mut app = PulseApp::headless(project);
        app.hum_to_song(ClipRef::Arrangement { track: 0, index: 0 }, Style::PopRock);
        let names: Vec<&str> = app.project.tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Tarareo", "Melodía", "Acordes", "Bajo", "Batería"], "{:?}", app.status);
        assert_eq!(app.project.key, Some(Key { tonic: 0, minor: false }));
        for t in &app.project.tracks[1..] {
            let placed = &t.arrangement[0];
            assert_eq!((placed.start, placed.length), (8.0, 16.0), "{}", t.name);
            let Clip::Midi(m) = &placed.clip else { panic!() };
            assert!(!m.notes.is_empty(), "{}", t.name);
        }
        // The chords' notes are C's, then F's, G's, C's.
        let Clip::Midi(chords) = &app.project.tracks[2].arrangement[0].clip else { panic!() };
        for n in &chords.notes {
            let bar = (n.start / 4.0) as usize;
            let tones: &[u8] = [&[0, 4, 7][..], &[5, 9, 0], &[7, 11, 2], &[0, 4, 7]][bar];
            assert!(tones.contains(&(n.pitch % 12)), "{} in bar {bar}", n.pitch);
        }
        assert!(app.project.tracks[4].is_drums());
        assert!(app.status.as_deref().unwrap_or("").contains("Do Fa Sol Do"), "{:?}", app.status);
    }

    /// Hummed into an audio clip (a voice-like tone, a quarter note each, at
    /// the project's 120): heard, then the same song.
    #[test]
    fn a_hummed_tune_becomes_a_song() {
        let rate = 48_000.0f32;
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("tarareo-{}.wav", std::process::id()));
        let tune = [60u8, 64, 67, 64, 65, 69, 72, 69, 67, 71, 74, 71, 72, 67, 64, 60];
        let mut phase = 0f32;
        let audio: Vec<f32> = (0..(8.0 * rate) as usize)
            .map(|i| {
                let t = i as f32 / rate;
                let k = (t / 0.5) as usize;
                let within = t - k as f32 * 0.5;
                // Hummed: a little late, sharp and with a breath between notes.
                let hz = 440.0 * 2f32.powf((tune[k.min(15)] as f32 + 0.2 - 69.0) / 12.0);
                phase = (phase + hz / rate).fract();
                let level = if within < 0.03 || within > 0.45 { 0.0 } else { 0.25 };
                level * ((std::f32::consts::TAU * phase).sin() + 0.4 * (2.0 * std::f32::consts::TAU * phase).sin())
            })
            .collect();
        crate::export::write(&path, &audio, &audio, rate as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
        let mut project = crate::project::Project::empty();
        let mut voice = crate::project::Track::new_audio("Voz", [0; 3]);
        voice.id = 1;
        let clip = AudioClip { hard_start: false, fades: Default::default(), name: "Tarareo".into(), color: [0; 3], sample: path.to_string_lossy().into_owned(), length: 16.0, offset: 0.0, warp: engine::model::WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false };
        voice.clips[0] = Some(Clip::Audio(clip));
        project.tracks = vec![voice];
        let mut app = PulseApp::headless(project);
        app.hum_to_song(ClipRef::Session { track: 0, slot: 0 }, Style::House);
        app.wait_hum_song();
        assert_eq!(app.project.tracks.len(), 5, "{:?}", app.status);
        let Some(Clip::Midi(melody)) = &app.project.tracks[1].clips[0] else { panic!() };
        let heard: Vec<(f64, u8)> = melody.notes.iter().map(|n| (n.start, n.pitch)).collect();
        println!("{heard:?}");
        assert_eq!(heard.iter().map(|h| h.1).collect::<Vec<_>>(), tune);
        assert!(heard.iter().enumerate().all(|(k, h)| h.0 == k as f64), "each on its beat");
        assert!(app.status.as_deref().unwrap_or("").contains("Do Fa Sol Do"), "{:?}", app.status);
    }
}
