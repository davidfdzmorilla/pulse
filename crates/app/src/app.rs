use crate::project::Project;
use crate::{device, piano_roll, session, transport};
use eframe::egui;
use engine::model::MidiClip;
use engine::{AudioOutput, Command, EngineHandle, SharedState, MAX_TRACKS};
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

    pub fn device_name(&self) -> Option<&str> {
        self.handle.as_ref().map(|h| h.device_name.as_str())
    }

    fn collect_garbage(&mut self) {
        if let Some(handle) = &mut self.handle {
            handle.collect_garbage();
        }
    }
}

pub struct PulseApp {
    pub project: Project,
    pub audio: Audio,
    /// Clip open in the piano roll, as (track, slot).
    pub selected_clip: Option<(usize, usize)>,
    pub selected_track: usize,
    /// Decaying peak meters, refreshed every frame.
    pub meters: [f32; MAX_TRACKS],
    pub master_meter: f32,
    pub piano_roll: piano_roll::State,
    /// Debug aid: with `PULSE_SCREENSHOT=<file.ppm>` the app launches scene 2,
    /// saves a screenshot of its own window after a few seconds and quits.
    screenshot: Option<(String, u32)>,
}

impl PulseApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let mut app = Self {
            project: Project::demo(),
            audio: Audio::start(),
            selected_clip: Some((0, 0)),
            selected_track: 0,
            meters: [0.0; MAX_TRACKS],
            master_meter: 0.0,
            piano_roll: Default::default(),
            screenshot: std::env::var("PULSE_SCREENSHOT").ok().map(|path| (path, 0)),
        };
        app.sync_all();
        if app.screenshot.is_some() {
            app.audio.send(Command::LaunchScene(1));
        }
        app
    }

    /// Pushes the whole project to the engine.
    fn sync_all(&mut self) {
        let p = &self.project;
        let mut commands = vec![
            Command::SetTempo(p.tempo),
            Command::SetMetronome(p.metronome),
            Command::SetQuantize(p.quantize),
            Command::SetMasterVolume(p.master_volume),
        ];
        for (index, track) in p.tracks.iter().enumerate() {
            commands.push(Command::SetInstrument { track: index, kind: track.instrument });
            commands.push(Command::SetVolume { track: index, volume: track.volume });
            commands.push(Command::SetPan { track: index, pan: track.pan });
            commands.push(Command::SetMute { track: index, mute: track.mute });
        }
        for command in commands {
            self.audio.send(command);
        }
        for track in 0..self.project.tracks.len() {
            for slot in 0..engine::NUM_SCENES {
                self.sync_clip(track, slot);
            }
        }
    }

    /// Sends the current contents of a slot to the engine.
    pub fn sync_clip(&mut self, track: usize, slot: usize) {
        let clip = self.project.tracks[track].clips[slot].clone().map(Box::new);
        self.audio.send(Command::SetClip { track, slot, clip });
    }

    pub fn set_clip(&mut self, track: usize, slot: usize, clip: Option<MidiClip>) {
        self.project.tracks[track].clips[slot] = clip;
        if self.selected_clip == Some((track, slot)) && self.project.tracks[track].clips[slot].is_none() {
            self.selected_clip = None;
        }
        self.sync_clip(track, slot);
    }

    pub fn toggle_play(&mut self) {
        let playing = self.audio.state().is_some_and(|s| s.playing.load(std::sync::atomic::Ordering::Relaxed));
        self.audio.send(if playing { Command::Stop } else { Command::Play });
    }

    fn update_meters(&mut self) {
        let Some(state) = self.audio.state() else { return };
        for (meter, status) in self.meters.iter_mut().zip(&state.tracks) {
            *meter = status.take_peak().max(*meter * 0.85);
        }
        self.master_meter = state.take_master_peak().max(self.master_meter * 0.85);
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

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (space, delete) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
            )
        });
        if space {
            self.toggle_play();
        }
        if delete {
            if let Some((track, slot)) = self.selected_clip {
                self.set_clip(track, slot, None);
            }
        }
    }
}

impl eframe::App for PulseApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.audio.collect_garbage();
        self.update_meters();
        self.handle_shortcuts(&ctx);

        egui::Panel::top("transport").show(ui, |ui| transport::show(self, ui));
        egui::Panel::bottom("detail")
            .resizable(true)
            .default_size(320.0)
            .min_size(180.0)
            .show(ui, |ui| {
                egui::Panel::left("device")
                    .resizable(true)
                    .default_size(270.0)
                    .show(ui, |ui| device::show(self, ui));
                egui::CentralPanel::default().show(ui, |ui| piano_roll::show(self, ui));
            });
        egui::CentralPanel::default().show(ui, |ui| session::show(self, ui));

        self.debug_screenshot(&ctx);
        // Meters and playheads animate continuously.
        ctx.request_repaint();
    }
}
