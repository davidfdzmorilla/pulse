//! Left-hand browser: instruments and audio effects to add to the selected track,
//! and Pulse's folder (see `library`) with the user's own places: samples to hear
//! and drag where they go, projects to open, presets to apply.

use crate::app::{Placement, PulseApp};
use crate::theme::{self, ACCENT, TEXT, TEXT_DIM};
use eframe::egui::{self, pos2, vec2, Align2, RichText, Sense};
use engine::model::{Device, Effect, InstrumentKind};
use engine::Target;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What the browser keeps between frames.
#[derive(Default)]
pub struct State {
    pub search: String,
    /// The file being heard, since when, and for how long.
    previewing: Option<(PathBuf, Instant, f64)>,
    /// The row clicked last.
    selected: Option<PathBuf>,
    /// Folder listings, refreshed every couple of seconds while shown.
    listings: HashMap<PathBuf, (Instant, Vec<Entry>)>,
    /// The last search and what it found.
    found: Option<(String, Instant, Vec<PathBuf>)>,
}

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    dir: bool,
}

/// What can be dragged out of the browser.
#[derive(Clone, Debug)]
pub struct Dragged(pub PathBuf);

/// Where things dragged from the browser can land this frame (filled while the
/// views are drawn).
#[derive(Default)]
pub struct DropTargets {
    /// Session slots: track, slot, where.
    pub slots: Vec<(usize, usize, egui::Rect)>,
    /// Arrangement lanes: track, the visible part, the timeline's left edge and
    /// its pixels per beat.
    pub lanes: Vec<(usize, egui::Rect, f32, f32)>,
    /// The Session or Arrangement area.
    pub view: Option<egui::Rect>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Audio,
    Project,
    Preset,
}

fn kind(path: &Path) -> Option<Kind> {
    if crate::app::is_audio_file(path) {
        return Some(Kind::Audio);
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some("pulse") => Some(Kind::Project),
        Some("json") => Some(Kind::Preset),
        _ => None,
    }
}

/// A folder's subfolders and the files the browser shows, folders first, by name.
fn read(dir: &Path) -> Vec<Entry> {
    let mut entries: Vec<Entry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let dir = e.file_type().ok()?.is_dir();
            (!name.starts_with('.') && (dir || kind(&e.path()).is_some())).then(|| Entry { path: e.path(), name, dir })
        })
        .collect();
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    entries
}

impl State {
    fn list(&mut self, dir: &Path) -> Vec<Entry> {
        let fresh = self.listings.get(dir).is_some_and(|(at, _)| at.elapsed() < Duration::from_secs(2));
        if !fresh {
            self.listings.insert(dir.to_path_buf(), (Instant::now(), read(dir)));
        }
        self.listings.get(dir).map(|(_, e)| e.clone()).unwrap_or_default()
    }

    /// Files under `roots` whose name has every word of `query` (up to a few
    /// hundred, from a bounded walk so a huge folder doesn't stall the app).
    fn search(&mut self, query: &str, roots: &[PathBuf]) -> Vec<PathBuf> {
        let stale = match &self.found {
            Some((q, at, _)) => q != query || at.elapsed() > Duration::from_secs(5),
            None => true,
        };
        if stale {
            let words: Vec<String> = query.to_lowercase().split_whitespace().map(String::from).collect();
            let mut found = Vec::new();
            let mut pending: Vec<PathBuf> = roots.to_vec();
            let mut visited = 0;
            while let Some(dir) = pending.pop() {
                for entry in read(&dir) {
                    visited += 1;
                    if entry.dir {
                        pending.push(entry.path);
                    } else if words.iter().all(|w| entry.name.to_lowercase().contains(w.as_str())) {
                        found.push(entry.path);
                    }
                }
                if found.len() >= 300 || visited > 20_000 {
                    break;
                }
            }
            found.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
            self.found = Some((query.to_string(), Instant::now(), found));
        }
        self.found.as_ref().map(|(_, _, f)| f.clone()).unwrap_or_default()
    }

    pub fn previewing(&self) -> Option<&Path> {
        self.previewing.as_ref().map(|(p, ..)| p.as_path())
    }
}

