//! Standard MIDI files (.mid): reading one into tracks of its own, or into a
//! track that is there, and writing a clip or the whole song to one, for other
//! programs. Beats are quarter notes, as in the file. Pitch bend, channel
//! pressure and controllers come in as the clip's lanes (see `expression`)
//! and go out again; the sustain pedal comes in as the notes' lengths.

use crate::app::{ClipRef, PulseApp};
use crate::project::{Track, PALETTE};
use engine::meter::{Meter, TimeSig};
use engine::model::{ArrangementClip, Clip, ControlLane, InstrumentKind, KeysParams, MidiClip, MidiControl, Note, SynthParams};
use std::path::Path;

/// The drums' channel in General MIDI (10, counting from 1).
const DRUM_CHANNEL: u8 = 9;
/// Ticks a beat in the files Pulse writes.
const PPQ: u16 = 480;

/// One part of a file: the notes of one channel of one of its tracks.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub name: String,
    pub channel: u8,
    /// Its first program change, if it has one (the General MIDI sound).
    pub program: Option<u8>,
    pub notes: Vec<Note>,
    /// Its pitch bend, pressure and controllers, in its time.
    pub controls: Vec<ControlLane>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    /// Its first tempo (BPM), if it says.
    pub tempo: Option<f64>,
    /// How many times the tempo changes after the first.
    pub tempo_changes: usize,
    /// Its time signatures, if it says.
    pub meter: Option<Meter>,
    pub parts: Vec<Part>,
}

