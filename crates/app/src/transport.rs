//! Top control bar and bottom status bar.

use crate::app::{PulseApp, View};
use crate::theme::{self, ACCENT, PLAY, TEXT, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, vec2, Align2, Color32, RichText, Sense};
use engine::Command;

const QUANTIZE_OPTIONS: [(f64, &str); 7] = [
    (0.0, "Ninguna"),
    (0.25, "1/16"),
    (0.5, "1/8"),
    (1.0, "1/4"),
    (2.0, "1/2"),
    (4.0, "1 Compás"),
    (8.0, "2 Compases"),
];

/// Formats a beat position as bar.beat.sixteenth, 1-based like most DAWs.
pub fn format_position(beats: f64) -> String {
    let bar = (beats / 4.0).floor() as i64 + 1;
    let beat = beats.rem_euclid(4.0).floor() as i64 + 1;
    let sixteenth = (beats.fract() * 4.0).floor() as i64 + 1;
    format!("{bar}. {beat}. {sixteenth}")
}

/// A boxed numeric readout like the tempo and position fields.
fn readout(ui: &mut egui::Ui, text: &str, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    ui.painter().rect_filled(rect, 2.0, theme::BG);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, egui::FontId::monospace(12.5), TEXT);
}

fn icon_button(ui: &mut egui::Ui, icon: &str, active: bool, active_color: Color32, hint: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 22.0), Sense::click());
    let fill = if active {
        active_color
    } else if response.hovered() {
        theme::CONTROL_HOVER
    } else {
        theme::CONTROL
    };
    ui.painter().rect_filled(rect, 2.0, fill);
    let color = if active { theme::text_on(active_color) } else { TEXT };
    match icon {
        "play" => widgets::play_triangle(ui.painter(), rect.center(), 10.0, color),
        "stop" => {
            ui.painter().rect_filled(egui::Rect::from_center_size(rect.center(), vec2(9.0, 9.0)), 1.0, color);
        }
        other => {
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, other, theme::body(), color);
        }
    }
    response.on_hover_text(hint)
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let playing = app.audio.is_playing();
    let position = app.audio.position();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        // Tempo and launch settings.
        let tempo = ui
            .add_sized(
                [74.0, 22.0],
                egui::DragValue::new(&mut app.project.tempo)
                    .range(20.0..=999.0)
                    .speed(0.2)
                    .fixed_decimals(2),
            )
            .on_hover_text("Tempo (arrastra o escribe)");
        if tempo.changed() {
            app.audio.send(Command::SetTempo(app.project.tempo));
        }
        ui.label(RichText::new("BPM").color(TEXT_DIM).size(10.5));
        if icon_button(ui, "●", app.project.metronome, ACCENT, "Metrónomo").clicked() {
            app.project.metronome = !app.project.metronome;
            app.audio.send(Command::SetMetronome(app.project.metronome));
        }
        let current = QUANTIZE_OPTIONS
            .iter()
            .find(|(v, _)| *v == app.project.quantize)
            .map_or("?", |(_, l)| *l);
        let mut quantize = app.project.quantize;
        egui::ComboBox::from_id_salt("quantize")
            .selected_text(current)
            .width(82.0)
            .show_ui(ui, |ui| {
                for (value, label) in QUANTIZE_OPTIONS {
                    ui.selectable_value(&mut quantize, value, label);
                }
            })
            .response
            .on_hover_text("Cuantización de lanzamiento");
        if quantize != app.project.quantize {
            app.project.quantize = quantize;
            app.audio.send(Command::SetQuantize(quantize));
        }

        ui.add_space(16.0);

        // Transport.
        readout(ui, &format_position(position), 84.0);
        if icon_button(ui, "play", playing, PLAY, "Reproducir (Espacio)").clicked() {
            app.audio.send(Command::Play);
        }
        if icon_button(ui, "stop", false, ACCENT, "Parar (Espacio)").clicked() {
            app.audio.send(Command::Stop);
        }
        let recording = app.is_arrangement_recording();
        if icon_button(ui, "●", recording, theme::METER_RED, "Grabar en el Arrangement las pistas armadas (F9)").clicked() {
            app.toggle_arrangement_record();
        }
        if icon_button(ui, "⟲", app.project.loop_enabled, ACCENT, "Loop del Arrangement").clicked() {
            app.project.loop_enabled = !app.project.loop_enabled;
            app.sync_loop();
        }
        readout(ui, &format_position(app.project.loop_start), 84.0);
        readout(ui, &format!("{} c.", (app.project.loop_end - app.project.loop_start) / 4.0), 54.0);

        ui.add_space(12.0);
        groove(app, ui);

        ui.add_space(12.0);
        let needed = app.back_to_arrangement_needed();
        if widgets::flat_button(ui, "▶ Arrangement", needed, vec2(104.0, 22.0))
            .on_hover_text("Volver al Arrangement: las pistas dejan los clips de sesión y la automatización que hayas\nmovido a mano vuelve a funcionar")
            .clicked()
        {
            app.back_to_arrangement();
        }

        ui.add_space(12.0);
        let keyboard = &app.keyboard;
        let label = if keyboard.enabled {
            format!("Teclado {} · {}", crate::project::note_name(keyboard.base_note()), keyboard.velocity)
        } else {
            "Teclado".to_string()
        };
        if widgets::flat_button(ui, &label, keyboard.enabled, vec2(if keyboard.enabled { 118.0 } else { 64.0 }, 22.0))
            .on_hover_text("Teclado del ordenador como teclado MIDI (M)\nA S D F G H J K L: teclas blancas · W E T Y U O P: negras\nZ / X: octava · C / V: velocidad")
            .clicked()
        {
            app.toggle_keyboard();
        }
        let devices = app.midi.names.len();
        let midi_hint = if devices == 0 {
            "Ningún dispositivo MIDI conectado. Clic para volver a buscar.".to_string()
        } else {
            format!("Entradas MIDI:\n{}\nClic para volver a buscar.", app.midi.names.join("\n"))
        };
        if widgets::toggle(ui, devices > 0, "MIDI", theme::PLAY, vec2(40.0, 22.0)).on_hover_text(midi_hint).clicked() {
            app.rescan_midi();
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            widgets::horizontal_meter(ui, vec2(70.0, 8.0), app.meters.master);
            ui.add_space(8.0);
            let arrangement = app.view == View::Arrangement;
            if widgets::flat_button(ui, "Arrangement", arrangement, vec2(92.0, 22.0)).on_hover_text("Tab").clicked() {
                app.view = View::Arrangement;
            }
            if widgets::flat_button(ui, "Session", !arrangement, vec2(68.0, 22.0)).on_hover_text("Tab").clicked() {
                app.view = View::Session;
            }
            ui.add_space(8.0);
            let redo = ui.add_enabled(app.can_redo(), egui::Button::new("Rehacer")).on_hover_text("⌘⇧Z");
            if redo.clicked() {
                app.redo();
            }
            let undo = ui.add_enabled(app.can_undo(), egui::Button::new("Deshacer")).on_hover_text("⌘Z");
            if undo.clicked() {
                app.undo();
            }
            ui.add_space(8.0);
            ui.menu_button("Archivo", |ui| {
                if ui.button("Nuevo   ⌘N").clicked() {
                    app.new_project();
                    ui.close();
                }
                if ui.button("Abrir…   ⌘O").clicked() {
                    app.open();
                    ui.close();
                }
                if ui.button("Importar set de Live (.als)…   ⌘I").clicked() {
                    app.import_live_set();
                    ui.close();
                }
                if ui.button("Exportar audio…   ⇧⌘R").clicked() {
                    app.export.open = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("Guardar   ⌘S").clicked() {
                    app.save();
                    ui.close();
                }
                if ui.button("Guardar como…   ⇧⌘S").clicked() {
                    app.save_as();
                    ui.close();
                }
            });
            if widgets::toggle(ui, app.show_browser, "☰", theme::CONTROL_HOVER, vec2(24.0, 22.0))
                .on_hover_text("Mostrar u ocultar el navegador")
                .clicked()
            {
                app.show_browser = !app.show_browser;
            }
        });
    });
}

