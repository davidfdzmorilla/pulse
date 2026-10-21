//! The lane under the note editor shows either the notes' velocities or one of the
//! clip's envelopes (its own automation, in clip time, looping with it). Here:
//! choosing what it shows, and drawing and editing an envelope: a clic adds a
//! point, dragging moves it, a double clic deletes it; ⌥ + drag on a stretch
//! bends it (see `curves`).

use crate::theme::{self, TEXT_DIM};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine::model::{AutomationLane, AutomationPoint, Param, ParamInfo};

const POINT_GRAB: f32 = 6.0;

/// What the lane under the notes shows, and the names it offers.
#[derive(Default)]
pub struct LaneChoice {
    /// An envelope's parameter, or the velocities.
    pub shown: Option<Param>,
    /// The track's parameters and their names, kept until the track's list changes.
    names: Vec<(Param, String)>,
    names_for: Option<(usize, usize)>,
    /// The point being dragged.
    dragging: Option<usize>,
    /// The stretch being bent: the point it starts at, its bend and the
    /// pointer's height when the drag began.
    bending: Option<(usize, f32, f32)>,
}

impl LaneChoice {
    /// Whether the names must be read again for track `track` with `count` parameters.
    pub fn stale(&self, track: usize, count: usize) -> bool {
        self.names_for != Some((track, count))
    }

    /// Keeps the parameter names of track `track` (new ones when they were stale).
    pub fn refresh(&mut self, track: usize, params: &[Param], names: Option<Vec<(Param, String)>>) {
        if let Some(names) = names {
            self.names = names;
            self.names_for = Some((track, params.len()));
        }
        if self.shown.is_some_and(|p| !params.contains(&p)) {
            self.shown = None;
        }
    }

    fn name(&self, param: Param) -> &str {
        self.names.iter().find(|(p, _)| *p == param).map_or("?", |(_, n)| n.as_str())
    }
}

/// The row above the lane: what it shows, and deleting the envelope shown.
pub fn header(ui: &mut egui::Ui, choice: &mut LaneChoice, envelopes: &mut Vec<AutomationLane>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Carril").color(TEXT_DIM).size(10.5));
        let has = |p: Param| envelopes.iter().any(|l| l.param == p && !l.points.is_empty());
        let label = |p: Option<Param>| match p {
            None => "Velocidad".to_string(),
            Some(p) => format!("{}{}", if has(p) { "● " } else { "" }, choice.name(p)),
        };
        let mut shown = choice.shown;
        egui::ComboBox::from_id_salt("clip_lane").selected_text(label(shown)).width(220.0).height(360.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut shown, None, "Velocidad");
            ui.separator();
            ui.label(egui::RichText::new("Envolventes del clip").color(TEXT_DIM).size(10.5));
            // Those with points first.
            let mut params: Vec<Param> = choice.names.iter().map(|(p, _)| *p).collect();
            params.sort_by_key(|&p| !has(p));
            for p in params {
                ui.selectable_value(&mut shown, Some(p), label(Some(p)));
            }
        });
        choice.shown = shown;
        if let Some(param) = shown.filter(|&p| has(p)) {
            if ui.small_button("Borrar envolvente").clicked() {
                envelopes.retain(|l| l.param != param);
            }
        }
        let count = envelopes.iter().filter(|l| !l.points.is_empty()).count();
        if count > 0 {
            ui.label(egui::RichText::new(format!("{count} envolvente{} en el clip", if count == 1 { "" } else { "s" })).color(TEXT_DIM).size(10.5));
        }
    });
}

