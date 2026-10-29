use super::*;
use crate::effects::{EffectProcessor, ProcessContext};
use crate::model::{drum_note, DrumParams, Effect, GlueRatio, SuperSawParams};
use std::f32::consts::TAU;

const SR: f32 = 48_000.0;

fn ctx() -> ProcessContext {
    ProcessContext { sample_rate: SR, tempo: 124.0, beat: 0.0, beats_per_sample: 0.0, scale: 0, meter: Default::default() }
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0f32, |m, s| m.max(s.abs()))
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Crest factor (peak over RMS) of a stereo sound, dB.
fn crest(l: &[f32], r: &[f32]) -> f32 {
    db(peak(l).max(peak(r))) - db(((rms(l).powi(2) + rms(r).powi(2)) / 2.0).sqrt())
}

/// Runs `effect` over a stereo sound in blocks of `block`.
fn run(effect: Effect, l: &[f32], r: &[f32], block: usize) -> (Vec<f32>, Vec<f32>) {
    let mut fx = EffectProcessor::new(&effect, SR);
    let (mut a, mut b) = (l.to_vec(), r.to_vec());
    for (x, y) in a.chunks_mut(block).zip(b.chunks_mut(block)) {
        fx.process(x, y, &ctx());
    }
    (a, b)
}

fn sine(freq: f32, amplitude: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| (TAU * freq * i as f32 / SR).sin() * amplitude).collect()
}

/// A square wave: its level is the same at every sample, so a peak
/// detector sees a clean step.
fn square(freq: f32, amplitude: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| if (freq * i as f32 / SR).fract() < 0.5 { amplitude } else { -amplitude }).collect()
}

/// Two bars of house (124 BPM), twice, from the engine's drum machine.
fn drum_loop() -> (Vec<f32>, Vec<f32>) {
    let mut kit = crate::instruments::DrumMachine::new(DrumParams::default(), SR);
    let step = (SR * 60.0 / 124.0 / 4.0) as usize; // a sixteenth
    let steps = 64;
    let n = step * steps + SR as usize;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for s in 0..steps {
        let at = s * step;
        if s % 4 == 0 {
            kit.note_on(drum_note::KICK, 1.0);
        }
        if s % 8 == 4 {
            kit.note_on(drum_note::CLAP, 0.9);
            kit.note_on(drum_note::SNARE, 0.7);
        }
        if s % 4 == 2 {
            kit.note_on(if s % 16 == 14 { drum_note::OPEN_HAT } else { drum_note::CLOSED_HAT }, 0.8);
        } else if s % 2 == 1 {
            kit.note_on(drum_note::CLOSED_HAT, 0.45);
        }
        kit.render(&mut l[at..at + step], &mut r[at..at + step]);
    }
    let end = steps * step;
    kit.render(&mut l[end..], &mut r[end..]);
    (l, r)
}

/// A super saw pad: two chords of four bars at 124 BPM.
fn pad() -> (Vec<f32>, Vec<f32>) {
    let params = SuperSawParams::presets().into_iter().find(|(name, _)| *name == "Pad · Ancho").unwrap().1;
    let mut synth = crate::instruments::SuperSaw::new(params, SR);
    let bar = (SR * 60.0 / 124.0 * 4.0) as usize;
    let n = bar * 4 + 3 * SR as usize;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for (chord, pitches) in [[57u8, 60, 64, 69], [53, 57, 60, 65]].into_iter().enumerate() {
        for p in pitches {
            synth.note_on(p, 0.8, (bar * 2 - SR as usize / 8) as u32);
        }
        let at = chord * bar * 2;
        synth.render(&mut l[at..at + bar * 2], &mut r[at..at + bar * 2]);
    }
    synth.render(&mut l[bar * 4..], &mut r[bar * 4..]);
    (l, r)
}

fn glue(params: GlueParams) -> Effect {
    Effect::Glue(params)
}

