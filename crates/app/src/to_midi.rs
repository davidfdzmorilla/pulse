//! Audio to MIDI: the notes of an audio clip as a MIDI clip on a new track,
//! in the same place, worked out on this Mac.
//!
//! - Melodía: one note at a time (a voice, a hummed tune, a solo instrument).
//! - Armonía: every note (chords, piano, guitar).
//! - Acordes: the chords of a song or a part, one per change, voiced.
//! - Batería: each hit as a kick, snare or hi-hat, by where its energy is.
//!
//! A melody's notes come from following the pitch of the voice (`stems::pitch`,
//! pYIN); chords' from Basic Pitch (`stems::transcribe`); drum hits from the
//! slicer's transient detection.

use crate::app::{ClipRef, PulseApp};
use engine::model::{AudioClip, Clip, InstrumentKind, MidiClip, Note, SynthParams};
use engine::{Target, MAX_TRACKS};
use stems::transcribe::NoteEvent;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToMidi {
    Melody,
    /// A melody without the key's scale: each note at its nearest semitone.
    MelodyFree,
    Harmony,
    Chords,
    Drums,
}

impl ToMidi {
    pub const ALL: [ToMidi; 5] = [ToMidi::Melody, ToMidi::MelodyFree, ToMidi::Harmony, ToMidi::Chords, ToMidi::Drums];

    pub fn label(self) -> &'static str {
        match self {
            ToMidi::Melody => "Melodía",
            ToMidi::MelodyFree => "Melodía sin ajustar",
            ToMidi::Harmony => "Armonía",
            ToMidi::Chords => "Acordes",
            ToMidi::Drums => "Batería",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            ToMidi::Melody => "Una nota cada vez: voz, tarareo, un instrumento solista. Cada nota va a la de la escala de la canción más cercana, así que cantar un poco alto o bajo no cambia la nota",
            ToMidi::MelodyFree => "Como Melodía, pero cada nota al semitono más cercano, sin la escala (para melodías con notas fuera de la tonalidad)",
            ToMidi::Harmony => "Todas las notas: acordes, piano, guitarra",
            ToMidi::Chords => "Los acordes de una canción o de un instrumento, uno por cada cambio (como mucho cada medio compás), con las voces enlazadas: para tocarlos con otro sonido",
            ToMidi::Drums => "Cada golpe como bombo, caja o charles, en una Drum Machine",
        }
    }
}

/// A conversion running on another thread.
pub struct MidiJob {
    clip: ClipRef,
    audio: AudioClip,
    mode: ToMidi,
    /// Whether the key was the project's (rather than heard).
    chosen: bool,
    progress: Arc<Mutex<f32>>,
    result: Arc<Mutex<Option<Result<Converted, String>>>>,
}

/// What a conversion found: the notes (seconds of the file) and, for a melody,
/// the key it was heard in.
type Converted = (Vec<NoteEvent>, Option<stems::key::Key>);

/// How clearly a file must fit a key to count for the song's.
const CLEAR_KEY: f64 = 0.6;

/// What else sounds in the song, to find its key: the other audio files, and
/// how much each pitch class plays in its MIDI clips (drums left out).
struct Context {
    audio: Vec<Arc<engine::Sample>>,
    midi: [f64; 12],
    /// The project's key, if chosen: melodies go to it, nothing is detected.
    key: Option<stems::key::Key>,
}

