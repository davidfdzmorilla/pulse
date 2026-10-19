//! Several clips at once: ⌘ clic adds or removes one, a rectangle drawn from an
//! empty place selects the clips and tracks it touches (in both views), and
//! session clips dragged together land in other slots (⌥ copies them).

use crate::app::{ClipRef, PulseApp, View};
use engine::model::{ArrangementClip, Clip};
use crate::theme;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use engine::Target;

/// Moves less than this (in points) are clics, not rectangles.
const MARQUEE_MIN: f32 = 4.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Gesture {
    /// A selection rectangle from `anchor`. With ⌘ or ⇧ held at the start, it adds
    /// to the selection there was (`base`).
    Marquee { anchor: Pos2, base: Vec<ClipRef>, base_tracks: Vec<usize>, add: bool },
    /// Session clips being dragged, grabbed at slot `from`; `to` is the slot under
    /// the pointer.
    SessionMove { from: (usize, usize), to: Option<(usize, usize)>, copy: bool },
}

/// Copied clips, placed relative to each other so they paste in the same layout.
/// Each has a place in both views: a slot for the session and a start for the
/// arrangement (clips from one view are laid out one after another in the other).
#[derive(Clone, Debug, PartialEq)]
pub struct Clipboard {
    pub items: Vec<CopiedClip>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CopiedClip {
    /// Tracks, slots and beats from the first copied clip's.
    pub track: usize,
    pub slot: usize,
    pub start: f64,
    pub length: f64,
    pub clip: Clip,
}

impl PulseApp {
    /// Copies the selected clips.
    pub fn copy_clips(&mut self) {
        let selection = self.clip_selection();
        let Some(&first) = selection.first() else { return };
        let first_track = selection.iter().map(|c| c.track()).min().unwrap_or(0);
        let first_slot = selection.iter().filter_map(|c| match *c {
            ClipRef::Session { slot, .. } => Some(slot),
            ClipRef::Arrangement { .. } => None,
        }).min().unwrap_or(0);
        let first_start = selection.iter().filter_map(|&c| self.placed(c).map(|p| p.start)).fold(f64::INFINITY, f64::min);
        // (track, slot, start, length, clip), the missing places filled in below.
        let mut raw: Vec<(usize, Option<usize>, Option<f64>, f64, Clip)> = Vec::new();
        for &c in &selection {
            let track = c.track() - first_track;
            match c {
                ClipRef::Session { slot, .. } => {
                    let Some(clip) = self.clip(c).cloned() else { continue };
                    raw.push((track, Some(slot - first_slot), None, clip.length().max(1.0), clip));
                }
                ClipRef::Arrangement { .. } => {
                    let Some(placed) = self.placed(c).cloned() else { continue };
                    raw.push((track, None, Some(placed.start - first_start), placed.length, placed.clip));
                }
            }
        }
        // Arrangement clips take the slots after the session ones of their track, in
        // time order; session clips go after the arrangement ones, in slot order.
        raw.sort_by(|a, b| (a.0, a.1, a.2).partial_cmp(&(b.0, b.1, b.2)).unwrap_or(std::cmp::Ordering::Equal));
        let mut items: Vec<CopiedClip> = Vec::new();
        for track in raw.iter().map(|r| r.0).collect::<std::collections::BTreeSet<_>>() {
            let lane: Vec<_> = raw.iter().filter(|r| r.0 == track).collect();
            let mut next_slot = lane.iter().filter_map(|r| r.1).max().map_or(0, |s| s + 1);
            let mut next_start = lane.iter().filter_map(|r| r.2.map(|s| s + r.3)).fold(0.0, f64::max);
            for &(_, slot, start, length, ref clip) in lane {
                let slot = slot.unwrap_or_else(|| {
                    next_slot += 1;
                    next_slot - 1
                });
                let start = start.unwrap_or_else(|| {
                    next_start += length;
                    next_start - length
                });
                items.push(CopiedClip { track, slot, start, length, clip: clip.clone() });
            }
        }
        self.status = Some(match items.len() {
            1 => format!("Copiado «{}»", self.clip(first).map_or("", |c| c.name())),
            n => format!("{n} clips copiados"),
        });
        self.clip_clipboard = Some(Clipboard { items });
    }

    fn placed(&self, clip: ClipRef) -> Option<&ArrangementClip> {
        match clip {
            ClipRef::Arrangement { track, index } => self.project.tracks.get(track)?.arrangement.get(index),
            ClipRef::Session { .. } => None,
        }
    }

