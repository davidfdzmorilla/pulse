//! Recorded expression: the pitch bend, the mod wheel, pressure and the other
//! controllers played while recording MIDI become lanes of the take's clip
//! (`MidiClip::controls`), which the engine plays back with its notes.
//!
//! A controller sends a value each time it moves: dozens a second. Between
//! two of them it holds still, so a move after a pause starts from where it
//! was (no ramp across the pause), and points the line through their
//! neighbours already passes near are thinned out. Looping over the
//! Arrangement, a later pass replaces a controller's moves only where it moved
//! it in that pass (as touch automation): the rest of the earlier passes stays.

use engine::model::{thin_points, AutomationPoint, ControlLane, MidiControl};
use engine::RecordedControl;
use std::collections::BTreeMap;

/// How far (in the controller's own units: a 7-bit controller's step is
/// 0.008) a thinned line may pass from a point it drops.
pub const TOLERANCE: f32 = 0.006;
/// A controller still for longer than this (beats) before it moves holds its
/// value until just before the move.
const HOLD_GAP: f64 = 1.0 / 8.0;
const HOLD_LEAD: f64 = 1.0 / 512.0;

/// The points a controller's values (transport beats, in time order) leave in
/// a clip starting at `start`, `length` beats long: what came just before it
/// is its value at the start.
fn shaped(values: &[(f64, f32)], start: f64, length: f64) -> Vec<AutomationPoint> {
    let mut points: Vec<AutomationPoint> = Vec::new();
    let mut before: Option<f32> = None;
    for &(beat, value) in values {
        let local = beat - start;
        if local < 0.0 {
            before = Some(value);
            continue;
        }
        if local > length {
            break;
        }
        if let (Some(value), true) = (before.take(), local > 0.0) {
            points.push(AutomationPoint { beat: 0.0, value, curve: 0.0 });
        }
        // Still since the last one: it held until now.
        if let Some(&last) = points.last() {
            if local - last.beat > HOLD_GAP && last.value != value {
                points.push(AutomationPoint { beat: local - HOLD_LEAD, value: last.value, curve: 0.0 });
            }
        }
        // Two at the same beat (in one block): the later one wins.
        match points.last_mut() {
            Some(last) if (last.beat - local).abs() < 1e-9 => last.value = value,
            _ => points.push(AutomationPoint { beat: local, value, curve: 0.0 }),
        }
    }
    if let Some(value) = before {
        // Only moved before the clip: it starts there.
        points.push(AutomationPoint { beat: 0.0, value, curve: 0.0 });
    }
    thin_points(&points, TOLERANCE)
}

/// The lanes for a take starting at `start` (transport beats), `length` beats
/// long, from the controllers moved (`pass`: which pass over the loop each
/// came in, 0 without one). Later passes replace a controller's points only
/// over the stretch they moved it.
pub fn take_lanes(moves: &[(usize, RecordedControl)], start: f64, length: f64) -> Vec<ControlLane> {
    let mut by_control: BTreeMap<MidiControl, BTreeMap<usize, Vec<(f64, f32)>>> = BTreeMap::new();
    for (pass, m) in moves {
        if m.control.recordable() {
            by_control.entry(m.control).or_default().entry(*pass).or_default().push((m.beat, m.value.clamp(m.control.min(), 1.0)));
        }
    }
    let mut lanes = Vec::new();
    for (control, passes) in by_control {
        let mut points: Vec<AutomationPoint> = Vec::new();
        for values in passes.values() {
            let new = shaped(values, start, length);
            let (Some(first), Some(last)) = (new.first(), new.last()) else { continue };
            let (from, to) = (first.beat - 1e-9, last.beat + 1e-9);
            // What the earlier passes had goes on right up to the stretch moved, and from right after it.
            let old = ControlLane { control, points: std::mem::take(&mut points) };
            let (before, after) = (old.value_at(first.beat), old.value_at(last.beat));
            points = old.points.into_iter().filter(|p| p.beat < from || p.beat > to).collect();
            if let (Some(value), true) = (before, first.beat > HOLD_LEAD) {
                points.push(AutomationPoint { beat: first.beat - HOLD_LEAD, value, curve: 0.0 });
            }
            if let (Some(value), true) = (after, last.beat + HOLD_LEAD < length) {
                points.push(AutomationPoint { beat: last.beat + HOLD_LEAD, value, curve: 0.0 });
            }
            points.extend(new);
            points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        }
        if !points.is_empty() {
            lanes.push(ControlLane { control, points });
        }
    }
    lanes
}

