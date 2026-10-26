//! Top control bar and bottom status bar.

use crate::app::{PulseApp, View};
use crate::midi_map::{Control, Transport};
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
    crate::widgets::name(&response, egui::WidgetType::Button, hint.split(" (").next().unwrap_or(hint));
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
        // A bar where recording starts (in) or stops (out), the recorded span beside it.
        "punch in" | "punch out" => {
            let c = rect.center();
            let x = if icon == "punch in" { c.x - 5.0 } else { c.x + 5.0 };
            ui.painter().line_segment([egui::pos2(x, c.y - 6.0), egui::pos2(x, c.y + 6.0)], egui::Stroke::new(2.0, color));
            let span = if icon == "punch in" { egui::Rect::from_min_max(egui::pos2(x + 3.0, c.y - 3.0), egui::pos2(x + 10.0, c.y + 3.0)) } else { egui::Rect::from_min_max(egui::pos2(x - 10.0, c.y - 3.0), egui::pos2(x - 3.0, c.y + 3.0)) };
            ui.painter().rect_filled(span, 1.0, color);
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

    // Wraps onto a second row when the window is too narrow for everything.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        // Tempo and launch settings.
        // With tempo automation, what plays now; moved by hand, it overrides it
        // (until «► Arrangement»), as any automated control.
        let automated = app.automation_state(engine::Target::Master, engine::model::Param::Tempo);
        let mut bpm = app.display_value(engine::Target::Master, engine::model::Param::Tempo) as f64;
        let hint = match automated {
            Some(false) => "Tempo: lo marca la automatización del Master. Moverlo a mano la anula hasta «► Arrangement»",
            Some(true) => "Tempo puesto a mano: la automatización está anulada («► Arrangement» la reactiva)",
            None => "Tempo (arrastra o escribe)",
        };
        let tempo = ui
            .add_sized([74.0, 22.0], egui::DragValue::new(&mut bpm).range(20.0..=999.0).speed(0.2).fixed_decimals(2))
            .on_hover_text(hint);
        if tempo.changed() {
            app.set_param(engine::Target::Master, engine::model::Param::Tempo, bpm as f32);
        }
        if automated == Some(false) {
            crate::widgets::outline(ui.painter(), tempo.rect, theme::METER_RED);
        }
        ui.label(RichText::new("BPM").color(TEXT_DIM).size(10.5));
        if icon_button(ui, "●", app.project.metronome, ACCENT, "Metrónomo").clicked() {
            press(app, Transport::Metronome);
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
        // While counting in, the beats left instead of the position.
        let count_in = app.audio.state().map_or(0.0, |s| s.count_in());
        let shown = if count_in > 0.0 { format!("Cuenta {}", count_in.ceil() as u32) } else { format_position(position) };
        readout(ui, &shown, 84.0);
        if icon_button(ui, "play", playing, PLAY, "Reproducir (Espacio)").clicked() {
            if app.learn.on {
                app.touched(Control::Transport(Transport::Play));
            } else {
                app.audio.send(Command::Play);
            }
        }
        if icon_button(ui, "stop", false, ACCENT, "Parar (Espacio)").clicked() {
            press(app, Transport::Stop);
        }
        let recording = app.is_arrangement_recording();
        if icon_button(ui, "●", recording, theme::METER_RED, "Grabar en el Arrangement las pistas armadas (si no hay ninguna, arma las seleccionadas) y, con «A» encendido, los controles que muevas como automatización (F9)").clicked() {
            press(app, Transport::Record);
        }
        let bars = app.settings.count_in_bars;
        let label = |b: u32| if b == 0 { "Sin cuenta".to_string() } else { format!("Cuenta {b} c.") };
        let mut chosen = bars;
        egui::ComboBox::from_id_salt("count_in")
            .selected_text(label(bars))
            .width(84.0)
            .show_ui(ui, |ui| {
                for b in [0, 1, 2, 4] {
                    ui.selectable_value(&mut chosen, b, label(b));
                }
            })
            .response
            .on_hover_text("Cuenta atrás: compases que suena el metrónomo antes de empezar a grabar");
        if chosen != bars {
            let mut next = app.settings.clone();
            next.count_in_bars = chosen;
            app.set_settings(next);
        }
        let armed = app.settings.automation_arm;
        let hint = if armed {
            "Armar automatización: encendido, al grabar se graban los controles que muevas. Clic para apagarlo"
        } else {
            "Armar automatización: apagado, al grabar no se graban los controles. Clic para encenderlo"
        };
        if icon_button(ui, "A", armed, theme::METER_RED, hint).clicked() {
            press(app, Transport::AutomationArm);
        }
        if icon_button(ui, "⟲", app.project.loop_enabled, ACCENT, "Loop del Arrangement").clicked() {
            press(app, Transport::Loop);
        }
        readout(ui, &format_position(app.project.loop_start), 84.0);
        readout(ui, &format!("{} c.", (app.project.loop_end - app.project.loop_start) / 4.0), 54.0);
        if icon_button(ui, "punch in", app.project.punch_in, theme::METER_RED, "Punch in: al grabar en el Arrangement, empieza a grabar en el inicio del loop (lo de antes solo suena)").clicked() {
            app.project.punch_in = !app.project.punch_in;
        }
        if icon_button(ui, "punch out", app.project.punch_out, theme::METER_RED, "Punch out: al grabar en el Arrangement, deja de grabar en el final del loop (y sigue sonando)").clicked() {
            app.project.punch_out = !app.project.punch_out;
        }

        ui.add_space(12.0);
        groove(app, ui);
        scale(app, ui);

        ui.add_space(12.0);
        let needed = app.back_to_arrangement_needed();
        if widgets::flat_button(ui, "► Arrangement", needed, vec2(104.0, 22.0))
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
        let voice_label = match (app.voice.enabled, app.voice.drums, app.voice.playing()) {
            (false, _, _) => "Voz".to_string(),
            (true, true, _) => match app.voice.last_hit() {
                Some(drum) => format!("Voz · {}", drum.name()),
                None => "Voz · …".to_string(),
            },
            (true, false, Some(pitch)) => format!("Voz · {}", crate::project::note_name(pitch)),
            (true, false, None) => "Voz · …".to_string(),
        };
        let voice = widgets::flat_button(ui, &voice_label, app.voice.enabled, vec2(if app.voice.enabled { 84.0 } else { 40.0 }, 22.0))
            .on_hover_text("Tocar con la voz: canta, tararea o silba al micrófono y suena la pista seleccionada (o las armadas), nota a nota; graba como un teclado MIDI. Con escala en el proyecto, las notas desafinadas van a la escala. O haz beatbox y toca una batería. Usa auriculares.\nClic derecho: notas o batería, octava, acordes, seguir el tono y enseñar tus sonidos");
        if voice.clicked() {
            app.toggle_voice();
        }
        voice.context_menu(|ui| {
            if ui.radio(!app.voice.drums, "Notas (cantar)").clicked() {
                app.voice.drums = false;
                ui.close();
            }
            if ui.radio(app.voice.drums, "Batería (beatbox)").on_hover_text("«pum»: bombo · «pf» o «ka»: caja · «ts»: charles, en la pista seleccionada (una Drum Machine)").clicked() {
                app.voice.drums = true;
                ui.close();
            }
            ui.separator();
            if app.voice.drums {
                use crate::beatbox::{Drum, TEACH};
                ui.label("Enseñar tus sonidos");
                for drum in Drum::ALL {
                    let taught = app.settings.beatbox.count(drum);
                    let label = format!("{} {}{}", drum.name(), drum.sound(), if taught > 0 { " ✓" } else { "" });
                    if ui.button(label).on_hover_text(format!("Haz el sonido {TEACH} veces: desde entonces, ese sonido tuyo toca el {}", drum.name().to_lowercase())).clicked() {
                        app.teach_drum(drum);
                        ui.close();
                    }
                }
                if !app.settings.beatbox.is_empty() && ui.button("Olvidar lo enseñado").clicked() {
                    app.forget_drums();
                    ui.close();
                }
                return;
            }
            ui.label("Octava");
            for (octave, label) in [(-2, "−2 (voz aguda → bajo)"), (-1, "−1"), (0, "Como suena"), (1, "+1"), (2, "+2 (voz grave → agudo)")] {
                if ui.radio(app.voice.octave == octave, label).clicked() {
                    app.voice.octave = octave;
                    ui.close();
                }
            }
            ui.separator();
            ui.label("Acordes");
            use crate::voice_play::Chords;
            for (chords, label) in [(Chords::Off, "No (una nota)"), (Chords::Triads, "Tríadas"), (Chords::Sevenths, "Con séptima")] {
                if ui.radio(app.voice.chords == chords, label).on_hover_text("La nota que cantas es la raíz de su acorde en la escala del proyecto (sin escala, o fuera de ella, un acorde mayor)").clicked() {
                    app.voice.chords = chords;
                    ui.close();
                }
            }
            ui.separator();
            ui.checkbox(&mut app.voice.bend, "Seguir el tono").on_hover_text("El vibrato y los deslizamientos de la voz mueven el pitch bend de la nota (hasta ±2 semitonos). Se oye en vivo; lo que se graba son las notas");
        });
        let devices = app.midi.names.len();
        let midi_hint = if devices == 0 {
            "Ningún dispositivo MIDI conectado. Clic para volver a buscar.".to_string()
        } else {
            format!("Entradas MIDI:\n{}\nClic para volver a buscar.", app.midi.names.join("\n"))
        };
        if widgets::toggle(ui, devices > 0, "MIDI", theme::PLAY, vec2(40.0, 22.0)).on_hover_text(midi_hint).clicked() {
            app.rescan_midi();
        }
        let learn_hint = "Asignar controles MIDI: toca un control de Pulse y mueve un knob o pulsa un botón del teclado MIDI";
        if widgets::toggle(ui, app.learn.on, "Asignar", ACCENT, vec2(60.0, 22.0)).on_hover_text(learn_hint).clicked() {
            app.toggle_learn();
        }

        // The view switch and meter at the right edge, if they fit after the rest
        // of the row; otherwise they follow on the next one.
        let right = 300.0 + if app.has_menu_bar() { 0.0 } else { 320.0 };
        // (In a wrapping row, what is left is up to the row's right edge.)
        if ui.max_rect().right() - ui.cursor().min.x < right {
            ui.end_row();
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
            // Without a system menu bar, the same menus inside the window.
            if !app.has_menu_bar() {
                in_app_menus(app, ui);
            }
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

/// The project's key: chosen by hand, detected from its clips, or none.
fn scale(app: &mut PulseApp, ui: &mut egui::Ui) {
    use stems::key::Key;
    let label = app.project.key.map_or("Escala".to_string(), |k| k.name());
    widgets::field_menu_button(ui, &label, |ui| {
        ui.set_min_width(230.0);
        if ui.button("Detectar en la canción").on_hover_text("Escucha las notas MIDI y el audio con una tonalidad clara").clicked() {
            app.detect_key();
            ui.close();
        }
        let mut follow = app.settings.loops_follow_key;
        if ui
            .checkbox(&mut follow, "Ajustar los loops nuevos a la escala")
            .on_hover_text("Los audios que añadas se transponen a la escala del proyecto (su tonalidad sale del nombre, como «Pad_Am», o del sonido); las baterías no. Si el proyecto no tiene escala, toma la del primero")
            .changed()
        {
            let settings = crate::settings::Settings { loops_follow_key: follow, ..app.settings.clone() };
            app.set_settings(settings);
        }
        ui.separator();
        if ui.selectable_label(app.project.key.is_none(), "Sin escala").clicked() {
            app.set_key(None);
            ui.close();
        }
        ui.separator();
        for minor in [false, true] {
            ui.label(RichText::new(if minor { "Menores" } else { "Mayores" }).color(TEXT_DIM).size(10.5));
            egui::Grid::new(("keys", minor)).spacing(vec2(2.0, 2.0)).show(ui, |ui| {
                for (i, key) in Key::all().filter(|k| k.minor == minor).enumerate() {
                    let name = key.name();
                    let short = name.split(' ').next().unwrap_or(&name);
                    if ui.selectable_label(app.project.key == Some(key), short).on_hover_text(&name).clicked() {
                        app.set_key(Some(key));
                        ui.close();
                    }
                    if i % 6 == 5 {
                        ui.end_row();
                    }
                }
            });
        }
    });
}

pub fn status_bar(app: &mut PulseApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let text = match (&app.audio.error, &app.status) {
            (Some(error), _) => RichText::new(format!("Sin audio: {error}")).color(theme::METER_RED),
            (None, Some(status)) => RichText::new(status).color(TEXT),
            (None, None) => RichText::new(crate::theme::keys(
                "Espacio: play/stop · Tab: Session/Arrangement · ⇧Tab: Clip/Dispositivos · M: teclado MIDI · F9: grabar · ⌘Z: deshacer · ⌘D: duplicar · Supr: borrar",
            ))
            .color(TEXT_DIM),
        };
        ui.label(text.size(10.5));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| app.update_badge(ui));
    });
}

