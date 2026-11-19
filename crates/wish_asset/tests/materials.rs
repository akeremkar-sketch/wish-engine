//! Material files: reading, the canonical form, hostile text, references
//! and moves.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};
use wish_asset::{
    AssetIndex, FileKind, MaterialDocument, apply_move, is_document, is_material, load_project,
    parse_material, plan_move,
};
use wish_ecs::World;
use wish_types::render::{AlphaMode, Material, MaterialFile, Model, Primitive};

fn world() -> World {
    let mut world = World::new();
    world.register_component::<Material>().unwrap();
    world.register_component::<Model>().unwrap();
    world.register_component::<Primitive>().unwrap();
    world
}

/// A throwaway project folder.
struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("wish-mat-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.0.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.0.join(path)).unwrap()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const BRICK: &str = r#"{
  "format": 1,
  "material": {
    "alpha_mode": "Mask",
    "base_color_texture": "assets/textures/brick.png",
    "normal_texture": "assets/textures/brick_n.png",
    "roughness": 0.9,
    "uv_scale": [4, 4]
  }
}
"#;

#[test]
fn a_material_file_round_trips_byte_for_byte() {
    let file = parse_material(BRICK, "assets/materials/brick.mat.json").unwrap();
    assert_eq!(file.to_canonical().unwrap(), BRICK);
    // Reading the canonical text gives the same file again.
    assert_eq!(
        parse_material(&file.to_canonical().unwrap(), "").unwrap(),
        file
    );
    // A file that is not canonical (key order, spacing) is made canonical,
    // and that is stable. (Numbers keep the notation they were written in,
    // as in scene files.)
    let loose = r#"{"material":{"uv_scale":[4,4],"roughness":0.9,"normal_texture":"assets/textures/brick_n.png","base_color_texture":"assets/textures/brick.png","alpha_mode":"Mask"},"format":1}"#;
    assert_eq!(
        parse_material(loose, "").unwrap().to_canonical().unwrap(),
        BRICK
    );
    // The default look is just its format number.
    let empty = parse_material("{ \"format\": 1, \"material\": {} }", "").unwrap();
    assert_eq!(empty.to_canonical().unwrap(), "{\n  \"format\": 1\n}\n");
    assert_eq!(
        parse_material("{\n  \"format\": 1\n}\n", "").unwrap(),
        MaterialDocument::default()
    );
}

#[test]
fn a_material_file_decodes_to_the_component_with_its_defaults() {
    let file = parse_material(BRICK, "").unwrap();
    let (material, texts) = file.decode().unwrap();
    assert_eq!(material.alpha_mode, AlphaMode::Mask);
    assert_eq!(material.roughness, 0.9);
    assert_eq!(material.uv_scale, [4.0, 4.0]);
    // What the file leaves out is the type's default.
    let default = MaterialFile::default();
    assert_eq!(material.metallic, default.metallic);
    assert_eq!(default.roughness, Material::default().roughness);
    assert_eq!(material.alpha_cutoff, 0.5);
    assert_eq!(material.normal_scale, 1.0);
    assert_eq!(material.base_color, default.base_color);
    assert!(material.emissive_texture.is_none());
    assert_eq!(
        texts.get(material.base_color_texture.text()),
        Some("assets/textures/brick.png")
    );
    let effective = file.effective().unwrap();
    assert_eq!(effective["occlusion_strength"], json!(1.0));
    assert_eq!(effective["normal_texture"], "assets/textures/brick_n.png");
    let textures: Vec<(String, String)> = file
        .textures()
        .into_iter()
        .map(|used| (used.pointer, used.reference))
        .collect();
    assert_eq!(
        textures,
        [
            (
                "/material/base_color_texture".to_owned(),
                "assets/textures/brick.png".to_owned()
            ),
            (
                "/material/normal_texture".to_owned(),
                "assets/textures/brick_n.png".to_owned()
            ),
        ]
    );
}

#[test]
fn hostile_material_files_are_diagnostics_with_a_place_and_a_hint() {
    let cases: &[(&str, &str, Option<&str>)] = &[
        // Not JSON, or not the right shape.
        ("", "json_syntax", None),
        ("{ \"format\": 1, ", "json_syntax", None),
        ("[1, 2]", "type_mismatch", None),
        ("{ \"material\": {} }", "missing_format", None),
        ("{ \"format\": 2 }", "unsupported_format", None),
        ("{ \"format\": \"1\" }", "unsupported_format", None),
        ("{ \"format\": 1, \"material\": [] }", "type_mismatch", None),
        (
            "{ \"format\": 1, \"shader\": \"x.wgsl\" }",
            "unknown_key",
            None,
        ),
        // A field at the top level instead of inside "material".
        ("{ \"format\": 1, \"roughness\": 1 }", "unknown_key", None),
        // Fields that do not exist, wrong types, out of range.
        (
            "{ \"format\": 1, \"material\": { \"colour\": [1, 0, 0, 1] } }",
            "unknown_field",
            Some("/material/colour"),
        ),
        (
            "{ \"format\": 1, \"material\": { \"roughness\": \"rough\" } }",
            "type_mismatch",
            Some("/material/roughness"),
        ),
        (
            "{ \"format\": 1, \"material\": { \"roughness\": 7 } }",
            "value_out_of_range",
            Some("/material/roughness"),
        ),
        (
            "{ \"format\": 1, \"material\": { \"alpha_cutoff\": -1 } }",
            "value_out_of_range",
            Some("/material/alpha_cutoff"),
        ),
        (
            "{ \"format\": 1, \"material\": { \"uv_scale\": [1] } }",
            "wrong_length",
            Some("/material/uv_scale"),
        ),
        (
            "{ \"format\": 1, \"material\": { \"alpha_mode\": \"Glass\" } }",
            "unknown_variant",
            Some("/material/alpha_mode"),
        ),
        (
            "{ \"format\": 1, \"material\": { \"double_sided\": 1 } }",
            "type_mismatch",
            Some("/material/double_sided"),
        ),
        // Numbers JSON or an f32 cannot hold.
        (
            "{ \"format\": 1, \"material\": { \"metallic\": NaN } }",
            "json_syntax",
            None,
        ),
        (
            "{ \"format\": 1, \"material\": { \"uv_offset\": [1e999, 0] } }",
            "json_number_out_of_range",
            None,
        ),
        (
            "{ \"format\": 1, \"material\": { \"uv_offset\": [1e300, 0] } }",
            "value_out_of_range",
            Some("/material/uv_offset/0"),
        ),
        // References that leave the project.
        (
            "{ \"format\": 1, \"material\": { \"base_color_texture\": \"../secret.png\" } }",
            "invalid_asset_ref",
            Some("/material/base_color_texture"),
        ),
        (
            "{ \"format\": 1, \"material\": { \"normal_texture\": \"C:/Windows/x.png\" } }",
            "invalid_asset_ref",
            Some("/material/normal_texture"),
        ),
        (
            "{ \"format\": 1, \"material\": { \"emissive_texture\": 5 } }",
            "type_mismatch",
            Some("/material/emissive_texture"),
        ),
        // A material file cannot name another one (so no circles): it has
        // no such field.
        (
            "{ \"format\": 1, \"material\": { \"file\": \"assets/other.mat.json\" } }",
            "unknown_field",
            Some("/material/file"),
        ),
    ];
    for (source, code, pointer) in cases {
        let problem = parse_material(source, "assets/m.mat.json").unwrap_err();
        assert_eq!(problem.code, *code, "{source}: {}", problem.message);
        assert!(!problem.hint.is_empty(), "{source}");
        assert_eq!(
            problem.location.file.as_deref(),
            Some("assets/m.mat.json"),
            "{source}"
        );
        assert!(problem.location.line.is_some(), "{source}: a line");
        if let Some(pointer) = pointer {
            assert_eq!(problem.location.path.as_deref(), Some(*pointer), "{source}");
        }
    }
    // A very deep or very long file is still just a diagnostic.
    let deep = format!("{}1{}", "[".repeat(100_000), "]".repeat(100_000));
    assert!(parse_material(&deep, "m.mat.json").is_err());
    let long = format!(
        "{{ \"format\": 1, \"material\": {{ \"base_color_texture\": \"{}\" }} }}",
        "a".repeat(100_000)
    );
    assert_eq!(
        parse_material(&long, "m.mat.json").unwrap_err().code,
        "invalid_asset_ref"
    );
}

const SCENE: &str = r#"{
  "entities": {
    "floor": {
      "components": {
        "Material": {
          "roughness": 1
        },
        "Primitive": {
          "shape": "Plane"
        }
      }
    },
    "wall": {
      "components": {
        "Material": {
          "file": "assets/materials/brick.mat.json"
        },
        "Primitive": {}
      }
    }
  },
  "format": 1
}
"#;

