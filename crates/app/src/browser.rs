//! Left-hand browser: a search over everything below (see `search`), the
//! user's favourites, instruments and effects to add to the selected track,
//! effect chains (see `chains`), and Pulse's folder (see `library`) with the
//! user's own places: samples to hear and drag where they go, projects to
//! open, presets to apply.

use crate::app::{Placement, PulseApp};
use crate::i18n::{fill, tr};
use crate::search::{Favorites, Item, What};
use crate::theme::{self, ACCENT, TEXT, TEXT_DIM};
use eframe::egui::{self, pos2, vec2, Align2, RichText, Sense};
use engine::model::{Device, Effect, InstrumentKind};
use engine::Target;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What the browser keeps between frames.
#[derive(Default)]
pub struct State {
    /// What is searched for, in the field at the top.
    pub search: String,
    /// ⌘F: the field takes the keys on the next frame.
    pub focus_search: bool,
    /// The file being heard, since when, and for how long (for ever: a loop).
    previewing: Option<(PathBuf, Instant, f64)>,
    /// The tempo it is heard at, a loop in time with the project's.
    pub in_tempo: Option<f64>,
    /// A file whose tempo is being found before it is heard.
    analysing: Option<(PathBuf, Arc<Mutex<Option<Option<beats::Grid>>>>)>,
    /// The files shown this frame, in order (for ↑ and ↓).
    shown: Vec<PathBuf>,
    /// The last click was in the browser: ↑ and ↓ move through its files.
    pub(crate) keys: bool,
    /// The selection moved with the keys: shown in the list.
    scroll: bool,
    /// The row clicked last.
    selected: Option<PathBuf>,
    /// Folder listings, refreshed every couple of seconds while shown.
    listings: HashMap<PathBuf, (Instant, Vec<Entry>)>,
    /// Starred entries, read the first time they are needed.
    favorites: Option<Favorites>,
    /// The search's index: what isn't a file, and the files, each with when it was made.
    catalog: Option<(Instant, Arc<Vec<Item>>)>,
    files: Option<(Instant, Arc<Vec<Item>>)>,
    /// Bumped when either changes (the results are found again).
    generation: u64,
    /// The results shown, in order, and the search and index they are for.
    pub(crate) results: Vec<Item>,
    results_for: Option<(String, u64)>,
    /// The result chosen (↑ ↓, a click), among `results`.
    pub result: usize,
}

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    dir: bool,
}

/// Files up to this long are heard as loops at the project's tempo.
const LOOP_SECONDS: f64 = 32.0;

/// Results shown a group at most.
const PER_GROUP: usize = 40;

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
    /// Let go with ⌘ or ⌥ held: on the Arrangement, exactly there, off the grid.
    pub free: bool,
}

impl DropTargets {
    /// The beat on the Arrangement at `x` on a timeline whose left edge is at
    /// `left`, `ppb` pixels a beat: on the grid it shows (by the zoom), or
    /// with [`Self::free`], exactly there.
    fn beat(&self, x: f32, left: f32, ppb: f32, meter: &engine::meter::Meter) -> f64 {
        let beat = (((x - left) / ppb) as f64).max(0.0);
        if self.free {
            beat
        } else {
            crate::arrangement::snap_unit(ppb, meter).snap(beat).max(0.0)
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Audio,
    Midi,
    Project,
    Preset,
}

fn kind(path: &Path) -> Option<Kind> {
    if crate::app::is_audio_file(path) {
        return Some(Kind::Audio);
    }
    if crate::midi_file::is_midi(path) {
        return Some(Kind::Midi);
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

    pub fn previewing(&self) -> Option<&Path> {
        self.previewing.as_ref().map(|(p, ..)| p.as_path())
    }

    /// The search's index is made again on its next use (a chain or preset saved).
    pub fn forget_index(&mut self) {
        self.catalog = None;
    }

    pub fn favorites(&mut self) -> &mut Favorites {
        self.favorites.get_or_insert_with(|| Favorites::load_from(&crate::search::favorites_path()))
    }

    /// Stars or unstars an entry, and keeps it for the next time.
    pub fn toggle_favorite(&mut self, key: &str) -> bool {
        let on = self.favorites().toggle(key);
        self.favorites().save_to(&crate::search::favorites_path());
        on
    }
}

/// The favourites as they are this frame, and the stars clicked in it.
struct Favs {
    keys: Vec<String>,
    toggled: RefCell<Vec<String>>,
}

impl Favs {
    fn has(&self, key: &str) -> bool {
        self.keys.iter().any(|k| k == key)
    }

    /// The row's right-click menu: star or unstar it, and `more`.
    fn menu(&self, response: &egui::Response, key: &str, more: impl FnOnce(&mut egui::Ui)) {
        response.context_menu(|ui| {
            let label = if self.has(key) { tr("Quitar de favoritos") } else { tr("Añadir a favoritos") };
            if ui.button(label).clicked() {
                self.toggled.borrow_mut().push(key.to_string());
                ui.close();
            }
            more(ui);
        });
    }

    /// The star at the row's right: full if starred, hollow under the pointer;
    /// a click on it stars or unstars. Returns the width it takes.
    fn star(&self, ui: &mut egui::Ui, row: &egui::Response, key: &str) -> f32 {
        let rect = egui::Rect::from_center_size(pos2(row.rect.right() - 9.0, row.rect.center().y), vec2(16.0, row.rect.height()));
        let star = ui.interact(rect, row.id.with("star"), Sense::click());
        let on = self.has(key);
        if on || row.contains_pointer() {
            let color = if on { ACCENT } else if star.hovered() { TEXT } else { TEXT_DIM };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, if on { "★" } else { "☆" }, theme::small(), color);
        }
        let tip = if on { tr("Quitar de favoritos") } else { tr("Añadir a favoritos") };
        if star.on_hover_text(tip).clicked() {
            self.toggled.borrow_mut().push(key.to_string());
        }
        18.0
    }
}

/// A section that folds away (egui remembers which are open).
fn section(ui: &mut egui::Ui, title: &str, contents: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(4.0);
    egui::CollapsingHeader::new(RichText::new(title).color(TEXT_DIM).size(10.5).strong()).id_salt(("browser_section", title)).default_open(true).show_unindented(ui, contents);
}

/// A row's icon and name (cut with "…" at `right`; the tooltip has the whole
/// name), and `detail` after it, dimmer. Whether the name was cut.
fn paint_row(ui: &egui::Ui, rect: egui::Rect, icon: &str, label: &str, detail: &str, right: f32) -> bool {
    ui.painter().text(rect.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, icon, theme::small(), ACCENT);
    let mut job = egui::text::LayoutJob::default();
    job.append(label, 0.0, egui::TextFormat::simple(theme::body(), TEXT));
    if !detail.is_empty() {
        job.append(detail, 6.0, egui::TextFormat::simple(theme::small(), TEXT_DIM));
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 26.0 - right);
    let galley = ui.painter().layout_job(job);
    let cut = galley.elided;
    ui.painter().galley(pos2(rect.left() + 22.0, rect.center().y - galley.size().y / 2.0), galley, TEXT);
    cut
}

/// A full-width list row with its star; returns true when clicked.
fn item(ui: &mut egui::Ui, favs: &Favs, key: &str, icon: &str, label: &str, hint: &str) -> bool {
    item_with(ui, favs, key, icon, label, hint, |_| {}).clicked()
}

/// [`item`], with `more` in its right-click menu.
fn item_with(ui: &mut egui::Ui, favs: &Favs, key: &str, icon: &str, label: &str, hint: &str, more: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 20.0), Sense::click());
    crate::widgets::name(&response, egui::WidgetType::Button, label);
    if response.hovered() {
        ui.painter().rect_filled(rect, 2.0, theme::CONTROL);
    }
    let star = favs.star(ui, &response, key);
    let cut = paint_row(ui, rect, icon, label, "", star);
    let hint = if cut { format!("{label}\n{hint}") } else { hint.to_string() };
    favs.menu(&response, key, more);
    response.on_hover_text(hint)
}

