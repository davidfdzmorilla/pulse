//! Clips' controller lanes (pitch bend, the wheel, pressure and other CCs)
//! played by the engine, and controllers recorded into takes.

use crate::engine::{build_chain, Command, Engine, LiveNote, PlacedClip, PlayClip, Target};
use crate::model::{AutomationPoint, ControlLane, InstrumentKind, MidiClip, MidiControl, Note, SynthParams};
use crate::Controller;
use std::sync::Arc;

const SR: f32 = 48_000.0;

fn point(beat: f64, value: f32) -> AutomationPoint {
    AutomationPoint { beat, value, curve: 0.0 }
}

fn lane(control: MidiControl, points: &[(f64, f32)]) -> ControlLane {
    ControlLane { control, points: points.iter().map(|&(b, v)| point(b, v)).collect() }
}

/// A clip `length` beats long holding A4 all through, with `lanes`.
fn held_a4(length: f64, lanes: Vec<ControlLane>) -> MidiClip {
    let mut clip = MidiClip::new("bend", length, [0; 3]);
    clip.notes.push(Note::new(0.0, length, 69, 1.0));
    clip.controls = lanes;
    clip
}

fn synth() -> InstrumentKind {
    InstrumentKind::Synth(SynthParams { cutoff: 8000.0, ..Default::default() })
}

/// Renders `beats` (120 BPM) in callbacks of 512 frames; the left channel.
fn run(engine: &mut Engine, beats: f64) -> Vec<f32> {
    let frames = (beats * 0.5 * SR as f64) as usize;
    let mut out = vec![0.0; frames * 2];
    for chunk in out.chunks_mut(1024) {
        engine.process(chunk, 2);
    }
    out.iter().step_by(2).copied().collect()
}

/// The fundamental of `x`, by autocorrelation between 200 and 1000 Hz.
fn pitch_of(x: &[f32]) -> f32 {
    let (shortest, longest) = ((SR / 1000.0) as usize, (SR / 200.0) as usize);
    let n = x.len() - longest - 1;
    let correlation = |lag: usize| -> f32 { (0..n).map(|i| x[i] * x[i + lag]).sum() };
    let values: Vec<f32> = (shortest..=longest).map(correlation).collect();
    let max = values.iter().copied().fold(f32::MIN, f32::max);
    let k = (1..values.len() - 1).find(|&k| values[k] >= 0.9 * max && values[k] >= values[k - 1] && values[k] >= values[k + 1]).unwrap_or(0);
    let best = shortest + k;
    let (a, b, c) = (correlation(best - 1), correlation(best), correlation(best + 1));
    let offset = 0.5 * (a - c) / (a - 2.0 * b + c);
    SR / (best as f32 + offset)
}

/// The pitch around beat `beat` of `audio` (rendered from beat 0).
fn pitch_at(audio: &[f32], beat: f64) -> f32 {
    let at = (beat * 0.5 * SR as f64) as usize;
    pitch_of(&audio[at..at + 2400])
}

const A4: f32 = 440.0;
const B4: f32 = 493.88;
const G4: f32 = 392.0;

