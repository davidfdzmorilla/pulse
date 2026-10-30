//! Clip view for audio clips: waveform, warp settings, start, length, gain and transposition.

use crate::app::{ClipRef, PulseApp};
use crate::theme::{self, ACCENT, TEXT_DIM};
use crate::widgets::{self, Knob};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use crate::slicing::SliceMode;
use engine::model::{AudioClip, Clip, WarpMarker, WarpMode};

const ENVELOPE_H: f32 = 60.0;
/// Warp markers on the waveform, and the strip below it where they're handled.
pub(crate) const MARKER: Color32 = Color32::from_rgb(240, 200, 80);
const MARKER_H: f32 = 14.0;
/// Colour of the slicing cuts.
const CUT: Color32 = Color32::from_rgb(90, 200, 255);

/// Changes the clip's warp markers with `change`, keeping the clip starting on the
/// same beat of the file (its start moves through the file instead), so the rest
/// of the clip stays on the project's bars.
fn keep_start(clip: &mut AudioClip, change: impl FnOnce(&mut AudioClip)) {
    let start = clip.file_beat(clip.offset);
    change(clip);
    clip.offset = clip.file_seconds(start);
}

/// A warp marker at `seconds` of the file, on the beat it plays on now.
fn add_marker(clip: &mut AudioClip, seconds: f64) {
    let beat = clip.file_beat(seconds);
    let m = &clip.warp_markers;
    let i = m.partition_point(|x| x.seconds < seconds);
    // Not on top of another one.
    let close = |j: usize| m.get(j).is_some_and(|x: &WarpMarker| (x.seconds - seconds).abs() < 0.01 || (x.beat - beat).abs() < 0.01);
    if close(i) || (i > 0 && close(i - 1)) {
        return;
    }
    clip.warp_markers.insert(i, WarpMarker { beat, seconds });
}

fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(TEXT_DIM).size(10.5));
}

