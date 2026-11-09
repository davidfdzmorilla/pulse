//! «¿Qué quieres hacer?»: what Pulse shows when it starts without a project
//! (and from Archivo › ¿Qué quieres hacer?…). Ways in, each a click: the
//! example song, humming an idea, remixing a song, a template, a recent
//! project, a lesson. A box (and Ajustes) stops it showing at the start.

use crate::app::{PulseApp, View};
use crate::i18n::{fill, tr};
use crate::project::{Project, Track, PALETTE};
use crate::theme::{self, ACCENT, CONTROL, CONTROL_HOVER, TEXT, TEXT_DIM};
use eframe::egui;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct Welcome {
    pub open: bool,
}

/// What can be chosen in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Choice {
    Example,
    Hum,
    Remix,
    Empty,
    Template(usize),
    Open,
    Recent(usize),
    Lesson(usize),
}

impl PulseApp {
    /// Pulse started without a project: the example song, and this window
    /// over it if wanted.
    pub fn start_fresh(&mut self) {
        self.open_example();
        self.welcome.open = self.settings.welcome_at_start;
    }

    /// Archivo › ¿Qué quieres hacer?…
    pub fn show_welcome(&mut self) {
        self.welcome.open = true;
    }

    /// Does what was chosen, asking first about unsaved changes when it replaces the project.
    pub fn welcome_choose(&mut self, choice: Choice) {
        let replaces = !matches!(choice, Choice::Lesson(_) | Choice::Open | Choice::Remix);
        if replaces && !self.may_discard() {
            return;
        }
        self.welcome.open = false;
        match choice {
            Choice::Example => self.open_example(),
            Choice::Hum => self.start_humming(),
            Choice::Remix => self.start_remix(),
            Choice::Empty => self.new_project(),
            Choice::Template(i) => self.new_from_template(i),
            Choice::Open => self.run(crate::commands::Command::Open, &egui::Context::default()),
            Choice::Recent(i) => {
                if let Some(path) = self.recent.get(i).cloned() {
                    self.open_path(&path);
                }
            }
            Choice::Lesson(i) => self.start_lesson(i),
        }
    }

    /// The example song, in the Arrangement, nothing to save yet.
    pub fn open_example(&mut self) {
        match crate::example::song() {
            Ok(song) => {
                self.replace_project(song, None);
                self.view = View::Arrangement;
                self.status = Some(theme::keys(&fill(tr("Canción de ejemplo «{}»: Espacio para oírla; en la Session (Tab) lanza sus escenas"), &[&crate::example::NAME])));
            }
            Err(error) => self.status = Some(fill(tr("No se pudo preparar la canción de ejemplo: {}"), &[&error])),
        }
    }

    /// A project to hum into: an audio track armed, the metronome on, and the
    /// lesson that goes from there to a song.
    pub fn start_humming(&mut self) {
        let mut project = Project::empty();
        project.tempo = 100.0;
        project.metronome = true;
        let mut voice = Track::new_audio("Tarareo", PALETTE[0]);
        voice.armed = true;
        project.tracks = vec![voice];
        self.replace_project(project, None);
        self.view = View::Arrangement;
        if let Some(lesson) = crate::lessons::find("tarareo") {
            self.start_lesson(lesson);
        }
        self.status = Some(theme::keys(tr("Pista «Tarareo» armada: F9 graba, Espacio para")));
    }

    /// «Remezclar una canción»: choose its file.
    fn start_remix(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter("Audio", &crate::app::AUDIO_EXTENSIONS);
        if let Some(path) = crate::files::pick_file(dialog) {
            if self.may_discard() && self.remix_from(&path) {
                self.separate_stems(crate::app::ClipRef::Arrangement { track: 0, index: 0 });
            }
        }
    }