/// A launched session clip bends the synth as its lane says, starts each lap
/// where the lane starts, and leaves no bend behind when it stops.
#[test]
fn a_clips_bend_lane_bends_the_synth_loops_and_lets_go_at_stop() {
    let (mut engine, mut handle) = crate::audio::detached(SR);
    assert!(handle.send(Command::set_instrument(0, synth(), SR)));
    assert!(handle.send(Command::SetQuantize(0.0)));
    // Flat for a beat, up a whole tone by beat 2, held; down at beat 3.
    let bend = lane(MidiControl::PitchBend, &[(0.0, 0.0), (1.0, 0.0), (2.0, 1.0), (2.99, 1.0), (3.0, -1.0)]);
    assert!(handle.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(PlayClip::Midi(held_a4(4.0, vec![bend])))) }));
    assert!(handle.send(Command::LaunchClip { track: 0, slot: 0 }));
    let audio = run(&mut engine, 8.0);
    for lap in [0.0, 4.0] {
        let flat = pitch_at(&audio, lap + 0.5);
        assert!((flat - A4).abs() < 3.0, "lap at {lap}: {flat} Hz");
        let rising = pitch_at(&audio, lap + 1.45);
        assert!(rising > A4 + 15.0 && rising < B4 - 15.0, "halfway up the ramp: {rising}");
        let up = pitch_at(&audio, lap + 2.4);
        assert!((up - B4).abs() < 3.0, "lap at {lap}: {up} Hz");
        let down = pitch_at(&audio, lap + 3.4);
        assert!((down - G4).abs() < 3.0, "lap at {lap}: {down} Hz");
    }
    // Stopped while bent down: a note played now is in tune.
    assert!(handle.send(Command::Stop));
    assert!(handle.send(Command::SetInputTrack(Some(0))));
    run(&mut engine, 0.25);
    handle.live.send(LiveNote::note(69, 1.0));
    let after = run(&mut engine, 0.5);
    let pitch = pitch_of(&after[4800..]);
    assert!((pitch - A4).abs() < 3.0, "no bend left stuck: {pitch} Hz");
}

/// On the Arrangement a clip's lanes are in its own time, whatever beat it
/// starts at; past its end the bend goes back to rest.
#[test]
fn arrangement_clips_play_their_lanes_in_clip_time_and_rest_after() {
    let (mut engine, mut handle) = crate::audio::detached(SR);
    assert!(handle.send(Command::set_instrument(0, synth(), SR)));
    let bend = lane(MidiControl::PitchBend, &[(0.0, 1.0), (1.0, 1.0), (1.01, -1.0)]);
    let clip = held_a4(2.0, vec![bend]);
    // At beat 2, two beats long; a plain note after it.
    let mut after = MidiClip::new("plain", 2.0, [0; 3]);
    after.notes.push(Note::new(0.0, 2.0, 69, 1.0));
    let placed = vec![PlacedClip::new(2.0, 2.0, PlayClip::Midi(clip)), PlacedClip::new(4.0, 2.0, PlayClip::Midi(after))];
    assert!(handle.send(Command::SetArrangement { track: 0, clips: Box::new(placed) }));
    assert!(handle.send(Command::Play));
    let audio = run(&mut engine, 6.0);
    let up = pitch_at(&audio, 2.5);
    assert!((up - B4).abs() < 3.0, "{up}");
    let down = pitch_at(&audio, 3.5);
    assert!((down - G4).abs() < 3.0, "{down}");
    let plain = pitch_at(&audio, 4.5);
    assert!((plain - A4).abs() < 3.0, "the next clip has no bend: {plain}");
}

/// Brightness (CC 74) opens a subtractive synth's filter; at rest the sound
/// is what it was without it.
#[test]
fn brightness_opens_the_filter_and_rests_where_it_does_nothing() {
    let render = |lanes: Vec<ControlLane>| {
        let (mut engine, mut handle) = crate::audio::detached(SR);
        let kind = InstrumentKind::Synth(SynthParams { cutoff: 600.0, ..Default::default() });
        assert!(handle.send(Command::set_instrument(0, kind, SR)));
        assert!(handle.send(Command::SetArrangement { track: 0, clips: Box::new(vec![PlacedClip::new(0.0, 2.0, PlayClip::Midi(held_a4(2.0, lanes)))]) }));
        assert!(handle.send(Command::Play));
        run(&mut engine, 2.0)
    };
    // How much of it is high: the energy of its slope against its own.
    let brightness = |x: &[f32]| {
        let slope: f32 = x.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
        slope / x.iter().map(|v| v * v).sum::<f32>()
    };
    let plain = render(Vec::new());
    let rest = render(vec![lane(MidiControl::Cc(74), &[(0.0, 64.0 / 127.0)])]);
    let open = render(vec![lane(MidiControl::Cc(74), &[(0.0, 1.0)])]);
    let at_rest = render(vec![lane(MidiControl::Cc(74), &[(0.0, 0.5)])]);
    assert_eq!(plain, at_rest, "at rest, CC 74 changes nothing");
    let (p, r, o) = (brightness(&plain[4800..]), brightness(&rest[4800..]), brightness(&open[4800..]));
    assert!(o > 2.0 * p, "open: {o} vs {p}");
    assert!((r / p - 1.0).abs() < 0.1, "nearly at rest: {r} vs {p}");
}

