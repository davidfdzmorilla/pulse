//! Saving and opening projects: a rich project sounds the same, bit for bit,
//! after a save and a reopen; damaged or odd files never crash Pulse; a save
//! cut short never leaves half a project. Everything in temporary folders.

use crate::app::PulseApp;
use crate::project::{Project, SongMarker, Track, PALETTE};
use engine::model::*;
use engine::{Command, OfflineRenderer};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const RATE: f32 = 48_000.0;

fn base() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pulse-audit-persist-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fresh(name: &str) -> PathBuf {
    let dir = base().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A stereo WAV: a decaying tone with a little deterministic noise.
fn write_wav(path: &Path, seconds: f32, freq: f32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec { channels: 2, sample_rate: 44_100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let n = (seconds * 44_100.0) as usize;
    let mut seed: u32 = 0xC0FFEE;
    for i in 0..n {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let t = i as f32 / 44_100.0;
        let v = (t * freq * std::f32::consts::TAU).sin() * (-t * 2.0).exp() * 0.5 + (seed as f32 / u32::MAX as f32 - 0.5) * 0.05;
        let s = (v * 32_000.0) as i16;
        w.write_sample(s).unwrap();
        w.write_sample(s / 2).unwrap();
    }
    w.finalize().unwrap();
}

fn note(start: f64, length: f64, pitch: u8, velocity: f32) -> Note {
    Note { start, length, pitch, velocity }
}

fn midi(name: &str, length: f64, notes: Vec<Note>) -> MidiClip {
    let mut c = MidiClip::new(name, length, PALETTE[2]);
    c.notes = notes;
    c
}

fn lane(param: Param, pts: &[(f64, f32, f32)]) -> AutomationLane {
    AutomationLane { param, points: pts.iter().map(|&(beat, value, curve)| AutomationPoint { beat, value, curve }).collect() }
}

fn audio_clip(path: &Path, length: f64, warp: WarpMode) -> AudioClip {
    AudioClip {
        name: "audio".into(),
        color: PALETTE[4],
        sample: path.to_string_lossy().into_owned(),
        length,
        offset: 0.05,
        warp,
        original_bpm: 100.0,
        warp_markers: Vec::new(),
        takes: Vec::new(),
        key: None,
        gain_db: -3.0,
        transpose: 2.0,
        envelopes: Vec::new(),
    }
}

/// A project with every kind of instrument, every effect, MIDI effects, automation
/// (track, return, master, tempo), clip envelopes, grooves, warp, sends, a group,
/// a sidechain, scenes, markers and key. Samples live in `media`.
pub(crate) fn rich_project(media: &Path) -> Project {
    let wav_a = media.join("tono á.wav");
    let wav_b = media.join("sub carpeta/golpe 2.wav");
    write_wav(&wav_a, 2.0, 220.0);
    write_wav(&wav_b, 0.5, 90.0);
    let mut p = Project::empty();
    p.tempo = 123.0;
    p.swing = 0.2;
    p.swing_grid = 0.25;
    p.key = Some(stems::key::Key { tonic: 9, minor: true });
    p.markers = vec![SongMarker { beat: 0.0, name: "Intro".into() }, SongMarker { beat: 8.0, name: "Drop".into() }];
    p.loop_enabled = true;
    p.loop_start = 0.0;
    p.loop_end = 16.0;
    p.scene_names[0] = "Escena ñ".into();

    let phrase = |root: u8| -> Vec<Note> { (0..8).map(|i| note(i as f64 * 0.5, 0.4, root + [0, 3, 7, 10][i % 4], 0.6 + 0.05 * (i % 3) as f32)).collect() };
    let kinds: Vec<(&str, InstrumentKind)> = vec![
        ("Synth", InstrumentKind::Synth(SynthParams::default())),
        ("Drums", InstrumentKind::DrumMachine(Default::default())),
        ("Saw", InstrumentKind::SuperSaw(Default::default())),
        ("Acid", InstrumentKind::Acid(Default::default())),
        ("FM", InstrumentKind::Fm(Default::default())),
        ("Kick", InstrumentKind::Kick(Default::default())),
        ("Wave", InstrumentKind::Wavetable(Default::default())),
        ("Poly", InstrumentKind::Polychrome(Default::default())),
        ("Modular", InstrumentKind::Modular(Default::default())),
        ("Sampler", InstrumentKind::Sampler(Default::default())),
        ("Rack", InstrumentKind::DrumRack(Default::default())),
    ];
    let effects = [
        Effect::Eq3(Default::default()),
        Effect::Compressor(Default::default()),
        Effect::Delay(Default::default()),
        Effect::Reverb(Default::default()),
        Effect::Saturator(Default::default()),
        Effect::AutoFilter(Default::default()),
        Effect::Chorus(Default::default()),
        Effect::Phaser(Default::default()),
        Effect::Bitcrusher(Default::default()),
        Effect::Ducker(Default::default()),
        Effect::Limiter(Default::default()),
        Effect::Utility(Default::default()),
    ];
    let mut tracks = Vec::new();
    for (i, (name, kind)) in kinds.into_iter().enumerate() {
        let mut t = Track::new(name, PALETTE[i % 12], kind);
        let drums = matches!(kind, InstrumentKind::DrumMachine(_) | InstrumentKind::DrumRack(_) | InstrumentKind::Kick(_));
        let root = if drums { 36 } else { 48 + (i as u8 % 3) * 5 };
        let mut c = midi(name, 4.0, phrase(root));
        if i % 2 == 0 {
            c.envelopes.push(lane(Param::Volume, &[(0.0, 0.3, 0.0), (2.0, 0.9, 0.5), (4.0, 0.3, 0.0)]));
        }
        if i % 3 == 0 {
            c.groove = Some(Groove { grid: 0.25, swing: 0.5, accent: 0.3, humanize: 0.4 });
        }
        t.clips[0] = Some(Clip::Midi(c.clone()));
        t.clips[1] = Some(Clip::Midi(midi("B", 2.0, phrase(root + 2))));
        t.arrangement = vec![ArrangementClip { start: 0.0, length: 8.0, clip: Clip::Midi(c) }, ArrangementClip { start: 8.5, length: 4.0, clip: t.clips[1].clone().unwrap() }];
        t.devices.push(Device::new(effects[i % effects.len()]));
        t.devices.push(Device { effect: effects[(i + 5) % effects.len()], enabled: i % 2 == 0 });
        t.sends = [0.1 * (i % 4) as f32, 0.05 * (i % 3) as f32];
        t.mixer.pan = (i as f32 - 5.0) / 10.0;
        t.automation.push(lane(Param::Pan, &[(0.0, -0.5, 0.0), (12.0, 0.5, -0.4)]));
        t.automation.push(lane(Param::Device { device: 0, param: 0 }, &[(0.0, 0.2, 0.0), (6.0, 0.8, 0.0)]));
        t.automation.push(lane(Param::Instrument(1), &[(2.0, 0.3, 0.0), (10.0, 0.6, 0.0)]));
        t.automation.push(lane(Param::Send(0), &[(0.0, 0.0, 0.0), (8.0, 0.4, 0.0)]));
        match kind {
            InstrumentKind::Sampler(_) => t.set_sample(0, Some(wav_a.to_string_lossy().into_owned())),
            InstrumentKind::DrumRack(_) => {
                t.set_sample(0, Some(wav_b.to_string_lossy().into_owned()));
                t.set_sample(2, Some(wav_a.to_string_lossy().into_owned()));
            }
            InstrumentKind::Wavetable(_) => t.set_sample(0, Some(wav_a.to_string_lossy().into_owned())),
            _ => {}
        }
        if i == 0 {
            t.midi_effects = vec![
                MidiDevice::new(MidiEffect::Chord(Default::default())),
                MidiDevice::new(MidiEffect::Arpeggiator(Default::default())),
                MidiDevice::new(MidiEffect::Scale(Default::default())),
            ];
        }
        tracks.push(t);
    }
    // Audio track: three warp modes, markers, envelopes, takes, the same file twice.
    let mut audio = Track::new_audio("Audio ñ", PALETTE[7]);
    let mut stretch = audio_clip(&wav_a, 4.0, WarpMode::Stretch);
    stretch.envelopes.push(lane(Param::Volume, &[(0.0, 1.0, 0.0), (4.0, 0.2, 0.0)]));
    stretch.key = Some(ClipKey { tonic: 0, minor: false, follow: true });
    let mut marked = audio_clip(&wav_a, 4.0, WarpMode::Stretch);
    marked.warp_markers = vec![WarpMarker { beat: 0.0, seconds: 0.0 }, WarpMarker { beat: 2.0, seconds: 0.7 }, WarpMarker { beat: 4.0, seconds: 1.9 }];
    marked.takes = vec![wav_b.to_string_lossy().into_owned()];
    audio.clips[0] = Some(Clip::Audio(audio_clip(&wav_a, 4.0, WarpMode::Off)));
    audio.clips[1] = Some(Clip::Audio(audio_clip(&wav_b, 2.0, WarpMode::Repitch)));
    audio.arrangement = vec![
        ArrangementClip { start: 0.0, length: 4.0, clip: Clip::Audio(audio_clip(&wav_a, 4.0, WarpMode::Repitch)) },
        ArrangementClip { start: 4.0, length: 4.0, clip: Clip::Audio(stretch) },
        ArrangementClip { start: 8.0, length: 4.0, clip: Clip::Audio(marked) },
        ArrangementClip { start: 12.0, length: 2.0, clip: Clip::Audio(audio_clip(&wav_b, 2.0, WarpMode::Off)) },
    ];
    audio.sends = [0.3, 0.2];
    tracks.push(audio);
    // A group with two tracks inside, one with a compressor side-chained to the drums.
    let mut group = Track::new_group("Grupo", PALETTE[9]);
    group.devices.push(Device::new(Effect::Saturator(Default::default())));
    group.automation.push(lane(Param::Volume, &[(0.0, 0.5, 0.0), (14.0, 0.9, 0.0)]));
    tracks.push(group);
    for k in 0..2 {
        let mut t = Track::new(format!("En grupo {k}"), PALETTE[10], InstrumentKind::Fm(Default::default()));
        t.depth = 1;
        let c = midi("G", 4.0, phrase(60 + k * 4));
        t.arrangement = vec![ArrangementClip { start: 2.0, length: 12.0, clip: Clip::Midi(c.clone()) }];
        t.clips[2] = Some(Clip::Midi(c));
        tracks.push(t);
    }
    p.tracks = tracks;
    p.assign_ids();
    let drums_id = p.tracks[1].id;
    let last = p.tracks.len() - 1;
    p.tracks[last].devices.push(Device::new(Effect::Compressor(CompressorParams { sidechain: Some(drums_id), threshold_db: -30.0, ratio: 8.0, ..Default::default() })));
    // Returns and master with automation, tempo automation on the master.
    p.returns[0].automation.push(lane(Param::Device { device: 0, param: 1 }, &[(0.0, 0.3, 0.0), (10.0, 0.9, 0.0)]));
    p.returns[1].devices.push(Device::new(Effect::Chorus(Default::default())));
    p.master.devices.insert(0, Device::new(Effect::Eq3(Default::default())));
    p.master.devices.push(Device::new(Effect::Limiter(Default::default())));
    p.master.automation.push(lane(Param::Tempo, &[(0.0, 123.0, 0.0), (8.0, 131.0, 0.3), (16.0, 118.0, 0.0)]));
    p.master.automation.push(lane(Param::Volume, &[(0.0, 0.9, 0.0), (16.0, 1.0, 0.0)]));
    p
}

/// The song as the export renders it (48 kHz float, before writing).
pub(crate) fn export_render(project: &Project) -> (Vec<f32>, Vec<f32>) {
    let settings = crate::export::Settings::default();
    let range = crate::export::beat_range(project, settings.range).expect("arrangement");
    let mut samples = crate::sync::SampleCache::default();
    let mut renders = crate::export::plan(project, &mut samples, &settings, Path::new("/dev/null"));
    let r = renders.remove(0);
    crate::export::render(r.commands, range, crate::export::Song::of(project), RATE, &AtomicBool::new(false), &|_| {}).unwrap()
}

/// Scene `scene` launched in Session, `frames` long.
fn scene_render(project: &Project, scene: usize, frames: usize) -> Vec<f32> {
    let mut samples = crate::sync::SampleCache::default();
    let mut r = OfflineRenderer::new(RATE);
    for c in crate::sync::project_commands(project, &mut samples, RATE, &Default::default()) {
        r.send(c);
    }
    r.send(Command::LaunchScene(scene));
    let (mut l, mut rr) = (Vec::new(), Vec::new());
    r.render(frames, &mut l, &mut rr);
    l.into_iter().zip(rr).flat_map(|(a, b)| [a, b]).collect()
}

fn interleave((l, r): (Vec<f32>, Vec<f32>)) -> Vec<f32> {
    l.into_iter().zip(r).flat_map(|(a, b)| [a, b]).collect()
}

/// (frames that differ, max abs difference, first differing index).
fn compare(a: &[f32], b: &[f32]) -> (usize, f32, Option<usize>) {
    let mut n = a.len().abs_diff(b.len());
    let mut max = 0f32;
    let mut first = None;
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        if x.to_bits() != y.to_bits() {
            n += 1;
            max = max.max((x - y).abs());
            first.get_or_insert(i);
        }
    }
    (n, max, first)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

// ===================================================================================
// 4. Round trip that sounds the same
// ===================================================================================

#[test]
fn audit_render_is_deterministic_in_process() {
    let dir = fresh("det");
    let p = rich_project(&dir.join("media"));
    let a = interleave(export_render(&p));
    let b = interleave(export_render(&p));
    let (n, max, first) = compare(&a, &b);
    println!("AUDIT det: len {} rms {} diff {n} max {max} first {first:?}", a.len(), rms(&a));
    assert!(rms(&a) > 0.001, "the rich project makes sound");
    assert_eq!(n, 0, "two renders of the same project differ (max {max}, first {first:?})");
    for scene in 0..3 {
        let (n, max, _) = compare(&scene_render(&p, scene, 96_000), &scene_render(&p, scene, 96_000));
        assert_eq!(n, 0, "scene {scene}: {max}");
    }
}

#[test]
fn audit_rich_project_round_trip_is_bit_identical() {
    let dir = fresh("rt");
    let original = rich_project(&dir.join("fuera"));
    let before_any = interleave(export_render(&original));

    // Through the app: save (collects samples into the folder), close, open in a fresh app.
    let file = dir.join("Proyectos/Tema ñ/Tema ñ.pulse");
    let mut app = PulseApp::headless(original.clone());
    app.save_to_path(&file);
    assert!(!app.is_dirty(), "{:?}", app.status);
    let collected = app.project.clone();
    let before = interleave(export_render(&collected));
    let (n0, m0, _) = compare(&before_any, &before);
    println!("AUDIT rt: collect changed sound? diff {n0} max {m0}");
    let text1 = std::fs::read_to_string(&file).unwrap();

    let loaded = Project::load(&file).unwrap();
    let reopened = PulseApp::headless(loaded.clone());
    assert_eq!(collected, reopened.project, "the model is the same after save → load");
    assert_eq!(collected.to_json().unwrap(), reopened.project.to_json().unwrap(), "JSON (with plugin state) the same");
    let after = interleave(export_render(&reopened.project));
    let (n, max, first) = compare(&before, &after);
    println!("AUDIT rt: export len {} rms {} diff {n} max {max} first {first:?}", before.len(), rms(&before));
    assert_eq!(n, 0, "export after save/load differs");
    for scene in 0..3 {
        let (n, max, _) = compare(&scene_render(&collected, scene, 120_000), &scene_render(&reopened.project, scene, 120_000));
        assert_eq!(n, 0, "scene {scene} differs after save/load: {max}");
    }

    // save → load → save: the same bytes.
    let mut again = PulseApp::headless(loaded);
    again.project.tempo += 1.0;
    again.project.tempo -= 1.0;
    again.save_to_path(&file);
    let text2 = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text1, text2, "second save changed the file");
    // Samples are in the folder, relative.
    assert!(text1.contains("\"Samples/Importados/tono á.wav\""), "relative sample path");
    assert!(!text1.contains(&dir.join("fuera").to_string_lossy().to_string()), "no absolute paths to the outside left");

    // The whole folder moved elsewhere (another disk): sounds the same.
    let moved_dir = dir.join("Otro disco/Tema ñ");
    std::fs::create_dir_all(moved_dir.parent().unwrap()).unwrap();
    std::fs::rename(file.parent().unwrap(), &moved_dir).unwrap();
    std::fs::remove_dir_all(dir.join("fuera")).unwrap();
    let moved = Project::load(&moved_dir.join("Tema ñ.pulse")).unwrap();
    assert!(crate::library::missing_files(&moved).is_empty(), "{:?}", crate::library::missing_files(&moved));
    let (n, max, _) = compare(&before, &interleave(export_render(&moved)));
    assert_eq!(n, 0, "moved folder differs: {max}");
}

#[test]
fn audit_demo_and_fixtures_round_trip_bit_identical() {
    let dir = fresh("fixtures");
    let cases = [
        ("demo", Project::demo().to_json().unwrap()),
        ("format0", include_str!("../fixtures/format0-demo.pulse").to_string()),
        ("format1", include_str!("../fixtures/format1-demo.pulse").to_string()),
    ];
    for (name, text) in cases {
        let p = Project::from_json(&text).unwrap();
        let a = interleave(export_render(&p));
        let file = dir.join(format!("{name}.pulse"));
        p.save(&file).unwrap();
        let q = Project::load(&file).unwrap();
        let b = interleave(export_render(&q));
        let (n, max, _) = compare(&a, &b);
        println!("AUDIT fixture {name}: len {} rms {:.5} diff {n} max {max}", a.len(), rms(&a));
        assert_eq!(n, 0, "{name}");
        // Re-saved old files: does anything of the old JSON disappear?
        let old: serde_json::Value = serde_json::from_str(&text).unwrap();
        let new: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let mut lost = Vec::new();
        missing_keys(&old, &new, String::new(), &mut lost);
        println!("AUDIT fixture {name}: keys lost on re-save: {lost:?}");
        // Old fixtures vs the current demo: same sound?
        let demo = interleave(export_render(&Project::demo()));
        let (n, max, _) = compare(&a, &demo);
        println!("AUDIT fixture {name} vs current demo: len {} vs {} diff {n} max {max}", a.len(), demo.len());
    }
}

fn missing_keys(old: &serde_json::Value, new: &serde_json::Value, at: String, out: &mut Vec<String>) {
    match (old, new) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            for (k, v) in a {
                match b.get(k) {
                    Some(w) => missing_keys(v, w, format!("{at}.{k}"), out),
                    None if !v.is_null() && v != &serde_json::Value::Bool(false) && v != &serde_json::json!([]) => out.push(format!("{at}.{k}={}", v.to_string().chars().take(60).collect::<String>())),
                    None => {}
                }
            }
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            for (i, (v, w)) in a.iter().zip(b).enumerate() {
                missing_keys(v, w, format!("{at}[{i}]"), out);
            }
        }
        _ => {}
    }
}

