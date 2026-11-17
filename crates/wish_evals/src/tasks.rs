//! The evaluation tasks: what the agent is asked (in the user's language),
//! how the project is prepared, how the result is checked with the engine's
//! own tools, and a reference solution that proves the check can pass.

use std::path::Path;

use serde_json::{Value, json};

use crate::probe::{Probe, VIEW_H, VIEW_W, Verdict, cube_bottom, require_visible};

/// One task.
pub struct Task {
    pub id: &'static str,
    pub prompt: &'static str,
    /// Prepares the project (after `wen new`).
    pub setup: fn(&Path) -> Result<(), String>,
    /// What must stay as it is, recorded after setup.
    pub baseline: fn(&mut Probe) -> Result<Value, String>,
    /// Whether the agent did it.
    pub check: fn(&mut Probe, &Path, &Value) -> Verdict,
    /// A correct solution, made with the tools (for the self-test).
    pub reference: fn(&mut Probe) -> Result<(), String>,
    /// Whether the agent may read and edit files (game code tasks); else it
    /// has the engine's tools only.
    pub files: bool,
}

fn edit(probe: &mut Probe, ops: Value) -> Result<(), String> {
    probe.call("edit_apply", json!({ "ops": ops })).map(|_| ())
}

fn edit_in(probe: &mut Probe, scene: &str, create: bool, ops: Value) -> Result<(), String> {
    probe
        .call(
            "edit_apply",
            json!({ "scene": scene, "create": create, "ops": ops }),
        )
        .map(|_| ())
}

fn setup_with(dir: &Path, ops: Value) -> Result<(), String> {
    let mut probe = Probe::open(dir)?;
    edit(&mut probe, ops)
}

fn nothing(_: &mut Probe) -> Result<Value, String> {
    Ok(Value::Null)
}

fn transform(probe: &mut Probe, id: &str) -> Result<Value, String> {
    Ok(probe.components(None, id)?["Transform"].clone())
}

fn same(a: &Value, b: &Value, what: &str) -> Verdict {
    let close = |x: &Value, y: &Value| -> bool {
        match (x, y) {
            (Value::Number(p), Value::Number(q)) => (p.as_f64().unwrap_or(0.0) - q.as_f64().unwrap_or(0.0)).abs() < 1e-4,
            (Value::Array(p), Value::Array(q)) => p.len() == q.len() && p.iter().zip(q).all(|(i, j)| i == j || matches!((i, j), (Value::Number(_), Value::Number(_)) if (i.as_f64().unwrap_or(0.0) - j.as_f64().unwrap_or(0.0)).abs() < 1e-4)),
            _ => x == y,
        }
    };
    let ok = match (a, b) {
        (Value::Object(p), Value::Object(q)) => {
            p.len() == q.len() && p.iter().all(|(k, v)| q.get(k).is_some_and(|w| close(v, w)))
        }
        _ => close(a, b),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("{what} changed: was {a}, now {b}"))
    }
}

// --------------------------------------------------------------------------

fn cube_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let red = probe.query(
        None,
        "Primitive.shape == \"Cube\" && Material.base_color.x > 0.5 && Material.base_color.y < 0.4 && Material.base_color.z < 0.4",
    )?;
    if red.is_empty() {
        return Err("no red cube in the scene".to_owned());
    }
    let seen = probe.visible(None)?;
    let mut reasons = Vec::new();
    for id in &red {
        let m = probe.matrix(None, id)?;
        let bottom = cube_bottom(&m);
        let verdict = if (bottom).abs() > 0.05 {
            Err(format!(
                "'{id}' bottom is at y = {bottom:.2}, not on the ground"
            ))
        } else if m[12].abs() > 2.0 || m[14].abs() > 2.0 {
            Err(format!(
                "'{id}' is at x = {:.1}, z = {:.1}, not in the middle",
                m[12], m[14]
            ))
        } else {
            require_visible(&seen, id, 50.0)
        };
        match verdict {
            Ok(()) => return Ok(()),
            Err(reason) => reasons.push(reason),
        }
    }
    Err(reasons.join("; "))
}

