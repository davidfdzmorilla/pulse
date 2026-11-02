//! Effect chains to keep and use again, like Live's racks or Bitwig's FX
//! chains: a track's effects with their settings, in order, and its
//! instrument if wanted. The factory ones are made of Pulse's presets; the
//! user's are JSON files in `Presets/Cadenas` (see `library`).
//!
//! Each effect is kept as the project keeps it but read on its own, so one a
//! Pulse doesn't know (a newer one wrote it) or a damaged one is left out and
//! the rest of the chain still loads. Plugins go with their saved state.

use crate::app::PulseApp;
use crate::project::PluginRef;
use crate::theme::TEXT_DIM;
use eframe::egui;
use engine::model::{Device, Effect, InstrumentKind, MidiDevice, PluginId};
use engine::{ChainSlot, Command, Target};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The format written; older files load, newer ones load what they can.
pub const VERSION: u32 = 1;

/// The folder the user's chains go in, under Presets.
pub const FOLDER: &str = "Cadenas";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    #[serde(default = "first_version")]
    pub version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<Link>,
    /// The instrument and its MIDI effects, when it was saved with them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument: Option<Voice>,
    /// Made by Pulse (not saved: the factory ones are built in).
    #[serde(skip)]
    pub factory: bool,
}

fn first_version() -> u32 {
    1
}

fn yes() -> bool {
    true
}

/// One effect of a chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    /// The effect as the project keeps it (see the module's notes).
    pub effect: serde_json::Value,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<SavedPlugin>,
}

/// The instrument a chain was saved with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    pub instrument: serde_json::Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub midi_effects: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<SavedPlugin>,
}

/// A plugin and its state, as a project keeps them (see `PluginRef`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedPlugin {
    pub path: String,
    pub cid: String,
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub state: String,
}

impl SavedPlugin {
    fn of(plugin: &PluginRef) -> Self {
        SavedPlugin { path: plugin.path.clone(), cid: plugin.cid.clone(), name: plugin.name.clone(), vendor: plugin.vendor.clone(), state: plugin.state.clone() }
    }
}

impl Chain {
    /// A chain of these effects, all on.
    pub fn of(name: &str, effects: Vec<Effect>) -> Chain {
        let devices = effects.into_iter().map(|effect| Link { effect: serde_json::to_value(effect).unwrap_or_default(), enabled: true, plugin: None }).collect();
        Chain { version: VERSION, name: name.to_string(), devices, instrument: None, factory: false }
    }

    /// Its effects this Pulse can read, in order (the others are left out).
    pub fn effects(&self) -> Vec<(Device, Option<SavedPlugin>)> {
        self.devices.iter().filter_map(|link| Some((Device { effect: serde_json::from_value::<Effect>(link.effect.clone()).ok()?, enabled: link.enabled }, link.plugin.clone()))).collect()
    }

    /// The effects left out for being unknown or damaged.
    pub fn unreadable(&self) -> usize {
        self.devices.len() - self.effects().len()
    }

    /// Its effects' names, for the browser's tooltip and the search.
    pub fn summary(&self) -> String {
        let mut names: Vec<String> = self.effects().iter().map(|(d, plugin)| plugin.as_ref().map_or_else(|| d.effect.label().to_string(), |p| p.name.clone())).collect();
        if let Some(kind) = self.instrument.as_ref().and_then(|v| serde_json::from_value::<InstrumentKind>(v.instrument.clone()).ok()) {
            names.insert(0, kind.label().to_string());
        }
        names.join(" → ")
    }
}

