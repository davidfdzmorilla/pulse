//! Custom controls: rotary knobs, dB faders, level meters and toggle buttons.

use crate::theme::{self, ACCENT, CONTROL, CONTROL_HOVER, KNOB_TRACK, TEXT, TEXT_DIM};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui};
use std::f32::consts::PI;
use std::ops::RangeInclusive;

/// How a knob maps its value onto its travel.
#[derive(Clone, Copy)]
pub enum Taper {
    Linear,
    Log,
}

pub struct Knob<'a> {
    value: &'a mut f32,
    range: RangeInclusive<f32>,
    default: f32,
    label: &'a str,
    taper: Taper,
    format: Box<dyn Fn(f32) -> String + 'a>,
    /// Draw the value arc from the center (pan) instead of from the minimum.
    bipolar: bool,
    diameter: f32,
}

impl<'a> Knob<'a> {
    pub fn new(value: &'a mut f32, range: RangeInclusive<f32>, default: f32, label: &'a str) -> Self {
        Self {
            value,
            range,
            default,
            label,
            taper: Taper::Linear,
            format: Box::new(|v| format!("{v:.2}")),
            bipolar: false,
            diameter: 30.0,
        }
    }

    pub fn taper(mut self, taper: Taper) -> Self {
        self.taper = taper;
        self
    }

    pub fn format(mut self, format: impl Fn(f32) -> String + 'a) -> Self {
        self.format = Box::new(format);
        self
    }

    pub fn bipolar(mut self) -> Self {
        self.bipolar = true;
        self
    }

    pub fn diameter(mut self, diameter: f32) -> Self {
        self.diameter = diameter;
        self
    }

    fn to_norm(&self, v: f32) -> f32 {
        let (lo, hi) = (*self.range.start(), *self.range.end());
        match self.taper {
            Taper::Linear => (v - lo) / (hi - lo),
            Taper::Log => (v / lo).ln() / (hi / lo).ln(),
        }
        .clamp(0.0, 1.0)
    }

    fn from_norm(&self, n: f32) -> f32 {
        let (lo, hi) = (*self.range.start(), *self.range.end());
        let n = n.clamp(0.0, 1.0);
        match self.taper {
            Taper::Linear => lo + n * (hi - lo),
            Taper::Log => lo * (hi / lo).powf(n),
        }
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let d = self.diameter;
        let width = (d + 22.0).max(46.0);
        let (rect, mut response) = ui.allocate_exact_size(vec2(width, d + 28.0), Sense::click_and_drag());
        learn_target(ui, &mut response);

        if response.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let pixels = if fine { 600.0 } else { 150.0 };
            let delta = -response.drag_delta().y / pixels;
            let n = self.to_norm(*self.value) + delta;
            let new = self.from_norm(n);
            if new != *self.value {
                *self.value = new;
                response.mark_changed();
            }
        }
        if response.double_clicked() && *self.value != self.default {
            *self.value = self.default;
            response.mark_changed();
        }

