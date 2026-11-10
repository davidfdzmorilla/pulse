//! Ayuda → Lecciones: short lessons, a few steps each, in a panel beside the
//! song. Each step says what to do and how; most mark themselves done when
//! what they ask for is there (a MIDI track more than when the lesson began, a
//! clip with chords, the assistant's report…), and any can be ticked by hand.
//! A lesson links to its topic of the guide, and the lessons finished are kept
//! in the settings.
//!
//! What a step checks compares the project with how it was when the lesson
//! began (see [`Start`]), so a song that already has drums doesn't tick «put
//! drums on a track» by itself.

use crate::app::PulseApp;
use crate::i18n::{fill, tr};
use crate::project::{Project, Track};
use crate::theme::{self, ACCENT, PLAY, TEXT, TEXT_DIM};
use eframe::egui;
use engine::model::Clip;

/// How things were when the lesson began.
#[derive(Clone)]
pub struct Start {
    pub project: Project,
    /// Exports finished by then (see `export::Dialog::exported`).
    pub exported: u32,
}

/// Whether a step is done: from the app now and how it was when the lesson began.
type Check = fn(&PulseApp, &Start) -> bool;

pub struct Step {
    /// What to do.
    pub text: &'static str,
    /// How (with keys as `{⌘T}`, written as this system writes them).
    pub how: &'static str,
    /// None: only ticked by hand.
    pub done: Option<Check>,
}

pub struct Lesson {
    /// Its name in the settings (never changes).
    pub id: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    /// The guide's topic about it.
    pub topic: &'static str,
    pub steps: &'static [Step],
}

// ----- What the steps look at -----------------------------------------------------

fn midi_clips(track: &Track) -> impl Iterator<Item = &engine::model::MidiClip> {
    track.clips.iter().flatten().chain(track.arrangement.iter().map(|c| &c.clip)).filter_map(|c| if let Clip::Midi(m) = c { Some(m) } else { None })
}

fn audio_clips(p: &Project) -> usize {
    p.tracks.iter().flat_map(|t| t.clips.iter().flatten().chain(t.arrangement.iter().map(|c| &c.clip))).filter(|c| matches!(c, Clip::Audio(_))).count()
}

fn midi_tracks(p: &Project) -> usize {
    p.tracks.iter().filter(|t| !t.is_audio() && !t.is_group()).count()
}

fn audio_tracks(p: &Project) -> usize {
    p.tracks.iter().filter(|t| t.is_audio()).count()
}

fn drum_tracks(p: &Project) -> usize {
    p.tracks.iter().filter(|t| t.plays_drums()).count()
}

fn drum_clips(p: &Project) -> usize {
    p.tracks.iter().filter(|t| t.plays_drums()).map(|t| midi_clips(t).count()).sum()
}

fn drum_notes(p: &Project) -> usize {
    p.tracks.iter().filter(|t| t.plays_drums()).flat_map(midi_clips).map(|m| m.notes.len()).sum()
}

/// MIDI clips with chords in them: at least two moments where three notes or
/// more start together.
fn chord_clips(p: &Project) -> usize {
    p.tracks
        .iter()
        .flat_map(midi_clips)
        .filter(|m| {
            let mut starts: Vec<i64> = m.notes.iter().map(|n| (n.start * 16.0).round() as i64).collect();
            starts.sort_unstable();
            let chords = starts.chunk_by(|a, b| a == b).filter(|same| same.len() >= 3).count();
            chords >= 2
        })
        .count()
}

/// Bass tracks with notes.
fn bass_lines(p: &Project) -> usize {
    p.tracks.iter().filter(|t| crate::arrange::role(t) == crate::arrange::Role::Bass && midi_clips(t).any(|m| !m.notes.is_empty())).count()
}

fn armed_audio(p: &Project) -> bool {
    p.tracks.iter().any(|t| t.is_audio() && t.armed)
}

