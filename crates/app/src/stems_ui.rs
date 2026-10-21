//! "Separar en stems": an audio clip's file split into drums, bass, other and
//! vocals (see the `stems` crate), each on a new audio track right under the
//! clip's, with a clip in the same place and with the same settings. The
//! original track is switched off, so the song sounds as before.
//!
//! The work runs on a thread: the model is downloaded once (from a release of
//! the project's GitHub repository, checked against its SHA-256), the file is
//! decoded and brought to 44.1 kHz, separated, and the stems are written as WAV
//! files in the project's Recordings/Stems folder.

use crate::app::{ClipRef, PulseApp};
use crate::theme::TEXT_DIM;
use eframe::egui;
use engine::model::{ArrangementClip, AudioClip, Clip};
use engine::MAX_TRACKS;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The release holding the model (a pre-release, so updates never take it for a version).
pub const MODEL_TAG: &str = "modelo-htdemucs-1";
const MODEL_FILE: &str = "htdemucs.onnx";
const MODEL_SIZE: u64 = 174_264_717;
const MODEL_SHA256: &str = "04c2b1a149788d048f710a6140446a72f7b4bab3171bc42189176a64ed609c78";
/// The stems' names, in the model's order (drums, bass, other, vocals).
pub const NAMES: [&str; 4] = ["Batería", "Bajo", "Resto", "Voz"];
const COLORS: [[u8; 3]; 4] = [[0xe8, 0x8a, 0x3c], [0x5a, 0x8a, 0xe6], [0x9a, 0xc4, 0x5a], [0xd6, 0x5f, 0xb8]];

#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    Download(f32),
    Separate(f32),
    Done(Result<[PathBuf; 4], String>),
}

/// A separation under way.
pub struct StemJob {
    pub clip: ClipRef,
    /// The clip as it was when the separation started.
    pub audio: AudioClip,
    pub stage: Arc<Mutex<Stage>>,
    cancel: Arc<AtomicBool>,
}

/// Where the model is kept (PULSE_STEMS_MODEL overrides it).
pub fn model_path() -> PathBuf {
    match std::env::var_os("PULSE_STEMS_MODEL") {
        Some(path) => path.into(),
        None => paths::data_dir().join("Models").join(MODEL_FILE),
    }
}

fn set(stage: &Mutex<Stage>, value: Stage) {
    if let Ok(mut s) = stage.lock() {
        *s = value;
    }
}

impl PulseApp {
    /// Starts separating the audio clip `clip`.
    pub fn separate_stems(&mut self, clip: ClipRef) {
        if self.stems.is_some() {
            self.notify("Separar en stems", "Ya hay una separación en marcha: espera a que termine.");
            return;
        }
        let Some(Clip::Audio(audio)) = self.clip(clip).cloned() else { return };
        if self.project.tracks.len() + NAMES.len() > MAX_TRACKS {
            self.notify("Separar en stems", &format!("Hacen falta {} pistas libres para los stems y el máximo es {MAX_TRACKS}. Elimina alguna pista y vuelve a intentarlo.", NAMES.len()));
            return;
        }
        let stage = Arc::new(Mutex::new(Stage::Separate(0.0)));
        let cancel = Arc::new(AtomicBool::new(false));
        let (source, out_dir) = (PathBuf::from(&audio.sample), self.media_dir(crate::library::STEMS));
        let base = source.file_stem().map_or_else(|| audio.name.clone(), |s| s.to_string_lossy().into_owned());
        let (shared, stop) = (stage.clone(), cancel.clone());
        std::thread::spawn(move || {
            let result = separate(&source, &out_dir, &base, &shared, &stop);
            set(&shared, Stage::Done(result));
        });
        self.stems = Some(StemJob { clip, audio, stage, cancel });
    }

