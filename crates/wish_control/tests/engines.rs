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

/// An engine packaged before `opens_from` existed.
fn engine(version: &str, source: Source, folder: &str) -> engines::Engine {
    engines::Engine {
        version: v(version),
        folder: PathBuf::from(folder),
        source,
        opens_from: None,
    }
}

/// An engine that says from which version on it opens projects unchanged.
fn engine_from(version: &str, opens_from: &str, folder: &str) -> engines::Engine {
    engines::Engine {
        opens_from: Some(v(opens_from)),
        ..engine(version, Source::Installed, folder)
    }
}

#[test]
fn a_project_opens_with_its_own_version_else_the_newest_compatible() {
    // A format window at 2.0.0 and one at 0.3.0.
    let all = [
        engine_from("2.0.0", "2.0.0", "a"),
        engine_from("1.4.0", "1.0.0", "b"),
        engine_from("1.2.3", "1.0.0", "c"),
        engine_from("1.2.0", "1.0.0", "d"),
        engines::Engine {
            source: Source::Running,
            ..engine_from("1.2.0", "1.0.0", "here")
        },
        engine_from("0.3.1", "0.3.0", "e"),
        engine_from("0.3.0", "0.3.0", "f"),
    ];
    let pick = |wanted: &str| {
        engines::pick(v(wanted), &all).map(|(e, fit)| (e.folder.display().to_string(), fit))
    };
    // The exact version, the running engine first among equals.
    assert_eq!(pick("1.2.0"), Some(("here".to_owned(), Fit::Exact)));
    assert_eq!(pick("1.2.3"), Some(("c".to_owned(), Fit::Exact)));
    // Else the newest one that opens it, never an older one.
    assert_eq!(pick("1.1.0"), Some(("b".to_owned(), Fit::Compatible)));
    assert_eq!(pick("1.3.0"), Some(("b".to_owned(), Fit::Compatible)));
    assert_eq!(pick("0.3.0"), Some(("f".to_owned(), Fit::Exact)));
    assert_eq!(pick("0.2.0"), None, "a format window lies between");
    assert_eq!(pick("1.5.0"), None, "only older ones");
    assert_eq!(pick("3.0.0"), None);
}

/// Engines packaged before `opens_from` existed are judged by the caret
/// rule, and only for projects older than `UNSTATED_BEFORE`: a 0.1.0 package
/// from before the look lock (such as an early `wen package` folder) cannot
/// read scene format 2, which a 0.1.0 project may use, so it is not taken
/// even for a project of its own number (review of package 11a).
#[test]
fn an_engine_without_opens_from_is_not_taken_for_a_project_it_may_not_read() {
    let old = [
        engine("0.0.5", Source::Installed, "older"),
        engine("0.1.0", Source::Installed, "pre-lock"),
    ];
    let pick = |all: &[engines::Engine], wanted: &str| {
        engines::pick(v(wanted), all).map(|(e, fit)| (e.folder.display().to_string(), fit))
    };
    assert_eq!(pick(&old, "0.0.5"), Some(("older".to_owned(), Fit::Exact)));
    assert_eq!(pick(&old, "0.0.3"), None, "^0.0.3 is 0.0.3 only");
    assert_eq!(pick(&old, "0.1.0"), None, "not even its own number");
    // A pre-lock package folder as it is on disk: 0.1.0 and no key.
    let base = base("pre-lock");
    let folder = base.join("engines/0.1.0");
    fake_engine(&folder, "0.1.0");
    let pre_lock = engines::read_engine(&folder, Source::Installed).unwrap();
    assert_eq!(pre_lock.opens_from, None);
    // A newer engine that states the key takes the project instead.
    let all = [pre_lock, engine_from("0.2.0", "0.1.0", "stated")];
    assert_eq!(
        pick(&all, "0.1.0"),
        Some(("stated".to_owned(), Fit::Compatible))
    );
    // In a person's list, beside the running engine (0.1.0, stating it).
    let config = base.join("config");
    fake_engine(&config.join("engines/old-0.1.0"), "0.1.0");
    project(&base.join("game"), "0.1.0");
    let choice = engines::choose(&base.join("game"), Some(&config));
    assert!(choice.switch().is_none(), "{:?}", choice.picked);
}

