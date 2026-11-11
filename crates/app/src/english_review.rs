//! The screens the README doesn't show, in English, for looking at them: the
//! file and tool windows, the clip editors' menus and rows, the instruments'
//! windows and devices, the welcome and the lessons. Drawn without a window
//! from the example song (see `readme_shots`), with a widget list beside each
//! picture (`widgets-*.txt`) to find what's cut off or still in Spanish.
//! `cargo test --release -p app english_review -- --ignored` writes them to
//! `PULSE_SHOTS` (else the system's temporary folder, `pulse-english-review`).

use crate::app::{ClipRef, Detail, PulseApp, View};
use crate::commands::Command;
use crate::example::{BREAK, DROP};
use crate::i18n::tr;
use crate::produccion::Studio;
use eframe::egui::{pos2, Key, Modifiers};
use engine::model::InstrumentKind;
use std::path::PathBuf;

fn out_dir() -> PathBuf {
    std::env::var_os("PULSE_SHOTS").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("pulse-english-review"))
}

#[test]
#[ignore]
fn english_review() {
    // `PULSE_REVIEW_SPANISH=1`: the same screens in Spanish, to compare.
    let language = if std::env::var_os("PULSE_REVIEW_SPANISH").is_some() { crate::i18n::Language::Spanish } else { crate::i18n::Language::English };
    crate::i18n::with(language, review);
}

