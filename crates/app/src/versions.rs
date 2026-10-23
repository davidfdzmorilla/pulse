//! Versions of a project: snapshots kept in its folder (`Versiones/`), named
//! by hand («Guardar versión…») or made on every save, that can be compared
//! with what there is now and gone back to.
//!
//! Each version is a project file of its own, with its audio files referred to
//! from the project's folder (they are already there: saving brings them in),
//! and `versiones.json` lists them with their names, notes and dates.

use crate::app::PulseApp;
use crate::project::Project;
use crate::theme::{ACCENT, TEXT_DIM};
use eframe::egui::{self, RichText};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const FOLDER: &str = "Versiones";
const INDEX: &str = "versiones.json";
/// How many versions made on saving are kept (named ones are kept for good).
pub const MAX_AUTO: usize = 20;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Version {
    /// Its file in `Versiones/`.
    pub file: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// When it was made, in seconds since 1970.
    pub created: u64,
    /// Made on saving, rather than by hand.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto: bool,
}

/// Where a project saved at `file` keeps its versions: only one in a folder of
/// its own has them.
pub fn folder(file: &Path) -> Option<PathBuf> {
    crate::library::in_own_folder(file).then(|| file.parent().map(|d| d.join(FOLDER))).flatten()
}

/// The versions in `dir`, oldest first.
pub fn list(dir: &Path) -> Vec<Version> {
    let versions: Vec<Version> = crate::store::read_json(&dir.join(INDEX));
    // The list comes with the project's folder, which may have been shared or
    // damaged: a version is a file in this folder, never a path elsewhere
    // (pruning would delete it).
    versions.into_iter().filter(|v| in_folder(&v.file)).collect()
}

/// Whether `file` names a file right in the versions folder.
fn in_folder(file: &str) -> bool {
    let path = Path::new(file);
    !file.contains(['/', '\\']) && path.file_name().is_some_and(|name| name == file) && !path.is_absolute()
}

fn write_list(dir: &Path, versions: &[Version]) -> Result<(), String> {
    crate::store::write_json(&dir.join(INDEX), versions).map_err(|e| format!("No se pudo guardar la lista de versiones: {e}"))
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The project as a version file: its paths relative to the project's folder
/// (the parent of `dir`).
fn text(project: &Project, dir: &Path) -> Result<String, String> {
    crate::library::relative(project, dir.parent().unwrap_or(dir)).to_json()
}

/// Keeps `project` as a new version in `dir`. A version made on saving is not
/// kept if nothing changed since the last one, and only the newest [`MAX_AUTO`]
/// of those stay.
pub fn add(dir: &Path, project: &Project, name: &str, note: &str, auto: bool) -> Result<Option<Version>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    let content = text(project, dir)?;
    let mut versions = list(dir);
    if auto && versions.last().is_some_and(|last| std::fs::read_to_string(dir.join(&last.file)).is_ok_and(|t| t == content)) {
        return Ok(None);
    }
    let created = now();
    let mut stem = created.to_string();
    let mut n = 1;
    while dir.join(format!("{stem}.pulse")).exists() || versions.iter().any(|v| v.file == format!("{stem}.pulse")) {
        n += 1;
        stem = format!("{created}-{n}");
    }
    let file = format!("{stem}.pulse");
    crate::store::write_atomic(&dir.join(&file), content.as_bytes()).map_err(|e| format!("No se pudo guardar la versión: {e}"))?;
    let version = Version { file, name: name.to_string(), note: note.to_string(), created, auto };
    versions.push(version.clone());
    let autos = versions.iter().filter(|v| v.auto).count();
    if autos > MAX_AUTO {
        let mut extra = autos - MAX_AUTO;
        versions.retain(|v| {
            if v.auto && extra > 0 {
                extra -= 1;
                let _ = std::fs::remove_file(dir.join(&v.file));
                false
            } else {
                true
            }
        });
    }
    write_list(dir, &versions)?;
    Ok(Some(version))
}

/// The project a version holds, its files found in the project's folder.
pub fn load(dir: &Path, version: &Version) -> Result<Project, String> {
    let text = std::fs::read_to_string(dir.join(&version.file)).map_err(|e| format!("No se pudo leer la versión: {e}"))?;
    let mut project = Project::from_json(&text)?;
    crate::library::resolve(&mut project, dir.parent().unwrap_or(dir));
    Ok(project)
}

