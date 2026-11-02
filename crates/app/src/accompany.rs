//! Accompaniment: from the chords a clip plays (MIDI, or heard in audio), a
//! bass line, an arpeggio, a pad or rhythmic chords that fit them, on a new
//! track under it, or a melody (see `melody`). «Otra variación» writes
//! another take of the same part.

use crate::app::{ClipRef, PulseApp};
use engine::meter::TimeSig;
use engine::model::{AcidParams, Clip, FmParams, InstrumentKind, MidiClip, Note, SuperSawParams, SynthParams};
use engine::{Target, MAX_TRACKS};
use crate::melody::Style;
use stems::chord::{self, Chord};
use stems::key::Key;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The root on every beat, an octave or an approach now and then.
    BassRoot,
    /// Root and its octave in eighths (disco).
    BassOctaves,
    /// On the off-beats (house).
    BassOffbeat,
    /// A note a beat walking through the chord to the next root.
    BassWalking,
    /// Sixteenths off the kick (or triplets), rolling (techno, melodic techno).
    BassRolling,
    /// A dotted eighth and a sixteenth a beat (rock, EBM, tech house).
    BassGallop,
    /// The chord's notes one after another, rising.
    ArpUp,
    /// Rising and falling.
    ArpUpDown,
    /// The chords held, voices leading smoothly.
    Pad,
    /// Short chords in a rhythm.
    Stabs,
    /// A lead over the chords (see `melody`).
    Melody(crate::melody::Melody),
}

impl Part {
    pub const ALL: [Part; 10] = [Part::BassRoot, Part::BassOctaves, Part::BassOffbeat, Part::BassWalking, Part::BassRolling, Part::BassGallop, Part::ArpUp, Part::ArpUpDown, Part::Pad, Part::Stabs];

    pub fn label(self) -> &'static str {
        match self {
            Part::BassRoot => "Bajo: raíz",
            Part::BassOctaves => "Bajo: octavas",
            Part::BassOffbeat => "Bajo: contratiempo",
            Part::BassWalking => "Bajo: caminante",
            Part::BassRolling => "Bajo: rodante",
            Part::BassGallop => "Bajo: galope",
            Part::ArpUp => "Arpegio ascendente",
            Part::ArpUpDown => "Arpegio arriba y abajo",
            Part::Pad => "Pad",
            Part::Stabs => "Acordes rítmicos",
            Part::Melody(m) => match m.style {
                Style::Hook => "Melodía: gancho",
                Style::Topline => "Melodía: cantable",
                Style::Trance => "Melodía: lead de trance",
                Style::CallResponse => "Melodía: pregunta y respuesta",
            },
        }
    }

    /// The melodies the menus offer, one per style (normal density, middle register).
    pub fn melodies() -> [Part; 4] {
        Style::ALL.map(|s| Part::Melody(crate::melody::Melody::new(s)))
    }

    /// A number for the part's kind, to tell seeds apart (the same as before
    /// the rolling and galloping basses, so old takes come out the same).
    fn id(self) -> u64 {
        match self {
            Part::Melody(m) => 8 + m.style as u64,
            Part::BassRolling => 12,
            Part::BassGallop => 13,
            _ => Part::ALL.iter().filter(|p| !matches!(p, Part::BassRolling | Part::BassGallop)).position(|&p| p == self).unwrap_or(0) as u64,
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Part::BassRoot => "La fundamental de cada acorde en cada tiempo, con alguna octava y notas de paso hacia el siguiente",
            Part::BassOctaves => "Fundamental y su octava en corcheas, como en la música disco",
            Part::BassOffbeat => "En los contratiempos, como el bajo del house",
            Part::BassWalking => "Una nota por tiempo que recorre el acorde y llega al siguiente por un semitono",
            Part::BassRolling => "Semicorcheas que dejan el tiempo al bombo (o tresillos), con acentos, octavas y quintas: el bajo del techno y el melodic techno",
            Part::BassGallop => "Corchea con puntillo y semicorchea en cada tiempo, como el galope del rock, el EBM y el tech house",
            Part::ArpUp => "Las notas de cada acorde una tras otra, subiendo, en semicorcheas",
            Part::ArpUpDown => "Las notas de cada acorde subiendo y bajando, en semicorcheas",
            Part::Pad => "Los acordes sostenidos, con las voces enlazadas",
            Part::Stabs => "Acordes cortos con un ritmo sincopado",
            Part::Melody(m) => m.style.hint(),
        }
    }

    /// The instrument its new track gets.
    pub(crate) fn instrument(self) -> InstrumentKind {
        match self {
            Part::BassRoot | Part::BassOctaves | Part::BassOffbeat | Part::BassWalking => InstrumentKind::Acid(AcidParams::default()),
            // Short and dark, so the sixteenths roll; full and driven, for the gallop.
            Part::BassRolling | Part::BassGallop => {
                let name = if self == Part::BassRolling { "Bajo · Techno rodante" } else { "Bajo · Gordo saturado" };
                let kind = InstrumentKind::Acid(AcidParams::default());
                kind.factory_presets().into_iter().find(|(n, _)| *n == name).map_or(kind, |(_, k)| k)
            }
            Part::ArpUp | Part::ArpUpDown => InstrumentKind::Fm(FmParams::default()),
            Part::Pad => InstrumentKind::SuperSaw(SuperSawParams { cutoff: 2500.0, attack: 0.3, release: 0.8, gain: 0.3, ..Default::default() }),
            Part::Stabs => InstrumentKind::Synth(SynthParams::default()),
            // A factory lead that suits the style.
            Part::Melody(m) => {
                let (kind, name) = match m.style {
                    Style::Hook => (InstrumentKind::Synth(SynthParams::default()), "Pluck · Corto"),
                    Style::Topline => (InstrumentKind::Fm(FmParams::default()), "Lead · Flauta FM"),
                    Style::Trance => (InstrumentKind::SuperSaw(SuperSawParams::default()), "Lead · Trance clásico"),
                    Style::CallResponse => (InstrumentKind::Synth(SynthParams::default()), "Lead · Sierra brillante"),
                };
                kind.factory_presets().into_iter().find(|(n, _)| *n == name).map_or(kind, |(_, k)| k)
            }
        }
    }

    /// Its fader next to the chords' it is made from (dB): where a mix
    /// starts (the bass a little over the chords, the arpeggio as loud, the
    /// pad under them), for the sounds it comes with. At the chords' own
    /// level, a bass, a pad and an arpeggio added up took the master over 0 dB.
    fn fader_db(self) -> f32 {
        match self {
            Part::BassRoot | Part::BassOctaves | Part::BassOffbeat | Part::BassWalking | Part::Stabs => 0.0,
            Part::BassRolling | Part::BassGallop => 0.0,
            Part::ArpUp | Part::ArpUpDown | Part::Pad => -1.5,
            // On top of everything, a little under the chords.
            Part::Melody(_) => -3.0,
        }
    }

    /// Names a track of its role goes by (a template's «Bajo», an imported «Bass»).
    fn roles(self) -> &'static [&'static str] {
        match self {
            Part::BassRoot | Part::BassOctaves | Part::BassOffbeat | Part::BassWalking => &["bajo", "bass", "sub", "subbass", "sub bass"],
            Part::BassRolling | Part::BassGallop => &["bajo", "bass", "sub", "subbass", "sub bass"],
            Part::ArpUp | Part::ArpUpDown => &["arpegio", "arp", "arpeggio", "arpegiador"],
            Part::Pad => &["pad", "pads"],
            Part::Stabs => &["acordes", "chords", "stabs"],
            Part::Melody(_) => &["melodía", "melodia", "melody", "lead", "leads", "topline", "hook", "gancho"],
        }
    }

    /// The color of a new track for it: one per role, the same in every song
    /// (basses blue, arpeggios cyan, pads violet, stabs lilac, melodies
    /// orange), and the next in the palette if the chords already have it.
    pub(crate) fn color(self, chords: [u8; 3]) -> [u8; 3] {
        use crate::project::PALETTE;
        let index = match self {
            Part::BassRoot | Part::BassOctaves | Part::BassOffbeat | Part::BassWalking => 9,
            Part::BassRolling | Part::BassGallop => 9,
            Part::ArpUp | Part::ArpUpDown => 7,
            Part::Pad => 11,
            Part::Stabs => 10,
            Part::Melody(_) => 1,
        };
        if PALETTE[index] == chords { PALETTE[(index + 1) % PALETTE.len()] } else { PALETTE[index] }
    }

    /// The name of the track it is written on.
    pub(crate) fn short(self) -> &'static str {
        match self {
            Part::BassRoot | Part::BassOctaves | Part::BassOffbeat | Part::BassWalking => "Bajo",
            Part::BassRolling | Part::BassGallop => "Bajo",
            Part::ArpUp | Part::ArpUpDown => "Arpegio",
            Part::Pad => "Pad",
            Part::Stabs => "Acordes",
            Part::Melody(_) => "Melodía",
        }
    }
}

