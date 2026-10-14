//! The modular synth's editor: the module stack (groups, generators, effects,
//! Mix/Aux and outputs) on the left, the effect lanes on the right and the
//! modulators along the bottom. Drag a module's ⟿ handle onto a generator's
//! Phase, Harmonic, Shift, Level or Semi knob (or an Aux) for audio-rate
//! modulation; drag a modulator's ✥ handle onto any knob for control-rate
//! modulation.

use crate::app::PulseApp;
use crate::devices::{self, Action};
use crate::theme::{self, ACCENT, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Color32, Stroke};
use engine::model::{
    module_slot, slot, AudioMod, AudioParam, CurveInput, Device, Effect, InstrumentKind, LfoMode, ModTarget, ModularParams, ModuleKind,
    Modulator, ModulatorKind, Param, ParamInfo, UnisonMode, Unit, ANALOG_WAVES, CHORDS, CHORD_PICKS, CURVE_LOOPS, DISTORTION_TYPES, FILTER_TYPES,
    GRAIN_SPAWN, GRAIN_SYNC, MODULAR_PARAMS, MOD_AUDIO_AMOUNT, MOD_GLIDE, MOD_LANES, MOD_LANE_FX, MOD_LANE_GAIN, MOD_MACRO_PARAMS, MOD_MASTER,
    MOD_MODULATORS, MOD_MODULES, MOD_MODULE_SLOTS, MOD_NO_LANE, MOD_NO_MODULE, MOD_UNISON, MOD_UNISON_VOICES, NOISE_SEEDS, NOISE_TYPES,
    NONLINEAR_MODES, NONLINEAR_TYPES, SAMPLE_LOOPS, WAVE_USER_TABLE,
};
use engine::wavetable::BUILTIN;
use engine::Target;

/// Payload dragged from a modulator's handle onto a knob.
#[derive(Clone, Copy)]
pub struct ModulatorRef(pub usize);

/// Payload dragged from a module's audio handle onto a modulation target.
#[derive(Clone, Copy)]
pub struct AudioSourceRef(pub usize);

/// Changes to the stack itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StackEdit {
    /// A new module below this one (None: at the top).
    Insert(Option<usize>, ModuleKind),
    Duplicate(usize),
    Remove(usize),
    /// Up (true) or down.
    Move(usize, bool),
}

const AUDIO_GREEN: Color32 = Color32::from_rgb(90, 220, 120);

fn set(actions: &mut Vec<Action>, params: ModularParams) {
    actions.push(Action::Instrument(InstrumentKind::Modular(params)));
}

fn modulator_color(kind: ModulatorKind) -> Color32 {
    match kind {
        ModulatorKind::Envelope => Color32::from_rgb(110, 200, 255),
        ModulatorKind::Lfo => Color32::from_rgb(255, 170, 70),
        ModulatorKind::Random => Color32::from_rgb(240, 210, 110),
        ModulatorKind::Curve => Color32::from_rgb(200, 160, 255),
        ModulatorKind::Macro => Color32::from_rgb(240, 140, 220),
        _ => Color32::from_rgb(255, 140, 120),
    }
}

fn module_color(kind: ModuleKind) -> Color32 {
    match kind {
        k if k.is_generator() => Color32::from_rgb(110, 170, 255),
        ModuleKind::Distortion | ModuleKind::Filter | ModuleKind::NonlinearFilter => ACCENT,
        k if k.is_output() => theme::PLAY,
        _ => Color32::from_gray(150),
    }
}

fn modulator_name(p: &ModularParams, m: usize) -> String {
    let kind = p.modulators[m].kind;
    let same = (0..m).filter(|&i| p.modulators[i].kind == kind).count() + 1;
    format!("{} {same}", kind.label())
}

/// A knob for parameter `i`. `display` shows a normalised (0..1) setting in real
/// units. It draws the control-rate modulation it gets and accepts modulators.
fn knob(app: &PulseApp, ui: &mut egui::Ui, t: usize, i: usize, label: &str, display: Option<ParamInfo>, diameter: f32, actions: &mut Vec<Action>) -> Option<egui::Response> {
    let raw = MODULAR_PARAMS.get(i)?;
    let (target, id) = (Target::Track(t), Param::Instrument(i));
    let value = app.display_value(target, id);
    let info = display.unwrap_or(*raw);
    let shown = if display.is_some() { info.denormalize(value) } else { value };
    let (changed, response) = widgets::param_knob_response(ui, &info, shown, label, diameter, app.automation_state(target, id));
    if let Some(v) = changed {
        actions.push(Action::Param(id, if display.is_some() { info.normalize(v) } else { v }));
    }
    let InstrumentKind::Modular(p) = app.project.tracks[t].instrument else { return Some(response) };
    let n = raw.normalize(value);
    let mut k = 0;
    for modulator in p.modulators.iter().filter(|md| md.kind != ModulatorKind::Off) {
        for target in modulator.targets.iter().filter(|tg| tg.dest as usize == i) {
            let (from, to) = if modulator.bipolar { (n - target.depth, n + target.depth) } else { (n, n + target.depth) };
            widgets::modulation_ring(ui.painter(), response.rect.expand(k as f32 * 2.5), diameter + k as f32 * 5.0, from, to, modulator_color(modulator.kind));
            k += 1;
        }
    }
    if response.dnd_hover_payload::<ModulatorRef>().is_some() {
        ui.painter().rect_stroke(response.rect, 3.0, Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
    }
    // Taking a payload of the wrong type would throw it away, so check the type first.
    if egui::DragAndDrop::has_payload_of_type::<ModulatorRef>(ui.ctx()) {
        if let Some(source) = response.dnd_release_payload::<ModulatorRef>() {
            actions.push(Action::ModularTarget(source.0, i));
        }
    }
    if k > 0 {
        response.context_menu(|ui| {
            for (m, modulator) in p.modulators.iter().enumerate() {
                for (s, target) in modulator.targets.iter().enumerate().filter(|(_, tg)| tg.dest as usize == i) {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(modulator_name(&p, m)).color(modulator_color(modulator.kind)));
                        let mut depth = target.depth * 100.0;
                        let slider = ui.add(egui::Slider::new(&mut depth, -100.0..=100.0).suffix(" %").fixed_decimals(0));
                        let remove = ui.small_button("✕").clicked();
                        if slider.changed() || remove {
                            let mut next = p;
                            next.modulators[m].targets[s] = if remove { ModTarget::NONE } else { ModTarget { depth: depth / 100.0, ..*target } };
                            set(actions, next);
                        }
                    });
                }
            }
        });
    }
    Some(response)
}

