//! The wavetable synth's interface: the compact device and the full editor
//! window (oscillators, sub and noise, filter, envelopes, LFOs, matrix).

use crate::app::PulseApp;
use crate::devices::{self, Action};
use crate::theme::{self, ACCENT, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine::model::{
    InstrumentKind, KickWave, LfoMode, ModSlot, ModSource, OscWarp, Param, WaveEnv, WaveFilter, WaveParams, WAVE_MOD_TARGETS, WAVE_NO_TARGET,
    WAVE_PARAMS, WAVE_SLOTS, WAVE_USER_TABLE,
};
use engine::wavetable::{Wavetable, BUILTIN, PREVIEW_POINTS};
use engine::Target;
use std::sync::Arc;

/// Parameter indices (see `WAVE_PARAMS`).
const OSC_PARAMS: usize = 13;
const SUB: usize = 26;
const NOISE: usize = 29;
const FILTER: usize = 32;
const ENVS: usize = 37;
const LFOS: usize = 52;
const MACROS: usize = 60;
const GLIDE: usize = 64;
const MASTER: usize = 65;
const AMOUNTS: usize = WAVE_MOD_TARGETS;

/// Knob for parameter `i`, with automation shown like everywhere else.
fn knob(app: &PulseApp, ui: &mut egui::Ui, t: usize, i: usize, label: &str, diameter: f32, actions: &mut Vec<Action>) {
    let Some(info) = WAVE_PARAMS.get(i) else { return };
    let (target, id) = (Target::Track(t), Param::Instrument(i));
    if let Some(v) = widgets::param_knob(ui, info, app.display_value(target, id), label, diameter, app.automation_state(target, id)) {
        actions.push(Action::Param(id, v));
    }
}

fn set(actions: &mut Vec<Action>, params: WaveParams) {
    actions.push(Action::Instrument(InstrumentKind::Wavetable(params)));
}

/// The wavetable's frames stacked in depth, with the one at `position` highlighted.
fn table_view(ui: &mut egui::Ui, table: Option<&Arc<Wavetable>>, position: f32, size: egui::Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::BG);
    let Some(table) = table else { return };
    let frames = table.preview.len().max(1);
    let shown = frames.min(24);
    let depth = vec2(rect.width() * 0.22, -rect.height() * 0.35);
    let front = Rect::from_min_max(rect.min + vec2(4.0, -depth.y + 4.0), rect.max - vec2(depth.x + 4.0, 4.0));
    let draw = |frame: usize, offset: egui::Vec2, stroke: Stroke| {
        let data = &table.preview[frame.min(frames - 1)];
        let points: Vec<_> = (0..PREVIEW_POINTS)
            .map(|i| pos2(front.left() + i as f32 / (PREVIEW_POINTS - 1) as f32 * front.width(), front.center().y - data[i] * front.height() * 0.45) + offset)
            .collect();
        painter.add(egui::Shape::line(points, stroke));
    };
    for k in (0..shown).rev() {
        let frame = k * (frames - 1) / (shown - 1).max(1);
        let along = k as f32 / (shown - 1).max(1) as f32;
        draw(frame, depth * along, Stroke::new(1.0, Color32::from_gray(52 + (k * 2) as u8)));
    }
    let current = (position.clamp(0.0, 1.0) * (frames - 1) as f32).round() as usize;
    let along = position.clamp(0.0, 1.0);
    draw(current, depth * along, Stroke::new(1.8, ACCENT));
    painter.text(rect.right_bottom() + vec2(-4.0, -3.0), Align2::RIGHT_BOTTOM, format!("{} / {frames}", current + 1), theme::small(), TEXT_DIM);
}

fn table_name(app: &PulseApp, t: usize, o: usize, params: &WaveParams) -> String {
    let index = params.osc[o].table;
    if index == WAVE_USER_TABLE {
        app.project.tracks[t].sample_name(o).unwrap_or_else(|| "Archivo".into())
    } else {
        BUILTIN.get(index as usize).copied().unwrap_or(BUILTIN[0]).to_string()
    }
}

