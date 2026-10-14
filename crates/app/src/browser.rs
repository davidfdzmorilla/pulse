//! Left-hand browser: instruments and audio effects to add to the selected track.

use crate::app::PulseApp;
use crate::theme::{self, ACCENT, TEXT, TEXT_DIM};
use eframe::egui::{self, vec2, Align2, RichText, Sense};
use engine::model::{Device, Effect, InstrumentKind, SynthParams};
use engine::Target;

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).color(TEXT_DIM).size(10.5).strong());
    ui.add_space(2.0);
}

/// A full-width list row; returns true when clicked.
fn item(ui: &mut egui::Ui, icon: &str, label: &str, hint: &str) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 20.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 2.0, theme::CONTROL);
    }
    ui.painter().text(
        rect.left_center() + vec2(6.0, 0.0),
        Align2::LEFT_CENTER,
        icon,
        theme::small(),
        ACCENT,
    );
    ui.painter().text(rect.left_center() + vec2(22.0, 0.0), Align2::LEFT_CENTER, label, theme::body(), TEXT);
    response.on_hover_text(hint).clicked()
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    ui.label(RichText::new("Navegador").strong());
    let target = app.selected_target;
    ui.label(
        RichText::new(format!("Destino: {}", app.target_name(target)))
            .color(TEXT_DIM)
            .size(10.5),
    );
    ui.separator();

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        heading(ui, "INSTRUMENTOS");
        let track = match target {
            Target::Track(t) => Some(t),
            _ => None,
        };
        let hint = if track.is_some() {
            "Clic: reemplaza el instrumento de la pista seleccionada"
        } else {
            "Selecciona una pista MIDI para cambiar su instrumento"
        };
        if item(ui, "♫", "Pulse Synth", hint) {
            if let Some(t) = track {
                app.set_instrument(t, InstrumentKind::Synth(SynthParams::default()));
                app.detail = crate::app::Detail::Devices;
            }
        }
        if item(ui, "◉", "Drum Kit", hint) {
            if let Some(t) = track {
                app.set_instrument(t, InstrumentKind::Drums);
                app.detail = crate::app::Detail::Devices;
            }
        }

        heading(ui, "EFECTOS DE AUDIO");
        for effect in Effect::catalog() {
            if item(ui, "≈", effect.label(), "Clic: añade el efecto al final de la cadena seleccionada") {
                app.add_device(target, Device::new(effect));
            }
        }

        heading(ui, "PISTAS");
        if item(ui, "+", "Nueva pista MIDI", "⌘T") {
            app.add_track();
        }
    });
}