        let painter = ui.painter();
        painter.text(
            pos2(rect.center().x, rect.top() + 6.0),
            Align2::CENTER_CENTER,
            self.label,
            theme::small(),
            TEXT_DIM,
        );
        let center = pos2(rect.center().x, rect.top() + 14.0 + d * 0.5);
        let radius = d * 0.5;
        // 270-degree sweep with the gap at the bottom.
        let start = PI * 0.75;
        let sweep = PI * 1.5;
        let arc = |from: f32, to: f32| -> Vec<egui::Pos2> {
            let steps = 32;
            (0..=steps)
                .map(|i| {
                    let a = start + sweep * (from + (to - from) * i as f32 / steps as f32);
                    pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
                })
                .collect()
        };
        painter.add(Shape::line(arc(0.0, 1.0), Stroke::new(3.0, KNOB_TRACK)));
        let n = self.to_norm(*self.value);
        let from = if self.bipolar { 0.5 } else { 0.0 };
        if (n - from).abs() > 0.002 {
            painter.add(Shape::line(arc(from.min(n), from.max(n)), Stroke::new(3.0, ACCENT)));
        }
        let angle = start + sweep * n;
        let hover = response.hovered() || response.dragged();
        painter.circle_filled(center, radius - 5.0, if hover { CONTROL_HOVER } else { CONTROL });
        painter.line_segment(
            [
                pos2(center.x + (radius - 12.0) * angle.cos() * 0.2, center.y + (radius - 12.0) * angle.sin() * 0.2),
                pos2(center.x + (radius - 5.0) * angle.cos(), center.y + (radius - 5.0) * angle.sin()),
            ],
            Stroke::new(2.0, TEXT),
        );
        painter.text(
            pos2(rect.center().x, rect.bottom() - 6.0),
            Align2::CENTER_CENTER,
            (self.format)(*self.value),
            theme::small(),
            if hover { Color32::WHITE } else { TEXT },
        );
        response.on_hover_text("Arrastra arriba/abajo (Shift = fino) · doble clic = valor por defecto")
    }
}

fn learn_id() -> egui::Id {
    egui::Id::new("pulse_midi_learn")
}

/// Tells the widgets whether MIDI learn is on (once a frame).
pub fn set_learning(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(learn_id(), on));
}

pub fn learning(ui: &Ui) -> bool {
    ui.ctx().data(|d| d.get_temp(learn_id())).unwrap_or(false)
}

/// While MIDI learn is on, knobs and faders show they can be linked, and a click picks
/// one (it reports a change of its value to itself, which tells the app which it is).
fn learn_target(ui: &Ui, response: &mut Response) {
    if !learning(ui) {
        return;
    }
    let color = if response.hovered() { ACCENT } else { ACCENT.gamma_multiply(0.45) };
    ui.painter().rect_stroke(response.rect.shrink(1.0), 3.0, Stroke::new(1.5, color), egui::StrokeKind::Inside);
    if response.clicked() || response.drag_started() {
        response.mark_changed();
    }
}

/// Fader taper: unity gain at about 80% of travel, +6 dB at the top.
pub fn gain_to_fader(gain: f32) -> f32 {
    (gain / 2.0).max(0.0).cbrt()
}

pub fn fader_to_gain(position: f32) -> f32 {
    2.0 * position.clamp(0.0, 1.0).powi(3)
}

/// Meter position for a linear peak: -60..+6 dB mapped onto 0..1.
fn meter_position(peak: f32) -> f32 {
    if peak <= 0.0 {
        return 0.0;
    }
    ((20.0 * peak.log10() + 60.0) / 66.0).clamp(0.0, 1.0)
}

pub fn paint_meter(painter: &egui::Painter, rect: Rect, peak: f32) {
    painter.rect_filled(rect, 1.0, theme::SLOT);
    let level = meter_position(peak);
    if level <= 0.0 {
        return;
    }
    let top = rect.bottom() - rect.height() * level;
    let zero_db = rect.bottom() - rect.height() * (60.0 / 66.0);
    let minus_six = rect.bottom() - rect.height() * (54.0 / 66.0);
    let segments = [
        (rect.bottom(), minus_six, theme::METER_GREEN),
        (minus_six, zero_db, theme::METER_YELLOW),
        (zero_db, rect.top(), theme::METER_RED),
    ];
    for (bottom, segment_top, color) in segments {
        let y = top.max(segment_top);
        if y < bottom {
            painter.rect_filled(Rect::from_min_max(pos2(rect.left(), y), pos2(rect.right(), bottom)), 0.0, color);
        }
    }
}

pub fn horizontal_meter(ui: &mut Ui, size: egui::Vec2, peak: f32) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 1.0, theme::BG);
    let level = meter_position(peak);
    let color = if level > 60.0 / 66.0 {
        theme::METER_RED
    } else if level > 54.0 / 66.0 {
        theme::METER_YELLOW
    } else {
        theme::METER_GREEN
    };
    painter.rect_filled(Rect::from_min_size(rect.min, vec2(rect.width() * level, rect.height())), 1.0, color);
}

