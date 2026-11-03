//! Many tracks at 1440×900, as in the second audit: the Arrangement scrolls up
//! and down with the wheel (the ruler and the master stay), clicks land on the
//! track under them after scrolling, the detail panel goes down with an audio
//! clip open, and the Session reaches its arm buttons, returns and master.

use crate::app::{ClipRef, PulseApp, View};
use crate::produccion::Studio;
use crate::project::Project;
use eframe::egui::{self, pos2, vec2, Event, Modifiers, Pos2};
use engine::model::{ArrangementClip, AudioClip, Clip, MidiClip, WarpMode};
use std::path::PathBuf;

/// `tracks` tracks with a clip each (the last one an audio track with the demo
/// song), in the Arrangement.
fn busy(tracks: usize) -> Studio {
    let mut app = PulseApp::headless(Project::empty());
    while app.project.tracks.len() < tracks - 1 {
        app.add_track();
    }
    for (t, track) in app.project.tracks.iter_mut().enumerate() {
        track.name = format!("Pista {}", t + 1);
        let clip = MidiClip::new(&format!("Clip {}", t + 1), 16.0, track.color);
        track.arrangement.push(ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Midi(clip) });
    }
    let voice = app.add_audio_track().unwrap();
    app.project.tracks[voice].name = format!("Pista {}", voice + 1);
    let audio = AudioClip {
        hard_start: false,
        fades: Default::default(),
        name: format!("Clip {}", voice + 1),
        color: app.project.tracks[voice].color,
        sample: crate::test_audio::demo_song().to_string_lossy().into_owned(),
        length: 32.0,
        offset: 0.0,
        warp: WarpMode::Off,
        original_bpm: 120.0,
        warp_markers: Vec::new(),
        key: None,
        takes: Vec::new(),
        gain_db: 0.0,
        transpose: 0.0,
        envelopes: Vec::new(),
        muted: false,
    };
    app.project.tracks[voice].arrangement.push(ArrangementClip { start: 16.0, length: 32.0, clip: Clip::Audio(audio) });
    app.sync_all();
    app.view = View::Arrangement;
    // The panel as tall as when these were written (290 points at 1440×900):
    // they are about the tracks over it.
    app.settings.detail_share = Some(0.36);
    // On the devices, so it keeps that height with no clip selected.
    app.detail = crate::app::Detail::Devices;
    let mut s = Studio::new(app, std::env::temp_dir());
    s.frames(4);
    s
}

fn wheel(s: &mut Studio, at: Pos2, delta: egui::Vec2, mods: Modifiers) {
    s.move_to(at, mods);
    for _ in 0..4 {
        let event = Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: delta / 4.0, phase: egui::TouchPhase::Move, modifiers: mods };
        s.frame_with(vec![event], mods);
    }
    s.frames(20);
}

fn shots() -> Option<PathBuf> {
    std::env::var_os("PULSE_SHOTS").map(PathBuf::from)
}

fn shot(s: &mut Studio, name: &str) {
    if let Some(dir) = shots() {
        std::fs::create_dir_all(&dir).unwrap();
        s.out = dir;
        s.shot(name);
    }
}

/// Where the text `text` is painted highest on screen (a track's name in its
/// header, a clip's in its lane: the detail panel is below them).
fn painted(s: &Studio, text: &str) -> egui::Rect {
    s.seen.iter().filter(|w| w.text == text).map(|w| w.rect).min_by(|a, b| a.top().total_cmp(&b.top())).unwrap_or_else(|| panic!("no veo «{text}»"))
}

/// The top of the detail panel (its «Clip» tab).
fn panel_top(s: &Studio) -> f32 {
    s.seen.iter().filter(|w| w.text == "Clip" && w.role != "Texto").map(|w| w.rect.top()).fold(f32::INFINITY, f32::min)
}

/// Whether row `name` of the Arrangement can be seen: under the ruler, above
/// the master's row.
fn in_sight(s: &Studio, name: &str) -> bool {
    let row = painted(s, name);
    let master = painted(s, "Master");
    let ruler = painted(s, "1");
    row.top() > ruler.bottom() && row.bottom() < master.top()
}

