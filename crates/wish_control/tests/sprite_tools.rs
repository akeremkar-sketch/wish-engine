//! Sprite sheets and sprite animation through the tools (release 1.1,
//! package 3, design note `docs/design/1.1-sprites.md`): a sheet made,
//! listed, reported, checked and moved; an animator played in a session,
//! seen by `entity_get` and `describe_view`, held by snapshots and
//! replays; game code switching clips in a scenario test.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_script::build::{SCRIPTS_ENTRY, Toolchain, pack_sdk};

const SHEET: &str = "assets/sprites/hero.sheet.json";

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], true, "{tool}: {answer}");
    answer
}

/// A new project with a 128 × 64 picture cut into 32 × 32 frames: idle on
/// the top row, walk below.
fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("sprite-tools-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Sprites", "wen").unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/sprites/hero.png",
                "picture": { "width": 128, "height": 64, "shape": "diamond", "color": [1, 1, 1, 1] } }),
    );
    call(
        &mut ws,
        "asset_create",
        json!({ "path": SHEET, "sheet": {
            "image": "assets/sprites/hero.png",
            "grid": { "frame": [32, 32], "columns": 4, "rows": 2 },
            "clips": {
                "idle": { "frames": [0, 1], "fps": 2 },
                "walk": { "frames": [4, 5, 6, 7], "fps": 8 },
                "fall": { "frames": [2, 3], "fps": 4, "loop": false }
            } } }),
    );
    (dir, ws)
}

/// A walker in a 2D view.
fn spawn_walker(ws: &mut Workspace) {
    call(
        ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "set", "id": "camera", "component": "Camera",
              "value": { "projection": "Orthographic", "ortho_height": 6 } },
            { "op": "replace", "id": "camera", "component": "Transform",
              "value": { "translation": [0, 0, 10] } },
            { "op": "spawn", "id": "hero", "components": {
                "Transform": {},
                "Sprite": { "pixels_per_unit": 32, "smooth": false, "color": [0.4, 0.8, 1, 1] },
                "SpriteAnimator": { "sheet": SHEET, "clip": "walk" } } }
        ] }),
    );
}

fn codes(answer: &Value) -> Vec<String> {
    answer["diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| d["code"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn animator(ws: &mut Workspace, session: &str) -> Value {
    call(
        ws,
        "entity_get",
        json!({ "session": session, "ids": ["hero"] }),
    )["entities"][0]["components"]["SpriteAnimator"]
        .clone()
}

#[test]
fn a_sheet_is_made_listed_reported_and_checked() {
    let (dir, mut ws) = project("made");
    // Written canonical, defaults left out as written.
    let text = std::fs::read_to_string(dir.join(SHEET)).unwrap();
    assert!(text.starts_with("{\n  \"clips\": {"), "{text}");
    assert!(text.contains("\"format\": 1"), "{text}");
    // A dry run checks and says, and writes nothing.
    let dry = call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/sprites/other.sheet.json", "dry_run": true,
                "sheet": { "image": "assets/sprites/hero.png", "frames": [[0, 0, 16, 16]] } }),
    );
    assert_eq!(
        (dry["frames"].clone(), dry["clips"].clone()),
        (json!(1), json!([]))
    );
    assert!(!dir.join("assets/sprites/other.sheet.json").exists());
    // A mistake is refused with where it is.
    let wrong = ws.call(
        "asset_create",
        &json!({ "path": "assets/sprites/bad.sheet.json",
                 "sheet": { "image": "assets/sprites/hero.png", "grid": { "frame": [32, 32], "columns": 2, "rows": 1 },
                            "clips": { "walk": { "frames": [0, 5] } } } }),
        &parse_actor("agent"),
    );
    assert_eq!(wrong["ok"], false);
    assert_eq!(wrong["error"]["code"], "value_out_of_range", "{wrong}");
    assert_eq!(
        wrong["error"]["location"]["path"],
        "/sheet/clips/walk/frames/1"
    );
    assert!(!dir.join("assets/sprites/bad.sheet.json").exists());
    // Listed as a sheet; reported with its frames and clips.
    let listed = call(&mut ws, "asset_list", json!({ "kind": "sheet" }));
    assert_eq!(listed["assets"][0]["path"], SHEET, "{listed}");
    let info = call(&mut ws, "asset_info", json!({ "path": SHEET }));
    assert_eq!(info["kind"], "sheet");
    assert_eq!(info["sheet"]["frame_count"], 8, "{info}");
    assert_eq!(info["sheet"]["frames"][5], json!([32, 32, 32, 32]));
    assert_eq!(
        info["sheet"]["clips"]["fall"],
        json!({ "frames": [2, 3], "fps": 4.0, "loop": false })
    );
    assert_eq!(info["sheet"]["image_exists"], true);
    let picture = call(
        &mut ws,
        "asset_info",
        json!({ "path": "assets/sprites/hero.png" }),
    );
    assert_eq!(picture["used_by"], json!([SHEET]), "{picture}");
    // A project with it checks clean.
    let checked = call(&mut ws, "check", json!({}));
    assert_eq!(checked["passed"], true, "{checked}");
}

