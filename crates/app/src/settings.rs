//! Preferences (⌘,): the audio devices, sample rate and buffer, the MIDI inputs,
//! extra plugin folders, and whether to reopen the last project at start. Kept for
//! the next time Pulse starts (Application Support/Pulse/settings.json).

use crate::app::PulseApp;
use crate::theme::TEXT_DIM;
use eframe::egui;
use engine::{AudioSettings, DeviceInfo};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub audio: AudioSettings,
    #[serde(default = "yes")]
    pub reopen_last: bool,
    /// Folders to look for plugins in, besides the system's.
    #[serde(default)]
    pub plugin_folders: Vec<PathBuf>,
    /// Look for a new version when Pulse starts (see `updates`).
    #[serde(default = "yes")]
    pub check_updates: bool,
    /// A version the user chose not to hear about.
    #[serde(default)]
    pub skipped_version: Option<String>,
    /// The version that ran last (its notes show on the first start of a new one).
    #[serde(default)]
    pub last_version: Option<String>,
    /// Record the controls moved while recording as automation ("Automation Arm").
    #[serde(default = "yes")]
    pub automation_arm: bool,
    /// The user's own folders in the browser.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub browser_folders: Vec<PathBuf>,
    /// Pulse's folder, if not ~/Music/Pulse (see `library`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<PathBuf>,
    /// Bars the metronome counts before recording starts the transport.
    #[serde(default)]
    pub count_in_bars: u32,
    /// Milliseconds recorded audio is moved earlier, on top of the latency the
    /// devices report (negative: later).
    #[serde(default)]
    pub record_offset_ms: f32,
    /// New loops are transposed to the project's key (see `loop_key`).
    #[serde(default = "yes")]
    pub loops_follow_key: bool,
    /// The browser plays loops at the project's tempo, looping (see `browser`).
    #[serde(default = "yes")]
    pub preview_in_tempo: bool,
    /// The export dialog's last choices.
    #[serde(default)]
    pub export: crate::export::Settings,
    /// Settings this Pulse doesn't know (a newer one wrote them): kept, so going
    /// back to an older version and forward again loses none.
    #[serde(flatten)]
    pub unknown: serde_json::Map<String, serde_json::Value>,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self { audio: AudioSettings::default(), reopen_last: true, plugin_folders: Vec::new(), check_updates: true, skipped_version: None, last_version: None, automation_arm: true, browser_folders: Vec::new(), library: None, count_in_bars: 0, record_offset_ms: 0.0, loops_follow_key: true, preview_in_tempo: true, export: Default::default(), unknown: Default::default() }
    }
}

fn path() -> PathBuf {
    paths::config_dir().join("settings.json")
}

impl Settings {
    pub fn load() -> Settings {
        crate::store::read_json(&path())
    }

    pub(crate) fn save(&self) {
        if let Err(e) = crate::store::write_json(&path(), self) {
            crate::logs::note(&e);
        }
    }
}

/// The devices shown in the window, read when it opens (listing them takes a moment).
#[derive(Default)]
pub struct Devices {
    pub outputs: Vec<DeviceInfo>,
    pub inputs: Vec<DeviceInfo>,
}

impl PulseApp {
    /// Where plugins are searched for: the system's folders and the user's own.
    pub fn plugin_folders(&self) -> Vec<PathBuf> {
        let mut folders = engine::plugin::default_folders();
        folders.extend(self.settings.plugin_folders.iter().cloned());
        folders
    }

    pub fn open_settings(&mut self) {
        self.open_panel(crate::app::Panel::Settings);
        self.devices = Devices { outputs: engine::output_devices(), inputs: engine::input_devices() };
    }

    /// Saves changed settings now (they wait a second, see `set_settings`).
    pub(crate) fn save_settings_now(&mut self) {
        if self.settings_changed.take().is_some() {
            self.settings.save();
        }
    }

    /// Changes the settings: saves them and restarts the audio if it changed.
    pub fn set_settings(&mut self, settings: Settings) {
        let audio_changed = settings.audio != self.settings.audio;
        if settings.library != self.settings.library {
            crate::library::set_root(settings.library.clone());
            crate::library::prepare();
        }
        let count_in = settings.count_in_bars != self.settings.count_in_bars;
        self.settings = settings;
        if count_in {
            self.audio.send(engine::Command::SetCountIn(self.settings.count_in_bars as f64 * 4.0));
        }
        if self.interactive {
            // A second after the last change (a drag changes them every frame,
            // and each save waits for the disk), or on quitting.
            self.settings_changed = Some(std::time::Instant::now());
        }
        if audio_changed {
            self.restart_audio();
        }
    }

