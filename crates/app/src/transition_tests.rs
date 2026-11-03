//! The «Transición» effect in a project: switched by a lane on the master,
//! it stops the tape, repeats and cuts in time with the song; it is saved and
//! opened (format 20); its device shows the controls of each mode. With
//! `PULSE_DEMOS=dir`, an ignored test leaves WAVs of a beat through its
//! presets, and `PULSE_SHOTS=dir` PNGs of its device.

use crate::app::PulseApp;
use crate::factory_recipes::{render_project, Stereo};
use crate::project::{Project, Track};
use eframe::egui::{self, vec2, Rect};
use engine::model::*;
use engine::Target;

const SR: f32 = 48_000.0;
/// Frames a beat at 120 BPM.
const BEAT: usize = 24_000;

/// Eight bars of a beat (kick, snare, hats) and a bass at 120 BPM, with
/// `devices` on the master and «Active» of the first switched on over `on`
/// (beats).
fn beat(devices: Vec<Device>, on: &[(f64, f64)]) -> Project {
    let mut p = Project::empty();
    p.tracks.clear();
    p.tempo = 120.0;
    let beats = 32.0;
    let mut drums = Track::new("Batería", [200, 80, 80], InstrumentKind::DrumMachine(Default::default()));
    let mut clip = MidiClip::new("Ritmo", beats, [0; 3]);
    for bar in 0..8 {
        let b = bar as f64 * 4.0;
        for (start, pitch, velocity) in [(0.0, 36u8, 1.0f32), (1.0, 38, 0.9), (2.0, 36, 1.0), (2.5, 36, 0.8), (3.0, 38, 0.9)] {
            clip.notes.push(Note { start: b + start, length: 0.25, pitch, velocity, chance: 1.0 });
        }
        for k in 0..8 {
            clip.notes.push(Note { start: b + k as f64 * 0.5, length: 0.1, pitch: 42, velocity: 0.6, chance: 1.0 });
        }
    }
    drums.arrangement = vec![ArrangementClip { start: 0.0, length: beats, clip: Clip::Midi(clip) }];
    let mut bass = Track::new("Bajo", [80, 160, 200], InstrumentKind::Synth(Default::default()));
    let mut clip = MidiClip::new("Bajo", beats, [0; 3]);
    for (k, pitch) in [33u8, 33, 36, 31].iter().cycle().take(16).enumerate() {
        clip.notes.push(Note { start: k as f64 * 2.0, length: 1.5, pitch: *pitch, velocity: 0.8, chance: 1.0 });
    }
    bass.arrangement = vec![ArrangementClip { start: 0.0, length: beats, clip: Clip::Midi(clip) }];
    p.tracks = vec![drums, bass];
    p.assign_ids();
    p.master.devices = devices;
    if !on.is_empty() {
        let mut lane = AutomationLane::new(Param::Device { device: 0, param: 0 });
        lane.points.push(AutomationPoint { beat: 0.0, value: 0.0, curve: 0.0 });
        for &(from, to) in on {
            for (beat, value) in [(from, 0.0), (from, 1.0), (to, 1.0), (to, 0.0)] {
                lane.points.push(AutomationPoint { beat, value, curve: 0.0 });
            }
        }
        p.master.automation.push(lane);
    }
    p
}

