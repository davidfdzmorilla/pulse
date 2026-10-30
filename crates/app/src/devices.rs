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
    /// Moves a multisample's zones by these semitones.
    ShiftZones(i32),
    /// The sampler back to no samples (multisample or not).
    ClearSampler,
    SelectPad(usize),
    BasicKit,
    /// A kit of «Sonidos de Pulse», by its place in `factory_sounds::kits`.
    FactoryKit(usize),
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
pub(crate) fn device_frame<R>(
    ui: &mut egui::Ui,
    title: &str,
    width: f32,
    enabled: Option<&mut bool>,
    header_extra: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    // At least as wide as its controls were last frame, so none is cut off.
    let id = ui.id().with(("device_width", title, ui.next_auto_id()));
    let needed: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
    let width = width.max(needed);
    let (rect, _) = ui.allocate_exact_size(vec2(width, DEVICE_HEIGHT), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, SURFACE);
    let title_rect = Rect::from_min_size(rect.min, vec2(width, 20.0));
    painter.rect_filled(title_rect, 3.0, CONTROL);

    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(title_rect.shrink2(vec2(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.spacing_mut().item_spacing.x = 4.0;
    if let Some(enabled) = enabled {
        let (dot, response) = child.allocate_exact_size(vec2(12.0, 12.0), Sense::click());
        let on = *enabled;
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, "Activar"));
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
    let result = body(&mut body_ui);
    let used = (body_ui.min_rect().right() - rect.left() + 8.0).ceil();
    if (used - needed).abs() > 0.5 && used > width - 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(id, used));
        ui.ctx().request_repaint();
    }
    result
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let target = app.selected_target;
    let mut actions = Vec::new();
    let mut zones = Vec::new();

    egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Target::Track(t) = target {
                if !app.project.tracks[t].is_audio() && !app.project.tracks[t].is_group() {
                    crate::midi_fx_ui::show(app, ui, t);
                    instrument_device(app, ui, t, &mut actions, &mut zones);
                }
            }

            // The tracks a compressor or ducker can listen to: the others.
            let sources: Vec<(u32, String)> = app
                .project
                .tracks
                .iter()
                .enumerate()
                .filter(|&(t, track)| target != Target::Track(t) && track.id != 0)
                .map(|(_, track)| (track.id, track.name.clone()))
                .collect();
            let count = app.devices(target).len();
            // The first EQ Eight shows the spectrum of what comes out of it.
            let analyzed = app.devices(target).iter().position(|d| matches!(d.effect, Effect::EqEight(_)));
            if let Some(state) = app.audio.state() {
                state.analyzer.watch(analyzed.map(|d| (target, d)));
                if analyzed.is_some() {
                    app.spectrum.update(&state.analyzer, app.audio.sample_rate(), ui.input(|i| i.time));
                    if app.spectrum.sounding() {
                        ui.ctx().request_repaint();
                    }
                }
            }
            for index in 0..count {
                let before = app.devices(target)[index];
                let mut device = before;
                let plugin_title = match device.effect {
                    Effect::Plugin(id) => Some(app.plugin_name(id)),
                    _ => None,
                };
                let title = plugin_title.as_deref().unwrap_or(device.effect.label());
                let width = effect_width(&device.effect);
                let presets = device.effect.factory_presets();
                // What its meter shows (a compressor's gain reduction), if it has one.
                let meter = meters(&before.effect).then(|| app.audio.state().map(|s| s.analyzer.meter(target, index)).unwrap_or_default());
                if meter.is_some_and(|m| m.iter().any(|v| v.abs() > 0.05)) {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
                }
                let Device { effect, enabled } = &mut device;
                let mut header_actions = Vec::new();
                device_frame(
                    ui,
                    title,
                    width,
                    Some(enabled),
                    |ui| {
                        if ui.small_button("×").on_hover_text("Eliminar").clicked() {
                            header_actions.push(Action::Remove(index));
                        }
                        if index + 1 < count && ui.small_button("►").on_hover_text("Mover a la derecha").clicked() {
                            header_actions.push(Action::Move(index, index + 1));
                        }
                        if index > 0 && ui.small_button("◄").on_hover_text("Mover a la izquierda").clicked() {
                            header_actions.push(Action::Move(index, index - 1));
                        }
                        if !presets.is_empty() {
                            effect_presets(ui, index, before, presets, &mut header_actions);
                        }
                    },
                    |ui| {
                        if let Effect::Plugin(id) = *effect {
                            let as_param = |param: u32| Param::Device { device: index, param: param as usize };
                            crate::plugins::device_body(app, ui, target, &as_param, id, PLUGIN_KNOBS, &mut actions);
                            return;
                        }
                        if let Effect::EqEight(p) = effect {
                            let mut shown = Effect::EqEight(*p);
                            for param in 0..shown.params().len() {
                                shown.set(param, app.display_value(target, Param::Device { device: index, param }));
                            }
                            let Effect::EqEight(shown) = shown else { unreachable!() };
                            let automation = |param| app.automation_state(target, Param::Device { device: index, param });
                            let spectrum = (analyzed == Some(index)).then_some(&app.spectrum);
                            crate::eq_ui::body(ui, target, index, p, &shown, &automation, spectrum, app.audio.sample_rate(), &mut actions);
                            return;
                        }
                        if let Effect::Multiband(_) = effect {
                            multiband_body(app, ui, target, index, meter, &mut actions);
                            return;
                        }
                        discrete_controls(ui, effect, index, &sources);
                        for param in 0..effect.params().len() {
                            effect_knob(app, ui, target, index, effect, param, &mut actions);
                        }
                        if let Some(meter) = meter {
                            reduction_meter(ui, meter[0]);
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
            Action::ShiftZones(semitones) => {
                if let Target::Track(t) = target {
                    app.shift_zones(t, semitones);
                }
            }
            Action::ClearSampler => {
                if let Target::Track(t) = target {
                    app.clear_sampler(t);
                }
            }
            Action::SelectPad(pad) => app.selected_pad = pad,
            Action::BasicKit => {
                if let Target::Track(t) = target {
                    app.load_basic_kit(t);
                }
            }
            Action::FactoryKit(kit) => {
                if let Target::Track(t) = target {
                    app.load_factory_kit(t, kit);
                }
            }
            Action::SliceFile => {
                if let Target::Track(t) = target {
                    app.slice_file_into(t);
                }
            }
            Action::OpenKickWindow => {
                if let Target::Track(t) = target {
                    app.open_instrument_editor(crate::app::InstrumentEditor::Kick, t);
                }
            }
            Action::OpenModularWindow => {
                if let Target::Track(t) = target {
                    app.open_instrument_editor(crate::app::InstrumentEditor::Modular, t);
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
                    app.open_instrument_editor(crate::app::InstrumentEditor::Poly, t);
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
                    app.open_instrument_editor(crate::app::InstrumentEditor::Wave, t);
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
        Effect::EqEight(_) => return crate::eq_ui::WIDTH,
        Effect::Compressor(_) => (5, 1),
        Effect::Gate(_) => (6, 1),
        Effect::Transient(_) => (3, 0),
        Effect::Tuner(_) => (3, 1),
        Effect::DeEsser(_) => (3, 0),
        Effect::Delay(_) => (3, 1),
        Effect::Reverb(_) => (4, 0),
        // Space and presets, the drawing, then Decay and Size and the five knobs.
        Effect::Convolution(_) => return 7.0 * 52.0 + 100.0 + SKETCH.x + 32.0,
        Effect::Saturator(_) => (3, 0),
        Effect::AutoFilter(_) => (3, 2),
        Effect::Chorus(_) => (4, 0),
        Effect::Phaser(_) => (5, 0),
        Effect::Bitcrusher(_) => (3, 0),
        Effect::Ducker(_) => (3, 1),
        Effect::Limiter(_) => (3, 0),
        Effect::Utility(_) => (3, 1),
        Effect::Glue(_) => (7, 1),
        // Two rows: the widest has eight knobs; then the bands' meters.
        Effect::Multiband(_) => return 8.0 * 52.0 + BAND_METERS_WIDTH + 28.0,
        Effect::Plugin(_) => (PLUGIN_KNOBS, 1),
    };
    let meter = if meters(effect) { METER_WIDTH + 4.0 } else { 0.0 };
    knobs as f32 * 52.0 + columns as f32 * 100.0 + 24.0 + meter
}

/// Whether its device shows a meter (a compressor's or limiter's gain
/// reduction, a multiband's bands).
fn meters(effect: &Effect) -> bool {
    matches!(effect, Effect::Compressor(_) | Effect::Limiter(_) | Effect::Glue(_) | Effect::Multiband(_))
}

const METER_WIDTH: f32 = 30.0;
const BAND_METERS_WIDTH: f32 = 66.0;
/// The most a meter shows, dB.
const METER_RANGE_DB: f32 = 24.0;

/// A gain reduction meter: a bar that grows down from the top, and its value.
fn reduction_meter(ui: &mut egui::Ui, reduction_db: f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(METER_WIDTH, DEVICE_HEIGHT - 32.0), Sense::hover());
    response.on_hover_text("Reducción de ganancia: cuánto baja el volumen ahora mismo");
    let painter = ui.painter();
    painter.text(pos2(rect.center().x, rect.top()), Align2::CENTER_TOP, "GR", theme::small(), TEXT_DIM);
    let bar = Rect::from_min_max(pos2(rect.center().x - 5.0, rect.top() + 14.0), pos2(rect.center().x + 5.0, rect.bottom() - 16.0));
    painter.rect_filled(bar, 2.0, theme::BG);
    let fraction = (reduction_db / METER_RANGE_DB).clamp(0.0, 1.0);
    if fraction > 0.0 {
        painter.rect_filled(Rect::from_min_max(bar.min, pos2(bar.max.x, bar.top() + bar.height() * fraction)), 2.0, ACCENT);
    }
    for db in [6.0, 12.0, 18.0] {
        let y = bar.top() + bar.height() * db / METER_RANGE_DB;
        painter.line_segment([pos2(bar.left() - 3.0, y), pos2(bar.left(), y)], Stroke::new(1.0, TEXT_DIM));
    }
    let value = if reduction_db >= 0.05 { format!("-{reduction_db:.1}") } else { "0".to_string() };
    painter.text(pos2(rect.center().x, rect.bottom()), Align2::CENTER_BOTTOM, value, theme::small(), theme::TEXT);
}

/// A multiband's three bands: each a bar from the middle, up when it pulls
/// the band up, down when it pushes it down.
fn band_meters(ui: &mut egui::Ui, bands: [f32; 3]) {
    let (rect, response) = ui.allocate_exact_size(vec2(BAND_METERS_WIDTH, DEVICE_HEIGHT - 32.0), Sense::hover());
    response.on_hover_text("Lo que hace cada banda ahora mismo: hacia arriba, sube lo flojo; hacia abajo, baja lo fuerte");
    let painter = ui.painter();
    let column = rect.width() / 3.0;
    let (top, bottom) = (rect.top() + 14.0, rect.bottom() - 16.0);
    let middle = (top + bottom) / 2.0;
    for (band, (name, db)) in ["L", "M", "H"].into_iter().zip(bands).enumerate() {
        let x = rect.left() + column * (band as f32 + 0.5);
        painter.text(pos2(x, rect.top()), Align2::CENTER_TOP, name, theme::small(), TEXT_DIM);
        let bar = Rect::from_min_max(pos2(x - 5.0, top), pos2(x + 5.0, bottom));
        painter.rect_filled(bar, 2.0, theme::BG);
        let reach = (db / METER_RANGE_DB).clamp(-1.0, 1.0) * (bottom - top) / 2.0;
        if reach.abs() > 0.5 {
            let color = if db > 0.0 { theme::METER_GREEN } else { ACCENT };
            painter.rect_filled(Rect::from_two_pos(pos2(bar.left(), middle), pos2(bar.right(), middle - reach)), 1.0, color);
        }
        painter.line_segment([pos2(bar.left() - 2.0, middle), pos2(bar.right() + 2.0, middle)], Stroke::new(1.0, TEXT_DIM));
        painter.text(pos2(x, rect.bottom()), Align2::CENTER_BOTTOM, format!("{db:+.0}"), theme::small(), theme::TEXT);
    }
}

/// The knob of continuous parameter `param` of device `index`, showing and
/// overriding automation like every other.
fn effect_knob(app: &PulseApp, ui: &mut egui::Ui, target: Target, index: usize, effect: &Effect, param: usize, actions: &mut Vec<Action>) {
    let Some(info) = effect.params().get(param) else { return };
    let id = Param::Device { device: index, param };
    let value = app.display_value(target, id);
    if let Some(v) = widgets::param_knob(ui, info, value, info.name, 30.0, app.automation_state(target, id)) {
        actions.push(Action::Param(id, v));
    }
}

/// The multiband's two rows: what it does (depth, time, amounts, gains) and
/// its bands (each one's gains, the crossovers between them).
fn multiband_body(app: &PulseApp, ui: &mut egui::Ui, target: Target, index: usize, meter: Option<[f32; 3]>, actions: &mut Vec<Action>) {
    let effect = app.devices(target)[index].effect;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.horizontal(|ui| {
            for param in 0..7 {
                effect_knob(app, ui, target, index, &effect, param, actions);
            }
        });
        ui.horizontal(|ui| {
            for param in [9, 10, 7, 11, 12, 8, 13, 14] {
                effect_knob(app, ui, target, index, &effect, param, actions);
            }
        });
    });
    band_meters(ui, meter.unwrap_or_default());
}

/// The factory settings of an effect that has them (keeping its sidechain).
fn effect_presets(ui: &mut egui::Ui, index: usize, device: Device, presets: Vec<(&'static str, Effect)>, actions: &mut Vec<Action>) {
    widgets::field_menu_button(ui, "Presets", |ui| {
        ui.set_min_width(170.0);
        let mut group = "";
        for (name, mut effect) in presets {
            let (category, short) = name.split_once(" · ").unwrap_or(("", name));
            if category != group {
                group = category;
                ui.label(egui::RichText::new(category.to_uppercase()).color(TEXT_DIM).size(10.0));
            }
            if ui.button(short).clicked() {
                if let (Some(slot), Some(source)) = (effect.sidechain_mut(), device.effect.sidechain()) {
                    *slot = Some(source);
                }
                actions.push(Action::Replace(index, Device { effect, ..device }));
                ui.close();
            }
        }
    });
}

fn beats_combo(ui: &mut egui::Ui, id: &str, value: &mut f64, options: &[f64]) -> egui::Response {
    egui::ComboBox::from_id_salt(id)
        .selected_text(beats_label(*value))
        .width(84.0)
        .show_ui(ui, |ui| {
            for &option in options {
                ui.selectable_value(value, option, beats_label(option));
            }
        })
        .response
}

/// What a compressor's or ducker's sidechain does.
const DUCKS: &str = "Con el bombo, el bajo o el pad se apartan cuando suena";

/// A compressor's, gate's or ducker's sidechain: none, or the track it listens to.
fn sidechain_combo(ui: &mut egui::Ui, id: &str, value: &mut Option<u32>, sources: &[(u32, String)], hint: &str) {
    ui.label(egui::RichText::new("Sidechain").color(TEXT_DIM).size(10.5));
    let current = value.and_then(|v| sources.iter().find(|(s, _)| *s == v)).map_or("Ninguno", |(_, name)| name.as_str());
    egui::ComboBox::from_id_salt(id)
        .selected_text(current)
        .width(84.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "Ninguno");
            for (track, name) in sources {
                ui.selectable_value(value, Some(*track), name);
            }
        })
        .response
        .on_hover_text(format!("La pista que lo dispara: escucha su sonido (tras sus efectos, aunque esté silenciada) en vez del propio. {hint}"));
}

/// Settings that are choices rather than knobs: time divisions, modes, switches.
/// `sources` are the tracks a sidechain can listen to (id and name).
pub(crate) fn discrete_controls(ui: &mut egui::Ui, effect: &mut Effect, index: usize, sources: &[(u32, String)]) {
    match effect {
        Effect::Compressor(p) if !sources.is_empty() => {
            ui.vertical(|ui| sidechain_combo(ui, &format!("comp_sc{index}"), &mut p.sidechain, sources, DUCKS));
        }
        Effect::Gate(p) if !sources.is_empty() => {
            ui.vertical(|ui| sidechain_combo(ui, &format!("gate_sc{index}"), &mut p.sidechain, sources, "Se abre solo cuando suena esa pista: un pad que corta al ritmo de los charles, o un bajo que suena solo con el bombo"));
        }
        Effect::Utility(p) => {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Fase").color(TEXT_DIM).size(10.5));
                ui.checkbox(&mut p.flip_right, "Invertir R").on_hover_text("Da la vuelta al canal derecho: arregla una pista grabada con un canal al revés, que en mono casi desaparece");
            });
        }
        Effect::Tuner(p) => {
            use engine::model::TunerScale;
            let name = |scale: TunerScale| match scale {
                TunerScale::Project => "Del proyecto".to_string(),
                TunerScale::Chromatic => "Cromática".to_string(),
                TunerScale::Key { tonic, minor } => stems::key::Key { tonic, minor }.name(),
            };
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Escala").color(TEXT_DIM).size(10.5));
                egui::ComboBox::from_id_salt(format!("tuner_scale{index}")).selected_text(name(p.scale)).width(84.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut p.scale, TunerScale::Project, name(TunerScale::Project)).on_hover_text("La escala del proyecto (Tonalidad y escala); sin escala, todas las notas");
                    ui.selectable_value(&mut p.scale, TunerScale::Chromatic, name(TunerScale::Chromatic)).on_hover_text("Todas las notas: la más cercana");
                    ui.separator();
                    for key in stems::key::Key::all() {
                        let scale = TunerScale::Key { tonic: key.tonic, minor: key.minor };
                        ui.selectable_value(&mut p.scale, scale, name(scale));
                    }
                });
            });
        }
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
                ui.checkbox(&mut p.steep, "24 dB").on_hover_text("Corta el doble por octava (24 dB en vez de 12): más grave y oscuro. Res por encima del 95 % hace que el filtro suene solo");
            });
        }
        Effect::Saturator(p) => {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Tipo").color(TEXT_DIM).size(10.5));
                egui::ComboBox::from_id_salt(format!("sat_kind{index}")).selected_text(p.kind.label()).width(64.0).show_ui(ui, |ui| {
                    for kind in KickDrive::ALL {
                        ui.selectable_value(&mut p.kind, kind, kind.label());
                    }
                });
                ui.checkbox(&mut p.oversample, "4×").on_hover_text("Trabaja a cuatro veces la frecuencia de muestreo: sin el aliasing (agudos sucios) de mucho drive. Gasta algo más de CPU (menos de un 1 % de un núcleo) y retrasa 36 muestras");
            });
        }
        Effect::Ducker(p) => {
            ui.vertical(|ui| {
                // With a sidechain the period is the time it takes to come back, and
                // the «Release» knob the part of it spent coming back: two names.
                let (name, hint) = if p.sidechain.is_some() {
                    ("Vuelve en", "Tras cada golpe del sidechain, el tiempo en que el sonido vuelve a su volumen. El knob «Release» dice qué parte de este tiempo tarda en subir (100 %: todo)")
                } else {
                    ("Ritmo", "Cada cuánto baja el volumen, a tempo. El knob «Release» dice qué parte de este tiempo tarda en volver (100 %: todo)")
                };
                ui.label(egui::RichText::new(name).color(TEXT_DIM).size(10.5)).on_hover_text(hint);
                beats_combo(ui, &format!("ducker{index}"), &mut p.rate_beats, &[0.25, 0.5, 1.0, 2.0, 4.0]).on_hover_text(hint);
                if !sources.is_empty() {
                    sidechain_combo(ui, &format!("duck_sc{index}"), &mut p.sidechain, sources, DUCKS);
                }
                if p.sidechain.is_some() {
                    ui.checkbox(&mut p.lows, "Solo graves").on_hover_text("Escucha solo por debajo de 600 Hz: bombea con el bombo y no con los charles o las palmas de la misma pista. Quítalo con un bombo de hardstyle");
                }
            });
        }
        Effect::Glue(p) => {
            use engine::model::GlueRatio;
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Ratio").color(TEXT_DIM).size(10.5));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for ratio in GlueRatio::ALL {
                        let hint = match ratio {
                            GlueRatio::Two => "Suave: pega sin que se note. Para el master y los grupos",
                            GlueRatio::Four => "El de siempre en el bus de batería",
                            GlueRatio::Ten => "Casi un limitador: para aplastar en paralelo",
                        };
                        ui.selectable_value(&mut p.ratio, ratio, ratio.label()).on_hover_text(hint);
                    }
                });
                ui.checkbox(&mut p.auto_release, "Auto").on_hover_text("Release automático: rápido tras un golpe suelto, lento cuando comprime mucho rato. Con él, el knob «Release» no hace nada");
                ui.checkbox(&mut p.soft_clip, "Clip").on_hover_text("Recorta suave los picos a la salida, por debajo de 0 dBFS: más volumen sin que pase de rojo");
                if !sources.is_empty() {
                    sidechain_combo(ui, &format!("glue_sc{index}"), &mut p.sidechain, sources, DUCKS);
                }
            });
        }
        Effect::Convolution(p) => convolution_controls(ui, p, index),
        Effect::Eq3(_)
        | Effect::EqEight(_)
        | Effect::Multiband(_)
        | Effect::Compressor(_)
        | Effect::Gate(_)
        | Effect::Transient(_)
        | Effect::DeEsser(_)
        | Effect::Reverb(_)
        | Effect::Chorus(_)
        | Effect::Phaser(_)
        | Effect::Bitcrusher(_)
        | Effect::Limiter(_)
        | Effect::Plugin(_) => {}
    }
}

