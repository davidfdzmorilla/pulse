//! Session view: the clip launcher grid, scene launchers and mixer strips.

use crate::app::PulseApp;
use crate::project::PALETTE;
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense, Shape, Stroke, StrokeKind};
use engine::model::MidiClip;
use engine::{Command, NONE, NUM_SCENES, QUEUED_STOP};
use std::sync::atomic::Ordering;

const CELL_W: f32 = 132.0;
const CELL_H: f32 = 26.0;
const LAUNCH_W: f32 = 22.0;
const GAP: f32 = 2.0;

pub fn color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

fn dim(c: Color32, factor: f32) -> Color32 {
    Color32::from_rgb(
        (c.r() as f32 * factor) as u8,
        (c.g() as f32 * factor) as u8,
        (c.b() as f32 * factor) as u8,
    )
}

fn triangle(painter: &egui::Painter, center: egui::Pos2, size: f32, fill: Color32) {
    painter.add(Shape::convex_polygon(
        vec![
            pos2(center.x - size * 0.5, center.y - size * 0.6),
            pos2(center.x + size * 0.6, center.y),
            pos2(center.x - size * 0.5, center.y + size * 0.6),
        ],
        fill,
        Stroke::NONE,
    ));
}

/// Horizontal level meter; green, then yellow and red near full scale.
pub fn paint_meter(ui: &mut egui::Ui, size: egui::Vec2, level: f32) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    paint_meter_in(ui.painter(), rect, level, size.x >= size.y);
}

fn paint_meter_in(painter: &egui::Painter, rect: Rect, level: f32, horizontal: bool) {
    painter.rect_filled(rect, 2.0, Color32::from_gray(25));
    let level = level.clamp(0.0, 1.0);
    let fill = if level > 0.95 {
        Color32::from_rgb(230, 70, 60)
    } else if level > 0.7 {
        Color32::from_rgb(230, 200, 70)
    } else {
        Color32::from_rgb(90, 200, 110)
    };
    let bar = if horizontal {
        Rect::from_min_size(rect.min, vec2(rect.width() * level, rect.height()))
    } else {
        Rect::from_min_max(pos2(rect.left(), rect.bottom() - rect.height() * level), rect.max)
    };
    painter.rect_filled(bar, 2.0, fill);
}

enum Action {
    Launch(usize, usize),
    Select(usize, usize),
    SelectTrack(usize),
    Create(usize, usize),
    Delete(usize, usize),
    Duplicate(usize, usize),
    StopTrack(usize),
    LaunchScene(usize),
    StopAll,
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let state = app.audio.state();
    let blink = (ui.input(|i| i.time) * 4.0).fract() < 0.5;
    let mut actions = Vec::new();

    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        ui.horizontal_top(|ui| {
            for t in 0..app.project.tracks.len() {
                let status = state.as_ref().map(|s| &s.tracks[t]);
                let playing = status.map_or(NONE, |s| s.playing.load(Ordering::Relaxed));
                let queued = status.map_or(NONE, |s| s.queued.load(Ordering::Relaxed));
                let clip_position = status.map_or(0.0, |s| s.clip_position());

                ui.vertical(|ui| {
                    ui.set_width(CELL_W);
                    track_header(app, ui, t, &mut actions);
                    for slot in 0..NUM_SCENES {
                        clip_cell(
                            app,
                            ui,
                            (t, slot),
                            playing == slot as i32,
                            queued == slot as i32,
                            clip_position,
                            blink,
                            &mut actions,
                        );
                    }
                    let stop_queued = queued == QUEUED_STOP && blink;
                    let stop = egui::Button::new("■").fill(if stop_queued {
                        Color32::from_rgb(160, 60, 50)
                    } else {
                        Color32::from_gray(45)
                    });
                    if ui.add_sized([CELL_W, 20.0], stop).on_hover_text("Parar pista").clicked() {
                        actions.push(Action::StopTrack(t));
                    }
                    mixer_strip(app, ui, t);
                });
                ui.add_space(4.0);
            }

            ui.vertical(|ui| {
                ui.set_width(CELL_W);
                let (rect, _) = ui.allocate_exact_size(vec2(CELL_W, CELL_H), Sense::hover());
                ui.painter().rect_filled(rect, 3.0, Color32::from_gray(55));
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "Escenas",
                    FontId::proportional(13.0),
                    Color32::WHITE,
                );
                for slot in 0..NUM_SCENES {
                    let (rect, response) = ui.allocate_exact_size(vec2(CELL_W, CELL_H), Sense::click());
                    let hovered = response.hovered();
                    ui.painter().rect_filled(rect, 3.0, Color32::from_gray(if hovered { 70 } else { 50 }));
                    triangle(
                        ui.painter(),
                        pos2(rect.left() + LAUNCH_W * 0.5, rect.center().y),
                        9.0,
                        Color32::from_gray(200),
                    );
                    ui.painter().text(
                        pos2(rect.left() + LAUNCH_W + 4.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        &app.project.scene_names[slot],
                        FontId::proportional(13.0),
                        Color32::from_gray(220),
                    );
                    if response.on_hover_text("Lanzar escena").clicked() {
                        actions.push(Action::LaunchScene(slot));
                    }
                }
                if ui.add_sized([CELL_W, 20.0], egui::Button::new("Parar todo")).clicked() {
                    actions.push(Action::StopAll);
                }
            });
        });
    });

    for action in actions {
        apply(app, action);
    }
}

