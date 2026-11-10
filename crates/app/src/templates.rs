//! Templates: projects to start new ones from. One is a project without its
//! content (clips, the arrangement, automation and markers): its tracks,
//! instruments, effects, samples, mix, returns and tempo. Pulse brings its
//! own (a beat, a voice recording, and one a genre: house, techno, trap,
//! reggaetón, pop, lo-fi, drum & bass, a podcast); the user's are saved in
//! `Plantillas/<name>/<name>.pulse` with their samples, as projects are, and one
//! of them (or Pulse's) can be what ⌘N starts from (Settings).

use crate::app::PulseApp;
use crate::example::{effect, instrument};
use crate::i18n::{fill, tr};
use crate::project::{Project, Track, PALETTE};
use engine::model::{AcidParams, BitcrusherParams, CompressorParams, DeEsserParams, DelayParams, Device, DrumModel, DrumParams, DuckerParams, Effect, Eq3Params, FmParams, GateParams, InstrumentKind, KeysParams, KickParams, LimiterParams, SaturatorParams, SuperSawParams, SynthParams, UtilityParams, WaveParams};
use std::path::PathBuf;

/// The empty project's name, for the choice of what ⌘N starts from.
pub const EMPTY: &str = "Vacío";

/// Pulse's own templates.
pub const BUILT_IN: [&str; 10] = ["Beat", "Grabar voz", "House", "Techno", "Trap", "Reggaetón", "Pop", "Lo-Fi", "Drum & Bass", "Podcast"];

/// What one of Pulse's templates is for, in a line.
pub fn about(name: &str) -> &'static str {
    match name {
        "Beat" => "Batería 909, bajo ácido, acordes y melodía, a 124 BPM",
        "Grabar voz" => "Una pista para la base y dos para voz y coros, listas para grabar, a 90 BPM",
        "House" => "Kit house, bajo, órgano y pad que bombean, pluck y sala, a 124 BPM",
        "Techno" => "Kit techno, bajo rodante, acid, stab y atmósfera oscura, a 130 BPM",
        "Trap" => "Kit trap, 808 que sigue las notas, campanas, pad y voz, a 140 BPM",
        "Reggaetón" => "Dembow, bajo sub, marimba, lead y voz, a 95 BPM",
        "Pop" => "Batería, bajo, piano, pad ancho, voz y coros, a 110 BPM",
        "Lo-Fi" => "Kit polvoriento con swing, piano lo-fi, bajo cálido y cinta en el master, a 85 BPM",
        "Drum & Bass" => "Kit rodante, reese, sub y pad, a 174 BPM",
        "Podcast" => "Dos voces con puerta, ecualizador y de-esser, y música que se aparta al hablar",
        _ => "",
    }
}

/// A template's name as menus show it: Pulse's own in the interface's
/// language, the user's as they named them. `name` stays the key.
pub fn shown(name: &str) -> &str {
    BUILT_IN.iter().chain([&EMPTY]).find(|n| **n == name).map_or(name, |n| crate::i18n::tr(n))
}

/// A template: one of Pulse's, or a file of the user's.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub name: String,
    pub file: Option<PathBuf>,
}

impl Template {
    /// Its name as menus show it (see [`shown`]).
    pub fn shown(&self) -> &str {
        if self.file.is_some() { &self.name } else { shown(&self.name) }
    }
}

/// Every template, Pulse's first, then the user's by name.
pub fn all() -> Vec<Template> {
    let mut list: Vec<Template> = BUILT_IN.iter().map(|n| Template { name: n.to_string(), file: None }).collect();
    list.extend(saved());
    list
}

/// The user's templates: the projects in `Plantillas`, each in its folder.
fn saved() -> Vec<Template> {
    let folder = crate::library::root().join(crate::library::TEMPLATES);
    let mut found: Vec<Template> = std::fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let file = entry.path().join(format!("{name}.pulse"));
            file.is_file().then_some(Template { name, file: Some(file) })
        })
        .collect();
    found.sort_by_key(|t| t.name.to_lowercase());
    found
}

/// The template called `name`: the user's if one has it, else Pulse's.
pub fn find(templates: &[Template], name: &str) -> Option<Template> {
    let named = |t: &&Template| t.name == name;
    templates.iter().filter(|t| t.file.is_some()).find(named).or_else(|| templates.iter().find(named)).cloned()
}

/// A new project from `template`.
pub fn project(template: &Template) -> Result<Project, String> {
    match &template.file {
        Some(file) => Project::load(file).map(|p| without_content(&p)),
        None => built_in(&template.name),
    }
}