/// Expression (CC 11) sets the level of a built-in instrument, gliding.
#[test]
fn expression_sets_the_level() {
    let render = |lanes: Vec<ControlLane>| {
        let (mut engine, mut handle) = crate::audio::detached(SR);
        assert!(handle.send(Command::set_instrument(0, synth(), SR)));
        assert!(handle.send(Command::SetArrangement { track: 0, clips: Box::new(vec![PlacedClip::new(0.0, 2.0, PlayClip::Midi(held_a4(2.0, lanes)))]) }));
        assert!(handle.send(Command::Play));
        run(&mut engine, 2.0)
    };
    let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
    let full = render(Vec::new());
    let half = render(vec![lane(MidiControl::Cc(11), &[(0.0, 1.0), (1.0, 1.0), (1.001, std::f32::consts::FRAC_1_SQRT_2)])]);
    assert_eq!(full[..20_000], half[..20_000], "at full expression, the same");
    let ratio = rms(&half[30_000..45_000]) / rms(&full[30_000..45_000]);
    assert!((ratio - 0.5).abs() < 0.02, "{ratio}");
}

/// The lanes of every kind, on several built-in instruments, sound the same in
/// callbacks of any size and shared among threads.
#[test]
fn lanes_sound_the_same_with_any_buffer_and_threads() {
    use crate::model::{FmParams, KeysModel, KeysParams, SamplerParams, SuperSawParams, WaveParams};
    let kinds = [
        synth(),
        InstrumentKind::SuperSaw(SuperSawParams::default()),
        InstrumentKind::Wavetable(WaveParams::default()),
        InstrumentKind::Keys(KeysParams::default()),
        InstrumentKind::Keys(KeysParams { model: KeysModel::Tines, ..KeysParams::default() }),
        InstrumentKind::Keys(KeysParams { model: KeysModel::Organ, ..KeysParams::default() }),
        InstrumentKind::Fm(FmParams::default()),
        InstrumentKind::Sampler(SamplerParams { looping: true, ..Default::default() }),
    ];
    let lanes = || {
        vec![
            lane(MidiControl::PitchBend, &[(0.0, 0.0), (1.0, 0.7), (1.5, -0.4), (3.0, 0.0)]),
            ControlLane { control: MidiControl::MOD_WHEEL, points: vec![AutomationPoint { beat: 0.5, value: 0.0, curve: 0.6 }, point(2.5, 1.0)] },
            lane(MidiControl::Cc(74), &[(0.0, 0.2), (2.0, 0.9)]),
            lane(MidiControl::Pressure, &[(1.0, 0.0), (2.0, 0.8), (3.5, 0.1)]),
            lane(MidiControl::Cc(11), &[(0.0, 1.0), (3.0, 0.4)]),
        ]
    };
    let render = |threads: usize, sizes: &[usize]| {
        let (mut engine, mut handle) = crate::audio::detached(SR);
        engine.set_threads(threads);
        engine.share_always();
        assert!(handle.send(Command::SetTrackIds(std::array::from_fn(|t| t as u32 + 1))));
        for (t, kind) in kinds.iter().enumerate() {
            assert!(handle.send(Command::set_instrument(t, *kind, SR)));
            let table = Arc::new(crate::wavetable::builtin(0));
            assert!(handle.send(Command::SetWavetables { track: t, tables: Box::new(vec![table.clone(), table]) }));
            assert!(handle.send(Command::SetSamples { track: t, samples: Box::new(vec![saw_sample()]) }));
            let mut clip = held_a4(4.0, lanes());
            clip.notes.push(Note::new(1.0, 1.5, 64, 0.8));
            // Looping twice inside its place, starting off the beat.
            assert!(handle.send(Command::SetArrangement { track: t, clips: Box::new(vec![PlacedClip::new(0.37, 8.0, PlayClip::Midi(clip))]) }));
            assert!(handle.send(Command::ReplaceChain { target: Target::Track(t), chain: build_chain(&[], SR) }));
        }
        assert!(handle.send(Command::SetLoop { enabled: true, start: 1.0, end: 7.3 }));
        assert!(handle.send(Command::SetOutputLimit(false)));
        assert!(handle.send(Command::Play));
        let total = 5 * 48_000;
        let mut out = Vec::with_capacity(2 * total);
        let mut k = 0;
        while out.len() < 2 * total {
            let n = sizes[k % sizes.len()];
            k += 1;
            let mut block = vec![0f32; 2 * n];
            engine.process(&mut block, 2);
            handle.collect_garbage();
            out.extend(block);
        }
        out.truncate(2 * total);
        (out, engine.helped())
    };
    let one = render(1, &[1024]).0;
    assert!(one.iter().map(|v| v * v).sum::<f32>() > 1.0);
    for (threads, sizes) in [(1, &[1usize, 63, 64, 500, 1024, 4096, 37][..]), (4, &[37, 1024, 63, 500][..])] {
        // A busy machine may keep the workers asleep through a whole render:
        // it is made again until they take part.
        let mut tries = 0;
        let (other, helped) = loop {
            let made = render(threads, sizes);
            tries += 1;
            if threads == 1 || made.1 > 0 || tries == 20 {
                break made;
            }
        };
        assert!(threads == 1 || helped > 0, "the other threads did nothing");
        let first = one.iter().zip(&other).position(|(a, b)| a.to_bits() != b.to_bits());
        assert_eq!(first, None, "{threads} threads: differ from frame {:?}", first.map(|i| i / 2));
    }
}

