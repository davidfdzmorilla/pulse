//! Where Pulse keeps what isn't the user's work, on each system, and where the
//! user's own folder starts out:
//!
//! | | macOS | Windows | Linux |
//! |---|---|---|---|
//! | settings | ~/Library/Application Support/Pulse | %APPDATA%\Pulse | $XDG_CONFIG_HOME/pulse |
//! | data (kits, models) | ~/Library/Application Support/Pulse | %LOCALAPPDATA%\Pulse | $XDG_DATA_HOME/pulse |
//! | caches | ~/Library/Caches/Pulse | %LOCALAPPDATA%\Pulse\Cache | $XDG_CACHE_HOME/pulse |
//! | logs | ~/Library/Logs/Pulse | %LOCALAPPDATA%\Pulse\Logs | $XDG_DATA_HOME/pulse/logs |
//! | Pulse's folder | ~/Music/Pulse | Music\Pulse | ~/Music/Pulse |
//!
//! On macOS these are where Pulse always kept them. On Windows the settings roam
//! with the user's profile, and what is big or can be made again (kits, models,
//! caches) stays on the machine. The folders come from the system (Windows'
//! known folders, so a Music folder moved to another disk is followed), the
//! environment when it doesn't say, and the temporary folder as a last resort.
//! Nothing here creates folders: [`ensure`] does.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

impl Os {
    pub const CURRENT: Os = if cfg!(target_os = "macos") {
        Os::Mac
    } else if cfg!(windows) {
        Os::Windows
    } else {
        Os::Linux
    };
}

/// The system's folders the layout is built on (each may be unknown).
#[derive(Clone, Debug, Default)]
pub struct Bases {
    pub home: Option<PathBuf>,
    /// Windows: the roaming application data (%APPDATA%). Linux: $XDG_CONFIG_HOME.
    pub config: Option<PathBuf>,
    /// Windows: the local application data (%LOCALAPPDATA%). Linux: $XDG_DATA_HOME.
    pub local: Option<PathBuf>,
    /// Linux: $XDG_CACHE_HOME.
    pub cache: Option<PathBuf>,
    /// The user's Music folder.
    pub music: Option<PathBuf>,
    pub temp: PathBuf,
}

/// Pulse's folders.
#[derive(Clone, Debug, PartialEq)]
pub struct Dirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    pub library: PathBuf,
}

/// The folders for `os` on `bases`.
pub fn layout(os: Os, bases: &Bases) -> Dirs {
    let fallback = || bases.temp.join("Pulse");
    let home = |rest: &str| bases.home.as_ref().map(|h| h.join(rest));
    let music = bases.music.clone().or_else(|| home("Music")).map(|m| m.join("Pulse"));
    match os {
        Os::Mac => {
            let support = home("Library/Application Support/Pulse").unwrap_or_else(fallback);
            Dirs {
                config: support.clone(),
                data: support,
                cache: home("Library/Caches/Pulse").unwrap_or_else(|| fallback().join("Cache")),
                logs: home("Library/Logs/Pulse").unwrap_or_else(|| fallback().join("Logs")),
                library: music.unwrap_or_else(|| fallback().join("Library")),
            }
        }
        Os::Windows => {
            let local = bases.local.as_ref().map(|l| l.join("Pulse")).unwrap_or_else(fallback);
            Dirs {
                config: bases.config.as_ref().map(|c| c.join("Pulse")).unwrap_or_else(|| local.clone()),
                cache: local.join("Cache"),
                logs: local.join("Logs"),
                library: music.unwrap_or_else(|| local.join("Library")),
                data: local,
            }
        }
        Os::Linux => {
            let xdg = |base: &Option<PathBuf>, rest: &str| base.clone().or_else(|| home(rest)).map(|b| b.join("pulse"));
            let data = xdg(&bases.local, ".local/share").unwrap_or_else(fallback);
            Dirs {
                config: xdg(&bases.config, ".config").unwrap_or_else(|| data.clone()),
                cache: xdg(&bases.cache, ".cache").unwrap_or_else(|| data.join("cache")),
                logs: data.join("logs"),
                library: music.unwrap_or_else(|| data.join("library")),
                data,
            }
        }
    }
}

