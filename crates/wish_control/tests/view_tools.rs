//! `capture_frame` and `describe_view`, on the software adapter.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = std::env::temp_dir().join(format!("wish-view-tools-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "V", "wen").unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    (dir, ws)
}

fn ok(answer: &Value) -> &Value {
    assert_eq!(answer["ok"], true, "{answer}");
    answer
}

#[test]
fn a_picture_comes_back_as_a_png_and_can_be_written_to_a_file() {
    let (dir, mut ws) = project("capture");
    let me = parse_actor("claude-code");
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "ball", "components": {
            "Primitive": { "shape": "Sphere" }, "Transform": { "translation": [0, 1, 0] },
            "Material": { "base_color": [0.9, 0.1, 0.1, 1] } } }] }),
        &me,
    ));
    let shot = ok(&ws.call("capture_frame", &json!({ "width": 160, "height": 90 }), &me)).clone();
    assert_eq!(shot["image"]["mime"], "image/png");
    assert_eq!(
        (
            shot["image"]["width"].as_u64(),
            shot["image"]["height"].as_u64()
        ),
        (Some(160), Some(90))
    );
    assert!(
        shot["image"]["data"]
            .as_str()
            .unwrap()
            .starts_with("iVBORw0KGgo"),
        "a PNG header"
    );
    assert_eq!(shot["camera"], "camera");
    assert_eq!(shot["drawn"], 2);
    assert_eq!(shot["adapter"]["software"], true);
    assert_eq!(shot["view"], "lit");
    // Same call, same bytes (software adapter).
    let again = ws.call("capture_frame", &json!({ "width": 160, "height": 90 }), &me);
    assert_eq!(again["image"]["data"], shot["image"]["data"]);
    // Every view draws; written to a file instead of returned.
    for view in ["ids", "depth", "normals", "wireframe"] {
        let answer = ok(&ws.call(
            "capture_frame",
            &json!({ "width": 64, "height": 36, "view": view, "out": format!("captures/{view}.png") }),
            &me,
        ))
        .clone();
        assert!(answer["image"].get("data").is_none());
        let png = std::fs::read(dir.join(format!("captures/{view}.png"))).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
    // Mistakes.
    let bad_view = ws.call("capture_frame", &json!({ "view": "wireframes" }), &me);
    assert_eq!(
        bad_view["error"]["hint"]
            .as_str()
            .unwrap()
            .split(' ')
            .next(),
        Some("Did")
    );
    let bad_camera = ws.call("capture_frame", &json!({ "camera": "camra" }), &me);
    assert_eq!(bad_camera["error"]["hint"], "Did you mean 'camera'?");
    let not_a_camera = ws.call("capture_frame", &json!({ "camera": "ball" }), &me);
    assert_eq!(not_a_camera["error"]["code"], "not_a_camera");
    for out in [
        "../escape.png",
        "scenes/main.scene.json",
        "wish.json",
        "/abs.png",
        ".hidden/x.png",
    ] {
        let refused = ws.call("capture_frame", &json!({ "out": out }), &me);
        assert_eq!(refused["error"]["code"], "invalid_output_path", "{out}");
    }
    let huge = ws.call("capture_frame", &json!({ "width": 5000 }), &me);
    assert_eq!(huge["error"]["code"], "invalid_parameter");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn describe_view_says_what_is_visible_and_what_is_hidden() {
    let (dir, mut ws) = project("describe");
    let me = parse_actor("claude-code");
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [
            { "op": "spawn", "id": "wall", "components": { "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [0, 2, 0], "scale": [4, 3, 1] } } },
            { "op": "spawn", "id": "behind", "components": { "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [0, 2, -3], "scale": [0.5, 0.5, 0.5] } } },
            { "op": "spawn", "id": "far_away", "components": { "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [500, 1, 0] } } }
        ] }),
        &me,
    ));
    let seen = ok(&ws.call("describe_view", &json!({ "width": 160, "height": 90 }), &me)).clone();
    let names: Vec<&str> = seen["visible"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["entity"].as_str().unwrap())
        .collect();
    assert_eq!(names[0], "wall", "nearest first: {names:?}");
    assert!(names.contains(&"ground"));
    assert_eq!(seen["visible"][0]["visible_percent"], 100.0);
    assert_eq!(seen["hidden"], json!(["behind", "far_away"]));
    assert_eq!(seen["camera"], "camera");
    // limit lists the nearest and says how many are left out.
    let one = ok(&ws.call(
        "describe_view",
        &json!({ "width": 160, "height": 90, "limit": 1 }),
        &me,
    ))
    .clone();
    assert_eq!(one["visible"].as_array().unwrap().len(), 1);
    assert!(one["visible_not_listed"].as_u64().unwrap() >= 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_session_can_be_looked_at_while_it_runs() {
    let (dir, mut ws) = project("session");
    let me = parse_actor("claude-code");
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "ball", "components": {
        "Primitive": { "shape": "Sphere" }, "Transform": { "translation": [0, 1, 0] } } }] }),
        &me,
    ));
    ok(&ws.call("sim_start", &json!({}), &me));
    let live = ok(&ws.call(
        "capture_frame",
        &json!({ "session": "s1", "width": 64, "height": 36 }),
        &me,
    ))
    .clone();
    assert_eq!(live["session"], "s1");
    assert_eq!(live["tick"], 0);
    // The session is a copy: removing the ball from the scene does not change it.
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "despawn", "id": "ball" }] }),
        &me,
    ));
    let still = ok(&ws.call(
        "describe_view",
        &json!({ "session": "s1", "width": 160, "height": 90 }),
        &me,
    ))
    .clone();
    assert!(
        still["visible"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["entity"] == "ball")
    );
    let scene = ok(&ws.call("describe_view", &json!({ "width": 160, "height": 90 }), &me)).clone();
    assert!(
        !scene["visible"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["entity"] == "ball")
    );
    let wrong = ws.call("capture_frame", &json!({ "session": "s9" }), &me);
    assert_eq!(wrong["error"]["code"], "unknown_session");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Whether a key of an answer names a time: a duration or a rate.
