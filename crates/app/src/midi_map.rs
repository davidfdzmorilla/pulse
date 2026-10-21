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
    /// One of the project's macros (see `macros`).
    Macro(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transport {
    /// Play, or stop if playing.
    Play,
    Stop,
    Record,
    Loop,
    Metronome,
    AutomationArm,
}

impl Transport {
    pub fn label(self) -> &'static str {
        match self {
            Transport::Play => "Reproducir",
            Transport::Stop => "Parar",
            Transport::Record => "Grabar",
            Transport::Loop => "Loop",
            Transport::Metronome => "Metrónomo",
            Transport::AutomationArm => "Armar automatización",
        }
    }
}

/// A controller (CC number, any channel) linked to a control. One controller
/// can move several controls, each over its own range.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mapping {
    pub cc: u8,
    pub control: Control,
    /// What the controller's bottom and top set the control to (0..1 of its
    /// range): a part of it, or backwards with `min` above `max`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub min: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub max: f32,
    /// Seconds the control takes to follow the controller (0: at once), so a
    /// jumpy knob or a 0–127 step moves it smoothly.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub smooth: f32,
}

fn one() -> f32 {
    1.0
}

fn is_one(x: &f32) -> bool {
    *x == 1.0
}

fn is_zero(x: &f32) -> bool {
    *x == 0.0
}

impl Mapping {
    pub fn new(cc: u8, control: Control) -> Self {
        Mapping { cc, control, min: 0.0, max: 1.0, smooth: 0.0 }
    }

    /// Where controller value `value` (0..127) puts the control (0..1).
    pub fn scaled(&self, value: u8) -> f32 {
        let (min, max) = (self.min.clamp(0.0, 1.0), self.max.clamp(0.0, 1.0));
        min + (max - min) * value as f32 / 127.0
    }
}

/// A control following its controller over `smooth` seconds.
#[derive(Clone, Copy, Debug)]
struct Glide {
    control: Control,
    at: f32,
    to: f32,
    smooth: f32,
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

/// How long a controller moved first waits for a control to be touched.
const PENDING_FOR: Duration = Duration::from_secs(10);

/// The MIDI learn state of the app.
#[derive(Default)]
pub struct Learn {
    pub on: bool,
    /// The control touched last, waiting for a controller.
    pub armed: Option<Control>,
    /// A free controller moved before any control was touched, waiting for one
    /// (either order links them).
    pub pending: Option<(u8, Instant)>,
    /// Transport assignments, kept across projects.
    pub transport: Vec<Mapping>,
    pub shared: Shared,
    /// When a controller last moved something (so a sweep is one undo step).
    pub last_move: Option<Instant>,
    /// Value last seen per controller, to tell presses from releases.
    last_value: Vec<(u8, u8, Instant)>,
    /// Controls still on their way to where their controller put them, and
    /// when they last moved.
    glides: Vec<Glide>,
    glided: Option<Instant>,
    /// Linking a controller adds a control to the ones it moves, instead of
    /// moving it from them.
    pub add: bool,
    /// Where transport assignments are kept (none in tests and headless apps).
    path: Option<PathBuf>,
}

fn transport_path() -> PathBuf {
    paths::config_dir().join("midi.json")
}

impl Learn {
    pub fn load() -> Learn {
        let path = transport_path();
        let transport = crate::store::read_json(&path);
        Learn { transport, path: Some(path), ..Default::default() }
    }

    fn save_transport(&self) {
        let Some(path) = &self.path else { return };
        if let Err(e) = crate::store::write_json(path, &self.transport) {
            crate::logs::note(&e);
        }
    }

    /// Whether a sweep of a controller is still going on (for undo).
    pub fn moving(&self) -> bool {
        self.last_move.is_some_and(|t| t.elapsed() < Duration::from_millis(400)) || !self.glides.is_empty()
    }

    /// Whether controls are still following their controllers (frames needed).
    pub fn gliding(&self) -> bool {
        !self.glides.is_empty()
    }
}

impl PulseApp {
    /// Remembers the control the user touched, to link it while learning.
    pub fn touched(&self, control: Control) {
        if self.picking() {
            self.touched_control.set(Some(control));
        }
    }

