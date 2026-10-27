//! A track keyed by the group it is in (a gate, a compressor, a ducker
//! listening to its own bus) sounds in its stem and frozen as in the mix.

use crate::app::PulseApp;
use crate::export::{beat_range, plan, render, Range, Settings, Song};
use crate::project::{Project, Track};
use crate::sync::{self, SampleCache};
use engine::model::*;
use engine::{Command, OfflineRenderer};
use std::collections::HashSet;
use std::path::PathBuf;

const SR: f32 = 48_000.0;
type Stereo = (Vec<f32>, Vec<f32>);

fn media() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn note(start: f64, length: f64, pitch: u8, velocity: f32) -> Note {
    Note { start, length, pitch, velocity, chance: 1.0 }
}

fn drums(id: u32, beats: f64) -> Track {
    let mut track = Track::new("Drums", [200, 0, 0], InstrumentKind::DrumMachine(Default::default()));
    track.id = id;
    let mut beat: Vec<Note> = (0..16).map(|s| note(s as f64 * 0.25, 0.2, drum_note::CLOSED_HAT, if s % 2 == 0 { 0.7 } else { 0.4 })).collect();
    beat.extend((0..4).map(|b| note(b as f64, 0.25, drum_note::KICK, 1.0)));
    beat.extend([1.0, 3.0].map(|b| note(b, 0.25, drum_note::SNARE, 0.9)));
    track.arrangement = vec![ArrangementClip { start: 0.0, length: beats, clip: Clip::Midi(MidiClip { notes: beat, ..MidiClip::new("beat", 4.0, [0; 3]) }) }];
    track
}

/// The engine playing from beat 0 in callbacks of `block`, after `warm` frames.
fn play_in(project: &Project, frames: usize, block: usize, warm: usize) -> Stereo {
    let mut renderer = OfflineRenderer::new(SR);
    for command in sync::project_commands(project, &mut SampleCache::default(), SR, &HashSet::new()) {
        renderer.send(command);
    }
    let (mut l, mut r) = (Vec::new(), Vec::new());
    if warm > 0 {
        renderer.render(warm, &mut l, &mut r);
    }
    l.clear();
    r.clear();
    for command in [Command::SetMetronome(false), Command::SetOutputLimit(false), Command::BackToArrangement, Command::SetPosition(0.0), Command::Play] {
        renderer.send(command);
    }
    while l.len() < frames {
        renderer.render(block.min(frames - l.len()), &mut l, &mut r);
    }
    (l, r)
}

fn play(project: &Project, frames: usize) -> Stereo {
    play_in(project, frames, 512, 4096)
}

fn db(x: f32) -> f32 {
    if x > 0.0 { 20.0 * x.log10() } else { -999.0 }
}

fn rms(x: &Stereo, r: std::ops::Range<usize>) -> f32 {
    ((x.0[r.clone()].iter().chain(&x.1[r.clone()]).map(|s| (*s as f64).powi(2)).sum::<f64>()) / (2 * r.len()) as f64).sqrt() as f32
}

/// Worst 20 ms loudness difference over `range` (where `a` is above -60 dB),
/// and where (frame).
fn gap(a: &Stereo, b: &Stereo, range: std::ops::Range<usize>) -> (f32, usize) {
    let n = range.end.min(a.0.len()).min(b.0.len());
    let mut worst = (0f32, 0);
    let mut i = range.start;
    while i + 960 <= n {
        let (x, y) = (rms(a, i..i + 960), rms(b, i..i + 960));
        if db(x) > -60.0 || db(y) > -60.0 {
            let d = (db(x.max(1e-9)) - db(y.max(1e-9))).abs();
            if d > worst.0 {
                worst = (d, i);
            }
        }
        i += 960;
    }
    worst
}

fn frames(project: &Project, beats: f64) -> usize {
    (beats * 60.0 / project.tempo * SR as f64) as usize
}

fn headless(project: Project, name: &str) -> PulseApp {
    let mut app = PulseApp::headless(project);
    let dir = media().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    app.file = Some(dir.join(format!("{name}.pulse")));
    app
}

