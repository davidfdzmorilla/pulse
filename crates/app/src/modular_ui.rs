//! The modular synth's editor: generator cards with their modifier chains,
//! effect lanes, and modulator cards whose handles drag onto any knob.

use crate::app::PulseApp;
use crate::devices::{self, Action};
use crate::theme::{self, ACCENT, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Color32, Sense, Stroke};
use engine::model::{
    CurveInput, Device, Effect, GenKind, Generator, InstrumentKind, KickDrive, LfoMode, ModTarget, Modifier, ModifierKind, ModularParams,
    Modulator, ModulatorKind, OscKind, Param, ParamInfo, Unit, WaveFilter, WaveOsc, MODULAR_PARAMS, MOD_AMP, MOD_GENS, MOD_GLIDE, MOD_LANES,
    MOD_LANE_FX, MOD_LANE_GAIN, MOD_MACRO_PARAMS, MOD_MASTER, MOD_MODIFIERS, MOD_MODULATORS, MOD_NO_LANE, WAVE_USER_TABLE,
};
use engine::wavetable::BUILTIN;
use engine::Target;

/// Payload dragged from a modulator's handle onto a knob.
#[derive(Clone, Copy)]
pub struct ModulatorRef(pub usize);

fn set(actions: &mut Vec<Action>, params: ModularParams) {
    actions.push(Action::Instrument(InstrumentKind::Modular(params)));
}

fn modulator_color(kind: ModulatorKind) -> Color32 {
    match kind {
        ModulatorKind::Envelope => Color32::from_rgb(110, 200, 255),
        ModulatorKind::Lfo => Color32::from_rgb(120, 230, 140),
        ModulatorKind::Random => Color32::from_rgb(240, 210, 110),
        ModulatorKind::Curve => Color32::from_rgb(200, 160, 255),
        ModulatorKind::Macro => Color32::from_rgb(240, 140, 220),
        _ => Color32::from_rgb(255, 170, 120),
    }
}

fn modulator_name(p: &ModularParams, m: usize) -> String {
    let kind = p.modulators[m].kind;
    let same = (0..m).filter(|&i| p.modulators[i].kind == kind).count() + 1;
    format!("{} {same}", kind.label())
}

/// A knob for parameter `i`. `display` shows a normalised (0..1) setting in real
/// units. The knob draws the modulation it gets and accepts dropped modulators.
fn knob(app: &PulseApp, ui: &mut egui::Ui, t: usize, i: usize, label: &str, display: Option<ParamInfo>, diameter: f32, actions: &mut Vec<Action>) {
    let Some(raw) = MODULAR_PARAMS.get(i) else { return };
    let (target, id) = (Target::Track(t), Param::Instrument(i));
    let value = app.display_value(target, id);
    let info = display.unwrap_or(*raw);
    let shown = if display.is_some() { info.denormalize(value) } else { value };
    let (changed, response) = widgets::param_knob_response(ui, &info, shown, label, diameter, app.automation_state(target, id));
    if let Some(v) = changed {
        actions.push(Action::Param(id, if display.is_some() { info.normalize(v) } else { v }));
    }
    let InstrumentKind::Modular(p) = app.project.tracks[t].instrument else { return };
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
    if let Some(source) = response.dnd_release_payload::<ModulatorRef>() {
        actions.push(Action::ModularTarget(source.0, i));
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
}

/// Real-unit views of the normalised modifier and modulator settings.
fn modifier_display(kind: ModifierKind) -> [Option<(&'static str, ParamInfo)>; 2] {
    use Unit::*;
    let pct = |name| Some((name, ParamInfo::linear(name, 0.0, 1.0, 0.5, Percent)));
    match kind {
        ModifierKind::Filter => [Some(("Cutoff", ParamInfo::log("Cutoff", 20.0, 20_000.0, 632.0, Hertz))), pct("Res")],
        ModifierKind::Distortion => [pct("Drive"), Some(("Tone", ParamInfo::log("Tone", 200.0, 20_000.0, 2000.0, Hertz)))],
        ModifierKind::FmFrom | ModifierKind::PmFrom => [pct("Depth"), None],
        ModifierKind::PhaseDistort => [pct("Amount"), None],
        ModifierKind::Bitcrush => [pct("Crush"), None],
        ModifierKind::Downsample => [Some(("Factor", ParamInfo::log("Factor", 1.0, 64.0, 8.0, Times))), None],
        ModifierKind::Gain => [Some(("Gain", ParamInfo::linear("Gain", -24.0, 24.0, 0.0, Decibels))), None],
        ModifierKind::RingMod | ModifierKind::Off => [None, None],
    }
}

fn gen_view(app: &PulseApp, ui: &mut egui::Ui, t: usize, g: usize, generator: &Generator) {
    let size = vec2(130.0, 64.0);
    match generator.kind {
        GenKind::Analog | GenKind::Harmonic => {
            let osc = WaveOsc {
                kind: if generator.kind == GenKind::Analog { OscKind::Analog } else { OscKind::Harmonic },
                position: if generator.kind == GenKind::Analog { generator.position } else { 0.0 },
                partials: 48.0,
                tilt: generator.position * 2.0 - 1.0,
                ..WaveOsc::default()
            };
            crate::wave_ui::cycle_view(ui, &osc, size);
        }
        GenKind::Wavetable => {
            let table = app.cached_wavetable(t, g);
            crate::wave_ui::table_view(ui, table.as_ref(), generator.position, size);
        }
        _ => {
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter().rect_filled(rect, 2.0, theme::BG);
            let text = match generator.kind {
                GenKind::Sample => app.project.tracks[t].sample_name(g).unwrap_or_else(|| "Carga una muestra".into()),
                _ => "Ruido".into(),
            };
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, theme::small(), TEXT_DIM);
        }
    }
}

fn lane_label(lane: u8) -> &'static str {
    match lane {
        0 => "→ A",
        1 => "→ B",
        2 => "→ C",
        _ => "Sin salida",
    }
}