/// `project` with nothing played in it: no clips, arrangement, automation,
/// markers or frozen audio, and no track armed. The rest stays.
pub fn without_content(project: &Project) -> Project {
    let mut project = project.clone();
    for track in &mut project.tracks {
        track.clips = Default::default();
        track.arrangement.clear();
        track.automation.clear();
        track.frozen = None;
        track.armed = false;
    }
    for r in &mut project.returns {
        r.automation.clear();
    }
    project.master.automation.clear();
    project.markers.clear();
    project
}

/// A fader at `db`.
fn gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn track(name: &str, color: usize, instrument: InstrumentKind, db: f32) -> Track {
    let mut track = Track::new(name, PALETTE[color], instrument);
    track.mixer.gain = gain(db);
    track
}

/// A factory kit on Drum Pads (its sounds made if missing).
fn kit(name: &str, kit: &str, db: f32) -> Result<Track, String> {
    let (mut track, _) = crate::example::kit_track(name, kit, PALETTE[1])?;
    track.mixer.gain = gain(db);
    Ok(track)
}

/// A Drum Machine with one of its factory kits.
fn drum_machine(name: &str, preset: &str, db: f32) -> Track {
    track(name, 1, instrument(InstrumentKind::DrumMachine(DrumParams::default()), preset), db)
}

/// A voice: less rumble and mud, a little more presence, an even level.
fn voice(name: &str, color: usize, reverb: f32) -> Track {
    let mut track = Track::new_audio(name, PALETTE[color]);
    track.devices.push(Device::new(Effect::Eq3(Eq3Params { low_db: -6.0, high_db: 2.0, low_freq: 150.0, ..Default::default() })));
    track.devices.push(Device::new(Effect::Compressor(CompressorParams { threshold_db: -20.0, ratio: 3.0, attack_ms: 5.0, release_ms: 80.0, makeup_db: 4.0, sidechain: None })));
    track.sends[0] = reverb;
    track
}

fn device(track: &mut Track, effect: Effect) {
    track.devices.push(Device::new(effect));
}

/// The returns: A a space (a factory reverb), B a delay of `delay` beats.
fn returns(project: &mut Project, space: &str, delay: f64, feedback: f32) {
    let mut reverb = effect(Effect::Convolution(Default::default()), space);
    if let Effect::Convolution(c) = &mut reverb {
        c.mix = 1.0;
    }
    project.returns[0].name = "A Espacio".into();
    project.returns[0].devices = vec![Device::new(reverb)];
    project.returns[1].name = "B Delay".into();
    project.returns[1].devices = vec![Device::new(Effect::Delay(DelayParams { time_beats: delay, feedback, mix: 1.0, ping_pong: true, ..Default::default() }))];
}

/// A club master: the mix glued, a limiter under 0 dB.
fn club_master(project: &mut Project, limiter_db: f32) {
    project.master.devices.push(Device::new(effect(Effect::Glue(Default::default()), "Pegar la mezcla")));
    project.master.devices.push(Device::new(Effect::Limiter(LimiterParams { gain_db: limiter_db, ..Default::default() })));
}

/// The house «pump»: ducking on every beat.
fn pump(track: &mut Track, depth: f32) {
    device(track, Effect::Ducker(DuckerParams { depth, ..Default::default() }));
}

