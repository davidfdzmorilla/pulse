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

pub(crate) enum Action {
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
    OpenKickWindow,
    OpenWaveWindow,
    OpenPolyWindow,
    OpenModularWindow,
    /// Opens a file dialog for modular generator `n`.
    LoadGenFile(usize),
    /// Connects modular modulator `.0` to parameter `.1`.
    ModularTarget(usize, usize),
    StackEdit(crate::modular_ui::StackEdit),
    /// Audio-rate connection: source module, target module, what it modulates.
    AudioLink(usize, usize, engine::model::AudioParam),
    /// New settings for the track's wavetable-engine synth (either kind).
    Wave(engine::model::WaveParams),
    /// Opens a file dialog for the sample of oscillator `n`.
    LoadOscSample(usize),
    /// Routes a modulation source to a wavetable synth parameter.
    AddModulation(engine::model::ModSource, usize),
    /// Opens the wavetable editor for oscillator `n`.
    OpenTableEditor(usize),
    /// Opens a file dialog for the wavetable of oscillator `n`.
    LoadWavetable(usize),
    ExportKick,
    /// Opens a plugin's editor or its parameter window.
    OpenPlugin(crate::plugins::PluginWindow),
}

/// Plugin parameters shown as knobs on its device; the rest are in its window.
const PLUGIN_KNOBS: usize = 6;

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
                let plugin_title = match device.effect {
                    Effect::Plugin(id) => Some(app.plugin_name(id)),
                    _ => None,
                };
                let title = plugin_title.as_deref().unwrap_or(device.effect.label());
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
                        if let Effect::Plugin(id) = *effect {
                            if let Some(id) = crate::plugins::device_body(app, ui, id, PLUGIN_KNOBS) {
                                actions.push(Action::OpenPlugin(id));
                            }
                            return;
                        }
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
    apply(app, target, actions);
}

/// Carries out what the device views asked for.
pub(crate) fn apply(app: &mut PulseApp, target: Target, actions: Vec<Action>) {
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
            Action::OpenKickWindow => {
                if let Target::Track(t) = target {
                    app.kick_window = Some(t);
                }
            }
            Action::OpenModularWindow => {
                if let Target::Track(t) = target {
                    app.modular_window = Some(t);
                }
            }
            Action::LoadGenFile(g) => {
                if let Target::Track(t) = target {
                    app.choose_gen_file(t, g);
                }
            }
            Action::StackEdit(edit) => {
                if let Target::Track(t) = target {
                    app.edit_stack(t, edit);
                }
            }
            Action::AudioLink(source, module, param) => {
                if let Target::Track(t) = target {
                    app.connect_audio(t, source, module, param);
                }
            }
            Action::ModularTarget(modulator, dest) => {
                if let Target::Track(t) = target {
                    app.add_modular_target(t, modulator, dest);
                }
            }
            Action::OpenPolyWindow => {
                if let Target::Track(t) = target {
                    app.poly_window = Some(t);
                }
            }
            Action::Wave(params) => {
                if let Target::Track(t) = target {
                    let kind = app.project.tracks[t].instrument.with_wave(params);
                    app.set_instrument(t, kind);
                }
            }
            Action::OpenWaveWindow => {
                if let Target::Track(t) = target {
                    app.wave_window = Some(t);
                }
            }
            Action::LoadOscSample(osc) => {
                if let Target::Track(t) = target {
                    app.choose_osc_sample(t, osc);
                }
            }
            Action::AddModulation(source, dest) => {
                if let Target::Track(t) = target {
                    app.add_modulation(t, source, dest);
                }
            }
            Action::OpenTableEditor(osc) => {
                if let Target::Track(t) = target {
                    crate::table_editor::open(app, t, osc);
                }
            }
            Action::LoadWavetable(osc) => {
                if let Target::Track(t) = target {
                    app.choose_wavetable(t, osc);
                }
            }
            Action::OpenPlugin(crate::plugins::PluginWindow::Params(id)) => app.plugin_library.window = Some(id),
            Action::OpenPlugin(crate::plugins::PluginWindow::Editor(id)) => app.open_editor(id),
            Action::ExportKick => {
                if let Target::Track(t) = target {
                    crate::kick_ui::export(app, t);
                }
            }
        }
    }
}