fn brick_project(name: &str) -> Project {
    let project = Project::new(name);
    project.write("assets/textures/brick.png", "png");
    project.write("assets/textures/brick_n.png", "png");
    project.write("assets/materials/brick.mat.json", BRICK);
    project.write("scenes/main.scene.json", SCENE);
    project
}

#[test]
fn a_material_file_is_a_document_and_an_asset() {
    assert!(is_material("assets/materials/brick.mat.json"));
    assert!(is_document("assets/materials/brick.mat.json"));
    assert!(!is_material("assets/materials/brick.json") && !is_material("brick.mat.json.bak"));
    let project = brick_project("index");
    let index = AssetIndex::scan(&project.0).unwrap();
    assert!(index.contains("assets/materials/brick.mat.json"));
    assert!(!index.contains("scenes/main.scene.json"));
    let documents: Vec<String> = wish_asset::index::documents(&project.0)
        .unwrap()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        documents,
        ["assets/materials/brick.mat.json", "scenes/main.scene.json"]
    );
    // The project loads clean, with the material among its files in
    // canonical form.
    let world = world();
    let loaded = load_project(&project.0, world.registry()).unwrap();
    assert_eq!(loaded.diagnostics, Vec::new());
    let material = loaded
        .files
        .iter()
        .find(|file| file.kind == FileKind::Material)
        .unwrap();
    assert_eq!(material.path, "assets/materials/brick.mat.json");
    assert_eq!(material.canonical.as_deref(), Some(BRICK));
    assert_eq!(material.source, BRICK);
}