fn table_menu(app: &PulseApp, ui: &mut egui::Ui, t: usize, o: usize, base: WaveParams, actions: &mut Vec<Action>) {
    let name = table_name(app, t, o, &base);
    egui::ComboBox::from_id_salt(("wave_table", t, o)).selected_text(name).width(130.0).show_ui(ui, |ui| {
        for (i, label) in BUILTIN.iter().enumerate() {
            if ui.selectable_label(base.osc[o].table == i as u8, *label).clicked() {
                let mut p = base;
                p.osc[o].table = i as u8;
                set(actions, p);
            }
        }
        ui.separator();
        if ui.button("Cargar wavetable…").on_hover_text("WAV con frames de 2048 muestras (u otro tamaño indicado en el archivo) o un solo ciclo").clicked() {
            actions.push(Action::LoadWavetable(o));
        }
    });
}

fn osc_panel(app: &PulseApp, ui: &mut egui::Ui, t: usize, o: usize, base: WaveParams, shown: &WaveParams, actions: &mut Vec<Action>) {
    let osc = base.osc[o];
    panel(ui, 340.0, |ui| {
        ui.horizontal(|ui| {
            let mut on = osc.on;
            if ui.checkbox(&mut on, egui::RichText::new(if o == 0 { "OSC A" } else { "OSC B" }).strong()).changed() {
                let mut p = base;
                p.osc[o].on = on;
                set(actions, p);
            }
            table_menu(app, ui, t, o, base, actions);
        });
        let table = app.cached_wavetable(t, o);
        table_view(ui, table.as_ref(), shown.osc[o].position, vec2(326.0, 96.0));
        let k = |i: usize| o * OSC_PARAMS + i;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, label) in [(0, "Pos"), (1, "Oct"), (2, "Semi"), (3, "Fine"), (4, "Level"), (5, "Pan")] {
                knob(app, ui, t, k(i), label, 28.0, actions);
            }
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, label) in [(6, "Unison"), (7, "Detune"), (8, "Blend"), (9, "Width"), (10, "Phase"), (11, "Rand")] {
                knob(app, ui, t, k(i), label, 28.0, actions);
            }
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Warp").color(TEXT_DIM).size(10.5));
            let mut warp = osc.warp;
            egui::ComboBox::from_id_salt(("wave_warp", t, o)).selected_text(warp.label()).width(80.0).show_ui(ui, |ui| {
                for w in OscWarp::ALL {
                    ui.selectable_value(&mut warp, w, w.label()).on_hover_text(match w {
                        OscWarp::Fm => "El otro oscilador modula la frecuencia de este",
                        OscWarp::Sync => "Reinicia el ciclo más rápido de lo que suena",
                        _ => "",
                    });
                }
            });
            if warp != osc.warp {
                let mut p = base;
                p.osc[o].warp = warp;
                set(actions, p);
            }
            knob(app, ui, t, k(12), "Amount", 24.0, actions);
        });
    });
}

/// A framed column of controls.
fn panel<R>(ui: &mut egui::Ui, width: f32, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .corner_radius(3.0)
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(width);
            ui.vertical(contents).inner
        })
        .inner
}