/// Controllers moved on an armed track while the Arrangement records reach
/// the UI with the beat they came at; the sustain pedal and poly pressure don't.
#[test]
fn controllers_played_while_recording_are_captured() {
    let (mut engine, mut handle) = crate::audio::detached(SR);
    assert!(handle.send(Command::set_instrument(0, synth(), SR)));
    assert!(handle.send(Command::SetArm { track: 0, armed: true }));
    run(&mut engine, 0.1);
    handle.live.send(LiveNote::control(Controller::PitchBend, 0, 0.5));
    run(&mut engine, 0.5);
    assert!(handle.take_recorded_controls().is_empty(), "not recording: nothing");
    assert!(handle.send(Command::SetArrangementRecord(true)));
    run(&mut engine, 1.0);
    handle.live.send(LiveNote::control(Controller::PitchBend, 0, -0.25));
    handle.live.send(LiveNote::control(Controller::Cc(64), 0, 1.0));
    handle.live.send(LiveNote::control(Controller::PolyPressure, 60, 1.0));
    run(&mut engine, 0.5);
    handle.live.send(LiveNote::control(Controller::Cc(1), 0, 0.75));
    handle.live.send(LiveNote::control(Controller::Pressure, 0, 0.3));
    run(&mut engine, 0.5);
    let got = handle.take_recorded_controls();
    let what: Vec<(MidiControl, f32)> = got.iter().map(|c| (c.control, c.value)).collect();
    assert_eq!(what, vec![(MidiControl::PitchBend, -0.25), (MidiControl::MOD_WHEEL, 0.75), (MidiControl::Pressure, 0.3)]);
    assert!((got[0].beat - 1.0).abs() < 0.05, "{}", got[0].beat);
    assert!((got[1].beat - 1.5).abs() < 0.05, "{}", got[1].beat);
    assert!(got.iter().all(|c| c.track == 0));
}