fn review() {
    let out = out_dir();
    std::fs::create_dir_all(&out).unwrap();
    let work = std::env::temp_dir().join(format!("pulse-english-review-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let (project, _) = crate::example::tracks(&work.join("Voz principal.wav")).unwrap();
    let mut app = PulseApp::headless(project);
    app.templates = crate::templates::all();
    let mut s = Studio::new(app, out.clone());
    let track = |s: &Studio, name: &'static str| s.app.project.tracks.iter().position(|t| crate::i18n::is_named(&t.name, name)).unwrap();
    // The pointer out of the way (on the status bar), so no hint covers the picture.
    let save = |s: &mut Studio, name: &str| {
        s.app.status = None;
        s.move_to(pos2(1435.0, 895.0), Modifiers::NONE);
        s.frames(3);
        s.dump(name);
        s.shot(name);
    };
    let close = |s: &mut Studio| {
        s.key(Key::Escape, Modifiers::NONE);
        let app = &mut s.app;
        (app.show_settings, app.show_shortcuts, app.show_about, app.guide.open, app.versions.open, app.mix.open, app.arrange.open, app.updates.show, app.export.open) = (false, false, false, false, false, false, false, false, false);
        s.frames(3);
    };

    // The file windows.
    s.menu(Command::Export);
    save(&mut s, "export");
    close(&mut s);
    s.menu(Command::Versions);
    save(&mut s, "versions");
    close(&mut s);
    s.app.updates.show = true;
    save(&mut s, "updates");
    close(&mut s);
    s.menu(Command::About);
    save(&mut s, "about");
    close(&mut s);
    s.menu(Command::Settings);
    save(&mut s, "settings");
    close(&mut s);
    s.menu(Command::Shortcuts);
    save(&mut s, "shortcuts");
    close(&mut s);

    // The tool windows.
    s.app.macro_window.open = true;
    save(&mut s, "macros");
    s.app.macro_window.open = false;
    s.app.toggle_learn();
    save(&mut s, "midi-map");
    s.app.toggle_learn();
    s.app.cpu.open = true;
    save(&mut s, "performance");
    s.app.cpu.open = false;
    s.menu(Command::Arrange);
    save(&mut s, "auto-arrange");
    close(&mut s);

    // The MIDI editor: the chords' clip with «Transformar» open.
    s.app.view = View::Session;
    let chords = track(&s, "Acordes");
    s.app.select_clip(ClipRef::Session { track: chords, slot: DROP });
    s.app.detail = Detail::Clip;
    s.app.editor_max = true;
    s.frames(10);
    s.click(tr("Transformar"));
    save(&mut s, "midi-transform");
    s.click(tr("Rasgueo"));
    save(&mut s, "midi-transform-strum");
    close(&mut s);

    // A drum clip: «Generar ritmo».
    let kick = track(&s, "Bombo y charles");
    s.app.select_clip(ClipRef::Session { track: kick, slot: DROP });
    s.frames(10);
    s.click(tr("Generar ritmo"));
    save(&mut s, "generate-beat");
    close(&mut s);
    s.app.editor_max = false;

    // A clip's menu in the Session, with «Acompañar» open.
    s.app.detail = Detail::Devices;
    s.app.selected_clip = None;
    s.frames(5);
    s.dump("session");
    // The chords' clip in the drop: its column's x and its row's y from the empty slots.
    let (column, row) = (format!("Slot {}:", chords + 1), format!(":{}", DROP + 1));
    let x = s.seen.iter().find(|w| w.text.starts_with(&column)).map(|w| w.rect.center().x).unwrap();
    let y = s.seen.iter().find(|w| w.text.starts_with("Slot ") && w.text.ends_with(&row)).map(|w| w.rect.center().y).unwrap();
    s.rclick_at(pos2(x, y));
    s.frames(3);
    s.dump("clip-menu");
    if s.find(tr("Acompañar")).is_some() {
        s.hover(tr("Acompañar"));
        save(&mut s, "accompany-menu");
    }
    close(&mut s);

    // The voice's audio editor, maximized: its lower rows.
    s.app.view = View::Arrangement;
    let sections = crate::arrange::plan(&s.app.project.tracks, &s.app.project.scene_names, crate::arrange::Template::Electronic);
    s.app.lay_out(&sections);
    s.app.sync_all();
    let vocal = track(&s, "Voz");
    s.app.select_clip(ClipRef::Session { track: vocal, slot: BREAK });
    s.app.detail = Detail::Clip;
    s.app.editor_max = true;
    s.frames(10);
    save(&mut s, "audio-editor");
    s.app.editor_max = false;
    s.frames(5);

    // The chord track's menu.
    save(&mut s, "arrangement");
    // The first chord on the strip under the ruler.
    let strip = s.seen.iter().find(|w| w.text.starts_with(tr("Acordes"))).map(|w| w.rect.center().y).unwrap();
    if let Some(chord) = s.seen.iter().find(|w| w.role == "Texto" && (w.rect.center().y - strip).abs() < 3.0 && w.rect.center().x > 300.0).map(|w| w.rect) {
        s.rclick_at(chord.center());
        save(&mut s, "chord-track-menu");
        close(&mut s);
    }

    // The voice's lyrics and tuning windows, and the transcription's question.
    let voice = ClipRef::Session { track: vocal, slot: BREAK };
    s.app.lyrics_ui.editing = Some(voice);
    save(&mut s, "lyrics");
    s.app.lyrics_ui.editing = None;
    s.app.lyrics_ui.ask = Some(crate::lyrics_ui::Ask { clip: voice, language: Default::default(), isolate: false });
    save(&mut s, "lyrics-transcribe");
    s.app.lyrics_ui.ask = None;
    s.app.open_tuning(voice);
    s.wait_for("the tuning's analysis", 60.0, |app| app.tuner.open.is_some_and(|_| true));
    s.frames(30);
    std::thread::sleep(std::time::Duration::from_secs(2));
    s.frames(10);
    save(&mut s, "tuning");
    s.app.tuner.open = None;
    s.frames(3);

    // The welcome and the lessons.
    s.app.welcome.open = true;
    save(&mut s, "welcome");
    s.app.welcome.open = false;
    s.menu(Command::Lessons);
    save(&mut s, "lessons");
    s.click(tr(crate::lessons::LESSONS[1].title));
    save(&mut s, "lesson");
    s.menu(Command::Lessons);

    // The instruments, each on the first track, its device and its window.
    s.app.view = View::Session;
    s.app.selected_clip = None;
    s.app.detail = Detail::Devices;
    s.app.selected_target = engine::Target::Track(chords);
    use engine::model::{KeysParams, ModularParams, WaveParams};
    let kinds: Vec<(&str, InstrumentKind, fn(&mut PulseApp, usize))> = vec![
        ("wavetable", InstrumentKind::Wavetable(WaveParams::presets()[4].1), |app, t| app.wave_window = Some(t)),
        ("wavetable-editor", InstrumentKind::Wavetable(WaveParams::presets()[4].1), |app, t| crate::table_editor::open(app, t, 0)),
        ("polychrome", InstrumentKind::Polychrome(WaveParams::polychrome()), |app, t| app.poly_window = Some(t)),
        ("modular", InstrumentKind::Modular(ModularParams::default()), |app, t| app.modular_window = Some(t)),
        ("kick-designer", InstrumentKind::Kick(Default::default()), |app, t| app.kick_window = Some(t)),
        ("pulse-keys", InstrumentKind::Keys(KeysParams::default()), |_, _| {}),
        ("drum-machine", InstrumentKind::DrumMachine(Default::default()), |_, _| {}),
    ];
    for (name, kind, open) in kinds {
        s.app.set_instrument(chords, kind);
        s.frames(5);
        save(&mut s, &format!("{name}-device"));
        open(&mut s.app, chords);
        s.frames(5);
        save(&mut s, name);
        s.app.wave_window = None;
        s.app.poly_window = None;
        s.app.modular_window = None;
        s.app.kick_window = None;
        s.app.table_editor = None;
    }
    // Drum Pads: the kick's track.
    s.app.selected_target = engine::Target::Track(kick);
    save(&mut s, "drum-pads");
    std::fs::remove_dir_all(&work).ok();
}
