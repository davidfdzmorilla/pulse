//! Device chain view: the track's instrument followed by its audio effects.

use crate::app::PulseApp;
use crate::project::drum_name;
use crate::theme::{self, ACCENT, CONTROL, SURFACE, TEXT_DIM};
use crate::widgets::{Knob, Taper};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine::model::{
    drum_note, AutoFilterParams, CompressorParams, DelayParams, Device, Effect, Eq3Params, FilterMode,
    InstrumentKind, ReverbParams, SaturatorParams, SynthParams, Waveform,
};
use engine::{Command, Target};

const DEVICE_HEIGHT: f32 = 168.0;

fn hz(v: f32) -> String {
    if v >= 1000.0 {
        format!("{:.2} kHz", v / 1000.0)
    } else {
        format!("{v:.0} Hz")
    }
}

fn db(v: f32) -> String {
    if v <= -39.9 {
        "-inf dB".into()
    } else {
        format!("{v:+.1} dB")
    }
}

fn percent(v: f32) -> String {
    format!("{:.0} %", v * 100.0)
}

fn seconds(v: f32) -> String {
    if v < 1.0 {
        format!("{:.0} ms", v * 1000.0)
    } else {
        format!("{v:.2} s")
    }
}

fn beats_label(beats: f64) -> String {
    match beats {
        b if b == 0.25 => "1/16".into(),
        b if b == 0.5 => "1/8".into(),
        b if b == 0.75 => "3/16".into(),
        b if b == 1.0 => "1/4".into(),
        b if b == 1.5 => "3/8".into(),
        b if b == 2.0 => "1/2".into(),
        b if b == 4.0 => "1 Compás".into(),
        b if b == 8.0 => "2 Compases".into(),
        b if b == 16.0 => "4 Compases".into(),
        b => format!("{b} beats"),
    }
}

enum Action {
    Remove(usize),
    Move(usize, usize),
    Changed(usize),
}

/// Draws a device frame with a title bar and returns the body's UI result.
fn device_frame<R>(
    ui: &mut egui::Ui,
    title: &str,
    width: f32,
    enabled: Option<&mut bool>,
    header_extra: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let (rect, _) = ui.allocate_exact_size(vec2(width, DEVICE_HEIGHT), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, SURFACE);
    let title_rect = Rect::from_min_size(rect.min, vec2(width, 20.0));
    painter.rect_filled(title_rect, 3.0, CONTROL);

    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(title_rect.shrink2(vec2(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.spacing_mut().item_spacing.x = 4.0;
    if let Some(enabled) = enabled {
        let (dot, response) = child.allocate_exact_size(vec2(12.0, 12.0), Sense::click());
        child.painter().circle_filled(dot.center(), 5.0, if *enabled { ACCENT } else { theme::BG });
        child.painter().circle_stroke(dot.center(), 5.0, Stroke::new(1.0, TEXT_DIM));
        if response.on_hover_text("Activar / desactivar").clicked() {
            *enabled = !*enabled;
        }
    }
    child.label(egui::RichText::new(title).strong().size(11.5));
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), header_extra);

    let body_rect = Rect::from_min_max(pos2(rect.left() + 8.0, title_rect.bottom() + 6.0), rect.max - vec2(8.0, 6.0));
    let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body_rect).layout(egui::Layout::left_to_right(egui::Align::Min)));
    body_ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
    body(&mut body_ui)
}

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let target = app.selected_target;
    let mut actions = Vec::new();
    let mut instrument_changed = false;
    let mut preview = None;

    egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Some(t) = match target {
                Target::Track(t) if !app.project.tracks[t].is_audio() => Some(t),
                _ => None,
            } {
                let instrument = &mut app.project.tracks[t].instrument;
                let (changed, pad) = instrument_device(ui, instrument);
                instrument_changed = changed;
                preview = pad.map(|pitch| (t, pitch));
            }

            let count = app.devices(target).len();
            for index in 0..count {
                let device = &mut app.devices_mut(target)[index];
                let before = *device;
                let mut remove = false;
                let mut move_to = None;
                let width = effect_width(&device.effect);
                let title = device.effect.label();
                let Device { effect, enabled } = device;
                device_frame(
                    ui,
                    title,
                    width,
                    Some(enabled),
                    |ui| {
                        if ui.small_button("✕").on_hover_text("Eliminar").clicked() {
                            remove = true;
                        }
                        if index + 1 < count && ui.small_button("▶").on_hover_text("Mover a la derecha").clicked() {
                            move_to = Some(index + 1);
                        }
                        if index > 0 && ui.small_button("◀").on_hover_text("Mover a la izquierda").clicked() {
                            move_to = Some(index - 1);
                        }
                    },
                    |ui| effect_controls(ui, effect),
                );
                if remove {
                    actions.push(Action::Remove(index));
                } else if let Some(to) = move_to {
                    actions.push(Action::Move(index, to));
                } else if *device != before {
                    actions.push(Action::Changed(index));
                }
            }

            let (rect, _) = ui.allocate_exact_size(vec2(170.0, DEVICE_HEIGHT), Sense::hover());
            ui.painter().rect_stroke(rect, 3.0, Stroke::new(1.0, theme::CONTROL), egui::StrokeKind::Inside);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Añade efectos\ndesde el navegador",
                theme::small(),
                TEXT_DIM,
            );
        });
    });

    if instrument_changed {
        if let Target::Track(t) = target {
            let kind = app.project.tracks[t].instrument;
            app.audio.send(Command::SetInstrument { track: t, kind });
        }
    }
    if let Some((track, pitch)) = preview {
        app.audio.send(Command::PreviewNote { track, pitch, velocity: 0.9 });
    }
    for action in actions {
        match action {
            Action::Remove(i) => app.remove_device(target, i),
            Action::Move(from, to) => app.move_device(target, from, to),
            Action::Changed(i) => app.update_device(target, i),
        }
    }
}

