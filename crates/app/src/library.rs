//! Pulse's folder, where everything the user makes lives (~/Music/Pulse by
//! default; Settings can move it):
//!
//! - `Proyectos/<name>/<name>.pulse`, each project in a folder of its own with
//!   every audio file it uses in `Samples/` (imported, recorded, stems, slices,
//!   consolidated),
//!   so it can be moved or copied to another Mac whole;
//! - `Exportaciones/`, `Presets/`, `Wavetables/` and `Samples/` (the user's own);
//! - `Sin guardar/`: what a project not saved yet records or makes; saving moves
//!   it into the project's folder.
//!
//! Settings, caches and models stay in Application Support: they aren't the
//! user's work.

use crate::project::Project;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

pub const PROJECTS: &str = "Proyectos";
pub const EXPORTS: &str = "Exportaciones";
pub const PRESETS: &str = "Presets";
pub const WAVETABLES: &str = "Wavetables";
pub const SAMPLES: &str = "Samples";
pub const UNSAVED: &str = "Sin guardar";

/// Where a project's media go, inside its `Samples/` folder.
pub const IMPORTED: &str = "Importados";
pub const RECORDINGS: &str = "Grabaciones";
pub const STEMS: &str = "Stems";
pub const SLICES: &str = "Trozos";
pub const CONSOLIDATED: &str = "Consolidados";
pub const FROZEN: &str = "Congelados";

/// The folder chosen in Settings, if not the default.
static CHOSEN: RwLock<Option<PathBuf>> = RwLock::new(None);

pub fn set_root(folder: Option<PathBuf>) {
    if let Ok(mut chosen) = CHOSEN.write() {
        *chosen = folder;
    }
}

/// Pulse's folder (a test script keeps its own with its output).
pub fn root() -> PathBuf {
    if let Some(dir) = crate::script::out_dir() {
        return dir.join("Pulse");
    }
    if let Some(folder) = CHOSEN.read().ok().and_then(|c| c.clone()) {
        return folder;
    }
    default_root()
}

pub fn default_root() -> PathBuf {
    // Tests never touch the user's folder.
    if cfg!(test) {
        return std::env::temp_dir().join(format!("pulse-test-library-{}", std::process::id()));
    }
    match std::env::var_os("HOME") {
        Some(home) => Path::new(&home).join("Music/Pulse"),
        None => std::env::temp_dir().join("Pulse"),
    }
}

/// A folder of Pulse's (created if missing).
pub fn folder(name: &str) -> PathBuf {
    let dir = root().join(name);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Creates the folders, and brings in presets and wavetables kept in Application
/// Support by older versions (copied: projects may point at those wavetables).
pub fn prepare() {
    for name in [PROJECTS, EXPORTS, PRESETS, WAVETABLES, SAMPLES] {
        folder(name);
    }
    let old = crate::kits::kits_dir();
    for name in [PRESETS, WAVETABLES] {
        let _ = copy_missing(&old.with_file_name(name), &root().join(name));
    }
}

/// Copies the files of `from` (recursively) that `to` lacks.
fn copy_missing(from: &Path, to: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(from)?.flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_missing(&entry.path(), &target)?;
        } else if !target.exists() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Where a project saved at `file` keeps its media of `kind`, or where an unsaved
/// one does until it is saved.
pub fn media_dir(file: Option<&Path>, kind: &str) -> PathBuf {
    // (A lone .pulse from before keeps them next to it, as it did.)
    match file.and_then(Path::parent) {
        Some(dir) => dir.join(SAMPLES).join(kind),
        None => root().join(UNSAVED).join(kind),
    }
}

/// Whether a project file is in a folder of its own name (`Song/Song.pulse`).
pub fn in_own_folder(file: &Path) -> bool {
    let stem = file.file_stem();
    stem.is_some() && file.parent().and_then(Path::file_name) == stem
}

/// Where to save a project the user named `chosen`: in a folder of its name (made
/// next to `chosen`), unless `chosen` already is in one.
pub fn project_file(chosen: &Path) -> PathBuf {
    if in_own_folder(chosen) {
        return chosen.to_path_buf();
    }
    let stem = chosen.file_stem().map_or("Sin título".into(), |s| s.to_string_lossy().into_owned());
    chosen.parent().unwrap_or(Path::new(".")).join(&stem).join(format!("{stem}.pulse"))
}

/// Every audio file a project uses: its audio clips and its tracks' samples.
fn files_mut(project: &mut Project) -> Vec<&mut String> {
    let mut files = Vec::new();
    for track in &mut project.tracks {
        for clip in track.clips.iter_mut().flatten() {
            if let engine::model::Clip::Audio(audio) = clip {
                files.push(&mut audio.sample);
            }
        }
        for placed in &mut track.arrangement {
            if let engine::model::Clip::Audio(audio) = &mut placed.clip {
                files.push(&mut audio.sample);
            }
        }
        files.extend(track.samples.iter_mut());
        if let Some(frozen) = &mut track.frozen {
            for placed in &mut frozen.arrangement {
                if let engine::model::Clip::Audio(audio) = &mut placed.clip {
                    files.push(&mut audio.sample);
                }
            }
            files.extend(frozen.clips.iter_mut().flatten().map(|a| &mut a.sample));
        }
    }
    files.retain(|f| !f.is_empty());
    files
}

/// What saving brought into the project's folder.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Collected {
    pub copied: usize,
    pub moved: usize,
    pub missing: usize,
}

