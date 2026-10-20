//! Writing and reading Pulse's own files so a crash, a full disk or a power cut
//! never leaves half of one: everything is written to a temporary file beside
//! it, flushed to the disk, and only then put in its place. A file that can't
//! be read is set aside (never overwritten) and noted in pulse.log.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Writes `bytes` to `path` all at once: either the whole new file is there or
/// the old one still is.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    let name = path.file_name().map_or_else(|| "archivo".to_string(), |n| n.to_string_lossy().into_owned());
    let temporary = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let written = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        // On the disk, not just in its cache (on macOS this is F_FULLFSYNC).
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        sync_dir(dir);
        Ok::<(), std::io::Error>(())
    })();
    written.map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        format!("No se pudo guardar {}: {e}", path.display())
    })
}

/// So the rename itself survives a power cut.
#[cfg(unix)]
fn sync_dir(dir: &Path) {
    if let Ok(dir) = std::fs::File::open(dir) {
        let _ = dir.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) {}

/// Writes `value` as JSON with [`write_atomic`].
pub fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    write_atomic(path, text.as_bytes())
}

/// Reads one of Pulse's JSON files (settings, lists). A missing one is the
/// default. One that can't be read is set aside as «… (dañado)» so the next
/// save doesn't replace it, and the default is used.
pub fn read_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return T::default(),
        Err(e) => {
            crate::logs::note(&format!("No se pudo leer {}: {e}", path.display()));
            return T::default();
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(e) => {
            let aside = set_aside(path);
            crate::logs::note(&format!("{} no se pudo leer ({e}); se apartó como {}", path.display(), aside.as_deref().map_or_else(|| "(no se pudo apartar)".into(), |p| p.display().to_string())));
            T::default()
        }
    }
}

/// Renames a damaged file out of the way, keeping it: «settings.json» becomes
/// «settings.json (dañado)», then «… (dañado 2)»…
pub fn set_aside(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let aside = (1..).map(|n| path.with_file_name(if n == 1 { format!("{name} (dañado)") } else { format!("{name} (dañado {n})") })).find(|p| !p.exists())?;
    std::fs::rename(path, &aside).ok()?;
    Some(aside)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pulse-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn files_are_replaced_whole_and_leave_nothing_behind() {
        let dir = dir("atomic");
        let file = dir.join("ajustes ñ.json");
        write_atomic(&file, b"uno").unwrap();
        write_atomic(&file, b"dos").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"dos");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "no temporary file left: {names:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_write_keeps_the_old_file_and_cleans_up() {
        use std::os::unix::fs::PermissionsExt;
        let dir = dir("readonly");
        let file = dir.join("lista.json");
        write_atomic(&file, b"bueno").unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let error = write_atomic(&file, b"nuevo").unwrap_err();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(error.contains("No se pudo guardar"), "{error}");
        assert_eq!(std::fs::read(&file).unwrap(), b"bueno");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_damaged_file_is_set_aside_not_overwritten() {
        let dir = dir("damaged");
        let file = dir.join("recent.json");
        assert_eq!(read_json::<Vec<String>>(&file), Vec::<String>::new(), "missing: the default");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, "[\"/a\", \"/b").unwrap();
        assert_eq!(read_json::<Vec<String>>(&file), Vec::<String>::new());
        assert!(!file.exists());
        assert_eq!(std::fs::read_to_string(dir.join("recent.json (dañado)")).unwrap(), "[\"/a\", \"/b");
        // A second damaged one doesn't replace the first.
        std::fs::write(&file, "{").unwrap();
        read_json::<Vec<String>>(&file);
        assert!(dir.join("recent.json (dañado 2)").exists());
        write_json(&file, &vec!["/c"]).unwrap();
        assert_eq!(read_json::<Vec<String>>(&file), vec!["/c".to_string()]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
