//! Regression tests for the M5 closing review's findings (design note §17,
//! "M5 kapanışı"). Each test is the review's reproduction; the finding's
//! number is in the test name (T = tools review, R = runtime review).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_script::Limits;
use wish_script::build::{
    GAME_WASM, SCRIPTS_ENTRY, Toolchain, build_game_within, clear_output_folder, pack_sdk,
};

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-review-m5");
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

fn open(dir: &Path) -> Workspace {
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .unwrap();
    ws.set_toolchain(toolchain().clone());
    ws.set_software(true);
    ws
}

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("review-m5-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Game", "wen").unwrap();
    let ws = open(&dir);
    (dir, ws)
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    ws.call(tool, &args, &parse_actor("agent"))
}

fn build(ws: &mut Workspace, dir: &Path, code: &str) -> Value {
    std::fs::write(dir.join(SCRIPTS_ENTRY), code).unwrap();
    call(ws, "script_build", json!({}))
}

fn write_test(dir: &Path, name: &str, text: &str) {
    let path = dir.join("tests").join(format!("{name}.test.json"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn run_test(ws: &mut Workspace, name: &str) -> Value {
    let answer = call(
        ws,
        "test_run",
        json!({ "paths": [format!("tests/{name}.test.json")] }),
    );
    assert_eq!(answer["ok"], true, "{answer}");
    answer["results"][0].clone()
}

const WALKER: &str = r#"
use wish_sdk::prelude::*;

/// Something that walks.
#[component]
pub struct Walker {
    /// How fast it walks.
    #[schema(unit = "m/s", default = 4.0)]
    pub speed: f32,
}

#[system]
fn walk(actions: Actions, mut walkers: Query<(&mut Transform, &Walker)>, time: Res<Time>) {
    let go = actions.axis2("move");
    for (transform, walker) in walkers.iter_mut() {
        transform.translation += Vec3::new(go.x, 0.0, -go.y) * walker.speed * time.delta;
    }
}

wish_sdk::export! {
    components: [Walker],
    systems: [walk],
}
"#;

fn walking_project(name: &str) -> (PathBuf, Workspace) {
    let (dir, mut ws) = project(name);
    assert_eq!(build(&mut ws, &dir, WALKER)["passed"], true);
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "hero", "components": { "Transform": {}, "Walker": {} } }] }),
    );
    (dir, ws)
}

#[test]
fn t1_a_negated_condition_does_not_pass_by_looking_at_the_wrong_entity() {
    let (dir, mut ws) = project("t1");
    let case = |ws: &mut Workspace, expect: &str| {
        write_test(
            &dir,
            "camera",
            &json!({ "format": 1, "scene": "scenes/main.scene.json", "steps": [{ "expect": expect }] })
                .to_string(),
        );
        run_test(ws, "camera")
    };
    // The scene has one camera, with a field of view of 50 degrees.
    let negated = case(&mut ws, "!has(Camera)");
    assert_eq!(negated["passed"], false, "{negated}");
    assert_eq!(negated["failure"]["kind"], "expect_ambiguous");
    assert!(
        negated["failure"]["hint"]
            .as_str()
            .unwrap()
            .contains("count(")
    );
    let not_wide = case(&mut ws, "!(Camera.fov_y_deg > 10)");
    assert_eq!(not_wide["passed"], false, "{not_wide}");
    assert_eq!(case(&mut ws, "count(Camera) == 1")["passed"], true);
    assert_eq!(case(&mut ws, "count(Camera) == 0")["passed"], false);
    assert_eq!(case(&mut ws, "Camera.fov_y_deg > 10")["passed"], true);
    // The query language agrees: unknown stays unknown under `!`.
    let query = call(
        &mut ws,
        "entity_query",
        json!({ "where": "!(Camera.fov_y_deg > 10)" }),
    );
    assert_eq!(query["entities"], json!([]), "{query}");
}