/// Test plugin (testplug): its state, set by hand, round-trips and sounds the same.
#[test]
fn audit_plugin_state_round_trip_bit_identical() {
    let dir = fresh("plugin");
    let mut app = PulseApp::headless(Project::demo());
    app.plugin_library.search(&[crate::plugins::tests::test_bundle().parent().unwrap().to_path_buf()]);
    let (synth, gain) = (app.plugin_library.found[0].clone(), app.plugin_library.found[1].clone());
    app.set_plugin_instrument(3, &synth);
    app.add_plugin_effect(engine::Target::Track(3), &gain);
    app.add_plugin_effect(engine::Target::Master, &gain);
    let InstrumentKind::Plugin(id) = app.project.tracks[3].instrument else { panic!("{:?}", app.status) };
    app.samples.plugins[&id].set(0, 0.3);
    let gid = app.project.plugins[1].id;
    app.samples.plugins[&gid].set(0, 0.7);
    app.capture_plugins();
    let before = interleave(export_render(&app.project));
    let file = dir.join("P/P.pulse");
    app.save_to_path(&file);
    let reopened = PulseApp::headless(Project::load(&file).unwrap());
    assert_eq!(app.project.to_json().unwrap(), reopened.project.to_json().unwrap());
    let after = interleave(export_render(&reopened.project));
    let (n, max, first) = compare(&before, &after);
    println!("AUDIT plugin: rms {} diff {n} max {max} first {first:?}", rms(&before));
    assert_eq!(n, 0);

    // The plugin uninstalled: the project opens, says so, and saving keeps its state.
    let mut text = std::fs::read_to_string(&file).unwrap();
    let path = app.project.plugins[0].path.clone();
    text = text.replace(&serde_json::to_string(&path).unwrap(), "\"/nowhere/Gone.vst3\"");
    std::fs::write(&file, &text).unwrap();
    let gone = Project::load(&file).unwrap();
    let state_before = gone.plugins[0].state.clone();
    let mut app2 = PulseApp::headless(gone);
    println!("AUDIT plugin missing: status {:?}", app2.status);
    app2.save_to_path(&file);
    let kept = Project::load(&file).unwrap();
    assert_eq!(kept.plugins[0].state, state_before, "state of a missing plugin survives a save");
    assert_eq!(kept.plugins[0].path, "/nowhere/Gone.vst3");
}

