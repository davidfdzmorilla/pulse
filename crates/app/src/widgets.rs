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
        let (enabled, value) = (response.enabled(), *self.value as f64);
        response.widget_info(|| egui::WidgetInfo::slider(enabled, value, self.label));

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
pub fn meter_position(peak: f32) -> f32 {
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

pub fn paint_horizontal_meter(painter: &egui::Painter, rect: Rect, peak: f32) {
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
    let (enabled, value) = (response.enabled(), *gain as f64);
    response.widget_info(|| egui::WidgetInfo::slider(enabled, value, "Volumen"));
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

/// A right-click menu with fields in it (names, amounts): egui's own closes on any
/// click, even in a text field, so this one closes only on a click outside it (and
/// its buttons close it themselves).
pub fn edit_menu<R>(response: &Response, add_contents: impl FnOnce(&mut Ui) -> R) -> Option<egui::InnerResponse<R>> {
    egui::Popup::context_menu(response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(add_contents)
}

/// A menu button whose menu has fields (a name to type, sliders), which egui's own
/// menus close on the first click in them: this one closes on a click outside it
/// (its buttons close it themselves).
pub fn field_menu_button<R>(ui: &mut Ui, label: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let config = egui::containers::menu::MenuConfig::find(ui).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    egui::containers::menu::MenuButton::new(label).config(config).ui(ui, add_contents).1.map(|r| r.inner)
}

/// `ui.menu_button`, but a submenu stays open while the pointer heads for it, as
/// on macOS: crossing the items between its button and the item one wants (going
/// down diagonally from «Acompañar ⏵» to «Bajo», over «De tarareo a canción ⏵»)
/// no longer swaps it for theirs. Every menu in Pulse goes through here.
pub fn menu_button<'a, R>(ui: &mut Ui, atoms: impl egui::IntoAtoms<'a>, add_contents: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<Option<R>> {
    use egui::containers::menu::{self, MenuState, SubMenu, SubMenuButton};
    if !menu::is_in_menu(ui) {
        let (response, inner) = menu::MenuButton::new(atoms).ui(ui, add_contents);
        return egui::InnerResponse::new(inner.map(|i| i.inner), response);
    }
    let before = MenuState::from_ui(ui, |state, _| state.open_item);
    let (response, inner) = SubMenuButton::new(atoms).ui(ui, add_contents);
    let id = SubMenu::id_from_widget_id(response.id);
    if let Some(popup) = inner.as_ref().map(|i| i.response.rect) {
        let (pointer, time, clicked) = ui.input(|i| (i.pointer.hover_pos(), i.time, i.pointer.any_click()));
        let after = MenuState::from_ui(ui, |state, _| state.open_item);
        let aim_id = id.with("aim");
        let aim: Option<(egui::Pos2, f64)> = ui.data(|d| d.get_temp(aim_id));
        let on_button = pointer.is_some_and(|p| response.rect.expand2(ui.spacing().item_spacing / 2.0).contains(p));
        let on_popup = pointer.is_some_and(|p| popup.contains(p));
        let mut keep = None;
        if let Some(p) = pointer {
            if on_button || on_popup {
                keep = Some((p, time));
            } else if let Some((anchor, since)) = aim {
                // Still heading for the submenu (inside the triangle from where the
                // pointer was to its near edge), and not resting for long.
                let moved = p.distance(anchor) > 0.5;
                if aiming(anchor, p, popup) && (moved || time - since < SUBMENU_GRACE) {
                    keep = Some(if moved { (p, time) } else { (anchor, since) });
                }
            }
        }
        ui.data_mut(|d| {
            if let Some(aim) = keep {
                d.insert_temp(aim_id, aim);
            } else {
                d.remove::<(egui::Pos2, f64)>(aim_id);
            }
        });
        // egui closed it because another item is hovered: undo that while aiming.
        if before == Some(id) && after.is_none() && keep.is_some() && !clicked && !response.clicked() {
            MenuState::from_ui(ui, |state, _| state.open_item = Some(id));
            ui.ctx().request_repaint();
        }
    }
    egui::InnerResponse::new(inner.map(|i| i.inner), response)
}

/// How long the pointer may rest on the way to an open submenu before the item
/// under it takes over (seconds).
pub const SUBMENU_GRACE: f64 = 0.35;

/// Is `pointer`, coming from `anchor`, inside the triangle from (a little behind)
/// `anchor` to the near edge of `popup`?
fn aiming(anchor: egui::Pos2, pointer: egui::Pos2, popup: Rect) -> bool {
    let (edge, back) = if anchor.x <= popup.left() {
        (popup.left(), -8.0)
    } else if anchor.x >= popup.right() {
        (popup.right(), 8.0)
    } else {
        return false;
    };
    let apex = pos2(anchor.x + back, anchor.y);
    let (a, b) = (pos2(edge, popup.top() - 4.0), pos2(edge, popup.bottom() + 4.0));
    let cross = |o: egui::Pos2, u: egui::Pos2, v: egui::Pos2| (u.x - o.x) * (v.y - o.y) - (u.y - o.y) * (v.x - o.x);
    let (d1, d2, d3) = (cross(apex, a, pointer), cross(a, b, pointer), cross(b, apex, pointer));
    let (neg, pos) = (d1 < 0.0 || d2 < 0.0 || d3 < 0.0, d1 > 0.0 || d2 > 0.0 || d3 > 0.0);
    !(neg && pos)
}

/// Names a hand-drawn widget for accessibility (VoiceOver, and test scripts).
pub fn name(response: &Response, typ: egui::WidgetType, label: impl Into<String>) {
    let (enabled, label) = (response.enabled(), label.into());
    response.widget_info(|| egui::WidgetInfo::labeled(typ, enabled, &label));
}

/// Small square toggle like a mixer's activator or solo button.
pub fn toggle(ui: &mut Ui, on: bool, label: &str, on_color: Color32, size: egui::Vec2) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let (enabled, text) = (response.enabled(), label.to_string());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, on, &text));
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
    name(&response, egui::WidgetType::Button, label);
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
        Unit::Hertz if value < 10.0 => format!("{value:.2} Hz"),
        Unit::Hertz if value < 100.0 => format!("{value:.1} Hz"),
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
        Unit::Bpm => format!("{value:.1} BPM"),
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

/// An instrument's editor window: sized to its contents when it fits on screen;
/// otherwise as big as the screen allows, with scroll bars. (Its full size is
/// measured while it fits, or on the first frame.)
pub fn editor_window<R>(ctx: &egui::Context, title: impl Into<egui::WidgetText>, id: egui::Id, open: &mut bool, pos: egui::Pos2, contents: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let screen = ctx.content_rect().size();
    let full_id = id.with("full_size");
    let full: Option<egui::Vec2> = ctx.data(|d| d.get_temp(full_id));
    let fits = full.is_none_or(|f| f.x <= screen.x - 16.0 && f.y <= screen.y - 16.0);
    let mut window = egui::Window::new(title).id(id).open(open).collapsible(false).resizable(false).default_pos(pos);
    if !fits {
        // The contents' area: the screen less the title bar and margins.
        window = window.scroll(true).fixed_size(screen - vec2(40.0, 72.0));
    }
    let response = window.show(ctx, contents)?;
    if fits {
        let size = response.response.rect.size().ceil();
        if full.is_none_or(|f| (f - size).length() > 1.0) {
            ctx.data_mut(|d| d.insert_temp(full_id, size));
            ctx.request_repaint();
        }
    }
    response.inner
}

/// Text over a graph: on a dark backdrop, so lines and curves drawn before it
/// don't run through it.
pub fn backdrop_text(painter: &egui::Painter, pos: egui::Pos2, align: Align2, text: impl ToString, font: egui::FontId, color: Color32) {
    let galley = painter.layout_no_wrap(text.to_string(), font, color);
    let rect = align.anchor_size(pos, galley.size());
    painter.rect_filled(rect.expand2(vec2(2.0, 0.5)), 2.0, Color32::from_black_alpha(190));
    painter.galley(rect.min, galley, color);
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

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2};
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A right-click menu as wide as Pulse's: «Acompañar ⏵» (six items), «Tarareo ⏵», «Otra cosa».
    struct Menus {
        ctx: egui::Context,
        time: f64,
        rects: RefCell<HashMap<String, Rect>>,
        clicked: RefCell<Vec<String>>,
    }

    impl Menus {
        fn new() -> Self {
            Self { ctx: egui::Context::default(), time: 0.0, rects: Default::default(), clicked: Default::default() }
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))), time: Some(self.time), events, ..Default::default() };
            let (rects, clicked) = (&self.rects, &self.clicked);
            let item = |ui: &mut Ui, name: &str| {
                let r = ui.button(name);
                rects.borrow_mut().insert(name.into(), r.rect);
                if r.clicked() {
                    clicked.borrow_mut().push(name.into());
                }
            };
            let _ = self.ctx.run_ui(raw, |ui| {
                let target = ui.button("Clip");
                rects.borrow_mut().insert("Clip".into(), target.rect);
                target.context_menu(|ui| {
                    ui.set_min_width(220.0);
                    let r = menu_button(ui, "Acompañar", |ui| {
                        for name in ["Bajo", "Bajo: corcheas", "Pad", "Arpegio", "Acordes", "Bajo: contratiempo"] {
                            item(ui, name);
                        }
                    });
                    rects.borrow_mut().insert("Acompañar".into(), r.response.rect);
                    let r = menu_button(ui, "Tarareo", |ui| {
                        for name in ["Techno", "House", "Trap"] {
                            item(ui, name);
                        }
                    });
                    rects.borrow_mut().insert("Tarareo".into(), r.response.rect);
                    item(ui, "Otra cosa");
                });
            });
        }

        fn idle(&mut self, n: usize) {
            for _ in 0..n {
                self.frame(Vec::new());
            }
        }

        fn at(&self, name: &str) -> Pos2 {
            self.rects.borrow()[name].center()
        }

        /// Shown in the next frame?
        fn shows(&mut self, name: &str) -> bool {
            self.rects.borrow_mut().clear();
            self.idle(1);
            self.rects.borrow().contains_key(name)
        }

        /// From `from` to `to` in `steps` frames, a straight line.
        fn glide(&mut self, from: Pos2, to: Pos2, steps: usize) {
            for i in 1..=steps {
                self.frame(vec![Event::PointerMoved(from + (to - from) * (i as f32 / steps as f32))]);
            }
        }

        fn click(&mut self, at: Pos2, button: PointerButton) {
            for pressed in [true, false] {
                self.frame(vec![Event::PointerButton { pos: at, button, pressed, modifiers: Modifiers::NONE }]);
            }
        }

        /// The menu open, the pointer resting on «Acompañar» and its submenu open.
        fn open(&mut self) {
            self.idle(2);
            let clip = self.at("Clip");
            self.frame(vec![Event::PointerMoved(clip)]);
            self.click(clip, PointerButton::Secondary);
            self.idle(3);
            let acompanar = self.at("Acompañar");
            self.glide(clip, acompanar, 3);
            self.idle(10);
            assert!(self.shows("Bajo: contratiempo"), "«Acompañar» abre su submenú");
            // Reading the submenu: a still pointer sends no events, so no frames.
            self.time += 0.6;
        }
    }

    #[test]
    fn a_submenu_stays_open_on_the_way_to_it() {
        let mut m = Menus::new();
        m.open();
        let (from, to) = (m.at("Acompañar"), m.at("Bajo: contratiempo"));
        let tarareo = m.rects.borrow()["Tarareo"];
        assert!(to.y > tarareo.top(), "the way down crosses «Tarareo»: {to:?} {tarareo:?}");
        // The first move after a rest lands on «Tarareo» before egui knows where
        // the pointer is heading (no recent movement): egui alone swaps the
        // submenus right then.
        let t = (tarareo.center().y - from.y) / (to.y - from.y);
        let over = from + (to - from) * t;
        assert!(tarareo.contains(over), "the way crosses «Tarareo»: {over:?} {tarareo:?}");
        m.glide(from, over, 1);
        for _ in 0..(SUBMENU_GRACE * 60.0) as usize / 2 {
            assert!(m.shows("Bajo: contratiempo"), "«Acompañar» sigue abierto en {over:?}");
            assert!(!m.rects.borrow().contains_key("Techno"), "«Tarareo» no se abre al cruzarlo");
        }
        m.glide(over, to, 3);
        assert!(m.shows("Bajo: contratiempo"));
        m.click(to, PointerButton::Primary);
        assert_eq!(*m.clicked.borrow(), vec!["Bajo: contratiempo".to_string()]);
    }

    #[test]
    fn resting_on_another_submenu_opens_it() {
        let mut m = Menus::new();
        m.open();
        let (from, to) = (m.at("Acompañar"), m.at("Tarareo"));
        m.glide(from, to, 4);
        // A moment of rest: «Tarareo» wins.
        m.idle((SUBMENU_GRACE * 60.0) as usize + 10);
        assert!(m.shows("Techno"), "parado en «Tarareo» se abre su submenú");
        assert!(!m.shows("Bajo: contratiempo"));
    }

    #[test]
    fn going_away_from_a_submenu_closes_it_at_once() {
        let mut m = Menus::new();
        m.open();
        // Straight down to «Otra cosa», left of the submenu: no triangle.
        let (from, to) = (m.at("Acompañar"), m.at("Otra cosa") - vec2(20.0, 0.0));
        m.glide(from, to, 3);
        m.idle(2);
        assert!(!m.shows("Bajo: contratiempo"), "lejos del submenú, se cierra");
    }

    #[test]
    fn the_triangle_points_at_the_submenu() {
        let popup = Rect::from_min_max(pos2(200.0, 100.0), pos2(300.0, 200.0));
        let anchor = pos2(150.0, 110.0);
        assert!(aiming(anchor, pos2(170.0, 130.0), popup));
        assert!(aiming(anchor, pos2(195.0, 195.0), popup));
        assert!(!aiming(anchor, pos2(150.0, 140.0), popup), "straight down is not towards it");
        assert!(!aiming(anchor, pos2(120.0, 110.0), popup), "nor is going back");
        // A submenu opened to the left.
        let left = Rect::from_min_max(pos2(0.0, 100.0), pos2(100.0, 200.0));
        assert!(aiming(anchor, pos2(130.0, 130.0), left));
    }

    /// Every submenu goes through [`menu_button`].
    #[test]
    fn no_menu_uses_egui_menu_button_directly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(!text.contains(concat!("ui.menu", "_button(")), "{}: usa widgets::menu_button", path.display());
        }
    }
}