/// The ones that come with Pulse, made of its effects' presets.
pub fn factory() -> Vec<Chain> {
    use engine::model::*;
    fn preset(effect: Effect, name: &str) -> Effect {
        effect.factory_presets().into_iter().find(|(n, _)| *n == name).map_or(effect, |(_, e)| e)
    }
    let glue = |name: &str| preset(Effect::Glue(Default::default()), name);
    let ott = |name: &str| preset(Effect::Multiband(Default::default()), name);
    let space = |name: &str| preset(Effect::Convolution(Default::default()), name);
    // An EQ Eight with some of its bands: (band, shape, frequency, gain).
    let eq = |bands: &[(usize, EqShape, f32, f32)]| {
        let mut p = EqEightParams::default();
        for &(b, shape, freq, gain_db) in bands {
            p.bands[b] = EqBand { on: true, shape, freq, gain_db, q: 0.71 };
        }
        Effect::EqEight(p)
    };
    let mix = |effect: Effect, amount: f32| match effect {
        Effect::Convolution(p) => Effect::Convolution(ConvolutionParams { mix: amount, ..p }),
        Effect::Multiband(p) => Effect::Multiband(MultibandParams { mix: amount, ..p }),
        other => other,
    };
    let chains = vec![
        Chain::of(
            "Voz · Pop",
            vec![
                eq(&[(0, EqShape::LowCut48, 90.0, 0.0), (4, EqShape::Bell, 3000.0, 2.0), (6, EqShape::HighShelf, 10_000.0, 2.5)]),
                // Makeup for what the compressor, the de-esser and the sends take: the
                // voice comes out as loud as it went in (measured on a spoken voice).
                Effect::Compressor(CompressorParams { threshold_db: -20.0, ratio: 4.0, attack_ms: 5.0, release_ms: 80.0, makeup_db: 7.5, sidechain: None }),
                Effect::DeEsser(DeEsserParams::default()),
                ott("Voces · Voz de cerca"),
                Effect::Delay(DelayParams { time_beats: 0.5, feedback: 0.25, mix: 0.12, ping_pong: true, filter_hz: 4000.0 }),
                mix(space("Voces · Placa brillante"), 0.18),
            ],
        ),
        Chain::of(
            "Voz · Radio",
            vec![
                eq(&[(0, EqShape::LowCut48, 350.0, 0.0), (3, EqShape::Bell, 1500.0, 5.0), (7, EqShape::HighCut48, 3500.0, 0.0)]),
                Effect::Saturator(SaturatorParams { drive_db: 12.0, output_db: -6.0, mix: 1.0, kind: KickDrive::Tube, oversample: false }),
                Effect::Compressor(CompressorParams { threshold_db: -26.0, ratio: 8.0, attack_ms: 2.0, release_ms: 60.0, makeup_db: 10.5, sidechain: None }),
                Effect::Utility(UtilityParams { width: 0.0, ..Default::default() }),
            ],
        ),
        Chain::of(
            "Bus de batería · Pegada",
            vec![
                Effect::Transient(TransientParams { attack: 0.3, sustain: -0.1, output_db: 0.0 }),
                glue("Batería · Pegada (ataque lento)"),
                // Its output takes back what the bus compressor and the drive add: more
                // punch, not more level (to judge it by ear, not by loudness).
                Effect::Saturator(SaturatorParams { drive_db: 4.0, output_db: -3.5, mix: 0.5, kind: KickDrive::Tube, oversample: false }),
            ],
        ),
        Chain::of(
            "Bajo · Sidechain y saturación",
            vec![
                eq(&[(0, EqShape::LowCut12, 30.0, 0.0)]),
                Effect::Saturator(SaturatorParams { drive_db: 8.0, output_db: -4.0, mix: 0.7, kind: KickDrive::Tube, oversample: false }),
                // Pumps with the beat: no track to listen to (see the guide).
                Effect::Ducker(DuckerParams { depth: 0.75, release: 0.6, rate_beats: 1.0, ..Default::default() }),
                Effect::Utility(UtilityParams { bass_mono_hz: 120.0, ..Default::default() }),
            ],
        ),
        Chain::of(
            "Máster · Club",
            vec![
                eq(&[(0, EqShape::LowCut12, 25.0, 0.0)]),
                glue("Mezcla · Pegar la mezcla"),
                mix(ott("OTT · OTT suave"), 0.4),
                Effect::Utility(UtilityParams { bass_mono_hz: 120.0, ..Default::default() }),
                // -1 dBTP: room for the encoders of the streaming services.
                Effect::Limiter(LimiterParams { gain_db: 3.0, ceiling_db: -1.0, release_ms: 120.0 }),
            ],
        ),
        Chain::of(
            "Pad · Ancho y lejano",
            vec![
                eq(&[(0, EqShape::LowCut12, 150.0, 0.0), (6, EqShape::HighShelf, 9000.0, -2.0)]),
                Effect::Chorus(ChorusParams { mix: 0.35, ..Default::default() }),
                Effect::Delay(DelayParams { time_beats: 0.75, feedback: 0.45, mix: 0.2, ping_pong: true, filter_hz: 3500.0 }),
                space("Pads · Sala grande"),
                // Wide but still there in mono; the level the chorus, the delay and the
                // room take, given back.
                Effect::Utility(UtilityParams { width: 1.15, gain_db: 4.5, ..Default::default() }),
            ],
        ),
    ];
    chains.into_iter().map(|c| Chain { factory: true, ..c }).collect()
}