/// The General MIDI families, eight programs each.
const FAMILIES: [&str; 16] = ["Piano", "Percusión afinada", "Órgano", "Guitarra", "Bajo", "Cuerdas", "Conjunto", "Metales", "Viento", "Flauta", "Lead", "Pad", "Efectos", "Étnico", "Percusión", "Efectos de sonido"];

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.bytes.get(self.at).ok_or("El archivo MIDI se corta antes de tiempo")?;
        self.at += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len()).ok_or("El archivo MIDI se corta antes de tiempo")?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A variable-length number (seven bits a byte, the top bit saying more
    /// follow). Four bytes at most, says the standard; some programs pad
    /// them with more, which other readers take, so up to eight.
    fn vlq(&mut self) -> Result<u32, String> {
        let mut value = 0u32;
        for _ in 0..8 {
            let b = self.byte()?;
            value = value.wrapping_shl(7) | (b & 0x7F) as u32;
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("El archivo MIDI tiene un número mal escrito".into())
    }
}

/// Reads a standard MIDI file (format 0 or 1). Each track's channels are parts
/// of their own; tracks without notes (the tempo track) give none.
pub fn read(bytes: &[u8]) -> Result<Song, String> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(4)? != b"MThd" {
        return Err("No es un archivo MIDI".into());
    }
    let header = r.u32()? as usize;
    let format = r.u16()?;
    let tracks = r.u16()?;
    let division = r.u16()?;
    r.take(header.saturating_sub(6))?;
    if format > 1 {
        return Err("Archivo MIDI de formato 2 (patrones sueltos): Pulse lee los de formato 0 y 1".into());
    }
    if division & 0x8000 != 0 {
        return Err("Archivo MIDI con tiempo en SMPTE: Pulse lee los que cuentan en negras".into());
    }
    let ppq = division.max(1) as f64;
    let mut tempos: Vec<(u64, f64)> = Vec::new();
    let mut sigs: Vec<(f64, TimeSig)> = Vec::new();
    let mut parts = Vec::new();
    for index in 0..tracks {
        // Chunks Pulse doesn't know are passed over.
        let (id, length) = loop {
            if r.at >= bytes.len() {
                return Ok(finish(tempos, sigs, parts));
            }
            let id = r.take(4)?;
            let length = r.u32()? as usize;
            if id == b"MTrk" {
                break (id, length);
            }
            r.take(length)?;
        };
        let _ = id;
        let end = r.at.checked_add(length).filter(|&e| e <= bytes.len()).ok_or("El archivo MIDI se corta antes de tiempo")?;
        let mut tick = 0u64;
        let mut status = 0u8;
        let mut name = String::new();
        // Per channel: its notes, the notes sounding (channel, pitch, start tick,
        // velocity, let go under the sustain pedal), its program, its pedal.
        let mut notes: [Vec<Note>; 16] = Default::default();
        let mut sounding: Vec<(u8, u8, u64, u8, bool)> = Vec::new();
        let mut programs: [Option<u8>; 16] = [None; 16];
        let mut pedal = [false; 16];
        // Per channel: its controllers as they moved (beat, which, value).
        let mut moves: [Vec<(f64, MidiControl, f32)>; 16] = Default::default();
        let beats = |t: u64| t as f64 / ppq;
        // The earliest of that note still sounding (or, with `pedalled`, the earliest
        // let go under the pedal) ends.
        let close = |notes: &mut [Vec<Note>; 16], sounding: &mut Vec<(u8, u8, u64, u8, bool)>, channel: u8, pitch: u8, tick: u64, pedalled: bool| {
            if let Some(i) = sounding.iter().position(|&(c, p, _, _, held)| c == channel && p == pitch && held == pedalled) {
                let (_, _, start, velocity, _) = sounding.remove(i);
                notes[channel as usize].push(Note { start: beats(start), length: (beats(tick) - beats(start)).max(1.0 / 64.0), pitch, velocity: velocity as f32 / 127.0, chance: 1.0 });
            }
        };
        // A key let go: with the pedal down the note goes on until the pedal comes up.
        let let_go = |notes: &mut [Vec<Note>; 16], sounding: &mut Vec<(u8, u8, u64, u8, bool)>, pedal: &[bool; 16], channel: u8, pitch: u8, tick: u64| {
            if pedal[channel as usize & 15] {
                if let Some(n) = sounding.iter_mut().find(|n| n.0 == channel && n.1 == pitch && !n.4) {
                    n.4 = true;
                }
            } else {
                close(notes, sounding, channel, pitch, tick, false);
            }
        };
        while r.at < end {
            tick += r.vlq()? as u64;
            let mut first = r.byte()?;
            if first & 0x80 == 0 {
                // Running status: the last status again, this its first data byte.
                if status == 0 {
                    return Err("El archivo MIDI tiene datos sin mensaje".into());
                }
                r.at -= 1;
                first = status;
            }
            match first {
                0xFF => {
                    let kind = r.byte()?;
                    let len = r.vlq()? as usize;
                    let data = r.take(len)?;
                    match kind {
                        0x03 if name.is_empty() => name = String::from_utf8_lossy(data).trim().to_string(),
                        // Time signature: beats, and the note as a power of two.
                        0x58 if len >= 2 => sigs.push((tick as f64 / ppq, TimeSig::new(data[0] as u32, 1 << data[1].min(5)))),
                        0x51 if len == 3 => {
                            let micros = ((data[0] as u32) << 16) | ((data[1] as u32) << 8) | data[2] as u32;
                            if micros > 0 {
                                tempos.push((tick, 60_000_000.0 / micros as f64));
                            }
                        }
                        _ => {}
                    }
                }
                0xF0 | 0xF7 => {
                    let len = r.vlq()? as usize;
                    r.take(len)?;
                }
                _ => {
                    status = first;
                    let channel = first & 0x0F;
                    match first & 0xF0 {
                        0x90 => {
                            let (pitch, velocity) = (r.byte()? & 0x7F, r.byte()? & 0x7F);
                            if velocity == 0 {
                                let_go(&mut notes, &mut sounding, &pedal, channel, pitch, tick);
                            } else {
                                // Struck again while the pedal holds it: the old note ends.
                                close(&mut notes, &mut sounding, channel, pitch, tick, true);
                                sounding.push((channel, pitch, tick, velocity, false));
                            }
                        }
                        0x80 => {
                            let pitch = r.byte()? & 0x7F;
                            r.byte()?;
                            let_go(&mut notes, &mut sounding, &pedal, channel, pitch, tick);
                        }
                        0xB0 => {
                            let (controller, value) = (r.byte()? & 0x7F, r.byte()? & 0x7F);
                            if controller == 64 {
                                let down = value >= 64;
                                if pedal[channel as usize] && !down {
                                    while let Some(&(_, pitch, ..)) = sounding.iter().find(|n| n.0 == channel && n.4) {
                                        close(&mut notes, &mut sounding, channel, pitch, tick, true);
                                    }
                                }
                                pedal[channel as usize] = down;
                            } else if MidiControl::Cc(controller).recordable() {
                                moves[channel as usize].push((beats(tick), MidiControl::Cc(controller), value as f32 / 127.0));
                            }
                        }
                        0xC0 => {
                            let program = r.byte()? & 0x7F;
                            programs[channel as usize].get_or_insert(program);
                        }
                        0xD0 => {
                            let pressure = r.byte()? & 0x7F;
                            moves[channel as usize].push((beats(tick), MidiControl::Pressure, pressure as f32 / 127.0));
                        }
                        0xE0 => {
                            let (lsb, msb) = (r.byte()? & 0x7F, r.byte()? & 0x7F);
                            let bend = (((msb as i32) << 7 | lsb as i32) - 8192) as f32 / 8192.0;
                            moves[channel as usize].push((beats(tick), MidiControl::PitchBend, bend.clamp(-1.0, 1.0)));
                        }
                        0xA0 => {
                            r.take(2)?;
                        }
                        _ => return Err("El archivo MIDI tiene un mensaje que no se entiende".into()),
                    }
                }
            }
        }
        r.at = end;
        // Notes never let go end where the track does.
        while let Some(&(channel, pitch, _, _, pedalled)) = sounding.first() {
            close(&mut notes, &mut sounding, channel, pitch, tick, pedalled);
        }
        let used: Vec<usize> = (0..16).filter(|&c| !notes[c].is_empty()).collect();
        for &c in &used {
            let mut list = std::mem::take(&mut notes[c]);
            list.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
            let program = programs[c];
            let base = if !name.is_empty() {
                name.clone()
            } else if c as u8 == DRUM_CHANNEL {
                "Batería".to_string()
            } else if let Some(p) = program {
                FAMILIES[(p / 8) as usize].to_string()
            } else {
                format!("MIDI {}", index + 1)
            };
            let name = if used.len() > 1 { format!("{base} (canal {})", c + 1) } else { base };
            let controls = lanes_of(&moves[c]);
            parts.push(Part { name, channel: c as u8, program, notes: list, controls });
        }
    }
    Ok(finish(tempos, sigs, parts))
}

