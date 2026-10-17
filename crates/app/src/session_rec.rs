//! Recording the session into the arrangement, as in Live: while recording onto
//! the arrangement, the session clips that play (launched one by one or with
//! scenes) are written onto their tracks' arrangement where they played, from
//! the beat they started (or recording did) to the one they stopped.

use crate::app::{ClipRef, PulseApp};
use crate::clip_split::{split, MIN_PART};
use engine::model::{ArrangementClip, Clip};
use engine::{SharedState, NONE};
use std::collections::HashMap;
use std::sync::atomic::Ordering;

/// A session clip playing while recording.
struct Playing {
    slot: usize,
    clip: Clip,
    /// Where its beat 0 falls on the transport (it loops from there).
    launch: f64,
    /// Where recording it began.
    from: f64,
}

/// What played on a track from `from` to `to`.
struct Played {
    track: usize,
    clip: Clip,
    launch: f64,
    from: f64,
    to: f64,
}

#[derive(Default)]
pub struct SessionCapture {
    playing: HashMap<usize, Playing>,
    played: Vec<Played>,
    /// Nothing before this is recorded: where recording started, or the loop's
    /// start once it has jumped back.
    floor: f64,
    last_position: f64,
    loop_end: f64,
}

impl SessionCapture {
    fn close(&mut self, track: usize, to: f64) {
        if let Some(p) = self.playing.remove(&track) {
            if to - p.from > MIN_PART {
                self.played.push(Played { track, clip: p.clip, launch: p.launch, from: p.from, to });
            }
        }
    }
}

/// Makes room on `lane` from `from` to `to`: clips there are cut at its edges
/// and what was inside goes (the session recorded over it, as in Live).
fn clear(lane: &mut Vec<ArrangementClip>, from: f64, to: f64, tempo: f64) {
    let mut kept = Vec::new();
    for clip in lane.drain(..) {
        if clip.end() <= from + 1e-9 || clip.start >= to - 1e-9 {
            kept.push(clip);
            continue;
        }
        if let Some((before, _)) = split(&clip, from, tempo) {
            kept.push(before);
        }
        if let Some((_, after)) = split(&clip, to, tempo) {
            kept.push(after);
        }
    }
    *lane = kept;
}

/// The part of a session clip launched at `launch` that played from `from` to
/// `to`, as an arrangement clip: it goes on where the clip was (its loop too).
fn placed(clip: &Clip, launch: f64, from: f64, to: f64, tempo: f64) -> Option<ArrangementClip> {
    let whole = ArrangementClip { start: launch, length: to - launch, clip: clip.clone() };
    let mut part = if from - launch > MIN_PART { split(&whole, from, tempo)?.1 } else { whole };
    part.length = to - part.start;
    (part.length > MIN_PART).then_some(part)
}

impl PulseApp {
    /// Recording onto the arrangement starts at `position`.
    pub fn start_session_capture(&mut self, position: f64) {
        self.session_capture = Some(SessionCapture { floor: position, last_position: position, loop_end: self.project.loop_end, ..Default::default() });
    }

    /// Once a frame while recording: follows which session clip plays on each track.
    pub fn follow_session_capture(&mut self, state: &SharedState) {
        let Some(mut capture) = self.session_capture.take() else { return };
        let position = state.position();
        // The loop jumped back: what played up to its end is done, and the clips
        // go on from its start.
        if position + 1e-6 < capture.last_position {
            let tracks: Vec<usize> = capture.playing.keys().copied().collect();
            for track in tracks {
                capture.close(track, capture.loop_end);
            }
            capture.floor = self.project.loop_start;
        }
        capture.loop_end = self.project.loop_end;
        for (track, t) in self.project.tracks.iter().enumerate() {
            let status = &state.tracks[track];
            let slot = status.playing.load(Ordering::Relaxed);
            let launch = status.launch_beat();
            let same = capture.playing.get(&track).is_some_and(|p| slot == p.slot as i32 && (launch - p.launch).abs() < 1e-6);
            if same {
                continue;
            }
            // Stopped, or another clip (or the same one again) launched: it ended at the switch.
            if capture.playing.contains_key(&track) {
                capture.close(track, status.switch_beat().min(position).max(capture.floor));
            }
            if slot != NONE {
                if let Some(clip) = t.clips.get(slot as usize).and_then(|c| c.clone()) {
                    capture.playing.insert(track, Playing { slot: slot as usize, clip, launch, from: launch.max(capture.floor) });
                }
            }
        }
        capture.last_position = position;
        self.session_capture = Some(capture);
    }