// ===================================================================================
// 1. Saving
// ===================================================================================

/// A save into a folder that can't be written: error, and the old file is untouched.
#[cfg(unix)]
#[test]
fn audit_failed_save_keeps_the_previous_file() {
    let dir = fresh("ro");
    let file = dir.join("T/T.pulse");
    Project::demo().save(&file).unwrap();
    let before = std::fs::read(&file).unwrap();
    let mut perms = std::fs::metadata(file.parent().unwrap()).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o555);
    std::fs::set_permissions(file.parent().unwrap(), perms.clone()).unwrap();
    let mut changed = Project::demo();
    changed.tempo = 99.0;
    let result = changed.save(&file);
    perms.set_mode(0o755);
    std::fs::set_permissions(file.parent().unwrap(), perms).unwrap();
    println!("AUDIT readonly save: {result:?}");
    assert!(result.is_err());
    assert_eq!(std::fs::read(&file).unwrap(), before);

    // Through the app: the error is shown and the project stays unsaved.
    let mut app = PulseApp::headless(Project::demo());
    let mut perms = std::fs::metadata(file.parent().unwrap()).unwrap().permissions();
    perms.set_mode(0o555);
    std::fs::set_permissions(file.parent().unwrap(), perms.clone()).unwrap();
    app.project.tempo = 101.0;
    app.save_to_path(&file);
    perms.set_mode(0o755);
    std::fs::set_permissions(file.parent().unwrap(), perms).unwrap();
    println!("AUDIT readonly app save: status {:?} dirty {}", app.status, app.is_dirty());
    assert!(app.is_dirty());
    assert_eq!(std::fs::read(&file).unwrap(), before);
}