/// A channel's controller moves (beats from the file's start) as lanes,
/// thinned as a recording's are.
fn lanes_of(moves: &[(f64, MidiControl, f32)]) -> Vec<ControlLane> {
    let recorded: Vec<(usize, engine::RecordedControl)> = moves.iter().map(|&(beat, control, value)| (0, engine::RecordedControl { track: 0, control, value, beat })).collect();
    crate::expression::take_lanes(&recorded, 0.0, f64::MAX)
}

/// The messages that play `lanes` on `channel`, as (tick, message): one at
/// each point, and along a ramp each step the value moves (a 32nd at most).
fn control_messages(lanes: &[ControlLane], channel: u8, tick: &dyn Fn(f64) -> u64) -> Vec<(u64, Vec<u8>)> {
    let c = channel & 0x0F;
    let mut out = Vec::new();
    for lane in lanes {
        let (Some(first), Some(last)) = (lane.points.first(), lane.points.last()) else { continue };
        let message = |value: f32| -> Vec<u8> {
            match lane.control {
                MidiControl::PitchBend => {
                    let v = ((value.clamp(-1.0, 1.0) * 8192.0).round() as i32 + 8192).clamp(0, 16383);
                    vec![0xE0 | c, (v & 0x7F) as u8, (v >> 7) as u8]
                }
                MidiControl::Pressure => vec![0xD0 | c, (value.clamp(0.0, 1.0) * 127.0).round() as u8],
                MidiControl::Cc(n) => vec![0xB0 | c, n & 0x7F, (value.clamp(0.0, 1.0) * 127.0).round() as u8],
            }
        };
        let mut last_sent: Option<Vec<u8>> = None;
        let mut beat = first.beat;
        let step = 1.0 / 32.0;
        // Each point exactly, and the ramps between them.
        let mut stops: Vec<f64> = lane.points.iter().map(|p| p.beat).collect();
        while beat < last.beat {
            stops.push(beat);
            beat += step;
        }
        stops.sort_by(|a, b| a.total_cmp(b));
        stops.dedup_by(|b, a| (*b - *a).abs() < 1e-9);
        for at in stops {
            // At a point that jumps, its own value (not the one held up to it).
            let value = lane.points.iter().rev().find(|p| (p.beat - at).abs() < 1e-9).map_or_else(|| lane.value_at(at).unwrap_or(0.0), |p| p.value);
            let m = message(value);
            if last_sent.as_ref() != Some(&m) {
                out.push((tick(at), m.clone()));
                last_sent = Some(m);
            }
        }
    }
    out
}

/// What a part plays in Pulse, from its General MIDI program: pianos, electric pianos
/// and organs get Pulse Keys with a sound like it; the rest the synth.
pub(crate) fn instrument_for(program: Option<u8>) -> InstrumentKind {
    let preset = match program {
        Some(0) => "Piano · De cola",
        Some(1 | 2) => "Piano · Brillante",
        Some(3) => "Piano · De pared",
        Some(4) => "Púas · Clásico",
        Some(5) => "Púas · Campana",
        Some(6 | 7) => "Lengüetas · Clásico",
        Some(16 | 17) => "Órgano · Jazz",
        Some(18) => "Órgano · Rock",
        Some(19) => "Órgano · Completo",
        Some(20) => "Órgano · Balada",
        _ => return InstrumentKind::Synth(SynthParams::default()),
    };
    KeysParams::presets().into_iter().find(|(name, _)| *name == preset).map_or(InstrumentKind::Synth(SynthParams::default()), |(_, p)| InstrumentKind::Keys(p))
}

fn finish(mut tempos: Vec<(u64, f64)>, sigs: Vec<(f64, TimeSig)>, parts: Vec<Part>) -> Song {
    tempos.sort_by_key(|t| t.0);
    tempos.dedup_by(|a, b| (a.1 - b.1).abs() < 1e-6);
    Song { tempo: tempos.first().map(|t| t.1), tempo_changes: tempos.len().saturating_sub(1), meter: meter_of(sigs), parts }
}

/// The meter of time signatures found at beats: each from the bar it falls in
/// (or the next, if it falls inside one).
fn meter_of(mut sigs: Vec<(f64, TimeSig)>) -> Option<Meter> {
    if sigs.is_empty() {
        return None;
    }
    sigs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut meter = Meter::COMMON;
    for (beat, sig) in sigs {
        let bar = meter.bar_at(meter.next_bar(beat - 1e-6)).0.max(0) as u32;
        meter = meter.with_change(bar, sig);
    }
    Some(meter)
}

fn vlq(out: &mut Vec<u8>, mut value: u32) {
    let mut bytes = vec![(value & 0x7F) as u8];
    value >>= 7;
    while value > 0 {
        bytes.push((value & 0x7F) as u8 | 0x80);
        value >>= 7;
    }
    bytes.reverse();
    out.extend(bytes);
}

fn chunk(out: &mut Vec<u8>, events: Vec<(u64, Vec<u8>)>) {
    let mut body = Vec::new();
    let mut last = 0u64;
    for (tick, event) in events {
        vlq(&mut body, (tick - last) as u32);
        body.extend(event);
        last = tick;
    }
    // End of track.
    vlq(&mut body, 0);
    body.extend([0xFF, 0x2F, 0x00]);
    out.extend(b"MTrk");
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
}