/// A small, repeatable random sequence (xorshift), so a seed gives the same take.
pub(crate) struct Dice(u64);

impl Dice {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// 0..1.
    pub(crate) fn roll(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    pub(crate) fn chance(&mut self, p: f64) -> bool {
        self.roll() < p
    }

    pub(crate) fn pick<'a, T>(&mut self, options: &'a [T]) -> &'a T {
        &options[((self.roll() * options.len() as f64) as usize).min(options.len() - 1)]
    }
}

/// The bass note of a chord, between E1 and D#2 (40..52).
fn bass_of(chord: &Chord) -> u8 {
    let class = chord.bass.unwrap_or(chord.root);
    40 + (class + 12 - 4) % 12
}

fn note(start: f64, length: f64, pitch: u8, velocity: f32) -> Note {
    Note { start, length, pitch, velocity, chance: 1.0 }
}

/// The pulses (start, length, first of its bar) from `from` to `to`, in bars of
/// `sig` from the clip's start.
fn pulses(sig: TimeSig, from: f64, to: f64) -> Vec<(f64, f64, bool)> {
    let bar = sig.bar();
    let mut out = Vec::new();
    let mut start = (from / bar - 1e-9).floor().max(0.0) * bar;
    while start < to - 1e-9 {
        for (i, &(at, length)) in sig.pulses().iter().enumerate() {
            let beat = start + at;
            if beat >= from - 1e-9 && beat < to - 1e-9 {
                out.push((beat, length, i == 0));
            }
        }
        start += bar;
    }
    out
}

/// The notes of `part` over `chords` (start and end beats, and the chord), for
/// a clip `length` beats long in bars of `sig`; `seed` picks among the variations.
pub fn generate(part: Part, chords: &[(f64, f64, Chord)], length: f64, seed: u64, sig: TimeSig) -> Vec<Note> {
    write(part, chords, length, seed, sig, None)
}