fn built_in(name: &str) -> Result<Project, String> {
    let mut project = Project::empty();
    match name {
        "Beat" => {
            project.tempo = 124.0;
            let mut drums = Track::new("Batería", PALETTE[1], InstrumentKind::DrumMachine(DrumParams { model: DrumModel::Tr909, ..Default::default() }));
            drums.devices.push(Device::new(Effect::Compressor(CompressorParams { threshold_db: -14.0, ratio: 3.0, attack_ms: 8.0, release_ms: 90.0, makeup_db: 1.5, sidechain: None })));
            let mut bass = Track::new("Bajo", PALETTE[9], InstrumentKind::Acid(AcidParams::default()));
            let mut chords = Track::new("Acordes", PALETTE[6], InstrumentKind::SuperSaw(SuperSawParams { gain: 0.3, ..Default::default() }));
            chords.sends[0] = 0.3;
            let mut lead = Track::new("Melodía", PALETTE[11], InstrumentKind::Fm(FmParams::default()));
            lead.sends[..2].copy_from_slice(&[0.15, 0.25]);
            // Headroom from the start: the drums at -7 dB, the rest under them
            // as a mix starts (the accompaniments made from the chords follow
            // their fader). All at the default fader, the drop went 3 dB over 0.
            drums.mixer.gain = 0.45;
            bass.mixer.gain = 0.32;
            chords.mixer.gain = 0.32;
            lead.mixer.gain = 0.28;
            project.tracks = vec![drums, bass, chords, lead];
        }
        "Grabar voz" => {
            project.tempo = 90.0;
            let backing = crate::project::Track::new_audio("Base", PALETTE[9]);
            project.tracks = vec![backing, voice("Voz", 0, 0.2), voice("Coros", 4, 0.35)];
        }
        "House" => {
            project.tempo = 124.0;
            let drums = kit("Batería", "House", -6.0)?;
            let mut bass = track("Bajo", 9, instrument(InstrumentKind::Synth(SynthParams::default()), "Bajo · Triángulo cálido"), -6.0);
            pump(&mut bass, 0.5);
            let mut organ = track("Órgano", 6, instrument(InstrumentKind::Keys(KeysParams::default()), "Órgano · House"), -12.0);
            organ.sends[0] = 0.2;
            pump(&mut organ, 0.7);
            let mut pad = track("Pad", 11, instrument(InstrumentKind::Wavetable(WaveParams::default()), "Pad · Aire"), -15.0);
            pad.sends[0] = 0.35;
            pump(&mut pad, 0.8);
            let mut pluck = track("Pluck", 7, instrument(InstrumentKind::Synth(SynthParams::default()), "Pluck · Corto"), -6.0);
            pluck.sends[..2].copy_from_slice(&[0.2, 0.25]);
            project.tracks = vec![drums, bass, organ, pad, pluck];
            returns(&mut project, "Retorno · Sala", 0.75, 0.4);
            club_master(&mut project, 2.0);
        }
        "Techno" => {
            project.tempo = 130.0;
            let mut drums = kit("Batería", "Techno", -8.0)?;
            drums.sends[0] = 0.08;
            let bass = track("Bajo", 9, instrument(InstrumentKind::Acid(AcidParams::default()), "Bajo · Techno rodante"), -14.0);
            let mut acid = track("Acid", 3, instrument(InstrumentKind::Acid(AcidParams::default()), "Acid · Clásico"), -13.0);
            acid.sends[1] = 0.2;
            let mut stab = track("Stab", 6, instrument(InstrumentKind::Wavetable(WaveParams::default()), "Acordes · Stab menor 7"), -18.0);
            stab.sends[..2].copy_from_slice(&[0.3, 0.3]);
            let mut atmosphere = track("Atmósfera", 11, instrument(InstrumentKind::Wavetable(WaveParams::default()), "Pad · Oscuro de cine"), -20.0);
            atmosphere.sends[0] = 0.4;
            project.tracks = vec![drums, bass, acid, stab, atmosphere];
            returns(&mut project, "Pads · Sala grande", 0.75, 0.45);
            club_master(&mut project, 2.0);
        }
        "Trap" => {
            project.tempo = 140.0;
            let drums = kit("Batería", "Trap", -6.0)?;
            let mut sub = track("808", 9, instrument(InstrumentKind::Kick(KickParams::default()), "808 · Largo"), -14.0);
            device(&mut sub, Effect::Saturator(SaturatorParams { drive_db: 6.0, output_db: -4.0, mix: 0.6, ..Default::default() }));
            let mut bells = track("Campanas", 6, instrument(InstrumentKind::Fm(FmParams::default()), "Campanas · Caja de música"), -9.0);
            bells.sends[..2].copy_from_slice(&[0.3, 0.2]);
            let mut pad = track("Pad", 11, instrument(InstrumentKind::Wavetable(WaveParams::default()), "Pad · Oscuro de cine"), -18.0);
            pad.sends[0] = 0.3;
            project.tracks = vec![drums, sub, bells, pad, voice("Voz", 0, 0.15)];
            returns(&mut project, "Voces · Placa brillante", 0.5, 0.35);
            club_master(&mut project, 1.0);
        }
        "Reggaetón" => {
            project.tempo = 95.0;
            let drums = drum_machine("Batería", "Reggaetón · Dembow", -6.0);
            let bass = track("Bajo", 9, instrument(InstrumentKind::Synth(SynthParams::default()), "Bajo · Sub puro"), -6.0);
            let mut marimba = track("Marimba", 6, instrument(InstrumentKind::Fm(FmParams::default()), "Pluck · Marimba"), -7.0);
            marimba.sends[..2].copy_from_slice(&[0.15, 0.15]);
            let mut lead = track("Lead", 7, instrument(InstrumentKind::Synth(SynthParams::default()), "Lead · Flauta suave"), -10.0);
            lead.sends[..2].copy_from_slice(&[0.2, 0.25]);
            project.tracks = vec![drums, bass, marimba, lead, voice("Voz", 0, 0.2)];
            returns(&mut project, "Voces · Sala cálida", 0.5, 0.3);
            club_master(&mut project, 1.5);
        }
        "Pop" => {
            project.tempo = 110.0;
            let mut drums = drum_machine("Batería", "Synthwave · Ochentera", -4.0);
            drums.sends[0] = 0.1;
            let bass = track("Bajo", 9, instrument(InstrumentKind::Fm(FmParams::default()), "Bajo · FM de los 80"), -4.0);
            let mut piano = track("Piano", 6, instrument(InstrumentKind::Keys(KeysParams::default()), "Piano · Pop"), -4.0);
            piano.sends[0] = 0.2;
            let mut pad = track("Pad", 11, instrument(InstrumentKind::SuperSaw(SuperSawParams::default()), "Pad · Ancho"), -12.0);
            pad.sends[0] = 0.3;
            project.tracks = vec![drums, bass, piano, pad, voice("Voz", 0, 0.2), voice("Coros", 4, 0.35)];
            returns(&mut project, "Voces · Placa brillante", 0.75, 0.3);
            club_master(&mut project, 1.0);
        }
        "Lo-Fi" => {
            project.tempo = 85.0;
            project.swing = 0.3;
            let mut drums = kit("Batería", "Lo-Fi", -3.0)?;
            device(&mut drums, Effect::Bitcrusher(BitcrusherParams { bits: 12.0, downsample: 2.0, mix: 0.4 }));
            let mut keys = track("Piano", 6, instrument(InstrumentKind::Keys(KeysParams::default()), "Piano · Lo-fi"), -1.0);
            keys.sends[..2].copy_from_slice(&[0.25, 0.15]);
            let bass = track("Bajo", 9, instrument(InstrumentKind::Synth(SynthParams::default()), "Bajo · Triángulo cálido"), -6.0);
            let mut pad = track("Textura", 11, instrument(InstrumentKind::Synth(SynthParams::default()), "Pad · Niebla"), -13.0);
            pad.sends[0] = 0.3;
            project.tracks = vec![drums, keys, bass, pad];
            returns(&mut project, "Voces · Muelle vintage", 0.75, 0.35);
            // The tape: a little warmth and a softer top, then the limiter.
            project.master.devices.push(Device::new(Effect::Saturator(SaturatorParams { drive_db: 3.0, output_db: -2.0, mix: 0.5, ..Default::default() })));
            project.master.devices.push(Device::new(Effect::Eq3(Eq3Params { high_db: -2.5, ..Default::default() })));
            project.master.devices.push(Device::new(Effect::Limiter(LimiterParams { gain_db: 1.0, ..Default::default() })));
        }
        "Drum & Bass" => {
            project.tempo = 174.0;
            let drums = kit("Batería", "Drum & Bass", -6.0)?;
            let mut reese = track("Reese", 3, instrument(InstrumentKind::Wavetable(WaveParams::default()), "Bajo · Reese oscuro"), -13.0);
            // Its lows are the sub's.
            device(&mut reese, Effect::Eq3(Eq3Params { low_db: -12.0, low_freq: 120.0, ..Default::default() }));
            let sub = track("Sub", 9, instrument(InstrumentKind::Wavetable(WaveParams::default()), "Bajo · Sub"), -13.0);
            let mut pad = track("Pad", 11, instrument(InstrumentKind::Wavetable(WaveParams::default()), "Pad · Aire"), -11.0);
            pad.sends[0] = 0.4;
            project.tracks = vec![drums, reese, sub, pad];
            returns(&mut project, "Pads · Sala grande", 0.75, 0.4);
            club_master(&mut project, 2.0);
        }
        "Podcast" => {
            let talker = |name: &str, color: usize| {
                let mut track = Track::new_audio(name, PALETTE[color]);
                // Quiet between words, no rumble, even and without hiss.
                track.devices.push(Device::new(Effect::Gate(GateParams { threshold_db: -50.0, floor_db: -20.0, ..Default::default() })));
                track.devices.push(Device::new(Effect::Eq3(Eq3Params { low_db: -12.0, low_freq: 90.0, high_db: 1.5, ..Default::default() })));
                track.devices.push(Device::new(Effect::Compressor(CompressorParams { threshold_db: -22.0, ratio: 3.5, attack_ms: 8.0, release_ms: 120.0, makeup_db: 6.0, sidechain: None })));
                track.devices.push(Device::new(Effect::DeEsser(DeEsserParams::default())));
                track
            };
            let mut music = Track::new_audio("Música", PALETTE[9]);
            music.mixer.gain = gain(-12.0);
            project.tracks = vec![talker("Voz 1", 0), talker("Voz 2", 4), music];
            project.assign_ids();
            // The music steps aside while the first voice talks.
            let first = project.tracks[0].id;
            project.tracks[2].devices.push(Device::new(Effect::Ducker(DuckerParams { depth: 0.6, sidechain: Some(first), ..Default::default() })));
            project.returns.truncate(1);
            project.returns[0].name = "A Sala".into();
            project.returns[0].devices = vec![Device::new(effect(Effect::Convolution(Default::default()), "Retorno · Habitación"))];
            // Voices in the middle, and the level podcasts are heard at.
            project.master.devices.push(Device::new(Effect::Utility(UtilityParams { width: 0.0, ..Default::default() })));
            project.master.devices.push(Device::new(Effect::Limiter(LimiterParams { gain_db: 3.0, ceiling_db: -1.0, ..Default::default() })));
        }
        _ => return Err(format!("No hay ninguna plantilla «{name}»")),
    }
    project.assign_ids();
    Ok(project)
}

