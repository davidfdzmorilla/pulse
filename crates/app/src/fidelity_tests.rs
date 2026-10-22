//! Edits that bounce or rewrite audio keep what was heard: each test renders
//! before and after the edit and compares. Everything here makes its own audio
//! (nothing outside the repository).

use crate::app::PulseApp;
use crate::project::{Project, Track};
use crate::sync::{self, SampleCache};
use engine::model::*;
use engine::{Command, OfflineRenderer};
use std::collections::HashSet;

const SR: f32 = 48_000.0;

type Stereo = (Vec<f32>, Vec<f32>);

fn note(start: f64, length: f64, pitch: u8, velocity: f32) -> Note {
    Note { start, length, pitch, velocity, chance: 1.0 }
}

fn lane(param: Param, points: &[(f64, f32)]) -> AutomationLane {
    AutomationLane { param, points: points.iter().map(|&(beat, value)| AutomationPoint { beat, value, curve: 0.0 }).collect() }
}

fn midi(name: &str, length: f64, notes: Vec<Note>) -> MidiClip {
    MidiClip { notes, ..MidiClip::new(name, length, [0; 3]) }
}

/// A drum beat on a drum machine, as track `id`.
fn drums(id: u32) -> Track {
    let mut track = Track::new("Drums", [200, 0, 0], InstrumentKind::DrumMachine(Default::default()));
    track.id = id;
    let mut beat: Vec<Note> = (0..16).map(|s| note(s as f64 * 0.25, 0.2, drum_note::CLOSED_HAT, if s % 2 == 0 { 0.7 } else { 0.4 })).collect();
    beat.extend((0..4).map(|b| note(b as f64, 0.25, drum_note::KICK, 1.0)));
    track.arrangement = vec![ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Midi(midi("beat", 4.0, beat)) }];
    track
}

/// What the engine plays from beat 0, `frames` long, in blocks of 512, the
/// output limit off, after it has been running a little (as the live one has).
fn play(project: &Project, frames: usize) -> Stereo {
    play_in(project, frames, 512)
}