/// Saving onto a full volume (path from PULSE_AUDIT_FULL_DIR, a small RAM disk).
#[test]
#[ignore = "needs PULSE_AUDIT_FULL_DIR on a nearly full volume"]
fn audit_save_on_full_disk() {
    let Ok(full) = std::env::var("PULSE_AUDIT_FULL_DIR") else { return };
    let file = Path::new(&full).join("T/T.pulse");
    let small = Project::demo();
    small.save(&file).unwrap();
    let before = std::fs::read(&file).unwrap();
    // Fill the volume.
    let filler = Path::new(&full).join("filler");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&filler).unwrap();
        let chunk = vec![0u8; 16 << 10];
        while f.write_all(&chunk).is_ok() {}
        let _ = f.sync_all();
    }
    println!("AUDIT full disk: filler {} bytes", std::fs::metadata(&filler).map(|m| m.len()).unwrap_or(0));
    let mut big = rich_project(&base().join("full-media"));
    for _ in 0..4 {
        let more = big.tracks.clone();
        big.tracks.extend(more);
    }
    let result = big.save(&file);
    let left: Vec<_> = std::fs::read_dir(file.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
    println!("AUDIT full disk: save {result:?}; files {left:?}");
    assert!(result.is_err());
    assert_eq!(std::fs::read(&file).unwrap(), before, "the old file is intact");
}