/// Approximate magnitude response (dB) of the filter at `hz`.
fn response_db(mode: WaveFilter, cutoff: f32, resonance: f32, hz: f32) -> f32 {
    let w = hz / cutoff.max(1.0);
    let k = (2.0 - 1.98 * resonance).max(0.02);
    let two_pole = |k: f32| -> (f32, f32) { (1.0 - w * w, w * k) };
    let mag = |(re, im): (f32, f32)| (re * re + im * im).sqrt().max(1e-6);
    let lp = |k: f32| 1.0 / mag(two_pole(k));
    let gain = match mode {
        WaveFilter::Lp12 => lp(k),
        WaveFilter::Lp24 => lp(k) * lp(std::f32::consts::SQRT_2),
        WaveFilter::Hp12 => w * w * lp(k),
        WaveFilter::Hp24 => w * w * lp(k) * w * w * lp(std::f32::consts::SQRT_2),
        WaveFilter::Bp12 => w * k * lp(k) * k.max(0.3),
        WaveFilter::Notch => (1.0 - w * w).abs() * lp(k),
        WaveFilter::Ladder => {
            // (1 + jw)^4 + feedback.
            let (mut re, mut im) = (1.0f32, 0.0f32);
            for _ in 0..4 {
                (re, im) = (re - im * w, im + re * w);
            }
            let feedback = resonance * 4.2;
            (1.0 + feedback) / mag((re + feedback, im)) * (1.0 + feedback * 0.4) / (1.0 + feedback)
        }
        WaveFilter::Comb => {
            let fb = resonance * 0.97;
            (1.0 - fb * 0.5) / (1.0 - 2.0 * fb * (std::f32::consts::TAU * w).cos() + fb * fb).sqrt()
        }
        WaveFilter::Formant => {
            let place = ((cutoff / 20.0).ln() / (1000f32).ln()).clamp(0.0, 1.0) * 4.0;
            const VOWELS: [[f32; 3]; 5] = [[800.0, 1150.0, 2900.0], [400.0, 1600.0, 2700.0], [300.0, 2300.0, 3000.0], [450.0, 800.0, 2830.0], [325.0, 700.0, 2530.0]];
            let (a, b) = (place.floor() as usize, (place.floor() as usize + 1).min(4));
            let t = place - place.floor();
            let q = 0.6 - resonance * 0.55;
            (0..3)
                .map(|i| {
                    let f = VOWELS[a][i] + (VOWELS[b][i] - VOWELS[a][i]) * t;
                    let w = hz / f;
                    w * q / mag((1.0 - w * w, w * q)) * [1.0, 0.6, 0.35][i]
                })
                .sum::<f32>()
                * 1.5
        }
    };
    20.0 * gain.max(1e-6).log10()
}

fn filter_view(ui: &mut egui::Ui, p: &WaveParams, size: egui::Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::BG);
    let f = &p.filter;
    let (lo, hi) = (20f32.ln(), 20_000f32.ln());
    let y_of = |db: f32| rect.top() + (18.0 - db.clamp(-36.0, 18.0)) / 54.0 * rect.height();
    painter.line_segment([pos2(rect.left(), y_of(0.0)), pos2(rect.right(), y_of(0.0))], Stroke::new(1.0, Color32::from_gray(40)));
    let points: Vec<_> = (0..=rect.width() as usize)
        .map(|x| {
            let hz = (lo + x as f32 / rect.width() * (hi - lo)).exp();
            let db = response_db(f.mode, f.cutoff, f.resonance, hz);
            let db = 20.0 * ((1.0 - f.mix) + f.mix * 10f32.powf(db / 20.0)).max(1e-6).log10();
            pos2(rect.left() + x as f32, y_of(db))
        })
        .collect();
    let colour = if f.on { ACCENT } else { Color32::from_gray(90) };
    painter.add(egui::Shape::line(points, Stroke::new(1.5, colour)));
    let label = if f.cutoff >= 1000.0 { format!("{:.1} kHz", f.cutoff / 1000.0) } else { format!("{:.0} Hz", f.cutoff) };
    painter.text(rect.right_top() + vec2(-4.0, 3.0), Align2::RIGHT_TOP, label, theme::small(), TEXT_DIM);
}

fn env_view(ui: &mut egui::Ui, e: &WaveEnv, size: egui::Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::BG);
    let sustain_time = 0.3 * (e.attack + e.hold + e.decay + e.release).max(0.05);
    let total = e.attack + e.hold + e.decay + sustain_time + e.release;
    let inner = rect.shrink(4.0);
    let x_of = |s: f32| inner.left() + s / total * inner.width();
    let y_of = |v: f32| inner.bottom() - v * inner.height();
    let mut points = vec![pos2(x_of(0.0), y_of(0.0)), pos2(x_of(e.attack), y_of(1.0)), pos2(x_of(e.attack + e.hold), y_of(1.0))];
    let curve = |from: f32, start: f32, target: f32, time: f32, points: &mut Vec<egui::Pos2>| {
        for i in 1..=16 {
            let s = i as f32 / 16.0 * time;
            let v = target + (start - target) * (-s * 5.0 / time.max(1e-4)).exp();
            points.push(pos2(x_of(from + s), y_of(v)));
        }
    };
    curve(e.attack + e.hold, 1.0, e.sustain, e.decay, &mut points);
    let release_at = e.attack + e.hold + e.decay + sustain_time;
    points.push(pos2(x_of(release_at), y_of(e.sustain)));
    curve(release_at, e.sustain, 0.0, e.release, &mut points);
    painter.add(egui::Shape::line(points, Stroke::new(1.5, ACCENT)));
}

