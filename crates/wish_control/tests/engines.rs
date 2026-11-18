//! Engine versions side by side (M10b; design note M10 §15): adding,
//! listing and removing engines, and which engine opens a project.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_control::engines::{self, Fit, Source};
use wish_control::packages::Version;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn base(name: &str) -> PathBuf {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("engines-{name}"));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
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

fn project(folder: &Path, made_with: &str) {
    std::fs::create_dir_all(folder.join("scenes")).unwrap();
    std::fs::write(
        folder.join("wish.json"),
        json!({ "engine": made_with, "format": 1, "name": "Game",
            "scene": "scenes/main.scene.json" })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        folder.join("scenes/main.scene.json"),
        r#"{ "format": 1, "entities": {} }"#,
    )
    .unwrap();
}

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn versions(config: &Path) -> Vec<String> {
    engines::engines(Some(config))
        .0
        .iter()
        .map(|e| format!("{}:{}", e.version, e.source.name()))
        .collect()
}

#[test]
fn engines_are_added_listed_and_removed() {
    let base = base("registry");
    let config = base.join("config");
    let here = Version::engine().to_string();
    assert_eq!(versions(&config), [format!("{here}:running")]);

    // Added where it is; adding it again changes nothing.
    fake_engine(&base.join("old"), "0.0.5");
    let (engine, new) = engines::add(&config, &base.join("old"), true).unwrap();
    assert!(new && engine.version == v("0.0.5"));
    assert!(!config.join(engines::REGISTRY_FILE).exists(), "dry run");
    engines::add(&config, &base.join("old"), false).unwrap();
    let (_, new) = engines::add(&config, &base.join("old"), false).unwrap();
    assert!(!new);
    // Installed ones are found in <config>/engines/.
    fake_engine(&config.join("engines/9.1.0"), "9.1.0");
    assert_eq!(
        versions(&config),
        [
            "9.1.0:installed".to_owned(),
            format!("{here}:running"),
            "0.0.5:added".to_owned()
        ]
    );
    // The list file is canonical.
    let text = std::fs::read_to_string(config.join(engines::REGISTRY_FILE)).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["format"], 1);
    assert_eq!(value["folders"].as_array().unwrap().len(), 1);

    // Not engines: explained, nothing added.
    std::fs::create_dir_all(base.join("empty")).unwrap();
    let problem = engines::add(&config, &base.join("empty"), false).unwrap_err();
    assert_eq!(problem.code, "engine_folder_invalid");
    assert!(
        problem.message.contains("wish-engine.json"),
        "{}",
        problem.message
    );
    assert!(!problem.message.contains(&base.display().to_string()));
    fake_engine(&base.join("bad"), "soon");
    assert_eq!(
        engines::add(&config, &base.join("bad"), false)
            .unwrap_err()
            .code,
        "engine_folder_invalid"
    );

    // A folder that stopped being an engine is a warning in the list, and
    // can still be forgotten.
    fake_engine(&base.join("gone"), "0.0.7");
    engines::add(&config, &base.join("gone"), false).unwrap();
    std::fs::remove_dir_all(base.join("gone")).unwrap();
    let (_, warnings) = engines::engines(Some(&config));
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].code, "engine_folder_invalid");
    engines::remove(&config, &base.join("gone"), false).unwrap();
    assert!(engines::engines(Some(&config)).1.is_empty());

    // Removing: an added one is forgotten (its files stay); an installed
    // one goes to the trash; the running one cannot remove itself.
    let removed = engines::remove(&config, &base.join("old"), false).unwrap();
    assert!(removed.trash.is_none());
    assert!(base.join("old").join(engines::ENGINE_FILE).is_file());
    let dry = engines::remove(&config, &config.join("engines/9.1.0"), true).unwrap();
    assert!(dry.trash.is_some());
    assert!(config.join("engines/9.1.0").is_dir(), "dry run");
    let removed = engines::remove(&config, &config.join("engines/9.1.0"), false).unwrap();
    let trash = removed.trash.unwrap();
    assert!(trash.join(engines::ENGINE_FILE).is_file());
    assert!(trash.starts_with(config.join("trash")));
    assert_eq!(versions(&config), [format!("{here}:running")]);
    let running = engines::running();
    assert_eq!(
        engines::remove(&config, &running.folder, false)
            .unwrap_err()
            .code,
        "engine_running"
    );
    assert_eq!(
        engines::remove(&config, &base.join("nowhere"), false)
            .unwrap_err()
            .code,
        "engine_not_found"
    );

    // A broken list is read as empty, with a warning.
    std::fs::write(config.join(engines::REGISTRY_FILE), "{ broken").unwrap();
    let (found, warnings) = engines::engines(Some(&config));
    assert_eq!(found.len(), 1);
    assert_eq!(warnings[0].code, "engine_registry_invalid");
    // Adding writes it anew.
    engines::add(&config, &base.join("old"), false).unwrap();
    assert!(engines::engines(Some(&config)).1.is_empty());
}

