//! From a scene file to a render scene: load, instantiate, propagate
//! transforms, extract.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_core::math::Vec3;
use wish_ecs::World;
use wish_render::{Mesh, RenderScene, Shape, extract};
use wish_scene::{SceneDocument, load};
use wish_transform::update_global_transforms;

fn world() -> World {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    world
}

fn scene(source: &str, world: &World) -> SceneDocument {
    let result = load(source, "scenes/test.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap()
}

fn extracted(source: &str) -> (World, RenderScene, wish_scene::SceneEntities) {
    let mut world = world();
    let entities = scene(source, &world).instantiate(&mut world).unwrap();
    update_global_transforms(&mut world).unwrap();
    let scene = extract(&world).unwrap();
    (world, scene, entities)
}

const SCENE: &str = r#"{
  "format": 1,
  "entities": {
    "camera": {
      "components": {
        "Camera": { "fov_y_deg": 50 },
        "Transform": { "translation": [0, 2, 8] }
      }
    },
    "cube": {
      "components": {
        "Material": { "base_color": [1, 0, 0, 1], "metallic": 1, "roughness": 0.25 },
        "Primitive": { "shape": "Cube" },
        "Transform": { "scale": [2, 2, 2], "translation": [1, 0, 0] }
      }
    },
    "ground": {
      "components": {
        "Primitive": { "shape": "Plane" },
        "Transform": { "scale": [20, 1, 20] }
      }
    },
    "hat": {
      "components": {
        "Model": { "asset": "assets/models/hat.glb" },
        "Transform": { "translation": [0, 1, 0] }
      },
      "parent": "cube"
    },
    "sun": {
      "components": {
        "DirectionalLight": { "illuminance_lux": 80000, "shadows": false },
        "Transform": { "rotation": [-0.7071068, 0, 0, 0.7071068] }
      }
    }
  },
  "resources": {
    "Ambient": { "intensity": 0.5 },
    "Exposure": { "ev100": 12 },
    "RenderSettings": { "shadow_map_size": 1024 },
    "Sky": { "zenith": [0.1, 0.2, 0.9, 1] }
  }
}
"#;

