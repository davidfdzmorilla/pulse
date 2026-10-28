//! What tripped up the producer of the audit (see `produccion`), done again the
//! same way, through the interface without a window: submenus that swapped on
//! the way to them, a progression that didn't fit, an editor looking at empty
//! rows, one-bar clips, duplicated tracks, a window taller than the screen.
//! With `PULSE_CAPTURAS=dir` each step also leaves a PNG there.

use crate::commands::Command;
use crate::produccion::Studio;
use crate::project::Project;
use eframe::egui::{pos2, vec2, Key, Modifiers, Rect};
use engine::model::Clip;
use std::path::PathBuf;

fn captures() -> Option<PathBuf> {
    std::env::var_os("PULSE_CAPTURAS").map(PathBuf::from)
}

/// The «Beat» template in A minor, in the Session, its captures named `name`-….
fn beat(name: &str) -> Studio {
    let out = captures().map_or_else(std::env::temp_dir, |d| d.join(name));
    if captures().is_some() {
        std::fs::create_dir_all(&out).unwrap();
    }
    let mut app = crate::app::PulseApp::headless(Project::empty());
    app.templates = crate::templates::all();
    let mut s = Studio::new(app, out);
    let beat = s.app.templates.iter().position(|t| t.name == "Beat").unwrap();
    s.menu(Command::NewFromTemplate(beat));
    s.app.project.key = Some(stems::key::Key { tonic: 9, minor: true });
    s.frames(3);
    s
}

/// The editor shows the notes' rows: all of them if they fit, else the one in
/// the middle of the notes (their median pitch).
fn centred(s: &Studio, notes: &[engine::model::Note]) -> bool {
    let view = s.app.piano_roll.in_view();
    let mut pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
    pitches.sort_unstable();
    let fit = pitches.iter().all(|p| view.contains(p));
    fit || view.contains(&pitches[pitches.len() / 2])
}

fn shot(s: &mut Studio, name: &str) {
    if captures().is_some() {
        s.shot(name);
    } else {
        s.frames(2);
    }
}

fn track(s: &Studio, name: &str) -> usize {
    s.app.project.tracks.iter().position(|t| t.name == name).unwrap_or_else(|| panic!("no hay pista «{name}»"))
}

fn midi(s: &Studio, track: usize, slot: usize) -> engine::model::MidiClip {
    match &s.app.project.tracks[track].clips[slot] {
        Some(Clip::Midi(m)) => m.clone(),
        other => panic!("no es un clip MIDI: {other:?}"),
    }
}

/// The editor's «Acordes» menu button (the track «Acordes» has one in its header too).
fn chord_button(s: &Studio) -> Rect {
    s.seen.iter().filter(|w| w.text == "Acordes" && w.role == "Button" && w.rect.top() > 250.0).map(|w| w.rect).last().expect("el botón Acordes del editor")
}

/// A new clip of one bar (as they were) and «Épica»: the clip grows to four bars
/// for its four chords, is named after them, and ⌘Z takes chords, length and the
/// insert marker back; ⇧⌘Z brings them again.
#[test]
fn a_progression_fits_and_undoes_with_its_marker() {
    let mut s = beat("progresion");
    s.app.settings.new_clip_bars = 1;
    let chords = track(&s, "Acordes");
    let slot = s.find("Slot 3:1").unwrap();
    s.dclick_at(slot.center());
    let button = chord_button(&s);
    s.click_at(button.center());
    shot(&mut s, "menu-acordes");
    s.click("Épica (i–VI–III–VII)");
    s.key(Key::Escape, Modifiers::NONE);
    shot(&mut s, "progresion-entera");
    let clip = midi(&s, chords, 0);
    assert_eq!(clip.length, 16.0, "four bars for four chords");
    assert_eq!(clip.notes.len(), 16);
    assert_eq!(clip.name, "Lam Fa Do Sol");
    assert_eq!(s.app.piano_roll.insert_at(), 16.0);
    // The view is where the chords are: the middle of them, as the editor is
    // only a few rows tall at 1440×900.
    assert!(centred(&s, &clip.notes), "{:?}", s.app.piano_roll.in_view());
    s.key(Key::Z, Modifiers::COMMAND);
    let clip = midi(&s, chords, 0);
    assert!(clip.notes.is_empty() && clip.length == 4.0, "{clip:?}");
    assert_eq!(s.app.piano_roll.insert_at(), 0.0, "the marker goes back with the chords");
    shot(&mut s, "deshecho");
    s.key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(midi(&s, chords, 0).notes.len(), 16);
    assert_eq!(s.app.piano_roll.insert_at(), 16.0);
}

/// «Generar ritmo» in a new clip: the editor shows the rows that play (the kick
/// and the hats), not the empty ones on top (clave, conga); the clip is named after the style.
#[test]
fn a_generated_beat_is_in_view() {
    let mut s = beat("ritmo");
    let drums = track(&s, "Batería");
    let slot = s.find("Slot 1:1").unwrap();
    s.dclick_at(slot.center());
    assert_eq!(midi(&s, drums, 0).length, 16.0, "a new clip is four bars long");
    s.app.piano_roll.drums.ai = false;
    s.click("Generar ritmo");
    s.click("Techno");
    s.click("Generar");
    s.key(Key::Escape, Modifiers::NONE);
    shot(&mut s, "ritmo-generado");
    let clip = midi(&s, drums, 0);
    assert!(clip.notes.len() > 20);
    assert_eq!(clip.name, "Techno");
    let view = s.app.piano_roll.in_view().to_vec();
    assert!(view.contains(&engine::model::drum_note::KICK) && view.contains(&engine::model::drum_note::CLOSED_HAT), "{view:?}");
    // The rows are named in Spanish.
    assert!(s.has("Bombo") && s.has("Charles"));
}

