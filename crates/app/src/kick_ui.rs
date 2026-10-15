//! The kick designer's interface: the envelope editor (also used in the compact
//! device), and the full editor window with layers, processing, oscilloscope and
//! spectrum.

use crate::app::PulseApp;
use crate::devices::{self, Action, Zones};
use crate::theme::{self, ACCENT, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine::model::{InstrumentKind, KickDrive, KickParams, KickWave, Param, Shape, KICK_CLICKS, KICK_SYNC_BEATS};
use engine::{Sample, Target};
use std::sync::Arc;

/// Rate of the renders behind the displays: plenty for a kick's spectrum below 6 kHz.
const PREVIEW_RATE: f32 = 12_000.0;

/// The track's kick settings with automated values as they currently play.
pub fn shown(app: &PulseApp, t: usize, base: KickParams) -> KickParams {
    let mut kind = InstrumentKind::Kick(base);
    for i in 0..kind.params().len() {
        kind.set(i, app.display_value(Target::Track(t), Param::Instrument(i)));
    }
    match kind {
        InstrumentKind::Kick(p) => p,
        _ => base,
    }
}

/// One rendered hit for the displays, cached until a setting, the tempo or the click changes.
fn preview(ui: &egui::Ui, id: egui::Id, p: &KickParams, click: Option<Arc<Sample>>, tempo: f64) -> Arc<Vec<f32>> {
    let key = (*p, click.as_ref().map(|c| Arc::as_ptr(c) as usize), tempo.to_bits());
    type Cached = ((KickParams, Option<usize>, u64), Arc<Vec<f32>>);
    match ui.ctx().memory(|m| m.data.get_temp::<Cached>(id.with("hit"))) {
        Some((k, hit)) if k == key => hit,
        _ => {
            let hit = Arc::new(engine::render_kick(*p, click, PREVIEW_RATE, tempo));
            ui.ctx().memory_mut(|m| m.data.insert_temp(id.with("hit"), (key, hit.clone())));
            hit
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Curve {
    Pitch,
    Amp,
    Drive,
}

/// The envelope editor: the hit's waveform with the pitch, level or drive curve
/// on top, all drawable by hand.
///
/// Drag a point to move it, double-click to add or remove one, drag the small
/// handle in the middle of a segment to bend it; right-click to reset. With
/// `snap`, points land on the grid.
#[allow(clippy::too_many_arguments)]
pub fn editor(
    ui: &mut egui::Ui,
    id: egui::Id,
    size: egui::Vec2,
    click: Option<Arc<Sample>>,
    shown: &KickParams,
    base: KickParams,
    tempo: f64,
    snap: bool,
    actions: &mut Vec<Action>,
) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::BG);
    let shown = engine::kick_effective(shown, tempo);

    // Which curve is edited, chosen with the tabs in the corner.
    let mut curve: Curve = ui.ctx().memory(|m| m.data.get_temp(id.with("tab"))).unwrap_or(Curve::Pitch);
    let mut x = rect.left() + 4.0;
    for (tab, label) in [(Curve::Pitch, "Tono"), (Curve::Amp, "Amp"), (Curve::Drive, "Drive")] {
        let galley = painter.layout_no_wrap(label.into(), theme::small(), theme::TEXT);
        let tab_rect = Rect::from_min_size(pos2(x, rect.top() + 2.0), galley.size() + vec2(8.0, 2.0));
        painter.rect_filled(tab_rect, 2.0, if tab == curve { theme::CONTROL_HOVER } else { theme::SURFACE });
        painter.galley(tab_rect.min + vec2(4.0, 1.0), galley, theme::TEXT);
        if response.clicked() && response.interact_pointer_pos().is_some_and(|p| tab_rect.contains(p)) {
            curve = tab;
            ui.ctx().memory_mut(|m| m.data.insert_temp(id.with("tab"), curve));
        }
        x = tab_rect.right() + 3.0;
    }

    let hit = preview(ui, id, &shown, click, tempo);
    let length = engine::kick_length_ms(&shown);
    let decay_start = shown.attack.max(0.05) + shown.hold;
    // Milliseconds across the editor, and where the drawn shape starts and how long it spans.
    let (span_ms, shape_start, shape_ms) = match curve {
        Curve::Pitch => (shown.pitch_time.max(0.1), 0.0, shown.pitch_time.max(0.1)),
        Curve::Amp => (length, decay_start, shown.decay.max(1.0)),
        Curve::Drive => (length, 0.0, length),
    };
    let x_of_ms = |ms: f32| rect.left() + ms / span_ms * rect.width();
    let ms_of_x = |x: f32| (x - rect.left()) / rect.width() * span_ms;
    let (top, bottom) = (rect.top() + 16.0, rect.bottom() - 4.0);
    let y_of = |v: f32| bottom - v * (bottom - top);
    let x_of_t = |time: f32| x_of_ms(shape_start + time * shape_ms);
    let t_of_x = |x: f32| ((ms_of_x(x) - shape_start) / shape_ms).clamp(0.0, 1.0);

    // Grid: a time step that gives a handful of lines, and quarters of the value range.
    let step_ms = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0].into_iter().find(|s| span_ms / s <= 12.0).unwrap_or(1000.0);
    let big = size.y > 120.0;
    let mut ms = step_ms;
    // Time labels go on top of the curve, at the end.
    let mut time_labels = Vec::new();
    while ms < span_ms {
        let x = x_of_ms(ms);
        painter.line_segment([pos2(x, top), pos2(x, bottom)], Stroke::new(1.0, Color32::from_gray(34)));
        if big {
            time_labels.push((pos2(x + 2.0, bottom - 2.0), format!("{ms:.0}")));
        }
        ms += step_ms;
    }
    for q in 1..4 {
        let y = y_of(q as f32 / 4.0);
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, Color32::from_gray(34)));
    }

    // Waveform of the part of the hit in view.
    let columns = rect.width() as usize;
    let (mid, half) = (rect.center().y + 6.0, (rect.height() - 16.0) * 0.45);
    for x in 0..columns {
        let from = ((x as f32 / columns as f32 * span_ms) * PREVIEW_RATE / 1000.0) as usize;
        let to = (((x + 1) as f32 / columns as f32 * span_ms) * PREVIEW_RATE / 1000.0) as usize;
        let slice = &hit[from.min(hit.len())..to.max(from + 1).min(hit.len())];
        let (lo, hi) = slice.iter().fold((0f32, 0f32), |(lo, hi), &s| (lo.min(s), hi.max(s)));
        let px = rect.left() + x as f32 + 0.5;
        painter.line_segment([pos2(px, mid - hi.clamp(-1.2, 1.2) * half), pos2(px, mid - lo.clamp(-1.2, 1.2) * half)], Stroke::new(1.0, Color32::from_rgb(60, 110, 70)));
    }

    // The curve as it sounds (with the curve knob's tension), then the drawn shape.
    let shape = match curve {
        Curve::Pitch => base.pitch_shape,
        Curve::Amp => base.amp_shape,
        Curve::Drive => base.drive_shape,
    };
    if curve != Curve::Drive {
        let sounding: Vec<_> = (0..=columns)
            .map(|x| {
                let px = rect.left() + x as f32;
                let ms = ms_of_x(px);
                let v = match curve {
                    Curve::Amp => engine::kick_amplitude(&shown, ms),
                    _ => engine::kick_pitch_offset(&shown, ms) / shown.pitch_amount.max(0.001),
                };
                pos2(px, y_of(v))
            })
            .collect();
        painter.add(egui::Shape::line(sounding, Stroke::new(1.0, theme::shade(ACCENT, 0.5))));
    }
    let drawn: Vec<_> = (0..=columns)
        .filter_map(|x| {
            let px = rect.left() + x as f32;
            (ms_of_x(px) >= shape_start).then(|| pos2(px, y_of(shape.value_at(t_of_x(px)))))
        })
        .collect();
    painter.add(egui::Shape::line(drawn, Stroke::new(1.5, ACCENT)));

    let points = shape.points();
    let point_pos = |i: usize| pos2(x_of_t(points[i].time), y_of(points[i].value));
    let bend_pos = |i: usize| {
        let time = (points[i - 1].time + points[i].time) * 0.5;
        pos2(x_of_t(time), y_of(shape.value_at(time)))
    };
    for i in 1..points.len() {
        painter.circle_stroke(bend_pos(i), 2.5, Stroke::new(1.0, theme::shade(ACCENT, 0.8)));
    }
    for (pos, label) in time_labels {
        widgets::backdrop_text(&painter, pos, Align2::LEFT_BOTTOM, label, theme::small(), Color32::from_gray(120));
    }
    let pointer = response.hover_pos();
    for i in 0..points.len() {
        let hovered = pointer.is_some_and(|p| p.distance(point_pos(i)) < 6.0);
        painter.circle_filled(point_pos(i), if hovered { 4.5 } else { 3.5 }, if hovered { Color32::WHITE } else { ACCENT });
    }

    // Editing.
    let snap_t = |x: f32| {
        let t = t_of_x(x);
        if snap {
            let ms = shape_start + t * shape_ms;
            (((ms / step_ms).round() * step_ms - shape_start) / shape_ms).clamp(0.0, 1.0)
        } else {
            t
        }
    };
    let snap_v = |y: f32| {
        let v = ((bottom - y) / (bottom - top)).clamp(0.0, 1.0);
        if snap { (v * 20.0).round() / 20.0 } else { v }
    };
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
                edited.move_point(i, snap_t(pos.x), snap_v(pos.y));
            }
        }
    }
    if response.double_clicked() {
        // The strip with the tabs is not part of the drawing area.
        if let Some(pos) = response.interact_pointer_pos().filter(|p| p.y > top - 4.0) {
            match nearest(pos) {
                Some((false, i)) => edited.remove(i),
                _ if ms_of_x(pos.x) > shape_start => {
                    edited.insert(snap_t(pos.x), snap_v(pos.y));
                }
                _ => {}
            }
        }
    }
    response.context_menu(|ui| {
        if ui.button("Restablecer curva").clicked() {
            edited = if curve == Curve::Drive { Shape::flat() } else { Shape::falling() };
            ui.close();
        }
    });
    response.on_hover_text("Arrastra los puntos · doble clic añade o borra · arrastra el círculo de un tramo para curvarlo · clic derecho para restablecer");
    if edited != shape {
        let params = match curve {
            Curve::Pitch => KickParams { pitch_shape: edited, ..base },
            Curve::Amp => KickParams { amp_shape: edited, ..base },
            Curve::Drive => KickParams { drive_shape: edited, ..base },
        };
        actions.push(Action::Instrument(InstrumentKind::Kick(params)));
    }

    let label = match curve {
        Curve::Pitch => format!("+{:.0} st → {} · {span_ms:.0} ms", shown.pitch_amount, note_label(&shown)),
        Curve::Amp => format!("{length:.0} ms"),
        Curve::Drive => format!("{} · {:.0} %", shown.drive_mode.label(), shown.drive * 100.0),
    };
    painter.text(rect.right_top() + vec2(-4.0, 3.0), Align2::RIGHT_TOP, label, theme::small(), TEXT_DIM);
}

