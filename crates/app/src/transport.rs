use crate::app::PulseApp;
use eframe::egui::{self, Color32, RichText};
use engine::Command;
use std::sync::atomic::Ordering;

const QUANTIZE_OPTIONS: [(f64, &str); 7] = [
    (0.0, "Ninguna"),
    (0.25, "1/16"),
    (0.5, "1/8"),
    (1.0, "1/4"),
    (2.0, "1/2"),
    (4.0, "1 compás"),
    (8.0, "2 compases"),
];

/// Formats a beat position as bar.beat.sixteenth, 1-based like most DAWs.
fn format_position(beats: f64) -> String {
    let bar = (beats / 4.0).floor() as i64 + 1;
    let beat = (beats.rem_euclid(4.0)).floor() as i64 + 1;
    let sixteenth = ((beats.fract()) * 4.0).floor() as i64 + 1;
    format!("{bar}.{beat}.{sixteenth}")
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let state = app.audio.state();
    let playing = state.as_ref().is_some_and(|s| s.playing.load(Ordering::Relaxed));
    let position = state.as_ref().map_or(0.0, |s| s.position());

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("PULSE").strong().size(18.0).color(Color32::from_rgb(242, 176, 72)));
        ui.separator();

        let play = egui::Button::new(RichText::new("▶").size(16.0))
            .fill(if playing { Color32::from_rgb(60, 140, 70) } else { Color32::from_gray(50) });
        if ui.add_sized([34.0, 26.0], play).on_hover_text("Reproducir (Espacio)").clicked() {
            app.audio.send(Command::Play);
        }
        if ui
            .add_sized([34.0, 26.0], egui::Button::new(RichText::new("■").size(16.0)))
            .on_hover_text("Parar (Espacio)")
            .clicked()
        {
            app.audio.send(Command::Stop);
        }

        ui.label(RichText::new(format_position(position)).monospace().size(16.0));
        ui.separator();

        let tempo = ui.add(
            egui::DragValue::new(&mut app.project.tempo)
                .range(20.0..=999.0)
                .speed(0.2)
                .fixed_decimals(2)
                .suffix(" BPM"),
        );
        if tempo.changed() {
            app.audio.send(Command::SetTempo(app.project.tempo));
        }

        if ui.toggle_value(&mut app.project.metronome, "Metrónomo").changed() {
            app.audio.send(Command::SetMetronome(app.project.metronome));
        }

        ui.label("Cuantización:");
        let current = QUANTIZE_OPTIONS
            .iter()
            .find(|(v, _)| *v == app.project.quantize)
            .map_or("?", |(_, l)| *l);
        let mut quantize = app.project.quantize;
        egui::ComboBox::from_id_salt("quantize")
            .selected_text(current)
            .width(90.0)
            .show_ui(ui, |ui| {
                for (value, label) in QUANTIZE_OPTIONS {
                    ui.selectable_value(&mut quantize, value, label);
                }
            });
        if quantize != app.project.quantize {
            app.project.quantize = quantize;
            app.audio.send(Command::SetQuantize(quantize));
        }

        ui.separator();
        ui.label("Master");
        let master = ui.add(
            egui::Slider::new(&mut app.project.master_volume, 0.0..=1.2).show_value(false),
        );
        if master.changed() {
            app.audio.send(Command::SetMasterVolume(app.project.master_volume));
        }
        crate::session::paint_meter(ui, egui::vec2(60.0, 10.0), app.master_meter);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match (&app.audio.error, app.audio.device_name()) {
            (Some(error), _) => {
                ui.colored_label(Color32::from_rgb(230, 90, 80), format!("Sin audio: {error}"));
            }
            (None, Some(name)) => {
                ui.weak(name);
            }
            _ => {}
        });
    });
    ui.add_space(4.0);
}