    /// Restarts the engine with the audio settings, and everything that hangs on it.
    pub fn restart_audio(&mut self) {
        if !self.interactive {
            return;
        }
        // The old stream goes first, so the device is free.
        self.capture_plugins();
        self.audio = crate::app::Audio::none();
        self.audio = crate::app::Audio::start(&self.settings.audio);
        self.capture = None;
        self.capture_failed = false;
        self.sync_all();
        self.rescan_midi();
        self.status = Some(match &self.audio.error {
            Some(error) => format!("Audio: {error}"),
            None => self.audio_summary(),
        });
    }

    /// The device, rate and buffer in use, e.g. "Altavoces · 48 kHz · 256 (5,3 ms)".
    pub fn audio_summary(&self) -> String {
        let (device, rate, buffer) = self.audio.info();
        let buffer = buffer.map_or_else(|| "búfer por defecto".to_string(), |b| format!("{b} muestras ({:.1} ms)", b as f32 / rate * 1000.0));
        format!("{device} · {:.1} kHz · {buffer}", rate / 1000.0)
    }
}

/// The preferences window.
pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    if !app.show_settings {
        return;
    }
    let mut open = true;
    let mut next = app.settings.clone();
    let mut rescan_midi = false;
    let mut refresh = false;
    let mut search_plugins = false;
    let mut show_folder = false;
    egui::Window::new("Ajustes").open(&mut open).resizable(false).collapsible(false).default_width(460.0).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
        ui.heading("Audio");
        ui.label(egui::RichText::new(format!("En uso: {}", app.audio_summary())).color(TEXT_DIM).size(10.5));
        if let Some(error) = &app.audio.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        let default = "Predeterminado del sistema";
        egui::Grid::new("audio_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Salida");
            device_combo(ui, "audio_output", &mut next.audio.output, &app.devices.outputs, default);
            ui.end_row();
            ui.label("Entrada (grabar audio)");
            device_combo(ui, "audio_input", &mut next.audio.input, &app.devices.inputs, default);
            ui.end_row();
            let output = app.devices.outputs.iter().find(|d| Some(&d.name) == next.audio.output.as_ref()).or(app.devices.outputs.first());
            let rates = output.map(|d| d.rates.clone()).unwrap_or_default();
            let buffers = output.map(|d| d.buffers.clone()).unwrap_or_default();
            ui.label("Frecuencia de muestreo");
            let label = |r: Option<u32>| r.map_or("Predeterminada".to_string(), |r| format!("{:.1} kHz", r as f32 / 1000.0));
            egui::ComboBox::from_id_salt("audio_rate").selected_text(label(next.audio.sample_rate)).width(200.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut next.audio.sample_rate, None, label(None));
                for r in &rates {
                    ui.selectable_value(&mut next.audio.sample_rate, Some(*r), label(Some(*r)));
                }
            });
            ui.end_row();
            ui.label("Tamaño del búfer");
            let rate = next.audio.sample_rate.unwrap_or(48_000) as f32;
            let label = |b: Option<u32>| b.map_or("Predeterminado".to_string(), |b| format!("{b} muestras · {:.1} ms", b as f32 / rate * 1000.0));
            egui::ComboBox::from_id_salt("audio_buffer").selected_text(label(next.audio.buffer)).width(200.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut next.audio.buffer, None, label(None));
                for b in &buffers {
                    ui.selectable_value(&mut next.audio.buffer, Some(*b), label(Some(*b)));
                }
            });
            ui.end_row();
        });
        ui.label(egui::RichText::new("Un búfer menor responde antes al tocar; si oyes cortes, súbelo.").color(TEXT_DIM).size(10.5));
        ui.add_space(4.0);
        let (input, output) = app.latencies();
        ui.label(format!(
            "Latencia: entrada {} · salida {:.1} ms. Pulse la descuenta al grabar para que las tomas caigan a tempo.",
            input.map_or("— (sin abrir)".to_string(), |s| format!("{:.1} ms", s * 1000.0)),
            output * 1000.0
        ));
        ui.horizontal(|ui| {
            ui.label("Ajuste fino de la grabación");
            ui.add(egui::DragValue::new(&mut next.record_offset_ms).range(-200.0..=200.0).speed(0.2).fixed_decimals(1).suffix(" ms"))
                .on_hover_text("Si las tomas de audio quedan tarde, súbelo (las adelanta); si quedan pronto, bájalo");
        });
        refresh = ui.small_button("Volver a buscar dispositivos").clicked();

        ui.separator();
        ui.heading("MIDI");
        if app.midi.names.is_empty() {
            ui.label(egui::RichText::new("Ningún dispositivo MIDI conectado").color(TEXT_DIM));
        }
        for name in &app.midi.names {
            ui.label(format!("● {name}"));
        }
        rescan_midi = ui.small_button("Volver a buscar dispositivos MIDI").clicked();

        ui.separator();
        ui.heading("Plugins");
        for folder in engine::plugin::default_folders() {
            ui.label(egui::RichText::new(folder.display().to_string()).color(TEXT_DIM).size(10.5));
        }
        let mut remove = None;
        for (i, folder) in next.plugin_folders.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(folder.display().to_string()).size(10.5));
                if ui.small_button("×").on_hover_text("Quitar").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            next.plugin_folders.remove(i);
        }
        ui.horizontal(|ui| {
            if ui.button("Añadir carpeta…").clicked() {
                if let Some(folder) = crate::files::pick_folder(rfd::FileDialog::new()) {
                    next.plugin_folders.push(folder);
                }
            }
            search_plugins = ui.button("Buscar plugins").clicked();
        });

        ui.separator();
        ui.heading("Carpeta de Pulse");
        ui.label(egui::RichText::new("Proyectos (cada uno en su carpeta, con sus audios), exportaciones, presets, wavetables y muestras").color(TEXT_DIM).size(10.5));
        // ~ for the home folder, and cut with "…" if long (the tooltip has it all).
        let root = crate::library::root().display().to_string();
        let shown = paths::shorten(&crate::library::root());
        ui.add(egui::Label::new(shown).truncate()).on_hover_text(root);
        ui.horizontal(|ui| {
            if ui.button("Cambiar…").on_hover_text("Lo que ya hay no se mueve: los proyectos se abren desde donde estén").clicked() {
                if let Some(folder) = crate::files::pick_folder(rfd::FileDialog::new().set_directory(crate::library::root())) {
                    next.library = (folder != crate::library::default_root()).then_some(folder);
                }
            }
            if next.library.is_some() && ui.button("Volver a Música/Pulse").clicked() {
                next.library = None;
            }
            if ui.button(if cfg!(windows) { "Mostrar en el Explorador" } else { "Mostrar en Finder" }).clicked() {
                show_folder = true;
            }
        });

        ui.separator();
        ui.heading("Proyectos");
        ui.checkbox(&mut next.reopen_last, "Abrir el último proyecto al arrancar");
        ui.checkbox(&mut next.check_updates, "Buscar actualizaciones al arrancar");
    });
    if next != app.settings {
        app.set_settings(next);
    }
    if refresh {
        app.open_settings();
    }
    if rescan_midi {
        app.rescan_midi();
    }
    if show_folder {
        app.run(crate::commands::Command::ShowFolder, ctx);
    }
    if search_plugins {
        let folders = app.plugin_folders();
        app.plugin_library.search(&folders);
        app.plugin_library.remember();
        app.status = Some(format!("{} plugins encontrados", app.plugin_library.found.len()));
    }
    if !open {
        app.show_settings = false;
    }
}

