//! The lane under the notes showing one of a MIDI clip's controllers (pitch
//! bend, the mod wheel, aftertouch or any CC), as Live's MIDI envelopes or
//! Bitwig's expressions: drawn freehand with the pencil, or as a straight
//! ramp with the line; a point dragged moves, a double clic deletes it, ⌥ +
//! drag between two points bends that stretch. With «Rejilla», what is drawn
//! falls on the editor's grid.

use crate::theme::{self, TEXT_DIM};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine::model::{thin_points, AutomationPoint, ControlLane, MidiControl};

const POINT_GRAB: f32 = 6.0;
/// The column of values on the left (the piano roll's keys).
pub const SCALE_W: f32 = 64.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    /// Freehand.
    #[default]
    Pencil,
    /// A straight ramp from where the drag starts to where it ends.
    Line,
}

#[derive(Clone, Copy, Debug)]
enum Drag {
    Point(usize),
    /// Freehand, from the last point drawn.
    Pencil { last: (f64, f32) },
    Line { from: (f64, f32), to: (f64, f32) },
    /// ⌥ + drag on the stretch from point `.0`: its bend when it began and
    /// the pointer's height then.
    Bend(usize, f32, f32),
}

/// How the controller lane is being drawn on.
pub struct Editing {
    pub tool: Tool,
    /// Points fall on the editor's grid.
    pub snap: bool,
    /// The CC number «Añadir» offers.
    pub cc: u8,
    drag: Option<Drag>,
}

impl Default for Editing {
    fn default() -> Self {
        Self { tool: Tool::Pencil, snap: false, cc: 74, drag: None }
    }
}

/// How far apart (beats) the pencil leaves points as it moves.
const PENCIL_STEP: f64 = 1.0 / 64.0;

/// The tools beside the lane menu, for the controller shown.
pub fn tools(ui: &mut egui::Ui, choice: &mut crate::clip_envelope::LaneChoice, controls: &mut Vec<ControlLane>, control: MidiControl) {
    let editing = &mut choice.expression;
    ui.add_space(6.0);
    ui.selectable_value(&mut editing.tool, Tool::Pencil, "Lápiz").on_hover_text("Arrastra para dibujar a mano alzada");
    ui.selectable_value(&mut editing.tool, Tool::Line, "Línea").on_hover_text("Arrastra de un punto a otro para trazar una rampa recta");
    ui.checkbox(&mut editing.snap, "Rejilla").on_hover_text("Lo que dibujas cae en la rejilla del editor");
    let count = controls.iter().find(|l| l.control == control).map_or(0, |l| l.points.len());
    if count > 0 {
        if ui.small_button("Borrar carril").on_hover_text("Quita este controlador del clip").clicked() {
            controls.retain(|l| l.control != control);
        }
        ui.label(egui::RichText::new(format!("{count} punto{}", if count == 1 { "" } else { "s" })).color(TEXT_DIM).size(10.5));
    }
    let others = controls.iter().filter(|l| !l.points.is_empty()).count();
    if others > 0 {
        ui.label(egui::RichText::new(format!("· {others} controlador{} en el clip", if others == 1 { "" } else { "es" })).color(TEXT_DIM).size(10.5));
    }
}

/// The values at the top, middle and bottom of a controller's lane, as written beside it.
fn scale(control: MidiControl) -> [&'static str; 3] {
    match control {
        MidiControl::PitchBend => ["+2 st", "0", "−2 st"],
        _ => ["127", "64", "0"],
    }
}

