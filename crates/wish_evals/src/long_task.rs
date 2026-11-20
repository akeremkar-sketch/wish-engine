//! The scorecard's long task (M12 package 10): from a one-paragraph brief to
//! a finished mini game that hidden scenario tests can win and lose, with a
//! replay of a win that holds. The project starts as `wen new` makes it; the
//! agent makes every entity, the game code, its own tests and the replay.
//!
//! The hidden tests (never shown to the agent) cover the win route, the
//! edges (standing still, a lantern never counts twice, the walls of the
//! square) and the way to lose (three catches by the patrolling guard, then
//! the player is frozen).

use std::path::Path;

use serde_json::{Value, json};

use crate::game_tasks::{hidden_passes, play_check};
use crate::probe::{Probe, Verdict, require_visible};

/// The brief, as a person would write it.
pub const PROMPT: &str = "Make a small game called Lantern Night in this project. The player is a cube named 'player' that starts at (0, 0.5, 0) and walks with the 'move' action of input.json at 5 meters per second (move's x is right, +X; its y is forward, -Z); it can never leave the square from -8 to 8 on x and on z. Four lanterns named lantern_1, lantern_2, lantern_3 and lantern_4 stand at (4, 0.5, 0), (4, 0.5, -4), (-4, 0.5, -4) and (-4, 0.5, 0); each has a Lantern component whose lit field starts false, and an unlit lantern is dark grey. When the player comes closer than 1 meter to an unlit lantern, the lantern is lit: lit becomes true, its Material's base_color turns warm yellow (red and green clearly above blue), and the points field of the Score resource goes up by one (a lit lantern never counts again). A guard named 'guard' starts at (-6, 0.5, 4) and patrols along z = 4 at 3 meters per second, toward +X first, turning around at x = 6 and at x = -6. When the guard comes closer than 1 meter to the player, the player is put back at (0, 0.5, 0) and the count field of the Lives resource, which starts at 3, goes down by one; at 0 lives the lost field of the GameState resource becomes true and the player no longer moves. When all the lanterns are lit, GameState.won becomes true. An interface text named 'hud' always shows exactly 'Lanterns <lit>/<all>, lives <lives>', for example 'Lanterns 2/4, lives 3'. The camera must show the whole square. Write the game code in scripts/src/lib.rs, build it with script_build, prove with scenario tests of your own under tests/ that the game can be won (GameState.won) and can be lost (GameState.lost), and save a replay of a won game named 'win'.";

fn hidden(steps: Value) -> Value {
    json!({ "format": 1, "scene": "scenes/main.scene.json", "seed": 1, "steps": steps })
}

fn at_x(lo: f64, hi: f64) -> String {
    format!("Transform.translation.x > {lo} && Transform.translation.x < {hi}")
}

fn at_z(lo: f64, hi: f64) -> String {
    format!("Transform.translation.z > {lo} && Transform.translation.z < {hi}")
}