#[test]
fn a_scene_file_becomes_a_render_scene() {
    let (world, scene, entities) = extracted(SCENE);

    // One camera, placed where the Transform put it.
    assert_eq!(scene.cameras.len(), 1);
    let camera = scene.cameras[0];
    assert_eq!(camera.entity, entities["camera"]);
    assert_eq!(camera.position, Vec3::new(0.0, 2.0, 8.0));
    assert!((camera.fov_y - 50f32.to_radians()).abs() < 1e-6);
    // The view matrix takes the camera position to the origin.
    let at_origin = camera.view.transform_point3(camera.position);
    assert!(at_origin.length() < 1e-5, "{at_origin:?}");

    // One light, shining along -Z of its entity: a quarter turn about -X
    // lays -Z down onto -Y, so the light points straight down.
    assert_eq!(scene.lights.len(), 1);
    let light = scene.lights[0];
    assert!(
        (light.direction - Vec3::NEG_Y).length() < 1e-5,
        "{:?}",
        light.direction
    );
    assert_eq!(light.radiance, [80000.0; 3]);
    assert!(!light.shadows);

    // Draws come out in entity order, with their material and placement.
    let order: Vec<_> = scene.draws.iter().map(|draw| draw.entity).collect();
    let mut expected = vec![entities["cube"], entities["ground"], entities["hat"]];
    expected.sort_by_key(|entity| world.entities().position(|e| e == *entity));
    assert_eq!(order, expected);
    let cube = scene
        .draws
        .iter()
        .find(|d| d.entity == entities["cube"])
        .unwrap();
    assert_eq!(cube.mesh, Mesh::Primitive(Shape::Cube));
    assert_eq!(cube.material.base_color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(
        (cube.material.metallic, cube.material.roughness),
        (1.0, 0.25)
    );
    assert_eq!(
        cube.world.transform_point3(Vec3::ONE * 0.5),
        Vec3::new(2.0, 1.0, 1.0)
    );
    // A child follows its parent: the hat sits 1 m above the cube's centre
    // in the cube's scaled space, so 2 m up in the world.
    let hat = scene
        .draws
        .iter()
        .find(|d| d.entity == entities["hat"])
        .unwrap();
    assert_eq!(hat.mesh, Mesh::Model);
    assert_eq!(hat.model_path.as_deref(), Some("assets/models/hat.glb"));
    assert_eq!(hat.world.w_axis.truncate(), Vec3::new(1.0, 2.0, 0.0));
    // No material means the default one.
    let ground = scene
        .draws
        .iter()
        .find(|d| d.entity == entities["ground"])
        .unwrap();
    assert_eq!(ground.material, wish_render::MaterialData::default());

    // Resources: set ones apply, the rest are defaults.
    assert_eq!(scene.ev100, 12.0);
    // A field that is left out keeps its default.
    assert_eq!(scene.settings.shadow_map_size, 1024);
    assert!(scene.settings.shadows);
    assert!(
        (scene.ambient[2] - 0.8 * 0.5).abs() < 1e-6,
        "{:?}",
        scene.ambient
    );
    assert_eq!(scene.sky.zenith, [0.1, 0.2, 0.9]);
    assert_eq!(scene.sky.horizon, [0.65, 0.75, 0.85]);
}

#[test]
fn extraction_is_read_only_and_repeatable() {
    let mut world = world();
    scene(SCENE, &world).instantiate(&mut world).unwrap();
    update_global_transforms(&mut world).unwrap();
    let before = world.hash();
    let first = extract(&world).unwrap();
    let second = extract(&world).unwrap();
    assert_eq!(first, second);
    assert_eq!(world.hash(), before, "drawing never changes the simulation");
}

#[test]
fn an_empty_world_gives_an_empty_scene_with_the_default_sky_and_exposure() {
    let world = world();
    let scene = extract(&world).unwrap();
    assert!(scene.cameras.is_empty() && scene.lights.is_empty() && scene.draws.is_empty());
    assert_eq!(scene.ev100, 14.0);
    // Without the resource the picture is drawn as it always was: shadows
    // on, in a map of 2048 texels.
    assert!(scene.settings.shadows);
    assert_eq!(
        scene.settings.shadow_map_size,
        wish_render::renderer::SHADOW_MAP_SIZE
    );
    assert_eq!(wish_render::renderer::SHADOW_MAP_SIZE, 2048);
    assert!((scene.ambient[0] - 0.5 * 20000.0).abs() < 1e-2);
    assert_eq!(scene.sky.luminance, 20000.0);
}

#[test]
fn an_entity_with_an_empty_model_path_is_not_drawn() {
    let (_, scene, _) =
        extracted(r#"{ "format": 1, "entities": { "m": { "components": { "Model": {} } } } }"#);
    assert!(scene.draws.is_empty());
}

#[test]
fn bad_cameras_are_explained() {
    let mut world = world();
    let source = r#"{ "format": 1, "entities": { "c": { "components": { "Camera": { "near": 5, "far": 2 } } } } }"#;
    scene(source, &world).instantiate(&mut world).unwrap();
    update_global_transforms(&mut world).unwrap();
    let problem = extract(&world).unwrap_err();
    assert_eq!(problem.code, "invalid_camera");
    assert!(problem.message.contains("far"), "{}", problem.message);
    assert!(problem.hint.contains("near"), "{}", problem.hint);
}

#[test]
fn component_values_are_checked_when_the_scene_loads() {
    let world = world();
    let cases = [
        (
            r#"{ "Camera": { "fov_y_deg": 200 } }"#,
            "value_out_of_range",
            "fov_y_deg",
        ),
        (
            r#"{ "Material": { "roughness": 2 } }"#,
            "value_out_of_range",
            "roughness",
        ),
        (
            r#"{ "Primitive": { "shape": "Cubee" } }"#,
            "unknown_variant",
            "Cube",
        ),
        (
            r#"{ "DirectionalLight": { "illuminance_lux": -1 } }"#,
            "value_out_of_range",
            "illuminance",
        ),
        (
            r#"{ "Model": { "asset": "../secret.glb" } }"#,
            "invalid_asset_ref",
            "relative",
        ),
        (
            r#"{ "GlobalTransform": {} }"#,
            "managed_component",
            "Transform",
        ),
    ];
    for (components, code, word) in cases {
        let source = format!(
            r#"{{ "format": 1, "entities": {{ "e": {{ "components": {components} }} }} }}"#
        );
        let result = load(&source, "s.scene.json", world.registry());
        let problem = result.diagnostics.iter().find(|d| d.is_error()).unwrap();
        assert_eq!(problem.code, code, "{components}: {problem}");
        let text = format!("{} {}", problem.message, problem.hint);
        assert!(text.contains(word), "{components}: {text}");
        assert!(problem.location.line.is_some(), "{components}");
    }
}

#[test]
fn a_camera_that_cannot_be_inverted_is_explained() {
    let mut world = world();
    let source = r#"{ "format": 1, "entities": { "c": { "components": { "Camera": {},
        "Transform": { "scale": [0, 1, 1] } } } } }"#;
    scene(source, &world).instantiate(&mut world).unwrap();
    update_global_transforms(&mut world).unwrap();
    let problem = extract(&world).unwrap_err();
    assert_eq!(problem.code, "invalid_camera");
    assert!(problem.hint.contains("scale"), "{}", problem.hint);
}

