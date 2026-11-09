//! The song's chord track: the chords of the whole song in the time, on a
//! strip at the top of the Arrangement. They are written by hand (the key's
//! chords by degree, a name, a progression), filled from what a clip or a
//! track plays (audio heard, MIDI notes) or from the song's MIDI clips, and
//! followed: «Acompañar» writes a bass, a pad, an arpeggio or a melody over
//! them for the whole song or a part, and a track set to follow them plays its
//! MIDI clips moved onto each chord (its notes, its scale, or the riff carried
//! to its bass) without its notes being changed.

use crate::app::{ClipRef, PulseApp};
use crate::i18n::{fill, music, tr};
use crate::arrangement::Grid;
use crate::project::{Project, Track};
use crate::theme::{self, ACCENT, TEXT, TEXT_DIM};
use eframe::egui::{self, pos2, Align2, Color32, Rect, RichText};
use engine::model::{ArrangementClip, Clip, MidiClip, Note};
use engine::Command;
use serde::{Deserialize, Serialize};
use stems::chord::{self, Chord, Quality};
use stems::key::Key;

const EPS: f64 = 1e-9;
/// How close to a chord's right edge the pointer grabs its length.
const EDGE_GRAB: f32 = 6.0;
/// A note this much early (beats) is on the chord it leads into.
const EARLY: f64 = 0.05;

/// A chord of the song: from `start` (beat) for `length` beats.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SongChord {
    pub start: f64,
    pub length: f64,
    pub chord: Chord,
}

impl SongChord {
    pub fn end(&self) -> f64 {
        self.start + self.length
    }
}

/// How a track's MIDI clips follow the chord track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Follow {
    /// Each note to the nearest of the chord's (a pad, stabs).
    Chords,
    /// Each note to the nearest of the chord's scale (a melody, an arpeggio).
    Scale,
    /// The whole riff carried to each chord's bass, keeping its shape.
    Bass,
}

impl Follow {
    pub const ALL: [Follow; 3] = [Follow::Chords, Follow::Scale, Follow::Bass];

    pub fn label(self) -> &'static str {
        match self {
            Follow::Chords => tr("Notas del acorde"),
            Follow::Scale => tr("Escala del acorde"),
            Follow::Bass => tr("Bajo: mover el riff"),
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Follow::Chords => tr("Cada nota va a la nota del acorde más cercana, sin repetir notas en un mismo golpe: para pads y acordes"),
            Follow::Scale => tr("Cada nota va a la nota más cercana de la escala del acorde: para melodías y arpegios"),
            Follow::Bass => tr("El riff entero se mueve a la nota del bajo de cada acorde y conserva su forma (la nota que más suena hace de raíz): para bajos"),
        }
    }
}

// ----- The chords on the track -------------------------------------------------------

/// Takes `from..to` out of the chords: the ones inside go, the ones across are cut.
pub fn clear(chords: &mut Vec<SongChord>, from: f64, to: f64) {
    let mut kept = Vec::with_capacity(chords.len() + 1);
    for c in chords.drain(..) {
        if c.end() <= from + EPS || c.start >= to - EPS {
            kept.push(c);
            continue;
        }
        if c.start < from - EPS {
            kept.push(SongChord { length: from - c.start, ..c });
        }
        if c.end() > to + EPS {
            kept.push(SongChord { start: to, length: c.end() - to, ..c });
        }
    }
    kept.sort_by(|a, b| a.start.total_cmp(&b.start));
    *chords = kept;
}

/// Writes `chord` over `start..start + length`, over what was there.
pub fn put(chords: &mut Vec<SongChord>, start: f64, length: f64, chord: Chord) {
    if length <= EPS {
        return;
    }
    clear(chords, start, start + length);
    chords.push(SongChord { start, length, chord });
    chords.sort_by(|a, b| a.start.total_cmp(&b.start));
}

/// `found` (song beats) in place of what the track had over `from..to`.
pub fn put_all(chords: &mut Vec<SongChord>, from: f64, to: f64, found: &[(f64, f64, Chord)]) {
    clear(chords, from, to);
    for &(a, b, chord) in found {
        put(chords, a, b - a, chord);
    }
}

/// The chord sounding at `beat`, by index.
pub fn index_at(chords: &[SongChord], beat: f64) -> Option<usize> {
    chords.iter().position(|c| beat >= c.start - EPS && beat < c.end() - EPS)
}

pub fn chord_at(chords: &[SongChord], beat: f64) -> Option<Chord> {
    index_at(chords, beat).map(|i| chords[i].chord)
}

/// The chords over `from..to`, cut to it, in beats from `from`.
pub fn between(chords: &[SongChord], from: f64, to: f64) -> Vec<(f64, f64, Chord)> {
    chords
        .iter()
        .filter(|c| c.end() > from + EPS && c.start < to - EPS)
        .map(|c| (c.start.max(from) - from, c.end().min(to) - from, c.chord))
        .collect()
}

/// Where the last chord ends (0 with none).
pub fn end(chords: &[SongChord]) -> f64 {
    chords.iter().map(|c| c.end()).fold(0.0, f64::max)
}

/// The chords of a clip `lap` beats long (`found`, in its beats) over each
/// lap of it placed at `start` for `length` beats, in song beats; a chord
/// that goes on into the next lap is one.
pub fn tile(found: &[(f64, f64, Chord)], lap: f64, start: f64, length: f64) -> Vec<(f64, f64, Chord)> {
    let mut out: Vec<(f64, f64, Chord)> = Vec::new();
    if lap <= EPS {
        return out;
    }
    let mut at = 0.0;
    while at < length - EPS {
        for &(a, b, chord) in found {
            let (from, to) = (start + at + a, start + (at + b).min(length));
            if to <= from + EPS {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.2 == chord && (last.1 - from).abs() < 1e-6 => last.1 = to,
                _ => out.push((from, to, chord)),
            }
        }
        at += lap;
    }
    out
}

// ----- Names --------------------------------------------------------------------------

/// A chord from what one types: «Lam», «F#m7», «Sol/Si», «Bb», «Do mayor», or
/// with a key, its degree: «vi», «V7», «ii°». None if it isn't one.
pub fn parse(text: &str, key: Option<Key>) -> Option<Chord> {
    let text: String = text.trim().replace('♯', "#").replace('♭', "b");
    if text.is_empty() {
        return None;
    }
    if let Some(chord) = key.and_then(|k| numeral(&text, k)) {
        return Some(chord);
    }
    let (main, bass) = match text.split_once('/') {
        Some((a, b)) => (a, Some(b)),
        None => (text.as_str(), None),
    };
    let (root, rest) = note(main)?;
    let quality = quality(rest)?;
    let mut chord = Chord::new(root, quality);
    if let Some(bass) = bass {
        let (class, rest) = note(bass.trim())?;
        if !rest.trim().is_empty() {
            return None;
        }
        chord.bass = (class != chord.root).then_some(class);
    }
    Some(chord)
}

/// A note's name at the start of `text` (Latin or English, with its sharps and
/// flats): its pitch class and what follows.
fn note(text: &str) -> Option<(u8, &str)> {
    const LATIN: [(&str, i32); 7] = [("sol", 7), ("do", 0), ("re", 2), ("mi", 4), ("fa", 5), ("la", 9), ("si", 11)];
    const ENGLISH: [(char, i32); 7] = [('C', 0), ('D', 2), ('E', 4), ('F', 5), ('G', 7), ('A', 9), ('B', 11)];
    let text = text.trim_start();
    let latin = LATIN.iter().find(|(name, _)| text.get(..name.len()).is_some_and(|start| start.eq_ignore_ascii_case(name)));
    let (mut class, mut rest) = match latin {
        Some(&(name, class)) => (class, &text[name.len()..]),
        None => {
            let first = text.chars().next()?;
            let &(_, class) = ENGLISH.iter().find(|(letter, _)| *letter == first.to_ascii_uppercase())?;
            (class, &text[first.len_utf8()..])
        }
    };
    loop {
        if let Some(r) = rest.strip_prefix('#') {
            class += 1;
            rest = r;
        } else if let Some(r) = rest.strip_prefix('b') {
            class -= 1;
            rest = r;
        } else {
            break;
        }
    }
    Some((class.rem_euclid(12) as u8, rest))
}

