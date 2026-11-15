//! Whole-project loading, change watching and hot reloading.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::PathBuf;

use serde_json::json;
use wish_asset::{FileKind, HotScene, Watcher, load_project};
use wish_core::AssetRef;
use wish_core::math::Vec3;
use wish_ecs::World;
use wish_schema::component;

#[component]
pub struct Model {
    pub mesh: AssetRef,
}

#[component]
pub struct Transform {
    pub translation: Vec3,
}

fn world() -> World {
    let mut world = World::new();
    world.register_component::<Model>().unwrap();
    world.register_component::<Transform>().unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    world
}

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("wish-project-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn write(&self, file: &str, text: &str) {
        let path = self.0.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const PREFAB: &str = "{\n  \"entities\": {\n    \"root\": {}\n  },\n  \"format\": 1\n}\n";

fn scene(extra: &str) -> String {
    format!(
        "{{\n  \"entities\": {{\n    \"a\": {{}},\n    \"m\": {{\n      \"components\": {{\n        \"Model\": {{\n          \"mesh\": \"assets/m.glb\"\n        }}\n      }}\n    }}{extra}\n  }},\n  \"format\": 1\n}}\n"
    )
}

fn project(name: &str) -> Dir {
    let dir = Dir::new(name);
    dir.write("assets/m.glb", "data");
    dir.write("prefabs/p.prefab.json", PREFAB);
    dir.write("main.scene.json", &scene(""));
    dir
}

#[test]
fn a_project_is_checked_as_a_whole_with_every_problem_reported() {
    let dir = project("whole");
    let world = world();
    let project = load_project(&dir.0, world.registry()).unwrap();
    assert!(!project.has_errors(), "{:?}", project.diagnostics);
    assert!(project.diagnostics.is_empty(), "canonical files are silent");
    let kinds: Vec<(&str, FileKind)> = project
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("main.scene.json", FileKind::Scene),
            ("prefabs/p.prefab.json", FileKind::Prefab)
        ]
    );
    assert!(
        project
            .files
            .iter()
            .all(|f| f.canonical.as_deref() == Some(f.source.as_str()))
    );
    assert_eq!(project.prefabs.paths(), ["prefabs/p.prefab.json"]);
    assert!(project.scenes.contains_key("main.scene.json"));

    // Three different mistakes in three files.
    dir.write("assets/m.glb.meta.json", "{ \"format\": 3 }");
    dir.write(
        "prefabs/bad.prefab.json",
        "{ \"format\": 1, \"entities\": {} }",
    );
    dir.write(
        "extra.scene.json",
        &scene(",\n    \"x\": { \"components\": { \"Nope\": {} } }"),
    );
    fs::remove_file(dir.0.join("assets/m.glb")).unwrap();
    let project = load_project(&dir.0, world.registry()).unwrap();
    let found: Vec<(&str, &str)> = project
        .diagnostics
        .iter()
        .map(|d| (d.location.file.as_deref().unwrap(), d.code.as_ref()))
        .collect();
    assert!(
        found.contains(&("assets/m.glb.meta.json", "unsupported_format")),
        "{found:?}"
    );
    assert!(
        found.contains(&("prefabs/bad.prefab.json", "prefab_without_root")),
        "{found:?}"
    );
    assert!(
        found.contains(&("extra.scene.json", "unknown_component")),
        "{found:?}"
    );
    assert!(
        found.contains(&("main.scene.json", "asset_not_found")),
        "{found:?}"
    );
    assert!(project.has_errors());
    // A file with errors has no canonical form; the others still do.
    let bad = project
        .files
        .iter()
        .find(|f| f.path == "extra.scene.json")
        .unwrap();
    assert_eq!(bad.canonical, None);
    assert!(
        project
            .files
            .iter()
            .find(|f| f.path == "main.scene.json")
            .unwrap()
            .canonical
            .is_some()
    );
}

