//! Colors, fonts and widget styling.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
pub const PANEL: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
pub const SURFACE: Color32 = Color32::from_rgb(0x30, 0x30, 0x30);
pub const SLOT: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2c);
pub const SLOT_HOVER: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);
pub const CONTROL: Color32 = Color32::from_rgb(0x3d, 0x3d, 0x3d);
pub const CONTROL_HOVER: Color32 = Color32::from_rgb(0x4a, 0x4a, 0x4a);
pub const LINE: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
pub const TEXT: Color32 = Color32::from_rgb(0xd8, 0xd8, 0xd8);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8c, 0x8c, 0x8c);
pub const ACCENT: Color32 = Color32::from_rgb(0xff, 0xa8, 0x24);
/// Frozen tracks (see `freeze`): ice blue.
/// Input monitoring buttons.
pub const MONITOR: Color32 = Color32::from_rgb(0xe6, 0xbe, 0x46);
pub const FROZEN: Color32 = Color32::from_rgb(140, 200, 255);
pub const PLAY: Color32 = Color32::from_rgb(0x6b, 0xe3, 0x6b);
pub const SOLO: Color32 = Color32::from_rgb(0x5a, 0xa8, 0xf0);
pub const SELECT: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
pub const KNOB_TRACK: Color32 = Color32::from_rgb(0x52, 0x52, 0x52);
pub const METER_GREEN: Color32 = Color32::from_rgb(0x5e, 0xd8, 0x6f);
pub const METER_YELLOW: Color32 = Color32::from_rgb(0xe6, 0xd0, 0x4a);
pub const METER_RED: Color32 = Color32::from_rgb(0xf0, 0x52, 0x46);

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Scales a color towards black.
pub fn shade(c: Color32, factor: f32) -> Color32 {
    let f = |v: u8| (v as f32 * factor).clamp(0.0, 255.0) as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// Black or white, whichever reads better on `background`.
pub fn text_on(background: Color32) -> Color32 {
    let luma = 0.299 * background.r() as f32 + 0.587 * background.g() as f32 + 0.114 * background.b() as f32;
    if luma > 140.0 {
        Color32::from_rgb(0x14, 0x14, 0x14)
    } else {
        Color32::from_rgb(0xf0, 0xf0, 0xf0)
    }
}

pub fn small() -> FontId {
    FontId::proportional(10.5)
}

pub fn body() -> FontId {
    FontId::proportional(12.0)
}

/// Shortcuts as this system writes them: macOS's ⌘ ⇧ ⌥ ⌃ ⌫ as they are,
/// elsewhere with names ("⇧⌘S" → "Ctrl+Mayús+S", "⌘ clic" → "Ctrl clic",
/// full screen → "F11"; egui reads ⌘ as Ctrl there).
pub fn keys(text: &str) -> String {
    if cfg!(target_os = "macos") { text.to_string() } else { named_keys(text) }
}

/// [`keys`] off macOS.
pub fn named_keys(text: &str) -> String {
    let text = text.replace("⌃⌘F", "F11").replace('⌫', "Supr");
    let modifier = |c: char| matches!(c, '⌘' | '⇧' | '⌥' | '⌃');
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if !modifier(c) {
            out.push(c);
            continue;
        }
        let mut held = vec![c];
        while let Some(next) = chars.next_if(|&n| modifier(n)) {
            held.push(next);
        }
        // Windows' order: Ctrl, Mayús, Alt.
        let names: Vec<&str> = [("Ctrl", '⌘'), ("Ctrl", '⌃'), ("Mayús", '⇧'), ("Alt", '⌥')]
            .into_iter()
            .filter(|(_, glyph)| held.contains(glyph))
            .map(|(name, _)| name)
            .fold(Vec::new(), |mut names, name| {
                if !names.contains(&name) {
                    names.push(name);
                }
                names
            });
        out += &names.join("+");
        // "⌘D" → "Ctrl+D", but "⌘ clic" → "Ctrl clic".
        if chars.peek().is_some_and(|n| !n.is_whitespace() && !matches!(n, ')' | ':' | '·')) {
            out.push('+');
        }
    }
    out
}

/// The system's interface, fixed-width and symbol fonts (macOS: San Francisco and
/// Apple Symbols; Windows: Segoe UI, Consolas, Segoe UI Symbol and Segoe UI
/// Emoji, which has the symbols that are also emoji since Windows 10: ▶ ◀ ⚠;
/// egui draws its plain outlines, not its colours).
fn system_fonts() -> ([std::path::PathBuf; 2], Vec<std::path::PathBuf>) {
    if cfg!(windows) {
        let fonts = std::env::var_os("WINDIR").map_or_else(|| std::path::PathBuf::from(r"C:\Windows"), std::path::PathBuf::from).join("Fonts");
        (["segoeui.ttf", "consola.ttf"].map(|f| fonts.join(f)), ["seguisym.ttf", "seguiemj.ttf"].map(|f| fonts.join(f)).to_vec())
    } else {
        (["/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/SFNSMono.ttf"].map(std::path::PathBuf::from), vec!["/System/Library/Fonts/Apple Symbols.ttf".into()])
    }
}