#[test]
fn check_finds_what_is_wrong_with_a_sheet_and_with_what_uses_it() {
    let (dir, mut ws) = project("broken");
    spawn_walker(&mut ws);
    // A frame number the sheet does not have, with the line it is on.
    let broken = std::fs::read_to_string(dir.join(SHEET))
        .unwrap()
        .replace("[4, 5, 6, 7]", "[4, 5, 6, 9]");
    std::fs::write(dir.join(SHEET), &broken).unwrap();
    let checked = call(&mut ws, "check", json!({}));
    assert_eq!(checked["passed"], false, "{checked}");
    let problem = checked["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["location"]["file"] == SHEET)
        .unwrap()
        .clone();
    assert_eq!(problem["code"], "value_out_of_range", "{problem}");
    let line = broken
        .lines()
        .position(|l| l.contains("[4, 5, 6, 9]"))
        .unwrap()
        + 1;
    assert_eq!(problem["location"]["line"], line, "{problem}");
    assert!(!problem["hint"].as_str().unwrap().is_empty());
    // Its picture missing; frames reaching past the picture.
    let fixed = broken.replace("[4, 5, 6, 9]", "[4, 5, 6, 7]");
    std::fs::write(
        dir.join(SHEET),
        fixed.replace("assets/sprites/hero.png", "assets/sprites/gone.png"),
    )
    .unwrap();
    assert!(
        codes(&ws.call("check", &json!({}), &parse_actor("agent")))
            .contains(&"asset_not_found".to_owned())
    );
    std::fs::write(dir.join(SHEET), fixed.replace("\"rows\": 2", "\"rows\": 3")).unwrap();
    let checked = call(&mut ws, "check", json!({}));
    assert!(
        codes(&checked).contains(&"sheet_frames_outside_picture".to_owned()),
        "{checked}"
    );
    std::fs::write(dir.join(SHEET), &fixed).unwrap();
    // A clip the sheet lacks, and a picture named as a sheet, said where the
    // scene writes them.
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "set", "id": "hero", "component": "SpriteAnimator", "value": { "clip": "run" } },
            { "op": "spawn", "id": "wrong", "components": {
                "Sprite": {}, "SpriteAnimator": { "sheet": "assets/sprites/hero.png" } } }
        ] }),
    );
    let checked = call(&mut ws, "check", json!({}));
    let found = codes(&checked);
    assert!(
        found.contains(&"sprite_clip_not_found".to_owned()),
        "{checked}"
    );
    assert!(found.contains(&"not_a_sheet_file".to_owned()), "{checked}");
    let clip = checked["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "sprite_clip_not_found")
        .unwrap();
    assert_eq!(
        clip["location"]["path"],
        "/entities/hero/components/SpriteAnimator/clip"
    );
    assert!(clip["hint"].as_str().unwrap().contains("walk"), "{clip}");
    // Not canonical: a warning, and `fmt`'s canonical text is the engine's.
    std::fs::write(dir.join(SHEET), fixed.replace("\n", "\r\n")).unwrap();
    let checked = call(&mut ws, "check", json!({}));
    assert!(
        codes(&checked).contains(&"not_canonical".to_owned()),
        "{checked}"
    );
    // Far too large: not read at all.
    std::fs::write(dir.join(SHEET), " ".repeat(300 * 1024)).unwrap();
    let checked = call(&mut ws, "check", json!({}));
    assert!(
        codes(&checked).contains(&"sheet_file_too_large".to_owned()),
        "{checked}"
    );
}