/// A section that folds away (egui remembers which are open).
fn section(ui: &mut egui::Ui, title: &str, contents: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(4.0);
    egui::CollapsingHeader::new(RichText::new(title).color(TEXT_DIM).size(10.5).strong()).id_salt(("browser_section", title)).default_open(true).show_unindented(ui, contents);
}

/// A full-width list row; returns true when clicked.
fn item(ui: &mut egui::Ui, icon: &str, label: &str, hint: &str) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 20.0), Sense::click());
    crate::widgets::name(&response, egui::WidgetType::Button, label);
    if response.hovered() {
        ui.painter().rect_filled(rect, 2.0, theme::CONTROL);
    }
    ui.painter().text(
        rect.left_center() + vec2(6.0, 0.0),
        Align2::LEFT_CENTER,
        icon,
        theme::small(),
        ACCENT,
    );
    // Cut with "…" at the panel's edge (the tooltip has the whole name).
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_string(), theme::body(), TEXT);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 26.0);
    let galley = ui.painter().layout_job(job);
    let cut = galley.elided;
    ui.painter().galley(pos2(rect.left() + 22.0, rect.center().y - galley.size().y / 2.0), galley, TEXT);
    let hint = if cut { format!("{label}\n{hint}") } else { hint.to_string() };
    response.on_hover_text(hint).clicked()
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    ui.label(RichText::new("Navegador").strong());
    let target = app.selected_target;
    ui.label(
        RichText::new(format!("Destino: {}", app.target_name(target)))
            .color(TEXT_DIM)
            .size(10.5),
    );
    ui.separator();

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        section(ui, "INSTRUMENTOS", |ui| {
            let track = match target {
                Target::Track(t) if !app.project.tracks[t].is_audio() => Some(t),
                _ => None,
            };
            let hint = if track.is_some() {
                "Clic: reemplaza el instrumento de la pista seleccionada"
            } else {
                "Selecciona una pista MIDI para cambiar su instrumento"
            };
            for kind in InstrumentKind::catalog() {
                let icon = if kind.is_drums() { "◉" } else { "♫" };
                if item(ui, icon, kind.label(), hint) {
                    if let Some(t) = track {
                        // An empty rack would be silent: start it with the basic kit.
                        if matches!(kind, InstrumentKind::DrumRack(_)) && app.project.tracks[t].samples.is_empty() {
                            app.load_basic_kit(t);
                        } else {
                            app.set_instrument(t, kind);
                        }
                        app.detail = crate::app::Detail::Devices;
                    }
                }
            }
        });

        section(ui, "EFECTOS MIDI", |ui| {
            let track = match app.selected_target {
                Target::Track(t) if !app.project.tracks[t].is_audio() && !app.project.tracks[t].is_group() => Some(t),
                _ => None,
            };
            let hint = if track.is_some() { "Clic: lo pone antes del instrumento de la pista seleccionada" } else { "Selecciona una pista MIDI" };
            for effect in engine::model::MidiEffect::catalog() {
                if item(ui, "♪", effect.label(), hint) {
                    if let Some(t) = track {
                        app.add_midi_effect(t, effect);
                        app.detail = crate::app::Detail::Devices;
                    }
                }
            }
        });

        section(ui, "EFECTOS DE AUDIO", |ui| {
            for effect in Effect::catalog() {
                if item(ui, "≈", effect.label(), "Clic: añade el efecto al final de la cadena seleccionada") {
                    app.add_device(target, Device::new(effect));
                }
            }
        });

        section(ui, "PLUGINS (VST3 · CLAP)", |ui| {
            crate::plugins::browser(app, ui, |ui, icon, label, hint| item(ui, icon, label, hint));
        });

        section(ui, "MUESTRAS", |ui| {
            if item(ui, "♪", "Añadir muestra…", "Elige archivos de audio (WAV, AIFF, FLAC, MP3…). También puedes arrastrarlos a la ventana") {
                app.choose_samples(crate::app::Placement::Auto);
            }
        });

        section(ui, "BIBLIOTECA", |ui| library(app, ui));

        section(ui, "PISTAS", |ui| {
            if item(ui, "+", "Nueva pista MIDI", &crate::theme::keys("⌘T")) {
                app.add_track();
            }
            if item(ui, "+", "Nueva pista de audio", &crate::theme::keys("⇧⌘T")) {
                app.add_audio_track();
            }
        });
    });
}