pub(crate) fn effect_width(effect: &Effect) -> f32 {
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
        Effect::Plugin(_) => (PLUGIN_KNOBS, 1),
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
pub(crate) fn discrete_controls(ui: &mut egui::Ui, effect: &mut Effect, index: usize) {
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
        | Effect::Utility(_)
        | Effect::Plugin(_) => {}
    }
}

pub(crate) type Zones = Vec<(Rect, usize, usize)>;

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
        InstrumentKind::Plugin(id) => {
            let title = app.plugin_name(id);
            let width = PLUGIN_KNOBS as f32 * 52.0 + 130.0;
            device_frame(ui, &title, width, None, |_| {}, |ui| {
                if let Some(id) = crate::plugins::device_body(app, ui, id, PLUGIN_KNOBS) {
                    actions.push(Action::OpenPlugin(id));
                }
            });
        }
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
        InstrumentKind::Modular(base) => {
            let header = |ui: &mut egui::Ui| {
                preset_menu(app, ui, t, &mut head);
                if ui.small_button("Editor ⤢").on_hover_text("Abre el editor: generadores, modificadores, carriles y moduladores").clicked() {
                    head.push(Action::OpenModularWindow);
                }
            };
            let mut body_actions = Vec::new();
            device_frame(ui, kind.label(), 520.0, None, header, |ui| crate::modular_ui::compact(app, ui, t, base, &mut body_actions));
            actions.extend(body_actions);
        }
        InstrumentKind::Wavetable(base) | InstrumentKind::Polychrome(base) => {
            let poly = matches!(kind, InstrumentKind::Polychrome(_));
            let header = |ui: &mut egui::Ui| {
                preset_menu(app, ui, t, &mut head);
                if ui.small_button("Editor ⤢").on_hover_text("Abre el editor completo").clicked() {
                    head.push(if poly { Action::OpenPolyWindow } else { Action::OpenWaveWindow });
                }
            };
            let mut body_actions = Vec::new();
            device_frame(ui, kind.label(), 560.0, None, header, |ui| crate::wave_ui::compact(app, ui, t, base, &mut body_actions));
            actions.extend(body_actions);
        }
        InstrumentKind::Kick(base) => {
            let header = |ui: &mut egui::Ui| {
                preset_menu(app, ui, t, &mut head);
                if ui.small_button("Editor ⤢").on_hover_text("Abre el editor completo: capas, envolventes, EQ, compresión y espectro").clicked() {
                    head.push(Action::OpenKickWindow);
                }
            };
            let mut body_actions = Vec::new();
            device_frame(ui, kind.label(), 640.0, None, header, |ui| {
                let shown = crate::kick_ui::shown(app, t, base);
                ui.vertical(|ui| {
                    let click = app.project.tracks[t].sample(0).and_then(|path| app.loaded_sample(path)).cloned();
                    let id = ui.id().with(("kick_compact", t));
                    crate::kick_ui::editor(ui, id, vec2(260.0, 84.0), click, &shown, base, app.project.tempo, false, &mut body_actions);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let mut mode = base.drive_mode;
                        egui::ComboBox::from_id_salt(("kick_drive_mode", t)).selected_text(mode.label()).width(56.0).show_ui(ui, |ui| {
                            for m in KickDrive::ALL {
                                ui.selectable_value(&mut mode, m, m.label());
                            }
                        });
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
                    for row in [[0, 1, 2, 15, 9, 12, 13], [4, 5, 6, 7, 10, 22, 18]] {
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
pub(crate) fn files_hovering(ui: &egui::Ui) -> bool {
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

/// Factory and user presets for the instrument on track `t`, and saving a new one.
pub(crate) fn preset_menu(app: &PulseApp, ui: &mut egui::Ui, t: usize, actions: &mut Vec<Action>) {
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
pub(crate) fn click_slot(app: &PulseApp, ui: &mut egui::Ui, t: usize, actions: &mut Vec<Action>, zones: &mut Zones) {
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
