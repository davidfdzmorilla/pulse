//! Chords in the MIDI editor: the chord each stretch of a clip plays, and the
//! «Acordes» menu that writes the key's chords, what usually comes next and
//! ready-made progressions, voiced so that each leads smoothly into the next.

use crate::theme::{self, ACCENT, TEXT_DIM};
use crate::widgets;
use eframe::egui::{self, RichText};
use engine::model::{MidiClip, Note};
use stems::chord::{self, Chord};
use engine::meter::TimeSig;
use stems::key::Key;

/// How far back the chord to lead on from is heard.
const STEP: f64 = 2.0;

/// The shortest stretch a chord is named for in bars of `sig`: half a bar where
/// it halves (4/4, 6/8, 2/4), else the whole bar (3/4, 5/4, 7/8).
pub fn chord_step(sig: TimeSig) -> f64 {
    let pulses = sig.pulses();
    if pulses.len() % 2 == 0 { pulses[pulses.len() / 2].0 } else { sig.bar() }
}

/// The chords of `notes` over `0..length` beats in bars of `sig`, a
/// [`chord_step`] at a time, equal ones run together: where each starts and
/// ends, and the chord (None where none sounds).
pub fn chords(notes: &[Note], length: f64, key: Option<Key>, sig: TimeSig) -> Vec<(f64, f64, Option<Chord>)> {
    let step = chord_step(sig);
    let mut out: Vec<(f64, f64, Option<Chord>)> = Vec::new();
    let mut from = 0.0;
    while from < length - 1e-9 {
        let to = (from + step).min(length);
        let chord = chord_between(notes, from, to, key);
        match out.last_mut() {
            Some(last) if last.2 == chord => last.1 = to,
            _ => out.push((from, to, chord)),
        }
        from = to;
    }
    out
}

/// The chord the notes sounding over `from..to` make: each pitch class by how
/// long it sounds there, the bass the lowest note held for a good part of it.
pub fn chord_between(notes: &[Note], from: f64, to: f64, key: Option<Key>) -> Option<Chord> {
    let mut weights = [0f64; 12];
    let mut lowest: Option<u8> = None;
    for n in notes {
        let overlap = (n.start + n.length).min(to) - n.start.max(from);
        if overlap <= 1e-9 {
            continue;
        }
        weights[n.pitch as usize % 12] += overlap;
        if overlap >= 0.25 * (to - from) && lowest.is_none_or(|l| n.pitch < l) {
            lowest = Some(n.pitch);
        }
    }
    Chord::detect(&weights, lowest.map(|p| p % 12), key)
}

/// The notes sounding just before `at` (the chord to lead on from), and the
/// chord they make.
fn before(notes: &[Note], at: f64, key: Option<Key>) -> (Vec<u8>, Option<Chord>) {
    let from = (at - STEP).max(0.0);
    let mut pitches: Vec<u8> = notes.iter().filter(|n| n.start < at - 1e-9 && n.start + n.length > from + 1e-9).map(|n| n.pitch).collect();
    pitches.sort_unstable();
    pitches.dedup();
    let chord = (at > 0.0).then(|| chord_between(notes, from, at, key)).flatten();
    (pitches, chord)
}

/// Writes `chord` at `at` for `length` beats, voiced from the `previous`
/// pitches (bass first); the indices of the new notes and their pitches.
fn insert(clip: &mut MidiClip, at: f64, length: f64, chord: &Chord, previous: &[u8]) -> (Vec<usize>, Vec<u8>) {
    let length = length.min(clip.length - at);
    if length <= 0.0 {
        return (Vec::new(), Vec::new());
    }
    // Lead on from the previous chord's upper notes (not its bass).
    let upper = if previous.len() > 1 { &previous[1..] } else { previous };
    let pitches = chord::voice(chord, upper);
    let mut added = Vec::new();
    for (i, &pitch) in pitches.iter().enumerate() {
        let velocity = if i == 0 { 0.8 } else { 0.7 };
        clip.notes.push(Note { start: at, length, pitch, velocity, chance: 1.0 });
        added.push(clip.notes.len() - 1);
    }
    (added, pitches)
}

/// What the «Acordes» menu remembers between uses.
pub struct ChordMenu {
    /// Seventh chords instead of triads.
    pub sevenths: bool,
    /// Beats each chord lasts.
    pub length: f64,
}

