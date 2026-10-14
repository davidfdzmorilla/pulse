//! A hand-drawn curve editor for [`Shape`]s (LFO cycles and the like).
//!
//! Drag a point to move it, double-click to add or remove one, drag the small
//! handle in the middle of a segment to bend it; right-click for preset shapes.

use crate::theme::{self, ACCENT};
use eframe::egui::{self, pos2, Color32, Sense, Stroke};
use engine::model::Shape;

/// Ready-made shapes offered in the context menu.
pub fn presets() -> [(&'static str, Shape); 7] {
    [
        ("Seno", sine()),
        ("Triángulo", Shape::from_points(&[(0.0, 0.5, 0.0), (0.25, 1.0, 0.0), (0.75, 0.0, 0.0), (1.0, 0.5, 0.0)])),
        ("Sierra", Shape::from_points(&[(0.0, 1.0, 0.0), (1.0, 0.0, 0.0)])),
        ("Rampa", Shape::from_points(&[(0.0, 0.0, 0.0), (1.0, 1.0, 0.0)])),
        ("Cuadrada", Shape::from_points(&[(0.0, 1.0, 0.0), (0.5, 1.0, 0.0), (0.501, 0.0, 0.0), (1.0, 0.0, 0.0)])),
        ("Bombeo", Shape::from_points(&[(0.0, 0.0, 0.0), (0.4, 1.0, -0.6), (1.0, 1.0, 0.0)])),
        ("Escalones", Shape::from_points(&[(0.0, 0.2, 0.0), (0.25, 0.2, 0.0), (0.251, 0.9, 0.0), (0.5, 0.9, 0.0), (0.501, 0.5, 0.0), (0.75, 0.5, 0.0), (0.751, 0.0, 0.0), (1.0, 0.0, 0.0)])),
    ]
}

/// A sine-like cycle from bent segments.
fn sine() -> Shape {
    Shape::from_points(&[(0.0, 0.5, 0.0), (0.25, 1.0, 1.0), (0.5, 0.5, -1.0), (0.75, 0.0, 1.0), (1.0, 0.5, -1.0)])
}

/// Draws and edits `shape` in a `size` area; returns the edited shape when it changed.
/// `playhead` (0..1) draws a marker, e.g. where an LFO is.
pub fn edit(ui: &mut egui::Ui, id: egui::Id, size: egui::Vec2, shape: Shape, playhead: Option<f32>) -> Option<Shape> {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::BG);
    let inner = rect.shrink2(egui::vec2(4.0, 6.0));
    let x_of = |t: f32| inner.left() + t * inner.width();
    let y_of = |v: f32| inner.bottom() - v * inner.height();
    let t_of = |x: f32| ((x - inner.left()) / inner.width()).clamp(0.0, 1.0);
    let v_of = |y: f32| ((inner.bottom() - y) / inner.height()).clamp(0.0, 1.0);

    for q in 1..4 {
        let x = x_of(q as f32 / 4.0);
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::from_gray(34)));
    }
    painter.line_segment([pos2(rect.left(), y_of(0.5)), pos2(rect.right(), y_of(0.5))], Stroke::new(1.0, Color32::from_gray(34)));
    let columns = inner.width() as usize;
    let line: Vec<_> = (0..=columns).map(|x| {
        let t = x as f32 / columns as f32;
        pos2(x_of(t), y_of(shape.value_at(t)))
    }).collect();
    painter.add(egui::Shape::line(line, Stroke::new(1.5, ACCENT)));
    if let Some(t) = playhead {
        let x = x_of(t.fract());
        painter.circle_filled(pos2(x, y_of(shape.value_at(t.fract()))), 3.0, Color32::WHITE);
    }

    let points = shape.points();
    let point_pos = |i: usize| pos2(x_of(points[i].time), y_of(points[i].value));
    let bend_pos = |i: usize| {
        let t = (points[i - 1].time + points[i].time) * 0.5;
        pos2(x_of(t), y_of(shape.value_at(t)))
    };
    for i in 1..points.len() {
        painter.circle_stroke(bend_pos(i), 2.5, Stroke::new(1.0, theme::shade(ACCENT, 0.8)));
    }
    let hover = response.hover_pos();
    for i in 0..points.len() {
        let hovered = hover.is_some_and(|p| p.distance(point_pos(i)) < 6.0);
        painter.circle_filled(point_pos(i), if hovered { 4.5 } else { 3.5 }, if hovered { Color32::WHITE } else { ACCENT });
    }

    let nearest = |pos: egui::Pos2| -> Option<(bool, usize)> {
        let point = (0..points.len()).map(|i| (point_pos(i).distance(pos), i)).min_by(|a, b| a.0.total_cmp(&b.0));
        let bend = (1..points.len()).map(|i| (bend_pos(i).distance(pos), i)).min_by(|a, b| a.0.total_cmp(&b.0));
        match (point, bend) {
            (Some((d, i)), _) if d < 7.0 => Some((false, i)),
            (_, Some((d, i))) if d < 6.0 => Some((true, i)),
            _ => None,
        }
    };
    let mut edited = shape;
    let grab_id = id.with("grab");
    if response.drag_started() {
        // Where the button went down: a drag only starts after the pointer has moved.
        let origin = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
        let grab = origin.and_then(nearest);
        ui.ctx().memory_mut(|m| m.data.insert_temp(grab_id, grab));
    }
    if response.dragged() {
        let grab: Option<(bool, usize)> = ui.ctx().memory(|m| m.data.get_temp(grab_id)).flatten();
        if let (Some((is_bend, i)), Some(pos)) = (grab, response.interact_pointer_pos()) {
            if is_bend {
                let direction = (points[i].value - points[i - 1].value).signum();
                edited.points[i].bend = (edited.points[i].bend - response.drag_delta().y / 40.0 * direction).clamp(-1.0, 1.0);
            } else {
                edited.move_point(i, t_of(pos.x), v_of(pos.y));
            }
        }
    }
    if response.double_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            match nearest(pos) {
                Some((false, i)) => edited.remove(i),
                _ => {
                    edited.insert(t_of(pos.x), v_of(pos.y));
                }
            }
        }
    }
    response.context_menu(|ui| {
        for (name, preset) in presets() {
            if ui.button(name).clicked() {
                edited = preset;
                ui.close();
            }
        }
    });
    response.on_hover_text("Arrastra los puntos · doble clic añade o borra · arrastra el círculo de un tramo para curvarlo · clic derecho: formas");
    (edited != shape).then_some(edited)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, PointerButton, Pos2, RawInput, Rect};

    #[test]
    fn double_click_adds_a_point_where_clicked() {
        let ctx = egui::Context::default();
        let size = egui::vec2(200.0, 100.0);
        let mut shape = Shape::falling();
        let mut area = Rect::NOTHING;
        let press = |pos: Pos2, pressed: bool| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        let mut frames: Vec<Vec<Event>> = vec![vec![]];
        for n in 0..4 {
            let target = pos2(4.0 + 0.5 * 192.0, 6.0 + 88.0 * 0.2);
            frames.push(match n {
                0 => vec![Event::PointerMoved(target)],
                _ => vec![press(target, true), press(target, false)],
            });
        }
        for (n, events) in frames.into_iter().enumerate() {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0))),
                events,
                time: Some(n as f64 * 0.05),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                area = Rect::from_min_size(ui.cursor().min, size);
                if let Some(edited) = edit(ui, egui::Id::new("t"), size, shape, None) {
                    shape = edited;
                }
            });
        }
        assert_eq!(area.min, Pos2::ZERO, "the editor sits at the origin, as the clicks assume");
        assert_eq!(shape.count, 3);
        assert!((shape.points[1].time - 0.5).abs() < 0.02 && (shape.points[1].value - 0.8).abs() < 0.03, "{:?}", shape.points[1]);
    }

    #[test]
    fn preset_shapes_span_the_range() {
        for (name, shape) in presets() {
            let values: Vec<f32> = (0..=100).map(|i| shape.value_at(i as f32 / 100.0)).collect();
            let (lo, hi) = values.iter().fold((1f32, 0f32), |(lo, hi), &v| (lo.min(v), hi.max(v)));
            assert!(lo < 0.25 && hi > 0.85, "{name}: {lo}..{hi}");
        }
    }
}