fn codes_at(project: &Project) -> Vec<(String, String, Option<u32>)> {
    let world = world();
    load_project(&project.0, world.registry())
        .unwrap()
        .diagnostics
        .iter()
        .map(|d| {
            assert!(!d.hint.is_empty(), "{}", d.code);
            (
                d.location.file.clone().unwrap_or_default(),
                d.code.to_string(),
                d.location.line,
            )
        })
        .collect()
}

#[test]
fn the_project_check_finds_what_is_wrong_around_materials() {
    // A texture the material names is missing; one is not a PNG.
    let project = brick_project("missing");
    fs::remove_file(project.0.join("assets/textures/brick_n.png")).unwrap();
    project.write("assets/textures/photo.jpg", "jpg");
    project.write(
        "assets/materials/photo.mat.json",
        "{\n  \"format\": 1,\n  \"material\": {\n    \"base_color_texture\": \"assets/textures/photo.jpg\"\n  }\n}\n",
    );
    assert_eq!(
        codes_at(&project),
        [
            (
                "assets/materials/brick.mat.json".to_owned(),
                "asset_not_found".to_owned(),
                Some(6)
            ),
            (
                "assets/materials/photo.mat.json".to_owned(),
                "texture_not_png".to_owned(),
                Some(4)
            ),
        ]
    );

    // A scene names a material file that is not there, and one that is not
    // a material file.
    let project = brick_project("scene");
    project.write(
        "scenes/main.scene.json",
        &SCENE
            .replace(
                "assets/materials/brick.mat.json",
                "assets/materials/stone.mat.json",
            )
            .replace(
                "\"roughness\": 1",
                "\"file\": \"assets/textures/brick.png\"",
            ),
    );
    let found = codes_at(&project);
    assert_eq!(
        found
            .iter()
            .map(|(file, code, _)| (file.as_str(), code.as_str()))
            .collect::<Vec<_>>(),
        [
            ("scenes/main.scene.json", "not_a_material_file"),
            ("scenes/main.scene.json", "asset_not_found"),
        ]
    );
    assert!(found.iter().all(|(_, _, line)| line.is_some()));

    // A broken material file is an error of the project, with its line; an
    // uncanonical one a warning.
    let project = brick_project("broken");
    project.write(
        "assets/materials/brick.mat.json",
        "{\n  \"format\": 1,\n  \"material\": {\n    \"roughness\": 9\n  }\n}\n",
    );
    project.write("assets/materials/loose.mat.json", "{\"format\":1}");
    assert_eq!(
        codes_at(&project),
        [
            (
                "assets/materials/brick.mat.json".to_owned(),
                "value_out_of_range".to_owned(),
                Some(4)
            ),
            (
                "assets/materials/loose.mat.json".to_owned(),
                "not_canonical".to_owned(),
                Some(1)
            ),
        ]
    );
    let world = world();
    let loaded = load_project(&project.0, world.registry()).unwrap();
    assert!(loaded.has_errors());
    let broken = &loaded.files[0];
    assert_eq!(broken.kind, FileKind::Material);
    assert!(broken.canonical.is_none(), "nothing to format");
    assert_eq!(
        loaded.files[1].canonical.as_deref(),
        Some("{\n  \"format\": 1\n}\n")
    );

    // A file far too large to be a material is not read.
    let project = brick_project("huge");
    project.write(
        "assets/materials/huge.mat.json",
        &format!("{{ \"format\": 1, \"x\": \"{}\" }}", "a".repeat(300_000)),
    );
    assert_eq!(
        codes_at(&project),
        [(
            "assets/materials/huge.mat.json".to_owned(),
            "material_file_too_large".to_owned(),
            None
        )]
    );
}

