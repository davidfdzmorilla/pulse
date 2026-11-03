//! The example song Pulse opens the first time (and from «¿Qué quieres
//! hacer?», and what `pulse --render` plays without a project): a short
//! melodic techno track made of Pulse's own sounds. Factory kits, a bass, a
//! pad and a lead written by «Acompañar» from the chords, a sung voice with
//! its lyrics, a drum group, two returns, scenes to play in the Session and an
//! arrangement with sections, transitions and automation.
//!
//! Everything is made in code with fixed seeds: the same song every time.
//! What it plays (the voice, the transitions) is written once into a folder
//! of its own in the data folder; the kits are Pulse's sounds (see
//! `factory_sounds`).

use crate::accompany::Part;
use crate::lyrics::{LyricLine, Lyrics};
use crate::melody::Melody;
use crate::project::{Project, Track, PALETTE};
use engine::model::{ArrangementClip, AudioClip, AutoFilterParams, AutomationLane, AutomationPoint, Clip, ControlLane, Device, Effect, InstrumentKind, MidiClip, MidiControl, Note, Param, WarpMode};
use std::path::{Path, PathBuf};

/// Its name, for the window's title and the status bar.
pub const NAME: &str = "Luces de la ciudad";
/// Bumped when the files it plays change: made anew in a folder of its own.
const VERSION: &str = "v1";
pub const TEMPO: f64 = 122.0;
/// D minor.
const KEY: stems::key::Key = stems::key::Key { tonic: 2, minor: true };
const BARS: f64 = 16.0;
const SEED: u64 = 0x5EED_0122;

// Scenes.
pub const INTRO: usize = 0;
pub const DROP: usize = 1;
pub const BREAK: usize = 2;

/// Where its files are made (tests use their own folder).
pub fn folder() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("pulse-test-ejemplo-{}", std::process::id())).join(VERSION);
    }
    paths::data_dir().join("Ejemplo").join(VERSION)
}

/// The example song, ready to play.
pub fn song() -> Result<Project, String> {
    build(&folder())
}

/// The example song with its files in `dir` (made if missing).
pub fn build(dir: &Path) -> Result<Project, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    let (mut project, voice_beats) = tracks(&dir.join("Voz principal.wav"))?;
    arrange(&mut project, voice_beats, dir)?;
    project.assign_ids();
    Ok(project)
}

fn color(i: usize) -> [u8; 3] {
    PALETTE[i]
}

/// A factory effect preset by (the end of) its name.
pub fn effect(kind: Effect, name: &str) -> Effect {
    kind.factory_presets().into_iter().find(|(n, _)| n.ends_with(name)).map(|(_, e)| e).unwrap_or(kind)
}

/// A factory instrument preset by its name.
pub fn instrument(kind: InstrumentKind, name: &str) -> InstrumentKind {
    kind.factory_presets().into_iter().find(|(n, _)| *n == name).map(|(_, i)| i).unwrap_or(kind)
}

/// A factory kit on a Drum Pads track, and its pattern over four bars.
pub fn kit_track(name: &str, kit: &str, track_color: [u8; 3]) -> Result<(Track, Vec<(f64, u8, f32)>), String> {
    let kits = crate::factory_sounds::kits();
    let kit = kits.iter().find(|k| k.name == kit).ok_or(format!("No hay ningún kit «{kit}»"))?;
    let (params, files) = crate::factory_sounds::make_kit(&crate::factory_sounds::root(), kit)?;
    let mut track = Track::new(name, track_color, InstrumentKind::DrumRack(params));
    track.samples = files;
    let two_bars = crate::factory_sounds::pattern_notes(kit);
    let pattern = two_bars.iter().copied().chain(two_bars.iter().map(|&(b, p, v)| (b + 8.0, p, v))).collect();
    Ok((track, pattern))
}

fn drum_clip(name: &str, track_color: [u8; 3], pattern: &[(f64, u8, f32)], keep: &[u8]) -> Clip {
    let mut clip = MidiClip::new(name, BARS, track_color);
    clip.notes = pattern.iter().filter(|(_, p, _)| keep.contains(p)).map(|&(start, pitch, velocity)| Note::new(start, 0.25, pitch, velocity)).collect();
    Clip::Midi(clip)
}

fn midi(name: &str, track_color: [u8; 3], notes: Vec<Note>) -> Clip {
    Clip::Midi(MidiClip { notes, ..MidiClip::new(name, BARS, track_color) })
}

