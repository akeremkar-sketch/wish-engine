//! The first-run flow (M9 WP3), headless: the welcome screen makes a
//! project from a template with an assistant, the editor opens on it in
//! the same window with the Agent panel shown; the project is then a recent
//! one and opens from the list; a project by hand starts from the empty
//! one; a folder that is not a project is explained.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use egui::Rect;
use serde_json::{Value, json};
use wish_editor::script::run_script;
use wish_editor::welcome::{Assistant, LaunchOptions, Launcher};
use wish_script::build::{Toolchain, pack_sdk};

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-welcome");
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

fn options(name: &str) -> (LaunchOptions, PathBuf) {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("welcome-{name}"));
    let _ = std::fs::remove_dir_all(&base);
    // The tests read the English texts, whatever the machine's language.
    std::fs::create_dir_all(base.join("prefs")).unwrap();
    std::fs::write(base.join("prefs/editor.json"), r#"{ "language": "en" }"#).unwrap();
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let options = LaunchOptions {
        software: true,
        preferences: Some(base.join("prefs").join("editor.json")),
        templates: wish_control::scaffold::templates(&examples),
        projects_folder: base.join("projects"),
        command: "wen".to_owned(),
        // Any file stands for Claude Code: nothing is started here.
        claude: Some(std::env::current_exe().unwrap()),
        build: true,
        toolchain: Some(toolchain().clone()),
        splash: false,
        engines: Some(base.join("config")),
        hand_off: false,
    };
    (options, base)
}

fn play(launcher: &mut Launcher, steps: Value) {
    run_script(launcher, &json!({ "size": [1280, 720], "steps": steps })).unwrap();
}

fn named(launcher: &Launcher, text: &str) -> Rect {
    launcher
        .widgets()
        .iter()
        .filter(|w| w.role != "TextRun" && w.role != "Label")
        .filter(|w| w.label.as_deref() == Some(text) || w.value.as_deref() == Some(text))
        .map(|w| w.rect)
        .min_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .unwrap_or_else(|| panic!("nothing called '{text}'"))
}

fn click(launcher: &mut Launcher, text: &str) {
    // A card below the visible part of the page is scrolled to first, as a
    // person would (the bar at the bottom covers the page's end).
    for _ in 0..10 {
        let before = named(launcher, text);
        if before.max.y < 560.0 {
            break;
        }
        play(
            launcher,
            json!([
                { "events": [{ "pointer": [640.0, 300.0] }] },
                { "events": [{ "scroll": [0.0, -120.0] }] },
                { "seconds": 0.4 },
                { "idle": 2 }
            ]),
        );
        // (Something that does not move with the page, like the bar's
        // Create button, stays where it is.)
        if named(launcher, text) == before {
            break;
        }
    }
    let rect = named(launcher, text);
    play(
        launcher,
        json!([
            { "events": [{ "pointer": [rect.center().x, rect.center().y] }] },
            { "events": [{ "click": [rect.center().x, rect.center().y] }] },
            { "idle": 3 }
        ]),
    );
}

#[test]
fn a_game_is_made_from_a_template_with_an_assistant_and_opens() {
    let (options, base) = options("template");
    let prefs = options.preferences.clone().unwrap();
    let mut launcher = Launcher::new(options);
    play(&mut launcher, json!([{ "idle": 2 }]));
    assert!(launcher.app().is_none());
    // Who helps: Claude Code was found, so it is chosen; start from Quiz.
    click(&mut launcher, "Quiz");
    let dir = launcher.new_project_folder().unwrap();
    assert_eq!(dir, base.join("projects").join("my-game"));
    click(&mut launcher, "Create");
    let app = launcher.app().expect("the editor opened");
    assert!(app.agent_panel_open(), "the assistant was chosen");
    // Its game code was built before the editor opened: the scene's types
    // are known.
    assert!(
        app.status().starts_with("Project created. Tell the agent"),
        "{}",
        app.status()
    );
    assert_eq!(app.call("check", &json!({}))["passed"], true);
    let info = app.call("project_info", &json!({}));
    assert_eq!(info["project"]["name"], "My game", "{info}");
    // The template's own files, not what the engine made in it.
    for file in [
        "scenes/main.scene.json",
        "scripts/src/lib.rs",
        "input.json",
        ".mcp.json",
        "CLAUDE.md",
    ] {
        assert!(dir.join(file).is_file(), "{file}");
    }
    // Built here, for this project.
    assert!(dir.join(".wish/build/game.wasm").is_file());
    let guide = std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
    assert!(guide.starts_with("# My game"), "{guide}");
    assert_eq!(
        std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap(),
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/quiz/scenes/main.scene.json")
        )
        .unwrap()
    );
    // The choices are kept, and the project is the first recent one.
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    assert_eq!(saved["assistant"], "claude");
    assert_eq!(
        saved["recent"][0],
        std::path::absolute(&dir).unwrap().display().to_string()
    );

    // A second start lists it; it opens from the list.
    let mut again_options = launcher_options_like(&prefs, &base);
    again_options.claude = None;
    let mut again = Launcher::new(again_options);
    play(&mut again, json!([{ "idle": 2 }]));
    click(&mut again, "my-game");
    let app = again.app().expect("opened from the list");
    assert!(app.status().starts_with("opened "), "{}", app.status());
    // Without Claude Code, nobody helps: the panel stays closed.
    assert!(!app.agent_panel_open());
}