/// Lanes of the clip over `[from, to)` of it, in the time of a clip starting at
/// `from` (`to - from` long): each with its value at `from` (and at `to`)
/// where it moves across them. A clip `length` beats long loops, so a span
/// past its end takes its lanes again.
pub fn slice(lanes: &[ControlLane], length: f64, from: f64, to: f64) -> Vec<ControlLane> {
    if length <= 0.0 || to <= from {
        return Vec::new();
    }
    let mut out = Vec::new();
    for lane in lanes {
        let mut points: Vec<AutomationPoint> = Vec::new();
        let first_lap = (from / length).floor() as i64;
        let last_lap = ((to - 1e-9) / length).floor() as i64;
        for lap in first_lap..=last_lap {
            let base = lap as f64 * length;
            let (lo, hi) = ((from - base).max(0.0), (to - base).min(length));
            if hi <= lo {
                continue;
            }
            let value = |beat: f64| lane.value_at(beat).unwrap_or(0.0);
            let shift = base - from;
            points.push(AutomationPoint { beat: lo + shift, value: value(lo), curve: 0.0 });
            for p in lane.points.iter().filter(|p| p.beat > lo + 1e-9 && p.beat < hi - 1e-9) {
                points.push(AutomationPoint { beat: p.beat + shift, ..*p });
            }
            // Where this lap's stretch ends: just before the next lap starts anew.
            let next = hi >= length - 1e-9 && lap < last_lap;
            points.push(AutomationPoint { beat: hi + shift - if next { HOLD_LEAD } else { 0.0 }, value: value(hi), curve: 0.0 });
        }
        // (A bent stretch cut in two has straight pieces at the cut.)
        points.dedup_by(|b, a| (a.beat - b.beat).abs() < 1e-9 && a.value == b.value);
        if !points.is_empty() {
            out.push(ControlLane { control: lane.control, points });
        }
    }
    out
}

/// What a take's status says of its lanes: « y el pitch bend y la rueda».
pub fn expression_note(lanes: &[ControlLane]) -> String {
    let names: Vec<String> = lanes.iter().map(|l| match l.control {
        MidiControl::PitchBend => "el pitch bend".to_string(),
        MidiControl::Pressure => "el aftertouch".to_string(),
        MidiControl::Cc(1) => "la rueda".to_string(),
        c => c.short(),
    }).collect();
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => format!(" y {last}"),
        Some((last, rest)) => format!(", {} y {last}", rest.join(", ")),
    }
}

