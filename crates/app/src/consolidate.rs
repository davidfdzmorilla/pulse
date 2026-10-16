//! Consolidating: an audio clip, or several in a row, rendered as they sound into
//! a file of their own (like Live's Consolidate or Logic's Bounce in Place).
//!
//! Splitting and moving clips never touches files: each clip plays a part of one
//! (non-destructive editing, as in every DAW). Consolidating makes the part its
//! own audio: the new file holds just what the clip plays, with its warp,
//! transposition and gain applied, so the clip then starts at 0 of it, at the
//! project's tempo (it keeps following the tempo if that changes later).

use crate::app::{ClipRef, PulseApp};
use engine::model::{ArrangementClip, AudioClip, AutomationLane, Clip, WarpMode};
use std::path::PathBuf;

/// Clip envelopes laid out over `beats` of a clip whose loop is `length` long.
fn laid_out(lanes: &[AutomationLane], length: f64, beats: f64) -> Vec<AutomationLane> {
    if beats <= length + 1e-9 {
        return lanes.to_vec();
    }
    lanes
        .iter()
        .map(|lane| {
            let mut points = Vec::new();
            let mut lap = 0.0;
            while lap < beats {
                points.extend(lane.points.iter().filter(|p| p.beat < length && lap + p.beat < beats).map(|p| engine::model::AutomationPoint { beat: lap + p.beat, ..*p }));
                lap += length;
            }
            AutomationLane { param: lane.param, points }
        })
        .collect()
}