fn track_header(app: &PulseApp, ui: &mut egui::Ui, t: usize, actions: &mut Vec<Action>) {
    let track = &app.project.tracks[t];
    let (rect, response) = ui.allocate_exact_size(vec2(CELL_W, CELL_H), Sense::click());
    let selected = app.selected_track == t;
    ui.painter().rect_filled(rect, 3.0, color(track.color));
    if selected {
        ui.painter().rect_stroke(rect, 3.0, Stroke::new(2.0, Color32::WHITE), StrokeKind::Inside);
    }
    ui.painter().text(
        pos2(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        &track.name,
        FontId::proportional(13.0),
        Color32::BLACK,
    );
    if response.clicked() {
        actions.push(Action::SelectTrack(t));
    }
}

#[allow(clippy::too_many_arguments)]
fn clip_cell(
    app: &PulseApp,
    ui: &mut egui::Ui,
    (t, slot): (usize, usize),
    is_playing: bool,
    is_queued: bool,
    clip_position: f64,
    blink: bool,
    actions: &mut Vec<Action>,
) {
    let (rect, response) = ui.allocate_exact_size(vec2(CELL_W, CELL_H), Sense::click());
    let painter = ui.painter();
    let launch_rect = Rect::from_min_size(rect.min, vec2(LAUNCH_W, CELL_H));
    let clip = app.project.tracks[t].clips[slot].as_ref();
    let selected = app.selected_clip == Some((t, slot));
    let pointer_on_launch = response
        .hover_pos()
        .is_some_and(|p| launch_rect.contains(p));

    match clip {
        Some(clip) => {
            let base = color(clip.color);
            painter.rect_filled(rect, 3.0, if selected { base } else { dim(base, 0.75) });
            painter.rect_filled(launch_rect, 3.0, dim(base, 0.45));
            let icon = if is_playing {
                Color32::from_rgb(120, 255, 130)
            } else if is_queued && blink {
                Color32::from_rgb(120, 255, 130)
            } else if pointer_on_launch {
                Color32::WHITE
            } else {
                Color32::from_gray(210)
            };
            triangle(painter, launch_rect.center(), 9.0, icon);
            painter.text(
                pos2(launch_rect.right() + 5.0, rect.center().y),
                Align2::LEFT_CENTER,
                &clip.name,
                FontId::proportional(12.5),
                Color32::BLACK,
            );
            if is_playing && clip.length > 0.0 {
                let progress = (clip_position / clip.length) as f32;
                let bar = Rect::from_min_size(
                    pos2(rect.left(), rect.bottom() - 3.0),
                    vec2(rect.width() * progress, 3.0),
                );
                painter.rect_filled(bar, 0.0, Color32::from_rgb(120, 255, 130));
            }
        }
        None => {
            painter.rect_filled(rect, 3.0, Color32::from_gray(if response.hovered() { 48 } else { 38 }));
            let stop = Rect::from_center_size(launch_rect.center(), vec2(7.0, 7.0));
            painter.rect_filled(stop, 1.0, Color32::from_gray(if pointer_on_launch { 160 } else { 85 }));
        }
    }
    if selected {
        painter.rect_stroke(rect, 3.0, Stroke::new(1.5, Color32::WHITE), StrokeKind::Inside);
    }

    let clicked_launch = response
        .interact_pointer_pos()
        .is_some_and(|p| launch_rect.contains(p));
    if response.double_clicked() && clip.is_none() {
        actions.push(Action::Create(t, slot));
    } else if response.clicked() {
        if clicked_launch {
            actions.push(Action::Launch(t, slot));
        } else if clip.is_some() {
            actions.push(Action::Select(t, slot));
        } else {
            actions.push(Action::SelectTrack(t));
        }
    }

    let hint = if clip.is_some() {
        "▶ lanza · clic edita · clic derecho para más"
    } else {
        "Doble clic: nuevo clip MIDI"
    };
    let response = response.on_hover_text(hint);
    if clip.is_some() {
        response.context_menu(|ui| {
            if ui.button("Lanzar").clicked() {
                actions.push(Action::Launch(t, slot));
                ui.close();
            }
            if ui.button("Duplicar").clicked() {
                actions.push(Action::Duplicate(t, slot));
                ui.close();
            }
            if ui.button("Eliminar").clicked() {
                actions.push(Action::Delete(t, slot));
                ui.close();
            }
        });
    }
}

fn mixer_strip(app: &mut PulseApp, ui: &mut egui::Ui, t: usize) {
    let track = &mut app.project.tracks[t];
    let mut commands = Vec::new();
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let mute_fill = if track.mute { Color32::from_gray(60) } else { Color32::from_rgb(220, 170, 60) };
        let mute = egui::Button::new(egui::RichText::new(format!("{}", t + 1)).color(Color32::BLACK))
            .fill(mute_fill);
        if ui.add_sized([28.0, 20.0], mute).on_hover_text("Activar / silenciar pista").clicked() {
            track.mute = !track.mute;
            commands.push(Command::SetMute { track: t, mute: track.mute });
        }
        ui.spacing_mut().slider_width = CELL_W - 34.0;
        let pan = ui
            .add(egui::Slider::new(&mut track.pan, -1.0..=1.0).show_value(false))
            .on_hover_text(format!("Pan {:+.0}", track.pan * 50.0));
        if pan.double_clicked() {
            track.pan = 0.0;
        }
        if pan.changed() || pan.double_clicked() {
            commands.push(Command::SetPan { track: t, pan: track.pan });
        }
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().slider_width = 110.0;
        let volume = ui
            .add(egui::Slider::new(&mut track.volume, 0.0..=1.2).vertical().show_value(false))
            .on_hover_text(format!("Volumen {:.0}%", track.volume * 100.0));
        if volume.changed() {
            commands.push(Command::SetVolume { track: t, volume: track.volume });
        }
        let (rect, _) = ui.allocate_exact_size(vec2(10.0, 110.0), Sense::hover());
        paint_meter_in(ui.painter(), rect, app.meters[t], false);
    });
    for command in commands {
        app.audio.send(command);
    }
}

