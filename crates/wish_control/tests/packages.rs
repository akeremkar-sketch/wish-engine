//! Packages (M10a): versions and requirements, manifests checked with
//! hints, install / remove / lock in the project, wish.json and wish.lock
//! kept, nothing half-installed, removed packages in the trash.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_control::packages::{Version, VersionReq};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("packages-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn project(dir: &Path) -> Workspace {
    let root = dir.join("game");
    create_project(&root, "Packages", "wen").unwrap();
    std::fs::remove_dir_all(root.join("scripts")).unwrap();
    Workspace::open(WorkspaceOptions {
        root,
        example: None,
    })
    .unwrap()
}

/// A theme package `id` at `version` in `dir/<name>`.
fn theme_package(dir: &Path, name: &str, id: &str, version: &str, license: &str) -> PathBuf {
    let package = dir.join(name);
    std::fs::create_dir_all(package.join("themes")).unwrap();
    std::fs::write(
        package.join("wish-package.json"),
        json!({
            "format": 1, "id": id, "name": "Dusk", "version": version,
            "engine": ">=0.1, <2.0", "license": license,
            "description": "A dusky theme.",
            "contents": { "themes": ["themes/dusk.json"] }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        package.join("themes/dusk.json"),
        r#"{ "format": 1, "colors": {} }"#,
    )
    .unwrap();
    std::fs::create_dir_all(package.join(".git")).unwrap();
    std::fs::write(package.join(".git/HEAD"), "not part of it").unwrap();
    package
}

fn call(ws: &mut Workspace, actor: &str, args: Value) -> Value {
    ws.call("packages", &args, &parse_actor(actor))
}

#[test]
fn versions_and_requirements() {
    let v = |text: &str| Version::parse(text).unwrap();
    assert_eq!(v("1.2.3").to_string(), "1.2.3");
    for bad in ["1.2", "1.2.3.4", "01.2.3", "a.b.c", "", "1.-2.3"] {
        assert!(Version::parse(bad).is_err(), "{bad}");
    }
    let r = |text: &str| VersionReq::parse(text).unwrap();
    assert!(r(">=0.9, <2.0").matches(v("1.5.0")));
    assert!(!r(">=0.9, <2.0").matches(v("2.0.0")));
    assert!(r("^1.2").matches(v("1.9.0")));
    assert!(!r("^1.2").matches(v("2.0.0")));
    assert!(!r("^1.2").matches(v("1.1.9")));
    assert!(r("^0.3.1").matches(v("0.3.9")));
    assert!(!r("^0.3.1").matches(v("0.4.0")));
    assert!(r("~1.2.0").matches(v("1.2.7")));
    assert!(!r("~1.2.0").matches(v("1.3.0")));
    assert!(r("=1.0.0").matches(v("1.0.0")));
    assert!(r("1.0").matches(v("1.4.0")), "a bare version is ^");
    let bad = VersionReq::parse(">= one").unwrap_err();
    assert_eq!(bad.code, "invalid_version_requirement");
    assert!(!bad.hint.is_empty());
}

#[test]
fn a_package_is_installed_locked_and_removed() {
    let dir = folder("install");
    let mut ws = project(&dir);
    let root = ws.root().to_path_buf();
    let original = std::fs::read_to_string(root.join("wish.json")).unwrap();
    let package = theme_package(&dir, "dusk", "com.example.dusk", "1.0.0", "MIT");
    // Checked without installing.
    let checked = call(
        &mut ws,
        "human",
        json!({ "action": "check", "path": package.display().to_string() }),
    );
    assert_eq!(checked["ok"], true, "{checked}");
    assert_eq!(checked["package"]["id"], "com.example.dusk");
    assert_eq!(
        checked["files"], 2,
        "hidden folders are not part of it: {checked}"
    );
    // A dry run changes nothing.
    let dry = call(
        &mut ws,
        "agent:a",
        json!({ "action": "install", "path": package.display().to_string(), "dry_run": true }),
    );
    assert_eq!(dry["ok"], true, "{dry}");
    assert!(!root.join("packages").exists());
    // Installed by an agent into the project.
    let done = call(
        &mut ws,
        "agent:a",
        json!({ "action": "install", "path": package.display().to_string() }),
    );
    assert_eq!(done["ok"], true, "{done}");
    assert!(
        root.join("packages/com.example.dusk/themes/dusk.json")
            .is_file()
    );
    assert!(!root.join("packages/com.example.dusk/.git").exists());
    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("wish.json")).unwrap()).unwrap();
    assert_eq!(config["packages"]["com.example.dusk"], "^1.0.0");
    let lock: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("wish.lock")).unwrap()).unwrap();
    assert_eq!(lock["packages"][0]["id"], "com.example.dusk");
    assert_eq!(lock["packages"][0]["version"], "1.0.0");
    let listed = call(&mut ws, "human", json!({}));
    assert_eq!(listed["project"][0]["locked"], true, "{listed}");
    assert_eq!(listed["diagnostics"], json!([]), "{listed}");
    // Packages are not assets, and the project still checks clean.
    let assets = ws.call("asset_list", &json!({}), &parse_actor("human"));
    assert!(!assets.to_string().contains("packages/"), "{assets}");
    let check = ws.call("check", &json!({}), &parse_actor("human"));
    assert_eq!(check["ok"], true, "{check}");
    // The same package again: replace it, or not.
    let again = call(
        &mut ws,
        "human",
        json!({ "action": "install", "path": package.display().to_string() }),
    );
    assert_eq!(again["error"]["code"], "package_exists", "{again}");
    let newer = theme_package(&dir, "dusk2", "com.example.dusk", "1.1.0", "MIT");
    let replaced = call(
        &mut ws,
        "human",
        json!({ "action": "install", "path": newer.display().to_string(), "replace": true }),
    );
    assert_eq!(replaced["replaces"], "1.0.0", "{replaced}");
    assert!(
        root.join(".wish/trash/packages/com.example.dusk-1")
            .is_dir()
    );
    // A changed file no longer matches the lock; lock writes it again.
    std::fs::write(
        root.join("packages/com.example.dusk/themes/dusk.json"),
        "{}",
    )
    .unwrap();
    let stale = call(&mut ws, "human", json!({}));
    assert_eq!(stale["project"][0]["locked"], false, "{stale}");
    assert_eq!(stale["diagnostics"][0]["code"], "package_lock_mismatch");
    let locked = call(&mut ws, "human", json!({ "action": "lock" }));
    assert_eq!(locked["ok"], true, "{locked}");
    assert_eq!(call(&mut ws, "human", json!({}))["diagnostics"], json!([]));
    // Removed: into the trash; the project is as it was.
    let removed = call(
        &mut ws,
        "human",
        json!({ "action": "remove", "id": "com.example.dusk" }),
    );
    assert_eq!(removed["ok"], true, "{removed}");
    assert!(
        root.join(".wish/trash/packages/com.example.dusk-2")
            .is_dir()
    );
    assert!(!root.join("packages/com.example.dusk").exists());
    assert!(!root.join("wish.lock").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("wish.json")).unwrap(),
        original
    );
    let gone = call(
        &mut ws,
        "human",
        json!({ "action": "remove", "id": "com.example.dusk" }),
    );
    assert_eq!(gone["error"]["code"], "package_not_found", "{gone}");
    // Everything above is in the activity feed with its actor; looking is not.
    let feed = ws.call("activity_get", &json!({}), &parse_actor("human"));
    let actors: Vec<&str> = feed["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["tool"] == "packages")
        .map(|e| e["actor"].as_str().unwrap())
        .collect();
    // The agent's dry run and install; the person's five changes (two of
    // them refused), but not their lists and checks.
    assert_eq!(
        actors.iter().filter(|a| **a == "agent:a").count(),
        2,
        "{actors:?}"
    );
    assert_eq!(
        actors.iter().filter(|a| **a == "human").count(),
        5,
        "{actors:?}"
    );
}

