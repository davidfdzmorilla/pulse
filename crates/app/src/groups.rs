//! Groups of tracks: a group is a track with no clips whose tracks play
//! through its effects and fader, like a bus (the engine mixes them into it).
//! Its tracks are the ones right under it, one level deeper (`Track::depth`),
//! as in Live; groups can hold groups. Folded, its tracks are hidden.

use crate::app::PulseApp;
use crate::project::Track;
use engine::{Target, MAX_TRACKS};

/// The group each track plays into, for the engine.
pub fn parents(tracks: &[Track]) -> [Option<u8>; MAX_TRACKS] {
    let mut out = [None; MAX_TRACKS];
    // The groups open above the track, the innermost last.
    let mut open: Vec<(usize, u8)> = Vec::new();
    for (i, track) in tracks.iter().enumerate().take(MAX_TRACKS) {
        while open.last().is_some_and(|&(_, d)| d >= track.depth) {
            open.pop();
        }
        out[i] = open.last().filter(|&&(_, d)| d + 1 == track.depth).map(|&(g, _)| g as u8);
        if track.is_group() {
            open.push((i, track.depth));
        }
    }
    out
}

/// The group a track is in.
pub fn parent(tracks: &[Track], t: usize) -> Option<usize> {
    parents(tracks).get(t).copied().flatten().map(usize::from)
}

/// The tracks a group holds, at every depth (empty for any other track).
pub fn children(tracks: &[Track], g: usize) -> std::ops::Range<usize> {
    let Some(group) = tracks.get(g).filter(|t| t.is_group()) else { return g + 1..g + 1 };
    let end = (g + 1..tracks.len()).find(|&i| tracks[i].depth <= group.depth).unwrap_or(tracks.len());
    g + 1..end
}

/// A track and, if it is a group, everything in it.
pub fn block(tracks: &[Track], t: usize) -> std::ops::Range<usize> {
    t..children(tracks, t).end.max(t + 1)
}

/// Whether a track is hidden inside a folded group.
pub fn hidden(tracks: &[Track], t: usize) -> bool {
    let mut at = parent(tracks, t);
    while let Some(g) = at {
        if tracks[g].folded {
            return true;
        }
        at = parent(tracks, g);
    }
    false
}

/// Makes every depth possible: one more than the group right above at most,
/// and no deeper than the track above otherwise.
pub fn normalize(tracks: &mut [Track]) {
    for i in 0..tracks.len() {
        let most = match i.checked_sub(1).map(|p| &tracks[p]) {
            None => 0,
            Some(prev) if prev.is_group() => prev.depth + 1,
            Some(prev) => prev.depth,
        };
        tracks[i].depth = tracks[i].depth.min(most);
    }
}

impl PulseApp {
    /// Puts the selected tracks (with what is in any selected group) in a new
    /// group, where the first of them was.
    pub fn group_selected(&mut self) {
        let tracks = &self.project.tracks;
        let mut members: Vec<usize> = self.track_selection().iter().flat_map(|&t| block(tracks, t)).collect();
        members.sort_unstable();
        members.dedup();
        if members.is_empty() {
            return;
        }
        if self.recording_anything() {
            self.status = Some("No se pueden agrupar pistas mientras se graba".into());
            return;
        }
        if tracks.len() >= MAX_TRACKS {
            self.status = Some(format!("Máximo de {MAX_TRACKS} pistas"));
            return;
        }
        // Each one's depth inside the new group: its own under the outermost of
        // them it is inside.
        let root_depth = |m: usize| {
            let mut root = m;
            while let Some(p) = parent(tracks, root).filter(|p| members.contains(p)) {
                root = p;
            }
            tracks[root].depth
        };
        let first = members[0];
        let depth = tracks[first].depth;
        let new_depths: Vec<u8> = members.iter().map(|&m| depth + 1 + (tracks[m].depth - root_depth(m))).collect();
        let color = tracks[first].color;
        self.move_tracks(&members, first);
        for (k, &d) in new_depths.iter().enumerate() {
            self.project.tracks[first + k].depth = d;
        }
        let count = self.project.groups_count() + 1;
        let mut group = Track::new_group(format!("Grupo {count}"), color);
        group.depth = depth;
        let ids = self.track_ids();
        self.project.tracks.insert(first, group);
        self.follow_tracks(&ids);
        crate::groups::normalize(&mut self.project.tracks);
        self.selected_clip = None;
        self.selected_tracks = vec![first];
        self.selected_target = Target::Track(first);
        self.sync_all();
        self.status = Some(format!("{} pista(s) en un grupo: suenan por su fader y sus efectos", members.len()));
    }