/// A module slot knob, which also accepts audio-rate connections when `audio` is set.
#[allow(clippy::too_many_arguments)]
fn slot_knob(app: &PulseApp, ui: &mut egui::Ui, t: usize, module: usize, s: usize, audio: Option<AudioParam>, p: &ModularParams, actions: &mut Vec<Action>) {
    let Some(info) = module_slot(p.modules[module].kind, s) else { return };
    let Some(response) = knob(app, ui, t, ModularParams::module_param(module, s), info.name, Some(info), 26.0, actions) else { return };
    let Some(param) = audio else { return };
    let linked = p.audio.iter().any(|l| l.target as usize == module && l.param == param && l.source != MOD_NO_MODULE);
    if linked {
        ui.painter().circle_filled(response.rect.left_top() + vec2(5.0, 6.0), 3.0, AUDIO_GREEN);
    }
    if response.dnd_hover_payload::<AudioSourceRef>().is_some() {
        ui.painter().rect_stroke(response.rect, 3.0, Stroke::new(1.5, AUDIO_GREEN), egui::StrokeKind::Inside);
    }
    if egui::DragAndDrop::has_payload_of_type::<AudioSourceRef>(ui.ctx()) {
        if let Some(source) = response.dnd_release_payload::<AudioSourceRef>() {
            actions.push(Action::AudioLink(source.0, module, param));
        }
    }
}

fn audio_handle(ui: &mut egui::Ui, t: usize, i: usize) {
    ui.dnd_drag_source(egui::Id::new(("modular_audio", t, i)), AudioSourceRef(i), |ui| {
        ui.label(egui::RichText::new("⟿").color(Color32::BLACK).background_color(AUDIO_GREEN).size(12.0));
    })
    .response
    .on_hover_text("Arrástralo a Phase, Harmonic, Shift, Level o Semi de un generador, o a un Aux: modulación de audio");
}

fn insert_menu(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, after: Option<usize>, label: &str, actions: &mut Vec<Action>) {
    egui::ComboBox::from_id_salt(id).selected_text(label).width(if label.len() > 2 { 120.0 } else { 26.0 }).show_ui(ui, |ui| {
        for kind in ModuleKind::ALL {
            if ui.selectable_label(false, kind.label()).clicked() {
                actions.push(Action::StackEdit(StackEdit::Insert(after, kind)));
            }
        }
    });
}

fn choice_combo(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, value: &mut u8, names: &[&str], width: f32) {
    egui::ComboBox::from_id_salt(id).selected_text(names.get(*value as usize).copied().unwrap_or("?")).width(width).show_ui(ui, |ui| {
        for (i, name) in names.iter().enumerate() {
            ui.selectable_value(value, i as u8, *name);
        }
    });
}

/// Settings that only matter in some modes stay out of the way in the others.
fn slot_shown(module: &engine::model::Module, s: usize) -> bool {
    match module.kind {
        ModuleKind::Granular => match s {
            slot::RATE => module.choice[0] == 0,
            slot::SYNC => module.choice[0] == 1,
            slot::DENSITY => module.choice[0] == 2,
            slot::CHORD_RANGE => module.choice[1] != 0,
            _ => true,
        },
        ModuleKind::Noise => s != slot::SLOPE || module.choice[0] == 0,
        ModuleKind::Sample => !matches!(s, slot::LOOP_START | slot::LOOP_LENGTH | slot::XFADE) || module.sample_loop() != 0,
        ModuleKind::CurveOutput => !matches!(s, slot::CURVE_START | slot::CURVE_END) || module.choice[1] != 0,
        _ => true,
    }
}

/// The synced grain rate, picked from note lengths.
fn sync_choice(app: &PulseApp, ui: &mut egui::Ui, t: usize, i: usize, module: &engine::model::Module, actions: &mut Vec<Action>) {
    let Some(info) = module_slot(module.kind, slot::SYNC) else { return };
    let id = Param::Instrument(ModularParams::module_param(i, slot::SYNC));
    let current = (info.denormalize(app.display_value(Target::Track(t), id)).round() as usize).min(GRAIN_SYNC.len() - 1);
    let mut chosen = current;
    ui.vertical(|ui| {
        ui.label(egui::RichText::new("Sync").size(10.0).color(TEXT_DIM));
        egui::ComboBox::from_id_salt(("mm_grain_sync", t, i)).selected_text(GRAIN_SYNC[current].1).width(52.0).show_ui(ui, |ui| {
            for (k, (_, name)) in GRAIN_SYNC.iter().enumerate() {
                ui.selectable_value(&mut chosen, k, *name);
            }
        });
    });
    if chosen != current {
        actions.push(Action::Param(id, info.normalize(chosen as f32)));
    }
}

/// The global unison: the whole stack once per unison voice.
fn unison_panel(app: &PulseApp, ui: &mut egui::Ui, t: usize, p: ModularParams, actions: &mut Vec<Action>) {
    let mut unison = p.unison;
    ui.label(egui::RichText::new("UNÍSONO").color(TEXT_DIM).size(10.0).strong());
    egui::ComboBox::from_id_salt(("modular_unison_voices", t)).selected_text(format!("{} voces", unison.voices)).width(66.0).show_ui(ui, |ui| {
        for v in 1..=MOD_UNISON_VOICES as u8 {
            ui.selectable_value(&mut unison.voices, v, format!("{v} voces"));
        }
    });
    egui::ComboBox::from_id_salt(("modular_unison_mode", t)).selected_text(unison.mode.label()).width(110.0).show_ui(ui, |ui| {
        for mode in UnisonMode::ALL {
            ui.selectable_value(&mut unison.mode, mode, mode.label());
        }
    });
    if unison.mode == UnisonMode::Chords {
        let chords: Vec<&str> = CHORDS.iter().map(|c| c.0).collect();
        choice_combo(ui, ("modular_unison_chord", t), &mut unison.chord, &chords, 96.0);
    }
    if unison != p.unison {
        set(actions, ModularParams { unison, ..p });
    }
    ui.add_enabled_ui(p.unison.voices > 1, |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        knob(app, ui, t, MOD_UNISON, "Detune", None, 22.0, actions);
        knob(app, ui, t, MOD_UNISON + 1, "Spread", None, 22.0, actions);
        knob(app, ui, t, MOD_UNISON + 2, "Blend", None, 22.0, actions);
        knob(app, ui, t, MOD_UNISON + 3, p.unison.mode.bias_label(), None, 22.0, actions);
    });
}