impl PulseApp {
    /// A new project from the template `index` of the list (see `commands`).
    pub fn new_from_template(&mut self, index: usize) {
        let Some(template) = self.templates.get(index).cloned() else { return };
        self.start_from(&template);
    }

    /// ⌘N: the template chosen in Settings, or an empty project.
    pub fn new_default(&mut self) {
        match self.settings.new_from.as_deref().and_then(|name| find(&self.templates, name)) {
            Some(template) => {
                // One that can't be opened still gives a new, empty project.
                if !self.start_from(&template) {
                    let why = self.status.take().unwrap_or_default();
                    self.new_project();
                    self.status = Some(fill(tr("{} · proyecto vacío"), &[&why]));
                }
            }
            None => self.new_project(),
        }
    }

    /// Whether the template opened.
    fn start_from(&mut self, template: &Template) -> bool {
        match project(template) {
            Ok(project) => {
                self.replace_project(project, None);
                self.status = Some(fill(tr("Proyecto nuevo desde la plantilla «{}»"), &[&template.shown()]));
                true
            }
            Err(error) => {
                self.status = Some(fill(tr("No se pudo abrir la plantilla «{}»: {}"), &[&template.shown(), &error]));
                self.templates = all();
                false
            }
        }
    }

    /// "Guardar como plantilla…": asks for a name, in the templates' folder.
    pub fn save_as_template(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter(tr("Plantilla de Pulse"), &["pulse"])
            .set_directory(crate::library::folder(crate::library::TEMPLATES))
            .set_file_name(format!("{}.pulse", self.file_name()));
        if let Some(path) = crate::files::save_file(dialog) {
            let name = path.file_stem().map_or("Plantilla".into(), |s| s.to_string_lossy().into_owned());
            self.save_template(&name);
        }
    }