/// The child of `audit_kill_during_save`: saves forever, alternating two projects.
#[test]
#[ignore = "child process for audit_kill_during_save"]
fn audit_save_loop_child() {
    let Ok(path) = std::env::var("PULSE_AUDIT_SAVE_LOOP") else { return };
    let (a, b) = kill_variants();
    let path = PathBuf::from(path);
    std::fs::write(path.with_extension("ready"), b"").unwrap();
    loop {
        a.save(&path).unwrap();
        b.save(&path).unwrap();
    }
}

fn kill_variants() -> (Project, Project) {
    let media = base().join("kill-media");
    let mut a = rich_project(&media);
    for _ in 0..3 {
        let more = a.tracks.clone();
        a.tracks.extend(more);
    }
    let mut b = a.clone();
    b.tempo = 77.0;
    (a, b)
}

/// Kills a process in the middle of saving, many times: the project file is always
/// one of the two complete versions.
#[test]
fn audit_kill_during_save() {
    let dir = fresh("kill");
    let file = dir.join("K/K.pulse");
    let (a, b) = kill_variants();
    a.save(&file).unwrap();
    println!("AUDIT kill: file size {} bytes", std::fs::metadata(&file).unwrap().len());
    let exe = std::env::current_exe().unwrap();
    let mut seed: u64 = 12345;
    let mut seen_tmp = 0;
    for round in 0..25 {
        let _ = std::fs::remove_file(file.with_extension("ready"));
        let mut child = std::process::Command::new(&exe)
            .args(["persist_tests::audit_save_loop_child", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
            .env("PULSE_AUDIT_SAVE_LOOP", &file)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let waiting = std::time::Instant::now();
        while !file.with_extension("ready").exists() {
            assert!(waiting.elapsed().as_secs() < 30, "the saving child never started");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        std::thread::sleep(std::time::Duration::from_millis(5 + (seed >> 33) % 300));
        child.kill().unwrap();
        child.wait().unwrap();
        let leftovers = std::fs::read_dir(file.parent().unwrap()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).count();
        seen_tmp += leftovers;
        let loaded = Project::load(&file).unwrap_or_else(|e| panic!("round {round}: the file does not open after a kill: {e}"));
        assert!(loaded.tempo == a.tempo || loaded.tempo == b.tempo, "round {round}");
        assert_eq!(loaded.tracks.len(), a.tracks.len());
    }
    println!("AUDIT kill: 25 kills, file always complete; temporary files left by a kill: {seen_tmp}");
}

// ===================================================================================
// 2. Loading bad files
// ===================================================================================

/// Loads `text`; if it loads, opens it in an app and renders a little. Returns what happened.
fn try_open(text: &str) -> Result<String, String> {
    let text = text.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(|| match Project::from_json(&text) {
            Err(e) => format!("error: {}", e.chars().take(100).collect::<String>()),
            Ok(p) => {
                let mut app = PulseApp::headless(p.clone());
                app.sync_all();
                let _ = crate::library::missing_files(&p);
                let _ = scene_render(&app.project, 0, 9_600);
                if let Some(range) = crate::export::beat_range(&p, crate::export::Range::Song) {
                    let range = (range.0, range.1.min(range.0 + 2.0));
                    let mut s = crate::sync::SampleCache::default();
                    let mut rs = crate::export::plan(&p, &mut s, &Default::default(), Path::new("/dev/null"));
                    let _ = crate::export::render(rs.remove(0).commands, range, crate::export::Song { tempo: p.tempo.clamp(20.0, 999.0), ring_out: 0.5 }, RATE, &AtomicBool::new(false), &|_| {});
                }
                let _ = p.to_json();
                "opened".into()
            }
        });
        let _ = tx.send(r.map_err(|e| e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()));
    });
    match rx.recv_timeout(std::time::Duration::from_secs(20)) {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(panic)) => Err(format!("PANIC: {panic}")),
        Err(_) => Err("HANG (>20 s)".into()),
    }
}