fn names_a_time(key: &str) -> bool {
    [
        "seconds",
        "secs",
        "ms",
        "millis",
        "milliseconds",
        "micros",
        "nanos",
        "took",
        "fps",
    ]
    .contains(&key)
        || ["_ms", "_seconds", "_secs", "_millis"]
            .iter()
            .any(|end| key.ends_with(end))
        || ["time", "duration", "elapsed"]
            .iter()
            .any(|word| key.contains(word))
}

/// Every key of an answer, at any depth.
fn keys_of(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                out.push(key.clone());
                keys_of(inner, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| keys_of(item, out)),
        _ => {}
    }
}

#[test]
fn the_cost_of_a_picture_is_reported_in_counts_and_render_settings_change_it() {
    let (dir, mut ws) = project("stats");
    let me = parse_actor("claude-code");
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "ball", "components": {
            "Primitive": { "shape": "Sphere" }, "Transform": { "translation": [0, 1, 0] } } }] }),
        &me,
    ));
    // The ground (2 triangles) and the ball (960), drawn into the shadow map
    // and into the picture, plus the sky and tone mapping.
    let shot = ok(&ws.call("capture_frame", &json!({ "width": 160, "height": 90 }), &me)).clone();
    assert_eq!(
        shot["stats"],
        json!({
            "width": 160, "height": 90,
            "draw_calls": 6, "triangles": 2 * (2 + 960) + 2,
            "shadow_casters": 2, "point_lights": 0, "spot_lights": 0,
            "effects": [],
        })
    );
    // describe_view says what the lit picture of its view takes.
    let seen = ok(&ws.call("describe_view", &json!({ "width": 160, "height": 90 }), &me)).clone();
    assert_eq!(seen["stats"], shot["stats"]);
    // A debug view counts its own pass.
    let ids = ok(&ws.call(
        "capture_frame",
        &json!({ "width": 160, "height": 90, "view": "ids" }),
        &me,
    ))
    .clone();
    assert_eq!(ids["stats"]["draw_calls"], 2);
    assert_eq!(ids["stats"]["shadow_casters"], 0);
    // Counts only: no key of these answers names a time (the answers being
    // the same in every run is checked by the test below).
    assert!(names_a_time("seconds") && names_a_time("frame_ms") && names_a_time("ms"));
    assert!(!names_a_time("triangles") && !names_a_time("mime"));
    for answer in [&shot, &seen, &ids] {
        let mut keys = Vec::new();
        keys_of(answer, &mut keys);
        let timed: Vec<&String> = keys.iter().filter(|key| names_a_time(key)).collect();
        assert!(timed.is_empty(), "time in a tool answer: {timed:?}");
    }

    // RenderSettings is an engine type like any other: documented, set with
    // an edit, written to the scene file, and it takes effect.
    let schema = ok(&ws.call(
        "schema_get",
        &json!({ "names": ["RenderSettings"], "detail": "full" }),
        &me,
    ))
    .clone();
    let properties = &schema["types"][0]["schema"]["properties"];
    assert_eq!(properties["shadows"]["default"], true, "{schema}");
    assert_eq!(properties["shadow_map_size"]["default"], 2048);
    assert_eq!(properties["shadow_map_size"]["minimum"], 64);
    assert_eq!(properties["shadow_map_size"]["maximum"], 4096);
    let found = ok(&ws.call("schema_get", &json!({ "search": "shadow quality" }), &me)).clone();
    assert!(
        found["types"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "RenderSettings"),
        "{found}"
    );
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "set_resource", "name": "RenderSettings", "value": { "shadows": false } }] }),
        &me,
    ));
    let file: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap())
            .unwrap();
    assert_eq!(
        file["resources"]["RenderSettings"],
        json!({ "shadows": false })
    );
    let flat = ok(&ws.call("capture_frame", &json!({ "width": 160, "height": 90 }), &me)).clone();
    assert_eq!(flat["stats"]["shadow_casters"], 0);
    assert_eq!(flat["stats"]["draw_calls"], 4);
    assert_ne!(
        flat["image"]["data"], shot["image"]["data"],
        "the ball's shadow is gone"
    );
    // A size outside the range is refused when it is set, with the range.
    let refused = ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "set_resource", "name": "RenderSettings", "value": { "shadow_map_size": 9000 } }] }),
        &me,
    );
    assert_eq!(refused["ok"], false, "{refused}");
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "set_resource", "name": "RenderSettings",
            "value": { "shadows": true, "shadow_map_size": 512 } }] }),
        &me,
    ));
    let sized = ok(&ws.call("capture_frame", &json!({ "width": 160, "height": 90 }), &me)).clone();
    assert_eq!(sized["stats"], shot["stats"], "shadows are back on");
    assert_ne!(
        sized["image"]["data"], shot["image"]["data"],
        "a smaller map draws a softer shadow"
    );

    std::fs::remove_dir_all(&dir).unwrap();
}

