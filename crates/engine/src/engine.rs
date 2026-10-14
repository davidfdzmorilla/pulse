//! The audio-thread side: transport, clip launching, sequencing and mixing.
//!
//! The UI talks to the engine only through a lock-free command queue, and reads
//! back transport and clip state through atomics in [`SharedState`]. Nothing in
//! [`Engine::process`] allocates or locks.

use crate::model::{InstrumentKind, MidiClip};
use crate::synth::{Click, DrumKit, PolySynth};
use rtrb::{Consumer, Producer};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

pub const MAX_TRACKS: usize = 8;
pub const NUM_SCENES: usize = 8;
const MAX_BLOCK: usize = 1024;
const MAX_EVENTS: usize = 512;

/// `TrackStatus::queued` value meaning "stop at the next quantization boundary".
pub const QUEUED_STOP: i32 = -2;
pub const NONE: i32 = -1;

pub enum Command {
    Play,
    Stop,
    SetTempo(f64),
    SetMetronome(bool),
    /// Launch quantization in beats; 0 launches immediately.
    SetQuantize(f64),
    SetClip {
        track: usize,
        slot: usize,
        clip: Option<Box<MidiClip>>,
    },
    LaunchClip {
        track: usize,
        slot: usize,
    },
    StopTrack(usize),
    LaunchScene(usize),
    StopAllClips,
    SetInstrument {
        track: usize,
        kind: InstrumentKind,
    },
    SetVolume {
        track: usize,
        volume: f32,
    },
    SetPan {
        track: usize,
        pan: f32,
    },
    SetMute {
        track: usize,
        mute: bool,
    },
    SetMasterVolume(f32),
    /// Audition a note on a track, e.g. while editing in the piano roll.
    PreviewNote {
        track: usize,
        pitch: u8,
        velocity: f32,
    },
}

#[derive(Default)]
pub struct TrackStatus {
    pub playing: AtomicI32,
    pub queued: AtomicI32,
    /// Playhead inside the playing clip, in beats (f64 bits).
    clip_position: AtomicU64,
    /// Peak since the UI last read it (f32 bits; non-negative floats order like u32).
    peak: AtomicU32,
}

impl TrackStatus {
    pub fn clip_position(&self) -> f64 {
        f64::from_bits(self.clip_position.load(Ordering::Relaxed))
    }

    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }
}

/// State the audio thread publishes for the UI.
pub struct SharedState {
    pub playing: AtomicBool,
    position: AtomicU64,
    master_peak: AtomicU32,
    pub tracks: [TrackStatus; MAX_TRACKS],
}

impl SharedState {
    fn new() -> Self {
        let tracks = std::array::from_fn(|_| TrackStatus {
            playing: AtomicI32::new(NONE),
            queued: AtomicI32::new(NONE),
            ..Default::default()
        });
        Self {
            playing: AtomicBool::new(false),
            position: AtomicU64::new(0f64.to_bits()),
            master_peak: AtomicU32::new(0),
            tracks,
        }
    }

    /// Transport position in beats.
    pub fn position(&self) -> f64 {
        f64::from_bits(self.position.load(Ordering::Relaxed))
    }

    pub fn take_master_peak(&self) -> f32 {
        f32::from_bits(self.master_peak.swap(0, Ordering::Relaxed))
    }
}

fn publish_peak(slot: &AtomicU32, peak: f32) {
    slot.fetch_max(peak.to_bits(), Ordering::Relaxed);
}

#[derive(Clone, Copy, PartialEq)]
enum Queued {
    Nothing,
    Slot(usize),
    Stop,
}

enum Instrument {
    Synth(PolySynth),
    Drums(DrumKit),
}

impl Instrument {
    fn new(kind: InstrumentKind, sample_rate: f32) -> Self {
        match kind {
            InstrumentKind::Synth(params) => Self::Synth(PolySynth::new(params, sample_rate)),
            InstrumentKind::Drums => Self::Drums(DrumKit::new(sample_rate)),
        }
    }

    fn note_on(&mut self, pitch: u8, velocity: f32, gate_samples: u32) {
        match self {
            Self::Synth(s) => s.note_on(pitch, velocity, gate_samples),
            Self::Drums(d) => d.note_on(pitch, velocity),
        }
    }

    fn all_notes_off(&mut self) {
        if let Self::Synth(s) = self {
            s.all_notes_off();
        }
    }

    fn render(&mut self, out: &mut [f32]) {
        match self {
            Self::Synth(s) => s.render(out),
            Self::Drums(d) => d.render(out),
        }
    }
}

