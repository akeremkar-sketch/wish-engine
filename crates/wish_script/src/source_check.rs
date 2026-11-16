//! What game code may not do while it is compiled (M5 closing review).
//!
//! The compiler runs on the user's machine with the user's rights. Game
//! code is untrusted, so before `rustc` sees it, its sources are checked
//! for the few ways Rust reads the outside world at compile time:
//! `include_str!`, `include_bytes!`, `include!` (files), `env!` and
//! `option_env!` (environment variables), and `#[path = "..."]` (a module
//! from any file). Their names are refused anywhere as identifiers (a macro
//! cannot call them without naming them), and attributes may not name
//! `path` or be built from macro variables. Links inside `scripts/` are
//! refused too: a `mod` file must be a real file of the project.

use std::path::Path;

use wish_core::{Diagnostic, Result};

/// The names game code may not use.
const FORBIDDEN: &[&str] = &[
    "include_str",
    "include_bytes",
    "include",
    "env",
    "option_env",
];

/// The most files and folders looked at under `scripts/`.
const MAX_ENTRIES: usize = 10_000;

const HINT: &str = "Game code cannot read files or environment variables while it is built (include_str!, include_bytes!, include!, env!, option_env!, #[path]); rename a variable that uses one of these names. Data belongs in components, resources or assets.";

fn refused(file: &str, line: u32, what: &str) -> Diagnostic {
    Diagnostic::error(
        "script_forbidden_source",
        format!("Game code may not use {what}."),
        HINT,
    )
    .at_file(file)
    .at_line(line, None)
}

