//! The embedded toolchain (ADR-023, ADR-025): a packed copy of the compiler,
//! with nothing else on the machine helping, builds game code that runs.
//!
//! The pack is about 450 MB, so the test is not part of the everyday run:
//! `cargo test -p wish_script --test toolchain -- --ignored --nocapture`
//! (CI runs it on every push).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::json;
use wish_app::{App, AppConfig};
use wish_script::build::{
    GAME_WASM, SCRIPTS_ENTRY, Toolchain, build_game, pack_sdk, pack_toolchain,
};
use wish_script::{GameModule, Limits, ScriptEngine, ScriptRuntime};

const GAME: &str = include_str!("../guests/collect/src/lib.rs");

#[test]
#[ignore = "packs ~450 MB; run by hand or in CI"]
fn a_packed_toolchain_builds_game_code_on_its_own() {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
    let sdk = tmp.join("sdk-pack-toolchain");
    // The folder is the test's own (a cache may have left anything in it).
    let _ = std::fs::remove_dir_all(&sdk);
    pack_sdk(&cargo, &manifest, &tmp.join("sdk-target"), &sdk).unwrap();
    let packed = tmp.join("packed-toolchain");
    let description = pack_toolchain(&PathBuf::from("rustc"), &packed).unwrap();
    let megabytes = description["bytes"].as_u64().unwrap() as f64 / (1024.0 * 1024.0);
    println!(
        "packed toolchain: {megabytes:.0} MiB ({})",
        description["rustc"]
    );
    assert!(
        megabytes < 800.0,
        "the pack should stay small enough to ship"
    );

    let exe = if cfg!(windows) { "rustc.exe" } else { "rustc" };
    let toolchain = Toolchain {
        sdk,
        rustc: packed.join("bin").join(exe),
    };
    let root = tmp.join("toolchain-game");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("scripts/src")).unwrap();
    std::fs::write(root.join(SCRIPTS_ENTRY), GAME).unwrap();
    // The build runs with an empty environment and only the pack's bin
    // folder on the PATH (build_game always does): no rustup, no cargo, no
    // Visual Studio, no host linker can take part.
    let report = build_game(&toolchain, &root, true).unwrap();
    assert!(report.passed, "{:?}", report.diagnostics);
    println!("built in {:.2} s with the packed toolchain", report.seconds);

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
    assert_eq!(
        app.world().resource_json("Score").unwrap(),
        json!({ "points": 10, "coins": 1 })
    );
}