/// [`generate`], a melody in `key` (the chords' own if None).
fn write(part: Part, chords: &[(f64, f64, Chord)], length: f64, seed: u64, sig: TimeSig, key: Option<Key>) -> Vec<Note> {
    if let Part::Melody(m) = part {
        return crate::melody::generate(m, chords, length, seed, sig, key);
    }
    let stab_steps = crate::drum_gen::steps_from_four(sig);
    let mut dice = Dice::new(seed);
    let mut notes = Vec::new();
    let mut previous: Vec<u8> = Vec::new();
    // Rhythms for the stabs, sixteenths of a bar (one picked per take).
    const STABS: [&[f64]; 4] = [&[2.0, 6.0, 10.0, 14.0], &[0.0, 3.0, 6.0, 10.0, 12.0], &[2.0, 5.0, 8.0, 10.0, 13.0], &[0.0, 6.0, 8.0, 14.0]];
    let stabs = *dice.pick(&STABS);
    let arp_step = *dice.pick(&[0.25, 0.25, 0.5]);
    // The rolling bass's take: triplets or sixteenths, which steps go up an
    // octave (1) or to the fifth (2), and whether it climbs into each chord.
    // Drawn only for its parts, so the others' takes stay as they were.
    let rolling = part == Part::BassRolling;
    let roll_triplets = rolling && dice.chance(0.25);
    const ROLL_SHAPES: [&[u8]; 5] = [&[0, 0, 0], &[0, 1, 0], &[0, 0, 1], &[0, 2, 0], &[0, 1, 2]];
    let roll_shape = if rolling { *dice.pick(&ROLL_SHAPES) } else { ROLL_SHAPES[0] };
    let roll_climbs = rolling && dice.chance(0.5);
    // The gallop's short note: the root, its octave or (off the bar's first) the fifth.
    let gallop_lift = if part == Part::BassGallop { *dice.pick(&[0u8, 0, 1, 2]) } else { 0 };
    for (i, &(from, to, chord)) in chords.iter().enumerate() {
        let to = to.min(length);
        if to <= from {
            continue;
        }
        let next = chords.get(i + 1).map(|c| c.2);
        let root = bass_of(&chord);
        let tones = chord.classes();
        // A tone of the chord in the bass's octave, `k` steps up the chord.
        let tone = |k: usize| -> u8 {
            let class = tones[k % tones.len()];
            let mut p = root - root % 12 + class;
            if p < root {
                p += 12;
            }
            p
        };
        match part {
            Part::BassRoot => {
                for (beat, pulse, downbeat) in pulses(sig, from, to) {
                    let last = beat + pulse >= to - 1e-9;
                    let pitch = match (last, next) {
                        // Into the next chord: a semitone or a fifth away, now and then.
                        (true, Some(n)) if dice.chance(0.5) => {
                            // A semitone below, or its fifth below.
                            let target = bass_of(&n);
                            if dice.chance(0.5) { target - 1 } else { target - 5 }
                        }
                        _ if dice.chance(0.15) => root + 12,
                        _ => root,
                    };
                    notes.push(note(beat, (to - beat).min(pulse * 0.9), pitch, if downbeat { 0.9 } else { 0.75 }));
                }
            }
            Part::BassOctaves => {
                let mut beat = from;
                let mut up = false;
                while beat < to - 1e-9 {
                    let pitch = if up { root + 12 } else { root };
                    notes.push(note(beat, (to - beat).min(0.4), pitch, if up { 0.7 } else { 0.9 }));
                    up = !up;
                    beat += 0.5;
                }
            }
            Part::BassOffbeat => {
                // Halfway through each pulse, on an eighth.
                let bar = sig.bar();
                let mut start = (from / bar - 1e-9).floor().max(0.0) * bar;
                while start < to - 1e-9 {
                    for &(at, length) in &sig.pulses() {
                        let beat = start + at + ((length / 2.0) / 0.5).round() * 0.5;
                        if beat >= from - 1e-9 && beat < to - 1e-9 {
                            let pitch = if dice.chance(0.1) { root + 12 } else { root };
                            notes.push(note(beat, (to - beat).min(0.4), pitch, 0.85));
                        }
                    }
                    start += bar;
                }
            }
            Part::BassWalking => {
                let mut k = 0;
                for (i, (beat, pulse, _)) in pulses(sig, from, to).into_iter().enumerate() {
                    let last = beat + pulse >= to - 1e-9;
                    let pitch = match next {
                        // A semitone below or above the next root.
                        Some(n) if last && beat > from => {
                            let target = bass_of(&n);
                            if dice.chance(0.6) { target - 1 } else { target + 1 }
                        }
                        _ => {
                            let p = tone(k);
                            k += *dice.pick(&[1, 1, 2]);
                            p
                        }
                    };
                    notes.push(note(beat, (to - beat).min(pulse * 0.95), pitch, if i % 2 == 0 { 0.85 } else { 0.7 }));
                }
            }
            Part::BassRolling => {
                // Each pulse's first step is the kick's: the line takes the
                // rest, in sixteenths or (a take in four) in triplets.
                for (beat, pulse, downbeat) in pulses(sig, from, to) {
                    let steps = if roll_triplets { 3 } else { (pulse / 0.25).round().max(1.0) as usize };
                    let step = pulse / steps as f64;
                    for k in 1..steps {
                        let at = beat + k as f64 * step;
                        if at >= to - 1e-9 {
                            break;
                        }
                        let last = beat + pulse >= to - 1e-9;
                        // The take's shape (octave, fifth), a climb into the next chord
                        // on its last pulse, and now and then an octave of its own.
                        let shape = roll_shape[(k - 1) % roll_shape.len()];
                        let lift = if last && next.is_some() && roll_climbs { [0, 2, 1][(k - 1) % 3] } else { shape };
                        let pitch = match lift {
                            1 => root + 12,
                            2 => tone(2),
                            _ if dice.chance(0.06) => root + 12,
                            _ => root,
                        };
                        // The off-beat (halfway through the pulse; in triplets, the
                        // last) accented, harder on the bar's first.
                        let offbeat = if steps % 2 == 1 { k == steps - 1 } else { k * 2 == steps };
                        let velocity = match (offbeat, downbeat) {
                            (true, true) => 0.95,
                            (true, false) => 0.88,
                            _ => 0.7,
                        };
                        notes.push(note(at, (to - at).min(step * 0.8), pitch, velocity));
                    }
                }
            }
            Part::BassGallop => {
                // In each pulse: three quarters of it long, the last quarter short.
                for (beat, pulse, downbeat) in pulses(sig, from, to) {
                    let short = beat + pulse * 0.75;
                    notes.push(note(beat, (to - beat).min(pulse * 0.7), root, if downbeat { 0.9 } else { 0.78 }));
                    if short < to - 1e-9 {
                        let pitch = match gallop_lift {
                            1 => root + 12,
                            2 if !downbeat => tone(2),
                            _ => root,
                        };
                        // Pushing into the next beat, and done before it.
                        notes.push(note(short, (to - short).min(pulse * 0.2), pitch, 0.72));
                    }
                }
            }
            Part::ArpUp | Part::ArpUpDown => {
                let voiced = chord::voice(&chord, &previous);
                previous = voiced[1..].to_vec();
                // The upper notes and the first again an octave up.
                let mut steps: Vec<u8> = voiced[1..].to_vec();
                steps.push(voiced[1] + 12);
                if part == Part::ArpUpDown {
                    let down: Vec<u8> = steps[1..steps.len() - 1].iter().rev().copied().collect();
                    steps.extend(down);
                }
                let starts: Vec<f64> = pulses(sig, from, to).iter().map(|p| p.0).collect();
                let mut beat = from;
                let mut k = 0;
                while beat < to - 1e-9 {
                    let accent = starts.iter().any(|s| (s - beat).abs() < 1e-9);
                    notes.push(note(beat, (to - beat).min(arp_step * 0.9), steps[k % steps.len()], if accent { 0.85 } else { 0.65 }));
                    k += 1;
                    beat += arp_step;
                }
            }
            Part::Pad => {
                let voiced = chord::voice(&chord, &previous);
                previous = voiced[1..].to_vec();
                for &p in &voiced[1..] {
                    notes.push(note(from, to - from, p, 0.6));
                }
                // The bass too, an octave up, softer: a fuller pad.
                notes.push(note(from, to - from, voiced[0] + 12, 0.45));
            }
            Part::Melody(_) => {}
            Part::Stabs => {
                let voiced = chord::voice(&chord, &previous);
                previous = voiced[1..].to_vec();
                // The rhythm is written in a 4/4 bar: in the song's meter, where its hits fall.
                let line: String = (0..16).map(|i| if stabs.contains(&(i as f64)) { 'x' } else { '.' }).collect();
                let steps: Vec<f64> = crate::drum_gen::in_meter(&line, &stab_steps, sig, None).char_indices().filter(|c| c.1 == 'x').map(|c| c.0 as f64).collect();
                let bar_length = sig.bar();
                let mut bar = (from / bar_length - 1e-9).floor().max(0.0) * bar_length;
                while bar < to - 1e-9 {
                    for &sixteenth in &steps {
                        let beat = bar + sixteenth * 0.25;
                        if beat >= from - 1e-9 && beat < to - 1e-9 {
                            for &p in &voiced[1..] {
                                notes.push(note(beat, 0.2_f64.min(to - beat), p, 0.75));
                            }
                        }
                    }
                    bar += bar_length;
                }
            }
        }
    }
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    notes
}

/// The last accompaniment written, for «Otra variación».
#[derive(Clone)]
pub struct Generated {
    pub clip: ClipRef,
    part: Part,
    seed: u64,
    chords: Vec<(f64, f64, Chord)>,
    /// The key a melody is in.
    key: Option<Key>,
    /// What it wrote, to know it has not been edited since.
    notes: Vec<Note>,
}

impl Generated {
    /// The melody it is, if it is one.
    pub fn melody(&self) -> Option<crate::melody::Melody> {
        match self.part {
            Part::Melody(m) => Some(m),
            _ => None,
        }
    }
}

/// What the bar over the editor of the part written last shows and asks:
/// «Otra variación», and a melody's style, density and register.
pub struct Retake {
    pub melody: Option<crate::melody::Melody>,
    /// «Otra variación» was clicked.
    pub again: bool,
}

impl PulseApp {
    /// The chords of a clip, in its beats: a MIDI clip's from its notes, an
    /// audio one's heard.
    fn clip_chords(&mut self, clip: ClipRef) -> (Vec<(f64, f64, Chord)>, f64) {
        let key = self.project.key;
        let sig = self.clip_sig(clip);
        match self.clip(clip).cloned() {
            Some(Clip::Midi(m)) => {
                let chords = crate::harmony::chords(&m.notes, m.length, key, sig).into_iter().filter_map(|(a, b, c)| c.map(|c| (a, b, c))).collect();
                (chords, m.length)
            }
            Some(Clip::Audio(a)) => {
                let sample = self.sample(&a.sample);
                (crate::to_midi::audio_chords(&sample, &a, self.project.tempo, key), a.length)
            }
            None => (Vec::new(), 0.0),
        }
    }