/// The field keeps the keys on Esc (it clears the search first) and the arrows.
const KEEPS_ESCAPE: egui::EventFilter = egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: true };

/// The search field at the top. With a search, ↑ ↓ choose among the results,
/// Enter puts the one chosen where it goes (⌥Enter: a chain instead of the
/// effects there) and Esc clears it.
fn search_field(app: &mut PulseApp, ui: &mut egui::Ui) {
    let id = egui::Id::new("browser_search");
    let focused = ui.memory(|m| m.has_focus(id));
    let searching = !app.browser.search.trim().is_empty();
    let mut step = 0isize;
    let mut go = None;
    let mut clear = false;
    if focused || (searching && app.browser.keys) {
        ui.input_mut(|i| {
            if searching {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    step = 1;
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    step = -1;
                }
                if i.consume_key(egui::Modifiers::ALT, egui::Key::Enter) {
                    go = Some(true);
                } else if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                    go = Some(false);
                }
            }
            clear = i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
        });
    }
    if clear {
        if searching {
            app.browser.search.clear();
            app.browser.result = 0;
        } else {
            ui.memory_mut(|m| m.surrender_focus(id));
        }
    }
    let hint = fill(tr("Buscar  {}"), &[&theme::keys("⌘F")]);
    let response = ui.add(egui::TextEdit::singleline(&mut app.browser.search).id(id).hint_text(hint).desired_width(f32::INFINITY).event_filter(KEEPS_ESCAPE));
    crate::widgets::name(&response, egui::WidgetType::TextEdit, "Buscar en el navegador");
    let response = response.on_hover_text(tr("Instrumentos, efectos, presets, cadenas, sonidos, muestras y plugins. ↑ ↓ eligen, Enter lo pone en el destino, Esc borra"));
    if std::mem::take(&mut app.browser.focus_search) {
        response.request_focus();
    }
    if response.changed() {
        app.browser.result = 0;
        app.browser.scroll = true;
    }
    if step != 0 && !app.browser.results.is_empty() {
        let last = app.browser.results.len() as isize - 1;
        app.browser.result = (app.browser.result as isize + step).clamp(0, last) as usize;
        app.browser.scroll = true;
        if let Some(Item { what: What::File(path), .. }) = app.browser.results.get(app.browser.result).cloned() {
            if kind(&path) == Some(Kind::Audio) {
                app.preview(Some(&path));
            }
        }
    }
    if let Some(alt) = go {
        if let Some(item) = app.browser.results.get(app.browser.result).cloned() {
            activate(app, &item.what, alt);
            // ⌘Z undoes it (in the field it would undo the typing); the arrows
            // and Enter still go through the results.
            ui.memory_mut(|m| m.surrender_focus(id));
            app.browser.keys = true;
        }
    }
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let panel = ui.max_rect();
    app.browser.shown.clear();
    ui.label(RichText::new(tr("Navegador")).strong());
    let target = app.selected_target;
    ui.label(
        RichText::new(fill(tr("Destino: {}"), &[&app.target_name(target)]))
            .color(TEXT_DIM)
            .size(10.5),
    );
    search_field(app, ui);
    ui.separator();
    let favs = Favs { keys: app.browser.favorites().keys.clone(), toggled: RefCell::new(Vec::new()) };
    let searching = !app.browser.search.trim().is_empty();

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        if searching {
            results(app, ui, &favs);
            return;
        }
        if !favs.keys.is_empty() {
            section(ui, tr("FAVORITOS"), |ui| favorites(app, ui, &favs));
        }
        section(ui, tr("INSTRUMENTOS"), |ui| {
            let track = match target {
                Target::Track(t) if !app.project.tracks[t].is_audio() => Some(t),
                _ => None,
            };
            let hint = if track.is_some() {
                tr("Clic: reemplaza el instrumento de la pista seleccionada")
            } else {
                tr("Selecciona una pista MIDI para cambiar su instrumento")
            };
            for kind in InstrumentKind::catalog() {
                let icon = if kind.is_drums() { "◉" } else { "♫" };
                if item(ui, &favs, &format!("instrumento:{}", kind.label()), icon, kind.label(), hint) {
                    activate(app, &What::Instrument(kind), false);
                }
            }
        });

        section(ui, tr("EFECTOS MIDI"), |ui| {
            let track = match app.selected_target {
                Target::Track(t) if !app.project.tracks[t].is_audio() && !app.project.tracks[t].is_group() => Some(t),
                _ => None,
            };
            let hint = if track.is_some() { tr("Clic: lo pone antes del instrumento de la pista seleccionada") } else { tr("Selecciona una pista MIDI") };
            for effect in engine::model::MidiEffect::catalog() {
                if item(ui, &favs, &format!("efecto-midi:{}", effect.label()), "♪", tr(effect.label()), hint) && track.is_some() {
                    activate(app, &What::MidiEffect(effect), false);
                }
            }
        });

        section(ui, tr("EFECTOS DE AUDIO"), |ui| {
            for effect in Effect::catalog() {
                if item(ui, &favs, &format!("efecto:{}", effect.label()), "≈", tr(effect.label()), tr("Clic: añade el efecto al final de la cadena seleccionada")) {
                    activate(app, &What::Effect(effect), false);
                }
            }
        });

        section(ui, tr("CADENAS"), |ui| chains(app, ui, &favs));

        section(ui, "PLUGINS (VST3 · CLAP)", |ui| {
            crate::plugins::browser(app, ui, |ui, key, icon, label, hint| item(ui, &favs, key, icon, label, hint));
        });

        section(ui, tr("MUESTRAS"), |ui| {
            if item(ui, &favs, "muestras:añadir", "♪", tr("Añadir muestra…"), tr("Elige archivos de audio (WAV, AIFF, FLAC, MP3…). También puedes arrastrarlos a la ventana")) {
                app.choose_samples(crate::app::Placement::Auto);
            }
        });

        section(ui, tr("BIBLIOTECA"), |ui| library(app, ui, &favs));

        section(ui, tr("PISTAS"), |ui| {
            if item(ui, &favs, "pistas:midi", "+", tr("Nueva pista MIDI"), &crate::theme::keys("⌘T")) {
                app.add_track();
            }
            if item(ui, &favs, "pistas:audio", "+", tr("Nueva pista de audio"), &crate::theme::keys("⇧⌘T")) {
                app.add_audio_track();
            }
        });
    });
    for key in favs.toggled.take() {
        let on = app.browser.toggle_favorite(&key);
        app.status = Some(if on { tr("Añadido a favoritos").into() } else { tr("Quitado de favoritos").into() });
    }
    if searching {
        if ui.ctx().input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !panel.contains(p))) {
            app.browser.keys = false;
        }
    } else {
        keys(app, ui, panel);
    }
}

