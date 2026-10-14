//! Translating a project into engine commands, for live playback and offline rendering.

use crate::project::{Project, Track};
use engine::model::{AutomationLane, Clip, InstrumentKind, Param, SynthParams};
use engine::wavetable::Wavetable;
use engine::{build_chain, Command, PlacedClip, PlayClip, Sample, Target, MAX_TRACKS, NUM_RETURNS, NUM_SCENES};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

/// Decoded audio files by path. Failed loads are cached as empty samples so they
/// are not retried every time the project is synced.
#[derive(Default)]
pub struct SampleCache {
    samples: HashMap<String, Arc<Sample>>,
    /// Wavetables by file path, or `builtin:<index>` for the generated ones.
    tables: HashMap<String, Arc<Wavetable>>,
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

    /// A built-in wavetable, generated on first use.
    pub fn builtin_table(&mut self, index: usize) -> Arc<Wavetable> {
        let index = index.min(engine::wavetable::BUILTIN.len() - 1);
        self.tables.entry(format!("builtin:{index}")).or_insert_with(|| Arc::new(engine::wavetable::builtin(index))).clone()
    }

    /// A wavetable file: frames of the size its `clm ` chunk declares (2048 by default),
    /// or the whole file as a single cycle.
    pub fn table(&mut self, path: &str) -> Arc<Wavetable> {
        if let Some(table) = self.tables.get(path) {
            return table.clone();
        }
        let sample = self.get(path);
        let frame_size = std::fs::read(path).ok().and_then(|bytes| engine::wavetable::declared_frame_size(&bytes)).unwrap_or(engine::wavetable::FRAME);
        let mono: Vec<f32> = sample.left.iter().zip(&sample.right).map(|(l, r)| (l + r) * 0.5).collect();
        let name = Path::new(path).file_stem().map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned());
        let table = if mono.is_empty() { Arc::new(engine::wavetable::builtin(0)) } else { Arc::new(Wavetable::from_samples(name, &mono, frame_size)) };
        self.tables.insert(path.to_string(), table.clone());
        table
    }

    /// Drops a file from the cache, so it is read again (after it was overwritten).
    pub fn forget(&mut self, path: &str) {
        self.samples.remove(path);
        self.tables.remove(path);
        self.tables.remove(&format!("spectral:{path}"));
    }

    /// A wavetable already in the cache (built-in key or file path).
    pub fn cached_table(&self, key: &str) -> Option<Arc<Wavetable>> {
        self.tables.get(key).cloned()
    }

    /// A file turned into a wavetable by its spectrum (the spectral oscillator).
    pub fn spectral_table(&mut self, path: &str) -> Arc<Wavetable> {
        let key = format!("spectral:{path}");
        if let Some(table) = self.tables.get(&key) {
            return table.clone();
        }
        let sample = self.get(path);
        let mono: Vec<f32> = sample.left.iter().zip(&sample.right).map(|(l, r)| (l + r) * 0.5).collect();
        let name = Path::new(path).file_stem().map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned());
        let table = Arc::new(engine::wavetable::spectral(name, &mono, sample.sample_rate.max(1.0)));
        self.tables.insert(key, table.clone());
        table
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

/// The table oscillator `osc` of a wavetable synth track reads.
pub fn table_for(track: &Track, osc: usize, samples: &mut SampleCache) -> Arc<Wavetable> {
    let InstrumentKind::Wavetable(params) = track.instrument else { return samples.builtin_table(0) };
    let o = &params.osc[osc.min(1)];
    match (o.kind, track.sample(osc)) {
        (engine::model::OscKind::Spectral, Some(path)) => samples.spectral_table(path),
        (engine::model::OscKind::Wavetable, Some(path)) if o.table == engine::model::WAVE_USER_TABLE => samples.table(path),
        _ => samples.builtin_table(o.table as usize),
    }
}

/// The table oscillator `osc` reads, if it is already loaded (for drawing).
pub fn cached_table_for(track: &Track, osc: usize, samples: &SampleCache) -> Option<Arc<Wavetable>> {
    let InstrumentKind::Wavetable(params) = track.instrument else { return None };
    let o = &params.osc[osc.min(1)];
    match (o.kind, track.sample(osc)) {
        (engine::model::OscKind::Spectral, Some(path)) => samples.cached_table(&format!("spectral:{path}")),
        (engine::model::OscKind::Wavetable, Some(path)) if o.table == engine::model::WAVE_USER_TABLE => samples.cached_table(path),
        _ => samples.cached_table(&format!("builtin:{}", (o.table as usize).min(engine::wavetable::BUILTIN.len() - 1))),
    }
}

/// Both oscillators' tables for a wavetable synth (nothing for other instruments).
pub fn instrument_tables(track: &Track, samples: &mut SampleCache) -> Box<Vec<Arc<Wavetable>>> {
    match track.instrument {
        InstrumentKind::Wavetable(_) => Box::new((0..2).map(|osc| table_for(track, osc, samples)).collect()),
        _ => Box::default(),
    }
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
            Command::set_instrument(t, track.instrument, sample_rate),
            Command::SetSamples { track: t, samples: instrument_samples(track, samples) },
            Command::SetWavetables { track: t, tables: instrument_tables(track, samples) },
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
