//! Tasks of the game systems (M6): physics, sound, lights, animation, the
//! in-game interface, 2D and particles. Each is checked by what the engine
//! does (a session run, describe_view, the logs), not by how it was set up.

use std::path::Path;

use serde_json::{Value, json};

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

/// An entity's components in a session.
fn live(probe: &mut Probe, session: &str, id: &str) -> Result<Value, String> {
    let got = probe.call("entity_get", json!({ "session": session, "ids": [id] }))?;
    Ok(got["entities"][0]["components"].clone())
}

fn num(value: &Value) -> f64 {
    value.as_f64().unwrap_or(f64::NAN)
}

fn live_y(probe: &mut Probe, session: &str, id: &str) -> Result<f64, String> {
    Ok(num(
        &live(probe, session, id)?["Transform"]["translation"][1]
    ))
}

/// The ground of a new project made solid (its plane is 30 × 30 m).
fn solid_ground() -> Value {
    json!({ "op": "set", "id": "ground", "component": "Collider",
            "value": { "shape": "Box", "size": [1, 0.1, 1], "offset": [0, -0.05, 0] } })
}

/// A mono 16-bit WAV of a quiet tone, `seconds` long.
fn wav(seconds: f32) -> Vec<u8> {
    let rate = 22_050u32;
    let samples = (seconds * rate as f32) as u32;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(samples * 2).to_le_bytes());
    for i in 0..samples {
        let t = i as f32 / rate as f32;
        let value = (t * 220.0 * std::f32::consts::TAU).sin() * 6000.0;
        out.extend_from_slice(&(value as i16).to_le_bytes());
    }
    out
}

/// A 16 × 16 PNG: a red figure on transparency.
fn hero_png() -> Result<Vec<u8>, String> {
    let mut rgba = Vec::new();
    for y in 0..16 {
        for x in 0..16 {
            let inside = (3..13).contains(&x) && (2..15).contains(&y);
            rgba.extend_from_slice(&if inside {
                [220, 40, 40, 255]
            } else {
                [0, 0, 0, 0]
            });
        }
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 16, 16);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// The interface elements `describe_view` lists (scene or session).
fn interface(probe: &mut Probe, session: Option<&str>) -> Result<Vec<Value>, String> {
    let mut args = json!({ "width": 640, "height": 360 });
    if let Some(session) = session {
        args["session"] = json!(session);
    }
    let seen = probe.call("describe_view", args)?;
    Ok(seen["interface"].as_array().cloned().unwrap_or_default())
}

fn bbox(element: &Value) -> [f64; 4] {
    let b = &element["bounding_box"];
    [num(&b[0]), num(&b[1]), num(&b[2]), num(&b[3])]
}

// --------------------------------------------------------------------------
// 13: a box that falls and rests.

fn drop_setup(dir: &Path) -> Result<(), String> {
    setup_ops(
        dir,
        json!([{ "op": "spawn", "id": "box", "components": {
            "Transform": { "translation": [0, 3, 0] },
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.8, 0.5, 0.2, 1] } } }]),
    )
}

fn drop_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let start_y = probe.position(None, "box")?[1];
    if start_y < 2.5 {
        return Err(format!(
            "'box' was moved down in the scene (y = {start_y:.2}); it should start in the air and fall"
        ));
    }
    let s = start(probe)?;
    step(probe, &s, 30, json!({}))?;
    let falling = live_y(probe, &s, "box")?;
    if falling.is_nan() || falling > start_y - 0.2 {
        return Err(format!(
            "'box' does not fall: after half a second y = {falling:.2}"
        ));
    }
    step(probe, &s, 210, json!({}))?;
    let rest = live_y(probe, &s, "box")?;
    if !(0.4..=0.62).contains(&rest) {
        return Err(format!(
            "after four seconds 'box' is at y = {rest:.2}, not resting on the ground (about 0.5)"
        ));
    }
    Ok(())
}

fn drop_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([
            solid_ground(),
            { "op": "set", "id": "box", "component": "RigidBody", "value": { "kind": "Dynamic" } },
            { "op": "set", "id": "box", "component": "Collider", "value": { "shape": "Box" } }
        ]),
    )
}