fn wait_freeze(app: &mut PulseApp) {
    let ctx = eframe::egui::Context::default();
    let started = std::time::Instant::now();
    while app.freeze_job.is_some() {
        app.follow_freeze(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(started.elapsed().as_secs() < 120, "the freeze never finished");
    }
}

/// A pad gated by the group it is in (with the drums): its stem has it as in
/// the mix, and the stems add up to the mix (the group, heard with the pad
/// alone, used to key the gate: it never opened). In the mix the pad hears its
/// group a block late (the group comes after it); in the stem it hears a copy
/// of it, on time: the gate moves up to 64 samples apart, hence no exact sum.
#[test]
fn a_track_gated_by_its_own_group_has_its_stem() {
    let mut p = Project::empty();
    let mut group = Track::new_group("Grupo", [0; 3]);
    group.id = 10;
    group.devices.push(Device::new(Effect::Utility(UtilityParams { gain_db: -12.0, width: 1.0, ..Default::default() })));
    let mut d = drums(1, 16.0);
    d.depth = 1;
    let mut pad = Track::new("Pad", [0, 100, 200], InstrumentKind::SuperSaw(Default::default()));
    pad.id = 2;
    pad.depth = 1;
    pad.mixer.gain = 0.3;
    pad.arrangement = vec![ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Midi(MidiClip { notes: vec![note(0.0, 16.0, 57, 0.7)], ..MidiClip::new("p", 16.0, [0; 3]) }) }];
    pad.devices.push(Device::new(Effect::Gate(GateParams { sidechain: Some(10), threshold_db: -30.0, return_db: 1.0, hold_ms: 0.0, release_ms: 30.0, ..Default::default() })));
    p.tracks = vec![group, d, pad];
    assert!(crate::export::stems_add_up(&p), "the dialog says the stems add up");
    let range = beat_range(&p, Range::Song).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let song = Song::of(&p);
    let mix = render(plan(&p, &mut SampleCache::default(), &Settings::default(), std::path::Path::new("g.wav")).remove(0).commands, range, song, SR, &cancel, &|_| {}).unwrap();
    let stems = plan(&p, &mut SampleCache::default(), &Settings { stems: true, stems_master: false, ..Default::default() }, std::path::Path::new("g.wav"));
    let mut sum = (vec![0f32; mix.0.len()], vec![0f32; mix.0.len()]);
    let mut levels = Vec::new();
    for stem in stems {
        let (l, r) = render(stem.commands, range, song, SR, &cancel, &|_| {}).unwrap();
        levels.push((stem.track, db(rms(&(l.clone(), r.clone()), 0..l.len()))));
        for i in 0..sum.0.len().min(l.len()) {
            sum.0[i] += l[i];
            sum.1[i] += r[i];
        }
    }
    let diff: f64 = (0..mix.0.len()).map(|i| ((mix.0[i] - sum.0[i]) as f64).powi(2)).sum::<f64>() / mix.0.len() as f64;
    println!("stems {levels:?}; mix {:.1} dB, mix - sum {:.1} dB", db(rms(&mix, 0..mix.0.len())), 10.0 * diff.log10());
    let pad_stem = levels.iter().find(|(t, _)| *t == Some(2)).unwrap().1;
    assert!(pad_stem > -60.0, "the pad's stem is silent ({pad_stem:.1} dB) though it is heard in the mix");
    assert!(10.0 * diff.log10() < -60.0, "the stems add up to the mix: {:.1} dB apart", 10.0 * diff.log10());
}

/// The same frozen: it sounds as before (it froze silence), at its level and
/// up to the 64 samples of the key (see above).
#[test]
fn a_track_gated_by_its_own_group_freezes_as_it_sounds() {
    let mut p = Project::empty();
    let mut group = Track::new_group("Grupo", [0; 3]);
    group.id = 10;
    group.devices.push(Device::new(Effect::Utility(UtilityParams { gain_db: -12.0, width: 1.0, ..Default::default() })));
    let mut d = drums(1, 16.0);
    d.depth = 1;
    let mut pad = Track::new("Pad", [0, 100, 200], InstrumentKind::SuperSaw(Default::default()));
    pad.id = 2;
    pad.depth = 1;
    pad.mixer.gain = 0.3;
    pad.arrangement = vec![ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Midi(MidiClip { notes: vec![note(0.0, 16.0, 57, 0.7)], ..MidiClip::new("p", 16.0, [0; 3]) }) }];
    pad.devices.push(Device::new(Effect::Gate(GateParams { sidechain: Some(10), threshold_db: -30.0, return_db: 1.0, hold_ms: 0.0, release_ms: 30.0, ..Default::default() })));
    p.tracks = vec![group, d, pad];
    let n = frames(&p, 16.0);
    let before = play(&p, n);
    let mut app = headless(p, "freeze-group-key");
    app.freeze(2);
    wait_freeze(&mut app);
    assert!(app.project.tracks[2].frozen.is_some(), "{:?}", app.status);
    let after = play(&app.project, n);
    let (g, at) = gap(&before, &after, 2400..n);
    let diff: f64 = (0..n).map(|i| ((before.0[i] - after.0[i]) as f64).powi(2)).sum();
    let mut pad_only = app.project.clone();
    pad_only.tracks[2].mixer.gain = 0.0;
    let drums_only = play(&pad_only, n);
    let pad: f64 = (0..n).map(|i| ((before.0[i] - drums_only.0[i]) as f64).powi(2)).sum();
    let pad_after: f64 = (0..n).map(|i| ((after.0[i] - drums_only.0[i]) as f64).powi(2)).sum();
    let level = 10.0 * (pad_after / pad).log10();
    assert!(level.abs() < 1.0, "frozen, the pad is {level:.1} dB from its level before");
    let change = 10.0 * (diff / pad).log10();
    assert!(change < -10.0 && g < 1.0, "frozen, the pad gates differently: the difference is {change:.1} dB of the pad's sound, {g:.2} dB at {at}");
}
