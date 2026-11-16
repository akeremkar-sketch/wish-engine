//! Tasks of the showcase work (M7): the building blocks of a platformer and
//! of small games (cameras, moving platforms, respawning, pickups, impulses,
//! timers), placeholder assets, interface layout, layers, replays and a
//! second level. Most come from the friction log: each is a place where an
//! agent tripped while the showcase game was built. Checked by what the
//! engine does.

use std::path::Path;

use serde_json::{Value, json};

use crate::game_tasks::play_check;
use crate::probe::{Probe, Verdict};
use crate::tasks::Task;

fn write_file(dir: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    let path = dir.join(relative);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

fn edit(probe: &mut Probe, ops: Value) -> Result<(), String> {
    probe.call("edit_apply", json!({ "ops": ops })).map(|_| ())
}

fn setup_ops(dir: &Path, ops: Value) -> Result<(), String> {
    let mut probe = Probe::open(dir)?;
    edit(&mut probe, ops)
}

fn nothing(_: &mut Probe) -> Result<Value, String> {
    Ok(Value::Null)
}

fn num(value: &Value) -> f64 {
    value.as_f64().unwrap_or(f64::NAN)
}

fn start(probe: &mut Probe) -> Result<String, String> {
    let started = probe.call("sim_start", json!({}))?;
    started["session"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "sim_start gave no session".to_owned())
}

fn step(probe: &mut Probe, session: &str, frames: u64, input: Value) -> Result<Value, String> {
    probe.call(
        "sim_step",
        json!({ "session": session, "frames": frames, "input": input }),
    )
}

fn live(probe: &mut Probe, session: &str, id: &str) -> Result<Value, String> {
    let got = probe.call("entity_get", json!({ "session": session, "ids": [id] }))?;
    Ok(got["entities"][0]["components"].clone())
}

fn live_pos(probe: &mut Probe, session: &str, id: &str) -> Result<[f64; 3], String> {
    let t = live(probe, session, id)?["Transform"]["translation"].clone();
    Ok([num(&t[0]), num(&t[1]), num(&t[2])])
}

fn solid_ground() -> Value {
    json!({ "op": "set", "id": "ground", "component": "Collider",
            "value": { "shape": "Box", "size": [1, 0.1, 1], "offset": [0, -0.05, 0] } })
}

fn cube(id: &str, at: [f64; 3]) -> Value {
    json!({ "op": "spawn", "id": id, "components": {
        "Transform": { "translation": at }, "Primitive": { "shape": "Cube" } } })
}

fn bbox(element: &Value) -> [f64; 4] {
    let b = &element["bounding_box"];
    [num(&b[0]), num(&b[1]), num(&b[2]), num(&b[3])]
}

fn interface(probe: &mut Probe) -> Result<Vec<Value>, String> {
    let seen = probe.call("describe_view", json!({ "width": 640, "height": 360 }))?;
    Ok(seen["interface"].as_array().cloned().unwrap_or_default())
}

fn hidden(steps: Value) -> Value {
    json!({ "format": 1, "scene": "scenes/main.scene.json", "seed": 1, "steps": steps })
}

/// The game module and a scenario test of its own, for a reference solution.
fn reference_code(probe: &mut Probe, code: &str, test: &str) -> Result<(), String> {
    let dir = probe.root();
    write_file(&dir, "scripts/src/lib.rs", code.as_bytes())?;
    probe.call("script_build", json!({}))?;
    write_file(&dir, "tests/own.test.json", test.as_bytes())
}

const OWN_TEST: &str =
    r#"{ "format": 1, "scene": "scenes/main.scene.json", "steps": [ { "frames": 30 } ] }"#;

// --------------------------------------------------------------------------
// 26: a camera that follows the player (game code).

fn follow_setup(dir: &Path) -> Result<(), String> {
    setup_ops(
        dir,
        json!([{ "op": "spawn", "id": "player", "components": {
            "Transform": { "translation": [0, 0.5, 0] }, "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.2, 0.4, 0.9, 1] } } }]),
    )
}

fn follow_check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    play_check(
        probe,
        dir,
        &hidden(json!([
            { "frames": 5 },
            { "input": { "move": [0, 1] }, "frames": 120 },
            { "expect": "Transform.translation.z < -7.5 && Transform.translation.z > -8.5",
              "entity": "player", "message": "the player walked about 8 m forward (-Z) at 4 m/s" },
            { "expect": "Transform.translation.z < 1 && Transform.translation.z > -6 && Transform.translation.y > 0.5",
              "entity": "camera", "message": "the camera came along behind the player (it started at z = 7)" }
        ])),
    )
}

const FOLLOW_CODE: &str = r#"
use wish_sdk::prelude::*;

/// The player.
#[component]
pub struct Player {
    /// Speed.
    #[schema(default = 4.0)]
    pub speed: f32,
}

/// A camera that follows.
#[component]
pub struct FollowCamera {
    /// Where it sits, from the player.
    #[schema(default = Vec3::new(0.0, 3.0, 6.0))]
    pub offset: Vec3,
}

#[system]
fn walk(actions: Actions, time: Res<Time>, mut players: Query<(&Player, &mut Transform)>) {
    let go = actions.axis2("move");
    for (player, at) in players.iter_mut() {
        at.translation += Vec3::new(go.x, 0.0, -go.y) * player.speed * time.delta;
    }
}

#[system(after = "walk")]
fn follow(players: Query<&Transform, With<Player>>, mut cameras: Query<(&FollowCamera, &mut Transform)>) {
    let Some(target) = players.iter().next().map(|t| t.translation) else {
        return;
    };
    for (camera, at) in cameras.iter_mut() {
        at.translation = target + camera.offset;
    }
}

wish_sdk::export! {
    components: [Player, FollowCamera],
    systems: [walk, follow],
}
"#;

fn follow_reference(probe: &mut Probe) -> Result<(), String> {
    reference_code(probe, FOLLOW_CODE, OWN_TEST)?;
    edit(
        probe,
        json!([
            { "op": "set", "id": "player", "component": "Player", "value": {} },
            { "op": "set", "id": "camera", "component": "FollowCamera", "value": {} }
        ]),
    )
}

// --------------------------------------------------------------------------
// 27: a platform that goes back and forth (game code).

fn lift_setup(dir: &Path) -> Result<(), String> {
    setup_ops(
        dir,
        json!([{ "op": "spawn", "id": "lift", "components": {
            "Transform": { "translation": [0, 1, 0], "scale": [2, 0.5, 2] },
            "Primitive": { "shape": "Cube" }, "Collider": { "shape": "Box" },
            "RigidBody": { "kind": "Kinematic" } } }]),
    )
}

fn lift_check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    play_check(
        probe,
        dir,
        &hidden(json!([
            { "frames": 60 },
            { "expect": "Transform.translation.x > 2.4 && Transform.translation.x < 3.6",
              "entity": "lift", "message": "after one second the lift is about 3 m along +X" },
            { "frames": 60 },
            { "expect": "Transform.translation.x > 5.4 && Transform.translation.x < 6.2",
              "entity": "lift", "message": "after two seconds it is at the far end, 6 m" },
            { "frames": 120 },
            { "expect": "Transform.translation.x > -0.2 && Transform.translation.x < 0.6 && Transform.translation.y > 0.99 && Transform.translation.y < 1.01",
              "entity": "lift", "message": "after four seconds it is back, and has not changed height" }
        ])),
    )
}

const LIFT_CODE: &str = r#"
use wish_sdk::prelude::*;

/// Goes back and forth along X.
#[component]
pub struct Lift {
    /// Speed.
    #[schema(default = 3.0)]
    pub speed: f32,
}

#[system]
fn slide(time: Res<Time>, mut lifts: Query<(&Lift, &mut Transform)>) {
    for (lift, at) in lifts.iter_mut() {
        let d = (time.elapsed as f32 * lift.speed).rem_euclid(12.0);
        at.translation.x = if d < 6.0 { d } else { 12.0 - d };
    }
}

wish_sdk::export! {
    components: [Lift],
    systems: [slide],
}
"#;

fn lift_reference(probe: &mut Probe) -> Result<(), String> {
    reference_code(probe, LIFT_CODE, OWN_TEST)?;
    edit(
        probe,
        json!([{ "op": "set", "id": "lift", "component": "Lift", "value": {} }]),
    )
}

// --------------------------------------------------------------------------
// 28: respawn after falling (game code).

fn crate_setup(dir: &Path) -> Result<(), String> {
    setup_ops(
        dir,
        json!([{ "op": "spawn", "id": "crate", "components": {
            "Transform": { "translation": [0, 2, 0] }, "Primitive": { "shape": "Cube" },
            "RigidBody": { "kind": "Dynamic" }, "Collider": { "shape": "Box" }, "BodyVelocity": {} } }]),
    )
}

fn crate_check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    play_check(
        probe,
        dir,
        &hidden(json!([
            { "frames": 150 },
            { "expect": "Transform.translation.y > -5 && Transform.translation.y < 2.1",
              "entity": "crate", "message": "it fell past -10, was put back at (0, 2, 0) at rest, and has been falling again since (a crate that kept its speed would be far below by now)" },
            { "expect": "Transform.translation.x > -0.1 && Transform.translation.x < 0.1 && Transform.translation.z > -0.1 && Transform.translation.z < 0.1",
              "entity": "crate", "message": "it comes back above the same spot" },
            { "frames": 250 },
            { "expect": "Transform.translation.y > -10.5",
              "entity": "crate", "message": "it keeps being put back, every time it falls below -10" }
        ])),
    )
}

const CRATE_CODE: &str = r#"
use wish_sdk::prelude::*;

/// A thing that comes back after falling off the world.
#[component]
pub struct Faller {
    /// Where it comes back.
    #[schema(default = Vec3::new(0.0, 2.0, 0.0))]
    pub home: Vec3,
}

#[system]
fn bring_back(mut fallers: Query<(&Faller, &mut Transform, &mut BodyVelocity)>) {
    for (faller, at, velocity) in fallers.iter_mut() {
        if at.translation.y < -10.0 {
            at.translation = faller.home;
            velocity.linear = Vec3::ZERO;
            velocity.angular = Vec3::ZERO;
        }
    }
}

wish_sdk::export! {
    components: [Faller],
    systems: [bring_back],
}
"#;

fn crate_reference(probe: &mut Probe) -> Result<(), String> {
    reference_code(probe, CRATE_CODE, OWN_TEST)?;
    edit(
        probe,
        json!([{ "op": "set", "id": "crate", "component": "Faller", "value": {} }]),
    )
}

// --------------------------------------------------------------------------
// 29: pick up gems with sensors (game code).

fn gems_setup(dir: &Path) -> Result<(), String> {
    let gem = |id: &str, x: f64| {
        json!({ "op": "spawn", "id": id, "components": {
            "Transform": { "translation": [x, 1, 0], "scale": [0.4, 0.4, 0.4] },
            "Primitive": { "shape": "Sphere" },
            "Collider": { "shape": "Sphere", "size": [1.5, 1.5, 1.5], "sensor": true } } })
    };
    setup_ops(
        dir,
        json!([
            { "op": "spawn", "id": "ball", "components": {
                "Transform": { "translation": [0, 1, 0] }, "Primitive": { "shape": "Sphere" },
                "RigidBody": { "kind": "Dynamic", "gravity_scale": 0, "linear_damping": 0 },
                "Collider": { "shape": "Sphere" }, "BodyVelocity": { "linear": [3, 0, 0] } } },
            gem("gem_a", 3.0), gem("gem_b", 6.0), gem("gem_c", 9.0)
        ]),
    )
}

fn gems_check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    play_check(
        probe,
        dir,
        &hidden(json!([
            { "expect": "Score.points == 0 && count(Gem) == 3", "message": "three gems, no points at the start" },
            { "until": "Score.points >= 1", "max_frames": 200 },
            { "until": "Score.points >= 3", "max_frames": 400 },
            { "frames": 5 },
            { "expect": "Score.points == 3 && count(Gem) == 0", "message": "the ball rolled through all three: three points, no gem left" }
        ])),
    )
}

const GEMS_CODE: &str = r#"
use wish_sdk::prelude::*;

/// A gem to pick up.
#[component]
pub struct Gem {
    /// Points.
    #[schema(default = 1)]
    pub value: u32,
}

/// Points so far.
#[resource]
pub struct Score {
    /// Points.
    pub points: u32,
}

#[system]
fn pick_up(touches: EventReader<CollisionStarted>, gems: Query<(Entity, &Gem)>, mut score: ResMut<Score>, mut commands: Commands) {
    for touch in touches.read() {
        for (gem, worth) in gems.iter() {
            if gem == touch.a || gem == touch.b {
                score.points += worth.value;
                commands.despawn(gem);
            }
        }
    }
}

wish_sdk::export! {
    components: [Gem],
    resources: [Score],
    systems: [pick_up],
}
"#;

fn gems_reference(probe: &mut Probe) -> Result<(), String> {
    reference_code(
        probe,
        GEMS_CODE,
        r#"{ "format": 1, "scene": "scenes/main.scene.json", "steps": [
            { "until": "Score.points >= 1", "max_frames": 200 } ] }"#,
    )?;
    edit(
        probe,
        json!([
            { "op": "set", "id": "gem_a", "component": "Gem", "value": {} },
            { "op": "set", "id": "gem_b", "component": "Gem", "value": {} },
            { "op": "set", "id": "gem_c", "component": "Gem", "value": {} }
        ]),
    )
}

