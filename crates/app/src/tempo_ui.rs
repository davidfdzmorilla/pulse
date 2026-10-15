//! Auto-warp: imported audio follows the project's tempo. Each new file is
//! analysed in the background (the `beats` crate: its tempo and where its bars
//! fall); its clips then play in Stretch at the tempo found, starting on a bar
//! line so its downbeats land on the project's, and as many whole bars long as
//! the file. "Usar el tempo de este clip" sets the project to the clip's tempo.

use crate::app::{ClipRef, PulseApp};
use beats::Grid;
use engine::model::{AudioClip, Clip, WarpMode};
use engine::Sample;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type Outcome = Result<Option<Grid>, String>;

/// A file being analysed, and the clips waiting for the result.
pub struct TempoJob {
    path: String,
    clips: Vec<ClipRef>,
    /// Asked for from a menu (then the clip is set whatever it was).
    manual: bool,
    result: Arc<Mutex<Option<Outcome>>>,
}

/// What has been found per file, and what is being analysed.
#[derive(Default)]
pub struct Tempo {
    grids: HashMap<String, Option<Grid>>,
    jobs: Vec<TempoJob>,
}

impl Tempo {
    pub fn busy(&self) -> bool {
        !self.jobs.is_empty()
    }
}

/// A clip set to follow the tempo with `grid`, for a file `duration` seconds long.
pub fn warped(clip: &AudioClip, grid: &Grid, duration: f64) -> AudioClip {
    let start = grid.start();
    let bars = ((duration - start) / grid.bar_seconds() - 0.02).ceil().max(1.0);
    AudioClip { warp: WarpMode::Stretch, original_bpm: grid.bpm, offset: start, length: bars * beats::BEATS_PER_BAR, ..clip.clone() }
}

impl PulseApp {
    /// Finds the tempo of the audio clips' file and sets them to follow the
    /// project's (in the background; at once if the file was analysed before).
    /// `manual`: asked for from a menu.
    pub fn detect_tempo(&mut self, clips: Vec<ClipRef>, manual: bool) {
        let Some(Clip::Audio(first)) = clips.first().and_then(|&c| self.clip(c)).cloned() else { return };
        let path = first.sample.clone();
        if let Some(grid) = self.tempo.grids.get(&path).cloned() {
            self.finish_tempo(&path, &clips, Ok(grid), manual);
            return;
        }
        if let Some(job) = self.tempo.jobs.iter_mut().find(|j| j.path == path) {
            job.clips.extend(clips);
            job.manual |= manual;
            return;
        }
        let sample: Arc<Sample> = self.sample(&path);
        let result = Arc::new(Mutex::new(None));
        let shared = result.clone();
        std::thread::spawn(move || {
            let outcome = beats::analyze(&sample.left, &sample.right, sample.sample_rate);
            if let Ok(mut r) = shared.lock() {
                *r = Some(outcome);
            }
        });
        self.status = Some(format!("Analizando el tempo de «{}»…", first.name));
        self.tempo.jobs.push(TempoJob { path, clips, manual, result });
    }

    /// Every frame: applies the analyses that have finished.
    pub fn follow_tempo(&mut self, ctx: &eframe::egui::Context) {
        if !self.tempo.busy() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let done: Vec<usize> = (0..self.tempo.jobs.len()).filter(|&i| self.tempo.jobs[i].result.lock().is_ok_and(|r| r.is_some())).collect();
        for i in done.into_iter().rev() {
            let job = self.tempo.jobs.remove(i);
            let outcome = job.result.lock().ok().and_then(|mut r| r.take()).unwrap_or(Err("Análisis interrumpido".into()));
            if let Ok(grid) = &outcome {
                self.tempo.grids.insert(job.path.clone(), grid.clone());
            }
            self.finish_tempo(&job.path, &job.clips, outcome, job.manual);
        }
    }