/// The folder with the user's chains.
pub fn user_root() -> PathBuf {
    crate::library::folder(crate::library::PRESETS).join(FOLDER)
}

fn file_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c }).collect();
    format!("{}.json", clean.trim())
}

/// A chain file, or None if it isn't one.
pub fn read(path: &Path) -> Option<Chain> {
    let chain: Chain = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    (!chain.name.trim().is_empty()).then_some(chain)
}

/// The user's chains in `root`, by name. Files that aren't chains are skipped.
pub fn list(root: &Path) -> Vec<Chain> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut chains: Vec<Chain> = entries.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "json")).filter_map(|e| read(&e.path())).collect();
    chains.sort_by_key(|c| c.name.to_lowercase());
    chains
}

/// Saves (or replaces) a chain in `root`; returns its file.
pub fn save(root: &Path, chain: &Chain) -> Result<PathBuf, String> {
    if chain.name.trim().is_empty() {
        return Err("La cadena necesita un nombre".into());
    }
    std::fs::create_dir_all(root).map_err(|e| format!("No se pudo crear {}: {e}", root.display()))?;
    let path = root.join(file_name(&chain.name));
    crate::store::write_json(&path, chain)?;
    Ok(path)
}

pub fn delete(root: &Path, name: &str) -> Result<(), String> {
    let path = root.join(file_name(name));
    std::fs::remove_file(&path).map_err(|e| format!("No se pudo borrar {}: {e}", path.display()))
}

/// «Guardar cadena…» at the end of a chain: its name and whether the
/// instrument goes too. What to save, once chosen.
pub fn save_menu(app: &PulseApp, ui: &mut egui::Ui, target: Target) -> Option<(String, bool)> {
    let midi = matches!(target, Target::Track(t) if !app.project.tracks[t].is_audio() && !app.project.tracks[t].is_group());
    let empty = app.devices(target).is_empty() && !midi;
    let mut chosen = None;
    ui.add_enabled_ui(!empty, |ui| {
        let response = crate::widgets::field_menu_button(ui, "Guardar cadena…", |ui| {
            ui.set_min_width(210.0);
            ui.label(egui::RichText::new("GUARDAR LA CADENA").color(TEXT_DIM).size(10.0));
            let name_id = egui::Id::new(("chain_name", target));
            let with_id = egui::Id::new(("chain_with_instrument", target));
            let mut name: String = ui.ctx().memory(|m| m.data.get_temp(name_id)).unwrap_or_default();
            let mut with: bool = ui.ctx().memory(|m| m.data.get_temp(with_id)).unwrap_or(false);
            let edit = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Mi cadena de voz").desired_width(190.0));
            crate::widgets::name(&edit, egui::WidgetType::TextEdit, "Nombre de la cadena");
            if midi {
                ui.checkbox(&mut with, "Con el instrumento").on_hover_text("Guarda también el instrumento de la pista y sus efectos MIDI");
            }
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let save = ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Guardar")).on_hover_text("Aparece en el navegador, en CADENAS");
            if (save.clicked() || enter) && !name.trim().is_empty() {
                chosen = Some((name.trim().to_string(), with && midi));
                name.clear();
                ui.close();
            }
            ui.ctx().memory_mut(|m| {
                m.data.insert_temp(name_id, name);
                m.data.insert_temp(with_id, with);
            });
        });
        let _ = response;
    });
    chosen
}

