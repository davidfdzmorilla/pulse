use super::*;
use crate::effects::{EffectProcessor, ProcessContext};
use crate::model::{Effect, VocoderChord};
use std::f32::consts::TAU;

const SR: f32 = 48_000.0;

fn ctx() -> ProcessContext {
    ProcessContext { sample_rate: SR, tempo: 120.0, beat: 0.0, beats_per_sample: 0.0, scale: 0, meter: Default::default() }
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
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

/// A vocoder with its sidechain fed `carrier`, over `voice`, in blocks of `block`.
fn run_keyed(params: VocoderParams, voice: &[f32], carrier: &[f32], block: usize) -> Vec<f32> {
    let mut fx = EffectProcessor::new(&Effect::Vocoder(VocoderParams { sidechain: Some(1), ..params }), SR);
    let (mut a, mut b) = (voice.to_vec(), voice.to_vec());
    for ((x, y), key) in a.chunks_mut(block).zip(b.chunks_mut(block)).zip(carrier.chunks(block)) {
        fx.feed_key(key, key);
        fx.process(x, y, &ctx());
    }
    a
}

/// A sung vowel: a pulse a period (`hz`) through three formants (Hz).
fn vowel(hz: f32, formants: [f32; 3], seconds: f32) -> Vec<f32> {
    let n = (seconds * SR) as usize;
    let mut x = vec![0.0f32; n];
    let mut phase = 0.0f32;
    for s in x.iter_mut() {
        phase += hz / SR;
        if phase >= 1.0 {
            phase -= 1.0;
            *s = 1.0;
        }
    }
    for (f, bw) in formants.into_iter().zip([90.0, 110.0, 160.0]) {
        // A two-pole resonator, unity at its peak.
        let r = (-PI * bw / SR).exp();
        let (a1, a2) = (2.0 * r * (TAU * f / SR).cos(), -r * r);
        let g = 1.0 - r;
        let (mut y1, mut y2) = (0.0f32, 0.0f32);
        for s in x.iter_mut() {
            let y = g * *s + a1 * y1 + a2 * y2;
            y2 = y1;
            y1 = y;
            *s = y;
        }
    }
    let peak = x.iter().fold(0f32, |m, s| m.max(s.abs()));
    x.iter_mut().for_each(|s| *s *= 0.5 / peak);
    x
}

/// An «a» (formants of a man's voice).
fn ah(hz: f32, seconds: f32) -> Vec<f32> {
    vowel(hz, [730.0, 1090.0, 2440.0], seconds)
}

/// The pitch (Hz) heard in `x` around `at` (YIN, as the shift hears it).
fn pitch_at(x: &[f32], at: usize) -> Option<f32> {
    let mut detector = Detector::with_range(SR, 60.0, 1200.0);
    let len = detector.len;
    detector.period(&x[at..at + len]).map(|p| SR / p)
}

fn cents(a: f32, b: f32) -> f32 {
    1200.0 * (a / b).log2()
}

/// The magnitude spectrum's centroid (Hz) of 8192 samples of `x` from `at`,
/// between 100 Hz and 8 kHz.
fn centroid(x: &[f32], at: usize) -> f32 {
    const N: usize = 8192;
    let mut re: Vec<f32> = (0..N).map(|i| x[at + i] * (0.5 - 0.5 * (TAU * i as f32 / N as f32).cos())).collect();
    let mut im = vec![0.0; N];
    crate::dsp::fft(&mut re, &mut im);
    let (mut sum, mut weighted) = (0.0f64, 0.0f64);
    for k in 1..N / 2 {
        let hz = k as f32 * SR / N as f32;
        if (100.0..8000.0).contains(&hz) {
            let m = (re[k] * re[k] + im[k] * im[k]).sqrt() as f64;
            sum += m;
            weighted += m * hz as f64;
        }
    }
    (weighted / sum) as f32
}

/// The level (RMS) of `x` around `hz`, in windows of `window`: a Goertzel
/// of each window, Hann-weighted.
fn band_envelope(x: &[f32], hz: f32, window: usize) -> Vec<f32> {
    x.chunks_exact(window)
        .map(|w| {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, s) in w.iter().enumerate() {
                let hann = 0.5 - 0.5 * (TAU * i as f32 / window as f32).cos();
                let a = TAU * hz * i as f32 / SR;
                re += (s * hann * a.cos()) as f64;
                im += (s * hann * a.sin()) as f64;
            }
            ((re * re + im * im).sqrt() / window as f64 * 4.0) as f32
        })
        .collect()
}

