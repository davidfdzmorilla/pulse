//! Accompaniment: from the chords a clip plays (MIDI, or heard in audio), a
//! bass line, an arpeggio, a pad or rhythmic chords that fit them, on a new
//! track under it. «Otra variación» writes another take of the same part.

use crate::app::{ClipRef, PulseApp};
use engine::model::{AcidParams, Clip, FmParams, InstrumentKind, MidiClip, Note, SuperSawParams, SynthParams};
use engine::{Target, MAX_TRACKS};
use stems::chord::{self, Chord};

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
    /// The chord's notes one after another, rising.
    ArpUp,
    /// Rising and falling.
    ArpUpDown,
    /// The chords held, voices leading smoothly.
    Pad,
    /// Short chords in a rhythm.
    Stabs,
}

impl Part {
    pub const ALL: [Part; 8] = [Part::BassRoot, Part::BassOctaves, Part::BassOffbeat, Part::BassWalking, Part::ArpUp, Part::ArpUpDown, Part::Pad, Part::Stabs];

    pub fn label(self) -> &'static str {
        match self {
            Part::BassRoot => "Bajo: raíz",
            Part::BassOctaves => "Bajo: octavas",
            Part::BassOffbeat => "Bajo: contratiempo",
            Part::BassWalking => "Bajo: caminante",
            Part::ArpUp => "Arpegio ascendente",
            Part::ArpUpDown => "Arpegio arriba y abajo",
            Part::Pad => "Pad",
            Part::Stabs => "Acordes rítmicos",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Part::BassRoot => "La fundamental de cada acorde en cada tiempo, con alguna octava y notas de paso hacia el siguiente",
            Part::BassOctaves => "Fundamental y su octava en corcheas, como en la música disco",
            Part::BassOffbeat => "En los contratiempos, como el bajo del house",
            Part::BassWalking => "Una nota por tiempo que recorre el acorde y llega al siguiente por un semitono",
            Part::ArpUp => "Las notas de cada acorde una tras otra, subiendo, en semicorcheas",
            Part::ArpUpDown => "Las notas de cada acorde subiendo y bajando, en semicorcheas",
            Part::Pad => "Los acordes sostenidos, con las voces enlazadas",
            Part::Stabs => "Acordes cortos con un ritmo sincopado",
        }
    }

    /// The instrument its new track gets.
    fn instrument(self) -> InstrumentKind {
        match self {
            Part::BassRoot | Part::BassOctaves | Part::BassOffbeat | Part::BassWalking => InstrumentKind::Acid(AcidParams::default()),
            Part::ArpUp | Part::ArpUpDown => InstrumentKind::Fm(FmParams::default()),
            Part::Pad => InstrumentKind::SuperSaw(SuperSawParams { cutoff: 2500.0, attack: 0.3, release: 0.8, gain: 0.3, ..Default::default() }),
            Part::Stabs => InstrumentKind::Synth(SynthParams::default()),
        }
    }

    fn short(self) -> &'static str {
        match self {
            Part::BassRoot | Part::BassOctaves | Part::BassOffbeat | Part::BassWalking => "Bajo",
            Part::ArpUp | Part::ArpUpDown => "Arpegio",
            Part::Pad => "Pad",
            Part::Stabs => "Acordes",
        }
    }
}

/// A small, repeatable random sequence (xorshift), so a seed gives the same take.
struct Dice(u64);

impl Dice {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// 0..1.
    fn roll(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn chance(&mut self, p: f64) -> bool {
        self.roll() < p
    }

    fn pick<'a, T>(&mut self, options: &'a [T]) -> &'a T {
        &options[((self.roll() * options.len() as f64) as usize).min(options.len() - 1)]
    }
}

/// The bass note of a chord, between E1 and D#2 (40..52).
fn bass_of(chord: &Chord) -> u8 {
    let class = chord.bass.unwrap_or(chord.root);
    40 + (class + 12 - 4) % 12
}

fn note(start: f64, length: f64, pitch: u8, velocity: f32) -> Note {
    Note { start, length, pitch, velocity }
}