/// Draws controller `control` of a clip `length` beats long in `rect` and
/// edits it. `x_of`/`beat_of` place clip beats; `grid` is the editor's grid in beats.
#[allow(clippy::too_many_arguments)]
pub fn lane(
    ui: &mut egui::Ui,
    editing: &mut Editing,
    controls: &mut Vec<ControlLane>,
    control: MidiControl,
    length: f64,
    rect: Rect,
    x_of: &dyn Fn(f64) -> f32,
    beat_of: &dyn Fn(f32) -> f64,
    grid: f64,
    color: Color32,
) -> egui::Response {
    let response = ui.interact(rect, ui.id().with(("clip_controls", control)), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::SLOT);
    let area = Rect::from_min_max(pos2(rect.left() + SCALE_W, rect.top() + 5.0), rect.max - vec2(0.0, 5.0));
    let (min, max) = (control.min(), 1.0f32);
    let y_of = |v: f32| area.bottom() - area.height() * (v - min) / (max - min);
    let value_of = |y: f32| {
        let v = (min + (max - min) * (area.bottom() - y) / area.height()).clamp(min, max);
        // The bend has a detent in the middle, as the wheel has.
        if control == MidiControl::PitchBend && v.abs() < 0.03 { 0.0 } else { v }
    };
    // The scale on the left, the beats faintly, and past the clip's end (where it loops) darker.
    let scale_rect = Rect::from_min_max(rect.min, pos2(rect.left() + SCALE_W - 4.0, rect.bottom()));
    let names = scale(control);
    painter.text(pos2(scale_rect.right(), area.top()), Align2::RIGHT_TOP, names[0], theme::small(), TEXT_DIM);
    painter.text(pos2(scale_rect.right(), (area.top() + area.bottom()) / 2.0), Align2::RIGHT_CENTER, names[1], theme::small(), TEXT_DIM);
    painter.text(pos2(scale_rect.right(), area.bottom()), Align2::RIGHT_BOTTOM, names[2], theme::small(), TEXT_DIM);
    painter.text(pos2(rect.left() + 4.0, area.top()), Align2::LEFT_TOP, control.short(), theme::small(), color);
    let clip = painter.with_clip_rect(Rect::from_min_max(pos2(area.left(), rect.top()), rect.max));
    let first = beat_of(area.left()).max(0.0).floor() as i64;
    let last = beat_of(area.right()).ceil() as i64;
    for beat in first..=last.min(first + 4096) {
        let x = x_of(beat as f64);
        let strong = beat % 4 == 0;
        clip.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, if strong { Color32::from_gray(62) } else { Color32::from_gray(50) }));
    }
    let middle = y_of(if control == MidiControl::PitchBend { 0.0 } else { 0.5 });
    clip.line_segment([pos2(area.left(), middle), pos2(area.right(), middle)], Stroke::new(1.0, Color32::from_gray(58)));
    let end = x_of(length);
    clip.rect_filled(Rect::from_min_max(pos2(end, rect.top()), rect.max), 0.0, Color32::from_black_alpha(80));

    let index = controls.iter().position(|l| l.control == control);
    let points: Vec<AutomationPoint> = index.map(|i| controls[i].points.clone()).unwrap_or_default();
    if points.is_empty() {
        let rest = control.rest().unwrap_or(min);
        clip.line_segment([pos2(area.left(), y_of(rest)), pos2(end, y_of(rest))], Stroke::new(1.0, Color32::from_gray(110)));
        let hint = match editing.tool {
            Tool::Pencil => "Arrastra para dibujar el controlador a mano alzada",
            Tool::Line => "Arrastra de un punto a otro para trazar una rampa",
        };
        clip.text(pos2(area.left() + 6.0, rect.top() + 4.0), Align2::LEFT_TOP, hint, theme::small(), TEXT_DIM);
    } else {
        let line = crate::curves::line(&points, x_of, &y_of, area.left(), end);
        // Filled from the rest line (the middle for the bend, the bottom for the rest).
        let base = y_of(if control == MidiControl::PitchBend { 0.0 } else { min });
        let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 40);
        let mut mesh = egui::Mesh::default();
        for w in line.windows(2) {
            let (a, b) = (w[0], w[1]);
            let i = mesh.vertices.len() as u32;
            for p in [a, b, pos2(b.x, base), pos2(a.x, base)] {
                mesh.colored_vertex(p, fill);
            }
            mesh.add_triangle(i, i + 1, i + 2);
            mesh.add_triangle(i, i + 2, i + 3);
        }
        clip.add(egui::Shape::mesh(mesh));
        clip.add(egui::Shape::line(line, Stroke::new(1.6, color)));
        // The points, unless they are too close to tell apart.
        let dense = points.windows(2).any(|w| x_of(w[1].beat) - x_of(w[0].beat) < 5.0);
        for (i, p) in points.iter().enumerate() {
            let at = pos2(x_of(p.beat), y_of(p.value));
            let held = matches!(editing.drag, Some(Drag::Point(j)) if j == i);
            if !dense || held {
                clip.circle_filled(at, if held { 4.5 } else { 3.0 }, color);
            }
        }
    }
    if let Some(Drag::Line { from, to }) = editing.drag {
        clip.line_segment([pos2(x_of(from.0), y_of(from.1)), pos2(x_of(to.0), y_of(to.1))], Stroke::new(1.5, Color32::WHITE));
    }

    let Some(pos) = response.interact_pointer_pos().or_else(|| response.hover_pos()) else { return response };
    let snap = |beat: f64| if editing.snap && grid > 0.0 { (beat / grid).round() * grid } else { beat };
    let beat_at = |x: f32| snap(beat_of(x)).clamp(0.0, length);
    let near = points.iter().position(|p| (x_of(p.beat) - pos.x).abs() <= POINT_GRAB && (y_of(p.value) - pos.y).abs() <= POINT_GRAB);
    let alt = ui.input(|i| i.modifiers.alt);
    let stretch = crate::curves::stretch_at(&points, pos.x, x_of).filter(|_| near.is_none());
    if alt && stretch.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    } else if near.is_none() && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    let lane_mut = |controls: &mut Vec<ControlLane>| -> usize {
        match controls.iter().position(|l| l.control == control) {
            Some(i) => i,
            None => {
                controls.push(ControlLane::new(control));
                controls.sort_by_key(|l| l.control);
                controls.iter().position(|l| l.control == control).unwrap_or(0)
            }
        }
    };
    if response.double_clicked() {
        if let (Some(i), Some(lane)) = (near, index) {
            controls[lane].points.remove(i);
            controls.retain(|l| !l.points.is_empty());
        } else if let (true, Some(i), Some(lane)) = (alt, stretch, index) {
            controls[lane].points[i].curve = 0.0;
        }
        editing.drag = None;
        return response;
    }
    let pressed = ui.input(|i| i.pointer.primary_pressed()) && response.hovered() && pos.x >= area.left();
    if pressed {
        editing.drag = match (near, alt.then_some(stretch).flatten(), editing.tool) {
            (Some(i), _, _) => Some(Drag::Point(i)),
            (None, Some(i), _) => index.map(|lane| Drag::Bend(i, controls[lane].points[i].curve, pos.y)),
            (None, None, Tool::Line) => {
                let at = (beat_at(pos.x), value_of(pos.y));
                Some(Drag::Line { from: at, to: at })
            }
            (None, None, Tool::Pencil) => {
                let at = (beat_at(pos.x), value_of(pos.y));
                let lane = lane_mut(controls);
                draw(&mut controls[lane].points, at, at);
                Some(Drag::Pencil { last: at })
            }
        };
    }
    if response.dragged() || pressed {
        match editing.drag {
            Some(Drag::Point(i)) => {
                if let Some(lane) = controls.iter().position(|l| l.control == control) {
                    let points = &mut controls[lane].points;
                    if i < points.len() {
                        let lo = if i > 0 { points[i - 1].beat } else { 0.0 };
                        let hi = points.get(i + 1).map_or(length, |p| p.beat);
                        points[i] = AutomationPoint { beat: beat_at(pos.x).clamp(lo, hi), value: value_of(pos.y), ..points[i] };
                    }
                }
            }
            Some(Drag::Bend(i, start, from_y)) => {
                if let Some(lane) = controls.iter().position(|l| l.control == control) {
                    let points = &mut controls[lane].points;
                    if let (Some(&a), Some(&b)) = (points.get(i), points.get(i + 1)) {
                        points[i].curve = crate::curves::bent(start, pos.y - from_y, &a, &b);
                    }
                }
            }
            Some(Drag::Line { from, .. }) => editing.drag = Some(Drag::Line { from, to: (beat_at(pos.x), value_of(pos.y)) }),
            Some(Drag::Pencil { last }) => {
                let at = (beat_at(pos.x), value_of(pos.y));
                if (at.0 - last.0).abs() >= if editing.snap { grid.min(PENCIL_STEP).max(1e-6) } else { PENCIL_STEP } || (at.0 == last.0 && at.1 != last.1) {
                    let lane = lane_mut(controls);
                    draw(&mut controls[lane].points, last, at);
                    editing.drag = Some(Drag::Pencil { last: at });
                }
            }
            None => {}
        }
    }
    if ui.input(|i| i.pointer.primary_released()) {
        match editing.drag.take() {
            Some(Drag::Line { from, to }) => {
                let lane = lane_mut(controls);
                draw(&mut controls[lane].points, from, to);
            }
            Some(Drag::Pencil { .. }) => {
                // A freehand stroke keeps its shape with fewer points.
                if let Some(lane) = controls.iter_mut().find(|l| l.control == control) {
                    lane.points = thin_points(&lane.points, crate::expression::TOLERANCE * 2.0);
                }
            }
            _ => {}
        }
    }
    response
}