/// The mix: each channel's fader, pan, sends and effects, and the master's.
fn mix(p: &Project) -> (Vec<(String, String)>, String) {
    let channel = |t: &Track| (format!("{:?}", t.mixer), format!("{:?}{:?}", t.sends, t.devices));
    (p.tracks.iter().map(channel).collect(), format!("{:?}{:?}", p.master.devices, p.returns.iter().map(|r| (&r.mixer, &r.devices)).collect::<Vec<_>>()))
}

fn arranged(p: &Project) -> bool {
    p.tracks.iter().any(|t| !t.arrangement.is_empty())
}

// ----- The lessons ------------------------------------------------------------------

pub const LESSONS: [Lesson; 6] = [
    Lesson {
        id: "ritmo",
        title: "Tu primer ritmo",
        about: "Una pista de batería, un clip y un ritmo que suena en bucle.",
        topic: "Generar ritmos",
        steps: &[
            Step { text: "Crea una pista MIDI", how: "Pulsa {⌘T} o elige Crear › Pista MIDI.", done: Some(|app, start| midi_tracks(&app.project) > midi_tracks(&start.project)) },
            Step { text: "Ponle una batería", how: "En el navegador, INSTRUMENTOS › Drum Machine, o arrastra un kit de Sonidos de Pulse › Kits a la pista.", done: Some(|app, start| drum_tracks(&app.project) > drum_tracks(&start.project)) },
            Step { text: "Crea un clip", how: "Haz doble clic en un slot vacío de la pista de batería: se abre en el editor.", done: Some(|app, start| drum_clips(&app.project) > drum_clips(&start.project)) },
            Step { text: "Escribe el ritmo", how: "Haz doble clic en el editor para poner bombos, cajas y charles, o clic derecho en el clip › Generar ritmo.", done: Some(|app, start| drum_notes(&app.project) >= drum_notes(&start.project) + 8) },
            Step { text: "Escúchalo", how: "Lanza el clip con su botón de reproducir o pulsa {Espacio}. {Espacio} otra vez para.", done: Some(|app, _| app.audio.is_playing()) },
        ],
    },
    Lesson {
        id: "acordes",
        title: "Acordes y bajo",
        about: "Elige una tonalidad, escribe acordes y deja que Pulse saque el bajo de ellos.",
        topic: "Acompañamiento",
        steps: &[
            Step { text: "Elige la tonalidad", how: "En el transporte, haz clic en la tonalidad y elige una, por ejemplo La menor.", done: Some(|app, start| app.project.key.is_some() && app.project.key != start.project.key) },
            Step { text: "Escribe unos acordes", how: "En un clip de una pista de teclas o pad, usa Acordes en el editor MIDI: uno por compás.", done: Some(|app, start| chord_clips(&app.project) > chord_clips(&start.project)) },
            Step { text: "Saca el bajo", how: "Clic derecho en el clip de acordes › Acompañar › un bajo. Va a una pista nueva, debajo.", done: Some(|app, start| bass_lines(&app.project) > bass_lines(&start.project)) },
            Step { text: "Prueba otra variación", how: "En el editor del bajo, Otra variación escribe otro con los mismos acordes. Márcalo cuando te guste uno.", done: None },
        ],
    },
    Lesson {
        id: "voz",
        title: "Graba tu voz y afínala",
        about: "Una toma de voz con el metrónomo, y sus notas llevadas a su sitio.",
        topic: "Grabar",
        steps: &[
            Step { text: "Crea una pista de audio", how: "Pulsa {⇧⌘T} o elige Crear › Pista de audio.", done: Some(|app, start| audio_tracks(&app.project) > audio_tracks(&start.project)) },
            Step { text: "Ármala", how: "Elige su entrada y pulsa su botón ●. Con auriculares, te oyes mientras cantas.", done: Some(|app, _| armed_audio(&app.project)) },
            Step { text: "Graba una toma", how: "Pulsa {F9} para grabar en el Arrangement y {Espacio} para parar.", done: Some(|app, start| audio_clips(&app.project) > audio_clips(&start.project)) },
            Step { text: "Abre sus notas", how: "Selecciona la toma y pulsa Afinación → Editar notas… en el editor de audio.", done: Some(|app, _| app.tuner.open.is_some()) },
            Step { text: "Afina una nota", how: "Haz doble clic en una nota para llevarla a la escala, o arrástrala.", done: Some(|app, start| app.project.tunings.len() > start.project.tunings.len()) },
        ],
    },
    Lesson {
        id: "tarareo",
        title: "De tarareo a canción",
        about: "Tararea una melodía y Pulse la convierte en melodía, acordes, bajo y batería.",
        topic: "De tarareo a canción",
        steps: &[
            Step { text: "Enciende el metrónomo", how: "El botón del metrónomo del transporte: tararea a su tempo.", done: Some(|app, _| app.project.metronome) },
            Step { text: "Arma una pista de audio", how: "Su botón ●, con la entrada del micrófono.", done: Some(|app, _| armed_audio(&app.project)) },
            Step { text: "Graba tu melodía", how: "Pulsa {F9}, tararea unos compases y {Espacio} para parar.", done: Some(|app, start| audio_clips(&app.project) > audio_clips(&start.project)) },
            Step { text: "Hazla canción", how: "Clic derecho en la toma › De tarareo a canción › un estilo. Salen cuatro pistas.", done: Some(|app, start| midi_tracks(&app.project) >= midi_tracks(&start.project) + 4) },
        ],
    },
    Lesson {
        id: "mezcla",
        title: "Mezcla con el asistente",
        about: "El asistente escucha la canción, dice qué arreglar y lo arregla contigo.",
        topic: "Asistente de mezcla y master",
        steps: &[
            Step { text: "Abre el asistente", how: "Archivo › Asistente de mezcla y master…", done: Some(|app, _| app.mix.open) },
            Step { text: "Analiza la mezcla", how: "Pulsa Analizar la mezcla: escucha la canción entera en unos segundos.", done: Some(|app, _| app.mix.report.is_some()) },
            Step { text: "Aplica una propuesta", how: "Pulsa Aplicar en una, o Aplicar todo. {⌘Z} la deshace.", done: Some(|app, start| mix(&app.project) != mix(&start.project)) },
            Step { text: "Compara antes y después", how: "Con A/B oyes la mezcla de antes de los cambios.", done: Some(|app, _| app.mix.listening.is_some()) },
        ],
    },
    Lesson {
        id: "exportar",
        title: "Exporta tu canción",
        about: "La canción en el Arrangement, y de ahí a un WAV o MP3 para compartirla.",
        topic: "Exportar audio",
        steps: &[
            Step { text: "Pon la canción en el Arrangement", how: "Crear › Arreglo automático… la monta con tus escenas, o arrastra clips a la línea de tiempo.", done: Some(|app, _| arranged(&app.project)) },
            Step { text: "Abre Exportar", how: "Pulsa {⇧⌘R} o Archivo › Exportar audio…", done: Some(|app, _| app.export.open) },
            Step { text: "Exporta", how: "Elige el formato y el destino (Streaming deja la sonoridad lista) y pulsa Exportar.", done: Some(|app, start| app.export.exported > start.exported) },
        ],
    },
];