/// The level (RMS) of `x` within a sixth of an octave of `hz`, in windows
/// of `window` (a power of two).
fn band_power(x: &[f32], hz: f32, window: usize) -> Vec<f32> {
    x.chunks_exact(window)
        .map(|w| {
            let mut re: Vec<f32> = w.iter().enumerate().map(|(i, s)| s * (0.5 - 0.5 * (TAU * i as f32 / window as f32).cos())).collect();
            let mut im = vec![0.0; window];
            crate::dsp::fft(&mut re, &mut im);
            let bins = (1..window / 2).filter(|&k| (hz / 1.12..hz * 1.12).contains(&(k as f32 * SR / window as f32)));
            (bins.map(|k| (re[k] * re[k] + im[k] * im[k]) as f64).sum::<f64>().sqrt() / window as f64) as f32
        })
        .collect()
}

fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len()) as f32;
    let (ma, mb) = (a.iter().sum::<f32>() / n, b.iter().sum::<f32>() / n);
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        ab += (x - ma) * (y - mb);
        aa += (x - ma) * (x - ma);
        bb += (y - mb) * (y - mb);
    }
    ab / (aa * bb).sqrt()
}

fn clean(x: &[f32]) -> bool {
    x.iter().all(|s| s.is_finite() && (*s == 0.0 || s.abs() >= f32::MIN_POSITIVE))
}

// ----- Vocoder ---------------------------------------------------------------------------

/// With a noise carrier, two tones (one swelling while the other fades) come
/// out each at its own place, each following its own envelope.
#[test]
fn the_vocoder_follows_each_bands_envelope() {
    let n = (4.0 * SR) as usize;
    let (low, high) = (400.0, 2500.0);
    let swell = |i: usize| (i as f32 / n as f32 * PI).sin().powi(2);
    let fade = |i: usize| (1.0 - i as f32 / n as f32).powi(2);
    let voice: Vec<f32> = (0..n).map(|i| 0.3 * swell(i) * (TAU * low * i as f32 / SR).sin() + 0.3 * fade(i) * (TAU * high * i as f32 / SR).sin()).collect();
    let params = VocoderParams { carrier: VocoderCarrier::Noise, bands: 32.0, sibilance: 0.0, attack_ms: 2.0, release_ms: 20.0, ..Default::default() };
    let (out, _) = run(Effect::Vocoder(params), &voice, &voice, 256);
    assert!(clean(&out));
    let window = 8192; // 170 ms
    let skip = 1;
    for (hz, shape) in [(low, &swell as &dyn Fn(usize) -> f32), (high, &fade)] {
        let heard = band_power(&out, hz, window);
        let wanted: Vec<f32> = (0..heard.len()).map(|w| shape(w * window + window / 2)).collect();
        let c = correlation(&heard[skip..], &wanted[skip..]);
        println!("{hz} Hz: correlación con su envolvente {c:.3}");
        assert!(c > 0.9, "{hz} Hz follows its envelope: {c}");
    }
    // Away from both tones there is next to nothing.
    let level = |hz: f32| band_power(&out, hz, window).into_iter().fold(0f32, f32::max);
    let away = level(6000.0).max(level(150.0));
    println!("a 400 Hz {:.1} dB, a 2,5 kHz {:.1} dB, lejos {:.1} dB", db(level(low)), db(level(high)), db(away));
    assert!(db(level(low)) - db(away) > 20.0 && db(level(high)) - db(away) > 20.0);
}

