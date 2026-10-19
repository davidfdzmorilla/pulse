//! Playing with the voice: sing, hum or whistle into the microphone and the
//! selected (or armed) track plays it, a note for each note sung, as from a
//! MIDI keyboard (so it records too). The pitch is followed as it comes in
//! (`stems::pitch::live`); a note holds while the voice wavers around it and
//! changes once the voice settles on another; with a project key, a note sung
//! off the scale goes to the scale's nearest.

use crate::app::PulseApp;
use engine::LiveNote;
use stems::key::Key;

/// Input analysed each time: about 43 ms at 48 kHz, two periods of a low voice.
const WINDOW_SECONDS: f32 = 0.043;
/// The lowest and highest pitch followed (Hz): a bass voice to a whistle.
const FMIN: f64 = 65.0;
const FMAX: f64 = 1_600.0;
/// How unsure a pitch may be and still count (YIN's aperiodicity).
const UNSURE: f64 = 0.2;
/// Below this (dBFS), silence.
const GATE_DB: f64 = -48.0;
/// How far (semitones) the voice may stray from the note playing and keep it.
const STRAY: f64 = 0.75;
/// Analyses in a row on another note before it changes (an octave or more
/// away, more: pitch trackers slip by octaves), and without a voice before the
/// note stops (about 16 ms each, at the screen's rate).
const SETTLE: u8 = 3;
const SETTLE_LEAP: u8 = 5;
const RELEASE: u8 = 3;
/// A jump in level (dB) that is a new attack on the same note.
const ATTACK_DB: f64 = 9.0;
/// Within this of a semitone (semitones), a note is sung in tune and stays,
/// even off the scale.
const IN_TUNE: f64 = 0.3;

/// Turns what is heard, analysis after analysis, into notes on and off.
#[derive(Default)]
pub struct Follower {
    pub playing: Option<u8>,
    candidate: Option<(u8, u8)>,
    quiet: u8,
    level: f64,
}

impl Follower {
    /// What one analysis heard: the pitch (fractional MIDI) if there is a voice,
    /// and how loud (dBFS). Returns the notes to send: (pitch, velocity), a
    /// velocity of 0 being a note off.
    pub fn step(&mut self, pitch: Option<f64>, level_db: f64, key: Option<Key>) -> Vec<(u8, f32)> {
        let mut out = Vec::new();
        let voiced = pitch.filter(|_| level_db > GATE_DB);
        let Some(m) = voiced else {
            self.quiet = self.quiet.saturating_add(1);
            self.candidate = None;
            if self.quiet >= RELEASE {
                if let Some(p) = self.playing.take() {
                    out.push((p, 0.0));
                }
            }
            self.level = level_db;
            return out;
        };
        self.quiet = 0;
        let velocity = ((level_db - GATE_DB) / 36.0).clamp(0.25, 1.0) as f32;
        let target = {
            let semitone = m.round().clamp(0.0, 127.0);
            match key {
                Some(k) if (m - semitone).abs() > IN_TUNE => k.snap(m),
                _ => semitone as u8,
            }
        };
        match self.playing {
            // From silence, once two analyses agree (the first moments of a
            // note are often off, even by an octave).
            None => {
                let agreed = matches!(self.candidate, Some((c, _)) if c == target);
                self.candidate = Some((target, 1));
                if agreed {
                    out.push((target, velocity));
                    self.playing = Some(target);
                    self.candidate = None;
                }
            }
            Some(p) if (m - p as f64).abs() < STRAY || target == p => {
                self.candidate = None;
                // Sung again, louder all at once: a new note on the same pitch.
                if level_db - self.level >= ATTACK_DB {
                    out.push((p, 0.0));
                    out.push((p, velocity));
                }
            }
            Some(p) => {
                let count = match self.candidate {
                    Some((c, n)) if c == target => n + 1,
                    _ => 1,
                };
                self.candidate = Some((target, count));
                let needed = if (target as i32 - p as i32).abs() >= 12 { SETTLE_LEAP } else { SETTLE };
                if count >= needed {
                    out.push((p, 0.0));
                    out.push((target, velocity));
                    self.playing = Some(target);
                    self.candidate = None;
                }
            }
        }
        self.level = level_db;
        out
    }

    /// Stops the note playing, if any.
    pub fn release(&mut self) -> Option<u8> {
        self.candidate = None;
        self.playing.take()
    }
}

/// The voice as an instrument: on or off, the octave it is moved by, and where
/// the input was analysed last.
#[derive(Default)]
pub struct Voice {
    pub enabled: bool,
    pub octave: i32,
    follower: Follower,
    analysed: u64,
}