#[derive(Clone, Copy)]
enum Event {
    AllNotesOff,
    NoteOn { pitch: u8, velocity: f32, gate: u32 },
}

struct Track {
    slots: [Option<Box<MidiClip>>; NUM_SCENES],
    playing: Option<usize>,
    launch_beat: f64,
    queued: Queued,
    instrument: Instrument,
    volume: f32,
    pan: f32,
    mute: bool,
    buffer: Vec<f32>,
    /// (frame offset, sort rank, event). Pre-allocated; never grows on the audio thread.
    events: Vec<(usize, u8, Event)>,
}

impl Track {
    fn new(sample_rate: f32) -> Self {
        Self {
            slots: Default::default(),
            playing: None,
            launch_beat: 0.0,
            queued: Queued::Nothing,
            instrument: Instrument::new(
                InstrumentKind::Synth(Default::default()),
                sample_rate,
            ),
            volume: 0.8,
            pan: 0.0,
            mute: false,
            buffer: vec![0.0; MAX_BLOCK],
            events: Vec::with_capacity(MAX_EVENTS),
        }
    }

    fn push_event(&mut self, frame: usize, event: Event) {
        push_event(&mut self.events, frame, event);
    }

    /// Queues note-ons of the playing clip that start in the beat range `[from, to)`.
    fn collect_notes(&mut self, from: f64, to: f64, block_start: f64, beats_per_sample: f64, frames: usize) {
        let Some(clip) = self.playing.and_then(|slot| self.slots[slot].as_ref()) else { return };
        if clip.length <= 0.0 || to <= from {
            return;
        }
        let local_from = from - self.launch_beat;
        let local_to = to - self.launch_beat;
        let first_lap = (local_from / clip.length).floor() as i64;
        let last_lap = (local_to / clip.length).floor() as i64;
        for lap in first_lap..=last_lap {
            let lap_start = lap as f64 * clip.length;
            for note in clip.notes.iter().filter(|note| note.start < clip.length) {
                let t = lap_start + note.start;
                if t < local_from || t >= local_to {
                    continue;
                }
                let beat = self.launch_beat + t;
                let frame = (((beat - block_start) / beats_per_sample) as usize).min(frames - 1);
                let gate = (note.length / beats_per_sample).max(1.0) as u32;
                push_event(
                    &mut self.events,
                    frame,
                    Event::NoteOn {
                        pitch: note.pitch,
                        velocity: note.velocity,
                        gate,
                    },
                );
            }
        }
    }
}

fn push_event(events: &mut Vec<(usize, u8, Event)>, frame: usize, event: Event) {
    if events.len() < MAX_EVENTS {
        let rank = match event {
            Event::AllNotesOff => 0,
            Event::NoteOn { .. } => 1,
        };
        events.push((frame, rank, event));
    }
}

pub struct Engine {
    commands: Consumer<Command>,
    garbage: Producer<Box<MidiClip>>,
    shared: Arc<SharedState>,
    sample_rate: f32,
    tracks: Vec<Track>,
    playing: bool,
    position: f64,
    tempo: f64,
    quantize: f64,
    metronome: bool,
    click: Click,
    master_volume: f32,
    master_left: Vec<f32>,
    master_right: Vec<f32>,
    click_buffer: Vec<f32>,
}

impl Engine {
    pub fn new(
        sample_rate: f32,
        commands: Consumer<Command>,
        garbage: Producer<Box<MidiClip>>,
    ) -> (Self, Arc<SharedState>) {
        let shared = Arc::new(SharedState::new());
        let engine = Self {
            commands,
            garbage,
            shared: shared.clone(),
            sample_rate,
            tracks: (0..MAX_TRACKS).map(|_| Track::new(sample_rate)).collect(),
            playing: false,
            position: 0.0,
            tempo: 120.0,
            quantize: 4.0,
            metronome: false,
            click: Click::new(sample_rate),
            master_volume: 0.8,
            master_left: vec![0.0; MAX_BLOCK],
            master_right: vec![0.0; MAX_BLOCK],
            click_buffer: vec![0.0; MAX_BLOCK],
        };
        (engine, shared)
    }

    /// Hands a replaced clip back to the UI thread so it is freed there.
    fn discard(&mut self, clip: Option<Box<MidiClip>>) {
        if let Some(clip) = clip {
            // If the queue is full we have to free it here; it is rare and only costs a free().
            let _ = self.garbage.push(clip);
        }
    }

