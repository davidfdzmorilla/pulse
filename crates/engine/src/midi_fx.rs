//! MIDI effects: what a MIDI track's notes go through before its instrument.
//!
//! - Arpeggiator: the held notes one at a time, in time with the transport (or
//!   its own clock while stopped), over some octaves.
//! - Chord: each note becomes a chord: its third and fifth (and seventh) in the
//!   key's scale, so the chords fit the song, or fixed intervals.
//! - Scale: each note goes to the scale's nearest.
//!
//! Notes come in as note-ons held until their note-off (live input) or with a
//! length of their own (clips). Everything is kept in fixed arrays: nothing
//! allocates on the audio thread.

use crate::engine::{push_event, Event, EventQueue};
use crate::model::{ArpMode, ArpeggiatorParams, ChordParams, MidiDevice, MidiEffect, ScaleKey, ScaleParams, MAX_MIDI_EFFECTS};
use crate::synth::HELD;

/// The notes one input note became (for its note-off), at most four.
#[derive(Clone, Copy)]
struct Mapped {
    pitches: [u8; 4],
    count: u8,
}

struct MapState {
    held: [Mapped; 128],
}

impl MapState {
    fn new() -> Self {
        Self { held: [Mapped { pitches: [0; 4], count: 0 }; 128] }
    }
}

/// Keys the arpeggiator holds: pitch, velocity, and the sample it ends at (for
/// notes with a length; `u64::MAX` while held).
const MAX_KEYS: usize = 32;

struct ArpState {
    keys: [(u8, f32, u64); MAX_KEYS],
    count: usize,
    step: usize,
    /// Samples processed so far (to end notes with a length).
    clock: u64,
    /// While the transport is stopped: samples until the next step.
    countdown: f64,
    seed: u32,
}

impl ArpState {
    fn new() -> Self {
        Self { keys: [(0, 0.0, 0); MAX_KEYS], count: 0, step: 0, clock: 0, countdown: 0.0, seed: 0x2545_F491 }
    }

    fn press(&mut self, pitch: u8, velocity: f32, ends: u64) {
        if let Some(k) = self.keys[..self.count].iter_mut().find(|k| k.0 == pitch) {
            *k = (pitch, velocity, ends);
            return;
        }
        if self.count < MAX_KEYS {
            if self.count == 0 {
                self.step = 0;
                self.countdown = 0.0;
            }
            self.keys[self.count] = (pitch, velocity, ends);
            self.count += 1;
        }
    }

    fn release(&mut self, pitch: u8) {
        if let Some(at) = self.keys[..self.count].iter().position(|k| k.0 == pitch) {
            self.keys.copy_within(at + 1..self.count, at);
            self.count -= 1;
        }
    }

    /// Lets go of the notes whose length ended by sample `now`.
    fn expire(&mut self, now: u64) {
        let mut i = 0;
        while i < self.count {
            if self.keys[i].2 <= now {
                self.keys.copy_within(i + 1..self.count, i);
                self.count -= 1;
            } else {
                i += 1;
            }
        }
    }

    /// The next note of the pattern: pitch and velocity.
    fn next(&mut self, p: &ArpeggiatorParams) -> Option<(u8, f32)> {
        if self.count == 0 {
            return None;
        }
        // The held notes low to high (or as played).
        let mut order = [0usize; MAX_KEYS];
        for (i, o) in order.iter_mut().enumerate().take(self.count) {
            *o = i;
        }
        if p.mode != ArpMode::Played {
            order[..self.count].sort_unstable_by_key(|&i| self.keys[i].0);
        }
        let octaves = p.octaves.clamp(1, 4) as usize;
        let length = self.count * octaves;
        // A position in the rising run of `length` notes.
        let rising = |k: usize| (order[k % self.count], k / self.count);
        let (key, octave) = match p.mode {
            ArpMode::Up | ArpMode::Played => rising(self.step % length),
            ArpMode::Down => rising(length - 1 - self.step % length),
            ArpMode::UpDown => {
                let cycle = if length > 1 { 2 * length - 2 } else { 1 };
                let k = self.step % cycle;
                rising(if k < length { k } else { cycle - k })
            }
            ArpMode::Random => {
                self.seed ^= self.seed << 13;
                self.seed ^= self.seed >> 17;
                self.seed ^= self.seed << 5;
                rising(self.seed as usize % length)
            }
        };
        self.step += 1;
        let (pitch, velocity, _) = self.keys[key];
        Some(((pitch as usize + 12 * octave).min(127) as u8, velocity))
    }
}