/// A bright test sample: a 220 Hz saw, a second long.
fn saw_sample() -> Arc<crate::sample::Sample> {
    let left: Vec<f32> = (0..48_000).map(|i| ((i as f32 * 220.0 / SR).fract() * 2.0 - 1.0) * 0.5).collect();
    Arc::new(crate::sample::Sample::from_channels(left.clone(), left, SR))
}

/// Renders A4 held for two beats on `kind`, with `lanes`; the left channel.
fn render_held(kind: InstrumentKind, lanes: Vec<ControlLane>) -> Vec<f32> {
    let (mut engine, mut handle) = crate::audio::detached(SR);
    assert!(handle.send(Command::set_instrument(0, kind, SR)));
    assert!(handle.send(Command::SetSamples { track: 0, samples: Box::new(vec![saw_sample()]) }));
    assert!(handle.send(Command::SetArrangement { track: 0, clips: Box::new(vec![PlacedClip::new(0.0, 2.0, PlayClip::Midi(held_a4(2.0, lanes)))]) }));
    assert!(handle.send(Command::Play));
    run(&mut engine, 2.0)
}

/// How much of `x` is high: the energy of its slope against its own.
fn slope_ratio(x: &[f32]) -> f32 {
    let slope: f32 = x.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
    slope / x.iter().map(|v| v * v).sum::<f32>().max(1e-12)
}

/// Brightness at rest and no pressure leave every factory preset of FM, Keys
/// and the sampler as it was: the same samples.
#[test]
fn at_rest_the_controllers_leave_fm_keys_and_the_sampler_alone() {
    use crate::model::{FmParams, KeysParams, SamplerParams};
    let mut kinds: Vec<InstrumentKind> = FmParams::presets().into_iter().map(|(_, p)| InstrumentKind::Fm(p)).collect();
    kinds.extend(KeysParams::presets().into_iter().map(|(_, p)| InstrumentKind::Keys(p)));
    kinds.push(InstrumentKind::Sampler(SamplerParams::default()));
    kinds.push(InstrumentKind::Sampler(SamplerParams { cutoff: 2000.0, resonance: 0.4, ..Default::default() }));
    for kind in kinds {
        let plain = render_held(kind, Vec::new());
        assert!(plain.iter().map(|v| v * v).sum::<f32>() > 0.1, "{kind:?} plays");
        let rest = vec![lane(MidiControl::Cc(74), &[(0.0, 0.5)]), lane(MidiControl::Pressure, &[(0.0, 0.0)])];
        assert_eq!(plain, render_held(kind, rest), "{kind:?}");
        // Moved away for a beat: the controllers do something.
        let moved = vec![lane(MidiControl::Cc(74), &[(0.0, 0.0), (1.0, 0.0), (1.001, 0.5)]), lane(MidiControl::Pressure, &[(0.0, 1.0), (1.0, 1.0), (1.001, 0.0)])];
        assert_ne!(plain[..24_000], render_held(kind, moved)[..24_000], "{kind:?}: the controllers do something");
    }
}

/// CC 74 and pressure raise the FM synth's modulation index: brighter, no louder.
#[test]
fn brightness_and_pressure_brighten_the_fm_synth() {
    use crate::model::FmParams;
    let kind = InstrumentKind::Fm(FmParams { mod_sustain: 0.3, sustain: 0.8, ..Default::default() });
    let plain = render_held(kind, Vec::new());
    let dark = render_held(kind, vec![lane(MidiControl::Cc(74), &[(0.0, 0.0)])]);
    let bright = render_held(kind, vec![lane(MidiControl::Cc(74), &[(0.0, 1.0)])]);
    let pressed = render_held(kind, vec![lane(MidiControl::Pressure, &[(0.0, 1.0)])]);
    let tail = 24_000..44_000;
    let (p, d, b, a) = (slope_ratio(&plain[tail.clone()]), slope_ratio(&dark[tail.clone()]), slope_ratio(&bright[tail.clone()]), slope_ratio(&pressed[tail.clone()]));
    assert!(b > 1.5 * p, "CC 74 up: {b} vs {p}");
    assert!(d < 0.8 * p, "CC 74 down: {d} vs {p}");
    assert!(a > 1.5 * p, "pressure: {a} vs {p}");
    let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
    let ratio = rms(&bright[tail.clone()]) / rms(&plain[tail]);
    assert!(ratio > 0.7 && ratio < 1.1, "no louder: {ratio}");
}

