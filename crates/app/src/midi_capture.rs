//! «Capturar MIDI», as Live's Capture MIDI or Cubase's Retrospective Record:
//! Pulse always listens to what you play (on the armed track, or the selected
//! one), recording or not. Capturing makes a clip of the last thing played.
//!
//! The notes come from the live input as they are sent (`engine::HeardNote`,
//! with the moment each was played) and are kept here, on the UI thread: the
//! audio thread never sees this buffer. Each is noted with the beat it fell on
//! if the transport was playing.
//!
//! What a capture takes: the last phrase (notes with no silence between them
//! longer than [`PHRASE_GAP`]), at most [`MAX_BARS`] bars of it (the last
//! ones). Played over the transport, it keeps its beats: the clip starts at the
//! bar of its first note and is a whole number of bars long (1, 2, 4 or 8).
//! Played with the transport stopped, its first note is the clip's downbeat;
//! in a project with nothing in it yet, the tempo is taken from what was
//! played (see [`detect_tempo`]), unless it is close to the project's.

use crate::app::{ClipRef, PulseApp, View};
use engine::meter::Meter;
use engine::model::{ArrangementClip, Clip, MidiClip, Note};
use engine::{Controller, HeardNote};
use std::collections::VecDeque;
use std::time::Instant;

/// Silence (seconds) that ends a phrase: what came before it is not captured.
pub const PHRASE_GAP: f64 = 3.0;
/// The longest a capture is.
pub const MAX_BARS: i64 = 8;
/// Notes kept (the oldest go first): far more than eight bars of anyone.
const KEEP: usize = 4096;
/// How early (beats) a note may come before a bar line and still start it.
const EARLY: f64 = 0.25;
/// Notes this close (seconds) start together (a chord) for finding the tempo.
const TOGETHER: f64 = 0.035;
/// The tempos a capture can find.
const TEMPOS: (f64, f64) = (70.0, 160.0);
/// A tempo found this close (a share) to the project's keeps the project's.
const CLOSE: f64 = 0.03;
/// The sustain pedal's controller.
const PEDAL: u8 = 64;

/// A note heard: when it began and ended (seconds since the listener started),
/// and, if the transport was playing, on which beats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Heard {
    pub pitch: u8,
    pub velocity: f32,
    pub start: f64,
    pub end: f64,
    pub beats: Option<(f64, f64)>,
}

/// The transport when notes are read: whether it plays, where, how fast, and
/// how late what is heard comes out (the player answers to that).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transport {
    pub now: Instant,
    pub playing: bool,
    pub position: f64,
    pub tempo: f64,
    pub latency: f64,
}

impl Transport {
    /// The beat a note played at `at` fell on, if the transport was playing.
    fn beat(&self, at: Instant) -> Option<f64> {
        let ago = self.now.saturating_duration_since(at).as_secs_f64() + self.latency;
        self.playing.then(|| (self.position - ago * self.tempo / 60.0).max(0.0))
    }
}

/// What a capture makes.
#[derive(Clone, Debug, PartialEq)]
pub struct Captured {
    pub notes: Vec<Note>,
    pub length: f64,
    /// Where it was played on the transport (the clip's first beat), if it was playing.
    pub start: Option<f64>,
    /// The tempo found in it, when asked to find one and it differs from the project's.
    pub tempo: Option<f64>,
}

/// Listens to the live input and keeps what was played.
pub struct Listener {
    queue: Option<engine::HeardQueue>,
    epoch: Option<Instant>,
    /// Keys down: when (seconds), on which beat, how hard.
    held: [Option<(f64, Option<f64>, f32)>; 128],
    /// Keys let go of while the pedal is down: they sound until it comes up.
    sustained: [bool; 128],
    pedal: bool,
    notes: VecDeque<Heard>,
    /// What started before this (seconds) was captured already.
    taken: f64,
}