/// The chord's kind from what follows its root: «m», «7», «maj7», «sus4»…
fn quality(text: &str) -> Option<Quality> {
    let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    Some(match text.as_str() {
        "" | "M" | "maj" | "mayor" => Quality::Major,
        "m" | "min" | "-" | "menor" => Quality::Minor,
        "°" | "dim" | "o" => Quality::Diminished,
        "+" | "aug" => Quality::Augmented,
        "sus2" => Quality::Sus2,
        "sus4" | "sus" => Quality::Sus4,
        "7" => Quality::Seventh,
        "maj7" | "M7" | "Δ" | "Δ7" | "7M" => Quality::Major7,
        "m7" | "min7" | "-7" => Quality::Minor7,
        "ø" | "ø7" | "m7b5" | "m7(b5)" => Quality::HalfDiminished,
        other => return Quality::ALL.into_iter().find(|q| !q.suffix().is_empty() && q.suffix() == other),
    })
}

/// A degree of `key` as a roman numeral: «IV», «vi», «V7», «vii°».
fn numeral(text: &str, key: Key) -> Option<Chord> {
    let roman: String = text.chars().take_while(|c| matches!(c, 'I' | 'V' | 'i' | 'v')).collect();
    let rest = &text[roman.len()..];
    const NUMERALS: [&str; 7] = ["i", "ii", "iii", "iv", "v", "vi", "vii"];
    let degree = NUMERALS.iter().position(|n| roman.eq_ignore_ascii_case(n))?;
    let sevenths = match rest {
        "" | "°" => false,
        "7" | "ø" | "°7" => true,
        _ => return None,
    };
    // In a minor key a capital V is the major dominant.
    let major_five = key.minor && degree == 4 && roman == "V";
    Some(chord::on_degree(key, degree, sevenths, major_five))
}

// ----- Following ------------------------------------------------------------------------

/// The pitch classes a chord's notes move to: the key's scale with the chord's
/// own notes in it (and the ones a semitone from them that aren't, out), or
/// with no key, the major or minor scale on its root.
pub fn chord_scale(chord: &Chord, key: Option<Key>) -> [bool; 12] {
    let minor = chord.quality.intervals().get(1) == Some(&3);
    let mut scale = key.unwrap_or(Key { tonic: chord.root, minor }).scale();
    let tones = chord.classes();
    for &t in &tones {
        if !scale[t as usize] {
            scale[t as usize] = true;
            for n in [(t + 1) % 12, (t + 11) % 12] {
                if !tones.contains(&n) {
                    scale[n as usize] = false;
                }
            }
        }
    }
    scale
}

/// The pitch nearest `pitch` whose class is `allowed`: a chord note (in
/// `prefer`) before another as near, then the lower; one not `taken` (by
/// another note of the same hit) if there is one within a third.
fn nearest(pitch: u8, allowed: &[bool; 12], prefer: &[u8], taken: &[u8]) -> u8 {
    let candidates = |free: bool, reach: i32| -> Option<u8> {
        for d in 0..=reach {
            let options: Vec<u8> = [pitch as i32 - d, pitch as i32 + d]
                .into_iter()
                .filter(|p| (0..=127).contains(p) && allowed[(*p % 12) as usize])
                .map(|p| p as u8)
                .filter(|p| !free || !taken.contains(p))
                .collect();
            if let Some(&p) = options.iter().find(|p| prefer.contains(&(*p % 12))).or(options.first()) {
                return Some(p);
            }
        }
        None
    };
    candidates(true, 4).or_else(|| candidates(false, 11)).unwrap_or(pitch)
}

/// The class a riff is heard on: the one that sounds longest (the lowest of
/// those that sound as long).
fn riff_root(notes: &[Note]) -> Option<u8> {
    let mut weight = [0f64; 12];
    let mut lowest = [u8::MAX; 12];
    for n in notes {
        let c = (n.pitch % 12) as usize;
        weight[c] += n.length.max(0.01);
        lowest[c] = lowest[c].min(n.pitch);
    }
    (0..12).filter(|&c| weight[c] > 0.0).max_by(|&a, &b| weight[a].total_cmp(&weight[b]).then(lowest[b].cmp(&lowest[a]))).map(|c| c as u8)
}

/// `notes` (from song beat `at`) as they follow `chords` in `mode`; those with
/// no chord under them stay as they are.
pub fn follow_notes(notes: &[Note], at: f64, chords: &[SongChord], mode: Follow, key: Option<Key>) -> Vec<Note> {
    let root = riff_root(notes).unwrap_or(0);
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by(|&a, &b| notes[a].start.total_cmp(&notes[b].start).then(notes[a].pitch.cmp(&notes[b].pitch)));
    let mut out = notes.to_vec();
    let (mut taken, mut onset): (Vec<u8>, f64) = (Vec::new(), f64::NAN);
    for i in order {
        let n = &notes[i];
        let Some(chord) = chord_at(chords, at + n.start + EARLY) else { continue };
        if (n.start - onset).abs() > 1e-6 {
            taken.clear();
            onset = n.start;
        }
        let tones = chord.classes();
        let pitch = match mode {
            Follow::Chords => {
                let mut allowed = [false; 12];
                for &t in &tones {
                    allowed[t as usize] = true;
                }
                nearest(n.pitch, &allowed, &tones, &taken)
            }
            Follow::Scale => nearest(n.pitch, &chord_scale(&chord, key), &tones, &taken),
            Follow::Bass => {
                let target = chord.bass.unwrap_or(chord.root) as i32;
                // The shortest way there: down to a tritone, up to a fourth.
                let shift = (target - root as i32 + 6).rem_euclid(12) - 6;
                let moved = (n.pitch as i32 + shift).clamp(0, 127) as u8;
                nearest(moved, &chord_scale(&chord, key), &tones, &[])
            }
        };
        taken.push(pitch);
        out[i].pitch = pitch;
    }
    out
}

/// How `track` follows the chord track, if it does and there are chords.
pub fn following(project: &Project, track: &Track) -> Option<Follow> {
    track.follow_chords.filter(|_| !project.chords.is_empty() && !track.is_drums() && !track.is_audio() && !track.is_group())
}

/// Each lap of a MIDI clip placed on a track that follows the chords: where it
/// starts, how long it plays and the clip with its notes on the chords there.
pub fn followed_laps(project: &Project, placed: &ArrangementClip, midi: &MidiClip, mode: Follow) -> Vec<(f64, f64, MidiClip)> {
    if midi.length <= EPS {
        return vec![(placed.start, placed.length, midi.clone())];
    }
    let mut laps = Vec::new();
    let mut at = placed.start;
    while at < placed.end() - EPS {
        let notes = follow_notes(&midi.notes, at, &project.chords, mode, project.key);
        laps.push((at, midi.length.min(placed.end() - at), MidiClip { notes, ..midi.clone() }));
        at += midi.length;
    }
    laps
}

/// What a track's arrangement plays: its clips, those of a track that follows
/// the chords lap by lap with their notes on them.
pub fn placed_clips(project: &Project, track: &Track, samples: &mut crate::sync::SampleCache) -> Box<Vec<engine::PlacedClip>> {
    let Some(mode) = following(project, track) else { return crate::sync::placed_clips(track, samples) };
    let mut placed = Vec::new();
    for a in &track.arrangement {
        match &a.clip {
            Clip::Midi(m) => {
                for (start, length, clip) in followed_laps(project, a, m, mode) {
                    placed.push(engine::PlacedClip::new(start, length, engine::PlayClip::Midi(clip)));
                }
            }
            clip => placed.push(engine::PlacedClip::new(a.start, a.length, crate::sync::play_clip(clip, samples))),
        }
    }
    engine::link_neighbours(&mut placed);
    Box::new(placed)
}