pub fn rename(dir: &Path, file: &str, name: &str, note: &str) -> Result<(), String> {
    let mut versions = list(dir);
    if let Some(v) = versions.iter_mut().find(|v| v.file == file) {
        v.name = name.to_string();
        v.note = note.to_string();
        // Named by hand, it is kept for good.
        v.auto = false;
    }
    write_list(dir, &versions)
}

pub fn remove(dir: &Path, file: &str) -> Result<(), String> {
    if !in_folder(file) {
        return Ok(());
    }
    let mut versions = list(dir);
    versions.retain(|v| v.file != file);
    let _ = std::fs::remove_file(dir.join(file));
    write_list(dir, &versions)
}

/// What changed from `old` to `new`, in words, one change a line.
pub fn diff(old: &Project, new: &Project) -> Vec<String> {
    let mut out = Vec::new();
    if old.tempo != new.tempo {
        out.push(format!("Tempo {} → {}", trim(old.tempo), trim(new.tempo)));
    }
    if old.key != new.key {
        let name = |k: Option<stems::key::Key>| k.map_or("sin escala".to_string(), |k| k.name());
        out.push(format!("Escala: {} → {}", name(old.key), name(new.key)));
    }
    // Tracks are matched by name, in order (two of the same name pair up in turn).
    let mut unmatched: Vec<usize> = (0..old.tracks.len()).collect();
    let mut pairs = Vec::new();
    for (j, track) in new.tracks.iter().enumerate() {
        match unmatched.iter().position(|&i| old.tracks[i].name == track.name) {
            Some(k) => pairs.push((unmatched.remove(k), j)),
            None => out.push(format!("+ Pista «{}»", track.name)),
        }
    }
    for &i in &unmatched {
        out.push(format!("− Pista «{}»", old.tracks[i].name));
    }
    let moved = pairs.windows(2).any(|w| w[0].0 > w[1].0);
    for (i, j) in pairs {
        let (a, b) = (&old.tracks[i], &new.tracks[j]);
        let mut what = Vec::new();
        if std::mem::discriminant(&a.instrument) != std::mem::discriminant(&b.instrument) {
            what.push("otro instrumento".to_string());
        } else if a.instrument != b.instrument || a.samples != b.samples {
            what.push("sonido retocado".to_string());
        }
        if a.devices.len() != b.devices.len() {
            what.push(format!("{} → {} efectos", a.devices.len(), b.devices.len()));
        } else if a.devices != b.devices {
            what.push("efectos retocados".to_string());
        }
        let session = |t: &crate::project::Track| t.clips.iter().flatten().count();
        if session(a) != session(b) {
            what.push(format!("{} → {} clips en Session", session(a), session(b)));
        } else if a.clips != b.clips {
            what.push("clips de Session editados".to_string());
        }
        if a.arrangement.len() != b.arrangement.len() {
            what.push(format!("{} → {} clips en el Arrangement", a.arrangement.len(), b.arrangement.len()));
        } else if a.arrangement != b.arrangement {
            what.push("Arrangement editado".to_string());
        }
        if a.automation != b.automation {
            what.push("automatización".to_string());
        }
        if a.mixer != b.mixer || a.sends != b.sends || a.solo != b.solo {
            what.push("mezcla".to_string());
        }
        if a.frozen.is_some() != b.frozen.is_some() {
            what.push(if b.frozen.is_some() { "congelada" } else { "descongelada" }.to_string());
        }
        if a.color != b.color && what.is_empty() {
            what.push("color".to_string());
        }
        if !what.is_empty() {
            out.push(format!("«{}»: {}", b.name, what.join(", ")));
        }
    }
    if moved {
        out.push("Pistas en otro orden".into());
    }
    if old.returns != new.returns {
        out.push("Retornos cambiados".into());
    }
    if old.master != new.master {
        out.push("Master cambiado".into());
    }
    let rest = |p: &Project| (p.loop_enabled, p.loop_start, p.loop_end, p.swing, p.swing_grid, p.quantize, p.scene_names.clone());
    if rest(old) != rest(new) {
        out.push("Loop, swing, escenas u otros ajustes".into());
    }
    out
}

