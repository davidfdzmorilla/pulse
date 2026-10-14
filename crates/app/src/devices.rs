//! Device chain view: the track's instrument followed by its audio effects.
//!
//! Continuous parameters are drawn from each device's parameter table, so every
//! knob shows and overrides automation the same way the mixer does.

use crate::app::PulseApp;
use crate::presets::Preset;
use crate::theme::{self, ACCENT, CONTROL, SURFACE, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine::model::{
    Device, Effect, FilterMode, InstrumentKind, KickDrive, KickParams, Param, RackParams, SamplerParams, SynthParams, Waveform, RACK_FIRST_NOTE,
    RACK_PADS,
};
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
    ApplyPreset(Preset),
    SavePreset(String),
    DeletePreset(Preset),
    Preview(u8),
    /// Opens a file dialog for sample slot `n`.
    LoadSamples(usize),
    ClearSample(usize),
    SelectPad(usize),
    BasicKit,
    SliceFile,
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
    let mut zones = Vec::new();

    egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Target::Track(t) = target {
                if !app.project.tracks[t].is_audio() {
                    instrument_device(app, ui, t, &mut actions, &mut zones);
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

    app.sample_drop_zones.extend(zones);
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
            Action::ApplyPreset(preset) => {
                if let Target::Track(t) = target {
                    app.apply_preset(t, preset);
                }
            }
            Action::SavePreset(name) => {
                if let Target::Track(t) = target {
                    app.save_preset(t, name);
                }
            }
            Action::DeletePreset(preset) => app.delete_preset(&preset),
            Action::Preview(pitch) => {
                if let Target::Track(track) = target {
                    app.audio.send(Command::PreviewNote { track, pitch, velocity: 0.9 });
                }
            }
            Action::LoadSamples(slot) => {
                if let Target::Track(t) = target {
                    app.choose_instrument_samples(t, slot);
                }
            }
            Action::ClearSample(slot) => {
                if let Target::Track(t) = target {
                    app.clear_instrument_sample(t, slot);
                }
            }
            Action::SelectPad(pad) => app.selected_pad = pad,
            Action::BasicKit => {
                if let Target::Track(t) = target {
                    app.load_basic_kit(t);
                }
            }
            Action::SliceFile => {
                if let Target::Track(t) = target {
                    app.slice_file_into(t);
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
        Effect::Chorus(_) => (4, 0),
        Effect::Phaser(_) => (5, 0),
        Effect::Bitcrusher(_) => (3, 0),
        Effect::Ducker(_) => (2, 1),
        Effect::Limiter(_) => (3, 0),
        Effect::Utility(_) => (2, 0),
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
        Effect::Ducker(p) => {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Ritmo").color(TEXT_DIM).size(10.5));
                beats_combo(ui, &format!("ducker{index}"), &mut p.rate_beats, &[0.25, 0.5, 1.0, 2.0, 4.0]);
            });
        }
        Effect::Eq3(_)
        | Effect::Compressor(_)
        | Effect::Reverb(_)
        | Effect::Saturator(_)
        | Effect::Chorus(_)
        | Effect::Phaser(_)
        | Effect::Bitcrusher(_)
        | Effect::Limiter(_)
        | Effect::Utility(_) => {}
    }
}

type Zones = Vec<(Rect, usize, usize)>;

fn instrument_device(app: &PulseApp, ui: &mut egui::Ui, t: usize, actions: &mut Vec<Action>, zones: &mut Zones) {
    let target = Target::Track(t);
    let kind = app.project.tracks[t].instrument;
    // Knob for instrument parameter `i`, showing automation like every other control.
    let knob = |ui: &mut egui::Ui, actions: &mut Vec<Action>, i: usize, label: &str, diameter: f32| {
        let Some(info) = kind.params().get(i) else { return };
        let id = Param::Instrument(i);
        let value = app.display_value(target, id);
        if let Some(v) = widgets::param_knob(ui, info, value, label, diameter, app.automation_state(target, id)) {
            actions.push(Action::Param(id, v));
        }
    };
    // Actions from the title bar (presets and the like).
    let mut head = Vec::new();
    match kind {
        InstrumentKind::Synth(base) => {
            device_frame(ui, kind.label(), 470.0, None, |ui| preset_menu(app, ui, t, &mut head), |ui| {
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
                ui.vertical(|ui| {
                    envelope_preview(ui, &shown);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (i, name) in ["Attack", "Decay", "Sustain", "Release"].into_iter().enumerate() {
                            knob(ui, actions, i, name, 30.0);
                        }
                    });
                });
                ui.vertical(|ui| {
                    knob(ui, actions, 4, "Filter", 30.0);
                    knob(ui, actions, 5, "Res", 30.0);
                });
                knob(ui, actions, 6, "Volume", 30.0);
            });
        }
        InstrumentKind::Drums | InstrumentKind::DrumMachine(_) => {
            device_frame(ui, kind.label(), 600.0, None, |ui| preset_menu(app, ui, t, &mut head), |ui| {
                egui::Grid::new("drum_machine").spacing([8.0, 2.0]).show(ui, |ui| {
                    for (voice, (pitch, name)) in engine::model::DRUM_VOICES.into_iter().enumerate() {
                        ui.vertical(|ui| {
                            let pad = egui::Button::new(egui::RichText::new(name).size(10.5)).min_size(vec2(136.0, 16.0));
                            if ui.add(pad).on_hover_text(format!("{} · clic para escuchar", crate::project::note_name(pitch))).clicked() {
                                actions.push(Action::Preview(pitch));
                            }
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                for (k, label) in ["Tune", "Decay", "Level"].into_iter().enumerate() {
                                    knob(ui, actions, voice * 3 + k, label, 18.0);
                                }
                            });
                        });
                        if voice % 4 == 3 {
                            ui.end_row();
                        }
                    }
                });
            });
        }
        InstrumentKind::Acid(base) => {
            let width = kind.params().len() as f32 * 52.0 + 110.0;
            device_frame(ui, kind.label(), width, None, |ui| preset_menu(app, ui, t, &mut head), |ui| {
                let mut square = base.square;
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Oscilador").color(TEXT_DIM).size(10.5));
                    ui.selectable_value(&mut square, false, "Saw");
                    ui.selectable_value(&mut square, true, "Square");
                });
                if square != base.square {
                    actions.push(Action::Instrument(InstrumentKind::Acid(engine::model::AcidParams { square, ..base })));
                }
                for (i, info) in kind.params().iter().enumerate() {
                    knob(ui, actions, i, info.name, 30.0);
                }
            });
        }
        InstrumentKind::Sampler(base) => {
            device_frame(ui, kind.label(), 560.0, None, |ui| preset_menu(app, ui, t, &mut head), |ui| {
                ui.vertical(|ui| {
                    let rect = sample_slot(app, ui, t, 0, vec2(220.0, 78.0), zones);
                    waveform(app, ui, t, 0, rect, app.display_value(target, Param::Instrument(2)));
                    ui.horizontal(|ui| {
                        if ui.small_button("Cargar…").on_hover_text("También puedes arrastrar un archivo de audio aquí").clicked() {
                            actions.push(Action::LoadSamples(0));
                        }
                        if app.project.tracks[t].sample(0).is_some() && ui.small_button("✕").on_hover_text("Quitar la muestra").clicked() {
                            actions.push(Action::ClearSample(0));
                        }
                        if ui.small_button("▶").on_hover_text("Escuchar en la nota raíz").clicked() {
                            actions.push(Action::Preview(base.root.round() as u8));
                        }
                    });
                    let mut mode = (base.one_shot, base.looping);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.selectable_value(&mut mode, (false, false), "Clásico").on_hover_text("Suena mientras se mantiene la nota");
                        ui.selectable_value(&mut mode, (true, false), "One-shot").on_hover_text("Suena entera, sin importar la duración de la nota");
                        ui.selectable_value(&mut mode, (false, true), "Loop").on_hover_text("Repite desde Start mientras se mantiene la nota");
                    });
                    if mode != (base.one_shot, base.looping) {
                        let (one_shot, looping) = mode;
                        actions.push(Action::Instrument(InstrumentKind::Sampler(SamplerParams { one_shot, looping, ..base })));
                    }
                });
                ui.vertical(|ui| {
                    for row in [[0, 1, 2, 7, 8], [3, 4, 5, 6, 9]] {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for i in row {
                                knob(ui, actions, i, kind.params()[i].name, 26.0);
                            }
                        });
                    }
                });
            });
        }
        InstrumentKind::DrumRack(base) => {
            let pad = app.selected_pad.min(RACK_PADS - 1);
            let header = |ui: &mut egui::Ui| {
                preset_menu(app, ui, t, &mut head);
                if ui.small_button("Kit básico").on_hover_text("Carga sonidos de la Drum Machine en los pads").clicked() {
                    head.push(Action::BasicKit);
                }
                if ui.small_button("Trocear archivo…").on_hover_text("Corta un loop en golpes, uno por pad, y crea el clip que lo toca").clicked() {
                    head.push(Action::SliceFile);
                }
            };
            let mut body_actions = Vec::new();
            device_frame(ui, kind.label(), 600.0, None, header, |ui| {
                pad_grid(app, ui, t, pad, &mut body_actions, zones);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    let track = &app.project.tracks[t];
                    let note = RACK_FIRST_NOTE + pad as u8;
                    let name = track.sample_name(pad).unwrap_or_else(|| "Vacío".into());
                    ui.label(egui::RichText::new(format!("Pad {} · {}", pad + 1, crate::project::note_name(note))).color(TEXT_DIM).size(10.5));
                    ui.label(egui::RichText::new(name).strong());
                    ui.horizontal(|ui| {
                        if ui.small_button("Cargar…").on_hover_text("Varios archivos llenan los pads siguientes").clicked() {
                            body_actions.push(Action::LoadSamples(pad));
                        }
                        if track.sample(pad).is_some() && ui.small_button("✕").on_hover_text("Vaciar el pad").clicked() {
                            body_actions.push(Action::ClearSample(pad));
                        }
                        let mut choke = base.choke[pad];
                        let label = |g: u8| if g == 0 { "Sin choke".to_string() } else { format!("Choke {g}") };
                        egui::ComboBox::from_id_salt(("choke", t))
                            .selected_text(label(choke))
                            .width(80.0)
                            .show_ui(ui, |ui| {
                                for g in 0..=4 {
                                    ui.selectable_value(&mut choke, g, label(g));
                                }
                            })
                            .response
                            .on_hover_text("Los pads del mismo grupo se cortan entre sí (como el hi-hat abierto y cerrado)");
                        if choke != base.choke[pad] {
                            let mut params: RackParams = base;
                            params.choke[pad] = choke;
                            body_actions.push(Action::Instrument(InstrumentKind::DrumRack(params)));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (k, label) in ["Tune", "Length", "Level", "Pan"].into_iter().enumerate() {
                            knob(ui, &mut body_actions, pad * 4 + k, label, 28.0);
                        }
                    });
                });
            });
            actions.extend(body_actions);
        }
        InstrumentKind::Kick(base) => {
            let header = |ui: &mut egui::Ui| preset_menu(app, ui, t, &mut head);
            let mut body_actions = Vec::new();
            device_frame(ui, kind.label(), 680.0, None, header, |ui| {
                // The display follows automation, like the knobs.
                let mut shown = kind;
                for i in 0..kind.params().len() {
                    shown.set(i, app.display_value(target, Param::Instrument(i)));
                }
                let InstrumentKind::Kick(shown) = shown else { return };
                ui.vertical(|ui| {
                    let click = app.project.tracks[t].sample(0).and_then(|path| app.loaded_sample(path)).cloned();
                    kick_editor(ui, t, click, &shown, base, &mut body_actions);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let mut mode = base.drive_mode;
                        for m in KickDrive::ALL {
                            ui.selectable_value(&mut mode, m, m.label()).on_hover_text("Tipo de distorsión del cuerpo");
                        }
                        let mut follow = base.key_follow;
                        ui.checkbox(&mut follow, "Key").on_hover_text("La nota tocada afina el cuerpo (dos octavas por debajo) en lugar de Tune");
                        if mode != base.drive_mode || follow != base.key_follow {
                            body_actions.push(Action::Instrument(InstrumentKind::Kick(KickParams { drive_mode: mode, key_follow: follow, ..base })));
                        }
                        if ui.small_button("▶").on_hover_text("Escuchar").clicked() {
                            body_actions.push(Action::Preview(60));
                        }
                    });
                    click_slot(app, ui, t, &mut body_actions, zones);
                });
                ui.vertical(|ui| {
                    for row in [[0, 1, 2, 3, 8, 12, 13], [4, 5, 6, 7, 9, 10, 11]] {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for i in row {
                                knob(ui, &mut body_actions, i, kind.params()[i].name, 26.0);
                            }
                        });
                    }
                });
            });
            actions.extend(body_actions);
        }
        InstrumentKind::SuperSaw(_) | InstrumentKind::Fm(_) => {
            let width = kind.params().len() as f32 * 52.0 + 24.0;
            device_frame(ui, kind.label(), width, None, |ui| preset_menu(app, ui, t, &mut head), |ui| {
                for (i, info) in kind.params().iter().enumerate() {
                    knob(ui, actions, i, info.name, 30.0);
                }
            });
        }
    }
    actions.extend(head);
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