impl PulseApp {
    /// Writes `left`/`right` as a new file for a clip named `name`, with the
    /// project's media.
    fn write_consolidated(&self, name: &str, left: &[f32], right: &[f32]) -> Result<PathBuf, String> {
        let dir = self.media_dir(crate::library::CONSOLIDATED);
        std::fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
        let clean: String = name.chars().map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c }).collect();
        let path = crate::library::unique(&dir.join(format!("{} (consolidado).wav", clean.trim())));
        crate::export::write(&path, left, right, self.audio.sample_rate() as u32, crate::export::BitDepth::Float32, 1.0)?;
        Ok(path)
    }

    /// `audio` as heard over `beats`, rendered into a new file, and the clip that
    /// plays it.
    fn consolidated(&mut self, audio: &AudioClip, beats: f64) -> Result<AudioClip, String> {
        let sample = self.sample(&audio.sample);
        if sample.frames() == 0 {
            return Err(format!("No se encuentra el audio de «{}»", audio.name));
        }
        let tempo = self.project.tempo;
        let (left, right) = engine::render_audio(audio, &sample, beats, tempo, self.audio.sample_rate());
        let path = self.write_consolidated(&audio.name, &left, &right)?;
        Ok(AudioClip {
            sample: path.to_string_lossy().into_owned(),
            offset: 0.0,
            length: beats,
            // Rendered at the project's tempo: that is its own now.
            warp: audio.warp,
            original_bpm: tempo,
            warp_markers: Vec::new(),
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: laid_out(&audio.envelopes, audio.length.max(1e-6), beats),
            ..audio.clone()
        })
    }

    /// Consolidates each of `clips` that is audio, in place.
    pub fn consolidate(&mut self, clips: Vec<ClipRef>) {
        let mut done = 0;
        for clip in clips {
            let Some(Clip::Audio(audio)) = self.clip(clip).cloned() else { continue };
            let beats = match clip {
                ClipRef::Session { .. } => audio.length,
                ClipRef::Arrangement { track, index } => self.project.tracks[track].arrangement[index].length,
            };
            match self.consolidated(&audio, beats) {
                Ok(new) => {
                    match clip {
                        ClipRef::Session { track, slot } => self.set_clip(track, slot, Some(Clip::Audio(new))),
                        ClipRef::Arrangement { track, index } => {
                            self.project.tracks[track].arrangement[index].clip = Clip::Audio(new);
                            self.sync_arrangement(track);
                        }
                    }
                    done += 1;
                }
                Err(error) => {
                    self.status = Some(error);
                    return;
                }
            }
        }
        self.status = Some(match done {
            0 => "Consolidar es para clips de audio".into(),
            1 => "Clip consolidado: ahora tiene su propio archivo de audio".into(),
            n => format!("{n} clips consolidados, cada uno con su propio archivo"),
        });
    }

    /// Audio clips of one track rendered together into one file, from the first's
    /// start to the last's end (gaps silent); the clip that plays it.
    pub(crate) fn join_audio(&mut self, parts: &[ArrangementClip]) -> Result<ArrangementClip, String> {
        let start = parts.iter().map(|p| p.start).fold(f64::INFINITY, f64::min);
        let end = parts.iter().map(|p| p.end()).fold(f64::NEG_INFINITY, f64::max);
        let first = parts.iter().min_by(|a, b| a.start.total_cmp(&b.start)).ok_or("Nada que unir")?;
        let Clip::Audio(base) = &first.clip else { return Err("Solo se unen clips del mismo tipo".into()) };
        let (tempo, rate) = (self.project.tempo, self.audio.sample_rate());
        let frames = ((end - start) * 60.0 / tempo * rate as f64).round() as usize;
        let (mut left, mut right) = (vec![0f32; frames], vec![0f32; frames]);
        for part in parts {
            let Clip::Audio(audio) = &part.clip else { return Err("Solo se unen clips del mismo tipo".into()) };
            let sample = self.sample(&audio.sample);
            let (l, r) = engine::render_audio(audio, &sample, part.length, tempo, rate);
            let at = ((part.start - start) * 60.0 / tempo * rate as f64).round() as usize;
            for (i, (a, b)) in l.iter().zip(&r).enumerate() {
                if let (Some(x), Some(y)) = (left.get_mut(at + i), right.get_mut(at + i)) {
                    *x += a;
                    *y += b;
                }
            }
        }
        let path = self.write_consolidated(&base.name, &left, &right)?;
        let warp = if parts.iter().all(|p| matches!(&p.clip, Clip::Audio(a) if a.warp == WarpMode::Off)) { WarpMode::Off } else { WarpMode::Stretch };
        let clip = AudioClip {
            sample: path.to_string_lossy().into_owned(),
            offset: 0.0,
            length: end - start,
            warp,
            original_bpm: tempo,
            warp_markers: Vec::new(),
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(),
            ..base.clone()
        };
        Ok(ArrangementClip { start, length: end - start, clip: Clip::Audio(clip) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    fn demo_clip(warp: WarpMode, length: f64, offset: f64) -> AudioClip {
        let song = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo.wav");
        AudioClip {
            name: "demo".into(),
            color: [0; 3],
            sample: song.to_string_lossy().into_owned(),
            length,
            offset,
            warp,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            gain_db: -6.0,
            transpose: 0.0,
            envelopes: Vec::new(),
        }
    }

    /// Renders the project's arrangement for `beats` from the start (track 0 muted out).
    fn render(app: &PulseApp, beats: f64) -> Vec<f32> {
        let rate = 48_000.0;
        let mut renderer = engine::OfflineRenderer::new(rate);
        let mut samples = crate::sync::SampleCache::default();
        for command in crate::sync::project_commands(&app.project, &mut samples, rate, &Default::default()) {
            renderer.send(command);
        }
        renderer.send(engine::Command::Play);
        let (mut l, mut r) = (Vec::new(), Vec::new());
        renderer.render((beats * 60.0 / app.project.tempo * rate as f64) as usize, &mut l, &mut r);
        l
    }

    fn app_with(clips: Vec<ArrangementClip>) -> (PulseApp, usize) {
        let mut app = PulseApp::headless(Project::empty());
        for track in &mut app.project.tracks {
            track.mixer.active = false;
        }
        let track = app.add_audio_track().unwrap();
        app.project.tracks[track].arrangement = clips;
        (app, track)
    }

    /// Consolidated, a clip sounds as it did, now from a file of its own that holds
    /// just its part.
    #[test]
    fn a_consolidated_clip_sounds_the_same_from_its_own_file() {
        // 4 bars from 3 s into the song, stretched to 100 BPM, 6 dB down.
        let (mut app, track) = app_with(vec![ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Audio(demo_clip(WarpMode::Repitch, 16.0, 3.0)) }]);
        app.project.tempo = 100.0;
        let before = render(&app, 16.0);
        app.consolidate(vec![ClipRef::Arrangement { track, index: 0 }]);
        let Clip::Audio(new) = &app.project.tracks[track].arrangement[0].clip else { panic!() };
        assert!(new.sample.contains("Consolidados"), "{}", new.sample);
        assert_eq!((new.offset, new.length, new.gain_db, new.original_bpm), (0.0, 16.0, 0.0, 100.0));
        // The file is just the part: 16 beats at 100 BPM.
        let file = engine::Sample::load(std::path::Path::new(&new.sample)).unwrap();
        assert!((file.duration() - 9.6).abs() < 0.01, "{}", file.duration());
        let after = render(&app, 16.0);
        // The same but for the few ms of the edges' fades (twice now: rendered in, played back).
        let edge = 48_000 / 50;
        let worst = before.iter().zip(&after).enumerate().filter(|(i, _)| *i > edge && *i + edge < before.len()).map(|(_, (a, b))| (a - b).abs()).fold(0f32, f32::max);
        assert!(before.iter().map(|s| s.abs()).fold(0f32, f32::max) > 0.05);
        assert!(worst < 1e-3, "{worst}");
        std::fs::remove_file(&new.sample).ok();
    }

    /// Audio clips in a row (with a gap) join into one file.
    #[test]
    fn audio_clips_join_into_one_file() {
        let a = ArrangementClip { start: 0.0, length: 4.0, clip: Clip::Audio(demo_clip(WarpMode::Stretch, 4.0, 0.0)) };
        let b = ArrangementClip { start: 8.0, length: 4.0, clip: Clip::Audio(demo_clip(WarpMode::Stretch, 4.0, 6.0)) };
        let (mut app, track) = app_with(vec![a, b]);
        let before = render(&app, 12.0);
        app.set_clip_selection(vec![ClipRef::Arrangement { track, index: 0 }, ClipRef::Arrangement { track, index: 1 }]);
        app.join_selected();
        let lane = &app.project.tracks[track].arrangement;
        assert_eq!(lane.len(), 1, "{:?}", app.status);
        assert_eq!((lane[0].start, lane[0].length), (0.0, 12.0));
        let after = render(&app, 12.0);
        // The gap stays silent.
        let gap = (5.0 * 0.5 * 48_000.0) as usize..(7.5 * 0.5 * 48_000.0) as usize;
        assert!(after[gap].iter().all(|s| s.abs() < 1e-4));
        // The rest sounds about the same (Stretch's grains, rendered twice, differ a little).
        let energy = |x: &[f32]| x.iter().map(|s| s * s).sum::<f32>();
        assert!((energy(&after) / energy(&before) - 1.0).abs() < 0.1, "{} vs {}", energy(&after), energy(&before));
        let Clip::Audio(new) = &lane[0].clip else { panic!() };
        std::fs::remove_file(&new.sample).ok();
    }
}