    /// The chords the other tracks play with an empty MIDI clip (the same
    /// scene's clips, or what the Arrangement plays under it), in its beats.
    fn song_chords(&self, clip: ClipRef) -> Vec<(f64, f64, Chord)> {
        let Some(length) = self.clip(clip).map(|c| c.length()) else { return Vec::new() };
        let mut notes = Vec::new();
        // A clip's notes over `from..to` of the song, looping, moved by `shift`.
        let mut add = |m: &MidiClip, from: f64, to: f64, shift: f64| {
            if m.length <= 0.0 || m.muted {
                return;
            }
            let mut at = from;
            while at < to - 1e-9 {
                for n in &m.notes {
                    let start = at + n.start;
                    if start < to - 1e-9 && n.start < m.length {
                        notes.push(Note { start: start - shift, length: n.length.min(to - start), ..*n });
                    }
                }
                at += m.length;
            }
        };
        for (i, t) in self.project.tracks.iter().enumerate() {
            if i == clip.track() || t.is_drums() || t.is_audio() || t.is_group() {
                continue;
            }
            match clip {
                ClipRef::Session { slot, .. } => {
                    if let Some(Clip::Midi(m)) = &t.clips[slot] {
                        add(m, 0.0, length, 0.0);
                    }
                }
                ClipRef::Arrangement { track, index } => {
                    let Some(p) = self.project.tracks[track].arrangement.get(index) else { return Vec::new() };
                    for a in &t.arrangement {
                        if let Clip::Midi(m) = &a.clip {
                            if a.start < p.end() && a.end() > p.start {
                                add(m, a.start, a.end().min(p.end()), p.start);
                            }
                        }
                    }
                }
            }
        }
        // Only what sounds inside the clip.
        notes.retain(|n| n.start + n.length > 0.0 && n.start < length);
        for n in &mut notes {
            if n.start < 0.0 {
                n.length += n.start;
                n.start = 0.0;
            }
        }
        crate::harmony::chords(&notes, length, self.project.key, self.clip_sig(clip)).into_iter().filter_map(|(a, b, c)| c.map(|c| (a, b, c))).collect()
    }

    /// A melody in the empty MIDI clip itself, over what the song plays with it.
    fn melody_here(&mut self, clip: ClipRef, part: Part) {
        let chords = self.song_chords(clip);
        if chords.is_empty() {
            self.status = Some("No suenan acordes con este clip vacío: ponlo en la escena o el tramo de una pista de acordes".into());
            return;
        }
        let key = crate::melody::key_for(&chords, self.project.key);
        let seed = crate::versions::now() ^ part.id() << 32;
        let sig = self.clip_sig(clip);
        let Some(Clip::Midi(m)) = self.clip_mut(clip) else { return };
        let notes = write(part, &chords, m.length, seed, sig, Some(key));
        m.notes = notes.clone();
        self.sync_clip_ref(clip);
        self.select_clip(clip);
        self.accompaniment = Some(Generated { clip, part, seed, chords, key: Some(key), notes });
        self.status = Some(format!("{} sobre los acordes de la canción: «Otra variación» en el editor escribe otra", part.label()));
    }

    /// An empty MIDI track for `part`'s role, other than `source`: a template's
    /// «Bajo» waiting for its bass line (named «Bajo», «Bajo 2», «Bass»…).
    fn track_for(&self, part: Part, source: usize) -> Option<usize> {
        self.project.tracks.iter().enumerate().position(|(i, t)| {
            let name = t.name.trim().to_lowercase();
            let name = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ');
            let empty = t.clips.iter().all(|c| c.is_none()) && t.arrangement.is_empty();
            i != source && empty && !t.is_audio() && !t.is_drums() && !t.is_group() && part.roles().contains(&name)
        })
    }

    /// Writes `part` for the clip's chords: on the empty track of its role if
    /// there is one, else on a new track under the clip's.
    pub fn accompany(&mut self, clip: ClipRef, part: Part) {
        // An empty clip gets a melody of its own, over the song's chords.
        if matches!((part, self.clip(clip)), (Part::Melody(_), Some(Clip::Midi(m))) if m.notes.is_empty()) {
            return self.melody_here(clip, part);
        }
        let (chords, length) = self.clip_chords(clip);
        if chords.is_empty() {
            self.status = Some("No se encuentran acordes en este clip para acompañarlos".into());
            return;
        }
        let seed = crate::versions::now() ^ part.id() << 32;
        let key = matches!(part, Part::Melody(_)).then(|| crate::melody::key_for(&chords, self.project.key));
        let notes = write(part, &chords, length, seed, self.clip_sig(clip), key);
        let source = clip.track();
        let existing = self.track_for(part, source);
        if existing.is_none() && self.project.tracks.len() >= MAX_TRACKS {
            self.status = Some(format!("Máximo de {MAX_TRACKS} pistas"));
            return;
        }
        // A new track takes its role's color; a template's, its own.
        let color = existing.map_or_else(|| part.color(self.clip(clip).map_or(self.project.tracks[source].color, |c| c.color())), |t| self.project.tracks[t].color);
        // The clip says what it plays: «Bajo: contratiempo», «Pad».
        let midi = Clip::Midi(MidiClip { notes: notes.clone(), ..MidiClip::new(part.label(), length, color) });
        let placed = match clip {
            ClipRef::Session { .. } => None,
            ClipRef::Arrangement { track: t, index } => match self.project.tracks[t].arrangement.get(index) {
                Some(p) => Some((p.start, p.length)),
                None => return,
            },
        };
        let (index, mut track) = match existing {
            Some(t) => (t, None),
            None => {
                let mut track = crate::project::Track::new(self.free_track_name(part.short()), color, part.instrument());
                // Beside its source, in the same group.
                track.depth = self.project.tracks[source].depth;
                // Next to the chords' fader, so the mix doesn't go over 0 dB.
                track.mixer.gain = self.project.tracks[source].mixer.gain * 10f32.powf(part.fader_db() / 20.0);
                (source + 1, Some(track))
            }
        };
        let target = track.as_mut().unwrap_or_else(|| &mut self.project.tracks[index]);
        let new = match (clip, placed) {
            (ClipRef::Session { slot, .. }, _) => {
                target.clips[slot] = Some(midi);
                ClipRef::Session { track: index, slot }
            }
            (_, Some((start, length))) => {
                target.arrangement.push(engine::model::ArrangementClip { start, length, clip: midi });
                ClipRef::Arrangement { track: index, index: target.arrangement.len() - 1 }
            }
            _ => return,
        };
        if let Some(track) = track {
            let ids = self.track_ids();
            self.project.tracks.insert(index, track);
            self.follow_tracks(&ids);
        }
        self.selected_target = Target::Track(index);
        self.sync_all();
        self.select_clip(new);
        self.accompaniment = Some(Generated { clip: new, part, seed, chords, key, notes });
        let more = if matches!(part, Part::Melody(_)) { ", y ahí cambias densidad y registro" } else { "" };
        self.status = Some(format!("{} para {} acordes: «Otra variación» en el editor escribe otra{more}", part.label(), self.accompaniment.as_ref().map_or(0, |g| g.chords.len())));
    }