/// The Convolution Reverb's drawing.
const SKETCH: egui::Vec2 = egui::vec2(150.0, 112.0);

/// The Convolution Reverb's space, presets, drawing, and the two knobs that
/// make its response (not automated: each change builds a new one).
fn convolution_controls(ui: &mut egui::Ui, p: &mut engine::model::ConvolutionParams, index: usize) {
    use engine::model::{ConvolutionParams, ReverbSpace};
    ui.vertical(|ui| {
        ui.label(egui::RichText::new("Espacio").color(TEXT_DIM).size(10.5));
        egui::ComboBox::from_id_salt(format!("space{index}"))
            .selected_text(p.space.label())
            .width(92.0)
            .show_ui(ui, |ui| {
                for space in ReverbSpace::ALL {
                    ui.selectable_value(&mut p.space, space, space.label()).on_hover_text(space.hint());
                }
            })
            .response
            .on_hover_text(p.space.hint());
    });
    space_sketch(ui, p);
    let base = *p;
    let seconds = move |decay: f32| engine::convolution::space_sketch(&ConvolutionParams { decay, ..base }).t60[1];
    widgets::Knob::new(&mut p.decay, ConvolutionParams::DECAY, 1.0, "Decay")
        .taper(widgets::Taper::Log)
        .format(move |v| format!("{:.1} s", seconds(v)))
        .show(ui)
        .on_hover_text("Lo que dura la cola (lo que tarda en caer 60 dB en los medios). Al cambiarlo, Pulse calcula el espacio de nuevo y pasa a él sin cortes en menos de medio segundo; por eso no se automatiza");
    widgets::Knob::new(&mut p.size, ConvolutionParams::SIZE, 1.0, "Size")
        .format(|v| format!("{:.0} %", v * 100.0))
        .show(ui)
        .on_hover_text("El tamaño del espacio: más grande, las primeras reflexiones llegan más tarde y más separadas. No se automatiza");
}