    /// Saves the project, without its content, as the template `name` (over one
    /// of that name): its samples are copied into its folder, and the project
    /// stays as it was.
    pub fn save_template(&mut self, name: &str) {
        let name = name.trim().replace(['/', '\\', ':'], "-");
        if name.is_empty() || name.starts_with('.') {
            self.status = Some(tr("La plantilla necesita un nombre").into());
            return;
        }
        self.capture_plugins();
        let mut template = without_content(&self.project);
        let dir = crate::library::folder(crate::library::TEMPLATES).join(&name);
        // Over one of that name: its old samples go, except those the project
        // (or its undo) uses.
        crate::library::prune_samples(std::iter::once(&self.project).chain(self.history.states()), &dir);
        let collected = crate::library::collect_copies(&mut template, &dir);
        let result = std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|_| template.save(&dir.join(format!("{name}.pulse"))));
        self.status = Some(match result {
            Ok(_) if collected.missing > 0 => fill(tr("Plantilla «{}» guardada · {} audio(s) no encontrados"), &[&name, &collected.missing]),
            Ok(_) => fill(tr("Plantilla «{}» guardada: está en Archivo ▸ Nuevo desde plantilla"), &[&name]),
            Err(error) => fill(tr("No se pudo guardar la plantilla: {}"), &[&error]),
        });
        self.templates = all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Command;
    use eframe::egui;

    #[test]
    fn pulse_brings_templates_that_open() {
        for name in BUILT_IN {
            let project = built_in(name).unwrap();
            assert!(!about(name).is_empty(), "{name}");
            assert!(!project.tracks.is_empty(), "{name}");
            // As saved and opened again.
            let again = Project::from_json(&project.to_json().unwrap()).unwrap();
            assert_eq!(again.tracks.len(), project.tracks.len());
        }
        assert!(built_in("Nada").is_err());
    }