/// The warning about a second sun in an answer.
fn second_sun(answer: &Value) -> &Value {
    answer["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("no diagnostics: {answer}"))
        .iter()
        .find(|d| d["code"] == "extra_directional_lights")
        .unwrap_or_else(|| panic!("no warning: {answer}"))
}

#[test]
fn a_second_sun_is_named_by_every_way_of_looking() {
    let (dir, mut ws) = project("two-suns");
    let me = parse_actor("claude-code");
    let small = json!({ "width": 160, "height": 90 });
    let before = ok(&ws.call("capture_frame", &small, &me)).clone();
    assert!(before.get("diagnostics").is_none(), "{before}");
    // The project's scene has a light called `sun`. An agent adds a fill
    // light: `moon` comes before `sun` in the scene's order (the same
    // `order`, then by id), so it becomes the one light of the picture and
    // the sun lights nothing.
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "moon", "components": {
            "DirectionalLight": { "illuminance_lux": 500 } } }] }),
        &me,
    ));
    let two = ok(&ws.call("capture_frame", &small, &me)).clone();
    assert_ne!(two["image"]["data"], before["image"]["data"]);
    // The answer says which is which by the scene's own names (an entity
    // number would mean removing lights by trial), and where the one that
    // is not drawn is in the scene file.
    let warning = second_sun(&two).clone();
    assert_eq!(
        warning["message"],
        "The scene has 2 directional lights and only one lights the picture: entity 'moon', the first in entity order. Not drawn: entity 'sun'."
    );
    assert!(
        warning["hint"].as_str().unwrap().contains("`order`"),
        "{warning}"
    );
    assert_eq!(
        warning["location"]["path"],
        "/entities/sun/components/DirectionalLight"
    );
    // Regression (second review): a path without a file reads as a pointer
    // into the call's arguments, so the place names the scene file too.
    assert!(
        warning["location"]["file"]
            .as_str()
            .unwrap()
            .ends_with(".scene.json"),
        "{warning}"
    );
    // Regression (closing review): describe_view, which the tools tell an
    // agent to check its work with, and the debug views said nothing.
    let seen = ok(&ws.call("describe_view", &small, &me)).clone();
    assert_eq!(second_sun(&seen), &warning);
    for view in ["lights", "wireframe", "ids"] {
        let debug = ok(&ws.call(
            "capture_frame",
            &json!({ "width": 160, "height": 90, "view": view }),
            &me,
        ))
        .clone();
        assert_eq!(second_sun(&debug), &warning, "{view}");
    }
    // A running game says it too, with the same names (and no place in the
    // scene file: its world is its own).
    ok(&ws.call("sim_start", &json!({}), &me));
    let live = ok(&ws.call(
        "capture_frame",
        &json!({ "session": "s1", "width": 160, "height": 90 }),
        &me,
    ))
    .clone();
    assert_eq!(second_sun(&live)["message"], warning["message"]);
    assert!(second_sun(&live).get("location").is_none(), "{live}");
    ok(&ws.call("sim_stop", &json!({ "session": "s1" }), &me));

    // And it tells the truth: without the light it names as not drawn the
    // picture is the same, and nothing is left to warn about.
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "despawn", "id": "sun" }] }),
        &me,
    ));
    let one = ok(&ws.call("capture_frame", &small, &me)).clone();
    assert_eq!(one["image"]["data"], two["image"]["data"]);
    assert!(one.get("diagnostics").is_none(), "{one}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_answers_of_the_view_tools_are_the_same_in_every_run() {
    // A time anywhere in an answer (how long the picture took, a rate) would
    // differ between two runs, whatever its key is called. Two projects made
    // and edited the same way, each with a workspace of its own, give the
    // same answers, pictures included.
    let answers = |name: &str| -> Vec<Value> {
        let (dir, mut ws) = project(name);
        let me = parse_actor("claude-code");
        ok(&ws.call(
            "edit_apply",
            &json!({ "ops": [{ "op": "spawn", "id": "ball", "components": {
                "Primitive": { "shape": "Sphere" }, "Transform": { "translation": [0, 1, 0] } } }] }),
            &me,
        ));
        let small = json!({ "width": 160, "height": 90 });
        let mut out = vec![
            ok(&ws.call("capture_frame", &small, &me)).clone(),
            ok(&ws.call("describe_view", &small, &me)).clone(),
        ];
        for view in ["ids", "depth", "wireframe"] {
            out.push(
                ok(&ws.call(
                    "capture_frame",
                    &json!({ "width": 160, "height": 90, "view": view }),
                    &me,
                ))
                .clone(),
            );
        }
        // The same call again in the same workspace: the same answer.
        let again = ok(&ws.call("capture_frame", &small, &me)).clone();
        assert_eq!(again, out[0]);
        let seen_again = ok(&ws.call("describe_view", &small, &me)).clone();
        assert_eq!(seen_again, out[1]);
        std::fs::remove_dir_all(&dir).unwrap();
        out
    };
    let first = answers("same-a");
    let second = answers("same-b");
    assert_eq!(first.len(), 5);
    for (index, (one, other)) in first.iter().zip(&second).enumerate() {
        assert_eq!(one, other, "answer {index}");
    }
}