/// A format 1 file: a tempo track (`tempo`, and the time signatures of `meter`),
/// then each part on its own track and channel (drums on the 10th).
pub fn write(parts: &[Part], tempo: f64, meter: &Meter) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(b"MThd");
    out.extend(6u32.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    out.extend(((parts.len() + 1) as u16).to_be_bytes());
    out.extend(PPQ.to_be_bytes());
    let micros = (60_000_000.0 / tempo.max(1.0)).round() as u32;
    let m = micros.to_be_bytes();
    let tick = |beat: f64| (beat.max(0.0) * PPQ as f64).round() as u64;
    let mut tempo_track = vec![(0, vec![0xFF, 0x51, 0x03, m[1], m[2], m[3]])];
    for change in meter.changes() {
        let at = tick(meter.bar_start(change.bar as i64));
        // Clocks a click (a quarter: 24) and 32nds a quarter (8), as everyone writes them.
        tempo_track.push((at, vec![0xFF, 0x58, 0x04, change.sig.beats, change.sig.unit.trailing_zeros() as u8, 24, 8]));
    }
    chunk(&mut out, tempo_track);
    for part in parts {
        let c = part.channel & 0x0F;
        let mut events: Vec<(u64, u8, Vec<u8>)> = Vec::new();
        let name = part.name.as_bytes();
        let mut meta = vec![0xFF, 0x03];
        vlq(&mut meta, name.len() as u32);
        meta.extend(name);
        events.push((0, 0, meta));
        if let Some(p) = part.program {
            events.push((0, 1, vec![0xC0 | c, p & 0x7F]));
        }
        for (at, message) in control_messages(&part.controls, c, &tick) {
            events.push((at, 1, message));
        }
        for n in &part.notes {
            let velocity = ((n.velocity * 127.0).round() as u8).clamp(1, 127);
            let (on, off) = (tick(n.start), tick(n.start + n.length).max(tick(n.start) + 1));
            events.push((on, 3, vec![0x90 | c, n.pitch & 0x7F, velocity]));
            // At the same tick, offs before ons: a note repeated isn't cut.
            events.push((off, 2, vec![0x80 | c, n.pitch & 0x7F, 0]));
        }
        events.sort_by_key(|e| (e.0, e.1));
        chunk(&mut out, events.into_iter().map(|(t, _, e)| (t, e)).collect());
    }
    out
}

/// What a clip's notes are as a part: within its length, from its start.
fn part_of(name: &str, clip: &MidiClip, drums: bool) -> Part {
    let notes = clip.notes.iter().filter(|n| n.start < clip.length).map(|n| Note { length: n.length.min(clip.length - n.start), ..*n }).collect();
    let controls = crate::expression::slice(&clip.controls, clip.length, 0.0, clip.length);
    Part { name: name.to_string(), channel: if drums { DRUM_CHANNEL } else { 0 }, program: None, notes, controls }
}

/// How long a clip for `notes` starting at `start` is: up to the song's bar the
/// last one ends in, a bar at least.
fn length_of(notes: &[Note], meter: &Meter, start: f64) -> f64 {
    let end = notes.iter().map(|n| n.start + n.length).fold(0.0, f64::max);
    (meter.next_bar(start + end - 1e-9) - start).max(meter.add_bars(start, 1) - start)
}

impl PulseApp {
    /// Asks for .mid files and brings them in as tracks of their own.
    pub fn choose_midi_files(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter("MIDI", &["mid", "midi"]);
        if let Some(paths) = crate::files::pick_files(dialog) {
            for path in paths {
                self.import_midi(&path, None);
            }
        }
    }