enum State {
    Map(MapState),
    Arp(ArpState),
}

struct Slot {
    device: MidiDevice,
    state: State,
}

/// What the effects need to know about this block.
#[derive(Clone, Copy)]
pub(crate) struct Timing {
    pub sample_rate: f32,
    pub tempo: f64,
    /// Transport beat at the block's first sample, and beats a sample (0: stopped).
    pub beat: f64,
    pub beats_per_sample: f64,
}

/// A track's MIDI effects, in order.
pub(crate) struct MidiChain {
    slots: [Option<Slot>; MAX_MIDI_EFFECTS],
    scratch: EventQueue,
}

impl MidiChain {
    /// Allocates its state; call off the audio thread (made with the track).
    pub fn new(capacity: usize) -> Self {
        Self { slots: Default::default(), scratch: Vec::with_capacity(capacity) }
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.as_ref().is_none_or(|s| !s.device.enabled))
    }

    /// New settings. An effect of the same kind in a place keeps its state (the
    /// notes it holds); another kind starts afresh (in place: nothing allocates).
    pub fn set(&mut self, devices: [Option<MidiDevice>; MAX_MIDI_EFFECTS]) {
        for (slot, device) in self.slots.iter_mut().zip(devices) {
            match (slot.as_mut(), device) {
                (Some(s), Some(d)) if std::mem::discriminant(&s.device.effect) == std::mem::discriminant(&d.effect) => s.device = d,
                (_, Some(d)) => {
                    let state = match d.effect {
                        MidiEffect::Arpeggiator(_) => State::Arp(ArpState::new()),
                        _ => State::Map(MapState::new()),
                    };
                    *slot = Some(Slot { device: d, state });
                }
                (_, None) => *slot = None,
            }
        }
    }

    /// Runs the block's events (sorted by frame) through the effects.
    pub fn process(&mut self, events: &mut EventQueue, n: usize, timing: Timing) {
        for slot in self.slots.iter_mut().flatten() {
            if !slot.device.enabled {
                continue;
            }
            self.scratch.clear();
            match (&mut slot.state, slot.device.effect) {
                (State::Map(state), MidiEffect::Chord(p)) => map(events, &mut self.scratch, state, |pitch, out| chord(&p, pitch, out), p.velocity),
                (State::Map(state), MidiEffect::Scale(p)) => map(events, &mut self.scratch, state, |pitch, out| scale(&p, pitch, out), 1.0),
                (State::Arp(state), MidiEffect::Arpeggiator(p)) => arpeggiate(events, &mut self.scratch, state, &p, n, timing),
                _ => continue,
            }
            std::mem::swap(events, &mut self.scratch);
            events.sort_unstable_by_key(|&(frame, rank, _)| (frame, rank));
        }
    }
}

/// The notes a chord effect makes of `pitch`, the played one first.
fn chord(p: &ChordParams, pitch: u8, out: &mut [u8; 4]) -> u8 {
    out[0] = pitch;
    let mut count = 1;
    match p.key.filter(|_| p.in_key) {
        Some(key) => {
            for steps in [2, 4].into_iter().chain(p.seventh.then_some(6)) {
                out[count] = key.up(pitch, steps);
                count += 1;
            }
        }
        None => {
            for &iv in &p.intervals {
                let q = pitch as i32 + iv as i32;
                if iv != 0 && (0..=127).contains(&q) {
                    out[count] = q as u8;
                    count += 1;
                }
            }
        }
    }
    count as u8
}