impl Context {
    fn weights(&self) -> [f64; 12] {
        let normalized = |w: [f64; 12]| -> [f64; 12] {
            let total: f64 = w.iter().sum();
            if total > 0.0 { w.map(|x| x / total) } else { w }
        };
        let mut sum = normalized(self.midi);
        for sample in &self.audio {
            let mono: Vec<f32> = sample.left.iter().zip(&sample.right).map(|(l, r)| (l + r) * 0.5).collect();
            let chroma = stems::key::chroma(&mono, sample.sample_rate);
            // Only files with a clear key of their own: drums and ambiguous loops
            // would only blur it.
            if stems::key::Key::detect_clearly(&chroma).is_some_and(|(_, clarity)| clarity >= CLEAR_KEY) {
                for (s, c) in sum.iter_mut().zip(normalized(chroma)) {
                    *s += c;
                }
            }
        }
        sum
    }
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
fn convert(sample: &engine::Sample, audio: &AudioClip, tempo: f64, mode: ToMidi, context: &Context, progress: &Mutex<f32>) -> Result<Converted, String> {
    let rate = sample.sample_rate;
    let from = audio.offset.max(0.0);
    let to = audio.source_seconds(audio.length, tempo).min(sample.duration());
    let (a, b) = ((from * rate as f64) as usize, ((to * rate as f64) as usize).min(sample.frames()));
    if b <= a + 64 {
        return Err("El clip es demasiado corto para sacar notas".into());
    }
    let mono: Vec<f32> = sample.left[a..b].iter().zip(&sample.right[a..b]).map(|(l, r)| (l + r) * 0.5).collect();
    let (mut events, key) = match mode {
        ToMidi::Drums => (drum_hits(&mono, rate), None),
        // One voice: its pitch followed frame by frame (pYIN), cut into notes,
        // heard in the song's key.
        ToMidi::Melody => match context.key {
            Some(key) => (stems::pitch::melody_in(&mono, rate, key), Some(key)),
            None => stems::pitch::melody(&mono, rate, context.weights(), true),
        },
        ToMidi::MelodyFree => stems::pitch::melody(&mono, rate, context.weights(), false),
        ToMidi::Chords => chord_events(&mono, rate, audio, tempo, from, context.key),
        ToMidi::Harmony => {
            let mut transcriber = stems::transcribe::Transcriber::load()?;
            let notes = transcriber.notes(&mono, rate, |f| {
                if let Ok(mut p) = progress.lock() {
                    *p = f;
                }
                true
            })?;
            (notes, None)
        }
    };
    for e in &mut events {
        e.start += from;
        e.end += from;
    }
    Ok((events, key))
}

/// The chords of `mono` (the clip's audio from `from` seconds of its file), half
/// a bar of the clip at a time, voiced one into the next; in `key` if known,
/// else in the one the audio sounds in.
fn chord_events(mono: &[f32], rate: f32, audio: &AudioClip, tempo: f64, from: f64, key: Option<stems::key::Key>) -> (Vec<NoteEvent>, Option<stems::key::Key>) {
    let key = key.or_else(|| stems::key::Key::detect(&stems::key::chroma(mono, rate)));
    let gram = stems::chord::Chromagram::new(mono, rate);
    let mut stretches = Vec::new();
    let mut beat = 0.0;
    while beat < audio.length - 1e-9 {
        let end = (beat + 2.0).min(audio.length);
        stretches.push((audio.source_seconds(beat, tempo) - from, audio.source_seconds(end, tempo) - from));
        beat = end;
    }
    let mut events = Vec::new();
    let mut previous: Vec<u8> = Vec::new();
    for (start, end, chord) in gram.chords(&stretches, key) {
        let Some(chord) = chord else { continue };
        let upper = if previous.len() > 1 { &previous[1..] } else { &previous[..] };
        let pitches = stems::chord::voice(&chord, upper);
        for &pitch in &pitches {
            events.push(NoteEvent { start, end, pitch, amplitude: 0.6 });
        }
        previous = pitches;
    }
    (events, key)
}

/// The chords of an audio clip, in its beats (half a bar at a time, equal ones
/// run together), in `key` if known, else in the one it sounds in.
pub fn audio_chords(sample: &engine::Sample, audio: &AudioClip, tempo: f64, key: Option<stems::key::Key>) -> Vec<(f64, f64, stems::chord::Chord)> {
    let rate = sample.sample_rate;
    let from = audio.offset.max(0.0);
    let to = audio.source_seconds(audio.length, tempo).min(sample.duration());
    let (a, b) = ((from * rate as f64) as usize, ((to * rate as f64) as usize).min(sample.frames()));
    if b <= a + 64 {
        return Vec::new();
    }
    let mono: Vec<f32> = sample.left[a..b].iter().zip(&sample.right[a..b]).map(|(l, r)| (l + r) * 0.5).collect();
    let key = key.or_else(|| stems::key::Key::detect(&stems::key::chroma(&mono, rate)));
    let gram = stems::chord::Chromagram::new(&mono, rate);
    // Each stretch in beats and in seconds of `mono`.
    let mut bounds: Vec<(f64, f64, f64, f64)> = Vec::new();
    let mut beat = 0.0;
    while beat < audio.length - 1e-9 {
        let end = (beat + 2.0).min(audio.length);
        bounds.push((beat, end, audio.source_seconds(beat, tempo) - from, audio.source_seconds(end, tempo) - from));
        beat = end;
    }
    let stretches: Vec<(f64, f64)> = bounds.iter().map(|b| (b.2, b.3)).collect();
    gram.chords(&stretches, key)
        .into_iter()
        .filter_map(|(s0, s1, chord)| {
            let start = bounds.iter().find(|b| (b.2 - s0).abs() < 1e-9)?.0;
            let end = bounds.iter().find(|b| (b.3 - s1).abs() < 1e-9)?.1;
            Some((start, end, chord?))
        })
        .collect()
}

impl PulseApp {
    /// The rest of the song, for finding its key (see [`Context`]).
    fn key_context(&mut self, own: &str) -> Context {
        let mut paths: Vec<String> = Vec::new();
        let mut midi = [0f64; 12];
        for track in &self.project.tracks {
            let clips = track.clips.iter().flatten().chain(track.arrangement.iter().map(|a| &a.clip));
            for clip in clips {
                match clip {
                    Clip::Audio(a) if a.sample != own && !paths.contains(&a.sample) => paths.push(a.sample.clone()),
                    Clip::Midi(m) if !track.instrument.is_drums() => {
                        for n in &m.notes {
                            midi[n.pitch as usize % 12] += n.length;
                        }
                    }
                    _ => {}
                }
            }
        }
        // A dozen files are plenty to hear the key.
        let audio = paths.into_iter().take(12).map(|p| self.sample(&p)).filter(|s| s.frames() > 0).collect();
        Context { audio, midi, key: self.project.key }
    }

