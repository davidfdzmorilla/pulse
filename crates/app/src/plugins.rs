//! VST3 plugins in the app: finding them on the computer, putting them on tracks
//! and chains, their parameter window and keeping their state in the project.

use crate::app::{Detail, PulseApp};
use crate::project::PluginRef;
use crate::theme::{self, TEXT_DIM};
use eframe::egui;
use engine::model::{Device, Effect, InstrumentKind, PluginId};
use engine::plugin::PluginInfo;
use engine::{ChainSlot, Command, Target};
use std::path::{Path, PathBuf};

/// What the app knows about the plugins on this computer.
#[derive(Default)]
pub struct Library {
    pub found: Vec<PluginInfo>,
    /// Bundles that failed to load in the last search, with why.
    pub failed: Vec<String>,
    pub searched: bool,
    /// Instance whose parameter window is open.
    pub window: Option<PluginId>,
    pub filter: String,
}

fn cache_path() -> PathBuf {
    crate::kits::kits_dir().with_file_name("plugins.json")
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Cached {
    path: String,
    cid: String,
    name: String,
    vendor: String,
    category: String,
    instrument: bool,
}

impl Library {
    /// The list from the last search, if one was saved.
    pub fn load_cache() -> Library {
        let found: Vec<Cached> = std::fs::read_to_string(cache_path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        Library {
            searched: !found.is_empty(),
            found: found
                .into_iter()
                .map(|c| PluginInfo { path: PathBuf::from(c.path), cid: c.cid, name: c.name, vendor: c.vendor, category: c.category, instrument: c.instrument })
                .collect(),
            ..Default::default()
        }
    }

    /// Looks through `folders` for plugins (loading each bundle once) and remembers the list.
    pub fn search(&mut self, folders: &[PathBuf]) {
        self.found.clear();
        self.failed.clear();
        for folder in folders {
            for bundle in engine::plugin::bundles(folder) {
                match engine::plugin::scan(&bundle) {
                    Ok(list) => self.found.extend(list),
                    Err(error) => self.failed.push(error),
                }
            }
        }
        self.found.sort_by_key(|p| (!p.instrument, p.name.to_lowercase()));
        self.searched = true;
        let cached: Vec<Cached> = self
            .found
            .iter()
            .map(|p| Cached { path: p.path.to_string_lossy().into_owned(), cid: p.cid.clone(), name: p.name.clone(), vendor: p.vendor.clone(), category: p.category.clone(), instrument: p.instrument })
            .collect();
        if let Ok(text) = serde_json::to_string_pretty(&cached) {
            let _ = std::fs::create_dir_all(cache_path().parent().unwrap_or(Path::new(".")));
            let _ = std::fs::write(cache_path(), text);
        }
    }
}

// ----- State in the project, as base64 -------------------------------------------------

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= chunk.len() {
                text.push(ALPHABET[(n >> (18 - 6 * k)) as usize & 63] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}

pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut count) = (0u32, 0);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace()) {
        if c == b'=' {
            break;
        }
        bits = bits << 6 | ALPHABET.iter().position(|&a| a == c)? as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            bytes.push((bits >> count) as u8);
        }
    }
    Some(bytes)
}

// ----- Putting plugins on tracks and chains --------------------------------------------

impl PulseApp {
    /// Adds a plugin instance to the project and starts it.
    fn new_plugin(&mut self, info: &PluginInfo) -> Option<(PluginId, engine::plugin::PluginProcessor)> {
        let id = self.project.new_plugin_id();
        self.project.plugins.push(PluginRef {
            id,
            path: info.path.to_string_lossy().into_owned(),
            cid: info.cid.clone(),
            name: info.name.clone(),
            vendor: info.vendor.clone(),
            state: String::new(),
        });
        let sample_rate = self.audio.sample_rate();
        let processor = crate::sync::load_plugin(&self.project, id, &mut self.samples, sample_rate);
        if processor.is_none() {
            self.project.plugins.retain(|p| p.id != id);
            self.status = self.samples.errors.pop();
            self.samples.errors.clear();
        }
        processor.map(|p| (id, p))
    }