/// Draws from `a` to `b` (beat, value): what was between goes, both ends stay
/// as points.
fn draw(points: &mut Vec<AutomationPoint>, a: (f64, f32), b: (f64, f32)) {
    let (lo, hi) = if a.0 <= b.0 { (a, b) } else { (b, a) };
    points.retain(|p| p.beat < lo.0 - 1e-9 || p.beat > hi.0 + 1e-9);
    points.push(AutomationPoint { beat: lo.0, value: lo.1, curve: 0.0 });
    if hi.0 > lo.0 + 1e-9 {
        points.push(AutomationPoint { beat: hi.0, value: hi.1, curve: 0.0 });
    } else {
        if let Some(p) = points.last_mut() {
            p.value = b.1;
        }
    }
    points.sort_by(|x, y| x.beat.total_cmp(&y.beat));
}

#[cfg(test)]
mod tests {
    use crate::app::{ClipRef, PulseApp};
    use crate::project::{Project, Track};
    use crate::tracks::tests::drag;
    use eframe::egui::{self, pos2, vec2, Modifiers, Rect};
    use engine::model::{AutomationPoint, Clip, ControlLane, InstrumentKind, MidiClip, MidiControl, Note};

    fn editor(app: &mut PulseApp, ui: &mut egui::Ui) {
        crate::piano_roll::show(app, ui);
    }

