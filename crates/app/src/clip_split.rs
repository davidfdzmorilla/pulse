//! Splitting arrangement clips in two (⌘E) and joining clips into one (⌘J: MIDI
//! notes laid out, audio rendered into a file of its own, see `consolidate`).
//!
//! A placed clip plays its content from the start, looping. The part after a split
//! must go on where it was, so its content is turned round to start there: MIDI
//! notes and clip envelopes rotate within the loop; audio starts later in the file.

use crate::i18n::{fill, tr};
use crate::app::{ClipRef, PulseApp};
use engine::model::{ArrangementClip, AudioClip, AutomationLane, AutomationPoint, Clip, Groove, MidiClip, Note, Param};

/// The shortest part a split leaves.
pub const MIN_PART: f64 = 1.0 / 16.0;

/// The most pieces «Trocear» leaves at once.
const MAX_PIECES: usize = 512;

/// Envelope lanes turned round to start `d` beats into a loop of `length`, with a
/// point at the new start and another at the end so the value there stays (a
/// ramp across `d` is in two halves: one at the start, the other at the end).
fn rotate_lanes(lanes: &[AutomationLane], d: f64, length: f64) -> Vec<AutomationLane> {
    lanes
        .iter()
        .map(|lane| {
            // With where each was: of two that land on the same beat (the loop's
            // last point and its first), the later one comes first.
            let mut points: Vec<(f64, AutomationPoint)> = lane.points.iter().map(|p| (p.beat, AutomationPoint { beat: (p.beat - d).rem_euclid(length), ..*p })).collect();
            if let Some(value) = lane.value_at(d) {
                points.retain(|(_, p)| p.beat > 1e-9);
                points.push((d, AutomationPoint { beat: 0.0, value, curve: lane.curve_at(d) }));
                if d > 1e-9 && lane.points.iter().any(|p| p.beat < d - 1e-9) {
                    points.push((d, AutomationPoint { beat: length, value, curve: 0.0 }));
                }
            }
            points.sort_by(|a, b| a.1.beat.total_cmp(&b.1.beat).then(b.0.total_cmp(&a.0)));
            AutomationLane { param: lane.param, points: points.into_iter().map(|(_, p)| p).collect() }
        })
        .collect()
}

/// The content of a clip as heard from `d` beats into it (within its loop).
fn from(clip: &Clip, d: f64, tempo: f64) -> Clip {
    match clip {
        Clip::Midi(midi) => {
            let length = midi.length.max(MIN_PART);
            let d = d.rem_euclid(length);
            let mut notes: Vec<Note> = midi.notes.iter().map(|n| Note { start: (n.start - d).rem_euclid(length), ..n.clone() }).collect();
            // A note held across `d` plays its rest from the start (played again:
            // nothing can sound from before a clip starts).
            notes.extend(midi.notes.iter().filter(|n| n.start < d && n.start + n.length.min(length - n.start) > d + 1e-9).map(|n| Note { start: 0.0, length: n.start + n.length.min(length - n.start) - d, ..n.clone() }));
            notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
            // Controller lanes too: from `d`, round the loop.
            let controls = crate::expression::slice(&midi.controls, length, d, d + length);
            Clip::Midi(MidiClip { notes, envelopes: rotate_lanes(&midi.envelopes, d, length), controls, ..midi.clone() })
        }
        Clip::Audio(audio) => {
            let length = audio.length.max(MIN_PART);
            let d = d.rem_euclid(length);
            // Later in the file, by what the clip plays in `d` beats (warp markers too).
            let offset = audio.source_seconds(d, tempo);
            let envelopes = audio
                .envelopes
                .iter()
                .map(|lane| {
                    let mut points: Vec<AutomationPoint> = lane.points.iter().filter(|p| p.beat > d).map(|p| AutomationPoint { beat: p.beat - d, ..*p }).collect();
                    if let Some(value) = lane.value_at(d) {
                        points.insert(0, AutomationPoint { beat: 0.0, value, curve: lane.curve_at(d) });
                    }
                    AutomationLane { param: lane.param, points }
                })
                .collect();
            let audio = AudioClip { offset, length: (length - d).max(MIN_PART), envelopes, ..audio.clone() };
            // With warp markers the start is a point of the file, as `source_seconds` gives.
            Clip::Audio(audio)
        }
    }
}

/// The parts of `placed` split at `beat` (timeline): the one before it and what
/// follows, or None if `beat` isn't inside it. What follows is one clip, except
/// for a looping audio clip cut in the middle of a lap: an audio clip can't start
/// part of the way into its loop, so the rest of that lap is a clip of its own
/// and the loop goes on in another from the next lap.
pub fn split_parts(placed: &ArrangementClip, beat: f64, tempo: f64) -> Option<(ArrangementClip, Vec<ArrangementClip>)> {
    let (left, right) = split(placed, beat, tempo)?;
    let Clip::Audio(whole) = &placed.clip else { return Some((left, vec![right])) };
    let lap = whole.length.max(MIN_PART);
    let into = (beat - placed.start).rem_euclid(lap);
    if placed.length <= lap + 1e-9 || into < 1e-9 || lap - into < 1e-9 {
        return Some((left, vec![right]));
    }
    let lap_end = beat + lap - into;
    let mut rest_of_lap = right.clone();
    rest_of_lap.length = lap_end.min(placed.end()) - beat;
    trim(&mut rest_of_lap.clip, rest_of_lap.length);
    if placed.end() <= lap_end + 1e-9 {
        return Some((left, vec![rest_of_lap]));
    }
    let mut on = ArrangementClip { start: lap_end, length: placed.end() - lap_end, clip: placed.clip.clone() };
    if let (Clip::Audio(a), Clip::Audio(b), Clip::Audio(r)) = (&mut rest_of_lap.clip, &mut on.clip, &right.clip) {
        (a.fades, b.fades) = r.fades.split(right.length, rest_of_lap.length);
    }
    Some((left, vec![rest_of_lap, on]))
}