    /// Makes plugin `info` the instrument of track `track`.
    pub fn set_plugin_instrument(&mut self, track: usize, info: &PluginInfo) {
        let Some((id, processor)) = self.new_plugin(info) else { return };
        self.capture_plugins();
        let previous = std::mem::replace(&mut self.project.tracks[track].instrument, InstrumentKind::Plugin(id));
        if let InstrumentKind::Plugin(old) = previous {
            self.samples.plugins.remove(&old);
        }
        self.audio.send(Command::set_plugin(track, id, processor));
        self.status = Some(format!("{} en {}", info.name, self.project.tracks[track].name));
        self.detail = Detail::Devices;
    }

    /// Adds plugin effect `info` at the end of a chain.
    pub fn add_plugin_effect(&mut self, target: Target, info: &PluginInfo) {
        let Some((id, processor)) = self.new_plugin(info) else { return };
        let device = Device::new(Effect::Plugin(id));
        let index = self.devices(target).len();
        self.devices_mut(target).push(device);
        self.audio.send(Command::InsertDevice { target, index, slot: ChainSlot::plugin(&device, processor) });
        self.detail = Detail::Devices;
    }

    /// Copies every loaded plugin's current state into the project (before saving,
    /// rendering or rebuilding the engine's instruments).
    pub fn capture_plugins(&mut self) {
        for plugin in &mut self.project.plugins {
            if let Some(handle) = self.samples.plugins.get(&plugin.id) {
                plugin.state = encode(&handle.state());
            }
        }
    }

    /// The plugin instance's name, for titles.
    pub fn plugin_name(&self, id: PluginId) -> String {
        self.project.plugin(id).map_or_else(|| "Plugin".into(), |p| p.name.clone())
    }
}

/// A plugin parameter knob: normalized values, shown the way the plugin shows them.
fn param_knob(ui: &mut egui::Ui, handle: &engine::plugin::PluginHandle, param: &engine::plugin::PluginParam, diameter: f32) -> Option<f64> {
    let mut value = handle.value(param.id) as f32;
    let label: String = param.name.chars().take(10).collect();
    let knob = crate::widgets::Knob::new(&mut value, 0.0..=1.0, param.default as f32, &label).diameter(diameter).format(|x| handle.display(param.id, x as f64));
    let response = knob.show(ui).on_hover_text(&param.name);
    response.changed().then_some(value as f64)
}

/// A window a plugin device asks to open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PluginWindow {
    /// The plugin's own interface.
    Editor(PluginId),
    /// The list of all its parameters.
    Params(PluginId),
}

/// The body of a plugin's device: buttons for its editor and its parameter list, and
/// its first parameters as knobs.
pub fn device_body(app: &PulseApp, ui: &mut egui::Ui, id: PluginId, knobs: usize) -> Option<PluginWindow> {
    let mut open = None;
    let Some(handle) = app.samples.plugins.get(&id) else {
        ui.label(egui::RichText::new("No se pudo cargar el plugin.\nComprueba que sigue instalado.").color(TEXT_DIM).size(10.5));
        return None;
    };
    ui.vertical(|ui| {
        if let Some(plugin) = app.project.plugin(id) {
            ui.label(egui::RichText::new(&plugin.vendor).color(TEXT_DIM).size(10.5));
        }
        let editor = if app.editors.is_open(id) { "Editor ✓" } else { "Editor" };
        if ui.button(editor).on_hover_text("La ventana del propio plugin").clicked() {
            open = Some(PluginWindow::Editor(id));
        }
        if ui.button("Parámetros…").on_hover_text("Todos los parámetros del plugin, en una lista").clicked() {
            open = Some(PluginWindow::Params(id));
        }
    });
    ui.spacing_mut().item_spacing.x = 0.0;
    for param in handle.params().iter().filter(|p| p.automatable).take(knobs) {
        if let Some(v) = param_knob(ui, handle, param, 30.0) {
            handle.set(param.id, v);
        }
    }
    open
}

