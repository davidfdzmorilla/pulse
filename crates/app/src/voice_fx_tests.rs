//! The Vocoder and «Voz: tono y formantes» in a project: a vocoder sings the
//! notes of the track it takes as carrier, through the engine's sidechain;
//! both are saved and opened; their devices show every control. With
//! `PULSE_DEMOS=dir`, an ignored test leaves WAVs of a voice (macOS's `say`)
//! through them, and `PULSE_SHOTS=dir` PNGs of their devices.

use crate::app::PulseApp;
use crate::factory_recipes::{render_project, Stereo};
use crate::project::{Project, Track};
use eframe::egui::{self, vec2, Rect};
use engine::model::*;
use engine::Target;

const SR: f32 = 48_000.0;

/// A vowel sung at `hz` (a pulse a period through three formants), as a WAV
/// file of its own, `seconds` long.
fn vowel_file(hz: f32, seconds: f32) -> String {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("vowel-{hz}-{seconds}-{}.wav", std::process::id()));
    if !path.exists() {
        let n = (seconds * SR) as usize;
        let mut x = vec![0.0f32; n];
        let mut phase = 0.0;
        for s in x.iter_mut() {
            phase += hz / SR;
            if phase >= 1.0 {
                phase -= 1.0;
                *s = 1.0;
            }
        }
        for (f, bw) in [(730.0f32, 90.0f32), (1090.0, 110.0), (2440.0, 160.0)] {
            let r = (-std::f32::consts::PI * bw / SR).exp();
            let (a1, a2, g) = (2.0 * r * (std::f32::consts::TAU * f / SR).cos(), -r * r, 1.0 - r);
            let (mut y1, mut y2) = (0.0f32, 0.0f32);
            for s in x.iter_mut() {
                let y = g * *s + a1 * y1 + a2 * y2;
                y2 = y1;
                y1 = y;
                *s = y;
            }
        }
        let peak = x.iter().fold(0f32, |m, s| m.max(s.abs()));
        x.iter_mut().for_each(|s| *s *= 0.5 / peak);
        let part = dir.join(format!("vowel-{}-{:?}.part", std::process::id(), std::thread::current().id()));
        crate::export::write(&part, &x, &x, SR as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
        std::fs::rename(&part, &path).unwrap();
    }
    path.to_string_lossy().into_owned()
}

fn audio_clip(path: &str, beats: f64) -> AudioClip {
    AudioClip { hard_start: false, fades: Fades::new(0.0, 0.0), name: "Voz".into(), color: [0; 3], sample: path.into(), length: beats, offset: 0.0, warp: WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false }
}