    /// `name`, or «name 2», «name 3»… if a track already has it.
    pub(crate) fn free_track_name(&self, name: &str) -> String {
        let taken = |n: &str| self.project.tracks.iter().any(|t| t.name == n);
        if !taken(name) {
            return name.to_string();
        }
        (2..).map(|i| format!("{name} {i}")).find(|n| !taken(n)).expect("some number is free")
    }

    /// Whether `clip` is the accompaniment written last, unedited.
    pub fn can_vary(&self, clip: ClipRef) -> bool {
        self.accompaniment.as_ref().is_some_and(|g| g.clip == clip && matches!(self.clip(clip), Some(Clip::Midi(m)) if m.notes == g.notes))
    }

    /// What the editor's bar offers for `clip`: None unless it is the part
    /// written last, unedited.
    pub fn retake(&self, clip: ClipRef) -> Option<Retake> {
        self.can_vary(clip).then(|| Retake { melody: self.accompaniment.as_ref().and_then(|g| g.melody()), again: false })
    }

    /// Writes another take of the last accompaniment in its clip.
    pub fn vary_accompaniment(&mut self) {
        let Some(generated) = self.accompaniment.as_mut() else { return };
        generated.seed = generated.seed.wrapping_add(0x5851_F42D_4C95_7F2D);
        self.rewrite_accompaniment();
        self.status = Some("Otra variación".into());
    }

    /// The last melody again, the same take in another style, density or
    /// register; a clip still named after its style takes the new one's name.
    pub fn set_melody(&mut self, melody: crate::melody::Melody) {
        let Some(generated) = self.accompaniment.as_mut() else { return };
        let old = generated.part;
        generated.part = Part::Melody(melody);
        let clip = generated.clip;
        if let Some(Clip::Midi(m)) = self.clip_mut(clip) {
            if m.name == old.label() {
                m.name = Part::Melody(melody).label().to_string();
            }
        }
        self.rewrite_accompaniment();
    }