impl Default for Listener {
    fn default() -> Self {
        Self { queue: None, epoch: None, held: [None; 128], sustained: [false; 128], pedal: false, notes: VecDeque::new(), taken: f64::NEG_INFINITY }
    }
}

impl Listener {
    /// Starts listening to `live` if it isn't already (or its input went away).
    pub fn connect(&mut self, live: Option<&engine::LiveInput>) {
        let gone = self.queue.as_ref().is_none_or(|q| q.is_abandoned());
        if let (true, Some(live)) = (gone, live) {
            self.queue = Some(live.listen());
        }
    }

    /// Reads what was played since the last call.
    pub fn pump(&mut self, transport: Transport) {
        let mut heard = Vec::new();
        if let Some(queue) = self.queue.as_mut() {
            while let Ok(note) = queue.pop() {
                heard.push(note);
            }
        }
        for note in heard {
            self.hear(note, transport);
        }
    }

    fn seconds(&mut self, at: Instant) -> f64 {
        let epoch = *self.epoch.get_or_insert(at);
        if at >= epoch { at.duration_since(epoch).as_secs_f64() } else { -epoch.duration_since(at).as_secs_f64() }
    }

    /// One note (or pedal) as played, read when the transport was as `transport` says.
    pub fn hear(&mut self, heard: HeardNote, transport: Transport) {
        let time = self.seconds(heard.at);
        let beat = transport.beat(heard.at);
        let note = heard.note;
        let pitch = note.pitch.min(127) as usize;
        match note.controller {
            Some(Controller::Cc(PEDAL)) => {
                let down = note.velocity >= 0.5;
                if self.pedal && !down {
                    for p in 0..128 {
                        if std::mem::take(&mut self.sustained[p]) {
                            self.end(p, time, beat);
                        }
                    }
                }
                self.pedal = down;
            }
            Some(_) => {}
            None if note.velocity > 0.0 => {
                // Struck again while the pedal held it: that one ends here.
                if std::mem::take(&mut self.sustained[pitch]) || self.held[pitch].is_some() {
                    self.end(pitch, time, beat);
                }
                self.held[pitch] = Some((time, beat, note.velocity));
            }
            None if self.pedal => self.sustained[pitch] = self.held[pitch].is_some(),
            None => self.end(pitch, time, beat),
        }
    }

    fn end(&mut self, pitch: usize, time: f64, beat: Option<f64>) {
        let Some((start, from, velocity)) = self.held[pitch].take() else { return };
        let beats = from.zip(beat).map(|(a, b)| (a, b.max(a)));
        self.notes.push_back(Heard { pitch: pitch as u8, velocity, start, end: time.max(start), beats });
        while self.notes.len() > KEEP {
            self.notes.pop_front();
        }
    }

    /// What a capture would take now (keys still down end now), oldest first.
    pub fn pending(&self, transport: Transport) -> Vec<Heard> {
        let now = self.epoch.map_or(0.0, |e| transport.now.saturating_duration_since(e).as_secs_f64());
        let beat = transport.beat(transport.now);
        let mut notes: Vec<Heard> = self.notes.iter().copied().filter(|n| n.start > self.taken).collect();
        for (pitch, held) in self.held.iter().enumerate() {
            if let Some((start, from, velocity)) = *held {
                if start > self.taken {
                    notes.push(Heard { pitch: pitch as u8, velocity, start, end: now.max(start), beats: from.zip(beat).map(|(a, b)| (a, b.max(a))) });
                }
            }
        }
        notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
        notes
    }

    /// Whether there is something to capture.
    pub fn has_notes(&self) -> bool {
        self.notes.back().is_some_and(|n| n.start > self.taken) || self.held.iter().flatten().any(|h| h.0 > self.taken)
    }

    /// What was captured is not captured again.
    pub fn taken(&mut self, notes: &[Heard]) {
        if let Some(last) = notes.iter().map(|n| n.start).reduce(f64::max) {
            self.taken = self.taken.max(last);
        }
    }
}