// --------------------------------------------------------------------------
// 14: a ball that bounces.

fn ball_ops(x: f64, y: f64) -> Value {
    json!({ "op": "spawn", "id": "ball", "components": {
        "Transform": { "translation": [x, y, 0] },
        "Primitive": { "shape": "Sphere" },
        "Material": { "base_color": [0.9, 0.2, 0.2, 1] },
        "RigidBody": { "kind": "Dynamic" },
        "Collider": { "shape": "Sphere" } } })
}

fn bounce_setup(dir: &Path) -> Result<(), String> {
    setup_ops(dir, json!([solid_ground(), ball_ops(0.0, 4.0)]))
}

fn bounce_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    if probe.position(None, "ball")?[1] < 3.5 {
        return Err("'ball' should still start 4 m up".to_owned());
    }
    let s = start(probe)?;
    let mut touched = false;
    let mut highest = f64::MIN;
    for _ in 0..150 {
        step(probe, &s, 2, json!({}))?;
        let y = live_y(probe, &s, "ball")?;
        if y < 0.65 {
            touched = true;
        } else if touched {
            highest = highest.max(y);
        }
    }
    if !touched {
        return Err("'ball' never reaches the ground".to_owned());
    }
    if highest < 1.5 {
        return Err(format!(
            "after hitting the ground 'ball' rises only to y = {highest:.2}; its bottom should rise at least 1 m (centre 1.5)"
        ));
    }
    Ok(())
}

fn bounce_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "set", "id": "ball", "component": "Collider", "value": { "restitution": 0.9 } }]),
    )
}

// --------------------------------------------------------------------------
// 15: a trigger zone the ball falls through.

fn trigger_setup(dir: &Path) -> Result<(), String> {
    setup_ops(dir, json!([solid_ground(), ball_ops(3.0, 5.0)]))
}

fn trigger_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let goal = probe.components(None, "goal")?;
    if goal["Collider"]["sensor"] != true {
        return Err("'goal' has no Collider with sensor true".to_owned());
    }
    let at = probe.position(None, "goal")?;
    if (at[0] - 3.0).abs() > 1.5 || at[2].abs() > 1.5 {
        return Err(format!(
            "'goal' is at {at:?}, not where the ball falls (x = 3, z = 0)"
        ));
    }
    let s = start(probe)?;
    let ran = step(probe, &s, 180, json!({}))?;
    let started = ran["events"]["CollisionStarted"].as_u64().unwrap_or(0);
    let y = live_y(probe, &s, "ball")?;
    if y > 0.6 {
        return Err(format!(
            "the ball stops at y = {y:.2}: the goal is not passed through"
        ));
    }
    if started < 2 {
        return Err(format!(
            "only {started} CollisionStarted event(s): the ball did not enter the goal on its way to the ground"
        ));
    }
    Ok(())
}

fn trigger_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "spawn", "id": "goal", "components": {
            "Transform": { "translation": [3, 0.6, 0] },
            "Collider": { "shape": "Box", "size": [2, 1, 2], "sensor": true } } }]),
    )
}

// --------------------------------------------------------------------------
// 16: game code makes a character walk and jump.

const JUMP_CODE: &str = r#"
use wish_sdk::prelude::*;

#[system]
fn run(actions: Actions, time: Res<Time>, mut characters: Query<(&mut CharacterMotion, &CharacterState)>) {
    let go = actions.axis2("move");
    for (motion, state) in characters.iter_mut() {
        let mut vy = motion.velocity.y - 9.81 * time.delta;
        if state.grounded {
            vy = if actions.pressed("jump") { 5.0 } else { -1.0 };
        }
        motion.velocity = Vec3::new(go.x * 3.0, vy, -go.y * 3.0);
    }
}

wish_sdk::export! {
    systems: [run],
}
"#;