/// The response at a glance: the early reflections as lines, and how the tail
/// dies in the lows, mids and highs (time across, 0 to -60 dB down).
fn space_sketch(ui: &mut egui::Ui, p: &engine::model::ConvolutionParams) {
    let (rect, response) = ui.allocate_exact_size(SKETCH, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, theme::BG);
    let sketch = engine::convolution::space_sketch(p);
    let pre = p.predelay_ms.clamp(0.0, 250.0) * 0.001;
    let span = (pre + sketch.length).max(0.1);
    let plot = rect.shrink2(vec2(4.0, 6.0));
    // Time on a square-root scale, so the first milliseconds show.
    let x = |t: f32| plot.left() + (t / span).clamp(0.0, 1.0).sqrt() * plot.width();
    let y = |db: f32| plot.top() + (-db / 60.0).clamp(0.0, 1.0) * plot.height();
    let line = Color32::from_rgba_unmultiplied(0xd8, 0xd8, 0xd8, 90);
    // The first ones: later they are too many to tell apart.
    for (t, db) in sketch.reflections.iter().take(24) {
        let at = x(pre + t);
        painter.line_segment([pos2(at, y(-60.0)), pos2(at, y(*db - 3.0))], Stroke::new(1.0, line));
    }
    let colors = [Color32::from_rgb(120, 150, 255), ACCENT, Color32::from_rgb(255, 210, 120)];
    for (band, color) in sketch.t60.iter().zip(colors) {
        // From the tail's peak until it is 60 dB down.
        let start = pre + sketch.tail_peak;
        let peak = if sketch.reflections.is_empty() { 0.0 } else { -6.0 };
        let end = (start + band * (60.0 + peak) / 60.0).min(span);
        let points: Vec<egui::Pos2> = (0..=48)
            .map(|i| {
                let t = start + (end - start) * i as f32 / 48.0;
                pos2(x(t), y(peak - 60.0 * (t - start) / band.max(0.01)))
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.5, color)));
    }
    if pre > 0.0 {
        painter.text(pos2(x(pre), plot.top()), Align2::CENTER_TOP, "▾", theme::small(), TEXT_DIM);
    }
    painter.text(rect.right_top() + vec2(-4.0, 3.0), Align2::RIGHT_TOP, format!("{:.1} s", span), theme::small(), TEXT_DIM);
    response.on_hover_text(format!(
        "Las primeras reflexiones (rayas) y cómo cae la cola: graves (azul) {:.1} s, medios {:.1} s, agudos (amarillo) {:.1} s",
        sketch.t60[0], sketch.t60[1], sketch.t60[2]
    ));
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
                let as_param = |param: u32| Param::Instrument(param as usize);
                crate::plugins::device_body(app, ui, target, &as_param, id, PLUGIN_KNOBS, actions);
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
                // Which machine it sounds like.
                if let InstrumentKind::DrumMachine(params) = kind {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Máquina").color(TEXT_DIM).size(10.5));
                        for model in engine::model::DrumModel::ALL {
                            let hint = match model {
                                engine::model::DrumModel::Tr808 => "Como la Roland TR-808: bombo largo y redondo, palma, cencerro (hip hop, trap, reggaetón)",
                                engine::model::DrumModel::Tr909 => "Como la Roland TR-909: bombo con pegada, charles brillantes (house, techno)",
                                engine::model::DrumModel::Classic => "Los sonidos primeros de Pulse",
                            };
                            if ui.selectable_label(params.model == model, model.label()).on_hover_text(hint).clicked() && params.model != model {
                                actions.push(Action::Instrument(InstrumentKind::DrumMachine(engine::model::DrumParams { model, ..params })));
                            }
                        }
                    });
                }
                egui::Grid::new("drum_machine").spacing([8.0, 2.0]).show(ui, |ui| {
                    for (voice, (pitch, _)) in engine::model::DRUM_VOICES.into_iter().enumerate() {
                        let name = crate::project::voice_name(voice);
                        ui.vertical(|ui| {
                            // A sound on a track of its own (see `drum_split`): marked, and where.
                            let elsewhere = app.sound_track(t, voice);
                            let label = if elsewhere.is_some() { format!("{name} →") } else { name.to_string() };
                            let pad = egui::Button::new(egui::RichText::new(label).size(10.5)).min_size(vec2(136.0, 16.0));
                            let hint = match elsewhere {
                                Some(track) => format!("{} · clic para escuchar · suena en «{track}», con sus efectos", crate::project::note_name(pitch)),
                                None => format!("{} · clic para escuchar", crate::project::note_name(pitch)),
                            };
                            if ui.add(pad).on_hover_text(hint).clicked() {
                                actions.push(Action::Preview(pitch));
                            }
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                for (k, label) in ["Tune", "Decay", "Level"].into_iter().enumerate() {
                                    knob(ui, actions, voice * 3 + k, label, 18.0);
                                }
                                knob(ui, actions, engine::model::DRUM_SOUNDS * 3 + voice, "Drive", 18.0);
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
                    let multi = base.zone_count > 0;
                    let rect = sample_slot(app, ui, t, 0, vec2(220.0, 78.0), zones);
                    if multi {
                        if let Some(pitch) = keymap(app, ui, t, &base, rect) {
                            actions.push(Action::Preview(pitch));
                        }
                    } else {
                        waveform(app, ui, t, 0, rect, app.display_value(target, Param::Instrument(2)));
                    }
                    ui.horizontal(|ui| {
                        if ui.small_button("Cargar…").on_hover_text("Una muestra, o varias para repartirlas por el teclado según la nota de su nombre (Piano C3, Piano F#3…). También puedes arrastrarlas aquí").clicked() {
                            actions.push(Action::LoadSamples(0));
                        }
                        if app.project.tracks[t].sample(0).is_some() && ui.small_button("×").on_hover_text(if multi { "Quitar las muestras" } else { "Quitar la muestra" }).clicked() {
                            actions.push(Action::ClearSampler);
                        }
                        if multi {
                            if ui.small_button("−12").on_hover_text("Mover el mapa una octava abajo (si los nombres van una octava desplazados)").clicked() {
                                actions.push(Action::ShiftZones(-12));
                            }
                            if ui.small_button("+12").on_hover_text("Mover el mapa una octava arriba").clicked() {
                                actions.push(Action::ShiftZones(12));
                            }
                            if let Some(params) = zone_menu(app, ui, t, &base) {
                                actions.push(Action::Instrument(InstrumentKind::Sampler(params)));
                            }
                        } else if ui.small_button("►").on_hover_text("Escuchar en la nota raíz").clicked() {
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
                            // With zones, each has its own root.
                            for i in row.into_iter().filter(|&i| i != 0 || base.zone_count == 0) {
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
                widgets::field_menu_button(ui, "Kits", |ui| {
                    ui.set_min_width(150.0);
                    if ui.button("Kit básico").on_hover_text("Los sonidos de la Drum Machine en los pads").clicked() {
                        head.push(Action::BasicKit);
                        ui.close();
                    }
                    ui.label(egui::RichText::new("SONIDOS DE PULSE").color(TEXT_DIM).size(10.0));
                    for (i, kit) in crate::factory_sounds::kits().iter().enumerate() {
                        if ui.button(kit.name).on_hover_text(kit.about).clicked() {
                            head.push(Action::FactoryKit(i));
                            ui.close();
                        }
                    }
                });
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
                        if track.sample(pad).is_some() && ui.small_button("×").on_hover_text("Vaciar el pad").clicked() {
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
                        if ui.small_button("►").on_hover_text("Escuchar").clicked() {
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
        InstrumentKind::Keys(base) => {
            use engine::model::{KeysModel, KeysParams, KEYS_DRAWBAR, KEYS_LESLIE_FAST};
            let width = if base.model == KeysModel::Organ { 560.0 } else { 470.0 };
            device_frame(ui, kind.label(), width, None, |ui| preset_menu(app, ui, t, &mut head), |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(egui::RichText::new("Modelo").color(TEXT_DIM).size(10.5));
                    let mut model = base.model;
                    for m in KeysModel::ALL {
                        ui.selectable_value(&mut model, m, m.label()).on_hover_text(m.hint());
                    }
                    if model != base.model {
                        // Each model starts from its own sound; the volume stays.
                        actions.push(Action::Instrument(InstrumentKind::Keys(KeysParams { gain: base.gain, ..KeysParams::model_default(model) })));
                    }
                    if ui.small_button("►").on_hover_text("Escuchar").clicked() {
                        actions.push(Action::Preview(60));
                    }
                });
                if base.model == KeysModel::Organ {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        for i in 0..9 {
                            let id = Param::Instrument(KEYS_DRAWBAR + i);
                            if let Some(v) = crate::keys_ui::drawbar(ui, i, app.display_value(target, id)) {
                                actions.push(Action::Param(id, v));
                            }
                        }
                    });
                }
                ui.vertical(|ui| {
                    for row in crate::keys_ui::rows(base.model) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for &i in *row {
                                ui.scope(|ui| knob(ui, actions, i, kind.params()[i].name, 26.0)).response.on_hover_text(crate::keys_ui::hint(i, base.model));
                            }
                        });
                    }
                    if base.model == KeysModel::Organ {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let mut third = base.perc_third;
                            ui.selectable_value(&mut third, false, "2ª").on_hover_text("La percusión suena una octava por encima");
                            ui.selectable_value(&mut third, true, "3ª").on_hover_text("La percusión suena una octava y una quinta por encima");
                            if third != base.perc_third {
                                actions.push(Action::Instrument(InstrumentKind::Keys(KeysParams { perc_third: third, ..base })));
                            }
                            ui.add_space(8.0);
                            let fast = app.display_value(target, Param::Instrument(KEYS_LESLIE_FAST)) >= 0.5;
                            let mut want = fast;
                            ui.selectable_value(&mut want, false, "Lento").on_hover_text("El altavoz giratorio despacio (coral)");
                            ui.selectable_value(&mut want, true, "Rápido").on_hover_text("El altavoz giratorio deprisa (trémolo); tarda un momento en llegar");
                            if want != fast {
                                actions.push(Action::Param(Param::Instrument(KEYS_LESLIE_FAST), if want { 1.0 } else { 0.0 }));
                            }
                        });
                    }
                });
            });
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

/// A multisample's map in `rect`: the keyboard, each zone a block over its keys
/// (velocity layers stacked). A clic plays the key under it.
fn keymap(app: &PulseApp, ui: &mut egui::Ui, t: usize, params: &SamplerParams, rect: Rect) -> Option<u8> {
    let painter = ui.painter_at(rect);
    let key_w = rect.width() / 128.0;
    let keys = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 14.0), rect.max);
    for pitch in 0..128u8 {
        let x = rect.left() + pitch as f32 * key_w;
        let color = if crate::project::is_black_key(pitch) { Color32::from_gray(40) } else { Color32::from_gray(190) };
        painter.rect_filled(Rect::from_min_max(pos2(x, keys.top()), pos2(x + key_w, keys.bottom())), 0.0, color);
    }
    let area = Rect::from_min_max(rect.min + vec2(0.0, 14.0), pos2(rect.right(), keys.top() - 2.0));
    let color = theme::rgb(app.project.tracks[t].color);
    for zone in params.zones() {
        let x0 = rect.left() + zone.lo as f32 * key_w;
        let x1 = rect.left() + (zone.hi as f32 + 1.0) * key_w;
        // Velocity: bottom is soft, top is loud.
        let y = |v: u8| area.bottom() - area.height() * v as f32 / 127.0;
        let block = Rect::from_min_max(pos2(x0, y(zone.vel_hi)), pos2(x1, y(zone.vel_lo.saturating_sub(1))));
        painter.rect_filled(block.shrink(0.5), 1.0, theme::shade(color, 0.6));
        painter.rect_stroke(block.shrink(0.5), 1.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
        let root = rect.left() + (zone.root as f32 + 0.5) * key_w;
        painter.line_segment([pos2(root, keys.top()), pos2(root, keys.bottom())], Stroke::new(2.0, ACCENT));
    }
    let count = params.zone_count;
    painter.text(rect.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, format!("{count} zonas"), theme::small(), TEXT_DIM);
    let response = ui.interact(rect, ui.id().with(("keymap", t)), Sense::click());
    let pitch = response.hover_pos().map(|p| ((p.x - rect.left()) / key_w).clamp(0.0, 127.0) as u8);
    if let Some(pitch) = pitch {
        let track = &app.project.tracks[t];
        let names: Vec<String> = params.zones().iter().enumerate().filter(|(_, z)| (z.lo..=z.hi).contains(&pitch)).map(|(i, z)| format!("{} ({}–{} · vel {}–{})", track.sample_name(i).unwrap_or_default(), crate::project::note_name(z.lo), crate::project::note_name(z.hi), z.vel_lo, z.vel_hi)).collect();
        let text = format!("{}: {}\nClic: escucharla", crate::project::note_name(pitch), if names.is_empty() { "nada".to_string() } else { names.join(", ") });
        response.clone().on_hover_text(text);
    }
    if response.clicked() { pitch } else { None }
}

/// «Zonas…»: each zone's keys, root and velocities, to set by hand. Returns the
/// new settings if something changed.
fn zone_menu(app: &PulseApp, ui: &mut egui::Ui, t: usize, params: &SamplerParams) -> Option<SamplerParams> {
    let mut edited = *params;
    widgets::field_menu_button(ui, "Zonas…", |ui| {
        let note = |ui: &mut egui::Ui, value: &mut u8| {
            ui.add(egui::DragValue::new(value).range(0..=127).speed(0.2).custom_formatter(|n, _| crate::project::note_name(n as u8)).custom_parser(|text| crate::multisample::note_in_name(text).map(|n| n as f64)));
        };
        let velocity = |ui: &mut egui::Ui, value: &mut u8| {
            ui.add(egui::DragValue::new(value).range(1..=127).speed(0.3));
        };
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            egui::Grid::new(("zones", t)).striped(true).show(ui, |ui| {
                for header in ["Muestra", "Desde", "Hasta", "Raíz", "Vel. mín.", "Vel. máx."] {
                    ui.label(egui::RichText::new(header).color(TEXT_DIM).size(10.5));
                }
                ui.end_row();
                let count = edited.zone_count as usize;
                for (i, zone) in edited.zones[..count].iter_mut().enumerate() {
                    let name = app.project.tracks[t].sample_name(i).unwrap_or_default();
                    ui.add_sized([170.0, 18.0], egui::Label::new(name).truncate()).on_hover_text("Las notas desde-hasta y las velocidades que la tocan; Raíz es la nota a la que suena tal cual");
                    note(ui, &mut zone.lo);
                    note(ui, &mut zone.hi);
                    note(ui, &mut zone.root);
                    velocity(ui, &mut zone.vel_lo);
                    velocity(ui, &mut zone.vel_hi);
                    zone.hi = zone.hi.max(zone.lo);
                    zone.vel_hi = zone.vel_hi.max(zone.vel_lo);
                    ui.end_row();
                }
            });
        });
    });
    (edited != *params).then_some(edited)
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
        widgets::name(&response, egui::WidgetType::Button, format!("Pad {}", pad + 1));
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
    widgets::field_menu_button(ui, "Presets", |ui| {
        ui.set_min_width(190.0);
        let factory = crate::presets::factory(&kind);
        if !factory.is_empty() {
            ui.label(egui::RichText::new("DE FÁBRICA").color(TEXT_DIM).size(10.0));
            let groups = crate::presets::grouped(factory);
            if groups.len() > 1 && groups.iter().map(|g| g.1.len()).sum::<usize>() > 12 {
                // Many sounds: one submenu a category («Bajos · …»).
                for (group, presets) in groups {
                    crate::widgets::menu_button(ui, format!("{group}  ({})", presets.len()), |ui| {
                        ui.set_min_width(180.0);
                        egui::ScrollArea::vertical().id_salt(("factory_group", &group)).max_height(420.0).show(ui, |ui| {
                            for (short, preset) in presets {
                                if ui.button(short).clicked() {
                                    actions.push(Action::ApplyPreset(preset));
                                    ui.close();
                                }
                            }
                        });
                    });
                }
            } else {
                egui::ScrollArea::vertical().id_salt("factory_presets").max_height(260.0).show(ui, |ui| {
                    for preset in groups.into_iter().flat_map(|g| g.1) {
                        if ui.button(&preset.1.name).clicked() {
                            actions.push(Action::ApplyPreset(preset.1));
                            ui.close();
                        }
                    }
                });
            }
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
                if ui.small_button("×").on_hover_text("Borrar el preset").clicked() {
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
        if track.sample(0).is_some() && ui.small_button("×").on_hover_text("Volver al click sintetizado").clicked() {
            actions.push(Action::ClearSample(0));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, Track};
    use engine::model::{ConvolutionParams, ReverbSpace};

    /// The devices of `app`'s selected chain drawn a few frames without a window.
    /// Also the picture of the last frame (each frame's textures go to the canvas).
    fn draw(app: &mut PulseApp, size: egui::Vec2) -> (egui::FullOutput, crate::offscreen::Image) {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
        let mut canvas = crate::offscreen::Canvas::default();
        let mut last = None;
        for _ in 0..4 {
            ctx.set_pixels_per_point(2.0);
            let output = ctx.run_ui(input(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| show(app, ui));
            });
            let shapes = output.shapes.clone();
            let image = canvas.draw(&ctx, output, size, 2.0);
            last = Some((shapes, image));
        }
        let (shapes, image) = last.unwrap();
        (egui::FullOutput { shapes, ..Default::default() }, image)
    }

    /// A drum track with the bus compressor and the multiband.
    fn dynamics_app() -> PulseApp {
        let mut project = Project::empty();
        project.tracks.clear();
        let mut track = Track::new_audio("Batería", [200, 120, 60]);
        track.devices.push(Device::new(Effect::Glue(Default::default())));
        track.devices.push(Device::new(Effect::Multiband(Default::default())));
        project.tracks.push(track);
        let mut app = PulseApp::headless(project);
        app.selected_target = Target::Track(0);
        app
    }

    /// Every control of both is drawn, with their meters and presets.
    #[test]
    fn dynamics_devices_show_every_control() {
        let mut app = dynamics_app();
        let (output, _) = draw(&mut app, vec2(2400.0, 220.0));
        let texts: Vec<String> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::epaint::Shape::Text(t) => Some(t.galley.job.text.clone()),
                _ => None,
            })
            .collect();
        for effect in [Effect::Glue(Default::default()), Effect::Multiband(Default::default())] {
            for info in effect.params() {
                assert!(texts.iter().any(|t| t == info.name), "{}: {} not drawn", effect.label(), info.name);
            }
            assert!(texts.iter().any(|t| t == effect.label()));
            assert!(!effect.factory_presets().is_empty());
        }
        for label in ["Ratio", "4:1", "Auto", "Clip", "GR", "L", "M", "H", "Presets"] {
            assert!(texts.iter().any(|t| t == label), "{label} not drawn: {texts:?}");
        }
    }

    /// Their devices as a PNG file, to look at:
    /// `PULSE_SHOTS=dir cargo test --release -p app dynamics_shots -- --ignored`.
    #[test]
    #[ignore]
    fn dynamics_shots() {
        let Some(dir) = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = dynamics_app();
        let size = vec2(1250.0, 190.0);
        let (_, image) = draw(&mut app, size);
        let temp = dir.join(".dinamica.png.tmp");
        image.save_png(&temp).unwrap();
        std::fs::rename(temp, dir.join("dinamica.png")).unwrap();
    }

    /// The device panel of a track with a Convolution Reverb, drawn off screen.
    fn draw_convolution(space: ReverbSpace, frames: usize) -> (egui::Context, crate::offscreen::Image) {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut project = Project::empty();
        project.tracks.clear();
        let mut track = Track::new("Voz", [200, 120, 60], InstrumentKind::Synth(Default::default()));
        track.devices.push(Device::new(Effect::Convolution(ConvolutionParams { space, predelay_ms: 30.0, ..Default::default() })));
        project.tracks.push(track);
        let mut app = crate::app::PulseApp::headless(project);
        app.selected_target = Target::Track(0);
        let mut canvas = crate::offscreen::Canvas::default();
        let size = vec2(1400.0, 200.0);
        let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
        let mut image = None;
        for _ in 0..frames {
            ctx.set_pixels_per_point(2.0);
            let output = ctx.run_ui(input(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| show(&mut app, ui));
            });
            image = Some(canvas.draw(&ctx, output, size, 2.0));
        }
        (ctx, image.unwrap())
    }

    #[test]
    fn the_convolution_reverb_has_its_device() {
        for space in ReverbSpace::ALL {
            draw_convolution(space, 2);
            assert!(!space.hint().is_empty());
        }
        // Factory settings by category, and within the ranges.
        let presets = ConvolutionParams::presets();
        assert!(presets.len() >= 10 && presets.iter().all(|(name, _)| name.contains(" · ")));
        for (name, p) in presets {
            assert!(ConvolutionParams::DECAY.contains(&p.decay) && ConvolutionParams::SIZE.contains(&p.size), "{name}");
            let effect = Effect::Convolution(p);
            for (i, info) in effect.params().iter().enumerate() {
                assert_eq!(info.clamp(effect.get(i)), effect.get(i), "{name}: {}", info.name);
            }
        }
    }

    /// `PULSE_SHOTS=dir cargo test --release -p app convolution_shots -- --ignored`.
    #[test]
    #[ignore]
    fn convolution_shots() {
        let Some(dir) = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        for space in [ReverbSpace::Room, ReverbSpace::Hall, ReverbSpace::Plate, ReverbSpace::Infinite] {
            let (_, image) = draw_convolution(space, 4);
            image.save_png(&dir.join(format!("espacio-{}.png", space.label()))).unwrap();
        }
    }
}
