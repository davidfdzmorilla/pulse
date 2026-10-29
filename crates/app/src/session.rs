//! Session view: clip slots, scene launchers in the master column, and the mixer.

use crate::app::{ClipRef, Placement, PulseApp};
use crate::project::PALETTE;
use crate::theme::{self, ACCENT, PLAY, SLOT, SLOT_HOVER, SOLO, TEXT, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense};
use engine::model::{Clip, Param, PAN_PARAM, SEND_PARAM};
use engine::{Command, Target, NONE, MAX_RETURNS, NUM_SCENES, QUEUED_STOP};
use std::sync::atomic::Ordering;

const COL_W: f32 = 108.0;
const HEADER_H: f32 = 22.0;
const SLOT_H: f32 = 22.0;
const LAUNCH_W: f32 = 18.0;
/// One row of send knobs (two to a row).
const SENDS_ROW_H: f32 = 50.0;

/// The height of the send knobs: two to a row, as many rows as the returns
/// need (one at least, as with none).
fn sends_h(returns: usize) -> f32 {
    returns.div_ceil(2).max(1) as f32 * SENDS_ROW_H + 6.0
}
const PAN_H: f32 = 58.0;
const FADER_H: f32 = 120.0;
const MONITOR_H: f32 = 16.0;

pub(crate) enum Action {
    Launch(usize, usize),
    /// A group's slot: its tracks play their clips in that scene.
    LaunchGroup(usize, usize),
    StopGroup(usize),
    ToggleFold(usize),
    GroupTracks,
    Ungroup(usize),
    LeaveGroup(usize),
    ClickClip(ClipRef, egui::Modifiers),
    StartMove(usize, usize, egui::Modifiers),
    StartMarquee(egui::Pos2, egui::Modifiers),
    SelectTarget(Target),
    Consolidate(usize, usize),
    Reverse(usize, usize),
    ToggleClipMuted(usize, usize),
    ToMidi(usize, usize, crate::to_midi::ToMidi),
    TakeGroove(usize, usize),
    ExportMidi(usize, usize),
    Accompany(usize, usize, crate::accompany::Part),
    HumSong(usize, usize, crate::drum_gen::Style),
    Freeze(usize),
    /// A rumble track from this one (see `rumble`).
    Rumble(usize),
    /// A drum machine sound of this track on a track of its own (see `drum_split`).
    SoundApart(usize, usize),
    Unfreeze(usize),
    Flatten(usize),
    /// An empty slot clicked: its track selected, no clip, pasting goes there.
    SelectSlot(usize, usize),
    Create(usize, usize),
    Delete(usize, usize),
    Duplicate(usize, usize),
    ToArrangement(usize, usize),
    SetClipColor(usize, usize, [u8; 3]),
    StopTrack(usize),
    LaunchScene(usize),
    StopAll,
    AddTrack,
    AddReturn,
    DeleteReturn(usize),
    Record(usize, usize),
    StopRecording(usize),
    DeleteTrack(usize),
    DuplicateTrack(usize),
    SeparateStems(usize, usize),
    DetectTempo(usize, usize),
    UseClipTempo(usize, usize),
}

struct TrackPlayState {
    playing: i32,
    queued: i32,
    clip_position: f64,
    recording: i32,
    queued_record: bool,
    /// The bar of the current take being recorded (from 1).
    recorded_bar: i64,
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let state = app.audio.state();
    let blink = (ui.input(|i| i.time) * 3.0).fract() < 0.5;
    let mut actions = Vec::new();
    let mut headers = Vec::new();
    let mut slots = Vec::new();

    let rows = shown_scenes(&app.project);
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
            for t in 0..app.project.tracks.len() {
                // Inside a folded group: not shown.
                if crate::groups::hidden(&app.project.tracks, t) {
                    continue;
                }
                let is_group = app.project.tracks[t].is_group();
                let play = state.as_ref().map_or(
                    TrackPlayState {
                        playing: NONE,
                        queued: NONE,
                        clip_position: 0.0,
                        recording: NONE,
                        queued_record: false,
                        recorded_bar: 1,
                    },
                    |s| {
                        let status = &s.tracks[t];
                        TrackPlayState {
                            playing: status.playing.load(Ordering::Relaxed),
                            queued: status.queued.load(Ordering::Relaxed),
                            clip_position: status.clip_position(),
                            recording: status.recording.load(Ordering::Relaxed),
                            queued_record: status.queued_record.load(Ordering::Relaxed),
                            recorded_bar: {
                                let meter = &app.project.meter;
                                (meter.bar_at(s.position()).0 - meter.bar_at(status.record_start()).0).max(0) + 1
                            },
                        }
                    },
                );
                ui.vertical(|ui| {
                    ui.set_width(COL_W);
                    headers.push((t, track_header(app, ui, t, &mut actions)));
                    for slot in 0..rows {
                        if is_group {
                            group_slot(app, ui, t, slot, state.as_deref(), &mut actions);
                        } else {
                            slots.push((t, slot, clip_slot(app, ui, t, slot, &play, blink, &mut actions)));
                        }
                    }
                    let stop_queued = play.queued == QUEUED_STOP && blink;
                    if stop_cell(ui, stop_queued, if is_group { "Parar las pistas del grupo" } else { "Parar pista" }).clicked() {
                        actions.push(if is_group { Action::StopGroup(t) } else { Action::StopTrack(t) });
                    }
                    ui.add_space(6.0);
                    mixer_strip(app, ui, Target::Track(t));
                });
            }