/// True while files are dragged over the window.
fn files_hovering(ui: &egui::Ui) -> bool {
    ui.ctx().input(|i| !i.raw.hovered_files.is_empty())
}

/// Allocates an area that accepts dropped samples for `slot` and returns it.
fn sample_slot(app: &PulseApp, ui: &mut egui::Ui, t: usize, slot: usize, size: egui::Vec2, zones: &mut Zones) -> Rect {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    zones.push((rect, t, slot));
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, theme::BG);
    if files_hovering(ui) {
        painter.rect_stroke(rect, 2.0, Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
    }
    if app.project.tracks[t].sample(slot).is_none() {
        painter.text(rect.center(), Align2::CENTER_CENTER, "Arrastra una muestra aquí", theme::small(), TEXT_DIM);
    }
    rect
}

/// Draws the sample in `slot` with its start point.
fn waveform(app: &PulseApp, ui: &egui::Ui, t: usize, slot: usize, rect: Rect, start: f32) {
    let track = &app.project.tracks[t];
    let Some(sample) = track.sample(slot).and_then(|p| app.loaded_sample(p)) else { return };
    let painter = ui.painter_at(rect);
    let peaks = &sample.peaks;
    if !peaks.is_empty() {
        let columns = rect.width() as usize;
        let mid = rect.center().y;
        let half = rect.height() * 0.42;
        for x in 0..columns {
            let from = x * peaks.len() / columns;
            let to = ((x + 1) * peaks.len() / columns).max(from + 1).min(peaks.len());
            let (lo, hi) = peaks[from..to].iter().fold((0f32, 0f32), |(a, b), &(lo, hi)| (a.min(lo), b.max(hi)));
            let px = rect.left() + x as f32 + 0.5;
            painter.line_segment([pos2(px, mid - hi * half), pos2(px, mid - lo * half)], Stroke::new(1.0, Color32::from_gray(170)));
        }
    }
    let x = rect.left() + rect.width() * start;
    painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.5, ACCENT));
    let name = track.sample_name(slot).unwrap_or_default();
    painter.text(rect.left_top() + vec2(4.0, 3.0), Align2::LEFT_TOP, name, theme::small(), theme::TEXT);
}

