//! Drum patterns: a beat in a style (house, techno, trap, drum & bass…) for a
//! drum track's clip, on whatever kit it has (its sounds found by their names),
//! busier or plainer, with a fill at the end of each phrase and, if asked, the
//! kick following the bass. Each take is different; the same seed, the same take.

use crate::project::Track;
use engine::model::Note;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    House,
    Techno,
    Trap,
    DrumAndBass,
    BoomBap,
    PopRock,
    Reggaeton,
    Breakbeat,
}

impl Style {
    pub const ALL: [Style; 8] = [Style::House, Style::Techno, Style::Trap, Style::DrumAndBass, Style::BoomBap, Style::PopRock, Style::Reggaeton, Style::Breakbeat];

    pub fn label(self) -> &'static str {
        match self {
            Style::House => "House",
            Style::Techno => "Techno",
            Style::Trap => "Trap",
            Style::DrumAndBass => "Drum & Bass",
            Style::BoomBap => "Hip hop",
            Style::PopRock => "Pop / Rock",
            Style::Reggaeton => "Reggaetón",
            Style::Breakbeat => "Breakbeat",
        }
    }

    /// The tempo it usually goes at (for the hint).
    pub fn tempo(self) -> u32 {
        match self {
            Style::House => 124,
            Style::Techno => 130,
            Style::Trap => 140,
            Style::DrumAndBass => 174,
            Style::BoomBap => 90,
            Style::PopRock => 110,
            Style::Reggaeton => 95,
            Style::Breakbeat => 130,
        }
    }
}

/// The sounds of a kit, by the note that plays them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Kit {
    pub kick: Option<u8>,
    pub snare: Option<u8>,
    pub clap: Option<u8>,
    pub closed: Option<u8>,
    pub open: Option<u8>,
    pub tom: Option<u8>,
    pub rim: Option<u8>,
    pub perc: Option<u8>,
}

impl Kit {
    /// From a drum track's rows (note and name: a drum machine's sounds, a rack's
    /// samples).
    pub fn from_rows(rows: &[(u8, String)]) -> Self {
        let find = |words: &[&str], not: &[&str]| {
            rows.iter().find(|(_, name)| {
                let n = name.to_lowercase();
                words.iter().any(|w| n.contains(w)) && !not.iter().any(|w| n.contains(w))
            })
            .map(|r| r.0)
        };
        Self {
            kick: find(&["kick", "bombo", "bd", "bass drum"], &[]),
            snare: find(&["snare", "caja", "sd"], &[]),
            clap: find(&["clap", "palma"], &[]),
            closed: find(&["closed", "cerrad", "chh", "hat", "hh", "charles"], &["open", "abiert"]),
            open: find(&["open", "abiert", "ohh", "oh "], &[]),
            tom: find(&["tom"], &[]),
            rim: find(&["rim", "stick"], &[]),
            perc: find(&["cowbell", "perc", "shaker", "conga", "bongo", "cencerro"], &[]),
        }
    }

    /// The backbeat: the snare, or the clap, or the rim.
    fn back(&self, prefer_clap: bool) -> Option<u8> {
        if prefer_clap { self.clap.or(self.snare) } else { self.snare.or(self.clap) }.or(self.rim)
    }

    fn hat(&self) -> Option<u8> {
        self.closed.or(self.open).or(self.perc)
    }
}

/// A sound's place in a bar of sixteen steps: how likely each step is (1: always;
/// less: more so the busier the take), and how loud.
struct Line {
    steps: [f32; 16],
    velocity: f32,
}

const fn on(steps: &[usize]) -> [f32; 16] {
    let mut out = [0.0; 16];
    let mut i = 0;
    while i < steps.len() {
        out[steps[i]] = 1.0;
        i += 1;
    }
    out
}

/// Every step at `p`, except those set already.
fn fill(mut steps: [f32; 16], p: f32) -> [f32; 16] {
    for s in &mut steps {
        if *s == 0.0 {
            *s = p;
        }
    }
    steps
}

fn with(mut steps: [f32; 16], extra: &[(usize, f32)]) -> [f32; 16] {
    for &(i, p) in extra {
        steps[i] = steps[i].max(p);
    }
    steps
}