/// The lesson called `id`.
pub fn find(id: &str) -> Option<usize> {
    LESSONS.iter().position(|l| l.id == id)
}

/// The panel's state.
#[derive(Default)]
pub struct Lessons {
    pub open: bool,
    /// The lesson being followed (else the list).
    pub current: Option<usize>,
    pub start: Option<Start>,
    /// Its steps done.
    pub done: Vec<bool>,
}

impl Lessons {
    /// Steps done, of how many.
    pub fn progress(&self) -> (usize, usize) {
        (self.done.iter().filter(|d| **d).count(), self.done.len())
    }
}

impl PulseApp {
    /// Ayuda › Lecciones: the panel, with the list (or the lesson being followed).
    pub fn toggle_lessons(&mut self) {
        self.lessons.open = !self.lessons.open;
    }

    /// Begins lesson `index` from how things are now.
    pub fn start_lesson(&mut self, index: usize) {
        let Some(lesson) = LESSONS.get(index) else { return };
        self.lessons = Lessons { open: true, current: Some(index), start: Some(Start { project: self.project.clone(), exported: self.export.exported }), done: vec![false; lesson.steps.len()] };
        self.check_lesson();
    }

    /// Ticks the steps that are done now (they stay ticked), and keeps the
    /// lesson as finished once all are.
    pub fn check_lesson(&mut self) {
        let (Some(index), Some(start)) = (self.lessons.current, self.lessons.start.as_ref()) else { return };
        let lesson = &LESSONS[index];
        let newly: Vec<usize> = lesson.steps.iter().enumerate().filter(|(i, step)| !self.lessons.done[*i] && step.done.is_some_and(|done| done(self, start))).map(|(i, _)| i).collect();
        for i in newly {
            self.lessons.done[i] = true;
        }
        self.finish_lesson_if_done();
    }