/// The win route: the four lanterns in order, the guard never near.
fn win_route() -> Value {
    hidden(json!([
        { "expect": "Score.points == 0 && Lives.count == 3 && GameState.won == false && GameState.lost == false",
          "message": "the game starts with no points, three lives, neither won nor lost" },
        { "expect": "Lantern.lit == false && Material.base_color.x < 0.5 && Material.base_color.y < 0.5",
          "entity": "lantern_1", "message": "an unlit lantern is dark" },
        { "frames": 1 },
        { "expect": "UiText.text == \"Lanterns 0/4, lives 3\"", "entity": "hud", "message": "the hud shows the start" },
        { "input": { "move": [1, 0] }, "frames": 48 },
        { "expect": format!("{} && {}", at_x(3.9, 4.1), at_z(-0.1, 0.1)), "entity": "player",
          "message": "0.8 s to the right at 5 m/s is 4 m along +X" },
        { "expect": "Score.points == 1", "message": "walking past lantern_1 lights it" },
        { "expect": "Lantern.lit == true && Material.base_color.x > 0.6 && Material.base_color.y > 0.4 && Material.base_color.z < min(Material.base_color.x, Material.base_color.y) - 0.2",
          "entity": "lantern_1", "message": "a lit lantern is warm yellow" },
        { "frames": 30 },
        { "expect": "Score.points == 1", "message": "standing at a lit lantern does not count it again" },
        { "expect": at_x(3.9, 4.1), "entity": "player", "message": "without input the player stands still" },
        { "input": { "move": [0, 1] }, "frames": 48 },
        { "expect": "Score.points == 2", "message": "lantern_2" },
        { "expect": "Lantern.lit == false", "entity": "lantern_3", "message": "lantern_3 is not lit yet" },
        { "input": { "move": [-1, 0] }, "frames": 96 },
        { "expect": "Score.points == 3 && GameState.won == false", "message": "lantern_3, not won yet" },
        { "input": { "move": [0, -1] }, "frames": 48 },
        { "expect": "Score.points == 4 && GameState.won == true && Lives.count == 3",
          "message": "four lanterns win, and the guard never came near" },
        { "frames": 2 },
        { "expect": "UiText.text == \"Lanterns 4/4, lives 3\"", "entity": "hud", "message": "the hud shows the win" },
        { "expect": format!("{} && Transform.translation.x > -6.05 && Transform.translation.x < 6.05", at_z(3.9, 4.1)),
          "entity": "guard", "message": "the guard stays on its line" },
        { "expect": "count(Lantern) == 4", "message": "the lanterns stay" }
    ]))
}

/// The edges: the walls of the square, standing still.
fn edges() -> Value {
    hidden(json!([
        { "frames": 30 },
        { "expect": format!("{} && {}", at_x(-0.01, 0.01), at_z(-0.01, 0.01)), "entity": "player",
          "message": "with no input the player stays at the start" },
        { "input": { "move": [1, 0] }, "frames": 240 },
        { "expect": format!("{} && {}", at_x(7.9, 8.01), at_z(-0.1, 0.1)), "entity": "player",
          "message": "the player stops at the edge x = 8" },
        { "input": { "move": [0, -1] }, "frames": 240 },
        { "expect": format!("{} && {}", at_x(7.9, 8.01), at_z(7.9, 8.01)), "entity": "player",
          "message": "and at the edge z = 8" },
        { "expect": "Lives.count == 3 && Score.points == 1", "message": "the corner is safe; one lantern was passed" }
    ]))
}

/// The way to lose: stand on the guard's line three times.
fn lose_route() -> Value {
    let wait_for =
        |lives: u32| json!({ "until": format!("Lives.count == {lives}"), "max_frames": 600 });
    let to_line = json!({ "input": { "move": [0, -1] }, "frames": 48 });
    let back = json!({ "expect": format!("{} && {}", at_x(-0.1, 0.1), at_z(-0.1, 0.1)), "entity": "player",
                       "message": "a caught player is put back at the start" });
    hidden(json!([
        to_line,
        { "expect": format!("{} && Transform.translation.x > -3.8 && Transform.translation.x < -3.4", at_z(3.9, 4.1)),
          "entity": "guard", "message": "after 0.8 s the guard has walked 2.4 m toward +X" },
        wait_for(2), back,
        to_line, wait_for(1), back,
        to_line, wait_for(0),
        { "expect": "GameState.lost == true && GameState.won == false", "message": "no lives left: lost" },
        { "input": { "move": [1, 0] }, "frames": 60 },
        { "expect": at_x(-0.1, 0.1), "entity": "player", "message": "a lost player no longer moves" },
        { "expect": "Lives.count == 0", "message": "lives stay at 0" },
        { "frames": 1 },
        { "expect": "UiText.text == \"Lanterns 0/4, lives 0\"", "entity": "hud", "message": "the hud shows no lives" }
    ]))
}

