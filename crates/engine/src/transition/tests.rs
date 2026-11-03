use super::*;
use crate::effects::EffectProcessor;
use crate::model::Effect;
use std::f32::consts::TAU;

const SR: f32 = 48_000.0;
const TEMPO: f64 = 120.0;
/// Frames a beat at 120 BPM.
const BEAT: usize = 24_000;

fn ctx(frame: usize) -> ProcessContext {
    let bps = TEMPO / 60.0 / SR as f64;
    ProcessContext { sample_rate: SR, tempo: TEMPO, beat: frame as f64 * bps, beats_per_sample: bps, scale: 0, meter: Default::default() }
}

fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| (TAU * hz * i as f32 / SR).sin() * amp).collect()
}

/// Deterministic noise, −0.5..0.5.
fn noise(n: usize) -> Vec<f32> {
    let mut x = 0x1234_5678u32;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as f32 / u32::MAX as f32 - 0.5
        })
        .collect()
}

/// Runs `params` over `input` (both channels) as the engine does: blocks cut
/// at every 64 frames, «Active» set at each from `on(frame)` (a lane), the
/// transport playing from frame 0. Returns the left channel.
fn run(params: TransitionParams, input: &[f32], on: &dyn Fn(usize) -> bool, block: usize) -> Vec<f32> {
    let mut fx = EffectProcessor::new(&Effect::Transition(params), SR);
    let (mut l, mut r) = (input.to_vec(), input.to_vec());
    let mut at = 0;
    while at < input.len() {
        if at % 64 == 0 {
            fx.set_param(0, if on(at) { 1.0 } else { 0.0 });
        }
        let n = block.min(64 - at % 64).min(input.len() - at);
        fx.process(&mut l[at..at + n], &mut r[at..at + n], &ctx(at));
        at += n;
    }
    assert!(l == r, "both channels alike");
    l
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

/// Frequency from the rising zero crossings in `x`.
fn frequency(x: &[f32]) -> f32 {
    let crossings: Vec<f32> = (1..x.len()).filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0).map(|i| (i - 1) as f32 + x[i - 1] / (x[i - 1] - x[i])).collect();
    if crossings.len() < 2 {
        return 0.0;
    }
    (crossings.len() - 1) as f32 * SR / (crossings[crossings.len() - 1] - crossings[0])
}

