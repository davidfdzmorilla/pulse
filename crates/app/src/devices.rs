//! Device chain view: the track's instrument followed by its audio effects.
//!
//! Continuous parameters are drawn from each device's parameter table, so every
//! knob shows and overrides automation the same way the mixer does.

use crate::app::PulseApp;
use crate::project::drum_name;
use crate::theme::{self, ACCENT, CONTROL, SURFACE, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine::model::{drum_note, Device, Effect, FilterMode, InstrumentKind, Param, SynthParams, Waveform};
use engine::{Command, Target};

const DEVICE_HEIGHT: f32 = 168.0;

fn beats_label(beats: f64) -> String {
    match beats {
        b if b == 0.25 => "1/16".into(),
        b if b == 0.5 => "1/8".into(),
        b if b == 0.75 => "3/16".into(),
        b if b == 1.0 => "1/4".into(),
        b if b == 1.5 => "3/8".into(),
        b if b == 2.0 => "1/2".into(),
        b if b == 4.0 => "1 Compás".into(),
        b if b == 8.0 => "2 Compases".into(),
        b if b == 16.0 => "4 Compases".into(),
        b => format!("{b} beats"),
    }
}

enum Action {
    Remove(usize),
    Move(usize, usize),
    /// Enable state or a discrete setting (mode, time division…) changed.
    Replace(usize, Device),
    Param(Param, f32),
    Instrument(InstrumentKind),
    Preview(u8),
}

/// Draws a device frame with a title bar and returns the body's UI result.
fn device_frame<R>(
    ui: &mut egui::Ui,
    title: &str,
    width: f32,
    enabled: Option<&mut bool>,
    header_extra: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let (rect, _) = ui.allocate_exact_size(vec2(width, DEVICE_HEIGHT), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, SURFACE);
    let title_rect = Rect::from_min_size(rect.min, vec2(width, 20.0));
    painter.rect_filled(title_rect, 3.0, CONTROL);

    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(title_rect.shrink2(vec2(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.spacing_mut().item_spacing.x = 4.0;
    if let Some(enabled) = enabled {
        let (dot, response) = child.allocate_exact_size(vec2(12.0, 12.0), Sense::click());
        child.painter().circle_filled(dot.center(), 5.0, if *enabled { ACCENT } else { theme::BG });
        child.painter().circle_stroke(dot.center(), 5.0, Stroke::new(1.0, TEXT_DIM));
        if response.on_hover_text("Activar / desactivar").clicked() {
            *enabled = !*enabled;
        }
    }
    child.label(egui::RichText::new(title).strong().size(11.5));
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), header_extra);

    let body_rect = Rect::from_min_max(pos2(rect.left() + 8.0, title_rect.bottom() + 6.0), rect.max - vec2(8.0, 6.0));
    let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body_rect).layout(egui::Layout::left_to_right(egui::Align::Min)));
    body_ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
    body(&mut body_ui)
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let target = app.selected_target;
    let mut actions = Vec::new();

    egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Target::Track(t) = target {
                if !app.project.tracks[t].is_audio() {
                    instrument_device(app, ui, t, &mut actions);
                }
            }

            let count = app.devices(target).len();
            for index in 0..count {
                let before = app.devices(target)[index];
                let mut device = before;
                let title = device.effect.label();
                let width = effect_width(&device.effect);
                let Device { effect, enabled } = &mut device;
                let mut header_actions = Vec::new();
                device_frame(
                    ui,
                    title,
                    width,
                    Some(enabled),
                    |ui| {
                        if ui.small_button("✕").on_hover_text("Eliminar").clicked() {
                            header_actions.push(Action::Remove(index));
                        }
                        if index + 1 < count && ui.small_button("▶").on_hover_text("Mover a la derecha").clicked() {
                            header_actions.push(Action::Move(index, index + 1));
                        }
                        if index > 0 && ui.small_button("◀").on_hover_text("Mover a la izquierda").clicked() {
                            header_actions.push(Action::Move(index, index - 1));
                        }
                    },
                    |ui| {
                        discrete_controls(ui, effect, index);
                        for (param, info) in effect.params().iter().enumerate() {
                            let id = Param::Device { device: index, param };
                            let value = app.display_value(target, id);
                            let state = app.automation_state(target, id);
                            if let Some(v) = widgets::param_knob(ui, info, value, info.name, 30.0, state) {
                                actions.push(Action::Param(id, v));
                            }
                        }
                    },
                );
                if device != before {
                    actions.push(Action::Replace(index, device));
                }
                actions.extend(header_actions);
            }

            let (rect, _) = ui.allocate_exact_size(vec2(170.0, DEVICE_HEIGHT), Sense::hover());
            ui.painter().rect_stroke(rect, 3.0, Stroke::new(1.0, theme::CONTROL), egui::StrokeKind::Inside);
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "Añade efectos\ndesde el navegador", theme::small(), TEXT_DIM);
        });
    });

    for action in actions {
        match action {
            Action::Remove(i) => app.remove_device(target, i),
            Action::Move(from, to) => app.move_device(target, from, to),
            Action::Replace(i, device) => {
                // Only discrete settings changed on the copy; knob values go through Param.
                app.devices_mut(target)[i] = device;
                app.update_device(target, i);
            }
            Action::Param(param, value) => app.set_param(target, param, value),
            Action::Instrument(kind) => {
                if let Target::Track(t) = target {
                    app.set_instrument(t, kind);
                }
            }
            Action::Preview(pitch) => {
                if let Target::Track(track) = target {
                    app.audio.send(Command::PreviewNote { track, pitch, velocity: 0.9 });
                }
            }
        }
    }
}