    /// Sets the project's key to the one its clips sound in: the MIDI notes and
    /// the audio files with a clear key of their own.
    pub fn detect_key(&mut self) {
        let context = self.key_context("");
        match stems::key::Key::detect_clearly(&context.weights()) {
            Some((key, clarity)) => {
                self.set_key(Some(key));
                self.status = Some(if clarity >= CLEAR_KEY {
                    format!("La canción está en {}", key.name())
                } else {
                    format!("La canción parece estar en {}, aunque no está muy claro: si no lo es, elige la escala a mano", key.name())
                });
            }
            None => self.status = Some("No hay notas ni audio con una tonalidad clara de donde sacarla: elige la escala a mano".into()),
        }
    }
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
        let context = match mode {
            // With the project's key nothing needs hearing.
            ToMidi::Melody if self.project.key.is_some() => Context { audio: Vec::new(), midi: [0.0; 12], key: self.project.key },
            ToMidi::Melody => self.key_context(&audio.sample),
            ToMidi::MelodyFree => Context { key: None, ..self.key_context(&audio.sample) },
            ToMidi::Chords => Context { audio: Vec::new(), midi: [0.0; 12], key: self.project.key },
            _ => Context { audio: Vec::new(), midi: [0.0; 12], key: None },
        };
        let chosen = self.project.key.is_some();
        let (progress, result) = (Arc::new(Mutex::new(0.0)), Arc::new(Mutex::new(None)));
        let (p, r, a, tempo) = (progress.clone(), result.clone(), audio.clone(), self.project.tempo);
        std::thread::spawn(move || {
            let outcome = convert(&sample, &a, tempo, mode, &context, &p);
            if let Ok(mut slot) = r.lock() {
                *slot = Some(outcome);
            }
        });
        self.status = Some(format!("Sacando las notas de «{}» ({})…", audio.name, mode.label().to_lowercase()));
        self.to_midi = Some(MidiJob { clip, audio, mode, chosen, progress, result });
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
            Ok((events, key)) => {
                self.place_midi(job.clip, &job.audio, job.mode, &events);
                if let (Some(key), Some(status)) = (key, self.status.as_mut()) {
                    status.push_str(&match job.mode {
                        ToMidi::Melody if job.chosen => format!(", ajustadas a la escala del proyecto ({})", key.name()),
                        ToMidi::Melody => format!(", en {} (ajustadas a su escala)", key.name()),
                        ToMidi::Chords => format!(", en {}", key.name()),
                        _ => format!(" (la canción parece estar en {})", key.name()),
                    });
                }
            }
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
        // Beside its source, in the same group.
        track.depth = self.project.tracks[source].depth;
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
            key: None,
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
            key: None,
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