    /// What a user would write first in a template: the drums' basic beat,
    /// roots on the bass, the chords held, a tune on top (Am F C G, a bar each,
    /// over `bars`), on the Arrangement.
    fn play_in(project: &mut Project, bars: usize) {
        use crate::arrange::Role;
        use engine::model::{ArrangementClip, Clip, MidiClip, Note};
        let roots = [45u8, 41, 48, 43];
        let chords: [[u8; 3]; 4] = [[57, 60, 64], [53, 57, 60], [55, 60, 64], [55, 59, 62]];
        for track in project.tracks.iter_mut().filter(|t| !t.is_audio() && !t.is_group()) {
            let mut notes = Vec::new();
            for bar in 0..bars {
                let at = bar as f64 * 4.0;
                let (root, chord) = (roots[bar % 4], chords[bar % 4]);
                let low = matches!(track.instrument, InstrumentKind::Kick(_));
                match crate::arrange::role(track) {
                    Role::Drums if !low => {
                        for beat in 0..4 {
                            let b = at + beat as f64;
                            notes.push(Note::new(b, 0.25, 36, 1.0));
                            notes.push(Note::new(b + 0.5, 0.25, 42, 0.7));
                            if beat % 2 == 1 {
                                notes.push(Note::new(b, 0.25, 39, 0.9));
                            }
                        }
                    }
                    Role::Bass | Role::Drums => {
                        let pitch = if low { root - 12 } else { root - 12 + 12 * (track.name == "Reese") as u8 };
                        for step in [0.0, 1.5, 2.5, 3.0] {
                            notes.push(Note::new(at + step, 0.5, pitch, 0.9));
                        }
                    }
                    Role::Harmony => notes.extend(chord.iter().map(|&p| Note::new(at, 3.8, p, 0.75))),
                    Role::Lead => {
                        for (k, &p) in chord.iter().chain(&[chord[1]]).enumerate() {
                            notes.push(Note::new(at + k as f64, 0.75, p + 12, 0.8));
                        }
                    }
                }
            }
            let clip = Clip::Midi(MidiClip { notes, ..MidiClip::new(&track.name, bars as f64 * 4.0, track.color) });
            track.arrangement = vec![ArrangementClip { start: 0.0, length: bars as f64 * 4.0, clip }];
        }
        project.loop_end = bars as f64 * 4.0;
    }

    /// The mix of `project`'s Arrangement: (peak, integrated LUFS, its quietest bar's RMS in dB).
    fn measure(project: &Project, name: &str) -> (f32, f64, f32) {
        let file = std::env::temp_dir().join(format!("pulse-plantilla-{}-{}.wav", name.replace(' ', "_"), std::process::id()));
        crate::export::render_project(&file, project).unwrap();
        let song = engine::Sample::load(&file).unwrap();
        let _ = std::fs::remove_file(&file);
        let bar = (4.0 * 60.0 / project.tempo * song.sample_rate as f64) as usize;
        let bars = (project.loop_end / 4.0) as usize;
        let quietest = song.left.chunks(bar).zip(song.right.chunks(bar)).take(bars).map(|(l, r)| {
            let power = l.iter().chain(r).map(|s| s * s).sum::<f32>() / (2 * l.len()) as f32;
            10.0 * power.max(1e-12).log10()
        }).fold(f32::MAX, f32::min);
        (crate::export::peak(&song.left, &song.right), stems::loudness::integrated(&song.left, &song.right, song.sample_rate).unwrap_or(-70.0), quietest)
    }

    /// Each of Pulse's templates with something played in it: every track
    /// sounds, the mix doesn't clip and is about as loud as a song starts,
    /// and the master's limiter isn't what keeps it under 0 dB.
    #[test]
    fn pulses_templates_sound_without_silence_or_clipping() {
        std::thread::scope(|scope| {
            for name in BUILT_IN {
                scope.spawn(move || sounds(name));
            }
        });
    }

    fn sounds(name: &str) {
        {
            let mut project = built_in(name).unwrap();
            if project.tracks.iter().all(|t| t.is_audio()) {
                // To record into: audio tracks with their chains, and a limiter at the end.
                assert!(project.tracks.iter().skip(1).all(|t| !t.devices.is_empty()), "{name}");
                return;
            }
            play_in(&mut project, 4);
            let (peak, lufs, quietest) = measure(&project, name);
            let mut raw = project.clone();
            raw.master.devices.clear();
            let (raw_peak, _, _) = measure(&raw, name);
            eprintln!("{name}: pico {:.1} dBFS ({:.1} sin master), {lufs:.1} LUFS, compás más bajo {quietest:.1} dB", 20.0 * peak.log10(), 20.0 * raw_peak.log10());
            assert!(peak <= 1.0, "{name}: satura ({peak})");
            assert!(raw_peak < 10f32.powf(4.0 / 20.0), "{name}: sin el master pasa de +4 dBFS ({raw_peak})");
            assert!((-20.0..=-8.0).contains(&lufs), "{name}: {lufs} LUFS");
            assert!(quietest > -40.0, "{name}: un compás en silencio ({quietest} dB)");
            // Each instrument on its own.
            for (t, track) in project.tracks.iter().enumerate().filter(|(_, t)| !t.arrangement.is_empty()) {
                let mut alone = project.clone();
                alone.loop_end = 8.0;
                for (u, other) in alone.tracks.iter_mut().enumerate() {
                    other.mixer.active = u == t || (other.is_group() && u < t);
                }
                let (_, _, quietest) = measure(&alone, name);
                eprintln!("    {}: {quietest:.1} dB", track.name);
                assert!(quietest > -45.0, "{name}: «{}» no suena ({quietest} dB)", track.name);
            }
        }
    }

