//! Translating a project into engine commands, for live playback and offline rendering.

use crate::project::{Project, Track};
use engine::model::{AutomationLane, Clip, Device, Effect, InstrumentKind, Param, PluginId, SynthParams};
use engine::plugin::{PluginHandle, PluginProcessor};
use engine::wavetable::Wavetable;
use engine::{build_chain, ChainSlot, Command, PlacedClip, PlayClip, Sample, Target, MAX_TRACKS, NUM_RETURNS, NUM_SCENES};
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
    /// The UI-thread side of the plugins loaded for the project, by id.
    pub plugins: HashMap<PluginId, PluginHandle>,
    /// Plugins loaded again (a new instance) since this was last emptied.
    pub reloaded: Vec<PluginId>,
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
    if let InstrumentKind::Modular(params) = track.instrument {
        let g = &params.modules[osc.min(engine::model::MOD_MODULES - 1)];
        return match track.sample(osc) {
            Some(path) if g.table == engine::model::WAVE_USER_TABLE => samples.table(path),
            _ => samples.builtin_table(g.table as usize),
        };
    }
    let Some(params) = track.instrument.wave().copied() else { return samples.builtin_table(0) };
    let o = &params.osc[osc.min(1)];
    match (o.kind, track.sample(osc)) {
        (engine::model::OscKind::Spectral, Some(path)) => samples.spectral_table(path),
        (engine::model::OscKind::Wavetable, Some(path)) if o.table == engine::model::WAVE_USER_TABLE => samples.table(path),
        _ => samples.builtin_table(o.table as usize),
    }
}

/// The table oscillator `osc` reads, if it is already loaded (for drawing).
pub fn cached_table_for(track: &Track, osc: usize, samples: &SampleCache) -> Option<Arc<Wavetable>> {
    if let InstrumentKind::Modular(params) = track.instrument {
        let g = &params.modules[osc.min(engine::model::MOD_MODULES - 1)];
        return match track.sample(osc) {
            Some(path) if g.table == engine::model::WAVE_USER_TABLE => samples.cached_table(path),
            _ => samples.cached_table(&format!("builtin:{}", (g.table as usize).min(engine::wavetable::BUILTIN.len() - 1))),
        };
    }
    let params = track.instrument.wave().copied()?;
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
        InstrumentKind::Wavetable(_) | InstrumentKind::Polychrome(_) => Box::new((0..2).map(|osc| table_for(track, osc, samples)).collect()),
        // The modules' tables, then each modulator's (LFO Table reads one).
        InstrumentKind::Modular(params) => {
            let mut tables: Vec<Arc<Wavetable>> = (0..engine::model::MOD_MODULES).map(|g| table_for(track, g, samples)).collect();
            tables.extend(params.modulators.iter().map(|m| samples.builtin_table(if m.kind == engine::model::ModulatorKind::LfoTable { m.index as usize } else { 0 })));
            Box::new(tables)
        }
        _ => Box::default(),
    }
}

pub fn play_clip(clip: &Clip, samples: &mut SampleCache) -> PlayClip {
    match clip {
        Clip::Midi(midi) => PlayClip::Midi(midi.clone()),
        Clip::Audio(audio) => {
            let sample = samples.get(&audio.sample);
            if audio.warp == engine::model::WarpMode::Beats {
                crate::slicing::find_onsets(&sample);
            }
            PlayClip::Audio(audio.clone(), sample)
        }
    }
}

