//! Macros: eight knobs of the project, each moving several controls at once,
//! each control over its own range (a filter opening while the reverb grows and
//! the drive comes in). A macro moves its controls as moving them by hand does,
//! so recording automation records each of them, and a MIDI knob can drive a
//! macro (see `midi_map`).

use crate::app::PulseApp;
use crate::midi_map::Control;
use crate::theme::{ACCENT, TEXT_DIM};
use eframe::egui;
use serde::{Deserialize, Serialize};

pub const MACROS: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Macro {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Where its knob is, 0..1.
    #[serde(default)]
    pub value: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<Route>,
}

/// A control a macro moves: from `min` (macro at 0) to `max` (at 1), in 0..1
/// of the control's range.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub control: Control,
    pub min: f32,
    pub max: f32,
}

impl Macro {
    pub fn label(&self, index: usize) -> String {
        if self.name.trim().is_empty() { format!("Macro {}", index + 1) } else { self.name.clone() }
    }
}

/// The macros window's state.
#[derive(Default)]
pub struct Window {
    pub open: bool,
    /// The macro whose next control touched is added to it.
    pub picking: Option<usize>,
}

impl PulseApp {
    /// The project's macros, all eight.
    fn macros_mut(&mut self) -> &mut Vec<Macro> {
        let macros = &mut self.project.macros;
        if macros.len() < MACROS {
            macros.resize_with(MACROS, Macro::default);
        }
        macros
    }

    /// Where macro `index` is, 0..1.
    pub fn macro_value(&self, index: usize) -> f32 {
        self.project.macros.get(index).map_or(0.0, |m| m.value)
    }

    /// Turns macro `index` to `value` (0..1): each of its controls goes to its
    /// place in its range.
    pub fn set_macro(&mut self, index: usize, value: f32) {
        if index >= MACROS {
            return;
        }
        self.touched(Control::Macro(index as u8));
        let value = value.clamp(0.0, 1.0);
        // Picked for MIDI learn: not moved.
        if self.learn.on && self.macro_value(index) == value {
            return;
        }
        self.macros_mut()[index].value = value;
        let routes = self.project.macros[index].routes.clone();
        for route in routes {
            let n = route.min + (route.max - route.min) * value;
            self.set_control(route.control, n.clamp(0.0, 1.0));
        }
    }

    /// Adds a control to what macro `index` moves (over its whole range), or
    /// says why it can't.
    pub fn add_to_macro(&mut self, index: usize, control: Control) {
        if matches!(control, Control::Macro(_) | Control::Transport(_)) {
            self.status = Some("Una macro mueve knobs, faders y parámetros, no otras macros ni el transporte".into());
            return;
        }
        let name = self.control_name(control);
        let routes = &mut self.macros_mut()[index].routes;
        if routes.iter().any(|r| r.control == control) {
            self.status = Some(format!("La macro ya mueve «{name}»"));
            return;
        }
        routes.push(Route { control, min: 0.0, max: 1.0 });
        let label = self.project.macros[index].label(index);
        self.status = Some(format!("«{label}» mueve también «{name}»"));
    }

    /// Whether a click on a control picks it (for MIDI learn or a macro).
    pub fn picking(&self) -> bool {
        self.learn.on || self.macro_window.picking.is_some()
    }
}

/// The «Macros» window: the eight knobs, and under the one chosen, what it moves.
pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    if !app.macro_window.open {
        app.macro_window.picking = None;
        return;
    }
    let mut open = true;
    egui::Window::new("Macros").id(egui::Id::new("macros")).open(&mut open).resizable(false).default_pos([600.0, 120.0]).show(ctx, |ui| contents(app, ui));
    if !open {
        app.macro_window.open = false;
        app.macro_window.picking = None;
    }
}