/// On Pulse Keys, CC 74 tilts the tone and pressure brings a tremolo (the
/// organ, drive).
#[test]
fn brightness_tilts_the_keys_and_pressure_trembles() {
    use crate::model::{KeysModel, KeysParams};
    let tines = InstrumentKind::Keys(KeysParams { model: KeysModel::Tines, tremolo: 0.0, ..KeysParams::default() });
    let plain = render_held(tines, Vec::new());
    let bright = render_held(tines, vec![lane(MidiControl::Cc(74), &[(0.0, 1.0)])]);
    let dark = render_held(tines, vec![lane(MidiControl::Cc(74), &[(0.0, 0.0)])]);
    let part = 4_800..24_000;
    let (p, b, d) = (slope_ratio(&plain[part.clone()]), slope_ratio(&bright[part.clone()]), slope_ratio(&dark[part.clone()]));
    assert!(b > 1.3 * p && d < 0.9 * p, "tone: {d} < {p} < {b}");
    // Pressure: the level swings at about 5.5 Hz, seen in 10 ms windows.
    let pressed = render_held(tines, vec![lane(MidiControl::Pressure, &[(0.0, 1.0)])]);
    let swing = |x: &[f32]| {
        let levels: Vec<f32> = x[9_600..33_600].chunks(480).map(|c| c.iter().map(|v| v * v).sum::<f32>().sqrt()).collect();
        let ratios: Vec<f32> = levels.windows(2).map(|w| (w[1] / w[0].max(1e-9)).ln().abs()).collect();
        ratios.iter().sum::<f32>() / ratios.len() as f32
    };
    assert!(swing(&pressed) > 3.0 * swing(&plain), "tremolo: {} vs {}", swing(&pressed), swing(&plain));
    // The organ drives instead: more overtones.
    let organ = InstrumentKind::Keys(KeysParams { model: KeysModel::Organ, ..KeysParams::default() });
    let (calm, pushed) = (render_held(organ, Vec::new()), render_held(organ, vec![lane(MidiControl::Pressure, &[(0.0, 1.0)])]));
    let (c, d) = (slope_ratio(&calm[part.clone()]), slope_ratio(&pushed[part]));
    assert!(d > 1.1 * c, "organ drive: {d} vs {c}");
}

/// The sampler's filter follows CC 74 and pressure: closing it darkens even a
/// sample played with the filter open.
#[test]
fn brightness_moves_the_samplers_filter() {
    use crate::model::SamplerParams;
    let open = InstrumentKind::Sampler(SamplerParams::default());
    let plain = render_held(open, Vec::new());
    let dark = render_held(open, vec![lane(MidiControl::Cc(74), &[(0.0, 0.0)])]);
    let part = 4_800..24_000;
    let (p, d) = (slope_ratio(&plain[part.clone()]), slope_ratio(&dark[part.clone()]));
    assert!(d < 0.3 * p, "closed: {d} vs {p}");
    let closed = InstrumentKind::Sampler(SamplerParams { cutoff: 1000.0, ..Default::default() });
    let shut = render_held(closed, Vec::new());
    let opened = render_held(closed, vec![lane(MidiControl::Pressure, &[(0.0, 1.0)])]);
    let (s, o) = (slope_ratio(&shut[part.clone()]), slope_ratio(&opened[part]));
    assert!(o > 2.0 * s, "opened: {o} vs {s}");
}