impl PulseApp {
    /// The chain of `target` as it is, named `name` (with the track's
    /// instrument if `with_instrument`). Also what was left out, said.
    pub fn chain_of(&mut self, target: Target, name: &str, with_instrument: bool) -> (Chain, Vec<String>) {
        self.capture_plugins();
        let mut notes = Vec::new();
        let mut devices = Vec::new();
        let mut sidechains = false;
        for device in self.devices(target).clone() {
            let mut effect = device.effect;
            let plugin = match effect {
                Effect::Plugin(id) => match self.project.plugin(id) {
                    Some(p) => Some(SavedPlugin::of(p)),
                    None => {
                        notes.push("un plugin que no se pudo cargar".to_string());
                        continue;
                    }
                },
                _ => None,
            };
            // A sidechain points at a track of this project: not elsewhere.
            if let Some(slot) = effect.sidechain_mut() {
                sidechains |= slot.take().is_some();
            }
            devices.push(Link { effect: serde_json::to_value(effect).unwrap_or_default(), enabled: device.enabled, plugin });
        }
        if sidechains {
            notes.push("los sidechain (apuntan a pistas de este proyecto)".to_string());
        }
        let instrument = match target {
            Target::Track(t) if with_instrument => {
                let track = &self.project.tracks[t];
                let kind = track.instrument;
                let plugin = match kind {
                    InstrumentKind::Plugin(id) => self.project.plugin(id).map(SavedPlugin::of),
                    _ => None,
                };
                Some(Voice {
                    instrument: serde_json::to_value(kind).unwrap_or_default(),
                    samples: if kind.uses_samples() { track.samples.clone() } else { Vec::new() },
                    midi_effects: track.midi_effects.iter().filter_map(|m| serde_json::to_value(m).ok()).collect(),
                    plugin,
                })
            }
            _ => None,
        };
        (Chain { version: VERSION, name: name.to_string(), devices, instrument, factory: false }, notes)
    }

    /// «Guardar cadena…»: saves the chain of `target` among the user's.
    pub fn save_chain(&mut self, target: Target, name: &str, with_instrument: bool) {
        let (chain, left_out) = self.chain_of(target, name, with_instrument);
        self.status = Some(match save(&user_root(), &chain) {
            Ok(_) if left_out.is_empty() => format!("Cadena guardada: {name}"),
            Ok(_) => format!("Cadena guardada: {name} (sin {})", left_out.join(" ni ")),
            Err(error) => error,
        });
        self.browser.forget_index();
    }

    pub fn delete_chain(&mut self, name: &str) {
        self.status = Some(match delete(&user_root(), name) {
            Ok(()) => format!("Cadena borrada: {name}"),
            Err(error) => error,
        });
        self.browser.forget_index();
    }

    /// Puts a chain on `target`: its effects after the ones there (or instead
    /// of them, `replace`), and its instrument if it has one and the target
    /// is a MIDI track. One step of ⌘Z.
    pub fn load_chain(&mut self, target: Target, chain: &Chain, replace: bool) {
        let mut skipped: Vec<String> = Vec::new();
        let effects = chain.effects();
        match chain.unreadable() {
            0 => {}
            1 => skipped.push("un efecto que esta versión de Pulse no conoce".into()),
            n => skipped.push(format!("{n} efectos que esta versión de Pulse no conoce")),
        }
        if replace {
            for index in (0..self.devices(target).len()).rev() {
                self.remove_device(target, index);
            }
        }
        if let Some(voice) = &chain.instrument {
            match target {
                Target::Track(t) if !self.project.tracks[t].is_audio() && !self.project.tracks[t].is_group() => self.load_voice(t, voice, replace, &mut skipped),
                _ => skipped.push("el instrumento (el destino no es una pista MIDI)".into()),
            }
        }
        let mut added = 0;
        for (mut device, plugin) in effects {
            match (device.effect, plugin) {
                (Effect::Plugin(_), Some(saved)) => match self.restore_plugin(&saved) {
                    Ok((id, processor)) => {
                        device.effect = Effect::Plugin(id);
                        let index = self.devices(target).len();
                        self.devices_mut(target).push(device);
                        self.audio.send(Command::InsertDevice { target, index, slot: ChainSlot::plugin(&device, processor) });
                        added += 1;
                    }
                    Err(why) => skipped.push(why),
                },
                (Effect::Plugin(_), None) => skipped.push("un plugin sin sus datos".into()),
                _ => {
                    self.add_device(target, device);
                    added += 1;
                }
            }
        }
        self.detail = crate::app::Detail::Devices;
        let what = match added {
            1 => "1 efecto".to_string(),
            n => format!("{n} efectos"),
        };
        let mut status = format!("Cadena «{}» en {}: {what}", chain.name, self.target_name(target));
        if !skipped.is_empty() {
            status += &format!(" · sin {}", skipped.join(", "));
        }
        self.status = Some(status);
    }