// --------------------------------------------------------------------------
// 30: a bell sound made with asset_create.

const BELL: &str = "assets/sounds/bell.wav";

fn bell_setup(dir: &Path) -> Result<(), String> {
    setup_ops(dir, json!([cube("bell", [0.0, 1.0, 0.0])]))
}

fn bell_check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    let bytes = std::fs::read(dir.join(BELL)).map_err(|_| format!("{BELL} does not exist"))?;
    let clip = wish_audio::decode(BELL, &bytes)
        .map_err(|p| format!("{BELL} cannot be decoded: {}", p.message))?;
    let seconds = f64::from(clip.seconds());
    if !(0.1..=0.4).contains(&seconds) {
        return Err(format!(
            "the sound is {seconds:.2} s long, not 0.1 to 0.4 s"
        ));
    }
    // The pitch, from the zero crossings (a whole number of half waves per second).
    let mut crossings = 0u32;
    for pair in clip.samples.windows(2) {
        if (pair[0] < 0.0) != (pair[1] < 0.0) {
            crossings += 1;
        }
    }
    let hertz = f64::from(crossings) / 2.0 / seconds;
    if hertz < 800.0 {
        return Err(format!(
            "the sound's pitch is about {hertz:.0} Hz, not above 800 Hz"
        ));
    }
    if !clip.samples.iter().any(|s| s.abs() > 0.1) {
        return Err("the sound is nearly silent".to_owned());
    }
    let s = start(probe)?;
    step(probe, &s, 30, json!({}))?;
    let logs = probe.call("logs_get", json!({ "session": s, "kinds": ["audio"] }))?;
    let started = logs["entries"].as_array().into_iter().flatten().any(|l| {
        l["code"] == "audio_started"
            && l["message"]
                .as_str()
                .is_some_and(|m| m.contains(BELL) && m.contains("'bell'"))
    });
    if started {
        Ok(())
    } else {
        Err("'bell' does not play the sound when the game starts".to_owned())
    }
}