/// The replay named `win` exists, plays back, and ends won.
fn win_replay(probe: &mut Probe, dir: &Path) -> Verdict {
    if !dir.join("replays/win.replay.jsonl").is_file() {
        return Err("there is no replay named 'win' (replays/win.replay.jsonl)".to_owned());
    }
    let ran = probe.call(
        "replay",
        json!({ "action": "run", "name": "win", "keep": true }),
    )?;
    let session = ran["session"].as_str().map(str::to_owned);
    let verdict = if ran["passed"] != true {
        Err(format!("the replay 'win' does not play back: {ran}"))
    } else if let Some(session) = &session {
        let got = probe.call(
            "entity_get",
            json!({ "session": session, "ids": [], "resources": ["GameState"] }),
        )?;
        if got["resources"]["GameState"]["won"] == true {
            Ok(())
        } else {
            Err("the replay 'win' does not end in a won game".to_owned())
        }
    } else {
        Err("the replay's session was not kept".to_owned())
    };
    if let Some(session) = session {
        let _ = probe.call("sim_stop", json!({ "session": session }));
    }
    verdict
}

/// The agent's own tests (not the starter's, not the hidden ones).
fn own_tests(dir: &Path) -> Vec<String> {
    let mut texts: Vec<(String, String)> = std::fs::read_dir(dir.join("tests"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name.ends_with(".test.json") && name != "boots.test.json" && !name.starts_with("zz_"))
                .then(|| std::fs::read_to_string(e.path()).ok().map(|t| (name, t)))
                .flatten()
        })
        .collect();
    texts.sort();
    texts.into_iter().map(|(_, t)| t).collect()
}

pub fn check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    // The brief asks for a test that wins and one that loses.
    let own = own_tests(dir);
    if !own.iter().any(|t| t.contains("GameState.won")) {
        return Err(
            "no test of the agent's own checks GameState.won (that the game can be won)".to_owned(),
        );
    }
    if !own.iter().any(|t| t.contains("GameState.lost")) {
        return Err(
            "no test of the agent's own checks GameState.lost (that the game can be lost)"
                .to_owned(),
        );
    }
    let seen = probe.visible(None)?;
    for id in [
        "player",
        "guard",
        "lantern_1",
        "lantern_2",
        "lantern_3",
        "lantern_4",
    ] {
        require_visible(&seen, id, 30.0)?;
    }
    play_check(probe, dir, &win_route())?;
    hidden_passes(probe, dir, &edges())?;
    hidden_passes(probe, dir, &lose_route())?;
    win_replay(probe, dir)
}

/// The reference game.
pub const CODE: &str = r#"
use wish_sdk::prelude::*;

/// The player.
#[component]
pub struct Player {
    /// Walking speed.
    #[schema(unit = "m/s", default = 5.0)]
    pub speed: f32,
}

/// A lantern to light.
#[component]
pub struct Lantern {
    /// Whether it has been lit.
    pub lit: bool,
}

/// The guard that patrols.
#[component]
pub struct Guard {
    /// Patrol speed.
    #[schema(unit = "m/s", default = 3.0)]
    pub speed: f32,
}

/// The text that shows the state.
#[component]
pub struct Hud {
    /// What it shows now, as a key (0: nothing yet).
    pub shown: u32,
}

/// Lanterns lit.
#[resource]
pub struct Score {
    /// Lanterns lit so far.
    pub points: u32,
}

/// Lives left.
#[resource]
pub struct Lives {
    /// Lives left.
    #[schema(default = 3)]
    pub count: u32,
}

/// Where the game stands.
#[resource]
pub struct GameState {
    /// Every lantern is lit.
    pub won: bool,
    /// No lives are left.
    pub lost: bool,
}

const EDGE: f32 = 8.0;
const REACH: f32 = 1.0;

#[system]
fn walk(actions: Actions, time: Res<Time>, state: Res<GameState>, mut players: Query<(&mut Transform, &Player)>) {
    if state.lost {
        return;
    }
    let go = actions.axis2("move");
    for (at, player) in players.iter_mut() {
        let next = at.translation + Vec3::new(go.x, 0.0, -go.y) * player.speed * time.delta;
        at.translation = Vec3::new(next.x.clamp(-EDGE, EDGE), next.y, next.z.clamp(-EDGE, EDGE));
    }
}

