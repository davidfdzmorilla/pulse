//! The browser's search, favourites and chains through the interface without
//! a window: ⌘F, typing, ↑ ↓, Enter, ⌥Enter, Esc, double clicks, the right-click
//! menu and «Guardar cadena…». With `PULSE_SHOTS=dir`, PNGs of each step.

use crate::app::PulseApp;
use crate::commands::Command;
use crate::produccion::Studio;
use crate::project::Project;
use eframe::egui::{self, vec2, Key, Modifiers, Rect};
use engine::model::Effect;
use engine::Target;
use std::path::PathBuf;

fn shots() -> Option<PathBuf> {
    std::env::var_os("PULSE_SHOTS").map(PathBuf::from)
}

fn shot(s: &mut Studio, name: &str) {
    if let Some(dir) = shots() {
        std::fs::create_dir_all(&dir).unwrap();
        s.out = dir;
        s.shot(name);
    } else {
        s.frames(2);
    }
}

/// A row of the browser (on the left) showing `text`.
fn row(s: &Studio, text: &str) -> Rect {
    s.seen.iter().filter(|w| w.text == text && w.role == "Button" && w.rect.left() < 320.0).map(|w| w.rect).next().unwrap_or_else(|| {
        let left: Vec<&str> = s.seen.iter().filter(|w| w.rect.left() < 320.0).map(|w| w.text.as_str()).collect();
        panic!("no hay «{text}» en el navegador: {left:?}")
    })
}

fn searching(s: &Studio) -> bool {
    s.ctx().memory(|m| m.has_focus(egui::Id::new("browser_search")))
}

fn labels(app: &PulseApp, target: Target) -> Vec<&'static str> {
    app.devices(target).iter().map(|d| d.effect.label()).collect()
}

