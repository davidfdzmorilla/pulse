//! What points at a track or a return (automation overridden by hand, lanes
//! shown, editors, drum sounds, sidechains) follows it when tracks and
//! returns are deleted, duplicated or grouped, and damaged files can't point
//! at returns that aren't there.

use crate::app::PulseApp;
use crate::project::{Project, Track};
use eframe::egui;
use engine::model::{AutomationLane, AutomationPoint, DrumModel, DrumParams, InstrumentKind, Param};
use engine::Target;

fn lane(param: Param) -> AutomationLane {
    let mut lane = AutomationLane::new(param);
    lane.points.push(AutomationPoint { beat: 0.0, value: 0.2, curve: 0.0 });
    lane.points.push(AutomationPoint { beat: 4.0, value: 0.9, curve: 0.0 });
    lane
}

/// Deleting a track: the automation overridden by hand stays on the track
/// whose control was moved (it used to stay on the position, silencing
/// another track's automation).
#[test]
fn deleting_a_track_keeps_overrides_on_their_track() {
    let mut app = PulseApp::headless(Project::demo());
    for t in 0..app.project.tracks.len() {
        app.project.tracks[t].automation.push(lane(Param::Volume));
    }
    app.sync_all();
    app.set_param(Target::Track(2), Param::Volume, 0.5);
    let (moved, other) = (app.project.tracks[2].id, app.project.tracks[3].id);
    app.delete_track(0);
    let at = |app: &PulseApp, id: u32| app.project.tracks.iter().position(|t| t.id == id).unwrap();
    assert_eq!(app.automation_state(Target::Track(at(&app, other)), Param::Volume), Some(false), "never touched: its automation plays");
    assert_eq!(app.automation_state(Target::Track(at(&app, moved)), Param::Volume), Some(true));
}

/// Grouping inserts a group track: the lane shown and an open editor stay
/// with their track.
#[test]
fn grouping_keeps_the_lane_shown_and_the_editor_on_their_track() {
    let mut app = PulseApp::headless(Project::demo());
    app.arrangement.automation.insert(2, Param::Pan);
    app.open_instrument_editor(crate::app::InstrumentEditor::Wave, 2);
    let id = app.project.tracks[2].id;
    app.click_track(0, egui::Modifiers::NONE);
    app.group_selected();
    let now = app.project.tracks.iter().position(|t| t.id == id).unwrap();
    assert_eq!(now, 3);
    assert_eq!(app.arrangement.automation.get(&now), Some(&Param::Pan));
    assert_eq!(app.wave_window, Some(now));
}

/// Deleting a return: overridden sends move with their returns, the one
/// deleted goes, and so does a lane shown for it.
#[test]
fn deleting_a_return_moves_overrides_and_lanes_with_the_sends() {
    let mut app = PulseApp::headless(Project::demo());
    app.add_return(); // C
    app.add_return(); // D
    let t = &mut app.project.tracks[0];
    t.automation.push(lane(Param::Send(2)));
    t.automation.push(lane(Param::Send(3)));
    app.sync_all();
    app.set_param(Target::Track(0), Param::Send(2), 0.1); // C, by hand
    app.arrangement.automation.insert(0, Param::Send(3)); // D shown
    app.arrangement.automation.insert(1, Param::Send(1)); // B shown
    app.delete_return(1); // B goes: C → B, D → C
    assert_eq!(app.automation_state(Target::Track(0), Param::Send(2)), Some(false), "D, now C, never touched");
    assert_eq!(app.automation_state(Target::Track(0), Param::Send(1)), Some(true), "C, now B, overridden");
    assert_eq!(app.arrangement.automation.get(&0), Some(&Param::Send(2)), "D's lane, now C's");
    assert_eq!(app.arrangement.automation.get(&1), None, "B's lane goes with it");
}