fn hard(threshold_db: f32, ratio: GlueRatio, knee_db: f32) -> GlueParams {
    GlueParams { threshold_db, ratio, knee_db, attack_ms: 0.01, release_s: 1.2, auto_release: false, ..GlueParams::default() }
}

// ----- Bus compressor ------------------------------------------------------------

/// The static curve, measured with steady tones, is the one it promises:
/// unity under the threshold, 1/ratio over it, a smooth knee between.
#[test]
fn glue_static_curve_follows_ratio_and_knee() {
    for (ratio, knee_db) in [(GlueRatio::Two, 0.0), (GlueRatio::Four, 0.0), (GlueRatio::Ten, 0.0), (GlueRatio::Four, 6.0), (GlueRatio::Ten, 12.0)] {
        let threshold = -20.0;
        let mut row = Vec::new();
        for level in [-40.0, -30.0, -24.0, -22.0, -20.0, -18.0, -16.0, -10.0, -5.0, 0.0] {
            let x = square(1000.0, db_to_gain(level), 24_000);
            let (out, _) = run(glue(hard(threshold, ratio, knee_db)), &x, &x, 128);
            let heard = db(peak(&out[12_000..]));
            let expected = level - Glue::curve(level, threshold, ratio.value(), knee_db);
            row.push(format!("{level}→{heard:.1}"));
            assert!((heard - expected).abs() < 0.15, "{ratio:?} knee {knee_db}: {level} dB in, {heard:.2} out, expected {expected:.2}");
        }
        println!("Glue {} knee {knee_db} dB: {}", ratio.label(), row.join("  "));
    }
    // Over the threshold the slope is 1/ratio; the knee is smooth (no step).
    let slope = (Glue::curve(-5.0, -20.0, 4.0, 0.0) - Glue::curve(-10.0, -20.0, 4.0, 0.0)) / 5.0;
    assert!((slope - 0.75).abs() < 1e-5);
    for knee_db in [3.0, 6.0, 12.0] {
        let steps: Vec<f32> = (0..400).map(|i| -40.0 + i as f32 * 0.1).map(|l| Glue::curve(l, -20.0, 10.0, knee_db)).collect();
        assert!(steps.windows(2).all(|w| w[1] >= w[0] && w[1] - w[0] < 0.1), "knee {knee_db}: monotone, no jumps");
    }
}

/// Reduction (dB) after each sample of `x` (one sample a block: its meter).
fn reduction_curve(params: GlueParams, x: &[f32]) -> Vec<f32> {
    let mut fx = Glue::new(params, SR);
    let (mut a, mut b) = (x.to_vec(), x.to_vec());
    a.chunks_mut(1)
        .zip(b.chunks_mut(1))
        .map(|(u, v)| {
            fx.process(u, v);
            fx.reduction()
        })
        .collect()
}

/// Seconds from `from` until `curve` crosses `fraction` of the way from
/// its value there to `to`.
fn time_to(curve: &[f32], from: usize, to: f32, fraction: f32) -> f32 {
    let start = curve[from];
    let goal = start + fraction * (to - start);
    let rising = to > start;
    let at = curve[from..].iter().position(|&v| if rising { v >= goal } else { v <= goal }).expect("never got there");
    at as f32 / SR
}

