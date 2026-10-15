//! Selecting several tracks (⌘ clic adds or removes one, ⇧ clic a range) and
//! dragging their headers to put them somewhere else, in both views.

use crate::app::{ClipRef, PulseApp};
use crate::midi_map::Control;
use crate::theme;
use eframe::egui::{self, pos2, Rect, Stroke};
use engine::{Command, Target, MAX_TRACKS};

/// Headers being dragged: the tracks that move and the gap they would land in.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackDrag {
    pub tracks: Vec<usize>,
    pub gap: Option<usize>,
}

impl PulseApp {
    /// The selected tracks, in order. The selected one is always among them; a
    /// return or the master selected means no tracks.
    pub fn track_selection(&self) -> Vec<usize> {
        match self.selected_target {
            Target::Track(t) if self.selected_tracks.contains(&t) => self.selected_tracks.clone(),
            Target::Track(t) => vec![t],
            _ => Vec::new(),
        }
    }

    pub fn is_track_selected(&self, t: usize) -> bool {
        match self.selected_target {
            Target::Track(s) if self.selected_tracks.contains(&s) => self.selected_tracks.contains(&t),
            target => target == Target::Track(t),
        }
    }

    /// A clic on a track header: alone, with ⌘ (add or remove it) or ⇧ (a range).
    pub fn click_track(&mut self, t: usize, modifiers: egui::Modifiers) {
        let mut selection = self.track_selection();
        if modifiers.command {
            if let Some(i) = selection.iter().position(|&s| s == t) {
                if selection.len() > 1 {
                    selection.remove(i);
                    if self.selected_target == Target::Track(t) {
                        self.selected_target = Target::Track(selection[0]);
                    }
                }
            } else {
                selection.push(t);
                selection.sort_unstable();
                self.selected_target = Target::Track(t);
            }
            self.selected_tracks = selection;
        } else if modifiers.shift {
            // From the selected track (which stays the anchor) to this one.
            let anchor = match self.selected_target {
                Target::Track(a) => a,
                _ => t,
            };
            self.selected_tracks = (anchor.min(t)..=anchor.max(t)).collect();
            self.selected_target = Target::Track(anchor);
        } else {
            self.selected_tracks = vec![t];
            self.selected_target = Target::Track(t);
        }
    }

    /// Moves `tracks` together, in their order, to the gap before track `gap` (the
    /// track count for the end). Returns whether anything moved.
    pub fn move_tracks(&mut self, tracks: &[usize], gap: usize) -> bool {
        let n = self.project.tracks.len();
        let mut moving: Vec<usize> = tracks.iter().copied().filter(|&t| t < n).collect();
        moving.sort_unstable();
        moving.dedup();
        if moving.is_empty() || gap > n {
            return false;
        }
        if self.recording_anything() {
            self.status = Some("No se pueden mover pistas mientras se graba".into());
            return false;
        }
        let rest: Vec<usize> = (0..n).filter(|t| !moving.contains(t)).collect();
        let at = gap - moving.iter().filter(|&&t| t < gap).count();
        let order: Vec<usize> = rest[..at].iter().chain(&moving).chain(&rest[at..]).copied().collect();
        if order.iter().enumerate().all(|(i, &o)| i == o) {
            return false;
        }
        self.reorder_tracks(&order);
        self.status = Some(match moving.len() {
            1 => "Pista movida".to_string(),
            k => format!("{k} pistas movidas"),
        });
        true
    }

    /// Puts the tracks in `order` (new track `i` is the old `order[i]`), with
    /// everything that points at them, here and in the engine.
    fn reorder_tracks(&mut self, order: &[usize]) {
        let new = |old: usize| order.iter().position(|&o| o == old).unwrap_or(old);
        let mut old: Vec<Option<_>> = std::mem::take(&mut self.project.tracks).into_iter().map(Some).collect();
        self.project.tracks = order.iter().map(|&o| old[o].take().expect("a permutation")).collect();

        let track = |target: Target| match target {
            Target::Track(t) => Target::Track(new(t)),
            other => other,
        };
        self.selected_target = track(self.selected_target);
        self.selected_tracks = self.selected_tracks.iter().map(|&t| new(t)).collect();
        self.selected_tracks.sort_unstable();
        let clip = |clip: ClipRef| match clip {
            ClipRef::Session { track, slot } => ClipRef::Session { track: new(track), slot },
            ClipRef::Arrangement { track, index } => ClipRef::Arrangement { track: new(track), index },
        };
        self.selected_clip = self.selected_clip.map(clip);
        self.selected_clips = self.selected_clips.iter().map(|&c| clip(c)).collect();
        self.arrangement.automation = self.arrangement.automation.drain().map(|(t, p)| (new(t), p)).collect();
        self.overridden = self.overridden.drain().map(|(target, p)| (track(target), p)).collect();
        let control = |control: &mut Control| {
            if let Control::Param { target, .. } = control {
                *target = track(*target);
            }
        };
        for mapping in &mut self.project.midi_map {
            control(&mut mapping.control);
        }
        if let Some(armed) = &mut self.learn.armed {
            control(armed);
        }
        for window in [&mut self.kick_window, &mut self.wave_window, &mut self.poly_window, &mut self.modular_window] {
            *window = window.map(new);
        }
        if let Some(editor) = &mut self.table_editor {
            editor.track = new(editor.track);
        }
        let meters = self.meters.tracks;
        for (i, &o) in order.iter().enumerate() {
            self.meters.tracks[i] = meters[o];
        }

        let mut engine_order: [usize; MAX_TRACKS] = std::array::from_fn(|i| i);
        engine_order[..order.len()].copy_from_slice(order);
        self.audio.send(Command::ReorderTracks(engine_order));
        // Sent again from the new indices next frame.
        self.input_track = None;
        self.sync_midi_map();
    }

