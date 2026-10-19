//! MIDI note editor for the selected clip (session or arrangement).
//!
//! Select mode (default): click a note to select it (Shift adds), drag to move
//! (Alt copies), drag its right edge to resize, drag on empty space to select
//! with a rectangle, double-click to create or delete. Draw mode (B): click to add
//! (drag to lengthen) or delete. The velocity lane below edits note velocities.
//! Keys: arrows move/transpose, ⌘A/⌘C/⌘X/⌘V/⌘D, ⌘U quantizes, Delete removes.
//!
//! With a project key the scale's rows are lit and the tonic's marked; «Plegar»
//! leaves only the scale's rows, and then notes move along the scale.
//!
//! Under the ruler, the chord each stretch plays (see `harmony`); «Acordes»
//! writes the key's chords at the insert marker.

use crate::app::{ClipRef, PulseApp};
use crate::note_edit::{self, Selection, MIN_LENGTH};
use crate::project::{is_black_key, note_name};
use crate::theme::{self, ACCENT, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Key, Rect, Sense, Stroke, StrokeKind};
use engine::model::{Clip, MidiClip, Note};
use engine::Command;
use stems::key::Key as Scale;
use std::sync::atomic::Ordering;

const KEY_W: f32 = 64.0;
const RULER_H: f32 = 16.0;
/// The strip of chord names under the ruler.
const CHORD_H: f32 = 15.0;
const VELOCITY_H: f32 = 60.0;
const LANE_HEADER_H: f32 = 22.0;
const EDGE_GRAB: f32 = 5.0;

const GRIDS: [(f64, &str); 6] = [
    (1.0, "1/4"),
    (0.5, "1/8"),
    (1.0 / 3.0, "1/8T"),
    (0.25, "1/16"),
    (1.0 / 6.0, "1/16T"),
    (0.125, "1/32"),
];
const NOTE_LENGTHS: [(f64, &str); 8] = [
    (0.125, "1/32"),
    (1.0 / 6.0, "1/16T"),
    (0.25, "1/16"),
    (1.0 / 3.0, "1/8T"),
    (0.5, "1/8"),
    (1.0, "1/4"),
    (2.0, "1/2"),
    (4.0, "1 Compás"),
];

#[derive(Clone)]
enum Drag {
    /// Draw mode: lengthening the note just added.
    Draw { index: usize },
    Move { origin: (f64, i32), snapshot: Vec<(usize, Note)>, copy: bool, last: (f64, i32) },
    Resize { origin: f64, snapshot: Vec<(usize, Note)> },
    Select { anchor: egui::Pos2, base: Selection },
    Velocity { origin_y: f32, snapshot: Vec<(usize, Note)> },
}

pub struct State {
    note_length: f64,
    grid: f64,
    zoom: f32,
    draw_mode: bool,
    selection: Selection,
    /// Clip the selection belongs to; switching clips clears it.
    selection_clip: Option<ClipRef>,
    drag: Option<Drag>,
    /// Where ⌘V pastes, set by clicking empty space (like Live's insert marker).
    insert_at: f64,
    clipboard: Vec<Note>,
    quantize_strength: f64,
    quantize_ends: bool,
    /// Clip whose initial scroll position has already been applied.
    scrolled: Option<ClipRef>,
    /// What the lane under the notes shows: velocities or a clip envelope.
    pub(crate) lane: crate::clip_envelope::LaneChoice,
    scroll_x: f32,
    /// Only the rows of the project's scale.
    fold: bool,
    chords: crate::harmony::ChordMenu,
    /// How «Generar ritmo» makes drum beats, and the takes made so far.
    pub(crate) drums: crate::drum_gen::Settings,
    drum_takes: u64,
    /// Why the drum model couldn't play, shown in the menu.
    drum_error: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            note_length: 0.25,
            grid: 0.25,
            zoom: 1.0,
            draw_mode: false,
            selection: Selection::new(),
            selection_clip: None,
            drag: None,
            insert_at: 0.0,
            clipboard: Vec::new(),
            quantize_strength: 1.0,
            quantize_ends: false,
            scrolled: None,
            lane: Default::default(),
            scroll_x: 0.0,
            fold: false,
            chords: Default::default(),
            drums: Default::default(),
            drum_takes: 0,
            drum_error: None,
        }
    }
}

impl State {
    /// Whether note-level shortcuts (Delete, ⌘D) should apply to notes rather than clips.
    pub fn has_selection(&self) -> bool {
        !self.selection.is_empty()
    }

    #[cfg(test)]
    pub fn select_notes(&mut self, clip: ClipRef, notes: &[usize]) {
        self.selection = notes.iter().copied().collect();
        self.selection_clip = Some(clip);
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

fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(TEXT_DIM).size(10.5));
}