#[test]
fn bad_manifests_say_what_is_wrong() {
    let dir = folder("bad");
    let mut ws = project(&dir);
    let package = theme_package(&dir, "p", "com.example.p", "1.0.0", "MIT");
    let manifest_path = package.join("wish-package.json");
    let base: Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    let cases: Vec<(Value, &str, &str)> = vec![
        (json!({ "colour": 1 }), "package_invalid", "/colour"),
        (json!({ "id": "Dusk" }), "package_invalid", "/id"),
        (json!({ "version": "1.0" }), "invalid_version", "/version"),
        (
            json!({ "engine": "newest" }),
            "invalid_version_requirement",
            "/engine",
        ),
        (
            json!({ "contents": { "theme": ["themes/dusk.json"] } }),
            "package_invalid",
            "/contents/theme",
        ),
        (
            json!({ "contents": { "themes": ["../outside.json"] } }),
            "package_invalid",
            "/contents/themes/0",
        ),
        (
            json!({ "contents": { "themes": ["themes/none.json"] } }),
            "package_invalid",
            "/contents/themes/0",
        ),
        (
            json!({ "contents": { "templates": ["themes"] } }),
            "package_invalid",
            "/contents/templates/0",
        ),
        (json!({ "format": 2 }), "package_invalid", "/format"),
    ];
    for (change, code, pointer) in cases {
        let mut manifest = base.clone();
        for (key, value) in change.as_object().unwrap() {
            manifest[key] = value.clone();
        }
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        let got = call(
            &mut ws,
            "human",
            json!({ "action": "check", "path": package.display().to_string() }),
        );
        assert_eq!(got["error"]["code"], code, "{change}: {got}");
        assert_eq!(got["error"]["location"]["path"], pointer, "{change}: {got}");
        assert!(!got["error"]["hint"].as_str().unwrap().is_empty());
    }
    // The closest kind is suggested.
    let mut manifest = base.clone();
    manifest["contents"] = json!({ "theme": ["themes/dusk.json"] });
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();
    let got = call(
        &mut ws,
        "human",
        json!({ "action": "check", "path": package.display().to_string() }),
    );
    assert!(
        got["error"]["hint"].as_str().unwrap().contains("themes"),
        "{got}"
    );
    // No manifest at all.
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let got = call(
        &mut ws,
        "human",
        json!({ "action": "check", "path": empty.display().to_string() }),
    );
    assert_eq!(got["error"]["code"], "package_not_found", "{got}");
}

