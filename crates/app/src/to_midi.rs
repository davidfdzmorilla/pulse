//! Audio to MIDI: the notes of an audio clip as a MIDI clip on a new track,
//! in the same place, worked out on this Mac.
//!
//! - Melodía: one note at a time (a voice, a hummed tune, a solo instrument).
//! - Armonía: every note (chords, piano, guitar).
//! - Batería: each hit as a kick, snare or hi-hat, by where its energy is.
//!
//! Pitched notes come from Basic Pitch (`stems::transcribe`); drum hits from the
//! slicer's transient detection.

use crate::app::{ClipRef, PulseApp};
use engine::model::{AudioClip, Clip, InstrumentKind, MidiClip, Note, SynthParams};
use engine::{Target, MAX_TRACKS};
use stems::transcribe::NoteEvent;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToMidi {
    Melody,
    Harmony,
    Drums,
}

impl ToMidi {
    pub const ALL: [ToMidi; 3] = [ToMidi::Melody, ToMidi::Harmony, ToMidi::Drums];

    pub fn label(self) -> &'static str {
        match self {
            ToMidi::Melody => "Melodía",
            ToMidi::Harmony => "Armonía",
            ToMidi::Drums => "Batería",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            ToMidi::Melody => "Una nota cada vez: voz, tarareo, un instrumento solista",
            ToMidi::Harmony => "Todas las notas: acordes, piano, guitarra",
            ToMidi::Drums => "Cada golpe como bombo, caja o charles, en una Drum Machine",
        }
    }
}

/// A conversion running on another thread.
pub struct MidiJob {
    clip: ClipRef,
    audio: AudioClip,
    mode: ToMidi,
    progress: Arc<Mutex<f32>>,
    result: Arc<Mutex<Option<Result<Vec<NoteEvent>, String>>>>,
}

/// General MIDI notes the drum hits become.
const KICK: u8 = 36;
const SNARE: u8 = 38;
const HAT: u8 = 42;

/// The drum hits in `mono`: when each starts (seconds), which drum, how loud.
pub fn drum_hits(mono: &[f32], rate: f32) -> Vec<NoteEvent> {
    let starts = crate::slicing::transients(mono, mono, rate, usize::MAX);
    let peak_all = mono.iter().fold(0f32, |m, s| m.max(s.abs())).max(1e-6);
    let window = (rate * 0.08) as usize;
    // One-pole filters: below 150 Hz, above 5 kHz.
    let coefficient = |hz: f32| 1.0 - (-2.0 * std::f32::consts::PI * hz / rate).exp();
    let (a_low, a_high) = (coefficient(150.0), coefficient(5000.0));
    let mut hits = Vec::new();
    for (i, &start) in starts.iter().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(mono.len()).min(start + window).min(mono.len());
        if end <= start {
            continue;
        }
        let (mut low, mut smooth) = (0f32, 0f32);
        let (mut e_low, mut e_high, mut e_all, mut peak) = (0f32, 0f32, 0f32, 0f32);
        for &x in &mono[start..end] {
            low += a_low * (x - low);
            smooth += a_high * (x - smooth);
            let high = x - smooth;
            e_low += low * low;
            e_high += high * high;
            e_all += x * x;
            peak = peak.max(x.abs());
        }
        let e_all = e_all.max(1e-12);
        let pitch = if e_low / e_all > 0.45 {
            KICK
        } else if e_high / e_all > 0.35 {
            HAT
        } else {
            SNARE
        };
        let start_s = start as f64 / rate as f64;
        hits.push(NoteEvent { start: start_s, end: start_s + 0.1, pitch, amplitude: (peak / peak_all).sqrt() });
    }
    hits
}