#[test]
fn search_keys_favourites_and_chains_through_the_interface() {
    let mut app = PulseApp::headless(Project::demo());
    app.selected_target = Target::Track(0);
    let mut s = Studio::new(app, std::env::temp_dir());
    let track = Target::Track(0);
    let before = s.app.devices(track).len();

    // ⌘F, «espa»: Espacio first, without pressing Enter.
    s.key(Key::F, Modifiers::COMMAND);
    assert!(searching(&s), "⌘F focuses the search");
    s.type_text("espa");
    assert_eq!(s.app.browser.results[0].name, "Espacio");
    shot(&mut s, "busqueda-espa");
    // ↓ ↑ move, Enter puts it on the track; ⌘Z takes it away.
    s.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(s.app.browser.result, 1);
    s.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(s.app.browser.result, 0);
    s.key(Key::Enter, Modifiers::NONE);
    assert_eq!(s.app.devices(track).len(), before + 1);
    assert!(matches!(s.app.devices(track).last().unwrap().effect, Effect::Convolution(_)));
    assert!(!searching(&s) && s.app.browser.keys, "after Enter ⌘Z undoes, the arrows still choose");
    s.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(s.app.browser.result, 1);
    s.menu(Command::Undo);
    assert_eq!(s.app.devices(track).len(), before);
    // Esc clears; in the field, a second Esc gives the keys back.
    s.key(Key::Escape, Modifiers::NONE);
    assert!(s.app.browser.search.is_empty());
    s.key(Key::F, Modifiers::COMMAND);
    assert!(searching(&s));
    s.key(Key::Escape, Modifiers::NONE);
    assert!(!searching(&s));

    // «comp bus», a double click on the result.
    s.key(Key::F, Modifiers::COMMAND);
    s.type_text("comp bus");
    assert_eq!(s.app.browser.results[0].name, "Compresor de bus");
    shot(&mut s, "busqueda-comp-bus");
    let compresor = row(&s, "Compresor de bus");
    s.dclick_at(compresor.center());
    assert_eq!(s.app.devices(track).last().map(|d| d.effect.label()), Some("Compresor de bus"));
    // Right click › Añadir a favoritos: kept in the user's settings.
    s.rclick_at(compresor.center());
    shot(&mut s, "favorito-menu");
    s.click("Añadir a favoritos");
    let saved = crate::search::Favorites::load_from(&crate::search::favorites_path());
    assert!(saved.keys.contains(&"efecto:Compresor de bus".to_string()), "{saved:?}");
    // Without a search, FAVORITOS at the top, with it.
    s.click_at(row(&s, "Compresor de bus").center() + vec2(0.0, -60.0));
    s.key(Key::F, Modifiers::COMMAND);
    s.key(Key::Escape, Modifiers::NONE);
    s.frames(3);
    assert!(s.has("FAVORITOS"));
    let favourite = s.find("FAVORITOS").unwrap();
    let instruments = s.find("INSTRUMENTOS").unwrap();
    assert!(favourite.top() < instruments.top(), "favourites come first");
    shot(&mut s, "favoritos");

    // A chain: Enter adds its effects, ⌥Enter replaces them; ⌘Z, one step.
    s.key(Key::F, Modifiers::COMMAND);
    s.type_text("voz pop");
    assert_eq!(s.app.browser.results[0].name, "Voz · Pop");
    shot(&mut s, "busqueda-cadena");
    let count = s.app.devices(track).len();
    s.key(Key::Enter, Modifiers::NONE);
    assert_eq!(s.app.devices(track).len(), count + 6);
    s.key(Key::Escape, Modifiers::NONE);
    s.key(Key::F, Modifiers::COMMAND);
    s.type_text("voz radio");
    s.key(Key::Enter, Modifiers::ALT);
    assert_eq!(labels(&s.app, track), ["EQ Eight", "Saturator", "Compressor", "Utility", "Limiter"]);
    s.menu(Command::Undo);
    assert_eq!(s.app.devices(track).len(), count + 6, "⌘Z: the chain before");
    s.key(Key::Escape, Modifiers::NONE);
    s.key(Key::Escape, Modifiers::NONE);

    // «Guardar cadena…» in Dispositivos, then found by the search.
    s.app.detail = crate::app::Detail::Devices;
    s.frames(3);
    s.click("Guardar cadena…");
    shot(&mut s, "guardar-cadena");
    let field = s.find("Nombre de la cadena").unwrap();
    s.click_at(field.center());
    s.type_text("Mi cadena de voz");
    s.click("Guardar");
    let file = crate::chains::user_root().join("Mi cadena de voz.json");
    assert!(file.exists(), "{:?}", s.app.status);
    let saved = crate::chains::read(&file).unwrap();
    assert_eq!(saved.devices.len(), count + 6);
    s.key(Key::F, Modifiers::COMMAND);
    s.type_text("mi cadena");
    assert_eq!(s.app.browser.results[0].name, "Mi cadena de voz");
    shot(&mut s, "busqueda-mi-cadena");
    s.key(Key::Escape, Modifiers::NONE);
    std::fs::remove_file(&file).ok();

    // The narrow window.
    s.size = vec2(1000.0, 720.0);
    s.key(Key::F, Modifiers::COMMAND);
    s.type_text("reverb");
    shot(&mut s, "busqueda-estrecha");
    let top: Vec<&str> = s.app.browser.results.iter().take(3).map(|r| r.name.as_str()).collect();
    assert!(top.contains(&"Espacio"), "{:?} {top:?}", s.app.browser.search);
}

/// In English, the favourites and the search show Pulse's names in English,
/// and the search finds them by them.
#[test]
fn in_english_favourites_and_search_show_english_names() {
    crate::i18n::with(crate::i18n::Language::English, || {
        let mut app = PulseApp::headless(Project::demo());
        app.selected_target = Target::Track(0);
        // Only in memory: the favourites file is left alone.
        app.browser.favorites().keys = vec!["preset:Acid Bass:Acid · Cuadrada hueca".into(), "efecto:Compresor de bus".into()];
        let mut s = Studio::new(app, std::env::temp_dir());
        s.frames(4);
        assert!(s.has("FAVORITES"));
        assert!(s.has("Acid · Hollow square") && !s.has("Acid · Cuadrada hueca"));
        shot(&mut s, "favoritos-en");
        s.key(Key::F, Modifiers::COMMAND);
        s.type_text("hollow sq");
        assert_eq!(s.app.browser.results[0].name, "Acid · Hollow square");
        assert!(s.has("PRESETS  (1)"), "the group's heading");
        shot(&mut s, "busqueda-en");
    });
}
