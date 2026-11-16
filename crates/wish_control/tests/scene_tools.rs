//! The scene tools on real files: building a scene, querying it, editing it
//! next to a person who edits the file by hand, undo across both.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

const PROJECT: &str = "{\n  \"engine\": \"0.1.0\",\n  \"format\": 1,\n  \"name\": \"T\",\n  \"scene\": \"scenes/main.scene.json\",\n  \"types\": {}\n}\n";

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wish-scene-tools-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::create_dir_all(dir.join("prefabs")).unwrap();
    std::fs::write(dir.join("wish.json"), PROJECT).unwrap();
    std::fs::write(
        dir.join("scenes/main.scene.json"),
        r#"{ "format": 1, "entities": {
            "cam": { "components": { "Camera": {}, "Transform": { "translation": [0, 1, 6] } } }
        } }"#,
    )
    .unwrap();
    dir
}

fn open(dir: &Path) -> Workspace {
    Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .unwrap()
}

fn ok(answer: &Value) -> &Value {
    assert_eq!(answer["ok"], true, "{answer}");
    answer
}

fn scene_text(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap()
}

/// Writes the scene file the way an editor does (whole file at once).
fn hand_edit(dir: &Path, text: &str) {
    let path = dir.join("scenes/main.scene.json");
    let part = dir.join("scenes/main.scene.json.part");
    std::fs::write(&part, text).unwrap();
    std::fs::rename(&part, &path).unwrap();
}