fn note_freq(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

fn note_label(p: &KickParams) -> String {
    if p.key_follow {
        "Key".into()
    } else {
        format!("{} · {:.1} Hz", crate::project::note_name(p.tune.round() as u8), note_freq(p.tune))
    }
}

/// The first 60 ms of the hit, where the attack and the click are.
fn oscilloscope(ui: &mut egui::Ui, hit: &[f32], size: egui::Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::BG);
    painter.line_segment([pos2(rect.left(), rect.center().y), pos2(rect.right(), rect.center().y)], Stroke::new(1.0, Color32::from_gray(40)));
    let frames = ((0.060 * PREVIEW_RATE) as usize).min(hit.len());
    let points: Vec<_> = (0..frames)
        .map(|i| pos2(rect.left() + i as f32 / frames.max(1) as f32 * rect.width(), rect.center().y - hit[i].clamp(-1.2, 1.2) * rect.height() * 0.42))
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.2, theme::PLAY)));
    painter.text(rect.left_top() + vec2(4.0, 3.0), Align2::LEFT_TOP, "Osciloscopio · 60 ms", theme::small(), TEXT_DIM);
    let peak = hit.iter().fold(0f32, |m, s| m.max(s.abs()));
    let db = 20.0 * peak.max(1e-6).log10();
    painter.text(rect.right_top() + vec2(-4.0, 3.0), Align2::RIGHT_TOP, format!("pico {db:+.1} dB"), theme::small(), if db > -0.1 { theme::METER_RED } else { TEXT_DIM });
}