    /// Pastes the copied clips in the same layout. In the session, from the selected
    /// slot down (lower if they don't fit there); on the arrangement, after the
    /// selected clips, or at the bar of the playhead if none is selected. The first
    /// track is the selected one.
    pub fn paste_clips(&mut self) {
        let to_arrangement = match self.selected_clip {
            Some(ClipRef::Arrangement { .. }) => true,
            Some(ClipRef::Session { .. }) => false,
            None => self.view == View::Arrangement,
        };
        let slot = self.paste_slot.filter(|_| self.selected_clip.is_none() && !to_arrangement);
        let first_track = match (self.selected_clip, slot, self.selected_target) {
            (Some(c), _, _) => c.track(),
            (None, Some((t, _)), _) => t,
            (None, None, Target::Track(t)) => t,
            _ => 0,
        };
        self.paste_into(to_arrangement, first_track, None);
    }

    /// Pastes the copied clips on the arrangement, the first on `track` at `start`.
    pub fn paste_clips_at(&mut self, track: usize, start: f64) {
        self.paste_into(true, track, Some(start));
    }

    fn paste_into(&mut self, to_arrangement: bool, first_track: usize, at: Option<f64>) {
        let Some(board) = self.clip_clipboard.clone() else { return };
        let tracks = self.project.tracks.len();
        let fits = |app: &PulseApp, item: &CopiedClip| {
            let t = first_track + item.track;
            t < tracks && !app.project.tracks[t].is_group() && matches!(item.clip, Clip::Audio(_)) == app.project.tracks[t].is_audio()
        };
        if !board.items.iter().all(|item| fits(self, item)) {
            self.status = Some("Los clips no caben: faltan pistas o son de otro tipo (audio o MIDI)".into());
            return;
        }
        let mut pasted = Vec::new();
        if to_arrangement {
            let selected_end = self.clip_selection().iter().filter_map(|&c| self.placed(c).map(|p| p.end())).fold(f64::NEG_INFINITY, f64::max);
            let start = match at {
                Some(start) => start,
                None if selected_end.is_finite() => selected_end,
                None => (self.audio.position() / 4.0).floor() * 4.0,
            };
            for item in &board.items {
                let t = first_track + item.track;
                let lane = &mut self.project.tracks[t].arrangement;
                lane.push(ArrangementClip { start: start + item.start, length: item.length, clip: item.clip.clone() });
                pasted.push(ClipRef::Arrangement { track: t, index: lane.len() - 1 });
            }
            let mut touched: Vec<usize> = pasted.iter().map(|c| c.track()).collect();
            touched.dedup();
            for t in touched {
                self.sync_arrangement(t);
            }
        } else {
            let first_slot = match (self.selected_clip, self.paste_slot) {
                (Some(ClipRef::Session { slot, .. }), _) => slot,
                (None, Some((_, slot))) => slot,
                _ => 0,
            };
            let place = |down: usize| -> Option<Vec<(usize, usize)>> {
                board
                    .items
                    .iter()
                    .map(|item| {
                        let (t, s) = (first_track + item.track, first_slot + down + item.slot);
                        (s < engine::NUM_SCENES && self.project.tracks[t].clips[s].is_none()).then_some((t, s))
                    })
                    .collect()
            };
            let Some(places) = (0..engine::NUM_SCENES).find_map(place) else {
                self.status = Some("No quedan huecos libres debajo para los clips".into());
                return;
            };
            for (item, &(t, s)) in board.items.iter().zip(&places) {
                self.project.tracks[t].clips[s] = Some(item.clip.clone());
                self.sync_clip(t, s);
                pasted.push(ClipRef::Session { track: t, slot: s });
            }
        }
        if pasted.len() > 1 {
            self.status = Some(format!("{} clips pegados", pasted.len()));
        }
        self.set_clip_selection(pasted);
    }

    /// The selected clips. The selected one is always among them.
    pub fn clip_selection(&self) -> Vec<ClipRef> {
        let Some(primary) = self.selected_clip.filter(|&c| self.clip(c).is_some()) else { return Vec::new() };
        if self.selected_clips.contains(&primary) {
            self.selected_clips.iter().copied().filter(|&c| self.clip(c).is_some()).collect()
        } else {
            vec![primary]
        }
    }

