//! The scorecard's optional second-agent step: after a task passed, a
//! second agent (a fresh session that never saw the first one's work) is
//! asked to change the result, and a small hidden check judges the change.
//! It measures whether what the first agent made can be maintained. Off by
//! default (`--second-agent`): it spends quota. Written for a sample of the
//! open set that covers scenes, interfaces and game code, and for the long
//! task.

use std::path::Path;

use serde_json::json;

use crate::game_tasks::play_check;
use crate::probe::{Probe, Verdict, cube_bottom, require_visible};

/// A change asked of a second agent.
pub trait FollowUpTask {
    /// What the second agent is asked.
    fn prompt(&self) -> &str;
    /// Whether the change was made (the result of the first task is there).
    fn check(&self, probe: &mut Probe, dir: &Path) -> Verdict;
    /// A correct change, applied on the task's reference (self-test).
    fn reference(&self, probe: &mut Probe) -> Result<(), String>;
}

/// A follow-up written in code.
pub struct FollowUp {
    /// The task it follows.
    pub task: &'static str,
    pub prompt: &'static str,
    pub check: fn(&mut Probe, &Path) -> Verdict,
    pub reference: fn(&mut Probe) -> Result<(), String>,
}

impl FollowUpTask for FollowUp {
    fn prompt(&self) -> &str {
        self.prompt
    }
    fn check(&self, probe: &mut Probe, dir: &Path) -> Verdict {
        (self.check)(probe, dir)
    }
    fn reference(&self, probe: &mut Probe) -> Result<(), String> {
        (self.reference)(probe)
    }
}

fn edit(probe: &mut Probe, ops: serde_json::Value) -> Result<(), String> {
    probe.call("edit_apply", json!({ "ops": ops })).map(|_| ())
}

fn hidden(steps: serde_json::Value) -> serde_json::Value {
    json!({ "format": 1, "scene": "scenes/main.scene.json", "seed": 1, "steps": steps })
}

// 01: a taller, blue cube.

fn cube_check(probe: &mut Probe, _: &Path) -> Verdict {
    if !probe
        .query(None, "Primitive.shape == \"Cube\" && Material.base_color.x > 0.5 && Material.base_color.y < 0.4 && Material.base_color.z < 0.4")?
        .is_empty()
    {
        return Err("a red cube is still there".to_owned());
    }
    let blue = probe.query(
        None,
        "Primitive.shape == \"Cube\" && Material.base_color.z > 0.5 && Material.base_color.x < 0.4",
    )?;
    let seen = probe.visible(None)?;
    for id in &blue {
        let m = probe.matrix(None, id)?;
        let height = (m[4] * m[4] + m[5] * m[5] + m[6] * m[6]).sqrt();
        if (1.8..=2.2).contains(&height)
            && cube_bottom(&m).abs() <= 0.05
            && m[12].abs() <= 2.0
            && m[14].abs() <= 2.0
            && require_visible(&seen, id, 50.0).is_ok()
        {
            return Ok(());
        }
    }
    Err(format!(
        "no visible blue cube 2 m high standing on the ground in the middle (blue cubes: {blue:?})"
    ))
}

fn cube_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([
            { "op": "set", "id": "red_cube", "component": "Transform",
              "value": { "translation": [0, 1, 0], "scale": [1, 2, 1] } },
            { "op": "set", "id": "red_cube", "component": "Material",
              "value": { "base_color": [0.1, 0.2, 0.9, 1] } }
        ]),
    )
}

// 02: a fourth step.

fn stairs_check(probe: &mut Probe, _: &Path) -> Verdict {
    let cubes = probe.query(None, "Primitive.shape == \"Cube\"")?;
    let seen = probe.visible(None)?;
    let mut placed = Vec::new();
    for id in &cubes {
        if require_visible(&seen, id, 30.0).is_ok() {
            placed.push(probe.position(None, id)?);
        }
    }
    placed.sort_by(|a, b| a[1].total_cmp(&b[1]));
    let step = |p: &[f64; 3], q: &[f64; 3]| {
        q[1] - p[1] >= 0.95 && ((q[0] - p[0]).powi(2) + (q[2] - p[2]).powi(2)).sqrt() < 4.0
    };
    // The longest rising chain of visible cubes.
    let mut longest = vec![1usize; placed.len()];
    for j in 0..placed.len() {
        for i in 0..j {
            if step(&placed[i], &placed[j]) {
                longest[j] = longest[j].max(longest[i] + 1);
            }
        }
    }
    let best = longest.iter().copied().max().unwrap_or(0);
    if best >= 4 {
        Ok(())
    } else {
        Err(format!(
            "the longest stair of visible cubes has {best} steps, not 4"
        ))
    }
}