/// Down from «Acompañar ⏵» straight to «Bajo: contratiempo», crossing «De
/// tarareo a canción ⏵»: the bass is written (on the template's empty «Bajo»),
/// not a whole song. Then a pad, on a new track «Pad», and the editor on its notes.
#[test]
fn accompanying_from_the_menus_uses_the_empty_track() {
    let mut s = beat("acompanar");
    let (chords, bass) = (track(&s, "Acordes"), track(&s, "Bajo"));
    let slot = s.find("Slot 3:1").unwrap();
    {
        let key = s.app.project.key.unwrap();
        let progression: Vec<_> = stems::chord::progressions(key)[0].degrees.iter().map(|&d| stems::chord::on_degree(key, d, false, false)).collect();
        let mut clip = engine::model::MidiClip::new("Acordes", 16.0, s.app.project.tracks[chords].color);
        crate::harmony::write(&mut clip, &mut 0.0, 4.0, &progression, Some(key), 4.0);
        s.app.set_clip(chords, 0, Some(Clip::Midi(clip)));
    }
    s.frames(3);
    let tracks = s.app.project.tracks.len();
    s.rclick_at(slot.center());
    s.click("Acompañar ⏵");
    shot(&mut s, "menu-acompanar");
    // Diagonally, as one does, without going across first.
    s.click("Bajo: contratiempo");
    shot(&mut s, "bajo");
    assert_eq!(s.app.project.tracks.len(), tracks, "no new tracks: {:?}", s.app.project.tracks.iter().map(|t| &t.name).collect::<Vec<_>>());
    assert_eq!(midi(&s, bass, 0).name, "Bajo: contratiempo");
    s.rclick_at(slot.center());
    s.click("Acompañar ⏵");
    s.submenu("Pad");
    shot(&mut s, "pad");
    assert_eq!(s.app.project.tracks[chords + 1].name, "Pad");
    let pad = midi(&s, chords + 1, 0);
    assert!(centred(&s, &pad.notes), "the pad's notes in view: {:?}", s.app.piano_roll.in_view());
}

/// The Ducker with a sidechain: its period and its «Release» knob have different names.
#[test]
fn the_ducker_names_its_two_times_apart() {
    let mut s = beat("ducker");
    let bass = track(&s, "Bajo");
    let kick = Some(s.app.project.tracks[0].id);
    s.app.project.tracks[bass].devices.push(engine::model::Device::new(engine::model::Effect::Ducker(engine::model::DuckerParams { sidechain: kick, ..Default::default() })));
    s.app.sync_all();
    s.app.selected_target = engine::Target::Track(bass);
    s.app.detail = crate::app::Detail::Devices;
    s.frames(5);
    shot(&mut s, "ducker");
    assert!(s.has("Vuelve en"), "the period has its own name");
    let releases: Vec<_> = s.seen.iter().filter(|w| w.text == "Release" && w.role == "Texto").map(|w| w.rect).collect();
    assert_eq!(releases.len(), 1, "one «Release»: the knob {releases:?}");
}

/// The mix assistant's report fits a 1440×900 screen: the title and the last
/// section can be reached (it scrolls), and in the narrow window too.
#[test]
fn the_mix_assistant_fits_the_screen() {
    for (size, name) in [(vec2(1440.0, 900.0), "asistente"), (vec2(1000.0, 720.0), "asistente-estrecho")] {
        let mut s = beat(name);
        s.size = size;
        s.app.project = Project::demo();
        s.app.sync_all();
        s.menu(Command::MixAssistant);
        s.click("Analizar la mezcla");
        s.wait_for("el análisis", 120.0, |app| app.mix.report.is_some());
        s.frames(10);
        // The track list open, at the bottom: the report grows past the screen.
        let tracks = s.find("Pistas").expect("the «Pistas» section");
        if tracks.bottom() < size.y {
            s.click_at(tracks.center());
        }
        s.frames(10);
        shot(&mut s, "analisis");
        let title = s.find("Asistente de mezcla y master").expect("the title");
        assert!(title.top() >= 0.0, "{title:?}");
        let window = s.ctx().memory(|m| m.area_rect(eframe::egui::Id::new("mix assistant"))).expect("the window");
        assert!(window.top() >= 0.0 && window.bottom() <= size.y + 0.5, "{window:?} on {size:?}");
    }
}

/// The screens the audit showed, after: the editor and menus in a narrow window.
#[test]
fn narrow_window_shots() {
    let Some(_) = captures() else { return };
    let mut s = beat("estrecha");
    s.size = vec2(1000.0, 720.0);
    s.frames(5);
    let slot = s.find("Slot 3:1").unwrap();
    s.dclick_at(slot.center());
    let button = chord_button(&s);
    s.click_at(button.center());
    s.click("Épica (i–VI–III–VII)");
    s.key(Key::Escape, Modifiers::NONE);
    shot(&mut s, "progresion");
    s.rclick_at(slot.center());
    s.click("Acompañar ⏵");
    s.move_to(pos2(s.find("Bajo: contratiempo").unwrap().center().x, s.find("Bajo: contratiempo").unwrap().center().y), Modifiers::NONE);
    shot(&mut s, "menu-acompanar");
    s.key(Key::Escape, Modifiers::NONE);
    s.app.show_settings = true;
    s.frames(3);
    shot(&mut s, "ajustes");
}