/// The biggest step from one sample to the next.
fn biggest_step(x: &[f32]) -> f32 {
    x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

const L: usize = LATENCY;

fn tape(mode: TransitionMode, length_beats: f64, curve: f32) -> TransitionParams {
    TransitionParams { mode, length_beats, curve, ..Default::default() }
}

/// A tape stop over a bar: speed and pitch fall together (evenly, with no
/// curve; early, with a curve up), the level holds until nearly stopped,
/// then silence; and switched off, the sound is back on the beat.
#[test]
fn a_tape_stop_falls_in_pitch_and_level_together() {
    let n = 8 * BEAT;
    let input = sine(1000.0, 0.5, n);
    let (start, stop) = (2 * BEAT, 7 * BEAT);
    let out = run(tape(TransitionMode::TapeStop, 4.0, 0.0), &input, &|f| (start..stop).contains(&f), 64);
    let d = 4 * BEAT;
    let window = |u: f64| {
        let at = start + (u * d as f64) as usize + L;
        &out[at - 1024..at + 1024]
    };
    for u in [0.1, 0.25, 0.5, 0.75] {
        let hz = frequency(window(u));
        let expected = 1000.0 * (1.0 - u as f32);
        println!("parada de cinta al {:>3.0} %: {hz:7.1} Hz (esperado {expected:.0}), {:6.2} dB", u * 100.0, db(rms(window(u)) / rms(&input[..4800])));
        assert!((hz / expected - 1.0).abs() < 0.02, "at {u}: {hz} Hz, not {expected}");
        assert!(db(rms(window(u)) / rms(&input[..4800])).abs() < 0.5, "at {u} the level holds");
    }
    // Stopped: silence until switched off; then the sound, on the beat.
    assert!(rms(&out[start + d + L + 100..stop + L - 100]) < 1e-6);
    assert_eq!(&out[stop + L..], &input[stop..n - L], "back on the beat");
    assert_eq!(&out[L..start + L], &input[..start], "the same before");
    // A curve up brakes early: lower a quarter of the way in.
    let early = run(tape(TransitionMode::TapeStop, 4.0, 0.6), &input, &|f| (start..stop).contains(&f), 64);
    let at = start + d / 4 + L;
    let hz = frequency(&early[at - 1024..at + 1024]);
    let expected = 1000.0 * 0.75f32.powf(3f32.powf(0.6));
    assert!((hz / expected - 1.0).abs() < 0.03, "curved: {hz} Hz, not {expected}");
}

/// A tape start comes up to speed from nothing and then crosses to the live
/// sound: half speed halfway, the input itself after.
#[test]
fn a_tape_start_rises_then_plays_live() {
    let n = 6 * BEAT;
    let input = sine(1000.0, 0.5, n);
    let start = BEAT;
    let out = run(tape(TransitionMode::TapeStart, 2.0, 0.0), &input, &|f| f >= start, 64);
    let d = 2 * BEAT;
    let at = start + d / 2 + L;
    let hz = frequency(&out[at - 1024..at + 1024]);
    assert!((hz / 500.0 - 1.0).abs() < 0.02, "halfway: {hz} Hz");
    let first = start + L;
    assert!(rms(&out[first + 100..first + 400]) < 0.01, "it starts from stopped");
    let live = start + d + (CATCH_UP_SECONDS * SR) as usize + L + 1;
    assert_eq!(&out[live..], &input[live - L..n - L], "live after");
}

/// A repeat plays the piece of the grid the switch falls in, from its line:
/// each grid step after the first is that piece again, exactly.
#[test]
fn repeats_fall_on_the_grid() {
    let n = 5 * BEAT;
    let input = noise(n);
    // On 192 frames after beat 2 (not on a line): the piece starts on beat 2.
    let on = 2 * BEAT + 192;
    let off = 4 * BEAT;
    let grid = BEAT / 4;
    let params = TransitionParams { mode: TransitionMode::Repeat, grid_beats: 0.25, ..Default::default() };
    let out = run(params, &input, &|f| (on..off).contains(&f), 64);
    let fade = (REPEAT_FADE_SECONDS * SR) as usize;
    let s = 2 * BEAT;
    assert_eq!(&out[L..s + grid + L], &input[..s + grid], "the sound itself until the first step ends");
    for k in 1..7 {
        let from = s + k * grid + fade;
        let to = s + (k + 1) * grid;
        assert_eq!(&out[from + L..to + L], &input[from - k * grid..to - k * grid], "repeat {k}");
    }
    assert_eq!(&out[off + L..], &input[off..n - L], "off on its beat");
}

/// Each repeat lower and quieter than the last when asked, and gated ones
/// fall silent before the next.
#[test]
fn repeats_fall_in_pitch_and_level() {
    let n = 4 * BEAT;
    let input = sine(800.0, 0.5, n);
    let params = TransitionParams { mode: TransitionMode::Repeat, grid_beats: 0.5, pitch: -2.0, decay_db: 3.0, ..Default::default() };
    let out = run(params, &input, &|f| f >= BEAT, 64);
    let step = BEAT / 2;
    for k in 1..4usize {
        let at = BEAT + k * step + L;
        let piece = &out[at + step / 4..at + 3 * step / 4];
        let hz = frequency(piece);
        let expected = 800.0 * 2f32.powf(-2.0 * k as f32 / 12.0);
        let level = db(rms(piece) / rms(&input[..step]));
        println!("repetición {k}: {hz:6.1} Hz (esperado {expected:.1}), {level:6.2} dB");
        assert!((hz / expected - 1.0).abs() < 0.01, "repeat {k}: {hz}");
        assert!((level + 3.0 * k as f32).abs() < 0.3, "repeat {k}: {level} dB");
    }
    let gated = TransitionParams { mode: TransitionMode::Repeat, grid_beats: 0.5, gate: 0.5, ..Default::default() };
    let out = run(gated, &input, &|f| f >= BEAT, 64);
    let at = BEAT + 2 * step + L;
    assert!(rms(&out[at + step / 2 + 10..at + step - 10]) < 1e-6, "the gated half is silent");
    assert!(rms(&out[at + 200..at + step / 2 - 200]) > 0.3);
}

/// A switch drawn on a line is seen at the next sub-block (here 36 and 44
/// frames late): the cut still starts and ends on the line, and the sound
/// after it comes back whole.
#[test]
fn a_cut_starts_and_ends_on_the_line() {
    let n = 8_000;
    let input = noise(n);
    // 1/64-note lines every 1500 frames at 120 BPM.
    let out = run(TransitionParams { mode: TransitionMode::Cut, ..Default::default() }, &input, &|f| (1536..4544).contains(&f), 64);
    let fade = (EDGE_SECONDS * SR) as usize;
    assert_eq!(&out[L..1500 + L], &input[..1500], "whole until the line");
    assert!(rms(&out[1500 + fade + L..4500 - fade + L]) == 0.0, "silent between");
    assert_eq!(&out[4500 + L..], &input[4500..n - L], "whole from the line");
    // Taking out the lows only.
    let lows = sine(60.0, 0.5, n);
    let highs = sine(5000.0, 0.2, n);
    let mixed: Vec<f32> = lows.iter().zip(&highs).map(|(a, b)| a + b).collect();
    let cut = TransitionParams { mode: TransitionMode::Cut, filter: Some(FilterMode::HighPass), filter_hz: 300.0, ..Default::default() };
    let out = run(cut, &mixed, &|f| f >= 1536, 64);
    let tail = &out[5000..];
    let residue: Vec<f32> = tail.iter().zip(&highs[5000 - L..n - L]).map(|(a, b)| a - b).collect();
    assert!(db(rms(&residue) / rms(&lows)) < -25.0, "the lows are gone: {}", db(rms(&residue) / rms(&lows)));
}

/// Switching on and off, between repeats, and as a tape stops or starts:
/// never a click (no step much bigger than the sine's own).
#[test]
fn no_clicks_on_the_way_in_or_out() {
    let n = 7 * BEAT;
    let input = sine(220.0, 0.5, n);
    let own = biggest_step(&input);
    let on = |f: usize| (BEAT + 3000..5 * BEAT + 777).contains(&f);
    let mut cases: Vec<(String, TransitionParams)> = TransitionParams::presets().into_iter().map(|(n, p)| (n.to_string(), p)).collect();
    cases.push(("pitched gated".into(), TransitionParams { mode: TransitionMode::Repeat, grid_beats: 1.0 / 6.0, pitch: -5.0, gate: 0.3, decay_db: 1.0, ..Default::default() }));
    cases.push(("down a lot".into(), TransitionParams { mode: TransitionMode::Repeat, grid_beats: 1.0 / 3.0, pitch: -12.0, ..Default::default() }));
    for (name, params) in cases {
        let out = run(params, &input, &on, 64);
        let step = biggest_step(&out);
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(step < 3.0 * own, "{name}: a step of {step} ({own} the sine's)");
    }
}

/// The same samples whatever blocks the audio comes in (the lane every 64
/// frames, as the engine applies it).
#[test]
fn the_same_in_any_blocks() {
    let n = 6 * BEAT;
    let input: Vec<f32> = noise(n).iter().zip(sine(150.0, 0.4, n)).map(|(a, b)| a * 0.5 + b).collect();
    let on = |f: usize| (BEAT / 3..BEAT * 2).contains(&f) || (BEAT * 2 + 7000..BEAT * 5).contains(&f);
    for (name, params) in TransitionParams::presets() {
        let whole = run(params, &input, &on, 64);
        for block in [1, 37, 100] {
            assert!(run(params, &input, &on, block) == whole, "{name} in blocks of {block}");
        }
    }
}

/// Every factory preset: in range, «Categoría · Nombre», and switched on it
/// sounds without going over what comes in (+1 dB).
#[test]
fn presets_leave_headroom() {
    let n = 6 * BEAT;
    // A beat: a low thump every beat and noise hats every eighth.
    let input: Vec<f32> = (0..n)
        .map(|i| {
            let t = (i % BEAT) as f32 / SR;
            let h = (i % (BEAT / 2)) as f32 / SR;
            0.6 * (-t * 12.0).exp() * (TAU * 55.0 * t).sin() + 0.15 * (-h * 60.0).exp() * ((i * 7919 % 1000) as f32 / 500.0 - 1.0)
        })
        .collect();
    let peak_in = input.iter().fold(0f32, |m, s| m.max(s.abs()));
    let presets = Effect::Transition(Default::default()).factory_presets();
    assert!(presets.len() >= 16);
    for (name, preset) in presets {
        assert!(name.contains(" · "), "{name}");
        for (i, info) in preset.params().iter().enumerate() {
            assert_eq!(info.clamp(preset.get(i)), preset.get(i), "{name}: {}", info.name);
        }
        let Effect::Transition(params) = preset else { unreachable!() };
        assert!(!params.on(), "{name}: off until a lane switches it");
        assert!(crate::model::TRANSITION_LENGTHS.contains(&params.length_beats) && crate::model::TRANSITION_GRIDS.contains(&params.grid_beats), "{name}");
        let out = run(params, &input, &|f| (BEAT..4 * BEAT).contains(&f), 64);
        let peak = out.iter().fold(0f32, |m, s| m.max(s.abs()));
        let level = db(rms(&out[BEAT + L..4 * BEAT + L]) / rms(&input[BEAT..4 * BEAT]));
        println!("{name:40} pico {:5.1} dB (entrada {:5.1}), nivel {level:6.1} dB", db(peak), db(peak_in));
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(peak <= peak_in * db_to_gain(1.0), "{name}: peak {peak} over {peak_in}");
    }
}

/// While stopped, its button still works (live, at the tempo); and stopping
/// the transport while it is on lets go.
#[test]
fn stopped_it_works_by_hand_and_stopping_lets_go() {
    let input = sine(1000.0, 0.5, 4 * BEAT);
    let stopped = ProcessContext { beats_per_sample: 0.0, ..ctx(0) };
    let mut fx = Transition::new(tape(TransitionMode::TapeStop, 1.0, 0.0), SR);
    let mut out = input.clone();
    let mut right = input.clone();
    for (k, (a, b)) in out.chunks_mut(256).zip(right.chunks_mut(256)).enumerate() {
        fx.params.active = if k >= 10 { 1.0 } else { 0.0 };
        fx.process(a, b, &stopped);
    }
    let start = 2560 + L;
    let hz = frequency(&out[start + BEAT / 2 - 1024..start + BEAT / 2 + 1024]);
    assert!((hz - 500.0).abs() < 10.0, "a beat at 120 BPM: halfway at half speed ({hz} Hz)");
    // On and playing, then the transport stops: the sound comes back.
    let mut fx = Transition::new(tape(TransitionMode::TapeStop, 0.25, 0.0), SR);
    fx.params.active = 1.0;
    let (mut a, mut b) = (input.clone(), input.clone());
    let half = 2 * BEAT;
    fx.process(&mut a[..half], &mut b[..half], &ctx(0));
    fx.process(&mut a[half..], &mut b[half..], &stopped);
    assert!(rms(&a[half - 1000..half]) < 1e-6, "stopped (the tape)");
    assert_eq!(&a[half + 200..], &input[half + 200 - L..4 * BEAT - L], "the transport stopped: the sound again");
}

/// Microseconds per 128-frame block at 48 kHz, repeating with a filter:
/// `cargo test --release -p engine transition_cpu -- --ignored --nocapture`.
#[test]
#[ignore]
fn transition_cpu() {
    let input = noise(10 * BEAT);
    for (name, params, on) in [
        ("repetición con filtro", TransitionParams { mode: TransitionMode::Repeat, grid_beats: 0.125, pitch: -1.0, filter: Some(FilterMode::LowPass), sweep: 0.3, ..Default::default() }, true),
        ("parada de cinta", tape(TransitionMode::TapeStop, 8.0, 0.0), true),
        ("en espera", TransitionParams::default(), false),
    ] {
        let started = std::time::Instant::now();
        let out = run(params, &input, &|_| on, 128);
        let blocks = input.len() as f64 / 128.0;
        println!("{name}: {:.2} µs por bloque de 128 ({:.3} % de un núcleo)", started.elapsed().as_secs_f64() * 1e6 / blocks, started.elapsed().as_secs_f64() / (input.len() as f64 / SR as f64) * 100.0);
        assert!(out.iter().all(|s| s.is_finite()));
    }
}