/// Without a voice the vocoder is silent: exactly, whatever the carrier,
/// own or by sidechain.
#[test]
fn without_a_voice_it_is_silent() {
    let n = SR as usize;
    let silence = vec![0.0; n];
    for carrier in VocoderCarrier::ALL {
        let params = VocoderParams { carrier, chord: VocoderChord::Major, sibilance: 1.0, ..Default::default() };
        let (l, r) = run(Effect::Vocoder(params), &silence, &silence, 128);
        assert!(l.iter().chain(&r).all(|s| *s == 0.0), "{carrier:?}");
    }
    let saw: Vec<f32> = (0..n).map(|i| ((i as f32 * 220.0 / SR).fract() * 2.0 - 1.0) * 0.5).collect();
    assert!(run_keyed(Default::default(), &silence, &saw, 128).iter().all(|s| *s == 0.0));
    // A voice stops: it dies away with its release (60 ms), to nothing
    // within a second and a half, no subnormal left on the way.
    let mut voice = ah(140.0, 1.0);
    voice.extend(vec![0.0; 2 * n]);
    for carrier in VocoderCarrier::ALL {
        let (out, _) = run(Effect::Vocoder(VocoderParams { carrier, ..Default::default() }), &voice, &voice, 128);
        assert!(clean(&out), "{carrier:?}");
        assert!(db(rms(&out[n + n / 2..])) < -60.0, "{carrier:?}: {}", db(rms(&out[n + n / 2..])));
        assert!(out[2 * n + n / 2..].iter().all(|s| *s == 0.0), "{carrier:?}: {}", db(rms(&out[2 * n + n / 2..])));
    }
}

/// By sidechain the voice takes the carrier track's pitch; with no sound
/// from it (a track that doesn't play, or isn't there), no sound.
#[test]
fn a_sidechained_carrier_gives_the_voice_its_notes() {
    let n = (1.5 * SR) as usize;
    let voice = ah(110.0, 1.5);
    let mut phase = 0.0f32;
    let saw: Vec<f32> = (0..n)
        .map(|_| {
            phase = (phase + 330.0 / SR).fract();
            (phase * 2.0 - 1.0) * 0.4
        })
        .collect();
    let out = run_keyed(Default::default(), &voice, &saw, 256);
    let hz = pitch_at(&out, n / 2).expect("a pitch");
    println!("voz a 110 Hz, portadora a 330 Hz: sale a {hz:.1} Hz");
    assert!(cents(hz, 330.0).abs() < 10.0, "{hz}");
    assert!(db(rms(&out[n / 3..])) > -30.0, "it sounds");
    // Its sidechain not fed: silence.
    let mut fx = EffectProcessor::new(&Effect::Vocoder(VocoderParams { sidechain: Some(9), ..Default::default() }), SR);
    let (mut a, mut b) = (voice.clone(), voice.clone());
    for (x, y) in a.chunks_mut(256).zip(b.chunks_mut(256)) {
        fx.process(x, y, &ctx());
    }
    assert!(db(rms(&a)) < -40.0, "only the esses at most: {}", db(rms(&a)));
}

/// Its own carrier sings at its note, and the voice comes out about as loud
/// as it went in.
#[test]
fn its_own_carrier_sings_at_its_note() {
    let voice = ah(120.0, 1.5);
    for note in [36.0, 45.0, 57.0] {
        let (out, _) = run(Effect::Vocoder(VocoderParams { note, sibilance: 0.0, ..Default::default() }), &voice, &voice, 512);
        let hz = pitch_at(&out, 30_000).expect("a pitch");
        let want = 440.0 * 2f32.powf((note - 69.0) / 12.0);
        let level = db(rms(&out[10_000..])) - db(rms(&voice[10_000..]));
        println!("nota {note}: {hz:.1} Hz (quería {want:.1}), nivel {level:+.1} dB");
        assert!(cents(hz, want).abs() < 10.0, "{note}: {hz}");
        assert!(level.abs() < 4.0, "{note}: {level:+.1} dB");
    }
}

/// Formant moves the carrier's bands: up, brighter; down, darker.
#[test]
fn the_vocoders_formant_moves_its_bands() {
    let voice = ah(120.0, 1.0);
    let at = |formant: f32| {
        let (out, _) = run(Effect::Vocoder(VocoderParams { formant, carrier: VocoderCarrier::Noise, sibilance: 0.0, ..Default::default() }), &voice, &voice, 256);
        centroid(&out, 20_000)
    };
    let (down, flat, up) = (at(-6.0), at(0.0), at(6.0));
    println!("centroide: −6 {down:.0} Hz, 0 {flat:.0} Hz, +6 {up:.0} Hz");
    assert!(up > flat * 1.2 && down < flat / 1.2);
}