fn sync_label(beats: f32) -> &'static str {
    match beats {
        b if b == 0.0 => "Hz",
        b if b == 0.25 => "1/16",
        b if b == 0.5 => "1/8",
        b if b == 1.0 => "1/4",
        b if b == 2.0 => "1/2",
        b if b == 4.0 => "1 compás",
        b if b == 8.0 => "2 compases",
        b if b == 16.0 => "4 compases",
        _ => "?",
    }
}

fn tabs(ui: &mut egui::Ui, id: egui::Id, labels: &[&str]) -> usize {
    let mut current: usize = ui.ctx().memory(|m| m.data.get_temp(id)).unwrap_or(0);
    ui.horizontal(|ui| {
        for (i, label) in labels.iter().enumerate() {
            if ui.selectable_label(current == i, *label).clicked() {
                current = i;
            }
        }
    });
    ui.ctx().memory_mut(|m| m.data.insert_temp(id, current));
    current
}

fn osc_page(app: &PulseApp, ui: &mut egui::Ui, t: usize, base: WaveParams, shown: &WaveParams, actions: &mut Vec<Action>) {
    ui.horizontal_top(|ui| {
        osc_panel(app, ui, t, 0, base, shown, actions);
        osc_panel(app, ui, t, 1, base, shown, actions);
        ui.vertical(|ui| {
            panel(ui, 150.0, |ui| {
                ui.horizontal(|ui| {
                    let mut on = base.sub.on;
                    if ui.checkbox(&mut on, egui::RichText::new("SUB").strong()).changed() {
                        let mut p = base;
                        p.sub.on = on;
                        set(actions, p);
                    }
                    let mut wave = base.sub.wave;
                    egui::ComboBox::from_id_salt(("wave_sub", t)).selected_text(wave.label()).width(64.0).show_ui(ui, |ui| {
                        for w in KickWave::ALL {
                            ui.selectable_value(&mut wave, w, w.label());
                        }
                    });
                    if wave != base.sub.wave {
                        let mut p = base;
                        p.sub.wave = wave;
                        set(actions, p);
                    }
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (i, label) in [(SUB, "Oct"), (SUB + 1, "Level"), (SUB + 2, "Pan")] {
                        knob(app, ui, t, i, label, 26.0, actions);
                    }
                });
            });
            panel(ui, 150.0, |ui| {
                let mut on = base.noise.on;
                if ui.checkbox(&mut on, egui::RichText::new("NOISE").strong()).changed() {
                    let mut p = base;
                    p.noise.on = on;
                    set(actions, p);
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (i, label) in [(NOISE, "Level"), (NOISE + 1, "Color"), (NOISE + 2, "Pan")] {
                        knob(app, ui, t, i, label, 26.0, actions);
                    }
                });
            });
        });
        panel(ui, 200.0, |ui| {
            ui.horizontal(|ui| {
                let mut on = base.filter.on;
                if ui.checkbox(&mut on, egui::RichText::new("FILTER").strong()).changed() {
                    let mut p = base;
                    p.filter.on = on;
                    set(actions, p);
                }
                let mut mode = base.filter.mode;
                egui::ComboBox::from_id_salt(("wave_filter", t)).selected_text(mode.label()).width(76.0).show_ui(ui, |ui| {
                    for m in WaveFilter::ALL {
                        ui.selectable_value(&mut mode, m, m.label());
                    }
                });
                if mode != base.filter.mode {
                    let mut p = base;
                    p.filter.mode = mode;
                    set(actions, p);
                }
            });
            filter_view(ui, shown, vec2(196.0, 64.0));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Entrada").color(TEXT_DIM).size(10.5));
                for (i, label) in ["A", "B", "S", "N"].into_iter().enumerate() {
                    let mut routed = base.filter.routes[i];
                    if ui.toggle_value(&mut routed, label).on_hover_text("Pasa por el filtro").changed() {
                        let mut p = base;
                        p.filter.routes[i] = routed;
                        set(actions, p);
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, label) in [(FILTER, "Cutoff"), (FILTER + 1, "Res"), (FILTER + 2, "Drive")] {
                    knob(app, ui, t, i, label, 28.0, actions);
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, label) in [(FILTER + 3, "Mix"), (FILTER + 4, "Key")] {
                    knob(app, ui, t, i, label, 28.0, actions);
                }
            });
        });
    });
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        panel(ui, 330.0, |ui| {
            let e = tabs(ui, ui.id().with(("wave_env_tab", t)), &["ENV 1 · Amp", "ENV 2", "ENV 3"]);
            env_view(ui, &shown.env[e], vec2(316.0, 70.0));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (k, label) in ["Attack", "Hold", "Decay", "Sustain", "Release"].into_iter().enumerate() {
                    knob(app, ui, t, ENVS + e * 5 + k, label, 28.0, actions);
                }
            });
        });
        panel(ui, 420.0, |ui| {
            let l = tabs(ui, ui.id().with(("wave_lfo_tab", t)), &["LFO 1", "LFO 2", "LFO 3", "LFO 4"]);
            ui.horizontal_top(|ui| {
                let id = ui.id().with(("wave_lfo_shape", t, l));
                if let Some(shape) = crate::shape_edit::edit(ui, id, vec2(260.0, 100.0), base.lfo[l].shape, None) {
                    let mut p = base;
                    p.lfo[l].shape = shape;
                    set(actions, p);
                }
                ui.vertical(|ui| {
                    let mut mode = base.lfo[l].mode;
                    let mut sync = base.lfo[l].sync_beats;
                    ui.horizontal(|ui| {
                        for m in LfoMode::ALL {
                            ui.selectable_value(&mut mode, m, m.label()).on_hover_text(match m {
                                LfoMode::Free => "Corre libre; todas las voces a la vez",
                                LfoMode::Retrig => "Empieza de cero con cada nota",
                                LfoMode::Envelope => "Recorre la forma una vez por nota",
                            });
                        }
                    });
                    egui::ComboBox::from_id_salt(("wave_lfo_sync", t, l)).selected_text(sync_label(sync)).width(90.0).show_ui(ui, |ui| {
                        for beats in [0.0, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0] {
                            ui.selectable_value(&mut sync, beats, sync_label(beats));
                        }
                    });
                    if mode != base.lfo[l].mode || sync != base.lfo[l].sync_beats {
                        let mut p = base;
                        p.lfo[l].mode = mode;
                        p.lfo[l].sync_beats = sync;
                        set(actions, p);
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.add_enabled_ui(sync == 0.0, |ui| knob(app, ui, t, LFOS + l * 2, "Rate", 26.0, actions));
                        knob(app, ui, t, LFOS + l * 2 + 1, "Rise", 26.0, actions);
                    });
                });
            });
        });
        panel(ui, 270.0, |ui| {
            ui.label(egui::RichText::new("ASIGNAR MODULACIÓN").color(TEXT_DIM).size(10.0).strong());
            ui.label(egui::RichText::new("Elige la fuente y el destino en la página MATRIX, o usa un atajo:").size(10.5).color(TEXT_DIM));
            for (label, source, dest) in [
                ("LFO 1 → Cutoff", ModSource::Lfo1, FILTER),
                ("LFO 1 → Pos A", ModSource::Lfo1, 0),
                ("Env 2 → Cutoff", ModSource::Env2, FILTER),
                ("Env 2 → Pitch A", ModSource::Env2, 2),
                ("Macro 1 → Pos A", ModSource::Macro1, 0),
                ("Velocity → Cutoff", ModSource::Velocity, FILTER),
            ] {
                if ui.small_button(label).clicked() {
                    if let Some(slot) = base.matrix.iter().position(|s| s.source == ModSource::None) {
                        let mut p = base;
                        p.matrix[slot] = ModSlot { source, dest: dest as u8, amount: 0.5, bipolar: false };
                        set(actions, p);
                    }
                }
            }
        });
    });
}

