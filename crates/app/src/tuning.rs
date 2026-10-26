//! Tuning a recorded voice note by note (see `stems::tune` and `tuning_ui`).
//!
//! A clip's tuning lives in the project: the file as recorded, the notes
//! with where each is taken, and the tuned file the clips play. Each state
//! is a file of its own, named after what was done, so undoing puts the clip
//! back on a file that is still there; the ones no state uses any more go
//! when the project is saved.

use crate::app::{ClipRef, PulseApp};
use engine::model::Clip;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use stems::tune::{Analysis, NoteEdit};

/// The folder (in the project's media) the tuned files go to.
pub const TUNED: &str = "Afinados";

fn one() -> f32 {
    1.0
}

/// A note of a tuned voice: where it is in the file (seconds), where it was
/// sung (fractional MIDI), where it is taken and how much of its wavering
/// stays (1: all, 0: none).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TunedNote {
    pub start: f64,
    pub end: f64,
    pub pitch: f32,
    pub target: f32,
    #[serde(default = "one")]
    pub modulation: f32,
}

impl TunedNote {
    pub fn edited(&self) -> bool {
        (self.target - self.pitch).abs() > 1e-4 || (self.modulation - 1.0).abs() > 1e-4
    }

    fn edit(&self) -> NoteEdit {
        NoteEdit { start: self.start, end: self.end, pitch: self.pitch, target: self.target, modulation: self.modulation }
    }
}

/// A voice tuned: the file it was, its notes, and the file it is now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tuning {
    pub id: u32,
    pub source: String,
    pub output: String,
    pub notes: Vec<TunedNote>,
}

/// What was done to the notes, as a number that names the file (the same
/// edits, the same name, every time).
fn fingerprint(notes: &[TunedNote]) -> u32 {
    // FNV-1a over what moves a note.
    let mut hash: u32 = 0x811c_9dc5;
    for n in notes.iter().filter(|n| n.edited()) {
        for word in [n.start.to_bits() as u32, (n.start.to_bits() >> 32) as u32, n.target.to_bits(), n.modulation.to_bits()] {
            for byte in word.to_le_bytes() {
                hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
            }
        }
    }
    hash
}

/// The notes of an analysis, as sung (nothing moved yet).
pub fn notes_of(analysis: &Analysis) -> Vec<TunedNote> {
    analysis.notes.iter().map(|n| TunedNote { start: n.start, end: n.end, pitch: n.pitch, target: n.pitch, modulation: 1.0 }).collect()
}

type Job = Arc<Mutex<Option<Analysis>>>;

/// What the window keeps while tuning: the clip open in the editor, the
/// voices analysed (by file) and being analysed, the notes chosen and the
/// files written in the background.
#[derive(Default)]
pub struct Tuner {
    pub open: Option<ClipRef>,
    analyses: HashMap<String, Arc<Analysis>>,
    jobs: Vec<(String, Job)>,
    pub selected: HashSet<usize>,
    writes: Vec<std::thread::JoinHandle<()>>,
    /// The tuned files made this session (only those may be deleted).
    made: HashSet<String>,
}

impl Tuner {
    pub fn analysis(&self, source: &str) -> Option<Arc<Analysis>> {
        self.analyses.get(source).cloned()
    }

    pub fn analysing(&self, source: &str) -> bool {
        self.jobs.iter().any(|(s, _)| s == source)
    }
}

impl PulseApp {
    /// The tuning `clip` plays, if it plays a tuned file (its index).
    pub fn tuning_of(&self, clip: ClipRef) -> Option<usize> {
        let Some(Clip::Audio(audio)) = self.clip(clip) else { return None };
        self.project.tunings.iter().position(|t| t.output == audio.sample)
    }

    /// The file as recorded that `clip` tunes (its own, if not tuned yet).
    pub fn tuning_source(&self, clip: ClipRef) -> Option<String> {
        let Some(Clip::Audio(audio)) = self.clip(clip) else { return None };
        Some(match self.tuning_of(clip) {
            Some(i) => self.project.tunings[i].source.clone(),
            None => audio.sample.clone(),
        })
    }

    /// The notes `clip` has now: its tuning's, or as sung (once analysed).
    pub fn tuning_notes(&self, clip: ClipRef) -> Option<Vec<TunedNote>> {
        if let Some(i) = self.tuning_of(clip) {
            return Some(self.project.tunings[i].notes.clone());
        }
        let analysis = self.tuner.analysis(&self.tuning_source(clip)?)?;
        Some(notes_of(&analysis))
    }

    /// Opens the tuning editor on `clip` (an audio clip), analysing its voice
    /// if it hasn't been.
    pub fn open_tuning(&mut self, clip: ClipRef) {
        let Some(source) = self.tuning_source(clip) else { return };
        self.tuner.open = Some(clip);
        self.tuner.selected.clear();
        if self.tuner.analysis(&source).is_some() || self.tuner.analysing(&source) {
            return;
        }
        let sample = self.sample(&source);
        if sample.frames() == 0 {
            self.status = Some("No se encuentra el audio de este clip".into());
            self.tuner.open = None;
            return;
        }
        let job: Job = Arc::new(Mutex::new(None));
        let out = job.clone();
        std::thread::spawn(move || {
            let mono: Vec<f32> = sample.left.iter().zip(&sample.right).map(|(l, r)| 0.5 * (l + r)).collect();
            let analysis = stems::tune::analyse(&mono, sample.sample_rate);
            *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(analysis);
        });
        self.tuner.jobs.push((source, job));
        self.status = Some("Analizando la voz: sus notas y su afinación…".into());
    }

