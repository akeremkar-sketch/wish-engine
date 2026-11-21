//! Line of sight and 2D lights through the tools (release 1.1, package 7,
//! design note `docs/design/1.1-lights-2d.md`): `physics_query sight` in a
//! scene and in a session, game code asking `can_see` and `visible_area` in
//! a scenario test, `describe_view` and `stats.effects`, and `check`'s
//! warnings.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_script::build::{SCRIPTS_ENTRY, Toolchain, pack_sdk};

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("sight-tools-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Sight", "wen").unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    (dir, ws)
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], true, "{tool}: {answer}");
    answer
}

fn fails(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], false, "{tool}: {answer}");
    answer["error"].clone()
}

fn codes(answer: &Value) -> Vec<String> {
    answer["diagnostics"]
        .as_array()
        .map(|all| {
            all.iter()
                .map(|d| d["code"].as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// A wall at x = 4 from y = 10.5 to 14 (an occluding box), and a spy
/// behind it at (6, 12).
fn wall_and_spy(ws: &mut Workspace) {
    call(
        ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "spawn", "id": "wall", "components": {
                "Transform": { "translation": [4.0, 12.25, 0.0] },
                "Collider": { "size": [0.4, 3.5, 1.0] },
                "Occluder": {} } },
            { "op": "spawn", "id": "spy", "components": {
                "Transform": { "translation": [6.0, 12.0, 0.0] } } }
        ] }),
    );
}

#[test]
fn physics_query_answers_line_of_sight_in_a_scene_and_a_session() {
    let (_, mut ws) = project("query");
    wall_and_spy(&mut ws);
    let started = call(&mut ws, "sim_start", json!({ "seed": 1 }));
    let id = started["session"].as_str().unwrap().to_owned();
    for session in [None, Some(id.as_str())] {
        let mut ask = |sight: Value| {
            let mut args = json!({ "sight": sight });
            if let Some(id) = session {
                args["session"] = json!(id);
            }
            call(&mut ws, "physics_query", args)
        };
        let blocked = ask(json!({ "from": [0, 10], "to": [6, 12] }));
        assert_eq!(blocked["visible"], false, "{blocked}");
        assert_eq!(blocked["blocked_by"], "wall", "{blocked}");
        let free = ask(json!({ "from": [5, 10], "to": [6, 12] }));
        assert_eq!(free["visible"], true, "{free}");
        assert!(free.get("blocked_by").is_none(), "{free}");
        // The wall itself, from the side, its own outline ignored.
        let own = ask(json!({ "from": [0, 12.25], "to": [4, 12.25], "ignore": "wall" }));
        assert_eq!(own["visible"], true, "{own}");
        // What is visible from (0, 10): a circle of 8 m cut by the wall.
        let area = ask(json!({ "from": [0, 10], "radius": 8 }));
        assert_eq!(area["walls"], 4, "{area}");
        let points = area["points"].as_array().unwrap();
        assert!(points.len() > 128, "{}", points.len());
        // No point of it lies behind the wall's west face along its height.
        for point in points {
            let (x, y) = (point[0].as_f64().unwrap(), point[1].as_f64().unwrap());
            assert!(
                !(x > 4.2 && (11.0..13.5).contains(&y)),
                "({x}, {y}) is behind the wall"
            );
        }
    }
    // Questions that cannot be answered.
    let both = fails(
        &mut ws,
        "physics_query",
        json!({ "sight": { "from": [0, 0], "to": [1, 1], "radius": 3 } }),
    );
    assert_eq!(both["location"]["path"], "/sight", "{both}");
    let no_from = fails(
        &mut ws,
        "physics_query",
        json!({ "sight": { "to": [1, 1] } }),
    );
    assert_eq!(no_from["location"]["path"], "/sight/from", "{no_from}");
    let two = fails(
        &mut ws,
        "physics_query",
        json!({ "sight": { "from": [0, 0], "radius": 3 }, "touching": "wall" }),
    );
    assert_eq!(two["code"], "invalid_parameter", "{two}");
}

#[test]
fn check_warns_about_an_occluder_without_a_collider_and_unlit_lights() {
    let (_, mut ws) = project("check");
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "spawn", "id": "door", "components": {
                "Transform": {}, "Occluder": {} } },
            { "op": "spawn", "id": "lamp", "components": {
                "Transform": {}, "Light2D": {} } }
        ] }),
    );
    let checked = call(&mut ws, "check", json!({}));
    let found = codes(&checked);
    assert!(
        found.contains(&"occluder_without_collider".to_owned()),
        "{checked}"
    );
    assert!(
        found.contains(&"light_2d_without_lighting".to_owned()),
        "{checked}"
    );
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "set_resource", "name": "Lighting2D", "value": { "enabled": true } },
            { "op": "set", "id": "door", "component": "Collider", "value": {} }
        ] }),
    );
    let checked = call(&mut ws, "check", json!({}));
    let found = codes(&checked);
    assert!(
        !found.contains(&"occluder_without_collider".to_owned()),
        "{checked}"
    );
    assert!(
        !found.contains(&"light_2d_without_lighting".to_owned()),
        "{checked}"
    );
}