/// Where the guard is follows from the time alone: x from -6 to 6 and back.
#[system(after = "walk")]
fn patrol(time: Res<Time>, mut guards: Query<(&mut Transform, &Guard)>) {
    for (at, guard) in guards.iter_mut() {
        let d = (time.elapsed as f32 * guard.speed).rem_euclid(24.0);
        at.translation.x = if d < 12.0 { -6.0 + d } else { 18.0 - d };
    }
}

#[system(after = "patrol")]
fn light(
    players: Query<&Transform, With<Player>>,
    mut lanterns: Query<(&Transform, &mut Lantern, &mut Material)>,
    mut score: ResMut<Score>,
) {
    for at in players.iter() {
        for (spot, lantern, look) in lanterns.iter_mut() {
            if !lantern.lit && (spot.translation - at.translation).length() < REACH {
                lantern.lit = true;
                look.base_color = Color::rgba(1.0, 0.8, 0.25, 1.0);
                score.points += 1;
            }
        }
    }
}

#[system(after = "light")]
fn caught(
    guards: Query<&Transform, With<Guard>>,
    mut players: Query<&mut Transform, (With<Player>, Without<Guard>)>,
    mut lives: ResMut<Lives>,
    mut state: ResMut<GameState>,
) {
    if state.lost {
        return;
    }
    let spots: Vec<Vec3> = guards.iter().map(|g| g.translation).collect();
    for at in players.iter_mut() {
        if spots.iter().any(|g| (*g - at.translation).length() < REACH) {
            at.translation = Vec3::new(0.0, 0.5, 0.0);
            lives.count = lives.count.saturating_sub(1);
            if lives.count == 0 {
                state.lost = true;
            }
        }
    }
}

#[system(after = "caught")]
fn finish(lanterns: Query<&Lantern>, mut state: ResMut<GameState>) {
    let all = lanterns.iter().count();
    let lit = lanterns.iter().filter(|l| l.lit).count();
    if all > 0 && lit == all && !state.lost {
        state.won = true;
    }
}

#[system(after = "finish")]
fn show(score: Res<Score>, lives: Res<Lives>, lanterns: Query<&Lantern>, mut huds: Query<(&mut Hud, &mut UiText)>) {
    let all = lanterns.iter().count() as u32;
    let key = 1 + score.points * 1000 + all * 10 + lives.count;
    for (hud, words) in huds.iter_mut() {
        if hud.shown != key {
            hud.shown = key;
            words.text = text(&format!("Lanterns {}/{}, lives {}", score.points, all, lives.count));
        }
    }
}

wish_sdk::export! {
    components: [Player, Lantern, Guard, Hud],
    resources: [Score, Lives, GameState],
    systems: [walk, patrol, light, caught, finish, show],
}
"#;

const OWN_WIN: &str = r#"{ "format": 1, "scene": "scenes/main.scene.json", "steps": [
    { "input": { "move": [1, 0] }, "frames": 48 },
    { "input": { "move": [0, 1] }, "frames": 48 },
    { "input": { "move": [-1, 0] }, "frames": 96 },
    { "input": { "move": [0, -1] }, "frames": 48 },
    { "expect": "GameState.won == true" } ] }"#;

const OWN_LOSE: &str = r#"{ "format": 1, "scene": "scenes/main.scene.json", "steps": [
    { "input": { "move": [0, -1] }, "frames": 48 }, { "until": "Lives.count == 2", "max_frames": 600 },
    { "input": { "move": [0, -1] }, "frames": 48 }, { "until": "Lives.count == 1", "max_frames": 600 },
    { "input": { "move": [0, -1] }, "frames": 48 }, { "until": "GameState.lost == true", "max_frames": 600 },
    { "expect": "Lives.count == 0" } ] }"#;

