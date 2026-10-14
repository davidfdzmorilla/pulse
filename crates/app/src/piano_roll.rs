//! MIDI note editor for the selected clip (session or arrangement).
//!
//! Click an empty cell to add a note (drag right to lengthen it), click a note
//! to delete it, click a key to audition it.

use crate::app::{ClipRef, PulseApp};
use crate::project::{drum_name, is_black_key, note_name};
use crate::theme::{self, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, StrokeKind};
use engine::model::{drum_note, Clip, Note};
use engine::Command;
use std::sync::atomic::Ordering;

const KEY_W: f32 = 64.0;
const RULER_H: f32 = 16.0;
const STEP: f64 = 0.25;

const NOTE_LENGTHS: [(f64, &str); 5] = [
    (0.25, "1/16"),
    (0.5, "1/8"),
    (1.0, "1/4"),
    (2.0, "1/2"),
    (4.0, "1 Compás"),
];

pub struct State {
    note_length: f64,
    zoom: f32,
    /// Index of the note being lengthened by the current drag.
    dragging: Option<usize>,
    /// Clip whose initial scroll position has already been applied.
    scrolled: Option<ClipRef>,
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

/// Playhead inside the clip, if it is currently sounding.
fn playhead(app: &PulseApp, clip_ref: ClipRef) -> Option<f64> {
    let state = app.audio.state()?;
    let status = &state.tracks[clip_ref.track()];
    match clip_ref {
        ClipRef::Session { slot, .. } => {
            (status.playing.load(Ordering::Relaxed) == slot as i32).then(|| status.clip_position())
        }
        ClipRef::Arrangement { track, index } => {
            let placed = app.project.tracks.get(track)?.arrangement.get(index)?;
            let position = state.position();
            let audible = status.follows_arrangement.load(Ordering::Relaxed)
                && status.playing.load(Ordering::Relaxed) < 0
                && position >= placed.start
                && position < placed.end();
            let length = placed.clip.length();
            (audible && length > 0.0).then(|| (position - placed.start).rem_euclid(length))
        }
    }
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let Some(clip_ref) = app.selected_clip.filter(|&c| app.clip(c).is_some()) else {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("Selecciona un clip para editarlo.\nDoble clic en un slot vacío o en una pista del Arrangement crea uno nuevo.")
                    .color(TEXT_DIM),
            );
        });
        return;
    };
    let t = clip_ref.track();
    let playhead = playhead(app, clip_ref);
    if matches!(app.clip(clip_ref), Some(Clip::Audio(_))) {
        crate::sample_editor::show(app, ui, clip_ref, playhead);
        return;
    }
    let is_drums = app.project.tracks[t].is_drums();
    let state = &mut app.piano_roll;
    let clip = match clip_ref {
        ClipRef::Session { track, slot } => app.project.tracks[track].clips[slot].as_mut(),
        ClipRef::Arrangement { track, index } => app.project.tracks[track].arrangement.get_mut(index).map(|a| &mut a.clip),
    };
    let Some(Clip::Midi(clip)) = clip else { return };
    let mut changed = false;
    let mut preview = None;

    ui.horizontal(|ui| {
        let (swatch, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        ui.painter().rect_filled(swatch, 2.0, theme::rgb(clip.color));
        ui.add(egui::TextEdit::singleline(&mut clip.name).desired_width(130.0));
        ui.label(egui::RichText::new("Loop").color(TEXT_DIM).size(10.5));
        let bars = clip.length / 4.0;
        egui::ComboBox::from_id_salt("clip_length")
            .selected_text(format!("{bars} comp."))
            .width(74.0)
            .show_ui(ui, |ui| {
                for option in [1.0, 2.0, 4.0, 8.0, 16.0] {
                    if ui.selectable_label(bars == option, format!("{option} comp.")).clicked() {
                        clip.length = option * 4.0;
                        changed = true;
                    }
                }
            });
        ui.label(egui::RichText::new("Nota").color(TEXT_DIM).size(10.5));
        let current = NOTE_LENGTHS
            .iter()
            .find(|(v, _)| *v == state.note_length)
            .map_or("?", |(_, l)| *l);
        egui::ComboBox::from_id_salt("note_length")
            .selected_text(current)
            .width(74.0)
            .show_ui(ui, |ui| {
                for (value, label) in NOTE_LENGTHS {
                    ui.selectable_value(&mut state.note_length, value, label);
                }
            });
        if ui.button("×2").on_hover_text("Duplicar loop: dobla la longitud copiando las notas").clicked() {
            let length = clip.length;
            let copies: Vec<Note> = clip.notes.iter().map(|n| Note { start: n.start + length, ..n.clone() }).collect();
            clip.notes.extend(copies);
            clip.length *= 2.0;
            changed = true;
        }
        if ui.button("Borrar notas").clicked() {
            clip.notes.clear();
            changed = true;
        }
        ui.label(egui::RichText::new("Zoom").color(TEXT_DIM).size(10.5));
        ui.add(egui::Slider::new(&mut state.zoom, 0.5..=3.0).show_value(false));
    });
    ui.add_space(4.0);

    let pitches: Vec<u8> = if is_drums {
        vec![drum_note::OPEN_HAT, drum_note::CLOSED_HAT, drum_note::CLAP, drum_note::SNARE, drum_note::KICK]
    } else {
        (24..=96).rev().collect()
    };
    let row_h = if is_drums { 24.0 } else { 13.0 };
    let step_w = 18.0 * state.zoom;
    let steps = (clip.length / STEP).ceil() as usize;
    let grid_w = steps as f32 * step_w;
    let base = theme::rgb(clip.color);

    let mut scroll = egui::ScrollArea::both().id_salt(("piano_roll", format!("{clip_ref:?}"))).auto_shrink(false);
    if !is_drums && state.scrolled != Some(clip_ref) {
        state.scrolled = Some(clip_ref);
        // Open around the clip's notes, or middle C, the first time it is shown.
        let center = clip.notes.iter().map(|n| n.pitch as f32).sum::<f32>() / clip.notes.len().max(1) as f32;
        let center = if clip.notes.is_empty() { 60.0 } else { center };
        scroll = scroll.vertical_scroll_offset((row_h * (96.0 - center) - 80.0).max(0.0));
    }
    scroll.show(ui, |ui| {
        let size = vec2(KEY_W + grid_w, RULER_H + pitches.len() as f32 * row_h);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let top = rect.top() + RULER_H;
        let grid = Rect::from_min_max(pos2(rect.left() + KEY_W, top), rect.max);

        // Ruler.
        for bar in 0..=(clip.length / 4.0).ceil() as usize {
            let x = grid.left() + (bar * 16) as f32 * step_w;
            painter.text(pos2(x + 3.0, rect.top() + 2.0), Align2::LEFT_TOP, (bar + 1).to_string(), theme::small(), TEXT_DIM);
        }

        // Rows and keys.
        for (row, &pitch) in pitches.iter().enumerate() {
            let y = top + row as f32 * row_h;
            let lane = Rect::from_min_size(pos2(grid.left(), y), vec2(grid_w, row_h));
            let dark = !is_drums && is_black_key(pitch);
            painter.rect_filled(lane, 0.0, if dark { Color32::from_gray(36) } else { Color32::from_gray(44) });
            let key = Rect::from_min_size(pos2(rect.left(), y), vec2(KEY_W - 2.0, row_h - 1.0));
            painter.rect_filled(key, 1.0, if dark { Color32::from_gray(30) } else { Color32::from_gray(205) });
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
                theme::small(),
                if dark { Color32::from_gray(200) } else { Color32::from_gray(30) },
            );
            painter.line_segment([pos2(grid.left(), y + row_h), pos2(grid.right(), y + row_h)], Stroke::new(1.0, Color32::from_gray(30)));
        }

        // Beat and bar lines.
        for step in 0..=steps {
            let x = grid.left() + step as f32 * step_w;
            let gray = if step % 16 == 0 {
                95
            } else if step % 4 == 0 {
                66
            } else {
                52
            };
            painter.line_segment([pos2(x, grid.top()), pos2(x, grid.bottom())], Stroke::new(1.0, Color32::from_gray(gray)));
        }

        // Notes.
        for n in &clip.notes {
            let Some(row) = pitches.iter().position(|&p| p == n.pitch) else { continue };
            let x = grid.left() + (n.start / STEP) as f32 * step_w;
            let w = ((n.length / STEP) as f32 * step_w).max(3.0);
            let r = Rect::from_min_size(pos2(x + 1.0, top + row as f32 * row_h + 1.0), vec2(w - 2.0, row_h - 2.0));
            painter.rect_filled(r, 2.0, theme::shade(base, 0.5 + 0.5 * n.velocity));
            painter.rect_stroke(r, 2.0, Stroke::new(1.0, theme::shade(base, 0.35)), StrokeKind::Inside);
        }

        if let Some(position) = playhead {
            let x = grid.left() + (position / STEP) as f32 * step_w;
            painter.line_segment([pos2(x, rect.top()), pos2(x, grid.bottom())], Stroke::new(1.5, Color32::WHITE));
            widgets::play_triangle(&painter, pos2(x, rect.top() + 8.0), 7.0, Color32::WHITE);
        }

        // Interaction.
        let pressed = ui.input(|i| i.pointer.primary_pressed());
        let released = ui.input(|i| i.pointer.primary_released());
        if let Some(pos) = response.interact_pointer_pos().or_else(|| response.hover_pos()) {
            let row = ((pos.y - top) / row_h).floor();
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
        app.sync_clip_ref(clip_ref);
    }
}