fn jump_setup(dir: &Path) -> Result<(), String> {
    setup_ops(
        dir,
        json!([solid_ground(), { "op": "spawn", "id": "player", "components": {
            "Transform": { "translation": [0, 0.95, 0] },
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.2, 0.4, 0.9, 1] },
            "RigidBody": { "kind": "Kinematic" },
            "Collider": { "shape": "Capsule", "size": [0.6, 1.8, 0.6] },
            "CharacterController": {}, "CharacterMotion": {}, "CharacterState": {} } }]),
    )
}

fn jump_check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    let hidden = json!({ "format": 1, "scene": "scenes/main.scene.json", "seed": 1, "steps": [
        { "frames": 20 },
        { "expect": "CharacterState.grounded == true", "entity": "player",
          "message": "the player stands on the ground" },
        { "input": { "move": [1, 0] }, "frames": 60 },
        { "expect": "Transform.translation.x > 2.4 && Transform.translation.x < 3.6", "entity": "player",
          "message": "a second to the right walks about 3 m along +X" },
        { "input": { "jump": true }, "frames": 1 },
        { "frames": 15 },
        { "expect": "Transform.translation.y > 1.6", "entity": "player",
          "message": "jumping lifts the player" },
        { "frames": 150 },
        { "expect": "CharacterState.grounded == true && Transform.translation.y < 1.1", "entity": "player",
          "message": "the player lands again" }
    ] });
    crate::game_tasks::play_check(probe, dir, &hidden)
}

fn jump_reference(probe: &mut Probe) -> Result<(), String> {
    let dir = probe.root();
    write_file(&dir, "scripts/src/lib.rs", JUMP_CODE.as_bytes())?;
    probe.call("script_build", json!({}))?;
    write_file(
        &dir,
        "tests/jump.test.json",
        br#"{ "format": 1, "scene": "scenes/main.scene.json", "steps": [
            { "frames": 20 },
            { "input": { "jump": true }, "frames": 10 },
            { "expect": "Transform.translation.y > 1.3", "entity": "player" } ] }"#,
    )
}

// --------------------------------------------------------------------------
// 17: looping music and a listener.

const MUSIC: &str = "assets/sounds/music.wav";

fn music_setup(dir: &Path) -> Result<(), String> {
    write_file(dir, MUSIC, &wav(2.0))
}

fn music_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let sources = probe.query(None, &format!("AudioSource.clip == \"{MUSIC}\""))?;
    if sources.is_empty() {
        return Err(format!("no AudioSource plays {MUSIC}"));
    }
    let listeners = probe.query(None, "has(AudioListener) && has(Camera)")?;
    if listeners.is_empty() {
        return Err("the camera has no AudioListener".to_owned());
    }
    let s = start(probe)?;
    step(probe, &s, 300, json!({}))?;
    let logs = probe.call("logs_get", json!({ "session": s, "kinds": ["audio"] }))?;
    let lines: Vec<&Value> = logs["entries"]
        .as_array()
        .map(|e| e.iter().collect())
        .unwrap_or_default();
    let mentions = |code: &str| {
        lines
            .iter()
            .filter(|l| {
                l["code"] == code && l["message"].as_str().is_some_and(|m| m.contains(MUSIC))
            })
            .count()
    };
    if mentions("audio_started") == 0 {
        return Err("the music does not start when the game starts".to_owned());
    }
    if mentions("audio_ended") > 0 {
        return Err("the music ends after two seconds: it does not loop".to_owned());
    }
    Ok(())
}

fn music_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([
            { "op": "spawn", "id": "music", "components": {
                "AudioSource": { "clip": MUSIC, "looping": true, "playing": true, "spatial": false } } },
            { "op": "set", "id": "camera", "component": "AudioListener", "value": {} }
        ]),
    )
}

// --------------------------------------------------------------------------
// 18: a lamp over a table at night.