fn combo(ui: &mut egui::Ui, id: &str, value: &mut f64, options: &[(f64, &str)]) {
    let current = options.iter().find(|(v, _)| v == value).map_or("?", |(_, l)| *l);
    egui::ComboBox::from_id_salt(id).selected_text(current).width(70.0).show_ui(ui, |ui| {
        for &(option, text) in options {
            ui.selectable_value(value, option, text);
        }
    });
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
    // The parameters the lane under the notes can show as clip envelopes.
    let target = engine::Target::Track(t);
    let params = app.automatable(target);
    let names = app.piano_roll.lane.stale(t, params.len()).then(|| params.iter().map(|&p| (p, app.param_name(target, p))).collect());
    app.piano_roll.lane.refresh(t, &params, names);
    let envelope = app.piano_roll.lane.shown.and_then(|p| app.param_info(target, p).map(|info| (p, info, app.param_value(target, p))));
    if matches!(app.clip(clip_ref), Some(Clip::Audio(_))) {
        crate::sample_editor::show(app, ui, clip_ref, playhead, envelope);
        return;
    }
    let is_drums = app.project.tracks[t].is_drums();
    // One row per drum sound or pad, the lowest note at the bottom.
    let drum_rows = if is_drums { app.project.tracks[t].drum_rows() } else { Vec::new() };
    let key = app.project.key.filter(|_| !is_drums);
    let drum_machine = match app.project.tracks[t].instrument {
        engine::model::InstrumentKind::DrumMachine(params) => Some(params),
        _ => None,
    };
    // A drum clip: its kit's sounds, and where the bass plays over it.
    let drums = is_drums.then(|| {
        let kit = crate::drum_gen::Kit::from_rows(&drum_rows);
        // Over as long as a generated beat can be (eight bars).
        let length = app.clip(clip_ref).map_or(4.0, |c| c.length()).max(32.0);
        let (slot, span) = match clip_ref {
            ClipRef::Session { slot, .. } => (Some(slot), None),
            ClipRef::Arrangement { track, index } => (None, app.project.tracks[track].arrangement.get(index).map(|p| (p.start, p.end()))),
        };
        (kit, crate::drum_gen::bass_onsets(&app.project.tracks, slot, span, length))
    });
    // The accompaniment just written can be written again, another way.
    let mut vary = app.can_vary(clip_ref).then_some(false);
    let keys_enabled = !ui.ctx().egui_wants_keyboard_input();
    let state = &mut app.piano_roll;
    if state.selection_clip != Some(clip_ref) {
        state.selection.clear();
        state.selection_clip = Some(clip_ref);
        state.drag = None;
        state.insert_at = 0.0;
    }
    let clip = match clip_ref {
        ClipRef::Session { track, slot } => app.project.tracks[track].clips[slot].as_mut(),
        ClipRef::Arrangement { track, index } => app.project.tracks[track].arrangement.get_mut(index).map(|a| &mut a.clip),
    };
    let Some(Clip::Midi(clip)) = clip else { return };
    let before = clip.clone();
    let mut previews: Vec<u8> = Vec::new();

    previews.extend(toolbar(ui, state, clip, key, is_drums, &mut vary));
    let (tempo, machine) = drums.as_ref().map_or((None, None), |(kit, bass)| drum_menu(ui, state, clip, kit, bass, drum_machine.is_some()));
    // Folded, notes move along the scale.
    let walk = key.filter(|_| state.fold);
    if keys_enabled {
        keyboard(ui, state, clip, walk);
    }
    state.selection.retain(|&i| i < clip.notes.len());

    let pitches: Vec<u8> = if is_drums {
        drum_rows.iter().map(|(pitch, _)| *pitch).collect()
    } else {
        rows(walk, &clip.notes)
    };
    let row_h = if !is_drums { 13.0 } else if pitches.len() > 12 { 20.0 } else { 24.0 };
    // Sixteenths are 18 px at zoom 1.
    let beat_w = 72.0 * state.zoom;
    let grid_w = clip.length as f32 * beat_w;
    let base = theme::rgb(clip.color);

    let lane_height = (ui.available_height() - VELOCITY_H - LANE_HEADER_H - 8.0).max(60.0);
    let mut scroll = egui::ScrollArea::both()
        .id_salt(("piano_roll", format!("{clip_ref:?}")))
        .auto_shrink(false)
        .max_height(lane_height);
    if !is_drums && state.scrolled != Some(clip_ref) {
        state.scrolled = Some(clip_ref);
        // Open around the clip's notes, or middle C, the first time it is shown.
        let center = clip.notes.iter().map(|n| n.pitch as f32).sum::<f32>() / clip.notes.len().max(1) as f32;
        let center = if clip.notes.is_empty() { 60.0 } else { center };
        scroll = scroll.vertical_scroll_offset((row_h * (96.0 - center) - 80.0).max(0.0));
    }
    let output = scroll.show(ui, |ui| {
        let chord_h = if is_drums { 0.0 } else { CHORD_H };
        let size = vec2(KEY_W + grid_w, RULER_H + chord_h + pitches.len() as f32 * row_h);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let top = rect.top() + RULER_H + chord_h;
        let grid = Rect::from_min_max(pos2(rect.left() + KEY_W, top), rect.max);
        let x_of = |beat: f64| grid.left() + beat as f32 * beat_w;
        let beat_of = |x: f32| ((x - grid.left()) / beat_w) as f64;
        let row_of_y = |y: f32| ((y - top) / row_h).floor() as i32;
        let row_of_pitch = |pitch: u8| pitches.iter().position(|&p| p == pitch);
        let note_rect = |n: &Note| -> Option<Rect> {
            let row = row_of_pitch(n.pitch)?;
            Some(Rect::from_min_size(
                pos2(x_of(n.start) + 1.0, top + row as f32 * row_h + 1.0),
                vec2((n.length as f32 * beat_w - 2.0).max(3.0), row_h - 2.0),
            ))
        };

        // Ruler and insert marker.
        for bar in 0..=(clip.length / 4.0).ceil() as usize {
            painter.text(pos2(x_of(bar as f64 * 4.0) + 3.0, rect.top() + 2.0), Align2::LEFT_TOP, (bar + 1).to_string(), theme::small(), TEXT_DIM);
        }
        let insert_x = x_of(state.insert_at);
        painter.line_segment([pos2(insert_x, rect.top()), pos2(insert_x, rect.top() + RULER_H)], Stroke::new(2.0, ACCENT));

        // The chords, with their degree in the project's key.
        if !is_drums {
            let strip_y = rect.top() + RULER_H;
            for (from, to, chord) in crate::harmony::chords(&clip.notes, clip.length, key) {
                let Some(chord) = chord else { continue };
                let r = Rect::from_min_max(pos2(x_of(from) + 1.0, strip_y + 1.0), pos2(x_of(to) - 1.0, strip_y + CHORD_H - 1.0));
                painter.rect_filled(r, 2.0, Color32::from_gray(52));
                let text = match key.and_then(|k| chord.numeral(k)) {
                    Some(numeral) => format!("{} · {numeral}", chord.name()),
                    None => chord.name(),
                };
                painter.with_clip_rect(r.intersect(painter.clip_rect())).text(pos2(r.left() + 4.0, r.center().y), Align2::LEFT_CENTER, text, theme::small(), Color32::from_gray(225));
            }
        }

        // Rows and keys; with a key, its scale's rows lit and the tonic's tinted.
        let scale = key.map(|k| k.scale());
        for (row, &pitch) in pitches.iter().enumerate() {
            let y = top + row as f32 * row_h;
            let dark = !is_drums && is_black_key(pitch);
            let tonic = key.is_some_and(|k| pitch % 12 == k.tonic);
            let fill = match scale {
                _ if tonic => theme::shade(ACCENT, 0.32),
                Some(scale) if scale[pitch as usize % 12] => Color32::from_gray(48),
                Some(_) => Color32::from_gray(33),
                None if dark => Color32::from_gray(36),
                None => Color32::from_gray(44),
            };
            painter.rect_filled(Rect::from_min_size(pos2(grid.left(), y), vec2(grid_w, row_h)), 0.0, fill);
            let piano_key = Rect::from_min_size(pos2(rect.left(), y), vec2(KEY_W - 2.0, row_h - 1.0));
            painter.rect_filled(piano_key, 1.0, if dark { Color32::from_gray(30) } else { Color32::from_gray(205) });
            if tonic {
                painter.rect_filled(Rect::from_min_size(piano_key.right_top() - vec2(4.0, 0.0), vec2(4.0, piano_key.height())), 0.0, ACCENT);
            }
            let text = if is_drums {
                drum_rows[row].1.clone()
            } else if pitch % 12 == 0 || tonic || walk.is_some() {
                note_name(pitch)
            } else {
                String::new()
            };
            painter.text(pos2(piano_key.left() + 4.0, piano_key.center().y), Align2::LEFT_CENTER, text, theme::small(), if dark { Color32::from_gray(200) } else { Color32::from_gray(30) });
            painter.line_segment([pos2(grid.left(), y + row_h), pos2(grid.right(), y + row_h)], Stroke::new(1.0, Color32::from_gray(30)));
        }

        // Grid lines at the chosen resolution, stronger on beats and bars.
        let steps = (clip.length / state.grid).ceil() as usize;
        for step in 0..=steps {
            let beat = step as f64 * state.grid;
            // Triplet steps are not exact in floating point, so compare with a tolerance.
            let on = |unit: f64| ((beat / unit) - (beat / unit).round()).abs() < 1e-6;
            let gray = if on(4.0) { 95 } else if on(1.0) { 66 } else { 50 };
            let x = x_of(beat);
            painter.line_segment([pos2(x, grid.top()), pos2(x, grid.bottom())], Stroke::new(1.0, Color32::from_gray(gray)));
        }

        // Notes.
        for (i, n) in clip.notes.iter().enumerate() {
            let Some(r) = note_rect(n) else { continue };
            let selected = state.selection.contains(&i);
            let fill = theme::shade(base, if selected { 1.25 } else { 0.5 + 0.5 * n.velocity });
            painter.rect_filled(r, 2.0, fill);
            let outline = if selected { Stroke::new(1.5, Color32::WHITE) } else { Stroke::new(1.0, theme::shade(base, 0.35)) };
            painter.rect_stroke(r, 2.0, outline, StrokeKind::Inside);
        }

        if let Some(Drag::Select { anchor, .. }) = &state.drag {
            if let Some(pos) = response.interact_pointer_pos() {
                let band = Rect::from_two_pos(*anchor, pos);
                painter.rect_filled(band, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 18));
                painter.rect_stroke(band, 0.0, Stroke::new(1.0, Color32::from_gray(200)), StrokeKind::Inside);
            }
        }

        if let Some(position) = playhead {
            let x = x_of(position);
            painter.line_segment([pos2(x, rect.top()), pos2(x, grid.bottom())], Stroke::new(1.5, Color32::WHITE));
            widgets::play_triangle(&painter, pos2(x, rect.top() + 8.0), 7.0, Color32::WHITE);
        }
        painter.rect_stroke(grid, 0.0, Stroke::new(1.0, Color32::from_gray(70)), StrokeKind::Inside);

        // ----- Pointer -------------------------------------------------------------
        let Some(pos) = response.interact_pointer_pos().or_else(|| response.hover_pos()) else { return };
        let (pressed, released, modifiers) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.modifiers));
        let beat = beat_of(pos.x).max(0.0);
        let row = row_of_y(pos.y);
        let pitch_here = (row >= 0).then(|| pitches.get(row as usize).copied()).flatten();
        let hit = clip.notes.iter().enumerate().rev().find_map(|(i, n)| {
            let r = note_rect(n)?;
            r.contains(pos).then(|| (i, r.right() - pos.x <= EDGE_GRAB))
        });
        if state.drag.is_none() && matches!(hit, Some((_, true))) && !state.draw_mode {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let snapshot = |notes: &[Note], selection: &Selection| -> Vec<(usize, Note)> {
            selection.iter().filter_map(|&i| notes.get(i).map(|n| (i, n.clone()))).collect()
        };

        if pressed && response.hovered() {
            if pos.x < grid.left() {
                previews.extend(pitch_here);
            } else if pos.y >= top {
                state.drag = match (state.draw_mode, hit) {
                    (true, Some((i, _))) => {
                        clip.notes.remove(i);
                        state.selection.clear();
                        None
                    }
                    (true, None) => pitch_here.and_then(|pitch| {
                        let start = note_edit::snap_down(beat, state.grid);
                        (start < clip.length).then(|| {
                            clip.notes.push(Note { start, length: state.note_length.min(clip.length - start), pitch, velocity: 0.8 });
                            previews.push(pitch);
                            Drag::Draw { index: clip.notes.len() - 1 }
                        })
                    }),
                    (false, Some((i, edge))) => {
                        if modifiers.shift {
                            if !state.selection.remove(&i) {
                                state.selection.insert(i);
                            }
                        } else if !state.selection.contains(&i) {
                            state.selection = Selection::from([i]);
                        }
                        previews.push(clip.notes[i].pitch);
                        let snap = snapshot(&clip.notes, &state.selection);
                        if !state.selection.contains(&i) {
                            None
                        } else if edge {
                            Some(Drag::Resize { origin: beat, snapshot: snap })
                        } else {
                            Some(Drag::Move { origin: (beat, row), snapshot: snap, copy: modifiers.alt, last: (0.0, 0) })
                        }
                    }
                    (false, None) => {
                        if !modifiers.shift {
                            state.selection.clear();
                        }
                        state.insert_at = note_edit::snap(beat, state.grid).min(clip.length);
                        Some(Drag::Select { anchor: pos, base: state.selection.clone() })
                    }
                };
            }
        }

        if response.double_clicked() && !state.draw_mode && pos.x >= grid.left() && pos.y >= top {
            match hit {
                Some((i, _)) => {
                    clip.notes.remove(i);
                    state.selection.clear();
                }
                None => {
                    if let Some(pitch) = pitch_here {
                        let start = note_edit::snap_down(beat, state.grid);
                        if start < clip.length {
                            clip.notes.push(Note { start, length: state.note_length.min(clip.length - start), pitch, velocity: 0.8 });
                            state.selection = Selection::from([clip.notes.len() - 1]);
                            previews.push(pitch);
                        }
                    }
                }
            }
            state.drag = None;
        }

        if response.dragged() {
            match &mut state.drag {
                Some(Drag::Draw { index }) => {
                    if let Some(n) = clip.notes.get_mut(*index) {
                        let end = ((beat / state.grid).ceil() * state.grid).min(clip.length);
                        n.length = (end - n.start).max(state.grid.min(clip.length - n.start));
                    }
                }
                Some(Drag::Move { origin, snapshot, copy, last }) => {
                    let originals: Vec<Note> = snapshot.iter().map(|(_, n)| n.clone()).collect();
                    let earliest = originals.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);
                    let latest = originals.iter().map(|n| n.start).fold(f64::NEG_INFINITY, f64::max);
                    let shift = note_edit::snap(beat - origin.0, state.grid).max(-earliest).min((clip.length - MIN_LENGTH - latest).max(0.0));
                    let rows = row - origin.1;
                    if (shift, rows) != *last {
                        if *copy {
                            // Alt-drag leaves the originals and moves fresh copies.
                            let mut added = Selection::new();
                            for n in &originals {
                                clip.notes.push(n.clone());
                                added.insert(clip.notes.len() - 1);
                            }
                            *snapshot = added.iter().copied().zip(originals.iter().cloned()).collect();
                            state.selection = added;
                            *copy = false;
                        }
                        let targets: Vec<usize> = snapshot.iter().map(|(i, _)| *i).collect();
                        for (&i, original) in targets.iter().zip(&originals) {
                            let Some(n) = clip.notes.get_mut(i) else { continue };
                            n.start = original.start + shift;
                            if let Some(r) = row_of_pitch(original.pitch) {
                                let new_row = (r as i32 + rows).clamp(0, pitches.len() as i32 - 1) as usize;
                                n.pitch = pitches[new_row];
                            }
                        }
                        if rows != last.1 {
                            if let Some(&i) = targets.first() {
                                previews.extend(clip.notes.get(i).map(|n| n.pitch));
                            }
                        }
                        *last = (shift, rows);
                    }
                }
                Some(Drag::Resize { origin, snapshot }) => {
                    let delta = note_edit::snap(beat - *origin, state.grid);
                    for (i, original) in snapshot.iter() {
                        if let Some(n) = clip.notes.get_mut(*i) {
                            n.length = (original.length + delta).max(MIN_LENGTH.max(state.grid.min(original.length))).min(clip.length - n.start);
                        }
                    }
                }
                Some(Drag::Select { anchor, base }) => {
                    let band = Rect::from_two_pos(*anchor, pos);
                    let (low, high) = (row_of_y(band.top()).max(0) as usize, row_of_y(band.bottom()).max(0) as usize);
                    let region = note_edit::in_region(&clip.notes, (beat_of(band.left()), beat_of(band.right())), |p| {
                        row_of_pitch(p).is_some_and(|r| r >= low && r <= high)
                    });
                    state.selection = base.union(&region).copied().collect();
                }
                Some(Drag::Velocity { .. }) | None => {}
            }
        }
        if released {
            state.drag = None;
        }
    });
    state.scroll_x = output.state.offset.x;

    // ----- The lane under the notes: a clip envelope or the velocities ----------------
    crate::clip_envelope::header(ui, &mut state.lane, &mut clip.envelopes);
    if let Some((param, info, value)) = envelope {
        let (lane, _) = ui.allocate_exact_size(vec2(ui.available_width(), VELOCITY_H), Sense::hover());
        let origin_x = lane.left() + KEY_W - state.scroll_x;
        let x_of = |beat: f64| origin_x + beat as f32 * beat_w;
        let beat_of = |x: f32| ((x - origin_x) / beat_w) as f64;
        crate::clip_envelope::lane(ui, &mut state.lane, &mut clip.envelopes, clip.length, param, &info, value, lane, lane.left() + KEY_W, &x_of, &beat_of, state.grid, theme::METER_RED)
            .on_hover_text("Clic: añadir un punto · arrastrar: moverlo · doble clic: borrarlo");
    } else {
        let (lane, response) = ui.allocate_exact_size(vec2(ui.available_width(), VELOCITY_H), Sense::click_and_drag());
        let painter = ui.painter_at(lane);
        painter.rect_filled(lane, 2.0, theme::SLOT);
        painter.text(lane.left_top() + vec2(4.0, 3.0), Align2::LEFT_TOP, "Velocidad", theme::small(), TEXT_DIM);
        let origin_x = lane.left() + KEY_W - state.scroll_x;
        let bar_area = Rect::from_min_max(pos2(lane.left() + KEY_W, lane.top() + 4.0), lane.max - vec2(0.0, 3.0));
        let x_of = |beat: f64| origin_x + beat as f32 * beat_w;
        for (i, n) in clip.notes.iter().enumerate() {
            let x = x_of(n.start);
            if x < bar_area.left() || x > bar_area.right() {
                continue;
            }
            let y = bar_area.bottom() - bar_area.height() * n.velocity;
            let color = if state.selection.contains(&i) { Color32::WHITE } else { theme::shade(base, 1.1) };
            painter.line_segment([pos2(x + 1.0, bar_area.bottom()), pos2(x + 1.0, y)], Stroke::new(2.0, color));
            painter.circle_filled(pos2(x + 1.0, y), 2.5, color);
        }
        if let Some(pos) = response.interact_pointer_pos() {
            if ui.input(|i| i.pointer.primary_pressed()) && response.hovered() {
                // The note whose stem is nearest the pointer; selected notes win ties.
                let nearest = clip
                    .notes
                    .iter()
                    .enumerate()
                    .map(|(i, n)| (i, (x_of(n.start) - pos.x).abs() - if state.selection.contains(&i) { 0.5 } else { 0.0 }))
                    .filter(|&(_, d)| d < 6.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| i);
                if let Some(i) = nearest {
                    if !state.selection.contains(&i) {
                        state.selection = Selection::from([i]);
                    }
                    let snapshot = state.selection.iter().filter_map(|&j| clip.notes.get(j).map(|n| (j, n.clone()))).collect();
                    state.drag = Some(Drag::Velocity { origin_y: pos.y, snapshot });
                }
            }
            if response.dragged() {
                if let Some(Drag::Velocity { origin_y, snapshot }) = &state.drag {
                    let delta = (origin_y - pos.y) / bar_area.height();
                    for (i, original) in snapshot {
                        if let Some(n) = clip.notes.get_mut(*i) {
                            n.velocity = (original.velocity + delta).clamp(0.01, 1.0);
                        }
                    }
                }
            }
            if ui.input(|i| i.pointer.primary_released()) {
                state.drag = None;
            }
        }
        response.on_hover_text("Arrastra arriba o abajo para cambiar la velocidad de las notas seleccionadas");

    }

    let changed = *clip != before;
    if let Some(bpm) = tempo {
        app.set_param(engine::Target::Master, engine::model::Param::Tempo, bpm as f32);
    }
    if let (Some(model), Some(params)) = (machine, drum_machine) {
        if params.model != model {
            app.set_instrument(t, engine::model::InstrumentKind::DrumMachine(engine::model::DrumParams { model, ..params }));
        }
    }
    if vary == Some(true) {
        app.vary_accompaniment();
        return;
    }
    for pitch in previews {
        app.audio.send(Command::PreviewNote { track: t, pitch, velocity: 0.8 });
    }
    if changed {
        app.sync_clip_ref(clip_ref);
    }
}

