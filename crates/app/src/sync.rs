//! Translating a project into engine commands, for live playback and offline rendering.

use crate::project::{Project, Track};
use engine::model::{AutomationLane, Clip, InstrumentKind, Param, SynthParams};
use engine::{build_chain, Command, PlacedClip, PlayClip, Sample, Target, MAX_TRACKS, NUM_RETURNS, NUM_SCENES};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

/// Decoded audio files by path. Failed loads are cached as empty samples so they
/// are not retried every time the project is synced.
#[derive(Default)]
pub struct SampleCache {
    samples: HashMap<String, Arc<Sample>>,
    /// Load errors not yet shown to the user.
    pub errors: Vec<String>,
}

impl SampleCache {
    pub fn get(&mut self, path: &str) -> Arc<Sample> {
        if let Some(sample) = self.samples.get(path) {
            return sample.clone();
        }
        let sample = match Sample::load(Path::new(path)) {
            Ok(sample) => Arc::new(sample),
            Err(error) => {
                self.errors.push(format!("No se pudo cargar la muestra: {error}"));
                Arc::new(Sample::default())
            }
        };
        self.samples.insert(path.to_string(), sample.clone());
        sample
    }

    pub fn loaded(&self, path: &str) -> Option<&Arc<Sample>> {
        self.samples.get(path)
    }

    pub fn insert(&mut self, path: String, sample: Arc<Sample>) {
        self.samples.insert(path, sample);
    }
}

/// The samples a track's sampler or drum rack plays, decoded.
pub fn instrument_samples(track: &Track, samples: &mut SampleCache) -> Box<Vec<Arc<Sample>>> {
    let decoded = (0..track.samples.len())
        .map(|slot| match track.sample(slot) {
            Some(path) => samples.get(path),
            None => Arc::new(Sample::default()),
        })
        .collect();
    Box::new(decoded)
}

pub fn play_clip(clip: &Clip, samples: &mut SampleCache) -> PlayClip {
    match clip {
        Clip::Midi(midi) => PlayClip::Midi(midi.clone()),
        Clip::Audio(audio) => PlayClip::Audio(audio.clone(), samples.get(&audio.sample)),
    }
}

pub fn placed_clips(track: &Track, samples: &mut SampleCache) -> Box<Vec<PlacedClip>> {
    let placed = track
        .arrangement
        .iter()
        .map(|a| PlacedClip { start: a.start, length: a.length, clip: play_clip(&a.clip, samples) })
        .collect();
    Box::new(placed)
}

/// Lanes with points that are not overridden by hand.
pub fn active_lanes(lanes: &[AutomationLane], target: Target, overridden: &HashSet<(Target, Param)>) -> Box<Vec<AutomationLane>> {
    let active = lanes
        .iter()
        .filter(|lane| !lane.points.is_empty() && !overridden.contains(&(target, lane.param)))
        .cloned()
        .collect();
    Box::new(active)
}

fn mixer_commands(target: Target, mixer: &crate::project::Mixer) -> [Command; 3] {
    [
        Command::SetGain { target, gain: mixer.gain },
        Command::SetPan { target, pan: mixer.pan },
        Command::SetMute { target, mute: !mixer.active },
    ]
}

/// Every command needed to make an engine play `project` from scratch.
pub fn project_commands(
    project: &Project,
    samples: &mut SampleCache,
    sample_rate: f32,
    overridden: &HashSet<(Target, Param)>,
) -> Vec<Command> {
    let mut commands = vec![
        Command::SetTempo(project.tempo),
        Command::SetMetronome(project.metronome),
        Command::SetQuantize(project.quantize),
        Command::SetLoop { enabled: project.loop_enabled, start: project.loop_start, end: project.loop_end },
        Command::SetSwing { amount: project.swing, grid: project.swing_grid },
    ];
    let empty = Track::new("", [0, 0, 0], InstrumentKind::Synth(SynthParams::default()));
    for t in 0..MAX_TRACKS {
        let track = project.tracks.get(t).unwrap_or(&empty);
        let target = Target::Track(t);
        commands.extend([
            Command::SetInstrument { track: t, kind: track.instrument },
            Command::SetSamples { track: t, samples: instrument_samples(track, samples) },
            Command::SetSolo { track: t, solo: track.solo },
            Command::SetArm { track: t, armed: track.armed },
            Command::ReplaceChain { target, chain: build_chain(&track.devices, sample_rate) },
            Command::SetArrangement { track: t, clips: placed_clips(track, samples) },
            Command::SetAutomation { target, lanes: active_lanes(&track.automation, target, overridden) },
        ]);
        for (send, &amount) in track.sends.iter().enumerate() {
            commands.push(Command::SetSend { track: t, send, amount });
        }
        for slot in 0..NUM_SCENES {
            let clip = track.clips[slot].as_ref().map(|c| Box::new(play_clip(c, samples)));
            commands.push(Command::SetClip { track: t, slot, clip });
        }
        commands.extend(mixer_commands(target, &track.mixer));
    }
    for (r, ret) in project.returns.iter().enumerate().take(NUM_RETURNS) {
        let target = Target::Return(r);
        commands.push(Command::ReplaceChain { target, chain: build_chain(&ret.devices, sample_rate) });
        commands.push(Command::SetAutomation { target, lanes: active_lanes(&ret.automation, target, overridden) });
        commands.extend(mixer_commands(target, &ret.mixer));
    }
    let master = &project.master;
    commands.push(Command::ReplaceChain { target: Target::Master, chain: build_chain(&master.devices, sample_rate) });
    commands.push(Command::SetAutomation {
        target: Target::Master,
        lanes: active_lanes(&master.automation, Target::Master, overridden),
    });
    commands.extend(mixer_commands(Target::Master, &master.mixer));
    commands
}