/// The chains: Pulse's and the user's. A click adds one's effects after the
/// target's; with ⌥ they replace them.
fn chains(app: &mut PulseApp, ui: &mut egui::Ui, favs: &Favs) {
    // From the search's index: the folder isn't read every frame.
    let (catalog, _) = app.search_index(false);
    let mut delete = None;
    for found in catalog.iter() {
        let What::Chain(chain) = &found.what else { continue };
        let key = &found.key;
        let hint = fill(tr("{}\nClic: añade sus efectos al final de la cadena del destino · ⌥ clic: sustituye los que hay"), &[&chain.summary()]);
        let response = item_with(ui, favs, key, "⛓", &chain.name, &hint, |ui| {
            if !chain.factory && ui.button(tr("Borrar la cadena")).clicked() {
                delete = Some(chain.name.clone());
                ui.close();
            }
        });
        if response.clicked() {
            let alt = ui.input(|i| i.modifiers.alt);
            app.load_chain(app.selected_target, chain, alt);
        }
    }
    if let Some(name) = delete {
        app.delete_chain(&name);
    }
    ui.label(RichText::new(tr("Guarda las tuyas en Dispositivos › Guardar cadena…")).color(TEXT_DIM).size(10.0));
}

/// The starred entries, in the order they were starred. They load with a click
/// (files: heard with a click, used with a double click, as in their folder).
fn favorites(app: &mut PulseApp, ui: &mut egui::Ui, favs: &Favs) {
    let (catalog, _) = app.search_index(false);
    for key in &favs.keys {
        if let Some(path) = key.strip_prefix("archivo:") {
            let path = PathBuf::from(path);
            if path.exists() {
                file_row(app, ui, &path, favs);
            }
            continue;
        }
        let Some(found) = catalog.iter().find(|i| &i.key == key) else { continue };
        let found = found.clone();
        if result_row(app, ui, &found, false, favs).0 {
            let alt = ui.input(|i| i.modifiers.alt);
            activate(app, &found.what, alt);
        }
    }
}

/// The results of the search, by group, the best first.
fn results(app: &mut PulseApp, ui: &mut egui::Ui, favs: &Favs) {
    let (catalog, files) = app.search_index(true);
    let query = app.browser.search.trim().to_string();
    let wanted = (query.clone(), app.browser.generation);
    if app.browser.results_for.as_ref() != Some(&wanted) {
        let groups = crate::search::find(&[&catalog, &files], &query, PER_GROUP);
        app.browser.results = groups.into_iter().flat_map(|(_, g)| g.into_iter().cloned()).collect();
        app.browser.results_for = Some(wanted);
        app.browser.result = app.browser.result.min(app.browser.results.len().saturating_sub(1));
    }
    if app.browser.results.is_empty() {
        ui.label(RichText::new(tr("Nada con ese nombre")).color(TEXT_DIM).size(10.5));
        return;
    }
    let shown = app.browser.results.clone();
    let mut group = None;
    let mut activated = None;
    for (i, found) in shown.iter().enumerate() {
        if group != Some(found.group) {
            group = Some(found.group);
            let count = shown.iter().filter(|r| r.group == found.group).count();
            ui.add_space(if i == 0 { 2.0 } else { 6.0 });
            ui.label(RichText::new(format!("{}  ({count})", found.group.label())).color(TEXT_DIM).size(10.0).strong());
        }
        let selected = app.browser.result == i;
        let (clicked, double) = result_row(app, ui, found, selected, favs);
        if clicked {
            app.browser.result = i;
            app.browser.keys = true;
        }
        if double {
            activated = Some((found.what.clone(), ui.input(|i| i.modifiers.alt)));
        }
    }
    if let Some((what, alt)) = activated {
        activate(app, &what, alt);
    }
}