            ui.vertical(|ui| {
                let (rect, response) = ui.allocate_exact_size(vec2(26.0, HEADER_H), Sense::click());
                widgets::name(&response, egui::WidgetType::Button, "Nueva pista MIDI");
                let fill = if response.hovered() { theme::CONTROL_HOVER } else { theme::CONTROL };
                ui.painter().rect_filled(rect, 2.0, fill);
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, "+", theme::body(), TEXT);
                if response.on_hover_text(crate::theme::keys("Nueva pista MIDI (⌘T)")).clicked() {
                    actions.push(Action::AddTrack);
                }
            });
            ui.add_space(10.0);

            for r in 0..app.project.returns.len() {
                ui.vertical(|ui| {
                    ui.set_width(COL_W);
                    let target = Target::Return(r);
                    let name = app.project.returns[r].name.clone();
                    let color = theme::rgb(app.project.returns[r].color);
                    let response = header(ui, &name, color, app.selected_target == target, Sense::click());
                    if response.clicked() {
                        actions.push(Action::SelectTarget(target));
                    }
                    response.context_menu(|ui| {
                        if ui.button("Eliminar retorno").on_hover_text("Las pistas dejan de enviarle sonido; los retornos de después suben una letra").clicked() {
                            actions.push(Action::DeleteReturn(r));
                            ui.close();
                        }
                    });
                    let (rect, _) = ui.allocate_exact_size(vec2(COL_W, (SLOT_H + 1.0) * (rows as f32 + 1.0) - 1.0), Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, theme::shade(SLOT, 0.8));
                    ui.add_space(6.0);
                    mixer_strip(app, ui, target);
                });
            }
            if app.project.returns.len() < MAX_RETURNS {
                ui.vertical(|ui| {
                    let (rect, response) = ui.allocate_exact_size(vec2(26.0, HEADER_H), Sense::click());
                    widgets::name(&response, egui::WidgetType::Button, "Nuevo retorno");
                    let fill = if response.hovered() { theme::CONTROL_HOVER } else { theme::CONTROL };
                    ui.painter().rect_filled(rect, 2.0, fill);
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, "+", theme::body(), TEXT);
                    if response.on_hover_text(crate::theme::keys("Nuevo retorno (⌥⌘T)")).clicked() {
                        actions.push(Action::AddReturn);
                    }
                });
            }
            ui.add_space(10.0);

            ui.vertical(|ui| {
                ui.set_width(COL_W);
                let target = Target::Master;
                if header(ui, "Master", theme::CONTROL_HOVER, app.selected_target == target, Sense::click()).clicked() {
                    actions.push(Action::SelectTarget(target));
                }
                for slot in 0..rows {
                    scene_cell(app, ui, slot, &mut actions);
                }
                if stop_cell(ui, false, "Parar todos los clips").clicked() {
                    actions.push(Action::StopAll);
                }
                ui.add_space(6.0);
                mixer_strip(app, ui, target);
            });
        });
    });

    for action in actions {
        apply(app, action);
    }
    app.finish_track_drag(ui, &headers, false);
    let clips: Vec<(ClipRef, Rect)> = slots
        .iter()
        .filter(|&&(t, slot, _)| app.project.tracks.get(t).is_some_and(|track| track.clips[slot].is_some()))
        .map(|&(track, slot, rect)| (ClipRef::Session { track, slot }, rect))
        .collect();
    let mut lanes: Vec<(usize, Rect)> = Vec::new();
    for &(t, _, rect) in &slots {
        match lanes.last_mut() {
            Some((last, column)) if *last == t => *column = column.union(rect),
            _ => lanes.push((t, rect)),
        }
    }
    app.drop_targets.slots = slots.iter().map(|&(t, s, r)| (t, s, r.intersect(ui.clip_rect()))).collect();
    app.update_clip_gesture(ui, &clips, &lanes, &slots);
}

/// How many scenes the Session shows: the ones in use and an empty one after
/// them to fill, at least 8, up to all of them.
pub fn shown_scenes(project: &crate::project::Project) -> usize {
    let used = project.tracks.iter().filter_map(|t| t.clips.iter().rposition(Option::is_some)).max().map_or(0, |last| last + 1);
    (used + 1).clamp(8, NUM_SCENES)
}

fn header(ui: &mut egui::Ui, name: &str, color: Color32, selected: bool, sense: Sense) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(COL_W, HEADER_H), sense);
    widgets::name(&response, egui::WidgetType::Button, format!("Pista {name}"));
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, if selected { color } else { theme::shade(color, 0.72) });
    painter.text(
        rect.left_center() + vec2(6.0, 0.0),
        Align2::LEFT_CENTER,
        name,
        theme::body(),
        theme::text_on(color),
    );
    if selected {
        widgets::outline(painter, rect, theme::SELECT);
    }
    response
}

/// In a group's tracks' headers, a bar of the group's colour for each group they
/// are in.
pub(crate) fn group_marks(app: &PulseApp, painter: &egui::Painter, t: usize, rect: Rect) {
    let mut at = crate::groups::parent(&app.project.tracks, t);
    let mut k = app.project.tracks[t].depth as f32;
    while let Some(g) = at {
        k -= 1.0;
        let bar = Rect::from_min_size(rect.left_top() + vec2(k * 4.0, 0.0), vec2(3.0, rect.height()));
        painter.rect_filled(bar, 0.0, theme::rgb(app.project.tracks[g].color));
        at = crate::groups::parent(&app.project.tracks, g);
    }
}