/// Brings every audio file the project uses into `dir/Samples`: files of an unsaved
/// project (in `Sin guardar`) are moved, the rest copied. The project then points
/// at them. Files already in `dir` stay.
pub fn collect(project: &mut Project, dir: &Path) -> Collected {
    let unsaved = root().join(UNSAVED);
    let mut done: HashMap<String, String> = HashMap::new();
    let mut report = Collected::default();
    let mut emptied: Vec<PathBuf> = Vec::new();
    for file in files_mut(project) {
        if let Some(new) = done.get(file.as_str()) {
            *file = new.clone();
            continue;
        }
        let source = PathBuf::from(file.as_str());
        if source.starts_with(dir) {
            continue;
        }
        if !source.is_file() {
            report.missing += 1;
            continue;
        }
        // Unsaved media keep their kind (Grabaciones, Stems…); the rest are imported.
        let (kind, rest) = match source.strip_prefix(&unsaved) {
            Ok(rest) => (None, rest.to_path_buf()),
            Err(_) => (Some(IMPORTED), PathBuf::from(source.file_name().unwrap_or_default())),
        };
        let target = unique(&match kind {
            Some(kind) => dir.join(SAMPLES).join(kind).join(&rest),
            None => dir.join(SAMPLES).join(&rest),
        });
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let moved = kind.is_none() && std::fs::rename(&source, &target).is_ok();
        if moved {
            report.moved += 1;
            emptied.extend(source.parent().map(Path::to_path_buf));
        } else if std::fs::copy(&source, &target).is_ok() {
            report.copied += 1;
        } else {
            report.missing += 1;
            continue;
        }
        let new = target.to_string_lossy().into_owned();
        done.insert(file.clone(), new.clone());
        *file = new;
    }
    // The folders the moved files leave empty go too.
    for left in emptied {
        let mut dir = left.as_path();
        while dir.starts_with(&unsaved) && dir != unsaved && std::fs::remove_dir(dir).is_ok() {
            dir = dir.parent().unwrap_or(&unsaved);
        }
    }
    report
}

/// `path`, or `path 2`, `path 3`… if taken.
pub fn unique(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned());
    let extension = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..).map(|n| path.with_file_name(format!("{stem} {n}{extension}"))).find(|p| !p.exists()).expect("a free name")
}

/// The project as saved in `dir`: files inside it as paths relative to it, so the
/// folder can be moved.
pub fn relative(project: &Project, dir: &Path) -> Project {
    let mut project = project.clone();
    for file in files_mut(&mut project) {
        if let Ok(rest) = Path::new(file.as_str()).strip_prefix(dir) {
            *file = rest.to_string_lossy().into_owned();
        }
    }
    project
}