/// A project at 120 BPM: a voice (track id 1) with `devices`, and a synth
/// (id 2) holding `notes` for `beats`, `audible` or muted.
fn voice_and_synth(voice: &str, beats: f64, devices: Vec<Device>, synth: InstrumentKind, notes: &[(f64, u8, f64)], audible: bool) -> Project {
    let mut p = Project::empty();
    p.tempo = 120.0;
    let mut v = Track::new_audio("Voz", [200, 80, 160]);
    v.id = 1;
    v.arrangement = vec![ArrangementClip { start: 0.0, length: beats, clip: Clip::Audio(audio_clip(voice, beats)) }];
    v.devices = devices;
    let mut s = Track::new("Synth", [80, 160, 200], synth);
    s.id = 2;
    let mut clip = MidiClip::new("Acordes", beats, [0; 3]);
    for &(start, pitch, length) in notes {
        clip.notes.push(Note { start, length, pitch, velocity: 0.8, chance: 1.0 });
    }
    s.arrangement = vec![ArrangementClip { start: 0.0, length: beats, clip: Clip::Midi(clip) }];
    s.mixer.active = audible;
    p.tracks = vec![v, s];
    p.next_id = 3;
    p
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// The pitch (Hz) at `at` seconds of the left channel.
fn pitch(sound: &Stereo, at: f32) -> Option<f64> {
    let from = (at * SR) as usize;
    stems::pitch::live(&sound.0[from..from + 4096], SR, 60.0, 1000.0).map(|(hz, _)| hz)
}

/// A muted synth track as carrier: the voice comes out at the synth's notes,
/// one after the other, and goes quiet when the synth stops.
#[test]
fn a_vocoder_sings_the_notes_of_its_carrier_track() {
    let voice = vowel_file(120.0, 4.0);
    let vocoder = Device::new(Effect::Vocoder(VocoderParams { sidechain: Some(2), sibilance: 0.0, ..Default::default() }));
    let synth = InstrumentKind::Synth(Default::default());
    // A3, then E4, then nothing for the last two beats.
    let p = voice_and_synth(&voice, 8.0, vec![vocoder], synth, &[(0.0, 57, 3.0), (3.0, 64, 3.0)], false);
    let out = render_project(&p, 8.0);
    let (a, e) = (pitch(&out, 0.8).expect("a pitch"), pitch(&out, 2.2).expect("a pitch"));
    let quiet = 20.0 * rms(&out.0[(3.5 * SR) as usize..(3.9 * SR) as usize]).max(1e-9).log10();
    let loud = 20.0 * rms(&out.0[(0.5 * SR) as usize..(2.5 * SR) as usize]).log10();
    println!("vocoder por sidechain: {a:.1} Hz y {e:.1} Hz; {loud:.1} dB con el synth, {quiet:.1} dB sin él");
    assert!((a / 220.0).log2().abs() * 1200.0 < 15.0, "{a}");
    assert!((e / 329.63).log2().abs() * 1200.0 < 15.0, "{e}");
    assert!(loud > -30.0 && quiet < loud - 30.0, "{loud} {quiet}");
    // The synth itself, muted, isn't heard: without the vocoder, silence.
    let dry = voice_and_synth(&voice, 8.0, Vec::new(), synth, &[(0.0, 57, 3.0)], false);
    let mut silent = dry.clone();
    silent.tracks[0].arrangement.clear();
    assert!(rms(&render_project(&silent, 4.0).0) < 1e-6);
}

/// A voice shift moves the voice's pitch in the project, and the mixer
/// makes up for its latency: a shift of nothing lines up with the voice
/// on another track sample for sample.
#[test]
fn a_voice_shift_moves_the_pitch_and_stays_in_time() {
    let voice = vowel_file(150.0, 3.0);
    let up = Device::new(Effect::VoiceShift(VoiceShiftParams { pitch: 7.0, ..Default::default() }));
    let p = voice_and_synth(&voice, 6.0, vec![up], InstrumentKind::Synth(Default::default()), &[], false);
    let out = render_project(&p, 6.0);
    let hz = pitch(&out, 1.0).expect("a pitch");
    println!("voz a 150 Hz, +7 st: {hz:.1} Hz");
    assert!((hz / (150.0 * 2f64.powf(7.0 / 12.0))).log2().abs() * 1200.0 < 15.0, "{hz}");
    // Compensated: set to nothing, next to a voice on another track, both
    // come out together, sample for sample as without it.
    let two = |devices: Vec<Device>| {
        let mut p = voice_and_synth(&voice, 6.0, devices, InstrumentKind::Synth(Default::default()), &[], false);
        let mut copy = Track::new_audio("Copia", [0; 3]);
        copy.id = 3;
        copy.arrangement = p.tracks[0].arrangement.clone();
        p.tracks.push(copy);
        p.next_id = 4;
        render_project(&p, 6.0)
    };
    let (with, without) = (two(vec![Device::new(Effect::VoiceShift(Default::default()))]), two(Vec::new()));
    let worst = with.0.iter().zip(&without.0).take((5.0 * SR) as usize).fold(0f32, |m, (a, b)| m.max((a - b).abs()));
    println!("voz movida de nada junto a una copia, contra las dos sin efecto: diferencia máxima {worst:e}");
    assert!(worst < 1e-5, "{worst}");
}

/// Saved and opened, both are the same, sidechain included.
#[test]
fn vocoder_and_voice_shift_are_saved_and_opened() {
    let voice = vowel_file(150.0, 1.0);
    let vocoder = Effect::Vocoder(VocoderParams { bands: 12.0, formant: 4.0, carrier: VocoderCarrier::Noise, chord: VocoderChord::Fifth, sidechain: Some(2), ..Default::default() });
    let shift = Effect::VoiceShift(VoiceShiftParams { pitch: 3.0, fine: -20.0, formant: -1.5, output_db: 2.0, mix: 0.7 });
    let p = voice_and_synth(&voice, 2.0, vec![Device::new(vocoder), Device::new(shift)], InstrumentKind::Synth(Default::default()), &[], true);
    let json = p.to_json().unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&json).unwrap()["format"], 17);
    let again = Project::from_json(&json).unwrap();
    assert_eq!(again.tracks[0].devices, p.tracks[0].devices);
    // Settings missing from a file take their defaults.
    let older: VocoderParams = serde_json::from_str(r#"{"bands":16.0}"#).unwrap();
    assert_eq!(older, VocoderParams { bands: 16.0, ..Default::default() });
    for effect in [vocoder, shift] {
        assert!(!effect.fits_in_synths(), "{}", effect.label());
    }
}

