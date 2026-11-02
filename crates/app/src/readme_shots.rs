//! The README's pictures: a melodic techno track built in code (factory kits,
//! a bass, chords, a pad and a lead from «Acompañar», a sung voice with its
//! lyrics, a drum group, two returns, an arrangement with sections and
//! automation), drawn without a window and saved as PNG. Fixed seeds and no
//! clock: the same pictures every run.
//! `cargo test --release -p app readme_shots -- --ignored` writes them in
//! `docs/capturas/` (or in `PULSE_SHOTS`).

use crate::accompany::Part;
use crate::app::{ClipRef, Detail, PulseApp, View};
use crate::lyrics::{LyricLine, Lyrics};
use crate::melody::Melody;
use crate::produccion::Studio;
use crate::project::{Project, Track, PALETTE};
use eframe::egui::{pos2, Modifiers};
use engine::model::{ArrangementClip, AudioClip, AutoFilterParams, AutomationLane, AutomationPoint, Clip, ControlLane, Device, Effect, InstrumentKind, MidiClip, MidiControl, Note, Param, WarpMode};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const TEMPO: f64 = 122.0;
/// D minor.
const KEY: stems::key::Key = stems::key::Key { tonic: 2, minor: true };
const BARS: f64 = 16.0;
const SEED: u64 = 0x5EED_0122;

// Scenes.
const INTRO: usize = 0;
const DROP: usize = 1;
const BREAK: usize = 2;

/// Where the pictures go.
fn out_dir() -> PathBuf {
    std::env::var_os("PULSE_SHOTS").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/capturas"))
}

fn color(i: usize) -> [u8; 3] {
    PALETTE[i]
}

/// A factory effect preset by (the end of) its name.
fn effect(kind: Effect, name: &str) -> Effect {
    kind.factory_presets().into_iter().find(|(n, _)| n.ends_with(name)).map(|(_, e)| e).unwrap_or_else(|| panic!("no hay preset «{name}»"))
}

/// A factory instrument preset by its name.
fn instrument(kind: InstrumentKind, name: &str) -> InstrumentKind {
    kind.factory_presets().into_iter().find(|(n, _)| *n == name).map(|(_, i)| i).unwrap_or_else(|| panic!("no hay preset «{name}»"))
}