    pub fn toggle_learn(&mut self) {
        self.learn.on = !self.learn.on;
        self.learn.armed = None;
        self.learn.pending = None;
        self.touched_control.set(None);
        self.learn.shared.learning.store(self.learn.on, Ordering::Relaxed);
        self.status = Some(if self.learn.on {
            "Asignar MIDI: toca un control de Pulse y mueve un knob o pulsa un botón del teclado (en cualquier orden)".into()
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

    /// Links controller `cc` to `control`, replacing what either had (with
    /// «add», the controller keeps its other controls too).
    pub fn assign(&mut self, cc: u8, control: Control) {
        let add = self.learn.add;
        let keep = |m: &Mapping| m.control != control && (m.cc != cc || add);
        self.project.midi_map.retain(keep);
        self.learn.transport.retain(keep);
        match control {
            Control::Transport(_) => {
                self.learn.transport.push(Mapping::new(cc, control));
                self.learn.save_transport();
            }
            _ => self.project.midi_map.push(Mapping::new(cc, control)),
        }
        self.sync_midi_map();
        let count = self.mappings().iter().filter(|m| m.cc == cc).count();
        self.status = Some(if count > 1 { format!("CC {cc} → {} (y {} más)", self.control_name(control), count - 1) } else { format!("CC {cc} → {}", self.control_name(control)) });
    }

    /// Removes one control from what controller `cc` moves.
    pub fn unassign_control(&mut self, cc: u8, control: Control) {
        let keep = |m: &Mapping| !(m.cc == cc && m.control == control);
        let transport = self.learn.transport.len();
        self.project.midi_map.retain(keep);
        self.learn.transport.retain(keep);
        if self.learn.transport.len() != transport {
            self.learn.save_transport();
        }
        self.sync_midi_map();
    }

    #[cfg(test)]
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
            Control::Macro(i) => {
                let i = i as usize;
                let name = self.project.macros.get(i).map_or_else(|| format!("Macro {}", i + 1), |m| m.label(i));
                if name.starts_with("Macro ") { name } else { format!("Macro {} · {name}", i + 1) }
            }
        }
    }

    /// Once a frame: links or applies the controllers that came in.
    pub fn poll_midi_map(&mut self) {
        let log = std::env::var_os("PULSE_MIDI_LOG").is_some();
        let touched = self.touched_control.take();
        // Setting up a macro: what is touched joins it.
        if let (Some(control), Some(index), false) = (touched, self.macro_window.picking, self.learn.on) {
            self.add_to_macro(index, control);
        }
        if let Some(control) = touched.filter(|_| self.learn.on) {
            if log {
                eprintln!("[asignar] elegido: {}", self.control_name(control));
            }
            match self.learn.pending.take().filter(|&(_, t)| t.elapsed() < PENDING_FOR) {
                Some((cc, _)) => {
                    if log {
                        eprintln!("[asignar] CC {cc} (movido antes) → {}", self.control_name(control));
                    }
                    self.assign(cc, control);
                }
                None => {
                    self.learn.armed = Some(control);
                    self.status = Some(format!("Mueve un knob o pulsa un botón del teclado para «{}»", self.control_name(control)));
                }
            }
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
                    // A free controller: it waits for the control touched next.
                    None if !self.mappings().iter().any(|m| m.cc == cc) => {
                        if log && self.learn.pending.is_none_or(|(c, _)| c != cc) {
                            eprintln!("[asignar] CC {cc}: esperando a que toques un control de Pulse");
                        }
                        self.learn.pending = Some((cc, Instant::now()));
                        self.status = Some(format!("CC {cc}: ahora toca el control de Pulse que quieres que mueva"));
                    }
                    None => {}
                }
                continue;
            }
            self.apply_cc(cc, value);
        }
        self.glide();
    }