    /// A step ticked or unticked by hand.
    pub fn toggle_step(&mut self, step: usize) {
        if let Some(done) = self.lessons.done.get_mut(step) {
            *done = !*done;
        }
        self.finish_lesson_if_done();
    }

    fn finish_lesson_if_done(&mut self) {
        let Some(index) = self.lessons.current else { return };
        let (done, all) = self.lessons.progress();
        let id = LESSONS[index].id;
        if done == all && !self.settings.lessons_done.iter().any(|d| d == id) {
            let mut settings = self.settings.clone();
            settings.lessons_done.push(id.to_string());
            self.set_settings(settings);
            self.status = Some(fill(tr("Lección «{}» terminada"), &[&tr(LESSONS[index].title)]));
        }
    }
}

const WIDTH: f32 = 290.0;

/// The panel's content (in a side panel of the window, see `app`).
pub fn panel(app: &mut PulseApp, ui: &mut egui::Ui) {
    app.check_lesson();
    ui.set_width(WIDTH - 16.0);
    ui.horizontal(|ui| {
        if app.lessons.current.is_some() {
            if ui.button(tr("‹ Lecciones")).on_hover_text(tr("Todas las lecciones")).clicked() {
                app.lessons.current = None;
            }
        } else {
            ui.label(egui::RichText::new(tr("Lecciones")).font(theme::bold(15.0)).color(TEXT));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("×").on_hover_text(tr("Cerrar las lecciones (Ayuda › Lecciones las abre)")).clicked() {
                app.lessons.open = false;
            }
        });
    });
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match app.lessons.current {
        None => list(app, ui),
        Some(index) => lesson(app, ui, index),
    });
}

fn list(app: &mut PulseApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new(tr("Unos minutos cada una. Los pasos se marcan solos al hacerlos, sobre tu proyecto.")).color(TEXT_DIM));
    ui.add_space(6.0);
    for (i, lesson) in LESSONS.iter().enumerate() {
        let finished = app.settings.lessons_done.iter().any(|d| d == lesson.id);
        let mark = if finished { "✓" } else { "" };
        let response = crate::welcome::card(ui, &format!("{}. {}", i + 1, tr(lesson.title)), tr(lesson.about), mark, ui.available_width(), 0.0);
        if response.on_hover_text(tr("Empezar esta lección con tu proyecto tal como está")).clicked() {
            app.start_lesson(i);
        }
        ui.add_space(4.0);
    }
}