    /// Recording ended: the session clips that played go onto the arrangement
    /// (within the punch, if it is on). Returns how many.
    pub fn finish_session_capture(&mut self) -> usize {
        let Some(mut capture) = self.session_capture.take() else { return 0 };
        let end = capture.last_position;
        let tracks: Vec<usize> = capture.playing.keys().copied().collect();
        for track in tracks {
            capture.close(track, end.max(capture.floor));
        }
        let (punch_in, punch_out) = self.project.punch();
        let tempo = self.project.tempo;
        let mut touched = Vec::new();
        let mut last = None;
        for p in capture.played {
            let from = punch_in.map_or(p.from, |i| p.from.max(i));
            let to = punch_out.map_or(p.to, |o| p.to.min(o));
            let Some(clip) = (to > from).then(|| placed(&p.clip, p.launch, from, to, tempo)).flatten() else { continue };
            let Some(lane) = self.project.tracks.get_mut(p.track).map(|t| &mut t.arrangement) else { continue };
            // Looping, every pass plays the same: once is enough.
            if lane.contains(&clip) {
                continue;
            }
            clear(lane, clip.start, clip.end(), tempo);
            lane.push(clip);
            last = Some(ClipRef::Arrangement { track: p.track, index: lane.len() - 1 });
            touched.push(p.track);
        }
        let count = touched.len();
        touched.sort_unstable();
        touched.dedup();
        for track in touched {
            self.sync_arrangement(track);
        }
        if let Some(clip) = last {
            self.select_clip(clip);
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::model::{MidiClip, Note};

    fn clip() -> Clip {
        let mut midi = MidiClip::new("Riff", 4.0, [0; 3]);
        midi.notes = vec![Note { start: 0.0, length: 1.0, pitch: 60, velocity: 1.0 }, Note { start: 2.0, length: 1.0, pitch: 64, velocity: 1.0 }];
        Clip::Midi(midi)
    }

    #[test]
    fn recording_over_the_arrangement_replaces_that_stretch() {
        let mut lane = vec![
            ArrangementClip { start: 0.0, length: 8.0, clip: clip() },
            ArrangementClip { start: 10.0, length: 2.0, clip: clip() },
            ArrangementClip { start: 12.0, length: 8.0, clip: clip() },
        ];
        clear(&mut lane, 6.0, 14.0, 120.0);
        assert_eq!(lane.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>(), vec![(0.0, 6.0), (14.0, 6.0)]);
    }

    #[test]
    fn a_session_clip_goes_on_where_it_was() {
        // Launched at 8, recorded from 10 to 20: it starts half way through its loop.
        let part = placed(&clip(), 8.0, 10.0, 20.0, 120.0).unwrap();
        assert_eq!((part.start, part.length), (10.0, 10.0));
        let Clip::Midi(midi) = &part.clip else { panic!() };
        let mut notes: Vec<(f64, u8)> = midi.notes.iter().map(|n| (n.start, n.pitch)).collect();
        notes.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(notes, vec![(0.0, 64), (2.0, 60)]);
        // From its launch: as it is.
        let whole = placed(&clip(), 8.0, 8.0, 16.0, 120.0).unwrap();
        assert_eq!((whole.start, whole.length, whole.clip), (8.0, 8.0, clip()));
    }

    /// On a real engine: clips launched while recording land on the arrangement
    /// where they played, a clip already playing from where recording began.
    #[test]
    fn launches_while_recording_are_written_onto_the_arrangement() {
        use crate::midi_map::Transport;
        use engine::Command;
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let (audio, mut engine) = crate::app::Audio::detached();
        app.audio = audio;
        for track in &mut app.project.tracks {
            track.armed = false;
            track.arrangement.clear();
        }
        app.project.tempo = 120.0;
        app.project.quantize = 4.0;
        let riff = clip();
        app.project.tracks[0].clips[0] = Some(riff.clone());
        app.project.tracks[0].clips[1] = Some(Clip::Midi(MidiClip::new("Otro", 2.0, [0; 3])));
        app.project.tracks[1].clips[0] = Some(riff.clone());
        app.sync_all();
        let state = app.audio.state().unwrap();
        let mut run = |app: &mut PulseApp, beats: f64| {
            let mut buffer = vec![0.0; 1024];
            for _ in 0..((beats * 0.5 * 48_000.0) / 512.0).round() as usize {
                engine.process(&mut buffer, 2);
                app.update_recording();
            }
        };
        // Track 1 plays from before recording (launched at 0).
        app.audio.send(Command::LaunchClip { track: 1, slot: 0 });
        run(&mut app, 2.0);
        assert_eq!(state.tracks[1].playing.load(Ordering::Relaxed), 0);
        app.transport(Transport::Record);
        assert!(app.project.tracks.iter().all(|t| !t.armed), "nothing to arm: the session is what is recorded");
        app.audio.send(Command::LaunchClip { track: 0, slot: 0 });
        run(&mut app, 4.0); // launched at 4
        app.audio.send(Command::LaunchClip { track: 0, slot: 1 });
        run(&mut app, 4.0); // switched at 8
        app.audio.send(Command::StopTrack(0));
        run(&mut app, 4.0); // stopped at 12
        app.transport(Transport::Record);
        run(&mut app, 0.5);
        assert!(!app.is_arrangement_recording());

        let lane = |t: usize| app.project.tracks[t].arrangement.iter().map(|c| (c.start, c.length, c.clip.name().to_string())).collect::<Vec<_>>();
        assert_eq!(lane(0), vec![(4.0, 4.0, "Riff".to_string()), (8.0, 4.0, "Otro".to_string())]);
        // Track 1, from where recording began (beat 2, half its loop in) to where it ended.
        let t1 = &app.project.tracks[1].arrangement;
        assert_eq!(t1.len(), 1);
        assert!((t1[0].start - 2.0).abs() < 0.05 && (t1[0].end() - 14.0).abs() < 0.1, "{} – {}", t1[0].start, t1[0].end());
        assert!(app.status.as_deref().unwrap_or("").contains("clips de Session"), "{:?}", app.status);
    }
}
