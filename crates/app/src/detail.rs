//! Bottom detail area: clip editor or device chain, switched with tabs or Shift+Tab.
//!
//! Its height is a share of the window: the one its edge was dragged to (kept
//! in the settings), else 45 % or what two octaves of notes need, whichever is
//! more, leaving some of the Session or the Arrangement in sight. Maximized
//! (⇧⌘E, double click on its edge, «Maximizar») it fills almost all of it.
//! With nothing to edit (the «Clip» tab, no clip selected) it is only its tabs
//! and a line saying how to open one: the Session or the Arrangement get the rest.

use crate::app::{Detail, PulseApp};
use crate::theme::{self, TEXT_DIM};
use crate::{devices, piano_roll, widgets};
use eframe::egui::{self, vec2, RichText};

/// The panel is never shorter, except when empty.
pub const MIN: f32 = 200.0;
/// Its height with nothing to edit: the tabs and one line.
pub const EMPTY: f32 = 62.0;
/// How long it waits, with nothing to edit and no button down, before folding:
/// a double click in an empty place (a new clip) lets go of the clip first,
/// and what is under the second click must not have moved.
pub const FOLD_AFTER: f64 = 0.5;
/// The share of the window it starts at, if two octaves fit in it.
pub const SHARE: f32 = 0.45;
/// How tall two octaves of the editor are, until it says (see `PulseApp::editor_need`).
pub const NEED_GUESS: f32 = 500.0;
/// What stays in sight of the Session or the Arrangement over the panel when
/// it grows by itself, and when maximized.
const VIEW_LEFT: f32 = 110.0;
const VIEW_LEFT_MAX: f32 = 36.0;

/// The tallest the panel gets in `room` points (the window under the transport).
pub fn highest(room: f32) -> f32 {
    (room - VIEW_LEFT_MAX).max(MIN)
}

/// The panel's height in `room`: maximized, the tallest; at the `share` the
/// user left it at; else `SHARE`, or `need` if more, leaving `VIEW_LEFT`.
pub fn height(room: f32, share: Option<f32>, need: f32, maximized: bool) -> f32 {
    if maximized {
        return highest(room);
    }
    match share {
        Some(share) => (share * room).clamp(MIN, highest(room)),
        None => (SHARE * room).max(need).clamp(MIN, (room - VIEW_LEFT).max(MIN)),
    }
}

impl PulseApp {
    /// Whether the panel has nothing to show: the clip editor with no clip.
    pub fn detail_empty(&self) -> bool {
        self.detail == Detail::Clip && !self.editor_max && self.selected_clip.filter(|&c| self.clip(c).is_some()).is_none()
    }

