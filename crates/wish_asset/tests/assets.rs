//! Asset discovery, meta files, reference checking and moves.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use wish_asset::{
    AssetIndex, META_FORMAT, apply_move, check_source, parse_meta, plan_move, read_meta,
    refs_in_value, write_meta,
};
use wish_core::AssetRef;
use wish_core::math::Vec3;
use wish_ecs::World;
use wish_schema::{component, json_schema, resource};

#[component]
pub struct Model {
    pub mesh: AssetRef,
    pub textures: [AssetRef; 2],
}

#[component]
pub struct Transform {
    pub translation: Vec3,
}

#[resource]
pub struct Sky {
    pub texture: AssetRef,
}

fn world() -> World {
    let mut world = World::new();
    world.register_component::<Model>().unwrap();
    world.register_component::<Transform>().unwrap();
    world.register_resource::<Sky>().unwrap();
    world
}

const SCENE: &str = r#"{
  "entities": {
    "crate": {
      "components": {
        "Model": { "mesh": "assets/models/crate.glb#Mesh0", "textures": ["assets/textures/wood.png", ""] },
        "Transform": { "translation": [1, 2, 3] }
      }
    },
    "enemy": {
      "overrides": { "weapon": { "components": { "Model": { "mesh": "assets/models/crate.glb" } } } },
      "prefab": "prefabs/enemy.prefab.json"
    }
  },
  "format": 1,
  "resources": { "Sky": { "texture": "assets/textures/sky.png" } }
}
"#;

const PREFAB: &str = r#"{
  "entities": {
    "root": { "components": { "Model": { "mesh": "assets/models/enemy.glb" } } },
    "weapon": { "components": { "Model": { "textures": ["assets/textures/wood.png", "assets/textures/steel.png"] } }, "parent": "root" }
  },
  "format": 1
}
"#;

