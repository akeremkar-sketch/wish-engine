//! The editor's languages (M8a WP8).
//!
//! The interface is written in English in the code, and every text a person
//! reads goes through [`t`] or [`tf`], keyed by that English text. Each other
//! language is one file, `lang/<code>.json`: `{ "English text": "its
//! translation" }`, so a translator needs no Rust. A text a file lacks shows
//! in English; the `languages` test lists what each file lacks or has too
//! much, that the `{names}` in a translation are the English text's, and that
//! the font has every letter.
//!
//! The language is per thread (the editor's frame runs on one thread, and
//! tests that run side by side each keep their own).

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt::Display;
use std::sync::OnceLock;

/// A language of the interface.
#[derive(Debug)]
pub struct Language {
    /// Its code (`tr`, `de`...): the file's name and what `WISH_LANG` holds.
    pub code: &'static str,
    /// Its name, in itself (`Türkçe`, `Deutsch`...).
    pub name: &'static str,
    /// Its translations (`lang/<code>.json`; English has none).
    pub source: &'static str,
}

/// The languages, English first (the one the code is written in).
pub const LANGUAGES: &[Language] = &[
    Language {
        code: "en",
        name: "English",
        source: "{}",
    },
    Language {
        code: "tr",
        name: "Türkçe",
        source: include_str!("../lang/tr.json"),
    },
    Language {
        code: "de",
        name: "Deutsch",
        source: include_str!("../lang/de.json"),
    },
    Language {
        code: "fr",
        name: "Français",
        source: include_str!("../lang/fr.json"),
    },
    Language {
        code: "es",
        name: "Español",
        source: include_str!("../lang/es.json"),
    },
    Language {
        code: "pt",
        name: "Português",
        source: include_str!("../lang/pt.json"),
    },
    Language {
        code: "it",
        name: "Italiano",
        source: include_str!("../lang/it.json"),
    },
    Language {
        code: "nl",
        name: "Nederlands",
        source: include_str!("../lang/nl.json"),
    },
    Language {
        code: "pl",
        name: "Polski",
        source: include_str!("../lang/pl.json"),
    },
    Language {
        code: "ru",
        name: "Русский",
        source: include_str!("../lang/ru.json"),
    },
    Language {
        code: "uk",
        name: "Українська",
        source: include_str!("../lang/uk.json"),
    },
    Language {
        code: "el",
        name: "Ελληνικά",
        source: include_str!("../lang/el.json"),
    },
];

thread_local! {
    static CURRENT: Cell<usize> = const { Cell::new(0) };
}

/// Every language's translations, read once. A file that cannot be read is
/// empty (its texts show in English; the test says why).
fn tables() -> &'static [BTreeMap<String, String>] {
    static TABLES: OnceLock<Vec<BTreeMap<String, String>>> = OnceLock::new();
    TABLES.get_or_init(|| {
        LANGUAGES
            .iter()
            .map(|language| {
                serde_json::from_str::<BTreeMap<String, String>>(language.source)
                    .unwrap_or_default()
            })
            .collect()
    })
}

/// Languages packages brought (M10a WP5), after the built-in ones: added
/// once, never taken away, so a thread's index into them stays right.
type Extra = (&'static Language, &'static BTreeMap<String, String>);

fn extras() -> &'static std::sync::Mutex<Vec<Extra>> {
    static EXTRAS: OnceLock<std::sync::Mutex<Vec<Extra>>> = OnceLock::new();
    EXTRAS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn extra(index: usize) -> Option<Extra> {
    let index = index.checked_sub(LANGUAGES.len())?;
    extras().lock().ok()?.get(index).copied()
}

/// Every language: the built-in ones, then the ones packages brought.
pub fn all_languages() -> Vec<&'static Language> {
    let mut all: Vec<&'static Language> = LANGUAGES.iter().collect();
    if let Ok(extras) = extras().lock() {
        all.extend(extras.iter().map(|(language, _)| *language));
    }
    all
}

/// Adds a language a package brings: its code (2 or 3 lowercase letters),
/// its name in itself and its translations. A code the editor has already
/// (built in or added before) keeps its first language; returns whether it
/// was added.
pub fn register(code: &str, name: &str, texts: BTreeMap<String, String>) -> bool {
    let code_ok = (2..=3).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_lowercase());
    let name = name.trim();
    if !code_ok || name.is_empty() || name.chars().count() > 40 || find(code).is_some() {
        return false;
    }
    let Ok(mut extras) = extras().lock() else {
        return false;
    };
    let language: &'static Language = Box::leak(Box::new(Language {
        code: Box::leak(code.to_owned().into_boxed_str()),
        name: Box::leak(name.to_owned().into_boxed_str()),
        source: "",
    }));
    extras.push((language, Box::leak(Box::new(texts))));
    true
}