    pub fn is_clip_selected(&self, clip: ClipRef) -> bool {
        match self.selected_clip {
            Some(primary) if self.selected_clips.contains(&primary) => self.selected_clips.contains(&clip),
            primary => primary == Some(clip),
        }
    }

    /// Selects these clips (the first is the one the editor shows) and their tracks.
    pub fn set_clip_selection(&mut self, clips: Vec<ClipRef>) {
        let Some(&first) = clips.first() else {
            self.selected_clip = None;
            self.selected_clips.clear();
            return;
        };
        self.select_clip(first);
        let mut tracks: Vec<usize> = clips.iter().map(|c| c.track()).collect();
        tracks.sort_unstable();
        tracks.dedup();
        self.selected_tracks = tracks;
        self.selected_clips = clips;
    }

    /// A clic on a clip: alone, or with ⌘ or ⇧ to add it to (or take it out of) the selection.
    pub fn click_clip(&mut self, clip: ClipRef, modifiers: egui::Modifiers) {
        if !(modifiers.command || modifiers.shift) {
            return self.select_clip(clip);
        }
        let mut clips = self.clip_selection();
        if let Some(i) = clips.iter().position(|&c| c == clip) {
            if clips.len() > 1 {
                clips.remove(i);
            }
        } else {
            // The clip clicked becomes the one the editor shows.
            clips.insert(0, clip);
        }
        self.set_clip_selection(clips);
    }

    /// Deletes these clips, from both views.
    pub fn delete_clips(&mut self, clips: &[ClipRef]) {
        let mut arrangement: Vec<(usize, usize)> = Vec::new();
        for &clip in clips {
            match clip {
                ClipRef::Session { track, slot } => {
                    if self.project.tracks.get(track).is_some_and(|t| t.clips[slot].is_some()) {
                        self.set_clip(track, slot, None);
                    }
                }
                ClipRef::Arrangement { track, index } => arrangement.push((track, index)),
            }
        }
        // Later indices first, so the earlier ones stay put.
        arrangement.sort_unstable_by(|a, b| b.cmp(a));
        arrangement.dedup();
        for &(track, index) in &arrangement {
            if let Some(lane) = self.project.tracks.get_mut(track).map(|t| &mut t.arrangement) {
                if index < lane.len() {
                    lane.remove(index);
                }
            }
        }
        let mut tracks: Vec<usize> = arrangement.iter().map(|&(t, _)| t).collect();
        tracks.dedup();
        for t in tracks {
            self.sync_arrangement(t);
        }
        if self.selected_clip.is_some_and(|c| clips.contains(&c) || self.clip(c).is_none()) {
            self.selected_clip = None;
        }
        self.selected_clips.clear();
    }

    /// Moves (or copies) session clips by `tracks` columns and `slots` rows. Nothing
    /// happens unless every clip lands in a slot that is free (or being emptied by
    /// the move) on a track of its kind. Returns whether they moved.
    pub fn move_session_clips(&mut self, clips: &[(usize, usize)], tracks: isize, slots: isize, copy: bool) -> bool {
        if clips.is_empty() || (tracks == 0 && slots == 0) {
            return false;
        }
        let Some(to) = self.session_destinations(clips, tracks, slots, copy) else {
            self.status = Some("Los clips no caben ahí".into());
            return false;
        };
        let taken: Vec<Option<Clip>> = clips
            .iter()
            .map(|&(t, s)| if copy { self.project.tracks[t].clips[s].clone() } else { self.project.tracks[t].clips[s].take() })
            .collect();
        for (&(t, s), clip) in to.iter().zip(taken) {
            self.project.tracks[t].clips[s] = clip;
        }
        for &(t, s) in clips.iter().chain(&to) {
            self.sync_clip(t, s);
        }
        let primary = self.selected_clip;
        let moved: Vec<ClipRef> = to.iter().map(|&(track, slot)| ClipRef::Session { track, slot }).collect();
        // The clip that was shown in the editor stays first.
        let first = clips.iter().position(|&(track, slot)| primary == Some(ClipRef::Session { track, slot })).unwrap_or(0);
        let mut selection = moved.clone();
        selection.swap(0, first);
        self.set_clip_selection(selection);
        self.status = Some(match (clips.len(), copy) {
            (1, false) => "Clip movido".to_string(),
            (1, true) => "Clip copiado".to_string(),
            (n, false) => format!("{n} clips movidos"),
            (n, true) => format!("{n} clips copiados"),
        });
        true
    }

