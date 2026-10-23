//! Arrangement view: a timeline per track with clips you can place, move and resize.
//!
//! - Drag in the top strip to set the loop region; click the ruler to move the start marker.
//! - Double-click an empty lane to create a clip; drag a clip to move it (also across
//!   tracks), drag its right edge to resize it. Clip contents loop when it is longer.
//! - The "A" button of a track shows an automation lane below it: click to add a
//!   breakpoint, drag to move it, double-click to delete it.
//! - A loop recording shows a lane per take below its track: drag across one to
//!   have that take play there (comping), click it for the whole clip.

use crate::app::{ClipRef, Placement, PulseApp};
use crate::theme::{self, ACCENT, SOLO, TEXT, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine::model::{ArrangementClip, AutomationPoint, Clip, MidiClip, Param};
use std::collections::{HashMap, HashSet};
use engine::{Sample, PEAK_BLOCK};
use engine::{Command, Target};
use std::sync::atomic::Ordering;

const HEADER_W: f32 = 150.0;
const LOOP_H: f32 = 12.0;
/// The markers' strip, between the loop's and the ruler.
const MARKER_H: f32 = 15.0;
const RULER_H: f32 = 20.0;
const LANE_H: f32 = 52.0;
const EDGE_GRAB: f32 = 6.0;
const AUTO_H: f32 = 64.0;
const POINT_GRAB: f32 = 6.0;
const TAKE_H: f32 = 24.0;

#[derive(Clone, Debug)]
enum Drag {
    /// A marker grabbed: dragged it moves, let go without moving it jumps there.
    Marker { index: usize, moved: bool },
    /// The selected clips, grabbed at `grab` (beat) on `grab_track`. They are laid
    /// out again from `snapshot` (every lane before the drag) each time the offset
    /// changes; `applied` is the offset in place (beats, tracks).
    Move { grab_track: usize, grab: f64, moved: Vec<(usize, usize)>, primary: usize, snapshot: Vec<Vec<ArrangementClip>>, copy: bool, applied: (f64, isize) },
    Resize { track: usize, index: usize },
    Loop { anchor: f64 },
    Scrub,
    /// Dragging a breakpoint; `lane` is the lane's rectangle when the drag began.
    Point { track: usize, param: Param, index: usize, lane: Rect },
    /// Bending the stretch that starts at point `index` (⌥ + drag): its bend and
    /// the pointer's height when the drag began.
    Bend { track: usize, param: Param, index: usize, start: f32, from_y: f32 },
    /// Choosing a take over a stretch: from `anchor` to `to` (beats) in its lane.
    Comp { track: usize, take: usize, anchor: f64, to: f64 },
    /// An audio clip's fade-in (or fade-out, `out`) handle.
    Fade { track: usize, index: usize, out: bool },
}

/// Size of the fade handles at an audio clip's top corners.
const FADE_HANDLE: f32 = 7.0;

/// Where a clip drawn in `rect` has its fade handles: (in, out), just under
/// its title, at the end of each fade.
fn fade_handles(rect: Rect, fades: engine::model::Fades, ppb: f32) -> [Rect; 2] {
    let y = rect.top() + 15.0;
    let x_in = (rect.left() + fades.fade_in as f32 * ppb).min(rect.right() - FADE_HANDLE);
    let x_out = (rect.right() - fades.fade_out as f32 * ppb).max(rect.left() + FADE_HANDLE);
    [
        Rect::from_min_size(pos2(x_in, y), vec2(FADE_HANDLE, FADE_HANDLE)),
        Rect::from_min_size(pos2(x_out - FADE_HANDLE, y), vec2(FADE_HANDLE, FADE_HANDLE)),
    ]
}

/// The fade handle of a selected audio clip of track `t` under `pos`: (clip, out).
fn fade_at(app: &PulseApp, t: usize, pos: egui::Pos2, clip_top: f32, ppb: f32, origin_x: f32) -> Option<(usize, bool)> {
    app.project.tracks[t].arrangement.iter().enumerate().rev().find_map(|(index, placed)| {
        let Clip::Audio(audio) = &placed.clip else { return None };
        if !app.is_clip_selected(ClipRef::Arrangement { track: t, index }) {
            return None;
        }
        let rect = Rect::from_min_max(pos2(origin_x + placed.start as f32 * ppb, clip_top), pos2(origin_x + placed.end() as f32 * ppb, clip_top + LANE_H - 2.0));
        let [fade_in, fade_out] = fade_handles(rect, audio.fades, ppb);
        if fade_in.expand(3.0).contains(pos) {
            Some((index, false))
        } else if fade_out.expand(3.0).contains(pos) {
            Some((index, true))
        } else {
            None
        }
    })
}

pub struct State {
    pub pixels_per_beat: f32,
    pub follow: bool,
    drag: Option<Drag>,
    /// Tracks showing an automation lane, and the parameter it shows.
    pub automation: HashMap<usize, Param>,
    /// The master's automation lane (the last row), if shown: tempo, volume…
    pub master_automation: Option<Param>,
    /// Tracks showing a lane per take of their loop recordings.
    pub takes: HashSet<usize>,
    /// What the automation parameter list is filtered by (long lists: plugins).
    pub automation_filter: String,
    /// What was right-clicked, for its menu.
    menu: Option<Menu>,
    /// Where a clic on a clip left the cut point (⌘E cuts there), in beats.
    pub(crate) cut: Option<f64>,
    /// The project as the drag in progress left it last frame.
    left: Option<Box<crate::project::Project>>,
    /// A track to duplicate or delete once the view is drawn (true: the selected ones).
    track_action: Option<(TrackAction, usize, bool)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TrackAction {
    Duplicate,
    Delete,
    Group,
    Ungroup,
    LeaveGroup,
    Fold,
    Rumble,
    SoundApart(usize),
}

/// What a right-click landed on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Menu {
    /// A clip, and the beat clicked (snapped), where «Dividir aquí» cuts.
    Clip(ClipRef, f64),
    /// An empty place on a track, at a beat (snapped to the grid).
    Lane { track: usize, beat: f64 },
    Automation { track: usize, param: Param },
    /// A marker.
    Marker(usize),
    /// The markers' strip, at a beat (snapped).
    Markers(f64),
}

impl State {
    /// Lets go of everything about the project shown (another one opens).
    pub(crate) fn forget_project(&mut self) {
        self.drag = None;
        self.left = None;
        self.cut = None;
        self.menu = None;
        self.track_action = None;
        self.automation.clear();
        self.master_automation = None;
        self.takes.clear();
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            pixels_per_beat: 14.0,
            follow: false,
            drag: None,
            automation: HashMap::new(),
            master_automation: None,
            takes: HashSet::new(),
            automation_filter: String::new(),
            menu: None,
            cut: None,
            left: None,
            track_action: None,
        }
    }
}

fn snap_unit(pixels_per_beat: f32) -> f64 {
    if pixels_per_beat >= 10.0 {
        1.0
    } else {
        4.0
    }
}

fn snap(beat: f64, unit: f64) -> f64 {
    (beat / unit).round() * unit
}

/// Breakpoints snap more finely than clips when zoomed in.
fn point_unit(pixels_per_beat: f32) -> f64 {
    if pixels_per_beat >= 24.0 {
        0.25
    } else {
        snap_unit(pixels_per_beat)
    }
}

/// Vertical position of each track row (relative to the first lane), its
/// automation lane, if shown, and how many take lanes it shows.
struct Layout {
    rows: Vec<(f32, Option<Param>, usize)>,
    /// Inside a folded group (no height, not drawn).
    hidden: Vec<bool>,
    /// Groups: their lane shows their tracks' clips, and takes none.
    groups: Vec<bool>,
    height: f32,
}

impl Layout {
    fn new(app: &PulseApp) -> Self {
        let mut rows = Vec::new();
        let mut y = 0.0;
        let tracks = &app.project.tracks;
        let hidden: Vec<bool> = (0..tracks.len()).map(|t| crate::groups::hidden(tracks, t)).collect();
        let groups: Vec<bool> = tracks.iter().map(|t| t.is_group()).collect();
        for t in 0..tracks.len() {
            let shown = app.arrangement.automation.get(&t).copied();
            let takes = if app.arrangement.takes.contains(&t) && !groups[t] { crate::comping::take_count(app, t) } else { 0 };
            rows.push((y, shown, takes));
            if !hidden[t] {
                y += row_height(shown.is_some(), takes);
            }
        }
        // The master's row, last: no clips, its automation.
        let (mut hidden, mut groups) = (hidden, groups);
        let master = app.arrangement.master_automation;
        rows.push((y, master, 0));
        hidden.push(false);
        groups.push(true);
        y += row_height(master.is_some(), 0);
        Self { rows, hidden, groups, height: y }
    }

    fn shown(&self, t: usize) -> bool {
        !self.hidden.get(t).copied().unwrap_or(true)
    }

    /// Track under `y` (relative to the first lane) and whether it is in its automation lane.
    fn hit(&self, y: f32) -> Option<(usize, bool)> {
        self.rows.iter().enumerate().filter(|&(t, _)| self.shown(t)).find_map(|(t, &(top, shown, takes))| {
            let clips_end = top + LANE_H + 1.0;
            let auto_top = clips_end + takes_height(takes);
            // A group's lane holds no clips of its own.
            if y >= top && y < clips_end && self.groups[t] {
                None
            } else if y >= top && y < clips_end {
                Some((t, false))
            } else if shown.is_some() && y >= auto_top && y < auto_top + AUTO_H + 1.0 {
                Some((t, true))
            } else {
                None
            }
        })
    }

    /// The track and take of the take lane under `y`.
    fn take_hit(&self, y: f32) -> Option<(usize, usize)> {
        self.rows.iter().enumerate().filter(|&(t, _)| self.shown(t)).find_map(|(t, &(top, _, takes))| {
            let below = y - (top + LANE_H + 1.0);
            (below >= 0.0 && below < takes_height(takes)).then(|| (t, (below / (TAKE_H + 1.0)) as usize))
        })
    }
}

fn takes_height(takes: usize) -> f32 {
    takes as f32 * (TAKE_H + 1.0)
}

