//! The polychrome synth's editor: two engines and a utility layer, two filters
//! with series/parallel routing, the modulation sources (envelopes, LFOs,
//! random, combinations) and the step sequencer, on the shared engine.

use crate::app::PulseApp;
use crate::devices::{self, Action};
use crate::theme::{self, ACCENT, TEXT_DIM};
use crate::wave_ui::{self, knob, panel, set, source_badge};
use eframe::egui::{self, pos2, vec2, Color32, Rect, Sense};
use engine::model::{ComboOp, ModSource, WaveFilter, WaveParams, SEQ_STEPS, WAVE_BALANCE, WAVE_FILTER2, WAVE_RANDOM_RATE};
use engine::Target;

fn filter2_panel(app: &PulseApp, ui: &mut egui::Ui, t: usize, base: WaveParams, shown: &WaveParams, actions: &mut Vec<Action>) {
    panel(ui, 200.0, |ui| {
        ui.horizontal(|ui| {
            let mut on = base.filter2.on;
            if ui.checkbox(&mut on, egui::RichText::new("FILTER 2").strong()).changed() {
                let mut p = base;
                p.filter2.on = on;
                set(actions, p);
            }
            let mut mode = base.filter2.mode;
            egui::ComboBox::from_id_salt(("poly_filter2", t)).selected_text(mode.label()).width(76.0).show_ui(ui, |ui| {
                for m in WaveFilter::ALL {
                    ui.selectable_value(&mut mode, m, m.label());
                }
            });
            if mode != base.filter2.mode {
                let mut p = base;
                p.filter2.mode = mode;
                set(actions, p);
            }
        });
        // The response view draws `filter`, so show filter 2 in its place.
        let view = WaveParams { filter: shown.filter2, ..*shown };
        wave_ui::filter_view(ui, &view, vec2(196.0, 64.0));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, label) in [(WAVE_FILTER2, "Cutoff"), (WAVE_FILTER2 + 1, "Res"), (WAVE_FILTER2 + 2, "Drive")] {
                knob(app, ui, t, i, label, 28.0, actions);
            }
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, label) in [(WAVE_FILTER2 + 3, "Mix"), (WAVE_FILTER2 + 4, "Key")] {
                knob(app, ui, t, i, label, 28.0, actions);
            }
        });
    });
}

fn routing_panel(app: &PulseApp, ui: &mut egui::Ui, t: usize, base: WaveParams, actions: &mut Vec<Action>) {
    panel(ui, 140.0, |ui| {
        ui.label(egui::RichText::new("RUTA DE FILTROS").color(TEXT_DIM).size(10.0).strong());
        let mut parallel = base.filter_parallel;
        ui.selectable_value(&mut parallel, false, "Serie: 1 → 2");
        ui.selectable_value(&mut parallel, true, "Paralelo: 1 | 2");
        if parallel != base.filter_parallel {
            set(actions, WaveParams { filter_parallel: parallel, ..base });
        }
        ui.add_enabled_ui(base.filter_parallel, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                knob(app, ui, t, WAVE_BALANCE, "Balance", 28.0, actions);
            });
        });
        ui.label(egui::RichText::new("Entradas: en el filtro 1").color(TEXT_DIM).size(10.0));
    });
}

fn random_panel(app: &PulseApp, ui: &mut egui::Ui, t: usize, base: WaveParams, actions: &mut Vec<Action>) {
    panel(ui, 300.0, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("RANDOM").strong());
            source_badge(ui, ModSource::SampleHold);
            source_badge(ui, ModSource::Smooth);
            ui.spacing_mut().item_spacing.x = 0.0;
            knob(app, ui, t, WAVE_RANDOM_RATE, "Rate", 24.0, actions);
        });
        ui.separator();
        ui.label(egui::RichText::new("COMBINAR").strong());
        for c in 0..2 {
            let combo = base.combos[c];
            let mut next = combo;
            ui.horizontal(|ui| {
                source_badge(ui, [ModSource::Combo1, ModSource::Combo2][c]);
                let pick = |ui: &mut egui::Ui, id: (&str, usize, usize), value: &mut ModSource| {
                    egui::ComboBox::from_id_salt(id).selected_text(value.label()).width(70.0).show_ui(ui, |ui| {
                        for source in ModSource::ALL.into_iter().filter(|s| !matches!(s, ModSource::Combo1 | ModSource::Combo2)) {
                            ui.selectable_value(value, source, source.label());
                        }
                    });
                };
                pick(ui, ("poly_combo_a", t, c), &mut next.a);
                egui::ComboBox::from_id_salt(("poly_combo_op", t, c)).selected_text(next.op.label()).width(48.0).show_ui(ui, |ui| {
                    for op in ComboOp::ALL {
                        ui.selectable_value(&mut next.op, op, op.label());
                    }
                });
                pick(ui, ("poly_combo_b", t, c), &mut next.b);
            });
            if next != combo {
                let mut p = base;
                p.combos[c] = next;
                set(actions, p);
            }
        }
    });
}