/// The last phrase of `notes` (sorted by start): from the last silence longer
/// than [`PHRASE_GAP`] on.
pub fn last_phrase(notes: &[Heard]) -> &[Heard] {
    let mut from = 0;
    let mut sounding = f64::NEG_INFINITY;
    for (i, n) in notes.iter().enumerate() {
        if i > 0 && n.start - sounding > PHRASE_GAP {
            from = i;
        }
        sounding = sounding.max(n.end);
    }
    &notes[from..]
}

/// The tempo `onsets` (seconds, sorted) were played at: the one, between 70
/// and 160 BPM, that puts them best on a grid of eighths starting at the first,
/// closest to 120 among equals, then fitted to them. None with fewer than four.
pub fn detect_tempo(onsets: &[f64]) -> Option<f64> {
    let mut starts: Vec<f64> = Vec::new();
    for &t in onsets {
        if starts.last().is_none_or(|&last| t - last > TOGETHER) {
            starts.push(t);
        }
    }
    if starts.len() < 4 {
        return None;
    }
    let first = starts[0];
    let fit = |bpm: f64| {
        let step = 30.0 / bpm;
        let sigma = 0.025;
        starts.iter().map(|&t| {
            let x = (t - first) / step;
            let off = (x - x.round()).abs() * step;
            (-(off / sigma).powi(2) / 2.0).exp()
        }).sum::<f64>() / starts.len() as f64
    };
    let score = |bpm: f64| fit(bpm) - 0.06 * (bpm / 120.0).log2().abs();
    let mut best = (f64::NEG_INFINITY, 120.0);
    let mut bpm = TEMPOS.0;
    while bpm < TEMPOS.1 + 1e-9 {
        let s = score(bpm);
        if s > best.0 {
            best = (s, bpm);
        }
        bpm += 0.25;
    }
    if fit(best.1) < 0.5 {
        return None;
    }
    // Fitted: the onsets' times against their steps of the grid (least squares).
    let step = 30.0 / best.1;
    let k: Vec<f64> = starts.iter().map(|&t| ((t - first) / step).round()).collect();
    let n = k.len() as f64;
    let (mk, mt) = (k.iter().sum::<f64>() / n, starts.iter().sum::<f64>() / n);
    let (mut num, mut den) = (0.0, 0.0);
    for (&k, &t) in k.iter().zip(&starts) {
        num += (k - mk) * (t - mt);
        den += (k - mk) * (k - mk);
    }
    let fitted = if den > 0.0 { 30.0 / (num / den) } else { best.1 };
    let fitted = if (fitted - best.1).abs() < 2.0 { fitted } else { best.1 };
    Some((fitted * 10.0).round() / 10.0)
}

/// 1, 2, 4 or 8 bars: the fewest that hold `bars`.
fn whole(bars: i64) -> i64 {
    [1, 2, 4, MAX_BARS].into_iter().find(|&b| b >= bars).unwrap_or(MAX_BARS)
}