/// One note at a time: the strongest where notes overlap (the others are often
/// a voice's harmonics), each cut where the next begins.
pub fn melody(mut notes: Vec<NoteEvent>) -> Vec<NoteEvent> {
    notes.sort_by(|a, b| b.amplitude.total_cmp(&a.amplitude));
    let mut kept: Vec<NoteEvent> = Vec::new();
    for note in notes {
        let length = note.end - note.start;
        let overlap: f64 = kept.iter().map(|k| (note.end.min(k.end) - note.start.max(k.start)).max(0.0)).sum();
        if overlap < 0.3 * length {
            kept.push(note);
        }
    }
    kept.sort_by(|a, b| a.start.total_cmp(&b.start));
    for i in 0..kept.len().saturating_sub(1) {
        kept[i].end = kept[i].end.min(kept[i + 1].start);
    }
    kept.retain(|n| n.end - n.start > 0.05);
    kept
}

/// `events` (seconds of the clip's file) as notes in the clip's beats, at `tempo`.
pub fn clip_notes(events: &[NoteEvent], audio: &AudioClip, tempo: f64, drums: bool) -> Vec<Note> {
    let mut notes: Vec<Note> = events
        .iter()
        .filter_map(|e| {
            let start = audio.beats_until(e.start, tempo);
            let end = if drums { start + 0.25 } else { audio.beats_until(e.end, tempo) };
            if start < -1e-6 || start >= audio.length {
                return None;
            }
            let start = start.max(0.0);
            let velocity = if drums { e.amplitude.clamp(0.2, 1.0) } else { (e.amplitude * 1.25).clamp(0.25, 1.0) };
            Some(Note { start, length: (end.min(audio.length) - start).max(1.0 / 16.0), pitch: e.pitch, velocity })
        })
        .collect();
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    notes
}

/// The thread's work: the clip's part of the file, mono, and its notes or hits
/// (times in the file's seconds).
fn convert(sample: &engine::Sample, audio: &AudioClip, tempo: f64, mode: ToMidi, progress: &Mutex<f32>) -> Result<Vec<NoteEvent>, String> {
    let rate = sample.sample_rate;
    let from = audio.offset.max(0.0);
    let to = audio.source_seconds(audio.length, tempo).min(sample.duration());
    let (a, b) = ((from * rate as f64) as usize, ((to * rate as f64) as usize).min(sample.frames()));
    if b <= a + 64 {
        return Err("El clip es demasiado corto para sacar notas".into());
    }
    let mono: Vec<f32> = sample.left[a..b].iter().zip(&sample.right[a..b]).map(|(l, r)| (l + r) * 0.5).collect();
    let mut events = match mode {
        ToMidi::Drums => drum_hits(&mono, rate),
        _ => {
            let mut transcriber = stems::transcribe::Transcriber::load()?;
            let notes = transcriber.notes(&mono, rate, |f| {
                if let Ok(mut p) = progress.lock() {
                    *p = f;
                }
                true
            })?;
            if mode == ToMidi::Melody { melody(notes) } else { notes }
        }
    };
    for e in &mut events {
        e.start += from;
        e.end += from;
    }
    Ok(events)
}

impl PulseApp {
    /// Starts turning an audio clip into MIDI (see `follow_to_midi`).
    pub fn convert_to_midi(&mut self, clip: ClipRef, mode: ToMidi) {
        if self.to_midi.is_some() {
            self.status = Some("Ya se está convirtiendo un clip a MIDI: espera a que termine".into());
            return;
        }
        let Some(Clip::Audio(audio)) = self.clip(clip).cloned() else { return };
        let sample = self.sample(&audio.sample);
        if sample.frames() == 0 {
            self.status = Some("No se encuentra el audio del clip".into());
            return;
        }
        let (progress, result) = (Arc::new(Mutex::new(0.0)), Arc::new(Mutex::new(None)));
        let (p, r, a, tempo) = (progress.clone(), result.clone(), audio.clone(), self.project.tempo);
        std::thread::spawn(move || {
            let outcome = convert(&sample, &a, tempo, mode, &p);
            if let Ok(mut slot) = r.lock() {
                *slot = Some(outcome);
            }
        });
        self.status = Some(format!("Sacando las notas de «{}» ({})…", audio.name, mode.label().to_lowercase()));
        self.to_midi = Some(MidiJob { clip, audio, mode, progress, result });
    }

