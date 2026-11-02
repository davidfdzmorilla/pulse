//! The clip editor uses the screen: at 1440×900 and 1000×720, a clip opened in
//! the «Beat» template shows two octaves and the Session stays in sight; the
//! panel can be maximized and back (button, ⇧⌘E, double click on its edge);
//! the height chosen is remembered as a share of the window; the editor's bar
//! takes two rows at most and every control in it can still be reached; ⌥ +
//! wheel makes the rows taller or shorter. `PULSE_SHOTS=dir` leaves PNGs.

use crate::app::ClipRef;
use crate::commands::Command;
use crate::produccion::Studio;
use crate::project::Project;
use eframe::egui::{self, pos2, vec2, Event, Key, Modifiers};
use engine::model::{Clip, MidiClip, Note};
use std::path::PathBuf;

const SIZES: [(f32, f32); 2] = [(1440.0, 900.0), (1000.0, 720.0)];

fn shots() -> Option<PathBuf> {
    std::env::var_os("PULSE_SHOTS").map(PathBuf::from)
}

/// The «Beat» template in A minor at `w`×`h`, a bass clip open in the editor.
fn beat(w: f32, h: f32) -> Studio {
    let mut app = crate::app::PulseApp::headless(Project::empty());
    app.templates = crate::templates::all();
    beat_with(app, w, h)
}

fn beat_with(app: crate::app::PulseApp, w: f32, h: f32) -> Studio {
    let mut s = Studio::new(app, std::env::temp_dir());
    s.size = vec2(w, h);
    let beat = s.app.templates.iter().position(|t| t.name == "Beat").unwrap();
    s.menu(Command::NewFromTemplate(beat));
    s.app.project.key = Some(stems::key::Key { tonic: 9, minor: true });
    let bass = bass(&s);
    let mut clip = MidiClip::new("Bajo", 16.0, s.app.project.tracks[bass].color);
    for (i, pitch) in [45, 48, 52, 57].into_iter().enumerate() {
        clip.notes.push(Note::new(i as f64, 0.5, pitch, 0.8));
    }
    s.app.set_clip(bass, 0, Some(Clip::Midi(clip)));
    s.app.select_clip(ClipRef::Session { track: bass, slot: 0 });
    s.frames(8);
    s
}

fn bass(s: &Studio) -> usize {
    s.app.project.tracks.iter().position(|t| t.name == "Bajo").unwrap()
}

fn shot(s: &mut Studio, name: &str) {
    if let Some(dir) = shots() {
        std::fs::create_dir_all(&dir).unwrap();
        s.out = dir;
        s.shot(name);
    }
}

/// The top of the detail panel (its «Clip» tab).
fn panel_top(s: &Studio) -> f32 {
    s.seen.iter().filter(|w| w.text == "Clip" && w.role != "Texto").map(|w| w.rect.top()).fold(f32::INFINITY, f32::min)
}

/// The panel's edge, a little above its tabs.
fn edge(s: &Studio) -> egui::Pos2 {
    pos2(s.size.x * 0.6, panel_top(s) - 7.0)
}

/// Where a widget (not a painted text) showing exactly `text` is in the
/// editor, under the panel's top.
fn control(s: &Studio, text: &str) -> Option<egui::Rect> {
    let top = panel_top(s);
    s.seen.iter().filter(|w| w.role != "Texto" && w.text == text && w.rect.top() > top - 4.0).map(|w| w.rect).next()
}

/// Two octaves of notes at both sizes, and the Session's clips still in sight above.
#[test]
fn a_clip_opened_shows_two_octaves() {
    for (w, h) in SIZES {
        let mut s = beat(w, h);
        let rows = s.app.piano_roll.in_view().len();
        assert!(rows >= crate::piano_roll::ROWS_WANTED, "{w}×{h}: {rows} filas");
        // The Session's track names and the clip's slot are above the panel.
        let slot = s.seen.iter().filter(|x| x.text == "Bajo").map(|x| x.rect).min_by(|a, b| a.top().total_cmp(&b.top())).unwrap();
        assert!(slot.bottom() < panel_top(&s) - 40.0, "{w}×{h}: the Session is covered");
        // Opened again, nothing moves.
        let top = panel_top(&s);
        s.app.selected_clip = None;
        s.frames(4);
        assert!((panel_top(&s) - top).abs() < 2.0, "{w}×{h}: the panel stays when the clip closes");
    }
}