/// The chords, Dm Bb F C, as stabs a bar each: on the one, the «and» of two
/// and on four.
fn chord_notes() -> Vec<Note> {
    let chords: [[u8; 4]; 4] = [[50, 53, 57, 62], [46, 50, 53, 58], [45, 48, 53, 57], [48, 52, 55, 60]];
    let mut notes = Vec::new();
    for (bar, chord) in chords.iter().enumerate() {
        for (at, length, velocity) in [(0.0, 1.25, 0.9), (1.5, 1.0, 0.75), (3.0, 0.75, 0.8)] {
            for &pitch in chord {
                notes.push(Note::new(bar as f64 * 4.0 + at, length, pitch, velocity));
            }
        }
    }
    notes
}

const PHRASES: [(&str, &[u8]); 4] = [
    ("Cuando la ciudad se apaga", &[69, 69, 67, 65, 64, 65, 67, 65, 62]),
    ("seguimos bailando", &[65, 67, 69, 69, 67, 69]),
    ("Somos luz en la oscuridad", &[69, 72, 69, 67, 65, 67, 65, 64, 62]),
    ("No mires atrás", &[62, 64, 65, 67, 69, 69]),
];

/// A phrase's syllables: each vowel group with the letters before it, and its vowel.
fn syllables(text: &str) -> Vec<(String, char)> {
    let mut syllables: Vec<(String, char)> = Vec::new();
    let mut pending = String::new();
    for c in text.to_lowercase().chars() {
        let vowel = match c {
            'a' | 'á' => Some('a'),
            'e' | 'é' => Some('e'),
            'i' | 'í' => Some('i'),
            'o' | 'ó' => Some('o'),
            'u' | 'ú' => Some('u'),
            _ => None,
        };
        match vowel {
            Some(v) if pending.chars().last().is_some_and(|p| "aeiouáéíóú".contains(p)) && !"aeo".contains(v) => pending.push(c),
            Some(v) => {
                pending.push(c);
                syllables.push((std::mem::take(&mut pending), v));
            }
            None if c != ' ' => pending.push(c),
            None => {}
        }
    }
    syllables
}

/// When each phrase is sung (seconds), and how long the voice lasts.
fn phrase_times() -> (f64, Vec<(f64, f64, &'static str)>) {
    let bar = 4.0 * 60.0 / TEMPO;
    let lines = PHRASES
        .iter()
        .enumerate()
        .map(|(k, (text, _))| {
            let start = k as f64 * bar * 2.0 + 0.25 * bar;
            let length = bar * 1.6;
            let step = length / syllables(text).len() as f64;
            (start, start + length - step * 0.2, *text)
        })
        .collect();
    (bar * 8.0, lines)
}

/// A sung voice, made here: a glottal tone through the formants of each
/// syllable's vowel, a breath of noise on its consonants, over D minor.
/// Written to `path` (24-bit, 48 kHz) unless it is there already.
fn sing(path: &Path) -> Result<(), String> {
    if path.is_file() {
        return Ok(());
    }
    const RATE: f32 = 48_000.0;
    let (seconds, lines) = phrase_times();
    let n = (seconds * RATE as f64) as usize;
    let mut out = vec![0.0f32; n];
    let mut seed = 0x2545_F491u32;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    };
    let bar = 4.0 * 60.0 / TEMPO;
    for ((text, notes), &(start, _, _)) in PHRASES.iter().zip(&lines) {
        let syllables = syllables(text);
        let length = bar * 1.6;
        let step = length / syllables.len() as f64;
        let mut f0 = hz(notes[0] as f32);
        for (i, (letters, vowel)) in syllables.iter().enumerate() {
            let target = hz(notes[i.min(notes.len() - 1)] as f32);
            let (f1, f2, f3) = match vowel {
                'a' => (800.0, 1250.0, 2600.0),
                'e' => (450.0, 1900.0, 2600.0),
                'i' => (300.0, 2300.0, 3000.0),
                'o' => (500.0, 850.0, 2500.0),
                _ => (330.0, 750.0, 2400.0),
            };
            let consonant = letters.chars().next().is_some_and(|c| !"aeiouáéíóú".contains(c));
            let hiss = letters.starts_with('s') || letters.starts_with('c') || letters.starts_with('z');
            let from = ((start + i as f64 * step) * RATE as f64) as usize;
            let held = if i + 1 == syllables.len() { step * 1.6 } else { step * 0.92 };
            let to = (from + (held * RATE as f64) as usize).min(n);
            let attack = (0.02 * RATE) as usize;
            let mut phase = [0.0f32; 24];
            let mut previous = 0.0f32;
            for (j, s) in out[from..to].iter_mut().enumerate() {
                let t = j as f32 / RATE;
                f0 += (target - f0) * 0.0015;
                let vibrato = 1.0 + 0.008 * (t * 5.5 * std::f32::consts::TAU).sin() * (t * 2.0).min(1.0);
                let hz = f0 * vibrato;
                let mut tone = 0.0;
                for (h, ph) in phase.iter_mut().enumerate() {
                    let fh = hz * (h + 1) as f32;
                    if fh > 5000.0 {
                        break;
                    }
                    *ph = (*ph + fh / RATE).fract();
                    let peak = |f: f32, bw: f32, gain: f32| gain * (-((fh - f) / bw).powi(2)).exp();
                    let amp = (peak(f1, 130.0, 1.0) + peak(f2, 160.0, 0.6) + peak(f3, 220.0, 0.25)) / (h + 1) as f32 + 0.02 / (h + 1) as f32;
                    tone += amp * (*ph * std::f32::consts::TAU).sin();
                }
                let len = to - from;
                let env = (j as f32 / attack as f32).min(1.0) * ((len - j) as f32 / (0.05 * RATE)).min(1.0) * (1.0 - 0.45 * j as f32 / len as f32) * (0.85 + 0.15 * (t * 7.0 * std::f32::consts::TAU).sin()) * (1.0 - 0.25 * (i % 3) as f32 / 2.0);
                let consonant_len = if hiss { 0.09 } else { 0.03 };
                let breath = if consonant && t < consonant_len {
                    let white = noise();
                    let high = white - previous;
                    previous = white;
                    high * if hiss { 0.25 } else { 0.4 } * (1.0 - t / consonant_len)
                } else {
                    0.0
                };
                let voiced = if consonant { ((t - consonant_len * 0.6) / 0.02).clamp(0.0, 1.0) } else { 1.0 };
                *s += tone * env * voiced * 0.5 + breath;
            }
        }
    }
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-6);
    for s in &mut out {
        *s *= 0.7 / peak;
    }
    write_once(path, &out, &out, RATE as u32)
}