fn stairs_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "spawn", "id": "p4", "components": { "Primitive": { "shape": "Cube" },
            "Transform": { "translation": [5.5, 3.5, -3], "scale": [2, 0.4, 2] } } }]),
    )
}

// 11: walk faster.

fn walk_check(probe: &mut Probe, dir: &Path) -> Verdict {
    play_check(
        probe,
        dir,
        &hidden(json!([
            { "input": { "move": [1, 0] }, "frames": 60 },
            { "expect": "Transform.translation.x > 5.5 && Transform.translation.x < 6.5 && Transform.translation.z > -0.2 && Transform.translation.z < 0.2",
              "entity": "player", "message": "a second to the right is now about 6 m" },
            { "input": { "move": [0, 1] }, "frames": 30 },
            { "expect": "Transform.translation.z < -2.7 && Transform.translation.z > -3.3",
              "entity": "player", "message": "half a second forward is about 3 m" },
            { "frames": 30 },
            { "expect": "Transform.translation.z < -2.7 && Transform.translation.z > -3.3",
              "entity": "player", "message": "without input the player still stands still" }
        ])),
    )
}

fn walk_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "set", "id": "player", "component": "Player", "value": { "speed": 6 } }]),
    )
}

// 12: coins worth two.

fn coins_check(probe: &mut Probe, dir: &Path) -> Verdict {
    play_check(
        probe,
        dir,
        &hidden(json!([
            { "expect": "Score.points == 0", "message": "no points at the start" },
            { "input": { "move": [1, 0] }, "until": "Score.points >= 1", "max_frames": 120 },
            { "expect": "Score.points == 2", "message": "a coin is now worth 2 points" },
            { "input": { "move": [0, 1] }, "until": "Score.points >= 3", "max_frames": 120 },
            { "input": { "move": [-1, 0] }, "until": "GameState.won == true", "max_frames": 120 },
            { "expect": "Score.points == 6", "message": "three coins, six points" }
        ])),
    )
}

fn coins_reference(probe: &mut Probe) -> Result<(), String> {
    let ops: Vec<_> = ["coin_a", "coin_b", "coin_c"]
        .iter()
        .map(
            |coin| json!({ "op": "set", "id": coin, "component": "Coin", "value": { "value": 2 } }),
        )
        .collect();
    edit(probe, json!(ops))
}

// 27: a shorter lift.

fn lift_check(probe: &mut Probe, dir: &Path) -> Verdict {
    let x = |lo: f64, hi: f64| {
        format!("Transform.translation.x > {lo} && Transform.translation.x < {hi}")
    };
    play_check(
        probe,
        dir,
        &hidden(json!([
            { "frames": 40 },
            { "expect": x(1.6, 2.4), "entity": "lift", "message": "after 2/3 s the lift is about 2 m along +X" },
            { "frames": 40 },
            { "expect": x(3.5, 4.2), "entity": "lift", "message": "after 4/3 s it is at the new far end, 4 m" },
            { "frames": 80 },
            { "expect": format!("{} && Transform.translation.y > 0.99 && Transform.translation.y < 1.01", x(-0.2, 0.6)),
              "entity": "lift", "message": "after 8/3 s it is back, at the same height" },
            { "frames": 40 },
            { "expect": x(1.6, 2.4), "entity": "lift", "message": "and it goes again" }
        ])),
    )
}

fn lift_reference(probe: &mut Probe) -> Result<(), String> {
    let code = crate::m7_tasks::LIFT_CODE
        .replace("rem_euclid(12.0)", "rem_euclid(8.0)")
        .replace(
            "d < 6.0 { d } else { 12.0 - d }",
            "d < 4.0 { d } else { 8.0 - d }",
        );
    std::fs::write(probe.root().join("scripts/src/lib.rs"), code).map_err(|e| e.to_string())?;
    let built = probe.call("script_build", json!({}))?;
    if built["passed"] == true {
        Ok(())
    } else {
        Err(format!("the shorter lift does not build: {built}"))
    }
}

// 33: a third button.

