//! A hard techno track, the way the genre makes it: kicks of 140–150 BPM with
//! long tails, a ducked bass and rumble, loops of tops. Each test renders and
//! measures what is heard (from the 2026-10b audit).

use crate::drum_gen::{self, Level, Style};
use crate::project::{Project, Track};
use engine::model::*;

const SR: f32 = 48_000.0;

fn render(project: &Project, beats: f64) -> (Vec<f32>, Vec<f32>) {
    let mut samples = crate::sync::SampleCache::default();
    let commands = crate::sync::project_commands(project, &mut samples, SR, &Default::default());
    crate::export::render(commands, (0.0, beats), crate::export::Song::of(project), SR, &Default::default(), &|_| {}).unwrap()
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|s| (s * s) as f64).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

fn note(start: f64, length: f64, pitch: u8, velocity: f32) -> Note {
    Note { start, length, pitch, velocity, chance: 1.0 }
}

fn four_on_the_floor(bars: usize, pitch: u8) -> MidiClip {
    let mut clip = MidiClip::new("4/4", 4.0 * bars as f64, [0; 3]);
    clip.notes = (0..bars * 4).map(|b| note(b as f64, 0.25, pitch, 1.0)).collect();
    clip
}

fn track(name: &str, instrument: InstrumentKind, clip: MidiClip) -> Track {
    let mut track = Track::new(name, [200, 0, 0], instrument);
    let length = clip.length;
    track.arrangement.push(ArrangementClip { start: 0.0, length, clip: Clip::Midi(clip) });
    track
}

fn kick_designer(name: &str) -> InstrumentKind {
    InstrumentKind::Kick(KickParams::presets().into_iter().find(|(n, _)| *n == name).unwrap().1)
}

/// The genre's kicks: every Kick Designer preset, and the Drum Machine's
/// whole Hardgroove and Hard techno beats (kick, claps and hats on one track).
fn kicks() -> Vec<(String, InstrumentKind, MidiClip)> {
    let mut kicks: Vec<_> = KickParams::presets().into_iter().map(|(name, _)| (name.to_string(), kick_designer(name), four_on_the_floor(8, 36))).collect();
    let kit = drum_gen::Kit::from_rows(&DRUM_VOICES.iter().map(|&(p, n)| (p, n.to_string())).collect::<Vec<_>>());
    for style in [Style::Hardgroove, Style::HardTechno] {
        let settings = drum_gen::Settings { style, level: Level::Intense, bars: 8, fills: false, sounds: style.sounds(), ..Default::default() };
        let (notes, groove) = drum_gen::generate(&kit, &settings, 32.0, &[], 0);
        kicks.push((format!("Drum Machine {style:?}"), InstrumentKind::DrumMachine(style.sound()), MidiClip { notes, groove, ..MidiClip::new("Beat", 32.0, [0; 3]) }));
    }
    kicks
}

/// The bass's level (dB) through a beat, in steps of 10 ms, averaged over
/// `beats`, with the kick track muted.
fn bass_through_a_beat(project: &Project, beats: &[usize]) -> Vec<f64> {
    let mut p = project.clone();
    p.tracks[0].mixer.gain = 0.0;
    let (l, _) = render(&p, 32.0);
    let beat = (60.0 / p.tempo * SR as f64) as usize;
    let step = (0.01 * SR) as usize;
    (0..beat / step)
        .map(|k| {
            let sum: f64 = beats.iter().map(|b| rms(&l[b * beat + k * step..][..step])).sum();
            db(sum / beats.len() as f64)
        })
        .collect()
}

#[test]
fn the_sidechain_pumps_on_every_kick_of_the_genre() {
    for (name, instrument, clip) in kicks() {
        let mut project = Project::empty();
        project.tempo = 150.0;
        project.tracks = vec![track("Kick", instrument, clip)];
        // A held bass note, so only the ducking moves its level.
        let mut bass = track("Bass", InstrumentKind::Synth(Default::default()), MidiClip { notes: vec![note(0.0, 32.0, 29, 0.8)], ..MidiClip::new("Sub", 32.0, [0; 3]) });
        project.assign_ids();
        let kick = project.tracks[0].id;
        // What the mix assistant puts on the bass.
        // A hardstyle kick's tail is as loud under 600 Hz as its hit: that one
        // listens to the whole sound.
        let lows = !name.contains("Hardstyle");
        bass.devices.push(Device::new(Effect::Ducker(DuckerParams { depth: 0.6, release: 0.25, rate_beats: 1.0, sidechain: Some(kick), lows, ..Default::default() })));
        project.tracks.push(bass);
        project.assign_ids();
        // The beats from the third bar on whose only kick is on the beat (the
        // Hard techno beat gallops: some have another one a sixteenth before the next).
        let Clip::Midi(clip) = &project.tracks[0].arrangement[0].clip else { unreachable!() };
        let kicks: Vec<f64> = clip.notes.iter().filter(|n| n.pitch == drum_note::KICK).map(|n| n.start).collect();
        let beats: Vec<usize> = (8..31).filter(|&b| kicks.iter().filter(|&&k| k >= b as f64 - 0.01 && k < b as f64 + 1.0).count() == 1 && kicks.iter().any(|&k| (k - b as f64).abs() < 0.01)).collect();
        assert!(beats.len() >= 8, "{name}: {beats:?}");
        let profile = bass_through_a_beat(&project, &beats);
        let top = profile.iter().cloned().fold(f64::MIN, f64::max);
        let shown: Vec<String> = profile.iter().map(|v| format!("{:.0}", v - top)).collect();
        println!("{name}: {shown:?}");
        // Down at once (within 30 ms), deep, and back up for the last third of the beat.
        let dip = profile[..3].iter().cloned().fold(f64::MAX, f64::min);
        assert!(dip < top - 6.0, "{name}: the bass should duck on each kick: {shown:?}");
        // With a whole drum machine as the key its low toms and congas move it a little too.
        let slack = if name.starts_with("Drum Machine") { 1.5 } else { 1.0 };
        let tail = &profile[profile.len() * 2 / 3..];
        assert!(tail.iter().all(|&v| v > top - slack), "{name}: the bass should be back up before the next kick: {shown:?}");
    }
}

