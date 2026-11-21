//! Command-line parity: every control-plane tool gives the same JSON answer
//! in process, through `wen call` and through `wen do` (and, from WP6 on,
//! through the MCP server). A tool added to the catalogue without an example
//! here fails the test. And every tool, resource and prompt answers the
//! same bytes in two separate processes on the same project, game code
//! built in both (ADR-027, rule 2: no wall-clock value in an answer).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde_json::{Value, json};
use wish_control::{Workspace, WorkspaceOptions, catalog, parse_actor};
use wish_script::build::{Toolchain, pack_sdk};

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
        ("entity_get", json!({ "resources": ["Sky"] })),
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
        (
            "asset_create",
            json!({ "path": "assets/sounds/blip.wav", "sound": { "wave": "square", "frequency": 300, "end_frequency": 700, "seconds": 0.1 } }),
        ),
        (
            "asset_create",
            json!({ "path": "assets/dot.png", "picture": { "shape": "circle", "width": 16, "height": 16, "color": [1, 0, 0, 1] } }),
        ),
        ("asset_create", json!({ "path": "assets/music.mp3" })),
        ("project_settings", json!({})),
        (
            "project_settings",
            json!({ "action": "set", "name": "Renamed", "input": { "format": 1, "actions": {
                "jump": { "kind": "button", "keys": ["Space", "Up"] } } } }),
        ),
        (
            "project_settings",
            json!({ "action": "set", "input": { "format": 1, "actions": {
                "jump": { "kind": "button", "keys": ["Spacebar"] } } } }),
        ),
        (
            "project_settings",
            json!({ "action": "set", "dry_run": true, "scene": "scenes/main.scene.json" }),
        ),
        ("activity_get", json!({ "limit": 5 })),
        ("checkpoint", json!({ "action": "save", "note": "before" })),
        ("checkpoint", json!({ "action": "list" })),
        (
            "checkpoint",
            json!({ "action": "restore", "name": "cp_1", "dry_run": true }),
        ),
        ("checkpoint", json!({ "action": "restore", "name": "nope" })),
        ("project_export", json!({ "name": "Bad Name" })),
        (
            "game_design",
            json!({ "action": "set_brief", "fields": { "pitch": "Hop the islands", "must": ["double jump"] } }),
        ),
        (
            "game_design",
            json!({ "action": "decide", "text": "Jump height 2.5 m" }),
        ),
        (
            "game_design",
            json!({ "action": "ask", "text": "How many enemies?", "options": ["3", "5", "10"] }),
        ),
        (
            "game_design",
            json!({ "action": "answer", "id": 2, "answer": "5", "remember": true }),
        ),
        ("game_design", json!({ "action": "forget", "id": 99 })),
        ("game_design", json!({})),
        ("packages", json!({})),
        ("packages", json!({ "action": "check", "path": "nowhere" })),
        (
            "packages",
            json!({ "action": "remove", "id": "com.example.none" }),
        ),
        ("packages", json!({ "action": "lock", "dry_run": true })),
        ("engines", json!({ "action": "add", "path": "nowhere" })),
        ("permissions", json!({})),
        ("permissions", json!({ "action": "set", "mode": "suggest" })),
        ("proposals", json!({})),
        ("proposals", json!({ "action": "accept", "id": 7 })),
        ("permissions", json!({ "action": "set", "mode": "apply" })),
        ("activity_get", json!({ "since": 2, "actor": "agents" })),
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
        // Players (release 1.1): a session of two, each with their input, and
        // a player the session does not have.
        ("sim_start", json!({ "players": 2 })),
        (
            "sim_step",
            json!({ "session": "s5", "frames": 2, "input": { "move": [0, 1] },
                    "players": { "1": { "move": [1, 0], "jump": true } } }),
        ),
        (
            "sim_step",
            json!({ "session": "s5", "players": { "4": { "jump": true } } }),
        ),
        ("sim_stop", json!({ "session": "s5" })),
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
        // Building needs the SDK pack and the compiler; here the parity of
        // the answer is shown on a mistake. A real build in two processes is
        // compared in `every_tool_answers_the_same_bytes_in_two_processes`.
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