/// Draws envelope `param` of a clip `length` beats long in `lane` and edits it. `x_of` places a clip
/// beat; `grid` is the snap in beats; `value` is the parameter's own value, shown
/// where there are no points.
#[allow(clippy::too_many_arguments)]
pub fn lane(
    ui: &mut egui::Ui,
    choice: &mut LaneChoice,
    envelopes: &mut Vec<AutomationLane>,
    length: f64,
    param: Param,
    info: &ParamInfo,
    value: f32,
    lane: Rect,
    area_left: f32,
    x_of: &dyn Fn(f64) -> f32,
    beat_of: &dyn Fn(f32) -> f64,
    grid: f64,
    color: Color32,
) -> egui::Response {
    let response = ui.interact(lane, ui.id().with("clip_envelope"), Sense::click_and_drag());
    let painter = ui.painter_at(lane);
    painter.rect_filled(lane, 2.0, theme::SLOT);
    let area = Rect::from_min_max(pos2(area_left, lane.top() + 4.0), lane.max - vec2(0.0, 4.0));
    let y_of = |v: f32| area.bottom() - area.height() * info.normalize(v);
    let value_of = |y: f32| info.denormalize((area.bottom() - y) / area.height());
    // The clip's end: past it the envelope loops.
    let end = x_of(length);
    painter.rect_filled(Rect::from_min_max(pos2(end, lane.top()), lane.max), 0.0, Color32::from_black_alpha(80));

    let index = envelopes.iter().position(|l| l.param == param);
    let points: Vec<AutomationPoint> = index.map(|i| envelopes[i].points.clone()).unwrap_or_default();
    if points.is_empty() {
        let y = y_of(value);
        painter.line_segment([pos2(area.left(), y), pos2(end, y)], Stroke::new(1.0, Color32::from_gray(110)));
        painter.text(pos2(area.left() + 4.0, lane.top() + 3.0), Align2::LEFT_TOP, "Clic para añadir puntos a la envolvente del clip", theme::small(), TEXT_DIM);
    } else {
        let line = crate::curves::line(&points, x_of, &y_of, area.left(), end);
        painter.add(egui::Shape::line(line, Stroke::new(1.5, color)));
        for p in &points {
            painter.circle_filled(pos2(x_of(p.beat), y_of(p.value)), 3.5, color);
        }
    }

    let Some(pos) = response.interact_pointer_pos().or_else(|| response.hover_pos()) else { return response };
    let near = points.iter().position(|p| (x_of(p.beat) - pos.x).abs() <= POINT_GRAB && (y_of(p.value) - pos.y).abs() <= POINT_GRAB);
    let snap = |beat: f64| ((beat / grid).round() * grid).clamp(0.0, length);
    let lane_mut = |envelopes: &mut Vec<AutomationLane>| -> usize {
        match envelopes.iter().position(|l| l.param == param) {
            Some(i) => i,
            None => {
                envelopes.push(AutomationLane::new(param));
                envelopes.len() - 1
            }
        }
    };
    let alt = ui.input(|i| i.modifiers.alt);
    let stretch = crate::curves::stretch_at(&points, pos.x, x_of).filter(|_| near.is_none());
    if alt && stretch.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if response.double_clicked() {
        if let (Some(i), Some(lane)) = (near, index) {
            envelopes[lane].points.remove(i);
            envelopes.retain(|l| !l.points.is_empty());
        } else if let (true, Some(i), Some(lane)) = (alt, stretch, index) {
            // ⌥ + double clic: straight again.
            envelopes[lane].points[i].curve = 0.0;
        }
        choice.dragging = None;
        choice.bending = None;
        return response;
    }
    // ⌥ + drag on a stretch bends it.
    if let Some((i, start, from_y)) = choice.bending {
        if let (true, Some(lane)) = (response.double_clicked(), index) {
            if let Some(point) = envelopes[lane].points.get_mut(i) {
                point.curve = 0.0;
            }
            choice.bending = None;
            return response;
        }
        if let (true, Some(lane)) = (response.dragged(), index) {
            let points = &mut envelopes[lane].points;
            if let (Some(&from), Some(&to)) = (points.get(i), points.get(i + 1)) {
                points[i].curve = crate::curves::bent(start, pos.y - from_y, &from, &to);
            }
        }
        if ui.input(|i| i.pointer.primary_released()) {
            choice.bending = None;
        }
        return response;
    }
    if let (true, true, Some(i), Some(lane)) = (alt, ui.input(|i| i.pointer.primary_pressed()) && response.hovered(), stretch, index) {
        choice.bending = Some((i, envelopes[lane].points[i].curve, pos.y));
        return response;
    }
    if ui.input(|i| i.pointer.primary_pressed()) && response.hovered() && pos.x >= area.left() {
        let lane = lane_mut(envelopes);
        let points = &mut envelopes[lane].points;
        let i = match near {
            Some(i) => i,
            None => {
                let point = AutomationPoint { beat: snap(beat_of(pos.x)), value: value_of(pos.y), curve: 0.0 };
                let i = points.partition_point(|p| p.beat <= point.beat);
                points.insert(i, point);
                i
            }
        };
        choice.dragging = Some(i);
    }
    if response.dragged() {
        if let (Some(i), Some(lane)) = (choice.dragging, envelopes.iter().position(|l| l.param == param)) {
            let points = &mut envelopes[lane].points;
            if i < points.len() {
                let lo = if i > 0 { points[i - 1].beat } else { 0.0 };
                let hi = points.get(i + 1).map_or(length, |p| p.beat);
                points[i] = AutomationPoint { beat: snap(beat_of(pos.x)).clamp(lo, hi), value: value_of(pos.y), ..points[i] };
            }
        }
    }
    if ui.input(|i| i.pointer.primary_released()) {
        choice.dragging = None;
    }
    response
}

#[cfg(test)]
mod tests {
    use crate::app::{ClipRef, PulseApp};
    use crate::tracks::tests::{click, drag};
    use eframe::egui::{self, pos2, Modifiers};
    use engine::model::{Clip, Param};