/// A factory kit on a Drum Pads track, its pattern (the pads in `keep`) over
/// four bars.
fn kit_track(name: &str, kit: &str, track_color: [u8; 3]) -> (Track, Vec<(f64, u8, f32)>) {
    let kits = crate::factory_sounds::kits();
    let kit = kits.iter().find(|k| k.name == kit).unwrap();
    let (params, files) = crate::factory_sounds::make_kit(&crate::factory_sounds::root(), kit).unwrap();
    let mut track = Track::new(name, track_color, InstrumentKind::DrumRack(params));
    track.samples = files;
    let two_bars = crate::factory_sounds::pattern_notes(kit);
    let pattern = two_bars.iter().copied().chain(two_bars.iter().map(|&(b, p, v)| (b + 8.0, p, v))).collect();
    (track, pattern)
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

/// A sung voice, made here: a glottal tone through the formants of each
/// syllable's vowel, a breath of noise on its consonants, over D minor. Its
/// phrases and when each is sung (seconds).
fn sing(path: &Path) -> (f64, Vec<(f64, f64, &'static str)>) {
    const RATE: f32 = 48_000.0;
    let bar = 4.0 * 60.0 / TEMPO;
    let phrases: [(&str, &[u8]); 4] = [
        ("Cuando la ciudad se apaga", &[69, 69, 67, 65, 64, 65, 67, 65, 62]),
        ("seguimos bailando", &[65, 67, 69, 69, 67, 69]),
        ("Somos luz en la oscuridad", &[69, 72, 69, 67, 65, 67, 65, 64, 62]),
        ("No mires atrás", &[62, 64, 65, 67, 69, 69]),
    ];
    let seconds = bar * 8.0;
    let n = (seconds * RATE as f64) as usize;
    let mut out = vec![0.0f32; n];
    let mut lines = Vec::new();
    let mut seed = 0x2545_F491u32;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    };
    for (k, (text, notes)) in phrases.iter().enumerate() {
        let start = k as f64 * bar * 2.0 + 0.25 * bar;
        // Syllables: each vowel group, with the letters before it.
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
        let length = bar * 1.6;
        let step = length / syllables.len() as f64;
        lines.push((start, start + length - step * 0.2, *text));
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
    crate::export::write(path, &out, &out, RATE as u32, crate::export::BitDepth::Int24, 1.0).unwrap();
    (seconds, lines)
}

fn hz(pitch: f32) -> f32 {
    440.0 * 2f32.powf((pitch - 69.0) / 12.0)
}

/// The song: its tracks, scenes and mix, before its arrangement.
fn project(voice: &Path) -> (Project, Vec<(f64, f64, &'static str)>, f64) {
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
    let (mut kick, pattern) = kit_track("Bombo y charles", "Techno", color(1));
    kick.depth = 1;
    kick.clips[INTRO] = Some(drum_clip("Bombo y charles", color(1), &pattern, &[36, 42]));
    kick.clips[DROP] = Some(drum_clip("Bombo y palmas", color(1), &pattern, &[36, 39, 42, 46]));
    let (mut perc, perc_pattern) = kit_track("Percusión", "Minimal", color(2));
    perc.depth = 1;
    perc.mixer.gain = 0.6;
    perc.sends[1] = 0.12;
    let percussion: Vec<u8> = perc_pattern.iter().map(|n| n.1).filter(|p| ![36, 38, 39, 40].contains(p)).collect();
    perc.clips[DROP] = Some(drum_clip("Percusión", color(2), &perc_pattern, &percussion));

    // Chords, and what «Acompañar» writes from them.
    let chord_color = color(6);
    let chords = chord_notes();
    let found: Vec<_> = crate::harmony::chords(&chords, BARS, Some(KEY), sig).into_iter().filter_map(|(a, b, c)| c.map(|c| (a, b, c))).collect();
    assert_eq!(found.len(), 4, "{found:?}");
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
    let (seconds, lines) = sing(voice);
    let mut vocal = Track::new_audio("Voz", color(0));
    vocal.mixer.gain = 0.5;
    vocal.sends[..2].copy_from_slice(&[0.3, 0.22]);
    vocal.devices.push(Device::new(Effect::Eq3(engine::model::Eq3Params { low_db: -6.0, high_db: 2.0, low_freq: 150.0, ..Default::default() })));
    vocal.devices.push(Device::new(Effect::Compressor(engine::model::CompressorParams { threshold_db: -20.0, ratio: 3.0, attack_ms: 5.0, release_ms: 80.0, makeup_db: 4.0, sidechain: None })));
    let beats = seconds * TEMPO / 60.0;
    vocal.clips[BREAK] = Some(Clip::Audio(AudioClip {
        hard_start: false,
        fades: Default::default(),
        name: "Voz principal".into(),
        color: color(0),
        sample: voice.to_string_lossy().into_owned(),
        length: beats,
        offset: 0.0,
        warp: WarpMode::Off,
        original_bpm: TEMPO,
        warp_markers: Vec::new(),
        key: None,
        takes: Vec::new(),
        gain_db: 0.0,
        transpose: 0.0,
        envelopes: Vec::new(),
        muted: false,
    }));

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
    (p, lines, beats)
}

/// The arrangement from the scenes, the voice on the breaks, risers and
/// impacts into the drops, and the pad's filter opening on the builds.
fn arrange(app: &mut PulseApp, voice_beats: f64) {
    let sections = crate::arrange::plan(&app.project.tracks, &app.project.scene_names, crate::arrange::Template::Electronic);
    app.lay_out(&sections);
    let markers = app.project.markers.clone();
    let at = |name: &str| markers.iter().filter(|m| m.name == name).map(|m| m.beat).collect::<Vec<_>>();
    let vocal = app.project.tracks.iter().position(|t| t.name == "Voz").unwrap();
    let clip = app.project.tracks[vocal].clips[BREAK].clone().unwrap();
    app.project.tracks[vocal].arrangement = at("Break").into_iter().chain(at("Drop").into_iter().skip(1)).map(|start| ArrangementClip { start, length: voice_beats, clip: clip.clone() }).collect();
    for drop in at("Drop") {
        app.add_transition(crate::transitions::Kind::Riser, 8, drop);
        app.add_transition(crate::transitions::Kind::Impact, 1, drop);
    }
    let pad = app.project.tracks.iter().position(|t| t.name == "Pad").unwrap();
    let mut lane = AutomationLane::new(Param::Device { device: 0, param: 0 });
    let point = |beat: f64, value: f32, curve: f32| AutomationPoint { beat, value, curve };
    for (name, from, to) in [("Subida", 500.0, 12_000.0), ("Break", 900.0, 4_000.0)] {
        for start in at(name) {
            lane.points.push(point(start, from, -0.5));
            lane.points.push(point(start + if name == "Break" { 60.0 } else { 31.0 }, to, 0.0));
        }
    }
    for start in at("Drop") {
        lane.points.push(point(start, 12_000.0, 0.0));
        lane.points.push(point(start + 63.0, 12_000.0, 0.0));
    }
    lane.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    app.project.tracks[pad].automation.push(lane);
    app.sync_all();
}

#[test]
#[ignore]
fn readme_shots() {
    let out = out_dir();
    std::fs::create_dir_all(&out).unwrap();
    let work = std::env::temp_dir().join(format!("pulse-readme-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let (project, _, voice_beats) = project(&work.join("Voz principal.wav"));
    let mut app = PulseApp::headless(project);
    arrange(&mut app, voice_beats);
    let mut s = Studio::new(app, work.clone());
    let save = |s: &mut Studio, name: &str| {
        s.app.status = None;
        let shot = s.shot(name);
        let path = out.join(format!("{name}.png"));
        compact_png(&shot, &path);
        s.note(format!("{} · {} KB", path.display(), std::fs::metadata(&path).unwrap().len() / 1024));
    };

    // 1. The Arrangement: the whole song.
    s.app.view = View::Arrangement;
    s.app.selected_clip = None;
    let pad = s.app.project.tracks.iter().position(|t| t.name == "Pad").unwrap();
    s.app.arrangement.automation.insert(pad, Param::Device { device: 0, param: 0 });
    s.app.arrangement.pixels_per_beat = 2.38;
    s.app.arrangement.lane_h = crate::arrangement::LANE_MIN;
    s.app.selected_target = engine::Target::Track(pad);
    s.app.detail = Detail::Devices;
    // The browser away: the whole song fits.
    s.app.show_browser = false;
    s.frames(10);
    panel_to(&mut s, 650.0);
    s.dump("arrangement");
    save(&mut s, "arrangement");

    // 2. The Session, with the mixer and the returns.
    s.app.show_browser = true;
    s.app.view = View::Session;
    let vocal = s.app.project.tracks.iter().position(|t| t.name == "Voz").unwrap();
    s.app.selected_target = engine::Target::Track(vocal);
    s.frames(10);
    panel_to(&mut s, 600.0);
    s.dump("session");
    save(&mut s, "session");

    // 3. The piano roll: the melody and its brightness.
    let lead = s.app.project.tracks.iter().position(|t| t.name == "Melodía").unwrap();
    s.app.select_clip(ClipRef::Session { track: lead, slot: DROP });
    s.app.detail = Detail::Clip;
    s.app.piano_roll.lane.show_control(MidiControl::Cc(74));
    s.frames(10);
    panel_to(&mut s, 250.0);
    s.app.piano_roll.recenter();
    s.frames(10);
    s.dump("piano-roll");
    save(&mut s, "piano-roll");

    // 4. Devices: the drum bus's chain, the browser away to see all of it.
    s.app.selected_clip = None;
    s.app.selected_target = engine::Target::Track(0);
    s.app.detail = Detail::Devices;
    s.app.show_browser = false;
    s.frames(10);
    panel_to(&mut s, 590.0);
    s.dump("dispositivos");
    save(&mut s, "dispositivos");

    // 5. The voice's editor with its lyrics; the pad's automation folded, so
    // the voice's row shows above the editor.
    s.app.show_browser = true;
    s.app.arrangement.automation.clear();
    s.app.view = View::Arrangement;
    s.app.select_clip(ClipRef::Arrangement { track: vocal, index: 0 });
    s.app.detail = Detail::Clip;
    s.frames(10);
    panel_to(&mut s, 520.0);
    s.dump("letra");
    save(&mut s, "editor-de-audio-con-letra");

    // 6. The mix assistant and its report.
    s.app.selected_clip = None;
    s.app.detail = Detail::Devices;
    s.menu(crate::commands::Command::MixAssistant);
    s.click("Analizar la mezcla");
    s.wait_for("el análisis", 300.0, |app| app.mix.report.is_some());
    s.move_to(pos2(1430.0, 890.0), Modifiers::NONE);
    s.frames(5);
    s.dump("asistente");
    save(&mut s, "asistente-de-mezcla");
    std::fs::remove_dir_all(&work).ok();
}

/// The detail panel's top (above its tabs) dragged to `y`.
fn panel_to(s: &mut Studio, y: f32) {
    let tabs = s.seen.iter().filter(|w| w.text == "Dispositivos" && w.role == "Button").map(|w| w.rect).next().expect("las pestañas del panel");
    s.drag(pos2(900.0, tabs.top() - 6.0), pos2(900.0, y), Modifiers::NONE);
    s.move_to(pos2(1435.0, 895.0), Modifiers::NONE);
    s.frames(10);
}

/// The PNG the canvas wrote (unfiltered, fast) again, smaller: each row with
/// the filter that leaves the least to compress, at the best compression.
fn compact_png(from: &Path, to: &Path) {
    let bytes = std::fs::read(from).unwrap();
    let mut at = 8;
    let (mut width, mut height, mut data) = (0usize, 0usize, Vec::new());
    while at + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind = &bytes[at + 4..at + 8];
        let body = &bytes[at + 8..at + 8 + len];
        match kind {
            b"IHDR" => {
                width = u32::from_be_bytes(body[0..4].try_into().unwrap()) as usize;
                height = u32::from_be_bytes(body[4..8].try_into().unwrap()) as usize;
                assert_eq!(&body[8..10], &[8, 2], "RGB, 8 bits");
            }
            b"IDAT" => data.extend_from_slice(body),
            _ => {}
        }
        at += 12 + len;
    }
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(&data[..]).read_to_end(&mut raw).unwrap();
    let stride = width * 3;
    let rows: Vec<&[u8]> = (0..height).map(|y| {
        let row = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        assert_eq!(row[0], 0, "unfiltered rows");
        &row[1..]
    }).collect();
    let zero = vec![0u8; stride];
    let mut filtered = Vec::with_capacity(raw.len());
    for y in 0..height {
        let (row, up) = (rows[y], if y > 0 { rows[y - 1] } else { &zero[..] });
        let mut best: Option<(u64, u8, Vec<u8>)> = None;
        for filter in 0..5u8 {
            let line: Vec<u8> = (0..stride).map(|i| {
                let a = if i >= 3 { row[i - 3] } else { 0 };
                let b = up[i];
                let c = if i >= 3 { up[i - 3] } else { 0 };
                let predicted = match filter {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((a as u16 + b as u16) / 2) as u8,
                    _ => {
                        let p = a as i16 + b as i16 - c as i16;
                        let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
                        if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
                    }
                };
                row[i].wrapping_sub(predicted)
            }).collect();
            let cost = line.iter().map(|&v| (v as i8).unsigned_abs() as u64).sum();
            if best.as_ref().is_none_or(|b| cost < b.0) {
                best = Some((cost, filter, line));
            }
        }
        let (_, filter, line) = best.unwrap();
        filtered.push(filter);
        filtered.extend_from_slice(&line);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&filtered).unwrap();
    let idat = z.finish().unwrap();
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    for (kind, body) in [(&b"IHDR"[..], header), (b"IDAT", idat), (b"IEND", Vec::new())] {
        png.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut chunk = kind.to_vec();
        chunk.extend_from_slice(&body);
        png.extend_from_slice(&chunk);
        png.extend_from_slice(&crc32(&chunk).to_be_bytes());
    }
    std::fs::write(to, png).unwrap();
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}
