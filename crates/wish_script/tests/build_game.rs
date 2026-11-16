//! Building game code the way the engine does (ADR-025): an SDK pack, one
//! rustc call, the compiler's messages as diagnostics on the project's own
//! lines, and a failed build that leaves the last good module in place.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::json;
use wish_app::{App, AppConfig};
use wish_script::build::{GAME_WASM, SCRIPTS_ENTRY, Toolchain, build_game, pack_sdk};
use wish_script::{GameModule, Limits, ScriptEngine, ScriptRuntime};

const GAME: &str = include_str!("../guests/collect/src/lib.rs");

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack");
        // The folder is the test's own (a cache may have left anything in it).
        let _ = std::fs::remove_dir_all(&sdk);
        pack_sdk(&cargo, &manifest, &tmp.join("sdk-target"), &sdk).unwrap();
        let found = {
            let rustc = std::process::Command::new("rustc")
                .args(["--print", "sysroot"])
                .output()
                .unwrap();
            let sysroot = PathBuf::from(String::from_utf8(rustc.stdout).unwrap().trim());
            sysroot
                .join("bin")
                .join(if cfg!(windows) { "rustc.exe" } else { "rustc" })
        };
        Toolchain { sdk, rustc: found }
    })
}

fn project(name: &str, source: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("build-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("scripts/src")).unwrap();
    std::fs::write(dir.join(SCRIPTS_ENTRY), source).unwrap();
    dir
}

fn play(root: &Path) -> serde_json::Value {
    let engine = ScriptEngine::new(Limits::default()).unwrap();
    let module = GameModule::load(
        &engine,
        GAME_WASM,
        &std::fs::read(root.join(GAME_WASM)).unwrap(),
    )
    .unwrap();
    let mut app = App::new(AppConfig::default());
    wish_transform::register(app.world_mut()).unwrap();
    module.install(app.world_mut()).unwrap();
    for system in ScriptRuntime::new(module).systems() {
        app.add_system(system).unwrap();
    }
    let world = app.world_mut();
    let player = world.spawn().unwrap();
    world
        .set_json(player, "Velocity", &json!({ "value": [6, 0, 0] }))
        .unwrap();
    world.set_json(player, "Transform", &json!({})).unwrap();
    world.set_json(player, "Player", &json!({})).unwrap();
    for _ in 0..60 {
        assert!(app.step().unwrap().failures.is_empty());
    }
    app.world().resource_json("Score").unwrap()
}

fn line_of(source: &str, text: &str) -> u32 {
    u32::try_from(source.lines().position(|l| l.contains(text)).unwrap()).unwrap() + 1
}

#[test]
fn game_code_builds_runs_and_reports_its_mistakes_on_its_own_lines() {
    let toolchain = toolchain();
    let root = project("game", GAME);
    let report = build_game(toolchain, &root, true).unwrap();
    assert!(report.passed, "{:?}", report.diagnostics);
    assert_eq!(report.errors, 0);
    // Only the bonus coin is in reach of a player who starts at 0 here.
    assert_eq!(play(&root), json!({ "points": 10, "coins": 1 }));
    let good = std::fs::read(root.join(GAME_WASM)).unwrap();

    // A one-line change rebuilds quickly (incremental).
    let edited = GAME.replace("Coin { value: 10 }", "Coin { value: 20 }");
    std::fs::write(root.join(SCRIPTS_ENTRY), &edited).unwrap();
    let again = build_game(toolchain, &root, true).unwrap();
    assert!(again.passed);
    println!(
        "first build {:.2} s, after a one-line edit {:.2} s",
        report.seconds, again.seconds
    );
    assert!(again.seconds < 10.0, "{} s", again.seconds);
    assert_eq!(play(&root), json!({ "points": 20, "coins": 1 }));
    let good = std::fs::read(root.join(GAME_WASM)).unwrap_or(good);

    // A type error: reported on its line, with the compiler's code and a
    // hint; the last good module stays.
    let broken = edited.replace(
        "score.points += event.value;",
        "score.points += event.value + 0.5;",
    );
    std::fs::write(root.join(SCRIPTS_ENTRY), &broken).unwrap();
    let failed = build_game(toolchain, &root, true).unwrap();
    assert!(!failed.passed);
    assert!(failed.errors >= 1);
    let first = &failed.diagnostics[0];
    assert_eq!(first.code, "script_compile_error", "{first:?}");
    assert_eq!(first.location.file.as_deref(), Some(SCRIPTS_ENTRY));
    assert_eq!(
        first.location.line,
        Some(line_of(&broken, "event.value + 0.5"))
    );
    assert!(first.message.contains("[E0"), "{first:?}");
    assert!(!first.hint.is_empty());
    assert_eq!(std::fs::read(root.join(GAME_WASM)).unwrap(), good);

    // A warning does not stop the build.
    let warned = edited.replace(
        "fn score(collected: EventReader<Collected>, mut score: ResMut<Score>) {",
        "fn score(collected: EventReader<Collected>, mut score: ResMut<Score>) {\n    let unused = 3;",
    );
    std::fs::write(root.join(SCRIPTS_ENTRY), &warned).unwrap();
    let warning = build_game(toolchain, &root, true).unwrap();
    assert!(warning.passed);
    assert!(warning.warnings >= 1);
    assert_eq!(warning.diagnostics[0].code, "script_compile_warning");
}

#[test]
fn a_project_without_game_code_or_a_mismatched_sdk_is_explained() {
    let toolchain = toolchain();
    let empty = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("build-empty");
    let _ = std::fs::remove_dir_all(&empty);
    std::fs::create_dir_all(&empty).unwrap();
    let missing = build_game(toolchain, &empty, true).unwrap_err();
    assert_eq!(missing.code, "script_sources_missing");
    assert!(missing.hint.contains("scripts/src/lib.rs"));

    // An SDK built by another compiler.
    let other = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sdk-other");
    let _ = std::fs::remove_dir_all(&other);
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(
        other.join("sdk.json"),
        r#"{ "format": 1, "rustc": "rustc 0.1.0", "externs": {} }"#,
    )
    .unwrap();
    let root = project("mismatch", GAME);
    let mismatch = build_game(
        &Toolchain {
            sdk: other,
            rustc: toolchain.rustc.clone(),
        },
        &root,
        true,
    )
    .unwrap_err();
    assert_eq!(mismatch.code, "script_toolchain_mismatch");
}