impl Voice {
    pub fn playing(&self) -> Option<u8> {
        self.follower.playing
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
            self.release_voice();
            self.status = Some("Voz apagada".into());
        }
    }

    fn release_voice(&mut self) {
        if let (Some(p), Some(live)) = (self.voice.follower.release(), self.audio.live()) {
            live.send(LiveNote::note(p, 0.0));
        }
    }

    /// Every frame: what the microphone hears since the last analysis, as notes.
    pub fn follow_voice(&mut self, ctx: &eframe::egui::Context) {
        if !self.voice.enabled {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(8));
        let Some(capture) = &self.capture else { return };
        let rate = capture.sample_rate();
        let window = (WINDOW_SECONDS * rate) as usize;
        let end = capture.end_frame();
        // A new analysis once there are a few ms of new input.
        if end < self.voice.analysed + (rate * 0.005) as u64 {
            return;
        }
        let Some(frame) = capture.latest(window) else { return };
        self.voice.analysed = end;
        let notes = self.hear(&frame, rate);
        if let Some(live) = self.audio.live() {
            for (pitch, velocity) in notes {
                live.send(LiveNote::note(pitch, velocity));
            }
        }
    }

    /// One analysis of `frame`: the notes it starts and stops.
    fn hear(&mut self, frame: &[f32], rate: f32) -> Vec<(u8, f32)> {
        let rms = (frame.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / frame.len().max(1) as f64).sqrt();
        let level = 20.0 * rms.max(1e-9).log10();
        let pitch = stems::pitch::live(frame, rate, FMIN, FMAX)
            .filter(|&(_, unsure)| unsure < UNSURE)
            .map(|(hz, _)| 69.0 + 12.0 * (hz / 440.0).log2() + 12.0 * self.voice.octave as f64)
            .filter(|m| (0.0..=127.0).contains(m));
        self.voice.follower.step(pitch, level, self.project.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steps of (pitch, level) and what was sent.
    fn run(follower: &mut Follower, steps: &[(Option<f64>, f64)], key: Option<Key>) -> Vec<(u8, f32)> {
        steps.iter().flat_map(|&(p, l)| follower.step(p, l, key)).collect()
    }

    /// Plays `PULSE_VOICE_IN` (a WAV) as if it came in at the microphone, a
    /// screen frame at a time, and prints the notes it would play.
    #[test]
    #[ignore]
    fn dump_voice() {
        let Ok(path) = std::env::var("PULSE_VOICE_IN") else { return };
        let sample = engine::Sample::load(std::path::Path::new(&path)).unwrap();
        let rate = sample.sample_rate;
        let mono: Vec<f32> = sample.left.iter().zip(&sample.right).map(|(l, r)| (l + r) * 0.5).collect();
        let (window, hop) = ((WINDOW_SECONDS * rate) as usize, (rate / 60.0) as usize);
        let mut f = Follower::default();
        let mut line = Vec::new();
        let mut at = window;
        while at <= mono.len() {
            let frame = &mono[at - window..at];
            let rms = (frame.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / frame.len() as f64).sqrt();
            let level = 20.0 * rms.max(1e-9).log10();
            let pitch = stems::pitch::live(frame, rate, FMIN, FMAX).filter(|&(_, u)| u < UNSURE).map(|(hz, _)| 69.0 + 12.0 * (hz / 440.0).log2());
            for (p, v) in f.step(pitch, level, None) {
                if v > 0.0 {
                    line.push(format!("{:.2}s:{p}", at as f32 / rate));
                }
            }
            at += hop;
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
        assert!(f.step(Some(48.0), -20.0, None).is_empty());
        assert!(f.step(Some(60.0), -20.0, None).is_empty());
        assert_eq!(f.step(Some(60.0), -20.0, None)[0].0, 60);
    }

    #[test]
    fn quiet_sounds_play_nothing() {
        let mut f = Follower::default();
        assert!(run(&mut f, &[(Some(60.0), -60.0), (Some(60.0), -55.0)], None).is_empty());
    }

    /// From a microphone fed by hand to the track: a sung A3 then C4 sound on
    /// the input track, and stop when the voice does.
    #[test]
    fn a_voice_at_the_microphone_plays_the_track() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let (audio, _engine) = crate::app::Audio::detached();
        app.audio = audio;
        app.sync_all();
        let (mut samples, _markers, handle) = engine::detached_input(48_000.0, 1);
        app.capture = Some(crate::capture::InputCapture::with_handle(None, handle));
        app.voice.enabled = true;
        // The bass track takes the input.
        app.audio.send(engine::Command::SetInputTrack(Some(1)));
        let rate = 48_000.0f32;
        let tone = |hz: f32, seconds: f32| -> Vec<f32> {
            (0..(rate * seconds) as usize).map(|i| { let t = i as f32 / rate; 0.2 * ((2.0 * std::f32::consts::PI * hz * t).sin() + 0.5 * (4.0 * std::f32::consts::PI * hz * t).sin()) }).collect()
        };
        let mut heard = Vec::new();
        let ctx = eframe::egui::Context::default();
        let mut feed = |app: &mut PulseApp, audio: &[f32], heard: &mut Vec<Option<u8>>| {
            // A frame of the screen at a time (about 16 ms).
            for chunk in audio.chunks(768) {
                for &s in chunk {
                    let _ = samples.push(s);
                }
                if let Some(capture) = app.capture.as_mut() {
                    capture.pump(false);
                }
                app.follow_voice(&ctx);
                heard.push(app.voice.playing());
            }
        };
        feed(&mut app, &tone(220.0, 0.3), &mut heard);
        feed(&mut app, &tone(261.63, 0.3), &mut heard);
        feed(&mut app, &vec![0.0; 9600], &mut heard);
        let played: Vec<u8> = heard.iter().flatten().copied().fold(Vec::new(), |mut v, p| {
            if v.last() != Some(&p) {
                v.push(p);
            }
            v
        });
        assert_eq!(played, vec![57, 60], "{heard:?}");
        assert_eq!(heard.last(), Some(&None));
    }
}