    /// Once a frame: shows the progress, and places the clip when it is done.
    pub fn follow_to_midi(&mut self, ctx: &eframe::egui::Context) {
        let Some(job) = &self.to_midi else { return };
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let done = job.result.lock().ok().and_then(|mut r| r.take());
        let Some(outcome) = done else {
            let fraction = job.progress.lock().map_or(0.0, |p| *p);
            if fraction > 0.0 {
                self.status = Some(format!("Sacando las notas de «{}»… {:.0} %", job.audio.name, fraction * 100.0));
            }
            return;
        };
        let job = self.to_midi.take().expect("checked above");
        match outcome {
            Ok(events) => self.place_midi(job.clip, &job.audio, job.mode, &events),
            Err(error) => self.status = Some(error),
        }
    }

    /// A new MIDI track under the clip's, with the notes in the clip's place.
    pub fn place_midi(&mut self, clip: ClipRef, audio: &AudioClip, mode: ToMidi, events: &[NoteEvent]) {
        let notes = clip_notes(events, audio, self.project.tempo, mode == ToMidi::Drums);
        if notes.is_empty() {
            self.status = Some(format!("No se encontraron notas en «{}»", audio.name));
            return;
        }
        if self.project.tracks.len() >= MAX_TRACKS {
            self.status = Some(format!("Máximo de {MAX_TRACKS} pistas"));
            return;
        }
        let instrument = match mode {
            ToMidi::Drums => InstrumentKind::DrumMachine(Default::default()),
            _ => InstrumentKind::Synth(SynthParams::default()),
        };
        let source = clip.track();
        let placed = match clip {
            ClipRef::Arrangement { track, index } => self.project.tracks.get(track).and_then(|t| t.arrangement.get(index)).map(|a| (a.start, a.length)),
            ClipRef::Session { .. } => None,
        };
        let count = notes.len();
        let midi = Clip::Midi(MidiClip { notes, ..MidiClip::new(format!("{} · {}", audio.name, mode.label()), audio.length, audio.color) });
        let name = format!("{} · {}", audio.name, mode.label());
        let mut track = crate::project::Track::new(name, audio.color, instrument);
        match (clip, placed) {
            (ClipRef::Session { slot, .. }, _) => track.clips[slot] = Some(midi),
            (_, Some((start, length))) => track.arrangement.push(engine::model::ArrangementClip { start, length, clip: midi }),
            _ => {}
        }
        // Under the clip's track.
        let at = (source + 1).min(self.project.tracks.len());
        self.project.tracks.insert(at, track);
        self.selected_target = Target::Track(at);
        self.sync_all();
        self.select_clip(match clip {
            ClipRef::Session { slot, .. } => ClipRef::Session { track: at, slot },
            ClipRef::Arrangement { .. } => ClipRef::Arrangement { track: at, index: 0 },
        });
        self.status = Some(format!("{count} notas de «{}» en una pista MIDI nueva", audio.name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(start: f64, end: f64, pitch: u8, amplitude: f32) -> NoteEvent {
        NoteEvent { start, end, pitch, amplitude }
    }

    #[test]
    fn a_melody_keeps_the_strongest_note_at_a_time() {
        // A sung note with a weaker octave above, then the next note a bit early.
        let notes = vec![note(0.0, 1.0, 60, 0.8), note(0.0, 0.9, 72, 0.3), note(0.9, 2.0, 62, 0.7), note(1.5, 1.52, 70, 0.9)];
        let line: Vec<(f64, f64, u8)> = melody(notes).iter().map(|n| (n.start, n.end, n.pitch)).collect();
        assert_eq!(line, vec![(0.0, 0.9, 60), (0.9, 1.5, 62)], "the octave goes, each note ends where the next begins, a blip under 50 ms goes");
    }

    #[test]
    fn drum_hits_are_told_apart_by_their_sound() {
        let rate = 44_100.0;
        let mut audio = vec![0f32; (rate * 1.5) as usize];
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
        };
        // A kick (60 Hz), a snare (200 Hz and noise) and a hat (filtered noise) every half second.
        let mut last = 0.0;
        for i in 0..(rate * 0.2) as usize {
            let t = i as f32 / rate;
            let decay = (-t * 25.0).exp();
            audio[i] += (2.0 * std::f32::consts::PI * 60.0 * t).sin() * decay * 0.9;
            let s = (rate * 0.5) as usize + i;
            audio[s] += ((2.0 * std::f32::consts::PI * 200.0 * t).sin() * 0.4 + noise() * 0.5) * decay;
            let h = (rate * 1.0) as usize + i;
            let n = noise();
            let high = n - last;
            last = n;
            audio[h] += high * 0.5 * (-t * 60.0).exp();
        }
        let hits = drum_hits(&audio, rate);
        let found: Vec<(u8, f64)> = hits.iter().map(|h| (h.pitch, (h.start * 100.0).round() / 100.0)).collect();
        assert_eq!(found.iter().map(|f| f.0).collect::<Vec<_>>(), vec![KICK, SNARE, HAT], "{found:?}");
    }

    #[test]
    fn notes_land_on_the_clips_beats() {
        let audio = AudioClip {
            name: "Voz".into(),
            color: [0; 3],
            sample: String::new(),
            length: 8.0,
            offset: 1.0,
            warp: engine::model::WarpMode::Stretch,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(),
        };
        // At 120 BPM half a second is a beat; the clip starts 1 s into the file.
        let notes = clip_notes(&[note(1.0, 1.5, 60, 0.6), note(2.0, 3.0, 64, 0.6), note(6.0, 7.0, 67, 0.6)], &audio, 100.0, false);
        let placed: Vec<(f64, f64, u8)> = notes.iter().map(|n| (n.start, n.length, n.pitch)).collect();
        assert_eq!(placed, vec![(0.0, 1.0, 60), (2.0, 2.0, 64)], "the last is past the clip's end");
    }

    /// From the clip's menu to a MIDI track under it: the test melody (C4 E4 G4
    /// C5 then a chord) in the clip's beats.
    #[test]
    fn a_clip_becomes_a_midi_track_under_it() {
        let melody = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../stems/tests/data/melody.wav");
        let mut app = PulseApp::headless(crate::project::Project::demo());
        app.project.tempo = 120.0;
        let t = app.add_audio_track().unwrap();
        let audio = AudioClip {
            name: "Melodía".into(),
            color: [1, 2, 3],
            sample: melody.to_string_lossy().into_owned(),
            length: 8.0,
            offset: 0.0,
            warp: engine::model::WarpMode::Stretch,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(),
        };
        app.project.tracks[t].clips[2] = Some(Clip::Audio(audio));
        let tracks = app.project.tracks.len();
        let ctx = eframe::egui::Context::default();
        for (mode, expected) in [(ToMidi::Melody, 5), (ToMidi::Harmony, 12)] {
            app.convert_to_midi(ClipRef::Session { track: t, slot: 2 }, mode);
            let started = std::time::Instant::now();
            while app.to_midi.is_some() {
                let _ = ctx.run_ui(Default::default(), |ui| app.follow_to_midi(ui.ctx()));
                std::thread::sleep(std::time::Duration::from_millis(20));
                assert!(started.elapsed().as_secs() < 60);
            }
            // Under the audio track, in the same slot.
            let Some(Clip::Midi(midi)) = &app.project.tracks[t + 1].clips[2] else { panic!("{:?}", app.status) };
            assert_eq!(midi.notes.len(), expected, "{mode:?}: {:?}", midi.notes);
            let first: Vec<(f64, u8)> = midi.notes.iter().take(4).map(|n| ((n.start * 4.0).round() / 4.0, n.pitch)).collect();
            assert_eq!(first, vec![(0.0, 60), (1.0, 64), (2.0, 67), (3.0, 72)], "{mode:?}: a beat each at 120 BPM");
        }
        assert_eq!(app.project.tracks.len(), tracks + 2);
    }
}