fn row_height(automation: bool, takes: usize) -> f32 {
    LANE_H + 1.0 + takes_height(takes) + if automation { AUTO_H + 1.0 } else { 0.0 }
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    // Something else changed the project in the middle of a drag (a key like ⌫
    // or ⌘Z, a menu, a recording): the clips, points and snapshot the drag holds
    // no longer fit it, so the drag ends here, leaving things as they are.
    if app.arrangement.drag.is_some() && app.arrangement.left.as_deref().is_some_and(|left| *left != app.project) {
        app.arrangement.drag = None;
    }
    let position = app.audio.position();
    let playing = app.audio.is_playing();
    let state = app.audio.state();

    // Toolbar.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Zoom").color(TEXT_DIM).size(10.5));
        ui.add(egui::Slider::new(&mut app.arrangement.pixels_per_beat, 3.0..=60.0).logarithmic(true).show_value(false));
        ui.checkbox(&mut app.arrangement.follow, "Seguir cursor");
        let hint = crate::theme::keys("Doble clic: nuevo clip · clic en un clip: punto de corte (⌘E corta ahí) · arrastra: mover (⌥ copia) · ⌘ clic: varios · arrastrar en vacío: recuadro · borde derecho: longitud · franja superior: loop");
        // Cut with "…" in a narrow window; the tooltip has all of it.
        ui.add(egui::Label::new(egui::RichText::new(&hint).color(TEXT_DIM).size(10.5)).truncate()).on_hover_text(hint);
    });
    ui.add_space(2.0);

    let ppb = app.arrangement.pixels_per_beat;
    let unit = snap_unit(ppb);
    let tracks = app.project.tracks.len();
    let content_end = app
        .project
        .tracks
        .iter()
        .map(|t| t.arrangement_end())
        .fold(app.project.loop_end, f64::max);
    let total_beats = ((content_end + 64.0) / 16.0).ceil() * 16.0;
    app.arrangement.automation.retain(|&t, _| t < tracks);
    app.arrangement.takes.retain(|&t| t < tracks);
    // The takes shown are drawn from their files: decoded now if they weren't yet.
    let shown_takes: Vec<String> = app
        .arrangement
        .takes
        .iter()
        .flat_map(|&t| app.project.tracks[t].arrangement.iter())
        .filter_map(|c| match &c.clip {
            Clip::Audio(audio) => Some(audio.takes.iter().cloned()),
            Clip::Midi(_) => None,
        })
        .flatten()
        .collect();
    for path in shown_takes {
        if app.loaded_sample(&path).is_none() {
            app.sample(&path);
        }
    }
    let layout = Layout::new(app);
    let lanes_height = LOOP_H + MARKER_H + RULER_H + layout.height;
    let mut headers = Vec::new();
    let mut clip_rects = Vec::new();
    let mut lane_rects = Vec::new();

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

        // Track headers.
        ui.vertical(|ui| {
            ui.set_width(HEADER_W);
            ui.add_space(LOOP_H + MARKER_H + RULER_H);
            for t in (0..tracks).filter(|&t| layout.shown(t)) {
                headers.push((t, track_header(app, ui, t, layout.rows[t].1, layout.rows[t].2)));
            }
            master_header(app, ui, tracks, layout.rows[tracks].1);
        });

        let mut scroll = egui::ScrollArea::horizontal().id_salt("arrangement").auto_shrink(false);
        if app.arrangement.follow && playing {
            let view = ui.available_width();
            let x = position as f32 * ppb;
            scroll = scroll.horizontal_scroll_offset((x - view * 0.3).max(0.0));
        }
        scroll.show(ui, |ui| {
            // At least as wide as the view: zoomed out, the timeline still fills it.
            let total_beats = total_beats.max((ui.available_width() / ppb / 16.0).ceil() as f64 * 16.0);
            let size = vec2(total_beats as f32 * ppb, lanes_height.max(ui.available_height()));
            let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
            crate::widgets::name(&response, egui::WidgetType::Other, "Línea de tiempo");
            let painter = ui.painter_at(rect);
            let x_at = |beat: f64| rect.left() + beat as f32 * ppb;
            let lanes_top = rect.top() + LOOP_H + MARKER_H + RULER_H;
            let lane_rect = |t: usize| {
                Rect::from_min_size(pos2(rect.left(), lanes_top + layout.rows[t].0), vec2(rect.width(), LANE_H))
            };
            let auto_rect = |t: usize| {
                let (top, _, takes) = layout.rows[t];
                Rect::from_min_size(pos2(rect.left(), lanes_top + top + LANE_H + 1.0 + takes_height(takes)), vec2(rect.width(), AUTO_H))
            };
            let take_rect = |t: usize, k: usize| {
                let top = lanes_top + layout.rows[t].0 + LANE_H + 1.0 + k as f32 * (TAKE_H + 1.0);
                Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), TAKE_H))
            };

            // Background, lanes and grid.
            painter.rect_filled(rect, 0.0, theme::BG);
            for t in (0..tracks).filter(|&t| layout.shown(t)) {
                if layout.groups[t] {
                    painter.rect_filled(lane_rect(t), 0.0, theme::shade(theme::SLOT, 0.8));
                    continue;
                }
                painter.rect_filled(lane_rect(t), 0.0, theme::SLOT);
                lane_rects.push((t, lane_rect(t)));
                app.drop_targets.lanes.push((t, lane_rect(t).intersect(ui.clip_rect()), rect.left(), ppb));
            }
            let label_every = if ppb < 6.0 { 4 } else if ppb < 10.0 { 2 } else { 1 };
            for beat in 0..=total_beats as i64 {
                let x = x_at(beat as f64);
                let bar = beat % 4 == 0;
                if !bar && ppb < 8.0 {
                    continue;
                }
                let color = if bar { Color32::from_gray(58) } else { Color32::from_gray(44) };
                painter.line_segment([pos2(x, lanes_top), pos2(x, rect.bottom())], Stroke::new(1.0, color));
                if bar && (beat / 4) % label_every == 0 {
                    painter.line_segment(
                        [pos2(x, rect.top() + LOOP_H + MARKER_H + 8.0), pos2(x, lanes_top)],
                        Stroke::new(1.0, TEXT_DIM),
                    );
                    painter.text(
                        pos2(x + 3.0, rect.top() + LOOP_H + MARKER_H + 2.0),
                        Align2::LEFT_TOP,
                        (beat / 4 + 1).to_string(),
                        theme::small(),
                        TEXT_DIM,
                    );
                }
            }

            // Loop region.
            let loop_rect = Rect::from_min_max(
                pos2(x_at(app.project.loop_start), rect.top() + 1.0),
                pos2(x_at(app.project.loop_end), rect.top() + LOOP_H - 1.0),
            );
            let loop_color = if app.project.loop_enabled { ACCENT } else { Color32::from_gray(90) };
            painter.rect_filled(loop_rect, 1.0, loop_color);
            if app.project.loop_enabled {
                let overlay = Rect::from_min_max(pos2(loop_rect.left(), lanes_top), pos2(loop_rect.right(), rect.bottom()));
                painter.rect_filled(overlay, 0.0, Color32::from_rgba_unmultiplied(255, 168, 36, 10));
            }
            // Punch in / out: red lines where recording starts and stops.
            for (on, beat) in [(app.project.punch_in, app.project.loop_start), (app.project.punch_out, app.project.loop_end)] {
                if on {
                    let x = x_at(beat);
                    painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.5, theme::METER_RED.gamma_multiply(0.8)));
                }
            }

            // A group's lane: its tracks' clips, one thin strip each.
            for g in (0..tracks).filter(|&t| layout.shown(t) && layout.groups[t]) {
                let lane = lane_rect(g);
                let inside: Vec<usize> = crate::groups::children(&app.project.tracks, g).filter(|&c| !layout.groups[c]).collect();
                let strip = (LANE_H - 6.0) / inside.len().max(1) as f32;
                for (k, &c) in inside.iter().enumerate() {
                    for placed in &app.project.tracks[c].arrangement {
                        let top = lane.top() + 3.0 + k as f32 * strip;
                        let piece = Rect::from_min_max(pos2(x_at(placed.start), top), pos2(x_at(placed.end()), top + (strip - 1.0).max(1.0)));
                        painter.rect_filled(piece, 1.0, theme::shade(theme::rgb(placed.clip.color()), 0.8));
                    }
                }
            }

            // Clips.
            for t in (0..tracks).filter(|&t| layout.shown(t) && !layout.groups[t]) {
                let lane = lane_rect(t);
                for (index, placed) in app.project.tracks[t].arrangement.iter().enumerate() {
                    let clip_rect = Rect::from_min_max(
                        pos2(x_at(placed.start), lane.top() + 1.0),
                        pos2(x_at(placed.end()), lane.bottom() - 1.0),
                    );
                    let selected = app.is_clip_selected(ClipRef::Arrangement { track: t, index });
                    clip_rects.push((ClipRef::Arrangement { track: t, index }, clip_rect));
                    let sample = match &placed.clip {
                        Clip::Audio(audio) => app.loaded_sample(&audio.sample).map(|s| s.as_ref()),
                        Clip::Midi(_) => None,
                    };
                    paint_clip(&painter, clip_rect, placed, sample, selected, ppb, app.project.tempo);
                }
            }

            // Take lanes: each take of the loop recordings, bright where it plays.
            for t in (0..tracks).filter(|&t| layout.shown(t)) {
                for k in 0..layout.rows[t].2 {
                    let lane = take_rect(t, k);
                    painter.rect_filled(lane, 0.0, theme::shade(theme::SLOT, 0.8));
                    for placed in &app.project.tracks[t].arrangement {
                        let Clip::Audio(audio) = &placed.clip else { continue };
                        let Some(path) = audio.takes.get(k) else { continue };
                        let piece = Rect::from_min_max(pos2(x_at(placed.start), lane.top() + 1.0), pos2(x_at(placed.end()), lane.bottom() - 1.0));
                        let playing = audio.sample == *path;
                        let color = theme::rgb(audio.color);
                        painter.rect_filled(piece, 2.0, theme::shade(color, if playing { 0.45 } else { 0.18 }));
                        if let Some(sample) = app.loaded_sample(path).filter(|s| s.frames() > 0) {
                            let clipped = painter.with_clip_rect(piece.intersect(painter.clip_rect()));
                            let take = engine::model::AudioClip { sample: path.clone(), ..audio.clone() };
                            paint_wave(&clipped, piece, piece.shrink2(vec2(0.0, 2.0)), &take, sample, ppb, app.project.tempo, theme::shade(color, if playing { 1.25 } else { 0.6 }));
                        }
                        if playing {
                            widgets::outline(&painter, piece, theme::shade(color, 1.1));
                        }
                    }
                }
            }
            if let Some(Drag::Comp { track, take, anchor, to }) = app.arrangement.drag {
                if take < layout.rows.get(track).map_or(0, |r| r.2) {
                    let lane = take_rect(track, take);
                    let (a, b) = (anchor.min(to), anchor.max(to));
                    let stretch = Rect::from_min_max(pos2(x_at(a), lane.top()), pos2(x_at(b).max(x_at(a) + 1.0), lane.bottom()));
                    painter.rect_filled(stretch, 2.0, Color32::from_white_alpha(40));
                }
            }

            painter.rect_filled(lane_rect(tracks), 0.0, theme::shade(theme::SLOT, 0.7));
            // Automation lanes (the master's too).
            for t in (0..=tracks).filter(|&t| layout.shown(t)) {
                if let Some(param) = layout.rows[t].1 {
                    paint_automation(app, &painter, auto_rect(t), t, param, &x_at);
                }
            }

            // Playhead and start marker.
            let following = |t: usize| {
                state.as_ref().is_none_or(|s| s.tracks[t].follows_arrangement.load(Ordering::Relaxed))
            };
            let x = x_at(position);
            painter.line_segment([pos2(x, rect.top() + LOOP_H), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::WHITE));
            widgets::play_triangle(&painter, pos2(x, rect.top() + LOOP_H + MARKER_H + RULER_H - 6.0), 8.0, Color32::WHITE);
            // The cut point, over the selected clips' lanes.
            if let Some(cut) = app.arrangement.cut {
                let x = x_at(cut);
                for t in app.clip_selection().iter().filter_map(|c| match *c {
                    ClipRef::Arrangement { track, index } => app.project.tracks.get(track)?.arrangement.get(index).filter(|p| p.start < cut && cut < p.end()).map(|_| track),
                    ClipRef::Session { .. } => None,
                }) {
                    if layout.shown(t) {
                        let lane = lane_rect(t);
                        painter.line_segment([pos2(x, lane.top()), pos2(x, lane.bottom())], Stroke::new(1.5, theme::ACCENT));
                        painter.add(egui::Shape::convex_polygon(vec![pos2(x - 4.0, lane.top()), pos2(x + 4.0, lane.top()), pos2(x, lane.top() + 5.0)], theme::ACCENT, Stroke::NONE));
                    }
                }
            }
            paint_markers(app, &painter, rect, &x_at);
            // Region being recorded on armed tracks.
            if let Some(take) = &app.recorder.arrangement {
                for t in (0..tracks).filter(|&t| app.project.tracks[t].armed && !app.project.tracks[t].is_audio()) {
                    let lane = lane_rect(t);
                    // From the punch-in, if it is later.
                    let from = app.project.punch().0.map_or(take.start, |p| take.start.max(p));
                    let region = Rect::from_min_max(pos2(x_at(from), lane.top()), pos2(x.max(x_at(from)), lane.bottom()));
                    painter.rect_filled(region, 2.0, Color32::from_rgba_unmultiplied(240, 82, 70, 60));
                }
            }
            for t in (0..tracks).filter(|&t| layout.shown(t) && !layout.groups[t] && !following(t)) {
                // Tracks playing session clips are dimmed, as their arrangement is overridden.
                painter.rect_filled(lane_rect(t), 0.0, Color32::from_black_alpha(110));
            }

            handle_pointer(app, ui, &response, rect, lanes_top, unit, x_at(0.0), &layout);
            context_menu(app, &response);
        });
    });
    // How the drag leaves the project, to notice next frame if something else changed it.
    app.arrangement.left = app.arrangement.drag.as_ref().filter(|d| !matches!(d, Drag::Scrub)).map(|_| Box::new(app.project.clone()));
    app.finish_track_drag(ui, &headers, true);
    match app.arrangement.track_action.take() {
        Some((TrackAction::Duplicate, _, true)) => app.duplicate_selected_tracks(),
        Some((TrackAction::Duplicate, t, false)) => app.duplicate_track(t),
        Some((TrackAction::Delete, _, true)) => app.delete_selected_tracks(),
        Some((TrackAction::Delete, t, false)) => app.delete_track(t),
        Some((TrackAction::Group, _, _)) => app.group_selected(),
        Some((TrackAction::Ungroup, t, _)) => app.ungroup(t),
        Some((TrackAction::LeaveGroup, t, _)) => app.leave_group(t),
        Some((TrackAction::Fold, t, _)) => app.toggle_fold(t),
        Some((TrackAction::Rumble, t, _)) => app.make_rumble(t),
        Some((TrackAction::SoundApart(voice), t, _)) => app.sound_apart(t, voice),
        None => {}
    }
    app.update_clip_gesture(ui, &clip_rects, &lane_rects, &[]);
}