fn drums_with_kick_apart() -> PulseApp {
    let mut project = Project::empty();
    project.tracks = vec![Track::new("Drums", [200, 120, 0], InstrumentKind::DrumMachine(DrumParams { model: DrumModel::Tr909, ..Default::default() }))];
    let mut app = PulseApp::headless(project);
    app.sound_apart(0, 0);
    app
}

/// Duplicating a group with drums and their kick on its own track: the copied
/// kick takes it from the copied drums (it took the original's: twice there,
/// never from the copy).
#[test]
fn a_duplicated_group_takes_its_sounds_from_its_own_drums() {
    let mut app = drums_with_kick_apart();
    app.click_track(0, egui::Modifiers::NONE);
    app.click_track(1, egui::Modifiers::SHIFT);
    app.group_selected(); // [Grupo, Drums, Drums Kick]
    app.duplicate_track(0);
    assert_eq!(app.project.tracks.len(), 6);
    assert_eq!(app.project.tracks[2].audio_from, Some(app.project.tracks[1].id), "the original still takes from the original");
    assert_eq!(app.project.tracks[5].audio_from, Some(app.project.tracks[4].id), "the copy from the copy");
}

/// Duplicating only the track that plays the kick would play it twice: it
/// is refused, saying why.
#[test]
fn a_track_taking_a_drum_sound_is_not_duplicated_alone() {
    let mut app = drums_with_kick_apart();
    app.duplicate_track(1);
    assert_eq!(app.project.tracks.len(), 2);
    assert!(app.status.as_deref().is_some_and(|s| s.contains("duplica «Drums»")), "{:?}", app.status);
}

/// A damaged file: sends to returns it doesn't have are dropped, and so are
/// returns past the most there can be.
#[test]
fn damaged_files_dont_send_to_returns_that_arent_there() {
    let mut value: serde_json::Value = serde_json::from_str(&Project::demo().to_json().unwrap()).unwrap();
    value["tracks"][0]["sends"] = serde_json::json!([0.0, 0.0, 1.0]);
    let project = Project::from_json(&value.to_string()).unwrap();
    assert_eq!(project.tracks[0].sends[2], 0.0);
    let saved: serde_json::Value = serde_json::from_str(&project.to_json().unwrap()).unwrap();
    assert_eq!(saved["tracks"][0]["sends"].as_array().unwrap().len(), 2, "what a Pulse with two returns reads");

    let mut many = Project::demo();
    while many.returns.len() <= engine::MAX_RETURNS {
        many.returns.push(crate::returns::new_return(many.returns.len()));
    }
    assert_eq!(Project::from_json(&many.to_json().unwrap()).unwrap().returns.len(), engine::MAX_RETURNS);
}