/// The rack's sixteen pads, pad 1 at the bottom left as on hardware controllers.
fn pad_grid(app: &PulseApp, ui: &mut egui::Ui, t: usize, selected: usize, actions: &mut Vec<Action>, zones: &mut Zones) {
    const PAD: egui::Vec2 = vec2(70.0, 30.0);
    let (area, _) = ui.allocate_exact_size(vec2(4.0 * (PAD.x + 3.0), 4.0 * (PAD.y + 3.0)), Sense::hover());
    let track = &app.project.tracks[t];
    let hovering = files_hovering(ui);
    for pad in 0..RACK_PADS {
        let (column, row) = (pad % 4, 3 - pad / 4);
        let min = area.min + vec2(column as f32 * (PAD.x + 3.0), row as f32 * (PAD.y + 3.0));
        let rect = Rect::from_min_size(min, PAD);
        zones.push((rect, t, pad));
        let response = ui.interact(rect, ui.id().with(("pad", pad)), Sense::click());
        let name = track.sample_name(pad);
        let fill = match (pad == selected, name.is_some(), response.hovered()) {
            (true, ..) => theme::CONTROL_HOVER,
            (_, true, true) => theme::SLOT_HOVER,
            (_, true, false) => theme::CONTROL,
            (_, false, _) => theme::SLOT,
        };
        let painter = ui.painter();
        painter.rect_filled(rect, 3.0, fill);
        if pad == selected || (hovering && response.contains_pointer()) {
            painter.rect_stroke(rect, 3.0, Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
        }
        let note = RACK_FIRST_NOTE + pad as u8;
        painter.text(rect.left_top() + vec2(4.0, 3.0), Align2::LEFT_TOP, crate::project::note_name(note), theme::small(), TEXT_DIM);
        if let Some(name) = name {
            let short: String = name.chars().take(11).collect();
            painter.text(rect.left_bottom() + vec2(4.0, -3.0), Align2::LEFT_BOTTOM, short, theme::small(), theme::TEXT);
        }
        let response = response.on_hover_text(track.sample_name(pad).unwrap_or_else(|| "Pad vacío: arrastra una muestra".into()));
        if response.clicked() {
            actions.push(Action::SelectPad(pad));
            actions.push(Action::Preview(note));
        }
    }
}

/// The kick's envelope editor: the hit's waveform with the pitch or the level
/// curve on top, both drawable by hand.
///
/// Drag a point to move it, double-click to add or remove one, drag the small
/// handle in the middle of a segment to bend it; right-click to reset.
fn kick_editor(ui: &mut egui::Ui, t: usize, click: Option<std::sync::Arc<engine::Sample>>, shown: &KickParams, base: KickParams, actions: &mut Vec<Action>) {
    let (rect, response) = ui.allocate_exact_size(vec2(260.0, 84.0), Sense::click_and_drag());
    let id = ui.id().with(("kick_editor", t));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::BG);

    // Which curve is edited, chosen with the tabs in the corner.
    let mut amp_tab: bool = ui.ctx().memory(|m| m.data.get_temp(id.with("tab"))).unwrap_or(false);
    let tabs = [(false, "Tono"), (true, "Amp")];
    let mut x = rect.left() + 4.0;
    for (tab, label) in tabs {
        let galley = painter.layout_no_wrap(label.into(), theme::small(), theme::TEXT);
        let tab_rect = Rect::from_min_size(pos2(x, rect.top() + 2.0), galley.size() + vec2(8.0, 2.0));
        painter.rect_filled(tab_rect, 2.0, if tab == amp_tab { theme::CONTROL_HOVER } else { theme::SURFACE });
        painter.galley(tab_rect.min + vec2(4.0, 1.0), galley, theme::TEXT);
        if response.clicked() && response.interact_pointer_pos().is_some_and(|p| tab_rect.contains(p)) {
            amp_tab = tab;
            ui.ctx().memory_mut(|m| m.data.insert_temp(id.with("tab"), amp_tab));
        }
        x = tab_rect.right() + 3.0;
    }

    // The rendered hit, cached until a setting or the click sample changes.
    let key = (*shown, click.as_ref().map(|c| std::sync::Arc::as_ptr(c) as usize));
    type Cached = ((KickParams, Option<usize>), std::sync::Arc<Vec<f32>>);
    const PREVIEW_RATE: f32 = 12_000.0;
    let hit = match ui.ctx().memory(|m| m.data.get_temp::<Cached>(id.with("hit"))) {
        Some((k, hit)) if k == key => hit,
        _ => {
            let hit = std::sync::Arc::new(engine::render_kick(*shown, click, PREVIEW_RATE));
            ui.ctx().memory_mut(|m| m.data.insert_temp(id.with("hit"), (key, hit.clone())));
            hit
        }
    };

    let length = engine::kick_length_ms(shown);
    let decay_start = shown.attack.max(0.05) + shown.hold;
    // Milliseconds shown across the editor, and where the drawn shape starts and spans.
    let (span_ms, shape_start, shape_ms) = if amp_tab { (length, decay_start, shown.decay.max(1.0)) } else { (shown.pitch_time.max(0.1), 0.0, shown.pitch_time.max(0.1)) };
    let x_of_ms = |ms: f32| rect.left() + ms / span_ms * rect.width();
    let ms_of_x = |x: f32| (x - rect.left()) / rect.width() * span_ms;
    let (top, bottom) = (rect.top() + 16.0, rect.bottom() - 4.0);
    let y_of = |v: f32| bottom - v * (bottom - top);
    let v_of = |y: f32| ((bottom - y) / (bottom - top)).clamp(0.0, 1.0);
    let x_of_t = |time: f32| x_of_ms(shape_start + time * shape_ms);
    let t_of_x = |x: f32| ((ms_of_x(x) - shape_start) / shape_ms).clamp(0.0, 1.0);

    // Waveform of the part of the hit in view.
    let columns = rect.width() as usize;
    let (mid, half) = (rect.center().y + 6.0, (rect.height() - 16.0) * 0.45);
    for x in 0..columns {
        let from = ((x as f32 / columns as f32 * span_ms) * PREVIEW_RATE / 1000.0) as usize;
        let to = ((((x + 1) as f32 / columns as f32) * span_ms) * PREVIEW_RATE / 1000.0) as usize;
        let slice = &hit[from.min(hit.len())..to.max(from + 1).min(hit.len())];
        let (lo, hi) = slice.iter().fold((0f32, 0f32), |(lo, hi), &s| (lo.min(s), hi.max(s)));
        let px = rect.left() + x as f32 + 0.5;
        painter.line_segment([pos2(px, mid - hi.clamp(-1.2, 1.2) * half), pos2(px, mid - lo.clamp(-1.2, 1.2) * half)], Stroke::new(1.0, Color32::from_rgb(60, 110, 70)));
    }

    // The curve as it sounds (with the curve knob's tension), then the drawn shape.
    let (shape, tension) = if amp_tab { (base.amp_shape, shown.decay_curve) } else { (base.pitch_shape, shown.pitch_curve) };
    let effective: Vec<_> = (0..=columns)
        .map(|x| {
            let px = rect.left() + x as f32;
            let ms = ms_of_x(px);
            let v = if amp_tab { engine::kick_amplitude(shown, ms) } else { engine::kick_pitch_offset(shown, ms) / shown.pitch_amount.max(0.001) };
            pos2(px, y_of(v))
        })
        .collect();
    painter.add(egui::Shape::line(effective, Stroke::new(1.0, theme::shade(ACCENT, 0.5))));
    let drawn: Vec<_> = (0..=columns)
        .filter_map(|x| {
            let px = rect.left() + x as f32;
            let ms = ms_of_x(px);
            (ms >= shape_start).then(|| pos2(px, y_of(shape.value_at(t_of_x(px)))))
        })
        .collect();
    painter.add(egui::Shape::line(drawn, Stroke::new(1.5, ACCENT)));
    if tension > 0.01 {
        painter.text(rect.right_bottom() + vec2(-4.0, -3.0), Align2::RIGHT_BOTTOM, "+ tensión", theme::small(), TEXT_DIM);
    }

    let points = shape.points();
    let point_pos = |i: usize| pos2(x_of_t(points[i].time), y_of(points[i].value));
    let bend_pos = |i: usize| {
        let (a, b) = (points[i - 1], points[i]);
        let time = (a.time + b.time) * 0.5;
        pos2(x_of_t(time), y_of(shape.value_at(time)))
    };
    for i in 1..points.len() {
        painter.circle_stroke(bend_pos(i), 2.5, Stroke::new(1.0, theme::shade(ACCENT, 0.8)));
    }
    let pointer = response.hover_pos();
    for i in 0..points.len() {
        let hovered = pointer.is_some_and(|p| p.distance(point_pos(i)) < 6.0);
        painter.circle_filled(point_pos(i), if hovered { 4.5 } else { 3.5 }, if hovered { Color32::WHITE } else { ACCENT });
    }

    // Editing.
    let nearest = |pos: egui::Pos2| -> Option<(bool, usize)> {
        let point = (0..points.len()).map(|i| (point_pos(i).distance(pos), i)).min_by(|a, b| a.0.total_cmp(&b.0));
        let bend = (1..points.len()).map(|i| (bend_pos(i).distance(pos), i)).min_by(|a, b| a.0.total_cmp(&b.0));
        match (point, bend) {
            (Some((d, i)), _) if d < 7.0 => Some((false, i)),
            (_, Some((d, i))) if d < 6.0 => Some((true, i)),
            _ => None,
        }
    };
    let mut edited = shape;
    let grab_id = id.with("grab");
    if response.drag_started() {
        // Where the button went down: a drag only starts after the pointer has moved.
        let origin = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
        let grab = origin.and_then(nearest);
        ui.ctx().memory_mut(|m| m.data.insert_temp(grab_id, grab));
    }
    if response.dragged() {
        let grab: Option<(bool, usize)> = ui.ctx().memory(|m| m.data.get_temp(grab_id)).flatten();
        if let (Some((is_bend, i)), Some(pos)) = (grab, response.interact_pointer_pos()) {
            if is_bend {
                // Dragging up raises the middle of the segment, whichever way it goes.
                let direction = (points[i].value - points[i - 1].value).signum();
                let bend = edited.points[i].bend - response.drag_delta().y / 40.0 * direction;
                edited.points[i].bend = bend.clamp(-1.0, 1.0);
            } else {
                edited.move_point(i, t_of_x(pos.x), v_of(pos.y));
            }
        }
    }
    if response.double_clicked() {
        // The strip with the tabs is not part of the drawing area.
        if let Some(pos) = response.interact_pointer_pos().filter(|p| p.y > top - 4.0) {
            match nearest(pos) {
                Some((false, i)) => edited.remove(i),
                _ if ms_of_x(pos.x) > shape_start => {
                    edited.insert(t_of_x(pos.x), v_of(pos.y));
                }
                _ => {}
            }
        }
    }
    response.context_menu(|ui| {
        if ui.button("Restablecer curva").clicked() {
            edited = engine::model::Shape::falling();
            ui.close();
        }
    });
    response.on_hover_text("Arrastra los puntos · doble clic añade o borra · arrastra el círculo de un tramo para curvarlo · clic derecho para restablecer");
    if edited != shape {
        let params = if amp_tab { KickParams { amp_shape: edited, ..base } } else { KickParams { pitch_shape: edited, ..base } };
        actions.push(Action::Instrument(InstrumentKind::Kick(params)));
    }

    let hz = 440.0 * 2f32.powf((shown.tune - 69.0) / 12.0);
    let note = if shown.key_follow { "Key".to_string() } else { format!("{} · {hz:.1} Hz", crate::project::note_name(shown.tune.round() as u8)) };
    let label = if amp_tab { format!("{length:.0} ms") } else { format!("{note} · {span_ms:.0} ms") };
    painter.text(rect.right_top() + vec2(-4.0, 3.0), Align2::RIGHT_TOP, label, theme::small(), TEXT_DIM);
}

