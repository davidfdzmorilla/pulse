//! Melodies over chords: a hook, a singable topline, a trance lead or a call
//! and response for the chords a clip plays. One more part of «Acompañar»
//! (see `accompany`): a short motif (its rhythm and its shape) repeats over
//! the clip and bends to each chord, the chord's notes on the strong beats,
//! the scale's in between, and each phrase ends on a chord note.

use crate::accompany::Dice;
use engine::meter::TimeSig;
use engine::model::Note;
use stems::chord::Chord;
use stems::key::Key;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// A one-bar syncopated riff, over and over (house, techno).
    Hook,
    /// A two-bar phrase of longer notes, singable (pop).
    Topline,
    /// Sixteenths leaping between the chord's notes (trance).
    Trance,
    /// A bar asks, the next answers with the same rhythm.
    CallResponse,
}

impl Style {
    pub const ALL: [Style; 4] = [Style::Hook, Style::Topline, Style::Trance, Style::CallResponse];

    pub fn label(self) -> &'static str {
        match self {
            Style::Hook => "Gancho",
            Style::Topline => "Cantable",
            Style::Trance => "Lead de trance",
            Style::CallResponse => "Pregunta y respuesta",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Style::Hook => "Un riff corto y sincopado de un compás que se repite, como los de house y techno",
            Style::Topline => "Una frase de dos compases con notas largas, que se puede cantar, como en el pop",
            Style::Trance => "Semicorcheas que saltan entre las notas del acorde, como un lead de trance",
            Style::CallResponse => "Un compás pregunta y el siguiente responde con el mismo ritmo y cierra en la nota del acorde",
        }
    }

    /// Bars the motif lasts.
    fn bars(self) -> usize {
        match self {
            Style::Hook | Style::Trance => 1,
            Style::Topline | Style::CallResponse => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density {
    Sparse,
    Normal,
    Busy,
}

impl Density {
    pub const ALL: [Density; 3] = [Density::Sparse, Density::Normal, Density::Busy];

    pub fn label(self) -> &'static str {
        match self {
            Density::Sparse => "Pocas notas",
            Density::Normal => "Normal",
            Density::Busy => "Muchas notas",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Register {
    Low,
    Mid,
    High,
}

impl Register {
    pub const ALL: [Register; 3] = [Register::Low, Register::Mid, Register::High];

    pub fn label(self) -> &'static str {
        match self {
            Register::Low => "Grave",
            Register::Mid => "Medio",
            Register::High => "Agudo",
        }
    }

    /// The lowest and highest notes: about an octave and a half, as a voice sings.
    pub fn range(self) -> (u8, u8) {
        match self {
            // C3..G4, A3..E5, G4..D6.
            Register::Low => (48, 67),
            Register::Mid => (57, 76),
            Register::High => (67, 86),
        }
    }
}

/// What a melody is: its style, how many notes, how high.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Melody {
    pub style: Style,
    pub density: Density,
    pub register: Register,
}

impl Melody {
    pub fn new(style: Style) -> Self {
        Self { style, density: Density::Normal, register: Register::Mid }
    }
}

/// How a note of the motif ends a phrase: on a chord note other than the root
/// (open, a question), or on the root or the tonic (closed, an answer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cadence {
    Open,
    Closed,
}

/// A note of the motif: where it starts and how long, in sixteenths from the
/// phrase's start, and how far above the melody's centre, in scale steps.
#[derive(Clone, Copy, Debug)]
struct Step {
    at: usize,
    length: usize,
    height: i32,
    /// A weak note that goes for a chord note anyway (arpeggiated leads).
    chordy: bool,
    /// The last note of a question or an answer.
    cadence: Option<Cadence>,
}

/// The key that best fits `chords`: each chord's notes by how long they sound,
/// its root twice.
pub fn key_of(chords: &[(f64, f64, Chord)]) -> Key {
    Key::detect(&weights(chords)).unwrap_or(Key { tonic: 0, minor: false })
}

fn weights(chords: &[(f64, f64, Chord)]) -> [f64; 12] {
    let mut weights = [0.0; 12];
    for (from, to, chord) in chords {
        for (i, class) in chord.classes().into_iter().enumerate() {
            weights[class as usize] += (to - from) * if i == 0 { 2.0 } else { 1.0 };
        }
    }
    weights
}

/// The key for a melody over `chords`: the project's (`song`) if they are in
/// it about as well as in any other, else theirs.
pub fn key_for(chords: &[(f64, f64, Chord)], song: Option<Key>) -> Key {
    let weights = weights(chords);
    let best = key_of(chords);
    match song {
        Some(key) if key.fit(&weights) >= best.fit(&weights) - 0.1 => key,
        _ => best,
    }
}

/// The scale over `chord`: the key's, with the chord's own notes in place of
/// the scale's a semitone from them (over E in A minor, G# for G).
fn chord_scale(key: Key, chord: &Chord) -> [bool; 12] {
    let mut scale = key.scale();
    let tones = chord.classes();
    for &c in &tones {
        if !scale[c as usize] {
            scale[c as usize] = true;
            for n in [(c + 1) % 12, (c + 11) % 12] {
                if !tones.contains(&n) {
                    scale[n as usize] = false;
                }
            }
        }
    }
    scale
}

/// `pitch` moved `steps` notes of `scale` up or down (from a note of it).
fn walk(scale: &[bool; 12], pitch: i32, steps: i32) -> i32 {
    let mut p = pitch;
    for _ in 0..steps.abs() {
        p += steps.signum();
        while !scale[p.rem_euclid(12) as usize] {
            p += steps.signum();
        }
    }
    p
}

/// Whether two pitch classes are a semitone apart.
fn semitone(a: u8, b: u8) -> bool {
    matches!((a + 12 - b) % 12, 1 | 11)
}

/// The note of `classes` inside `low..=high` nearest `target`, and nearer
/// `previous` on a tie.
fn nearest(classes: &[u8], target: i32, previous: Option<i32>, (low, high): (u8, u8)) -> i32 {
    (low as i32..=high as i32)
        .filter(|p| classes.contains(&(p.rem_euclid(12) as u8)))
        .min_by(|a, b| {
            let cost = |p: i32| (p - target).abs() as f64 + 0.1 * previous.map_or(0.0, |q| (p - q).abs() as f64);
            cost(*a).total_cmp(&cost(*b))
        })
        .unwrap_or(target.clamp(low as i32, high as i32))
}

/// The motif's onsets: `count` of the sixteenths in `window` (from the
/// phrase's start), one in each `count`th of it (so they spread over the
/// phrase), picked by how much each is felt in the style (beats, off-beat
/// eighths, sixteenths); `first` makes the first one sound.
fn onsets(dice: &mut Dice, window: usize, bar: usize, beats: &[usize], weights: [f64; 3], count: usize, first: bool) -> Vec<usize> {
    let count = count.clamp(1, window);
    let mut chosen: Vec<usize> = (0..count)
        .map(|i| {
            let (from, to) = (i * window / count, (i + 1) * window / count);
            // Weighted picking: the highest roll^(1/weight).
            (from..to.max(from + 1))
                .map(|s| {
                    let level = if beats.contains(&(s % bar)) {
                        0
                    } else if s % 2 == 0 {
                        1
                    } else {
                        2
                    };
                    let score = if first && s == 0 { 2.0 } else { dice.roll().powf(1.0 / weights[level].max(1e-6)) };
                    (score, s)
                })
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .map_or(from, |s| s.1)
        })
        .collect();
    chosen.dedup();
    // At least one note on a beat, so the chord is heard under it.
    if !chosen.iter().any(|s| beats.contains(&(s % bar))) {
        chosen.insert(0, 0);
    }
    chosen
}

/// The motif of `melody` for a bar of `bar` sixteenths whose pulses start at
/// `beats`, and the answer's (the same rhythm, its tail lower, ending closed).
fn motif(melody: Melody, dice: &mut Dice, bar: usize, beats: &[usize]) -> (Vec<Step>, Vec<Step>) {
    let phrase = bar * melody.style.bars();
    // Notes per bar of 16 sixteenths, in a bar of this meter.
    let per_bar = |n: [usize; 3]| -> usize {
        let n = n[melody.density as usize] as f64 * bar as f64 / 16.0;
        (n.round() as usize).max(2)
    };
    let mut steps: Vec<Step> = Vec::new();
    let note = |at, length, height| Step { at, length, height, chordy: false, cadence: None };
    match melody.style {
        Style::Hook => {
            // Most hooks start on the one; some come in just after it.
            let first = dice.chance(0.6);
            let at = onsets(dice, phrase, bar, beats, [1.0, 1.5, 0.5], per_bar([3, 5, 7]), first);
            // A riff stays near a note: repeats, steps and a leap now and then.
            let mut height = *dice.pick(&[0, 2, 4]);
            for (i, &s) in at.iter().enumerate() {
                let gap = at.get(i + 1).map_or(phrase - s, |n| n - s);
                let length = gap.min(*dice.pick(&[1, 2, 2, 3]));
                steps.push(note(s, length, height));
                height = (height + *dice.pick(&[0, 0, 1, -1, 2, -2, 3, -3])).clamp(-3, 6);
            }
        }
        Style::Topline => {
            // Room to breathe at the end of the phrase.
            let window = phrase - 2;
            let at = onsets(dice, window, bar, beats, [1.6, 0.8, 0.1], per_bar([2, 4, 5]) * 2, true);
            // An arch: up to a peak and back down, by steps.
            let n = at.len();
            let peak = (n as f64 * (0.35 + 0.3 * dice.roll())) as usize;
            let top = *dice.pick(&[3, 4, 5]);
            let start = *dice.pick(&[-2, -1, 0]);
            for (i, &s) in at.iter().enumerate() {
                let gap = at.get(i + 1).map_or(window - s, |n| n - s);
                let curve = if i <= peak {
                    start as f64 + (top - start) as f64 * i as f64 / peak.max(1) as f64
                } else {
                    top as f64 - (top + 1) as f64 * (i - peak) as f64 / (n - peak).max(1) as f64
                };
                let height = curve.round() as i32 + *dice.pick(&[0, 0, 0, 1, -1]);
                steps.push(note(s, gap, height));
            }
        }
        Style::Trance => {
            // Every eighth, and sixteenths as busy as asked.
            let extra = [0.0, 0.35, 1.0][melody.density as usize];
            let mut at = Vec::new();
            for s in 0..phrase {
                if s % 2 == 0 || dice.chance(extra) {
                    at.push(s);
                }
            }
            const SHAPES: [&[i32]; 5] = [&[0, 2, 4, 7], &[0, 4, 2, 7], &[7, 4, 2, 0], &[0, 2, 4, 2], &[0, 4, 7, 4]];
            let shape = *dice.pick(&SHAPES);
            // The second half of the bar a step higher, now and then.
            let lift = *dice.pick(&[0, 0, 2, -2]);
            for (i, &s) in at.iter().enumerate() {
                let gap = at.get(i + 1).map_or(phrase - s, |n| n - s);
                let height = shape[i % shape.len()] - 2 + if s >= phrase / 2 { lift } else { 0 };
                steps.push(Step { chordy: dice.chance(0.8), ..note(s, gap, height) });
            }
        }
        Style::CallResponse => {
            // The call in the first three quarters of the bar; the answer, the
            // same rhythm a bar later.
            let window = (bar * 3 / 4).max(2);
            let at = onsets(dice, window, bar, beats, [1.0, 1.4, 0.5], per_bar([3, 4, 6]), true);
            let mut height = *dice.pick(&[-1, 0, 1]);
            let mut call = Vec::new();
            for (i, &s) in at.iter().enumerate() {
                let gap = at.get(i + 1).map_or(window - s, |n| n - s);
                let length = if i + 1 == at.len() { gap } else { gap.min(*dice.pick(&[2, 2, 3, 4])) };
                call.push(note(s, length, height));
                // The call rises; the answer comes back down.
                height = (height + *dice.pick(&[1, 1, 2, 0, -1])).clamp(-2, 6);
            }
            let first = call[0].height;
            let answer: Vec<Step> = call.iter().map(|c| Step { at: c.at + bar, height: first - (c.height - first) / 2, ..*c }).collect();
            if let Some(last) = call.last_mut() {
                last.cadence = Some(Cadence::Open);
            }
            steps = call;
            steps.extend(answer);
        }
    }
    let mut question = steps.clone();
    let mut answer = steps;
    // The answer's last third settles lower.
    let settle = *dice.pick(&[1, 2]);
    let tail = answer.len() * 2 / 3;
    let n = answer.len();
    for step in answer.iter_mut().skip(tail).take(n.saturating_sub(tail + 1)) {
        step.height -= settle;
    }
    if let Some(last) = question.last_mut() {
        last.cadence = Some(Cadence::Open);
    }
    if let Some(last) = answer.last_mut() {
        last.cadence = Some(Cadence::Closed);
    }
    (question, answer)
}

/// The melody over `chords` (start and end beats, and the chord) in a clip
/// `length` beats long in bars of `sig`, in `key` (the chords' own if None);
/// `seed` picks the take.
pub fn generate(melody: Melody, chords: &[(f64, f64, Chord)], length: f64, seed: u64, sig: TimeSig, key: Option<Key>) -> Vec<Note> {
    if chords.is_empty() || length <= 0.0 {
        return Vec::new();
    }
    let key = key.unwrap_or_else(|| key_of(chords));
    let bar = ((sig.bar() * 4.0).round() as usize).max(2);
    let beats: Vec<usize> = sig.pulses().iter().map(|p| (p.0 * 4.0).round() as usize).collect();
    let mut dice = Dice::new(seed);
    let (question, answer) = motif(melody, &mut dice, bar, &beats);
    let range = melody.register.range();
    // The melody's centre, a note of the scale a little under the middle.
    let scale = key.scale();
    let mut centre = (range.0 as i32 + range.1 as i32) / 2 - 2;
    while !scale[centre.rem_euclid(12) as usize] {
        centre += 1;
    }
    let phrase = (bar * melody.style.bars()) as f64 * 0.25;
    let phrases = (length / phrase - 1e-9).ceil().max(1.0) as usize;
    let gate = match melody.style {
        Style::Hook => 0.85,
        Style::Topline => 0.95,
        Style::Trance => 0.8,
        Style::CallResponse => 0.9,
    };
    let chord_at = |beat: f64| chords.iter().find(|(a, b, _)| beat >= a - 1e-9 && beat < b - 1e-9).copied();
    let on_beat = |beat: f64| {
        let s = (beat * 4.0).round() as usize;
        (beat * 4.0 - s as f64).abs() < 1e-6 && beats.contains(&(s % bar))
    };
    // Whether a beat other than its first falls inside start..end.
    let crosses = |start: f64, end: f64| {
        let mut s = (start * 4.0).floor() as usize + 1;
        while (s as f64) * 0.25 < end - 1e-9 {
            if beats.contains(&(s % bar)) {
                return true;
            }
            s += 1;
        }
        false
    };
    let mut notes: Vec<Note> = Vec::new();
    let mut previous: Option<i32> = None;
    for k in 0..phrases {
        // Questions and answers in turn; the clip ends on an answer.
        let answering = k % 2 == 1 || (k + 1 == phrases && (k + 1) as f64 * phrase <= length + 1e-9);
        let motif = if answering { &answer } else { &question };
        let base = k as f64 * phrase;
        for step in motif {
            let start = base + step.at as f64 * 0.25;
            if start >= length - 1e-9 {
                break;
            }
            let Some((_, chord_end, chord)) = chord_at(start) else { continue };
            let mut end = (start + step.length as f64 * 0.25).min(length);
            let strong_start = on_beat(start);
            let target = walk(&scale, centre, step.height);
            // The note over `harmony`, on a strong beat or not.
            let choose = |harmony: Chord, strong: bool| -> i32 {
                let tones = harmony.classes();
                let local = chord_scale(key, &harmony);
                // The scale note the motif asks for, over this chord.
                let mut target = target;
                if !local[target.rem_euclid(12) as usize] {
                    target += if local[(target + 1).rem_euclid(12) as usize] { 1 } else { -1 };
                }
                if strong {
                    let choices: Vec<u8> = match step.cadence {
                        // The tonic if the chord has it, else its root.
                        Some(Cadence::Closed) => vec![if tones.contains(&key.tonic) { key.tonic } else { harmony.root }],
                        Some(Cadence::Open) if tones.len() > 1 => tones[1..].to_vec(),
                        _ => tones.clone(),
                    };
                    return nearest(&choices, target, previous, range);
                }
                // Off the beat: the scale's note, if it moves by step (a
                // passing or neighbour note); else a chord note.
                let mut p = target;
                while p > range.1 as i32 {
                    p -= 12;
                }
                while p < range.0 as i32 {
                    p += 12;
                }
                let in_chord = tones.contains(&(p.rem_euclid(12) as u8));
                let stepwise = previous.is_none_or(|q| (p - q).abs() <= 2);
                if in_chord || (stepwise && !step.chordy) { p } else { nearest(&tones, target, previous, range) }
            };
            let next = chord_at(chord_end).map(|c| c.2).filter(|_| end > chord_end + 1e-9);
            let pitch = match next {
                // Off the beat into the next chord: it anticipates that one,
                // unless it rubs a semitone against the chord under its start.
                Some(n) if !strong_start && chord.classes().iter().all(|&c| !semitone(choose(n, true).rem_euclid(12) as u8, c)) => choose(n, true),
                // On the beat, it holds over the next chord only if that one has it too.
                Some(n) => {
                    let p = choose(chord, true);
                    if !n.classes().contains(&(p.rem_euclid(12) as u8)) {
                        end = chord_end;
                    }
                    if end > chord_end + 1e-9 || strong_start || step.cadence.is_some() || crosses(start, end) { p } else { choose(chord, false) }
                }
                None => {
                    end = end.min(chord_end);
                    choose(chord, strong_start || step.cadence.is_some() || crosses(start, end))
                }
            };
            previous = Some(pitch);
            let velocity = match (on_beat(start), (start * 4.0).round() as usize % bar == 0) {
                (true, true) => 0.9,
                (true, false) => 0.82,
                _ => if melody.style == Style::Trance { 0.66 } else { 0.74 },
            };
            let length = ((end - start) * gate).max(0.05);
            notes.push(Note { start, length, pitch: pitch as u8, velocity, chance: 1.0 });
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use stems::chord::Quality;

    /// `chords` (root and quality), `beats` each.
    fn progression(chords: &[(u8, Quality)], beats: f64) -> Vec<(f64, f64, Chord)> {
        chords.iter().enumerate().map(|(i, &(root, q))| (i as f64 * beats, (i + 1) as f64 * beats, Chord::new(root, q))).collect()
    }

    /// Am F C G, a bar each, twice.
    fn pop() -> Vec<(f64, f64, Chord)> {
        let four = [(9, Quality::Minor), (5, Quality::Major), (0, Quality::Major), (7, Quality::Major)];
        progression(&[four, four].concat(), 4.0)
    }

    fn every() -> Vec<Melody> {
        let mut all = Vec::new();
        for style in Style::ALL {
            for density in Density::ALL {
                for register in Register::ALL {
                    all.push(Melody { style, density, register });
                }
            }
        }
        all
    }

    fn chord_at(chords: &[(f64, f64, Chord)], beat: f64) -> Chord {
        chords.iter().find(|(a, b, _)| beat >= *a && beat < *b).unwrap().2
    }

    /// On every beat, a note of the chord that sounds (so never a semitone
    /// against it); every note in the key's scale, inside the register, in the
    /// clip, one at a time.
    fn check(melody: Melody, chords: &[(f64, f64, Chord)], length: f64, sig: TimeSig, seed: u64) -> Vec<Note> {
        let notes = generate(melody, chords, length, seed, sig, None);
        assert!(!notes.is_empty(), "{melody:?}");
        let key = key_of(chords);
        let scale = key.scale();
        let (low, high) = melody.register.range();
        let beats: Vec<f64> = sig.pulses().iter().map(|p| p.0).collect();
        for (i, n) in notes.iter().enumerate() {
            let chord = chord_at(chords, n.start);
            let in_bar = n.start % sig.bar();
            let on_beat = beats.iter().any(|b| (b - in_bar).abs() < 1e-9);
            if on_beat {
                assert!(chord.classes().contains(&(n.pitch % 12)), "{melody:?} seed {seed}: {} on beat {} over {}", n.pitch, n.start, chord.name());
            }
            assert!(scale[n.pitch as usize % 12], "{melody:?}: {} off {}", n.pitch, key.name());
            assert!((low..=high).contains(&n.pitch), "{melody:?}: {} outside {low}..{high}", n.pitch);
            assert!(n.start >= 0.0 && n.start + n.length <= length + 1e-9, "{n:?}");
            if let Some(next) = notes.get(i + 1) {
                assert!(n.start + n.length <= next.start + 1e-9, "{melody:?}: overlaps {n:?} {next:?}");
            }
        }
        notes
    }

    #[test]
    fn strong_beats_play_the_chord_in_the_key_and_register() {
        let chords = pop();
        for melody in every() {
            for seed in 0..6 {
                check(melody, &chords, 32.0, TimeSig::COMMON, seed);
            }
        }
    }

    /// In 3/4 (and 6/8) the same, over bars of three beats.
    #[test]
    fn in_three_four_and_six_eight_too() {
        let waltz = progression(&[(0, Quality::Major), (9, Quality::Minor), (5, Quality::Major), (7, Quality::Major)], 3.0);
        for melody in every() {
            for seed in 0..4 {
                let notes = check(melody, &waltz, 12.0, TimeSig::new(3, 4), seed);
                // Each phrase starts with the bar.
                assert!(notes.iter().all(|n| n.start < 12.0));
                check(melody, &waltz, 12.0, TimeSig::new(6, 8), seed);
            }
        }
    }

    /// A chord off the key (E in A minor) gets its own note (G#), never the
    /// scale's G a semitone from it.
    #[test]
    fn a_chord_off_the_key_brings_its_notes() {
        let chords = progression(&[(9, Quality::Minor), (5, Quality::Major), (4, Quality::Major), (9, Quality::Minor)], 4.0);
        let key = Key { tonic: 9, minor: true };
        for melody in every() {
            for seed in 0..4 {
                for n in generate(melody, &chords, 16.0, seed, TimeSig::COMMON, Some(key)) {
                    if (8.0..12.0).contains(&n.start) {
                        assert_ne!(n.pitch % 12, 7, "{melody:?}: G over E");
                    }
                }
            }
        }
    }

    /// The onsets of each phrase, from its start, in sixteenths.
    fn rhythm(notes: &[Note], phrase: f64, k: usize) -> Vec<i64> {
        notes.iter().filter(|n| n.start >= k as f64 * phrase - 1e-9 && n.start < (k + 1) as f64 * phrase - 1e-9).map(|n| ((n.start - k as f64 * phrase) * 4.0).round() as i64).collect()
    }

    /// The motif comes back: each phrase's rhythm is the first's (all but the
    /// odd note cut at a chord change), and the melody does not sound the
    /// same pitches every time: it follows the chords.
    #[test]
    fn the_motif_repeats_over_the_chords() {
        let chords = pop();
        for melody in every() {
            for seed in 0..6 {
                let notes = generate(melody, &chords, 32.0, seed, TimeSig::COMMON, None);
                let phrase = 4.0 * melody.style.bars() as f64;
                let first = rhythm(&notes, phrase, 0);
                let phrases = (32.0 / phrase) as usize;
                for k in 1..phrases {
                    let other = rhythm(&notes, phrase, k);
                    let shared = first.iter().filter(|s| other.contains(s)).count();
                    let all = first.len() + other.len() - shared;
                    let similarity = shared as f64 / all.max(1) as f64;
                    assert!(similarity >= 0.75, "{melody:?} seed {seed}: phrase {k} {other:?} against {first:?}");
                }
                let pitches: std::collections::HashSet<u8> = notes.iter().map(|n| n.pitch).collect();
                assert!(pitches.len() >= 3, "{melody:?}: {pitches:?}");
            }
        }
    }

    /// A question ends open, on a chord note other than the root; the answer
    /// and the clip end closed, on the tonic or the chord's root.
    #[test]
    fn phrases_end_on_the_chord() {
        let chords = pop();
        for style in Style::ALL {
            for seed in 0..6 {
                let melody = Melody::new(style);
                let notes = generate(melody, &chords, 32.0, seed, TimeSig::COMMON, None);
                let last = notes.last().unwrap();
                let chord = chord_at(&chords, last.start);
                assert!(last.pitch % 12 == chord.root || last.pitch % 12 == 0 || last.pitch % 12 == 9, "{style:?}: ends on {} over {}", last.pitch, chord.name());
                let phrase = 4.0 * style.bars() as f64;
                let question = notes.iter().filter(|n| n.start < phrase).last().unwrap();
                let chord = chord_at(&chords, question.start);
                assert!(chord.classes().contains(&(question.pitch % 12)), "{style:?}: the question ends on {} over {}", question.pitch, chord.name());
            }
        }
    }

    #[test]
    fn a_seed_is_a_take() {
        let chords = pop();
        for melody in every() {
            let a = generate(melody, &chords, 32.0, 11, TimeSig::COMMON, None);
            assert_eq!(a, generate(melody, &chords, 32.0, 11, TimeSig::COMMON, None));
            let takes: std::collections::HashSet<String> = (0..6).map(|s| format!("{:?}", generate(melody, &chords, 32.0, s, TimeSig::COMMON, None))).collect();
            assert!(takes.len() >= 4, "{melody:?}: {} takes", takes.len());
        }
    }

    /// More density, more notes; a topline's notes are longer than a hook's.
    #[test]
    fn density_and_style_shape_the_notes() {
        let chords = pop();
        let count = |style, density| -> usize { (0..8).map(|s| generate(Melody { style, density, register: Register::Mid }, &chords, 32.0, s, TimeSig::COMMON, None).len()).sum() };
        for style in Style::ALL {
            assert!(count(style, Density::Sparse) < count(style, Density::Normal) && count(style, Density::Normal) < count(style, Density::Busy), "{style:?}");
        }
        let mean = |style| -> f64 {
            let notes: Vec<Note> = (0..8).flat_map(|s| generate(Melody::new(style), &chords, 32.0, s, TimeSig::COMMON, None)).collect();
            notes.iter().map(|n| n.length).sum::<f64>() / notes.len() as f64
        };
        assert!(mean(Style::Topline) > 1.5 * mean(Style::Hook), "{} {}", mean(Style::Topline), mean(Style::Hook));
        assert!(mean(Style::Trance) < mean(Style::Hook));
    }
}
