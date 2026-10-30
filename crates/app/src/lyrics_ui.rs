//! «Transcribir letra»: the words sung in an audio clip, read on this
//! computer by Whisper (see `stems::whisper`), kept in the project with their
//! times (see `lyrics`) and shown under the waveform in the clip view and
//! along the clip in the Arrangement.
//!
//! The model (Whisper small, 251 MB) is downloaded the first time, after
//! asking, from Hugging Face at a fixed revision, each file checked against
//! its SHA-256 and written whole (a `.part` renamed). A download cut short
//! goes on from there the next time.

use crate::app::{ClipRef, PulseApp};
use crate::lyrics::{ms, LyricLine, LyricWord};
use crate::theme::{self, TEXT_DIM};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense};
use engine::model::Clip;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Where the model comes from: onnx-community/whisper-small, at this revision.
const REPOSITORY: &str = "https://huggingface.co/onnx-community/whisper-small/resolve/36050c46d777d46dc4b5f43f6d90574fc38f8732";

/// A file of the model: where it is in the repository, its size and SHA-256.
struct ModelFile {
    path: &'static str,
    size: u64,
    sha256: &'static str,
}

/// In the order of `stems::whisper::FILES`.
const MODEL: [ModelFile; 3] = [
    ModelFile { path: "onnx/encoder_model_quantized.onnx", size: 92_326_160, sha256: "a43a83f3c5361cd591cfa7c36f14b43cf7cb22f47a415cc14a8d557be800fa92" },
    ModelFile { path: "onnx/decoder_model_merged_quantized.onnx", size: 156_750_845, sha256: "ec07c3cbb64172c39791e26ee870a65ac22b458c36722bfe2776b3dbf741e0c9" },
    ModelFile { path: "tokenizer.json", size: 2_480_466, sha256: "27fc476bfe7f17299480be2273fc0608e4d5a99aba2ab5dec5374b4482d1a566" },
];

/// The model's size, as the user is told it.
pub const MODEL_MB: u64 = (92_326_160 + 156_750_845 + 2_480_466) / 1_000_000;

/// Where the model is kept (PULSE_LYRICS_MODEL, a folder, overrides it).
pub fn model_dir() -> PathBuf {
    match std::env::var_os("PULSE_LYRICS_MODEL") {
        Some(path) => path.into(),
        None => paths::data_dir().join("Models").join("whisper-small"),
    }
}

/// Whether the model is all there (each file is only renamed into place once checked).
pub fn model_ready(dir: &Path) -> bool {
    stems::whisper::FILES.iter().zip(&MODEL).all(|(name, file)| std::fs::metadata(dir.join(name)).is_ok_and(|m| m.len() == file.size))
}

/// The language the words are read in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Language {
    #[default]
    Auto,
    Spanish,
    English,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::Auto, Language::Spanish, Language::English];

    pub fn label(self) -> &'static str {
        match self {
            Language::Auto => "Automático",
            Language::Spanish => "Español",
            Language::English => "Inglés",
        }
    }

    fn code(self) -> Option<&'static str> {
        match self {
            Language::Auto => None,
            Language::Spanish => Some("es"),
            Language::English => Some("en"),
        }
    }
}

/// A language's name, from its code.
pub fn language_name(code: &str) -> String {
    match code {
        "es" => "español".into(),
        "en" => "inglés".into(),
        "" => "idioma sin detectar".into(),
        other => format!("idioma «{other}»"),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    Download(f32),
    Isolate(f32),
    Listen(f32),
    /// The language read in and the lines, or what went wrong.
    Done(Result<(String, Vec<LyricLine>), String>),
}

/// A transcription under way.
pub struct LyricsJob {
    clip: ClipRef,
    sample: String,
    name: String,
    /// The part of the file read (seconds): its lines are replaced.
    region: (f64, f64),
    pub stage: Arc<Mutex<Stage>>,
    cancel: Arc<AtomicBool>,
}

/// What is being asked before transcribing.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub clip: ClipRef,
    pub language: Language,
    /// Separate the voice from the music first (with the stems model).
    pub isolate: bool,
}

/// A word being corrected in the clip view: its file, line, word and text.
#[derive(Clone, Debug, PartialEq)]
pub struct WordEdit {
    pub sample: String,
    pub line: usize,
    pub word: usize,
    pub text: String,
    pub at: egui::Pos2,
}

#[derive(Default)]
pub struct LyricsUi {
    pub ask: Option<Ask>,
    pub job: Option<LyricsJob>,
    /// The clip whose lyrics' window is open.
    pub editing: Option<ClipRef>,
    pub word: Option<WordEdit>,
    /// The language chosen last time.
    language: Language,
}

/// What the lyrics' menus do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Transcribe,
    Edit,
    Copy,
    ExportTxt,
    ExportLrc,
}