#[test]
fn a_tool_call_does_not_leave_a_large_picture_in_memory() {
    // Regression (closing review): the renderer keeps the targets of its
    // last frames, and a tool server that took one large picture held them
    // until 64 more view calls came, which may be never.
    const MIB: u64 = 1 << 20;
    let (dir, mut ws) = project("kept");
    let me = parse_actor("claude-code");
    assert_eq!(ws.kept_render_bytes(), 0);
    // The usual sizes stay for the next call: a small picture and its 2048
    // shadow map are 16 MiB and a little.
    let small = json!({ "width": 160, "height": 90 });
    ok(&ws.call("capture_frame", &small, &me));
    let usual = ws.kept_render_bytes();
    assert!(usual > 16 * MIB && usual < 17 * MIB, "{usual}");
    ok(&ws.call("describe_view", &small, &me));
    assert!(ws.kept_render_bytes() >= usual);
    // A shadow map of 4096 texels is 64 MiB by itself: with the picture it
    // is more than a tool call leaves behind, so the largest target goes
    // when the call ends, not when some later call begins.
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "set_resource", "name": "RenderSettings",
            "value": { "shadow_map_size": 4096 } }] }),
        &me,
    ));
    let large = ok(&ws.call("capture_frame", &small, &me)).clone();
    assert_eq!(large["stats"]["shadow_casters"], 1);
    let kept = ws.kept_render_bytes();
    assert!(kept < 64 * MIB, "{kept} bytes kept after a large picture");
    // What a later call needs again is made again: the same picture.
    let again = ok(&ws.call("capture_frame", &small, &me)).clone();
    assert_eq!(again["image"]["data"], large["image"]["data"]);
    // describe_view lets go the same way.
    ok(&ws.call("describe_view", &small, &me));
    assert!(ws.kept_render_bytes() < 64 * MIB);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The look settings of M12 (sky light, sun disc, fog, colour grading, FXAA)
