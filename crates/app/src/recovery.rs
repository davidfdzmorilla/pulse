//! Autosave and recovery. Every [`EVERY`] while there are unsaved changes, a copy
//! of the project goes to `Recuperación/` in Pulse's data folder (never over the
//! user's file). Saving, or quitting after the unsaved-changes question, removes
//! it; finding it at the start means Pulse closed without either, and offers it.
//!
//! The same folder keeps a note while the last project is reopened at the start:
//! if Pulse closes during that, the next start doesn't reopen it again.

use crate::app::PulseApp;
use crate::project::Project;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often unsaved changes are copied.
pub const EVERY: Duration = Duration::from_secs(30);

const PROJECT: &str = "autoguardado.pulse";
const INFO: &str = "autoguardado.json";
const DISCARDED: &str = "descartado.pulse";
const OPENING: &str = "abriendo.txt";

/// What an autosave is of.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Info {
    /// The project's file, if it had been saved.
    pub original: Option<PathBuf>,
    /// When (seconds since 1970).
    pub saved_at: u64,
    /// The Pulse that wrote it.
    pub version: String,
}

impl Info {
    /// The project's name, for the question.
    pub fn name(&self) -> String {
        self.original.as_deref().and_then(|p| p.file_stem()).map_or_else(|| "Sin título".into(), |s| s.to_string_lossy().into_owned())
    }
}

#[derive(Clone, Debug)]
pub struct Recovery {
    dir: PathBuf,
}

impl Recovery {
    pub fn new(dir: PathBuf) -> Self {
        Recovery { dir }
    }

    /// Pulse's own: `Recuperación` in its data folder.
    pub fn standard() -> Self {
        Self::new(paths::data_dir().join("Recuperación"))
    }

    pub fn project_path(&self) -> PathBuf {
        self.dir.join(PROJECT)
    }

    /// Writes an autosave: the project first, then what it is of, so a copy is
    /// only offered once it is whole.
    pub fn write(&self, project: &Project, info: &Info) -> Result<(), String> {
        project.save(&self.project_path())?;
        crate::store::write_json(&self.dir.join(INFO), info)
    }

    /// The autosave left by a Pulse that closed without saving or asking, if any.
    pub fn pending(&self) -> Option<Info> {
        let info = self.dir.join(INFO);
        if !info.exists() || !self.project_path().exists() {
            return None;
        }
        Some(crate::store::read_json(&info))
    }

    pub fn load(&self) -> Result<Project, String> {
        Project::load(&self.project_path())
    }

    /// A copy that could not be opened (from a newer Pulse, or damaged) is moved
    /// out of the way, so the next autosave doesn't write over it.
    pub fn set_aside(&self) -> Option<PathBuf> {
        let aside = crate::store::set_aside(&self.project_path());
        let _ = std::fs::remove_file(self.dir.join(INFO));
        aside
    }

    /// No unsaved changes are left to recover.
    pub fn clear(&self) {
        let _ = std::fs::remove_file(self.dir.join(INFO));
        let _ = std::fs::remove_file(self.project_path());
    }

    /// The user chose not to recover: the copy is kept once more as
    /// `descartado.pulse` (until the next one), in case that was a mistake.
    pub fn discard(&self) {
        let _ = std::fs::remove_file(self.dir.join(INFO));
        let _ = std::fs::rename(self.project_path(), self.dir.join(DISCARDED));
    }

    /// Notes that `path` is being reopened at the start.
    pub fn opening(&self, path: &Path) {
        let _ = crate::store::write_atomic(&self.dir.join(OPENING), path.to_string_lossy().as_bytes());
    }

    /// It opened (and Pulse kept running a while).
    pub fn opened(&self) {
        let _ = std::fs::remove_file(self.dir.join(OPENING));
    }

    /// The project Pulse was reopening when it last closed by itself, if any.
    pub fn interrupted_opening(&self) -> Option<PathBuf> {
        std::fs::read_to_string(self.dir.join(OPENING)).ok().map(PathBuf::from)
    }
}

/// The app's side of autosaving.
#[derive(Default)]
pub struct Autosave {
    /// When it last looked.
    last: Option<Instant>,
    /// What it last wrote.
    snapshot: Option<Project>,
    /// The write under way (it runs apart, so the UI never waits for the disk).
    job: Option<std::thread::JoinHandle<Result<(), String>>>,
    /// Since when the last project is being reopened at the start.
    opening: Option<Instant>,
}

/// How long a project reopened at the start must keep Pulse running to count as opened.
const SETTLED: Duration = Duration::from_secs(5);