/// Writes a WAV beside `path` and moves it there: a file half written (Pulse
/// closed meanwhile) is never taken for a finished one.
fn write_once(path: &Path, left: &[f32], right: &[f32], rate: u32) -> Result<(), String> {
    // Unique among the processes and threads making it at once.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let partial = path.with_extension(format!("{}-{n}.tmp", std::process::id()));
    crate::export::write(&partial, left, right, rate, crate::export::BitDepth::Int24, 1.0)?;
    std::fs::rename(&partial, path).map_err(|e| format!("No se pudo guardar {}: {e}", path.display()))
}

fn hz(pitch: f32) -> f32 {
    440.0 * 2f32.powf((pitch - 69.0) / 12.0)
}

/// The song: its tracks, scenes and mix, before its arrangement, with the
/// voice sung into `voice`; and how many beats the voice lasts.
pub fn tracks(voice: &Path) -> Result<(Project, f64), String> {
    let mut p = Project::empty();
    p.tempo = TEMPO;
    p.key = Some(KEY);
    p.scene_names[INTRO] = "Intro".into();
    p.scene_names[DROP] = "Drop".into();
    p.scene_names[BREAK] = "Break".into();
    let sig = p.meter.first();

    // Drums: a group with the Techno and Minimal kits.
    let mut group = Track::new_group("Batería", color(1));
    group.devices.push(Device::new(effect(Effect::Glue(Default::default()), "Bus de batería")));
    group.devices.push(Device::new(effect(Effect::Convolution(Default::default()), "Batería · Habitación")));
    group.mixer.gain = 0.55;
    let (mut kick, pattern) = kit_track("Bombo y charles", "Techno", color(1))?;
    kick.depth = 1;
    kick.clips[INTRO] = Some(drum_clip("Bombo y charles", color(1), &pattern, &[36, 42]));
    kick.clips[DROP] = Some(drum_clip("Bombo y palmas", color(1), &pattern, &[36, 39, 42, 46]));
    let (mut perc, perc_pattern) = kit_track("Percusión", "Minimal", color(2))?;
    perc.depth = 1;
    perc.mixer.gain = 0.6;
    perc.sends[1] = 0.12;
    let percussion: Vec<u8> = perc_pattern.iter().map(|n| n.1).filter(|p| ![36, 38, 39, 40].contains(p)).collect();
    perc.clips[DROP] = Some(drum_clip("Percusión", color(2), &perc_pattern, &percussion));

    // Chords, and what «Acompañar» writes from them.
    let chord_color = color(6);
    let chords = chord_notes();
    let found: Vec<_> = crate::harmony::chords(&chords, BARS, Some(KEY), sig).into_iter().filter_map(|(a, b, c)| c.map(|c| (a, b, c))).collect();
    let mut keys = Track::new("Acordes", chord_color, instrument(InstrumentKind::Wavetable(Default::default()), "Teclas · Acordes future"));
    keys.mixer.gain = 0.34;
    keys.sends[0] = 0.25;
    keys.devices.push(Device::new(effect(Effect::Multiband(Default::default()), "OTT suave")));
    for scene in [INTRO, DROP, BREAK] {
        keys.clips[scene] = Some(midi("Rem Sib Fa Do", chord_color, chords.clone()));
    }

    let mut bass = Track::new("Bajo", color(9), Part::BassRolling.instrument());
    bass.mixer.gain = 0.4;
    bass.clips[DROP] = Some(midi(Part::BassRolling.label(), color(9), crate::accompany::generate(Part::BassRolling, &found, BARS, SEED, sig)));

    let mut pad = Track::new("Pad", color(11), instrument(InstrumentKind::Wavetable(Default::default()), "Pad · Analógico cálido"));
    pad.mixer.gain = 0.3;
    pad.sends[0] = 0.4;
    pad.devices.push(Device::new(Effect::AutoFilter(AutoFilterParams { cutoff: 600.0, ..Default::default() })));
    pad.devices.push(Device::new(effect(Effect::Multiband(Default::default()), "Brillo en pads")));
    let pad_notes = crate::accompany::generate(Part::Pad, &found, BARS, SEED, sig);
    pad.clips[DROP] = Some(midi("Pad", color(11), pad_notes.clone()));
    pad.clips[BREAK] = Some(midi("Pad", color(11), pad_notes));

    let melody = Melody::new(crate::melody::Style::Topline);
    let key = crate::melody::key_for(&found, Some(KEY));
    let tune = crate::melody::generate(melody, &found, BARS, SEED, sig, Some(key));
    let mut lead = Track::new("Melodía", color(7), Part::Melody(melody).instrument());
    lead.mixer.gain = 0.45;
    lead.sends[..2].copy_from_slice(&[0.2, 0.3]);
    let mut hook = MidiClip { notes: tune, ..MidiClip::new(Part::Melody(melody).label(), BARS, color(7)) };
    // Played with expression: the brightness opening along the phrase, a bend into the last note.
    let point = |beat: f64, value: f32, curve: f32| AutomationPoint { beat, value, curve };
    hook.controls.push(ControlLane { control: MidiControl::Cc(74), points: vec![point(0.0, 0.25, 0.0), point(4.0, 0.4, -0.5), point(11.5, 0.9, 0.0), point(12.0, 0.6, 0.4), point(16.0, 0.95, 0.0)] });
    hook.controls.push(ControlLane { control: MidiControl::PitchBend, points: vec![point(0.0, 0.0, 0.0), point(14.5, 0.0, 0.0), point(14.75, -0.5, 0.5), point(15.25, 0.0, 0.0)] });
    lead.clips[DROP] = Some(Clip::Midi(hook.clone()));
    lead.clips[BREAK] = Some(Clip::Midi(hook));

    // The voice, sung over the break.
    sing(voice)?;
    let (seconds, lines) = phrase_times();
    let mut vocal = Track::new_audio("Voz", color(0));
    vocal.mixer.gain = 0.5;
    vocal.sends[..2].copy_from_slice(&[0.3, 0.22]);
    vocal.devices.push(Device::new(Effect::Eq3(engine::model::Eq3Params { low_db: -6.0, high_db: 2.0, low_freq: 150.0, ..Default::default() })));
    vocal.devices.push(Device::new(Effect::Compressor(engine::model::CompressorParams { threshold_db: -20.0, ratio: 3.0, attack_ms: 5.0, release_ms: 80.0, makeup_db: 4.0, sidechain: None })));
    let beats = seconds * TEMPO / 60.0;
    vocal.clips[BREAK] = Some(audio_clip("Voz principal", color(0), voice, beats, WarpMode::Off));

    p.tracks = vec![group, kick, perc, bass, keys, pad, lead, vocal];
    crate::groups::normalize(&mut p.tracks);

    // Returns: a big space and a dotted delay.
    let mut space = effect(Effect::Convolution(Default::default()), "Sala grande");
    if let Effect::Convolution(c) = &mut space {
        c.mix = 1.0;
    }
    p.returns[0].name = "A Espacio".into();
    p.returns[0].devices = vec![Device::new(space)];
    p.returns[1].name = "B Delay".into();
    p.returns[1].devices = vec![Device::new(Effect::Delay(engine::model::DelayParams { time_beats: 0.75, feedback: 0.45, mix: 1.0, ..Default::default() }))];
    p.returns.truncate(2);
    p.master.devices.push(Device::new(effect(Effect::Glue(Default::default()), "Pegar la mezcla")));
    p.master.devices.push(Device::new(Effect::Limiter(Default::default())));
    p.lyrics.push(Lyrics {
        sample: voice.to_string_lossy().into_owned(),
        language: "es".into(),
        lines: lines.iter().map(|&(start, end, text)| LyricLine { start, end, words: crate::lyrics::words_of(text, start, end) }).collect(),
    });
    Ok((p, beats))
}