/// A clip from what was played (see the module's notes). `find_tempo`: the
/// project has nothing yet, the tempo may come from the notes.
pub fn capture(notes: &[Heard], meter: &Meter, tempo: f64, find_tempo: bool) -> Option<Captured> {
    let phrase = last_phrase(notes);
    if phrase.is_empty() {
        return None;
    }
    if phrase.iter().all(|n| n.beats.is_some()) {
        let beats: Vec<(f64, f64)> = phrase.iter().filter_map(|n| n.beats).collect();
        let last_end = beats.iter().map(|b| b.1).fold(f64::NEG_INFINITY, f64::max);
        let first = beats.iter().map(|b| b.0).fold(f64::INFINITY, f64::min);
        let end = meter.next_bar(last_end.max(first + 1e-6) - 1e-9);
        let mut start = meter.prev_bar(first + EARLY);
        // The last eight bars at most.
        let earliest = meter.add_bars(end, -MAX_BARS);
        start = start.max(earliest);
        let bars = count_bars(meter, start, end);
        let end = meter.add_bars(start, whole(bars));
        let length = end - start;
        let notes = phrase
            .iter()
            .filter_map(|n| {
                let (a, b) = n.beats?;
                let at = a - start;
                (at >= -EARLY && at < length).then(|| Note { start: at.max(0.0), length: (b - a.max(start)).clamp(crate::note_edit::MIN_LENGTH, length - at.max(0.0)), pitch: n.pitch, velocity: n.velocity, chance: 1.0 })
            })
            .collect();
        return Some(Captured { notes, length, start: Some(start), tempo: None });
    }
    // Stopped: the first note is the downbeat.
    let onsets: Vec<f64> = phrase.iter().map(|n| n.start).collect();
    let found = if find_tempo { detect_tempo(&onsets) } else { None };
    let found = found.filter(|&t| (t - tempo).abs() > tempo * CLOSE);
    let bpm = found.unwrap_or(tempo);
    let first = onsets[0];
    let to_beats = |t: f64| (t - first) * bpm / 60.0;
    let bar = meter.first().bar();
    let last_end = phrase.iter().map(|n| to_beats(n.end)).fold(0.0, f64::max);
    // The last eight bars at most: whole bars back from the end.
    let bars_played = ((last_end - 1e-6) / bar).floor() as i64 + 1;
    let skip = (bars_played - MAX_BARS).max(0) as f64 * bar;
    let length = whole(bars_played.min(MAX_BARS)) as f64 * bar;
    let notes = phrase
        .iter()
        .filter_map(|n| {
            let at = to_beats(n.start) - skip;
            (at >= 0.0 && at < length).then(|| Note { start: at, length: ((n.end - n.start) * bpm / 60.0).clamp(crate::note_edit::MIN_LENGTH, length - at), pitch: n.pitch, velocity: n.velocity, chance: 1.0 })
        })
        .collect();
    Some(Captured { notes, length, start: None, tempo: found })
}

/// Bars from `start` (a bar line) to `end` (one too), at least one.
fn count_bars(meter: &Meter, start: f64, end: f64) -> i64 {
    let mut bars = 0;
    let mut at = start;
    while at < end - 1e-9 && bars < 64 {
        at = meter.add_bars(at, 1);
        bars += 1;
    }
    bars.max(1)
}

impl PulseApp {
    /// The track a capture goes to: the first armed MIDI track, or the selected one.
    pub(crate) fn capture_track(&self) -> Option<usize> {
        let midi = |t: &crate::project::Track| !t.is_audio() && !t.is_group();
        if let Some(t) = self.project.tracks.iter().position(|t| t.armed && midi(t)) {
            return Some(t);
        }
        match self.selected_target {
            engine::Target::Track(t) if self.project.tracks.get(t).is_some_and(midi) => Some(t),
            _ => None,
        }
    }

    /// The transport as the listener reads notes against.
    fn capture_transport(&self) -> Transport {
        let state = self.audio.state();
        Transport {
            now: Instant::now(),
            playing: self.audio.is_playing(),
            position: self.audio.position(),
            tempo: state.as_ref().map(|s| s.tempo()).filter(|t| *t > 0.0).unwrap_or(self.project.tempo),
            latency: state.as_ref().map_or(0.0, |s| s.output_latency()),
        }
    }

    /// Every frame: what was played is read.
    pub(crate) fn listen_midi(&mut self) {
        let live = self.audio.live();
        self.midi_capture.connect(live.as_ref());
        let transport = self.capture_transport();
        self.midi_capture.pump(transport);
    }

    /// Whether «Capturar MIDI» has something to capture.
    pub fn can_capture(&self) -> bool {
        self.midi_capture.has_notes()
    }

