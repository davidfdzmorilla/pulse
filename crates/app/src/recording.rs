//! Turns notes captured by the engine into clips.
//!
//! The engine decides when a take starts (quantized) and reports each note on
//! note-off. The UI collects them per take and builds the clip when the engine
//! reports the take has ended, so notes flushed at the very end are included.

use engine::model::Note;
use engine::{RecordedNote, SharedState};
use std::sync::atomic::Ordering;

pub struct SessionTake {
    pub track: usize,
    pub slot: usize,
    /// Set once the engine has started the take.
    pub start: Option<f64>,
    /// Whether the engine has acknowledged the take (queued or recording).
    seen: bool,
    /// Frames waited for acknowledgement; a take the engine never accepts is dropped.
    waited: u32,
    /// Transport position when the user stopped the take.
    pub stop_at: Option<f64>,
    notes: Vec<RecordedNote>,
}

pub struct ArrangementTake {
    pub start: f64,
    /// Transport position seen on the last frame while recording.
    pub last_position: f64,
    seen: bool,
    notes: Vec<RecordedNote>,
    /// The passes over the loop finished so far (beats from, to), and where the
    /// current one began.
    passes: Vec<(f64, f64)>,
    pass_start: f64,
}

/// A finished take, ready to become a clip.
pub enum Finished {
    Session { track: usize, slot: usize, start: f64, length: f64, notes: Vec<Note> },
    Arrangement { track: usize, start: f64, length: f64, notes: Vec<Note> },
    /// An arrangement take ended; audio tracks record exactly these spans, one per
    /// pass over the loop (one alone without a loop), in the order they were played.
    ArrangementEnded { passes: Vec<(f64, f64)> },
    /// The engine never started the take (e.g. the slot was filled meanwhile).
    Cancelled,
}

#[derive(Default)]
pub struct Recorder {
    pub session: Vec<SessionTake>,
    pub arrangement: Option<ArrangementTake>,
}

/// Rounds a take up to whole bars, at least one.
fn bars(length: f64) -> f64 {
    ((length / 4.0).ceil() * 4.0).max(4.0)
}

/// How early, in beats, a note may come and still belong to a take.
const EARLY: f64 = 0.125;

fn local_notes(notes: &[RecordedNote], start: f64, length: f64) -> Vec<Note> {
    let mut local: Vec<Note> = notes
        .iter()
        .map(|n| Note {
            start: n.start - start,
            length: n.length,
            pitch: n.pitch,
            velocity: n.velocity,
        })
        // A note played a hair early (the downbeat, or after latency compensation) is its first.
        .filter(|n| n.start >= -EARLY && n.start < length)
        .map(|mut n| {
            n.start = n.start.max(0.0);
            n.length = n.length.min(length - n.start);
            n
        })
        .collect();
    local.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    local
}

impl Recorder {
    pub fn start_session(&mut self, track: usize, slot: usize) {
        self.session.retain(|t| t.track != track);
        self.session.push(SessionTake { track, slot, start: None, seen: false, waited: 0, stop_at: None, notes: Vec::new() });
    }

    pub fn stop_session(&mut self, track: usize, position: f64) {
        if let Some(take) = self.session.iter_mut().find(|t| t.track == track) {
            take.stop_at.get_or_insert(position);
        }
    }

    pub fn start_arrangement(&mut self, start: f64) {
        self.arrangement = Some(ArrangementTake { start, last_position: start, seen: false, notes: Vec::new(), passes: Vec::new(), pass_start: start });
    }

