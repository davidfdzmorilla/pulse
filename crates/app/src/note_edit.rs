//! Editing operations on a clip's notes, independent of the UI.
//!
//! Selections are sets of indices into `notes`. Operations that add notes append
//! them and return the new indices, so callers can select what was just created.

use engine::model::Note;
use std::collections::BTreeSet;

pub type Selection = BTreeSet<usize>;

/// Shortest note the editor creates or allows when resizing.
pub const MIN_LENGTH: f64 = 1.0 / 32.0;

pub fn snap(beat: f64, grid: f64) -> f64 {
    (beat / grid).round() * grid
}

pub fn snap_down(beat: f64, grid: f64) -> f64 {
    (beat / grid).floor() * grid
}

/// The selected indices, or every note when nothing is selected.
pub fn targets(notes: &[Note], selection: &Selection) -> Vec<usize> {
    if selection.is_empty() {
        (0..notes.len()).collect()
    } else {
        selection.iter().copied().filter(|&i| i < notes.len()).collect()
    }
}

/// The chance (0..1) the notes at `indices` have of playing each time round.
pub fn set_chance(notes: &mut [Note], indices: &[usize], chance: f32) {
    for &i in indices {
        notes[i].chance = chance.clamp(0.0, 1.0);
    }
}

/// Moves note starts towards the grid by `strength` (0..1); optionally their ends too.
pub fn quantize(notes: &mut [Note], indices: &[usize], grid: f64, strength: f64, ends: bool, clip_length: f64) {
    for &i in indices {
        let note = &mut notes[i];
        let end = note.start + note.length;
        let start = note.start + (snap(note.start, grid) - note.start) * strength;
        note.start = start.clamp(0.0, (clip_length - MIN_LENGTH).max(0.0));
        let end = if ends { end + (snap(end, grid) - end) * strength } else { end };
        note.length = (end - note.start).max(MIN_LENGTH);
    }
}

/// Transposes notes, keeping them in the MIDI range.
pub fn transpose(notes: &mut [Note], indices: &[usize], semitones: i32) {
    let fits = indices.iter().all(|&i| (0..=127).contains(&(notes[i].pitch as i32 + semitones)));
    if !fits {
        return;
    }
    for &i in indices {
        notes[i].pitch = (notes[i].pitch as i32 + semitones) as u8;
    }
}

/// Clamps a time offset so every moved note stays inside the clip.
pub fn clamp_shift(notes: &[Note], indices: &[usize], beats: f64, clip_length: f64) -> f64 {
    let earliest = indices.iter().map(|&i| notes[i].start).fold(f64::INFINITY, f64::min);
    let latest = indices.iter().map(|&i| notes[i].start).fold(f64::NEG_INFINITY, f64::max);
    if indices.is_empty() {
        return 0.0;
    }
    beats.max(-earliest).min((clip_length - MIN_LENGTH - latest).max(0.0))
}

pub fn shift(notes: &mut [Note], indices: &[usize], beats: f64, clip_length: f64) {
    let beats = clamp_shift(notes, indices, beats, clip_length);
    for &i in indices {
        notes[i].start += beats;
    }
}

/// Removes the given notes. Indices of the remaining notes change.
pub fn delete(notes: &mut Vec<Note>, indices: &Selection) {
    for &i in indices.iter().rev() {
        if i < notes.len() {
            notes.remove(i);
        }
    }
}

/// Copies notes with times relative to the earliest one.
pub fn copy(notes: &[Note], indices: &[usize]) -> Vec<Note> {
    let earliest = indices.iter().map(|&i| notes[i].start).fold(f64::INFINITY, f64::min);
    indices
        .iter()
        .map(|&i| Note { start: notes[i].start - earliest, ..notes[i].clone() })
        .collect()
}

/// Inserts copied notes at `at`; notes that would fall outside the clip are dropped.
pub fn paste(notes: &mut Vec<Note>, clipboard: &[Note], at: f64, clip_length: f64) -> Selection {
    let mut added = Selection::new();
    for note in clipboard {
        let start = at + note.start;
        if start < clip_length {
            notes.push(Note { start, length: note.length.min(clip_length - start), ..note.clone() });
            added.insert(notes.len() - 1);
        }
    }
    added
}

/// Copies the notes right after the end of their own span (like ⌘D in Live).
pub fn duplicate(notes: &mut Vec<Note>, indices: &[usize], clip_length: f64) -> Selection {
    if indices.is_empty() {
        return Selection::new();
    }
    let start = indices.iter().map(|&i| notes[i].start).fold(f64::INFINITY, f64::min);
    let end = indices.iter().map(|&i| notes[i].start + notes[i].length).fold(0.0, f64::max);
    let clipboard = copy(notes, indices);
    paste(notes, &clipboard, end.max(start), clip_length)
}