/// Record quantization and playback swing.
fn groove(app: &mut PulseApp, ui: &mut egui::Ui) {
    const RECORD_GRIDS: [(Option<f64>, &str); 7] = [
        (None, "Rec Q: Off"),
        (Some(1.0), "Rec Q: 1/4"),
        (Some(0.5), "Rec Q: 1/8"),
        (Some(1.0 / 3.0), "Rec Q: 1/8T"),
        (Some(0.25), "Rec Q: 1/16"),
        (Some(1.0 / 6.0), "Rec Q: 1/16T"),
        (Some(0.125), "Rec Q: 1/32"),
    ];
    let current = RECORD_GRIDS
        .iter()
        .find(|(g, _)| *g == app.project.record_quantize)
        .map_or("Rec Q", |(_, l)| *l);
    egui::ComboBox::from_id_salt("record_quantize")
        .selected_text(current)
        .width(92.0)
        .show_ui(ui, |ui| {
            for (grid, text) in RECORD_GRIDS {
                ui.selectable_value(&mut app.project.record_quantize, grid, text);
            }
        })
        .response
        .on_hover_text("Cuantización al grabar: ajusta el inicio de las notas grabadas a esta rejilla");

    let mut percent = app.project.swing * 100.0;
    let swing = ui
        .add_sized([62.0, 22.0], egui::DragValue::new(&mut percent).range(0.0..=100.0).speed(0.5).prefix("Swing ").suffix("%").fixed_decimals(0))
        .on_hover_text("Swing al reproducir: retrasa los contratiempos. 100 % = shuffle de tresillo. No cambia las notas.");
    let mut grid = app.project.swing_grid;
    egui::ComboBox::from_id_salt("swing_grid")
        .selected_text(if grid == 0.5 { "1/8" } else { "1/16" })
        .width(48.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut grid, 0.5, "1/8");
            ui.selectable_value(&mut grid, 0.25, "1/16");
        });
    if swing.changed() || grid != app.project.swing_grid {
        app.project.swing = percent / 100.0;
        app.project.swing_grid = grid;
        app.audio.send(Command::SetSwing { amount: app.project.swing, grid });
    }
}

pub fn status_bar(app: &mut PulseApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let text = match (&app.audio.error, &app.status) {
            (Some(error), _) => RichText::new(format!("Sin audio: {error}")).color(theme::METER_RED),
            (None, Some(status)) => RichText::new(status).color(TEXT),
            (None, None) => RichText::new(
                "Espacio: play/stop · Tab: Session/Arrangement · ⇧Tab: Clip/Dispositivos · M: teclado MIDI · F9: grabar · ⌘Z: deshacer · ⌘D: duplicar · Supr: borrar",
            )
            .color(TEXT_DIM),
        };
        ui.label(text.size(10.5));
    });
}
