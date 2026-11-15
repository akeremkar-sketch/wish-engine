//! The tool catalogue's own rules, and the project tools on small projects.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::{Kind, Workspace, WorkspaceOptions, catalog, input_schema, parse_actor};

#[test]
fn every_tool_is_documented_and_named_by_the_rules() {
    let mut names = std::collections::BTreeSet::new();
    for tool in catalog() {
        assert!(names.insert(tool.name), "'{}' is listed twice", tool.name);
        assert!(
            tool.name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_'),
            "'{}' is not snake_case (noun_verb, or one verb such as check)",
            tool.name
        );
        assert!(
            tool.description.len() > 40,
            "'{}' needs a real description",
            tool.name
        );
        assert!(!tool.title.is_empty());
        for param in tool.params {
            assert!(
                !param.doc.is_empty(),
                "{}.{} has no doc",
                tool.name,
                param.name
            );
            if let Some(default) = param.default {
                let value: Value = serde_json::from_str(default)
                    .unwrap_or_else(|_| panic!("{}.{}: bad default", tool.name, param.name));
                // The default must pass the parameter's own check.
                let args = json!({ param.name: value });
                let mut all = serde_json::Map::new();
                for other in tool.params.iter().filter(|p| p.required) {
                    all.insert(other.name.to_owned(), example_of(other.kind));
                }
                all.extend(args.as_object().unwrap().clone());
                wish_control::param::parse(tool.name, tool.params, &Value::Object(all))
                    .unwrap_or_else(|e| panic!("{}.{} default: {e}", tool.name, param.name));
            }
            if tool.mutates {
                assert!(
                    tool.params.iter().any(|p| p.name == "dry_run"),
                    "{} mutates",
                    tool.name
                );
            }
        }
        let schema = input_schema(tool.params);
        assert_eq!(schema["type"], "object");
    }
}

fn example_of(kind: Kind) -> Value {
    match kind {
        Kind::String => json!("x"),
        Kind::Integer { min, .. } => json!(min),
        Kind::Number { min, .. } => json!(min),
        Kind::Boolean => json!(true),
        Kind::StringList => json!([]),
        Kind::Object => json!({}),
        Kind::Array => json!([]),
        Kind::Choice(choices) => json!(choices[0]),
    }
}

fn folder(name: &str, project_file: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wish-control-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::write(
        dir.join("scenes/main.scene.json"),
        "{ \"format\": 1, \"entities\": { \"a\": { \"components\": { \"Transfrom\": {} } } } }",
    )
    .unwrap();
    if let Some(text) = project_file {
        std::fs::write(dir.join("wish.json"), text).unwrap();
    }
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
fn a_broken_project_file_is_reported_and_the_tools_still_work() {
    let dir = folder(
        "broken",
        Some("{ \"format\": 1, \"name\": \"x\", \"scene\": \"s.json\" }"),
    );
    let mut workspace = open(&dir);
    let actor = parse_actor("claude-code");
    let info = workspace.call("project_info", &json!({}), &actor);
    assert_eq!(info["ok"], true);
    assert_eq!(info["project"], Value::Null);
    assert_eq!(info["diagnostics"][0]["code"], "invalid_project_file");
    assert_eq!(info["diagnostics"][0]["location"]["path"], "/scene");
    let check = workspace.call("check", &json!({}), &actor);
    assert_eq!(check["passed"], false);
    let codes: Vec<&str> = check["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"invalid_project_file"), "{codes:?}");
    assert!(codes.contains(&"unknown_component"), "{codes:?}");
    // Only one file asked for: only its problems.
    let one = workspace.call("check", &json!({ "paths": ["wish.json"] }), &actor);
    assert_eq!(one["errors"], 1);
    assert_eq!(one["files"], 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_folder_without_a_project_file_is_a_warning_with_the_way_forward() {
    let dir = folder("bare", None);
    let mut workspace = open(&dir);
    let info = workspace.call("project_info", &Value::Null, &parse_actor("human"));
    assert_eq!(info["ok"], true);
    assert_eq!(info["diagnostics"][0]["code"], "no_project_file");
    assert!(
        info["diagnostics"][0]["hint"]
            .as_str()
            .unwrap()
            .contains("wen new")
    );
    assert_eq!(info["scenes"], json!(["scenes/main.scene.json"]));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn opening_something_that_is_not_a_folder_is_explained() {
    let problem = Workspace::open(WorkspaceOptions {
        root: PathBuf::from("this/does/not/exist"),
        example: None,
    })
    .unwrap_err();
    assert_eq!(problem.code, "project_not_found");
    let dir = folder("example", None);
    let problem = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: Some("bonce".to_owned()),
    })
    .unwrap_err();
    assert_eq!(problem.hint, "Did you mean 'bounce'?");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn schema_get_finds_types_by_words_and_explains_empty_searches() {
    let dir = folder("schema", None);
    let mut workspace = open(&dir);
    let actor = parse_actor("human");
    let found = workspace.call("schema_get", &json!({ "search": "field of view" }), &actor);
    assert_eq!(found["types"][0]["name"], "Camera");
    let none = workspace.call("schema_get", &json!({ "search": "zzzz" }), &actor);
    assert_eq!(none["count"], 0);
    assert_eq!(none["diagnostics"][0]["code"], "nothing_found");
    let full = workspace.call(
        "schema_get",
        &json!({ "names": ["Camera"], "detail": "full" }),
        &actor,
    );
    assert!(full["types"][0]["schema"]["properties"]["fov_y_deg"].is_object());
    std::fs::remove_dir_all(&dir).unwrap();
}