    /// Takes a group's tracks out of it, and the group away.
    pub fn ungroup(&mut self, g: usize) {
        if !self.project.tracks.get(g).is_some_and(|t| t.is_group()) {
            return;
        }
        let ids = self.track_ids();
        let tracks = &mut self.project.tracks;
        let inside = children(tracks, g);
        for t in inside.clone() {
            tracks[t].depth = tracks[t].depth.saturating_sub(1);
        }
        tracks.remove(g);
        self.follow_tracks(&ids);
        let tracks = &mut self.project.tracks;
        normalize(tracks);
        self.selected_clip = None;
        self.selected_tracks = (inside.start - 1..inside.end - 1).collect();
        self.selected_target = Target::Track(g.min(self.project.tracks.len() - 1));
        self.sync_all();
        self.status = Some("Grupo deshecho".into());
    }

    /// Takes a track (and what it holds) out of its group, right after it.
    pub fn leave_group(&mut self, t: usize) {
        let Some(g) = parent(&self.project.tracks, t) else { return };
        let moving: Vec<usize> = block(&self.project.tracks, t).collect();
        let end = children(&self.project.tracks, g).end;
        self.move_tracks(&moving, end);
        // Now right before where the group's block ended, a level up.
        let at = end - moving.len();
        for i in at..at + moving.len() {
            self.project.tracks[i].depth = self.project.tracks[i].depth.saturating_sub(1);
        }
        normalize(&mut self.project.tracks);
        self.selected_target = Target::Track(at);
        self.selected_tracks = vec![at];
        self.sync_groups();
    }

    /// Shows or hides a group's tracks.
    pub fn toggle_fold(&mut self, g: usize) {
        if let Some(track) = self.project.tracks.get_mut(g).filter(|t| t.is_group()) {
            track.folded = !track.folded;
        }
        // A hidden track selected: its group instead.
        if let Target::Track(t) = self.selected_target {
            if hidden(&self.project.tracks, t) {
                self.selected_target = Target::Track(g);
                self.selected_tracks = vec![g];
                self.selected_clip = None;
            }
        }
    }

    /// A group's slot launched: its tracks play their clips in that scene, and
    /// the ones without a clip there stop.
    pub fn launch_group(&mut self, g: usize, slot: usize) {
        for t in children(&self.project.tracks, g) {
            let track = &self.project.tracks[t];
            if track.is_group() {
                continue;
            }
            let command = if track.clips[slot].is_some() { engine::Command::LaunchClip { track: t, slot } } else { engine::Command::StopTrack(t) };
            self.audio.send(command);
        }
    }

    /// Tells the engine which group each track plays into.
    pub fn sync_groups(&mut self) {
        self.audio.send(engine::Command::SetGroups(parents(&self.project.tracks)));
    }
}