/// Each band count and range works and stays finite; the esses come through.
#[test]
fn every_band_count_works_and_the_esses_come_through() {
    let voice = ah(150.0, 0.5);
    for bands in [8.0, 13.0, 27.0, 40.0] {
        for (low_hz, high_hz) in [(50.0, 1000.0), (100.0, 8000.0), (1000.0, 16_000.0)] {
            let (out, _) = run(Effect::Vocoder(VocoderParams { bands, low_hz, high_hz, ..Default::default() }), &voice, &voice, 100);
            assert!(clean(&out) && rms(&out) > 1e-3, "{bands} {low_hz}–{high_hz}");
        }
    }
    // A hiss (an «s»): with sibilance it is heard as it is.
    let mut noise = 1u32;
    let hiss: Vec<f32> = (0..24_000)
        .map(|_| {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            (noise as f32 / u32::MAX as f32 - 0.5) * 0.2
        })
        .collect();
    let hiss: Vec<f32> = hiss.windows(2).map(|w| w[1] - w[0]).collect();
    let level = |sibilance: f32| {
        let (out, _) = run(Effect::Vocoder(VocoderParams { sibilance, high_hz: 4000.0, ..Default::default() }), &hiss, &hiss, 256);
        rms(&band_envelope(&out, 9000.0, 960))
    };
    println!("«s» a 9 kHz: sin {:.1} dB, con {:.1} dB", db(level(0.0)), db(level(1.0)));
    assert!(db(level(1.0)) - db(level(0.0)) > 12.0);
}

// ----- VoiceShift ------------------------------------------------------------------------

fn shift(pitch: f32, formant: f32) -> Effect {
    Effect::VoiceShift(VoiceShiftParams { pitch, formant, ..Default::default() })
}

/// The pitch moves the semitones asked for, at any voice, and the level
/// stays about the same.
#[test]
fn the_shift_moves_the_pitch_the_semitones_asked_for() {
    for hz in [110.0, 196.0, 330.0] {
        let voice = ah(hz, 1.5);
        for semitones in [-12.0, -7.0, -3.0, 4.0, 7.0, 12.0] {
            let want = hz * 2f32.powf(semitones / 12.0);
            if want < 65.0 {
                continue;
            }
            let (out, _) = run(shift(semitones, 0.0), &voice, &voice, 333);
            let heard: Vec<f32> = [24_000, 36_000, 48_000].iter().map(|&at| pitch_at(&out, at).expect("a pitch")).collect();
            let level = db(rms(&out[20_000..60_000])) - db(rms(&voice[20_000..60_000]));
            println!("{hz} Hz {semitones:+} st: {:?} Hz (quería {want:.1}), nivel {level:+.1} dB", heard.iter().map(|h| h.round()).collect::<Vec<_>>());
            for h in heard {
                assert!(cents(h, want).abs() < 15.0, "{hz} {semitones}: {h} for {want}");
            }
            assert!(level.abs() < 3.0, "{hz} {semitones}: {level:+.1} dB");
        }
    }
    // Cents on top.
    let voice = ah(150.0, 1.0);
    let (out, _) = run(Effect::VoiceShift(VoiceShiftParams { pitch: 2.0, fine: 50.0, ..Default::default() }), &voice, &voice, 256);
    let h = pitch_at(&out, 30_000).unwrap();
    assert!(cents(h, 150.0 * 2f32.powf(2.5 / 12.0)).abs() < 10.0, "{h}");
}

