//! The tuning editor: a recorded voice's notes as blocks over the keys, at
//! the pitch they are taken to, with the pitch sung drawn through them.
//! Dragging a note up or down moves it (to whole semitones; with ⌥, freely);
//! double-click tunes it. The voice is rendered again when the mouse is let
//! go (see `tuning`).

use crate::app::{ClipRef, PulseApp};
use crate::theme::{self, TEXT_DIM};
use crate::tuning::TunedNote;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine::model::Clip;

/// Room on the left for the notes' names.
const KEYS: f32 = 38.0;

/// The note `pitch` is taken to when tuned: the project's scale's nearest,
/// or the nearest semitone.
fn tuned(pitch: f32, key: Option<stems::key::Key>) -> f32 {
    match key {
        Some(k) => k.snap(pitch as f64) as f32,
        None => pitch.round(),
    }
}

/// How far `pitch` is from its nearest semitone, as «La3 −23 cents».
fn describe(pitch: f32) -> String {
    let note = pitch.round().clamp(0.0, 127.0) as u8;
    let cents = ((pitch - pitch.round()) * 100.0).round() as i32;
    if cents == 0 { crate::project::note_name(note) } else { format!("{} {cents:+} cents", crate::project::note_name(note)) }
}

impl PulseApp {
    /// Changes the notes of `clip` with `change`, and renders it.
    fn edit_tuning(&mut self, clip: ClipRef, change: impl FnOnce(&mut Vec<TunedNote>)) {
        let Some(mut notes) = self.tuning_notes(clip) else { return };
        change(&mut notes);
        if let Err(error) = self.apply_tuning(clip, notes) {
            self.status = Some(error);
        }
    }
}

