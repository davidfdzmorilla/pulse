//! Freezing and sidechains keep what is heard: a group as a sidechain source,
//! sources of sources, the frozen track's tail and first hit, live solo and
//! stems. Each renders before and after and compares (from the 2026-10b audit).

use crate::app::PulseApp;
use crate::export::{beat_range, plan, render, Range, Settings, Song};
use crate::project::{Project, Track};
use crate::sync::{self, SampleCache};
use engine::model::*;
use engine::{Command, MAX_TRACKS};
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;

const SR: f32 = 48_000.0;
type Stereo = (Vec<f32>, Vec<f32>);

fn note(start: f64, length: f64, pitch: u8, velocity: f32) -> Note {
    Note { start, length, pitch, velocity, chance: 1.0 }
}

fn midi(name: &str, length: f64, notes: Vec<Note>) -> MidiClip {
    MidiClip { notes, ..MidiClip::new(name, length, [0; 3]) }
}

fn drums(beats: f64) -> Track {
    let mut track = Track::new("Drums", [200, 0, 0], InstrumentKind::DrumMachine(Default::default()));
    let mut beat: Vec<Note> = (0..16).map(|s| note(s as f64 * 0.25, 0.2, drum_note::CLOSED_HAT, if s % 2 == 0 { 0.7 } else { 0.4 })).collect();
    beat.extend((0..4).map(|b| note(b as f64, 0.25, drum_note::KICK, 1.0)));
    track.arrangement = vec![ArrangementClip { start: 0.0, length: beats, clip: Clip::Midi(midi("beat", 4.0, beat)) }];
    track
}

fn pad(beats: f64) -> Track {
    let mut pad = Track::new("Pad", [0, 100, 200], InstrumentKind::SuperSaw(Default::default()));
    pad.arrangement = vec![ArrangementClip { start: 0.0, length: beats, clip: Clip::Midi(midi("chords", 8.0, vec![note(0.0, 8.0, 57, 0.7), note(0.0, 8.0, 64, 0.7)])) }];
    pad
}

/// Residual energy of `b - a` relative to `a`, in dB, over the common length.
fn null_db(a: &Stereo, b: &Stereo) -> f32 {
    let n = a.0.len().min(b.0.len());
    let (mut sig, mut diff) = (0f64, 0f64);
    for i in 0..n {
        for (x, y) in [(a.0[i], b.0[i]), (a.1[i], b.1[i])] {
            sig += (x as f64).powi(2);
            diff += ((x - y) as f64).powi(2);
        }
    }
    (10.0 * (diff / sig.max(1e-30)).max(1e-30).log10()) as f32
}

fn max_diff(a: &Stereo, b: &Stereo) -> f32 {
    let n = a.0.len().min(b.0.len());
    (0..n).fold(0f32, |m, i| m.max((a.0[i] - b.0[i]).abs()).max((a.1[i] - b.1[i]).abs()))
}

/// The track `t` heard soloed in the whole project (what the stem should be).
fn whole_soloed(project: &Project, t: usize, range: (f64, f64)) -> Stereo {
    let mut list = sync::project_commands(project, &mut SampleCache::default(), SR, &HashSet::new());
    list.extend((0..MAX_TRACKS).map(|other| Command::SetSolo { track: other, solo: other == t }));
    render(list, range, Song::of(project), SR, &AtomicBool::new(false), &|_| {}).unwrap()
}

fn stem_of(project: &Project, t: usize, settings: &Settings) -> Stereo {
    let renders = plan(project, &mut SampleCache::default(), settings, Path::new("/tmp/audit.wav"));
    let r = renders.into_iter().find(|r| r.track == Some(t)).expect("a stem for the track");
    render(r.commands, r.range, r.song, SR, &AtomicBool::new(false), &|_| {}).unwrap()
}

