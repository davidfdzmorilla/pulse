//! Arrangement view: a timeline per track with clips you can place, move and resize.
//!
//! - Drag in the top strip to set the loop region; click the ruler to move the start marker.
//! - Double-click an empty lane to create a clip; drag a clip to move it (also across
//!   tracks), drag its right edge to resize it. Clip contents loop when it is longer.

use crate::app::{ClipRef, Placement, PulseApp};
use crate::theme::{self, ACCENT, SOLO, TEXT, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine::model::{ArrangementClip, Clip, MidiClip};
use engine::{Sample, PEAK_BLOCK};
use engine::{Command, Target};
use std::sync::atomic::Ordering;

const HEADER_W: f32 = 150.0;
const LOOP_H: f32 = 12.0;
const RULER_H: f32 = 20.0;
const LANE_H: f32 = 52.0;
const EDGE_GRAB: f32 = 6.0;

#[derive(Clone, Copy)]
enum Drag {
    Move { track: usize, index: usize, grab: f64 },
    Resize { track: usize, index: usize },
    Loop { anchor: f64 },
    Scrub,
}

pub struct State {
    pub pixels_per_beat: f32,
    pub follow: bool,
    drag: Option<Drag>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            pixels_per_beat: 14.0,
            follow: false,
            drag: None,
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

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let position = app.audio.position();
    let playing = app.audio.is_playing();
    let state = app.audio.state();

    // Toolbar.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Zoom").color(TEXT_DIM).size(10.5));
        ui.add(egui::Slider::new(&mut app.arrangement.pixels_per_beat, 3.0..=60.0).logarithmic(true).show_value(false));
        ui.checkbox(&mut app.arrangement.follow, "Seguir cursor");
        ui.label(
            egui::RichText::new("Doble clic: nuevo clip · arrastra: mover · borde derecho: longitud · franja superior: loop")
                .color(TEXT_DIM)
                .size(10.5),
        );
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
    let lanes_height = LOOP_H + RULER_H + tracks as f32 * (LANE_H + 1.0);

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

        // Track headers.
        ui.vertical(|ui| {
            ui.set_width(HEADER_W);
            ui.add_space(LOOP_H + RULER_H);
            for t in 0..tracks {
                track_header(app, ui, t);
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
                Rect::from_min_size(pos2(rect.left(), lanes_top + t as f32 * (LANE_H + 1.0)), vec2(rect.width(), LANE_H))
            };

            // Background, lanes and grid.
            painter.rect_filled(rect, 0.0, theme::BG);
            for t in 0..tracks {
                painter.rect_filled(lane_rect(t), 0.0, theme::SLOT);
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
                    let selected = app.selected_clip == Some(ClipRef::Arrangement { track: t, index });
                    let sample = match &placed.clip {
                        Clip::Audio(audio) => app.loaded_sample(&audio.sample).map(|s| s.as_ref()),
                        Clip::Midi(_) => None,
                    };
                    paint_clip(&painter, clip_rect, placed, sample, selected, ppb, app.project.tempo);
                }
            }

            // Playhead and start marker.
            let following = |t: usize| {
                state.as_ref().is_none_or(|s| s.tracks[t].follows_arrangement.load(Ordering::Relaxed))
            };
            let x = x_at(position);
            painter.line_segment([pos2(x, rect.top() + LOOP_H), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::WHITE));
            widgets::play_triangle(&painter, pos2(x, rect.top() + LOOP_H + RULER_H - 6.0), 8.0, Color32::WHITE);
            for t in (0..tracks).filter(|&t| !following(t)) {
                // Tracks playing session clips are dimmed, as their arrangement is overridden.
                painter.rect_filled(lane_rect(t), 0.0, Color32::from_black_alpha(110));
            }

            handle_pointer(app, ui, &response, rect, lanes_top, unit, x_at(0.0));
        });
    });
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

fn track_header(app: &mut PulseApp, ui: &mut egui::Ui, t: usize) {
    let (rect, response) = ui.allocate_exact_size(vec2(HEADER_W, LANE_H + 1.0), Sense::click());
    let selected = app.selected_target == Target::Track(t);
    let track = &app.project.tracks[t];
    let color = theme::rgb(track.color);
    let painter = ui.painter();
    let body = Rect::from_min_size(rect.min, vec2(HEADER_W - 4.0, LANE_H));
    painter.rect_filled(body, 2.0, if selected { theme::CONTROL_HOVER } else { theme::SURFACE });
    painter.rect_filled(Rect::from_min_size(body.min, vec2(5.0, LANE_H)), 2.0, color);
    painter.text(body.left_top() + vec2(10.0, 6.0), Align2::LEFT_TOP, &track.name, theme::body(), TEXT);
    let (active, solo) = (track.mixer.active, track.solo);
    if response.clicked() {
        app.selected_target = Target::Track(t);
    }

    let buttons = Rect::from_min_size(body.left_bottom() + vec2(10.0, -22.0), vec2(80.0, 18.0));
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

fn handle_pointer(app: &mut PulseApp, ui: &egui::Ui, response: &egui::Response, rect: Rect, lanes_top: f32, unit: f64, origin_x: f32) {
    let ppb = app.arrangement.pixels_per_beat;
    let tracks = app.project.tracks.len();
    let Some(pos) = response.interact_pointer_pos().or_else(|| response.hover_pos()) else {
        return;
    };
    let beat = ((pos.x - origin_x) / ppb).max(0.0) as f64;
    let lane = if pos.y >= lanes_top {
        let t = ((pos.y - lanes_top) / (LANE_H + 1.0)) as usize;
        (t < tracks).then_some(t)
    } else {
        None
    };

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
            match clip_at(app, t, beat, pos.x, ppb, origin_x) {
                Some((index, edge)) => {
                    app.select_clip(ClipRef::Arrangement { track: t, index });
                    let start = app.project.tracks[t].arrangement[index].start;
                    Some(if edge {
                        Drag::Resize { track: t, index }
                    } else {
                        Drag::Move { track: t, index, grab: beat - start }
                    })
                }
                None => {
                    app.selected_target = Target::Track(t);
                    None
                }
            }
        } else {
            None
        };
    }

    if response.double_clicked() {
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
        match app.arrangement.drag {
            Some(Drag::Move { track, index, grab }) => {
                let start = snap(beat - grab, unit).max(0.0);
                let mut current = (track, index);
                // Moving vertically hands the clip over to another track.
                if let Some(target) = lane.filter(|&l| l != track) {
                    let placed = app.project.tracks[track].arrangement.remove(index);
                    app.project.tracks[target].arrangement.push(placed);
                    let new_index = app.project.tracks[target].arrangement.len() - 1;
                    app.sync_arrangement(track);
                    current = (target, new_index);
                    app.arrangement.drag = Some(Drag::Move { track: target, index: new_index, grab });
                    app.select_clip(ClipRef::Arrangement { track: target, index: new_index });
                }
                let (t, i) = current;
                let placed = &mut app.project.tracks[t].arrangement[i];
                if placed.start != start || current != (track, index) {
                    placed.start = start;
                    app.sync_arrangement(t);
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
            None => {}
        }
    }

    if ui.input(|i| i.pointer.primary_released()) {
        app.arrangement.drag = None;
    }
}