/// `playhead` is the position inside the clip in beats, when it is playing.
/// `envelope`: the clip envelope the lane at the bottom shows (its parameter, what
/// it is and its own value), if not the velocities (audio clips have none).
pub fn show(app: &mut PulseApp, ui: &mut egui::Ui, clip_ref: ClipRef, playhead: Option<f64>, envelope: Option<(engine::model::Param, engine::model::ParamInfo, f32)>) {
    let tempo = app.project.tempo;
    let Some(Clip::Audio(before)) = app.clip(clip_ref).cloned() else { return };
    let sample = app.sample(&before.sample);
    let mut clip = before.clone();
    let mut slice = false;
    let mut separate = false;
    let (mut detect, mut use_tempo) = (false, false);
    let (mut detect_key, mut follow_to) = (false, None);
    let project_key = app.project.key;
    // Where it will be sliced, while the cuts are shown to edit them.
    let mut cuts = if app.show_cuts { app.cuts_for(clip_ref) } else { None };
    let cuts_before = cuts.clone();

    ui.horizontal(|ui| {
        let (swatch, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        ui.painter().rect_filled(swatch, 2.0, theme::rgb(clip.color));
        ui.add(egui::TextEdit::singleline(&mut clip.name).desired_width(130.0));
        let file = std::path::Path::new(&clip.sample).file_name().map_or(String::new(), |f| f.to_string_lossy().into_owned());
        let details = if sample.frames() > 0 {
            format!("{file} · {:.2} s · {:.0} Hz", sample.duration(), sample.sample_rate)
        } else {
            format!("{file} · no encontrado")
        };
        ui.label(egui::RichText::new(details).color(TEXT_DIM).size(10.5))
            .on_hover_text(&clip.sample);
    });
    ui.add_space(4.0);

    ui.horizontal_top(|ui| {
        // Settings column.
        ui.vertical(|ui| {
            ui.set_width(250.0);
            ui.horizontal(|ui| {
                label(ui, "Warp");
                for mode in WarpMode::ALL {
                    ui.selectable_value(&mut clip.warp, mode, mode.label());
                }
            });
            ui.horizontal(|ui| {
                label(ui, "BPM original");
                let enabled = clip.warp != WarpMode::Off;
                // Through set_original_bpm, so warp markers keep their points.
                let mut bpm = clip.original_bpm;
                ui.add_enabled(enabled, egui::DragValue::new(&mut bpm).range(20.0..=400.0).speed(0.1).fixed_decimals(2));
                if ui.add_enabled(enabled, egui::Button::new("÷2")).clicked() {
                    bpm /= 2.0;
                }
                if ui.add_enabled(enabled, egui::Button::new("×2")).clicked() {
                    bpm *= 2.0;
                }
                if bpm != clip.original_bpm {
                    clip.set_original_bpm(bpm);
                }
            });
            ui.horizontal(|ui| {
                label(ui, "Longitud");
                ui.add(egui::DragValue::new(&mut clip.length).range(0.25..=512.0).speed(0.25).suffix(" beats"));
                if ui.button("Ajustar al archivo").on_hover_text("Longitud = lo que dura el archivo desde el inicio").clicked() {
                    let beats = clip.beats_until(sample.duration(), tempo).max(0.0);
                    clip.length = ((beats * 4.0).round() / 4.0).max(0.25);
                }
            });
            ui.horizontal(|ui| {
                label(ui, "Ver");
                ui.selectable_value(&mut app.sample_view_whole, false, "El clip").on_hover_text("Solo lo que suena el clip");
                ui.selectable_value(&mut app.sample_view_whole, true, "Archivo entero").on_hover_text("Todo el audio, con el trozo del clip resaltado");
            });
            ui.horizontal(|ui| {
                label(ui, "Inicio");
                // Below 0: the clip starts before the file (a bar line before its first downbeat).
                ui.add(egui::DragValue::new(&mut clip.offset).range(-8.0..=sample.duration().max(0.0)).speed(0.005).suffix(" s").fixed_decimals(3));
            });
            ui.horizontal(|ui| {
                label(ui, "Fundidos");
                // In the Arrangement also with the handles at the clip's top corners.
                ui.add(egui::DragValue::new(&mut clip.fades.fade_in).range(0.0..=64.0).speed(0.05).fixed_decimals(2).prefix("entrada ").suffix(" beats"))
                    .on_hover_text("Cuánto tarda en entrar desde el silencio");
                ui.add(egui::DragValue::new(&mut clip.fades.fade_out).range(0.0..=64.0).speed(0.05).fixed_decimals(2).prefix("salida ").suffix(" beats"))
                    .on_hover_text("Cuánto tarda en irse al silencio al final");
            });
            ui.horizontal(|ui| {
                label(ui, "Marcadores");
                let count = clip.warp_markers.len();
                ui.label(egui::RichText::new(if count == 0 { "ninguno".to_string() } else { count.to_string() }).size(10.5));
                if ui
                    .add_enabled(count > 0, egui::Button::new("Quitar"))
                    .on_hover_text("El clip vuelve a un solo tempo, el BPM original")
                    .clicked()
                {
                    clip.warp_markers.clear();
                }
            });
            ui.horizontal(|ui| {
                label(ui, "Trocear");
                egui::ComboBox::from_id_salt("slice_mode")
                    .selected_text(app.slice_mode.label())
                    .width(92.0)
                    .show_ui(ui, |ui| {
                        for mode in SliceMode::ALL {
                            ui.selectable_value(&mut app.slice_mode, mode, mode.label());
                        }
                    });
                slice = ui
                    .button("A Drum Pads")
                    .on_hover_text("Corta la región del clip en golpes, uno por pad, en una pista MIDI nueva con un clip que los toca en orden")
                    .clicked();
            });
            ui.horizontal(|ui| {
                label(ui, "");
                ui.checkbox(&mut app.show_cuts, "Editar cortes").on_hover_text("Muestra dónde se cortará, bajo la forma de onda: arrastra un corte para moverlo, doble clic para añadir o quitar uno (hasta 16, uno por pad)");
            });
            ui.horizontal(|ui| {
                label(ui, "Tempo");
                if ui.add_enabled(!app.tempo.busy(), egui::Button::new("Detectar")).on_hover_text("Busca el tempo y los compases del audio para que siga el del proyecto").clicked() {
                    detect = true;
                }
                if ui.add_enabled(clip.warp != WarpMode::Off, egui::Button::new("Usar en el proyecto")).on_hover_text("Pone el proyecto al tempo de este clip").clicked() {
                    use_tempo = true;
                }
            });
            ui.horizontal(|ui| {
                label(ui, "Tonalidad");
                match clip.key {
                    Some(k) => {
                        let own = stems::key::Key { tonic: k.tonic, minor: k.minor };
                        ui.label(egui::RichText::new(own.name()).size(10.5));
                        let mut follow = k.follow;
                        let hint = match project_key {
                            Some(p) => format!("Transpone el clip a la escala del proyecto ({}) por el camino más corto, y lo sigue si cambia", p.name()),
                            None => "Transpone el clip a la escala del proyecto cuando la elijas (barra de transporte)".to_string(),
                        };
                        if ui.checkbox(&mut follow, "Seguir la escala").on_hover_text(hint).changed() {
                            follow_to = Some(follow);
                        }
                    }
                    None => {
                        ui.label(egui::RichText::new("sin detectar").color(TEXT_DIM).size(10.5));
                    }
                }
                if ui.button("Detectar").on_hover_text("Busca la tonalidad del audio (o la lee de su nombre, como «Pad_Am»)").clicked() {
                    detect_key = true;
                }
            });
            ui.horizontal(|ui| {
                label(ui, "Afinación");
                let tuned = app.tuning_of(clip_ref).is_some();
                if ui.button(if tuned { "Editar notas… ✓" } else { "Editar notas…" }).on_hover_text("Las notas de la voz sobre el teclado: arrastra cada una a otra nota, afínala o quítale el vibrato (como Melodyne)").clicked() {
                    app.open_tuning(clip_ref);
                }
            });
            ui.horizontal(|ui| {
                label(ui, "Stems");
                separate = ui
                    .add_enabled(app.stems.is_none(), egui::Button::new("Separar en stems"))
                    .on_hover_text("Batería, bajo, resto y voz, cada uno en una pista de audio nueva bajo esta; esta queda desactivada")
                    .clicked();
            });
            ui.label(
                egui::RichText::new(match clip.warp {
                    WarpMode::Off => "Off: suena a su velocidad original, sin seguir el tempo.",
                    WarpMode::Repitch => "Re-Pitch: sigue el tempo cambiando la velocidad y el tono.",
                    WarpMode::Stretch => "Stretch: sigue el tempo manteniendo el tono.",
                    WarpMode::Beats => "Beats: para baterías. Cada golpe empieza donde le toca y suena a su velocidad, sin eco.",
                })
                .color(TEXT_DIM)
                .size(10.5),
            );
        });

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            Knob::new(&mut clip.gain_db, -36.0..=12.0, 0.0, "Gain").format(|v| format!("{v:+.1} dB")).show(ui);
            ui.add_enabled_ui(clip.warp == WarpMode::Stretch, |ui| {
                Knob::new(&mut clip.transpose, -24.0..=24.0, 0.0, "Transp.")
                    .bipolar()
                    .format(|v| format!("{:+.0} st", v.round()))
                    .show(ui)
                    .on_disabled_hover_text("La transposición se aplica en modo Stretch");
            });
            clip.transpose = clip.transpose.round();
            // Moved by hand, it no longer follows the project's key.
            if clip.transpose != before.transpose {
                if let Some(k) = clip.key.as_mut() {
                    k.follow = false;
                }
            }
        });
        ui.add_space(8.0);

        // The waveform: the clip's part of the file (what it plays), or the whole
        // file with that part highlighted.
        let width = ui.available_width().max(200.0);
        // Room below for the clip's envelopes.
        let height = (ui.available_height() - ENVELOPE_H - 30.0).clamp(80.0, 200.0);
        let strips = if cuts.is_some() { 2.0 } else { 1.0 };
        let (rect, response) = ui.allocate_exact_size(vec2(width, height - MARKER_H * strips), Sense::click_and_drag());
        // Below it, the warp markers' strip, and the cuts' one when they are shown.
        let strip = Rect::from_min_size(rect.left_bottom(), vec2(width, MARKER_H));
        let strip_response = ui.interact(strip, response.id.with("markers"), Sense::click_and_drag());
        let cut_strip = Rect::from_min_size(strip.left_bottom(), vec2(width, MARKER_H));
        let cut_response = cuts.is_some().then(|| ui.interact(cut_strip, response.id.with("cuts"), Sense::click_and_drag()));
        let painter = ui.painter_at(rect.union(strip).union(if cuts.is_some() { cut_strip } else { strip }));
        painter.rect_filled(strip, 0.0, theme::SURFACE);
        if cuts.is_some() {
            painter.rect_filled(cut_strip, 0.0, theme::shade(theme::SURFACE, 0.85));
        }
        painter.rect_filled(rect, 2.0, theme::SLOT);
        let duration = sample.duration();
        if duration <= 0.0 {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Muestra no encontrada", theme::body(), TEXT_DIM);
            return;
        }
        let whole = app.sample_view_whole;
        let region_end = clip.source_seconds(clip.length, tempo);
        let shown = if whole { (clip.offset.min(0.0), duration.max(region_end)) } else { (clip.offset, region_end.max(clip.offset + 0.01)) };
        // While a marker is dragged the view stays put (the part changes under it).
        let view_id = response.id.with("view");
        let (v0, v1) = if strip_response.dragged() || cut_response.as_ref().is_some_and(|r| r.dragged()) {
            ui.data(|d| d.get_temp(view_id)).unwrap_or(shown)
        } else {
            ui.data_mut(|d| d.insert_temp(view_id, shown));
            shown
        };
        let x_of = |seconds: f64| rect.left() + ((seconds - v0) / (v1 - v0)) as f32 * rect.width();
        let seconds_at = |x: f32| v0 + ((x - rect.left()) / rect.width()) as f64 * (v1 - v0);
        let region = Rect::from_min_max(pos2(x_of(clip.offset).max(rect.left()), rect.top()), pos2(x_of(region_end.min(duration)).min(rect.right()), rect.bottom()));
        painter.rect_filled(region, 0.0, theme::shade(theme::rgb(clip.color), 0.35));

        let (mid, half) = (rect.center().y, rect.height() * 0.45);
        let block = |seconds: f64| (seconds.max(0.0) * sample.sample_rate as f64) as usize / engine::PEAK_BLOCK;
        for px in 0..rect.width() as usize {
            let (from, to) = (seconds_at(rect.left() + px as f32), seconds_at(rect.left() + px as f32 + 1.0));
            // Before the file (a clip starting on the bar line before it) or after it: nothing.
            if to <= 0.0 || from >= duration {
                continue;
            }
            let a = block(from).min(sample.peaks.len().saturating_sub(1));
            let b = block(to).max(a + 1).min(sample.peaks.len());
            let (lo, hi) = sample.peaks[a..b]
                .iter()
                .fold((0f32, 0f32), |(lo, hi), &(l, h)| (lo.min(l), hi.max(h)));
            let x = rect.left() + px as f32 + 0.5;
            let inside = x >= region.left() && x <= region.right();
            let color = if inside { theme::rgb(clip.color) } else { Color32::from_gray(110) };
            painter.line_segment([pos2(x, mid - hi * half), pos2(x, mid - lo * half)], Stroke::new(1.0, color));
        }
        // The clip's bars, where they fall in the file (to check they're on the music's).
        if clip.warp != WarpMode::Off {
            let mut bar = 4.0;
            while bar < clip.length {
                let x = x_of(clip.source_seconds(bar, tempo));
                painter.line_segment([pos2(x, rect.top()), pos2(x, rect.top() + 8.0)], Stroke::new(1.0, Color32::from_gray(170)));
                bar += 4.0;
            }
        }
        painter.line_segment([pos2(region.left(), rect.top()), pos2(region.left(), rect.bottom())], Stroke::new(2.0, ACCENT));
        widgets::play_triangle(&painter, pos2(region.left() + 5.0, rect.top() + 7.0), 8.0, ACCENT);
        let warped = clip.warp != WarpMode::Off;
        if warped {
            for m in &clip.warp_markers {
                let x = x_of(m.seconds);
                painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, MARKER));
                painter.add(egui::Shape::convex_polygon(
                    vec![pos2(x - 4.0, strip.bottom() - 2.0), pos2(x + 4.0, strip.bottom() - 2.0), pos2(x, strip.top() + 2.0)],
                    MARKER,
                    Stroke::NONE,
                ));
            }
            if clip.warp_markers.is_empty() {
                painter.text(strip.center(), Align2::CENTER_CENTER, "Doble clic aquí: marcador de warp", theme::small(), TEXT_DIM);
            }
        }
        // The cuts: where each pad's slice begins, numbered.
        if let Some(cuts) = &cuts {
            for (i, &seconds) in cuts.iter().enumerate() {
                let x = x_of(seconds);
                painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, CUT));
                painter.add(egui::Shape::convex_polygon(vec![pos2(x - 4.0, cut_strip.top() + 2.0), pos2(x + 4.0, cut_strip.top() + 2.0), pos2(x, cut_strip.bottom() - 2.0)], CUT, Stroke::NONE));
                painter.text(pos2(x + 3.0, rect.top() + 2.0), Align2::LEFT_TOP, (i + 1).to_string(), theme::small(), CUT);
            }
            if cuts.is_empty() {
                painter.text(cut_strip.center(), Align2::CENTER_CENTER, "Doble clic aquí: un corte", theme::small(), TEXT_DIM);
            }
        }
        if let Some(beats) = playhead {
            let x = x_of(clip.source_seconds(beats, tempo));
            painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.5, Color32::WHITE));
        }
        // The marker under the pointer, if any (within a few pixels).
        let near = |pos: egui::Pos2, clip: &engine::model::AudioClip| {
            if !warped {
                return None;
            }
            clip.warp_markers
                .iter()
                .enumerate()
                .map(|(i, m)| (i, (x_of(m.seconds) - pos.x).abs()))
                .filter(|&(_, d)| d < 5.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        };
        let drag_id = strip_response.id.with("dragging");
        if strip_response.drag_started() {
            // Where the button went down: by the time a drag starts the pointer has moved.
            let origin = ui.input(|i| i.pointer.press_origin()).or(strip_response.interact_pointer_pos());
            let grabbed = origin.and_then(|pos| near(pos, &clip));
            ui.data_mut(|d| d.insert_temp(drag_id, grabbed));
        }
        let dragging: Option<usize> = ui.data(|d| d.get_temp(drag_id)).flatten();
        if let (Some(pos), true) = (strip_response.interact_pointer_pos(), warped) {
            if strip_response.double_clicked() {
                // Double clic: a marker where it is (none before: the clip's start and
                // end too, so only this point moves), or away with the one there.
                keep_start(&mut clip, |clip| match near(pos, clip) {
                    Some(i) => {
                        clip.warp_markers.remove(i);
                        if clip.warp_markers.len() < 2 {
                            clip.warp_markers.clear();
                        }
                    }
                    None => {
                        let mut points = vec![seconds_at(pos.x)];
                        if clip.warp_markers.is_empty() {
                            points.extend([clip.offset.max(0.0), region_end]);
                        }
                        for seconds in points {
                            add_marker(clip, seconds);
                        }
                    }
                });
            } else if let (Some(i), true) = (dragging, strip_response.dragged()) {
                // Dragging a marker: that point of the file plays on its beat, the
                // stretches on both sides speed up or slow down.
                keep_start(&mut clip, |clip| {
                    let m = &clip.warp_markers;
                    let lo = if i > 0 { m[i - 1].seconds + 0.01 } else { f64::MIN };
                    let hi = m.get(i + 1).map_or(f64::MAX, |n| n.seconds - 0.01);
                    clip.warp_markers[i].seconds = seconds_at(pos.x).clamp(lo, hi.max(lo));
                });
            }
        }
        // Editing the cuts: drag one, or double clic to add or remove one.
        if let (Some(cut_response), Some(list)) = (&cut_response, cuts.as_mut()) {
            let near = |pos: egui::Pos2, list: &[f64]| list.iter().enumerate().map(|(i, &s)| (i, (x_of(s) - pos.x).abs())).filter(|&(_, d)| d < 5.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i);
            let grab_id = cut_response.id.with("grabbed");
            if cut_response.drag_started() {
                let origin = ui.input(|i| i.pointer.press_origin()).or(cut_response.interact_pointer_pos());
                let grabbed = origin.and_then(|pos| near(pos, list));
                ui.data_mut(|d| d.insert_temp(grab_id, grabbed));
            }
            let grabbed: Option<usize> = ui.data(|d| d.get_temp(grab_id)).flatten();
            let (start, end) = (clip.offset.max(0.0), region_end.min(duration));
            if let Some(pos) = cut_response.interact_pointer_pos() {
                if cut_response.double_clicked() {
                    match near(pos, list) {
                        Some(i) => {
                            list.remove(i);
                        }
                        None if list.len() >= engine::model::RACK_PADS => app.status = Some(format!("Hasta {} cortes: uno por pad del Drum Pads", engine::model::RACK_PADS)),
                        None => list.push(seconds_at(pos.x).clamp(start, end)),
                    }
                } else if let (Some(i), true) = (grabbed, cut_response.dragged()) {
                    let lo = if i > 0 { list[i - 1] + 0.005 } else { start };
                    let hi = list.get(i + 1).map_or(end, |n| n - 0.005);
                    list[i] = seconds_at(pos.x).clamp(lo, hi.max(lo));
                }
            }
            if cut_response.hover_pos().is_some_and(|pos| near(pos, list).is_some()) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            cut_response.clone().on_hover_text("Cortes: cada uno empieza el trozo de un pad · arrastrar: moverlo · doble clic: añadir o quitar uno");
        }
        if let Some(pos) = response.interact_pointer_pos() {
            if whole && (response.clicked() || response.dragged()) {
                // Seeing the whole file, a clic moves the clip's start there.
                clip.offset = seconds_at(pos.x).clamp(0.0, duration);
            }
        }
        if let Some(pos) = strip_response.hover_pos() {
            if near(pos, &clip).is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
        if warped {
            strip_response.on_hover_text("Marcadores de warp · doble clic: añadir uno o quitarlo · arrastrar: que ese punto del audio caiga en su tiempo");
        }
        response.on_hover_text(if whole { "Clic o arrastra para mover el inicio del clip" } else { "Lo que suena el clip. «Archivo entero» muestra el resto del audio (y ahí un clic mueve el inicio)" });
    });

    // The clip's envelopes, across the clip's length.
    ui.add_space(4.0);
    let choice = &mut app.piano_roll.lane;
    crate::clip_envelope::header(ui, choice, &mut clip.envelopes, None);
    let (lane, _) = ui.allocate_exact_size(vec2(ui.available_width(), ENVELOPE_H), Sense::hover());
    match envelope {
        Some((param, info, value)) => {
            let (left, beat_w) = (lane.left() + 8.0, (lane.width() - 16.0) / clip.length.max(0.25) as f32);
            let x_of = |beat: f64| left + beat as f32 * beat_w;
            let beat_of = |x: f32| ((x - left) / beat_w) as f64;
            let length = clip.length;
            crate::clip_envelope::lane(ui, choice, &mut clip.envelopes, length, param, &info, value, lane, left, &x_of, &beat_of, 0.25, theme::METER_RED)
                .on_hover_text("Clic: añadir un punto · arrastrar: moverlo · doble clic: borrarlo");
        }
        None => {
            ui.painter().rect_filled(lane, 2.0, theme::SLOT);
            ui.painter().text(lane.center(), Align2::CENTER_CENTER, "Elige arriba un parámetro para ver o dibujar su envolvente en el clip", theme::small(), TEXT_DIM);
        }
    }

    if clip != before {
        let new = Clip::Audio(clip);
        match clip_ref {
            ClipRef::Session { track, slot } => app.project.tracks[track].clips[slot] = Some(new),
            ClipRef::Arrangement { track, index } => app.project.tracks[track].arrangement[index].clip = new,
        }
        app.sync_clip_ref(clip_ref);
    }
    if cuts != cuts_before {
        if let Some(cuts) = cuts {
            app.set_cuts(clip_ref, cuts);
        }
    }
    if slice {
        app.slice_clip(clip_ref);
    }
    if separate {
        app.separate_stems(clip_ref);
    }
    if detect {
        app.detect_tempo(vec![clip_ref], true);
    }
    if let Some(follow) = follow_to {
        app.set_follow_key(clip_ref, follow);
    }
    if detect_key {
        app.detect_clip_key(clip_ref);
    }
    if use_tempo {
        app.use_clip_tempo(clip_ref);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip() -> AudioClip {
        AudioClip { hard_start: false, fades: Default::default(),
            name: "x".into(),
            color: [0; 3],
            sample: "/x.wav".into(),
            length: 16.0,
            offset: 0.25,
            warp: WarpMode::Stretch,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(), muted: false,
        }
    }

    #[test]
    fn adding_markers_changes_nothing_until_one_is_moved() {
        let mut c = clip();
        let before: Vec<f64> = (0..16).map(|b| c.source_seconds(b as f64, 100.0)).collect();
        // The first double clic: that point, and the clip's start and end.
        keep_start(&mut c, |c| {
            for seconds in [3.0, c.offset, c.source_seconds(16.0, 100.0)] {
                add_marker(c, seconds);
            }
        });
        assert_eq!(c.warp_markers.len(), 3);
        assert!(c.warp_markers.windows(2).all(|w| w[0].seconds < w[1].seconds));
        for (b, s) in before.iter().enumerate() {
            assert!((c.source_seconds(b as f64, 100.0) - s).abs() < 1e-9);
        }
        // Not twice at one point.
        add_marker(&mut c, 3.0);
        assert_eq!(c.warp_markers.len(), 3);
        // Dragged from 3 s to 3.5 s: that point now plays on the same beat, the
        // clip's start and end stay where they were.
        let beat = c.warp_markers[1].beat - c.file_beat(c.offset);
        keep_start(&mut c, |c| c.warp_markers[1].seconds = 3.5);
        assert!((c.source_seconds(beat, 100.0) - 3.5).abs() < 1e-9);
        assert!((c.offset - 0.25).abs() < 1e-9);
        assert!((c.source_seconds(16.0, 100.0) - before[0] - 8.0).abs() < 1e-9);
    }
}
