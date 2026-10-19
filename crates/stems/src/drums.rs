//! A drum model that plays on from a beat: given some bars, it writes how a
//! drummer would carry on, step by sixteenth step, with the variations and
//! fills that come at the ends of phrases. Pulse's own small transformer,
//! trained on the drum parts of the Lakh MIDI Dataset and the Groove MIDI
//! Dataset (both CC BY 4.0; see `models/DRUMS_NOTICE` and `ml/drums`). It is
//! built into the app.

use crate::environment;

const MODEL: &[u8] = include_bytes!("../models/drums.onnx");

/// The sounds the model plays, in its order: the drum machine's sixteen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Voice {
    Kick,
    Snare,
    Clap,
    Hat,
    Open,
    Ride,
    Crash,
    Rim,
    LowTom,
    MidTom,
    HighTom,
    Shaker,
    Tambourine,
    Cowbell,
    /// Congas, bongos and timbales.
    Conga,
    /// Claves and wood blocks.
    Clave,
}

pub const VOICES: usize = 16;
pub const ALL: [Voice; VOICES] = [
    Voice::Kick, Voice::Snare, Voice::Clap, Voice::Hat, Voice::Open, Voice::Ride, Voice::Crash, Voice::Rim,
    Voice::LowTom, Voice::MidTom, Voice::HighTom, Voice::Shaker, Voice::Tambourine, Voice::Cowbell, Voice::Conga, Voice::Clave,
];

/// How hard each voice plays on a step: 0 silent, 1 a ghost note, 2 normal,
/// 3 accented.
pub type Step = [u8; VOICES];

const LEVELS: usize = 4;
/// Steps in the eight-bar phrase the model counts in.
const PHRASE: usize = 128;
/// Steps it can look back at.
const CONTEXT: usize = 128;

pub struct DrumWriter {
    _environment: ort::environment::Environment,
    session: ort::session::Session,
}

impl DrumWriter {
    pub fn load() -> Result<Self, String> {
        let build = || -> ort::Result<(ort::environment::Environment, ort::session::Session)> {
            let environment = environment()?;
            let session = ort::session::Session::builder(&environment)?
                .with_auto_device(ort::session::builder::AutoDevicePolicy::PreferCPU)?
                .with_intra_threads(2)
                .unwrap_or_else(|e| e.recover())
                .commit_from_memory(MODEL)?;
            Ok((environment, session))
        };
        let (_environment, session) = build().map_err(|e| format!("No se pudo cargar el modelo de batería: {e}"))?;
        Ok(Self { _environment, session })
    }

    /// Carries `seed` (whole bars) on for `steps` more steps. The seed is the
    /// end of a phrase: what it writes starts on a phrase's first bar. `heat` is how adventurous it plays: 0 always the likeliest
    /// level, 1 as the model has it, more than 1 wilder. Only the `voices`
    /// marked true are played (the kit's sounds). `luck` picks the take.
    pub fn play_on(&mut self, seed: &[Step], steps: usize, heat: f32, voices: [bool; VOICES], luck: u64) -> Result<Vec<Step>, String> {
        if seed.is_empty() {
            return Err("El modelo de batería necesita al menos un paso para empezar".into());
        }
        let mut rng = Rng(luck.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut song: Vec<Step> = seed.to_vec();
        let offset = (PHRASE - seed.len() % PHRASE) % PHRASE;
        for _ in 0..steps {
            // The latest bars that fit, from a downbeat: it learned on windows
            // that start on one, and gets lost when they start mid-bar.
            let first = song.len().saturating_sub(CONTEXT).div_ceil(16) * 16;
            let logits = self.next(&song[first..], first + offset)?;
            let mut step = [0u8; VOICES];
            for (v, level) in step.iter_mut().enumerate() {
                if voices[v] {
                    *level = pick(&logits[v * LEVELS..(v + 1) * LEVELS], heat, &mut rng);
                }
            }
            song.push(step);
        }
        Ok(song.split_off(seed.len()))
    }

    /// The model's logits for the step after `view`, whose first step is at
    /// `at` in the phrases, VOICES × LEVELS.
    fn next(&mut self, view: &[Step], at: usize) -> Result<Vec<f32>, String> {
        let t = view.len();
        let steps: Vec<i64> = view.iter().flat_map(|s| s.iter().map(|&l| l as i64)).collect();
        // Each position predicts the step after it; its phrase position is that one's.
        let phase: Vec<i64> = (0..t).map(|i| ((at + i + 1) % PHRASE) as i64).collect();
        let steps = ort::value::Tensor::from_array(([1usize, t, VOICES], steps)).map_err(|e| e.to_string())?;
        let phase = ort::value::Tensor::from_array(([1usize, t], phase)).map_err(|e| e.to_string())?;
        let outputs = self.session.run(ort::inputs!["steps" => steps, "phase" => phase]).map_err(|e| format!("Error del modelo de batería: {e}"))?;
        let (_, data) = outputs["logits"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let last = (t - 1) * VOICES * LEVELS;
        Ok(data[last..last + VOICES * LEVELS].to_vec())
    }
}

/// A level drawn from `logits` at temperature `heat`.
fn pick(logits: &[f32], heat: f32, rng: &mut Rng) -> u8 {
    if heat <= 0.01 {
        return (0..logits.len()).max_by(|&a, &b| logits[a].total_cmp(&logits[b])).unwrap_or(0) as u8;
    }
    let top = logits.iter().copied().fold(f32::MIN, f32::max);
    let weights: Vec<f32> = logits.iter().map(|&l| ((l - top) / heat).exp()).collect();
    let mut x = rng.unit() * weights.iter().sum::<f32>();
    for (i, w) in weights.iter().enumerate() {
        x -= w;
        if x <= 0.0 {
            return i as u8;
        }
    }
    (logits.len() - 1) as u8
}

struct Rng(u64);

impl Rng {
    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four bars of house: kick on the beat, open hat off it, clap on 2 and 4.
    fn house() -> Vec<Step> {
        (0..64)
            .map(|i| {
                let mut s = [0; VOICES];
                if i % 4 == 0 {
                    s[Voice::Kick as usize] = 3;
                }
                if i % 4 == 2 {
                    s[Voice::Open as usize] = 2;
                }
                if i % 8 == 4 {
                    s[Voice::Clap as usize] = 3;
                }
                s
            })
            .collect()
    }

    #[test]
    fn it_plays_on_in_the_style_it_was_given() {
        let mut writer = DrumWriter::load().unwrap();
        let out = writer.play_on(&house(), 64, 0.0, [true; VOICES], 1).unwrap();
        assert_eq!(out.len(), 64);
        // Played safe it keeps the floor: a kick on most beats, the clap on the backbeat.
        let kicks = (0..64).step_by(4).filter(|&i| out[i][Voice::Kick as usize] > 0).count();
        assert!(kicks >= 12, "kicks on the beat: {kicks}/16");
        let claps = (4..64).step_by(8).filter(|&i| out[i][Voice::Clap as usize] > 0).count();
        assert!(claps >= 6, "claps on 2 and 4: {claps}/8");
    }

    #[test]
    fn takes_differ_and_missing_sounds_stay_silent() {
        let mut writer = DrumWriter::load().unwrap();
        let mut voices = [true; VOICES];
        voices[Voice::LowTom as usize] = false;
        let a = writer.play_on(&house(), 128, 1.0, voices, 1).unwrap();
        let b = writer.play_on(&house(), 128, 1.0, voices, 2).unwrap();
        assert_ne!(a, b);
        assert!(a.iter().chain(&b).all(|s| s[Voice::LowTom as usize] == 0));
    }
}