#[test]
fn audit_bad_files() {
    let demo = Project::demo().to_json().unwrap();
    let v: serde_json::Value = serde_json::from_str(&demo).unwrap();
    let edit = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut w = v.clone();
        f(&mut w);
        w.to_string()
    };
    let mut cases: Vec<(String, String)> = vec![
        ("empty".into(), String::new()),
        ("whitespace".into(), "  \n".into()),
        ("binary".into(), String::from_utf8_lossy(&[0u8, 159, 146, 150, 255, 1, 2]).into_owned()),
        ("json null".into(), "null".into()),
        ("json array".into(), "[]".into()),
        ("json {}".into(), "{}".into()),
        ("truncated half".into(), demo[..demo.len() / 2].to_string()),
        ("format string".into(), edit(&|w| w["format"] = "1".into())),
        ("format 999".into(), edit(&|w| w["format"] = 999.into())),
        ("format -1".into(), edit(&|w| w["format"] = (-1).into())),
        ("tempo string".into(), edit(&|w| w["tempo"] = "fast".into())),
        ("tempo 0".into(), edit(&|w| w["tempo"] = 0.into())),
        ("tempo negative".into(), edit(&|w| w["tempo"] = (-120).into())),
        ("tempo 1e300".into(), edit(&|w| w["tempo"] = 1e300.into())),
        ("tempo 1e999 literal".into(), demo.replacen("\"tempo\": 120.0", "\"tempo\": 1e999", 1)),
        ("NaN literal".into(), demo.replacen("\"tempo\": 120.0", "\"tempo\": NaN", 1)),
        ("missing tracks".into(), edit(&|w| { w.as_object_mut().unwrap().remove("tracks"); })),
        ("missing master".into(), edit(&|w| { w.as_object_mut().unwrap().remove("master"); })),
        ("no returns".into(), edit(&|w| w["returns"] = serde_json::json!([]))),
        ("5 returns".into(), edit(&|w| { let r = w["returns"][0].clone(); w["returns"] = serde_json::json!([r.clone(), r.clone(), r.clone(), r.clone(), r]); })),
        ("no scene names".into(), edit(&|w| w["scene_names"] = serde_json::json!([]))),
        ("3 clip slots".into(), edit(&|w| { let c = w["tracks"][0]["clips"].as_array().unwrap()[..3].to_vec(); w["tracks"][0]["clips"] = c.into(); })),
        ("20 clip slots".into(), edit(&|w| { let mut c = w["tracks"][0]["clips"].as_array().unwrap().clone(); c.resize(20, serde_json::Value::Null); w["tracks"][0]["clips"] = c.into(); })),
        ("3 sends".into(), edit(&|w| w["tracks"][0]["sends"] = serde_json::json!([0.1, 0.2, 0.3]))),
        ("no tracks".into(), edit(&|w| w["tracks"] = serde_json::json!([]))),
        ("100 tracks".into(), edit(&|w| { let t = w["tracks"][0].clone(); w["tracks"] = vec![t; 100].into(); })),
        ("clip length 0".into(), edit(&|w| w["tracks"][0]["clips"][0]["length"] = 0.into())),
        ("clip length negative".into(), edit(&|w| w["tracks"][0]["clips"][0]["length"] = (-4).into())),
        ("arrangement length 0".into(), edit(&|w| { w["tracks"][0]["arrangement"][0]["length"] = 0.into(); w["tracks"][0]["arrangement"][0]["clip"]["length"] = 0.into(); })),
        ("arrangement start negative".into(), edit(&|w| w["tracks"][0]["arrangement"][0]["start"] = (-8).into())),
        ("arrangement at 1e12".into(), edit(&|w| w["tracks"][0]["arrangement"][0]["start"] = 1e12.into())),
        ("note pitch 255".into(), edit(&|w| w["tracks"][1]["clips"][0]["notes"][0]["pitch"] = 255.into())),
        ("note pitch 256".into(), edit(&|w| w["tracks"][1]["clips"][0]["notes"][0]["pitch"] = 256.into())),
        ("note length negative".into(), edit(&|w| w["tracks"][1]["clips"][0]["notes"][0]["length"] = (-1).into())),
        ("note velocity 50".into(), edit(&|w| w["tracks"][1]["clips"][0]["notes"][0]["velocity"] = 50.into())),
        ("automation device 99".into(), edit(&|w| w["tracks"][2]["automation"][0]["param"] = serde_json::json!({"Device": {"device": 99, "param": 99}}))),
        ("automation send 9".into(), edit(&|w| w["tracks"][2]["automation"][0]["param"] = serde_json::json!({"Send": 9}))),
        ("automation instrument 9999".into(), edit(&|w| w["tracks"][2]["automation"][0]["param"] = serde_json::json!({"Instrument": 9999}))),
        ("automation tempo on track, value 0".into(), edit(&|w| { w["tracks"][2]["automation"][0]["param"] = "Tempo".into(); w["tracks"][2]["automation"][0]["points"][0]["value"] = 0.into(); })),
        ("master tempo automation 0 bpm".into(), edit(&|w| w["master"]["automation"] = serde_json::json!([{"param": "Tempo", "points": [{"beat": 0.0, "value": 0.0}, {"beat": 4.0, "value": 0.0}]}]))),
        ("automation unsorted".into(), edit(&|w| w["tracks"][2]["automation"][0]["points"] = serde_json::json!([{"beat": 30.0, "value": 1.0}, {"beat": 0.0, "value": 9e9}]))),
        ("sidechain to missing id".into(), edit(&|w| w["tracks"][0]["devices"][0]["effect"]["Compressor"]["sidechain"] = 9999.into())),
        ("sidechain to itself".into(), edit(&|w| { w["tracks"][0]["id"] = 1.into(); w["tracks"][0]["devices"][0]["effect"]["Compressor"]["sidechain"] = 1.into(); })),
        ("compressor ratio 0".into(), edit(&|w| w["tracks"][0]["devices"][0]["effect"]["Compressor"]["ratio"] = 0.into())),
        ("depth 200 without group".into(), edit(&|w| w["tracks"][2]["depth"] = 200.into())),
        ("group kind on first track".into(), edit(&|w| w["tracks"][0]["kind"] = "Group".into())),
        ("unknown instrument".into(), edit(&|w| w["tracks"][0]["instrument"] = serde_json::json!({"Theremin": {}}))),
        ("plugin instrument id not in list".into(), edit(&|w| w["tracks"][0]["instrument"] = serde_json::json!({"Plugin": 77}))),
        ("plugin effect id not in list".into(), edit(&|w| w["tracks"][0]["devices"][0]["effect"] = serde_json::json!({"Plugin": 77}))),
        ("plugin with bad base64".into(), edit(&|w| { w["plugins"] = serde_json::json!([{"id": 1, "path": "/nope.vst3", "cid": "00", "name": "X", "state": "!!!"}]); w["tracks"][0]["instrument"] = serde_json::json!({"Plugin": 1}); })),
        ("missing sample".into(), edit(&|w| { w["tracks"][0]["instrument"] = serde_json::to_value(InstrumentKind::Sampler(Default::default())).unwrap(); w["tracks"][0]["samples"] = serde_json::json!(["/no/such/file.wav"]); })),
        ("sample is a folder".into(), edit(&|w| { w["tracks"][0]["instrument"] = serde_json::to_value(InstrumentKind::Sampler(Default::default())).unwrap(); w["tracks"][0]["samples"] = serde_json::json!(["/tmp"]); })),
        ("sample is not audio".into(), edit(&|w| { w["tracks"][0]["instrument"] = serde_json::to_value(InstrumentKind::Sampler(Default::default())).unwrap(); w["tracks"][0]["samples"] = serde_json::json!(["/etc/hosts"]); })),
        ("1000 empty sample slots".into(), edit(&|w| w["tracks"][0]["samples"] = vec![serde_json::Value::String("x".into()); 1000].into())),
        ("loop end < start".into(), edit(&|w| { w["loop_start"] = 32.into(); w["loop_end"] = 4.into(); w["loop_enabled"] = true.into(); })),
        ("swing grid 0".into(), edit(&|w| { w["swing_grid"] = 0.into(); w["swing"] = 1.into(); })),
        ("quantize negative".into(), edit(&|w| w["quantize"] = (-1).into())),
        ("color 3 → 2 components".into(), edit(&|w| w["tracks"][0]["color"] = serde_json::json!([1, 2]))),
        ("key tonic 200".into(), edit(&|w| w["key"] = serde_json::json!({"tonic": 200, "minor": false}))),
        ("midi_map junk".into(), edit(&|w| w["midi_map"] = serde_json::json!([{"cc": 300, "control": "x"}]))),
    ];
    // Truncated at many points: never opens, never panics.
    for cut in (1..demo.len()).step_by(demo.len() / 40) {
        cases.push((format!("truncated at {cut}"), demo[..cut].to_string()));
    }
    let mut problems = Vec::new();
    for (name, text) in &cases {
        let r = try_open(text);
        println!("AUDIT bad [{name}]: {r:?}");
        if r.is_err() {
            problems.push(format!("{name}: {r:?}"));
        }
    }
    println!("AUDIT bad: {} cases, problems: {problems:#?}", cases.len());
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Random mutations of the fixtures: values replaced by others of any type or a
/// strange number, and bytes changed. Nothing may panic or hang.
#[test]
fn audit_fuzz_fixtures() {
    let iterations: usize = std::env::var("PULSE_AUDIT_FUZZ").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    let sources = [include_str!("../fixtures/format0-demo.pulse"), include_str!("../fixtures/format1-demo.pulse")];
    let mut seed: u64 = 0x5EED;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let weird = [serde_json::json!(null), serde_json::json!(-1), serde_json::json!(0), serde_json::json!(1e300), serde_json::json!(-1e300), serde_json::json!(4294967296u64), serde_json::json!(-0.0), serde_json::json!("x"), serde_json::json!([]), serde_json::json!({}), serde_json::json!(true), serde_json::json!(255), serde_json::json!(1e-300), serde_json::json!(65)];
    let (mut opened, mut errors, mut problems) = (0, 0, Vec::new());
    for i in 0..iterations {
        let src = sources[i % 2];
        let text = if i % 4 == 3 {
            // Byte-level.
            let mut b = src.as_bytes().to_vec();
            for _ in 0..1 + rnd() % 4 {
                let at = (rnd() as usize) % b.len();
                b[at] = b"0123456789-.e,:{}[]\"nul "[(rnd() % 24) as usize];
            }
            String::from_utf8_lossy(&b).into_owned()
        } else {
            let mut v: serde_json::Value = serde_json::from_str(src).unwrap();
            for _ in 0..1 + rnd() % 3 {
                mutate(&mut v, &mut rnd, &weird);
            }
            v.to_string()
        };
        match try_open(&text) {
            Ok(s) if s == "opened" => opened += 1,
            Ok(_) => errors += 1,
            Err(e) => {
                println!("AUDIT fuzz #{i}: {e}");
                problems.push((i, e, text.chars().take(0).collect::<String>()));
                if problems.len() == 1 {
                    std::fs::write(base().join(format!("fuzz-{i}.pulse")), &text).unwrap();
                }
            }
        }
    }
    println!("AUDIT fuzz: {iterations} iterations: opened {opened}, refused {errors}, problems {}", problems.len());
    let mut kinds: Vec<String> = problems.iter().map(|(_, e, _)| e.lines().next().unwrap_or("").chars().take(140).collect()).collect();
    kinds.sort();
    kinds.dedup();
    println!("AUDIT fuzz distinct problems: {kinds:#?}");
    assert!(problems.is_empty());
}