    /// A controller moved: sets what it is linked to, each over its range,
    /// at once or gliding there.
    pub fn apply_cc(&mut self, cc: u8, value: u8) {
        let mappings: Vec<Mapping> = self.mappings().into_iter().filter(|m| m.cc == cc).collect();
        if mappings.is_empty() {
            return;
        }
        // Buttons act when pressed (from below the middle to above it, or again after a pause).
        let now = Instant::now();
        let previous = self.learn.last_value.iter().position(|&(c, _, _)| c == cc).map(|i| self.learn.last_value.swap_remove(i));
        self.learn.last_value.push((cc, value, now));
        let pressed = value >= 64 && previous.is_none_or(|(_, v, t)| v < 64 || now.duration_since(t) > Duration::from_millis(150));
        for mapping in mappings {
            let to = mapping.scaled(value);
            match mapping.control {
                Control::Transport(action) if pressed => self.transport(action),
                Control::Transport(_) => {}
                control if mapping.smooth > 0.0 => {
                    let glide = match self.learn.glides.iter().position(|g| g.control == control) {
                        Some(i) => &mut self.learn.glides[i],
                        None => {
                            let at = self.control_value(control).unwrap_or(to);
                            self.learn.glides.push(Glide { control, at, to, smooth: mapping.smooth });
                            self.learn.glided.get_or_insert(now);
                            self.learn.glides.last_mut().expect("just pushed")
                        }
                    };
                    (glide.to, glide.smooth) = (to, mapping.smooth);
                    self.learn.last_move = Some(now);
                }
                control => {
                    self.learn.glides.retain(|g| g.control != control);
                    self.set_control(control, to);
                    self.learn.last_move = Some(now);
                }
            }
        }
    }

    /// Once a frame: moves the gliding controls on towards their controllers.
    fn glide(&mut self) {
        let now = Instant::now();
        let dt = self.learn.glided.map_or(0.0, |t| now.duration_since(t).as_secs_f32());
        self.learn.glided = (!self.learn.glides.is_empty()).then_some(now);
        let mut glides = std::mem::take(&mut self.learn.glides);
        for g in &mut glides {
            // Most of the way (95 %) in `smooth` seconds, then there.
            let step = 1.0 - (-3.0 * dt / g.smooth.max(1e-3)).exp();
            g.at += (g.to - g.at) * step;
            if (g.to - g.at).abs() < 0.002 {
                g.at = g.to;
            }
            self.set_control(g.control, g.at);
        }
        glides.retain(|g| g.at != g.to);
        self.learn.glides = glides;
    }

    /// Where a control is, 0..1 of its range.
    fn control_value(&self, control: Control) -> Option<f32> {
        match control {
            Control::Param { target, param } => self.param_info(target, param).map(|info| info.normalize(self.param_value(target, param))),
            Control::Plugin { id, param } => self.samples.plugins.get(&id).map(|h| h.value(param) as f32),
            Control::Transport(_) => None,
            Control::Macro(i) => Some(self.macro_value(i as usize)),
        }
    }

