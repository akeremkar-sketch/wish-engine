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
            "box": { "components": { "Primitive": { "shape": "Cube" } } }
        } }"#,
    )
    .unwrap();
    dir
}

/// One example call per tool, plus a few failing ones (errors must match
/// too). Every catalogue tool must appear.
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

#[test]
fn in_process_call_and_do_give_the_same_bytes() {
    let dir = project("all");
    let root = dir.to_str().unwrap().to_owned();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    let actor = parse_actor("human");
    let expected: Vec<String> = examples()
        .iter()
        .map(|(tool, args)| workspace.call(tool, args, &actor).to_string())
        .collect();

    // `wen call`, one process per call.
    for ((tool, args), expected) in examples().iter().zip(&expected) {
        let output = wen()
            .args(["call", tool, &args.to_string(), "--root", &root])
            .output()
            .unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.trim_end(), expected, "wen call {tool} {args}");
        let ok = serde_json::from_str::<Value>(expected).unwrap()["ok"] == json!(true);
        assert_eq!(output.status.success(), ok, "exit status of {tool}");
    }

    // `wen do`, all calls in one process.
    let calls: Vec<Value> = examples()
        .iter()
        .map(|(tool, args)| json!({ "tool": tool, "args": args }))
        .collect();
    let mut child = wen()
        .args(["do", "-", "--root", &root])
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
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert!(
        !output.status.success(),
        "some example calls fail on purpose"
    );
    std::fs::remove_dir_all(&dir).unwrap();
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
