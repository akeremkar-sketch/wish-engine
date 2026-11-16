//! The recipes page (`wish://docs/recipes`, M7 WP4) stays true: every
//! recipe's game code builds without warnings in a fresh project, its scene
//! operations apply, and its scenario test examples are well formed.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_script::build::{SCRIPTS_ENTRY, Toolchain, pack_sdk};

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-recipes");
        let _ = std::fs::remove_dir_all(&sdk);
        pack_sdk(&cargo, &manifest, &tmp.join("sdk-target"), &sdk).unwrap();
        let out = std::process::Command::new("rustc")
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

/// A fenced block: its info string and its text.
struct Block {
    info: String,
    text: String,
}

/// The recipes, each a title and its fenced blocks.
fn recipes() -> Vec<(String, Vec<Block>)> {
    let page = wish_control::docs::recipes_page();
    let mut found = Vec::new();
    for section in page.split("\n## ").skip(1) {
        let title = section.lines().next().unwrap_or_default().to_owned();
        let mut blocks = Vec::new();
        let mut lines = section.lines();
        while let Some(line) = lines.next() {
            if let Some(info) = line.strip_prefix("```") {
                let mut text = String::new();
                for inner in lines.by_ref() {
                    if inner.starts_with("```") {
                        break;
                    }
                    text.push_str(inner);
                    text.push('\n');
                }
                blocks.push(Block {
                    info: info.trim().to_owned(),
                    text,
                });
            }
        }
        found.push((title, blocks));
    }
    found
}

#[test]
fn every_recipe_builds_and_applies() {
    let all = recipes();
    assert!(all.len() >= 8, "{} recipes", all.len());
    let mut built = 0;
    let mut applied = 0;
    let mut tests = 0;
    for (index, (title, blocks)) in all.iter().enumerate() {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("recipe-{index}"));
        let _ = std::fs::remove_dir_all(&dir);
        create_project(&dir, "Recipe", "wen").unwrap();
        let mut ws = Workspace::open(WorkspaceOptions {
            root: dir.clone(),
            example: None,
        })
        .unwrap();
        ws.set_toolchain(toolchain().clone());
        let agent = parse_actor("agent");
        for block in blocks {
            match block.info.as_str() {
                "rust" => {
                    std::fs::write(dir.join(SCRIPTS_ENTRY), &block.text).unwrap();
                    let answer = ws.call("script_build", &json!({}), &agent);
                    assert_eq!(answer["passed"], true, "'{title}' does not build: {answer}");
                    assert_eq!(
                        answer["warnings"], 0,
                        "'{title}' builds with warnings: {answer}"
                    );
                    built += 1;
                }
                "json ops" => {
                    let ops: Value = serde_json::from_str(&block.text)
                        .unwrap_or_else(|e| panic!("'{title}': the operations are not JSON: {e}"));
                    let answer = ws.call("edit_apply", &json!({ "ops": ops }), &agent);
                    assert_eq!(answer["ok"], true, "'{title}': {answer}");
                    applied += 1;
                }
                "json test" => {
                    let test: Value = serde_json::from_str(&block.text)
                        .unwrap_or_else(|e| panic!("'{title}': the test is not JSON: {e}"));
                    assert_eq!(test["format"], 1, "'{title}'");
                    assert!(test["steps"].as_array().is_some_and(|s| !s.is_empty()));
                    tests += 1;
                }
                other => panic!("'{title}' has a block of unknown kind '{other}'"),
            }
        }
        // After the code and the operations the project checks clean and a
        // session runs.
        let checked = ws.call("check", &json!({}), &agent);
        assert_eq!(checked["passed"], true, "'{title}': {checked}");
        let started = ws.call("sim_start", &json!({}), &agent);
        assert_eq!(started["ok"], true, "'{title}': {started}");
        let stepped = ws.call("sim_step", &json!({ "frames": 30 }), &agent);
        assert!(
            stepped.get("diagnostics").is_none()
                || stepped["diagnostics"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|d| d["severity"] != "error"),
            "'{title}': {stepped}"
        );
    }
    assert!(
        built >= 6 && applied >= 7 && tests >= 1,
        "{built} {applied} {tests}"
    );
}

#[test]
fn the_recipes_are_found_by_search_and_named_in_the_guide() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("recipes-search");
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Search", "wen").unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    let found = ws.call(
        "docs_search",
        &json!({ "query": "moving platform" }),
        &parse_actor("agent"),
    );
    assert_eq!(found["results"][0]["uri"], "wish://docs/recipes", "{found}");
    assert!(wish_control::docs::guide().contains("wish://docs/recipes"));
    let claude = std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
    assert!(claude.contains("wish://docs/recipes"));
    // A name from another engine leads to the recipes.
    let unknown = ws.call(
        "schema_get",
        &json!({ "names": ["FollowCamera"] }),
        &parse_actor("agent"),
    );
    assert!(
        unknown.to_string().contains("wish://docs/recipes"),
        "{unknown}"
    );
}