fn lamp_setup(dir: &Path) -> Result<(), String> {
    setup_ops(
        dir,
        json!([
            { "op": "despawn", "id": "sun" },
            { "op": "set_resource", "name": "Sky", "value": { "luminance": 0 } },
            // No fill light at all: a flat ambient light of 0 (the sky,
            // which lights a scene by default, is dark too).
            { "op": "set_resource", "name": "Ambient", "value": { "source": "Color", "intensity": 0 } },
            { "op": "set_resource", "name": "Exposure", "value": { "ev100": 4 } },
            { "op": "spawn", "id": "table", "components": {
                "Transform": { "translation": [0, 0.5, 0], "scale": [2, 1, 1] },
                "Primitive": { "shape": "Cube" } } }
        ]),
    )
}

fn lamp_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let lights = probe.query(None, "has(PointLight)")?;
    if lights.is_empty() {
        return Err("there is no PointLight".to_owned());
    }
    let mut reasons = Vec::new();
    for id in &lights {
        let at = probe.position(None, id)?;
        let light = probe.components(None, id)?["PointLight"].clone();
        let lumens = light["intensity_lm"].as_f64().unwrap_or(800.0);
        let range = light["range"].as_f64().unwrap_or(10.0);
        if at[0].abs() > 1.0 || at[2].abs() > 1.0 {
            reasons.push(format!(
                "'{id}' is not over the table (x {:.1}, z {:.1})",
                at[0], at[2]
            ));
        } else if !(2.0..=3.5).contains(&at[1]) {
            reasons.push(format!(
                "'{id}' is at y = {:.1}, not 1 to 2 m above the table top (y = 1)",
                at[1]
            ));
        } else if lumens < 100.0 || range < 2.0 {
            reasons.push(format!(
                "'{id}' is too weak to light the table ({lumens} lm, range {range} m)"
            ));
        } else {
            return Ok(());
        }
    }
    Err(reasons.join("; "))
}

fn lamp_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "spawn", "id": "lamp", "components": {
            "Transform": { "translation": [0, 2.5, 0] },
            "PointLight": { "color": [1, 0.85, 0.6, 1], "intensity_lm": 800, "range": 6 } } }]),
    )
}

// --------------------------------------------------------------------------
// 19: play an animation clip on a loop.

fn anim_setup(dir: &Path) -> Result<(), String> {
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

fn anim_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let s = start(probe)?;
    step(probe, &s, 150, json!({}))?;
    let player = live(probe, &s, "bar")?["AnimationPlayer"].clone();
    if player.is_null() {
        return Err("'bar' has no AnimationPlayer".to_owned());
    }
    if player["clip"] != "bend" {
        return Err(format!("'bar' plays {}, not \"bend\"", player["clip"]));
    }
    let time = num(&player["time"]);
    if player["playing"] != true || !(0.0..1.0).contains(&time) {
        return Err(format!(
            "after 2.5 s the 1 s clip is at {time:.2} s, playing {}: it does not loop",
            player["playing"]
        ));
    }
    Ok(())
}

fn anim_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "set", "id": "bar", "component": "AnimationPlayer",
                 "value": { "clip": "bend", "looping": true, "playing": true } }]),
    )
}

// --------------------------------------------------------------------------
// 20: a score at the top left.

fn hud_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let shown = interface(probe, None)?;
    let label = shown
        .iter()
        .find(|e| e["text"].as_str().is_some_and(|t| t.trim() == "Score: 0"));
    let Some(label) = label else {
        return Err("no interface text says \"Score: 0\"".to_owned());
    };
    let [left, top, right, bottom] = bbox(label);
    if left > 160.0 || top > 90.0 {
        return Err(format!(
            "the score is at ({left}, {top}) of a 640x360 picture, not in the top left quarter"
        ));
    }
    if right - left < 20.0 || bottom - top < 8.0 {
        return Err("the score is too small to read".to_owned());
    }
    Ok(())
}

fn hud_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "spawn", "id": "score", "components": {
            "UiNode": { "position": "Absolute", "offset": [24, 24] },
            "UiText": { "text": "Score: 0", "size": 36 } } }]),
    )
}

// --------------------------------------------------------------------------
// 21: a start button in the middle.

