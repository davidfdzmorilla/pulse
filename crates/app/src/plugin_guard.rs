//! A plugin that takes Pulse down doesn't take the project with it. Before a
//! plugin is loaded a note goes to disk (`plugin-cargando.json`, gone once it
//! loaded), and while the audio thread is inside one, the flight recorder says
//! which (see `engine::plugin::flight`). If Pulse closes in the middle of
//! either, the next start finds out which plugin it was and offers to open
//! without it: this time only, or for good (Ajustes → Plugins keeps the list
//! of blocked plugins). The project keeps the plugin and its settings: once
//! unblocked, it loads as it was.
//!
//! Plugins in a process of their own (so that one going down only silences
//! itself) would go further; see the notes at the end of this file.

use crate::app::PulseApp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const LOADING: &str = "plugin-cargando.json";
const TAGS: &str = "plugins-en-uso.json";
const FLIGHT: &str = "plugin-procesando.bin";

/// A plugin, as the project names it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginKey {
    pub name: String,
    /// Its bundle.
    pub path: String,
    /// Its class (VST3) or plugin (CLAP) id.
    pub cid: String,
}

impl PluginKey {
    pub fn of(plugin: &crate::project::PluginRef) -> Self {
        PluginKey { name: plugin.name.clone(), path: plugin.path.clone(), cid: plugin.cid.clone() }
    }

    /// The same plugin (wherever the project names it from, by its class).
    pub fn matches(&self, plugin: &crate::project::PluginRef) -> bool {
        self.cid == plugin.cid && same_path(&self.path, &plugin.path)
    }
}