/// Vertical volume fader with a level meter next to it. `gain` is linear.
pub fn fader_with_meter(ui: &mut Ui, gain: &mut f32, peak: f32, height: f32) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(34.0, height + 16.0), Sense::click_and_drag());
    learn_target(ui, &mut response);
    let track = Rect::from_min_size(pos2(rect.left() + 6.0, rect.top() + 4.0), vec2(4.0, height - 8.0));
    let meter = Rect::from_min_size(pos2(rect.left() + 18.0, rect.top()), vec2(8.0, height));

    if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let scale = if fine { 4.0 } else { 1.0 };
        let position = gain_to_fader(*gain) - response.drag_delta().y / (track.height() * scale);
        *gain = fader_to_gain(position);
        response.mark_changed();
    }
    if response.double_clicked() {
        *gain = 1.0;
        response.mark_changed();
    }

    let painter = ui.painter();
    painter.rect_filled(track, 2.0, theme::CONTROL);
    let unity_y = track.bottom() - track.height() * gain_to_fader(1.0);
    painter.line_segment(
        [pos2(track.left() - 3.0, unity_y), pos2(track.right() + 3.0, unity_y)],
        Stroke::new(1.0, TEXT_DIM),
    );
    let y = track.bottom() - track.height() * gain_to_fader(*gain);
    let thumb = Rect::from_center_size(pos2(track.center().x, y), vec2(14.0, 6.0));
    let hot = response.hovered() || response.dragged();
    painter.rect_filled(thumb, 1.0, if hot { Color32::WHITE } else { TEXT });
    paint_meter(painter, meter, peak);

    let db = 20.0 * gain.max(1e-6).log10();
    painter.text(
        pos2(rect.center().x, rect.bottom() - 6.0),
        Align2::CENTER_CENTER,
        if db <= -69.0 { "-inf".to_string() } else { format!("{db:.1}") },
        theme::small(),
        TEXT,
    );
    response.on_hover_text("Volumen · doble clic = 0 dB · Shift = fino")
}

/// Small square toggle like a mixer's activator or solo button.
pub fn toggle(ui: &mut Ui, on: bool, label: &str, on_color: Color32, size: egui::Vec2) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if on {
        on_color
    } else if response.hovered() {
        CONTROL_HOVER
    } else {
        CONTROL
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, fill);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        theme::small(),
        if on { theme::text_on(on_color) } else { TEXT },
    );
    response
}

/// A flat button styled like the transport controls.
pub fn flat_button(ui: &mut Ui, label: &str, active: bool, size: egui::Vec2) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if active {
        ACCENT
    } else if response.hovered() {
        CONTROL_HOVER
    } else {
        CONTROL
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, fill);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        theme::body(),
        if active { theme::text_on(ACCENT) } else { TEXT },
    );
    response
}

pub fn outline(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.rect_stroke(rect, 2.0, Stroke::new(1.5, color), StrokeKind::Inside);
}

pub fn play_triangle(painter: &egui::Painter, center: egui::Pos2, size: f32, fill: Color32) {
    painter.add(Shape::convex_polygon(
        vec![
            pos2(center.x - size * 0.45, center.y - size * 0.55),
            pos2(center.x + size * 0.55, center.y),
            pos2(center.x - size * 0.45, center.y + size * 0.55),
        ],
        fill,
        Stroke::NONE,
    ));
}

