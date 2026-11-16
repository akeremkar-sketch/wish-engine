//! M8a's exit criterion 1, end to end and by hand only: a copy of the
//! showcase game (`examples/islands`) is opened as a person would open a
//! fresh clone; its game code is built from the editor; a new part of the
//! level is made (an entity, its components, its place, a prefab of it and a
//! second instance); undo and redo work; the game is played, recorded and
//! played back; the project's tests pass; a picture is saved; `check` finds
//! no problems. Every change goes in through the interface (clicks, keys,
//! typing) and is recorded as the human's. No AI, no tool call made by the
//! test itself except to read what happened.
//!
//! Needs a software adapter and builds game code (the SDK pack, ~1 min the
//! first time); run it in release too (`cargo test --release -p wish_editor`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use egui::Rect;
use serde_json::{Value, json};
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::script::run_script;
use wish_editor::{EditorApp, ids};
use wish_script::build::{Toolchain, pack_sdk};

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-manual");
        let _ = std::fs::remove_dir_all(&sdk);
        pack_sdk(&cargo, &manifest, &tmp.join("sdk-target"), &sdk).unwrap();
        let out = std::process::Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            .unwrap();
        let sysroot = PathBuf::from(String::from_utf8(out.stdout).unwrap().trim());
        let rustc = sysroot
            .join("bin")
            .join(if cfg!(windows) { "rustc.exe" } else { "rustc" });
        Toolchain { sdk, rustc }
    })
}

/// Copies a folder (the example project), leaving out what a clone would
/// not have (`.wish/`: built code, history).
fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let name = entry.file_name();
        if name == ".wish" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            copy(&path, &to.join(&name));
        } else {
            std::fs::copy(&path, to.join(&name)).unwrap();
        }
    }
}

fn play(app: &mut EditorApp, steps: Value) {
    run_script(app, &json!({ "size": [1280, 720], "steps": steps })).unwrap();
}

fn find(app: &EditorApp, text: &str) -> Option<Rect> {
    app.widgets()
        .iter()
        .filter(|w| w.role != "TextRun" && w.role != "Label")
        .find(|w| w.label.as_deref() == Some(text) || w.value.as_deref() == Some(text))
        .map(|w| w.rect)
}

fn named(app: &EditorApp, text: &str) -> Rect {
    find(app, text).unwrap_or_else(|| panic!("nothing called '{text}' ({})", app.status()))
}

/// A widget called `text`, scrolled into view in the panel under `x`.
fn shown(app: &mut EditorApp, text: &str, x: f32) -> Rect {
    for _ in 0..30 {
        let rect = named(app, text);
        if rect.max.y < 480.0 && rect.min.y > 50.0 {
            return rect;
        }
        let dy = if rect.max.y >= 480.0 { -60 } else { 60 };
        play(
            app,
            json!([{ "events": [{ "pointer": [x, 300] }, { "scroll": [0, dy] }] }, { "seconds": 0.4 }, { "idle": 2 }]),
        );
    }
    named(app, text)
}

fn at(rect: Rect) -> [f32; 2] {
    [rect.center().x, rect.center().y]
}

fn click(app: &mut EditorApp, rect: Rect) {
    play(
        app,
        json!([{ "events": [{ "click": at(rect) }] }, { "idle": 2 }]),
    );
}

fn right_click(app: &mut EditorApp, rect: Rect) {
    play(
        app,
        json!([
            { "events": [{ "pointer": at(rect) }] },
            { "events": [{ "press": "secondary" }] },
            { "events": [{ "release": "secondary" }] },
            { "idle": 2 }
        ]),
    );
}

fn key(app: &mut EditorApp, name: &str, ctrl: bool) {
    play(
        app,
        json!([
            { "events": [{ "pointer": [600, 300] }, { "modifiers": { "ctrl": ctrl } }] },
            { "events": [{ "key": name, "ctrl": ctrl }] },
            { "events": [{ "key": name, "ctrl": ctrl, "up": true }, { "modifiers": {} }] },
            { "idle": 2 }
        ]),
    );
}