/// A style's bar: kick, backbeat, closed hat, open hat, rim, perc; and whether
/// the backbeat is a clap.
struct Pattern {
    kick: Line,
    back: Line,
    hat: Line,
    open: Line,
    rim: Line,
    perc: Line,
    clap: bool,
}

fn line(steps: [f32; 16], velocity: f32) -> Line {
    Line { steps, velocity }
}

fn pattern(style: Style) -> Pattern {
    let eighths = on(&[0, 2, 4, 6, 8, 10, 12, 14]);
    let offbeats = on(&[2, 6, 10, 14]);
    let four = on(&[0, 4, 8, 12]);
    let backbeat = on(&[4, 12]);
    let none = [0.0; 16];
    match style {
        Style::House => Pattern {
            kick: line(four, 1.0),
            back: line(backbeat, 0.9),
            hat: line(fill(none, 0.35), 0.5),
            open: line(offbeats, 0.75),
            rim: line(with(none, &[(7, 0.4), (15, 0.3), (3, 0.2)]), 0.6),
            perc: line(with(none, &[(5, 0.25), (13, 0.25)]), 0.5),
            clap: true,
        },
        Style::Techno => Pattern {
            kick: line(four, 1.0),
            back: line(with(none, &[(4, 0.7), (12, 0.7)]), 0.8),
            hat: line(fill(offbeats, 0.6), 0.6),
            open: line(with(none, &[(2, 0.5), (6, 0.5), (10, 0.5), (14, 0.5)]), 0.6),
            rim: line(with(none, &[(3, 0.4), (11, 0.4), (7, 0.25)]), 0.6),
            perc: line(with(none, &[(1, 0.2), (9, 0.2), (14, 0.2)]), 0.5),
            clap: true,
        },
        Style::Trap => Pattern {
            kick: line(with(on(&[0]), &[(10, 0.8), (3, 0.4), (11, 0.35), (6, 0.25)]), 1.0),
            back: line(on(&[8]), 0.95),
            hat: line(fill(eighths, 0.9), 0.6),
            open: line(with(none, &[(6, 0.3), (14, 0.2)]), 0.55),
            rim: line(with(none, &[(13, 0.25)]), 0.5),
            perc: line(none, 0.5),
            clap: true,
        },
        Style::DrumAndBass => Pattern {
            kick: line(with(on(&[0, 10]), &[(7, 0.3), (2, 0.2)]), 1.0),
            back: line(with(backbeat, &[(7, 0.25), (9, 0.2), (15, 0.25)]), 0.95),
            hat: line(fill(eighths, 0.35), 0.55),
            open: line(with(none, &[(14, 0.3)]), 0.55),
            rim: line(with(none, &[(11, 0.25)]), 0.5),
            perc: line(none, 0.5),
            clap: false,
        },
        Style::BoomBap => Pattern {
            kick: line(with(on(&[0, 10]), &[(7, 0.6), (3, 0.25), (15, 0.2)]), 1.0),
            back: line(with(backbeat, &[(15, 0.2), (7, 0.15)]), 0.95),
            hat: line(fill(eighths, 0.2), 0.6),
            open: line(with(none, &[(14, 0.25)]), 0.55),
            rim: line(none, 0.5),
            perc: line(none, 0.5),
            clap: false,
        },
        Style::PopRock => Pattern {
            kick: line(with(on(&[0, 8]), &[(6, 0.4), (10, 0.4), (3, 0.15)]), 1.0),
            back: line(with(backbeat, &[(14, 0.15)]), 0.95),
            hat: line(fill(eighths, 0.15), 0.65),
            open: line(with(none, &[(14, 0.3)]), 0.55),
            rim: line(none, 0.5),
            perc: line(none, 0.5),
            clap: false,
        },
        Style::Reggaeton => Pattern {
            kick: line(four, 1.0),
            back: line(on(&[3, 6, 11, 14]), 0.85),
            hat: line(with(eighths, &[]), 0.5).with_chance(0.7),
            open: line(none, 0.5),
            rim: line(with(none, &[(7, 0.3), (15, 0.3)]), 0.5),
            perc: line(with(none, &[(2, 0.2), (10, 0.2)]), 0.5),
            clap: false,
        },
        Style::Breakbeat => Pattern {
            kick: line(with(on(&[0, 10]), &[(7, 0.4), (2, 0.3)]), 1.0),
            back: line(with(backbeat, &[(14, 0.3), (9, 0.2)]), 0.95),
            hat: line(fill(eighths, 0.4), 0.6),
            open: line(with(none, &[(6, 0.25), (14, 0.25)]), 0.55),
            rim: line(with(none, &[(11, 0.2)]), 0.5),
            perc: line(none, 0.5),
            clap: false,
        },
    }
}

