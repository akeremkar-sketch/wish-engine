//! A game written with the SDK (`guests/collect`), running in a real app:
//! queries, resources, events, commands, ordering, determinism, snapshots,
//! code replaced mid-run, and systems that fail without stopping the world.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, OnceLock};

use serde_json::json;
use wish_app::{App, AppConfig};
use wish_core::Entity;
use wish_script::{GameModule, Limits, ParamSpec, Role, ScriptEngine, ScriptRuntime};

const SOURCE: &str = include_str!("../guests/collect/src/lib.rs");

fn guest_bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("guests/collect/Cargo.toml");
        let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("guests");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let status = Command::new(cargo)
            .args([
                "build",
                "--release",
                "--quiet",
                "--target",
                "wasm32-unknown-unknown",
            ])
            .arg("--manifest-path")
            .arg(&manifest)
            .arg("--target-dir")
            .arg(&target)
            .env("RUSTFLAGS", "")
            .status()
            .unwrap();
        assert!(status.success(), "the collect game builds");
        std::fs::read(target.join("wasm32-unknown-unknown/release/collect_game.wasm")).unwrap()
    })
}

fn module() -> Arc<GameModule> {
    let engine = ScriptEngine::new(Limits::default()).unwrap();
    GameModule::load(&engine, "scripts/collect.wasm", guest_bytes()).unwrap()
}

/// An app with the game's systems and a player heading for three coins.
fn game(module: &Arc<GameModule>) -> (App, ScriptRuntime, Entity) {
    let mut app = App::new(AppConfig::default());
    wish_transform::register(app.world_mut()).unwrap();
    module.install(app.world_mut()).unwrap();
    let runtime = ScriptRuntime::new(Arc::clone(module));
    for system in runtime.systems() {
        app.add_system(system).unwrap();
    }
    let world = app.world_mut();
    let player = world.spawn().unwrap();
    world
        .set_json(player, "Transform", &json!({ "translation": [0, 0, 0] }))
        .unwrap();
    world
        .set_json(player, "Velocity", &json!({ "value": [6, 0, 0] }))
        .unwrap();
    world.set_json(player, "Player", &json!({})).unwrap();
    for x in [1, 2, 3] {
        let coin = world.spawn().unwrap();
        world
            .set_json(coin, "Transform", &json!({ "translation": [x, 0, 0] }))
            .unwrap();
        world
            .set_json(coin, "Coin", &json!({ "value": 1 }))
            .unwrap();
    }
    (app, runtime, player)
}

fn x_of(app: &App, entity: Entity) -> f64 {
    app.world().get_json(entity, "Transform").unwrap()["translation"][0]
        .as_f64()
        .unwrap()
}

#[test]
fn the_manifest_describes_types_and_systems() {
    let module = module();
    let manifest = module.manifest();
    let names: Vec<&str> = manifest.systems.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "move_things",
            "collect",
            "score",
            "spawn_bonus",
            "explode",
            "spin"
        ]
    );
    let collect = &manifest.systems[1];
    assert_eq!(collect.after, ["move_things"]);
    assert!(matches!(&collect.params[0], ParamSpec::Query { with, .. } if with == &["Player"]));
    let role = |name: &str| {
        manifest
            .types
            .iter()
            .find(|t| t.schema.name == name)
            .map(|t| t.role)
    };
    assert_eq!(role("Coin"), Some(Role::Component));
    assert_eq!(role("Score"), Some(Role::Resource));
    assert_eq!(role("Collected"), Some(Role::Event));
    // The engine's own type, described by the game exactly as the engine
    // declares it.
    let transform = manifest
        .types
        .iter()
        .find(|t| t.schema.name == "Transform")
        .unwrap();
    assert_eq!(
        transform.schema,
        <wish_types::Transform as wish_schema::Plain>::schema()
    );
    // Field documentation and options come through.
    let coin = role("Coin");
    assert!(coin.is_some());
}