/// Magnitude spectrum (dB) of `hit`, from `PREVIEW_RATE` samples, as (Hz, dB) pairs.
pub(crate) fn spectrum(hit: &[f32]) -> Vec<(f32, f32)> {
    const N: usize = 8192;
    let (mut re, mut im) = (vec![0f32; N], vec![0f32; N]);
    for (i, s) in hit.iter().take(N).enumerate() {
        // Hann-windowed over the hit's own length, so short kicks are not smeared.
        let len = hit.len().min(N);
        re[i] = s * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / len as f32).cos());
    }
    crate::slicing::fft(&mut re, &mut im);
    let loudest = (1..N / 2).map(|k| re[k].hypot(im[k])).fold(1e-9, f32::max);
    (1..N / 2).map(|k| (k as f32 * PREVIEW_RATE / N as f32, 20.0 * (re[k].hypot(im[k]) / loudest).max(1e-6).log10())).collect()
}

/// The strongest frequency below 200 Hz: the kick's fundamental as heard.
pub(crate) fn fundamental(spectrum: &[(f32, f32)]) -> Option<f32> {
    spectrum.iter().filter(|(hz, _)| (20.0..200.0).contains(hz)).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(hz, _)| *hz)
}

fn spectrum_view(ui: &mut egui::Ui, hit: &[f32], p: &KickParams, size: egui::Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::BG);
    let (lo, hi) = (20f32.ln(), 5000f32.ln());
    let x_of = |hz: f32| rect.left() + (hz.max(20.0).ln() - lo) / (hi - lo) * rect.width();
    for hz in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0] {
        let x = x_of(hz);
        painter.line_segment([pos2(x, rect.top() + 14.0), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::from_gray(34)));
    }
    let mut fundamental_label = None;
    let bins = spectrum(hit);
    let mut columns = vec![-90f32; rect.width() as usize + 1];
    for &(hz, db) in &bins {
        if hz <= 5000.0 {
            let column = ((x_of(hz) - rect.left()) as usize).min(columns.len() - 1);
            columns[column] = columns[column].max(db);
        }
    }
    let y_of = |db: f32| rect.top() + 14.0 + (-db / 72.0).clamp(0.0, 1.0) * (rect.height() - 16.0);
    let mut last = None;
    let points: Vec<_> = columns
        .iter()
        .enumerate()
        .filter_map(|(x, &db)| {
            let value = if db > -90.0 { Some(db) } else { last };
            last = value;
            value.map(|db| pos2(rect.left() + x as f32, y_of(db)))
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.2, theme::SOLO)));
    if let Some(hz) = fundamental(&bins) {
        let x = x_of(hz);
        painter.line_segment([pos2(x, rect.top() + 14.0), pos2(x, rect.bottom())], Stroke::new(1.0, ACCENT));
        let note = 69.0 + 12.0 * (hz / 440.0).log2();
        fundamental_label = Some(format!("Fundamental {hz:.0} Hz ≈ {}", crate::project::note_name(note.round().clamp(0.0, 127.0) as u8)));
    }
    if !p.key_follow {
        let x = x_of(note_freq(p.tune));
        painter.circle_filled(pos2(x, rect.top() + 18.0), 3.0, ACCENT);
    }
    // Labels last, over the curve.
    for hz in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0] {
        let label = if hz >= 1000.0 { format!("{}k", hz / 1000.0) } else { format!("{hz}") };
        widgets::backdrop_text(&painter, pos2(x_of(hz) + 2.0, rect.bottom() - 2.0), Align2::LEFT_BOTTOM, label, theme::small(), Color32::from_gray(120));
    }
    if let Some(text) = fundamental_label {
        widgets::backdrop_text(&painter, rect.left_top() + vec2(4.0, 3.0), Align2::LEFT_TOP, text, theme::small(), theme::TEXT);
    }
}

