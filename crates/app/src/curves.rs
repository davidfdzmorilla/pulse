//! Bent automation: the stretch between two breakpoints can curve (see
//! `AutomationPoint::curve`). Shared by the arrangement's automation lanes and
//! the clip envelopes: drawing the bends, and ⌥ + drag on a stretch to bend it
//! (⌥ + double clic straightens it), as in Live.

use eframe::egui::{pos2, Pos2};
use engine::model::{bend, AutomationPoint};

/// How far (pixels) the pointer drags for a full bend.
const BEND_PIXELS: f32 = 80.0;
/// Steps a bent stretch is drawn with.
const STEPS: usize = 24;

/// The line through `points`, flat before the first (from `left`) and after the
/// last (to `right`), bends drawn as curves.
pub fn line(points: &[AutomationPoint], x_of: &dyn Fn(f64) -> f32, y_of: &dyn Fn(f32) -> f32, left: f32, right: f32) -> Vec<Pos2> {
    let Some(first) = points.first() else { return Vec::new() };
    let mut line = vec![pos2(left, y_of(first.value))];
    for (i, p) in points.iter().enumerate() {
        line.push(pos2(x_of(p.beat), y_of(p.value)));
        match points.get(i + 1) {
            Some(next) if p.curve != 0.0 => {
                for step in 1..STEPS {
                    let u = step as f32 / STEPS as f32;
                    let beat = p.beat + (next.beat - p.beat) * u as f64;
                    let value = p.value + (next.value - p.value) * bend(u, p.curve);
                    line.push(pos2(x_of(beat), y_of(value)));
                }
            }
            _ => {}
        }
    }
    line.push(pos2(right, y_of(points[points.len() - 1].value)));
    line
}

/// The stretch (index of the point it starts at) under `x`.
pub fn stretch_at(points: &[AutomationPoint], x: f32, x_of: &dyn Fn(f64) -> f32) -> Option<usize> {
    points.windows(2).position(|w| x > x_of(w[0].beat) && x < x_of(w[1].beat))
}

/// The bend after dragging `dy` pixels (down is positive) from `start`: the
/// stretch follows the pointer, bulging up when dragged up.
pub fn bent(start: f32, dy: f32, from: &AutomationPoint, to: &AutomationPoint) -> f32 {
    // A rising stretch bulges up with a positive bend; a falling one, down.
    let rising = if to.value >= from.value { 1.0 } else { -1.0 };
    (start - dy / BEND_PIXELS * rising).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(beat: f64, value: f32, curve: f32) -> AutomationPoint {
        AutomationPoint { beat, value, curve }
    }

    #[test]
    fn dragging_up_bulges_the_stretch_up() {
        let (low, high) = (point(0.0, 0.0, 0.0), point(4.0, 1.0, 0.0));
        // Rising: up (negative dy) is a positive bend, which is above the straight line.
        let up = bent(0.0, -40.0, &low, &high);
        assert!(up > 0.0 && bend(0.5, up) > 0.5);
        // Falling: up is a negative bend, which also stays above the line.
        let up = bent(0.0, -40.0, &high, &low);
        let value = 1.0 + (0.0 - 1.0) * bend(0.5, up);
        assert!(value > 0.5, "{value}");
        assert_eq!(bent(0.9, -400.0, &low, &high), 1.0, "at most a full bend");
    }

    #[test]
    fn bends_are_drawn_as_curves() {
        let points = [point(0.0, 0.0, 0.8), point(4.0, 1.0, 0.0), point(8.0, 0.5, 0.0)];
        let x_of = |b: f64| b as f32 * 10.0;
        let y_of = |v: f32| 100.0 - v * 100.0;
        let line = line(&points, &x_of, &y_of, -10.0, 100.0);
        // The bent stretch has its steps; the straight one doesn't.
        assert_eq!(line.len(), 2 + 3 + (STEPS - 1));
        let middle = line.iter().find(|p| (p.x - 20.0).abs() < 0.01).unwrap();
        assert!(middle.y < 50.0, "above the straight line: {}", middle.y);
        assert_eq!(stretch_at(&points, 50.0, &x_of), Some(1));
        assert_eq!(stretch_at(&points, 90.0, &x_of), None);
    }
}