/// The notes of the song's MIDI clips over `from..to` (song beats): every
/// track's but drums', groups' and those that follow the chords.
pub fn song_notes(project: &Project, from: f64, to: f64) -> Vec<Note> {
    let mut notes = Vec::new();
    for track in &project.tracks {
        if track.is_drums() || track.is_audio() || track.is_group() || track.follow_chords.is_some() {
            continue;
        }
        for a in &track.arrangement {
            let Clip::Midi(m) = &a.clip else { continue };
            if m.muted || m.length <= EPS || a.end() <= from || a.start >= to {
                continue;
            }
            let mut lap = a.start;
            while lap < a.end().min(to) - EPS {
                let lap_end = (lap + m.length).min(a.end());
                for n in &m.notes {
                    let start = lap + n.start;
                    let end = (start + n.length).min(lap_end).min(to);
                    if n.start < m.length && start < lap_end - EPS && end > from + EPS {
                        let s = start.max(from);
                        notes.push(Note { start: s - from, length: end - s, ..*n });
                    }
                }
                lap += m.length;
            }
        }
    }
    notes
}

// ----- In the app -------------------------------------------------------------------------

/// What the chord strip remembers: the chord grabbed and the menu's settings.
pub struct Lane {
    grab: Option<Grab>,
    /// The chord last clicked, outlined.
    pub selected: Option<usize>,
    /// Where the menu writes the next chord (it moves on after each).
    insert: f64,
    pub sevenths: bool,
    /// Bars each chord written from the menu lasts.
    pub bars: f64,
    /// A progression is repeated to the end of the song.
    pub repeat: bool,
    /// The name typed in the menu.
    pub name: String,
}

impl Default for Lane {
    fn default() -> Self {
        Self { grab: None, selected: None, insert: 0.0, sevenths: false, bars: 1.0, repeat: true, name: String::new() }
    }
}

#[derive(Clone, Copy, Debug)]
struct Grab {
    index: usize,
    /// Its length (the right edge), or the whole chord.
    edge: bool,
    /// Beats from the chord's start to where it was grabbed.
    offset: f64,
    moved: bool,
}

/// What a pointer in the strip did, for the Arrangement.
pub enum Took {
    Pointer,
    /// A right click: the menu, at a beat, over a chord or not.
    Menu { beat: f64, chord: Option<usize> },
}

impl PulseApp {
    /// Sends again what the tracks that follow the chords play (the chords or
    /// the key changed).
    pub fn sync_chord_followers(&mut self) {
        for t in 0..self.project.tracks.len() {
            if self.project.tracks[t].follow_chords.is_some() {
                self.sync_arrangement(t);
            }
        }
    }

    /// The chords that arrangement clip `index` of track `t` plays, in song
    /// beats over all its laps, and where it starts and ends.
    pub(crate) fn placed_chords(&mut self, t: usize, index: usize) -> Option<(f64, f64, Vec<(f64, f64, Chord)>)> {
        let placed = self.project.tracks.get(t)?.arrangement.get(index)?;
        let (start, length) = (placed.start, placed.length);
        let (found, lap) = self.clip_chords(ClipRef::Arrangement { track: t, index });
        Some((start, start + length, tile(&found, lap, start, length)))
    }

    /// The names of `chords`, as the song's key spells them.
    fn chord_names(&self, chords: &[(f64, f64, Chord)]) -> String {
        let names: Vec<String> = chords.iter().take(8).map(|c| music(&c.2.name_in(self.project.key))).collect();
        let more = if chords.len() > 8 { " …" } else { "" };
        format!("{}{more}", names.join(" "))
    }

    /// The chords an arrangement clip plays (a MIDI clip's notes, an audio
    /// clip's heard), onto the chord track in its place.
    pub fn chords_from_clip(&mut self, clip: ClipRef) {
        let ClipRef::Arrangement { track, index } = clip else {
            self.status = Some(tr("La pista de acordes se rellena desde clips del Arrangement").into());
            return;
        };
        let name = self.clip(clip).map(|c| c.name().to_string()).unwrap_or_default();
        let Some((from, to, found)) = self.placed_chords(track, index) else { return };
        if found.is_empty() {
            self.status = Some(fill(tr("No se oyen acordes en «{}»"), &[&name]));
            return;
        }
        put_all(&mut self.project.chords, from, to, &found);
        self.sync_chord_followers();
        self.status = Some(fill(tr("{} acordes de «{}» en la pista de acordes: {}"), &[&found.len(), &name, &self.chord_names(&found)]));
    }

    /// The chords every arrangement clip of track `t` plays, onto the chord track.
    pub fn chords_from_track(&mut self, t: usize) {
        let Some(track) = self.project.tracks.get(t) else { return };
        let name = track.name.clone();
        let mut all = Vec::new();
        for index in 0..track.arrangement.len() {
            if let Some((from, to, found)) = self.placed_chords(t, index) {
                put_all(&mut self.project.chords, from, to, &found);
                all.extend(found);
            }
        }
        if all.is_empty() {
            self.status = Some(fill(tr("No se oyen acordes en «{}»"), &[&name]));
            return;
        }
        self.sync_chord_followers();
        self.status = Some(fill(tr("{} acordes de «{}» en la pista de acordes: {}"), &[&all.len(), &name, &self.chord_names(&all)]));
    }

    /// The chords the song's MIDI clips play together, in place of the chord
    /// track's; how many.
    pub fn chords_from_song(&mut self) -> usize {
        let to = self.project.tracks.iter().map(|t| t.arrangement_end()).fold(0.0, f64::max);
        let found = song_chords(&self.project, 0.0, to);
        if found.is_empty() {
            self.status = Some(tr("Los clips MIDI de la canción no tocan acordes").into());
            return 0;
        }
        self.project.chords.clear();
        put_all(&mut self.project.chords, 0.0, to, &found);
        self.sync_chord_followers();
        self.status = Some(fill(tr("{} acordes de la canción en la pista de acordes: {}"), &[&found.len(), &self.chord_names(&found)]));
        found.len()
    }

    /// The chord to write after what comes before `at`: the first of those
    /// that usually follow it, or the key's first (Do with no key).
    fn next_song_chord(&self, at: f64) -> Chord {
        let key = self.project.key.unwrap_or(Key { tonic: 0, minor: false });
        let sevenths = self.chord_lane.sevenths;
        let diatonic = chord::diatonic(key, sevenths);
        let previous = self.project.chords.iter().rev().find(|c| c.start < at - EPS).map(|c| c.chord);
        let next = previous.and_then(|p| diatonic.iter().position(|d| d.root == p.root)).and_then(|d| chord::next_degrees(key, d).first().copied());
        chord::on_degree(key, next.unwrap_or(0), sevenths, false)
    }

    /// A chord written by a double click at `beat`: from its bar (or where the
    /// chord there ends), a bar long or up to the next chord.
    pub fn add_song_chord(&mut self, beat: f64) {
        let meter = self.project.meter;
        let mut start = meter.prev_bar(beat.max(0.0));
        if let Some(i) = index_at(&self.project.chords, start) {
            start = self.project.chords[i].end();
        }
        let mut length = meter.bar_length_at(start);
        if let Some(next) = self.project.chords.iter().find(|c| c.start > start + EPS) {
            length = length.min(next.start - start);
        }
        if length <= EPS {
            return;
        }
        let chord = self.next_song_chord(start);
        put(&mut self.project.chords, start, length, chord);
        self.chord_lane.selected = index_at(&self.project.chords, start);
        self.sync_chord_followers();
        self.status = Some(fill(tr("{} en la pista de acordes: clic derecho para cambiarlo"), &[&music(&chord.name_in(self.project.key))]));
    }

    /// Writes `chords` from the menu's place on, each `length` beats (a
    /// progression, again and again to the song's end if asked); the place
    /// moves on after them.
    fn write_song_chords(&mut self, chords: &[Chord], length: f64, repeat: bool) {
        let mut at = self.chord_lane.insert;
        let song_end = self.project.tracks.iter().map(|t| t.arrangement_end()).fold(end(&self.project.chords), f64::max);
        loop {
            for &c in chords {
                put(&mut self.project.chords, at, length, c);
                at += length;
            }
            if !repeat || at >= song_end - EPS {
                break;
            }
        }
        self.chord_lane.insert = at;
        self.sync_chord_followers();
    }