    fn finish_tempo(&mut self, path: &str, clips: &[ClipRef], outcome: Outcome, manual: bool) {
        let name = std::path::Path::new(path).file_stem().map_or(path.to_string(), |s| s.to_string_lossy().into_owned());
        let grid = match outcome {
            Ok(Some(grid)) => grid,
            Ok(None) => {
                self.status = Some(if manual {
                    format!("«{name}»: no se encontró un ritmo claro")
                } else {
                    format!("«{name}»: no se encontró un ritmo claro; suena a su velocidad")
                });
                return;
            }
            Err(e) => {
                self.status = Some(e);
                return;
            }
        };
        let duration = self.sample(path).duration();
        let mut count = 0;
        for &clip in clips {
            // Only clips still on that file (the user may have changed them meanwhile).
            let Some(Clip::Audio(audio)) = self.clip(clip).cloned() else { continue };
            if audio.sample != path {
                continue;
            }
            let new = warped(&audio, &grid, duration);
            match clip {
                ClipRef::Session { track, slot } => self.set_clip(track, slot, Some(Clip::Audio(new))),
                ClipRef::Arrangement { track, index } => {
                    let placed = &mut self.project.tracks[track].arrangement[index];
                    // Its length on the timeline follows, unless the user changed it.
                    if (placed.length - audio.length).abs() < 1e-9 {
                        placed.length = new.length;
                    }
                    placed.clip = Clip::Audio(new);
                    self.sync_arrangement(track);
                }
            }
            count += 1;
        }
        if count > 0 {
            let tempo = self.project.tempo;
            self.status = Some(if (grid.bpm - tempo).abs() < 0.01 {
                format!("«{name}» va a {} BPM, como el proyecto", bpm_text(grid.bpm))
            } else {
                format!("«{name}» va a {} BPM y ahora sigue el tempo del proyecto ({} BPM)", bpm_text(grid.bpm), bpm_text(tempo))
            });
        }
    }

    /// Sets the project's tempo to the clip's own (its original tempo).
    pub fn use_clip_tempo(&mut self, clip: ClipRef) {
        let Some(Clip::Audio(audio)) = self.clip(clip) else { return };
        if audio.warp == WarpMode::Off {
            self.status = Some("Este clip no tiene tempo: usa «Detectar tempo» primero".into());
            return;
        }
        let bpm = audio.original_bpm;
        self.project.tempo = bpm;
        self.audio.send(engine::Command::SetTempo(bpm));
        self.status = Some(format!("Tempo del proyecto: {} BPM", bpm_text(bpm)));
    }
}