fn same_path(a: &str, b: &str) -> bool {
    a == b || matches!((Path::new(a).canonicalize(), Path::new(b).canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// What Pulse was doing with a plugin when it closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Loading,
    Processing,
}

/// The notes and the flight recorder, in a folder.
pub struct Guard {
    dir: PathBuf,
    /// The plugins loaded since the start, by the tag their processors record.
    tags: Mutex<BTreeMap<u64, PluginKey>>,
}

/// Tags for the flight recorder: never 0 (nothing), one per instance.
static NEXT_TAG: AtomicU64 = AtomicU64::new(1);

impl Guard {
    pub fn new(dir: PathBuf) -> Self {
        Guard { dir, tags: Mutex::new(BTreeMap::new()) }
    }

    /// Pulse's own: in the recovery folder.
    pub fn standard() -> Self {
        Self::new(paths::data_dir().join("Recuperación"))
    }

    /// The plugin the last Pulse to use this folder was loading or processing
    /// when it closed, if it closed inside one. Only reads.
    pub fn crashed(&self) -> Option<(PluginKey, Stage)> {
        if let Ok(text) = std::fs::read_to_string(self.dir.join(LOADING)) {
            if let Ok(plugin) = serde_json::from_str::<PluginKey>(&text) {
                return Some((plugin, Stage::Loading));
            }
        }
        let tag = engine::plugin::flight::read(&self.dir.join(FLIGHT));
        let tags: BTreeMap<u64, PluginKey> = crate::store::read_json(&self.dir.join(TAGS));
        tags.get(&tag).filter(|_| tag != 0).map(|p| (p.clone(), Stage::Processing))
    }

    /// At the start: what [`Guard::crashed`] says, and from now on the notes
    /// and the flight recorder are this Pulse's (the old ones go).
    pub fn start(&self) -> Option<(PluginKey, Stage)> {
        let crashed = self.crashed();
        let _ = std::fs::remove_file(self.dir.join(LOADING));
        let _ = std::fs::remove_file(self.dir.join(TAGS));
        if let Err(e) = engine::plugin::flight::install(&self.dir.join(FLIGHT)) {
            crate::logs::note(&format!("Sin registro de plugins: {e}"));
        }
        crashed
    }

    /// `plugin` is about to load (UI thread).
    pub fn loading(&self, plugin: &PluginKey) {
        if let Err(e) = crate::store::write_json(&self.dir.join(LOADING), plugin) {
            crate::logs::note(&e);
        }
    }

    /// It loaded (or failed without taking Pulse down).
    pub fn loaded(&self) {
        let _ = std::fs::remove_file(self.dir.join(LOADING));
    }

    /// A tag for a new instance of `plugin`, written down with it, for its
    /// processor to record (see `engine::plugin::flight`).
    pub fn tag(&self, plugin: &PluginKey) -> u64 {
        let tag = NEXT_TAG.fetch_add(1, Ordering::Relaxed);
        let Ok(mut tags) = self.tags.lock() else { return 0 };
        tags.insert(tag, plugin.clone());
        if let Err(e) = crate::store::write_json(&self.dir.join(TAGS), &*tags) {
            crate::logs::note(&e);
        }
        tag
    }
}

/// What the user chose for a plugin that closed Pulse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Not loaded from now on (Ajustes → Plugins lets it back).
    Block,
    /// Not loaded until Pulse closes.
    SkipOnce,
    /// Loaded as usual.
    Load,
}

impl PulseApp {
    /// At the start, before anything loads: if Pulse closed inside a plugin,
    /// asks what to do with it (see [`Choice`]).
    pub fn offer_plugin_recovery(&mut self) {
        let Some(guard) = self.samples.guard.clone() else { return };
        let Some((plugin, stage)) = guard.start() else { return };
        if self.settings.blocked_plugins.contains(&plugin) {
            return;
        }
        let doing = match stage {
            Stage::Loading => "cargaba",
            Stage::Processing => "hacía sonar",
        };
        let description = format!(
            "Pulse se cerró de golpe mientras {doing} el plugin «{}». Puedes abrir tus proyectos sin él: sus pistas quedan en silencio, pero el plugin y sus ajustes siguen en el proyecto, y vuelve a sonar cuando lo desbloquees en Ajustes → Plugins.",
            plugin.name
        );
        let choice = self.ask_about_plugin(&plugin, &description);
        self.status = Some(match choice {
            Choice::Block => {
                let mut settings = self.settings.clone();
                settings.blocked_plugins.push(plugin.clone());
                self.set_settings(settings);
                format!("«{}» está bloqueado: no se carga hasta que lo desbloquees en Ajustes → Plugins", plugin.name)
            }
            Choice::SkipOnce => {
                self.samples.skipped.push(plugin.clone());
                format!("«{}» no se carga hasta que cierres Pulse", plugin.name)
            }
            Choice::Load => format!("«{}» se carga como siempre", plugin.name),
        });
    }

    fn ask_about_plugin(&mut self, plugin: &PluginKey, description: &str) -> Choice {
        if let Some(answer) = self.answers.pop_front() {
            return match answer {
                crate::commands::Answer::Save => Choice::Block,
                crate::commands::Answer::Discard => Choice::SkipOnce,
                _ => Choice::Load,
            };
        }
        if !self.interactive {
            return Choice::SkipOnce;
        }
        let (block, once, load) = ("Bloquearlo".to_string(), "Solo esta vez".to_string(), "Cargarlo".to_string());
        let result = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title(format!("«{}» cerró Pulse", plugin.name))
            .set_description(description)
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(block.clone(), once.clone(), load.clone()))
            .show();
        match result {
            rfd::MessageDialogResult::Custom(label) if label == block => Choice::Block,
            rfd::MessageDialogResult::Custom(label) if label == load => Choice::Load,
            rfd::MessageDialogResult::Yes => Choice::Block,
            rfd::MessageDialogResult::Cancel => Choice::Load,
            // Closing the question: the safe way.
            _ => Choice::SkipOnce,
        }
    }

    /// Lets a blocked plugin load again (Ajustes → Plugins).
    pub fn unblock_plugin(&mut self, plugin: &PluginKey) {
        let mut settings = self.settings.clone();
        settings.blocked_plugins.retain(|p| p != plugin);
        self.samples.skipped.retain(|p| p != plugin);
        self.set_settings(settings);
    }
}