/// Grouping in a track's menu.
pub(crate) fn group_items(ui: &mut egui::Ui, group: bool, in_group: bool, selected: usize, mut act: impl FnMut(Action), t: usize) {
    ui.separator();
    let label = crate::theme::keys(&if selected > 1 { format!("Agrupar {selected} pistas  ⌘G") } else { "Agrupar pista  ⌘G".to_string() });
    if ui.button(label).on_hover_text("Las pone en un grupo nuevo: suenan juntas por su fader y sus efectos").clicked() {
        act(Action::GroupTracks);
        ui.close();
    }
    if group && ui.button(crate::theme::keys("Desagrupar  ⇧⌘G")).on_hover_text("Quita el grupo; sus pistas se quedan").clicked() {
        act(Action::Ungroup(t));
        ui.close();
    }
    if in_group && ui.button("Sacar del grupo").clicked() {
        act(Action::LeaveGroup(t));
        ui.close();
    }
}

/// A group's slot: lit when its tracks have clips in that scene; clicked, they
/// play them (and the others in the group stop).
fn group_slot(app: &PulseApp, ui: &mut egui::Ui, g: usize, slot: usize, state: Option<&engine::SharedState>, actions: &mut Vec<Action>) {
    let (rect, response) = ui.allocate_exact_size(vec2(COL_W, SLOT_H), Sense::click());
    let tracks = &app.project.tracks;
    let inside = crate::groups::children(tracks, g);
    let with_clips = inside.clone().filter(|&c| tracks[c].clips[slot].is_some()).count();
    let playing = state.is_some_and(|s| inside.clone().any(|c| s.tracks[c].playing.load(Ordering::Relaxed) == slot as i32));
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, theme::shade(SLOT, 0.85));
    if with_clips > 0 {
        let color = theme::rgb(tracks[g].color);
        let bar = Rect::from_min_size(rect.left_top() + vec2(LAUNCH_W, 4.0), vec2(COL_W - LAUNCH_W - 4.0, SLOT_H - 8.0));
        painter.rect_filled(bar, 2.0, theme::shade(color, if playing { 1.0 } else { 0.55 }));
        widgets::play_triangle(painter, pos2(rect.left() + LAUNCH_W / 2.0, rect.center().y), 8.0, if playing { theme::PLAY } else { TEXT });
        if response.on_hover_text(format!("Lanza los {with_clips} clips de sus pistas en esta escena")).clicked() {
            actions.push(Action::LaunchGroup(g, slot));
        }
    }
}

/// Freeze, unfreeze and flatten in a track's menu.
pub(crate) fn freeze_items(ui: &mut egui::Ui, frozen: bool, freezing: bool, mut act: impl FnMut(Action), t: usize) {
    ui.separator();
    if frozen {
        if ui.button("Descongelar pista").on_hover_text("Vuelven a sonar su instrumento y sus efectos").clicked() {
            act(Action::Unfreeze(t));
            ui.close();
        }
        if ui.button("Convertir en audio").on_hover_text("Pasa a ser una pista de audio con lo congelado (su instrumento y efectos se quitan)").clicked() {
            act(Action::Flatten(t));
            ui.close();
        }
    } else if ui
        .add_enabled(!freezing, egui::Button::new("Congelar pista"))
        .on_hover_text("Renderiza su instrumento y efectos a audio y suena eso: deja de gastar CPU. El volumen, el paneo y los envíos siguen en vivo")
        .clicked()
    {
        act(Action::Freeze(t));
        ui.close();
    }
    if ui.button("Crear rumble").on_hover_text("Una pista nueva con esta tocando por reverb, saturación y un filtro de graves, que se aparta cuando suena esta: el rumble del techno a partir del bombo").clicked() {
        act(Action::Rumble(t));
        ui.close();
    }
}

/// «Sacar a su pista ▸», for a Drum Machine track: one of its sounds on a track of its own.
pub(crate) fn sound_items(app: &PulseApp, ui: &mut egui::Ui, mut act: impl FnMut(Action), t: usize) {
    if !matches!(app.project.tracks[t].instrument, engine::model::InstrumentKind::DrumMachine(_)) {
        return;
    }
    let taken = app.sounds_taken(t);
    crate::widgets::menu_button(ui, "Sacar a su pista", |ui| {
        for voice in 0..engine::model::DRUM_SOUNDS {
            let name = crate::project::voice_name(voice);
            let hint = match app.sound_track(t, voice) {
                Some(track) => format!("Ya suena en «{track}»"),
                None => format!("Una pista nueva que toca solo «{name}», con sus efectos y su fader: esta deja de tocarlo y los clips siguen aquí"),
            };
            if ui.add_enabled(taken >> voice & 1 == 0, egui::Button::new(name)).on_hover_text(hint).on_disabled_hover_text("Ya está en su pista").clicked() {
                act(Action::SoundApart(t, voice));
                ui.close();
            }
        }
    });
}

