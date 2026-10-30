//! How long each analysis takes, on made-up audio: a sung line of 30 s and of
//! 3 min, and a mix (voice, bass, chords and drums). Run with
//! `cargo test --release -p stems --test speed -- --ignored --nocapture`
//! (with PULSE_STEMS_MODEL pointing at htdemucs.onnx, the separation too).
//!
//! Measured on a 10-core M-series Mac, before (the code at 1f7cd3b, this same
//! file run against it) and after, one right after the other (seconds; other
//! work was running, so take them as ratios):
//!
//! | Análisis                                     | Antes  | Después |
//! |----------------------------------------------|-------:|--------:|
//! | remuestreo 3 min 44,1→22,05 kHz              |  6.834 |   0.052 |
//! | remuestreo 3 min 48→44,1 kHz (stems, por canal) | 7.820 | 0.044 |
//! | pYIN voz 30 s (pitch::track)                 |  2.806 |   0.165 |
//! | pYIN voz 3 min (pitch::track)                | 16.798 |   0.976 |
//! | Audio a MIDI melodía 3 min (pitch::melody)   | 16.843 |   0.982 |
//! | Afinar voz 30 s (tune::analyse)              |  2.789 |   0.177 |
//! | Afinar voz 3 min (tune::analyse)             | 16.822 |   1.016 |
//! | Basic Pitch: cargar el modelo                |  0.035 |   0.027 |
//! | Basic Pitch voz 30 s                         |  1.529 |   0.252 |
//! | Basic Pitch mezcla 3 min                     | 11.634 |   1.349 |
//! | pYIN voz 3 min, otra vez (de memoria)        | 16.979 |   0.002 |
//! | Afinar voz 3 min tras Audio a MIDI           | 16.895 |   0.052 |
//! | tonalidad mezcla 3 min (key::chroma)         |  0.015 |   0.015 |
//! | acordes mezcla 3 min (Chromagram::new)       |  0.106 |   0.027 |
//! | stems mezcla 30 s (HTDemucs, sin remuestreo) | 22.823 | (igual) |

use std::time::Instant;

const RATE: f32 = 44_100.0;

/// A small generator, the same numbers every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn hz(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

/// A voice singing `seconds` of notes (with vibrato, glides in, breaths between).
pub fn voice(seconds: f64, rate: f32, seed: u64) -> Vec<f32> {
    let mut rng = Rng(seed);
    let mut out = vec![0f32; (seconds * rate as f64) as usize];
    let scale = [0.0, 2.0, 3.0, 5.0, 7.0, 8.0, 10.0];
    let mut t = 0.1;
    let mut phase = 0.0f64;
    while t < seconds - 0.8 {
        let length = 0.18 + 0.5 * rng.next();
        let pitch = 50.0 + scale[(rng.next() * 7.0) as usize] + 12.0 * (rng.next() * 2.0).floor();
        let start = (t * rate as f64) as usize;
        let n = (length * rate as f64) as usize;
        for i in 0..n.min(out.len() - start) {
            let s = i as f64 / rate as f64;
            let vibrato = 0.3 * (2.0 * std::f64::consts::PI * 5.5 * s).sin() * (s / 0.3).min(1.0);
            let f = hz(pitch + vibrato);
            phase += 2.0 * std::f64::consts::PI * f / rate as f64;
            let env = (s / 0.03).min(1.0) * ((length - s) / 0.04).min(1.0);
            let v: f64 = [1.0, 0.7, 0.5, 0.35, 0.2, 0.12].iter().enumerate().map(|(h, a)| a * ((h + 1) as f64 * phase).sin()).sum();
            out[start + i] += (0.12 * env * v + 0.004 * (rng.next() - 0.5)) as f32;
        }
        t += length + if rng.next() < 0.3 { 0.25 } else { 0.03 };
    }
    out
}