fn synth_page(app: &PulseApp, ui: &mut egui::Ui, t: usize, base: WaveParams, shown: &WaveParams, actions: &mut Vec<Action>) {
    ui.horizontal_top(|ui| {
        wave_ui::osc_panel(app, ui, t, 0, base, shown, actions);
        wave_ui::osc_panel(app, ui, t, 1, base, shown, actions);
        wave_ui::utility_panel(app, ui, t, base, shown, actions);
    });
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        wave_ui::filter_panel(app, ui, t, base, shown, actions);
        filter2_panel(app, ui, t, base, shown, actions);
        routing_panel(app, ui, t, base, actions);
        wave_ui::env_panel(app, ui, t, base, shown, actions);
    });
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        wave_ui::lfo_panel(app, ui, t, base, shown, actions);
        random_panel(app, ui, t, base, actions);
        wave_ui::arp_panel(app, ui, t, base, shown, actions);
    });
}

const CELL: f32 = 40.0;

/// A row of sixteen bars set by dragging across them; returns the edited values.
fn lane(ui: &mut egui::Ui, label: &str, values: [f32; SEQ_STEPS], (min, max): (f32, f32), length: usize, format: impl Fn(f32) -> String) -> Option<[f32; SEQ_STEPS]> {
    let mut edited = values;
    ui.horizontal(|ui| {
        ui.add_sized(vec2(70.0, 44.0), egui::Label::new(egui::RichText::new(label).color(TEXT_DIM).size(10.5)));
        let (rect, response) = ui.allocate_exact_size(vec2(CELL * SEQ_STEPS as f32, 44.0), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let zero = if min < 0.0 { (0.0 - min) / (max - min) } else { 0.0 };
        for (i, value) in values.iter().enumerate() {
            let cell = Rect::from_min_size(pos2(rect.left() + i as f32 * CELL, rect.top()), vec2(CELL - 2.0, rect.height()));
            let active = i < length;
            painter.rect_filled(cell, 2.0, if active { theme::BG } else { theme::SURFACE });
            let n = ((value - min) / (max - min)).clamp(0.0, 1.0);
            let (a, b) = (zero.min(n), zero.max(n));
            let bar = Rect::from_min_max(pos2(cell.left() + 3.0, cell.bottom() - b * cell.height()), pos2(cell.right() - 3.0, cell.bottom() - a * cell.height()));
            painter.rect_filled(bar, 1.0, if active { ACCENT } else { Color32::from_gray(80) });
            painter.text(cell.center_top() + vec2(0.0, 2.0), egui::Align2::CENTER_TOP, format(*value), theme::small(), theme::TEXT);
        }
        if let Some(pos) = response.interact_pointer_pos().filter(|_| response.dragged() || response.clicked()) {
            let i = (((pos.x - rect.left()) / CELL) as usize).min(SEQ_STEPS - 1);
            let n = ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0);
            edited[i] = min + n * (max - min);
        }
        response.on_hover_text("Arrastra sobre los pasos para dibujar");
    });
    (edited != values).then_some(edited)
}

/// A row of sixteen switches.
fn toggles(ui: &mut egui::Ui, label: &str, values: [bool; SEQ_STEPS], length: usize) -> Option<[bool; SEQ_STEPS]> {
    let mut edited = values;
    ui.horizontal(|ui| {
        ui.add_sized(vec2(70.0, 18.0), egui::Label::new(egui::RichText::new(label).color(TEXT_DIM).size(10.5)));
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, value) in edited.iter_mut().enumerate() {
            let text = egui::RichText::new(format!("{}", i + 1)).size(10.0).color(if *value { Color32::BLACK } else { TEXT_DIM });
            let button = egui::Button::new(text).fill(if *value { if i < length { ACCENT } else { Color32::from_gray(90) } } else { theme::BG }).min_size(vec2(CELL - 2.0, 18.0));
            if ui.add(button).clicked() {
                *value = !*value;
            }
        }
    });
    (edited != values).then_some(edited)
}