impl PulseApp {
    /// Once a frame: every [`EVERY`], copies unsaved changes to the recovery folder.
    pub fn autosave_now_and_then(&mut self, ctx: &eframe::egui::Context) {
        if let Some(since) = self.autosave.opening {
            if since.elapsed() >= SETTLED {
                self.autosave.opening = None;
                if let Some(recovery) = &self.recovery {
                    recovery.opened();
                }
            } else {
                ctx.request_repaint_after(SETTLED);
            }
        }
        let Some(recovery) = self.recovery.clone() else { return };
        if self.is_dirty() {
            // Even with nothing on screen moving, the next look comes.
            ctx.request_repaint_after(EVERY);
        }
        if self.autosave.job.as_ref().is_some_and(|job| !job.is_finished()) {
            return;
        }
        self.finish_autosave();
        let now = Instant::now();
        if self.autosave.last.is_some_and(|last| now - last < EVERY) {
            return;
        }
        self.autosave.last = Some(now);
        if !self.is_dirty() || (!self.plugins_edited && self.autosave.snapshot.as_ref() == Some(&self.project)) {
            return;
        }
        self.capture_plugins();
        let project = self.project.clone();
        self.autosave.snapshot = Some(project.clone());
        let info = Info { original: self.file.clone(), saved_at: crate::versions::now(), version: crate::updates::VERSION.into() };
        self.autosave.job = Some(std::thread::spawn(move || recovery.write(&project, &info)));
    }

    /// Waits for the write under way, if any.
    fn finish_autosave(&mut self) {
        if let Some(job) = self.autosave.job.take() {
            if let Ok(Err(e)) = job.join() {
                crate::logs::note(&format!("No se pudo autoguardar: {e}"));
            }
        }
    }

    /// Nothing is left to recover: saved, or the changes were let go.
    pub fn clear_recovery(&mut self) {
        self.finish_autosave();
        self.autosave.snapshot = None;
        if let Some(recovery) = &self.recovery {
            recovery.clear();
        }
    }

    /// At the start: if Pulse closed with unsaved changes, asks whether to get
    /// them back. True if they were.
    pub fn offer_recovery(&mut self) -> bool {
        let Some(recovery) = self.recovery.clone() else { return false };
        let Some(info) = recovery.pending() else { return false };
        let recordings_dir = crate::library::media_dir(info.original.as_deref(), crate::library::RECORDINGS);
        let recordings = unfinished_recordings(&recordings_dir);
        let mut description = format!("Pulse se cerró sin guardar los cambios de «{}» (el último autoguardado es de {}).", info.name(), crate::versions::ago(info.saved_at, crate::versions::now()));
        let unfinished = (!recordings.is_empty()).then(|| format!("{} grabación(es) sin terminar en {}", recordings.len(), recordings_dir.display()));
        if let Some(unfinished) = &unfinished {
            description += &format!(" Además hay {unfinished}.");
        }
        if !self.ask_to_recover(&description) {
            recovery.discard();
            self.status = Some("Cambios sin guardar descartados (la copia queda en «descartado.pulse», en la carpeta Recuperación de Pulse)".into());
            return false;
        }
        let project = match recovery.load() {
            Ok(project) => project,
            Err(e) => {
                let kept = recovery.set_aside().map_or_else(String::new, |p| format!(" · la copia queda en {}", p.display()));
                self.status = Some(format!("No se pudieron recuperar los cambios: {e}{kept}"));
                return false;
            }
        };
        let saved = info.original.as_deref().and_then(|p| Project::load(p).ok());
        self.replace_project(project, info.original.clone());
        // The copy was written by this Pulse, the file by whichever saved it:
        // saving over a newer Pulse's file still asks first.
        if let Some(saved) = &saved {
            self.project.saved_by = saved.saved_by.clone();
        }
        // Unsaved until saved: against the file as it is on the disk.
        self.saved = saved.unwrap_or_else(Project::empty);
        // replace_project let the old copy go: this one stays until saved.
        if let Err(e) = recovery.write(&self.project, &info) {
            crate::logs::note(&format!("No se pudo autoguardar: {e}"));
        }
        let mut status = format!("Recuperados los cambios de «{}»: guárdalos (⌘S) para conservarlos", info.name());
        if let Some(unfinished) = unfinished {
            status += &format!(" · {unfinished}");
        }
        self.status = Some(crate::theme::named_keys(&status));
        true
    }

    fn ask_to_recover(&mut self, description: &str) -> bool {
        if let Some(answer) = self.answers.pop_front() {
            return answer != crate::commands::Answer::Discard;
        }
        let result = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("¿Recuperar los cambios sin guardar?")
            .set_description(description)
            .set_buttons(rfd::MessageButtons::OkCancelCustom("Recuperar".into(), "Descartar".into()))
            .show();
        // Only a clear «Descartar» lets them go.
        !matches!(result, rfd::MessageDialogResult::Cancel | rfd::MessageDialogResult::No)
            && !matches!(&result, rfd::MessageDialogResult::Custom(label) if label == "Descartar")
    }