    fn editor(app: &mut PulseApp, ui: &mut egui::Ui) {
        crate::piano_roll::show(app, ui);
    }

    fn envelope(app: &PulseApp, clip: ClipRef) -> Vec<(f64, f32)> {
        let Some(c) = app.clip(clip) else { return Vec::new() };
        c.envelopes().iter().find(|l| l.param == Param::Volume).map(|l| l.points.iter().map(|p| (p.beat, p.value)).collect()).unwrap_or_default()
    }

    #[test]
    fn envelopes_are_drawn_with_the_mouse_under_the_notes() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let clip = ClipRef::Session { track: 0, slot: 0 };
        assert!(matches!(app.clip(clip), Some(Clip::Midi(_))));
        app.select_clip(clip);
        // The lane shows the track's volume.
        app.piano_roll.lane.shown = Some(Param::Volume);
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| editor(&mut app, ui));
        // Beat 1 is 72 px after the 64 px key column; the lane is the bottom 60 px.
        let at = |beat: f32, height: f32| pos2(64.0 + beat * 72.0, 895.0 - height * 50.0);
        click(&mut app, editor, at(1.0, 0.5), Modifiers::NONE);
        let points = envelope(&app, clip);
        assert_eq!(points.len(), 1, "{points:?}");
        assert!((points[0].0 - 1.0).abs() < 1e-9, "on the grid at beat 1: {points:?}");
        click(&mut app, editor, at(3.0, 0.9), Modifiers::NONE);
        let points = envelope(&app, clip);
        assert_eq!(points.len(), 2);
        assert!(points[1].1 > points[0].1, "higher up, a higher value: {points:?}");
        // Dragging the first point to beat 2.
        drag(&mut app, editor, at(1.0, 0.5), at(2.0, 0.5), Modifiers::NONE);
        let points = envelope(&app, clip);
        assert!((points[0].0 - 2.0).abs() < 1e-9, "{points:?}");
        // A double clic deletes it.
        double_click(&mut app, at(2.0, 0.5));
        let points = envelope(&app, clip);
        assert_eq!(points.len(), 1, "{points:?}");
        assert!((points[0].0 - 3.0).abs() < 1e-9);
        // "Borrar envolvente" is shown, and the clip plays its envelope: the engine got it.
        assert!(matches!(app.clip(clip), Some(c) if !c.envelopes().is_empty()));
    }

    /// ⌥ + drag between two points bends that stretch; no point is added.
    #[test]
    fn alt_dragging_between_points_bends_the_envelope() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let clip = ClipRef::Session { track: 0, slot: 0 };
        app.select_clip(clip);
        app.piano_roll.lane.shown = Some(Param::Volume);
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| editor(&mut app, ui));
        let at = |beat: f32, height: f32| pos2(64.0 + beat * 72.0, 895.0 - height * 50.0);
        click(&mut app, editor, at(0.0, 0.1), Modifiers::NONE);
        click(&mut app, editor, at(3.0, 0.9), Modifiers::NONE);
        drag(&mut app, editor, at(1.5, 0.5), at(1.5, 0.9), Modifiers::ALT);
        let Some(c) = app.clip(clip) else { panic!() };
        let lane = c.envelopes().iter().find(|l| l.param == Param::Volume).unwrap();
        assert_eq!(lane.points.len(), 2, "no point added");
        assert!(lane.points[0].curve > 0.2, "dragged up, it bulges up: {}", lane.points[0].curve);
    }

    /// Two clics in a row in one context, as egui needs for a double clic.
    fn double_click(app: &mut PulseApp, at: egui::Pos2) {
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut frame = |app: &mut PulseApp, events: Vec<egui::Event>| {
            time += 0.02;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1600.0, 900.0))),
                time: Some(time),
                events,
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| editor(app, ui));
        };
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(app, vec![egui::Event::PointerMoved(at)]);
        for _ in 0..2 {
            frame(app, vec![button(true)]);
            frame(app, vec![button(false)]);
        }
        frame(app, vec![]);
    }

    #[test]
    fn audio_clips_show_their_envelopes_too() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let track = app.add_audio_track().unwrap();
        let mut audio = engine::model::AudioClip { hard_start: false,
            name: "a".into(),
            color: [1, 2, 3],
            sample: "/no/such.wav".into(),
            length: 4.0,
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
        audio.envelopes.push(engine::model::AutomationLane { param: Param::Pan, points: vec![engine::model::AutomationPoint { beat: 1.0, value: -1.0, curve: 0.0 }] });
        app.set_clip(track, 0, Some(Clip::Audio(audio)));
        app.select_clip(ClipRef::Session { track, slot: 0 });
        app.piano_roll.lane.shown = Some(Param::Pan);
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| editor(&mut app, ui));
        assert_eq!(app.piano_roll.lane.shown, Some(Param::Pan), "an audio track's pan can be shown");
    }
}