fn column(steps: &[engine::model::SeqStep; SEQ_STEPS], f: fn(&engine::model::SeqStep) -> f32) -> [f32; SEQ_STEPS] {
    std::array::from_fn(|i| f(&steps[i]))
}

fn seq_page(ui: &mut egui::Ui, t: usize, base: WaveParams, actions: &mut Vec<Action>) {
    let seq = base.seq;
    let mut next = seq;
    ui.horizontal(|ui| {
        ui.checkbox(&mut next.on, egui::RichText::new("SECUENCIADOR").strong()).on_hover_text("Mientras mantienes una tecla, toca los pasos transpuestos a ella");
        ui.label(egui::RichText::new("Pasos").color(TEXT_DIM).size(10.5));
        egui::ComboBox::from_id_salt(("poly_seq_len", t)).selected_text(next.length.to_string()).width(44.0).show_ui(ui, |ui| {
            for n in 1..=SEQ_STEPS as u8 {
                ui.selectable_value(&mut next.length, n, n.to_string());
            }
        });
        let rate_label = |b: f32| match b {
            b if b == 0.125 => "1/32",
            b if b == 0.25 => "1/16",
            b if b == 0.5 => "1/8",
            b if b == 1.0 => "1/4",
            _ => "1/2",
        };
        egui::ComboBox::from_id_salt(("poly_seq_rate", t)).selected_text(rate_label(next.rate_beats)).width(56.0).show_ui(ui, |ui| {
            for b in [0.125, 0.25, 0.5, 1.0, 2.0] {
                ui.selectable_value(&mut next.rate_beats, b, rate_label(b));
            }
        });
        if ui.button("Variar").on_hover_text("Cambia notas al azar dentro de la escala menor").clicked() {
            const SCALE: [f32; 8] = [0.0, 3.0, 5.0, 7.0, 10.0, 12.0, 15.0, -5.0];
            let mut seed = (ui.ctx().input(|i| i.time) * 1000.0) as u32 | 1;
            for step in &mut next.steps {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                if seed % 3 == 0 {
                    step.pitch = SCALE[(seed / 3) as usize % SCALE.len()];
                }
            }
        }
        if ui.button("Limpiar").clicked() {
            next.steps = [engine::model::SeqStep::default(); SEQ_STEPS];
        }
        source_badge(ui, ModSource::Sequencer);
        if base.arp.on && next.on {
            ui.label(egui::RichText::new("(el secuenciador manda sobre el arpegiador)").color(TEXT_DIM).size(10.5));
        }
    });
    ui.add_space(4.0);
    let length = next.length as usize;
    if let Some(on) = toggles(ui, "Paso", std::array::from_fn(|i| next.steps[i].on), length) {
        for (step, v) in next.steps.iter_mut().zip(on) {
            step.on = v;
        }
    }
    if let Some(values) = lane(ui, "Nota (st)", column(&next.steps, |s| s.pitch), (-24.0, 24.0), length, |v| format!("{:+}", v.round())) {
        for (step, v) in next.steps.iter_mut().zip(values) {
            step.pitch = v.round();
        }
    }
    let lanes: [(&str, fn(&mut engine::model::SeqStep) -> &mut f32, fn(&engine::model::SeqStep) -> f32); 4] = [
        ("Velocidad", |s| &mut s.velocity, |s| s.velocity),
        ("Gate", |s| &mut s.gate, |s| s.gate),
        ("Probabilidad", |s| &mut s.probability, |s| s.probability),
        ("Mod", |s| &mut s.value, |s| s.value),
    ];
    for (label, field, read) in lanes {
        if let Some(values) = lane(ui, label, column(&next.steps, read), (0.0, 1.0), length, |v| format!("{:.0}", v * 100.0)) {
            for (step, v) in next.steps.iter_mut().zip(values) {
                *field(step) = v;
            }
        }
    }
    if let Some(slides) = toggles(ui, "Slide", std::array::from_fn(|i| next.steps[i].slide), length) {
        for (step, v) in next.steps.iter_mut().zip(slides) {
            step.slide = v;
        }
    }
    ui.label(egui::RichText::new("Velocidad multiplica la de la tecla · Gate es la duración de la nota en el paso · Slide liga con el siguiente (usa Glide) · Mod sale por la fuente «Seq Mod»").color(TEXT_DIM).size(10.5));
    if next != seq {
        set(actions, WaveParams { seq: next, ..base });
    }
}