fn stack_buttons(ui: &mut egui::Ui, t: usize, i: usize, actions: &mut Vec<Action>) {
    if ui.small_button("▲").on_hover_text("Subir").clicked() {
        actions.push(Action::StackEdit(StackEdit::Move(i, true)));
    }
    if ui.small_button("▼").on_hover_text("Bajar").clicked() {
        actions.push(Action::StackEdit(StackEdit::Move(i, false)));
    }
    if ui.small_button("⧉").on_hover_text("Duplicar").clicked() {
        actions.push(Action::StackEdit(StackEdit::Duplicate(i)));
    }
    if ui.small_button("✕").on_hover_text("Quitar").clicked() {
        actions.push(Action::StackEdit(StackEdit::Remove(i)));
    }
    insert_menu(ui, ("modular_insert", t, i), Some(i), "+", actions);
}

fn module_card(app: &PulseApp, ui: &mut egui::Ui, t: usize, i: usize, p: ModularParams, actions: &mut Vec<Action>) {
    let module = p.modules[i];
    let group_number = (0..=i).filter(|&k| p.modules[k].kind == ModuleKind::Group).count();
    if module.kind == ModuleKind::Group {
        egui::Frame::new().fill(theme::CONTROL).corner_radius(2.0).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
            ui.set_width(740.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("GRUPO {group_number}")).strong());
                ui.label(egui::RichText::new("la señal empieza de cero aquí").color(TEXT_DIM).size(10.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| stack_buttons(ui, t, i, actions));
            });
        });
        return;
    }
    let color = module_color(module.kind);
    egui::Frame::new().fill(theme::SURFACE).corner_radius(3.0).inner_margin(egui::Margin { left: 10, right: 6, top: 4, bottom: 4 }).show(ui, |ui| {
        ui.set_width(726.0);
        let rect = ui.max_rect();
        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min - vec2(8.0, 4.0), vec2(3.0, rect.height() + 8.0)), 1.0, color);
        let mut next = module;
        ui.horizontal(|ui| {
            ui.checkbox(&mut next.on, "");
            ui.label(egui::RichText::new(format!("{} {}", i + 1, module.kind.label())).strong().color(color));
            audio_handle(ui, t, i);
            match module.kind {
                ModuleKind::Analog => choice_combo(ui, ("mm_wave", t, i), &mut next.choice[0], &ANALOG_WAVES, 72.0),
                ModuleKind::Wavetable => {
                    let name = if module.table == WAVE_USER_TABLE { app.project.tracks[t].sample_name(i).unwrap_or_else(|| "Archivo".into()) } else { BUILTIN.get(module.table as usize).copied().unwrap_or(BUILTIN[0]).to_string() };
                    egui::ComboBox::from_id_salt(("mm_table", t, i)).selected_text(name).width(120.0).show_ui(ui, |ui| {
                        for (k, label) in BUILTIN.iter().enumerate() {
                            ui.selectable_value(&mut next.table, k as u8, *label);
                        }
                    });
                }
                ModuleKind::Distortion => choice_combo(ui, ("mm_dist", t, i), &mut next.choice[0], &DISTORTION_TYPES, 84.0),
                ModuleKind::Filter => {
                    choice_combo(ui, ("mm_filter", t, i), &mut next.choice[0], &FILTER_TYPES, 84.0);
                    choice_combo(ui, ("mm_slope", t, i), &mut next.choice[1], &["1×", "2×"], 36.0);
                }
                ModuleKind::Mix | ModuleKind::Aux => {
                    ui.checkbox(&mut next.flag, "Invertir");
                }
                ModuleKind::Sample => {
                    // The first layout kept the loop in `flag`; the mode replaces it.
                    let mut mode = module.sample_loop();
                    choice_combo(ui, ("mm_loop", t, i), &mut mode, &SAMPLE_LOOPS, 78.0);
                    if mode != module.sample_loop() {
                        next.choice[0] = mode;
                        next.flag = false;
                    }
                }
                ModuleKind::Granular => {
                    choice_combo(ui, ("mm_spawn", t, i), &mut next.choice[0], &GRAIN_SPAWN, 78.0);
                    let chords: Vec<&str> = CHORDS.iter().map(|c| c.0).collect();
                    choice_combo(ui, ("mm_chord", t, i), &mut next.choice[1], &chords, 96.0);
                    if module.choice[1] != 0 {
                        choice_combo(ui, ("mm_pick", t, i), &mut next.table, &CHORD_PICKS, 66.0);
                    }
                    ui.checkbox(&mut next.flag, "Warm").on_hover_text("Warm Start: todos los granos empiezan ya al tocar la nota");
                    ui.checkbox(&mut next.alt, "Align").on_hover_text("Alinea los granos con la fase de la fundamental (necesita una raíz correcta)");
                }
                ModuleKind::Noise => {
                    choice_combo(ui, ("mm_noise", t, i), &mut next.choice[0], &NOISE_TYPES, 72.0);
                    choice_combo(ui, ("mm_seed", t, i), &mut next.choice[1], &NOISE_SEEDS, 66.0);
                }
                ModuleKind::NonlinearFilter => {
                    choice_combo(ui, ("mm_nl", t, i), &mut next.choice[0], &NONLINEAR_TYPES, 84.0);
                    choice_combo(ui, ("mm_nl_mode", t, i), &mut next.choice[1], &NONLINEAR_MODES, 60.0);
                }
                ModuleKind::Output | ModuleKind::CurveOutput => {
                    ui.checkbox(&mut next.flag, "Out").on_hover_text("Apagado: no suena, solo sirve como fuente de modulación");
                    choice_combo(ui, ("mm_send", t, i), &mut next.choice[0], &["→ Carril 1", "→ Carril 2", "→ Carril 3", "→ Master"], 84.0);
                    if module.kind == ModuleKind::CurveOutput {
                        choice_combo(ui, ("mm_curve_loop", t, i), &mut next.choice[1], &CURVE_LOOPS, 78.0);
                    }
                }
                _ => {}
            }
            if (module.kind == ModuleKind::Wavetable || module.kind.uses_sample()) && ui.small_button("Cargar…").clicked() {
                actions.push(Action::LoadGenFile(i));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| stack_buttons(ui, t, i, actions));
        });
        if module.kind == ModuleKind::CurveOutput {
            let edited = ui.scope(|ui| crate::shape_edit::edit(ui, egui::Id::new(("modular_curve_out", t, i)), vec2(300.0, 60.0), module.curve, None));
            if let Some(curve) = edited.inner {
                next.curve = curve;
            }
            // The loop points, when the mode loops.
            if module.choice[1] != 0 {
                let rect = edited.response.rect;
                for s in [slot::CURVE_START, slot::CURVE_END] {
                    let x = rect.left() + rect.width() * module.get(s);
                    ui.painter().vline(x, rect.y_range(), Stroke::new(1.0, theme::PLAY));
                }
            }
        }
        if next != module {
            let mut params = p;
            params.modules[i] = next;
            set(actions, params);
        }
        // Settings.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let audio_for = |s: usize| -> Option<AudioParam> {
                if module.kind.is_generator() {
                    match s {
                        slot::PHASE => Some(AudioParam::Phase),
                        slot::HARMONIC => Some(AudioParam::Harmonic),
                        slot::SHIFT => Some(AudioParam::Shift),
                        slot::LEVEL => Some(AudioParam::Level),
                        slot::SEMI => Some(AudioParam::Pitch),
                        _ => None,
                    }
                } else if (matches!(module.kind, ModuleKind::Mix | ModuleKind::Aux) || module.kind.is_output()) && s == slot::LEVEL {
                    Some(AudioParam::Level)
                } else {
                    None
                }
            };
            for s in 0..MOD_MODULE_SLOTS {
                if module_slot(module.kind, s).is_none() || !slot_shown(&module, s) {
                    continue;
                }
                if module.kind == ModuleKind::Granular && s == slot::SYNC {
                    sync_choice(app, ui, t, i, &module, actions);
                    continue;
                }
                slot_knob(app, ui, t, i, s, audio_for(s), &p, actions);
            }
            if module.kind == ModuleKind::Aux {
                let (rect, response) = ui.allocate_exact_size(vec2(90.0, 50.0), egui::Sense::hover());
                let hovered = response.dnd_hover_payload::<AudioSourceRef>().is_some();
                ui.painter().rect_stroke(rect.shrink(4.0), 3.0, Stroke::new(1.0, if hovered { AUDIO_GREEN } else { Color32::from_gray(80) }), egui::StrokeKind::Inside);
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "⟵ Input", theme::small(), AUDIO_GREEN);
                if let Some(source) = response.dnd_release_payload::<AudioSourceRef>() {
                    actions.push(Action::AudioLink(source.0, i, AudioParam::Input));
                }
            }
        });
        // Audio-rate connections into this module.
        for (c, link) in p.audio.iter().enumerate().filter(|(_, l)| l.target as usize == i && l.source != MOD_NO_MODULE) {
            ui.horizontal(|ui| {
                let source = link.source as usize;
                let name = p.modules.get(source).map_or("?", |m| m.kind.label());
                ui.label(egui::RichText::new(format!("⟿ {} {name} → {}", source + 1, link.param.label())).color(AUDIO_GREEN).size(10.5));
                let id = Param::Instrument(MOD_AUDIO_AMOUNT + c);
                let mut amount = app.display_value(Target::Track(t), id) * 100.0;
                if ui.add(egui::Slider::new(&mut amount, -100.0..=100.0).suffix(" %").fixed_decimals(0)).changed() {
                    actions.push(Action::Param(id, amount / 100.0));
                }
                if ui.small_button("✕").clicked() {
                    let mut params = p;
                    params.audio[c] = AudioMod::NONE;
                    set(actions, params);
                }
            });
        }
    });
}

