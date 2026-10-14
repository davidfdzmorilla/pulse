use crate::history::History;
use crate::project::{Mixer, Project, Track, PALETTE};
use std::collections::HashSet;
use std::path::Path;
use crate::capture::{write_wav, InputCapture};
use crate::input::{ComputerKeyboard, MidiInputs};
use crate::recording::{Finished, Recorder};
use crate::sync::{self, SampleCache};
use crate::{arrangement, browser, detail, piano_roll, session, theme, transport};
use eframe::egui;
use engine::model::{
    ArrangementClip, AudioClip, AutomationLane, Clip, Device, InstrumentKind, MidiClip, Param, ParamInfo, SynthParams, WarpMode,
    PAN_PARAM, RACK_PADS, SEND_PARAM, VOLUME_PARAM,
};
use engine::{
    build_chain, guess_loop_bpm, AudioOutput, ChainSlot, Command, EngineHandle, LiveInput,
    RecordedNote, Sample, SharedState,
    Target, MAX_TRACKS, NUM_RETURNS, NUM_SCENES,
};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Wraps the optional audio connection so the UI keeps working without a device.
pub struct Audio {
    _output: Option<AudioOutput>,
    handle: Option<EngineHandle>,
    pub error: Option<String>,
}

impl Audio {
    fn start() -> Self {
        match engine::start() {
            Ok((output, handle)) => Self {
                _output: Some(output),
                handle: Some(handle),
                error: None,
            },
            Err(error) => Self {
                _output: None,
                handle: None,
                error: Some(error),
            },
        }
    }

    pub fn send(&mut self, command: Command) {
        if let Some(handle) = &mut self.handle {
            if !handle.send(command) {
                eprintln!("engine command queue full; command dropped");
            }
        }
    }

    pub fn state(&self) -> Option<Arc<SharedState>> {
        self.handle.as_ref().map(|h| h.state.clone())
    }

    pub fn sample_rate(&self) -> f32 {
        self.handle.as_ref().map_or(48_000.0, |h| h.sample_rate)
    }

    pub fn is_playing(&self) -> bool {
        self.handle.as_ref().is_some_and(|h| h.state.playing.load(Ordering::Relaxed))
    }

    pub fn position(&self) -> f64 {
        self.handle.as_ref().map_or(0.0, |h| h.state.position())
    }

    pub fn live(&self) -> Option<LiveInput> {
        self.handle.as_ref().map(|h| h.live.clone())
    }

    fn take_recorded(&mut self) -> Vec<RecordedNote> {
        self.handle.as_mut().map_or_else(Vec::new, |h| h.take_recorded())
    }

    fn collect_garbage(&mut self) {
        if let Some(handle) = &mut self.handle {
            handle.collect_garbage();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Session,
    Arrangement,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    Clip,
    Devices,
}

/// A clip the editor can open: a session slot or a clip on the arrangement timeline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipRef {
    Session { track: usize, slot: usize },
    Arrangement { track: usize, index: usize },
}

impl ClipRef {
    pub fn track(self) -> usize {
        match self {
            ClipRef::Session { track, .. } | ClipRef::Arrangement { track, .. } => track,
        }
    }
}

/// File extensions offered when choosing samples.
pub const AUDIO_EXTENSIONS: [&str; 9] = ["wav", "aif", "aiff", "flac", "mp3", "ogg", "m4a", "caf", "wave"];

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Where newly added samples go.
#[derive(Clone, Copy)]
pub enum Placement {
    /// The selected audio track (or a new one), in the current view.
    Auto,
    Session { track: usize, slot: usize },
    Arrangement { track: usize, start: f64 },
}

#[derive(Default)]
pub struct Meters {
    pub tracks: [f32; MAX_TRACKS],
    pub returns: [f32; NUM_RETURNS],
    pub master: f32,
}

impl Meters {
    fn update(&mut self, state: &SharedState) {
        let fall = |meter: &mut f32, peak: f32| *meter = peak.max(*meter * 0.86);
        for (meter, status) in self.tracks.iter_mut().zip(&state.tracks) {
            fall(meter, status.peak.take());
        }
        for (meter, peak) in self.returns.iter_mut().zip(&state.returns) {
            fall(meter, peak.take());
        }
        fall(&mut self.master, state.master.take());
    }