fn effect_width(effect: &Effect) -> f32 {
    // Knobs are 46 px wide plus spacing; option columns (combos, radios) take about 96 px.
    let (knobs, columns) = match effect {
        Effect::Eq3(_) => (5, 0),
        Effect::Compressor(_) => (5, 0),
        Effect::Delay(_) => (3, 1),
        Effect::Reverb(_) => (4, 0),
        Effect::Saturator(_) => (3, 0),
        Effect::AutoFilter(_) => (3, 2),
    };
    knobs as f32 * 52.0 + columns as f32 * 100.0 + 24.0
}

fn beats_combo(ui: &mut egui::Ui, id: &str, value: &mut f64, options: &[f64]) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(beats_label(*value))
        .width(84.0)
        .show_ui(ui, |ui| {
            for &option in options {
                ui.selectable_value(value, option, beats_label(option));
            }
        });
}

/// Settings that are choices rather than knobs: time divisions, modes, switches.
fn discrete_controls(ui: &mut egui::Ui, effect: &mut Effect, index: usize) {
    match effect {
        Effect::Delay(p) => {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Tiempo").color(TEXT_DIM).size(10.5));
                beats_combo(ui, &format!("delay{index}"), &mut p.time_beats, &[0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0]);
                ui.checkbox(&mut p.ping_pong, "Ping Pong");
            });
        }
        Effect::AutoFilter(p) => {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Tipo").color(TEXT_DIM).size(10.5));
                for mode in FilterMode::ALL {
                    ui.radio_value(&mut p.mode, mode, mode.label());
                }
            });
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("LFO").color(TEXT_DIM).size(10.5));
                beats_combo(ui, &format!("lfo{index}"), &mut p.lfo_beats, &[0.5, 1.0, 2.0, 4.0, 8.0, 16.0]);
            });
        }
        Effect::Eq3(_) | Effect::Compressor(_) | Effect::Reverb(_) | Effect::Saturator(_) => {}
    }
}

fn instrument_device(app: &PulseApp, ui: &mut egui::Ui, t: usize, actions: &mut Vec<Action>) {
    let target = Target::Track(t);
    match app.project.tracks[t].instrument {
        InstrumentKind::Synth(base) => {
            device_frame(ui, "Pulse Synth", 470.0, None, |_| {}, |ui| {
                let mut waveform = base.waveform;
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Oscilador").color(TEXT_DIM).size(10.5));
                    for w in Waveform::ALL {
                        ui.selectable_value(&mut waveform, w, w.label());
                    }
                });
                if waveform != base.waveform {
                    actions.push(Action::Instrument(InstrumentKind::Synth(SynthParams { waveform, ..base })));
                }
                // The envelope preview follows automation too.
                let mut shown = base;
                for i in 0..SynthParams::params().len() {
                    shown.set(i, app.display_value(target, Param::Instrument(i)));
                }
                let mut knob = |ui: &mut egui::Ui, i: usize| {
                    let info = &SynthParams::params()[i];
                    let id = Param::Instrument(i);
                    let state = app.automation_state(target, id);
                    if let Some(v) = widgets::param_knob(ui, info, shown.get(i), info.name, 30.0, state) {
                        actions.push(Action::Param(id, v));
                    }
                };
                ui.vertical(|ui| {
                    envelope_preview(ui, &shown);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for i in 0..4 {
                            knob(ui, i);
                        }
                    });
                });
                ui.vertical(|ui| {
                    knob(ui, 4);
                    knob(ui, 5);
                });
                knob(ui, 6);
            });
        }
        InstrumentKind::Drums => {
            device_frame(ui, "Drum Kit", 330.0, None, |_| {}, |ui| {
                egui::Grid::new("pads").spacing([4.0, 4.0]).show(ui, |ui| {
                    let pads = [drum_note::KICK, drum_note::SNARE, drum_note::CLAP, drum_note::CLOSED_HAT, drum_note::OPEN_HAT];
                    for (i, pitch) in pads.into_iter().enumerate() {
                        let label = format!("{}\n{}", drum_name(pitch).unwrap_or(""), crate::project::note_name(pitch));
                        let button = egui::Button::new(egui::RichText::new(label).size(10.5)).min_size(vec2(96.0, 54.0));
                        if ui.add(button).on_hover_text("Clic para escuchar").clicked() {
                            actions.push(Action::Preview(pitch));
                        }
                        if i % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
            });
        }
    }
}

/// Small ADSR curve, like the envelope displays on hardware synths.
fn envelope_preview(ui: &mut egui::Ui, p: &SynthParams) {
    let (rect, _) = ui.allocate_exact_size(vec2(192.0, 46.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, theme::BG);
    let total = p.attack + p.decay + 0.3 + p.release;
    let x = |t: f32| rect.left() + 4.0 + (rect.width() - 8.0) * (t / total);
    let y = |level: f32| rect.bottom() - 4.0 - (rect.height() - 8.0) * level;
    let points = vec![
        pos2(x(0.0), y(0.0)),
        pos2(x(p.attack), y(1.0)),
        pos2(x(p.attack + p.decay), y(p.sustain)),
        pos2(x(p.attack + p.decay + 0.3), y(p.sustain)),
        pos2(x(total), y(0.0)),
    ];
    painter.add(egui::Shape::line(points, Stroke::new(1.5, ACCENT)));
    painter.text(rect.right_top() + vec2(-4.0, 4.0), Align2::RIGHT_TOP, "ADSR", theme::small(), Color32::from_gray(110));
}