fn bell_reference(probe: &mut Probe) -> Result<(), String> {
    probe.call(
        "asset_create",
        json!({ "path": BELL, "sound": { "wave": "sine", "frequency": 1200, "seconds": 0.2, "release": 0.1 } }),
    )?;
    edit(
        probe,
        json!([{ "op": "set", "id": "bell", "component": "AudioSource",
                 "value": { "clip": BELL, "playing": true, "looping": false } }]),
    )?;
    edit(
        probe,
        json!([{ "op": "set", "id": "camera", "component": "AudioListener", "value": {} }]),
    )
}

// --------------------------------------------------------------------------
// 31: an icon made with asset_create, shown in the interface.

const RING: &str = "assets/ui/ring.png";

fn icon_check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    let bytes = std::fs::read(dir.join(RING)).map_err(|_| format!("{RING} does not exist"))?;
    let picture = wish_import::load_png(&bytes, RING).map_err(|p| p.message.to_string())?;
    if !(32..=128).contains(&picture.width) || picture.width != picture.height {
        return Err(format!(
            "the picture is {}x{}, not square and 32 to 128 pixels",
            picture.width, picture.height
        ));
    }
    // A ring: transparent in the middle and in the corners, opaque on the ring.
    let alpha = |x: u32, y: u32| picture.rgba[((y * picture.width + x) * 4 + 3) as usize];
    let (w, h) = (picture.width, picture.height);
    if alpha(w / 2, h / 2) != 0 || alpha(0, 0) != 0 || alpha(w / 2, h * 9 / 100 + 1) == 0 {
        return Err(
            "the picture is not a ring (clear middle, clear corners, solid band)".to_owned(),
        );
    }
    let shown = interface(probe)?;
    let icon = shown
        .iter()
        .find(|e| e["entity"] == "icon" && e["image"] == true)
        .ok_or("'icon' does not show a picture")?;
    let [l, t, r, b] = bbox(icon);
    if l < 320.0 || t > 180.0 || r - l < 16.0 || b - t < 16.0 {
        return Err(format!(
            "the icon is at ({l}, {t}) to ({r}, {b}) of a 640x360 picture: it belongs in the top right and at least 16 px big"
        ));
    }
    let used = probe.components(None, "icon")?;
    if used["UiImage"]["image"] != RING {
        return Err(format!(
            "the icon shows {}, not {RING}",
            used["UiImage"]["image"]
        ));
    }
    Ok(())
}

