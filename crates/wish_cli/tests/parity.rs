//! Command-line parity: every control-plane tool gives the same JSON answer
//! in process, through `wen call` and through `wen do` (and, from WP6 on,
//! through the MCP server). A tool added to the catalogue without an example
//! here fails the test.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use wish_control::{Workspace, WorkspaceOptions, catalog, parse_actor};

/// A small project: wish.json, a scene with a camera and a cube, a prefab.
fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wish-parity-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::write(
        dir.join("wish.json"),
        "{\n  \"engine\": \"0.1.0\",\n  \"format\": 1,\n  \"name\": \"Parity\",\n  \"scene\": \"scenes/main.scene.json\",\n  \"types\": {\n    \"example\": \"stress\"\n  }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("scenes/main.scene.json"),
        r#"{ "format": 1, "entities": {
            "cam": { "components": { "Camera": {}, "Transform": { "translation": [0, 1, 5] } } },
            "box": { "components": { "Primitive": { "shape": "Cube" } } },
            "tri": { "components": { "Model": { "asset": "assets/tri.glb" } } }
        } }"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("assets/tri.glb"), triangle_glb()).unwrap();
    dir
}

/// One example call per tool, plus a few failing ones (errors must match
/// too). Every catalogue tool must appear. They run in this order, as one
/// session: later calls see what earlier ones changed.
fn examples() -> Vec<(&'static str, Value)> {
    vec![
        ("project_info", json!({})),
        ("schema_get", json!({})),
        (
            "schema_get",
            json!({ "names": ["Transform"], "detail": "normal" }),
        ),
        ("schema_get", json!({ "search": "light", "detail": "full" })),
        ("schema_get", json!({ "names": ["Trasform"] })),
        ("check", json!({})),
        ("check", json!({ "paths": ["scenes/main.scene.json"] })),
        ("check", json!({ "paths": ["scenes/mian.scene.json"] })),
        ("check", json!({ "pahts": [] })),
        ("scene_get", json!({ "detail": "full" })),
        ("entity_get", json!({ "ids": ["box", "cam"] })),
        ("entity_get", json!({ "ids": ["bx"] })),
        (
            "entity_query",
            json!({ "with": ["Primitive"], "components": ["Primitive"] }),
        ),
        (
            "entity_query",
            json!({ "where": "Transform.translation.z >" }),
        ),
        (
            "edit_apply",
            json!({ "ops": [{ "op": "spawn", "id": "crate", "components": {
                "Primitive": { "shape": "Cube" }, "Transform": { "translation": [0, 2, 0], "euler_deg": [0, 30, 0] } } }] }),
        ),
        (
            "edit_apply",
            json!({ "dry_run": true, "ops": [{ "op": "despawn", "id": "box" }] }),
        ),
        (
            "edit_apply",
            json!({ "ops": [{ "op": "spwan", "id": "x" }] }),
        ),
        (
            "entity_query",
            json!({ "where": "Transform.translation.y > 1" }),
        ),
        ("edit_history", json!({})),
        ("edit_history", json!({ "action": "undo" })),
        ("edit_history", json!({ "action": "redo", "dry_run": true })),
        ("scene_get", json!({})),
        ("asset_list", json!({})),
        ("asset_list", json!({ "kind": "model", "search": "TRI" })),
        ("asset_list", json!({ "kind": "modle" })),
        ("asset_info", json!({ "path": "assets/tri.glb" })),
        ("asset_info", json!({ "path": "assets/tr.glb" })),
        (
            "asset_move",
            json!({ "from": "assets/tri.glb", "to": "assets/models/tri.glb", "dry_run": true }),
        ),
        (
            "asset_move",
            json!({ "from": "assets/tri.glb", "to": "assets/models/tri.glb" }),
        ),
        (
            "asset_move",
            json!({ "from": "assets/gone.glb", "to": "assets/x.glb" }),
        ),
        ("scene_get", json!({ "detail": "normal" })),
        ("asset_info", json!({ "path": "assets/models/tri.glb" })),
        ("sim_step", json!({})),
        ("sim_start", json!({ "seed": 3 })),
        ("sim_step", json!({ "frames": 60 })),
        ("sim_snapshot", json!({ "label": "a" })),
        (
            "sim_step",
            json!({ "until": "Transform.translation.y > 100", "max_frames": 10 }),
        ),
        ("sim_snapshot", json!({ "action": "list" })),
        (
            "entity_query",
            json!({ "session": "s1", "with": ["Camera"], "components": ["Transform"] }),
        ),
        ("entity_get", json!({ "session": "s1", "ids": ["box"] })),
        ("sim_restore", json!({ "snapshot": "a", "fork": true })),
        ("sim_step", json!({ "session": "s2", "frames": 5 })),
        ("sim_restore", json!({ "session": "s1", "snapshot": "b" })),
        ("logs_get", json!({})),
        ("sim_stop", json!({ "session": "s2" })),
        ("sim_stop", json!({ "session": "s1" })),
    ]
}