/// The two parts of `placed` split at `beat` (timeline), or None if `beat` isn't
/// inside it. (A looping audio clip cut in the middle of a lap takes
/// [`split_parts`]: its second part here only plays the end of its loop.)
pub fn split(placed: &ArrangementClip, beat: f64, tempo: f64) -> Option<(ArrangementClip, ArrangementClip)> {
    if beat <= placed.start + MIN_PART || beat >= placed.end() - MIN_PART {
        return None;
    }
    let d = beat - placed.start;
    let mut left = ArrangementClip { length: d, ..placed.clone() };
    let mut right = ArrangementClip { start: beat, length: placed.end() - beat, clip: from(&placed.clip, d, tempo) };
    // The fade-in stays at the start, the fade-out at the end; one the cut
    // falls in goes on in the other part, so the two sound as the whole.
    if let (Clip::Audio(a), Clip::Audio(b)) = (&mut left.clip, &mut right.clip) {
        (a.fades, b.fades) = a.fades.split(placed.length, d);
    }
    // Not looping: each part is only its piece (the clip editor shows just that).
    // A loop keeps its loop.
    let loop_length = match &placed.clip {
        Clip::Midi(m) => m.length,
        Clip::Audio(a) => a.length,
    };
    if placed.length <= loop_length + 1e-9 {
        trim(&mut left.clip, d);
        trim(&mut right.clip, placed.length - d);
    }
    Some((left, right))
}

/// Cuts envelope lanes to their first `length` beats, with a point at the cut
/// so a ramp across it still gets to the value it had there.
fn cut_lanes(lanes: &mut [AutomationLane], length: f64) {
    for lane in lanes {
        let at_cut = lane.points.iter().any(|p| p.beat > length + 1e-9).then(|| lane.value_at(length)).flatten();
        lane.points.retain(|p| p.beat <= length + 1e-9);
        match at_cut {
            Some(value) if lane.points.last().is_none_or(|p| p.beat < length - 1e-9) => lane.points.push(AutomationPoint { beat: length, value, curve: 0.0 }),
            _ => {}
        }
    }
}

/// Cuts a clip's content to its first `length` beats.
fn trim(clip: &mut Clip, length: f64) {
    match clip {
        Clip::Midi(midi) => {
            // The controllers keep their value at the cut.
            midi.controls = crate::expression::slice(&midi.controls, midi.length.max(MIN_PART), 0.0, length);
            midi.length = length;
            midi.notes.retain(|n| n.start < length);
            cut_lanes(&mut midi.envelopes, length);
        }
        Clip::Audio(audio) => {
            audio.length = length;
            cut_lanes(&mut audio.envelopes, length);
        }
    }
}

/// The notes a placed MIDI clip plays, its loops laid out, from its start.
fn laid_out(placed: &ArrangementClip, midi: &MidiClip) -> Vec<Note> {
    let length = midi.length.max(MIN_PART);
    let mut notes = Vec::new();
    let mut lap = 0.0;
    while lap < placed.length {
        for n in midi.notes.iter().filter(|n| n.start < length) {
            let start = lap + n.start;
            if start < placed.length {
                notes.push(Note { start, length: n.length.min(placed.length - start).min(length - n.start), ..n.clone() });
            }
        }
        lap += length;
    }
    notes
}

/// [`laid_out`] with `groove` played into the notes (when and how loud each one
/// sounds on its lap), as the engine plays them.
fn laid_out_grooved(placed: &ArrangementClip, midi: &MidiClip, groove: &Groove) -> Vec<Note> {
    let length = midi.length.max(MIN_PART);
    let mut notes = Vec::new();
    let mut lap = 0.0;
    let mut lap_index = 0i64;
    while lap < placed.length {
        for (index, n) in midi.notes.iter().enumerate().filter(|(_, n)| n.start < length) {
            let start = lap + n.start;
            if start < placed.length {
                // The lap as the engine counts it, set apart by where the copy is.
                let (shift, velocity) = groove.apply(index, lap_index.wrapping_add(engine::copy_salt(placed.start)), n.start, n.velocity);
                let start = (start + shift).max(0.0);
                notes.push(Note { start, length: n.length.min(placed.length - start).min(length - n.start).max(MIN_PART / 16.0), velocity, ..n.clone() });
            }
        }
        lap += length;
        lap_index += 1;
    }
    notes
}