fn button_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let shown = interface(probe, None)?;
    let button = shown.iter().find(|e| {
        let [l, t, r, b] = bbox(e);
        !e["button"].is_null()
            && ((l + r) / 2.0 - 320.0).abs() < 64.0
            && ((t + b) / 2.0 - 180.0).abs() < 40.0
    });
    let Some(button) = button else {
        return Err("no button in the middle of the screen".to_owned());
    };
    let [l, t, r, b] = bbox(button);
    let labelled = shown.iter().any(|e| {
        let [x0, y0, x1, y1] = bbox(e);
        e["text"]
            .as_str()
            .is_some_and(|text| text.trim() == "Start")
            && x0 >= l - 1.0
            && y0 >= t - 1.0
            && x1 <= r + 1.0
            && y1 <= b + 1.0
    });
    if !labelled {
        return Err("the button does not say \"Start\" inside it".to_owned());
    }
    // It can be clicked: press and release on its middle.
    let id = button["entity"].as_str().unwrap_or_default().to_owned();
    let s = start(probe)?;
    let layout = live(probe, &s, &id)?["UiLayout"].clone();
    let x = num(&layout["position"][0]) + num(&layout["size"][0]) / 2.0;
    let y = num(&layout["position"][1]) + num(&layout["size"][1]) / 2.0;
    step(probe, &s, 1, json!({ "pointer": [x, y], "click": true }))?;
    let clicked = step(probe, &s, 1, json!({ "pointer": [x, y] }))?;
    if clicked["events"]["ButtonClicked"].as_u64().unwrap_or(0) == 0 {
        return Err("clicking the button's middle does not click it".to_owned());
    }
    Ok(())
}

fn button_reference(probe: &mut Probe) -> Result<(), String> {
    let full = json!({ "unit": "Percent", "value": 100 });
    edit(
        probe,
        json!([
            { "op": "spawn", "id": "menu", "components": {
                "UiNode": { "width": full, "height": full, "justify": "Center", "align": "Center" } } },
            { "op": "spawn", "id": "start", "parent": "menu", "components": {
                "UiNode": { "padding": [16, 40, 16, 40], "background": [0.1, 0.3, 0.9, 1] },
                "UiButton": {} } },
            { "op": "spawn", "id": "start_label", "parent": "start", "components": {
                "UiNode": {}, "UiText": { "text": "Start", "size": 40 } } }
        ]),
    )
}

// --------------------------------------------------------------------------
// 22: game code counts clicks into a label.

const COUNTER_CODE: &str = r#"
use wish_sdk::prelude::*;

/// Clicks so far.
#[resource]
pub struct Clicks {
    /// How many.
    pub count: u32,
}

/// The text that shows the count.
#[component]
pub struct CountLabel {
    /// The count it shows.
    pub shown: u32,
}

#[system]
fn count(clicks: EventReader<ButtonClicked>, mut total: ResMut<Clicks>) {
    for _ in clicks.read() {
        total.count += 1;
    }
}

#[system]
fn show(total: Res<Clicks>, mut labels: Query<(&mut CountLabel, &mut UiText)>) {
    for (label, words) in labels.iter_mut() {
        if label.shown != total.count || text_of(words.text).is_empty() {
            words.text = text(&format!("Clicks: {}", total.count));
            label.shown = total.count;
        }
    }
}

wish_sdk::export! {
    components: [CountLabel],
    resources: [Clicks],
    systems: [count, show],
}
"#;

fn counter_setup(dir: &Path) -> Result<(), String> {
    setup_ops(
        dir,
        json!([
            { "op": "spawn", "id": "more", "components": {
                "UiNode": { "position": "Absolute", "offset": [540, 300],
                    "width": { "unit": "Px", "value": 200 }, "height": { "unit": "Px", "value": 80 },
                    "background": [0.1, 0.5, 0.2, 1] },
                "UiButton": {} } },
            { "op": "spawn", "id": "more_label", "parent": "more", "components": {
                "UiNode": {}, "UiText": { "text": "+1", "size": 40 } } },
            { "op": "spawn", "id": "label", "components": {
                "UiNode": { "position": "Absolute", "offset": [20, 20] },
                "UiText": { "text": "Clicks: 0", "size": 32 } } }
        ]),
    )
}

