//! Arrangement view: a timeline per track with clips you can place, move and resize.
//!
//! - Drag in the top strip to set the loop region; click the ruler to move the start marker.
//! - Double-click an empty lane to create a clip; drag a clip to move it (also across
//!   tracks), drag its right edge to resize it. Clip contents loop when it is longer.
//! - The "A" button of a track shows an automation lane below it: click to add a
//!   breakpoint, drag to move it, double-click to delete it.

use crate::app::{ClipRef, Placement, PulseApp};
use crate::theme::{self, ACCENT, SOLO, TEXT, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine::model::{ArrangementClip, AutomationPoint, Clip, MidiClip, Param};
use std::collections::HashMap;
use engine::{Sample, PEAK_BLOCK};
use engine::{Command, Target};
use std::sync::atomic::Ordering;

const HEADER_W: f32 = 150.0;
const LOOP_H: f32 = 12.0;
const RULER_H: f32 = 20.0;
const LANE_H: f32 = 52.0;
const EDGE_GRAB: f32 = 6.0;
const AUTO_H: f32 = 64.0;
const POINT_GRAB: f32 = 6.0;

#[derive(Clone, Debug)]
enum Drag {
    /// The selected clips, grabbed at `grab` (beat) on `grab_track`. They are laid
    /// out again from `snapshot` (every lane before the drag) each time the offset
    /// changes; `applied` is the offset in place (beats, tracks).
    Move { grab_track: usize, grab: f64, moved: Vec<(usize, usize)>, primary: usize, snapshot: Vec<Vec<ArrangementClip>>, copy: bool, applied: (f64, isize) },
    Resize { track: usize, index: usize },
    Loop { anchor: f64 },
    Scrub,
    /// Dragging a breakpoint; `lane` is the lane's rectangle when the drag began.
    Point { track: usize, param: Param, index: usize, lane: Rect },
}

pub struct State {
    pub pixels_per_beat: f32,
    pub follow: bool,
    drag: Option<Drag>,
    /// Tracks showing an automation lane, and the parameter it shows.
    pub automation: HashMap<usize, Param>,
    /// What the automation parameter list is filtered by (long lists: plugins).
    pub automation_filter: String,
    /// What was right-clicked, for its menu.
    menu: Option<Menu>,
    /// A track to duplicate or delete once the view is drawn (true: the selected ones).
    track_action: Option<(TrackAction, usize, bool)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TrackAction {
    Duplicate,
    Delete,
}

/// What a right-click landed on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Menu {
    Clip(ClipRef),
    /// An empty place on a track, at a beat (snapped to the grid).
    Lane { track: usize, beat: f64 },
    Automation { track: usize, param: Param },
}

impl Default for State {
    fn default() -> Self {
        Self {
            pixels_per_beat: 14.0,
            follow: false,
            drag: None,
            automation: HashMap::new(),
            automation_filter: String::new(),
            menu: None,
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

/// Vertical position of each track row (relative to the first lane) and its automation lane, if shown.
struct Layout {
    rows: Vec<(f32, Option<Param>)>,
    height: f32,
}

impl Layout {
    fn new(app: &PulseApp) -> Self {
        let mut rows = Vec::new();
        let mut y = 0.0;
        for t in 0..app.project.tracks.len() {
            let shown = app.arrangement.automation.get(&t).copied();
            rows.push((y, shown));
            y += row_height(shown.is_some());
        }
        Self { rows, height: y }
    }

    /// Track under `y` (relative to the first lane) and whether it is in its automation lane.
    fn hit(&self, y: f32) -> Option<(usize, bool)> {
        self.rows.iter().enumerate().find_map(|(t, &(top, shown))| {
            let clips_end = top + LANE_H + 1.0;
            if y >= top && y < clips_end {
                Some((t, false))
            } else if shown.is_some() && y >= clips_end && y < clips_end + AUTO_H + 1.0 {
                Some((t, true))
            } else {
                None
            }
        })
    }
}

fn row_height(automation: bool) -> f32 {
    LANE_H + 1.0 + if automation { AUTO_H + 1.0 } else { 0.0 }
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let position = app.audio.position();
    let playing = app.audio.is_playing();
    let state = app.audio.state();

    // Toolbar.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Zoom").color(TEXT_DIM).size(10.5));
        ui.add(egui::Slider::new(&mut app.arrangement.pixels_per_beat, 3.0..=60.0).logarithmic(true).show_value(false));
        ui.checkbox(&mut app.arrangement.follow, "Seguir cursor");
        let hint = "Doble clic: nuevo clip · arrastra: mover (⌥ copia) · ⌘ clic: varios · arrastrar en vacío: recuadro · borde derecho: longitud · franja superior: loop";
        // Cut with "…" in a narrow window; the tooltip has all of it.
        ui.add(egui::Label::new(egui::RichText::new(hint).color(TEXT_DIM).size(10.5)).truncate()).on_hover_text(hint);
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
    let layout = Layout::new(app);
    let lanes_height = LOOP_H + RULER_H + layout.height;
    let mut headers = Vec::new();
    let mut clip_rects = Vec::new();
    let mut lane_rects = Vec::new();

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

        // Track headers.
        ui.vertical(|ui| {
            ui.set_width(HEADER_W);
            ui.add_space(LOOP_H + RULER_H);
            for t in 0..tracks {
                headers.push(track_header(app, ui, t, layout.rows[t].1));
            }
        });

        let mut scroll = egui::ScrollArea::horizontal().id_salt("arrangement").auto_shrink(false);
        if app.arrangement.follow && playing {
            let view = ui.available_width();
            let x = position as f32 * ppb;
            scroll = scroll.horizontal_scroll_offset((x - view * 0.3).max(0.0));
        }
        scroll.show(ui, |ui| {
            let size = vec2(total_beats as f32 * ppb, lanes_height.max(ui.available_height()));
            let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
            let painter = ui.painter_at(rect);
            let x_at = |beat: f64| rect.left() + beat as f32 * ppb;
            let lanes_top = rect.top() + LOOP_H + RULER_H;
            let lane_rect = |t: usize| {
                Rect::from_min_size(pos2(rect.left(), lanes_top + layout.rows[t].0), vec2(rect.width(), LANE_H))
            };
            let auto_rect = |t: usize| {
                Rect::from_min_size(pos2(rect.left(), lanes_top + layout.rows[t].0 + LANE_H + 1.0), vec2(rect.width(), AUTO_H))
            };

            // Background, lanes and grid.
            painter.rect_filled(rect, 0.0, theme::BG);
            for t in 0..tracks {
                painter.rect_filled(lane_rect(t), 0.0, theme::SLOT);
                lane_rects.push((t, lane_rect(t)));
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
                        [pos2(x, rect.top() + LOOP_H + 8.0), pos2(x, lanes_top)],
                        Stroke::new(1.0, TEXT_DIM),
                    );
                    painter.text(
                        pos2(x + 3.0, rect.top() + LOOP_H + 2.0),
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

            // Clips.
            for t in 0..tracks {
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

            // Automation lanes.
            for t in 0..tracks {
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
            widgets::play_triangle(&painter, pos2(x, rect.top() + LOOP_H + RULER_H - 6.0), 8.0, Color32::WHITE);
            // Region being recorded on armed tracks.
            if let Some(take) = &app.recorder.arrangement {
                for t in (0..tracks).filter(|&t| app.project.tracks[t].armed && !app.project.tracks[t].is_audio()) {
                    let lane = lane_rect(t);
                    let region = Rect::from_min_max(pos2(x_at(take.start), lane.top()), pos2(x.max(x_at(take.start)), lane.bottom()));
                    painter.rect_filled(region, 2.0, Color32::from_rgba_unmultiplied(240, 82, 70, 60));
                }
            }
            for t in (0..tracks).filter(|&t| !following(t)) {
                // Tracks playing session clips are dimmed, as their arrangement is overridden.
                painter.rect_filled(lane_rect(t), 0.0, Color32::from_black_alpha(110));
            }

            handle_pointer(app, ui, &response, rect, lanes_top, unit, x_at(0.0), &layout);
            context_menu(app, &response);
        });
    });
    app.finish_track_drag(ui, &headers, true);
    match app.arrangement.track_action.take() {
        Some((TrackAction::Duplicate, _, true)) => app.duplicate_selected_tracks(),
        Some((TrackAction::Duplicate, t, false)) => app.duplicate_track(t),
        Some((TrackAction::Delete, _, true)) => app.delete_selected_tracks(),
        Some((TrackAction::Delete, t, false)) => app.delete_track(t),
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
    let color = theme::rgb(placed.clip.color());
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
            // One waveform column per pixel, mapping beats to file time like the engine does.
            let reference_bpm = match audio.warp {
                engine::model::WarpMode::Off => tempo,
                _ => audio.original_bpm.max(1.0),
            };
            let mid = body.center().y;
            let half = body.height() * 0.5;
            let wave = theme::shade(color, 1.25);
            let x_end = rect.right().min(painter.clip_rect().right());
            let mut x = rect.left().max(painter.clip_rect().left());
            while x < x_end {
                let beat = ((x - rect.left()) / ppb) as f64;
                let local = beat.rem_euclid(audio.length.max(0.0001));
                let next_local = local + (1.0 / ppb) as f64;
                let to_block = |b: f64| ((audio.offset + b * 60.0 / reference_bpm) * sample.sample_rate as f64) as usize / PEAK_BLOCK;
                let (a, b) = (to_block(local), to_block(next_local).max(to_block(local) + 1));
                let (lo, hi) = sample.peaks.get(a..b.min(sample.peaks.len())).unwrap_or(&[]).iter().fold((0f32, 0f32), |(lo, hi), &(l, h)| (lo.min(l), hi.max(h)));
                if hi > lo {
                    clipped.line_segment([pos2(x, mid - hi * half), pos2(x, mid - lo * half)], Stroke::new(1.0, wave));
                }
                x += 1.0;
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

/// A track's header: clic to select (⌘ and ⇧ for several), drag to move. Returns where it is.
fn track_header(app: &mut PulseApp, ui: &mut egui::Ui, t: usize, shown: Option<Param>) -> Rect {
    let (rect, response) = ui.allocate_exact_size(vec2(HEADER_W, row_height(shown.is_some())), Sense::click_and_drag());
    let response = response.on_hover_text("Clic: seleccionar · ⌘ clic: añadir o quitar · ⇧ clic: rango · arrastrar: mover");
    app.track_header_input(t, &response);
    track_menu(app, &response, t, shown.is_some());
    let selected = app.is_track_selected(t);
    let track = &app.project.tracks[t];
    let color = theme::rgb(track.color);
    let painter = ui.painter();
    let body = Rect::from_min_size(rect.min, vec2(HEADER_W - 4.0, LANE_H));
    painter.rect_filled(body, 2.0, if selected { theme::CONTROL_HOVER } else { theme::SURFACE });
    painter.rect_filled(Rect::from_min_size(body.min, vec2(5.0, LANE_H)), 2.0, color);
    painter.text(body.left_top() + vec2(10.0, 6.0), Align2::LEFT_TOP, &track.name, theme::body(), TEXT);
    let (active, solo) = (track.mixer.active, track.solo);

    let buttons = Rect::from_min_size(body.left_bottom() + vec2(10.0, -22.0), vec2(110.0, 18.0));
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
    if widgets::toggle(&mut child, armed, "●", theme::METER_RED, vec2(24.0, 16.0))
        .on_hover_text("Armar para grabar")
        .clicked()
    {
        app.toggle_arm(t);
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

    let Some(param) = shown else { return rect };
    let area = Rect::from_min_size(rect.left_top() + vec2(10.0, LANE_H + 6.0), vec2(HEADER_W - 18.0, AUTO_H - 8.0));
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
        app.arrangement.automation.insert(t, chosen);
    }
    if app.automation_state(target, param).is_some() && child.small_button("Borrar automatización").clicked() {
        app.automation_mut(target).retain(|l| l.param != param);
        app.overridden.remove(&(target, param));
        app.sync_automation(target);
    }
    rect
}

/// Maps a parameter value to a y position inside an automation lane.
fn value_y(rect: Rect, normalized: f32) -> f32 {
    rect.bottom() - 4.0 - normalized * (rect.height() - 8.0)
}

fn paint_automation(app: &PulseApp, painter: &egui::Painter, rect: Rect, t: usize, param: Param, x_at: &dyn Fn(f64) -> f32) {
    painter.rect_filled(rect, 0.0, theme::shade(theme::SLOT, 0.8));
    let target = Target::Track(t);
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
            let mut line = vec![pos2(rect.left(), y(lane.points[0].value))];
            line.extend(lane.points.iter().map(|p| pos2(x_at(p.beat), y(p.value))));
            line.push(pos2(rect.right(), y(lane.points[lane.points.len() - 1].value)));
            painter.add(egui::Shape::line(line, Stroke::new(1.5, color)));
            for p in &lane.points {
                painter.circle_filled(pos2(x_at(p.beat), y(p.value)), 3.5, color);
            }
            if overridden {
                painter.text(
                    rect.left_top() + vec2(8.0, 4.0),
                    Align2::LEFT_TOP,
                    "Anulada a mano · ▶ Arrangement la reactiva",
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
    response.context_menu(|ui| {
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
    });
}

/// The right-click menu of the timeline: for a clip, an empty place on a track,
/// or an automation lane.
fn context_menu(app: &mut PulseApp, response: &egui::Response) {
    response.context_menu(|ui| {
        let Some(menu) = app.arrangement.menu else {
            ui.close();
            return;
        };
        match menu {
            Menu::Clip(clip) => clip_menu(app, ui, clip),
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
                } else if ui.button("Nuevo clip MIDI aquí").clicked() {
                    let color = app.project.tracks[track].color;
                    app.project.tracks[track].arrangement.push(ArrangementClip { start: bar, length: 4.0, clip: Clip::Midi(MidiClip::new("MIDI", 4.0, color)) });
                    let index = app.project.tracks[track].arrangement.len() - 1;
                    app.sync_arrangement(track);
                    app.select_clip(ClipRef::Arrangement { track, index });
                    ui.close();
                }
            }
            Menu::Automation { track, param } => {
                let target = Target::Track(track);
                ui.label(egui::RichText::new(app.param_name(target, param)).color(TEXT_DIM).size(10.5));
                if app.automation_state(target, param).is_some() && ui.button("Borrar automatización").clicked() {
                    app.automation_mut(target).retain(|l| l.param != param);
                    app.overridden.remove(&(target, param));
                    app.sync_automation(target);
                    ui.close();
                }
                if ui.button("Ocultar carril").clicked() {
                    app.arrangement.automation.remove(&track);
                    ui.close();
                }
            }
        }
    });
}

/// A clip's menu: what acts on the selected clips, and the clip's name and colour.
fn clip_menu(app: &mut PulseApp, ui: &mut egui::Ui, clip: ClipRef) {
    let ClipRef::Arrangement { track, index } = clip else { return };
    let Some(placed) = app.project.tracks.get(track).and_then(|t| t.arrangement.get(index)) else { return ui.close() };
    let audio = matches!(placed.clip, Clip::Audio(_));
    let count = app.clip_selection().len();
    let label = |verb: &str| if count > 1 { format!("{verb} {count} clips") } else { verb.to_string() };
    if audio && count == 1 && ui.add_enabled(app.stems.is_none(), egui::Button::new("Separar en stems…")).on_hover_text("Batería, bajo, resto y voz, cada uno en una pista nueva").clicked() {
        app.separate_stems(clip);
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
    if ui.button(format!("{}   ⌘D", label("Duplicar"))).clicked() {
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
    ui.separator();
    let clip = &mut app.project.tracks[track].arrangement[index].clip;
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
        let param = layout.rows[t].1?;
        let top = lanes_top + layout.rows[t].0 + LANE_H + 1.0;
        Some((t, param, Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), AUTO_H))))
    });
    // A right-click picks what the menu is about (and selects a clip not selected).
    if response.secondary_clicked() {
        app.arrangement.menu = match (lane, automation) {
            (Some(t), _) => Some(match clip_at(app, t, beat, pos.x, ppb, origin_x) {
                Some((index, _)) => {
                    let clip = ClipRef::Arrangement { track: t, index };
                    if !app.is_clip_selected(clip) {
                        app.select_clip(clip);
                    }
                    Menu::Clip(clip)
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
    if handle_automation(app, ui, response, pos, beat, origin_x, automation) {
        return;
    }

    // Cursor feedback for resizing.
    if app.arrangement.drag.is_none() {
        if let Some(t) = lane {
            if let Some((_, true)) = clip_at(app, t, beat, pos.x, ppb, origin_x) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
    }

    let pressed = ui.input(|i| i.pointer.primary_pressed()) && response.hovered();
    if pressed {
        app.arrangement.drag = if pos.y < rect.top() + LOOP_H {
            Some(Drag::Loop { anchor: snap(beat, unit) })
        } else if pos.y < lanes_top {
            app.audio.send(Command::SetPosition(snap(beat, unit)));
            Some(Drag::Scrub)
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
            None
        };
    }

    // A plain clic on one of several selected clips leaves only that one.
    if response.clicked() && !ui.input(|i| i.modifiers.command || i.modifiers.shift) {
        if let Some((index, _)) = lane.and_then(|t| clip_at(app, t, beat, pos.x, ppb, origin_x)) {
            let clip = ClipRef::Arrangement { track: lane.unwrap_or(0), index };
            if app.clip_selection().len() > 1 {
                app.select_clip(clip);
            }
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
            Some(Drag::Point { .. }) | None => {}
        }
    }

    if ui.input(|i| i.pointer.primary_released()) {
        app.arrangement.drag = None;
    }
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
    if app.arrangement.drag.as_ref().is_some_and(|d| !matches!(d, Drag::Point { .. })) {
        return false;
    }
    let ppb = app.arrangement.pixels_per_beat;
    let unit = point_unit(ppb);
    let x_at = |b: f64| origin_x + b as f32 * ppb;

    // Dragging a breakpoint continues even if the pointer leaves the lane.
    if let Some(Drag::Point { track, param, index, lane: rect }) = app.arrangement.drag {
        if response.dragged() {
            let target = Target::Track(track);
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

    let Some((track, param, rect)) = lane else { return false };
    let target = Target::Track(track);
    let Some(info) = app.param_info(target, param) else { return true };
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

    if response.double_clicked() {
        if let Some(index) = near {
            app.lane_mut(target, param).points.remove(index);
            app.sync_automation(target);
        }
        return true;
    }
    if ui.input(|i| i.pointer.primary_pressed()) && response.hovered() {
        app.selected_target = target;
        let index = match near {
            Some(index) => index,
            None => {
                let value = info.denormalize((rect.bottom() - 4.0 - pos.y) / (rect.height() - 8.0));
                let lane = app.lane_mut(target, param);
                let point = AutomationPoint { beat: snap(beat, unit), value };
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
        pos2(150.0 + beat as f32 * app.arrangement.pixels_per_beat, 81.0 + t as f32 * 53.0)
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
        screen.menu(&mut app, clip, "Duplicar   ⌘D");
        assert_eq!(app.project.tracks[0].arrangement.len(), 2);
        screen.menu(&mut app, clip, "Eliminar   ⌫");
        let starts: Vec<f64> = app.project.tracks[0].arrangement.iter().map(|c| c.start).collect();
        assert_eq!(starts, vec![8.0], "the copy stays");
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

    #[test]
    fn track_headers_and_automation_lanes_have_menus_too() {
        let mut app = app();
        let mut screen = Screen::new();
        let tracks = app.project.tracks.len();
        let header = |t: usize| pos2(60.0, 70.0 + t as f32 * 53.0);
        screen.menu(&mut app, header(1), "Mostrar automatización");
        assert_eq!(app.arrangement.automation.get(&1), Some(&Param::Volume));
        // Points on the lane, then its menu (the lane is under track 1's clips).
        app.lane_mut(Target::Track(1), Param::Volume).points.push(AutomationPoint { beat: 0.0, value: 0.5 });
        let lane = pos2(150.0 + 20.0 * app.arrangement.pixels_per_beat, 55.0 + 53.0 + 53.0 + 30.0);
        screen.menu(&mut app, lane, "Borrar automatización");
        assert!(app.automation(Target::Track(1)).iter().all(|l| l.param != Param::Volume || l.points.is_empty()));
        screen.menu(&mut app, lane, "Ocultar carril");
        assert!(app.arrangement.automation.is_empty());
        // Deleting a track from its own header, drawn as it goes.
        screen.menu(&mut app, header(1), "Eliminar pista");
        assert_eq!(app.project.tracks.len(), tracks - 1);
    }

    /// The user's case: the demo project plus an audio track with a song on the
    /// arrangement, right-click → «Separar en stems…», four tracks come out.
    #[test]
    #[ignore = "needs PULSE_STEMS_MODEL"]
    fn separating_from_the_arrangement_menu() {
        let mut app = app();
        let song = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo.wav");
        let t = app.add_audio_track().unwrap();
        let sample = app.sample(&song.to_string_lossy());
        let audio = engine::model::AudioClip {
            name: "Canción".into(),
            color: [1, 2, 3],
            sample: song.to_string_lossy().into_owned(),
            length: (sample.duration() * 2.0).floor(),
            offset: 0.0,
            warp: engine::model::WarpMode::Off,
            original_bpm: 120.0,
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(),
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