pub fn placed_clips(track: &Track, samples: &mut SampleCache) -> Box<Vec<PlacedClip>> {
    let mut placed: Vec<PlacedClip> = track.arrangement.iter().map(|a| PlacedClip::new(a.start, a.length, play_clip(&a.clip, samples))).collect();
    engine::link_neighbours(&mut placed);
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
/// Starts plugin `id` of the project from its saved state, keeping its UI half in
/// `samples.plugins`. None (with an error to show) if it cannot be loaded.
pub fn load_plugin(project: &Project, id: PluginId, samples: &mut SampleCache, sample_rate: f32) -> Option<PluginProcessor> {
    let Some(plugin) = project.plugin(id) else {
        samples.errors.push(format!("Falta el plugin {}", id.0));
        return None;
    };
    let state = crate::plugins::decode(&plugin.state);
    match engine::plugin::instantiate(Path::new(&plugin.path), &plugin.cid, sample_rate, state.as_deref().filter(|s| !s.is_empty())) {
        Ok((processor, handle)) => {
            // Restoring its state is not an edit.
            handle.take_edited();
            samples.plugins.insert(id, handle);
            samples.reloaded.push(id);
            Some(processor)
        }
        Err(error) => {
            samples.errors.push(format!("{}: {error}", plugin.name));
            None
        }
    }
}

/// The command that puts a track's instrument in place, loading it if it is a plugin.
pub fn instrument_command(project: &Project, track: usize, kind: InstrumentKind, samples: &mut SampleCache, sample_rate: f32) -> Command {
    if let InstrumentKind::Plugin(id) = kind {
        if let Some(processor) = load_plugin(project, id, samples, sample_rate) {
            return Command::set_plugin(track, id, processor);
        }
    }
    Command::set_instrument(track, kind, sample_rate)
}

/// A chain ready to run, with its plugin effects loaded.
pub fn chain(project: &Project, devices: &[Device], samples: &mut SampleCache, sample_rate: f32) -> Box<Vec<ChainSlot>> {
    let mut chain = build_chain(devices, sample_rate);
    for (slot, device) in chain.iter_mut().zip(devices) {
        if let Effect::Plugin(id) = device.effect {
            if let Some(processor) = load_plugin(project, id, samples, sample_rate) {
                *slot = ChainSlot::plugin(device, processor);
            }
        }
    }
    chain
}

pub fn project_commands(
    project: &Project,
    samples: &mut SampleCache,
    sample_rate: f32,
    overridden: &HashSet<(Target, Param)>,
) -> Vec<Command> {
    commands_from(None, project, samples, sample_rate, overridden)
}

/// The commands that bring an engine playing `before` to `project`. What didn't
/// change isn't sent again, so it goes on sounding as it was: a track's voices,
/// the tails of its effects, its plugins. Effects whose settings changed (but
/// not which effects they are) are updated in place. Tracks are matched by
/// place and id: one that moved is sent whole.
pub fn changed_commands(
    before: &Project,
    project: &Project,
    samples: &mut SampleCache,
    sample_rate: f32,
    overridden: &HashSet<(Target, Param)>,
) -> Vec<Command> {
    commands_from(Some(before), project, samples, sample_rate, overridden)
}

/// The commands for a chain of `devices` at `target`, given what it had.
fn chain_commands(project: &Project, target: Target, before: Option<&[Device]>, devices: &[Device], samples: &mut SampleCache, sample_rate: f32) -> Vec<Command> {
    let same_layout = |old: &[Device]| {
        old.len() == devices.len()
            && old.iter().zip(devices).all(|(a, b)| match (a.effect, b.effect) {
                (Effect::Plugin(x), Effect::Plugin(y)) => x == y,
                (x, y) => std::mem::discriminant(&x) == std::mem::discriminant(&y),
            })
    };
    match before {
        Some(old) if old == devices => Vec::new(),
        Some(old) if same_layout(old) => old.iter().zip(devices).enumerate().filter(|(_, (a, b))| a != b).map(|(index, (_, &device))| Command::UpdateDevice { target, index, device }).collect(),
        _ => vec![Command::ReplaceChain { target, chain: chain(project, devices, samples, sample_rate) }],
    }
}

fn commands_from(
    before: Option<&Project>,
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
        let new = project.tracks.get(t);
        // What the engine has for this place: the same track as before, if it is.
        let old = before.map(|b| b.tracks.get(t).filter(|o| new.is_some_and(|n| n.id == o.id)));
        if old.is_some() && before.is_some_and(|b| b.tracks.get(t).is_none()) && new.is_none() {
            continue;
        }
        let old = old.flatten().map(crate::freeze::playing);
        let old = old.as_deref();
        // A frozen track plays its frozen audio, through no instrument or effects.
        let track = crate::freeze::playing(new.unwrap_or(&empty));
        let track = track.as_ref();
        let target = Target::Track(t);
        if !old.is_some_and(|o| o.instrument == track.instrument && o.samples == track.samples) {
            commands.extend([
                instrument_command(project, t, track.instrument, samples, sample_rate),
                Command::SetSamples { track: t, samples: instrument_samples(track, samples) },
                Command::SetWavetables { track: t, tables: instrument_tables(track, samples) },
            ]);
        }
        commands.extend([
            Command::SetSolo { track: t, solo: track.solo },
            Command::SetArm { track: t, armed: track.armed },
            // A frozen track has no effects left to monitor through.
            Command::SetMonitor { track: t, on: track.monitors() && new.is_some_and(|t| t.frozen.is_none()) },
        ]);
        commands.extend(chain_commands(project, target, old.map(|o| o.devices.as_slice()), &track.devices, samples, sample_rate));
        if !old.is_some_and(|o| o.arrangement == track.arrangement && o.samples == track.samples) {
            commands.push(Command::SetArrangement { track: t, clips: placed_clips(track, samples) });
        }
        commands.push(Command::SetAutomation { target, lanes: active_lanes(&track.automation, target, overridden) });
        for (send, &amount) in track.sends.iter().enumerate() {
            commands.push(Command::SetSend { track: t, send, amount });
        }
        for slot in 0..NUM_SCENES {
            if old.is_some_and(|o| o.clips[slot] == track.clips[slot]) {
                continue;
            }
            let clip = track.clips[slot].as_ref().map(|c| Box::new(play_clip(c, samples)));
            commands.push(Command::SetClip { track: t, slot, clip });
        }
        commands.extend(mixer_commands(target, &track.mixer));
    }
    commands.push(Command::SetGroups(crate::groups::parents(&project.tracks)));
    // MIDI effects, with the project's key where they follow it.
    let key = project.key.map(|k| engine::model::ScaleKey { tonic: k.tonic, minor: k.minor });
    for t in 0..MAX_TRACKS {
        let effects = |p: &Project| p.tracks.get(t).map(|track| (track.id, track.midi_effects.clone()));
        if before.is_some_and(|b| effects(b) == effects(project) && b.key == project.key) {
            continue;
        }
        let mut devices = [None; engine::model::MAX_MIDI_EFFECTS];
        if let Some(track) = project.tracks.get(t) {
            for (slot, device) in devices.iter_mut().zip(&track.midi_effects) {
                *slot = Some(engine::model::MidiDevice { effect: device.effect.with_project_key(key), ..*device });
            }
        }
        commands.push(Command::SetMidiEffects { track: t, devices });
    }
    commands.push(Command::SetTrackIds(std::array::from_fn(|t| project.tracks.get(t).map_or(0, |track| track.id))));
    for (r, ret) in project.returns.iter().enumerate().take(NUM_RETURNS) {
        let target = Target::Return(r);
        let old = before.and_then(|b| b.returns.get(r)).map(|o| o.devices.as_slice());
        commands.extend(chain_commands(project, target, old, &ret.devices, samples, sample_rate));
        commands.push(Command::SetAutomation { target, lanes: active_lanes(&ret.automation, target, overridden) });
        commands.extend(mixer_commands(target, &ret.mixer));
    }
    let master = &project.master;
    let old = before.map(|b| b.master.devices.as_slice());
    commands.extend(chain_commands(project, Target::Master, old, &master.devices, samples, sample_rate));
    commands.push(Command::SetAutomation {
        target: Target::Master,
        lanes: active_lanes(&master.automation, Target::Master, overridden),
    });
    commands.extend(mixer_commands(Target::Master, &master.mixer));
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::model::ArrangementClip;

    fn heavy(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|c| match c {
                Command::SetInstrument { track, .. } => Some(format!("instrumento {track}")),
                Command::ReplaceChain { target, .. } => Some(format!("cadena {target:?}")),
                Command::UpdateDevice { target, index, .. } => Some(format!("efecto {target:?} {index}")),
                Command::SetArrangement { track, .. } => Some(format!("arreglo {track}")),
                Command::SetClip { track, slot, .. } => Some(format!("clip {track} {slot}")),
                _ => None,
            })
            .collect()
    }

    /// From a project the engine plays, only what changed is sent again, and all
    /// of what changed is.
    #[test]
    fn only_what_changed_is_sent_again() {
        let mut before = Project::demo();
        before.assign_ids();
        let mut samples = SampleCache::default();
        let unchanged = changed_commands(&before, &before, &mut samples, 48_000.0, &HashSet::new());
        assert!(heavy(&unchanged).is_empty(), "{:?}", heavy(&unchanged));
        // A clip moved on one track, one effect's setting on another.
        let mut after = before.clone();
        let clip = after.tracks[1].clips[0].clone().unwrap();
        after.tracks[1].arrangement.push(ArrangementClip { start: 8.0, length: 4.0, clip });
        let fx = after.tracks.iter().position(|t| !t.devices.is_empty()).unwrap();
        after.tracks[fx].devices[0].enabled = !after.tracks[fx].devices[0].enabled;
        let mut expected = vec!["arreglo 1".to_string(), format!("efecto Track({fx}) 0")];
        expected.sort();
        let mut got = heavy(&changed_commands(&before, &after, &mut samples, 48_000.0, &HashSet::new()));
        got.sort();
        assert_eq!(got, expected);
        // Another effect in the chain: the chain is built again.
        let mut more = before.clone();
        more.tracks[0].devices.push(Device::new(Effect::Saturator(Default::default())));
        assert_eq!(heavy(&changed_commands(&before, &more, &mut samples, 48_000.0, &HashSet::new())), ["cadena Track(0)"]);
        // A track in front of the rest: every one moved, every one goes again.
        let mut moved = before.clone();
        moved.tracks.insert(0, Track::new("Nueva", [0; 3], InstrumentKind::Synth(SynthParams::default())));
        moved.assign_ids();
        let sent = heavy(&changed_commands(&before, &moved, &mut samples, 48_000.0, &HashSet::new()));
        assert!((0..moved.tracks.len()).all(|t| sent.contains(&format!("instrumento {t}"))), "{sent:?}");
    }
}
