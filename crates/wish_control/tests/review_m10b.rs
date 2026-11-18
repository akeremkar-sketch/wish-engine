//! Regression tests for the M10b closing review (design note M10 §16):
//! package index updates that keep going past a failing package, links
//! out of an index, the engine prefix behind `..`, partial versions in
//! requirements, oversized manifests, and the list of added engines
//! (newer formats, broken files, the limit, entries listed twice).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};
use wish_control::engines::{self, Source};
use wish_control::packages::{self, Version, VersionReq};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("review-m10b-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn project(dir: &Path) -> Workspace {
    let root = dir.join("game");
    create_project(&root, "Review", "wen").unwrap();
    std::fs::remove_dir_all(root.join("scripts")).unwrap();
    Workspace::open(WorkspaceOptions {
        root,
        example: None,
    })
    .unwrap()
}

/// A theme package in `dir/<name>`.
fn package(dir: &Path, name: &str, id: &str, version: &str) -> PathBuf {
    let folder = dir.join(name);
    std::fs::create_dir_all(folder.join("themes")).unwrap();
    std::fs::write(
        folder.join("wish-package.json"),
        json!({
            "format": 1, "id": id, "name": format!("Theme {id}"), "version": version,
            "engine": ">=0.1", "license": "MIT",
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

fn call(ws: &mut Workspace, args: Value) -> Value {
    ws.call("packages", &args, &parse_actor("human"))
}

#[test]
fn partial_versions_in_requirements_mean_every_version_they_start() {
    let r = |text: &str| VersionReq::parse(text).unwrap();
    let v = |text: &str| Version::parse(text).unwrap();
    let yes = |req: &str, version: &str| assert!(r(req).matches(v(version)), "{req} {version}");
    let no = |req: &str, version: &str| assert!(!r(req).matches(v(version)), "{req} !{version}");
    yes("=1.2", "1.2.7");
    no("=1.2", "1.3.0");
    yes("=1", "1.9.0");
    no("=1", "2.0.0");
    yes("<=1.2", "1.2.5");
    no("<=1.2", "1.3.0");
    yes("<=1", "1.9.9");
    no(">1.2", "1.2.1");
    yes(">1.2", "1.3.0");
    no(">1", "1.9.0");
    yes(">1", "2.0.0");
    yes("~1", "1.4.0");
    no("~1", "2.0.0");
    yes("~1.2", "1.2.9");
    no("~1.2", "1.3.0");
    yes("^0", "0.9.3");
    no("^0", "1.0.0");
    yes("^0.0", "0.0.7");
    no("^0.0", "0.1.0");
    yes("^0.0.3", "0.0.3");
    no("^0.0.3", "0.0.4");
    yes("^0.2", "0.2.9");
    no("^0.2", "0.3.0");
    // Whole versions keep their meaning.
    yes("^1.2", "1.9.0");
    no("^1.2", "1.1.9");
    yes("=1.2.3", "1.2.3");
    no("=1.2.3", "1.2.4");
    yes(">=1.2", "1.2.0");
    yes("<1.2", "1.1.9");
    no("<1.2", "1.2.0");
}

#[test]
fn an_update_keeps_going_past_a_package_that_fails_and_locks_what_went_in() {
    let dir = folder("update");
    let index = dir.join("index");
    package(&index, "dusk-1.0.0", "com.example.dusk", "1.0.0");
    package(&index, "dusk-1.1.0", "com.example.dusk", "1.1.0");
    package(&index, "mist-1.0.0", "com.example.mist", "1.0.0");
    // The index is out of date: this folder holds 1.2.0, not 1.1.0.
    package(&index, "mist-wrong", "com.example.mist", "1.2.0");
    let version = |v: &str, path: &str| json!({ "version": v, "engine": ">=0.1", "path": path });
    std::fs::write(
        index.join("index.json"),
        json!({ "format": 1, "packages": [
            { "id": "com.example.dusk", "versions": [
                version("1.0.0", "dusk-1.0.0"), version("1.1.0", "dusk-1.1.0") ] },
            { "id": "com.example.mist", "versions": [
                version("1.0.0", "mist-1.0.0"), version("1.1.0", "mist-wrong") ] },
        ] })
        .to_string(),
    )
    .unwrap();
    let index_arg = index.display().to_string();
    let mut ws = project(&dir);
    let root = ws.root().to_path_buf();
    for id in ["com.example.dusk", "com.example.mist"] {
        let added = call(
            &mut ws,
            json!({ "action": "add", "id": id, "version": "=1.0", "index": index_arg }),
        );
        assert_eq!(added["ok"], true, "{added}");
        assert_eq!(added["package"]["version"], "1.0.0", "{added}");
    }

    // A dry run says what would happen, and changes nothing.
    let dry = call(
        &mut ws,
        json!({ "action": "update", "index": index_arg, "dry_run": true }),
    );
    assert_eq!(dry["ok"], true, "{dry}");
    assert_eq!(dry["updated"][0]["to"], "1.1.0", "{dry}");
    assert_eq!(dry["failed"][0]["id"], "com.example.mist", "{dry}");
    let lock_before = std::fs::read_to_string(root.join("wish.lock")).unwrap();

    // The search names, per scope, where an update waits.
    let seen = call(
        &mut ws,
        json!({ "action": "search", "index": index_arg, "query": "dusk" }),
    );
    assert_eq!(seen["packages"][0]["update"]["project"], true, "{seen}");
    assert_eq!(seen["packages"][0]["update"]["user"], false, "{seen}");

    let done = call(&mut ws, json!({ "action": "update", "index": index_arg }));
    assert_eq!(done["ok"], true, "{done}");
    assert_eq!(
        done["updated"],
        json!([{ "id": "com.example.dusk", "from": "1.0.0", "to": "1.1.0" }]),
        "{done}"
    );
    assert_eq!(done["failed"][0]["code"], "package_index_invalid", "{done}");
    // What went in is locked; what failed stayed as it was.
    let lock = std::fs::read_to_string(root.join("wish.lock")).unwrap();
    assert_ne!(lock, lock_before);
    assert!(lock.contains("1.1.0"), "{lock}");
    let listed = call(&mut ws, json!({ "action": "list" }));
    assert_eq!(listed["diagnostics"], json!([]), "{listed}");

    // Asked for by id, the problem is the answer.
    let one = call(
        &mut ws,
        json!({ "action": "update", "id": "com.example.mist", "index": index_arg }),
    );
    assert_eq!(one["error"]["code"], "package_index_invalid", "{one}");
    // An installed package whose manifest is broken is not "up to date".
    std::fs::write(
        root.join("packages/com.example.mist/wish-package.json"),
        "{ broken",
    )
    .unwrap();
    let broken = call(
        &mut ws,
        json!({ "action": "update", "id": "com.example.mist", "index": index_arg }),
    );
    assert_eq!(broken["ok"], false, "{broken}");
    let all = call(&mut ws, json!({ "action": "update", "index": index_arg }));
    assert_eq!(all["failed"][0]["id"], "com.example.mist", "{all}");
}

/// Makes `link` lead to `target` (a junction on Windows, which needs no
/// rights); false when this system cannot.
fn link(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    {
        // (mklink reads a forward slash as one of its switches.)
        let windows = |path: &Path| path.display().to_string().replace('/', "\\");
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(windows(link))
            .arg(windows(target))
            .output()
            .is_ok_and(|out| out.status.success())
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

#[test]
fn an_index_folder_that_leads_out_of_the_index_is_refused() {
    let dir = folder("link");
    let outside = dir.join("outside");
    package(&outside, "1.0.0", "com.example.dusk", "1.0.0");
    let index = dir.join("index");
    std::fs::create_dir_all(&index).unwrap();
    if !link(&outside, &index.join("dusk")) {
        eprintln!("links cannot be made here; skipped");
        return;
    }
    std::fs::write(
        index.join("index.json"),
        json!({ "format": 1, "packages": [ { "id": "com.example.dusk", "versions": [
            { "version": "1.0.0", "engine": ">=0.1", "path": "dusk/1.0.0" } ] } ] })
        .to_string(),
    )
    .unwrap();
    let mut ws = project(&dir);
    let added = call(
        &mut ws,
        json!({ "action": "add", "id": "com.example.dusk", "index": index.display().to_string() }),
    );
    assert_eq!(added["error"]["code"], "package_index_invalid", "{added}");
    assert!(!ws.root().join("packages/com.example.dusk").exists());
}

#[test]
fn the_engine_prefix_is_not_reached_through_dot_dot() {
    let dir = folder("prefix");
    let fake = package(&dir, "wish.theme.fake", "wish.theme.fake", "1.0.0");
    let engine = packages::engine_packages_folder().expect("the repository's packages");
    // <engine packages>/../../…/<fake>: it starts with the engine's folder
    // only as text.
    let mut sneaky = engine.clone();
    for part in engine.components() {
        if matches!(part, Component::Normal(_)) {
            sneaky.push("..");
        }
    }
    for part in fake.components() {
        if let Component::Normal(name) = part {
            sneaky.push(name);
        }
    }
    assert!(sneaky.starts_with(&engine));
    let mut ws = project(&dir);
    let installed = call(
        &mut ws,
        json!({ "action": "install", "path": sneaky.display().to_string() }),
    );
    assert_eq!(installed["error"]["code"], "package_invalid", "{installed}");
}

#[test]
fn an_oversized_manifest_is_not_read() {
    let dir = folder("manifest");
    let big = package(&dir, "big", "com.example.big", "1.0.0");
    let mut text = std::fs::read_to_string(big.join("wish-package.json")).unwrap();
    text.pop();
    text.push_str(&format!(r#", "padding": "{}" }}"#, "x".repeat(300 * 1024)));
    std::fs::write(big.join("wish-package.json"), text).unwrap();
    let problem = packages::read_manifest(&big).unwrap_err();
    assert_eq!(problem.code, "package_invalid");
    assert!(
        problem.message.contains("larger than"),
        "{}",
        problem.message
    );
}

fn fake_engine(folder: &Path, version: &str) {
    std::fs::create_dir_all(folder).unwrap();
    std::fs::write(
        folder.join(engines::ENGINE_FILE),
        json!({ "format": 1, "version": version }).to_string(),
    )
    .unwrap();
    std::fs::write(folder.join(engines::WEN), "").unwrap();
}

fn registry(config: &Path, folders: &[&Path]) {
    std::fs::create_dir_all(config).unwrap();
    let list: Vec<String> = folders.iter().map(|f| f.display().to_string()).collect();
    std::fs::write(
        config.join(engines::REGISTRY_FILE),
        json!({ "format": 1, "folders": list }).to_string(),
    )
    .unwrap();
}

fn listed(config: &Path) -> usize {
    engines::added_folders(config).0.len()
}

#[test]
fn a_list_of_engines_from_a_newer_engine_is_never_overwritten() {
    let dir = folder("registry-newer");
    let config = dir.join("config");
    fake_engine(&dir.join("old"), "0.0.5");
    std::fs::create_dir_all(&config).unwrap();
    let newer = r#"{ "format": 2, "folders": [{ "path": "D:/Wish/3.0" }] }"#;
    std::fs::write(config.join(engines::REGISTRY_FILE), newer).unwrap();
    let refused = engines::add(&config, &dir.join("old"), false).unwrap_err();
    assert_eq!(refused.code, "engine_registry_invalid");
    let refused = engines::remove(&config, &dir.join("old"), false).unwrap_err();
    assert_eq!(refused.code, "engine_not_found");
    assert_eq!(
        std::fs::read_to_string(config.join(engines::REGISTRY_FILE)).unwrap(),
        newer
    );

    // A broken one is kept aside, and the list is written anew.
    std::fs::write(config.join(engines::REGISTRY_FILE), "{ broken").unwrap();
    engines::add(&config, &dir.join("old"), true).unwrap();
    assert!(!config.join("engines.json.bak").exists(), "dry run");
    engines::add(&config, &dir.join("old"), false).unwrap();
    assert_eq!(
        std::fs::read_to_string(config.join("engines.json.bak")).unwrap(),
        "{ broken"
    );
    assert_eq!(listed(&config), 1);
}

#[test]
fn the_engine_limit_counts_every_listed_folder() {
    let dir = folder("registry-limit");
    let config = dir.join("config");
    let gone: Vec<PathBuf> = (0..63)
        .map(|n| dir.join(format!("unplugged-{n}")))
        .collect();
    let refs: Vec<&Path> = gone.iter().map(PathBuf::as_path).collect();
    registry(&config, &refs);
    fake_engine(&dir.join("one-more"), "0.0.5");
    let refused = engines::add(&config, &dir.join("one-more"), false).unwrap_err();
    assert_eq!(refused.code, "engine_too_many");
    // Nothing was dropped from the list.
    assert_eq!(listed(&config), 63);
    // The gone folders are listed apart, and can be forgotten.
    let listing = engines::listing(Some(&config));
    assert_eq!(listing.stale.len(), 63);
    assert!(listing.warnings.is_empty(), "{:?}", listing.warnings);
    assert_eq!(listing.all_warnings().len(), 63);
    let forgotten = engines::remove(&config, &gone[0], false).unwrap();
    assert!(forgotten.stale);
    assert_eq!(listed(&config), 62);
    engines::add(&config, &dir.join("one-more"), false).unwrap();
}

#[test]
fn an_engine_listed_twice_is_forgotten_with_it() {
    let dir = folder("registry-twice");
    let config = dir.join("config");
    // Installed, and added by hand too.
    let installed = config.join(engines::ENGINES_FOLDER).join("9.1.0");
    fake_engine(&installed, "9.1.0");
    let running = engines::running().folder;
    registry(&config, &[&installed, &running]);
    let all = engines::engines(Some(&config)).0;
    assert_eq!(all.len(), 2, "each once: {all:?}");
    assert_eq!(all[0].source, Source::Installed);
    let removed = engines::remove(&config, &installed, false).unwrap();
    assert!(removed.trash.is_some());
    // The running engine's folder in the list is forgotten; the engine
    // itself still cannot be removed.
    let removed = engines::remove(&config, &running, false).unwrap();
    assert!(removed.trash.is_none() && !removed.stale);
    assert_eq!(listed(&config), 0);
    assert!(engines::engines(Some(&config)).1.is_empty());
    assert_eq!(
        engines::remove(&config, &running, false).unwrap_err().code,
        "engine_running"
    );
}
