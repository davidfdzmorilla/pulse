//! The UI-side project model. It is the source of truth; the engine receives copies.

use engine::model::{drum_note::*, InstrumentKind, MidiClip, Note, SynthParams, Waveform};
use engine::NUM_SCENES;

pub const PALETTE: [[u8; 3]; 8] = [
    [233, 108, 75],
    [242, 176, 72],
    [206, 214, 92],
    [111, 201, 125],
    [80, 190, 200],
    [96, 145, 230],
    [160, 115, 225],
    [225, 105, 170],
];

pub struct Track {
    pub name: String,
    pub color: [u8; 3],
    pub instrument: InstrumentKind,
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub clips: [Option<MidiClip>; NUM_SCENES],
}

impl Track {
    pub fn new(name: impl Into<String>, color: [u8; 3], instrument: InstrumentKind) -> Self {
        Self {
            name: name.into(),
            color,
            instrument,
            volume: 0.8,
            pan: 0.0,
            mute: false,
            clips: Default::default(),
        }
    }

    pub fn is_drums(&self) -> bool {
        matches!(self.instrument, InstrumentKind::Drums)
    }
}

pub struct Project {
    pub tempo: f64,
    pub metronome: bool,
    pub quantize: f64,
    pub master_volume: f32,
    pub tracks: Vec<Track>,
    pub scene_names: Vec<String>,
}

fn note(start: f64, length: f64, pitch: u8, velocity: f32) -> Note {
    Note {
        start,
        length,
        pitch,
        velocity,
    }
}

fn clip(name: &str, length: f64, color: [u8; 3], notes: Vec<Note>) -> Option<MidiClip> {
    Some(MidiClip {
        name: name.into(),
        length,
        notes,
        color,
    })
}

impl Project {
    /// A small A-minor demo set so the app makes sound right away.
    pub fn demo() -> Self {
        let [drums_color, bass_color, keys_color, lead_color] =
            [PALETTE[0], PALETTE[5], PALETTE[3], PALETTE[6]];

        let mut drums = Track::new("Batería", drums_color, InstrumentKind::Drums);
        let four_floor: Vec<Note> = (0..4)
            .flat_map(|beat| {
                let b = beat as f64;
                let mut notes = vec![
                    note(b, 0.25, KICK, 1.0),
                    note(b + 0.5, 0.25, CLOSED_HAT, 0.7),
                ];
                if beat % 2 == 1 {
                    notes.push(note(b, 0.25, CLAP, 0.9));
                }
                notes
            })
            .collect();
        let mut breakbeat: Vec<Note> = (0..16)
            .map(|step| note(step as f64 * 0.25, 0.25, CLOSED_HAT, if step % 2 == 0 { 0.6 } else { 0.35 }))
            .collect();
        breakbeat.extend([
            note(0.0, 0.25, KICK, 1.0),
            note(1.0, 0.25, SNARE, 0.9),
            note(1.75, 0.25, KICK, 0.8),
            note(2.5, 0.25, KICK, 0.9),
            note(3.0, 0.25, SNARE, 0.9),
            note(3.5, 0.25, OPEN_HAT, 0.5),
        ]);
        drums.clips[0] = clip("Four on the floor", 4.0, drums_color, four_floor);
        drums.clips[1] = clip("Breakbeat", 4.0, drums_color, breakbeat);

        let mut bass = Track::new(
            "Bajo",
            bass_color,
            InstrumentKind::Synth(SynthParams {
                waveform: Waveform::Saw,
                attack: 0.002,
                decay: 0.15,
                sustain: 0.5,
                release: 0.08,
                cutoff: 900.0,
                resonance: 0.35,
                gain: 0.6,
            }),
        );
        // Off-beat eighths following the progression Am - F - C - G.
        let roots = [33u8, 29, 36, 31];
        let bassline = roots
            .iter()
            .enumerate()
            .flat_map(|(bar, &root)| {
                (0..4).map(move |beat| note(bar as f64 * 4.0 + beat as f64 + 0.5, 0.4, root, 0.9))
            })
            .collect();
        bass.clips[0] = clip("Bajo offbeat", 16.0, bass_color, bassline);

        let mut keys = Track::new(
            "Acordes",
            keys_color,
            InstrumentKind::Synth(SynthParams {
                waveform: Waveform::Square,
                attack: 0.05,
                decay: 0.6,
                sustain: 0.4,
                release: 0.6,
                cutoff: 1800.0,
                resonance: 0.1,
                gain: 0.25,
            }),
        );
        let chords: [[u8; 3]; 4] = [[57, 60, 64], [53, 57, 60], [55, 60, 64], [55, 59, 62]];
        let chord_notes = chords
            .iter()
            .enumerate()
            .flat_map(|(bar, chord)| chord.iter().map(move |&p| note(bar as f64 * 4.0, 3.8, p, 0.7)))
            .collect();
        keys.clips[0] = clip("Am F C G", 16.0, keys_color, chord_notes);
        keys.clips[1] = keys.clips[0].clone();

        let mut lead = Track::new(
            "Lead",
            lead_color,
            InstrumentKind::Synth(SynthParams {
                waveform: Waveform::Saw,
                attack: 0.01,
                decay: 0.3,
                sustain: 0.6,
                release: 0.3,
                cutoff: 3500.0,
                resonance: 0.3,
                gain: 0.3,
            }),
        );
        let melody = [
            (0.0, 0.5, 76),
            (0.5, 0.5, 74),
            (1.0, 1.0, 72),
            (2.0, 0.5, 69),
            (2.5, 1.5, 72),
            (4.0, 0.5, 74),
            (4.5, 0.5, 72),
            (5.0, 1.0, 71),
            (6.0, 2.0, 67),
        ]
        .into_iter()
        .map(|(s, l, p)| note(s, l, p, 0.8))
        .collect();
        lead.clips[1] = clip("Melodía", 8.0, lead_color, melody);

        let mut tracks = vec![drums, bass, keys, lead];
        for (i, extra) in ["Synth 5", "Synth 6"].into_iter().enumerate() {
            tracks.push(Track::new(
                extra,
                PALETTE[(i + 1) % PALETTE.len()],
                InstrumentKind::Synth(SynthParams::default()),
            ));
        }

        Self {
            tempo: 120.0,
            metronome: false,
            quantize: 4.0,
            master_volume: 0.8,
            tracks,
            scene_names: (1..=NUM_SCENES)
                .map(|i| match i {
                    1 => "Intro".to_string(),
                    2 => "Tema".to_string(),
                    n => format!("Escena {n}"),
                })
                .collect(),
        }
    }
}

pub fn note_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[pitch as usize % 12], pitch as i32 / 12 - 1)
}

pub fn is_black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

pub fn drum_name(pitch: u8) -> Option<&'static str> {
    Some(match pitch {
        KICK => "Bombo",
        SNARE => "Caja",
        CLAP => "Palmas",
        CLOSED_HAT => "Hat cerrado",
        OPEN_HAT => "Hat abierto",
        _ => return None,
    })
}