fn matrix_page(app: &PulseApp, ui: &mut egui::Ui, t: usize, base: WaveParams, actions: &mut Vec<Action>) {
    egui::Grid::new(("wave_matrix", t)).num_columns(6).spacing([10.0, 3.0]).striped(true).show(ui, |ui| {
        for h in ["#", "Fuente", "Cantidad", "Destino", "Bipolar", ""] {
            ui.label(egui::RichText::new(h).color(TEXT_DIM).size(10.5));
        }
        ui.end_row();
        for s in 0..WAVE_SLOTS {
            let slot = base.matrix[s];
            ui.label(format!("{}", s + 1));
            let mut source = slot.source;
            egui::ComboBox::from_id_salt(("wave_src", t, s)).selected_text(source.label()).width(90.0).show_ui(ui, |ui| {
                for src in ModSource::ALL {
                    ui.selectable_value(&mut source, src, src.label());
                }
            });
            let id = Param::Instrument(AMOUNTS + s);
            let mut amount = app.display_value(Target::Track(t), id) * 100.0;
            if ui.add(egui::Slider::new(&mut amount, -100.0..=100.0).suffix(" %").fixed_decimals(0)).changed() {
                actions.push(Action::Param(id, amount / 100.0));
            }
            let mut dest = slot.dest;
            let dest_name = WAVE_PARAMS.get(dest as usize).filter(|_| (dest as usize) < WAVE_MOD_TARGETS).map_or("—", |p| p.name);
            egui::ComboBox::from_id_salt(("wave_dst", t, s)).selected_text(dest_name).width(120.0).height(320.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut dest, WAVE_NO_TARGET, "—");
                for (i, info) in WAVE_PARAMS.iter().enumerate().take(WAVE_MOD_TARGETS) {
                    ui.selectable_value(&mut dest, i as u8, info.name);
                }
            });
            let mut bipolar = slot.bipolar;
            ui.checkbox(&mut bipolar, "");
            let clear = ui.small_button("✕").on_hover_text("Vaciar").clicked();
            if source != slot.source || dest != slot.dest || bipolar != slot.bipolar || clear {
                let mut p = base;
                p.matrix[s] = if clear { ModSlot::EMPTY } else { ModSlot { source, dest, bipolar, ..slot } };
                if source != ModSource::None && slot.source == ModSource::None && slot.amount == 0.0 {
                    p.matrix[s].amount = 0.5;
                }
                set(actions, p);
            }
            ui.end_row();
        }
    });
}

