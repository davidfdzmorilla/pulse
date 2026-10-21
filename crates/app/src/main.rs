// A published Windows build opens without a console window (see `attach_console`).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod about;
mod accompany;
mod als;
mod arrange;
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
mod drum_gen;
mod editor;
mod export;
mod files;
#[cfg(test)]
mod fidelity_tests;
mod finite;
mod groups;
mod guide;
mod harmony;
mod freeze;
mod history;
mod input;
mod kick_ui;
mod kits;
mod library;
mod logs;
mod macros;
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
#[cfg(test)]
mod persist_tests;
mod plugins;
mod poly_ui;
mod presets;
mod project;
mod recording;
mod recovery;
mod rumble;
mod sample_editor;
mod session;
mod session_rec;
mod scene;
mod script;
mod settings;
mod stems_ui;
mod shape_edit;
mod slicing;
mod store;
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

/// Windows: a program without a console window of its own writes nowhere, so the
/// command line's options (`--render`, `--check-editors`…) borrow the console of
/// the terminal they were typed in.
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

fn main() -> eframe::Result {
    logs::install();
    let args: Vec<String> = std::env::args().collect();
    #[cfg(windows)]
    if args.get(1).is_some_and(|a| a.starts_with("--")) {
        attach_console();
    }
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

    #[allow(unused_mut)]
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("Pulse")
        .with_inner_size(window_size())
        .with_min_inner_size([1000.0, 640.0]);
    // macOS takes the icon from Pulse.app; elsewhere the window carries it (or
    // shows egui's).
    #[cfg(not(target_os = "macos"))]
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../packaging/windows/icon-256.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    open_files::listen();
    eframe::run_native(
        "Pulse",
        options,
        Box::new(|cc| Ok(Box::new(app::PulseApp::new(cc)))),
    )
}
