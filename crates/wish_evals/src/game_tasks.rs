//! Tasks where the agent writes game code (M5): it edits
//! `scripts/src/lib.rs` with file tools and uses the engine's tools to build,
//! place and test. The check plays the game with a scenario test the agent
//! never saw.

use std::path::Path;

use serde_json::{Value, json};

use crate::probe::{Probe, Verdict};

/// A correct walking game, for the self-test.
const WALK_CODE: &str = r#"
use wish_sdk::prelude::*;

/// The player.
#[component]
pub struct Player {
    /// How fast the player walks.
    #[schema(unit = "m/s", default = 4.0)]
    pub speed: f32,
}

#[system]
fn walk(actions: Actions, mut players: Query<(&mut Transform, &Player)>, time: Res<Time>) {
    let go = actions.axis2("move");
    for (transform, player) in players.iter_mut() {
        transform.translation += Vec3::new(go.x, 0.0, -go.y) * player.speed * time.delta;
    }
}

wish_sdk::export! {
    components: [Player],
    systems: [walk],
}
"#;

/// The collect game of `examples/collect`, for the self-test.
const COLLECT_CODE: &str = include_str!("../../../examples/collect/scripts/src/lib.rs");

const HIDDEN: &str = "tests/zz_check.test.json";

fn write(dir: &Path, relative: &str, text: &str) -> Result<(), String> {
    let path = dir.join(relative);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn spawn_player(probe: &mut Probe) -> Result<(), String> {
    probe
        .call(
            "edit_apply",
            json!({ "ops": [{ "op": "spawn", "id": "player", "components": {
                "Transform": { "translation": [0, 0.5, 0] },
                "Primitive": { "shape": "Cube" },
                "Material": { "base_color": [0.15, 0.35, 0.9, 1] } } }] }),
        )
        .map(|_| ())
}

pub fn walk_setup(dir: &Path) -> Result<(), String> {
    let mut probe = Probe::open(dir)?;
    spawn_player(&mut probe)
}

pub fn collect_setup(dir: &Path) -> Result<(), String> {
    let mut probe = Probe::open(dir)?;
    spawn_player(&mut probe)?;
    let coin = |id: &str, x: f64, z: f64| {
        json!({ "op": "spawn", "id": id, "components": {
            "Transform": { "translation": [x, 0.5, z], "scale": [0.6, 0.6, 0.6] },
            "Primitive": { "shape": "Sphere" },
            "Material": { "base_color": [1, 0.78, 0.2, 1], "metallic": 1, "roughness": 0.3 } } })
    };
    probe
        .call(
            "edit_apply",
            json!({ "ops": [coin("coin_a", 3.0, 0.0), coin("coin_b", 3.0, -3.0), coin("coin_c", 0.0, -3.0)] }),
        )
        .map(|_| ())
}

/// Builds the agent's code, then runs `hidden` and the agent's own tests.
fn play_check(probe: &mut Probe, dir: &Path, hidden: &Value) -> Verdict {
    let built = probe.call("script_build", json!({}))?;
    if built["passed"] != true {
        return Err(format!(
            "the game code does not build: {}",
            built["diagnostics"]
        ));
    }
    probe.check_passes()?;
    let own: Vec<String> = std::fs::read_dir(dir.join("tests"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|name| {
            name.ends_with(".test.json") && name != "boots.test.json" && !name.starts_with("zz_")
        })
        .collect();
    if own.is_empty() {
        return Err("no scenario test of the agent's own in tests/".to_owned());
    }
    let mine = probe.call("test_run", json!({}))?;
    if mine["passed"] != true {
        return Err(format!("the project's tests fail: {}", mine["results"]));
    }
    write(dir, HIDDEN, &hidden.to_string())?;
    let checked = probe.call("test_run", json!({ "paths": [HIDDEN] }));
    let _ = std::fs::remove_file(dir.join(HIDDEN));
    let checked = checked?;
    if checked["passed"] == true {
        Ok(())
    } else {
        Err(format!(
            "the game does not play as asked: {}",
            checked["results"][0]["failure"]
        ))
    }
}

pub fn walk_check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    let hidden = json!({ "format": 1, "scene": "scenes/main.scene.json", "seed": 1, "steps": [
        { "input": { "move": [1, 0] }, "frames": 60 },
        { "expect": "Transform.translation.x > 3.6 && Transform.translation.x < 4.4 && Transform.translation.z > -0.2 && Transform.translation.z < 0.2",
          "entity": "player", "message": "a second to the right moves the player about 4 m along +X" },
        { "input": { "move": [0, 1] }, "frames": 30 },
        { "expect": "Transform.translation.z < -1.8 && Transform.translation.z > -2.2",
          "entity": "player", "message": "half a second forward moves the player about 2 m along -Z" },
        { "frames": 30 },
        { "expect": "Transform.translation.z < -1.8 && Transform.translation.z > -2.2",
          "entity": "player", "message": "without input the player stands still" }
    ] });
    play_check(probe, dir, &hidden)
}

pub fn collect_check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    let hidden = json!({ "format": 1, "scene": "scenes/main.scene.json", "seed": 1, "steps": [
        { "expect": "Score.points == 0 && GameState.won == false", "message": "the game starts with no points" },
        { "input": { "move": [1, 0] }, "until": "Score.points >= 1", "max_frames": 120 },
        { "input": { "move": [0, 1] }, "until": "Score.points >= 2", "max_frames": 120 },
        { "input": { "move": [-1, 0] }, "until": "GameState.won == true", "max_frames": 120 },
        { "expect": "Score.points == 3", "message": "three coins, three points" },
        { "expect": "!has(Coin)", "message": "no coin is left" }
    ] });
    play_check(probe, dir, &hidden)
}

fn own_test(dir: &Path, text: &str) -> Result<(), String> {
    write(dir, "tests/play.test.json", text)
}

pub fn walk_reference(probe: &mut Probe) -> Result<(), String> {
    let dir = probe.root();
    write(&dir, "scripts/src/lib.rs", WALK_CODE)?;
    probe.call("script_build", json!({}))?;
    probe.call(
        "edit_apply",
        json!({ "ops": [{ "op": "set", "id": "player", "component": "Player", "value": {} }] }),
    )?;
    own_test(
        &dir,
        r#"{ "format": 1, "scene": "scenes/main.scene.json", "steps": [
            { "input": { "move": [1, 0] }, "frames": 30 },
            { "expect": "Transform.translation.x > 1.5", "entity": "player" } ] }"#,
    )
}

pub fn collect_reference(probe: &mut Probe) -> Result<(), String> {
    let dir = probe.root();
    write(&dir, "scripts/src/lib.rs", COLLECT_CODE)?;
    probe.call("script_build", json!({}))?;
    let mut ops = vec![json!({ "op": "set", "id": "player", "component": "Player", "value": {} })];
    for coin in ["coin_a", "coin_b", "coin_c"] {
        ops.push(json!({ "op": "set", "id": coin, "component": "Coin", "value": { "value": 1 } }));
    }
    probe.call("edit_apply", json!({ "ops": ops }))?;
    own_test(
        &dir,
        r#"{ "format": 1, "scene": "scenes/main.scene.json", "steps": [
            { "input": { "move": [1, 0] }, "until": "Score.points >= 1", "max_frames": 120 } ] }"#,
    )
}
