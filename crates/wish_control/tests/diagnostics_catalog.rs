//! Every diagnostic code the engine's source creates has a page in the
//! catalogue (`wish://docs/diagnostics/<code>`), so the `docs` link in an
//! error never leads nowhere. (Codes made in other ways are covered by the
//! fuzz test, which checks every code an answer carries.)

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Calls whose first string literal is a diagnostic code.
const CONSTRUCTORS: &[&str] = &[
    "Diagnostic::error(",
    "Diagnostic::warning(",
    "Diagnostic::info(",
    "Diagnostic::new(",
    "gpu_error(",
    "failure(",
    "warn(",
    "problem(",
];

fn is_code(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && text.starts_with(|c: char| c.is_ascii_lowercase())
        && !text.ends_with('_')
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n != "tests" && n != "target")
            {
                sources(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The codes the source creates, found by reading it.
fn codes_in_source() -> BTreeSet<String> {
    let crates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&crates).unwrap().flatten() {
        let src = entry.path().join("src");
        if src.is_dir() {
            sources(&src, &mut files);
        }
    }
    let mut codes = BTreeSet::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        // Unit tests at the end of a file make their own diagnostics.
        let text = text.split("#[cfg(test)]").next().unwrap_or_default();
        for constructor in CONSTRUCTORS {
            let mut rest = text;
            while let Some(at) = rest.find(constructor) {
                // `io_failure(` is not `failure(`: the name must start here.
                let starts_word = rest[..at]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
                rest = &rest[at + constructor.len()..];
                if !starts_word {
                    continue;
                }
                let window: String = rest.chars().take(160).collect();
                // The first string literal of the call.
                let Some(open) = window.find('"') else {
                    continue;
                };
                // Only if nothing but plain arguments comes before it.
                let before = &window[..open];
                if before.contains('(') || before.contains(')') || before.contains(';') {
                    continue;
                }
                let after = &window[open + 1..];
                let Some(close) = after.find('"') else {
                    continue;
                };
                let literal = &after[..close];
                if is_code(literal) {
                    codes.insert(literal.to_owned());
                }
            }
        }
    }
    codes
}

#[test]
fn every_code_in_the_source_has_a_catalogue_page() {
    let catalogued: BTreeSet<&str> = wish_control::docs::DIAGNOSTICS
        .iter()
        .map(|(code, _)| *code)
        .collect();
    let found = codes_in_source();
    assert!(
        found.len() > 100,
        "the scan found only {} codes",
        found.len()
    );
    let missing: Vec<&String> = found
        .iter()
        .filter(|code| !catalogued.contains(code.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "{} code(s) have no entry in wish_control::docs::DIAGNOSTICS: {missing:?}",
        missing.len()
    );
}

#[test]
fn the_catalogue_is_sorted_unique_and_written_out() {
    let codes: Vec<&str> = wish_control::docs::DIAGNOSTICS
        .iter()
        .map(|(code, _)| *code)
        .collect();
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(codes, sorted, "keep DIAGNOSTICS sorted, without repeats");
    for (code, text) in wish_control::docs::DIAGNOSTICS {
        assert!(is_code(code), "{code}");
        assert!(
            text.len() > 30 && text.ends_with('.'),
            "{code}: write a sentence or two"
        );
    }
}
