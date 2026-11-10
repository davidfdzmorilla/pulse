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

/// A name Pulse gives what it makes (a track, a clip, a marker, a return) in
/// the interface's language when it's made: kept so in the project, it never
/// changes with the language afterwards. It's `tr`'s English, unless the word
/// as a name is another one (`en::NAMES`: «Subida» is a Riser in the
/// transitions menu but a Build as a section).
pub fn named(spanish: &'static str) -> &'static str {
    match current() {
        Language::Spanish => spanish,
        Language::English => english_name(spanish),
    }
}

/// Whether `name` is the one Pulse gives as `spanish`, made in either
/// language («Voz», «Vocals»): what finds them again, in old projects and new.
pub fn is_named(name: &str, spanish: &'static str) -> bool {
    name == spanish || name == english_name(spanish)
}

fn english_name(spanish: &'static str) -> &'static str {
    crate::en::NAMES.iter().find(|(es, _)| *es == spanish).map(|(_, en)| *en).or_else(|| english().get(spanish).copied()).unwrap_or(spanish)
}

/// A key's or chord's name («Re menor», «Lam7», «Sib/Re», «Rem Sib Fa Do»), as
/// the interface writes it: in English, with letters («D minor», «Am7», «Bb/D»).
/// What a project keeps stays as it is (a key is kept in Spanish); this is
/// for showing it, and for naming a new clip after its chords (see `example`).
pub fn music(name: &str) -> String {
    if current() == Language::Spanish {
        return name.to_string();
    }
    english_music(name)
}

fn english_music(name: &str) -> String {
    // Sol before Si and the rest: the longest syllable first.
    const NOTES: [(&str, char); 7] = [("Sol", 'G'), ("Do", 'C'), ("Re", 'D'), ("Mi", 'E'), ("Fa", 'F'), ("La", 'A'), ("Si", 'B')];
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    let mut word_start = true;
    while let Some(c) = rest.chars().next() {
        if word_start {
            if let Some(&(syllable, letter)) = NOTES.iter().find(|(s, _)| rest.starts_with(s)) {
                let after = &rest[syllable.len()..];
                // A note only if what follows is a chord's (b, #, m, sus, 7,
                // °, +, ø, /), not more of a word («Restaurar», «Mio»).
                let accidental = after.strip_prefix(['b', '#']).unwrap_or(after);
                let next = accidental.chars().next();
                let fits = match next {
                    None => true,
                    Some(n) if !n.is_alphabetic() => true,
                    Some('m') => !accidental.starts_with("me") && !accidental.starts_with("ma") || accidental.starts_with("maj"),
                    Some('s') => accidental.starts_with("sus"),
                    _ => false,
                };
                if fits {
                    out.push(letter);
                    out.push_str(&after[..after.len() - accidental.len()]);
                    rest = accidental;
                    word_start = false;
                    continue;
                }
            }
            for (spanish, english) in [("mayor", "major"), ("menor", "minor")] {
                if let Some(after) = rest.strip_prefix(spanish) {
                    if after.chars().next().is_none_or(|n| !n.is_alphabetic()) {
                        out.push_str(english);
                        rest = after;
                        break;
                    }
                }
            }
            if rest.is_empty() {
                break;
            }
        }
        let c = rest.chars().next().unwrap_or(c);
        out.push(c);
        rest = &rest[c.len_utf8()..];
        word_start = !c.is_alphanumeric();
    }
    out
}

/// One of Pulse's own names (a factory preset, kit, chain or sound) in the
/// interface's language, a part at a time («Bajos · Sub oscuro» → «Basses ·
/// Dark sub»). A part without English shows as it is. Only for names Pulse
/// made: the user's own are never translated.
pub fn tr_name(name: &str) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    if current() == Language::Spanish {
        return Cow::Borrowed(name);
    }
    let table = english();
    if !name.contains(" · ") {
        return Cow::Borrowed(table.get(name).copied().unwrap_or(name));
    }
    Cow::Owned(name.split(" · ").map(|part| table.get(part).copied().unwrap_or(part)).collect::<Vec<_>>().join(" · "))
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
    fn pulse_names_go_a_part_at_a_time() {
        assert_eq!(tr_name("Bajos · Sub oscuro"), "Bajos · Sub oscuro");
        with(Language::English, || {
            assert_eq!(tr_name("Bajos · Sub oscuro"), "Basses · Dark sub");
            assert_eq!(tr_name("Bombo house"), "House kick");
            assert_eq!(tr_name("Mi bombo · raro"), "Mi bombo · raro");
        });
    }

    #[test]
    fn names_are_given_in_the_language_of_the_moment() {
        assert_eq!((named("Bajo"), named("Voz")), ("Bajo", "Voz"));
        with(Language::English, || {
            // A name's own English where the interface's word is another.
            assert_eq!((named("Bajo"), named("Voz")), ("Bass", "Vocals"));
            assert_eq!((tr("Voz"), tr("Subida"), named("Subida")), ("Voice", "Riser", "Build"));
        });
        for language in Language::ALL {
            with(language, || assert!(is_named("Voz", "Voz") && is_named("Vocals", "Voz") && !is_named("Voice", "Voz")));
        }
    }

    #[test]
    fn templates_take_their_values_in_order() {
        assert_eq!(fill("{} de {}", &[&3, &"4"]), "3 de 4");
        assert_eq!(fill("sin huecos", &[&1]), "sin huecos");
        assert_eq!(fill("{} y {}", &[&1]), "1 y ");
    }

    #[test]
    fn keys_and_chords_show_with_letters_in_english() {
        let names = ["Re menor", "Do mayor", "Mib mayor", "Fa# menor", "Lam7", "Sib/Re", "Do#m", "Solmaj7", "Resus4", "Si°", "Rem Sib Fa/La Do", "vi"];
        for name in names {
            assert_eq!(music(name), name, "Spanish stays as it is");
        }
        with(Language::English, || {
            let english: Vec<String> = names.iter().map(|n| music(n)).collect();
            assert_eq!(english, ["D minor", "C major", "Eb major", "F# minor", "Am7", "Bb/D", "C#m", "Gmaj7", "Dsus4", "B°", "Dm Bb F/A C", "vi"]);
            assert_eq!(music("Restaurar"), "Restaurar", "a word that starts like a note stays");
        });
        // What Pulse keeps is still the Spanish name.
        let key = stems::key::Key { tonic: 2, minor: true };
        with(Language::English, || assert_eq!(key.name(), "Re menor"));
    }

    #[test]
    fn the_language_setting_reads_and_writes_short_codes() {
        assert_eq!(serde_json::to_string(&Language::English).unwrap(), "\"en\"");
        assert_eq!(serde_json::from_str::<Language>("\"es\"").unwrap(), Language::Spanish);
    }
}
