//! MIDI effects in the app: a MIDI track's chain of them (before its
//! instrument, in the devices view), added from the browser, and sent to the
//! engine with the project's key filled in where they follow it.

use crate::app::PulseApp;
use crate::theme::TEXT_DIM;
use eframe::egui::{self, RichText};
use engine::model::{ArpMode, ChordParams, MidiDevice, MidiEffect, ScaleKey, MAX_MIDI_EFFECTS};
use engine::Command;

const NAMES: [&str; 12] = ["Do", "Do#", "Re", "Mib", "Mi", "Fa", "Fa#", "Sol", "Lab", "La", "Sib", "Si"];

fn key_name(key: ScaleKey) -> String {
    format!("{} {}", NAMES[key.tonic as usize % 12], if key.minor { "menor" } else { "mayor" })
}

fn mode_label(mode: ArpMode) -> &'static str {
    match mode {
        ArpMode::Up => "Arriba",
        ArpMode::Down => "Abajo",
        ArpMode::UpDown => "Arriba y abajo",
        ArpMode::Random => "Al azar",
        ArpMode::Played => "Como se tocan",
    }
}

const RATES: [(f64, &str); 6] = [(1.0, "1/4"), (0.5, "1/8"), (1.0 / 3.0, "1/8T"), (0.25, "1/16"), (1.0 / 6.0, "1/16T"), (0.125, "1/32")];

/// Ready-made fixed chords: name and semitones above.
const SHAPES: [(&str, [i8; 3]); 6] = [
    ("Mayor", [4, 7, 0]),
    ("Menor", [3, 7, 0]),
    ("Quinta", [7, 12, 0]),
    ("Octava", [12, 0, 0]),
    ("Sus4", [5, 7, 0]),
    ("Séptima", [4, 7, 10]),
];

impl PulseApp {
    /// The project's key, as the MIDI effects take it.
    fn scale_key(&self) -> Option<ScaleKey> {
        self.project.key.map(|k| ScaleKey { tonic: k.tonic, minor: k.minor })
    }

    /// Sends a track's MIDI effects to the engine.
    pub fn sync_midi_effects(&mut self, t: usize) {
        let Some(track) = self.project.tracks.get(t) else { return };
        let key = self.scale_key();
        let mut devices = [None; MAX_MIDI_EFFECTS];
        for (slot, device) in devices.iter_mut().zip(&track.midi_effects) {
            *slot = Some(MidiDevice { effect: device.effect.with_project_key(key), ..*device });
        }
        self.audio.send(Command::SetMidiEffects { track: t, devices });
    }

    /// After the project's key changes: the effects that follow it.
    pub fn sync_all_midi_effects(&mut self) {
        for t in 0..self.project.tracks.len() {
            if !self.project.tracks[t].midi_effects.is_empty() {
                self.sync_midi_effects(t);
            }
        }
    }

    /// Adds a MIDI effect at the end of a MIDI track's chain.
    pub fn add_midi_effect(&mut self, t: usize, effect: MidiEffect) {
        let Some(track) = self.project.tracks.get_mut(t) else { return };
        if track.is_audio() || track.is_group() {
            self.status = Some("Los efectos MIDI van en pistas MIDI".into());
            return;
        }
        if track.midi_effects.len() >= MAX_MIDI_EFFECTS {
            self.status = Some(format!("Como mucho {MAX_MIDI_EFFECTS} efectos MIDI por pista"));
            return;
        }
        track.midi_effects.push(MidiDevice::new(effect));
        self.sync_midi_effects(t);
        self.status = Some(format!("{} en «{}»", effect.label(), self.project.tracks[t].name));
    }
}

/// What a MIDI effect's view asked for.
enum Change {
    Set(usize, MidiDevice),
    Remove(usize),
    Move(usize, usize),
}

/// A MIDI track's MIDI effects, before its instrument.
pub fn show(app: &mut PulseApp, ui: &mut egui::Ui, t: usize) {
    let project_key = app.scale_key();
    let count = app.project.tracks[t].midi_effects.len();
    let mut changes = Vec::new();
    for index in 0..count {
        let before = app.project.tracks[t].midi_effects[index];
        let mut device = before;
        let title = device.effect.label();
        let width = match device.effect {
            MidiEffect::Arpeggiator(_) => 250.0,
            MidiEffect::Chord(_) => 230.0,
            MidiEffect::Scale(_) => 170.0,
        };
        let MidiDevice { effect, enabled } = &mut device;
        crate::devices::device_frame(
            ui,
            title,
            width,
            Some(enabled),
            |ui| {
                if ui.small_button("×").on_hover_text("Eliminar").clicked() {
                    changes.push(Change::Remove(index));
                }
                if index + 1 < count && ui.small_button("▶").on_hover_text("Mover a la derecha").clicked() {
                    changes.push(Change::Move(index, index + 1));
                }
                if index > 0 && ui.small_button("◀").on_hover_text("Mover a la izquierda").clicked() {
                    changes.push(Change::Move(index, index - 1));
                }
            },
            |ui| body(ui, effect, index, project_key),
        );
        if device != before {
            changes.push(Change::Set(index, device));
        }
    }
    if changes.is_empty() {
        return;
    }
    let chain = &mut app.project.tracks[t].midi_effects;
    for change in changes {
        match change {
            Change::Set(i, device) => chain[i] = device,
            Change::Remove(i) => {
                chain.remove(i);
            }
            Change::Move(from, to) => chain.swap(from, to),
        }
    }
    app.sync_midi_effects(t);
}

