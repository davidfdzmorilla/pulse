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
                label(ui, "Inicio");
                // Below 0: the clip starts before the file (a bar line before its first downbeat).
                ui.add(egui::DragValue::new(&mut clip.offset).range(-8.0..=sample.duration().max(0.0)).speed(0.005).suffix(" s").fixed_decimals(3));
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
                    .button("A Drum Rack")
                    .on_hover_text("Corta la región del clip en golpes, uno por pad, en una pista MIDI nueva con un clip que los toca en orden")
                    .clicked();
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
        });
        ui.add_space(8.0);

        // Waveform of the whole file with the clip's region highlighted.
        let width = ui.available_width().max(200.0);
        // Room below for the clip's envelopes.
        let height = (ui.available_height() - ENVELOPE_H - 30.0).clamp(80.0, 200.0);
        let (rect, response) = ui.allocate_exact_size(vec2(width, height - MARKER_H), Sense::click_and_drag());
        // Below it, the warp markers' strip.
        let strip = Rect::from_min_size(rect.left_bottom(), vec2(width, MARKER_H));
        let strip_response = ui.interact(strip, response.id.with("markers"), Sense::click_and_drag());
        let painter = ui.painter_at(rect.union(strip));
        painter.rect_filled(strip, 0.0, theme::SURFACE);
        painter.rect_filled(rect, 2.0, theme::SLOT);
        let duration = sample.duration();
        if duration <= 0.0 {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Muestra no encontrada", theme::body(), TEXT_DIM);
            return;
        }
        let x_of = |seconds: f64| rect.left() + (seconds / duration) as f32 * rect.width();
        let seconds_at = |x: f32| ((x - rect.left()) / rect.width()) as f64 * duration;
        let region_end = clip.source_seconds(clip.length, tempo).min(duration);
        let region = Rect::from_min_max(pos2(x_of(clip.offset), rect.top()), pos2(x_of(region_end), rect.bottom()));
        painter.rect_filled(region, 0.0, theme::shade(theme::rgb(clip.color), 0.35));

        let blocks = sample.peaks.len().max(1) as f32;
        let (mid, half) = (rect.center().y, rect.height() * 0.45);
        for px in 0..rect.width() as usize {
            let a = (px as f32 / rect.width() * blocks) as usize;
            let b = (((px + 1) as f32 / rect.width() * blocks) as usize).max(a + 1).min(sample.peaks.len());
            let (lo, hi) = sample.peaks[a.min(sample.peaks.len() - 1)..b]
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
        if let Some(pos) = response.interact_pointer_pos() {
            if response.clicked() || response.dragged() {
                // Clicking the waveform moves the clip start there.
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
        response.on_hover_text("Clic o arrastra para mover el inicio del clip");
    });

    // The clip's envelopes, across the clip's length.
    ui.add_space(4.0);
    let choice = &mut app.piano_roll.lane;
    crate::clip_envelope::header(ui, choice, &mut clip.envelopes);
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
    if slice {
        app.slice_clip(clip_ref);
    }
    if separate {
        app.separate_stems(clip_ref);
    }
    if detect {
        app.detect_tempo(vec![clip_ref], true);
    }
    if use_tempo {
        app.use_clip_tempo(clip_ref);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip() -> AudioClip {
        AudioClip {
            name: "x".into(),
            color: [0; 3],
            sample: "/x.wav".into(),
            length: 16.0,
            offset: 0.25,
            warp: WarpMode::Stretch,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(),
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
