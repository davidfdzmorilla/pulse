//! Plugins wired into the mix: a sidechain input hearing another track, an
//! instrument's other outputs on tracks of their own (with their sends and
//! their delay made up for), the load of a slow one, and the flight recorder.
//! All with the test plugin (crates/testplug).

use crate::engine::{ChainSlot, Command, PlayClip, Target};
use crate::model::{Device, Effect, MidiClip, Note, PluginId};
use crate::plugin::tests::{clap_bundle, test_bundle};
use crate::plugin::{instantiate, scan, PluginInfo, PluginProcessor};
use crate::OfflineRenderer;
use std::path::PathBuf;

const SR: f32 = 48_000.0;

fn plugins(bundle: &PathBuf) -> (PluginInfo, PluginInfo) {
    let found = scan(bundle).unwrap();
    (found.iter().find(|p| p.instrument).unwrap().clone(), found.iter().find(|p| !p.instrument).unwrap().clone())
}

fn load(info: &PluginInfo, sr: f32) -> PluginProcessor {
    instantiate(&info.path, &info.cid, sr, None).unwrap().0
}

/// A clip holding `pitches` from its start, for a bar.
fn chord(pitches: &[u8]) -> Box<PlayClip> {
    let mut clip = MidiClip::new("test", 4.0, [0, 0, 0]);
    for &pitch in pitches {
        clip.notes.push(Note { start: 0.0, length: 4.0, pitch, velocity: 1.0, chance: 1.0 });
    }
    Box::new(PlayClip::Midi(clip))
}

/// A renderer with tracks 0..3 named 1..4, playing at once.
fn renderer(sr: f32) -> OfflineRenderer {
    let mut r = OfflineRenderer::new(sr);
    let mut ids = [0u32; crate::MAX_TRACKS];
    for (t, id) in ids.iter_mut().enumerate().take(4) {
        *id = t as u32 + 1;
    }
    r.send(Command::SetTrackIds(ids));
    r.send(Command::SetQuantize(0.0));
    r.send(Command::SetOutputLimit(false));
    r
}