    /// A synth track with a 4-beat clip of a few notes, open in the editor.
    fn app_with_clip(lanes: Vec<ControlLane>) -> (PulseApp, ClipRef) {
        let mut project = Project::empty();
        project.tracks.clear();
        let mut track = Track::new("Lead", [230, 120, 70], InstrumentKind::Synth(Default::default()));
        let mut clip = MidiClip::new("Lead", 4.0, [230, 120, 70]);
        for (i, pitch) in [60u8, 64, 67, 72, 71, 67].iter().enumerate() {
            clip.notes.push(Note::new(i as f64 * 0.5, 0.75, *pitch, 0.8));
        }
        clip.notes.push(Note::new(3.0, 1.0, 62, 0.9));
        clip.controls = lanes;
        track.clips[0] = Some(Clip::Midi(clip));
        project.tracks.push(track);
        let mut app = PulseApp::headless(project);
        let clip = ClipRef::Session { track: 0, slot: 0 };
        app.select_clip(clip);
        (app, clip)
    }

    fn lane(app: &PulseApp, clip: ClipRef, control: MidiControl) -> Vec<AutomationPoint> {
        let Some(Clip::Midi(c)) = app.clip(clip) else { return Vec::new() };
        c.controls.iter().find(|l| l.control == control).map(|l| l.points.clone()).unwrap_or_default()
    }