fn counter_check(probe: &mut Probe, dir: &Path, _: &Value) -> Verdict {
    let click = |n: u64| {
        json!([
            { "input": { "pointer": [640, 340], "click": true }, "frames": 1 },
            { "input": { "pointer": [640, 340] }, "frames": 2 },
            { "expect": format!("Clicks.count == {n}"), "message": format!("{n} click(s) counted") }
        ])
    };
    let mut steps = vec![
        json!({ "frames": 2 }),
        json!({ "expect": "Clicks.count == 0" }),
    ];
    for n in 1..=3 {
        steps.extend(click(n).as_array().cloned().unwrap_or_default());
    }
    steps.push(json!({ "frames": 1 }));
    steps.push(
        json!({ "expect": "UiText.text == \"Clicks: 3\"", "entity": "label",
                       "message": "the label says how many clicks" }),
    );
    steps.push(json!({ "input": { "pointer": [100, 600], "click": true }, "frames": 1 }));
    steps.push(json!({ "input": { "pointer": [100, 600] }, "frames": 2 }));
    steps.push(
        json!({ "expect": "Clicks.count == 3", "message": "a click elsewhere does not count" }),
    );
    let hidden =
        json!({ "format": 1, "scene": "scenes/main.scene.json", "seed": 1, "steps": steps });
    crate::game_tasks::play_check(probe, dir, &hidden)
}

fn counter_reference(probe: &mut Probe) -> Result<(), String> {
    let dir = probe.root();
    write_file(&dir, "scripts/src/lib.rs", COUNTER_CODE.as_bytes())?;
    probe.call("script_build", json!({}))?;
    edit(
        probe,
        json!([{ "op": "set", "id": "label", "component": "CountLabel", "value": {} }]),
    )?;
    write_file(
        &dir,
        "tests/clicks.test.json",
        br#"{ "format": 1, "scene": "scenes/main.scene.json", "steps": [
            { "input": { "pointer": [640, 340], "click": true }, "frames": 1 },
            { "input": { "pointer": [640, 340] }, "frames": 2 },
            { "expect": "Clicks.count == 1" } ] }"#,
    )
}

// --------------------------------------------------------------------------
// 23: a 2D scene with a sprite.

const HERO: &str = "assets/sprites/hero.png";

fn two_d_setup(dir: &Path) -> Result<(), String> {
    write_file(dir, HERO, &hero_png()?)
}

fn two_d_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    if probe
        .query(None, "Camera.projection == \"Orthographic\"")?
        .is_empty()
    {
        return Err("no camera is orthographic".to_owned());
    }
    let hero = probe.components(None, "hero")?;
    if hero["Sprite"]["image"] != HERO {
        return Err(format!("'hero' has no Sprite showing {HERO}"));
    }
    let seen = probe.call("describe_view", json!({ "width": 640, "height": 360 }))?;
    let shown = seen["sprites"]
        .as_array()
        .and_then(|all| all.iter().find(|s| s["entity"] == "hero").cloned());
    let Some(shown) = shown else {
        return Err("'hero' is not among the sprites the camera sees".to_owned());
    };
    if shown["bounding_box"].is_null() {
        return Err("'hero' is outside the picture".to_owned());
    }
    let [l, t, r, b] = bbox(&shown);
    if r - l < 8.0 || b - t < 8.0 {
        return Err(format!(
            "'hero' is only {}x{} pixels in a 640x360 picture",
            r - l,
            b - t
        ));
    }
    Ok(())
}

fn two_d_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([
            { "op": "set", "id": "camera", "component": "Camera",
              "value": { "projection": "Orthographic", "ortho_height": 10 } },
            { "op": "spawn", "id": "hero", "components": {
                "Transform": { "translation": [0, 2, 0] },
                "Sprite": { "image": HERO, "pixels_per_unit": 16, "smooth": false } } }
        ]),
    )
}

// --------------------------------------------------------------------------
// 24: sparks over a campfire.