impl crate::project::Project {
    fn groups_count(&self) -> usize {
        self.tracks.iter().filter(|t| t.is_group()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    fn names(app: &PulseApp) -> Vec<String> {
        app.project.tracks.iter().map(|t| format!("{}{}", "·".repeat(t.depth as usize), t.name)).collect()
    }

    fn app() -> PulseApp {
        let mut app = PulseApp::headless(Project::demo());
        app.project.tracks.truncate(4);
        app.sync_all();
        app
    }

    #[test]
    fn the_structure_is_read_from_the_depths() {
        let mut tracks: Vec<Track> = ["G", "a", "H", "b", "c", "d"].iter().map(|n| Track::new_audio(*n, [0; 3])).collect();
        tracks[0] = Track::new_group("G", [0; 3]);
        tracks[2] = Track { depth: 1, ..Track::new_group("H", [0; 3]) };
        tracks[1].depth = 1;
        tracks[3].depth = 2;
        tracks[4].depth = 1;
        let p = parents(&tracks);
        assert_eq!(&p[..6], &[None, Some(0), Some(0), Some(2), Some(0), None]);
        assert_eq!(children(&tracks, 0), 1..5);
        assert_eq!(children(&tracks, 2), 3..4);
        assert_eq!(block(&tracks, 5), 5..6);
        tracks[2].folded = true;
        assert!(hidden(&tracks, 3) && !hidden(&tracks, 4));
        // A track can't be deeper than the group above allows.
        tracks[5].depth = 4;
        normalize(&mut tracks);
        assert_eq!(tracks[5].depth, 1);
    }

    #[test]
    fn tracks_are_grouped_ungrouped_and_taken_out() {
        let mut app = app();
        let before = names(&app);
        app.selected_tracks = vec![1, 3];
        app.selected_target = Target::Track(1);
        app.group_selected();
        assert_eq!(names(&app), vec![before[0].clone(), "Grupo 1".into(), format!("·{}", before[1]), format!("·{}", before[3]), before[2].clone()]);
        assert_eq!(app.selected_target, Target::Track(1));
        // One of them out: right after the group.
        app.leave_group(3);
        assert_eq!(names(&app), vec![before[0].clone(), "Grupo 1".into(), format!("·{}", before[1]), before[3].clone(), before[2].clone()]);
        // A group in a group, then undone.
        app.selected_tracks = vec![1];
        app.selected_target = Target::Track(1);
        app.group_selected();
        assert_eq!(names(&app)[1..4], ["Grupo 2".to_string(), "·Grupo 1".into(), format!("··{}", before[1])]);
        app.ungroup(1);
        app.ungroup(1);
        assert_eq!(names(&app), vec![before[0].clone(), before[1].clone(), before[3].clone(), before[2].clone()]);
    }

    #[test]
    fn a_group_goes_with_its_tracks_when_moved_deleted_or_duplicated() {
        let mut app = app();
        let before = names(&app);
        app.selected_tracks = vec![0, 1];
        app.selected_target = Target::Track(0);
        app.group_selected();
        // Moved to the end: its tracks too.
        assert!(app.move_tracks(&[0], app.project.tracks.len()));
        assert_eq!(names(&app), vec![before[2].clone(), before[3].clone(), "Grupo 1".into(), format!("·{}", before[0]), format!("·{}", before[1])]);
        // Duplicated: the copy holds copies.
        app.duplicate_track(2);
        assert_eq!(names(&app).len(), 8);
        assert_eq!(names(&app)[5], "Grupo 1 copia");
        assert_eq!(names(&app)[6], format!("·{}", before[0]));
        // Deleted: with what is in it.
        app.delete_track(5);
        assert_eq!(names(&app).len(), 5);
        // A track dropped right under a group's header joins it.
        assert!(app.move_tracks(&[0], 3));
        assert_eq!(names(&app)[2], format!("·{}", before[2]));
    }

    /// Tracks keep their ids when they move, copies get new ones, and a
    /// sidechain naming one survives saving and opening.
    #[test]
    fn track_ids_are_stable_for_sidechains() {
        let mut app = app();
        let ids: Vec<u32> = app.project.tracks.iter().map(|t| t.id).collect();
        assert!(ids.iter().all(|&id| id != 0));
        assert_eq!(ids.iter().collect::<std::collections::HashSet<_>>().len(), ids.len());
        // The bass ducks with the drums.
        let drums = app.project.tracks[0].id;
        let ducker = engine::model::DuckerParams { sidechain: Some(drums), ..Default::default() };
        app.project.tracks[1].devices.push(engine::model::Device::new(engine::model::Effect::Ducker(ducker)));
        app.move_tracks(&[0], 4);
        assert_eq!(app.project.tracks[3].id, drums, "moved with its id");
        app.duplicate_track(3);
        assert_ne!(app.project.tracks[4].id, drums, "a copy has its own");
        let dir = std::env::temp_dir().join(format!("pulse-ids-{}", std::process::id()));
        let file = dir.join("Ids").join("Ids.pulse");
        app.save_to_path(&file);
        let loaded = crate::project::Project::load(&file).unwrap();
        let bass = loaded.tracks.iter().find(|t| t.devices.iter().any(|d| matches!(d.effect, engine::model::Effect::Ducker(_)))).unwrap();
        let engine::model::Effect::Ducker(p) = bass.devices.last().unwrap().effect else { panic!() };
        assert_eq!(p.sidechain, Some(drums));
        assert!(loaded.tracks.iter().any(|t| t.id == drums));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The texts a view draws.
    fn texts(app: &mut PulseApp, view: fn(&mut PulseApp, &mut eframe::egui::Ui)) -> Vec<String> {
        let ctx = eframe::egui::Context::default();
        let input = || eframe::egui::RawInput { screen_rect: Some(eframe::egui::Rect::from_min_size(eframe::egui::pos2(0.0, 0.0), eframe::egui::vec2(1600.0, 900.0))), ..Default::default() };
        let _ = ctx.run_ui(input(), |ui| view(app, ui));
        let output = ctx.run_ui(input(), |ui| view(app, ui));
        let mut texts = Vec::new();
        fn collect(shape: &eframe::egui::Shape, texts: &mut Vec<String>) {
            match shape {
                eframe::egui::Shape::Text(t) => texts.push(t.galley.text().to_string()),
                eframe::egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
                _ => {}
            }
        }
        output.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
        texts
    }

    /// Both views show the group and, folded, not its tracks.
    #[test]
    fn the_views_show_groups_and_hide_folded_tracks() {
        let mut app = app();
        let inside = app.project.tracks[1].name.clone();
        app.selected_tracks = vec![1, 2];
        app.selected_target = Target::Track(1);
        app.group_selected();
        for view in [crate::session::show as fn(&mut PulseApp, &mut eframe::egui::Ui), crate::arrangement::show] {
            app.project.tracks[1].folded = false;
            let shown = texts(&mut app, view);
            assert!(shown.contains(&"▾ Grupo 1".to_string()), "{shown:?}");
            assert!(shown.contains(&inside));
            app.project.tracks[1].folded = true;
            let shown = texts(&mut app, view);
            assert!(shown.contains(&"▸ Grupo 1".to_string()));
            assert!(!shown.contains(&inside), "folded: {shown:?}");
        }
    }

    /// A group's slot plays its tracks' clips of that scene, through the engine.
    #[test]
    fn a_group_slot_launches_its_tracks() {
        let mut app = app();
        let (audio, mut engine) = crate::app::Audio::detached();
        app.audio = audio;
        app.selected_tracks = vec![0, 1];
        app.selected_target = Target::Track(0);
        app.group_selected();
        let state = app.audio.state().unwrap();
        // Both tracks have a clip in the first scene in the demo.
        assert!(app.project.tracks[1].clips[0].is_some() && app.project.tracks[2].clips[0].is_some());
        app.launch_group(0, 0);
        let mut out = vec![0f32; 2 * 512];
        for _ in 0..20 {
            engine.process(&mut out, 2);
        }
        let playing = |t: usize| state.tracks[t].playing.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!((playing(1), playing(2)), (0, 0));
        assert!(playing(3) < 0, "a track outside the group is left alone");
        // The group sounds: its meter moves.
        assert!(state.tracks[0].peak.take() > 0.0);
    }

    #[test]
    fn folding_hides_the_tracks_and_keeps_the_selection_visible() {
        let mut app = app();
        app.selected_tracks = vec![1, 2];
        app.selected_target = Target::Track(1);
        app.group_selected();
        app.selected_target = Target::Track(2);
        app.toggle_fold(1);
        assert!(hidden(&app.project.tracks, 2) && hidden(&app.project.tracks, 3));
        assert_eq!(app.selected_target, Target::Track(1));
        app.toggle_fold(1);
        assert!(!hidden(&app.project.tracks, 2));
    }
}