// Plugins in a process of their own: what it would take.
//
// A plugin host process per plugin (or per bundle), `pulse --plugin-host`,
// started on loading: it loads the plugin and runs it; Pulse talks to it
// through shared memory (the audio of each block, its sidechain and its
// outputs, events and parameter changes, in rings in a mapped file) and wakes
// it each block with a semaphore or a futex (on macOS a mach semaphore, on
// Windows an event). The audio thread waits for its answer with a deadline
// (the block's time): a host that doesn't answer in time, or is gone, is
// silence and a message, and Pulse goes on. Its state, parameters and editor
// go through a second channel (a socket) on the UI thread; the editor's window
// would be the host's own (embedding a window of another process is possible
// on macOS only with private APIs, and on Windows with SetParent across
// processes, which many plugins handle badly).
//
// The cost: about a block more of latency for each plugin apart, or a busy
// wait on the audio thread for the host's answer (the way Bitwig and
// Reaper's «dedicated process» do it, which costs CPU); a copy of every
// buffer in and out; one process per plugin (memory: tens of MB each, Kontakt
// and the like hundreds); and a lot of code: the whole VST3/CLAP host behind
// a protocol, the editor in its own window, crash reports. Weeks, not days. It
// is the next step if crashes while playing turn out frequent; meanwhile the
// recovery above keeps projects from being lost to one.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{PluginRef, Project};
    use engine::model::{InstrumentKind, PluginId};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pulse-guard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn synth_ref(id: PluginId) -> PluginRef {
        let bundle = crate::plugins::tests::test_bundle();
        // Its folder is one plugins may load from (as a search makes it).
        crate::plugins::trust(&[bundle.parent().unwrap().to_path_buf()]);
        let info = engine::plugin::scan(&bundle).unwrap().into_iter().find(|p| p.instrument).unwrap();
        PluginRef { id, path: bundle.to_string_lossy().into_owned(), cid: info.cid, name: info.name, vendor: info.vendor, state: String::new(), sidechain: None }
    }

    /// The note of a plugin loading is there while it loads (a plugin that
    /// hangs leaves it behind), and gone once it loaded.
    #[test]
    fn a_plugin_hanging_while_loading_is_found() {
        let dir = scratch("hang");
        let mut project = Project::demo();
        project.plugins.push(synth_ref(PluginId(5)));
        let mut samples = crate::sync::SampleCache::default();
        samples.guard = Some(std::sync::Arc::new(Guard::new(dir.clone())));
        // The test synth at 11 025 Hz waits while this file is there.
        let hold = std::env::temp_dir().join("pulse-testplug-hold");
        std::fs::write(&hold, b"").unwrap();
        let loader = std::thread::spawn(move || crate::sync::load_plugin(&project, PluginId(5), &mut samples, 11_025.0).map(|p| p.tag));
        let started = std::time::Instant::now();
        let seen = loop {
            if let Some(found) = Guard::new(dir.clone()).crashed() {
                break Some(found);
            }
            if started.elapsed() > std::time::Duration::from_secs(20) {
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        std::fs::remove_file(&hold).unwrap();
        let tag = loader.join().unwrap();
        let (plugin, stage) = seen.expect("the note is there while it loads");
        assert_eq!((plugin.name.as_str(), stage), ("Pulse Test Synth", Stage::Loading));
        assert!(tag.is_some_and(|t| t != 0), "loaded, with a tag for the recorder");
        // Loaded: nothing to say any more.
        assert_eq!(Guard::new(dir.clone()).crashed(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real thing, in a process of its own (a test run again with
    /// `PULSE_GUARD_CHILD`): the test synth takes it down while loading (at
    /// 8 000 Hz) or with its first block (8 100 Hz); the next start finds
    /// which plugin and when, and offers to leave it out.
    #[cfg(unix)]
    #[test]
    fn a_plugin_that_crashes_pulse_is_named_at_the_next_start() {
        for (rate, stage) in [(8_000, Stage::Loading), (8_100, Stage::Processing)] {
            let dir = scratch(&format!("crash-{rate}"));
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "plugin_guard::tests::crash_child", "--ignored", "--test-threads", "1"])
                .env("PULSE_GUARD_CHILD", &dir)
                .env("PULSE_GUARD_RATE", rate.to_string())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(!status.success(), "{rate}: the plugin took the process down");
            let found = Guard::new(dir.clone()).crashed();
            assert_eq!(found.as_ref().map(|(p, s)| (p.name.as_str(), *s)), Some(("Pulse Test Synth", stage)), "{rate}");

            // The next start asks; «Bloquearlo» keeps it out, and the project
            // opens with the plugin silent but in it.
            let mut project = Project::demo();
            project.plugins.push(synth_ref(PluginId(5)));
            project.tracks[3].instrument = InstrumentKind::Plugin(PluginId(5));
            let mut app = PulseApp::headless(Project::demo());
            app.samples.guard = Some(std::sync::Arc::new(Guard::new(dir.clone())));
            app.answers.push_back(crate::commands::Answer::Save);
            app.offer_plugin_recovery();
            assert!(app.settings.blocked_plugins.iter().any(|p| p.name == "Pulse Test Synth"), "{:?}", app.status);
            app.replace_project(project, None);
            app.sync_all();
            assert!(!app.samples.plugins.contains_key(&PluginId(5)), "not loaded");
            assert!(app.project.plugin(PluginId(5)).is_some(), "still in the project");
            // Once asked, the notes are this Pulse's: nothing to say next time.
            assert_eq!(Guard::new(dir.clone()).crashed(), None);
            // Unblocked, it loads again.
            let key = app.settings.blocked_plugins[0].clone();
            app.unblock_plugin(&key);
            app.sync_all();
            assert!(app.samples.plugins.contains_key(&PluginId(5)));
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// The process the test above takes down: loads the test synth at the
    /// rate it is given and plays it, as the audio thread would.
    #[test]
    #[ignore = "run by a_plugin_that_crashes_pulse_is_named_at_the_next_start"]
    fn crash_child() {
        let Some(dir) = std::env::var_os("PULSE_GUARD_CHILD").map(PathBuf::from) else { return };
        let rate: f32 = std::env::var("PULSE_GUARD_RATE").unwrap().parse().unwrap();
        let guard = std::sync::Arc::new(Guard::new(dir));
        guard.start();
        let mut project = Project::demo();
        project.plugins.push(synth_ref(PluginId(5)));
        let mut samples = crate::sync::SampleCache::default();
        samples.guard = Some(guard);
        let mut processor = crate::sync::load_plugin(&project, PluginId(5), &mut samples, rate).expect("loads");
        engine::plugin::flight::record_this_thread();
        let (mut l, mut r) = (vec![0f32; 64], vec![0f32; 64]);
        processor.render(&mut l, &mut r);
    }

    /// Asked, each answer does what it says; a blocked plugin doesn't load,
    /// says why, and keeps its place (and state) in the project.
    #[test]
    fn the_answers_block_skip_or_load() {
        let mut project = Project::demo();
        project.plugins.push(synth_ref(PluginId(5)));
        project.tracks[3].instrument = InstrumentKind::Plugin(PluginId(5));
        let key = PluginKey::of(&project.plugins[0]);
        for (answer, blocked, skipped) in [(crate::commands::Answer::Save, true, false), (crate::commands::Answer::Discard, false, true), (crate::commands::Answer::Cancel, false, false)] {
            let dir = scratch("answers");
            std::fs::create_dir_all(&dir).unwrap();
            crate::store::write_json(&dir.join(LOADING), &key).unwrap();
            let mut app = PulseApp::headless(Project::demo());
            app.samples.guard = Some(std::sync::Arc::new(Guard::new(dir.clone())));
            app.answers.push_back(answer);
            app.offer_plugin_recovery();
            assert_eq!(app.settings.blocked_plugins.contains(&key), blocked, "{answer:?}");
            assert_eq!(app.samples.skipped.contains(&key), skipped, "{answer:?}");
            app.replace_project(project.clone(), None);
            app.sync_all();
            assert_eq!(app.samples.plugins.contains_key(&PluginId(5)), !blocked && !skipped, "{answer:?}");
            if blocked || skipped {
                assert!(app.status.as_deref().is_some_and(|s| s.contains("Pulse Test Synth")), "{:?}", app.status);
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
