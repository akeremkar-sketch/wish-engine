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
        ("docs_search", json!({ "query": "field of view" })),
        ("docs_search", json!({ "query": "zzzz" })),
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
        ("capture_frame", json!({ "width": 64, "height": 36 })),
        (
            "capture_frame",
            json!({ "width": 64, "height": 36, "view": "ids", "out": "captures/ids.png", "overwrite": true }),
        ),
        ("capture_frame", json!({ "camera": "box" })),
        ("capture_frame", json!({ "out": "scenes/main.scene.json" })),
        ("describe_view", json!({ "width": 64, "height": 36 })),
        (
            "physics_query",
            json!({ "ray": { "from": [0, 10, 0], "dir": [0, -1, 0] } }),
        ),
        ("physics_query", json!({ "touching": "box" })),
        (
            "physics_query",
            json!({ "ray": { "from": [0, 1, 0], "dir": [0, 0, 0] } }),
        ),
        ("logs_get", json!({})),
        // Replays: record a short run, save it, play it back.
        ("sim_start", json!({})),
        ("replay", json!({ "action": "record", "session": "s3" })),
        (
            "sim_step",
            json!({ "session": "s3", "frames": 5, "input": { "move": [0, 1] } }),
        ),
        (
            "replay",
            json!({ "action": "save", "session": "s3", "name": "parity" }),
        ),
        ("replay", json!({ "action": "run", "name": "parity" })),
        ("replay", json!({ "action": "run", "name": "nothing" })),
        ("sim_stop", json!({ "session": "s3" })),
        // Scenario tests: none yet, then one that cannot be found.
        ("test_run", json!({})),
        ("test_run", json!({ "paths": ["tests/win.test.json"] })),
        // Input: a known action is held, an unknown one is explained.
        (
            "sim_step",
            json!({ "session": "s1", "frames": 2, "input": { "move": [0, 1], "jump": true } }),
        ),
        (
            "sim_step",
            json!({ "session": "s1", "input": { "jmup": true } }),
        ),
        ("sim_stop", json!({ "session": "s2" })),
        ("sim_stop", json!({ "session": "s1" })),
        // Building needs the SDK pack and the compiler, which differ between
        // machines; the parity of the answer is shown on a mistake (building
        // itself is tested in wish_control/tests/script_tools.rs).
        ("script_build", json!({ "reload": "yes" })),
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
        .args(["do", "-", "--software", "--root", root])
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
    workspace.set_software(true);
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
        fresh.set_software(true);
        let expected = fresh.call(tool, &args, &actor).to_string();
        let output = wen()
            .args([
                "call",
                tool,
                &args.to_string(),
                "--software",
                "--root",
                &root,
            ])
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
        .args(["do", "-", "--software", "--root", root])
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

