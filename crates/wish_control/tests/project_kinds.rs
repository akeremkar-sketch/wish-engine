//! Project kinds (release 1.1, package 1): `wish.json` may say `"kind":
//! "2d"` or `"3d"`; a project without it is a 3D project and its file is
//! never rewritten; a new project says its kind; `check` and `project_info`
//! know it; templates have one.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_control::project::{ProjectKind, parse_project, read_project};
use wish_control::scaffold::{
    create_from_template, create_project, create_project_of_kind, templates,
};
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn tmp(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("kinds-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn open(dir: &Path) -> Workspace {
    Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .unwrap()
}

/// A `wish.json` as every project before release 1.1 has it.
const OLD: &str = "{
  \"engine\": \"0.9.0\",
  \"format\": 1,
  \"name\": \"Old game\",
  \"scene\": \"scenes/main.scene.json\",
  \"types\": {}
}
";

#[test]
fn a_project_without_a_kind_is_3d_and_its_file_stays_byte_identical() {
    let config = parse_project(OLD).unwrap();
    assert_eq!(config.kind, None, "the file says no kind");
    assert_eq!(config.kind(), ProjectKind::D3);
    assert_eq!(config.to_canonical().unwrap(), OLD, "no key is added");

    // Through a workspace too: a project that changes its default scene
    // writes `wish.json` again, still without a kind.
    let dir = tmp("old");
    create_project(&dir, "Old game", "wen").unwrap();
    std::fs::write(dir.join("wish.json"), OLD).unwrap();
    let mut workspace = open(&dir);
    let info = workspace.call("project_info", &json!({}), &parse_actor("human"));
    assert_eq!(info["project"]["kind"], "3d", "{info}");
    assert_eq!(std::fs::read_to_string(dir.join("wish.json")).unwrap(), OLD);
}

#[test]
fn a_kind_is_read_written_and_validated() {
    let two_d = OLD.replace("  \"name\"", "  \"kind\": \"2d\",\n  \"name\"");
    let config = parse_project(&two_d).unwrap();
    assert_eq!(config.kind, Some(ProjectKind::D2));
    assert_eq!(config.to_canonical().unwrap(), two_d, "a kind round trips");
    // Case does not matter when reading; the file is written in lower case.
    let shouting = two_d.replace("\"2d\"", "\"3D\"");
    assert_eq!(parse_project(&shouting).unwrap().kind(), ProjectKind::D3);

    // A word that is not a kind: an error with a hint, at the setting.
    let bad = two_d.replace("\"2d\"", "\"2dd\"");
    let error = parse_project(&bad).unwrap_err();
    assert_eq!(error.code, "invalid_project_file");
    assert!(error.message.contains("'2dd'"), "{}", error.message);
    assert!(error.hint.contains("'2d'"), "{}", error.hint);
    assert!(error.hint.contains("3D project"), "{}", error.hint);
    // Not even a text.
    let number = two_d.replace("\"2d\"", "2");
    assert_eq!(
        parse_project(&number).unwrap_err().code,
        "invalid_project_file"
    );
}

#[test]
fn check_reports_a_bad_kind_with_its_place_and_a_hint() {
    let dir = tmp("bad");
    create_project(&dir, "Bad", "wen").unwrap();
    let text = std::fs::read_to_string(dir.join("wish.json"))
        .unwrap()
        .replace("\"3d\"", "\"4d\"");
    std::fs::write(dir.join("wish.json"), text).unwrap();
    let mut workspace = open(&dir);
    let check = workspace.call("check", &json!({}), &parse_actor("human"));
    assert_eq!(check["passed"], false, "{check}");
    let found = check["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "invalid_project_file")
        .unwrap_or_else(|| panic!("{check}"));
    assert_eq!(found["location"]["path"], "/kind");
    assert!(
        found["hint"].as_str().is_some_and(|hint| !hint.is_empty()),
        "{found}"
    );
}

#[test]
fn a_new_project_says_its_kind() {
    let three = tmp("new-3d");
    create_project(&three, "Three", "wen").unwrap();
    let config = read_project(&three).unwrap().unwrap();
    assert_eq!(config.kind, Some(ProjectKind::D3));

    let two = tmp("new-2d");
    let written = create_project_of_kind(&two, "Two", "wen", ProjectKind::D2).unwrap();
    assert!(written.iter().any(|file| file == "wish.json"));
    let config = read_project(&two).unwrap().unwrap();
    assert_eq!(config.kind, Some(ProjectKind::D2));
    assert!(
        std::fs::read_to_string(two.join("wish.json"))
            .unwrap()
            .contains("\"kind\": \"2d\"")
    );
    // The scene: an orthographic camera, no ground, no sun.
    let scene: Value =
        serde_json::from_str(&std::fs::read_to_string(two.join("scenes/main.scene.json")).unwrap())
            .unwrap();
    let entities = scene["entities"].as_object().unwrap();
    assert_eq!(entities.len(), 1, "{scene}");
    assert_eq!(
        entities["camera"]["components"]["Camera"]["projection"],
        "Orthographic"
    );
    // The project is sound, and says what it is.
    let mut workspace = open(&two);
    let actor = parse_actor("human");
    let check = workspace.call("check", &json!({}), &actor);
    assert_eq!(check["passed"], true, "{check}");
    let info = workspace.call("project_info", &json!({}), &actor);
    assert_eq!(info["project"]["kind"], "2d", "{info}");
    // Its agent guide knows.
    assert!(
        std::fs::read_to_string(two.join("CLAUDE.md"))
            .unwrap()
            .contains("This is a 2D project")
    );
    assert!(
        !std::fs::read_to_string(three.join("CLAUDE.md"))
            .unwrap()
            .contains("This is a 2D project")
    );
}

#[test]
fn templates_have_a_kind_and_a_new_project_takes_it() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let all = templates(&examples);
    let kind_of = |id: &str| all.iter().find(|t| t.id == id).unwrap().kind;
    assert_eq!(kind_of("bricks"), ProjectKind::D2);
    for id in ["islands", "hop", "collect", "quiz"] {
        assert_eq!(kind_of(id), ProjectKind::D3, "{id}");
    }
    let bricks = all.iter().find(|t| t.id == "bricks").unwrap();
    let dir = tmp("from-bricks");
    create_from_template(&dir, "Mine", "wen", &bricks.path).unwrap();
    assert_eq!(
        read_project(&dir).unwrap().unwrap().kind,
        Some(ProjectKind::D2)
    );
    // A template from before release 1.1 says no kind: the new project
    // from it says 3d.
    let islands = all.iter().find(|t| t.id == "islands").unwrap();
    let dir = tmp("from-islands");
    create_from_template(&dir, "Mine", "wen", &islands.path).unwrap();
    assert_eq!(
        read_project(&dir).unwrap().unwrap().kind,
        Some(ProjectKind::D3)
    );
}
