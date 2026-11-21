//! The recipes page (`wish://docs/recipes`, M7 WP4) stays true: every
//! recipe's game code builds without warnings in a fresh project, its scene
//! operations apply, its scenario test examples are well formed, and it
//! does what it says: a scenario test plays each one (M7 closing review:
//! building and applying alone did not show that a coin is picked up).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_script::build::{SCRIPTS_ENTRY, Toolchain, pack_sdk};

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-recipes");
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

/// What proves a recipe works: extra scene operations (what the rest of a
/// game would add) and the steps of a scenario test. By the recipe's title.
fn behaviour(title: &str) -> Option<(Value, Value)> {
    let click = |x: u32, y: u32| {
        json!([
            { "input": { "pointer": [x, y], "click": true }, "frames": 1 },
            { "input": { "pointer": [x, y] }, "frames": 2 }
        ])
    };
    let (ops, steps) = if title.starts_with("A platformer character") {
        (
            json!([]),
            json!([
                { "frames": 20 },
                { "expect": "CharacterState.grounded == true", "entity": "player", "message": "it stands on the floor" },
                { "input": { "move": [1, 0] }, "frames": 60 },
                { "expect": "Transform.translation.x > 4", "entity": "player", "message": "it runs 5 m/s along +X" },
                { "input": { "jump": true }, "frames": 1 },
                { "frames": 12 },
                { "expect": "Transform.translation.y > 1.6", "entity": "player", "message": "it jumps" },
                { "frames": 60 },
                { "expect": "CharacterState.grounded == true", "entity": "player", "message": "it lands" }
            ]),
        )
    } else if title.starts_with("A camera that follows") {
        (
            json!([]),
            json!([
                { "frames": 180 },
                { "expect": "Transform.translation.y > 3.9 && Transform.translation.z > 7.9 && Transform.translation.z < 8.1",
                  "entity": "camera", "message": "the camera settles behind and above the player" }
            ]),
        )
    } else if title.starts_with("Things to pick up") {
        (
            json!([
                { "op": "spawn", "id": "player", "components": {
                    "Transform": { "translation": [0, 0.6, 0] }, "Player": {},
                    "RigidBody": { "kind": "Dynamic", "gravity_scale": 0, "linear_damping": 0 },
                    "Collider": { "shape": "Sphere" }, "BodyVelocity": { "linear": [2, 0, 0] } } }
            ]),
            json!([
                { "until": "Score.points >= 1", "max_frames": 120 },
                { "frames": 2 },
                { "expect": "Score.points == 1 && count(Coin) == 0", "message": "the coin is picked up once" },
                { "expect": "UiText.text == \"Score: 1\"", "entity": "score_label", "message": "the label says so" },
                { "expect": "count(ParticleEmitter) == 1", "message": "sparks start" }
            ]),
        )
    } else if title.starts_with("Falling off and checkpoints") {
        (
            json!([
                { "op": "despawn", "id": "ground" },
                { "op": "spawn", "id": "player", "components": {
                    "Transform": { "translation": [6, 3, 0], "scale": [0.6, 1.8, 0.6] },
                    "RigidBody": { "kind": "Kinematic" }, "Collider": { "shape": "Capsule" },
                    "CharacterController": {}, "CharacterMotion": { "velocity": [0, -5, 0] }, "CharacterState": {},
                    "Player": {} } }
            ]),
            json!([
                { "frames": 200 },
                { "expect": "Player.respawn.x == 6 && Transform.translation.y > 0.99 && Transform.translation.y < 1.01",
                  "entity": "player", "message": "it fell through the checkpoint, off the world, and came back there" }
            ]),
        )
    } else if title.starts_with("A moving platform") {
        (
            json!([]),
            json!([
                { "frames": 90 },
                { "expect": "Transform.translation.z < -6.9 && Transform.translation.z > -7.1", "entity": "lift",
                  "message": "half way at 1.5 s" },
                { "frames": 90 },
                { "expect": "Transform.translation.z < -9.9", "entity": "lift", "message": "at the far end at 3 s" }
            ]),
        )
    } else if title.starts_with("A win screen") {
        let mut steps = vec![json!({ "frames": 2 })];
        steps.extend(click(640, 360).as_array().unwrap().iter().cloned());
        steps.push(json!({ "expect": "GameState.started == true", "message": "the button starts the game" }));
        steps.push(json!({ "expect": "UiNode.visible == false", "entity": "win_panel", "message": "not won yet" }));
        (json!([]), Value::Array(steps))
    } else if title.starts_with("A 2D game") {
        (
            json!([]),
            json!([
                { "frames": 70 },
                { "until": "has(Sprite) && has(RigidBody) && Transform.translation.y > -3.2", "max_frames": 120 },
                { "expect": "Transform.translation.z == 0", "entity": "ball", "message": "it bounced and stays in the plane" }
            ]),
        )
    } else if title.starts_with("A top-down character") {
        (
            json!([]),
            json!([
                { "input": { "move": [1, 0] }, "frames": 120 },
                { "expect": "Transform.translation.x > 4.6 && Transform.translation.x < 4.71", "entity": "player", "message": "the east wall stops it" },
                { "expect": "CharacterState.blocked == true && CharacterState.grounded == false", "entity": "player", "message": "it met a wall, and there is no ground seen from above" },
                { "expect": "Transform.translation.y > -0.001 && Transform.translation.y < 0.001", "entity": "player", "message": "nothing falls seen from above" },
                { "input": { "move": [0, 1] }, "frames": 120 },
                { "expect": "Transform.translation.y > 3.6 && Transform.translation.y < 3.71 && Transform.translation.x > 4.6", "entity": "player", "message": "it walks up along the wall into the corner" },
                { "expect": "Transform.translation.z > 0.09", "entity": "player", "message": "it keeps its own depth" }
            ]),
        )
    } else {
        return None;
    };
    Some((
        ops,
        json!({ "format": 1, "scene": "scenes/main.scene.json", "seed": 1, "steps": steps }),
    ))
}