/// The piano roll's rows, highest first: every note from C0 to C6, or, folded
/// to a scale, the scale's and those of the notes off it (so none is hidden).
fn rows(folded: Option<Scale>, notes: &[Note]) -> Vec<u8> {
    let scale = folded.map(|k| k.scale());
    (24..=96).rev().filter(|&p| scale.is_none_or(|s| s[p as usize % 12]) || notes.iter().any(|n| n.pitch == p)).collect()
}

/// Moves notes `semitones` up or down; folded to a scale, ±1 is a step of the
/// scale. Nothing moves if any would leave the MIDI range.
fn transpose(notes: &mut [Note], targets: &[usize], semitones: i32, walk: Option<Scale>) {
    match walk {
        Some(scale) if semitones.abs() == 1 => {
            let moved: Option<Vec<u8>> = targets.iter().map(|&i| scale.step(notes[i].pitch, semitones)).collect();
            for (&i, pitch) in targets.iter().zip(moved.into_iter().flatten()) {
                notes[i].pitch = pitch;
            }
        }
        _ => note_edit::transpose(notes, targets, semitones),
    }
}

/// Moves each note off the scale to the scale's nearest (the lower of two as
/// near); how many moved.
fn to_scale(notes: &mut [Note], targets: &[usize], scale: Scale) -> usize {
    let mut moved = 0;
    for &i in targets {
        let pitch = scale.snap(notes[i].pitch as f64);
        if pitch != notes[i].pitch {
            notes[i].pitch = pitch;
            moved += 1;
        }
    }
    moved
}

