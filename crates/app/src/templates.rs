//! Templates: projects to start new ones from. One is a project without its
//! content (clips, the arrangement, automation and markers): its tracks,
//! instruments, effects, samples, mix, returns and tempo. Pulse brings two of
//! its own (a beat, a voice recording); the user's are saved in
//! `Plantillas/<name>/<name>.pulse` with their samples, as projects are, and one
//! of them (or Pulse's) can be what ⌘N starts from (Settings).

use crate::app::PulseApp;
use crate::project::{Project, Track, PALETTE};
use engine::model::{AcidParams, CompressorParams, Device, DrumModel, DrumParams, Effect, Eq3Params, FmParams, InstrumentKind, SuperSawParams};
use std::path::PathBuf;

/// The empty project's name, for the choice of what ⌘N starts from.
pub const EMPTY: &str = "Vacío";

/// Pulse's own templates.
pub const BUILT_IN: [&str; 2] = ["Beat", "Grabar voz"];

/// A template: one of Pulse's, or a file of the user's.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub name: String,
    pub file: Option<PathBuf>,
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
        None => built_in(&template.name).ok_or(format!("No hay ninguna plantilla «{}»", template.name)),
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

fn built_in(name: &str) -> Option<Project> {
    let mut project = Project::empty();
    match name {
        "Beat" => {
            project.tempo = 124.0;
            let mut drums = Track::new("Batería", PALETTE[1], InstrumentKind::DrumMachine(DrumParams { model: DrumModel::Tr909, ..Default::default() }));
            drums.devices.push(Device::new(Effect::Compressor(CompressorParams { threshold_db: -14.0, ratio: 3.0, attack_ms: 8.0, release_ms: 90.0, makeup_db: 1.5, sidechain: None })));
            let bass = Track::new("Bajo", PALETTE[9], InstrumentKind::Acid(AcidParams::default()));
            let mut chords = Track::new("Acordes", PALETTE[6], InstrumentKind::SuperSaw(SuperSawParams { gain: 0.3, ..Default::default() }));
            chords.sends[0] = 0.3;
            let mut lead = Track::new("Melodía", PALETTE[11], InstrumentKind::Fm(FmParams::default()));
            lead.sends[..2].copy_from_slice(&[0.15, 0.25]);
            project.tracks = vec![drums, bass, chords, lead];
        }
        "Grabar voz" => {
            project.tempo = 90.0;
            let backing = crate::project::Track::new_audio("Base", PALETTE[9]);
            let voice = |name: &str, color, reverb| {
                let mut track = Track::new_audio(name, color);
                // Less rumble and mud, a little more presence; even level.
                track.devices.push(Device::new(Effect::Eq3(Eq3Params { low_db: -6.0, high_db: 2.0, low_freq: 150.0, ..Default::default() })));
                track.devices.push(Device::new(Effect::Compressor(CompressorParams { threshold_db: -20.0, ratio: 3.0, attack_ms: 5.0, release_ms: 80.0, makeup_db: 4.0, sidechain: None })));
                track.sends[0] = reverb;
                track
            };
            project.tracks = vec![backing, voice("Voz", PALETTE[0], 0.2), voice("Coros", PALETTE[4], 0.35)];
        }
        _ => return None,
    }
    project.assign_ids();
    Some(project)
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
            Some(template) => self.start_from(&template),
            None => self.new_project(),
        }
    }

    fn start_from(&mut self, template: &Template) {
        match project(template) {
            Ok(project) => {
                self.replace_project(project, None);
                self.status = Some(format!("Proyecto nuevo desde la plantilla «{}»", template.name));
            }
            Err(error) => {
                self.status = Some(format!("No se pudo abrir la plantilla «{}»: {error}", template.name));
                self.templates = all();
            }
        }
    }

    /// "Guardar como plantilla…": asks for a name, in the templates' folder.
    pub fn save_as_template(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter("Plantilla de Pulse", &["pulse"])
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
            self.status = Some("La plantilla necesita un nombre".into());
            return;
        }
        self.capture_plugins();
        let mut template = without_content(&self.project);
        let dir = crate::library::folder(crate::library::TEMPLATES).join(&name);
        let collected = crate::library::collect_copies(&mut template, &dir);
        let result = std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|_| template.save(&dir.join(format!("{name}.pulse"))));
        self.status = Some(match result {
            Ok(_) if collected.missing > 0 => format!("Plantilla «{name}» guardada · {} audio(s) no encontrados", collected.missing),
            Ok(_) => format!("Plantilla «{name}» guardada: está en Archivo ▸ Nuevo desde plantilla"),
            Err(error) => format!("No se pudo guardar la plantilla: {error}"),
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
            assert!(!project.tracks.is_empty(), "{name}");
            // As saved and opened again.
            let again = Project::from_json(&project.to_json().unwrap()).unwrap();
            assert_eq!(again.tracks.len(), project.tracks.len());
        }
        assert!(built_in("Nada").is_none());
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

    #[test]
    fn the_users_template_wins_over_pulses_of_the_same_name() {
        let mine = Template { name: "Beat".into(), file: Some(PathBuf::from("/x/Beat/Beat.pulse")) };
        let list = [Template { name: "Beat".into(), file: None }, mine.clone()];
        assert_eq!(find(&list, "Beat"), Some(mine));
        assert_eq!(find(&list, "Otra"), None);
    }
}