    fn recording_anything(&self) -> bool {
        !self.recorder.session.is_empty()
            || self.recorder.arrangement.is_some()
            || self.audio.state().is_some_and(|s| {
                s.tracks.iter().any(|t| t.recording.load(std::sync::atomic::Ordering::Relaxed) != engine::NONE || t.queued_record.load(std::sync::atomic::Ordering::Relaxed))
            })
    }

    /// Deletes the selected tracks (one is always kept).
    pub fn delete_selected_tracks(&mut self) {
        // From the last, so the earlier indices stay put; `delete_track` keeps the last track.
        for &t in self.track_selection().iter().rev() {
            self.delete_track(t);
        }
        self.selected_tracks.clear();
    }

    /// Duplicates the selected tracks, each copy right after its track.
    pub fn duplicate_selected_tracks(&mut self) {
        let selection = self.track_selection();
        let mut copies = Vec::new();
        // From the last, so the earlier indices stay put.
        for &t in selection.iter().rev() {
            let before = self.project.tracks.len();
            self.duplicate_track(t);
            if self.project.tracks.len() > before {
                copies.iter_mut().for_each(|c: &mut usize| *c += 1);
                copies.push(t + 1);
            }
        }
        if let Some(&first) = copies.iter().min() {
            copies.sort_unstable();
            self.selected_tracks = copies;
            self.selected_target = Target::Track(first);
        }
    }

    /// Handles a header's clics and drags. Call it for every track header, in
    /// order; `rect` is where the header is drawn.
    pub fn track_header_input(&mut self, t: usize, response: &egui::Response) {
        if response.drag_started() {
            if !self.is_track_selected(t) {
                self.click_track(t, egui::Modifiers::NONE);
            }
            self.track_drag = Some(TrackDrag { tracks: self.track_selection(), gap: None });
        } else if response.clicked() {
            let modifiers = response.ctx.input(|i| i.modifiers);
            self.click_track(t, modifiers);
        }
    }