#[test]
fn a_material_that_says_what_is_not_used_is_a_warning() {
    let project = brick_project("notes");
    project.write("assets/models/crate.glb", "glb");
    project.write(
        "scenes/main.scene.json",
        r#"{
  "format": 1,
  "entities": {
    "both": { "components": { "Primitive": {},
      "Material": { "file": "assets/materials/brick.mat.json", "base_color": [1, 0, 0, 1], "roughness": 1 } } },
    "crate": { "components": { "Model": { "asset": "assets/models/crate.glb" },
      "Material": { "base_color": [1, 0, 0, 1] } } },
    "faded": { "components": { "Primitive": {}, "Material": { "base_color": [1, 0, 0, 0.5] } } },
    "ghost": { "components": { "Primitive": {}, "Material": { "base_color": [1, 0, 0, 0] } } },
    "skinned": { "components": { "Model": { "asset": "assets/models/crate.glb" },
      "Material": { "file": "assets/materials/brick.mat.json" } } }
  }
}"#,
    );
    let world = world();
    let loaded = load_project(&project.0, world.registry()).unwrap();
    let notes: Vec<(&str, Option<&str>, bool)> = loaded
        .diagnostics
        .iter()
        .filter(|d| d.code != "not_canonical")
        .map(|d| (d.code.as_ref(), d.location.path.as_deref(), d.is_error()))
        .collect();
    assert_eq!(
        notes,
        [
            (
                "material_fields_unused",
                Some("/entities/both/components/Material"),
                false
            ),
            (
                "material_unused_on_model",
                Some("/entities/crate/components/Material"),
                false
            ),
            // Alpha 0 hides the whole entity; 0.5 is a see-through one.
            (
                "material_invisible",
                Some("/entities/ghost/components/Material/base_color/3"),
                false
            ),
        ]
    );
    let unused = &loaded
        .diagnostics
        .iter()
        .find(|d| d.code == "material_fields_unused");
    assert!(unused.unwrap().message.contains("base_color, roughness"));
    assert!(!loaded.has_errors());
}

#[test]
fn moving_a_texture_rewrites_the_material_files_that_name_it() {
    let project = brick_project("move-texture");
    let world = world();
    let plan = plan_move(
        &project.0,
        "assets/textures/brick.png",
        "assets/pictures/wall.png",
        world.registry(),
    )
    .unwrap();
    let edits: Vec<(&str, usize)> = plan
        .documents
        .iter()
        .map(|edit| (edit.file.as_str(), edit.changes))
        .collect();
    assert_eq!(edits, [("assets/materials/brick.mat.json", 1)]);
    apply_move(&project.0, &plan).unwrap();
    assert_eq!(
        project.read("assets/materials/brick.mat.json"),
        BRICK.replace("assets/textures/brick.png", "assets/pictures/wall.png")
    );
    assert_eq!(project.read("scenes/main.scene.json"), SCENE, "untouched");
    assert_eq!(codes_at(&project), Vec::new());
}