    /// Where beat `beat` and value `value` of the controller lane fall on a
    /// 1600×900 screen: the lane is 120 px tall at the bottom, its values 5 px in.
    fn at(control: MidiControl, beat: f64, value: f32) -> egui::Pos2 {
        let (bottom, height) = (889.0, 110.0);
        let (min, max) = (control.min(), 1.0);
        pos2(64.0 + beat as f32 * 72.0, bottom - height * (value - min) / (max - min))
    }

    #[test]
    fn the_pencil_draws_a_bend_under_the_notes() {
        let (mut app, clip) = app_with_clip(Vec::new());
        app.piano_roll.lane.show_control(MidiControl::PitchBend);
        let bend = MidiControl::PitchBend;
        drag(&mut app, editor, at(bend, 0.5, -1.0), at(bend, 3.5, 1.0), Modifiers::NONE);
        let points = lane(&app, clip, bend);
        assert!(points.len() >= 2 && points.len() <= 6, "drawn and thinned: {points:?}");
        let value = |beat: f64| ControlLane { control: bend, points: points.clone() }.value_at(beat).unwrap();
        assert!((points[0].beat - 0.5).abs() < 0.05, "{points:?}");
        assert!(value(0.6) < -0.85 && value(3.45) > 0.85, "{points:?}");
        assert!(value(2.0).abs() < 0.1, "through the middle: {}", value(2.0));
        // Near the middle snaps to it (the wheel's detent).
        drag(&mut app, editor, at(bend, 3.7, 0.02), at(bend, 3.9, -0.02), Modifiers::NONE);
        let points = lane(&app, clip, bend);
        assert_eq!(points.last().unwrap().value, 0.0, "{points:?}");
        // A double clic on a point deletes it.
        let first = points[0];
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut frame = |app: &mut PulseApp, events: Vec<egui::Event>| {
            time += 0.02;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1600.0, 900.0))), time: Some(time), events, ..Default::default() };
            let _ = ctx.run_ui(input, |ui| editor(app, ui));
        };
        let p = at(bend, first.beat, first.value);
        let button = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(&mut app, vec![egui::Event::PointerMoved(p)]);
        for _ in 0..2 {
            frame(&mut app, vec![button(true)]);
            frame(&mut app, vec![button(false)]);
        }
        frame(&mut app, vec![]);
        assert_eq!(lane(&app, clip, bend).len(), points.len() - 1, "deleted");
        // The engine plays what was drawn: the clip it has holds it.
        assert!(matches!(app.clip(clip), Some(Clip::Midi(c)) if !c.controls.is_empty()));
    }

    #[test]
    fn the_line_draws_a_ramp_on_the_grid_and_points_move() {
        let (mut app, clip) = app_with_clip(Vec::new());
        let cc = MidiControl::Cc(74);
        app.piano_roll.lane.show_control(cc);
        app.piano_roll.lane.expression.tool = super::Tool::Line;
        app.piano_roll.lane.expression.snap = true;
        drag(&mut app, editor, at(cc, 1.08, 0.2), at(cc, 2.93, 0.8), Modifiers::NONE);
        let points = lane(&app, clip, cc);
        assert_eq!(points.len(), 2, "{points:?}");
        assert_eq!((points[0].beat, points[1].beat), (1.0, 3.0), "on the grid");
        assert!((points[0].value - 0.2).abs() < 0.02 && (points[1].value - 0.8).abs() < 0.02, "{points:?}");
        // A ramp the other way over its end replaces what was there.
        drag(&mut app, editor, at(cc, 2.5, 0.5), at(cc, 3.5, 0.1), Modifiers::NONE);
        let points = lane(&app, clip, cc);
        assert_eq!(points.iter().map(|p| p.beat).collect::<Vec<_>>(), [1.0, 2.5, 3.5]);
        // Dragging a point moves it, between its neighbours.
        drag(&mut app, editor, at(cc, 2.5, 0.5), at(cc, 2.0, 0.9), Modifiers::NONE);
        let points = lane(&app, clip, cc);
        assert_eq!(points.len(), 3);
        assert_eq!(points[1].beat, 2.0);
        assert!((points[1].value - 0.9).abs() < 0.02);
        // ⌥ + drag between two points bends that stretch.
        drag(&mut app, editor, at(cc, 1.5, 0.4), at(cc, 1.5, 0.8), Modifiers::ALT);
        let points = lane(&app, clip, cc);
        assert_eq!(points.len(), 3, "no point added");
        assert!(points[0].curve > 0.2, "{points:?}");
    }

    /// Pictures of the editor with recorded expression, wide and narrow:
    /// `PULSE_SHOTS=dir cargo test --release -p app expression_shots -- --ignored`.
    #[test]
    #[ignore]
    fn expression_shots() {
        let Some(dir) = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        // As a take leaves them: a bend into the last note and back with some
        // shake, the wheel brought in, and a filter sweep with brightness.
        let point = |beat: f64, value: f32| AutomationPoint { beat, value, curve: 0.0 };
        let bend: Vec<AutomationPoint> = [(0.0, 0.0), (2.9, 0.0), (3.0, -0.6), (3.25, 0.05), (3.4, 0.0)].iter().map(|&(b, v)| point(b, v)).chain((0..12).map(|i| point(3.45 + i as f64 * 0.04, ((i as f32) * 1.3).sin() * 0.12))).collect();
        let wheel = vec![point(0.0, 0.0), point(1.0, 0.0), AutomationPoint { beat: 1.2, value: 0.15, curve: 0.5 }, point(2.6, 0.85), point(4.0, 0.85)];
        let bright = vec![point(0.0, 0.3), AutomationPoint { beat: 0.5, value: 0.35, curve: -0.6 }, point(3.5, 0.95)];
        let lanes = vec![ControlLane { control: MidiControl::PitchBend, points: bend }, ControlLane { control: MidiControl::MOD_WHEEL, points: wheel }, ControlLane { control: MidiControl::Cc(74), points: bright }];
        for (name, control, size) in [
            ("expresion-bend-1440", MidiControl::PitchBend, vec2(1440.0, 900.0)),
            ("expresion-rueda-1440", MidiControl::MOD_WHEEL, vec2(1440.0, 900.0)),
            ("expresion-cc74-1000", MidiControl::Cc(74), vec2(1000.0, 720.0)),
            ("expresion-vacio-1440", MidiControl::Pressure, vec2(1440.0, 900.0)),
        ] {
            let (mut app, _) = app_with_clip(lanes.clone());
            app.piano_roll.lane.show_control(control);
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            let mut canvas = crate::offscreen::Canvas::default();
            let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
            let mut output = None;
            for frame in 0..4 {
                ctx.set_pixels_per_point(2.0);
                output = Some(ctx.run_ui(input(), |ui| {
                    egui::CentralPanel::default().show(ui, |ui| editor(&mut app, ui));
                }));
                if frame < 3 {
                    canvas.draw(&ctx, output.take().unwrap(), size, 2.0);
                }
            }
            let image = canvas.draw(&ctx, output.unwrap(), size, 2.0);
            image.save_png(&dir.join(format!("{name}.png"))).unwrap();
        }
    }
}