impl Line {
    /// The same line with its sure steps made `p` likely.
    fn with_chance(mut self, p: f32) -> Self {
        for s in &mut self.steps {
            if *s >= 1.0 {
                *s = p;
            }
        }
        self
    }
}

/// How the beat is made.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub style: Style,
    /// 0 plain (only the sure hits) .. 1 busy.
    pub complexity: f32,
    /// A fill on the last beat of each four-bar phrase (or of the clip).
    pub fills: bool,
    /// The kick plays with the bass (where it falls on eighths).
    pub follow_bass: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { style: Style::House, complexity: 0.5, fills: true, follow_bass: false }
    }
}

/// A small, repeatable random sequence.
struct Dice(u64);

impl Dice {
    fn roll(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// A beat `length` beats long for `kit`. `bass` are the beats (in the clip) where
/// the bass starts its notes, for `follow_bass`.
pub fn generate(kit: &Kit, settings: &Settings, length: f64, bass: &[f64], seed: u64) -> Vec<Note> {
    let p = pattern(settings.style);
    let mut dice = Dice(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut notes = Vec::new();
    let bars = (length / 4.0).ceil().max(1.0) as usize;
    // How likely a step that is not sure is played: none when plain, the line's
    // own chance at the middle, more when busy.
    let busy = settings.complexity.clamp(0.0, 1.0) * 2.0;
    let back = kit.back(p.clap);
    let lines: [(&Line, Option<u8>); 6] = [(&p.kick, kit.kick), (&p.back, back), (&p.hat, kit.hat()), (&p.open, kit.open), (&p.rim, kit.rim), (&p.perc, kit.perc)];
    // One take of the optional steps, kept for the phrase and varied a little on
    // its even bars: a beat that repeats like a groove, not noise.
    let mut take: Vec<[bool; 16]> = lines.iter().map(|(l, _)| std::array::from_fn(|s| l.steps[s] < 1.0 && dice.roll() < l.steps[s] * busy)).collect();
    let phrase = if bars >= 4 { 4 } else { bars };
    for bar in 0..bars {
        let start = bar as f64 * 4.0;
        if bar % 2 == 1 {
            // A step changed here and there.
            for (k, (l, _)) in lines.iter().enumerate() {
                for s in 0..16 {
                    if l.steps[s] < 1.0 && l.steps[s] > 0.0 && dice.roll() < 0.15 {
                        take[k][s] = !take[k][s];
                    }
                }
            }
        }
        let filled = settings.fills && (bar + 1) % phrase == 0;
        for (k, (l, pitch)) in lines.iter().enumerate() {
            let Some(pitch) = *pitch else { continue };
            for s in 0..16 {
                // The fill takes the last beat: hats and extras out of its way.
                if filled && s >= 12 && k != 0 {
                    continue;
                }
                let sure = l.steps[s] >= 1.0;
                if !(sure || take[k][s]) {
                    continue;
                }
                let beat = start + s as f64 * 0.25;
                if beat >= length - 1e-9 {
                    continue;
                }
                // Accents on the beat, softer on sixteenths and for the extras.
                let accent = if s % 4 == 0 { 1.0 } else if s % 2 == 0 { 0.85 } else { 0.7 };
                let extra = if sure { 1.0 } else { 0.8 };
                let velocity = (l.velocity * accent * extra * (0.95 + 0.1 * dice.roll())).clamp(0.1, 1.0);
                notes.push(Note { start: beat, length: 0.25, pitch, velocity });
                // Trap's hats roll: thirty-seconds now and then.
                if settings.style == Style::Trap && k == 2 && s % 4 == 3 && dice.roll() < 0.3 * busy {
                    notes.push(Note { start: beat + 0.125, length: 0.125, pitch, velocity: velocity * 0.8 });
                }
            }
        }
        if filled {
            // The last beat: sixteenths on the snare (and toms, if any), getting louder.
            let voices: Vec<u8> = [back, kit.tom].into_iter().flatten().collect();
            for (i, s) in (12..16).enumerate() {
                let beat = start + s as f64 * 0.25;
                if beat >= length - 1e-9 {
                    break;
                }
                let pitch = voices[(i / 2) % voices.len().max(1)];
                notes.push(Note { start: beat, length: 0.25, pitch, velocity: 0.6 + 0.12 * i as f32 });
            }
        }
    }
    // With the bass: a kick where it starts a note on an eighth and none is there.
    if settings.follow_bass {
        if let Some(kick) = kit.kick {
            for &b in bass {
                let on_eighth = ((b * 2.0) - (b * 2.0).round()).abs() < 1e-6;
                let taken = notes.iter().any(|n| n.pitch == kick && (n.start - b).abs() < 0.1);
                if on_eighth && !taken && b < length {
                    notes.push(Note { start: b, length: 0.25, pitch: kick, velocity: 0.85 });
                }
            }
        }
    }
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    notes.dedup_by(|a, b| a.pitch == b.pitch && (a.start - b.start).abs() < 1e-9);
    notes
}

/// Where the bass tracks start notes over a clip's span: the session clips in the
/// same scene, or the arrangement clips under it (in its beats).
pub fn bass_onsets(tracks: &[Track], slot: Option<usize>, span: Option<(f64, f64)>, length: f64) -> Vec<f64> {
    let mut onsets = Vec::new();
    for track in tracks.iter().filter(|t| crate::arrange::role(t) == crate::arrange::Role::Bass) {
        if let Some(slot) = slot {
            if let Some(engine::model::Clip::Midi(m)) = &track.clips[slot] {
                let mut lap = 0.0;
                while lap < length && m.length > 0.0 {
                    onsets.extend(m.notes.iter().map(|n| lap + n.start).filter(|&b| b < length));
                    lap += m.length;
                }
            }
        }
        if let Some((from, to)) = span {
            for placed in &track.arrangement {
                let engine::model::Clip::Midi(m) = &placed.clip else { continue };
                if m.length <= 0.0 {
                    continue;
                }
                let mut lap = placed.start;
                while lap < placed.end().min(to) {
                    for n in &m.notes {
                        let b = lap + n.start;
                        if b >= from && b < to && b < placed.end() {
                            onsets.push(b - from);
                        }
                    }
                    lap += m.length;
                }
            }
        }
    }
    onsets.sort_by(f64::total_cmp);
    onsets
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::model::drum_note;

    fn machine() -> Kit {
        let rows: Vec<(u8, String)> = engine::model::DRUM_VOICES.iter().map(|&(p, n)| (p, n.to_string())).collect();
        Kit::from_rows(&rows)
    }

    #[test]
    fn the_bass_of_the_same_scene_is_found() {
        let demo = crate::project::Project::demo();
        let onsets = bass_onsets(&demo.tracks, Some(0), None, 4.0);
        assert!(!onsets.is_empty() && onsets.iter().all(|&b| (0.0..4.0).contains(&b)), "{onsets:?}");
        // The second scene has no bass.
        assert!(bass_onsets(&demo.tracks, Some(1), None, 4.0).is_empty());
    }

    #[test]
    fn kits_are_read_from_their_names() {
        let kit = machine();
        assert_eq!(kit.kick, Some(drum_note::KICK));
        assert_eq!(kit.snare, Some(drum_note::SNARE));
        assert_eq!(kit.clap, Some(drum_note::CLAP));
        assert_eq!(kit.closed, Some(drum_note::CLOSED_HAT));
        assert_eq!(kit.open, Some(drum_note::OPEN_HAT));
        // A rack named in Spanish.
        let rack = Kit::from_rows(&[(36, "Bombo 909".into()), (37, "Caja seca".into()), (38, "Charles abierto".into()), (39, "Charles cerrado".into())]);
        assert_eq!((rack.kick, rack.snare, rack.open, rack.closed), (Some(36), Some(37), Some(38), Some(39)));
    }

    fn hits(notes: &[Note], pitch: u8, bar: usize) -> Vec<usize> {
        notes.iter().filter(|n| n.pitch == pitch && (n.start / 4.0) as usize == bar).map(|n| ((n.start % 4.0) * 4.0).round() as usize).collect()
    }

    #[test]
    fn each_style_has_its_backbone() {
        let kit = machine();
        let plain = |style| generate(&kit, &Settings { style, complexity: 0.0, fills: false, follow_bass: false }, 16.0, &[], 1);
        // House: four on the floor, claps on 2 and 4, open hats on the off-beats.
        let house = plain(Style::House);
        assert_eq!(hits(&house, drum_note::KICK, 0), vec![0, 4, 8, 12]);
        assert_eq!(hits(&house, drum_note::CLAP, 0), vec![4, 12]);
        assert_eq!(hits(&house, drum_note::OPEN_HAT, 0), vec![2, 6, 10, 14]);
        // Trap: the snare (a clap) on 3, half time.
        assert_eq!(hits(&plain(Style::Trap), drum_note::CLAP, 0), vec![8]);
        // Drum & bass: kick on 1 and the «and» of 3, snare on 2 and 4.
        let dnb = plain(Style::DrumAndBass);
        assert_eq!(hits(&dnb, drum_note::KICK, 0), vec![0, 10]);
        assert_eq!(hits(&dnb, drum_note::SNARE, 0), vec![4, 12]);
        // Reggaetón: the dembow.
        assert_eq!(hits(&plain(Style::Reggaeton), drum_note::SNARE, 0), vec![3, 6, 11, 14]);
    }

    #[test]
    fn busier_takes_have_more_and_seeds_differ() {
        let kit = machine();
        let count = |complexity| generate(&kit, &Settings { style: Style::Techno, complexity, fills: false, follow_bass: false }, 16.0, &[], 3).len();
        assert!(count(1.0) > count(0.5) && count(0.5) > count(0.0));
        let settings = Settings { style: Style::Breakbeat, complexity: 0.7, ..Default::default() };
        assert_eq!(generate(&kit, &settings, 16.0, &[], 9), generate(&kit, &settings, 16.0, &[], 9));
        let takes: std::collections::HashSet<String> = (0..6).map(|s| format!("{:?}", generate(&kit, &settings, 16.0, &[], s))).collect();
        assert!(takes.len() > 3);
    }

    #[test]
    fn phrases_end_in_a_fill_and_the_kick_can_follow_the_bass() {
        let kit = machine();
        let notes = generate(&kit, &Settings { style: Style::PopRock, complexity: 0.0, fills: true, follow_bass: false }, 16.0, &[], 1);
        // The fourth bar's last beat: sixteenths of snare and tom, no hats.
        let last: Vec<&Note> = notes.iter().filter(|n| n.start >= 15.0).collect();
        assert!(last.iter().any(|n| n.pitch == drum_note::SNARE) && last.iter().any(|n| n.pitch == drum_note::LOW_TOM));
        assert!(!last.iter().any(|n| n.pitch == drum_note::CLOSED_HAT));
        assert_eq!(last.iter().filter(|n| n.pitch != drum_note::KICK).count(), 4);
        // The bass on the «and» of 2 and on 4: kicks there too.
        let bass = [1.5, 3.0, 2.3];
        let follow = generate(&kit, &Settings { style: Style::PopRock, complexity: 0.0, fills: false, follow_bass: true }, 4.0, &bass, 2);
        let kicks: Vec<f64> = follow.iter().filter(|n| n.pitch == drum_note::KICK).map(|n| n.start).collect();
        assert!(kicks.contains(&1.5) && kicks.contains(&3.0), "{kicks:?}");
        assert!(!kicks.contains(&2.3), "not off the eighths");
        // A kit without some sounds still makes a beat with what it has.
        let small = Kit { kick: Some(36), clap: Some(39), ..Default::default() };
        let beat = generate(&small, &Settings { style: Style::DrumAndBass, ..Default::default() }, 8.0, &[], 1);
        assert!(beat.iter().any(|n| n.pitch == 39), "the clap stands in for the snare");
    }
}