/// The «Letra» submenu of an audio clip; `has` says whether it has lyrics.
pub fn menu(ui: &mut egui::Ui, has: bool, busy: bool) -> Option<Command> {
    let mut chosen = None;
    crate::widgets::menu_button(ui, "Letra", |ui| {
        let transcribe = if has { "Transcribir otra vez…" } else { "Transcribir letra…" };
        if ui.add_enabled(!busy, egui::Button::new(transcribe)).on_hover_text("Saca el texto que se canta, con el tiempo de cada frase (en este equipo, sin internet)").clicked() {
            chosen = Some(Command::Transcribe);
        }
        let items = [
            (Command::Edit, "Editar letra…", "Corrige las palabras a mano"),
            (Command::Copy, "Copiar letra", "Toda la letra del clip al portapapeles, una frase por línea"),
            (Command::ExportTxt, "Exportar .txt…", "La letra en un archivo de texto"),
            (Command::ExportLrc, "Exportar .lrc…", "La letra con el tiempo de cada frase, para karaoke y reproductores"),
        ];
        for (command, label, hint) in items {
            if ui.add_enabled(has, egui::Button::new(label)).on_hover_text(hint).clicked() {
                chosen = Some(command);
            }
        }
    });
    if chosen.is_some() {
        ui.close();
    }
    chosen
}

fn set(stage: &Mutex<Stage>, value: Stage) {
    if let Ok(mut s) = stage.lock() {
        *s = value;
    }
}

const CANCELLED: &str = "Transcripción cancelada";

impl PulseApp {
    /// Does what a lyrics menu chose for `clip` (copying, with `ctx`).
    pub fn lyrics_command(&mut self, clip: ClipRef, command: Command, ctx: &egui::Context) {
        if command == Command::Copy {
            ctx.copy_text(self.clip_lyrics_text(clip));
        }
        self.lyrics_action(clip, command);
    }

    /// The same, once the text is copied.
    pub fn lyrics_action(&mut self, clip: ClipRef, command: Command) {
        match command {
            Command::Transcribe => self.ask_lyrics(clip),
            Command::Edit => self.lyrics_ui.editing = Some(clip),
            Command::Copy => self.status = Some("Letra copiada".into()),
            Command::ExportTxt => self.export_lyrics(clip, false),
            Command::ExportLrc => self.export_lyrics(clip, true),
        }
    }

    /// Asks how to transcribe the audio clip `clip` (and, the first time,
    /// whether to download the model).
    pub fn ask_lyrics(&mut self, clip: ClipRef) {
        if self.lyrics_ui.job.is_some() {
            self.notify("Transcribir letra", "Ya hay una transcripción en marcha: espera a que termine.");
            return;
        }
        if !matches!(self.clip(clip), Some(Clip::Audio(_))) {
            return;
        }
        let isolate = crate::stems_ui::model_path().exists();
        self.lyrics_ui.ask = Some(Ask { clip, language: self.lyrics_ui.language, isolate });
    }

    /// Starts reading the words of `clip`, on a thread.
    pub fn transcribe_lyrics(&mut self, clip: ClipRef, language: Language, isolate: bool) {
        let Some(Clip::Audio(audio)) = self.clip(clip).cloned() else { return };
        if self.lyrics_ui.job.is_some() {
            return;
        }
        self.lyrics_ui.language = language;
        let duration = self.sample(&audio.sample).duration();
        let region = crate::lyrics::region(&audio, self.project.tempo, duration);
        let stage = Arc::new(Mutex::new(Stage::Listen(0.0)));
        let cancel = Arc::new(AtomicBool::new(false));
        let (shared, stop, source) = (stage.clone(), cancel.clone(), PathBuf::from(&audio.sample));
        std::thread::spawn(move || {
            let result = transcribe(&source, region, language, isolate, &shared, &stop);
            set(&shared, Stage::Done(result));
        });
        self.lyrics_ui.job = Some(LyricsJob { clip, sample: audio.sample.clone(), name: audio.name.clone(), region, stage, cancel });
    }

    /// Every frame: the question, the progress, the result, and the lyrics' window.
    pub fn follow_lyrics(&mut self, ctx: &egui::Context) {
        self.ask_window(ctx);
        self.progress_window(ctx);
        edit_window(self, ctx);
        word_popup(self, ctx);
    }