fn effect_width(effect: &Effect) -> f32 {
    // Knobs are 46 px wide plus spacing; option columns (combos, radios) take about 96 px.
    let (knobs, columns) = match effect {
        Effect::Eq3(_) => (5, 0),
        Effect::Compressor(_) => (5, 0),
        Effect::Delay(_) => (3, 1),
        Effect::Reverb(_) => (4, 0),
        Effect::Saturator(_) => (3, 0),
        Effect::AutoFilter(_) => (3, 2),
    };
    knobs as f32 * 52.0 + columns as f32 * 100.0 + 24.0
}

fn knob(ui: &mut egui::Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, default: f32, label: &str, format: impl Fn(f32) -> String) {
    Knob::new(value, range, default, label).format(format).show(ui);
}

fn log_knob(ui: &mut egui::Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, default: f32, label: &str, format: impl Fn(f32) -> String) {
    Knob::new(value, range, default, label).taper(Taper::Log).format(format).show(ui);
}

fn beats_combo(ui: &mut egui::Ui, id: &str, value: &mut f64, options: &[f64]) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(beats_label(*value))
        .width(84.0)
        .show_ui(ui, |ui| {
            for &option in options {
                ui.selectable_value(value, option, beats_label(option));
            }
        });
}

fn effect_controls(ui: &mut egui::Ui, effect: &mut Effect) {
    let id = ui.id();
    match effect {
        Effect::Eq3(p) => {
            let d = Eq3Params::default();
            knob(ui, &mut p.low_db, -40.0..=6.0, 0.0, "Low", db);
            knob(ui, &mut p.mid_db, -40.0..=6.0, 0.0, "Mid", db);
            knob(ui, &mut p.high_db, -40.0..=6.0, 0.0, "High", db);
            log_knob(ui, &mut p.low_freq, 50.0..=1000.0, d.low_freq, "FreqLo", hz);
            log_knob(ui, &mut p.high_freq, 800.0..=12000.0, d.high_freq, "FreqHi", hz);
        }
        Effect::Compressor(p) => {
            let d = CompressorParams::default();
            knob(ui, &mut p.threshold_db, -60.0..=0.0, d.threshold_db, "Threshold", |v| format!("{v:.1} dB"));
            log_knob(ui, &mut p.ratio, 1.0..=20.0, d.ratio, "Ratio", |v| format!("{v:.1}:1"));
            log_knob(ui, &mut p.attack_ms, 0.1..=200.0, d.attack_ms, "Attack", |v| format!("{v:.1} ms"));
            log_knob(ui, &mut p.release_ms, 5.0..=2000.0, d.release_ms, "Release", |v| format!("{v:.0} ms"));
            knob(ui, &mut p.makeup_db, 0.0..=24.0, d.makeup_db, "Makeup", |v| format!("{v:.1} dB"));
        }
        Effect::Delay(p) => {
            let d = DelayParams::default();
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Tiempo").color(TEXT_DIM).size(10.5));
                beats_combo(ui, &format!("{id:?}delay"), &mut p.time_beats, &[0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0]);
                ui.checkbox(&mut p.ping_pong, "Ping Pong");
            });
            knob(ui, &mut p.feedback, 0.0..=0.95, d.feedback, "Feedback", percent);
            log_knob(ui, &mut p.filter_hz, 500.0..=18000.0, d.filter_hz, "Filter", hz);
            knob(ui, &mut p.mix, 0.0..=1.0, d.mix, "Dry/Wet", percent);
        }
        Effect::Reverb(p) => {
            let d = ReverbParams::default();
            knob(ui, &mut p.size, 0.0..=1.0, d.size, "Size", percent);
            knob(ui, &mut p.damping, 0.0..=1.0, d.damping, "Damping", percent);
            knob(ui, &mut p.width, 0.0..=1.0, d.width, "Width", percent);
            knob(ui, &mut p.mix, 0.0..=1.0, d.mix, "Dry/Wet", percent);
        }
        Effect::Saturator(p) => {
            let d = SaturatorParams::default();
            knob(ui, &mut p.drive_db, 0.0..=36.0, d.drive_db, "Drive", |v| format!("{v:.1} dB"));
            knob(ui, &mut p.output_db, -36.0..=6.0, d.output_db, "Output", |v| format!("{v:.1} dB"));
            knob(ui, &mut p.mix, 0.0..=1.0, d.mix, "Dry/Wet", percent);
        }
        Effect::AutoFilter(p) => {
            let d = AutoFilterParams::default();
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Tipo").color(TEXT_DIM).size(10.5));
                for mode in FilterMode::ALL {
                    ui.radio_value(&mut p.mode, mode, mode.label());
                }
            });
            log_knob(ui, &mut p.cutoff, 30.0..=18000.0, d.cutoff, "Freq", hz);
            knob(ui, &mut p.resonance, 0.0..=0.95, d.resonance, "Res", percent);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("LFO").color(TEXT_DIM).size(10.5));
                beats_combo(ui, &format!("{id:?}lfo"), &mut p.lfo_beats, &[0.5, 1.0, 2.0, 4.0, 8.0, 16.0]);
            });
            knob(ui, &mut p.lfo_amount, 0.0..=4.0, 0.0, "Amount", |v| format!("{v:.1} oct"));
        }
    }
}