#[test]
fn what_needs_care_is_refused_or_warned() {
    let dir = folder("care");
    let mut ws = project(&dir);
    // The engine's own prefix, from outside the engine.
    let engine = theme_package(&dir, "engine", "wish.theme.fake", "1.0.0", "MIT");
    let got = call(
        &mut ws,
        "human",
        json!({ "action": "install", "path": engine.display().to_string() }),
    );
    assert_eq!(got["error"]["code"], "package_invalid", "{got}");
    // Another engine.
    let future = theme_package(&dir, "future", "com.example.future", "1.0.0", "MIT");
    let mut manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(future.join("wish-package.json")).unwrap())
            .unwrap();
    manifest["engine"] = json!(">=9.0");
    std::fs::write(future.join("wish-package.json"), manifest.to_string()).unwrap();
    let got = call(
        &mut ws,
        "human",
        json!({ "action": "install", "path": future.display().to_string() }),
    );
    assert_eq!(got["error"]["code"], "package_engine_mismatch", "{got}");
    // An unknown license installs, with a warning.
    let odd = theme_package(&dir, "odd", "com.example.odd", "1.0.0", "LicenseRef-Mine");
    let got = call(
        &mut ws,
        "human",
        json!({ "action": "install", "path": odd.display().to_string(), "dry_run": true }),
    );
    assert_eq!(got["ok"], true, "{got}");
    assert_eq!(
        got["diagnostics"][0]["code"], "package_license_unknown",
        "{got}"
    );
    // The person's own packages are the person's.
    let theme = theme_package(&dir, "mine", "com.example.mine", "1.0.0", "MIT");
    let got = call(
        &mut ws,
        "agent:a",
        json!({ "action": "install", "path": theme.display().to_string(), "scope": "user" }),
    );
    assert_eq!(got["error"]["code"], "permission_denied", "{got}");
    // A broken wish.json packages entry is explained.
    let root = ws.root().to_path_buf();
    let mut config: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("wish.json")).unwrap()).unwrap();
    config["packages"] = json!({ "com.example.x": "soon" });
    std::fs::write(root.join("wish.json"), config.to_string()).unwrap();
    let read = wish_control::project::read_project(&root).unwrap_err();
    assert_eq!(
        read.location.path.as_deref(),
        Some("/packages/com.example.x"),
        "{read:?}"
    );
}

#[test]
fn recipes_from_packages_are_searched() {
    let dir = folder("recipes");
    let mut ws = project(&dir);
    let package = dir.join("dialogue");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("wish-package.json"),
        json!({ "format": 1, "id": "com.example.dialogue", "name": "Dialogue kit", "version": "1.0.0",
            "engine": ">=0.1", "license": "MIT", "contents": { "recipes": ["branching.md"] } })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        package.join("branching.md"),
        "# Branching dialogue\n\nA conversation tree with choices: keep each line in a resource and show it with UiText.\n",
    )
    .unwrap();
    let before = ws.call(
        "docs_search",
        &json!({ "query": "conversation tree" }),
        &parse_actor("agent:a"),
    );
    assert_eq!(before["results"], json!([]), "{before}");
    let installed = call(
        &mut ws,
        "human",
        json!({ "action": "install", "path": package.display().to_string() }),
    );
    assert_eq!(installed["ok"], true, "{installed}");
    let found = ws.call(
        "docs_search",
        &json!({ "query": "conversation tree" }),
        &parse_actor("agent:a"),
    );
    assert_eq!(
        found["results"][0]["uri"], "wish://docs/packages/com.example.dialogue/branching.md",
        "{found}"
    );
    assert!(
        found["results"][0]["title"]
            .as_str()
            .unwrap()
            .contains("Branching dialogue")
    );
}