fn icon_reference(probe: &mut Probe) -> Result<(), String> {
    probe.call(
        "asset_create",
        json!({ "path": RING, "picture": { "width": 64, "height": 64, "shape": "ring", "color": [1, 0.8, 0.2, 1] } }),
    )?;
    edit(
        probe,
        json!([{ "op": "spawn", "id": "icon", "components": {
            "UiNode": { "position": "Absolute", "offset": [1100, 24],
                "width": { "unit": "Px", "value": 96 }, "height": { "unit": "Px", "value": 96 } },
            "UiImage": { "image": RING } } }]),
    )
}

// --------------------------------------------------------------------------
// 32: read a resource of a running game.

const COUNTER_CODE: &str = r#"
use wish_sdk::prelude::*;

/// Ticks counted.
#[resource]
pub struct Counter {
    /// How many.
    pub ticks: u32,
}

#[system]
fn count(mut counter: ResMut<Counter>) {
    counter.ticks += 1;
}

wish_sdk::export! {
    resources: [Counter],
    systems: [count],
}
"#;

fn counter_setup(dir: &Path) -> Result<(), String> {
    write_file(dir, "scripts/src/lib.rs", COUNTER_CODE.as_bytes())?;
    let mut probe = Probe::open(dir)?;
    probe.call("script_build", json!({}))?;
    edit(
        &mut probe,
        json!([{ "op": "spawn", "id": "report", "components": { "Transform": {} } }]),
    )
}

fn counter_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    match probe.entity(None, "report")?["notes"]
        .as_str()
        .map(str::trim)
    {
        Some("90") => Ok(()),
        other => Err(format!(
            "'report' has notes {other:?}, not \"90\" (Counter.ticks after 90 ticks)"
        )),
    }
}

fn counter_reference(probe: &mut Probe) -> Result<(), String> {
    let s = start(probe)?;
    step(probe, &s, 90, json!({}))?;
    let got = probe.call(
        "entity_get",
        json!({ "session": s, "resources": ["Counter"] }),
    )?;
    let ticks = got["resources"]["Counter"]["ticks"]
        .as_u64()
        .ok_or("no Counter.ticks")?;
    edit(
        probe,
        json!([{ "op": "set_notes", "id": "report", "notes": ticks.to_string() }]),
    )
}

// --------------------------------------------------------------------------
// 33: a vertical menu (interface order).

fn menu_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let shown = interface(probe)?;
    let find_text = |text: &str| {
        shown
            .iter()
            .find(|e| e["text"].as_str().is_some_and(|t| t.trim() == text))
    };
    let (Some(title), Some(play), Some(quit)) =
        (find_text("Menü"), find_text("Oyna"), find_text("Çık"))
    else {
        return Err("the screen needs texts \"Menü\", \"Oyna\" and \"Çık\"".to_owned());
    };
    let (title, play, quit) = (bbox(title), bbox(play), bbox(quit));
    if !(title[1] < play[1] && play[1] < quit[1]) {
        return Err(format!(
            "the order from the top should be Menü, Oyna, Çık, but they are at y = {}, {}, {}",
            title[1], play[1], quit[1]
        ));
    }
    let buttons: Vec<[f64; 4]> = shown
        .iter()
        .filter(|e| !e["button"].is_null())
        .map(bbox)
        .collect();
    if buttons.len() != 2 {
        return Err(format!(
            "there should be two buttons, not {}",
            buttons.len()
        ));
    }
    for (label, name) in [(play, "Oyna"), (quit, "Çık")] {
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
    for b in &buttons {
        if ((b[0] + b[2]) / 2.0 - 320.0).abs() > 64.0 {
            return Err("the buttons are not in the middle (horizontally)".to_owned());
        }
    }
    Ok(())
}

fn menu_reference(probe: &mut Probe) -> Result<(), String> {
    let full = json!({ "unit": "Percent", "value": 100 });
    let label = |id: &str, parent: &str, text: &str, size: u32, order: u32| {
        json!({ "op": "spawn", "id": id, "parent": parent, "order": order,
                "components": { "UiNode": {}, "UiText": { "text": text, "size": size } } })
    };
    let button = |id: &str, order: u32| {
        json!({ "op": "spawn", "id": id, "parent": "menu", "order": order,
                "components": { "UiNode": { "padding": [12, 40, 12, 40], "background": [0.1, 0.3, 0.9, 1] }, "UiButton": {} } })
    };
    edit(
        probe,
        json!([
            { "op": "spawn", "id": "menu", "components": { "UiNode": {
                "width": full, "height": full, "justify": "Center", "align": "Center", "gap": 14 } } },
            label("menu_title", "menu", "Menü", 56, 0),
            button("play_button", 1), label("play_label", "play_button", "Oyna", 36, 0),
            button("quit_button", 2), label("quit_label", "quit_button", "Çık", 36, 0)
        ]),
    )
}

// --------------------------------------------------------------------------
// 34: a 2D bouncing ball.

fn bounce2d_setup(dir: &Path) -> Result<(), String> {
    setup_ops(
        dir,
        json!([
            { "op": "despawn", "id": "ground" },
            { "op": "set", "id": "camera", "component": "Camera", "value": { "projection": "Orthographic", "ortho_height": 10 } },
            { "op": "replace", "id": "camera", "component": "Transform", "value": { "translation": [0, 3, 10] } },
            { "op": "spawn", "id": "floor", "components": {
                "Transform": { "translation": [0, 0, 0] }, "Sprite": { "rect": [0, 0, 1000, 50], "color": [0.3, 0.6, 0.3, 1] } } },
            { "op": "spawn", "id": "ball", "components": {
                "Transform": { "translation": [0, 4, 0] }, "Sprite": { "rect": [0, 0, 50, 50], "color": [1, 0.4, 0.2, 1] } } }
        ]),
    )
}

