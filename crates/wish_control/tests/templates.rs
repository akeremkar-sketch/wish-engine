//! Projects made from templates (M9 WP3): what is copied, what is left
//! behind, what is the new project's own.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use wish_control::scaffold::{copy_template, create_from_template, create_project, templates};

fn tmp(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("templates-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn put(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn the_engine_examples_are_the_templates() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let ids: Vec<String> = templates(&examples).into_iter().map(|t| t.id).collect();
    // (first-pixel is a capture example without a wish.json: not a template)
    assert_eq!(
        ids,
        ["bricks", "collect", "hop", "islands", "quiz", "ridgewatch"]
    );
}

#[test]
fn a_template_is_copied_without_what_the_engine_made_in_it() {
    let base = tmp("copy");
    let template = base.join("template");
    create_project(&template, "Template", "wen").unwrap();
    put(&template, "assets/sky.png", "png");
    put(
        &template,
        "scenes/level2.scene.json",
        "{
  \"entities\": {},
  \"format\": 1
}
",
    );
    put(&template, ".wish/build/game.wasm", "built");
    put(&template, "exports/game/game.exe", "exported");
    put(&template, "assets/half.part", "half written");
    put(&template, "CLAUDE.md", "the template's guide");
    let dir = base.join("new");
    let written =
        create_from_template(&dir, "  New game ", "C:/engine/wen.exe", &template).unwrap();
    for file in [
        "assets/sky.png",
        "scenes/level2.scene.json",
        "scenes/main.scene.json",
        "scripts/src/lib.rs",
        "tests/boots.test.json",
        "input.json",
        "wish.json",
        ".mcp.json",
        "CLAUDE.md",
        ".gitignore",
    ] {
        assert!(dir.join(file).is_file(), "{file}");
        assert!(written.iter().any(|w| w == file), "{file} in {written:?}");
    }
    for left in [".wish", "exports", "assets/half.part"] {
        assert!(!dir.join(left).exists(), "{left}");
    }
    let project = wish_control::project::read_project(&dir).unwrap().unwrap();
    assert_eq!(project.name, "New game");
    let guide = std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
    assert!(guide.starts_with("# New game"), "{guide}");
    assert!(
        std::fs::read_to_string(dir.join(".mcp.json"))
            .unwrap()
            .contains("C:/engine/wen.exe")
    );
    // The copy is a valid project.
    let mut ws = wish_control::Workspace::open(wish_control::WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    let check = ws.call(
        "check",
        &serde_json::json!({}),
        &wish_control::parse_actor("agent:test"),
    );
    assert_eq!(check["passed"], true, "{check}");
    // Not twice in one folder; not from a folder that is no project.
    let again = create_from_template(&dir, "Again", "wen", &template).unwrap_err();
    assert_eq!(again.code, "project_exists");
    let nothing = create_from_template(&base.join("other"), "Other", "wen", &base).unwrap_err();
    assert_eq!(nothing.code, "invalid_template");
    assert!(!base.join("other").exists());
    let unnamed = create_from_template(&base.join("other"), " ", "wen", &template).unwrap_err();
    assert_eq!(unnamed.code, "invalid_project_name");
}

#[test]
fn a_package_copies_a_template_as_a_template() {
    let base = tmp("package");
    let template = base.join("template");
    create_project(&template, "Template", "wen").unwrap();
    put(&template, ".wish/build/game.wasm", "built");
    put(&template, ".template/thumbnail.png", "picture");
    let to = base.join("templates").join("mine");
    copy_template(&template, &to).unwrap();
    // Its picture for the welcome screen goes with it; a new project made
    // from it does not get it.
    assert!(to.join(".template/thumbnail.png").is_file());
    let made = base.join("made");
    create_from_template(&made, "Made", "wen", &to).unwrap();
    assert!(!made.join(".template").exists());
    assert!(to.join("wish.json").is_file());
    assert!(to.join("scenes/main.scene.json").is_file());
    assert!(!to.join(".wish").exists());
    // It is a template again.
    let found = templates(&base.join("templates"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "Template");
}