/// The formants move without the pitch: up, the spectrum's centroid goes up
/// (a smaller throat); down, it goes down; the pitch stays.
#[test]
fn the_formants_move_without_the_pitch() {
    let hz = 140.0;
    let voice = ah(hz, 1.5);
    let flat = centroid(&voice, 24_000);
    for (formant, pitch) in [(-5.0, 0.0), (5.0, 0.0), (-4.0, 7.0), (4.0, -7.0)] {
        let (out, _) = run(shift(pitch, formant), &voice, &voice, 256);
        let c = centroid(&out, 24_000);
        let want = hz * 2f32.powf(pitch / 12.0);
        let heard = pitch_at(&out, 30_000).expect("a pitch");
        println!("formante {formant:+} tono {pitch:+}: centroide {flat:.0} → {c:.0} Hz, tono {heard:.1} Hz (quería {want:.1})");
        assert!(cents(heard, want).abs() < 15.0, "the pitch stays: {heard}");
        let ratio = c / flat;
        if formant > 0.0 {
            assert!(ratio > 1.15, "{formant}: {ratio}");
        } else {
            assert!(ratio < 1.0 / 1.15, "{formant}: {ratio}");
        }
    }
    // The formant peak itself moves: the strongest harmonic near the first.
    let (up, _) = run(shift(0.0, 7.0), &voice, &voice, 256);
    let peak = |x: &[f32]| (1..40).map(|k| k as f32 * hz).max_by(|a, b| rms(&band_envelope(&x[24_000..48_000], *a, 4800)).total_cmp(&rms(&band_envelope(&x[24_000..48_000], *b, 4800)))).unwrap();
    let (before, after) = (peak(&voice), peak(&up));
    println!("armónico más fuerte: {before} Hz → {after} Hz");
    assert!(after > before * 1.25, "{before} → {after}");
}

/// Set to nothing, it is the input exactly, its latency later; that latency
/// is what it tells the mixer, about 29 ms at any rate.
#[test]
fn set_to_nothing_it_is_the_input_its_latency_later() {
    let voice = ah(150.0, 0.5);
    let fx = VoiceShift::new(Default::default(), SR);
    let latency = fx.latency();
    assert_eq!(EffectProcessor::new(&shift(0.0, 0.0), SR).latency(), latency);
    assert_eq!(EffectProcessor::new(&shift(7.0, 3.0), SR).latency(), latency, "the same whatever it is set to");
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        let ms = VoiceShift::new(Default::default(), rate).latency() as f32 / rate * 1000.0;
        assert!((31.0..33.0).contains(&ms), "{rate}: {ms} ms");
    }
    let (out, _) = run(shift(0.0, 0.0), &voice, &voice, 100);
    for i in 0..voice.len() - latency {
        assert_eq!(out[i + latency].to_bits(), voice[i].to_bits(), "at {i}");
    }
    // A shifted voice lines up with the dry one (the chorus presets): it
    // starts when the dry one does, its latency later, give or take a grain.
    let mut late = vec![0.0; 4800];
    late.extend(&voice);
    let onset = |x: &[f32]| x.iter().position(|s| s.abs() > 0.05).unwrap();
    for semitones in [-12.0, 7.0] {
        let (moved, _) = run(shift(semitones, 0.0), &late, &late, 100);
        let lag = onset(&moved) as isize - onset(&late) as isize;
        println!("latencia {latency} muestras; la voz a {semitones:+} st empieza a sonar {lag} después");
        assert!((lag - latency as isize).abs() < 2 * (SR / 150.0) as isize, "{lag}");
    }
}

/// Without a voice (noise, silence) it doesn't buzz or blow up, and the
/// silence after a voice is clean.
#[test]
fn noise_and_silence_stay_clean() {
    let mut noise = 7u32;
    let mut x: Vec<f32> = (0..48_000)
        .map(|_| {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            (noise as f32 / u32::MAX as f32 - 0.5) * 0.3
        })
        .collect();
    x.extend(ah(200.0, 0.5));
    x.extend(vec![0.0; 48_000]);
    for (pitch, formant) in [(12.0, 0.0), (-12.0, -12.0), (24.0, 12.0), (-24.0, 12.0)] {
        let (out, _) = run(shift(pitch, formant), &x, &x, 128);
        assert!(clean(&out), "{pitch} {formant}");
        assert!(out[x.len() - 10_000..].iter().all(|s| *s == 0.0), "silence after");
        // Noise comes out as noise: about as loud.
        let level = db(rms(&out[5_000..45_000])) - db(rms(&x[5_000..45_000]));
        assert!(level.abs() < 4.0, "{pitch} {formant}: noise {level:+.1} dB");
    }
}

// ----- Both ------------------------------------------------------------------------------