/// A transport button: picked for MIDI learn while assigning, otherwise pressed.
fn press(app: &mut PulseApp, action: Transport) {
    if app.learn.on {
        app.touched(Control::Transport(action));
    } else {
        app.transport(action);
    }
}

/// The menu bar's menus as buttons, where there is no system menu bar.
fn in_app_menus(app: &mut PulseApp, ui: &mut egui::Ui) {
    use crate::native_menu::{Item, MENUS};
    let ctx = ui.ctx().clone();
    let mut chosen = None;
    for (title, items) in MENUS.iter() {
        ui.menu_button(*title, |ui| {
            for item in *items {
                match *item {
                    Item::Run(command, label, shortcut) => {
                        let label = if shortcut.is_empty() { label.to_string() } else { crate::theme::keys(&format!("{label}   ⌘{}", shortcut.to_uppercase().replace('⌃', "⌃⌘"))) };
                        if ui.add_enabled(app.command_enabled(command), egui::Button::new(label)).clicked() {
                            chosen = Some(command);
                            ui.close();
                        }
                    }
                    Item::Separator => {
                        ui.separator();
                    }
                    Item::Recent => {
                        ui.menu_button("Abrir recientes", |ui| {
                            for (i, path) in app.recent.iter().enumerate() {
                                let name = path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned());
                                if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
                                    chosen = Some(crate::commands::Command::OpenRecent(i));
                                    ui.close();
                                }
                            }
                            if app.recent.is_empty() {
                                ui.label("Ningún proyecto reciente");
                            }
                        });
                    }
                    Item::Templates => {
                        ui.menu_button("Nuevo desde plantilla", |ui| {
                            for (i, template) in app.templates.iter().enumerate() {
                                if i == crate::templates::BUILT_IN.len() {
                                    ui.separator();
                                }
                                if ui.button(&template.name).clicked() {
                                    chosen = Some(crate::commands::Command::NewFromTemplate(i));
                                    ui.close();
                                }
                            }
                        });
                    }
                    Item::System(..) => {}
                }
            }
        });
    }
    if let Some(command) = chosen {
        app.run(command, &ctx);
    }
}
