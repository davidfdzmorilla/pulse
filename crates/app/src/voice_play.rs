//! Playing with the voice: sing, hum or whistle into the microphone and the
//! selected (or armed) track plays it, a note for each note sung, as from a
//! MIDI keyboard (so it records too). The pitch is followed as it comes in
//! (`stems::pitch::LiveTracker`), every few milliseconds, on a thread of its
//! own (not at the screen's pace); a note holds while the voice wavers around
//! it and changes once the voice settles on another; with a project key, a
//! note sung off the scale goes to the scale's nearest. Following the pitch
//! («Seguir el tono»), what the voice strays from the note (vibrato, a slide
//! to the next) bends it too. With chords, the note sung is the root of its
//! chord in the project's scale (a triad or a seventh chord). As drums, the
//! voice beatboxes instead: «pum», «pf» and «ts» play the kick, the snare and
//! the hat (see `beatbox`).

use crate::app::PulseApp;
use crate::beatbox::{Beatbox, Drum, Examples, Features, TEACH};
use engine::{Controller, LiveInput, LiveNote, VoiceInput};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use stems::key::Key;
use stems::pitch::LiveTracker;

/// Input analysed each time: 32 ms, two periods of the lowest voice (each
/// of its newest 16.6 ms compared with the one a period before).
const WINDOW_SECONDS: f32 = 0.032;
/// Input between analyses: 128 samples at 48 kHz.
const HOP_SECONDS: f32 = 128.0 / 48_000.0;
/// The lowest and highest pitch followed (Hz): a bass voice to a whistle.
const FMIN: f64 = 65.0;
const FMAX: f64 = 1_600.0;
/// How unsure a pitch may be and still count (YIN's aperiodicity).
const UNSURE: f64 = 0.2;
/// Below this (dBFS), silence.
const GATE_DB: f64 = -48.0;
/// How far (semitones) the voice may stray from the note playing and keep it.
const STRAY: f64 = 0.75;
/// Seconds a note is heard from silence before it plays (the first moments
/// of a note are often off, even by an octave).
const AGREE: f64 = 0.012;
/// Seconds on another note before it changes (an octave or more away, more:
/// pitch trackers slip by octaves), and without a voice before the note stops.
const SETTLE: f64 = 0.03;
const SETTLE_LEAP: f64 = 0.06;
const RELEASE: f64 = 0.03;
/// A jump in level (dB) within `ATTACK_SPAN` seconds that is a new attack on
/// the same note.
const ATTACK_DB: f64 = 9.0;
const ATTACK_SPAN: f64 = 0.016;
/// Within this of a semitone (semitones), a note is sung in tune and stays,
/// even off the scale.
const IN_TUNE: f64 = 0.3;
/// The pitch bend's range (semitones) in the built-in instruments (and most
/// synths): the bend sent is the voice's distance from the note over this.
const BEND_RANGE: f64 = 2.0;
/// How fast the bend follows the voice (seconds; a vibrato of 5–7 Hz goes
/// through, the tracker's jitter does not).
const BEND_SMOOTH: f64 = 0.008;
/// A change in the bend smaller than this (semitones) is not sent.
const BEND_STEP: f64 = 0.01;

/// Turns what is heard, analysis after analysis, into notes on and off.
#[derive(Default)]
pub struct Follower {
    pub playing: Option<u8>,
    /// Another note heard, and for how long (seconds).
    candidate: Option<(u8, f64)>,
    /// How long without a voice (seconds), if it is quiet.
    quiet: Option<f64>,
    /// The time (seconds since it started) and the levels heard lately.
    clock: f64,
    levels: VecDeque<(f64, f64)>,
    /// Where the voice is in tune with the note playing (fractional MIDI):
    /// the note, or the pitch it was sung at if the scale moved it.
    reference: f64,
    /// How far the voice is from `reference` (semitones, smoothed).
    pub bend: f64,
}

impl Follower {
    /// What one analysis heard, `dt` seconds after the last: the pitch
    /// (fractional MIDI) if there is a voice, and how loud (dBFS). Returns the
    /// notes to send: (pitch, velocity), a velocity of 0 being a note off.
    pub fn step(&mut self, pitch: Option<f64>, level_db: f64, key: Option<Key>, dt: f64) -> Vec<(u8, f32)> {
        self.clock += dt;
        let rose = self.rose(level_db);
        let mut out = Vec::new();
        let voiced = pitch.filter(|_| level_db > GATE_DB);
        let Some(m) = voiced else {
            let quiet = self.quiet.map_or(0.0, |q| q + dt);
            self.quiet = Some(quiet);
            self.candidate = None;
            if quiet >= RELEASE - 1e-9 {
                if let Some(p) = self.playing.take() {
                    out.push((p, 0.0));
                }
            }
            return out;
        };
        self.quiet = None;
        let velocity = ((level_db - GATE_DB) / 36.0).clamp(0.25, 1.0) as f32;
        let target = {
            let semitone = m.round().clamp(0.0, 127.0);
            match key {
                Some(k) if (m - semitone).abs() > IN_TUNE => k.snap(m),
                _ => semitone as u8,
            }
        };
        // How long `target` has been heard, this analysis included.
        let heard = |candidate: &mut Option<(u8, f64)>| {
            let held = match *candidate {
                Some((c, held)) if c == target => held + dt,
                _ => 0.0,
            };
            *candidate = Some((target, held));
            held
        };
        // A note starting: the bend is how far the voice is from it, unless the
        // scale moved it (it plays in tune, and moves with the voice from there).
        let reference = if target as f64 == m.round() { target as f64 } else { m };
        let follow = 1.0 - (-dt / BEND_SMOOTH).exp();
        match self.playing {
            None => {
                if heard(&mut self.candidate) >= AGREE - 1e-9 {
                    out.push((target, velocity));
                    self.playing = Some(target);
                    self.candidate = None;
                    self.reference = reference;
                    self.bend = m - reference;
                }
            }
            Some(p) if (m - p as f64).abs() < STRAY || target == p => {
                self.candidate = None;
                self.bend += (m - self.reference - self.bend) * follow;
                // Sung again, louder all at once: a new note on the same pitch.
                if rose {
                    out.push((p, 0.0));
                    out.push((p, velocity));
                    self.levels.clear();
                    self.levels.push_back((self.clock, level_db));
                }
            }
            Some(p) => {
                let needed = if (target as i32 - p as i32).abs() >= 12 { SETTLE_LEAP } else { SETTLE };
                if heard(&mut self.candidate) >= needed - 1e-9 {
                    out.push((p, 0.0));
                    out.push((target, velocity));
                    self.playing = Some(target);
                    self.candidate = None;
                    self.reference = reference;
                    self.bend = m - reference;
                } else {
                    // On the way to another note: the note slides after it.
                    self.bend += (m - self.reference - self.bend) * follow;
                }
            }
        }
        out
    }