    fn load_voice(&mut self, t: usize, voice: &Voice, replace: bool, skipped: &mut Vec<String>) {
        let kind = match serde_json::from_value::<InstrumentKind>(voice.instrument.clone()) {
            Ok(InstrumentKind::Plugin(_)) => match &voice.plugin {
                // `set_instrument` loads it from its state, as an undo would.
                Some(saved) if Path::new(&saved.path).exists() => {
                    let id = self.project.new_plugin_id();
                    self.project.plugins.push(PluginRef { id, path: saved.path.clone(), cid: saved.cid.clone(), name: saved.name.clone(), vendor: saved.vendor.clone(), state: saved.state.clone() });
                    Some(InstrumentKind::Plugin(id))
                }
                Some(saved) => {
                    skipped.push(format!("«{}» (no está instalado)", saved.name));
                    None
                }
                None => {
                    skipped.push("un plugin sin sus datos".into());
                    None
                }
            },
            Ok(kind) => Some(kind),
            Err(_) => {
                skipped.push("el instrumento, que esta versión de Pulse no conoce".into());
                None
            }
        };
        if let Some(kind) = kind {
            self.set_instrument(t, kind);
            if kind.uses_samples() {
                self.project.tracks[t].samples = voice.samples.clone();
                self.sync_samples(t);
            }
        }
        let midi: Vec<MidiDevice> = voice.midi_effects.iter().filter_map(|m| serde_json::from_value(m.clone()).ok()).collect();
        if replace {
            self.project.tracks[t].midi_effects.clear();
        }
        for device in midi {
            let effects = &mut self.project.tracks[t].midi_effects;
            if effects.len() < engine::model::MAX_MIDI_EFFECTS {
                effects.push(device);
            }
        }
        self.sync_midi_effects(t);
    }

