//! The UI-side project model. It is the source of truth; the engine receives copies.

use engine::model::{
    drum_note::*, AcidParams, ArrangementClip, AutoFilterParams, AutomationLane, AutomationPoint, Clip, FmParams, SuperSawParams, CompressorParams, DelayParams, Device, Effect,
    InstrumentKind, MidiClip, Note, Param, PluginId, ReverbParams, SynthParams, RACK_FIRST_NOTE, RACK_PADS,
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
    /// A bus for the tracks under it (see `groups`): no clips, no instrument.
    Group,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// When an audio track plays its input (instead of its clips), as in Live.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Monitor {
    /// While armed.
    #[default]
    Auto,
    /// Always.
    In,
    Off,
}

impl Monitor {
    pub const ALL: [Monitor; 3] = [Monitor::In, Monitor::Auto, Monitor::Off];

    pub fn label(self) -> &'static str {
        match self {
            Monitor::Auto => "Auto",
            Monitor::In => "In",
            Monitor::Off => "Off",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Monitor::Auto => "Monitorizar en Auto: oyes la entrada de audio mientras la pista está armada",
            Monitor::In => "Monitorizar siempre: oyes la entrada de audio en vez de los clips",
            Monitor::Off => "Sin monitorizar: no oyes la entrada (usa la del micrófono o la interfaz)",
        }
    }
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
    /// Armed tracks receive live input and record.
    #[serde(default)]
    pub armed: bool,
    /// When an audio track plays its audio input.
    #[serde(default, skip_serializing_if = "is_default")]
    pub monitor: Monitor,
    pub sends: [f32; NUM_RETURNS],
    pub devices: Vec<Device>,
    pub clips: [Option<Clip>; NUM_SCENES],
    pub arrangement: Vec<ArrangementClip>,
    #[serde(default)]
    pub automation: Vec<AutomationLane>,
    /// Audio files played by the sampler (slot 0) or the drum rack (one per pad).
    /// Empty strings are empty pads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<String>,
    /// Its instrument and effects rendered to audio, which it plays instead (see `freeze`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen: Option<crate::freeze::Frozen>,
    /// How many groups it is inside (see `groups`): the tracks of a group are
    /// the ones right under it, one level deeper.
    #[serde(default, skip_serializing_if = "is_default")]
    pub depth: u8,
    /// A group with its tracks hidden.
    #[serde(default, skip_serializing_if = "is_default")]
    pub folded: bool,
    /// Its own number in the project, which stays when tracks move: what a
    /// sidechain names its source by (0 until it is given one, see `assign_ids`).
    #[serde(default, skip_serializing_if = "is_default")]
    pub id: u32,
    /// What its notes go through before its instrument (see `midi_fx_ui`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub midi_effects: Vec<engine::model::MidiDevice>,
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
            armed: false,
            monitor: Monitor::Auto,
            sends: [0.0; NUM_RETURNS],
            devices: Vec::new(),
            clips: Default::default(),
            arrangement: Vec::new(),
            automation: Vec::new(),
            samples: Vec::new(),
            frozen: None,
            depth: 0,
            folded: false,
            id: 0,
            midi_effects: Vec::new(),
        }
    }

    pub fn new_group(name: impl Into<String>, color: [u8; 3]) -> Self {
        Self {
            kind: TrackKind::Group,
            ..Self::new(name, color, InstrumentKind::Synth(SynthParams::default()))
        }
    }

    pub fn is_group(&self) -> bool {
        self.kind == TrackKind::Group
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

    /// Whether it plays the audio input now.
    pub fn monitors(&self) -> bool {
        self.is_audio()
            && match self.monitor {
                Monitor::In => true,
                Monitor::Auto => self.armed,
                Monitor::Off => false,
            }
    }

    pub fn is_drums(&self) -> bool {
        self.instrument.is_drums()
    }

    /// The file in sample slot `slot`, if any.
    pub fn sample(&self, slot: usize) -> Option<&str> {
        self.samples.get(slot).map(String::as_str).filter(|p| !p.is_empty())
    }

    pub fn set_sample(&mut self, slot: usize, path: Option<String>) {
        if self.samples.len() <= slot {
            self.samples.resize(slot + 1, String::new());
        }
        self.samples[slot] = path.unwrap_or_default();
        while self.samples.last().is_some_and(String::is_empty) {
            self.samples.pop();
        }
    }

    /// Display name of sample slot `slot`: the file name without extension.
    pub fn sample_name(&self, slot: usize) -> Option<String> {
        let path = Path::new(self.sample(slot)?);
        Some(path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned()))
    }

    /// Piano roll rows for drum instruments, top to bottom: (note, name).
    pub fn drum_rows(&self) -> Vec<(u8, String)> {
        match self.instrument {
            InstrumentKind::DrumRack(_) => (0..RACK_PADS)
                .rev()
                .map(|pad| {
                    let pitch = RACK_FIRST_NOTE + pad as u8;
                    (pitch, self.sample_name(pad).unwrap_or_else(|| note_name(pitch)))
                })
                .collect(),
            _ => engine::model::DRUM_VOICES.iter().rev().map(|&(pitch, name)| (pitch, name.to_string())).collect(),
        }
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
    #[serde(default)]
    pub automation: Vec<AutomationLane>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MasterTrack {
    pub mixer: Mixer,
    pub devices: Vec<Device>,
    #[serde(default)]
    pub automation: Vec<AutomationLane>,
}

/// The project file format. Raise it when a change needs old files converted
/// (in [`migrate`]), or when an older Pulse opening a new file would get its
/// sound wrong (it drops fields it doesn't know): it then refuses the file
/// instead. Fields added with a default that an older Pulse can miss without
/// sounding different don't need it; for those, `saved_by` makes it ask before
/// saving over the file.
pub const FORMAT: u32 = 1;
const FORMAT_KEY: &str = "format";
/// The Pulse that saved a file (see [`Project::saved_by`]).
const SAVED_BY_KEY: &str = "saved_by";

/// Converts a project file from `format` to the current one.
fn migrate(_value: &mut serde_json::Value, format: u64) {
    // Format 0 (no number) and 1 are the same: the number was added in 1.
    if format < 1 {}
}

/// Which Pulse saved a project's file. Not its content: projects that differ
/// only here are equal (an unchanged project isn't "unsaved").
#[derive(Clone, Debug, Default)]
pub struct SavedBy(pub Option<String>);

impl PartialEq for SavedBy {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub tempo: f64,
    pub metronome: bool,
    pub quantize: f64,
    pub loop_enabled: bool,
    pub loop_start: f64,
    pub loop_end: f64,
    /// Swing applied to MIDI notes on playback, 0..1 (1 = triplet shuffle).
    #[serde(default)]
    pub swing: f32,
    /// Grid the swing acts on, in beats (0.5 = eighths, 0.25 = sixteenths).
    #[serde(default = "default_swing_grid")]
    pub swing_grid: f64,
    /// Grid recorded MIDI note starts snap to, if any.
    #[serde(default)]
    pub record_quantize: Option<f64>,
    /// Punch in / out: recording onto the arrangement only from the loop's start
    /// and/or up to its end (whether the loop plays or not), as in Live.
    #[serde(default, skip_serializing_if = "is_default")]
    pub punch_in: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub punch_out: bool,
    /// The song's key, chosen or detected: the piano roll shows its scale and
    /// Audio a MIDI fits melodies to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<stems::key::Key>,
    pub tracks: Vec<Track>,
    pub returns: Vec<ReturnTrack>,
    pub master: MasterTrack,
    pub scene_names: Vec<String>,
    /// The Pulse that saved the file it came from, if it said (written from
    /// this version on; see `PulseApp::save_to`).
    #[serde(skip)]
    pub saved_by: SavedBy,
    /// The VST3 plugins the instruments and devices refer to by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<PluginRef>,
    /// Named places on the Arrangement: the song's parts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<SongMarker>,
    /// MIDI controllers linked to the project's controls (see `midi_map`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub midi_map: Vec<crate::midi_map::Mapping>,
    /// Knobs that move several controls at once (see `macros`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macros: Vec<crate::macros::Macro>,
}

/// A marker on the Arrangement: where (beat) and its name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SongMarker {
    pub beat: f64,
    pub name: String,
}