#[test]
fn a_material_file_is_extracted_beside_the_numbers() {
    let (world, scene, entities) = extracted(
        r#"{ "format": 1, "entities": {
            "shared": { "components": { "Primitive": {}, "Material": {
                "base_color": [1, 0, 0, 1], "file": "assets/wall.mat.json" } } },
            "plain": { "components": { "Primitive": {}, "Material": { "base_color": [0, 1, 0, 1] } } },
            "bare": { "components": { "Primitive": {} } }
        } }"#,
    );
    let draw = |id: &str| {
        scene
            .draws
            .iter()
            .find(|d| d.entity == entities[id])
            .unwrap()
    };
    // A file is named, not followed here: the numbers of the entity stay
    // beside it (what the renderer falls back to).
    let shared = draw("shared");
    assert_eq!(
        shared.material_file.as_deref(),
        Some("assets/wall.mat.json")
    );
    assert_eq!(shared.material.base_color, [1.0, 0.0, 0.0, 1.0]);
    assert!(draw("plain").material_file.is_none());
    assert!(draw("bare").material_file.is_none());
    assert_eq!(draw("bare").material, wish_render::MaterialData::default());
    assert_eq!(world.entities().count(), 3);
    // The component is its four numbers and the handle of the file.
    assert_eq!(size_of::<wish_render::Material>(), 40);
}

#[test]
fn numbers_that_are_not_numbers_never_reach_a_shader_from_a_look() {
    use wish_render::{AlphaMode, Look, MaterialFile};
    // A look can be built by code as well as read from a checked file.
    let wild = MaterialFile {
        base_color: wish_core::Color::rgba(f32::NAN, -1.0, f32::INFINITY, 0.5),
        metallic: f32::NAN,
        roughness: 7.0,
        emissive: Vec3::new(f32::NEG_INFINITY, f32::NAN, 2.0),
        normal_scale: f32::INFINITY,
        occlusion_strength: f32::NAN,
        uv_scale: [f32::NAN, 0.0],
        uv_offset: [f32::INFINITY, f32::NAN],
        alpha_mode: AlphaMode::Mask,
        alpha_cutoff: f32::NAN,
        ..MaterialFile::default()
    };
    let look = Look::of(&wild, &|_| None);
    let numbers = look
        .material
        .base_color
        .iter()
        .chain(&look.material.emissive)
        .chain(&look.surface.uv_scale)
        .chain(&look.surface.uv_offset)
        .chain([
            &look.material.metallic,
            &look.material.roughness,
            &look.surface.normal_scale,
            &look.surface.occlusion_strength,
            &look.surface.alpha_cutoff,
        ]);
    for number in numbers {
        assert!(number.is_finite(), "{look:?}");
    }
    assert_eq!(look.material.base_color[1], 0.0, "no negative colour");
    assert_eq!(look.surface.uv_scale, [1.0, 1.0], "a picture is drawn once");
    assert_eq!(look.surface.uv_offset[1], 0.0);
    assert_eq!(
        (look.material.roughness, look.surface.normal_scale),
        (1.0, 8.0)
    );
    assert!(look.surface.alpha_cutoff > 0.0, "a mask stays a mask");
    assert!(!look.surface.textured());
    // An opaque look never cuts, whatever its cutoff says; the defaults are
    // the plain white plastic of an entity without a Material.
    let opaque = MaterialFile {
        alpha_cutoff: 0.9,
        ..MaterialFile::default()
    };
    assert_eq!(Look::of(&opaque, &|_| None).surface.alpha_cutoff, 0.0);
    assert_eq!(
        Look::of(&MaterialFile::default(), &|_| None),
        Look::default()
    );
}