    /// The part of the song `beat` is in: from its marker to the next (or the
    /// song's end), and the marker's name.
    pub fn section_at(&self, beat: f64) -> Option<(String, f64, f64)> {
        let markers = &self.project.markers;
        let i = markers.iter().rposition(|m| m.beat <= beat + EPS)?;
        let song_end = self.project.tracks.iter().map(|t| t.arrangement_end()).fold(end(&self.project.chords), f64::max);
        let to = markers.get(i + 1).map_or(song_end, |m| m.beat);
        (to > markers[i].beat + EPS).then(|| (markers[i].name.clone(), markers[i].beat, to))
    }

    /// Writes into the clips of track `t` (or only clip `only`) the notes they
    /// play following the chords in `mode`; a whole track stops following.
    /// A clip that loops becomes one with all its laps (with its own clip
    /// envelopes or controls, a clip a lap).
    pub fn apply_chord_follow(&mut self, t: usize, mode: Follow, only: Option<usize>) {
        let Some(track) = self.project.tracks.get(t) else { return };
        if self.project.chords.is_empty() {
            self.status = Some(tr("La pista de acordes está vacía").into());
            return;
        }
        let mut arrangement = Vec::new();
        let mut changed = 0;
        for (index, placed) in track.arrangement.iter().enumerate() {
            let Clip::Midi(m) = &placed.clip else {
                arrangement.push(placed.clone());
                continue;
            };
            if only.is_some_and(|o| o != index) {
                arrangement.push(placed.clone());
                continue;
            }
            changed += 1;
            let laps = followed_laps(&self.project, placed, m, mode);
            if laps.len() == 1 || !m.envelopes.is_empty() || !m.controls.is_empty() {
                arrangement.extend(laps.into_iter().map(|(start, length, clip)| ArrangementClip { start, length, clip: Clip::Midi(clip) }));
            } else {
                let notes = laps.iter().flat_map(|(start, length, clip)| {
                    let offset = start - placed.start;
                    clip.notes.iter().filter(|n| n.start < *length - EPS).map(move |n| Note { start: n.start + offset, ..*n })
                });
                let clip = MidiClip { notes: notes.collect(), length: placed.length, ..m.clone() };
                arrangement.push(ArrangementClip { start: placed.start, length: placed.length, clip: Clip::Midi(clip) });
            }
        }
        let name = track.name.clone();
        self.project.tracks[t].arrangement = arrangement;
        if only.is_none() {
            self.project.tracks[t].follow_chords = None;
        }
        self.set_clip_selection(Vec::new());
        self.sync_arrangement(t);
        self.status = Some(fill(tr("{} clip(s) de «{}» ajustados a la pista de acordes ({})"), &[&changed, &name, &mode.label().to_lowercase()]));
    }

    /// Has track `t` follow the chord track in `mode` (None: no longer).
    pub fn set_chord_follow(&mut self, t: usize, mode: Option<Follow>) {
        let Some(track) = self.project.tracks.get_mut(t) else { return };
        track.follow_chords = mode;
        let name = track.name.clone();
        self.sync_arrangement(t);
        self.status = Some(match mode {
            Some(m) if self.project.chords.is_empty() => fill(tr("«{}» seguirá la pista de acordes ({}) cuando tenga acordes"), &[&name, &m.label().to_lowercase()]),
            Some(m) => fill(tr("«{}» sigue la pista de acordes ({}): sus notas no cambian, cambia lo que suena"), &[&name, &m.label().to_lowercase()]),
            None => fill(tr("«{}» ya no sigue la pista de acordes"), &[&name]),
        });
    }
}

/// The chord track's chords over `start..start + length` (in beats from
/// `start`) if they cover it and `melody` (from `start`) fits them: half of
/// what it sings, at least, on their notes.
pub fn fitting(chords: &[SongChord], melody: &[Note], start: f64, length: f64) -> Option<Vec<(f64, f64, Chord)>> {
    let found = between(chords, start, start + length);
    let covered: f64 = found.iter().map(|c| c.1 - c.0).sum();
    if covered < 0.9 * length {
        return None;
    }
    let (mut on, mut sung) = (0.0, 0.0);
    for n in melody {
        if let Some(chord) = chord_at(chords, start + n.start + EARLY) {
            sung += n.length;
            if chord.classes().contains(&(n.pitch % 12)) {
                on += n.length;
            }
        }
    }
    (sung > 0.0 && on >= 0.5 * sung).then_some(found)
}

/// The chords the song's MIDI clips play over `from..to`, in song beats.
pub fn song_chords(project: &Project, from: f64, to: f64) -> Vec<(f64, f64, Chord)> {
    let notes = song_notes(project, from, to);
    crate::harmony::chords(&notes, to - from, project.key, project.meter.sig_at(from))
        .into_iter()
        .filter_map(|(a, b, c)| c.map(|c| (from + a, from + b, c)))
        .collect()
}

// ----- The strip ----------------------------------------------------------------------------

/// The chords in their strip (`strip`, the timeline's width), the one
/// sounding brighter; out of the key's, violet.
pub fn paint(app: &PulseApp, painter: &egui::Painter, strip: Rect, x_at: &dyn Fn(f64) -> f32, view: Rect) {
    painter.rect_filled(strip, 0.0, theme::shade(theme::SLOT, 0.7));
    let key = app.project.key;
    let position = app.audio.position();
    let scale = key.map(|k| k.scale());
    for (i, c) in app.project.chords.iter().enumerate() {
        let rect = Rect::from_min_max(pos2(x_at(c.start) + 1.0, strip.top() + 2.0), pos2((x_at(c.end()) - 1.0).max(x_at(c.start) + 2.0), strip.bottom() - 2.0));
        if rect.right() < view.left() || rect.left() > view.right() {
            continue;
        }
        let in_key = scale.is_none_or(|s| c.chord.classes().iter().all(|&t| s[t as usize]));
        let base = if in_key { ACCENT } else { Color32::from_rgb(0xa0, 0x8c, 0xff) };
        let now = position >= c.start && position < c.end();
        painter.rect_filled(rect, 2.0, theme::shade(base, if now { 0.6 } else { 0.32 }));
        if app.chord_lane.selected == Some(i) {
            crate::widgets::outline(painter, rect, base);
        }
        let name = music(&c.chord.name_in(key));
        let label = match key.and_then(|k| c.chord.numeral(k)) {
            Some(n) if rect.width() > 70.0 => format!("{name} · {n}"),
            _ => name,
        };
        let clipped = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
        clipped.text(pos2(rect.left().max(view.left()) + 4.0, rect.center().y), Align2::LEFT_CENTER, label, theme::small(), TEXT);
    }
    if app.project.chords.is_empty() {
        let hint = tr("Pista de acordes: doble clic escribe uno · clic derecho: grados, progresiones o sacarlos de un audio");
        painter.text(pos2(view.left().max(strip.left()) + 6.0, strip.center().y), Align2::LEFT_CENTER, hint, theme::small(), TEXT_DIM);
    }
}

/// Clicks and drags in the chord strip: a double click on a free place writes
/// a chord, dragging one moves it, its right edge its length, a click jumps
/// there, a right click opens the menu. None if the pointer isn't its.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle(app: &mut PulseApp, ui: &egui::Ui, response: &egui::Response, strip: Rect, pos: egui::Pos2, beat: f64, unit: Grid, origin_x: f32) -> Option<Took> {
    let ppb = app.arrangement.pixels_per_beat;
    if let Some(grab) = app.chord_lane.grab {
        if response.dragged() {
            drag(app, grab, unit, beat);
        }
        if ui.input(|i| i.pointer.primary_released()) {
            let moved = app.chord_lane.grab.is_some_and(|g| g.moved);
            if !moved && !grab.edge {
                if let Some(c) = app.project.chords.get(grab.index) {
                    app.audio.send(Command::SetPosition(c.start));
                }
            }
            app.chord_lane.grab = None;
        }
        return Some(Took::Pointer);
    }
    if pos.y < strip.top() || pos.y >= strip.bottom() {
        return None;
    }
    let under = index_at(&app.project.chords, beat);
    let edge = under.is_some_and(|i| origin_x + app.project.chords[i].end() as f32 * ppb - pos.x <= EDGE_GRAB);
    if response.secondary_clicked() {
        let at = unit.snap(beat);
        app.chord_lane.insert = under.map_or(at, |i| app.project.chords[i].start);
        app.chord_lane.selected = under;
        return Some(Took::Menu { beat: at, chord: under });
    }
    if response.double_clicked() && under.is_none() {
        app.add_song_chord(beat);
        return Some(Took::Pointer);
    }
    if ui.input(|i| i.pointer.primary_pressed()) && response.hovered() {
        app.chord_lane.selected = under;
        if let Some(index) = under {
            let offset = beat - app.project.chords[index].start;
            app.chord_lane.grab = Some(Grab { index, edge, offset, moved: false });
        }
    }
    if edge {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    } else if under.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    Some(Took::Pointer)
}