    /// Sets a control to `n`, 0..1 of its range, as moving it by hand does.
    pub(crate) fn set_control(&mut self, control: Control, n: f32) {
        match control {
            Control::Param { target, param } => {
                if let Some(info) = self.param_info(target, param) {
                    self.set_param(target, param, info.denormalize(n));
                }
            }
            Control::Plugin { id, param } => {
                if let Some(handle) = self.samples.plugins.get(&id) {
                    handle.set(param, n as f64);
                }
                // Moved by hand: its automation stops playing (and the move can be recorded).
                if let Some((target, p)) = self.plugin_target_param(id, param) {
                    if self.automation_state(target, p) == Some(false) && self.overridden.insert((target, p)) {
                        self.sync_automation(target);
                    }
                }
            }
            Control::Transport(_) => {}
            Control::Macro(i) => self.set_macro(i as usize, n),
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
            Transport::AutomationArm => self.toggle_automation_arm(),
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
        ui.label(egui::RichText::new("Toca un control de Pulse (knob, fader, parámetro de plugin o botón de transporte) y mueve un knob o pulsa un botón del teclado MIDI, en cualquier orden.").color(TEXT_DIM).size(10.5));
        if let Some(control) = app.learn.armed {
            ui.label(egui::RichText::new(format!("Esperando un control para «{}»…", app.control_name(control))).strong());
        } else if let Some((cc, _)) = app.learn.pending {
            ui.label(egui::RichText::new(format!("CC {cc}: toca el control de Pulse que quieres que mueva…")).strong());
        }
        ui.separator();
        let mappings = app.mappings();
        if mappings.is_empty() {
            ui.label(egui::RichText::new("Sin asignaciones").color(TEXT_DIM));
        }
        egui::Grid::new("midi_mappings").num_columns(6).striped(true).show(ui, |ui| {
            ui.label(egui::RichText::new("CC").color(TEXT_DIM).size(10.5));
            ui.label(egui::RichText::new("Mueve").color(TEXT_DIM).size(10.5));
            ui.label(egui::RichText::new("Desde").color(TEXT_DIM).size(10.5)).on_hover_text("Dónde pone el control con el knob abajo del todo (en % de su recorrido)");
            ui.label(egui::RichText::new("Hasta").color(TEXT_DIM).size(10.5)).on_hover_text("Y arriba del todo. «Desde» por encima de «Hasta» lo mueve al revés");
            ui.label(egui::RichText::new("Suavizado").color(TEXT_DIM).size(10.5)).on_hover_text("Lo que tarda el control en llegar adonde pones el knob: sin saltos aunque el knob vaya a escalones");
            ui.end_row();
            for m in &mappings {
                ui.label(format!("CC {}", m.cc));
                ui.label(app.control_name(m.control));
                let mut edited = *m;
                if matches!(m.control, Control::Transport(_)) {
                    ui.label("");
                    ui.label("");
                    ui.label("");
                } else {
                    let percent = |ui: &mut egui::Ui, value: &mut f32| {
                        let mut p = *value * 100.0;
                        if ui.add(egui::DragValue::new(&mut p).range(0.0..=100.0).speed(0.5).suffix(" %").max_decimals(0)).changed() {
                            *value = p / 100.0;
                        }
                    };
                    percent(ui, &mut edited.min);
                    percent(ui, &mut edited.max);
                    let mut ms = edited.smooth * 1000.0;
                    if ui.add(egui::DragValue::new(&mut ms).range(0.0..=2000.0).speed(5.0).suffix(" ms").max_decimals(0)).changed() {
                        edited.smooth = ms / 1000.0;
                    }
                }
                if edited != *m {
                    if let Some(slot) = app.project.midi_map.iter_mut().find(|x| x.cc == m.cc && x.control == m.control) {
                        *slot = edited;
                    }
                }
                if ui.small_button("×").on_hover_text("Quitar").clicked() {
                    remove = Some((m.cc, m.control));
                }
                ui.end_row();
            }
        });
        ui.checkbox(&mut app.learn.add, "Un knob para varios controles").on_hover_text("Al asignar un knob que ya mueve algo, lo añade a lo que mueve en vez de cambiarlo: un solo gesto para el filtro, la reverb y el volumen a la vez");
        ui.separator();
        if ui.button("Terminar").clicked() {
            close = true;
        }
    });
    if let Some((cc, control)) = remove {
        app.unassign_control(cc, control);
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
        assert_eq!(app.project.midi_map, [Mapping::new(74, volume)]);
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
    fn moving_a_knob_then_touching_a_control_links_them_too() {
        let mut app = app();
        app.toggle_learn();
        // The knob first: it waits for a control…
        cc(&mut app, 10, 40);
        cc(&mut app, 10, 45);
        assert!(app.project.midi_map.is_empty());
        assert_eq!(app.learn.pending.map(|(c, _)| c), Some(10));
        // …and the control touched next takes it.
        let pan = Control::Param { target: Target::Track(1), param: Param::Pan };
        app.set_param(Target::Track(1), Param::Pan, app.param_value(Target::Track(1), Param::Pan));
        app.poll_midi_map();
        assert_eq!(app.project.midi_map, [Mapping::new(10, pan)]);
        assert_eq!(app.learn.armed, None);
        // Turning the linked knob on does not steal the next control touched.
        cc(&mut app, 10, 50);
        assert_eq!(app.learn.pending, None);
        app.set_param(Target::Master, Param::Volume, app.param_value(Target::Master, Param::Volume));
        app.poll_midi_map();
        assert_eq!(app.project.midi_map, [Mapping::new(10, pan)]);
        assert_eq!(app.learn.armed, Some(Control::Param { target: Target::Master, param: Param::Volume }));
    }

    /// Each control over its own part of its range, or backwards.
    #[test]
    fn a_knob_moves_a_control_over_its_range() {
        let mut app = app();
        let volume = Control::Param { target: Target::Track(1), param: Param::Volume };
        app.assign(74, volume);
        app.project.midi_map[0].min = 0.25;
        app.project.midi_map[0].max = 0.75;
        let info = app.param_info(Target::Track(1), Param::Volume).unwrap();
        let at = |app: &PulseApp| info.normalize(app.param_value(Target::Track(1), Param::Volume));
        cc(&mut app, 74, 0);
        assert!((at(&app) - 0.25).abs() < 1e-4);
        cc(&mut app, 74, 127);
        assert!((at(&app) - 0.75).abs() < 1e-4);
        // Backwards: the knob up turns it down.
        (app.project.midi_map[0].min, app.project.midi_map[0].max) = (1.0, 0.0);
        cc(&mut app, 74, 127);
        assert!(at(&app).abs() < 1e-4);
        // Saved with the project; a mapping from before ranges is the whole range.
        let text = serde_json::to_string(&app.project.midi_map).unwrap();
        assert_eq!(serde_json::from_str::<Vec<Mapping>>(&text).unwrap(), app.project.midi_map);
        let old: Mapping = serde_json::from_str(r#"{"cc": 3, "control": {"Transport": "Play"}}"#).unwrap();
        assert_eq!(old, Mapping::new(3, Control::Transport(Transport::Play)));
    }

    /// With «Un knob para varios controles», one knob moves them all; taking
    /// one away leaves the rest.
    #[test]
    fn one_knob_can_move_several_controls() {
        let mut app = app();
        let pan = Control::Param { target: Target::Track(0), param: Param::Pan };
        let volume = Control::Param { target: Target::Master, param: Param::Volume };
        app.assign(20, pan);
        app.assign(20, volume);
        assert_eq!(app.project.midi_map, [Mapping::new(20, volume)], "without it, the knob moves to the new control");
        app.learn.add = true;
        app.assign(20, pan);
        assert_eq!(app.project.midi_map.len(), 2);
        cc(&mut app, 20, 127);
        assert_eq!(app.project.tracks[0].mixer.pan, 1.0);
        assert_eq!(app.project.master.mixer.gain, app.param_info(Target::Master, Param::Volume).unwrap().max);
        app.unassign_control(20, volume);
        assert_eq!(app.project.midi_map, [Mapping::new(20, pan)]);
        assert!(app.learn.shared.take(20, 0), "still taken by the pan");
    }

    /// Smoothed, the control glides to where the knob went instead of jumping.
    #[test]
    fn a_smoothed_control_glides_to_the_knob() {
        let mut app = app();
        app.assign(30, Control::Param { target: Target::Track(0), param: Param::Pan });
        app.project.midi_map[0].smooth = 0.2;
        cc(&mut app, 30, 0);
        let pan = |app: &PulseApp| app.project.tracks[0].mixer.pan;
        let start = pan(&app);
        assert!(start > -0.5, "it doesn't jump: {start}");
        assert!(app.learn.gliding());
        let mut steps = vec![start];
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(10));
            app.poll_midi_map();
            steps.push(pan(&app));
        }
        assert!(steps.windows(2).all(|w| w[1] <= w[0]), "always towards it: {steps:?}");
        assert_eq!(pan(&app), -1.0, "and there in the end");
        assert!(!app.learn.gliding());
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
        assert_eq!(loaded.midi_map, [Mapping::new(10, control)]);
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
        app.project.midi_map.push(Mapping::new(7, Control::Param { target: Target::Master, param: Param::Volume }));
        app.toggle_learn();
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| window(&mut app, &ui.ctx().clone()));
        assert!(app.control_name(app.project.midi_map[0].control).contains("Volumen"));
    }
}
