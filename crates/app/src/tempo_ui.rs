//! Auto-warp: imported audio follows the project's tempo. Each new file is
//! analysed in the background (the `beats` crate: its tempo and where its bars
//! fall); its clips then play in Stretch at the tempo found, starting on a bar
//! line so its downbeats land on the project's, and as many whole bars long as
//! the file. "Usar el tempo de este clip" sets the project to the clip's tempo.

use crate::app::{ClipRef, PulseApp};
use beats::Grid;
use engine::model::{AudioClip, Clip, WarpMarker, WarpMode};
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

    /// What was found for a file: `None` if it hasn't been analysed.
    pub fn grid(&self, path: &str) -> Option<Option<Grid>> {
        self.grids.get(path).cloned()
    }

    /// Keeps what an analysis found for a file.
    pub fn remember(&mut self, path: &str, grid: Option<Grid>) {
        self.grids.insert(path.to_string(), grid);
    }
}

/// A clip set to follow the tempo with `grid`, for a file `duration` seconds long:
/// from the bar line at or before the file's start, whole bars long, between
/// warp markers on every bar if its tempo varies.
/// A drum loop, which keeps its hits clean in Beats: one up to 32 s that its
/// name says is drums, or one up to 16 s (a loop, not a mix) without pitches
/// that stand out.
pub fn is_drum_loop(name: &str, sample: &Sample) -> bool {
    let duration = sample.duration();
    (duration <= 32.0 && crate::loop_key::is_drums(name)) || (duration <= 16.0 && !tonal_sound(sample))
}

/// Whether a file has pitches that stand out (a loop without is drums).
fn tonal_sound(sample: &Sample) -> bool {
    let mono: Vec<f32> = sample.left.iter().zip(&sample.right).map(|(l, r)| 0.5 * (l + r)).collect();
    crate::loop_key::sounds_tonal(&mono, sample.sample_rate)
}

/// A loop's tempo from its name («Tops 150», «Bass_128bpm_Am»): a number from 60
/// to 200 at which the file lasts whole bars.
pub fn name_bpm(name: &str, duration: f64) -> Option<f64> {
    name.split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter_map(|word| word.trim_matches('.').parse::<f64>().ok())
        .filter(|bpm| (60.0..=200.0).contains(bpm))
        .find(|bpm| {
            let bars = duration * bpm / 240.0;
            bars >= 0.9 && (bars - bars.round()).abs() < 0.02
        })
}

/// A loop's grid: its name's tempo when it has one (a loop named «Tops 150»
/// that lasts whole bars at 150 is at 150, whatever the detector heard: a loop
/// of tops without a kick can come out at 245), else the detector's, fitted to
/// whole bars (see `beats::fit_loop`). None when neither knows.
pub fn loop_grid(grid: Option<Grid>, name: &str, duration: f64, project_bpm: f64) -> Option<Grid> {
    match name_bpm(name, duration).filter(|_| duration <= 32.0) {
        Some(bpm) => Some(Grid { bpm, downbeat: 0.0, beats: grid.map_or(0, |g| g.beats), markers: Vec::new() }),
        None => grid.map(|g| beats::fit_loop(g, duration, Some(project_bpm))),
    }
}