    /// Every frame: the progress window, and the tracks once the stems are ready.
    pub fn follow_stems(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.stems else { return };
        let stage = job.stage.lock().map(|s| s.clone()).unwrap_or(Stage::Separate(0.0));
        if let Stage::Done(result) = stage {
            let job = self.stems.take().expect("checked above");
            match result {
                Ok(paths) => self.place_stems(job.clip, &job.audio, &paths),
                Err(e) if e == CANCELLED => self.status = Some(e),
                Err(e) => self.notify("No se pudo separar en stems", &e),
            }
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let mut cancel = false;
        egui::Window::new("Separar en stems").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            let (text, fraction) = match stage {
                Stage::Download(f) => ("Descargando el modelo de separación (174 MB, solo la primera vez)…".to_string(), f),
                Stage::Separate(f) => (format!("Separando «{}» en batería, bajo, resto y voz…", job.audio.name), f),
                Stage::Done(_) => (String::new(), 1.0),
            };
            ui.label(text);
            ui.add(egui::ProgressBar::new(fraction).show_percentage().desired_width(360.0));
            ui.label(egui::RichText::new("Se hace en este Mac, sin enviar nada a internet. Puedes seguir trabajando.").color(TEXT_DIM).size(10.5));
            cancel = ui.button("Cancelar").clicked();
        });
        if cancel {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Four audio tracks under the clip's, each with the clip again on a stem;
    /// the original track goes silent. Each is a copy of the original (its
    /// effects, with plugins of their own, mixer, sends, automation and group)
    /// and the clip keeps its envelopes, so together they sound like it.
    pub fn place_stems(&mut self, clip: ClipRef, audio: &AudioClip, paths: &[PathBuf; 4]) {
        // The clip may have moved meanwhile: then the stems go at the end.
        let still_there = matches!(self.clip(clip), Some(Clip::Audio(a)) if a.sample == audio.sample);
        if self.project.tracks.len() + NAMES.len() > MAX_TRACKS {
            let folder = paths[0].parent().map_or(String::new(), |p| p.display().to_string());
            self.notify("Separar en stems", &format!("Los stems están en {folder}, pero no caben más pistas (el máximo es {MAX_TRACKS})."));
            return;
        }
        let first = self.project.tracks.len();
        let source = clip.track();
        let mut tracks = Vec::new();
        if still_there {
            self.capture_plugins();
        }
        for (i, path) in paths.iter().enumerate() {
            let t = if still_there {
                let mut copy = self.copy_track(source);
                copy.clips = Default::default();
                copy.arrangement.clear();
                copy.frozen = None;
                copy.mixer.active = true;
                (copy.armed, copy.solo, copy.id) = (false, false, 0);
                self.project.tracks.push(copy);
                self.project.tracks.len() - 1
            } else {
                let Some(t) = self.add_audio_track() else { break };
                t
            };
            let track = &mut self.project.tracks[t];
            track.name = format!("{} · {}", audio.name, NAMES[i]);
            track.color = COLORS[i];
            let stem = AudioClip { name: NAMES[i].to_string(), color: COLORS[i], sample: path.to_string_lossy().into_owned(), ..audio.clone() };
            match clip {
                ClipRef::Session { slot, .. } => self.set_clip(t, slot, Some(Clip::Audio(stem))),
                ClipRef::Arrangement { track: source, index } => {
                    let placed = self.project.tracks.get(source).and_then(|tr| tr.arrangement.get(index)).filter(|_| still_there);
                    let (start, length) = placed.map_or((0.0, audio.length), |p| (p.start, p.length));
                    self.project.tracks[t].arrangement.push(ArrangementClip { start, length, clip: Clip::Audio(stem) });
                    self.sync_arrangement(t);
                }
            }
            tracks.push(t);
        }
        if still_there {
            self.move_tracks(&tracks, source + 1);
            self.project.tracks[source].mixer.active = false;
            self.project.assign_ids();
            // The copies' effects and plugins, and where they now are.
            self.sync_all();
        }
        let placed_at = if still_there { source + 1 } else { first };
        let first_clip = match clip {
            ClipRef::Session { slot, .. } => ClipRef::Session { track: placed_at, slot },
            ClipRef::Arrangement { .. } => ClipRef::Arrangement { track: placed_at, index: self.project.tracks[placed_at].arrangement.len().saturating_sub(1) },
        };
        self.select_clip(first_clip);
        self.status = Some(if still_there {
            format!("Stems de «{}» en 4 pistas nuevas; la pista original queda desactivada", audio.name)
        } else {
            format!("Stems de «{}» en 4 pistas nuevas al final", audio.name)
        });
    }
}

const CANCELLED: &str = "Separación cancelada";

/// The thread's work: model, decoding, separation, files.
fn separate(source: &Path, out_dir: &Path, base: &str, stage: &Mutex<Stage>, cancel: &AtomicBool) -> Result<[PathBuf; 4], String> {
    let model = model_path();
    if !model.exists() {
        download_model(&model, stage, cancel)?;
    }
    set(stage, Stage::Separate(0.0));
    let sample = engine::Sample::load(source)?;
    let mix = stems::Stereo {
        left: stems::resample(&sample.left, sample.sample_rate, stems::SAMPLE_RATE as f32),
        right: stems::resample(&sample.right, sample.sample_rate, stems::SAMPLE_RATE as f32),
    };
    let mut separator = stems::Separator::load(&model)?;
    let separated = separator
        .separate(&mix, |fraction| {
            set(stage, Stage::Separate(fraction * 0.97));
            !cancel.load(Ordering::Relaxed)
        })
        .map_err(|e| if cancel.load(Ordering::Relaxed) { CANCELLED.to_string() } else { e })?;
    std::fs::create_dir_all(out_dir).map_err(|e| format!("No se pudo crear {}: {e}", out_dir.display()))?;
    let mut paths: [PathBuf; 4] = Default::default();
    for (i, stem) in separated.iter().enumerate() {
        let path = unique(&out_dir.join(format!("{base} - {}.wav", NAMES[i])));
        crate::capture::write_wav(&path, &stem.left, &stem.right, stems::SAMPLE_RATE as f32)?;
        paths[i] = path;
    }
    Ok(paths)
}

/// `path`, or "name 2.wav", "name 3.wav"… if it is taken.
fn unique(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    (2..).map(|n| path.with_file_name(format!("{stem} {n}.wav"))).find(|p| !p.exists()).expect("a free name")
}

/// Downloads the model from the project's GitHub release, checking its size and SHA-256.
fn download_model(dest: &Path, stage: &Mutex<Stage>, cancel: &AtomicBool) -> Result<(), String> {
    set(stage, Stage::Download(0.0));
    let token = crate::updates::token();
    let release = crate::updates::release_at(token.as_deref(), &format!("tags/{MODEL_TAG}")).map_err(|e| format!("No se encontró el modelo de separación: {e}"))?;
    let asset = release.assets.iter().find(|a| a.name == MODEL_FILE).ok_or("La publicación del modelo no tiene el archivo")?;
    let dir = dest.parent().ok_or("Carpeta del modelo no válida")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    // What an earlier try left is kept: the download goes on from there.
    let partial = dest.with_extension("part");
    // Unless it is all there already (or more: something else, found below).
    if std::fs::metadata(&partial).map_or(0, |m| m.len()) < MODEL_SIZE {
        fetch(&asset.url, token.as_deref(), &partial, stage, cancel)?;
    }
    let size = std::fs::metadata(&partial).map_or(0, |m| m.len());
    let hash = sha256(&partial)?;
    if size != MODEL_SIZE || hash != MODEL_SHA256 {
        let _ = std::fs::remove_file(&partial);
        return Err("El modelo descargado no es el esperado (tamaño o huella distintos); vuelve a intentarlo".into());
    }
    std::fs::rename(&partial, dest).map_err(|e| e.to_string())
}

/// Downloads `url` into `partial`, going on from what it has, with the
/// progress in `stage`. A token GitHub turns down is left out (the repository
/// is public). Cancelled, what came stays for next time.
fn fetch(url: &str, token: Option<&str>, partial: &Path, stage: &Mutex<Stage>, cancel: &AtomicBool) -> Result<(), String> {
    let attempt = |token: Option<&str>| -> Result<Result<(), String>, String> {
        let mut child = crate::updates::spawn_curl(url, token, "application/octet-stream", Some(partial), true)?;
        let status = loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CANCELLED.into());
            }
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break status;
            }
            let size = std::fs::metadata(partial).map_or(0, |m| m.len());
            set(stage, Stage::Download(size as f32 / MODEL_SIZE as f32));
            std::thread::sleep(std::time::Duration::from_millis(200));
        };
        if status.success() {
            return Ok(Ok(()));
        }
        let mut error = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            use std::io::Read;
            let _ = stderr.read_to_string(&mut error);
        }
        Ok(Err(error))
    };
    match attempt(token)? {
        Err(e) if token.is_some() && crate::updates::refused(&e) => attempt(None)?,
        result => result,
    }
    .map_err(|e| format!("No se pudo descargar el modelo (se seguirá desde aquí la próxima vez): {}", crate::updates::curl_error(&e)))
}

