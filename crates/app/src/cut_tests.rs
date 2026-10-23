//! Cutting audio clips keeps their sound: a split in the middle of a fade
//! leaves each part its piece of the curve, and a piece consolidated or turned
//! round still cross-fades with its neighbour at the cut.

use crate::app::{ClipRef, PulseApp};
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

fn audio_project(clip: AudioClip, start: f64, length: f64) -> Project {
    let mut p = Project::empty();
    let mut t = Track::new_audio("Audio", [0; 3]);
    t.id = 1;
    t.arrangement = vec![ArrangementClip { start, length, clip: Clip::Audio(clip) }];
    p.tracks = vec![t];
    p
}

#[test]
fn splitting_in_a_fade_in_keeps_its_sound() {
    let p = audio_project(audio(&tone(), 16.0, WarpMode::Off, Fades::new(8.0, 8.0)), 0.0, 16.0);
    let n = frames(&p, 16.0);
    let before = play(&p, n);
    let mut app = headless(p, "split");
    assert!(app.split_clip_at(0, 0, 4.0));
    let after = play(&app.project, n);
    for beat in [2.0, 3.5, 5.0, 7.0, 10.0, 13.0] {
        let at = frames(&app.project, beat);
        println!("beat {beat}: before {:.1} dB, after {:.1} dB", db(rms(&before, at..at + 960)), db(rms(&after, at..at + 960)));
    }
    let (worst, at) = gap(&before, &after, 2400..n);
    assert!(worst < 1.0, "split at beat 4 changes the sound by {worst:.1} dB at beat {:.2}", at as f64 / SR as f64 * 2.0);
}

#[test]
fn splitting_in_a_fade_out_keeps_its_sound() {
    let p = audio_project(audio(&tone(), 16.0, WarpMode::Off, Fades::new(0.0, 8.0)), 0.0, 16.0);
    let n = frames(&p, 16.0);
    let before = play(&p, n);
    let mut app = headless(p, "split2");
    assert!(app.split_clip_at(0, 0, 12.0));
    let after = play(&app.project, n);
    let (worst, at) = gap(&before, &after, 2400..n);
    assert!(worst < 1.0, "split at beat 12 changes the sound by {worst:.1} dB at beat {:.2}", at as f64 / SR as f64 * 2.0);
}

/// Turned round, a piece of a split clip keeps a little of its sound past each
/// end (handles), and plays from after the first: at the cut it cross-fades
/// with its neighbour on real sound, not silence. (Two unrelated sounds — a
/// tone and the same tone backwards — cross-fading cancel here and there: no
/// level is asked of the cut itself.)
#[test]
fn a_reversed_piece_still_cross_fades_at_the_cut() {
    let p = audio_project(audio(&tone(), 8.0, WarpMode::Off, Fades::NONE), 0.0, 8.0);
    let mut app = headless(p, "reverse-piece");
    let m = frames(&app.project, 8.0);
    assert!(app.split_clip_at(0, 0, 4.0));
    app.reverse_clips(vec![ClipRef::Arrangement { track: 0, index: 1 }]);
    let Clip::Audio(reversed) = &app.project.tracks[0].arrangement[1].clip else { panic!() };
    assert_eq!(reversed.offset, engine::HANDLE_SECONDS, "it starts after its handle");
    let file = engine::Sample::load(std::path::Path::new(&reversed.sample)).unwrap();
    let handle = (engine::HANDLE_SECONDS * SR as f64) as usize;
    let head: f32 = (0..handle).map(|i| file.read(i as f64).0.abs()).fold(0.0, f32::max);
    assert!(head > 0.3, "its handle has sound: {head}");
    let after = play(&app.project, m);
    let cut = frames(&app.project, 4.0);
    assert!(db(rms(&after, cut - 480..cut + 480)) > -20.0, "around the cut it sounds");
}

#[test]
fn a_consolidated_piece_still_cross_fades_at_the_cut() {
    let p = audio_project(audio(&tone(), 8.0, WarpMode::Off, Fades::NONE), 0.0, 8.0);
    let mut app = headless(p, "bounce-piece");
    let m = frames(&app.project, 8.0);
    assert!(app.split_clip_at(0, 0, 4.0));
    let split = play(&app.project, m);
    app.consolidate(vec![ClipRef::Arrangement { track: 0, index: 0 }]);
    let after = play(&app.project, m);
    let cut = frames(&app.project, 4.0);
    let low = |x: &Stereo| (cut - 480..cut + 480).step_by(48).map(|i| rms(x, i..i + 96)).fold(f32::MAX, f32::min);
    assert!(db(low(&after)) > db(low(&split)) - 3.0, "a dip at the cut: {:.1} dB vs {:.1} dB", db(low(&after)), db(low(&split)));
}