    fn start_transport(&mut self) {
        if !self.playing {
            self.playing = true;
            self.position = 0.0;
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Play => self.start_transport(),
            Command::Stop => {
                self.playing = false;
                self.position = 0.0;
                for track in &mut self.tracks {
                    track.playing = None;
                    track.queued = Queued::Nothing;
                    track.instrument.all_notes_off();
                }
            }
            Command::SetTempo(bpm) => self.tempo = bpm.clamp(20.0, 999.0),
            Command::SetMetronome(on) => self.metronome = on,
            Command::SetQuantize(beats) => self.quantize = beats.max(0.0),
            Command::SetClip { track, slot, clip } => {
                let Some(t) = self.tracks.get_mut(track) else { return };
                if slot >= NUM_SCENES {
                    return;
                }
                let removed = clip.is_none();
                let old = std::mem::replace(&mut t.slots[slot], clip);
                if removed && t.playing == Some(slot) {
                    t.playing = None;
                    t.instrument.all_notes_off();
                }
                if removed && t.queued == Queued::Slot(slot) {
                    t.queued = Queued::Nothing;
                }
                self.discard(old);
            }
            Command::LaunchClip { track, slot } => {
                let Some(t) = self.tracks.get_mut(track) else { return };
                if slot >= NUM_SCENES {
                    return;
                }
                // Launching an empty slot acts as that track's stop button.
                t.queued = if t.slots[slot].is_some() {
                    Queued::Slot(slot)
                } else {
                    Queued::Stop
                };
                if t.queued != Queued::Stop {
                    self.start_transport();
                }
            }
            Command::StopTrack(track) => {
                if let Some(t) = self.tracks.get_mut(track) {
                    t.queued = Queued::Stop;
                }
            }
            Command::LaunchScene(slot) => {
                if slot >= NUM_SCENES {
                    return;
                }
                for t in &mut self.tracks {
                    t.queued = if t.slots[slot].is_some() {
                        Queued::Slot(slot)
                    } else {
                        Queued::Stop
                    };
                }
                self.start_transport();
            }
            Command::StopAllClips => {
                for t in &mut self.tracks {
                    t.queued = Queued::Stop;
                }
            }
            Command::SetInstrument { track, kind } => {
                let Some(t) = self.tracks.get_mut(track) else { return };
                match (&mut t.instrument, kind) {
                    // Keep sounding voices when only synth parameters change.
                    (Instrument::Synth(synth), InstrumentKind::Synth(params)) => synth.set_params(params),
                    (Instrument::Drums(_), InstrumentKind::Drums) => {}
                    _ => t.instrument = Instrument::new(kind, self.sample_rate),
                }
            }
            Command::SetVolume { track, volume } => {
                if let Some(t) = self.tracks.get_mut(track) {
                    t.volume = volume.max(0.0);
                }
            }
            Command::SetPan { track, pan } => {
                if let Some(t) = self.tracks.get_mut(track) {
                    t.pan = pan.clamp(-1.0, 1.0);
                }
            }
            Command::SetMute { track, mute } => {
                if let Some(t) = self.tracks.get_mut(track) {
                    t.mute = mute;
                }
            }
            Command::SetMasterVolume(volume) => self.master_volume = volume.max(0.0),
            Command::PreviewNote {
                track,
                pitch,
                velocity,
            } => {
                let gate = (0.3 * self.sample_rate) as u32;
                if let Some(t) = self.tracks.get_mut(track) {
                    t.push_event(0, Event::NoteOn { pitch, velocity, gate });
                }
            }
        }
    }