fn bpm_text(bpm: f64) -> String {
    if (bpm - bpm.round()).abs() < 0.005 { format!("{bpm:.0}") } else { format!("{bpm:.2}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Placement;
    use crate::project::Project;

    #[test]
    fn a_clip_follows_with_whole_bars_from_the_bar_line() {
        let clip = AudioClip {
            name: "x".into(),
            color: [0; 3],
            sample: "/x.wav".into(),
            length: 10.0,
            offset: 0.0,
            warp: WarpMode::Off,
            original_bpm: 120.0,
            gain_db: -2.0,
            transpose: 0.0,
            envelopes: Vec::new(),
        };
        // 124 BPM, the first downbeat 0.3 s in, 60 s long.
        let grid = Grid { bpm: 124.0, downbeat: 0.3, beats: 120 };
        let w = warped(&clip, &grid, 60.0);
        let bar = 4.0 * 60.0 / 124.0;
        assert_eq!((w.warp, w.original_bpm, w.gain_db), (WarpMode::Stretch, 124.0, -2.0));
        assert!((w.offset - (0.3 - bar)).abs() < 1e-9, "the bar line before the downbeat: {}", w.offset);
        assert_eq!(w.length, ((60.0 - w.offset) / bar).ceil() * 4.0);
        // A loop of exactly 4 bars on the beat: 16 beats from the start.
        let w = warped(&clip, &Grid { bpm: 120.0, downbeat: 0.01, beats: 16 }, 8.0);
        assert_eq!((w.offset, w.length), (0.0, 16.0));
    }

    /// The real thing: Pulse's demo song (120 BPM) imported into a 100 BPM project.
    #[test]
    fn an_imported_song_follows_the_project() {
        let song = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo.wav");
        let mut app = PulseApp::headless(Project::demo());
        app.project.tempo = 100.0;
        app.view = crate::app::View::Arrangement;
        let track = app.add_audio_track().unwrap();
        app.add_samples(vec![song.clone()], Placement::Arrangement { track, start: 8.0 });
        assert!(app.tempo.busy(), "analysing");
        let ctx = eframe::egui::Context::default();
        let started = std::time::Instant::now();
        while app.tempo.busy() {
            app.follow_tempo(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(20));
            assert!(started.elapsed().as_secs() < 60);
        }
        let placed = &app.project.tracks[track].arrangement[0];
        let Clip::Audio(audio) = &placed.clip else { panic!() };
        assert_eq!((audio.warp, audio.original_bpm), (WarpMode::Stretch, 120.0), "{:?}", app.status);
        assert_eq!(audio.offset, 0.0, "it starts on the beat");
        assert_eq!(placed.start, 8.0);
        assert_eq!(placed.length, audio.length);
        assert_eq!(audio.length % 4.0, 0.0, "whole bars");
        assert!(app.status.as_deref().unwrap_or("").contains("120 BPM"), "{:?}", app.status);
        // To its own tempo.
        app.use_clip_tempo(ClipRef::Arrangement { track, index: 0 });
        assert_eq!(app.project.tempo, 120.0);
        // Imported again: from the cache, at once.
        app.add_samples(vec![song], Placement::Arrangement { track, start: 40.0 });
        assert!(!app.tempo.busy());
        let Clip::Audio(again) = &app.project.tracks[track].arrangement[1].clip else { panic!() };
        assert_eq!(again.warp, WarpMode::Stretch);
    }

    /// Heard, not just set: the project at 100 BPM with the song warped, rendered
    /// from the clip's start, has the song's beats at 100 BPM on the project's bars.
    #[test]
    fn a_warped_song_plays_in_time_with_the_project() {
        let song = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo.wav");
        let mut app = PulseApp::headless(Project::demo());
        app.project.tempo = 100.0;
        // Only the song: the demo's own tracks silent.
        for track in &mut app.project.tracks {
            track.mixer.active = false;
        }
        let track = app.add_audio_track().unwrap();
        app.add_samples(vec![song], Placement::Arrangement { track, start: 8.0 });
        let ctx = eframe::egui::Context::default();
        while app.tempo.busy() {
            app.follow_tempo(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let rate = 48_000.0;
        let mut renderer = engine::OfflineRenderer::new(rate);
        let mut samples = crate::sync::SampleCache::default();
        for command in crate::sync::project_commands(&app.project, &mut samples, rate, &Default::default()) {
            renderer.send(command);
        }
        renderer.send(engine::Command::SetPosition(8.0));
        renderer.send(engine::Command::Play);
        // 32 beats at 100 BPM.
        let (mut l, mut r) = (Vec::new(), Vec::new());
        renderer.render((32.0 * 60.0 / 100.0 * rate as f64) as usize, &mut l, &mut r);
        let grid = beats::analyze(&l, &r, rate).unwrap().expect("beats in the render");
        println!("{grid:?}");
        assert!((grid.bpm - 100.0).abs() < 0.5, "{grid:?}");
        // The song's first downbeat is on the project's bar line (where the render starts).
        let beat = 60.0 / 100.0;
        let off = grid.downbeat / beat;
        assert!((off - off.round()).abs() < 0.08 && (off.round() as i64) % 4 == 0, "downbeat {} s: {off} beats", grid.downbeat);
    }
}