/// The options of a second start that shares the first one's preferences
/// and projects folder.
fn launcher_options_like(prefs: &Path, base: &Path) -> LaunchOptions {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    LaunchOptions {
        software: true,
        preferences: Some(prefs.to_path_buf()),
        templates: wish_control::scaffold::templates(&examples),
        projects_folder: base.join("projects"),
        command: "wen".to_owned(),
        claude: Some(std::env::current_exe().unwrap()),
        build: true,
        toolchain: Some(toolchain().clone()),
        splash: false,
        engines: Some(base.join("config")),
        hand_off: false,
    }
}

#[test]
fn a_game_by_hand_starts_empty_and_a_wrong_folder_is_explained() {
    let (options, base) = options("by-hand");
    // A project of that name is there already: the next name is offered.
    std::fs::create_dir_all(base.join("projects").join("my-game")).unwrap();
    std::fs::write(base.join("projects/my-game/notes.txt"), "mine").unwrap();
    let mut launcher = Launcher::new(options);
    play(&mut launcher, json!([{ "idle": 2 }]));
    assert_eq!(
        launcher.new_project_folder().unwrap(),
        base.join("projects").join("my-game-2")
    );
    // A folder that is not a project.
    let open = launcher
        .widgets()
        .iter()
        .find(|w| w.role == "TextInput" && w.rect.min.x > 600.0)
        .map(|w| w.rect)
        .expect("the folder field");
    let not_a_project = base.join("projects").join("my-game");
    play(
        &mut launcher,
        json!([
            { "events": [{ "click": [open.center().x, open.center().y] }] },
            { "events": [{ "text": not_a_project.display().to_string() }] },
            { "idle": 1 }
        ]),
    );
    click(&mut launcher, "Open");
    assert!(launcher.app().is_none());
    assert!(
        launcher.status().contains("is not a project"),
        "{}",
        launcher.status()
    );
    // By hand, from the empty project.
    launcher.set_assistant(Assistant::Nobody);
    click(&mut launcher, "Empty project");
    click(&mut launcher, "Create");
    let app = launcher.app().expect("the editor opened");
    assert!(!app.agent_panel_open());
    assert_eq!(
        app.call("project_info", &json!({}))["project"]["name"],
        "My game 2"
    );
    assert!(
        app.status().starts_with("Project created. Press Play"),
        "{}",
        app.status()
    );
    assert_eq!(app.call("check", &json!({}))["passed"], true);
    // What was there before is untouched.
    assert_eq!(
        std::fs::read_to_string(base.join("projects/my-game/notes.txt")).unwrap(),
        "mine"
    );
}