fn cube_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "spawn", "id": "red_cube", "components": {
        "Primitive": { "shape": "Cube" }, "Material": { "base_color": [0.9, 0.1, 0.1, 1] },
        "Transform": { "translation": [0, 0.5, 0] } } }]),
    )
}

// --------------------------------------------------------------------------

fn stairs_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let cubes = probe.query(None, "Primitive.shape == \"Cube\"")?;
    let mut placed = Vec::new();
    for id in &cubes {
        placed.push((id.clone(), probe.position(None, id)?));
    }
    placed.sort_by(|a, b| a.1[1].total_cmp(&b.1[1]));
    let seen = probe.visible(None)?;
    let n = placed.len();
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let (a, b, c) = (&placed[i], &placed[j], &placed[k]);
                let step = |p: &[f64; 3], q: &[f64; 3]| {
                    let dy = q[1] - p[1];
                    let flat = ((q[0] - p[0]).powi(2) + (q[2] - p[2]).powi(2)).sqrt();
                    dy >= 0.95 && flat < 4.0
                };
                if step(&a.1, &b.1)
                    && step(&b.1, &c.1)
                    && [a, b, c]
                        .iter()
                        .all(|p| require_visible(&seen, &p.0, 30.0).is_ok())
                {
                    return Ok(());
                }
            }
        }
    }
    Err(format!(
        "no three visible cubes rise like stairs (each at least 1 m higher, under 4 m apart); cubes: {}",
        placed
            .iter()
            .map(|(id, p)| format!("{id} ({:.1}, {:.1}, {:.1})", p[0], p[1], p[2]))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn stairs_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([
            { "op": "spawn", "id": "p1", "components": { "Primitive": { "shape": "Cube" }, "Transform": { "translation": [-3, 0.5, 0], "scale": [2, 0.4, 2] } } },
            { "op": "spawn", "id": "p2", "components": { "Primitive": { "shape": "Cube" }, "Transform": { "translation": [0, 1.5, -1], "scale": [2, 0.4, 2] } } },
            { "op": "spawn", "id": "p3", "components": { "Primitive": { "shape": "Cube" }, "Transform": { "translation": [3, 2.5, -2], "scale": [2, 0.4, 2] } } }
        ]),
    )
}

// --------------------------------------------------------------------------

fn sunset_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let lights = probe.query(None, "has(DirectionalLight)")?;
    if lights.is_empty() {
        return Err("no DirectionalLight".to_owned());
    }
    let mut reasons = Vec::new();
    for id in &lights {
        let light = probe.components(None, id)?["DirectionalLight"].clone();
        let color: Vec<f64> = light["color"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_f64)
            .collect();
        let m = probe.matrix(None, id)?;
        // The light travels along the entity's -Z axis.
        let dir = [-m[8], -m[9], -m[10]];
        let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2])
            .sqrt()
            .max(1e-9);
        let elevation = (-dir[1] / len).asin().to_degrees();
        if color.len() < 3 || color[0] <= color[2] + 0.25 || color[0] < color[1] {
            reasons.push(format!("'{id}' color {color:?} is not orange"));
        } else if !(0.5..=20.0).contains(&elevation) {
            reasons.push(format!(
                "'{id}' comes from {elevation:.1} degrees above the horizon (needs 0.5 to 20)"
            ));
        } else {
            let sky = probe.query(None, "Sky.horizon.x > Sky.horizon.z + 0.1")?;
            return if sky.is_empty() {
                Err(
                    "the sky's horizon is not a warm color (Sky.horizon red > blue + 0.1)"
                        .to_owned(),
                )
            } else {
                Ok(())
            };
        }
    }
    Err(reasons.join("; "))
}

fn sunset_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([
            { "op": "set", "id": "sun", "component": "Transform", "value": { "euler_deg": [-10, 30, 0] } },
            { "op": "set", "id": "sun", "component": "DirectionalLight", "value": { "color": [1, 0.55, 0.25, 1] } },
            { "op": "set_resource", "name": "Sky", "value": { "horizon": [1, 0.5, 0.3, 1] } }
        ]),
    )
}

