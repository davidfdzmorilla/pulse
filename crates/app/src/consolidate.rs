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

/// How long `clip` (`audio`) plays, in beats, and whether it plays once in the
/// Arrangement (then it may cross-fade with a neighbour: rendered with handles).
fn placed_length(app: &PulseApp, clip: ClipRef, audio: &AudioClip) -> (f64, bool) {
    match clip {
        ClipRef::Session { .. } => (audio.length, false),
        ClipRef::Arrangement { track, index } => {
            let length = app.project.tracks[track].arrangement[index].length;
            (length, length <= audio.length + 1e-9)
        }
    }
}

impl PulseApp {
    /// Writes `left`/`right` as a new file for a clip named `name`, with the
    /// project's media.
    fn write_consolidated(&self, name: &str, left: &[f32], right: &[f32]) -> Result<PathBuf, String> {
        let dir = self.media_dir(crate::library::CONSOLIDATED);
        std::fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
        let path = crate::library::unique(&dir.join(format!("{} (consolidado).wav", crate::files::safe_name(name))));
        crate::export::write(&path, left, right, self.audio.sample_rate() as u32, crate::export::BitDepth::Float32, 1.0)?;
        Ok(path)
    }

    /// `audio` as heard over `beats`, rendered into a new file, and the clip that
    /// plays it. `handles`: with a little more of it before and after, for an
    /// arrangement clip that cross-fades with its neighbours (see
    /// `engine::render_audio_handles`).
    fn consolidated(&mut self, audio: &AudioClip, beats: f64, handles: bool) -> Result<AudioClip, String> {
        let sample = self.sample(&audio.sample);
        if sample.frames() == 0 {
            return Err(format!("No se encuentra el audio de «{}»", audio.name));
        }
        let tempo = self.project.tempo;
        // What it sounds when on: a deactivated clip stays deactivated, with its sound.
        let heard = AudioClip { muted: false, ..audio.clone() };
        let render = if handles { engine::render_audio_handles } else { engine::render_audio };
        let (left, right) = render(&heard, &sample, beats, tempo, self.audio.sample_rate());
        let path = self.write_consolidated(&audio.name, &left, &right)?;
        Ok(AudioClip {
            sample: path.to_string_lossy().into_owned(),
            offset: if handles { engine::HANDLE_SECONDS } else { 0.0 },
            length: beats,
            // Rendered at the project's tempo: that is its own now.
            warp: audio.warp,
            original_bpm: tempo,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: laid_out(&audio.envelopes, audio.length.max(1e-6), beats),
            // Its fades are in the file now.
            fades: engine::model::Fades::NONE,
            ..audio.clone()
        })
    }

