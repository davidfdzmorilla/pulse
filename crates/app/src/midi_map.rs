//! MIDI learn: knobs, faders and buttons of a MIDI controller drive Pulse's controls.
//!
//! With "Asignar" on, touching a control of Pulse (a knob, a fader, a plugin
//! parameter or a transport button) and then moving a knob or pressing a button on
//! the controller links them. Assigned controllers no longer reach the instrument;
//! the rest still do. Parameter assignments belong to the project; transport ones to
//! the controller, so they are kept for every project.

use crate::app::PulseApp;
use crate::theme::TEXT_DIM;
use eframe::egui;
use engine::model::{Param, PluginId};
use engine::{Command, Target};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Something a controller can drive.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Control {
    Param { target: Target, param: Param },
    Plugin { id: PluginId, param: u32 },
    Transport(Transport),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transport {
    /// Play, or stop if playing.
    Play,
    Stop,
    Record,
    Loop,
    Metronome,
}

impl Transport {
    pub fn label(self) -> &'static str {
        match self {
            Transport::Play => "Reproducir",
            Transport::Stop => "Parar",
            Transport::Record => "Grabar",
            Transport::Loop => "Loop",
            Transport::Metronome => "Metrónomo",
        }
    }
}

/// A controller (CC number, any channel) linked to a control.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mapping {
    pub cc: u8,
    pub control: Control,
}

/// What the MIDI input threads and the UI share.
#[derive(Clone)]
pub struct Shared {
    /// Controllers for the UI: (CC, value 0..127), in order.
    pub incoming: Arc<Mutex<Vec<(u8, u8)>>>,
    /// Controllers the UI takes instead of the instrument.
    pub taken: Arc<Mutex<[bool; 128]>>,
    /// While learning, every controller goes to the UI.
    pub learning: Arc<AtomicBool>,
}

impl Default for Shared {
    fn default() -> Self {
        Self { incoming: Default::default(), taken: Arc::new(Mutex::new([false; 128])), learning: Default::default() }
    }
}

impl Shared {
    /// Whether controller `cc` goes to the UI (and not to the instrument). Called on
    /// the MIDI input thread.
    pub fn take(&self, cc: u8, value: u8) -> bool {
        let taken = self.learning.load(Ordering::Relaxed) || self.taken.lock().map(|t| t[cc as usize & 127]).unwrap_or(false);
        if taken {
            if let Ok(mut incoming) = self.incoming.lock() {
                if incoming.len() < 4096 {
                    incoming.push((cc, value));
                }
            }
        }
        taken
    }
}

/// The MIDI learn state of the app.
#[derive(Default)]
pub struct Learn {
    pub on: bool,
    /// The control touched last, waiting for a controller.
    pub armed: Option<Control>,
    /// Transport assignments, kept across projects.
    pub transport: Vec<Mapping>,
    pub shared: Shared,
    /// When a controller last moved something (so a sweep is one undo step).
    pub last_move: Option<Instant>,
    /// Value last seen per controller, to tell presses from releases.
    last_value: Vec<(u8, u8, Instant)>,
    /// Where transport assignments are kept (none in tests and headless apps).
    path: Option<PathBuf>,
}

fn transport_path() -> PathBuf {
    crate::kits::kits_dir().with_file_name("midi.json")
}

impl Learn {
    pub fn load() -> Learn {
        let path = transport_path();
        let transport = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Learn { transport, path: Some(path), ..Default::default() }
    }

    fn save_transport(&self) {
        let Some(path) = &self.path else { return };
        if let Ok(text) = serde_json::to_string_pretty(&self.transport) {
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
            let _ = std::fs::write(path, text);
        }
    }

    /// Whether a sweep of a controller is still going on (for undo).
    pub fn moving(&self) -> bool {
        self.last_move.is_some_and(|t| t.elapsed() < Duration::from_millis(400))
    }
}

impl PulseApp {
    /// Remembers the control the user touched, to link it while learning.
    pub fn touched(&self, control: Control) {
        if self.learn.on {
            self.touched_control.set(Some(control));
        }
    }