fn preset(name: &str) -> Effect {
    Effect::Transition(Default::default()).factory_presets().into_iter().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("{name}")).1
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Frames at `beat` of the song as exported (it starts on its beat: the
/// effect's latency is made up for).
fn at(beat: f64) -> usize {
    (beat * BEAT as f64) as usize
}

/// On the master, switched by its lane: the cut is silent exactly between
/// its beats and the song comes back whole on the drop; a tape stop ends in
/// silence; a repeat plays the first sixteenth after its beat again.
#[test]
fn a_lane_on_the_master_cuts_stops_and_repeats_in_time() {
    let plain: Stereo = render_project(&beat(Vec::new(), &[]), 20.0);
    let cut = render_project(&beat(vec![Device::new(preset("Corte · Antes del drop"))], &[(15.0, 16.0)]), 20.0);
    let fade = (0.001 * SR) as usize;
    assert_eq!(&cut.0[..at(15.0)], &plain.0[..at(15.0)], "the same until the cut");
    let gap = &cut.0[at(15.0) + fade..at(16.0) - fade];
    let loud: Vec<usize> = (0..gap.len()).filter(|&i| gap[i] != 0.0).collect();
    assert!(loud.is_empty(), "silence before the drop: {} {:?} {:?} max {}", loud.len(), loud.first(), loud.last(), gap.iter().fold(0f32, |m, s| m.max(s.abs())));
    assert_eq!(&cut.0[at(16.0)..], &plain.0[at(16.0)..cut.0.len()], "the drop whole, on its beat");

    let stop = render_project(&beat(vec![Device::new(preset("Parada de cinta · 1 compás"))], &[(8.0, 16.0)]), 20.0);
    assert!(rms(&stop.0[at(8.0)..at(9.0)]) > 0.02, "slowing down, it still sounds");
    assert!(rms(&stop.0[at(12.0) + 2000..at(16.0) - 100]) < 1e-6, "stopped after a bar");
    assert_eq!(&stop.0[at(16.0)..], &plain.0[at(16.0)..stop.0.len()]);

    let repeat = render_project(&beat(vec![Device::new(preset("Repetición · 1/16"))], &[(14.0, 16.0)]), 20.0);
    let step = BEAT / 4;
    let fade = (0.002 * SR) as usize;
    // (The eighth fades out into the drop.)
    for k in 1..7 {
        let from = at(14.0) + k * step + fade;
        assert_eq!(&repeat.0[from..at(14.0) + (k + 1) * step], &plain.0[from - k * step..at(14.0) + step], "repeat {k}");
    }
}

/// Saved and opened as it was, in format 20 (an older Pulse refuses it);
/// settings missing from a file take their defaults.
#[test]
fn it_is_saved_and_opened() {
    let effect = Effect::Transition(TransitionParams { mode: TransitionMode::Repeat, grid_beats: 1.0 / 3.0, pitch: -1.5, decay_db: 2.0, gate: 0.6, filter: Some(FilterMode::HighPass), filter_hz: 400.0, sweep: 0.2, resonance: 0.5, mix: 0.8, ..Default::default() });
    let p = beat(vec![Device::new(effect)], &[(4.0, 6.0)]);
    let json = p.to_json().unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&json).unwrap()["format"], 20);
    let again = Project::from_json(&json).unwrap();
    assert_eq!(again.master.devices, p.master.devices);
    assert_eq!(again.master.automation, p.master.automation);
    assert!(!beat(Vec::new(), &[]).to_json().unwrap().contains("\"format\": 20"), "without one, no new format");
    let older: TransitionParams = serde_json::from_str(r#"{"mode":"Cut"}"#).unwrap();
    assert_eq!(older, TransitionParams { mode: TransitionMode::Cut, ..Default::default() });
    assert!(!effect.fits_in_synths());
}

/// The device panel of a track with `devices`, drawn off screen a few
/// frames; and the picture of the last.
fn draw(devices: Vec<Device>, size: egui::Vec2) -> (Vec<String>, crate::offscreen::Image) {
    let mut project = Project::empty();
    project.tracks.clear();
    let mut voice = Track::new_audio("Voz", [200, 80, 160]);
    voice.devices = devices;
    project.tracks.push(voice);
    project.assign_ids();
    let mut app = PulseApp::headless(project);
    app.selected_target = Target::Track(0);
    let ctx = egui::Context::default();
    crate::theme::apply(&ctx);
    let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
    let mut canvas = crate::offscreen::Canvas::default();
    let mut last = None;
    for _ in 0..4 {
        ctx.set_pixels_per_point(2.0);
        let output = ctx.run_ui(input(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| crate::devices::show(&mut app, ui));
        });
        let texts = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::epaint::Shape::Text(t) => Some(t.galley.job.text.clone()),
                _ => None,
            })
            .collect();
        let image = canvas.draw(&ctx, output, size, 2.0);
        last = Some((texts, image));
    }
    last.unwrap()
}