fn mutate(v: &mut serde_json::Value, rnd: &mut impl FnMut() -> u64, weird: &[serde_json::Value]) {
    // Walk down to a random node.
    let mut node = v;
    loop {
        let children = match node {
            serde_json::Value::Object(m) => m.len(),
            serde_json::Value::Array(a) => a.len(),
            _ => 0,
        };
        if children == 0 || rnd() % 5 == 0 {
            break;
        }
        let k = (rnd() as usize) % children;
        node = match node {
            serde_json::Value::Object(m) => m.values_mut().nth(k).unwrap(),
            serde_json::Value::Array(a) => &mut a[k],
            _ => unreachable!(),
        };
    }
    match rnd() % 4 {
        0 => *node = weird[(rnd() as usize) % weird.len()].clone(),
        1 if node.is_number() => *node = serde_json::json!(node.as_f64().unwrap_or(0.0) * -7.5),
        2 if node.is_array() => {
            let a = node.as_array_mut().unwrap();
            if !a.is_empty() && rnd() % 2 == 0 {
                a.pop();
            } else if let Some(x) = a.first().cloned() {
                a.push(x);
            }
        }
        _ if node.is_object() => {
            let m = node.as_object_mut().unwrap();
            if let Some(k) = m.keys().nth((rnd() as usize) % m.len().max(1)).cloned() {
                m.remove(&k);
            }
        }
        _ => *node = weird[(rnd() as usize) % weird.len()].clone(),
    }
}