fn mix(project: &Project, range: (f64, f64)) -> Stereo {
    let list = sync::project_commands(project, &mut SampleCache::default(), SR, &HashSet::new());
    render(list, range, Song::of(project), SR, &AtomicBool::new(false), &|_| {}).unwrap()
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


/// A pad ducked by a group (the drums inside it, its fader down).
fn ducked_by_a_group() -> Project {
    let mut p = Project::empty();
    let group = Track::new_group("Bus", [0; 3]);
    let mut d = drums(16.0);
    d.depth = 1;
    p.tracks = vec![group, d, pad(16.0)];
    p.assign_ids();
    let bus = p.tracks[0].id;
    p.tracks[2].devices.push(Device::new(Effect::Ducker(DuckerParams { depth: 1.0, sidechain: Some(bus), ..Default::default() })));
    p.tracks[0].mixer.gain = 0.0;
    p
}

/// The pad's stem, the pad soloed and the pad frozen all duck as the mix does.
#[test]
fn a_group_as_sidechain_source_ducks_everywhere() {
    let p = ducked_by_a_group();
    let range = (0.0, 16.0);
    let heard = mix(&p, range);
    let plain = {
        let mut q = p.clone();
        q.tracks[2].devices.clear();
        mix(&q, range)
    };
    assert!(null_db(&plain, &heard) > -20.0, "it ducks at all");
    let stem = stem_of(&p, 2, &Settings { stems: true, ..Default::default() });
    assert!(null_db(&heard, &stem) < -60.0, "stem: {:.1} dB", null_db(&heard, &stem));
    assert!(max_diff(&whole_soloed(&p, 2, range), &stem) == 0.0);
    let mut soloed = p.clone();
    soloed.tracks[2].solo = true;
    assert!(null_db(&heard, &mix(&soloed, range)) < -60.0, "soloed live");
    let mut app = PulseApp::headless(p);
    app.freeze(2);
    wait_freeze(&mut app);
    assert!(app.project.tracks[2].frozen.is_some(), "{:?}", app.status);
    let frozen = mix(&app.project, beat_range(&app.project, Range::Song).unwrap());
    assert!(null_db(&heard, &frozen) < -30.0, "frozen: {:.1} dB", null_db(&heard, &frozen));
}

/// The pad ducks to the bass, which ducks to the drums: frozen, the pad
/// still ducks; changing the drums unfreezes it.
#[test]
fn a_freeze_follows_sources_of_sources() {
    let mut p = Project::empty();
    let mut bass = Track::new("Bass", [0; 3], InstrumentKind::Synth(Default::default()));
    bass.arrangement = vec![ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Midi(midi("b", 2.0, vec![note(0.0, 1.5, 36, 1.0)])) }];
    p.tracks = vec![drums(16.0), bass, pad(16.0)];
    p.assign_ids();
    let (d, b) = (p.tracks[0].id, p.tracks[1].id);
    p.tracks[1].devices.push(Device::new(Effect::Ducker(DuckerParams { depth: 1.0, sidechain: Some(d), ..Default::default() })));
    p.tracks[2].devices.push(Device::new(Effect::Compressor(CompressorParams { threshold_db: -40.0, ratio: 20.0, attack_ms: 1.0, sidechain: Some(b), ..Default::default() })));
    p.tracks[0].mixer.gain = 0.0;
    p.tracks[1].mixer.gain = 0.0;
    let range = (0.0, 16.0);
    let before = mix(&p, range);
    let mut app = PulseApp::headless(p);
    app.freeze(2);
    wait_freeze(&mut app);
    let after = mix(&app.project, beat_range(&app.project, Range::Song).unwrap());
    assert!(null_db(&before, &after) < -30.0, "{:.1} dB", null_db(&before, &after));
    let content = crate::freeze::content(&app.project, 2);
    if let InstrumentKind::DrumMachine(d) = &mut app.project.tracks[0].instrument {
        d.level[0] *= 0.5;
    }
    assert_ne!(crate::freeze::content(&app.project, 2), content, "the drums are part of it");
}

/// A frozen track with a long delay: the export keeps its tail, and its first
/// hit is as loud as before (no fade-in on a file that starts in silence).
#[test]
fn a_frozen_track_exports_its_tail_and_first_hit() {
    let mut p = Project::empty();
    let mut d = drums(16.0);
    d.devices.push(Device::new(Effect::Delay(DelayParams { time_beats: 0.75, feedback: 0.6, mix: 0.4, ping_pong: false, ..Default::default() })));
    p.tracks = vec![d];
    let export_it = |p: &Project| {
        let r = plan(p, &mut SampleCache::default(), &Settings::default(), Path::new("/tmp/freeze-tail.wav")).remove(0);
        render(r.commands, r.range, r.song, SR, &AtomicBool::new(false), &|_| {}).unwrap()
    };
    let before = export_it(&p);
    let mut app = PulseApp::headless(p);
    app.freeze(0);
    wait_freeze(&mut app);
    assert!(beat_range(&app.project, Range::Song).unwrap().1 > 16.0, "the song lasts the frozen tail");
    let after = export_it(&app.project);
    let song = (16.0 * 60.0 / app.project.tempo * SR as f64) as usize;
    let energy = |x: &Stereo, a: usize| x.0.iter().skip(a).map(|s| (*s as f64).powi(2)).sum::<f64>();
    assert!(energy(&after, song) > 0.5 * energy(&before, song), "tail {} vs {}", energy(&after, song), energy(&before, song));
    let cut = |x: &Stereo, a: usize, b: usize| (x.0[a..b].to_vec(), x.1[a..b].to_vec());
    assert!(null_db(&cut(&before, 0, 480), &cut(&after, 0, 480)) < -30.0, "first 10 ms: {:.1} dB", null_db(&cut(&before, 0, 480), &cut(&after, 0, 480)));
    assert!(null_db(&cut(&before, 0, song), &cut(&after, 0, song)) < -40.0);
}