/// Talks MCP to a `wen mcp` process: every message, then end of input; the
/// answers in order.
fn run_mcp(root: &str, messages: &[Value]) -> (Vec<Value>, bool, String) {
    let mut child = wen()
        .args(["mcp", "--software", "--root", root])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for message in messages {
            stdin.write_all(message.to_string().as_bytes()).unwrap();
            stdin.write_all(b"\n").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let answers = text
        .lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|_| panic!("stdout is protocol only: {line}"))
        })
        .collect();
    (
        answers,
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn the_mcp_server_answers_with_the_same_json() {
    let actor = parse_actor("parity");
    let in_process_dir = project("mcp-a");
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: in_process_dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let expected: Vec<Value> = examples()
        .iter()
        .map(|(tool, args)| {
            let mut answer = workspace.call(tool, args, &actor);
            if let Some(image) = answer.get_mut("image").and_then(Value::as_object_mut) {
                image.remove("data");
            }
            answer
        })
        .collect();
    let mcp_dir = project("mcp-b");
    let mut messages = vec![
        json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "clientInfo": { "name": "parity" } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    ];
    for (index, (tool, args)) in examples().iter().enumerate() {
        messages.push(json!({ "jsonrpc": "2.0", "id": index + 1, "method": "tools/call", "params": { "name": tool, "arguments": args } }));
    }
    let (answers, success, stderr) = run_mcp(mcp_dir.to_str().unwrap(), &messages);
    assert!(success, "{stderr}");
    assert_eq!(
        answers.len(),
        expected.len() + 1,
        "one answer per request, none for the notification"
    );
    for (index, (answer, expected)) in answers[1..].iter().zip(&expected).enumerate() {
        assert_eq!(answer["id"], index + 1);
        assert_eq!(
            &answer["result"]["structuredContent"],
            expected,
            "call {index}: {:?}",
            examples()[index]
        );
    }
    assert_eq!(
        std::fs::read_to_string(in_process_dir.join("scenes/main.scene.json")).unwrap(),
        std::fs::read_to_string(mcp_dir.join("scenes/main.scene.json")).unwrap()
    );
    for dir in [in_process_dir, mcp_dir] {
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// The M4 exit criterion, scripted: from an empty project, only through MCP,
/// build a scene, run it, take a picture and check the result.
#[test]
fn an_agent_builds_runs_looks_at_and_verifies_a_scene_through_mcp_only() {
    let dir = std::env::temp_dir().join(format!("wish-mcp-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.to_str().unwrap().to_owned();
    assert!(
        wen()
            .args(["new", &root, "--name", "E2E"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let request = |id: u64, tool: &str, arguments: Value| json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": tool, "arguments": arguments } });
    let messages = vec![
        json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "clientInfo": { "name": "e2e-agent" } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/read", "params": { "uri": "wish://docs/guide" } }),
        request(2, "project_info", json!({})),
        request(3, "schema_get", json!({ "search": "shape" })),
        // Three platforms and a ball over the middle one.
        request(
            4,
            "edit_apply",
            json!({ "ops": [
                { "op": "spawn", "id": "platforms" },
                { "op": "spawn", "id": "platform_1", "parent": "platforms", "components": { "Primitive": { "shape": "Cube" }, "Transform": { "translation": [-3, 0.5, 0], "scale": [2, 0.4, 2] } } },
                { "op": "spawn", "id": "platform_2", "parent": "platforms", "components": { "Primitive": { "shape": "Cube" }, "Transform": { "translation": [0, 1.5, -1], "scale": [2, 0.4, 2] } } },
                { "op": "spawn", "id": "platform_3", "parent": "platforms", "components": { "Primitive": { "shape": "Cube" }, "Transform": { "translation": [3, 2.5, -2], "scale": [2, 0.4, 2] } } },
                { "op": "spawn", "id": "ball", "components": { "Primitive": { "shape": "Sphere" }, "Material": { "base_color": [0.9, 0.2, 0.1, 1] }, "Transform": { "translation": [0, 2.2, -1], "scale": [0.6, 0.6, 0.6] } } }
            ] }),
        ),
        request(5, "check", json!({})),
        request(6, "sim_start", json!({ "seed": 1 })),
        request(7, "sim_step", json!({ "frames": 60 })),
        request(8, "describe_view", json!({ "session": "s1" })),
        request(
            9,
            "capture_frame",
            json!({ "session": "s1", "width": 320, "height": 180 }),
        ),
        request(
            10,
            "entity_query",
            json!({ "session": "s1", "name": "platform_*", "where": "Transform.translation.y > 1" }),
        ),
    ];
    let (answers, success, stderr) = run_mcp(&root, &messages);
    assert!(success, "{stderr}");
    let result = |id: u64| -> &Value {
        let answer = answers.iter().find(|a| a["id"] == id).unwrap();
        assert!(answer.get("error").is_none(), "{answer}");
        &answer["result"]
    };
    assert!(
        result(1)["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("The loop")
    );
    assert_eq!(result(2)["structuredContent"]["project"]["name"], "E2E");
    assert_eq!(result(4)["isError"], false, "{}", result(4));
    assert_eq!(result(5)["structuredContent"]["passed"], true);
    assert_eq!(result(7)["structuredContent"]["tick"], 60);
    // Everything built is on screen.
    let visible: Vec<&str> = result(8)["structuredContent"]["visible"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["entity"].as_str().unwrap())
        .collect();
    for id in ["platform_1", "platform_2", "platform_3", "ball"] {
        assert!(visible.contains(&id), "{id} is not visible: {visible:?}");
    }
    assert_eq!(result(9)["content"][1]["type"], "image");
    let raised: Vec<&str> = result(10)["structuredContent"]["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(raised, ["platform_2", "platform_3"]);
    let scene = std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap();
    assert!(scene.contains("platform_3") && scene.contains("ball"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn wen_mcp_reports_a_bad_folder_on_standard_error() {
    let output = wen()
        .args(["mcp", "--root", "no/such/folder"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "standard output belongs to the protocol"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("project_not_found"));
}
