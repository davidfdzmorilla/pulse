//! Left-hand browser: instruments and audio effects to add to the selected track.

use crate::app::PulseApp;
use crate::theme::{self, ACCENT, TEXT, TEXT_DIM};
use eframe::egui::{self, vec2, Align2, RichText, Sense};
use engine::model::{Device, Effect, InstrumentKind};
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
            Target::Track(t) if !app.project.tracks[t].is_audio() => Some(t),
            _ => None,
        };
        let hint = if track.is_some() {
            "Clic: reemplaza el instrumento de la pista seleccionada"
        } else {
            "Selecciona una pista MIDI para cambiar su instrumento"
        };
        for kind in InstrumentKind::catalog() {
            let icon = if kind.is_drums() { "◉" } else { "♫" };
            if item(ui, icon, kind.label(), hint) {
                if let Some(t) = track {
                    // An empty rack would be silent: start it with the basic kit.
                    if matches!(kind, InstrumentKind::DrumRack(_)) && app.project.tracks[t].samples.is_empty() {
                        app.load_basic_kit(t);
                    } else {
                        app.set_instrument(t, kind);
                    }
                    app.detail = crate::app::Detail::Devices;
                }
            }
        }

        heading(ui, "EFECTOS DE AUDIO");
        for effect in Effect::catalog() {
            if item(ui, "≈", effect.label(), "Clic: añade el efecto al final de la cadena seleccionada") {
                app.add_device(target, Device::new(effect));
            }
        }

        heading(ui, "PLUGINS (VST3 · CLAP)");
        crate::plugins::browser(app, ui, |ui, icon, label, hint| item(ui, icon, label, hint));

        heading(ui, "MUESTRAS");
        if item(ui, "♪", "Añadir muestra…", "Elige archivos de audio (WAV, AIFF, FLAC, MP3…). También puedes arrastrarlos a la ventana") {
            app.choose_samples(crate::app::Placement::Auto);
        }

        heading(ui, "PISTAS");
        if item(ui, "+", "Nueva pista MIDI", "⌘T") {
            app.add_track();
        }
        if item(ui, "+", "Nueva pista de audio", "⇧⌘T") {
            app.add_audio_track();
        }
    });
}