fn device_combo(ui: &mut egui::Ui, id: &str, value: &mut Option<String>, devices: &[DeviceInfo], default: &str) {
    egui::ComboBox::from_id_salt(id).selected_text(value.clone().unwrap_or_else(|| default.into())).width(200.0).show_ui(ui, |ui| {
        ui.selectable_value(value, None, default);
        for d in devices {
            ui.selectable_value(value, Some(d.name.clone()), &d.name);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_load_from_older_files_and_round_trip() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, Settings::default());
        assert!(settings.reopen_last);
        let custom = Settings { audio: AudioSettings { output: Some("Altavoces".into()), input: None, sample_rate: Some(44_100), buffer: Some(128) }, reopen_last: false, plugin_folders: vec![PathBuf::from("/x")], check_updates: false, skipped_version: Some("0.2.0".into()), last_version: Some("0.1.2".into()), automation_arm: false, browser_folders: vec![PathBuf::from("/Volumes/Muestras")], library: Some("/Volumes/Musica/Pulse".into()), count_in_bars: 2, record_offset_ms: -3.5, loops_follow_key: false, preview_in_tempo: false, export: Default::default(), unknown: Default::default() };
        let text = serde_json::to_string(&custom).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&text).unwrap(), custom);
    }

    /// Settings a newer Pulse wrote survive this one saving them.
    #[test]
    fn settings_this_pulse_does_not_know_are_kept() {
        let newer = r#"{"reopen_last":false,"a_new_setting":{"x":1},"another":[1,2]}"#;
        let settings: Settings = serde_json::from_str(newer).unwrap();
        assert!(!settings.reopen_last);
        let again: serde_json::Value = serde_json::to_value(&settings).unwrap();
        assert_eq!(again["a_new_setting"]["x"], 1);
        assert_eq!(again["another"][1], 2);
    }

    #[test]
    fn the_window_draws_and_changes_apply() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        app.show_settings = true;
        app.devices = Devices { outputs: vec![DeviceInfo { name: "Altavoces".into(), rates: vec![44_100, 48_000], buffers: vec![128, 256] }], inputs: Vec::new() };
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| window(&mut app, ui.ctx()));
        assert!(app.show_settings);
        assert_eq!(app.settings, Settings::default(), "drawing changes nothing");
        // Headless apps neither save nor restart the audio.
        app.set_settings(Settings { reopen_last: false, ..Settings::default() });
        assert!(!app.settings.reopen_last);
        assert!(app.plugin_folders().len() >= 2);
        assert!(app.audio_summary().contains("kHz"));
    }
}
