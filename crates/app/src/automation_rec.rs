//! Recording automation: while recording onto the arrangement, every control the
//! user moves (with the mouse, a linked MIDI knob, or in a plugin's window) is
//! written as automation where the playhead is. Any parameter that changed since
//! the last frame counts, so it works the same whatever moved it.
//!
//! When the take ends, each moved parameter's lane gets the recorded points,
//! replacing the ones that were in that span (like touch automation in Live),
//! and plays again from the arrangement.

use crate::app::PulseApp;
use engine::model::{AutomationPoint, Param};
use engine::{Target, NUM_RETURNS};
use std::collections::HashMap;

/// Points closer than this (in beats) replace each other: a sweep stays light.
const MIN_GAP: f64 = 1.0 / 64.0;

#[derive(Default)]
pub struct AutomationTake {
    /// Each parameter's value on the last frame.
    last: HashMap<(Target, Param), f32>,
    /// The points recorded per parameter, in time order.
    points: HashMap<(Target, Param), Vec<AutomationPoint>>,
}

impl PulseApp {
    fn all_targets(&self) -> impl Iterator<Item = Target> {
        (0..self.project.tracks.len()).map(Target::Track).chain((0..NUM_RETURNS).map(Target::Return)).chain([Target::Master])
    }

    /// Every parameter's value now.
    fn param_values(&self) -> HashMap<(Target, Param), f32> {
        self.all_targets()
            .flat_map(|target| self.automatable(target).into_iter().map(move |param| (target, param)))
            .map(|(target, param)| ((target, param), self.param_value(target, param)))
            .collect()
    }

    /// Recording starts: remembers where every control is.
    pub fn start_automation_take(&mut self) {
        self.automation_take = Some(AutomationTake { last: self.param_values(), points: HashMap::new() });
    }

    /// Once a frame while recording: writes down what moved at `beat`.
    pub fn record_automation(&mut self, beat: f64) {
        let Some(mut take) = self.automation_take.take() else { return };
        let now = self.param_values();
        for (&key, &value) in &now {
            let (target, param) = key;
            let Some(&before) = take.last.get(&key) else { continue };
            if (value - before).abs() <= f32::EPSILON * value.abs().max(1.0) {
                continue;
            }
            // A parameter its automation is playing moves by itself: that isn't the user.
            if self.automation_state(target, param) == Some(false) {
                continue;
            }
            let points = take.points.entry(key).or_default();
            if points.is_empty() {
                // Holds the old value up to the first move.
                points.push(AutomationPoint { beat: (beat - MIN_GAP).max(0.0), value: before });
            }
            // The held old value (the first point) is never overwritten.
            let several = points.len() > 1;
            match points.last_mut() {
                Some(last) if several && beat - last.beat < MIN_GAP => last.value = value,
                _ => points.push(AutomationPoint { beat, value }),
            }
        }
        take.last = now;
        self.automation_take = Some(take);
    }

    /// Recording ended: the recorded moves replace the automation in their span.
    /// Returns how many parameters got automation.
    pub fn finish_automation_take(&mut self) -> usize {
        let Some(take) = self.automation_take.take() else { return 0 };
        let mut targets = Vec::new();
        for ((target, param), mut recorded) in take.points {
            // A loop may have wrapped the playhead: keep time order.
            recorded.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            let (Some(first), Some(last)) = (recorded.first().map(|p| p.beat), recorded.last().map(|p| p.beat)) else { continue };
            let lane = self.lane_mut(target, param);
            lane.points.retain(|p| p.beat < first || p.beat > last);
            lane.points.extend(recorded);
            lane.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            // The new automation plays from now on, and its lane shows.
            self.overridden.remove(&(target, param));
            if let Target::Track(t) = target {
                self.arrangement.automation.insert(t, param);
            }
            targets.push(target);
        }
        let count = targets.len();
        targets.sort_by_key(|t| format!("{t:?}"));
        targets.dedup();
        for target in targets {
            self.sync_automation(target);
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    #[test]
    fn moves_while_recording_become_automation_replacing_their_span() {
        let mut app = PulseApp::headless(Project::demo());
        let volume = (Target::Track(1), Param::Volume);
        // Existing automation: before, inside and after the span about to be recorded.
        let lane = app.lane_mut(volume.0, volume.1);
        for (beat, value) in [(0.0, 0.2), (5.05, 0.3), (12.0, 0.4)] {
            lane.points.push(AutomationPoint { beat, value });
        }
        // It plays now; touching the knob overrides it (as set_param does).
        app.overridden.insert(volume);

        app.start_automation_take();
        app.record_automation(4.0);
        // A sweep from 4 to 8 beats.
        for step in 1..=40 {
            let beat = 4.0 + step as f64 * 0.1;
            app.project.tracks[1].mixer.gain = 0.5 + step as f32 * 0.01;
            app.record_automation(beat);
        }
        // Untouched afterwards: nothing more is written.
        app.record_automation(9.0);
        assert_eq!(app.finish_automation_take(), 1);

        let points = &app.automation(volume.0).iter().find(|l| l.param == volume.1).unwrap().points;
        let beats: Vec<f64> = points.iter().map(|p| p.beat).collect();
        assert_eq!(beats.first(), Some(&0.0), "the point before the span stays");
        assert_eq!(beats.last(), Some(&12.0), "the point after it too");
        assert!(!beats.contains(&5.05), "the one inside is replaced");
        assert!(beats.windows(2).all(|w| w[0] <= w[1]));
        // The old value holds until the first move, then the sweep.
        let inside: Vec<&AutomationPoint> = points.iter().filter(|p| p.beat > 3.5 && p.beat < 9.0).collect();
        assert!((inside[0].value - 0.2).abs() < 1e-6 || inside[0].beat < 4.1, "starts from where the knob was");
        assert!((inside.last().unwrap().value - 0.9).abs() < 1e-4, "ends where the knob stopped: {:?}", inside.last());
        assert!(inside.len() >= 10 && inside.len() <= 42, "a light sweep: {}", inside.len());
        // Its automation plays again, now with the recorded moves, and its lane shows.
        assert!(!app.overridden.contains(&volume));
        assert_eq!(app.arrangement.automation.get(&1), Some(&Param::Volume));
        assert_eq!(app.automation_state(volume.0, volume.1), Some(false));
    }

    #[test]
    fn automation_that_plays_by_itself_is_not_recorded() {
        let mut app = PulseApp::headless(Project::demo());
        app.lane_mut(Target::Track(0), Param::Pan).points.push(AutomationPoint { beat: 0.0, value: -1.0 });
        app.start_automation_take();
        // The pan changes, but its automation is playing (not overridden): not the user.
        app.project.tracks[0].mixer.pan = 0.7;
        app.record_automation(2.0);
        // A send, never automated, moved by hand.
        app.project.tracks[0].sends[0] = 0.6;
        app.record_automation(2.5);
        assert_eq!(app.finish_automation_take(), 1);
        assert_eq!(app.automation(Target::Track(0)).iter().find(|l| l.param == Param::Pan).unwrap().points.len(), 1);
        let send = app.automation(Target::Track(0)).iter().find(|l| l.param == Param::Send(0)).unwrap();
        assert_eq!(send.points.last().map(|p| (p.beat, p.value)), Some((2.5, 0.6)));
        // Nothing moved: nothing written.
        app.start_automation_take();
        app.record_automation(3.0);
        assert_eq!(app.finish_automation_take(), 0);
    }
}