/// M9 closing review: game code that does not compile is said, not
/// reported as a project ready to play.
#[test]
fn a_template_whose_code_does_not_build_says_so() {
    let (mut options, base) = options("broken-code");
    let template = base.join("broken-template");
    wish_control::scaffold::create_project(&template, "Broken", "wen").unwrap();
    std::fs::write(template.join("scripts/src/lib.rs"), "fn main( {").unwrap();
    options.templates = wish_control::scaffold::templates(&base)
        .into_iter()
        .filter(|t| t.id == "broken-template")
        .collect();
    let mut launcher = Launcher::new(options);
    launcher.set_assistant(Assistant::Nobody);
    play(&mut launcher, json!([{ "idle": 2 }]));
    click(&mut launcher, "Broken");
    click(&mut launcher, "Create");
    let app = launcher.app().expect("the editor opened");
    assert!(
        app.status().starts_with("the game code was not built: "),
        "{}",
        app.status()
    );
    // Nothing of the work is left beside the new project.
    let left: Vec<String> = std::fs::read_dir(base.join("projects"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["my-game"]);
}

/// M9: a crash report is told once, then not again.
#[test]
fn a_crash_report_is_told_once() {
    let (options, base) = options("crash-notice");
    let prefs = options.preferences.clone().unwrap();
    let crashes = base.join("crashes");
    assert_eq!(wish_editor::crash_notice(&prefs, &crashes), None);
    std::fs::create_dir_all(&crashes).unwrap();
    std::fs::write(crashes.join("crash-000000000001-wen.txt"), "report").unwrap();
    let told = wish_editor::crash_notice(&prefs, &crashes).expect("told");
    assert!(told.contains("crash-000000000001-wen.txt"), "{told}");
    assert_eq!(wish_editor::crash_notice(&prefs, &crashes), None);
    // The language the person chose is kept beside it.
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    assert_eq!(saved["language"], "en");
    std::fs::write(crashes.join("crash-000000000002-wen.txt"), "report").unwrap();
    assert!(wish_editor::crash_notice(&prefs, &crashes).is_some());
}

/// Paste (Ctrl+V) types into a field; copy (Ctrl+C) gives the selection to
/// the window for the system clipboard (found by driving the real window:
/// a request could not be pasted into the Agent panel).
#[test]
fn text_is_pasted_and_copied() {
    use wish_window::Shell;
    let (options, _) = options("clipboard");
    let mut launcher = Launcher::new(options);
    play(&mut launcher, json!([{ "idle": 2 }]));
    let name = launcher
        .widgets()
        .iter()
        .find(|w| w.role == "TextInput" && w.value.as_deref() == Some("My game"))
        .map(|w| w.rect)
        .expect("the name field");
    play(
        &mut launcher,
        json!([
            { "events": [{ "click": [name.center().x, name.center().y] }] },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "paste": "Pasted Game" }] },
            { "idle": 2 }
        ]),
    );
    assert!(
        launcher
            .new_project_folder()
            .unwrap()
            .ends_with("pasted-game"),
        "{:?}",
        launcher.new_project_folder()
    );
    play(
        &mut launcher,
        json!([
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "copy": true }] },
            { "idle": 1 }
        ]),
    );
    assert_eq!(launcher.take_clipboard().as_deref(), Some("Pasted Game"));
    assert_eq!(launcher.take_clipboard(), None, "taken once");
}

#[test]
fn the_mark_plays_when_the_engine_opens_and_a_click_skips_it() {
    let (mut options, _base) = options("splash");
    options.splash = true;
    let mut launcher = Launcher::new(options.clone());
    play(&mut launcher, json!([{ "idle": 3 }]));
    assert!(launcher.splash_playing());
    assert!(
        launcher
            .widgets()
            .iter()
            .all(|w| w.label.as_deref() != Some("Create"))
    );
    // It ends by itself.
    play(
        &mut launcher,
        json!([{ "seconds": 1.0 }, { "seconds": 1.2 }, { "idle": 2 }]),
    );
    assert!(!launcher.splash_playing());
    named(&launcher, "Create");
    // A click skips it.
    let mut again = Launcher::new(options);
    play(&mut again, json!([{ "idle": 2 }]));
    assert!(again.splash_playing());
    play(
        &mut again,
        json!([{ "events": [{ "click": [640.0, 360.0] }] }, { "idle": 2 }]),
    );
    assert!(!again.splash_playing());
    named(&again, "Create");
}

#[test]
fn the_hub_tabs_change_the_look_and_the_assistant() {
    let (options, base) = options("hub");
    let prefs = base.join("prefs").join("editor.json");
    let mut launcher = Launcher::new(options);
    play(&mut launcher, json!([{ "idle": 2 }]));
    assert!(
        launcher
            .widgets()
            .iter()
            .any(|w| w.label.as_deref() == Some("Create"))
    );
    // Look: a theme for everything that follows.
    click(&mut launcher, "Look");
    assert!(
        !launcher
            .widgets()
            .iter()
            .any(|w| w.label.as_deref() == Some("Create")),
        "the Projects page's bar is gone"
    );
    click(&mut launcher, "Midnight");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    assert_eq!(saved["theme"], "wish.theme.midnight/midnight.json");
    // AI: the default model.
    click(&mut launcher, "AI");
    click(&mut launcher, "Sonnet");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    assert_eq!(saved["agent"]["model"], "sonnet");
    // Learn: the shortcuts are listed.
    click(&mut launcher, "Learn");
    assert!(
        launcher
            .widgets()
            .iter()
            .any(|w| w.value.as_deref() == Some("Ctrl+K") || w.label.as_deref() == Some("Ctrl+K"))
    );
    // Back to the projects.
    click(&mut launcher, "Projects");
    named(&launcher, "Create");
}