/// ADR-027, rule 3: across 0.x the number is not a gate; an engine opens a
/// project unchanged from its `opens_from` up to its own version.
#[test]
fn across_0_x_an_engine_opens_every_project_from_its_opens_from_on() {
    let all = [
        engine_from("1.2.0", "1.2.0", "after-a-window"),
        engine_from("0.4.0", "0.1.0", "d"),
        engine_from("0.3.0", "0.1.0", "c"),
        engine("0.2.5", Source::Added, "old-package"),
    ];
    let pick = |wanted: &str| {
        engines::pick(v(wanted), &all).map(|(e, fit)| (e.folder.display().to_string(), fit))
    };
    // The newest that opens it, never an older one.
    assert_eq!(pick("0.2.0"), Some(("d".to_owned(), Fit::Compatible)));
    assert_eq!(pick("0.1.0"), Some(("d".to_owned(), Fit::Compatible)));
    assert_eq!(pick("0.3.0"), Some(("c".to_owned(), Fit::Exact)));
    // An old package without the key is not taken for a 0.1.0 or newer
    // project, even of its own number; 0.4.0 opens it.
    assert_eq!(pick("0.2.5"), Some(("d".to_owned(), Fit::Compatible)));
    // A format window between them: 1.2.0 opens only from 1.2.0 on.
    assert_eq!(pick("0.5.0"), None);
    assert_eq!(pick("1.1.0"), None);
    // Older than every opens_from: nobody opens it unchanged.
    assert_eq!(pick("0.0.9"), None);
    // The running engine says from which version on it opens projects.
    assert_eq!(engines::running().opens_from, Some(engines::OPENS_FROM));
    assert!(engines::OPENS_FROM <= Version::engine());
}

#[test]
fn a_0_2_project_opens_in_0_3_as_compatible_and_with_no_note() {
    let base = base("zero-x");
    let config = base.join("config");
    project(&base.join("game"), "0.2.0");
    // An installed 0.3.0 that opens projects from 0.1.0 on (as `wen package`
    // writes it).
    let folder = config.join("engines/0.3.0");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join(engines::ENGINE_FILE),
        json!({ "format": 1, "opens_from": "0.1.0", "version": "0.3.0" }).to_string(),
    )
    .unwrap();
    std::fs::write(folder.join(engines::WEN), "").unwrap();
    let answer =
        wish_control::engines_act_in(Some(&config), "pick", Some(&base.join("game")), None, false)
            .unwrap();
    assert_eq!(answer["fit"], "compatible", "{answer:?}");
    assert_eq!(answer["opens_with"]["version"], "0.3.0");
    assert_eq!(answer["opens_with"]["opens_from"], "0.1.0");
    let choice = engines::choose(&base.join("game"), Some(&config));
    assert!(choice.note().is_none(), "{:?}", choice.note());
    // The same engine without the key (packaged before it existed) is
    // judged by the caret rule, as before: 0.3.0 is not ^0.2.0.
    std::fs::write(
        folder.join(engines::ENGINE_FILE),
        json!({ "format": 1, "version": "0.3.0" }).to_string(),
    )
    .unwrap();
    let choice = engines::choose(&base.join("game"), Some(&config));
    assert!(choice.picked.is_none());
    // A key that is not a version, or names a newer one than the engine's
    // own, makes the folder not an engine.
    for bad in [json!("soon"), json!("0.4.0"), json!(3)] {
        std::fs::write(
            folder.join(engines::ENGINE_FILE),
            json!({ "format": 1, "opens_from": bad, "version": "0.3.0" }).to_string(),
        )
        .unwrap();
        let problem = engines::read_engine(&folder, Source::Installed).unwrap_err();
        assert_eq!(problem.code, "engine_folder_invalid", "{bad}");
        assert!(
            problem.message.contains("opens_from"),
            "{}",
            problem.message
        );
        assert!(
            problem.hint.contains("remove the key"),
            "a specific hint: {}",
            problem.hint
        );
    }
}

/// `opens_from` moves only in a format window (ADR-027, rule 3), and then
/// it must move: every format the engine writes is recorded here beside the
/// `opens_from` decided for it. Rows are never edited; a format change adds
/// a row with the first version that writes the new format.
#[test]
fn opens_from_moves_with_the_formats() {
    // (opens_from, [scene, project, material, meta, package manifest, game ABI])
    const DECIDED: &[(&str, [u64; 6])] = &[
        // The look lock (M12 package 9): scene files are format 2; 0.1.0 is
        // the only version there has been (M12-public-face-code, section 3).
        ("0.1.0", [2, 1, 1, 1, 1, 1]),
    ];
    let now = [
        u64::from(wish_control::SCENE_FORMAT),
        wish_control::PROJECT_FORMAT,
        u64::from(wish_asset::material::MATERIAL_FORMAT),
        u64::from(wish_asset::meta::META_FORMAT),
        wish_control::packages::MANIFEST_FORMAT,
        u64::try_from(wish_script::ABI_VERSION).unwrap(),
    ];
    for pair in DECIDED.windows(2) {
        assert_ne!(pair[0].1, pair[1].1, "a row without a format change");
        assert!(
            v(pair[0].0) < v(pair[1].0),
            "a format changed but opens_from did not move: {pair:?}"
        );
    }
    let (opens_from, formats) = DECIDED[DECIDED.len() - 1];
    assert_eq!(
        now, formats,
        "a format constant changed: that is a format window (ADR-027, rule 1). Add a row here with the new formats and set engines::OPENS_FROM to the first version that writes them"
    );
    assert_eq!(engines::OPENS_FROM, v(opens_from));
    // `wen package` writes it.
    let text = engines::engine_file_text(Version::engine()).unwrap();
    let file: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(file["opens_from"], opens_from);
    assert_eq!(file["format"], 1);
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
    assert!(note.contains("a file format has changed since"), "{note}");
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