    fn ask_window(&mut self, ctx: &egui::Context) {
        let Some(mut ask) = self.lyrics_ui.ask.clone() else { return };
        let Some(Clip::Audio(audio)) = self.clip(ask.clip).cloned() else {
            self.lyrics_ui.ask = None;
            return;
        };
        let ready = model_ready(&model_dir());
        let separable = crate::stems_ui::model_path().exists();
        let (mut go, mut cancel) = (false, false);
        egui::Window::new("Transcribir letra").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(400.0);
            ui.label(format!("Saca el texto que se canta en «{}», con el tiempo de cada frase.", audio.name));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Idioma");
                for language in Language::ALL {
                    ui.selectable_value(&mut ask.language, language, language.label()).on_hover_text(match language {
                        Language::Auto => "Lo reconoce al escuchar el principio",
                        _ => "Mejor si lo sabes: no se equivoca de idioma en frases cortas",
                    });
                }
            });
            if separable {
                ui.checkbox(&mut ask.isolate, "Aislar la voz antes").on_hover_text("Separa la voz de la música con el modelo de stems y lee solo la voz: más lento, pero mucho mejor si hay música de fondo");
            } else {
                ui.label(egui::RichText::new("Con música de fondo sale mejor sobre la voz sola: usa «Separar en stems» y transcribe la pista de voz.").color(TEXT_DIM).size(10.5));
            }
            if !ready {
                ui.add_space(4.0);
                ui.label(format!("La primera vez hay que descargar el modelo de voz (Whisper small, {MODEL_MB} MB) de Hugging Face. Se guarda en este equipo y después funciona sin internet."));
            }
            ui.label(egui::RichText::new("El audio no sale de este equipo.").color(TEXT_DIM).size(10.5));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let label = if ready { "Transcribir".to_string() } else { format!("Descargar ({MODEL_MB} MB) y transcribir") };
                go = ui.button(label).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter));
                cancel = ui.button("Cancelar").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        });
        if go {
            self.lyrics_ui.ask = None;
            self.transcribe_lyrics(ask.clip, ask.language, ask.isolate && separable);
        } else if cancel {
            self.lyrics_ui.ask = None;
        } else {
            self.lyrics_ui.ask = Some(ask);
        }
    }

    fn progress_window(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.lyrics_ui.job else { return };
        let stage = job.stage.lock().map(|s| s.clone()).unwrap_or(Stage::Listen(0.0));
        if let Stage::Done(result) = stage {
            let job = self.lyrics_ui.job.take().expect("checked above");
            match result {
                Ok((language, lines)) if lines.is_empty() => {
                    let _ = language;
                    self.notify("Transcribir letra", &format!("No se oye letra en «{}». Si hay música de fondo, prueba a aislar la voz antes.", job.name));
                }
                Ok((language, lines)) => {
                    let count = lines.len();
                    let (from, to) = job.region;
                    self.edit_lyrics(&job.sample, |l| {
                        l.language = language.clone();
                        l.replace(from, to, lines);
                    });
                    if matches!(self.clip(job.clip), Some(Clip::Audio(a)) if a.sample == job.sample) {
                        self.select_clip(job.clip);
                    }
                    self.status = Some(format!("Letra de «{}»: {count} frase(s) en {}. Haz clic en una palabra para corregirla.", job.name, language_name(&language)));
                }
                Err(e) if e == CANCELLED => self.status = Some(e),
                Err(e) => self.notify("No se pudo transcribir la letra", &e),
            }
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let mut cancel = false;
        egui::Window::new("Transcribir letra").id(egui::Id::new("lyrics_progress")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            let (text, fraction) = match stage {
                Stage::Download(f) => (format!("Descargando el modelo de voz ({MODEL_MB} MB, solo la primera vez)…"), f),
                Stage::Isolate(f) => (format!("Aislando la voz de «{}»…", job.name), f),
                Stage::Listen(f) => (format!("Escuchando la letra de «{}»…", job.name), f),
                Stage::Done(_) => (String::new(), 1.0),
            };
            ui.label(text);
            ui.add(egui::ProgressBar::new(fraction).show_percentage().desired_width(360.0));
            ui.label(egui::RichText::new("Se hace en este equipo, sin enviar el audio a internet. Puedes seguir trabajando.").color(TEXT_DIM).size(10.5));
            cancel = ui.button("Cancelar").clicked();
        });
        if cancel {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// The thread's work: the model, the clip's part of the file (its voice
/// alone, if asked), and its lines, in the file's seconds.
fn transcribe(source: &Path, region: (f64, f64), language: Language, isolate: bool, stage: &Mutex<Stage>, cancel: &AtomicBool) -> Result<(String, Vec<LyricLine>), String> {
    let dir = model_dir();
    if !model_ready(&dir) {
        download_model(&dir, stage, cancel)?;
    }
    let sample = engine::Sample::load(source)?;
    let rate = sample.sample_rate;
    let at = |seconds: f64| ((seconds * rate as f64).max(0.0) as usize).min(sample.left.len());
    let (a, b) = (at(region.0), at(region.1));
    let (mut left, mut right) = (sample.left[a..b].to_vec(), sample.right[a..b].to_vec());
    let mut rate = rate;
    if isolate {
        set(stage, Stage::Isolate(0.0));
        let mix = stems::Stereo { left: stems::resample(&left, rate, stems::SAMPLE_RATE as f32), right: stems::resample(&right, rate, stems::SAMPLE_RATE as f32) };
        let mut separator = stems::Separator::load(&crate::stems_ui::model_path())?;
        let [_, _, _, voice] = separator
            .separate(&mix, |f| {
                set(stage, Stage::Isolate(f));
                !cancel.load(Ordering::Relaxed)
            })
            .map_err(|e| if cancel.load(Ordering::Relaxed) { CANCELLED.to_string() } else { e })?;
        (left, right, rate) = (voice.left, voice.right, stems::SAMPLE_RATE as f32);
    }
    let mono: Vec<f32> = left.iter().zip(&right).map(|(l, r)| (l + r) * 0.5).collect();
    set(stage, Stage::Listen(0.0));
    let mut whisper = stems::whisper::Whisper::load(&dir)?;
    let transcript = whisper
        .transcribe(&mono, rate, language.code(), |f| {
            set(stage, Stage::Listen(f));
            !cancel.load(Ordering::Relaxed)
        })
        .map_err(|e| if cancel.load(Ordering::Relaxed) { CANCELLED.to_string() } else { e })?;
    let lines = transcript
        .segments
        .into_iter()
        .map(|s| LyricLine {
            start: ms(s.start + region.0),
            end: ms(s.end + region.0),
            words: s.words.into_iter().map(|w| LyricWord { start: ms(w.start + region.0), end: ms(w.end + region.0), text: w.text }).collect(),
        })
        .filter(|l| !l.words.is_empty())
        .collect();
    Ok((transcript.language, lines))
}

/// Downloads the model's files that are missing into `dir`, checking each.
fn download_model(dir: &Path, stage: &Mutex<Stage>, cancel: &AtomicBool) -> Result<(), String> {
    set(stage, Stage::Download(0.0));
    std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    let total: u64 = MODEL.iter().map(|f| f.size).sum();
    let mut done = 0u64;
    for (name, file) in stems::whisper::FILES.iter().zip(&MODEL) {
        let dest = dir.join(name);
        if std::fs::metadata(&dest).is_ok_and(|m| m.len() == file.size) {
            done += file.size;
            continue;
        }
        // What an earlier try left is kept: the download goes on from there.
        let partial = dir.join(format!("{name}.part"));
        if std::fs::metadata(&partial).map_or(0, |m| m.len()) < file.size {
            fetch(&format!("{REPOSITORY}/{}", file.path), &partial, cancel, |bytes| set(stage, Stage::Download((done + bytes) as f32 / total as f32)))?;
        }
        let size = std::fs::metadata(&partial).map_or(0, |m| m.len());
        if size != file.size || crate::stems_ui::sha256(&partial)? != file.sha256 {
            let _ = std::fs::remove_file(&partial);
            return Err("El modelo descargado no es el esperado (tamaño o huella distintos); vuelve a intentarlo".into());
        }
        std::fs::rename(&partial, &dest).map_err(|e| e.to_string())?;
        done += file.size;
    }
    Ok(())
}

/// Downloads `url` into `partial`, going on from what it has, telling
/// `progress` how much there is. No token: it isn't GitHub.
fn fetch(url: &str, partial: &Path, cancel: &AtomicBool, progress: impl Fn(u64)) -> Result<(), String> {
    let mut child = crate::updates::spawn_curl(url, None, "application/octet-stream", Some(partial), true)?;
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CANCELLED.into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        progress(std::fs::metadata(partial).map_or(0, |m| m.len()));
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    if status.success() {
        return Ok(());
    }
    let mut error = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        use std::io::Read;
        let _ = stderr.read_to_string(&mut error);
    }
    Err(format!("No se pudo descargar el modelo de voz (se seguirá desde aquí la próxima vez): {}", crate::updates::curl_error(&error)))
}

/// The lyrics' window: each line with its time, to correct by hand, and
/// copying and exporting.
pub fn edit_window(app: &mut PulseApp, ctx: &egui::Context) {
    let Some(clip) = app.lyrics_ui.editing else { return };
    let Some(Clip::Audio(audio)) = app.clip(clip).cloned() else {
        app.lyrics_ui.editing = None;
        return;
    };
    let Some(lyrics) = app.lyrics_of(&audio.sample).cloned() else {
        app.lyrics_ui.editing = None;
        return;
    };
    let mut open = true;
    let mut command = None;
    let mut edits = Vec::new();
    egui::Window::new(format!("Letra · {}", audio.name)).id(egui::Id::new("lyrics_window")).open(&mut open).default_size([420.0, 360.0]).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (c, label, hint) in [
                (Command::Copy, "Copiar", "Toda la letra del clip al portapapeles"),
                (Command::ExportTxt, ".txt…", "Exportar la letra como texto"),
                (Command::ExportLrc, ".lrc…", "Exportar la letra con los tiempos (LRC)"),
                (Command::Transcribe, "Transcribir otra vez…", "Vuelve a escuchar el clip; las frases de su trozo se sustituyen"),
            ] {
                if ui.button(label).on_hover_text(hint).clicked() {
                    command = Some(c);
                }
            }
        });
        ui.label(egui::RichText::new(format!("{} · los tiempos son del archivo · deja una frase vacía para quitarla", language_name(&lyrics.language))).color(TEXT_DIM).size(10.5));
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, line) in lyrics.lines.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(crate::lyrics::lrc_time(line.start)).monospace().color(TEXT_DIM).size(10.5));
                    let mut text = line.text();
                    let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(ui.available_width()).id(egui::Id::new(("lyric_line", i))));
                    if response.changed() {
                        edits.push((i, text));
                    }
                });
            }
        });
    });
    for (line, text) in edits {
        // Emptied while typing, the line stays until the field is left.
        let typing = ctx.memory(|m| m.has_focus(egui::Id::new(("lyric_line", line))));
        if text.trim().is_empty() && typing {
            continue;
        }
        app.edit_lyrics(&audio.sample, |l| l.set_line(line, &text));
    }
    if !open {
        app.lyrics_ui.editing = None;
    }
    if let Some(command) = command {
        app.lyrics_command(clip, command, ctx);
    }
}

