//! Updates from the project's GitHub repository (public). Each release there has a
//! `Pulse-<version>.zip` with the signed Pulse.app (see `scripts/release.sh`).
//!
//! Pulse looks for the latest release every time it starts, every few hours while
//! it runs, and from Pulse → Buscar actualizaciones…. A new version (unless the
//! user skipped it) opens the window, which shows what's new and installs it:
//! it downloads the zip, checks that the app inside is that version and signed
//! like this one, keeps this version as "Pulse (anterior).app" next to it, puts
//! the new one in its place and restarts. "Volver a la versión anterior" swaps
//! them back. GitHub is reached with `curl`, with no account needed; a token,
//! if there is one (the GitHub CLI's, `gh auth token`, or one pasted in the
//! window, kept in the login keychain), is sent too, and left out if GitHub
//! turns it down (expired, revoked).
//!
//! On Windows the release has `Pulse-<version>-windows-x64.zip` (see
//! `scripts/package-windows.ps1`) and what is replaced is `pulse.exe` itself
//! (a running program can be renamed there, not deleted): this one is kept as
//! "pulse (anterior).exe". The download must match the SHA-256 GitHub gives for
//! it, and the program inside must say it is that version (its version
//! resource). There is no signing certificate yet, so a download is checked for
//! being the file published, not for who made it. `curl.exe` and `tar.exe` come
//! with Windows 10 and 11; a token pasted in the window is kept in Windows'
//! Credential Manager.

use crate::app::PulseApp;
use crate::theme::TEXT_DIM;
use eframe::egui;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// The repository releases come from.
pub const REPOSITORY: &str = "davidfdzmorilla/pulse";
/// This build's version: the release's (scripts/release.sh sets PULSE_VERSION from
/// the new tag), or, for builds of our own, git's (the last release and how far
/// past it, see build.rs), or Cargo's without git.
pub const VERSION: &str = match (option_env!("PULSE_VERSION"), option_env!("PULSE_GIT_VERSION")) {
    (Some(version), _) => version,
    (None, Some(version)) => version,
    (None, None) => env!("CARGO_PKG_VERSION"),
};
/// The commit it was built from, if known.
pub const COMMIT: &str = match option_env!("PULSE_COMMIT") {
    Some(commit) => commit,
    None => "?",
};
/// The keychain's (macOS) or Credential Manager's (Windows) name for the token.
const KEYCHAIN_SERVICE: &str = "Pulse GitHub";
/// The version kept beside this one, and where this one waits while they swap.
#[cfg(not(windows))]
const PREVIOUS: &str = "Pulse (anterior).app";
#[cfg(not(windows))]
const PARKED: &str = ".Pulse-cambio.app";
#[cfg(windows)]
const PREVIOUS: &str = "pulse (anterior).exe";
#[cfg(windows)]
const PARKED: &str = ".pulse-cambio.exe";
/// The previous version kept before, while a new one goes in (see [`swap_in`]).
#[cfg(not(windows))]
const OLDER: &str = ".Pulse-anterior-vieja.app";
#[cfg(windows)]
const OLDER: &str = ".pulse-anterior-vieja.exe";
/// Pulse's signing identifier (Info.plist's CFBundleIdentifier).
#[cfg(not(windows))]
const BUNDLE_ID: &str = "io.github.davidfdzmorilla.pulse";
/// The end of the name of this system's release asset.
#[cfg(not(windows))]
const PACKAGE: &str = ".zip";
#[cfg(windows)]
const PACKAGE: &str = "-windows-x64.zip";
/// While Pulse stays open, it looks again this often.
const CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(Release),
    /// Installed: restart to use it.
    Installed(String),
    /// The first start of a new version: its notes.
    WhatsNew(Release),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Asset {
    pub name: String,
    /// The API address (it works for private repositories).
    pub url: String,
    #[serde(default)]
    pub size: u64,
    /// "sha256:<hex>", as GitHub works it out on upload.
    #[serde(default)]
    pub digest: Option<String>,
}

impl Release {
    pub fn version(&self) -> &str {
        self.tag_name.trim_start_matches('v')
    }

    /// This system's package (the macOS one is the zip that isn't Windows').
    /// This system's package: by its exact name, so other files of the release
    /// (the debugging symbols, `Pulse-X.Y.Z-simbolos.zip`) are never taken for it.
    fn zip(&self) -> Option<&Asset> {
        let name = format!("Pulse-{}{PACKAGE}", self.version());
        self.assets.iter().find(|a| a.name == name)
    }

    /// Pulse's own signature of the zip (`<zip>.firma`, see the `firma` crate).
    #[cfg_attr(not(windows), allow(dead_code))]
    fn signature(&self) -> Option<&Asset> {
        let name = format!("{}.firma", self.zip()?.name);
        self.assets.iter().find(|a| a.name == name)
    }
}

/// The updater: its state (shared with the thread doing the work) and window.
pub struct Updates {
    pub state: Arc<Mutex<State>>,
    pub show: bool,
    /// A token typed in the window.
    pub token_input: String,
    /// An automatic check is under way: open the window if it finds something.
    pub announce: bool,
    pub last_check: Option<std::time::Instant>,
}

impl Default for Updates {
    fn default() -> Self {
        Self { state: Arc::new(Mutex::new(State::Idle)), show: false, token_input: String::new(), announce: false, last_check: None }
    }
}

impl Updates {
    pub fn state(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or(State::Idle)
    }

    fn set(&self, state: State) {
        set(&self.state, state);
    }
}

fn set(shared: &Mutex<State>, state: State) {
    if let Ok(mut s) = shared.lock() {
        *s = state;
    }
}