#[test]
fn t2_game_code_cannot_read_the_machine_while_it_builds() {
    let (dir, mut ws) = project("t2");
    let before = std::fs::read(dir.join(GAME_WASM)).ok();
    for (code, what) in [
        (
            "use wish_sdk::prelude::*;\nconst HOST: &str = include_str!(\"C:/Windows/win.ini\");\nwish_sdk::export! { systems: [] }\n",
            "include_str",
        ),
        (
            "use wish_sdk::prelude::*;\nconst WHO: &str = env!(\"USERPROFILE\");\nwish_sdk::export! { systems: [] }\n",
            "env",
        ),
        (
            "use wish_sdk::prelude::*;\n#[path = \"C:/Windows/win.ini\"]\nmod secret;\nwish_sdk::export! { systems: [] }\n",
            "#[path]",
        ),
    ] {
        let answer = build(&mut ws, &dir, code);
        assert_eq!(answer["passed"], false, "{answer}");
        let problem = &answer["diagnostics"][0];
        assert_eq!(problem["code"], "script_forbidden_source", "{answer}");
        assert!(problem["message"].as_str().unwrap().contains(what));
        assert_eq!(problem["location"]["file"], "scripts/src/lib.rs");
        assert!(!problem["hint"].as_str().unwrap().is_empty());
    }
    assert_eq!(std::fs::read(dir.join(GAME_WASM)).ok(), before);
}

#[test]
fn t3_a_module_that_compiles_but_cannot_be_used_does_not_replace_the_build() {
    let (dir, mut ws) = walking_project("t3");
    let good = std::fs::read(dir.join(GAME_WASM)).unwrap();
    // A game type that clashes with the engine's Camera.
    let clash = WALKER.replace(
        "wish_sdk::export! {\n    components: [Walker],",
        "/// Not the engine's camera.\n#[component]\npub struct Camera {\n    /// A number.\n    pub x: f32,\n}\n\nwish_sdk::export! {\n    components: [Walker, Camera],",
    );
    let answer = build(&mut ws, &dir, &clash);
    assert_eq!(answer["passed"], false, "{answer}");
    assert!(answer["errors"].as_u64().unwrap() >= 1, "{answer}");
    assert!(answer.get("output").is_none_or(Value::is_null), "{answer}");
    assert_eq!(
        std::fs::read(dir.join(GAME_WASM)).unwrap(),
        good,
        "the previous build stays in place"
    );
    let info = call(&mut ws, "project_info", json!({}));
    assert_eq!(info["scripts"]["last_build"]["passed"], false, "{info}");
    // A new process still has the old game's types.
    let mut again = open(&dir);
    let check = call(&mut again, "check", json!({}));
    assert_eq!(check["passed"], true, "{check}");
    let started = call(&mut again, "sim_start", json!({}));
    assert_eq!(started["ok"], true, "{started}");
}

#[test]
fn t4_a_compiler_that_runs_for_ever_is_stopped() {
    let (dir, _ws) = project("t4");
    std::fs::write(
        dir.join(SCRIPTS_ENTRY),
        "#![allow(long_running_const_eval)]\nuse wish_sdk::prelude::*;\nconst SPIN: u64 = { let mut i = 0u64; while i < u64::MAX { i += 1; } i };\n#[system]\nfn s() { wish_sdk::info!(\"{}\", SPIN); }\nwish_sdk::export! { systems: [s] }\n",
    )
    .unwrap();
    let started = std::time::Instant::now();
    let report = build_game_within(toolchain(), &dir, true, Duration::from_secs(5)).unwrap();
    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(!report.passed);
    assert_eq!(report.diagnostics[0].code, "script_build_timeout");
    assert!(!report.diagnostics[0].hint.is_empty());
}