/// Factory and user presets for the instrument on track `t`, and saving a new one.
fn preset_menu(app: &PulseApp, ui: &mut egui::Ui, t: usize, actions: &mut Vec<Action>) {
    let kind = app.project.tracks[t].instrument;
    ui.menu_button("Presets", |ui| {
        ui.set_min_width(190.0);
        let factory = crate::presets::factory(&kind);
        if !factory.is_empty() {
            ui.label(egui::RichText::new("DE FÁBRICA").color(TEXT_DIM).size(10.0));
            egui::ScrollArea::vertical().id_salt("factory_presets").max_height(260.0).show(ui, |ui| {
                for preset in factory {
                    if ui.button(&preset.name).clicked() {
                        actions.push(Action::ApplyPreset(preset));
                        ui.close();
                    }
                }
            });
            ui.separator();
        }
        ui.label(egui::RichText::new("MIS PRESETS").color(TEXT_DIM).size(10.0));
        let mine = crate::presets::list(&crate::presets::user_root(), &kind);
        if mine.is_empty() {
            ui.label(egui::RichText::new("Todavía no hay ninguno").color(TEXT_DIM).size(10.5));
        }
        for preset in mine {
            ui.horizontal(|ui| {
                if ui.button(&preset.name).clicked() {
                    actions.push(Action::ApplyPreset(preset.clone()));
                    ui.close();
                }
                if ui.small_button("✕").on_hover_text("Borrar el preset").clicked() {
                    actions.push(Action::DeletePreset(preset.clone()));
                }
            });
        }
        ui.separator();
        let name_id = ui.id().with(("preset_name", t));
        let mut name: String = ui.ctx().memory(|m| m.data.get_temp(name_id)).unwrap_or_default();
        ui.horizontal(|ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Nombre").desired_width(110.0));
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Guardar").clicked() || enter) && !name.trim().is_empty() {
                actions.push(Action::SavePreset(name.trim().to_string()));
                name.clear();
                ui.close();
            }
        });
        ui.ctx().memory_mut(|m| m.data.insert_temp(name_id, name));
    });
}