fn bounce2d_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let s = start(probe)?;
    let mut touched = false;
    let mut after = f64::MIN;
    for _ in 0..70 {
        step(probe, &s, 3, json!({}))?;
        let at = live_pos(probe, &s, "ball")?;
        if at[2].abs() > 0.01 {
            return Err(format!("the ball left the 2D plane (z = {:.3})", at[2]));
        }
        if at[1] < 0.8 {
            touched = true;
        } else if touched {
            after = after.max(at[1]);
        }
    }
    if !touched {
        return Err("the ball never reaches the floor".to_owned());
    }
    if after < 1.3 {
        return Err(format!(
            "after hitting the floor the ball rises only to y = {after:.2}"
        ));
    }
    Ok(())
}

fn bounce2d_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([
            { "op": "set", "id": "floor", "component": "Collider", "value": { "shape": "Box", "size": [10, 0.5, 1] } },
            { "op": "set", "id": "ball", "component": "RigidBody", "value": {
                "kind": "Dynamic", "lock_translation": [false, false, true], "lock_rotation": [true, true, false] } },
            { "op": "set", "id": "ball", "component": "Collider", "value": { "shape": "Sphere", "size": [0.5, 0.5, 0.5], "restitution": 0.8 } }
        ]),
    )
}

// --------------------------------------------------------------------------
// 35: a spot light over an actor.

fn stage_setup(dir: &Path) -> Result<(), String> {
    setup_ops(
        dir,
        json!([
            { "op": "despawn", "id": "sun" },
            { "op": "spawn", "id": "actor", "components": {
                "Transform": { "translation": [0, 0.9, 0], "scale": [0.6, 1.8, 0.6] }, "Primitive": { "shape": "Cube" } } }
        ]),
    )
}

fn stage_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let lights = probe.query(None, "has(SpotLight)")?;
    if lights.is_empty() {
        return Err("there is no SpotLight".to_owned());
    }
    let mut reasons = Vec::new();
    for id in &lights {
        let m = probe.matrix(None, id)?;
        let at = [m[12], m[13], m[14]];
        // The light shines along its -Z axis.
        let dir = [-m[8], -m[9], -m[10]];
        let length = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        let down = if length > 0.0 { -dir[1] / length } else { 0.0 };
        let spot = probe.components(None, id)?["SpotLight"].clone();
        let outer = spot["outer_angle_deg"].as_f64().unwrap_or(35.0);
        let inner = spot["inner_angle_deg"].as_f64().unwrap_or(25.0);
        let range = spot["range"].as_f64().unwrap_or(10.0);
        let distance = ((at[0]).powi(2) + (at[1] - 0.9).powi(2) + (at[2]).powi(2)).sqrt();
        if at[0].abs() > 1.5 || at[2].abs() > 1.5 || !(2.5..=8.0).contains(&at[1]) {
            reasons.push(format!("'{id}' is not above the actor (at {at:?}; it should be 2.5 to 8 m up, within 1.5 m sideways)"));
        } else if down < 0.9 {
            reasons.push(format!(
                "'{id}' does not shine downward (down = {down:.2}); a light looks along its -Z axis"
            ));
        } else if outer > 45.0 || inner >= outer {
            reasons.push(format!("'{id}' has a cone of {inner} to {outer} degrees: a stage light is at most 45 and inner must be less than outer"));
        } else if range < distance {
            reasons.push(format!(
                "'{id}' reaches {range} m but the actor is {distance:.1} m away"
            ));
        } else {
            return Ok(());
        }
    }
    Err(reasons.join("; "))
}

fn stage_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "spawn", "id": "spot", "components": {
            "Transform": { "translation": [0, 5, 0], "euler_deg": [-90, 0, 0] },
            "SpotLight": { "intensity_lm": 3000, "range": 12, "inner_angle_deg": 15, "outer_angle_deg": 30 } } }]),
    )
}

// --------------------------------------------------------------------------
// 36: play a clip once.

fn once_setup(dir: &Path) -> Result<(), String> {
    write_file(
        dir,
        "assets/models/bar.gltf",
        wish_import::samples::bending_bar().as_bytes(),
    )?;
    setup_ops(
        dir,
        json!([{ "op": "spawn", "id": "bar", "components": {
            "Transform": {}, "Model": { "asset": "assets/models/bar.gltf" } } }]),
    )
}

fn once_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let s = start(probe)?;
    let ran = step(probe, &s, 120, json!({}))?;
    let player = live(probe, &s, "bar")?["AnimationPlayer"].clone();
    if player.is_null() {
        return Err("'bar' has no AnimationPlayer".to_owned());
    }
    if player["clip"] != "bend" {
        return Err(format!("'bar' plays {}, not \"bend\"", player["clip"]));
    }
    if player["playing"] != false || (num(&player["time"]) - 1.0).abs() > 1e-3 {
        return Err(format!(
            "after 2 s the 1 s clip should have stopped at its end (time 1.0, playing false), but time is {} and playing is {}",
            player["time"], player["playing"]
        ));
    }
    if ran["events"]["AnimationFinished"].as_u64() != Some(1) {
        return Err(format!(
            "AnimationFinished was sent {} time(s), not once",
            ran["events"]["AnimationFinished"]
        ));
    }
    Ok(())
}

fn once_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "set", "id": "bar", "component": "AnimationPlayer",
                 "value": { "clip": "bend", "looping": false, "playing": true } }]),
    )
}

// --------------------------------------------------------------------------
// 37: collision layers and masks.