    pub fn of(&self, target: Target) -> f32 {
        match target {
            Target::Track(i) => self.tracks[i],
            Target::Return(i) => self.returns[i],
            Target::Master => self.master,
        }
    }
}

pub struct PulseApp {
    pub project: Project,
    pub audio: Audio,
    pub view: View,
    pub detail: Detail,
    pub selected_clip: Option<ClipRef>,
    pub selected_target: Target,
    pub meters: Meters,
    pub piano_roll: piano_roll::State,
    pub arrangement: arrangement::State,
    pub show_browser: bool,
    /// Decoded audio files by path.
    pub(crate) samples: SampleCache,
    pub midi: MidiInputs,
    pub keyboard: ComputerKeyboard,
    pub recorder: Recorder,
    /// Audio input, opened when an audio track is armed.
    pub capture: Option<InputCapture>,
    pub export: crate::export::Dialog,
    /// Automated parameters the user moved by hand; their automation is
    /// suspended until "Back to Arrangement", as in Live.
    pub overridden: HashSet<(Target, Param)>,
    /// Drum rack pad shown in the device view.
    pub selected_pad: usize,
    /// How loops are cut into drum rack pads.
    pub slice_mode: crate::slicing::SliceMode,
    /// Sample slots drawn this frame, where dropped files can land: (area, track, slot).
    pub sample_drop_zones: Vec<(egui::Rect, usize, usize)>,
    /// Input track last sent to the engine.
    input_track: Option<Option<usize>>,
    history: History,
    file: Option<PathBuf>,
    saved: Project,
    pub status: Option<String>,
    title: String,
    /// Debug aid: with `PULSE_SCREENSHOT=<file.ppm>` the app saves a screenshot of
    /// its own window after a few seconds and quits. `PULSE_VIEW=arrangement`
    /// opens the arrangement view and `PULSE_PLAY=1` starts playback.
    screenshot: Option<(String, u32)>,
}

impl PulseApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let project = Project::demo();
        let mut app = Self {
            history: History::new(&project),
            saved: project.clone(),
            project,
            audio: Audio::start(),
            view: View::Session,
            detail: Detail::Devices,
            selected_clip: Some(ClipRef::Session { track: 0, slot: 0 }),
            selected_target: Target::Track(0),
            meters: Meters::default(),
            piano_roll: Default::default(),
            arrangement: Default::default(),
            show_browser: true,
            samples: SampleCache::default(),
            midi: MidiInputs::default(),
            keyboard: ComputerKeyboard::default(),
            recorder: Recorder::default(),
            capture: None,
            overridden: HashSet::new(),
            selected_pad: 0,
            slice_mode: crate::slicing::SliceMode::Transients,
            sample_drop_zones: Vec::new(),
            export: Default::default(),
            input_track: None,
            file: None,
            status: None,
            title: String::new(),
            screenshot: std::env::var("PULSE_SCREENSHOT").ok().map(|path| (path, 0)),
        };
        app.sync_all();
        app.midi = MidiInputs::connect(app.audio.live());
        if std::env::var("PULSE_VIEW").as_deref() == Ok("arrangement") {
            app.view = View::Arrangement;
        }
        if std::env::var("PULSE_DETAIL").as_deref() == Ok("clip") {
            app.detail = Detail::Clip;
        }
        if std::env::var("PULSE_PLAY").is_ok() {
            match app.view {
                View::Session => app.audio.send(Command::LaunchScene(1)),
                View::Arrangement => app.audio.send(Command::Play),
            }
        }
        app
    }

    // ----- Engine synchronisation -------------------------------------------------

    /// Pushes the whole project to the engine (after loading or undo).
    pub fn sync_all(&mut self) {
        let sample_rate = self.audio.sample_rate();
        let commands = sync::project_commands(&self.project, &mut self.samples, sample_rate, &self.overridden);
        for command in commands {
            self.audio.send(command);
        }
        self.show_sample_errors();
    }

    fn show_sample_errors(&mut self) {
        if let Some(error) = self.samples.errors.pop() {
            self.status = Some(error);
            self.samples.errors.clear();
        }
    }

    /// Sends a channel's lanes that have points and are not overridden.
    pub fn sync_automation(&mut self, target: Target) {
        let lanes = sync::active_lanes(self.automation(target), target, &self.overridden);
        self.audio.send(Command::SetAutomation { target, lanes });
    }

    pub fn sync_mixer(&mut self, target: Target) {
        let mixer = self.mixer(target).clone();
        self.audio.send(Command::SetGain { target, gain: mixer.gain });
        self.audio.send(Command::SetPan { target, pan: mixer.pan });
        self.audio.send(Command::SetMute { target, mute: !mixer.active });
    }

    pub fn sync_chain(&mut self, target: Target) {
        let chain = build_chain(self.devices(target), self.audio.sample_rate());
        self.audio.send(Command::ReplaceChain { target, chain });
    }

    pub fn sync_clip(&mut self, track: usize, slot: usize) {
        let clip = self.project.tracks[track].clips[slot].clone();
        let clip = clip.map(|c| Box::new(sync::play_clip(&c, &mut self.samples)));
        self.audio.send(Command::SetClip { track, slot, clip });
        self.show_sample_errors();
    }

    pub fn sync_arrangement(&mut self, track: usize) {
        let clips = sync::placed_clips(&self.project.tracks[track], &mut self.samples);
        self.audio.send(Command::SetArrangement { track, clips });
        self.show_sample_errors();
    }

    // ----- Samples ----------------------------------------------------------------

    /// Returns the decoded file, loading it on first use.
    pub fn sample(&mut self, path: &str) -> Arc<Sample> {
        let sample = self.samples.get(path);
        self.show_sample_errors();
        sample
    }

    /// The decoded file if it is already loaded (for drawing).
    pub fn loaded_sample(&self, path: &str) -> Option<&Arc<Sample>> {
        self.samples.loaded(path)
    }

    /// Builds a clip for a file, guessing whether it is a tempo-synced loop or a one-shot.
    fn new_audio_clip(&self, path: &Path, sample: &Sample, color: [u8; 3]) -> AudioClip {
        let duration = sample.duration();
        let tempo = self.project.tempo;
        let name = path.file_stem().map_or("Audio".into(), |s| s.to_string_lossy().into_owned());
        let (warp, original_bpm, length) = match guess_loop_bpm(duration, tempo) {
            Some(bpm) => (WarpMode::Stretch, bpm, (duration * bpm / 60.0).round().max(1.0)),
            None => (WarpMode::Off, tempo, ((duration * tempo / 60.0) * 4.0).ceil().max(1.0) / 4.0),
        };
        AudioClip {
            name,
            color,
            sample: path.to_string_lossy().into_owned(),
            length,
            offset: 0.0,
            warp,
            original_bpm,
            gain_db: 0.0,
            transpose: 0.0,
        }
    }

    pub fn choose_samples(&mut self, placement: Placement) {
        let dialog = rfd::FileDialog::new().add_filter("Audio", &AUDIO_EXTENSIONS);
        if let Some(paths) = dialog.pick_files() {
            self.add_samples(paths, placement);
        }
    }

    /// Adds audio files as clips: consecutive session slots or back to back on the timeline.
    pub fn add_samples(&mut self, paths: Vec<PathBuf>, placement: Placement) {
        let mut placement = match placement {
            Placement::Auto => {
                let track = match self.selected_target {
                    Target::Track(t) if self.project.tracks[t].is_audio() => Some(t),
                    _ => self.add_audio_track(),
                };
                let Some(track) = track else { return };
                match self.view {
                    View::Session => {
                        let free = (0..NUM_SCENES).find(|&s| self.project.tracks[track].clips[s].is_none());
                        let Some(slot) = free else {
                            self.status = Some("No quedan slots libres en esa pista".into());
                            return;
                        };
                        Placement::Session { track, slot }
                    }
                    View::Arrangement => {
                        let start = (self.project.tracks[track].arrangement_end() / 4.0).ceil() * 4.0;
                        Placement::Arrangement { track, start }
                    }
                }
            }
            explicit => explicit,
        };

        let mut added = 0;
        for path in paths {
            let sample = self.sample(&path.to_string_lossy());
            if sample.frames() == 0 {
                continue;
            }
            match placement {
                Placement::Session { track, slot } => {
                    if slot >= NUM_SCENES {
                        break;
                    }
                    let clip = self.new_audio_clip(&path, &sample, self.project.tracks[track].color);
                    self.set_clip(track, slot, Some(Clip::Audio(clip)));
                    self.select_clip(ClipRef::Session { track, slot });
                    let next = (slot + 1..NUM_SCENES).find(|&s| self.project.tracks[track].clips[s].is_none());
                    placement = Placement::Session { track, slot: next.unwrap_or(NUM_SCENES) };
                }
                Placement::Arrangement { track, start } => {
                    let clip = self.new_audio_clip(&path, &sample, self.project.tracks[track].color);
                    let length = clip.length;
                    let lane = &mut self.project.tracks[track].arrangement;
                    lane.push(engine::model::ArrangementClip { start, length, clip: Clip::Audio(clip) });
                    let index = lane.len() - 1;
                    self.sync_arrangement(track);
                    self.select_clip(ClipRef::Arrangement { track, index });
                    placement = Placement::Arrangement { track, start: start + length };
                }
                Placement::Auto => unreachable!("resolved above"),
            }
            added += 1;
        }
        if added > 0 {
            self.status = Some(format!("{added} muestra(s) añadida(s)"));
        }
    }

    pub fn sync_clip_ref(&mut self, clip: ClipRef) {
        match clip {
            ClipRef::Session { track, slot } => self.sync_clip(track, slot),
            ClipRef::Arrangement { track, .. } => self.sync_arrangement(track),
        }
    }

    pub fn sync_loop(&mut self) {
        let p = &self.project;
        let command = Command::SetLoop {
            enabled: p.loop_enabled,
            start: p.loop_start,
            end: p.loop_end,
        };
        self.audio.send(command);
    }

    // ----- Model access -----------------------------------------------------------

    pub fn mixer(&self, target: Target) -> &Mixer {
        match target {
            Target::Track(i) => &self.project.tracks[i].mixer,
            Target::Return(i) => &self.project.returns[i].mixer,
            Target::Master => &self.project.master.mixer,
        }
    }

    pub fn mixer_mut(&mut self, target: Target) -> &mut Mixer {
        match target {
            Target::Track(i) => &mut self.project.tracks[i].mixer,
            Target::Return(i) => &mut self.project.returns[i].mixer,
            Target::Master => &mut self.project.master.mixer,
        }
    }

    pub fn devices(&self, target: Target) -> &Vec<Device> {
        match target {
            Target::Track(i) => &self.project.tracks[i].devices,
            Target::Return(i) => &self.project.returns[i].devices,
            Target::Master => &self.project.master.devices,
        }
    }

    pub fn devices_mut(&mut self, target: Target) -> &mut Vec<Device> {
        match target {
            Target::Track(i) => &mut self.project.tracks[i].devices,
            Target::Return(i) => &mut self.project.returns[i].devices,
            Target::Master => &mut self.project.master.devices,
        }
    }

    pub fn target_name(&self, target: Target) -> String {
        match target {
            Target::Track(i) => self.project.tracks[i].name.clone(),
            Target::Return(i) => self.project.returns[i].name.clone(),
            Target::Master => "Master".into(),
        }
    }

    pub fn target_color(&self, target: Target) -> egui::Color32 {
        match target {
            Target::Track(i) => theme::rgb(self.project.tracks[i].color),
            Target::Return(i) => theme::rgb(self.project.returns[i].color),
            Target::Master => theme::CONTROL_HOVER,
        }
    }

    pub fn clip(&self, clip: ClipRef) -> Option<&Clip> {
        match clip {
            ClipRef::Session { track, slot } => self.project.tracks.get(track)?.clips[slot].as_ref(),
            ClipRef::Arrangement { track, index } => {
                self.project.tracks.get(track)?.arrangement.get(index).map(|a| &a.clip)
            }
        }
    }

    // ----- Parameters and automation ----------------------------------------------

    pub fn automation(&self, target: Target) -> &Vec<AutomationLane> {
        match target {
            Target::Track(i) => &self.project.tracks[i].automation,
            Target::Return(i) => &self.project.returns[i].automation,
            Target::Master => &self.project.master.automation,
        }
    }

    pub fn automation_mut(&mut self, target: Target) -> &mut Vec<AutomationLane> {
        match target {
            Target::Track(i) => &mut self.project.tracks[i].automation,
            Target::Return(i) => &mut self.project.returns[i].automation,
            Target::Master => &mut self.project.master.automation,
        }
    }

    /// The lane for `param`, created empty if needed.
    pub fn lane_mut(&mut self, target: Target, param: Param) -> &mut AutomationLane {
        let lanes = self.automation_mut(target);
        let index = match lanes.iter().position(|l| l.param == param) {
            Some(index) => index,
            None => {
                lanes.push(AutomationLane::new(param));
                lanes.len() - 1
            }
        };
        &mut lanes[index]
    }

    pub fn param_info(&self, target: Target, param: Param) -> Option<ParamInfo> {
        match param {
            Param::Volume => Some(VOLUME_PARAM),
            Param::Pan => Some(PAN_PARAM),
            Param::Send(_) => Some(SEND_PARAM),
            Param::Device { device, param } => self.devices(target).get(device)?.effect.params().get(param).copied(),
            Param::Instrument(i) => match target {
                Target::Track(t) if !self.project.tracks[t].is_audio() => self.project.tracks[t].instrument.params().get(i).copied(),
                _ => None,
            },
        }
    }

    pub fn param_name(&self, target: Target, param: Param) -> String {
        let info = self.param_info(target, param).map_or("?", |i| i.name);
        match param {
            Param::Volume => "Volumen".into(),
            Param::Pan => "Pan".into(),
            Param::Send(i) => format!("Envío {}", ["A", "B"].get(i).unwrap_or(&"?")),
            Param::Device { device, .. } => {
                let label = self.devices(target).get(device).map_or("?", |d| d.effect.label());
                format!("{} {label}: {info}", device + 1)
            }
            Param::Instrument(_) => match target {
                Target::Track(t) => format!("{}: {info}", self.project.tracks[t].instrument.label()),
                _ => info.to_string(),
            },
        }
    }

    /// Every parameter of a channel that can be automated.
    pub fn automatable(&self, target: Target) -> Vec<Param> {
        let mut params = vec![Param::Volume, Param::Pan];
        if let Target::Track(t) = target {
            params.extend((0..NUM_RETURNS).map(Param::Send));
            if !self.project.tracks[t].is_audio() {
                params.extend((0..self.project.tracks[t].instrument.params().len()).map(Param::Instrument));
            }
        }
        for (device, d) in self.devices(target).iter().enumerate() {
            params.extend((0..d.effect.params().len()).map(|param| Param::Device { device, param }));
        }
        params
    }

    /// The value set by hand (what plays where there is no automation).
    pub fn param_value(&self, target: Target, param: Param) -> f32 {
        let mixer = self.mixer(target);
        match param {
            Param::Volume => mixer.gain,
            Param::Pan => mixer.pan,
            Param::Send(i) => match target {
                Target::Track(t) => self.project.tracks[t].sends.get(i).copied().unwrap_or(0.0),
                _ => 0.0,
            },
            Param::Device { device, param } => self.devices(target).get(device).map_or(0.0, |d| d.effect.get(param)),
            Param::Instrument(i) => match target {
                Target::Track(t) => self.project.tracks[t].instrument.get(i),
                _ => 0.0,
            },
        }
    }

    /// Whether `param` has automation, and whether it is currently overridden.
    pub fn automation_state(&self, target: Target, param: Param) -> Option<bool> {
        let has_points = self.automation(target).iter().any(|l| l.param == param && !l.points.is_empty());
        has_points.then(|| self.overridden.contains(&(target, param)))
    }

    /// Value to display: the automated one while the transport plays, else the manual one.
    pub fn display_value(&self, target: Target, param: Param) -> f32 {
        if self.audio.is_playing() && self.automation_state(target, param) == Some(false) {
            let position = self.audio.position();
            if let Some(value) = self.automation(target).iter().find(|l| l.param == param).and_then(|l| l.value_at(position)) {
                return value;
            }
        }
        self.param_value(target, param)
    }

    /// Sets a parameter by hand. Moving an automated parameter overrides its automation.
    pub fn set_param(&mut self, target: Target, param: Param, value: f32) {
        match param {
            Param::Volume => self.mixer_mut(target).gain = value.max(0.0),
            Param::Pan => self.mixer_mut(target).pan = value.clamp(-1.0, 1.0),
            Param::Send(i) => {
                if let Target::Track(t) = target {
                    if let Some(send) = self.project.tracks[t].sends.get_mut(i) {
                        *send = value.clamp(0.0, 1.0);
                    }
                    self.audio.send(Command::SetSend { track: t, send: i, amount: value.clamp(0.0, 1.0) });
                }
            }
            Param::Device { device, param } => {
                if let Some(d) = self.devices_mut(target).get_mut(device) {
                    d.effect.set(param, value);
                }
                self.update_device(target, device);
            }
            Param::Instrument(i) => {
                if let Target::Track(t) = target {
                    let instrument = &mut self.project.tracks[t].instrument;
                    instrument.set(i, value);
                    let kind = *instrument;
                    self.audio.send(Command::SetInstrument { track: t, kind });
                }
            }
        }
        if matches!(param, Param::Volume | Param::Pan) {
            self.sync_mixer(target);
        }
        if self.automation_state(target, param) == Some(false) && self.overridden.insert((target, param)) {
            self.sync_automation(target);
        }
    }

    // ----- Edits ------------------------------------------------------------------

    pub fn select_clip(&mut self, clip: ClipRef) {
        self.selected_clip = Some(clip);
        self.selected_target = Target::Track(clip.track());
        self.detail = Detail::Clip;
    }

    pub fn set_clip(&mut self, track: usize, slot: usize, clip: Option<Clip>) {
        self.project.tracks[track].clips[slot] = clip;
        if self.selected_clip == Some(ClipRef::Session { track, slot }) && self.project.tracks[track].clips[slot].is_none() {
            self.selected_clip = None;
        }
        self.sync_clip(track, slot);
    }

    pub fn delete_selected_clip(&mut self) {
        match self.selected_clip {
            Some(ClipRef::Session { track, slot }) => self.set_clip(track, slot, None),
            Some(ClipRef::Arrangement { track, index }) => {
                let lane = &mut self.project.tracks[track].arrangement;
                if index < lane.len() {
                    lane.remove(index);
                    self.selected_clip = None;
                    self.sync_arrangement(track);
                }
            }
            None => {}
        }
    }

    pub fn duplicate_selected_clip(&mut self) {
        match self.selected_clip {
            Some(ClipRef::Session { track, slot }) => {
                let free = (slot + 1..NUM_SCENES).find(|&s| self.project.tracks[track].clips[s].is_none());
                if let Some(target) = free {
                    let copy = self.project.tracks[track].clips[slot].clone();
                    self.set_clip(track, target, copy);
                    self.select_clip(ClipRef::Session { track, slot: target });
                }
            }
            Some(ClipRef::Arrangement { track, index }) => {
                let lane = &mut self.project.tracks[track].arrangement;
                if let Some(original) = lane.get(index).cloned() {
                    let mut copy = original.clone();
                    copy.start = original.end();
                    lane.push(copy);
                    let index = lane.len() - 1;
                    self.sync_arrangement(track);
                    self.select_clip(ClipRef::Arrangement { track, index });
                }
            }
            None => {}
        }
    }

    /// Places a copy of a session clip at the end of the track's arrangement.
    pub fn copy_to_arrangement(&mut self, track: usize, slot: usize) {
        let t = &mut self.project.tracks[track];
        let Some(clip) = t.clips[slot].clone() else { return };
        let start = (t.arrangement_end() / 4.0).ceil() * 4.0;
        let length = clip.length().max(4.0);
        t.arrangement.push(engine::model::ArrangementClip { start, length, clip });
        self.sync_arrangement(track);
        self.status = Some(format!("Clip copiado al Arrangement en el compás {}", start as i64 / 4 + 1));
    }

    pub fn add_device(&mut self, target: Target, device: Device) {
        let index = self.devices(target).len();
        let slot = ChainSlot::new(&device, self.audio.sample_rate());
        self.devices_mut(target).push(device);
        self.audio.send(Command::InsertDevice { target, index, slot });
        self.detail = Detail::Devices;
    }

    pub fn remove_device(&mut self, target: Target, index: usize) {
        if index < self.devices(target).len() {
            self.devices_mut(target).remove(index);
            self.audio.send(Command::RemoveDevice { target, index });
            // Lanes follow their device: drop the removed one's, shift the ones after it.
            let lanes = self.automation_mut(target);
            lanes.retain(|l| !matches!(l.param, Param::Device { device, .. } if device == index));
            for lane in lanes.iter_mut() {
                if let Param::Device { device, .. } = &mut lane.param {
                    if *device > index {
                        *device -= 1;
                    }
                }
            }
            self.sync_automation(target);
        }
    }

    pub fn update_device(&mut self, target: Target, index: usize) {
        if let Some(&device) = self.devices(target).get(index) {
            self.audio.send(Command::UpdateDevice { target, index, device });
        }
    }

    pub fn move_device(&mut self, target: Target, from: usize, to: usize) {
        let devices = self.devices_mut(target);
        if from < devices.len() && to < devices.len() {
            devices.swap(from, to);
            for lane in self.automation_mut(target).iter_mut() {
                if let Param::Device { device, .. } = &mut lane.param {
                    if *device == from {
                        *device = to;
                    } else if *device == to {
                        *device = from;
                    }
                }
            }
            self.sync_chain(target);
            self.sync_automation(target);
        }
    }

    pub fn set_instrument(&mut self, track: usize, kind: InstrumentKind) {
        let previous = std::mem::replace(&mut self.project.tracks[track].instrument, kind);
        self.audio.send(Command::SetInstrument { track, kind });
        // Another instrument has other parameters: its automation no longer applies.
        if previous.label() != kind.label() {
            let target = Target::Track(track);
            self.automation_mut(target).retain(|l| !matches!(l.param, Param::Instrument(_)));
            self.overridden.retain(|(t, p)| !(*t == target && matches!(p, Param::Instrument(_))));
            self.sync_automation(target);
        }
    }

    pub fn sync_samples(&mut self, track: usize) {
        let samples = sync::instrument_samples(&self.project.tracks[track], &mut self.samples);
        self.audio.send(Command::SetSamples { track, samples });
        self.show_sample_errors();
    }

    /// Loads files into consecutive sample slots of a sampler or drum rack, starting at `slot`.
    pub fn set_instrument_samples(&mut self, track: usize, slot: usize, paths: Vec<PathBuf>) {
        let slots = match self.project.tracks[track].instrument {
            InstrumentKind::Sampler(_) | InstrumentKind::Kick(_) => 1,
            InstrumentKind::DrumRack(_) => RACK_PADS,
            _ => return,
        };
        let mut loaded = 0;
        for (slot, path) in (slot..slots).zip(paths) {
            let path = path.to_string_lossy().into_owned();
            if self.sample(&path).frames() == 0 {
                continue;
            }
            self.project.tracks[track].set_sample(slot, Some(path));
            loaded += 1;
        }
        if loaded > 0 {
            self.sync_samples(track);
            let name = self.project.tracks[track].sample_name(slot).unwrap_or_default();
            self.status = Some(if loaded == 1 { format!("Cargado: {name}") } else { format!("{loaded} muestras cargadas") });
        }
    }

    pub fn clear_instrument_sample(&mut self, track: usize, slot: usize) {
        self.project.tracks[track].set_sample(slot, None);
        self.sync_samples(track);
    }

    pub fn choose_instrument_samples(&mut self, track: usize, slot: usize) {
        let dialog = rfd::FileDialog::new().add_filter("Audio", &AUDIO_EXTENSIONS);
        let paths = match self.project.tracks[track].instrument {
            InstrumentKind::DrumRack(_) => dialog.pick_files(),
            _ => dialog.pick_file().map(|p| vec![p]),
        };
        if let Some(paths) = paths {
            self.set_instrument_samples(track, slot, paths);
        }
    }

    /// Fills a drum rack with the starter kit rendered from the drum machine.
    pub fn load_basic_kit(&mut self, track: usize) {
        match crate::kits::basic_kit(&crate::kits::kits_dir().join("Basic Kit")) {
            Ok((params, files)) => {
                self.set_instrument(track, InstrumentKind::DrumRack(params));
                self.project.tracks[track].samples = files;
                self.sync_samples(track);
                self.status = Some("Kit básico cargado".into());
            }
            Err(error) => self.status = Some(error),
        }
    }

    /// Loads a preset's settings (and samples, for instruments that play them).
    pub fn apply_preset(&mut self, track: usize, preset: crate::presets::Preset) {
        self.set_instrument(track, preset.instrument);
        if preset.instrument.uses_samples() {
            self.project.tracks[track].samples = preset.samples;
            self.sync_samples(track);
        }
        self.status = Some(format!("Preset: {}", preset.name));
    }

    /// Saves the track's instrument as a user preset named `name`.
    pub fn save_preset(&mut self, track: usize, name: String) {
        let t = &self.project.tracks[track];
        let samples = if t.instrument.uses_samples() { t.samples.clone() } else { Vec::new() };
        let preset = crate::presets::Preset { name, instrument: t.instrument, samples };
        self.status = Some(match crate::presets::save(&crate::presets::user_root(), &preset) {
            Ok(_) => format!("Preset guardado: {}", preset.name),
            Err(error) => error,
        });
    }

    pub fn delete_preset(&mut self, preset: &crate::presets::Preset) {
        self.status = Some(match crate::presets::delete(&crate::presets::user_root(), preset) {
            Ok(()) => format!("Preset borrado: {}", preset.name),
            Err(error) => error,
        });
    }

    /// A new folder for the slices of `name`, next to the kits.
    fn slices_dir(name: &str) -> PathBuf {
        let base = crate::kits::kits_dir().with_file_name("Slices");
        let clean: String = name.chars().map(|c| if c == '/' || c == ':' { '-' } else { c }).collect();
        (1..)
            .map(|n| base.join(if n == 1 { clean.clone() } else { format!("{clean} {n}") }))
            .find(|dir| !dir.exists())
            .expect("an unused folder name")
    }

    /// Slices a whole file into the drum rack on `track`, replacing its pads, and
    /// puts the clip that replays it in the first free session slot.
    pub fn slice_file_into(&mut self, track: usize) {
        let Some(path) = rfd::FileDialog::new().add_filter("Audio", &AUDIO_EXTENSIONS).pick_file() else { return };
        let sample = self.sample(&path.to_string_lossy());
        if sample.frames() == 0 {
            return;
        }
        let name = path.file_stem().map_or("Loop".into(), |s| s.to_string_lossy().into_owned());
        let bpm = guess_loop_bpm(sample.duration(), self.project.tempo).unwrap_or(self.project.tempo);
        let color = self.project.tracks[track].color;
        let region = (0.0, sample.duration());
        match crate::slicing::slice(&sample, region, bpm, self.slice_mode, &name, &Self::slices_dir(&name), color) {
            Ok((sliced, _)) => {
                let pads = sliced.clip.notes.len();
                self.set_instrument(track, InstrumentKind::DrumRack(sliced.params));
                self.project.tracks[track].samples = sliced.files;
                self.sync_samples(track);
                let free = (0..NUM_SCENES).find(|&s| self.project.tracks[track].clips[s].is_none());
                match free {
                    Some(slot) => {
                        self.set_clip(track, slot, Some(Clip::Midi(sliced.clip)));
                        self.status = Some(format!("{name}: {pads} trozos a {bpm:.1} BPM, clip en la escena {}", slot + 1));
                    }
                    None => self.status = Some(format!("{name}: {pads} trozos (no quedan slots libres para el clip)")),
                }
            }
            Err(error) => self.status = Some(error),
        }
    }

    /// Cuts an audio clip into a new drum rack track, with a MIDI clip in the same
    /// place that plays the slices in order (like "Slice to New MIDI Track").
    pub fn slice_clip(&mut self, clip_ref: ClipRef) {
        let Some(Clip::Audio(audio)) = self.clip(clip_ref).cloned() else { return };
        let sample = self.sample(&audio.sample);
        if sample.frames() == 0 {
            self.status = Some("No se encuentra la muestra del clip".into());
            return;
        }
        let bpm = if audio.warp == WarpMode::Off { self.project.tempo } else { audio.original_bpm };
        let region = (audio.offset, audio.length * 60.0 / bpm);
        let mode = self.slice_mode;
        let dir = Self::slices_dir(&audio.name);
        let (mut sliced, grid) = match crate::slicing::slice(&sample, region, bpm, mode, &audio.name, &dir, audio.color) {
            Ok(result) => result,
            Err(error) => {
                self.status = Some(error);
                return;
            }
        };
        sliced.params.tune = [audio.transpose; RACK_PADS];
        sliced.params.level = [engine::model::db_to_gain(audio.gain_db).min(1.0); RACK_PADS];
        let pads = sliced.clip.notes.len();
        let name = audio.name.clone();
        let midi = Clip::Midi(sliced.clip);
        let placed = match clip_ref {
            ClipRef::Arrangement { track, index } => self.project.tracks[track].arrangement.get(index).map(|a| (a.start, a.length)),
            ClipRef::Session { .. } => None,
        };
        let added = self.push_track(|_, color| {
            let mut track = Track::new(format!("{name} Slices"), color, InstrumentKind::DrumRack(sliced.params));
            track.samples = sliced.files;
            match (clip_ref, placed) {
                (ClipRef::Session { slot, .. }, _) => track.clips[slot] = Some(midi),
                (_, Some((start, length))) => track.arrangement.push(ArrangementClip { start, length, clip: midi }),
                _ => {}
            }
            track
        });
        let Some(track) = added else { return };
        self.select_clip(match clip_ref {
            ClipRef::Session { slot, .. } => ClipRef::Session { track, slot },
            ClipRef::Arrangement { .. } => ClipRef::Arrangement { track, index: 0 },
        });
        let grid_note = if grid > 0.0 { format!(" (rejilla de {grid} beats)") } else { String::new() };
        self.status = Some(format!("{name}: {pads} trozos en un Drum Rack nuevo{grid_note}"));
    }

    fn push_track(&mut self, make: impl FnOnce(usize, [u8; 3]) -> Track) -> Option<usize> {
        let index = self.project.tracks.len();
        if index >= MAX_TRACKS {
            self.status = Some(format!("Máximo de {MAX_TRACKS} pistas"));
            return None;
        }
        let color = PALETTE[(index * 5 + 1) % PALETTE.len()];
        self.project.tracks.push(make(index, color));
        self.selected_target = Target::Track(index);
        self.sync_all();
        Some(index)
    }

    pub fn add_track(&mut self) {
        self.push_track(|i, color| Track::new(format!("{} MIDI", i + 1), color, InstrumentKind::Synth(SynthParams::default())));
    }

    pub fn add_audio_track(&mut self) -> Option<usize> {
        self.push_track(|i, color| Track::new_audio(format!("{} Audio", i + 1), color))
    }

    pub fn delete_track(&mut self, track: usize) {
        if self.project.tracks.len() <= 1 {
            return;
        }
        self.project.tracks.remove(track);
        self.selected_clip = None;
        self.selected_target = Target::Track(track.min(self.project.tracks.len() - 1));
        self.sync_all();
    }

    pub fn duplicate_track(&mut self, track: usize) {
        if self.project.tracks.len() >= MAX_TRACKS {
            self.status = Some(format!("Máximo de {MAX_TRACKS} pistas"));
            return;
        }
        let mut copy = self.project.tracks[track].clone();
        copy.name = format!("{} copia", copy.name);
        self.project.tracks.insert(track + 1, copy);
        self.selected_target = Target::Track(track + 1);
        self.sync_all();
    }

    /// Returns tracks to their arrangement clips and re-enables overridden automation.
    pub fn back_to_arrangement(&mut self) {
        self.audio.send(Command::BackToArrangement);
        let targets: Vec<Target> = self.overridden.drain().map(|(target, _)| target).collect();
        for target in targets {
            self.sync_automation(target);
        }
    }

    pub fn back_to_arrangement_needed(&self) -> bool {
        !self.overridden.is_empty()
            || self.audio.state().is_some_and(|s| {
            s.tracks[..self.project.tracks.len()]
                .iter()
                .any(|t| !t.follows_arrangement.load(Ordering::Relaxed))
        })
    }

    // ----- Live input and recording ------------------------------------------------

    pub fn rescan_midi(&mut self) {
        self.midi = MidiInputs::default();
        self.midi = MidiInputs::connect(self.audio.live());
        self.status = Some(match self.midi.names.len() {
            0 => "No hay dispositivos MIDI conectados".into(),
            n => format!("{n} dispositivo(s) MIDI: {}", self.midi.names.join(", ")),
        });
    }

    pub fn toggle_keyboard(&mut self) {
        let live = self.audio.live();
        self.keyboard.toggle(live.as_ref());
    }

    pub fn toggle_arm(&mut self, track: usize) {
        let t = &mut self.project.tracks[track];
        t.armed = !t.armed;
        let (armed, audio) = (t.armed, t.is_audio());
        self.audio.send(Command::SetArm { track, armed });
        if armed && audio {
            self.ensure_capture();
        }
    }

    /// Opens the audio input (macOS asks for microphone permission the first time).
    fn ensure_capture(&mut self) -> bool {
        if self.capture.is_some() {
            return true;
        }
        let Some(state) = self.audio.state() else { return false };
        match InputCapture::open(state) {
            Ok(capture) => {
                self.status = Some(format!("Entrada de audio: {}", capture.device_name()));
                self.capture = Some(capture);
                true
            }
            Err(error) => {
                self.status = Some(format!("No se pudo abrir la entrada de audio: {error}"));
                false
            }
        }
    }

    /// Keeps the input flowing while audio tracks are armed; closes it otherwise.
    fn pump_capture(&mut self) {
        let wanted = self.project.tracks.iter().any(|t| t.armed && t.is_audio())
            || self.recorder.session.iter().any(|take| self.project.tracks.get(take.track).is_some_and(|t| t.is_audio()));
        if !wanted {
            self.capture = None;
            return;
        }
        let keep_all = !self.recorder.session.is_empty() || self.recorder.arrangement.is_some();
        if let Some(capture) = &mut self.capture {
            capture.pump(keep_all);
        }
    }

    /// Folder for recorded takes: next to the project, or in ~/Music/Pulse.
    fn recordings_dir(&self) -> PathBuf {
        let base = match &self.file {
            Some(file) => file.parent().map(Path::to_path_buf).unwrap_or_default(),
            None => std::env::var_os("HOME").map_or_else(PathBuf::new, |home| PathBuf::from(home).join("Music/Pulse")),
        };
        base.join("Recordings")
    }

    /// Cuts `beats` of input starting at `start` (transport beats) into a new audio clip.
    fn audio_take(&mut self, track: usize, start: f64, beats: f64) -> Option<AudioClip> {
        let tempo = self.project.tempo;
        let capture = self.capture.as_ref()?;
        let rate = capture.sample_rate();
        let first = capture.frame_at_beat(start)?;
        let frames = (beats * 60.0 / tempo * rate as f64).round() as usize;
        let (left, right) = capture.extract(first, frames);

        let dir = self.recordings_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.status = Some(format!("No se pudo crear {}: {e}", dir.display()));
            return None;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let name = format!("{} {stamp}", self.project.tracks[track].name);
        let path = dir.join(format!("{name}.wav"));
        if let Err(error) = write_wav(&path, &left, &right, rate) {
            self.status = Some(error);
            return None;
        }
        let key = path.to_string_lossy().into_owned();
        self.samples.insert(key.clone(), Arc::new(Sample::from_channels(left, right, rate)));
        Some(AudioClip {
            name,
            color: self.project.tracks[track].color,
            sample: key,
            length: beats,
            offset: 0.0,
            // Recorded at the project tempo; Stretch keeps the pitch if the tempo changes later.
            warp: WarpMode::Stretch,
            original_bpm: tempo,
            gain_db: 0.0,
            transpose: 0.0,
        })
    }

    /// Records a new clip into an empty session slot, starting at the next launch boundary.
    pub fn record_slot(&mut self, track: usize, slot: usize) {
        if self.project.tracks[track].is_audio() && !self.ensure_capture() {
            return;
        }
        if !self.project.tracks[track].armed {
            self.toggle_arm(track);
        }
        self.recorder.start_session(track, slot);
        self.selected_target = Target::Track(track);
        self.audio.send(Command::RecordClip { track, slot });
    }

    pub fn stop_session_recording(&mut self, track: usize) {
        let position = self.audio.position();
        self.recorder.stop_session(track, position);
        self.audio.send(Command::StopRecording(track));
    }

    pub fn is_arrangement_recording(&self) -> bool {
        self.recorder.arrangement.is_some()
    }

    /// Starts or stops recording armed tracks onto the arrangement.
    pub fn toggle_arrangement_record(&mut self) {
        if self.recorder.arrangement.is_some() {
            self.audio.send(Command::SetArrangementRecord(false));
            return;
        }
        if !self.project.tracks.iter().any(|t| t.armed) {
            self.status = Some("Arma una pista (botón ●) para grabar en ella".into());
            return;
        }
        if self.project.tracks.iter().any(|t| t.armed && t.is_audio()) && !self.ensure_capture() {
            return;
        }
        self.recorder.start_arrangement(self.audio.position());
        self.audio.send(Command::SetArrangementRecord(true));
        self.audio.send(Command::Play);
    }

    /// Snaps recorded note starts to the record quantization grid, if one is set.
    /// The grid is the song's, so a take that began off the beat still lands on it.
    fn quantize_take(&self, notes: Vec<engine::model::Note>, start: f64, length: f64) -> Vec<engine::model::Note> {
        let Some(grid) = self.project.record_quantize else { return notes };
        let mut absolute: Vec<_> = notes.into_iter().map(|n| engine::model::Note { start: n.start + start, ..n }).collect();
        let all: Vec<usize> = (0..absolute.len()).collect();
        crate::note_edit::quantize(&mut absolute, &all, grid, 1.0, false, start + length);
        absolute
            .into_iter()
            .map(|n| engine::model::Note { start: (n.start - start).max(0.0), ..n })
            .filter(|n| n.start < length)
            .collect()
    }

    /// Live input follows the selected track when no track is armed.
    fn update_input_track(&mut self) {
        let desired = match self.selected_target {
            Target::Track(t) => Some(t),
            _ => None,
        };
        if self.input_track != Some(desired) {
            self.audio.send(Command::SetInputTrack(desired));
            self.input_track = Some(desired);
        }
    }

    fn update_recording(&mut self) {
        let Some(state) = self.audio.state() else { return };
        self.pump_capture();
        let notes = self.audio.take_recorded();
        let armed: Vec<bool> = self.project.tracks.iter().map(|t| t.armed && !t.is_audio()).collect();
        for take in self.recorder.update(notes, &state, &armed) {
            match take {
                Finished::Session { track, slot, start, length, notes } => {
                    let Some(t) = self.project.tracks.get(track) else { continue };
                    if t.clips[slot].is_some() {
                        continue;
                    }
                    if t.is_audio() {
                        if let Some(clip) = self.audio_take(track, start, length) {
                            self.set_clip(track, slot, Some(Clip::Audio(clip)));
                            self.select_clip(ClipRef::Session { track, slot });
                            self.status = Some(format!("Toma de audio guardada en {}", self.recordings_dir().display()));
                        }
                        continue;
                    }
                    let count = notes.len();
                    let notes = self.quantize_take(notes, start, length);
                    let t = &self.project.tracks[track];
                    let clip = MidiClip { name: format!("Toma {}", slot + 1), length, notes, color: t.color };
                    self.set_clip(track, slot, Some(Clip::Midi(clip)));
                    self.select_clip(ClipRef::Session { track, slot });
                    self.status = Some(format!("Grabadas {count} notas en {}", self.project.tracks[track].name));
                }
                Finished::Arrangement { track, start, length, notes } => {
                    let notes = self.quantize_take(notes, start, length);
                    let Some(t) = self.project.tracks.get_mut(track) else { continue };
                    let count = notes.len();
                    let clip = MidiClip { name: "Toma".into(), length, notes, color: t.color };
                    t.arrangement.push(ArrangementClip { start, length, clip: Clip::Midi(clip) });
                    let index = t.arrangement.len() - 1;
                    self.sync_arrangement(track);
                    self.select_clip(ClipRef::Arrangement { track, index });
                    self.status = Some(format!("Grabadas {count} notas en el Arrangement"));
                }
                Finished::ArrangementEnded { start, end } => {
                    let beats = ((end - start) * 4.0).round() / 4.0;
                    if beats <= 0.0 {
                        continue;
                    }
                    let tracks: Vec<usize> = (0..self.project.tracks.len())
                        .filter(|&t| self.project.tracks[t].armed && self.project.tracks[t].is_audio())
                        .collect();
                    for track in tracks {
                        let Some(clip) = self.audio_take(track, start, beats) else { continue };
                        let lane = &mut self.project.tracks[track].arrangement;
                        lane.push(ArrangementClip { start, length: beats, clip: Clip::Audio(clip) });
                        let index = lane.len() - 1;
                        self.sync_arrangement(track);
                        self.select_clip(ClipRef::Arrangement { track, index });
                        self.status = Some(format!("Toma de audio guardada en {}", self.recordings_dir().display()));
                    }
                }
                Finished::Cancelled => {}
            }
        }
    }

    pub fn toggle_play(&mut self) {
        let command = if self.audio.is_playing() { Command::Stop } else { Command::Play };
        self.audio.send(command);
    }

    // ----- History and files -----------------------------------------------------

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo(&mut self) {
        if self.history.undo(&mut self.project) {
            self.after_project_replaced();
        }
    }

    pub fn redo(&mut self) {
        if self.history.redo(&mut self.project) {
            self.after_project_replaced();
        }
    }

    fn after_project_replaced(&mut self) {
        if self.selected_clip.is_some_and(|c| self.clip(c).is_none()) {
            self.selected_clip = None;
        }
        let tracks = self.project.tracks.len();
        if matches!(self.selected_target, Target::Track(t) if t >= tracks) {
            self.selected_target = Target::Track(0);
        }
        self.sync_all();
    }

    fn replace_project(&mut self, project: Project, file: Option<PathBuf>) {
        self.audio.send(Command::Stop);
        self.project = project;
        self.saved = self.project.clone();
        self.history.reset(&self.project);
        self.file = file;
        self.selected_clip = None;
        self.selected_target = Target::Track(0);
        self.sync_all();
    }

    pub fn is_dirty(&self) -> bool {
        self.project != self.saved
    }

    pub fn new_project(&mut self) {
        self.replace_project(Project::empty(), None);
        self.status = Some("Proyecto nuevo".into());
    }

    pub fn open(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Proyecto Pulse", &["pulse"]).pick_file() {
            self.open_from(&path);
        }
    }

    pub fn import_live_set(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter("Set de Ableton Live", &["als"]);
        if let Some(path) = dialog.pick_file() {
            self.import_live_set_from(&path);
        }
    }

    fn import_live_set_from(&mut self, path: &std::path::Path) {
        match crate::als::import(path) {
            Ok((project, report)) => {
                // No file is attached, so saving asks for a new .pulse and never touches the .als.
                self.replace_project(project, None);
                // Mark the import as unsaved work.
                self.saved = Project::empty();
                self.status = Some(report.summary());
            }
            Err(error) => self.status = Some(format!("No se pudo importar: {error}")),
        }
    }

    fn open_from(&mut self, path: &std::path::Path) {
        match Project::load(path) {
            Ok(project) => {
                self.status = Some(format!("Abierto {}", path.display()));
                self.replace_project(project, Some(path.to_path_buf()));
            }
            Err(error) => self.status = Some(error),
        }
    }

    /// Where dropped audio files go when a sampler or drum rack is on screen: the
    /// slot under the pointer, else the sampler or the selected pad.
    fn instrument_drop_slot(&self, ctx: &egui::Context) -> Option<(usize, usize)> {
        let (_, track, _) = *self.sample_drop_zones.first()?;
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let under = pointer.and_then(|p| self.sample_drop_zones.iter().find(|(rect, ..)| rect.contains(p)));
        if let Some(&(_, track, slot)) = under {
            return Some((track, slot));
        }
        match self.project.tracks.get(track)?.instrument {
            InstrumentKind::DrumRack(_) => Some((track, self.selected_pad)),
            _ => Some((track, 0)),
        }
    }

    /// Opens `.pulse` projects and imports `.als` sets dropped onto the window.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        let (audio, other): (Vec<PathBuf>, Vec<PathBuf>) = dropped.into_iter().partition(|p| is_audio_file(p));
        if !audio.is_empty() {
            match self.instrument_drop_slot(ctx) {
                Some((track, slot)) => self.set_instrument_samples(track, slot, audio),
                None => self.add_samples(audio, Placement::Auto),
            }
        }
        for path in other {
            match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
                Some("als") => self.import_live_set_from(&path),
                Some("pulse") => self.open_from(&path),
                _ => self.status = Some(format!("Formato no compatible: {}", path.display())),
            }
        }
    }

    pub fn save(&mut self) {
        match self.file.clone() {
            Some(path) => self.save_to(path),
            None => self.save_as(),
        }
    }

    pub fn save_as(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter("Proyecto Pulse", &["pulse"])
            .set_file_name("Sin título.pulse");
        if let Some(path) = dialog.save_file() {
            self.save_to(path);
        }
    }

    fn save_to(&mut self, path: PathBuf) {
        match self.project.save(&path) {
            Ok(()) => {
                self.saved = self.project.clone();
                self.status = Some(format!("Guardado en {}", path.display()));
                self.file = Some(path);
            }
            Err(error) => self.status = Some(error),
        }
    }

    pub fn file_name(&self) -> String {
        self.file
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or("Sin título".into(), |s| s.to_string_lossy().into_owned())
    }

    // ----- Frame ------------------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let input = ctx.input(|i| {
            let cmd = i.modifiers.command;
            let shift = i.modifiers.shift;
            [
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::Tab) && !shift,
                i.key_pressed(egui::Key::Tab) && shift,
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                cmd && i.key_pressed(egui::Key::D),
                cmd && !shift && i.key_pressed(egui::Key::Z),
                cmd && shift && i.key_pressed(egui::Key::Z),
                cmd && !shift && i.key_pressed(egui::Key::S),
                cmd && shift && i.key_pressed(egui::Key::S),
                cmd && i.key_pressed(egui::Key::O),
                cmd && i.key_pressed(egui::Key::N),
                cmd && !shift && i.key_pressed(egui::Key::T),
                cmd && shift && i.key_pressed(egui::Key::T),
                cmd && i.key_pressed(egui::Key::I),
                cmd && shift && i.key_pressed(egui::Key::R),
                !cmd && i.key_pressed(egui::Key::M),
                i.key_pressed(egui::Key::F9),
            ]
        });
        let [space, tab, shift_tab, delete, duplicate, undo, redo, save, save_as, open, new, add_track, add_audio, import, export, keyboard, record] =
            input;
        if export {
            self.export.open = true;
        }
        if keyboard {
            self.toggle_keyboard();
        }
        if record {
            self.toggle_arrangement_record();
        }
        if add_audio {
            self.add_audio_track();
        }
        if import {
            self.import_live_set();
        }
        if space {
            self.toggle_play();
        }
        if tab {
            self.view = match self.view {
                View::Session => View::Arrangement,
                View::Arrangement => View::Session,
            };
        }
        if shift_tab {
            self.detail = match self.detail {
                Detail::Clip => Detail::Devices,
                Detail::Devices => Detail::Clip,
            };
        }
        // With notes selected in the clip editor, these act on the notes instead.
        let editing_notes = self.detail == Detail::Clip && self.piano_roll.has_selection();
        if delete && !editing_notes {
            self.delete_selected_clip();
        }
        if duplicate && !editing_notes {
            self.duplicate_selected_clip();
        }
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if save {
            self.save();
        }
        if save_as {
            self.save_as();
        }
        if open {
            self.open();
        }
        if new {
            self.new_project();
        }
        if add_track {
            self.add_track();
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = format!("{}{} — Pulse", self.file_name(), if self.is_dirty() { " *" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn debug_screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = &mut self.screenshot else { return };
        *frames += 1;
        if *frames == 240 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            let mut ppm = format!("P6 {w} {h} 255\n").into_bytes();
            for p in &image.pixels {
                ppm.extend_from_slice(&[p.r(), p.g(), p.b()]);
            }
            if let Err(e) = std::fs::write(&*path, ppm) {
                eprintln!("cannot write screenshot: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for PulseApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.audio.collect_garbage();
        if let Some(state) = self.audio.state() {
            self.meters.update(&state);
        }
        self.handle_shortcuts(&ctx);
        self.handle_dropped_files(&ctx);
        self.sample_drop_zones.clear();
        let live = self.audio.live();
        self.keyboard.handle(&ctx, live.as_ref());
        self.update_input_track();
        self.update_recording();

        let bar = egui::Frame::new().fill(theme::SURFACE).inner_margin(egui::Margin::symmetric(8, 5));
        egui::Panel::top("transport").frame(bar).show(ui, |ui| transport::show(self, ui));
        let status = egui::Frame::new().fill(theme::SURFACE).inner_margin(egui::Margin::symmetric(8, 2));
        egui::Panel::bottom("status").frame(status).show(ui, |ui| transport::status_bar(self, ui));
        if self.show_browser {
            let frame = egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(6));
            egui::Panel::left("browser")
                .frame(frame)
                .resizable(true)
                .default_size(180.0)
                .min_size(140.0)
                .show(ui, |ui| browser::show(self, ui));
        }
        let frame = egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(6));
        egui::Panel::bottom("detail")
            .frame(frame)
            .resizable(true)
            .default_size(290.0)
            .min_size(200.0)
            .show(ui, |ui| detail::show(self, ui));
        let frame = egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(6));
        egui::CentralPanel::default().frame(frame).show(ui, |ui| match self.view {
            View::Session => session::show(self, ui),
            View::Arrangement => arrangement::show(self, ui),
        });

        let interacting = ctx.input(|i| i.pointer.any_down());
        self.history.commit(&self.project, interacting);
        self.update_title(&ctx);
        crate::export::window(self, &ctx);
        self.debug_screenshot(&ctx);
        // Meters and playheads animate continuously.
        ctx.request_repaint();
    }
}