fn write(dir: &Path, relative: &str, text: &str) -> Result<(), String> {
    let path = dir.join(relative);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn lantern(id: &str, x: f64, z: f64) -> Value {
    json!({ "op": "spawn", "id": id, "components": {
        "Transform": { "translation": [x, 0.5, z], "scale": [0.5, 0.5, 0.5] },
        "Primitive": { "shape": "Sphere" },
        "Material": { "base_color": [0.15, 0.15, 0.15, 1] },
        "Lantern": {} } })
}

/// The scene of the reference solution (after the code is built).
fn reference_scene(probe: &mut Probe) -> Result<(), String> {
    probe.call(
        "edit_apply",
        json!({ "ops": [
            { "op": "set", "id": "camera", "component": "Transform",
              "value": { "translation": [0, 20, 14], "euler_deg": [-55, 0, 0] } },
            { "op": "spawn", "id": "player", "components": {
                "Transform": { "translation": [0, 0.5, 0] }, "Primitive": { "shape": "Cube" },
                "Material": { "base_color": [0.15, 0.35, 0.9, 1] }, "Player": {} } },
            lantern("lantern_1", 4.0, 0.0), lantern("lantern_2", 4.0, -4.0),
            lantern("lantern_3", -4.0, -4.0), lantern("lantern_4", -4.0, 0.0),
            { "op": "spawn", "id": "guard", "components": {
                "Transform": { "translation": [-6, 0.5, 4] }, "Primitive": { "shape": "Cube" },
                "Material": { "base_color": [0.85, 0.1, 0.1, 1] }, "Guard": {} } },
            { "op": "spawn", "id": "hud", "components": {
                "UiNode": { "position": "Absolute", "offset": [24, 24] },
                "UiText": { "text": "Lanterns 0/4, lives 3", "size": 32 }, "Hud": {} } }
        ] }),
    )?;
    Ok(())
}

/// Records the replay of a win, the way an agent would with the tools.
pub fn record_win(probe: &mut Probe) -> Result<(), String> {
    record_route(
        probe,
        &[([1, 0], 48), ([0, 1], 48), ([-1, 0], 96), ([0, -1], 48)],
    )
}

/// Records a replay named `win` of walking `route`.
fn record_route(probe: &mut Probe, route: &[([i32; 2], u32)]) -> Result<(), String> {
    let started = probe.call("sim_start", json!({ "seed": 1 }))?;
    let s = started["session"].as_str().ok_or("no session")?.to_owned();
    probe.call("replay", json!({ "action": "record", "session": s }))?;
    for &(input, frames) in route {
        probe.call(
            "sim_step",
            json!({ "session": s, "frames": frames, "input": { "move": input } }),
        )?;
    }
    probe.call("sim_step", json!({ "session": s, "frames": 10 }))?;
    probe.call(
        "replay",
        json!({ "action": "save", "session": s, "name": "win", "overwrite": true }),
    )?;
    probe.call("sim_stop", json!({ "session": s })).map(|_| ())
}

/// Which replay a solution leaves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Replay {
    /// A won game named `win`.
    Win,
    /// None at all.
    Missing,
    /// One named `win` that stops before the last lantern.
    NotWon,
}

fn reference_with(probe: &mut Probe, code: &str, replay: Replay) -> Result<(), String> {
    let dir = probe.root();
    write(&dir, "scripts/src/lib.rs", code)?;
    let built = probe.call("script_build", json!({}))?;
    if built["passed"] != true {
        return Err(format!("the reference game does not build: {built}"));
    }
    reference_scene(probe)?;
    write(&dir, "tests/win.test.json", OWN_WIN)?;
    write(&dir, "tests/lose.test.json", OWN_LOSE)?;
    match replay {
        Replay::Win => record_win(probe),
        Replay::Missing => Ok(()),
        Replay::NotWon => record_route(probe, &[([1, 0], 48), ([0, 1], 48)]),
    }
}

pub fn reference(probe: &mut Probe) -> Result<(), String> {
    reference_with(probe, CODE, Replay::Win)
}

fn mutant(probe: &mut Probe, from: &str, to: &str) -> Result<(), String> {
    if !CODE.contains(from) {
        return Err(format!("the mutant's text is not in the reference: {from}"));
    }
    reference_with(probe, &CODE.replacen(from, to, 1), Replay::Win)
}