/// Returns (parameters changed, drum pad clicked).
fn instrument_device(ui: &mut egui::Ui, instrument: &mut InstrumentKind) -> (bool, Option<u8>) {
    let before = *instrument;
    let mut pad = None;
    match instrument {
        InstrumentKind::Synth(p) => {
            device_frame(ui, "Pulse Synth", 470.0, None, |_| {}, |ui| synth_controls(ui, p));
        }
        InstrumentKind::Drums => {
            device_frame(ui, "Drum Kit", 330.0, None, |_| {}, |ui| {
                egui::Grid::new("pads").spacing([4.0, 4.0]).show(ui, |ui| {
                    let pads = [
                        drum_note::KICK,
                        drum_note::SNARE,
                        drum_note::CLAP,
                        drum_note::CLOSED_HAT,
                        drum_note::OPEN_HAT,
                    ];
                    for (i, pitch) in pads.into_iter().enumerate() {
                        let label = format!("{}\n{}", drum_name(pitch).unwrap_or(""), crate::project::note_name(pitch));
                        let button = egui::Button::new(egui::RichText::new(label).size(10.5)).min_size(vec2(96.0, 54.0));
                        if ui.add(button).on_hover_text("Clic para escuchar").clicked() {
                            pad = Some(pitch);
                        }
                        if i % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
            });
        }
    }
    (*instrument != before, pad)
}

fn synth_controls(ui: &mut egui::Ui, p: &mut SynthParams) {
    let d = SynthParams::default();
    ui.vertical(|ui| {
        ui.label(egui::RichText::new("Oscilador").color(TEXT_DIM).size(10.5));
        for w in Waveform::ALL {
            ui.selectable_value(&mut p.waveform, w, w.label());
        }
    });
    ui.vertical(|ui| {
        envelope_preview(ui, p);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            log_knob(ui, &mut p.attack, 0.001..=4.0, d.attack, "Attack", seconds);
            log_knob(ui, &mut p.decay, 0.005..=4.0, d.decay, "Decay", seconds);
            knob(ui, &mut p.sustain, 0.0..=1.0, d.sustain, "Sustain", percent);
            log_knob(ui, &mut p.release, 0.005..=4.0, d.release, "Release", seconds);
        });
    });
    ui.vertical(|ui| {
        log_knob(ui, &mut p.cutoff, 40.0..=18000.0, d.cutoff, "Filter", hz);
        knob(ui, &mut p.resonance, 0.0..=0.95, d.resonance, "Res", percent);
    });
    knob(ui, &mut p.gain, 0.0..=1.0, d.gain, "Volume", percent);
}

/// Small ADSR curve, like the envelope displays on hardware synths.
fn envelope_preview(ui: &mut egui::Ui, p: &SynthParams) {
    let (rect, _) = ui.allocate_exact_size(vec2(192.0, 46.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, theme::BG);
    let total = p.attack + p.decay + 0.3 + p.release;
    let x = |t: f32| rect.left() + 4.0 + (rect.width() - 8.0) * (t / total);
    let y = |level: f32| rect.bottom() - 4.0 - (rect.height() - 8.0) * level;
    let points = vec![
        pos2(x(0.0), y(0.0)),
        pos2(x(p.attack), y(1.0)),
        pos2(x(p.attack + p.decay), y(p.sustain)),
        pos2(x(p.attack + p.decay + 0.3), y(p.sustain)),
        pos2(x(total), y(0.0)),
    ];
    painter.add(egui::Shape::line(points, Stroke::new(1.5, ACCENT)));
    painter.text(rect.right_top() + vec2(-4.0, 4.0), Align2::RIGHT_TOP, "ADSR", theme::small(), Color32::from_gray(110));
}