fn generator_card(app: &PulseApp, ui: &mut egui::Ui, t: usize, g: usize, p: ModularParams, actions: &mut Vec<Action>) {
    let generator = p.gens[g];
    egui::Frame::new().fill(theme::SURFACE).corner_radius(3.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.set_width(720.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("G{}", g + 1)).strong().color(ACCENT));
            let mut kind = generator.kind;
            egui::ComboBox::from_id_salt(("mod_gen_kind", t, g)).selected_text(kind.label()).width(84.0).show_ui(ui, |ui| {
                for k in GenKind::ALL {
                    ui.selectable_value(&mut kind, k, k.label());
                }
            });
            let mut output = generator.output;
            egui::ComboBox::from_id_salt(("mod_gen_out", t, g)).selected_text(lane_label(output)).width(84.0).show_ui(ui, |ui| {
                for lane in [0, 1, 2, MOD_NO_LANE] {
                    ui.selectable_value(&mut output, lane, lane_label(lane)).on_hover_text(if lane == MOD_NO_LANE { "Solo se oye a través de los generadores que lo leen (FM, PM, ring)" } else { "" });
                }
            });
            let mut table = generator.table;
            if generator.kind == GenKind::Wavetable {
                let name = if table == WAVE_USER_TABLE { app.project.tracks[t].sample_name(g).unwrap_or_else(|| "Archivo".into()) } else { BUILTIN.get(table as usize).copied().unwrap_or(BUILTIN[0]).to_string() };
                egui::ComboBox::from_id_salt(("mod_gen_table", t, g)).selected_text(name).width(120.0).show_ui(ui, |ui| {
                    for (i, label) in BUILTIN.iter().enumerate() {
                        ui.selectable_value(&mut table, i as u8, *label);
                    }
                });
            }
            if matches!(generator.kind, GenKind::Wavetable | GenKind::Sample) && ui.small_button("Cargar…").on_hover_text("Archivo de audio o de wavetable para este generador").clicked() {
                actions.push(Action::LoadGenFile(g));
            }
            let remove = ui.small_button("✕").on_hover_text("Quitar el generador").clicked();
            if kind != generator.kind || output != generator.output || table != generator.table || remove {
                let mut next = p;
                next.gens[g] = if remove { Generator::OFF } else { Generator { kind, output, table, ..generator } };
                set(actions, next);
            }
        });
        ui.horizontal_top(|ui| {
            gen_view(app, ui, t, g, &generator);
            ui.spacing_mut().item_spacing.x = 0.0;
            let shape_label = match generator.kind {
                GenKind::Analog => "Shape",
                GenKind::Wavetable => "Pos",
                GenKind::Sample => "Start",
                GenKind::Noise => "Color",
                GenKind::Harmonic => "Tilt",
                GenKind::Off => "",
            };
            for (i, label) in [(0, "Level"), (1, "Pan"), (2, "Oct"), (3, "Semi"), (4, "Fine"), (5, shape_label), (6, "Unison"), (7, "Detune"), (8, "Width")] {
                knob(app, ui, t, ModularParams::gen_param(g, i), label, None, 26.0, actions);
            }
        });
        // The modifier chain.
        ui.horizontal_top(|ui| {
            for k in 0..MOD_MODIFIERS {
                let modifier = generator.modifiers[k];
                if modifier.kind == ModifierKind::Off {
                    continue;
                }
                egui::Frame::new().fill(theme::CONTROL).corner_radius(3.0).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    ui.vertical(|ui| {
                        let mut next = modifier;
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(modifier.kind.label()).strong().size(11.0));
                            match modifier.kind {
                                ModifierKind::Filter => {
                                    egui::ComboBox::from_id_salt(("mod_filter", t, g, k)).selected_text(next.filter.label()).width(64.0).show_ui(ui, |ui| {
                                        for f in WaveFilter::ALL {
                                            ui.selectable_value(&mut next.filter, f, f.label());
                                        }
                                    });
                                }
                                ModifierKind::Distortion => {
                                    egui::ComboBox::from_id_salt(("mod_drive", t, g, k)).selected_text(next.drive.label()).width(54.0).show_ui(ui, |ui| {
                                        for d in KickDrive::ALL {
                                            ui.selectable_value(&mut next.drive, d, d.label());
                                        }
                                    });
                                }
                                kind if kind.uses_source() => {
                                    egui::ComboBox::from_id_salt(("mod_source", t, g, k)).selected_text(format!("G{}", next.source + 1)).width(44.0).show_ui(ui, |ui| {
                                        for s in (0..MOD_GENS).filter(|&s| s != g && p.gens[s].kind != GenKind::Off) {
                                            ui.selectable_value(&mut next.source, s as u8, format!("G{}", s + 1));
                                        }
                                    });
                                }
                                _ => {}
                            }
                            if ui.small_button("✕").clicked() {
                                next.kind = ModifierKind::Off;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for (j, display) in modifier_display(modifier.kind).into_iter().enumerate() {
                                if let Some((label, info)) = display {
                                    knob(app, ui, t, ModularParams::modifier_param(g, k, j), label, Some(info), 24.0, actions);
                                }
                            }
                            let mix_label = if modifier.kind == ModifierKind::RingMod { "Amount" } else { "Mix" };
                            if !modifier.kind.on_phase() {
                                knob(app, ui, t, ModularParams::modifier_param(g, k, 2), mix_label, None, 24.0, actions);
                            }
                        });
                        if next != modifier {
                            let mut params = p;
                            params.gens[g].modifiers[k] = next;
                            set(actions, params);
                        }
                    });
                });
            }
            if let Some(free) = generator.modifiers.iter().position(|m| m.kind == ModifierKind::Off) {
                egui::ComboBox::from_id_salt(("mod_add_modifier", t, g)).selected_text("+ Modificador").width(110.0).show_ui(ui, |ui| {
                    for kind in ModifierKind::ALL {
                        if ui.selectable_label(false, kind.label()).clicked() {
                            let mut next = p;
                            let source = (0..MOD_GENS).find(|&s| s != g && p.gens[s].kind != GenKind::Off).unwrap_or(0) as u8;
                            next.gens[g].modifiers[free] = Modifier { kind, source, ..Modifier::OFF };
                            set(actions, next);
                        }
                    }
                });
            }
        });
    });
}