#[test]
fn the_hub_searches_the_package_index() {
    let (options, base) = options("hub-index");
    let package = base.join("index/dusk");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("wish-package.json"),
        json!({ "format": 1, "id": "com.example.dusk", "name": "Dusk", "version": "1.0.0",
            "engine": ">=0.1", "license": "MIT", "description": "Evening colours.",
            "contents": { "themes": ["dusk.json"] } })
        .to_string(),
    )
    .unwrap();
    std::fs::write(package.join("dusk.json"), r#"{ "format": 1 }"#).unwrap();
    // The index folder is remembered in the preferences.
    std::fs::write(
        base.join("prefs/editor.json"),
        json!({ "language": "en", "package_index": base.join("index").display().to_string() })
            .to_string(),
    )
    .unwrap();
    let mut launcher = Launcher::new(options);
    play(&mut launcher, json!([{ "idle": 2 }]));
    click(&mut launcher, "Packages");
    click(&mut launcher, "Search");
    assert!(
        launcher
            .widgets()
            .iter()
            .any(|w| w.label.as_deref() == Some("Dusk") || w.value.as_deref() == Some("Dusk")),
        "the index's package is listed"
    );
    named(&launcher, "Add for me");
    // A folder that is not there is explained, not a crash.
    std::fs::remove_dir_all(base.join("index")).unwrap();
    click(&mut launcher, "Search");
    assert!(
        launcher.widgets().iter().any(|w| w
            .label
            .as_deref()
            .unwrap_or("")
            .contains("does not exist")
            || w.value.as_deref().unwrap_or("").contains("does not exist")),
        "the problem is shown"
    );
}

/// A folder that is an engine of version `version` (its `wen` is a stand-in:
/// nothing is started in these tests).
fn fake_engine(folder: &Path, version: &str) {
    std::fs::create_dir_all(folder).unwrap();
    std::fs::write(
        folder.join("wish-engine.json"),
        json!({ "format": 1, "version": version }).to_string(),
    )
    .unwrap();
    std::fs::write(folder.join(wish_control::engines::WEN), "").unwrap();
}

#[test]
fn engines_are_added_and_removed_in_the_hub_and_a_project_opens_with_its_own() {
    let (options, base) = options("hub-engines");
    let engine = base.join("engines/0.0.5");
    fake_engine(&engine, "0.0.5");
    // A recent project made with 0.0.5.
    let old = base.join("projects/old-game");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(
        old.join("wish.json"),
        r#"{ "engine": "0.0.5", "format": 1, "name": "Old", "scene": "scenes/main.scene.json" }"#,
    )
    .unwrap();
    std::fs::write(
        base.join("prefs/editor.json"),
        json!({ "language": "en", "recent": [old.display().to_string()] }).to_string(),
    )
    .unwrap();
    let mut launcher = Launcher::new(options);
    play(&mut launcher, json!([{ "idle": 2 }]));
    click(&mut launcher, "Engine versions");
    let field = launcher
        .widgets()
        .iter()
        .find(|w| w.role == "TextInput")
        .map(|w| w.rect)
        .expect("the folder field");
    play(
        &mut launcher,
        json!([
            { "events": [{ "click": [field.center().x, field.center().y] }] },
            { "events": [{ "text": engine.display().to_string() }] },
            { "idle": 1 }
        ]),
    );
    click(&mut launcher, "Add an engine");
    assert!(
        launcher.status().contains("0.0.5 added"),
        "{}",
        launcher.status()
    );
    let has = |launcher: &Launcher, text: &str| {
        launcher
            .widgets()
            .iter()
            .any(|w| w.label.as_deref() == Some(text) || w.value.as_deref() == Some(text))
    };
    assert!(has(&launcher, "Wish Engine 0.0.5"), "listed");
    let saved = std::fs::read_to_string(base.join("config/engines.json")).unwrap();
    assert!(saved.contains("0.0.5"), "{saved}");
    // The project made with 0.0.5 goes to that engine; this one does not
    // open it.
    click(&mut launcher, "Projects");
    click(&mut launcher, "old-game");
    assert_eq!(
        launcher.handed_off().map(|e| e.version.to_string()),
        Some("0.0.5".to_owned())
    );
    assert!(launcher.app().is_none());
    assert!(
        launcher.status().contains("Wish Engine 0.0.5"),
        "{}",
        launcher.status()
    );
    // Removed by hand: only forgotten, its files stay.
    click(&mut launcher, "Engine versions");
    click(&mut launcher, "Remove");
    assert!(!has(&launcher, "Wish Engine 0.0.5"), "gone from the list");
    assert!(engine.join("wish-engine.json").is_file(), "its files stay");
    // A folder that is not an engine is explained.
    let field = launcher
        .widgets()
        .iter()
        .find(|w| w.role == "TextInput")
        .map(|w| w.rect)
        .unwrap();
    play(
        &mut launcher,
        json!([
            { "events": [{ "click": [field.center().x, field.center().y] }] },
            { "events": [{ "text": base.join("projects").display().to_string() }] },
            { "idle": 1 }
        ]),
    );
    click(&mut launcher, "Add an engine");
    assert!(
        launcher.status().contains("is not an engine folder"),
        "{}",
        launcher.status()
    );
}
