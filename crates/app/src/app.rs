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
    ArrangementClip, AudioClip, AutomationLane, Clip, Device, Effect, InstrumentKind, MidiClip, Param, ParamInfo, PluginId, SynthParams, WarpMode,
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
    /// No audio (headless, or between two starts).
    pub(crate) fn none() -> Self {
        Self { _output: None, handle: None, error: None }
    }

    /// The device name, sample rate and buffer size in use.
    pub fn info(&self) -> (String, f32, Option<u32>) {
        match &self.handle {
            Some(h) => (h.device_name.clone(), h.sample_rate, h.buffer),
            None => ("Sin audio".into(), 48_000.0, None),
        }
    }

    pub(crate) fn start(settings: &engine::AudioSettings) -> Self {
        match engine::start_with(settings) {
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

    /// Audio from an engine the caller runs by hand (tests).
    #[cfg(test)]
    pub(crate) fn detached() -> (Self, engine::Engine) {
        let (engine, handle) = engine::detached(48_000.0);
        (Self { _output: None, handle: Some(handle), error: None }, engine)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// The instruments with an editor window of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstrumentEditor {
    Kick,
    Wave,
    Poly,
    Modular,
}

/// The app's own windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Settings,
    Shortcuts,
    Guide,
    Versions,
    Mix,
    Updates,
    Export,
}

/// Where newly added samples go.
#[derive(Clone, Copy)]
pub enum Placement {
    /// The selected audio track (or a new one), in the current view.
    Auto,
    Session { track: usize, slot: usize },
    Arrangement { track: usize, start: f64 },
}

pub struct Meters {
    pub tracks: [f32; MAX_TRACKS],
    pub returns: [f32; NUM_RETURNS],
    pub master: f32,
}

impl Default for Meters {
    fn default() -> Self {
        Self { tracks: [0.0; MAX_TRACKS], returns: [0.0; NUM_RETURNS], master: 0.0 }
    }
}

impl Meters {
    /// `dt`: seconds since the last frame (the meters fall at the same speed
    /// whatever the frame rate).
    fn update(&mut self, state: &SharedState, dt: f32) {
        let k = 0.86f32.powf(dt * 60.0);
        let fall = |meter: &mut f32, peak: f32| *meter = peak.max(*meter * k);
        for (meter, status) in self.tracks.iter_mut().zip(&state.tracks) {
            fall(meter, status.peak.take());
        }
        for (meter, peak) in self.returns.iter_mut().zip(&state.returns) {
            fall(meter, peak.take());
        }
        fall(&mut self.master, state.master.take());
    }

    /// Whether any meter still shows something.
    pub fn sounding(&self) -> bool {
        self.tracks.iter().chain(&self.returns).chain([&self.master]).any(|&m| m > 1e-4)
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
    /// The empty session slot clicked last (with no clip selected): where pasting goes.
    pub paste_slot: Option<(usize, usize)>,
    /// A track being frozen.
    pub freeze_job: Option<crate::freeze::FreezeJob>,
    /// The audio clip editor shows the whole file (else just the clip's part).
    pub sample_view_whole: bool,
    /// Clips selected together with the selected one (⌘ clic, recuadro); see `clip_select`.
    pub selected_clips: Vec<ClipRef>,
    /// A selection rectangle or session clips being dragged.
    pub clip_gesture: Option<crate::clip_select::Gesture>,
    pub selected_target: Target,
    /// Tracks selected together with the selected one (⌘ or ⇧ clic); see `tracks`.
    pub selected_tracks: Vec<usize>,
    /// Track headers being dragged to a new place.
    pub track_drag: Option<crate::tracks::TrackDrag>,
    pub meters: Meters,
    pub piano_roll: piano_roll::State,
    pub arrangement: arrangement::State,
    pub show_browser: bool,
    /// Decoded audio files by path.
    pub(crate) samples: SampleCache,
    pub midi: MidiInputs,
    pub keyboard: ComputerKeyboard,
    pub recorder: Recorder,
    /// Controls moved while recording onto the arrangement (see `automation_rec`).
    pub automation_take: Option<crate::automation_rec::AutomationTake>,
    /// Controls moved while recording session clips, for their envelopes.
    pub clip_automation: Vec<crate::automation_rec::ClipAutomationTake>,
    /// Audio input, opened when an audio track is armed.
    pub capture: Option<InputCapture>,
    /// The input could not be opened: not retried every frame.
    pub capture_failed: bool,
    /// Tab pressed this frame (with shift?), kept from egui's focus navigation.
    tab_pressed: Option<bool>,
    pub export: crate::export::Dialog,
    /// Automated parameters the user moved by hand; their automation is
    /// suspended until "Back to Arrangement", as in Live.
    pub overridden: HashSet<(Target, Param)>,
    /// Drum rack pad shown in the device view.
    pub selected_pad: usize,
    /// Track whose kick designer editor window is open.
    pub kick_window: Option<usize>,
    /// Track whose wavetable synth editor window is open.
    pub wave_window: Option<usize>,
    pub table_editor: Option<crate::table_editor::TableEditor>,
    /// Track whose polychrome synth editor window is open.
    pub poly_window: Option<usize>,
    /// Track whose modular synth editor window is open.
    pub modular_window: Option<usize>,
    /// The VST3 plugins found on this computer, and the parameter window.
    pub plugin_library: crate::plugins::Library,
    /// The plugins' own editor windows.
    pub editors: crate::editor::Editors,
    /// MIDI learn, and the controllers linked to controls.
    pub learn: crate::midi_map::Learn,
    /// The control the user touched this frame (for MIDI learn).
    pub touched_control: std::cell::Cell<Option<crate::midi_map::Control>>,
    /// How loops are cut into drum rack pads.
    pub slice_mode: crate::slicing::SliceMode,
    /// Shows where a clip will be sliced, to move, add or remove cuts by hand.
    pub show_cuts: bool,
    pub slice_cuts: Option<crate::slicing::SliceCuts>,
    /// Sample slots drawn this frame, where dropped files can land: (area, track, slot).
    pub sample_drop_zones: Vec<(egui::Rect, usize, usize)>,
    /// The library browser's state, and where what it drags can land.
    pub browser: crate::browser::State,
    pub drop_targets: crate::browser::DropTargets,
    /// Input track last sent to the engine.
    pub(crate) input_track: Option<Option<usize>>,
    pub(crate) history: History,
    pub(crate) file: Option<PathBuf>,
    saved: Project,
    /// Recent projects, newest first.
    pub recent: Vec<PathBuf>,
    /// A copied or cut clip.
    pub clip_clipboard: Option<crate::clip_select::Clipboard>,
    /// Whether notes were copied last (⌘V pastes notes in the clip editor).
    pub paste_notes: bool,
    /// Answers to questions (unsaved changes…) for tests and headless apps, in order.
    pub answers: std::collections::VecDeque<crate::commands::Answer>,
    /// Whether dialogs may be shown (false in tests and headless apps).
    pub interactive: bool,
    /// The window is closing (the user chose to quit).
    pub quitting: bool,
    pub show_shortcuts: bool,
    pub guide: crate::guide::Guide,
    pub versions: crate::versions::Window,
    pub mix: crate::mix_assist::Assistant,
    /// The accompaniment written last (see `accompany`).
    pub accompaniment: Option<crate::accompany::Generated>,
    /// The session clips playing while recording onto the arrangement.
    pub session_capture: Option<crate::session_rec::SessionCapture>,
    /// Preferences, the window for them and the devices it shows.
    pub settings: crate::settings::Settings,
    pub show_settings: bool,
    /// New versions from GitHub (see `updates`).
    pub updates: crate::updates::Updates,
    /// A clip being separated into stems (see `stems_ui`).
    pub stems: Option<crate::stems_ui::StemJob>,
    /// An audio clip being turned into MIDI.
    pub to_midi: Option<crate::to_midi::MidiJob>,
    /// Something the user must see (title, text), in a window until dismissed.
    pub notice: Option<(String, String)>,
    /// Imported audio's tempo, found in the background (see `tempo_ui`).
    pub tempo: crate::tempo_ui::Tempo,
    pub devices: crate::settings::Devices,
    /// A text field has the keyboard: editing shortcuts are its.
    pub text_focus: bool,
    /// The macOS menu bar.
    menu_bar: Option<crate::native_menu::MenuBar>,
    pub status: Option<String>,
    title: String,
    /// Debug aid: with `PULSE_SCREENSHOT=<file.ppm>` the app saves a screenshot of
    /// its own window after a few seconds and quits. `PULSE_VIEW=arrangement`
    /// opens the arrangement view and `PULSE_PLAY=1` starts playback.
    screenshot: Option<(String, u32)>,
    /// A test script driving the app (`PULSE_SCRIPT`).
    pub script: Option<crate::script::Script>,
}

impl PulseApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let settings = crate::settings::Settings::load();
        crate::library::set_root(settings.library.clone());
        let mut app = Self::with_audio(Project::demo(), Audio::start(&settings.audio));
        app.settings = settings;
        app.plugin_library = crate::plugins::Library::load_cache();
        app.learn = crate::midi_map::Learn::load();
        // Screenshots (a debug aid) leave the user's settings and recent projects alone.
        // So do test scripts.
        app.script = crate::script::Script::from_env();
        let screenshot = std::env::var_os("PULSE_SCREENSHOT").is_some() || app.script.is_some();
        app.interactive = !screenshot;
        app.recent = crate::commands::load_recent();
        crate::library::prepare();
        app.menu_bar = crate::native_menu::MenuBar::install();
        // A project opened from Finder or given on the command line, else the
        // last one (if wanted).
        crate::open_files::set_context(&cc.egui_ctx);
        let project = |p: &PathBuf| p.extension().is_some_and(|e| e == "pulse" || e == "als");
        let from_finder = crate::open_files::take().into_iter().find(project);
        let argument = std::env::args().nth(1).map(PathBuf::from).filter(project);
        // Screenshots and scripts open the project they're given, not the last one.
        let last = app.settings.reopen_last.then(|| app.recent.first().cloned()).flatten().filter(|_| !screenshot);
        if let Some(path) = from_finder.or(argument).or(last) {
            if path.extension().is_some_and(|e| e == "als") {
                app.import_live_set_from(&path);
            } else {
                app.open_from(&path);
            }
        }
        app.sync_all();
        app.check_updates_on_start();
        app.midi = MidiInputs::connect(app.audio.live(), app.learn.shared.clone());
        app.sync_midi_map();
        if std::env::var("PULSE_VIEW").as_deref() == Ok("arrangement") {
            app.view = View::Arrangement;
        }
        if std::env::var("PULSE_DETAIL").as_deref() == Ok("clip") {
            app.detail = Detail::Clip;
        }
        if let Ok(scene) = std::env::var("PULSE_SCENE") {
            app.apply_scene(&scene);
        }
        if std::env::var("PULSE_PLAY").is_ok() {
            match app.view {
                View::Session => app.audio.send(Command::LaunchScene(1)),
                View::Arrangement => app.audio.send(Command::Play),
            }
        }
        app
    }

    /// An app without an audio device or a window, for testing the interface.
    #[cfg(test)]
    pub fn headless(project: Project) -> Self {
        let mut app = Self::with_audio(project, Audio { _output: None, handle: None, error: None });
        app.screenshot = None;
        app.sync_all();
        app
    }

    fn with_audio(project: Project, audio: Audio) -> Self {
        Self {
            history: History::new(&project),
            saved: project.clone(),
            project,
            audio,
            view: View::Session,
            detail: Detail::Devices,
            selected_clip: Some(ClipRef::Session { track: 0, slot: 0 }),
            paste_slot: None,
            freeze_job: None,
            sample_view_whole: false,
            selected_clips: Vec::new(),
            clip_gesture: None,
            selected_target: Target::Track(0),
            selected_tracks: Vec::new(),
            track_drag: None,
            meters: Meters::default(),
            piano_roll: Default::default(),
            arrangement: Default::default(),
            show_browser: true,
            samples: SampleCache::default(),
            midi: MidiInputs::default(),
            keyboard: ComputerKeyboard::default(),
            recorder: Recorder::default(),
            automation_take: None,
            clip_automation: Vec::new(),
            capture: None,
            capture_failed: false,
            tab_pressed: None,
            overridden: HashSet::new(),
            selected_pad: 0,
            slice_mode: crate::slicing::SliceMode::Transients,
            show_cuts: false,
            slice_cuts: None,
            kick_window: None,
            wave_window: None,
            table_editor: None,
            poly_window: None,
            modular_window: None,
            plugin_library: Default::default(),
            editors: Default::default(),
            learn: Default::default(),
            touched_control: Default::default(),
            sample_drop_zones: Vec::new(),
            browser: Default::default(),
            drop_targets: Default::default(),
            export: Default::default(),
            input_track: None,
            file: None,
            recent: Vec::new(),
            clip_clipboard: None,
            paste_notes: false,
            answers: Default::default(),
            interactive: false,
            quitting: false,
            show_shortcuts: false,
            guide: Default::default(),
            versions: Default::default(),
            mix: Default::default(),
            accompaniment: None,
            session_capture: None,
            settings: Default::default(),
            show_settings: false,
            updates: Default::default(),
            stems: None,
            to_midi: None,
            notice: None,
            tempo: Default::default(),
            devices: Default::default(),
            text_focus: false,
            menu_bar: None,
            status: None,
            title: String::new(),
            screenshot: std::env::var("PULSE_SCREENSHOT").ok().map(|path| (path, 0)),
            script: None,
        }
    }

    // ----- Engine synchronisation -------------------------------------------------

    /// Pushes the whole project to the engine (after loading or undo).
    pub fn sync_all(&mut self) {
        // Plugins are loaded again from their state: take it from the running ones first.
        self.capture_plugins();
        self.suspend_editors();
        self.samples.plugins.clear();
        let sample_rate = self.audio.sample_rate();
        let commands = sync::project_commands(&self.project, &mut self.samples, sample_rate, &self.overridden);
        for command in commands {
            self.audio.send(command);
        }
        self.audio.send(Command::SetCountIn(self.settings.count_in_bars as f64 * 4.0));
        self.sync_midi_map();
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
        // A first guess while the tempo is found (see `tempo_ui`): short files are
        // likely loops cut to whole bars; longer ones play as they are.
        let guess = if duration <= 30.0 { guess_loop_bpm(duration, tempo) } else { None };
        let (warp, original_bpm, length) = match guess {
            Some(bpm) => (WarpMode::Stretch, bpm, (duration * bpm / 60.0).round().max(1.0)),
            None => (WarpMode::Off, tempo, ((duration * tempo / 60.0) * 4.0).ceil().max(1.0) / 4.0),
        };
        AudioClip {
            envelopes: Vec::new(),
            name,
            color,
            sample: path.to_string_lossy().into_owned(),
            length,
            offset: 0.0,
            warp,
            original_bpm,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: 0.0,
            transpose: 0.0,
        }
    }

    pub fn choose_samples(&mut self, placement: Placement) {
        let dialog = rfd::FileDialog::new().add_filter("Audio", &AUDIO_EXTENSIONS);
        if let Some(paths) = crate::files::pick_files(dialog) {
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
        let mut created = Vec::new();
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
                    created.push(ClipRef::Session { track, slot });
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
                    created.push(ClipRef::Arrangement { track, index });
                    placement = Placement::Arrangement { track, start: start + length };
                }
                Placement::Auto => unreachable!("resolved above"),
            }
            added += 1;
        }
        if added > 0 {
            self.status = Some(format!("{added} muestra(s) añadida(s)"));
        }
        // Each file's key, so its clips follow the project's, and its tempo.
        self.find_clip_keys(&created);
        for clip in created {
            self.detect_tempo(vec![clip], false);
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

    /// The plugin and plugin parameter behind `param`, if it is one: a plugin's
    /// parameters are its instrument or device parameters, numbered by their ids.
    pub fn plugin_param(&self, target: Target, param: Param) -> Option<(PluginId, u32)> {
        let id = match param {
            Param::Instrument(i) => match (target, self.project.tracks.get(target_track(target)?)?.instrument) {
                (Target::Track(_), InstrumentKind::Plugin(id)) => return Some((id, i as u32)),
                _ => return None,
            },
            Param::Device { device, param } => match self.devices(target).get(device)?.effect {
                Effect::Plugin(id) => (id, param as u32),
                _ => return None,
            },
            _ => return None,
        };
        Some(id)
    }

    /// Where a plugin's parameter is automated: its track (or return) and parameter.
    pub fn plugin_target_param(&self, id: PluginId, param: u32) -> Option<(Target, Param)> {
        for (t, track) in self.project.tracks.iter().enumerate() {
            if track.instrument == InstrumentKind::Plugin(id) {
                return Some((Target::Track(t), Param::Instrument(param as usize)));
            }
        }
        let targets = (0..self.project.tracks.len()).map(Target::Track).chain((0..NUM_RETURNS).map(Target::Return)).chain([Target::Master]);
        for target in targets {
            if let Some(device) = self.devices(target).iter().position(|d| d.effect == Effect::Plugin(id)) {
                return Some((target, Param::Device { device, param: param as usize }));
            }
        }
        None
    }

    pub fn param_info(&self, target: Target, param: Param) -> Option<ParamInfo> {
        if let Some((id, p)) = self.plugin_param(target, param) {
            let default = self.samples.plugins.get(&id)?.param(p)?.default as f32;
            return Some(ParamInfo::linear("Plugin", 0.0, 1.0, default, engine::model::Unit::Percent));
        }
        match param {
            Param::Volume => Some(VOLUME_PARAM),
            Param::Pan => Some(PAN_PARAM),
            Param::Send(_) => Some(SEND_PARAM),
            Param::Device { device, param } => self.devices(target).get(device)?.effect.params().get(param).copied(),
            Param::Instrument(i) => match target {
                Target::Track(t) if !self.project.tracks[t].is_audio() => self.project.tracks[t].instrument.param_info(i),
                _ => None,
            },
        }
    }

    pub fn param_name(&self, target: Target, param: Param) -> String {
        if let Some((id, p)) = self.plugin_param(target, param) {
            let name = self.samples.plugins.get(&id).and_then(|h| h.param(p)).map_or_else(|| format!("parámetro {p}"), |p| p.name.clone());
            return format!("{}: {name}", self.plugin_name(id));
        }
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
                let instrument = &self.project.tracks[t].instrument;
                // Only settings that exist now (a modular synth's lane effects come and go).
                params.extend((0..instrument.params().len()).filter(|&i| instrument.param_info(i).is_some()).map(Param::Instrument));
                if let InstrumentKind::Plugin(id) = *instrument {
                    params.extend(self.plugin_params(id).map(|p| Param::Instrument(p as usize)));
                }
            }
        }
        for (device, d) in self.devices(target).iter().enumerate() {
            params.extend((0..d.effect.params().len()).map(|param| Param::Device { device, param }));
            if let Effect::Plugin(id) = d.effect {
                params.extend(self.plugin_params(id).map(|param| Param::Device { device, param: param as usize }));
            }
        }
        params
    }

    /// The automatable parameter ids of a loaded plugin.
    fn plugin_params(&self, id: PluginId) -> impl Iterator<Item = u32> + '_ {
        self.samples.plugins.get(&id).into_iter().flat_map(|h| h.params().iter().filter(|p| p.automatable).map(|p| p.id))
    }

    /// The value set by hand (what plays where there is no automation).
    pub fn param_value(&self, target: Target, param: Param) -> f32 {
        if let Some((id, p)) = self.plugin_param(target, param) {
            return self.samples.plugins.get(&id).map_or(0.0, |h| h.value(p) as f32);
        }
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
        self.touched(crate::midi_map::Control::Param { target, param });
        // Picking a control for MIDI learn sets it to itself: not a move (it would
        // override its automation).
        if self.learn.on && self.param_value(target, param) == value {
            return;
        }
        if let Some((id, p)) = self.plugin_param(target, param) {
            if let Some(handle) = self.samples.plugins.get(&id) {
                handle.set(p, value as f64);
            }
            if self.automation_state(target, param) == Some(false) && self.overridden.insert((target, param)) {
                self.sync_automation(target);
            }
            return;
        }
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
                    self.audio.send(Command::update_instrument(t, kind));
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

    /// Tells the user something in a window (a problem they must know about, not
    /// just a line in the status bar), and in the status bar too.
    pub fn notify(&mut self, title: &str, text: &str) {
        self.status = Some(text.to_string());
        self.notice = Some((title.to_string(), text.to_string()));
    }

    fn show_notice(&mut self, ctx: &egui::Context) {
        let Some((title, text)) = &self.notice else { return };
        let mut close = false;
        egui::Window::new(title.as_str()).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.label(text.as_str());
            ui.add_space(6.0);
            close = ui.button("Aceptar").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape));
        });
        if close {
            self.notice = None;
        }
    }

    // ----- Edits ------------------------------------------------------------------

    /// Opens an instrument's editor window, closing any other one (they would
    /// stack on top of each other).
    pub fn open_instrument_editor(&mut self, editor: InstrumentEditor, track: usize) {
        self.kick_window = None;
        self.wave_window = None;
        self.poly_window = None;
        self.modular_window = None;
        let slot = match editor {
            InstrumentEditor::Kick => &mut self.kick_window,
            InstrumentEditor::Wave => &mut self.wave_window,
            InstrumentEditor::Poly => &mut self.poly_window,
            InstrumentEditor::Modular => &mut self.modular_window,
        };
        *slot = Some(track);
    }

    /// Opens one of the app's windows (settings, shortcuts, updates, export),
    /// closing the others: one at a time, in the middle.
    pub fn open_panel(&mut self, panel: Panel) {
        self.show_settings = panel == Panel::Settings;
        self.show_shortcuts = panel == Panel::Shortcuts;
        self.guide.open = panel == Panel::Guide;
        self.versions.open = panel == Panel::Versions;
        self.mix.open = panel == Panel::Mix;
        self.updates.show = panel == Panel::Updates;
        self.export.open = panel == Panel::Export;
    }

    pub fn select_clip(&mut self, clip: ClipRef) {
        self.paste_slot = None;
        self.selected_clip = Some(clip);
        self.selected_clips.clear();
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

    /// Deletes every selected clip.
    pub fn delete_selected_clip(&mut self) {
        let clips = self.clip_selection();
        self.delete_clips(&clips);
    }

    /// Duplicates every selected clip: below it in the session (in the next free
    /// slot), after it on the arrangement. The copies end up selected.
    pub fn duplicate_selected_clip(&mut self) {
        let clips = self.clip_selection();
        if clips.len() <= 1 {
            return self.duplicate_clip();
        }
        let mut copies = Vec::new();
        for clip in clips {
            self.selected_clip = Some(clip);
            self.duplicate_clip();
            if let Some(copy) = self.selected_clip.filter(|&c| c != clip) {
                copies.push(copy);
            }
        }
        self.set_clip_selection(copies);
    }

    fn duplicate_clip(&mut self) {
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
            if let Effect::Plugin(id) = self.devices(target)[index].effect {
                self.capture_plugins();
                self.samples.plugins.remove(&id);
            }
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
        // A plugin replaced by something else: its instance goes (its state stays in the
        // project, for undo).
        if let InstrumentKind::Plugin(id) = previous {
            if kind != previous {
                self.capture_plugins();
                self.samples.plugins.remove(&id);
            }
        }
        if let InstrumentKind::Plugin(_) = kind {
            if kind != previous {
                // A plugin comes back (undo, presets): load it from its state.
                let sample_rate = self.audio.sample_rate();
                let command = sync::instrument_command(&self.project, track, kind, &mut self.samples, sample_rate);
                self.audio.send(command);
                self.show_sample_errors();
            }
            return;
        }
        if !previous.needs_rebuild(&kind) {
            self.audio.send(Command::update_instrument(track, kind));
        } else {
            let sample_rate = self.audio.sample_rate();
            self.audio.send(Command::set_instrument(track, kind, sample_rate));
        }
        if let InstrumentKind::Modular(after) = kind {
            self.sync_tables(track);
            let kinds_changed = match previous {
                InstrumentKind::Modular(before) => before.modules.map(|g| (g.kind, g.table)) != after.modules.map(|g| (g.kind, g.table)),
                _ => true,
            };
            if kinds_changed {
                self.sync_samples(track);
            }
        }
        if let Some(after) = kind.wave() {
            self.sync_tables(track);
            // The oscillators' files are sent as samples once their kind plays them.
            let kinds_changed = match previous.wave() {
                Some(before) => before.osc.map(|o| o.kind) != after.osc.map(|o| o.kind),
                None => true,
            };
            if kinds_changed {
                self.sync_samples(track);
            }
        }
        // Another instrument has other parameters: its automation no longer applies.
        if previous.label() != kind.label() {
            let target = Target::Track(track);
            self.automation_mut(target).retain(|l| !matches!(l.param, Param::Instrument(_)));
            self.overridden.retain(|(t, p)| !(*t == target && matches!(p, Param::Instrument(_))));
            self.sync_automation(target);
        }
    }

    /// Sends the wavetables the track's wavetable synth reads (built-in or loaded).
    pub fn sync_tables(&mut self, track: usize) {
        let tables = sync::instrument_tables(&self.project.tracks[track], &mut self.samples);
        self.audio.send(Command::SetWavetables { track, tables });
        self.show_sample_errors();
    }


    /// The table an oscillator plays, if loaded (wavetable tracks always have theirs).
    pub fn cached_wavetable(&self, track: usize, osc: usize) -> Option<Arc<engine::wavetable::Wavetable>> {
        sync::cached_table_for(&self.project.tracks[track], osc, &self.samples)
    }

    /// Loads a wavetable file into oscillator `osc` of a wavetable synth.
    pub fn choose_wavetable(&mut self, track: usize, osc: usize) {
        let Some(path) = crate::files::pick_file(rfd::FileDialog::new().add_filter("Wavetable", &AUDIO_EXTENSIONS)) else { return };
        let path = path.to_string_lossy().into_owned();
        let table = self.samples.table(&path);
        if table.frames() == 0 || self.samples.errors.last().is_some() {
            self.show_sample_errors();
            return;
        }
        let kind = self.project.tracks[track].instrument;
        let Some(mut params) = kind.wave().copied() else { return };
        params.osc[osc].table = engine::model::WAVE_USER_TABLE;
        self.project.tracks[track].set_sample(osc, Some(path));
        self.set_instrument(track, kind.with_wave(params));
        self.status = Some(format!("Wavetable: {} ({} frames)", table.name, table.frames()));
    }

    /// Connects modulator `modulator` of a modular synth to parameter `dest` (depth 50 %).
    pub fn add_modular_target(&mut self, track: usize, modulator: usize, dest: usize) {
        use engine::model::ModTarget;
        let InstrumentKind::Modular(mut params) = self.project.tracks[track].instrument else { return };
        let name = params.param_name(dest);
        let targets = &mut params.modulators[modulator].targets;
        if targets.iter().any(|t| t.dest as usize == dest) {
            self.status = Some(format!("Ya modula {name}"));
            return;
        }
        let Some(free) = targets.iter().position(|t| t.dest == engine::model::MOD_NO_TARGET) else {
            self.status = Some("Este modulador ya tiene 8 destinos".into());
            return;
        };
        targets[free] = ModTarget::new(dest, 0.5);
        self.set_instrument(track, InstrumentKind::Modular(params));
        self.status = Some(format!("→ {name} (50 %) · clic derecho en el knob para ajustar"));
    }

    /// Edits a modular synth's stack, moving the modules' files along with them.
    pub fn edit_stack(&mut self, track: usize, edit: crate::modular_ui::StackEdit) {
        use crate::modular_ui::StackEdit;
        use engine::model::MOD_MODULES;
        let InstrumentKind::Modular(mut params) = self.project.tracks[track].instrument else { return };
        let mut files: Vec<String> = (0..MOD_MODULES).map(|i| self.project.tracks[track].sample(i).unwrap_or_default().to_string()).collect();
        match edit {
            StackEdit::Insert(after, kind) => {
                let Some(at) = params.insert_module(after, engine::model::Module::new(kind)) else {
                    self.status = Some(format!("La pila admite {MOD_MODULES} módulos"));
                    return;
                };
                files.insert(at, String::new());
                files.truncate(MOD_MODULES);
            }
            StackEdit::Duplicate(i) => {
                let module = params.modules[i];
                let Some(at) = params.insert_module(Some(i), module) else { return };
                let file = files[i].clone();
                files.insert(at, file);
                files.truncate(MOD_MODULES);
            }
            StackEdit::Remove(i) => {
                params.remove_module(i);
                files.remove(i);
                files.push(String::new());
            }
            StackEdit::Move(i, up) => {
                let j = if up { i.checked_sub(1) } else { Some(i + 1).filter(|&j| j < MOD_MODULES && params.modules[j].kind != engine::model::ModuleKind::Empty) };
                let Some(j) = j else { return };
                params.swap_modules(i, j);
                files.swap(i, j);
            }
        }
        let t = &mut self.project.tracks[track];
        t.samples.clear();
        for (i, file) in files.into_iter().enumerate() {
            if !file.is_empty() {
                t.set_sample(i, Some(file));
            }
        }
        self.set_instrument(track, InstrumentKind::Modular(params));
        self.sync_samples(track);
        self.sync_tables(track);
    }

    /// Connects module `source`'s audio to `param` of module `target`.
    pub fn connect_audio(&mut self, track: usize, source: usize, target: usize, param: engine::model::AudioParam) {
        let InstrumentKind::Modular(mut params) = self.project.tracks[track].instrument else { return };
        if source == target {
            return;
        }
        match params.connect_audio(source, target, param) {
            Some(_) => {
                self.status = Some(format!("Audio {} → {} {} (50 %)", source + 1, target + 1, param.label()));
                self.set_instrument(track, InstrumentKind::Modular(params));
            }
            None => self.status = Some("No quedan conexiones de audio libres".into()),
        }
    }

    /// Loads the audio or wavetable file a modular generator plays.
    pub fn choose_gen_file(&mut self, track: usize, g: usize) {
        let Some(path) = crate::files::pick_file(rfd::FileDialog::new().add_filter("Audio", &AUDIO_EXTENSIONS)) else { return };
        let path = path.to_string_lossy().into_owned();
        if self.sample(&path).frames() == 0 {
            return;
        }
        let InstrumentKind::Modular(mut params) = self.project.tracks[track].instrument else { return };
        if params.modules[g].kind == engine::model::ModuleKind::Wavetable {
            params.modules[g].table = engine::model::WAVE_USER_TABLE;
        }
        self.project.tracks[track].set_sample(g, Some(path));
        self.set_instrument(track, InstrumentKind::Modular(params));
        self.sync_samples(track);
        self.sync_tables(track);
    }

    /// Routes `source` to parameter `dest` in the first free matrix slot (50 %).
    pub fn add_modulation(&mut self, track: usize, source: engine::model::ModSource, dest: usize) {
        use engine::model::{ModSlot, WAVE_PARAMS};
        let kind = self.project.tracks[track].instrument;
        let Some(mut params) = kind.wave().copied() else { return };
        let name = WAVE_PARAMS.get(dest).map_or("?", |p| p.name);
        if params.matrix.iter().any(|s| s.source == source && s.dest as usize == dest) {
            self.status = Some(format!("{} ya modula {name}", source.label()));
            return;
        }
        let Some(slot) = params.matrix.iter().position(|s| s.source == engine::model::ModSource::None) else {
            self.status = Some("La matriz de modulación está llena".into());
            return;
        };
        params.matrix[slot] = ModSlot { source, dest: dest as u8, amount: 0.5, bipolar: false };
        self.set_instrument(track, kind.with_wave(params));
        self.status = Some(format!("{} → {name} (50 %) · clic derecho en el knob para ajustar", source.label()));
    }

    /// Loads the audio file oscillator `osc` plays in sample, granular or spectral mode.
    pub fn choose_osc_sample(&mut self, track: usize, osc: usize) {
        let Some(path) = crate::files::pick_file(rfd::FileDialog::new().add_filter("Audio", &AUDIO_EXTENSIONS)) else { return };
        let path = path.to_string_lossy().into_owned();
        if self.sample(&path).frames() == 0 {
            return;
        }
        self.project.tracks[track].set_sample(osc, Some(path));
        self.sync_samples(track);
        self.sync_tables(track);
        self.status = Some(format!("Muestra del oscilador: {}", self.project.tracks[track].sample_name(osc).unwrap_or_default()));
    }

    pub fn sync_samples(&mut self, track: usize) {
        let samples = sync::instrument_samples(&self.project.tracks[track], &mut self.samples);
        self.audio.send(Command::SetSamples { track, samples });
        self.show_sample_errors();
    }

    /// Loads files into consecutive sample slots of a sampler or drum rack, starting at `slot`.
    pub fn set_instrument_samples(&mut self, track: usize, slot: usize, paths: Vec<PathBuf>) {
        // Several files on a sampler: spread over the keyboard.
        if let InstrumentKind::Sampler(params) = self.project.tracks[track].instrument {
            if paths.len() > 1 {
                return self.load_multisample(track, paths);
            }
            if params.zone_count > 0 {
                // One file: back to a single sample over the whole keyboard.
                self.project.tracks[track].samples.clear();
                self.set_instrument(track, InstrumentKind::Sampler(engine::model::SamplerParams { zone_count: 0, zones: Default::default(), ..params }));
            }
        }
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
            InstrumentKind::DrumRack(_) | InstrumentKind::Sampler(_) => crate::files::pick_files(dialog),
            _ => crate::files::pick_file(dialog).map(|p| vec![p]),
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

    /// A new folder for the slices of `name`, with the project's media.
    fn slices_dir(&self, name: &str) -> PathBuf {
        let base = self.media_dir(crate::library::SLICES);
        let clean: String = name.chars().map(|c| if c == '/' || c == ':' { '-' } else { c }).collect();
        (1..)
            .map(|n| base.join(if n == 1 { clean.clone() } else { format!("{clean} {n}") }))
            .find(|dir| !dir.exists())
            .expect("an unused folder name")
    }

    /// Slices a whole file into the drum rack on `track`, replacing its pads, and
    /// puts the clip that replays it in the first free session slot.
    pub fn slice_file_into(&mut self, track: usize) {
        let Some(path) = crate::files::pick_file(rfd::FileDialog::new().add_filter("Audio", &AUDIO_EXTENSIONS)) else { return };
        let sample = self.sample(&path.to_string_lossy());
        if sample.frames() == 0 {
            return;
        }
        let name = path.file_stem().map_or("Loop".into(), |s| s.to_string_lossy().into_owned());
        // Its tempo from its beats; a loop without clear ones by how long it is.
        let bpm = self
            .file_bpm(&path.to_string_lossy())
            .or_else(|| guess_loop_bpm(sample.duration(), self.project.tempo))
            .unwrap_or(self.project.tempo);
        let color = self.project.tracks[track].color;
        let region = (0.0, sample.duration());
        match crate::slicing::slice(&sample, region, bpm, self.slice_mode, &name, &self.slices_dir(&name), color) {
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
        let (region, bpm) = crate::slicing::clip_region(&audio, self.project.tempo);
        let mode = self.slice_mode;
        let dir = self.slices_dir(&audio.name);
        // The cuts edited by hand, if they are shown for this clip.
        let edited = self.show_cuts.then(|| self.cuts_for(clip_ref)).flatten();
        let result = match &edited {
            Some(cuts) => crate::slicing::slice_at(&sample, region, bpm, cuts, &audio.name, &dir, audio.color).map(|s| (s, 0.0)),
            None => crate::slicing::slice(&sample, region, bpm, mode, &audio.name, &dir, audio.color),
        };
        let (mut sliced, grid) = match result {
            Ok(result) => result,
            Err(error) => {
                self.status = Some(error);
                return;
            }
        };
        // As long as the audio clip, so the two loop together (the file may end
        // before the clip's last bar).
        sliced.clip.length = audio.length;
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
        // The copy gets plugin instances of its own, starting from the same state.
        self.capture_plugins();
        let fresh = |project: &mut Project, id: PluginId| -> PluginId {
            let new = project.new_plugin_id();
            if let Some(mut plugin) = project.plugin(id).cloned() {
                plugin.id = new;
                project.plugins.push(plugin);
            }
            new
        };
        if let InstrumentKind::Plugin(id) = copy.instrument {
            copy.instrument = InstrumentKind::Plugin(fresh(&mut self.project, id));
        }
        for device in &mut copy.devices {
            if let Effect::Plugin(id) = device.effect {
                device.effect = Effect::Plugin(fresh(&mut self.project, id));
            }
        }
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
        self.midi = MidiInputs::connect(self.audio.live(), self.learn.shared.clone());
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
            self.capture_failed = false;
            self.ensure_capture();
        }
        self.sync_monitor(track);
    }

    /// Sets when an audio track plays its input.
    pub fn set_monitor(&mut self, track: usize, monitor: crate::project::Monitor) {
        let Some(t) = self.project.tracks.get_mut(track) else { return };
        t.monitor = monitor;
        self.status = Some(monitor.hint().into());
        if t.monitors() {
            self.capture_failed = false;
            self.ensure_capture();
        }
        self.sync_monitor(track);
    }

    fn sync_monitor(&mut self, track: usize) {
        let Some(t) = self.project.tracks.get(track) else { return };
        let on = t.monitors() && t.frozen.is_none();
        self.audio.send(Command::SetMonitor { track, on });
    }

    /// Opens the audio input (macOS asks for microphone permission the first time).
    fn ensure_capture(&mut self) -> bool {
        if self.capture.is_some() {
            return true;
        }
        let Some(state) = self.audio.state() else { return false };
        match InputCapture::open(state, &self.settings.audio) {
            Ok(mut capture) => {
                self.status = Some(format!("Entrada de audio: {}", capture.device_name()));
                // The engine resamples it if the input runs at another rate than the output.
                if let Some(monitor) = capture.take_monitor() {
                    self.audio.send(Command::SetMonitorInput(Some(Box::new(monitor))));
                }
                self.capture = Some(capture);
                true
            }
            Err(error) => {
                self.status = Some(format!("No se pudo abrir la entrada de audio: {error}"));
                self.capture_failed = true;
                false
            }
        }
    }

    /// Keeps the input flowing while audio tracks are armed; closes it otherwise.
    fn pump_capture(&mut self) {
        let wanted = self.project.tracks.iter().any(|t| (t.armed && t.is_audio()) || t.monitors())
            || self.recorder.session.iter().any(|take| self.project.tracks.get(take.track).is_some_and(|t| t.is_audio()));
        if !wanted {
            if self.capture.take().is_some() {
                self.audio.send(Command::SetMonitorInput(None));
            }
            return;
        }
        // A project opened with tracks that monitor: open the input for them (once).
        if self.capture.is_none() && !self.capture_failed && self.project.tracks.iter().any(|t| t.monitors()) {
            self.ensure_capture();
        }
        let keep_all = !self.recorder.session.is_empty() || self.recorder.arrangement.is_some();
        if let Some(capture) = &mut self.capture {
            capture.pump(keep_all);
        }
    }

    /// Where the project keeps media of `kind` (see `library`): its folder's
    /// `Samples/`, or Pulse's `Sin guardar/` until it is saved.
    pub(crate) fn media_dir(&self, kind: &str) -> PathBuf {
        crate::library::media_dir(self.file.as_deref(), kind)
    }

    /// Folder for recorded takes.
    pub(crate) fn recordings_dir(&self) -> PathBuf {
        self.media_dir(crate::library::RECORDINGS)
    }

    /// Cuts `beats` of input starting at `start` (transport beats) into a new audio clip.
    fn audio_take(&mut self, track: usize, start: f64, beats: f64) -> Option<AudioClip> {
        let (left, right, _) = self.take_audio(start, beats, 0)?;
        self.save_take(track, left, right, beats)
    }

    /// The input played while `beats` from `start` were heard, from input frame
    /// `after` on, and the frame (before latency) where `start` was rendered.
    fn take_audio(&self, start: f64, beats: f64, after: u64) -> Option<(Vec<f32>, Vec<f32>, u64)> {
        let capture = self.capture.as_ref()?;
        let rate = capture.sample_rate();
        // The take is what was played while `start` was heard: the input got it this
        // much later than the engine rendered that beat.
        let latency = self.audio.state().map_or(0.0, |s| s.output_latency()) + capture.latency() + self.settings.record_offset_ms as f64 / 1000.0;
        let rendered = capture.frame_at_beat_after(start, after)?;
        let first = (rendered as i64 + (latency * rate as f64).round() as i64).max(0) as u64;
        let (left, right) = capture.extract(first, self.take_frames(beats));
        Some((left, right, rendered))
    }

    fn take_frames(&self, beats: f64) -> usize {
        let rate = self.capture.as_ref().map_or(48_000.0, |c| c.sample_rate());
        (beats * 60.0 / self.project.tempo * rate as f64).round() as usize
    }

    /// Writes a take to the recordings folder and makes it a clip of `beats`.
    fn save_take(&mut self, track: usize, left: Vec<f32>, right: Vec<f32>, beats: f64) -> Option<AudioClip> {
        let tempo = self.project.tempo;
        let rate = self.capture.as_ref()?.sample_rate();
        let dir = self.recordings_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.status = Some(format!("No se pudo crear {}: {e}", dir.display()));
            return None;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        // Takes of one loop recording are written in the same second.
        let base = format!("{} {stamp}", self.project.tracks[track].name);
        let (name, path) = (1..)
            .map(|n| if n == 1 { base.clone() } else { format!("{base} {n}") })
            .map(|name| (name.clone(), dir.join(format!("{name}.wav"))))
            .find(|(_, path)| !path.exists())
            .expect("a free name");
        if let Err(error) = write_wav(&path, &left, &right, rate) {
            self.status = Some(error);
            return None;
        }
        let key = path.to_string_lossy().into_owned();
        self.samples.insert(key.clone(), Arc::new(Sample::from_channels(left, right, rate)));
        Some(AudioClip {
            envelopes: Vec::new(),
            name,
            color: self.project.tracks[track].color,
            sample: key,
            length: beats,
            offset: 0.0,
            // Recorded at the project tempo; Stretch keeps the pitch if the tempo changes later.
            warp: WarpMode::Stretch,
            original_bpm: tempo,
            warp_markers: Vec::new(),
            key: None,
            takes: Vec::new(),
            gain_db: 0.0,
            transpose: 0.0,
        })
    }

    /// Plays take `k` of a loop recording's clip.
    pub fn choose_take(&mut self, track: usize, index: usize, k: usize) {
        let Some(placed) = self.project.tracks.get_mut(track).and_then(|t| t.arrangement.get_mut(index)) else { return };
        let Clip::Audio(audio) = &mut placed.clip else { return };
        let Some(take) = audio.takes.get(k).cloned() else { return };
        audio.sample = take;
        audio.name = format!("Toma {}", k + 1);
        self.sync_arrangement(track);
        self.status = Some(format!("Suena la toma {}", k + 1));
    }

    /// The clips of an audio recording over the loop: each pass is a take of the
    /// loop (silent where the pass did not play), the last one playing; what was
    /// played before the loop is a clip of its own.
    fn loop_takes(&mut self, track: usize, passes: &[(f64, f64)], (loop_start, loop_end): (f64, f64)) -> Vec<ArrangementClip> {
        let mut clips = Vec::new();
        let mut takes: Vec<AudioClip> = Vec::new();
        let mut after = 0;
        for &(from, to) in passes {
            if to - from <= 1e-6 {
                continue;
            }
            let Some((left, right, rendered)) = self.take_audio(from, to - from, after) else { continue };
            // The next pass is looked for from where this one ended (the markers
            // across the wrap don't count, so landing a block off is harmless).
            after = rendered + self.take_frames(to - from) as u64;
            let mut skip = 0;
            if from < loop_start - 1e-6 {
                skip = self.take_frames(loop_start - from).min(left.len());
                let beats = loop_start - from;
                if let Some(clip) = self.save_take(track, left[..skip].to_vec(), right[..skip].to_vec(), beats) {
                    clips.push(ArrangementClip { start: from, length: beats, clip: Clip::Audio(clip) });
                }
            }
            let begin = from.max(loop_start);
            // Too little of the loop to be a take (stopped right after it wrapped).
            if to - begin < 0.5 {
                continue;
            }
            let frames = self.take_frames(loop_end - loop_start);
            let offset = self.take_frames(begin - loop_start).min(frames);
            let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
            let n = (left.len() - skip).min(frames - offset);
            l[offset..offset + n].copy_from_slice(&left[skip..skip + n]);
            r[offset..offset + n].copy_from_slice(&right[skip..skip + n]);
            if let Some(clip) = self.save_take(track, l, r, loop_end - loop_start) {
                takes.push(clip);
            }
        }
        if let Some(mut last) = takes.pop() {
            if !takes.is_empty() {
                takes.push(last.clone());
                last.takes = takes.iter().map(|t| t.sample.clone()).collect();
                last.name = format!("Toma {}", last.takes.len());
            }
            clips.push(ArrangementClip { start: loop_start, length: loop_end - loop_start, clip: Clip::Audio(last) });
        }
        clips
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
        // Nothing armed: the selected tracks are, as in Live (unless session clips
        // are playing: recording them onto the arrangement is enough).
        let session_playing = self.audio.state().is_some_and(|s| (0..self.project.tracks.len()).any(|t| s.tracks[t].playing.load(Ordering::Relaxed) != engine::NONE));
        if !self.project.tracks.iter().any(|t| t.armed) && !session_playing {
            let selection = self.track_selection();
            if selection.is_empty() {
                self.status = Some("Selecciona una pista (o ármala con su botón ●) para grabar en ella".into());
                return;
            }
            for t in selection {
                self.toggle_arm(t);
            }
        }
        if self.project.tracks.iter().any(|t| t.armed && t.is_audio()) && !self.ensure_capture() {
            return;
        }
        self.recorder.start_arrangement(self.audio.position());
        self.start_session_capture(self.audio.position());
        if self.settings.automation_arm {
            self.start_automation_take();
        }
        self.audio.send(Command::SetArrangementRecord(true));
        self.audio.send(Command::Play);
        let bar = |beat: f64| beat / 4.0 + 1.0;
        match self.project.punch() {
            (Some(i), Some(o)) => self.status = Some(format!("Punch: graba del compás {} al {}", bar(i), bar(o))),
            (Some(i), None) => self.status = Some(format!("Punch in: empieza a grabar en el compás {}", bar(i))),
            (None, Some(o)) => self.status = Some(format!("Punch out: deja de grabar en el compás {}", bar(o))),
            (None, None) => {}
        }
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

    pub(crate) fn update_recording(&mut self) {
        let Some(state) = self.audio.state() else { return };
        self.pump_capture();
        let punch = self.project.punch();
        let position = state.position();
        let inside = punch.0.is_none_or(|p| position >= p) && punch.1.is_none_or(|p| position < p);
        if self.automation_take.is_some() && state.arrangement_recording.load(Ordering::Relaxed) && inside {
            self.record_automation(position);
        }
        // Past the punch-out (the loop not playing, or it would have jumped back):
        // recording stops, playback goes on.
        if let (Some(out), Some(take)) = (punch.1, self.recorder.arrangement.as_mut()) {
            if position >= out && !take.punched_out && !self.project.loop_enabled && state.arrangement_recording.load(Ordering::Relaxed) {
                take.punched_out = true;
                self.audio.send(Command::SetArrangementRecord(false));
            }
        }
        // Notes were played while hearing what the engine rendered a moment before.
        let lag = state.output_latency() * self.project.tempo / 60.0;
        let notes: Vec<_> = self.audio.take_recorded().into_iter().map(|n| engine::RecordedNote { start: (n.start - lag).max(0.0), ..n }).collect();
        let armed: Vec<bool> = self.project.tracks.iter().map(|t| t.armed && !t.is_audio()).collect();
        let looped = self.project.loop_enabled.then_some((self.project.loop_start, self.project.loop_end));
        if state.arrangement_recording.load(Ordering::Relaxed) {
            self.follow_session_capture(&state);
        }
        let finished = self.recorder.update(notes, &state, &armed, looped, punch);
        let recording: Vec<(usize, usize, f64)> = self
            .recorder
            .session
            .iter()
            .filter(|take| state.tracks[take.track].recording.load(Ordering::Relaxed) == take.slot as i32)
            .filter_map(|take| take.start.map(|start| (take.track, take.slot, start)))
            .collect();
        self.record_clip_automation(state.position(), &recording);
        for take in finished {
            match take {
                Finished::Session { track, slot, start, length, notes } => {
                    let Some(t) = self.project.tracks.get(track) else { continue };
                    if t.clips[slot].is_some() {
                        continue;
                    }
                    let audio = t.is_audio();
                    let envelopes = self.take_clip_envelopes(track, slot, length);
                    let moved = envelopes.len();
                    if audio {
                        if let Some(mut clip) = self.audio_take(track, start, length) {
                            clip.envelopes = envelopes;
                            self.set_clip(track, slot, Some(Clip::Audio(clip)));
                            self.select_clip(ClipRef::Session { track, slot });
                            self.status = Some(format!("Toma de audio guardada en {}", self.recordings_dir().display()));
                        }
                        continue;
                    }
                    let count = notes.len();
                    let notes = self.quantize_take(notes, start, length);
                    let t = &self.project.tracks[track];
                    let clip = MidiClip { name: format!("Toma {}", slot + 1), length, notes, color: t.color, envelopes, groove: None };
                    self.set_clip(track, slot, Some(Clip::Midi(clip)));
                    self.select_clip(ClipRef::Session { track, slot });
                    let automation = match moved {
                        0 => String::new(),
                        1 => " y la automatización de 1 control".into(),
                        n => format!(" y la automatización de {n} controles"),
                    };
                    self.status = Some(format!("Grabadas {count} notas{automation} en {}", self.project.tracks[track].name));
                }
                Finished::Arrangement { track, start, length, notes } => {
                    let notes = self.quantize_take(notes, start, length);
                    let Some(t) = self.project.tracks.get_mut(track) else { continue };
                    let count = notes.len();
                    let clip = MidiClip { name: "Toma".into(), length, notes, color: t.color, envelopes: Vec::new(), groove: None };
                    t.arrangement.push(ArrangementClip { start, length, clip: Clip::Midi(clip) });
                    let index = t.arrangement.len() - 1;
                    self.sync_arrangement(track);
                    self.select_clip(ClipRef::Arrangement { track, index });
                    self.status = Some(format!("Grabadas {count} notas en el Arrangement"));
                }
                Finished::ArrangementEnded { passes } => {
                    let clips = self.finish_session_capture();
                    if clips > 0 {
                        self.status = Some(match clips {
                            1 => "Grabado 1 clip de Session en el Arrangement".to_string(),
                            n => format!("Grabados {n} clips de Session en el Arrangement"),
                        });
                    }
                    let moved = self.finish_automation_take();
                    if moved > 0 {
                        self.status = Some(match moved {
                            1 => "Grabada la automatización de 1 control".to_string(),
                            n => format!("Grabada la automatización de {n} controles"),
                        });
                    }
                    // Nothing between the punch-in and the punch-out.
                    let (Some(&(start, _)), Some(&(_, end))) = (passes.first(), passes.last()) else { continue };
                    let beats = ((end - start) * 4.0).round() / 4.0;
                    if passes.len() == 1 && beats <= 0.0 {
                        continue;
                    }
                    let tracks: Vec<usize> = (0..self.project.tracks.len())
                        .filter(|&t| self.project.tracks[t].armed && self.project.tracks[t].is_audio())
                        .collect();
                    for track in tracks {
                        let clips = match looped {
                            Some(region) if passes.len() > 1 => self.loop_takes(track, &passes, region),
                            _ => self.audio_take(track, start, beats).map(|clip| ArrangementClip { start, length: beats, clip: Clip::Audio(clip) }).into_iter().collect(),
                        };
                        let Some(last) = clips.last() else { continue };
                        let takes = match &last.clip {
                            Clip::Audio(a) => a.takes.len(),
                            Clip::Midi(_) => 0,
                        };
                        let lane = &mut self.project.tracks[track].arrangement;
                        lane.extend(clips);
                        let index = lane.len() - 1;
                        self.sync_arrangement(track);
                        self.select_clip(ClipRef::Arrangement { track, index });
                        if takes > 1 {
                            self.arrangement.takes.insert(track);
                        }
                        self.status = Some(if takes > 1 {
                            format!("{takes} tomas grabadas en el loop: arrastra en sus filas para quedarte con lo mejor de cada una")
                        } else {
                            format!("Toma de audio guardada en {}", self.recordings_dir().display())
                        });
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
        let dialog = rfd::FileDialog::new().add_filter("Proyecto Pulse", &["pulse"]).set_directory(crate::library::folder(crate::library::PROJECTS));
        if let Some(path) = crate::files::pick_file(dialog) {
            self.open_from(&path);
        }
    }

    pub fn import_live_set(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter("Set de Ableton Live", &["als"]);
        if let Some(path) = crate::files::pick_file(dialog) {
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

    /// Whether the system menu bar has the commands (otherwise the in-app menu does).
    pub fn has_menu_bar(&self) -> bool {
        self.menu_bar.is_some()
    }

    /// Opens a project file (without asking about unsaved changes: see `commands`).
    pub(crate) fn open_path(&mut self, path: &std::path::Path) {
        self.open_from(path);
    }

    fn open_from(&mut self, path: &std::path::Path) {
        match Project::load(path) {
            Ok(project) => {
                self.status = Some(format!("Abierto {}", path.display()));
                self.replace_project(project, Some(path.to_path_buf()));
                self.remember_recent(path);
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
    pub(crate) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        let (audio, other): (Vec<PathBuf>, Vec<PathBuf>) = dropped.into_iter().partition(|p| is_audio_file(p));
        if !audio.is_empty() {
            // Where the mouse is (macOS knows; the drop doesn't say): the slot, lane
            // or pad under it, as when dragging from the browser.
            let pos = if self.script.is_some() { ctx.pointer_latest_pos() } else { crate::open_files::pointer_in_window() };
            if pos.is_some_and(|p| self.drop_files(audio.clone(), p)) {
                // Placed.
            } else {
                match self.instrument_drop_slot(ctx) {
                    Some((track, slot)) => self.set_instrument_samples(track, slot, audio),
                    None => self.add_samples(audio, Placement::Auto),
                }
            }
        }
        self.open_documents(other);
    }

    /// Opens projects (dropped on the window or sent by Finder), asking first about
    /// unsaved changes.
    pub(crate) fn open_documents(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
                Some("als") if self.may_discard() => self.import_live_set_from(&path),
                Some("pulse") if self.may_discard() => self.open_from(&path),
                Some("als" | "pulse") => {}
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

    /// Saves under a new name: in Pulse's Proyectos folder unless another is
    /// chosen, and always in a folder of the project's own (see `library`).
    pub fn save_as(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter("Proyecto Pulse", &["pulse"])
            .set_directory(crate::library::folder(crate::library::PROJECTS))
            .set_file_name(format!("{}.pulse", self.file_name()));
        if let Some(path) = crate::files::save_file(dialog) {
            self.save_to(crate::library::project_file(&path));
        }
    }

    #[cfg(test)]
    pub fn save_to_path(&mut self, path: &Path) {
        self.save_to(path.to_path_buf());
    }

    #[cfg(test)]
    pub fn commit_history(&mut self) {
        self.history.commit(&self.project, false);
    }

    fn save_to(&mut self, path: PathBuf) {
        self.capture_plugins();
        // Every audio file it uses goes into its folder (a lone .pulse from before
        // stays as it was: "Guardar como…" gives it a folder).
        let collected = match path.parent().filter(|_| crate::library::in_own_folder(&path)) {
            Some(dir) => {
                let before = self.project.clone();
                let collected = crate::library::collect(&mut self.project, dir);
                if self.project != before {
                    self.history.commit(&self.project, false);
                    self.sync_all();
                }
                Some(collected)
            }
            None => None,
        };
        match self.project.save(&path) {
            Ok(()) => {
                self.saved = self.project.clone();
                let mut status = format!("Guardado en {}", path.display());
                match collected {
                    Some(c) if c.copied + c.moved > 0 => status += &format!(" · {} audio(s) traídos a su carpeta", c.copied + c.moved),
                    None => status += " · usa «Guardar como…» para darle su propia carpeta con sus audios",
                    _ => {}
                }
                if let Some(c) = collected.filter(|c| c.missing > 0) {
                    status += &format!(" · {} audio(s) no encontrados", c.missing);
                }
                self.status = Some(status);
                self.remember_recent(&path);
                self.file = Some(path);
                self.auto_version();
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

    /// The input's latency (if it is open) and the output's, in seconds.
    pub fn latencies(&self) -> (Option<f64>, f64) {
        let output = self.audio.state().map_or(0.0, |s| s.output_latency());
        (self.capture.as_ref().map(|c| c.latency()), output)
    }

    // ----- Frame ------------------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            self.tab_pressed = None;
            return;
        }
        let input = ctx.input(|i| {
            let cmd = i.modifiers.command;
            let shift = i.modifiers.shift;
            [
                i.key_pressed(egui::Key::Space),
                self.tab_pressed == Some(false),
                self.tab_pressed == Some(true),
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                cmd && i.key_pressed(egui::Key::D),
                cmd && !shift && i.key_pressed(egui::Key::Z),
                cmd && shift && i.key_pressed(egui::Key::Z),
                cmd && !shift && !i.modifiers.alt && i.key_pressed(egui::Key::S),
                cmd && shift && i.key_pressed(egui::Key::S),
                cmd && i.key_pressed(egui::Key::O),
                cmd && i.key_pressed(egui::Key::N),
                cmd && !shift && i.key_pressed(egui::Key::T),
                cmd && shift && i.key_pressed(egui::Key::T),
                cmd && i.key_pressed(egui::Key::I),
                cmd && shift && i.key_pressed(egui::Key::R),
                !cmd && i.key_pressed(egui::Key::M),
                i.key_pressed(egui::Key::F9),
                cmd && i.modifiers.alt && i.key_pressed(egui::Key::S),
            ]
        });
        let [space, tab, shift_tab, delete, duplicate, undo, redo, save, save_as, open, new, add_track, add_audio, import, export, keyboard, record, versions] =
            input;
        self.tab_pressed = None;
        if versions {
            self.open_panel(Panel::Versions);
        }
        if export {
            self.open_panel(Panel::Export);
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
            self.run(crate::commands::Command::ImportLive, ctx);
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
            self.run(crate::commands::Command::Open, ctx);
        }
        if new {
            self.run(crate::commands::Command::New, ctx);
        }
        if add_track {
            self.add_track();
        }
    }

    /// A file dragged from the browser: its name follows the pointer, and it lands
    /// where the button is let go.
    fn follow_browser_drag(&mut self, ctx: &egui::Context) {
        let Some(dragged) = egui::DragAndDrop::payload::<crate::browser::Dragged>(ctx) else { return };
        let Some(pos) = ctx.pointer_latest_pos() else { return };
        let name = dragged.0.file_stem().map_or(String::new(), |n| n.to_string_lossy().into_owned());
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("browser_drag")));
        let text = painter.layout_no_wrap(format!("♪ {name}"), theme::body(), theme::TEXT);
        let rect = egui::Rect::from_min_size(pos + egui::vec2(12.0, 8.0), text.size() + egui::vec2(10.0, 4.0));
        painter.rect_filled(rect, 3.0, theme::CONTROL);
        painter.galley(rect.min + egui::vec2(5.0, 2.0), text, theme::TEXT);
        if ctx.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(ctx);
            self.drop_from_browser((*dragged).clone(), pos);
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
        // Frames come one after another, not only on input.
        ctx.request_repaint();
        if *frames == 60 {
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
            // Nothing to save: the scene's changes are throwaway.
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for PulseApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.script_input(ctx, raw_input);
        // Tab is Pulse's (Session/Arrangement, Clip/Devices), not egui's focus
        // navigation: that would leave the tempo field focused, and the shortcuts
        // off until a click elsewhere. Inside a text field it stays egui's.
        if !ctx.egui_wants_keyboard_input() {
            raw_input.events.retain(|event| match event {
                egui::Event::Key { key: egui::Key::Tab, pressed, modifiers, .. } => {
                    if *pressed {
                        self.tab_pressed = Some(modifiers.shift);
                    }
                    false
                }
                _ => true,
            });
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::widgets::set_learning(&ctx, self.learn.on);
        // The menu bar's choices, and the window's close button.
        let chosen = self.menu_bar.as_ref().map(|bar| bar.chosen()).unwrap_or_default();
        for command in chosen {
            self.run(command, &ctx);
        }
        self.handle_close_request(&ctx);
        self.audio.collect_garbage();
        if let Some(state) = self.audio.state() {
            let dt = ctx.input(|i| i.stable_dt).min(0.1);
            self.meters.update(&state, dt);
        }
        self.handle_shortcuts(&ctx);
        self.handle_dropped_files(&ctx);
        // Sent by Finder or the Dock: projects open, audio files land as samples.
        let (audio, opened): (Vec<PathBuf>, Vec<PathBuf>) = crate::open_files::take().into_iter().partition(|p| is_audio_file(p));
        if !audio.is_empty() {
            self.add_samples(audio, Placement::Auto);
        }
        self.open_documents(opened);
        self.sample_drop_zones.clear();
        self.drop_targets = Default::default();
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
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            self.drop_targets.view = Some(ui.max_rect());
            match self.view {
                View::Session => session::show(self, ui),
                View::Arrangement => arrangement::show(self, ui),
            }
        });
        crate::kick_ui::window(self, &ctx);
        crate::wave_ui::window(self, &ctx);
        crate::poly_ui::window(self, &ctx);
        crate::modular_ui::window(self, &ctx);
        crate::table_editor::window(self, &ctx);

        crate::plugins::window(self, &ctx);
        self.poll_editors();
        self.show_plugin_automation();
        self.poll_midi_map();
        crate::midi_map::window(self, &ctx);
        crate::commands::shortcuts_window(self, &ctx);
        crate::guide::window(self, &ctx);
        crate::versions::window(self, &ctx);
        crate::mix_assist::window(self, &ctx);
        crate::settings::window(self, &ctx);
        self.follow_updates();
        self.follow_stems(&ctx);
        self.follow_to_midi(&ctx);
        self.follow_mix(&ctx);
        self.follow_tempo(&ctx);
        self.show_notice(&ctx);
        crate::updates::window(self, &ctx);
        self.text_focus = ctx.egui_wants_keyboard_input();
        if let Some(mut bar) = self.menu_bar.take() {
            bar.update(|c| self.command_enabled(c), &self.recent);
            self.menu_bar = Some(bar);
        }

        // A sweep of a controller is one undo step, like a drag.
        let interacting = ctx.input(|i| i.pointer.any_down()) || self.learn.moving();
        if self.history.commit(&self.project, interacting) {
            self.check_frozen();
        }
        self.follow_freeze(&ctx);
        self.update_title(&ctx);
        crate::export::window(self, &ctx);
        self.follow_browser_drag(&ctx);
        self.debug_screenshot(&ctx);
        self.script_output(&ctx);
        // Meters and playheads move while anything plays or sounds (60 frames a
        // second is plenty); otherwise a few frames a second are enough for what
        // comes in without input (MIDI, the menu bar, background jobs). Every vsync
        // (144 Hz on some displays) kept a core a fifth busy doing nothing.
        let moving = self.audio.is_playing() || self.meters.sounding() || self.capture.is_some();
        ctx.request_repaint_after(std::time::Duration::from_millis(if moving { 16 } else { 100 }));
    }
}

/// The track of a target, if it is one.
fn target_track(target: Target) -> Option<usize> {
    match target {
        Target::Track(t) => Some(t),
        _ => None,
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;

    /// One instrument editor and one of the app's windows at a time: they don't stack.
    #[test]
    fn windows_open_one_at_a_time() {
        let mut app = PulseApp::headless(Project::demo());
        app.open_instrument_editor(InstrumentEditor::Kick, 0);
        app.open_instrument_editor(InstrumentEditor::Wave, 1);
        assert_eq!((app.kick_window, app.wave_window, app.poly_window, app.modular_window), (None, Some(1), None, None));
        app.open_panel(Panel::Settings);
        app.open_panel(Panel::Shortcuts);
        assert!(app.show_shortcuts && !app.show_settings && !app.updates.show && !app.export.open);
        assert_eq!(app.wave_window, Some(1), "an editor and a window can be open together");
    }
}