/// Attack and release times, measured with tone bursts (a square wave from
/// −40 to 0 dBFS and back), are the knobs' (a time constant: 63 %).
#[test]
fn glue_attack_and_release_are_what_the_knobs_say() {
    let burst = |on: usize, total: usize| -> Vec<f32> {
        let loud = square(1000.0, 1.0, total);
        let quiet = square(1000.0, 0.01, total);
        (0..total).map(|i| if (SR as usize / 10..SR as usize / 10 + on).contains(&i) { loud[i] } else { quiet[i] }).collect()
    };
    let start = SR as usize / 10;
    let full = Glue::curve(0.0, -20.0, 10.0, 0.0);
    let mut report = Vec::new();
    for attack_ms in [0.1, 1.0, 3.0, 10.0, 30.0] {
        let p = GlueParams { attack_ms, ..hard(-20.0, GlueRatio::Ten, 0.0) };
        let curve = reduction_curve(p, &burst(SR as usize / 2, SR as usize));
        let took = time_to(&curve, start, full, 1.0 - (-1f32).exp()) * 1000.0;
        report.push(format!("ataque {attack_ms} ms → {took:.2} ms"));
        assert!((took - attack_ms).abs() <= attack_ms * 0.05 + 0.03, "attack {attack_ms} ms took {took:.3} ms");
    }
    for release_s in [0.1, 0.2, 0.4, 0.8, 1.2] {
        let p = GlueParams { release_s, ..hard(-20.0, GlueRatio::Ten, 0.0) };
        let on = SR as usize / 2;
        let curve = reduction_curve(p, &burst(on, (SR * 6.0) as usize));
        let took = time_to(&curve, start + on, 0.0, 1.0 - (-1f32).exp());
        report.push(format!("release {release_s} s → {took:.3} s"));
        assert!((took - release_s).abs() <= release_s * 0.05, "release {release_s} s took {took:.3} s");
    }
    // Auto: quick after a short hit, slower after a long stretch.
    let auto = GlueParams { auto_release: true, ..hard(-20.0, GlueRatio::Ten, 0.0) };
    let release_after = |on: usize| {
        let curve = reduction_curve(auto, &burst(on, start + on + (SR * 6.0) as usize));
        // Time to fall to 37 % of where it was when the burst ended.
        time_to(&curve, start + on, 0.0, 1.0 - (-1f32).exp())
    };
    let (short, long) = (release_after(SR as usize / 20), release_after((SR * 3.0) as usize));
    report.push(format!("auto: tras 50 ms → {short:.3} s, tras 3 s → {long:.3} s"));
    assert!(short < 0.2, "after a short hit it lets go fast: {short:.3} s");
    assert!(long > 0.5 && long < 1.5, "after a long stretch it lets go slowly: {long:.3} s");
    println!("{}", report.join("\n"));
}

/// With the sidechain filter, a kick in the lows no longer pulls the whole
/// bus down; hats above it still do.
#[test]
fn glue_sidechain_filter_ignores_the_kick() {
    let kick = sine(50.0, 0.7, SR as usize);
    let hats = sine(8000.0, 0.7, SR as usize);
    let most = |params: GlueParams, x: &[f32]| {
        let mut fx = Glue::new(params, SR);
        let (mut a, mut b) = (x.to_vec(), x.to_vec());
        let mut most = 0f32;
        for (u, v) in a.chunks_mut(128).zip(b.chunks_mut(128)) {
            fx.process(u, v);
            most = most.max(fx.reduction());
        }
        most
    };
    let base = GlueParams { threshold_db: -20.0, auto_release: false, release_s: 0.2, ..GlueParams::default() };
    let open = most(base, &kick);
    let filtered = most(GlueParams { sidechain_hpf_hz: 200.0, ..base }, &kick);
    let hats_filtered = most(GlueParams { sidechain_hpf_hz: 200.0, ..base }, &hats);
    println!("bombo: {open:.1} dB sin filtro, {filtered:.1} dB con 200 Hz; charles con filtro {hats_filtered:.1} dB");
    assert!(open > 10.0, "{open}");
    assert!(filtered < open * 0.3, "{filtered} vs {open}");
    assert!((hats_filtered - open).abs() < 1.5, "the highs still compress: {hats_filtered}");
}