/// Height of the lyrics' strip under the clip view's waveform.
pub const STRIP_H: f32 = 18.0;

/// The words in `strip` of the clip view, each where it is sung (`x_of`
/// gives the file's seconds' place); clicking one corrects it.
pub fn strip(app: &mut PulseApp, ui: &mut egui::Ui, strip: Rect, sample: &str, x_of: &dyn Fn(f64) -> f32) {
    let Some(lyrics) = app.lyrics_of(sample).cloned() else { return };
    let response = ui.interact(strip, ui.id().with("lyrics_strip"), Sense::click());
    let painter = ui.painter_at(strip);
    painter.rect_filled(strip, 0.0, theme::shade(theme::SURFACE, 0.8));
    let font = theme::small();
    let mut right = f32::MIN;
    let mut hit = None;
    for (l, line) in lyrics.lines.iter().enumerate() {
        let x0 = x_of(line.start);
        if x0 > strip.right() || x_of(line.end) < strip.left() {
            continue;
        }
        for (w, word) in line.words.iter().enumerate() {
            let x = x_of(word.start).max(right + 4.0);
            // A tick where each line starts (where its first word is drawn).
            if w == 0 {
                painter.line_segment([pos2(x, strip.top() + 2.0), pos2(x, strip.bottom() - 2.0)], egui::Stroke::new(1.0, theme::ACCENT));
            }
            let galley = painter.layout_no_wrap(word.text.clone(), font.clone(), theme::TEXT);
            let rect = Rect::from_min_size(pos2(x + 2.0, strip.center().y - galley.size().y / 2.0), galley.size());
            // Too crowded to read: the word is left out (zoom in to see it).
            if x > x_of(word.end).max(x_of(word.start) + 1.0) + 40.0 {
                continue;
            }
            right = rect.right();
            let hovered = response.hover_pos().is_some_and(|p| rect.expand(2.0).contains(p));
            if hovered {
                painter.rect_filled(rect.expand(1.0), 2.0, theme::CONTROL_HOVER);
            }
            painter.galley(rect.min, galley, theme::TEXT);
            if response.clicked() && response.interact_pointer_pos().is_some_and(|p| rect.expand(2.0).contains(p)) {
                hit = Some((l, w, word.text.clone(), rect.left_bottom()));
            }
        }
    }
    let response = response.on_hover_text("La letra, donde se canta: clic en una palabra para corregirla");
    let _ = response;
    if let Some((line, word, text, at)) = hit {
        app.lyrics_ui.word = Some(WordEdit { sample: sample.to_string(), line, word, text, at });
    }
}

