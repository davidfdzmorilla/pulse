//! Playing with the voice: sing, hum or whistle into the microphone and the
//! selected (or armed) track plays it, a note for each note sung, as from a
//! MIDI keyboard (so it records too). The pitch is followed as it comes in
//! (`stems::pitch::LiveTracker`), every few milliseconds, on a thread of its
//! own (not at the screen's pace); a note holds while the voice wavers around
//! it and changes once the voice settles on another; with a project key, a
//! note sung off the scale goes to the scale's nearest.

use crate::app::PulseApp;
use engine::{LiveInput, LiveNote, VoiceInput};
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
        match self.playing {
            None => {
                if heard(&mut self.candidate) >= AGREE - 1e-9 {
                    out.push((target, velocity));
                    self.playing = Some(target);
                    self.candidate = None;
                }
            }
            Some(p) if (m - p as f64).abs() < STRAY || target == p => {
                self.candidate = None;
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
}

impl Listener {
    pub fn new(rate: f32) -> Self {
        let window = (WINDOW_SECONDS * rate) as usize;
        Self { tracker: LiveTracker::new(window, rate, FMIN, FMAX), rate, hop: ((HOP_SECONDS * rate) as usize).max(1), recent: Vec::with_capacity(4 * window), since: 0, follower: Follower::default() }
    }

    /// Takes new input: the notes it starts and stops, in order, moved by
    /// `octave` octaves.
    pub fn hear(&mut self, input: &[f32], octave: i32, key: Option<Key>) -> Vec<(u8, f32)> {
        let window = self.tracker.len();
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
                out.extend(self.follower.step(pitch, level, key, self.hop as f64 / self.rate as f64));
            }
        }
        out
    }
}

/// What the voice's thread and the window share.
#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    /// The octave and the project's key, as the window last set them.
    settings: Mutex<(i32, Option<Key>)>,
    /// The note playing (255: none).
    playing: AtomicU8,
}

const NO_NOTE: u8 = 255;

/// The thread that listens; it gives the input back when it ends.
struct Worker {
    shared: Arc<Shared>,
    thread: std::thread::JoinHandle<VoiceInput>,
}

impl Worker {
    fn start(mut input: VoiceInput, live: LiveInput, octave: i32, key: Option<Key>) -> Option<Self> {
        let shared = Arc::new(Shared { settings: Mutex::new((octave, key)), playing: AtomicU8::new(NO_NOTE), ..Default::default() });
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
                    let (octave, key) = *mine.settings.lock().unwrap_or_else(|e| e.into_inner());
                    for (pitch, velocity) in listener.hear(&new, octave, key) {
                        live.send(LiveNote::note(pitch, velocity));
                    }
                    mine.playing.store(listener.follower.playing.unwrap_or(NO_NOTE), Ordering::Relaxed);
                }
                if let Some(p) = listener.follower.release() {
                    live.send(LiveNote::note(p, 0.0));
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

/// The voice as an instrument: on or off, and the octave it is moved by.
#[derive(Default)]
pub struct Voice {
    pub enabled: bool,
    pub octave: i32,
    worker: Option<Worker>,
}

impl Voice {
    pub fn playing(&self) -> Option<u8> {
        self.worker.as_ref().map(|w| w.shared.playing.load(Ordering::Relaxed)).filter(|&p| p != NO_NOTE)
    }
}

impl PulseApp {
    /// Starts or stops playing with the voice (opening the microphone).
    pub fn toggle_voice(&mut self) {
        self.voice.enabled = !self.voice.enabled;
        if self.voice.enabled {
            self.capture_failed = false;
            if self.ensure_capture() {
                self.status = Some("Canta, tararea o silba: suena la pista seleccionada (o las armadas). Con auriculares, para que el micrófono no oiga los altavoces".into());
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

    /// Every frame: the voice's thread runs while the voice is on (started
    /// again on a new input), with the octave and key as they are now.
    pub fn follow_voice(&mut self, ctx: &eframe::egui::Context) {
        let ended = self.voice.worker.as_ref().is_some_and(|w| w.thread.is_finished());
        if !self.voice.enabled || ended {
            self.stop_voice();
        }
        if !self.voice.enabled {
            return;
        }
        if self.voice.worker.is_none() {
            let input = self.capture.as_mut().and_then(|c| c.take_voice());
            if let (Some(input), Some(live)) = (input, self.audio.live()) {
                self.voice.worker = Worker::start(input, live, self.voice.octave, self.project.key);
            }
        }
        if let Some(worker) = &self.voice.worker {
            *worker.shared.settings.lock().unwrap_or_else(|e| e.into_inner()) = (self.voice.octave, self.project.key);
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
            for (p, v) in listener.hear(chunk, 0, None) {
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
            if listener.hear(block, 0, None).iter().any(|n| n.1 > 0.0) {
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
            audio.chunks(chunk).flat_map(|c| listener.hear(c, 0, None)).collect::<Vec<_>>()
        };
        let reference = notes(128);
        // A3, E4, and E4 again after the gap.
        assert_eq!(reference.iter().filter(|n| n.1 > 0.0).map(|n| n.0).collect::<Vec<_>>(), vec![57, 64, 64]);
        for chunk in [1, 37, 512, 4800] {
            assert_eq!(notes(chunk), reference, "in blocks of {chunk}");
        }
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
