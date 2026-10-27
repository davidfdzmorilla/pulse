//! Beatbox to drums: «pum», «pf» (or «ka») and «ts» into the microphone play
//! the kick, the snare and the closed hat (a Drum Machine's notes 36, 38 and
//! 42). A hit is heard where the level jumps; which sound it is, from how its
//! first 16 ms spread over five bands, compared with what each sound is like:
//! built in, or learned from the user's own hits.

use engine::model::drum_note;
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;

/// The bands a hit is described by: their edges (Hz).
pub const BANDS: usize = 5;
const EDGES: [f32; BANDS - 1] = [200.0, 600.0, 2_000.0, 6_000.0];
/// How a hit spreads over the bands: each band's share of it, in dB.
pub type Features = [f32; BANDS];

/// What is heard of a hit to tell what it is: 16 ms from just before it.
const WINDOW_SECONDS: f32 = 0.016;
/// Input between looks at the level: 128 samples at 48 kHz.
const HOP_SECONDS: f32 = 128.0 / 48_000.0;
/// Below this (dBFS, the peak over a hop), silence.
const GATE_DB: f32 = -45.0;
/// A hit: the level this much (dB) over the lowest of the last
/// `FLOOR_SECONDS`, at least `APART` seconds after the last hit.
const RISE_DB: f32 = 10.0;
const FLOOR_SECONDS: f32 = 0.02;
const APART: f32 = 0.07;
/// Hits of each sound kept to learn from (the newest), and how many teaching
/// asks for.
pub const KEPT: usize = 8;
pub const TEACH: usize = 4;

/// The sounds the voice plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Drum {
    Kick,
    Snare,
    Hat,
}

impl Drum {
    pub const ALL: [Drum; 3] = [Drum::Kick, Drum::Snare, Drum::Hat];

    pub fn note(self) -> u8 {
        match self {
            Drum::Kick => drum_note::KICK,
            Drum::Snare => drum_note::SNARE,
            Drum::Hat => drum_note::CLOSED_HAT,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Drum::Kick => "Bombo",
            Drum::Snare => "Caja",
            Drum::Hat => "Charles",
        }
    }