// ===================================================================================
// 3. Samples
// ===================================================================================

#[test]
fn audit_samples_moved_replaced_and_missing() {
    let dir = fresh("samples");
    let media = dir.join("fuera");
    let file = dir.join("S/S.pulse");
    let mut app = PulseApp::headless(rich_project(&media));
    app.save_to_path(&file);
    let reference = interleave(export_render(&app.project));
    let sample = file.parent().unwrap().join("Samples/Importados/tono á.wav");
    assert!(sample.exists());

    // Deleted: opens, reports, renders silence where it was, keeps its path on save.
    let backup = dir.join("backup.wav");
    std::fs::rename(&sample, &backup).unwrap();
    let missing = Project::load(&file).unwrap();
    let list = crate::library::missing_files(&missing);
    let mut app2 = PulseApp::headless(missing.clone());
    app2.sync_all();
    println!("AUDIT samples: missing {list:?}; errors {:?}", app2.samples.errors.first());
    let without = interleave(export_render(&missing));
    let (n, max, _) = compare(&reference, &without);
    println!("AUDIT samples: render with a missing sample differs in {n} samples (max {max}) — no error from render");
    app2.project.tempo += 0.0;
    app2.save_to_path(&file);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("Samples/Importados/tono á.wav"), "the missing file's path is kept");
    // Put back: identical again.
    std::fs::rename(&backup, &sample).unwrap();
    let (n, _, _) = compare(&reference, &interleave(export_render(&Project::load(&file).unwrap())));
    assert_eq!(n, 0, "the file put back plays as before");

    // Replaced by a different file with the same name: no check at all.
    write_wav(&sample, 2.0, 330.0);
    let replaced = Project::load(&file).unwrap();
    assert!(crate::library::missing_files(&replaced).is_empty());
    let (n, max, _) = compare(&reference, &interleave(export_render(&replaced)));
    println!("AUDIT samples: same-name different file: opens silently, render differs in {n} samples (max {max})");
    assert!(n > 0);
}

/// Cross-version: with PULSE_AUDIT_OUT set, writes the rich project and its render
/// (raw f32) there; with PULSE_AUDIT_COMPARE also set, loads the project another
/// version wrote there and compares its render with that version's.
#[test]
#[ignore = "cross-version check, driven by PULSE_AUDIT_OUT"]
fn audit_cross_version() {
    let Ok(out) = std::env::var("PULSE_AUDIT_OUT") else { return };
    let out = PathBuf::from(out);
    let file = out.join("rich/rich.pulse");
    let mute: Option<usize> = std::env::var("PULSE_AUDIT_MUTE").ok().and_then(|s| s.parse().ok());
    let solo: Option<usize> = std::env::var("PULSE_AUDIT_SOLO").ok().and_then(|s| s.parse().ok());
    let raw = |p: &Project| -> Vec<u8> {
        let mut p = p.clone();
        if let Some(m) = mute {
            p.tracks[m].mixer.active = false;
        }
        if let Some(s) = solo {
            for (i, t) in p.tracks.iter_mut().enumerate() {
                t.mixer.active = i == s;
            }
        }
        interleave(export_render(&p)).iter().flat_map(|s| s.to_le_bytes()).collect()
    };
    if std::env::var("PULSE_AUDIT_COMPARE").is_err() {
        let _ = std::fs::remove_dir_all(out.join("rich"));
        let p = rich_project(&out.join("rich/Samples/Importados"));
        p.save(&file).unwrap();
        std::fs::write(out.join("rich-render.f32"), raw(&Project::load(&file).unwrap())).unwrap();
        return;
    }
    let old = std::fs::read(out.join("rich-render.f32")).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    let p = Project::load(&file).unwrap();
    let new = raw(&p);
    let f = |b: &[u8]| -> Vec<f32> { b.chunks(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect() };
    let (a, b) = (f(&old), f(&new));
    let (n, max, first) = compare(&a, &b);
    let diff: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x - y).collect();
    println!("AUDIT cross: len {} vs {}; differing {n}; max {max}; first {:?}s; rms sig {} rms diff {}", a.len(), b.len(), first.map(|i| i as f32 / 2.0 / RATE), rms(&a), rms(&diff));
    let mut lost = Vec::new();
    let resaved: serde_json::Value = serde_json::from_str(&p.to_json().unwrap()).unwrap();
    missing_keys(&serde_json::from_str(&text).unwrap(), &resaved, String::new(), &mut lost);
    println!("AUDIT cross: keys lost when the new version re-saves: {lost:?}");
}