#[test]
fn describe_view_lists_2d_lights_and_visions() {
    let (_, mut ws) = project("describe");
    wall_and_spy(&mut ws);
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "set_resource", "name": "Lighting2D", "value": { "enabled": true } },
            { "op": "spawn", "id": "eye", "components": {
                "Camera": { "projection": "Orthographic", "ortho_height": 20 },
                "Transform": { "translation": [3.0, 12.0, 10.0] } } },
            { "op": "spawn", "id": "lamp", "components": {
                "Transform": { "translation": [6.0, 12.0, 0.0] },
                "Light2D": { "radius": 6 } } },
            { "op": "spawn", "id": "hero", "components": {
                "Transform": { "translation": [1.0, 12.0, 0.0] },
                "Sprite": { "rect": [0, 0, 50, 50] },
                "Vision2D": { "radius": 5 } } }
        ] }),
    );
    let described = call(
        &mut ws,
        "describe_view",
        json!({ "camera": "eye", "width": 160, "height": 90 }),
    );
    assert_eq!(described["lights_2d"][0]["entity"], "lamp", "{described}");
    assert_eq!(described["lights_2d"][0]["lit"], true, "{described}");
    assert!(described["lights_2d"][0]["area_points"].as_u64().unwrap() > 128);
    assert_eq!(described["vision"][0]["entity"], "hero", "{described}");
    let effects = described["stats"]["effects"].clone();
    assert!(
        effects.as_array().unwrap().contains(&json!("lights_2d"))
            && effects.as_array().unwrap().contains(&json!("vision")),
        "{effects}"
    );
    let captured = call(
        &mut ws,
        "capture_frame",
        json!({ "camera": "eye", "width": 160, "height": 90 }),
    );
    assert_eq!(captured["stats"]["effects"], effects, "{captured}");
}

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-sight");
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

/// Looks at the spy from two places, and at what is visible from the
/// corner; opens the door after one second.
const WATCHER: &str = r#"
use wish_sdk::prelude::*;

/// What the watcher saw this tick.
#[resource]
pub struct Seen {
    /// The spy, seen from beside the wall's end.
    pub from_end: bool,
    /// The spy, seen from the corner (the wall is in the way).
    pub from_corner: bool,
    /// How many points the area visible from the corner has.
    pub area: u32,
    /// Whether the area holds the spy.
    pub area_holds_spy: bool,
    /// Whether a ray along +X from the wall's west, with nothing to
    /// ignore, hits it (`raycast`).
    pub ray_hits_wall: bool,
}

#[system]
fn look(time: Res<Time>, mut seen: ResMut<Seen>, mut doors: Query<&mut Occluder>) {
    let spy = Vec2::new(6.0, 12.0);
    seen.from_end = can_see(Vec2::new(5.0, 10.0), spy);
    seen.from_corner = can_see(Vec2::new(0.0, 10.0), spy);
    let area = visible_area(Vec2::new(0.0, 10.0), 8.0);
    seen.area = area.len() as u32;
    // Even-odd: is the spy inside the outline?
    let mut inside = false;
    for i in 0..area.len() {
        let (a, b) = (area[i], area[(i + 1) % area.len()]);
        if (a.y > spy.y) != (b.y > spy.y) && spy.x < a.x + (spy.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    seen.area_holds_spy = inside;
    seen.ray_hits_wall = raycast(Vec3::new(0.0, 12.25, 0.0), Vec3::X, 10.0).is_some();
    if time.elapsed > 1.0 {
        for door in doors.iter_mut() {
            door.enabled = false;
        }
    }
}

wish_sdk::export! {
    resources: [Seen],
    systems: [look],
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
fn game_code_sees_past_the_wall_s_end_and_not_through_it() {
    let (dir, mut ws) = project("game");
    ws.set_toolchain(toolchain().clone());
    std::fs::write(dir.join(SCRIPTS_ENTRY), WATCHER).unwrap();
    let built = call(&mut ws, "script_build", json!({}));
    assert_eq!(built["passed"], true, "{built}");
    wall_and_spy(&mut ws);
    // Game code sees the walls as of the end of the previous tick: the
    // door opened during the tick after one second shows a tick later.
    write_test(
        &dir,
        "sight",
        json!([
            { "frames": 2 },
            { "expect": "Seen.from_end == true && Seen.from_corner == false", "message": "the wall hides the spy from the corner" },
            { "expect": "Seen.area > 128 && Seen.area_holds_spy == false", "message": "the visible area is cut by the wall" },
            { "expect": "Seen.ray_hits_wall == true", "message": "a ray cast with nothing to ignore is answered" },
            { "frames": 70 },
            { "expect": "Seen.from_corner == true && Seen.area_holds_spy == true", "message": "with the wall opened, the spy is seen" }
        ]),
    );
    let ran = call(&mut ws, "test_run", json!({}));
    assert_eq!(ran["passed"], true, "{ran}");
}