#[allow(clippy::too_many_arguments)]
fn paint_clip(
    painter: &egui::Painter,
    rect: Rect,
    placed: &ArrangementClip,
    sample: Option<&Sample>,
    selected: bool,
    ppb: f32,
    tempo: f64,
) {
    // Switched off: grey.
    let color = if placed.clip.muted() { Color32::from_gray(90) } else { theme::rgb(placed.clip.color()) };
    painter.rect_filled(rect, 2.0, theme::shade(color, 0.45));
    let title = Rect::from_min_size(rect.min, vec2(rect.width(), 14.0));
    painter.rect_filled(title, 2.0, color);
    painter.with_clip_rect(title.intersect(painter.clip_rect())).text(
        title.left_center() + vec2(4.0, 0.0),
        Align2::LEFT_CENTER,
        placed.clip.name(),
        theme::small(),
        theme::text_on(color),
    );
    let body = Rect::from_min_max(pos2(rect.left(), title.bottom() + 3.0), pos2(rect.right(), rect.bottom() - 3.0));
    let content_length = placed.clip.length();
    let laps = if content_length > 0.0 { (placed.length / content_length).ceil() as usize } else { 0 };
    let clipped = painter.with_clip_rect(rect.intersect(painter.clip_rect()));

    match &placed.clip {
        Clip::Midi(clip) if !clip.notes.is_empty() => {
            // Note preview, repeating the clip content across the placed length.
            let (lo, hi) = clip
                .notes
                .iter()
                .fold((u8::MAX, u8::MIN), |(lo, hi), n| (lo.min(n.pitch), hi.max(n.pitch)));
            let span = (hi - lo).max(1) as f32;
            for lap in 0..laps {
                for n in &clip.notes {
                    let start = lap as f64 * clip.length + n.start;
                    if start >= placed.length {
                        continue;
                    }
                    let x0 = rect.left() + start as f32 * ppb;
                    let x1 = x0 + (n.length as f32 * ppb).max(1.5);
                    let y = body.bottom() - body.height() * (n.pitch - lo) as f32 / span;
                    clipped.line_segment([pos2(x0, y), pos2(x1.min(rect.right()), y)], Stroke::new(2.0, theme::shade(color, 1.15)));
                }
            }
        }
        Clip::Audio(audio) => {
            let Some(sample) = sample.filter(|s| s.frames() > 0) else {
                clipped.text(body.center(), Align2::CENTER_CENTER, "Muestra no encontrada", theme::small(), TEXT);
                return;
            };
            paint_wave(&clipped, rect, body, audio, sample, ppb, tempo, theme::shade(color, 1.25));
            paint_fades(&clipped, rect, body, audio.fades.fitted(placed.length), selected, ppb);
            // Warp markers, where they fall in each lap of the clip.
            if audio.warp != engine::model::WarpMode::Off {
                let first = audio.file_beat(audio.offset);
                for lap in 0..laps {
                    for m in &audio.warp_markers {
                        let local = m.beat - first;
                        let at = lap as f64 * content_length + local;
                        if local < 0.0 || local >= audio.length || at >= placed.length {
                            continue;
                        }
                        let x = rect.left() + at as f32 * ppb;
                        clipped.line_segment([pos2(x, body.top()), pos2(x, body.bottom())], Stroke::new(1.0, crate::sample_editor::MARKER.gamma_multiply(0.6)));
                        clipped.add(egui::Shape::convex_polygon(
                            vec![pos2(x - 3.0, body.bottom()), pos2(x + 3.0, body.bottom()), pos2(x, body.bottom() - 5.0)],
                            crate::sample_editor::MARKER,
                            Stroke::NONE,
                        ));
                    }
                }
            }
        }
        Clip::Midi(_) => {}
    }
    for lap in 1..laps {
        let x = rect.left() + (lap as f64 * content_length) as f32 * ppb;
        clipped.line_segment([pos2(x, body.top()), pos2(x, body.bottom())], Stroke::new(1.0, theme::shade(color, 0.7)));
    }
    if selected {
        widgets::outline(painter, rect, theme::SELECT);
    }
}

/// An audio clip's fades over its waveform (the curve its volume follows, the
/// silence outside it shaded), and, selected, the handles that set them.
fn paint_fades(clipped: &egui::Painter, rect: Rect, body: Rect, fades: engine::model::Fades, selected: bool, ppb: f32) {
    let shade = Color32::from_black_alpha(110);
    let line = Stroke::new(1.2, Color32::from_white_alpha(200));
    for (length, out) in [(fades.fade_in, false), (fades.fade_out, true)] {
        let width = length as f32 * ppb;
        if width < 1.0 {
            continue;
        }
        let steps = (width / 3.0).clamp(4.0, 48.0) as usize;
        let curve: Vec<egui::Pos2> = (0..=steps)
            .map(|i| {
                let x = i as f32 / steps as f32;
                let gain = (x * std::f32::consts::FRAC_PI_2).sin();
                let at = if out { rect.right() - width * x } else { rect.left() + width * x };
                pos2(at, body.bottom() - gain * body.height())
            })
            .collect();
        // Above the curve: what the fade takes away.
        for pair in curve.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            clipped.add(egui::Shape::convex_polygon(vec![pos2(a.x, body.top()), pos2(b.x, body.top()), b, a], shade, Stroke::NONE));
        }
        clipped.add(egui::Shape::line(curve, line));
    }
    if selected {
        for handle in fade_handles(rect, fades, ppb) {
            clipped.rect_filled(handle, 1.0, Color32::WHITE);
            clipped.rect_stroke(handle, 1.0, Stroke::new(1.0, Color32::from_black_alpha(160)), egui::StrokeKind::Inside);
        }
    }
}

/// An audio clip's waveform in `body`, one column per pixel, mapping beats to file
/// time like the engine does (`rect` is where the clip starts).
#[allow(clippy::too_many_arguments)]
fn paint_wave(clipped: &egui::Painter, rect: Rect, body: Rect, audio: &engine::model::AudioClip, sample: &Sample, ppb: f32, tempo: f64, wave: Color32) {
    let mid = body.center().y;
    let half = body.height() * 0.5;
    let x_end = rect.right().min(clipped.clip_rect().right());
    let mut x = rect.left().max(clipped.clip_rect().left());
    while x < x_end {
        let beat = ((x - rect.left()) / ppb) as f64;
        let local = beat.rem_euclid(audio.length.max(0.0001));
        let next_local = local + (1.0 / ppb) as f64;
        // Before the file's start (a clip starting on the bar line before its first downbeat): silence.
        if audio.source_seconds(local, tempo) < 0.0 {
            x += 1.0;
            continue;
        }
        let to_block = |b: f64| (audio.source_seconds(b, tempo) * sample.sample_rate as f64) as usize / PEAK_BLOCK;
        let (a, b) = (to_block(local), to_block(next_local).max(to_block(local) + 1));
        let (lo, hi) = sample.peaks.get(a..b.min(sample.peaks.len())).unwrap_or(&[]).iter().fold((0f32, 0f32), |(lo, hi), &(l, h)| (lo.min(l), hi.max(h)));
        if hi > lo {
            clipped.line_segment([pos2(x, mid - hi * half), pos2(x, mid - lo * half)], Stroke::new(1.0, wave));
        }
        x += 1.0;
    }
}