/// 16 tracks don't fit at 1440×900: the last ones are under the master's row,
/// which stays at the bottom of the view; the wheel brings them up, and up again.
#[test]
fn the_wheel_reaches_the_last_track_and_the_master_stays() {
    let mut s = busy(16);
    assert!(in_sight(&s, "Pista 1"));
    assert!(!in_sight(&s, "Pista 16"), "16 tracks don't fit");
    let master = painted(&s, "Master");
    assert!(master.bottom() < panel_top(&s), "the master's row is in sight");
    wheel(&mut s, pos2(900.0, 300.0), vec2(0.0, -3000.0), Modifiers::NONE);
    assert!(in_sight(&s, "Pista 16"), "the wheel goes down to the last track");
    assert!(!in_sight(&s, "Pista 1"));
    assert_eq!(painted(&s, "Master"), master, "the master stays where it was");
    // Over the headers too, and back up.
    wheel(&mut s, pos2(330.0, 300.0), vec2(0.0, 3000.0), Modifiers::NONE);
    assert!(in_sight(&s, "Pista 1"));
    assert_eq!(s.app.arrangement.scroll_y, 0.0);
    // ⇧ + wheel goes sideways, not down.
    wheel(&mut s, pos2(900.0, 300.0), vec2(0.0, -600.0), Modifiers::SHIFT);
    assert_eq!(s.app.arrangement.scroll_y, 0.0);
    assert!(s.app.arrangement.scroll_x > 100.0, "{}", s.app.arrangement.scroll_x);
}