fn lane_panel(app: &PulseApp, ui: &mut egui::Ui, t: usize, l: usize, p: ModularParams, actions: &mut Vec<Action>) {
    let lane = p.lanes[l];
    egui::Frame::new().fill(theme::SURFACE).corner_radius(3.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.set_width(360.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("CARRIL {}", ["A", "B", "C"][l])).strong());
            let mut dest = lane.dest;
            let label = |d: u8| if d == MOD_NO_LANE { "→ Master".to_string() } else { format!("→ Carril {}", ["A", "B", "C"][d as usize]) };
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
                ui.label(egui::RichText::new(format!("→ {}", MODULAR_PARAMS[target.dest as usize].name)).size(10.5));
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

/// The compact device in the chain: generators summary and the main knobs.
pub fn compact(app: &PulseApp, ui: &mut egui::Ui, t: usize, p: ModularParams, actions: &mut Vec<Action>) {
    ui.vertical(|ui| {
        let active: Vec<usize> = (0..MOD_GENS).filter(|&g| p.gens[g].kind != GenKind::Off).collect();
        ui.label(egui::RichText::new(format!("{} generadores · {} moduladores", active.len(), p.modulators.iter().filter(|m| m.kind != ModulatorKind::Off).count())).color(TEXT_DIM).size(10.5));
        for &g in active.iter().take(4) {
            ui.label(egui::RichText::new(format!("G{} {} {}", g + 1, p.gens[g].kind.label(), lane_label(p.gens[g].output))).size(10.5));
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
            knob(app, ui, t, MOD_MASTER, "Master", None, 26.0, actions);
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (j, label) in ["Attack", "Hold", "Decay", "Sustain", "Release"].into_iter().enumerate() {
                knob(app, ui, t, MOD_AMP + j, label, None, 26.0, actions);
            }
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
        ui.set_width(1110.0);
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
            ui.add_space(8.0);
            for (j, label) in ["Amp A", "Amp H", "Amp D", "Amp S", "Amp R"].into_iter().enumerate() {
                knob(app, ui, t, MOD_AMP + j, label, None, 22.0, &mut actions);
            }
            ui.add_space(8.0);
            for m in 0..8 {
                knob(app, ui, t, MOD_MACRO_PARAMS + m, &format!("M{}", m + 1), None, 22.0, &mut actions);
            }
            if ui.button("▶").on_hover_text("Escuchar").clicked() {
                actions.push(Action::Preview(60));
            }
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("GENERADORES").color(TEXT_DIM).size(10.0).strong());
                egui::ScrollArea::vertical().id_salt(("modular_gens", t)).max_height(430.0).show(ui, |ui| {
                    for g in (0..MOD_GENS).filter(|&g| p.gens[g].kind != GenKind::Off) {
                        generator_card(app, ui, t, g, p, &mut actions);
                    }
                    if let Some(free) = p.gens.iter().position(|g| g.kind == GenKind::Off) {
                        egui::ComboBox::from_id_salt(("modular_add_gen", t)).selected_text("+ Generador").width(130.0).show_ui(ui, |ui| {
                            for kind in GenKind::ALL {
                                if ui.selectable_label(false, kind.label()).clicked() {
                                    let mut next = p;
                                    next.gens[free] = Generator::new(kind);
                                    set(&mut actions, next);
                                }
                            }
                        });
                    }
                });
            });
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("CARRILES DE EFECTOS").color(TEXT_DIM).size(10.0).strong());
                egui::ScrollArea::vertical().id_salt(("modular_lanes", t)).max_height(430.0).show(ui, |ui| {
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

    fn app_with(p: ModularParams) -> PulseApp {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        app.set_instrument(0, InstrumentKind::Modular(p));
        app.selected_target = Target::Track(0);
        app
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
            assert_eq!(app.project.tracks[0].instrument, InstrumentKind::Modular(preset), "{name}: drawing changes nothing");
        }
    }

    #[test]
    fn dragging_a_modulator_onto_a_knob_connects_it() {
        let mut p = ModularParams::default();
        p.modulators[0] = Modulator::new(ModulatorKind::Lfo);
        let mut app = app_with(p);
        let ctx = egui::Context::default();
        let (mut handle, mut knob_rect) = (Rect::NOTHING, Rect::NOTHING);
        let mut actions = Vec::new();
        let press = |pos: Pos2, pressed: bool| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        let run = |events: Vec<Event>, n: usize, app: &mut PulseApp, actions: &mut Vec<Action>, handle: &mut Rect, knob_rect: &mut Rect| {
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), events, time: Some(n as f64 * 0.05), ..Default::default() };
            let _ = ctx.run_ui(input, |ui| {
                let at = ui.cursor().min;
                ui.dnd_drag_source(egui::Id::new("handle"), ModulatorRef(0), |ui| {
                    ui.label("✥ LFO 1");
                });
                *handle = Rect::from_min_max(at, pos2(at.x + 40.0, ui.cursor().min.y));
                ui.add_space(30.0);
                let at = ui.cursor().min;
                knob(app, ui, 0, ModularParams::gen_param(0, 0), "Level", None, 30.0, actions);
                *knob_rect = Rect::from_min_size(at, vec2(46.0, 58.0));
            });
        };
        run(vec![], 0, &mut app, &mut actions, &mut handle, &mut knob_rect);
        let (from, to) = (handle.left_center() + vec2(10.0, 0.0), knob_rect.center());
        let mut frames = vec![vec![Event::PointerMoved(from)], vec![press(from, true)]];
        for k in 1..=8 {
            frames.push(vec![Event::PointerMoved(from + (to - from) * k as f32 / 8.0)]);
        }
        frames.push(vec![press(to, false)]);
        frames.push(vec![]);
        for (n, events) in frames.into_iter().enumerate() {
            run(events, n + 1, &mut app, &mut actions, &mut handle, &mut knob_rect);
        }
        devices::apply(&mut app, Target::Track(0), actions);
        let InstrumentKind::Modular(after) = app.project.tracks[0].instrument else { unreachable!() };
        assert_eq!(after.modulators[0].targets[0].dest as usize, ModularParams::gen_param(0, 0));
        assert_eq!(after.modulators[0].targets[0].depth, 0.5);
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

    fn render_presets(out: Option<String>) {
        use engine::OfflineRenderer;
        let (mut all_l, mut all_r) = (Vec::new(), Vec::new());
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
            assert!(rms > 0.005 && peak < 1.0, "{name}: peak {peak}, rms {rms}");
            renderer.send(engine::Command::StopAllClips);
            renderer.render(48_000, &mut l, &mut r);
            all_l.extend(l);
            all_r.extend(r);
        }
        if let Some(out) = out {
            crate::capture::write_wav(std::path::Path::new(&out), &all_l, &all_r, 48_000.0).unwrap();
        }
    }
}