/// The project as loaded from `dir`: relative paths made whole again.
pub fn resolve(project: &mut Project, dir: &Path) {
    for file in files_mut(project) {
        if Path::new(file.as_str()).is_relative() {
            *file = dir.join(file.as_str()).to_string_lossy().into_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::model::{AudioClip, Clip, WarpMode};

    fn audio(path: &Path) -> Clip {
        Clip::Audio(AudioClip {
            name: "a".into(),
            color: [0; 3],
            sample: path.to_string_lossy().into_owned(),
            length: 4.0,
            offset: 0.0,
            warp: WarpMode::Off,
            original_bpm: 120.0,
            warp_markers: Vec::new(),
            gain_db: 0.0,
            transpose: 0.0,
            envelopes: Vec::new(),
        })
    }

    #[test]
    fn projects_go_in_a_folder_of_their_name() {
        assert_eq!(project_file(Path::new("/m/Proyectos/Canción.pulse")), Path::new("/m/Proyectos/Canción/Canción.pulse"));
        assert_eq!(project_file(Path::new("/m/Proyectos/Canción/Canción.pulse")), Path::new("/m/Proyectos/Canción/Canción.pulse"));
        assert!(in_own_folder(Path::new("/x/Tema/Tema.pulse")));
        assert!(!in_own_folder(Path::new("/x/Documents/Tema.pulse")));
    }

    #[test]
    fn saving_brings_every_file_into_the_project_folder() {
        let base = std::env::temp_dir().join(format!("pulse-library-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let outside = base.join("Descargas");
        std::fs::create_dir_all(&outside).unwrap();
        let (song, kick) = (outside.join("song.wav"), outside.join("kick.wav"));
        std::fs::write(&song, b"song").unwrap();
        std::fs::write(&kick, b"kick").unwrap();
        // A recording made before the project was saved.
        let recording = root().join(UNSAVED).join(RECORDINGS).join(format!("toma-{}.wav", std::process::id()));
        std::fs::create_dir_all(recording.parent().unwrap()).unwrap();
        std::fs::write(&recording, b"take").unwrap();

        let mut project = Project::empty();
        project.tracks[0].clips[0] = Some(audio(&song));
        project.tracks[0].clips[1] = Some(audio(&song));
        project.tracks[0].clips[2] = Some(audio(&recording));
        project.tracks[1].samples = vec![String::new(), kick.to_string_lossy().into_owned(), "/no/such.wav".into()];

        let dir = base.join("Proyectos").join("Tema");
        let report = collect(&mut project, &dir);
        assert_eq!(report, Collected { copied: 2, moved: 1, missing: 1 });
        let sample = |t: usize, s: usize| match &project.tracks[t].clips[s] {
            Some(Clip::Audio(a)) => a.sample.clone(),
            _ => panic!(),
        };
        assert_eq!(sample(0, 0), dir.join("Samples/Importados/song.wav").to_string_lossy());
        assert_eq!(sample(0, 1), sample(0, 0), "one copy of a file used twice");
        assert_eq!(sample(0, 2), dir.join("Samples/Grabaciones").join(recording.file_name().unwrap()).to_string_lossy());
        assert!(!recording.exists(), "the unsaved take moved in");
        assert!(!recording.parent().unwrap().exists(), "its folder, empty, went too");
        assert!(song.exists(), "imported files are copied, not moved");
        assert_eq!(project.tracks[1].samples[1], dir.join("Samples/Importados/kick.wav").to_string_lossy());
        assert_eq!(project.tracks[1].samples[2], "/no/such.wav", "missing files are left as they were");

        // Saved with paths relative to the folder; loaded back from wherever it is.
        let saved = relative(&project, &dir);
        let Some(Clip::Audio(a)) = &saved.tracks[0].clips[0] else { panic!() };
        assert_eq!(a.sample, "Samples/Importados/song.wav");
        assert_eq!(saved.tracks[1].samples[2], "/no/such.wav");
        let mut moved = saved.clone();
        resolve(&mut moved, Path::new("/Volumes/Otro/Tema"));
        let Some(Clip::Audio(a)) = &moved.tracks[0].clips[0] else { panic!() };
        assert_eq!(a.sample, "/Volumes/Otro/Tema/Samples/Importados/song.wav");

        // Saved again: nothing to bring in (the missing one is still missing); a new
        // file with a taken name gets another.
        assert_eq!(collect(&mut project, &dir), Collected { missing: 1, ..Default::default() });
        let other = base.join("Otra").join("song.wav");
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::write(&other, b"other").unwrap();
        project.tracks[0].clips[3] = Some(audio(&other));
        collect(&mut project, &dir);
        let Some(Clip::Audio(a)) = &project.tracks[0].clips[3] else { panic!() };
        assert_eq!(a.sample, dir.join("Samples/Importados/song 2.wav").to_string_lossy());
        std::fs::remove_dir_all(&base).ok();
    }
}