    /// Whether `level_db` is `ATTACK_DB` over the level `ATTACK_SPAN` ago (and
    /// keeps it).
    fn rose(&mut self, level_db: f64) -> bool {
        let then = self.clock - ATTACK_SPAN + 1e-9;
        while self.levels.len() >= 2 && self.levels[1].0 <= then {
            self.levels.pop_front();
        }
        let rose = self.levels.front().is_some_and(|&(t, l)| t <= then && level_db - l >= ATTACK_DB);
        self.levels.push_back((self.clock, level_db));
        rose
    }

    /// Stops the note playing, if any.
    pub fn release(&mut self) -> Option<u8> {
        self.candidate = None;
        self.playing.take()
    }
}

/// What the voice plays from the input as it comes: an analysis every
/// [`HOP_SECONDS`] of it, however it arrives.
pub struct Listener {
    tracker: LiveTracker,
    rate: f32,
    hop: usize,
    /// The newest input (at least the window, once there is as much).
    recent: Vec<f32>,
    /// Input since the last analysis.
    since: usize,
    pub follower: Follower,
    /// Whether the voice's pitch bends the note, and the bend last sent
    /// (semitones).
    pub bend: bool,
    sent_bend: f64,
    /// Chords or single notes, and the notes the note playing sounds.
    pub chords: Chords,
    sounding: Vec<u8>,
    /// The key the last notes were heard in (their chords are in it).
    key: Option<Key>,
    /// Beatboxing instead of singing: the hits heard, the sound being taught
    /// (its hits play it, and are kept in `taught`), the drum notes waiting to
    /// stop (when, in input samples) and the last sound played.
    pub drums: bool,
    pub beatbox: Beatbox,
    pub teach: Option<Drum>,
    pub taught: Vec<(Drum, Features)>,
    hits_off: Vec<(u8, u64)>,
    heard: u64,
    pub last_hit: Option<Drum>,
}

impl Listener {
    pub fn new(rate: f32) -> Self {
        let window = (WINDOW_SECONDS * rate) as usize;
        Self { tracker: LiveTracker::new(window, rate, FMIN, FMAX), rate, hop: ((HOP_SECONDS * rate) as usize).max(1), recent: Vec::with_capacity(4 * window), since: 0, follower: Follower::default(), bend: false, sent_bend: 0.0, chords: Chords::Off, sounding: Vec::new(), key: None, drums: false, beatbox: Beatbox::new(rate), teach: None, taught: Vec::new(), hits_off: Vec::new(), heard: 0, last_hit: None }
    }

    /// Takes new input: the notes it starts and stops (and, following the
    /// pitch, the bends), in order, moved by `octave` octaves.
    pub fn hear(&mut self, input: &[f32], octave: i32, key: Option<Key>) -> Vec<LiveNote> {
        if self.drums {
            return self.hear_drums(input);
        }
        let window = self.tracker.len();
        self.key = key;
        let mut out = Vec::new();
        let mut rest = input;
        while !rest.is_empty() {
            let take = (self.hop - self.since).min(rest.len());
            self.recent.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            self.since += take;
            if self.since < self.hop {
                break;
            }
            self.since = 0;
            if self.recent.len() > 3 * window {
                self.recent.drain(..self.recent.len() - window);
            }
            if self.recent.len() >= window {
                let frame = &self.recent[self.recent.len() - window..];
                let rms = (frame.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / window as f64).sqrt();
                let level = 20.0 * rms.max(1e-9).log10();
                let pitch = self.tracker.pitch(frame)
                    .filter(|&(_, unsure)| unsure < UNSURE)
                    .map(|(hz, _)| 69.0 + 12.0 * (hz / 440.0).log2() + 12.0 * octave as f64)
                    .filter(|m| (0.0..=127.0).contains(m));
                let notes = self.follower.step(pitch, level, key, self.hop as f64 / self.rate as f64);
                self.send(notes, &mut out);
            }
        }
        // Turned off with the note bent: back to the note.
        if !self.bend && self.sent_bend != 0.0 {
            self.send(Vec::new(), &mut out);
        }
        out
    }

