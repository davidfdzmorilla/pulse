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
}

/// A finished take, ready to become a clip.
pub enum Finished {
    Session { track: usize, slot: usize, length: f64, notes: Vec<Note> },
    Arrangement { track: usize, start: f64, length: f64, notes: Vec<Note> },
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

fn local_notes(notes: &[RecordedNote], start: f64, length: f64) -> Vec<Note> {
    let mut local: Vec<Note> = notes
        .iter()
        .map(|n| Note {
            start: n.start - start,
            length: n.length,
            pitch: n.pitch,
            velocity: n.velocity,
        })
        .filter(|n| n.start >= -1e-6 && n.start < length)
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
        self.arrangement = Some(ArrangementTake { start, last_position: start, seen: false, notes: Vec::new() });
    }

    /// Distributes captured notes and returns the takes that ended this frame.
    pub fn update(&mut self, notes: Vec<RecordedNote>, state: &SharedState, armed: &[bool]) -> Vec<Finished> {
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
                take.seen = true;
                take.last_position = position;
            } else if take.seen {
                let take = self.arrangement.take().expect("checked above");
                let start = take.start.floor();
                for (track, &is_armed) in armed.iter().enumerate() {
                    let notes: Vec<RecordedNote> = take.notes.iter().filter(|n| n.track == track).copied().collect();
                    if !is_armed || notes.is_empty() {
                        continue;
                    }
                    let notes_end = notes.iter().map(|n| n.start + n.length).fold(take.last_position, f64::max);
                    let length = bars(notes_end - start);
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
}