fn engine(version: &str, source: Source, folder: &str) -> engines::Engine {
    engines::Engine {
        version: v(version),
        folder: PathBuf::from(folder),
        source,
    }
}

#[test]
fn a_project_opens_with_its_own_version_else_the_newest_compatible() {
    let all = [
        engine("2.0.0", Source::Installed, "a"),
        engine("1.4.0", Source::Added, "b"),
        engine("1.2.3", Source::Installed, "c"),
        engine("1.2.0", Source::Added, "d"),
        engine("1.2.0", Source::Running, "here"),
        engine("0.3.1", Source::Added, "e"),
        engine("0.3.0", Source::Added, "f"),
    ];
    let pick = |wanted: &str| {
        engines::pick(v(wanted), &all).map(|(e, fit)| (e.folder.display().to_string(), fit))
    };
    // The exact version, the running engine first among equals.
    assert_eq!(pick("1.2.0"), Some(("here".to_owned(), Fit::Exact)));
    assert_eq!(pick("1.2.3"), Some(("c".to_owned(), Fit::Exact)));
    // Else the newest compatible one, never an older one.
    assert_eq!(pick("1.1.0"), Some(("b".to_owned(), Fit::Compatible)));
    assert_eq!(pick("1.3.0"), Some(("b".to_owned(), Fit::Compatible)));
    assert_eq!(pick("0.3.0"), Some(("f".to_owned(), Fit::Exact)));
    assert_eq!(pick("0.2.0"), None, "0.x minors are not compatible");
    assert_eq!(pick("1.5.0"), None, "only older ones");
    assert_eq!(pick("3.0.0"), None);
}

#[test]
fn the_engines_tool_lists_picks_and_is_a_persons() {
    let base = base("tool");
    let here = Version::engine().to_string();
    project(&base.join("game"), &here);
    let mut ws = Workspace::open(WorkspaceOptions {
        root: base.join("game"),
        example: None,
    })
    .unwrap();
    let mut call = |args: Value, actor: &str| ws.call("engines", &args, &parse_actor(actor));

    let listed = call(json!({}), "human");
    assert_eq!(listed["ok"], true, "{listed}");
    assert_eq!(listed["running"], here);
    assert_eq!(listed["project"]["project_engine"], here);
    assert_eq!(listed["project"]["fit"], "exact");
    assert_eq!(listed["project"]["switch"], false);
    assert_eq!(listed["project"]["opens_with"]["source"], "running");

    // Another project, made with a version nobody has.
    project(&base.join("future"), "99.0.0");
    let picked = call(json!({ "action": "pick", "path": "../future" }), "agent:x");
    assert_eq!(picked["ok"], true, "{picked}");
    assert_eq!(picked["fit"], "none");
    assert_eq!(picked["switch"], false);
    let not_project = call(json!({ "action": "pick", "path": "../nowhere" }), "human");
    assert_eq!(not_project["error"]["code"], "project_not_found");

    // An agent cannot add or remove engines.
    fake_engine(&base.join("other"), "0.0.5");
    let refused = call(
        json!({ "action": "add", "path": base.join("other").display().to_string() }),
        "agent:x",
    );
    assert_eq!(refused["error"]["code"], "permission_denied");
    let missing = call(json!({ "action": "remove" }), "human");
    assert_eq!(missing["error"]["code"], "missing_parameter");
}

#[test]
fn a_note_says_when_no_engine_fits() {
    let base = base("note");
    let config = base.join("config");
    project(&base.join("future"), "99.0.0");
    let choice = engines::choose(&base.join("future"), Some(&config));
    assert!(choice.switch().is_none());
    assert!(choice.note().unwrap().contains("newer than this one"));
    project(&base.join("ancient"), "0.0.1");
    let note = engines::choose(&base.join("ancient"), Some(&config))
        .note()
        .unwrap();
    assert!(note.contains("no engine compatible"), "{note}");
    // With that engine installed, the project goes to it.
    fake_engine(&config.join("engines/0.0.1"), "0.0.1");
    let choice = engines::choose(&base.join("ancient"), Some(&config));
    assert_eq!(choice.switch().unwrap().version, v("0.0.1"));
    assert!(choice.note().is_none());
    // A project of this engine opens here.
    project(&base.join("now"), &Version::engine().to_string());
    let choice = engines::choose(&base.join("now"), Some(&config));
    assert!(choice.switch().is_none() && choice.note().is_none());
}
