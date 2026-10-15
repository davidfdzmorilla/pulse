//! Debug aid for screenshots (`PULSE_SCREENSHOT`): `PULSE_SCENE` puts the app in a
//! given state first, as `;`-separated steps:
//! `track=N`, `view=arrangement|session`, `detail=clip|devices`,
//! `instrument=<name>` (as in the browser), `effects=all` or `effects=<a>,<b>`, `learn`,
//! `automation=N` (shows track N's lane), `browser=off`, `clip=N:S` (selects it),
//! `plugin=<name>` / `plugin-effect=<name>` (from the plugin list),
//! `window=kick|wave|poly|modular|settings|shortcuts|updates|export|tables|params`.
//! The window size comes from `PULSE_SIZE=WxH`.

use crate::app::{ClipRef, Detail, PulseApp, View};
use engine::model::{Device, Effect, InstrumentKind, Param};
use engine::Target;

impl PulseApp {
    pub fn apply_scene(&mut self, scene: &str) {
        for step in scene.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let (key, value) = step.split_once('=').unwrap_or((step, ""));
            let track = match self.selected_target {
                Target::Track(t) => t,
                _ => 0,
            };
            match key {
                "track" => {
                    if let Ok(t) = value.parse::<usize>() {
                        self.click_track(t.min(self.project.tracks.len() - 1), Default::default());
                    }
                }
                "view" => self.view = if value == "arrangement" { View::Arrangement } else { View::Session },
                "detail" => self.detail = if value == "clip" { Detail::Clip } else { Detail::Devices },
                "instrument" => {
                    let wanted = value.to_lowercase();
                    if let Some(kind) = InstrumentKind::catalog().into_iter().find(|k| k.label().to_lowercase().contains(&wanted)) {
                        if matches!(kind, InstrumentKind::DrumRack(_)) {
                            self.load_basic_kit(track);
                        } else {
                            self.set_instrument(track, kind);
                        }
                        self.detail = Detail::Devices;
                    }
                }
                "effects" => {
                    // "all", or names as in the browser, comma-separated.
                    let wanted: Vec<String> = value.split(',').map(|n| n.trim().to_lowercase()).collect();
                    let chosen = Effect::catalog().into_iter().filter(|e| value == "all" || wanted.iter().any(|w| e.label().to_lowercase().contains(w.as_str())));
                    for effect in chosen {
                        self.add_device(Target::Track(track), Device::new(effect));
                    }
                    self.detail = Detail::Devices;
                }
                "learn" => self.toggle_learn(),
                // A plugin from the list (by name), as the track's instrument or as an
                // effect; `window=params` then opens its parameter window.
                "plugin" | "plugin-effect" => {
                    let wanted = value.to_lowercase();
                    let found = self.plugin_library.found.iter().filter(|p| p.name.to_lowercase().contains(&wanted)).min_by_key(|p| p.name.len()).cloned();
                    match found {
                        Some(info) if key == "plugin" => self.set_plugin_instrument(track, &info),
                        Some(info) => self.add_plugin_effect(Target::Track(track), &info),
                        None => eprintln!("[escena] plugin no encontrado: {value}"),
                    }
                    self.detail = Detail::Devices;
                }
                "automation" => {
                    if let Ok(t) = value.parse::<usize>() {
                        self.arrangement.automation.insert(t, Param::Volume);
                    }
                }
                "browser" => self.show_browser = value != "off",
                "clip" => {
                    if let Some((t, s)) = value.split_once(':').and_then(|(t, s)| Some((t.parse().ok()?, s.parse().ok()?))) {
                        self.select_clip(ClipRef::Session { track: t, slot: s });
                    }
                }
                "window" => match value {
                    "kick" => self.kick_window = Some(track),
                    "wave" => self.wave_window = Some(track),
                    "poly" => self.poly_window = Some(track),
                    "modular" => self.modular_window = Some(track),
                    "settings" => self.open_settings(),
                    "shortcuts" => self.show_shortcuts = true,
                    "updates" => self.updates.show = true,
                    "export" => self.export.open = true,
                    "tables" => crate::table_editor::open(self, track, 0),
                    "params" => {
                        let id = match self.project.tracks[track].instrument {
                            InstrumentKind::Plugin(id) => Some(id),
                            _ => self.devices(Target::Track(track)).iter().find_map(|d| match d.effect {
                                Effect::Plugin(id) => Some(id),
                                _ => None,
                            }),
                        };
                        self.plugin_library.window = id;
                    }
                    _ => eprintln!("[escena] ventana desconocida: {value}"),
                },
                _ => eprintln!("[escena] paso desconocido: {step}"),
            }
        }
    }
}