fn layers_setup(dir: &Path) -> Result<(), String> {
    let ball = |id: &str, z: f64| {
        json!({ "op": "spawn", "id": id, "components": {
            "Transform": { "translation": [0, 1, z] }, "Primitive": { "shape": "Sphere" },
            "RigidBody": { "kind": "Dynamic", "gravity_scale": 0, "linear_damping": 0 },
            "Collider": { "shape": "Sphere", "restitution": 0 }, "BodyVelocity": { "linear": [4, 0, 0] } } })
    };
    setup_ops(
        dir,
        json!([
            { "op": "spawn", "id": "wall", "components": {
                "Transform": { "translation": [5, 1, 0], "scale": [1, 4, 6] }, "Primitive": { "shape": "Cube" },
                "Collider": { "shape": "Box" } } },
            ball("ghost", -1.5), ball("solid", 1.5)
        ]),
    )
}

fn layers_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let wall = probe.components(None, "wall")?;
    if wall["Collider"]["sensor"] == true
        || wall.get("RigidBody").is_some_and(|b| b["kind"] != "Fixed")
    {
        return Err("'wall' must stay a solid, fixed wall".to_owned());
    }
    let s = start(probe)?;
    step(probe, &s, 180, json!({}))?;
    let ghost = live_pos(probe, &s, "ghost")?;
    let solid = live_pos(probe, &s, "solid")?;
    if ghost[0] < 8.0 {
        return Err(format!(
            "'ghost' did not pass through the wall (x = {:.2}, the wall is at x = 5)",
            ghost[0]
        ));
    }
    if solid[0] > 4.6 {
        return Err(format!(
            "'solid' went through the wall (x = {:.2}): it must still be stopped",
            solid[0]
        ));
    }
    Ok(())
}

fn layers_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([
            { "op": "set", "id": "wall", "component": "Collider", "value": { "layer": 2 } },
            { "op": "set", "id": "ghost", "component": "Collider", "value": { "mask": 4294967293u32 } }
        ]),
    )
}

// --------------------------------------------------------------------------
// 38: jump with an impulse (game code).

fn jumper_setup(dir: &Path) -> Result<(), String> {
    setup_ops(
        dir,
        json!([
            solid_ground(),
            { "op": "spawn", "id": "box", "components": {
                "Transform": { "translation": [0, 0.5, 0] }, "Primitive": { "shape": "Cube" },
                "RigidBody": { "kind": "Dynamic" }, "Collider": { "shape": "Box" } } }
        ]),
    )
}

fn jumper_check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    play_check(
        probe,
        dir,
        &hidden(json!([
            { "frames": 60 },
            { "expect": "Transform.translation.y > 0.4 && Transform.translation.y < 0.6",
              "entity": "box", "message": "the box rests on the ground" },
            { "input": { "jump": true }, "frames": 1 },
            { "frames": 20 },
            { "expect": "Transform.translation.y > 1.3", "entity": "box",
              "message": "jumping kicks the box up by more than 0.8 m" },
            { "frames": 120 },
            { "expect": "Transform.translation.y > 0.4 && Transform.translation.y < 0.6",
              "entity": "box", "message": "it lands and rests again" }
        ])),
    )
}

const JUMPER_CODE: &str = r#"
use wish_sdk::prelude::*;

/// Jumps when the jump action is pressed.
#[component]
pub struct Jumper {
    /// Change of momentum of a jump.
    #[schema(unit = "N*s", default = 6000.0)]
    pub impulse: f32,
}

#[system]
fn jump(actions: Actions, jumpers: Query<(Entity, &Jumper)>, mut kicks: EventWriter<BodyImpulse>) {
    if !actions.pressed("jump") {
        return;
    }
    for (entity, jumper) in jumpers.iter() {
        kicks.send(BodyImpulse {
            entity,
            linear: Vec3::new(0.0, jumper.impulse, 0.0),
            angular: Vec3::ZERO,
        });
    }
}

wish_sdk::export! {
    components: [Jumper],
    systems: [jump],
}
"#;

fn jumper_reference(probe: &mut Probe) -> Result<(), String> {
    reference_code(probe, JUMPER_CODE, OWN_TEST)?;
    edit(
        probe,
        json!([{ "op": "set", "id": "box", "component": "Jumper", "value": {} }]),
    )
}

// --------------------------------------------------------------------------
// 39: a seconds counter written as text (game code).

fn timer_setup(dir: &Path) -> Result<(), String> {
    setup_ops(
        dir,
        json!([{ "op": "spawn", "id": "timer", "components": {
            "UiNode": { "position": "Absolute", "offset": [24, 24] },
            "UiText": { "text": "Süre: 0", "size": 40 } } }]),
    )
}

fn timer_check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    play_check(
        probe,
        dir,
        &hidden(json!([
            { "frames": 10 },
            { "expect": "UiText.text == \"Süre: 0\"", "entity": "timer", "message": "it starts at 0" },
            { "frames": 120 },
            { "expect": "UiText.text == \"Süre: 2\"", "entity": "timer", "message": "after about 2.2 s it says 2" },
            { "frames": 70 },
            { "expect": "UiText.text == \"Süre: 3\"", "entity": "timer", "message": "after about 3.3 s it says 3" }
        ])),
    )
}

const TIMER_CODE: &str = r#"
use wish_sdk::prelude::*;

/// The text that counts seconds.
#[component]
pub struct TimerLabel {
    /// The second it shows now.
    pub shown: u32,
}

#[system]
fn tick(time: Res<Time>, mut labels: Query<(&mut TimerLabel, &mut UiText)>) {
    let seconds = time.elapsed as u32;
    for (label, words) in labels.iter_mut() {
        if label.shown != seconds {
            label.shown = seconds;
            words.text = text(&format!("Süre: {seconds}"));
        }
    }
}

wish_sdk::export! {
    components: [TimerLabel],
    systems: [tick],
}
"#;

fn timer_reference(probe: &mut Probe) -> Result<(), String> {
    reference_code(probe, TIMER_CODE, OWN_TEST)?;
    edit(
        probe,
        json!([{ "op": "set", "id": "timer", "component": "TimerLabel", "value": {} }]),
    )
}