/// Formats a parameter value for display according to its unit.
pub fn format_param(info: &engine::model::ParamInfo, value: f32) -> String {
    use engine::model::Unit;
    match info.unit {
        Unit::Decibels if info.name == "Volumen" => {
            let db = 20.0 * value.max(1e-6).log10();
            if db <= -69.0 { "-inf dB".into() } else { format!("{db:.1} dB") }
        }
        Unit::Decibels if info.min <= -40.0 && value <= -39.9 => "-inf dB".into(),
        Unit::Decibels => format!("{value:+.1} dB"),
        Unit::Hertz if value >= 1000.0 => format!("{:.2} kHz", value / 1000.0),
        Unit::Hertz => format!("{value:.0} Hz"),
        Unit::Percent => format!("{:.0} %", value * 100.0),
        Unit::Seconds if value < 1.0 => format!("{:.0} ms", value * 1000.0),
        Unit::Seconds => format!("{value:.2} s"),
        Unit::Milliseconds => format!("{value:.1} ms"),
        Unit::Ratio => format!("{value:.1}:1"),
        Unit::Octaves => format!("{value:.1} oct"),
        Unit::Semitones => format!("{value:+.1} st"),
        Unit::Times => format!("×{value:.2}"),
        Unit::Bits => format!("{value:.1} bits"),
        Unit::Count => format!("{}", value.round()),
        Unit::Degrees => format!("{value:.0}°"),
        Unit::Note => crate::project::note_name(value.round().clamp(0.0, 127.0) as u8),
        Unit::Plain => format!("{value:.2}"),
        Unit::Pan => match (value * 50.0).round() as i32 {
            0 => "C".into(),
            n if n < 0 => format!("{}L", -n),
            n => format!("{n}R"),
        },
    }
}

/// Marks an automated control: red while automation drives it, hollow when overridden.
pub fn automation_dot(painter: &egui::Painter, at: egui::Pos2, overridden: Option<bool>) {
    match overridden {
        Some(false) => {
            painter.circle_filled(at, 3.0, theme::METER_RED);
        }
        Some(true) => {
            painter.circle_stroke(at, 3.0, Stroke::new(1.0, theme::METER_RED));
        }
        None => {}
    }
}

/// Knob for a described parameter. Returns the new value when the user moves it.
pub fn param_knob(
    ui: &mut Ui,
    info: &engine::model::ParamInfo,
    value: f32,
    label: &str,
    diameter: f32,
    automation: Option<bool>,
) -> Option<f32> {
    param_knob_response(ui, info, value, label, diameter, automation).0
}

/// Like [`param_knob`], also returning the knob's response (for drops, menus and overlays).
pub fn param_knob_response(
    ui: &mut Ui,
    info: &engine::model::ParamInfo,
    value: f32,
    label: &str,
    diameter: f32,
    automation: Option<bool>,
) -> (Option<f32>, Response) {
    let mut v = value;
    let mut knob = Knob::new(&mut v, info.min..=info.max, info.default, label)
        .diameter(diameter)
        .format(|x| format_param(info, x));
    if info.log {
        knob = knob.taper(Taper::Log);
    }
    if info.unit == engine::model::Unit::Pan {
        knob = knob.bipolar();
    }
    let response = knob.show(ui);
    automation_dot(ui.painter(), response.rect.right_top() + vec2(-4.0, 6.0), automation);
    (response.changed().then_some(v), response)
}

/// An arc just outside a knob drawn by [`Knob::show`] in `rect`, between two
/// positions of its travel (0..1): how far a modulation moves it.
pub fn modulation_ring(painter: &egui::Painter, rect: egui::Rect, diameter: f32, from: f32, to: f32, color: Color32) {
    let center = pos2(rect.center().x, rect.top() + 14.0 + diameter * 0.5);
    let radius = diameter * 0.5 + 3.5;
    let (start, sweep) = (PI * 0.75, PI * 1.5);
    let (a, b) = (from.clamp(0.0, 1.0).min(to.clamp(0.0, 1.0)), from.clamp(0.0, 1.0).max(to.clamp(0.0, 1.0)));
    if b - a < 0.002 {
        return;
    }
    let points: Vec<_> = (0..=24)
        .map(|i| {
            let angle = start + sweep * (a + (b - a) * i as f32 / 24.0);
            pos2(center.x + radius * angle.cos(), center.y + radius * angle.sin())
        })
        .collect();
    painter.add(Shape::line(points, Stroke::new(2.0, color)));
}
