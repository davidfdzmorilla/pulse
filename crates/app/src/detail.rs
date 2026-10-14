//! Bottom detail area: clip editor or device chain, switched with tabs or Shift+Tab.

use crate::app::{Detail, PulseApp};
use crate::theme::{self, TEXT_DIM};
use crate::{devices, piano_roll, widgets};
use eframe::egui::{self, vec2, RichText};

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
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
    });
    ui.add_space(4.0);
    let frame = egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(4)).corner_radius(2);
    frame.show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        match app.detail {
            Detail::Clip => piano_roll::show(app, ui),
            Detail::Devices => devices::show(app, ui),
        }
    });
}