    /// Distributes captured notes and returns the takes that ended this frame.
    /// `looped` is the arrangement loop, if it is on: passes wrap at its end.
    pub fn update(&mut self, notes: Vec<RecordedNote>, state: &SharedState, armed: &[bool], looped: Option<(f64, f64)>) -> Vec<Finished> {
        for note in notes {
            if let Some(take) = self.session.iter_mut().find(|t| t.track == note.track) {
                take.notes.push(note);
            } else if let Some(take) = &mut self.arrangement {
                take.notes.push(note);
            }
        }

        let mut finished = Vec::new();
        let position = state.position();
        self.session.retain_mut(|take| {
            let status = &state.tracks[take.track];
            let recording = status.recording.load(Ordering::Relaxed);
            let queued = status.queued_record.load(Ordering::Relaxed)
                && status.queued.load(Ordering::Relaxed) == take.slot as i32;
            if recording == take.slot as i32 {
                take.seen = true;
                take.start = Some(status.record_start());
                return true;
            }
            if queued {
                take.seen = true;
                return true;
            }
            match take.start {
                Some(start) => {
                    let notes_end = take.notes.iter().map(|n| n.start + n.length).fold(start, f64::max);
                    let end = take.stop_at.unwrap_or(position).max(notes_end);
                    let length = bars(end - start);
                    finished.push(Finished::Session {
                        track: take.track,
                        slot: take.slot,
                        start,
                        length,
                        notes: local_notes(&take.notes, start, length),
                    });
                    false
                }
                None => {
                    take.waited += 1;
                    let gave_up = take.seen || take.waited > 30;
                    if gave_up {
                        finished.push(Finished::Cancelled);
                    }
                    !gave_up
                }
            }
        });

        let engine_recording = state.arrangement_recording.load(Ordering::Relaxed);
        if let Some(take) = &mut self.arrangement {
            if engine_recording {
                // Back at the loop's start: a pass is done.
                if take.seen && position + 1e-6 < take.last_position {
                    let (loop_start, loop_end) = looped.unwrap_or((position, take.last_position));
                    take.passes.push((take.pass_start, loop_end.max(take.pass_start)));
                    take.pass_start = loop_start;
                }
                take.seen = true;
                take.last_position = position;
            } else if take.seen {
                let mut take = self.arrangement.take().expect("checked above");
                take.passes.push((take.pass_start, take.last_position));
                let wrapped = take.passes.len() > 1;
                finished.push(Finished::ArrangementEnded { passes: take.passes.clone() });
                // Looped, the notes of every pass land in the loop (overdubbed).
                let start = match looped {
                    Some((loop_start, _)) if wrapped => take.start.min(loop_start).floor(),
                    _ => take.start.floor(),
                };
                for (track, &is_armed) in armed.iter().enumerate() {
                    let notes: Vec<RecordedNote> = take.notes.iter().filter(|n| n.track == track).copied().collect();
                    if !is_armed || notes.is_empty() {
                        continue;
                    }
                    let length = match looped {
                        Some((_, loop_end)) if wrapped => loop_end - start,
                        _ => bars(notes.iter().map(|n| n.start + n.length).fold(take.last_position, f64::max) - start),
                    };
                    finished.push(Finished::Arrangement {
                        track,
                        start,
                        length,
                        notes: local_notes(&notes, start, length),
                    });
                }
            }
        }
        finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(start: f64, length: f64, pitch: u8) -> RecordedNote {
        RecordedNote { track: 0, pitch, velocity: 0.8, start, length }
    }

    #[test]
    fn takes_round_up_to_bars_and_use_clip_time() {
        let notes = [note(8.5, 1.0, 60), note(10.0, 0.5, 62), note(7.0, 0.5, 40)];
        let local = local_notes(&notes, 8.0, bars(3.0));
        assert_eq!(bars(3.0), 4.0);
        assert_eq!(bars(4.5), 8.0);
        // The note before the take start is dropped; the rest are relative to it.
        assert_eq!(local.iter().map(|n| (n.start, n.pitch)).collect::<Vec<_>>(), vec![(0.5, 60), (2.0, 62)]);
    }

    #[test]
    fn notes_running_past_the_end_are_trimmed() {
        let local = local_notes(&[note(3.5, 2.0, 60)], 0.0, 4.0);
        assert_eq!(local[0].length, 0.5);
    }

    /// The record button with nothing armed, on a real engine run by hand: it arms
    /// the selected track, records what is played, and the take becomes a clip.
    #[test]
    fn the_record_button_arms_the_selected_track_and_records_it() {
        use crate::midi_map::Transport;
        let mut app = crate::app::PulseApp::headless(crate::project::Project::demo());
        let (audio, mut engine) = crate::app::Audio::detached();
        app.audio = audio;
        for track in &mut app.project.tracks {
            track.armed = false;
            track.arrangement.clear();
        }
        app.sync_all();
        let mut run = |app: &mut crate::app::PulseApp, beats: f64| {
            // 120 BPM: half a second per beat, in blocks of 512 frames.
            let mut buffer = vec![0.0; 1024];
            for _ in 0..((beats * 0.5 * 48_000.0) / 512.0).ceil() as usize {
                engine.process(&mut buffer, 2);
                app.update_recording();
            }
        };
        app.project.tempo = 120.0;
        app.sync_all();

        // The master selected: nothing to arm, and it says so.
        app.selected_target = engine::Target::Master;
        app.transport(Transport::Record);
        assert!(!app.is_arrangement_recording());
        assert!(app.status.as_deref().unwrap_or("").contains("Selecciona una pista"));

        // A MIDI track selected: armed, recording and playing.
        app.selected_target = engine::Target::Track(1);
        app.transport(Transport::Record);
        assert!(app.project.tracks[1].armed, "the selected track is armed");
        assert!(app.is_arrangement_recording());
        run(&mut app, 0.5);
        assert!(app.audio.is_playing());
        let live = app.audio.live().unwrap();
        live.send(engine::LiveNote::note(60, 0.8));
        run(&mut app, 1.0);
        live.send(engine::LiveNote::note(60, 0.0));
        run(&mut app, 0.5);
        // Pressed again: the take ends and lands on the arrangement.
        app.transport(Transport::Record);
        run(&mut app, 0.5);
        assert!(!app.is_arrangement_recording());
        let lane = &app.project.tracks[1].arrangement;
        assert_eq!(lane.len(), 1, "one take");
        let engine::model::Clip::Midi(clip) = &lane[0].clip else { panic!("a MIDI take") };
        assert_eq!(clip.notes.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![60]);
        // Recording again keeps the armed track and doesn't arm others.
        app.selected_target = engine::Target::Track(2);
        app.transport(Transport::Record);
        assert!(app.project.tracks[1].armed && !app.project.tracks[2].armed);
    }

    /// A MIDI knob linked with MIDI learn, turned while recording: its moves become
    /// automation that plays back.
    #[test]
    fn a_linked_knob_turned_while_recording_becomes_automation() {
        use crate::midi_map::{Control, Mapping, Transport};
        use engine::model::Param;
        use engine::Target;
        let mut app = crate::app::PulseApp::headless(crate::project::Project::demo());
        let (audio, mut engine) = crate::app::Audio::detached();
        app.audio = audio;
        for track in &mut app.project.tracks {
            track.armed = false;
            track.automation.clear();
        }
        app.project.tempo = 120.0;
        let volume = Control::Param { target: Target::Track(1), param: Param::Volume };
        app.project.midi_map.push(Mapping { cc: 74, control: volume });
        app.sync_all();
        let mut run = |app: &mut crate::app::PulseApp, beats: f64| {
            let mut buffer = vec![0.0; 1024];
            for _ in 0..((beats * 0.5 * 48_000.0) / 512.0).ceil() as usize {
                engine.process(&mut buffer, 2);
                app.update_recording();
            }
        };
        app.selected_target = Target::Track(1);
        app.transport(Transport::Record);
        run(&mut app, 1.0);
        // The knob turns up over two beats.
        for step in 0..=16 {
            app.apply_cc(74, 40 + step * 5);
            run(&mut app, 0.125);
        }
        run(&mut app, 1.0);
        app.transport(Transport::Record);
        run(&mut app, 0.5);
        assert!(!app.is_arrangement_recording());
        let lane = app.automation(Target::Track(1)).iter().find(|l| l.param == Param::Volume).expect("a volume lane");
        let (first, last) = (lane.points.first().unwrap(), lane.points.last().unwrap());
        assert!(lane.points.len() >= 10, "{} points", lane.points.len());
        assert!(first.beat > 0.5 && first.beat < 1.5, "starts where the knob started: {first:?}");
        assert!(last.beat > 2.5 && last.beat < 3.5, "ends where it stopped: {last:?}");
        assert!(last.value > first.value);
        assert!(app.status.as_deref().unwrap_or("").contains("automatización"), "{:?}", app.status);
        // It plays back: not overridden, and the engine has it.
        assert_eq!(app.automation_state(Target::Track(1), Param::Volume), Some(false));
    }

    /// With Automation Arm off nothing is written; turned on while recording, the
    /// moves from then on are; turned off, what was recorded is kept.
    #[test]
    fn automation_arm_decides_whether_moves_are_recorded() {
        use crate::midi_map::{Control, Mapping, Transport};
        use engine::model::Param;
        use engine::Target;
        let mut app = crate::app::PulseApp::headless(crate::project::Project::demo());
        let (audio, mut engine) = crate::app::Audio::detached();
        app.audio = audio;
        for track in &mut app.project.tracks {
            track.armed = false;
            track.automation.clear();
        }
        app.project.tempo = 120.0;
        app.project.midi_map.push(Mapping { cc: 74, control: Control::Param { target: Target::Track(1), param: Param::Volume } });
        app.project.midi_map.push(Mapping { cc: 20, control: Control::Transport(Transport::AutomationArm) });
        app.sync_all();
        let mut run = |app: &mut crate::app::PulseApp, beats: f64| {
            let mut buffer = vec![0.0; 1024];
            for _ in 0..((beats * 0.5 * 48_000.0) / 512.0).ceil() as usize {
                engine.process(&mut buffer, 2);
                app.update_recording();
            }
        };
        let sweep = |app: &mut crate::app::PulseApp, run: &mut dyn FnMut(&mut crate::app::PulseApp, f64), from: u8| {
            for step in 0..8 {
                app.apply_cc(74, from + step * 4);
                run(app, 0.125);
            }
        };
        let lane = |app: &crate::app::PulseApp| app.automation(Target::Track(1)).iter().find(|l| l.param == Param::Volume).map(|l| l.points.clone()).unwrap_or_default();
        assert!(app.settings.automation_arm, "on by default");

        // Off: a take with the knob turned writes nothing.
        app.transport(Transport::AutomationArm);
        assert!(!app.settings.automation_arm);
        app.selected_target = Target::Track(1);
        app.transport(Transport::Record);
        run(&mut app, 0.5);
        sweep(&mut app, &mut run, 40);
        app.transport(Transport::Record);
        run(&mut app, 0.5);
        assert!(lane(&app).is_empty(), "nothing recorded with Automation Arm off");

        // On halfway through a take (with a button linked to it): only what follows.
        app.transport(Transport::Record);
        run(&mut app, 0.5);
        sweep(&mut app, &mut run, 20);
        let turned_on = app.audio.position();
        app.apply_cc(20, 127);
        assert!(app.settings.automation_arm);
        run(&mut app, 0.25);
        sweep(&mut app, &mut run, 60);
        // Off again before the take ends: what was recorded stays.
        app.transport(Transport::AutomationArm);
        let points = lane(&app);
        assert!(!points.is_empty(), "kept when turned off");
        assert!(points.first().unwrap().beat >= turned_on - 0.1, "nothing from before it was on: {:?} vs {turned_on}", points.first());
        let count = points.len();
        sweep(&mut app, &mut run, 90);
        app.transport(Transport::Record);
        run(&mut app, 0.5);
        assert_eq!(lane(&app).len(), count, "nothing after it was turned off");
    }

    /// Recording a session clip while a linked knob turns: the moves become the
    /// clip's envelope (in clip time), not the track's automation, and play with it.
    #[test]
    fn a_knob_turned_while_recording_a_session_clip_becomes_its_envelope() {
        use crate::midi_map::{Control, Mapping};
        use engine::model::Param;
        use engine::Target;
        let mut app = crate::app::PulseApp::headless(crate::project::Project::demo());
        let (audio, mut engine) = crate::app::Audio::detached();
        app.audio = audio;
        for track in &mut app.project.tracks {
            track.armed = false;
            track.automation.clear();
            track.clips = Default::default();
        }
        app.project.tempo = 120.0;
        app.project.midi_map.push(Mapping { cc: 74, control: Control::Param { target: Target::Track(1), param: Param::Volume } });
        app.sync_all();
        let mut run = |app: &mut crate::app::PulseApp, beats: f64| {
            let mut buffer = vec![0.0; 1024];
            for _ in 0..((beats * 0.5 * 48_000.0) / 512.0).ceil() as usize {
                engine.process(&mut buffer, 2);
                app.update_recording();
            }
        };
        app.toggle_arm(1);
        app.record_slot(1, 0);
        run(&mut app, 0.5);
        let start = app.recorder.session.first().and_then(|t| t.start).expect("the take started");
        // Turn the knob over two beats, a beat into the take.
        run(&mut app, 1.0);
        for step in 0..=16 {
            app.apply_cc(74, 20 + step * 6);
            run(&mut app, 0.125);
        }
        run(&mut app, 0.5);
        app.stop_session_recording(1);
        run(&mut app, 4.5);
        let Some(engine::model::Clip::Midi(clip)) = &app.project.tracks[1].clips[0] else { panic!("a recorded clip") };
        let envelope = clip.envelopes.iter().find(|l| l.param == Param::Volume).expect("a volume envelope in the clip");
        let (first, last) = (envelope.points.first().unwrap(), envelope.points.last().unwrap());
        assert!(first.beat > 0.5 && first.beat < 2.0, "in clip time (the take started at {start}): {first:?}");
        assert!(last.beat > 2.5 && last.beat < 4.0 && last.beat <= clip.length, "{last:?} in {}", clip.length);
        assert!(last.value > first.value);
        assert!(app.automation(Target::Track(1)).iter().all(|l| l.points.is_empty()), "not the track's automation");
        assert!(app.status.as_deref().unwrap_or("").contains("automatización"), "{:?}", app.status);
        assert!(app.clip_automation.is_empty());
    }

    /// Recording over the loop on a real engine, with an input fed by hand: every
    /// pass is a take of the loop, what came before it a clip of its own, and MIDI
    /// passes are overdubbed into one clip.
    #[test]
    fn recording_over_the_loop_keeps_a_take_per_pass() {
        use crate::midi_map::Transport;
        use engine::model::Clip;
        let mut app = crate::app::PulseApp::headless(crate::project::Project::demo());
        let (audio, mut engine) = crate::app::Audio::detached();
        app.audio = audio;
        for track in &mut app.project.tracks {
            track.armed = false;
            track.arrangement.clear();
        }
        app.project.tracks.push(crate::project::Track::new_audio("Voz", [0, 0, 0]));
        let voice = app.project.tracks.len() - 1;
        app.project.tracks[voice].monitor = crate::project::Monitor::Off;
        app.project.tempo = 120.0;
        (app.project.loop_enabled, app.project.loop_start, app.project.loop_end) = (true, 4.0, 8.0);
        app.sync_all();
        app.sync_loop();
        let (mut samples, mut markers, handle) = engine::detached_input(48_000.0, 1);
        app.capture = Some(crate::capture::InputCapture::with_handle(None, handle));
        app.project.tracks[voice].armed = true;
        app.project.tracks[1].armed = true;
        app.audio.send(engine::Command::SetArm { track: voice, armed: true });
        app.audio.send(engine::Command::SetArm { track: 1, armed: true });
        app.audio.send(engine::Command::SetPosition(2.0));

        // Each input sample is the time it was captured at, so a take tells which pass it holds.
        let frame = std::cell::Cell::new(0u64);
        let state = app.audio.state().unwrap();
        let mut run = |app: &mut crate::app::PulseApp, beats: f64| {
            let mut buffer = vec![0.0; 1024];
            for _ in 0..((beats * 0.5 * 48_000.0) / 512.0).round() as usize {
                let beat = state.position();
                let playing = state.playing.load(Ordering::Relaxed);
                engine.process(&mut buffer, 2);
                let _ = markers.push(engine::Marker { frame: frame.get(), beat, playing });
                for i in 0..512 {
                    let _ = samples.push((frame.get() + i) as f32 / 48_000.0);
                }
                frame.set(frame.get() + 512);
                app.update_recording();
            }
        };
        run(&mut app, 0.5);
        let started = frame.get() as f32 / 48_000.0;
        app.transport(Transport::Record);
        // Two beats before the loop, three passes and a half; a note in two of them.
        let live = app.audio.live().unwrap();
        run(&mut app, 2.0 + 4.0 + 1.0);
        live.send(engine::LiveNote::note(60, 0.8));
        run(&mut app, 0.5);
        live.send(engine::LiveNote::note(60, 0.0));
        run(&mut app, 4.0);
        live.send(engine::LiveNote::note(64, 0.8));
        run(&mut app, 0.5);
        live.send(engine::LiveNote::note(64, 0.0));
        run(&mut app, 4.0);
        app.transport(Transport::Record);
        run(&mut app, 0.5);
        assert!(!app.is_arrangement_recording());

        let lane = app.project.tracks[voice].arrangement.clone();
        assert_eq!(lane.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>(), vec![(2.0, 2.0), (4.0, 4.0)]);
        let Clip::Audio(before) = &lane[0].clip else { panic!() };
        let Clip::Audio(looped) = &lane[1].clip else { panic!() };
        assert_eq!(looped.takes.len(), 4, "three passes and the half one");
        assert_eq!(looped.sample, looped.takes[3], "the last take plays");
        assert_eq!(looped.name, "Toma 4");
        let mut first = |path: &str| app.samples.get(path).read(0.0).0;
        // Before the loop: from the moment recording started.
        assert!((first(&before.sample) - started).abs() < 0.02, "{} vs {started}", first(&before.sample));
        // Pass k began 2 + 4k beats (1 + 2k seconds) after that.
        for (k, take) in looped.takes.iter().enumerate() {
            let expected = started + 1.0 + 2.0 * k as f32;
            assert!((first(take) - expected).abs() < 0.02, "take {k}: {} vs {expected}", first(take));
        }
        // The half pass is silent past where it stopped.
        let half = app.samples.get(&looped.takes[3]);
        assert_eq!(half.read(3.5 * 24_000.0).0, 0.0);

        // Choosing another take swaps what plays.
        app.choose_take(voice, 1, 1);
        let Clip::Audio(chosen) = &app.project.tracks[voice].arrangement[1].clip else { panic!() };
        assert_eq!((chosen.sample.as_str(), chosen.name.as_str()), (looped.takes[1].as_str(), "Toma 2"));

        // MIDI: one clip over the loop with the notes of both passes.
        let midi = &app.project.tracks[1].arrangement;
        assert_eq!(midi.len(), 1);
        let Clip::Midi(clip) = &midi[0].clip else { panic!() };
        assert_eq!((midi[0].start, midi[0].length), (2.0, 6.0));
        assert_eq!(clip.notes.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![60, 64]);
    }
}