/// A track's header: clic to select (⌘ and ⇧ for several), drag to move. Returns where it is.
fn track_header(app: &mut PulseApp, ui: &mut egui::Ui, t: usize, actions: &mut Vec<Action>) -> egui::Rect {
    let track = &app.project.tracks[t];
    let group = track.is_group();
    let name = if group { format!("{} {}", if track.folded { "▸" } else { "▾" }, track.name) } else { track.name.clone() };
    let hint = if group {
        "Grupo: sus pistas suenan por su fader y sus efectos · clic en ▾: plegar o desplegar · clic: seleccionar · arrastrar: mover con sus pistas"
    } else {
        "Clic: seleccionar · ⌘ clic: añadir o quitar · ⇧ clic: rango · arrastrar: mover"
    };
    let hint = match app.follows_hint(t) {
        Some(follows) => format!("{follows} · {hint}"),
        None => hint.to_string(),
    };
    let response = header(ui, &name, theme::rgb(track.color), app.is_track_selected(t), Sense::click_and_drag()).on_hover_text(crate::theme::keys(&hint));
    group_marks(app, ui.painter(), t, response.rect);
    // The fold arrow.
    let on_arrow = response.interact_pointer_pos().is_some_and(|p| p.x < response.rect.left() + 18.0);
    if group && response.clicked() && on_arrow {
        actions.push(Action::ToggleFold(t));
    }
    app.track_header_input(t, &response);
    let in_group = crate::groups::parent(&app.project.tracks, t).is_some();
    let several = app.is_track_selected(t) && app.track_selection().len() > 1;
    let count = app.track_selection().len();
    let (frozen, freezing) = (app.is_frozen(t), app.freeze_job.is_some());
    widgets::edit_menu(&response, |ui| {
        let track = &mut app.project.tracks[t];
        ui.label("Nombre");
        ui.text_edit_singleline(&mut track.name);
        ui.label("Color");
        color_picker(ui, &mut track.color);
        ui.separator();
        let label = |verb: &str| if several { format!("{verb} {count} pistas") } else { format!("{verb} pista") };
        if ui.button(label("Duplicar")).clicked() {
            actions.push(Action::DuplicateTrack(t));
            ui.close();
        }
        if ui.button(label("Eliminar")).clicked() {
            actions.push(Action::DeleteTrack(t));
            ui.close();
        }
        group_items(ui, group, in_group, count, |action| actions.push(action), t);
        if !group {
            freeze_items(ui, frozen, freezing, |action| actions.push(action), t);
            sound_items(app, ui, |action| actions.push(action), t);
        }
    });
    if frozen {
        crate::freeze::mark(ui.painter(), response.rect);
    }
    response.rect
}

pub(crate) fn color_picker(ui: &mut egui::Ui, color: &mut [u8; 3]) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        ui.set_max_width(6.0 * 18.0);
        for c in PALETTE {
            let (rect, response) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
            ui.painter().rect_filled(rect, 2.0, theme::rgb(c));
            if *color == c {
                widgets::outline(ui.painter(), rect, Color32::WHITE);
            }
            if response.clicked() {
                *color = c;
                changed = true;
            }
        }
    });
    changed
}

