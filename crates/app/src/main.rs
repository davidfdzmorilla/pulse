mod accompany;
mod als;
mod app;
mod arrangement;
mod automation_rec;
mod browser;
mod capture;
mod clip_envelope;
mod clip_select;
mod clip_split;
mod comping;
mod commands;
mod consolidate;
mod curves;
mod detail;
mod devices;
mod editor;
mod export;
mod files;
mod groups;
mod guide;
mod harmony;
mod freeze;
mod history;
mod input;
mod kick_ui;
mod kits;
mod library;
mod loop_key;
mod midi_fx_ui;
mod midi_map;
mod mix_assist;
mod multisample;
mod modular_ui;
mod native_menu;
mod note_edit;
mod open_files;
mod piano_roll;
mod plugins;
mod poly_ui;
mod presets;
mod project;
mod recording;
mod sample_editor;
mod session;
mod session_rec;
mod scene;
mod script;
mod settings;
mod stems_ui;
mod shape_edit;
mod slicing;
mod sync;
mod table_editor;
mod tempo_ui;
mod theme;
mod to_midi;
mod tracks;
mod updates;
mod voice_play;
mod versions;
mod transport;
mod wave_ui;
mod widgets;

/// The window's size: 1440×900, or PULSE_SIZE=WxH (for screenshots).
fn window_size() -> [f32; 2] {
    std::env::var("PULSE_SIZE")
        .ok()
        .and_then(|s| s.split_once('x').and_then(|(w, h)| Some([w.parse().ok()?, h.parse().ok()?])))
        .unwrap_or([1440.0, 900.0])
}

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
            .with_inner_size(window_size())
            .with_min_inner_size([1000.0, 640.0]),
        ..Default::default()
    };
    open_files::listen();
    eframe::run_native(
        "Pulse",
        options,
        Box::new(|cc| Ok(Box::new(app::PulseApp::new(cc)))),
    )
}