fn scale(p: &ScaleParams, pitch: u8, out: &mut [u8; 4]) -> u8 {
    out[0] = p.key.map_or(pitch, |k: ScaleKey| k.snap(pitch));
    1
}

/// Note for note effects: each note-on becomes the notes `make` says (the added
/// ones `added` as loud), and its note-off ends them all.
fn map(events: &EventQueue, out: &mut EventQueue, state: &mut MapState, make: impl Fn(u8, &mut [u8; 4]) -> u8, added: f32) {
    for &(frame, _, event) in events.iter() {
        match event {
            Event::NoteOn { pitch, velocity, gate } => {
                let mut pitches = [0u8; 4];
                let count = make(pitch, &mut pitches);
                for (k, &q) in pitches[..count as usize].iter().enumerate() {
                    let v = if k == 0 { velocity } else { velocity * added };
                    push_event(out, frame, Event::NoteOn { pitch: q, velocity: v, gate });
                }
                if gate == HELD {
                    state.held[pitch as usize & 127] = Mapped { pitches, count };
                }
            }
            Event::NoteOff { pitch } => {
                let mapped = std::mem::replace(&mut state.held[pitch as usize & 127], Mapped { pitches: [0; 4], count: 0 });
                if mapped.count == 0 {
                    push_event(out, frame, Event::NoteOff { pitch });
                }
                for &q in &mapped.pitches[..mapped.count as usize] {
                    push_event(out, frame, Event::NoteOff { pitch: q });
                }
            }
            Event::AllNotesOff => {
                for m in state.held.iter_mut() {
                    m.count = 0;
                }
                push_event(out, frame, event);
            }
            Event::Control { .. } => push_event(out, frame, event),
        }
    }
}