#[test]
fn moving_a_material_file_rewrites_the_scenes_that_use_it() {
    let project = brick_project("move-material");
    let world = world();
    let plan = plan_move(
        &project.0,
        "assets/materials/brick.mat.json",
        "assets/looks/wall.mat.json",
        world.registry(),
    )
    .unwrap();
    assert_eq!(plan.documents.len(), 1);
    assert_eq!(plan.documents[0].file, "scenes/main.scene.json");
    apply_move(&project.0, &plan).unwrap();
    assert_eq!(project.read("assets/looks/wall.mat.json"), BRICK);
    assert_eq!(
        project.read("scenes/main.scene.json"),
        SCENE.replace(
            "assets/materials/brick.mat.json",
            "assets/looks/wall.mat.json"
        )
    );
    assert_eq!(codes_at(&project), Vec::new());

    // A whole folder with the material and its textures: the material is
    // rewritten in its new place.
    let plan = plan_move(&project.0, "assets", "content", world.registry()).unwrap();
    let edits: Vec<(&str, &str, usize)> = plan
        .documents
        .iter()
        .map(|edit| (edit.file.as_str(), edit.target.as_str(), edit.changes))
        .collect();
    assert_eq!(
        edits,
        [
            (
                "assets/looks/wall.mat.json",
                "content/looks/wall.mat.json",
                2
            ),
            ("scenes/main.scene.json", "scenes/main.scene.json", 1),
        ]
    );
    apply_move(&project.0, &plan).unwrap();
    let moved: Value = serde_json::from_str(&project.read("content/looks/wall.mat.json")).unwrap();
    assert_eq!(
        moved["material"]["normal_texture"],
        "content/textures/brick_n.png"
    );
    assert_eq!(codes_at(&project), Vec::new());
}

#[test]
fn a_material_file_keeps_its_ending_when_it_moves() {
    let project = brick_project("kind");
    let world = world();
    for to in [
        "assets/materials/brick.png",
        "assets/materials/brick.json",
        "scenes/brick.scene.json",
    ] {
        let problem = plan_move(
            &project.0,
            "assets/materials/brick.mat.json",
            to,
            world.registry(),
        )
        .unwrap_err();
        assert_eq!(problem.code, "kind_change", "{to}");
    }
    let problem = plan_move(
        &project.0,
        "assets/textures/brick.png",
        "assets/textures/brick.mat.json",
        world.registry(),
    )
    .unwrap_err();
    assert_eq!(problem.code, "kind_change");
    assert!(problem.hint.contains(".mat.json"));
}

#[test]
fn tiling_the_renderer_cannot_use_as_written_is_a_warning() {
    let source = "{
  \"format\": 1,
  \"material\": {
    \"uv_offset\": [5000, 0.5],
    \"uv_scale\": [0, 4]
  }
}
";
    let (file, problems) =
        wish_asset::check_material(source, "m.mat.json", &wish_asset::AssetIndex::default());
    assert!(file.is_some(), "a warning, not an error");
    let found: Vec<(&str, Option<&str>, Option<u32>)> = problems
        .iter()
        .map(|d| (d.code.as_ref(), d.location.path.as_deref(), d.location.line))
        .collect();
    assert_eq!(
        found,
        [
            (
                "material_uv_unusable",
                Some("/material/uv_scale/0"),
                Some(5)
            ),
            (
                "material_uv_unusable",
                Some("/material/uv_offset/0"),
                Some(4)
            ),
        ]
    );
    assert!(problems.iter().all(|d| !d.is_error() && !d.hint.is_empty()));
    // Usable numbers, negative ones (a mirror) among them, say nothing.
    let fine = "{\n  \"format\": 1,\n  \"material\": {\n    \"uv_scale\": [-2, 4096]\n  }\n}\n";
    let (_, problems) =
        wish_asset::check_material(fine, "m.mat.json", &wish_asset::AssetIndex::default());
    assert!(problems.is_empty(), "{problems:?}");
}