/// Another track's sound drives it (sidechain), as with the Compressor.
#[test]
fn glue_listens_to_its_sidechain() {
    let mut fx = EffectProcessor::new(&glue(GlueParams { threshold_db: -20.0, ..GlueParams::default() }), SR);
    let quiet = sine(220.0, 0.05, 128);
    let loud = vec![0.9f32; 128];
    let mut last = (0.0, 0.0);
    for _ in 0..200 {
        fx.feed_key(&loud, &loud);
        let (mut a, mut b) = (quiet.clone(), quiet.clone());
        fx.process(&mut a, &mut b, &ctx());
        last = (peak(&a), fx.meter().unwrap()[0]);
    }
    assert!(last.1 > 10.0 && db(last.0) < db(0.05) - 10.0, "ducked by the key: {last:?}");
    assert_eq!(fx.sidechain(), None);
}

/// Dry/wet blends in the dry sound (parallel compression); at 0 it is the
/// input untouched. The soft clip keeps it under 0 dBFS.
#[test]
fn glue_mix_and_soft_clip() {
    let (l, r) = drum_loop();
    let crushed = GlueParams { threshold_db: -30.0, ratio: GlueRatio::Ten, makeup_db: 12.0, ..GlueParams::default() };
    let (dry, _) = run(glue(GlueParams { mix: 0.0, ..crushed }), &l, &r, 256);
    assert_eq!(dry, l, "all dry: untouched");
    let (wet, _) = run(glue(crushed), &l, &r, 256);
    let (half, _) = run(glue(GlueParams { mix: 0.5, ..crushed }), &l, &r, 256);
    let worst = half.iter().zip(&wet).zip(&l).map(|((h, w), d)| (h - 0.5 * (w + d)).abs()).fold(0f32, f32::max);
    assert!(worst < 1e-5, "half: the mean of dry and wet ({worst})");
    let (clipped, _) = run(glue(GlueParams { makeup_db: 20.0, soft_clip: true, ..crushed }), &l, &r, 256);
    assert!(peak(&clipped) <= CLIP_CEILING + 1e-6, "{}", peak(&clipped));
    assert!(peak(&clipped) > 0.9, "it gets close to the ceiling");
}

// ----- Multiband: crossovers -------------------------------------------------------

/// Magnitude (dB) of an impulse response at `freq`, by a direct DFT.
fn magnitude_db(h: &[f32], freq: f32) -> f32 {
    let (mut re, mut im) = (0f64, 0f64);
    for (n, &v) in h.iter().enumerate() {
        let w = std::f64::consts::TAU * freq as f64 * n as f64 / SR as f64;
        re += v as f64 * w.cos();
        im -= v as f64 * w.sin();
    }
    (20.0 * (re * re + im * im).sqrt().log10()) as f32
}

