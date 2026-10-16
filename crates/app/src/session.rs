//! Session view: clip slots, scene launchers in the master column, and the mixer.

use crate::app::{ClipRef, Placement, PulseApp};
use crate::project::PALETTE;
use crate::theme::{self, ACCENT, PLAY, SLOT, SLOT_HOVER, SOLO, TEXT, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense};
use engine::model::{Clip, MidiClip, Param, PAN_PARAM, SEND_PARAM};
use engine::{Command, Target, NONE, NUM_RETURNS, NUM_SCENES, QUEUED_STOP};
use std::sync::atomic::Ordering;

const COL_W: f32 = 108.0;
const HEADER_H: f32 = 22.0;
const SLOT_H: f32 = 22.0;
const LAUNCH_W: f32 = 18.0;
const SENDS_H: f32 = 56.0;
const PAN_H: f32 = 58.0;
const FADER_H: f32 = 120.0;
const MONITOR_H: f32 = 16.0;

pub(crate) enum Action {
    Launch(usize, usize),
    ClickClip(ClipRef, egui::Modifiers),
    StartMove(usize, usize, egui::Modifiers),
    StartMarquee(egui::Pos2, egui::Modifiers),
    SelectTarget(Target),
    Consolidate(usize, usize),
    Freeze(usize),
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
    /// Beats recorded so far in the current take.
    recorded_beats: f64,
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let state = app.audio.state();
    let blink = (ui.input(|i| i.time) * 3.0).fract() < 0.5;
    let mut actions = Vec::new();
    let mut headers = Vec::new();
    let mut slots = Vec::new();

    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
            for t in 0..app.project.tracks.len() {
                let play = state.as_ref().map_or(
                    TrackPlayState {
                        playing: NONE,
                        queued: NONE,
                        clip_position: 0.0,
                        recording: NONE,
                        queued_record: false,
                        recorded_beats: 0.0,
                    },
                    |s| {
                        let status = &s.tracks[t];
                        TrackPlayState {
                            playing: status.playing.load(Ordering::Relaxed),
                            queued: status.queued.load(Ordering::Relaxed),
                            clip_position: status.clip_position(),
                            recording: status.recording.load(Ordering::Relaxed),
                            queued_record: status.queued_record.load(Ordering::Relaxed),
                            recorded_beats: (s.position() - status.record_start()).max(0.0),
                        }
                    },
                );
                ui.vertical(|ui| {
                    ui.set_width(COL_W);
                    headers.push(track_header(app, ui, t, &mut actions));
                    for slot in 0..NUM_SCENES {
                        slots.push((t, slot, clip_slot(app, ui, t, slot, &play, blink, &mut actions)));
                    }
                    let stop_queued = play.queued == QUEUED_STOP && blink;
                    if stop_cell(ui, stop_queued, "Parar pista").clicked() {
                        actions.push(Action::StopTrack(t));
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
                if response.on_hover_text("Nueva pista MIDI (⌘T)").clicked() {
                    actions.push(Action::AddTrack);
                }
            });
            ui.add_space(10.0);

            for r in 0..NUM_RETURNS {
                ui.vertical(|ui| {
                    ui.set_width(COL_W);
                    let target = Target::Return(r);
                    let name = app.project.returns[r].name.clone();
                    let color = theme::rgb(app.project.returns[r].color);
                    if header(ui, &name, color, app.selected_target == target, Sense::click()).clicked() {
                        actions.push(Action::SelectTarget(target));
                    }
                    let (rect, _) = ui.allocate_exact_size(vec2(COL_W, (SLOT_H + 1.0) * (NUM_SCENES as f32 + 1.0) - 1.0), Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, theme::shade(SLOT, 0.8));
                    ui.add_space(6.0);
                    mixer_strip(app, ui, target);
                });
            }
            ui.add_space(10.0);

            ui.vertical(|ui| {
                ui.set_width(COL_W);
                let target = Target::Master;
                if header(ui, "Master", theme::CONTROL_HOVER, app.selected_target == target, Sense::click()).clicked() {
                    actions.push(Action::SelectTarget(target));
                }
                for slot in 0..NUM_SCENES {
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
}

/// A track's header: clic to select (⌘ and ⇧ for several), drag to move. Returns where it is.
fn track_header(app: &mut PulseApp, ui: &mut egui::Ui, t: usize, actions: &mut Vec<Action>) -> egui::Rect {
    let track = &app.project.tracks[t];
    let response = header(ui, &track.name, theme::rgb(track.color), app.is_track_selected(t), Sense::click_and_drag())
        .on_hover_text("Clic: seleccionar · ⌘ clic: añadir o quitar · ⇧ clic: rango · arrastrar: mover");
    app.track_header_input(t, &response);
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
        freeze_items(ui, frozen, freezing, |action| actions.push(action), t);
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
            let color = theme::rgb(clip.color());
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
            let bar = (play.recorded_beats / 4.0).floor() as i64 + 1;
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
        (true, _) => "▶ lanza · clic: editar · ⌘ clic: añadir a la selección · arrastrar: mover (⌥ copia) · clic derecho: más opciones",
        (false, false) if armed => "● graba un clip nuevo · doble clic: clip MIDI vacío",
        (false, false) => "Doble clic: nuevo clip MIDI · arrastrar: seleccionar con un recuadro",
        (false, true) => "Doble clic: elegir una muestra (o arrastra archivos de audio a la ventana)",
    };
    let response = response.on_hover_text(hint);
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
            if matches!(clip, Clip::Audio(_)) && ui.button("Consolidar").on_hover_text("Lo que suena el clip, a un archivo de audio nuevo solo con su trozo (con su warp, transposición y ganancia)").clicked() {
                actions.push(Action::Consolidate(t, slot));
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
            if ui.button(if several { format!("Duplicar {count} clips   ⌘D") } else { "Duplicar   ⌘D".into() }).clicked() {
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
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.set_height(SENDS_H);
                for send in 0..NUM_RETURNS {
                    let param = Param::Send(send);
                    let value = app.display_value(target, param);
                    let state = app.automation_state(target, param);
                    if let Some(v) = widgets::param_knob(ui, &SEND_PARAM, value, ["A", "B"][send], 22.0, state) {
                        changes.push((param, v));
                    }
                }
            });
        }
        _ => {
            ui.add_space(SENDS_H + 1.0);
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
            Target::Return(r) => ["A", "B"][r].to_string(),
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
        Action::Unfreeze(t) => app.unfreeze(t),
        Action::Flatten(t) => app.flatten(t),
        Action::Consolidate(t, slot) => app.consolidate(vec![ClipRef::Session { track: t, slot }]),
        Action::SelectSlot(t, slot) => {
            app.set_clip_selection(Vec::new());
            app.selected_target = Target::Track(t);
            app.paste_slot = Some((t, slot));
        }
        Action::Create(t, slot) => {
            if app.project.tracks[t].is_audio() {
                app.choose_samples(Placement::Session { track: t, slot });
            } else {
                let color = app.project.tracks[t].color;
                app.set_clip(t, slot, Some(Clip::Midi(MidiClip::new("MIDI", 4.0, color))));
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
        Action::LaunchScene(slot) => app.audio.send(Command::LaunchScene(slot)),
        Action::StopAll => app.audio.send(Command::StopAllClips),
        Action::AddTrack => app.add_track(),
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