    /// «Acordes»: Am F C G from audio (a bar each at 120 BPM) as voiced chords.
    #[test]
    fn the_chords_of_audio_become_voiced_chords() {
        let rate = 22_050.0;
        let progression: [&[u8]; 4] = [&[45, 57, 60, 64], &[41, 57, 60, 65], &[48, 55, 60, 64], &[43, 55, 59, 62]];
        let mut mono = Vec::new();
        for chord in progression {
            for i in 0..(rate * 2.0) as usize {
                let t = i as f32 / rate;
                let v: f32 = chord.iter().map(|&p| {
                    let f = 440.0 * 2f32.powf((p as f32 - 69.0) / 12.0);
                    (1..5).map(|h| (2.0 * std::f32::consts::PI * f * h as f32 * t).sin() / (h * h) as f32).sum::<f32>()
                }).sum();
                mono.push(v * 0.1);
            }
        }
        let sample = engine::Sample::from_channels(mono.clone(), mono, rate);
        let audio = AudioClip {
            name: "Piano".into(),
            color: [0; 3],
            sample: String::new(),
            length: 16.0,
            offset: 0.0,
            warp: engine::model::WarpMode::Stretch,
            original_bpm: 120.0,
            key: None,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(),
        };
        let context = Context { audio: Vec::new(), midi: [0.0; 12], key: None };
        let (events, key) = convert(&sample, &audio, 120.0, ToMidi::Chords, &context, &Mutex::new(0.0)).unwrap();
        assert_eq!(key.map(|k| k.name()).as_deref(), Some("Do mayor"));
        let notes = clip_notes(&events, &audio, 120.0, false);
        let named: Vec<(f64, String)> = crate::harmony::chords(&notes, 16.0, key).iter().filter_map(|(a, _, c)| c.map(|c| (*a, c.name()))).collect();
        let expected = [(0.0, "Lam"), (4.0, "Fa"), (8.0, "Do"), (12.0, "Sol")].map(|(b, n)| (b, n.to_string()));
        assert_eq!(named, expected);
        assert_eq!(notes.len(), 16, "a bass and three notes each");
        // The same chords in the clip's beats, for accompanying it.
        let chords: Vec<(f64, f64, String)> = audio_chords(&sample, &audio, 120.0, None).into_iter().map(|(a, b, c)| (a, b, c.name())).collect();
        let expected = [(0.0, 4.0, "Lam"), (4.0, 8.0, "Fa"), (8.0, 12.0, "Do"), (12.0, 16.0, "Sol")].map(|(a, b, n)| (a, b, n.to_string()));
        assert_eq!(chords, expected);
    }

    /// «Detectar»: the key the MIDI clips play in, leaving the drums out.
    #[test]
    fn the_project_key_comes_from_its_clips() {
        let mut app = PulseApp::headless(crate::project::Project::empty());
        app.detect_key();
        assert_eq!(app.project.key, None, "nothing sounds");
        let mut clip = MidiClip::new("Acordes", 8.0, [0; 3]);
        // E minor, A minor, B minor, E minor (E G B, A C E, B D F#, E G B).
        for (bar, chord) in [[52u8, 55, 59], [45, 48, 52], [47, 50, 54], [52, 55, 59]].iter().enumerate() {
            for &pitch in chord {
                clip.notes.push(Note { start: bar as f64 * 2.0, length: 2.0, pitch, velocity: 0.8 });
            }
        }
        clip.notes.push(Note { start: 7.0, length: 1.0, pitch: 64, velocity: 0.8 });
        let synth = crate::project::Track::new("Synth", [0; 3], InstrumentKind::Synth(SynthParams::default()));
        app.project.tracks.push(synth);
        app.project.tracks.last_mut().unwrap().clips[0] = Some(Clip::Midi(clip));
        // A drum clip full of Cs and F#s must not count.
        let mut drums = crate::project::Track::new("Drums", [0; 3], InstrumentKind::DrumMachine(Default::default()));
        let hits = (0..32).map(|i| Note { start: i as f64 * 0.25, length: 0.25, pitch: if i % 2 == 0 { 36 } else { 42 }, velocity: 0.8 }).collect();
        drums.clips[0] = Some(Clip::Midi(MidiClip { notes: hits, ..MidiClip::new("Ritmo", 8.0, [0; 3]) }));
        app.project.tracks.push(drums);
        app.detect_key();
        assert_eq!(app.project.key.map(|k| k.name()), Some("Mi menor".into()), "{:?}", app.status);
    }

    /// With the project's key chosen, «Melodía» uses it rather than one it hears
    /// (and, as ever, notes sung in tune stay where they are).
    #[test]
    fn a_melody_goes_to_the_project_key() {
        let melody = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../stems/tests/data/melody.wav");
        let sample = engine::Sample::load(&melody).expect("test audio");
        let audio = AudioClip {
            name: "Melodía".into(),
            color: [0; 3],
            sample: String::new(),
            length: 8.0,
            offset: 0.0,
            warp: engine::model::WarpMode::Stretch,
            key: None,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(),
        };
        // C E G C, in tune: C# major is not what they sound in, but it is chosen.
        let c_sharp = stems::key::Key { tonic: 1, minor: false };
        let context = Context { audio: Vec::new(), midi: [0.0; 12], key: Some(c_sharp) };
        let (events, key) = convert(&sample, &audio, 120.0, ToMidi::Melody, &context, &Mutex::new(0.0)).unwrap();
        assert_eq!(key, Some(c_sharp));
        let pitches: Vec<u8> = events.iter().take(4).map(|e| e.pitch).collect();
        assert_eq!(pitches, vec![60, 64, 67, 72], "in tune notes stay, even off the scale");
    }
}
