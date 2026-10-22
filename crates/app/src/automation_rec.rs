//! Recording automation: while recording onto the arrangement, every control the
//! user moves (with the mouse, a linked MIDI knob, or in a plugin's window) is
//! written as automation where the playhead is. Any parameter that changed since
//! the last frame counts, so it works the same whatever moved it.
//!
//! When the take ends, each moved parameter's lane gets the recorded points,
//! replacing the ones that were in that span (like touch automation in Live),
//! and plays again from the arrangement.
//!
//! Recording a clip in the session works the same for its track's controls,
//! but the moves become the clip's envelopes, in clip time (clip automation).

use crate::app::PulseApp;
use engine::model::{AutomationLane, AutomationPoint, Param};
use engine::Target;
use std::collections::HashMap;

/// Points closer than this (in beats) replace each other: a sweep stays light.
const MIN_GAP: f64 = 1.0 / 64.0;

#[derive(Default)]
pub struct AutomationTake {
    /// Only this channel's controls (a session clip's track), or all.
    only: Option<Target>,
    /// Beat 0 of the recorded points (a session clip's start).
    origin: f64,
    /// Each parameter's value on the last frame.
    last: HashMap<(Target, Param), f32>,
    /// The points recorded per parameter, in time order.
    points: HashMap<(Target, Param), Vec<AutomationPoint>>,
    /// Where the playhead was on the last frame (a loop takes it back).
    beat: Option<f64>,
    /// Per parameter, once the loop came round: the first of its points of this
    /// pass, and the beat the pass started at. What it records replaces what
    /// earlier passes left there.
    passes: HashMap<(Target, Param), (usize, f64)>,
}

/// The controls moved while recording a session clip, for its envelopes.
pub struct ClipAutomationTake {
    pub track: usize,
    pub slot: usize,
    take: AutomationTake,
}

impl PulseApp {
    fn all_targets(&self) -> impl Iterator<Item = Target> {
        (0..self.project.tracks.len()).map(Target::Track).chain((0..self.project.returns.len()).map(Target::Return)).chain([Target::Master])
    }

    /// Every parameter's value now (of one channel, or all).
    fn param_values(&self, only: Option<Target>) -> HashMap<(Target, Param), f32> {
        self.all_targets()
            .filter(|&target| only.is_none_or(|o| o == target))
            .flat_map(|target| self.automatable(target).into_iter().map(move |param| (target, param)))
            .map(|(target, param)| ((target, param), self.param_value(target, param)))
            .collect()
    }

    /// Automation Arm on or off. While recording: off keeps what was recorded so
    /// far, on starts recording the controls from here.
    pub fn toggle_automation_arm(&mut self) {
        let armed = !self.settings.automation_arm;
        let settings = crate::settings::Settings { automation_arm: armed, ..self.settings.clone() };
        self.set_settings(settings);
        if self.is_arrangement_recording() {
            if armed {
                self.start_automation_take();
            } else {
                self.finish_automation_take();
            }
        }
        self.status = Some(if armed { "Armar automatización: encendido" } else { "Armar automatización: apagado" }.into());
    }

    fn new_take(&self, only: Option<Target>, origin: f64) -> AutomationTake {
        AutomationTake { only, origin, last: self.param_values(only), ..Default::default() }
    }

    /// Recording starts: remembers where every control is.
    pub fn start_automation_take(&mut self) {
        self.automation_take = Some(self.new_take(None, 0.0));
    }

    /// Once a frame while recording: writes down what moved at `beat`.
    pub fn record_automation(&mut self, beat: f64) {
        let Some(mut take) = self.automation_take.take() else { return };
        self.record_into(&mut take, beat);
        self.automation_take = Some(take);
    }

    /// Once a frame: follows the session clips being recorded (with Automation Arm
    /// on), writing down their track's moves in clip time.
    pub fn record_clip_automation(&mut self, beat: f64, recording: &[(usize, usize, f64)]) {
        let mut takes = std::mem::take(&mut self.clip_automation);
        takes.retain(|t| recording.iter().any(|&(track, slot, _)| (track, slot) == (t.track, t.slot)) || !t.take.points.is_empty());
        for &(track, slot, start) in recording {
            match takes.iter_mut().find(|t| (t.track, t.slot) == (track, slot)) {
                Some(clip) => self.record_into(&mut clip.take, beat),
                None if self.settings.automation_arm => {
                    takes.push(ClipAutomationTake { track, slot, take: self.new_take(Some(Target::Track(track)), start) });
                }
                None => {}
            }
        }
        self.clip_automation = takes;
    }