/// The window with every parameter of the plugin it was opened for.
pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    let Some(id) = app.plugin_library.window else { return };
    if !app.samples.plugins.contains_key(&id) {
        app.plugin_library.window = None;
        return;
    }
    let mut open = true;
    let title = format!("{} · parámetros", app.plugin_name(id));
    let mut filter = app.plugin_library.filter.clone();
    egui::Window::new(title).id(egui::Id::new(("plugin_window", id.0))).open(&mut open).default_size([420.0, 520.0]).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Buscar");
            ui.text_edit_singleline(&mut filter);
        });
        ui.separator();
        let handle = &app.samples.plugins[&id];
        let wanted = filter.to_lowercase();
        let params: Vec<_> = handle.params().into_iter().filter(|p| wanted.is_empty() || p.name.to_lowercase().contains(&wanted)).collect();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new(("plugin_params", id.0)).num_columns(3).striped(true).show(ui, |ui| {
                for param in &params {
                    ui.label(&param.name);
                    let mut value = handle.value(param.id);
                    let slider = egui::Slider::new(&mut value, 0.0..=1.0).show_value(false);
                    let slider = if param.steps > 0 { slider.step_by(1.0 / param.steps as f64) } else { slider };
                    if ui.add(slider).changed() {
                        handle.set(param.id, value);
                    }
                    ui.label(egui::RichText::new(handle.display(param.id, value)).color(theme::TEXT));
                    ui.end_row();
                }
            });
            if params.is_empty() {
                ui.label(egui::RichText::new("Ningún parámetro").color(TEXT_DIM));
            }
        });
    });
    app.plugin_library.filter = filter;
    if !open {
        app.plugin_library.window = None;
    }
}