    /// A plugin of a chain, back with its state, as a new instance.
    fn restore_plugin(&mut self, saved: &SavedPlugin) -> Result<(PluginId, engine::plugin::PluginProcessor), String> {
        if !Path::new(&saved.path).exists() {
            return Err(format!("«{}» (no está instalado)", saved.name));
        }
        let id = self.project.new_plugin_id();
        self.project.plugins.push(PluginRef { id, path: saved.path.clone(), cid: saved.cid.clone(), name: saved.name.clone(), vendor: saved.vendor.clone(), state: saved.state.clone() });
        let sample_rate = self.audio.sample_rate();
        match crate::sync::load_plugin(&self.project, id, &mut self.samples, sample_rate) {
            Some(processor) => Ok((id, processor)),
            None => {
                self.project.plugins.retain(|p| p.id != id);
                let why = self.samples.errors.pop().unwrap_or_default();
                self.samples.errors.clear();
                Err(format!("«{}» ({why})", saved.name))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, Track};
    use engine::model::*;

    fn root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pulse-cadenas-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A project with a synth playing chords for two bars.
    fn playing() -> PulseApp {
        let mut project = Project::empty();
        project.tracks.clear();
        let mut synth = Track::new("Voz", [200, 100, 50], InstrumentKind::Synth(Default::default()));
        let notes: Vec<Note> = (0..8).flat_map(|b| [57u8, 60, 64].map(|p| Note::new(b as f64, 0.9, p, 0.8))).collect();
        synth.arrangement.push(ArrangementClip { start: 0.0, length: 8.0, clip: Clip::Midi(MidiClip { notes, ..MidiClip::new("Acordes", 8.0, [0; 3]) }) });
        project.tracks.push(synth);
        PulseApp::headless(project)
    }

    #[test]
    fn a_chain_saves_lists_and_loads_back() {
        let dir = root("guardar");
        let mut app = playing();
        let t = Target::Track(0);
        app.add_device(t, Device::new(Effect::Compressor(CompressorParams { ratio: 6.0, sidechain: Some(7), ..Default::default() })));
        app.add_device(t, Device { effect: Effect::Delay(DelayParams { feedback: 0.6, ..Default::default() }), enabled: false });
        app.project.tracks[0].midi_effects.push(MidiDevice::new(MidiEffect::Arpeggiator(Default::default())));
        app.project.tracks[0].instrument = InstrumentKind::Acid(AcidParams { resonance: 0.9, ..Default::default() });
        let (chain, notes) = app.chain_of(t, "Mi cadena de voz", false);
        assert_eq!(notes.len(), 1, "the sidechain is left out, and said: {notes:?}");
        assert!(chain.instrument.is_none());
        save(&dir, &chain).unwrap();
        let (with, _) = app.chain_of(t, "Con el ácido", true);
        save(&dir, &with).unwrap();
        let listed = list(&dir);
        assert_eq!(listed.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Con el ácido", "Mi cadena de voz"]);
        let effects = listed[1].effects();
        assert_eq!(effects.len(), 2);
        assert!(matches!(effects[0].0.effect, Effect::Compressor(p) if p.ratio == 6.0 && p.sidechain.is_none()));
        assert!(!effects[1].0.enabled, "off stays off");

        // On another track: after its effects, or instead of them with ⌥.
        app.project.tracks.push(Track::new("Pad", [0, 0, 0], InstrumentKind::Synth(Default::default())));
        app.sync_all();
        let pad = Target::Track(1);
        app.add_device(pad, Device::new(Effect::Reverb(Default::default())));
        app.history.commit(&app.project, false);
        app.load_chain(pad, &listed[1], false);
        let labels = |app: &PulseApp| app.devices(pad).iter().map(|d| d.effect.label()).collect::<Vec<_>>();
        assert_eq!(labels(&app), ["Reverb", "Compressor", "Delay"]);
        assert!(matches!(app.project.tracks[1].instrument, InstrumentKind::Synth(_)), "without the instrument");
        app.history.commit(&app.project, false);
        app.undo();
        assert_eq!(labels(&app), ["Reverb"], "⌘Z takes it all back");
        app.load_chain(pad, &listed[0], true);
        assert_eq!(labels(&app), ["Compressor", "Delay"]);
        assert!(matches!(app.project.tracks[1].instrument, InstrumentKind::Acid(p) if p.resonance == 0.9));
        assert_eq!(app.project.tracks[1].midi_effects.len(), 1);
        // On an audio track the instrument has nowhere to go: said.
        let audio = app.add_audio_track().unwrap();
        app.load_chain(Target::Track(audio), &listed[0], false);
        assert_eq!(app.devices(Target::Track(audio)).len(), 2);
        assert!(app.status.as_deref().unwrap().contains("sin el instrumento"), "{:?}", app.status);
        delete(&dir, "Mi cadena de voz").unwrap();
        assert_eq!(list(&dir).len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A damaged file is no chain; an effect this Pulse doesn't know is left out
    /// and the rest loads; a newer version's extra fields are ignored.
    #[test]
    fn damaged_chains_break_nothing() {
        let dir = root("danada");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Rota.json"), b"{\"name\": \"Rota\", \"devices\": [").unwrap();
        std::fs::write(dir.join("Sin nombre.json"), b"{\"devices\": []}").unwrap();
        std::fs::write(dir.join("notas.txt"), b"hola").unwrap();
        let mut future = Chain::of("Del futuro", vec![Effect::Reverb(Default::default()), Effect::Utility(Default::default())]);
        future.version = 9;
        future.devices.insert(1, Link { effect: serde_json::json!({ "Vocoder3000": { "bands": 40 } }), enabled: true, plugin: None });
        future.devices.push(Link { effect: serde_json::json!({ "Reverb": { "size": "grande" } }), enabled: true, plugin: None });
        let mut text = serde_json::to_value(&future).unwrap();
        text["algo_nuevo"] = serde_json::json!(true);
        std::fs::write(dir.join("Del futuro.json"), serde_json::to_string(&text).unwrap()).unwrap();
        let listed = list(&dir);
        assert_eq!(listed.len(), 1, "only the readable chain");
        assert_eq!(listed[0].unreadable(), 2);
        let mut app = playing();
        app.load_chain(Target::Master, &listed[0], false);
        assert_eq!(app.devices(Target::Master).iter().map(|d| d.effect.label()).collect::<Vec<_>>(), ["Reverb", "Utility"]);
        assert!(app.status.as_deref().unwrap().contains("2 efectos que esta versión de Pulse no conoce"), "{:?}", app.status);
        // A plugin that isn't installed is left out, said.
        let mut gone = Chain::of("Plugin", vec![Effect::Plugin(PluginId(4))]);
        gone.devices[0].plugin = Some(SavedPlugin { path: "/no/existe/Nada.vst3".into(), cid: "0".repeat(32), name: "Nada".into(), vendor: String::new(), state: String::new() });
        app.load_chain(Target::Master, &gone, false);
        assert_eq!(app.devices(Target::Master).len(), 2);
        assert!(app.status.as_deref().unwrap().contains("«Nada» (no está instalado)"), "{:?}", app.status);
        assert!(app.project.plugins.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Every factory chain is made of presets that exist, loads whole, and
    /// sounds: finite, not silent, not clipping.
    #[test]
    fn factory_chains_load_and_sound() {
        let names: Vec<String> = factory().iter().map(|c| c.name.clone()).collect();
        assert_eq!(names, ["Voz · Pop", "Voz · Radio", "Bus de batería · Pegada", "Bajo · Sidechain y saturación", "Máster · Club", "Pad · Ancho y lejano"]);
        for (effect, name) in [(Effect::Glue(Default::default()), "Batería · Pegada (ataque lento)"), (Effect::Glue(Default::default()), "Mezcla · Pegar la mezcla"), (Effect::Multiband(Default::default()), "Voces · Voz de cerca"), (Effect::Multiband(Default::default()), "OTT · OTT suave"), (Effect::Convolution(Default::default()), "Voces · Placa brillante"), (Effect::Convolution(Default::default()), "Pads · Sala grande")] {
            assert!(effect.factory_presets().iter().any(|(n, _)| *n == name), "falta el preset {name}");
        }
        for chain in factory() {
            assert_eq!(chain.unreadable(), 0);
            let mut app = playing();
            let target = if chain.name.starts_with("Máster") { Target::Master } else { Target::Track(0) };
            app.load_chain(target, &chain, false);
            assert_eq!(app.devices(target).len(), chain.devices.len(), "{}", chain.name);
            assert!(!app.status.as_deref().unwrap_or("").contains("sin"), "{:?}", app.status);
            let project = app.project.clone();
            let mut samples = crate::sync::SampleCache::default();
            let commands = crate::sync::project_commands(&project, &mut samples, 48_000.0, &Default::default());
            let (l, r) = crate::export::render(commands, (0.0, 8.0), crate::export::Song::of(&project), 48_000.0, &Default::default(), &|_| {}).unwrap();
            assert!(l.iter().chain(&r).all(|s| s.is_finite()), "{}", chain.name);
            let peak = l.iter().chain(&r).fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak > 0.01 && peak < 1.5, "{}: pico {peak}", chain.name);
        }
    }

    /// «Máster · Club» leaves a true peak of -1 dBTP at most, however hot the
    /// mix comes in, and makes a mix with room louder.
    #[test]
    fn the_club_master_peaks_at_minus_one_dbtp() {
        let chain = factory().into_iter().find(|c| c.name == "Máster · Club").unwrap();
        for gain in [0.5, 1.0, 2.0] {
            let mut app = playing();
            app.project.tracks[0].mixer.gain = gain;
            let render = |app: &PulseApp| {
                let project = app.project.clone();
                let mut samples = crate::sync::SampleCache::default();
                let commands = crate::sync::project_commands(&project, &mut samples, 48_000.0, &Default::default());
                crate::export::render(commands, (0.0, 8.0), crate::export::Song::of(&project), 48_000.0, &Default::default(), &|_| {}).unwrap()
            };
            let (dl, dr) = render(&app);
            app.load_chain(Target::Master, &chain, false);
            let (l, r) = render(&app);
            let peak = stems::loudness::true_peak(&l, &r);
            assert!(peak <= -0.95, "a {gain}: {peak:.2} dBTP");
            let (before, after) = (stems::loudness::integrated(&dl, &dr, 48_000.0).unwrap(), stems::loudness::integrated(&l, &r, 48_000.0).unwrap());
            println!("a {gain}: {before:.1} → {after:.1} LUFS, {peak:.2} dBTP");
            assert!(gain >= 1.0 || after > before + 1.0, "a {gain}: {before:.1} → {after:.1} LUFS");
        }
    }
}