/// A track's header: clic to select (⌘ and ⇧ for several), drag to move. Returns where it is.
fn track_header(app: &mut PulseApp, ui: &mut egui::Ui, t: usize, shown: Option<Param>, takes: usize) -> Rect {
    let (rect, response) = ui.allocate_exact_size(vec2(HEADER_W, row_height(shown.is_some(), takes)), Sense::click_and_drag());
    let group = app.project.tracks[t].is_group();
    let hint = if group {
        "Grupo: sus pistas suenan por su fader y sus efectos · clic en ▾: plegar o desplegar · arrastrar: mover con sus pistas"
    } else {
        "Clic: seleccionar · ⌘ clic: añadir o quitar · ⇧ clic: rango · arrastrar: mover"
    };
    let hint = match app.follows_hint(t) {
        Some(follows) => format!("{follows} · {hint}"),
        None => hint.to_string(),
    };
    let response = response.on_hover_text(crate::theme::keys(&hint));
    // Inside groups: indented, a bar of each group's colour.
    let indent = app.project.tracks[t].depth as f32 * 4.0;
    let arrow = Rect::from_min_size(rect.min + vec2(indent + 8.0, 4.0), vec2(14.0, 16.0));
    if group && response.clicked() && response.interact_pointer_pos().is_some_and(|p| arrow.contains(p)) {
        app.arrangement.track_action = Some((TrackAction::Fold, t, false));
    }
    app.track_header_input(t, &response);
    track_menu(app, &response, t, shown.is_some());
    let selected = app.is_track_selected(t);
    let track = &app.project.tracks[t];
    let color = theme::rgb(track.color);
    let painter = ui.painter();
    let body = Rect::from_min_size(rect.min, vec2(HEADER_W - 4.0, LANE_H));
    painter.rect_filled(body, 2.0, if selected { theme::CONTROL_HOVER } else { theme::SURFACE });
    crate::session::group_marks(app, painter, t, body);
    painter.rect_filled(Rect::from_min_size(body.min + vec2(indent, 0.0), vec2(5.0, LANE_H)), 2.0, color);
    let name = if group { format!("{} {}", if track.folded { "▸" } else { "▾" }, track.name) } else { track.name.clone() };
    painter.text(body.left_top() + vec2(10.0 + indent, 6.0), Align2::LEFT_TOP, name, theme::body(), TEXT);
    if track.frozen.is_some() {
        crate::freeze::mark(painter, body);
    }
    let (active, solo) = (track.mixer.active, track.solo);

    let buttons = Rect::from_min_size(body.left_bottom() + vec2(10.0, -22.0), vec2(150.0, 18.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(buttons).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.spacing_mut().item_spacing.x = 2.0;
    if widgets::toggle(&mut child, active, &(t + 1).to_string(), ACCENT, vec2(24.0, 16.0)).clicked() {
        app.project.tracks[t].mixer.active = !active;
        app.audio.send(Command::SetMute { target: Target::Track(t), mute: active });
    }
    if widgets::toggle(&mut child, solo, "S", SOLO, vec2(24.0, 16.0)).clicked() {
        app.project.tracks[t].solo = !solo;
        app.audio.send(Command::SetSolo { track: t, solo: !solo });
    }
    let armed = app.project.tracks[t].armed;
    if !group && widgets::toggle(&mut child, armed, "●", theme::METER_RED, vec2(24.0, 16.0))
        .on_hover_text("Armar para grabar")
        .clicked()
    {
        app.toggle_arm(t);
    }
    if app.project.tracks[t].is_audio() {
        // One button that steps through the modes (the header has no room for three).
        let mode = app.project.tracks[t].monitor;
        let hint = format!("{}\nClic: cambiar (In · Auto · Off)", mode.hint());
        if widgets::toggle(&mut child, mode != crate::project::Monitor::Off, mode.label(), theme::MONITOR, vec2(34.0, 16.0)).on_hover_text(hint).clicked() {
            let all = crate::project::Monitor::ALL;
            let next = all[(all.iter().position(|&m| m == mode).unwrap_or(0) + 1) % all.len()];
            app.set_monitor(t, next);
        }
    }
    let target = Target::Track(t);
    if widgets::toggle(&mut child, shown.is_some(), "A", theme::METER_RED, vec2(24.0, 16.0))
        .on_hover_text("Mostrar la automatización de la pista")
        .clicked()
    {
        if shown.is_some() {
            app.arrangement.automation.remove(&t);
        } else {
            // Open on a parameter that already has automation, or the volume.
            let param = app
                .automation(target)
                .iter()
                .find(|l| !l.points.is_empty())
                .map_or(Param::Volume, |l| l.param);
            app.arrangement.automation.insert(t, param);
        }
    }

    // The take lanes' names, the one playing somewhere ticked.
    for k in 0..takes {
        let lane = Rect::from_min_size(rect.left_top() + vec2(0.0, LANE_H + 1.0 + k as f32 * (TAKE_H + 1.0)), vec2(HEADER_W - 4.0, TAKE_H));
        let playing = app.project.tracks[t].arrangement.iter().any(|c| match &c.clip {
            Clip::Audio(a) => a.takes.get(k) == Some(&a.sample),
            Clip::Midi(_) => false,
        });
        let response = ui.interact(lane, ui.id().with(("take", t, k)), Sense::hover());
        response.on_hover_text("Arrastra en esta fila para que suene esta toma en ese trozo · clic: en todo el clip");
        ui.painter().rect_filled(lane, 2.0, theme::shade(theme::SURFACE, 0.8));
        let mark = if playing { "✓ " } else { "" };
        ui.painter().text(lane.left_center() + vec2(14.0, 0.0), Align2::LEFT_CENTER, format!("{mark}Toma {}", k + 1), theme::small(), if playing { TEXT } else { TEXT_DIM });
    }

    if let Some(param) = shown {
        automation_picker(app, ui, rect, t, param, takes);
    }
    rect
}

/// The channel a row of the arrangement is: a track's, or the master's (the
/// last row, after the tracks).
fn row_target(app: &PulseApp, t: usize) -> Target {
    if t < app.project.tracks.len() { Target::Track(t) } else { Target::Master }
}

/// Shows `param` in a row's automation lane.
fn set_row_param(app: &mut PulseApp, t: usize, param: Param) {
    if t < app.project.tracks.len() {
        app.arrangement.automation.insert(t, param);
    } else {
        app.arrangement.master_automation = Some(param);
    }
}

/// The master's row: its name, and its automation (the tempo among it).
fn master_header(app: &mut PulseApp, ui: &mut egui::Ui, row: usize, shown: Option<Param>) {
    let (rect, response) = ui.allocate_exact_size(vec2(HEADER_W, row_height(shown.is_some(), 0)), Sense::click());
    if response.clicked() {
        app.selected_target = Target::Master;
    }
    let body = Rect::from_min_size(rect.min, vec2(HEADER_W - 4.0, LANE_H));
    let selected = app.selected_target == Target::Master;
    ui.painter().rect_filled(body, 2.0, if selected { theme::CONTROL_HOVER } else { theme::SURFACE });
    ui.painter().rect_filled(Rect::from_min_size(body.min, vec2(5.0, LANE_H)), 2.0, theme::CONTROL_HOVER);
    ui.painter().text(body.left_top() + vec2(10.0, 6.0), Align2::LEFT_TOP, "Master", theme::body(), TEXT);
    let buttons = Rect::from_min_size(body.left_bottom() + vec2(10.0, -22.0), vec2(150.0, 18.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(buttons).layout(egui::Layout::left_to_right(egui::Align::Center)));
    if widgets::toggle(&mut child, shown.is_some(), "A", theme::METER_RED, vec2(24.0, 16.0))
        .on_hover_text("Mostrar la automatización del Master: tempo, volumen y efectos")
        .clicked()
    {
        app.arrangement.master_automation = match shown {
            Some(_) => None,
            // Open on what already has automation, or the tempo.
            None => Some(app.automation(Target::Master).iter().find(|l| !l.points.is_empty()).map_or(Param::Tempo, |l| l.param)),
        };
    }
    if let Some(param) = shown {
        automation_picker(app, ui, rect, row, param, 0);
    }
}

/// The parameter list of a row's automation lane, and «Borrar automatización».
fn automation_picker(app: &mut PulseApp, ui: &mut egui::Ui, rect: Rect, t: usize, param: Param, takes: usize) {
    let target = row_target(app, t);
    let area = Rect::from_min_size(rect.left_top() + vec2(10.0, LANE_H + 1.0 + takes_height(takes) + 5.0), vec2(HEADER_W - 18.0, AUTO_H - 8.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)));
    let mut chosen = param;
    egui::ComboBox::from_id_salt(("automation", t))
        .selected_text(app.param_name(target, param))
        .width(HEADER_W - 22.0)
        .height(420.0)
        .show_ui(&mut child, |ui| {
            let candidates = app.automatable(target);
            // Plugins can have thousands of parameters: a search box and the matches.
            let long = candidates.len() > 60;
            let mut filter = app.arrangement.automation_filter.clone();
            if long {
                ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Buscar parámetro…").desired_width(HEADER_W - 40.0)).request_focus();
            }
            let wanted = filter.to_lowercase();
            let mut shown = 0;
            for candidate in candidates {
                let automated = app.automation_state(target, candidate).is_some();
                let mut label = app.param_name(target, candidate);
                // Automated ones always show; the rest when they match.
                if long && !automated && !label.to_lowercase().contains(&wanted) {
                    continue;
                }
                if long && shown >= 300 {
                    ui.label(egui::RichText::new("…escribe para ver más").color(TEXT_DIM).size(10.5));
                    break;
                }
                shown += 1;
                if automated {
                    label.push_str("  ●");
                }
                ui.selectable_value(&mut chosen, candidate, label);
            }
            app.arrangement.automation_filter = filter;
        });
    if chosen != param {
        set_row_param(app, t, chosen);
    }
    if app.automation_state(target, param).is_some() && child.small_button("Borrar automatización").clicked() {
        app.automation_mut(target).retain(|l| l.param != param);
        app.overridden.remove(&(target, param));
        app.sync_automation(target);
    }
}

/// Maps a parameter value to a y position inside an automation lane.
fn value_y(rect: Rect, normalized: f32) -> f32 {
    rect.bottom() - 4.0 - normalized * (rect.height() - 8.0)
}

fn paint_automation(app: &PulseApp, painter: &egui::Painter, rect: Rect, t: usize, param: Param, x_at: &dyn Fn(f64) -> f32) {
    painter.rect_filled(rect, 0.0, theme::shade(theme::SLOT, 0.8));
    let target = row_target(app, t);
    let Some(info) = app.param_info(target, param) else {
        painter.text(rect.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, "Parámetro no disponible", theme::small(), TEXT_DIM);
        return;
    };
    let overridden = app.overridden.contains(&(target, param));
    let color = if overridden { Color32::from_gray(130) } else { theme::METER_RED };
    let lane = app.automation(target).iter().find(|l| l.param == param && !l.points.is_empty());
    match lane {
        None => {
            let y = value_y(rect, info.normalize(app.param_value(target, param)));
            painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, Color32::from_gray(100)));
            painter.text(rect.left_top() + vec2(8.0, 4.0), Align2::LEFT_TOP, "Clic para añadir puntos", theme::small(), TEXT_DIM);
        }
        Some(lane) => {
            let y = |v: f32| value_y(rect, info.normalize(v));
            let line = crate::curves::line(&lane.points, x_at, &y, rect.left(), rect.right());
            painter.add(egui::Shape::line(line, Stroke::new(1.5, color)));
            for p in &lane.points {
                painter.circle_filled(pos2(x_at(p.beat), y(p.value)), 3.5, color);
            }
            if overridden {
                painter.text(
                    rect.left_top() + vec2(8.0, 4.0),
                    Align2::LEFT_TOP,
                    "Anulada a mano · ► Arrangement la reactiva",
                    theme::small(),
                    TEXT_DIM,
                );
            }
        }
    }
}

/// Lays the lanes out again from `snapshot` with the `moved` clips shifted by
/// `beats` and `tracks` (or copied there, the originals staying), and returns the
/// moved clips' new places, the grabbed one first.
fn place_clips(app: &mut PulseApp, snapshot: &[Vec<ArrangementClip>], moved: &[(usize, usize)], primary: usize, beats: f64, tracks: isize, copy: bool) -> Vec<ClipRef> {
    let mut lanes: Vec<Vec<ArrangementClip>> = snapshot
        .iter()
        .enumerate()
        .map(|(t, lane)| lane.iter().enumerate().filter(|&(i, _)| copy || !moved.contains(&(t, i))).map(|(_, c)| c.clone()).collect())
        .collect();
    let mut placed = Vec::new();
    for &(t, i) in moved {
        let to = (t as isize + tracks) as usize;
        let mut clip = snapshot[t][i].clone();
        clip.start = (clip.start + beats).max(0.0);
        lanes[to].push(clip);
        placed.push(ClipRef::Arrangement { track: to, index: lanes[to].len() - 1 });
    }
    for (t, lane) in lanes.into_iter().enumerate() {
        if app.project.tracks[t].arrangement != lane {
            app.project.tracks[t].arrangement = lane;
            app.sync_arrangement(t);
        }
    }
    placed.swap(0, primary);
    placed
}

/// A track header's menu: name, colour, automation lane, duplicate and delete (the
/// selected tracks, if it is one of them).
fn track_menu(app: &mut PulseApp, response: &egui::Response, t: usize, automation_shown: bool) {
    crate::widgets::edit_menu(response, |ui| {
        let several = app.is_track_selected(t) && app.track_selection().len() > 1;
        let count = app.track_selection().len();
        let track = &mut app.project.tracks[t];
        ui.label("Nombre");
        ui.text_edit_singleline(&mut track.name);
        ui.label("Color");
        crate::session::color_picker(ui, &mut track.color);
        ui.separator();
        if ui.button(if automation_shown { "Ocultar automatización" } else { "Mostrar automatización" }).clicked() {
            if automation_shown {
                app.arrangement.automation.remove(&t);
            } else {
                let param = app.automation(Target::Track(t)).iter().find(|l| !l.points.is_empty()).map_or(Param::Volume, |l| l.param);
                app.arrangement.automation.insert(t, param);
            }
            ui.close();
        }
        if crate::comping::take_count(app, t) > 1 {
            let shown = app.arrangement.takes.contains(&t);
            if ui.button(if shown { "Ocultar tomas" } else { "Mostrar tomas" }).on_hover_text("Una fila por toma de las grabaciones en bucle, para quedarte con lo mejor de cada una").clicked() {
                app.toggle_take_lanes(t);
                ui.close();
            }
        }
        let label = |verb: &str| if several { format!("{verb} {count} pistas") } else { format!("{verb} pista") };
        // After the view is drawn: the rest of it still reads this track.
        if ui.button(label("Duplicar")).clicked() {
            app.arrangement.track_action = Some((TrackAction::Duplicate, t, several));
            ui.close();
        }
        if ui.button(label("Eliminar")).clicked() {
            app.arrangement.track_action = Some((TrackAction::Delete, t, several));
            ui.close();
        }
        let (frozen, freezing) = (app.is_frozen(t), app.freeze_job.is_some());
        let group = app.project.tracks[t].is_group();
        let in_group = crate::groups::parent(&app.project.tracks, t).is_some();
        let mut chosen = None;
        crate::session::group_items(ui, group, in_group, count, |action| chosen = Some(action), t);
        if !group {
            crate::session::freeze_items(ui, frozen, freezing, |action| chosen = Some(action), t);
            crate::session::sound_items(app, ui, |action| chosen = Some(action), t);
        }
        match chosen {
            Some(crate::session::Action::Freeze(t)) => app.freeze(t),
            Some(crate::session::Action::Unfreeze(t)) => app.unfreeze(t),
            Some(crate::session::Action::Flatten(t)) => app.flatten(t),
            Some(crate::session::Action::Rumble(t)) => app.arrangement.track_action = Some((TrackAction::Rumble, t, false)),
            Some(crate::session::Action::SoundApart(t, voice)) => app.arrangement.track_action = Some((TrackAction::SoundApart(voice), t, false)),
            // After the view is drawn, as they change the tracks.
            Some(crate::session::Action::GroupTracks) => app.arrangement.track_action = Some((TrackAction::Group, t, several)),
            Some(crate::session::Action::Ungroup(t)) => app.arrangement.track_action = Some((TrackAction::Ungroup, t, false)),
            Some(crate::session::Action::LeaveGroup(t)) => app.arrangement.track_action = Some((TrackAction::LeaveGroup, t, false)),
            _ => {}
        }
    });
}