/// Makes a solution in a prepared project.
pub type Solution = fn(&mut Probe) -> Result<(), String>;

/// Plausible wrong solutions, each with what it gets wrong. The hidden
/// checks must refute every one (`wish-evals selftest`).
pub static MUTANTS: &[(&str, Solution)] = &[
    ("a lit lantern counts again", |p| {
        mutant(p, "if !lantern.lit && ", "if ")
    }),
    ("no walls: the player walks out of the square", |p| {
        mutant(
            p,
            "Vec3::new(next.x.clamp(-EDGE, EDGE), next.y, next.z.clamp(-EDGE, EDGE))",
            "next",
        )
    }),
    ("the player still moves after losing", |p| {
        mutant(
            p,
            "if state.lost {\n        return;\n    }\n    let go",
            "let go",
        )
    }),
    ("the guard turns at x = 5 and -5", |p| {
        mutant(
            p,
            "rem_euclid(24.0);\n        at.translation.x = if d < 12.0 { -6.0 + d } else { 18.0 - d };",
            "rem_euclid(20.0);\n        at.translation.x = if d < 10.0 { -5.0 + d } else { 15.0 - d };",
        )
    }),
    ("no replay named 'win'", |p| {
        reference_with(p, CODE, Replay::Missing)
    }),
    ("a replay named 'win' of a game that is not won", |p| {
        reference_with(p, CODE, Replay::NotWon)
    }),
];

// --------------------------------------------------------------------------
// The second agent's change (optional step of the scorecard).

pub const FOLLOW_UP: &str = "Add a fifth lantern named lantern_5 at (0, 0.5, -6), like the other four. The game is now won when all five are lit, and the hud counts to five ('Lanterns 2/5, lives 3'). Keep the project's own tests passing (change them where the game changed) and build the game again.";

pub fn follow_up_check(probe: &mut Probe, dir: &Path) -> Verdict {
    play_check(
        probe,
        dir,
        &hidden(json!([
            { "expect": "count(Lantern) == 5", "message": "five lanterns" },
            { "expect": "Lantern.lit == false && Material.base_color.x < 0.5", "entity": "lantern_5",
              "message": "the new lantern starts unlit and dark" },
            { "input": { "move": [1, 0] }, "frames": 48 },
            { "input": { "move": [0, 1] }, "frames": 48 },
            { "input": { "move": [-1, 0] }, "frames": 96 },
            { "input": { "move": [0, -1] }, "frames": 48 },
            { "expect": "Score.points == 4 && GameState.won == false", "message": "four of five is not a win" },
            { "input": { "move": [1, 0] }, "frames": 48 },
            { "input": { "move": [0, 1] }, "frames": 72 },
            { "expect": "Score.points == 5 && GameState.won == true", "message": "the fifth lantern wins" },
            { "frames": 2 },
            { "expect": "UiText.text == \"Lanterns 5/5, lives 3\"", "entity": "hud", "message": "the hud counts to five" }
        ])),
    )
}

pub fn follow_up_reference(probe: &mut Probe) -> Result<(), String> {
    probe.call(
        "edit_apply",
        json!({ "ops": [lantern("lantern_5", 0.0, -6.0)] }),
    )?;
    let dir = probe.root();
    write(&dir, "tests/win.test.json", &OWN_WIN.replace(
        "{ \"expect\": \"GameState.won == true\" }",
        "{ \"input\": { \"move\": [1, 0] }, \"frames\": 48 }, { \"input\": { \"move\": [0, 1] }, \"frames\": 72 }, { \"expect\": \"GameState.won == true\" }",
    ))
}

/// The long task's id.
pub const ID: &str = "42_lantern_night";

/// The long task.
pub fn task() -> crate::tasks::Task {
    crate::tasks::Task {
        id: ID,
        prompt: PROMPT,
        setup: |_| Ok(()),
        baseline: |_| Ok(Value::Null),
        check,
        reference,
        files: true,
    }
}