fn trim(x: f64) -> String {
    let text = format!("{x:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// «hace 5 min», «hace 3 h», «hace 2 días».
pub fn ago(created: u64, now: u64) -> String {
    let s = now.saturating_sub(created);
    match s {
        0..60 => "ahora mismo".into(),
        60..3600 => format!("hace {} min", s / 60),
        3600..86_400 => format!("hace {} h", s / 3600),
        86_400..172_800 => "ayer".into(),
        _ => format!("hace {} días", s / 86_400),
    }
}

/// The «Versiones» window's state.
#[derive(Default)]
pub struct Window {
    pub open: bool,
    /// The name and note of the version about to be saved.
    name: String,
    note: String,
    /// The version being renamed, with its new name and note.
    renaming: Option<(String, String, String)>,
    /// The version shown against what there is now.
    comparing: Option<String>,
    /// The version whose «Borrar» asks again.
    deleting: Option<String>,
    /// What each version changed from the one before (by file), worked out once.
    changes: HashMap<String, Vec<String>>,
}

impl PulseApp {
    /// Where this project's versions are, or why it has none yet.
    pub(crate) fn versions_dir(&self) -> Result<PathBuf, String> {
        match &self.file {
            None => Err(crate::theme::keys("Guarda el proyecto (⌘S) para poder guardar versiones")),
            Some(file) => folder(file).ok_or_else(|| "Usa «Guardar como…» para dar al proyecto su carpeta: las versiones se guardan en ella".into()),
        }
    }

    /// Keeps what there is now as a version named `name`.
    pub fn save_version(&mut self, name: &str, note: &str) {
        let dir = match self.versions_dir() {
            Ok(dir) => dir,
            Err(e) => {
                self.status = Some(e);
                return;
            }
        };
        self.capture_plugins();
        let name = if name.trim().is_empty() { format!("Versión {}", list(&dir).iter().filter(|v| !v.auto).count() + 1) } else { name.trim().to_string() };
        self.status = Some(match add(&dir, &self.project, &name, note.trim(), false) {
            Ok(_) => format!("Versión «{name}» guardada"),
            Err(e) => e,
        });
    }

    /// After saving: a version of it, if it changed since the last.
    pub(crate) fn auto_version(&mut self) {
        if let Ok(dir) = self.versions_dir() {
            if let Err(e) = add(&dir, &self.project, "Guardado", "", true) {
                self.status = Some(e);
            }
        }
    }

    /// Goes back to a version, keeping what there was as a version first (so
    /// nothing is lost) and as a step ⌘Z undoes.
    pub fn restore_version(&mut self, file: &str) {
        let Ok(dir) = self.versions_dir() else { return };
        let Some(version) = list(&dir).into_iter().find(|v| v.file == file) else { return };
        let project = match load(&dir, &version) {
            Ok(p) => p,
            Err(e) => {
                self.status = Some(e);
                return;
            }
        };
        self.capture_plugins();
        // Named, so the automatic ones that come after never push it out.
        let _ = add(&dir, &self.project, &format!("Antes de volver a «{}»", version.name), "", false);
        self.history.commit(&self.project, false);
        self.project = project;
        self.history.commit(&self.project, false);
        self.selected_clip = None;
        self.after_project_replaced(None);
        self.status = Some(crate::theme::keys(&format!("Has vuelto a «{}» (⌘Z lo deshace; guarda para quedarte con ella)", version.name)));
    }
}

/// The «Versiones» window.
pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    if !app.versions.open {
        return;
    }
    let mut open = true;
    let title = format!("Versiones de «{}»", app.file_name());
    egui::Window::new(title)
        .id(egui::Id::new("versions"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([520.0, 560.0])
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| contents(app, ui));
    app.versions.open &= open;
}

fn contents(app: &mut PulseApp, ui: &mut egui::Ui) {
    let dir = match app.versions_dir() {
        Ok(dir) => dir,
        Err(e) => {
            ui.label(RichText::new(e).color(TEXT_DIM));
            return;
        }
    };
    // Save one.
    ui.horizontal(|ui| {
        let w = &mut app.versions;
        ui.add(egui::TextEdit::singleline(&mut w.name).hint_text("Nombre (p. ej. «Con el bajo nuevo»)").desired_width(220.0));
        ui.add(egui::TextEdit::singleline(&mut w.note).hint_text("Nota (opcional)").desired_width(150.0));
        if ui.button("Guardar versión").on_hover_text(crate::theme::keys("Guarda cómo está el proyecto ahora (⌥⌘S)")).clicked() {
            let (name, note) = (std::mem::take(&mut app.versions.name), std::mem::take(&mut app.versions.note));
            app.save_version(&name, &note);
        }
    });
    ui.label(RichText::new(crate::theme::keys(&format!("Al guardar (⌘S) se hace también una versión automática; se guardan las {MAX_AUTO} últimas."))).color(TEXT_DIM).size(10.5));
    ui.separator();

    let versions = list(&dir);
    if versions.is_empty() {
        ui.label(RichText::new("Aún no hay versiones.").color(TEXT_DIM));
        return;
    }
    // What each changed from the one before, worked out the first time it is shown.
    for (i, v) in versions.iter().enumerate() {
        if !app.versions.changes.contains_key(&v.file) {
            let changes = match (i.checked_sub(1).map(|p| load(&dir, &versions[p])), load(&dir, v)) {
                (None, _) => vec!["La primera".to_string()],
                (Some(Ok(before)), Ok(this)) => diff(&before, &this),
                _ => vec!["No se pudo leer".to_string()],
            };
            app.versions.changes.insert(v.file.clone(), changes);
        }
    }
    let now = now();
    let mut action: Option<(&str, String)> = None;
    egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
        for v in versions.iter().rev() {
            let w = &mut app.versions;
            ui.horizontal(|ui| {
                ui.label(RichText::new(&v.name).strong().color(if v.auto { TEXT_DIM } else { ACCENT }));
                let when = if v.auto { format!("{} · automática", ago(v.created, now)) } else { ago(v.created, now) };
                ui.label(RichText::new(when).color(TEXT_DIM).size(10.5));
            });
            if !v.note.is_empty() {
                ui.label(RichText::new(&v.note).italics().size(11.0));
            }
            let changes = w.changes.get(&v.file).cloned().unwrap_or_default();
            let shown = if changes.is_empty() { "Sin cambios".to_string() } else { changes.join(" · ") };
            ui.label(RichText::new(shown).color(TEXT_DIM).size(10.5));
            ui.horizontal(|ui| {
                if ui.small_button("Volver a esta").on_hover_text(crate::theme::keys("La abre en lugar de lo que hay ahora, que queda guardado como versión (⌘Z lo deshace)")).clicked() {
                    action = Some(("restore", v.file.clone()));
                }
                let comparing = w.comparing.as_deref() == Some(v.file.as_str());
                if ui.selectable_label(comparing, "Comparar con ahora").clicked() {
                    w.comparing = if comparing { None } else { Some(v.file.clone()) };
                }
                if ui.small_button("Renombrar").clicked() {
                    w.renaming = Some((v.file.clone(), v.name.clone(), v.note.clone()));
                }
                let asking = w.deleting.as_deref() == Some(v.file.as_str());
                if ui.small_button(if asking { "¿Seguro? Borrar" } else { "Borrar" }).clicked() {
                    if asking {
                        action = Some(("remove", v.file.clone()));
                    } else {
                        w.deleting = Some(v.file.clone());
                    }
                }
            });
            if let Some((file, name, note)) = w.renaming.as_mut().filter(|(f, ..)| f == &v.file) {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(name).desired_width(200.0));
                    ui.add(egui::TextEdit::singleline(note).hint_text("Nota").desired_width(150.0));
                    if ui.small_button("Aceptar").clicked() {
                        action = Some(("rename", file.clone()));
                    }
                });
            }
            if w.comparing.as_deref() == Some(v.file.as_str()) {
                let lines = match load(&dir, v) {
                    Ok(then) => diff(&then, &app.project),
                    Err(e) => vec![e],
                };
                ui.group(|ui| {
                    ui.label(RichText::new("De esta versión a ahora:").size(10.5));
                    if lines.is_empty() {
                        ui.label(RichText::new("Igual que ahora").color(TEXT_DIM));
                    }
                    for line in lines {
                        ui.label(RichText::new(line).size(11.0));
                    }
                });
            }
            ui.separator();
        }
    });
    match action {
        Some(("restore", file)) => {
            app.restore_version(&file);
            app.versions.changes.clear();
        }
        Some(("remove", file)) => {
            if let Err(e) = remove(&dir, &file) {
                app.status = Some(e);
            }
            app.versions.deleting = None;
            app.versions.changes.clear();
        }
        Some(("rename", file)) => {
            if let Some((_, name, note)) = app.versions.renaming.take() {
                if let Err(e) = rename(&dir, &file, name.trim(), note.trim()) {
                    app.status = Some(e);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pulse-versions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A version list that names files outside its folder (a shared or
    /// damaged project folder): those are left out, so pruning never deletes
    /// the project or anything else.
    #[test]
    fn versions_never_reach_outside_their_folder() {
        let root = temp("outside");
        let project_dir = root.join("Tema");
        let dir = project_dir.join(FOLDER);
        std::fs::create_dir_all(&dir).unwrap();
        let project_file = project_dir.join("Tema.pulse");
        std::fs::write(&project_file, "the project").unwrap();
        let mut versions = vec![Version { file: "../Tema.pulse".into(), name: "x".into(), note: String::new(), created: 1, auto: true }];
        for i in 0..MAX_AUTO {
            versions.push(Version { file: format!("{i}.pulse"), name: format!("v{i}"), note: String::new(), created: 2 + i as u64, auto: true });
        }
        write_list(&dir, &versions).unwrap();
        assert_eq!(list(&dir).len(), MAX_AUTO, "the one outside is left out");
        let mut project = Project::demo();
        project.tempo = 101.0;
        add(&dir, &project, "nueva", "", true).unwrap();
        remove(&dir, "../Tema.pulse").unwrap();
        for bad in ["..", ".", "/tmp/x.pulse", "a/b.pulse", "a\\b.pulse", ""] {
            assert!(!in_folder(bad), "{bad}");
        }
        assert!(in_folder("1729000000-2.pulse"));
        let survived = project_file.is_file();
        let _ = std::fs::remove_dir_all(&root);
        assert!(survived, "the project file was deleted");
    }

    #[test]
    fn versions_are_kept_listed_and_read_back() {
        let root = temp("keep");
        let dir = root.join("Canción").join(FOLDER);
        let mut project = Project::demo();
        let first = add(&dir, &project, "Idea", "la primera", false).unwrap().unwrap();
        // Unchanged, a version on saving is not kept.
        assert_eq!(add(&dir, &project, "Guardado", "", true).unwrap(), None);
        project.tempo = 128.0;
        let second = add(&dir, &project, "Guardado", "", true).unwrap().unwrap();
        assert_ne!(first.file, second.file);
        let versions = list(&dir);
        assert_eq!(versions.iter().map(|v| (v.name.as_str(), v.auto)).collect::<Vec<_>>(), vec![("Idea", false), ("Guardado", true)]);
        assert_eq!(load(&dir, &versions[0]).unwrap().tempo, Project::demo().tempo);
        assert_eq!(load(&dir, &versions[1]).unwrap(), project);
        rename(&dir, &second.file, "Más rápida", "a 128").unwrap();
        assert_eq!(list(&dir)[1].name, "Más rápida");
        assert!(!list(&dir)[1].auto, "named, it stays for good");
        remove(&dir, &first.file).unwrap();
        assert_eq!(list(&dir).len(), 1);
        assert!(!dir.join(&first.file).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_the_newest_automatic_versions_stay() {
        let root = temp("prune");
        let dir = root.join("Canción").join(FOLDER);
        let mut project = Project::empty();
        add(&dir, &project, "A mano", "", false).unwrap();
        for i in 0..MAX_AUTO + 5 {
            project.tempo = 100.0 + i as f64;
            add(&dir, &project, "Guardado", "", true).unwrap();
        }
        let versions = list(&dir);
        assert_eq!(versions.len(), MAX_AUTO + 1);
        assert_eq!(versions[0].name, "A mano");
        assert_eq!(load(&dir, &versions[1]).unwrap().tempo, 105.0, "the oldest automatic ones went");
        let files = std::fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "pulse")).count();
        assert_eq!(files, MAX_AUTO + 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn changes_are_told_in_words() {
        let old = Project::demo();
        let mut new = old.clone();
        assert!(diff(&old, &new).is_empty());
        new.tempo = 124.0;
        new.key = Some(stems::key::Key { tonic: 9, minor: true });
        let bass = new.tracks.iter().position(|t| t.name == "Bass").unwrap();
        new.tracks[bass].mixer.gain = 0.3;
        new.tracks[bass].clips[3] = new.tracks[bass].clips[0].clone();
        let mut voice = crate::project::Track::new("Voz", [0; 3], engine::model::InstrumentKind::Synth(Default::default()));
        voice.name = "Voz".into();
        new.tracks.push(voice);
        new.tracks.retain(|t| t.name != "Lead");
        let lines = diff(&old, &new);
        for expected in ["Tempo 120 → 124", "Escala: sin escala → La menor", "+ Pista «Voz»", "− Pista «Lead»", "«Bass»: 1 → 2 clips en Session, mezcla"] {
            assert!(lines.iter().any(|l| l == expected), "{expected}: {lines:?}");
        }
    }

    /// Saving makes a version; going back to one brings it, keeps what there
    /// was, and ⌘Z undoes it.
    #[test]
    fn saving_makes_versions_and_one_can_go_back() {
        let root = temp("app");
        let file = root.join("Canción").join("Canción.pulse");
        let mut app = PulseApp::headless(Project::demo());
        app.save_version("Nada", "");
        assert!(app.status.as_deref().is_some_and(|s| s.contains("Guarda el proyecto")));
        app.save_to_path(&file);
        let dir = folder(&file).unwrap();
        assert_eq!(list(&dir).len(), 1, "a version on saving");
        app.save_version("Original", "a 120");
        app.project.tempo = 90.0;
        app.commit_history();
        app.save_to_path(&file);
        let versions = list(&dir);
        assert_eq!(versions.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), vec!["Guardado", "Original", "Guardado"]);
        let original = versions[1].file.clone();
        app.project.tempo = 95.0;
        app.commit_history();
        app.restore_version(&original);
        assert_eq!(app.project.tempo, Project::demo().tempo);
        let last = list(&dir).last().cloned().unwrap();
        assert_eq!(last.name, "Antes de volver a «Original»");
        assert!(!last.auto, "named: the automatic ones never push it out");
        assert_eq!(load(&dir, &last).unwrap().tempo, 95.0, "what there was is kept");
        app.undo();
        assert_eq!(app.project.tempo, 95.0);
        let _ = std::fs::remove_dir_all(root);
    }

    /// Going back to a version with fewer tracks and returns than the selected
    /// one: the selection and the editors stay within it (Pulse used to close).
    #[test]
    fn going_back_to_a_smaller_version_keeps_the_selection_in_it() {
        let root = temp("smaller");
        let file = root.join("Canción").join("Canción.pulse");
        let mut app = PulseApp::headless(Project::demo());
        app.save_to_path(&file);
        app.save_version("Original", "");
        let dir = folder(&file).unwrap();
        let original = list(&dir).into_iter().find(|v| v.name == "Original").unwrap().file;
        let tracks = app.project.tracks.len();
        app.add_track();
        app.add_track();
        app.selected_target = engine::Target::Track(tracks + 1);
        app.selected_tracks = vec![tracks, tracks + 1];
        app.open_instrument_editor(crate::app::InstrumentEditor::Wave, tracks + 1);
        app.commit_history();
        app.restore_version(&original);
        assert_eq!(app.project.tracks.len(), tracks);
        assert!(matches!(app.selected_target, engine::Target::Track(t) if t < tracks));
        assert!(app.selected_tracks.is_empty() && app.wave_window.is_none());
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| crate::detail::show(&mut app, ui));
        let _ = std::fs::remove_dir_all(root);
    }

    /// The window lists the versions, newest first, with what each changed.
    #[test]
    fn the_window_shows_each_version_and_its_changes() {
        let root = temp("window");
        let file = root.join("Canción").join("Canción.pulse");
        let mut app = PulseApp::headless(Project::demo());
        app.save_to_path(&file);
        app.project.tempo = 124.0;
        app.save_version("Más rápida", "para el directo");
        app.open_panel(crate::app::Panel::Versions);
        let ctx = egui::Context::default();
        let input = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1400.0, 900.0))), ..Default::default() };
        let _ = ctx.run_ui(input(), |ui| window(&mut app, ui.ctx()));
        let output = ctx.run_ui(input(), |ui| window(&mut app, ui.ctx()));
        let mut texts = Vec::new();
        fn collect(shape: &egui::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => texts.push(t.galley.text().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
                _ => {}
            }
        }
        output.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
        for expected in ["Más rápida", "para el directo", "Tempo 120 → 124", "Guardado", "La primera"] {
            assert!(texts.iter().any(|t| t == expected), "{expected}: {texts:?}");
        }
        let newest = texts.iter().position(|t| t == "Más rápida").unwrap();
        assert!(newest < texts.iter().position(|t| t == "Guardado").unwrap(), "newest first");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dates_read_as_how_long_ago() {
        assert_eq!(ago(1000, 1030), "ahora mismo");
        assert_eq!(ago(1000, 1000 + 300), "hace 5 min");
        assert_eq!(ago(0, 3 * 3600), "hace 3 h");
        assert_eq!(ago(0, 100_000), "ayer");
        assert_eq!(ago(0, 5 * 86_400), "hace 5 días");
    }
}