/// This system's folders.
pub fn bases() -> Bases {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let mut bases = Bases { temp: std::env::temp_dir(), ..Bases::default() };
    match Os::CURRENT {
        Os::Mac => {
            bases.home = env("HOME");
        }
        Os::Windows => {
            bases.home = known::home().or_else(|| env("USERPROFILE"));
            bases.config = known::roaming().or_else(|| env("APPDATA"));
            bases.local = known::local().or_else(|| env("LOCALAPPDATA"));
            bases.music = known::music();
        }
        Os::Linux => {
            bases.home = env("HOME");
            bases.config = env("XDG_CONFIG_HOME");
            bases.local = env("XDG_DATA_HOME");
            bases.cache = env("XDG_CACHE_HOME");
            bases.music = env("XDG_MUSIC_DIR");
        }
    }
    bases
}

/// This system's layout.
pub fn dirs() -> Dirs {
    layout(Os::CURRENT, &bases())
}

/// Settings (settings.json, midi.json, plugins.json, recent.json).
pub fn config_dir() -> PathBuf {
    dirs().config
}

/// Data Pulse makes and keeps: generated kits, downloaded models.
pub fn data_dir() -> PathBuf {
    dirs().data
}

/// What can be made again (models unpacked for the runtime, analyses).
pub fn cache_dir() -> PathBuf {
    dirs().cache
}

/// Error reports.
pub fn logs_dir() -> PathBuf {
    dirs().logs
}

/// Pulse's folder when the user hasn't chosen another.
pub fn default_library() -> PathBuf {
    dirs().library
}

/// `dir`, created if missing.
pub fn ensure(dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    Ok(dir.to_path_buf())
}

/// The user's home folder shown as "~" in a path, where it starts with it.
pub fn shorten(path: &Path) -> String {
    match bases().home {
        Some(home) => match path.strip_prefix(&home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
            Ok(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
            Err(_) => path.display().to_string(),
        },
        None => path.display().to_string(),
    }
}

#[cfg(windows)]
mod known {
    use std::path::PathBuf;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_LocalAppData, FOLDERID_Music, FOLDERID_Profile, FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    use windows_sys::core::GUID;

    /// A known folder, where the system has it (moved folders included).
    fn folder(id: &GUID) -> Option<PathBuf> {
        let mut path: *mut u16 = std::ptr::null_mut();
        let result = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT as _, std::ptr::null_mut(), &mut path) };
        let found = (result == 0 && !path.is_null()).then(|| {
            let len = (0..).take_while(|&i| unsafe { *path.add(i) } != 0).count();
            let units = unsafe { std::slice::from_raw_parts(path, len) };
            use std::os::windows::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_wide(units))
        });
        // Freed even when the call fails.
        unsafe { CoTaskMemFree(path as *const _) };
        found.filter(|p| !p.as_os_str().is_empty())
    }

    pub fn home() -> Option<PathBuf> {
        folder(&FOLDERID_Profile)
    }

    pub fn roaming() -> Option<PathBuf> {
        folder(&FOLDERID_RoamingAppData)
    }

    pub fn local() -> Option<PathBuf> {
        folder(&FOLDERID_LocalAppData)
    }

    pub fn music() -> Option<PathBuf> {
        folder(&FOLDERID_Music)
    }
}

#[cfg(not(windows))]
mod known {
    use std::path::PathBuf;

    pub fn home() -> Option<PathBuf> {
        None
    }

    pub fn roaming() -> Option<PathBuf> {
        None
    }

    pub fn local() -> Option<PathBuf> {
        None
    }