/// Each mode shows its button, its mode, its time or grid, its filter and
/// the knobs it uses (and not the others').
#[test]
fn its_device_shows_the_controls_of_each_mode() {
    let names = |effect: &Effect, indices: &[usize]| indices.iter().map(|&i| effect.params()[i].name).collect::<Vec<_>>();
    let cases = [
        (TransitionMode::TapeStop, vec!["Duración", "1/4"], vec![1, 8], vec![2, 3, 5]),
        (TransitionMode::TapeStart, vec!["Duración"], vec![1, 8], vec![4]),
        (TransitionMode::Repeat, vec!["Rejilla", "1/16", "Filtro", "Sin filtro"], vec![2, 3, 4, 5, 6, 7, 8], vec![1]),
        (TransitionMode::Cut, vec!["Filtro", "Silencio"], vec![5, 7, 8], vec![1, 2, 3, 4, 6]),
    ];
    for (mode, labels, shown, hidden) in cases {
        let effect = Effect::Transition(TransitionParams { mode, ..Default::default() });
        let (texts, _) = draw(vec![Device::new(effect)], vec2(1400.0, 220.0));
        for label in ["Transición", "○ Activo", "Modo", mode.label(), "Presets"].into_iter().chain(labels) {
            assert!(texts.iter().any(|t| t == label), "{mode:?}: {label} not drawn: {texts:?}");
        }
        for name in names(&effect, &shown) {
            assert!(texts.iter().any(|t| t == name), "{mode:?}: {name} not drawn");
        }
        for name in names(&effect, &hidden) {
            assert!(!texts.iter().any(|t| t == name), "{mode:?}: {name} drawn");
        }
    }
    let on = Effect::Transition(TransitionParams { active: 1.0, ..Default::default() });
    let (texts, _) = draw(vec![Device::new(on)], vec2(1400.0, 220.0));
    assert!(texts.iter().any(|t| t == "● Activo"), "{texts:?}");
}

/// `PULSE_SHOTS=dir cargo test --release -p app transition_shots -- --ignored`.
#[test]
#[ignore]
fn transition_shots() {
    let Some(dir) = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    for (file, name) in [("transicion-cinta.png", "Parada de cinta · 1 compás"), ("transicion-repeticion.png", "Repetición · Eco que se apaga"), ("transicion-corte.png", "Corte · Sin graves antes del drop")] {
        let effect = preset(name);
        let width = crate::devices::effect_width(&effect) + 260.0;
        let (_, image) = draw(vec![Device::new(effect)], vec2(width, 200.0));
        image.save_png(&dir.join(file)).unwrap();
    }
}

/// WAVs of the beat through each preset, switched on before the drop (bar 5),
/// and the beat alone:
/// `PULSE_DEMOS=dir cargo test --release -p app transition_demos -- --ignored --nocapture`.
#[test]
#[ignore]
fn transition_demos() {
    let Some(dir) = std::env::var_os("PULSE_DEMOS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let beats = 24.0;
    let file = |name: &str| name.to_lowercase().replace(" · ", "-").replace(['/', ' '], "-");
    let mut demos: Vec<(String, Vec<Device>, Vec<(f64, f64)>)> = vec![("ritmo-sin-efecto".into(), Vec::new(), Vec::new())];
    for (name, effect) in Effect::Transition(Default::default()).factory_presets() {
        let Effect::Transition(p) = effect else { unreachable!() };
        // Before the drop at beat 16: a tape over its length (the start, after
        // a bar of silence from a cut), repeats over the last two beats, a cut
        // over the last one.
        let on = match p.mode {
            TransitionMode::TapeStop => vec![(16.0 - p.length_beats.max(1.0), 16.0)],
            TransitionMode::TapeStart => vec![(16.0, 24.0)],
            TransitionMode::Repeat => vec![(14.0, 16.0)],
            TransitionMode::Cut => vec![(15.0, 16.0)],
        };
        demos.push((file(name), vec![Device::new(effect)], on));
    }
    for (name, devices, on) in demos {
        let (l, r) = render_project(&beat(devices, &on), beats);
        let peak = l.iter().chain(&r).fold(0f32, |m, s| m.max(s.abs()));
        let step = l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
        println!("{name:44} rms {:6.1} dB  pico {:5.1} dB  salto máx {step:.3}", 20.0 * rms(&l).max(1e-9).log10(), 20.0 * peak.max(1e-9).log10());
        crate::export::write(&dir.join(format!("{name}.wav")), &l, &r, 48_000, crate::export::BitDepth::Int16, 1.0).unwrap();
    }
}