/// With `PULSE_DEMOS=dir`, WAVs of FM and Keys under the controllers: a chord
/// held twelve beats (120 BPM), first plain, then CC 74 up and down, then
/// pressure up and away.
#[test]
fn expression_demos() {
    use crate::model::{FmParams, KeysModel, KeysParams};
    let Ok(dir) = std::env::var("PULSE_DEMOS") else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let fm = FmParams::presets().into_iter().find(|(n, _)| *n == "Lead · Flauta FM").map_or(FmParams::default(), |(_, p)| p);
    let kinds = [
        ("fm-flauta", InstrumentKind::Fm(fm)),
        ("fm-defecto", InstrumentKind::Fm(FmParams { sustain: 0.8, mod_sustain: 0.3, ..FmParams::default() })),
        ("keys-tines", InstrumentKind::Keys(KeysParams { model: KeysModel::Tines, ..KeysParams::default() })),
        ("keys-piano", InstrumentKind::Keys(KeysParams::default())),
        ("keys-organo", InstrumentKind::Keys(KeysParams { model: KeysModel::Organ, ..KeysParams::default() })),
    ];
    for (name, kind) in kinds {
        let (mut engine, mut handle) = crate::audio::detached(SR);
        assert!(handle.send(Command::set_instrument(0, kind, SR)));
        let mut clip = MidiClip::new("demo", 12.0, [0; 3]);
        for pitch in [57, 60, 64] {
            clip.notes.push(Note::new(0.0, 11.5, pitch, 0.8));
        }
        clip.controls = vec![
            lane(MidiControl::Cc(74), &[(0.0, 0.5), (2.0, 0.5), (4.0, 1.0), (6.0, 0.0), (7.0, 0.5)]),
            lane(MidiControl::Pressure, &[(0.0, 0.0), (8.0, 0.0), (9.5, 1.0), (11.0, 1.0), (11.5, 0.0)]),
        ];
        assert!(handle.send(Command::SetArrangement { track: 0, clips: Box::new(vec![PlacedClip::new(0.0, 12.0, PlayClip::Midi(clip))]) }));
        assert!(handle.send(Command::Play));
        let frames = (12.5 * 0.5 * SR) as usize;
        let mut out = vec![0.0; frames * 2];
        for chunk in out.chunks_mut(1024) {
            engine.process(chunk, 2);
        }
        let l: Vec<f32> = out.iter().step_by(2).copied().collect();
        let r: Vec<f32> = out.iter().skip(1).step_by(2).copied().collect();
        crate::keys::tests::write_wav(&format!("{dir}/{name}.wav"), &l, &r);
    }
}

/// A short sine `hz` Hz, `seconds` long, as a pad's or a sampler's sample.
fn tone(hz: f32, seconds: f32) -> Arc<crate::sample::Sample> {
    let data: Vec<f32> = (0..(seconds * SR) as usize).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / SR).sin()).collect();
    Arc::new(crate::sample::Sample::from_channels(data.clone(), data, SR))
}

/// Rising zero crossings of `x`, as Hz.
fn crossings_hz(x: &[f32]) -> f32 {
    let c: Vec<f32> = (1..x.len()).filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0).map(|i| (i - 1) as f32 + x[i - 1] / (x[i - 1] - x[i])).collect();
    SR * (c.len() - 1) as f32 / (c[c.len() - 1] - c[0])
}