/// M9 WP3: `wen new --template` starts from one of the engine's templates
/// (the repository's examples in a development build).
#[test]
fn wen_new_starts_from_a_template() {
    let dir = std::env::temp_dir().join(format!("wish-new-template-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.to_str().unwrap();
    let output = wen()
        .args([
            "new",
            root,
            "--name",
            "My bricks",
            "--template",
            "bricks",
            "--json",
        ])
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(
        std::fs::read_to_string(dir.join("scripts/src/lib.rs")).unwrap(),
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/bricks/scripts/src/lib.rs")
        )
        .unwrap()
    );
    let project: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("wish.json")).unwrap()).unwrap();
    assert_eq!(project["name"], "My bricks");
    std::fs::remove_dir_all(&dir).unwrap();
    // An unknown template is named with the ones there are.
    let unknown = wen()
        .args(["new", root, "--template", "nope", "--json"])
        .output()
        .unwrap();
    assert!(!unknown.status.success());
    let problem: Value = serde_json::from_slice(&unknown.stdout).unwrap();
    assert_eq!(problem["error"]["code"], "invalid_template");
    assert!(
        problem["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("islands"),
        "{problem}"
    );
    assert!(!dir.exists());
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

/// The SDK pack and the compiler, for `wen` processes that build game code
/// (`WISH_SDK`, `WISH_RUSTC`).
fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-cli-parity");
        // The folder is the test's own (a cache may have left anything in it).
        let _ = std::fs::remove_dir_all(&sdk);
        pack_sdk(&cargo, &manifest, &tmp.join("sdk-target"), &sdk).unwrap();
        let out = Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            .unwrap();
        let sysroot = PathBuf::from(String::from_utf8(out.stdout).unwrap().trim());
        let rustc = sysroot
            .join("bin")
            .join(if cfg!(windows) { "rustc.exe" } else { "rustc" });
        Toolchain { sdk, rustc }
    })
}

/// A fresh `wen new` project (game code, a scenario test, an action map) at
/// `dir`: the same files every time.
fn new_project(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    wish_control::scaffold::create_project(dir, "Twice", "wen").unwrap();
}