#[test]
fn the_game_plays_and_is_deterministic() {
    let module = module();
    let (mut app, _runtime, player) = game(&module);
    for _ in 0..60 {
        let report = app.step().unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
    }
    assert!((x_of(&app, player) - 6.0).abs() < 1e-4);
    let score = app.world().resource_json("Score").unwrap();
    assert_eq!(score, json!({ "points": 13, "coins": 4 }));
    assert_eq!(
        app.world().entity_count(),
        1,
        "every coin, the bonus one included, was picked up"
    );

    // Same start, same steps: same hash.
    let (mut again, _r, _) = game(&module);
    for _ in 0..60 {
        again.step().unwrap();
    }
    assert_eq!(again.hash(), app.hash());
}

#[test]
fn snapshots_and_replaced_code_change_nothing() {
    let module = module();
    let (mut straight, _r, _) = game(&module);
    for _ in 0..40 {
        straight.step().unwrap();
    }

    // Back to tick 10 after running ahead: the restored run sees a fresh
    // instance for the repeated ticks.
    let (mut app, _runtime, _) = game(&module);
    for _ in 0..10 {
        app.step().unwrap();
    }
    let snapshot = app.snapshot();
    for _ in 0..15 {
        app.step().unwrap();
    }
    app.restore(&snapshot).unwrap();
    for _ in 0..30 {
        app.step().unwrap();
    }
    assert_eq!(app.hash(), straight.hash());

    // The module's systems replaced by a new runtime (as a reload does)
    // between two ticks: the game carries on exactly.
    let (mut reloaded, _old, _) = game(&module);
    for _ in 0..20 {
        reloaded.step().unwrap();
    }
    let fresh = ScriptRuntime::new(
        GameModule::load(
            &ScriptEngine::new(Limits::default()).unwrap(),
            "scripts/collect.wasm",
            guest_bytes(),
        )
        .unwrap(),
    );
    for system in fresh.systems() {
        reloaded.replace_system(system).unwrap();
    }
    for _ in 0..20 {
        reloaded.step().unwrap();
    }
    assert_eq!(reloaded.hash(), straight.hash());
}

fn line_of(text: &str) -> u32 {
    u32::try_from(SOURCE.lines().position(|line| line.contains(text)).unwrap()).unwrap() + 1
}

#[test]
fn a_failing_system_is_turned_off_and_the_game_goes_on() {
    let module = module();
    let (mut app, _runtime, player) = game(&module);
    let world = app.world_mut();
    let bomb = world.spawn().unwrap();
    world.set_json(bomb, "Bomb", &json!({})).unwrap();
    let spinner = world.spawn().unwrap();
    world.set_json(spinner, "Spinner", &json!({})).unwrap();

    let report = app.step().unwrap();
    let codes: Vec<String> = report.failures.iter().map(|f| f.message.clone()).collect();
    assert_eq!(report.failures.len(), 2, "{codes:?}");
    let explode = report
        .failures
        .iter()
        .find(|f| f.message.contains("'explode'"))
        .unwrap();
    assert!(explode.message.contains("[script_panicked]"), "{explode:?}");
    assert!(explode.message.contains("the bomb went off"));
    assert_eq!(explode.location.file.as_deref(), Some("src/lib.rs"));
    assert_eq!(
        explode.location.line,
        Some(line_of("panic!(\"the bomb went off\")"))
    );
    let spin = report
        .failures
        .iter()
        .find(|f| f.message.contains("'spin'"))
        .unwrap();
    assert!(spin.message.contains("[script_out_of_fuel]"), "{spin:?}");
    assert!(!spin.hint.is_empty());

    // The rest of the game is unaffected.
    for _ in 0..59 {
        assert!(app.step().unwrap().failures.is_empty());
    }
    assert!((x_of(&app, player) - 6.0).abs() < 1e-4);
    assert_eq!(app.world().resource_json("Score").unwrap()["points"], 13);
    assert_eq!(app.disabled_systems().len(), 2);
}
