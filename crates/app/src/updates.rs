//! Updates from the project's private GitHub repository. Each release there has a
//! `Pulse-<version>.zip` with the signed Pulse.app (see `scripts/release.sh`).
//!
//! Pulse looks for the latest release every time it starts, every few hours while
//! it runs, and from Pulse → Buscar actualizaciones…. A new version (unless the
//! user skipped it) opens the window, which shows what's new and installs it:
//! it downloads the zip, checks that the app inside is that version and signed
//! like this one, keeps this version as "Pulse (anterior).app" next to it, puts
//! the new one in its place and restarts. "Volver a la versión anterior" swaps
//! them back. GitHub is reached with `curl`; the token comes from the GitHub CLI
//! (`gh auth token`) or one pasted in the window, kept in the login keychain.

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
/// the new tag), or Cargo's for builds of our own.
pub const VERSION: &str = match option_env!("PULSE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
const KEYCHAIN_SERVICE: &str = "Pulse GitHub";
const PREVIOUS: &str = "Pulse (anterior).app";
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
}

impl Release {
    pub fn version(&self) -> &str {
        self.tag_name.trim_start_matches('v')
    }

    fn zip(&self) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name.ends_with(".zip"))
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

/// A token for the GitHub API: the GitHub CLI's, else the one kept in the keychain.
fn token() -> Option<String> {
    let gh = ["/opt/homebrew/bin/gh", "/usr/local/bin/gh"].into_iter().find(|p| Path::new(p).exists());
    let from_gh = gh.and_then(|gh| output(Command::new(gh).args(["auth", "token"])));
    from_gh.or_else(|| output(Command::new("/usr/bin/security").args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"])))
}

/// Keeps a token in the login keychain.
fn remember_token(token: &str) -> Result<(), String> {
    let status = Command::new("/usr/bin/security")
        .args(["add-generic-password", "-U", "-s", KEYCHAIN_SERVICE, "-a", "github", "-w", token])
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err("No se pudo guardar el token en el llavero".into()) }
}

fn output(command: &mut Command) -> Option<String> {
    let out = command.stderr(Stdio::null()).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

/// Runs curl with the token in its configuration (read from stdin, so it never
/// shows in the process list). Returns the body, or writes it to `to`.
fn curl(url: &str, token: &str, accept: &str, to: Option<&Path>) -> Result<Vec<u8>, String> {
    let mut command = Command::new("/usr/bin/curl");
    command.args(["--silent", "--show-error", "--fail", "--location", "--max-time", "600", "--config", "-"]);
    if let Some(path) = to {
        command.arg("--output").arg(path);
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
        let config = format!(
            "header = \"Authorization: Bearer {token}\"\nheader = \"Accept: {accept}\"\nheader = \"X-GitHub-Api-Version: 2022-11-28\"\nuser-agent = \"Pulse/{VERSION}\"\n"
        );
        stdin.write_all(config.as_bytes()).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let error = String::from_utf8_lossy(&out.stderr);
        Err(if error.contains("404") {
            format!("No se encontraron versiones publicadas en {REPOSITORY} (o el token no tiene acceso)")
        } else if error.contains("401") || error.contains("403") {
            "GitHub no acepta el token: inicia sesión con «gh auth login» o pega uno nuevo".to_string()
        } else if error.contains("resolve host") || error.contains("connect") {
            "Sin conexión con GitHub".to_string()
        } else {
            format!("Error al hablar con GitHub: {}", error.trim())
        })
    }
}

fn latest_release(token: &str) -> Result<Release, String> {
    release_at(token, "latest")
}

/// `which`: "latest" or "tags/v1.2.3".
fn release_at(token: &str, which: &str) -> Result<Release, String> {
    let body = curl(&format!("https://api.github.com/repos/{REPOSITORY}/releases/{which}"), token, "application/vnd.github+json", None)?;
    serde_json::from_slice(&body).map_err(|e| format!("Respuesta de GitHub no válida: {e}"))
}

// ----- Installing --------------------------------------------------------------------

/// The Pulse.app this program runs from, if it runs from one.
pub fn running_bundle() -> Option<PathBuf> {
    bundle_of(&std::env::current_exe().ok()?)
}

fn bundle_of(executable: &Path) -> Option<PathBuf> {
    // …/Pulse.app/Contents/MacOS/pulse
    let bundle = executable.parent()?.parent()?.parent()?;
    (bundle.extension().is_some_and(|e| e == "app")).then(|| bundle.to_path_buf())
}

/// The bundle's version, from its Info.plist.
fn bundle_version(bundle: &Path) -> Option<String> {
    output(Command::new("/usr/libexec/PlistBuddy").args(["-c", "Print :CFBundleShortVersionString"]).arg(bundle.join("Contents/Info.plist")))
}

/// Who signed a bundle ("adhoc" for ad hoc signatures), if the signature is valid.
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
/// signature.
fn same_signer(ours: &Path, new: &Path) -> Result<(), String> {
    let theirs = signer(new, true)?;
    // A broken signature here must not open the door to any download.
    let mine = signer(ours, false).map_err(|e| format!("No se puede comprobar esta versión de Pulse: {e}"))?;
    if mine == "adhoc" {
        return Ok(());
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
/// beside it (replacing an older one). If anything fails, `current` stays.
pub fn swap_in(current: &Path, new: &Path) -> Result<(), String> {
    let previous = current.with_file_name(PREVIOUS);
    if previous.exists() {
        std::fs::remove_dir_all(&previous).map_err(|e| format!("No se pudo quitar la versión anterior guardada: {e}"))?;
    }
    std::fs::rename(current, &previous).map_err(|e| format!("No se pudo apartar esta versión: {e}"))?;
    if let Err(e) = std::fs::rename(new, current) {
        let _ = std::fs::rename(&previous, current);
        return Err(format!("No se pudo instalar la nueva versión: {e}"));
    }
    Ok(())
}

/// Puts the previous version back, keeping this one as the previous.
pub fn swap_back(current: &Path) -> Result<(), String> {
    let previous = current.with_file_name(PREVIOUS);
    if !previous.exists() {
        return Err("No hay ninguna versión anterior guardada".into());
    }
    let parked = current.with_file_name(".Pulse-cambio.app");
    let _ = std::fs::remove_dir_all(&parked);
    std::fs::rename(current, &parked).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&previous, current) {
        let _ = std::fs::rename(&parked, current);
        return Err(e.to_string());
    }
    std::fs::rename(&parked, &previous).map_err(|e| e.to_string())
}

/// Downloads a release and installs it over `bundle`. Returns the new version.
fn install(release: &Release, bundle: &Path, token: &str) -> Result<String, String> {
    let asset = release.zip().ok_or("La versión publicada no tiene ningún .zip")?;
    // Next to the bundle, so the final move stays on the same disk.
    let work = bundle.with_file_name(".pulse-update");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("No se pudo preparar la descarga: {e}"))?;
    let result = (|| {
        let zip = work.join(&asset.name);
        curl(&asset.url, token, "application/octet-stream", Some(&zip))?;
        let unzip = Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(&zip).arg(&work).status().map_err(|e| e.to_string())?;
        if !unzip.success() {
            return Err("No se pudo descomprimir la descarga".to_string());
        }
        let new = work.join("Pulse.app");
        let version = bundle_version(&new).ok_or("La descarga no contiene Pulse.app")?;
        if version != release.version() {
            return Err(format!("La descarga es la versión {version}, no la {}", release.version()));
        }
        same_signer(bundle, &new)?;
        swap_in(bundle, &new)?;
        Ok(version)
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Opens `bundle` once this process has quit.
fn relaunch(bundle: &Path) -> Result<(), String> {
    let script = format!("while /bin/kill -0 {} 2>/dev/null; do /bin/sleep 0.2; done; /usr/bin/open \"$0\"", std::process::id());
    Command::new("/bin/sh").arg("-c").arg(script).arg(bundle).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(|_| ()).map_err(|e| e.to_string())
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
            if let Some(release) = token().and_then(|token| release_at(&token, &format!("tags/v{VERSION}")).ok()) {
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
                    self.updates.show = true;
                    self.updates.announce = false;
                }
                State::WhatsNew(_) => {
                    self.updates.show = true;
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
            let state = match token() {
                None => State::Failed("Falta el acceso a GitHub: inicia sesión con «gh auth login» o pega un token aquí".into()),
                Some(token) => match latest_release(&token) {
                    Ok(release) if newer(release.version(), VERSION) => State::Available(release),
                    Ok(_) => State::UpToDate,
                    Err(e) => State::Failed(e),
                },
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
            let state = match token() {
                None => State::Failed("Falta el acceso a GitHub".into()),
                Some(token) => match install(&release, &bundle, &token) {
                    Ok(version) => State::Installed(version),
                    Err(e) => State::Failed(e),
                },
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
                format!("⬆ Pulse {} disponible", release.version())
            }
            State::Installed(version) => format!("↻ Reinicia para usar Pulse {version}"),
            _ => return,
        };
        if ui.small_button(label).clicked() {
            self.updates.show = true;
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
    egui::Window::new("Actualizaciones").open(&mut open).resizable(false).collapsible(false).default_width(460.0).show(ctx, |ui| {
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
                ui.label("✔ Tienes la última versión.");
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
                    ui.label(egui::RichText::new("Pulse se está ejecutando fuera de Pulse.app (desde el terminal): no se puede instalar desde aquí.").color(TEXT_DIM).size(10.5));
                }
            }
            State::Downloading(release) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Descargando e instalando Pulse {}…", release.version()));
                });
            }
            State::Installed(version) => {
                ui.label(format!("✔ Pulse {version} está instalado. Se usará al reiniciar."));
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
        ui.collapsing("Acceso a GitHub", |ui| {
            ui.label(
                egui::RichText::new("Pulse usa la sesión de GitHub CLI («gh auth login»). Si no la tienes, pega un token con permiso de lectura del repositorio; se guarda en el llavero.")
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
            match remember_token(token.trim()) {
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
            {"name":"Pulse-0.2.0.zip","url":"https://api.github.com/repos/o/r/releases/assets/2","size":12000000,"extra":1}]}"#;
        let release: Release = serde_json::from_str(json).unwrap();
        assert_eq!(release.version(), "0.2.0");
        assert_eq!(release.zip().map(|a| a.name.as_str()), Some("Pulse-0.2.0.zip"));
        assert!(newer(release.version(), "0.1.0"));
    }

    #[test]
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
        // A missing new version leaves the current one in place.
        assert!(swap_in(&current, &dir.join("missing.app")).is_err());
        assert_eq!(read(&current), Some("2".into()));
        std::fs::remove_dir_all(&dir).ok();
        assert!(swap_back(&current).is_err());
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
    #[ignore = "diagnostic: talks to GitHub and downloads the latest release"]
    fn update_a_copy_from_github() {
        let token = token().expect("gh auth token");
        let release = latest_release(&token).unwrap();
        println!("última publicada: {} ({:?})", release.version(), release.zip().map(|a| &a.name));
        let dir = std::env::temp_dir().join(format!("pulse-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bundle = dir.join("Pulse.app");
        let installed = PathBuf::from(std::env::var("HOME").unwrap()).join("Applications/Pulse.app");
        assert!(Command::new("/usr/bin/ditto").arg(&installed).arg(&bundle).status().unwrap().success());
        let result = install(&release, &bundle, &token);
        println!("instalación: {result:?}");
        println!("en su sitio: {:?} · anterior: {:?}", bundle_version(&bundle), bundle_version(&dir.join(PREVIOUS)));
        println!("volver: {:?} → {:?}", swap_back(&bundle), bundle_version(&bundle));
        println!("restos de la descarga: {}", dir.join(".pulse-update").exists());
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