fn wen() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wen"))
}

#[test]
fn every_tool_has_an_example() {
    let covered: std::collections::BTreeSet<&str> =
        examples().iter().map(|(tool, _)| *tool).collect();
    for tool in catalog() {
        assert!(
            covered.contains(tool.name),
            "the tool '{}' has no example in parity.rs",
            tool.name
        );
    }
}

fn run_do(root: &str, calls: &[(&str, Value)]) -> (Vec<String>, bool) {
    let calls: Vec<Value> = calls
        .iter()
        .map(|(tool, args)| json!({ "tool": tool, "args": args }))
        .collect();
    let mut child = wen()
        .args(["do", "-", "--root", root])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(Value::Array(calls).to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    (
        text.lines().map(str::to_owned).collect(),
        output.status.success(),
    )
}

#[test]
fn in_process_call_and_do_give_the_same_bytes() {
    let actor = parse_actor("human");
    // The whole list as one session: in process, and through `wen do`, each
    // on its own copy of the project.
    let in_process_dir = project("session-a");
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: in_process_dir.clone(),
        example: None,
    })
    .unwrap();
    let expected: Vec<String> = examples()
        .iter()
        .map(|(tool, args)| workspace.call(tool, args, &actor).to_string())
        .collect();
    let do_dir = project("session-b");
    let (lines, success) = run_do(do_dir.to_str().unwrap(), &examples());
    for (index, (line, expected)) in lines.iter().zip(&expected).enumerate() {
        assert_eq!(line, expected, "call {index}: {:?}", examples()[index]);
    }
    assert_eq!(lines.len(), expected.len());
    assert!(!success, "some example calls fail on purpose");
    // Both sessions left the same files.
    {
        let file = "scenes/main.scene.json";
        assert_eq!(
            std::fs::read_to_string(in_process_dir.join(file)).unwrap(),
            std::fs::read_to_string(do_dir.join(file)).unwrap()
        );
    }

    // `wen call`, one process per call, for every call that changes
    // nothing: against a fresh workspace on an untouched project.
    let single = project("single");
    let root = single.to_str().unwrap().to_owned();
    for (tool, args) in examples() {
        if wish_control::find_tool(tool).is_ok_and(|t| t.mutates) {
            continue;
        }
        let mut fresh = Workspace::open(WorkspaceOptions {
            root: single.clone(),
            example: None,
        })
        .unwrap();
        let expected = fresh.call(tool, &args, &actor).to_string();
        let output = wen()
            .args(["call", tool, &args.to_string(), "--root", &root])
            .output()
            .unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.trim_end(), expected, "wen call {tool} {args}");
        let ok = serde_json::from_str::<Value>(&expected).unwrap()["ok"] == json!(true);
        assert_eq!(output.status.success(), ok, "exit status of {tool}");
    }
    for dir in [in_process_dir, do_dir, single] {
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
fn flags_and_json_arguments_are_the_same_call() {
    let dir = project("flags");
    let root = dir.to_str().unwrap();
    let run = |args: &[&str]| {
        let mut all = vec!["call"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["--root", root]);
        String::from_utf8(wen().args(&all).output().unwrap().stdout).unwrap()
    };
    assert_eq!(
        run(&[
            "schema_get",
            "--names",
            "Camera,Transform",
            "--detail",
            "normal"
        ]),
        run(&[
            "schema_get",
            r#"{"names":["Camera","Transform"],"detail":"normal"}"#
        ])
    );
    // Mistakes in either form are the same diagnostics.
    let typo: Value = serde_json::from_str(&run(&["schema_get", "--detial", "full"])).unwrap();
    assert_eq!(typo["error"]["code"], "unknown_parameter");
    assert_eq!(typo["error"]["hint"], "Did you mean '--detail'?");
    let both: Value =
        serde_json::from_str(&run(&["schema_get", "{}", "--detail", "full"])).unwrap();
    assert_eq!(both["error"]["code"], "invalid_arguments");
    let unknown: Value = serde_json::from_str(&run(&["scheme_get"])).unwrap();
    assert_eq!(unknown["error"]["hint"], "Did you mean 'schema_get'?");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_broken_list_of_calls_is_explained() {
    let dir = project("broken");
    let root = dir.to_str().unwrap();
    let mut child = wen()
        .args(["do", "-", "--root", root])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"tool\":\"project_info\"}\n{\"tol\":\"check\"}\n{ nope\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(answer["error"]["code"], "invalid_arguments");
    assert_eq!(answer["error"]["location"]["line"], 3);
    std::fs::remove_dir_all(&dir).unwrap();
}
/// A one-triangle GLB with a red material, built by hand.
fn triangle_glb() -> Vec<u8> {
    let positions: Vec<u8> = [-1.0f32, -1.0, 0.0, 1.0, -1.0, 0.0, 0.0, 1.5, 0.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let json = serde_json::json!({
        "asset": { "version": "2.0" },
        "buffers": [{ "byteLength": 36 }],
        "bufferViews": [{ "buffer": 0, "byteLength": 36 }],
        "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [-1, -1, 0], "max": [1, 1.5, 0] }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "material": 0 }] }],
        "materials": [{ "doubleSided": true, "pbrMetallicRoughness": { "baseColorFactor": [0.9, 0.1, 0.1, 1.0], "metallicFactor": 0.0 } }],
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

#[test]
fn wen_new_makes_a_project_the_tools_can_use() {
    let dir = std::env::temp_dir().join(format!("wish-new-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.to_str().unwrap();
    let output = wen()
        .args(["new", root, "--name", "Fresh", "--json"])
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["name"], "Fresh");
    // The MCP configuration names this very program, by full path.
    let mcp: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(".mcp.json")).unwrap()).unwrap();
    let same = |a: &str, b: &str| a.replace('\\', "/") == b.replace('\\', "/");
    assert!(same(
        mcp["mcpServers"]["wish"]["command"].as_str().unwrap(),
        env!("CARGO_BIN_EXE_wen")
    ));
    // And the shortcut commands work in it, answering as the tools do.
    let check = wen()
        .args(["check", "--example", "stress", "--root", root, "--json"])
        .output()
        .unwrap();
    assert!(check.status.success());
    let answer: Value = serde_json::from_slice(&check.stdout).unwrap();
    assert_eq!(answer["passed"], true);
    let again = wen().args(["new", root, "--json"]).output().unwrap();
    assert!(!again.status.success());
    let problem: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(problem["error"]["code"], "project_exists");
    let missing = wen().args(["new", "--json"]).output().unwrap();
    assert!(!missing.status.success());
    std::fs::remove_dir_all(&dir).unwrap();
}