// --------------------------------------------------------------------------

fn statue_setup(dir: &Path) -> Result<(), String> {
    setup_with(
        dir,
        json!([{ "op": "spawn", "id": "statue", "components": {
        "Primitive": { "shape": "Cube" }, "Material": { "base_color": [0.9, 0.9, 0.85, 1] },
        "Transform": { "translation": [8, 1, -10], "scale": [1, 2, 1] } } }]),
    )
}

fn statue_baseline(probe: &mut Probe) -> Result<Value, String> {
    transform(probe, "statue")
}

fn framing_check(probe: &mut Probe, _: &Path, baseline: &Value) -> Verdict {
    same(
        baseline,
        &transform(probe, "statue")?,
        "the statue's Transform",
    )?;
    let seen = probe.visible(None)?;
    require_visible(&seen, "statue", 90.0)?;
    let s = &seen["statue"];
    let cx = (s.bbox[0] + s.bbox[2]) / 2.0 / VIEW_W - 0.5;
    let cy = (s.bbox[1] + s.bbox[3]) / 2.0 / VIEW_H - 0.5;
    if cx.abs() > 0.15 || cy.abs() > 0.15 {
        return Err(format!(
            "the statue is off centre by {:.0}% x, {:.0}% y (max 15%)",
            cx * 100.0,
            cy * 100.0
        ));
    }
    let share = s.pixels / (VIEW_W * VIEW_H);
    if share < 0.05 {
        return Err(format!(
            "the statue covers {:.1}% of the picture (needs 5%)",
            share * 100.0
        ));
    }
    Ok(())
}

fn framing_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "set", "id": "camera", "component": "Transform",
        "value": { "translation": [8, 1.5, -6.5], "euler_deg": [-8.13, 0, 0] } }]),
    )
}

// --------------------------------------------------------------------------

const CRATE_PREFAB: &str = "prefabs/crate.prefab.json";

fn crate_setup(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir.join("prefabs")).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join(CRATE_PREFAB),
        "{\n  \"entities\": {\n    \"root\": {\n      \"components\": {\n        \"Material\": {\n          \"base_color\": [0.6, 0.4, 0.2, 1]\n        },\n        \"Primitive\": {\n          \"shape\": \"Cube\"\n        },\n        \"Transform\": {}\n      }\n    }\n  },\n  \"format\": 1\n}\n",
    )
    .map_err(|e| e.to_string())
}