    /// Reopens the last project at the start, unless Pulse closed by itself the
    /// last time it did (that project may be what makes it close).
    pub fn reopen_guarded(&mut self, path: &Path) {
        if let Some(recovery) = &self.recovery {
            if recovery.interrupted_opening().as_deref() == Some(path) {
                recovery.opened();
                let name = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                self.status = Some(format!("No se volvió a abrir «{name}»: Pulse se cerró la última vez mientras lo abría. Puedes abrirlo desde Archivo → Abrir recientes."));
                return;
            }
            recovery.opening(path);
            self.autosave.opening = Some(Instant::now());
        }
        self.open_from(path);
    }
}

/// Recordings that were going on when Pulse closed (see `capture::Safety`), in `dir`.
pub fn unfinished_recordings(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(crate::capture::UNFINISHED))).collect();
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn scratch(name: &str) -> Recovery {
        let dir = std::env::temp_dir().join(format!("pulse-recovery-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Recovery::new(dir)
    }

    /// A project a newer Pulse saved, recovered after a crash: ⌘S still asks
    /// before writing over it (the recovered copy, written by this Pulse,
    /// used to let it through and lose what the newer one knew).
    #[test]
    fn recovered_changes_still_ask_before_overwriting_a_newer_pulses_file() {
        let dir = std::env::temp_dir().join(format!("pulse-recovery-newer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("Tema").join("Tema.pulse");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let text = Project::demo().to_json().unwrap().replace(&format!("\"saved_by\": \"{}\"", crate::updates::VERSION), "\"saved_by\": \"99.0.0\"");
        assert!(text.contains("99.0.0"));
        std::fs::write(&file, &text).unwrap();
        let recovery = Recovery::new(dir.join("Recuperación"));
        let mut changed = Project::load(&file).unwrap();
        changed.tempo = 133.0;
        recovery.write(&changed, &Info { original: Some(file.clone()), saved_at: 1, version: crate::updates::VERSION.into() }).unwrap();

        let mut app = PulseApp::headless(Project::empty());
        app.recovery = Some(recovery);
        app.answers.push_back(crate::commands::Answer::Save);
        assert!(app.offer_recovery());
        assert_eq!(app.project.tempo, 133.0);
        app.save_to_path(&file);
        let after = std::fs::read_to_string(&file).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(after.contains("99.0.0"), "written over without asking");
    }

    #[test]
    fn an_autosave_is_offered_until_cleared_and_kept_once_when_discarded() {
        let recovery = scratch("module");
        assert_eq!(recovery.pending(), None);
        let mut project = Project::demo();
        project.tempo = 133.0;
        let info = Info { original: Some("/Música/Tema.pulse".into()), saved_at: 5, version: "0.2.0".into() };
        recovery.write(&project, &info).unwrap();
        assert_eq!(recovery.pending(), Some(info.clone()));
        assert_eq!(recovery.pending().unwrap().name(), "Tema");
        assert_eq!(recovery.load().unwrap().tempo, 133.0);
        recovery.clear();
        assert_eq!(recovery.pending(), None);
        recovery.write(&project, &info).unwrap();
        recovery.discard();
        assert_eq!(recovery.pending(), None);
        assert_eq!(Project::load(&recovery.dir.join(DISCARDED)).unwrap().tempo, 133.0);
        // A project without its note (cut short) isn't offered.
        project.save(&recovery.project_path()).unwrap();
        assert_eq!(recovery.pending(), None);
        let _ = std::fs::remove_dir_all(&recovery.dir);
    }

    /// An app with its recovery folder in `recovery`, working on a saved project.
    fn app_with(recovery: &Recovery, name: &str) -> (PulseApp, PathBuf) {
        let mut app = PulseApp::headless(Project::demo());
        app.recovery = Some(recovery.clone());
        let file = std::env::temp_dir().join(format!("pulse-recovery-file-{name}-{}/Tema/Tema.pulse", std::process::id()));
        app.save_to_path(&file);
        (app, file)
    }

    fn autosave_now(app: &mut PulseApp) {
        app.autosave.last = None;
        app.autosave_now_and_then(&eframe::egui::Context::default());
        app.finish_autosave();
    }

    #[test]
    fn unsaved_changes_are_autosaved_and_recovered_after_a_crash() {
        let recovery = scratch("crash");
        let (mut app, file) = app_with(&recovery, "crash");
        autosave_now(&mut app);
        assert_eq!(recovery.pending(), None, "nothing unsaved, nothing written");
        app.project.tempo = 141.0;
        autosave_now(&mut app);
        let info = recovery.pending().expect("the changes are copied");
        assert_eq!(info.original.as_deref(), Some(file.as_path()));
        assert_eq!(Project::load(&file).unwrap().tempo, Project::demo().tempo, "the user's file is not touched");
        // Pulse closes by itself; the next one asks, and «Recuperar» brings them back.
        drop(app);
        let mut next = PulseApp::headless(Project::demo());
        next.recovery = Some(recovery.clone());
        next.answers.push_back(crate::commands::Answer::Save);
        assert!(next.offer_recovery());
        assert_eq!(next.project.tempo, 141.0);
        assert_eq!(next.file.as_deref(), Some(file.as_path()));
        assert!(next.is_dirty(), "still to be saved");
        assert!(next.status.as_deref().unwrap().contains("Recuperados los cambios de «Tema»"), "{:?}", next.status);
        assert!(recovery.pending().is_some(), "kept until saved");
        next.save_to_path(&file);
        assert_eq!(recovery.pending(), None, "saved: nothing to recover");
        assert_eq!(Project::load(&file).unwrap().tempo, 141.0);
        let _ = std::fs::remove_dir_all(&recovery.dir);
        let _ = std::fs::remove_dir_all(file.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn discarded_changes_are_kept_once_and_not_offered_again() {
        let recovery = scratch("discard");
        let (mut app, file) = app_with(&recovery, "discard");
        app.project.tempo = 99.0;
        autosave_now(&mut app);
        let mut next = PulseApp::headless(Project::demo());
        next.recovery = Some(recovery.clone());
        next.answers.push_back(crate::commands::Answer::Discard);
        assert!(!next.offer_recovery());
        assert_eq!(recovery.pending(), None);
        assert!(!next.offer_recovery(), "not asked again");
        assert_eq!(Project::load(&recovery.dir.join(DISCARDED)).unwrap().tempo, 99.0);
        let _ = std::fs::remove_dir_all(&recovery.dir);
        let _ = std::fs::remove_dir_all(file.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn opening_another_project_or_quitting_leaves_nothing_to_recover() {
        let recovery = scratch("replace");
        let (mut app, file) = app_with(&recovery, "replace");
        app.project.tempo = 101.0;
        autosave_now(&mut app);
        assert!(recovery.pending().is_some());
        // «No guardar» and a new project: those changes were let go.
        app.new_project();
        assert_eq!(recovery.pending(), None);
        app.project.tempo = 102.0;
        autosave_now(&mut app);
        assert!(recovery.pending().is_some());
        app.clear_recovery();
        assert_eq!(recovery.pending(), None);
        let _ = std::fs::remove_dir_all(&recovery.dir);
        let _ = std::fs::remove_dir_all(file.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_project_that_closed_pulse_while_reopening_is_not_reopened_again() {
        let recovery = scratch("guard");
        let (app, file) = app_with(&recovery, "guard");
        drop(app);
        let mut first = PulseApp::headless(Project::empty());
        first.recovery = Some(recovery.clone());
        first.reopen_guarded(&file);
        assert_eq!(first.file.as_deref(), Some(file.as_path()));
        // It closed before settling: the note stays, and the next start skips it.
        assert_eq!(recovery.interrupted_opening().as_deref(), Some(file.as_path()));
        let mut second = PulseApp::headless(Project::empty());
        second.recovery = Some(recovery.clone());
        second.reopen_guarded(&file);
        assert_eq!(second.file, None);
        assert!(second.status.as_deref().unwrap().contains("No se volvió a abrir «Tema»"), "{:?}", second.status);
        // Only once: the start after that tries again.
        let mut third = PulseApp::headless(Project::empty());
        third.recovery = Some(recovery.clone());
        third.reopen_guarded(&file);
        assert_eq!(third.file.as_deref(), Some(file.as_path()));
        let _ = std::fs::remove_dir_all(&recovery.dir);
        let _ = std::fs::remove_dir_all(file.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn an_opening_that_never_finished_is_remembered() {
        let recovery = scratch("opening");
        assert_eq!(recovery.interrupted_opening(), None);
        recovery.opening(Path::new("/Música/Pulse/Proyectos/Tema ñ/Tema ñ.pulse"));
        assert_eq!(recovery.interrupted_opening(), Some(PathBuf::from("/Música/Pulse/Proyectos/Tema ñ/Tema ñ.pulse")));
        recovery.opened();
        assert_eq!(recovery.interrupted_opening(), None);
        let _ = std::fs::remove_dir_all(&recovery.dir);
    }
}
