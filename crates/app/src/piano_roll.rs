//! MIDI note editor for the selected clip.
//!
//! Click an empty cell to add a note (drag right to lengthen it), click a note
//! to delete it, click a key to audition it.

use crate::app::PulseApp;
use crate::project::{drum_name, is_black_key, note_name};
use crate::session::color;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind};
use engine::model::{drum_note, Note};
use engine::Command;
use std::sync::atomic::Ordering;

const KEY_W: f32 = 70.0;
const STEP: f64 = 0.25;

const NOTE_LENGTHS: [(f64, &str); 5] = [
    (0.25, "1/16"),
    (0.5, "1/8"),
    (1.0, "1/4"),
    (2.0, "1/2"),
    (4.0, "1 compás"),
];

pub struct State {
    note_length: f64,
    zoom: f32,
    /// Index of the note being lengthened by the current drag.
    dragging: Option<usize>,
    /// Clip whose initial scroll position has already been applied.
    scrolled: Option<(usize, usize)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            note_length: 0.25,
            zoom: 1.0,
            dragging: None,
            scrolled: None,
        }
    }
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let Some((t, slot)) = app.selected_clip else {
        ui.centered_and_justified(|ui| {
            ui.weak("Selecciona un clip para editarlo, o haz doble clic en un slot vacío para crear uno.");
        });
        return;
    };
    let playhead = app.audio.state().and_then(|s| {
        let status = &s.tracks[t];
        (status.playing.load(Ordering::Relaxed) == slot as i32).then(|| status.clip_position())
    });

    let is_drums = app.project.tracks[t].is_drums();
    let state = &mut app.piano_roll;
    let Some(clip) = app.project.tracks[t].clips[slot].as_mut() else {
        app.selected_clip = None;
        return;
    };
    let mut changed = false;
    let mut preview = None;

    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut clip.name).desired_width(140.0));
        ui.label("Longitud:");
        let bars = clip.length / 4.0;
        egui::ComboBox::from_id_salt("clip_length")
            .selected_text(format!("{bars} comp."))
            .width(80.0)
            .show_ui(ui, |ui| {
                for option in [1.0, 2.0, 4.0, 8.0, 16.0] {
                    if ui.selectable_label(bars == option, format!("{option} comp.")).clicked() {
                        clip.length = option * 4.0;
                        changed = true;
                    }
                }
            });
        ui.label("Nota:");
        let current = NOTE_LENGTHS
            .iter()
            .find(|(v, _)| *v == state.note_length)
            .map_or("?", |(_, l)| *l);
        egui::ComboBox::from_id_salt("note_length")
            .selected_text(current)
            .width(80.0)
            .show_ui(ui, |ui| {
                for (value, label) in NOTE_LENGTHS {
                    ui.selectable_value(&mut state.note_length, value, label);
                }
            });
        if ui.button("Duplicar loop").on_hover_text("Dobla la longitud copiando las notas").clicked() {
            let length = clip.length;
            let copies: Vec<Note> = clip
                .notes
                .iter()
                .map(|n| Note { start: n.start + length, ..n.clone() })
                .collect();
            clip.notes.extend(copies);
            clip.length *= 2.0;
            changed = true;
        }
        if ui.button("Borrar notas").clicked() {
            clip.notes.clear();
            changed = true;
        }
        ui.label("Zoom");
        ui.add(egui::Slider::new(&mut state.zoom, 0.5..=3.0).show_value(false));
    });
    ui.separator();

    let pitches: Vec<u8> = if is_drums {
        vec![
            drum_note::OPEN_HAT,
            drum_note::CLOSED_HAT,
            drum_note::CLAP,
            drum_note::SNARE,
            drum_note::KICK,
        ]
    } else {
        (24..=96).rev().collect()
    };
    let row_h = if is_drums { 26.0 } else { 14.0 };
    let step_w = 20.0 * state.zoom;
    let steps = (clip.length / STEP).ceil() as usize;
    let grid_w = steps as f32 * step_w;
    let base = color(clip.color);

    let mut scroll = egui::ScrollArea::both().id_salt(("piano_roll", t, slot)).auto_shrink(false);
    if !is_drums && state.scrolled != Some((t, slot)) {
        state.scrolled = Some((t, slot));
        // Open roughly around C4 the first time a clip is shown.
        scroll = scroll.vertical_scroll_offset(row_h * (96 - 72) as f32);
    }
    scroll.show(ui, |ui| {
        let size = vec2(KEY_W + grid_w, pitches.len() as f32 * row_h);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let grid = Rect::from_min_max(pos2(rect.left() + KEY_W, rect.top()), rect.max);

        // Rows and keys.
        for (row, &pitch) in pitches.iter().enumerate() {
            let y = rect.top() + row as f32 * row_h;
            let lane = Rect::from_min_size(pos2(grid.left(), y), vec2(grid_w, row_h));
            let dark = !is_drums && is_black_key(pitch);
            painter.rect_filled(lane, 0.0, Color32::from_gray(if dark { 30 } else { 38 }));
            let key = Rect::from_min_size(pos2(rect.left(), y), vec2(KEY_W - 2.0, row_h - 1.0));
            painter.rect_filled(key, 1.0, if dark { Color32::from_gray(25) } else { Color32::from_gray(200) });
            let label = if is_drums {
                drum_name(pitch).unwrap_or_default().to_string()
            } else if pitch % 12 == 0 {
                note_name(pitch)
            } else {
                String::new()
            };
            painter.text(
                pos2(key.left() + 4.0, key.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.0),
                if dark { Color32::from_gray(200) } else { Color32::BLACK },
            );
            painter.line_segment(
                [pos2(grid.left(), y + row_h), pos2(grid.right(), y + row_h)],
                Stroke::new(1.0, Color32::from_gray(22)),
            );
        }

        // Beat and bar lines.
        for step in 0..=steps {
            let x = grid.left() + step as f32 * step_w;
            let gray = if step % 16 == 0 {
                90
            } else if step % 4 == 0 {
                60
            } else {
                45
            };
            painter.line_segment([pos2(x, grid.top()), pos2(x, grid.bottom())], Stroke::new(1.0, Color32::from_gray(gray)));
        }

        // Notes.
        let note_rect = |n: &Note| -> Option<Rect> {
            let row = pitches.iter().position(|&p| p == n.pitch)?;
            let x = grid.left() + (n.start / STEP) as f32 * step_w;
            let w = ((n.length / STEP) as f32 * step_w).max(3.0);
            Some(Rect::from_min_size(
                pos2(x + 1.0, rect.top() + row as f32 * row_h + 1.0),
                vec2(w - 2.0, row_h - 2.0),
            ))
        };
        for n in &clip.notes {
            if let Some(r) = note_rect(n) {
                let fill = base.linear_multiply(0.45 + 0.55 * n.velocity);
                painter.rect_filled(r, 2.0, fill);
                painter.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_gray(15)), StrokeKind::Inside);
            }
        }

        if let Some(position) = playhead {
            let x = grid.left() + (position / STEP) as f32 * step_w;
            painter.line_segment([pos2(x, grid.top()), pos2(x, grid.bottom())], Stroke::new(1.5, Color32::WHITE));
        }

        // Interaction.
        let pressed = ui.input(|i| i.pointer.primary_pressed());
        let released = ui.input(|i| i.pointer.primary_released());
        let pointer = response.interact_pointer_pos().or_else(|| response.hover_pos());
        if let Some(pos) = pointer {
            let row = ((pos.y - rect.top()) / row_h).floor();
            let pitch = (row >= 0.0).then(|| pitches.get(row as usize).copied()).flatten();
            if pressed && response.hovered() {
                if pos.x < grid.left() {
                    preview = pitch;
                } else if let Some(pitch) = pitch {
                    let beat = ((pos.x - grid.left()) / step_w) as f64 * STEP;
                    let hit = clip
                        .notes
                        .iter()
                        .position(|n| n.pitch == pitch && beat >= n.start && beat < n.start + n.length);
                    match hit {
                        Some(index) => {
                            clip.notes.remove(index);
                        }
                        None => {
                            let start = (beat / STEP).floor() * STEP;
                            if start < clip.length {
                                clip.notes.push(Note {
                                    start,
                                    length: state.note_length.min(clip.length - start),
                                    pitch,
                                    velocity: 0.8,
                                });
                                state.dragging = Some(clip.notes.len() - 1);
                                preview = Some(pitch);
                            }
                        }
                    }
                    changed = true;
                }
            } else if let Some(index) = state.dragging {
                if response.dragged() {
                    if let Some(n) = clip.notes.get_mut(index) {
                        let beat = ((pos.x - grid.left()) / step_w) as f64 * STEP;
                        let end = ((beat / STEP).ceil() * STEP).min(clip.length);
                        let length = (end - n.start).max(STEP);
                        if length != n.length {
                            n.length = length;
                            changed = true;
                        }
                    }
                }
            }
        }
        if released {
            state.dragging = None;
        }

        painter.rect_stroke(grid, 0.0, Stroke::new(1.0, Color32::from_gray(70)), StrokeKind::Inside);
    });

    if let Some(pitch) = preview {
        app.audio.send(Command::PreviewNote { track: t, pitch, velocity: 0.8 });
    }
    if changed {
        app.sync_clip(t, slot);
    }
}
