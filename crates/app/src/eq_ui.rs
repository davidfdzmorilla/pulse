//! The EQ Eight's device: its curve, edited by dragging the bands' points, over
//! the spectrum of what comes out of it.
//!
//! Every move goes through the device's parameters (frequency, gain and Q of
//! each band), so the points follow automation and override it like the knobs.

use crate::devices::Action;
use crate::theme::{self, ACCENT, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke};
use engine::model::{EqEightParams, EqShape, Param, EQ_BANDS, EQ_BAND_PARAMS, EQ_EIGHT_PARAMS};
use engine::{Analyzer, Target, ANALYZER_SAMPLES};

/// The graph's size.
pub const GRAPH: egui::Vec2 = vec2(340.0, 104.0);
/// How wide the device is.
pub const WIDTH: f32 = 700.0;

const LOW_HZ: f32 = 20.0;
const HIGH_HZ: f32 = 20_000.0;
/// The gain the graph shows, up and down.
const RANGE_DB: f32 = 15.0;
/// Points of the curve and the spectrum, across the graph.
const COLUMNS: usize = 170;
/// The quietest the spectrum shows.
const FLOOR_DB: f32 = -84.0;
/// How fast the spectrum falls when the sound does, dB a second.
const FALL_DB: f32 = 48.0;

/// Each band's colour, as on its point and its number.
pub const COLORS: [Color32; EQ_BANDS] = [
    Color32::from_rgb(0xf0, 0x5a, 0x5a),
    Color32::from_rgb(0xf0, 0x9a, 0x46),
    Color32::from_rgb(0xe6, 0xd0, 0x4a),
    Color32::from_rgb(0x7c, 0xd8, 0x5e),
    Color32::from_rgb(0x4a, 0xd0, 0xc0),
    Color32::from_rgb(0x5a, 0xa8, 0xf0),
    Color32::from_rgb(0x9a, 0x7c, 0xf0),
    Color32::from_rgb(0xe0, 0x6c, 0xd0),
];

/// Where frequency `hz` is across the graph, 0..1.
fn x_of(hz: f32) -> f32 {
    ((hz / LOW_HZ).ln() / (HIGH_HZ / LOW_HZ).ln()).clamp(0.0, 1.0)
}

fn hz_at(x: f32) -> f32 {
    LOW_HZ * (HIGH_HZ / LOW_HZ).powf(x.clamp(0.0, 1.0))
}

/// The frequency of column `c`.
fn column_hz(c: usize) -> f32 {
    hz_at(c as f32 / (COLUMNS - 1) as f32)
}

/// The spectrum the analyzer hears, column by column, falling slowly.
pub struct Spectrum {
    samples: Vec<f32>,
    written: u64,
    /// dB at each column.
    levels: Vec<f32>,
    /// When it was last updated (seconds).
    at: f64,
}

impl Default for Spectrum {
    fn default() -> Self {
        Self { samples: vec![0.0; ANALYZER_SAMPLES], written: 0, levels: vec![FLOOR_DB; COLUMNS], at: 0.0 }
    }
}

impl Spectrum {
    /// Takes what the analyzer heard since the last time; `now` in seconds.
    pub fn update(&mut self, analyzer: &Analyzer, sample_rate: f32, now: f64) {
        let dt = (now - self.at).clamp(0.0, 0.25) as f32;
        self.at = now;
        let written = analyzer.read(&mut self.samples);
        // Nothing new: the sound stopped reaching it.
        let fresh = if written != self.written { spectrum_db(&self.samples, sample_rate) } else { vec![FLOOR_DB; COLUMNS] };
        self.written = written;
        for (level, new) in self.levels.iter_mut().zip(fresh) {
            *level = new.max(*level - FALL_DB * dt).max(FLOOR_DB);
        }
    }

    /// Whether it shows any sound.
    pub fn sounding(&self) -> bool {
        self.levels.iter().any(|&l| l > FLOOR_DB)
    }
}

