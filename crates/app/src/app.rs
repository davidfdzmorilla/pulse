use crate::history::History;
use crate::project::{Mixer, Project, Track, PALETTE};
use std::collections::HashMap;
use std::path::Path;
use crate::capture::{write_wav, InputCapture};
use crate::input::{ComputerKeyboard, MidiInputs};
use crate::recording::{Finished, Recorder};
use crate::{arrangement, browser, detail, piano_roll, session, theme, transport};
use eframe::egui;
use engine::model::{ArrangementClip, AudioClip, Clip, Device, InstrumentKind, MidiClip, SynthParams, WarpMode};
use engine::{
    build_chain, guess_loop_bpm, AudioOutput, ChainSlot, Command, EngineHandle, LiveInput, PlacedClip, PlayClip,
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
    /// Decoded audio files by path; failed loads are cached as empty samples.
    samples: HashMap<String, Arc<Sample>>,
    pub midi: MidiInputs,
    pub keyboard: ComputerKeyboard,
    pub recorder: Recorder,
    /// Audio input, opened when an audio track is armed.
    pub capture: Option<InputCapture>,
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
            samples: HashMap::new(),
            midi: MidiInputs::default(),
            keyboard: ComputerKeyboard::default(),
            recorder: Recorder::default(),
            capture: None,
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
        let p = &self.project;
        let commands = [
            Command::SetTempo(p.tempo),
            Command::SetMetronome(p.metronome),
            Command::SetQuantize(p.quantize),
            Command::SetLoop {
                enabled: p.loop_enabled,
                start: p.loop_start,
                end: p.loop_end,
            },
        ];
        for command in commands {
            self.audio.send(command);
        }
        for t in 0..MAX_TRACKS {
            let track = self
                .project
                .tracks
                .get(t)
                .cloned()
                .unwrap_or_else(|| Track::new("", [0, 0, 0], InstrumentKind::Synth(SynthParams::default())));
            let arrangement = self.placed_clips(&track);
            let mut commands = vec![
                Command::SetInstrument { track: t, kind: track.instrument },
                Command::SetSolo { track: t, solo: track.solo },
                Command::SetArm { track: t, armed: track.armed },
                Command::ReplaceChain {
                    target: Target::Track(t),
                    chain: build_chain(&track.devices, self.audio.sample_rate()),
                },
                Command::SetArrangement { track: t, clips: arrangement },
            ];
            for (send, &amount) in track.sends.iter().enumerate() {
                commands.push(Command::SetSend { track: t, send, amount });
            }
            for slot in 0..NUM_SCENES {
                let clip = track.clips[slot].as_ref().map(|c| Box::new(self.play_clip(c)));
                commands.push(Command::SetClip { track: t, slot, clip });
            }
            let mixer = track.mixer.clone();
            for command in commands {
                self.audio.send(command);
            }
            self.send_mixer(Target::Track(t), &mixer);
        }
        for r in 0..NUM_RETURNS {
            self.sync_chain(Target::Return(r));
            self.sync_mixer(Target::Return(r));
        }
        self.sync_chain(Target::Master);
        self.sync_mixer(Target::Master);
    }

    fn send_mixer(&mut self, target: Target, mixer: &Mixer) {
        self.audio.send(Command::SetGain { target, gain: mixer.gain });
        self.audio.send(Command::SetPan { target, pan: mixer.pan });
        self.audio.send(Command::SetMute { target, mute: !mixer.active });
    }

    pub fn sync_mixer(&mut self, target: Target) {
        let mixer = self.mixer(target).clone();
        self.send_mixer(target, &mixer);
    }

    pub fn sync_chain(&mut self, target: Target) {
        let chain = build_chain(self.devices(target), self.audio.sample_rate());
        self.audio.send(Command::ReplaceChain { target, chain });
    }

    pub fn sync_clip(&mut self, track: usize, slot: usize) {
        let clip = self.project.tracks[track].clips[slot].clone();
        let clip = clip.map(|c| Box::new(self.play_clip(&c)));
        self.audio.send(Command::SetClip { track, slot, clip });
    }

    pub fn sync_arrangement(&mut self, track: usize) {
        let t = self.project.tracks[track].clone();
        let clips = self.placed_clips(&t);
        self.audio.send(Command::SetArrangement { track, clips });
    }

    // ----- Samples ----------------------------------------------------------------

    /// Returns the decoded file, loading it on first use.
    pub fn sample(&mut self, path: &str) -> Arc<Sample> {
        if let Some(sample) = self.samples.get(path) {
            return sample.clone();
        }
        let sample = match Sample::load(Path::new(path)) {
            Ok(sample) => Arc::new(sample),
            Err(error) => {
                self.status = Some(format!("No se pudo cargar la muestra: {error}"));
                Arc::new(Sample::default())
            }
        };
        self.samples.insert(path.to_string(), sample.clone());
        sample
    }

    /// The decoded file if it is already loaded (for drawing).
    pub fn loaded_sample(&self, path: &str) -> Option<&Arc<Sample>> {
        self.samples.get(path)
    }

    fn play_clip(&mut self, clip: &Clip) -> PlayClip {
        match clip {
            Clip::Midi(midi) => PlayClip::Midi(midi.clone()),
            Clip::Audio(audio) => {
                let sample = self.sample(&audio.sample);
                PlayClip::Audio(audio.clone(), sample)
            }
        }
    }

    fn placed_clips(&mut self, track: &Track) -> Box<Vec<PlacedClip>> {
        let placed = track
            .arrangement
            .iter()
            .map(|a| PlacedClip {
                start: a.start,
                length: a.length,
                clip: self.play_clip(&a.clip),
            })
            .collect();
        Box::new(placed)
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
            self.sync_chain(target);
        }
    }

    pub fn set_instrument(&mut self, track: usize, kind: InstrumentKind) {
        self.project.tracks[track].instrument = kind;
        self.audio.send(Command::SetInstrument { track, kind });
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

    pub fn back_to_arrangement_needed(&self) -> bool {
        self.audio.state().is_some_and(|s| {
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
                    let clip = MidiClip { name: format!("Toma {}", slot + 1), length, notes, color: t.color };
                    self.set_clip(track, slot, Some(Clip::Midi(clip)));
                    self.select_clip(ClipRef::Session { track, slot });
                    self.status = Some(format!("Grabadas {count} notas en {}", self.project.tracks[track].name));
                }
                Finished::Arrangement { track, start, length, notes } => {
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

    /// Opens `.pulse` projects and imports `.als` sets dropped onto the window.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        let (audio, other): (Vec<PathBuf>, Vec<PathBuf>) = dropped.into_iter().partition(|p| is_audio_file(p));
        if !audio.is_empty() {
            self.add_samples(audio, Placement::Auto);
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
                !cmd && i.key_pressed(egui::Key::M),
                i.key_pressed(egui::Key::F9),
            ]
        });
        let [space, tab, shift_tab, delete, duplicate, undo, redo, save, save_as, open, new, add_track, add_audio, import, keyboard, record] =
            input;
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
        if delete {
            self.delete_selected_clip();
        }
        if duplicate {
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
        self.debug_screenshot(&ctx);
        // Meters and playheads animate continuously.
        ctx.request_repaint();
    }
}