/// A grabbed chord follows the pointer: its length, or where it starts,
/// between its neighbours.
fn drag(app: &mut PulseApp, grab: Grab, unit: Grid, beat: f64) {
    let chords = &app.project.chords;
    let Some(&c) = chords.get(grab.index) else { return };
    let prev_end = grab.index.checked_sub(1).map_or(0.0, |p| chords[p].end());
    let next_start = chords.get(grab.index + 1).map_or(f64::INFINITY, |n| n.start);
    let (start, length) = if grab.edge {
        let end = unit.snap(beat).max(c.start + unit.length_at(c.start)).min(next_start);
        (c.start, end - c.start)
    } else {
        let start = unit.snap(beat - grab.offset).max(prev_end).min(next_start - c.length);
        (start, c.length)
    };
    if length > EPS && ((start - c.start).abs() > EPS || (length - c.length).abs() > EPS) {
        app.project.chords[grab.index] = SongChord { start, length, ..c };
        app.chord_lane.grab = Some(Grab { moved: true, ..grab });
        app.sync_chord_followers();
    }
}

/// The strip's right-click menu: the key's chords by degree, a name, the
/// length, progressions, where to take them from, and what follows them.
pub(crate) fn menu(app: &mut PulseApp, ui: &mut egui::Ui, beat: f64, index: Option<usize>) {
    ui.set_min_width(300.0);
    let key = app.project.key;
    let meter = app.project.meter;
    let bar = meter.bar_length_at(beat);
    let length = app.chord_lane.bars * bar;
    let over = index.and_then(|i| app.project.chords.get(i).copied());
    let mut chosen: Option<Chord> = None;
    match over {
        Some(c) => {
            ui.label(RichText::new(fill(tr("Acorde {}"), &[&music(&c.chord.name_in(key))])).strong());
        }
        None => {
            ui.label(RichText::new(tr("Pista de acordes")).strong());
        }
    }
    match key {
        None => {
            ui.label(RichText::new(tr("Elige una Escala en la barra de transporte (o detéctala) para ver sus acordes por grados. Puedes escribir el nombre de cualquier acorde abajo.")).color(TEXT_DIM).size(10.5));
        }
        Some(key) => {
            let chords = chord::diatonic(key, app.chord_lane.sevenths);
            let at = app.chord_lane.insert;
            let previous = app.project.chords.iter().rev().find(|c| c.start < at - EPS).map(|c| c.chord);
            let suggested: &[usize] = previous.and_then(|p| chords.iter().position(|d| d.root == p.root)).map_or(&[], |d| chord::next_degrees(key, d));
            let hint = if over.is_some() { tr("Clic en uno para cambiarlo") } else { tr("Se escriben uno detrás de otro desde donde hiciste clic") };
            ui.label(RichText::new(format!("{} · {hint}", music(&key.name()))).color(TEXT_DIM).size(10.5));
            egui::Grid::new("song_chords").spacing(egui::vec2(3.0, 3.0)).show(ui, |ui| {
                for (d, c) in chords.iter().enumerate() {
                    let numeral = c.numeral(key).unwrap_or_default();
                    let mut text = RichText::new(format!("{numeral}\n{}", music(&c.name_in(Some(key)))));
                    if suggested.contains(&d) && over.is_none() {
                        text = text.color(ACCENT);
                    }
                    if ui.add(egui::Button::new(text).min_size(egui::vec2(38.0, 34.0))).clicked() {
                        chosen = Some(*c);
                    }
                }
                ui.end_row();
            });
            if key.minor {
                let five = chord::on_degree(key, 4, app.chord_lane.sevenths, true);
                if ui.button(fill(tr("V {} (menor armónica)"), &[&music(&five.name_in(Some(key)))])).on_hover_text(tr("La dominante mayor, que lleva con más fuerza a la tónica")).clicked() {
                    chosen = Some(five);
                }
            }
        }
    }
    ui.horizontal(|ui| {
        ui.label(tr("Nombre"));
        let field = ui.add(egui::TextEdit::singleline(&mut app.chord_lane.name).desired_width(110.0).hint_text(tr("Lam, F#m7, Sol/Si…")));
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button(tr("Poner")).on_hover_text(tr("Cualquier acorde por su nombre (Do, Lam7, Fa#m, Sib/Re, Gsus4…) o, con escala, por su grado (vi, V7)")).clicked() || enter {
            match parse(&app.chord_lane.name, key) {
                Some(c) => chosen = Some(c),
                None => app.status = Some(fill(tr("«{}» no es un acorde que Pulse entienda: prueba Lam, F#m7 o Sol/Si"), &[&app.chord_lane.name.trim()])),
            }
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.chord_lane.sevenths, tr("Con séptima"));
        ui.label(RichText::new(tr("Duración")).color(TEXT_DIM).size(10.5));
        for (bars, label) in [(0.5, tr("½ c.")), (1.0, tr("1 c.")), (2.0, tr("2 c.")), (4.0, tr("4 c."))] {
            ui.selectable_value(&mut app.chord_lane.bars, bars, label);
        }
    });
    if let Some(c) = chosen {
        match (index, over) {
            (Some(i), Some(_)) => {
                app.project.chords[i].chord = c;
                app.sync_chord_followers();
                ui.close();
            }
            _ => app.write_song_chords(&[c], length, false),
        }
        app.status = Some(fill(tr("{} en la pista de acordes"), &[&music(&c.name_in(key))]));
    }
    if let (Some(i), Some(_)) = (index, over) {
        if ui.button(tr("Eliminar acorde")).clicked() {
            app.project.chords.remove(i);
            app.chord_lane.selected = None;
            app.sync_chord_followers();
            ui.close();
        }
    }
    if let Some(key) = key {
        crate::widgets::menu_button(ui, tr("Progresiones"), |ui| {
            ui.checkbox(&mut app.chord_lane.repeat, tr("Repetir hasta el final de la canción"));
            for progression in chord::progressions(key) {
                let chords: Vec<Chord> = progression.degrees.iter().map(|&d| chord::on_degree(key, d, app.chord_lane.sevenths, progression.major_five)).collect();
                let names: Vec<String> = chords.iter().map(|c| music(&c.name_in(Some(key)))).collect();
                if ui.button(tr(progression.name)).on_hover_text(names.join(" – ")).clicked() {
                    app.write_song_chords(&chords, length, app.chord_lane.repeat);
                    app.status = Some(fill(tr("{} en la pista de acordes"), &[&names.join(" ")]));
                    ui.close();
                }
            }
        });
    }
    ui.separator();
    crate::widgets::menu_button(ui, tr("Sacar los acordes de"), |ui| {
        let sources: Vec<(usize, String)> = app
            .project
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.is_drums() && !t.is_group() && t.follow_chords.is_none() && !t.arrangement.is_empty())
            .map(|(i, t)| (i, format!("{} ({})", t.name, if t.is_audio() { "audio" } else { "MIDI" })))
            .collect();
        if sources.is_empty() {
            ui.label(RichText::new(tr("Ninguna pista tiene clips en el Arrangement")).color(TEXT_DIM).size(10.5));
        }
        for (t, label) in sources {
            if ui.button(label).on_hover_text(tr("Lee los acordes de sus clips (un audio, oyéndolo) y los escribe en la pista de acordes donde están")).clicked() {
                app.chords_from_track(t);
                ui.close();
            }
        }
        ui.separator();
        if ui.button(tr("Todos los clips MIDI de la canción")).on_hover_text(tr("Los acordes que suenan juntando las pistas MIDI (sin la batería), en lugar de los que hay")).clicked() {
            app.chords_from_song();
            ui.close();
        }
    });
    if !app.project.chords.is_empty() {
        let song = (meter.prev_bar(app.project.chords[0].start), end(&app.project.chords));
        accompany_menu(app, ui, tr("Acompañar toda la canción"), song);
        if let Some((name, from, to)) = app.section_at(beat) {
            accompany_menu(app, ui, &fill(tr("Acompañar «{}»"), &[&name]), (from, to));
        }
        if ui.button(tr("Borrar todos los acordes")).clicked() {
            app.project.chords.clear();
            app.chord_lane.selected = None;
            app.sync_chord_followers();
            ui.close();
        }
    }
}