fn audio_clip(name: &str, color: [u8; 3], sample: &Path, beats: f64, warp: WarpMode) -> Clip {
    Clip::Audio(AudioClip {
        hard_start: false,
        fades: Default::default(),
        name: name.into(),
        color,
        sample: sample.to_string_lossy().into_owned(),
        length: beats,
        offset: 0.0,
        warp,
        original_bpm: TEMPO,
        warp_markers: Vec::new(),
        key: None,
        takes: Vec::new(),
        gain_db: 0.0,
        transpose: 0.0,
        envelopes: Vec::new(),
        muted: false,
    })
}

/// The song's parts and their bars: short, as an example is.
const SECTIONS: [u32; 7] = [8, 8, 16, 8, 8, 16, 8];

/// The arrangement from the scenes: intro, build, drop, break with the voice,
/// build, drop with the voice again, outro. Risers and impacts into the
/// drops (their audio written into `dir`), and the pad's filter opening on
/// the builds.
fn arrange(p: &mut Project, voice_beats: f64, dir: &Path) -> Result<(), String> {
    let mut sections = crate::arrange::plan(&p.tracks, &p.scene_names, crate::arrange::Template::Electronic);
    for (section, bars) in sections.iter_mut().zip(SECTIONS) {
        section.bars = bars;
    }
    crate::arrange::lay_out_project(p, &sections);
    let markers = p.markers.clone();
    let at = |name: &str| markers.iter().filter(|m| m.name == name).map(|m| m.beat).collect::<Vec<_>>();
    let length = |start: f64| markers.iter().map(|m| m.beat).find(|&b| b > start).unwrap_or(p.loop_end) - start;

    let vocal = p.tracks.iter().position(|t| t.name == "Voz").ok_or("sin voz")?;
    let clip = p.tracks[vocal].clips[BREAK].clone().ok_or("sin voz")?;
    p.tracks[vocal].arrangement = at("Break").into_iter().chain(at("Drop").into_iter().skip(1)).map(|start| ArrangementClip { start, length: voice_beats, clip: clip.clone() }).collect();

    // Transitions, on a track of their own, as «Transición aquí» puts them.
    let bar = p.meter.first().bar();
    let rate = 48_000.0;
    let made = |kind: crate::transitions::Kind, bars: u32| -> Result<(PathBuf, f64), String> {
        let beats = bar * bars as f64;
        let path = dir.join(format!("{} {bars}.wav", kind.label()));
        if !path.is_file() {
            let (l, r) = crate::transitions::render(kind, beats * 60.0 / TEMPO, rate, KEY.tonic);
            write_once(&path, &l, &r, rate as u32)?;
        }
        Ok((path, beats))
    };
    let (riser, riser_beats) = made(crate::transitions::Kind::Riser, 8)?;
    let (impact, impact_beats) = made(crate::transitions::Kind::Impact, 1)?;
    let mut transitions = Track::new_audio(crate::transitions::TRACK, color(4));
    transitions.mixer.gain = 0.5;
    for drop in at("Drop") {
        let riser_clip = audio_clip("Subida 8 compases", color(4), &riser, riser_beats, WarpMode::Stretch);
        transitions.arrangement.push(ArrangementClip { start: (drop - riser_beats).max(0.0), length: riser_beats, clip: riser_clip });
        let impact_clip = audio_clip("Impacto 1 compás", color(4), &impact, impact_beats, WarpMode::Stretch);
        transitions.arrangement.push(ArrangementClip { start: drop, length: impact_beats, clip: impact_clip });
    }
    transitions.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
    p.tracks.push(transitions);

    // The pad's filter: closed, opening along each build and the break, open in the drops.
    let pad = p.tracks.iter().position(|t| t.name == "Pad").ok_or("sin pad")?;
    let mut lane = AutomationLane::new(Param::Device { device: 0, param: 0 });
    let point = |beat: f64, value: f32, curve: f32| AutomationPoint { beat, value, curve };
    for (name, from, to, before_end) in [("Subida", 500.0, 12_000.0, 1.0), ("Break", 900.0, 4_000.0, 4.0)] {
        for start in at(name) {
            lane.points.push(point(start, from, -0.5));
            lane.points.push(point(start + length(start) - before_end, to, 0.0));
        }
    }
    for start in at("Drop") {
        lane.points.push(point(start, 12_000.0, 0.0));
        lane.points.push(point(start + length(start) - 1.0, 12_000.0, 0.0));
    }
    lane.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    p.tracks[pad].automation.push(lane);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Its parts, its scenes and its sounds, all there and the same each time.
    #[test]
    fn the_example_has_a_song_and_scenes_to_play() {
        let project = song().unwrap();
        let names: Vec<&str> = project.markers.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Intro", "Subida", "Drop", "Break", "Subida", "Drop", "Outro"]);
        assert_eq!(project.loop_end, SECTIONS.iter().sum::<u32>() as f64 * 4.0);
        assert_eq!(&project.scene_names[..3], ["Intro", "Drop", "Break"]);
        for scene in [INTRO, DROP, BREAK] {
            assert!(project.tracks.iter().filter(|t| t.clips[scene].is_some()).count() >= 2, "escena {scene}");
        }
        assert!(project.key.is_some() && !project.lyrics.is_empty());
        assert!(project.tracks.iter().any(|t| t.automation.iter().any(|l| l.points.len() > 4)));
        assert!(crate::library::missing_files(&project).is_empty(), "{:?}", crate::library::missing_files(&project));
        // Spanish names throughout.
        for track in &project.tracks {
            assert!(!["Drums", "Bass", "Chords", "Lead"].contains(&track.name.as_str()), "{}", track.name);
        }
        // Made again from its files: the same song.
        assert_eq!(song().unwrap(), project);
        // Saved and opened again.
        let again = Project::from_json(&project.to_json().unwrap()).unwrap();
        assert_eq!(again.tracks.len(), project.tracks.len());
    }

    /// `pulse --render` without a project plays the example: every bar
    /// sounds, nothing clips, and it is about as loud as a finished song.
    /// With `PULSE_EJEMPLO=dir` the WAV stays there.
    #[test]
    fn the_example_renders_from_the_command_line() {
        let keep = std::env::var_os("PULSE_EJEMPLO").map(PathBuf::from);
        let dir = keep.clone().unwrap_or_else(|| std::env::temp_dir().join(format!("pulse-ejemplo-render-{}", std::process::id())));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("Luces de la ciudad.wav");
        let started = std::time::Instant::now();
        let report = crate::export::render_from_command_line(&out, None).unwrap();
        let song = engine::Sample::load(&out).unwrap();
        let (l, r) = (&song.left, &song.right);
        let seconds = SECTIONS.iter().sum::<u32>() as f64 * 4.0 * 60.0 / TEMPO;
        assert!(song.duration() >= seconds && song.duration() < seconds + 12.0, "{} s", song.duration());
        let peak = crate::export::peak(l, r);
        let lufs = stems::loudness::integrated(l, r, song.sample_rate).unwrap();
        let bar = (4.0 * 60.0 / TEMPO * song.sample_rate as f64) as usize;
        let quietest = l.chunks(bar).take(SECTIONS.iter().sum::<u32>() as usize).map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt()).fold(f32::MAX, f32::min);
        eprintln!("{report} · {lufs:.1} LUFS · pico {:.2} dBFS · compás más bajo {:.1} dB · {:.1} s", 20.0 * peak.log10(), 20.0 * quietest.log10(), started.elapsed().as_secs_f64());
        assert!(quietest > 0.005, "un compás en silencio ({quietest})");
        assert!(peak <= 1.0, "satura: pico {peak}");
        assert!((-14.0..=-7.0).contains(&lufs), "{lufs} LUFS");
        if keep.is_none() {
            std::fs::remove_dir_all(&dir).ok();
        }
    }
}