/// The kick's click source: synthesized, or a sample dropped or loaded here.
fn click_slot(app: &PulseApp, ui: &mut egui::Ui, t: usize, actions: &mut Vec<Action>, zones: &mut Zones) {
    let track = &app.project.tracks[t];
    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(vec2(170.0, 18.0), Sense::click());
        zones.push((rect, t, 0));
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, theme::BG);
        if files_hovering(ui) {
            painter.rect_stroke(rect, 2.0, Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
        }
        let text = match track.sample_name(0) {
            Some(name) => format!("Click: {name}"),
            None => "Click: sintetizado".into(),
        };
        painter.text(rect.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, text, theme::small(), theme::TEXT);
        if response.on_hover_text("Clic o arrastra una muestra para usarla como click").clicked() {
            actions.push(Action::LoadSamples(0));
        }
        if track.sample(0).is_some() && ui.small_button("✕").on_hover_text("Volver al click sintetizado").clicked() {
            actions.push(Action::ClearSample(0));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, PointerButton, Pos2, RawInput};

    /// Runs the kick editor headless for one frame per event batch and returns
    /// the shape it last asked for, and where the editor was drawn.
    fn run(frames: &[Vec<Event>], params: KickParams) -> (Option<KickParams>, Rect) {
        let ctx = egui::Context::default();
        let mut result = None;
        let mut rect = Rect::NOTHING;
        let mut current = params;
        for (n, events) in frames.iter().enumerate() {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 200.0))),
                events: events.clone(),
                time: Some(n as f64 * 0.05),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let mut actions = Vec::new();
                rect = ui.cursor();
                rect = Rect::from_min_size(rect.min, vec2(260.0, 84.0));
                kick_editor(ui, 0, None, &current, current, &mut actions);
                for action in actions {
                    if let Action::Instrument(InstrumentKind::Kick(p)) = action {
                        current = p;
                        result = Some(p);
                    }
                }
            });
        }
        (result, rect)
    }

    fn press(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() }
    }

    /// Where shape time/value land in the editor (pitch tab: time spans the pitch sweep).
    fn at(rect: Rect, time: f32, value: f32) -> Pos2 {
        let (top, bottom) = (rect.top() + 16.0, rect.bottom() - 4.0);
        pos2(rect.left() + time * rect.width(), bottom - value * (bottom - top))
    }

    #[test]
    fn double_click_adds_a_point_and_dragging_moves_it() {
        let params = KickParams::default();
        let (_, rect) = run(&[vec![]], params);
        let target = at(rect, 0.5, 0.8);
        let click = vec![Event::PointerMoved(target), press(target, true), press(target, false)];
        let (added, _) = run(&[vec![Event::PointerMoved(target)], click.clone(), click], params);
        let added = added.expect("a double click adds a point");
        assert_eq!(added.pitch_shape.count, 3);
        let point = added.pitch_shape.points[1];
        assert!((point.time - 0.5).abs() < 0.02 && (point.value - 0.8).abs() < 0.03, "{point:?}");

        // Drag that point down and to the right.
        let to = at(rect, 0.7, 0.3);
        let mut frames = vec![vec![Event::PointerMoved(target)], vec![press(target, true)]];
        for step in 1..=6 {
            frames.push(vec![Event::PointerMoved(target + (to - target) * step as f32 / 6.0)]);
        }
        frames.push(vec![press(to, false)]);
        let (moved, _) = run(&frames, added);
        let point = moved.expect("dragging edits the shape").pitch_shape.points[1];
        assert!((point.time - 0.7).abs() < 0.03 && (point.value - 0.3).abs() < 0.05, "{point:?}");
    }

    #[test]
    fn double_clicking_a_point_removes_it() {
        let params = KickParams { pitch_shape: engine::model::Shape::from_points(&[(0.0, 1.0, 0.0), (0.4, 0.5, 0.0), (1.0, 0.0, 0.0)]), ..Default::default() };
        let (_, rect) = run(&[vec![]], params);
        let target = at(rect, 0.4, 0.5);
        let click = vec![Event::PointerMoved(target), press(target, true), press(target, false)];
        let (removed, _) = run(&[vec![Event::PointerMoved(target)], click.clone(), click], params);
        assert_eq!(removed.expect("removed").pitch_shape.count, 2);
    }

    #[test]
    fn dragging_a_segment_handle_up_bends_it_up() {
        let params = KickParams::default();
        let (_, rect) = run(&[vec![]], params);
        // The handle of the straight 1 → 0 line sits in its middle.
        let handle = at(rect, 0.5, 0.5);
        let mut frames = vec![vec![Event::PointerMoved(handle)], vec![press(handle, true)]];
        for step in 1..=5 {
            frames.push(vec![Event::PointerMoved(handle - vec2(0.0, 4.0 * step as f32))]);
        }
        frames.push(vec![press(handle - vec2(0.0, 20.0), false)]);
        let (bent, _) = run(&frames, params);
        let shape = bent.expect("bent").pitch_shape;
        assert_eq!(shape.count, 2, "no point was added");
        assert!(shape.value_at(0.5) > 0.6, "the middle rose: {}", shape.value_at(0.5));
    }
}