/// The markers: a flag and its name in their strip, a faint line down the lanes.
fn paint_markers(app: &PulseApp, painter: &egui::Painter, rect: Rect, x_at: &dyn Fn(f64) -> f32) {
    let top = rect.top() + LOOP_H;
    for marker in &app.project.markers {
        let x = x_at(marker.beat);
        painter.line_segment([pos2(x, top), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 168, 36, 50)));
        let flag = egui::Shape::convex_polygon(vec![pos2(x, top + 1.0), pos2(x + 7.0, top + 1.0), pos2(x + 7.0, top + MARKER_H - 4.0), pos2(x, top + MARKER_H - 1.0)], ACCENT, Stroke::NONE);
        painter.add(flag);
        painter.text(pos2(x + 10.0, top + MARKER_H / 2.0), Align2::LEFT_CENTER, &marker.name, theme::small(), TEXT);
    }
}

/// The marker under `x`, if any (its flag and name).
fn marker_at(app: &PulseApp, x: f32, origin_x: f32) -> Option<usize> {
    let ppb = app.arrangement.pixels_per_beat;
    app.project.markers.iter().enumerate().rev().find_map(|(i, m)| {
        let mx = origin_x + m.beat as f32 * ppb;
        let width = 12.0 + m.name.chars().count() as f32 * 6.0;
        (x >= mx - 3.0 && x <= mx + width).then_some(i)
    })
}

/// Clicks and drags in the markers' strip: a click on a marker jumps there,
/// dragging moves it, a double click on empty space adds one, a right click
/// opens its menu. Returns whether it took the pointer.
#[allow(clippy::too_many_arguments)]
fn handle_markers(app: &mut PulseApp, ui: &egui::Ui, response: &egui::Response, rect: Rect, pos: egui::Pos2, beat: f64, unit: f64, origin_x: f32) -> bool {
    let strip = pos.y >= rect.top() + LOOP_H && pos.y < rect.top() + LOOP_H + MARKER_H;
    if let Some(Drag::Marker { index, moved }) = app.arrangement.drag {
        if response.dragged() {
            if let Some(marker) = app.project.markers.get_mut(index) {
                let to = snap(beat, unit);
                if (to - marker.beat).abs() > 1e-9 {
                    marker.beat = to;
                    app.arrangement.drag = Some(Drag::Marker { index, moved: true });
                }
            }
        }
        if ui.input(|i| i.pointer.primary_released()) {
            if !moved {
                if let Some(marker) = app.project.markers.get(index) {
                    app.audio.send(Command::SetPosition(marker.beat));
                }
            }
            app.project.markers.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            app.arrangement.drag = None;
        }
        return true;
    }
    if !strip {
        return false;
    }
    let under = marker_at(app, pos.x, origin_x);
    if response.secondary_clicked() {
        app.arrangement.menu = Some(match under {
            Some(i) => Menu::Marker(i),
            None => Menu::Markers(snap(beat, unit)),
        });
        return true;
    }
    if response.double_clicked() && under.is_none() {
        app.add_marker(snap(beat, unit));
        return true;
    }
    if ui.input(|i| i.pointer.primary_pressed()) && response.hovered() {
        if let Some(index) = under {
            app.arrangement.drag = Some(Drag::Marker { index, moved: false });
        }
    }
    if under.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    true
}

impl PulseApp {
    /// A new marker at `beat`, named after how many there are.
    pub fn add_marker(&mut self, beat: f64) {
        let name = format!("Marcador {}", self.project.markers.len() + 1);
        self.project.markers.push(crate::project::SongMarker { beat, name });
        self.project.markers.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    }
}

/// The right-click menu of the timeline: for a clip, an empty place on a track,
/// or an automation lane.
fn context_menu(app: &mut PulseApp, response: &egui::Response) {
    crate::widgets::edit_menu(response, |ui| {
        let Some(menu) = app.arrangement.menu else {
            ui.close();
            return;
        };
        match menu {
            Menu::Marker(i) => {
                let Some(marker) = app.project.markers.get_mut(i) else { return ui.close() };
                ui.label("Nombre");
                ui.text_edit_singleline(&mut marker.name);
                let beat = marker.beat;
                if ui.button("Ir aquí").clicked() {
                    app.audio.send(Command::SetPosition(beat));
                    ui.close();
                }
                if ui.button("Eliminar marcador").clicked() {
                    app.project.markers.remove(i);
                    ui.close();
                }
                crate::transitions::menu(app, ui, beat);
            }
            Menu::Markers(beat) => {
                if ui.button("Añadir marcador aquí").clicked() {
                    app.add_marker(beat);
                    ui.close();
                }
                crate::transitions::menu(app, ui, (beat / 4.0).round() * 4.0);
            }
            Menu::Clip(clip, beat) => clip_menu(app, ui, clip, beat),
            Menu::Lane { track, beat } => {
                let Some(t) = app.project.tracks.get(track) else { return ui.close() };
                let audio = t.is_audio();
                let bar = (beat / 4.0).floor() * 4.0;
                if app.clip_clipboard.is_some() && ui.button("Pegar aquí").clicked() {
                    app.paste_clips_at(track, beat);
                    ui.close();
                }
                if audio {
                    if ui.button("Añadir muestra aquí…").clicked() {
                        app.choose_samples(Placement::Arrangement { track, start: bar });
                        ui.close();
                    }
                }
                if !audio && ui.button("Nuevo clip MIDI aquí").clicked() {
                    let color = app.project.tracks[track].color;
                    app.project.tracks[track].arrangement.push(ArrangementClip { start: bar, length: 4.0, clip: Clip::Midi(MidiClip::new("MIDI", 4.0, color)) });
                    let index = app.project.tracks[track].arrangement.len() - 1;
                    app.sync_arrangement(track);
                    app.select_clip(ClipRef::Arrangement { track, index });
                    ui.close();
                }
                crate::transitions::menu(app, ui, (beat / 4.0).round() * 4.0);
            }
            Menu::Automation { track, param } => {
                let target = row_target(app, track);
                ui.label(egui::RichText::new(app.param_name(target, param)).color(TEXT_DIM).size(10.5));
                if app.automation_state(target, param).is_some() && ui.button("Borrar automatización").clicked() {
                    app.automation_mut(target).retain(|l| l.param != param);
                    app.overridden.remove(&(target, param));
                    app.sync_automation(target);
                    ui.close();
                }
                if ui.button("Ocultar carril").clicked() {
                    if track < app.project.tracks.len() {
                        app.arrangement.automation.remove(&track);
                    } else {
                        app.arrangement.master_automation = None;
                    }
                    ui.close();
                }
            }
        }
    });
}

/// A clip's menu: what acts on the selected clips, and the clip's name and colour.
fn clip_menu(app: &mut PulseApp, ui: &mut egui::Ui, clip: ClipRef, beat: f64) {
    let ClipRef::Arrangement { track, index } = clip else { return };
    let Some(placed) = app.project.tracks.get(track).and_then(|t| t.arrangement.get(index)) else { return ui.close() };
    let audio = matches!(placed.clip, Clip::Audio(_));
    let count = app.clip_selection().len();
    let label = |verb: &str| if count > 1 { format!("{verb} {count} clips") } else { verb.to_string() };
    // The takes of a loop recording: the one that plays is ticked.
    if let (Clip::Audio(a), 1) = (&placed.clip, count) {
        if a.takes.len() > 1 {
            ui.label(egui::RichText::new("Tomas").color(theme::TEXT_DIM).size(10.5));
            let playing = a.takes.iter().position(|t| *t == a.sample);
            let mut chosen = None;
            for k in 0..a.takes.len() {
                let mark = if playing == Some(k) { "✓ " } else { "    " };
                if ui.button(format!("{mark}Toma {}", k + 1)).clicked() {
                    chosen = Some(k);
                }
            }
            if let Some(k) = chosen {
                app.choose_take(track, index, k);
                ui.close();
            }
            let shown = app.arrangement.takes.contains(&track);
            if ui.button(if shown { "Ocultar las filas de tomas" } else { "Mostrar las tomas en filas…" }).on_hover_text("Arrastra en una fila para que suene esa toma en ese trozo").clicked() {
                app.toggle_take_lanes(track);
                ui.close();
            }
            ui.separator();
        }
    }
    if count == 1 && !app.project.tracks[track].is_drums() {
        ui.menu_button("Acompañar", |ui| {
            for part in crate::accompany::Part::ALL {
                if ui.button(part.label()).on_hover_text(part.hint()).clicked() {
                    app.accompany(clip, part);
                    ui.close();
                }
            }
        });
    }
    if audio && count == 1 {
        ui.menu_button("Convertir a MIDI", |ui| {
            for mode in crate::to_midi::ToMidi::ALL {
                if ui.add_enabled(app.to_midi.is_none(), egui::Button::new(mode.label())).on_hover_text(mode.hint()).clicked() {
                    app.convert_to_midi(clip, mode);
                    ui.close();
                }
            }
        });
    }
    if audio && count == 1 && ui.add_enabled(app.stems.is_none(), egui::Button::new("Separar en stems…")).on_hover_text("Batería, bajo, resto y voz, cada uno en una pista nueva").clicked() {
        app.separate_stems(clip);
        ui.close();
    }
    if audio && count == 1 {
        if ui.button("Detectar tempo").on_hover_text("Busca el tempo y los compases del audio para que siga el del proyecto").clicked() {
            app.detect_tempo(vec![clip], true);
            ui.close();
        }
        if ui.button("Usar el tempo de este clip en el proyecto").clicked() {
            app.use_clip_tempo(clip);
            ui.close();
        }
    }
    let muted = app.clip(clip).is_some_and(|c| c.muted());
    if ui.button(label(if muted { "Activar" } else { "Desactivar" }) + "   0").on_hover_text("Desactivado, el clip se queda donde está pero no suena").clicked() {
        app.toggle_clips_muted();
        ui.close();
    }
    let all_audio = app.clip_selection().iter().all(|&c| matches!(app.clip(c), Some(Clip::Audio(_))));
    if audio && all_audio && ui.button(label("Consolidar")).on_hover_text("Lo que suena el clip, a un archivo de audio nuevo solo con su trozo (con su warp, transposición y ganancia)").clicked() {
        app.consolidate(app.clip_selection());
        ui.close();
    }
    if audio && all_audio && ui.button(label("Invertir")).on_hover_text("Lo que suena el clip, del final al principio, en un archivo nuevo (otra vez lo devuelve)").clicked() {
        app.reverse_clips(app.clip_selection());
        ui.close();
    }
    if count == 1 && ui.button(crate::theme::keys("Dividir aquí   ⌘E")).on_hover_text(crate::theme::keys("Corta el clip en dos donde hiciste clic derecho (⌘E corta donde hiciste clic en el clip)")).clicked() {
        app.split_clip_at(track, index, beat);
        ui.close();
    }
    ui.menu_button(label("Trocear"), |ui| {
        for (name, every) in [("Cada tiempo", 1.0), ("Cada 2 tiempos", 2.0), ("Cada compás", 4.0), ("Cada 2 compases", 8.0), ("Cada 4 compases", 16.0)] {
            if ui.button(name).on_hover_text("Corta los clips en trozos iguales, en las líneas de la rejilla").clicked() {
                app.slice_selected(every);
                ui.close();
            }
        }
    });
    if count > 1 && ui.button(crate::theme::keys(&format!("Unir {count} clips   ⌘J"))).on_hover_text("Clips de una pista en uno solo, de principio a fin (los de audio, en un archivo nuevo)").clicked() {
        app.join_selected();
        ui.close();
    }
    if ui.button(label("Copiar")).clicked() {
        app.copy_clips();
        ui.close();
    }
    if ui.button(label("Cortar")).clicked() {
        app.copy_clips();
        app.delete_selected_clip();
        ui.close();
    }
    if ui.button(crate::theme::keys(&format!("{}   ⌘D", label("Duplicar")))).clicked() {
        app.duplicate_selected_clip();
        ui.close();
    }
    if ui.button(format!("{}   ⌫", label("Eliminar"))).clicked() {
        app.delete_selected_clip();
        ui.close();
    }
    if count > 1 {
        return;
    }
    // What was chosen above may have taken the clip away (cutting, deleting,
    // joining): its name and colour only while it is there.
    let Some(placed) = app.project.tracks.get_mut(track).and_then(|t| t.arrangement.get_mut(index)) else { return };
    ui.separator();
    let clip = &mut placed.clip;
    let before = clip.clone();
    ui.label("Nombre");
    ui.text_edit_singleline(clip.name_mut());
    ui.label("Color");
    crate::session::color_picker(ui, clip.color_mut());
    if *clip != before {
        app.sync_arrangement(track);
    }
}