/// The same sound whatever the blocks are cut into.
#[test]
fn voice_effects_do_not_depend_on_the_block_size() {
    let mut voice = ah(130.0, 0.6);
    voice.extend(ah(210.0, 0.4));
    let other: Vec<f32> = voice.iter().map(|s| s * 0.7).collect();
    let effects = [
        Effect::Vocoder(Default::default()),
        Effect::Vocoder(VocoderParams { carrier: VocoderCarrier::SuperSaw, chord: VocoderChord::Minor7, formant: 3.0, ..Default::default() }),
        Effect::Vocoder(VocoderParams { carrier: VocoderCarrier::Noise, bands: 40.0, ..Default::default() }),
        shift(7.0, -3.0),
        Effect::VoiceShift(VoiceShiftParams { pitch: -12.0, mix: 0.5, ..Default::default() }),
    ];
    for effect in effects {
        let reference = run(effect, &voice, &other, 1024);
        for block in [1, 31, 64, 128, 1000] {
            assert!(run(effect, &voice, &other, block) == reference, "{} in blocks of {block}", effect.label());
        }
    }
    let saw: Vec<f32> = (0..voice.len()).map(|i| ((i as f32 * 196.0 / SR).fract() * 2.0 - 1.0) * 0.3).collect();
    let reference = run_keyed(Default::default(), &voice, &saw, 1024);
    for block in [1, 63, 500] {
        assert!(run_keyed(Default::default(), &voice, &saw, block) == reference, "keyed, blocks of {block}");
    }
}

/// Turning their knobs at once (automation jumps) doesn't blow up or click
/// hard, and their factory presets sound.
#[test]
fn jumps_and_presets_stay_in_bounds() {
    let voice = ah(160.0, 2.0);
    for effect in [Effect::Vocoder(Default::default()), Effect::VoiceShift(Default::default())] {
        let mut fx = EffectProcessor::new(&effect, SR);
        let (mut l, mut r) = (voice.clone(), voice.clone());
        for (k, (a, b)) in l.chunks_mut(480).zip(r.chunks_mut(480)).enumerate() {
            // All but the output level (+24 dB would be loud on purpose).
            for (i, info) in effect.params().iter().enumerate().filter(|(_, info)| info.name != "Output") {
                let n = ((k * 7 + i * 3) % 11) as f32 / 10.0;
                fx.set_param(i, info.denormalize(n));
            }
            fx.process(a, b, &ctx());
        }
        assert!(clean(&l) && clean(&r), "{}", effect.label());
        let peak = l.iter().chain(&r).fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak < 8.0, "{}: {peak}", effect.label());
        for (name, preset) in effect.factory_presets() {
            assert!(name.contains(" · "), "{name}");
            for (i, info) in preset.params().iter().enumerate() {
                assert_eq!(info.clamp(preset.get(i)), preset.get(i), "{name}: {}", info.name);
            }
            let (out, _) = run(preset, &voice, &voice, 256);
            let level = db(rms(&out[10_000..]));
            let peak = db(out.iter().fold(0f32, |m, s| m.max(s.abs())));
            println!("{:24} {name:36} rms {level:6.1} dB  pico {peak:5.1} dB", effect.label());
            assert!(clean(&out) && level > -30.0 && peak < 3.0, "{name}: {level} {peak}");
        }
    }
}

/// Microseconds per 128-sample block at 48 kHz:
/// `cargo test --release -p engine voice_cpu -- --ignored --nocapture`.
#[test]
#[ignore]
fn voice_cpu() {
    let voice = ah(140.0, 4.0);
    for effect in [
        Effect::Vocoder(VocoderParams { bands: 40.0, carrier: VocoderCarrier::SuperSaw, chord: VocoderChord::Major, ..Default::default() }),
        Effect::Vocoder(VocoderParams { bands: 20.0, ..Default::default() }),
        shift(7.0, 3.0),
    ] {
        let mut fx = EffectProcessor::new(&effect, SR);
        let (mut a, mut b) = (voice.clone(), voice.clone());
        let started = std::time::Instant::now();
        for (x, y) in a.chunks_mut(128).zip(b.chunks_mut(128)) {
            fx.process(x, y, &ctx());
        }
        let us = started.elapsed().as_secs_f64() * 1e6 / (voice.len() / 128) as f64;
        println!("{:24} {us:6.1} µs por bloque de 128 ({:.1} % de un núcleo)", effect.label(), us / (128.0 / 48_000.0 * 1e6) * 100.0);
    }
}
