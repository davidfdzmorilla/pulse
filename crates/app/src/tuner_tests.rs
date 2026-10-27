//! A Tuner on a voice track tunes to the project's scale, and to the new one
//! when it changes; saved and opened, it is the same.

use crate::app::PulseApp;
use crate::project::{Project, Track};
use engine::model::*;
use engine::Command;
use stems::key::Key;

const SR: f32 = 48_000.0;

/// A voice singing F# 40 cents flat (MIDI 66.4), 6 s.
fn voice() -> String {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("tuner-voice-{}.wav", std::process::id()));
    if !path.exists() {
        let hz = 440.0 * 2f32.powf((66.4 - 69.0) / 12.0);
        let l: Vec<f32> = (0..(6.0 * SR) as usize).map(|i| 0.3 * (std::f32::consts::TAU * hz * i as f32 / SR).sin()).collect();
        // Written aside and moved in whole: tests running at once never
        // read it half written.
        let part = dir.join(format!("tuner-voice-{}-{:?}.part", std::process::id(), std::thread::current().id()));
        crate::export::write(&part, &l, &l, SR as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
        std::fs::rename(&part, &path).unwrap();
    }
    path.to_string_lossy().into_owned()
}

fn project(scale: TunerScale) -> Project {
    let mut p = Project::empty();
    let mut t = Track::new_audio("Voz", [0; 3]);
    t.id = 1;
    let clip = AudioClip { hard_start: false, fades: Fades::new(0.0, 0.0), name: "voz".into(), color: [0; 3], sample: voice(), length: 12.0, offset: 0.0, warp: WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false };
    t.arrangement = vec![ArrangementClip { start: 0.0, length: 12.0, clip: Clip::Audio(clip) }];
    t.devices.push(Device::new(Effect::Tuner(TunerParams { speed_ms: 0.0, scale, ..Default::default() })));
    p.tracks = vec![t];
    p
}

/// The note the engine plays (MIDI, fractional), a second from the start.
fn played(app: &mut PulseApp, engine: &mut engine::Engine) -> f32 {
    for command in [Command::SetMetronome(false), Command::Stop, Command::BackToArrangement, Command::SetPosition(0.0), Command::Play] {
        app.audio.send(command);
    }
    let mut out = vec![0f32; 2 * SR as usize];
    for block in out.chunks_mut(512) {
        engine.process(block, 2);
        app.audio.collect_garbage();
    }
    app.audio.send(Command::Stop);
    let left: Vec<f32> = out.iter().step_by(2).skip((0.4 * SR) as usize).copied().collect();
    let mut crossings = Vec::new();
    for i in 1..left.len() {
        if left[i - 1] < 0.0 && left[i] >= 0.0 {
            crossings.push(i as f32 - 1.0 + left[i - 1] / (left[i - 1] - left[i]));
        }
    }
    let hz = (crossings.len() - 1) as f32 / (crossings[crossings.len() - 1] - crossings[0]) * SR;
    69.0 + 12.0 * (hz / 440.0).log2()
}

#[test]
fn a_voice_is_tuned_to_the_projects_scale_as_it_changes() {
    let mut app = PulseApp::headless(project(TunerScale::Project));
    let (audio, mut engine) = crate::app::Audio::detached();
    app.audio = audio;
    app.sync_all();
    // No scale: the nearest note, F#.
    let none = played(&mut app, &mut engine);
    // C major has no F#: G. D major has: F#.
    app.set_key(Some(Key { tonic: 0, minor: false }));
    let c = played(&mut app, &mut engine);
    app.set_key(Some(Key { tonic: 2, minor: false }));
    let d = played(&mut app, &mut engine);
    println!("sung 66.40: no scale {none:.2}, C major {c:.2}, D major {d:.2}");
    for (got, want) in [(none, 66.0), (c, 67.0), (d, 66.0)] {
        assert!((got - want).abs() < 0.05, "{got:.2} instead of {want}");
    }
    // Its own scale, whatever the project's.
    app.project.tracks[0].devices[0].effect = Effect::Tuner(TunerParams { speed_ms: 0.0, scale: TunerScale::Key { tonic: 0, minor: false }, ..Default::default() });
    app.sync_all();
    assert!((played(&mut app, &mut engine) - 67.0).abs() < 0.05);
}

#[test]
fn a_tuner_is_saved_and_needs_a_pulse_that_knows_it() {
    let p = project(TunerScale::Key { tonic: 9, minor: true });
    let json = p.to_json().unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&json).unwrap()["format"], 10);
    let again = Project::from_json(&json).unwrap();
    assert_eq!(again.tracks[0].devices, p.tracks[0].devices);
    // An older project's Tuner (without a scale) follows the project's.
    let older = r#"{"speed_ms":5.0,"humanize":0.5,"amount":1.0}"#;
    assert_eq!(serde_json::from_str::<TunerParams>(older).unwrap().scale, TunerScale::Project);
    // Not among a synth's own effects.
    assert!(!Effect::Tuner(Default::default()).fits_in_synths());
}