pub fn warped(clip: &AudioClip, grid: &Grid, duration: f64) -> AudioClip {
    let mut new = AudioClip { warp: WarpMode::Stretch, original_bpm: grid.bpm, warp_markers: Vec::new(), ..clip.clone() };
    if grid.markers.len() < 2 {
        let start = grid.start();
        let bars = ((duration - start) / grid.bar_seconds() - 0.02).ceil().max(1.0);
        return AudioClip { offset: start, length: bars * beats::BEATS_PER_BAR, ..new };
    }
    new.warp_markers = grid.markers.iter().map(|&(beat, seconds)| WarpMarker { beat, seconds }).collect();
    let bar = beats::BEATS_PER_BAR;
    // The file's start, in beats from its first downbeat (≤ 0).
    let before = new.file_beat(0.0);
    let start_beat = if grid.start() == 0.0 { before } else { -(-before / bar - 1e-6).ceil().max(0.0) * bar };
    new.offset = new.file_seconds(start_beat);
    let bars = ((new.file_beat(duration) - start_beat) / bar - 0.02).ceil().max(1.0);
    new.length = bars * bar;
    new
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
        let duration = self.sample(path).duration();
        let grid = match outcome.map(|grid| loop_grid(grid, &name, duration, self.project.tempo)) {
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
        let drum_loop = is_drum_loop(&name, &self.sample(path));
        let mut count = 0;
        for &clip in clips {
            // Only clips still on that file (the user may have changed them meanwhile).
            let Some(Clip::Audio(audio)) = self.clip(clip).cloned() else { continue };
            if audio.sample != path {
                continue;
            }
            let mut new = warped(&audio, &grid, duration);
            if drum_loop && new.warp == WarpMode::Stretch {
                new.warp = WarpMode::Beats;
            }
            match clip {
                ClipRef::Session { track, slot } => self.set_clip(track, slot, Some(Clip::Audio(new))),
                ClipRef::Arrangement { track, index } => {
                    let lane = &mut self.project.tracks[track].arrangement;
                    let placed = &mut lane[index];
                    // Its length on the timeline follows, unless the user changed it;
                    // the clips placed right after it (files imported together) move
                    // with its end, so there are no gaps or overlaps.
                    if (placed.length - audio.length).abs() < 1e-9 {
                        let old_end = placed.end();
                        placed.length = new.length;
                        let shift = placed.end() - old_end;
                        placed.clip = Clip::Audio(new);
                        follow_end(lane, index, old_end, shift);
                    } else {
                        placed.clip = Clip::Audio(new);
                    }
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

    /// A file's tempo: found before, or now (it takes a moment: about a second a
    /// minute of audio). None without a clear beat.
    pub fn file_bpm(&mut self, path: &str) -> Option<f64> {
        // A loop named with its tempo is at that tempo (see `loop_grid`).
        let name = std::path::Path::new(path).file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned());
        if let Some(bpm) = name_bpm(&name, self.sample(path).duration()).filter(|_| self.sample(path).duration() <= 32.0) {
            return Some(bpm);
        }
        if let Some(grid) = self.tempo.grids.get(path) {
            return grid.as_ref().map(|g| g.bpm);
        }
        let sample = self.sample(path);
        let grid = beats::analyze(&sample.left, &sample.right, sample.sample_rate).ok().flatten();
        self.tempo.grids.insert(path.to_string(), grid.clone());
        grid.map(|g| g.bpm)
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

/// Moves by `shift` the clips that start where clip `index` used to end, and those
/// right after them, in a chain (the files of one import, one after the other).
fn follow_end(lane: &mut [engine::model::ArrangementClip], index: usize, old_end: f64, shift: f64) {
    if shift.abs() < 1e-9 {
        return;
    }
    let mut end = old_end;
    let mut moved = vec![index];
    while let Some(next) = (0..lane.len()).find(|i| !moved.contains(i) && (lane[*i].start - end).abs() < 1e-6) {
        end = lane[next].end();
        lane[next].start += shift;
        moved.push(next);
    }
}

pub fn bpm_text(bpm: f64) -> String {
    if (bpm - bpm.round()).abs() < 0.005 { format!("{bpm:.0}") } else { format!("{bpm:.2}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Placement;
    use crate::project::Project;

    #[test]
    fn a_clip_follows_with_whole_bars_from_the_bar_line() {
        let clip = AudioClip { hard_start: false,
            name: "x".into(),
            color: [0; 3],
            sample: "/x.wav".into(),
            length: 10.0,
            offset: 0.0,
            warp: WarpMode::Off,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: -2.0,
            transpose: 0.0,
            envelopes: Vec::new(), muted: false,
        };
        // 124 BPM, the first downbeat 0.3 s in, 60 s long.
        let grid = Grid { bpm: 124.0, downbeat: 0.3, beats: 120, markers: Vec::new() };
        let w = warped(&clip, &grid, 60.0);
        let bar = 4.0 * 60.0 / 124.0;
        assert_eq!((w.warp, w.original_bpm, w.gain_db), (WarpMode::Stretch, 124.0, -2.0));
        assert!((w.offset - (0.3 - bar)).abs() < 1e-9, "the bar line before the downbeat: {}", w.offset);
        assert_eq!(w.length, ((60.0 - w.offset) / bar).ceil() * 4.0);
        // A loop of exactly 4 bars on the beat: 16 beats from the start.
        let w = warped(&clip, &Grid { bpm: 120.0, downbeat: 0.01, beats: 16, markers: Vec::new() }, 8.0);
        assert_eq!((w.offset, w.length), (0.0, 16.0));
    }

    #[test]
    fn a_varying_tempo_is_warped_bar_by_bar() {
        let clip = AudioClip { hard_start: false,
            name: "x".into(),
            color: [0; 3],
            sample: "/x.wav".into(),
            length: 10.0,
            offset: 0.0,
            warp: WarpMode::Off,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            key: None,
            takes: Vec::new(),
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(), muted: false,
        };
        // Bars of 2 s then 2.4 s, the first downbeat 1.1 s in; 10 s long.
        let markers = vec![(0.0, 1.1), (4.0, 3.1), (8.0, 5.5)];
        let grid = Grid { bpm: 110.0, downbeat: 1.1, beats: 12, markers };
        let w = warped(&clip, &grid, 10.0);
        assert_eq!((w.warp, w.warp_markers.len()), (WarpMode::Stretch, 3));
        // The bar line before the file: beat -4, 0.9 s before it (at the first bar's tempo).
        assert!((w.offset + 0.9).abs() < 1e-9, "{}", w.offset);
        assert!((w.source_seconds(4.0, 100.0) - 1.1).abs() < 1e-9, "the first downbeat on the clip's bar 2");
        assert!((w.source_seconds(12.0, 100.0) - 5.5).abs() < 1e-9);
        // 10 s is beat 8 + 4.5 / 0.6 = 15.5, 19.5 from the bar line before: 5 bars.
        assert_eq!(w.length, 20.0);
    }

    /// The real thing: Pulse's demo song (120 BPM) imported into a 100 BPM project.
    #[test]
    fn an_imported_song_follows_the_project() {
        let song = crate::test_audio::demo_song();
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
        let song = crate::test_audio::demo_song();
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

    /// A drummer speeding up from 108 to 128 BPM over `bars` bars, 0.7 s in.
    fn write_drummer(path: &std::path::Path, bars: usize) {
        let rate = 44_100u32;
        let mut times = Vec::new();
        let mut t = 0.7;
        for i in 0..bars * 4 {
            times.push(t);
            t += 60.0 / (108.0 + 20.0 * i as f64 / (bars * 4) as f64);
    }
    let mut x = vec![0f32; ((t + 1.0) * rate as f64) as usize];
    for (i, &at) in times.iter().enumerate() {
        let at = (at * rate as f64) as usize;
        for j in 0..4000 {
            let (env, sec) = ((-(j as f32) / 300.0).exp(), j as f32 / rate as f32);
            let tone = if i % 4 == 0 { 1600.0 } else { 800.0 };
            x[at + j] += 0.5 * env * (std::f32::consts::TAU * tone * sec).sin();
            if i % 2 == 0 {
                x[at + j] += 0.8 * (-(j as f32) / 900.0).exp() * (std::f32::consts::TAU * (60.0 + 80.0 * (-(j as f32) / 200.0).exp()) * sec).sin();
            }
        }
    }
    let mut writer = hound::WavWriter::create(path, hound::WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float }).unwrap();
    for v in &x {
        writer.write_sample(*v).unwrap();
    }
    writer.finalize().unwrap();
    }

    /// A drummer speeding up, from 108 to 128 BPM over 48 bars: imported into a
    /// 120 BPM project it gets a marker on every bar, and rendered it plays at a
    /// steady 120 BPM with every bar on the project's.
    #[test]
    fn a_varying_tempo_plays_steady_with_the_project() {
        let dir = std::env::temp_dir().join(format!("pulse-warp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("drummer.wav");
        write_drummer(&path, 48);

        let mut app = PulseApp::headless(Project::demo());
        app.project.tempo = 120.0;
        for track in &mut app.project.tracks {
            track.mixer.active = false;
        }
        let track = app.add_audio_track().unwrap();
        app.add_samples(vec![path], Placement::Arrangement { track, start: 8.0 });
        let ctx = eframe::egui::Context::default();
        while app.tempo.busy() {
            app.follow_tempo(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let Clip::Audio(audio) = &app.project.tracks[track].arrangement[0].clip else { panic!() };
        assert!(audio.warp_markers.len() >= 40, "{} markers ({:?})", audio.warp_markers.len(), app.status);

        let out = 48_000.0;
        let mut renderer = engine::OfflineRenderer::new(out);
        let mut samples = crate::sync::SampleCache::default();
        for command in crate::sync::project_commands(&app.project, &mut samples, out, &Default::default()) {
            renderer.send(command);
        }
        renderer.send(engine::Command::SetPosition(8.0));
        renderer.send(engine::Command::Play);
        let (mut l, mut r) = (Vec::new(), Vec::new());
        renderer.render((audio.length * 0.5 * out as f64) as usize, &mut l, &mut r);
        std::fs::remove_dir_all(&dir).ok();
        let (beats, downbeats) = beats::detect(&l, &r, out).unwrap();
        let grid = beats::fit(&beats, &downbeats).expect("beats in the render");
        assert!((grid.bpm - 120.0).abs() < 0.3, "{grid:?}");
        assert!(grid.markers.is_empty(), "steady now");
        println!("{grid:?}");
        // Every beat from the music's first on the project's beats (0.5 s apart from
        // the clip's start), give or take the detector's 20 ms steps. (Before it, in
        // the silence of the bar before the file, the detector makes some up.)
        let worst = beats.iter().filter(|&&b| b > grid.downbeat - 0.1).map(|&b| ((b / 0.5) - (b / 0.5).round()).abs() * 0.5).fold(0.0, f64::max);
        println!("the furthest beat {:.1} ms off", worst * 1000.0);
        assert!(worst < 0.03, "a beat {worst} s off the grid");
        let off = grid.downbeat / 0.5;
        assert!((off - off.round()).abs() < 0.08 && (off.round() as i64) % 4 == 0, "downbeat {} s", grid.downbeat);
    }

    /// Files imported together sit one after the other; when the analysis changes
    /// one's length the next ones move with it.
    #[test]
    fn clips_imported_together_stay_end_to_end() {
        use engine::model::ArrangementClip;
        let clip = |start: f64, length: f64| ArrangementClip { start, length, clip: Clip::Midi(engine::model::MidiClip::new("x", length, [0; 3])) };
        // Three in a row from beat 0, and one the user put further on.
        let mut lane = vec![clip(0.0, 8.0), clip(8.0, 4.0), clip(12.0, 4.0), clip(40.0, 4.0)];
        lane[0].length = 10.0;
        follow_end(&mut lane, 0, 8.0, 2.0);
        let starts: Vec<f64> = lane.iter().map(|c| c.start).collect();
        assert_eq!(starts, vec![0.0, 10.0, 14.0, 40.0]);
        // Shorter: they come back.
        lane[1].length = 2.0;
        follow_end(&mut lane, 1, 14.0, -2.0);
        let starts: Vec<f64> = lane.iter().map(|c| c.start).collect();
        assert_eq!(starts, vec![0.0, 10.0, 12.0, 40.0]);
    }

    /// The real thing: two songs imported at once onto the arrangement end up end to end.
    #[test]
    fn two_songs_imported_at_once_end_to_end() {
        // Files whose length changes once their tempo is found (the import's first
        // guess, by their duration, isn't whole bars of it).
        let dir = std::env::temp_dir().join(format!("pulse-two-songs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (song, other) = (dir.join("una.wav"), dir.join("otra.wav"));
        write_drummer(&song, 12);
        write_drummer(&other, 9);
        let mut app = PulseApp::headless(Project::demo());
        let track = app.add_audio_track().unwrap();
        app.add_samples(vec![song, other], Placement::Arrangement { track, start: 0.0 });
        let ctx = eframe::egui::Context::default();
        while app.tempo.busy() {
            app.follow_tempo(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::fs::remove_dir_all(&dir).ok();
        let lane = &app.project.tracks[track].arrangement;
        assert_eq!(lane.len(), 2);
        assert!(lane.iter().all(|c| matches!(&c.clip, Clip::Audio(a) if a.warp == WarpMode::Stretch)), "both analysed");
        assert_eq!(lane[0].start, 0.0);
        assert!((lane[1].start - lane[0].end()).abs() < 1e-9, "{} vs {}", lane[1].start, lane[0].end());
    }

    /// A loop's name beats a detector gone wrong: tops without a kick, found at
    /// 245 BPM (from the 2026-10b audit), are at the 150 of their name; one the
    /// detector found nothing in too. Without a tempo in the name, the detector's.
    #[test]
    fn a_loops_name_beats_an_absurd_detected_tempo() {
        let found = |bpm: f64| Some(Grid { bpm, downbeat: 0.37, beats: 24, markers: Vec::new() });
        let six_four = 4.0 * 4.0 * 60.0 / 150.0;
        assert_eq!(loop_grid(found(245.64), "Tops_150", six_four, 147.0).map(|g| (g.bpm, g.downbeat)), Some((150.0, 0.0)));
        assert_eq!(loop_grid(None, "Tops_150", six_four / 2.0, 147.0).map(|g| g.bpm), Some(150.0));
        assert_eq!(loop_grid(found(146.9), "Tops", 4.0 * 4.0 * 60.0 / 145.0, 147.0).map(|g| g.bpm), Some(145.0), "fitted to whole bars");
        assert_eq!(loop_grid(None, "Tops", six_four, 147.0), None);
    }

    /// The whole way: a loop of tops at 150 without a kick, imported into a
    /// project at 147, follows it from 150.
    #[test]
    fn imported_tops_named_150_come_in_at_150() {
        let dir = std::env::temp_dir().join(format!("pulse-tops-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Tops_150.wav");
        // Four bars of sixteenths of noise, louder on the off-beat (no kick).
        let rate = 48_000.0f32;
        let sixteenth = 60.0 / 150.0 / 4.0;
        let mut seed = 7u64;
        let tops: Vec<f32> = (0..(64.0 * sixteenth * rate as f64) as usize)
            .map(|i| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let noise = ((seed >> 33) as f32 / (1u64 << 31) as f32) - 1.0;
                let t = i as f64 / rate as f64;
                let step = (t / sixteenth) as usize;
                let (level, decay) = if step % 4 == 2 { (0.9, 18.0) } else { (0.35, 70.0) };
                noise * level * (-(t - step as f64 * sixteenth) * decay).exp() as f32
            })
            .collect();
        crate::capture::write_wav(&path, &tops, &tops, rate).unwrap();
        let mut project = Project::demo();
        project.tempo = 147.0;
        let mut app = PulseApp::headless(project);
        let track = app.add_audio_track().unwrap();
        app.add_samples(vec![path.clone()], Placement::Arrangement { track, start: 0.0 });
        let ctx = eframe::egui::Context::default();
        while app.tempo.busy() {
            app.follow_tempo(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(app.file_bpm(&path.to_string_lossy()), Some(150.0), "for slicing too");
        std::fs::remove_dir_all(&dir).ok();
        let Clip::Audio(clip) = &app.project.tracks[track].arrangement[0].clip else { panic!() };
        assert_eq!((clip.original_bpm, clip.length), (150.0, 16.0), "{:?}", app.status);
        assert!(app.status.as_deref().is_some_and(|s| s.contains("150")), "{:?}", app.status);
    }

    /// A drum loop comes in in Beats (its hits clean when the tempo differs);
    /// a chord loop in Stretch.
    #[test]
    fn a_loops_name_can_tell_its_tempo() {
        assert_eq!(name_bpm("Tops 150", 6.4), Some(150.0));
        assert_eq!(name_bpm("Bass_128bpm_Am", 7.5), Some(128.0));
        assert_eq!(name_bpm("Kick 909 v2", 6.4), None, "909 isn't a tempo");
        assert_eq!(name_bpm("Loop 140", 6.4), None, "6.4 s aren't whole bars at 140");
        assert_eq!(name_bpm("Groove 122.5", 7.836734), Some(122.5));
    }

    #[test]
    fn drum_loops_come_in_in_beats_mode() {
        let dir = std::env::temp_dir().join(format!("pulse-beats-mode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rate = 48_000.0;
        // Four bars at 150: a hit on every sixteenth, an accent on the beats.
        let sixteenth = 60.0 / 150.0 / 4.0;
        let frames = (64.0 * sixteenth * rate as f64) as usize;
        let mut seed = 7u64;
        let mut noise = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f32 / (1u64 << 31) as f32) - 1.0
        };
        let tops: Vec<f32> = (0..frames)
            .map(|i| {
                let t = i as f64 / rate as f64;
                let step = (t / sixteenth) as usize;
                let into = t - step as f64 * sixteenth;
                let level = if step % 4 == 0 { 0.9 } else { 0.4 };
                noise() * level * (-into * 60.0).exp() as f32
            })
            .collect();
        let chords: Vec<f32> = (0..frames).map(|i| [220.0f32, 261.63, 329.63].iter().map(|f| (i as f32 / rate * f * std::f32::consts::TAU).sin()).sum::<f32>() * 0.2).collect();
        let (hats, pad) = (dir.join("Tops_150.wav"), dir.join("Pad_150_Am.wav"));
        crate::capture::write_wav(&hats, &tops, &tops, rate).unwrap();
        crate::capture::write_wav(&pad, &chords, &chords, rate).unwrap();
        let mut app = PulseApp::headless(Project::demo());
        app.project.tempo = 140.0;
        app.view = crate::app::View::Arrangement;
        let track = app.add_audio_track().unwrap();
        app.add_samples(vec![hats, pad], Placement::Arrangement { track, start: 0.0 });
        let ctx = eframe::egui::Context::default();
        let started = std::time::Instant::now();
        while app.tempo.busy() {
            app.follow_tempo(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(20));
            assert!(started.elapsed().as_secs() < 60);
        }
        let warp = |i: usize| match &app.project.tracks[track].arrangement[i].clip {
            Clip::Audio(a) => (a.warp, a.original_bpm),
            _ => panic!(),
        };
        assert_eq!(warp(0), (WarpMode::Beats, 150.0), "{:?}", app.status);
        assert_eq!(warp(1).0, WarpMode::Stretch);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