/// A plugin instance: which plugin, and its saved state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginRef {
    pub id: PluginId,
    /// The `.vst3` bundle.
    pub path: String,
    /// The class id (32 hex digits).
    pub cid: String,
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    /// The plugin's state, in base64, as of the last save (see `PulseApp::capture_plugins`).
    #[serde(default)]
    pub state: String,
}

/// The state is left out: it changes inside the plugin and is copied in only to
/// save, so it must not count as an edit (or undo would undo the plugin's sound).
impl PartialEq for PluginRef {
    fn eq(&self, other: &Self) -> bool {
        (self.id, &self.path, &self.cid, &self.name, &self.vendor) == (other.id, &other.path, &other.cid, &other.name, &other.vendor)
    }
}

impl Project {
    /// Gives each track without one (or with another's: a copy) an id of its own.
    pub fn assign_ids(&mut self) {
        let mut next = self.tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        let mut seen = std::collections::HashSet::new();
        for track in &mut self.tracks {
            if track.id == 0 || !seen.insert(track.id) {
                track.id = next;
                seen.insert(next);
                next += 1;
            }
        }
    }

    pub fn plugin(&self, id: PluginId) -> Option<&PluginRef> {
        self.plugins.iter().find(|p| p.id == id)
    }

    /// An id no plugin of the project uses yet.
    pub fn new_plugin_id(&self) -> PluginId {
        PluginId(self.plugins.iter().map(|p| p.id.0 + 1).max().unwrap_or(1))
    }
}