#[test]
fn the_watcher_reports_added_changed_and_removed_files() {
    let dir = project("watcher");
    let mut watcher = Watcher::new(&dir.0).unwrap();
    assert!(watcher.poll().unwrap().is_empty());
    dir.write("new.scene.json", "{}");
    dir.write("main.scene.json", &scene(",\n    \"b\": {}"));
    fs::remove_file(dir.0.join("assets/m.glb")).unwrap();
    assert_eq!(
        watcher.poll().unwrap(),
        ["assets/m.glb", "main.scene.json", "new.scene.json"]
    );
    assert!(
        watcher.poll().unwrap().is_empty(),
        "changes are reported once"
    );
}

#[test]
fn hot_reload_swaps_the_scene_and_keeps_the_last_good_one_when_a_change_is_invalid() {
    let dir = project("hot");
    let mut world = world();
    let (mut hot, warnings) = HotScene::start(&dir.0, "main.scene.json", &mut world).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(world.entity_count(), 2);
    assert!(hot.poll(&mut world).unwrap().is_none(), "nothing changed");
    let before = world.hash();

    // A valid edit: a new entity appears.
    dir.write("main.scene.json", &scene(",\n    \"b\": {}"));
    let reload = hot.poll(&mut world).unwrap().unwrap();
    assert!(reload.applied, "{:?}", reload.diagnostics);
    assert_eq!(reload.changed, ["main.scene.json"]);
    assert_eq!(world.entity_count(), 3);
    assert_eq!(hot.entities().len(), 3);
    assert_ne!(world.hash(), before);

    // An invalid edit: rejected, the world is untouched.
    let good = world.hash();
    dir.write(
        "main.scene.json",
        &scene(",\n    \"b\": { \"components\": { \"Nope\": {} } }"),
    );
    let reload = hot.poll(&mut world).unwrap().unwrap();
    assert!(!reload.applied);
    assert!(
        reload
            .diagnostics
            .iter()
            .any(|d| d.code == "unknown_component")
    );
    assert_eq!(world.entity_count(), 3);
    assert_eq!(world.hash(), good, "the last good scene keeps running");

    // A missing asset also blocks the reload, naming the file.
    fs::remove_file(dir.0.join("assets/m.glb")).unwrap();
    dir.write("main.scene.json", &scene(",\n    \"b\": {}"));
    let reload = hot.poll(&mut world).unwrap().unwrap();
    assert!(!reload.applied);
    assert!(
        reload
            .diagnostics
            .iter()
            .any(|d| d.code == "asset_not_found")
    );

    // Fixing it applies again. The content equals a fresh load's; the world
    // hash does not, because the replaced entities advanced their slots'
    // generations (old handles must not point at the new entities).
    dir.write("assets/m.glb", "data");
    let reload = hot.poll(&mut world).unwrap().unwrap();
    assert!(reload.applied);
    let mut fresh = self::world();
    let (fresh_hot, _) = HotScene::start(&dir.0, "main.scene.json", &mut fresh).unwrap();
    assert_eq!(world.entity_count(), fresh.entity_count());
    for (id, entity) in hot.entities() {
        assert_eq!(
            world.entity_json(*entity).unwrap(),
            fresh.entity_json(fresh_hot.entities()[id]).unwrap(),
            "{id}"
        );
    }
    let model = hot.entities()["m"];
    assert_eq!(
        world.get_json(model, "Model").unwrap(),
        json!({ "mesh": "assets/m.glb" })
    );
}

#[test]
fn starting_needs_a_valid_project_and_an_existing_scene() {
    let dir = project("start");
    let mut world = world();
    let missing = HotScene::start(&dir.0, "mian.scene.json", &mut world).unwrap_err();
    assert_eq!(missing.code, "scene_not_found");
    assert_eq!(missing.hint, "Did you mean 'main.scene.json'?");

    dir.write("main.scene.json", "{");
    let broken = HotScene::start(&dir.0, "main.scene.json", &mut world).unwrap_err();
    assert_eq!(broken.code, "json_syntax");
    assert_eq!(
        world.entity_count(),
        0,
        "a failed start leaves the world empty"
    );
}