    /// Whether the panel folds to its tabs this frame: empty for `FOLD_AFTER`
    /// with no button down (at once when the app starts). A clip opens it at once.
    pub(crate) fn fold_detail(&mut self, ctx: &egui::Context) -> bool {
        if !self.detail_empty() {
            self.detail_empty_since = None;
            self.detail_folded = false;
            return false;
        }
        let (now, down) = ctx.input(|i| (i.time, i.pointer.any_down()));
        let since = *self.detail_empty_since.get_or_insert(now);
        if !self.detail_folded {
            let starting = since == now && ctx.cumulative_frame_nr() < 2;
            if starting || (!down && now - since >= FOLD_AFTER) {
                self.detail_folded = true;
            } else if !down {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(FOLD_AFTER - (now - since) + 0.01));
            } else {
                // Held down: it waits from when the button comes up.
                self.detail_empty_since = Some(now);
            }
        }
        self.detail_folded
    }

    /// Maximizes the panel, or brings it back to its height.
    pub fn toggle_editor_max(&mut self) {
        self.editor_max = !self.editor_max;
        self.status = Some(crate::theme::keys(if self.editor_max { "Editor maximizado: ⇧⌘E lo devuelve a su altura" } else { "Editor a su altura" }));
    }

    /// After drawing the panel (`shown` points tall in `room`): what the editor
    /// needs for two octaves, and what was done to its edge. Dragged, its share
    /// of the window is kept in the settings; double clicked, it maximizes.
    pub(crate) fn follow_detail_edge(&mut self, ctx: &egui::Context, id: egui::Id, shown: f32, room: f32) {
        if let Some(short) = self.piano_roll.take_short() {
            let need = (shown + short).max(MIN);
            if (need - self.editor_need).abs() > 0.5 {
                self.editor_need = need;
            }
        }
        let Some(edge) = ctx.read_response(id.with("__resize")) else { return };
        if edge.double_clicked() {
            self.toggle_editor_max();
        } else if (edge.dragged() || edge.drag_stopped()) && room > 0.0 {
            self.editor_max = false;
            let share = (shown / room * 1000.0).round() / 1000.0;
            if self.settings.detail_share != Some(share) {
                let mut next = self.settings.clone();
                next.detail_share = Some(share);
                self.set_settings(next);
            }
        }
    }
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let mut save_chain = None;
    let mut maximize = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if widgets::flat_button(ui, "Clip", app.detail == Detail::Clip, vec2(60.0, 20.0)).clicked() {
            app.detail = Detail::Clip;
        }
        if widgets::flat_button(ui, "Dispositivos", app.detail == Detail::Devices, vec2(92.0, 20.0)).clicked() {
            app.detail = Detail::Devices;
        }
        ui.add_space(10.0);
        let target = app.selected_target;
        let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, app.target_color(target));
        ui.label(RichText::new(app.target_name(target)).strong());
        ui.label(RichText::new("⇧Tab cambia de vista").color(TEXT_DIM).size(10.5));
        // A frozen track's instrument, effects and clips can't be edited.
        if let engine::Target::Track(t) = target {
            if app.is_frozen(t) {
                ui.add_space(12.0);
                ui.label(RichText::new("✱ Pista congelada: suena su audio renderizado").color(theme::FROZEN));
                if ui.button("Descongelar").on_hover_text("Vuelve a sonar su instrumento y sus efectos, y se pueden editar").clicked() {
                    app.unfreeze(t);
                }
            }
        }
        if app.detail == Detail::Clip {
            ui.add_space(12.0);
            app.follow_button(ui);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (text, hint) = if app.editor_max {
                ("Restaurar", "Devuelve el panel a su altura (⇧⌘E o doble clic en su borde)")
            } else {
                ("Maximizar", "El editor ocupa casi toda la ventana; vuelve con lo mismo (⇧⌘E o doble clic en el borde del panel)")
            };
            maximize = widgets::flat_button(ui, text, app.editor_max, vec2(78.0, 20.0)).on_hover_text(theme::keys(hint)).clicked();
            // Here, not at the end of the chain: a long one would hide it.
            if app.detail == Detail::Devices {
                save_chain = crate::chains::save_menu(app, ui, target);
            }
        });
    });
    if let Some((name, with_instrument)) = save_chain {
        app.save_chain(app.selected_target, &name, with_instrument);
    }
    if maximize {
        app.toggle_editor_max();
    }
    ui.add_space(4.0);
    let frame = egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(4)).corner_radius(2);
    if app.detail_folded {
        frame.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let text = "Selecciona un clip para editarlo · doble clic en un slot vacío o en una pista del Arrangement crea uno nuevo";
            ui.add(egui::Label::new(RichText::new(text).color(TEXT_DIM).size(10.5)).truncate()).on_hover_text(text);
        });
        return;
    }
    frame.show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        // What doesn't fit scrolls inside: the panel never grows to hold it, so
        // the divider above can always bring it down.
        egui::ScrollArea::vertical().id_salt("detail").auto_shrink(false).show(ui, |ui| {
            // A whole number of points: what fills the view doesn't overflow it
            // by a fraction (and show a scroll bar for nothing).
            let size = vec2(ui.available_width(), ui.available_height().floor() - 1.0);
            ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
                let frozen = matches!(app.selected_target, engine::Target::Track(t) if app.is_frozen(t));
                ui.add_enabled_ui(!frozen, |ui| match app.detail {
                    Detail::Clip => piano_roll::show(app, ui),
                    Detail::Devices => devices::show(app, ui),
                });
            });
        });
    });
}