/// A throwaway project folder.
struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("wish-asset-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for file in [
            "assets/models/crate.glb",
            "assets/models/enemy.glb",
            "assets/textures/wood.png",
            "assets/textures/steel.png",
            "assets/textures/sky.png",
            ".hidden/secret.png",
        ] {
            Self::write_at(&root, file, "data");
        }
        Self::write_at(
            &root,
            "assets/textures/wood.png.meta.json",
            "{ \"format\": 1 }",
        );
        Self::write_at(&root, "scenes/main.scene.json", SCENE);
        Self::write_at(&root, "prefabs/enemy.prefab.json", PREFAB);
        Self(root)
    }

    fn write_at(root: &Path, file: &str, text: &str) {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(&self, file: &str) -> String {
        fs::read_to_string(self.0.join(file)).unwrap()
    }

    fn exists(&self, file: &str) -> bool {
        self.0.join(file).exists()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_schema_knows_asset_references_and_checks_their_form() {
    let mut world = world();
    let entity = world.spawn().unwrap();
    world
        .set_json(
            entity,
            "Model",
            &json!({ "mesh": "assets/a.glb#Mesh0", "textures": ["b.png", ""] }),
        )
        .unwrap();
    assert_eq!(
        world.get_json(entity, "Model").unwrap(),
        json!({ "mesh": "assets/a.glb#Mesh0", "textures": ["b.png", ""] })
    );
    for bad in ["/abs.png", "a/../b.png", "a\\b.png", "a.glb#"] {
        let problem = world
            .set_json(entity, "Model", &json!({ "mesh": bad }))
            .unwrap_err();
        assert_eq!(problem.code, "invalid_asset_ref", "{bad}");
        assert_eq!(problem.location.path.as_deref(), Some("/mesh"), "{bad}");
        assert!(problem.hint.contains("relative"), "{bad}: {}", problem.hint);
    }
    // The value did not change.
    assert_eq!(
        world.get_json(entity, "Model").unwrap()["mesh"],
        "assets/a.glb#Mesh0"
    );
    let info = world
        .registry()
        .component(world.component_id("Model").unwrap())
        .unwrap();
    let schema = json_schema(info.schema());
    assert_eq!(schema["properties"]["mesh"]["x-asset"], true);
    assert_eq!(schema["properties"]["textures"]["items"]["x-asset"], true);
}

#[test]
fn the_index_lists_assets_and_checks_references() {
    let project = Project::new("index");
    let index = AssetIndex::scan(&project.0).unwrap();
    assert_eq!(
        index.len(),
        5,
        "documents and hidden folders are not assets"
    );
    assert!(index.contains("assets/models/crate.glb"));
    assert!(!index.contains(".hidden/secret.png"));
    assert!(!index.contains("assets/textures/wood.png.meta.json"));

    assert!(index.check("").is_ok());
    assert!(index.check("assets/models/crate.glb").is_ok());
    assert!(index.check("assets/models/crate.glb#Mesh0").is_ok());
    let missing = index.check("assets/models/crat.glb").unwrap_err();
    assert_eq!(missing.code, "asset_not_found");
    assert_eq!(missing.hint, "Did you mean 'assets/models/crate.glb'?");
    assert_eq!(
        index.check("../x.png").unwrap_err().code,
        "invalid_asset_ref"
    );
}

#[test]
fn meta_files_round_trip_and_report_mistakes() {
    let project = Project::new("meta");
    assert_eq!(
        read_meta(&project.0, "assets/models/crate.glb").unwrap(),
        None
    );
    let meta = read_meta(&project.0, "assets/textures/wood.png")
        .unwrap()
        .unwrap();
    assert!(meta.import.is_empty());

    let mut meta = meta;
    meta.import
        .insert("generate_mipmaps".to_owned(), json!(true));
    write_meta(&project.0, "assets/textures/wood.png", &meta).unwrap();
    assert_eq!(
        project.read("assets/textures/wood.png.meta.json"),
        "{\n  \"format\": 1,\n  \"import\": {\n    \"generate_mipmaps\": true\n  }\n}\n"
    );
    assert_eq!(
        read_meta(&project.0, "assets/textures/wood.png")
            .unwrap()
            .unwrap(),
        meta
    );
    assert_eq!(META_FORMAT, 1);

    let cases = [
        ("{}", "missing_format", None),
        ("{ \"format\": 2 }", "unsupported_format", Some((1, 13))),
        (
            "{ \"format\": 1, \"imprt\": {} }",
            "unknown_key",
            Some((1, 16)),
        ),
        (
            "{ \"format\": 1, \"import\": 3 }",
            "type_mismatch",
            Some((1, 26)),
        ),
        ("[]", "type_mismatch", Some((1, 1))),
    ];
    for (source, code, position) in cases {
        let problem = parse_meta(source, "x.meta.json").unwrap_err();
        assert_eq!(problem.code, code, "{source}");
        assert!(!problem.hint.is_empty(), "{source}");
        if let Some((line, column)) = position {
            assert_eq!(
                (problem.location.line, problem.location.column),
                (Some(line), Some(column)),
                "{source}"
            );
        }
    }
    assert_eq!(
        parse_meta("{ \"format\": 1, \"imprt\": {} }", "")
            .unwrap_err()
            .hint,
        "Did you mean 'import'?"
    );
}

#[test]
fn references_are_found_by_schema_in_components_overrides_and_resources() {
    let world = world();
    let document: serde_json::Value = serde_json::from_str(SCENE).unwrap();
    let found = refs_in_value(world.registry(), &document);
    let listed: Vec<(&str, &str)> = found
        .iter()
        .map(|used| (used.pointer.as_str(), used.reference.as_str()))
        .collect();
    assert_eq!(
        listed,
        [
            (
                "/entities/crate/components/Model/mesh",
                "assets/models/crate.glb#Mesh0"
            ),
            (
                "/entities/crate/components/Model/textures/0",
                "assets/textures/wood.png"
            ),
            (
                "/entities/enemy/overrides/weapon/components/Model/mesh",
                "assets/models/crate.glb"
            ),
            ("/resources/Sky/texture", "assets/textures/sky.png"),
        ],
        "the empty texture is not a reference, strings elsewhere are ignored"
    );
}

#[test]
fn missing_files_are_reported_where_they_are_written() {
    let project = Project::new("check");
    let world = world();
    let index = AssetIndex::scan(&project.0).unwrap();
    assert!(check_source(SCENE, "scenes/main.scene.json", world.registry(), &index).is_empty());

    fs::remove_file(project.0.join("assets/models/crate.glb")).unwrap();
    let problems = check_source(SCENE, "scenes/main.scene.json", world.registry(), &index);
    assert!(problems.is_empty(), "the old index still has it");
    let index = AssetIndex::scan(&project.0).unwrap();
    let problems = check_source(SCENE, "scenes/main.scene.json", world.registry(), &index);
    assert_eq!(problems.len(), 2);
    assert!(problems.iter().all(|p| p.code == "asset_not_found"));
    let first = &problems[0];
    assert_eq!(
        first.location.path.as_deref(),
        Some("/entities/crate/components/Model/mesh")
    );
    assert_eq!(
        first.location.file.as_deref(),
        Some("scenes/main.scene.json")
    );
    // Line 5 holds the mesh; the position is the opening quote of its value.
    assert_eq!(first.location.line, Some(5));
    assert_eq!(first.location.column, Some(28));

    let syntax = check_source("{", "x.scene.json", world.registry(), &index);
    assert_eq!(syntax.len(), 1);
    assert_eq!(syntax[0].code, "json_syntax");
}

#[test]
fn moving_a_file_updates_every_reference_and_the_meta_file() {
    let project = Project::new("move-file");
    let world = world();
    let plan = plan_move(
        &project.0,
        "assets/textures/wood.png",
        "assets/materials/oak.png",
        world.registry(),
    )
    .unwrap();
    assert_eq!(
        plan.meta,
        Some((
            "assets/textures/wood.png.meta.json".to_owned(),
            "assets/materials/oak.png.meta.json".to_owned()
        ))
    );
    let summary: Vec<(&str, usize)> = plan
        .documents
        .iter()
        .map(|edit| (edit.file.as_str(), edit.changes))
        .collect();
    assert_eq!(
        summary,
        [
            ("prefabs/enemy.prefab.json", 1),
            ("scenes/main.scene.json", 1)
        ]
    );
    // Planning changes nothing.
    assert!(project.exists("assets/textures/wood.png"));
    assert_eq!(project.read("scenes/main.scene.json"), SCENE);

    apply_move(&project.0, &plan).unwrap();
    assert!(!project.exists("assets/textures/wood.png"));
    assert!(project.exists("assets/materials/oak.png"));
    assert!(project.exists("assets/materials/oak.png.meta.json"));
    let scene = project.read("scenes/main.scene.json");
    assert!(scene.contains("\"assets/materials/oak.png\""));
    assert!(!scene.contains("wood.png"));
    assert!(
        project
            .read("prefabs/enemy.prefab.json")
            .contains("assets/materials/oak.png")
    );
    // Untouched references stay, and the files are canonical.
    assert!(scene.contains("assets/models/crate.glb#Mesh0"));
    // Only the one reference changed; the file is rewritten canonically.
    let expected: serde_json::Value = serde_json::from_str(
        &SCENE.replace("assets/textures/wood.png", "assets/materials/oak.png"),
    )
    .unwrap();
    assert_eq!(scene, wish_scene::json::to_canonical(&expected).unwrap());

    // After the move, every reference resolves.
    let index = AssetIndex::scan(&project.0).unwrap();
    assert!(check_source(&scene, "s", world.registry(), &index).is_empty());
}

#[test]
fn a_failed_move_puts_everything_back() {
    let project = Project::new("move-rollback");
    let world = world();
    let plan = plan_move(
        &project.0,
        "assets/textures/wood.png",
        "assets/materials/oak.png",
        world.registry(),
    )
    .unwrap();
    // The prefab comes first in path order; make the scene unwritable so the
    // move fails after the prefab and the files have already changed.
    let scene_path = project.0.join("scenes/main.scene.json");
    let mut permissions = fs::metadata(&scene_path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&scene_path, permissions).unwrap();

    let problem = apply_move(&project.0, &plan).unwrap_err();
    assert_eq!(problem.code, "file_unwritable");
    assert!(problem.message.contains("scenes/main.scene.json"));

    let mut permissions = fs::metadata(&scene_path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&scene_path, permissions).unwrap();
    assert!(project.exists("assets/textures/wood.png"));
    assert!(project.exists("assets/textures/wood.png.meta.json"));
    assert!(!project.exists("assets/materials/oak.png"));
    assert_eq!(project.read("scenes/main.scene.json"), SCENE);
    assert_eq!(project.read("prefabs/enemy.prefab.json"), PREFAB);
}

#[test]
fn moving_a_folder_keeps_the_sub_objects() {
    let project = Project::new("move-folder");
    let world = world();
    let plan = plan_move(
        &project.0,
        "assets/models",
        "assets/meshes",
        world.registry(),
    )
    .unwrap();
    assert_eq!(plan.meta, None);
    apply_move(&project.0, &plan).unwrap();
    assert!(project.exists("assets/meshes/crate.glb"));
    assert!(!project.exists("assets/models"));
    let scene = project.read("scenes/main.scene.json");
    assert!(scene.contains("\"assets/meshes/crate.glb#Mesh0\""));
    assert!(
        project
            .read("prefabs/enemy.prefab.json")
            .contains("assets/meshes/enemy.glb")
    );
    // A folder with a similar prefix is not touched.
    let other = plan_move(&project.0, "assets/mesh", "assets/m", world.registry());
    assert_eq!(other.unwrap_err().code, "asset_not_found");
}

#[test]
fn bad_moves_are_refused_without_changing_anything() {
    let project = Project::new("move-bad");
    let world = world();
    let registry = world.registry();
    let cases = [
        ("assets/models/crat.glb", "assets/x.glb", "asset_not_found"),
        (
            "assets/models/crate.glb",
            "assets/models/enemy.glb",
            "asset_exists",
        ),
        ("assets/models", "assets/models/inner", "move_into_itself"),
        (
            "assets/models/crate.glb",
            "assets/models/crate.glb",
            "nothing_to_move",
        ),
        (
            "assets/models/crate.glb",
            "../outside.glb",
            "invalid_asset_ref",
        ),
        (
            "assets/models/crate.glb",
            "a.glb#Mesh0",
            "invalid_asset_ref",
        ),
    ];
    for (from, to, code) in cases {
        let problem = plan_move(&project.0, from, to, registry).unwrap_err();
        assert_eq!(problem.code, code, "{from} -> {to}");
        assert!(!problem.hint.is_empty(), "{from} -> {to}");
    }
    assert_eq!(
        plan_move(&project.0, "assets/models/crat.glb", "x.glb", registry)
            .unwrap_err()
            .hint,
        "Did you mean 'assets/models/crate.glb'?"
    );
    assert_eq!(project.read("scenes/main.scene.json"), SCENE);

    // A scene with a syntax error stops the plan before anything moves.
    Project::write_at(&project.0, "scenes/broken.scene.json", "{ \"format\": ");
    let problem = plan_move(
        &project.0,
        "assets/models/crate.glb",
        "assets/c.glb",
        registry,
    )
    .unwrap_err();
    assert_eq!(problem.code, "json_syntax");
    assert!(project.exists("assets/models/crate.glb"));
}