/// The notes a button acts on: those chosen, or all.
fn chosen(selected: &std::collections::HashSet<usize>, i: usize) -> bool {
    selected.is_empty() || selected.contains(&i)
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui, clip_ref: ClipRef, playhead: Option<f64>) {
    let tempo = app.project.tempo;
    let Some(Clip::Audio(clip)) = app.clip(clip_ref).cloned() else {
        app.tuner.open = None;
        return;
    };
    let key = app.project.key;
    let source = app.tuning_source(clip_ref).unwrap_or_default();
    let analysis = app.tuner.analysis(&source);
    let notes = app.tuning_notes(clip_ref);
    ui.horizontal(|ui| {
        if ui.button("← Forma de onda").clicked() {
            app.tuner.open = None;
        }
        ui.label(egui::RichText::new(format!("Afinación · {}", clip.name)).strong());
        let Some(notes) = &notes else { return };
        ui.add_space(12.0);
        let selected = app.tuner.selected.clone();
        let scope = if selected.is_empty() { "todas" } else { "las elegidas" };
        let hint = match key {
            Some(k) => format!("Lleva {scope} a la nota de la escala ({}) más cercana", k.name()),
            None => format!("Lleva {scope} al semitono más cercano"),
        };
        if ui.button("Afinar").on_hover_text(hint).clicked() {
            app.edit_tuning(clip_ref, |n| {
                for (i, note) in n.iter_mut().enumerate().filter(|(i, _)| chosen(&selected, *i)) {
                    let _ = i;
                    note.target = tuned(note.pitch, key);
                }
            });
        }
        if ui.button("Sin vibrato").on_hover_text(format!("Quita el vibrato y las oscilaciones de {scope}: cada nota se queda en su tono")).clicked() {
            app.edit_tuning(clip_ref, |n| {
                for (_, note) in n.iter_mut().enumerate().filter(|(i, _)| chosen(&selected, *i)) {
                    note.modulation = 0.0;
                }
            });
        }
        if ui.button("Como se cantó").on_hover_text(format!("Devuelve {scope} a como se cantaron")).clicked() {
            app.edit_tuning(clip_ref, |n| {
                for (_, note) in n.iter_mut().enumerate().filter(|(i, _)| chosen(&selected, *i)) {
                    note.target = note.pitch;
                    note.modulation = 1.0;
                }
            });
        }
        // The note chosen: where it was sung, where it goes, and its vibrato.
        if let [i] = selected.iter().copied().collect::<Vec<_>>()[..] {
            if let Some(note) = notes.get(i) {
                ui.add_space(12.0);
                ui.label(egui::RichText::new(format!("{} → {}", describe(note.pitch), describe(note.target))).size(11.0));
                let mut vibrato = note.modulation * 100.0;
                let response = ui.add(egui::Slider::new(&mut vibrato, 0.0..=150.0).suffix(" %").text("vibrato")).on_hover_text("Cuánto de lo que oscila la voz en esta nota se queda: 100 % como se cantó, 0 % nada");
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    app.edit_tuning(clip_ref, |n| n[i].modulation = vibrato / 100.0);
                }
            }
        }
    });
    ui.add_space(4.0);
    let Some(analysis) = analysis else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(egui::RichText::new("Analizando la voz…").color(TEXT_DIM));
        });
        return;
    };
    let mut notes = notes.unwrap_or_default();

    let size = vec2(ui.available_width().max(200.0), (ui.available_height() - 8.0).max(120.0));
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::SLOT);
    // What the clip plays of the file.
    let (v0, v1) = (clip.offset.max(0.0), clip.source_seconds(clip.length, tempo).max(clip.offset + 0.05));
    let shown: Vec<usize> = (0..notes.len()).filter(|&i| notes[i].end > v0 && notes[i].start < v1).collect();
    if shown.is_empty() {
        painter.text(rect.center(), Align2::CENTER_CENTER, "No se oyen notas en este clip", theme::body(), TEXT_DIM);
        return;
    }
    // The notes shown, sung and moved, with room around.
    let pitches = shown.iter().flat_map(|&i| [notes[i].pitch, notes[i].target]);
    let (lo, hi) = pitches.fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p), hi.max(p)));
    let mid = (lo + hi) * 0.5;
    let span = (hi - lo + 5.0).max(12.0);
    let (bottom, top) = ((mid - span / 2.0).floor(), (mid + span / 2.0).ceil());
    let row = rect.height() / (top - bottom);
    let area = Rect::from_min_max(pos2(rect.left() + KEYS, rect.top()), rect.max);
    let x_of = |seconds: f64| area.left() + ((seconds - v0) / (v1 - v0)) as f32 * area.width();
    let y_of = |pitch: f32| rect.bottom() - (pitch - bottom) * row;

    // The keys: a row per semitone, the black keys darker, the C's named.
    for p in bottom as i32..top as i32 {
        let y = y_of(p as f32 + 0.5);
        let black = matches!(p.rem_euclid(12), 1 | 3 | 6 | 8 | 10);
        let band = Rect::from_center_size(pos2(rect.center().x, y), vec2(rect.width(), row));
        if black {
            painter.rect_filled(band, 0.0, theme::shade(theme::SLOT, 0.8));
        }
        if p.rem_euclid(12) == 0 || row >= 14.0 {
            painter.text(pos2(rect.left() + 3.0, y), Align2::LEFT_CENTER, crate::project::note_name(p.clamp(0, 127) as u8), theme::small(), if black { TEXT_DIM } else { Color32::from_gray(190) });
        }
        if p.rem_euclid(12) == 0 {
            painter.line_segment([pos2(area.left(), y_of(p as f32)), pos2(rect.right(), y_of(p as f32))], Stroke::new(1.0, Color32::from_gray(70)));
        }
    }
    painter.line_segment([pos2(area.left(), rect.top()), pos2(area.left(), rect.bottom())], Stroke::new(1.0, Color32::from_gray(60)));

    // Dragging: the notes as they will be (shown before they are rendered).
    let drag_id = response.id.with("tuning_drag");
    let dragging: Option<(usize, f32, Vec<(usize, f32)>)> = ui.data(|d| d.get_temp(drag_id));
    if let (Some((_, y0, starts)), Some(pos)) = (&dragging, response.interact_pointer_pos()) {
        let delta = (y0 - pos.y) / row;
        let free = ui.input(|i| i.modifiers.alt);
        for &(i, start) in starts {
            notes[i].target = if free { start + delta } else { (start + delta).round() };
        }
    }

    // The pitch sung (grey) and as it will sound (the clip's colour).
    let colour = theme::rgb(clip.color);
    let steps = (area.width() / 2.0) as usize;
    let mut sung = Vec::new();
    let mut moved = Vec::new();
    let flush = |line: &mut Vec<egui::Pos2>, stroke: Stroke| {
        if line.len() > 1 {
            painter.add(egui::Shape::line(std::mem::take(line), stroke));
        }
        line.clear();
    };
    for k in 0..=steps {
        let t = v0 + (v1 - v0) * k as f64 / steps as f64;
        let x = x_of(t);
        match analysis.pitch_at(t) {
            Some(p) => {
                sung.push(pos2(x, y_of(p)));
                match notes.iter().find(|n| n.start <= t && t < n.end) {
                    Some(n) if n.edited() => moved.push(pos2(x, y_of(n.target + n.modulation * (p - n.pitch)))),
                    _ => flush(&mut moved, Stroke::new(1.5, colour)),
                }
            }
            None => {
                flush(&mut sung, Stroke::new(1.0, Color32::from_gray(140)));
                flush(&mut moved, Stroke::new(1.5, colour));
            }
        }
    }
    flush(&mut sung, Stroke::new(1.0, Color32::from_gray(140)));
    flush(&mut moved, Stroke::new(1.5, colour));

    // The notes, where they are taken.
    let selected = app.tuner.selected.clone();
    let blocks: Vec<(usize, Rect)> = shown
        .iter()
        .map(|&i| {
            let n = &notes[i];
            let r = Rect::from_min_max(pos2(x_of(n.start).max(area.left()), y_of(n.target + 0.4)), pos2(x_of(n.end).min(area.right()), y_of(n.target - 0.4)));
            (i, r)
        })
        .collect();
    for &(i, r) in &blocks {
        let on = selected.contains(&i);
        painter.rect_filled(r, 3.0, theme::shade(colour, if on { 0.95 } else { 0.55 }).gamma_multiply(0.85));
        if on {
            painter.rect_stroke(r, 3.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
        }
        if r.width() > 34.0 && row >= 9.0 {
            painter.text(pos2(r.left() + 3.0, r.center().y), Align2::LEFT_CENTER, describe(notes[i].target), theme::small(), Color32::WHITE);
        }
    }
    if let Some(beats) = playhead {
        let x = x_of(clip.source_seconds(beats, tempo));
        if x >= area.left() {
            painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.5, Color32::WHITE));
        }
    }
    painter.text(pos2(rect.right() - 4.0, rect.top() + 3.0), Align2::RIGHT_TOP, "Arrastra una nota para cambiarla (⌥: sin ajustar a semitonos) · doble clic: afinarla", theme::small(), TEXT_DIM);

    // Picking, dragging, tuning one.
    let under = |pos: egui::Pos2| blocks.iter().find(|(_, r)| r.expand(2.0).contains(pos)).map(|&(i, _)| i);
    let adding = ui.input(|i| i.modifiers.shift || i.modifiers.command);
    if response.double_clicked() {
        if let Some(i) = response.interact_pointer_pos().and_then(under) {
            app.edit_tuning(clip_ref, |n| n[i].target = tuned(n[i].pitch, key));
        }
    } else if response.clicked() {
        match response.interact_pointer_pos().and_then(under) {
            Some(i) if adding => {
                if !app.tuner.selected.remove(&i) {
                    app.tuner.selected.insert(i);
                }
            }
            Some(i) => app.tuner.selected = [i].into(),
            None => app.tuner.selected.clear(),
        }
    }
    if response.drag_started() {
        if let Some(pos) = response.interact_pointer_pos() {
            if let Some(i) = under(pos) {
                if !app.tuner.selected.contains(&i) {
                    app.tuner.selected = [i].into();
                }
                let starts: Vec<(usize, f32)> = app.tuner.selected.iter().filter_map(|&j| notes.get(j).map(|n| (j, n.target))).collect();
                ui.data_mut(|d| d.insert_temp(drag_id, (i, pos.y, starts)));
            }
        }
    }
    if response.drag_stopped() {
        ui.data_mut(|d| d.remove::<(usize, f32, Vec<(usize, f32)>)>(drag_id));
        if dragging.is_some() {
            let moved = notes.clone();
            if let Err(error) = app.apply_tuning(clip_ref, moved) {
                app.status = Some(error);
            }
        }
    }
}