fn lane_panel(app: &PulseApp, ui: &mut egui::Ui, t: usize, l: usize, p: ModularParams, actions: &mut Vec<Action>) {
    let lane = p.lanes[l];
    egui::Frame::new().fill(theme::SURFACE).corner_radius(3.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.set_width(330.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("CARRIL {}", l + 1)).strong());
            let mut dest = lane.dest;
            let label = |d: u8| if d == MOD_NO_LANE { "→ Master".to_string() } else { format!("→ Carril {}", d + 1) };
            egui::ComboBox::from_id_salt(("mod_lane_dest", t, l)).selected_text(label(dest)).width(90.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut dest, MOD_NO_LANE, label(MOD_NO_LANE));
                for d in (l + 1)..MOD_LANES {
                    ui.selectable_value(&mut dest, d as u8, label(d as u8));
                }
            });
            if dest != lane.dest {
                let mut next = p;
                next.lanes[l].dest = dest;
                set(actions, next);
            }
            ui.spacing_mut().item_spacing.x = 0.0;
            knob(app, ui, t, MOD_LANE_GAIN + l, "Gain", None, 22.0, actions);
        });
        for slot in 0..MOD_LANE_FX {
            let Some(device) = lane.fx[slot] else { continue };
            let mut edited = device;
            let mut remove = false;
            ui.horizontal(|ui| {
                ui.checkbox(&mut edited.enabled, egui::RichText::new(device.effect.label()).size(11.0));
                remove = ui.small_button("✕").clicked();
            });
            ui.horizontal_wrapped(|ui| {
                devices::discrete_controls(ui, &mut edited.effect, 200 + l * 10 + slot);
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, info) in device.effect.params().iter().enumerate() {
                    if let Some(v) = widgets::param_knob(ui, info, device.effect.get(i), info.name, 20.0, None) {
                        edited.effect.set(i, v);
                    }
                }
            });
            if remove || edited != device {
                let mut next = p;
                next.lanes[l].fx[slot] = if remove { None } else { Some(edited) };
                if remove {
                    let kept: Vec<Option<Device>> = next.lanes[l].fx.iter().copied().filter(Option::is_some).collect();
                    next.lanes[l].fx = [None; MOD_LANE_FX];
                    for (i, d) in kept.into_iter().enumerate() {
                        next.lanes[l].fx[i] = d;
                    }
                }
                set(actions, next);
            }
        }
        if let Some(free) = lane.fx.iter().position(Option::is_none) {
            egui::ComboBox::from_id_salt(("mod_lane_add", t, l)).selected_text("+ Efecto").width(100.0).show_ui(ui, |ui| {
                for effect in Effect::catalog() {
                    if ui.selectable_label(false, effect.label()).clicked() {
                        let mut next = p;
                        next.lanes[l].fx[free] = Some(Device::new(effect));
                        set(actions, next);
                    }
                }
            });
        }
    });
}