    /// «Capturar MIDI»: the last thing played, as a clip on the capture track.
    pub fn capture_midi(&mut self) {
        let Some(track) = self.capture_track() else {
            self.status = Some("Capturar MIDI: selecciona o arma una pista MIDI".into());
            return;
        };
        let transport = self.capture_transport();
        let pending = self.midi_capture.pending(transport);
        // The tempo comes from the notes only into an empty project.
        let empty = self.project.tracks.iter().all(|t| t.arrangement.is_empty() && t.clips.iter().all(Option::is_none));
        let tempo_lane = self.project.master.automation.iter().any(|l| l.param == engine::model::Param::Tempo && !l.points.is_empty());
        let Some(captured) = capture(&pending, &self.project.meter, self.project.tempo, empty && !tempo_lane) else {
            self.status = Some("Capturar MIDI: aún no has tocado nada".into());
            return;
        };
        self.midi_capture.taken(&pending);
        if let Some(bpm) = captured.tempo {
            self.project.tempo = bpm;
            self.audio.send(engine::Command::SetTempo(bpm));
        }
        let count = captured.notes.len();
        let t = &self.project.tracks[track];
        let clip = MidiClip { name: "Captura".into(), length: captured.length, notes: captured.notes, color: t.color, envelopes: Vec::new(), groove: None, muted: false, controls: Vec::new() };
        let bars = captured.length / self.project.meter.first().bar();
        let tempo = captured.tempo.map_or(String::new(), |bpm| format!(", a {bpm} BPM"));
        match self.view {
            View::Session => {
                let Some(slot) = t.clips.iter().position(Option::is_none) else {
                    self.status = Some(format!("Capturar MIDI: «{}» no tiene ningún slot libre", t.name));
                    return;
                };
                self.set_clip(track, slot, Some(Clip::Midi(clip)));
                self.select_clip(ClipRef::Session { track, slot });
            }
            View::Arrangement => {
                let start = captured.start.unwrap_or_else(|| self.project.meter.prev_bar(transport.position));
                let lane = &mut self.project.tracks[track].arrangement;
                lane.push(ArrangementClip { start, length: clip.length, clip: Clip::Midi(clip) });
                let index = lane.len() - 1;
                self.sync_arrangement(track);
                self.select_clip(ClipRef::Arrangement { track, index });
            }
        }
        self.status = Some(format!("Capturadas {count} notas en «{}»: {bars} comp.{tempo}", self.project.tracks[track].name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::LiveNote;
    use std::time::Duration;

    fn stopped(now: Instant) -> Transport {
        Transport { now, playing: false, position: 0.0, tempo: 120.0, latency: 0.0 }
    }

    /// Notes (pitch, start, length in seconds) played into a listener, the transport stopped.
    fn played(listener: &mut Listener, base: Instant, notes: &[(u8, f64, f64)]) {
        let mut events: Vec<(f64, LiveNote)> = Vec::new();
        for &(pitch, start, length) in notes {
            events.push((start, LiveNote::note(pitch, 0.8)));
            events.push((start + length, LiveNote::note(pitch, 0.0)));
        }
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (t, note) in events {
            let at = base + Duration::from_secs_f64(t);
            listener.hear(HeardNote { at, note }, stopped(at));
        }
    }

    #[test]
    fn a_riff_played_stopped_into_an_empty_project_brings_its_tempo() {
        // Eighths at 96 BPM (0.3125 s), a little out, two bars of them.
        let base = Instant::now();
        let mut listener = Listener::default();
        let jitter = [0.0, 0.008, -0.006, 0.004, -0.009, 0.005, 0.0, -0.004];
        let notes: Vec<(u8, f64, f64)> = (0..16).map(|i| (60 + (i % 5) as u8, i as f64 * 0.3125 + jitter[i % 8], 0.2)).collect();
        played(&mut listener, base, &notes);
        let end = base + Duration::from_secs_f64(6.0);
        let pending = listener.pending(stopped(end));
        let captured = capture(&pending, &Meter::default(), 120.0, true).unwrap();
        let bpm = captured.tempo.expect("a tempo");
        assert!((bpm - 96.0).abs() < 0.6, "{bpm}");
        // Two bars of eighths: the first on the downbeat, the ninth on bar 2.
        assert_eq!(captured.length, 8.0);
        assert_eq!(captured.notes.len(), 16);
        assert!(captured.notes[0].start.abs() < 1e-9);
        assert!((captured.notes[8].start - 4.0).abs() < 0.05, "{}", captured.notes[8].start);
        // Not in an empty project: the project's tempo.
        let kept = capture(&pending, &Meter::default(), 120.0, false).unwrap();
        assert_eq!(kept.tempo, None);
        // At the project's tempo (a hair off), it stays.
        let close = capture(&pending, &Meter::default(), 97.0, true).unwrap();
        assert_eq!(close.tempo, None);
    }

    #[test]
    fn tempo_finding_keeps_to_its_range_and_needs_notes() {
        let quarters = |bpm: f64| (0..12).map(|i| i as f64 * 60.0 / bpm).collect::<Vec<_>>();
        assert!((detect_tempo(&quarters(100.0)).unwrap() - 100.0).abs() < 0.2);
        assert!((detect_tempo(&quarters(150.0)).unwrap() - 150.0).abs() < 0.2);
        assert!((detect_tempo(&quarters(128.0)).unwrap() - 128.0).abs() < 0.2);
        // A chord is one onset: three of them aren't enough.
        assert_eq!(detect_tempo(&[0.0, 0.01, 0.02, 0.5, 1.0]), None);
    }

    #[test]
    fn only_the_last_phrase_is_taken_and_once() {
        let base = Instant::now();
        let mut listener = Listener::default();
        // A phrase, five seconds of nothing, another of four notes.
        let mut notes: Vec<(u8, f64, f64)> = (0..6).map(|i| (50, i as f64 * 0.5, 0.25)).collect();
        notes.extend((0..4).map(|i| (70, 8.0 + i as f64 * 0.5, 0.25)));
        played(&mut listener, base, &notes);
        let now = stopped(base + Duration::from_secs(12));
        let pending = listener.pending(now);
        let captured = capture(&pending, &Meter::default(), 120.0, false).unwrap();
        assert_eq!(captured.notes.len(), 4);
        assert!(captured.notes.iter().all(|n| n.pitch == 70));
        // One bar at 120: four quarters.
        assert_eq!(captured.length, 4.0);
        listener.taken(&pending);
        assert!(!listener.has_notes());
        assert!(capture(&listener.pending(now), &Meter::default(), 120.0, false).is_none());
    }

    #[test]
    fn played_over_the_transport_it_keeps_its_bars_and_at_most_eight() {
        let base = Instant::now();
        let mut listener = Listener::default();
        // At 120 BPM from beat 16 (bar 5): a quarter each half second, a little
        // early on the downbeat, for ten bars.
        let tempo = 120.0;
        for i in 0..40 {
            let t = i as f64 * 0.5 - if i == 0 { 0.03 } else { 0.0 };
            for (dt, velocity) in [(t, 0.7), (t + 0.25, 0.0)] {
                let at = base + Duration::from_secs_f64(dt + 1.0);
                // Read at once, the transport where it was when played.
                let position = 16.0 + dt * tempo / 60.0;
                listener.hear(HeardNote { at, note: LiveNote::note(60 + (i % 3) as u8, velocity) }, Transport { now: at, playing: true, position, tempo, latency: 0.0 });
            }
        }
        let pending = listener.pending(Transport { now: base + Duration::from_secs(30), playing: true, position: 60.0, tempo, latency: 0.0 });
        let captured = capture(&pending, &Meter::default(), tempo, true).unwrap();
        // Ten bars played (5 to 14): the last eight, from bar 7 (beat 24).
        assert_eq!(captured.start, Some(24.0));
        assert_eq!(captured.length, 32.0);
        assert_eq!(captured.notes.len(), 32);
        assert!(captured.notes[0].start.abs() < 1e-6);
        assert_eq!(captured.tempo, None);

        // Three bars, the first note a hair early: from that bar line, four bars long.
        let short: Vec<Heard> = pending.iter().copied().filter(|n| n.beats.unwrap().0 < 28.0).collect();
        let captured = capture(&short, &Meter::default(), tempo, false).unwrap();
        assert_eq!(captured.start, Some(16.0));
        assert_eq!(captured.length, 16.0);
        assert_eq!(captured.notes[0].start, 0.0);
    }

    #[test]
    fn the_pedal_holds_notes_and_keys_down_end_now() {
        let base = Instant::now();
        let mut listener = Listener::default();
        let at = |t: f64| base + Duration::from_secs_f64(t);
        let hear = |l: &mut Listener, t: f64, note: LiveNote| l.hear(HeardNote { at: at(t), note }, stopped(at(t)));
        hear(&mut listener, 0.0, LiveNote::control(Controller::Cc(PEDAL), 0, 1.0));
        hear(&mut listener, 0.0, LiveNote::note(60, 0.9));
        hear(&mut listener, 0.2, LiveNote::note(60, 0.0));
        hear(&mut listener, 1.0, LiveNote::control(Controller::Cc(PEDAL), 0, 0.0));
        hear(&mut listener, 1.5, LiveNote::note(64, 0.5));
        let pending = listener.pending(stopped(at(2.0)));
        assert_eq!(pending.len(), 2);
        assert!((pending[0].end - 1.0).abs() < 1e-6, "held by the pedal until it came up");
        assert!((pending[1].end - 2.0).abs() < 1e-6, "still down: it ends now");
    }

    #[test]
    fn capturing_makes_a_clip_on_the_track_and_undoes() {
        let mut app = PulseApp::headless(crate::project::Project::empty());
        app.commit_history();
        let base = Instant::now();
        let notes: Vec<(u8, f64, f64)> = (0..8).map(|i| (60 + i as u8, i as f64 * 0.25, 0.2)).collect();
        played(&mut app.midi_capture, base, &notes);
        assert!(app.can_capture());
        let empty = app.project.tracks[0].clips.iter().position(Option::is_none).unwrap();
        app.capture_midi();
        let Some(Clip::Midi(clip)) = &app.project.tracks[0].clips[empty] else { panic!("no clip: {:?}", app.status) };
        assert_eq!(clip.notes.len(), 8);
        assert!(!app.can_capture());
        app.commit_history();
        app.undo();
        assert!(app.project.tracks[0].clips[empty].is_none());
    }

    /// On the Arrangement, into an empty project: the tempo played becomes the song's.
    #[test]
    fn a_capture_on_the_arrangement_sets_the_tempo_of_an_empty_song() {
        let mut app = PulseApp::headless(crate::project::Project::empty());
        app.view = View::Arrangement;
        let base = Instant::now();
        let notes: Vec<(u8, f64, f64)> = (0..16).map(|i| (48 + (i % 4) as u8 * 3, i as f64 * 0.3125, 0.25)).collect();
        played(&mut app.midi_capture, base, &notes);
        app.capture_midi();
        assert!((app.project.tempo - 96.0).abs() < 0.5, "{}", app.project.tempo);
        let placed = &app.project.tracks[0].arrangement;
        assert_eq!(placed.len(), 1, "{:?}", app.status);
        assert_eq!((placed[0].start, placed[0].length), (0.0, 8.0));
    }

    #[test]
    fn notes_sent_live_reach_the_listener() {
        let mut app = PulseApp::headless(crate::project::Project::empty());
        let (audio, _engine) = crate::app::Audio::detached();
        app.audio = audio;
        app.listen_midi();
        let live = app.audio.live().unwrap();
        live.send(LiveNote::note(62, 0.8));
        live.send(LiveNote::note(62, 0.0));
        app.listen_midi();
        assert!(app.can_capture());
    }
}