/// The button, ⇧⌘E and a double click on the edge maximize the panel and
/// bring it back where it was.
#[test]
fn the_editor_maximizes_and_comes_back() {
    let mut s = beat(1440.0, 900.0);
    let top = panel_top(&s);
    let rows = s.app.piano_roll.in_view().len();
    s.click_at(control(&s, "Maximizar").unwrap().center());
    s.frames(4);
    assert!(s.app.editor_max);
    let maxed = panel_top(&s);
    assert!(maxed < 120.0, "almost the whole window: {maxed}");
    assert!(s.app.piano_roll.in_view().len() > rows + 15, "{rows} → {} rows", s.app.piano_roll.in_view().len());
    s.click_at(control(&s, "Restaurar").unwrap().center());
    s.frames(4);
    assert!(!s.app.editor_max);
    assert!((panel_top(&s) - top).abs() < 2.0, "{top} → {}", panel_top(&s));
    // ⇧⌘E, there and back.
    s.key(Key::E, Modifiers::COMMAND | Modifiers::SHIFT);
    s.frames(4);
    assert!(s.app.editor_max && panel_top(&s) < 120.0);
    s.key(Key::E, Modifiers::COMMAND | Modifiers::SHIFT);
    s.frames(4);
    assert!(!s.app.editor_max && (panel_top(&s) - top).abs() < 2.0);
    // A double click on the edge.
    let at = edge(&s);
    s.dclick_at(at);
    s.frames(4);
    assert!(s.app.editor_max, "double click");
    assert!(panel_top(&s) < 120.0);
    let at = edge(&s);
    s.dclick_at(at);
    s.frames(4);
    assert!(!s.app.editor_max && (panel_top(&s) - top).abs() < 2.0);
    assert_eq!(s.app.settings.detail_share, None, "none of this is a height chosen");
}

/// Dragged, the panel's share of the window goes to the settings, and the next
/// time (in another window too) it starts at that share. Maximized and back, it
/// returns to it.
#[test]
fn the_height_chosen_is_remembered() {
    let mut s = beat(1440.0, 900.0);
    let top = panel_top(&s);
    let at = edge(&s);
    s.drag(at, at + vec2(0.0, 150.0), Modifiers::NONE);
    s.frames(4);
    let lowered = panel_top(&s);
    assert!(lowered > top + 130.0, "{top} → {lowered}");
    let share = s.app.settings.detail_share.expect("the height chosen is kept");
    assert!(share > 0.2 && share < 0.5, "{share}");
    s.key(Key::E, Modifiers::COMMAND | Modifiers::SHIFT);
    s.key(Key::E, Modifiers::COMMAND | Modifiers::SHIFT);
    s.frames(4);
    assert!((panel_top(&s) - lowered).abs() < 2.0, "back to the height chosen");
    // The settings round trip (in memory: the tests don't touch the user's).
    let text = serde_json::to_string(&s.app.settings).unwrap();
    let kept: crate::settings::Settings = serde_json::from_str(&text).unwrap();
    assert_eq!(kept.detail_share, Some(share));
    for (w, h) in SIZES {
        let mut app = crate::app::PulseApp::headless(Project::empty());
        app.templates = crate::templates::all();
        app.settings = kept.clone();
        let s = beat_with(app, w, h);
        // The panel's share of what is under the transport.
        let bottom = s.seen.iter().filter(|x| x.text.starts_with("Proyecto nuevo")).map(|x| x.rect.top()).fold(h, f32::min);
        let room_top = s.seen.iter().filter(|x| x.text == "Batería").map(|x| x.rect.top()).fold(h, f32::min);
        let panel = bottom - panel_top(&s);
        let ratio = panel / (bottom - room_top);
        assert!((ratio - share).abs() < 0.08, "{w}×{h}: {ratio} vs {share}");
    }
}