// --------------------------------------------------------------------------
// 40: a second level.

fn level_baseline(probe: &mut Probe) -> Result<Value, String> {
    let mut main = probe.call("scene_get", json!({ "detail": "normal" }))?;
    if let Some(object) = main.as_object_mut() {
        object.remove("revision");
        object.remove("diagnostics");
    }
    Ok(main)
}

fn level_check(probe: &mut Probe, _: &Path, baseline: &Value) -> Verdict {
    let level = "scenes/level2.scene.json";
    let ids = probe.query(Some(level), "has(Transform)")?;
    for id in ["camera", "ground", "sun", "bonus"] {
        if !ids.iter().any(|i| i == id) {
            return Err(format!(
                "{level} has no '{id}' (it needs the main scene's entities and 'bonus')"
            ));
        }
    }
    let bonus = probe.components(Some(level), "bonus")?;
    let colour = &bonus["Material"]["base_color"];
    let (r, g, b) = (num(&colour[0]), num(&colour[1]), num(&colour[2]));
    if bonus["Primitive"]["shape"] != "Sphere" || !(r > 0.7 && g > 0.5 && b < 0.4) {
        return Err("'bonus' should be a golden (yellow-orange) sphere".to_owned());
    }
    let now = level_baseline(probe)?;
    if &now != baseline {
        return Err("the main scene was changed".to_owned());
    }
    Ok(())
}

fn level_reference(probe: &mut Probe) -> Result<(), String> {
    let camera = probe.components(None, "camera")?;
    let ground = probe.components(None, "ground")?;
    let sun = probe.components(None, "sun")?;
    probe.call(
        "edit_apply",
        json!({ "scene": "scenes/level2.scene.json", "create": true, "ops": [
            { "op": "spawn", "id": "camera", "components": { "Camera": camera["Camera"], "Transform": camera["Transform"] } },
            { "op": "spawn", "id": "ground", "components": { "Primitive": ground["Primitive"], "Material": ground["Material"], "Transform": ground["Transform"] } },
            { "op": "spawn", "id": "sun", "components": { "DirectionalLight": sun["DirectionalLight"], "Transform": sun["Transform"] } },
            { "op": "spawn", "id": "bonus", "components": {
                "Primitive": { "shape": "Sphere" }, "Material": { "base_color": [1, 0.8, 0.2, 1] },
                "Transform": { "translation": [0, 0.5, 0] } } }
        ] }),
    )
    .map(|_| ())
}

// --------------------------------------------------------------------------
// 41: record and save a replay.

fn replay_check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    let text = std::fs::read_to_string(dir.join("replays/jump.replay.jsonl"))
        .map_err(|_| "there is no replay named 'jump' (replays/jump.replay.jsonl)".to_owned())?;
    let header: Value = text
        .lines()
        .next()
        .and_then(|l| serde_json::from_str(l).ok())
        .ok_or("the replay file has no header")?;
    let ticks = header["ticks"].as_u64().unwrap_or(0);
    if ticks < 60 {
        return Err(format!("the replay has {ticks} ticks, not at least 60"));
    }
    if !text.contains("\"jump\"") {
        return Err("the replay does not contain the jump input".to_owned());
    }
    let ran = probe.call("replay", json!({ "action": "run", "name": "jump" }))?;
    if ran["passed"] == true {
        Ok(())
    } else {
        Err(format!("the replay does not play back: {ran}"))
    }
}

fn replay_reference(probe: &mut Probe) -> Result<(), String> {
    let s = start(probe)?;
    probe.call("replay", json!({ "action": "record", "session": s }))?;
    step(probe, &s, 20, json!({}))?;
    step(probe, &s, 1, json!({ "jump": true }))?;
    step(probe, &s, 60, json!({}))?;
    probe
        .call(
            "replay",
            json!({ "action": "save", "session": s, "name": "jump" }),
        )
        .map(|_| ())
}