fn menu_check(probe: &mut Probe, _: &Path) -> Verdict {
    use crate::m7_tasks::{bbox, interface};
    let shown = interface(probe)?;
    let find = |text: &str| {
        shown
            .iter()
            .find(|e| e["text"].as_str().is_some_and(|t| t.trim() == text))
            .map(bbox)
    };
    let (Some(title), Some(play), Some(options), Some(quit)) =
        (find("Menu"), find("Play"), find("Options"), find("Quit"))
    else {
        return Err(
            "the screen needs texts \"Menu\", \"Play\", \"Options\" and \"Quit\"".to_owned(),
        );
    };
    if !(title[1] < play[1] && play[1] < options[1] && options[1] < quit[1]) {
        return Err(format!(
            "the order from the top should be Menu, Play, Options, Quit, but they are at y = {}, {}, {}, {}",
            title[1], play[1], options[1], quit[1]
        ));
    }
    let buttons: Vec<[f64; 4]> = shown
        .iter()
        .filter(|e| !e["button"].is_null())
        .map(bbox)
        .collect();
    if buttons.len() != 3 {
        return Err(format!(
            "there should be three buttons, not {}",
            buttons.len()
        ));
    }
    for (label, name) in [(play, "Play"), (options, "Options"), (quit, "Quit")] {
        let inside = buttons.iter().any(|b| {
            label[0] >= b[0] - 1.0
                && label[1] >= b[1] - 1.0
                && label[2] <= b[2] + 1.0
                && label[3] <= b[3] + 1.0
        });
        if !inside {
            return Err(format!("\"{name}\" is not inside a button"));
        }
    }
    if buttons
        .iter()
        .any(|b| ((b[0] + b[2]) / 2.0 - 320.0).abs() > 64.0)
    {
        return Err("the buttons are not in the middle (horizontally)".to_owned());
    }
    Ok(())
}

fn menu_reference(probe: &mut Probe) -> Result<(), String> {
    let button = |id: &str, order: u32| {
        json!({ "op": "spawn", "id": id, "parent": "menu", "order": order,
                "components": { "UiNode": { "padding": [12, 40, 12, 40], "background": [0.1, 0.3, 0.9, 1] }, "UiButton": {} } })
    };
    let label = |id: &str, parent: &str, text: &str| {
        json!({ "op": "spawn", "id": id, "parent": parent, "order": 0,
                "components": { "UiNode": {}, "UiText": { "text": text, "size": 36 } } })
    };
    edit(
        probe,
        json!([
            { "op": "despawn", "id": "quit_label" },
            { "op": "despawn", "id": "quit_button" },
            button("options_button", 2), label("options_label", "options_button", "Options"),
            button("quit_button", 3), label("quit_label", "quit_button", "Quit")
        ]),
    )
}

/// Every follow-up written in code, by the task it follows.
pub static ALL: &[FollowUp] = &[
    FollowUp {
        task: "01_red_cube",
        prompt: "Make the red cube in the scene 2 meters tall instead of 1, still standing on the ground in the same place, and paint it blue.",
        check: cube_check,
        reference: cube_reference,
    },
    FollowUp {
        task: "02_stairs",
        prompt: "Add a fourth platform that continues the stairs upward: at least 1 m higher than the highest platform and closer than 4 m to it horizontally. All four platforms should be visible from the camera.",
        check: stairs_check,
        reference: stairs_reference,
    },
    FollowUp {
        task: "11_walk_code",
        prompt: "The player should now walk at 6 meters per second instead of 4. Keep the project's own tests passing and build the game again if the code changes.",
        check: walk_check,
        reference: walk_reference,
    },
    FollowUp {
        task: "12_collect_game",
        prompt: "Each coin should now be worth 2 points instead of 1; the game is still won when no coin is left. Keep the project's own tests passing.",
        check: coins_check,
        reference: coins_reference,
    },
    FollowUp {
        task: "27_moving_platform",
        prompt: "The lift should now travel only 4 meters along +X before it comes back (still at 3 meters per second, still repeating). Build the game again and keep the project's own tests passing.",
        check: lift_check,
        reference: lift_reference,
    },
    FollowUp {
        task: "33_vertical_menu",
        prompt: "Add a third button labelled 'Options' to the menu, between 'Play' and 'Quit', looking like the other two.",
        check: menu_check,
        reference: menu_reference,
    },
    FollowUp {
        task: crate::long_task::ID,
        prompt: crate::long_task::FOLLOW_UP,
        check: crate::long_task::follow_up_check,
        reference: crate::long_task::follow_up_reference,
    },
];

/// The follow-up of a task written in code, if it has one.
pub fn for_task(id: &str) -> Option<&'static FollowUp> {
    ALL.iter().find(|f| f.task == id)
}