/// Compares versions like 0.10.2 and 0.9.7 by their numbers.
pub fn newer(candidate: &str, current: &str) -> bool {
    let parts = |v: &str| v.split(['.', '-']).map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (a, b) = (parts(candidate), parts(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

// ----- GitHub ----------------------------------------------------------------------

/// A token for the GitHub API: the GitHub CLI's, else the one kept in the keychain
/// (Credential Manager on Windows).
pub(crate) fn token() -> Option<String> {
    // A bare name is looked for on the PATH; a full path must be there.
    let candidates = system::GH.iter().filter(|p| !p.contains(['/', '\\']) || Path::new(p).exists());
    let from_gh = candidates.into_iter().find_map(|gh| output(command(gh).args(["auth", "token"])));
    from_gh.or_else(system::stored_token)
}

/// A program run without a console window of its own (Windows opens one for
/// each otherwise).
fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn output(command: &mut Command) -> Option<String> {
    let out = command.stderr(Stdio::null()).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

/// Starts curl, with the token (if any) in its configuration (read from stdin,
/// so it never shows in the process list), writing the body to `to` if given;
/// `resume`: going on from what `to` already has. A slow line is fine as long
/// as it moves (under 1 KB/s for a minute is given up), and a dropped one is
/// tried again.
pub(crate) fn spawn_curl(url: &str, token: Option<&str>, accept: &str, to: Option<&Path>, resume: bool) -> Result<std::process::Child, String> {
    let mut command = command(system::curl());
    // Only HTTPS, also where GitHub redirects to (its downloads go to another
    // host), and a few redirects at most.
    command.args(["--silent", "--show-error", "--fail", "--location", "--proto", "=https", "--proto-redir", "=https", "--max-redirs", "5"]);
    command.args(["--connect-timeout", "20", "--speed-limit", "1024", "--speed-time", "60", "--retry", "3", "--config", "-"]);
    if let Some(path) = to {
        command.arg("--output").arg(path);
        if resume {
            command.args(["--continue-at", "-"]);
        }
    }
    let mut child = command
        .arg(url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("No se pudo ejecutar curl: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let config = curl_config(token, accept);
        stdin.write_all(config.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(child)
}

/// curl's configuration: the headers, with the token's only if there is one.
fn curl_config(token: Option<&str>, accept: &str) -> String {
    let authorization = token.map_or_else(String::new, |token| format!("header = \"Authorization: Bearer {token}\"\n"));
    format!("{authorization}header = \"Accept: {accept}\"\nheader = \"X-GitHub-Api-Version: 2022-11-28\"\nuser-agent = \"Pulse/{VERSION}\"\n")
}

/// Whether a release is news here: newer than this one, with a package for
/// this system (a release only for macOS isn't one on Windows, and back).
fn news(release: &Release) -> bool {
    newer(release.version(), VERSION) && release.zip().is_some()
}

/// Runs curl (see [`spawn_curl`]). Returns the body, or writes it to `to`. A
/// token GitHub turns down (expired, revoked) is left out and it asks again:
/// the repository is public.
fn curl(url: &str, token: Option<&str>, accept: &str, to: Option<&Path>) -> Result<Vec<u8>, String> {
    let run = |token: Option<&str>| -> Result<Vec<u8>, String> {
        let out = spawn_curl(url, token, accept, to, false)?.wait_with_output().map_err(|e| e.to_string())?;
        if out.status.success() { Ok(out.stdout) } else { Err(String::from_utf8_lossy(&out.stderr).into_owned()) }
    };
    match run(token) {
        Err(e) if token.is_some() && refused(&e) => run(None),
        result => result,
    }
    .map_err(|e| curl_error(&e))
}

/// Whether curl's error is GitHub turning the request down (not a missing page).
pub(crate) fn refused(error: &str) -> bool {
    error.contains("401") || error.contains("403")
}

/// curl's error, in words.
pub(crate) fn curl_error(error: &str) -> String {
    if error.contains("404") {
        format!("No se encontró en {REPOSITORY}")
    } else if refused(error) {
        "GitHub no responde a más consultas por ahora (tiene un límite por hora): prueba dentro de un rato, o pega un token en «Acceso a GitHub»".to_string()
    } else if error.contains("resolve host") || error.contains("connect") {
        "Sin conexión con GitHub".to_string()
    } else {
        format!("Error al hablar con GitHub: {}", error.trim())
    }
}

fn latest_release(token: Option<&str>) -> Result<Release, String> {
    release_at(token, "latest")
}

/// `which`: "latest" or "tags/v1.2.3".
pub(crate) fn release_at(token: Option<&str>, which: &str) -> Result<Release, String> {
    let body = curl(&format!("https://api.github.com/repos/{REPOSITORY}/releases/{which}"), token, "application/vnd.github+json", None)?;
    serde_json::from_slice(&body).map_err(|e| format!("Respuesta de GitHub no válida: {e}"))
}

// ----- Installing --------------------------------------------------------------------

/// What an update replaces, if this program can be updated: the Pulse.app it
/// runs from (macOS), or itself when it is a published build (Windows; ones
/// built with cargo can't be).
pub fn running_bundle() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    if cfg!(windows) {
        return option_env!("PULSE_VERSION").is_some().then_some(executable);
    }
    bundle_of(&executable)
}

#[cfg_attr(windows, allow(dead_code))]
fn bundle_of(executable: &Path) -> Option<PathBuf> {
    // …/Pulse.app/Contents/MacOS/pulse
    let bundle = executable.parent()?.parent()?.parent()?;
    (bundle.extension().is_some_and(|e| e == "app")).then(|| bundle.to_path_buf())
}

use system::bundle_version;

/// Checks a download against the SHA-256 GitHub gives for it. Windows requires
/// one (it has no signature to check otherwise); macOS checks it when there is.
fn check_digest(file: &Path, asset: &Asset) -> Result<(), String> {
    let Some(expected) = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) else {
        return if cfg!(windows) { Err("GitHub no da la huella (SHA-256) de la descarga: no se instala".into()) } else { Ok(()) };
    };
    let actual = crate::stems_ui::sha256(file)?;
    if actual.eq_ignore_ascii_case(expected) { Ok(()) } else { Err("La descarga no coincide con la publicada (SHA-256 distinto): no se instala".into()) }
}

/// Whether `zip` is signed by Pulse's own key (`signature`, the text of its
/// `.firma`): on Windows nothing else says who made a download, as the code
/// signature does on macOS. GitHub's SHA-256 only says it arrived whole.
#[cfg_attr(not(windows), allow(dead_code))]
fn check_signature(zip: &Path, signature: &str) -> Result<(), String> {
    check_signature_with(&firma::PUBLIC_KEY, zip, signature)
}

/// [`check_signature`] against the key `public`.
#[cfg_attr(not(windows), allow(dead_code))]
fn check_signature_with(public: &[u8; 32], zip: &Path, signature: &str) -> Result<(), String> {
    let data = std::fs::read(zip).map_err(|e| format!("No se pudo leer la descarga: {e}"))?;
    firma::verify(public, &data, signature).map_err(|e| format!("{e}: no se instala"))
}

/// Who signed a bundle ("adhoc" for ad hoc signatures), if the signature is valid.
#[cfg(not(windows))]
/// `strict` also refuses stray file attributes: right for a fresh download, but
/// an installed copy picks some up from the system (Finder info, iCloud).
fn signer(bundle: &Path, strict: bool) -> Result<String, String> {
    let mut verify = Command::new("/usr/bin/codesign");
    verify.arg("--verify");
    if strict {
        verify.arg("--strict");
    }
    let verify = verify.arg(bundle).output().map_err(|e| e.to_string())?;
    if !verify.status.success() {
        return Err(format!("La firma de {} no es válida", bundle.display()));
    }
    let details = Command::new("/usr/bin/codesign").args(["-dvv"]).arg(bundle).output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&details.stderr);
    Ok(text
        .lines()
        .find_map(|l| l.strip_prefix("Authority="))
        .map(str::to_string)
        .unwrap_or_else(|| "adhoc".into()))
}

/// Checks that `new` is validly signed with the very certificate `ours` was (its
/// designated requirement names the certificate's hash, not just its name). An
/// ad hoc signed `ours` (a build that was never signed properly) accepts any valid
/// signature of Pulse (its identifier).
#[cfg(not(windows))]
fn same_signer(ours: &Path, new: &Path) -> Result<(), String> {
    let theirs = signer(new, true)?;
    // A broken signature here must not open the door to any download.
    let mine = signer(ours, false).map_err(|e| format!("No se puede comprobar esta versión de Pulse: {e}"))?;
    if mine == "adhoc" {
        let pulse = Command::new("/usr/bin/codesign")
            .args(["--verify", &format!("-R=identifier \"{BUNDLE_ID}\"")])
            .arg(new)
            .output()
            .map_err(|e| e.to_string())?;
        return if pulse.status.success() { Ok(()) } else { Err("La descarga no es Pulse (otro identificador de firma): no se instala".into()) };
    }
    let requirement = output(Command::new("/usr/bin/codesign").args(["-d", "-r-"]).arg(ours))
        .and_then(|text| text.lines().find_map(|l| l.strip_prefix("designated => ").map(str::to_string)))
        .ok_or("No se pudo leer la firma de esta versión de Pulse")?;
    let check = Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict"])
        .arg(format!("-R={requirement}"))
        .arg(new)
        .output()
        .map_err(|e| e.to_string())?;
    if check.status.success() {
        Ok(())
    } else {
        Err(format!("La versión descargada está firmada por «{theirs}» con otro certificado que esta: no se instala"))
    }
}

/// Puts `new` where `current` is, keeping `current` as "Pulse (anterior).app"
/// beside it (replacing an older one). If anything fails, everything stays as
/// it was: `current`, and the previous version kept before (set aside first,
/// removed only once the new one is in).
pub fn swap_in(current: &Path, new: &Path) -> Result<(), String> {
    let previous = current.with_file_name(PREVIOUS);
    let older = current.with_file_name(OLDER);
    let _ = remove(&older);
    let had_previous = previous.exists();
    if had_previous {
        std::fs::rename(&previous, &older).map_err(|e| format!("No se pudo apartar la versión anterior guardada: {e}"))?;
    }
    let undo_previous = || {
        if had_previous {
            let _ = std::fs::rename(&older, &previous);
        }
    };
    if let Err(e) = std::fs::rename(current, &previous) {
        undo_previous();
        return Err(format!("No se pudo apartar esta versión: {e}"));
    }
    if let Err(e) = std::fs::rename(new, current) {
        let _ = std::fs::rename(&previous, current);
        undo_previous();
        return Err(format!("No se pudo instalar la nueva versión: {e}"));
    }
    let _ = remove(&older);
    Ok(())
}

/// Puts the previous version back, keeping this one as the previous.
pub fn swap_back(current: &Path) -> Result<(), String> {
    let previous = current.with_file_name(PREVIOUS);
    if !previous.exists() {
        return Err("No hay ninguna versión anterior guardada".into());
    }
    let parked = current.with_file_name(PARKED);
    let _ = remove(&parked);
    std::fs::rename(current, &parked).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&previous, current) {
        let _ = std::fs::rename(&parked, current);
        return Err(e.to_string());
    }
    std::fs::rename(&parked, &previous).map_err(|e| e.to_string())
}

/// Removes a bundle (a folder) or a program (a file).
fn remove(path: &Path) -> std::io::Result<()> {
    if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }
}

/// Downloads a release and installs it over `bundle`. Returns the new version.
fn install(release: &Release, bundle: &Path, token: Option<&str>) -> Result<String, String> {
    let asset = release.zip().ok_or("La versión publicada no tiene ningún .zip")?;
    replaceable(bundle)?;
    // Next to the bundle, so the final move stays on the same disk (one per
    // process: two Pulses open don't share it).
    let work = bundle.with_file_name(format!(".pulse-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("No se pudo preparar la descarga: {e}"))?;
    let result = (|| {
        let zip = work.join(&asset.name);
        curl(&asset.url, token, "application/octet-stream", Some(&zip))?;
        check_digest(&zip, asset)?;
        // On Windows, signed by Pulse's key or not installed.
        #[cfg(windows)]
        {
            let signature = release.signature().ok_or("La versión publicada no está firmada por Pulse: no se instala")?;
            let text = curl(&signature.url, token, "application/octet-stream", None)?;
            check_signature(&zip, &String::from_utf8_lossy(&text))?;
        }
        system::unzip(&zip, &work)?;
        let new = work.join(system::IN_PACKAGE);
        let version = bundle_version(&new).ok_or(format!("La descarga no contiene {}", system::IN_PACKAGE))?;
        if version != release.version() {
            return Err(format!("La descarga es la versión {version}, no la {}", release.version()));
        }
        #[cfg(not(windows))]
        same_signer(bundle, &new)?;
        // The runtime DLLs first: if that fails, this pulse.exe stays, and it
        // runs with newer ones. The other way round, a failure would leave the
        // new pulse.exe with the old DLLs.
        #[cfg(windows)]
        system::update_libraries(new.parent().unwrap_or(&work), bundle.parent().unwrap_or(Path::new(".")))?;
        swap_in(bundle, &new)?;
        Ok(version)
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Whether Pulse can replace itself where it is, or why not in words: opened
/// straight from Downloads (macOS runs it from a hidden read-only copy) or from
/// a folder this user can't write.
fn replaceable(bundle: &Path) -> Result<(), String> {
    if bundle.to_string_lossy().contains("/AppTranslocation/") {
        return Err("Pulse se está ejecutando desde una copia temporal de macOS (se abrió sin moverlo de Descargas): muévelo a Aplicaciones, ábrelo desde ahí y vuelve a intentarlo".into());
    }
    let dir = bundle.parent().ok_or("No se sabe dónde está Pulse")?;
    let probe = dir.join(format!(".pulse-escritura-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(_) => Err(format!("No se puede escribir en {} (la carpeta de Pulse): instala Pulse en una carpeta tuya, como Aplicaciones de tu usuario, o actualiza a mano", dir.display())),
    }
}

use system::relaunch;

/// What differs between systems: where the tools are, how the token is kept, what
/// a package holds and how Pulse is started again.
#[cfg(not(windows))]
mod system {
    use super::*;

    /// The GitHub CLI, where Homebrew puts it.
    pub const GH: &[&str] = &["/opt/homebrew/bin/gh", "/usr/local/bin/gh"];
    /// What the package unzips to.
    pub const IN_PACKAGE: &str = "Pulse.app";

    pub fn curl() -> PathBuf {
        PathBuf::from("/usr/bin/curl")
    }

    pub fn stored_token() -> Option<String> {
        output(Command::new("/usr/bin/security").args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"]))
    }

    /// Keeps a token in the login keychain.
    /// The token goes to `security` through its standard input (`-i`), so it
    /// never shows in the process list.
    pub fn remember_token(token: &str) -> Result<(), String> {
        use std::io::Write;
        // GitHub's tokens are letters, digits and underscores; anything else isn't one.
        if token.is_empty() || !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err("Eso no parece un token de GitHub".into());
        }
        let mut child = Command::new("/usr/bin/security").arg("-i").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
        if let Some(mut stdin) = child.stdin.take() {
            writeln!(stdin, "add-generic-password -U -s \"{KEYCHAIN_SERVICE}\" -a github -w {token}").map_err(|e| e.to_string())?;
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        // `-i` exits well whatever the command did: its complaints are on stderr.
        if out.status.success() && out.stderr.is_empty() { Ok(()) } else { Err("No se pudo guardar el token en el llavero".into()) }
    }

    /// The bundle's version, from its Info.plist.
    pub fn bundle_version(bundle: &Path) -> Option<String> {
        output(Command::new("/usr/libexec/PlistBuddy").args(["-c", "Print :CFBundleShortVersionString"]).arg(bundle.join("Contents/Info.plist")))
    }

    pub fn unzip(zip: &Path, to: &Path) -> Result<(), String> {
        let unzip = Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(zip).arg(to).status().map_err(|e| e.to_string())?;
        if unzip.success() { Ok(()) } else { Err("No se pudo descomprimir la descarga".to_string()) }
    }

    /// Opens `bundle` once this process has quit.
    pub fn relaunch(bundle: &Path) -> Result<(), String> {
        let script = format!("while /bin/kill -0 {} 2>/dev/null; do /bin/sleep 0.2; done; /usr/bin/open \"$0\"", std::process::id());
        Command::new("/bin/sh").arg("-c").arg(script).arg(bundle).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
mod system {
    use super::*;
    use windows_sys::Win32::Security::Credentials::{CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW, CredWriteW};

    /// The GitHub CLI: on the PATH, or where its installer puts it.
    pub const GH: &[&str] = &["gh", r"C:\Program Files\GitHub CLI\gh.exe"];
    /// What the package unzips to.
    pub const IN_PACKAGE: &str = r"Pulse\pulse.exe";

    fn system32(program: &str) -> PathBuf {
        let root = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        root.join("System32").join(program)
    }

    /// The curl that comes with Windows 10 and 11.
    pub fn curl() -> PathBuf {
        system32("curl.exe")
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn stored_token() -> Option<String> {
        let target = wide(KEYCHAIN_SERVICE);
        let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 || credential.is_null() {
            return None;
        }
        let token = unsafe {
            let c = &*credential;
            let bytes = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
            String::from_utf8(bytes.to_vec()).ok()
        };
        unsafe { CredFree(credential as *const _) };
        token.filter(|t| !t.trim().is_empty())
    }

    /// Keeps a token in Credential Manager (Windows credentials, "Pulse GitHub").
    pub fn remember_token(token: &str) -> Result<(), String> {
        let (target, user) = (wide(KEYCHAIN_SERVICE), wide("github"));
        let mut blob = token.as_bytes().to_vec();
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_ptr() as *mut u16,
            Comment: std::ptr::null_mut(),
            LastWritten: unsafe { std::mem::zeroed() },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: std::ptr::null_mut(),
            UserName: user.as_ptr() as *mut u16,
        };
        if unsafe { CredWriteW(&credential, 0) } != 0 { Ok(()) } else { Err("No se pudo guardar el token en el Administrador de credenciales".into()) }
    }

    /// A program's version, from its version resource (see the app's build.rs).
    pub fn bundle_version(program: &Path) -> Option<String> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW};
        let path: Vec<u16> = program.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), std::ptr::null_mut()) };
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr() as *mut _) } == 0 {
            return None;
        }
        let (mut info, mut len): (*mut std::ffi::c_void, u32) = (std::ptr::null_mut(), 0);
        let root = wide("\\");
        if unsafe { VerQueryValueW(data.as_ptr() as *const _, root.as_ptr(), &mut info, &mut len) } == 0 || info.is_null() {
            return None;
        }
        let fixed = unsafe { &*(info as *const VS_FIXEDFILEINFO) };
        let (ms, ls) = (fixed.dwProductVersionMS, fixed.dwProductVersionLS);
        Some(format!("{}.{}.{}", ms >> 16, ms & 0xffff, ls >> 16))
    }

    /// The DLLs that come with pulse.exe (the Visual C++ runtime) brought up to the
    /// package's: a changed one is renamed to "<name>.anterior" (Windows lets a DLL
    /// in use be renamed, not replaced) and the new one copied in. They stay when
    /// going back to the previous version: newer runtimes run older programs.
    pub fn update_libraries(package: &Path, installed: &Path) -> Result<(), String> {
        let entries = std::fs::read_dir(package).map_err(|e| e.to_string())?;
        for entry in entries.flatten() {
            let new = entry.path();
            if !new.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll")) {
                continue;
            }
            let target = installed.join(entry.file_name());
            if std::fs::read(&target).ok() == std::fs::read(&new).ok() {
                continue;
            }
            if target.exists() {
                let old = target.with_extension("dll.anterior");
                let _ = std::fs::remove_file(&old);
                std::fs::rename(&target, &old).map_err(|e| format!("No se pudo apartar {}: {e}", target.display()))?;
            }
            std::fs::copy(&new, &target).map_err(|e| format!("No se pudo copiar {}: {e}", target.display()))?;
        }
        Ok(())
    }

    /// With the tar that comes with Windows 10 and 11 (it reads zips).
    pub fn unzip(zip: &Path, to: &Path) -> Result<(), String> {
        let unzip = command(system32("tar.exe")).arg("-xf").arg(zip).arg("-C").arg(to).status().map_err(|e| e.to_string())?;
        if unzip.success() { Ok(()) } else { Err("No se pudo descomprimir la descarga".to_string()) }
    }

    /// Starts `program` once this process has quit (PowerShell waits for it).
    pub fn relaunch(program: &Path) -> Result<(), String> {
        let quoted = program.display().to_string().replace('\'', "''");
        let script = format!("Wait-Process -Id {} -ErrorAction SilentlyContinue; Start-Process -FilePath '{quoted}'", std::process::id());
        command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

// ----- In the app --------------------------------------------------------------------

impl PulseApp {
    /// At start: the notes of a version started for the first time, else a look
    /// for a newer one (both in the background; the window opens if there's news).
    pub fn check_updates_on_start(&mut self) {
        if !self.interactive || running_bundle().is_none() {
            return;
        }
        if self.settings.last_version.as_deref() != Some(VERSION) {
            let settings = crate::settings::Settings { last_version: Some(VERSION.to_string()), ..self.settings.clone() };
            self.set_settings(settings);
            self.show_whats_new();
        } else if self.settings.check_updates {
            self.updates.announce = true;
            self.check_updates();
        }
    }

    /// Fetches this version's notes, to show them.
    fn show_whats_new(&mut self) {
        self.updates.announce = true;
        let shared = self.updates.state.clone();
        std::thread::spawn(move || {
            if let Ok(release) = release_at(token().as_deref(), &format!("tags/v{VERSION}")) {
                set(&shared, State::WhatsNew(release));
            }
        });
    }

    /// Every frame: opens the window when an automatic check finds a version not
    /// skipped (or the notes arrive), and looks again every few hours.
    pub fn follow_updates(&mut self) {
        let state = self.updates.state();
        if self.updates.announce {
            match &state {
                State::Checking | State::Idle => {}
                State::Available(release) if self.settings.skipped_version.as_deref() != Some(release.version()) => {
                    self.open_panel(crate::app::Panel::Updates);
                    self.updates.announce = false;
                }
                State::WhatsNew(_) => {
                    self.open_panel(crate::app::Panel::Updates);
                    self.updates.announce = false;
                }
                _ => self.updates.announce = false,
            }
        }
        let due = self.updates.last_check.is_some_and(|t| t.elapsed() >= CHECK_EVERY);
        let quiet = matches!(state, State::Idle | State::UpToDate | State::Failed(_) | State::WhatsNew(_));
        if due && quiet && self.interactive && self.settings.check_updates && running_bundle().is_some() {
            self.updates.announce = true;
            self.check_updates();
        }
    }

    /// Looks for a new version in the background.
    pub fn check_updates(&mut self) {
        if matches!(self.updates.state(), State::Checking | State::Downloading(_)) {
            return;
        }
        self.updates.set(State::Checking);
        self.updates.last_check = Some(std::time::Instant::now());
        let shared = self.updates.state.clone();
        std::thread::spawn(move || {
            let state = match latest_release(token().as_deref()) {
                // Only a version with a package for this system is news.
                Ok(release) if news(&release) => State::Available(release),
                Ok(_) => State::UpToDate,
                Err(e) => State::Failed(e),
            };
            set(&shared, state);
        });
    }

    fn install_update(&mut self, release: Release) {
        let Some(bundle) = running_bundle() else {
            self.updates.set(State::Failed("Pulse no se está ejecutando como aplicación (Pulse.app): no hay nada que reemplazar".into()));
            return;
        };
        self.updates.set(State::Downloading(release.clone()));
        let shared = self.updates.state.clone();
        std::thread::spawn(move || {
            let state = match install(&release, &bundle, token().as_deref()) {
                Ok(version) => State::Installed(version),
                Err(e) => State::Failed(e),
            };
            set(&shared, state);
        });
    }

    /// Restarts into the installed version (asking about unsaved changes first).
    fn restart(&mut self, ctx: &egui::Context) {
        let Some(bundle) = running_bundle() else { return };
        if !self.may_discard() {
            return;
        }
        self.save_settings_now();
        match relaunch(&bundle) {
            Ok(()) => {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => self.updates.set(State::Failed(format!("No se pudo reiniciar: {e}"))),
        }
    }

    /// The little notice in the transport bar when a new version is out.
    pub fn update_badge(&mut self, ui: &mut egui::Ui) {
        let label = match self.updates.state() {
            State::Available(release) if self.settings.skipped_version.as_deref() != Some(release.version()) => {
                format!("↑ Pulse {} disponible", release.version())
            }
            State::Installed(version) => format!("↻ Reinicia para usar Pulse {version}"),
            _ => return,
        };
        if ui.small_button(label).clicked() {
            self.open_panel(crate::app::Panel::Updates);
        }
    }
}

/// Pulse → Buscar actualizaciones…
pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    if !app.updates.show {
        return;
    }
    let mut open = true;
    let state = app.updates.state();
    let mut action = None;
    egui::Window::new("Actualizaciones").open(&mut open).resizable(false).collapsible(false).default_width(460.0).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
        ui.label(format!("Versión instalada: Pulse {VERSION}"));
        ui.label(egui::RichText::new(format!("Las versiones se publican en github.com/{REPOSITORY}")).color(TEXT_DIM).size(10.5));
        ui.separator();
        match &state {
            State::Idle => {
                ui.label("Pulse busca versiones nuevas cada vez que arranca, y cada 6 horas si sigue abierto.");
            }
            State::Checking => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Buscando…");
                });
            }
            State::UpToDate => {
                ui.label("✓ Tienes la última versión.");
            }
            State::Available(release) => {
                ui.heading(format!("Pulse {} disponible", release.version()));
                if let Some(asset) = release.zip() {
                    ui.label(egui::RichText::new(format!("{:.1} MB", asset.size as f64 / 1e6)).color(TEXT_DIM).size(10.5));
                }
                ui.label("Novedades:");
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    ui.label(release.body.as_deref().filter(|b| !b.trim().is_empty()).unwrap_or("(sin notas)"));
                });
                if running_bundle().is_some() {
                    ui.horizontal(|ui| {
                        if ui.button("Descargar e instalar").clicked() {
                            action = Some(Action::Install(release.clone()));
                        }
                        if ui.button("Más tarde").clicked() {
                            action = Some(Action::Later);
                        }
                        if ui.button("Omitir esta versión").on_hover_text("No volver a avisar de esta versión (sí de las siguientes)").clicked() {
                            action = Some(Action::Skip(release.version().to_string()));
                        }
                    });
                    ui.label(egui::RichText::new("Esta versión se guarda como «Pulse (anterior)» para poder volver a ella.").color(TEXT_DIM).size(10.5));
                } else {
                    let outside = if cfg!(windows) {
                        "Este Pulse no es una versión publicada (se compiló aquí): no se puede instalar desde aquí."
                    } else {
                        "Pulse se está ejecutando fuera de Pulse.app (desde el terminal): no se puede instalar desde aquí."
                    };
                    ui.label(egui::RichText::new(outside).color(TEXT_DIM).size(10.5));
                }
            }
            State::Downloading(release) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Descargando e instalando Pulse {}…", release.version()));
                });
            }
            State::Installed(version) => {
                ui.label(format!("✓ Pulse {version} está instalado. Se usará al reiniciar."));
                if ui.button("Reiniciar ahora").clicked() {
                    action = Some(Action::Restart);
                }
            }
            State::WhatsNew(release) => {
                ui.heading(format!("Novedades de Pulse {}", release.version()));
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    ui.label(release.body.as_deref().filter(|b| !b.trim().is_empty()).unwrap_or("(sin notas)"));
                });
                if ui.button("Cerrar").clicked() {
                    action = Some(Action::Later);
                }
            }
            State::Failed(error) => {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            let busy = matches!(state, State::Checking | State::Downloading(_));
            if ui.add_enabled(!busy, egui::Button::new("Buscar ahora")).clicked() {
                action = Some(Action::Check);
            }
            let previous = running_bundle().is_some_and(|b| b.with_file_name(PREVIOUS).exists());
            if ui.add_enabled(previous && !busy, egui::Button::new("Volver a la versión anterior")).clicked() {
                action = Some(Action::Back);
            }
        });
        let mut check = app.settings.check_updates;
        if ui.checkbox(&mut check, "Buscar actualizaciones al arrancar").changed() {
            action = Some(Action::CheckOnStart(check));
        }
        ui.collapsing("Acceso a GitHub (opcional)", |ui| {
            ui.label(
                egui::RichText::new(if cfg!(windows) {
                    "No hace falta: el repositorio es público. Solo si GitHub dice que hay demasiadas consultas: Pulse usa la sesión de GitHub CLI («gh auth login») o un token que pegues aquí (se guarda en el Administrador de credenciales de Windows)."
                } else {
                    "No hace falta: el repositorio es público. Solo si GitHub dice que hay demasiadas consultas: Pulse usa la sesión de GitHub CLI («gh auth login») o un token que pegues aquí (se guarda en el llavero)."
                })
                    .color(TEXT_DIM)
                    .size(10.5),
            );
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut app.updates.token_input).password(true).desired_width(280.0));
                if ui.button("Guardar token").clicked() && !app.updates.token_input.trim().is_empty() {
                    action = Some(Action::SaveToken);
                }
            });
        });
    });
    match action {
        Some(Action::Check) => app.check_updates(),
        Some(Action::Later) => app.updates.show = false,
        Some(Action::Skip(version)) => {
            let settings = crate::settings::Settings { skipped_version: Some(version), ..app.settings.clone() };
            app.set_settings(settings);
            app.updates.show = false;
        }
        Some(Action::CheckOnStart(check)) => {
            let settings = crate::settings::Settings { check_updates: check, ..app.settings.clone() };
            app.set_settings(settings);
        }
        Some(Action::Install(release)) => app.install_update(release),
        Some(Action::Restart) => app.restart(ctx),
        Some(Action::Back) => {
            if let Some(bundle) = running_bundle() {
                match swap_back(&bundle) {
                    Ok(()) => {
                        let version = bundle_version(&bundle).unwrap_or_default();
                        app.updates.set(State::Installed(version));
                    }
                    Err(e) => app.updates.set(State::Failed(e)),
                }
            }
        }
        Some(Action::SaveToken) => {
            let token = std::mem::take(&mut app.updates.token_input);
            match system::remember_token(token.trim()) {
                Ok(()) => app.check_updates(),
                Err(e) => app.updates.set(State::Failed(e)),
            }
        }
        None => {}
    }
    if !open {
        app.updates.show = false;
    }
}