    #[test]
    fn a_template_keeps_the_setup_and_leaves_the_content_out() {
        let mut app = PulseApp::headless(Project::demo());
        app.project.tempo = 97.0;
        let had_clips = app.project.tracks.iter().any(|t| t.clips.iter().any(Option::is_some));
        assert!(had_clips);
        let before = app.project.clone();
        let name = format!("Prueba {}", std::process::id());
        app.save_template(&name);
        // The project is untouched, and the template is in the list.
        assert_eq!(app.project, before);
        let index = app.templates.iter().position(|t| t.name == name).expect("listed");
        assert!(app.templates[..BUILT_IN.len()].iter().all(|t| t.file.is_none()));

        app.run(Command::New, &egui::Context::default());
        app.new_from_template(index);
        assert!(!app.is_dirty(), "a new project, nothing to save yet");
        assert_eq!(app.file, None, "saving asks where");
        assert_eq!(app.project.tempo, 97.0);
        assert_eq!(app.project.tracks.len(), before.tracks.len());
        assert_eq!(app.project.tracks[0].devices, before.tracks[0].devices);
        assert!(app.project.tracks.iter().all(|t| t.clips.iter().all(Option::is_none) && t.arrangement.is_empty()));

        // ⌘N starts from it once chosen; from an empty project if it is gone.
        app.settings.new_from = Some(name.clone());
        app.project.tempo = 120.0;
        app.saved = app.project.clone();
        app.run(Command::New, &egui::Context::default());
        assert_eq!(app.project.tempo, 97.0);
        let _ = std::fs::remove_dir_all(crate::library::folder(crate::library::TEMPLATES).join(&name));
        app.templates = all();
        app.run(Command::New, &egui::Context::default());
        assert_eq!(app.project.tempo, 120.0);
    }

    #[test]
    fn a_template_copies_its_samples_and_the_project_keeps_its_own() {
        // In a folder of its own: another test checks that `Grabaciones` empties.
        let unsaved = crate::library::root().join(crate::library::UNSAVED).join(format!("plantilla-{}", std::process::id()));
        let sample = unsaved.join("golpe.wav");
        std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
        std::fs::write(&sample, b"RIFF").unwrap();
        let mut project = Project::empty();
        project.tracks[0].instrument = InstrumentKind::Sampler(Default::default());
        project.tracks[0].set_sample(0, Some(sample.to_string_lossy().into_owned()));
        let mut app = PulseApp::headless(project);
        let name = format!("Muestras {}", std::process::id());
        app.save_template(&name);
        // An unsaved project's recording is copied, never moved away from it.
        assert!(sample.is_file());
        let template = find(&app.templates, &name).unwrap();
        let opened = super::project(&template).unwrap();
        let copied = opened.tracks[0].sample(0).unwrap();
        assert!(copied.starts_with(&*crate::library::folder(crate::library::TEMPLATES).join(&name).to_string_lossy()), "{copied}");
        assert!(std::path::Path::new(copied).is_file());
        let _ = std::fs::remove_dir_all(crate::library::folder(crate::library::TEMPLATES).join(&name));
        let _ = std::fs::remove_dir_all(&unsaved);
    }

    /// ⌘N with the chosen template damaged, or gone since the list was made:
    /// an empty project, and the status says why.
    #[test]
    fn cmd_n_with_a_broken_template_makes_an_empty_project() {
        let name = format!("Rota {}", std::process::id());
        let dir = crate::library::folder(crate::library::TEMPLATES).join(&name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.pulse")), "{ dañado").unwrap();
        let mut app = PulseApp::headless(Project::demo());
        app.templates = all();
        app.settings.new_from = Some(name.clone());
        let mut empty = Project::empty();
        empty.assign_ids();
        for _ in 0..2 {
            app.project.tempo = 77.0;
            app.saved = app.project.clone();
            app.run(Command::New, &egui::Context::default());
            assert_eq!(app.project, empty);
            let status = app.status.clone().unwrap_or_default();
            assert!(status.contains("No se pudo abrir la plantilla") && status.contains(&name), "{status}");
        }
        // Deleted while still in the list.
        app.templates = all();
        let _ = std::fs::remove_dir_all(&dir);
        app.project.tempo = 77.0;
        app.saved = app.project.clone();
        app.run(Command::New, &egui::Context::default());
        assert_eq!(app.project, empty);
        assert_eq!(app.settings.new_from, Some(name), "the choice stays");
    }

    /// The `.wav` files in a template's folder, by name.
    fn wavs(dir: &std::path::Path) -> Vec<String> {
        let mut files = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                if e.path().is_dir() {
                    stack.push(e.path());
                } else if e.path().extension().is_some_and(|x| x == "wav") {
                    files.push(e.file_name().to_string_lossy().into_owned());
                }
            }
        }
        files.sort();
        files
    }