/// [`play`] with audio callbacks of `block` frames.
fn play_in(project: &Project, frames: usize, block: usize) -> Stereo {
    let mut renderer = OfflineRenderer::new(SR);
    for command in sync::project_commands(project, &mut SampleCache::default(), SR, &HashSet::new()) {
        renderer.send(command);
    }
    let (mut l, mut r) = (Vec::new(), Vec::new());
    renderer.render(4096, &mut l, &mut r);
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

fn db(x: f32) -> f32 {
    if x > 0.0 { 20.0 * x.log10() } else { -999.0 }
}

/// The largest loudness difference between `a` and `b` in 20 ms windows over
/// `range` (where `a` is above −60 dBFS), in dB: timing within a window and
/// phase don't count.
fn loudness_gap(a: &Stereo, b: &Stereo, range: std::ops::Range<usize>) -> f32 {
    let n = range.end.min(a.0.len()).min(b.0.len());
    let rms = |x: &Stereo, r: std::ops::Range<usize>| ((x.0[r.clone()].iter().chain(&x.1[r.clone()]).map(|s| (*s as f64).powi(2)).sum::<f64>()) / (2 * r.len()) as f64).sqrt() as f32;
    let mut worst = 0f32;
    let mut i = range.start;
    while i + 960 <= n {
        let (x, y) = (rms(a, i..i + 960), rms(b, i..i + 960));
        if db(x) > -60.0 {
            worst = worst.max((db(x) - db(y.max(1e-9))).abs());
        }
        i += 960;
    }
    worst
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

fn seconds(project: &Project, beats: f64) -> usize {
    (beats * 60.0 / project.tempo * SR as f64) as usize
}

/// A pad ducked by the drums' sidechain, frozen: it still ducks (it rendered
/// alone, without the drums, and stopped ducking: +7 dB on average).
#[test]
fn a_frozen_track_keeps_ducking_to_its_sidechain() {
    let mut p = Project::empty();
    let mut pad = Track::new("Pad", [0, 100, 200], InstrumentKind::SuperSaw(Default::default()));
    pad.id = 2;
    pad.arrangement = vec![ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Midi(midi("chords", 16.0, vec![note(0.0, 16.0, 57, 0.7), note(0.0, 16.0, 64, 0.7)])) }];
    pad.devices.push(Device::new(Effect::Ducker(DuckerParams { sidechain: Some(1), ..Default::default() })));
    p.tracks = vec![drums(1), pad];
    // Only the pad heard: the drums still drive its ducker.
    p.tracks[0].mixer.gain = 0.0;
    let frames = seconds(&p, 16.0);
    let before = play(&p, frames);
    let mut app = PulseApp::headless(p);
    app.freeze(1);
    wait_freeze(&mut app);
    assert!(app.project.tracks[1].frozen.is_some(), "{:?}", app.status);
    let after = play(&app.project, frames);
    let gap = loudness_gap(&before, &after, 2400..frames);
    assert!(gap < 1.0, "frozen, it ducks as before: {gap:.2} dB apart");
}

/// A song that slows from 120 to 80 BPM, its drums frozen: the hits stay where
/// they were (they drifted up to 44 ms, the frozen audio playing at one tempo).
#[test]
fn a_frozen_track_follows_the_songs_tempo_changes() {
    let mut p = Project::empty();
    p.tracks = vec![drums(1)];
    p.master.automation = vec![lane(Param::Tempo, &[(0.0, 120.0), (16.0, 80.0)])];
    let frames = 10 * SR as usize;
    let before = play(&p, frames);
    let mut app = PulseApp::headless(p);
    app.freeze(0);
    wait_freeze(&mut app);
    assert!(app.project.tracks[0].frozen.is_some(), "{:?}", app.status);
    let after = play(&app.project, frames);
    let gap = loudness_gap(&before, &after, 2400..frames);
    assert!(gap < 1.0, "the same hits at the same times: {gap:.2} dB apart");
}

/// A session clip with a volume envelope, frozen: the envelope moves the
/// frozen clip once (it was baked in and applied again: 12 dB down).
#[test]
fn a_frozen_session_clip_keeps_its_volume_envelope_once() {
    let mut p = Project::empty();
    p.quantize = 0.0;
    let mut synth = Track::new("Synth", [0; 3], InstrumentKind::Synth(Default::default()));
    let mut clip = midi("c", 4.0, vec![note(0.0, 3.9, 60, 0.8)]);
    clip.envelopes.push(lane(Param::Volume, &[(0.0, 0.25), (4.0, 0.25)]));
    synth.clips[0] = Some(Clip::Midi(clip));
    p.tracks = vec![synth];
    let launch = |p: &Project| {
        let mut renderer = OfflineRenderer::new(SR);
        for command in sync::project_commands(p, &mut SampleCache::default(), SR, &HashSet::new()) {
            renderer.send(command);
        }
        renderer.send(Command::SetOutputLimit(false));
        renderer.send(Command::LaunchClip { track: 0, slot: 0 });
        let (mut l, mut r) = (Vec::new(), Vec::new());
        // Two laps; the second is compared.
        renderer.render(2 * 96_000, &mut l, &mut r);
        (l.split_off(96_000), r.split_off(96_000))
    };
    let before = launch(&p);
    let mut app = PulseApp::headless(p);
    app.freeze(0);
    wait_freeze(&mut app);
    let after = launch(&app.project);
    let rms = |x: &Stereo| (x.0.iter().map(|s| s * s).sum::<f32>() / x.0.len() as f32).sqrt();
    let change = db(rms(&after)) - db(rms(&before));
    assert!(change.abs() < 0.5, "{change:.1} dB");
}

/// Two MIDI clips joined: the second's envelope and groove still sound where
/// they did (both were dropped: 17 dB louder, the hits off the swing).
#[test]
fn joined_clips_keep_their_envelopes_and_grooves() {
    let mut p = Project::empty();
    let mut track = drums(1);
    let beat = |groove: Option<Groove>, volume: Option<f32>| {
        let mut clip = midi("beat", 4.0, (0..16).map(|s| note(s as f64 * 0.25, 0.2, drum_note::CLOSED_HAT, 0.8)).collect());
        clip.groove = groove;
        if let Some(v) = volume {
            clip.envelopes.push(lane(Param::Volume, &[(0.0, v)]));
        }
        Clip::Midi(clip)
    };
    let swung = Groove { grid: 0.25, swing: 1.0, accent: 0.3, humanize: 0.5 };
    track.arrangement = vec![ArrangementClip { start: 0.0, length: 4.0, clip: beat(None, None) }, ArrangementClip { start: 4.0, length: 4.0, clip: beat(Some(swung), Some(0.1)) }];
    p.tracks = vec![track];
    let frames = seconds(&p, 8.0);
    let before = play(&p, frames);
    let mut app = PulseApp::headless(p);
    app.set_clip_selection(vec![crate::app::ClipRef::Arrangement { track: 0, index: 0 }, crate::app::ClipRef::Arrangement { track: 0, index: 1 }]);
    app.join_selected();
    assert_eq!(app.project.tracks[0].arrangement.len(), 1, "{:?}", app.status);
    let after = play(&app.project, frames);
    let gap = loudness_gap(&before, &after, 2400..frames);
    assert!(gap < 1.0, "joined, it sounds as before: {gap:.2} dB apart");
}

/// A held note split in two keeps sounding after the cut (it stopped there).
#[test]
fn a_held_note_sounds_on_after_a_split() {
    let mut p = Project::empty();
    let mut synth = Track::new("Synth", [0; 3], InstrumentKind::Synth(Default::default()));
    synth.arrangement = vec![ArrangementClip { start: 0.0, length: 8.0, clip: Clip::Midi(midi("pad", 8.0, vec![note(0.0, 8.0, 57, 0.8)])) }];
    p.tracks = vec![synth];
    let mut app = PulseApp::headless(p);
    assert!(app.split_clip_at(0, 0, 4.0));
    let after = play(&app.project, seconds(&app.project, 8.0));
    let from = seconds(&app.project, 5.0);
    let to = seconds(&app.project, 7.5);
    let level = after.0[from..to].iter().fold(0f32, |m, s| m.max(s.abs()));
    assert!(level > 0.05, "after the cut the note goes on: peak {level}");
}

/// Stems without the master's effects add up to the mix: switched-off tracks
/// and groups are not stems of their own.
#[test]
fn stems_without_the_master_add_up_to_the_mix() {
    use crate::export::{beat_range, plan, render, Range, Settings, Song};
    let mut p = Project::demo();
    p.master.devices.push(Device::new(Effect::Saturator(SaturatorParams { drive_db: 12.0, ..Default::default() })));
    let off = p.tracks.len() - 1;
    p.tracks[off].mixer.active = false;
    let range = beat_range(&p, Range::Song).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let song = Song::of(&p);
    let mix_settings = Settings { stems: false, ..Default::default() };
    let mut plain = p.clone();
    plain.master.devices.clear();
    let mix = render(plan(&plain, &mut SampleCache::default(), &mix_settings, std::path::Path::new("/tmp/mix.wav")).remove(0).commands, range, song, SR, &cancel, &|_| {}).unwrap();
    let stems = plan(&p, &mut SampleCache::default(), &Settings { stems: true, stems_master: false, ..Default::default() }, std::path::Path::new("/tmp/mix.wav"));
    assert_eq!(stems.len(), p.tracks.len() - 1, "the switched-off track has no stem");
    assert!(stems.iter().all(|s| s.track != Some(off)));
    let mut sum = (vec![0f32; mix.0.len()], vec![0f32; mix.0.len()]);
    for stem in stems {
        let (l, r) = render(stem.commands, range, song, SR, &cancel, &|_| {}).unwrap();
        for i in 0..sum.0.len().min(l.len()) {
            sum.0[i] += l[i];
            sum.1[i] += r[i];
        }
    }
    let gap = loudness_gap(&mix, &sum, 0..mix.0.len());
    assert!(gap < 1.0, "the stems add up to the mix: {gap:.2} dB apart");
}

/// What plays is the same whatever size of audio callback the device uses, and
/// the export (in 1024-frame blocks) is what was heard: automation moves on the
/// same frames (it moved once per callback: 20 ms steps at 1024).
#[test]
fn what_plays_does_not_depend_on_the_buffer_size() {
    let mut p = Project::empty();
    let mut pad = Track::new("Pad", [0, 100, 200], InstrumentKind::SuperSaw(Default::default()));
    pad.arrangement = vec![ArrangementClip { start: 0.0, length: 8.0, clip: Clip::Midi(midi("chords", 8.0, vec![note(0.0, 8.0, 57, 0.7), note(0.0, 8.0, 64, 0.7)])) }];
    pad.devices.push(Device::new(Effect::AutoFilter(Default::default())));
    pad.automation.push(lane(Param::Device { device: 0, param: 0 }, &[(0.0, 200.0), (4.0, 8000.0), (8.0, 300.0)]));
    pad.automation.push(lane(Param::Volume, &[(0.0, 0.2), (2.0, 1.0), (2.01, 0.3), (8.0, 1.0)]));
    p.tracks = vec![drums(1), pad];
    let frames = seconds(&p, 8.0);
    let reference = play_in(&p, frames, 1024);
    for block in [32, 37, 64, 441] {
        let other = play_in(&p, frames, block);
        let gap = loudness_gap(&reference, &other, 2400..frames);
        println!("bloque {block}: {gap:.3} dB");
        assert!(gap < 0.5, "{block}-frame callbacks: {gap:.2} dB from the export's");
    }
}

/// The demo with sends into the reverb and delay returns, its first scene laid
/// out on the arrangement, playing in 256-frame callbacks; `action` runs on the
/// app at 3 s. Gives the left channel.
fn playing_through(action: &dyn Fn(&mut PulseApp)) -> Vec<f32> {
    let mut project = Project::demo();
    for track in &mut project.tracks {
        track.sends[..2].copy_from_slice(&[0.5, 0.3]);
        if let Some(clip) = track.clips[0].clone() {
            track.arrangement = vec![ArrangementClip { start: 0.0, length: 32.0, clip }];
        }
    }
    let mut app = PulseApp::headless(project);
    let (audio, mut engine) = crate::app::Audio::detached();
    app.audio = audio;
    app.sync_all();
    app.audio.send(Command::BackToArrangement);
    app.audio.send(Command::Play);
    let mut out = vec![0f32; 256 * 2];
    let mut left = Vec::new();
    for call in 0..6 * 48_000 / 256 {
        if call == 3 * 48_000 / 256 {
            action(&mut app);
        }
        engine.process(&mut out, 2);
        left.extend(out.chunks(2).map(|f| f[0]));
        app.audio.collect_garbage();
    }
    left
}

/// Undo, redo and adding a track while it plays are not heard: what didn't
/// change goes on (the engine got every chain and arrangement again, which cut
/// the reverb and delay tails: 3 to 8 dB down for a quarter of a second).
#[test]
fn undo_and_adding_a_track_while_playing_are_not_heard() {
    let reference = playing_through(&|_| {});
    let from = 3 * 48_000;
    let heard = |other: &[f32]| {
        let signal: f32 = reference[from..].iter().map(|s| s * s).sum();
        let difference: f32 = reference[from..].iter().zip(&other[from..]).map(|(a, b)| (a - b).powi(2)).sum();
        10.0 * (difference / signal).max(1e-20).log10()
    };
    // A device setting changed (and synced), then undone.
    let undone = playing_through(&|app| {
        let t = app.project.tracks.iter().position(|t| !t.devices.is_empty()).expect("a track with effects");
        app.commit_history();
        let device = app.project.tracks[t].devices[0];
        let mut changed = device;
        changed.enabled = !device.enabled;
        app.project.tracks[t].devices[0] = changed;
        app.commit_history();
        app.undo();
        app.redo();
        app.undo();
    });
    let added = playing_through(&|app| app.add_track());
    let (undone, added) = (heard(&undone), heard(&added));
    assert!(undone < -60.0, "undo and redo are heard: {undone:.1} dB");
    assert!(added < -60.0, "adding a track is heard: {added:.1} dB");
}

/// Played twice live (stop, back to the start, play), a random arpeggio and
/// a SuperSaw pad sound the same both times, as they do exported.
#[test]
fn chance_in_the_sound_repeats_on_every_play() {
    for arp in [true, false] {
        let mut p = Project::empty();
        let mut t = Track::new("Pad", [0; 3], if arp { InstrumentKind::Synth(Default::default()) } else { InstrumentKind::SuperSaw(Default::default()) });
        t.arrangement = vec![ArrangementClip { start: 0.0, length: 8.0, clip: Clip::Midi(midi("chords", 8.0, vec![note(0.0, 4.0, 57, 0.7), note(0.0, 4.0, 64, 0.7), note(4.0, 4.0, 60, 0.7)])) }];
        if arp {
            t.midi_effects.push(MidiDevice::new(MidiEffect::Arpeggiator(ArpeggiatorParams { mode: ArpMode::Random, octaves: 3, ..Default::default() })));
        }
        p.tracks = vec![t];
        let frames = (8.0 * 60.0 / p.tempo * SR as f64) as usize;
        let mut renderer = OfflineRenderer::new(SR);
        for c in sync::project_commands(&p, &mut SampleCache::default(), SR, &HashSet::new()) {
            renderer.send(c);
        }
        let mut takes = Vec::new();
        for _ in 0..2 {
            for c in [Command::SetOutputLimit(false), Command::BackToArrangement, Command::SetPosition(0.0), Command::Play] {
                renderer.send(c);
            }
            let (mut l, mut r) = (Vec::new(), Vec::new());
            renderer.render(frames, &mut l, &mut r);
            renderer.send(Command::Stop);
            let (mut a, mut b) = (Vec::new(), Vec::new());
            renderer.render(5 * 48_000, &mut a, &mut b);
            takes.push(l);
        }
        let diff = takes[0].iter().zip(&takes[1]).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        let first = takes[0].iter().zip(&takes[1]).position(|(a, b)| a != b);
        assert!(takes[0] == takes[1], "{}: {diff:e} from {first:?}", if arp { "arpeggio" } else { "SuperSaw" });
    }
}