#[test]
fn an_agent_builds_a_scene_and_the_file_follows() {
    let dir = project("build");
    let mut ws = open(&dir);
    let agent = parse_actor("claude-code");
    let answer = ws.call(
        "edit_apply",
        &json!({ "ops": [
            { "op": "spawn", "id": "level" },
            { "op": "spawn", "id": "floor", "parent": "level", "components": {
                "Primitive": { "shape": "Plane" }, "Transform": { "scale": [20, 1, 20] } } },
            { "op": "spawn", "id": "crate_1", "parent": "level", "components": {
                "Primitive": { "shape": "Cube" }, "Transform": { "translation": [0, 0.5, 0], "euler_deg": [0, 45, 0] } } },
            { "op": "spawn", "id": "crate_2", "parent": "level", "components": {
                "Primitive": { "shape": "Cube" }, "Transform": { "translation": [3, 0.5, -2] } } },
            { "op": "set_resource", "name": "Sky", "value": { "luminance": 4000 } }
        ] }),
        &agent,
    );
    ok(&answer);
    assert_eq!(answer["revision"], 1);
    assert_eq!(
        answer["changes"]["entities"],
        json!(["crate_1", "crate_2", "floor", "level", "resource:Sky"])
    );
    // Written at once, canonical, with a quaternion (no euler_deg).
    let text = scene_text(&dir);
    assert!(wish_scene::json::is_canonical(&text));
    assert!(
        text.contains("\"crate_1\"") && text.contains("0.382683"),
        "{text}"
    );
    assert!(!text.contains("euler_deg"));
    assert_eq!(ok(&ws.call("check", &json!({}), &agent))["passed"], true);

    // The outline.
    let outline = ok(&ws.call("scene_get", &json!({}), &agent)).clone();
    assert_eq!(outline["entities"], 5);
    let level = outline["tree"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == "level")
        .unwrap();
    assert_eq!(level["children"].as_array().unwrap().len(), 3);
    assert_eq!(outline["resources"], json!(["Sky"]));
    let shallow =
        ok(&ws.call("scene_get", &json!({ "root": "level", "depth": 1 }), &agent)).clone();
    assert_eq!(shallow["tree"][0]["children_hidden"], 3);
    let normal = ok(&ws.call(
        "scene_get",
        &json!({ "root": "floor", "detail": "normal" }),
        &agent,
    ))
    .clone();
    assert_eq!(
        normal["tree"][0]["components"]["Primitive"]["shape"],
        "Plane"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn queries_find_by_component_condition_name_and_distance() {
    let dir = project("query");
    let mut ws = open(&dir);
    let me = parse_actor("human");
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [
            { "op": "spawn", "id": "crate_1", "components": { "Primitive": { "shape": "Cube" }, "Transform": { "translation": [1, 0, 0] } } },
            { "op": "spawn", "id": "crate_2", "components": { "Primitive": { "shape": "Cube" }, "Transform": { "translation": [5, -3, 0] } } },
            { "op": "spawn", "id": "ball", "components": { "Primitive": { "shape": "Sphere" }, "Transform": { "translation": [2, 0, 0] } } },
            { "op": "spawn", "id": "group", "components": { "Transform": { "translation": [0, 10, 0] } } },
            { "op": "spawn", "id": "child", "parent": "group", "components": { "Transform": { "translation": [0, 1, 0] } } }
        ] }),
        &me,
    ));
    let ids = |answer: &Value| -> Vec<String> {
        ok(answer)["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        ids(&ws.call("entity_query", &json!({ "with": ["Primitive"] }), &me)),
        ["ball", "crate_1", "crate_2"]
    );
    assert_eq!(
        ids(&ws.call("entity_query", &json!({ "name": "crate_*" }), &me)),
        ["crate_1", "crate_2"]
    );
    assert_eq!(
        ids(&ws.call(
            "entity_query",
            &json!({ "where": "Transform.translation.y < 0" }),
            &me
        )),
        ["crate_2"]
    );
    assert_eq!(
        ids(&ws.call(
            "entity_query",
            &json!({ "where": "has(Camera) || Primitive.shape == \"Sphere\"" }),
            &me
        )),
        ["ball", "cam"]
    );
    // World positions: the child sits at y = 11 under its parent.
    let near = ws.call(
        "entity_query",
        &json!({ "near": { "point": [0, 11, 0], "radius": 0.5 } }),
        &me,
    );
    assert_eq!(ids(&near), ["child"]);
    let nearest = ws.call(
        "entity_query",
        &json!({ "near": { "point": [0, 0, 0], "radius": 3 }, "with": ["Primitive"] }),
        &me,
    );
    assert_eq!(ids(&nearest), ["crate_1", "ball"]);
    assert_eq!(nearest["entities"][0]["distance"], 1.0);
    // Paging.
    let first = ws.call("entity_query", &json!({ "limit": 2 }), &me);
    assert_eq!(first["matched"], 6);
    let cursor = first["next_cursor"].as_str().unwrap().to_owned();
    let rest = ws.call(
        "entity_query",
        &json!({ "limit": 10, "cursor": cursor }),
        &me,
    );
    assert_eq!(ids(&first).len() + ids(&rest).len(), 6);
    assert!(rest.get("next_cursor").is_none());
    // Values come along when asked.
    let with_values = ws.call(
        "entity_query",
        &json!({ "name": "ball", "components": ["Transform"] }),
        &me,
    );
    assert_eq!(
        with_values["entities"][0]["components"]["Transform"]["translation"],
        json!([2.0, 0.0, 0.0])
    );
    // Mistakes.
    let bad = ws.call(
        "entity_query",
        &json!({ "where": "Transform.translaton.y < 0" }),
        &me,
    );
    assert_eq!(bad["error"]["code"], "expr_unknown_name");
    assert_eq!(bad["error"]["location"]["path"], "/where");
    assert_eq!(bad["error"]["location"]["column"], 11);
    assert_eq!(bad["error"]["hint"], "Did you mean 'translation'?");
    let bad = ws.call("entity_query", &json!({ "with": ["Primitve"] }), &me);
    assert_eq!(bad["error"]["hint"], "Did you mean 'Primitive'?");
    let bad = ws.call("entity_query", &json!({ "near": { "point": [0, 0] } }), &me);
    assert_eq!(bad["error"]["code"], "invalid_parameter");
    let bad = ws.call("entity_query", &json!({ "cursor": "after:nobody" }), &me);
    assert_eq!(bad["error"]["code"], "invalid_cursor");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn entity_get_gives_effective_values_and_names_the_closest_id() {
    let dir = project("get");
    let mut ws = open(&dir);
    let me = parse_actor("human");
    let answer = ok(&ws.call("entity_get", &json!({ "ids": ["cam"] }), &me)).clone();
    let camera = &answer["entities"][0]["components"]["Camera"];
    // The file gives no Camera fields: the defaults are filled in.
    assert_eq!(camera["fov_y_deg"], 60.0);
    assert!(answer["entities"][0]["components"].get("SceneId").is_none());
    let only = ok(&ws.call(
        "entity_get",
        &json!({ "ids": ["cam"], "components": ["GlobalTransform"] }),
        &me,
    ))
    .clone();
    assert!(only["entities"][0]["components"]["GlobalTransform"].is_object());
    let bad = ws.call("entity_get", &json!({ "ids": ["cma"] }), &me);
    assert_eq!(bad["error"]["code"], "unknown_entity");
    assert_eq!(bad["error"]["hint"], "Did you mean 'cam'?");
    assert_eq!(bad["error"]["location"]["path"], "/ids/0");
    let bad = ws.call(
        "entity_get",
        &json!({ "scene": "scenes/mian.scene.json", "ids": ["cam"] }),
        &me,
    );
    assert_eq!(bad["error"]["code"], "scene_not_found");
    assert!(
        bad["error"]["hint"]
            .as_str()
            .unwrap()
            .starts_with("Did you mean 'scenes/main.scene.json'?")
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn failed_dry_run_and_stale_edits_change_nothing() {
    let dir = project("refuse");
    let mut ws = open(&dir);
    let me = parse_actor("claude-code");
    let before = scene_text(&dir);
    let preview = ok(&ws.call(
        "edit_apply",
        &json!({ "dry_run": true, "ops": [{ "op": "spawn", "id": "x" }] }),
        &me,
    ))
    .clone();
    assert_eq!(preview["dry_run"], true);
    assert_eq!(preview["revision"], 1);
    assert_eq!(scene_text(&dir), before, "a dry run writes nothing");
    // The second operation is wrong: nothing happens, and it says which.
    let failed = ws.call(
        "edit_apply",
        &json!({ "ops": [
            { "op": "spawn", "id": "a" },
            { "op": "set", "id": "a", "component": "Camera", "value": { "fov_y_deg": 500 } }
        ] }),
        &me,
    );
    assert_eq!(failed["ok"], false);
    assert_eq!(failed["error"]["location"]["op"], 1);
    assert!(!failed["error"]["hint"].as_str().unwrap().is_empty());
    assert_eq!(scene_text(&dir), before);
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "b" }] }),
        &me,
    ));
    let stale = ws.call(
        "edit_apply",
        &json!({ "base_revision": 0, "ops": [{ "op": "despawn", "id": "b" }] }),
        &me,
    );
    assert_eq!(stale["error"]["code"], "stale_revision");
    let stale = ws.call(
        "edit_apply",
        &json!({ "dry_run": true, "base_revision": 0, "ops": [{ "op": "despawn", "id": "b" }] }),
        &me,
    );
    assert_eq!(stale["error"]["code"], "stale_revision");
    let wrong_op = ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spwan", "id": "c" }] }),
        &me,
    );
    assert_eq!(wrong_op["error"]["hint"], "Did you mean 'spawn'?");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_person_editing_the_file_and_an_agent_share_one_history() {
    let dir = project("shared");
    let mut ws = open(&dir);
    let agent = parse_actor("claude-code");
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "crate", "components": { "Transform": { "translation": [0, 0, 0] } } }] }),
        &agent,
    ));
    // A person moves the crate and adds a lamp in a text editor.
    let text = scene_text(&dir).replace("\"translation\": [0, 0, 0]", "\"translation\": [4, 0, 0]");
    assert_ne!(
        text,
        scene_text(&dir),
        "the test must really change the file"
    );
    let text = text.replacen(
        "\"entities\": {",
        "\"entities\": {\n    \"lamp\": { \"components\": { \"Primitive\": { \"shape\": \"Sphere\" } } },",
        1,
    );
    hand_edit(&dir, &text);

    // The agent's next change tells it what the person did.
    let answer = ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "set", "id": "crate", "component": "Transform", "value": { "scale": [2, 2, 2] } }] }),
        &agent,
    ))
    .clone();
    assert_eq!(answer["changed_by_others"]["by"], json!(["file"]));
    assert_eq!(
        answer["changed_by_others"]["entities"],
        json!(["crate", "lamp"])
    );
    // The agent's own change kept the person's move.
    let crate_now = ok(&ws.call("entity_get", &json!({ "ids": ["crate"] }), &agent)).clone();
    let transform = &crate_now["entities"][0]["components"]["Transform"];
    assert_eq!(transform["translation"], json!([4.0, 0.0, 0.0]));
    assert_eq!(transform["scale"], json!([2.0, 2.0, 2.0]));

    // One history, three actors' worth.
    let history = ok(&ws.call("edit_history", &json!({}), &agent)).clone();
    let actors: Vec<&str> = history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["actor"].as_str().unwrap())
        .collect();
    assert_eq!(actors, ["agent:claude-code", "file", "agent:claude-code"]);
    // Undo twice: the agent's scale, then the person's file change.
    let undo = ok(&ws.call(
        "edit_history",
        &json!({ "action": "undo", "steps": 2, "dry_run": true }),
        &agent,
    ))
    .clone();
    assert_eq!(undo["steps_done"], 2);
    assert!(scene_text(&dir).contains("lamp"), "dry run");
    ok(&ws.call(
        "edit_history",
        &json!({ "action": "undo", "steps": 2 }),
        &agent,
    ));
    assert!(
        !scene_text(&dir).contains("lamp"),
        "the person's lamp is undone too"
    );
    let redo = ok(&ws.call("edit_history", &json!({ "action": "redo" }), &agent)).clone();
    assert_eq!(redo["steps_done"], 1);
    assert!(scene_text(&dir).contains("lamp"));
    let nothing = ok(&ws.call(
        "edit_history",
        &json!({ "action": "redo", "steps": 5 }),
        &agent,
    ))
    .clone();
    assert_eq!(nothing["steps_done"], 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_broken_hand_edit_is_never_overwritten() {
    let dir = project("broken");
    let mut ws = open(&dir);
    let agent = parse_actor("claude-code");
    ok(&ws.call("scene_get", &json!({}), &agent));
    hand_edit(&dir, "{ \"format\": 1, \"entities\": { \"cam\": ");
    // Reading still works, with a warning.
    let outline = ok(&ws.call("scene_get", &json!({}), &agent)).clone();
    assert_eq!(outline["entities"], 1);
    assert_eq!(
        outline["diagnostics"][0]["code"],
        "external_change_rejected"
    );
    // Changing is refused; the half-written file stays as it is.
    let refused = ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "x" }] }),
        &agent,
    );
    assert_eq!(refused["error"]["code"], "scene_file_invalid");
    assert_eq!(
        scene_text(&dir),
        "{ \"format\": 1, \"entities\": { \"cam\": "
    );
    // Once the person fixes it, work goes on.
    hand_edit(
        &dir,
        "{ \"format\": 1, \"entities\": { \"cam\": { \"components\": { \"Camera\": {} } } } }",
    );
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "x" }] }),
        &agent,
    ));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn new_scenes_and_prefab_instances() {
    let dir = project("prefab");
    std::fs::write(
        dir.join("prefabs/cart.prefab.json"),
        r#"{ "format": 1, "entities": {
            "root": { "components": { "Transform": {} } },
            "wheel": { "parent": "root", "components": { "Primitive": { "shape": "Sphere" }, "Transform": { "translation": [1, 0, 0] } } }
        } }"#,
    )
    .unwrap();
    let mut ws = open(&dir);
    let me = parse_actor("claude-code");
    let missing = ws.call(
        "scene_get",
        &json!({ "scene": "scenes/level2.scene.json" }),
        &me,
    );
    assert_eq!(missing["error"]["code"], "scene_not_found");
    assert!(
        missing["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("create")
    );
    ok(&ws.call(
        "edit_apply",
        &json!({ "scene": "scenes/level2.scene.json", "create": true, "ops": [
            { "op": "instantiate", "id": "cart", "prefab": "prefabs/cart.prefab.json",
              "components": { "Transform": { "translation": [0, 0, -5] } },
              "overrides": { "wheel": { "Primitive": { "shape": "Cube" } } } }
        ] }),
        &me,
    ));
    assert!(dir.join("scenes/level2.scene.json").is_file());
    let wheel = ok(&ws.call(
        "entity_get",
        &json!({ "scene": "scenes/level2.scene.json", "ids": ["cart/wheel"] }),
        &me,
    ))
    .clone();
    assert_eq!(wheel["entities"][0]["from_prefab"], true);
    assert_eq!(
        wheel["entities"][0]["components"]["Primitive"]["shape"],
        "Cube"
    );
    let found = ok(&ws.call(
        "entity_query",
        &json!({ "scene": "scenes/level2.scene.json", "where": "Transform.translation.x == 1" }),
        &me,
    ))
    .clone();
    assert_eq!(found["entities"][0]["id"], "cart/wheel");
    let bad = ws.call(
        "scene_get",
        &json!({ "scene": "../outside.scene.json" }),
        &me,
    );
    assert_eq!(bad["error"]["code"], "invalid_scene_path");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Found by the M4 agent evaluations: agents tried to read a prefab with
/// file tools (there was no tool for it), asked for components other
/// engines have, and used set_override on a prefab that has only a root.
#[test]
fn what_the_agent_evaluations_found() {
    let dir = project("evals");
    std::fs::write(
        dir.join("prefabs/crate.prefab.json"),
        r#"{ "format": 1, "entities": { "root": { "components": { "Primitive": { "shape": "Cube" }, "Transform": {} } } } }"#,
    )
    .unwrap();
    let mut ws = open(&dir);
    let me = parse_actor("claude-code");
    // A prefab can be looked at...
    let outline = ok(&ws.call(
        "scene_get",
        &json!({ "scene": "prefabs/crate.prefab.json", "detail": "normal" }),
        &me,
    ))
    .clone();
    assert_eq!(outline["tree"][0]["id"], "root");
    assert_eq!(
        outline["tree"][0]["components"]["Primitive"]["shape"],
        "Cube"
    );
    let values = ok(&ws.call(
        "entity_get",
        &json!({ "scene": "prefabs/crate.prefab.json", "ids": ["root"] }),
        &me,
    ))
    .clone();
    assert_eq!(
        values["entities"][0]["components"]["Transform"]["scale"],
        json!([1.0, 1.0, 1.0])
    );
    // ...and changed (M8a): a prefab is edited like a scene, and the scenes
    // that have instances of it see the change at once.
    let edited = ws.call(
        "edit_apply",
        &json!({ "scene": "prefabs/crate.prefab.json", "ops": [{
            "op": "set", "id": "root", "component": "Primitive", "value": { "shape": "Sphere" } }] }),
        &me,
    );
    assert_eq!(edited["ok"], true, "{edited}");
    let after = ok(&ws.call(
        "scene_get",
        &json!({ "scene": "prefabs/crate.prefab.json", "detail": "normal" }),
        &me,
    ))
    .clone();
    assert_eq!(
        after["tree"][0]["components"]["Primitive"]["shape"],
        "Sphere"
    );
    // Familiar names from other engines are explained.
    let collider = ws.call("entity_query", &json!({ "with": ["BoxCollider"] }), &me);
    assert!(
        collider["error"]["hint"]
            .as_str()
            .unwrap()
            .starts_with("Physics"),
        "{collider}"
    );
    let name = ws.call("schema_get", &json!({ "names": ["Name"] }), &me);
    assert!(
        name["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("named by their id"),
        "{name}"
    );
    let visibility = ws.call(
        "entity_query",
        &json!({ "where": "Visibility.visible == true" }),
        &me,
    );
    assert!(
        visibility["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("Primitive"),
        "{visibility}"
    );
    // An override on a root-only prefab says what to do instead.
    let override_root = ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "instantiate", "id": "c1", "prefab": "prefabs/crate.prefab.json",
            "overrides": { "c1": { "Transform": { "translation": [1, 0, 0] } } } }] }),
        &me,
    );
    assert_eq!(override_root["error"]["code"], "unknown_prefab_entity");
    assert!(
        override_root["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("only its root"),
        "{override_root}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