enum Action {
    Check,
    Later,
    Skip(String),
    CheckOnStart(bool),
    Install(Release),
    Restart,
    Back,
    SaveToken,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(newer("0.1.1", "0.1.0"));
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("1.0", "0.99.99"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
    }

    #[test]
    fn releases_are_read_from_githubs_answer() {
        let json = r#"{"tag_name":"v0.2.0","name":"Pulse 0.2.0","body":"- Algo nuevo","assets":[
            {"name":"notes.txt","url":"https://api.github.com/x/1","size":3},
            {"name":"Pulse-0.2.0.zip","url":"https://api.github.com/repos/o/r/releases/assets/2","size":12000000,"extra":1},
            {"name":"Pulse-0.2.0-windows-x64.zip","url":"https://api.github.com/repos/o/r/releases/assets/3","size":9000000}]}"#;
        let release: Release = serde_json::from_str(json).unwrap();
        assert_eq!(release.version(), "0.2.0");
        let package = if cfg!(windows) { "Pulse-0.2.0-windows-x64.zip" } else { "Pulse-0.2.0.zip" };
        assert_eq!(release.zip().map(|a| a.name.as_str()), Some(package));
        // The symbols beside it are not the package, whatever their order.
        let symbols = Asset { name: "Pulse-0.2.0-simbolos.zip".into(), url: String::new(), size: 1, digest: None };
        let mut with_symbols = release.clone();
        with_symbols.assets.insert(0, symbols);
        if !cfg!(windows) {
            assert_eq!(with_symbols.zip().map(|a| a.name.as_str()), Some("Pulse-0.2.0.zip"));
        }
        assert!(newer(release.version(), "0.1.0"));
    }

    #[test]
    fn each_system_takes_its_own_package_and_checks_its_digest() {
        let json = r#"{"tag_name":"v0.2.0","assets":[
            {"name":"Pulse-0.2.0-windows-x64.zip","url":"https://api.github.com/a/1","size":9,"digest":"sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"},
            {"name":"Pulse-0.2.0.zip","url":"https://api.github.com/a/2","size":12}]}"#;
        let release: Release = serde_json::from_str(json).unwrap();
        let zip = release.zip().unwrap();
        assert_eq!(zip.name, if cfg!(windows) { "Pulse-0.2.0-windows-x64.zip" } else { "Pulse-0.2.0.zip" });
        let file = std::env::temp_dir().join(format!("pulse digest ñ {}.zip", std::process::id()));
        std::fs::write(&file, b"abc").unwrap();
        let with = |digest: Option<&str>| Asset { name: "x.zip".into(), url: String::new(), size: 3, digest: digest.map(str::to_string) };
        assert_eq!(check_digest(&file, &release.assets[0]), Ok(()));
        assert!(check_digest(&file, &with(Some("sha256:00"))).is_err(), "another file is refused");
        // Without a digest only Windows refuses (macOS checks the signature instead).
        assert_eq!(check_digest(&file, &with(None)).is_err(), cfg!(windows));
        std::fs::remove_file(&file).unwrap();
    }

    /// The Windows zip is installed only signed by Pulse's key: its own
    /// signature, of that very zip, beside it in the release.
    #[test]
    fn a_download_must_carry_pulses_signature() {
        let dir = std::env::temp_dir().join(format!("pulse-firma-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let zip = dir.join("Pulse-0.2.0-windows-x64.zip");
        std::fs::write(&zip, b"el paquete").unwrap();
        let ours = [3u8; 32];
        let signed = firma::sign(&ours, b"el paquete");
        assert_eq!(check_signature_with(&firma::public_of(&ours), &zip, &signed), Ok(()));
        assert!(check_signature_with(&firma::public_of(&[4u8; 32]), &zip, &signed).is_err(), "another key's");
        std::fs::write(&zip, b"otro paquete").unwrap();
        assert!(check_signature_with(&firma::public_of(&ours), &zip, &signed).is_err(), "a changed zip");
        // Against the key built in, a signature made with any other is refused.
        assert!(check_signature(&zip, &firma::sign(&ours, b"otro paquete")).unwrap_err().ends_with("no se instala"));
        std::fs::remove_dir_all(&dir).ok();
        // The signature is found beside the zip in the release.
        let asset = |name: &str| Asset { name: name.into(), url: String::new(), size: 1, digest: None };
        let release = Release { tag_name: "v0.2.0".into(), body: None, assets: vec![asset(&format!("Pulse-0.2.0{PACKAGE}")), asset(&format!("Pulse-0.2.0{PACKAGE}.firma"))] };
        assert_eq!(release.signature().map(|a| a.name.as_str()), Some(format!("Pulse-0.2.0{PACKAGE}.firma").as_str()));
        let unsigned = Release { assets: release.assets[..1].to_vec(), ..release.clone() };
        assert!(unsigned.signature().is_none());
    }

    #[test]
    #[cfg(windows)]
    fn the_runtime_libraries_follow_the_package() {
        let dir = std::env::temp_dir().join(format!("pulse dll ñ {}", std::process::id()));
        let (package, installed) = (dir.join("Pulse"), dir.join("instalado"));
        std::fs::create_dir_all(&package).unwrap();
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(package.join("vcruntime140.dll"), b"nueva").unwrap();
        std::fs::write(package.join("msvcp140.dll"), b"igual").unwrap();
        std::fs::write(package.join("pulse.exe"), b"programa").unwrap();
        std::fs::write(installed.join("vcruntime140.dll"), b"vieja").unwrap();
        std::fs::write(installed.join("msvcp140.dll"), b"igual").unwrap();
        system::update_libraries(&package, &installed).unwrap();
        assert_eq!(std::fs::read(installed.join("vcruntime140.dll")).unwrap(), b"nueva");
        assert_eq!(std::fs::read(installed.join("vcruntime140.dll.anterior")).unwrap(), b"vieja");
        assert!(!installed.join("msvcp140.dll.anterior").exists(), "unchanged ones stay");
        assert!(!installed.join("pulse.exe").exists(), "only libraries");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[cfg(not(windows))]
    fn the_bundle_is_found_from_the_executable() {
        assert_eq!(bundle_of(Path::new("/Applications/Pulse.app/Contents/MacOS/pulse")), Some(PathBuf::from("/Applications/Pulse.app")));
        assert_eq!(bundle_of(Path::new("/x/target/release/pulse")), None);
    }

    #[test]
    fn versions_are_swapped_keeping_the_previous_one() {
        let dir = std::env::temp_dir().join(format!("pulse-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let make = |path: &Path, version: &str| {
            std::fs::create_dir_all(path.join("Contents")).unwrap();
            std::fs::write(path.join("Contents/version"), version).unwrap();
        };
        let read = |path: &Path| std::fs::read_to_string(path.join("Contents/version")).ok();
        let current = dir.join("Pulse.app");
        make(&current, "1");
        make(&dir.join("new/Pulse.app"), "2");
        swap_in(&current, &dir.join("new/Pulse.app")).unwrap();
        assert_eq!((read(&current), read(&dir.join(PREVIOUS))), (Some("2".into()), Some("1".into())));
        // A third version replaces the kept one.
        make(&dir.join("new/Pulse.app"), "3");
        swap_in(&current, &dir.join("new/Pulse.app")).unwrap();
        assert_eq!((read(&current), read(&dir.join(PREVIOUS))), (Some("3".into()), Some("2".into())));
        // Back and forth.
        swap_back(&current).unwrap();
        assert_eq!((read(&current), read(&dir.join(PREVIOUS))), (Some("2".into()), Some("3".into())));
        // A missing new version leaves the current one in place, and the
        // previous one kept, nothing else beside them.
        assert!(swap_in(&current, &dir.join("missing.app")).is_err());
        assert_eq!((read(&current), read(&dir.join(PREVIOUS))), (Some("2".into()), Some("3".into())));
        assert!(!dir.join(OLDER).exists());
        std::fs::remove_dir_all(&dir).ok();
        assert!(swap_back(&current).is_err());
    }

    /// Without a token no Authorization header goes; with one, it does.
    #[test]
    fn a_token_is_only_sent_when_there_is_one() {
        assert!(!curl_config(None, "application/json").contains("Authorization"));
        assert!(curl_config(Some("ghp_abc"), "application/json").contains("Authorization: Bearer ghp_abc"));
        assert!(refused("curl: (22) The requested URL returned error: 401") && refused("error: 403") && !refused("error: 404"));
        assert!(curl_error("error: 403").contains("límite"));
    }

    /// A newer release with nothing for this system isn't announced.
    #[test]
    fn releases_without_a_package_here_are_not_news() {
        let asset = |name: &str| Asset { name: name.into(), url: String::new(), size: 1, digest: None };
        let here = if cfg!(windows) { "Pulse-99.0.0-windows-x64.zip" } else { "Pulse-99.0.0.zip" };
        let other = if cfg!(windows) { "Pulse-99.0.0.zip" } else { "Pulse-99.0.0-windows-x64.zip" };
        let release = |assets: Vec<Asset>| Release { tag_name: "v99.0.0".into(), body: None, assets };
        assert!(news(&release(vec![asset(here)])));
        assert!(!news(&release(vec![asset(other)])), "only for the other system");
        assert!(!news(&release(Vec::new())));
        assert!(!news(&Release { tag_name: format!("v{VERSION}"), body: None, assets: vec![asset(here)] }), "not newer");
    }

    /// Pulse says why it can't replace itself before downloading anything:
    /// run from macOS's temporary copy, or from a folder it can't write.
    #[cfg(unix)]
    #[test]
    fn where_pulse_cannot_replace_itself_it_says_why() {
        use std::os::unix::fs::PermissionsExt;
        let error = replaceable(Path::new("/private/var/folders/x/T/AppTranslocation/ABC/d/Pulse.app")).unwrap_err();
        assert!(error.contains("Aplicaciones"), "{error}");
        let dir = std::env::temp_dir().join(format!("pulse-solo-lectura-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Pulse.app")).unwrap();
        assert!(replaceable(&dir.join("Pulse.app")).is_ok());
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let error = replaceable(&dir.join("Pulse.app"));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert!(error.unwrap_err().contains("No se puede escribir"));
    }

    #[test]
    fn new_versions_open_the_window_unless_skipped() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let release = |v: &str| Release { tag_name: format!("v{v}"), body: None, assets: Vec::new() };
        // An automatic check that finds a version opens the window, once.
        app.updates.announce = true;
        app.updates.set(State::Checking);
        app.follow_updates();
        assert!(!app.updates.show, "still looking");
        app.updates.set(State::Available(release("9.0.0")));
        app.follow_updates();
        assert!(app.updates.show && !app.updates.announce);
        // Skipped: neither the window nor the badge.
        app.updates.show = false;
        app.settings.skipped_version = Some("9.0.0".into());
        app.updates.announce = true;
        app.follow_updates();
        assert!(!app.updates.show && !app.updates.announce);
        let ctx = egui::Context::default();
        let mut badge = false;
        let _ = ctx.run_ui(Default::default(), |ui| {
            let before = ui.min_rect();
            app.update_badge(ui);
            badge = ui.min_rect() != before;
        });
        assert!(!badge, "no badge for a skipped version");
        // A later one is announced again.
        app.updates.set(State::Available(release("9.0.1")));
        app.updates.announce = true;
        app.follow_updates();
        assert!(app.updates.show);
        // Nothing found: the window stays closed.
        app.updates.show = false;
        app.updates.set(State::UpToDate);
        app.updates.announce = true;
        app.follow_updates();
        assert!(!app.updates.show && !app.updates.announce);
        // The notes of a new version open it too.
        app.updates.set(State::WhatsNew(release("0.1.3")));
        app.updates.announce = true;
        app.follow_updates();
        assert!(app.updates.show);
        // Headless apps never look by themselves.
        app.updates.last_check = Some(std::time::Instant::now() - CHECK_EVERY);
        app.updates.set(State::Idle);
        app.follow_updates();
        assert_eq!(app.updates.state(), State::Idle);
    }

    /// Who signed the installed Pulse and a downloaded release (set PULSE_DOWNLOADED
    /// to an unzipped Pulse.app).
    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "diagnostic: reads signatures of the installed app"]
    fn signers() {
        let installed = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Applications/Pulse.app");
        println!("instalada: {:?}", signer(&installed, false));
        println!("instalada (estricta): {:?}", signer(&installed, true));
        if let Ok(path) = std::env::var("PULSE_DOWNLOADED") {
            println!("descargada (estricta): {:?}", signer(Path::new(&path), true));
            println!("misma firma que la instalada: {:?}", same_signer(&installed, Path::new(&path)));
            // Itself ad hoc signed: must be refused.
            let copy = std::env::temp_dir().join(format!("pulse-adhoc-{}/Pulse.app", std::process::id()));
            let _ = std::fs::remove_dir_all(copy.parent().unwrap());
            std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
            Command::new("/usr/bin/ditto").arg(&path).arg(&copy).status().unwrap();
            Command::new("/usr/bin/codesign").args(["--force", "--sign", "-"]).arg(&copy).status().unwrap();
            println!("copia con firma ad hoc: {:?}", same_signer(&installed, &copy));
            let _ = std::fs::remove_dir_all(copy.parent().unwrap());
        }
    }

    /// The whole update against GitHub, on a copy of the installed Pulse in a
    /// temporary folder: finds the latest release, downloads it, checks it and
    /// swaps it in, then back.
    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "diagnostic: talks to GitHub and downloads the latest release"]
    fn update_a_copy_from_github() {
        // No token: the repository is public.
        let release = latest_release(None).unwrap();
        println!("última publicada: {} ({:?})", release.version(), release.zip().map(|a| &a.name));
        let dir = std::env::temp_dir().join(format!("pulse-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bundle = dir.join("Pulse.app");
        let installed = PathBuf::from(std::env::var("HOME").unwrap()).join("Applications/Pulse.app");
        assert!(Command::new("/usr/bin/ditto").arg(&installed).arg(&bundle).status().unwrap().success());
        let result = install(&release, &bundle, None);
        println!("instalación: {result:?}");
        println!("en su sitio: {:?} · anterior: {:?}", bundle_version(&bundle), bundle_version(&dir.join(PREVIOUS)));
        println!("volver: {:?} → {:?}", swap_back(&bundle), bundle_version(&bundle));
        println!("restos de la descarga: {}", dir.join(format!(".pulse-update-{}", std::process::id())).exists());
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(result, Ok(release.version().to_string()));
    }

    #[test]
    fn the_window_draws_in_every_state() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        app.updates.show = true;
        let release = Release { tag_name: "v9.0.0".into(), body: Some("- Nuevo".into()), assets: Vec::new() };
        for state in [
            State::Idle,
            State::Checking,
            State::UpToDate,
            State::Available(release.clone()),
            State::Downloading(release.clone()),
            State::Installed("9.0.0".into()),
            State::WhatsNew(release),
            State::Failed("x".into()),
        ] {
            app.updates.set(state.clone());
            let ctx = egui::Context::default();
            let _ = ctx.run_ui(Default::default(), |ui| {
                window(&mut app, ui.ctx());
                app.update_badge(ui);
            });
            assert_eq!(app.updates.state(), state, "drawing changes nothing");
        }
    }
}