/// The browser section: the plugins found, and a button to look for them.
pub fn browser(app: &mut PulseApp, ui: &mut egui::Ui, item: impl Fn(&mut egui::Ui, &str, &str, &str) -> bool) {
    let target = app.selected_target;
    let track = match target {
        Target::Track(t) if !app.project.tracks[t].is_audio() => Some(t),
        _ => None,
    };
    let folders = engine::plugin::default_folders();
    let hint = format!("Busca plugins VST3 en {}", folders.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", "));
    let search = if app.plugin_library.searched { "Volver a buscar plugins" } else { "Buscar plugins VST3" };
    if item(ui, "⟳", search, &hint) {
        app.plugin_library.search(&folders);
        let (found, failed) = (app.plugin_library.found.len(), app.plugin_library.failed.len());
        app.status = Some(match (found, failed) {
            (0, 0) => "No hay plugins VST3 instalados".to_string(),
            (n, 0) => format!("{n} plugins encontrados"),
            (n, f) => format!("{n} plugins encontrados; {f} no se pudieron cargar"),
        });
    }
    let found = app.plugin_library.found.clone();
    for info in &found {
        let label = if info.vendor.is_empty() { info.name.clone() } else { format!("{} · {}", info.name, info.vendor) };
        if info.instrument {
            let hint = if track.is_some() { "Clic: será el instrumento de la pista seleccionada" } else { "Selecciona una pista MIDI" };
            if item(ui, "♫", &label, hint) {
                if let Some(t) = track {
                    app.set_plugin_instrument(t, info);
                }
            }
        } else if item(ui, "≈", &label, "Clic: añade el efecto al final de la cadena seleccionada") {
            app.add_plugin_effect(target, info);
        }
    }
    if app.plugin_library.searched && found.is_empty() {
        ui.label(egui::RichText::new("No hay plugins VST3 instalados").color(TEXT_DIM).size(10.5));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The test plugin from crates/testplug, built and bundled.
    pub(crate) fn test_bundle() -> PathBuf {
        static BUNDLE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        BUNDLE
            .get_or_init(|| {
                let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let target = root.join("target/testplug");
                let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
                let status = std::process::Command::new(cargo).args(["build", "--release", "-p", "pulse-testplug", "--target-dir"]).arg(&target).current_dir(&root).status().expect("cargo build");
                assert!(status.success());
                let library = target.join("release").join(if cfg!(target_os = "macos") { "libpulse_testplug.dylib" } else if cfg!(target_os = "windows") { "pulse_testplug.dll" } else { "libpulse_testplug.so" });
                // A folder of its own, as a plugins folder would be.
                let bundle = target.join("app-plugins/PulseTest.vst3");
                let binary = bundle.join("Contents/MacOS/PulseTest");
                std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
                std::fs::copy(&library, &binary).unwrap();
                bundle
            })
            .clone()
    }

    #[test]
    fn base64_round_trips() {
        for bytes in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0, 255, 128, 7, 9]] {
            assert_eq!(decode(&encode(bytes)).unwrap(), bytes);
        }
        assert_eq!(encode(b"Pulse"), "UHVsc2U=");
        assert!(decode("no válido").is_none());
    }

    fn app() -> PulseApp {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        app.plugin_library.search(&[test_bundle().parent().unwrap().to_path_buf()]);
        app
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// Renders track 3's first clip offline, as an export would (fresh plugin instances
    /// from the saved state).
    fn render(app: &mut PulseApp) -> Vec<f32> {
        app.capture_plugins();
        let mut project = app.project.clone();
        let mut clip = engine::model::MidiClip::new("A", 4.0, [0, 0, 0]);
        clip.notes.push(engine::model::Note { start: 0.0, length: 4.0, pitch: 69, velocity: 1.0 });
        project.tracks[3].clips[0] = Some(engine::model::Clip::Midi(clip));
        let mut renderer = engine::OfflineRenderer::new(48_000.0);
        let mut samples = crate::sync::SampleCache::default();
        for command in crate::sync::project_commands(&project, &mut samples, 48_000.0, &Default::default()) {
            renderer.send(command);
        }
        renderer.send(Command::LaunchClip { track: 3, slot: 0 });
        let (mut l, mut r) = (Vec::new(), Vec::new());
        renderer.render(96_000, &mut l, &mut r);
        l
    }

    #[test]
    fn a_plugin_instrument_plays_the_track_and_keeps_its_settings() {
        let mut app = app();
        let names: Vec<&str> = app.plugin_library.found.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Pulse Test Synth", "Pulse Test Gain"], "instruments first");
        // The lead track (3) plays notes in scene 1; only it is heard.
        for (t, track) in app.project.tracks.iter_mut().enumerate() {
            track.mixer.gain = if t == 3 { 1.0 } else { 0.0 };
        }
        let synth = app.plugin_library.found[0].clone();
        app.set_plugin_instrument(3, &synth);
        let InstrumentKind::Plugin(id) = app.project.tracks[3].instrument else { panic!("the track has the plugin") };
        let loud = rms(&render(&mut app));
        assert!(loud > 0.01, "the plugin plays the clip: {loud}");

        // Turn its level down, save, load: the plugin comes back with it.
        let handle = &app.samples.plugins[&id];
        let level = handle.params().iter().find(|p| p.name == "Level").unwrap().id;
        handle.set(level, 0.125);
        let quiet = rms(&render(&mut app));
        assert!((quiet / loud - 0.25).abs() < 0.02, "{quiet} / {loud}");
        let file = std::env::temp_dir().join(format!("pulse-plugin-test-{}.pulse", std::process::id()));
        app.save_to_path(&file);
        let loaded = crate::project::Project::load(&file).unwrap();
        let _ = std::fs::remove_file(&file);
        let mut reopened = PulseApp::headless(loaded);
        assert!((rms(&render(&mut reopened)) / loud - 0.25).abs() < 0.02, "the saved state");
        reopened.sync_all();
        assert_eq!(reopened.samples.plugins[&id].value(level), 0.125, "the live instance too");
    }

    #[test]
    fn a_plugin_effect_works_in_a_chain_and_survives_undo() {
        let mut app = app();
        for (t, track) in app.project.tracks.iter_mut().enumerate() {
            track.mixer.gain = if t == 3 { 1.0 } else { 0.0 };
        }
        let plain = rms(&render(&mut app));
        let gain = app.plugin_library.found[1].clone();
        app.add_plugin_effect(Target::Track(3), &gain);
        let Effect::Plugin(id) = app.project.tracks[3].devices.last().unwrap().effect else { panic!() };
        app.samples.plugins[&id].set(0, 0.25);
        let halved = rms(&render(&mut app));
        assert!((halved / plain - 0.5).abs() < 0.02, "{halved} / {plain}");
        // An undo rebuilds the engine; the plugin keeps its setting.
        app.commit_history();
        app.project.tempo = 100.0;
        app.commit_history();
        app.undo();
        assert_eq!(app.samples.plugins[&id].value(0), 0.25);
        assert_eq!(app.project.tracks[3].devices.last().unwrap().effect, Effect::Plugin(id));
    }

    /// The first instrument installed on this computer, through the whole app: found,
    /// put on a track, changed, saved, opened again and rendered.
    #[test]
    #[ignore = "diagnostic: uses the plugins installed on this computer"]
    fn an_installed_instrument_in_a_project() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        app.plugin_library.search(&engine::plugin::default_folders());
        let Some(info) = app.plugin_library.found.iter().find(|p| p.instrument).cloned() else {
            println!("no hay instrumentos instalados");
            return;
        };
        for (t, track) in app.project.tracks.iter_mut().enumerate() {
            track.mixer.gain = if t == 3 { 1.0 } else { 0.0 };
        }
        app.set_plugin_instrument(3, &info);
        let InstrumentKind::Plugin(id) = app.project.tracks[3].instrument else { panic!("{:?}", app.status) };
        let before = rms(&render(&mut app));
        // Half the volume, if it has a parameter called that.
        let handle = &app.samples.plugins[&id];
        let volume = handle.params().into_iter().find(|p| p.name.to_lowercase().contains("volume"));
        if let Some(v) = &volume {
            println!("{}: {} = {} → {}", info.name, v.name, handle.display(v.id, handle.value(v.id)), handle.display(v.id, handle.value(v.id) * 0.5));
            handle.set(v.id, handle.value(v.id) * 0.5);
        }
        let file = std::env::temp_dir().join(format!("pulse-installed-{}.pulse", std::process::id()));
        app.save_to_path(&file);
        let mut reopened = PulseApp::headless(crate::project::Project::load(&file).unwrap());
        let _ = std::fs::remove_file(&file);
        let after = rms(&render(&mut reopened));
        println!("{}: nivel {before:.4} → {after:.4} tras guardar y abrir", info.name);
        assert!(before > 0.001 && after > 0.0);
        if volume.is_some() {
            assert!(after < before * 0.9, "the saved volume came back");
        }
    }

    #[test]
    fn editors_explain_why_they_do_not_open() {
        let mut app = app();
        let synth = app.plugin_library.found[0].clone();
        app.set_plugin_instrument(3, &synth);
        let InstrumentKind::Plugin(id) = app.project.tracks[3].instrument else { panic!() };
        app.open_editor(id);
        assert!(!app.editors.is_open(id));
        let status = app.status.clone().unwrap_or_default();
        // Tests do not run on the main thread; the test plugin has no editor anyway.
        assert!(status.contains("ventana principal") || status.contains("no tiene editor"), "{status}");
        app.open_editor(PluginId(999));
        assert!(app.status.as_deref().is_some_and(|s| s.contains("no está cargado")));
        // Rebuilding the engine forgets nothing it should not.
        app.sync_all();
        app.poll_editors();
        assert!(!app.editors.is_open(id));
    }

    #[test]
    fn missing_plugins_leave_a_silent_slot_and_say_so() {
        let mut project = crate::project::Project::demo();
        project.plugins.push(PluginRef { id: PluginId(1), path: "/nowhere/Gone.vst3".into(), cid: "00".repeat(16), name: "Gone".into(), vendor: String::new(), state: String::new() });
        project.tracks[3].instrument = InstrumentKind::Plugin(PluginId(1));
        let mut app = PulseApp::headless(project);
        app.sync_all();
        assert!(app.status.as_deref().is_some_and(|s| s.contains("Gone")), "{:?}", app.status);
        assert!(!app.samples.plugins.contains_key(&PluginId(1)));
        // The device view says what happened instead of failing.
        app.selected_target = Target::Track(3);
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| crate::devices::show(&mut app, ui));
    }

    #[test]
    fn the_device_view_and_the_parameter_window_draw() {
        let mut app = app();
        let synth = app.plugin_library.found[0].clone();
        let gain = app.plugin_library.found[1].clone();
        app.set_plugin_instrument(3, &synth);
        app.add_plugin_effect(Target::Track(3), &gain);
        app.selected_target = Target::Track(3);
        let InstrumentKind::Plugin(id) = app.project.tracks[3].instrument else { panic!() };
        app.plugin_library.window = Some(id);
        app.plugin_library.filter = "lev".into();
        let ctx = egui::Context::default();
        for n in 0..3 {
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), time: Some(n as f64 * 0.05), ..Default::default() };
            let _ = ctx.run_ui(input, |ui| {
                crate::devices::show(&mut app, ui);
                crate::browser::show(&mut app, ui);
                window(&mut app, &ui.ctx().clone());
            });
        }
        assert_eq!(app.plugin_library.window, Some(id));
        assert_eq!(app.samples.plugins[&id].value(0), 0.5, "drawing changes nothing");
    }
}