/// Clicks a box, replaces what it holds with `text` and presses Enter.
fn type_into(app: &mut EditorApp, rect: Rect, text: &str) {
    play(
        app,
        json!([
            { "events": [{ "click": at(rect) }] },
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": text }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
}

/// The inspector's boxes on the row of the field `label`, left to right,
/// scrolled into view first (a long inspector goes below the fold).
fn row_boxes(app: &mut EditorApp, label: &str) -> Vec<Rect> {
    for _ in 0..20 {
        let below = app
            .widgets()
            .iter()
            .filter(|w| w.role == "Label" && w.value.as_deref() == Some(label))
            .filter(|w| w.rect.min.x > 880.0)
            .any(|w| w.rect.max.y > 470.0);
        if !below {
            break;
        }
        play(
            app,
            json!([{ "events": [{ "pointer": [1100, 300] }, { "scroll": [0, -60] }] }, { "seconds": 0.4 }, { "idle": 2 }]),
        );
    }
    let name = app
        .widgets()
        .iter()
        .filter(|w| w.role == "Label" && w.value.as_deref() == Some(label))
        .filter(|w| w.rect.min.x > 880.0)
        .map(|w| w.rect)
        .next()
        .unwrap_or_else(|| panic!("no field '{label}'"));
    let mut boxes: Vec<Rect> = app
        .widgets()
        .iter()
        .filter(|w| w.role == "SpinButton")
        .filter(|w| (w.rect.center().y - name.center().y).abs() < 8.0)
        .map(|w| w.rect)
        .collect();
    boxes.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
    boxes
}

fn add_component(app: &mut EditorApp, name: &str) {
    let add = named(app, "Add component...");
    click(app, add);
    play(
        app,
        json!([{ "events": [{ "text": name }] }, { "idle": 2 }]),
    );
    let choice = app
        .widgets()
        .iter()
        .find(|w| w.role == "Button" && w.label.as_deref() == Some(name))
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("'{name}' is not offered"));
    click(app, choice);
}

fn project_menu(app: &mut EditorApp, path: &[&str]) {
    let menu = named(app, "Islands");
    click(app, menu);
    for item in path {
        let rect = named(app, item);
        play(
            app,
            json!([{ "events": [{ "pointer": at(rect) }] }, { "idle": 2 }]),
        );
        click(app, rect);
    }
}

fn translation(app: &mut EditorApp, id: &str) -> Value {
    app.call(
        "entity_get",
        &json!({ "ids": [id], "components": ["Transform"] }),
    )["entities"][0]["components"]["Transform"]["translation"]
        .clone()
}

#[test]
fn a_part_of_the_showcase_is_built_played_and_readied_by_hand_only() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("editor-manual-islands");
    let _ = std::fs::remove_dir_all(&dir);
    copy(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/islands"),
        &dir,
    );
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    workspace.set_toolchain(toolchain().clone());
    let mut app = EditorApp::new(workspace);
    play(&mut app, json!([{ "idle": 3 }]));

    // 1. A fresh clone: the game code is not built, the view says so and
    //    offers to build it.
    assert!(find(&app, "Show problems").is_some(), "{}", app.status());
    let build = named(&app, "Build code");
    click(&mut app, build);
    assert!(
        app.status().starts_with("game code built"),
        "{}",
        app.status()
    );
    play(&mut app, json!([{ "seconds": 2.5 }, { "idle": 3 }]));
    assert!(ids(app.rows()).contains("player"), "{}", app.status());

    // 2. A new island, far off the course: add, rename, components, place.
    let add = named(&app, "+");
    click(&mut app, add);
    // The new entity is selected (it may be below the hierarchy's fold):
    // F2 renames it.
    let added = app.selection().to_vec();
    assert_eq!(added.len(), 1, "{}", app.status());
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": [100, 300] }] },
            { "events": [{ "key": "F2" }] },
            { "idle": 2 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": "island_far" }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
    assert_eq!(app.selection(), ["island_far"], "{}", app.status());
    add_component(&mut app, "Primitive");
    add_component(&mut app, "Collider");
    let boxes = row_boxes(&mut app, "scale");
    type_into(&mut app, boxes[0], "4");
    let boxes = row_boxes(&mut app, "translation");
    type_into(&mut app, boxes[0], "200");
    assert_eq!(translation(&mut app, "island_far")[0], 200.0);

    // 3. A prefab of it, and a second one from the browser.
    let row = shown(&mut app, "island_far", 100.0);
    right_click(&mut app, row);
    let make = named(&app, "Make prefab");
    click(&mut app, make);
    assert_eq!(app.status(), "made prefab prefabs/island_far.prefab.json");
    play(&mut app, json!([{ "seconds": 2.5 }, { "idle": 2 }]));
    // The browser's Prefabs filter shows it at the top.
    let prefabs = app
        .widgets()
        .iter()
        .find(|w| {
            w.role == "Button" && w.label.as_deref() == Some("Prefabs") && w.rect.min.y > 500.0
        })
        .map(|w| w.rect)
        .unwrap();
    click(&mut app, prefabs);
    let file = named(&app, "prefabs/island_far.prefab.json");
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": at(file) }] },
            { "seconds": 0.5 },
            { "events": [{ "click": at(file) }] },
            { "events": [{ "click": at(file) }] },
            { "idle": 3 }
        ]),
    );
    let second = app
        .rows()
        .iter()
        .find(|row| {
            row.id != "island_far"
                && row.prefab.as_deref() == Some("prefabs/island_far.prefab.json")
        })
        .map(|row| row.id.clone())
        .unwrap_or_else(|| {
            panic!(
                "a second instance ({}): {:?}",
                app.status(),
                app.selection()
            )
        });
    app.select(&[&second]);
    play(&mut app, json!([{ "idle": 2 }]));
    let boxes = row_boxes(&mut app, "translation");
    type_into(&mut app, boxes[0], "212");
    assert_eq!(translation(&mut app, &second)[0], 212.0);

    // 4. Undo and redo, from the keyboard.
    key(&mut app, "Z", true);
    assert_ne!(translation(&mut app, &second)[0], 212.0);
    key(&mut app, "Y", true);
    assert_eq!(translation(&mut app, &second)[0], 212.0);

    // 5. Play it, recorded; stop; play the recording back.
    let record = named(&app, "Record");
    click(&mut app, record);
    key(&mut app, "P", true);
    assert!(app.playing(), "{}", app.status());
    play(&mut app, json!([{ "seconds": 1.0 }, { "idle": 2 }]));
    key(&mut app, "P", true);
    assert!(!app.playing());
    assert!(
        app.status().contains("replay take_1 saved"),
        "{}",
        app.status()
    );
    project_menu(&mut app, &["Replays", "take_1"]);
    assert!(app.status().contains("plays the same"), "{}", app.status());

    // 6. The project's tests still pass; a picture; no problems.
    // The new island makes the scene bigger, so its sun's shadow map covers
    // more and the winning picture's shadows move a little: the person
    // looks at it, takes it as the new golden picture, and all pass.
    project_menu(&mut app, &["Run tests"]);
    assert_eq!(
        app.status(),
        "tests: 3 passed, 1 failed",
        "{:?}",
        app.problems()
    );
    assert!(
        app.problems()[0]["message"]
            .as_str()
            .unwrap()
            .contains("golden"),
        "{:?}",
        app.problems()
    );
    project_menu(&mut app, &["Update golden pictures"]);
    project_menu(&mut app, &["Run tests"]);
    assert_eq!(
        app.status(),
        "tests: 4 passed, 0 failed",
        "{:?}",
        app.problems()
    );
    project_menu(&mut app, &["Save picture"]);
    assert!(dir.join("captures/main_1.png").exists(), "{}", app.status());
    project_menu(&mut app, &["Check"]);
    assert_eq!(app.status(), "check: no problems");

    // 7. Every change is in the one history, as the human's.
    let history = app.call("edit_history", &json!({ "action": "list", "limit": 100 }));
    let entries = history["entries"].as_array().unwrap();
    assert!(entries.len() >= 8, "{history}");
    for entry in entries {
        assert_eq!(entry["actor"], "human", "{entry}");
    }
    let saved = std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap();
    assert!(saved.contains("\"island_far\""));
    assert!(saved.contains("prefabs/island_far.prefab.json"));
}
