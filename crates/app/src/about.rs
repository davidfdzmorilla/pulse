//! «Acerca de Pulse»: version, copyright, the GPL's notice (its section 5 asks an
//! interactive program to show one) and the licences of what Pulse carries. Both
//! texts go inside the program, so every copy has them, also after an update.

use crate::app::PulseApp;
use crate::theme::TEXT_DIM;
use eframe::egui;

pub const COPYRIGHT: &str = "Copyright © 2026 David Fernández Morilla";
/// The GNU GPL v3, the licence of Pulse.
pub const LICENSE: &str = include_str!("../../../LICENSE");
/// Everything Pulse carries that isn't Pulse's (scripts/notices.py writes it).
pub const NOTICES: &str = include_str!("../../../packaging/THIRD_PARTY_NOTICES.txt");

const NOTICE: &str = "Pulse es software libre: puedes redistribuirlo y modificarlo según los \
términos de la Licencia Pública General de GNU publicada por la Free Software Foundation, \
versión 3 o (a tu elección) cualquier versión posterior. Pulse se distribuye con la esperanza \
de que sea útil, pero SIN NINGUNA GARANTÍA; ni siquiera la garantía implícita de \
COMERCIABILIDAD o IDONEIDAD PARA UN PROPÓSITO PARTICULAR. Los detalles están en la licencia.";

const TRADEMARKS: &str = "Ableton y Live son marcas de Ableton AG; VST es una marca de Steinberg \
Media Technologies GmbH; Roland, TR-808 y TR-909 son marcas de Roland Corporation. Se nombran \
solo para describir la compatibilidad o el estilo: Pulse no tiene relación con ellas.";

/// Writes `text` to the cache folder and opens it with the system's text viewer.
fn open_text(name: &str, text: &str) -> Result<(), String> {
    let dir = paths::ensure(&paths::cache_dir())?;
    let path = dir.join(name);
    std::fs::write(&path, text).map_err(|e| format!("No se pudo escribir {}: {e}", path.display()))?;
    crate::files::show_folder(&path).map_err(|e| format!("No se pudo abrir {}: {e}", path.display()))
}

pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    let mut open = app.show_about;
    egui::Window::new("Acerca de Pulse")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading(format!("Pulse {}", crate::updates::VERSION));
            ui.label(egui::RichText::new(crate::logs::system()).color(TEXT_DIM).size(10.5)).on_hover_text("Con qué se compiló y dónde corre: dilo si avisas de un fallo");
            ui.label(COPYRIGHT);
            ui.add_space(6.0);
            ui.label(NOTICE);
            ui.add_space(4.0);
            let source = format!("https://github.com/{}", crate::updates::REPOSITORY);
            ui.horizontal(|ui| {
                ui.label("Código fuente:");
                ui.hyperlink(&source);
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (button, name, text) in [
                    ("Licencia (GPL-3.0)", "Pulse - LICENSE.txt", LICENSE),
                    ("Componentes de terceros", "Pulse - THIRD_PARTY_NOTICES.txt", NOTICES),
                ] {
                    if ui.button(button).clicked() {
                        if let Err(e) = open_text(name, text) {
                            app.status = Some(e);
                        }
                    }
                }
            });
            ui.add_space(6.0);
            ui.label(egui::RichText::new(TRADEMARKS).color(TEXT_DIM).size(10.5));
        });
    app.show_about = open;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_licence_is_the_gpl_3() {
        // Git may check it out with Windows line ends.
        let licence = LICENSE.replace("\r\n", "\n");
        assert!(licence.starts_with("                    GNU GENERAL PUBLIC LICENSE\n                       Version 3, 29 June 2007"));
        assert!(LICENSE.contains("END OF TERMS AND CONDITIONS"));
        let manifest = include_str!("../Cargo.toml");
        assert!(manifest.contains("license = \"GPL-3.0-or-later\""));
    }

    /// The notices list every crate in Cargo.lock: after changing dependencies,
    /// run `python3 scripts/notices.py`.
    #[test]
    fn the_notices_cover_every_crate_in_the_lock_file() {
        let lock = include_str!("../../../Cargo.lock");
        let mut missing = vec![];
        for package in lock.split("[[package]]").skip(1) {
            // Pulse's own crates have no source.
            if !package.contains("\nsource = ") {
                continue;
            }
            let field = |key: &str| package.lines().find_map(|l| l.strip_prefix(key)).map(|v| v.trim_matches('"').to_string());
            let (name, version) = (field("name = ").unwrap(), field("version = ").unwrap());
            if !NOTICES.contains(&format!("\n{name} {version} (")) {
                missing.push(format!("{name} {version}"));
            }
        }
        assert!(missing.is_empty(), "Faltan en packaging/THIRD_PARTY_NOTICES.txt (python3 scripts/notices.py): {missing:?}");
        for model in ["ONNX Runtime", "Beat This", "Basic Pitch", "HTDemucs", "Groove MIDI Dataset", "SIL Open Font License"] {
            assert!(NOTICES.contains(model), "{model}");
        }
    }

    #[test]
    fn about_opens_as_one_of_the_app_windows() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        let ctx = egui::Context::default();
        app.run(crate::commands::Command::About, &ctx);
        assert!(app.show_about && !app.show_settings);
        app.run(crate::commands::Command::Settings, &ctx);
        assert!(!app.show_about);
    }
}