fn impulse(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

const PROBES: [f32; 14] = [20.0, 40.0, 70.0, 88.0, 120.0, 250.0, 600.0, 1200.0, 2500.0, 4000.0, 7000.0, 10000.0, 15000.0, 20000.0];

/// With no compression the three bands add up flat (±0.1 dB) and in phase:
/// to the same allpass as the two crossovers', sample by sample.
#[test]
fn multiband_bands_add_up_flat_and_phase_coherent() {
    for (low_hz, high_hz) in [(88.0, 2500.0), (200.0, 5000.0), (40.0, 800.0)] {
        let neutral = MultibandParams { depth: 0.0, low_hz, high_hz, ..MultibandParams::default() };
        let x = impulse(1 << 15);
        let (h, _) = run(Effect::Multiband(neutral), &x, &x, 128);
        let mut worst = 0f32;
        for f in PROBES {
            worst = worst.max(magnitude_db(&h, f).abs());
        }
        assert!(worst < 0.1, "{low_hz}/{high_hz} Hz: off by {worst:.3} dB");
        // The reference: the second-order allpasses of both crossovers.
        let (c1, c2) = (SvfCoefs::new(low_hz, SR), SvfCoefs::new(high_hz, SR));
        let (mut a1, mut a2) = (Svf::default(), Svf::default());
        let reference: Vec<f32> = x
            .iter()
            .map(|&v| {
                let y = v - 2.0 * K * a1.tick(v, &c1).1;
                y - 2.0 * K * a2.tick(y, &c2).1
            })
            .collect();
        let error = h.iter().zip(&reference).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        assert!(error < 1e-4, "{low_hz}/{high_hz} Hz: not the allpass ({error})");
        println!("cruces {low_hz}/{high_hz} Hz: suma plana a ±{worst:.4} dB, igual al pasa-todo a {error:.1e}");
    }
}

/// Each band keeps to its range: 24 dB an octave out of it.
#[test]
fn multiband_bands_are_isolated() {
    let (c1, c2) = (SvfCoefs::new(88.0, SR), SvfCoefs::new(2500.0, SR));
    let mut split = Split::default();
    let bands: Vec<[f32; 3]> = impulse(1 << 15).iter().map(|&v| split.tick(v, &c1, &c2)).collect();
    let band = |b: usize| bands.iter().map(|s| s[b]).collect::<Vec<f32>>();
    let (low, mid, high) = (band(0), band(1), band(2));
    // At the crossovers, −6 dB each (Linkwitz-Riley).
    assert!((magnitude_db(&low, 88.0) + 6.02).abs() < 0.1 && (magnitude_db(&mid, 88.0) + 6.02).abs() < 0.3);
    assert!((magnitude_db(&mid, 2500.0) + 6.02).abs() < 0.3 && (magnitude_db(&high, 2500.0) + 6.02).abs() < 0.1);
    // Two octaves out: under −45 dB.
    for (name, h, f) in [("low", &low, 352.0), ("low", &low, 5000.0), ("mid", &mid, 22.0), ("mid", &mid, 10000.0), ("high", &high, 625.0), ("high", &high, 88.0)] {
        let m = magnitude_db(h, f);
        assert!(m < -45.0, "{name} band at {f} Hz: {m:.1} dB");
    }
    // In its own range, flat.
    assert!(magnitude_db(&low, 30.0).abs() < 0.3 && magnitude_db(&mid, 500.0).abs() < 0.3 && magnitude_db(&high, 10000.0).abs() < 0.3);
}

// ----- Multiband: dynamics ---------------------------------------------------------

/// Gain (dB) on a steady sine once settled.
fn steady_gain(params: MultibandParams, freq: f32, amplitude: f32) -> f32 {
    let x = sine(freq, amplitude, 2 * SR as usize);
    let (y, _) = run(Effect::Multiband(params), &x, &x, 128);
    db(rms(&y[SR as usize..]) / rms(&x[SR as usize..]))
}

/// A quiet sound is pulled up, a loud one pushed down, in every band;
/// with depth at 0 nothing changes.
#[test]
fn multiband_lifts_the_quiet_and_pushes_down_the_loud() {
    let d = MultibandParams::default();
    for freq in [50.0, 1000.0, 8000.0] {
        let quiet = steady_gain(d, freq, db_to_gain(-60.0));
        let loud = steady_gain(d, freq, db_to_gain(-6.0));
        let neutral = steady_gain(MultibandParams { depth: 0.0, ..d }, freq, db_to_gain(-6.0));
        let gentle = steady_gain(MultibandParams { depth: 0.3, ..d }, freq, db_to_gain(-60.0));
        let only_down = steady_gain(MultibandParams { upward: 0.0, ..d }, freq, db_to_gain(-60.0));
        println!("{freq} Hz: −60 dBFS {quiet:+.1} dB (30 %: {gentle:+.1}, sin upward: {only_down:+.1}), −6 dBFS {loud:+.1} dB");
        assert!(quiet > 25.0, "{freq} Hz: quiet pulled up {quiet}");
        // 54 dB apart going in, under 24 coming out: the loud one is
        // pushed down by all of its make-up.
        assert!(loud < 3.0 && quiet - loud > 30.0, "{freq} Hz: loud pushed down {loud}");
        assert!(neutral.abs() < 0.1, "{freq} Hz: depth 0 {neutral}");
        assert!(gentle > 3.0 && gentle < quiet * 0.5, "{freq} Hz: 30 % does less {gentle}");
        // Without upward only the make-up is left at the bottom.
        assert!(only_down < 21.0 && only_down < quiet - 5.0, "{freq} Hz: {only_down}");
    }
    // Silence stays silence (nothing to pull up from).
    let silent = vec![0.0f32; 48_000];
    let (y, _) = run(Effect::Multiband(d), &silent, &silent, 128);
    assert!(y.iter().all(|&s| s == 0.0));
}

/// Its time constants scale with «Time».
#[test]
fn multiband_time_scales_attack() {
    let curve = |time: f32| {
        let mut fx = Multiband::new(MultibandParams { time, ..MultibandParams::default() }, SR);
        let x: Vec<f32> = (0..SR as usize).map(|i| if i < 4800 { 0.0 } else { (TAU * 1000.0 * i as f32 / SR).sin() * 0.5 }).collect();
        let (mut a, mut b) = (x.clone(), x);
        a.chunks_mut(16).zip(b.chunks_mut(16)).map(|(u, v)| {
            fx.process(u, v);
            fx.reduction()[1]
        }).collect::<Vec<f32>>()
    };
    let settle = |c: &[f32]| (c.iter().position(|&v| v < 0.9 * c[c.len() - 1]).unwrap() as f32 * 16.0 - 4800.0) / SR;
    let (base, slow) = (settle(&curve(1.0)), settle(&curve(4.0)));
    assert!(slow > 2.5 * base, "×4 time is slower: {base:.4} s vs {slow:.4} s");
}

/// A hit and its tail: the tail comes up, the hit goes down (OTT), and a
/// drum loop loses crest factor.
#[test]
fn multiband_squashes_a_drum_loop() {
    // A decaying tone, as a pad's or a reverb's tail: 20 dB a second.
    let n = (SR * 3.5) as usize;
    let x: Vec<f32> = (0..n).map(|i| (TAU * 440.0 * i as f32 / SR).sin() * db_to_gain(-20.0 * i as f32 / SR)).collect();
    let (y, _) = run(Effect::Multiband(MultibandParams::default()), &x, &x, 128);
    let level = |s: &[f32], at: f32| db(rms(&s[(at * SR) as usize..(at * SR) as usize + 2400]));
    let (hit_in, tail_in, hit_out, tail_out) = (level(&x, 0.05), level(&x, 2.8), level(&y, 0.05), level(&y, 2.8));
    println!("cola: golpe {hit_in:.1} → {hit_out:.1} dB, cola {tail_in:.1} → {tail_out:.1} dB");
    assert!(tail_out - tail_in > 10.0, "the tail is lifted");
    assert!(hit_out < hit_in - 5.0, "the hit is pushed down");
    assert!((hit_out - tail_out) < 0.5 * (hit_in - tail_in), "the distance between them halves at least");

    let (l, r) = drum_loop();
    // The loop without its last second (the kit ringing out).
    let k = l.len() - SR as usize;
    let (l, r) = (&l[..k], &r[..k]);
    // How far apart the loud and quiet moments are: 50 ms RMS, top to bottom.
    let range = |x: &[f32]| {
        let levels: Vec<f32> = x.chunks(2400).map(|c| db(rms(c))).collect();
        levels.iter().fold(f32::MIN, |m, &v| m.max(v)) - levels.iter().fold(f32::MAX, |m, &v| m.min(v))
    };
    let (before, spread) = (crest(l, r), range(l));
    for (name, params) in [("OTT 100 %", MultibandParams::default()), ("OTT 30 %", MultibandParams { depth: 0.3, ..MultibandParams::default() })] {
        let (a, b) = run(Effect::Multiband(params), l, r, 128);
        let (after, flat) = (crest(&a, &b), range(&a));
        println!("{name}: factor de cresta {before:.1} → {after:.1} dB, distancia fuerte-flojo (50 ms) {spread:.1} → {flat:.1} dB; pico {:.1} → {:.1} dBFS, RMS {:.1} → {:.1} dBFS", db(peak(l)), db(peak(&a)), db(rms(l)), db(rms(&a)));
        assert!(after < before, "{name}: crest {before:.1} → {after:.1}");
        if params.depth > 0.5 {
            assert!(flat < 0.5 * spread, "{name}: {spread:.1} → {flat:.1} dB");
            assert!(db(rms(&a)) > db(rms(l)) - 1.0, "{name}: about as loud or louder");
        }
    }
    // The bus compressor holds the loud moments together; with its 10 ms
    // attack the hits' first milliseconds go through (punch: more crest),
    // with a fast one they don't.
    for (name, p) in [("Bus de batería", GlueParams::presets()[0].1), ("ataque 0,1 ms", GlueParams { attack_ms: 0.1, ..GlueParams::presets()[0].1 })] {
        let (a, b) = run(glue(p), l, r, 128);
        println!("{name}: factor de cresta {before:.1} → {:.1} dB, distancia {spread:.1} → {:.1} dB", crest(&a, &b), range(&a));
        assert!(range(&a) < spread - 1.5, "{name}");
        if p.attack_ms < 1.0 {
            assert!(crest(&a, &b) < before - 1.0, "{name}");
        }
    }
}

// ----- Both ----------------------------------------------------------------------

fn both() -> [Effect; 4] {
    [
        glue(GlueParams::default()),
        glue(GlueParams { sidechain_hpf_hz: 150.0, soft_clip: true, makeup_db: 8.0, mix: 0.7, ..GlueParams::default() }),
        Effect::Multiband(MultibandParams::default()),
        Effect::Multiband(MultibandParams { depth: 0.6, time: 0.4, mix: 0.5, band_in_db: [3.0, -2.0, 1.0], band_out_db: [-1.0, 2.0, 0.5], ..MultibandParams::default() }),
    ]
}

/// The same sound whatever the blocks are cut into.
#[test]
fn dynamics_do_not_depend_on_the_block_size() {
    let (l, r) = drum_loop();
    for effect in both() {
        let reference = run(effect, &l, &r, 4096);
        for block in [1, 31, 64, 128, 1000] {
            assert!(run(effect, &l, &r, block) == reference, "{} in blocks of {block}", effect.label());
        }
    }
}

/// Turning a knob all the way at once doesn't click: the jump between
/// samples stays near the signal's own.
#[test]
fn dynamics_settings_move_without_clicks() {
    let x = sine(100.0, 0.5, SR as usize);
    let own = x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
    let cases: [(Effect, Effect); 4] = [
        (glue(GlueParams { threshold_db: 0.0, attack_ms: 0.01, ..GlueParams::default() }), glue(GlueParams { threshold_db: -40.0, attack_ms: 0.01, ..GlueParams::default() })),
        (glue(GlueParams { makeup_db: 0.0, ..GlueParams::default() }), glue(GlueParams { makeup_db: 20.0, ..GlueParams::default() })),
        (Effect::Multiband(MultibandParams { depth: 0.0, ..MultibandParams::default() }), Effect::Multiband(MultibandParams::default())),
        (Effect::Multiband(MultibandParams::default()), Effect::Multiband(MultibandParams { low_hz: 400.0, high_hz: 9000.0, output_db: 12.0, ..MultibandParams::default() })),
    ];
    for (from, to) in cases {
        let mut fx = EffectProcessor::new(&from, SR);
        let (mut a, mut b) = (x.clone(), x.clone());
        let half = a.len() / 2;
        let (a1, a2) = a.split_at_mut(half);
        let (b1, b2) = b.split_at_mut(half);
        fx.process(a1, b1, &ctx());
        let before = a1.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max).max(own);
        fx.set_params(&to);
        fx.process(a2, b2, &ctx());
        let jump = a[half - 64..half + 4800].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
        let after = a[a.len() - 4800..].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
        // A click jumps many times more than the sound does before or after.
        assert!(jump < 1.5 * before.max(after), "{}: {jump} vs {before} before, {after} after", to.label());
    }
}