fn first_empty_slot(app: &PulseApp, t: usize, after: usize) -> Option<usize> {
    (after + 1..NUM_SCENES).find(|&s| app.project.tracks[t].clips[s].is_none())
}

fn apply(app: &mut PulseApp, action: Action) {
    match action {
        Action::Launch(t, slot) => {
            if app.project.tracks[t].clips[slot].is_some() {
                app.selected_clip = Some((t, slot));
            }
            app.selected_track = t;
            app.audio.send(Command::LaunchClip { track: t, slot });
        }
        Action::Select(t, slot) => {
            app.selected_clip = Some((t, slot));
            app.selected_track = t;
        }
        Action::SelectTrack(t) => app.selected_track = t,
        Action::Create(t, slot) => {
            let track = &app.project.tracks[t];
            let name = format!("Clip {}", slot + 1);
            let clip_color = if track.color == [0, 0, 0] { PALETTE[0] } else { track.color };
            app.set_clip(t, slot, Some(MidiClip::new(name, 4.0, clip_color)));
            app.selected_clip = Some((t, slot));
            app.selected_track = t;
        }
        Action::Delete(t, slot) => app.set_clip(t, slot, None),
        Action::Duplicate(t, slot) => {
            if let Some(target) = first_empty_slot(app, t, slot) {
                let copy = app.project.tracks[t].clips[slot].clone();
                app.set_clip(t, target, copy);
                app.selected_clip = Some((t, target));
            }
        }
        Action::StopTrack(t) => app.audio.send(Command::StopTrack(t)),
        Action::LaunchScene(slot) => app.audio.send(Command::LaunchScene(slot)),
        Action::StopAll => app.audio.send(Command::StopAllClips),
    }
}