/// Scrolled down, a clic on a clip selects that clip, one on a header that track,
/// and a double clic in an empty place makes a clip on the track under it.
#[test]
fn clicks_land_on_the_track_under_them_after_scrolling() {
    let mut s = busy(16);
    wheel(&mut s, pos2(900.0, 300.0), vec2(0.0, -400.0), Modifiers::NONE);
    assert!(s.app.arrangement.scroll_y > 300.0);
    let clip = painted(&s, "Clip 12");
    s.click_at(clip.center() + vec2(0.0, 12.0));
    assert_eq!(s.app.selected_clip, Some(ClipRef::Arrangement { track: 11, index: 0 }));
    let header = painted(&s, "Pista 13");
    s.click_at(header.center());
    assert_eq!(s.app.selected_target, engine::Target::Track(12));
    let lane = painted(&s, "Clip 10");
    s.dclick_at(pos2(lane.left() + 600.0, lane.center().y + 12.0));
    assert_eq!(s.app.project.tracks[9].arrangement.len(), 2, "a new clip on «Pista 10»");
    assert!((0..16).filter(|&t| t != 9).all(|t| s.app.project.tracks[t].arrangement.len() == 1));
    // A clip dragged down near the bottom scrolls the tracks down.
    let before = s.app.arrangement.scroll_y;
    let clip = painted(&s, "Clip 10");
    let low = painted(&s, "Master").top() - 6.0;
    s.move_to(clip.center(), Modifiers::NONE);
    let from = clip.center() + vec2(0.0, 12.0);
    s.frame_with(vec![Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }], Modifiers::NONE);
    for i in 1..=30 {
        let at = from + (pos2(from.x, low) - from) * (i.min(10) as f32 / 10.0);
        s.frame_with(vec![Event::PointerMoved(at)], Modifiers::NONE);
    }
    assert!(s.app.arrangement.scroll_y > before + 40.0, "{before} → {}", s.app.arrangement.scroll_y);
    s.frame_with(vec![Event::PointerButton { pos: pos2(from.x, low), button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
    s.frames(3);
    let moved = (0..16).find(|&t| s.app.project.tracks[t].arrangement.iter().any(|c| c.clip.name() == "Clip 10")).unwrap();
    assert!(moved > 9, "it went down with the view: {moved}");
}

/// A track's height: dragging the bottom of its header; ⌥ + wheel, all of them.
#[test]
fn tracks_get_taller() {
    let mut s = busy(6);
    let header = painted(&s, "Pista 2");
    let next = painted(&s, "Pista 3");
    let edge = pos2(header.left() + 60.0, next.top() - 7.0);
    s.drag(edge, edge + vec2(0.0, 40.0), Modifiers::NONE);
    let lane = |s: &Studio, t: usize| s.app.arrangement.lane(&s.app.project.tracks[t]);
    assert!((lane(&s, 1) - 92.0).abs() < 2.0, "{}", lane(&s, 1));
    assert_eq!(lane(&s, 0), crate::arrangement::LANE_H);
    assert!((painted(&s, "Pista 3").top() - next.top() - 40.0).abs() < 2.0);
    wheel(&mut s, pos2(900.0, 300.0), vec2(0.0, 80.0), Modifiers::ALT);
    assert!(lane(&s, 0) > crate::arrangement::LANE_H + 10.0);
    assert_eq!(s.app.arrangement.scroll_y, 0.0);
}

/// With an audio clip open (its editor is tall), the panel still goes down: what
/// doesn't fit scrolls inside it.
#[test]
fn the_panel_goes_down_with_an_audio_clip_open() {
    let mut s = busy(10);
    // Its height with a MIDI clip open (with none, it is only its tabs).
    s.app.select_clip(ClipRef::Arrangement { track: 0, index: 0 });
    s.frames(4);
    let closed = panel_top(&s);
    let voice = s.app.project.tracks.len() - 1;
    s.app.select_clip(ClipRef::Arrangement { track: voice, index: 0 });
    s.frames(4);
    let top = panel_top(&s);
    assert!((top - closed).abs() < 4.0, "the editor doesn't make the panel grow: {closed} → {top}");
    // Up, to see more of the editor, and down again, past where it was.
    s.drag(pos2(900.0, top - 7.0), pos2(900.0, top - 207.0), Modifiers::NONE);
    let raised = panel_top(&s);
    assert!(raised < top - 190.0, "{top} → {raised}");
    s.drag(pos2(900.0, raised - 7.0), pos2(900.0, raised + 293.0), Modifiers::NONE);
    let lowered = panel_top(&s);
    assert!(lowered > top + 60.0, "{raised} → {lowered}");
    assert!(in_sight(&s, "Pista 8"), "more tracks in sight");
}

/// The Session with 16 tracks: the returns and the master are on the right,
/// the arm buttons above the panel, whatever is scrolled.
#[test]
fn the_session_keeps_its_master_and_mixer_in_sight() {
    let mut s = busy(16);
    s.app.view = View::Session;
    s.frames(6);
    let panel = panel_top(&s);
    let master = painted(&s, "Master");
    assert!(master.right() <= 1440.0 && master.left() > 1000.0, "{master:?}");
    assert!(painted(&s, "A Reverb").right() < master.left());
    let arms: Vec<egui::Rect> = s.seen.iter().filter(|w| w.text == "●" && w.role != "Texto").map(|w| w.rect).collect();
    assert!(!arms.is_empty() && arms.iter().all(|r| r.bottom() < panel), "{arms:?} / {panel}");
    s.click_at(master.center());
    assert_eq!(s.app.selected_target, engine::Target::Master);
    // Sideways: the tracks go, the master stays.
    let first = painted(&s, "Pista 1");
    wheel(&mut s, pos2(600.0, 200.0), vec2(0.0, -800.0), Modifiers::SHIFT);
    assert!(painted(&s, "Pista 1").left() < first.left() - 100.0);
    assert_eq!(painted(&s, "Master"), master);
}

/// With nothing to edit, the panel is only its tabs: the Arrangement gets the
/// window (it took 60 % in the third audit); a clip opened, it comes back to
/// its height, and goes again when the clip is let go.
#[test]
fn the_empty_panel_leaves_the_room_to_the_view() {
    let mut s = busy(16);
    s.app.settings.detail_share = None;
    s.app.detail = crate::app::Detail::Clip;
    s.frames(40);
    let empty = panel_top(&s);
    assert!(empty > s.size.y - 110.0, "{empty}");
    assert!(s.has("Selecciona un clip para editarlo · doble clic en un slot vacío o en una pista del Arrangement crea uno nuevo"));
    let tracks = (1..=16).filter(|t| in_sight(&s, &format!("Pista {t}"))).count();
    s.app.select_clip(ClipRef::Arrangement { track: 0, index: 0 });
    s.frames(4);
    let open = panel_top(&s);
    assert!(open < empty - 250.0, "{empty} → {open}");
    let fewer = (1..=16).filter(|t| in_sight(&s, &format!("Pista {t}"))).count();
    assert!(tracks >= fewer + 4, "{tracks} tracks without the editor, {fewer} with it");
    s.app.selected_clip = None;
    // Not at once: the second click of a double click lands where the first did.
    s.frames(4);
    assert!((panel_top(&s) - open).abs() < 2.0, "{open} → {}", panel_top(&s));
    s.frames(40);
    assert!((panel_top(&s) - empty).abs() < 2.0);
    // The devices always have something to show: their tab keeps its height.
    s.app.detail = crate::app::Detail::Devices;
    s.frames(40);
    assert!((panel_top(&s) - open).abs() < 2.0, "{open} → {}", panel_top(&s));
}

/// The zoom fits a whole song: at its least, five minutes at 140 BPM in a
/// narrow window; «Ver todo» fits the 104 bars of the audit's song.
#[test]
fn the_whole_song_fits_in_the_arrangement() {
    use crate::arrangement::{fit_zoom, ZOOM_MIN};
    assert!(ZOOM_MIN * 700.0 <= 1000.0 - 150.0 - 190.0 - 200.0, "five minutes at 140 BPM in a window 1000 wide");
    let mut s = busy(4);
    s.size = vec2(1000.0, 720.0);
    let beats = 104.0 * 4.0;
    s.app.project.tracks[0].arrangement[0].length = beats;
    s.frames(4);
    s.click("Ver todo");
    s.frames(4);
    let ppb = s.app.arrangement.pixels_per_beat;
    // The ruler's «1» is 3 points after the song's start.
    let end = painted(&s, "1").left() - 3.0 + ppb * beats as f32;
    assert!(end <= 1000.0, "the song ends at {end}");
    assert!(end > 900.0, "and fills the view: {end}");
    assert_eq!(s.app.arrangement.scroll_x, 0.0);
    assert!((fit_zoom(beats, 600.0) - 584.0 / 416.0).abs() < 1e-4);
    assert_eq!(fit_zoom(20_000.0, 600.0), ZOOM_MIN);
}

/// With more tracks than fit, the returns narrow to their fader and pan, and
/// the tracks keep the width.
#[test]
fn the_returns_narrow_when_the_tracks_dont_fit() {
    let width = |s: &Studio, name: &str| s.seen.iter().find(|w| w.text == format!("Pista {name}") && w.role != "Texto").map(|w| w.rect.width()).unwrap();
    let mut s = busy(4);
    s.app.view = View::Session;
    s.frames(6);
    assert!(width(&s, "A Reverb") > 100.0, "with room, as wide as a track");
    let mut s = busy(16);
    s.app.view = View::Session;
    s.frames(6);
    assert!(width(&s, "A Reverb") < 70.0 && width(&s, "Pista 1") > 100.0);
}

/// The pictures of the audit's problems, before and after:
/// `PULSE_SHOTS=dir cargo test --release -p app many_tracks_shots -- --ignored`.
#[test]
#[ignore]
fn many_tracks_shots() {
    if shots().is_none() {
        return;
    }
    let mut s = busy(16);
    shot(&mut s, "arrangement-16-pistas");
    wheel(&mut s, pos2(900.0, 300.0), vec2(0.0, -2000.0), Modifiers::NONE);
    shot(&mut s, "arrangement-rueda-abajo");
    let voice = s.app.project.tracks.len() - 1;
    s.app.select_clip(ClipRef::Arrangement { track: voice, index: 0 });
    s.frames(4);
    shot(&mut s, "clip-de-audio-abierto");
    let top = panel_top(&s);
    s.drag(pos2(900.0, top - 7.0), pos2(900.0, top + 230.0), Modifiers::NONE);
    shot(&mut s, "panel-bajado");
    s.app.view = View::Session;
    s.app.selected_clip = None;
    s.frames(4);
    shot(&mut s, "session-16-pistas");
    // A clip open in the editor, at the panel's own height: the scenes stay.
    let mut clip = MidiClip::new("Acordes", 16.0, s.app.project.tracks[0].color);
    clip.notes.push(engine::model::Note::new(0.0, 4.0, 60, 0.8));
    s.app.set_clip(0, 0, Some(Clip::Midi(clip)));
    s.app.select_clip(ClipRef::Session { track: 0, slot: 0 });
    s.app.settings.detail_share = None;
    shot(&mut s, "session-con-editor");
    // A song of 104 bars, whole.
    s.app.selected_clip = None;
    s.app.project.tracks[0].arrangement[0].length = 416.0;
    s.app.view = View::Arrangement;
    s.frames(40);
    s.click("Ver todo");
    shot(&mut s, "arrangement-104-compases");
    s.size = vec2(1000.0, 720.0);
    s.frames(6);
    shot(&mut s, "session-estrecha");
    s.app.view = View::Arrangement;
    s.frames(6);
    shot(&mut s, "arrangement-estrecha");
}
