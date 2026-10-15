//! The app's commands, as in any program's menus: files (new, open, recent, revert,
//! save, import, export, quit), editing (undo, clipboard, duplicate, delete, select
//! all), creating tracks, the views and the transport. The menu bar, the keyboard
//! shortcuts and the in-app menu all run them from here.
//!
//! Anything that would throw away unsaved work asks first.

use crate::app::{Detail, PulseApp, View};
use crate::theme::TEXT_DIM;
use eframe::egui;
use engine::Target;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    New,
    Open,
    OpenRecent(usize),
    ClearRecent,
    Revert,
    Save,
    SaveAs,
    ImportLive,
    ImportAudio,
    Export,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
    SelectAll,
    NewMidiTrack,
    NewAudioTrack,
    DuplicateTrack,
    DeleteTrack,
    ShowSession,
    ShowArrangement,
    ToggleDetail,
    ToggleBrowser,
    FullScreen,
    PlayStop,
    Record,
    Loop,
    Metronome,
    ComputerKeyboard,
    MidiAssign,
    Shortcuts,
    About,
    Settings,
}

/// How the question about unsaved changes was answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Save,
    Discard,
    Cancel,
}

/// The most recent projects, kept for the next time Pulse starts.
pub const MAX_RECENT: usize = 10;

fn recent_path() -> PathBuf {
    crate::kits::kits_dir().with_file_name("recent.json")
}