/// The full editor for the track in `app.poly_window`.
pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    let Some(t) = app.poly_window else { return };
    let Some(base) = app.project.tracks.get(t).and_then(|track| match track.instrument {
        engine::model::InstrumentKind::Polychrome(p) => Some(p),
        _ => None,
    }) else {
        app.poly_window = None;
        return;
    };
    let mut open = true;
    let mut actions = Vec::new();
    let title = format!("Polychrome Synth · {}", app.project.tracks[t].name);
    let shown = wave_ui::shown(app, t, base);
    crate::widgets::editor_window(ctx, title, egui::Id::new(("poly_window", t)), &mut open, pos2(50.0, 50.0), |ui| {
            ui.set_width(1120.0);
            let app: &PulseApp = app;
            ui.horizontal(|ui| {
                devices::preset_menu(app, ui, t, &mut actions);
                ui.separator();
                let mut voicing = (base.mono, base.legato);
                ui.selectable_value(&mut voicing, (false, false), "Poly");
                ui.selectable_value(&mut voicing, (true, false), "Mono");
                ui.selectable_value(&mut voicing, (true, true), "Legato");
                if voicing != (base.mono, base.legato) {
                    set(&mut actions, WaveParams { mono: voicing.0, legato: voicing.1, ..base });
                }
                ui.spacing_mut().item_spacing.x = 0.0;
                knob(app, ui, t, 64, "Glide", 24.0, &mut actions);
                knob(app, ui, t, 65, "Master", 24.0, &mut actions);
                ui.add_space(12.0);
                for m in 0..4 {
                    knob(app, ui, t, 60 + m, &format!("Macro {}", m + 1), 24.0, &mut actions);
                }
                ui.add_space(12.0);
                if ui.button("▶ Escuchar").clicked() {
                    actions.push(Action::Preview(60));
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("FUENTES").color(TEXT_DIM).size(10.0).strong());
                for source in ModSource::ALL.into_iter().skip(1) {
                    source_badge(ui, source);
                }
            });
            ui.separator();
            let page = wave_ui::tabs(ui, ui.id().with(("poly_page", t)), &["SYNTH", "SEQ", "FX", "MATRIX"]);
            ui.add_space(4.0);
            match page {
                0 => synth_page(app, ui, t, base, &shown, &mut actions),
                1 => seq_page(ui, t, base, &mut actions),
                2 => wave_ui::fx_page(ui, t, base, &mut actions),
                _ => wave_ui::matrix_page(app, ui, t, base, &mut actions),
            }
        });
    if !open {
        app.poly_window = None;
    }
    devices::apply(app, Target::Track(t), actions);
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::model::InstrumentKind;

    fn frames(app: &mut PulseApp, mut draw: impl FnMut(&mut PulseApp, &mut egui::Ui)) {
        let ctx = egui::Context::default();
        for n in 0..3 {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, 1000.0))), time: Some(n as f64 * 0.05), ..Default::default() };
            let _ = ctx.run_ui(input, |ui| draw(app, ui));
        }
    }

    #[test]
    fn every_page_of_the_editor_draws() {
        for (_, preset) in WaveParams::polychrome_presets() {
            let mut app = PulseApp::headless(crate::project::Project::demo());
            app.set_instrument(0, InstrumentKind::Polychrome(preset));
            app.poly_window = Some(0);
            frames(&mut app, |app, ui| window(app, &ui.ctx().clone()));
            assert_eq!(app.poly_window, Some(0));
            frames(&mut app, |app, ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let mut actions = Vec::new();
                    synth_page(app, ui, 0, preset, &preset, &mut actions);
                    seq_page(ui, 0, preset, &mut actions);
                    assert!(actions.is_empty(), "drawing changes nothing");
                });
            });
        }
    }

    #[test]
    fn drawing_on_a_lane_sets_the_steps_under_the_pointer() {
        use eframe::egui::{Event, PointerButton, Pos2, RawInput};
        let ctx = egui::Context::default();
        let mut values = [0.0f32; SEQ_STEPS];
        let mut area = Rect::NOTHING;
        let press = |pos: Pos2, pressed: bool| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        let run = |events: Vec<Event>, n: usize, values: &mut [f32; SEQ_STEPS], area: &mut Rect| {
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 200.0))), events, time: Some(n as f64 * 0.05), ..Default::default() };
            let _ = ctx.run_ui(input, |ui| {
                let at = ui.cursor().min;
                if let Some(edited) = lane(ui, "Mod", *values, (0.0, 1.0), 16, |v| format!("{v:.1}")) {
                    *values = edited;
                }
                *area = Rect::from_min_size(at + vec2(70.0 + ui.spacing().item_spacing.x, 0.0), vec2(CELL * 16.0, 44.0));
            });
        };
        run(vec![], 0, &mut values, &mut area);
        // Drag along the top of steps 3 to 6.
        let (from, to) = (pos2(area.left() + CELL * 2.5, area.top() + 2.0), pos2(area.left() + CELL * 5.5, area.top() + 2.0));
        let mut frames = vec![vec![Event::PointerMoved(from)], vec![press(from, true)]];
        for k in 1..=12 {
            frames.push(vec![Event::PointerMoved(from + (to - from) * k as f32 / 12.0)]);
        }
        frames.push(vec![press(to, false)]);
        for (n, events) in frames.into_iter().enumerate() {
            run(events, n + 1, &mut values, &mut area);
        }
        assert!(values[2..6].iter().all(|&v| v > 0.9), "{values:?}");
        assert!(values[0] == 0.0 && values[7] == 0.0, "{values:?}");
    }

    /// Renders each preset through a project (held chord or bass note) and returns
    /// (name, peak, rms) and the audio.
    fn render_presets() -> (Vec<(&'static str, f32, f32)>, Vec<f32>, Vec<f32>) {
        use engine::OfflineRenderer;
        let (mut all_l, mut all_r, mut stats) = (Vec::new(), Vec::new(), Vec::new());
        for (name, preset) in WaveParams::polychrome_presets() {
            let mut project = crate::project::Project::empty();
            project.tempo = 124.0;
            let mut track = crate::project::Track::new(name, [200, 100, 50], InstrumentKind::Polychrome(preset));
            let mut clip = engine::model::MidiClip::new("MIDI", 8.0, [200, 100, 50]);
            let notes: &[u8] = if preset.mono || preset.seq.on { &[45] } else { &[60, 64, 67] };
            for &pitch in notes {
                clip.notes.push(engine::model::Note { start: 0.0, length: 7.0, pitch, velocity: 0.9, chance: 1.0 });
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
            renderer.render(144_000, &mut l, &mut r);
            renderer.send(engine::Command::StopAllClips);
            renderer.render(48_000, &mut l, &mut r);
            let peak = l.iter().chain(&r).fold(0f32, |m, s| m.max(s.abs()));
            let rms = (l.iter().map(|s| s * s).sum::<f32>() / l.len() as f32).sqrt();
            stats.push((name, peak, rms));
            all_l.extend(l);
            all_r.extend(r);
        }
        (stats, all_l, all_r)
    }

    #[test]
    fn every_factory_preset_plays_through_a_project() {
        let (stats, l, r) = render_presets();
        assert!(l.iter().chain(&r).all(|s| s.is_finite()));
        for (name, peak, rms) in stats {
            assert!(rms > 0.005 && peak < 1.0, "{name}: peak {peak}, rms {rms}");
        }
    }

    #[test]
    #[ignore = "diagnostic: writes every polychrome preset, four seconds each, to $PULSE_OUT"]
    fn poly_presets_to_wav() {
        let (stats, l, r) = render_presets();
        for (name, peak, _) in stats {
            println!("{name:28} pico {:6.1} dBFS", 20.0 * peak.max(1e-6).log10());
        }
        crate::capture::write_wav(std::path::Path::new(&std::env::var("PULSE_OUT").unwrap()), &l, &r, 48_000.0).unwrap();
    }
}