/// A knob for kick parameter `i`, showing automation like every other control.
fn knob(app: &PulseApp, ui: &mut egui::Ui, t: usize, i: usize, actions: &mut Vec<Action>) {
    let kind = app.project.tracks[t].instrument;
    let Some(info) = kind.params().get(i) else { return };
    let (target, id) = (Target::Track(t), Param::Instrument(i));
    if let Some(v) = widgets::param_knob(ui, info, app.display_value(target, id), info.name, 30.0, app.automation_state(target, id)) {
        actions.push(Action::Param(id, v));
    }
}

/// A titled group of controls in the editor window.
fn module(ui: &mut egui::Ui, title: &str, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(theme::SURFACE).corner_radius(3.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(title).color(TEXT_DIM).size(10.0).strong());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                contents(ui);
            });
        });
    });
}

fn beats_label(beats: f32) -> &'static str {
    match beats {
        b if b == 0.25 => "1/16",
        b if b == 0.5 => "1/8",
        b if b == 1.0 => "1/4",
        b if b == 2.0 => "1/2",
        b if b == 4.0 => "1 compás",
        _ => "Libre",
    }
}

/// The full editor, in its own window inside Pulse, for the track in `app.kick_window`.
pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    let Some(t) = app.kick_window else { return };
    let Some(InstrumentKind::Kick(base)) = app.project.tracks.get(t).map(|track| track.instrument) else {
        app.kick_window = None;
        return;
    };
    let mut open = true;
    let mut actions = Vec::new();
    let mut zones: Zones = Vec::new();
    let title = format!("Kick Designer · {}", app.project.tracks[t].name);
    let shown = shown(app, t, base);
    let tempo = app.project.tempo;
    let click = app.project.tracks[t].sample(0).and_then(|path| app.loaded_sample(path)).cloned();
    widgets::editor_window(ctx, title, egui::Id::new(("kick_window", t)), &mut open, pos2(120.0, 90.0), |ui| {
            ui.set_width(930.0);
            let app_ref: &PulseApp = app;
            // Top bar.
            ui.horizontal(|ui| {
                devices::preset_menu(app_ref, ui, t, &mut actions);
                ui.separator();
                ui.label(egui::RichText::new("Duración").color(TEXT_DIM).size(10.5));
                let mut sync = base.sync_beats;
                egui::ComboBox::from_id_salt(("kick_sync", t)).selected_text(beats_label(sync)).width(84.0).show_ui(ui, |ui| {
                    for beats in KICK_SYNC_BEATS {
                        ui.selectable_value(&mut sync, beats, beats_label(beats));
                    }
                });
                if sync != base.sync_beats {
                    actions.push(Action::Instrument(InstrumentKind::Kick(KickParams { sync_beats: sync, ..base })));
                }
                let length = engine::kick_length_ms(&engine::kick_effective(&shown, tempo));
                ui.label(egui::RichText::new(format!("{length:.0} ms")).size(10.5)).on_hover_text("Con duración sincronizada, el Decay se ajusta para durar lo elegido al tempo actual");
                ui.separator();
                let snap_id = ui.id().with(("kick_snap", t));
                let mut snap: bool = ui.ctx().memory(|m| m.data.get_temp(snap_id)).unwrap_or(false);
                if ui.checkbox(&mut snap, "Ajustar a rejilla").changed() {
                    ui.ctx().memory_mut(|m| m.data.insert_temp(snap_id, snap));
                }
                ui.separator();
                if ui.button("▶ Escuchar").clicked() {
                    actions.push(Action::Preview(60));
                }
                if ui.button("Exportar WAV…").on_hover_text("Guarda un golpe como archivo de audio").clicked() {
                    actions.push(Action::ExportKick);
                }
            });
            ui.add_space(4.0);

            // Envelope editor and displays.
            let snap = ui.ctx().memory(|m| m.data.get_temp(ui.id().with(("kick_snap", t)))).unwrap_or(false);
            ui.horizontal_top(|ui| {
                let id = ui.id().with(("kick_big", t));
                editor(ui, id, vec2(610.0, 250.0), click.clone(), &shown, base, tempo, snap, &mut actions);
                ui.vertical(|ui| {
                    let hit = preview(ui, ui.id().with(("kick_display", t)), &engine::kick_effective(&shown, tempo), click.clone(), tempo);
                    oscilloscope(ui, &hit, vec2(300.0, 122.0));
                    spectrum_view(ui, &hit, &shown, vec2(300.0, 122.0));
                });
            });
            ui.add_space(4.0);

            // Modules.
            ui.horizontal_top(|ui| {
                module(ui, "CUERPO", |ui| {
                    ui.vertical(|ui| {
                        let mut wave = base.wave;
                        egui::ComboBox::from_id_salt(("kick_wave", t)).selected_text(wave.label()).width(72.0).show_ui(ui, |ui| {
                            for w in KickWave::ALL {
                                ui.selectable_value(&mut wave, w, w.label());
                            }
                        });
                        let mut follow = base.key_follow;
                        ui.checkbox(&mut follow, "Key").on_hover_text("La nota tocada afina el cuerpo (dos octavas por debajo) en lugar de Tune");
                        if wave != base.wave || follow != base.key_follow {
                            actions.push(Action::Instrument(InstrumentKind::Kick(KickParams { wave, key_follow: follow, ..base })));
                        }
                    });
                    for i in [0, 1, 2, 3, 14, 8] {
                        knob(app_ref, ui, t, i, &mut actions);
                    }
                });
                module(ui, "ENVOLVENTE", |ui| {
                    for i in [4, 5, 6, 7] {
                        knob(app_ref, ui, t, i, &mut actions);
                    }
                });
                module(ui, "SUB", |ui| {
                    for i in [15, 16] {
                        knob(app_ref, ui, t, i, &mut actions);
                    }
                });
            });
            ui.horizontal_top(|ui| {
                module(ui, "CLICK", |ui| {
                    ui.vertical(|ui| {
                        let mut kind = base.click_type;
                        let name = KICK_CLICKS.get(kind as usize).copied().unwrap_or("Noise");
                        let has_sample = app_ref.project.tracks[t].sample(0).is_some();
                        ui.add_enabled_ui(!has_sample, |ui| {
                            egui::ComboBox::from_id_salt(("kick_click", t)).selected_text(name).width(80.0).show_ui(ui, |ui| {
                                for (i, label) in KICK_CLICKS.iter().enumerate() {
                                    ui.selectable_value(&mut kind, i as u8, *label);
                                }
                            });
                        });
                        if kind != base.click_type {
                            actions.push(Action::Instrument(InstrumentKind::Kick(KickParams { click_type: kind, ..base })));
                        }
                        devices::click_slot(app_ref, ui, t, &mut actions, &mut zones);
                    });
                    for i in [9, 10, 11, 17] {
                        knob(app_ref, ui, t, i, &mut actions);
                    }
                });
                module(ui, "DISTORSIÓN", |ui| {
                    ui.vertical(|ui| {
                        let mut mode = base.drive_mode;
                        for m in KickDrive::ALL {
                            ui.selectable_value(&mut mode, m, m.label());
                        }
                        if mode != base.drive_mode {
                            actions.push(Action::Instrument(InstrumentKind::Kick(KickParams { drive_mode: mode, ..base })));
                        }
                    });
                    knob(app_ref, ui, t, 12, &mut actions);
                });
                module(ui, "EQ", |ui| {
                    for i in [18, 19, 20, 21] {
                        knob(app_ref, ui, t, i, &mut actions);
                    }
                });
                module(ui, "SALIDA", |ui| {
                    for i in [22, 23, 13] {
                        knob(app_ref, ui, t, i, &mut actions);
                    }
                    let mut limiter = base.limiter;
                    if ui.checkbox(&mut limiter, "Limitador").on_hover_text("Mantiene el bombo por debajo de -0.3 dBFS").changed() {
                        actions.push(Action::Instrument(InstrumentKind::Kick(KickParams { limiter, ..base })));
                    }
                });
            });
        });
    if !open {
        app.kick_window = None;
    }
    app.sample_drop_zones.extend(zones);
    devices::apply(app, Target::Track(t), actions);
}