/// What the bar has, in it or in «⋯», for a bass clip and for a melody.
const CONTROLS: [&str; 14] = ["Loop", "Rejilla", "Nota", "Dibujar", "Acordes", "Cuantizar", "finales", "Prob.", "-12", "+12", "Plegar", "A la escala", "×2", "Alto"];

/// Whether `text` shows in the editor (a widget or a label).
fn shows(s: &Studio, text: &str) -> bool {
    let top = panel_top(s);
    s.seen.iter().any(|w| (w.text == text || w.text.starts_with(&format!("{text} "))) && w.rect.top() > top)
}

/// The rows the bar takes: the distinct heights of its controls, over the notes.
fn bar_rows(s: &Studio) -> usize {
    let top = panel_top(s);
    let ruler = s.seen.iter().filter(|w| w.text == "Carril").map(|w| w.rect.top()).fold(f32::INFINITY, f32::min);
    let mut rows: Vec<f32> = s
        .seen
        .iter()
        .filter(|w| w.role != "Texto" && w.rect.top() > top + 26.0 && w.rect.bottom() < ruler && w.rect.height() < 30.0 && !w.text.is_empty())
        .filter(|w| ["Loop", "Rejilla", "Dibujar", "Acordes", "⋯", "Otra variación", "Cuantizar"].iter().any(|t| w.text.starts_with(t)) || w.text.contains("comp."))
        .map(|w| w.rect.center().y.round())
        .collect();
    rows.sort_by(f32::total_cmp);
    rows.dedup_by(|a, b| (*a - *b).abs() < 8.0);
    rows.len()
}

/// At both sizes the bar takes two rows at most (one for a bass clip at
/// 1440), and what isn't in it is in «⋯», where it works.
#[test]
fn every_control_of_the_bar_can_be_reached() {
    for (w, h) in SIZES {
        let mut s = beat(w, h);
        let rows = bar_rows(&s);
        assert!(rows <= 2, "{w}×{h}: {rows} rows");
        if w > 1200.0 {
            assert_eq!(rows, 1, "a bass clip at {w}");
        }
        let hidden: Vec<&str> = CONTROLS.into_iter().filter(|c| !shows(&s, c)).collect();
        if hidden.is_empty() {
            continue;
        }
        assert!(!s.app.piano_roll.hidden.is_empty(), "{w}×{h}: {hidden:?} lost");
        s.click_at(control(&s, "⋯").expect("«⋯»").center());
        s.frames(4);
        let lost: Vec<&str> = hidden.iter().copied().filter(|c| !shows(&s, c)).collect();
        assert!(lost.is_empty(), "{w}×{h}: not in «⋯» either: {lost:?}");
        // They work from the menu, and it stays open.
        let bass = bass(&s);
        let first = |s: &Studio| match &s.app.project.tracks[bass].clips[0] {
            Some(Clip::Midi(m)) => m.notes[0].pitch,
            _ => unreachable!(),
        };
        if hidden.contains(&"+12") {
            let before = first(&s);
            s.click_at(control(&s, "+12").unwrap().center());
            assert_eq!(first(&s), before + 12, "{w}×{h}");
            assert!(shows(&s, "+12"), "«⋯» stays open");
        }
        if hidden.contains(&"Plegar") {
            s.click_at(control(&s, "Plegar").unwrap().center());
            s.frames(2);
            assert!(s.app.piano_roll.in_view().iter().all(|p| [0, 2, 4, 5, 7, 9, 11].contains(&(p % 12))), "folded to A minor");
        }
        s.key(Key::Escape, Modifiers::NONE);
    }
}