/// A result: chosen with a click (a sample is heard), put where it goes with
/// a double click, dragged like a file of the list. Whether it was clicked,
/// and whether double clicked.
fn result_row(app: &mut PulseApp, ui: &mut egui::Ui, found: &Item, selected: bool, favs: &Favs) -> (bool, bool) {
    let file = match &found.what {
        What::File(path) => kind(path).map(|k| (path.clone(), k)),
        _ => None,
    };
    let width = ui.available_width();
    let sense = if file.as_ref().is_some_and(|(_, k)| matches!(k, Kind::Audio | Kind::Midi | Kind::Preset)) { Sense::click_and_drag() } else { Sense::click() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 20.0), sense);
    crate::widgets::name(&response, egui::WidgetType::Button, &found.name);
    if selected && std::mem::take(&mut app.browser.scroll) {
        response.scroll_to_me(None);
    }
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, 2.0, if selected { theme::CONTROL_HOVER } else { theme::CONTROL });
    }
    let playing = file.as_ref().is_some_and(|(p, _)| app.browser.previewing() == Some(p.as_path()));
    let icon = match (&found.what, &file) {
        (_, Some((_, Kind::Audio))) if playing => "►",
        (_, Some((_, Kind::Audio))) => "♪",
        (_, Some((_, Kind::Midi))) => "♫",
        (_, Some((_, Kind::Project))) => "◆",
        (_, Some((_, Kind::Preset))) => "☰",
        (What::Instrument(k), _) if k.is_drums() => "◉",
        (What::Instrument(_), _) | (What::Plugin(_), _) => "♫",
        (What::MidiEffect(_), _) => "♪",
        (What::Effect(_), _) => "≈",
        (What::Chain(_), _) => "⛓",
        (What::Kit(_), _) => "◉",
        _ => "☰",
    };
    let star = favs.star(ui, &response, &found.key);
    paint_row(ui, rect, icon, &found.name, &found.detail, star);
    let what = match &found.what {
        What::File(path) => path.display().to_string(),
        What::Chain(chain) => chain.summary(),
        _ => found.detail.clone(),
    };
    let hint = match &found.what {
        What::File(_) => tr("Clic: escuchar · doble clic o Enter: a la pista seleccionada · arrastrar: a un slot, un carril, un pad o el Sampler"),
        What::Chain(_) => tr("Doble clic o Enter: añade sus efectos al destino · con ⌥: sustituye los que hay"),
        What::Instrument(_) | What::Preset(_) | What::Kit(_) => tr("Doble clic o Enter: en la pista MIDI seleccionada"),
        _ => tr("Doble clic o Enter: al final de la cadena del destino"),
    };
    favs.menu(&response, &found.key, |_| {});
    let response = response.on_hover_text(format!("{}\n{what}\n{hint}", found.name));
    if let Some((path, k)) = &file {
        if response.drag_started() && matches!(k, Kind::Audio | Kind::Midi | Kind::Preset) {
            egui::DragAndDrop::set_payload(ui.ctx(), Dragged(path.clone()));
        }
        if response.clicked() && *k == Kind::Audio && !response.double_clicked() {
            app.preview((!playing).then_some(path.as_path()));
        }
    }
    let double = response.double_clicked();
    (response.clicked() || double, double)
}

/// Puts what an entry is where it goes: on the selected track or chain (an
/// instrument or a preset on a MIDI track). With `alt`, a chain replaces the
/// effects there.
pub fn activate(app: &mut PulseApp, what: &What, alt: bool) {
    let target = app.selected_target;
    let midi = match target {
        Target::Track(t) if !app.project.tracks[t].is_audio() && !app.project.tracks[t].is_group() => Some(t),
        _ => None,
    };
    let instrument_track = match target {
        Target::Track(t) if !app.project.tracks[t].is_audio() => Some(t),
        _ => None,
    };
    let need_midi = |app: &mut PulseApp| app.status = Some(tr("Selecciona una pista MIDI").into());
    match what {
        What::Instrument(kind) => match instrument_track {
            Some(t) => {
                // An empty rack would be silent: start it with the basic kit.
                if matches!(kind, InstrumentKind::DrumRack(_)) && app.project.tracks[t].samples.is_empty() {
                    app.load_basic_kit(t);
                } else {
                    app.set_instrument(t, *kind);
                }
                app.detail = crate::app::Detail::Devices;
            }
            None => need_midi(app),
        },
        What::MidiEffect(effect) => match midi {
            Some(t) => {
                app.add_midi_effect(t, *effect);
                app.detail = crate::app::Detail::Devices;
            }
            None => need_midi(app),
        },
        What::Effect(effect) => app.add_device(target, Device::new(crate::returns::for_target(*effect, target))),
        What::EffectPreset(effect) => app.add_device(target, Device::new(*effect)),
        What::Preset(preset) => match midi {
            Some(t) => {
                app.apply_preset(t, (**preset).clone());
                app.detail = crate::app::Detail::Devices;
            }
            None => need_midi(app),
        },
        What::Chain(chain) => app.load_chain(target, chain, alt),
        What::Kit(kit) => match midi {
            Some(t) => {
                app.load_factory_kit(t, *kit);
                app.detail = crate::app::Detail::Devices;
            }
            None => need_midi(app),
        },
        What::File(path) => match kind(path) {
            Some(Kind::Audio) => {
                app.preview(None);
                app.add_samples(vec![path.clone()], Placement::Auto);
            }
            Some(Kind::Midi) => {
                app.import_midi(path, None);
            }
            Some(Kind::Project) => app.open_documents(vec![path.clone()]),
            Some(Kind::Preset) => app.apply_preset_file(path),
            None => {}
        },
        What::Plugin(info) => {
            if info.instrument {
                match midi {
                    Some(t) => app.set_plugin_instrument(t, info),
                    None => need_midi(app),
                }
            } else {
                app.add_plugin_effect(target, info);
            }
        }
    }
}

/// ↑ and ↓ after a click in the browser: the file before or after the one
/// selected, heard if it is audio. A click elsewhere gives the keys back.
fn keys(app: &mut PulseApp, ui: &egui::Ui, panel: egui::Rect) {
    let ctx = ui.ctx();
    if ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !panel.contains(p))) {
        app.browser.keys = false;
    }
    if !app.browser.keys || ctx.memory(|m| m.focused().is_some()) {
        return;
    }
    let (up, down) = ctx.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)));
    let step: isize = if down { 1 } else if up { -1 } else { return };
    if let Some(next) = app.browser.next_file(step) {
        app.browser.selected = Some(next.clone());
        app.browser.scroll = true;
        if kind(&next) == Some(Kind::Audio) {
            app.preview(Some(&next));
        }
    }
}

impl State {
    /// The file `step` rows from the one selected among those shown (the first
    /// or last if none is).
    fn next_file(&self, step: isize) -> Option<PathBuf> {
        let at = self.selected.as_ref().and_then(|s| self.shown.iter().position(|p| p == s));
        let index = match at {
            Some(i) => (i as isize + step).clamp(0, self.shown.len() as isize - 1) as usize,
            None if step > 0 => 0,
            None => self.shown.len().checked_sub(1)?,
        };
        self.shown.get(index).cloned()
    }
}

/// Pulse's folder, «Sonidos de Pulse» and the user's places, by name.
fn places(app: &PulseApp) -> Vec<(String, PathBuf)> {
    let root = crate::library::root();
    let mut places: Vec<(String, PathBuf)> = [
        ("Samples", crate::library::SAMPLES),
        (tr("Proyectos"), crate::library::PROJECTS),
        (tr("Exportaciones"), crate::library::EXPORTS),
        ("Presets", crate::library::PRESETS),
    ]
    .into_iter()
    .map(|(name, folder)| (name.to_string(), root.join(folder)))
    .collect();
    // Pulse's own sounds, made the first time they are opened.
    places.insert(1, (crate::factory_sounds::NAME.to_string(), crate::factory_sounds::root()));
    places.extend(app.settings.browser_folders.iter().map(|f| (f.file_name().map_or(f.display().to_string(), |n| n.to_string_lossy().into_owned()), f.clone())));
    places
}