    /// Adds `notes` to `out`, with the bend where it changed: after the notes
    /// that stop, before those that start (so they start bent).
    fn send(&mut self, notes: Vec<(u8, f32)>, out: &mut Vec<LiveNote>) {
        let starts = notes.iter().position(|n| n.1 > 0.0).unwrap_or(notes.len());
        for _ in &notes[..starts] {
            out.extend(self.sounding.drain(..).map(|p| LiveNote::note(p, 0.0)));
        }
        let bend = match self.follower.playing {
            Some(_) if self.bend => self.follower.bend.clamp(-BEND_RANGE, BEND_RANGE),
            _ => 0.0,
        };
        let moved = (bend - self.sent_bend).abs() >= BEND_STEP || (bend == 0.0 && self.sent_bend != 0.0);
        // A note starting takes the bend as it is, however small the change.
        let starting = starts < notes.len() && bend != self.sent_bend;
        if moved || starting {
            out.push(bend_to(bend));
            self.sent_bend = bend;
        }
        for &(root, velocity) in &notes[starts..] {
            out.extend(self.sounding.drain(..).map(|p| LiveNote::note(p, 0.0)));
            self.sounding = chord(root, self.key, self.chords);
            out.extend(self.sounding.iter().map(|&p| LiveNote::note(p, velocity)));
        }
    }

    /// Beatboxing: a drum note for each hit (stopped [`HIT_LENGTH`] later, so
    /// it records as a note).
    fn hear_drums(&mut self, input: &[f32]) -> Vec<LiveNote> {
        let mut out = Vec::new();
        self.heard += input.len() as u64;
        let now = self.heard;
        self.hits_off.retain(|&(p, due)| {
            let done = due <= now;
            if done {
                out.push(LiveNote::note(p, 0.0));
            }
            !done
        });
        for hit in self.beatbox.hear(input) {
            let drum = self.teach.unwrap_or(hit.drum);
            if let Some(taught) = self.teach {
                self.taught.push((taught, hit.features));
            }
            let note = drum.note();
            // Hit again before it stopped.
            if let Some(i) = self.hits_off.iter().position(|&(p, _)| p == note) {
                self.hits_off.remove(i);
                out.push(LiveNote::note(note, 0.0));
            }
            out.push(LiveNote::note(note, hit.velocity));
            self.hits_off.push((note, now + (HIT_LENGTH * self.rate as f64) as u64));
            self.last_hit = Some(drum);
        }
        out
    }

    /// Stops the note playing (and brings the bend back), if any.
    pub fn release(&mut self) -> Vec<LiveNote> {
        self.follower.release();
        let mut out: Vec<LiveNote> = self.sounding.drain(..).chain(self.hits_off.drain(..).map(|(p, _)| p)).map(|p| LiveNote::note(p, 0.0)).collect();
        if self.sent_bend != 0.0 {
            out.push(bend_to(0.0));
            self.sent_bend = 0.0;
        }
        out
    }
}

/// How long a drum note beatboxed lasts (seconds).
const HIT_LENGTH: f64 = 0.05;

/// What a note sung plays: itself, or the chord on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Chords {
    #[default]
    Off,
    Triads,
    Sevenths,
}

/// The notes `root` plays: with chords, its chord stacked in thirds of the
/// scale above it (a major one without a scale, or for a note off it).
pub fn chord(root: u8, key: Option<Key>, chords: Chords) -> Vec<u8> {
    let tones = match chords {
        Chords::Off => return vec![root],
        Chords::Triads => 3,
        Chords::Sevenths => 4,
    };
    let in_scale = key.filter(|k| k.scale()[root as usize % 12]);
    (0..tones)
        .filter_map(|i| match in_scale {
            Some(k) => k.step(root, 2 * i as i32),
            None => Some(root as i32 + [0, 4, 7, 11][i]).filter(|p| *p <= 127).map(|p| p as u8),
        })
        .collect()
}

/// The pitch bend for `semitones` (over [`BEND_RANGE`]).
fn bend_to(semitones: f64) -> LiveNote {
    LiveNote::control(Controller::PitchBend, 0, (semitones / BEND_RANGE).clamp(-1.0, 1.0) as f32)
}

/// What the voice's thread and the window share.
#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    /// The octave, the project's key and whether to follow the pitch, as the
    /// window last set them.
    settings: Mutex<Settings>,
    /// The note playing (255: none).
    playing: AtomicU8,
    /// Beatboxing: the last sound played (255: none) and the hits heard
    /// while teaching, for the window to keep.
    last_hit: AtomicU8,
    taught: Mutex<Vec<(Drum, Features)>>,
}

const NO_NOTE: u8 = 255;

/// What the window sets.
#[derive(Clone, Default)]
struct Settings {
    octave: i32,
    key: Option<Key>,
    bend: bool,
    chords: Chords,
    drums: bool,
    teach: Option<Drum>,
    examples: Arc<Examples>,
}

/// The thread that listens; it gives the input back when it ends.
struct Worker {
    shared: Arc<Shared>,
    thread: std::thread::JoinHandle<VoiceInput>,
}