/// One MIDI clip from several on a track, from the first's start to the last's
/// end. If the parts have different grooves, each one's is played into its
/// notes; their envelopes are laid out along it (see [`join_envelopes`]).
pub fn join(parts: &[ArrangementClip], base: &dyn Fn(Param) -> f32) -> Option<ArrangementClip> {
    let start = parts.iter().map(|p| p.start).fold(f64::INFINITY, f64::min);
    let end = parts.iter().map(|p| p.end()).fold(f64::NEG_INFINITY, f64::max);
    let first = parts.iter().min_by(|a, b| a.start.total_cmp(&b.start))?;
    let Clip::Midi(first_midi) = &first.clip else { return None };
    let grooves: Vec<Option<Groove>> = parts.iter().map(|p| if let Clip::Midi(m) = &p.clip { m.groove } else { None }).collect();
    let shared = grooves.iter().all(|g| *g == grooves[0]);
    // What was heard: the deactivated parts left out, unless all of them are
    // (then the whole, still deactivated).
    let all_off = parts.iter().all(|p| p.clip.muted());
    let heard: Vec<ArrangementClip> = parts.iter().filter(|p| all_off || !p.clip.muted()).cloned().collect();
    let mut notes = Vec::new();
    for part in &heard {
        let Clip::Midi(midi) = &part.clip else { return None };
        let laid = match &midi.groove {
            Some(groove) if !shared => laid_out_grooved(part, midi, groove),
            _ => laid_out(part, midi),
        };
        notes.extend(laid.into_iter().map(|n| Note { start: n.start + part.start - start, ..n }));
    }
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    let length = end - start;
    let envelopes = join_envelopes(&heard, start, base);
    let controls = join_controls(&heard, start);
    let groove = if shared { first_midi.groove } else { None };
    Some(ArrangementClip { start, length, clip: Clip::Midi(MidiClip { notes, length, envelopes, groove, muted: all_off, controls, ..first_midi.clone() }) })
}

/// The controller lanes of MIDI `parts` laid out along one clip starting at
/// `start`, each part's looping with it; after a part, where no other takes
/// over, a controller goes back to rest as the engine sends it then.
pub(crate) fn join_controls(parts: &[ArrangementClip], start: f64) -> Vec<engine::model::ControlLane> {
    use engine::model::{ControlLane, MidiControl};
    let mut sorted: Vec<&ArrangementClip> = parts.iter().collect();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut lanes: std::collections::BTreeMap<MidiControl, Vec<AutomationPoint>> = Default::default();
    for (i, part) in sorted.iter().enumerate() {
        let Clip::Midi(midi) = &part.clip else { continue };
        let at = part.start - start;
        for lane in crate::expression::slice(&midi.controls, midi.length.max(MIN_PART), 0.0, part.length) {
            let points = lanes.entry(lane.control).or_default();
            points.extend(lane.points.iter().map(|p| AutomationPoint { beat: p.beat + at, ..*p }));
            // Nothing follows right after with this controller: it rests.
            let next_holds = sorted.get(i + 1).is_some_and(|n| (n.start - part.end()).abs() < 1e-6 && matches!(&n.clip, Clip::Midi(m) if m.controls.iter().any(|l| l.control == lane.control)));
            if let (Some(rest), false) = (lane.control.rest(), next_holds || i + 1 == sorted.len()) {
                points.push(AutomationPoint { beat: at + part.length + 1e-4, value: rest, curve: 0.0 });
            }
        }
    }
    lanes
        .into_iter()
        .map(|(control, mut points)| {
            points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            ControlLane { control, points }
        })
        .collect()
}

/// The envelopes of `parts` laid out along one clip starting at `start`: each
/// part's in its own time, looping with it; where a part has none for a
/// parameter, or between parts, that parameter's `base` (its mixer or device
/// setting), as it was heard there.
pub(crate) fn join_envelopes(parts: &[ArrangementClip], start: f64, base: &dyn Fn(Param) -> f32) -> Vec<AutomationLane> {
    const EDGE: f64 = 1e-6;
    let envelopes = |clip: &Clip| match clip {
        Clip::Midi(m) => m.envelopes.clone(),
        Clip::Audio(a) => a.envelopes.clone(),
    };
    let mut sorted: Vec<&ArrangementClip> = parts.iter().collect();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut params: Vec<Param> = sorted.iter().flat_map(|p| envelopes(&p.clip)).filter(|l| !l.points.is_empty()).map(|l| l.param).collect();
    params.dedup();
    let mut seen = Vec::new();
    params.retain(|p| if seen.contains(p) { false } else { seen.push(*p); true });
    let mut lanes = Vec::new();
    for param in params {
        let held = base(param);
        let mut points: Vec<AutomationPoint> = Vec::new();
        let mut at = 0.0;
        for part in &sorted {
            let (from, to) = (part.start - start, part.end() - start);
            let lane = envelopes(&part.clip).into_iter().find(|l| l.param == param && !l.points.is_empty());
            let Some(lane) = lane else { continue };
            // Up to here, nothing held it.
            if from > at + EDGE {
                points.push(AutomationPoint { beat: at, value: held, curve: 0.0 });
                points.push(AutomationPoint { beat: from - EDGE, value: held, curve: 0.0 });
            }
            let length = part.clip.length().max(MIN_PART);
            let mut lap = 0.0;
            while from + lap < to - EDGE {
                let lap_end = (lap + length).min(to - from);
                points.push(AutomationPoint { beat: from + lap, value: lane.value_at(0.0).unwrap_or(held), curve: lane.curve_at(0.0) });
                for p in lane.points.iter().filter(|p| p.beat > 0.0 && p.beat < lap_end - lap) {
                    points.push(AutomationPoint { beat: from + lap + p.beat, ..*p });
                }
                points.push(AutomationPoint { beat: from + lap_end - EDGE, value: lane.value_at(lap_end - lap - EDGE).unwrap_or(held), curve: 0.0 });
                lap += length;
            }
            at = to;
        }
        points.push(AutomationPoint { beat: at, value: held, curve: 0.0 });
        points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        lanes.push(AutomationLane { param, points });
    }
    lanes
}