fn fire_setup(dir: &Path) -> Result<(), String> {
    setup_ops(
        dir,
        json!([{ "op": "spawn", "id": "campfire", "components": {
            "Transform": { "translation": [2, 0.15, 0], "scale": [0.6, 0.3, 0.6] },
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.3, 0.15, 0.05, 1] } } }]),
    )
}

fn fire_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let emitters = probe.query(None, "has(ParticleEmitter)")?;
    let fire = probe.position(None, "campfire")?;
    let mut reasons = Vec::new();
    let mut found = None;
    for id in &emitters {
        let e = probe.components(None, id)?["ParticleEmitter"].clone();
        let at = probe.position(None, id)?;
        let m = probe.matrix(None, id)?;
        let color = &e["color_start"];
        let (r, g, b) = (
            color[0].as_f64().unwrap_or(1.0),
            color[1].as_f64().unwrap_or(1.0),
            color[2].as_f64().unwrap_or(1.0),
        );
        let near = ((at[0] - fire[0]).powi(2) + (at[2] - fire[2]).powi(2)).sqrt() < 1.0
            && (at[1] - fire[1]).abs() < 1.5;
        if !near {
            reasons.push(format!("'{id}' is not at the campfire"));
        } else if e["rate"].as_f64().unwrap_or(20.0) < 20.0 {
            reasons.push(format!("'{id}' gives off fewer than 20 particles a second"));
        } else if !(r > 0.6 && r > g && b < 0.4) {
            reasons.push(format!("'{id}' starts {r:.2}, {g:.2}, {b:.2}: not orange"));
        } else if m[5] < 0.7 {
            reasons.push(format!("'{id}' does not point up (its +Y axis)"));
        } else {
            found = Some(id.clone());
            break;
        }
    }
    let Some(id) = found else {
        return Err(if reasons.is_empty() {
            "there is no ParticleEmitter".to_owned()
        } else {
            reasons.join("; ")
        });
    };
    let s = start(probe)?;
    step(probe, &s, 60, json!({}))?;
    let alive = live(probe, &s, &id)?["ParticleState"]["alive"]
        .as_u64()
        .unwrap_or(0);
    if alive < 10 {
        return Err(format!("after a second only {alive} particles are alive"));
    }
    Ok(())
}

fn fire_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "spawn", "id": "sparks", "components": {
            "Transform": { "translation": [2, 0.3, 0] },
            "ParticleEmitter": { "rate": 40, "lifetime": 1.2, "speed": 1.5, "spread_deg": 15,
                "size_start": 0.15, "color_start": [1, 0.45, 0.05, 1], "color_end": [0.8, 0.1, 0, 0] } } }]),
    )
}

// --------------------------------------------------------------------------
// 25: what the camera's centre ray hits first.

fn ray_setup(dir: &Path) -> Result<(), String> {
    let block = |id: &str, x: f64, y: f64, z: f64| {
        json!({ "op": "spawn", "id": id, "components": {
            "Transform": { "translation": [x, y, z] },
            "Primitive": { "shape": "Cube" },
            "Collider": { "shape": "Box" } } })
    };
    setup_ops(
        dir,
        json!([
            block("block_a", 0.0, 0.5, -4.0),
            block("block_b", 0.0, 0.75, 0.0),
            block("block_c", 2.5, 0.75, 0.0)
        ]),
    )
}

fn ray_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let note = |probe: &mut Probe, id: &str| -> Result<Option<String>, String> {
        Ok(probe.entity(None, id)?["notes"].as_str().map(str::to_owned))
    };
    match note(probe, "block_b")?.as_deref() {
        Some(text) if text.trim().eq_ignore_ascii_case("hit") => {}
        other => {
            return Err(format!(
                "'block_b' (hit first) has notes {other:?}, not \"hit\""
            ));
        }
    }
    for id in ["block_a", "block_c", "camera", "ground", "sun"] {
        if let Some(text) = note(probe, id)? {
            return Err(format!("'{id}' is not hit first but got the note {text:?}"));
        }
    }
    Ok(())
}

fn ray_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "set_notes", "id": "block_b", "notes": "hit" }]),
    )
}

