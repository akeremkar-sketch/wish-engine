//! The package index (M10b, design note M10 §14): a folder of packages or
//! an index.json is searched; add picks the newest version that fits the
//! engine; update stays compatible; a package whose files differ from the
//! index's hash is refused without a trace; a hostile index.json is
//! explained with a pointer.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("package-index-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn project(dir: &Path) -> Workspace {
    let root = dir.join("game");
    create_project(&root, "Index", "wen").unwrap();
    std::fs::remove_dir_all(root.join("scripts")).unwrap();
    Workspace::open(WorkspaceOptions {
        root,
        example: None,
    })
    .unwrap()
}

/// A theme package in `dir/<name>`.
fn package(dir: &Path, name: &str, id: &str, version: &str, engine: &str) -> PathBuf {
    let folder = dir.join(name);
    std::fs::create_dir_all(folder.join("themes")).unwrap();
    std::fs::write(
        folder.join("wish-package.json"),
        json!({
            "format": 1, "id": id, "name": format!("Theme {id}"), "version": version,
            "engine": engine, "license": "MIT",
            "description": format!("A dusky theme, version {version}."),
            "contents": { "themes": ["themes/look.json"] }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        folder.join("themes/look.json"),
        format!(r#"{{ "format": 1, "colors": {{}}, "note": "{version}" }}"#),
    )
    .unwrap();
    folder
}

fn call(ws: &mut Workspace, actor: &str, args: Value) -> Value {
    ws.call("packages", &args, &parse_actor(actor))
}

fn installed_version(root: &Path, id: &str) -> Option<String> {
    let text =
        std::fs::read_to_string(root.join("packages").join(id).join("wish-package.json")).ok()?;
    let manifest: Value = serde_json::from_str(&text).ok()?;
    manifest["version"].as_str().map(str::to_owned)
}

#[test]
fn a_folder_of_packages_is_searched_added_and_updated() {
    let dir = folder("scan");
    let index = dir.join("index");
    package(&index, "dusk-1", "com.example.dusk", "1.0.0", ">=0.1");
    package(
        &index.join("com.example.dusk"),
        "1.1.0",
        "com.example.dusk",
        "1.1.0",
        ">=0.1",
    );
    package(&index, "dusk-future", "com.example.dusk", "2.0.0", ">=9.0");
    package(&index, "mist", "com.example.mist", "0.1.0", ">=0.1");
    let index_arg = index.display().to_string();
    let mut ws = project(&dir);
    let root = ws.root().to_path_buf();

    // Everything, then by words.
    let all = call(
        &mut ws,
        "human",
        json!({ "action": "search", "index": index_arg }),
    );
    assert_eq!(all["ok"], true, "{all}");
    assert_eq!(all["count"], 2, "{all}");
    let dusk = call(
        &mut ws,
        "human",
        json!({ "action": "search", "index": index_arg, "query": "DUSK themes" }),
    );
    assert_eq!(dusk["count"], 2, "both are dusky themes: {dusk}");
    let mist = call(
        &mut ws,
        "human",
        json!({ "action": "search", "index": index_arg, "query": "mist" }),
    );
    assert_eq!(mist["count"], 1, "{mist}");
    let entry = &all["packages"][0];
    assert_eq!(entry["id"], "com.example.dusk");
    assert_eq!(
        entry["latest"], "1.1.0",
        "2.0.0 needs another engine: {entry}"
    );
    assert_eq!(entry["versions"], json!(["2.0.0", "1.1.0", "1.0.0"]));
    assert_eq!(entry["installed"]["project"], Value::Null);
    assert_eq!(entry["kinds"], json!(["themes"]));
    // A person searching only looks: it is not in the activity feed.
    let feed = ws.call("activity_get", &json!({}), &parse_actor("human"));
    assert!(!feed.to_string().contains("search"), "{feed}");

    // Added at an older version by an agent; a dry run changes nothing.
    let dry = call(
        &mut ws,
        "agent:a",
        json!({ "action": "add", "index": index_arg, "id": "com.example.dusk", "version": "=1.0.0", "dry_run": true }),
    );
    assert_eq!(dry["ok"], true, "{dry}");
    assert!(!root.join("packages").exists());
    let added = call(
        &mut ws,
        "agent:a",
        json!({ "action": "add", "index": index_arg, "id": "com.example.dusk", "version": "=1.0.0" }),
    );
    assert_eq!(added["ok"], true, "{added}");
    assert_eq!(
        installed_version(&root, "com.example.dusk").unwrap(),
        "1.0.0"
    );
    let lock = std::fs::read_to_string(root.join("wish.lock")).unwrap();
    assert!(lock.contains("1.0.0"), "{lock}");

    // The search sees it, and the update waiting.
    let seen = call(
        &mut ws,
        "human",
        json!({ "action": "search", "index": index_arg, "query": "com.example.dusk" }),
    );
    assert_eq!(
        seen["packages"][0]["installed"]["project"], "1.0.0",
        "{seen}"
    );
    assert_eq!(seen["packages"][0]["update"]["project"], true, "{seen}");
    assert_eq!(seen["packages"][0]["update"]["user"], false, "{seen}");

    // Adding it again needs replace.
    let again = call(
        &mut ws,
        "human",
        json!({ "action": "add", "index": index_arg, "id": "com.example.dusk" }),
    );
    assert_eq!(again["error"]["code"], "package_exists", "{again}");

    // Update: a dry run says what, then it is done; 2.0.0 stays out.
    let plan = call(
        &mut ws,
        "human",
        json!({ "action": "update", "index": index_arg, "dry_run": true }),
    );
    assert_eq!(
        plan["updated"],
        json!([{ "id": "com.example.dusk", "from": "1.0.0", "to": "1.1.0" }]),
        "{plan}"
    );
    assert_eq!(
        installed_version(&root, "com.example.dusk").unwrap(),
        "1.0.0"
    );
    let done = call(
        &mut ws,
        "human",
        json!({ "action": "update", "index": index_arg }),
    );
    assert_eq!(done["ok"], true, "{done}");
    assert_eq!(
        installed_version(&root, "com.example.dusk").unwrap(),
        "1.1.0"
    );
    let lock = std::fs::read_to_string(root.join("wish.lock")).unwrap();
    assert!(lock.contains("1.1.0"), "{lock}");
    let listed = call(&mut ws, "human", json!({}));
    assert_eq!(listed["diagnostics"], json!([]), "{listed}");
    let current = call(
        &mut ws,
        "human",
        json!({ "action": "update", "index": index_arg }),
    );
    assert_eq!(current["updated"], json!([]), "{current}");
    assert_eq!(current["current"], json!(["com.example.dusk"]), "{current}");

    // Back to an older version, asked for by id.
    let back = call(
        &mut ws,
        "human",
        json!({ "action": "update", "index": index_arg, "id": "com.example.dusk", "version": "=1.0.0" }),
    );
    assert_eq!(back["updated"][0]["to"], "1.0.0", "{back}");
    assert_eq!(
        installed_version(&root, "com.example.dusk").unwrap(),
        "1.0.0"
    );

    // What is not there is explained.
    let typo = call(
        &mut ws,
        "human",
        json!({ "action": "add", "index": index_arg, "id": "com.example.dusc" }),
    );
    assert_eq!(typo["error"]["code"], "package_not_found", "{typo}");
    assert!(
        typo["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("com.example.dusk"),
        "{typo}"
    );
    let none = call(
        &mut ws,
        "human",
        json!({ "action": "add", "index": index_arg, "id": "com.example.mist", "version": "^3" }),
    );
    assert_eq!(none["error"]["code"], "package_version_not_found", "{none}");
    assert!(
        none["error"]["hint"].as_str().unwrap().contains("0.1.0"),
        "{none}"
    );
    let not_installed = call(
        &mut ws,
        "human",
        json!({ "action": "update", "index": index_arg, "id": "com.example.mist" }),
    );
    assert_eq!(
        not_installed["error"]["code"], "package_not_found",
        "{not_installed}"
    );
    let version_alone = call(
        &mut ws,
        "human",
        json!({ "action": "update", "index": index_arg, "version": "^1" }),
    );
    assert_eq!(
        version_alone["error"]["code"], "missing_parameter",
        "{version_alone}"
    );
    let nowhere = call(
        &mut ws,
        "human",
        json!({ "action": "search", "index": dir.join("nowhere").display().to_string() }),
    );
    assert_eq!(
        nowhere["error"]["code"], "package_index_missing",
        "{nowhere}"
    );
    // The person's own packages stay the person's.
    let theirs = call(
        &mut ws,
        "agent:a",
        json!({ "action": "add", "index": index_arg, "id": "com.example.mist", "scope": "user" }),
    );
    assert_eq!(theirs["error"]["code"], "permission_denied", "{theirs}");
}

#[test]
fn an_index_file_names_versions_and_their_hashes() {
    let dir = folder("file");
    let index = dir.join("index");
    let one = package(
        &index.join("dusk"),
        "1.0.0",
        "com.example.dusk",
        "1.0.0",
        ">=0.1",
    );
    let mut ws = project(&dir);
    let root = ws.root().to_path_buf();
    let checked = call(
        &mut ws,
        "human",
        json!({ "action": "check", "path": one.display().to_string() }),
    );
    let hash = checked["hash"].as_str().unwrap().to_owned();
    let write = |packages: Value| {
        std::fs::write(
            index.join("index.json"),
            json!({ "format": 1, "name": "Test", "packages": packages }).to_string(),
        )
        .unwrap();
    };
    let index_arg = index.display().to_string();
    let entry = |hash: &str, path: &str| {
        json!([{
            "id": "com.example.dusk", "name": "Dusk", "description": "Evening colours.",
            "license": "MIT", "kinds": ["themes", "not-a-kind"],
            "versions": [ { "version": "1.0.0", "engine": ">=0.1", "hash": hash, "path": path } ]
        }])
    };

    // A wrong hash: refused, and nothing is left behind.
    write(entry(&"0".repeat(32), "dusk/1.0.0"));
    let found = call(
        &mut ws,
        "human",
        json!({ "action": "search", "index": index_arg }),
    );
    assert_eq!(
        found["packages"][0]["name"], "Dusk",
        "the index's words: {found}"
    );
    assert_eq!(found["packages"][0]["kinds"], json!(["themes"]), "{found}");
    let wrong = call(
        &mut ws,
        "human",
        json!({ "action": "add", "index": index_arg, "id": "com.example.dusk" }),
    );
    assert_eq!(wrong["error"]["code"], "package_hash_mismatch", "{wrong}");
    let left: Vec<_> = std::fs::read_dir(root.join("packages"))
        .map(|entries| entries.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "nothing half-installed: {left:?}");
    assert!(!root.join("wish.lock").exists());

    // The right hash installs.
    write(entry(&hash, "dusk/1.0.0"));
    let right = call(
        &mut ws,
        "human",
        json!({ "action": "add", "index": index_arg, "id": "com.example.dusk" }),
    );
    assert_eq!(right["ok"], true, "{right}");
    assert_eq!(
        installed_version(&root, "com.example.dusk").unwrap(),
        "1.0.0"
    );

    // A folder holding another version than the index says.
    package(
        &index.join("dusk"),
        "1.2.0",
        "com.example.dusk",
        "1.0.0",
        ">=0.1",
    );
    write(json!([{
        "id": "com.example.dusk",
        "versions": [ { "version": "1.2.0", "engine": ">=0.1", "path": "dusk/1.2.0" } ]
    }]));
    let stale = call(
        &mut ws,
        "human",
        json!({ "action": "update", "index": index_arg }),
    );
    // Updating everything lists it as failed (the others would still go);
    // asked for by id, it is the answer.
    assert_eq!(
        stale["failed"][0]["code"], "package_index_invalid",
        "{stale}"
    );
    assert_eq!(stale["updated"], json!([]), "{stale}");
    let stale = call(
        &mut ws,
        "human",
        json!({ "action": "update", "id": "com.example.dusk", "index": index_arg }),
    );
    assert_eq!(stale["error"]["code"], "package_index_invalid", "{stale}");

    // Hostile index files, each explained with a pointer.
    let cases = [
        (json!({ "format": 2, "packages": [] }), "/format"),
        (json!({ "format": 1 }), "/packages"),
        (
            json!({ "format": 1, "packages": [ { "id": "Not An Id", "versions": [] } ] }),
            "/packages/0/id",
        ),
        (
            json!({ "format": 1, "packages": [ { "id": "com.example.a", "versions": [] } ] }),
            "/packages/0/versions",
        ),
        (
            json!({ "format": 1, "packages": [ { "id": "com.example.a", "versions": [
                { "version": "1.0.0", "engine": ">=0.1", "path": "../outside" } ] } ] }),
            "/packages/0/versions/0/path",
        ),
        (
            json!({ "format": 1, "packages": [ { "id": "com.example.a", "versions": [
                { "version": "1.0.0", "engine": ">=0.1", "path": "C:/outside" } ] } ] }),
            "/packages/0/versions/0/path",
        ),
        (
            json!({ "format": 1, "packages": [ { "id": "com.example.a", "versions": [
                { "version": "one", "engine": ">=0.1", "path": "a" } ] } ] }),
            "/packages/0/versions/0/version",
        ),
        (
            json!({ "format": 1, "packages": [ { "id": "com.example.a", "versions": [
                { "version": "1.0.0", "engine": ">=0.1", "path": "a", "hash": "xyz" } ] } ] }),
            "/packages/0/versions/0/hash",
        ),
        (
            json!({ "format": 1, "packages": [
                { "id": "com.example.a", "versions": [ { "version": "1.0.0", "engine": ">=0.1", "path": "a" } ] },
                { "id": "com.example.a", "versions": [ { "version": "1.0.1", "engine": ">=0.1", "path": "b" } ] } ] }),
            "/packages/1/id",
        ),
    ];
    for (bad, pointer) in cases {
        std::fs::write(index.join("index.json"), bad.to_string()).unwrap();
        let got = call(
            &mut ws,
            "human",
            json!({ "action": "search", "index": index_arg }),
        );
        let error = &got["error"];
        assert!(
            error["code"] == "package_index_invalid"
                || error["code"] == "invalid_version"
                || error["code"] == "invalid_version_requirement",
            "{bad}: {got}"
        );
        assert_eq!(error["location"]["path"], pointer, "{bad}: {got}");
        assert!(!error["hint"].as_str().unwrap_or("").is_empty(), "{got}");
    }
    std::fs::write(index.join("index.json"), "{ not json").unwrap();
    let got = call(
        &mut ws,
        "human",
        json!({ "action": "search", "index": index_arg }),
    );
    assert_eq!(got["error"]["code"], "package_index_invalid", "{got}");
}
