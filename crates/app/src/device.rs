//! Instrument panel for the selected track.

use crate::app::PulseApp;
use crate::project::drum_name;
use eframe::egui;
use engine::model::{drum_note, InstrumentKind, SynthParams, Waveform};
use engine::Command;

pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    let t = app.selected_track.min(app.project.tracks.len() - 1);
    let track = &mut app.project.tracks[t];
    let before = track.instrument;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("Pista {}", t + 1)).strong());
        ui.add(egui::TextEdit::singleline(&mut track.name).desired_width(150.0));
    });
    ui.separator();

    let is_drums = matches!(track.instrument, InstrumentKind::Drums);
    ui.horizontal(|ui| {
        ui.label("Instrumento:");
        if ui.selectable_label(!is_drums, "Sintetizador").clicked() && is_drums {
            track.instrument = InstrumentKind::Synth(SynthParams::default());
        }
        if ui.selectable_label(is_drums, "Batería").clicked() && !is_drums {
            track.instrument = InstrumentKind::Drums;
        }
    });
    ui.add_space(6.0);

    match &mut track.instrument {
        InstrumentKind::Synth(params) => synth_controls(ui, params),
        InstrumentKind::Drums => {
            ui.label("Kit sintetizado (mapa General MIDI):");
            for pitch in [
                drum_note::KICK,
                drum_note::SNARE,
                drum_note::CLAP,
                drum_note::CLOSED_HAT,
                drum_note::OPEN_HAT,
            ] {
                ui.weak(format!(
                    "{} — {}",
                    crate::project::note_name(pitch),
                    drum_name(pitch).unwrap_or("")
                ));
            }
        }
    }

    if track.instrument != before {
        let kind = track.instrument;
        app.audio.send(Command::SetInstrument { track: t, kind });
    }
}

fn synth_controls(ui: &mut egui::Ui, p: &mut SynthParams) {
    egui::Grid::new("synth").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Onda");
        egui::ComboBox::from_id_salt("waveform")
            .selected_text(p.waveform.label())
            .show_ui(ui, |ui| {
                for w in Waveform::ALL {
                    ui.selectable_value(&mut p.waveform, w, w.label());
                }
            });
        ui.end_row();

        let row = |ui: &mut egui::Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, log: bool, suffix: &str| {
            ui.label(label);
            ui.add(egui::Slider::new(value, range).logarithmic(log).suffix(suffix).max_decimals(3));
            ui.end_row();
        };
        row(ui, "Ataque", &mut p.attack, 0.001..=2.0, true, " s");
        row(ui, "Decay", &mut p.decay, 0.005..=4.0, true, " s");
        row(ui, "Sustain", &mut p.sustain, 0.0..=1.0, false, "");
        row(ui, "Release", &mut p.release, 0.005..=4.0, true, " s");
        row(ui, "Filtro", &mut p.cutoff, 40.0..=18000.0, true, " Hz");
        row(ui, "Resonancia", &mut p.resonance, 0.0..=0.95, false, "");
        row(ui, "Ganancia", &mut p.gain, 0.0..=1.0, false, "");
    });
}
