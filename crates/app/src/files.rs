//! The system's file dialogs. While a test script runs (`PULSE_SCRIPT`) they don't
//! open: the script answers them (`pick <path>`, or cancel when it has no answer).

use std::path::PathBuf;

pub fn pick_file(dialog: rfd::FileDialog) -> Option<PathBuf> {
    match crate::script::dialog_answer() {
        Some(answer) => answer.into_iter().next(),
        None => dialog.pick_file(),
    }
}

pub fn pick_files(dialog: rfd::FileDialog) -> Option<Vec<PathBuf>> {
    match crate::script::dialog_answer() {
        Some(answer) => (!answer.is_empty()).then_some(answer),
        None => dialog.pick_files(),
    }
}

pub fn save_file(dialog: rfd::FileDialog) -> Option<PathBuf> {
    match crate::script::dialog_answer() {
        Some(answer) => answer.into_iter().next(),
        None => dialog.save_file(),
    }
}

pub fn pick_folder(dialog: rfd::FileDialog) -> Option<PathBuf> {
    match crate::script::dialog_answer() {
        Some(answer) => answer.into_iter().next(),
        None => dialog.pick_folder(),
    }
}

/// `name` made fit for a file name on macOS and Windows: no path separators
/// or characters Windows refuses, no dots or spaces at the end.
pub fn safe_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '-' } else { c }).collect();
    let clean = clean.trim().trim_end_matches(['.', ' ']).to_string();
    if clean.is_empty() { "Sin nombre".into() } else { clean }
}

/// Shows `folder` in the system's file manager (Finder, the Explorer…).
pub fn show_folder(folder: &std::path::Path) -> std::io::Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program).arg(folder).spawn().map(|_| ())
}
