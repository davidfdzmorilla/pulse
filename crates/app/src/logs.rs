//! Pulse's error log: when Pulse fails (a panic, on any thread) what happened and
//! where is added to `pulse.log` in the logs folder (macOS:
//! ~/Library/Logs/Pulse, Windows: %LOCALAPPDATA%\Pulse\Logs), so a window that
//! closed by itself leaves a trace. The log keeps the latest 1 MB.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Past this the log starts again (the old one kept as pulse.1.log).
const LIMIT: u64 = 1 << 20;

pub fn file() -> PathBuf {
    paths::logs_dir().join("pulse.log")
}

/// Sets the panic hook (tests keep Rust's).
pub fn install() {
    // Its system worked out now, not while failing.
    let _ = system();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current().name().unwrap_or("sin nombre").to_string();
        let message = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        let place = info.location().map_or(String::new(), |l| format!("{}:{}", l.file(), l.line()));
        let trace = std::backtrace::Backtrace::force_capture();
        let _ = append(&file(), &format!("Pulse {} — fallo en el hilo «{thread}» ({place}): {message}\n{}\n{trace}", crate::updates::VERSION, system()));
        previous(info);
    }));
}

/// What Pulse runs on, for an error report: its commit, the system and its
/// version, the processor.
pub fn system() -> String {
    format!("commit {} · {} {} · {}", crate::updates::COMMIT, std::env::consts::OS, os_version(), std::env::consts::ARCH)
}

#[cfg(target_os = "macos")]
fn os_version() -> String {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION
        .get_or_init(|| {
            std::process::Command::new("/usr/bin/sw_vers").arg("-productVersion").output().ok().and_then(|o| String::from_utf8(o.stdout).ok()).map(|s| s.trim().to_string()).unwrap_or_default()
        })
        .clone()
}

#[cfg(not(target_os = "macos"))]
fn os_version() -> String {
    String::new()
}

/// Notes something that went wrong without stopping Pulse (a damaged file set
/// aside…). Tests only print it: the log is the user's.
pub fn note(entry: &str) {
    eprintln!("{entry}");
    #[cfg(not(test))]
    let _ = append(&file(), &format!("Pulse {}: {entry}", crate::updates::VERSION));
}

/// Adds an entry, with the time (seconds since 1970, UTC).
fn append(path: &Path, entry: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > LIMIT) {
        let _ = std::fs::rename(path, path.with_file_name("pulse.1.log"));
    }
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "[{seconds}] {entry}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An error report says the commit, the system and its version, and the processor.
    #[test]
    fn reports_say_what_pulse_runs_on() {
        let system = system();
        assert!(system.starts_with(&format!("commit {}", crate::updates::COMMIT)), "{system}");
        assert!(system.contains(std::env::consts::OS) && system.ends_with(std::env::consts::ARCH), "{system}");
        if cfg!(target_os = "macos") {
            assert!(system.split(' ').any(|w| w.split('.').count() >= 2 && w.chars().next().is_some_and(|c| c.is_ascii_digit())), "the macOS version: {system}");
        }
        assert_ne!(crate::updates::COMMIT, "?", "built from git");
    }

    #[test]
    fn entries_are_added_and_a_full_log_starts_again() {
        let dir = std::env::temp_dir().join(format!("pulse logs ñ {}", std::process::id()));
        let log = dir.join("pulse.log");
        append(&log, "uno").unwrap();
        append(&log, "dos").unwrap();
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("] uno\n") && text.contains("] dos\n"), "{text}");
        std::fs::write(&log, vec![b'x'; LIMIT as usize + 1]).unwrap();
        append(&log, "tres").unwrap();
        assert!(std::fs::read_to_string(&log).unwrap().ends_with("] tres\n"));
        assert!(dir.join("pulse.1.log").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