    /// Renders `frames` interleaved frames into `out`.
    pub fn process(&mut self, out: &mut [f32], channels: usize) {
        for t in &mut self.tracks {
            t.events.clear();
        }
        while let Ok(command) = self.commands.pop() {
            self.handle(command);
        }

        let frames = out.len() / channels;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(MAX_BLOCK);
            self.render_block(n);
            for i in 0..n {
                let frame = &mut out[(done + i) * channels..(done + i + 1) * channels];
                let (l, r) = (self.master_left[i], self.master_right[i]);
                match channels {
                    1 => frame[0] = 0.5 * (l + r),
                    _ => {
                        frame[0] = l;
                        frame[1] = r;
                        for extra in &mut frame[2..] {
                            *extra = 0.0;
                        }
                    }
                }
            }
            done += n;
        }
        self.publish();
    }

    fn render_block(&mut self, n: usize) {
        if self.playing {
            self.sequence(n);
        }

        self.master_left[..n].fill(0.0);
        self.master_right[..n].fill(0.0);
        for (index, track) in self.tracks.iter_mut().enumerate() {
            let buffer = &mut track.buffer[..n];
            buffer.fill(0.0);
            track.events.sort_unstable_by_key(|&(frame, rank, _)| (frame, rank));
            let mut cursor = 0;
            for &(frame, _, event) in &track.events {
                let frame = frame.min(n - 1);
                if frame > cursor {
                    track.instrument.render(&mut buffer[cursor..frame]);
                    cursor = frame;
                }
                match event {
                    Event::AllNotesOff => track.instrument.all_notes_off(),
                    Event::NoteOn { pitch, velocity, gate } => {
                        track.instrument.note_on(pitch, velocity, gate)
                    }
                }
            }
            track.instrument.render(&mut buffer[cursor..]);
            track.events.clear();

            let gain = if track.mute { 0.0 } else { track.volume };
            // Equal-power pan.
            let angle = (track.pan + 1.0) * std::f32::consts::FRAC_PI_4;
            let (left_gain, right_gain) = (angle.cos() * gain, angle.sin() * gain);
            let mut peak = 0f32;
            for i in 0..n {
                let s = buffer[i];
                peak = peak.max((s * gain).abs());
                self.master_left[i] += s * left_gain;
                self.master_right[i] += s * right_gain;
            }
            publish_peak(&self.shared.tracks[index].peak, peak);
        }

        let click = &mut self.click_buffer[..n];
        click.fill(0.0);
        self.click.render(click);

        let mut peak = 0f32;
        for i in 0..n {
            // Gentle saturation keeps an overloaded mix from hard clipping.
            let l = ((self.master_left[i] + click[i]) * self.master_volume).tanh();
            let r = ((self.master_right[i] + click[i]) * self.master_volume).tanh();
            peak = peak.max(l.abs()).max(r.abs());
            self.master_left[i] = l;
            self.master_right[i] = r;
        }
        publish_peak(&self.shared.master_peak, peak);
    }

    /// Advances the transport over `n` frames, applying quantized launches and
    /// scheduling the notes that fall inside the block.
    fn sequence(&mut self, n: usize) {
        let beats_per_sample = self.tempo / 60.0 / self.sample_rate as f64;
        let start = self.position;
        let end = start + n as f64 * beats_per_sample;
        let frame_of = |beat: f64| (((beat - start) / beats_per_sample) as usize).min(n - 1);

        let any_queued = self.tracks.iter().any(|t| t.queued != Queued::Nothing);
        let boundary = if self.quantize <= 0.0 {
            start
        } else {
            // Small epsilon so a launch exactly on a boundary is not pushed a bar later.
            ((start - 1e-9) / self.quantize).ceil() * self.quantize
        };

        if any_queued && boundary < end {
            let frame = frame_of(boundary);
            for track in &mut self.tracks {
                track.collect_notes(start, boundary, start, beats_per_sample, n);
                match track.queued {
                    Queued::Nothing => {}
                    Queued::Stop => {
                        track.playing = None;
                        track.push_event(frame, Event::AllNotesOff);
                    }
                    Queued::Slot(slot) => {
                        track.playing = Some(slot);
                        track.launch_beat = boundary;
                        track.push_event(frame, Event::AllNotesOff);
                    }
                }
                track.queued = Queued::Nothing;
                track.collect_notes(boundary, end, start, beats_per_sample, n);
            }
        } else {
            for track in &mut self.tracks {
                track.collect_notes(start, end, start, beats_per_sample, n);
            }
        }

        if self.metronome {
            let first_beat = (start - 1e-9).ceil();
            if first_beat < end {
                self.click
                    .trigger((first_beat as i64).rem_euclid(4) == 0, frame_of(first_beat));
            }
        }

        self.position = end;
    }

    fn publish(&self) {
        let s = &self.shared;
        s.playing.store(self.playing, Ordering::Relaxed);
        s.position.store(self.position.to_bits(), Ordering::Relaxed);
        for (track, status) in self.tracks.iter().zip(&s.tracks) {
            let playing = track.playing.map_or(NONE, |p| p as i32);
            let queued = match track.queued {
                Queued::Nothing => NONE,
                Queued::Stop => QUEUED_STOP,
                Queued::Slot(slot) => slot as i32,
            };
            status.playing.store(playing, Ordering::Relaxed);
            status.queued.store(queued, Ordering::Relaxed);
            let clip_position = match track.playing.and_then(|p| track.slots[p].as_ref()) {
                Some(clip) if clip.length > 0.0 => {
                    (self.position - track.launch_beat).rem_euclid(clip.length)
                }
                _ => 0.0,
            };
            status.clip_position.store(clip_position.to_bits(), Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{InstrumentKind, MidiClip, Note};
    use rtrb::RingBuffer;

    const SR: f32 = 48_000.0;

    struct Harness {
        engine: Engine,
        commands: Producer<Command>,
        shared: Arc<SharedState>,
        _garbage: Consumer<Box<MidiClip>>,
    }

    impl Harness {
        fn new() -> Self {
            let (commands, rx) = RingBuffer::new(64);
            let (gtx, grx) = RingBuffer::new(64);
            let (engine, shared) = Engine::new(SR, rx, gtx);
            Self { engine, commands, shared, _garbage: grx }
        }

        fn send(&mut self, c: Command) {
            assert!(self.commands.push(c).is_ok());
        }

        /// Renders `beats` worth of stereo audio at 120 BPM and returns it.
        fn run_beats(&mut self, beats: f64) -> Vec<f32> {
            let frames = (beats * 0.5 * SR as f64) as usize;
            let mut out = vec![0.0; frames * 2];
            for chunk in out.chunks_mut(512 * 2) {
                self.engine.process(chunk, 2);
            }
            out
        }
    }

    fn one_note_clip() -> Box<MidiClip> {
        let mut clip = MidiClip::new("test", 4.0, [0, 0, 0]);
        clip.notes.push(Note { start: 0.0, length: 1.0, pitch: 60, velocity: 1.0 });
        Box::new(clip)
    }

    fn energy(samples: &[f32]) -> f32 {
        samples.iter().map(|s| s * s).sum()
    }

    #[test]
    fn launching_a_clip_starts_the_transport_and_plays_notes() {
        let mut h = Harness::new();
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        let audio = h.run_beats(1.0);
        assert!(h.shared.playing.load(Ordering::Relaxed));
        assert_eq!(h.shared.tracks[0].playing.load(Ordering::Relaxed), 0);
        assert!(energy(&audio) > 1.0, "clip should be audible");
    }

    #[test]
    fn launches_wait_for_the_next_bar() {
        let mut h = Harness::new();
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::SetClip { track: 0, slot: 1, clip: Some(one_note_clip()) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        h.run_beats(1.0);
        h.send(Command::LaunchClip { track: 0, slot: 1 });
        h.run_beats(2.0); // now at beat 3: still in the first bar
        assert_eq!(h.shared.tracks[0].playing.load(Ordering::Relaxed), 0);
        assert_eq!(h.shared.tracks[0].queued.load(Ordering::Relaxed), 1);
        h.run_beats(1.5); // past beat 4
        assert_eq!(h.shared.tracks[0].playing.load(Ordering::Relaxed), 1);
        assert_eq!(h.shared.tracks[0].queued.load(Ordering::Relaxed), NONE);
    }

    #[test]
    fn scene_launch_stops_tracks_with_empty_slots() {
        let mut h = Harness::new();
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::SetClip { track: 1, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::SetClip { track: 0, slot: 1, clip: Some(one_note_clip()) });
        h.send(Command::LaunchScene(0));
        h.run_beats(1.0);
        h.send(Command::LaunchScene(1));
        h.run_beats(4.0);
        assert_eq!(h.shared.tracks[0].playing.load(Ordering::Relaxed), 1);
        assert_eq!(h.shared.tracks[1].playing.load(Ordering::Relaxed), NONE);
    }

    #[test]
    fn drums_and_clip_loop_keep_playing() {
        let mut h = Harness::new();
        h.send(Command::SetInstrument { track: 0, kind: InstrumentKind::Drums });
        let mut clip = MidiClip::new("beat", 1.0, [0, 0, 0]);
        clip.notes.push(Note { start: 0.0, length: 0.25, pitch: 36, velocity: 1.0 });
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(Box::new(clip)) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        h.run_beats(8.0);
        // Fifth lap of a one-beat loop: the kick must sound again.
        let late = h.run_beats(1.0);
        assert!(energy(&late) > 1.0);
        let pos = h.shared.tracks[0].clip_position();
        assert!((0.0..1.0).contains(&pos));
    }

    #[test]
    fn stop_silences_and_resets() {
        let mut h = Harness::new();
        h.send(Command::SetClip { track: 0, slot: 0, clip: Some(one_note_clip()) });
        h.send(Command::LaunchClip { track: 0, slot: 0 });
        h.run_beats(0.5);
        h.send(Command::Stop);
        h.run_beats(1.0); // let the release tail finish
        let tail = h.run_beats(0.5);
        assert!(!h.shared.playing.load(Ordering::Relaxed));
        assert_eq!(h.shared.position(), 0.0);
        assert!(energy(&tail) < 1e-6);
    }
}