/// How many of the places are Pulse's own (the rest are the user's).
const BUILTIN_PLACES: usize = 5;

impl PulseApp {
    /// The search's index (made again every few seconds: presets saved, files
    /// added). Without `with_files`, the files are left as they are.
    fn search_index(&mut self, with_files: bool) -> (Arc<Vec<Item>>, Arc<Vec<Item>>) {
        let fresh = |made: &Option<(Instant, Arc<Vec<Item>>)>, seconds: u64| made.as_ref().is_some_and(|(at, _)| at.elapsed() < Duration::from_secs(seconds));
        let factory = crate::factory_sounds::root();
        if !fresh(&self.browser.catalog, 3) {
            let mut chains = crate::chains::factory();
            chains.extend(crate::chains::list(&crate::chains::user_root()));
            let presets = crate::presets::user_root();
            let sources = crate::search::Sources { presets: &presets, chains, plugins: &self.plugin_library.found, factory: &factory };
            self.browser.catalog = Some((Instant::now(), Arc::new(crate::search::catalog(&sources))));
            self.browser.generation += 1;
        }
        if with_files && !fresh(&self.browser.files, 10) {
            let roots: Vec<PathBuf> = places(self).into_iter().map(|(_, p)| p).collect();
            // The user's presets and chains are in the index already.
            let presets = crate::library::root().join(crate::library::PRESETS);
            let paths = crate::search::walk(&roots, 50_000, |p| kind(p).is_some() && !(p.starts_with(&presets) && kind(p) == Some(Kind::Preset)));
            let items = paths.iter().map(|p| crate::search::file_item(p, &factory)).collect();
            self.browser.files = Some((Instant::now(), Arc::new(items)));
            self.browser.generation += 1;
        }
        let catalog = self.browser.catalog.as_ref().map(|(_, c)| c.clone()).unwrap_or_default();
        let files = self.browser.files.as_ref().map(|(_, f)| f.clone()).unwrap_or_default();
        (catalog, files)
    }
}