fn row_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let scene = probe.call("scene_get", json!({ "detail": "full" }))?;
    let mut ids = Vec::new();
    fn walk(node: &Value, ids: &mut Vec<String>) {
        if node["prefab"] == CRATE_PREFAB {
            ids.push(node["id"].as_str().unwrap_or_default().to_owned());
        }
        for child in node["children"].as_array().into_iter().flatten() {
            walk(child, ids);
        }
    }
    for node in scene["tree"].as_array().into_iter().flatten() {
        walk(node, &mut ids);
    }
    if ids.len() < 5 {
        return Err(format!(
            "{} instance(s) of {CRATE_PREFAB}, need 5",
            ids.len()
        ));
    }
    let mut placed = Vec::new();
    for id in &ids {
        placed.push((id.clone(), probe.matrix(None, id)?));
    }
    placed.sort_by(|a, b| a.1[12].total_cmp(&b.1[12]));
    // "2 m between them" is read two ways, both fair (found in the evals):
    // centres 2 m apart, or a 2 m gap between 1 m crates (centres 3 m apart).
    // Either is accepted, as long as the whole row uses the same one.
    for spacing in [2.0, 3.0] {
        for window in placed.windows(5) {
            let ok = window.windows(2).all(|pair| {
                let (a, b) = (&pair[0].1, &pair[1].1);
                let dx = b[12] - a[12];
                (dx - spacing).abs() <= 0.2
                    && (b[14] - a[14]).abs() < 0.2
                    && (b[13] - a[13]).abs() < 0.05
            }) && window.iter().all(|(_, m)| cube_bottom(m).abs() <= 0.05);
            if ok {
                return Ok(());
            }
        }
    }
    Err(format!(
        "no five instances 2 m apart in a row along x, resting on the ground: {}",
        placed
            .iter()
            .map(|(id, m)| format!("{id} ({:.2}, {:.2}, {:.2})", m[12], m[13], m[14]))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn row_reference(probe: &mut Probe) -> Result<(), String> {
    let ops: Vec<Value> = (0..5)
        .map(|i| {
            json!({ "op": "instantiate", "id": format!("crate_{}", i + 1), "prefab": CRATE_PREFAB,
            "components": { "Transform": { "translation": [-4.0 + 2.0 * f64::from(i), 0.5, 0] } } })
        })
        .collect();
    edit(probe, Value::Array(ops))
}

// --------------------------------------------------------------------------

fn lamp_setup(dir: &Path) -> Result<(), String> {
    setup_with(
        dir,
        json!([{ "op": "spawn", "id": "lamp", "components": {
        "Primitive": { "shape": "Sphere" }, "Material": { "base_color": [1, 0.9, 0.4, 1], "emissive": [1, 0.8, 0.3] },
        "Transform": { "translation": [0, -3, 0], "scale": [0.5, 0.5, 0.5] } } }]),
    )
}

fn others_baseline(probe: &mut Probe) -> Result<Value, String> {
    let mut out = serde_json::Map::new();
    for id in ["camera", "ground", "sun"] {
        out.insert(id.to_owned(), probe.components(None, id)?);
    }
    Ok(Value::Object(out))
}

fn lamp_check(probe: &mut Probe, _: &Path, baseline: &Value) -> Verdict {
    for id in ["camera", "ground", "sun"] {
        let now = probe.components(None, id)?;
        if now != baseline[id] {
            return Err(format!("'{id}' was changed, but only the lamp should be"));
        }
    }
    let seen = probe.visible(None)?;
    require_visible(&seen, "lamp", 50.0)
}

fn lamp_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "set", "id": "lamp", "component": "Transform", "value": { "translation": [0, 1, 0] } }]),
    )
}

// --------------------------------------------------------------------------

fn falling_setup(dir: &Path) -> Result<(), String> {
    let mut config = wish_control::ProjectConfig::new("Eval");
    config.example = Some("stress".to_owned());
    std::fs::write(
        dir.join("wish.json"),
        config.to_canonical().map_err(|p| p.message.clone())?,
    )
    .map_err(|e| e.to_string())?;
    setup_with(
        dir,
        json!([
            { "op": "spawn", "id": "a", "components": { "Position": { "value": [0, 1, 0] }, "Velocity": { "value": [1, 0, 0] } } },
            { "op": "spawn", "id": "b", "components": { "Position": { "value": [0, 1, 0] }, "Velocity": { "value": [0, -1, 0] } } },
            { "op": "spawn", "id": "c", "components": { "Position": { "value": [0, 5, 0] }, "Velocity": { "value": [0, -1, 0] } } }
        ]),
    )
}

fn falling_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let note = |probe: &mut Probe, id: &str| -> Result<Option<String>, String> {
        Ok(probe.entity(None, id)?["notes"].as_str().map(str::to_owned))
    };
    match note(probe, "b")?.as_deref() {
        Some(text) if text.trim().eq_ignore_ascii_case("falls") => {}
        other => {
            return Err(format!(
                "'b' (the one that falls) has notes {other:?}, not \"falls\""
            ));
        }
    }
    for id in ["a", "c"] {
        if let Some(text) = note(probe, id)? {
            return Err(format!(
                "'{id}' does not fall below zero but got the note {text:?}"
            ));
        }
    }
    Ok(())
}

fn falling_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "set_notes", "id": "b", "notes": "falls" }]),
    )
}

// --------------------------------------------------------------------------

fn wall_setup(dir: &Path) -> Result<(), String> {
    setup_with(
        dir,
        json!([
            { "op": "spawn", "id": "target", "components": { "Primitive": { "shape": "Sphere" }, "Material": { "base_color": [0.2, 0.6, 1, 1] }, "Transform": { "translation": [0, 1, -4] } } },
            { "op": "spawn", "id": "wall", "components": { "Primitive": { "shape": "Cube" }, "Transform": { "translation": [0, 1.5, 1], "scale": [3, 3, 0.3] } } }
        ]),
    )
}

