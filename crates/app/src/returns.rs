//! Adding and deleting return tracks: up to `MAX_RETURNS`, as in Live. Deleting
//! one takes with it what sent to it (the tracks' sends, their automation and
//! clip envelopes, the MIDI controllers and macros on it), and the ones after it
//! move up a letter: their names too, when they start with it ("C Delay" → "B
//! Delay").

use crate::app::PulseApp;
use crate::project::{return_letter, Mixer, Project, ReturnTrack, PALETTE};
use engine::model::{AutomationLane, Clip, Param};
use engine::{Target, MAX_RETURNS};

/// A new, empty return: its letter and "Retorno".
pub fn new_return(index: usize) -> ReturnTrack {
    ReturnTrack { name: format!("{} Retorno", return_letter(index)), color: PALETTE[(index * 5 + 8) % PALETTE.len()], mixer: Mixer::default(), devices: Vec::new(), automation: Vec::new() }
}

/// Points the lanes that automate send `from` at `to` (`None`: they go).
fn move_lanes(lanes: &mut Vec<AutomationLane>, to: impl Fn(usize) -> Option<usize>) {
    lanes.retain_mut(|lane| match lane.param {
        Param::Send(i) => match to(i) {
            Some(j) => {
                lane.param = Param::Send(j);
                true
            }
            None => false,
        },
        _ => true,
    });
}

/// Takes return `index` out of `project`, and every send to it with it.
fn remove(project: &mut Project, index: usize) {
    project.returns.remove(index);
    let to = |i: usize| match i {
        i if i == index => None,
        i if i > index => Some(i - 1),
        i => Some(i),
    };
    for track in &mut project.tracks {
        track.sends.copy_within(index + 1.., index);
        track.sends[MAX_RETURNS - 1] = 0.0;
        move_lanes(&mut track.automation, to);
        let clips = track.clips.iter_mut().flatten().chain(track.arrangement.iter_mut().map(|a| &mut a.clip));
        for clip in clips {
            match clip {
                Clip::Midi(m) => move_lanes(&mut m.envelopes, to),
                Clip::Audio(a) => move_lanes(&mut a.envelopes, to),
            }
        }
    }
    relabel(project, index);
}

/// The returns from `from` on, named after their letter as it was (one
/// further on), take their new one.
fn relabel(project: &mut Project, from: usize) {
    for (i, ret) in project.returns.iter_mut().enumerate().skip(from) {
        let old = format!("{} ", return_letter(i + 1));
        if let Some(rest) = ret.name.strip_prefix(&old) {
            ret.name = format!("{} {rest}", return_letter(i));
        }
    }
}

impl PulseApp {
    /// A new return after the others (Crear ▸ Retorno).
    pub fn add_return(&mut self) {
        let index = self.project.returns.len();
        if index >= MAX_RETURNS {
            self.status = Some(format!("Máximo de {MAX_RETURNS} retornos"));
            return;
        }
        self.project.returns.push(new_return(index));
        self.selected_target = Target::Return(index);
        self.sync_all();
    }