fn lesson(app: &mut PulseApp, ui: &mut egui::Ui, index: usize) {
    let lesson = &LESSONS[index];
    ui.label(egui::RichText::new(tr(lesson.title)).font(theme::bold(15.0)).color(TEXT));
    ui.label(egui::RichText::new(tr(lesson.about)).color(TEXT_DIM));
    let (done, all) = app.lessons.progress();
    ui.add_space(4.0);
    ui.add(egui::ProgressBar::new(done as f32 / all.max(1) as f32).text(fill(tr("{} de {}"), &[&done, &all])).desired_width(ui.available_width()));
    ui.add_space(6.0);
    let next = app.lessons.done.iter().position(|d| !d);
    let mut toggled = None;
    for (i, step) in lesson.steps.iter().enumerate() {
        let is_done = app.lessons.done.get(i).copied().unwrap_or(false);
        ui.horizontal_top(|ui| {
            let (mark, color) = if is_done { ("✓", PLAY) } else if Some(i) == next { ("●", ACCENT) } else { ("○", TEXT_DIM) };
            let tip = if is_done { tr("Hecho (clic para desmarcarlo)") } else if step.done.is_some() { tr("Se marca solo al hacerlo; también con un clic") } else { tr("Márcalo cuando lo hayas hecho") };
            let button = egui::Button::new(egui::RichText::new(mark).color(color).font(theme::bold(14.0))).frame(false).min_size(egui::vec2(20.0, 20.0));
            if ui.add(button).on_hover_text(tip).clicked() {
                toggled = Some(i);
            }
            ui.vertical(|ui| {
                let text = egui::RichText::new(tr(step.text)).color(if is_done { TEXT_DIM } else { TEXT });
                ui.label(if Some(i) == next { text.strong() } else { text });
                if !is_done {
                    ui.label(egui::RichText::new(theme::keys(&tr(step.how).replace(['{', '}'], ""))).color(TEXT_DIM).size(11.0));
                }
            });
        });
        ui.add_space(4.0);
    }
    if let Some(i) = toggled {
        app.toggle_step(i);
    }
    ui.add_space(6.0);
    if next.is_none() {
        ui.label(egui::RichText::new(tr("¡Lección terminada!")).color(PLAY).strong());
        if let Some(following) = LESSONS.get(index + 1) {
            if ui.button(fill(tr("Siguiente: {}"), &[&tr(following.title)])).clicked() {
                app.start_lesson(index + 1);
            }
        }
    }
    if ui.button(fill(tr("Más en la guía: {}"), &[&crate::guide::topic_title(lesson.topic)])).on_hover_text(tr("Abre el tema de la guía con todos los detalles")).clicked() {
        app.guide.open_at(lesson.topic);
    }
}

