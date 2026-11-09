//! The interface's language. Pulse is written in Spanish: every text in the code
//! is the Spanish one, and `tr` gives its English version from `en.rs` when the
//! interface is in English. A text without one shows in Spanish, so the
//! translation can grow a piece at a time.
//!
//! Texts with values in them are templates: `fill(tr("{} compases"), &[&bars])`
//! puts the values where the `{}` are, in order.
//!
//! The language is the system's unless Ajustes says otherwise. Test scripts
//! (`PULSE_SCRIPT`) and screenshots stay in Spanish, since they find widgets by
//! their text.

use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "en")]
    English,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::Spanish, Language::English];

    /// Its name in itself, as language menus show them.
    pub fn name(self) -> &'static str {
        match self {
            Language::Spanish => "Español",
            Language::English => "English",
        }
    }

    /// The language for a system locale such as "es-ES", "es_MX.UTF-8" or
    /// "en-GB": Spanish for Spanish, English for anything else (it's the
    /// language most people read after their own).
    pub fn from_locale(locale: &str) -> Language {
        let code = locale.split(['-', '_', '.']).next().unwrap_or("").to_ascii_lowercase();
        if code == "es" { Language::Spanish } else { Language::English }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

thread_local! {
    /// A test's own language, so tests that draw in English don't change what
    /// the others, running at the same time, see.
    static OVERRIDE: Cell<Option<Language>> = const { Cell::new(None) };
}

pub fn current() -> Language {
    OVERRIDE.with(|o| o.get()).unwrap_or(if CURRENT.load(Ordering::Relaxed) == 1 { Language::English } else { Language::Spanish })
}

pub fn set(language: Language) {
    CURRENT.store(language as u8, Ordering::Relaxed);
}

/// Runs `f` with this thread's texts in `language`.
#[cfg(test)]
pub fn with<R>(language: Language, f: impl FnOnce() -> R) -> R {
    let before = OVERRIDE.with(|o| o.replace(Some(language)));
    let result = f();
    OVERRIDE.with(|o| o.set(before));
    result
}

/// The language to use: the one chosen in Ajustes, else the system's.
pub fn resolve(chosen: Option<Language>) -> Language {
    chosen.unwrap_or_else(system)
}

/// The system's language (its first preferred one).
pub fn system() -> Language {
    system_locale().map_or(Language::Spanish, |l| Language::from_locale(&l))
}

#[cfg(target_os = "macos")]
fn system_locale() -> Option<String> {
    let languages = objc2_foundation::NSLocale::preferredLanguages();
    languages.iter().next().map(|l| l.to_string())
}

#[cfg(windows)]
fn system_locale() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buffer = [0u16; 85];
    // SAFETY: the buffer is as long as we say; it returns the characters written.
    let written = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    (written > 1).then(|| String::from_utf16_lossy(&buffer[..written as usize - 1]))
}

#[cfg(not(any(target_os = "macos", windows)))]
fn system_locale() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|v| std::env::var(v).ok().filter(|l| !l.is_empty() && l != "C" && l != "POSIX"))
}

fn english() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| crate::en::EN.iter().copied().collect())
}

/// `spanish` in the interface's language.
pub fn tr(spanish: &'static str) -> &'static str {
    match current() {
        Language::Spanish => spanish,
        Language::English => english().get(spanish).copied().unwrap_or(spanish),
    }
}

/// `template` with each `{}` replaced by the next value.
pub fn fill(template: &str, values: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut values = values.iter();
    let mut rest = template;
    while let Some(at) = rest.find("{}") {
        out.push_str(&rest[..at]);
        if let Some(value) = values.next() {
            out.push_str(&value.to_string());
        }
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_choose_the_language() {
        for spanish in ["es", "es-ES", "es_MX.UTF-8", "ES-419"] {
            assert_eq!(Language::from_locale(spanish), Language::Spanish, "{spanish}");
        }
        for other in ["en-US", "en_GB.UTF-8", "fr-FR", "de", "pt-BR", "ca-ES", ""] {
            assert_eq!(Language::from_locale(other), Language::English, "{other}");
        }
        assert_eq!(resolve(Some(Language::Spanish)), Language::Spanish);
        assert_eq!(resolve(Some(Language::English)), Language::English);
    }

    #[test]
    fn texts_follow_the_language_and_fall_back_to_spanish() {
        assert_eq!(tr("Archivo"), "Archivo");
        with(Language::English, || {
            assert_eq!(tr("Archivo"), "File");
            assert_eq!(tr("una frase que no está en la tabla"), "una frase que no está en la tabla");
        });
        assert_eq!(tr("Archivo"), "Archivo", "the override ends with `with`");
    }

    #[test]
    fn templates_take_their_values_in_order() {
        assert_eq!(fill("{} de {}", &[&3, &"4"]), "3 de 4");
        assert_eq!(fill("sin huecos", &[&1]), "sin huecos");
        assert_eq!(fill("{} y {}", &[&1]), "1 y ");
    }

    #[test]
    fn the_language_setting_reads_and_writes_short_codes() {
        assert_eq!(serde_json::to_string(&Language::English).unwrap(), "\"en\"");
        assert_eq!(serde_json::from_str::<Language>("\"es\"").unwrap(), Language::Spanish);
    }
}