    /// Brings in a MIDI file. Onto a MIDI track (`into`: the track, and the
    /// slot or the beat): all its notes in one clip there. Otherwise, each of
    /// its parts on a track of its own (drums on a drum machine), in the
    /// Arrangement at the start, or in the first slot in the Session. An empty
    /// project takes its tempo.
    pub fn import_midi(&mut self, path: &Path, into: Option<crate::app::Placement>) -> bool {
        let file = path.file_stem().map_or("MIDI".into(), |s| s.to_string_lossy().into_owned());
        let song = match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| read(&b)) {
            Ok(song) => song,
            Err(e) => {
                self.status = Some(format!("No se pudo leer «{file}»: {e}"));
                return false;
            }
        };
        if song.parts.is_empty() {
            self.status = Some(format!("«{file}» no tiene notas"));
            return false;
        }
        let empty = self.project.tracks.iter().all(|t| t.arrangement.is_empty() && t.clips.iter().all(Option::is_none));
        let mut tempo_note = String::new();
        if let Some(tempo) = song.tempo {
            if empty {
                self.project.tempo = tempo.clamp(20.0, 999.0);
                tempo_note = format!(", a su tempo ({tempo:.0} BPM)");
            } else if (tempo - self.project.tempo).abs() > 0.5 {
                tempo_note = format!(". El archivo va a {tempo:.0} BPM: suena al tempo del proyecto");
            }
        }
        if song.tempo_changes > 0 {
            tempo_note.push_str(". Sus cambios de tempo no se traen");
        }
        if let Some(meter) = song.meter.filter(|m| *m != self.project.meter) {
            if empty {
                self.set_meter(meter);
                tempo_note.push_str(&format!(", en {}", meter.first().label()));
            } else {
                tempo_note.push_str(&format!(". El archivo va en {}: suena en el compás del proyecto", meter.first().label()));
            }
        }
        let before = self.project.clone();
        let placed = match into {
            Some(placement) => {
                let notes: Vec<Note> = {
                    let mut all: Vec<Note> = song.parts.iter().flat_map(|p| p.notes.iter().cloned()).collect();
                    all.sort_by(|a, b| a.start.total_cmp(&b.start));
                    all
                };
                let at = match placement {
                    crate::app::Placement::Arrangement { start, .. } => start,
                    _ => 0.0,
                };
                // One lane a controller: the first part's that has it.
                let mut controls: Vec<ControlLane> = Vec::new();
                for lane in song.parts.iter().flat_map(|p| &p.controls) {
                    if !controls.iter().any(|l| l.control == lane.control) {
                        controls.push(lane.clone());
                    }
                }
                controls.sort_by_key(|l| l.control);
                let clip = MidiClip { notes: notes.clone(), controls, ..MidiClip::new(&file, length_of(&notes, &self.project.meter, at), PALETTE[(self.project.tracks.len() * 3) % PALETTE.len()]) };
                match placement {
                    crate::app::Placement::Session { track, slot } => self.project.tracks[track].clips[slot] = Some(Clip::Midi(clip)),
                    crate::app::Placement::Arrangement { track, start } => {
                        let length = clip.length;
                        self.project.tracks[track].arrangement.push(ArrangementClip { start, length, clip: Clip::Midi(clip) });
                        self.project.tracks[track].arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
                    }
                    crate::app::Placement::Auto => {}
                }
                1
            }
            None => {
                let session = self.view == crate::app::View::Session;
                let mut made = 0;
                for part in &song.parts {
                    let index = self.project.tracks.len();
                    if index >= engine::MAX_TRACKS {
                        break;
                    }
                    let color = PALETTE[(index * 5 + 1) % PALETTE.len()];
                    let drums = part.channel == DRUM_CHANNEL;
                    let instrument = if drums { InstrumentKind::DrumMachine(Default::default()) } else { instrument_for(part.program) };
                    let mut track = Track::new(part.name.clone(), color, instrument);
                    let clip = MidiClip { notes: part.notes.clone(), controls: part.controls.clone(), ..MidiClip::new(&part.name, length_of(&part.notes, &self.project.meter, 0.0), color) };
                    if session {
                        track.clips[0] = Some(Clip::Midi(clip));
                    } else {
                        let length = clip.length;
                        track.arrangement.push(ArrangementClip { start: 0.0, length, clip: Clip::Midi(clip) });
                    }
                    self.project.tracks.push(track);
                    made += 1;
                }
                if made > 0 {
                    self.selected_target = engine::Target::Track(before.tracks.len());
                }
                made
            }
        };
        self.sync_from(&before);
        let what = if into.is_some() { "en la pista".to_string() } else if placed == 1 { "en una pista nueva".to_string() } else { format!("en {placed} pistas nuevas") };
        let lost = song.parts.len().saturating_sub(if into.is_some() { song.parts.len() } else { placed });
        let lost = if lost > 0 { format!(". {lost} partes no caben (máximo de {} pistas)", engine::MAX_TRACKS) } else { String::new() };
        self.status = Some(format!("«{file}» {what}{tempo_note}{lost}"));
        placed > 0
    }

    /// The MIDI tracks' Arrangement clips, each track a part (drums on the
    /// 10th channel), at the project's tempo.
    pub fn song_parts(&self) -> Vec<Part> {
        let mut parts = Vec::new();
        for track in self.project.tracks.iter().filter(|t| !t.is_audio() && !t.is_group()) {
            let mut notes: Vec<Note> = Vec::new();
            for placed in &track.arrangement {
                let Clip::Midi(clip) = &placed.clip else { continue };
                if clip.muted {
                    continue;
                }
                // A clip shorter in the Arrangement than its own length plays
                // that much; longer, it loops.
                let mut offset = 0.0;
                while offset < placed.length - 1e-9 && clip.length > 0.0 {
                    for n in &clip.notes {
                        let start = offset + n.start;
                        if n.start < clip.length && start < placed.length - 1e-9 {
                            let length = n.length.min(clip.length - n.start).min(placed.length - start);
                            notes.push(Note { start: placed.start + start, length, ..*n });
                        }
                    }
                    offset += clip.length;
                }
            }
            if !notes.is_empty() {
                notes.sort_by(|a, b| a.start.total_cmp(&b.start));
                // The clips' lanes along the song, as the engine plays them.
                let heard: Vec<ArrangementClip> = track.arrangement.iter().filter(|p| matches!(&p.clip, Clip::Midi(m) if !m.muted)).cloned().collect();
                let controls = crate::clip_split::join_controls(&heard, 0.0);
                parts.push(Part { name: track.name.clone(), channel: if track.is_drums() { DRUM_CHANNEL } else { (parts.len() % 15) as u8 + if parts.len() % 15 >= DRUM_CHANNEL as usize { 1 } else { 0 } }, program: None, notes, controls });
            }
        }
        parts
    }

    /// Writes the song (the Arrangement's MIDI) to a .mid file the user picks.
    pub fn export_song_midi(&mut self) {
        let parts = self.song_parts();
        if parts.is_empty() {
            self.status = Some("No hay clips MIDI en el Arrangement para exportar".into());
            return;
        }
        let dialog = rfd::FileDialog::new().add_filter("MIDI", &["mid"]).set_file_name(format!("{}.mid", self.file_name()));
        let Some(path) = crate::files::save_file(dialog) else { return };
        self.write_midi(&path, &parts);
    }

    /// Writes one MIDI clip to a .mid file the user picks.
    pub fn export_clip_midi(&mut self, clip: ClipRef) {
        let Some(Clip::Midi(midi)) = self.clip(clip).cloned() else { return };
        let drums = self.project.tracks.get(clip.track()).is_some_and(|t| t.is_drums());
        let dialog = rfd::FileDialog::new().add_filter("MIDI", &["mid"]).set_file_name(format!("{}.mid", midi.name));
        let Some(path) = crate::files::save_file(dialog) else { return };
        self.write_midi(&path, &[part_of(&midi.name, &midi, drums)]);
    }

    pub(crate) fn write_midi(&mut self, path: &Path, parts: &[Part]) {
        let bytes = write(parts, self.project.tempo, &self.project.meter);
        self.status = Some(match crate::store::write_atomic(path, &bytes) {
            Ok(()) => format!("MIDI guardado en «{}»", path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())),
            Err(e) => format!("No se pudo guardar el MIDI: {e}"),
        });
    }
}