#[test]
fn t6_a_recording_belongs_to_the_code_and_scene_it_started_with() {
    let (dir, mut ws) = walking_project("t6");
    call(&mut ws, "sim_start", json!({}));
    call(&mut ws, "replay", json!({ "action": "record" }));
    call(
        &mut ws,
        "sim_step",
        json!({ "frames": 30, "input": { "move": [0, 1] } }),
    );
    // New code is built but the session keeps the old one.
    let faster = WALKER.replace("default = 4.0", "default = 6.0");
    std::fs::write(dir.join(SCRIPTS_ENTRY), &faster).unwrap();
    let built = call(&mut ws, "script_build", json!({ "reload": false }));
    assert_eq!(built["passed"], true, "{built}");
    call(&mut ws, "sim_step", json!({ "frames": 30 }));
    let saved = call(
        &mut ws,
        "replay",
        json!({ "action": "save", "name": "old" }),
    );
    assert_eq!(saved["ok"], true, "{saved}");
    let ran = call(&mut ws, "replay", json!({ "action": "run", "name": "old" }));
    let codes: Vec<&str> = ran["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert!(codes.contains(&"replay_code_changed"), "{ran}");

    // New code into a recording session stops the recording.
    let id = call(&mut ws, "sim_start", json!({}))["session"].clone();
    call(
        &mut ws,
        "replay",
        json!({ "action": "record", "session": id }),
    );
    call(&mut ws, "sim_step", json!({ "frames": 5, "session": id }));
    std::fs::write(dir.join(SCRIPTS_ENTRY), WALKER).unwrap();
    let reloaded = call(&mut ws, "script_build", json!({}));
    assert!(
        reloaded.to_string().contains("stops the recording"),
        "{reloaded}"
    );
    let save = call(
        &mut ws,
        "replay",
        json!({ "action": "save", "name": "mixed", "session": id }),
    );
    assert_eq!(save["error"]["code"], "replay_not_recording", "{save}");
}

#[test]
fn t7_a_snapshot_later_than_the_recording_stops_it() {
    let (_dir, mut ws) = walking_project("t7");
    call(&mut ws, "sim_start", json!({}));
    call(&mut ws, "replay", json!({ "action": "record" }));
    call(&mut ws, "sim_step", json!({ "frames": 50 }));
    call(&mut ws, "sim_snapshot", json!({ "label": "a" }));
    call(&mut ws, "sim_step", json!({ "frames": 50 }));
    call(&mut ws, "sim_snapshot", json!({ "label": "b" }));
    call(&mut ws, "sim_restore", json!({ "snapshot": "a" }));
    let forward = call(&mut ws, "sim_restore", json!({ "snapshot": "b" }));
    assert!(forward["recording_stopped"].is_string(), "{forward}");
    let save = call(&mut ws, "replay", json!({ "action": "save", "name": "x" }));
    assert_eq!(save["error"]["code"], "replay_not_recording");
}

#[test]
fn t8_an_output_folder_is_emptied_only_if_wen_made_it() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("review-m5-t8");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    std::fs::write(dir.join("work/precious.txt"), "keep me").unwrap();
    let refused = clear_output_folder(&dir.join("work"), "sdk.json").unwrap_err();
    assert_eq!(refused.code, "output_exists");
    assert!(dir.join("work/precious.txt").is_file());
    std::fs::create_dir_all(dir.join("sdk")).unwrap();
    std::fs::write(dir.join("sdk/sdk.json"), "{}").unwrap();
    clear_output_folder(&dir.join("sdk"), "sdk.json").unwrap();
    assert!(!dir.join("sdk").exists());
    clear_output_folder(&dir.join("missing"), "sdk.json").unwrap();
}

#[test]
fn t9_r9_a_game_system_cannot_take_an_engine_systems_name() {
    let (dir, mut ws) = walking_project("t9");
    let answer = build(
        &mut ws,
        &dir,
        &WALKER
            .replace("fn walk(", "fn transform_propagation(")
            .replace("systems: [walk]", "systems: [transform_propagation]"),
    );
    assert_eq!(answer["passed"], false, "{answer}");
    let codes = answer["diagnostics"].to_string();
    assert!(codes.contains("script_system_name_taken"), "{answer}");
}