/// «Acompañar …»: the parts, written over the chord track's chords in `range`.
fn accompany_menu(app: &mut PulseApp, ui: &mut egui::Ui, label: &str, range: (f64, f64)) {
    crate::widgets::menu_button(ui, label, |ui| {
        for part in crate::accompany::Part::ALL {
            if ui.button(part.label()).on_hover_text(part.hint()).clicked() {
                app.accompany_chords(part, range.0, range.1);
                ui.close();
            }
        }
        ui.separator();
        for part in crate::accompany::Part::melodies() {
            if ui.button(part.label()).on_hover_text(part.hint()).clicked() {
                app.accompany_chords(part, range.0, range.1);
                ui.close();
            }
        }
    });
}

/// The track menu's «Seguir la pista de acordes»: how, or not, and writing
/// what it plays into its notes.
pub(crate) fn track_items(app: &mut PulseApp, ui: &mut egui::Ui, t: usize) {
    let Some(track) = app.project.tracks.get(t) else { return };
    if track.is_drums() || track.is_audio() || track.is_group() {
        return;
    }
    let now = track.follow_chords;
    crate::widgets::menu_button(ui, tr("Seguir la pista de acordes"), |ui| {
        let mark = |on: bool| if on { "✓ " } else { "    " };
        if ui.button(format!("{}{}", mark(now.is_none()), tr("No"))).on_hover_text(tr("Sus clips suenan con sus notas, como están")).clicked() {
            app.set_chord_follow(t, None);
            ui.close();
        }
        for mode in Follow::ALL {
            if ui.button(format!("{}{}", mark(now == Some(mode)), mode.label())).on_hover_text(mode.hint()).clicked() {
                app.set_chord_follow(t, Some(mode));
                ui.close();
            }
        }
        if let Some(mode) = now {
            ui.separator();
            if ui.add_enabled(!app.project.chords.is_empty(), egui::Button::new(tr("Escribirlo en las notas"))).on_hover_text(tr("Las notas de sus clips pasan a ser las que suenan, y la pista deja de seguir los acordes")).clicked() {
                app.apply_chord_follow(t, mode, None);
                ui.close();
            }
        }
    });
}