/// A melody adds «Otra variación» and its style, density and register: still
/// two rows at most, at both sizes.
#[test]
fn a_melody_bar_takes_two_rows_at_most() {
    for (w, h) in SIZES {
        let mut s = beat(w, h);
        let chords = s.app.project.tracks.iter().position(|t| t.name == "Acordes").unwrap();
        let key = s.app.project.key.unwrap();
        let progression: Vec<_> = stems::chord::progressions(key)[0].degrees.iter().map(|&d| stems::chord::on_degree(key, d, false, false)).collect();
        let mut clip = MidiClip::new("Acordes", 16.0, s.app.project.tracks[chords].color);
        crate::harmony::write(&mut clip, &mut 0.0, 4.0, &progression, Some(key), 4.0);
        s.app.set_clip(chords, 0, Some(Clip::Midi(clip)));
        s.app.select_clip(ClipRef::Session { track: chords, slot: 0 });
        s.frames(3);
        s.app.accompany(ClipRef::Session { track: chords, slot: 0 }, crate::accompany::Part::melodies()[0]);
        s.frames(8);
        assert!(shows(&s, "Otra variación"), "{w}×{h}: the melody's bar");
        let rows = bar_rows(&s);
        assert!(rows <= 2, "{w}×{h}: {rows} rows");
        if w < 1200.0 {
            shot(&mut s, "melodia-estrecha");
        }
    }
}

/// ⌥ + wheel over the notes: taller rows (fewer in sight), then shorter, never
/// under the least; without ⌥ the wheel scrolls.
#[test]
fn alt_and_the_wheel_make_the_rows_taller() {
    let mut s = beat(1440.0, 900.0);
    let rows = s.app.piano_roll.in_view().len();
    let notes = s.app.piano_roll.view().unwrap();
    let over = pos2(s.size.x * 0.6, panel_top(&s) + 140.0);
    let wheel = |s: &mut Studio, dy: f32, mods: Modifiers| {
        s.move_to(over, mods);
        for _ in 0..4 {
            let event = Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, dy / 4.0), phase: egui::TouchPhase::Move, modifiers: mods };
            s.frame_with(vec![event], mods);
        }
        s.frames(20);
    };
    // Without ⌥ the rows stay and the notes scroll.
    let offset = notes.0.y;
    wheel(&mut s, -200.0, Modifiers::NONE);
    assert_eq!(s.app.piano_roll.row_zoom, 1.0);
    assert!(s.app.piano_roll.view().unwrap().0.y > offset, "{offset}");
    wheel(&mut s, 200.0, Modifiers::ALT);
    assert!(s.app.piano_roll.row_zoom > 1.3, "{}", s.app.piano_roll.row_zoom);
    let taller = s.app.piano_roll.in_view().len();
    assert!(taller < rows - 4, "{rows} → {taller}");
    assert_eq!(s.app.piano_roll.view().unwrap().0.x, notes.0.x, "it doesn't scroll sideways");
    wheel(&mut s, -3000.0, Modifiers::ALT);
    assert!((s.app.piano_roll.row_zoom - 0.65).abs() < 1e-3, "the least: {}", s.app.piano_roll.row_zoom);
    assert!(s.app.piano_roll.in_view().len() > rows + 8);
}

/// The pictures: `PULSE_SHOTS=dir cargo test --release -p app editor_space_shots -- --ignored`.
#[test]
#[ignore]
fn editor_space_shots() {
    if shots().is_none() {
        return;
    }
    for (w, h) in SIZES {
        let mut s = beat(w, h);
        eprintln!("{w}×{h}: {} filas a la vista", s.app.piano_roll.in_view().len());
        shot(&mut s, &format!("editor-{w}x{h}"));
        if !s.app.piano_roll.hidden.is_empty() {
            s.click_at(control(&s, "⋯").unwrap().center());
            shot(&mut s, &format!("mas-{w}x{h}"));
            s.key(Key::Escape, Modifiers::NONE);
        }
        s.key(Key::E, Modifiers::COMMAND | Modifiers::SHIFT);
        shot(&mut s, &format!("maximizado-{w}x{h}"));
        s.key(Key::E, Modifiers::COMMAND | Modifiers::SHIFT);
    }
}