impl Worker {
    fn start(mut input: VoiceInput, live: LiveInput, settings: Settings) -> Option<Self> {
        let shared = Arc::new(Shared { settings: Mutex::new(settings), playing: AtomicU8::new(NO_NOTE), last_hit: AtomicU8::new(NO_NOTE), ..Default::default() });
        let mine = shared.clone();
        // What waited from before is not what is sung now.
        input.skip();
        let thread = std::thread::Builder::new()
            .name("voz".into())
            .spawn(move || {
                let mut listener = Listener::new(input.sample_rate);
                let mut new = Vec::new();
                while !mine.stop.load(Ordering::Relaxed) {
                    new.clear();
                    input.take_into(&mut new);
                    if new.is_empty() {
                        if input.closed() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    }
                    let Settings { octave, key, bend, chords, drums, teach, examples } = mine.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    if drums != listener.drums {
                        for note in listener.release() {
                            live.send(note);
                        }
                        listener.drums = drums;
                    }
                    listener.bend = bend;
                    listener.chords = chords;
                    listener.teach = teach;
                    listener.beatbox.learn(&examples);
                    for note in listener.hear(&new, octave, key) {
                        live.send(note);
                    }
                    mine.playing.store(listener.follower.playing.unwrap_or(NO_NOTE), Ordering::Relaxed);
                    mine.last_hit.store(listener.last_hit.map_or(NO_NOTE, |d| d as u8), Ordering::Relaxed);
                    if !listener.taught.is_empty() {
                        mine.taught.lock().unwrap_or_else(|e| e.into_inner()).append(&mut listener.taught);
                    }
                }
                for note in listener.release() {
                    live.send(note);
                }
                mine.playing.store(NO_NOTE, Ordering::Relaxed);
                input
            })
            .ok()?;
        Some(Self { shared, thread })
    }

    /// Stops it (the note playing stops too) and gives the input back.
    fn stop(self) -> Option<VoiceInput> {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.thread.join().ok()
    }
}

/// The voice as an instrument: on or off, the octave it is moved by, whether
/// its pitch bends the notes, whether it plays chords, and whether it plays
/// drums instead.
#[derive(Default)]
pub struct Voice {
    pub enabled: bool,
    pub octave: i32,
    pub bend: bool,
    pub chords: Chords,
    /// Beatboxing instead, the sound being taught and how many of its hits
    /// were heard.
    pub drums: bool,
    pub teaching: Option<Drum>,
    taught: usize,
    worker: Option<Worker>,
}

impl Voice {
    /// The note sung playing (the chord's root).
    pub fn playing(&self) -> Option<u8> {
        self.worker.as_ref().map(|w| w.shared.playing.load(Ordering::Relaxed)).filter(|&p| p != NO_NOTE)
    }

    /// Beatboxing: the last sound played.
    pub fn last_hit(&self) -> Option<Drum> {
        let hit = self.worker.as_ref()?.shared.last_hit.load(Ordering::Relaxed);
        Drum::ALL.into_iter().find(|d| *d as u8 == hit)
    }
}

impl PulseApp {
    /// Starts or stops playing with the voice (opening the microphone).
    pub fn toggle_voice(&mut self) {
        self.voice.enabled = !self.voice.enabled;
        if self.voice.enabled {
            self.capture_failed = false;
            if self.ensure_capture() {
                self.status = Some(if self.voice.drums {
                    "Haz beatbox: «pum» es el bombo, «pf» la caja y «ts» el charles de la pista seleccionada (una Drum Machine). Con auriculares".into()
                } else {
                    "Canta, tararea o silba: suena la pista seleccionada (o las armadas). Con auriculares, para que el micrófono no oiga los altavoces".into()
                });
            } else {
                self.voice.enabled = false;
            }
        } else {
            self.stop_voice();
            self.status = Some("Voz apagada".into());
        }
    }

    /// Stops the voice's thread, giving its input back to the capture.
    fn stop_voice(&mut self) {
        if let Some(input) = self.voice.worker.take().and_then(Worker::stop) {
            if let Some(capture) = &mut self.capture {
                capture.give_back_voice(input);
            }
        }
    }

    fn voice_settings(&self) -> Settings {
        let v = &self.voice;
        Settings { octave: v.octave, key: self.project.key, bend: v.bend, chords: v.chords, drums: v.drums, teach: v.teaching.filter(|_| v.drums), examples: Arc::new(self.settings.beatbox.clone()) }
    }

    /// Beatboxing: the next [`TEACH`] hits are `drum` (the voice on, as drums).
    pub fn teach_drum(&mut self, drum: Drum) {
        self.voice.drums = true;
        if !self.voice.enabled {
            self.toggle_voice();
            if !self.voice.enabled {
                return;
            }
        }
        self.voice.teaching = Some(drum);
        self.voice.taught = 0;
        self.status = Some(format!("{}: haz {} {TEACH} veces, como lo harías tocando", drum.name(), drum.sound()));
    }

    /// Forgets the sounds taught: back to the built-in ones.
    pub fn forget_drums(&mut self) {
        let mut settings = self.settings.clone();
        settings.beatbox = Examples::default();
        self.set_settings(settings);
        self.status = Some("Sonidos olvidados: el bombo, la caja y el charles vuelven a ser «pum», «pf» y «ts»".into());
    }

    /// Keeps the hits heard while teaching.
    fn keep_taught(&mut self) {
        let Some(worker) = &self.voice.worker else { return };
        let taught = std::mem::take(&mut *worker.shared.taught.lock().unwrap_or_else(|e| e.into_inner()));
        let Some(drum) = self.voice.teaching else { return };
        let mut examples = self.settings.beatbox.clone();
        for (d, features) in taught.into_iter().filter(|(d, _)| *d == drum) {
            if self.voice.taught == 0 {
                // Taught again: the old hits of it go.
                examples.forget(d);
            }
            examples.add(d, features);
            self.voice.taught += 1;
        }
        if examples == self.settings.beatbox {
            return;
        }
        let mut settings = self.settings.clone();
        settings.beatbox = examples;
        self.set_settings(settings);
        if self.voice.taught >= TEACH {
            self.voice.teaching = None;
            self.status = Some(format!("{} aprendido: ya suena con tu {}", drum.name(), drum.sound()));
        } else {
            self.status = Some(format!("{}: {} de {TEACH}", drum.name(), self.voice.taught));
        }
    }