fn arpeggiate(events: &EventQueue, out: &mut EventQueue, state: &mut ArpState, p: &ArpeggiatorParams, n: usize, t: Timing) {
    let rate = p.rate.max(1.0 / 32.0);
    let playing = t.beats_per_sample > 0.0;
    let bps = if playing { t.beats_per_sample } else { t.tempo.max(1.0) / 60.0 / t.sample_rate as f64 };
    let step_samples = rate / bps;
    let gate = ((step_samples * p.gate.clamp(0.05, 1.0) as f64) as u32).max(1);
    // Playing, the steps fall on the transport's grid: the next one from here.
    let mut grid = (t.beat / rate - 1e-9).ceil();
    let grid_frame = |k: f64| ((k * rate - t.beat) / bps).round().max(0.0) as usize;
    // Where the block has got to (for the clock of its own, while stopped).
    let mut cursor = 0usize;
    let mut next_event = 0;
    loop {
        let step = if playing {
            Some(grid_frame(grid)).filter(|&f| f < n)
        } else if state.count > 0 {
            Some(cursor + state.countdown.max(0.0).round() as usize).filter(|&f| f < n)
        } else {
            None
        };
        let event = events.get(next_event).copied();
        match (event, step) {
            (Some((f, _, event)), s) if s.is_none_or(|s| f <= s) => {
                next_event += 1;
                if !playing && state.count > 0 {
                    state.countdown -= (f - cursor.min(f)) as f64;
                }
                cursor = cursor.max(f);
                match event {
                    Event::NoteOn { pitch, velocity, gate: length } => {
                        let ends = if length == HELD { u64::MAX } else { state.clock + f as u64 + length as u64 };
                        // (The first note restarts the pattern; stopped, on the spot.)
                        state.press(pitch, velocity, ends);
                    }
                    Event::NoteOff { pitch } => state.release(pitch),
                    Event::AllNotesOff => {
                        state.count = 0;
                        push_event(out, f, event);
                    }
                    Event::Control { .. } => push_event(out, f, event),
                }
            }
            (_, Some(s)) => {
                state.expire(state.clock + s as u64);
                // Playing, a random step's luck is its place on the grid: the
                // song plays the same notes every time, live or exported.
                if playing {
                    state.seed = (grid as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_right(29) as u32 | 1;
                }
                if let Some((pitch, velocity)) = state.next(p) {
                    push_event(out, s, Event::NoteOn { pitch, velocity, gate });
                }
                if playing {
                    grid += 1.0;
                } else {
                    cursor = s;
                    state.countdown = step_samples;
                }
            }
            _ => {
                if !playing && state.count > 0 {
                    state.countdown -= (n - cursor) as f64;
                }
                break;
            }
        }
    }
    state.clock += n as u64;
    state.expire(state.clock);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn note_ons(events: &EventQueue) -> Vec<(usize, u8)> {
        events.iter().filter_map(|&(f, _, e)| if let Event::NoteOn { pitch, .. } = e { Some((f, pitch)) } else { None }).collect()
    }

    fn chain(effect: MidiEffect) -> MidiChain {
        let mut c = MidiChain::new(256);
        let mut devices = [None; MAX_MIDI_EFFECTS];
        devices[0] = Some(MidiDevice::new(effect));
        c.set(devices);
        c
    }

    fn queue(items: &[(usize, Event)]) -> EventQueue {
        let mut q = Vec::with_capacity(256);
        for &(f, e) in items {
            push_event(&mut q, f, e);
        }
        q
    }

    #[test]
    fn chords_in_the_key_and_off_together() {
        let c_major = ScaleKey { tonic: 0, minor: false };
        let mut fx = chain(MidiEffect::Chord(ChordParams { key: Some(c_major), ..Default::default() }));
        let t = Timing { sample_rate: SR, tempo: 120.0, beat: 0.0, beats_per_sample: 0.0 };
        // D: D F A (a minor chord, as D minor is C major's); held, then let go.
        let mut events = queue(&[(10, Event::NoteOn { pitch: 62, velocity: 1.0, gate: HELD })]);
        fx.process(&mut events, 512, t);
        let mut on: Vec<u8> = note_ons(&events).iter().map(|n| n.1).collect();
        on.sort();
        assert_eq!(on, vec![62, 65, 69]);
        let mut events = queue(&[(5, Event::NoteOff { pitch: 62 })]);
        fx.process(&mut events, 512, t);
        assert_eq!(events.len(), 3, "all three end");
        // Fixed intervals without the key: a major chord on anything.
        let mut fx = chain(MidiEffect::Chord(ChordParams { in_key: false, ..Default::default() }));
        let mut events = queue(&[(0, Event::NoteOn { pitch: 62, velocity: 1.0, gate: 1000 })]);
        fx.process(&mut events, 512, t);
        let mut on: Vec<u8> = note_ons(&events).iter().map(|n| n.1).collect();
        on.sort();
        assert_eq!(on, vec![62, 66, 69]);
    }

    #[test]
    fn notes_go_to_the_scale() {
        let a_minor = ScaleKey { tonic: 9, minor: true };
        let mut fx = chain(MidiEffect::Scale(ScaleParams { key: Some(a_minor), follow_project: false }));
        let t = Timing { sample_rate: SR, tempo: 120.0, beat: 0.0, beats_per_sample: 0.0 };
        let mut events = queue(&[(0, Event::NoteOn { pitch: 61, velocity: 1.0, gate: HELD }), (0, Event::NoteOn { pitch: 64, velocity: 1.0, gate: HELD })]);
        fx.process(&mut events, 512, t);
        assert_eq!(note_ons(&events).iter().map(|n| n.1).collect::<Vec<_>>(), vec![60, 64], "C# to C, E stays");
        let mut events = queue(&[(0, Event::NoteOff { pitch: 61 })]);
        fx.process(&mut events, 512, t);
        assert!(matches!(events[0].2, Event::NoteOff { pitch: 60 }), "the note that sounded ends");
        // Without a key, as they come.
        let mut fx = chain(MidiEffect::Scale(ScaleParams::default()));
        let mut events = queue(&[(0, Event::NoteOn { pitch: 61, velocity: 1.0, gate: HELD })]);
        fx.process(&mut events, 512, t);
        assert_eq!(note_ons(&events)[0].1, 61);
    }

    /// Runs blocks of 512 at 120 BPM from beat 0, playing; returns the notes and
    /// their sample positions.
    fn run(fx: &mut MidiChain, blocks: usize, input: &[(usize, Event)], playing: bool) -> Vec<(usize, u8)> {
        let bps = 2.0 / SR as f64;
        let mut out = Vec::new();
        for b in 0..blocks {
            let at = b * 512;
            let mine: Vec<(usize, Event)> = input.iter().filter(|(f, _)| *f >= at && *f < at + 512).map(|&(f, e)| (f - at, e)).collect();
            let mut events = queue(&mine);
            let t = Timing { sample_rate: SR, tempo: 120.0, beat: if playing { at as f64 * bps } else { 0.0 }, beats_per_sample: if playing { bps } else { 0.0 } };
            fx.process(&mut events, 512, t);
            out.extend(note_ons(&events).into_iter().map(|(f, p)| (f + at, p)));
        }
        out
    }

    #[test]
    fn the_arpeggiator_plays_held_notes_in_time() {
        // C E G held from the start, sixteenths at 120 BPM: a note every 6000 samples.
        let held = [(0, Event::NoteOn { pitch: 60, velocity: 1.0, gate: HELD }), (0, Event::NoteOn { pitch: 67, velocity: 1.0, gate: HELD }), (0, Event::NoteOn { pitch: 64, velocity: 1.0, gate: HELD })];
        let mut fx = chain(MidiEffect::Arpeggiator(Default::default()));
        let notes = run(&mut fx, 60, &held, true);
        assert_eq!(notes.iter().take(5).map(|n| n.1).collect::<Vec<_>>(), vec![60, 64, 67, 60, 64]);
        assert_eq!(notes.iter().take(4).map(|n| n.0).collect::<Vec<_>>(), vec![0, 6000, 12000, 18000]);
        // Down over two octaves, and up and down.
        let mut fx = chain(MidiEffect::Arpeggiator(ArpeggiatorParams { mode: ArpMode::Down, octaves: 2, ..Default::default() }));
        assert_eq!(run(&mut fx, 60, &held, true).iter().take(4).map(|n| n.1).collect::<Vec<_>>(), vec![79, 76, 72, 67]);
        let mut fx = chain(MidiEffect::Arpeggiator(ArpeggiatorParams { mode: ArpMode::UpDown, ..Default::default() }));
        assert_eq!(run(&mut fx, 60, &held, true).iter().take(6).map(|n| n.1).collect::<Vec<_>>(), vec![60, 64, 67, 64, 60, 64]);
    }

    #[test]
    fn the_arpeggiator_stops_when_the_notes_end_and_runs_on_its_own_clock() {
        // A clip's chord a beat long (24000 samples): four sixteenths, no more.
        let chord = [(0, Event::NoteOn { pitch: 60, velocity: 1.0, gate: 24_000 }), (0, Event::NoteOn { pitch: 64, velocity: 1.0, gate: 24_000 })];
        let mut fx = chain(MidiEffect::Arpeggiator(Default::default()));
        assert_eq!(run(&mut fx, 100, &chord, true).len(), 4);
        // Stopped: it starts with the first key and goes on at the tempo.
        let keys = [(1000, Event::NoteOn { pitch: 60, velocity: 1.0, gate: HELD }), (1000, Event::NoteOn { pitch: 64, velocity: 1.0, gate: HELD }), (30_000, Event::NoteOff { pitch: 60 }), (30_000, Event::NoteOff { pitch: 64 })];
        let mut fx = chain(MidiEffect::Arpeggiator(Default::default()));
        let notes = run(&mut fx, 100, &keys, false);
        assert_eq!(notes.iter().map(|n| n.0).collect::<Vec<_>>(), vec![1000, 7000, 13000, 19000, 25000], "{notes:?}");
    }
}