/// Pulse's folder and the user's places: a search over all of them, or their folders.
fn library(app: &mut PulseApp, ui: &mut egui::Ui) {
    ui.add(egui::TextEdit::singleline(&mut app.browser.search).hint_text("Buscar muestras, proyectos, presets").desired_width(f32::INFINITY));
    // A preview that has played to its end is over.
    if app.browser.previewing.as_ref().is_some_and(|(_, at, length)| at.elapsed().as_secs_f64() > *length) {
        app.browser.previewing = None;
    }
    let root = crate::library::root();
    let mut places: Vec<(String, PathBuf)> = [
        ("Samples", crate::library::SAMPLES),
        ("Proyectos", crate::library::PROJECTS),
        ("Exportaciones", crate::library::EXPORTS),
        ("Presets", crate::library::PRESETS),
    ]
    .into_iter()
    .map(|(name, folder)| (name.to_string(), root.join(folder)))
    .collect();
    places.extend(app.settings.browser_folders.iter().map(|f| (f.file_name().map_or(f.display().to_string(), |n| n.to_string_lossy().into_owned()), f.clone())));

    let query = app.browser.search.trim().to_string();
    if !query.is_empty() {
        let roots: Vec<PathBuf> = places.iter().map(|(_, p)| p.clone()).collect();
        let found = app.browser.search(&query, &roots);
        if found.is_empty() {
            ui.label(RichText::new("Nada con ese nombre").color(TEXT_DIM).size(10.5));
        }
        for path in found {
            file_row(app, ui, &path);
        }
        return;
    }
    let mut remove = None;
    for (i, (name, dir)) in places.iter().enumerate() {
        let own = i >= 4;
        let response = egui::CollapsingHeader::new(RichText::new(name).color(TEXT))
            .id_salt(("browser_place", dir))
            .show(ui, |ui| folder(app, ui, dir, 0))
            .header_response;
        let response = response.on_hover_text(dir.display().to_string());
        if own {
            response.context_menu(|ui| {
                if ui.button("Quitar del navegador").clicked() {
                    remove = Some(i - 4);
                    ui.close();
                }
            });
        }
    }
    if let Some(i) = remove {
        let mut next = app.settings.clone();
        next.browser_folders.remove(i);
        app.set_settings(next);
    }
    if item(ui, "+", "Añadir carpeta…", "Una carpeta tuya (de muestras, por ejemplo) en el navegador") {
        if let Some(dir) = crate::files::pick_folder(rfd::FileDialog::new()) {
            let mut next = app.settings.clone();
            if !next.browser_folders.contains(&dir) {
                next.browser_folders.push(dir);
            }
            app.set_settings(next);
        }
    }
}

/// A folder's contents: subfolders to open, files as rows.
fn folder(app: &mut PulseApp, ui: &mut egui::Ui, dir: &Path, depth: usize) {
    let entries = app.browser.list(dir);
    if entries.is_empty() {
        ui.label(RichText::new("Vacía").color(TEXT_DIM).size(10.5));
    }
    for entry in entries {
        if entry.dir {
            if depth < 12 {
                egui::CollapsingHeader::new(&entry.name).id_salt(("browser_dir", &entry.path)).show(ui, |ui| folder(app, ui, &entry.path, depth + 1));
            }
        } else {
            file_row(app, ui, &entry.path);
        }
    }
}