fn target_baseline(probe: &mut Probe) -> Result<Value, String> {
    transform(probe, "target")
}

fn wall_check(probe: &mut Probe, _: &Path, baseline: &Value) -> Verdict {
    same(
        baseline,
        &transform(probe, "target")?,
        "the target's Transform",
    )?;
    if probe.query(None, "id == \"wall\"")?.is_empty() {
        return Err("the wall was removed".to_owned());
    }
    let seen = probe.visible(None)?;
    require_visible(&seen, "target", 60.0)
}

fn wall_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "set", "id": "wall", "component": "Transform", "value": { "translation": [6, 1.5, 1] } }]),
    )
}

// --------------------------------------------------------------------------

/// A 1 x 2 x 1 m box standing on y = 0, as a GLB.
pub fn box_glb() -> Vec<u8> {
    let (x, y, z) = (0.5f32, 2.0f32, 0.5f32);
    let corners = [
        [-x, 0.0, -z],
        [x, 0.0, -z],
        [x, y, -z],
        [-x, y, -z],
        [-x, 0.0, z],
        [x, 0.0, z],
        [x, y, z],
        [-x, y, z],
    ];
    let positions: Vec<u8> = corners
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let indices: [u16; 36] = [
        4, 5, 6, 4, 6, 7, 1, 0, 3, 1, 3, 2, 5, 1, 2, 5, 2, 6, 0, 4, 7, 0, 7, 3, 7, 6, 2, 7, 2, 3,
        0, 1, 5, 0, 5, 4,
    ];
    let index_bytes: Vec<u8> = indices.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut binary = positions.clone();
    binary.extend_from_slice(&index_bytes);
    let gltf = json!({
        "asset": { "version": "2.0" },
        "buffers": [{ "byteLength": binary.len() }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": positions.len() },
            { "buffer": 0, "byteOffset": positions.len(), "byteLength": index_bytes.len() }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 8, "type": "VEC3", "min": [-x, 0, -z], "max": [x, y, z] },
            { "bufferView": 1, "componentType": 5123, "count": 36, "type": "SCALAR" }
        ],
        "materials": [{ "pbrMetallicRoughness": { "baseColorFactor": [0.8, 0.75, 0.6, 1], "metallicFactor": 0 } }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1, "material": 0 }] }],
        "nodes": [{ "mesh": 0 }],
        "scenes": [{ "nodes": [0] }]
    });
    let mut text = serde_json::to_vec(&gltf).unwrap_or_default();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    while !binary.len().is_multiple_of(4) {
        binary.push(0);
    }
    let total = 12 + 8 + text.len() + 8 + binary.len();
    let mut out = b"glTF".to_vec();
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&u32::try_from(total).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(&u32::try_from(text.len()).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&u32::try_from(binary.len()).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&binary);
    out
}

fn model_setup(dir: &Path) -> Result<(), String> {
    std::fs::write(dir.join("assets/statue.glb"), box_glb()).map_err(|e| e.to_string())
}

fn model_check(probe: &mut Probe, _: &Path, _: &Value) -> Verdict {
    let models = probe.query(None, "Model.asset == \"assets/statue.glb\"")?;
    if models.is_empty() {
        return Err("no entity uses assets/statue.glb".to_owned());
    }
    let seen = probe.visible(None)?;
    let mut reasons = Vec::new();
    for id in &models {
        let m = probe.matrix(None, id)?;
        if m[13].abs() > 0.05 {
            reasons.push(format!(
                "'{id}' stands at y = {:.2}: the model's base is at its origin, so it should be 0",
                m[13]
            ));
            continue;
        }
        match require_visible(&seen, id, 30.0) {
            Ok(()) => return Ok(()),
            Err(reason) => reasons.push(reason),
        }
    }
    Err(reasons.join("; "))
}