    /// A new project with the song `path` on an audio track of the
    /// Arrangement, to separate into stems: whether it could be read.
    pub(crate) fn remix_from(&mut self, path: &Path) -> bool {
        let mut project = Project::empty();
        let name = path.file_stem().map_or("Canción".into(), |s| s.to_string_lossy().into_owned());
        project.tracks = vec![Track::new_audio(name, PALETTE[3])];
        self.replace_project(project, None);
        self.view = View::Arrangement;
        self.add_samples(vec![PathBuf::from(path)], crate::app::Placement::Arrangement { track: 0, start: 0.0 });
        let placed = !self.project.tracks[0].arrangement.is_empty();
        if placed {
            // Nothing to save yet: the song alone is not work.
            self.saved = self.project.clone();
        }
        placed
    }
}

/// A box to click: a title, a line under it, and a mark at its right (a ✓),
/// at least `min_height` tall.
pub fn card(ui: &mut egui::Ui, title: &str, line: &str, mark: &str, width: f32, min_height: f32) -> egui::Response {
    let font = theme::body();
    let small = egui::FontId::proportional(11.0);
    let inner = width - 20.0;
    let title_galley = ui.painter().layout(title.to_string(), theme::semibold(13.0), TEXT, inner - 16.0);
    let line_galley = ui.painter().layout(line.to_string(), small, TEXT_DIM, inner);
    let height = (title_galley.size().y + line_galley.size().y + 18.0).max(min_height);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let fill = if response.hovered() { CONTROL_HOVER } else { CONTROL };
    ui.painter().rect_filled(rect, 6.0, fill);
    if response.hovered() {
        ui.painter().rect_stroke(rect, 6.0, egui::Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
    }
    let top = rect.min + egui::vec2(10.0, 8.0);
    let title_height = title_galley.size().y;
    ui.painter().galley(top, title_galley, TEXT);
    ui.painter().galley(top + egui::vec2(0.0, title_height + 2.0), line_galley, TEXT_DIM);
    if !mark.is_empty() {
        ui.painter().text(egui::pos2(rect.right() - 10.0, rect.top() + 8.0), egui::Align2::RIGHT_TOP, mark, font, theme::PLAY);
    }
    let label = format!("{title} · {line}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A heading over a group of choices.
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(10.0);
    ui.label(egui::RichText::new(text).font(theme::semibold(12.0)).color(TEXT_DIM));
    ui.add_space(2.0);
}

pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    if !app.welcome.open {
        return;
    }
    let mut open = true;
    let mut chosen = None;
    let narrow = ctx.content_rect().width() < 1100.0;
    let width = if narrow { 640.0 } else { 760.0 };
    egui::Window::new(tr("¿Qué quieres hacer?"))
        .id(egui::Id::new("welcome"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size([width, 0.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(tr("Empieza por donde quieras: todo se puede cambiar después.")).color(TEXT_DIM));
            ui.add_space(6.0);
            let gap = ui.spacing().item_spacing.x;
            let third = (width - 2.0 * gap) / 3.0;
            ui.horizontal_top(|ui| {
                let ways = [
                    (Choice::Example, tr("Escuchar el ejemplo"), fill(tr("«{}»: un tema corto con escenas para lanzar y la canción montada, para oír y desmontar"), &[&crate::example::NAME])),
                    (Choice::Hum, tr("Tararear una idea"), tr("Graba tu melodía y Pulse la convierte en canción: melodía, acordes, bajo y batería").to_string()),
                    (Choice::Remix, tr("Remezclar una canción"), tr("Elige un audio y Pulse lo separa en batería, bajo, voz y el resto, cada uno en su pista").to_string()),
                ];
                for (choice, title, line) in ways {
                    if card(ui, title, &line, "", third, 86.0).clicked() {
                        chosen = Some(choice);
                    }
                }
            });

            heading(ui, tr("EMPEZAR DE UNA PLANTILLA"));
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr(crate::templates::EMPTY)).on_hover_text(tr("Dos pistas MIDI y nada más")).clicked() {
                    chosen = Some(Choice::Empty);
                }
                for (i, template) in app.templates.iter().enumerate() {
                    let about = if template.file.is_some() { tr("Una plantilla tuya") } else { tr(crate::templates::about(&template.name)) };
                    if ui.button(template.shown()).on_hover_text(about).clicked() {
                        chosen = Some(Choice::Template(i));
                    }
                }
            });

            ui.columns(2, |columns| {
                let ui = &mut columns[0];
                heading(ui, tr("APRENDER"));
                for (i, lesson) in crate::lessons::LESSONS.iter().enumerate() {
                    let finished = app.settings.lessons_done.iter().any(|d| d == lesson.id);
                    let text = format!("{}. {}{}", i + 1, tr(lesson.title), if finished { "  ✓" } else { "" });
                    if ui.link(text).on_hover_text(tr(lesson.about)).clicked() {
                        chosen = Some(Choice::Lesson(i));
                    }
                }
                let ui = &mut columns[1];
                heading(ui, tr("ABRIR"));
                for (i, path) in app.recent.iter().take(6).enumerate() {
                    let name = path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned());
                    if ui.link(name).on_hover_text(path.display().to_string()).clicked() {
                        chosen = Some(Choice::Recent(i));
                    }
                }
                if app.recent.is_empty() {
                    ui.label(egui::RichText::new(tr("Aún no hay proyectos recientes")).color(TEXT_DIM));
                }
                if ui.button(tr("Abrir un proyecto…")).clicked() {
                    chosen = Some(Choice::Open);
                }
            });

            ui.add_space(10.0);
            ui.separator();
            ui.horizontal(|ui| {
                let mut at_start = app.settings.welcome_at_start;
                if ui.checkbox(&mut at_start, tr("Mostrar al arrancar")).on_hover_text(tr("Si no, Pulse empieza con la canción de ejemplo o el último proyecto. Siempre está en Archivo › ¿Qué quieres hacer?…")).changed() {
                    let mut settings = app.settings.clone();
                    settings.welcome_at_start = at_start;
                    app.set_settings(settings);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("Cerrar")).clicked() {
                        app.welcome.open = false;
                    }
                    if ui.button(tr("Abrir la guía")).on_hover_text(tr("Primeros pasos, en la guía de Pulse")).clicked() {
                        app.welcome.open = false;
                        app.guide.open_at("Primeros pasos");
                    }
                });
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    if !open {
        app.welcome.open = false;
    }
    if let Some(choice) = chosen {
        app.welcome_choose(choice);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::produccion::Studio;

    fn shots() -> PathBuf {
        let dir = std::env::var_os("PULSE_SHOTS").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join(format!("pulse-bienvenida-{}", std::process::id())));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Each way in does what it says: the example, a template, humming (with
    /// its lesson), a lesson on the project as it is.
    #[test]
    fn each_choice_starts_what_it_says() {
        let mut app = PulseApp::headless(Project::empty());
        app.templates = crate::templates::all();
        app.welcome.open = true;
        app.welcome_choose(Choice::Example);
        assert!(!app.welcome.open);
        assert_eq!(app.project.markers.len(), 7, "the example's song");
        assert!(!app.is_dirty(), "nothing to save yet");
        assert!(app.view == View::Arrangement);

        let house = app.templates.iter().position(|t| t.name == "House").unwrap();
        app.welcome_choose(Choice::Template(house));
        assert_eq!(app.project.tempo, 124.0);
        assert!(app.project.tracks.iter().all(|t| t.clips.iter().all(Option::is_none)));

        app.welcome_choose(Choice::Hum);
        assert!(app.project.metronome && app.project.tracks[0].armed && app.project.tracks[0].is_audio());
        assert_eq!(app.lessons.current, crate::lessons::find("tarareo"));
        assert_eq!(app.lessons.progress(), (2, 4), "metronome and armed track already there");

        app.project.tempo = 90.0;
        app.welcome_choose(Choice::Lesson(0));
        assert_eq!(app.project.tempo, 90.0, "a lesson keeps the project");
        assert_eq!(app.lessons.current, Some(0));

        // With unsaved changes, cancelling keeps them.
        app.answers.push_back(crate::commands::Answer::Cancel);
        app.welcome.open = true;
        app.welcome_choose(Choice::Empty);
        assert_eq!(app.project.tempo, 90.0);
        assert!(app.welcome.open);
    }

    /// «Remezclar»: the song on a track of its own, at the start of the Arrangement.
    #[test]
    fn a_remix_starts_from_the_song_alone() {
        let mut app = PulseApp::headless(Project::empty());
        let song = crate::test_audio::demo_song();
        assert!(app.remix_from(&song));
        assert_eq!(app.project.tracks.len(), 1);
        assert!(matches!(app.project.tracks[0].arrangement[0].clip, engine::model::Clip::Audio(_)));
        assert_eq!(app.project.tracks[0].arrangement[0].start, 0.0);
        assert!(!app.is_dirty());
        assert!(!app.remix_from(Path::new("/no/existe.wav")));
    }

    /// In English: the welcome, the menus' window and Ajustes, with the language
    /// choice in it. Pictures in `PULSE_SHOTS=dir`.
    #[test]
    fn the_first_windows_show_in_english() {
        let dir = shots();
        crate::i18n::with(crate::i18n::Language::English, || {
            let mut app = PulseApp::headless(Project::empty());
            app.templates = crate::templates::all();
            app.start_fresh();
            let mut s = Studio::new(app, dir.clone());
            s.frames(4);
            for text in ["What would you like to do?", "Listen to the example", "Hum an idea", "Remix a song", "START FROM A TEMPLATE", "Show at startup", "Open the guide"] {
                assert!(s.has(text), "falta «{text}»");
            }
            assert!(!s.has("Escuchar el ejemplo"));
            let shot = s.shot("bienvenida-en");
            std::fs::copy(&shot, dir.join("bienvenida-en.png")).unwrap();
            s.click("Close");
            s.app.open_settings();
            s.frames(4);
            for text in ["Settings", "Idioma · Language", "Output", "Sample rate", "Buffer size", "Compensate for effect latency", "Grid controllers", "Pulse folder", "Open the last project at startup", "Check for updates at startup"] {
                assert!(s.has(text), "falta «{text}»");
            }
            let shot = s.shot("ajustes-en");
            std::fs::copy(&shot, dir.join("ajustes-en.png")).unwrap();
        });
    }

    /// The main window in English, with the example song: the transport, the
    /// browser, the Arrangement, the Session and a track's devices. Pictures
    /// with `PULSE_SHOTS=dir`.
    #[test]
    fn the_main_window_shows_in_english() {
        let dir = shots();
        crate::i18n::with(crate::i18n::Language::English, || {
            let mut app = PulseApp::headless(Project::empty());
            app.templates = crate::templates::all();
            app.start_fresh();
            app.welcome.open = false;
            app.view = View::Arrangement;
            app.show_browser = true;
            let mut s = Studio::new(app, dir.clone());
            s.frames(6);
            for text in ["Browser", "INSTRUMENTS", "AUDIO EFFECTS", "Keyboard", "Voice", "Map"] {
                assert!(s.has(text), "falta «{text}»");
            }
            assert!(!s.has("Navegador") && !s.has("INSTRUMENTOS"));
            let shot = s.shot("arrangement-en");
            std::fs::copy(&shot, dir.join("arrangement-en.png")).unwrap();
            s.app.view = View::Session;
            s.frames(6);
            for text in ["Browser", "Devices", "Maximize", "Save Chain…"] {
                assert!(s.has(text), "falta «{text}»");
            }
            let shot = s.shot("session-en");
            std::fs::copy(&shot, dir.join("session-en.png")).unwrap();
            s.app.selected_target = engine::Target::Track(0);
            s.app.detail = crate::app::Detail::Devices;
            s.frames(6);
            let shot = s.shot("dispositivos-en");
            std::fs::copy(&shot, dir.join("dispositivos-en.png")).unwrap();
        });
    }

    /// Starting without a project: the example behind the window, unless it
    /// is turned off. Its box changes the setting. Pictures of the window
    /// (`PULSE_SHOTS=dir`), wide and narrow.
    #[test]
    fn the_welcome_shows_at_the_start_and_its_box_turns_it_off() {
        let dir = shots();
        let mut app = PulseApp::headless(Project::empty());
        app.templates = crate::templates::all();
        app.settings.lessons_done = vec!["ritmo".into()];
        app.start_fresh();
        assert!(app.welcome.open);
        assert_eq!(app.project.markers.len(), 7, "the example behind it");
        let mut s = Studio::new(app, dir.clone());
        s.frames(4);
        for text in ["¿Qué quieres hacer?", "Escuchar el ejemplo", "Tararear una idea", "Remezclar una canción", "House", "Podcast", "1. Tu primer ritmo  ✓", "Mostrar al arrancar"] {
            assert!(s.has(text), "falta «{text}»");
        }
        let shot = s.shot("bienvenida");
        std::fs::copy(&shot, dir.join("bienvenida.png")).unwrap();
        s.size = egui::vec2(1000.0, 720.0);
        s.frames(4);
        let shot = s.shot("bienvenida-estrecha");
        std::fs::copy(&shot, dir.join("bienvenida-estrecha.png")).unwrap();
        s.size = egui::vec2(1440.0, 900.0);
        s.frames(4);
        s.click("Mostrar al arrancar");
        assert!(!s.app.settings.welcome_at_start);
        s.click("Escuchar el ejemplo");
        assert!(!s.app.welcome.open);

        let mut app = PulseApp::headless(Project::empty());
        app.settings.welcome_at_start = false;
        app.start_fresh();
        assert!(!app.welcome.open);
        assert_eq!(app.project.markers.len(), 7);
    }

    /// A lesson in its panel beside the song: a picture (`PULSE_SHOTS=dir`).
    #[test]
    fn a_lesson_shows_beside_the_song() {
        let dir = shots();
        let mut app = PulseApp::headless(Project::empty());
        app.start_lesson(0);
        app.add_track();
        let mut s = Studio::new(app, dir.clone());
        s.frames(4);
        for text in ["Tu primer ritmo", "Crea una pista MIDI", "Ponle una batería", "1 de 5", "Más en la guía: Generar ritmos"] {
            assert!(s.has(text), "falta «{text}»");
        }
        let shot = s.shot("leccion");
        std::fs::copy(&shot, dir.join("leccion.png")).unwrap();
        // A step ticked by hand, and the list.
        s.click("‹ Lecciones");
        assert!(s.has("Graba tu voz y afínala"));
        let shot = s.shot("lecciones");
        std::fs::copy(&shot, dir.join("lecciones.png")).unwrap();
        s.click("×");
        assert!(!s.app.lessons.open);
    }

    /// In English: the welcome's lessons and Pulse's templates, a lesson's
    /// steps and its progress; the user's templates keep their names.
    #[test]
    fn lessons_and_templates_show_in_english() {
        let dir = shots();
        crate::i18n::with(crate::i18n::Language::English, || {
            assert_eq!(crate::templates::shown("Grabar voz"), "Record Vocals");
            assert_eq!(crate::templates::shown(crate::templates::EMPTY), "Empty");
            let mine = crate::templates::Template { name: "Grabar voz".into(), file: Some(PathBuf::from("/x/Grabar voz/Grabar voz.pulse")) };
            assert_eq!(mine.shown(), "Grabar voz", "a template of the user's, as named");
            let mut app = PulseApp::headless(Project::empty());
            app.templates = crate::templates::all();
            app.start_fresh();
            let mut s = Studio::new(app, dir.clone());
            s.frames(4);
            for text in ["1. Your first beat", "3. Record your voice and tune it", "Record Vocals", "Empty", "House"] {
                assert!(s.has(text), "falta «{text}»");
            }
            assert!(!s.has("Grabar voz") && !s.has("1. Tu primer ritmo"));
            s.click("1. Your first beat");
            s.frames(4);
            for text in ["Your first beat", "Create a MIDI track", "Give it drums", "0 of 5", "More in the guide (in Spanish): Generar ritmos"] {
                assert!(s.has(text), "falta «{text}»");
            }
            let shot = s.shot("leccion-en");
            std::fs::copy(&shot, dir.join("leccion-en.png")).unwrap();
        });
    }
}