    pub fn toggle_learn(&mut self) {
        self.learn.on = !self.learn.on;
        self.learn.armed = None;
        self.touched_control.set(None);
        self.learn.shared.learning.store(self.learn.on, Ordering::Relaxed);
        self.status = Some(if self.learn.on {
            "Asignar MIDI: toca un control de Pulse y después mueve un knob o pulsa un botón del teclado".into()
        } else {
            format!("{} asignaciones MIDI", self.mappings().len())
        });
    }

    /// Every assignment: the project's and the transport ones.
    pub fn mappings(&self) -> Vec<Mapping> {
        self.project.midi_map.iter().chain(&self.learn.transport).copied().collect()
    }

    /// Tells the MIDI threads which controllers are taken.
    pub fn sync_midi_map(&self) {
        let mut taken = [false; 128];
        for m in self.mappings() {
            taken[m.cc as usize & 127] = true;
        }
        if let Ok(mut shared) = self.learn.shared.taken.lock() {
            *shared = taken;
        }
    }

    /// Links controller `cc` to `control`, replacing what either had.
    pub fn assign(&mut self, cc: u8, control: Control) {
        self.project.midi_map.retain(|m| m.cc != cc && m.control != control);
        self.learn.transport.retain(|m| m.cc != cc && m.control != control);
        match control {
            Control::Transport(_) => {
                self.learn.transport.push(Mapping { cc, control });
                self.learn.save_transport();
            }
            _ => self.project.midi_map.push(Mapping { cc, control }),
        }
        self.sync_midi_map();
        self.status = Some(format!("CC {cc} → {}", self.control_name(control)));
    }

    pub fn unassign(&mut self, cc: u8) {
        let transport = self.learn.transport.len();
        self.project.midi_map.retain(|m| m.cc != cc);
        self.learn.transport.retain(|m| m.cc != cc);
        if self.learn.transport.len() != transport {
            self.learn.save_transport();
        }
        self.sync_midi_map();
    }

    pub fn control_name(&self, control: Control) -> String {
        match control {
            Control::Param { target, param } => format!("{} · {}", self.target_name(target), self.param_name(target, param)),
            Control::Plugin { id, param } => {
                let name = self.samples.plugins.get(&id).and_then(|h| h.param(param)).map_or_else(|| format!("parámetro {param}"), |p| p.name.clone());
                format!("{} · {name}", self.plugin_name(id))
            }
            Control::Transport(t) => t.label().into(),
        }
    }

    /// Once a frame: links or applies the controllers that came in.
    pub fn poll_midi_map(&mut self) {
        let log = std::env::var_os("PULSE_MIDI_LOG").is_some();
        if let Some(control) = self.touched_control.take() {
            if log {
                eprintln!("[asignar] elegido: {}", self.control_name(control));
            }
            self.learn.armed = Some(control);
            self.status = Some(format!("Mueve un knob o pulsa un botón del teclado para «{}»", self.control_name(control)));
        }
        let incoming: Vec<(u8, u8)> = self.learn.shared.incoming.lock().map(|mut i| std::mem::take(&mut *i)).unwrap_or_default();
        for (cc, value) in incoming {
            if self.learn.on {
                match self.learn.armed.take() {
                    Some(control) => {
                        if log {
                            eprintln!("[asignar] CC {cc} → {}", self.control_name(control));
                        }
                        self.assign(cc, control);
                    }
                    None if log => eprintln!("[asignar] CC {cc}: ningún control de Pulse elegido todavía"),
                    None => {}
                }
                continue;
            }
            self.apply_cc(cc, value);
        }
    }

    /// A controller moved: sets what it is linked to.
    pub fn apply_cc(&mut self, cc: u8, value: u8) {
        let Some(mapping) = self.mappings().into_iter().find(|m| m.cc == cc) else { return };
        let normalized = value as f32 / 127.0;
        // Buttons act when pressed (from below the middle to above it, or again after a pause).
        let now = Instant::now();
        let previous = self.learn.last_value.iter().position(|&(c, _, _)| c == cc).map(|i| self.learn.last_value.swap_remove(i));
        self.learn.last_value.push((cc, value, now));
        let pressed = value >= 64 && previous.is_none_or(|(_, v, t)| v < 64 || now.duration_since(t) > Duration::from_millis(150));
        match mapping.control {
            Control::Param { target, param } => {
                let Some(info) = self.param_info(target, param) else { return };
                self.set_param(target, param, info.denormalize(normalized));
                self.learn.last_move = Some(now);
            }
            Control::Plugin { id, param } => {
                if let Some(handle) = self.samples.plugins.get(&id) {
                    handle.set(param, normalized as f64);
                }
            }
            Control::Transport(action) if pressed => self.transport(action),
            Control::Transport(_) => {}
        }
    }