/// The editor's bar; the pitches of any chord written, to preview.
fn toolbar(ui: &mut egui::Ui, state: &mut State, clip: &mut MidiClip, key: Option<Scale>, is_drums: bool, vary: &mut Option<bool>) -> Vec<u8> {
    let mut previews = Vec::new();
    ui.horizontal_wrapped(|ui| {
        if let Some(asked) = vary.as_mut() {
            if widgets::flat_button(ui, "Otra variación", true, vec2(104.0, 20.0)).on_hover_text("Escribe este acompañamiento de otra manera, sobre los mismos acordes").clicked() {
                *asked = true;
            }
        }
        let (swatch, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        ui.painter().rect_filled(swatch, 2.0, theme::rgb(clip.color));
        ui.add(egui::TextEdit::singleline(&mut clip.name).desired_width(110.0));
        label(ui, "Loop");
        let bars = clip.length / 4.0;
        egui::ComboBox::from_id_salt("clip_length").selected_text(format!("{bars} comp.")).width(70.0).show_ui(ui, |ui| {
            for option in [1.0, 2.0, 4.0, 8.0, 16.0] {
                if ui.selectable_label(bars == option, format!("{option} comp.")).clicked() {
                    clip.length = option * 4.0;
                }
            }
        });
        label(ui, "Rejilla");
        combo(ui, "grid", &mut state.grid, &GRIDS);
        label(ui, "Nota");
        combo(ui, "note_length", &mut state.note_length, &NOTE_LENGTHS);
        if widgets::flat_button(ui, "Dibujar", state.draw_mode, vec2(70.0, 20.0)).on_hover_text("Modo dibujo (B): clic añade o borra notas").clicked() {
            state.draw_mode = !state.draw_mode;
        }
        groove_menu(ui, clip);
        if !is_drums {
            let (added, pitches) = crate::harmony::menu(ui, &mut state.chords, clip, &mut state.insert_at, key);
            if !added.is_empty() {
                state.selection = added.into_iter().collect();
                previews = pitches;
            }
        }
        ui.separator();
        let targets = note_edit::targets(&clip.notes, &state.selection);
        let scope = if state.selection.is_empty() { "todas las notas" } else { "las notas seleccionadas" };
        if ui.button("Cuantizar").on_hover_text(format!("Cuantiza {scope} a la rejilla (⌘U)")).clicked() {
            note_edit::quantize(&mut clip.notes, &targets, state.grid, state.quantize_strength, state.quantize_ends, clip.length);
        }
        let mut percent = state.quantize_strength * 100.0;
        if ui.add(egui::DragValue::new(&mut percent).range(0.0..=100.0).speed(1.0).suffix(" %")).on_hover_text("Intensidad de la cuantización").changed() {
            state.quantize_strength = percent / 100.0;
        }
        ui.checkbox(&mut state.quantize_ends, "finales").on_hover_text("Cuantiza también el final de las notas");
        ui.separator();
        let walk = key.filter(|_| state.fold);
        for (text, semitones) in [("-12", -12), ("-1", -1), ("+1", 1), ("+12", 12)] {
            let hint = if walk.is_some() { format!("Transponer {scope} por la escala (↑↓, ⇧ octava)") } else { format!("Transponer {scope} (↑↓, ⇧ octava)") };
            if ui.small_button(text).on_hover_text(hint).clicked() {
                transpose(&mut clip.notes, &targets, semitones, walk);
            }
        }
        if let Some(key) = key {
            ui.separator();
            let hint = format!("Solo las filas de {}: las notas se mueven por la escala", key.name());
            if widgets::flat_button(ui, "Plegar", state.fold, vec2(56.0, 20.0)).on_hover_text(hint).clicked() {
                state.fold = !state.fold;
                state.scrolled = None;
            }
            if ui.button("A la escala").on_hover_text(format!("Lleva {scope} que no son de {} a la nota de la escala más cercana", key.name())).clicked() {
                to_scale(&mut clip.notes, &targets, key);
            }
        }
        if ui.small_button("×2").on_hover_text("Duplicar loop: dobla la longitud copiando las notas").clicked() {
            let length = clip.length;
            let copies: Vec<Note> = clip.notes.iter().map(|n| Note { start: n.start + length, ..n.clone() }).collect();
            clip.notes.extend(copies);
            clip.length *= 2.0;
        }
        label(ui, "Zoom");
        ui.add(egui::Slider::new(&mut state.zoom, 0.5..=4.0).logarithmic(true).show_value(false));
    });
    ui.add_space(2.0);
    previews
}

/// «Generar ritmo»: a beat in a style for this drum clip (see `drum_gen`); each
/// «Otra versión» the style's next take. Returns a tempo to set the project to,
/// if asked.
fn drum_menu(ui: &mut egui::Ui, state: &mut State, clip: &mut MidiClip, kit: &crate::drum_gen::Kit, bass: &[f64], machine: bool) -> (Option<f64>, Option<engine::model::DrumModel>) {
    use crate::drum_gen::{Level, Style};
    let mut tempo = None;
    let mut model = None;
    ui.horizontal(|ui| {
        widgets::field_menu_button(ui, "Generar ritmo", |ui| {
            ui.set_min_width(320.0);
            let before = state.drums.clone();
            let settings = &mut state.drums;
            ui.label(egui::RichText::new("Estilo").color(TEXT_DIM).size(10.5));
            ui.horizontal_wrapped(|ui| {
                for style in Style::ALL {
                    ui.selectable_value(&mut settings.style, style, style.label()).on_hover_text(format!("Suele ir a unos {} BPM", style.tempo()));
                }
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Nivel").color(TEXT_DIM).size(10.5));
                for level in Level::ALL {
                    ui.selectable_value(&mut settings.level, level, level.label());
                }
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Largo").color(TEXT_DIM).size(10.5));
                for bars in [1, 2, 4, 8] {
                    ui.selectable_value(&mut settings.bars, bars, format!("{bars} c.")).on_hover_text("Compases del clip: con 4, el cuarto da la vuelta a la frase; con 8, redoble al final");
                }
            });
            ui.checkbox(&mut settings.ai, "Que lo toque la IA")
                .on_hover_text("El modelo de batería de Pulse oye la base del estilo y la toca a su manera, como un batería: varía, mete redobles donde acaban las frases y cada versión es distinta");
            ui.add_enabled(!settings.ai, egui::Checkbox::new(&mut settings.fills, "Redoble al final de cada frase"))
                .on_hover_text("En el último tiempo de cada 8 compases (o del clip, si tiene 4 o más), el de cada estilo")
                .on_disabled_hover_text("La IA pone los suyos donde acaban las frases");
            ui.add_enabled(!bass.is_empty(), egui::Checkbox::new(&mut settings.follow_bass, "El bombo sigue al bajo"))
                .on_hover_text("Añade bombos donde el bajo empieza notas (en corcheas)")
                .on_disabled_hover_text("No hay una pista de bajo sonando con este clip");
            if machine {
                ui.checkbox(&mut settings.machine, format!("Sonidos de su máquina ({})", settings.style.machine().label()))
                    .on_hover_text("Pone la Drum Machine en la máquina del estilo: 808 para trap, hip hop y reggaetón; 909 para el resto");
            }
            let style = settings.style;
            if ui.small_button(format!("Tempo del proyecto a {} BPM", style.tempo())).on_hover_text("Cada estilo suena a sí mismo a su tempo").clicked() {
                tempo = Some(style.tempo() as f64);
            }
            // Another style or level starts from its first take.
            if *settings != before {
                state.drum_takes = 0;
            }
            ui.separator();
            let takes = crate::drum_gen::take_count(state.drums.style, state.drums.level);
            let label = match state.drum_takes {
                0 => "Generar".to_string(),
                n if state.drums.ai => format!("Otra versión ({n})"),
                n => format!("Otra versión ({} de {takes})", (n as usize - 1) % takes + 1),
            };
            if ui.button(label).on_hover_text("Sustituye las notas del clip (⌘Z las recupera)").clicked() {
                let take = state.drum_takes;
                state.drum_takes += 1;
                let bass = if state.drums.follow_bass { bass } else { &[] };
                clip.length = state.drums.bars as f64 * 4.0;
                let beat = if state.drums.ai {
                    crate::drum_gen::generate_ai(kit, &state.drums, state.drums.bars as f64 * 4.0, bass, take)
                } else {
                    Ok(crate::drum_gen::generate(kit, &state.drums, state.drums.bars as f64 * 4.0, bass, take))
                };
                state.drum_error = None;
                match beat {
                    Ok((notes, swing)) => {
                        clip.length = state.drums.bars as f64 * 4.0;
                        clip.notes = notes;
                        clip.groove = swing;
                        state.selection.clear();
                        if machine && state.drums.machine {
                            model = Some(state.drums.style.machine());
                        }
                    }
                    Err(e) => state.drum_error = Some(e),
                }
            }
            if let Some(e) = &state.drum_error {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
        });
    });
    (tempo, model)
}

/// «Groove»: the clip follows the project's swing, or has its own groove
/// (swing, softer off-beats, humanize), with some ready-made ones.
fn groove_menu(ui: &mut egui::Ui, clip: &mut MidiClip) {
    use engine::model::Groove;
    let label = if clip.groove.is_some() { "Groove: propio" } else { "Groove: proyecto" };
    widgets::field_menu_button(ui, label, |ui| {
        ui.set_min_width(250.0);
        let mut own = clip.groove.is_some();
        ui.radio_value(&mut own, false, "El del proyecto").on_hover_text("El swing del transporte, como los demás clips");
        ui.radio_value(&mut own, true, "Uno propio para este clip");
        if own != clip.groove.is_some() {
            clip.groove = own.then(Groove::default);
        }
        let Some(groove) = clip.groove.as_mut() else { return };
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let presets = [
                ("Recto", Groove::default()),
                ("Swing 1/16", Groove { grid: 0.25, swing: 0.5, ..Groove::default() }),
                ("MPC 1/16", Groove { grid: 0.25, swing: 0.36, accent: 0.3, ..Groove::default() }),
                ("Shuffle 1/8", Groove { grid: 0.5, swing: 1.0, accent: 0.2, ..Groove::default() }),
                ("Humano", Groove { grid: 0.25, swing: 0.15, accent: 0.25, humanize: 0.5 }),
            ];
            for (name, preset) in presets {
                if ui.selectable_label(*groove == preset, name).clicked() {
                    *groove = preset;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Rejilla");
            ui.selectable_value(&mut groove.grid, 0.5, "1/8");
            ui.selectable_value(&mut groove.grid, 0.25, "1/16");
        });
        let percent = |ui: &mut egui::Ui, name: &str, value: &mut f32, hint: &str| {
            let mut p = *value * 100.0;
            ui.horizontal(|ui| {
                ui.add_sized([70.0, 18.0], egui::Label::new(name));
                if ui.add(egui::Slider::new(&mut p, 0.0..=100.0).suffix(" %").fixed_decimals(0)).on_hover_text(hint).changed() {
                    *value = p / 100.0;
                }
            });
        };
        percent(ui, "Swing", &mut groove.swing, "Retrasa los contratiempos de la rejilla (100 % = shuffle de tresillo)");
        percent(ui, "Acentos", &mut groove.accent, "Los contratiempos suenan más suaves");
        percent(ui, "Humanizar", &mut groove.humanize, "Pequeños desplazamientos al azar de tiempo y velocidad, distintos en cada vuelta");
        ui.label(egui::RichText::new("Se aplica al tocar: las notas no se mueven.").color(theme::TEXT_DIM).size(10.5));
    });
}

/// Prefix marking copied notes in the system clipboard.
const CLIPBOARD_PREFIX: &str = "pulse-notes:";

/// Puts copied notes on the system clipboard too. macOS only reports ⌘V to the app
/// when the clipboard holds text, so this is also what makes pasting work.
fn copy_notes(ui: &egui::Ui, state: &mut State, notes: Vec<Note>) {
    if let Ok(json) = serde_json::to_string(&notes) {
        ui.ctx().copy_text(format!("{CLIPBOARD_PREFIX}{json}"));
    }
    state.clipboard = notes;
}

/// Notes from pasted text, falling back to the last notes copied in Pulse.
fn pasted_notes(state: &State, text: &str) -> Vec<Note> {
    text.strip_prefix(CLIPBOARD_PREFIX)
        .and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_else(|| state.clipboard.clone())
}

fn keyboard(ui: &mut egui::Ui, state: &mut State, clip: &mut MidiClip, walk: Option<Scale>) {
    let events = ui.input(|i| i.events.clone());
    for event in events {
        let targets = note_edit::targets(&clip.notes, &state.selection);
        let selected: Vec<usize> = state.selection.iter().copied().collect();
        match event {
            egui::Event::Copy if !selected.is_empty() => copy_notes(ui, state, note_edit::copy(&clip.notes, &selected)),
            egui::Event::Cut if !selected.is_empty() => {
                copy_notes(ui, state, note_edit::copy(&clip.notes, &selected));
                note_edit::delete(&mut clip.notes, &state.selection);
                state.selection.clear();
            }
            egui::Event::Paste(text) => {
                let notes = pasted_notes(state, &text);
                if !notes.is_empty() {
                    state.selection = note_edit::paste(&mut clip.notes, &notes, state.insert_at, clip.length);
                }
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                let cmd = modifiers.command;
                match key {
                    Key::A if cmd => state.selection = (0..clip.notes.len()).collect(),
                    Key::D if cmd && !selected.is_empty() => {
                        state.selection = note_edit::duplicate(&mut clip.notes, &selected, clip.length);
                    }
                    Key::U if cmd => {
                        note_edit::quantize(&mut clip.notes, &targets, state.grid, state.quantize_strength, state.quantize_ends, clip.length);
                    }
                    Key::B if !cmd => state.draw_mode = !state.draw_mode,
                    Key::Escape => state.selection.clear(),
                    Key::Delete | Key::Backspace if !selected.is_empty() => {
                        note_edit::delete(&mut clip.notes, &state.selection);
                        state.selection.clear();
                    }
                    Key::ArrowLeft | Key::ArrowRight if !selected.is_empty() => {
                        let step = if modifiers.shift { 4.0 } else { state.grid };
                        let sign = if key == Key::ArrowLeft { -1.0 } else { 1.0 };
                        note_edit::shift(&mut clip.notes, &selected, sign * step, clip.length);
                    }
                    Key::ArrowUp | Key::ArrowDown if !selected.is_empty() => {
                        let step = if modifiers.shift { 12 } else { 1 };
                        let sign = if key == Key::ArrowUp { 1 } else { -1 };
                        transpose(&mut clip.notes, &selected, sign * step, walk);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_text_round_trips_notes_and_falls_back_to_the_internal_clipboard() {
        let notes = vec![Note { start: 0.5, length: 0.25, pitch: 64, velocity: 0.7 }];
        let text = format!("{CLIPBOARD_PREFIX}{}", serde_json::to_string(&notes).unwrap());
        let state = State { clipboard: vec![Note { start: 0.0, length: 1.0, pitch: 36, velocity: 1.0 }], ..Default::default() };
        assert_eq!(pasted_notes(&state, &text), notes);
        assert_eq!(pasted_notes(&state, "texto cualquiera")[0].pitch, 36);
    }

    #[test]
    fn folded_to_a_scale_notes_walk_it() {
        let e_minor = Scale { tonic: 4, minor: true };
        let note = |pitch| Note { start: 0.0, length: 1.0, pitch, velocity: 0.8 };
        let folded = rows(Some(e_minor), &[note(64)]);
        assert!(folded.iter().all(|p| e_minor.scale()[*p as usize % 12]));
        assert_eq!(folded.len(), 7 * 6 + 1, "six octaves of seven notes, and the top C");
        assert_eq!(rows(None, &[]).len(), 73);
        // An F (off E minor) keeps its row.
        assert_eq!(rows(Some(e_minor), &[note(65)]).len(), 7 * 6 + 2);

        // E and B a step up: F# and C.
        let mut notes = vec![note(64), note(71)];
        transpose(&mut notes, &[0, 1], 1, Some(e_minor));
        assert_eq!((notes[0].pitch, notes[1].pitch), (66, 72));
        transpose(&mut notes, &[0, 1], -1, Some(e_minor));
        assert_eq!((notes[0].pitch, notes[1].pitch), (64, 71));
        // Not folded, a semitone.
        transpose(&mut notes, &[0], 1, None);
        assert_eq!(notes[0].pitch, 65);

        // F and G# are not in E minor: to E (as near as F#, and lower) and G.
        let mut notes = vec![note(65), note(68), note(67)];
        assert_eq!(to_scale(&mut notes, &[0, 1, 2], e_minor), 2);
        assert_eq!(notes.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![64, 67, 67]);
    }

    /// What the editor draws, as texts and where.
    fn texts(app: &mut PulseApp) -> Vec<(String, Rect)> {
        let ctx = egui::Context::default();
        let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1600.0, 900.0))), ..Default::default() };
        let _ = ctx.run_ui(input(), |ui| show(app, ui));
        let output = ctx.run_ui(input(), |ui| show(app, ui));
        let mut texts = Vec::new();
        fn collect(shape: &egui::Shape, texts: &mut Vec<(String, Rect)>) {
            match shape {
                egui::Shape::Text(t) => texts.push((t.galley.text().to_string(), Rect::from_min_size(t.pos, t.galley.size()))),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
                _ => {}
            }
        }
        output.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
        texts
    }

    /// With a project key: «Plegar» leaves the scale's rows, «A la escala» moves
    /// the notes off it; without one, neither is there.
    #[test]
    fn the_project_scale_folds_the_rows_and_fits_the_notes() {
        use crate::tracks::tests::click;
        let mut app = PulseApp::headless(crate::project::Project::demo());
        // The bass (the drums have no scale).
        let clip = ClipRef::Session { track: 1, slot: 0 };
        app.select_clip(clip);
        let Some(Clip::Midi(midi)) = app.project.tracks[1].clips[0].as_mut() else { panic!() };
        midi.notes = vec![Note { start: 0.0, length: 1.0, pitch: 61, velocity: 0.8 }, Note { start: 1.0, length: 1.0, pitch: 64, velocity: 0.8 }];
        let find = |texts: &[(String, Rect)], text: &str| texts.iter().find(|(t, _)| t == text).map(|(_, r)| r.center());
        let view = |app: &mut PulseApp, ui: &mut egui::Ui| show(app, ui);
        assert!(find(&texts(&mut app), "Plegar").is_none());

        app.project.key = Some(Scale { tonic: 0, minor: false });
        let shown = texts(&mut app);
        let fold = find(&shown, "Plegar").expect("a Plegar button");
        click(&mut app, view, fold, egui::Modifiers::NONE);
        assert!(app.piano_roll.fold);
        // Folded every row is named; of the black keys only the note's (C#3) is left.
        let shown = texts(&mut app);
        assert!(find(&shown, "D3").is_some() && find(&shown, "C#3").is_some() && find(&shown, "D#3").is_none());

        let fit = find(&shown, "A la escala").expect("an «A la escala» button");
        click(&mut app, view, fit, egui::Modifiers::NONE);
        let Some(Clip::Midi(midi)) = app.clip(clip) else { panic!() };
        assert_eq!(midi.notes.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![60, 64], "C# to C, E stays");
    }

    /// A drum clip's editor has «Generar ritmo»; a bass clip's, not.
    #[test]
    fn drum_clips_can_generate_a_beat() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        app.select_clip(ClipRef::Session { track: 0, slot: 0 });
        assert!(texts(&mut app).iter().any(|(t, _)| t == "Generar ritmo"));
        app.select_clip(ClipRef::Session { track: 1, slot: 0 });
        assert!(!texts(&mut app).iter().any(|(t, _)| t == "Generar ritmo"));
    }

    /// The strip under the ruler names the demo's «Am F C G» (C over G), with
    /// their degrees once the project has a key.
    #[test]
    fn the_chord_strip_names_each_bar() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let t = app.project.tracks.iter().position(|t| t.clips[0].as_ref().is_some_and(|c| c.name() == "Am F C G")).expect("the keys");
        app.select_clip(ClipRef::Session { track: t, slot: 0 });
        let names = |app: &mut PulseApp| -> Vec<String> { texts(app).into_iter().map(|(t, _)| t).filter(|t| t.starts_with(['L', 'F', 'D', 'S'])).collect() };
        let shown = names(&mut app);
        for chord in ["Lam", "Fa", "Do/Sol", "Sol"] {
            assert!(shown.contains(&chord.to_string()), "{chord}: {shown:?}");
        }
        app.project.key = Some(Scale { tonic: 0, minor: false });
        let shown = names(&mut app);
        for chord in ["Lam · vi", "Fa · IV", "Do/Sol · I", "Sol · V"] {
            assert!(shown.contains(&chord.to_string()), "{chord}: {shown:?}");
        }
    }
}