/// The small field where a clicked word is corrected: ⏎ keeps it, Esc leaves it.
pub fn word_popup(app: &mut PulseApp, ctx: &egui::Context) {
    let Some(mut edit) = app.lyrics_ui.word.clone() else { return };
    let (mut done, mut cancel) = (false, false);
    let id = egui::Id::new("lyric_word");
    egui::Area::new(id.with("area")).order(egui::Order::Foreground).fixed_pos(edit.at + vec2(0.0, 2.0)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(egui::TextEdit::singleline(&mut edit.text).id(id).desired_width(140.0)).on_hover_text("La palabra bien escrita (vacía: se quita; con espacios: varias palabras)");
                if !response.has_focus() && !response.lost_focus() {
                    response.request_focus();
                }
                done = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                done |= ui.button("✓").on_hover_text("Corregir").clicked();
            });
        });
    });
    if done {
        app.lyrics_ui.word = None;
        let (line, word, text) = (edit.line, edit.word, edit.text.clone());
        app.edit_lyrics(&edit.sample, |l| l.set_word(line, word, &text));
    } else if cancel {
        app.lyrics_ui.word = None;
    } else {
        app.lyrics_ui.word = Some(edit);
    }
}

/// The lyrics along an audio clip in the Arrangement: each word where it is
/// sung, on a band at the bottom of the clip (only words that fit).
#[allow(clippy::too_many_arguments)]
pub fn paint_on_clip(painter: &egui::Painter, rect: Rect, body: Rect, audio: &engine::model::AudioClip, lyrics: &crate::lyrics::Lyrics, placed_length: f64, ppb: f32, tempo: f64) {
    if body.height() < 22.0 || audio.length <= 0.0 {
        return;
    }
    let font = egui::FontId::proportional(9.5);
    let band = Rect::from_min_max(pos2(rect.left(), body.bottom() - 12.0), pos2(rect.right(), body.bottom()));
    let mut any = false;
    let mut right = f32::MIN;
    let laps = (placed_length / audio.length).ceil() as usize;
    for lap in 0..laps {
        for line in &lyrics.lines {
            for word in &line.words {
                let local = audio.beats_until(word.start, tempo);
                let at = lap as f64 * audio.length + local;
                if local < 0.0 || local >= audio.length || at >= placed_length {
                    continue;
                }
                let x = rect.left() + at as f32 * ppb;
                if x < right + 3.0 || x > painter.clip_rect().right() {
                    continue;
                }
                if !any {
                    painter.rect_filled(band, 0.0, Color32::from_black_alpha(120));
                    any = true;
                }
                let galley = painter.layout_no_wrap(word.text.clone(), font.clone(), theme::TEXT);
                right = x + galley.size().x;
                painter.galley(pos2(x, band.center().y - galley.size().y / 2.0), galley, theme::TEXT);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use eframe::egui::{Event, Modifiers, PointerButton};
    use engine::model::{ArrangementClip, AudioClip, WarpMode};

    fn voice(path: &str) -> AudioClip {
        AudioClip { hard_start: false, fades: Default::default(), name: "Voz".into(), color: [0xd6, 0x5f, 0xb8], sample: path.into(), length: 8.0, offset: 0.0, warp: WarpMode::Off, original_bpm: 120.0, warp_markers: Vec::new(), takes: Vec::new(), key: None, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), muted: false }
    }

    /// Four seconds of a tone in a WAV file of its own (a folder to remove after).
    fn voice_file(name: &str) -> (PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("pulse-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("voz.wav");
        let tone: Vec<f32> = (0..44_100 * 4).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        crate::capture::write_wav(&wav, &tone, &tone, 44_100.0).unwrap();
        (dir, wav.to_string_lossy().into_owned())
    }

    fn lines() -> Vec<LyricLine> {
        let line = |start: f64, end: f64, text: &str| LyricLine { start, end, words: crate::lyrics::words_of(text, start, end) };
        vec![line(0.2, 1.8, "Bailando bajo la luna"), line(2.0, 3.6, "mi corazon late contigo")]
    }

    #[test]
    fn the_model_is_ready_only_when_every_file_is_whole() {
        let dir = std::env::temp_dir().join(format!("pulse-whisper-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!model_ready(&dir));
        std::fs::write(dir.join(stems::whisper::FILES[2]), b"{}").unwrap();
        assert!(!model_ready(&dir), "a file of another size is not it");
        assert_eq!(MODEL_MB, 251);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn asking_then_cancelling_starts_nothing() {
        let mut app = PulseApp::headless(Project::demo());
        let t = app.add_audio_track().unwrap();
        app.set_clip(t, 0, Some(Clip::Audio(voice("/x/voz.wav"))));
        app.ask_lyrics(ClipRef::Session { track: t, slot: 0 });
        assert!(app.lyrics_ui.ask.is_some());
        let ctx = egui::Context::default();
        let input = egui::RawInput { events: vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], ..Default::default() };
        let _ = ctx.run_ui(input, |ui| app.follow_lyrics(ui.ctx()));
        assert!(app.lyrics_ui.ask.is_none() && app.lyrics_ui.job.is_none());
        // A MIDI clip has nothing to read.
        app.ask_lyrics(ClipRef::Session { track: 0, slot: 0 });
        assert!(app.lyrics_ui.ask.is_none());
    }

    /// A finished transcription lands in the project, on the clip's file, and
    /// undoes in one step.
    #[test]
    fn a_result_lands_on_the_clips_file() {
        let mut app = PulseApp::headless(Project::demo());
        let t = app.add_audio_track().unwrap();
        app.set_clip(t, 0, Some(Clip::Audio(voice("/x/voz.wav"))));
        let clip = ClipRef::Session { track: t, slot: 0 };
        let stage = Arc::new(Mutex::new(Stage::Done(Ok(("es".into(), lines())))));
        app.lyrics_ui.job = Some(LyricsJob { clip, sample: "/x/voz.wav".into(), name: "Voz".into(), region: (0.0, 4.0), stage, cancel: Default::default() });
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| app.follow_lyrics(ui.ctx()));
        assert!(app.lyrics_ui.job.is_none());
        let lyrics = app.clip_lyrics(clip).expect("the clip has lyrics");
        assert_eq!((lyrics.language.as_str(), lyrics.lines.len()), ("es", 2));
        assert!(app.status.as_deref().unwrap().contains("2 frase(s) en español"), "{:?}", app.status);
        // Nothing heard: it says so.
        let stage = Arc::new(Mutex::new(Stage::Done(Ok(("es".into(), Vec::new())))));
        app.lyrics_ui.job = Some(LyricsJob { clip, sample: "/x/voz.wav".into(), name: "Voz".into(), region: (0.0, 4.0), stage, cancel: Default::default() });
        let _ = ctx.run_ui(Default::default(), |ui| app.follow_lyrics(ui.ctx()));
        assert!(app.notice.as_ref().is_some_and(|(_, t)| t.contains("No se oye letra")));
        assert_eq!(app.project.lyrics[0].lines.len(), 2, "kept as they were");
    }

    /// The clip view with lyrics: clic on a word, type, ⏎: it is corrected.
    #[test]
    fn a_word_is_corrected_by_clicking_it_in_the_clip_view() {
        let (dir, path) = voice_file("lyrics-word");
        let mut app = PulseApp::headless(Project::demo());
        app.project.tempo = 120.0;
        let t = app.add_audio_track().unwrap();
        app.set_clip(t, 0, Some(Clip::Audio(voice(&path))));
        let clip = ClipRef::Session { track: t, slot: 0 };
        app.edit_lyrics(&path, |l| l.lines = lines());
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut frame = |app: &mut PulseApp, events: Vec<Event>| {
            time += 0.05;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 300.0))), time: Some(time), events, ..Default::default() };
            let output = ctx.run_ui(input, |ui| {
                crate::sample_editor::show(app, ui, clip, None, None);
                app.follow_lyrics(ui.ctx());
            });
            let mut texts = Vec::new();
            fn collect(shape: &egui::Shape, texts: &mut Vec<(String, Rect)>) {
                match shape {
                    egui::Shape::Text(t) => texts.push((t.galley.text().to_string(), Rect::from_min_size(t.pos, t.galley.size()))),
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
                    _ => {}
                }
            }
            output.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
            texts
        };
        let texts = frame(&mut app, vec![]);
        let (_, rect) = texts.iter().find(|(t, _)| t == "corazon").cloned().unwrap_or_else(|| panic!("the word is shown: {:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>()));
        let at = rect.center();
        let press = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(&mut app, vec![Event::PointerMoved(at)]);
        frame(&mut app, vec![press(true)]);
        frame(&mut app, vec![press(false)]);
        assert_eq!(app.lyrics_ui.word.as_ref().map(|w| (w.line, w.word, w.text.as_str())), Some((1, 1, "corazon")));
        frame(&mut app, vec![]);
        // Its text, all of it chosen and typed over.
        frame(&mut app, vec![Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
        frame(&mut app, vec![Event::Text("corazón".into())]);
        frame(&mut app, vec![Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        frame(&mut app, vec![]);
        assert!(app.lyrics_ui.word.is_none());
        assert_eq!(app.clip_lyrics(clip).unwrap().lines[1].text(), "mi corazón late contigo");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// From the Arrangement: right-click → Letra ▸ Copiar letra puts it on the clipboard.
    #[test]
    fn the_arrangement_menu_copies_the_lyrics() {
        let mut app = PulseApp::headless(Project::demo());
        app.view = crate::app::View::Arrangement;
        for track in &mut app.project.tracks {
            track.arrangement.clear();
            track.automation.clear();
        }
        app.project.tempo = 120.0;
        let (dir, path) = voice_file("lyrics-menu");
        let t = app.add_audio_track().unwrap();
        app.project.tracks[t].arrangement.push(ArrangementClip { start: 4.0, length: 8.0, clip: Clip::Audio(voice(&path)) });
        app.sync_all();
        let _ = app.sample(&path);
        app.edit_lyrics(&path, |l| l.lines = lines());
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut frame = |app: &mut PulseApp, events: Vec<Event>| {
            time += 0.05;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0))), time: Some(time), events, ..Default::default() };
            ctx.run_ui(input, |ui| crate::arrangement::show(app, ui))
        };
        let texts = |output: &egui::FullOutput| {
            let mut texts = Vec::new();
            fn collect(shape: &egui::Shape, texts: &mut Vec<(String, Rect)>) {
                match shape {
                    egui::Shape::Text(t) => texts.push((t.galley.text().to_string(), Rect::from_min_size(t.pos, t.galley.size()))),
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
                    _ => {}
                }
            }
            output.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
            texts
        };
        let shown = texts(&frame(&mut app, vec![]));
        assert!(shown.iter().any(|(t, _)| t == "Bailando"), "the words are on the clip");
        let click = |app: &mut PulseApp, frame: &mut dyn FnMut(&mut PulseApp, Vec<Event>) -> egui::FullOutput, at: egui::Pos2, button| {
            let press = |pressed| Event::PointerButton { pos: at, button, pressed, modifiers: Modifiers::NONE };
            frame(app, vec![Event::PointerMoved(at)]);
            frame(app, vec![press(true)]);
            frame(app, vec![press(false)]);
            frame(app, vec![])
        };
        let at = pos2(150.0 + 6.0 * app.arrangement.pixels_per_beat, 96.0 + t as f32 * 53.0);
        let menu = texts(&click(&mut app, &mut frame, at, PointerButton::Secondary));
        let (_, letra) = menu.iter().find(|(t, _)| t == "Letra").cloned().expect("a Letra submenu");
        let sub = texts(&click(&mut app, &mut frame, letra.center(), PointerButton::Primary));
        let (_, copy) = sub.iter().find(|(t, _)| t == "Copiar letra").cloned().unwrap_or_else(|| panic!("{:?}", sub.iter().map(|t| &t.0).collect::<Vec<_>>()));
        let output = click(&mut app, &mut frame, copy.center(), PointerButton::Primary);
        let _ = output;
        assert_eq!(app.status.as_deref(), Some("Letra copiada"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole thing, as the user does it: a Spanish voice made by macOS's
    /// `say` (40 s, so it takes two windows of the model), on the Arrangement,
    /// right-click → Letra ▸ Transcribir letra… → Transcribir. It needs the
    /// model downloaded (Pulse's folder, or PULSE_LYRICS_MODEL); with
    /// `PULSE_SHOTS=dir` it leaves PNGs of the result there.
    #[test]
    #[ignore = "needs the lyrics model and macOS's say"]
    fn a_sung_voice_is_transcribed_from_the_menu() {
        use crate::produccion::Studio;
        assert!(model_ready(&model_dir()), "download the model first");
        let shots = std::env::var_os("PULSE_SHOTS").map(PathBuf::from);
        let out = shots.clone().unwrap_or_else(std::env::temp_dir);
        std::fs::create_dir_all(&out).unwrap();
        let dir = std::env::temp_dir().join(format!("pulse-lyrics-say-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("Voz.wav");
        let text = "Bailando bajo la luna, mi corazón late contigo. [[slnc 1200]] \
            Las luces de la ciudad se apagan, pero tú sigues brillando. [[slnc 1200]] \
            No quiero que esta noche termine nunca. [[slnc 1200]] \
            Dame tu mano y vamos a volar sobre el mar. [[slnc 1200]] \
            Cuando suena la música, todo lo demás desaparece. [[slnc 1200]] \
            Bailando bajo la luna, mi corazón late contigo. [[slnc 1200]] \
            El verano se acaba y las olas guardan nuestro secreto. [[slnc 1200]] \
            Canta conmigo hasta que salga el sol.";
        let status = std::process::Command::new("say").args(["-v", "Monica", "-r", "130", "-o"]).arg(&wav).args(["--data-format=LEI16@44100", text]).status().unwrap();
        assert!(status.success());
        let path = wav.to_string_lossy().into_owned();

        let mut app = PulseApp::headless(Project::demo());
        app.view = crate::app::View::Arrangement;
        app.project.tempo = 120.0;
        // The voice under the drums, in view.
        app.project.tracks.truncate(1);
        let duration = app.sample(&path).duration();
        let t = app.add_audio_track().unwrap();
        app.project.tracks[t].name = "Voz".into();
        let mut audio = voice(&path);
        audio.name = "Voz cantada".into();
        audio.length = (duration * 2.0).ceil();
        app.project.tracks[t].arrangement.push(ArrangementClip { start: 0.0, length: audio.length, clip: Clip::Audio(audio) });
        app.sync_all();
        let mut s = Studio::new(app, out);
        s.app.arrangement.pixels_per_beat = 14.0;
        s.frames(3);
        let clip = ClipRef::Arrangement { track: t, index: 0 };
        let title = s.find("Voz cantada").expect("the clip on screen");
        s.rclick_at(title.center() + vec2(40.0, 20.0));
        s.click("Letra ⏵");
        if shots.is_some() {
            s.shot("letra-menu");
        }
        s.submenu("Transcribir letra…");
        assert!(s.app.lyrics_ui.ask.is_some(), "it asks first");
        if shots.is_some() {
            s.shot("letra-pregunta");
        }
        s.click("Transcribir");
        assert!(s.app.lyrics_ui.job.is_some(), "it started: {:?}", s.app.notice);
        let started = std::time::Instant::now();
        assert!(s.wait_for("transcripción", 300.0, |app| app.lyrics_ui.job.is_none()), "{:?}", s.app.status);
        let seconds = started.elapsed().as_secs_f64();
        let lyrics = s.app.clip_lyrics(clip).cloned().unwrap_or_else(|| panic!("{:?} {:?}", s.app.status, s.app.notice));
        println!("{duration:.1} s de voz leídos en {seconds:.1} s, idioma {}:", lyrics.language);
        for line in &lyrics.lines {
            println!("  [{}–{}] {}", crate::lyrics::lrc_time(line.start), crate::lyrics::lrc_time(line.end), line.text());
        }
        assert_eq!(lyrics.language, "es");
        let heard = s.app.clip_lyrics_text(clip).to_lowercase();
        for word in ["luna", "corazón", "ciudad", "noche", "mano", "música"] {
            assert!(heard.contains(word), "«{word}» in «{heard}»");
        }
        assert!(lyrics.lines.last().unwrap().start > 30.0, "past the first window: {:?}", lyrics.lines.last());
        if shots.is_some() {
            s.frames(3);
            s.shot("letra-arrangement");
            // The clip view: the words under the waveform.
            s.app.select_clip(clip);
            s.frames(3);
            s.shot("letra-editor");
            let word = s.app.clip_lyrics(clip).unwrap().lines[0].words[1].text.clone();
            if let Some(rect) = s.find(&word) {
                s.click_at(rect.center());
                s.shot("letra-corregir-palabra");
                s.key(egui::Key::Escape, Modifiers::NONE);
            }
            s.app.lyrics_ui.editing = Some(clip);
            s.frames(3);
            s.shot("letra-ventana");
            s.size = vec2(1000.0, 720.0);
            s.frames(5);
            s.shot("letra-estrecha");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