impl crate::app::PulseApp {
    /// A take with lanes opens on its first one under the notes.
    pub fn show_recorded_expression(&mut self, lanes: &[ControlLane]) {
        if let Some(lane) = lanes.first() {
            self.piano_roll.lane.show_control(lane.control);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved(control: MidiControl, beat: f64, value: f32) -> RecordedControl {
        RecordedControl { track: 0, control, value, beat }
    }

    #[test]
    fn a_dense_sweep_is_thinned_and_keeps_its_shape() {
        // A wheel swept up and down over two beats, a value every 1/200 of a
        // beat as a controller sends them (7-bit steps).
        let moves: Vec<(usize, RecordedControl)> = (0..=400)
            .map(|i| {
                let t = i as f64 / 200.0;
                let v = ((t * std::f64::consts::PI / 2.0).sin() * 127.0).round() as f32 / 127.0;
                (0, moved(MidiControl::MOD_WHEEL, 8.0 + t, v))
            })
            .collect();
        let lanes = take_lanes(&moves, 8.0, 4.0);
        assert_eq!(lanes.len(), 1);
        let lane = &lanes[0];
        assert!(lane.points.len() < 60, "thinned: {} points", lane.points.len());
        assert!(lane.points.len() > 5, "the shape is kept: {} points", lane.points.len());
        for (_, m) in &moves {
            let at = lane.value_at(m.beat - 8.0).unwrap();
            assert!((at - m.value).abs() < 0.012, "at {}: {at} vs {}", m.beat, m.value);
        }
    }

    #[test]
    fn a_move_after_a_pause_starts_from_where_it_held() {
        let moves = [(0, moved(MidiControl::PitchBend, 1.0, 0.0)), (0, moved(MidiControl::PitchBend, 3.0, 1.0))];
        let lane = &take_lanes(&moves, 0.0, 4.0)[0];
        assert_eq!(lane.value_at(2.9).unwrap(), 0.0, "held until the move: {:?}", lane.points);
        assert_eq!(lane.value_at(3.0).unwrap(), 1.0);
        // What came before the take is its value at the start.
        let moves = [(0, moved(MidiControl::Cc(74), 3.0, 0.2)), (0, moved(MidiControl::Cc(74), 5.0, 0.9))];
        let lane = &take_lanes(&moves, 4.0, 4.0)[0];
        assert_eq!(lane.points[0].beat, 0.0);
        assert_eq!(lane.value_at(0.5).unwrap(), 0.2);
        assert_eq!(lane.value_at(1.0).unwrap(), 0.9);
    }

    #[test]
    fn a_later_pass_replaces_only_where_it_moved() {
        // Pass 0 bends over the whole loop; pass 1 only over beats 1 to 2; the
        // wheel only in pass 1.
        let mut moves: Vec<(usize, RecordedControl)> = (0..=8).map(|i| (0, moved(MidiControl::PitchBend, i as f64 * 0.5, 0.5))).collect();
        moves.extend((0..=4).map(|i| (1, moved(MidiControl::PitchBend, 1.0 + i as f64 * 0.25, -0.5))));
        moves.push((1, moved(MidiControl::MOD_WHEEL, 2.0, 0.7)));
        let lanes = take_lanes(&moves, 0.0, 4.0);
        let bend = lanes.iter().find(|l| l.control == MidiControl::PitchBend).unwrap();
        assert_eq!(bend.value_at(0.5).unwrap(), 0.5);
        assert_eq!(bend.value_at(1.5).unwrap(), -0.5, "replaced where it moved");
        assert_eq!(bend.value_at(3.0).unwrap(), 0.5, "kept after");
        assert!(lanes.iter().any(|l| l.control == MidiControl::MOD_WHEEL));
        // The sustain pedal is never a lane.
        assert!(take_lanes(&[(0, moved(MidiControl::Cc(64), 1.0, 1.0))], 0.0, 4.0).is_empty());
    }

    /// Split on the Arrangement, each part keeps the controller as it sounded there.
    #[test]
    fn a_split_clip_keeps_its_lanes_where_they_were() {
        use engine::model::{ArrangementClip, Clip, MidiClip};
        let mut midi = MidiClip::new("m", 4.0, [0; 3]);
        midi.controls.push(ControlLane { control: MidiControl::PitchBend, points: vec![AutomationPoint { beat: 0.0, value: -1.0, curve: 0.0 }, AutomationPoint { beat: 4.0, value: 1.0, curve: 0.0 }] });
        let whole = ArrangementClip { start: 8.0, length: 4.0, clip: Clip::Midi(midi) };
        let (left, right) = crate::clip_split::split(&whole, 10.0, 120.0).unwrap();
        let bend = |c: &ArrangementClip, beat: f64| match &c.clip {
            Clip::Midi(m) => m.controls[0].value_at(beat).unwrap(),
            _ => panic!(),
        };
        assert!((bend(&left, 1.0) + 0.5).abs() < 1e-4);
        assert!(bend(&right, 0.0).abs() < 1e-4, "the right part starts where the bend was");
        assert!((bend(&right, 1.0) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn slicing_keeps_the_values_at_the_cut_and_follows_the_loop() {
        let lane = ControlLane { control: MidiControl::MOD_WHEEL, points: vec![AutomationPoint { beat: 0.0, value: 0.0, curve: 0.0 }, AutomationPoint { beat: 4.0, value: 1.0, curve: 0.0 }] };
        // Beats 2..6 of a 4-beat clip: up from 0.5 to 1, then from 0 again.
        let cut = slice(&[lane], 4.0, 2.0, 6.0);
        let l = &cut[0];
        assert!((l.value_at(0.0).unwrap() - 0.5).abs() < 1e-6);
        assert!((l.value_at(1.0).unwrap() - 0.75).abs() < 1e-3);
        assert!((l.value_at(2.0).unwrap() - 0.0).abs() < 1e-6, "{:?}", l.points);
        assert!((l.value_at(3.0).unwrap() - 0.25).abs() < 1e-3);
    }
}
