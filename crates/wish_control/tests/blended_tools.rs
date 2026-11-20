//! See-through surfaces through the control plane (M12 package 8): asked
//! for by a material file and by a plain `Material`, counted in the
//! pictures' statistics, marked in `describe_view`, and found by people's
//! words.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = std::env::temp_dir().join(format!("wish-blended-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "B", "wen").unwrap();
    // No game code: these projects are about looks.
    std::fs::remove_dir_all(dir.join("scripts")).unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    (dir, ws)
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    ws.call(tool, &args, &parse_actor("agent:glazier"))
}

fn ok(answer: Value) -> Value {
    assert_eq!(answer["ok"], true, "{answer}");
    answer
}

#[test]
fn glass_is_made_drawn_counted_and_described() {
    let (dir, mut ws) = project("glass");
    // A material file that blends: written as asked, read back the same.
    let made = ok(call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/glass.mat.json",
            "material": { "base_color": [0.6, 0.8, 0.9, 0.3], "alpha_mode": "Blend" } }),
    ));
    assert!(made.get("diagnostics").is_none(), "{made}");
    let text = std::fs::read_to_string(dir.join("assets/glass.mat.json")).unwrap();
    assert_eq!(
        text,
        "{\n  \"format\": 1,\n  \"material\": {\n    \"alpha_mode\": \"Blend\",\n    \"base_color\": [0.6, 0.8, 0.9, 0.3]\n  }\n}\n"
    );
    // Before: nothing is see-through, and the answers say 0.
    let before = ok(call(
        &mut ws,
        "capture_frame",
        json!({ "width": 96, "height": 54 }),
    ));
    assert_eq!(before["stats"]["blended_surfaces"], 0, "{before}");
    // A window of that file in front of the camera, and a ghost whose
    // plain Material is half see-through, with a box behind the window.
    ok(call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "spawn", "id": "window", "components": {
                "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [0, 1.5, 3], "scale": [3, 2, 0.05] },
                "Material": { "file": "assets/glass.mat.json" } } },
            { "op": "spawn", "id": "box", "components": {
                "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [0, 1.5, 0] },
                "Material": { "base_color": [0.8, 0.2, 0.1, 1] } } },
            { "op": "spawn", "id": "ghost", "components": {
                "Primitive": { "shape": "Sphere" },
                "Transform": { "translation": [-2.5, 1, 0] },
                "Material": { "base_color": [0.9, 0.9, 1, 0.5] } } }
        ] }),
    ));
    let checked = ok(call(&mut ws, "check", json!({})));
    assert_eq!(checked["passed"], true, "{checked}");
    let shot = ok(call(
        &mut ws,
        "capture_frame",
        json!({ "width": 96, "height": 54 }),
    ));
    assert!(shot.get("diagnostics").is_none(), "{shot}");
    assert_eq!(shot["stats"]["blended_surfaces"], 2, "{shot}");
    let ids = ok(call(
        &mut ws,
        "capture_frame",
        json!({ "width": 96, "height": 54, "view": "ids" }),
    ));
    assert_eq!(ids["stats"]["blended_surfaces"], 2, "{ids}");

    // The box behind the window is visible, and wholly: the window does not
    // hide it. The window and the ghost are marked; the box is not.
    let seen = ok(call(
        &mut ws,
        "describe_view",
        json!({ "width": 96, "height": 54 }),
    ));
    let entry = |name: &str| {
        seen["visible"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["entity"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("{name} is not visible: {seen}"))
    };
    assert_eq!(entry("window")["blended"], true);
    assert_eq!(entry("ghost")["blended"], true);
    assert!(entry("box").get("blended").is_none(), "{seen}");
    assert_eq!(entry("box")["visible_percent"], 100.0, "{seen}");
    assert_eq!(seen["stats"]["blended_surfaces"], 2);

    // Asked again, the same answer: counts, no time.
    let again = ok(call(
        &mut ws,
        "describe_view",
        json!({ "width": 96, "height": 54 }),
    ));
    assert_eq!(again, seen);
}

#[test]
fn people_find_see_through_surfaces_by_their_words() {
    let (_dir, mut ws) = project("words");
    for words in ["glass", "transparent", "see-through", "fade"] {
        let found = ok(call(&mut ws, "schema_get", json!({ "search": words })));
        let names: Vec<&str> = found["types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Material"), "{words}: {names:?}");
    }
    let found = ok(call(
        &mut ws,
        "docs_search",
        json!({ "query": "glass window" }),
    ));
    assert_eq!(found["results"][0]["uri"], "wish://docs/recipes", "{found}");
    // A component from another engine's way of doing it leads here.
    for name in ["Transparency", "Opacity", "Glass"] {
        let unknown = call(&mut ws, "schema_get", json!({ "names": [name] }));
        assert!(
            unknown.to_string().contains("base_color alpha below 1"),
            "{name}: {unknown}"
        );
    }
    // The material file's page says what Blend does.
    let page = wish_control::docs::type_page(ws.registry(), "MaterialFile").unwrap();
    assert!(page.contains("(`Blend`)"), "{page}");
}
