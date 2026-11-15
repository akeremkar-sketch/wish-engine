//! Asset tools and the project scaffold.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

/// A one-triangle GLB, built by hand.
fn triangle_glb() -> Vec<u8> {
    let positions: Vec<u8> = [0.0f32, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let json = json!({
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_materials_clearcoat"],
        "buffers": [{ "byteLength": 36 }],
        "bufferViews": [{ "buffer": 0, "byteLength": 36 }],
        "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [2, 3, 0] }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }],
        "nodes": [{ "mesh": 0 }],
        "scenes": [{ "nodes": [0] }]
    });
    let mut text = serde_json::to_vec(&json).unwrap();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + 8 + positions.len();
    let mut out = b"glTF".to_vec();
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&(positions.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&positions);
    out
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wish-asset-tools-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn open(dir: &std::path::Path) -> Workspace {
    Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .unwrap()
}

#[test]
fn a_new_project_checks_clean_and_is_ready_for_an_agent() {
    let dir = folder("new");
    let files = create_project(&dir, "Test Game", "/opt/wen").unwrap();
    assert!(files.contains(&".mcp.json".to_owned()) && files.contains(&"CLAUDE.md".to_owned()));
    let mcp: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(mcp["mcpServers"]["wish"]["command"], "/opt/wen");
    assert_eq!(mcp["mcpServers"]["wish"]["args"], json!(["mcp"]));
    let guide = std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
    assert!(guide.starts_with("# Test Game") && guide.contains("project_info"));

    let mut ws = open(&dir);
    let actor = parse_actor("claude-code");
    let info = ws.call("project_info", &json!({}), &actor);
    assert_eq!(info["project"]["name"], "Test Game");
    assert_eq!(info["problems"], json!({ "errors": 0, "warnings": 0 }));
    let check = ws.call("check", &json!({}), &actor);
    assert_eq!(check["passed"], true, "{check}");
    assert_eq!(check["warnings"], 0, "{check}");
    // The starting scene can be worked on straight away.
    let scene = ws.call("scene_get", &json!({}), &actor);
    assert_eq!(scene["entities"], 3);
    // Creating again in the same folder is refused, and nothing is lost.
    let again = create_project(&dir, "Other", "wen").unwrap_err();
    assert_eq!(again.code, "project_exists");
    assert!(
        std::fs::read_to_string(dir.join("CLAUDE.md"))
            .unwrap()
            .starts_with("# Test Game")
    );
    for bad in ["", "   ", "line\nbreak"] {
        assert_eq!(
            create_project(&folder("bad"), bad, "wen").unwrap_err().code,
            "invalid_project_name"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn assets_are_listed_described_and_moved_with_their_references() {
    let dir = folder("assets");
    create_project(&dir, "A", "wen").unwrap();
    std::fs::write(dir.join("assets/tri.glb"), triangle_glb()).unwrap();
    std::fs::write(dir.join("assets/notes.txt"), "hi").unwrap();
    std::fs::write(
        dir.join("assets/tri.glb.meta.json"),
        "{ \"format\": 1, \"import\": { \"scale\": 2 } }",
    )
    .unwrap();
    // Temporary files of editors are not assets.
    std::fs::write(dir.join("assets/tri.glb.part"), "x").unwrap();
    let mut ws = open(&dir);
    let actor = parse_actor("claude-code");
    ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "tri", "components": { "Model": { "asset": "assets/tri.glb" } } }] }),
        &actor,
    );

    let list = ws.call("asset_list", &json!({}), &actor);
    let paths: Vec<&str> = list["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["assets/notes.txt", "assets/tri.glb"], "{list}");
    let models = ws.call("asset_list", &json!({ "kind": "model" }), &actor);
    assert_eq!(models["matched"], 1);
    assert_eq!(
        ws.call("asset_list", &json!({ "search": "NOTES" }), &actor)["matched"],
        1
    );

    let info = ws.call("asset_info", &json!({ "path": "assets/tri.glb" }), &actor);
    assert_eq!(info["ok"], true, "{info}");
    assert_eq!(info["model"]["triangles"], 1);
    assert_eq!(info["model"]["size_m"], json!([2.0, 3.0, 0.0]));
    assert_eq!(info["import_settings"], json!({ "scale": 2 }));
    assert_eq!(info["used_by"], json!(["scenes/main.scene.json"]));
    assert_eq!(info["diagnostics"][0]["code"], "unsupported_extension");
    let missing = ws.call("asset_info", &json!({ "path": "assets/tr.glb" }), &actor);
    assert_eq!(missing["error"]["hint"], "Did you mean 'assets/tri.glb'?");

    // Move: the preview changes nothing, the move rewrites the reference, and
    // the open scene catches up with the file.
    let before = std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap();
    let preview = ws.call(
        "asset_move",
        &json!({ "from": "assets/tri.glb", "to": "assets/models/tri.glb", "dry_run": true }),
        &actor,
    );
    assert_eq!(preview["documents"][0]["references"], 1);
    assert!(dir.join("assets/tri.glb").is_file());
    assert_eq!(
        std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap(),
        before
    );
    let moved = ws.call(
        "asset_move",
        &json!({ "from": "assets/tri.glb", "to": "assets/models/tri.glb" }),
        &actor,
    );
    assert_eq!(moved["ok"], true, "{moved}");
    assert_eq!(moved["meta"]["to"], "assets/models/tri.glb.meta.json");
    assert!(dir.join("assets/models/tri.glb").is_file() && !dir.join("assets/tri.glb").exists());
    let scene = ws.call("entity_get", &json!({ "ids": ["tri"] }), &actor);
    assert_eq!(
        scene["entities"][0]["components"]["Model"]["asset"],
        "assets/models/tri.glb"
    );
    assert_eq!(ws.call("check", &json!({}), &actor)["passed"], true);
    // The move is in the scene's history as the agent's own change, and is
    // not reported back to it as someone else's (M4 closing review).
    let history = ws.call("edit_history", &json!({}), &actor);
    assert_eq!(history["entries"][0]["actor"], "agent:claude-code");
    let next = ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "after_move" }] }),
        &actor,
    );
    assert!(next.get("changed_by_others").is_none(), "{next}");
    let bad = ws.call(
        "asset_move",
        &json!({ "from": "assets/gone.glb", "to": "assets/x.glb" }),
        &actor,
    );
    assert_eq!(bad["ok"], false);
    assert!(!bad["error"]["hint"].as_str().unwrap().is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}