/// The game systems' tasks, 13 to 25.
pub fn all() -> Vec<Task> {
    let task = |id, prompt, setup, check, reference, files| Task {
        id,
        prompt,
        setup,
        baseline: nothing,
        check,
        reference,
        files,
    };
    vec![
        task(
            "13_drop_box",
            "The cube named 'box' hangs in the air. Add physics: when the game starts it should fall with gravity, hit the ground and stay on top of it (not fall through the ground). Do not move the box in the scene.",
            drop_setup,
            drop_check,
            drop_reference,
            false,
        ),
        task(
            "14_bouncy_ball",
            "The ball named 'ball' falls to the ground but does not bounce. After it hits the ground it should bounce at least 1 meter high (the bottom of the ball should rise at least 1 m above the ground). Do not move the ball.",
            bounce_setup,
            bounce_check,
            bounce_reference,
            false,
        ),
        task(
            "15_trigger_zone",
            "The ball named 'ball' falls. Where the ball falls, just above the ground, put a trigger zone named 'goal': the ball should pass through it and fall all the way to the ground, but a CollisionStarted event should come when it enters.",
            trigger_setup,
            trigger_check,
            trigger_reference,
            false,
        ),
        task(
            "16_jump_code",
            "'player' is a physics character (CharacterController). Write the game code (scripts/src/lib.rs): the 'move' action in input.json should walk it at 3 meters per second (x right +X, y forward -Z); when 'jump' is pressed and it is on the ground, it should jump (rise at least half a meter) and land back on the ground; the code should add the gravity. Build it with script_build and prove the behaviour with a scenario test that you write under tests/.",
            jump_setup,
            jump_check,
            jump_reference,
            true,
        ),
        task(
            "17_music",
            "The music assets/sounds/music.wav should play as soon as the game starts and start again from the beginning when it ends (loop). Also put a listener (AudioListener) on the camera.",
            music_setup,
            music_check,
            music_reference,
            false,
        ),
        task(
            "18_table_lamp",
            "The scene is at night and dark. To light the top of the table ('table'), put a point light (PointLight) 1–2 meters above the table's top surface.",
            lamp_setup,
            lamp_check,
            lamp_reference,
            false,
        ),
        task(
            "19_play_animation",
            "Play the 'bend' animation inside the 'bar' model when the game starts, and have it repeat continuously (loop).",
            anim_setup,
            anim_check,
            anim_reference,
            false,
        ),
        task(
            "20_hud_score",
            "Using the in-game interface, show a text that says 'Score: 0' in the top left corner of the screen, large enough to read.",
            |_| Ok(()),
            hud_check,
            hud_reference,
            false,
        ),
        task(
            "21_start_button",
            "In the middle of the screen there should be a clickable button with 'Start' written on it.",
            |_| Ok(()),
            button_check,
            button_reference,
            false,
        ),
        task(
            "22_click_counter",
            "Each time the 'more' button is clicked, the text named 'label' should become 'Clicks: N' (N is the number of clicks so far; 'Clicks: 0' at the start). In the game code (scripts/src/lib.rs), keep the number in the count field of the Clicks resource. Prove the behaviour with a scenario test that you write under tests/ (to click in the test, use pointer and click in input).",
            counter_setup,
            counter_check,
            counter_reference,
            true,
        ),
        task(
            "23_two_d",
            "Turn the scene into 2D: the camera should be orthographic, and add a sprite named 'hero' that shows the picture assets/sprites/hero.png; it should be visible from the camera and at least 8x8 pixels (in a 640x360 picture).",
            two_d_setup,
            two_d_check,
            two_d_reference,
            false,
        ),
        task(
            "24_campfire",
            "Orange sparks should rise upward from the top of the campfire named 'campfire': add a particle emitter that emits at least 20 particles per second.",
            fire_setup,
            fire_check,
            fire_reference,
            false,
        ),
        task(
            "25_center_ray",
            "Imagine a ray sent from the exact middle of the picture, in the direction the camera looks. Add the note 'hit' in the scene to the first entity this ray hits; do not add notes to any other entity.",
            ray_setup,
            ray_check,
            ray_reference,
            false,
        ),
    ]
}