fn clip_slot(
    app: &PulseApp,
    ui: &mut egui::Ui,
    t: usize,
    slot: usize,
    play: &TrackPlayState,
    blink: bool,
    actions: &mut Vec<Action>,
) -> Rect {
    let (rect, response) = ui.allocate_exact_size(vec2(COL_W, SLOT_H), Sense::click_and_drag());
    let painter = ui.painter();
    let launch = Rect::from_min_size(rect.min, vec2(LAUNCH_W, SLOT_H));
    let clip = app.project.tracks[t].clips[slot].as_ref();
    widgets::name(&response, egui::WidgetType::Button, match clip {
        Some(c) => format!("Clip {} · {}", t + 1, c.name()),
        None => format!("Slot {}:{}", t + 1, slot + 1),
    });
    let selected = app.is_clip_selected(ClipRef::Session { track: t, slot });
    let on_launch = response.hover_pos().is_some_and(|p| launch.contains(p));
    let is_playing = play.playing == slot as i32;
    let record_queued = play.queued_record && play.queued == slot as i32;
    let is_queued = play.queued == slot as i32 && !record_queued;
    let is_recording = play.recording == slot as i32;
    let armed = app.project.tracks[t].armed;

    match clip {
        Some(clip) => {
            // Switched off: grey.
            let color = if clip.muted() { Color32::from_gray(90) } else { theme::rgb(clip.color()) };
            painter.rect_filled(rect, 2.0, if response.hovered() { color } else { theme::shade(color, 0.88) });
            let icon = if is_playing || (is_queued && blink) {
                PLAY
            } else if on_launch {
                Color32::WHITE
            } else {
                theme::shade(theme::text_on(color), 0.9)
            };
            if is_playing {
                painter.rect_filled(launch.shrink(2.0), 2.0, theme::shade(color, 0.5));
            }
            widgets::play_triangle(painter, launch.center(), 9.0, icon);
            painter.text(
                pos2(launch.right() + 2.0, rect.center().y),
                Align2::LEFT_CENTER,
                clip.name(),
                theme::small(),
                theme::text_on(color),
            );
            if is_playing && clip.length() > 0.0 {
                let progress = (play.clip_position / clip.length()) as f32;
                let bar = Rect::from_min_size(pos2(rect.left(), rect.bottom() - 2.0), vec2(rect.width() * progress, 2.0));
                painter.rect_filled(bar, 0.0, theme::shade(theme::text_on(color), 0.9));
            }
        }
        None if is_recording => {
            painter.rect_filled(rect, 2.0, theme::shade(theme::METER_RED, 0.75));
            painter.circle_filled(launch.center(), 5.0, Color32::WHITE);
            let bar = play.recorded_bar;
            painter.text(
                pos2(launch.right() + 2.0, rect.center().y),
                Align2::LEFT_CENTER,
                format!("Grabando · compás {bar}"),
                theme::small(),
                Color32::WHITE,
            );
        }
        None if armed => {
            painter.rect_filled(rect, 2.0, if response.hovered() { SLOT_HOVER } else { SLOT });
            let lit = (record_queued && blink) || on_launch;
            let color = if lit { theme::METER_RED } else { theme::shade(theme::METER_RED, 0.55) };
            painter.circle_filled(launch.center(), 4.5, color);
        }
        None => {
            painter.rect_filled(rect, 2.0, if response.hovered() { SLOT_HOVER } else { SLOT });
            let stop = Rect::from_center_size(launch.center(), vec2(6.0, 6.0));
            painter.rect_filled(stop, 1.0, if on_launch { TEXT } else { theme::CONTROL_HOVER });
        }
    }
    if selected {
        widgets::outline(painter, rect, theme::SELECT);
    }

    let clicked_launch = response.interact_pointer_pos().is_some_and(|p| launch.contains(p));
    let modifiers = ui.input(|i| i.modifiers);
    if response.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(rect.center());
        if clip.is_some() && !launch.contains(origin) {
            actions.push(Action::StartMove(t, slot, modifiers));
        } else if clip.is_none() {
            actions.push(Action::StartMarquee(origin, modifiers));
        }
    }
    if (is_recording || record_queued) && response.clicked() {
        actions.push(Action::StopRecording(t));
    } else if response.double_clicked() && clip.is_none() {
        actions.push(Action::Create(t, slot));
    } else if response.clicked() {
        if clicked_launch && clip.is_none() && armed {
            actions.push(Action::Record(t, slot));
        } else if clicked_launch {
            actions.push(Action::Launch(t, slot));
        } else if clip.is_some() {
            actions.push(Action::ClickClip(ClipRef::Session { track: t, slot }, modifiers));
        } else {
            actions.push(Action::SelectSlot(t, slot));
        }
    }

    let hint = match (clip.is_some(), app.project.tracks[t].is_audio()) {
        (true, _) => "► lanza · clic: editar · ⌘ clic: añadir a la selección · arrastrar: mover (⌥ copia) · clic derecho: más opciones",
        (false, false) if armed => "● graba un clip nuevo · doble clic: clip MIDI vacío",
        (false, false) => "Doble clic: nuevo clip MIDI · arrastrar: seleccionar con un recuadro",
        (false, true) => "Doble clic: elegir una muestra (o arrastra archivos de audio a la ventana)",
    };
    let response = response.on_hover_text(crate::theme::keys(hint));
    // Drums have no chords to accompany.
    let app_drums = app.project.tracks[t].is_drums();
    let hearing = app.hum_song.is_some();
    if let Some(clip) = clip {
        let mut color = clip.color();
        response.context_menu(|ui| {
            if ui.button("Lanzar").clicked() {
                actions.push(Action::Launch(t, slot));
                ui.close();
            }
            if ui.button("Copiar al Arrangement").clicked() {
                actions.push(Action::ToArrangement(t, slot));
                ui.close();
            }
            if matches!(clip, Clip::Audio(_)) && ui.button("Separar en stems…").on_hover_text("Batería, bajo, resto y voz, cada uno en una pista nueva").clicked() {
                actions.push(Action::SeparateStems(t, slot));
                ui.close();
            }
            if matches!(clip, Clip::Midi(_)) && ui.button("Exportar MIDI…").on_hover_text("Sus notas a un archivo .mid, para llevarlas a otro programa").clicked() {
                actions.push(Action::ExportMidi(t, slot));
                ui.close();
            }
            if matches!(clip, Clip::Audio(_)) && ui.button("Sacar groove").on_hover_text(crate::arrangement::GROOVE_HINT).clicked() {
                actions.push(Action::TakeGroove(t, slot));
                ui.close();
            }
            if matches!(clip, Clip::Audio(_)) {
                crate::widgets::menu_button(ui, "Convertir a MIDI", |ui| {
                    for mode in crate::to_midi::ToMidi::ALL {
                        if ui.button(mode.label()).on_hover_text(mode.hint()).clicked() {
                            actions.push(Action::ToMidi(t, slot, mode));
                            ui.close();
                        }
                    }
                });
            }
            if !app_drums {
                crate::widgets::menu_button(ui, "Acompañar", |ui| {
                    for part in crate::accompany::Part::ALL {
                        if ui.button(part.label()).on_hover_text(part.hint()).clicked() {
                            actions.push(Action::Accompany(t, slot, part));
                            ui.close();
                        }
                    }
                });
                crate::widgets::menu_button(ui, "De tarareo a canción", |ui| {
                    ui.label(egui::RichText::new("La melodía, sus acordes, un bajo y una batería, en pistas nuevas").color(crate::theme::TEXT_DIM).size(10.5));
                    for style in crate::drum_gen::Style::ALL {
                        if ui.add_enabled(!hearing, egui::Button::new(style.label())).on_hover_text("Saca la melodía que tarareas, cantas o silbas (con el metrónomo, al tempo del proyecto), la ajusta a la rejilla, le pone acordes de su escala y añade bajo y batería de este estilo").clicked() {
                            actions.push(Action::HumSong(t, slot, style));
                            ui.close();
                        }
                    }
                });
            }
            if ui.button(if clip.muted() { "Activar clip   0" } else { "Desactivar clip   0" }).on_hover_text("Desactivado, el clip se queda donde está pero no suena").clicked() {
                actions.push(Action::ToggleClipMuted(t, slot));
                ui.close();
            }
            if matches!(clip, Clip::Audio(_)) && ui.button("Consolidar").on_hover_text("Lo que suena el clip, a un archivo de audio nuevo solo con su trozo (con su warp, transposición y ganancia)").clicked() {
                actions.push(Action::Consolidate(t, slot));
            }
            if matches!(clip, Clip::Audio(_)) && ui.button("Invertir").on_hover_text("Lo que suena el clip, del final al principio, en un archivo nuevo (otra vez lo devuelve)").clicked() {
                actions.push(Action::Reverse(t, slot));
                ui.close();
            }
            if matches!(clip, Clip::Audio(_)) {
                if ui.button("Detectar tempo").on_hover_text("Busca el tempo y los compases del audio para que siga el del proyecto").clicked() {
                    actions.push(Action::DetectTempo(t, slot));
                    ui.close();
                }
                if ui.button("Usar el tempo de este clip en el proyecto").clicked() {
                    actions.push(Action::UseClipTempo(t, slot));
                    ui.close();
                }
            }
            let several = app.is_clip_selected(ClipRef::Session { track: t, slot }) && app.clip_selection().len() > 1;
            let count = app.clip_selection().len();
            if ui.button(crate::theme::keys(&if several { format!("Duplicar {count} clips   ⌘D") } else { "Duplicar   ⌘D".into() })).clicked() {
                actions.push(Action::Duplicate(t, slot));
                ui.close();
            }
            if ui.button(if several { format!("Eliminar {count} clips   Supr") } else { "Eliminar   Supr".into() }).clicked() {
                actions.push(Action::Delete(t, slot));
                ui.close();
            }
            ui.separator();
            if color_picker(ui, &mut color) {
                actions.push(Action::SetClipColor(t, slot, color));
            }
        });
    }
    rect
}