/// Notes overlapping the beat range `[from, to]` whose pitch is in `pitches`.
pub fn in_region(notes: &[Note], (from, to): (f64, f64), pitches: impl Fn(u8) -> bool) -> Selection {
    notes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.start <= to && n.start + n.length >= from && pitches(n.pitch))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod chance_tests {
    use super::*;

    /// A chance is set on the notes chosen, saved only when it isn't 100 %,
    /// and notes from before have 100 %.
    #[test]
    fn notes_keep_their_chance() {
        let mut notes = vec![Note::new(0.0, 0.25, 42, 0.8), Note::new(0.5, 0.25, 42, 0.8)];
        set_chance(&mut notes, &[1], 0.4);
        assert_eq!((notes[0].chance, notes[1].chance), (1.0, 0.4));
        assert!(!serde_json::to_string(&notes[0]).unwrap().contains("chance"));
        let back: Note = serde_json::from_str(&serde_json::to_string(&notes[1]).unwrap()).unwrap();
        assert_eq!(back, notes[1]);
        let old: Note = serde_json::from_str(r#"{"start": 0.0, "length": 1.0, "pitch": 36, "velocity": 1.0}"#).unwrap();
        assert_eq!(old.chance, 1.0);
        set_chance(&mut notes, &[0], 7.0);
        assert_eq!(notes[0].chance, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(start: f64, length: f64, pitch: u8) -> Note {
        Note { start, length, pitch, velocity: 0.8, chance: 1.0 }
    }

    #[test]
    fn quantize_moves_starts_by_strength_and_optionally_ends() {
        let mut notes = vec![note(0.1, 0.4, 60), note(0.9, 0.2, 62)];
        quantize(&mut notes, &[0, 1], 0.25, 1.0, false, 4.0);
        assert_eq!(notes[0].start, 0.0);
        assert!((notes[0].length - 0.5).abs() < 1e-9, "the end stays where it was");
        assert_eq!(notes[1].start, 1.0);

        let mut notes = vec![note(0.1, 0.4, 60)];
        quantize(&mut notes, &[0], 0.25, 0.5, true, 4.0);
        assert!((notes[0].start - 0.05).abs() < 1e-9, "half strength goes half way");
        assert!((notes[0].start + notes[0].length - 0.5).abs() < 1e-9, "the end at 0.5 is already on the grid");
    }

    #[test]
    fn shifting_keeps_notes_inside_the_clip() {
        let mut notes = vec![note(0.5, 0.25, 60), note(3.5, 0.25, 62)];
        shift(&mut notes, &[0, 1], -1.0, 4.0);
        assert_eq!((notes[0].start, notes[1].start), (0.0, 3.0));
        shift(&mut notes, &[0, 1], 10.0, 4.0);
        assert!(notes[1].start < 4.0);
    }

    #[test]
    fn transpose_refuses_to_leave_the_midi_range() {
        let mut notes = vec![note(0.0, 1.0, 120), note(0.0, 1.0, 60)];
        transpose(&mut notes, &[0, 1], 12);
        assert_eq!((notes[0].pitch, notes[1].pitch), (120, 60));
        transpose(&mut notes, &[0, 1], -12);
        assert_eq!((notes[0].pitch, notes[1].pitch), (108, 48));
    }

    #[test]
    fn duplicate_places_a_copy_after_the_selection() {
        let mut notes = vec![note(1.0, 0.5, 60), note(1.5, 0.5, 64), note(0.0, 0.25, 36)];
        let added = duplicate(&mut notes, &[0, 1], 8.0);
        assert_eq!(added.into_iter().collect::<Vec<_>>(), vec![3, 4]);
        assert_eq!((notes[3].start, notes[3].pitch), (2.0, 60));
        assert_eq!((notes[4].start, notes[4].pitch), (2.5, 64));
    }

    #[test]
    fn copy_and_paste_keep_relative_timing_and_respect_the_clip_end() {
        let notes = vec![note(2.0, 1.0, 60), note(3.0, 2.0, 62)];
        let clipboard = copy(&notes, &[0, 1]);
        assert_eq!((clipboard[0].start, clipboard[1].start), (0.0, 1.0));
        let mut target = Vec::new();
        let added = paste(&mut target, &clipboard, 3.0, 4.5);
        assert_eq!(added.len(), 2);
        assert_eq!(target[1].start, 4.0);
        assert_eq!(target[1].length, 0.5, "trimmed at the clip end");
        assert!(paste(&mut target, &clipboard, 4.5, 4.5).is_empty());
    }

    #[test]
    fn delete_and_region_selection() {
        let mut notes = vec![note(0.0, 1.0, 60), note(2.0, 1.0, 62), note(2.5, 0.5, 70)];
        let region = in_region(&notes, (1.5, 2.2), |p| p < 65);
        assert_eq!(region.iter().copied().collect::<Vec<_>>(), vec![1]);
        delete(&mut notes, &Selection::from([0, 2]));
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].pitch, 62);
    }

    #[test]
    fn empty_selection_targets_every_note() {
        let notes = vec![note(0.0, 1.0, 60), note(1.0, 1.0, 62)];
        assert_eq!(targets(&notes, &Selection::new()), vec![0, 1]);
        assert_eq!(targets(&notes, &Selection::from([1, 5])), vec![1]);
    }
}