/// A song: the voice over a bass, chords and a drum beat at 120 BPM.
pub fn mix(seconds: f64, rate: f32) -> Vec<f32> {
    let mut out = voice(seconds, rate, 11);
    let mut rng = Rng(5);
    let roots = [57.0, 53.0, 48.0, 55.0];
    let beat = 0.5;
    for (i, x) in out.iter_mut().enumerate() {
        let s = i as f64 / rate as f64;
        let bar = (s / (4.0 * beat)) as usize;
        let root = roots[bar % 4];
        let bass = 0.15 * (2.0 * std::f64::consts::PI * hz(root - 24.0) * s).sin();
        let chord: f64 = [0.0, 3.0, 7.0].iter().map(|d| 0.04 * (2.0 * std::f64::consts::PI * hz(root + d) * s).sin()).sum();
        let into = s % beat;
        let kick = 0.4 * (-into / 0.08).exp() * (2.0 * std::f64::consts::PI * (50.0 + 80.0 * (-into / 0.02).exp()) * into).sin();
        let hat = if (s / (beat / 2.0)) as usize % 2 == 1 { 0.05 * (-(s % (beat / 2.0)) / 0.02).exp() * (rng.next() - 0.5) } else { 0.0 };
        *x += (bass + chord + kick + hat) as f32;
    }
    out
}

fn time<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let out = f();
    eprintln!("| {label:<48} | {:>9.3} s |", started.elapsed().as_secs_f64());
    out
}

#[test]
#[ignore = "a benchmark: prints how long each analysis takes"]
fn analysis_speed() {
    let short = voice(30.0, RATE, 1);
    let long = voice(180.0, RATE, 2);
    let long_48k = voice(180.0, 48_000.0, 2);
    let song = mix(180.0, RATE);
    let other = [voice(180.0, RATE, 3), voice(30.0, RATE, 4), voice(180.0, RATE, 5)];
    eprintln!("| Análisis | Tiempo |\n|---|---|");
    time("remuestreo 3 min 44,1→22,05 kHz", || stems::resample(&long, RATE, 22_050.0));
    time("remuestreo 3 min 48→44,1 kHz (stems)", || stems::resample(&long_48k, 48_000.0, RATE));
    time("pYIN voz 30 s (pitch::track)", || stems::pitch::track(&short, RATE));
    time("pYIN voz 3 min (pitch::track)", || stems::pitch::track(&long, RATE));
    // Each its own voice: the same audio again comes from memory (measured last).
    time("Audio a MIDI melodía 3 min (pitch::melody)", || stems::pitch::melody(&other[0], RATE, [0.0; 12], true));
    time("Afinar voz 30 s (tune::analyse)", || stems::tune::analyse(&other[1], RATE));
    time("Afinar voz 3 min (tune::analyse)", || stems::tune::analyse(&other[2], RATE));
    let mut transcriber = time("Basic Pitch: cargar el modelo", || stems::transcribe::Transcriber::load().unwrap());
    time("Basic Pitch voz 30 s", || transcriber.notes(&short, RATE, |_| true).unwrap());
    time("Basic Pitch mezcla 3 min", || transcriber.notes(&song, RATE, |_| true).unwrap());
    time("Basic Pitch: cargar otra vez", || stems::transcribe::Transcriber::load().unwrap());
    time("pYIN voz 3 min, otra vez (de memoria)", || stems::pitch::track(&long, RATE));
    time("Afinar voz 3 min tras Audio a MIDI", || stems::tune::analyse(&long, RATE));
    time("tonalidad mezcla 3 min (key::chroma)", || stems::key::chroma(&song, RATE));
    time("acordes mezcla 3 min (Chromagram::new)", || stems::chord::Chromagram::new(&song, RATE));
    let model = std::env::var_os("PULSE_STEMS_MODEL").map(std::path::PathBuf::from);
    if let Some(model) = model.filter(|m| m.exists()) {
        let left = mix(30.0, RATE);
        let stereo = stems::Stereo { right: left.clone(), left };
        let mut separator = time("stems: cargar HTDemucs", || stems::Separator::load(&model).unwrap());
        time("stems mezcla 30 s (HTDemucs)", || separator.separate(&stereo, |_| true).unwrap());
    }
}