    /// Consolidates each of `clips` that is audio, in place.
    pub fn consolidate(&mut self, clips: Vec<ClipRef>) {
        let mut done = 0;
        for clip in clips {
            let Some(Clip::Audio(audio)) = self.clip(clip).cloned() else { continue };
            let (beats, handles) = placed_length(self, clip, &audio);
            match self.consolidated(&audio, beats, handles) {
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

    /// Turns each of `clips` that is audio round: what it sounds, from its end
    /// to its start, in a new file (its envelopes turned round too). Again,
    /// and it plays forwards.
    pub fn reverse_clips(&mut self, clips: Vec<ClipRef>) {
        let mut done = 0;
        for clip in clips {
            let Some(Clip::Audio(audio)) = self.clip(clip).cloned() else { continue };
            let (beats, handles) = placed_length(self, clip, &audio);
            let sample = self.sample(&audio.sample);
            if sample.frames() == 0 {
                self.status = Some(format!("No se encuentra el audio de «{}»", audio.name));
                return;
            }
            let tempo = self.project.tempo;
            // What it sounds when on: a deactivated clip stays deactivated, with its sound.
            let heard = AudioClip { muted: false, ..audio.clone() };
            let render = if handles { engine::render_audio_handles } else { engine::render_audio };
            let (mut left, mut right) = render(&heard, &sample, beats, tempo, self.audio.sample_rate());
            // Turned round, the handles swap ends: still as long each.
            left.reverse();
            right.reverse();
            let name = match audio.name.strip_suffix(" (invertido)") {
                Some(forwards) => forwards.to_string(),
                None => format!("{} (invertido)", audio.name),
            };
            let path = match self.write_consolidated(&name, &left, &right) {
                Ok(path) => path,
                Err(error) => {
                    self.status = Some(error);
                    return;
                }
            };
            let mut envelopes = laid_out(&audio.envelopes, audio.length.max(1e-6), beats);
            for lane in &mut envelopes {
                for point in &mut lane.points {
                    point.beat = beats - point.beat;
                }
                lane.points.reverse();
            }
            let new = AudioClip {
                name,
                sample: path.to_string_lossy().into_owned(),
                offset: if handles { engine::HANDLE_SECONDS } else { 0.0 },
                length: beats,
                original_bpm: tempo,
                warp_markers: Vec::new(),
                takes: Vec::new(),
                gain_db: 0.0,
                transpose: 0.0,
                envelopes,
                // Its fades are in the file now.
                fades: engine::model::Fades::NONE,
                ..audio.clone()
            };
            match clip {
                ClipRef::Session { track, slot } => self.set_clip(track, slot, Some(Clip::Audio(new))),
                ClipRef::Arrangement { track, index } => {
                    self.project.tracks[track].arrangement[index].clip = Clip::Audio(new);
                    self.sync_arrangement(track);
                }
            }
            done += 1;
        }
        self.status = Some(match done {
            0 => "Invertir es para clips de audio".into(),
            1 => "Clip invertido: suena del final al principio, desde un archivo nuevo".into(),
            n => format!("{n} clips invertidos"),
        });
    }

    /// Audio clips of one track rendered together into one file, from the first's
    /// start to the last's end (gaps silent); the clip that plays it.
    pub(crate) fn join_audio(&mut self, track: usize, parts: &[ArrangementClip]) -> Result<ArrangementClip, String> {
        let start = parts.iter().map(|p| p.start).fold(f64::INFINITY, f64::min);
        let end = parts.iter().map(|p| p.end()).fold(f64::NEG_INFINITY, f64::max);
        let first = parts.iter().min_by(|a, b| a.start.total_cmp(&b.start)).ok_or("Nada que unir")?;
        let Clip::Audio(base) = &first.clip else { return Err("Solo se unen clips del mismo tipo".into()) };
        let (tempo, rate) = (self.project.tempo, self.audio.sample_rate());
        let frames = ((end - start) * 60.0 / tempo * rate as f64).round() as usize;
        let (mut left, mut right) = (vec![0f32; frames], vec![0f32; frames]);
        // What was heard: the deactivated parts left out, unless all of them
        // are (then the whole, still deactivated).
        let all_off = parts.iter().all(|p| p.clip.muted());
        let mut placed = Vec::new();
        for part in parts {
            let Clip::Audio(audio) = &part.clip else { return Err("Solo se unen clips del mismo tipo".into()) };
            let sample = self.sample(&audio.sample);
            let audio = AudioClip { muted: audio.muted && !all_off, ..audio.clone() };
            placed.push(engine::PlacedClip::new(part.start, part.length, engine::PlayClip::Audio(audio, sample)));
        }
        let heard: Vec<ArrangementClip> = parts.iter().filter(|p| all_off || !p.clip.muted()).cloned().collect();
        // Pieces that follow on (of a split clip, or of takes) cross-fade as they play.
        engine::link_neighbours(&mut placed);
        for part in &placed {
            let (l, r) = engine::render_placed(part, start, frames, tempo, rate);
            for (i, (a, b)) in l.iter().zip(&r).enumerate() {
                left[i] += a;
                right[i] += b;
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
            key: None,
            takes: Vec::new(),
            gain_db: 0.0,
            transpose: 0.0,
            // The pieces' envelopes still move the mixer and effects, along it.
            envelopes: crate::clip_split::join_envelopes(&heard, start, &|p| self.envelope_base(track, p)),
            // Its fades are in the file now.
            fades: engine::model::Fades::NONE,
            muted: all_off,
            ..base.clone()
        };
        Ok(ArrangementClip { start, length: end - start, clip: Clip::Audio(clip) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Consolidating or reversing a deactivated clip keeps its sound in the
    /// new file (it used to render silence): switched back on, it sounds.
    #[test]
    fn a_deactivated_clip_consolidated_or_reversed_keeps_its_sound() {
        for reverse in [false, true] {
            let mut clip = demo_clip(WarpMode::Repitch, 8.0, 3.0);
            clip.muted = true;
            let (mut app, track) = app_with(vec![ArrangementClip { start: 0.0, length: 8.0, clip: Clip::Audio(clip) }]);
            let refs = vec![ClipRef::Arrangement { track, index: 0 }];
            if reverse { app.reverse_clips(refs) } else { app.consolidate(refs) }
            let Clip::Audio(new) = &mut app.project.tracks[track].arrangement[0].clip else { panic!() };
            assert!(new.muted, "still deactivated");
            let path = new.sample.clone();
            new.muted = false;
            let loudest = render(&app, 8.0).iter().map(|s| s.abs()).fold(0f32, f32::max);
            std::fs::remove_file(&path).ok();
            assert!(loudest > 0.05, "reverse {reverse}: switched back on it is silent ({loudest})");
        }
    }
    use crate::project::Project;

    fn demo_clip(warp: WarpMode, length: f64, offset: f64) -> AudioClip {
        let song = crate::test_audio::demo_song();
        AudioClip { hard_start: false, fades: Default::default(),
            name: "demo".into(),
            color: [0; 3],
            sample: song.to_string_lossy().into_owned(),
            length,
            offset,
            warp,
            original_bpm: 120.0,
            key: None,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            gain_db: -6.0,
            transpose: 0.0,
            envelopes: Vec::new(), muted: false,
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
        assert_eq!((new.offset, new.length, new.gain_db, new.original_bpm), (engine::HANDLE_SECONDS, 16.0, 0.0, 100.0));
        // The file is just the part, 16 beats at 100 BPM, and a handle at each
        // end (for cross-fading with a neighbour: it plays from after the first).
        let file = engine::Sample::load(std::path::Path::new(&new.sample)).unwrap();
        assert!((file.duration() - 9.6 - 2.0 * engine::HANDLE_SECONDS).abs() < 0.001, "{}", file.duration());
        let after = render(&app, 16.0);
        // The same but for the few ms of the edges' fades (twice now: rendered in, played back).
        let edge = 48_000 / 50;
        let worst = before.iter().zip(&after).enumerate().filter(|(i, _)| *i > edge && *i + edge < before.len()).map(|(_, (a, b))| (a - b).abs()).fold(0f32, f32::max);
        assert!(before.iter().map(|s| s.abs()).fold(0f32, f32::max) > 0.05);
        assert!(worst < 1e-3, "{worst}");
        std::fs::remove_file(&new.sample).ok();
    }

    /// Its fades go into the consolidated file, once: the new clip has none
    /// and sounds as the old one did.
    #[test]
    fn a_consolidated_clip_keeps_its_fades_once() {
        let mut clip = demo_clip(WarpMode::Repitch, 8.0, 3.0);
        clip.fades = engine::model::Fades::new(2.0, 1.0);
        let (mut app, track) = app_with(vec![ArrangementClip { start: 0.0, length: 8.0, clip: Clip::Audio(clip) }]);
        let before = render(&app, 8.0);
        app.consolidate(vec![ClipRef::Arrangement { track, index: 0 }]);
        let Clip::Audio(new) = app.project.tracks[track].arrangement[0].clip.clone() else { panic!() };
        assert_eq!(new.fades, engine::model::Fades::NONE);
        let after = render(&app, 8.0);
        let edge = 48_000 / 50;
        let worst = before.iter().zip(&after).enumerate().filter(|(i, _)| *i > edge && *i + edge < before.len()).map(|(_, (a, b))| (a - b).abs()).fold(0f32, f32::max);
        assert!(worst < 1e-3, "{worst}");
        // It starts silent and swells: the fade-in is in the file.
        let quarter = |from: usize| before[from..from + 2_000].iter().map(|s| s.abs()).fold(0f32, f32::max);
        assert!(quarter(edge) < quarter(48_000) * 0.5);
        std::fs::remove_file(&new.sample).ok();
    }

    /// Reversed, a clip plays what it sounded from its end to its start; reversed
    /// again, forwards as it did, with its name back.
    #[test]
    fn a_reversed_clip_plays_backwards_and_back_again() {
        let clip = |offset| ArrangementClip { start: 0.0, length: 8.0, clip: Clip::Audio(demo_clip(WarpMode::Repitch, 8.0, offset)) };
        let (mut app, track) = app_with(vec![clip(3.0), clip(3.0)]);
        app.project.tracks[track].arrangement[1].start = 8.0;
        let mut lane = AutomationLane::new(engine::model::Param::Volume);
        lane.points.push(engine::model::AutomationPoint { beat: 1.0, value: 0.2, curve: 0.0 });
        if let Clip::Audio(a) = &mut app.project.tracks[track].arrangement[0].clip {
            a.envelopes.push(lane);
        }
        app.consolidate(vec![ClipRef::Arrangement { track, index: 1 }]);
        app.reverse_clips(vec![ClipRef::Arrangement { track, index: 0 }]);
        let lane = app.project.tracks[track].arrangement.clone();
        let (Clip::Audio(forwards), Clip::Audio(backwards)) = (&lane[1].clip, &lane[0].clip) else { panic!() };
        assert_eq!(backwards.name, "demo (invertido)");
        assert_eq!(backwards.envelopes[0].points[0].beat, 7.0, "the envelope turned round too");
        let load = |path: &str| engine::Sample::load(std::path::Path::new(path)).unwrap();
        let (a, b) = (load(&forwards.sample), load(&backwards.sample));
        let mut turned = b.left.clone();
        turned.reverse();
        assert!(a.left.iter().map(|s| s.abs()).fold(0f32, f32::max) > 0.05);
        assert_eq!(turned, a.left);

        let first = backwards.sample.clone();
        app.reverse_clips(vec![ClipRef::Arrangement { track, index: 0 }]);
        let Clip::Audio(again) = &app.project.tracks[track].arrangement[0].clip else { panic!() };
        assert_eq!(again.name, "demo");
        assert_eq!(again.envelopes[0].points[0].beat, 1.0);
        // The same but for the few ms of the edges' fades (rendered once more).
        let edge = 48_000 / 50;
        let back = load(&again.sample).left;
        assert_eq!(back.len(), a.left.len());
        let worst = back.iter().zip(&a.left).enumerate().filter(|(i, _)| *i > edge && *i + edge < back.len()).map(|(_, (x, y))| (x - y).abs()).fold(0f32, f32::max);
        assert!(worst < 1e-6, "{worst}");
        for file in [&forwards.sample, &first, &again.sample.clone()] {
            std::fs::remove_file(file).ok();
        }
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

    /// Pieces of a split clip (or of takes) joined back cross-fade at the cut,
    /// as they played: the file sounds like the clip did before it was split.
    #[test]
    fn joined_pieces_keep_their_cross_fade() {
        let whole = ArrangementClip { start: 0.0, length: 8.0, clip: Clip::Audio(demo_clip(WarpMode::Repitch, 8.0, 2.0)) };
        let (mut app, track) = app_with(vec![whole]);
        let before = render(&app, 8.0);
        assert!(app.split_clip_at(track, 0, 4.5));
        app.join_selected();
        let lane = &app.project.tracks[track].arrangement;
        assert_eq!(lane.len(), 1, "{:?}", app.status);
        let after = render(&app, 8.0);
        let edge = 48_000 / 50;
        let worst = before.iter().zip(&after).enumerate().filter(|(i, _)| *i > edge && *i + edge < before.len()).map(|(_, (a, b))| (a - b).abs()).fold(0f32, f32::max);
        assert!(worst < 1e-3, "also at the cut (beat 4.5): {worst}");
        let Clip::Audio(new) = &lane[0].clip else { panic!() };
        std::fs::remove_file(&new.sample).ok();
    }
}