    /// A project with a sampler playing `golpe.wav`, in a folder of its own.
    fn with_a_sample(test: &str) -> (PulseApp, std::path::PathBuf) {
        let source = std::env::temp_dir().join(format!("pulse-plantilla-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&source);
        std::fs::create_dir_all(&source).unwrap();
        let sample = source.join("golpe.wav");
        std::fs::write(&sample, b"RIFF").unwrap();
        let mut project = Project::empty();
        project.tracks[0].instrument = InstrumentKind::Sampler(Default::default());
        project.tracks[0].set_sample(0, Some(sample.to_string_lossy().into_owned()));
        (PulseApp::headless(project), source)
    }

    #[test]
    fn saving_a_template_again_keeps_one_copy_of_each_sample() {
        let (mut app, source) = with_a_sample("repetida");
        let name = format!("Repetida {}", std::process::id());
        let dir = crate::library::folder(crate::library::TEMPLATES).join(&name);
        for _ in 0..3 {
            app.save_template(&name);
        }
        assert_eq!(wavs(&dir), vec!["golpe.wav".to_string()]);
        let opened = super::project(&find(&app.templates, &name).unwrap()).unwrap();
        assert!(std::path::Path::new(opened.tracks[0].sample(0).unwrap()).is_file());
        // A sample the project no longer uses leaves the template too.
        app.project.tracks[0].set_sample(0, None);
        app.save_template(&name);
        assert!(wavs(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&source);
    }

    /// Opened from the template and saved over it: its samples are the ones
    /// the project plays, and they stay.
    #[test]
    fn saving_over_the_template_a_project_came_from_keeps_its_samples() {
        let (mut app, source) = with_a_sample("reabierta");
        let name = format!("Reabierta {}", std::process::id());
        let dir = crate::library::folder(crate::library::TEMPLATES).join(&name);
        app.save_template(&name);
        let index = app.templates.iter().position(|t| t.name == name).unwrap();
        app.new_from_template(index);
        let played = app.project.tracks[0].sample(0).unwrap().to_string();
        assert!(played.starts_with(&*dir.to_string_lossy()), "{played}");
        // The original is gone: only the template's copy is left.
        let _ = std::fs::remove_dir_all(&source);
        app.save_template(&name);
        app.save_template(&name);
        assert_eq!(wavs(&dir), vec!["golpe.wav".to_string()]);
        assert!(std::path::Path::new(&played).is_file());
        let opened = super::project(&find(&app.templates, &name).unwrap()).unwrap();
        assert_eq!(opened.tracks[0].sample(0), Some(played.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_users_template_wins_over_pulses_of_the_same_name() {
        let mine = Template { name: "Beat".into(), file: Some(PathBuf::from("/x/Beat/Beat.pulse")) };
        let list = [Template { name: "Beat".into(), file: None }, mine.clone()];
        assert_eq!(find(&list, "Beat"), Some(mine));
        assert_eq!(find(&list, "Otra"), None);
    }

    #[test]
    fn templates_kits_and_presets_speak_english() {
        use crate::i18n::{with, Language};
        let mut app = PulseApp::headless(Project::demo());
        app.templates = all();
        let index = app.templates.iter().position(|t| t.name == "Grabar voz").unwrap();
        with(Language::English, || {
            app.new_from_template(index);
            assert_eq!(app.status.as_deref(), Some("New project from the \"Record Vocals\" template"));
            app.save_template("  ");
            assert_eq!(app.status.as_deref(), Some("The template needs a name"));
            let house = crate::factory_sounds::kits().into_iter().find(|k| k.name == "House").unwrap();
            assert!(house.about.starts_with("Round, punchy kick"), "{}", house.about);
            let nameless = crate::presets::Preset { name: " ".into(), instrument: InstrumentKind::Drums, samples: Vec::new() };
            assert_eq!(crate::presets::save(std::path::Path::new("/nonexistent"), &nameless), Err("The preset needs a name".into()));
        });
        // In Spanish, as it was.
        app.save_template("");
        assert_eq!(app.status.as_deref(), Some("La plantilla necesita un nombre"));
    }
}