    /// Every frame: analyses finished.
    pub fn follow_tuning(&mut self, ctx: &eframe::egui::Context) {
        if self.tuner.jobs.is_empty() {
            return;
        }
        let mut done = Vec::new();
        self.tuner.jobs.retain(|(source, job)| match job.lock().unwrap_or_else(|e| e.into_inner()).take() {
            Some(analysis) => {
                done.push((source.clone(), analysis));
                false
            }
            None => true,
        });
        for (source, analysis) in done {
            let count = analysis.notes.len();
            self.tuner.analyses.insert(source, Arc::new(analysis));
            self.status = Some(match count {
                0 => "No se oyen notas en este clip: la afinación es para una voz o un instrumento que toque una nota a la vez".into(),
                n => format!("{n} notas: arrastra una arriba o abajo para cambiarla de nota"),
            });
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    /// Waits for the analysis of `source` (tests).
    #[cfg(test)]
    pub fn wait_tuning_analysis(&mut self, source: &str) {
        let ctx = eframe::egui::Context::default();
        let started = std::time::Instant::now();
        while self.tuner.analysing(source) {
            self.follow_tuning(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert!(started.elapsed().as_secs() < 60, "the analysis never finished");
        }
    }

    /// Gives `clip` these notes: its voice rendered with them into a file
    /// (or the one made before for the same), which the clip (and its copies)
    /// play from now on. Notes as sung: the file as recorded again.
    pub fn apply_tuning(&mut self, clip: ClipRef, notes: Vec<TunedNote>) -> Result<(), String> {
        let source = self.tuning_source(clip).ok_or("No es un clip de audio")?;
        let analysis = self.tuner.analysis(&source).ok_or("La voz aún se está analizando")?;
        let Some(Clip::Audio(audio)) = self.clip(clip).cloned() else { return Err("No es un clip de audio".into()) };
        let current = self.tuning_of(clip);
        let id = match current {
            Some(i) => self.project.tunings[i].id,
            None => self.project.tunings.iter().map(|t| t.id).max().unwrap_or(0) + 1,
        };
        let output = if notes.iter().any(TunedNote::edited) {
            let stem = std::path::Path::new(&source).file_stem().map_or("voz".into(), |s| s.to_string_lossy().into_owned());
            let dir = self.media_dir(TUNED);
            let path = dir.join(format!("{} (afinado {id}-{:08x}).wav", crate::files::safe_name(&stem), fingerprint(&notes)));
            let path_text = path.to_string_lossy().into_owned();
            if !path.exists() && self.samples.loaded(&path_text).is_none() {
                let sample = self.sample(&source);
                let edits: Vec<NoteEdit> = notes.iter().map(TunedNote::edit).collect();
                let mut moved = stems::tune::render(&[&sample.left, &sample.right], sample.sample_rate, &analysis, &edits);
                let (right, left) = (moved.pop().unwrap_or_default(), moved.pop().unwrap_or_default());
                std::fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
                let rate = sample.sample_rate;
                let tuned = Arc::new(engine::Sample::from_channels(left, right, rate));
                self.samples.insert(path_text.clone(), tuned.clone());
                let target = path.clone();
                self.tuner.writes.push(std::thread::spawn(move || {
                    if let Err(e) = crate::export::write(&target, &tuned.left, &tuned.right, rate as u32, crate::export::BitDepth::Float32, 1.0) {
                        crate::logs::note(&e);
                    }
                }));
                self.tuner.made.insert(path_text.clone());
            }
            path_text
        } else {
            source.clone()
        };
        let before = audio.sample.clone();
        // A tuned file is this clip's and its copies': they all move. The file
        // as recorded may be played by other clips, untuned: only this one.
        if before != source {
            for track in &mut self.project.tracks {
                for c in track.clips.iter_mut().flatten().chain(track.arrangement.iter_mut().map(|a| &mut a.clip)) {
                    if let Clip::Audio(a) = c {
                        if a.sample == before {
                            a.sample = output.clone();
                        }
                    }
                }
            }
        } else if let Some(Clip::Audio(a)) = self.clip_mut(clip) {
            a.sample = output.clone();
        }
        let tuning = Tuning { id, source: source.clone(), output: output.clone(), notes };
        match current {
            Some(i) => self.project.tunings[i] = tuning,
            None => self.project.tunings.push(tuning),
        }
        // Back as recorded, or no clip plays it any more: no tuning.
        let playing = self.audio_files_playing();
        self.project.tunings.retain(|t| t.output != t.source && playing.contains(&t.output));
        self.sync_all();
        Ok(())
    }

    /// The files the project's audio clips play.
    fn audio_files_playing(&self) -> HashSet<String> {
        let mut files = HashSet::new();
        for track in &self.project.tracks {
            for c in track.clips.iter().flatten().chain(track.arrangement.iter().map(|a| &a.clip)) {
                if let Clip::Audio(a) = c {
                    files.insert(a.sample.clone());
                }
            }
        }
        files
    }

    /// Waits for the tuned files still being written (before saving: they
    /// are copied with the project).
    pub fn finish_tuning_writes(&mut self) {
        for write in self.tuner.writes.drain(..) {
            let _ = write.join();
        }
    }

    /// Deletes the tuned files made this session that neither the project nor
    /// any state to undo or redo to plays.
    pub fn clean_tunings(&mut self) {
        self.finish_tuning_writes();
        let mut used: HashSet<String> = HashSet::new();
        for project in std::iter::once(&self.project).chain(self.history.states()) {
            used.extend(project.tunings.iter().map(|t| t.output.clone()));
        }
        let gone: Vec<String> = self.tuner.made.iter().filter(|f| !used.contains(*f)).cloned().collect();
        for file in gone {
            let _ = std::fs::remove_file(&file);
            self.tuner.made.remove(&file);
        }
    }
}