fn model_reference(probe: &mut Probe) -> Result<(), String> {
    edit(
        probe,
        json!([{ "op": "spawn", "id": "statue", "components": {
        "Model": { "asset": "assets/statue.glb" }, "Transform": { "translation": [0, 0, 0] } } }]),
    )
}

// --------------------------------------------------------------------------

const NIGHT: &str = "scenes/night.scene.json";

fn main_scene_baseline(probe: &mut Probe) -> Result<Value, String> {
    let scene = probe.call("scene_get", json!({ "detail": "full" }))?;
    Ok(
        json!({ "tree": scene["tree"], "resources": scene["resources"], "camera": transform(probe, "camera")? }),
    )
}

fn night_check(probe: &mut Probe, dir: &Path, baseline: &Value) -> Verdict {
    if !dir.join(NIGHT).is_file() {
        return Err(format!("{NIGHT} was not created"));
    }
    let main = probe.call("scene_get", json!({ "detail": "full" }))?;
    if main["tree"] != baseline["tree"] || main["resources"] != baseline["resources"] {
        return Err("the main scene was changed".to_owned());
    }
    probe.check_passes()?;
    let lights = probe.query(Some(NIGHT), "has(DirectionalLight)")?;
    if lights.len() != 1 {
        return Err(format!(
            "{NIGHT} has {} directional lights, needs exactly one (the moon)",
            lights.len()
        ));
    }
    let light = probe.components(Some(NIGHT), &lights[0])?["DirectionalLight"].clone();
    let lux = light["illuminance_lux"].as_f64().unwrap_or(f64::MAX);
    let color: Vec<f64> = light["color"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .collect();
    if lux > 1000.0 {
        return Err(format!("the moon is {lux} lux (at most 1000)"));
    }
    if color.len() < 3 || color[2] <= color[0] {
        return Err(format!("the moon's color {color:?} is not bluish"));
    }
    let cameras = probe.query(Some(NIGHT), "has(Camera)")?;
    let mut camera_ok = false;
    for id in &cameras {
        let t = probe.components(Some(NIGHT), id)?["Transform"].clone();
        if same(&baseline["camera"], &t, "camera").is_ok() {
            camera_ok = true;
        }
    }
    if !camera_ok {
        return Err("no camera in the night scene placed like the main scene's camera".to_owned());
    }
    if probe
        .query(Some(NIGHT), "Primitive.shape == \"Plane\"")?
        .is_empty()
    {
        return Err("the night scene has no ground plane".to_owned());
    }
    Ok(())
}

fn night_reference(probe: &mut Probe) -> Result<(), String> {
    let camera = probe.components(None, "camera")?;
    let ground = probe.components(None, "ground")?;
    edit_in(
        probe,
        NIGHT,
        true,
        json!([
            { "op": "spawn", "id": "camera", "components": { "Camera": camera["Camera"], "Transform": camera["Transform"] } },
            { "op": "spawn", "id": "ground", "components": { "Primitive": ground["Primitive"], "Material": ground["Material"], "Transform": ground["Transform"] } },
            { "op": "spawn", "id": "moon", "components": { "DirectionalLight": { "illuminance_lux": 300, "color": [0.6, 0.7, 1, 1] }, "Transform": { "euler_deg": [-40, 20, 0] } } }
        ]),
    )
}

fn plain(_: &Path) -> Result<(), String> {
    Ok(())
}

/// Every task, easiest first.
pub fn all() -> Vec<Task> {
    vec![
        Task {
            id: "01_red_cube",
            prompt: "Put a red cube in the middle of the scene, standing on the ground. It should be visible from the camera.",
            setup: plain,
            baseline: nothing,
            check: cube_check,
            files: false,
            reference: cube_reference,
        },
        Task {
            id: "02_stairs",
            prompt: "Build a simple level with three platforms: the platforms rise like stairs; each one is at least 1 m higher than the previous one and closer than 4 m to it horizontally, so that it can be jumped to. All three should be visible from the camera.",
            setup: plain,
            baseline: nothing,
            check: stairs_check,
            files: false,
            reference: stairs_reference,
        },
        Task {
            id: "03_sunset",
            prompt: "Make the light look like a sunset: the sun should be close to the horizon and come in at a low angle (at most 20 degrees above the horizon), and its color should be orange; the horizon of the sky should also be a warm color (orange or red).",
            setup: plain,
            baseline: nothing,
            check: sunset_check,
            files: false,
            reference: sunset_reference,
        },
        Task {
            id: "04_frame_statue",
            prompt: "Position the camera so that it looks at the entity named 'statue': the entity should be near the middle of the picture and cover at least 5% of the picture. Do not move the statue.",
            setup: statue_setup,
            baseline: statue_baseline,
            check: framing_check,
            files: false,
            reference: framing_reference,
        },
        Task {
            id: "05_crate_row",
            prompt: "Place five instances of the prefab prefabs/crate.prefab.json: in a single row along the x axis, 2 m apart, all standing on the ground.",
            setup: crate_setup,
            baseline: nothing,
            check: row_check,
            files: false,
            reference: row_reference,
        },
        Task {
            id: "06_hidden_lamp",
            prompt: "The 'lamp' in the scene is not visible from the camera. Find out why and make it visible; do not change any other entity.",
            setup: lamp_setup,
            baseline: others_baseline,
            check: lamp_check,
            files: false,
            reference: lamp_reference,
        },
        Task {
            id: "07_who_falls",
            prompt: "Run the simulation for 120 ticks. Find the entity whose Position.value.y goes below zero during that time, and add the note 'falls' to that entity in the scene. Do not add notes to the other entities.",
            setup: falling_setup,
            baseline: nothing,
            check: falling_check,
            files: false,
            reference: falling_reference,
        },
        Task {
            id: "08_behind_wall",
            prompt: "The sphere named 'target' is not visible from the camera: there is a wall in front of it. Make target visible from the camera without deleting the wall and without moving target.",
            setup: wall_setup,
            baseline: target_baseline,
            check: wall_check,
            files: false,
            reference: wall_reference,
        },
        Task {
            id: "09_import_model",
            prompt: "Add the model assets/statue.glb to the scene: it should sit on the ground (not sunk into it and not floating in the air) and be visible from the camera.",
            setup: model_setup,
            baseline: nothing,
            check: model_check,
            files: false,
            reference: model_reference,
        },
        Task {
            id: "10_night_scene",
            prompt: "Create a second scene named scenes/night.scene.json: it should have the same camera and ground as the main scene, and instead of the sun, a dim, bluish moonlight (a single DirectionalLight, at most 1000 lux). Do not change the main scene.",
            setup: plain,
            baseline: main_scene_baseline,
            check: night_check,
            files: false,
            reference: night_reference,
        },
        Task {
            id: "11_walk_code",
            prompt: "Make the cube named 'player' walkable by the player: write the game code (scripts/src/lib.rs). The 'move' action in input.json should walk the player on the ground at 4 meters per second (move's x is right, +X; its y is forward, that is the -Z direction); with no input it should stop. Build it with script_build and prove the behaviour with a scenario test that you write under tests/.",
            setup: crate::game_tasks::walk_setup,
            baseline: nothing,
            check: crate::game_tasks::walk_check,
            files: true,
            reference: crate::game_tasks::walk_reference,
        },
        Task {
            id: "12_collect_game",
            prompt: "Make a collecting game. The scene has 'player' and three coins (coin_a, coin_b, coin_c). In the game code (scripts/src/lib.rs) there should be a Coin component, a Score resource (field points) and a GameState resource (field won). The player walks at 4 meters per second with the 'move' action in input.json (x right +X, y forward -Z). When the player comes closer than 0.8 meters to a coin, the coin disappears and Score.points goes up by one; when no coins are left, GameState.won becomes true. Add the Coin component to the coins in the scene. Prove that the game can be won with a scenario test that you write under tests/.",
            setup: crate::game_tasks::collect_setup,
            baseline: nothing,
            check: crate::game_tasks::collect_check,
            files: true,
            reference: crate::game_tasks::collect_reference,
        },
    ]
}