/// Hit-tests a pointer position against the clips of a lane.
fn clip_at(app: &PulseApp, track: usize, beat: f64, x: f32, ppb: f32, origin_x: f32) -> Option<(usize, bool)> {
    app.project.tracks[track]
        .arrangement
        .iter()
        .enumerate()
        .rev()
        .find(|(_, c)| beat >= c.start && beat < c.end())
        .map(|(index, c)| {
            let right = origin_x + c.end() as f32 * ppb;
            (index, right - x <= EDGE_GRAB)
        })
}

#[allow(clippy::too_many_arguments)]
fn handle_pointer(
    app: &mut PulseApp,
    ui: &egui::Ui,
    response: &egui::Response,
    rect: Rect,
    lanes_top: f32,
    unit: f64,
    origin_x: f32,
    layout: &Layout,
) {
    let ppb = app.arrangement.pixels_per_beat;
    let Some(pos) = response.interact_pointer_pos().or_else(|| response.hover_pos()) else {
        return;
    };
    let beat = ((pos.x - origin_x) / ppb).max(0.0) as f64;
    let hit = if pos.y >= lanes_top { layout.hit(pos.y - lanes_top) } else { None };
    let lane = hit.filter(|&(_, automation)| !automation).map(|(t, _)| t);
    let automation = hit.filter(|&(_, automation)| automation).and_then(|(t, _)| {
        let (top, param, takes) = layout.rows[t];
        let param = param?;
        let top = lanes_top + top + LANE_H + 1.0 + takes_height(takes);
        Some((t, param, Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), AUTO_H))))
    });
    // The markers' strip: its own clicks and drags.
    if handle_markers(app, ui, response, rect, pos, beat, unit, origin_x) {
        return;
    }
    // A right-click picks what the menu is about (and selects a clip not selected).
    if response.secondary_clicked() {
        app.arrangement.menu = match (lane, automation) {
            (Some(t), _) => Some(match clip_at(app, t, beat, pos.x, ppb, origin_x) {
                Some((index, _)) => {
                    let clip = ClipRef::Arrangement { track: t, index };
                    if !app.is_clip_selected(clip) {
                        app.select_clip(clip);
                    }
                    Menu::Clip(clip, snap(beat, unit))
                }
                None => {
                    app.selected_target = Target::Track(t);
                    Menu::Lane { track: t, beat: snap(beat, unit) }
                }
            }),
            (None, Some((t, param, _))) => Some(Menu::Automation { track: t, param }),
            _ => None,
        };
    }
    let take_lane = if pos.y >= lanes_top { layout.take_hit(pos.y - lanes_top) } else { None };
    if handle_takes(app, ui, response, beat, take_lane) {
        return;
    }
    if handle_automation(app, ui, response, pos, beat, origin_x, automation) {
        return;
    }

    // Where the clips of the lane under the pointer start, for their fade handles.
    let fade = lane.and_then(|t| fade_at(app, t, pos, lanes_top + layout.rows[t].0 + 1.0, ppb, origin_x).map(|(index, out)| (t, index, out)));
    // Cursor feedback for resizing (and fading).
    if app.arrangement.drag.is_none() {
        if let Some(t) = lane {
            if fade.is_some() || matches!(clip_at(app, t, beat, pos.x, ppb, origin_x), Some((_, true))) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
    }
    if let Some((t, index, _)) = fade.filter(|_| app.arrangement.drag.is_none()) {
        let hint = "Arrastrar: fundido (entrada o salida) · doble clic: quitarlo";
        response.clone().on_hover_text_at_pointer(hint);
        if response.double_clicked() {
            if let Some(Clip::Audio(audio)) = app.project.tracks[t].arrangement.get_mut(index).map(|p| &mut p.clip) {
                let out = fade.is_some_and(|f| f.2);
                if out { audio.fades.fade_out = 0.0 } else { audio.fades.fade_in = 0.0 }
                app.sync_arrangement(t);
            }
            return;
        }
    }

    let pressed = ui.input(|i| i.pointer.primary_pressed()) && response.hovered();
    if pressed {
        app.arrangement.drag = if pos.y < rect.top() + LOOP_H {
            Some(Drag::Loop { anchor: snap(beat, unit) })
        } else if pos.y < lanes_top {
            app.audio.send(Command::SetPosition(snap(beat, unit)));
            // ⌘E cuts at the playhead again.
            app.arrangement.cut = None;
            Some(Drag::Scrub)
        } else if let Some((track, index, out)) = fade {
            Some(Drag::Fade { track, index, out })
        } else if let Some(t) = lane {
            let modifiers = ui.input(|i| i.modifiers);
            match clip_at(app, t, beat, pos.x, ppb, origin_x) {
                Some((index, _)) if modifiers.command || modifiers.shift => {
                    app.click_clip(ClipRef::Arrangement { track: t, index }, modifiers);
                    None
                }
                Some((index, true)) => {
                    app.select_clip(ClipRef::Arrangement { track: t, index });
                    Some(Drag::Resize { track: t, index })
                }
                Some((index, false)) => {
                    let grabbed = ClipRef::Arrangement { track: t, index };
                    if !app.is_clip_selected(grabbed) {
                        app.select_clip(grabbed);
                    } else {
                        // Keep the selection; the grabbed clip goes to the editor.
                        app.selected_clip = Some(grabbed);
                    }
                    let moved: Vec<(usize, usize)> = app
                        .clip_selection()
                        .iter()
                        .filter_map(|c| match *c {
                            ClipRef::Arrangement { track, index } => Some((track, index)),
                            ClipRef::Session { .. } => None,
                        })
                        .collect();
                    let primary = moved.iter().position(|&m| m == (t, index)).unwrap_or(0);
                    let snapshot = app.project.tracks.iter().map(|t| t.arrangement.clone()).collect();
                    Some(Drag::Move { grab_track: t, grab: beat, moved, primary, snapshot, copy: modifiers.alt, applied: (0.0, 0) })
                }
                None => {
                    app.selected_target = Target::Track(t);
                    app.start_marquee(pos, modifiers);
                    None
                }
            }
        } else {
            // Below the tracks: a rectangle too (a clic there deselects).
            app.start_marquee(pos, ui.input(|i| i.modifiers));
            None
        };
    }

    // A plain clic on one of several selected clips leaves only that one, and
    // leaves the cut point there (on the grid; with ⌥, exactly there).
    if response.clicked() && !ui.input(|i| i.modifiers.command || i.modifiers.shift) {
        if let Some((index, _)) = lane.and_then(|t| clip_at(app, t, beat, pos.x, ppb, origin_x)) {
            let clip = ClipRef::Arrangement { track: lane.unwrap_or(0), index };
            if app.clip_selection().len() > 1 {
                app.select_clip(clip);
            }
            app.arrangement.cut = Some(if ui.input(|i| i.modifiers.alt) { beat.max(0.0) } else { snap(beat, unit) });
        }
    }

    if response.double_clicked() && automation.is_none() {
        if let Some(t) = lane {
            if clip_at(app, t, beat, pos.x, ppb, origin_x).is_none() {
                let start = (beat / 4.0).floor() * 4.0;
                if app.project.tracks[t].is_audio() {
                    app.choose_samples(Placement::Arrangement { track: t, start });
                    return;
                }
                let color = app.project.tracks[t].color;
                app.project.tracks[t].arrangement.push(ArrangementClip {
                    start,
                    length: 4.0,
                    clip: Clip::Midi(MidiClip::new("MIDI", 4.0, color)),
                });
                let index = app.project.tracks[t].arrangement.len() - 1;
                app.sync_arrangement(t);
                app.select_clip(ClipRef::Arrangement { track: t, index });
            }
        }
    }

    if response.dragged() {
        match app.arrangement.drag.clone() {
            Some(Drag::Move { grab_track, grab, moved, primary, snapshot, copy, applied }) => {
                let Some(&(pt, pi)) = moved.get(primary) else { return };
                // The grabbed clip snaps to the grid; the others keep their distance to it.
                let original = snapshot[pt][pi].start;
                let earliest = moved.iter().map(|&(t, i)| snapshot[t][i].start).fold(f64::INFINITY, f64::min);
                let beats = (snap(original + beat - grab, unit) - original).max(-earliest);
                let wanted = lane.map_or(applied.1, |l| l as isize - grab_track as isize);
                let fits = |dt: isize| {
                    moved.iter().all(|&(t, i)| {
                        let to = t as isize + dt;
                        to >= 0
                            && (to as usize) < app.project.tracks.len()
                            && matches!(snapshot[t][i].clip, Clip::Audio(_)) == app.project.tracks[to as usize].is_audio()
                    })
                };
                let tracks = if fits(wanted) { wanted } else { applied.1 };
                if (beats, tracks) != applied {
                    let selection = place_clips(app, &snapshot, &moved, primary, beats, tracks, copy);
                    app.set_clip_selection(selection);
                    app.arrangement.drag = Some(Drag::Move { grab_track, grab, moved, primary, snapshot, copy, applied: (beats, tracks) });
                }
            }
            Some(Drag::Resize { track, index }) => {
                let placed = &mut app.project.tracks[track].arrangement[index];
                let length = (snap(beat, unit) - placed.start).max(unit);
                if placed.length != length {
                    placed.length = length;
                    app.sync_arrangement(track);
                }
            }
            Some(Drag::Loop { anchor }) => {
                let here = snap(beat, unit);
                let (start, end) = (anchor.min(here), anchor.max(here));
                if end - start >= unit {
                    app.project.loop_start = start;
                    app.project.loop_end = end;
                    app.project.loop_enabled = true;
                    app.sync_loop();
                }
            }
            Some(Drag::Scrub) => {
                app.audio.send(Command::SetPosition(snap(beat, unit)));
            }
            Some(Drag::Fade { track, index, out }) => {
                let Some(placed) = app.project.tracks[track].arrangement.get_mut(index) else { return };
                let (start, end, length) = (placed.start, placed.end(), placed.length);
                let Clip::Audio(audio) = &mut placed.clip else { return };
                let mut fades = audio.fades.fitted(length);
                // Free, not on the grid; ⇧ snaps it to the grid.
                let at = if ui.input(|i| i.modifiers.shift) { snap(beat, unit) } else { beat };
                if out {
                    fades.fade_out = (end - at).clamp(0.0, length - fades.fade_in);
                } else {
                    fades.fade_in = (at - start).clamp(0.0, length - fades.fade_out);
                }
                if fades != audio.fades {
                    audio.fades = fades;
                    app.sync_arrangement(track);
                }
            }
            Some(Drag::Point { .. } | Drag::Bend { .. } | Drag::Comp { .. } | Drag::Marker { .. }) | None => {}
        }
    }

    if ui.input(|i| i.pointer.primary_released()) {
        app.arrangement.drag = None;
    }
}

/// Comping in a take lane (`lane`: the track and take under the pointer): a drag
/// has the take play over that stretch, a clic over the whole clip there.
/// Returns true if the pointer was handled.
fn handle_takes(app: &mut PulseApp, ui: &egui::Ui, response: &egui::Response, beat: f64, lane: Option<(usize, usize)>) -> bool {
    let ppb = app.arrangement.pixels_per_beat;
    let unit = point_unit(ppb);
    match app.arrangement.drag {
        Some(Drag::Comp { .. }) => {}
        Some(_) => return false,
        None => {
            let pressed = ui.input(|i| i.pointer.primary_pressed()) && response.hovered();
            if let (true, Some((track, take))) = (pressed, lane) {
                app.arrangement.drag = Some(Drag::Comp { track, take, anchor: beat, to: beat });
            }
            return lane.is_some();
        }
    }
    let Some(Drag::Comp { track, take, anchor, .. }) = app.arrangement.drag else { return false };
    app.arrangement.drag = Some(Drag::Comp { track, take, anchor, to: beat });
    if ui.input(|i| i.pointer.primary_released()) {
        app.arrangement.drag = None;
        if (beat - anchor).abs() as f32 * ppb < 4.0 {
            // A clic: the clip there, all of it.
            let piece = app.project.tracks[track].arrangement.iter().find(|c| anchor >= c.start && anchor < c.end()).map(|c| (c.start, c.end()));
            if let Some((a, b)) = piece {
                app.comp_takes(track, take, a, b);
            }
        } else {
            app.comp_takes(track, take, snap(anchor, unit), snap(beat, unit));
        }
    }
    true
}