fn stop_cell(ui: &mut egui::Ui, highlighted: bool, hint: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(COL_W, SLOT_H), Sense::click());
    widgets::name(&response, egui::WidgetType::Button, hint);
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, theme::shade(SLOT, 0.85));
    let square = Rect::from_center_size(rect.center(), vec2(9.0, 9.0));
    let fill = if highlighted {
        ACCENT
    } else if response.hovered() {
        TEXT
    } else {
        TEXT_DIM
    };
    painter.rect_filled(square, 1.0, fill);
    response.on_hover_text(hint)
}

fn scene_cell(app: &mut PulseApp, ui: &mut egui::Ui, slot: usize, actions: &mut Vec<Action>) {
    let (rect, response) = ui.allocate_exact_size(vec2(COL_W, SLOT_H), Sense::click());
    widgets::name(&response, egui::WidgetType::Button, format!("Escena {}", slot + 1));
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, if response.hovered() { theme::CONTROL_HOVER } else { theme::CONTROL });
    let launch = Rect::from_min_size(rect.min, vec2(LAUNCH_W, SLOT_H));
    widgets::play_triangle(painter, launch.center(), 9.0, if response.hovered() { Color32::WHITE } else { TEXT });
    painter.text(
        pos2(launch.right() + 2.0, rect.center().y),
        Align2::LEFT_CENTER,
        &app.project.scene_names[slot],
        theme::small(),
        TEXT,
    );
    if response.clicked() {
        actions.push(Action::LaunchScene(slot));
    }
    widgets::edit_menu(&response.on_hover_text("Lanzar escena · clic derecho: renombrar"), |ui| {
        ui.label("Nombre de la escena");
        ui.text_edit_singleline(&mut app.project.scene_names[slot]);
    });
}