/// A file: click to hear it (audio), double click to use it, drag it where it goes.
fn file_row(app: &mut PulseApp, ui: &mut egui::Ui, path: &Path) {
    let Some(kind) = kind(path) else { return };
    let name = path.file_stem().map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let width = ui.available_width();
    let sense = if kind == Kind::Audio { Sense::click_and_drag() } else { Sense::click() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 20.0), sense);
    crate::widgets::name(&response, egui::WidgetType::Button, &name);
    let playing = app.browser.previewing() == Some(path);
    let selected = app.browser.selected.as_deref() == Some(path);
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, 2.0, if selected { theme::CONTROL_HOVER } else { theme::CONTROL });
    }
    let icon = match kind {
        Kind::Audio if playing => "▶",
        Kind::Audio => "♪",
        Kind::Project => "◆",
        Kind::Preset => "☰",
    };
    ui.painter().text(rect.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, icon, theme::small(), ACCENT);
    let mut job = egui::text::LayoutJob::simple_singleline(name.clone(), theme::body(), TEXT);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 26.0);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(pos2(rect.left() + 22.0, rect.center().y - galley.size().y / 2.0), galley, TEXT);
    let hint = match kind {
        Kind::Audio => "Clic: escuchar (otra vez: parar) · doble clic: a la pista de audio seleccionada · arrastrar: a un slot, un carril, un pad o el Sampler",
        Kind::Project => "Doble clic: abrir el proyecto",
        Kind::Preset => "Doble clic: aplicar el preset a la pista seleccionada",
    };
    let response = response.on_hover_text(format!("{}\n{hint}", path.display()));

    if response.drag_started() && kind == Kind::Audio {
        egui::DragAndDrop::set_payload(ui.ctx(), Dragged(path.to_path_buf()));
    }
    if response.double_clicked() {
        match kind {
            Kind::Audio => {
                app.preview(None);
                app.add_samples(vec![path.to_path_buf()], Placement::Auto);
            }
            Kind::Project => app.open_documents(vec![path.to_path_buf()]),
            Kind::Preset => app.apply_preset_file(path),
        }
    } else if response.clicked() {
        app.browser.selected = Some(path.to_path_buf());
        if kind == Kind::Audio {
            app.preview((!playing).then_some(path));
        }
    }
}

impl PulseApp {
    /// Hears a file over the output (or stops the one playing).
    pub fn preview(&mut self, path: Option<&Path>) {
        let sample = path.map(|p| self.sample(&p.to_string_lossy())).filter(|s| s.frames() > 0);
        self.browser.previewing = match (path, &sample) {
            (Some(path), Some(s)) => Some((path.to_path_buf(), Instant::now(), s.duration())),
            _ => None,
        };
        if path.is_some() && sample.is_none() {
            self.status = Some("No se pudo leer el archivo".into());
        }
        self.audio.send(engine::Command::PreviewSample(sample));
    }