/// Breakpoint editing in an automation lane. Returns true if the pointer was handled.
fn handle_automation(
    app: &mut PulseApp,
    ui: &egui::Ui,
    response: &egui::Response,
    pos: egui::Pos2,
    beat: f64,
    origin_x: f32,
    lane: Option<(usize, Param, Rect)>,
) -> bool {
    // Clip, loop and scrub drags keep going when they pass over an automation lane.
    if app.arrangement.drag.as_ref().is_some_and(|d| !matches!(d, Drag::Point { .. } | Drag::Bend { .. })) {
        return false;
    }
    let ppb = app.arrangement.pixels_per_beat;
    let unit = point_unit(ppb);
    let x_at = |b: f64| origin_x + b as f32 * ppb;

    // Dragging a breakpoint continues even if the pointer leaves the lane.
    if let Some(Drag::Point { track, param, index, lane: rect }) = app.arrangement.drag {
        if response.dragged() {
            let target = row_target(app, track);
            if let Some(info) = app.param_info(target, param) {
                let value = info.denormalize((rect.bottom() - 4.0 - pos.y) / (rect.height() - 8.0));
                let lane = app.lane_mut(target, param);
                let lo = if index > 0 { lane.points[index - 1].beat } else { 0.0 };
                let hi = lane.points.get(index + 1).map_or(f64::INFINITY, |p| p.beat);
                let point = &mut lane.points[index];
                point.beat = snap(beat, unit).clamp(lo, hi);
                point.value = value;
                app.sync_automation(target);
            }
        }
        if ui.input(|i| i.pointer.primary_released()) {
            app.arrangement.drag = None;
        }
        return true;
    }

    // Bending a stretch, likewise.
    if let Some(Drag::Bend { track, param, index, start, from_y }) = app.arrangement.drag {
        // ⌥ + double clic: straight again.
        if response.double_clicked() {
            let target = row_target(app, track);
            if let Some(point) = app.lane_mut(target, param).points.get_mut(index) {
                point.curve = 0.0;
            }
            app.sync_automation(target);
            app.arrangement.drag = None;
            return true;
        }
        if response.dragged() {
            let target = row_target(app, track);
            let lane = app.lane_mut(target, param);
            if let (Some(&from), Some(&to)) = (lane.points.get(index), lane.points.get(index + 1)) {
                lane.points[index].curve = crate::curves::bent(start, pos.y - from_y, &from, &to);
                app.sync_automation(target);
            }
        }
        if ui.input(|i| i.pointer.primary_released()) {
            app.arrangement.drag = None;
        }
        return true;
    }

    let Some((track, param, rect)) = lane else { return false };
    let target = row_target(app, track);
    let Some(info) = app.param_info(target, param) else { return true };
    let alt = ui.input(|i| i.modifiers.alt);
    let y_of = |v: f32| value_y(rect, info.normalize(v));
    let near = app
        .automation(target)
        .iter()
        .find(|l| l.param == param)
        .and_then(|l| {
            l.points
                .iter()
                .position(|p| (x_at(p.beat) - pos.x).abs() <= POINT_GRAB && (y_of(p.value) - pos.y).abs() <= POINT_GRAB)
        });

    // ⌥ over a stretch: it bends.
    let stretch = app.automation(target).iter().find(|l| l.param == param).and_then(|l| crate::curves::stretch_at(&l.points, pos.x, &x_at));
    if alt && near.is_none() && stretch.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if response.double_clicked() {
        if let Some(index) = near {
            app.lane_mut(target, param).points.remove(index);
            app.sync_automation(target);
        } else if let (true, Some(index)) = (alt, stretch) {
            // ⌥ + double clic: straight again.
            app.lane_mut(target, param).points[index].curve = 0.0;
            app.sync_automation(target);
        }
        return true;
    }
    if ui.input(|i| i.pointer.primary_pressed()) && response.hovered() {
        app.selected_target = target;
        if let (true, None, Some(index)) = (alt, near, stretch) {
            let start = app.lane_mut(target, param).points[index].curve;
            app.arrangement.drag = Some(Drag::Bend { track, param, index, start, from_y: pos.y });
            return true;
        }
        let index = match near {
            Some(index) => index,
            None => {
                let value = info.denormalize((rect.bottom() - 4.0 - pos.y) / (rect.height() - 8.0));
                let lane = app.lane_mut(target, param);
                let point = AutomationPoint { beat: snap(beat, unit), value, curve: 0.0 };
                let index = lane.points.partition_point(|p| p.beat <= point.beat);
                lane.points.insert(index, point);
                app.sync_automation(target);
                index
            }
        };
        app.arrangement.drag = Some(Drag::Point { track, param, index, lane: rect });
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use egui::{Modifiers, PointerButton, Pos2};

    /// Drives the arrangement view with mouse events and reads the texts it draws.
    struct Screen {
        ctx: egui::Context,
        time: f64,
    }

    impl Screen {
        fn new() -> Self {
            Self { ctx: egui::Context::default(), time: 0.0 }
        }

        fn frame(&mut self, app: &mut PulseApp, events: Vec<egui::Event>) -> Vec<(String, Rect)> {
            self.time += 0.05;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let output = self.ctx.run_ui(input, |ui| show(app, ui));
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

        fn click(&mut self, app: &mut PulseApp, at: Pos2, button: PointerButton) -> Vec<(String, Rect)> {
            let press = |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: Modifiers::NONE };
            self.frame(app, vec![egui::Event::PointerMoved(at)]);
            self.frame(app, vec![press(true)]);
            self.frame(app, vec![press(false)]);
            self.frame(app, vec![])
        }

        /// Right-clicks `at`, then clicks the menu item whose text is `item`.
        fn menu(&mut self, app: &mut PulseApp, at: Pos2, item: &str) {
            let texts = self.click(app, at, PointerButton::Secondary);
            let Some((_, rect)) = texts.iter().find(|(text, _)| text == item) else {
                panic!("no «{item}» in the menu: {:?}", texts.iter().map(|(t, _)| t).filter(|t| t.len() < 40).collect::<Vec<_>>())
            };
            self.click(app, rect.center(), PointerButton::Primary);
            self.frame(app, vec![]);
        }
    }

    /// Lane `t` at `beat`, with no automation lanes shown above it.
    fn at(app: &PulseApp, t: usize, beat: f64) -> Pos2 {
        pos2(150.0 + beat as f32 * app.arrangement.pixels_per_beat, 96.0 + t as f32 * 53.0)
    }

    fn app() -> PulseApp {
        let mut app = PulseApp::headless(Project::demo());
        app.view = crate::app::View::Arrangement;
        for track in &mut app.project.tracks {
            track.arrangement.clear();
            track.automation.clear();
        }
        let color = app.project.tracks[0].color;
        app.project.tracks[0].arrangement.push(ArrangementClip { start: 4.0, length: 4.0, clip: Clip::Midi(MidiClip::new("A", 4.0, color)) });
        app.sync_all();
        app
    }

    #[test]
    fn right_clicking_a_clip_opens_its_menu() {
        let mut app = app();
        let mut screen = Screen::new();
        let clip = at(&app, 0, 5.0);
        screen.menu(&mut app, clip, "Copiar");
        assert_eq!(app.selected_clip, Some(ClipRef::Arrangement { track: 0, index: 0 }), "right-clicking selects it");
        assert!(app.clip_clipboard.is_some());
        screen.menu(&mut app, clip, &crate::theme::keys("Duplicar   ⌘D"));
        assert_eq!(app.project.tracks[0].arrangement.len(), 2);
        screen.menu(&mut app, clip, "Eliminar   ⌫");
        let starts: Vec<f64> = app.project.tracks[0].arrangement.iter().map(|c| c.start).collect();
        assert_eq!(starts, vec![8.0], "the copy stays");
    }

    /// Cutting or deleting the only clip of a track from its menu: the menu
    /// went on to show the name of a clip no longer there, and Pulse closed.
    #[test]
    fn cutting_or_deleting_a_tracks_only_clip_from_its_menu() {
        for item in ["Cortar", "Eliminar   ⌫"] {
            let mut app = app();
            let mut screen = Screen::new();
            let (clip, empty) = (at(&app, 0, 5.0), at(&app, 1, 9.5));
            screen.menu(&mut app, clip, item);
            assert!(app.project.tracks[0].arrangement.is_empty(), "{item}");
            if item == "Cortar" {
                screen.menu(&mut app, empty, "Pegar aquí");
                assert_eq!(app.project.tracks[1].arrangement.len(), 1, "what was cut pastes");
            }
        }
    }

    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    /// Presses at `from` and drags to `from + delta` in `steps` frames, the button still held.
    fn hold_and_drag(screen: &mut Screen, app: &mut PulseApp, from: Pos2, delta: egui::Vec2, steps: usize) {
        screen.frame(app, vec![egui::Event::PointerMoved(from)]);
        screen.frame(app, vec![button(from, true)]);
        for k in 1..=steps {
            screen.frame(app, vec![egui::Event::PointerMoved(from + delta * (k as f32 / steps as f32))]);
        }
    }

    /// Moves on and lets go.
    fn drag_on_and_release(screen: &mut Screen, app: &mut PulseApp, to: Pos2) {
        screen.frame(app, vec![egui::Event::PointerMoved(to)]);
        screen.frame(app, vec![button(to, false)]);
        screen.frame(app, vec![]);
    }

    /// ⌫ while a clip's right edge is being dragged: the drag ends, no crash.
    #[test]
    fn deleting_a_clip_while_resizing_it() {
        let mut app = app();
        let mut screen = Screen::new();
        let edge = at(&app, 0, 7.8);
        hold_and_drag(&mut screen, &mut app, edge, vec2(30.0, 0.0), 3);
        assert!(app.project.tracks[0].arrangement[0].length > 4.0, "the resize is under way");
        app.delete_selected_clip();
        drag_on_and_release(&mut screen, &mut app, edge + vec2(60.0, 0.0));
        assert!(app.project.tracks[0].arrangement.is_empty());
    }

    /// ⌘Z while resizing the clip that the undone step added.
    #[test]
    fn undoing_while_resizing_a_clip() {
        let mut app = app();
        app.commit_history();
        let color = app.project.tracks[0].color;
        app.project.tracks[0].arrangement.push(ArrangementClip { start: 12.0, length: 4.0, clip: Clip::Midi(MidiClip::new("B", 4.0, color)) });
        app.sync_arrangement(0);
        app.commit_history();
        let mut screen = Screen::new();
        let edge = at(&app, 0, 15.8);
        hold_and_drag(&mut screen, &mut app, edge, vec2(30.0, 0.0), 3);
        assert!(app.project.tracks[0].arrangement[1].length > 4.0, "the resize is under way");
        app.undo();
        drag_on_and_release(&mut screen, &mut app, edge + vec2(60.0, 0.0));
        assert_eq!(app.project.tracks[0].arrangement.len(), 1, "undone, and it stays undone");
    }

    /// ⌘Z of a new track while moving a clip.
    #[test]
    fn undoing_a_new_track_while_moving_a_clip() {
        let mut app = app();
        app.commit_history();
        app.add_track();
        app.commit_history();
        let tracks = app.project.tracks.len();
        let mut screen = Screen::new();
        let grab = at(&app, 0, 5.0);
        hold_and_drag(&mut screen, &mut app, grab, vec2(28.0, 0.0), 3);
        assert!(app.project.tracks[0].arrangement[0].start > 4.0, "the move is under way");
        app.undo();
        drag_on_and_release(&mut screen, &mut app, grab + vec2(70.0, 0.0));
        assert_eq!(app.project.tracks.len(), tracks - 1);
    }

    /// ⌘Z while dragging the point a press just added to an automation lane.
    #[test]
    fn undoing_while_dragging_a_new_automation_point() {
        let mut app = app();
        app.arrangement.automation.insert(1, Param::Volume);
        app.commit_history();
        app.project.tempo = 121.0;
        app.commit_history();
        let mut screen = Screen::new();
        let from = pos2(150.0 + 20.0 * app.arrangement.pixels_per_beat, 70.0 + 53.0 + 53.0 + 40.0);
        hold_and_drag(&mut screen, &mut app, from, vec2(20.0, -10.0), 3);
        assert_eq!(app.automation(Target::Track(1)).iter().find(|l| l.param == Param::Volume).map(|l| l.points.len()), Some(1), "a point is being dragged");
        app.undo();
        drag_on_and_release(&mut screen, &mut app, from + vec2(40.0, -20.0));
        assert!(app.automation(Target::Track(1)).iter().all(|l| l.points.is_empty()), "undone, the point stays gone");
    }

    /// ⌫ while moving a clip: it stays deleted (the drag used to lay the lanes
    /// out again from what it held and bring it back).
    #[test]
    fn deleting_a_clip_while_moving_it() {
        let mut app = app();
        let mut screen = Screen::new();
        let grab = at(&app, 0, 5.0);
        hold_and_drag(&mut screen, &mut app, grab, vec2(28.0, 0.0), 3);
        app.delete_selected_clip();
        drag_on_and_release(&mut screen, &mut app, grab + vec2(70.0, 0.0));
        assert!(app.project.tracks.iter().all(|t| t.arrangement.is_empty()), "it came back: {:?}", app.project.tracks.iter().map(|t| t.arrangement.len()).collect::<Vec<_>>());
    }

    /// A drag that nothing interrupts still works as before.
    #[test]
    fn a_plain_move_still_moves() {
        let mut app = app();
        let mut screen = Screen::new();
        let (grab, bar) = (at(&app, 0, 5.0), vec2(4.0 * app.arrangement.pixels_per_beat, 0.0));
        hold_and_drag(&mut screen, &mut app, grab, bar, 4);
        drag_on_and_release(&mut screen, &mut app, grab + bar);
        assert_eq!(app.project.tracks[0].arrangement[0].start, 8.0);
    }

    /// A clic on a clip leaves the cut point there (on the grid), and ⌘E cuts
    /// there: one clip into three with two clics.
    #[test]
    fn clicking_a_clip_then_split_cuts_where_clicked() {
        let mut app = app();
        let mut screen = Screen::new();
        let lane = |app: &PulseApp| app.project.tracks[0].arrangement.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>();
        let here = at(&app, 0, 5.3);
        screen.click(&mut app, here, PointerButton::Primary);
        assert_eq!(app.arrangement.cut, Some(5.0), "on the grid");
        assert_eq!(app.audio.position(), 0.0, "the playhead stays");
        app.split_selected();
        assert_eq!(lane(&app), vec![(4.0, 1.0), (5.0, 3.0)]);
        let here = at(&app, 0, 6.6);
        screen.click(&mut app, here, PointerButton::Primary);
        app.split_selected();
        let mut parts = lane(&app);
        parts.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(parts, vec![(4.0, 1.0), (5.0, 2.0), (7.0, 1.0)]);
    }

    /// «Trocear ▸ Cada tiempo» cuts a clip at every beat.
    #[test]
    fn slicing_a_clip_from_its_menu() {
        let mut app = app();
        let mut screen = Screen::new();
        let here = at(&app, 0, 5.0);
        let texts = screen.click(&mut app, here, PointerButton::Secondary);
        let (_, rect) = texts.iter().find(|(t, _)| t == "Trocear").expect("in the clip's menu");
        let texts = screen.click(&mut app, rect.center(), PointerButton::Primary);
        let (_, rect) = texts.iter().find(|(t, _)| t == "Cada tiempo").expect("its submenu");
        screen.click(&mut app, rect.center(), PointerButton::Primary);
        let mut parts: Vec<(f64, f64)> = app.project.tracks[0].arrangement.iter().map(|c| (c.start, c.length)).collect();
        parts.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(parts, vec![(4.0, 1.0), (5.0, 1.0), (6.0, 1.0), (7.0, 1.0)]);
        assert_eq!(app.clip_selection().len(), 4, "the pieces stay selected");
    }

    #[test]
    fn right_clicking_an_empty_place_pastes_or_makes_a_clip_there() {
        let mut app = app();
        let mut screen = Screen::new();
        let (empty, clip, other) = (at(&app, 1, 9.5), at(&app, 0, 5.0), at(&app, 2, 13.0));
        screen.menu(&mut app, empty, "Nuevo clip MIDI aquí");
        let lane: Vec<(f64, f64)> = app.project.tracks[1].arrangement.iter().map(|c| (c.start, c.length)).collect();
        assert_eq!(lane, vec![(8.0, 4.0)], "at the bar");
        screen.menu(&mut app, clip, "Copiar");
        screen.menu(&mut app, other, "Pegar aquí");
        let lane: Vec<f64> = app.project.tracks[2].arrangement.iter().map(|c| c.start).collect();
        assert_eq!(lane, vec![13.0], "where it was right-clicked (on the grid)");
    }

    /// The markers' strip: a double click adds one, dragging moves it, a click
    /// jumps there.
    #[test]
    fn markers_are_added_moved_and_jumped_to() {
        let mut app = app();
        let mut screen = Screen::new();
        screen.frame(&mut app, vec![]);
        let ppb = app.arrangement.pixels_per_beat;
        // The strip is under the loop's (12 px), at the top of the timeline (x 150,
        // y about 22 under the toolbar).
        let strip = |beat: f64| pos2(150.0 + beat as f32 * ppb + 2.0, 22.5 + 12.0 + 7.0);
        screen.click(&mut app, strip(8.0), PointerButton::Primary);
        let at = strip(8.0);
        let press = |pressed| egui::Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        screen.frame(&mut app, vec![press(true)]);
        screen.frame(&mut app, vec![press(false)]);
        screen.frame(&mut app, vec![]);
        assert_eq!(app.project.markers.len(), 1, "a double click adds one");
        assert_eq!(app.project.markers[0].beat, 8.0);
        assert_eq!(app.project.markers[0].name, "Marcador 1");
        // Dragged to beat 16.
        let (from, to) = (strip(8.0), strip(16.0));
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        screen.frame(&mut app, vec![egui::Event::PointerMoved(from)]);
        screen.frame(&mut app, vec![button(from, true)]);
        for k in 1..=6 {
            screen.frame(&mut app, vec![egui::Event::PointerMoved(from + (to - from) * k as f32 / 6.0)]);
        }
        screen.frame(&mut app, vec![button(to, false)]);
        assert_eq!(app.project.markers[0].beat, 16.0);
        // The texts show its name.
        let texts = screen.frame(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t == "Marcador 1"));
    }

    /// The master's row, last, opens its automation on the tempo.
    #[test]
    fn the_master_row_shows_tempo_automation() {
        let mut app = app();
        let mut screen = Screen::new();
        let texts = screen.frame(&mut app, vec![]);
        assert!(texts.iter().any(|(t, _)| t == "Master"));
        app.arrangement.master_automation = Some(Param::Tempo);
        let texts = screen.frame(&mut app, vec![]);
        let texts = screen.frame(&mut app, vec![]).into_iter().chain(texts).collect::<Vec<_>>();
        assert!(texts.iter().any(|(t, _)| t == "Tempo"), "{:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>());
        assert!(texts.iter().any(|(t, _)| t == "Clic para añadir puntos"));
    }

    #[test]
    fn track_headers_and_automation_lanes_have_menus_too() {
        let mut app = app();
        let mut screen = Screen::new();
        let tracks = app.project.tracks.len();
        let header = |t: usize| pos2(60.0, 70.0 + t as f32 * 53.0);
        screen.menu(&mut app, header(1), "Mostrar automatización");
        assert_eq!(app.arrangement.automation.get(&1), Some(&Param::Volume));
        // Points on the lane, then its menu (the lane is under track 1's clips).
        app.lane_mut(Target::Track(1), Param::Volume).points.push(AutomationPoint { beat: 0.0, value: 0.5, curve: 0.0 });
        let lane = pos2(150.0 + 20.0 * app.arrangement.pixels_per_beat, 55.0 + 53.0 + 53.0 + 30.0);
        screen.menu(&mut app, lane, "Borrar automatización");
        assert!(app.automation(Target::Track(1)).iter().all(|l| l.param != Param::Volume || l.points.is_empty()));
        screen.menu(&mut app, lane, "Ocultar carril");
        assert!(app.arrangement.automation.is_empty());
        // Deleting a track from its own header, drawn as it goes.
        screen.menu(&mut app, header(1), "Eliminar pista");
        assert_eq!(app.project.tracks.len(), tracks - 1);
    }

    /// ⌥ + drag on a stretch of an automation lane bends it; ⌥ + double clic
    /// straightens it.
    #[test]
    fn alt_dragging_a_stretch_bends_it() {
        let mut app = app();
        let target = Target::Track(1);
        app.arrangement.automation.insert(1, Param::Volume);
        let info = app.param_info(target, Param::Volume).unwrap();
        let points = &mut app.lane_mut(target, Param::Volume).points;
        points.push(AutomationPoint { beat: 0.0, value: info.denormalize(0.1), curve: 0.0 });
        points.push(AutomationPoint { beat: 40.0, value: info.denormalize(0.9), curve: 0.0 });
        let screen = Screen::new();
        let mut time = 0.0;
        let mut frame = |app: &mut PulseApp, events: Vec<egui::Event>| {
            time += 0.05;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0))),
                time: Some(time),
                events: [egui::Event::ModifiersChanged(Modifiers::ALT)].into_iter().chain(events).collect(),
                ..Default::default()
            };
            let _ = screen.ctx.run_ui(input, |ui| show(app, ui));
        };
        // The middle of the stretch, in track 1's automation lane (under its clips).
        let from = pos2(150.0 + 20.0 * app.arrangement.pixels_per_beat, 70.0 + 53.0 + 53.0 + 40.0);
        let to = from - vec2(0.0, 30.0);
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::ALT };
        frame(&mut app, vec![egui::Event::PointerMoved(from)]);
        frame(&mut app, vec![button(from, true)]);
        for i in 1..=6 {
            frame(&mut app, vec![egui::Event::PointerMoved(from + (to - from) * (i as f32 / 6.0))]);
        }
        frame(&mut app, vec![button(to, false)]);
        let lane = app.automation(target).iter().find(|l| l.param == Param::Volume).unwrap().clone();
        assert_eq!(lane.points.len(), 2, "no point added");
        assert!(lane.points[0].curve > 0.2, "dragged up, it bulges up: {}", lane.points[0].curve);
        assert!(info.normalize(lane.value_at(20.0).unwrap()) > 0.6, "above the straight line's 0.5");
        // ⌥ + double clic: straight again.
        let middle = pos2(from.x, to.y);
        frame(&mut app, vec![egui::Event::PointerMoved(middle)]);
        for _ in 0..2 {
            frame(&mut app, vec![button(middle, true)]);
            frame(&mut app, vec![button(middle, false)]);
        }
        let lane = app.automation(target).iter().find(|l| l.param == Param::Volume).unwrap();
        assert_eq!((lane.points.len(), lane.points[0].curve), (2, 0.0));
    }

    /// The user's case: the demo project plus an audio track with a song on the
    /// arrangement, right-click → «Separar en stems…», four tracks come out.
    #[test]
    #[ignore = "needs PULSE_STEMS_MODEL"]
    fn separating_from_the_arrangement_menu() {
        let mut app = app();
        let song = crate::test_audio::demo_song();
        let t = app.add_audio_track().unwrap();
        let sample = app.sample(&song.to_string_lossy());
        let audio = engine::model::AudioClip { hard_start: false, fades: Default::default(),
            name: "Canción".into(),
            color: [1, 2, 3],
            sample: song.to_string_lossy().into_owned(),
            length: (sample.duration() * 2.0).floor(),
            offset: 0.0,
            warp: engine::model::WarpMode::Off,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(), muted: false,
        };
        app.project.tracks[t].arrangement.push(ArrangementClip { start: 8.0, length: audio.length, clip: Clip::Audio(audio) });
        app.sync_arrangement(t);
        let mut screen = Screen::new();
        let clip = at(&app, t, 10.0);
        screen.menu(&mut app, clip, "Separar en stems…");
        assert!(app.stems.is_some(), "it started: {:?}", app.notice);
        let started = std::time::Instant::now();
        while app.stems.is_some() {
            let _ = screen.ctx.run_ui(Default::default(), |ui| app.follow_stems(ui.ctx()));
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert!(started.elapsed().as_secs() < 300);
        }
        assert_eq!(app.project.tracks.len(), t + 5, "{:?} {:?}", app.status, app.notice);
        for i in 0..4 {
            let placed = &app.project.tracks[t + 1 + i].arrangement[0];
            assert_eq!(placed.start, 8.0);
            if let Clip::Audio(stem) = &placed.clip {
                let _ = std::fs::remove_file(&stem.sample);
            }
        }
        assert!(!app.project.tracks[t].mixer.active);
    }
}

