mod als;
mod app;
mod arrangement;
mod browser;
mod capture;
mod commands;
mod detail;
mod devices;
mod editor;
mod export;
mod history;
mod input;
mod kick_ui;
mod kits;
mod midi_map;
mod modular_ui;
mod native_menu;
mod note_edit;
mod piano_roll;
mod plugins;
mod poly_ui;
mod presets;
mod project;
mod recording;
mod sample_editor;
mod session;
mod settings;
mod shape_edit;
mod slicing;
mod sync;
mod table_editor;
mod theme;
mod tracks;
mod transport;
mod wave_ui;
mod widgets;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--render") {
        let Some(output) = args.get(2) else {
            eprintln!("Uso: pulse --render <salida.wav> [proyecto.pulse]");
            std::process::exit(2);
        };
        let project = args.get(3).map(std::path::Path::new);
        match export::render_from_command_line(std::path::Path::new(output), project) {
            Ok(summary) => println!("{summary}"),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    if args.get(1).map(String::as_str) == Some("--check-menus") {
        // As for a fresh project: nothing to undo, revert or paste, no clip selected.
        use commands::Command as C;
        let fresh = |c: C| !matches!(c, C::Undo | C::Redo | C::Revert | C::Cut | C::Copy | C::Paste | C::Duplicate | C::Delete | C::SelectAll);
        for line in native_menu::describe(fresh) {
            println!("{line}");
        }
        return Ok(());
    }

    if args.get(1).map(String::as_str) == Some("--check-editors") {
        for line in editor::check_installed() {
            println!("{line}");
        }
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Pulse")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1000.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Pulse",
        options,
        Box::new(|cc| Ok(Box::new(app::PulseApp::new(cc)))),
    )
}