/// Every factory preset sounds, stays finite and under a sane level.
#[test]
fn dynamics_presets_stay_sane() {
    let (l, r) = drum_loop();
    let (pl, pr) = pad();
    let effects = GlueParams::presets().into_iter().map(|(n, p)| (n, glue(p))).chain(MultibandParams::presets().into_iter().map(|(n, p)| (n, Effect::Multiband(p))));
    for (name, effect) in effects {
        for (x, y) in [(&l, &r), (&pl, &pr)] {
            let (a, b) = run(effect, x, y, 256);
            assert!(a.iter().chain(&b).all(|s| s.is_finite()), "{name}");
            let p = db(peak(&a).max(peak(&b)));
            assert!(p < 12.0 && p > db(peak(x)) - 24.0, "{name}: peak {p:.1} dBFS");
            assert!(name.contains(" · "), "{name}: «Categoría · Nombre»");
        }
    }
}

/// Microseconds per 128-sample block at 48 kHz:
/// `cargo test --release -p engine dynamics_cpu -- --ignored --nocapture`.
#[test]
#[ignore]
fn dynamics_cpu() {
    let (l, r) = drum_loop();
    for effect in both() {
        let mut fx = EffectProcessor::new(&effect, SR);
        let (mut a, mut b) = (l.clone(), r.clone());
        let blocks = a.len() / 128;
        let started = std::time::Instant::now();
        for _ in 0..4 {
            a.copy_from_slice(&l);
            b.copy_from_slice(&r);
            for (u, v) in a.chunks_mut(128).zip(b.chunks_mut(128)) {
                fx.process(u, v, &ctx());
            }
        }
        let us = started.elapsed().as_secs_f64() * 1e6 / (4 * blocks) as f64;
        println!("{:18} {us:.2} µs / bloque de 128 ({:.2} % de un núcleo)", effect.label(), us / (128.0 / 48_000.0 * 1e6) * 100.0);
    }
}