fn render(r: &mut OfflineRenderer, frames: usize) -> Vec<f32> {
    let (mut l, mut rr) = (Vec::new(), Vec::new());
    r.render(frames, &mut l, &mut rr);
    l
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// A plugin effect's sidechain input hears the track it points at (after its
/// effects, before its fader: muted, it is still heard), in the same block
/// whichever comes first; pointed at nothing, it hears silence. VST3 and CLAP.
#[test]
fn a_plugin_sidechain_hears_another_track() {
    for bundle in [test_bundle(), clap_bundle()] {
        let (synth, gain) = plugins(&bundle);
        // The source alone, as the reference: track 1 plays the synth.
        let source = |r: &mut OfflineRenderer| {
            r.send(Command::set_plugin(1, PluginId(1), load(&synth, SR)));
            r.send(Command::SetClip { track: 1, slot: 0, clip: Some(chord(&[69])) });
            r.send(Command::LaunchClip { track: 1, slot: 0 });
        };
        let mut alone = renderer(SR);
        source(&mut alone);
        let reference = render(&mut alone, 24_000);
        assert!(rms(&reference) > 0.2, "{bundle:?}");

        // Track 0, before it, has the Gain at 0× listening to track 1 (id 2), muted.
        let mut r = renderer(SR);
        source(&mut r);
        r.send(Command::SetMute { target: Target::Track(1), mute: true });
        let mut processor = load(&gain, SR);
        assert!(processor.has_sidechain(), "{bundle:?}: the test gain has a sidechain input");
        processor.set_normalized(0, 0.0);
        processor.sidechain = Some(2);
        let device = Device::new(Effect::Plugin(PluginId(2)));
        r.send(Command::InsertDevice { target: Target::Track(0), index: 0, slot: ChainSlot::plugin(&device, processor) });
        let heard = render(&mut r, 24_000);
        let worst = heard.iter().zip(&reference).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        assert!(worst < 1e-5, "{bundle:?}: the sidechain is the source's sound, in time: off by {worst}");

        // Pointed elsewhere (nothing): silence.
        r.send(Command::SetPluginSidechain { id: PluginId(2), source: 0 });
        render(&mut r, 1024);
        assert_eq!(rms(&render(&mut r, 4800)), 0.0, "{bundle:?}");
        // And back.
        r.send(Command::SetPluginSidechain { id: PluginId(2), source: 2 });
        render(&mut r, 1024);
        assert!(rms(&render(&mut r, 4800)) > 0.2, "{bundle:?}");
    }
}

/// A sidechain listening across a loop (a plugin hearing its own track's
/// listener, which hears it) is heard 64 samples late, as built-in effects'.
#[test]
fn a_plugin_sidechain_across_a_loop_is_a_cell_late() {
    let (synth, gain) = plugins(&test_bundle());
    let mut r = renderer(SR);
    r.send(Command::set_plugin(0, PluginId(1), load(&synth, SR)));
    r.send(Command::SetClip { track: 0, slot: 0, clip: Some(chord(&[69])) });
    r.send(Command::LaunchClip { track: 0, slot: 0 });
    r.send(Command::SetMute { target: Target::Track(0), mute: true });
    // Track 1 hears track 0 through the plugin's sidechain (its own input at
    // 0×); track 0 hears track 1 too, through a compressor that doesn't compress.
    let mut processor = load(&gain, SR);
    processor.set_normalized(0, 0.0);
    processor.sidechain = Some(1);
    let device = Device::new(Effect::Plugin(PluginId(2)));
    r.send(Command::InsertDevice { target: Target::Track(1), index: 0, slot: ChainSlot::plugin(&device, processor) });
    let listens = Device::new(Effect::Compressor(crate::model::CompressorParams { threshold_db: 0.0, ratio: 1.0, sidechain: Some(2), ..Default::default() }));
    r.send(Command::InsertDevice { target: Target::Track(0), index: 0, slot: ChainSlot::new(&listens, SR) });
    let out = render(&mut r, 4800);
    // Silent until the first cell has gone by, then the sine.
    let first = out.iter().position(|s| s.abs() > 1e-6).unwrap();
    assert!((60..=70).contains(&first), "late by a cell: {first}");
}

/// A drum plugin's other outputs on tracks of their own: each takes its
/// output, the plugin's track doesn't play them, the source muted mutes them,
/// and their sends reach the returns.
#[test]
fn an_instruments_other_outputs_go_to_their_tracks() {
    for bundle in [test_bundle(), clap_bundle()] {
        let (synth, _) = plugins(&bundle);
        let setup = |takers: &[(usize, u16)]| {
            let mut r = renderer(SR);
            r.send(Command::set_plugin(0, PluginId(1), load(&synth, SR)));
            // C2 on Aux 1 (C1..B1), nothing elsewhere.
            r.send(Command::SetClip { track: 0, slot: 0, clip: Some(chord(&[40])) });
            r.send(Command::LaunchClip { track: 0, slot: 0 });
            let mut from = [0u32; crate::MAX_TRACKS];
            let mut sounds = [0u16; crate::MAX_TRACKS];
            for &(t, mask) in takers {
                from[t] = 1;
                sounds[t] = mask;
            }
            r.send(Command::SetAudioFrom(from));
            r.send(Command::SetSoundsFrom(sounds));
            r
        };
        let level = |r: &mut OfflineRenderer| {
            render(r, 2400);
            for t in 0..3 {
                r.state.tracks[t].peak.take();
            }
            let out = render(r, 9600);
            (rms(&out), [0, 1, 2].map(|t| r.state.tracks[t].peak.take()))
        };
        let (silent, _) = level(&mut setup(&[]));
        assert_eq!(silent, 0.0, "{bundle:?}: an output no track takes isn't heard");
        let mut r = setup(&[(1, 1 << 1), (2, 1 << 2)]);
        let (heard, peaks) = level(&mut r);
        assert!((heard - 0.5 / 2f32.sqrt()).abs() < 0.02, "{bundle:?}: {heard}");
        assert_eq!(peaks[0], 0.0, "{bundle:?}: the plugin's own track plays its main output only");
        assert!(peaks[1] > 0.4 && peaks[2] == 0.0, "{bundle:?}: Aux 1 on track 1, Aux 2 (silent) on track 2: {peaks:?}");
        // A send from the taker reaches a return.
        let mut sends = [0.0; crate::MAX_RETURNS];
        sends[0] = 1.0;
        r.send(Command::SetSends { track: 1, sends });
        render(&mut r, 2400);
        r.state.returns[0].take();
        render(&mut r, 2400);
        assert!(r.state.returns[0].take() > 0.4, "{bundle:?}: its send");
        // The plugin's track muted: so are its outputs.
        r.send(Command::SetMute { target: Target::Track(0), mute: true });
        render(&mut r, 2400);
        r.state.tracks[1].peak.take();
        render(&mut r, 2400);
        assert_eq!(r.state.tracks[1].peak.take(), 0.0, "{bundle:?}");
    }
}

/// A plugin that sounds late (its outputs too): a track taking one of them is
/// as late as the plugin, so it isn't delayed again; it meets the main
/// output on the same sample.
#[test]
fn an_output_on_its_own_track_is_as_late_as_its_plugin() {
    let (synth, _) = plugins(&test_bundle());
    let sr = 22_050.0;
    let onset = |muted: usize| {
        let mut r = renderer(sr);
        let processor = load(&synth, sr);
        assert_eq!(processor.latency(), 256, "the test synth says it at 22 050 Hz");
        r.send(Command::set_plugin(0, PluginId(1), processor));
        // C4 on the main output, C2 on Aux 1 (track 1 takes it).
        r.send(Command::SetClip { track: 0, slot: 0, clip: Some(chord(&[60, 40])) });
        r.send(Command::LaunchClip { track: 0, slot: 0 });
        let mut from = [0u32; crate::MAX_TRACKS];
        let mut sounds = [0u16; crate::MAX_TRACKS];
        (from[1], sounds[1]) = (1, 1 << 1);
        r.send(Command::SetAudioFrom(from));
        r.send(Command::SetSoundsFrom(sounds));
        // One of them out of the master (its fader down: a mute would silence the other).
        r.send(Command::SetGain { target: Target::Track(muted), gain: 0.0 });
        assert_eq!(r.latency(), 256);
        let out = render(&mut r, 4096);
        out.iter().position(|s| s.abs() > 1e-4).unwrap()
    };
    let (aux, main) = (onset(0), onset(1));
    assert_eq!(aux, main, "both on the same sample");
}

/// A slow plugin (2 ms a block) shows in the load and in its track's and
/// device's times; blocks made in more time than they last are dropouts.
#[test]
fn the_load_and_the_dropouts_of_a_slow_plugin() {
    let (_, gain) = plugins(&test_bundle());
    let sr = 16_000.0;
    let (mut engine, mut handle) = crate::audio::detached(sr);
    let device = Device::new(Effect::Plugin(PluginId(1)));
    assert!(handle.send(Command::InsertDevice { target: Target::Track(0), index: 0, slot: ChainSlot::plugin(&device, load(&gain, sr)) }));
    let cpu = &handle.state.cpu;
    cpu.set_detail(true);
    // Callbacks of 16 ms (four blocks of 64): about half of each is the plugin's.
    let mut out = vec![0f32; 256 * 2];
    // (Over 0.8 s of sound: the mean follows in about 0.3.)
    for _ in 0..50 {
        engine.process(&mut out, 2);
    }
    let load = cpu.take_load();
    assert!(load.mean > 0.35 && load.peak >= 0.45, "{load:?}");
    // (Most are made in time; how many aren't depends on how busy the
    // computer is, running the other tests.)
    let in_time = cpu.late_blocks();
    let (track, device_time) = (cpu.track(0), cpu.device(Target::Track(0), 0));
    assert!(device_time >= 200 * 2_000_000 && track >= device_time, "{track} ≥ {device_time}");
    assert!(cpu.track(1) < device_time / 20, "an empty track costs next to nothing: {}", cpu.track(1));
    // Callbacks of 1 ms: each one late.
    let mut out = vec![0f32; 16 * 2];
    for _ in 0..10 {
        engine.process(&mut out, 2);
    }
    assert!(cpu.take_load().peak > 1.5);
    assert_eq!(cpu.late_blocks() - in_time, 10);
    // Without the detail, nothing is timed.
    cpu.set_detail(false);
    let before = cpu.track(0);
    engine.process(&mut out, 2);
    assert_eq!(cpu.track(0), before);
}

/// The flight recorder holds the tag of the plugin being processed, in its
/// file (what a crash leaves behind), and nothing once it is done.
#[test]
fn the_flight_recorder_says_who_is_processing() {
    let file = std::env::temp_dir().join(format!("pulse-flight-{}", std::process::id()));
    std::fs::write(&file, 77u64.to_le_bytes()).unwrap();
    assert_eq!(crate::plugin::flight::read(&file), 77);
    assert_eq!(crate::plugin::flight::install(&file).unwrap(), 77, "what the last run left");
    assert_eq!(crate::plugin::flight::read(&file), 0, "a fresh start");
    // Only on a thread that says so (the audio thread), not an export's.
    crate::plugin::flight::enter(4);
    assert_eq!(crate::plugin::flight::read(&file), 0);
    crate::plugin::flight::record_this_thread();
    crate::plugin::flight::enter(5);
    assert_eq!(crate::plugin::flight::read(&file), 5, "in the file, not only in memory");
    crate::plugin::flight::leave(5);
    assert_eq!(crate::plugin::flight::read(&file), 0);
    // A plugin with a tag writes it while it runs, and clears it after.
    let (_, gain) = plugins(&test_bundle());
    let mut processor = load(&gain, SR);
    processor.tag = 9;
    let (mut l, mut r) = (vec![0.1f32; 256], vec![0.1f32; 256]);
    processor.process(&mut l, &mut r);
    assert_eq!(crate::plugin::flight::now(), 0);
    let _ = std::fs::remove_file(&file);
}