    /// Deletes return `index`; the tracks stop sending to it.
    pub fn delete_return(&mut self, index: usize) {
        if index >= self.project.returns.len() {
            return;
        }
        remove(&mut self.project, index);
        self.retarget(|target, param| {
            let send = |param| match param {
                Param::Send(i) if i == index => None,
                Param::Send(i) if i > index => Some(Param::Send(i - 1)),
                other => Some(other),
            };
            match target {
                Target::Return(r) if r == index => None,
                Target::Return(r) if r > index => Some((Target::Return(r - 1), param)),
                Target::Track(_) => send(param).map(|p| (target, p)),
                other => Some((other, param)),
            }
        });
        self.selected_target = match self.selected_target {
            Target::Return(r) if r > index => Target::Return(r - 1),
            Target::Return(r) if r == index => match self.project.returns.len() {
                0 => Target::Master,
                n => Target::Return(r.min(n - 1)),
            },
            other => other,
        };
        self.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi_map::{Control, Mapping};

    #[test]
    fn deleting_a_return_takes_its_sends_and_moves_the_others_up() {
        let mut app = PulseApp::headless(Project::demo());
        app.add_return();
        app.add_return();
        assert_eq!(app.project.returns.len(), 4);
        assert_eq!(app.project.returns[3].name, "D Retorno");
        assert_eq!(app.selected_target, Target::Return(3));
        let t = &mut app.project.tracks[0];
        t.sends[..4].copy_from_slice(&[0.1, 0.2, 0.3, 0.4]);
        let mut lane = AutomationLane::new(Param::Send(2));
        lane.points.push(engine::model::AutomationPoint { beat: 0.0, value: 0.5, curve: 0.0 });
        t.automation.push(lane.clone());
        t.automation.push(AutomationLane { param: Param::Send(1), ..lane.clone() });
        if let Some(Clip::Midi(m)) = &mut t.clips[0] {
            m.envelopes.push(AutomationLane { param: Param::Send(3), ..lane.clone() });
        }
        app.project.midi_map.push(Mapping::new(20, Control::Param { target: Target::Track(0), param: Param::Send(2) }));
        app.project.midi_map.push(Mapping::new(21, Control::Param { target: Target::Return(1), param: Param::Pan }));
        app.project.midi_map.push(Mapping::new(22, Control::Param { target: Target::Return(3), param: Param::Pan }));
        app.selected_target = Target::Return(2);

        app.delete_return(1);
        assert_eq!(app.project.returns.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["A Reverb", "B Retorno", "C Retorno"]);
        let t = &app.project.tracks[0];
        assert_eq!(t.sends[..4], [0.1, 0.3, 0.4, 0.0]);
        assert_eq!(t.automation.iter().map(|l| l.param).collect::<Vec<_>>(), [Param::Send(1)], "B's lane went, C's is B's now");
        let Some(Clip::Midi(m)) = &t.clips[0] else { panic!() };
        assert_eq!(m.envelopes.last().map(|l| l.param), Some(Param::Send(2)));
        let controls: Vec<Control> = app.project.midi_map.iter().map(|m| m.control).collect();
        assert_eq!(controls, [Control::Param { target: Target::Track(0), param: Param::Send(1) }, Control::Param { target: Target::Return(2), param: Param::Pan }]);
        assert_eq!(app.selected_target, Target::Return(1), "the selected return, where it is now");

        // Down to none, and up to the most.
        for _ in 0..3 {
            app.delete_return(0);
        }
        assert!(app.project.returns.is_empty());
        assert_eq!(app.selected_target, Target::Master);
        for _ in 0..MAX_RETURNS + 1 {
            app.add_return();
        }
        assert_eq!(app.project.returns.len(), MAX_RETURNS);
        assert_eq!(app.project.returns[11].name, "L Retorno");
    }

    #[test]
    fn projects_with_two_returns_save_as_before() {
        let mut project = Project::demo();
        let text = project.to_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(value["format"].as_u64().unwrap() < 5);
        assert_eq!(value["tracks"][0]["sends"].as_array().unwrap().len(), 2, "what a Pulse with two returns reads");

        project.returns.push(new_return(2));
        project.tracks[0].sends[2] = 0.5;
        let text = project.to_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["format"], 5, "an older Pulse asks to be updated");
        // Fades: 6 (a Pulse that reads 5 would play the clip without them).
        let mut faded = Project::demo();
        let mut clip = engine::model::AudioClip { hard_start: false, fades: engine::model::Fades { fade_in: 1.0, fade_out: 0.0 }, name: "a".into(), color: [0; 3], sample: "a.wav".into(), length: 4.0, offset: 0.0, warp: engine::model::WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false };
        faded.tracks[0].arrangement.push(engine::model::ArrangementClip { start: 0.0, length: 4.0, clip: Clip::Audio(clip.clone()) });
        assert_eq!(serde_json::from_str::<serde_json::Value>(&faded.to_json().unwrap()).unwrap()["format"], 6);
        clip.fades = engine::model::Fades::NONE;
        faded.tracks[0].arrangement.last_mut().unwrap().clip = Clip::Audio(clip);
        assert!(serde_json::from_str::<serde_json::Value>(&faded.to_json().unwrap()).unwrap()["format"].as_u64().unwrap() < 5);
        assert_eq!(value["tracks"][0]["sends"].as_array().unwrap().len(), 3);
        let again = Project::from_json(&text).unwrap();
        assert_eq!(again.returns.len(), 3);
        assert_eq!(again.tracks[0].sends, project.tracks[0].sends);
        // One return only: also 5 (a Pulse that reads 4 needs two).
        project.returns.truncate(1);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&project.to_json().unwrap()).unwrap()["format"], 5);
        // An EQ Eight, on the master too: 7 (a Pulse that reads 6 doesn't know it).
        let mut eq = Project::demo();
        eq.master.devices.push(engine::model::Device::new(engine::model::Effect::EqEight(Default::default())));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&eq.to_json().unwrap()).unwrap()["format"], 7);
        // A Gate or a Transient, also inside a synth: 8.
        let format = |p: &Project| serde_json::from_str::<serde_json::Value>(&p.to_json().unwrap()).unwrap()["format"].clone();
        let mut gated = Project::demo();
        gated.tracks[1].devices.push(engine::model::Device::new(engine::model::Effect::Gate(Default::default())));
        assert_eq!(format(&gated), 8);
        let mut synth = Project::demo();
        let mut wave = engine::model::WaveParams::default();
        wave.fx[0] = Some(engine::model::Device::new(engine::model::Effect::Transient(Default::default())));
        synth.tracks[0].instrument = engine::model::InstrumentKind::Wavetable(wave);
        assert_eq!(format(&synth), 8);
    }

    #[test]
    fn a_third_return_is_heard() {
        use engine::model::{Device, Effect};
        let mut app = PulseApp::headless(Project::demo());
        app.add_return();
        app.project.returns[2].devices.push(Device::new(Effect::Utility(Default::default())));
        app.project.tracks[0].sends[2] = 1.0;
        app.sync_all();
        let commands = crate::sync::project_commands(&app.project, &mut crate::sync::SampleCache::default(), 48_000.0, &Default::default());
        assert!(commands.iter().any(|c| matches!(c, engine::Command::SetSends { track: 0, sends } if sends[2] == 1.0)));
        assert!(commands.iter().any(|c| matches!(c, engine::Command::ReplaceChain { target: Target::Return(2), .. })));
    }
}
