//! Editing operations on a clip's notes, independent of the UI.
//!
//! Selections are sets of indices into `notes`. Operations that add notes append
//! them and return the new indices, so callers can select what was just created.

use crate::i18n::tr;
use engine::model::{AutomationPoint, ControlLane, MidiControl, Note};
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

/// How the velocities of a roll go: from soft to loud, the other way, or flat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ramp {
    Up,
    Down,
    Flat,
}

impl Ramp {
    pub const ALL: [Ramp; 3] = [Ramp::Up, Ramp::Down, Ramp::Flat];

    pub fn label(self) -> &'static str {
        match self {
            Ramp::Up => tr("Crece"),
            Ramp::Down => tr("Se apaga"),
            Ramp::Flat => tr("Plana"),
        }
    }
}

/// Where a ramp starts (or ends), as a share of the note's velocity.
pub const RAMP_LOW: f32 = 0.35;

/// Turns the notes at `indices` into rolls of `step` beats. Each run of
/// selected notes of one pitch that follow on without a gap (a long note, or a
/// roll made before) becomes one roll as long as it, its velocities ramped
/// from the loudest of them. The new notes are appended; the rest keep their
/// order. Returns the new notes' indices.
pub fn roll(notes: &mut Vec<Note>, indices: &[usize], step: f64, ramp: Ramp) -> Selection {
    if indices.is_empty() || step <= 0.0 {
        return Selection::new();
    }
    let mut chosen: Vec<Note> = indices.iter().filter_map(|&i| notes.get(i).cloned()).collect();
    chosen.sort_by(|a, b| a.pitch.cmp(&b.pitch).then(a.start.total_cmp(&b.start)));
    // Runs: (start, end, velocity, model note).
    let mut runs: Vec<(f64, f64, f32, Note)> = Vec::new();
    for n in chosen {
        match runs.last_mut() {
            Some(run) if run.3.pitch == n.pitch && n.start <= run.1 + 1e-6 => {
                run.1 = run.1.max(n.start + n.length);
                run.2 = run.2.max(n.velocity);
            }
            _ => runs.push((n.start, n.start + n.length, n.velocity, n)),
        }
    }
    let removed: Selection = indices.iter().copied().collect();
    delete(notes, &removed);
    let mut added = Selection::new();
    for (start, end, velocity, model) in runs {
        let count = (((end - start) / step) + 1e-6).floor().max(1.0) as usize;
        let length = if count == 1 { end - start } else { step };
        for k in 0..count {
            let at = if count > 1 { k as f32 / (count - 1) as f32 } else { 1.0 };
            let share = match ramp {
                Ramp::Up => RAMP_LOW + (1.0 - RAMP_LOW) * at,
                Ramp::Down => 1.0 - (1.0 - RAMP_LOW) * at,
                Ramp::Flat => 1.0,
            };
            notes.push(Note { start: start + k as f64 * step, length, velocity: (velocity * share).clamp(0.05, 1.0), ..model.clone() });
            added.insert(notes.len() - 1);
        }
    }
    added
}

/// The pitch ramps a roll can have on the drums: semitones its last hit goes
/// (0: none).
pub const PITCH_RAMPS: [(f32, &str); 4] = [(0.0, "Igual"), (5.0, "Sube 5"), (12.0, "Sube 12"), (-12.0, "Baja 12")];