    /// Where session clips would land, if they all fit.
    fn session_destinations(&self, clips: &[(usize, usize)], tracks: isize, slots: isize, copy: bool) -> Option<Vec<(usize, usize)>> {
        let n = self.project.tracks.len() as isize;
        clips
            .iter()
            .map(|&(t, s)| {
                let (nt, ns) = (t as isize + tracks, s as isize + slots);
                if nt < 0 || nt >= n || ns < 0 || ns >= engine::NUM_SCENES as isize {
                    return None;
                }
                let (nt, ns) = (nt as usize, ns as usize);
                let clip = self.project.tracks[t].clips[s].as_ref()?;
                let track = &self.project.tracks[nt];
                let fits = matches!(clip, Clip::Audio(_)) == track.is_audio();
                let free = track.clips[ns].is_none() || (!copy && clips.contains(&(nt, ns)));
                (fits && free).then_some((nt, ns))
            })
            .collect()
    }

    /// Starts a selection rectangle at `anchor`.
    pub fn start_marquee(&mut self, anchor: Pos2, modifiers: egui::Modifiers) {
        self.clip_gesture = Some(Gesture::Marquee {
            anchor,
            base: self.clip_selection(),
            base_tracks: self.track_selection(),
            add: modifiers.command || modifiers.shift,
        });
    }

    /// Starts dragging the session clip at `slot` of `track` (and the others selected).
    pub fn start_session_move(&mut self, track: usize, slot: usize, modifiers: egui::Modifiers) {
        let clip = ClipRef::Session { track, slot };
        if !self.is_clip_selected(clip) {
            self.select_clip(clip);
        }
        self.clip_gesture = Some(Gesture::SessionMove { from: (track, slot), to: None, copy: modifiers.alt });
    }