/// The device panel of a voice track with both, drawn off screen a few
/// frames; and the picture of the last.
fn draw(devices: Vec<Device>, size: egui::Vec2) -> (Vec<String>, crate::offscreen::Image) {
    let mut project = Project::empty();
    project.tracks.clear();
    let mut voice = Track::new_audio("Voz", [200, 80, 160]);
    voice.devices = devices;
    project.tracks.push(voice);
    project.tracks.push(Track::new("Synth", [80, 160, 200], InstrumentKind::Synth(Default::default())));
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

/// Every knob of both is drawn, with the vocoder's carrier and presets.
#[test]
fn their_devices_show_every_control() {
    let devices = vec![Device::new(Effect::Vocoder(Default::default())), Device::new(Effect::VoiceShift(Default::default()))];
    let (texts, _) = draw(devices, vec2(2000.0, 220.0));
    for effect in [Effect::Vocoder(Default::default()), Effect::VoiceShift(Default::default())] {
        for info in effect.params() {
            assert!(texts.iter().any(|t| t == info.name), "{}: {} not drawn", effect.label(), info.name);
        }
        assert!(texts.iter().any(|t| t == effect.label()), "{}", effect.label());
        assert!(!effect.factory_presets().is_empty());
    }
    for label in ["Portadora", "Propia", "Sierra", "Una nota", "Retraso", "Presets"] {
        assert!(texts.iter().any(|t| t == label), "{label} not drawn: {texts:?}");
    }
    // By sidechain, the carrier is the track's name.
    let keyed = vec![Device::new(Effect::Vocoder(VocoderParams { sidechain: Some(2), ..Default::default() }))];
    let (texts, _) = draw(keyed, vec2(1200.0, 220.0));
    assert!(texts.iter().any(|t| t == "Synth"), "{texts:?}");
}

/// `PULSE_SHOTS=dir cargo test --release -p app voice_fx_shots -- --ignored`.
#[test]
#[ignore]
fn voice_fx_shots() {
    let Some(dir) = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let preset = |effect: Effect, name: &str| effect.factory_presets().into_iter().find(|(n, _)| *n == name).unwrap().1;
    let shots = [
        ("vocoder.png", vec![Device::new(preset(Effect::Vocoder(Default::default()), "Coros · Coro de cuerdas"))], vec2(560.0, 190.0)),
        ("vocoder-sidechain.png", vec![Device::new(Effect::Vocoder(VocoderParams { sidechain: Some(2), ..Default::default() }))], vec2(560.0, 190.0)),
        ("tono-y-formantes.png", vec![Device::new(preset(Effect::VoiceShift(Default::default()), "Voz · Ardilla"))], vec2(480.0, 190.0)),
    ];
    for (file, devices, size) in shots {
        let (_, image) = draw(devices, size);
        image.save_png(&dir.join(file)).unwrap();
    }
}

/// WAVs of a voice (macOS's `say`) through both, to listen to:
/// `PULSE_DEMOS=dir cargo test --release -p app voice_fx_demos -- --ignored --nocapture`.
#[test]
#[ignore = "needs macOS's say"]
fn voice_fx_demos() {
    let Some(dir) = std::env::var_os("PULSE_DEMOS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("voz-original.wav");
    let text = "Bailando toda la noche, bajo las luces de la ciudad. Siente el ritmo, no pares de bailar.";
    let status = std::process::Command::new("say").args(["-v", "Mónica", "-r", "150", "-o"]).arg(&wav).args(["--data-format=LEI16@48000", text]).status().unwrap();
    assert!(status.success());
    let path = wav.to_string_lossy().into_owned();
    let seconds = hound::WavReader::open(&wav).unwrap().duration() as f64 / 48_000.0;
    let beats = (seconds * 2.0).ceil() + 2.0;
    let find = |effect: Effect, name: &str| effect.factory_presets().into_iter().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("{name}")).1;
    let vocoder = |name: &str| find(Effect::Vocoder(Default::default()), name);
    let shift = |name: &str| find(Effect::VoiceShift(Default::default()), name);
    // Chords for the synth carrier: Am, F, C, G, two beats each.
    let mut chords = Vec::new();
    for (bar, root) in [57u8, 53, 48, 55].iter().cycle().take((beats / 2.0) as usize).enumerate() {
        let minor = bar % 4 == 0;
        for interval in [0u8, if minor { 3 } else { 4 }, 7, 12] {
            chords.push((bar as f64 * 2.0, root - 12 + interval, 1.9));
        }
    }
    let pad = InstrumentKind::catalog().into_iter().find(|k| matches!(k, InstrumentKind::SuperSaw(_))).unwrap();
    let Effect::Vocoder(base) = vocoder("Sidechain · Synth de otra pista") else { unreachable!() };
    let keyed = Effect::Vocoder(VocoderParams { sidechain: Some(2), ..base });
    let demos: Vec<(&str, Vec<Effect>)> = vec![
        ("vocoder-sierra", vec![vocoder("Robot · Robot clásico")]),
        ("vocoder-daft", vec![vocoder("Robot · Daft")]),
        ("vocoder-coro-de-cuerdas", vec![vocoder("Coros · Coro de cuerdas")]),
        ("vocoder-susurro", vec![vocoder("Textura · Susurro")]),
        ("vocoder-8-bandas", vec![vocoder("Clásico · Vocoder de 8 bandas")]),
        ("vocoder-synth-por-sidechain", vec![keyed]),
        ("tono-mas-5", vec![Effect::VoiceShift(VoiceShiftParams { pitch: 5.0, ..Default::default() })]),
        ("tono-menos-5", vec![Effect::VoiceShift(VoiceShiftParams { pitch: -5.0, ..Default::default() })]),
        ("formantes-mas-4", vec![Effect::VoiceShift(VoiceShiftParams { formant: 4.0, ..Default::default() })]),
        ("formantes-menos-4", vec![Effect::VoiceShift(VoiceShiftParams { formant: -4.0, ..Default::default() })]),
        ("ardilla", vec![shift("Voz · Ardilla")]),
        ("demonio", vec![shift("Voz · Demonio")]),
        ("octava-abajo", vec![shift("Voz · Octava abajo")]),
        ("coro-de-octava", vec![shift("Coros · Coro de octava")]),
    ];
    for (name, effects) in demos {
        let devices = effects.into_iter().map(Device::new).collect();
        let p = voice_and_synth(&path, beats, devices, pad, &chords, false);
        let (l, r) = render_project(&p, beats);
        let peak = l.iter().chain(&r).fold(0f32, |m, s| m.max(s.abs()));
        println!("{name:28} rms {:6.1} dB  pico {:5.1} dB", 20.0 * rms(&l).max(1e-9).log10(), 20.0 * peak.max(1e-9).log10());
        crate::export::write(&dir.join(format!("{name}.wav")), &l, &r, 48_000, crate::export::BitDepth::Int16, 1.0).unwrap();
    }
}