/// The level of `samples` at each column's frequency, in dB (a full-scale sine
/// reads 0).
fn spectrum_db(samples: &[f32], sample_rate: f32) -> Vec<f32> {
    let n = samples.len();
    let window = |i: usize| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
    let mut re: Vec<f32> = samples.iter().enumerate().map(|(i, &s)| if s.is_finite() { s * window(i) } else { 0.0 }).collect();
    let mut im = vec![0.0; n];
    engine::fft(&mut re, &mut im);
    // The window halves a sine's amplitude, and half of it is in the mirror bins.
    let magnitude: Vec<f32> = (0..n / 2).map(|k| 4.0 * (re[k] * re[k] + im[k] * im[k]).sqrt() / n as f32).collect();
    let bin_hz = sample_rate / n as f32;
    let step = (HIGH_HZ / LOW_HZ).powf(0.5 / (COLUMNS - 1) as f32);
    (0..COLUMNS)
        .map(|c| {
            let hz = column_hz(c);
            let (low, high) = ((hz / step / bin_hz).ceil() as usize, (hz * step / bin_hz).floor() as usize);
            let level = if high >= low + 1 {
                // Several bins in the column: the loudest.
                magnitude[low.min(n / 2 - 1)..=high.min(n / 2 - 1)].iter().fold(0f32, |a, &b| a.max(b))
            } else {
                // Fewer than one (the lows): between the two nearest.
                let at = (hz / bin_hz).min((n / 2 - 2) as f32);
                let (k, t) = (at.floor() as usize, at.fract());
                magnitude[k] * (1.0 - t) + magnitude[k + 1] * t
            };
            20.0 * level.max(1e-9).log10()
        })
        .collect()
}

/// The band chosen in the device (its controls are shown).
fn chosen_id(target: Target, device: usize) -> egui::Id {
    egui::Id::new(("eq8_band", target, device))
}

/// Draws the device's body. `p` is the device's own copy (its on/off and
/// shapes change on it); `shown` its parameters as they sound now (with
/// automation); knob and point moves go to `actions`.
pub(crate) fn body(
    ui: &mut egui::Ui,
    target: Target,
    device: usize,
    p: &mut EqEightParams,
    shown: &EqEightParams,
    automation: &dyn Fn(usize) -> Option<bool>,
    spectrum: Option<&Spectrum>,
    sample_rate: f32,
    actions: &mut Vec<Action>,
) {
    let id = chosen_id(target, device);
    let mut chosen: usize = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(2).min(EQ_BANDS - 1);
    let param = |band: usize, k: usize| Param::Device { device, param: band * EQ_BAND_PARAMS + k };
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        if let Some(band) = graph(ui, id, p, shown, spectrum, sample_rate, chosen, &param, actions) {
            chosen = band;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (b, band) in p.bands.iter().enumerate() {
                let color = if band.on { COLORS[b] } else { TEXT_DIM };
                let text = egui::RichText::new(format!("{}", b + 1)).color(color).size(11.0);
                let button = egui::Button::selectable(chosen == b, text).min_size(vec2(GRAPH.x / EQ_BANDS as f32 - 2.0, 18.0));
                if ui.add(button).on_hover_text(format!("Banda {}: {}{}", b + 1, band.shape.label(), if band.on { "" } else { " (apagada)" })).clicked() {
                    chosen = b;
                }
            }
        });
    });

    let band = &mut p.bands[chosen];
    ui.vertical(|ui| {
        ui.set_width(104.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("Banda {}", chosen + 1)).color(COLORS[chosen]).strong().size(11.0));
            ui.checkbox(&mut band.on, "").on_hover_text("Encender o apagar la banda (también con doble clic en su punto)");
        });
        egui::ComboBox::from_id_salt(("eq8_shape", target, device)).selected_text(band.shape.label()).width(100.0).show_ui(ui, |ui| {
            for shape in EqShape::ALL {
                ui.selectable_value(&mut band.shape, shape, shape.label());
            }
        });
        let hint = match band.shape {
            EqShape::LowCut48 | EqShape::LowCut12 => "Quita lo que hay por debajo de la frecuencia (48 o 12 dB por octava). Q por encima de 0,71 resalta el corte",
            EqShape::HighCut48 | EqShape::HighCut12 => "Quita lo que hay por encima de la frecuencia (48 o 12 dB por octava). Q por encima de 0,71 resalta el corte",
            EqShape::LowShelf => "Sube o baja todo lo que hay por debajo de la frecuencia",
            EqShape::HighShelf => "Sube o baja todo lo que hay por encima de la frecuencia",
            EqShape::Bell => "Sube o baja alrededor de la frecuencia; Q, cuánto de estrecho",
            EqShape::Notch => "Quita una frecuencia muy concreta (un zumbido, un pitido); Q, cuánto de estrecho",
        };
        ui.label(egui::RichText::new(hint).color(TEXT_DIM).size(9.5));
    });

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (k, name) in ["Freq", "Gain", "Q"].into_iter().enumerate() {
            let index = chosen * EQ_BAND_PARAMS + k;
            let value = shown.bands[chosen].get(k);
            if let Some(v) = widgets::param_knob(ui, &EQ_EIGHT_PARAMS[index], value, name, 30.0, automation(index)) {
                actions.push(Action::Param(param(chosen, k), v));
            }
        }
        let output = EQ_BANDS * EQ_BAND_PARAMS;
        if let Some(v) = widgets::param_knob(ui, &EQ_EIGHT_PARAMS[output], shown.output_db, "Output", 30.0, automation(output)) {
            actions.push(Action::Param(Param::Device { device, param: output }, v));
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, chosen));
}