/// from the control plane: documented, searchable, set with edits, written
/// to the scene file, seen in the answers, and off until asked for.
#[test]
fn the_look_settings_are_documented_set_with_edits_and_reported() {
    let (dir, mut ws) = project("look");
    let me = parse_actor("claude-code");
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "ball", "components": {
            "Primitive": { "shape": "Sphere" }, "Transform": { "translation": [0, 1, 0] } } }] }),
        &me,
    ));
    let size = json!({ "width": 160, "height": 90 });
    let plain = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_eq!(plain["stats"]["effects"], json!([]));

    // Every setting is in the schema with its documentation, range and
    // default, and every default is "off".
    let schema = ok(&ws.call(
        "schema_get",
        &json!({ "names": ["Ambient", "Sky", "Fog", "ColorGrading", "RenderSettings"], "detail": "full" }),
        &me,
    ))
    .clone();
    let properties = |name: &str| {
        schema["types"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} is not a type: {schema}"))["schema"]["properties"]
            .clone()
    };
    let fog = properties("Fog");
    assert_eq!(fog["density"]["default"], 0.0, "{fog}");
    assert_eq!(fog["density"]["x-unit"], "1/m");
    assert_eq!(
        (
            fog["density"]["minimum"].as_f64(),
            fog["density"]["maximum"].as_f64()
        ),
        (Some(0.0), Some(1.0))
    );
    assert_eq!(fog["height_falloff"]["x-unit"], "1/m");
    assert_eq!(fog["base_height"]["x-unit"], "m");
    assert_eq!(fog["start"]["x-unit"], "m");
    assert_eq!(fog["max_opacity"]["default"], 1.0);
    let grading = properties("ColorGrading");
    for (field, default, low, high) in [
        ("contrast", 1.0, 0.0, 2.0),
        ("saturation", 1.0, 0.0, 2.0),
        ("temperature", 0.0, -1.0, 1.0),
        ("tint", 0.0, -1.0, 1.0),
        ("vignette", 0.0, 0.0, 1.0),
    ] {
        let property = &grading[field];
        assert_eq!(property["default"].as_f64(), Some(default), "{field}");
        assert_eq!(property["minimum"].as_f64(), Some(low), "{field}");
        assert_eq!(property["maximum"].as_f64(), Some(high), "{field}");
    }
    let ambient = properties("Ambient");
    assert_eq!(ambient["source"]["default"], "Color", "{ambient}");
    assert_eq!(ambient["sky_strength"]["default"], 1.0);
    let sky = properties("Sky");
    assert_eq!(sky["sun_size_deg"]["default"], 0.0);
    assert_eq!(sky["sun_size_deg"]["x-unit"], "deg");
    let settings = properties("RenderSettings");
    assert_eq!(settings["antialiasing"]["default"], "Off", "{settings}");
    for (name, fields) in [
        ("Fog", &fog),
        ("ColorGrading", &grading),
        ("Ambient", &ambient),
        ("Sky", &sky),
        ("RenderSettings", &settings),
    ] {
        for (field, property) in fields.as_object().unwrap() {
            assert!(
                property["description"]
                    .as_str()
                    .is_some_and(|text| text.len() > 10),
                "{name}.{field} says what it is: {property}"
            );
        }
    }
    // The words people use find them.
    for (words, name) in [
        ("fog", "Fog"),
        ("haze", "Fog"),
        ("contrast", "ColorGrading"),
        ("white balance", "ColorGrading"),
        ("vignette", "ColorGrading"),
        ("antialiasing", "RenderSettings"),
        ("fxaa", "RenderSettings"),
        ("sun disc", "Sky"),
        ("skylight", "Ambient"),
    ] {
        let found = ok(&ws.call("schema_get", &json!({ "search": words }), &me)).clone();
        assert!(
            found["types"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["name"] == name),
            "a search for {words} finds {name}: {found}"
        );
    }

    // Set with one edit; the scene file holds what was said and nothing
    // else.
    let look = json!({ "ops": [
        { "op": "set_resource", "name": "Ambient", "value": { "source": "Sky", "sky_strength": 0.5 } },
        { "op": "set_resource", "name": "Sky", "value": { "sun_size_deg": 4 } },
        { "op": "set_resource", "name": "Fog", "value": { "density": 0.01, "height_falloff": 0.05 } },
        { "op": "set_resource", "name": "ColorGrading", "value": { "contrast": 1.1, "vignette": 0.3 } },
        { "op": "set_resource", "name": "RenderSettings", "value": { "antialiasing": "Fxaa" } }
    ] });
    ok(&ws.call("edit_apply", &look, &me));
    let file: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap())
            .unwrap();
    assert_eq!(
        file["resources"],
        json!({
            "Ambient": { "sky_strength": 0.5, "source": "Sky" },
            "ColorGrading": { "contrast": 1.1, "vignette": 0.3 },
            "Fog": { "density": 0.01, "height_falloff": 0.05 },
            "RenderSettings": { "antialiasing": "Fxaa" },
            "Sky": { "sun_size_deg": 4 }
        })
    );
    let all = json!(["sky_light", "sun_disc", "fog", "color_grading", "fxaa"]);
    let shot = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_eq!(shot["stats"]["effects"], all);
    assert_eq!(
        shot["stats"]["draw_calls"].as_u64().unwrap(),
        plain["stats"]["draw_calls"].as_u64().unwrap() + 1,
        "FXAA is one more pass"
    );
    assert_ne!(shot["image"]["data"], plain["image"]["data"]);
    assert_eq!(shot.get("diagnostics"), plain.get("diagnostics"));
    // describe_view says what the lit picture uses; a debug view uses none.
    let seen = ok(&ws.call("describe_view", &size, &me)).clone();
    assert_eq!(seen["stats"], shot["stats"]);
    let ids = ok(&ws.call(
        "capture_frame",
        &json!({ "width": 160, "height": 90, "view": "ids" }),
        &me,
    ))
    .clone();
    assert_eq!(ids["stats"]["effects"], json!([]));
    // A running game is drawn with the scene's look.
    ok(&ws.call("sim_start", &json!({}), &me));
    let live = ok(&ws.call(
        "capture_frame",
        &json!({ "session": "s1", "width": 160, "height": 90 }),
        &me,
    ))
    .clone();
    assert_eq!(live["stats"]["effects"], all);
    ok(&ws.call("sim_stop", &json!({ "session": "s1" }), &me));

    // What is out of range, or not one of the choices, is refused when it is
    // set, with what would be accepted.
    for value in [
        json!({ "name": "Fog", "value": { "density": 5 } }),
        json!({ "name": "ColorGrading", "value": { "saturation": -1 } }),
        json!({ "name": "Sky", "value": { "sun_size_deg": 90 } }),
        json!({ "name": "RenderSettings", "value": { "antialiasing": "Msaa" } }),
        json!({ "name": "Ambient", "value": { "source": "Sun" } }),
    ] {
        let mut op = value.clone();
        op["op"] = json!("set_resource");
        let refused = ws.call("edit_apply", &json!({ "ops": [op] }), &me);
        assert_eq!(refused["ok"], false, "{value}: {refused}");
        assert!(
            refused["error"]["hint"]
                .as_str()
                .is_some_and(|hint| !hint.is_empty()),
            "{refused}"
        );
    }

    // A sun disc without a sun is said, not hidden.
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "despawn", "id": "sun" }] }),
        &me,
    ));
    let sunless = ok(&ws.call("capture_frame", &size, &me)).clone();
    let warned = |answer: &Value| {
        answer["diagnostics"].as_array().is_some_and(|all| {
            all.iter().any(|d| {
                // The place is the field of the scene file that asks for
                // the disc (review of package 4).
                d["code"] == "sun_disc_without_light"
                    && d["hint"]
                        .as_str()
                        .is_some_and(|hint| hint.contains("DirectionalLight"))
                    && d["location"]["path"] == "/resources/Sky/sun_size_deg"
                    && d["location"]["file"]
                        .as_str()
                        .is_some_and(|file| file.ends_with(".scene.json"))
            })
        })
    };
    assert!(warned(&sunless), "{}", sunless["diagnostics"]);
    assert_eq!(
        sunless["stats"]["effects"],
        json!(["sky_light", "fog", "color_grading", "fxaa"])
    );
    assert!(warned(ok(&ws.call("describe_view", &size, &me))));

    // Taking the settings away again (and the ball's sun back) gives the
    // first picture, to the byte: nothing of a look stays behind.
    let mut ops: Vec<Value> = ["Ambient", "Sky", "Fog", "ColorGrading", "RenderSettings"]
        .iter()
        .map(|name| json!({ "op": "remove_resource", "name": name }))
        .collect();
    ops.push(json!({ "op": "spawn", "id": "sun", "components": {
        "DirectionalLight": { "illuminance_lux": 100000 },
        "Transform": { "rotation": [-0.4545195, 0.3304543, 0.1786597, 0.8067046] } } }));
    ok(&ws.call("edit_apply", &json!({ "ops": ops }), &me));
    let back = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_eq!(back["stats"], plain["stats"]);
    assert_eq!(back["image"]["data"], plain["image"]["data"]);

    std::fs::remove_dir_all(&dir).unwrap();
}