/// The panel's width, for the window's layout.
pub fn width() -> f32 {
    WIDTH
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use engine::model::{ArrangementClip, InstrumentKind, MidiClip, Note};

    /// Each lesson names a topic the guide has, and each step says how.
    #[test]
    fn every_lesson_links_to_the_guide() {
        let mut guide = crate::guide::Guide::default();
        for lesson in &LESSONS {
            assert!(guide.has_topic(lesson.topic), "«{}» → «{}»", lesson.title, lesson.topic);
            assert!(!lesson.steps.is_empty() && lesson.steps.iter().all(|s| !s.how.is_empty()));
            assert_eq!(find(lesson.id), LESSONS.iter().position(|l| l.id == lesson.id));
        }
    }

    /// «Tu primer ritmo» followed as a user would: each step ticks itself as
    /// it is done, not before, and the lesson is kept as finished.
    #[test]
    fn the_first_beat_lesson_ticks_its_steps_as_they_are_done() {
        let mut app = PulseApp::headless(Project::empty());
        app.start_lesson(find("ritmo").unwrap());
        assert_eq!(app.lessons.progress(), (0, 5), "nothing done yet, though the project has MIDI tracks");
        app.add_track();
        app.check_lesson();
        assert_eq!(app.lessons.done, [true, false, false, false, false]);
        let t = app.project.tracks.len() - 1;
        app.project.tracks[t].instrument = InstrumentKind::DrumMachine(Default::default());
        app.check_lesson();
        assert!(app.lessons.done[1]);
        app.project.tracks[t].clips[0] = Some(Clip::Midi(MidiClip::new("Ritmo", 4.0, [0, 0, 0])));
        app.check_lesson();
        assert!(app.lessons.done[2] && !app.lessons.done[3], "an empty clip is not a beat yet");
        if let Some(Clip::Midi(m)) = &mut app.project.tracks[t].clips[0] {
            m.notes = (0..8).map(|i| Note::new(i as f64 * 0.5, 0.25, if i % 2 == 0 { 36 } else { 42 }, 0.9)).collect();
        }
        app.check_lesson();
        assert_eq!(app.lessons.progress(), (4, 5));
        // Playing can't be seen without a sound card: ticked by hand.
        assert!(!app.settings.lessons_done.contains(&"ritmo".to_string()));
        app.toggle_step(4);
        assert_eq!(app.lessons.progress(), (5, 5));
        assert!(app.settings.lessons_done.contains(&"ritmo".to_string()));
        // Undone, a step stays ticked; ticking by hand can be undone.
        app.project.tracks.pop();
        app.check_lesson();
        assert_eq!(app.lessons.progress(), (5, 5));
        app.toggle_step(4);
        assert_eq!(app.lessons.progress(), (4, 5));
    }

    /// The other lessons' checks see what they ask for.
    #[test]
    fn the_steps_see_chords_a_bass_recordings_the_mix_and_exports() {
        let mut app = PulseApp::headless(Project::empty());
        app.start_lesson(find("acordes").unwrap());
        app.project.key = Some(stems::key::Key { tonic: 9, minor: true });
        let chords: Vec<Note> = (0..4).flat_map(|bar| [57u8, 60, 64].map(|p| Note::new(bar as f64 * 4.0, 3.5, p, 0.8))).collect();
        app.project.tracks[1].clips[0] = Some(Clip::Midi(MidiClip { notes: chords, ..MidiClip::new("Acordes", 16.0, [0, 0, 0]) }));
        app.check_lesson();
        assert_eq!(app.lessons.done, [true, true, false, false]);
        let mut bass = Track::new("Bajo", [0, 0, 0], InstrumentKind::Acid(Default::default()));
        bass.clips[0] = Some(Clip::Midi(MidiClip { notes: vec![Note::new(0.0, 1.0, 45, 1.0)], ..MidiClip::new("Bajo", 4.0, [0, 0, 0]) }));
        app.project.tracks.push(bass);
        app.check_lesson();
        assert_eq!(app.lessons.done, [true, true, true, false], "the last one is ticked by hand");

        app.start_lesson(find("voz").unwrap());
        app.add_audio_track();
        let t = app.project.tracks.len() - 1;
        app.project.tracks[t].armed = true;
        app.check_lesson();
        assert_eq!(app.lessons.done, [true, true, false, false, false]);

        app.start_lesson(find("mezcla").unwrap());
        app.mix.open = true;
        app.check_lesson();
        assert_eq!(app.lessons.done, [true, false, false, false]);
        app.project.tracks[0].mixer.gain *= 0.5;
        app.check_lesson();
        assert_eq!(app.lessons.done, [true, false, true, false]);

        app.start_lesson(find("exportar").unwrap());
        assert!(!app.lessons.done[0], "nothing on the Arrangement yet");
        let clip = app.project.tracks[1].clips[0].clone().unwrap();
        app.project.tracks[1].arrangement.push(ArrangementClip { start: 0.0, length: 16.0, clip });
        app.export.open = true;
        app.export.exported += 1;
        app.check_lesson();
        assert_eq!(app.lessons.progress(), (3, 3));
    }
}