/// The height of a track's lane goes with it when tracks are moved, deleted
/// or duplicated (the copy as high), and so do the take lanes shown; the
/// automation lane shown does too (it always did). Saved and opened, it is
/// the same; a file without heights opens as before.
#[test]
fn a_tracks_height_goes_with_it() {
    use crate::project::LaneHeight;
    let mut app = PulseApp::headless(Project::demo());
    let id = |app: &PulseApp, t: usize| app.project.tracks[t].id;
    let at = |app: &PulseApp, id: u32| app.project.tracks.iter().position(|t| t.id == id).unwrap();
    let (tall, low) = (id(&app, 1), id(&app, 2));
    app.project.tracks[1].height = LaneHeight(Some(150.0));
    app.project.tracks[2].height = LaneHeight(Some(48.0));
    app.arrangement.takes.insert(1);
    app.arrangement.automation.insert(2, Param::Pan);
    let lane = |app: &PulseApp, id: u32| app.arrangement.lane(&app.project.tracks[at(app, id)]);
    // Moved: track 1 to the end.
    assert!(app.move_tracks(&[1], 4));
    assert_eq!((at(&app, tall), at(&app, low)), (3, 1));
    assert_eq!((lane(&app, tall), lane(&app, low)), (150.0, 48.0));
    assert_eq!(app.arrangement.lane(&app.project.tracks[0]), crate::arrangement::LANE_H);
    assert!(app.arrangement.takes.contains(&3) && app.arrangement.takes.len() == 1);
    assert_eq!(app.arrangement.automation.get(&1), Some(&Param::Pan));
    // A track before them deleted.
    app.delete_track(0);
    assert_eq!((at(&app, tall), at(&app, low)), (2, 0));
    assert_eq!((lane(&app, tall), lane(&app, low)), (150.0, 48.0));
    assert!(app.arrangement.takes.contains(&2) && app.arrangement.takes.len() == 1);
    assert_eq!(app.arrangement.automation.get(&0), Some(&Param::Pan));
    // Duplicated: the copy, right after it, as high.
    app.duplicate_track(at(&app, tall));
    assert_eq!(app.project.tracks.len(), 4);
    assert_eq!(app.arrangement.lane(&app.project.tracks[3]), 150.0);
    assert_eq!((lane(&app, tall), lane(&app, low)), (150.0, 48.0));
    // Saved and opened.
    let dir = std::env::temp_dir().join(format!("pulse-heights-{}", std::process::id()));
    let file = dir.join("H/H.pulse");
    app.project.save(&file).unwrap();
    let loaded = Project::load(&file).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let heights = |p: &Project| p.tracks.iter().map(|t| t.height.0).collect::<Vec<_>>();
    assert_eq!(heights(&loaded), heights(&app.project));
    assert_eq!(heights(&loaded), vec![Some(48.0), None, Some(150.0), Some(150.0)]);
    // Without heights, the file is as before (an older Pulse reads it all), and
    // a damaged height is brought within the range.
    assert!(!Project::demo().to_json().unwrap().contains("height"));
    let mut value: serde_json::Value = serde_json::from_str(&Project::demo().to_json().unwrap()).unwrap();
    value["tracks"][0]["height"] = 5000.0.into();
    let damaged = Project::from_json(&value.to_string()).unwrap();
    assert_eq!(app.arrangement.lane(&damaged.tracks[0]), crate::arrangement::LANE_MAX);
}

/// A lane made higher isn't a step to undo (it is the view's, as scrolling
/// is), but the project has changed, to be saved; undoing something else
/// leaves the heights as they are, and a deleted track comes back as high.
#[test]
fn a_tracks_height_is_saved_but_not_undone() {
    use crate::project::LaneHeight;
    let mut app = PulseApp::headless(Project::demo());
    app.commit_history();
    assert!(!app.is_dirty());
    app.project.tracks[0].height = LaneHeight(Some(120.0));
    app.commit_history();
    assert!(!app.can_undo(), "not a step");
    assert!(app.is_dirty(), "to be saved");
    // Something else changed, then undone: the height stays.
    app.project.tracks[1].name = "Otro".into();
    app.commit_history();
    app.project.tracks[2].height = LaneHeight(Some(60.0));
    app.commit_history();
    app.undo();
    assert_ne!(app.project.tracks[1].name, "Otro");
    assert_eq!((app.project.tracks[0].height.0, app.project.tracks[2].height.0), (Some(120.0), Some(60.0)));
    app.redo();
    assert_eq!(app.project.tracks[1].name, "Otro");
    assert_eq!(app.project.tracks[2].height.0, Some(60.0));
    // Deleted and undone: back as high as it was.
    let id = app.project.tracks[2].id;
    app.delete_track(2);
    app.commit_history();
    app.undo();
    let back = app.project.tracks.iter().find(|t| t.id == id).unwrap();
    assert_eq!(back.height.0, Some(60.0));
    // All the lanes' (⌥ + wheel): the same.
    let mut app = PulseApp::headless(Project::demo());
    app.commit_history();
    app.project.lane_height = LaneHeight(Some(90.0));
    app.commit_history();
    assert!(!app.can_undo() && app.is_dirty());
    app.project.tracks[1].name = "Otro".into();
    app.commit_history();
    app.undo();
    assert_eq!(app.project.lane_height.0, Some(90.0));
}