impl Default for ChordMenu {
    fn default() -> Self {
        Self { sevenths: false, length: 4.0 }
    }
}

/// What a click in the menu writes: the chords in order, from `at`.
pub fn write(clip: &mut MidiClip, at: &mut f64, length: f64, chords: &[Chord], key: Option<Key>) -> (Vec<usize>, Vec<u8>) {
    let (mut added, mut sounded) = (Vec::new(), Vec::new());
    for chord in chords {
        if *at >= clip.length - 1e-9 {
            break;
        }
        let (previous, _) = before(&clip.notes, *at, key);
        let (new, pitches) = insert(clip, *at, length, chord, &previous);
        added.extend(new);
        if sounded.is_empty() {
            sounded = pitches;
        }
        *at = (*at + length).min(clip.length);
    }
    (added, sounded)
}

/// «Acordes»: the key's chords (the ones that usually follow the chord before
/// the insert marker lit), progressions, and the chord length. Writes at `at`
/// and moves it on, so clicking chords one after another builds a progression;
/// returns the new notes and the pitches to preview.
pub fn menu(ui: &mut egui::Ui, settings: &mut ChordMenu, clip: &mut MidiClip, at: &mut f64, key: Option<Key>, sig: TimeSig) -> (Vec<usize>, Vec<u8>) {
    // The lengths offered, in the clip's bars; a length from another meter becomes a bar.
    let lengths = [(chord_step(sig), "½ c."), (sig.bar(), "1 c."), (sig.bar() * 2.0, "2 c.")];
    if !lengths.iter().any(|l| (l.0 - settings.length).abs() < 1e-9) {
        settings.length = sig.bar();
    }
    let mut result = (Vec::new(), Vec::new());
    widgets::field_menu_button(ui, "Acordes", |ui| {
        ui.set_min_width(300.0);
        let Some(key) = key else {
            ui.label(RichText::new("Elige una Escala en la barra de transporte (o detéctala) para ver sus acordes.").color(TEXT_DIM).size(11.0));
            return;
        };
        let (_, previous) = before(&clip.notes, *at, Some(key));
        let chords = chord::diatonic(key, settings.sevenths);
        let degree_of = |c: &Chord| chords.iter().position(|d| d.root == c.root);
        let suggested: &[usize] = previous.as_ref().and_then(degree_of).map_or(&[], |d| chord::next_degrees(key, d));
        ui.label(RichText::new(format!("Acordes de {}", key.name())).strong());
        match previous {
            Some(p) if !suggested.is_empty() => {
                let names: Vec<String> = suggested.iter().map(|&d| chords[d].name()).collect();
                ui.label(RichText::new(format!("Después de {} suelen ir: {}", p.name(), names.join(", "))).color(TEXT_DIM).size(10.5));
            }
            _ => {
                ui.label(RichText::new("Se escriben en el marcador naranja, uno detrás de otro").color(TEXT_DIM).size(10.5));
            }
        }
        egui::Grid::new("diatonic").spacing(egui::vec2(3.0, 3.0)).show(ui, |ui| {
            for (d, c) in chords.iter().enumerate() {
                let numeral = c.numeral(key).unwrap_or_default();
                let mut text = RichText::new(format!("{numeral}\n{}", c.name()));
                if suggested.contains(&d) {
                    text = text.color(ACCENT);
                }
                if ui.add(egui::Button::new(text).min_size(egui::vec2(38.0, 34.0))).clicked() {
                    result = write(clip, at, settings.length, &[*c], Some(key));
                }
            }
            ui.end_row();
        });
        if key.minor {
            let five = chord::on_degree(key, 4, settings.sevenths, true);
            if ui.button(format!("V {} (menor armónica)", five.name())).on_hover_text("La dominante mayor, que lleva con más fuerza a la tónica").clicked() {
                result = write(clip, at, settings.length, &[five], Some(key));
            }
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut settings.sevenths, "Con séptima");
            ui.label(RichText::new("Duración").color(TEXT_DIM).size(10.5));
            for (beats, label) in lengths {
                if beats < sig.bar() - 1e-9 || label != "½ c." {
                    ui.selectable_value(&mut settings.length, beats, label);
                }
            }
        });
        ui.separator();
        ui.label(RichText::new("Progresiones").color(TEXT_DIM).size(10.5));
        for progression in chord::progressions(key) {
            let chords: Vec<Chord> = progression.degrees.iter().map(|&d| chord::on_degree(key, d, settings.sevenths, progression.major_five)).collect();
            let names: Vec<String> = chords.iter().map(|c| c.name()).collect();
            if ui.button(progression.name).on_hover_text(names.join(" – ")).clicked() {
                result = write(clip, at, settings.length, &chords, Some(key));
            }
        }
        ui.label(RichText::new("Las voces se enlazan con el acorde anterior: se mueven lo menos posible.").color(theme::TEXT_DIM).size(10.0));
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(start: f64, length: f64, pitch: u8) -> Note {
        Note { start, length, pitch, velocity: 0.8, chance: 1.0 }
    }

    #[test]
    fn a_clips_chords_by_stretch() {
        // Am for a bar, then F and G half a bar each, with a melody on top.
        let mut notes = vec![note(0.0, 4.0, 45), note(0.0, 4.0, 60), note(0.0, 4.0, 64), note(0.0, 4.0, 69)];
        notes.extend([note(4.0, 2.0, 41), note(4.0, 2.0, 60), note(4.0, 2.0, 65), note(4.0, 2.0, 69)]);
        notes.extend([note(6.0, 2.0, 43), note(6.0, 2.0, 62), note(6.0, 2.0, 67), note(6.0, 2.0, 71)]);
        notes.extend([note(0.0, 0.5, 76), note(1.0, 0.5, 74), note(5.0, 0.5, 72)]);
        let found: Vec<(f64, f64, Option<String>)> = chords(&notes, 12.0, None, TimeSig::COMMON).into_iter().map(|(a, b, c)| (a, b, c.map(|c| c.name()))).collect();
        assert_eq!(
            found,
            vec![(0.0, 4.0, Some("Lam".into())), (4.0, 6.0, Some("Fa".into())), (6.0, 8.0, Some("Sol".into())), (8.0, 12.0, None)]
        );
    }

    #[test]
    fn clicking_chords_builds_a_progression() {
        let c_major = Key { tonic: 0, minor: false };
        let mut clip = MidiClip::new("Acordes", 16.0, [0; 3]);
        let mut at = 0.0;
        let progression = chord::progressions(c_major)[0].degrees.iter().map(|&d| chord::on_degree(c_major, d, false, false)).collect::<Vec<_>>();
        let (added, sounded) = write(&mut clip, &mut at, 4.0, &progression, Some(c_major));
        assert_eq!(added.len(), 16, "four chords of a bass and three notes");
        assert_eq!(sounded.len(), 4);
        assert_eq!(at, 16.0);
        let named: Vec<String> = chords(&clip.notes, 16.0, Some(c_major), TimeSig::COMMON).iter().filter_map(|c| c.2.map(|c| c.name())).collect();
        assert_eq!(named, ["Do", "Sol", "Lam", "Fa"]);
        // Voice leading: the upper notes of each chord stay close to the last's.
        for bar in 1..4 {
            let upper = |b: usize| -> Vec<i32> {
                let mut p: Vec<i32> = clip.notes.iter().filter(|n| n.start == b as f64 * 4.0).map(|n| n.pitch as i32).collect();
                p.sort();
                p[1..].to_vec()
            };
            let moved: i32 = upper(bar - 1).iter().zip(upper(bar)).map(|(a, b)| (a - b).abs()).sum();
            assert!(moved <= 5, "bar {bar}: {:?} → {:?}", upper(bar - 1), upper(bar));
        }
        // Past the clip's end nothing is written.
        assert_eq!(write(&mut clip, &mut at, 4.0, &progression, Some(c_major)).0.len(), 0);
    }

    #[test]
    fn what_comes_after_the_chord_before_the_marker() {
        let c_major = Key { tonic: 0, minor: false };
        let mut clip = MidiClip::new("Acordes", 8.0, [0; 3]);
        let mut at = 0.0;
        write(&mut clip, &mut at, 4.0, &[chord::on_degree(c_major, 4, false, false)], Some(c_major));
        let (pitches, chord) = before(&clip.notes, at, Some(c_major));
        assert_eq!(chord.map(|c| c.name()).as_deref(), Some("Sol"));
        assert_eq!(pitches.len(), 4);
        // After V: I, then vi.
        assert_eq!(chord::next_degrees(c_major, 4), &[0, 5]);
    }
}