/// The notes of `part` over `chords` (start and end beats, and the chord), for
/// a clip `length` beats long; `seed` picks among the variations.
pub fn generate(part: Part, chords: &[(f64, f64, Chord)], length: f64, seed: u64) -> Vec<Note> {
    let mut dice = Dice::new(seed);
    let mut notes = Vec::new();
    let mut previous: Vec<u8> = Vec::new();
    // Rhythms for the stabs, sixteenths of a bar (one picked per take).
    const STABS: [&[f64]; 4] = [&[2.0, 6.0, 10.0, 14.0], &[0.0, 3.0, 6.0, 10.0, 12.0], &[2.0, 5.0, 8.0, 10.0, 13.0], &[0.0, 6.0, 8.0, 14.0]];
    let stabs = *dice.pick(&STABS);
    let arp_step = *dice.pick(&[0.25, 0.25, 0.5]);
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
                let mut beat = from;
                while beat < to - 1e-9 {
                    let last = beat + 1.0 >= to - 1e-9;
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
                    notes.push(note(beat, (to - beat).min(0.9), pitch, if (beat - from) % 4.0 == 0.0 { 0.9 } else { 0.75 }));
                    beat += 1.0;
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
                let mut beat = from.floor() + 0.5;
                while beat < to - 1e-9 {
                    if beat >= from {
                        let pitch = if dice.chance(0.1) { root + 12 } else { root };
                        notes.push(note(beat, (to - beat).min(0.4), pitch, 0.85));
                    }
                    beat += 1.0;
                }
            }
            Part::BassWalking => {
                let mut beat = from;
                let mut k = 0;
                while beat < to - 1e-9 {
                    let last = beat + 1.0 >= to - 1e-9;
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
                    notes.push(note(beat, (to - beat).min(0.95), pitch, if (beat - from) % 2.0 == 0.0 { 0.85 } else { 0.7 }));
                    beat += 1.0;
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
                let mut beat = from;
                let mut k = 0;
                while beat < to - 1e-9 {
                    let accent = ((beat / 1.0) - (beat / 1.0).round()).abs() < 1e-9;
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
            Part::Stabs => {
                let voiced = chord::voice(&chord, &previous);
                previous = voiced[1..].to_vec();
                let bar_start = (from / 4.0).floor() * 4.0;
                let mut bar = bar_start;
                while bar < to - 1e-9 {
                    for &sixteenth in stabs {
                        let beat = bar + sixteenth * 0.25;
                        if beat >= from - 1e-9 && beat < to - 1e-9 {
                            for &p in &voiced[1..] {
                                notes.push(note(beat, 0.2_f64.min(to - beat), p, 0.75));
                            }
                        }
                    }
                    bar += 4.0;
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
    /// What it wrote, to know it has not been edited since.
    notes: Vec<Note>,
}

impl PulseApp {
    /// The chords of a clip, in its beats: a MIDI clip's from its notes, an
    /// audio one's heard.
    fn clip_chords(&mut self, clip: ClipRef) -> (Vec<(f64, f64, Chord)>, f64) {
        let key = self.project.key;
        match self.clip(clip).cloned() {
            Some(Clip::Midi(m)) => {
                let chords = crate::harmony::chords(&m.notes, m.length, key).into_iter().filter_map(|(a, b, c)| c.map(|c| (a, b, c))).collect();
                (chords, m.length)
            }
            Some(Clip::Audio(a)) => {
                let sample = self.sample(&a.sample);
                (crate::to_midi::audio_chords(&sample, &a, self.project.tempo, key), a.length)
            }
            None => (Vec::new(), 0.0),
        }
    }

    /// Writes `part` for the clip's chords on a new track under it.
    pub fn accompany(&mut self, clip: ClipRef, part: Part) {
        let (chords, length) = self.clip_chords(clip);
        if chords.is_empty() {
            self.status = Some("No se encuentran acordes en este clip para acompañarlos".into());
            return;
        }
        if self.project.tracks.len() >= MAX_TRACKS {
            self.status = Some(format!("Máximo de {MAX_TRACKS} pistas"));
            return;
        }
        let seed = crate::versions::now() ^ (part as u64) << 32;
        let notes = generate(part, &chords, length, seed);
        let source = clip.track();
        let color = self.clip(clip).map_or([128; 3], |c| c.color());
        let name = format!("{} · {}", self.clip(clip).map_or("Clip", |c| c.name()), part.short());
        let midi = Clip::Midi(MidiClip { notes: notes.clone(), ..MidiClip::new(name.clone(), length, color) });
        let mut track = crate::project::Track::new(name, color, part.instrument());
        let new = match clip {
            ClipRef::Session { slot, .. } => {
                track.clips[slot] = Some(midi);
                ClipRef::Session { track: source + 1, slot }
            }
            ClipRef::Arrangement { track: t, index } => {
                let Some(placed) = self.project.tracks[t].arrangement.get(index) else { return };
                track.arrangement.push(engine::model::ArrangementClip { start: placed.start, length: placed.length, clip: midi });
                ClipRef::Arrangement { track: source + 1, index: 0 }
            }
        };
        self.project.tracks.insert(source + 1, track);
        self.selected_target = Target::Track(source + 1);
        self.sync_all();
        self.select_clip(new);
        self.accompaniment = Some(Generated { clip: new, part, seed, chords, notes });
        self.status = Some(format!("{} para {} acordes: «Otra variación» en el editor escribe otra", part.label(), self.accompaniment.as_ref().map_or(0, |g| g.chords.len())));
    }

    /// Whether `clip` is the accompaniment written last, unedited.
    pub fn can_vary(&self, clip: ClipRef) -> bool {
        self.accompaniment.as_ref().is_some_and(|g| g.clip == clip && matches!(self.clip(clip), Some(Clip::Midi(m)) if m.notes == g.notes))
    }

    /// Writes another take of the last accompaniment in its clip.
    pub fn vary_accompaniment(&mut self) {
        let Some(mut generated) = self.accompaniment.take() else { return };
        generated.seed = generated.seed.wrapping_add(0x5851_F42D_4C95_7F2D);
        let length = match self.clip(generated.clip) {
            Some(Clip::Midi(m)) => m.length,
            _ => return,
        };
        let notes = generate(generated.part, &generated.chords, length, generated.seed);
        let clip = generated.clip;
        let target = match clip {
            ClipRef::Session { track, slot } => self.project.tracks[track].clips[slot].as_mut(),
            ClipRef::Arrangement { track, index } => self.project.tracks[track].arrangement.get_mut(index).map(|a| &mut a.clip),
        };
        if let Some(Clip::Midi(m)) = target {
            m.notes = notes.clone();
        }
        generated.notes = notes;
        self.accompaniment = Some(generated);
        self.sync_clip_ref(clip);
        self.status = Some("Otra variación".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stems::chord::Quality;

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
                let notes = generate(part, &chords, 16.0, seed);
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
        let root = generate(Part::BassRoot, &chords, 16.0, 1);
        assert_eq!(root.len(), 16, "a note a beat");
        assert!(root.iter().all(|n| (40..=64).contains(&n.pitch)));
        assert_eq!(root[0].pitch, 45, "A1 first");
        let offbeat = generate(Part::BassOffbeat, &chords, 16.0, 1);
        assert!(offbeat.iter().all(|n| n.start % 1.0 == 0.5));
        let octaves = generate(Part::BassOctaves, &chords, 16.0, 1);
        assert_eq!(octaves.len(), 32);
        assert_eq!(octaves[1].pitch, octaves[0].pitch + 12);
    }

    #[test]
    fn pads_lead_smoothly_and_seeds_vary_the_take() {
        let chords = progression();
        let pad = generate(Part::Pad, &chords, 16.0, 1);
        assert_eq!(pad.len(), 16, "three voices and a bass, four chords");
        // The same seed, the same take; another seed, another (for the varied parts).
        assert_eq!(generate(Part::Stabs, &chords, 16.0, 7), generate(Part::Stabs, &chords, 16.0, 7));
        let takes: std::collections::HashSet<String> = (0..8).map(|s| format!("{:?}", generate(Part::BassRoot, &chords, 16.0, s))).collect();
        assert!(takes.len() > 1);
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
}