/// A MIDI clip's «Ajustar a la pista de acordes»: its notes moved onto them for good.
pub(crate) fn clip_items(app: &mut PulseApp, ui: &mut egui::Ui, track: usize, index: usize) {
    if app.project.chords.is_empty() {
        return;
    }
    crate::widgets::menu_button(ui, tr("Ajustar a la pista de acordes"), |ui| {
        for mode in Follow::ALL {
            if ui.button(mode.label()).on_hover_text(mode.hint()).clicked() {
                app.apply_chord_follow(track, mode, Some(index));
                ui.close();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(chords: &[SongChord]) -> Vec<(f64, f64, String)> {
        chords.iter().map(|c| (c.start, c.end(), c.chord.name())).collect()
    }

    #[test]
    fn names_are_read_in_latin_english_and_degrees() {
        let a_minor = Key { tonic: 9, minor: true };
        let name = |t: &str, key| parse(t, key).map(|c| c.name());
        assert_eq!(name("Lam", None).as_deref(), Some("Lam"));
        assert_eq!(name("Am", None).as_deref(), Some("Lam"));
        assert_eq!(name("F#m7", None).as_deref(), Some("Fa#m7"));
        assert_eq!(name("Sol/Si", None).as_deref(), Some("Sol/Si"));
        assert_eq!(name("Bb", None).as_deref(), Some("Sib"));
        assert_eq!(name("Do mayor", None).as_deref(), Some("Do"));
        assert_eq!(name("Gsus4", None).as_deref(), Some("Solsus4"));
        assert_eq!(name("Cmaj7", None).as_deref(), Some("Domaj7"));
        assert_eq!(name("Bm7b5", None).as_deref(), Some("Siø"));
        assert_eq!(name("vi", Some(Key { tonic: 0, minor: false })).as_deref(), Some("Lam"));
        assert_eq!(name("V7", Some(a_minor)).as_deref(), Some("Mi7"), "a capital V in minor is major");
        assert_eq!(name("iv", Some(a_minor)).as_deref(), Some("Rem"));
        assert_eq!(parse("Hola", None), None);
        assert_eq!(parse("Lam xyz", None), None);
        assert_eq!(parse("", None), None);
    }

    #[test]
    fn writing_over_chords_cuts_them() {
        let (am, f, c) = (Chord::new(9, Quality::Minor), Chord::new(5, Quality::Major), Chord::new(0, Quality::Major));
        let mut track = Vec::new();
        put(&mut track, 0.0, 8.0, am);
        put(&mut track, 8.0, 8.0, f);
        put(&mut track, 6.0, 4.0, c);
        assert_eq!(named(&track), [(0.0, 6.0, "Lam".into()), (6.0, 10.0, "Do".into()), (10.0, 16.0, "Fa".into())]);
        assert_eq!(between(&track, 4.0, 12.0), [(0.0, 2.0, am), (2.0, 6.0, c), (6.0, 8.0, f)]);
        clear(&mut track, 7.0, 9.0);
        assert_eq!(named(&track), [(0.0, 6.0, "Lam".into()), (6.0, 7.0, "Do".into()), (9.0, 10.0, "Do".into()), (10.0, 16.0, "Fa".into())]);
        // A clip's chords over its laps: the last lap cut, one chord across laps.
        let found = [(0.0, 4.0, am), (4.0, 8.0, f)];
        let laps = tile(&found, 8.0, 16.0, 20.0);
        assert_eq!(laps, [(16.0, 20.0, am), (20.0, 24.0, f), (24.0, 28.0, am), (28.0, 32.0, f), (32.0, 36.0, am)]);
        assert_eq!(tile(&[(0.0, 4.0, am)], 4.0, 0.0, 12.0), [(0.0, 12.0, am)]);
    }

    /// A pad written on Do (C E G) over Am, F, C, G: each hit on the chord's
    /// notes, three different ones, close to where they were. A riff on La
    /// carried to each bass keeps its shape; a melody stays in the scale.
    #[test]
    fn notes_follow_each_chord() {
        let key = Some(Key { tonic: 9, minor: true });
        let chords: Vec<SongChord> = [(9, Quality::Minor), (5, Quality::Major), (0, Quality::Major), (7, Quality::Major)]
            .iter()
            .enumerate()
            .map(|(i, &(r, q))| SongChord { start: i as f64 * 4.0, length: 4.0, chord: Chord::new(r, q) })
            .collect();
        let note = |start: f64, pitch: u8| Note { start, length: 1.0, pitch, velocity: 0.8, chance: 1.0 };
        let pad = [note(0.0, 60), note(0.0, 64), note(0.0, 67)];
        for bar in 0..4 {
            let moved = follow_notes(&pad, bar as f64 * 4.0, &chords, Follow::Chords, key);
            let tones = chords[bar].chord.classes();
            let mut classes: Vec<u8> = moved.iter().map(|n| n.pitch % 12).collect();
            assert!(classes.iter().all(|c| tones.contains(c)), "bar {bar}: {moved:?}");
            classes.sort();
            classes.dedup();
            assert_eq!(classes.len(), 3, "bar {bar}: {moved:?}");
            assert!(moved.iter().zip(&pad).all(|(m, p)| (m.pitch as i32 - p.pitch as i32).abs() <= 4));
        }
        // La, Do, Mi, La (the riff's root is La): on Fa it's Fa, La, Do, Fa.
        let riff = [note(0.0, 45), note(1.0, 48), note(2.0, 52), note(3.0, 45)];
        let on_f: Vec<u8> = follow_notes(&riff, 4.0, &chords, Follow::Bass, key).iter().map(|n| n.pitch).collect();
        assert_eq!(on_f, [41, 45, 48, 41]);
        let on_g: Vec<u8> = follow_notes(&riff, 12.0, &chords, Follow::Bass, key).iter().map(|n| n.pitch).collect();
        assert_eq!(on_g, [43, 47, 50, 43], "Sol mayor: Si, not Sib");
        // A melody with a note off the scale: in it, and on Sol mayor its Si.
        let tune = [note(0.0, 70), note(1.0, 71), note(2.0, 72)];
        let on_g = follow_notes(&tune, 12.0, &chords, Follow::Scale, key);
        assert!(on_g.iter().all(|n| chord_scale(&chords[3].chord, key)[(n.pitch % 12) as usize]), "{on_g:?}");
        // Where there is no chord, the notes stay.
        assert_eq!(follow_notes(&pad, 40.0, &chords, Follow::Chords, key), pad);
    }

    /// Am, F, C and G, a bar each, twice: three voices and the root an octave
    /// under them, at 120 BPM (2 s a chord), written to a WAV.
    fn chords_wav() -> String {
        let rate = 48_000u32;
        let seconds = 16.0;
        let chords: [[f32; 4]; 4] = [[45.0, 57.0, 60.0, 64.0], [41.0, 57.0, 60.0, 65.0], [48.0, 55.0, 60.0, 64.0], [43.0, 55.0, 59.0, 62.0]];
        let n = (seconds * rate as f32) as usize;
        let sound: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let chord = chords[(t / 2.0) as usize % 4];
                let env = 0.4 + 0.6 * (-(t % 2.0) * 1.2).exp();
                chord.iter().map(|&p| (std::f32::consts::TAU * 440.0 * 2f32.powf((p - 69.0) / 12.0) * t).sin()).sum::<f32>() * 0.15 * env
            })
            .collect();
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("pista-de-acordes-{}.wav", std::process::id()));
        crate::export::write(&path, &sound, &sound, rate, crate::export::BitDepth::Float32, 1.0).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn audio_clip(path: &str, beats: f64) -> engine::model::AudioClip {
        engine::model::AudioClip {
            hard_start: false,
            fades: Default::default(),
            name: "Piano".into(),
            color: [0; 3],
            sample: path.into(),
            length: beats,
            offset: 0.0,
            warp: engine::model::WarpMode::Off,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(),
            muted: false,
        }
    }

    fn names(app: &PulseApp) -> Vec<(f64, String)> {
        app.project.chords.iter().map(|c| (c.start, c.chord.name_in(app.project.key))).collect()
    }

    /// The whole flow: the chords heard in an audio onto the chord track, with
    /// their rhythm; a bass and a pad for the whole song that follow them; a
    /// chord changed, and «Otra variación» plays the new one.
    #[test]
    fn an_audio_s_chords_fill_the_track_and_a_bass_and_a_pad_follow_them() {
        let mut project = Project::empty();
        project.tempo = 120.0;
        project.tracks.clear();
        let mut piano = Track::new_audio("Piano", [0; 3]);
        piano.arrangement.push(ArrangementClip { start: 0.0, length: 32.0, clip: Clip::Audio(audio_clip(&chords_wav(), 32.0)) });
        project.tracks.push(piano);
        let mut app = PulseApp::headless(project);
        app.chords_from_track(0);
        let expected: Vec<(f64, String)> = (0..8).map(|bar| (bar as f64 * 4.0, ["Lam", "Fa", "Do", "Sol"][bar % 4].to_string())).collect();
        assert_eq!(names(&app), expected, "{:?}", app.status);
        assert!(app.project.chords.iter().all(|c| c.length == 4.0));
        assert!(app.status.as_deref().unwrap().starts_with("8 acordes de «Piano»"), "{:?}", app.status);

        // A bass for the whole song: the root of each chord on each bar's first beat.
        app.accompany_chords(crate::accompany::Part::BassRoot, 0.0, end(&app.project.chords));
        let bass = app.project.tracks.iter().position(|t| t.name == "Bajo").expect("a bass track");
        let placed = &app.project.tracks[bass].arrangement[0];
        assert_eq!((placed.start, placed.length), (0.0, 32.0));
        let Clip::Midi(line) = &placed.clip else { panic!() };
        for bar in 0..8 {
            let first = line.notes.iter().find(|n| n.start == bar as f64 * 4.0).expect("a note on the bar");
            assert_eq!(first.pitch % 12, app.project.chords[bar].chord.root, "bar {bar}");
        }
        // A pad: every note in its bar's chord.
        app.accompany_chords(crate::accompany::Part::Pad, 0.0, 32.0);
        let pad = app.project.tracks.iter().position(|t| t.name == "Pad").expect("a pad track");
        let in_chords = |app: &PulseApp| {
            let Clip::Midi(m) = &app.project.tracks[pad].arrangement[0].clip else { panic!() };
            m.notes.iter().all(|n| chord_at(&app.project.chords, n.start + 0.01).is_some_and(|c| c.classes().contains(&(n.pitch % 12))))
        };
        assert!(in_chords(&app));
        // Bar 3 becomes Mi mayor: another take plays it.
        app.project.chords[2].chord = Chord::new(4, Quality::Major);
        app.vary_accompaniment();
        assert!(in_chords(&app), "the pad reads the chord track again");
        let Clip::Midi(m) = &app.project.tracks[pad].arrangement[0].clip else { panic!() };
        assert!(m.notes.iter().any(|n| (8.0..12.0).contains(&n.start) && n.pitch % 12 == 8), "Sol# in Mi mayor");
        // A part of the song: from its marker to the next.
        app.add_marker(16.0);
        app.project.markers[0].name = "Estribillo".into();
        let (name, from, to) = app.section_at(20.0).unwrap();
        assert_eq!((name.as_str(), from, to), ("Estribillo", 16.0, 32.0));
        app.accompany_chords(crate::accompany::Part::ArpUp, from, to);
        let arp = app.project.tracks.iter().position(|t| t.name == "Arpegio").unwrap();
        let placed = &app.project.tracks[arp].arrangement[0];
        assert_eq!((placed.start, placed.length), (16.0, 16.0));
    }

    /// One bar of a pad on Do, looping over Am F C G, on a track that follows
    /// the chord track: rendered, the chords heard are the track's. Its notes
    /// stay as written; written into them, they are what was heard.
    #[test]
    fn a_track_that_follows_the_chord_track_sounds_in_its_chords() {
        let mut project = Project::empty();
        project.tempo = 120.0;
        project.tracks.truncate(1);
        let note = |pitch| Note { start: 0.0, length: 4.0, pitch, velocity: 0.8, chance: 1.0 };
        let pad = MidiClip { notes: vec![note(60), note(64), note(67)], ..MidiClip::new("Pad", 4.0, [0; 3]) };
        project.tracks[0].arrangement.push(ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Midi(pad.clone()) });
        let progression = [(9, Quality::Minor), (5, Quality::Major), (0, Quality::Major), (7, Quality::Major)];
        for (bar, &(root, q)) in progression.iter().enumerate() {
            put(&mut project.chords, bar as f64 * 4.0, 4.0, Chord::new(root, q));
        }
        let heard = |project: &Project| -> Vec<Option<String>> {
            let mut samples = crate::sync::SampleCache::default();
            let commands = crate::sync::project_commands(project, &mut samples, 48_000.0, &Default::default());
            let (l, r) = crate::export::render(commands, (0.0, 16.0), crate::export::Song::of(project), 48_000.0, &Default::default(), &|_| {}).unwrap();
            let mono: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect();
            let gram = stems::chord::Chromagram::new(&mono, 48_000.0);
            (0..4).map(|bar| gram.chords(&[(bar as f64 * 2.0 + 0.1, bar as f64 * 2.0 + 1.9)], None)[0].2.map(|c| c.name())).collect()
        };
        assert_eq!(heard(&project), vec![Some("Do".to_string()); 4], "as written: Do all along");
        project.tracks[0].follow_chords = Some(Follow::Chords);
        let in_chords = vec![Some("Lam".to_string()), Some("Fa".into()), Some("Do".into()), Some("Sol".into())];
        assert_eq!(heard(&project), in_chords);
        // What the engine is sent: a lap a bar, each in its chord.
        let mut samples = crate::sync::SampleCache::default();
        let laps = placed_clips(&project, &project.tracks[0], &mut samples);
        assert_eq!(laps.len(), 4);
        for (lap, &(root, quality)) in laps.iter().zip(&progression) {
            let engine::PlayClip::Midi(m) = &lap.clip else { panic!() };
            let tones = Chord::new(root, quality).classes();
            assert!(m.notes.iter().all(|n| tones.contains(&(n.pitch % 12))), "{:?}", m.notes);
        }
        let mut app = PulseApp::headless(project);
        assert_eq!(app.project.tracks[0].arrangement[0].clip, Clip::Midi(pad), "the notes as written");
        // Changing a chord (or the key) sends the following track again.
        let before = app.project.clone();
        app.project.chords[3].chord = Chord::new(2, Quality::Minor);
        let sent = crate::sync::changed_commands(&before, &app.project, &mut app.samples, 48_000.0, &Default::default());
        assert!(sent.iter().any(|c| matches!(c, Command::SetArrangement { track: 0, .. })), "followers sent again");
        let unchanged = crate::sync::changed_commands(&app.project.clone(), &app.project, &mut app.samples, 48_000.0, &Default::default());
        assert!(!unchanged.iter().any(|c| matches!(c, Command::SetArrangement { .. })));
        app.project.chords[3].chord = Chord::new(7, Quality::Major);
        // Written into the notes: one clip with the four bars, no longer following.
        app.apply_chord_follow(0, Follow::Chords, None);
        assert_eq!(app.project.tracks[0].follow_chords, None);
        let placed = &app.project.tracks[0].arrangement;
        assert_eq!(placed.len(), 1);
        let Clip::Midi(m) = &placed[0].clip else { panic!() };
        assert_eq!((m.length, m.notes.len()), (16.0, 12));
        assert_eq!(heard(&app.project), in_chords);
    }

    /// Saved and opened again with its chords and how a track follows them,
    /// in format 22 (only then); ⌘Z takes a chord away.
    #[test]
    fn the_chord_track_is_saved_and_undone() {
        let mut project = Project::demo();
        assert!(!project.to_json().unwrap().contains("\"format\": 22"));
        put(&mut project.chords, 0.0, 4.0, Chord { bass: Some(4), ..Chord::new(0, Quality::Major) });
        put(&mut project.chords, 4.0, 8.0, Chord::new(9, Quality::Minor7));
        project.tracks[1].follow_chords = Some(Follow::Bass);
        let json = project.to_json().unwrap();
        assert!(json.contains("\"format\": 22"));
        let loaded = Project::from_json(&json).unwrap();
        assert_eq!(loaded.chords, project.chords);
        assert_eq!(loaded.tracks[1].follow_chords, Some(Follow::Bass));
        assert_eq!(loaded, project);
        // A project from before has none.
        let old = Project::from_json(&Project::demo().to_json().unwrap()).unwrap();
        assert!(old.chords.is_empty() && old.tracks.iter().all(|t| t.follow_chords.is_none()));

        let mut app = PulseApp::headless(Project::demo());
        app.set_key(Some(Key { tonic: 9, minor: true }));
        app.commit_history();
        app.add_song_chord(1.0);
        app.add_song_chord(5.0);
        // The tonic, then what usually comes after it.
        let added = names(&app);
        assert_eq!(added[0], (0.0, "Lam".to_string()));
        assert_eq!(added.len(), 2);
        assert_eq!(app.project.chords[1].start, 4.0);
        app.commit_history();
        app.undo();
        assert!(app.project.chords.is_empty(), "⌘Z takes them away");
        app.redo();
        assert_eq!(names(&app), added);
    }

    /// «De tarareo a canción» on the Arrangement writes its chords on the chord
    /// track; again over them, with a tune that fits, it plays them.
    #[test]
    fn a_hummed_song_writes_and_reads_the_chord_track() {
        let mut app = PulseApp::headless(Project::empty());
        let note = |start: f64, pitch: u8| Note { start, length: 1.0, pitch, velocity: 0.8, chance: 1.0 };
        // Do Mi Sol, then Fa La Do, Sol Si Re, Do Mi Sol: a bar each.
        let tune: Vec<Note> = [[60, 64, 67, 64], [65, 69, 72, 69], [67, 71, 74, 71], [72, 67, 64, 60]]
            .iter()
            .enumerate()
            .flat_map(|(bar, ns)| ns.iter().enumerate().map(move |(k, &p)| note(bar as f64 * 4.0 + k as f64, p)))
            .collect();
        let clip = MidiClip { notes: tune, ..MidiClip::new("Tarareo", 16.0, [0; 3]) };
        app.project.tracks[0].arrangement.push(ArrangementClip { start: 8.0, length: 16.0, clip: Clip::Midi(clip) });
        app.hum_to_song(ClipRef::Arrangement { track: 0, index: 0 }, crate::drum_gen::Style::House);
        let written = names(&app);
        assert_eq!(written.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(), ["Do", "Fa", "Sol", "Do"], "{:?}", app.status);
        assert_eq!(written[0].0, 8.0, "where the clip is");
        // Another song over the same chords: the chord track's, kept.
        app.project.chords[3].chord = Chord::new(9, Quality::Minor);
        app.hum_to_song(ClipRef::Arrangement { track: 0, index: 0 }, crate::drum_gen::Style::House);
        assert_eq!(names(&app).last().unwrap().1, "Lam", "the tune fits the track's chords enough");
    }

    /// The automatic arrangement writes the chords the song plays.
    #[test]
    fn laying_out_a_song_writes_its_chords() {
        let mut app = PulseApp::headless(Project::demo());
        let sections = crate::arrange::plan(&app.project.tracks, &app.project.scene_names, crate::arrange::Template::Electronic);
        app.lay_out(&sections);
        // The demo's Am F C G (its bass plays Do over Sol), all along the song.
        let roots: Vec<String> = app.project.chords.iter().map(|c| Chord::new(c.chord.root, c.chord.quality).name()).collect();
        assert_eq!(roots[..4], ["Lam", "Fa", "Do", "Sol"], "{:?}", names(&app));
        assert_eq!(end(&app.project.chords), app.project.loop_end);
    }

    /// The strip drawn off screen: chords in the key (the major V out of it,
    /// violet), their degrees, the header's «Acordes · La menor», a track that
    /// follows them, and the empty strip's hint. PULSE_SHOTS=<dir>.
    #[test]
    fn chord_track_shots() {
        let Some(dir) = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        for (name, width, chords) in [("pista-de-acordes", 1440.0, true), ("pista-de-acordes-vacia", 1000.0, false)] {
            let mut app = PulseApp::headless(Project::demo());
            app.view = crate::app::View::Arrangement;
            let key = Key { tonic: 9, minor: true };
            app.project.key = Some(key);
            app.arrangement.pixels_per_beat = 20.0;
            if chords {
                for (bar, d) in [0, 5, 2, 6, 0, 5, 2, 4].iter().enumerate() {
                    put(&mut app.project.chords, bar as f64 * 4.0, 4.0, chord::on_degree(key, *d, false, bar == 7));
                }
                app.project.tracks[1].follow_chords = Some(Follow::Bass);
                app.project.tracks[1].arrangement.push(ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Midi(MidiClip::new("Bajo", 4.0, [80, 120, 220])) });
                app.chord_lane.selected = Some(2);
            }
            let ctx = egui::Context::default();
            theme::apply(&ctx);
            let size = egui::vec2(width, 420.0);
            let mut canvas = crate::offscreen::Canvas::default();
            let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
            let mut output = None;
            for frame in 0..4 {
                ctx.set_pixels_per_point(2.0);
                output = Some(ctx.run_ui(input(), |ui| {
                    egui::CentralPanel::default().show(ui, |ui| crate::arrangement::show(&mut app, ui));
                }));
                if frame < 3 {
                    canvas.draw(&ctx, output.take().unwrap(), size, 2.0);
                }
            }
            canvas.draw(&ctx, output.unwrap(), size, 2.0).save_png(&dir.join(format!("{name}.png"))).unwrap();
        }
    }
}