    /// Each frame after the clips are drawn: follows the rectangle or the dragged
    /// session clips, draws them, and finishes when the button is released.
    /// `clips` are the clips on screen with their rects, `lanes` each track's area
    /// and `slots` the session slots (empty in the arrangement).
    pub fn update_clip_gesture(&mut self, ui: &egui::Ui, clips: &[(ClipRef, Rect)], lanes: &[(usize, Rect)], slots: &[(usize, usize, Rect)]) {
        let Some(gesture) = self.clip_gesture.clone() else { return };
        let Some(pos) = ui.ctx().pointer_latest_pos() else { return };
        let released = !ui.input(|i| i.pointer.any_down());
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("clip_gesture")));
        match gesture {
            Gesture::Marquee { anchor, base, base_tracks, add } => {
                let area = Rect::from_two_pos(anchor, pos);
                // A plain clic on empty space, not a rectangle: no clip selected
                // any more (the clip editor doesn't keep showing one).
                if released && !add && area.width() < MARQUEE_MIN && area.height() < MARQUEE_MIN {
                    let detail = self.detail;
                    self.selected_clip = None;
                    self.selected_clips.clear();
                    self.detail = detail;
                }
                if area.width() >= MARQUEE_MIN || area.height() >= MARQUEE_MIN {
                    painter.rect_filled(area, 0.0, Color32::from_rgba_unmultiplied(120, 170, 255, 28));
                    painter.rect_stroke(area, 0.0, Stroke::new(1.0, theme::SELECT), egui::StrokeKind::Inside);
                    let mut selection: Vec<ClipRef> = if add { base.clone() } else { Vec::new() };
                    for &(clip, rect) in clips {
                        if rect.intersects(area) && !selection.contains(&clip) {
                            selection.push(clip);
                        }
                    }
                    let mut tracks: Vec<usize> = if add { base_tracks.clone() } else { Vec::new() };
                    tracks.extend(lanes.iter().filter(|(_, r)| r.intersects(area)).map(|&(t, _)| t));
                    tracks.extend(selection.iter().map(|c| c.track()));
                    tracks.sort_unstable();
                    tracks.dedup();
                    if selection.is_empty() {
                        self.selected_clip = None;
                        self.selected_clips.clear();
                    } else if selection != self.clip_selection() {
                        let detail = self.detail;
                        self.set_clip_selection(selection.clone());
                        // Drawing a rectangle does not switch the bottom panel.
                        self.detail = detail;
                    }
                    if let Some(&first) = tracks.first() {
                        let anchor_track = selection.first().map_or(first, |c| c.track());
                        self.selected_target = Target::Track(anchor_track);
                        self.selected_tracks = tracks;
                    }
                }
            }
            Gesture::SessionMove { from, copy, .. } => {
                let to = slots.iter().find(|(_, _, r)| r.contains(pos)).map(|&(t, s, _)| (t, s));
                let moving: Vec<(usize, usize)> = self
                    .clip_selection()
                    .iter()
                    .filter_map(|c| match *c {
                        ClipRef::Session { track, slot } => Some((track, slot)),
                        ClipRef::Arrangement { .. } => None,
                    })
                    .collect();
                let delta = to.map(|(t, s)| (t as isize - from.0 as isize, s as isize - from.1 as isize));
                if let Some((dt, ds)) = delta.filter(|&d| d != (0, 0)) {
                    let fits = self.session_destinations(&moving, dt, ds, copy).is_some();
                    let color = if fits { theme::SELECT } else { theme::METER_RED };
                    for &(t, s) in &moving {
                        let target = ((t as isize + dt) as usize, (s as isize + ds) as usize);
                        if let Some(&(_, _, r)) = slots.iter().find(|&&(st, ss, _)| (st, ss) == target) {
                            painter.rect_stroke(r.shrink(1.0), 2.0, Stroke::new(2.0, color), egui::StrokeKind::Inside);
                        }
                    }
                    ui.ctx().set_cursor_icon(if copy { egui::CursorIcon::Copy } else { egui::CursorIcon::Grabbing });
                }
                if released {
                    if let Some((dt, ds)) = delta {
                        self.move_session_clips(&moving, dt, ds, copy);
                    }
                } else {
                    self.clip_gesture = Some(Gesture::SessionMove { from, to, copy });
                }
            }
        }
        if released {
            self.clip_gesture = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use egui::Modifiers;

    fn session_clips(app: &PulseApp) -> Vec<(usize, usize)> {
        let mut found = Vec::new();
        for (t, track) in app.project.tracks.iter().enumerate() {
            for (s, clip) in track.clips.iter().enumerate() {
                if clip.is_some() {
                    found.push((t, s));
                }
            }
        }
        found
    }

    #[test]
    fn command_clic_builds_a_selection_of_clips() {
        let mut app = PulseApp::headless(Project::demo());
        let clips = session_clips(&app);
        assert!(clips.len() >= 3, "{clips:?}");
        let refs: Vec<ClipRef> = clips.iter().map(|&(track, slot)| ClipRef::Session { track, slot }).collect();
        app.click_clip(refs[0], Modifiers::NONE);
        app.click_clip(refs[1], Modifiers::COMMAND);
        app.click_clip(refs[2], Modifiers::SHIFT);
        assert_eq!(app.clip_selection(), vec![refs[2], refs[1], refs[0]]);
        assert_eq!(app.selected_clip, Some(refs[2]), "the editor shows the last one clicked");
        assert!(app.is_clip_selected(refs[0]));
        app.click_clip(refs[2], Modifiers::COMMAND);
        assert_eq!(app.clip_selection(), vec![refs[1], refs[0]]);
        app.click_clip(refs[0], Modifiers::NONE);
        assert_eq!(app.clip_selection(), vec![refs[0]]);
        assert!(!app.is_clip_selected(refs[1]));
    }

    #[test]
    fn session_clips_move_and_copy_together_only_where_they_fit() {
        let mut app = PulseApp::headless(Project::demo());
        // Two clips on the first two tracks, in the first slot.
        for t in 0..2 {
            for s in 0..engine::NUM_SCENES {
                app.set_clip(t, s, None);
            }
            let color = app.project.tracks[t].color;
            app.set_clip(t, 0, Some(Clip::Midi(engine::model::MidiClip::new(&format!("c{t}"), 4.0, color))));
        }
        let audio = app.add_audio_track().unwrap();
        app.set_clip_selection(vec![ClipRef::Session { track: 1, slot: 0 }, ClipRef::Session { track: 0, slot: 0 }]);
        let name = |app: &PulseApp, t: usize, s: usize| app.project.tracks[t].clips[s].as_ref().map(|c| c.name().to_string());

        assert!(app.move_session_clips(&[(0, 0), (1, 0)], 0, 2, false));
        assert_eq!((name(&app, 0, 0), name(&app, 0, 2), name(&app, 1, 2)), (None, Some("c0".into()), Some("c1".into())));
        assert_eq!(app.selected_clip, Some(ClipRef::Session { track: 1, slot: 2 }), "the shown clip stays shown");
        assert_eq!(app.clip_selection().len(), 2);
        // Overlapping their own old places is fine.
        assert!(app.move_session_clips(&[(0, 2), (1, 2)], 0, 1, false));
        assert_eq!((name(&app, 0, 2), name(&app, 0, 3), name(&app, 1, 3)), (None, Some("c0".into()), Some("c1".into())));
        // Off the grid, or a MIDI clip onto an audio track: nothing moves.
        let before = session_clips(&app);
        assert!(!app.move_session_clips(&before, 0, -5, false));
        let last = before.iter().map(|&(t, _)| t).max().unwrap();
        assert!(!app.move_session_clips(&before, (audio - last) as isize, 0, false));
        assert_eq!(session_clips(&app), before);
        // ⌥: copies, the originals stay.
        assert!(app.move_session_clips(&[(0, 3), (1, 3)], 0, 1, true));
        assert_eq!(session_clips(&app).len(), before.len() + 2);
        assert_eq!((name(&app, 0, 3), name(&app, 0, 4)), (Some("c0".into()), Some("c0".into())));
        // A copy can't land on a clip, not even its own original.
        assert!(!app.move_session_clips(&[(0, 3), (1, 3)], 0, 1, true));
        assert!(!app.move_session_clips(&[(0, 3)], 0, 0, true));
    }

    #[test]
    fn deleting_and_duplicating_several_clips() {
        let mut app = PulseApp::headless(Project::demo());
        let color = app.project.tracks[0].color;
        let lane = &mut app.project.tracks[0].arrangement;
        lane.clear();
        for start in [0.0, 8.0, 16.0] {
            lane.push(engine::model::ArrangementClip { start, length: 4.0, clip: Clip::Midi(engine::model::MidiClip::new("a", 4.0, color)) });
        }
        app.sync_arrangement(0);
        let session = session_clips(&app);
        let (t, s) = session[0];
        app.set_clip_selection(vec![
            ClipRef::Arrangement { track: 0, index: 0 },
            ClipRef::Arrangement { track: 0, index: 2 },
            ClipRef::Session { track: t, slot: s },
        ]);
        app.duplicate_selected_clip();
        assert_eq!(app.project.tracks[0].arrangement.len(), 5);
        assert_eq!(app.clip_selection().len(), 3, "the copies are selected");
        assert_eq!(session_clips(&app).len(), session.len() + 1);
        app.delete_selected_clip();
        assert_eq!(app.project.tracks[0].arrangement.len(), 3);
        assert_eq!(session_clips(&app), session);
        assert!(app.clip_selection().is_empty());
        app.set_clip_selection(vec![ClipRef::Arrangement { track: 0, index: 0 }, ClipRef::Arrangement { track: 0, index: 2 }]);
        app.delete_selected_clip();
        let starts: Vec<f64> = app.project.tracks[0].arrangement.iter().map(|c| c.start).collect();
        assert_eq!(starts, vec![8.0]);
    }

    use crate::tracks::tests::{click, drag};
    use egui::pos2;

    /// Four MIDI tracks with no clips.
    fn empty_tracks() -> PulseApp {
        let mut app = PulseApp::headless(Project::demo());
        app.project.tracks.truncate(4);
        for track in &mut app.project.tracks {
            track.clips = Default::default();
            track.arrangement.clear();
        }
        app.sync_all();
        app
    }

    fn midi(app: &PulseApp, name: &str) -> Clip {
        Clip::Midi(engine::model::MidiClip::new(name, 4.0, app.project.tracks[0].color))
    }

    #[test]
    fn session_clips_are_selected_and_dragged_with_the_mouse() {
        let view = crate::session::show as fn(&mut PulseApp, &mut egui::Ui);
        // Middle of a slot's name area (past the launch button).
        let slot = |t: usize, s: usize| pos2(t as f32 * 109.0 + 70.0, 23.0 + s as f32 * 23.0 + 11.0);
        let mut app = empty_tracks();
        for t in 0..2 {
            let clip = midi(&app, &format!("c{t}"));
            app.set_clip(t, 0, Some(clip));
        }
        click(&mut app, view, slot(0, 0), Modifiers::NONE);
        click(&mut app, view, slot(1, 0), Modifiers::COMMAND);
        assert_eq!(app.clip_selection().len(), 2);
        // Drag both two slots down.
        drag(&mut app, view, slot(0, 0), slot(0, 2), Modifiers::NONE);
        assert_eq!(session_clips(&app), vec![(0, 2), (1, 2)]);
        assert_eq!(app.clip_selection().len(), 2, "they stay selected");
        // ⌥ drag of both one track to the right: the first would land on the
        // second's original, so nothing happens.
        drag(&mut app, view, slot(1, 2), slot(2, 2), Modifiers::ALT);
        assert_eq!(session_clips(&app), vec![(0, 2), (1, 2)]);
        // Only one selected: it is copied.
        click(&mut app, view, slot(1, 2), Modifiers::NONE);
        drag(&mut app, view, slot(1, 2), slot(2, 2), Modifiers::ALT);
        assert_eq!(session_clips(&app), vec![(0, 2), (1, 2), (2, 2)]);
        // A rectangle from an empty slot selects the clips and tracks it covers.
        click(&mut app, view, slot(3, 6), Modifiers::NONE);
        drag(&mut app, view, slot(3, 6), slot(1, 1), Modifiers::NONE);
        assert!(app.clip_gesture.is_none());
        let mut selected: Vec<(usize, usize)> = app.clip_selection().iter().map(|c| (c.track(), match c { ClipRef::Session { slot, .. } => *slot, _ => 99 })).collect();
        selected.sort_unstable();
        assert_eq!(selected, vec![(1, 2), (2, 2)]);
        assert_eq!(app.track_selection(), vec![1, 2, 3]);
        // A plain clic on one of them leaves only that one.
        click(&mut app, view, slot(2, 2), Modifiers::NONE);
        assert_eq!(app.clip_selection(), vec![ClipRef::Session { track: 2, slot: 2 }]);
    }

    #[test]
    fn arrangement_clips_are_selected_and_dragged_with_the_mouse() {
        let view = crate::arrangement::show as fn(&mut PulseApp, &mut egui::Ui);
        let ppb = PulseApp::headless(Project::demo()).arrangement.pixels_per_beat;
        let at = |t: usize, beat: f64| pos2(150.0 + beat as f32 * ppb, 96.0 + t as f32 * 53.0);
        let mut app = empty_tracks();
        for (t, start) in [(0, 0.0), (1, 8.0)] {
            let clip = midi(&app, &format!("c{t}"));
            app.project.tracks[t].arrangement.push(engine::model::ArrangementClip { start, length: 4.0, clip });
            app.sync_arrangement(t);
        }
        let starts = |app: &PulseApp| -> Vec<(usize, f64)> {
            app.project.tracks.iter().enumerate().flat_map(|(t, track)| track.arrangement.iter().map(move |c| (t, c.start))).collect()
        };
        click(&mut app, view, at(0, 1.0), Modifiers::NONE);
        click(&mut app, view, at(1, 9.0), Modifiers::COMMAND);
        assert_eq!(app.clip_selection().len(), 2);
        // Both four beats later and one track down.
        drag(&mut app, view, at(0, 1.0), at(1, 5.0), Modifiers::NONE);
        assert_eq!(starts(&app), vec![(1, 4.0), (2, 12.0)]);
        assert_eq!(app.clip_selection().len(), 2);
        assert_eq!(app.selected_clip.map(|c| c.track()), Some(1), "the grabbed clip is shown");
        // ⌥ copies them back to the start, the moved ones stay.
        drag(&mut app, view, at(1, 5.0), at(1, 1.0), Modifiers::ALT);
        assert_eq!(starts(&app), vec![(1, 4.0), (1, 0.0), (2, 12.0), (2, 8.0)]);
        // A rectangle over empty space at the right of track 3 up to track 1 at beat 6.
        drag(&mut app, view, at(3, 30.0), at(1, 6.0), Modifiers::NONE);
        let mut selected: Vec<(usize, f64)> = app
            .clip_selection()
            .iter()
            .map(|c| match *c {
                ClipRef::Arrangement { track, index } => (track, app.project.tracks[track].arrangement[index].start),
                ClipRef::Session { .. } => (99, 0.0),
            })
            .collect();
        selected.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(selected, vec![(1, 4.0), (2, 8.0), (2, 12.0)]);
        assert_eq!(app.track_selection(), vec![1, 2, 3]);
        // ⌫ (the menu's Delete) deletes the three.
        app.delete_selected_clip();
        assert_eq!(starts(&app), vec![(1, 0.0)]);
    }

    /// As in Live: a clip copied, an empty slot clicked, ⌘V puts it there.
    #[test]
    fn a_clip_is_pasted_in_the_empty_slot_clicked() {
        let mut app = empty_tracks();
        let a = midi(&app, "a");
        app.set_clip(0, 0, Some(a));
        app.select_clip(ClipRef::Session { track: 0, slot: 0 });
        app.copy_clips();
        // What clicking slot 6 of track 2 does (session::Action::SelectSlot).
        app.set_clip_selection(Vec::new());
        app.selected_target = Target::Track(1);
        app.paste_slot = Some((1, 5));
        app.paste_clips();
        assert_eq!(session_clips(&app), vec![(0, 0), (1, 5)]);
        assert_eq!(app.clip_selection(), vec![ClipRef::Session { track: 1, slot: 5 }], "the pasted clip is selected");
        assert_eq!(app.paste_slot, None, "selecting it forgets the slot");
        // Again: below it, the next free slot.
        app.paste_clips();
        assert_eq!(session_clips(&app), vec![(0, 0), (1, 5), (1, 6)]);
    }

    #[test]
    fn several_clips_are_copied_and_pasted_in_their_layout() {
        let mut app = empty_tracks();
        let (a, b) = (midi(&app, "a"), midi(&app, "b"));
        app.set_clip(0, 0, Some(a.clone()));
        app.set_clip(1, 1, Some(b.clone()));
        app.set_clip_selection(vec![ClipRef::Session { track: 0, slot: 0 }, ClipRef::Session { track: 1, slot: 1 }]);
        app.copy_clips();
        // From the selected slot down: the first row where both fit.
        app.paste_clips();
        assert_eq!(session_clips(&app), vec![(0, 0), (0, 1), (1, 1), (1, 2)]);
        assert_eq!(app.clip_selection(), vec![ClipRef::Session { track: 0, slot: 1 }, ClipRef::Session { track: 1, slot: 2 }]);
        // Into the arrangement (nothing selected there): at the playhead's bar, one
        // track per track, each clip after the other.
        app.view = View::Arrangement;
        app.selected_clip = None;
        app.selected_target = Target::Track(1);
        app.paste_clips();
        let starts = |app: &PulseApp| -> Vec<(usize, f64, String)> {
            app.project.tracks.iter().enumerate().flat_map(|(t, track)| track.arrangement.iter().map(move |c| (t, c.start, c.clip.name().to_string()))).collect()
        };
        assert_eq!(starts(&app), vec![(1, 0.0, "a".into()), (2, 0.0, "b".into())]);
        // Pasted again: after the pasted ones (which are selected).
        app.paste_clips();
        assert_eq!(starts(&app), vec![(1, 0.0, "a".into()), (1, 4.0, "a".into()), (2, 0.0, "b".into()), (2, 4.0, "b".into())]);

        // Arrangement clips keep their distances, and go to the session one per slot.
        app.set_clip_selection(vec![ClipRef::Arrangement { track: 1, index: 0 }, ClipRef::Arrangement { track: 1, index: 1 }, ClipRef::Arrangement { track: 2, index: 1 }]);
        app.copy_clips();
        let board = app.clip_clipboard.clone().unwrap();
        let places: Vec<(usize, usize, f64)> = board.items.iter().map(|i| (i.track, i.slot, i.start)).collect();
        assert_eq!(places, vec![(0, 0, 0.0), (0, 1, 4.0), (1, 0, 4.0)]);
        app.view = View::Session;
        app.selected_clip = Some(ClipRef::Session { track: 2, slot: 4 });
        app.paste_clips();
        assert!(session_clips(&app).ends_with(&[(2, 4), (2, 5), (3, 4)]), "{:?}", session_clips(&app));

        // ⌘X takes them all, and they paste back.
        let before = session_clips(&app);
        app.set_clip_selection(vec![ClipRef::Session { track: 2, slot: 4 }, ClipRef::Session { track: 3, slot: 4 }]);
        assert_eq!(app.detail, crate::app::Detail::Clip, "the clip editor is open, with no notes selected");
        app.run(crate::commands::Command::Cut, &egui::Context::default());
        assert_eq!(session_clips(&app).len(), before.len() - 2);
        app.selected_clip = Some(ClipRef::Session { track: 2, slot: 4 });
        app.paste_clips();
        assert_eq!(session_clips(&app), before);

        // Not enough tracks to the right: nothing pasted.
        app.selected_clip = Some(ClipRef::Session { track: 3, slot: 7 });
        app.paste_clips();
        assert_eq!(session_clips(&app), before);
        assert!(app.status.as_deref().unwrap_or("").contains("no caben"));
    }
}