fn mixer_strip(app: &mut PulseApp, ui: &mut egui::Ui, target: Target) {
    let monitoring_input = matches!(target, Target::Track(t) if app.project.tracks[t].armed && app.project.tracks[t].is_audio());
    let peak = match (&app.capture, monitoring_input) {
        (Some(capture), true) => capture.level,
        _ => app.meters.of(target),
    };
    let mut commands = Vec::new();
    let mut arm = None;

    // When an audio track plays its input.
    // The same height on every strip, so the faders line up.
    let (row, _) = ui.allocate_exact_size(vec2(COL_W, MONITOR_H), Sense::hover());
    if let Target::Track(t) = target {
        if app.project.tracks[t].is_audio() {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::left_to_right(egui::Align::Center)));
            child.spacing_mut().item_spacing.x = 2.0;
            let current = app.project.tracks[t].monitor;
            for mode in crate::project::Monitor::ALL {
                if widgets::toggle(&mut child, current == mode, mode.label(), theme::MONITOR, vec2((COL_W - 4.0) / 3.0, MONITOR_H))
                    .on_hover_text(mode.hint())
                    .clicked()
                {
                    app.set_monitor(t, mode);
                }
            }
        }
    }

    // Sends, pan and fader go through the parameter helpers so they follow automation.
    let mut changes: Vec<(Param, f32)> = Vec::new();
    match target {
        Target::Track(_) => {
            let returns = app.project.returns.len();
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.set_height(sends_h(returns));
                for row in 0..returns.div_ceil(2) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.set_height(SENDS_ROW_H);
                        for send in row * 2..(row * 2 + 2).min(returns) {
                            let param = Param::Send(send);
                            let value = app.display_value(target, param);
                            let state = app.automation_state(target, param);
                            let letter = crate::project::return_letter(send).to_string();
                            if let Some(v) = widgets::param_knob(ui, &SEND_PARAM, value, &letter, 22.0, state) {
                                changes.push((param, v));
                            }
                        }
                    });
                }
            });
        }
        _ => {
            ui.add_space(sends_h(app.project.returns.len()) + 1.0);
        }
    }

    ui.horizontal(|ui| {
        ui.set_height(PAN_H);
        ui.add_space((COL_W - 46.0) / 2.0);
        let value = app.display_value(target, Param::Pan);
        let state = app.automation_state(target, Param::Pan);
        if let Some(v) = widgets::param_knob(ui, &PAN_PARAM, value, "Pan", 26.0, state) {
            changes.push((Param::Pan, v));
        }
    });

    ui.horizontal(|ui| {
        ui.add_space((COL_W - 34.0) / 2.0);
        let mut gain = app.display_value(target, Param::Volume);
        let response = widgets::fader_with_meter(ui, &mut gain, peak, FADER_H);
        widgets::automation_dot(ui.painter(), response.rect.left_top() + vec2(2.0, 2.0), app.automation_state(target, Param::Volume));
        if response.changed() {
            changes.push((Param::Volume, gain));
        }
    });
    for (param, value) in changes {
        app.set_param(target, param, value);
    }

    // Activator and solo.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let label = match target {
            Target::Track(t) => (t + 1).to_string(),
            Target::Return(r) => crate::project::return_letter(r).to_string(),
            Target::Master => String::new(),
        };
        if target == Target::Master {
            ui.add_space(18.0);
            return;
        }
        let active = app.mixer(target).active;
        let width = if matches!(target, Target::Track(_)) { (COL_W - 4.0) / 3.0 } else { COL_W };
        if widgets::toggle(ui, active, &label, ACCENT, vec2(width, 18.0))
            .on_hover_text("Activador de pista (silenciar)")
            .clicked()
        {
            let mixer = app.mixer_mut(target);
            mixer.active = !mixer.active;
            commands.push(Command::SetMute { target, mute: !mixer.active });
        }
        if let Target::Track(t) = target {
            let solo = app.project.tracks[t].solo;
            if widgets::toggle(ui, solo, "S", SOLO, vec2(width, 18.0)).on_hover_text("Solo").clicked() {
                app.project.tracks[t].solo = !solo;
                commands.push(Command::SetSolo { track: t, solo: !solo });
            }
            let armed = app.project.tracks[t].armed;
            if widgets::toggle(ui, armed, "●", theme::METER_RED, vec2(width, 18.0))
                .on_hover_text("Armar para grabar: recibe la entrada MIDI y graba")
                .clicked()
            {
                arm = Some(t);
            }
        }
    });

    for command in commands {
        app.audio.send(command);
    }
    if let Some(t) = arm {
        app.toggle_arm(t);
    }
}