/// A fenced block: its info string and its text.
struct Block {
    info: String,
    text: String,
}

/// The recipes, each a title and its fenced blocks.
fn recipes() -> Vec<(String, Vec<Block>)> {
    let page = wish_control::docs::recipes_page();
    let mut found = Vec::new();
    for section in page.split("\n## ").skip(1) {
        let title = section.lines().next().unwrap_or_default().to_owned();
        let mut blocks = Vec::new();
        let mut lines = section.lines();
        while let Some(line) = lines.next() {
            if let Some(info) = line.strip_prefix("```") {
                let mut text = String::new();
                for inner in lines.by_ref() {
                    if inner.starts_with("```") {
                        break;
                    }
                    text.push_str(inner);
                    text.push('\n');
                }
                blocks.push(Block {
                    info: info.trim().to_owned(),
                    text,
                });
            }
        }
        found.push((title, blocks));
    }
    found
}

#[test]
fn every_recipe_builds_and_applies() {
    let all = recipes();
    assert!(all.len() >= 9, "{} recipes", all.len());
    let mut built = 0;
    let mut applied = 0;
    let mut tests = 0;
    let mut played = 0;
    let mut called = 0;
    let mut drawn = 0;
    for (index, (title, blocks)) in all.iter().enumerate() {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("recipe-{index}"));
        let _ = std::fs::remove_dir_all(&dir);
        create_project(&dir, "Recipe", "wen").unwrap();
        let mut ws = Workspace::open(WorkspaceOptions {
            root: dir.clone(),
            example: None,
        })
        .unwrap();
        ws.set_toolchain(toolchain().clone());
        let agent = parse_actor("agent");
        for block in blocks {
            match block.info.as_str() {
                "rust" => {
                    std::fs::write(dir.join(SCRIPTS_ENTRY), &block.text).unwrap();
                    let answer = ws.call("script_build", &json!({}), &agent);
                    assert_eq!(answer["passed"], true, "'{title}' does not build: {answer}");
                    assert_eq!(
                        answer["warnings"], 0,
                        "'{title}' builds with warnings: {answer}"
                    );
                    built += 1;
                }
                "json ops" => {
                    let ops: Value = serde_json::from_str(&block.text)
                        .unwrap_or_else(|e| panic!("'{title}': the operations are not JSON: {e}"));
                    let answer = ws.call("edit_apply", &json!({ "ops": ops }), &agent);
                    assert_eq!(answer["ok"], true, "'{title}': {answer}");
                    applied += 1;
                }
                "json calls" => {
                    let calls: Value = serde_json::from_str(&block.text)
                        .unwrap_or_else(|e| panic!("'{title}': the calls are not JSON: {e}"));
                    for call in calls.as_array().expect("a list of calls") {
                        let tool = call["tool"].as_str().expect("a tool name");
                        let answer = ws.call(tool, &call["args"], &agent);
                        assert_eq!(answer["ok"], true, "'{title}': {tool}: {answer}");
                        assert!(
                            answer.get("diagnostics").is_none(),
                            "'{title}': {tool}: {answer}"
                        );
                    }
                    called += 1;
                }
                "json test" => {
                    let test: Value = serde_json::from_str(&block.text)
                        .unwrap_or_else(|e| panic!("'{title}': the test is not JSON: {e}"));
                    assert_eq!(test["format"], 1, "'{title}'");
                    assert!(test["steps"].as_array().is_some_and(|s| !s.is_empty()));
                    tests += 1;
                }
                other => panic!("'{title}' has a block of unknown kind '{other}'"),
            }
        }
        // After the code and the operations the project checks clean and a
        // session runs.
        let checked = ws.call("check", &json!({}), &agent);
        assert_eq!(checked["passed"], true, "'{title}': {checked}");
        let started = ws.call("sim_start", &json!({}), &agent);
        assert_eq!(started["ok"], true, "'{title}': {started}");
        let stepped = ws.call("sim_step", &json!({ "frames": 30 }), &agent);
        assert!(
            stepped.get("diagnostics").is_none()
                || stepped["diagnostics"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|d| d["severity"] != "error"),
            "'{title}': {stepped}"
        );
        // A recipe about looks is drawn: nothing it names is missing, and
        // its pictures are on the surfaces.
        if title.starts_with("A texture on a box") {
            ws.set_software(true);
            assert_eq!(checked["warnings"], 0, "'{title}': {checked}");
            let shot = ws.call(
                "capture_frame",
                &json!({ "width": 160, "height": 90 }),
                &agent,
            );
            assert_eq!(shot["ok"], true, "'{title}': {shot}");
            assert!(shot.get("diagnostics").is_none(), "'{title}': {shot}");
            let picture = wish_render::RgbaImage::from_png(
                &wish_control::render::base64_decode(shot["image"]["data"].as_str().unwrap())
                    .unwrap(),
            )
            .unwrap();
            // The tiles' two colours are both on the ground (a flat colour
            // would be one), and the crate's picture is on the crate.
            let mut colours = std::collections::BTreeSet::new();
            for y in (60..90).step_by(2) {
                for x in (0..160).step_by(2) {
                    let [r, g, b, _] = picture.pixel(x, y).unwrap();
                    colours.insert((r / 64, g / 64, b / 64));
                }
            }
            assert!(colours.len() >= 2, "'{title}': the ground is one colour");
            let info = ws.call(
                "asset_info",
                &json!({ "path": "assets/textures/tile.png" }),
                &agent,
            );
            assert_eq!(
                info["used_by"],
                json!([
                    "assets/materials/crate.mat.json",
                    "assets/materials/tiles.mat.json"
                ]),
                "{info}"
            );
            drawn += 1;
        }
        // The see-through ones are drawn see-through, and said to be.
        if title.starts_with("A glass window") {
            ws.set_software(true);
            assert_eq!(checked["warnings"], 0, "'{title}': {checked}");
            let shot = ws.call(
                "capture_frame",
                &json!({ "width": 160, "height": 90 }),
                &agent,
            );
            assert_eq!(shot["ok"], true, "'{title}': {shot}");
            assert!(shot.get("diagnostics").is_none(), "'{title}': {shot}");
            assert_eq!(shot["stats"]["blended_surfaces"], 2, "'{title}': {shot}");
            let seen = ws.call(
                "describe_view",
                &json!({ "width": 160, "height": 90 }),
                &agent,
            );
            let window = seen["visible"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["entity"] == "window")
                .unwrap_or_else(|| panic!("'{title}': the window is not visible: {seen}"));
            assert_eq!(window["blended"], true, "'{title}': {seen}");
            drawn += 1;
        }
        // And it does what it says.
        if let Some((ops, test)) = behaviour(title) {
            if ops.as_array().is_some_and(|o| !o.is_empty()) {
                let answer = ws.call("edit_apply", &json!({ "ops": ops }), &agent);
                assert_eq!(answer["ok"], true, "'{title}': {answer}");
            }
            std::fs::write(
                dir.join("tests/recipe.test.json"),
                serde_json::to_string_pretty(&test).unwrap(),
            )
            .unwrap();
            let ran = ws.call("test_run", &json!({}), &agent);
            assert_eq!(ran["passed"], true, "'{title}' does not work: {ran}");
            played += 1;
        }
    }
    assert!(
        built >= 6 && applied >= 9 && tests >= 1 && played >= 7 && called >= 2 && drawn == 2,
        "{built} {applied} {tests} {played} {called} {drawn}"
    );
}

#[test]
fn the_recipes_are_found_by_search_and_named_in_the_guide() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("recipes-search");
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Search", "wen").unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    let found = ws.call(
        "docs_search",
        &json!({ "query": "moving platform" }),
        &parse_actor("agent"),
    );
    assert_eq!(found["results"][0]["uri"], "wish://docs/recipes", "{found}");
    assert!(wish_control::docs::guide().contains("wish://docs/recipes"));
    let claude = std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
    assert!(claude.contains("wish://docs/recipes"));
    // A name from another engine leads to the recipes.
    let unknown = ws.call(
        "schema_get",
        &json!({ "names": ["FollowCamera"] }),
        &parse_actor("agent"),
    );
    assert!(
        unknown.to_string().contains("wish://docs/recipes"),
        "{unknown}"
    );
}