    pub fn music() -> Option<PathBuf> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn macos_keeps_where_pulse_always_kept_things() {
        let bases = Bases { home: Some(p("/Users/ana")), temp: p("/tmp"), ..Bases::default() };
        let dirs = layout(Os::Mac, &bases);
        assert_eq!(dirs.config, p("/Users/ana/Library/Application Support/Pulse"));
        assert_eq!(dirs.data, dirs.config);
        assert_eq!(dirs.cache, p("/Users/ana/Library/Caches/Pulse"));
        assert_eq!(dirs.logs, p("/Users/ana/Library/Logs/Pulse"));
        assert_eq!(dirs.library, p("/Users/ana/Music/Pulse"));
    }

    #[test]
    fn windows_roams_settings_and_keeps_the_rest_on_the_machine() {
        let bases = Bases {
            home: Some(p(r"C:\Users\Ana María")),
            config: Some(p(r"C:\Users\Ana María\AppData\Roaming")),
            local: Some(p(r"C:\Users\Ana María\AppData\Local")),
            music: Some(p(r"D:\Música")),
            cache: None,
            temp: p(r"C:\Temp"),
        };
        let dirs = layout(Os::Windows, &bases);
        assert_eq!(dirs.config, p(r"C:\Users\Ana María\AppData\Roaming").join("Pulse"));
        assert_eq!(dirs.data, p(r"C:\Users\Ana María\AppData\Local").join("Pulse"));
        assert_eq!(dirs.cache, dirs.data.join("Cache"));
        assert_eq!(dirs.logs, dirs.data.join("Logs"));
        // A Music folder moved to another disk is followed.
        assert_eq!(dirs.library, p(r"D:\Música").join("Pulse"));
    }

    #[test]
    fn windows_without_a_music_folder_uses_the_home_one() {
        let bases = Bases { home: Some(p(r"C:\Users\ana")), local: Some(p(r"C:\L")), temp: p(r"C:\Temp"), ..Bases::default() };
        let dirs = layout(Os::Windows, &bases);
        assert_eq!(dirs.library, p(r"C:\Users\ana").join("Music").join("Pulse"));
        // Without the roaming folder the settings stay with the data.
        assert_eq!(dirs.config, p(r"C:\L").join("Pulse"));
    }

    #[test]
    fn linux_follows_xdg_and_falls_back_to_home() {
        let bases = Bases { home: Some(p("/home/ana")), config: Some(p("/cfg")), temp: p("/tmp"), ..Bases::default() };
        let dirs = layout(Os::Linux, &bases);
        assert_eq!(dirs.config, p("/cfg/pulse"));
        assert_eq!(dirs.data, p("/home/ana/.local/share/pulse"));
        assert_eq!(dirs.cache, p("/home/ana/.cache/pulse"));
        assert_eq!(dirs.library, p("/home/ana/Music/Pulse"));
    }

    #[test]
    fn with_nothing_known_everything_goes_to_the_temporary_folder() {
        let bases = Bases { temp: p("/tmp"), ..Bases::default() };
        for os in [Os::Mac, Os::Windows, Os::Linux] {
            let dirs = layout(os, &bases);
            for dir in [&dirs.config, &dirs.data, &dirs.cache, &dirs.logs, &dirs.library] {
                assert!(dir.starts_with("/tmp"), "{os:?}: {}", dir.display());
            }
            // Never two kinds of thing in one folder that's cleared as a cache.
            assert_ne!(dirs.cache, dirs.data, "{os:?}");
        }
    }

    #[test]
    fn this_system_has_absolute_folders() {
        let dirs = dirs();
        for dir in [&dirs.config, &dirs.data, &dirs.cache, &dirs.logs, &dirs.library] {
            assert!(dir.is_absolute(), "{}", dir.display());
        }
    }

    #[test]
    fn ensure_creates_folders_with_spaces_and_accents() {
        let dir = std::env::temp_dir().join(format!("pulse paths ñandú {}", std::process::id())).join("Sub carpeta");
        assert_eq!(ensure(&dir).unwrap(), dir);
        assert!(dir.is_dir());
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }
}