#[test]
fn t10_paths_from_files_stay_inside_the_project() {
    let (dir, mut ws) = walking_project("t10");
    std::fs::create_dir_all(dir.join("replays")).unwrap();
    std::fs::write(
        dir.join("replays/away.replay.jsonl"),
        "{\"format\":1,\"scene\":\"../other/x.scene.json\",\"scene_hash\":\"0\",\"seed\":1,\"tick_rate\":60,\"code\":null,\"ticks\":1}\n{\"tick\":0,\"hash\":\"0\"}\n",
    )
    .unwrap();
    let ran = call(
        &mut ws,
        "replay",
        json!({ "action": "run", "name": "away" }),
    );
    assert_eq!(ran["ok"], false, "{ran}");
    assert!(
        ["invalid_scene_path", "invalid_replay_file"]
            .contains(&ran["error"]["code"].as_str().unwrap()),
        "{ran}"
    );
    let outside = call(
        &mut ws,
        "test_run",
        json!({ "paths": ["../review-m5-t3/tests/boots.test.json"] }),
    );
    assert_eq!(outside["error"]["code"], "test_not_found", "{outside}");
}

#[test]
fn t11_tests_leave_the_adapter_choice_alone_and_check_their_input() {
    let (dir, mut ws) = walking_project("t11");
    ws.set_software(false);
    write_test(
        &dir,
        "look",
        r#"{ "format": 1, "scene": "scenes/main.scene.json", "steps": [
            { "capture": "tests/golden/look.png", "size": [32, 32] } ] }"#,
    );
    call(&mut ws, "test_run", json!({ "update_golden": true }));
    assert!(!ws.software(), "the workspace's own choice is back");
    ws.set_software(true);
    write_test(
        &dir,
        "typo",
        r#"{ "format": 1, "scene": "scenes/main.scene.json", "steps": [
            { "input": "jump", "frames": 1 } ] }"#,
    );
    let typo = run_test(&mut ws, "typo");
    assert_eq!(typo["passed"], false, "{typo}");
    assert!(typo.to_string().contains("invalid_parameter"), "{typo}");
}

/// A walker with a counter, for snapshots across new code.
const COUNTER: &str = r#"
use wish_sdk::prelude::*;

/// Counts ticks.
#[component]
pub struct Counter {
    /// How many ticks so far.
    pub n: u32,
}

#[system]
fn count(mut counters: Query<&mut Counter>) {
    for counter in counters.iter_mut() {
        counter.n += 1;
    }
}

wish_sdk::export! {
    components: [Counter],
    systems: [count],
}
"#;

#[test]
fn r6_snapshots_survive_new_code_that_changes_a_type() {
    let (dir, mut ws) = project("r6");
    assert_eq!(build(&mut ws, &dir, COUNTER)["passed"], true);
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "c", "components": { "Counter": {} } }] }),
    );
    call(&mut ws, "sim_start", json!({}));
    call(&mut ws, "sim_step", json!({ "frames": 10 }));
    call(&mut ws, "sim_snapshot", json!({ "label": "ten" }));
    call(&mut ws, "sim_step", json!({ "frames": 10 }));
    // The counter gains a field; a new system appears too.
    let changed = COUNTER
        .replace(
            "    pub n: u32,\n}",
            "    pub n: u32,\n    /// A new field.\n    pub extra: u32,\n}",
        )
        .replace("systems: [count]", "systems: [count, idle]")
        .replace(
            "wish_sdk::export!",
            "#[system]\nfn idle() {}\n\nwish_sdk::export!",
        );
    let built = build(&mut ws, &dir, &changed);
    assert_eq!(built["passed"], true, "{built}");
    let restored = call(&mut ws, "sim_restore", json!({ "snapshot": "ten" }));
    assert_eq!(restored["ok"], true, "{restored}");
    let counter = call(
        &mut ws,
        "entity_get",
        json!({ "session": "s1", "ids": ["c"] }),
    );
    assert_eq!(
        counter["entities"][0]["components"]["Counter"],
        json!({ "n": 10, "extra": 0 }),
        "{counter}"
    );
    call(&mut ws, "sim_step", json!({ "frames": 5 }));
    let counter = call(
        &mut ws,
        "entity_get",
        json!({ "session": "s1", "ids": ["c"] }),
    );
    assert_eq!(counter["entities"][0]["components"]["Counter"]["n"], 15);
}