/// Every call through one `wen do` process on a fresh project at `dir`,
/// with the toolchain; one answer line per call.
fn do_with_toolchain(dir: &Path, calls: &[(&str, Value)]) -> Vec<String> {
    new_project(dir);
    let toolchain = toolchain();
    let list: Vec<Value> = calls
        .iter()
        .map(|(tool, args)| json!({ "tool": tool, "args": args }))
        .collect();
    let mut child = wen()
        .args(["do", "-", "--software", "--root", dir.to_str().unwrap()])
        .env("WISH_SDK", &toolchain.sdk)
        .env("WISH_RUSTC", &toolchain.rustc)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(Value::Array(list).to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The calls of the two-process comparison, one row per tool of the
/// catalogue, in the order they run (a row may first call other tools to
/// set up what its tool needs). Each row's own tool must answer `ok` at
/// least once: an answer that is only "missing parameter" proves little.
/// A tool missing here fails the test.
fn two_process_calls() -> Vec<(&'static str, Vec<(&'static str, Value)>)> {
    vec![
        ("project_info", vec![("project_info", json!({}))]),
        (
            "schema_get",
            vec![(
                "schema_get",
                json!({ "names": ["Transform"], "detail": "normal" }),
            )],
        ),
        (
            "docs_search",
            vec![("docs_search", json!({ "query": "camera field of view" }))],
        ),
        ("check", vec![("check", json!({}))]),
        (
            "scene_get",
            vec![("scene_get", json!({ "detail": "normal" }))],
        ),
        (
            "edit_apply",
            vec![(
                "edit_apply",
                json!({ "ops": [{ "op": "spawn", "id": "crate", "components": {
                    "Primitive": { "shape": "Cube" }, "Transform": { "translation": [0, 1, 0] } } }] }),
            )],
        ),
        ("edit_history", vec![("edit_history", json!({}))]),
        (
            "entity_query",
            vec![(
                "entity_query",
                json!({ "with": ["Primitive"], "components": ["Transform"] }),
            )],
        ),
        (
            "entity_get",
            vec![("entity_get", json!({ "ids": ["camera", "crate"] }))],
        ),
        (
            "asset_create",
            vec![
                (
                    "asset_create",
                    json!({ "path": "assets/dot.png", "picture": { "shape": "circle", "width": 16, "height": 16, "color": [1, 0, 0, 1] } }),
                ),
                (
                    "asset_create",
                    json!({ "path": "assets/blip.wav", "sound": { "wave": "square", "frequency": 300, "seconds": 0.1 } }),
                ),
            ],
        ),
        ("asset_list", vec![("asset_list", json!({}))]),
        (
            "asset_info",
            vec![
                ("asset_info", json!({ "path": "scenes/main.scene.json" })),
                ("asset_info", json!({ "path": "assets/dot.png" })),
            ],
        ),
        (
            "asset_move",
            vec![(
                "asset_move",
                json!({ "from": "assets/dot.png", "to": "assets/pictures/dot.png" }),
            )],
        ),
        ("sim_start", vec![("sim_start", json!({ "seed": 3 }))]),
        (
            "sim_step",
            vec![(
                "sim_step",
                json!({ "session": "s1", "frames": 30, "watch": ["crate"] }),
            )],
        ),
        (
            "sim_snapshot",
            vec![
                ("sim_snapshot", json!({ "session": "s1", "label": "a" })),
                ("sim_snapshot", json!({ "session": "s1", "action": "list" })),
            ],
        ),
        (
            "sim_restore",
            vec![(
                "sim_restore",
                json!({ "session": "s1", "snapshot": "a", "fork": true }),
            )],
        ),
        ("logs_get", vec![("logs_get", json!({ "session": "s1" }))]),
        (
            "physics_query",
            vec![(
                "physics_query",
                json!({ "ray": { "from": [0, 5, 0], "dir": [0, -1, 0] } }),
            )],
        ),
        (
            "capture_frame",
            vec![
                ("capture_frame", json!({ "width": 64, "height": 36 })),
                (
                    "capture_frame",
                    json!({ "session": "s1", "width": 64, "height": 36, "view": "ids" }),
                ),
            ],
        ),
        (
            "describe_view",
            vec![("describe_view", json!({ "width": 64, "height": 36 }))],
        ),
        (
            "replay",
            vec![
                ("sim_start", json!({})),
                ("replay", json!({ "action": "record", "session": "s3" })),
                (
                    "sim_step",
                    json!({ "session": "s3", "frames": 5, "input": { "move": [0, 1] } }),
                ),
                (
                    "replay",
                    json!({ "action": "save", "session": "s3", "name": "twice" }),
                ),
                ("replay", json!({ "action": "run", "name": "twice" })),
                // Two players: a format-2 replay (the engine version in its
                // header) recorded and played back.
                ("sim_start", json!({ "players": 2 })),
                ("replay", json!({ "action": "record", "session": "s5" })),
                (
                    "sim_step",
                    json!({ "session": "s5", "frames": 4, "input": { "move": [0, 1] },
                            "players": { "1": { "move": [-1, 0] } } }),
                ),
                (
                    "replay",
                    json!({ "action": "save", "session": "s5", "name": "duo" }),
                ),
                ("replay", json!({ "action": "run", "name": "duo" })),
                ("sim_stop", json!({ "session": "s5" })),
            ],
        ),
        (
            "sim_stop",
            vec![
                ("sim_stop", json!({ "session": "s3" })),
                ("sim_stop", json!({ "session": "s2" })),
                ("sim_stop", json!({ "session": "s1" })),
            ],
        ),
        ("script_build", vec![("script_build", json!({}))]),
        ("test_run", vec![("test_run", json!({}))]),
        ("project_settings", vec![("project_settings", json!({}))]),
        (
            "checkpoint",
            vec![
                ("checkpoint", json!({ "action": "save", "note": "twice" })),
                ("checkpoint", json!({ "action": "list" })),
            ],
        ),
        ("permissions", vec![("permissions", json!({}))]),
        ("proposals", vec![("proposals", json!({}))]),
        // A dry run lists what an export writes without copying the player
        // program (tens of MB) twice.
        (
            "project_export",
            vec![("project_export", json!({ "dry_run": true }))],
        ),
        (
            "game_design",
            vec![
                (
                    "game_design",
                    json!({ "action": "decide", "text": "Jump height 2.5 m" }),
                ),
                ("game_design", json!({})),
            ],
        ),
        ("packages", vec![("packages", json!({}))]),
        ("engines", vec![("engines", json!({}))]),
        // Last: what everything above left in the feed, the history and the
        // project's summary (the build included).
        (
            "activity_get",
            vec![
                ("activity_get", json!({})),
                ("edit_history", json!({})),
                ("project_info", json!({})),
            ],
        ),
    ]
}

/// ADR-027, rule 2: a tool's answer depends on the project, the call and
/// the session, and on nothing else, so no duration, timestamp or date can
/// be in it. Every tool of the catalogue is called for real (a row of
/// [`two_process_calls`] each: edits, assets, sessions, a replay, a
/// checkpoint, the game code built) in one `wen do` process, and again in a
/// second process on the same project made afresh; the answers are compared
/// byte for byte. `capture_frame` and `describe_view` draw on the software
/// adapter.
#[test]
fn every_tool_answers_the_same_bytes_in_two_processes() {
    let rows = two_process_calls();
    for tool in catalog() {
        assert!(
            rows.iter().any(|(name, _)| *name == tool.name),
            "the tool '{}' has no row in two_process_calls (parity.rs): add calls that use it for real",
            tool.name
        );
    }
    let calls: Vec<(&str, Value)> = rows
        .iter()
        .flat_map(|(_, calls)| calls.iter().cloned())
        .collect();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("parity-two-processes");
    let first = do_with_toolchain(&dir, &calls);
    let second = do_with_toolchain(&dir, &calls);
    assert_eq!(first.len(), calls.len(), "one answer per call: {first:?}");
    assert_eq!(second.len(), calls.len());
    for (index, (a, b)) in first.iter().zip(&second).enumerate() {
        assert_eq!(a, b, "call {index}: {:?}", calls[index]);
    }
    let answers: Vec<Value> = first
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    // Each row's tool really did something.
    let mut index = 0;
    for (tool, row) in &rows {
        let worked = row
            .iter()
            .zip(&answers[index..])
            .any(|((name, _), answer)| name == tool && answer["ok"] == true);
        assert!(
            worked,
            "no call of '{tool}' answered ok: {:?}",
            &first[index..index + row.len()]
        );
        index += row.len();
    }
    // The build was a real one, its test passes, and the project knows it.
    let answer_of = |tool: &str| {
        let at = calls.iter().rposition(|(name, _)| *name == tool).unwrap();
        &answers[at]
    };
    assert_eq!(answer_of("script_build")["passed"], true);
    assert!(answer_of("script_build").get("seconds").is_none());
    assert_eq!(answer_of("test_run")["passed"], true);
    let info = answer_of("project_info");
    assert_eq!(info["scripts"]["last_build"]["passed"], true, "{info}");
    assert!(info["scripts"]["last_build"].get("seconds").is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ADR-027, rule 2 for what the MCP server serves besides tool calls: the
/// lists of tools, resources and prompts and every listed resource are the
/// same bytes in two `wen mcp` processes on the same project.
#[test]
fn every_resource_reads_the_same_in_two_processes() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("parity-two-resources");
    let start = vec![
        json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "clientInfo": { "name": "parity" } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    ];
    let mut listing = start.clone();
    listing.push(json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/list" }));
    new_project(&dir);
    let (answers, success, stderr) = run_mcp(dir.to_str().unwrap(), &listing);
    assert!(success, "{stderr}");
    let uris: Vec<String> = answers[1]["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["uri"].as_str().unwrap().to_owned())
        .collect();
    assert!(uris.len() > 5, "{uris:?}");
    let mut messages = start;
    for (id, method) in ["tools/list", "resources/list", "prompts/list"]
        .iter()
        .enumerate()
    {
        messages.push(json!({ "jsonrpc": "2.0", "id": id + 1, "method": method }));
    }
    for (index, uri) in uris.iter().enumerate() {
        messages.push(json!({ "jsonrpc": "2.0", "id": index + 10, "method": "resources/read", "params": { "uri": uri } }));
    }
    let run = || {
        new_project(&dir);
        let (answers, success, stderr) = run_mcp(dir.to_str().unwrap(), &messages);
        assert!(success, "{stderr}");
        answers
            .iter()
            .map(Value::to_string)
            .collect::<Vec<String>>()
    };
    let first = run();
    let second = run();
    assert_eq!(first.len(), messages.len() - 1, "every request answered");
    for (index, (a, b)) in first.iter().zip(&second).enumerate() {
        assert_eq!(a, b, "answer {index}");
        let answer: Value = serde_json::from_str(a).unwrap();
        assert!(answer.get("error").is_none(), "answer {index}: {a}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