/// The settings as they currently play, with automation applied.
pub fn shown(app: &PulseApp, t: usize, base: WaveParams) -> WaveParams {
    let mut kind = InstrumentKind::Wavetable(base);
    for i in 0..kind.params().len() {
        kind.set(i, app.display_value(Target::Track(t), Param::Instrument(i)));
    }
    match kind {
        InstrumentKind::Wavetable(p) => p,
        _ => base,
    }
}

/// The compact device in the chain: table, main knobs and macros.
pub fn compact(app: &PulseApp, ui: &mut egui::Ui, t: usize, base: WaveParams, actions: &mut Vec<Action>) {
    let shown = shown(app, t, base);
    ui.vertical(|ui| {
        table_menu(app, ui, t, 0, base, actions);
        let table = app.cached_wavetable(t, 0);
        table_view(ui, table.as_ref(), shown.osc[0].position, vec2(190.0, 80.0));
        if ui.small_button("▶").on_hover_text("Escuchar").clicked() {
            actions.push(Action::Preview(60));
        }
    });
    ui.vertical(|ui| {
        for row in [[(0, "Pos"), (6, "Unison"), (7, "Detune"), (FILTER, "Cutoff"), (FILTER + 1, "Res"), (MASTER, "Volume")], [(ENVS, "Attack"), (ENVS + 2, "Decay"), (ENVS + 3, "Sustain"), (ENVS + 4, "Release"), (MACROS, "Macro 1"), (MACROS + 1, "Macro 2")]] {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, label) in row {
                    knob(app, ui, t, i, label, 26.0, actions);
                }
            });
        }
    });
}