/// A file's SHA-256, in hex.
pub(crate) fn sha256(path: &Path) -> Result<String, String> {
    use sha2::Digest;
    let mut file = std::fs::File::open(path).map_err(|e| format!("No se pudo comprobar {}: {e}", path.display()))?;
    let mut hasher = sha2::Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| format!("No se pudo comprobar {}: {e}", path.display()))?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    #[test]
    fn sha256_matches_the_known_digest() {
        let file = std::env::temp_dir().join(format!("pulse sha ñ {}.txt", std::process::id()));
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(sha256(&file).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        std::fs::remove_file(&file).unwrap();
    }

    fn audio_clip(path: &str) -> AudioClip {
        AudioClip {
            name: "Canción".into(),
            color: [1, 2, 3],
            sample: path.into(),
            length: 8.0,
            offset: 0.5,
            warp: engine::model::WarpMode::Stretch,
            original_bpm: 124.0,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: -3.0,
            transpose: 0.0,
            envelopes: Vec::new(), muted: false,
        }
    }

    /// Four stem files (silence) in a temporary folder.
    fn stem_files(dir: &Path) -> [PathBuf; 4] {
        std::fs::create_dir_all(dir).unwrap();
        std::array::from_fn(|i| {
            let path = dir.join(format!("s{i}.wav"));
            crate::capture::write_wav(&path, &[0.0; 100], &[0.0; 100], 44_100.0).unwrap();
            path
        })
    }

    #[test]
    fn stems_land_under_the_clip_in_the_same_place() {
        let dir = std::env::temp_dir().join(format!("pulse-stems-{}", std::process::id()));
        let paths = stem_files(&dir);
        let mut app = PulseApp::headless(Project::demo());
        app.project.tracks.truncate(2);
        let source = app.add_audio_track().unwrap();
        let mut audio = audio_clip("/x/song.wav");
        audio.envelopes.push(engine::model::AutomationLane { param: engine::model::Param::Volume, points: vec![engine::model::AutomationPoint { beat: 0.0, value: 0.5, curve: 0.0 }] });
        // The original's sound: an effect, its fader and a send.
        app.project.tracks[source].devices.push(engine::model::Device::new(engine::model::Effect::Saturator(Default::default())));
        app.project.tracks[source].mixer.gain = 0.7;
        app.project.tracks[source].sends[0] = 0.3;
        app.set_clip(source, 3, Some(Clip::Audio(audio.clone())));
        app.place_stems(ClipRef::Session { track: source, slot: 3 }, &audio, &paths);
        for stem in &app.project.tracks[source + 1..source + 5] {
            assert_eq!((stem.devices.len(), stem.mixer.gain, stem.sends[0], stem.mixer.active), (1, 0.7, 0.3, true), "{}: sounds like the original", stem.name);
            let Some(Clip::Audio(clip)) = &stem.clips[3] else { panic!() };
            assert_eq!(clip.envelopes, audio.envelopes);
        }
        let ids: std::collections::HashSet<u32> = app.project.tracks.iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), app.project.tracks.len(), "each track its own id");

        let names: Vec<&str> = app.project.tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(&names[3..], ["Canción · Batería", "Canción · Bajo", "Canción · Resto", "Canción · Voz"], "{names:?}");
        for (i, path) in paths.iter().enumerate() {
            let Some(Clip::Audio(stem)) = &app.project.tracks[source + 1 + i].clips[3] else { panic!("a clip in the same slot") };
            assert_eq!(stem.sample, path.to_string_lossy());
            assert_eq!((stem.offset, stem.length, stem.warp, stem.original_bpm, stem.gain_db), (0.5, 8.0, engine::model::WarpMode::Stretch, 124.0, -3.0));
        }
        assert!(!app.project.tracks[source].mixer.active, "the original goes silent");
        assert_eq!(app.selected_clip, Some(ClipRef::Session { track: source + 1, slot: 3 }));

        // On the arrangement: same start and length.
        let mut app = PulseApp::headless(Project::demo());
        app.project.tracks.truncate(1);
        let source = app.add_audio_track().unwrap();
        app.project.tracks[source].arrangement.push(ArrangementClip { start: 16.0, length: 12.0, clip: Clip::Audio(audio.clone()) });
        app.place_stems(ClipRef::Arrangement { track: source, index: 0 }, &audio, &paths);
        for i in 0..4 {
            let placed = &app.project.tracks[source + 1 + i].arrangement[0];
            assert_eq!((placed.start, placed.length), (16.0, 12.0));
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_room_for_four_tracks_no_separation() {
        let mut app = PulseApp::headless(Project::demo());
        while app.project.tracks.len() < MAX_TRACKS - 2 {
            app.add_track();
        }
        let t = app.add_audio_track().unwrap();
        app.set_clip(t, 0, Some(Clip::Audio(audio_clip("/x/song.wav"))));
        app.separate_stems(ClipRef::Session { track: t, slot: 0 });
        assert!(app.stems.is_none());
        assert!(app.notice.as_ref().is_some_and(|(_, text)| text.contains("pistas libres")), "{:?}", app.notice);
    }

    #[test]
    fn taken_names_get_a_number() {
        let dir = std::env::temp_dir().join(format!("pulse-unique-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a - Voz.wav");
        assert_eq!(unique(&path), path);
        std::fs::write(&path, b"").unwrap();
        assert_eq!(unique(&path), dir.join("a - Voz 2.wav"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The whole thing with the real model (PULSE_STEMS_MODEL) on demo.wav.
    #[test]
    #[ignore = "needs PULSE_STEMS_MODEL"]
    fn a_clip_is_separated_into_four_tracks() {
        let song = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo.wav");
        let mut app = PulseApp::headless(Project::demo());
        app.project.tracks.truncate(2);
        let t = app.add_audio_track().unwrap();
        app.set_clip(t, 0, Some(Clip::Audio(audio_clip(&song.to_string_lossy()))));
        app.separate_stems(ClipRef::Session { track: t, slot: 0 });
        let ctx = egui::Context::default();
        let started = std::time::Instant::now();
        while app.stems.is_some() {
            let _ = ctx.run_ui(Default::default(), |ui| app.follow_stems(ui.ctx()));
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert!(started.elapsed().as_secs() < 300);
        }
        println!("{:?} in {:.1} s", app.status, started.elapsed().as_secs_f32());
        assert_eq!(app.project.tracks.len(), 7, "{:?}", app.status);
        for i in 0..4 {
            let Some(Clip::Audio(stem)) = &app.project.tracks[t + 1 + i].clips[0] else { panic!("{:?}", app.status) };
            let sample = engine::Sample::load(Path::new(&stem.sample)).unwrap();
            let energy: f32 = sample.left.iter().map(|x| x * x).sum();
            println!("{}: {:.1} s, energía {energy:.1}", NAMES[i], sample.duration());
            let _ = std::fs::remove_file(&stem.sample);
        }
    }
}