/// Before/after WAVs to listen to: `PULSE_DYNAMICS_WAVS=dir cargo test
/// --release -p engine dynamics_wavs -- --ignored`.
#[test]
#[ignore]
fn dynamics_wavs() {
    let Ok(dir) = std::env::var("PULSE_DYNAMICS_WAVS") else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, l: &[f32], r: &[f32]| {
        let temp = format!("{dir}/.{name}.tmp");
        crate::keys::tests::write_wav(&temp, l, r);
        std::fs::rename(&temp, format!("{dir}/{name}.wav")).unwrap();
    };
    let (l, r) = drum_loop();
    let (pl, pr) = pad();
    write("bateria-seca", &l, &r);
    write("pad-seco", &pl, &pr);
    for (name, p) in GlueParams::presets() {
        let (a, b) = run(glue(p), &l, &r, 128);
        write(&format!("bateria-compresor-de-bus-{}", slug(name)), &a, &b);
    }
    for (name, p) in MultibandParams::presets() {
        let (a, b) = run(Effect::Multiband(p), &pl, &pr, 128);
        write(&format!("pad-ott-{}", slug(name)), &a, &b);
        let (a, b) = run(Effect::Multiband(p), &l, &r, 128);
        write(&format!("bateria-ott-{}", slug(name)), &a, &b);
    }
}

fn slug(name: &str) -> String {
    let short = name.split(" · ").last().unwrap_or(name).to_lowercase();
    short.chars().map(|c| match c {
        'á' => 'a',
        'é' => 'e',
        'í' => 'i',
        'ó' => 'o',
        'ú' => 'u',
        'ñ' => 'n',
        c if c.is_alphanumeric() => c,
        _ => '-',
    }).collect()
}

