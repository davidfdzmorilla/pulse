//! Tuning a recorded voice note by note in the app: the clip plays the tuned
//! file, undo and redo go between files that are there, copies move
//! together, and what nothing uses goes when the project is saved.

use crate::app::{ClipRef, PulseApp};
use crate::project::{Project, Track};
use crate::tuning::TunedNote;
use engine::model::*;
use std::path::PathBuf;

const SR: f32 = 48_000.0;

fn dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media").join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A voice singing an A3 30 cents sharp for 0.7 s, then a D4: a vowel's
/// harmonics.
fn voice(dir: &std::path::Path) -> String {
    let path = dir.join("voz.wav");
    let mut phase = 0f32;
    let l: Vec<f32> = (0..(1.4 * SR) as usize)
        .map(|i| {
            let t = i as f32 / SR;
            let hz = if t < 0.7 { 220.0 * 2f32.powf(0.3 / 12.0) } else { 293.66 };
            phase = (phase + hz / SR).fract();
            0.2 * (1..8).map(|k| (std::f32::consts::TAU * phase * k as f32).sin() / (1.0 + ((k as f32 * hz - 700.0) / 200.0).powi(2))).sum::<f32>()
        })
        .collect();
    crate::export::write(&path, &l, &l, SR as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
    path.to_string_lossy().into_owned()
}

fn clip(sample: &str) -> AudioClip {
    AudioClip { hard_start: false, fades: Fades::new(0.0, 0.0), name: "voz".into(), color: [200, 80, 80], sample: sample.into(), length: 2.8, offset: 0.0, warp: WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false }
}

/// An app with a voice track: the clip, a copy of it later on, and another
/// track's clip of the same file.
fn app(name: &str) -> (PulseApp, String) {
    let dir = dir(name);
    let sample = voice(&dir);
    let mut p = Project::empty();
    let mut t = Track::new_audio("Voz", [0; 3]);
    t.id = 1;
    t.arrangement = vec![ArrangementClip { start: 0.0, length: 2.8, clip: Clip::Audio(clip(&sample)) }];
    let mut other = Track::new_audio("Doble", [0; 3]);
    other.id = 2;
    other.arrangement = vec![ArrangementClip { start: 0.0, length: 2.8, clip: Clip::Audio(clip(&sample)) }];
    p.tracks = vec![t, other];
    let mut app = PulseApp::headless(p);
    app.file = Some(dir.join("voz.pulse"));
    (app, sample)
}

const VOICE: ClipRef = ClipRef::Arrangement { track: 0, index: 0 };

fn playing(app: &PulseApp, track: usize, index: usize) -> String {
    match &app.project.tracks[track].arrangement[index].clip {
        Clip::Audio(a) => a.sample.clone(),
        _ => unreachable!(),
    }
}

/// The pitch (fractional MIDI) the file `path` sings between `from` and `to` s.
fn sung(app: &mut PulseApp, path: &str, from: f32, to: f32) -> f32 {
    let sample = app.sample(path);
    let window = (0.04 * SR) as usize;
    let mut v = Vec::new();
    let mut at = (from * SR) as usize;
    while at + window <= (to * SR) as usize {
        if let Some((hz, _)) = stems::pitch::live(&sample.left[at..at + window], SR, 60.0, 1000.0) {
            v.push((69.0 + 12.0 * (hz / 440.0).log2()) as f32);
        }
        at += 480;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

fn open(app: &mut PulseApp, source: &str) -> Vec<TunedNote> {
    app.open_tuning(VOICE);
    app.wait_tuning_analysis(source);
    app.tuning_notes(VOICE).unwrap()
}

#[test]
fn a_note_tuned_plays_tuned_and_undo_brings_the_voice_back() {
    let (mut app, source) = app("tuning-undo");
    app.commit_history();
    let notes = open(&mut app, &source);
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!((notes[0].pitch - 57.3).abs() < 0.05 && (notes[1].pitch - 62.0).abs() < 0.05, "{notes:?}");
    // The first note tuned: the clip plays a tuned file, in tune; the other
    // track (the same file, untuned) still sings it as it was.
    let mut tuned = notes.clone();
    tuned[0].target = 57.0;
    app.apply_tuning(VOICE, tuned).unwrap();
    app.commit_history();
    let file = playing(&app, 0, 0);
    assert_ne!(file, source);
    assert!(file.contains("Afinados") && file.contains("(afinado 1-"), "{file}");
    assert_eq!(playing(&app, 1, 0), source, "the other track's clip is not tuned");
    let (before, after) = (sung(&mut app, &source, 0.15, 0.55), sung(&mut app, &file, 0.15, 0.55));
    println!("{before:.2} → {after:.2}");
    assert!((after - 57.0).abs() < 0.04 && (before - 57.3).abs() < 0.04);
    assert!((sung(&mut app, &file, 0.85, 1.3) - 62.0).abs() < 0.04, "the second note as it was");
    assert_eq!(app.project.tunings.len(), 1);
    // Undo: the voice as recorded; redo: tuned again, the file still there.
    app.finish_tuning_writes();
    app.undo();
    assert_eq!(playing(&app, 0, 0), source);
    assert!(app.project.tunings.is_empty());
    app.redo();
    assert_eq!(playing(&app, 0, 0), file);
    assert!(std::path::Path::new(&file).exists());
    // The editor knows the notes as tuned.
    assert_eq!(app.tuning_notes(VOICE).unwrap()[0].target, 57.0);
}

#[test]
fn copies_move_together_and_as_sung_is_the_file_again() {
    let (mut app, source) = app("tuning-copies");
    let notes = open(&mut app, &source);
    let mut tuned = notes.clone();
    tuned[1].target = 63.0;
    app.apply_tuning(VOICE, tuned.clone()).unwrap();
    let first = playing(&app, 0, 0);
    // A copy of the tuned clip further on.
    let copy = app.project.tracks[0].arrangement[0].clone();
    app.project.tracks[0].arrangement.push(ArrangementClip { start: 4.0, ..copy });
    // Edited again: both play the new file.
    tuned[0].modulation = 0.0;
    app.apply_tuning(VOICE, tuned.clone()).unwrap();
    let second = playing(&app, 0, 0);
    assert_ne!(second, first);
    assert_eq!(playing(&app, 0, 1), second, "the copy moves with it");
    assert_eq!(app.project.tunings.len(), 1);
    // The same edits again: the same file, not rendered again.
    app.apply_tuning(VOICE, tuned).unwrap();
    assert_eq!(playing(&app, 0, 0), second);
    // Every note as sung: the file as recorded, and no tuning left.
    app.apply_tuning(VOICE, notes).unwrap();
    assert_eq!(playing(&app, 0, 0), source);
    assert_eq!(playing(&app, 0, 1), source);
    assert!(app.project.tunings.is_empty());
}

/// Saved, the tunings and files go with the project (the paths relative to
/// its folder) and the files no state uses any more are deleted; opened
/// again, the notes are as they were.
#[test]
fn saving_keeps_what_is_used_and_deletes_the_rest() {
    let (mut app, source) = app("tuning-save");
    let notes = open(&mut app, &source);
    let mut tuned = notes.clone();
    tuned[0].target = 56.0;
    app.apply_tuning(VOICE, tuned.clone()).unwrap();
    let old = playing(&app, 0, 0);
    tuned[0].target = 58.0;
    app.apply_tuning(VOICE, tuned).unwrap();
    let new = playing(&app, 0, 0);
    // No step to go back to the first edit.
    app.history.reset(&app.project);
    let path = app.file.clone().unwrap();
    app.save_to_path(&path);
    assert!(!std::path::Path::new(&old).exists(), "the first edit's file is gone");
    assert!(std::path::Path::new(&new).exists());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"tunings\"") && text.contains("Samples/Afinados/") && !text.contains(&*path.parent().unwrap().to_string_lossy()), "relative paths");
    let again = Project::load(&path).unwrap();
    assert_eq!(again.tunings.len(), 1);
    assert_eq!(again.tunings[0].notes[0].target, 58.0);
    assert_eq!(again.tunings[0].output, new);
}

/// The editor draws, analysing and with its notes, without a window.
#[test]
fn the_editor_draws() {
    let (mut app, source) = app("tuning-ui");
    app.selected_clip = Some(VOICE);
    let ctx = eframe::egui::Context::default();
    let input = || eframe::egui::RawInput { screen_rect: Some(eframe::egui::Rect::from_min_size(eframe::egui::Pos2::ZERO, eframe::egui::vec2(1000.0, 320.0))), ..Default::default() };
    app.open_tuning(VOICE);
    let _ = ctx.run_ui(input(), |ui| crate::tuning_ui::show(&mut app, ui, VOICE, Some(1.0)));
    app.wait_tuning_analysis(&source);
    app.tuner.selected = [0].into();
    let _ = ctx.run_ui(input(), |ui| crate::tuning_ui::show(&mut app, ui, VOICE, Some(1.0)));
    let _ = ctx.run_ui(input(), |ui| crate::piano_roll::show(&mut app, ui));
    assert_eq!(app.tuner.open, Some(VOICE));
}

/// With the mouse: a note dragged a little down goes to the semitone below
/// where it was sung (the A sung sharp, in tune), rendered when let go.
#[test]
fn a_note_dragged_goes_to_its_semitone() {
    use eframe::egui::{self, pos2, vec2, Event, Modifiers, PointerButton};
    let (mut app, source) = app("tuning-drag");
    app.selected_clip = Some(VOICE);
    app.open_tuning(VOICE);
    app.wait_tuning_analysis(&source);
    let ctx = egui::Context::default();
    let mut time = 0.0;
    let mut frame = |app: &mut PulseApp, events: Vec<Event>| {
        time += 0.05;
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 320.0))), time: Some(time), events, ..Default::default() };
        let output = ctx.run_ui(input, |ui| crate::tuning_ui::show(app, ui, VOICE, None));
        let mut texts = Vec::new();
        fn collect(shape: &egui::Shape, texts: &mut Vec<(String, egui::Pos2)>) {
            match shape {
                egui::Shape::Text(t) => texts.push((t.galley.text().to_string(), t.pos)),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
                _ => {}
            }
        }
        output.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
        texts
    };
    let texts = frame(&mut app, vec![]);
    let (label, at) = texts.iter().find(|(t, _)| t.contains("+30 cents")).cloned().unwrap_or_else(|| panic!("the sharp note's label: {texts:?}"));
    println!("{label} at {at:?}");
    let grab = at + vec2(10.0, 4.0);
    let press = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    frame(&mut app, vec![Event::PointerMoved(grab)]);
    frame(&mut app, vec![press(grab, true)]);
    for k in 1..=4 {
        frame(&mut app, vec![Event::PointerMoved(grab + vec2(0.0, 2.0 * k as f32))]);
    }
    frame(&mut app, vec![press(grab + vec2(0.0, 8.0), false)]);
    frame(&mut app, vec![]);
    assert_eq!(app.tuning_notes(VOICE).unwrap()[0].target, 57.0);
    assert_ne!(playing(&app, 0, 0), source, "rendered when let go");
}