/// A pitch ramp's name (from [`PITCH_RAMPS`]) in the interface's language.
pub fn pitch_ramp_name(name: &'static str) -> &'static str {
    match name {
        "Igual" => tr("Igual"),
        "Sube 5" => tr("Sube 5"),
        "Sube 12" => tr("Sube 12"),
        "Baja 12" => tr("Baja 12"),
        other => other,
    }
}

/// A pitch ramp over the rolls among `added` (as [`roll`] returns them): the
/// clip's pitch bend lane goes from rest at each roll's first hit to `value`
/// (-1..1) at its last, holds to the roll's end and goes back to rest. Drum
/// hits take the bend they start with, so each hit of the roll is a step
/// higher (or lower). What the lane had over the rolls goes.
pub fn pitch_ramp(controls: &mut Vec<ControlLane>, notes: &[Note], added: &Selection, value: f32) {
    let mut hits: Vec<&Note> = added.iter().filter_map(|&i| notes.get(i)).collect();
    if hits.is_empty() || value == 0.0 {
        return;
    }
    hits.sort_by(|a, b| a.pitch.cmp(&b.pitch).then(a.start.total_cmp(&b.start)));
    // Runs of one pitch without a gap: (first start, last start, end).
    let mut runs: Vec<(f64, f64, f64)> = Vec::new();
    let mut last_pitch = None;
    for n in hits {
        match runs.last_mut() {
            Some(run) if last_pitch == Some(n.pitch) && n.start <= run.2 + 1e-6 => {
                run.1 = n.start;
                run.2 = n.start + n.length;
            }
            _ => runs.push((n.start, n.start, n.start + n.length)),
        }
        last_pitch = Some(n.pitch);
    }
    let lane = match controls.iter().position(|l| l.control == MidiControl::PitchBend) {
        Some(i) => &mut controls[i],
        None => {
            controls.push(ControlLane::new(MidiControl::PitchBend));
            controls.last_mut().unwrap()
        }
    };
    let value = value.clamp(-1.0, 1.0);
    let point = |beat: f64, value: f32| AutomationPoint { beat, value, curve: 0.0 };
    for (first, last, end) in runs {
        lane.points.retain(|p| p.beat < first - 1e-6 || p.beat > end + 1e-6);
        let hold = (end - 1.0 / 256.0).max(last);
        lane.points.extend([point(first, 0.0), point(last, value), point(hold, value), point(end, 0.0)]);
    }
    lane.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    lane.points.dedup_by(|b, a| (a.beat - b.beat).abs() < 1e-9 && a.value == b.value);
}

/// The grid a clip's notes were written on: of `grids` (coarsest first), the
/// coarsest two thirds of their starts fall on (sixteenths with a triplet
/// roll are on 1/16), else the one most fall on; 1/16 if there are no notes.
pub fn guess_grid(notes: &[Note], grids: &[f64]) -> f64 {
    let fits = |g: f64| notes.iter().filter(|n| ((n.start / g) - (n.start / g).round()).abs() < 1e-3).count();
    if notes.is_empty() {
        return 0.25;
    }
    if let Some(&g) = grids.iter().find(|&&g| fits(g) * 3 >= notes.len() * 2) {
        return g;
    }
    let mut best = (0.25, 0);
    for &g in grids {
        let count = fits(g);
        if count > best.1 {
            best = (g, count);
        }
    }
    best.0
}

#[cfg(test)]
mod roll_tests {
    use super::*;

    fn note(start: f64, length: f64, pitch: u8, velocity: f32) -> Note {
        Note { start, length, pitch, velocity, chance: 1.0 }
    }

    /// A quarter note becomes six 1/16T notes rising from soft to its velocity.
    #[test]
    fn a_long_note_becomes_a_ramped_roll() {
        let mut notes = vec![note(0.0, 0.5, 36, 0.9), note(1.0, 1.0, 42, 0.8)];
        let added = roll(&mut notes, &[1], 1.0 / 6.0, Ramp::Up);
        assert_eq!(added.len(), 6);
        assert_eq!(notes.len(), 7);
        assert_eq!(notes[0].pitch, 36, "the other note stays");
        let rolled: Vec<&Note> = added.iter().map(|&i| &notes[i]).collect();
        for (k, n) in rolled.iter().enumerate() {
            assert!((n.start - (1.0 + k as f64 / 6.0)).abs() < 1e-9, "on the 1/16T grid");
            assert!((n.length - 1.0 / 6.0).abs() < 1e-9);
            assert_eq!(n.pitch, 42);
        }
        assert!((rolled[0].velocity - 0.8 * RAMP_LOW).abs() < 1e-6);
        assert!((rolled[5].velocity - 0.8).abs() < 1e-6);
        assert!(rolled.windows(2).all(|w| w[1].velocity > w[0].velocity), "rising");
    }

    /// Each rate fills the note: 1/16, 1/16T, 1/32, 1/32T, 1/64 in one beat.
    #[test]
    fn every_rate_fills_the_note() {
        for (step, count) in [(0.25, 4), (1.0 / 6.0, 6), (0.125, 8), (1.0 / 12.0, 12), (0.0625, 16)] {
            let mut notes = vec![note(2.0, 1.0, 38, 1.0)];
            let added = roll(&mut notes, &[0], step, Ramp::Flat);
            assert_eq!(added.len(), count, "step {step}");
            let end = notes.iter().map(|n| n.start + n.length).fold(0.0, f64::max);
            assert!((end - 3.0).abs() < 1e-9, "ends where the note did");
            assert!(notes.iter().all(|n| n.velocity == 1.0));
        }
    }

    /// A roll rolled again at another rate is one roll, not a roll of each
    /// note; fading out goes the other way. Separate notes stay separate.
    #[test]
    fn a_roll_can_be_rolled_again_and_runs_are_by_pitch() {
        let mut notes = vec![note(0.0, 1.0, 42, 0.7)];
        let first = roll(&mut notes, &[0], 0.125, Ramp::Up);
        let indices: Vec<usize> = first.into_iter().collect();
        let again = roll(&mut notes, &indices, 1.0 / 12.0, Ramp::Down);
        assert_eq!(again.len(), 12);
        assert_eq!(notes.len(), 12);
        let v: Vec<f32> = notes.iter().map(|n| n.velocity).collect();
        assert!((v[0] - 0.7).abs() < 1e-6 && (v[11] - 0.7 * RAMP_LOW).abs() < 1e-6, "{v:?}");
        // Two pitches and a gap: three rolls.
        let mut notes = vec![note(0.0, 0.5, 42, 1.0), note(1.0, 0.5, 42, 1.0), note(0.0, 0.5, 46, 1.0)];
        let added = roll(&mut notes, &[0, 1, 2], 0.125, Ramp::Flat);
        assert_eq!(added.len(), 12);
        assert!(!notes.iter().any(|n| n.start >= 0.5 && n.start < 1.0), "the gap stays empty");
        // A note shorter than one step stays one note.
        let mut notes = vec![note(0.0, 0.1, 42, 1.0)];
        roll(&mut notes, &[0], 0.125, Ramp::Up);
        assert_eq!(notes.len(), 1);
        assert!((notes[0].length - 0.1).abs() < 1e-9);
    }

    #[test]
    fn the_grid_is_guessed_from_the_notes() {
        let grids = [0.25, 1.0 / 6.0, 0.125, 1.0 / 12.0, 0.0625];
        assert_eq!(guess_grid(&[], &grids), 0.25);
        let sixteenths: Vec<Note> = (0..16).map(|k| note(k as f64 * 0.25, 0.25, 36, 1.0)).collect();
        assert_eq!(guess_grid(&sixteenths, &grids), 0.25, "the 808 on sixteenths");
        let triplets: Vec<Note> = (0..12).map(|k| note(k as f64 / 6.0, 1.0 / 6.0, 42, 1.0)).collect();
        assert_eq!(guess_grid(&triplets, &grids), 1.0 / 6.0);
        let mut hats = sixteenths.clone();
        hats.extend((0..6).map(|k| note(3.0 + k as f64 / 6.0, 1.0 / 6.0, 42, 1.0)));
        assert_eq!(guess_grid(&hats, &grids), 0.25, "mostly sixteenths, a triplet roll");
        let quarters: Vec<Note> = (0..4).map(|k| note(k as f64, 1.0, 60, 1.0)).collect();
        assert_eq!(guess_grid(&quarters, &grids), 0.25, "no coarser than 1/16");
        let mut with_roll: Vec<Note> = (0..4).map(|k| note(k as f64 * 0.25, 0.25, 36, 1.0)).collect();
        with_roll.extend((0..16).map(|k| note(2.0 + k as f64 * 0.125, 0.125, 38, 1.0)));
        assert_eq!(guess_grid(&with_roll, &grids), 0.125, "mostly a 1/32 roll");
    }

    /// A roll with a pitch ramp: the bend lane rises from rest at its first
    /// hit to the value at its last, and comes back after it; the notes are
    /// untouched and what the lane had elsewhere stays.
    #[test]
    fn a_pitch_ramp_bends_the_roll_and_nothing_else() {
        let mut notes = vec![note(0.0, 0.5, 36, 0.9), note(1.0, 1.0, 42, 0.8)];
        let added = roll(&mut notes, &[1], 0.125, Ramp::Up);
        let mut controls = vec![ControlLane { control: MidiControl::PitchBend, points: vec![AutomationPoint { beat: 3.0, value: 0.5, curve: 0.0 }, AutomationPoint { beat: 1.5, value: -1.0, curve: 0.0 }] }];
        controls[0].points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        pitch_ramp(&mut controls, &notes, &added, 1.0);
        let lane = &controls[0];
        assert!(lane.points.windows(2).all(|w| w[0].beat <= w[1].beat));
        let hits: Vec<f32> = added.iter().map(|&i| lane.value_at(notes[i].start).unwrap()).collect();
        assert_eq!(hits.len(), 8);
        assert_eq!(hits[0], 0.0);
        assert!((hits[7] - 1.0).abs() < 1e-6);
        assert!(hits.windows(2).all(|w| w[1] > w[0]), "each hit higher: {hits:?}");
        assert_eq!(lane.value_at(0.0), Some(0.0), "the kick before it isn't bent");
        assert_eq!(lane.value_at(2.0), Some(0.0), "back at rest after the roll");
        assert_eq!(lane.value_at(3.0), Some(0.5), "the lane's own point after it stays");
        // A ramp down, on a clip without a lane: one is made.
        let mut controls = Vec::new();
        pitch_ramp(&mut controls, &notes, &added, -0.5);
        assert!((controls[0].value_at(1.875).unwrap() + 0.5).abs() < 1e-6);
        // No ramp: nothing written.
        let mut none = Vec::new();
        pitch_ramp(&mut none, &notes, &added, 0.0);
        assert!(none.is_empty());
    }
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