#[test]
fn moving_the_picture_or_the_sheet_rewrites_what_names_it() {
    let (dir, mut ws) = project("move");
    spawn_walker(&mut ws);
    let moved = call(
        &mut ws,
        "asset_move",
        json!({ "from": "assets/sprites/hero.png", "to": "art/hero.png" }),
    );
    assert_eq!(moved["documents"][0]["file"], SHEET, "{moved}");
    let sheet: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(SHEET)).unwrap()).unwrap();
    assert_eq!(sheet["image"], "art/hero.png");
    call(
        &mut ws,
        "asset_move",
        json!({ "from": SHEET, "to": "art/hero.sheet.json" }),
    );
    let hero = call(
        &mut ws,
        "entity_get",
        json!({ "ids": ["hero"], "components": ["SpriteAnimator"] }),
    );
    assert_eq!(
        hero["entities"][0]["components"]["SpriteAnimator"]["sheet"],
        "art/hero.sheet.json"
    );
    // A sheet keeps its ending.
    let refused = ws.call(
        "asset_move",
        &json!({ "from": "art/hero.sheet.json", "to": "art/hero.json" }),
        &parse_actor("agent"),
    );
    assert_eq!(refused["error"]["code"], "kind_change", "{refused}");
    assert_eq!(call(&mut ws, "check", json!({}))["passed"], true);
}

#[test]
fn an_animator_plays_in_a_session_and_is_seen() {
    let (_dir, mut ws) = project("plays");
    spawn_walker(&mut ws);
    // The scene, no session: the clip's first frame.
    let scene = call(
        &mut ws,
        "describe_view",
        json!({ "width": 320, "height": 180 }),
    );
    let hero = &scene["sprites"][0];
    assert_eq!(
        (
            hero["clip"].clone(),
            hero["frame"].clone(),
            hero["sheet_frame"].clone()
        ),
        (json!("walk"), json!(0), json!(4)),
        "{scene}"
    );
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    // 8 frames a second: after 20 ticks the third (20/60 * 8 = 2.7).
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 20 }));
    let playing = animator(&mut ws, &s);
    assert_eq!(playing["frame"], 2, "{playing}");
    let seen = call(
        &mut ws,
        "describe_view",
        json!({ "session": s, "width": 320, "height": 180 }),
    );
    assert_eq!(seen["sprites"][0]["sheet_frame"], 6, "{seen}");
    let picture = call(
        &mut ws,
        "capture_frame",
        json!({ "session": s, "width": 160, "height": 90 }),
    );
    assert!(picture.get("diagnostics").is_none(), "{picture}");
    // A snapshot holds where it is.
    call(
        &mut ws,
        "sim_snapshot",
        json!({ "session": s, "label": "here" }),
    );
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 7 }));
    assert_ne!(animator(&mut ws, &s), playing);
    call(
        &mut ws,
        "sim_restore",
        json!({ "session": s, "snapshot": "here" }),
    );
    assert_eq!(animator(&mut ws, &s), playing);
    // A clip that does not loop stops on its last frame and says so.
    call(
        &mut ws,
        "edit_apply",
        json!({ "session": s, "ops": [{ "op": "set", "id": "hero", "component": "SpriteAnimator",
                "value": { "clip": "fall", "time": 0 } }] }),
    );
    let stepped = call(&mut ws, "sim_step", json!({ "session": s, "frames": 40 }));
    assert_eq!(stepped["events"]["AnimationFinished"], 1, "{stepped}");
    let fallen = animator(&mut ws, &s);
    assert_eq!(
        (fallen["frame"].clone(), fallen["playing"].clone()),
        (json!(1), json!(false))
    );
}

#[test]
fn a_replay_holds_the_animation_and_knows_when_the_sheet_changed() {
    let (dir, mut ws) = project("replay");
    spawn_walker(&mut ws);
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        &mut ws,
        "replay",
        json!({ "action": "record", "session": s }),
    );
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 90 }));
    call(
        &mut ws,
        "replay",
        json!({ "action": "save", "session": s, "name": "walk" }),
    );
    let same = call(
        &mut ws,
        "replay",
        json!({ "action": "run", "name": "walk" }),
    );
    assert_eq!(same["passed"], true, "{same}");
    assert!(
        !same.to_string().contains("replay_assets_changed"),
        "{same}"
    );
    // The walk is faster now: which frame shows, so the world, differs.
    let text = std::fs::read_to_string(dir.join(SHEET))
        .unwrap()
        .replace("\"fps\": 8", "\"fps\": 12");
    std::fs::write(dir.join(SHEET), text).unwrap();
    let changed = ws.call(
        "replay",
        &json!({ "action": "run", "name": "walk" }),
        &parse_actor("agent"),
    );
    assert!(
        changed.to_string().contains("replay_assets_changed"),
        "{changed}"
    );
    assert_eq!(changed["passed"], false, "{changed}");
}