/// Pulse's folder and the user's places, as folders.
fn library(app: &mut PulseApp, ui: &mut egui::Ui, favs: &Favs) {
    ui.horizontal(|ui| {
        let mut on = app.settings.preview_in_tempo;
        let response = ui.checkbox(&mut on, RichText::new(tr("A tempo")).size(10.5)).on_hover_text(tr("Los loops se escuchan al tempo del proyecto, en bucle y, si la canción suena, a tiempo con ella. Sin marcar, cada archivo suena una vez a su velocidad"));
        if response.changed() {
            let mut next = app.settings.clone();
            next.preview_in_tempo = on;
            app.set_settings(next);
            if let Some(path) = app.browser.previewing().map(Path::to_path_buf) {
                app.preview(Some(&path));
            }
        }
        if let Some(bpm) = app.browser.in_tempo {
            ui.label(RichText::new(fill(tr("loop a {} BPM"), &[&crate::tempo_ui::bpm_text(bpm)])).color(TEXT_DIM).size(10.5));
        }
    });
    app.hear_analysed();
    // A preview that has played to its end is over.
    if app.browser.previewing.as_ref().is_some_and(|(_, at, length)| at.elapsed().as_secs_f64() > *length) {
        app.browser.previewing = None;
    }
    if app.browser.analysing.is_some() {
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }
    let factory = crate::factory_sounds::root();
    let places = places(app);
    let mut remove = None;
    for (i, (name, dir)) in places.iter().enumerate() {
        let own = i >= BUILTIN_PLACES;
        let response = egui::CollapsingHeader::new(RichText::new(name).color(TEXT))
            .id_salt(("browser_place", dir))
            .show(ui, |ui| {
                if *dir == factory {
                    factory_status(app, ui);
                }
                folder(app, ui, dir, 0, favs)
            })
            .header_response;
        let response = response.on_hover_text(dir.display().to_string());
        if own {
            response.context_menu(|ui| {
                if ui.button(tr("Quitar del navegador")).clicked() {
                    remove = Some(i - BUILTIN_PLACES);
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
    if item(ui, favs, "biblioteca:añadir-carpeta", "+", tr("Añadir carpeta…"), tr("Una carpeta tuya (de muestras, por ejemplo) en el navegador")) {
        if let Some(dir) = crate::files::pick_folder(rfd::FileDialog::new()) {
            let mut next = app.settings.clone();
            if !next.browser_folders.contains(&dir) {
                next.browser_folders.push(dir);
            }
            app.set_settings(next);
        }
    }
}

/// «Sonidos de Pulse» opened: made if it isn't, with how far that has got.
fn factory_status(app: &mut PulseApp, ui: &mut egui::Ui) {
    app.make_factory_sounds();
    if let Some((done, of)) = app.factory.progress() {
        ui.add(egui::ProgressBar::new(done as f32 / of.max(1) as f32).text(RichText::new(fill(tr("Pulse está creando sus sonidos… {} de {}"), &[&done, &of])).size(10.5)));
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }
    if let Some(error) = &app.factory.error {
        ui.label(RichText::new(fill(tr("No se pudieron crear todos los sonidos: {}"), &[error])).color(TEXT_DIM).size(10.5));
    }
}

/// A folder's contents: subfolders to open, files as rows.
fn folder(app: &mut PulseApp, ui: &mut egui::Ui, dir: &Path, depth: usize, favs: &Favs) {
    let entries = app.browser.list(dir);
    if entries.is_empty() {
        ui.label(RichText::new(tr("Vacía")).color(TEXT_DIM).size(10.5));
    }
    for entry in entries {
        if entry.dir {
            if depth < 12 {
                egui::CollapsingHeader::new(&entry.name).id_salt(("browser_dir", &entry.path)).show(ui, |ui| folder(app, ui, &entry.path, depth + 1, favs));
            }
        } else {
            file_row(app, ui, &entry.path, favs);
        }
    }
}

/// A file: click to hear it (audio), double click to use it, drag it where it goes.
fn file_row(app: &mut PulseApp, ui: &mut egui::Ui, path: &Path, favs: &Favs) {
    let Some(kind) = kind(path) else { return };
    let name = path.file_stem().map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let width = ui.available_width();
    let sense = if matches!(kind, Kind::Audio | Kind::Midi | Kind::Preset) { Sense::click_and_drag() } else { Sense::click() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 20.0), sense);
    crate::widgets::name(&response, egui::WidgetType::Button, &name);
    app.browser.shown.push(path.to_path_buf());
    let playing = app.browser.previewing() == Some(path);
    let selected = app.browser.selected.as_deref() == Some(path);
    if selected && std::mem::take(&mut app.browser.scroll) {
        response.scroll_to_me(None);
    }
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, 2.0, if selected { theme::CONTROL_HOVER } else { theme::CONTROL });
    }
    let icon = match kind {
        Kind::Audio if playing => "►",
        Kind::Audio => "♪",
        Kind::Midi => "♫",
        Kind::Project => "◆",
        Kind::Preset => "☰",
    };
    let key = format!("archivo:{}", path.display());
    let star = favs.star(ui, &response, &key);
    paint_row(ui, rect, icon, &name, "", star);
    let hint = match kind {
        Kind::Audio => tr("Clic: escuchar (otra vez: parar) · ↑ ↓: el anterior o el siguiente · doble clic: a la pista de audio seleccionada · arrastrar: a un slot, un carril, un pad o el Sampler"),
        Kind::Midi => tr("Doble clic: sus partes en pistas nuevas · arrastrar: a una pista MIDI (todo en un clip) o a un hueco vacío (pistas nuevas)"),
        Kind::Project => tr("Doble clic: abrir el proyecto"),
        Kind::Preset => tr("Doble clic: aplicar el preset, el kit o la cadena a la pista seleccionada · arrastrar: a una pista"),
    };
    favs.menu(&response, &key, |_| {});
    let response = response.on_hover_text(format!("{}\n{hint}", path.display()));

    if response.drag_started() && matches!(kind, Kind::Audio | Kind::Midi | Kind::Preset) {
        egui::DragAndDrop::set_payload(ui.ctx(), Dragged(path.to_path_buf()));
    }
    if response.double_clicked() {
        activate(app, &What::File(path.to_path_buf()), false);
    } else if response.clicked() {
        app.browser.selected = Some(path.to_path_buf());
        app.browser.keys = true;
        if kind == Kind::Audio {
            app.preview((!playing).then_some(path));
        }
    }
}

/// Whether a JSON file's text is an instrument preset's (else it may be a chain's).
fn is_preset(text: Option<&str>) -> bool {
    text.is_some_and(|t| serde_json::from_str::<crate::presets::Preset>(t).is_ok())
}

impl PulseApp {
    /// Hears a file over the output (or stops the one playing). With «A
    /// tempo», a loop plays at the project's tempo, looping, once its tempo is
    /// known (found in the background the first time: a moment).
    pub fn preview(&mut self, path: Option<&Path>) {
        self.browser.analysing = None;
        self.browser.in_tempo = None;
        let sample = path.map(|p| self.sample(&p.to_string_lossy())).filter(|s| s.frames() > 0);
        let (Some(path), Some(sample)) = (path, sample) else {
            if path.is_some() {
                self.status = Some(tr("No se pudo leer el archivo").into());
            }
            self.browser.previewing = None;
            self.audio.send(engine::Command::PreviewSample(None));
            return;
        };
        if self.settings.preview_in_tempo && sample.duration() <= LOOP_SECONDS {
            let key = path.to_string_lossy().into_owned();
            match self.tempo.grid(&key) {
                Some(grid) => {
                    if self.hear_in_tempo(path, &sample, grid) {
                        return;
                    }
                }
                None => {
                    // Silent until its tempo is known (see `hear_analysed`).
                    let result = Arc::new(Mutex::new(None));
                    let shared = result.clone();
                    let file = sample.clone();
                    std::thread::spawn(move || {
                        let grid = beats::analyze(&file.left, &file.right, file.sample_rate).ok().flatten();
                        if let Ok(mut slot) = shared.lock() {
                            *slot = Some(grid);
                        }
                    });
                    self.browser.analysing = Some((path.to_path_buf(), result));
                    self.browser.previewing = Some((path.to_path_buf(), Instant::now(), f64::INFINITY));
                    self.audio.send(engine::Command::PreviewSample(None));
                    return;
                }
            }
        }
        self.browser.previewing = Some((path.to_path_buf(), Instant::now(), sample.duration()));
        self.audio.send(engine::Command::PreviewSample(Some(sample)));
    }

    /// Once the tempo of the file about to be heard is found: heard.
    pub(crate) fn hear_analysed(&mut self) {
        let done = self.browser.analysing.as_ref().and_then(|(path, result)| Some((path.clone(), result.lock().ok()?.clone()?)));
        let Some((path, grid)) = done else { return };
        self.tempo.remember(&path.to_string_lossy(), grid);
        self.browser.analysing = None;
        if self.browser.previewing() == Some(path.as_path()) {
            self.preview(Some(&path));
        }
    }

    /// Plays a loop at the project's tempo, looping, as a clip of it would
    /// (whole bars, Beats for drums). Without a tempo found, one whose length
    /// is whole bars at a likely tempo is taken as that, as when importing it;
    /// false if it isn't a loop.
    fn hear_in_tempo(&mut self, path: &Path, sample: &Arc<engine::Sample>, grid: Option<beats::Grid>) -> bool {
        use engine::model::WarpMode;
        let duration = sample.duration();
        let name = path.file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned());
        let guess = self.new_audio_clip(path, sample, [0; 3]);
        let mut clip = match crate::tempo_ui::loop_grid(grid, &name, duration, self.project.tempo) {
            Some(grid) => crate::tempo_ui::warped(&guess, &grid, duration),
            None if guess.warp != WarpMode::Off => guess,
            None => return false,
        };
        if clip.warp == WarpMode::Stretch && crate::tempo_ui::is_drum_loop(&name, sample) {
            clip.warp = WarpMode::Beats;
        }
        self.browser.in_tempo = Some(clip.original_bpm);
        let playing = crate::sync::play_clip(&engine::model::Clip::Audio(clip), &mut self.samples);
        self.browser.previewing = Some((path.to_path_buf(), Instant::now(), f64::INFINITY));
        self.audio.send(engine::Command::PreviewClip(Some(Box::new(playing))));
        true
    }

    /// Applies a preset file (or a chain's) to the selected track: a preset
    /// to a MIDI one, a chain's effects to any.
    fn apply_preset_file(&mut self, path: &Path) {
        let text = std::fs::read_to_string(path).ok();
        if !is_preset(text.as_deref()) {
            if let Some(chain) = crate::chains::read(path) {
                self.load_chain(self.selected_target, &chain, false);
                return;
            }
        }
        let preset = text.and_then(|t| serde_json::from_str::<crate::presets::Preset>(&t).ok());
        match (preset, self.selected_target) {
            (Some(preset), Target::Track(t)) if !self.project.tracks[t].is_audio() => {
                self.apply_preset(t, preset);
                self.detail = crate::app::Detail::Devices;
            }
            (Some(_), _) => self.status = Some(tr("Selecciona una pista MIDI para aplicarle el preset").into()),
            (None, _) => self.status = Some(fill(tr("No se pudo leer el preset {}"), &[&path.display()])),
        }
    }

    /// Drops what was dragged from the browser at `pos`: on a pad or the sampler,
    /// a session slot, an arrangement lane, or empty space in the view (a new
    /// audio track).
    pub fn drop_from_browser(&mut self, Dragged(path): Dragged, pos: egui::Pos2) {
        self.preview(None);
        if kind(&path) == Some(Kind::Preset) {
            self.drop_preset(&path, pos);
            return;
        }
        self.drop_files(vec![path], pos);
    }

    /// A preset (a kit of «Sonidos de Pulse», say) let go on a MIDI track's
    /// slot or lane: applied to that track.
    fn drop_preset(&mut self, path: &Path, pos: egui::Pos2) {
        let targets = &self.drop_targets;
        let track = targets.slots.iter().find(|(.., r)| r.contains(pos)).map(|&(t, ..)| t).or_else(|| targets.lanes.iter().find(|(_, r, ..)| r.contains(pos)).map(|&(t, ..)| t));
        let Some(t) = track else { return };
        if !is_preset(std::fs::read_to_string(path).ok().as_deref()) {
            if let Some(chain) = crate::chains::read(path) {
                self.load_chain(Target::Track(t), &chain, false);
                return;
            }
        }
        if self.project.tracks[t].is_audio() || self.project.tracks[t].is_group() {
            self.status = Some(tr("Suelta el preset en una pista MIDI").into());
            return;
        }
        match std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<crate::presets::Preset>(&t).ok()) {
            Some(preset) => {
                self.apply_preset(t, preset);
                self.detail = crate::app::Detail::Devices;
            }
            None => self.status = Some(fill(tr("No se pudo leer el preset {}"), &[&path.display()])),
        }
    }

    /// Audio files dropped at `pos` (from the browser or Finder): on a pad or the
    /// sampler, a session slot, an arrangement lane, or empty space in the view (a
    /// new audio track). False if `pos` is none of these.
    pub fn drop_files(&mut self, paths: Vec<PathBuf>, pos: egui::Pos2) -> bool {
        let (midi, paths): (Vec<PathBuf>, Vec<PathBuf>) = paths.into_iter().partition(|p| crate::midi_file::is_midi(p));
        if !midi.is_empty() {
            let placed = self.drop_midi(midi, pos);
            if paths.is_empty() {
                return placed;
            }
        }
        if let Some(&(_, track, slot)) = self.sample_drop_zones.iter().find(|(rect, ..)| rect.contains(pos)) {
            self.set_instrument_samples(track, slot, paths);
            return true;
        }
        let targets = &self.drop_targets;
        let place = if let Some(&(track, slot, _)) = targets.slots.iter().find(|(.., r)| r.contains(pos)) {
            Some((track, Placement::Session { track, slot }))
        } else if let Some(&(track, _, left, ppb)) = targets.lanes.iter().find(|(_, r, ..)| r.contains(pos)) {
            Some((track, Placement::Arrangement { track, start: targets.beat(pos.x, left, ppb, &self.project.meter) }))
        } else {
            None
        };
        match place {
            Some((track, placement)) if self.project.tracks[track].is_audio() => {
                if let Placement::Session { slot, .. } = placement {
                    if self.project.tracks[track].clips[slot].is_some() {
                        self.status = Some(tr("Ese slot ya tiene un clip").into());
                        return true;
                    }
                }
                self.add_samples(paths, placement);
                true
            }
            Some(_) => {
                self.status = Some(tr("Suelta el audio en una pista de audio, o en un hueco vacío para crear una").into());
                true
            }
            None if targets.view.is_some_and(|v| v.contains(pos)) => {
                // Empty space: a new audio track.
                let lanes_end = targets.lanes.first().map(|&(_, _, left, ppb)| targets.beat(pos.x, left, ppb, &self.project.meter));
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

    /// MIDI files dropped at `pos`: on a MIDI track's empty slot or its lane,
    /// all the notes in a clip there; anywhere else, each part on a track of
    /// its own.
    pub fn drop_midi(&mut self, paths: Vec<PathBuf>, pos: egui::Pos2) -> bool {
        let targets = &self.drop_targets;
        let place = if let Some(&(track, slot, _)) = targets.slots.iter().find(|(.., r)| r.contains(pos)) {
            Some((track, Placement::Session { track, slot }))
        } else if let Some(&(track, _, left, ppb)) = targets.lanes.iter().find(|(_, r, ..)| r.contains(pos)) {
            Some((track, Placement::Arrangement { track, start: targets.beat(pos.x, left, ppb, &self.project.meter) }))
        } else {
            None
        };
        let into = match place {
            Some((track, placement)) => {
                let t = &self.project.tracks[track];
                if t.is_audio() || t.is_group() {
                    self.status = Some(tr("Suelta el MIDI en una pista MIDI, o en un hueco vacío para crear pistas").into());
                    return true;
                }
                if let Placement::Session { slot, .. } = placement {
                    if t.clips[slot].is_some() {
                        self.status = Some(tr("Ese slot ya tiene un clip").into());
                        return true;
                    }
                }
                Some(placement)
            }
            None => None,
        };
        for path in paths {
            self.import_midi(&path, into);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use eframe::egui::{pos2, Rect};

    #[test]
    fn a_dragged_file_lands_where_it_is_let_go() {
        let song = crate::test_audio::demo_song();
        let mut app = PulseApp::headless(Project::demo());
        let audio = app.add_audio_track().unwrap();
        let slot = Rect::from_min_max(pos2(100.0, 100.0), pos2(200.0, 120.0));
        let lane = Rect::from_min_max(pos2(300.0, 300.0), pos2(900.0, 350.0));
        let set_targets = |app: &mut PulseApp| {
            app.drop_targets = DropTargets {
                slots: vec![(audio, 2, slot), (0, 2, slot.translate(vec2(0.0, 40.0)))],
                lanes: vec![(audio, lane, 300.0, 14.0)],
                view: Some(Rect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 800.0))),
                ..Default::default()
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

    /// Audio let go on the Arrangement starts on the grid it shows: counts zoomed
    /// in, bars zoomed out; with ⌘ or ⌥, exactly there. In the Session, its slot.
    #[test]
    fn audio_dropped_on_the_arrangement_falls_on_the_grid() {
        let song = crate::test_audio::demo_song();
        let mut app = PulseApp::headless(Project::demo());
        let audio = app.add_audio_track().unwrap();
        let lane = Rect::from_min_max(pos2(300.0, 300.0), pos2(900.0, 350.0));
        let drop_at = |app: &mut PulseApp, beat: f32, ppb: f32, free: bool| {
            app.project.tracks[audio].arrangement.clear();
            app.drop_targets = DropTargets { lanes: vec![(audio, lane, 300.0, ppb)], view: Some(Rect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 800.0))), free, ..Default::default() };
            assert!(app.drop_files(vec![song.clone()], pos2(300.0 + beat * ppb, 320.0)));
            app.project.tracks[audio].arrangement[0].start
        };
        // Zoomed in (counts of 4/4): the nearest beat, ahead or behind.
        assert_eq!(drop_at(&mut app, 8.6, 14.0, false), 9.0);
        assert_eq!(drop_at(&mut app, 8.3, 14.0, false), 8.0);
        // Zoomed out: the nearest bar.
        assert_eq!(drop_at(&mut app, 6.5, 5.0, false), 8.0);
        assert_eq!(drop_at(&mut app, 5.5, 5.0, false), 4.0);
        // Held ⌘ or ⌥: off the grid.
        let free = drop_at(&mut app, 6.5, 5.0, true);
        assert!((free - 6.5).abs() < 1e-3, "{free}");
        // In 3/4 the bars are three beats long.
        app.project.meter = engine::meter::Meter::new(engine::meter::TimeSig::new(3, 4));
        assert_eq!(drop_at(&mut app, 4.0, 5.0, false), 3.0);
    }

    #[test]
    fn the_search_finds_files_by_their_words() {
        let dir = std::env::temp_dir().join(format!("pulse-browser-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Baterías/Viejas")).unwrap();
        for f in ["Baterías/Bombo gordo.wav", "Baterías/Viejas/bombo SECO.aif", "notas.txt", "Mi tema.pulse"] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let files: Vec<crate::search::Item> = crate::search::walk(&[dir.clone()], 1000, |p| kind(p).is_some()).iter().map(|p| crate::search::file_item(p, Path::new("/nada"))).collect();
        let names = |query: &str| crate::search::find(&[&files], query, 50).into_iter().flat_map(|(_, g)| g.into_iter().map(|i| i.name.clone())).collect::<Vec<_>>();
        let mut bombos = names("bombo");
        bombos.sort();
        assert_eq!(bombos, vec!["Bombo gordo", "bombo SECO"]);
        assert_eq!(names("seco bombo"), vec!["bombo SECO"]);
        assert_eq!(names("tema"), vec!["Mi tema"]);
        assert!(names("notas").is_empty(), "only files Pulse uses");
        // Folders first, hidden files left out.
        let listed: Vec<String> = read(&dir).into_iter().map(|e| e.name).collect();
        assert_eq!(listed, vec!["Baterías", "Mi tema.pulse"]);
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use crate::project::Project;

    /// Four bars of hats at 150 BPM: a hit on every sixteenth.
    fn tops(path: &Path, seconds_more: f64) {
        let rate = 48_000.0;
        let sixteenth = 60.0 / 150.0 / 4.0;
        let frames = ((64.0 * sixteenth + seconds_more) * rate as f64) as usize;
        let mut seed = 7u64;
        let tops: Vec<f32> = (0..frames)
            .map(|i| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let noise = ((seed >> 33) as f32 / (1u64 << 31) as f32) - 1.0;
                let t = i as f64 / rate as f64;
                let step = (t / sixteenth) as usize;
                let level = if step % 4 == 0 { 0.9 } else { 0.4 };
                noise * level * (-(t - step as f64 * sixteenth) * 60.0).exp() as f32
            })
            .collect();
        crate::capture::write_wav(path, &tops, &tops, rate).unwrap();
    }

    fn wait(app: &mut PulseApp) {
        // Until it is done; the deadline, generous for a busy machine, only
        // keeps a broken analysis from hanging the tests.
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(120) {
            app.hear_analysed();
            if app.browser.analysing.is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the analysis didn't finish");
    }

    /// A loop is heard at the project's tempo, looping, once its tempo is
    /// found; the next time at once. Without «A tempo», once at its own speed.
    #[test]
    fn loops_are_heard_in_tempo() {
        let dir = std::env::temp_dir().join(format!("pulse-preescucha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Tops 150.wav");
        tops(&path, 0.0);
        let mut app = PulseApp::headless(Project::empty());
        app.project.tempo = 128.0;
        app.preview(Some(&path));
        assert_eq!(app.browser.in_tempo, None, "silent while its tempo is found");
        assert!(app.browser.analysing.is_some());
        wait(&mut app);
        assert_eq!(app.browser.in_tempo, Some(150.0));
        assert_eq!(app.browser.previewing(), Some(path.as_path()));
        assert!(app.browser.previewing.as_ref().unwrap().2.is_infinite(), "a loop goes on");
        app.preview(None);
        app.preview(Some(&path));
        assert_eq!(app.browser.in_tempo, Some(150.0), "known: at once");
        // Not «A tempo»: once, as it is.
        app.settings.preview_in_tempo = false;
        app.preview(Some(&path));
        assert_eq!(app.browser.in_tempo, None);
        assert!((app.browser.previewing.as_ref().unwrap().2 - 6.4).abs() < 0.01);
        // A file longer than a loop is heard as it is.
        app.settings.preview_in_tempo = true;
        let long = dir.join("Largo.wav");
        tops(&long, 30.0);
        app.preview(Some(&long));
        assert!(app.browser.analysing.is_none() && app.browser.in_tempo.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// ↑ and ↓ go through the files shown, from the one selected, and stop at
    /// the ends.
    #[test]
    fn the_arrows_go_through_the_files() {
        let mut state = State::default();
        state.shown = ["a.wav", "b.wav", "c.pulse"].map(PathBuf::from).to_vec();
        assert_eq!(state.next_file(1), Some(PathBuf::from("a.wav")), "nothing selected: the first");
        assert_eq!(state.next_file(-1), Some(PathBuf::from("c.pulse")), "or the last");
        state.selected = Some(PathBuf::from("b.wav"));
        assert_eq!(state.next_file(1), Some(PathBuf::from("c.pulse")));
        assert_eq!(state.next_file(-1), Some(PathBuf::from("a.wav")));
        state.selected = Some(PathBuf::from("c.pulse"));
        assert_eq!(state.next_file(1), Some(PathBuf::from("c.pulse")));
        state.shown.clear();
        assert_eq!(state.next_file(1), None);
    }
}