/// Uses the system's fonts when available; egui's bundled font otherwise.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let ([system, mono], symbols) = system_fonts();
    for (name, path) in [("system", system), ("system-mono", mono)] {
        let Ok(bytes) = std::fs::read(path) else { continue };
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_owned(bytes)));
        let family = if name == "system" { FontFamily::Proportional } else { FontFamily::Monospace };
        fonts.families.entry(family).or_default().insert(0, name.into());
    }
    // Symbols neither has (✎ ⤢ ✔ ⬆ …), from the system's symbol fonts, last.
    for (i, path) in symbols.into_iter().enumerate() {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let name = format!("symbols-{i}");
        fonts.font_data.insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push(name.clone());
        }
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, small()),
            (TextStyle::Body, body()),
            (TextStyle::Button, body()),
            (TextStyle::Heading, FontId::proportional(15.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(7.0, 2.0);
        style.spacing.interact_size.y = 20.0;
        style.spacing.slider_rail_height = 4.0;

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.override_text_color = Some(TEXT);
        v.panel_fill = PANEL;
        v.window_fill = SURFACE;
        v.extreme_bg_color = BG;
        v.faint_bg_color = SURFACE;
        v.window_stroke = egui::Stroke::new(1.0, LINE);
        v.selection.bg_fill = shade(ACCENT, 0.55);
        v.selection.stroke = egui::Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        let radius = egui::CornerRadius::same(2);
        for (w, fill) in [
            (&mut v.widgets.noninteractive, PANEL),
            (&mut v.widgets.inactive, CONTROL),
            (&mut v.widgets.hovered, CONTROL_HOVER),
            (&mut v.widgets.active, shade(ACCENT, 0.7)),
            (&mut v.widgets.open, CONTROL_HOVER),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.corner_radius = radius;
            w.bg_stroke = egui::Stroke::NONE;
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, LINE);
        v.widgets.noninteractive.fg_stroke.color = TEXT_DIM;
        v.widgets.inactive.fg_stroke.color = TEXT;
        v.widgets.hovered.fg_stroke.color = Color32::WHITE;
        v.widgets.active.fg_stroke.color = Color32::WHITE;
    });
}

#[cfg(test)]
mod glyph_tests {
    use super::*;

    /// Every symbol in the interface's texts (string literals of the app's sources)
    /// is in its fonts: a missing one shows as an empty box.
    #[test]
    fn the_fonts_have_every_symbol_the_interface_uses() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let _ = ctx.run_ui(Default::default(), |_| {});
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut used = std::collections::BTreeMap::<char, String>::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            for line in text.lines().filter(|l| !l.trim_start().starts_with("//")) {
                // The parts between quotes.
                for (i, part) in line.split('"').enumerate() {
                    if i % 2 == 1 {
                        for c in part.chars().filter(|c| !c.is_ascii()) {
                            used.entry(c).or_insert_with(|| format!("{}: {}", path.file_name().unwrap().to_string_lossy(), line.trim()));
                        }
                    }
                }
            }
        }
        let missing: Vec<String> = ctx.fonts_mut(|f| {
            used.iter().filter(|(c, _)| !f.has_glyph(&body(), **c)).map(|(c, at)| format!("{c} (U+{:04X}) en {at}", *c as u32)).collect()
        });
        assert!(missing.is_empty(), "{} símbolos sin glifo:\n{}", missing.len(), missing.join("\n"));
    }
}

#[cfg(test)]
mod key_tests {
    use super::named_keys;

    #[test]
    fn shortcuts_are_named_the_windows_way() {
        assert_eq!(named_keys("⌘D"), "Ctrl+D");
        assert_eq!(named_keys("⇧⌘S"), "Ctrl+Mayús+S");
        assert_eq!(named_keys("⌥⌘S"), "Ctrl+Alt+S");
        assert_eq!(named_keys("⌘S / ⇧⌘S"), "Ctrl+S / Ctrl+Mayús+S");
        assert_eq!(named_keys("Duplicar   ⌘D"), "Duplicar   Ctrl+D");
        assert_eq!(named_keys("⌘ clic / ⇧ clic"), "Ctrl clic / Mayús clic");
        assert_eq!(named_keys("arrastrar: mover (⌥ copia)"), "arrastrar: mover (Alt copia)");
        assert_eq!(named_keys("Nueva pista MIDI (⌘T)"), "Nueva pista MIDI (Ctrl+T)");
        assert_eq!(named_keys("⇧Tab"), "Mayús+Tab");
        assert_eq!(named_keys("⌃⌘F"), "F11");
        assert_eq!(named_keys("⌘,"), "Ctrl+,");
        assert_eq!(named_keys("⌫"), "Supr");
        assert_eq!(named_keys("Sin atajos"), "Sin atajos");
    }
}