impl PulseApp {
    /// What track `track`'s parameter is where no clip envelope holds it: its
    /// mixer's or its device's setting (what a joined clip's envelope keeps there).
    pub(crate) fn envelope_base(&self, track: usize, param: Param) -> f32 {
        let Some(t) = self.project.tracks.get(track) else { return 0.0 };
        match param {
            Param::Volume => t.mixer.gain,
            Param::Pan => t.mixer.pan,
            Param::Send(i) => t.sends.get(i).copied().unwrap_or(0.0),
            Param::Device { device, param } => t.devices.get(device).map_or(0.0, |d| d.effect.get(param)),
            Param::Instrument(i) => t.instrument.get(i),
            Param::Tempo => self.project.tempo as f32,
        }
    }

    /// Splits an arrangement clip at `beat`; its parts end up selected (what
    /// follows the cut goes to the end of the lane).
    pub fn split_clip_at(&mut self, track: usize, index: usize, beat: f64) -> bool {
        let tempo = self.project.tempo;
        let Some(placed) = self.project.tracks.get(track).and_then(|t| t.arrangement.get(index)) else { return false };
        let Some((left, following)) = split_parts(placed, beat, tempo) else { return false };
        let lane = &mut self.project.tracks[track].arrangement;
        lane[index] = left;
        let first = lane.len();
        lane.extend(following);
        let parts = std::iter::once(index).chain(first..lane.len()).map(|index| ClipRef::Arrangement { track, index }).collect();
        self.sync_arrangement(track);
        self.set_clip_selection(parts);
        true
    }

    /// Cuts the selected arrangement clips at every multiple of `every` beats
    /// inside them (4 and over: every that many bars, 4 a bar, on the song's bar
    /// lines); the pieces end up selected.
    pub fn slice_selected(&mut self, every: f64) {
        let selected: Vec<(usize, usize)> = self
            .clip_selection()
            .into_iter()
            .filter_map(|c| match c {
                ClipRef::Arrangement { track, index } => Some((track, index)),
                ClipRef::Session { .. } => None,
            })
            .collect();
        let mut pieces = Vec::new();
        for (track, index) in selected {
            let Some(placed) = self.project.tracks.get(track).and_then(|t| t.arrangement.get(index)) else { continue };
            let (start, end) = (placed.start, placed.end());
            pieces.push(ClipRef::Arrangement { track, index });
            // Each cut leaves what follows at the end of the lane: the next one
            // falls in one of those pieces.
            let mut mine = vec![index];
            let mut cuts = Vec::new();
            if every >= 4.0 - 1e-9 {
                let bars = (every / 4.0).round().max(1.0) as i64;
                self.project.meter.bars(start + MIN_PART, end - MIN_PART, |bar, beat| {
                    if bar.rem_euclid(bars) == 0 {
                        cuts.push(beat);
                    }
                });
            } else {
                let mut at = ((start + MIN_PART) / every).ceil() * every;
                while at < end - MIN_PART {
                    cuts.push(at);
                    at += every;
                }
            }
            for at in cuts {
                if pieces.len() >= MAX_PIECES {
                    break;
                }
                let lane = &self.project.tracks[track].arrangement;
                let Some(&piece) = mine.iter().find(|&&i| lane[i].start < at && at < lane[i].end()) else { continue };
                let before = lane.len();
                if self.split_clip_at(track, piece, at) {
                    for new in before..self.project.tracks[track].arrangement.len() {
                        mine.push(new);
                        pieces.push(ClipRef::Arrangement { track, index: new });
                    }
                }
            }
        }
        if pieces.is_empty() {
            self.status = Some(tr("Selecciona clips del Arrangement para trocearlos").into());
        } else {
            self.set_clip_selection(pieces);
        }
    }

    /// ⌘E: the selected arrangement clips split where a clic on a clip left the
    /// cut point, or else at the playhead.
    pub fn split_selected(&mut self) {
        let selected: Vec<(usize, usize)> = self
            .clip_selection()
            .into_iter()
            .filter_map(|c| match c {
                ClipRef::Arrangement { track, index } => Some((track, index)),
                ClipRef::Session { .. } => None,
            })
            .collect();
        if selected.is_empty() {
            self.status = Some(tr("Selecciona clips del Arrangement para dividirlos").into());
            return;
        }
        let inside = |beat: f64| selected.iter().any(|&(t, i)| self.project.tracks.get(t).and_then(|t| t.arrangement.get(i)).is_some_and(|p| p.start < beat && beat < p.end()));
        let beat = self.arrangement.cut.filter(|&c| inside(c)).unwrap_or_else(|| self.audio.position());
        let mut parts = Vec::new();
        for (track, index) in selected {
            if self.split_clip_at(track, index, beat) {
                parts.extend(self.clip_selection());
            }
        }
        if parts.is_empty() {
            self.status = Some(crate::theme::keys(tr("Haz clic en el clip donde quieras cortar y pulsa ⌘E otra vez")));
        } else {
            self.set_clip_selection(parts);
        }
    }