fn modulator_card(app: &PulseApp, ui: &mut egui::Ui, t: usize, i: usize, p: ModularParams, actions: &mut Vec<Action>) {
    let modulator = p.modulators[i];
    let color = modulator_color(modulator.kind);
    egui::Frame::new().fill(theme::SURFACE).corner_radius(3.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.set_width(250.0);
        let mut next = modulator;
        ui.horizontal(|ui| {
            ui.dnd_drag_source(egui::Id::new(("modular_source", t, i)), ModulatorRef(i), |ui| {
                ui.label(egui::RichText::new(format!("✥ {}", modulator_name(&p, i))).size(11.0).color(Color32::BLACK).background_color(color));
            })
            .response
            .on_hover_text("Arrástralo a cualquier knob para modularlo");
            ui.checkbox(&mut next.bipolar, "±").on_hover_text("Bipolar: mueve el destino hacia los dos lados");
            if ui.small_button("✕").clicked() {
                next = Modulator::OFF;
            }
        });
        let k = |j: usize| ModularParams::modulator_param(i, j);
        use Unit::*;
        ui.horizontal_top(|ui| match modulator.kind {
            ModulatorKind::Envelope => {
                ui.spacing_mut().item_spacing.x = 0.0;
                knob(app, ui, t, k(0), "Attack", Some(ParamInfo::log("Attack", 0.0005, 10.0, 0.01, Seconds)), 24.0, actions);
                knob(app, ui, t, k(1), "Decay", Some(ParamInfo::log("Decay", 0.001, 10.0, 0.1, Seconds)), 24.0, actions);
                knob(app, ui, t, k(2), "Sustain", None, 24.0, actions);
                knob(app, ui, t, k(3), "Release", Some(ParamInfo::log("Release", 0.001, 10.0, 0.1, Seconds)), 24.0, actions);
            }
            ModulatorKind::Lfo => {
                if let Some(shape) = crate::shape_edit::edit(ui, egui::Id::new(("modular_lfo", t, i)), vec2(120.0, 56.0), modulator.shape, None) {
                    next.shape = shape;
                }
                ui.vertical(|ui| {
                    egui::ComboBox::from_id_salt(("modular_lfo_mode", t, i)).selected_text(next.mode.label()).width(56.0).show_ui(ui, |ui| {
                        for m in LfoMode::ALL {
                            ui.selectable_value(&mut next.mode, m, m.label());
                        }
                    });
                    egui::ComboBox::from_id_salt(("modular_lfo_sync", t, i)).selected_text(crate::wave_ui::sync_label(next.sync_beats)).width(56.0).show_ui(ui, |ui| {
                        for beats in [0.0, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0] {
                            ui.selectable_value(&mut next.sync_beats, beats, crate::wave_ui::sync_label(beats));
                        }
                    });
                });
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_enabled_ui(modulator.sync_beats == 0.0, |ui| knob(app, ui, t, k(0), "Rate", Some(ParamInfo::log("Rate", 0.02, 40.0, 1.0, Hertz)), 22.0, actions));
            }
            ModulatorKind::Random => {
                ui.spacing_mut().item_spacing.x = 0.0;
                knob(app, ui, t, k(0), "Rate", Some(ParamInfo::log("Rate", 0.1, 40.0, 2.0, Hertz)), 24.0, actions);
                knob(app, ui, t, k(1), "Smooth", None, 24.0, actions);
            }
            ModulatorKind::Curve => {
                egui::ComboBox::from_id_salt(("modular_curve_in", t, i)).selected_text(next.input.label()).width(70.0).show_ui(ui, |ui| {
                    for input in CurveInput::ALL {
                        ui.selectable_value(&mut next.input, input, input.label());
                    }
                });
                if let Some(shape) = crate::shape_edit::edit(ui, egui::Id::new(("modular_curve", t, i)), vec2(140.0, 56.0), modulator.shape, None) {
                    next.shape = shape;
                }
            }
            ModulatorKind::Macro => {
                egui::ComboBox::from_id_salt(("modular_macro", t, i)).selected_text(format!("Macro {}", next.index + 1)).width(80.0).show_ui(ui, |ui| {
                    for m in 0..8u8 {
                        ui.selectable_value(&mut next.index, m, format!("Macro {}", m + 1));
                    }
                });
            }
            _ => {
                ui.label(egui::RichText::new(if modulator.kind == ModulatorKind::Note { "La nota tocada, 0 a 1" } else { "La velocidad de la nota" }).color(TEXT_DIM).size(10.5));
            }
        });
        let targets: Vec<(usize, ModTarget)> = modulator.targets.iter().copied().enumerate().filter(|(_, tg)| (tg.dest as usize) < MODULAR_PARAMS.len()).collect();
        if targets.is_empty() {
            ui.label(egui::RichText::new("Sin destinos: arrastra ✥ a un knob").color(TEXT_DIM).size(10.5));
        }
        for (s, target) in targets {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("→ {}", p.param_name(target.dest as usize))).size(10.5));
                let mut depth = target.depth * 100.0;
                if ui.add(egui::Slider::new(&mut depth, -100.0..=100.0).suffix(" %").fixed_decimals(0)).changed() {
                    next.targets[s].depth = depth / 100.0;
                }
                if ui.small_button("✕").clicked() {
                    next.targets[s] = ModTarget::NONE;
                }
            });
        }
        if next != modulator {
            let mut params = p;
            params.modulators[i] = next;
            set(actions, params);
        }
    });
}

/// The compact device in the chain: the stack in brief and the macros.
pub fn compact(app: &PulseApp, ui: &mut egui::Ui, t: usize, p: ModularParams, actions: &mut Vec<Action>) {
    ui.vertical(|ui| {
        let used: Vec<usize> = (0..MOD_MODULES).filter(|&m| p.modules[m].kind != ModuleKind::Empty).collect();
        ui.label(egui::RichText::new(format!("{} módulos · {} moduladores", used.len(), p.modulators.iter().filter(|m| m.kind != ModulatorKind::Off).count())).color(TEXT_DIM).size(10.5));
        for &m in used.iter().take(6) {
            let module = p.modules[m];
            ui.label(egui::RichText::new(format!("{} {}", m + 1, module.kind.label())).size(10.5).color(module_color(module.kind)));
        }
        if ui.small_button("▶").on_hover_text("Escuchar").clicked() {
            actions.push(Action::Preview(60));
        }
    });
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for m in 0..4 {
                knob(app, ui, t, MOD_MACRO_PARAMS + m, &format!("Macro {}", m + 1), None, 26.0, actions);
            }
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for m in 4..8 {
                knob(app, ui, t, MOD_MACRO_PARAMS + m, &format!("Macro {}", m + 1), None, 26.0, actions);
            }
            knob(app, ui, t, MOD_MASTER, "Master", None, 26.0, actions);
        });
    });
}

pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    let Some(t) = app.modular_window else { return };
    let Some(InstrumentKind::Modular(p)) = app.project.tracks.get(t).map(|track| track.instrument) else {
        app.modular_window = None;
        return;
    };
    let mut open = true;
    let mut actions = Vec::new();
    let title = format!("Modular Synth · {}", app.project.tracks[t].name);
    egui::Window::new(title).id(egui::Id::new(("modular_window", t))).open(&mut open).resizable(false).collapsible(false).default_pos(pos2(40.0, 40.0)).show(ctx, |ui| {
        ui.set_width(1120.0);
        let app: &PulseApp = app;
        ui.horizontal(|ui| {
            devices::preset_menu(app, ui, t, &mut actions);
            ui.separator();
            let mut voicing = (p.mono, p.legato);
            ui.selectable_value(&mut voicing, (false, false), "Poly");
            ui.selectable_value(&mut voicing, (true, false), "Mono");
            ui.selectable_value(&mut voicing, (true, true), "Legato");
            if voicing != (p.mono, p.legato) {
                set(&mut actions, ModularParams { mono: voicing.0, legato: voicing.1, ..p });
            }
            ui.spacing_mut().item_spacing.x = 0.0;
            knob(app, ui, t, MOD_GLIDE, "Glide", None, 22.0, &mut actions);
            knob(app, ui, t, MOD_MASTER, "Master", None, 22.0, &mut actions);
            if ui.button("▶").on_hover_text("Escuchar").clicked() {
                actions.push(Action::Preview(60));
            }
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.separator();
            unison_panel(app, ui, t, p, &mut actions);
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("MACROS").color(TEXT_DIM).size(10.0).strong());
            ui.spacing_mut().item_spacing.x = 4.0;
            for m in 0..8 {
                knob(app, ui, t, MOD_MACRO_PARAMS + m, &format!("Macro {}", m + 1), None, 34.0, &mut actions);
            }
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("PILA DE MÓDULOS").color(TEXT_DIM).size(10.0).strong());
                    ui.label(egui::RichText::new("de arriba abajo; cada grupo empieza de cero").color(TEXT_DIM).size(10.0));
                });
                egui::ScrollArea::vertical().id_salt(("modular_stack", t)).max_height(470.0).show(ui, |ui| {
                    for i in (0..MOD_MODULES).filter(|&i| p.modules[i].kind != ModuleKind::Empty) {
                        module_card(app, ui, t, i, p, &mut actions);
                    }
                    let last = p.modules.iter().rposition(|m| m.kind != ModuleKind::Empty);
                    insert_menu(ui, ("modular_add", t), last, "+ Añadir módulo", &mut actions);
                });
            });
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("CARRILES DE EFECTOS").color(TEXT_DIM).size(10.0).strong());
                egui::ScrollArea::vertical().id_salt(("modular_lanes", t)).max_height(470.0).show(ui, |ui| {
                    for l in 0..MOD_LANES {
                        lane_panel(app, ui, t, l, p, &mut actions);
                    }
                });
            });
        });
        ui.separator();
        ui.label(egui::RichText::new("MODULADORES").color(TEXT_DIM).size(10.0).strong());
        egui::ScrollArea::horizontal().id_salt(("modular_mods", t)).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for i in (0..MOD_MODULATORS).filter(|&i| p.modulators[i].kind != ModulatorKind::Off) {
                    modulator_card(app, ui, t, i, p, &mut actions);
                }
                if let Some(free) = p.modulators.iter().position(|m| m.kind == ModulatorKind::Off) {
                    egui::ComboBox::from_id_salt(("modular_add_mod", t)).selected_text("+ Modulador").width(120.0).show_ui(ui, |ui| {
                        for kind in ModulatorKind::ALL {
                            if ui.selectable_label(false, kind.label()).clicked() {
                                let mut next = p;
                                next.modulators[free] = Modulator::new(kind);
                                set(&mut actions, next);
                            }
                        }
                    });
                }
            });
        });
    });
    if !open {
        app.modular_window = None;
    }
    devices::apply(app, Target::Track(t), actions);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, PointerButton, Pos2, RawInput, Rect};
    use engine::model::Module;

    fn app_with(p: ModularParams) -> PulseApp {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        app.set_instrument(0, InstrumentKind::Modular(p));
        app.selected_target = Target::Track(0);
        app
    }

    fn params(app: &PulseApp) -> ModularParams {
        match app.project.tracks[0].instrument {
            InstrumentKind::Modular(p) => p,
            _ => unreachable!(),
        }
    }

    #[test]
    fn the_editor_draws_every_preset() {
        for (name, preset) in ModularParams::presets() {
            let mut app = app_with(preset);
            app.modular_window = Some(0);
            let ctx = egui::Context::default();
            for n in 0..3 {
                let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 1000.0))), time: Some(n as f64 * 0.05), ..Default::default() };
                let _ = ctx.run_ui(input, |ui| window(&mut app, &ui.ctx().clone()));
            }
            assert_eq!(app.modular_window, Some(0), "{name}");
            assert_eq!(params(&app), preset, "{name}: drawing changes nothing");
        }
    }

    #[test]
    fn every_module_kind_and_mode_draws() {
        let mut p = ModularParams::default();
        p.modules = [Module::EMPTY; MOD_MODULES];
        let mut at = 0;
        let mut add = |m: Module| {
            p.modules[at] = m;
            at += 1;
        };
        add(Module::new(ModuleKind::Group));
        for kind in ModuleKind::ALL.into_iter().filter(|&k| k != ModuleKind::Group) {
            add(Module::new(kind));
        }
        for spawn in 0..3 {
            let mut g = Module::new(ModuleKind::Granular);
            g.choice = [spawn, 4];
            add(g);
        }
        for mode in 0..5 {
            let mut s = Module::new(ModuleKind::Sample);
            s.choice[0] = mode;
            add(s);
        }
        let mut curve = Module::new(ModuleKind::CurveOutput);
        curve.choice[1] = 3;
        add(curve);
        p.unison.voices = 4;
        p.unison.mode = UnisonMode::Chords;
        let mut app = app_with(p);
        app.modular_window = Some(0);
        let ctx = egui::Context::default();
        for n in 0..3 {
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 1000.0))), time: Some(n as f64 * 0.05), ..Default::default() };
            let _ = ctx.run_ui(input, |ui| window(&mut app, &ui.ctx().clone()));
        }
        assert_eq!(params(&app), p, "drawing changes nothing");
        // Settings for other modes stay hidden.
        let granular = Module::new(ModuleKind::Granular);
        assert!(slot_shown(&granular, slot::RATE) && !slot_shown(&granular, slot::DENSITY) && !slot_shown(&granular, slot::CHORD_RANGE));
    }

    /// Drags `payload` from a label onto the slot knob, headless.
    fn drag_onto_knob<P: Clone + Send + Sync + 'static>(app: &mut PulseApp, payload: P, module: usize, s: usize) -> Vec<Action> {
        let ctx = egui::Context::default();
        let (mut handle, mut knob_rect) = (Rect::NOTHING, Rect::NOTHING);
        let mut actions = Vec::new();
        let press = |pos: Pos2, pressed: bool| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        let run = |events: Vec<Event>, n: usize, app: &mut PulseApp, actions: &mut Vec<Action>, handle: &mut Rect, knob_rect: &mut Rect| {
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), events, time: Some(n as f64 * 0.05), ..Default::default() };
            let _ = ctx.run_ui(input, |ui| {
                let at = ui.cursor().min;
                ui.dnd_drag_source(egui::Id::new("handle"), payload.clone(), |ui| {
                    ui.label("⟿ source");
                });
                *handle = Rect::from_min_max(at, pos2(at.x + 40.0, ui.cursor().min.y));
                ui.add_space(30.0);
                let at = ui.cursor().min;
                let p = params(app);
                let audio = (s == slot::PHASE).then_some(AudioParam::Phase);
                slot_knob(app, ui, 0, module, s, audio, &p, actions);
                *knob_rect = Rect::from_min_size(at, vec2(46.0, 54.0));
            });
        };
        run(vec![], 0, app, &mut actions, &mut handle, &mut knob_rect);
        let (from, to) = (handle.left_center() + vec2(10.0, 0.0), knob_rect.center());
        let mut frames = vec![vec![Event::PointerMoved(from)], vec![press(from, true)]];
        for k in 1..=8 {
            frames.push(vec![Event::PointerMoved(from + (to - from) * k as f32 / 8.0)]);
        }
        frames.push(vec![press(to, false)]);
        frames.push(vec![]);
        for (n, events) in frames.into_iter().enumerate() {
            run(events, n + 1, app, &mut actions, &mut handle, &mut knob_rect);
        }
        actions
    }

    #[test]
    fn dragging_audio_onto_a_phase_knob_makes_an_audio_connection() {
        let mut p = ModularParams::default();
        p.insert_module(Some(2), Module::new(ModuleKind::Group));
        p.insert_module(Some(3), Module::new(ModuleKind::Analog));
        let mut app = app_with(p);
        let actions = drag_onto_knob(&mut app, AudioSourceRef(4), 1, slot::PHASE);
        devices::apply(&mut app, Target::Track(0), actions);
        let link = params(&app).audio[0];
        assert_eq!((link.source, link.target, link.param, link.amount), (4, 1, AudioParam::Phase, 0.5));
    }

    #[test]
    fn dragging_a_modulator_onto_a_knob_connects_it() {
        let mut p = ModularParams::default();
        p.modulators[0] = Modulator::new(ModulatorKind::Lfo);
        let mut app = app_with(p);
        let actions = drag_onto_knob(&mut app, ModulatorRef(0), 1, slot::LEVEL);
        devices::apply(&mut app, Target::Track(0), actions);
        let target = params(&app).modulators[0].targets[0];
        assert_eq!((target.dest as usize, target.depth), (ModularParams::module_param(1, slot::LEVEL), 0.5));
    }

    #[test]
    fn stack_edits_move_files_with_their_modules() {
        let mut p = ModularParams::default();
        p.modules[1] = Module::new(ModuleKind::Sample);
        let mut app = app_with(p);
        app.project.tracks[0].set_sample(1, Some("/x/hit.wav".into()));
        app.edit_stack(0, StackEdit::Insert(Some(0), ModuleKind::Analog));
        assert_eq!(params(&app).modules[2].kind, ModuleKind::Sample);
        assert_eq!(app.project.tracks[0].sample(2), Some("/x/hit.wav"), "the file followed its module");
        app.edit_stack(0, StackEdit::Move(2, true));
        assert_eq!(app.project.tracks[0].sample(1), Some("/x/hit.wav"));
        app.edit_stack(0, StackEdit::Remove(1));
        assert_eq!(app.project.tracks[0].sample(1), None);
        assert_eq!(params(&app).modules[2].kind, ModuleKind::Output);
    }

    #[test]
    fn every_preset_plays_through_a_project() {
        render_presets(None);
    }

    #[test]
    #[ignore = "diagnostic: writes every modular preset to $PULSE_OUT"]
    fn modular_presets_to_wav() {
        render_presets(Some(std::env::var("PULSE_OUT").unwrap()));
    }

    /// Renders one modular patch for `seconds`, with `file` loaded into module 1.
    fn render_patch(p: ModularParams, file: &std::path::Path, notes: &[u8], seconds: f32) -> (Vec<f32>, Vec<f32>) {
        use engine::OfflineRenderer;
        let mut project = crate::project::Project::empty();
        let mut track = crate::project::Track::new("Granular", [200, 100, 50], InstrumentKind::Modular(p));
        track.set_sample(1, Some(file.to_string_lossy().into_owned()));
        let mut clip = engine::model::MidiClip::new("MIDI", 8.0, [200, 100, 50]);
        for &pitch in notes {
            clip.notes.push(engine::model::Note { start: 0.0, length: seconds as f64 * 2.0 - 1.0, pitch, velocity: 0.9 });
        }
        track.clips[0] = Some(engine::model::Clip::Midi(clip));
        project.tracks = vec![track];
        let mut samples = crate::sync::SampleCache::default();
        let mut renderer = OfflineRenderer::new(48_000.0);
        for command in crate::sync::project_commands(&project, &mut samples, 48_000.0, &Default::default()) {
            renderer.send(command);
        }
        renderer.send(engine::Command::LaunchClip { track: 0, slot: 0 });
        let (mut l, mut r) = (Vec::new(), Vec::new());
        renderer.render((seconds * 48_000.0) as usize, &mut l, &mut r);
        renderer.send(engine::Command::StopAllClips);
        let (mut tail_l, mut tail_r) = (Vec::new(), Vec::new());
        renderer.render(24_000, &mut tail_l, &mut tail_r);
        l.extend(tail_l);
        r.extend(tail_r);
        (l, r)
    }

    /// A source to granulate: two seconds of a vowel-like chord that brightens.
    fn write_source(path: &std::path::Path) {
        let sr = 48_000.0;
        let data: Vec<f32> = (0..96_000)
            .map(|i| {
                let t = i as f32 / sr;
                let bright = 1.0 + 6.0 * t / 2.0;
                [220.0f32, 277.2, 329.6]
                    .iter()
                    .map(|f| (1..8).map(|h| (std::f32::consts::TAU * f * h as f32 * t).sin() / (h as f32).powf(2.5 - bright * 0.2)).sum::<f32>())
                    .sum::<f32>()
                    * 0.15
            })
            .collect();
        crate::capture::write_wav(path, &data, &data, sr).unwrap();
    }

    /// Granular variations of the same source, as heard through the whole app.
    fn granular_patches() -> Vec<(&'static str, ModularParams)> {
        let mut base = ModularParams::default();
        let mut g = Module::new(ModuleKind::Granular);
        g.set(slot::ROOT, 57.0);
        g.set(slot::POSITION, 0.4);
        base.modules[1] = g;
        let mut out = base.modules[2];
        out.set(slot::ATTACK, 0.2);
        out.set(slot::RELEASE, 1.0);
        base.modules[2] = out;
        base.gain = 1.0;
        let with = |f: &dyn Fn(&mut Module)| {
            let mut p = base;
            f(&mut p.modules[1]);
            p
        };
        vec![
            ("Plain", base),
            (
                "Cloud",
                with(&|g| {
                    g.flag = true;
                    g.set(slot::LENGTH, 250.0);
                    g.set(slot::RATE, 40.0);
                    for (s, v) in [(slot::RANDOM_POSITION, 0.6), (slot::RANDOM_PITCH, 0.15), (slot::RANDOM_PAN, 1.0), (slot::RANDOM_TIMING, 0.8), (slot::RANDOM_LEVEL, 0.5)] {
                        g.set(s, v);
                    }
                }),
            ),
            (
                "Chord arpeggio",
                with(&|g| {
                    g.choice = [1, 4];
                    g.table = 2;
                    g.set(slot::SYNC, 5.0);
                    g.set(slot::LENGTH, 140.0);
                    g.set(slot::CHORD_RANGE, 2.0);
                }),
            ),
            (
                "Reverse density",
                with(&|g| {
                    g.choice[0] = 2;
                    g.set(slot::DENSITY, 8.0);
                    g.set(slot::LENGTH, 400.0);
                    g.set(slot::RANDOM_REVERSE, 1.0);
                    g.set(slot::RANDOM_POSITION, 0.3);
                }),
            ),
        ]
    }

    #[test]
    fn a_granular_module_plays_its_file_through_a_project() {
        let dir = std::env::temp_dir().join(format!("pulse-granular-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("source.wav");
        write_source(&file);
        for (name, patch) in granular_patches() {
            let (l, r) = render_patch(patch, &file, &[57], 1.5);
            let rms = (l.iter().map(|s| s * s).sum::<f32>() / l.len() as f32).sqrt();
            let peak = l.iter().chain(&r).fold(0f32, |m, s| m.max(s.abs()));
            assert!(rms > 0.01 && peak < 1.0, "{name}: rms {rms}, peak {peak}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "diagnostic: writes the granular variations to $PULSE_OUT"]
    fn granular_demo_to_wav() {
        let out = std::path::PathBuf::from(std::env::var("PULSE_OUT").unwrap());
        let file = out.with_file_name("granular-source.wav");
        write_source(&file);
        let (mut all_l, mut all_r) = (Vec::new(), Vec::new());
        for (name, patch) in granular_patches() {
            let (l, r) = render_patch(patch, &file, &[57, 64], 3.0);
            println!("{name}");
            all_l.extend(l);
            all_r.extend(r);
        }
        crate::capture::write_wav(&out, &all_l, &all_r, 48_000.0).unwrap();
    }

    fn render_presets(out: Option<String>) {
        use engine::OfflineRenderer;
        let (mut all_l, mut all_r, mut failures) = (Vec::new(), Vec::new(), Vec::new());
        for (name, preset) in ModularParams::presets() {
            let mut project = crate::project::Project::empty();
            let mut track = crate::project::Track::new(name, [200, 100, 50], InstrumentKind::Modular(preset));
            let mut clip = engine::model::MidiClip::new("MIDI", 4.0, [200, 100, 50]);
            let notes: &[u8] = if preset.mono { &[45] } else { &[60, 64, 67] };
            for &pitch in notes {
                clip.notes.push(engine::model::Note { start: 0.0, length: 3.0, pitch, velocity: 0.9 });
            }
            track.clips[0] = Some(engine::model::Clip::Midi(clip));
            project.tracks = vec![track];
            let mut samples = crate::sync::SampleCache::default();
            let mut renderer = OfflineRenderer::new(48_000.0);
            for command in crate::sync::project_commands(&project, &mut samples, 48_000.0, &Default::default()) {
                renderer.send(command);
            }
            renderer.send(engine::Command::LaunchClip { track: 0, slot: 0 });
            let (mut l, mut r) = (Vec::new(), Vec::new());
            renderer.render(96_000, &mut l, &mut r);
            let peak = l.iter().chain(&r).fold(0f32, |m, s| m.max(s.abs()));
            let rms = (l.iter().map(|s| s * s).sum::<f32>() / l.len() as f32).sqrt();
            println!("{name:26} pico {:6.1} dBFS", 20.0 * peak.max(1e-6).log10());
            assert!(l.iter().chain(&r).all(|s| s.is_finite()), "{name}");
            if !(rms > 0.005 && peak < 1.0) {
                failures.push(format!("{name}: peak {peak}, rms {rms}"));
            }
            renderer.send(engine::Command::StopAllClips);
            renderer.render(48_000, &mut l, &mut r);
            all_l.extend(l);
            all_r.extend(r);
        }
        if let Some(out) = out {
            crate::capture::write_wav(std::path::Path::new(&out), &all_l, &all_r, 48_000.0).unwrap();
        }
        assert!(failures.is_empty(), "{failures:?}");
    }
}
