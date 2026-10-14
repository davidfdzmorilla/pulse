//! Clip view for audio clips: waveform, warp settings, start, length, gain and transposition.

use crate::app::{ClipRef, PulseApp};
use crate::theme::{self, ACCENT, TEXT_DIM};
use crate::widgets::{self, Knob};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use crate::slicing::SliceMode;
use engine::model::{Clip, WarpMode};

fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(TEXT_DIM).size(10.5));
}

/// `playhead` is the position inside the clip in beats, when it is playing.
pub fn show(app: &mut PulseApp, ui: &mut egui::Ui, clip_ref: ClipRef, playhead: Option<f64>) {
    let tempo = app.project.tempo;
    let Some(Clip::Audio(before)) = app.clip(clip_ref).cloned() else { return };
    let sample = app.sample(&before.sample);
    let mut clip = before.clone();
    let mut slice = false;

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
                ui.add_enabled(enabled, egui::DragValue::new(&mut clip.original_bpm).range(20.0..=400.0).speed(0.1).fixed_decimals(2));
                if ui.add_enabled(enabled, egui::Button::new("÷2")).clicked() {
                    clip.original_bpm /= 2.0;
                }
                if ui.add_enabled(enabled, egui::Button::new("×2")).clicked() {
                    clip.original_bpm *= 2.0;
                }
            });
            ui.horizontal(|ui| {
                label(ui, "Longitud");
                ui.add(egui::DragValue::new(&mut clip.length).range(0.25..=512.0).speed(0.25).suffix(" beats"));
                if ui.button("Ajustar al archivo").on_hover_text("Longitud = lo que dura el archivo desde el inicio").clicked() {
                    let reference = if clip.warp == WarpMode::Off { tempo } else { clip.original_bpm };
                    let beats = (sample.duration() - clip.offset).max(0.0) * reference / 60.0;
                    clip.length = ((beats * 4.0).round() / 4.0).max(0.25);
                }
            });
            ui.horizontal(|ui| {
                label(ui, "Inicio");
                ui.add(egui::DragValue::new(&mut clip.offset).range(0.0..=sample.duration().max(0.0)).speed(0.005).suffix(" s").fixed_decimals(3));
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
        let height = ui.available_height().clamp(80.0, 200.0);
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, theme::SLOT);
        let duration = sample.duration();
        if duration <= 0.0 {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Muestra no encontrada", theme::body(), TEXT_DIM);
            return;
        }
        let reference = if clip.warp == WarpMode::Off { tempo } else { clip.original_bpm.max(1.0) };
        let x_of = |seconds: f64| rect.left() + (seconds / duration) as f32 * rect.width();
        let region_end = (clip.offset + clip.length * 60.0 / reference).min(duration);
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
        painter.line_segment([pos2(region.left(), rect.top()), pos2(region.left(), rect.bottom())], Stroke::new(2.0, ACCENT));
        widgets::play_triangle(&painter, pos2(region.left() + 5.0, rect.top() + 7.0), 8.0, ACCENT);
        if let Some(beats) = playhead {
            let x = x_of(clip.offset + beats * 60.0 / reference);
            painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.5, Color32::WHITE));
        }
        if let Some(pos) = response.interact_pointer_pos() {
            if response.clicked() || response.dragged() {
                // Clicking the waveform moves the clip start there.
                clip.offset = (((pos.x - rect.left()) / rect.width()) as f64 * duration).clamp(0.0, duration);
            }
        }
        response.on_hover_text("Clic o arrastra para mover el inicio del clip");
    });

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
}
