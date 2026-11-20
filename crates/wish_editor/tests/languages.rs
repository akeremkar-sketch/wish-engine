//! The editor's languages (M8a WP8): every text of the interface is in every
//! language file, nothing else is, the `{names}` match, the font has every
//! letter, and the interface still fits in each language.
//!
//! A text a file lacks would show in English; this test lists such texts
//! so that a translator knows what to add (`lang/<code>.json`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::json;
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::i18n::{LANGUAGES, placeholders, translations};
use wish_editor::script::run_script;
use wish_editor::{Editor, EditorApp};

/// The English texts the code shows: every `t("...")` and `tf("...", ...)`.
fn texts_in_code() -> BTreeSet<String> {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = BTreeSet::new();
    let mut not_literal = Vec::new();
    for entry in std::fs::read_dir(&src).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap() == "i18n.rs" {
            continue;
        }
        let code = std::fs::read_to_string(&path).unwrap();
        let bytes = code.as_bytes();
        for call in ["t(", "tf("] {
            let mut from = 0;
            while let Some(at) = code[from..].find(call) {
                let start = from + at;
                from = start + call.len();
                // A call of the function, not the end of another name.
                if start > 0 {
                    let before = bytes[start - 1];
                    if before.is_ascii_alphanumeric() || before == b'_' || before == b'.' {
                        continue;
                    }
                }
                if code[..start].ends_with("fn ") {
                    continue;
                }
                let rest = code[from..].trim_start();
                let Some(literal) = rest.strip_prefix('"') else {
                    not_literal.push(format!(
                        "{}: {}",
                        path.display(),
                        &code[start..(start + 40).min(code.len())]
                    ));
                    continue;
                };
                let mut text = String::new();
                let mut chars = literal.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => text.push('\n'),
                            Some(other) => text.push(other),
                            None => {}
                        },
                        c => text.push(c),
                    }
                }
                found.insert(text);
            }
        }
    }
    assert!(
        not_literal.is_empty(),
        "t() and tf() take the English text itself, so that this test finds it: {not_literal:#?}"
    );
    found
}

#[test]
fn every_language_has_every_text_and_nothing_else() {
    let texts = texts_in_code();
    assert!(
        texts.len() > 100,
        "the scan found the texts: {}",
        texts.len()
    );
    let mut problems = Vec::new();
    for language in &LANGUAGES[1..] {
        let parsed: Result<serde_json::Map<String, serde_json::Value>, _> =
            serde_json::from_str(language.source);
        if let Err(error) = parsed {
            problems.push(format!("lang/{}.json is not JSON: {error}", language.code));
            continue;
        }
        let table = translations(language.code).unwrap();
        let missing: Vec<_> = texts.iter().filter(|t| !table.contains_key(*t)).collect();
        let extra: Vec<_> = table.keys().filter(|k| !texts.contains(*k)).collect();
        if !missing.is_empty() {
            problems.push(format!(
                "lang/{}.json lacks (shown in English): {missing:#?}",
                language.code
            ));
        }
        if !extra.is_empty() {
            problems.push(format!(
                "lang/{}.json has texts the code no longer shows: {extra:#?}",
                language.code
            ));
        }
        for (english, translated) in table {
            if placeholders(english) != placeholders(translated) {
                problems.push(format!(
                    "lang/{}.json: '{translated}' must have the names of '{english}'",
                    language.code
                ));
            }
            if translated.trim().is_empty() {
                problems.push(format!("lang/{}.json: '{english}' is empty", language.code));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_font_has_every_letter_of_every_language() {
    let mut editor = Editor::new();
    // Fonts are ready after the first frame.
    editor.layout((64, 64), 0.0, Vec::new(), |_| {});
    let font = egui::FontId::proportional(14.0);
    let mut lacking = Vec::new();
    for language in LANGUAGES {
        let table = translations(language.code).unwrap();
        // English is the code's own texts (the keys).
        let english = if language.code == "en" {
            texts_in_code()
        } else {
            std::collections::BTreeSet::new()
        };
        let all = table
            .values()
            .map(String::as_str)
            .chain(english.iter().map(String::as_str))
            .chain([language.name])
            .collect::<Vec<_>>();
        for text in all {
            for c in text.chars() {
                if !editor
                    .context()
                    .fonts_mut(|fonts| fonts.has_glyph(&font, c))
                {
                    lacking.push(format!("{}: '{c}' in '{text}'", language.code));
                }
            }
        }
    }
    assert!(lacking.is_empty(), "{lacking:#?}");
}

#[test]
fn the_interface_fits_and_is_translated_in_every_language() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("editor-languages");
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    let mut problems = Vec::new();
    for language in LANGUAGES {
        assert!(app.set_language(language.code));
        run_script(
            &mut app,
            &json!({ "size": [1280, 720], "steps": [{ "idle": 2 }] }),
        )
        .unwrap();
        let undo = wish_editor::i18n::t("Undo");
        if language.code != "en" && undo == "Undo" {
            problems.push(format!("{}: 'Undo' is not translated", language.code));
        }
        let bar: Vec<_> = app
            .widgets()
            .iter()
            .filter(|w| w.role == "Button" && w.rect.max.y < 72.0)
            .collect();
        if !bar.iter().any(|w| w.label.as_deref() == Some(undo)) {
            problems.push(format!("{}: no '{undo}' in the menu bar", language.code));
        }
        for widget in bar {
            if widget.rect.max.x > 1280.0 {
                problems.push(format!(
                    "{}: '{}' is off the window at 1280 px ({})",
                    language.code,
                    widget.label.as_deref().unwrap_or("?"),
                    widget.rect.max.x
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn the_choice_of_language_is_kept_in_the_preferences() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("editor-language-prefs");
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    let open = || {
        let mut workspace = Workspace::open(WorkspaceOptions {
            root: dir.clone(),
            example: None,
        })
        .unwrap();
        workspace.set_software(true);
        EditorApp::new(workspace)
    };
    let prefs = dir.join("prefs").join("editor.json");
    std::fs::create_dir_all(prefs.parent().unwrap()).unwrap();
    std::fs::write(&prefs, r#"{ "language": "de" }"#).unwrap();
    let mut app = open();
    app.use_preferences(prefs.clone());
    assert_eq!(app.language(), "de");
    let step = |app: &mut EditorApp, events: serde_json::Value| {
        run_script(
            app,
            &json!({ "size": [1280, 720], "steps": [{ "events": events }, { "idle": 2 }] }),
        )
        .unwrap();
    };
    step(&mut app, json!([]));
    // Project menu → Language → Türkçe.
    let find = |app: &EditorApp, label: &str| {
        app.widgets()
            .iter()
            .find(|w| w.role == "Button" && w.label.as_deref() == Some(label))
            .map(|w| w.rect.center())
            .unwrap_or_else(|| panic!("no '{label}'"))
    };
    let menu = find(&app, wish_editor::i18n::t("View"));
    step(&mut app, json!([{ "click": [menu.x, menu.y] }]));
    let language = find(&app, wish_editor::i18n::t("Language"));
    step(&mut app, json!([{ "pointer": [language.x, language.y] }]));
    step(&mut app, json!([{ "click": [language.x, language.y] }]));
    let turkish = find(&app, "Türkçe");
    step(&mut app, json!([{ "click": [turkish.x, turkish.y] }]));
    assert_eq!(app.language(), "tr");
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    assert_eq!(saved["language"], "tr");
    // A new editor opens in Turkish.
    let mut again = open();
    again.use_preferences(prefs);
    assert_eq!(again.language(), "tr");
    wish_editor::i18n::set_language("en");
}