    /// After the headers are drawn: works out where dragged tracks would land
    /// from the headers' rects (in track order), draws a line there, and moves
    /// them when the button is released. `vertical` is for headers stacked
    /// top to bottom (arrangement); otherwise they go left to right (session).
    pub fn finish_track_drag(&mut self, ui: &egui::Ui, headers: &[Rect], vertical: bool) {
        let Some(drag) = &mut self.track_drag else { return };
        let Some(pos) = ui.ctx().pointer_latest_pos() else { return };
        let along = |r: &Rect| if vertical { (r.top(), r.bottom()) } else { (r.left(), r.right()) };
        let p = if vertical { pos.y } else { pos.x };
        drag.gap = Some(headers.iter().position(|r| p < (along(r).0 + along(r).1) / 2.0).unwrap_or(headers.len()));
        if let Some(gap) = drag.gap {
            if let (Some(first), Some(last)) = (headers.first(), headers.last()) {
                let edge = match headers.get(gap) {
                    Some(r) => along(r).0 - 1.0,
                    None => along(last).1 + 1.0,
                };
                let line = if vertical {
                    [pos2(first.left(), edge), pos2(first.right(), edge)]
                } else {
                    [pos2(edge, first.top()), pos2(edge, first.bottom())]
                };
                ui.ctx()
                    .layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("track_drop")))
                    .line_segment(line, Stroke::new(2.0, theme::SELECT));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }
        if !ui.input(|i| i.pointer.any_down()) {
            let drag = self.track_drag.take().expect("checked above");
            if let Some(gap) = drag.gap {
                self.move_tracks(&drag.tracks, gap);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::project::Project;
    use egui::Modifiers;

    fn names(app: &PulseApp) -> Vec<String> {
        app.project.tracks.iter().map(|t| t.name.clone()).collect()
    }

    fn command() -> Modifiers {
        Modifiers { command: true, mac_cmd: true, ..Modifiers::NONE }
    }

    #[test]
    fn clics_select_one_toggle_with_command_and_ranges_with_shift() {
        let mut app = PulseApp::headless(Project::demo());
        assert!(app.project.tracks.len() >= 4);
        app.click_track(1, Modifiers::NONE);
        assert_eq!(app.track_selection(), vec![1]);
        app.click_track(3, command());
        assert_eq!(app.track_selection(), vec![1, 3]);
        assert_eq!(app.selected_target, Target::Track(3));
        app.click_track(3, command());
        assert_eq!(app.track_selection(), vec![1]);
        assert_eq!(app.selected_target, Target::Track(1));
        app.click_track(1, command());
        assert_eq!(app.track_selection(), vec![1], "the last one stays selected");
        app.click_track(3, Modifiers::SHIFT);
        assert_eq!(app.track_selection(), vec![1, 2, 3]);
        assert!(app.is_track_selected(2) && !app.is_track_selected(0));
        // Selecting something else the usual way leaves one track selected.
        app.selected_target = Target::Track(0);
        assert_eq!(app.track_selection(), vec![0]);
        assert!(!app.is_track_selected(2));
        app.selected_target = Target::Master;
        assert!(app.track_selection().is_empty());
    }

    #[test]
    fn moving_tracks_carries_what_points_at_them() {
        let mut app = PulseApp::headless(Project::demo());
        let before = names(&app);
        let n = before.len();
        app.click_track(0, Modifiers::NONE);
        app.click_track(2, command());
        app.selected_clip = Some(ClipRef::Session { track: 2, slot: 0 });
        app.arrangement.automation.insert(0, engine::model::Param::Volume);
        app.project.midi_map.push(crate::midi_map::Mapping { cc: 20, control: Control::Param { target: Target::Track(2), param: engine::model::Param::Pan } });
        app.kick_window = Some(1);

        assert!(app.move_tracks(&[0, 2], n), "to the end");
        let mut expected: Vec<String> = before.iter().enumerate().filter(|(i, _)| *i != 0 && *i != 2).map(|(_, n)| n.clone()).collect();
        expected.extend([before[0].clone(), before[2].clone()]);
        assert_eq!(names(&app), expected);
        assert_eq!(app.track_selection(), vec![n - 2, n - 1]);
        assert_eq!(app.selected_target, Target::Track(n - 1));
        assert_eq!(app.selected_clip, Some(ClipRef::Session { track: n - 1, slot: 0 }));
        assert_eq!(app.arrangement.automation.keys().copied().collect::<Vec<_>>(), vec![n - 2]);
        assert_eq!(app.project.midi_map[0].control, Control::Param { target: Target::Track(n - 1), param: engine::model::Param::Pan });
        assert_eq!(app.kick_window, Some(0));

        // Back to the start: the original order.
        assert!(app.move_tracks(&[n - 2], 0));
        assert!(app.move_tracks(&[n - 1], 2));
        assert_eq!(names(&app), before);
        // Dropping tracks where they already are changes nothing.
        assert!(!app.move_tracks(&[1, 2], 1));
        assert!(!app.move_tracks(&[1, 2], 3));
        assert!(!app.move_tracks(&[], 0));
        assert!(!app.move_tracks(&[0], n + 1));
        assert_eq!(names(&app), before);
    }

    #[test]
    fn several_tracks_are_duplicated_and_deleted_together() {
        let mut app = PulseApp::headless(Project::demo());
        let before = names(&app);
        app.click_track(0, Modifiers::NONE);
        app.click_track(1, Modifiers::SHIFT);
        app.duplicate_selected_tracks();
        let after = names(&app);
        assert_eq!(after.len(), before.len() + 2);
        assert_eq!(after[1], format!("{} copia", before[0]));
        assert_eq!(after[3], format!("{} copia", before[1]));
        assert_eq!(app.track_selection(), vec![1, 3], "the copies end up selected");
        app.delete_selected_tracks();
        assert_eq!(names(&app), before);

        // Deleting every track keeps one.
        let n = app.project.tracks.len();
        app.click_track(0, Modifiers::NONE);
        app.click_track(n - 1, Modifiers::SHIFT);
        app.delete_selected_tracks();
        assert_eq!(app.project.tracks.len(), 1);
    }

    #[test]
    fn dragging_headers_moves_the_selection() {
        // Four headers side by side, as in the session view.
        let headers: Vec<Rect> = (0..4).map(|i| Rect::from_min_size(pos2(i as f32 * 100.0, 0.0), egui::vec2(98.0, 20.0))).collect();
        let mut app = PulseApp::headless(Project::demo());
        app.project.tracks.truncate(4);
        app.sync_all();
        let before = names(&app);
        app.click_track(0, Modifiers::NONE);
        app.click_track(1, command());
        app.track_drag = Some(TrackDrag { tracks: app.track_selection(), gap: None });
        let ctx = egui::Context::default();
        let mut held = false;
        let mut frame = |app: &mut PulseApp, x: f32, down: bool| {
            let mut input = egui::RawInput::default();
            input.events.push(egui::Event::PointerMoved(pos2(x, 10.0)));
            let pressed = down && !std::mem::replace(&mut held, true);
            if pressed || !down {
                input.events.push(egui::Event::PointerButton { pos: pos2(x, 10.0), button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            }
            let _ = ctx.run_ui(input, |ui| app.finish_track_drag(ui, &headers, false));
        };
        frame(&mut app, 260.0, true);
        assert_eq!(app.track_drag.as_ref().and_then(|d| d.gap), Some(3), "past the middle of the third header");
        frame(&mut app, 380.0, true);
        assert_eq!(app.track_drag.as_ref().and_then(|d| d.gap), Some(4));
        frame(&mut app, 380.0, false);
        assert!(app.track_drag.is_none());
        assert_eq!(names(&app), vec![before[2].clone(), before[3].clone(), before[0].clone(), before[1].clone()]);
        assert_eq!(app.track_selection(), vec![2, 3]);
    }

    /// Presses at `from`, moves in steps to `to` and releases, drawing `view` each frame.
    pub(crate) fn drag(app: &mut PulseApp, view: fn(&mut PulseApp, &mut egui::Ui), from: egui::Pos2, to: egui::Pos2, modifiers: Modifiers) {
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut frame = |app: &mut PulseApp, events: Vec<egui::Event>| {
            time += 0.05;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1600.0, 900.0))),
                time: Some(time),
                events: [egui::Event::ModifiersChanged(modifiers)].into_iter().chain(events).collect(),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| view(app, ui));
        };
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers };
        frame(app, vec![egui::Event::PointerMoved(from)]);
        frame(app, vec![button(from, true)]);
        for step in 1..=10 {
            frame(app, vec![egui::Event::PointerMoved(from + (to - from) * step as f32 / 10.0)]);
        }
        frame(app, vec![button(to, false)]);
        frame(app, vec![egui::Event::PointerGone]);
    }

    pub(crate) fn click(app: &mut PulseApp, view: fn(&mut PulseApp, &mut egui::Ui), at: egui::Pos2, modifiers: Modifiers) {
        drag(app, view, at, at, modifiers);
    }

    #[test]
    fn headers_in_both_views_select_and_drag_with_the_mouse() {
        for (view, header) in [
            (crate::session::show as fn(&mut PulseApp, &mut egui::Ui), (|t: usize| pos2(54.0 + t as f32 * 109.0, 11.0)) as fn(usize) -> egui::Pos2),
            (crate::arrangement::show, |t: usize| pos2(75.0, 81.0 + t as f32 * 53.0)),
        ] {
            let mut app = PulseApp::headless(Project::demo());
            app.project.tracks.truncate(4);
            app.sync_all();
            let before = names(&app);
            click(&mut app, view, header(1), Modifiers::NONE);
            assert_eq!(app.track_selection(), vec![1]);
            click(&mut app, view, header(3), command());
            assert_eq!(app.track_selection(), vec![1, 3]);
            // ⇧ goes from the track clicked last.
            click(&mut app, view, header(1), Modifiers::SHIFT);
            assert_eq!(app.track_selection(), vec![1, 2, 3]);
            click(&mut app, view, header(0), Modifiers::NONE);
            click(&mut app, view, header(1), command());
            assert_eq!(app.track_selection(), vec![0, 1]);
            // Drag the two past the last track.
            drag(&mut app, view, header(0), header(3) + (header(3) - header(2)) * 0.4, Modifiers::NONE);
            assert!(app.track_drag.is_none());
            assert_eq!(names(&app), vec![before[2].clone(), before[3].clone(), before[0].clone(), before[1].clone()]);
            assert_eq!(app.track_selection(), vec![2, 3]);
            // Dragging an unselected header moves only that one.
            drag(&mut app, view, header(1), header(0) - (header(1) - header(0)) * 0.4, Modifiers::NONE);
            assert_eq!(names(&app), vec![before[3].clone(), before[2].clone(), before[0].clone(), before[1].clone()]);
            assert_eq!(app.track_selection(), vec![0]);
        }
    }
}