#[test]
fn r1_two_types_with_one_name_are_refused() {
    let (dir, mut ws) = project("r1");
    let answer = build(
        &mut ws,
        &dir,
        r#"
use wish_sdk::prelude::*;

mod player {
    use wish_sdk::prelude::*;
    /// The player's state.
    #[component]
    pub struct State {
        /// Health.
        pub health: f32,
    }
}

mod enemy {
    use wish_sdk::prelude::*;
    /// An enemy's state.
    #[component]
    pub struct State {
        /// Kills.
        pub kills: u32,
    }
}

#[system]
fn look(players: Query<&player::State>, enemies: Query<&enemy::State>) {
    let _ = (players.len(), enemies.len());
}

wish_sdk::export! {
    systems: [look],
}
"#,
    );
    assert_eq!(answer["passed"], false, "{answer}");
    assert!(
        answer["diagnostics"]
            .to_string()
            .contains("script_duplicate_type"),
        "{answer}"
    );
}

#[test]
fn r4_two_parameters_over_the_same_data_do_not_undo_each_other() {
    let (dir, mut ws) = walking_project("r4");
    let both = WALKER.replace(
        "#[system]\nfn walk(",
        "#[system]\nfn nudge(mut a: Query<&mut Transform, With<Walker>>, mut b: Query<&mut Transform>) {\n    for t in a.iter_mut() {\n        t.translation.x += 1.0;\n    }\n    let _ = b.iter_mut().count();\n}\n\n#[system]\nfn walk(",
    )
    .replace("systems: [walk]", "systems: [nudge, walk]");
    assert_eq!(build(&mut ws, &dir, &both)["passed"], true);
    call(&mut ws, "sim_start", json!({}));
    call(&mut ws, "sim_step", json!({ "frames": 5 }));
    let hero = call(
        &mut ws,
        "entity_get",
        json!({ "session": "s1", "ids": ["hero"] }),
    );
    assert_eq!(
        hero["entities"][0]["components"]["Transform"]["translation"][0], 5.0,
        "{hero}"
    );
}

#[test]
fn r5_a_ticks_systems_do_not_add_up_their_frames() {
    let (dir, mut ws) = project("r5");
    ws.set_script_limits(Limits {
        memory_bytes: 8 * 1024 * 1024,
        ..Limits::default()
    })
    .unwrap();
    // One system fills the world, sixteen read all of it every tick: each
    // frame is about a mebibyte, all of them together twice the memory.
    let mut code = String::from(
        "use wish_sdk::prelude::*;\n\n/// A dot.\n#[component]\npub struct Dot {\n    /// Its number.\n    pub n: u32,\n}\n\n#[system]\nfn fill(dots: Query<&Dot>, mut commands: Commands) {\n    if dots.is_empty() {\n        for n in 0..20000u32 {\n            commands.spawn().insert(Dot { n }).insert(Transform::default());\n        }\n    }\n}\n",
    );
    let mut names = vec!["fill".to_owned()];
    for i in 0..16 {
        code.push_str(&format!(
            "\n#[system(after = \"fill\")]\nfn read{i}(dots: Query<(&Transform, &Dot)>) {{\n    let _ = dots.iter().count();\n}}\n"
        ));
        names.push(format!("read{i}"));
    }
    code.push_str(&format!(
        "\nwish_sdk::export! {{\n    components: [Dot],\n    systems: [{}],\n}}\n",
        names.join(", ")
    ));
    let built = build(&mut ws, &dir, &code);
    assert_eq!(built["passed"], true, "{built}");
    call(&mut ws, "sim_start", json!({}));
    let stepped = call(&mut ws, "sim_step", json!({ "frames": 3 }));
    assert_eq!(stepped["ok"], true, "{stepped}");
    let logs = call(&mut ws, "logs_get", json!({ "severity": "error" }));
    assert!(!logs.to_string().contains("script_out_of_memory"), "{logs}");
}