    /// Every frame: the voice's thread runs while the voice is on (started
    /// again on a new input), with the octave and key as they are now.
    pub fn follow_voice(&mut self, ctx: &eframe::egui::Context) {
        let ended = self.voice.worker.as_ref().is_some_and(|w| w.thread.is_finished());
        if !self.voice.enabled || ended {
            self.stop_voice();
        }
        if !self.voice.enabled {
            self.voice.teaching = None;
            return;
        }
        if self.voice.worker.is_none() {
            let input = self.capture.as_mut().and_then(|c| c.take_voice());
            if let (Some(input), Some(live)) = (input, self.audio.live()) {
                self.voice.worker = Worker::start(input, live, self.voice_settings());
            }
        }
        self.keep_taught();
        if let Some(worker) = &self.voice.worker {
            *worker.shared.settings.lock().unwrap_or_else(|e| e.into_inner()) = self.voice_settings();
        }
        // The button shows the note.
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Analyses this far apart in the tests of the follower (a screen frame,
    /// as it used to be).
    const DT: f64 = 0.016;

    /// The notes among what the listener sends: (pitch, velocity).
    fn notes(sent: &[LiveNote]) -> Vec<(u8, f32)> {
        sent.iter().filter(|n| n.controller.is_none()).map(|n| (n.pitch, n.velocity)).collect()
    }

    /// The bends among what the listener sends, in semitones.
    fn bends(sent: &[LiveNote]) -> Vec<f64> {
        sent.iter().filter(|n| n.controller == Some(Controller::PitchBend)).map(|n| n.velocity as f64 * BEND_RANGE).collect()
    }

    /// Steps of (pitch, level) and what was sent.
    fn run(follower: &mut Follower, steps: &[(Option<f64>, f64)], key: Option<Key>) -> Vec<(u8, f32)> {
        steps.iter().flat_map(|&(p, l)| follower.step(p, l, key, DT)).collect()
    }

    /// Plays `PULSE_VOICE_IN` (a WAV) as if it came in at the microphone and
    /// prints the notes it would play.
    #[test]
    #[ignore]
    fn dump_voice() {
        let Ok(path) = std::env::var("PULSE_VOICE_IN") else { return };
        let sample = engine::Sample::load(std::path::Path::new(&path)).unwrap();
        let rate = sample.sample_rate;
        let mono: Vec<f32> = sample.left.iter().zip(&sample.right).map(|(l, r)| (l + r) * 0.5).collect();
        let mut listener = Listener::new(rate);
        let mut line = Vec::new();
        for (k, chunk) in mono.chunks(512).enumerate() {
            for (p, v) in notes(&listener.hear(chunk, 0, None)) {
                if v > 0.0 {
                    line.push(format!("{:.3}s:{p}", ((k + 1) * 512) as f32 / rate));
                }
            }
        }
        println!("{} notas: {}", line.len(), line.join(" "));
    }

    #[test]
    fn a_held_wavering_note_is_one_note() {
        let mut f = Follower::default();
        // A4 wavering ±40 cents for a while, then silence.
        let mut steps: Vec<(Option<f64>, f64)> = (0..40).map(|i| (Some(69.0 + 0.4 * ((i as f64) * 0.7).sin()), -20.0)).collect();
        steps.extend([(None, -70.0); 4]);
        let sent = run(&mut f, &steps, None);
        assert_eq!(sent.iter().map(|n| n.0).collect::<Vec<_>>(), vec![69, 69]);
        assert!(sent[0].1 > 0.5 && sent[1].1 == 0.0);
    }

    #[test]
    fn notes_change_once_the_voice_settles_and_gaps_are_bridged() {
        let mut f = Follower::default();
        // C, a glide through C# for one analysis, then D; a softer moment does
        // not stop it.
        let steps = [(Some(60.0), -20.0), (Some(60.1), -20.0), (Some(61.0), -20.0), (Some(62.0), -20.0), (Some(62.0), -26.0), (Some(62.0), -20.0), (Some(62.0), -20.0)];
        let on_off = |sent: &[(u8, f32)]| sent.iter().map(|n| (n.0, n.1 > 0.0)).collect::<Vec<_>>();
        let sent = run(&mut f, &steps, None);
        assert_eq!(on_off(&sent), vec![(60, true), (60, false), (62, true)], "{sent:?}");
        assert_eq!(f.playing, Some(62));
        // A short breath (two analyses) does not stop it either; singing on is a
        // new attack on the same note («la-la»).
        let sent = run(&mut f, &[(None, -60.0), (None, -60.0)], None);
        assert!(sent.is_empty());
        let sent = run(&mut f, &[(Some(62.0), -20.0)], None);
        assert_eq!(on_off(&sent), vec![(62, false), (62, true)]);
    }

    #[test]
    fn short_slips_and_octave_jumps_do_not_change_the_note() {
        let mut f = Follower::default();
        // A4, two analyses a third up, four an octave up: still A4.
        let mut steps = vec![(Some(69.0), -20.0); 3];
        steps.extend([(Some(73.0), -20.0); 2]);
        steps.push((Some(69.0), -20.0));
        steps.extend([(Some(81.0), -20.0); 4]);
        steps.push((Some(69.0), -20.0));
        assert_eq!(run(&mut f, &steps, None).len(), 1, "only the first A4");
        assert_eq!(f.playing, Some(69));
    }

    #[test]
    fn a_new_attack_on_the_same_note_plays_it_again() {
        let mut f = Follower::default();
        let steps = [(Some(64.0), -30.0), (Some(64.0), -30.0), (Some(64.0), -35.0), (Some(64.0), -15.0)];
        let sent = run(&mut f, &steps, None);
        assert_eq!(sent.iter().map(|n| (n.0, n.1 > 0.0)).collect::<Vec<_>>(), vec![(64, true), (64, false), (64, true)]);
    }

    #[test]
    fn with_a_key_out_of_tune_notes_go_to_the_scale() {
        let c = Key { tonic: 0, minor: false };
        // Halfway between E and F: F (in C major, the nearer scale note above
        // the middle); F# sung in tune stays F#; between F# and G: G.
        let first = |m: f64| run(&mut Follower::default(), &[(Some(m), -20.0); 2], Some(c))[0].0;
        assert_eq!(first(64.55), 65);
        assert_eq!(first(66.1), 66);
        assert_eq!(first(66.6), 67);
    }

    #[test]
    fn a_note_starts_once_two_analyses_agree() {
        let mut f = Follower::default();
        // An onset an octave low for a moment, then the note.
        assert!(f.step(Some(48.0), -20.0, None, DT).is_empty());
        assert!(f.step(Some(60.0), -20.0, None, DT).is_empty());
        assert_eq!(f.step(Some(60.0), -20.0, None, DT)[0].0, 60);
    }

    #[test]
    fn quiet_sounds_play_nothing() {
        let mut f = Follower::default();
        assert!(run(&mut f, &[(Some(60.0), -60.0), (Some(60.0), -55.0)], None).is_empty());
    }

    /// A tone from silence at the input: how long (ms) until its note plays.
    fn delay(hz: f32, rate: f32, chunk: usize) -> f32 {
        let mut listener = Listener::new(rate);
        let silence = (0.2 * rate) as usize;
        let audio: Vec<f32> = (0..(0.5 * rate) as usize)
            .map(|i| if i < silence { 0.0 } else { let t = (i - silence) as f32 / rate; 0.2 * ((std::f32::consts::TAU * hz * t).sin() + 0.5 * (2.0 * std::f32::consts::TAU * hz * t).sin()) })
            .collect();
        for (k, block) in audio.chunks(chunk).enumerate() {
            if notes(&listener.hear(block, 0, None)).iter().any(|n| n.1 > 0.0) {
                return (((k + 1) * chunk).saturating_sub(silence)) as f32 / rate * 1000.0;
            }
        }
        f32::INFINITY
    }

    /// A note plays some 30–35 ms after the voice starts (about half what it
    /// was), with the input coming in small blocks or big ones, at 48 or 96 kHz:
    /// the note has to fill what is compared (16.6 ms and a period) and hold
    /// for [`AGREE`].
    #[test]
    fn a_sung_note_plays_soon() {
        for (hz, most) in [(110.0, 38.0), (220.0, 35.0), (440.0, 35.0)] {
            for (rate, chunk) in [(48_000.0, 64), (48_000.0, 512), (96_000.0, 256)] {
                let ms = delay(hz, rate, chunk);
                println!("{hz} Hz at {rate} in blocks of {chunk}: {ms:.1} ms");
                // A big block arrives all at once: its length on top.
                let allowed = most + chunk as f32 / rate * 1000.0;
                assert!(ms <= allowed, "{hz} Hz at {rate} in blocks of {chunk}: {ms:.1} ms (at most {allowed:.1})");
            }
        }
    }

    /// The listener hears the same however the input is cut into blocks.
    #[test]
    fn the_blocks_do_not_matter() {
        let rate = 48_000.0;
        let audio: Vec<f32> = (0..(1.2 * rate) as usize)
            .map(|i| { let t = i as f32 / rate; let hz = if t < 0.6 { 220.0 } else { 330.0 }; if t < 0.1 || (1.0..1.1).contains(&t) { 0.0 } else { 0.2 * (std::f32::consts::TAU * hz * t).sin() } })
            .collect();
        let notes = |chunk: usize| {
            let mut listener = Listener::new(rate);
            audio.chunks(chunk).flat_map(|c| notes(&listener.hear(c, 0, None))).collect::<Vec<_>>()
        };
        let reference = notes(128);
        // A3, E4, and E4 again after the gap.
        assert_eq!(reference.iter().filter(|n| n.1 > 0.0).map(|n| n.0).collect::<Vec<_>>(), vec![57, 64, 64]);
        for chunk in [1, 37, 512, 4800] {
            assert_eq!(notes(chunk), reference, "in blocks of {chunk}");
        }
    }

    /// A tone at `hz` times `ratio(t)` (seconds since it started), with an
    /// octave above it, after `silence` seconds of nothing.
    fn sung(rate: f32, silence: f32, seconds: f32, hz: f32, ratio: impl Fn(f32) -> f32) -> Vec<f32> {
        let mut phase = 0f32;
        (0..((silence + seconds) * rate) as usize)
            .map(|i| {
                let t = i as f32 / rate - silence;
                if t < 0.0 {
                    return 0.0;
                }
                phase = (phase + hz * ratio(t) / rate).fract();
                0.2 * ((std::f32::consts::TAU * phase).sin() + 0.5 * (2.0 * std::f32::consts::TAU * phase).sin())
            })
            .collect()
    }

    /// What the listener sends for `audio`, in blocks of 256, following the
    /// pitch or not, then on release.
    fn listen(audio: &[f32], bend: bool, key: Option<Key>) -> Vec<LiveNote> {
        let mut listener = Listener::new(48_000.0);
        listener.bend = bend;
        let mut sent: Vec<LiveNote> = audio.chunks(256).flat_map(|c| listener.hear(c, 0, key)).collect();
        sent.extend(listener.release());
        sent
    }

    /// A vibrato (±50 cents, 5.5 Hz) on one A3: one note, bent up and down
    /// with it, and back to straight at the end. Not following the pitch: the
    /// note alone.
    #[test]
    fn a_vibrato_bends_the_note() {
        let vibrato = |t: f32| 2f32.powf(0.5 * (std::f32::consts::TAU * 5.5 * t).sin() / 12.0);
        let audio = sung(48_000.0, 0.1, 1.0, 220.0, vibrato);
        let sent = listen(&audio, true, None);
        assert_eq!(notes(&sent).iter().map(|n| (n.0, n.1 > 0.0)).collect::<Vec<_>>(), vec![(57, true), (57, false)]);
        let b = bends(&sent);
        let (low, high) = b.iter().fold((0f64, 0f64), |(l, h), &x| (l.min(x), h.max(x)));
        println!("{} bends, {low:.2}..{high:.2} semitones", b.len());
        assert!(high > 0.35 && high < 0.6 && low < -0.35 && low > -0.6, "the bend follows ±0.5: {low:.2}..{high:.2}");
        // About one every analysis at most (a vibrato moves all the time).
        assert!(b.len() < 400, "{} bends in a second", b.len());
        assert_eq!(*b.last().unwrap(), 0.0, "straight again at the end");
        assert!(matches!(sent.last(), Some(n) if n.controller == Some(Controller::PitchBend)), "the bend comes back after the note stops");
        let plain = listen(&audio, false, None);
        assert!(bends(&plain).is_empty() && notes(&plain) == notes(&sent));
    }

    /// A quick slide (60 ms) from A3 up to B3: the note bends up on the way,
    /// then B3 plays straight (the bend sent before it starts). (A slower one
    /// goes through A#3.)
    #[test]
    fn a_slide_bends_then_changes_the_note() {
        let slide = |t: f32| 2f32.powf(((t - 0.3) / 0.06).clamp(0.0, 1.0) * 2.0 / 12.0);
        let sent = listen(&sung(48_000.0, 0.1, 0.8, 220.0, slide), true, None);
        let ons: Vec<u8> = notes(&sent).iter().filter(|n| n.1 > 0.0).map(|n| n.0).collect();
        assert_eq!(ons, vec![57, 59], "{:?}", notes(&sent));
        let b_on = sent.iter().position(|n| n.controller.is_none() && n.pitch == 59 && n.velocity > 0.0).unwrap();
        let before: Vec<f64> = bends(&sent[..b_on]);
        let up = before[..before.len() - 1].iter().cloned().fold(0f64, f64::max);
        println!("up to {up:.2} on A3, then {:.2} as B3 starts", before.last().unwrap());
        assert!(up > 0.6, "A3 bends up on the way to B3: {up:.2}");
        assert!(before.last().unwrap().abs() < 0.3, "B3 starts about straight: {:.2}", before.last().unwrap());
    }

    /// A note the scale moved (a flat F#, in C major: G) plays in tune, not
    /// bent back to where it was sung.
    #[test]
    fn a_note_moved_to_the_scale_is_not_bent_back() {
        let c = Key { tonic: 0, minor: false };
        let hz = 440.0 * 2f32.powf((66.35 - 69.0) / 12.0);
        let sent = listen(&sung(48_000.0, 0.1, 0.5, hz, |_| 1.0), true, Some(c));
        assert_eq!(notes(&sent)[0].0, 67);
        let worst = bends(&sent).iter().fold(0f64, |w, x| w.max(x.abs()));
        assert!(worst < 0.1, "bent {worst:.2} semitones");
    }

    #[test]
    fn a_note_plays_its_chord_in_the_scale() {
        let c = Some(Key { tonic: 0, minor: false });
        let a = Some(Key { tonic: 9, minor: true });
        assert_eq!(chord(60, c, Chords::Off), vec![60]);
        assert_eq!(chord(60, c, Chords::Triads), vec![60, 64, 67]);
        // ii in C: D minor; vii°7: B half-diminished.
        assert_eq!(chord(62, c, Chords::Triads), vec![62, 65, 69]);
        assert_eq!(chord(71, c, Chords::Sevenths), vec![71, 74, 77, 81]);
        // I and V7 in C, i in A minor.
        assert_eq!(chord(60, c, Chords::Sevenths), vec![60, 64, 67, 71]);
        assert_eq!(chord(67, c, Chords::Sevenths), vec![67, 71, 74, 77]);
        assert_eq!(chord(57, a, Chords::Triads), vec![57, 60, 64]);
        // Without a scale, or off it: major.
        assert_eq!(chord(61, None, Chords::Triads), vec![61, 65, 68]);
        assert_eq!(chord(61, c, Chords::Sevenths), vec![61, 65, 68, 72]);
        // What would go over the top is left out.
        assert_eq!(chord(122, None, Chords::Sevenths), vec![122, 126]);
    }

    /// Singing A3 then C4 in C major, with triads: A minor, then C major,
    /// each stopped before the next, and nothing left sounding.
    #[test]
    fn a_sung_note_plays_its_chord() {
        let rate = 48_000.0;
        let mut audio = sung(rate, 0.1, 0.3, 220.0, |_| 1.0);
        audio.extend(sung(rate, 0.0, 0.3, 261.63, |_| 1.0));
        audio.extend(vec![0.0; 4800]);
        let mut listener = Listener::new(rate);
        listener.chords = Chords::Triads;
        listener.bend = true;
        let c = Some(Key { tonic: 0, minor: false });
        let mut sent: Vec<LiveNote> = audio.chunks(256).flat_map(|b| listener.hear(b, 0, c)).collect();
        sent.extend(listener.release());
        let on_off: Vec<(u8, bool)> = notes(&sent).iter().map(|n| (n.0, n.1 > 0.0)).collect();
        assert_eq!(on_off, vec![(57, true), (60, true), (64, true), (57, false), (60, false), (64, false), (60, true), (64, true), (67, true), (60, false), (64, false), (67, false)]);
        // Every chord's notes at one velocity.
        let first = notes(&sent)[0].1;
        assert!(notes(&sent)[..3].iter().all(|n| n.1 == first));
    }

    /// Beatboxing «pum ts pf»: the kick, the hat and the snare, each stopped
    /// 50 ms later; on release, nothing is left on.
    #[test]
    fn a_beatbox_plays_the_drum_notes() {
        use crate::beatbox::tests::pattern;
        let audio = pattern(&[(Drum::Kick, 0.1), (Drum::Hat, 0.35), (Drum::Snare, 0.6)], 0.62);
        let mut listener = Listener::new(48_000.0);
        listener.drums = true;
        let mut sent: Vec<LiveNote> = audio.chunks(256).flat_map(|c| listener.hear(c, 0, None)).collect();
        sent.extend(listener.release());
        let on_off: Vec<(u8, bool)> = notes(&sent).iter().map(|n| (n.0, n.1 > 0.0)).collect();
        assert_eq!(on_off, vec![(36, true), (36, false), (42, true), (42, false), (38, true), (38, false)]);
        assert_eq!(listener.last_hit, Some(Drum::Snare));
    }

    /// Teaching from the microphone: four hits (here a «ts») taught as the
    /// kick are kept in the settings, and from then on that sound plays the
    /// kick.
    #[test]
    fn hits_taught_at_the_microphone_play_what_they_were_taught_as() {
        use crate::beatbox::tests::pattern;
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let (audio, _engine) = crate::app::Audio::detached();
        app.audio = audio;
        app.sync_all();
        let (_samples, _markers, mut voice, handle) = engine::detached_input_with_voice(48_000.0, 1);
        app.capture = Some(crate::capture::InputCapture::with_handle(None, handle));
        app.voice.enabled = true;
        app.voice.drums = true;
        let ctx = eframe::egui::Context::default();
        app.follow_voice(&ctx);
        let mut feed = |app: &mut PulseApp, audio: Vec<f32>| {
            for s in audio {
                let _ = voice.push(s);
            }
            let started = std::time::Instant::now();
            while voice.slots() < voice.buffer().capacity() || started.elapsed().as_millis() < 100 {
                app.follow_voice(&ctx);
                assert!(started.elapsed().as_secs() < 5);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            app.follow_voice(&ctx);
        };
        let hats = |n: usize| pattern(&(0..n).map(|i| (Drum::Hat, 0.05 + 0.2 * i as f32)).collect::<Vec<_>>(), 0.05 + 0.2 * n as f32);
        feed(&mut app, hats(1));
        assert_eq!(app.voice.last_hit(), Some(Drum::Hat));
        app.teach_drum(Drum::Kick);
        feed(&mut app, hats(TEACH));
        assert_eq!(app.voice.teaching, None, "{:?}", app.status);
        assert_eq!(app.settings.beatbox.count(Drum::Kick), TEACH);
        feed(&mut app, hats(1));
        assert_eq!(app.voice.last_hit(), Some(Drum::Kick));
        app.forget_drums();
        app.follow_voice(&ctx);
        feed(&mut app, hats(1));
        assert_eq!(app.voice.last_hit(), Some(Drum::Hat));
    }

    /// From a microphone fed by hand: the voice's thread plays a sung A3 then
    /// C4 and stops when the voice does, without the window drawing anything.
    #[test]
    fn a_voice_at_the_microphone_plays_the_track() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let (audio, _engine) = crate::app::Audio::detached();
        app.audio = audio;
        app.sync_all();
        let (_samples, _markers, mut voice, handle) = engine::detached_input_with_voice(48_000.0, 1);
        app.capture = Some(crate::capture::InputCapture::with_handle(None, handle));
        app.voice.enabled = true;
        let ctx = eframe::egui::Context::default();
        app.follow_voice(&ctx);
        let rate = 48_000.0f32;
        let tone = |hz: f32, seconds: f32| -> Vec<f32> {
            (0..(rate * seconds) as usize).map(|i| { let t = i as f32 / rate; 0.2 * ((2.0 * std::f32::consts::PI * hz * t).sin() + 0.5 * (4.0 * std::f32::consts::PI * hz * t).sin()) }).collect()
        };
        let wait_for = |app: &PulseApp, note: Option<u8>| {
            let started = std::time::Instant::now();
            while app.voice.playing() != note {
                assert!(started.elapsed().as_secs() < 5, "never {note:?}: {:?}", app.voice.playing());
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        };
        for (sound, note) in [(tone(220.0, 0.3), Some(57)), (tone(261.63, 0.3), Some(60)), (vec![0.0; 9600], None)] {
            for &s in &sound {
                let _ = voice.push(s);
            }
            wait_for(&app, note);
        }
        // Off: the thread ends and the input goes back to the capture.
        app.voice.enabled = false;
        app.follow_voice(&ctx);
        assert!(app.voice.worker.is_none());
        assert!(app.capture.as_mut().unwrap().take_voice().is_some());
    }
}