/// Whether `path` is a MIDI file (by its extension).
pub fn is_midi(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("mid") || e.eq_ignore_ascii_case("midi"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    fn note(start: f64, length: f64, pitch: u8, velocity: f32) -> Note {
        Note { start, length, pitch, velocity, chance: 1.0 }
    }

    /// What is written reads back: the parts, their channels and names, the
    /// tempo, and every note (velocity to the nearest of 127 steps).
    #[test]
    fn a_written_file_reads_back() {
        let melody = Part { name: "Melodía".into(), channel: 0, program: Some(80), notes: vec![note(0.0, 1.0, 60, 0.8), note(1.0, 0.5, 64, 0.5), note(1.0, 0.5, 67, 0.5), note(2.5, 1.5, 60, 1.0)], controls: Vec::new() };
        // The same note again right where it ends: not cut.
        let drums = Part { name: "Batería".into(), channel: DRUM_CHANNEL, program: None, notes: vec![note(0.0, 0.25, 36, 1.0), note(0.25, 0.25, 36, 0.7), note(1.0, 0.25, 38, 0.9)], controls: Vec::new() };
        let bytes = write(&[melody.clone(), drums.clone()], 128.0, &Meter::COMMON);
        let song = read(&bytes).unwrap();
        assert!((song.tempo.unwrap() - 128.0).abs() < 0.01);
        assert_eq!(song.parts.len(), 2);
        for (got, want) in song.parts.iter().zip([&melody, &drums]) {
            assert_eq!((got.name.as_str(), got.channel, got.program), (want.name.as_str(), want.channel, want.program));
            assert_eq!(got.notes.len(), want.notes.len(), "{}", got.name);
            for (a, b) in got.notes.iter().zip(&want.notes) {
                assert!((a.start - b.start).abs() < 1e-9 && (a.length - b.length).abs() < 1e-9 && a.pitch == b.pitch && (a.velocity - b.velocity).abs() < 0.005, "{a:?} / {b:?}");
            }
        }
    }

    /// Pitch bend, pressure and controllers go out and come back as lanes:
    /// a ramp as a ramp, a jump as a jump, each value to the nearest step.
    #[test]
    fn controller_lanes_go_out_and_come_back() {
        use engine::model::AutomationPoint;
        let point = |beat: f64, value: f32| AutomationPoint { beat, value, curve: 0.0 };
        let lanes = vec![
            ControlLane { control: MidiControl::PitchBend, points: vec![point(0.0, 0.0), point(1.0, 0.0), point(2.0, 1.0), point(3.0, -0.5)] },
            ControlLane { control: MidiControl::Pressure, points: vec![point(0.5, 0.2), point(1.5, 0.9)] },
            ControlLane { control: MidiControl::MOD_WHEEL, points: vec![point(0.0, 0.0), point(1.99, 0.0), point(2.0, 1.0)] },
            ControlLane { control: MidiControl::Cc(74), points: vec![AutomationPoint { beat: 0.0, value: 0.2, curve: 0.7 }, point(4.0, 0.9)] },
        ];
        let part = Part { name: "Lead".into(), channel: 3, program: None, notes: vec![note(0.0, 4.0, 60, 0.8)], controls: lanes.clone() };
        let song = read(&write(&[part], 120.0, &Meter::COMMON)).unwrap();
        let got = &song.parts[0].controls;
        assert_eq!(got.iter().map(|l| l.control).collect::<Vec<_>>(), [MidiControl::PitchBend, MidiControl::Pressure, MidiControl::Cc(1), MidiControl::Cc(74)]);
        for (a, b) in got.iter().zip(&lanes) {
            assert!(a.points.len() < 40, "{:?}: {} points", a.control, a.points.len());
            for i in 0..=64 {
                let beat = i as f64 / 16.0;
                let (x, y) = (a.value_at(beat).unwrap(), b.value_at(beat).unwrap());
                // Within a step and a 32nd of a beat of the ramp's slope.
                let slack = 1.5 / 127.0 + (b.value_at(beat + 1.0 / 32.0).unwrap() - y).abs();
                assert!((x - y).abs() <= slack, "{:?} at {beat}: {x} vs {y}", a.control);
            }
        }
        // The jump stays a jump.
        let wheel = &got[2];
        assert!(wheel.value_at(1.95).unwrap() < 0.01 && wheel.value_at(2.0).unwrap() > 0.99, "{:?}", wheel.points);
    }

    /// A file as other programs write them: format 0, every channel in one
    /// track, running status, notes let go with a «note on» at 0, 96 ticks a
    /// beat, a tempo of 100 and a program change. Each channel is a part.
    #[test]
    fn a_format_0_file_with_running_status_is_read() {
        let mut track = Vec::new();
        track.extend([0x00, 0xFF, 0x51, 0x03, 0x09, 0x27, 0xC0]); // 600 000 µs: 100 BPM
        track.extend([0x00, 0xC1, 33]); // channel 2: a bass (program 34)
        track.extend([0x00, 0x90, 60, 100]); // C4 on, channel 1
        track.extend([0x00, 64, 100]); // E4 on (running status)
        track.extend([0x60, 60, 0]); // a beat later: C4 off (on at 0)
        track.extend([0x00, 64, 0]); // E4 off
        track.extend([0x00, 0x91, 36, 90]); // C2 on, channel 2
        track.extend([0x81, 0x40, 0x81, 36, 0]); // two beats later (192 ticks): off
        track.extend([0x00, 0x99, 42, 127]); // a hat, channel 10, never let go
        track.extend([0x18, 0xFF, 0x2F, 0x00]); // end of track a quarter beat later
        let mut bytes = b"MThd".to_vec();
        bytes.extend([0, 0, 0, 6, 0, 0, 0, 1, 0, 96]);
        bytes.extend(b"MTrk");
        bytes.extend((track.len() as u32).to_be_bytes());
        bytes.extend(track);
        let song = read(&bytes).unwrap();
        assert!((song.tempo.unwrap() - 100.0).abs() < 0.01);
        let names: Vec<&str> = song.parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["MIDI 1 (canal 1)", "Bajo (canal 2)", "Batería (canal 10)"]);
        assert_eq!(song.parts[0].notes, [note(1.0, 1.0, 60, 100.0 / 127.0), note(1.0, 1.0, 64, 100.0 / 127.0)].map(|n| Note { start: 0.0, ..n }));
        assert_eq!(song.parts[1].notes, [note(1.0, 2.0, 36, 90.0 / 127.0)]);
        assert_eq!(song.parts[2].notes, [note(3.0, 0.25, 42, 1.0)]);
        assert!(read(b"RIFF....").unwrap_err().contains("No es un archivo MIDI"));
        assert!(read(&bytes[..30]).is_err(), "cut short");
    }

    /// Time signatures go out and come back: a waltz that turns 7/8 at its third
    /// bar, and an empty project takes them (a project with songs keeps its own).
    #[test]
    fn time_signatures_go_out_and_come_back() {
        let meter = Meter::new(TimeSig::new(3, 4)).with_change(2, TimeSig::new(7, 8));
        let part = Part { name: "Vals".into(), channel: 0, program: None, notes: vec![note(0.0, 1.0, 60, 0.8), note(6.0, 0.5, 62, 0.8)], controls: Vec::new() };
        let song = read(&write(&[part.clone()], 90.0, &meter)).unwrap();
        assert_eq!(song.meter, Some(meter));
        assert_eq!(read(&write(&[part.clone()], 90.0, &Meter::COMMON)).unwrap().meter, Some(Meter::COMMON));

        let dir = std::env::temp_dir().join(format!("pulse-midi-meter-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Vals.mid");
        std::fs::write(&path, write(&[part], 90.0, &meter)).unwrap();
        let mut project = Project::empty();
        project.tracks.clear();
        let mut app = PulseApp::headless(project);
        app.view = crate::app::View::Arrangement;
        assert!(app.import_midi(&path, None));
        assert_eq!(app.project.meter, meter);
        // Up to the bar the last note ends in: bars 0 and 1 are 3 beats, bar 2 is 3.5.
        assert_eq!(app.project.tracks[0].arrangement[0].length, 9.5);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The sustain pedal (controller 64) holds the keys let go under it until it
    /// comes up, or until the same key is struck again; pianos, electric pianos
    /// and organs come in on Pulse Keys.
    #[test]
    fn the_sustain_pedal_lengthens_the_notes_it_holds() {
        let mut track = Vec::new();
        track.extend([0x00, 0xC0, 4]); // an electric piano
        track.extend([0x00, 0xB0, 64, 127]); // pedal down
        track.extend([0x00, 0x90, 60, 100]); // C4
        track.extend([0x60, 0x80, 60, 0]); // let go a beat later: held
        track.extend([0x00, 0x90, 64, 100]); // E4
        track.extend([0x30, 0x90, 64, 0]); // let go half a beat later: held
        track.extend([0x30, 0x90, 64, 90]); // E4 again: the first E4 ends here
        track.extend([0x30, 0xB0, 64, 0]); // pedal up at beat 2.5: C4 ends
        track.extend([0x30, 0x80, 64, 0]); // E4 let go with the pedal up
        track.extend([0x00, 0xFF, 0x2F, 0x00]);
        let mut bytes = b"MThd".to_vec();
        bytes.extend([0, 0, 0, 6, 0, 0, 0, 1, 0, 96]);
        bytes.extend(b"MTrk");
        bytes.extend((track.len() as u32).to_be_bytes());
        bytes.extend(track);
        let song = read(&bytes).unwrap();
        let mut notes = song.parts[0].notes.clone();
        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
        let got: Vec<(f64, f64, u8)> = notes.iter().map(|n| (n.start, n.length, n.pitch)).collect();
        assert_eq!(got, [(0.0, 2.5, 60), (1.0, 1.0, 64), (2.0, 1.0, 64)]);
        assert!(matches!(instrument_for(song.parts[0].program), InstrumentKind::Keys(p) if p.model == engine::model::KeysModel::Tines));
        assert!(matches!(instrument_for(Some(18)), InstrumentKind::Keys(p) if p.model == engine::model::KeysModel::Organ));
        assert!(matches!(instrument_for(Some(33)), InstrumentKind::Synth(_)));
        assert!(matches!(instrument_for(None), InstrumentKind::Synth(_)));
    }

    /// Into an empty project: a track a part, drums on a drum machine, the
    /// file's tempo. Onto a MIDI track's lane: one clip there. And the song
    /// written out has every note where it plays, a looped clip too.
    #[test]
    fn files_come_in_as_tracks_and_the_song_goes_out() {
        let dir = std::env::temp_dir().join(format!("pulse-midi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Tema.mid");
        let keys = Part { name: "Piano".into(), channel: 0, program: None, notes: vec![note(0.0, 2.0, 60, 0.8), note(4.0, 2.0, 62, 0.8), note(8.0, 2.0, 64, 0.8)], controls: Vec::new() };
        let drums = Part { name: "Batería".into(), channel: DRUM_CHANNEL, program: None, notes: (0..8).map(|b| note(b as f64, 0.25, 36, 1.0)).collect(), controls: Vec::new() };
        std::fs::write(&path, write(&[keys, drums], 92.0, &Meter::COMMON)).unwrap();
        let mut project = Project::empty();
        project.tracks.clear();
        let mut app = PulseApp::headless(project);
        app.view = crate::app::View::Arrangement;
        assert!(app.import_midi(&path, None));
        assert_eq!(app.project.tempo.round(), 92.0);
        assert_eq!(app.project.tracks.len(), 2);
        assert!(!app.project.tracks[0].is_drums() && app.project.tracks[1].is_drums());
        let placed = &app.project.tracks[0].arrangement[0];
        assert_eq!((placed.start, placed.length), (0.0, 12.0), "up to the bar it ends in");
        // Onto the piano's lane at beat 16: all its notes in one clip.
        assert!(app.import_midi(&path, Some(crate::app::Placement::Arrangement { track: 0, start: 16.0 })));
        assert_eq!(app.project.tracks.len(), 2);
        let second = &app.project.tracks[0].arrangement[1];
        let Clip::Midi(clip) = &second.clip else { panic!() };
        assert_eq!((second.start, clip.notes.len()), (16.0, 11));
        // The drums' clip looped twice over in the Arrangement.
        app.project.tracks[1].arrangement[0].length = 16.0;
        let parts = app.song_parts();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].channel, DRUM_CHANNEL);
        assert_eq!(parts[1].notes.len(), 16, "the loop's second time too");
        assert_eq!(parts[1].notes[8].start, 8.0);
        let back = read(&write(&parts, app.project.tempo, &app.project.meter)).unwrap();
        assert_eq!(back.parts[0].notes.len(), 3 + 11);
        assert!((back.parts[0].notes.last().unwrap().start - 24.0).abs() < 1e-9);
        // A project with clips keeps its tempo, and says the file's.
        app.project.tempo = 120.0;
        app.import_midi(&path, None);
        assert_eq!(app.project.tempo, 120.0);
        assert!(app.status.as_deref().unwrap().contains("92 BPM"), "{:?}", app.status);
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod lakh {
    /// Real files: every 60th of the Lakh MIDI set the drum model learned
    /// from (if it is here). Nearly all read (what doesn't is not MIDI, or
    /// cut short), and each reads back the same once written. Compared with
    /// mido on 2976 of them, the same notes, drums and tempo in every one.
    #[test]
    #[ignore = "needs ml/drums/data/lmd_full"]
    fn the_lakh_files_read_and_round_trip() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ml/drums/data/lmd_full");
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&root).unwrap().flatten().flat_map(|d| std::fs::read_dir(d.path()).into_iter().flatten().flatten().map(|f| f.path())).collect();
        files.sort();
        let (mut read, mut failed) = (0, 0);
        for path in files.iter().step_by(60) {
            match super::read(&std::fs::read(path).unwrap()) {
                Ok(song) => {
                    read += 1;
                    let again = super::read(&super::write(&song.parts, song.tempo.unwrap_or(120.0), &engine::meter::Meter::COMMON)).unwrap();
                    assert_eq!(again.parts.len(), song.parts.len(), "{}", path.display());
                    for (a, b) in again.parts.iter().zip(&song.parts) {
                        assert_eq!(a.notes.len(), b.notes.len(), "{}", path.display());
                        // Written at 480 ticks a beat: as close as that (two notes
                        // within a tick of each other may swap).
                        let key = |n: &super::Note| ((n.start * 480.0).round() as i64, n.pitch);
                        let (mut x, mut y): (Vec<_>, Vec<_>) = (a.notes.iter().map(key).collect(), b.notes.iter().map(key).collect());
                        x.sort();
                        y.sort();
                        assert!(x.iter().zip(&y).all(|(p, q)| p.1 == q.1 && (p.0 - q.0).abs() <= 1), "{}", path.display());
                    }
                }
                Err(_) => failed += 1,
            }
        }
        println!("{read} leídos, {failed} no");
        assert!(read > 2000 && failed * 100 < (read + failed) * 3);
    }
}