    /// ⌘J: the selected MIDI clips of one track become one.
    pub fn join_selected(&mut self) {
        let selected: Vec<(usize, usize)> = self
            .clip_selection()
            .into_iter()
            .filter_map(|c| match c {
                ClipRef::Arrangement { track, index } => Some((track, index)),
                ClipRef::Session { .. } => None,
            })
            .collect();
        let Some(&(track, _)) = selected.first() else {
            self.status = Some(tr("Selecciona dos o más clips MIDI de una pista del Arrangement para unirlos").into());
            return;
        };
        if selected.len() < 2 || selected.iter().any(|&(t, _)| t != track) {
            self.status = Some(tr("Selecciona dos o más clips MIDI de la misma pista para unirlos").into());
            return;
        }
        let lane = &self.project.tracks[track].arrangement;
        let parts: Vec<ArrangementClip> = selected.iter().filter_map(|&(_, i)| lane.get(i).cloned()).collect();
        // MIDI clips join their notes; audio ones render into one file.
        let joined = if parts.iter().all(|p| matches!(p.clip, Clip::Audio(_))) {
            match self.join_audio(track, &parts) {
                Ok(joined) => joined,
                Err(error) => {
                    self.status = Some(error);
                    return;
                }
            }
        } else if let Some(joined) = join(&parts, &|p| self.envelope_base(track, p)) {
            joined
        } else {
            self.status = Some(tr("Solo se unen clips del mismo tipo: todos MIDI o todos audio").into());
            return;
        };
        let mut indices: Vec<usize> = selected.iter().map(|&(_, i)| i).collect();
        indices.sort_unstable();
        let lane = &mut self.project.tracks[track].arrangement;
        for i in indices.into_iter().rev() {
            lane.remove(i);
        }
        lane.push(joined);
        let index = lane.len() - 1;
        self.sync_arrangement(track);
        self.select_clip(ClipRef::Arrangement { track, index });
        self.status = Some(fill(tr("{} clips unidos en uno"), &[&parts.len()]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⌘J keeps what was heard: a deactivated part's notes are left out and
    /// the result plays; all parts deactivated, it keeps them all, deactivated.
    #[test]
    fn joining_deactivated_clips_keeps_what_was_heard() {
        let mut off = MidiClip { notes: vec![note(0.0, 60)], ..MidiClip::new("off", 4.0, [0; 3]) };
        off.muted = true;
        let on = MidiClip { notes: vec![note(0.0, 64)], ..MidiClip::new("on", 4.0, [0; 3]) };
        for first_off in [true, false] {
            let (a, b) = if first_off { (off.clone(), on.clone()) } else { (on.clone(), off.clone()) };
            let parts = [ArrangementClip { start: 0.0, length: 4.0, clip: Clip::Midi(a) }, ArrangementClip { start: 4.0, length: 4.0, clip: Clip::Midi(b) }];
            let Clip::Midi(m) = join(&parts, &|_| 0.0).unwrap().clip else { panic!() };
            assert!(!m.muted);
            assert_eq!(m.notes.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![64]);
        }
        let parts = [ArrangementClip { start: 0.0, length: 4.0, clip: Clip::Midi(off.clone()) }, ArrangementClip { start: 4.0, length: 4.0, clip: Clip::Midi(off) }];
        let Clip::Midi(m) = join(&parts, &|_| 0.0).unwrap().clip else { panic!() };
        assert!(m.muted && m.notes.len() == 2, "all off: all kept, off");
    }
    use engine::model::{Param, WarpMarker, WarpMode};

    fn note(start: f64, pitch: u8) -> Note {
        Note { start, length: 0.5, pitch, velocity: 0.8, chance: 1.0 }
    }

    fn midi(notes: Vec<Note>, length: f64) -> Clip {
        Clip::Midi(MidiClip { notes, ..MidiClip::new("m", length, [0; 3]) })
    }

    #[test]
    fn a_split_clip_keeps_its_fade_in_at_the_start_and_its_fade_out_at_the_end() {
        let mut audio = engine::model::AudioClip { hard_start: false, fades: engine::model::Fades::new(1.0, 2.0), name: "a".into(), color: [0; 3], sample: String::new(), length: 8.0, offset: 0.0, warp: engine::model::WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false };
        audio.length = 8.0;
        let placed = ArrangementClip { start: 0.0, length: 8.0, clip: Clip::Audio(audio) };
        let (left, right) = split(&placed, 4.0, 120.0).unwrap();
        let fades = |p: &ArrangementClip| match &p.clip {
            Clip::Audio(a) => (a.fades.fade_in, a.fades.fade_out),
            _ => panic!(),
        };
        assert_eq!(fades(&left), (1.0, 0.0));
        assert_eq!(fades(&right), (0.0, 2.0));
    }

    #[test]
    fn a_looping_midi_clip_splits_where_it_was() {
        // A 4-beat loop placed for 12 beats, split at beat 6 of the timeline (start 2):
        // 4 beats in, so the right part starts at the loop's start again.
        let placed = ArrangementClip { start: 2.0, length: 12.0, clip: midi(vec![note(0.0, 60), note(1.0, 62), note(3.0, 64)], 4.0) };
        let (left, right) = split(&placed, 6.0, 120.0).unwrap();
        assert_eq!((left.start, left.length), (2.0, 4.0));
        assert_eq!((right.start, right.length), (6.0, 8.0));
        // At beat 7 of the timeline (1 beat into the loop): what played there plays first.
        let (_, right) = split(&placed, 7.0, 120.0).unwrap();
        let Clip::Midi(m) = &right.clip else { panic!() };
        let mut starts: Vec<(f64, u8)> = m.notes.iter().map(|n| (n.start, n.pitch)).collect();
        starts.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(starts, vec![(0.0, 62), (2.0, 64), (3.0, 60)]);
        // Not inside: no split.
        assert!(split(&placed, 2.0, 120.0).is_none());
        assert!(split(&placed, 14.0, 120.0).is_none());
    }

    /// A clip envelope that ramps across the cut: each part keeps its half of
    /// the ramp, so the level doesn't jump at the cut (before, the first part
    /// stayed at the ramp's first value).
    #[test]
    fn an_envelope_ramp_across_the_cut_stays_in_both_parts() {
        let lane = AutomationLane { param: Param::Volume, points: vec![AutomationPoint { beat: 0.0, value: 0.2, curve: 0.0 }, AutomationPoint { beat: 16.0, value: 1.0, curve: 0.0 }] };
        let audio = AudioClip { hard_start: false, fades: Default::default(),
            name: "a".into(),
            color: [0; 3],
            sample: "/a.wav".into(),
            length: 16.0,
            offset: 0.0,
            warp: WarpMode::Repitch,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            key: None,
            takes: Vec::new(),
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: vec![lane.clone()], muted: false,
        };
        let midi = MidiClip { name: "m".into(), length: 16.0, notes: Vec::new(), color: [0; 3], envelopes: vec![lane.clone()], groove: None, muted: false, controls: Vec::new() };
        for clip in [Clip::Audio(audio), Clip::Midi(midi)] {
            let (left, right) = split(&ArrangementClip { start: 4.0, length: 16.0, clip }, 12.0, 120.0).unwrap();
            let lanes = |c: &Clip| match c {
                Clip::Audio(a) => a.envelopes.clone(),
                Clip::Midi(m) => m.envelopes.clone(),
            };
            let (l, r) = (&lanes(&left.clip)[0], &lanes(&right.clip)[0]);
            for beat in [0.0, 3.0, 7.99] {
                assert!((l.value_at(beat).unwrap() - lane.value_at(beat).unwrap()).abs() < 1e-3, "left at {beat}: {:?}", l.value_at(beat));
                assert!((r.value_at(beat).unwrap() - lane.value_at(beat + 8.0).unwrap()).abs() < 1e-3, "right at {beat}: {:?}", r.value_at(beat));
            }
            assert!(l.points.iter().all(|p| p.beat <= 8.0), "{:?}", l.points);
        }
        // A looping MIDI clip is turned round: the ramp's second half first, then
        // its first half, back to where it started.
        let midi = MidiClip { name: "m".into(), length: 16.0, notes: Vec::new(), color: [0; 3], envelopes: vec![lane.clone()], groove: None, muted: false, controls: Vec::new() };
        let (_, right) = split(&ArrangementClip { start: 4.0, length: 48.0, clip: Clip::Midi(midi) }, 12.0, 120.0).unwrap();
        let Clip::Midi(turned) = &right.clip else { panic!() };
        for beat in [0.0, 3.0, 7.99, 8.01, 12.0, 15.99] {
            let (got, was) = (turned.envelopes[0].value_at(beat).unwrap(), lane.value_at((beat + 8.0) % 16.0).unwrap());
            assert!((got - was).abs() < 1e-3, "turned round, at {beat}: {got} vs {was}");
        }
        // A lane that ends before the cut is left as it is.
        let mut lanes = vec![lane.clone()];
        cut_lanes(&mut lanes, 20.0);
        assert_eq!(lanes[0], lane);
    }

    #[test]
    fn an_audio_clip_split_starts_later_in_its_file() {
        let audio = AudioClip { hard_start: false, fades: Default::default(),
            name: "a".into(),
            color: [0; 3],
            sample: "/a.wav".into(),
            length: 16.0,
            offset: 0.5,
            warp: WarpMode::Stretch,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: vec![AutomationLane { param: Param::Volume, points: vec![AutomationPoint { beat: 0.0, value: 0.0, curve: 0.0 }, AutomationPoint { beat: 8.0, value: 1.0, curve: 0.0 }] }], muted: false,
        };
        let placed = ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Audio(audio.clone()) };
        let (_, right) = split(&placed, 4.0, 100.0).unwrap();
        let Clip::Audio(r) = &right.clip else { panic!() };
        // 4 beats at its original 120 BPM: 2 s later.
        assert!((r.offset - 2.5).abs() < 1e-9);
        assert_eq!(r.length, 12.0);
        // The fade goes on from where it was (halfway).
        assert_eq!(r.envelopes[0].points[0].beat, 0.0);
        assert!((r.envelopes[0].points[0].value - 0.5).abs() < 1e-6);
        assert_eq!(r.envelopes[0].points[1].beat, 4.0);
        // With warp markers, the point of the file the clip had reached.
        let warped = AudioClip { warp_markers: [(0.0, 0.0), (4.0, 1.0), (8.0, 3.0)].map(|(beat, seconds)| WarpMarker { beat, seconds }).to_vec(), offset: 0.0, ..audio };
        let placed = ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Audio(warped) };
        let (_, right) = split(&placed, 6.0, 100.0).unwrap();
        let Clip::Audio(r) = &right.clip else { panic!() };
        assert!((r.offset - 2.0).abs() < 1e-9, "{}", r.offset);
    }

    #[test]
    fn midi_clips_join_with_their_loops_laid_out() {
        let a = ArrangementClip { start: 0.0, length: 8.0, clip: midi(vec![note(0.0, 60)], 4.0) };
        let b = ArrangementClip { start: 12.0, length: 2.0, clip: midi(vec![note(1.0, 67), note(3.0, 69)], 4.0) };
        let joined = join(&[b, a], &|_| 1.0).unwrap();
        assert_eq!((joined.start, joined.length), (0.0, 14.0));
        let Clip::Midi(m) = &joined.clip else { panic!() };
        assert_eq!(m.length, 14.0);
        let starts: Vec<(f64, u8)> = m.notes.iter().map(|n| (n.start, n.pitch)).collect();
        // The loop twice, then the second clip's notes inside its 2 beats.
        assert_eq!(starts, vec![(0.0, 60), (4.0, 60), (13.0, 67)]);
        assert!(join(&[], &|_| 1.0).is_none());
    }

    #[test]
    fn split_and_join_from_the_app() {
        let mut app = PulseApp::headless(crate::project::Project::empty());
        app.project.tracks[0].arrangement.push(ArrangementClip { start: 0.0, length: 8.0, clip: midi(vec![note(0.0, 60), note(5.0, 62)], 8.0) });
        app.select_clip(ClipRef::Arrangement { track: 0, index: 0 });
        assert!(app.split_clip_at(0, 0, 4.0));
        assert_eq!(app.clip_selection().len(), 2, "both parts selected");
        let spans: Vec<(f64, f64)> = app.project.tracks[0].arrangement.iter().map(|c| (c.start, c.length)).collect();
        assert_eq!(spans, vec![(0.0, 4.0), (4.0, 4.0)]);
        app.join_selected();
        let lane = &app.project.tracks[0].arrangement;
        assert_eq!(lane.len(), 1);
        let Clip::Midi(m) = &lane[0].clip else { panic!() };
        let starts: Vec<f64> = m.notes.iter().map(|n| n.start).collect();
        assert_eq!(starts, vec![0.0, 5.0], "as it was");
    }

    /// Heard, not just set: the demo song as an audio clip renders the same split
    /// in two as whole, but for the few milliseconds of the cut's fades.
    #[test]
    fn a_split_clip_sounds_the_same() {
        let song = crate::test_audio::demo_song();
        let render = |split_at: Option<f64>| {
            let mut app = PulseApp::headless(crate::project::Project::empty());
            for track in &mut app.project.tracks {
                track.mixer.active = false;
            }
            let track = app.add_audio_track().unwrap();
            let audio = AudioClip { hard_start: false, fades: Default::default(),
                name: "demo".into(),
                color: [0; 3],
                sample: song.to_string_lossy().into_owned(),
                length: 32.0,
                offset: 0.0,
                warp: WarpMode::Repitch,
                original_bpm: 120.0,
                warp_markers: Vec::new(),
                key: None,
                takes: Vec::new(),
                gain_db: 0.0,
                transpose: 0.0,
                envelopes: Vec::new(), muted: false,
            };
            app.project.tracks[track].arrangement.push(ArrangementClip { start: 4.0, length: 32.0, clip: Clip::Audio(audio) });
            if let Some(beat) = split_at {
                assert!(app.split_clip_at(track, 0, beat));
            }
            let rate = 48_000.0;
            let mut renderer = engine::OfflineRenderer::new(rate);
            let mut samples = crate::sync::SampleCache::default();
            for command in crate::sync::project_commands(&app.project, &mut samples, rate, &Default::default()) {
                renderer.send(command);
            }
            renderer.send(engine::Command::Play);
            let (mut l, mut r) = (Vec::new(), Vec::new());
            renderer.render((40.0 * 0.5 * rate as f64) as usize, &mut l, &mut r);
            l
        };
        let whole = render(None);
        let split = render(Some(14.5));
        // The parts cross-fade at the cut, so it sounds the same there too.
        let worst = whole.iter().zip(&split).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        assert!(whole.iter().map(|s| s.abs()).fold(0f32, f32::max) > 0.1, "the song plays");
        assert!(worst < 1e-3, "they match, also at the cut: {worst}");
    }

    /// A looping audio clip cut in the middle of a lap goes on sounding the
    /// same: the rest of that lap and then its loop (before, the part after the
    /// cut looped only the end of the loop). Cut on every beat too.
    #[test]
    fn a_looping_audio_clip_split_inside_a_lap_sounds_the_same() {
        let song = crate::test_audio::demo_song();
        let rate = 48_000.0;
        // A loop of 4 beats laid out over 12, from beat 4.
        let render = |cut: &dyn Fn(&mut PulseApp, usize)| {
            let mut app = PulseApp::headless(crate::project::Project::empty());
            for track in &mut app.project.tracks {
                track.mixer.active = false;
            }
            let track = app.add_audio_track().unwrap();
            let audio = AudioClip { hard_start: false, fades: Default::default(),
                name: "demo".into(),
                color: [0; 3],
                sample: song.to_string_lossy().into_owned(),
                length: 4.0,
                offset: 0.0,
                warp: WarpMode::Repitch,
                original_bpm: 120.0,
                warp_markers: Vec::new(),
                key: None,
                takes: Vec::new(),
                gain_db: 0.0,
                transpose: 0.0,
                envelopes: Vec::new(), muted: false,
            };
            app.project.tracks[track].arrangement.push(ArrangementClip { start: 4.0, length: 12.0, clip: Clip::Audio(audio) });
            cut(&mut app, track);
            let mut lane: Vec<(f64, f64)> = app.project.tracks[track].arrangement.iter().map(|c| (c.start, c.length)).collect();
            lane.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut renderer = engine::OfflineRenderer::new(rate);
            let mut samples = crate::sync::SampleCache::default();
            for command in crate::sync::project_commands(&app.project, &mut samples, rate, &Default::default()) {
                renderer.send(command);
            }
            renderer.send(engine::Command::Play);
            let (mut l, mut r) = (Vec::new(), Vec::new());
            renderer.render((18.0 * 0.5 * rate as f64) as usize, &mut l, &mut r);
            (l, lane)
        };
        let (whole, _) = render(&|_, _| {});
        assert!(whole.iter().map(|s| s.abs()).fold(0f32, f32::max) > 0.1, "the song plays");
        // Away from the cuts and from where the loop comes round: the parts fade
        // out and in at a cut as the laps of the loop do, and two parts that meet
        // where the loop comes round cross-fade there (a few ms either side).
        let apart = |split: &[f32], cuts: &[f64]| {
            let near = |i: usize| cuts.iter().chain(&[8.0, 12.0]).any(|c| (i as f64 - c * 0.5 * rate as f64).abs() < 0.006 * rate as f64);
            whole.iter().zip(split).enumerate().filter(|&(i, _)| !near(i)).map(|(_, (a, b))| (a - b).abs()).fold(0f32, f32::max)
        };
        for at in [10.0, 9.5, 7.25] {
            let (split, lane) = render(&|app, track| assert!(app.split_clip_at(track, 0, at)));
            let lap_end = 4.0 + ((at - 4.0) / 4.0f64).ceil() * 4.0;
            assert_eq!(lane, vec![(4.0, at - 4.0), (at, lap_end - at), (lap_end, 16.0 - lap_end)], "cut at {at}");
            let worst = apart(&split, &[at]);
            assert!(worst < 1e-3, "cut at {at}: {worst}");
            // No click there either: no step between samples bigger than the song's own.
            let step = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
            assert!(step(&split) <= step(&whole) * 1.05, "cut at {at}: {} vs {}", step(&split), step(&whole));
        }
        // In the last lap there is no loop to go on: two parts.
        let (split, lane) = render(&|app, track| assert!(app.split_clip_at(track, 0, 14.0)));
        assert_eq!(lane, vec![(4.0, 10.0), (14.0, 2.0)]);
        assert!(apart(&split, &[14.0]) < 1e-3);
        // «Trocear» on every beat: twelve pieces.
        let (split, lane) = render(&|app, track| {
            app.set_clip_selection(vec![ClipRef::Arrangement { track, index: 0 }]);
            app.slice_selected(1.0);
        });
        assert_eq!(lane, (4..16).map(|b| (b as f64, 1.0)).collect::<Vec<_>>());
        let cuts: Vec<f64> = (5..16).map(|b| b as f64).collect();
        let worst = apart(&split, &cuts);
        assert!(worst < 1e-3, "sliced: {worst}");
    }

    /// A clip that doesn't loop splits into two that are only their pieces (what
    /// the clip editor shows); a looping one keeps its loop.
    #[test]
    fn the_parts_of_a_clip_are_only_their_pieces() {
        let audio = AudioClip { hard_start: false, fades: Default::default(),
            name: "a".into(),
            color: [0; 3],
            sample: "/a.wav".into(),
            length: 40.0,
            offset: 0.0,
            warp: WarpMode::Stretch,
            original_bpm: 120.0,
            key: None,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(), muted: false,
        };
        let placed = ArrangementClip { start: 4.0, length: 40.0, clip: Clip::Audio(audio) };
        let (left, right) = split(&placed, 10.0, 120.0).unwrap();
        let (Clip::Audio(l), Clip::Audio(r)) = (&left.clip, &right.clip) else { panic!() };
        assert_eq!((l.offset, l.length), (0.0, 6.0), "the first 6 beats");
        assert_eq!((r.offset, r.length), (3.0, 34.0), "from 3 s, the rest");
        let notes = vec![note(0.0, 60), note(5.0, 62)];
        let placed = ArrangementClip { start: 0.0, length: 8.0, clip: midi(notes.clone(), 8.0) };
        let (left, right) = split(&placed, 4.0, 120.0).unwrap();
        let (Clip::Midi(l), Clip::Midi(r)) = (&left.clip, &right.clip) else { panic!() };
        assert_eq!((l.length, l.notes.len()), (4.0, 1));
        assert_eq!((r.length, r.notes.iter().map(|n| n.start).collect::<Vec<_>>()), (4.0, vec![1.0]));
        // Looping (a 4-beat loop placed for 8): both keep the 4-beat loop.
        let placed = ArrangementClip { start: 0.0, length: 8.0, clip: midi(notes, 4.0) };
        let (left, right) = split(&placed, 6.0, 120.0).unwrap();
        let (Clip::Midi(l), Clip::Midi(r)) = (&left.clip, &right.clip) else { panic!() };
        assert_eq!((l.length, r.length), (4.0, 4.0));
    }
}