/// Bloom (M12, design note M12-look-2-bloom) from the control plane:
/// documented, searchable, set with an edit, written to the scene file,
/// seen in the answers, and off until asked for.
#[test]
fn bloom_is_documented_set_with_an_edit_and_reported() {
    let (dir, mut ws) = project("bloom");
    let me = parse_actor("claude-code");
    // A lamp: a ball that gives off light.
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "lamp", "components": {
            "Primitive": { "shape": "Sphere" }, "Transform": { "translation": [0, 1, 0] },
            "Material": { "base_color": [0, 0, 0, 1], "emissive": [12, 6, 1] } } }] }),
        &me,
    ));
    let size = json!({ "width": 160, "height": 90 });
    let plain = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_eq!(plain["stats"]["effects"], json!([]));

    // The setting is in the schema with its documentation, ranges and
    // defaults, and the default is "off".
    let schema = ok(&ws.call(
        "schema_get",
        &json!({ "names": ["Bloom"], "detail": "full" }),
        &me,
    ))
    .clone();
    let bloom = schema["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "Bloom")
        .unwrap_or_else(|| panic!("Bloom is not a type: {schema}"))
        .clone();
    // What does not bloom is said where an agent reads it.
    assert!(bloom.to_string().contains("particles"), "{bloom}");
    let properties = &bloom["schema"]["properties"];
    for (field, default, low, high) in [
        ("strength", 0.0, 0.0, 1.0),
        ("spread", 0.5, 0.0, 1.0),
        ("threshold", 0.0, 0.0, 64.0),
    ] {
        let property = &properties[field];
        assert_eq!(property["default"].as_f64(), Some(default), "{field}");
        assert_eq!(property["minimum"].as_f64(), Some(low), "{field}");
        assert_eq!(property["maximum"].as_f64(), Some(high), "{field}");
        assert!(
            property["description"]
                .as_str()
                .is_some_and(|text| text.len() > 10),
            "Bloom.{field} says what it is: {property}"
        );
    }
    assert_eq!(properties.as_object().unwrap().len(), 3, "{properties}");
    // The words people use find it.
    for words in ["bloom", "glow", "glare", "halo", "neon"] {
        let found = ok(&ws.call("schema_get", &json!({ "search": words }), &me)).clone();
        assert!(
            found["types"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["name"] == "Bloom"),
            "a search for {words} finds Bloom: {found}"
        );
    }

    // Set with one edit; the scene file holds what was said and nothing
    // else.
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "set_resource", "name": "Bloom", "value": { "strength": 0.15 } }] }),
        &me,
    ));
    let file: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap())
            .unwrap();
    assert_eq!(file["resources"], json!({ "Bloom": { "strength": 0.15 } }));
    let shot = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_eq!(shot["stats"]["effects"], json!(["bloom"]));
    assert_eq!(
        shot["stats"]["draw_calls"].as_u64().unwrap(),
        plain["stats"]["draw_calls"].as_u64().unwrap() + 8,
        "a picture 90 pixels high has four levels, two passes each"
    );
    assert_ne!(shot["image"]["data"], plain["image"]["data"]);
    assert_eq!(shot.get("diagnostics"), plain.get("diagnostics"));
    // The same call gives the same bytes.
    let again = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_eq!(again["image"]["data"], shot["image"]["data"]);
    // describe_view says what the lit picture uses; a debug view uses none.
    let seen = ok(&ws.call("describe_view", &size, &me)).clone();
    assert_eq!(seen["stats"], shot["stats"]);
    let ids = ok(&ws.call(
        "capture_frame",
        &json!({ "width": 160, "height": 90, "view": "ids" }),
        &me,
    ))
    .clone();
    assert_eq!(ids["stats"]["effects"], json!([]));
    // A running game is drawn with the scene's bloom.
    ok(&ws.call("sim_start", &json!({}), &me));
    let live = ok(&ws.call(
        "capture_frame",
        &json!({ "session": "s1", "width": 160, "height": 90 }),
        &me,
    ))
    .clone();
    assert_eq!(live["stats"]["effects"], json!(["bloom"]));
    ok(&ws.call("sim_stop", &json!({ "session": "s1" }), &me));

    // With the rest of the look it is named where it is drawn: after the
    // fog, before the grading.
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [
            { "op": "set_resource", "name": "Fog", "value": { "density": 0.01 } },
            { "op": "set_resource", "name": "ColorGrading", "value": { "contrast": 1.1 } },
            { "op": "set_resource", "name": "RenderSettings", "value": { "antialiasing": "Fxaa" } }
        ] }),
        &me,
    ));
    let together = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_eq!(
        together["stats"]["effects"],
        json!(["fog", "bloom", "color_grading", "fxaa"])
    );

    // What is out of range, or not a field, is refused when it is set, with
    // what would be accepted.
    for value in [
        json!({ "strength": 2 }),
        json!({ "strength": -0.1 }),
        json!({ "spread": 1.5 }),
        json!({ "threshold": 100 }),
        json!({ "radius": 4 }),
    ] {
        let refused = ws.call(
            "edit_apply",
            &json!({ "ops": [{ "op": "set_resource", "name": "Bloom", "value": value }] }),
            &me,
        );
        assert_eq!(refused["ok"], false, "{value}: {refused}");
        assert!(
            refused["error"]["hint"]
                .as_str()
                .is_some_and(|hint| !hint.is_empty()),
            "{refused}"
        );
    }

    // Taking the settings away again gives the first picture, to the byte.
    let ops: Vec<Value> = ["Bloom", "Fog", "ColorGrading", "RenderSettings"]
        .iter()
        .map(|name| json!({ "op": "remove_resource", "name": name }))
        .collect();
    ok(&ws.call("edit_apply", &json!({ "ops": ops }), &me));
    let back = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_eq!(back["stats"], plain["stats"]);
    assert_eq!(back["image"]["data"], plain["image"]["data"]);

    std::fs::remove_dir_all(&dir).unwrap();
}
