//! The UI-side project model. It is the source of truth; the engine receives copies.

use engine::model::{
    drum_note::*, ArrangementClip, AutoFilterParams, Clip, CompressorParams, DelayParams, Device, Effect,
    Eq3Params, InstrumentKind, MidiClip, Note, ReverbParams, SynthParams, Waveform,
};
use engine::{NUM_RETURNS, NUM_SCENES};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const PALETTE: [[u8; 3]; 12] = [
    [255, 148, 166],
    [255, 165, 41],
    [204, 153, 39],
    [247, 244, 124],
    [191, 251, 0],
    [26, 255, 47],
    [37, 255, 168],
    [92, 255, 232],
    [139, 197, 255],
    [84, 128, 228],
    [146, 167, 255],
    [216, 108, 228],
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mixer {
    /// Linear gain; 1.0 is 0 dB.
    pub gain: f32,
    pub pan: f32,
    /// Track activator; off mutes the channel.
    pub active: bool,
}

impl Default for Mixer {
    fn default() -> Self {
        Self {
            gain: 0.7,
            pan: 0.0,
            active: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    #[default]
    Midi,
    Audio,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Defaults to MIDI so projects saved before audio tracks existed still load.
    #[serde(default)]
    pub kind: TrackKind,
    pub name: String,
    pub color: [u8; 3],
    pub instrument: InstrumentKind,
    pub mixer: Mixer,
    pub solo: bool,
    pub sends: [f32; NUM_RETURNS],
    pub devices: Vec<Device>,
    pub clips: [Option<Clip>; NUM_SCENES],
    pub arrangement: Vec<ArrangementClip>,
}

impl Track {
    pub fn new(name: impl Into<String>, color: [u8; 3], instrument: InstrumentKind) -> Self {
        Self {
            kind: TrackKind::Midi,
            name: name.into(),
            color,
            instrument,
            mixer: Mixer::default(),
            solo: false,
            sends: [0.0; NUM_RETURNS],
            devices: Vec::new(),
            clips: Default::default(),
            arrangement: Vec::new(),
        }
    }

    pub fn new_audio(name: impl Into<String>, color: [u8; 3]) -> Self {
        Self {
            kind: TrackKind::Audio,
            ..Self::new(name, color, InstrumentKind::Synth(SynthParams::default()))
        }
    }

    pub fn is_audio(&self) -> bool {
        self.kind == TrackKind::Audio
    }

    pub fn is_drums(&self) -> bool {
        matches!(self.instrument, InstrumentKind::Drums)
    }

    /// Beat where the last arrangement clip ends.
    pub fn arrangement_end(&self) -> f64 {
        self.arrangement.iter().map(ArrangementClip::end).fold(0.0, f64::max)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReturnTrack {
    pub name: String,
    pub color: [u8; 3],
    pub mixer: Mixer,
    pub devices: Vec<Device>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MasterTrack {
    pub mixer: Mixer,
    pub devices: Vec<Device>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub tempo: f64,
    pub metronome: bool,
    pub quantize: f64,
    pub loop_enabled: bool,
    pub loop_start: f64,
    pub loop_end: f64,
    pub tracks: Vec<Track>,
    pub returns: Vec<ReturnTrack>,
    pub master: MasterTrack,
    pub scene_names: Vec<String>,
}

impl Project {
    pub fn empty() -> Self {
        Self {
            tempo: 120.0,
            metronome: false,
            quantize: 4.0,
            loop_enabled: false,
            loop_start: 0.0,
            loop_end: 16.0,
            tracks: (0..2)
                .map(|i| {
                    Track::new(
                        format!("{} MIDI", i + 1),
                        PALETTE[(i * 5) % PALETTE.len()],
                        InstrumentKind::Synth(SynthParams::default()),
                    )
                })
                .collect(),
            returns: default_returns(),
            master: MasterTrack {
                mixer: Mixer {
                    gain: 1.0,
                    ..Default::default()
                },
                devices: Vec::new(),
            },
            scene_names: (1..=NUM_SCENES).map(|i| i.to_string()).collect(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("No se pudo leer: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("Archivo no válido: {e}"))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("No se pudo guardar: {e}"))
    }

    /// A small A-minor demo set so the app makes sound right away.
    pub fn demo() -> Self {
        let [drums_color, bass_color, keys_color, lead_color] = [PALETTE[1], PALETTE[9], PALETTE[6], PALETTE[11]];

        let mut drums = Track::new("Drums", drums_color, InstrumentKind::Drums);
        let four_floor: Vec<Note> = (0..4)
            .flat_map(|beat| {
                let b = beat as f64;
                let mut notes = vec![note(b, 0.25, KICK, 1.0), note(b + 0.5, 0.25, CLOSED_HAT, 0.7)];
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
        drums.devices.push(Device::new(Effect::Compressor(CompressorParams {
            threshold_db: -14.0,
            ratio: 3.0,
            attack_ms: 8.0,
            release_ms: 90.0,
            makeup_db: 3.0,
        })));

        let mut bass = Track::new(
            "Bass",
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
            .flat_map(|(bar, &root)| (0..4).map(move |beat| note(bar as f64 * 4.0 + beat as f64 + 0.5, 0.4, root, 0.9)))
            .collect();
        bass.clips[0] = clip("Offbeat Bass", 16.0, bass_color, bassline);
        bass.devices.push(Device::new(Effect::Eq3(Eq3Params {
            low_db: 2.0,
            high_db: -6.0,
            ..Default::default()
        })));

        let mut keys = Track::new(
            "Chords",
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
        keys.sends = [0.35, 0.0];
        keys.devices.push(Device::new(Effect::AutoFilter(AutoFilterParams {
            cutoff: 1200.0,
            resonance: 0.4,
            lfo_beats: 8.0,
            lfo_amount: 1.5,
            ..Default::default()
        })));

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
        lead.clips[1] = clip("Melody", 8.0, lead_color, melody);
        lead.sends = [0.3, 0.25];

        // A short arrangement built from the session clips.
        let place = |track: &Track, slot: usize, start: f64, length: f64| ArrangementClip {
            start,
            length,
            clip: track.clips[slot].clone().expect("demo clip"),
        };
        drums.arrangement = vec![place(&drums, 0, 0.0, 16.0), place(&drums, 1, 16.0, 16.0)];
        bass.arrangement = vec![place(&bass, 0, 16.0, 16.0)];
        keys.arrangement = vec![place(&keys, 0, 0.0, 32.0)];
        lead.arrangement = vec![place(&lead, 1, 16.0, 16.0)];

        let mut project = Self::empty();
        project.tracks = vec![drums, bass, keys, lead];
        project.loop_end = 32.0;
        project.scene_names = (1..=NUM_SCENES)
            .map(|i| match i {
                1 => "Intro".to_string(),
                2 => "Drop".to_string(),
                n => n.to_string(),
            })
            .collect();
        project
    }
}

fn default_returns() -> Vec<ReturnTrack> {
    vec![
        ReturnTrack {
            name: "A Reverb".into(),
            color: PALETTE[8],
            mixer: Mixer::default(),
            devices: vec![Device::new(Effect::Reverb(ReverbParams {
                mix: 1.0,
                ..Default::default()
            }))],
        },
        ReturnTrack {
            name: "B Delay".into(),
            color: PALETTE[3],
            mixer: Mixer::default(),
            devices: vec![Device::new(Effect::Delay(DelayParams {
                mix: 1.0,
                ..Default::default()
            }))],
        },
    ]
}

fn note(start: f64, length: f64, pitch: u8, velocity: f32) -> Note {
    Note {
        start,
        length,
        pitch,
        velocity,
    }
}

fn clip(name: &str, length: f64, color: [u8; 3], notes: Vec<Note>) -> Option<Clip> {
    Some(Clip::Midi(MidiClip {
        name: name.into(),
        length,
        notes,
        color,
    }))
}

pub fn note_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[pitch as usize % 12], pitch as i32 / 12 - 2)
}

pub fn is_black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

pub fn drum_name(pitch: u8) -> Option<&'static str> {
    Some(match pitch {
        KICK => "Kick",
        SNARE => "Snare",
        CLAP => "Clap",
        CLOSED_HAT => "Closed Hat",
        OPEN_HAT => "Open Hat",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_round_trip_through_json() {
        let project = Project::demo();
        let dir = std::env::temp_dir().join(format!("pulse-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("demo.pulse");
        project.save(&path).unwrap();
        let loaded = Project::load(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(project, loaded);
    }
}