/// A hat roll on the Drum Pads under a clip's bend lane ramping up an
/// octave: each hit a step higher, as the lane is where it starts; the kick
/// in the middle of it, a pad without a bend range, stays in tune.
#[test]
fn a_bend_ramp_tunes_a_pad_roll_hit_by_hit() {
    use crate::model::{RackParams, RACK_PADS};
    let (mut engine, mut handle) = crate::audio::detached(SR);
    let mut rack = RackParams::default();
    rack.bend[6] = 12.0;
    assert!(handle.send(Command::set_instrument(0, InstrumentKind::DrumRack(rack), SR)));
    let mut samples = vec![Arc::new(crate::sample::Sample::default()); RACK_PADS];
    samples[0] = tone(200.0, 0.04);
    samples[6] = tone(1000.0, 0.04);
    assert!(handle.send(Command::SetSamples { track: 0, samples: Box::new(samples) }));
    let mut clip = MidiClip::new("roll", 4.0, [0; 3]);
    let hats = [1.0, 1.125, 1.25, 1.375, 1.625, 1.75, 1.875];
    for &beat in &hats {
        clip.notes.push(Note::new(beat, 0.125, 42, 1.0));
    }
    clip.notes.push(Note::new(1.5, 0.125, 36, 1.0));
    // Up a full bend (an octave on the hat) by the roll's last hit, then back.
    clip.controls = vec![lane(MidiControl::PitchBend, &[(0.0, 0.0), (1.0, 0.0), (1.875, 1.0), (2.0, 0.0)])];
    assert!(handle.send(Command::SetArrangement { track: 0, clips: Box::new(vec![PlacedClip::new(0.0, 4.0, PlayClip::Midi(clip))]) }));
    assert!(handle.send(Command::Play));
    let audio = run(&mut engine, 3.0);
    let hit = |beat: f64| {
        let at = (beat * 0.5 * SR as f64) as usize + 48;
        crossings_hz(&audio[at..at + (0.035 * SR) as usize])
    };
    let mut line = String::new();
    for &beat in &hats {
        let expected = 1000.0 * 2f32.powf((beat as f32 - 1.0) / 0.875);
        let heard = hit(beat);
        line += &format!(" {heard:.0}");
        let cents = 1200.0 * (heard / expected).log2();
        assert!(cents.abs() < 10.0, "hat at beat {beat}: {heard} Hz, {expected} expected");
    }
    println!("hat roll (Hz):{line}");
    let kick = hit(1.5);
    assert!((kick - 200.0).abs() < 1.0, "the kick ignores the bend: {kick}");
}

/// The Sampler in Legato through the engine with clip notes (timed gates):
/// the tied note slides, the one after a rest starts on its pitch.
#[test]
fn the_sampler_slides_tied_clip_notes_only() {
    use crate::model::SamplerParams;
    let (mut engine, mut handle) = crate::audio::detached(SR);
    let params = SamplerParams { root: 57.0, gain: 1.0, mono: true, legato: true, glide: 80.0, release: 0.02, ..Default::default() };
    assert!(handle.send(Command::set_instrument(0, InstrumentKind::Sampler(params), SR)));
    assert!(handle.send(Command::SetSamples { track: 0, samples: Box::new(vec![tone(220.0, 6.0)]) }));
    let mut clip = MidiClip::new("808", 8.0, [0; 3]);
    // 45 for two beats, 57 tied over its end, a rest, then 50.
    clip.notes.push(Note::new(0.0, 2.0, 45, 1.0));
    clip.notes.push(Note::new(1.75, 1.0, 57, 1.0));
    clip.notes.push(Note::new(4.0, 1.0, 50, 1.0));
    assert!(handle.send(Command::SetArrangement { track: 0, clips: Box::new(vec![PlacedClip::new(0.0, 8.0, PlayClip::Midi(clip))]) }));
    assert!(handle.send(Command::Play));
    let audio = run(&mut engine, 6.0);
    let note_at = |beat: f64, ms: f32| {
        let at = (beat * 0.5 * SR as f64) as usize + (ms * 0.001 * SR) as usize;
        45.0 + 12.0 * (crossings_hz(&audio[at..at + 960]) / 110.0).log2()
    };
    let (start, tied, arrived, after_rest) = (note_at(0.0, 15.0), note_at(1.75, 15.0), note_at(1.75, 300.0), note_at(4.0, 15.0));
    println!("sampler legato: 45 at 15 ms {start:.2}; 57 tied, at 15 ms {tied:.2}, at 300 ms {arrived:.2}; 50 after a rest at 15 ms {after_rest:.2}");
    assert!((start - 45.0).abs() < 0.05);
    assert!(tied > 45.5 && tied < 55.0, "{tied}");
    assert!((arrived - 57.0).abs() < 0.05, "{arrived}");
    assert!((after_rest - 50.0).abs() < 0.05, "{after_rest}");
}