fn apply(app: &mut PulseApp, action: Action) {
    match action {
        Action::Launch(t, slot) => {
            if app.project.tracks[t].clips[slot].is_some() {
                app.selected_clip = Some(ClipRef::Session { track: t, slot });
            }
            app.selected_target = Target::Track(t);
            app.audio.send(Command::LaunchClip { track: t, slot });
        }
        Action::ClickClip(clip, modifiers) => app.click_clip(clip, modifiers),
        Action::StartMove(t, slot, modifiers) => app.start_session_move(t, slot, modifiers),
        Action::StartMarquee(anchor, modifiers) => app.start_marquee(anchor, modifiers),
        Action::SelectTarget(target) => app.selected_target = target,
        Action::Freeze(t) => app.freeze(t),
        Action::Rumble(t) => app.make_rumble(t),
        Action::SoundApart(t, voice) => app.sound_apart(t, voice),
        Action::Unfreeze(t) => app.unfreeze(t),
        Action::Flatten(t) => app.flatten(t),
        Action::Consolidate(t, slot) => app.consolidate(vec![ClipRef::Session { track: t, slot }]),
        Action::Reverse(t, slot) => app.reverse_clips(vec![ClipRef::Session { track: t, slot }]),
        Action::ToggleClipMuted(t, slot) => {
            let clip = ClipRef::Session { track: t, slot };
            if !app.is_clip_selected(clip) {
                app.set_clip_selection(vec![clip]);
            }
            app.toggle_clips_muted();
        }
        Action::ToMidi(t, slot, mode) => app.convert_to_midi(ClipRef::Session { track: t, slot }, mode),
        Action::TakeGroove(t, slot) => app.take_groove(ClipRef::Session { track: t, slot }),
        Action::ExportMidi(t, slot) => app.export_clip_midi(ClipRef::Session { track: t, slot }),
        Action::Accompany(t, slot, part) => app.accompany(ClipRef::Session { track: t, slot }, part),
        Action::HumSong(t, slot, style) => app.hum_to_song(ClipRef::Session { track: t, slot }, style),
        Action::SelectSlot(t, slot) => {
            app.set_clip_selection(Vec::new());
            app.selected_target = Target::Track(t);
            app.paste_slot = Some((t, slot));
        }
        Action::Create(t, slot) => {
            if app.project.tracks[t].is_audio() {
                app.choose_samples(Placement::Session { track: t, slot });
            } else {
                let clip = app.new_midi_clip(t, None);
                app.set_clip(t, slot, Some(Clip::Midi(clip)));
                app.select_clip(ClipRef::Session { track: t, slot });
            }
        }
        Action::Delete(t, slot) if app.is_clip_selected(ClipRef::Session { track: t, slot }) => app.delete_selected_clip(),
        Action::Delete(t, slot) => app.set_clip(t, slot, None),
        Action::Duplicate(t, slot) => {
            app.selected_clip = Some(ClipRef::Session { track: t, slot });
            app.duplicate_selected_clip();
        }
        Action::ToArrangement(t, slot) => app.copy_to_arrangement(t, slot),
        Action::SetClipColor(t, slot, color) => {
            if let Some(clip) = app.project.tracks[t].clips[slot].as_mut() {
                *clip.color_mut() = color;
                app.sync_clip(t, slot);
            }
        }
        Action::StopTrack(t) => app.audio.send(Command::StopTrack(t)),
        Action::LaunchGroup(g, slot) => app.launch_group(g, slot),
        Action::StopGroup(g) => {
            for t in crate::groups::children(&app.project.tracks, g) {
                app.audio.send(Command::StopTrack(t));
            }
        }
        Action::ToggleFold(g) => app.toggle_fold(g),
        Action::GroupTracks => app.group_selected(),
        Action::Ungroup(g) => app.ungroup(g),
        Action::LeaveGroup(t) => app.leave_group(t),
        Action::LaunchScene(slot) => app.audio.send(Command::LaunchScene(slot)),
        Action::StopAll => app.audio.send(Command::StopAllClips),
        Action::AddTrack => app.add_track(),
        Action::AddReturn => app.add_return(),
        Action::DeleteReturn(r) => app.delete_return(r),
        Action::DeleteTrack(t) if app.is_track_selected(t) => app.delete_selected_tracks(),
        Action::DeleteTrack(t) => app.delete_track(t),
        Action::Record(t, slot) => app.record_slot(t, slot),
        Action::StopRecording(t) => app.stop_session_recording(t),
        Action::DuplicateTrack(t) if app.is_track_selected(t) => app.duplicate_selected_tracks(),
        Action::DuplicateTrack(t) => app.duplicate_track(t),
        Action::SeparateStems(t, slot) => app.separate_stems(ClipRef::Session { track: t, slot }),
        Action::DetectTempo(t, slot) => app.detect_tempo(vec![ClipRef::Session { track: t, slot }], true),
        Action::UseClipTempo(t, slot) => app.use_clip_tempo(ClipRef::Session { track: t, slot }),
    }
}

#[cfg(test)]
mod scene_tests {
    use super::*;
    use crate::project::Project;

    /// Projects from when there were 8 scenes open with 16, their clips and
    /// names where they were and the rest empty.
    #[test]
    fn projects_with_eight_scenes_open_with_sixteen() {
        for fixture in [include_str!("../fixtures/format0-demo.pulse"), include_str!("../fixtures/format1-demo.pulse")] {
            let saved: serde_json::Value = serde_json::from_str(fixture).unwrap();
            assert_eq!(saved["tracks"][0]["clips"].as_array().unwrap().len(), 8);
            let project = Project::from_json(fixture).unwrap();
            assert_eq!(project.scene_names.len(), NUM_SCENES);
            assert_eq!(project.scene_names[15], "16");
            for (t, track) in project.tracks.iter().enumerate() {
                for slot in 0..8 {
                    assert_eq!(track.clips[slot].is_some(), !saved["tracks"][t]["clips"][slot].is_null());
                }
                assert!(track.clips[8..].iter().all(Option::is_none));
            }
            // Saved again and opened: the same.
            assert_eq!(Project::from_json(&project.to_json().unwrap()).unwrap(), project);
        }
    }

    /// The Session shows the scenes in use and one more, 8 at least.
    #[test]
    fn the_session_shows_the_scenes_in_use_and_one_more() {
        let mut project = Project::demo();
        assert_eq!(shown_scenes(&project), 8);
        project.tracks[0].clips[10] = project.tracks[0].clips[0].clone();
        assert_eq!(shown_scenes(&project), 12);
        project.tracks[1].clips[15] = project.tracks[1].clips[0].clone();
        assert_eq!(shown_scenes(&project), NUM_SCENES);
    }

    /// A clip in the 13th scene plays when that scene is launched.
    #[test]
    fn a_high_scene_launches() {
        let mut app = PulseApp::headless(Project::demo());
        let clip = app.project.tracks[1].clips[0].clone();
        app.set_clip(1, 12, clip);
        let mut renderer = engine::OfflineRenderer::new(48_000.0);
        for command in crate::sync::project_commands(&app.project, &mut crate::sync::SampleCache::default(), 48_000.0, &Default::default()) {
            renderer.send(command);
        }
        renderer.send(Command::LaunchScene(12));
        let (mut l, mut r) = (Vec::new(), Vec::new());
        renderer.render(48_000, &mut l, &mut r);
        assert_eq!(renderer.state.tracks[1].playing.load(std::sync::atomic::Ordering::Relaxed), 12);
        assert!(crate::export::peak(&l, &r) > 0.01);
    }
}