/// Renders one hit of the track's kick to a WAV file the user picks.
pub fn export(app: &mut PulseApp, t: usize) {
    let Some(InstrumentKind::Kick(params)) = app.project.tracks.get(t).map(|track| track.instrument) else { return };
    let Some(path) = rfd::FileDialog::new().add_filter("WAV", &["wav"]).set_file_name("Kick.wav").save_file() else { return };
    let click = app.project.tracks[t].sample(0).and_then(|p| app.loaded_sample(p)).cloned();
    let hit = engine::render_kick(params, click, 48_000.0, app.project.tempo);
    app.status = Some(match crate::capture::write_wav(&path, &hit, &hit, 48_000.0) {
        Ok(()) => format!("Bombo exportado: {}", path.display()),
        Err(error) => error,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, PointerButton, Pos2, RawInput};

    /// Runs the editor headless for one frame per event batch and returns the
    /// settings it last asked for, and where the editor was drawn.
    fn run(frames: &[Vec<Event>], params: KickParams, snap: bool) -> (Option<KickParams>, Rect) {
        let ctx = egui::Context::default();
        let mut result = None;
        let mut rect = Rect::NOTHING;
        let mut current = params;
        for (n, events) in frames.iter().enumerate() {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))),
                events: events.clone(),
                time: Some(n as f64 * 0.05),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let mut actions = Vec::new();
                rect = Rect::from_min_size(ui.cursor().min, vec2(260.0, 84.0));
                editor(ui, egui::Id::new("test"), vec2(260.0, 84.0), None, &current, current, 120.0, snap, &mut actions);
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

    fn double_click(target: Pos2) -> Vec<Vec<Event>> {
        let click = vec![Event::PointerMoved(target), press(target, true), press(target, false)];
        vec![vec![Event::PointerMoved(target)], click.clone(), click]
    }

    fn drag(from: Pos2, to: Pos2) -> Vec<Vec<Event>> {
        let mut frames = vec![vec![Event::PointerMoved(from)], vec![press(from, true)]];
        for step in 1..=6 {
            frames.push(vec![Event::PointerMoved(from + (to - from) * step as f32 / 6.0)]);
        }
        frames.push(vec![press(to, false)]);
        frames
    }

    #[test]
    fn double_click_adds_a_point_and_dragging_moves_it() {
        let params = KickParams::default();
        let (_, rect) = run(&[vec![]], params, false);
        let target = at(rect, 0.5, 0.8);
        let (added, _) = run(&double_click(target), params, false);
        let added = added.expect("a double click adds a point");
        assert_eq!(added.pitch_shape.count, 3);
        let point = added.pitch_shape.points[1];
        assert!((point.time - 0.5).abs() < 0.02 && (point.value - 0.8).abs() < 0.03, "{point:?}");

        let to = at(rect, 0.7, 0.3);
        let (moved, _) = run(&drag(target, to), added, false);
        let point = moved.expect("dragging edits the shape").pitch_shape.points[1];
        assert!((point.time - 0.7).abs() < 0.03 && (point.value - 0.3).abs() < 0.05, "{point:?}");
    }

    #[test]
    fn double_clicking_a_point_removes_it() {
        let params = KickParams { pitch_shape: Shape::from_points(&[(0.0, 1.0, 0.0), (0.4, 0.5, 0.0), (1.0, 0.0, 0.0)]), ..Default::default() };
        let (_, rect) = run(&[vec![]], params, false);
        let (removed, _) = run(&double_click(at(rect, 0.4, 0.5)), params, false);
        assert_eq!(removed.expect("removed").pitch_shape.count, 2);
    }

    #[test]
    fn dragging_a_segment_handle_up_bends_it_up() {
        let params = KickParams::default();
        let (_, rect) = run(&[vec![]], params, false);
        let handle = at(rect, 0.5, 0.5);
        let (bent, _) = run(&drag(handle, handle - vec2(0.0, 20.0)), params, false);
        let shape = bent.expect("bent").pitch_shape;
        assert_eq!(shape.count, 2, "no point was added");
        assert!(shape.value_at(0.5) > 0.6, "the middle rose: {}", shape.value_at(0.5));
    }

    #[test]
    fn snapping_puts_new_points_on_the_grid() {
        // The pitch sweep spans 60 ms: grid lines every 5 ms, values in steps of 0.05.
        let params = KickParams::default();
        let (_, rect) = run(&[vec![]], params, true);
        let (added, _) = run(&double_click(at(rect, 0.37, 0.62)), params, true);
        let point = added.expect("added").pitch_shape.points[1];
        let ms = point.time * 60.0;
        assert!((ms / 5.0 - (ms / 5.0).round()).abs() < 1e-3, "{ms} ms is on the grid");
        assert!((point.value * 20.0 - (point.value * 20.0).round()).abs() < 1e-3, "{} is on the grid", point.value);
    }

    #[test]
    fn the_spectrum_finds_the_tuned_fundamental() {
        for tune in [28.0, 31.0, 36.0] {
            let params = KickParams { tune, click: 0.0, drive: 0.0, hold: 300.0, decay: 600.0, ..Default::default() };
            let hit = engine::render_kick(params, None, PREVIEW_RATE, 120.0);
            let found = fundamental(&spectrum(&hit)).unwrap();
            let expected = note_freq(tune);
            assert!((found / expected - 1.0).abs() < 0.06, "tune {tune}: {found} Hz vs {expected} Hz");
        }
    }
}