/// The full editor, in its own window inside Pulse, for the track in `app.wave_window`.
pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    let Some(t) = app.wave_window else { return };
    let Some(InstrumentKind::Wavetable(base)) = app.project.tracks.get(t).map(|track| track.instrument) else {
        app.wave_window = None;
        return;
    };
    let mut open = true;
    let mut actions = Vec::new();
    let title = format!("Wavetable Synth · {}", app.project.tracks[t].name);
    let shown = shown(app, t, base);
    egui::Window::new(title)
        .id(egui::Id::new(("wave_window", t)))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .default_pos(pos2(60.0, 60.0))
        .show(ctx, |ui| {
            ui.set_width(1100.0);
            let app: &PulseApp = app;
            ui.horizontal(|ui| {
                devices::preset_menu(app, ui, t, &mut actions);
                ui.separator();
                let mut voicing = (base.mono, base.legato);
                ui.selectable_value(&mut voicing, (false, false), "Poly");
                ui.selectable_value(&mut voicing, (true, false), "Mono");
                ui.selectable_value(&mut voicing, (true, true), "Legato").on_hover_text("Mono; las notas que se solapan se deslizan sin reiniciar las envolventes");
                if voicing != (base.mono, base.legato) {
                    set(&mut actions, WaveParams { mono: voicing.0, legato: voicing.1, ..base });
                }
                ui.spacing_mut().item_spacing.x = 0.0;
                knob(app, ui, t, GLIDE, "Glide", 24.0, &mut actions);
                knob(app, ui, t, MASTER, "Master", 24.0, &mut actions);
                ui.add_space(12.0);
                for m in 0..4 {
                    knob(app, ui, t, MACROS + m, &format!("Macro {}", m + 1), 24.0, &mut actions);
                }
                ui.add_space(12.0);
                if ui.button("▶ Escuchar").clicked() {
                    actions.push(Action::Preview(60));
                }
            });
            ui.separator();
            let page = tabs(ui, ui.id().with(("wave_page", t)), &["OSC", "MATRIX"]);
            ui.add_space(4.0);
            match page {
                0 => osc_page(app, ui, t, base, &shown, &mut actions),
                _ => matrix_page(app, ui, t, base, &mut actions),
            }
        });
    if !open {
        app.wave_window = None;
    }
    devices::apply(app, Target::Track(t), actions);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_curves_have_the_right_shape() {
        let db = |mode, hz| response_db(mode, 1000.0, 0.1, hz);
        assert!(db(WaveFilter::Lp24, 100.0) > -1.0 && db(WaveFilter::Lp24, 10_000.0) < -30.0);
        assert!(db(WaveFilter::Hp12, 100.0) < -20.0 && db(WaveFilter::Hp12, 10_000.0) > -1.0);
        assert!(db(WaveFilter::Bp12, 1000.0) > db(WaveFilter::Bp12, 100.0) + 10.0);
        assert!(db(WaveFilter::Notch, 1000.0) < -20.0);
        let resonant = response_db(WaveFilter::Lp12, 1000.0, 0.9, 1000.0);
        assert!(resonant > 10.0, "resonance peaks at the cutoff: {resonant}");
    }

    /// Runs `draw` headless for a few frames.
    fn frames(app: &mut PulseApp, mut draw: impl FnMut(&mut PulseApp, &mut egui::Ui)) {
        let ctx = egui::Context::default();
        for n in 0..3 {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, 900.0))), time: Some(n as f64 * 0.05), ..Default::default() };
            let _ = ctx.run_ui(input, |ui| draw(app, ui));
        }
    }

    fn app_with(kind: InstrumentKind) -> PulseApp {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        app.set_instrument(0, kind);
        app.selected_target = Target::Track(0);
        app
    }

    #[test]
    fn every_instrument_device_draws() {
        for kind in InstrumentKind::catalog() {
            let mut app = app_with(kind);
            frames(&mut app, |app, ui| {
                egui::CentralPanel::default().show(ui, |ui| devices::show(app, ui));
            });
        }
    }

    #[test]
    fn the_editor_windows_draw_every_page() {
        let mut app = app_with(InstrumentKind::Wavetable(WaveParams::presets()[4].1));
        app.wave_window = Some(0);
        frames(&mut app, |app, ui| window(app, &ui.ctx().clone()));
        assert_eq!(app.wave_window, Some(0), "still open");
        let base = match app.project.tracks[0].instrument {
            InstrumentKind::Wavetable(p) => p,
            _ => unreachable!(),
        };
        frames(&mut app, |app, ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let mut actions = Vec::new();
                matrix_page(app, ui, 0, base, &mut actions);
                assert!(actions.is_empty(), "drawing changes nothing");
            });
        });

        let mut app = app_with(InstrumentKind::Kick(Default::default()));
        app.kick_window = Some(0);
        frames(&mut app, |app, ui| crate::kick_ui::window(app, &ui.ctx().clone()));
        assert_eq!(app.kick_window, Some(0));
    }

    #[test]
    fn a_wavetable_track_renders_through_the_project() {
        use engine::OfflineRenderer;
        let mut project = crate::project::Project::empty();
        let mut track = crate::project::Track::new("Lead", [200, 100, 50], InstrumentKind::Wavetable(WaveParams::presets()[1].1));
        let mut clip = engine::model::MidiClip::new("MIDI", 4.0, [200, 100, 50]);
        for pitch in [60, 64, 67] {
            clip.notes.push(engine::model::Note { start: 0.0, length: 2.0, pitch, velocity: 0.9 });
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
        renderer.render(24_000, &mut l, &mut r);
        let peak = renderer.state.tracks[0].peak.take();
        assert!(peak > 0.05 && peak < 2.0, "the synth plays its chord: {peak}");
    }

    #[test]
    fn wavetable_files_load_with_their_frame_size() {
        let dir = std::env::temp_dir().join(format!("pulse-wt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("table.wav");
        // Four 1024-sample frames, from sine to square, with the frame size in a clm chunk.
        let data: Vec<f32> = (0..4096)
            .map(|i| {
                let phase = (i % 1024) as f32 / 1024.0;
                let sine = (phase * std::f32::consts::TAU).sin();
                let k = (i / 1024) as f32 / 3.0;
                sine + (sine.signum() - sine) * k
            })
            .collect();
        crate::capture::write_wav(&path, &data, &data, 48_000.0).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        let text = b"<!>1024 10000000 wavetable";
        bytes.extend_from_slice(b"clm ");
        bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
        bytes.extend_from_slice(text);
        let riff = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&riff.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();

        let mut cache = crate::sync::SampleCache::default();
        let table = cache.table(&path.to_string_lossy());
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(table.frames(), 4, "{:?}", cache.errors);
        assert!((table.read(0, 0.0, 0.25) - 1.0).abs() < 0.05, "first frame is a sine");
        assert!(table.read(0, 1.0, 0.1) > 0.8, "last frame is square");
    }

    #[test]
    #[ignore = "diagnostic: writes every wavetable preset, two seconds each, to $PULSE_OUT"]
    fn wave_presets_to_wav() {
        use engine::OfflineRenderer;
        let out = std::env::var("PULSE_OUT").unwrap();
        let (mut all_l, mut all_r) = (Vec::new(), Vec::new());
        for (name, preset) in WaveParams::presets() {
            let mut project = crate::project::Project::empty();
            let mut track = crate::project::Track::new(name, [200, 100, 50], InstrumentKind::Wavetable(preset));
            let mut clip = engine::model::MidiClip::new("MIDI", 4.0, [200, 100, 50]);
            let notes: &[u8] = if preset.mono { &[36, 36, 43, 36] } else { &[60, 64, 67] };
            for (i, &pitch) in notes.iter().enumerate() {
                let start = if preset.mono { i as f64 * 0.75 } else { 0.0 };
                let length = if preset.mono { 0.7 } else { 2.5 };
                clip.notes.push(engine::model::Note { start, length, pitch, velocity: 0.9 });
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
            renderer.send(engine::Command::StopAllClips);
            renderer.render(48_000, &mut l, &mut r);
            let peak = l.iter().chain(&r).fold(0f32, |m, s| m.max(s.abs()));
            println!("{name:22} pico {:6.1} dBFS", 20.0 * peak.max(1e-6).log10());
            all_l.extend(l);
            all_r.extend(r);
        }
        crate::capture::write_wav(std::path::Path::new(&out), &all_l, &all_r, 48_000.0).unwrap();
    }
}