#[test]
fn a_checkpoint_keeps_and_brings_back_a_sheet() {
    let (dir, mut ws) = project("checkpoint");
    call(
        &mut ws,
        "checkpoint",
        json!({ "action": "save", "name": "before" }),
    );
    let before = std::fs::read_to_string(dir.join(SHEET)).unwrap();
    std::fs::write(dir.join(SHEET), before.replace("\"fps\": 8", "\"fps\": 12")).unwrap();
    call(
        &mut ws,
        "checkpoint",
        json!({ "action": "restore", "name": "before" }),
    );
    assert_eq!(std::fs::read_to_string(dir.join(SHEET)).unwrap(), before);
}

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-sprites");
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

/// Walks with `move`, and shows it: walk while moving, idle while not,
/// mirrored when going left.
const WALKER: &str = r#"
use wish_sdk::prelude::*;

/// The player.
#[component]
pub struct Hero {
    /// How fast it walks.
    #[schema(unit = "m/s", default = 3.0)]
    pub speed: f32,
}

#[system]
fn walk(
    actions: Actions,
    time: Res<Time>,
    mut heroes: Query<(&Hero, &mut Transform, &mut Sprite, &mut SpriteAnimator)>,
) {
    let go = actions.axis2("move");
    for (hero, transform, sprite, animator) in heroes.iter_mut() {
        transform.translation += Vec3::new(go.x, go.y, 0.0) * hero.speed * time.delta;
        if go.x != 0.0 {
            sprite.flip_x = go.x < 0.0;
        }
        let moving = go.x != 0.0 || go.y != 0.0;
        animator.play(text(if moving { "walk" } else { "idle" }));
    }
}

wish_sdk::export! {
    components: [Hero],
    systems: [walk],
}
"#;

fn write_test(dir: &Path, name: &str, steps: Value) {
    std::fs::write(
        dir.join(format!("tests/{name}.test.json")),
        serde_json::to_string_pretty(&json!({
            "format": 1, "scene": "scenes/main.scene.json", "seed": 1, "steps": steps
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn game_code_switches_clips_in_a_scenario_test() {
    let (dir, mut ws) = project("game");
    ws.set_toolchain(toolchain().clone());
    std::fs::write(dir.join(SCRIPTS_ENTRY), WALKER).unwrap();
    let built = call(&mut ws, "script_build", json!({}));
    assert_eq!(built["passed"], true, "{built}");
    spawn_walker(&mut ws);
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "replace", "id": "hero", "component": "Hero", "value": {} }] }),
    );
    // Standing: idle. Walking right: walk, from its first frame. Turning:
    // the same clip runs on, mirrored. Stopping: idle from its start.
    write_test(
        &dir,
        "walk",
        json!([
            { "frames": 10 },
            { "expect": "SpriteAnimator.clip == \"idle\" && SpriteAnimator.frame == 0", "entity": "hero", "message": "standing still plays idle" },
            { "input": { "move": [1, 0] }, "frames": 20 },
            { "expect": "SpriteAnimator.clip == \"walk\" && SpriteAnimator.frame == 2", "entity": "hero", "message": "walking plays walk from its start" },
            { "input": { "move": [-1, 0] }, "frames": 4 },
            { "expect": "SpriteAnimator.clip == \"walk\" && SpriteAnimator.frame == 3 && Sprite.flip_x == true", "entity": "hero", "message": "turning keeps the cycle, mirrored" },
            { "frames": 3 },
            { "expect": "SpriteAnimator.clip == \"idle\" && SpriteAnimator.frame == 0", "entity": "hero", "message": "stopping goes back to idle" }
        ]),
    );
    let ran = call(&mut ws, "test_run", json!({}));
    assert_eq!(ran["passed"], true, "{ran}");
}