/// The translations of the language `code`, as its file holds them.
pub fn translations(code: &str) -> Option<&'static BTreeMap<String, String>> {
    if let Some(index) = LANGUAGES.iter().position(|l| l.code == code) {
        return tables().get(index);
    }
    let extras = extras().lock().ok()?;
    extras
        .iter()
        .find(|(language, _)| language.code == code)
        .map(|(_, table)| *table)
}

/// The language of a code as people and systems write it: `tr`, `TR`,
/// `tr-TR`, `tr_TR.UTF-8`, `pt-BR`.
pub fn find(code: &str) -> Option<&'static Language> {
    let base = code
        .split(['-', '_', '.', '@'])
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|language| language.code == base)
        .or_else(|| {
            let extras = extras().lock().ok()?;
            extras
                .iter()
                .find(|(language, _)| language.code == base)
                .map(|(language, _)| *language)
        })
}

/// Shows the interface of this thread in `code`'s language. Returns whether
/// there is such a language (if not, nothing changes).
pub fn set_language(code: &str) -> bool {
    match find(code).and_then(|l| all_languages().iter().position(|x| x.code == l.code)) {
        Some(index) => {
            CURRENT.set(index);
            true
        }
        None => false,
    }
}

/// The language this thread's interface is shown in.
pub fn language() -> &'static Language {
    let index = CURRENT.get();
    LANGUAGES
        .get(index)
        .or_else(|| extra(index).map(|(language, _)| language))
        .unwrap_or(&LANGUAGES[0])
}

/// The language the person asked for outside the editor: `WISH_LANG`, then
/// the system's `LC_ALL`, `LC_MESSAGES` and `LANG` (a language the editor
/// does not have is skipped).
pub fn from_environment() -> Option<&'static Language> {
    ["WISH_LANG", "LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find_map(|value| find(&value))
}

/// `english` in the interface's language (itself if the language has no
/// translation for it).
pub fn t(english: &'static str) -> &'static str {
    let index = CURRENT.get();
    if index == 0 {
        return english;
    }
    let table = if index < LANGUAGES.len() {
        tables().get(index)
    } else {
        extra(index).map(|(_, table)| table)
    };
    table
        .and_then(|table| table.get(english))
        .map(String::as_str)
        .unwrap_or(english)
}

/// `english` in the interface's language with its `{names}` filled in:
/// `tf("opened {path}", &[("path", &path)])`. A translation may put the
/// names in any order.
pub fn tf(english: &'static str, args: &[(&str, &dyn Display)]) -> String {
    fill(t(english), args)
}

/// Fills the `{names}` of `template` in one pass (a value that holds braces
/// stays as it is).
fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                match args.iter().find(|(n, _)| *n == name) {
                    Some((_, value)) => out.push_str(&value.to_string()),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The `{names}` of a text, sorted (a translation must have the same ones).
pub fn placeholders(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        names.push(after[..close].to_owned());
        rest = &after[close + 1..];
    }
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_filled_in_any_order() {
        let n = 3;
        assert_eq!(
            fill("{a} then {b}", &[("b", &"two"), ("a", &n)]),
            "3 then two"
        );
        assert_eq!(fill("{x} stays", &[]), "{x} stays");
        assert_eq!(fill("{a}", &[("a", &"{b}")]), "{b}");
        assert_eq!(fill("open {", &[]), "open {");
        assert_eq!(placeholders("{b} {a}"), ["a", "b"]);
    }

    #[test]
    fn codes_are_read_as_systems_write_them() {
        assert_eq!(find("tr_TR.UTF-8").map(|l| l.code), Some("tr"));
        assert_eq!(find("pt-BR").map(|l| l.code), Some("pt"));
        assert_eq!(find("DE").map(|l| l.code), Some("de"));
        assert!(find("xx").is_none());
        assert!(find("").is_none());
    }

    #[test]
    fn the_language_is_per_thread_and_english_is_the_default() {
        assert_eq!(t("Undo"), "Undo");
        assert!(set_language("tr"));
        assert_eq!(language().code, "tr");
        assert_ne!(t("Undo"), "Undo");
        let other = std::thread::spawn(|| t("Undo")).join().unwrap_or("");
        assert_eq!(other, "Undo");
        assert!(!set_language("xx"));
        assert_eq!(language().code, "tr");
        set_language("en");
    }
}
