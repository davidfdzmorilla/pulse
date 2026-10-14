//! Instrument presets: the factory ones built into Pulse and the user's own,
//! saved as JSON files (one folder per instrument) with the samples they use.

use engine::model::{InstrumentKind, KickParams};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub instrument: InstrumentKind,
    /// Sample files (click, sampler, pads) the preset plays.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<String>,
}

/// Presets that ship with Pulse for this kind of instrument.
pub fn factory(kind: &InstrumentKind) -> Vec<Preset> {
    match kind {
        InstrumentKind::Kick(_) => KickParams::presets()
            .into_iter()
            .map(|(name, params)| Preset { name: name.to_string(), instrument: InstrumentKind::Kick(params), samples: Vec::new() })
            .collect(),
        _ => Vec::new(),
    }
}

/// Folder with the user's presets for this kind of instrument, under `root`.
pub fn folder(root: &Path, kind: &InstrumentKind) -> PathBuf {
    root.join(kind.label())
}

pub fn user_root() -> PathBuf {
    crate::kits::kits_dir().with_file_name("Presets")
}

/// The user's presets for `kind`, by name. Unreadable files are skipped.
pub fn list(root: &Path, kind: &InstrumentKind) -> Vec<Preset> {
    let Ok(entries) = std::fs::read_dir(folder(root, kind)) else { return Vec::new() };
    let mut presets: Vec<Preset> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_str::<Preset>(&std::fs::read_to_string(e.path()).ok()?).ok())
        .filter(|p| p.instrument.label() == kind.label())
        .collect();
    presets.sort_by_key(|p| p.name.to_lowercase());
    presets
}

fn file_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c }).collect();
    format!("{}.json", clean.trim())
}

/// Saves (or overwrites) a preset; returns its file.
pub fn save(root: &Path, preset: &Preset) -> Result<PathBuf, String> {
    if preset.name.trim().is_empty() {
        return Err("El preset necesita un nombre".into());
    }
    let dir = folder(root, &preset.instrument);
    std::fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    let path = dir.join(file_name(&preset.name));
    let json = serde_json::to_string_pretty(preset).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("No se pudo guardar {}: {e}", path.display()))?;
    Ok(path)
}

pub fn delete(root: &Path, preset: &Preset) -> Result<(), String> {
    let path = folder(root, &preset.instrument).join(file_name(&preset.name));
    std::fs::remove_file(&path).map_err(|e| format!("No se pudo borrar {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::model::{AcidParams, SynthParams};

    #[test]
    fn user_presets_save_list_and_delete_per_instrument() {
        let root = std::env::temp_dir().join(format!("pulse-presets-{}", std::process::id()));
        let acid = Preset { name: "Squelch: 303".into(), instrument: InstrumentKind::Acid(AcidParams { resonance: 0.9, ..Default::default() }), samples: Vec::new() };
        let kick = Preset {
            name: "Mi bombo".into(),
            instrument: InstrumentKind::Kick(KickParams {
                tune: 30.0,
                pitch_shape: engine::model::Shape::from_points(&[(0.0, 1.0, 0.0), (0.3, 0.6, 0.5), (1.0, 0.0, -0.4)]),
                ..Default::default()
            }),
            samples: vec!["/clicks/tick.wav".into()],
        };
        save(&root, &acid).unwrap();
        save(&root, &kick).unwrap();
        assert_eq!(list(&root, &InstrumentKind::Acid(AcidParams::default())), vec![acid.clone()]);
        assert_eq!(list(&root, &InstrumentKind::Kick(KickParams::default())), vec![kick.clone()]);
        assert!(list(&root, &InstrumentKind::Synth(SynthParams::default())).is_empty());
        assert!(save(&root, &Preset { name: "  ".into(), ..acid.clone() }).is_err());

        // Saving under the same name replaces the preset.
        let louder = Preset { instrument: InstrumentKind::Acid(AcidParams { gain: 0.9, ..Default::default() }), ..acid.clone() };
        save(&root, &louder).unwrap();
        assert_eq!(list(&root, &acid.instrument), vec![louder.clone()]);
        delete(&root, &louder).unwrap();
        assert!(list(&root, &acid.instrument).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_kick_designer_has_factory_presets() {
        let presets = factory(&InstrumentKind::Kick(KickParams::default()));
        assert!(presets.len() >= 15);
        assert!(presets.iter().any(|p| matches!(p.instrument, InstrumentKind::Kick(k) if k.pitch_shape.count > 2)), "some use drawn curves");
    }
}