fn small(text: &str) -> RichText {
    RichText::new(text).color(TEXT_DIM).size(10.5)
}

/// The key an effect uses: the project's, or one of its own.
fn key_picker(ui: &mut egui::Ui, id: (&str, usize), follow: &mut bool, key: &mut Option<ScaleKey>, project_key: Option<ScaleKey>) {
    ui.checkbox(follow, "La del proyecto").on_hover_text("Sigue la Escala de la barra de transporte (y cambia con ella)");
    if *follow {
        ui.label(small(&project_key.map_or("(el proyecto no tiene escala)".to_string(), key_name)));
        return;
    }
    let shown = key.map_or("Ninguna".to_string(), key_name);
    egui::ComboBox::from_id_salt(id).selected_text(shown).width(110.0).show_ui(ui, |ui| {
        ui.selectable_value(key, None, "Ninguna");
        for minor in [false, true] {
            for tonic in 0..12 {
                let k = ScaleKey { tonic, minor };
                ui.selectable_value(key, Some(k), key_name(k));
            }
        }
    });
}

fn body(ui: &mut egui::Ui, effect: &mut MidiEffect, index: usize, project_key: Option<ScaleKey>) {
    match effect {
        MidiEffect::Arpeggiator(p) => {
            ui.vertical(|ui| {
                ui.label(small("Orden"));
                egui::ComboBox::from_id_salt(("arp mode", index)).selected_text(mode_label(p.mode)).width(110.0).show_ui(ui, |ui| {
                    for mode in ArpMode::ALL {
                        ui.selectable_value(&mut p.mode, mode, mode_label(mode));
                    }
                });
                ui.label(small("Ritmo"));
                let rate = RATES.iter().find(|r| (r.0 - p.rate).abs() < 1e-9).map_or("?", |r| r.1);
                egui::ComboBox::from_id_salt(("arp rate", index)).selected_text(rate).width(110.0).show_ui(ui, |ui| {
                    for (value, label) in RATES {
                        ui.selectable_value(&mut p.rate, value, label);
                    }
                });
            });
            ui.vertical(|ui| {
                ui.label(small("Octavas"));
                ui.add(egui::Slider::new(&mut p.octaves, 1..=4));
                ui.label(small("Duración"));
                let mut percent = p.gate * 100.0;
                if ui.add(egui::Slider::new(&mut percent, 5.0..=100.0).suffix(" %").fixed_decimals(0)).on_hover_text("Lo que dura cada nota, del paso").changed() {
                    p.gate = percent / 100.0;
                }
            });
        }
        MidiEffect::Chord(p) => {
            ui.vertical(|ui| {
                ui.checkbox(&mut p.in_key, "En la escala").on_hover_text("Cada nota con su tercera y su quinta de la escala: los acordes encajan con la canción");
                if p.in_key {
                    ui.checkbox(&mut p.seventh, "Con séptima");
                    key_picker(ui, ("chord key", index), &mut p.follow_project, &mut p.key, project_key);
                } else {
                    ui.label(small("Semitonos añadidos"));
                    ui.horizontal(|ui| {
                        for iv in &mut p.intervals {
                            ui.add(egui::DragValue::new(iv).range(-24..=24));
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for (name, shape) in SHAPES {
                            if ui.selectable_label(p.intervals == shape, name).clicked() {
                                p.intervals = shape;
                            }
                        }
                    });
                }
                velocity(ui, p);
            });
        }
        MidiEffect::Scale(p) => {
            ui.vertical(|ui| {
                ui.label(small("Las notas fuera de la escala van a la más cercana"));
                key_picker(ui, ("scale key", index), &mut p.follow_project, &mut p.key, project_key);
            });
        }
    }
}

fn velocity(ui: &mut egui::Ui, p: &mut ChordParams) {
    let mut percent = p.velocity * 100.0;
    ui.horizontal(|ui| {
        ui.label(small("Velocidad añadidas"));
        if ui.add(egui::DragValue::new(&mut percent).range(10.0..=100.0).suffix(" %").speed(1.0)).changed() {
            p.velocity = percent / 100.0;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use engine::model::{ArrangementClip, Clip, MidiClip, Note};

    /// One C held for a bar on a synth, through `effects`, rendered.
    fn render(effects: Vec<MidiDevice>, key: Option<stems::key::Key>) -> (Vec<f32>, Vec<f32>) {
        let mut project = Project::empty();
        project.key = key;
        project.tracks.truncate(1);
        let mut clip = MidiClip::new("C", 4.0, [0; 3]);
        clip.notes.push(Note { start: 0.0, length: 4.0, pitch: 60, velocity: 0.8, chance: 1.0 });
        project.tracks[0].arrangement.push(ArrangementClip { start: 0.0, length: 4.0, clip: Clip::Midi(clip) });
        project.tracks[0].midi_effects = effects;
        let mut samples = crate::sync::SampleCache::default();
        let commands = crate::sync::project_commands(&project, &mut samples, 48_000.0, &Default::default());
        crate::export::render(commands, (0.0, 4.0), crate::export::Song::of(&project), 48_000.0, &Default::default(), &|_| {}).unwrap()
    }

    /// Power at `freq` Hz over `from..to` seconds (Goertzel), both channels mixed.
    fn power(audio: &(Vec<f32>, Vec<f32>), freq: f64, from: f64, to: f64) -> f64 {
        let rate = 48_000.0;
        let (a, b) = ((from * rate) as usize, ((to * rate) as usize).min(audio.0.len()));
        let k = 2.0 * (2.0 * std::f64::consts::PI * freq / rate).cos();
        let (mut s1, mut s2) = (0.0, 0.0);
        for i in a..b {
            let x = (audio.0[i] + audio.1[i]) as f64 * 0.5;
            let s0 = x + k * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - k * s1 * s2) / ((b - a) as f64).powi(2)
    }

    const C4: f64 = 261.63;
    const EB4: f64 = 311.13;
    const E4: f64 = 329.63;
    const G4: f64 = 392.0;

    #[test]
    fn the_devices_view_shows_the_midi_effects_before_the_instrument() {
        let mut app = crate::app::PulseApp::headless(Project::demo());
        app.selected_target = engine::Target::Track(1);
        for effect in MidiEffect::catalog() {
            app.add_midi_effect(1, effect);
        }
        assert_eq!(app.project.tracks[1].midi_effects.len(), 3);
        // An audio track takes none.
        let audio = app.add_audio_track().unwrap();
        app.add_midi_effect(audio, MidiEffect::catalog()[0]);
        assert!(app.project.tracks[audio].midi_effects.is_empty());
        app.selected_target = engine::Target::Track(1);
        let ctx = egui::Context::default();
        let input = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(2400.0, 400.0))), ..Default::default() };
        let _ = ctx.run_ui(input(), |ui| crate::devices::show(&mut app, ui));
        let output = ctx.run_ui(input(), |ui| crate::devices::show(&mut app, ui));
        let mut texts = Vec::new();
        fn collect(shape: &egui::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => texts.push(t.galley.text().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
                _ => {}
            }
        }
        output.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
        for expected in ["Arpegiador", "Acorde", "Escala", "Orden", "En la escala"] {
            assert!(texts.iter().any(|t| t == expected), "{expected}: {texts:?}");
        }
    }

    #[test]
    fn a_chord_effect_in_the_project_key_adds_its_third_and_fifth() {
        let c_major = stems::key::Key { tonic: 0, minor: false };
        let alone = render(Vec::new(), Some(c_major));
        let chord = MidiDevice::new(MidiEffect::Chord(ChordParams::default()));
        let major = render(vec![chord], Some(c_major));
        let e = |audio| power(audio, E4, 0.2, 1.8);
        let g = |audio| power(audio, G4, 0.2, 1.8);
        assert!(e(&major) > 100.0 * e(&alone) && g(&major) > 100.0 * g(&alone), "E {} vs {}, G {} vs {}", e(&major), e(&alone), g(&major), g(&alone));
        // In C minor's scale the third is Eb.
        let minor = render(vec![chord], Some(stems::key::Key { tonic: 0, minor: true }));
        assert!(power(&minor, EB4, 0.2, 1.8) > 100.0 * power(&minor, E4, 0.2, 1.8));
    }

    #[test]
    fn an_arpeggiator_plays_a_held_chord_one_note_at_a_time() {
        let c_major = Some(stems::key::Key { tonic: 0, minor: false });
        let chord = MidiDevice::new(MidiEffect::Chord(ChordParams::default()));
        let arp = MidiDevice::new(MidiEffect::Arpeggiator(Default::default()));
        let audio = render(vec![chord, arp], c_major);
        // Sixteenths at 120 BPM (0.125 s): C, then E, then G. Each comes in on
        // its step and not before (the one before still rings in its release).
        let step = |k: f64| (0.125 * k + 0.03, 0.125 * k + 0.1);
        let in_step = |freq: f64, k: f64| {
            let (a, b) = step(k);
            power(&audio, freq, a, b)
        };
        assert!(in_step(C4, 0.0) > 20.0 * in_step(E4, 0.0) && in_step(C4, 0.0) > 20.0 * in_step(G4, 0.0), "C first, alone");
        assert!(in_step(E4, 1.0) > 20.0 * in_step(E4, 0.0), "then E");
        assert!(in_step(G4, 2.0) > 20.0 * in_step(G4, 1.0), "then G");
    }
}