    /// What the sound to make for it is.
    pub fn sound(self) -> &'static str {
        match self {
            Drum::Kick => "«pum»",
            Drum::Snare => "«pf» o «ka»",
            Drum::Hat => "«ts»",
        }
    }

    /// What it is like, untaught: a «pum» low, a «pf» or «ka» in the middle
    /// and highs, a «ts» high.
    fn built_in(self) -> Features {
        match self {
            Drum::Kick => [-1.0, -10.0, -22.0, -32.0, -40.0],
            Drum::Snare => [-14.0, -11.0, -5.0, -4.0, -9.0],
            Drum::Hat => [-38.0, -32.0, -20.0, -7.0, -1.5],
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The hits the user taught, of each sound (saved in the settings).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Examples(pub Vec<(Drum, Features)>);

impl Examples {
    pub fn count(&self, drum: Drum) -> usize {
        self.0.iter().filter(|(d, _)| *d == drum).count()
    }

    /// Adds a hit of `drum`, forgetting its oldest past [`KEPT`].
    pub fn add(&mut self, drum: Drum, features: Features) {
        self.0.push((drum, features));
        if self.count(drum) > KEPT {
            let oldest = self.0.iter().position(|(d, _)| *d == drum).unwrap();
            self.0.remove(oldest);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Forgets what was taught of `drum`.
    pub fn forget(&mut self, drum: Drum) {
        self.0.retain(|(d, _)| *d != drum);
    }

    /// What each sound is like: the average of its hits taught, or the built-in.
    fn centres(&self) -> [Features; 3] {
        Drum::ALL.map(|drum| {
            let taught: Vec<&Features> = self.0.iter().filter(|(d, _)| *d == drum).map(|(_, f)| f).collect();
            if taught.is_empty() {
                return drum.built_in();
            }
            std::array::from_fn(|b| taught.iter().map(|f| f[b]).sum::<f32>() / taught.len() as f32)
        })
    }
}

/// A hit heard: where it starts (samples into all the input), the sound, how
/// hard and what it was like.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub at: u64,
    pub drum: Drum,
    pub velocity: f32,
    pub features: Features,
}

/// Hears hits in the input as it comes, however it is cut.
pub struct Beatbox {
    rate: f32,
    hop: usize,
    window: usize,
    fft: Arc<dyn Fft<f32>>,
    /// The newest input, and how much came before it.
    recent: Vec<f32>,
    before: u64,
    since: usize,
    /// The level of the last hops (dB).
    levels: VecDeque<f32>,
    floor_hops: usize,
    /// A hit being heard: where its window starts (in all the input) and its
    /// loudest hop so far.
    pending: Option<(u64, f32)>,
    /// Seconds since the last hit.
    after: f32,
    taught: Examples,
    centres: [Features; 3],
}

impl Beatbox {
    pub fn new(rate: f32) -> Self {
        let hop = ((HOP_SECONDS * rate) as usize).max(1);
        let window = (WINDOW_SECONDS * rate) as usize;
        let fft = FftPlanner::new().plan_fft_forward(window.next_power_of_two());
        let floor_hops = ((FLOOR_SECONDS * rate) as usize / hop).max(1);
        let taught = Examples::default();
        Self { rate, hop, window, fft, recent: Vec::new(), before: 0, since: 0, levels: VecDeque::new(), floor_hops, pending: None, after: APART, centres: taught.centres(), taught }
    }

    /// Learns from `examples` (if they changed).
    pub fn learn(&mut self, examples: &Examples) {
        if *examples != self.taught {
            self.taught = examples.clone();
            self.centres = self.taught.centres();
        }
    }

    /// Takes new input: the hits heard in it, in order.
    pub fn hear(&mut self, input: &[f32]) -> Vec<Hit> {
        let mut hits = Vec::new();
        let mut rest = input;
        while !rest.is_empty() {
            let take = (self.hop - self.since).min(rest.len());
            self.recent.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            self.since += take;
            if self.since < self.hop {
                break;
            }
            self.since = 0;
            if let Some(hit) = self.look() {
                hits.push(hit);
            }
            // Enough for a window starting a hop back.
            let keep = 2 * self.window + 2 * self.hop;
            if self.recent.len() > 2 * keep {
                let drop = self.recent.len() - keep;
                self.recent.drain(..drop);
                self.before += drop as u64;
            }
        }
        hits
    }

    /// One hop more: a hit, once one is heard to the end of its window.
    fn look(&mut self) -> Option<Hit> {
        let end = self.before + self.recent.len() as u64;
        let last = &self.recent[self.recent.len() - self.hop..];
        // The peak, not the RMS: over a hop, a low note's RMS swings with its
        // phase (and would seem to hit again in a kick's tail).
        let level = 20.0 * last.iter().fold(0f32, |m, x| m.max(x.abs())).max(1e-6).log10();
        self.after += self.hop as f32 / self.rate;
        // (Nothing heard yet: silence before.)
        let floor = self.levels.iter().cloned().reduce(f32::min).unwrap_or(-120.0);
        self.levels.push_back(level);
        if self.levels.len() > self.floor_hops {
            self.levels.pop_front();
        }
        if let Some((start, peak)) = self.pending.as_mut() {
            *peak = peak.max(level);
            if end - *start < self.window as u64 {
                return None;
            }
            let (start, peak) = (*start, *peak);
            self.pending = None;
            let from = (start - self.before) as usize;
            let frame = &self.recent[from..from + self.window];
            let features = self.features(frame);
            let drum = self.classify(&features);
            let velocity = ((peak - GATE_DB) / 40.0).clamp(0.3, 1.0);
            // Where it starts: the first sample a third as loud as its loudest.
            let loudest = frame.iter().fold(0f32, |m, x| m.max(x.abs()));
            let onset = frame.iter().position(|x| x.abs() >= loudest / 3.0).unwrap_or(0);
            return Some(Hit { at: start + onset as u64, drum, velocity, features });
        }
        if level > GATE_DB && level - floor >= RISE_DB && self.after >= APART {
            self.after = 0.0;
            // From a hop back: the hit's very start.
            let start = end.saturating_sub(2 * self.hop as u64).max(self.before);
            self.pending = Some((start, level));
        }
        None
    }

    /// How `frame` spreads over the bands.
    pub fn features(&self, frame: &[f32]) -> Features {
        let size = self.fft.len();
        let n = frame.len();
        let mut buffer: Vec<Complex<f32>> = (0..size)
            .map(|i| {
                let x = if i < n { frame[i] * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos()) } else { 0.0 };
                Complex::new(x, 0.0)
            })
            .collect();
        self.fft.process(&mut buffer);
        let mut bands = [0f32; BANDS];
        for (k, c) in buffer[1..size / 2].iter().enumerate() {
            let hz = (k + 1) as f32 * self.rate / size as f32;
            let band = EDGES.iter().take_while(|&&e| hz >= e).count();
            bands[band] += c.norm_sqr();
        }
        let total = bands.iter().sum::<f32>().max(1e-20);
        bands.map(|b| 10.0 * (b / total).max(1e-6).log10())
    }

    /// The sound `features` are nearest.
    fn classify(&self, features: &Features) -> Drum {
        let distance = |c: &Features| c.iter().zip(features).map(|(a, b)| (a - b).powi(2)).sum::<f32>();
        Drum::ALL.into_iter().min_by(|a, b| distance(&self.centres[a.index()]).total_cmp(&distance(&self.centres[b.index()]))).unwrap()
    }
}

/// The hits in a whole recording, as `examples` taught. It is heard as if its
/// loudest moment were at full scale, so how loud it was recorded doesn't
/// matter: the loudest hit plays hardest.
pub fn hits_in(mono: &[f32], rate: f32, examples: &Examples) -> Vec<Hit> {
    let peak = mono.iter().fold(0f32, |m, x| m.max(x.abs()));
    if peak < 1e-4 {
        return Vec::new();
    }
    let mut beatbox = Beatbox::new(rate);
    beatbox.learn(examples);
    let gain = 1.0 / peak;
    let mut hits = Vec::new();
    let mut chunk = Vec::with_capacity(4096);
    for part in mono.chunks(4096) {
        chunk.clear();
        chunk.extend(part.iter().map(|x| x * gain));
        hits.extend(beatbox.hear(&chunk));
    }
    // Silence after, for a hit at the very end to be heard to the end of its window.
    hits.extend(beatbox.hear(&vec![0.0; beatbox.window + 2 * beatbox.hop]));
    hits
}

#[cfg(test)]
pub mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// Noise, the same every time.
    fn noise(seed: &mut u32) -> f32 {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*seed >> 8) as f32 / (1 << 23) as f32 - 1.0
    }

    /// A «pum», a «pf» and a «ts» as made with the mouth (roughly), `gain` loud.
    pub fn mouth(drum: Drum, gain: f32, seed: u32) -> Vec<f32> {
        let mut seed = seed;
        let n = (0.12 * RATE) as usize;
        let (mut low, mut low2, mut phase) = (0f32, 0f32, 0f32);
        (0..n)
            .map(|i| {
                let t = i as f32 / RATE;
                let x = noise(&mut seed);
                // One-pole low passes of the noise.
                low += (x - low) * 0.05;
                low2 += (x - low2) * 0.5;
                let s = match drum {
                    Drum::Kick => {
                        phase += (55.0 + 70.0 * (-t / 0.02).exp()) / RATE;
                        (std::f32::consts::TAU * phase).sin() * (-t / 0.06).exp() + 0.05 * low * (-t / 0.005).exp()
                    }
                    Drum::Snare => {
                        phase += 180.0 / RATE;
                        (1.2 * (low2 - low) + 0.3 * (x - low2)) * (-t / 0.07).exp() + 0.15 * (std::f32::consts::TAU * phase).sin() * (-t / 0.03).exp()
                    }
                    Drum::Hat => (x - low2) * (-t / 0.04).exp(),
                };
                gain * s
            })
            .collect()
    }

    /// `hits` (sound, at seconds), in silence, `seconds` long.
    pub fn pattern(hits: &[(Drum, f32)], seconds: f32) -> Vec<f32> {
        let mut out = vec![0f32; (seconds * RATE) as usize];
        for (k, &(drum, at)) in hits.iter().enumerate() {
            let start = (at * RATE) as usize;
            for (i, s) in mouth(drum, 0.3, k as u32 + 7).into_iter().enumerate() {
                if let Some(o) = out.get_mut(start + i) {
                    *o += s;
                }
            }
        }
        out
    }

    fn heard(beatbox: &mut Beatbox, audio: &[f32], chunk: usize) -> Vec<Hit> {
        audio.chunks(chunk).flat_map(|c| beatbox.hear(c)).collect()
    }

    /// A beat (kick, hat, snare, hat… an eighth apart at 120) is heard hit by
    /// hit, each as what it is, however the input is cut.
    #[test]
    fn a_beatboxed_beat_plays_kick_snare_and_hat() {
        use Drum::*;
        let order = [Kick, Hat, Snare, Hat, Kick, Kick, Snare, Hat];
        let hits: Vec<(Drum, f32)> = order.iter().enumerate().map(|(i, &d)| (d, 0.1 + 0.25 * i as f32)).collect();
        let audio = pattern(&hits, 2.3);
        for chunk in [64, 256, 1024] {
            let heard = heard(&mut Beatbox::new(RATE), &audio, chunk);
            for h in &heard {
                println!("{:?} {:.2} {:?}", h.drum, h.velocity, h.features.map(|f| f.round()));
            }
            assert_eq!(heard.iter().map(|h| h.drum).collect::<Vec<_>>(), order, "in blocks of {chunk}");
            assert!(heard.iter().all(|h| h.velocity > 0.5));
        }
    }

    /// In a recording, each hit where it starts (to the millisecond), however
    /// softly it was recorded.
    #[test]
    fn a_recording_has_its_hits_where_they_start() {
        use Drum::*;
        let hits = [(Kick, 0.2), (Hat, 0.45), (Snare, 0.7), (Hat, 0.95)];
        for gain in [1.0, 0.05] {
            let audio: Vec<f32> = pattern(&hits, 1.2).iter().map(|s| s * gain).collect();
            let heard = hits_in(&audio, RATE, &Examples::default());
            let found: Vec<(Drum, f32)> = heard.iter().map(|h| (h.drum, h.at as f32 / RATE)).collect();
            println!("{found:?}");
            assert_eq!(found.len(), hits.len());
            for ((drum, at), (want, when)) in found.iter().zip(hits) {
                assert_eq!(*drum, want);
                assert!((at - when).abs() < 0.002, "{at} for {when}");
            }
            assert!(heard[0].velocity > 0.9, "the loudest hit plays hard, however it was recorded");
        }
    }

    /// A hit plays some 20 ms after it starts.
    #[test]
    fn a_hit_plays_soon() {
        let audio = pattern(&[(Drum::Kick, 0.1)], 0.3);
        let mut beatbox = Beatbox::new(RATE);
        let at = audio.chunks(64).position(|c| !beatbox.hear(c).is_empty()).unwrap();
        let ms = ((at + 1) * 64) as f32 / RATE * 1000.0 - 100.0;
        println!("{ms:.1} ms");
        assert!(ms <= 22.0, "{ms:.1} ms");
    }

    /// A softer hit plays softer.
    #[test]
    fn softer_hits_are_softer() {
        let velocity = |gain: f32| {
            let mut audio = vec![0f32; 4800];
            audio.extend(mouth(Drum::Snare, gain, 3));
            heard(&mut Beatbox::new(RATE), &audio, 256)[0].velocity
        };
        let (loud, soft) = (velocity(0.3), velocity(0.03));
        println!("{loud:.2} {soft:.2}");
        assert!(loud > 0.8 && soft < loud - 0.3, "{loud:.2} {soft:.2}");
    }

    /// Quiet sounds, and a held note, play nothing.
    #[test]
    fn quiet_and_held_sounds_are_not_hits() {
        let quiet: Vec<f32> = pattern(&[(Drum::Kick, 0.1), (Drum::Hat, 0.4)], 0.6).iter().map(|s| s * 0.003).collect();
        assert!(heard(&mut Beatbox::new(RATE), &quiet, 256).is_empty());
        // A sung note: one hit where it starts, none while it holds.
        let note: Vec<f32> = (0..RATE as usize).map(|i| if i < 4800 { 0.0 } else { 0.2 * (std::f32::consts::TAU * 220.0 * i as f32 / RATE).sin() }).collect();
        assert!(heard(&mut Beatbox::new(RATE), &note, 256).len() <= 1);
    }

    /// A sound of the user's own (a low «ka», heard as a snare untaught),
    /// taught as the kick, plays the kick; the rest stay as they were.
    #[test]
    fn hits_taught_are_what_they_were_taught_as() {
        let ka = |seed: u32| -> Vec<f32> {
            let mut seed = seed;
            let mut low = 0f32;
            let mut phase = 0f32;
            (0..(0.1 * RATE) as usize)
                .map(|i| {
                    let t = i as f32 / RATE;
                    let x = noise(&mut seed);
                    low += (x - low) * 0.15;
                    phase += 400.0 / RATE;
                    0.3 * (0.5 * low + 0.6 * (std::f32::consts::TAU * phase).sin()) * (-t / 0.05).exp()
                })
                .collect()
        };
        let mut audio = vec![0f32; (0.1 * RATE) as usize];
        audio.extend(ka(1));
        audio.extend(vec![0f32; (0.2 * RATE) as usize]);
        let mut beatbox = Beatbox::new(RATE);
        let untaught = heard(&mut beatbox, &audio, 256);
        assert_eq!(untaught.len(), 1);
        assert_ne!(untaught[0].drum, Drum::Kick, "{:?}", untaught[0].features);
        // Taught with other hits of it.
        let mut examples = Examples::default();
        for seed in 10..10 + TEACH as u32 {
            let mut one = vec![0f32; 4800];
            one.extend(ka(seed));
            let hit = heard(&mut Beatbox::new(RATE), &one, 256)[0];
            examples.add(Drum::Kick, hit.features);
        }
        let mut beatbox = Beatbox::new(RATE);
        beatbox.learn(&examples);
        assert_eq!(heard(&mut beatbox, &audio, 256)[0].drum, Drum::Kick);
        let others = pattern(&[(Drum::Snare, 0.1), (Drum::Hat, 0.4)], 0.7);
        assert_eq!(heard(&mut beatbox, &others, 256).iter().map(|h| h.drum).collect::<Vec<_>>(), vec![Drum::Snare, Drum::Hat]);
    }

    #[test]
    fn examples_keep_the_newest_of_each_sound() {
        let mut examples = Examples::default();
        for i in 0..KEPT + 3 {
            examples.add(Drum::Hat, [i as f32; BANDS]);
        }
        examples.add(Drum::Kick, [0.0; BANDS]);
        assert_eq!(examples.count(Drum::Hat), KEPT);
        assert_eq!(examples.0[0].1[0], 3.0, "the oldest went");
        examples.forget(Drum::Hat);
        assert_eq!(examples.0.len(), 1);
    }
}