    pub fn transport(&mut self, action: Transport) {
        match action {
            Transport::Play => {
                let playing = self.audio.is_playing();
                self.audio.send(if playing { Command::Stop } else { Command::Play });
            }
            Transport::Stop => self.audio.send(Command::Stop),
            Transport::Record => self.toggle_arrangement_record(),
            Transport::Loop => {
                self.project.loop_enabled = !self.project.loop_enabled;
                self.sync_loop();
            }
            Transport::Metronome => {
                self.project.metronome = !self.project.metronome;
                self.audio.send(Command::SetMetronome(self.project.metronome));
            }
        }
    }
}

/// The list of assignments, shown while learning.
pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    if !app.learn.on {
        return;
    }
    let mut remove = None;
    let mut close = false;
    egui::Window::new("Asignar MIDI").default_pos([980.0, 80.0]).resizable(false).show(ctx, |ui| {
        ui.label(egui::RichText::new("1. Toca un control de Pulse (knob, fader, parámetro de plugin o botón de transporte).\n2. Mueve un knob o pulsa un botón del teclado MIDI.").color(TEXT_DIM).size(10.5));
        if let Some(control) = app.learn.armed {
            ui.label(egui::RichText::new(format!("Esperando un control para «{}»…", app.control_name(control))).strong());
        }
        ui.separator();
        let mappings = app.mappings();
        if mappings.is_empty() {
            ui.label(egui::RichText::new("Sin asignaciones").color(TEXT_DIM));
        }
        egui::Grid::new("midi_mappings").num_columns(3).striped(true).show(ui, |ui| {
            for m in &mappings {
                ui.label(format!("CC {}", m.cc));
                ui.label(app.control_name(m.control));
                if ui.small_button("✕").on_hover_text("Quitar").clicked() {
                    remove = Some(m.cc);
                }
                ui.end_row();
            }
        });
        ui.separator();
        if ui.button("Terminar").clicked() {
            close = true;
        }
    });
    if let Some(cc) = remove {
        app.unassign(cc);
    }
    if close {
        app.toggle_learn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    fn app() -> PulseApp {
        let app = PulseApp::headless(Project::demo());
        // Headless apps neither read nor write the user's transport assignments.
        assert!(app.learn.transport.is_empty() && app.learn.path.is_none());
        app
    }

    /// A controller message from the keyboard, as the MIDI thread hands it over.
    fn cc(app: &mut PulseApp, cc: u8, value: u8) -> bool {
        let taken = app.learn.shared.take(cc, value);
        app.poll_midi_map();
        taken
    }

    #[test]
    fn touching_a_control_then_moving_a_knob_links_them() {
        let mut app = app();
        assert!(!cc(&mut app, 74, 10), "unassigned controllers go to the instrument");
        app.toggle_learn();
        // The user drags the volume of track 2…
        let volume = Control::Param { target: Target::Track(1), param: Param::Volume };
        app.set_param(Target::Track(1), Param::Volume, 0.5);
        app.poll_midi_map();
        assert_eq!(app.learn.armed, Some(volume));
        // …and turns knob CC 74.
        assert!(cc(&mut app, 74, 64), "while learning, controllers go to Pulse");
        assert_eq!(app.project.midi_map, [Mapping { cc: 74, control: volume }]);
        app.toggle_learn();

        // From now on CC 74 moves that fader, and no longer reaches the instrument.
        assert!(cc(&mut app, 74, 127));
        let max = app.param_info(Target::Track(1), Param::Volume).unwrap().max;
        assert_eq!(app.project.tracks[1].mixer.gain, max);
        assert!(cc(&mut app, 74, 0));
        assert_eq!(app.project.tracks[1].mixer.gain, 0.0);
        assert!(app.learn.moving(), "a sweep is one undo step");
        assert!(!cc(&mut app, 71, 5), "other controllers still go to the instrument");

        // Linking the same knob elsewhere moves it; removing it frees it.
        app.toggle_learn();
        app.set_param(Target::Master, Param::Volume, 0.8);
        app.poll_midi_map();
        cc(&mut app, 74, 3);
        assert_eq!(app.project.midi_map.len(), 1);
        assert_eq!(app.project.midi_map[0].control, Control::Param { target: Target::Master, param: Param::Volume });
        app.unassign(74);
        app.toggle_learn();
        assert!(!cc(&mut app, 74, 3));
    }

    #[test]
    fn transport_buttons_act_when_pressed() {
        let mut app = app();
        app.assign(22, Control::Transport(Transport::Metronome));
        let before = app.project.metronome;
        // The Axiom sends 127 on each press only.
        cc(&mut app, 22, 127);
        assert_eq!(app.project.metronome, !before);
        std::thread::sleep(Duration::from_millis(200));
        cc(&mut app, 22, 127);
        assert_eq!(app.project.metronome, before);
        // Buttons that also send 0 on release act once per press.
        cc(&mut app, 22, 0);
        cc(&mut app, 22, 127);
        assert_eq!(app.project.metronome, !before);
        assert!(app.project.midi_map.is_empty(), "transport assignments are not the project's");
    }

    #[test]
    fn assignments_are_saved_with_the_project() {
        let mut app = app();
        let control = Control::Param { target: Target::Track(0), param: Param::Pan };
        app.assign(10, control);
        let text = serde_json::to_string(&app.project).unwrap();
        let loaded: Project = serde_json::from_str(&text).unwrap();
        assert_eq!(loaded.midi_map, [Mapping { cc: 10, control }]);
        let mut reopened = PulseApp::headless(loaded);
        assert!(reopened.learn.shared.take(10, 0));
        reopened.poll_midi_map();
        assert_eq!(reopened.project.tracks[0].mixer.pan, -1.0);
    }

    #[test]
    fn a_click_picks_a_knob_or_fader_while_learning() {
        use eframe::egui::{Event, PointerButton, Pos2, RawInput, Rect};
        let info = engine::model::VOLUME_PARAM;
        let click = |learning: bool| {
            let ctx = egui::Context::default();
            let mut picked = (false, false);
            let mut rects = (Rect::NOTHING, Rect::NOTHING);
            let press = |pos: Pos2, pressed: bool| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() };
            for frame in 0..8 {
                let events = match frame {
                    1 => vec![Event::PointerMoved(rects.0.center())],
                    2 => vec![press(rects.0.center(), true)],
                    3 => vec![press(rects.0.center(), false)],
                    4 => vec![Event::PointerMoved(rects.1.center())],
                    5 => vec![press(rects.1.center(), true)],
                    6 => vec![press(rects.1.center(), false)],
                    _ => vec![],
                };
                let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 400.0))), events, time: Some(frame as f64 * 0.1), ..Default::default() };
                let _ = ctx.run_ui(input, |ui| {
                    crate::widgets::set_learning(ui.ctx(), learning);
                    let (changed, response) = crate::widgets::param_knob_response(ui, &info, 0.5, "Vol", 30.0, None);
                    rects.0 = response.rect;
                    picked.0 |= changed.is_some();
                    let mut gain = 0.5;
                    let fader = crate::widgets::fader_with_meter(ui, &mut gain, 0.0, 100.0);
                    rects.1 = fader.rect;
                    picked.1 |= fader.changed();
                });
            }
            picked
        };
        assert_eq!(click(true), (true, true), "a click picks them while learning");
        assert_eq!(click(false), (false, false), "and does nothing otherwise");
        // Picking does not move the control.
        let mut app = app();
        app.toggle_learn();
        let gain = app.project.tracks[0].mixer.gain;
        app.set_param(Target::Track(0), Param::Volume, gain);
        app.poll_midi_map();
        assert_eq!(app.learn.armed, Some(Control::Param { target: Target::Track(0), param: Param::Volume }));
        assert!(app.overridden.is_empty(), "no automation overridden by picking");
    }

    #[test]
    fn the_window_lists_the_assignments() {
        let mut app = app();
        app.project.midi_map.push(Mapping { cc: 7, control: Control::Param { target: Target::Master, param: Param::Volume } });
        app.toggle_learn();
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| window(&mut app, &ui.ctx().clone()));
        assert!(app.control_name(app.project.midi_map[0].control).contains("Volumen"));
    }
}