/// The showcase tasks, 26 to 41.
pub fn all() -> Vec<Task> {
    let task = |id,
                prompt,
                setup: fn(&Path) -> Result<(), String>,
                baseline: fn(&mut Probe) -> Result<Value, String>,
                check,
                reference,
                files| Task {
        id,
        prompt,
        setup,
        baseline,
        check,
        reference,
        files,
    };
    let plain: fn(&Path) -> Result<(), String> = |_| Ok(());
    vec![
        task(
            "26_follow_camera",
            "'player' adlı küpü oyuncu yürütebilsin (input.json'daki 'move': x sağa +X, y ileri -Z, saniyede 4 metre) ve 'camera' adlı kamera oyuncuyu arkadan ve yukarıdan izlesin (oyuncu hareket ettikçe kamera da gelsin). Oyun kodunu (scripts/src/lib.rs) yaz, script_build ile derle ve davranışı tests/ altına yazacağın bir senaryo testiyle kanıtla.",
            follow_setup,
            nothing,
            follow_check,
            follow_reference,
            true,
        ),
        task(
            "27_moving_platform",
            "'lift' adlı platform oyun başlayınca x ekseninde (+X) saniyede 3 metre hızla 6 metre gidip aynı hızla başa dönsün ve bunu sürekli tekrarlasın (y'si değişmesin). Oyun kodunu (scripts/src/lib.rs) yaz, script_build ile derle ve davranışı tests/ altına yazacağın bir senaryo testiyle kanıtla.",
            lift_setup,
            nothing,
            lift_check,
            lift_reference,
            true,
        ),
        task(
            "28_respawn_fall",
            "'crate' adlı sandık zeminsiz boşlukta düşüyor. Oyun kodu (scripts/src/lib.rs) yaz: y değeri -10'un altına inince sandık (0, 2, 0) noktasına geri konsun ve hızı sıfırlansın; bu her düşüşte tekrarlansın. script_build ile derle ve davranışı tests/ altına yazacağın bir senaryo testiyle kanıtla.",
            crate_setup,
            nothing,
            crate_check,
            crate_reference,
            true,
        ),
        task(
            "29_gem_pickup",
            "'ball' topu +X yönünde yuvarlanıyor; yolunda 'gem_a', 'gem_b', 'gem_c' adlı üç algılayıcı (sensör) mücevher var. Oyun kodunda (scripts/src/lib.rs) Gem bileşeni ve Score kaynağı (points alanı) olsun: top bir mücevhere değince mücevher yok olsun ve Score.points bir artsın. Sahnedeki üç mücevhere Gem bileşenini ekle. script_build ile derle ve davranışı tests/ altına yazacağın bir senaryo testiyle kanıtla.",
            gems_setup,
            nothing,
            gems_check,
            gems_reference,
            true,
        ),
        task(
            "30_bell_sound",
            "assets/sounds/bell.wav adında 0,1 ile 0,4 saniye uzunlukta, yüksek perdeli (800 Hz'in üstünde) bir zil sesi üret (indirme yok; motorun aracıyla yap). 'bell' adlı varlık oyun başlar başlamaz bu sesi çalsın; kameraya da bir dinleyici ekle.",
            bell_setup,
            nothing,
            bell_check,
            bell_reference,
            false,
        ),
        task(
            "31_ring_icon",
            "assets/ui/ring.png adında 64x64 bir halka resmi üret (ortası ve köşeleri şeffaf; indirme yok, motorun aracıyla yap) ve onu 'icon' adlı bir arayüz öğesinde göster: ekranın sağ üst köşesinde, en az 16 piksel büyüklükte olsun.",
            plain,
            nothing,
            icon_check,
            icon_reference,
            false,
        ),
        task(
            "32_read_counter",
            "Oyun kodunda Counter kaynağı (ticks alanı) her tick bir artıyor. Simülasyonu 90 tick çalıştır, Counter.ticks değerini oku ve sahnedeki 'report' varlığına not olarak yalnızca o sayıyı yaz.",
            counter_setup,
            nothing,
            counter_check,
            counter_reference,
            false,
        ),
        task(
            "33_vertical_menu",
            "Oyun içi arayüzle ekranın ortasında dikey bir menü kur: en üstte 'Menü' başlığı, altında sırayla 'Oyna' ve 'Çık' yazılı iki düğme (yazılar düğmelerin içinde olsun, düğmeler yatayda ortada dursun).",
            plain,
            nothing,
            menu_check,
            menu_reference,
            false,
        ),
        task(
            "34_bouncing_ball_2d",
            "Sahne 2B: ortografik kamera, 'floor' ve 'ball' adlı sprite'lar var ama fizik yok. Fizik ekle: 'ball' yerçekimiyle düşüp 'floor'a çarpsın ve zıplasın (sekmeli), 2B düzlemde kalsın (z değişmesin).",
            bounce2d_setup,
            nothing,
            bounce2d_check,
            bounce2d_reference,
            false,
        ),
        task(
            "35_stage_spot",
            "'actor' adlı nesnenin tepesinden aşağı doğru bir sahne ışığı (SpotLight) koy: nesnenin 2,5–8 metre üstünde (yatayda en çok 1,5 m kayık), aşağıya baksın, koni açısı en çok 45 derece olsun (iç açı dış açıdan küçük) ve nesneye erişsin.",
            stage_setup,
            nothing,
            stage_check,
            stage_reference,
            false,
        ),
        task(
            "36_animate_once",
            "'bar' modelinin 'bend' animasyonunu oyun başlayınca bir kez oynat: bitince sonda dursun (döngü yok).",
            once_setup,
            nothing,
            once_check,
            once_reference,
            false,
        ),
        task(
            "37_ghost_layers",
            "'ghost' ve 'solid' adlı toplar +X yönünde 'wall' duvarına doğru gidiyor. 'ghost' duvardan içinden geçsin ama 'solid' duvara çarpıp dursun; duvar sağlam ve sabit kalsın, topların hızlarını ve konumlarını değiştirme. (Çarpışma katmanı ve maskeleri kullan.)",
            layers_setup,
            nothing,
            layers_check,
            layers_reference,
            false,
        ),
        task(
            "38_impulse_jump",
            "'box' adlı dinamik kutu zeminde duruyor. Oyun kodunda (scripts/src/lib.rs) 'jump' eylemi basılınca kutuya yukarı bir itme verilsin (Jumper bileşeni ile işaretle): kutu 0,8 metreden fazla zıplasın. script_build ile derle ve davranışı tests/ altına yazacağın bir senaryo testiyle kanıtla.",
            jumper_setup,
            nothing,
            jumper_check,
            jumper_reference,
            true,
        ),
        task(
            "39_seconds_label",
            "'timer' adlı arayüz yazısı 'Süre: 0' diyor. Oyun kodunda (scripts/src/lib.rs) bu yazı geçen tam saniyeyi göstersin: 'Süre: 1', 'Süre: 2', ... script_build ile derle ve davranışı tests/ altına yazacağın bir senaryo testiyle kanıtla.",
            timer_setup,
            nothing,
            timer_check,
            timer_reference,
            true,
        ),
        task(
            "40_second_level",
            "scenes/level2.scene.json adında ikinci bir sahne oluştur: ana sahnenin tüm varlıkları (aynı kimliklerle) olsun, bir de 'bonus' adlı altın sarısı bir küre eklensin. Ana sahneyi değiştirme.",
            plain,
            level_baseline,
            level_check,
            level_reference,
            false,
        ),
        task(
            "41_save_replay",
            "Oyunu başlat, tekrar kaydını aç, 20 tick bekle, 'jump' girdisini bir tick ver, 60 tick daha bekle ve kaydı 'jump' adıyla kaydet.",
            plain,
            nothing,
            replay_check,
            replay_reference,
            false,
        ),
    ]
}