    /// Writes the last accompaniment over again from its part and seed.
    fn rewrite_accompaniment(&mut self) {
        let Some(mut generated) = self.accompaniment.take() else { return };
        let length = match self.clip(generated.clip) {
            Some(Clip::Midi(m)) => m.length,
            _ => return,
        };
        let notes = write(generated.part, &generated.chords, length, generated.seed, self.clip_sig(generated.clip), generated.key);
        let clip = generated.clip;
        if let Some(Clip::Midi(m)) = self.clip_mut(clip) {
            m.notes = notes.clone();
        }
        generated.notes = notes;
        self.accompaniment = Some(generated);
        self.piano_roll.recenter();
        self.sync_clip_ref(clip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stems::chord::Quality;

    /// The Beat template with everything a producer adds in a hurry: a techno
    /// beat, four chords, and a bass, a pad and an arpeggio from them, as they
    /// come and with the sounds the audit picked. The drop leaves headroom on
    /// the master (it went 2.7 dB over before: every new track came in at
    /// the drums' level).
    #[test]
    fn a_beat_with_everything_leaves_headroom() {
        use engine::model::{ArrangementClip, Clip, MidiClip, Note};
        let presets: [(&str, &str); 4] = [("Acordes", "Rave de los 90"), ("Bajo", "Techno rodante"), ("Pad", "Ancho"), ("Arpegio", "Metálico")];
        let mut peaks = Vec::new();
        for with_presets in [false, true] {
            let mut app = PulseApp::headless(crate::project::Project::empty());
            app.templates = vec![crate::templates::Template { name: "Beat".into(), file: None }];
            app.new_from_template(0);
            let kit = crate::drum_gen::Kit::from_rows(&engine::model::DRUM_VOICES.iter().map(|&(p, n)| (p, n.to_string())).collect::<Vec<_>>());
            let settings = crate::drum_gen::Settings { style: crate::drum_gen::Style::Techno, bars: 8, sounds: crate::drum_gen::Style::Techno.sounds(), ..Default::default() };
            let (beat, groove) = crate::drum_gen::generate(&kit, &settings, 32.0, &[], 0, TimeSig::COMMON);
            app.project.tracks[0].arrangement.push(ArrangementClip { start: 0.0, length: 32.0, clip: Clip::Midi(MidiClip { notes: beat, groove, ..MidiClip::new("Ritmo", 32.0, [0; 3]) }) });
            // Am F C G, a bar each, twice.
            let chords = [[57, 60, 64], [53, 57, 60], [48, 52, 55], [55, 59, 62]];
            let notes: Vec<Note> = (0..8).flat_map(|bar| chords[bar % 4].iter().map(move |&p| Note { start: bar as f64 * 4.0, length: 4.0, pitch: p, velocity: 0.8, chance: 1.0 })).collect();
            app.project.tracks[2].arrangement.push(ArrangementClip { start: 0.0, length: 32.0, clip: Clip::Midi(MidiClip { notes, ..MidiClip::new("Acordes", 32.0, [0; 3]) }) });
            for part in [Part::BassOffbeat, Part::Pad, Part::ArpUpDown] {
                app.accompany(ClipRef::Arrangement { track: 2, index: 0 }, part);
            }
            if with_presets {
                for t in &mut app.project.tracks {
                    let Some(&(_, name)) = presets.iter().find(|(part, _)| t.name.ends_with(part)) else { continue };
                    if t.arrangement.is_empty() {
                        continue;
                    }
                    if let Some((_, kind)) = t.instrument.factory_presets().into_iter().find(|(n, _)| n.ends_with(&format!("· {name}"))) {
                        t.instrument = kind;
                    }
                }
            }
            let project = app.project.clone();
            let mut samples = crate::sync::SampleCache::default();
            let commands = crate::sync::project_commands(&project, &mut samples, 48_000.0, &Default::default());
            let (l, r) = crate::export::render(commands, (0.0, 32.0), crate::export::Song::of(&project), 48_000.0, &Default::default(), &|_| {}).unwrap();
            let peak = stems::loudness::sample_peak(&l, &r);
            let lufs = stems::loudness::integrated(&l, &r, 48_000.0).unwrap();
            let faders: Vec<String> = project.tracks.iter().map(|t| format!("{} {:+.1} dB", t.name, 20.0 * t.mixer.gain.log10())).collect();
            println!("{}: {lufs:.1} LUFS, pico {peak:+.1} dBFS · {faders:?}", if with_presets { "con presets" } else { "de serie" });
            peaks.push(peak);
            // Loud enough still for the master to take it where it goes.
            assert!(lufs > -18.0, "{lufs} LUFS");
        }
        assert!(peaks.iter().all(|&p| p <= -1.0), "{peaks:?} dBFS");
    }

    /// In 6/8 the bass walks the dotted beats and the stabs keep inside the bars;
    /// in 3/4 the root comes on each beat, the first of each bar accented.
    #[test]
    fn parts_follow_the_song_s_meter() {
        let mut w = [0.0; 12];
        for class in [9, 0, 4] {
            w[class] = 1.0;
        }
        let am = Chord::detect(&w, Some(9), None).unwrap();
        let chords = [(0.0, 6.0, am)];
        let six = TimeSig::new(6, 8);
        let starts: Vec<f64> = generate(Part::BassWalking, &chords, 6.0, 1, six).iter().map(|n| n.start).collect();
        assert_eq!(starts, [0.0, 1.5, 3.0, 4.5]);
        let waltz = TimeSig::new(3, 4);
        let root = generate(Part::BassRoot, &chords, 6.0, 1, waltz);
        assert_eq!(root.iter().map(|n| n.start).collect::<Vec<_>>(), [0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
        assert!(root[0].velocity > root[1].velocity && root[3].velocity > root[4].velocity);
        let seven = TimeSig::new(7, 8);
        let stabs = generate(Part::Stabs, &[(0.0, 7.0, am)], 7.0, 3, seven);
        assert!(!stabs.is_empty() && stabs.iter().all(|n| n.start < 7.0));
    }

    /// Am, F, C, G, a bar each.
    fn progression() -> Vec<(f64, f64, Chord)> {
        [(9, Quality::Minor), (5, Quality::Major), (0, Quality::Major), (7, Quality::Major)]
            .iter()
            .enumerate()
            .map(|(i, &(root, q))| (i as f64 * 4.0, i as f64 * 4.0 + 4.0, Chord::new(root, q)))
            .collect()
    }

    #[test]
    fn every_part_plays_the_chords_notes_inside_the_clip() {
        let chords = progression();
        for part in Part::ALL {
            for seed in 0..5 {
                let notes = generate(part, &chords, 16.0, seed, TimeSig::COMMON);
                assert!(!notes.is_empty(), "{part:?}");
                for n in &notes {
                    assert!(n.start >= 0.0 && n.start + n.length <= 16.0 + 1e-9, "{part:?}: {n:?}");
                    let (_, _, chord) = chords.iter().find(|(a, b, _)| n.start >= *a && n.start < *b).unwrap();
                    let in_chord = chord.classes().contains(&(n.pitch % 12));
                    // Only the bass's passing notes, on a chord's last beat, may leave it.
                    let passing = matches!(part, Part::BassRoot | Part::BassWalking) && n.start % 4.0 == 3.0;
                    assert!(in_chord || passing, "{part:?} seed {seed}: {} at {} over {}", n.pitch, n.start, chord.name());
                }
            }
        }
    }

    #[test]
    fn basses_are_low_and_on_their_rhythm() {
        let chords = progression();
        let root = generate(Part::BassRoot, &chords, 16.0, 1, TimeSig::COMMON);
        assert_eq!(root.len(), 16, "a note a beat");
        assert!(root.iter().all(|n| (40..=64).contains(&n.pitch)));
        assert_eq!(root[0].pitch, 45, "A1 first");
        let offbeat = generate(Part::BassOffbeat, &chords, 16.0, 1, TimeSig::COMMON);
        assert!(offbeat.iter().all(|n| n.start % 1.0 == 0.5));
        let octaves = generate(Part::BassOctaves, &chords, 16.0, 1, TimeSig::COMMON);
        assert_eq!(octaves.len(), 32);
        assert_eq!(octaves[1].pitch, octaves[0].pitch + 12);
    }

    /// The rolling bass leaves each beat to the kick and plays the other three
    /// sixteenths (or two triplets), the off-beat accented; the gallop plays a
    /// long and a short note a beat, done before the next. Both in the
    /// chords, low, in any meter, and the same take for the same seed.
    #[test]
    fn rolling_and_galloping_basses_leave_the_kick_its_beat() {
        let chords = progression();
        let mut triplets = 0;
        let mut shapes = std::collections::HashSet::new();
        for seed in 0..16 {
            let notes = generate(Part::BassRolling, &chords, 16.0, seed, TimeSig::COMMON);
            assert_eq!(notes, generate(Part::BassRolling, &chords, 16.0, seed, TimeSig::COMMON), "the same take");
            shapes.insert(format!("{:?}", notes.iter().map(|n| n.pitch).collect::<Vec<_>>()));
            assert!(notes.iter().all(|n| (40..=64).contains(&n.pitch)), "low: {notes:?}");
            // Nothing on the beat, nothing over the next one.
            for n in &notes {
                assert!(n.start.fract() > 0.2, "seed {seed}: {n:?} on the kick");
                assert!(n.start + n.length <= n.start.floor() + 1.0 + 1e-9, "seed {seed}: {n:?} over the kick");
            }
            let per_beat = notes.len() / 16;
            assert!(notes.len() % 16 == 0 && (per_beat == 3 || per_beat == 2), "seed {seed}: {}", notes.len());
            if per_beat == 2 {
                triplets += 1;
            }
            // Accents (the Acid's, over 80 %): the off-beat of each beat.
            let accented: Vec<f64> = notes.iter().filter(|n| n.velocity > 0.8).map(|n| n.start.fract()).collect();
            assert_eq!(accented.len(), 16, "seed {seed}");
            let off = if per_beat == 3 { 0.5 } else { 2.0 / 3.0 };
            assert!(accented.iter().all(|f| (f - off).abs() < 1e-9), "seed {seed}: {accented:?}");
        }
        assert!(triplets > 0 && triplets < 16, "{triplets} takes in triplets");
        assert!(shapes.len() > 3, "octaves and fifths vary the takes");
        // In 6/8, the dotted beats are the kick's.
        let am = progression()[0].2;
        let six = generate(Part::BassRolling, &[(0.0, 6.0, am)], 6.0, 2, TimeSig::new(6, 8));
        assert!(!six.is_empty() && six.iter().all(|n| ![0.0, 1.5, 3.0, 4.5].contains(&n.start) && n.start < 6.0));

        for seed in 0..6 {
            let gallop = generate(Part::BassGallop, &chords, 16.0, seed, TimeSig::COMMON);
            assert_eq!(gallop.len(), 32);
            for pair in gallop.chunks(2) {
                assert_eq!(pair[1].start - pair[0].start, 0.75, "{pair:?}");
                assert!(pair[0].length > pair[1].length && pair[1].start + pair[1].length < pair[0].start + 1.0, "{pair:?}");
            }
            assert!(gallop.iter().step_by(8).all(|n| n.velocity > 0.8), "the bar's first accented");
        }
        // Their sounds: the factory basses that suit them.
        let preset = |name: &str| InstrumentKind::Acid(AcidParams::default()).factory_presets().into_iter().find(|(n, _)| *n == name).unwrap().1;
        assert_eq!(Part::BassRolling.instrument(), preset("Bajo · Techno rodante"));
        assert_eq!(Part::BassGallop.instrument(), preset("Bajo · Gordo saturado"));
        // The other parts' takes are as they were (their seeds didn't move).
        assert_eq!(Part::Pad.id(), 6);
        assert_eq!(Part::Stabs.id(), 7);
    }

    #[test]
    fn pads_lead_smoothly_and_seeds_vary_the_take() {
        let chords = progression();
        let pad = generate(Part::Pad, &chords, 16.0, 1, TimeSig::COMMON);
        assert_eq!(pad.len(), 16, "three voices and a bass, four chords");
        // The same seed, the same take; another seed, another (for the varied parts).
        assert_eq!(generate(Part::Stabs, &chords, 16.0, 7, TimeSig::COMMON), generate(Part::Stabs, &chords, 16.0, 7, TimeSig::COMMON));
        let takes: std::collections::HashSet<String> = (0..8).map(|s| format!("{:?}", generate(Part::BassRoot, &chords, 16.0, s, TimeSig::COMMON))).collect();
        assert!(takes.len() > 1);
    }

    /// The «Beat» template has an empty «Bajo»: the bass goes there, not on a
    /// new track; the pad, which has none, gets its own, named «Pad»; and the
    /// clips say what they play.
    #[test]
    fn the_empty_track_of_its_role_is_used() {
        let template = crate::templates::all().into_iter().find(|t| t.name == "Beat").unwrap();
        let mut app = PulseApp::headless(crate::templates::project(&template).unwrap());
        let chords = app.project.tracks.iter().position(|t| t.name == "Acordes").unwrap();
        let bass = app.project.tracks.iter().position(|t| t.name == "Bajo").unwrap();
        let instrument = app.project.tracks[bass].instrument.clone();
        let c = chord::progressions(stems::key::Key { tonic: 9, minor: true })[0].degrees.iter().map(|&d| chord::on_degree(stems::key::Key { tonic: 9, minor: true }, d, false, false)).collect::<Vec<_>>();
        let mut clip = MidiClip::new("Acordes", 4.0, [0; 3]);
        crate::harmony::write(&mut clip, &mut 0.0, 4.0, &c, None, 4.0);
        app.project.tracks[chords].clips[0] = Some(Clip::Midi(clip));
        let tracks = app.project.tracks.len();
        app.accompany(ClipRef::Session { track: chords, slot: 0 }, Part::BassOffbeat);
        assert_eq!(app.project.tracks.len(), tracks, "{:?}", app.status);
        assert_eq!(app.project.tracks[bass].clips[0].as_ref().map(|c| c.name()), Some("Bajo: contratiempo"));
        assert_eq!(app.project.tracks[bass].instrument, instrument, "the template's sound stays");
        assert_eq!(app.selected_clip, Some(ClipRef::Session { track: bass, slot: 0 }));
        // Again: that «Bajo» is no longer empty, so a new one, «Bajo 2».
        app.accompany(ClipRef::Session { track: chords, slot: 0 }, Part::BassRoot);
        assert_eq!(app.project.tracks[chords + 1].name, "Bajo 2");
        app.accompany(ClipRef::Session { track: chords, slot: 0 }, Part::Pad);
        assert_eq!(app.project.tracks[chords + 1].name, "Pad");
        assert_eq!(app.project.tracks.len(), tracks + 2);
        let names: Vec<&str> = app.project.tracks.iter().map(|t| t.name.as_str()).collect();
        assert!(names.iter().all(|n| !n.contains('·')), "{names:?}");
    }

    /// From the demo's «Am F C G»: a bass track under it, and another take.
    #[test]
    fn a_clip_gets_an_accompaniment_and_another_take() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let t = app.project.tracks.iter().position(|t| t.clips[0].as_ref().is_some_and(|c| c.name() == "Am F C G")).unwrap();
        let tracks = app.project.tracks.len();
        app.accompany(ClipRef::Session { track: t, slot: 0 }, Part::BassRoot);
        assert_eq!(app.project.tracks.len(), tracks + 1);
        let new = ClipRef::Session { track: t + 1, slot: 0 };
        let Some(Clip::Midi(bass)) = app.clip(new).cloned() else { panic!("{:?}", app.status) };
        assert_eq!(bass.length, 16.0);
        assert_eq!(bass.notes[0].pitch % 12, 9, "A first");
        assert!(app.can_vary(new));
        let mut varied = false;
        for _ in 0..6 {
            app.vary_accompaniment();
            let Some(Clip::Midi(other)) = app.clip(new) else { panic!() };
            varied |= other.notes != bass.notes;
        }
        assert!(varied);
        // Edited by hand, it is no longer the one to vary.
        if let Some(Clip::Midi(m)) = app.project.tracks[t + 1].clips[0].as_mut() {
            m.notes.pop();
        }
        assert!(!app.can_vary(new));
    }

    /// Bass, pad, arpeggio, stabs and melody from the same chords: each new
    /// track its own color, none the chords', and the same role the same
    /// color (from the 2026-11 audit: they all came out the chords').
    #[test]
    fn each_part_gets_its_own_color() {
        use crate::melody::{Melody, Style};
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let t = app.project.tracks.iter().position(|t| t.clips[0].as_ref().is_some_and(|c| c.name() == "Am F C G")).unwrap();
        let chords = app.project.tracks[t].color;
        let mut colors = Vec::new();
        for part in [Part::BassRoot, Part::Pad, Part::ArpUp, Part::Stabs, Part::Melody(Melody::new(Style::Hook))] {
            app.accompany(ClipRef::Session { track: t, slot: 0 }, part);
            let new = &app.project.tracks[t + 1];
            assert_eq!(new.name, part.short());
            assert_ne!(new.color, chords, "{}", new.name);
            let Some(Clip::Midi(m)) = &new.clips[0] else { panic!() };
            assert_eq!(m.color, new.color, "its clip too");
            colors.push(new.color);
        }
        let distinct: std::collections::HashSet<[u8; 3]> = colors.iter().copied().collect();
        assert_eq!(distinct.len(), colors.len(), "{colors:?}");
        assert_eq!(Part::BassOffbeat.color(chords), Part::BassRoot.color(chords));
        // Chords already in a role's color: the next one.
        let blue = crate::project::PALETTE[9];
        assert_ne!(Part::BassRoot.color(blue), blue);
    }

    /// A trance lead from the demo's «Am F C G»: a new «Melodía» track with
    /// a SuperSaw lead; the same take higher, then another take.
    #[test]
    fn a_melody_gets_its_track_and_its_controls() {
        use crate::melody::{Melody, Register, Style};
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let t = app.project.tracks.iter().position(|t| t.clips[0].as_ref().is_some_and(|c| c.name() == "Am F C G")).unwrap();
        app.accompany(ClipRef::Session { track: t, slot: 0 }, Part::Melody(Melody::new(Style::Trance)));
        let new = ClipRef::Session { track: t + 1, slot: 0 };
        assert_eq!(app.project.tracks[t + 1].name, "Melodía");
        assert!(matches!(app.project.tracks[t + 1].instrument, InstrumentKind::SuperSaw(p) if p.cutoff == 5000.0), "Lead · Trance clásico");
        let Some(Clip::Midi(lead)) = app.clip(new).cloned() else { panic!("{:?}", app.status) };
        assert_eq!(lead.name, "Melodía: lead de trance");
        assert!(app.retake(new).and_then(|r| r.melody).is_some());
        app.set_melody(Melody { register: Register::High, ..Melody::new(Style::Trance) });
        let Some(Clip::Midi(high)) = app.clip(new).cloned() else { panic!() };
        assert_eq!(high.notes.iter().map(|n| n.start).collect::<Vec<_>>(), lead.notes.iter().map(|n| n.start).collect::<Vec<_>>());
        assert!(high.notes.iter().zip(&lead.notes).all(|(h, l)| h.pitch >= l.pitch) && high.notes != lead.notes);
        app.vary_accompaniment();
        let Some(Clip::Midi(other)) = app.clip(new).cloned() else { panic!() };
        assert_ne!(other.notes, high.notes);
        assert!(other.notes.iter().all(|n| (67..=86).contains(&n.pitch)), "another take, still high");
    }

    /// An empty clip on the Arrangement (and in the Session) gets a melody of
    /// its own over what the other tracks play with it.
    #[test]
    fn an_empty_clip_gets_a_melody_over_the_song() {
        use crate::melody::{Melody, Style};
        use engine::model::ArrangementClip;
        let template = crate::templates::all().into_iter().find(|t| t.name == "Beat").unwrap();
        let mut app = PulseApp::headless(crate::templates::project(&template).unwrap());
        let chords_track = app.project.tracks.iter().position(|t| t.name == "Acordes").unwrap();
        let lead = app.project.tracks.iter().position(|t| t.name == "Melodía").unwrap();
        // Am F C G, a bar each, from bar 3 on; the empty clip over its last two bars and one more.
        let notes: Vec<Note> = [[57, 60, 64], [53, 57, 60], [48, 52, 55], [55, 59, 62]].iter().enumerate().flat_map(|(i, c)| c.iter().map(move |&p| note(i as f64 * 4.0, 4.0, p, 0.8))).collect();
        app.project.tracks[chords_track].arrangement.push(ArrangementClip { start: 8.0, length: 16.0, clip: Clip::Midi(MidiClip { notes: notes.clone(), ..MidiClip::new("Acordes", 16.0, [0; 3]) }) });
        app.project.tracks[lead].arrangement.push(ArrangementClip { start: 16.0, length: 12.0, clip: Clip::Midi(MidiClip::new("Melodía", 12.0, [0; 3])) });
        let tracks = app.project.tracks.len();
        let clip = ClipRef::Arrangement { track: lead, index: 0 };
        app.accompany(clip, Part::Melody(Melody::new(Style::Topline)));
        assert_eq!(app.project.tracks.len(), tracks, "in the clip itself");
        let Some(Clip::Midi(m)) = app.clip(clip).cloned() else { panic!() };
        assert!(!m.notes.is_empty(), "{:?}", app.status);
        // C then G under its first two bars, nothing under the third.
        for n in &m.notes {
            assert!(n.start < 8.0, "{n:?}");
            if n.start % 1.0 == 0.0 {
                let chord: &[u8] = if n.start < 4.0 { &[0, 4, 7] } else { &[7, 11, 2] };
                assert!(chord.contains(&(n.pitch % 12)), "{n:?}");
            }
        }
        assert!(app.can_vary(clip));
        // In the Session, the same scene's chords.
        app.project.tracks[chords_track].clips[1] = Some(Clip::Midi(MidiClip { notes, ..MidiClip::new("Acordes", 16.0, [0; 3]) }));
        app.project.tracks[lead].clips[1] = Some(Clip::Midi(MidiClip::new("Melodía", 16.0, [0; 3])));
        app.accompany(ClipRef::Session { track: lead, slot: 1 }, Part::Melody(Melody::new(Style::Hook)));
        let Some(Clip::Midi(m)) = app.clip(ClipRef::Session { track: lead, slot: 1 }).cloned() else { panic!() };
        assert!(m.notes.len() >= 8 && m.notes.iter().any(|n| n.start >= 12.0), "{:?}", app.status);
        // Nothing playing with it: it says so.
        app.project.tracks[lead].clips[2] = Some(Clip::Midi(MidiClip::new("Melodía", 16.0, [0; 3])));
        app.accompany(ClipRef::Session { track: lead, slot: 2 }, Part::Melody(Melody::new(Style::Hook)));
        assert!(app.status.as_deref().is_some_and(|s| s.contains("No suenan acordes")));
    }

    /// The Beat template (without its «Melodía» track) with a beat, Am F C G,
    /// a bass and a melody of `style` in its own sound, rendered.
    fn song_with_melody(style: crate::melody::Style) -> (Vec<f32>, Vec<f32>) {
        use engine::model::ArrangementClip;
        let template = crate::templates::all().into_iter().find(|t| t.name == "Beat").unwrap();
        let mut project = crate::templates::project(&template).unwrap();
        project.tracks.retain(|t| t.name != "Melodía");
        let mut app = PulseApp::headless(project);
        let kit = crate::drum_gen::Kit::from_rows(&engine::model::DRUM_VOICES.iter().map(|&(p, n)| (p, n.to_string())).collect::<Vec<_>>());
        let settings = crate::drum_gen::Settings { style: crate::drum_gen::Style::House, bars: 8, sounds: crate::drum_gen::Style::House.sounds(), ..Default::default() };
        let (beat, groove) = crate::drum_gen::generate(&kit, &settings, 32.0, &[], 0, TimeSig::COMMON);
        let drums = app.project.tracks.iter().position(|t| t.is_drums()).unwrap();
        app.project.tracks[drums].arrangement.push(ArrangementClip { start: 0.0, length: 32.0, clip: Clip::Midi(MidiClip { notes: beat, groove, ..MidiClip::new("Ritmo", 32.0, [0; 3]) }) });
        let chords = app.project.tracks.iter().position(|t| t.name == "Acordes").unwrap();
        let notes: Vec<Note> = (0..8).flat_map(|bar| [[57, 60, 64], [53, 57, 60], [48, 52, 55], [55, 59, 62]][bar % 4].iter().map(move |&p| note(bar as f64 * 4.0, 4.0, p, 0.8))).collect();
        app.project.tracks[chords].arrangement.push(ArrangementClip { start: 0.0, length: 32.0, clip: Clip::Midi(MidiClip { notes, ..MidiClip::new("Acordes", 32.0, [0; 3]) }) });
        app.accompany(ClipRef::Arrangement { track: chords, index: 0 }, Part::BassOffbeat);
        app.accompany(ClipRef::Arrangement { track: chords, index: 0 }, Part::Melody(crate::melody::Melody::new(style)));
        let project = app.project.clone();
        let mut samples = crate::sync::SampleCache::default();
        let commands = crate::sync::project_commands(&project, &mut samples, 48_000.0, &Default::default());
        crate::export::render(commands, (0.0, 32.0), crate::export::Song::of(&project), 48_000.0, &Default::default(), &|_| {}).unwrap()
    }

    /// The loudest lead on top of a beat, chords and a bass still leaves headroom.
    #[test]
    fn a_melody_leaves_headroom() {
        let (l, r) = song_with_melody(crate::melody::Style::Trance);
        let peak = stems::loudness::sample_peak(&l, &r);
        assert!(peak <= -1.0, "{peak:+.1} dBFS");
    }

    /// With `PULSE_SHOTS=dir`, each style over the same song, as WAV.
    #[test]
    #[ignore]
    fn melody_renders() {
        let Some(dir) = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        for style in crate::melody::Style::ALL {
            let (l, r) = song_with_melody(style);
            let peak = stems::loudness::sample_peak(&l, &r);
            let lufs = stems::loudness::integrated(&l, &r, 48_000.0).unwrap();
            let name = Part::Melody(crate::melody::Melody::new(style)).label().replace(": ", "-").replace(' ', "-").replace('í', "i").to_lowercase();
            let path = dir.join(format!("{name}.wav"));
            crate::export::write(&path, &l, &r, 48_000, crate::export::BitDepth::Int24, 1.0).unwrap();
            println!("{}: {lufs:.1} LUFS, pico {peak:+.1} dBFS", path.display());
            assert!(peak <= -1.0);
        }
    }
}
