//! What is exported sounds like what plays: the same samples, whatever the
//! callback size and wherever the engine was when Play was pressed (drum
//! noise, the new effects with automated output, sections from markers).

use crate::export::{beat_range, plan, render, Range, Settings, Song};
use crate::project::{Project, SongMarker, Track};
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

/// A WAV of `seconds` of `f(i)` (mono into both sides), written once per name.
fn wav(name: &str, seconds: f64, f: impl Fn(usize) -> f32) -> String {
    let path = media().join(format!("{name}-{}.wav", std::process::id()));
    if !path.exists() {
        let n = (seconds * SR as f64) as usize;
        let l: Vec<f32> = (0..n).map(&f).collect();
        crate::export::write(&path, &l, &l, SR as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
    }
    path.to_string_lossy().into_owned()
}

/// A steady 220 Hz tone at -9 dBFS, 20 s.
fn tone() -> String {
    wav("tone", 20.0, |i| 0.35 * (std::f32::consts::TAU * 220.0 * i as f32 / SR).sin())
}

/// A tone that grows louder (so turning it round is heard).
fn ramp() -> String {
    wav("ramp", 20.0, |i| (i as f32 / (20.0 * SR)) * 0.6 * (std::f32::consts::TAU * 330.0 * i as f32 / SR).sin())
}

fn audio(sample: &str, length: f64, warp: WarpMode, fades: Fades) -> AudioClip {
    AudioClip {
        hard_start: false,
        fades,
        name: "a".into(),
        color: [0; 3],
        sample: sample.into(),
        length,
        offset: 0.0,
        warp,
        original_bpm: 120.0,
        gain_db: 0.0,
        transpose: 0.0,
        envelopes: Vec::new(),
        warp_markers: Vec::new(),
        takes: Vec::new(),
        key: None,
        muted: false,
    }
}

fn note(start: f64, length: f64, pitch: u8, velocity: f32) -> Note {
    Note { start, length, pitch, velocity, chance: 1.0 }
}

fn lane(param: Param, points: &[(f64, f32)]) -> AutomationLane {
    AutomationLane { param, points: points.iter().map(|&(beat, value)| AutomationPoint { beat, value, curve: 0.0 }).collect() }
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


/// The song exported (mix).
fn export_mix(project: &Project) -> Stereo {
    let range = beat_range(project, Range::Song).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let r = plan(project, &mut SampleCache::default(), &Settings::default(), std::path::Path::new("/tmp/x.wav")).remove(0);
    render(r.commands, range, Song::of(project), SR, &cancel, &|_| {}).unwrap()
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

fn max_diff(a: &Stereo, b: &Stereo, n: usize) -> (f32, usize) {
    let mut worst = (0f32, 0);
    for i in 0..n.min(a.0.len()).min(b.0.len()) {
        let d = (a.0[i] - b.0[i]).abs().max((a.1[i] - b.1[i]).abs());
        if d > worst.0 {
            worst = (d, i);
        }
    }
    worst
}

fn frames(project: &Project, beats: f64) -> usize {
    (beats * 60.0 / project.tempo * SR as f64) as usize
}





#[test]
fn the_drums_play_as_exported() {
    let mut p = Project::empty();
    p.tracks = vec![drums(1, 8.0)];
    let exported = export_mix(&p);
    let n = frames(&p, 8.0);
    let live = play_in(&p, n, 512, 48_000);
    let again = play_in(&p, n, 512, 96_000);
    let (d, at) = max_diff(&exported, &live, n);
    let (d2, at2) = max_diff(&live, &again, n);
    let (g, gat) = gap(&exported, &live, 0..n);
    println!("export vs live: {d} at {at} ({g:.2} dB at {gat}); live vs live: {d2} at {at2}");
    assert!(d < 1e-4, "the drums differ live and exported: samples up to {d} apart (frame {at})");
}

/// The new effects (a Gate keyed by the drums, Transient, EQ Eight) with
/// their settings and output automated: the export is the playback, sample
/// for sample, whatever the callback size and wherever Play was pressed,
/// also with the tempo automated.
#[test]
fn the_new_effects_play_as_exported() {
    for tempo_automated in [false, true] {
        let mut p = Project::empty();
        let mut a = Track::new_audio("Audio", [0; 3]);
        a.id = 2;
        a.arrangement = vec![
            ArrangementClip { start: 0.5, length: 6.0, clip: Clip::Audio(audio(&tone(), 6.0, WarpMode::Stretch, Fades { fade_in: 1.0, fade_out: 2.0 })) },
            ArrangementClip { start: 6.5, length: 5.5, clip: Clip::Audio(audio(&ramp(), 5.5, WarpMode::Repitch, Fades { fade_in: 0.5, fade_out: 1.5 })) },
        ];
        a.devices.push(Device::new(Effect::Gate(GateParams { sidechain: Some(1), threshold_db: -30.0, hold_ms: 5.0, release_ms: 40.0, ..Default::default() })));
        a.devices.push(Device::new(Effect::Transient(TransientParams { attack: 0.8, sustain: -0.5, output_db: 0.0 })));
        a.devices.push(Device::new(Effect::EqEight(Default::default())));
        a.automation.push(lane(Param::Device { device: 1, param: 0 }, &[(0.0, -1.0), (12.0, 1.0)]));
        a.automation.push(lane(Param::Device { device: 1, param: 2 }, &[(0.0, -6.0), (6.0, 6.0), (6.01, -3.0)]));
        a.automation.push(lane(Param::Device { device: 2, param: 24 }, &[(0.0, -6.0), (8.0, 4.0)]));
        a.automation.push(lane(Param::Device { device: 0, param: 0 }, &[(0.0, -50.0), (12.0, -10.0)]));
        let mut d = drums(1, 12.0);
        d.devices.push(Device::new(Effect::Gate(GateParams { threshold_db: -20.0, ..Default::default() })));
        d.devices.push(Device::new(Effect::Transient(TransientParams { attack: -0.6, sustain: 0.7, output_db: -2.0 })));
        // Heard only through the gate's key.
        d.mixer.gain = 0.0;
        p.tracks = vec![d, a];
        if tempo_automated {
            p.master.automation = vec![lane(Param::Tempo, &[(0.0, 120.0), (12.0, 96.0)])];
        }
        let exported = export_mix(&p);
        let n = exported.0.len().min(6 * SR as usize);
        for (block, warm) in [(1024, 4096), (64, 4096), (256, 4096), (512, 4096), (1000, 4096), (37, 4096), (441, 4096), (512, 4096 + 37), (1000, 4096 + 13)] {
            let live = play_in(&p, n, block, warm);
            let (d, at) = max_diff(&exported, &live, n);
            assert!(d < 1e-6, "tempo automated {tempo_automated}, block {block}, start {warm}: samples up to {d} apart at {at}");
        }
    }
}

#[test]
fn a_section_from_a_marker_off_the_beat_is_the_songs_slice() {
    let mut q = Project::empty();
    let mut pad = Track::new("Pad", [0, 100, 200], InstrumentKind::Synth(Default::default()));
    pad.arrangement = vec![ArrangementClip { start: 0.0, length: 32.0, clip: Clip::Midi(MidiClip { notes: (0..32).map(|b| note(b as f64, 0.5, 60 + (b % 5) as u8, 0.8)).collect(), ..MidiClip::new("p", 32.0, [0; 3]) }) }];
    q.tracks = vec![pad];
    q.markers = vec![SongMarker { beat: 4.0, name: "B".into() }, SongMarker { beat: 9.5, name: "C".into() }];
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let whole = export_mix(&q);
    let mut worst = (0f32, String::new());
    for part in plan(&q, &mut SampleCache::default(), &Settings { range: Range::Sections, ..Default::default() }, std::path::Path::new("/tmp/m.wav")) {
        let (b0, b1) = part.range;
        let mut map = Vec::new();
        let got = crate::export::render_mapped(part.commands, part.range, part.song, SR, &cancel, &|_| {}, &mut map).unwrap();
        let from = frames(&q, b0);
        let len = frames(&q, b1 - b0).min(got.0.len());
        let slice = (whole.0[from..from + len].to_vec(), whole.1[from..from + len].to_vec());
        let (d, at) = max_diff(&slice, &got, len);
        if d > worst.0 {
            worst = (d, format!("section from beat {b0} starts at beat {:.9}: samples up to {d} off at frame {at}", map[0].0));
        }
    }
    assert!(worst.0 < 1e-4, "{}", worst.1);
}