/// The saved list of recent projects that still exist.
pub fn load_recent() -> Vec<PathBuf> {
    let list: Vec<PathBuf> = std::fs::read_to_string(recent_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    list.into_iter().filter(|p| p.exists()).take(MAX_RECENT).collect()
}

/// The keyboard shortcuts, for the help window and the menus: (keys, what they do).
pub const SHORTCUTS: [(&str, &str); 29] = [
    ("⌘N", "Proyecto nuevo"),
    ("⌘O", "Abrir"),
    ("⌘S / ⇧⌘S", "Guardar / Guardar como"),
    ("⌘I", "Importar set de Live"),
    ("⇧⌘R", "Exportar audio"),
    ("⌘Q", "Salir"),
    ("⌘,", "Ajustes (audio, MIDI, plugins)"),
    ("⌘Z / ⇧⌘Z", "Deshacer / Rehacer"),
    ("⌘X / ⌘C / ⌘V", "Cortar / Copiar / Pegar los clips seleccionados (o las notas seleccionadas en el editor)"),
    ("⌘D", "Duplicar"),
    ("⌫", "Eliminar"),
    ("⌘A", "Seleccionar todas las notas"),
    ("⌘T / ⇧⌘T", "Nueva pista MIDI / de audio"),
    ("⌘ clic / ⇧ clic", "Seleccionar varias pistas (en sus cabeceras) o varios clips"),
    ("Arrastrar cabecera", "Mover las pistas seleccionadas"),
    ("Arrastrar en un hueco", "Seleccionar clips y pistas con un recuadro"),
    ("Arrastrar clip / ⌥", "Mover los clips seleccionados / copiarlos"),
    ("Tab", "Cambiar entre Session y Arrangement"),
    ("⇧Tab", "Cambiar entre el clip y los dispositivos"),
    ("⌃⌘F", "Pantalla completa"),
    ("Espacio", "Reproducir / Parar"),
    ("F9", "Grabar en el Arrangement"),
    ("M", "Teclado del ordenador como teclado MIDI"),
    ("A S D F G H J K L", "Teclas blancas (con el teclado del ordenador)"),
    ("W E T Y U O P", "Teclas negras"),
    ("Z / X", "Octava abajo / arriba"),
    ("C / V", "Velocidad abajo / arriba"),
    ("⌘U", "Cuantizar notas (editor de clips)"),
    ("B", "Lápiz (editor de clips)"),
];

impl PulseApp {
    /// Whether `command` can run now (for greying out menu items).
    pub fn command_enabled(&self, command: Command) -> bool {
        let clip_selected = self.selected_clip.is_some_and(|c| self.clip(c).is_some());
        let editing_notes = self.detail == Detail::Clip && clip_selected;
        let track = matches!(self.selected_target, Target::Track(_));
        // While typing, ⌘Z, ⌘X, ⌘C, ⌘V and ⌘A belong to the text field.
        let editing = matches!(command, Command::Undo | Command::Redo | Command::Cut | Command::Copy | Command::Paste | Command::SelectAll | Command::Duplicate | Command::Delete);
        if editing && self.text_focus {
            return false;
        }
        match command {
            Command::Undo => self.can_undo(),
            Command::Redo => self.can_redo(),
            Command::Revert => self.file.is_some() && self.is_dirty(),
            Command::OpenRecent(i) => i < self.recent.len(),
            Command::ClearRecent => !self.recent.is_empty(),
            Command::Cut | Command::Copy | Command::Duplicate | Command::Delete => clip_selected || editing_notes,
            Command::Paste => self.clip_clipboard.is_some() || editing_notes,
            Command::SelectAll => editing_notes,
            Command::DuplicateTrack | Command::DeleteTrack => track,
            _ => true,
        }
    }

    /// Whether an editing command goes to the notes in the clip editor rather than
    /// to the clips: only with one clip open, and notes selected (or, for pasting,
    /// notes copied last).
    fn edits_notes(&self, command: Command) -> bool {
        if self.detail != Detail::Clip || self.clip_selection().len() > 1 {
            return false;
        }
        match command {
            Command::SelectAll => true,
            Command::Paste => self.paste_notes || self.clip_clipboard.is_none(),
            _ => self.piano_roll.has_selection(),
        }
    }

    /// Runs `command` (on the UI thread, at the start of a frame).
    pub fn run(&mut self, command: Command, ctx: &egui::Context) {
        if !self.command_enabled(command) {
            return;
        }
        match command {
            Command::New => {
                if self.may_discard() {
                    self.new_project();
                }
            }
            Command::Open => {
                if self.may_discard() {
                    self.open();
                }
            }
            Command::OpenRecent(i) => {
                if let Some(path) = self.recent.get(i).cloned() {
                    if self.may_discard() {
                        self.open_path(&path);
                    }
                }
            }
            Command::ClearRecent => {
                self.recent.clear();
                self.save_recent();
            }
            Command::Revert => {
                if let Some(path) = self.file.clone() {
                    if self.ask_discard(&format!("¿Volver a la versión guardada de «{}»?", self.file_name()), "Se perderán los cambios desde la última vez que guardaste.", "Volver") {
                        self.open_path(&path);
                    }
                }
            }
            Command::Save => self.save(),
            Command::SaveAs => self.save_as(),
            Command::ImportLive => {
                if self.may_discard() {
                    self.import_live_set();
                }
            }
            Command::ImportAudio => self.choose_samples(crate::app::Placement::Auto),
            Command::Export => self.export.open = true,
            Command::Quit => {
                if self.may_discard() {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
            // In the clip editor, with notes selected (or notes copied last, for
            // pasting), these act on notes: it reads them as editing events.
            Command::Cut | Command::Copy | Command::Paste | Command::SelectAll if self.edits_notes(command) => {
                if matches!(command, Command::Cut | Command::Copy) {
                    self.paste_notes = true;
                }
                let event = match command {
                    Command::Cut => egui::Event::Cut,
                    Command::Copy => egui::Event::Copy,
                    Command::Paste => egui::Event::Paste(String::new()),
                    _ => egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND },
                };
                ctx.input_mut(|i| i.events.push(event));
            }
            Command::Duplicate if self.edits_notes(command) => {
                ctx.input_mut(|i| i.events.push(egui::Event::Key { key: egui::Key::D, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }));
            }
            Command::Delete if self.edits_notes(command) => {
                ctx.input_mut(|i| i.events.push(egui::Event::Key { key: egui::Key::Delete, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }));
            }
            Command::Cut => {
                self.paste_notes = false;
                self.copy_clips();
                self.delete_selected_clip();
            }
            Command::Copy => {
                self.paste_notes = false;
                self.copy_clips();
            }
            Command::Paste => self.paste_clips(),
            Command::Duplicate => self.duplicate_selected_clip(),
            Command::Delete => self.delete_selected_clip(),
            Command::SelectAll => {}
            Command::NewMidiTrack => self.add_track(),
            Command::NewAudioTrack => {
                self.add_audio_track();
            }
            Command::DuplicateTrack => self.duplicate_selected_tracks(),
            Command::DeleteTrack => self.delete_selected_tracks(),
            Command::ShowSession => self.view = View::Session,
            Command::ShowArrangement => self.view = View::Arrangement,
            Command::ToggleDetail => {
                self.detail = match self.detail {
                    Detail::Clip => Detail::Devices,
                    Detail::Devices => Detail::Clip,
                }
            }
            Command::ToggleBrowser => self.show_browser = !self.show_browser,
            Command::FullScreen => {
                let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
            }
            Command::PlayStop => self.toggle_play(),
            Command::Record => self.toggle_arrangement_record(),
            Command::Loop => self.transport(crate::midi_map::Transport::Loop),
            Command::Metronome => self.transport(crate::midi_map::Transport::Metronome),
            Command::ComputerKeyboard => self.toggle_keyboard(),
            Command::MidiAssign => self.toggle_learn(),
            Command::Shortcuts => self.show_shortcuts = true,
            Command::About => crate::native_menu::about(),
            Command::Settings => self.open_settings(),
        }
    }

    // ----- Unsaved changes ----------------------------------------------------------

    /// True if the current project may be replaced or closed: saved, or the user chose
    /// to save it or to discard its changes.
    pub fn may_discard(&mut self) -> bool {
        if !self.is_dirty() {
            return true;
        }
        let title = format!("¿Guardar los cambios de «{}»?", self.file_name());
        match self.ask(&title, "Si no los guardas, se perderán.") {
            Answer::Save => {
                self.save();
                !self.is_dirty()
            }
            Answer::Discard => true,
            Answer::Cancel => false,
        }
    }

    /// Asks before throwing changes away (for "Revert"): true to go ahead.
    fn ask_discard(&mut self, title: &str, description: &str, action: &str) -> bool {
        if let Some(answer) = self.answers.pop_front() {
            return answer == Answer::Discard;
        }
        if !self.interactive {
            return false;
        }
        let result = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title(title)
            .set_description(description)
            .set_buttons(rfd::MessageButtons::OkCancelCustom(action.into(), "Cancelar".into()))
            .show();
        matches!(result, rfd::MessageDialogResult::Ok) || result == rfd::MessageDialogResult::Custom(action.into())
    }

    /// Save, don't save or cancel. Tests and headless apps answer from `answers`.
    fn ask(&mut self, title: &str, description: &str) -> Answer {
        if let Some(answer) = self.answers.pop_front() {
            return answer;
        }
        if !self.interactive {
            return Answer::Cancel;
        }
        let result = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title(title)
            .set_description(description)
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom("Guardar".into(), "No guardar".into(), "Cancelar".into()))
            .show();
        match result {
            rfd::MessageDialogResult::Yes => Answer::Save,
            rfd::MessageDialogResult::No => Answer::Discard,
            rfd::MessageDialogResult::Custom(label) if label == "Guardar" => Answer::Save,
            rfd::MessageDialogResult::Custom(label) if label == "No guardar" => Answer::Discard,
            _ => Answer::Cancel,
        }
    }

    /// The window's close button and ⌘Q: ask about unsaved changes first.
    pub fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            if self.may_discard() {
                self.quitting = true;
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
    }

    // ----- Recent projects ----------------------------------------------------------

    /// Puts `path` first in the recent projects.
    pub fn remember_recent(&mut self, path: &Path) {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.recent.retain(|p| *p != path);
        self.recent.insert(0, path);
        self.recent.truncate(MAX_RECENT);
        self.save_recent();
    }

    fn save_recent(&self) {
        if !self.interactive {
            return;
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.recent) {
            let path = recent_path();
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
            let _ = std::fs::write(path, text);
        }
    }
}

/// The help window with every keyboard shortcut.
pub fn shortcuts_window(app: &mut PulseApp, ctx: &egui::Context) {
    let mut open = app.show_shortcuts;
    egui::Window::new("Atajos de teclado").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
        egui::Grid::new("shortcuts").num_columns(2).striped(true).show(ui, |ui| {
            for (keys, what) in SHORTCUTS {
                ui.label(egui::RichText::new(keys).strong());
                ui.label(what);
                ui.end_row();
            }
        });
        ui.label(egui::RichText::new("Los atajos con ⌘ también están en la barra de menús.").color(TEXT_DIM).size(10.5));
    });
    app.show_shortcuts = open;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ClipRef;
    use crate::project::Project;

    fn app() -> PulseApp {
        PulseApp::headless(Project::demo())
    }

    fn ctx() -> egui::Context {
        egui::Context::default()
    }

    #[test]
    fn unsaved_changes_are_asked_about_before_they_are_lost() {
        let mut app = app();
        app.project.tempo = 99.0;
        assert!(app.is_dirty());
        // Cancel keeps everything as it was.
        app.answers.push_back(Answer::Cancel);
        app.run(Command::New, &ctx());
        assert_eq!(app.project.tempo, 99.0);
        // Not saving goes ahead.
        app.answers.push_back(Answer::Discard);
        app.run(Command::New, &ctx());
        assert_eq!(app.project, Project::empty());
        // Nothing unsaved: no question.
        app.run(Command::New, &ctx());
        assert!(app.answers.is_empty());
        // Quitting asks too; cancelling keeps the window.
        app.project.tempo = 98.0;
        app.answers.push_back(Answer::Cancel);
        app.run(Command::Quit, &ctx());
        assert!(!app.quitting);
        app.answers.push_back(Answer::Discard);
        app.run(Command::Quit, &ctx());
        assert!(app.quitting);
    }

    #[test]
    fn dropping_a_project_on_the_window_asks_first() {
        let mut app = app();
        let file = std::env::temp_dir().join(format!("pulse-drop-{}.pulse", std::process::id()));
        let mut other = Project::empty();
        other.tempo = 77.0;
        other.save(&file).unwrap();
        app.project.tempo = 99.0;
        let drop = |app: &mut PulseApp| {
            let ctx = ctx();
            #[derive(Debug)]
            struct Dropped(PathBuf);
            impl egui::DroppedFile for Dropped {
                fn path(&self) -> &Path {
                    &self.0
                }
                fn bytes(&self) -> Result<Vec<u8>, String> {
                    std::fs::read(&self.0).map_err(|e| e.to_string())
                }
            }
            let input = egui::RawInput { dropped_files: vec![std::sync::Arc::new(Dropped(file.clone()))], ..Default::default() };
            let _ = ctx.run_ui(input, |ui| app.handle_dropped_files(ui.ctx()));
        };
        app.answers.push_back(Answer::Cancel);
        drop(&mut app);
        assert_eq!(app.project.tempo, 99.0, "kept");
        app.answers.push_back(Answer::Discard);
        drop(&mut app);
        assert_eq!(app.project.tempo, 77.0, "opened");
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn recent_projects_open_and_revert() {
        let mut app = app();
        let dir = std::env::temp_dir().join(format!("pulse-recent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.pulse"), dir.join("b.pulse"));
        app.project.tempo = 101.0;
        app.save_to_path(&a);
        app.project.tempo = 102.0;
        app.save_to_path(&b);
        assert_eq!(app.recent.len(), 2);
        assert!(app.recent[0].ends_with("b.pulse"), "the newest first");
        app.run(Command::OpenRecent(1), &ctx());
        assert_eq!(app.project.tempo, 101.0);
        assert!(app.recent[0].ends_with("a.pulse"));
        // Revert: back to the saved file, after asking.
        app.project.tempo = 140.0;
        assert!(app.command_enabled(Command::Revert));
        app.answers.push_back(Answer::Discard);
        app.run(Command::Revert, &ctx());
        assert_eq!(app.project.tempo, 101.0);
        assert!(!app.command_enabled(Command::Revert), "nothing to revert");
        app.run(Command::ClearRecent, &ctx());
        assert!(app.recent.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clips_are_cut_copied_and_pasted() {
        let mut app = app();
        app.detail = Detail::Devices;
        let source = ClipRef::Session { track: 0, slot: 0 };
        let original = app.clip(source).cloned().expect("the demo has a clip there");
        app.select_clip(source);
        app.detail = Detail::Devices;
        assert!(!app.command_enabled(Command::Paste));
        app.run(Command::Copy, &ctx());
        // Into the next free slot below.
        app.run(Command::Paste, &ctx());
        let Some(ClipRef::Session { track: 0, slot }) = app.selected_clip else { panic!("{:?}", app.selected_clip) };
        assert!(slot > 0);
        assert_eq!(app.project.tracks[0].clips[slot], Some(original.clone()));
        // Cut removes it, and it pastes back.
        app.detail = Detail::Devices;
        app.run(Command::Cut, &ctx());
        assert_eq!(app.project.tracks[0].clips[slot], None);
        app.select_clip(ClipRef::Session { track: 0, slot });
        app.detail = Detail::Devices;
        app.run(Command::Paste, &ctx());
        assert_eq!(app.project.tracks[0].clips[slot], Some(original.clone()));
        // In the arrangement, after the selected clip.
        app.project.tracks[0].arrangement = vec![engine::model::ArrangementClip { start: 0.0, length: 4.0, clip: original.clone() }];
        app.select_clip(ClipRef::Arrangement { track: 0, index: 0 });
        app.detail = Detail::Devices;
        app.run(Command::Paste, &ctx());
        assert_eq!(app.project.tracks[0].arrangement.len(), 2);
        assert_eq!(app.project.tracks[0].arrangement[1].start, 4.0);
    }

    #[test]
    fn in_the_clip_editor_the_clipboard_acts_on_notes() {
        let mut app = app();
        app.select_clip(ClipRef::Session { track: 0, slot: 0 });
        assert_eq!(app.detail, Detail::Clip);
        // With no notes selected, ⌘C copies the clip.
        app.run(Command::Copy, &ctx());
        assert!(app.clip_clipboard.take().is_some());
        app.piano_roll.select_notes(ClipRef::Session { track: 0, slot: 0 }, &[0]);
        let ctx = ctx();
        let mut copied = Vec::new();
        let _ = ctx.run_ui(Default::default(), |ui| {
            app.run(Command::Copy, ui.ctx());
            copied = ui.input(|i| i.events.clone());
        });
        assert_eq!(copied, [egui::Event::Copy], "the note editor gets the copy");
        assert!(app.clip_clipboard.is_none(), "no clip was copied");
        // ⌘V then pastes notes, even with a clip on the clipboard.
        app.clip_clipboard = Some(crate::clip_select::Clipboard { items: Vec::new() });
        let mut pasted = Vec::new();
        let _ = ctx.run_ui(Default::default(), |ui| {
            app.run(Command::Paste, ui.ctx());
            pasted = ui.input(|i| i.events.clone());
        });
        assert_eq!(pasted, [egui::Event::Paste(String::new())]);
    }

    #[test]
    fn while_typing_the_editing_shortcuts_go_to_the_text_field() {
        let mut app = app();
        app.select_clip(ClipRef::Session { track: 0, slot: 0 });
        app.project.tempo = 96.0;
        app.commit_history();
        assert!(app.command_enabled(Command::Copy) && app.command_enabled(Command::Undo));
        app.text_focus = true;
        assert!(!app.command_enabled(Command::Copy) && !app.command_enabled(Command::Paste) && !app.command_enabled(Command::Undo));
        assert!(app.command_enabled(Command::Save), "the rest still work");
    }

    #[test]
    fn menus_are_greyed_out_when_they_cannot_act() {
        let mut app = app();
        assert!(!app.command_enabled(Command::Undo));
        assert!(!app.command_enabled(Command::Revert), "no file");
        app.selected_clip = None;
        assert!(!app.command_enabled(Command::Copy));
        app.project.tempo = 97.0;
        app.commit_history();
        assert!(app.command_enabled(Command::Undo));
        let ctx = ctx();
        app.show_shortcuts = true;
        let _ = ctx.run_ui(Default::default(), |ui| shortcuts_window(&mut app, ui.ctx()));
        assert!(app.show_shortcuts);
    }
}