    /// Applies a user preset file to the selected MIDI track.
    fn apply_preset_file(&mut self, path: &Path) {
        let preset = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<crate::presets::Preset>(&t).ok());
        match (preset, self.selected_target) {
            (Some(preset), Target::Track(t)) if !self.project.tracks[t].is_audio() => {
                self.apply_preset(t, preset);
                self.detail = crate::app::Detail::Devices;
            }
            (Some(_), _) => self.status = Some("Selecciona una pista MIDI para aplicarle el preset".into()),
            (None, _) => self.status = Some(format!("No se pudo leer el preset {}", path.display())),
        }
    }

    /// Drops what was dragged from the browser at `pos`: on a pad or the sampler,
    /// a session slot, an arrangement lane, or empty space in the view (a new
    /// audio track).
    pub fn drop_from_browser(&mut self, Dragged(path): Dragged, pos: egui::Pos2) {
        self.preview(None);
        self.drop_files(vec![path], pos);
    }

    /// Audio files dropped at `pos` (from the browser or Finder): on a pad or the
    /// sampler, a session slot, an arrangement lane, or empty space in the view (a
    /// new audio track). False if `pos` is none of these.
    pub fn drop_files(&mut self, paths: Vec<PathBuf>, pos: egui::Pos2) -> bool {
        if let Some(&(_, track, slot)) = self.sample_drop_zones.iter().find(|(rect, ..)| rect.contains(pos)) {
            self.set_instrument_samples(track, slot, paths);
            return true;
        }
        let targets = &self.drop_targets;
        let place = if let Some(&(track, slot, _)) = targets.slots.iter().find(|(.., r)| r.contains(pos)) {
            Some((track, Placement::Session { track, slot }))
        } else if let Some(&(track, _, left, ppb)) = targets.lanes.iter().find(|(_, r, ..)| r.contains(pos)) {
            let beat = (((pos.x - left) / ppb) as f64).max(0.0).floor();
            Some((track, Placement::Arrangement { track, start: beat }))
        } else {
            None
        };
        match place {
            Some((track, placement)) if self.project.tracks[track].is_audio() => {
                if let Placement::Session { slot, .. } = placement {
                    if self.project.tracks[track].clips[slot].is_some() {
                        self.status = Some("Ese slot ya tiene un clip".into());
                        return true;
                    }
                }
                self.add_samples(paths, placement);
                true
            }
            Some(_) => {
                self.status = Some("Suelta el audio en una pista de audio, o en un hueco vacío para crear una".into());
                true
            }
            None if targets.view.is_some_and(|v| v.contains(pos)) => {
                // Empty space: a new audio track.
                let lanes_end = targets.lanes.first().map(|&(_, _, left, ppb)| (((pos.x - left) / ppb) as f64).max(0.0).floor());
                if let Some(track) = self.add_audio_track() {
                    let placement = match (self.view, lanes_end) {
                        (crate::app::View::Arrangement, Some(start)) => Placement::Arrangement { track, start },
                        _ => Placement::Session { track, slot: 0 },
                    };
                    self.add_samples(paths, placement);
                }
                true
            }
            None => false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use eframe::egui::{pos2, Rect};

    #[test]
    fn a_dragged_file_lands_where_it_is_let_go() {
        let song = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo.wav");
        let mut app = PulseApp::headless(Project::demo());
        let audio = app.add_audio_track().unwrap();
        let slot = Rect::from_min_max(pos2(100.0, 100.0), pos2(200.0, 120.0));
        let lane = Rect::from_min_max(pos2(300.0, 300.0), pos2(900.0, 350.0));
        let set_targets = |app: &mut PulseApp| {
            app.drop_targets = DropTargets {
                slots: vec![(audio, 2, slot), (0, 2, slot.translate(vec2(0.0, 40.0)))],
                lanes: vec![(audio, lane, 300.0, 14.0)],
                view: Some(Rect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 800.0))),
            };
        };
        // On a session slot of the audio track.
        set_targets(&mut app);
        app.drop_from_browser(Dragged(song.clone()), slot.center());
        assert!(app.project.tracks[audio].clips[2].is_some());
        // On a MIDI track's slot: not there.
        set_targets(&mut app);
        app.drop_from_browser(Dragged(song.clone()), pos2(150.0, 150.0));
        assert!(app.project.tracks[0].clips[2].is_none());
        assert!(app.status.as_deref().unwrap_or("").contains("pista de audio"));
        // On the lane, at the beat under the pointer (14 px a beat: beat 8).
        set_targets(&mut app);
        app.drop_from_browser(Dragged(song.clone()), pos2(300.0 + 8.0 * 14.0 + 5.0, 320.0));
        assert_eq!(app.project.tracks[audio].arrangement[0].start, 8.0);
        // On empty space: a new audio track.
        let tracks = app.project.tracks.len();
        set_targets(&mut app);
        app.drop_from_browser(Dragged(song), pos2(950.0, 700.0));
        assert_eq!(app.project.tracks.len(), tracks + 1);
        assert!(app.project.tracks[tracks].is_audio());
    }

    #[test]
    fn the_search_finds_files_by_their_words() {
        let dir = std::env::temp_dir().join(format!("pulse-browser-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Baterías/Viejas")).unwrap();
        for f in ["Baterías/Bombo gordo.wav", "Baterías/Viejas/bombo SECO.aif", "notas.txt", "Mi tema.pulse"] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let mut state = State::default();
        let names = |found: Vec<PathBuf>| found.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names(state.search("bombo", &[dir.clone()])), vec!["Bombo gordo.wav", "bombo SECO.aif"]);
        assert_eq!(names(state.search("seco bombo", &[dir.clone()])), vec!["bombo SECO.aif"]);
        assert_eq!(names(state.search("tema", &[dir.clone()])), vec!["Mi tema.pulse"]);
        assert!(state.search("notas", &[dir.clone()]).is_empty(), "only files Pulse uses");
        // Folders first, hidden files left out.
        let listed: Vec<String> = read(&dir).into_iter().map(|e| e.name).collect();
        assert_eq!(listed, vec!["Baterías", "Mi tema.pulse"]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