    /// The envelopes recorded for a session clip that just ended, `length` beats long.
    pub fn take_clip_envelopes(&mut self, track: usize, slot: usize, length: f64) -> Vec<AutomationLane> {
        let Some(i) = self.clip_automation.iter().position(|t| (t.track, t.slot) == (track, slot)) else { return Vec::new() };
        let take = self.clip_automation.swap_remove(i).take;
        let mut lanes: Vec<AutomationLane> = take
            .points
            .into_iter()
            .map(|((_, param), mut points)| {
                points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
                points.retain(|p| p.beat <= length);
                AutomationLane { param, points }
            })
            .filter(|lane| !lane.points.is_empty())
            .collect();
        lanes.sort_by_key(|l| format!("{:?}", l.param));
        lanes
    }

    fn record_into(&self, take: &mut AutomationTake, beat: f64) {
        let now = self.param_values(take.only);
        let beat = beat - take.origin;
        // The loop came round: a new pass for every parameter.
        if take.beat.is_some_and(|last| beat < last - 1e-6) {
            for (key, points) in &take.points {
                take.passes.insert(*key, (points.len(), beat));
            }
        }
        take.beat = Some(beat);
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
                points.push(AutomationPoint { beat: (beat - MIN_GAP).max(0.0), value: before, curve: 0.0 });
            }
            // Another pass over the loop: up to here, it replaces the earlier ones.
            let mut first_of_pass = 0;
            if let Some((first, from)) = take.passes.get_mut(&key) {
                let earlier = *first;
                let mut kept = Vec::with_capacity(points.len());
                for (i, p) in points.drain(..).enumerate() {
                    if i < earlier && p.beat >= *from - 1e-9 && p.beat <= beat {
                        *first -= 1;
                    } else {
                        kept.push(p);
                    }
                }
                *points = kept;
                first_of_pass = *first;
            }
            // The held old value (the first point) is never overwritten, nor one
            // of an earlier pass.
            let mergeable = points.len() > 1 && points.len() - 1 >= first_of_pass.max(1);
            match points.last_mut() {
                Some(last) if mergeable && beat >= last.beat && beat - last.beat < MIN_GAP => last.value = value,
                _ => points.push(AutomationPoint { beat, value, curve: 0.0 }),
            }
        }
        take.last = now;
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
            lane.points.push(AutomationPoint { beat, value, curve: 0.0 });
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
        app.lane_mut(Target::Track(0), Param::Pan).points.push(AutomationPoint { beat: 0.0, value: -1.0, curve: 0.0 });
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

    /// Recording over a loop: the second pass replaces the first where it moved
    /// the control, and the first stays where it didn't (the second pass was
    /// lost: every point overwrote the last one).
    #[test]
    fn a_second_pass_over_the_loop_replaces_the_first_where_it_moves() {
        let mut app = PulseApp::headless(Project::demo());
        let volume = (Target::Track(1), Param::Volume);
        app.start_automation_take();
        app.record_automation(4.0);
        // Pass 1, beats 4 to 8: a rise.
        for step in 1..=40 {
            app.project.tracks[1].mixer.gain = 0.5 + step as f32 * 0.01;
            app.record_automation(4.0 + step as f64 * 0.1);
        }
        // The loop comes round to 4; pass 2 pulls it down until 6.
        for step in 0..=20 {
            app.project.tracks[1].mixer.gain = 0.3 - step as f32 * 0.005;
            app.record_automation(4.0 + step as f64 * 0.1);
        }
        assert_eq!(app.finish_automation_take(), 1);
        let lane = app.automation(volume.0).iter().find(|l| l.param == volume.1).unwrap().clone();
        assert!(lane.points.windows(2).all(|w| w[0].beat <= w[1].beat));
        let at = |beat: f64| lane.value_at(beat).unwrap();
        assert!(at(5.0) < 0.3 && at(5.9) < 0.3, "pass 2 where it moved: {} {}", at(5.0), at(5.9));
        assert!(at(7.0) > 0.7 && at(8.0) > 0.85, "pass 1 after it: {} {}", at(7.0), at(8.0));
    }
}