trait BandValue {
    fn get(&self, k: usize) -> f32;
}

impl BandValue for engine::model::EqBand {
    fn get(&self, k: usize) -> f32 {
        match k {
            0 => self.freq,
            1 => self.gain_db,
            _ => self.q,
        }
    }
}

/// Where band `b`'s point is in `rect`.
fn point(rect: Rect, p: &EqEightParams, b: usize) -> Pos2 {
    let band = &p.bands[b];
    let gain = if band.shape.has_gain() { band.gain_db } else { 0.0 };
    pos2(rect.left() + x_of(band.freq) * rect.width(), y_of(rect, gain))
}

fn y_of(rect: Rect, db: f32) -> f32 {
    rect.center().y - (db / RANGE_DB).clamp(-1.0, 1.0) * rect.height() * 0.5
}

/// The graph; returns the band clicked or dragged, if any.
#[allow(clippy::too_many_arguments)]
fn graph(
    ui: &mut egui::Ui,
    id: egui::Id,
    p: &mut EqEightParams,
    shown: &EqEightParams,
    spectrum: Option<&Spectrum>,
    sample_rate: f32,
    chosen: usize,
    param: &dyn Fn(usize, usize) -> Param,
    actions: &mut Vec<Action>,
) -> Option<usize> {
    let (outer, response) = ui.allocate_exact_size(GRAPH, Sense::click_and_drag());
    let painter = ui.painter_at(outer);
    painter.rect_filled(outer, 2.0, theme::BG);
    // Room for the points at the edges.
    let rect = outer.shrink2(vec2(0.0, 5.0));
    let x_at = |hz: f32| rect.left() + x_of(hz) * rect.width();
    let grid = Stroke::new(1.0, Color32::from_gray(38));
    for hz in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0] {
        painter.line_segment([pos2(x_at(hz), outer.top()), pos2(x_at(hz), outer.bottom())], grid);
    }
    for db in [-12.0, -6.0, 6.0, 12.0] {
        painter.line_segment([pos2(rect.left(), y_of(rect, db)), pos2(rect.right(), y_of(rect, db))], grid);
    }
    painter.line_segment([pos2(rect.left(), y_of(rect, 0.0)), pos2(rect.right(), y_of(rect, 0.0))], Stroke::new(1.0, Color32::from_gray(60)));
    for (hz, label) in [(100.0, "100"), (1000.0, "1k"), (10000.0, "10k")] {
        painter.text(pos2(x_at(hz) + 2.0, outer.bottom() - 1.0), Align2::LEFT_BOTTOM, label, egui::FontId::proportional(9.0), Color32::from_gray(90));
    }

    // The spectrum, from the floor up.
    if let Some(spectrum) = spectrum.filter(|s| s.sounding()) {
        let mut mesh = egui::epaint::Mesh::default();
        let fill = Color32::from_rgba_unmultiplied(0x8c, 0x8c, 0x8c, 70);
        for (c, &level) in spectrum.levels.iter().enumerate() {
            let x = x_at(column_hz(c));
            let top = outer.bottom() - (level - FLOOR_DB) / -FLOOR_DB * outer.height();
            mesh.colored_vertex(pos2(x, outer.bottom()), fill);
            mesh.colored_vertex(pos2(x, top.max(outer.top())), fill);
            if c > 0 {
                let i = 2 * c as u32;
                mesh.add_triangle(i - 2, i - 1, i);
                mesh.add_triangle(i - 1, i, i + 1);
            }
        }
        painter.add(mesh);
    }

    // The chosen band on its own, then all of them together.
    let band = shown.bands[chosen];
    if band.on {
        let line: Vec<Pos2> = (0..COLUMNS).map(|c| pos2(x_at(column_hz(c)), y_of(rect, engine::eq_band_response_db(&band, sample_rate, column_hz(c))))).collect();
        painter.add(egui::Shape::line(line, Stroke::new(1.0, theme::shade(COLORS[chosen], 0.7))));
    }
    let line: Vec<Pos2> = (0..COLUMNS).map(|c| pos2(x_at(column_hz(c)), y_of(rect, engine::eq_eight_response_db(shown, sample_rate, column_hz(c))))).collect();
    painter.add(egui::Shape::line(line, Stroke::new(1.6, ACCENT)));

    let hover = response.hover_pos();
    let nearest = |pos: Pos2| {
        (0..EQ_BANDS)
            .map(|b| (point(rect, shown, b).distance(pos), b))
            // The chosen band first when points overlap.
            .min_by(|a, b| (a.0 - if a.1 == chosen { 0.5 } else { 0.0 }).total_cmp(&(b.0 - if b.1 == chosen { 0.5 } else { 0.0 })))
            .filter(|&(d, _)| d < 9.0)
            .map(|(_, b)| b)
    };
    let hovered = hover.and_then(nearest);
    for b in (0..EQ_BANDS).rev() {
        let at = point(rect, shown, b);
        let radius = if hovered == Some(b) || chosen == b { 6.5 } else { 5.5 };
        if shown.bands[b].on {
            painter.circle_filled(at, radius, COLORS[b]);
            painter.text(at, Align2::CENTER_CENTER, format!("{}", b + 1), egui::FontId::proportional(8.5), theme::BG);
        } else {
            painter.circle_stroke(at, radius - 0.5, Stroke::new(1.0, theme::shade(COLORS[b], 0.6)));
            painter.text(at, Align2::CENTER_CENTER, format!("{}", b + 1), egui::FontId::proportional(8.5), theme::shade(COLORS[b], 0.6));
        }
        if chosen == b {
            painter.circle_stroke(at, radius + 1.5, Stroke::new(1.0, Color32::WHITE));
        }
    }

    let mut picked = None;
    let grab_id = id.with("grab");
    if response.drag_started() {
        // Where the button went down: a drag only starts after the pointer has moved.
        let origin = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
        let grab = origin.and_then(nearest);
        ui.ctx().data_mut(|d| d.insert_temp(grab_id, grab));
    }
    if response.dragged() {
        let grab: Option<usize> = ui.ctx().data(|d| d.get_temp(grab_id)).flatten();
        if let (Some(b), Some(pos)) = (grab, response.interact_pointer_pos()) {
            picked = Some(b);
            let band = shown.bands[b];
            if ui.input(|i| i.modifiers.alt) {
                // ⌥: up and down, the Q.
                let q = (band.q * (-response.drag_delta().y / 60.0).exp2()).clamp(EQ_EIGHT_PARAMS[2].min, EQ_EIGHT_PARAMS[2].max);
                actions.push(Action::Param(param(b, 2), q));
            } else {
                let hz = hz_at((pos.x - rect.left()) / rect.width()).clamp(EQ_EIGHT_PARAMS[0].min, EQ_EIGHT_PARAMS[0].max);
                actions.push(Action::Param(param(b, 0), hz));
                if band.shape.has_gain() {
                    let db = ((rect.center().y - pos.y) / (rect.height() * 0.5) * RANGE_DB).clamp(-RANGE_DB, RANGE_DB);
                    actions.push(Action::Param(param(b, 1), db));
                }
            }
        }
    }
    if response.clicked() || response.double_clicked() {
        if let Some(b) = response.interact_pointer_pos().and_then(nearest) {
            picked = Some(b);
            if response.double_clicked() {
                p.bands[b].on = !p.bands[b].on;
            }
        }
    }
    let tip = match hovered {
        Some(b) => {
            let band = shown.bands[b];
            let gain = if band.shape.has_gain() { format!(" · {:+.1} dB", band.gain_db) } else { String::new() };
            format!("Banda {} · {} · {}{gain} · Q {:.2}", b + 1, band.shape.label(), widgets::format_param(&EQ_EIGHT_PARAMS[0], band.freq), band.q)
        }
        None => theme::keys("Arrastra un punto: izquierda y derecha, la frecuencia; arriba y abajo, la ganancia · con ⌥, arriba y abajo cambia la Q · doble clic enciende o apaga la banda"),
    };
    response.on_hover_text(tip);
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine reads its level at its frequency, and little far from it.
    #[test]
    fn the_spectrum_shows_a_sine_where_it_is() {
        let sr = 48_000.0;
        let samples: Vec<f32> = (0..ANALYZER_SAMPLES).map(|i| 0.5 * (std::f32::consts::TAU * 1000.0 * i as f32 / sr).sin()).collect();
        let levels = spectrum_db(&samples, sr);
        let at = |hz: f32| levels[(0..COLUMNS).min_by(|&a, &b| (column_hz(a) - hz).abs().total_cmp(&(column_hz(b) - hz).abs())).unwrap()];
        assert!((at(1000.0) + 6.0).abs() < 1.5, "half scale is −6 dB: {}", at(1000.0));
        assert!(at(100.0) < -60.0 && at(10000.0) < -60.0, "{} {}", at(100.0), at(10000.0));
        let silence = spectrum_db(&[0.0; ANALYZER_SAMPLES], sr);
        assert!(silence.iter().all(|&l| l <= FLOOR_DB));
    }

    /// Dragging a band's point moves its frequency and gain, a double click
    /// turns a band on, and the analyzer listens to that EQ.
    #[test]
    fn points_drag_and_switch_bands() {
        use crate::app::{Audio, PulseApp};
        use eframe::egui::{Event, Modifiers, PointerButton, RawInput};
        use engine::model::{Device, Effect};
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let (audio, _engine) = Audio::detached();
        app.audio = audio;
        app.add_device(Target::Track(0), Device::new(Effect::EqEight(Default::default())));
        let index = app.project.tracks[0].devices.len() - 1;
        app.selected_target = Target::Track(0);
        let eq = |app: &PulseApp| match app.project.tracks[0].devices[index].effect {
            Effect::EqEight(p) => p,
            _ => unreachable!(),
        };
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut frame = |app: &mut PulseApp, events: Vec<Event>| {
            time += 0.05;
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(2400.0, 400.0))), time: Some(time), events, ..Default::default() };
            let output = ctx.run_ui(input, |ui| crate::devices::show(app, ui));
            let mut texts = Vec::new();
            fn collect(shape: &egui::Shape, texts: &mut Vec<(String, Pos2)>) {
                match shape {
                    egui::Shape::Text(t) => texts.push((t.galley.text().to_string(), Rect::from_min_size(t.pos, t.galley.size()).center())),
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
                    _ => {}
                }
            }
            output.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
            texts
        };
        // A band's point: its number, above the row of buttons with the same.
        let dot = |texts: &[(String, Pos2)], band: usize| {
            texts.iter().filter(|(t, _)| *t == format!("{band}")).map(|(_, at)| *at).min_by(|a, b| a.y.total_cmp(&b.y)).unwrap()
        };
        let press = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(&mut app, vec![]);
        let texts = frame(&mut app, vec![]);
        assert_eq!(app.audio.state().unwrap().analyzer.watching(), Some((Target::Track(0), index)), "the analyzer listens after it");
        assert!(texts.iter().any(|(t, _)| t.starts_with("Sube o baja alrededor")), "the chosen band (3, a bell) and what it does");
        assert!(texts.iter().any(|(t, _)| t == "Output"));

        let from = dot(&texts, 3);
        let to = from + vec2(80.0, -20.0);
        frame(&mut app, vec![Event::PointerMoved(from)]);
        frame(&mut app, vec![press(from, true)]);
        for k in 1..=4 {
            frame(&mut app, vec![Event::PointerMoved(from + (to - from) * k as f32 / 4.0)]);
        }
        frame(&mut app, vec![press(to, false)]);
        let band = eq(&app).bands[2];
        assert!(band.freq > 1150.0 && band.freq < 1400.0, "80 px right of 250 Hz, 2.3 octaves higher: {} Hz", band.freq);
        assert!(band.gain_db > 5.0 && band.gain_db < 8.0, "higher up, louder: {} dB", band.gain_db);
        assert!(!eq(&app).bands[0].on);

        let texts = frame(&mut app, vec![]);
        let first = dot(&texts, 1);
        frame(&mut app, vec![Event::PointerMoved(first)]);
        frame(&mut app, vec![press(first, true), press(first, false)]);
        frame(&mut app, vec![press(first, true), press(first, false)]);
        frame(&mut app, vec![]);
        assert!(eq(&app).bands[0].on, "a double click turns the low cut on");
        assert_eq!(eq(&app).bands[2], band, "the others stay");
    }

    #[test]
    fn the_frequency_axis_goes_both_ways() {
        for hz in [20.0, 63.0, 1000.0, 15000.0] {
            assert!((hz_at(x_of(hz)) - hz).abs() < hz * 1e-4);
        }
    }
}