fn contents(app: &mut PulseApp, ui: &mut egui::Ui) {
    let info = engine::model::ParamInfo::linear("Macro", 0.0, 1.0, 0.0, engine::model::Unit::Percent);
    ui.horizontal(|ui| {
        for i in 0..MACROS {
            ui.vertical(|ui| {
                let label = app.project.macros.get(i).map_or_else(|| format!("Macro {}", i + 1), |m| m.label(i));
                let (changed, response) = crate::widgets::param_knob_response(ui, &info, app.macro_value(i), &label, 34.0, None);
                if let Some(value) = changed {
                    app.set_macro(i, value);
                }
                let count = app.project.macros.get(i).map_or(0, |m| m.routes.len());
                let text = egui::RichText::new(if count == 1 { "1 control".to_string() } else { format!("{count} controles") }).size(10.0);
                let chosen = app.macro_window.picking == Some(i);
                if ui.selectable_label(chosen, text.color(if chosen { ACCENT } else { TEXT_DIM })).on_hover_text("Añadir un control: pulsa y toca el knob, fader o parámetro que quieras que mueva").clicked() {
                    app.macro_window.picking = if chosen { None } else { Some(i) };
                }
                response.on_hover_text("Gira para mover a la vez todo lo que tiene asignado");
            });
        }
    });
    let Some(i) = app.macro_window.picking else {
        ui.label(egui::RichText::new("Pulsa «controles» bajo una macro para ver qué mueve y añadirle más.").color(TEXT_DIM).size(10.5));
        return;
    };
    ui.separator();
    let mut name = app.project.macros.get(i).map(|m| m.name.clone()).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Nombre");
        if ui.add(egui::TextEdit::singleline(&mut name).hint_text(format!("Macro {}", i + 1)).desired_width(160.0)).changed() {
            app.macros_mut()[i].name = name.clone();
        }
    });
    ui.label(egui::RichText::new("Toca un knob, un fader o un parámetro de plugin para añadirlo. Desde y Hasta: dónde lo pone la macro al mínimo y al máximo (en % de su recorrido; al revés si Desde es mayor).").color(TEXT_DIM).size(10.5));
    let routes = app.project.macros.get(i).map(|m| m.routes.clone()).unwrap_or_default();
    let mut remove = None;
    let mut edited = Vec::new();
    egui::Grid::new("macro_routes").num_columns(4).striped(true).show(ui, |ui| {
        for (k, route) in routes.iter().enumerate() {
            ui.label(app.control_name(route.control));
            let mut r = *route;
            for value in [&mut r.min, &mut r.max] {
                let mut p = *value * 100.0;
                if ui.add(egui::DragValue::new(&mut p).range(0.0..=100.0).speed(0.5).suffix(" %").max_decimals(0)).changed() {
                    *value = p / 100.0;
                }
            }
            if r != *route {
                edited.push((k, r));
            }
            if ui.small_button("×").on_hover_text("Quitar").clicked() {
                remove = Some(k);
            }
            ui.end_row();
        }
    });
    if routes.is_empty() {
        ui.label(egui::RichText::new("Aún no mueve nada.").color(TEXT_DIM));
    }
    let changed = !edited.is_empty() || remove.is_some();
    let macro_ = &mut app.macros_mut()[i];
    for (k, r) in edited {
        macro_.routes[k] = r;
    }
    if let Some(k) = remove {
        macro_.routes.remove(k);
    }
    if changed {
        let value = macro_.value;
        // Its controls go to where the macro now puts them.
        app.set_macro(i, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use engine::model::Param;
    use engine::Target;

    fn app() -> PulseApp {
        PulseApp::headless(Project::demo())
    }

    /// One macro moves several controls at once, each over its own range.
    #[test]
    fn a_macro_moves_its_controls_each_over_its_range() {
        let mut app = app();
        let pan = Control::Param { target: Target::Track(0), param: Param::Pan };
        let send = Control::Param { target: Target::Track(1), param: Param::Send(0) };
        app.add_to_macro(2, pan);
        app.add_to_macro(2, send);
        app.project.macros[2].routes[1] = Route { control: send, min: 0.2, max: 0.6 };
        app.set_macro(2, 1.0);
        assert_eq!(app.project.tracks[0].mixer.pan, 1.0);
        assert!((app.project.tracks[1].sends[0] - 0.6).abs() < 1e-6);
        app.set_macro(2, 0.0);
        assert_eq!(app.project.tracks[0].mixer.pan, -1.0);
        assert!((app.project.tracks[1].sends[0] - 0.2).abs() < 1e-6);
        assert_eq!(app.project.macros.len(), MACROS);
        // The same control twice, another macro, the transport: not added.
        app.add_to_macro(2, pan);
        app.add_to_macro(2, Control::Macro(1));
        assert_eq!(app.project.macros[2].routes.len(), 2);
        // Saved with the project.
        let loaded: Project = serde_json::from_str(&serde_json::to_string(&app.project).unwrap()).unwrap();
        assert_eq!(loaded.macros, app.project.macros);
    }

    /// With «controles» pressed, the next control touched joins the macro.
    #[test]
    fn touching_a_control_adds_it_to_the_macro_being_set_up() {
        let mut app = app();
        app.macro_window.open = true;
        app.macro_window.picking = Some(0);
        assert!(app.picking());
        let gain = app.project.master.mixer.gain;
        app.set_param(Target::Master, Param::Volume, gain);
        app.poll_midi_map();
        assert_eq!(app.project.macros[0].routes, [Route { control: Control::Param { target: Target::Master, param: Param::Volume }, min: 0.0, max: 1.0 }]);
        assert_eq!(app.project.master.mixer.gain, gain, "picking doesn't move it");
        assert!(app.overridden.is_empty());
    }

    /// A MIDI knob can turn a macro, and so all it moves.
    #[test]
    fn a_midi_knob_turns_a_macro() {
        let mut app = app();
        app.add_to_macro(0, Control::Param { target: Target::Track(0), param: Param::Pan });
        app.assign(21, Control::Macro(0));
        assert!(app.learn.shared.take(21, 127));
        app.poll_midi_map();
        assert_eq!(app.macro_value(0), 1.0);
        assert_eq!(app.project.tracks[0].mixer.pan, 1.0);
        assert!(app.control_name(Control::Macro(0)).contains("Macro 1"));
    }

    /// The window draws its knobs, and what the chosen one moves.
    #[test]
    fn the_window_shows_the_knobs_and_their_controls() {
        let mut app = app();
        app.add_to_macro(3, Control::Param { target: Target::Track(0), param: Param::Pan });
        app.project.macros[3].name = "Filtro".into();
        app.macro_window.open = true;
        app.macro_window.picking = Some(3);
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| window(&mut app, &ui.ctx().clone()));
        assert!(app.macro_window.open);
        assert_eq!(app.control_name(Control::Macro(3)), "Macro 4 · Filtro");
    }

    /// Moving tracks keeps each macro on its controls.
    #[test]
    fn macros_follow_their_tracks_when_they_move() {
        let mut app = app();
        app.add_to_macro(0, Control::Param { target: Target::Track(0), param: Param::Pan });
        let n = app.project.tracks.len();
        assert!(app.move_tracks(&[0], n));
        let moved = app.project.tracks.len() - 1;
        assert_eq!(app.project.macros[0].routes[0].control, Control::Param { target: Target::Track(moved), param: Param::Pan });
    }
}