fn default_swing_grid() -> f64 {
    0.25
}

impl Project {
    /// Where recording onto the arrangement may start and must stop (punch in
    /// and out, at the loop's edges), if they are on.
    pub fn punch(&self) -> (Option<f64>, Option<f64>) {
        (self.punch_in.then_some(self.loop_start), self.punch_out.then_some(self.loop_end))
    }

    pub fn empty() -> Self {
        Self {
            saved_by: SavedBy::default(),
            tempo: 120.0,
            metronome: false,
            quantize: 4.0,
            loop_enabled: false,
            loop_start: 0.0,
            loop_end: 16.0,
            swing: 0.0,
            swing_grid: default_swing_grid(),
            record_quantize: None,
            punch_in: false,
            punch_out: false,
            key: None,
            markers: Vec::new(),
            macros: Vec::new(),
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
                automation: Vec::new(),
            },
            scene_names: (1..=NUM_SCENES).map(|i| i.to_string()).collect(),
            plugins: Vec::new(),
            midi_map: Vec::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("No se pudo leer: {e}"))?;
        let mut project = Self::from_json(&text)?;
        // Files in its folder are saved relative to it (it can be moved).
        crate::library::resolve(&mut project, path.parent().unwrap_or(Path::new(".")));
        Ok(project)
    }

    /// Reads a saved project, bringing files from older versions of Pulse up to date.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut value: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("Archivo no válido: {e}"))?;
        // Files from before the format number are format 0.
        let format = value.get(FORMAT_KEY).and_then(|f| f.as_u64()).unwrap_or(0);
        if format > FORMAT as u64 {
            return Err("Este proyecto se guardó con una versión más nueva de Pulse. Actualiza Pulse para abrirlo.".into());
        }
        let saved_by = value.get(SAVED_BY_KEY).and_then(|v| v.as_str()).map(str::to_string);
        if let Some(object) = value.as_object_mut() {
            object.remove(FORMAT_KEY);
            object.remove(SAVED_BY_KEY);
        }
        migrate(&mut value, format);
        let mut project: Project = serde_json::from_value(value).map_err(|e| format!("Archivo no válido: {e}"))?;
        for track in &mut project.tracks {
            track.instrument = track.instrument.upgraded();
        }
        // A tempo of 0 (an invalid number saved as 0) would stop time.
        if !(project.tempo.is_finite() && project.tempo > 0.0) {
            project.tempo = 120.0;
        }
        project.saved_by = SavedBy(saved_by);
        Ok(project)
    }

    pub fn to_json(&self) -> Result<String, String> {
        self.to_json_repaired().map(|(text, _)| text)
    }

    /// The project's JSON, and how many numbers in it weren't valid (NaN or
    /// infinite: they are written as 0, see `finite`). What it writes always
    /// reads back.
    pub fn to_json_repaired(&self) -> Result<(String, usize), String> {
        let (mut value, repaired) = crate::finite::repair(self)?;
        if let Some(object) = value.as_object_mut() {
            object.insert(FORMAT_KEY.into(), FORMAT.into());
            object.insert(SAVED_BY_KEY.into(), crate::updates::VERSION.into());
        }
        let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
        if repaired > 0 {
            // Never write what wouldn't open again.
            Self::from_json(&text).map_err(|e| format!("El proyecto tiene valores no válidos y no se pudo corregir: {e}"))?;
        }
        Ok((text, repaired))
    }

    /// Saves all at once (see `store`), so a crash or a full disk never leaves
    /// half a project. Gives how many invalid numbers were saved as 0.
    pub fn save(&self, path: &Path) -> Result<usize, String> {
        let dir = path.parent().unwrap_or(Path::new("."));
        let (text, repaired) = crate::library::relative(self, dir).to_json_repaired()?;
        crate::store::write_atomic(path, text.as_bytes())?;
        Ok(repaired)
    }

    /// A small A-minor demo set so the app makes sound right away.
    pub fn demo() -> Self {
        let [drums_color, bass_color, keys_color, lead_color] = [PALETTE[1], PALETTE[9], PALETTE[6], PALETTE[11]];

        let mut drums = Track::new("Drums", drums_color, InstrumentKind::DrumMachine(Default::default()));
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
            makeup_db: 1.5,
            sidechain: None,
        })));

        let mut bass = Track::new("Bass", bass_color, InstrumentKind::Acid(AcidParams::default()));
        // An acid line over Am - F - C - G: accents (velocity 1.0) and one slide per bar,
        // where a long note overlaps the next.
        let roots = [45u8, 41, 48, 43];
        let steps: [(f64, f64, u8, f32); 7] = [
            (0.5, 0.2, 0, 0.9),
            (1.0, 0.2, 12, 0.6),
            (1.5, 0.2, 0, 1.0),
            (2.5, 0.2, 0, 0.7),
            (2.75, 0.2, 12, 0.7),
            (3.0, 0.6, 12, 1.0),
            (3.5, 0.3, 7, 0.8),
        ];
        let bassline = roots
            .iter()
            .enumerate()
            .flat_map(|(bar, &root)| {
                steps.iter().map(move |&(at, length, interval, velocity)| note(bar as f64 * 4.0 + at, length, root + interval, velocity))
            })
            .collect();
        bass.clips[0] = clip("Acid Line", 16.0, bass_color, bassline);

        let mut keys = Track::new(
            "Chords",
            keys_color,
            InstrumentKind::SuperSaw(SuperSawParams {
                detune: 0.3,
                spread: 0.9,
                cutoff: 3000.0,
                decay: 0.5,
                sustain: 0.8,
                release: 0.4,
                gain: 0.3,
                ..Default::default()
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
        // The house "pump": the chords duck on every beat.
        keys.devices.push(Device::new(Effect::Ducker(Default::default())));

        let mut lead = Track::new(
            "Lead",
            lead_color,
            // A bell: inharmonic ratio, bright attack that fades.
            InstrumentKind::Fm(FmParams {
                ratio: 3.5,
                index: 4.0,
                mod_decay: 0.4,
                mod_sustain: 0.1,
                feedback: 0.1,
                decay: 1.5,
                release: 0.8,
                gain: 0.2,
                ..Default::default()
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

        // The chords' filter opens over the eight bars of the arrangement.
        keys.automation.push(AutomationLane {
            param: Param::Device { device: 0, param: 0 },
            points: vec![
                AutomationPoint { beat: 0.0, value: 300.0, curve: 0.0 },
                AutomationPoint { beat: 28.0, value: 6000.0, curve: 0.0 },
                AutomationPoint { beat: 32.0, value: 1200.0, curve: 0.0 },
            ],
        });

        let mut project = Self::empty();
        project.tracks = vec![drums, bass, keys, lead];
        project.master.devices.push(Device::new(Effect::Limiter(Default::default())));
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
            automation: Vec::new(),
        },
        ReturnTrack {
            name: "B Delay".into(),
            color: PALETTE[3],
            mixer: Mixer::default(),
            devices: vec![Device::new(Effect::Delay(DelayParams {
                mix: 1.0,
                ..Default::default()
            }))],
            automation: Vec::new(),
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
        groove: None,
        envelopes: Vec::new(),
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_tracks_monitor_as_their_mode_says() {
        let mut audio = Track::new_audio("Voz", [0, 0, 0]);
        assert_eq!(audio.monitor, Monitor::Auto);
        assert!(!audio.monitors(), "Auto: only while armed");
        audio.armed = true;
        assert!(audio.monitors());
        audio.monitor = Monitor::Off;
        assert!(!audio.monitors());
        audio.armed = false;
        audio.monitor = Monitor::In;
        assert!(audio.monitors(), "In: always");
        let mut midi = Track::new("Synth", [0, 0, 0], InstrumentKind::Synth(SynthParams::default()));
        midi.armed = true;
        midi.monitor = Monitor::In;
        assert!(!midi.monitors(), "MIDI tracks have no audio input");

        // Saved only when it is not the default; older projects load as Auto.
        let json = serde_json::to_string(&Track::new_audio("Voz", [0, 0, 0])).unwrap();
        assert!(!json.contains("monitor"));
        let loaded: Track = serde_json::from_str(&serde_json::to_string(&audio).unwrap()).unwrap();
        assert_eq!(loaded.monitor, Monitor::In);
    }

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

    /// Projects saved by earlier versions, kept in the repository: every new
    /// version must still open them. Add one each time the format changes.
    const FIXTURES: [(&str, &str); 2] = [
        ("format 0 (antes del número de formato)", include_str!("../fixtures/format0-demo.pulse")),
        ("format 1", include_str!("../fixtures/format1-demo.pulse")),
    ];

    #[test]
    fn projects_from_every_earlier_format_still_open() {
        for (name, text) in FIXTURES {
            let project = Project::from_json(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(project.tracks.len(), 4, "{name}");
            assert!(project.tracks.iter().any(|t| !t.arrangement.is_empty() || t.clips.iter().any(|c| c.is_some())), "{name}");
            // And they save in the current format.
            assert!(project.to_json().unwrap().contains(&format!("\"format\": {FORMAT}")));
        }
    }

    #[test]
    fn projects_from_a_newer_pulse_are_refused_with_a_clear_message() {
        let mut value = serde_json::to_value(Project::demo()).unwrap();
        value["format"] = (FORMAT + 1).into();
        let error = Project::from_json(&value.to_string()).unwrap_err();
        assert!(error.contains("versión más nueva"), "{error}");
    }

    /// Writes the fixture for the current format (run once when it changes).
    #[test]
    #[ignore = "writes crates/app/fixtures/format<N>-demo.pulse"]
    fn write_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/format{FORMAT}-demo.pulse"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, Project::demo().to_json().unwrap()).unwrap();
    }

    #[test]
    fn sample_slots_round_trip_and_trim_empty_pads() {
        let mut track = Track::new("Rack", PALETTE[0], InstrumentKind::DrumRack(Default::default()));
        track.set_sample(2, Some("/a/snare.wav".into()));
        assert_eq!(track.samples, vec!["", "", "/a/snare.wav"]);
        assert_eq!(track.sample_name(2).as_deref(), Some("snare"));
        let rows = track.drum_rows();
        assert_eq!(rows.len(), RACK_PADS);
        assert_eq!(rows[RACK_PADS - 3], (38, "snare".to_string()), "pads go up from the bottom row");
        let json = serde_json::to_string(&track).unwrap();
        assert_eq!(serde_json::from_str::<Track>(&json).unwrap(), track);
        track.set_sample(2, None);
        assert!(track.samples.is_empty());
        // Tracks saved before samples existed still load.
        let old = serde_json::to_string(&Track::new("Synth", PALETTE[0], InstrumentKind::Synth(Default::default()))).unwrap();
        assert!(!old.contains("samples"));
    }

    /// A NaN or an infinity anywhere (JSON writes them as null) never leaves a
    /// file that doesn't open: they are saved as 0, and saving says how many.
    #[test]
    fn invalid_numbers_are_saved_as_zero_and_the_file_opens() {
        let dir = std::env::temp_dir().join(format!("pulse-nan-{}", std::process::id()));
        let file = dir.join("T/T.pulse");
        let mut project = Project::demo();
        project.tempo = f64::INFINITY;
        project.tracks[0].mixer.gain = f32::NAN;
        project.tracks[2].automation[0].points[0].value = f32::NEG_INFINITY;
        assert_eq!(project.save(&file).unwrap(), 3);
        let loaded = Project::load(&file).expect("it opens");
        assert_eq!(loaded.tempo, 120.0, "a tempo of 0 would stop time");
        assert_eq!(loaded.tracks[0].mixer.gain, 0.0);
        assert_eq!(loaded.tracks[2].automation[0].points[0].value, 0.0);
        assert_eq!(Project::demo().save(&file).unwrap(), 0);
        // From the app: it says so, and what plays is what was saved.
        let mut app = crate::app::PulseApp::headless(Project::demo());
        app.project.tracks[1].mixer.pan = f32::NAN;
        app.save_to_path(&file);
        assert!(app.status.as_deref().unwrap().contains("1 valor(es) no válidos"), "{:?}", app.status);
        assert_eq!(app.project.tracks[1].mixer.pan, 0.0);
        assert!(!app.is_dirty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A project says which Pulse saved it; one from a newer Pulse warns on
    /// opening and asks before being saved over (it would lose what this one
    /// doesn't know).
    #[test]
    fn a_project_from_a_newer_pulse_is_not_saved_over_without_asking() {
        let dir = std::env::temp_dir().join(format!("pulse-newer-{}", std::process::id()));
        let file = dir.join("N/N.pulse");
        Project::demo().save(&file).unwrap();
        assert_eq!(Project::load(&file).unwrap().saved_by.0.as_deref(), Some(crate::updates::VERSION));
        let mut json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        json["saved_by"] = "99.0.0".into();
        json["a_future_field"] = 1.into();
        std::fs::write(&file, json.to_string()).unwrap();

        let mut app = crate::app::PulseApp::headless(Project::empty());
        app.open_from(&file);
        assert!(app.status.as_deref().unwrap().contains("lo guardó Pulse 99.0.0, más nuevo"), "{:?}", app.status);
        app.answers.push_back(crate::commands::Answer::Cancel);
        app.save_to_path(&file);
        assert!(std::fs::read_to_string(&file).unwrap().contains("a_future_field"), "not saved over");
        app.answers.push_back(crate::commands::Answer::Save);
        app.save_to_path(&file);
        let saved = Project::load(&file).unwrap();
        assert_eq!(saved.saved_by.0.as_deref(), Some(crate::updates::VERSION), "saved, by this one now");
        // Saved by this Pulse or an older one: no question.
        let mut again = crate::app::PulseApp::headless(Project::empty());
        again.open_from(&file);
        again.save_to_path(&file);
        assert!(again.status.as_deref().unwrap().starts_with("Guardado"), "{:?}", again.status);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