/// Checks every file under `<root>/scripts`: no links, and no `.rs` file
/// that reads the outside world at compile time.
pub fn check_sources(root: &Path) -> Result<()> {
    let scripts = root.join("scripts");
    let mut folders = vec![scripts];
    let mut seen = 0usize;
    while let Some(folder) = folders.pop() {
        let entries = std::fs::read_dir(&folder).map_err(|error| {
            Diagnostic::error(
                "script_build_failed",
                format!("Could not read '{}': {error}.", folder.display()),
                "Check that the project's scripts folder can be read.",
            )
        })?;
        let mut entries: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
        entries.sort();
        for path in entries {
            seen += 1;
            if seen > MAX_ENTRIES {
                return Err(Diagnostic::error(
                    "script_forbidden_source",
                    format!("The scripts folder has more than {MAX_ENTRIES} files and folders."),
                    "Keep only the game's sources in scripts/.",
                ));
            }
            let relative = path.strip_prefix(root).map_or_else(
                |_| path.display().to_string(),
                |p| p.to_string_lossy().replace('\\', "/"),
            );
            let meta = std::fs::symlink_metadata(&path).map_err(|error| {
                Diagnostic::error(
                    "script_build_failed",
                    format!("Could not read '{relative}': {error}."),
                    "Check that the project's scripts folder can be read.",
                )
            })?;
            if meta.file_type().is_symlink() || is_reparse_point(&meta) {
                return Err(Diagnostic::error(
                    "script_forbidden_source",
                    format!("'{relative}' is a link; game code may only use real files of the project."),
                    "Replace the link with the file itself (a link could make the compiler read a file outside the project).",
                )
                .at_file(&relative));
            }
            if meta.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).map_err(|error| {
                    Diagnostic::error(
                        "script_build_failed",
                        format!("Could not read '{relative}': {error}."),
                        "Game code files must be UTF-8 text.",
                    )
                })?;
                check_text(&relative, &text)?;
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_: &std::fs::Metadata) -> bool {
    false
}

/// Checks one source file (`file` is its name for diagnostics).
pub fn check_text(file: &str, text: &str) -> Result<()> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut line = 1u32;
    // Bracket depth inside an attribute (`#[...]`), 0 outside.
    let mut attribute = 0usize;
    // The last two significant punctuation characters, for `#[` / `#![`.
    let mut last: [char; 2] = [' ', ' '];
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // Comments.
        if c == '/' && at(i + 1) == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && at(i + 1) == '*' {
            let mut depth = 0usize;
            while i < chars.len() {
                if chars[i] == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            continue;
        }
        // Raw strings: r"..", r#".."#, br#".."#, cr#".."#.
        let prefix = match (c, at(i + 1)) {
            ('r', '"' | '#') => Some(1),
            ('b' | 'c', 'r') if matches!(at(i + 2), '"' | '#') => Some(2),
            _ => None,
        };
        if let Some(prefix) = prefix {
            let mut j = i + prefix;
            let mut hashes = 0;
            while at(j) == '#' {
                hashes += 1;
                j += 1;
            }
            if at(j) == '"' {
                j += 1;
                loop {
                    if j >= chars.len() {
                        break;
                    }
                    if chars[j] == '\n' {
                        line += 1;
                    }
                    if chars[j] == '"' && (1..=hashes).all(|k| at(j + k) == '#') {
                        j += 1 + hashes;
                        break;
                    }
                    j += 1;
                }
                i = j;
                last = [last[1], '"'];
                continue;
            }
            if prefix == 1 && hashes == 1 && is_ident_start(at(j)) {
                // A raw identifier, r#name: check it as `name`.
                i = j;
                continue;
            }
        }
        // Strings: "..", b"..", c"..".
        if c == '"' || (matches!(c, 'b' | 'c') && at(i + 1) == '"') {
            let mut j = if c == '"' { i + 1 } else { i + 2 };
            while j < chars.len() && chars[j] != '"' {
                if chars[j] == '\\' {
                    j += 1;
                }
                if at(j) == '\n' {
                    line += 1;
                }
                j += 1;
            }
            i = j + 1;
            last = [last[1], '"'];
            continue;
        }
        // Characters and lifetimes.
        if c == '\'' || (c == 'b' && at(i + 1) == '\'') {
            let start = if c == '\'' { i + 1 } else { i + 2 };
            if at(start) == '\\' {
                let mut j = start + 1;
                while j < chars.len() && chars[j] != '\'' && chars[j] != '\n' {
                    j += 1;
                }
                i = j + 1;
            } else if at(start + 1) == '\'' {
                i = start + 2;
            } else {
                // A lifetime or label: skip the quote, the name follows.
                i = start;
            }
            last = [last[1], '\''];
            continue;
        }
        if is_ident_start(c) {
            let mut j = i;
            while j < chars.len() && is_ident_continue(chars[j]) {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            if FORBIDDEN.contains(&word.as_str()) {
                return Err(refused(file, line, &format!("'{word}'")));
            }
            if attribute > 0 && word == "path" {
                return Err(refused(file, line, "#[path] (a module from another file)"));
            }
            i = j;
            last = [last[1], 'a'];
            continue;
        }
        if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            last = [last[1], '0'];
            continue;
        }
        // Punctuation.
        match c {
            '[' if attribute > 0 => attribute += 1,
            '[' if last[1] == '#' || (last[0] == '#' && last[1] == '!') => attribute = 1,
            ']' if attribute > 0 => attribute -= 1,
            '$' if attribute > 0 => {
                return Err(refused(
                    file,
                    line,
                    "an attribute built from a macro variable (#[$...])",
                ));
            }
            _ => {}
        }
        last = [last[1], c];
        i += 1;
    }
    Ok(())
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::check_text;

    fn refused(text: &str) -> bool {
        check_text("scripts/src/lib.rs", text).is_err()
    }

    #[test]
    fn ways_of_reading_the_outside_world_are_refused() {
        for text in [
            r#"const A: &str = include_str!("C:/Windows/win.ini");"#,
            r#"const A: &[u8] = core::include_bytes!("../x");"#,
            r#"include!("/etc/passwd");"#,
            r#"const W: &str = env!("USERPROFILE");"#,
            r#"const W: Option<&str> = option_env!("HOME");"#,
            r#"use std::env as e;"#,
            r#"const A: &str = r#include_str!("x");"#,
            "#[path = \"C:/secret.rs\"] mod x;",
            "#![path = \"x\"]",
            "#[cfg_attr(all(), path = \"C:/x.rs\")] mod x;",
            "macro_rules! m { ($a:meta) => { #[$a] mod y; } }",
            "macro_rules! m { ($i:ident) => { $i!(\"X\") } } m!(env);",
        ] {
            assert!(refused(text), "{text}");
        }
    }

    #[test]
    fn ordinary_game_code_is_fine() {
        let text = r##"
            use wish_sdk::prelude::*;
            /// A path the player follows (the word is fine outside attributes).
            #[component]
            pub struct Waypoint { /// Index.
                pub path: u32 }
            // include_str!("comments are not code"), env!("X")
            /* nested /* include!("x") */ still a comment env!() */
            const NAME: &str = "include_str!(\"in a string\") env!";
            const RAW: &str = r#"env!("raw")"#;
            const C: char = '\'';
            fn f<'a>(x: &'a str) -> &'a str { let environment = 1; let _ = environment; x }
            #[system(after = "move_player")]
            fn s() { let path = 3; let _ = path; }
        "##;
        check_text("scripts/src/lib.rs", text).unwrap();
        let err = check_text(
            "scripts/src/lib.rs",
            "fn a() {}\nconst W: &str = env!(\"X\");",
        )
        .unwrap_err();
        assert_eq!(err.code, "script_forbidden_source");
        assert_eq!(err.location.line, Some(2));
        assert!(!err.hint.is_empty());
    }
}
