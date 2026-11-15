//! Drawing scenes: from a scene file to pixels, on the software adapter.
//!
//! `UPDATE_GOLDEN=1` rewrites the golden images (review the diff before
//! committing). Every run keeps what it drew in
//! `$CARGO_TARGET_TMPDIR/captures/` for CI artifacts.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use wish_core::math::Vec3;
use wish_ecs::World;
use wish_render::{
    Frame, Gpu, GpuOptions, RenderScene, Renderer, RgbaImage, Tolerance, engine_shaders, extract,
    project_to_pixel,
};
use wish_scene::load;
use wish_transform::update_global_transforms;

/// Measured on this scene (design note §14, WP3): WARP, lavapipe and an RTX
/// 3060 Ti differ by at most 1/255. A pixel counts as different above 2.
const TOLERANCE: Tolerance = Tolerance {
    channel: 2,
    fraction: 0.005,
};

const SCENE: &str = r#"{
  "format": 1,
  "entities": {
    "camera": {
      "components": {
        "Camera": { "fov_y_deg": 45 },
        "Transform": { "translation": [0, 1.6, 6], "rotation": [-0.0871557, 0, 0, 0.9961947] }
      }
    },
    "cube": {
      "components": {
        "Material": { "base_color": [0.8, 0.1, 0.08, 1], "roughness": 0.6 },
        "Primitive": { "shape": "Cube" },
        "Transform": { "translation": [-1.1, 0.5, 0], "rotation": [0, 0.3826834, 0, 0.9238795] }
      }
    },
    "ground": {
      "components": {
        "Material": { "base_color": [0.45, 0.45, 0.42, 1], "roughness": 0.9 },
        "Primitive": { "shape": "Plane" },
        "Transform": { "scale": [30, 1, 30] }
      }
    },
    "lamp": {
      "components": {
        "Material": { "base_color": [0.1, 0.1, 0.1, 1], "emissive": [0.2, 0.9, 0.3] },
        "Primitive": { "shape": "Sphere" },
        "Transform": { "translation": [0, 0.25, 1.5], "scale": [0.5, 0.5, 0.5] }
      }
    },
    "orb": {
      "components": {
        "Material": { "base_color": [0.95, 0.75, 0.35, 1], "metallic": 1, "roughness": 0.25 },
        "Primitive": { "shape": "Sphere" },
        "Transform": { "translation": [1.1, 0.6, 0], "scale": [1.2, 1.2, 1.2] }
      }
    },
    "sun": {
      "components": {
        "DirectionalLight": { "illuminance_lux": 100000 },
        "Transform": { "rotation": [-0.4545195, 0.3304543, 0.1786597, 0.8067046] }
      }
    }
  }
}
"#;

fn world() -> World {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    world
}

fn prepare(source: &str) -> (RenderScene, wish_scene::SceneEntities) {
    let mut world = world();
    let result = load(source, "scenes/test.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let entities = result.document.unwrap().instantiate(&mut world).unwrap();
    update_global_transforms(&mut world).unwrap();
    (extract(&world).unwrap(), entities)
}

fn gpu() -> Gpu {
    Gpu::new(GpuOptions { software: true }).expect("a software adapter")
}

fn keep(name: &str, image: &RgbaImage, gpu: &Gpu) {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("captures");
    std::fs::create_dir_all(&folder).unwrap();
    let tag = format!(
        "{}-{}",
        std::env::consts::OS,
        gpu.adapter().backend.to_lowercase()
    );
    std::fs::write(
        folder.join(format!("{name}-{tag}.png")),
        image.to_png().unwrap(),
    )
    .unwrap();
}

fn check_golden(name: &str, image: &RgbaImage) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, image.to_png().unwrap()).unwrap();
        return;
    }
    let golden = RgbaImage::from_png(&std::fs::read(&path).unwrap_or_else(|_| {
        panic!(
            "no golden image at {}; run with UPDATE_GOLDEN=1",
            path.display()
        )
    }))
    .unwrap();
    let diff = image.diff(&golden, TOLERANCE.channel).expect("same size");
    assert!(
        image.matches(&golden, TOLERANCE),
        "{name} differs from its golden image: {diff:?} (tolerance {TOLERANCE:?})"
    );
}

fn pixel_at(frame: &Frame, scene: &RenderScene, point: Vec3) -> [u8; 4] {
    let camera = scene
        .cameras
        .iter()
        .find(|c| c.entity == frame.camera)
        .unwrap();
    let (width, height) = (frame.image.width, frame.image.height);
    let view_proj = camera.projection(width as f32 / height as f32) * camera.view;
    let (x, y) = project_to_pixel(&view_proj, point, width, height).expect("in front");
    frame.image.pixel(x as u32, y as u32).expect("on screen")
}

#[test]
fn a_scene_is_drawn_with_sky_light_and_materials() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, entities) = prepare(SCENE);
    let frame = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
    keep("basic", &frame.image, &gpu);
    assert_eq!(frame.camera, entities["camera"]);
    assert_eq!(frame.drawn, 4);
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);

    // The top of the image is sky: blue dominates.
    let sky = frame.image.pixel(160, 4).unwrap();
    assert!(sky[2] > sky[0] && sky[2] > 120, "sky {sky:?}");
    // The cube's lit top is red.
    let cube = pixel_at(&frame, &scene, Vec3::new(-1.1, 1.0, 0.0));
    assert!(
        cube[0] > 2 * cube[1] && cube[0] > 2 * cube[2],
        "cube {cube:?}"
    );
    // The lamp glows green even though its base colour is dark.
    let lamp = pixel_at(&frame, &scene, Vec3::new(0.0, 0.25, 1.75));
    assert!(lamp[1] > lamp[0] && lamp[1] > 120, "lamp {lamp:?}");
    // The gold orb reflects warm light.
    let orb = pixel_at(&frame, &scene, Vec3::new(1.1, 0.9, 0.55));
    assert!(orb[0] > orb[2], "orb {orb:?}");
    // The ground in front is grey.
    let ground = pixel_at(&frame, &scene, Vec3::new(0.0, 0.0, 2.5));
    let spread = ground.iter().take(3).max().unwrap() - ground.iter().take(3).min().unwrap();
    assert!(spread < 40, "ground {ground:?}");

    check_golden("basic", &frame.image);
}

/// The ground point the sun's ray through `point` lands on.
fn shadow_of(scene: &RenderScene, point: Vec3) -> Vec3 {
    let direction = scene.lights[0].direction;
    point + direction * (-point.y / direction.y)
}

#[test]
fn objects_cast_shadows_on_the_ground_and_a_light_can_turn_them_off() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (shadowed, _) = prepare(SCENE);
    let source = SCENE.replace(
        "\"DirectionalLight\": { \"illuminance_lux\": 100000 }",
        "\"DirectionalLight\": { \"illuminance_lux\": 100000, \"shadows\": false }",
    );
    assert_ne!(source, SCENE, "the scene was edited");
    let (unshadowed, _) = prepare(&source);
    assert!(shadowed.lights[0].shadows && !unshadowed.lights[0].shadows);
    let with = renderer.render(&gpu, &shadowed, None, 320, 240).unwrap();
    let without = renderer.render(&gpu, &unshadowed, None, 320, 240).unwrap();
    let luminance = |p: [u8; 4]| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]);

    // The tips of the shadows: where the sun's rays through the top of each
    // object land on the ground. These points are open ground in the image
    // (not hidden behind the objects), so without shadows they are lit.
    let reference = luminance(pixel_at(&without, &unshadowed, Vec3::new(0.0, 0.0, 2.5)));
    {
        let (name, top) = ("orb", Vec3::new(1.1, 1.2, 0.0));
        let tip = shadow_of(&shadowed, top);
        let dark = luminance(pixel_at(&with, &shadowed, tip));
        let bright = luminance(pixel_at(&without, &unshadowed, tip));
        assert!(
            bright * 10 > reference * 9,
            "without shadows the ground at the {name}'s shadow tip is lit ({bright} vs {reference})"
        );
        assert!(
            dark * 10 < bright * 7,
            "with shadows the ground at the {name}'s shadow tip is clearly darker ({dark} vs {bright})"
        );
    }
    // All three objects cast shadows: the images differ in a lot of pixels
    // (the cube's and the lamp's shadows are not checked point by point).
    let diff = with.image.diff(&without.image, 20).expect("same size");
    assert!(
        diff.differing_pixels > 400,
        "shadows change the image: {diff:?}"
    );
    // Away from every shadow the two images agree.
    let far = Vec3::new(0.6, 0.0, 2.5);
    let (a, b) = (
        pixel_at(&with, &shadowed, far),
        pixel_at(&without, &unshadowed, far),
    );
    assert!(
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2),
        "{a:?} vs {b:?}"
    );
}
#[test]
fn a_scene_without_a_light_draws_without_shadow_trouble() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let source = SCENE.replace(
        "\"DirectionalLight\": { \"illuminance_lux\": 100000 }",
        "\"Primitive\": { \"shape\": \"Cube\" }",
    );
    let (scene, _) = prepare(&source);
    assert!(scene.lights.is_empty());
    let frame = renderer.render(&gpu, &scene, None, 96, 72).unwrap();
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
}
#[test]
fn drawing_is_repeatable() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, _) = prepare(SCENE);
    let first = renderer.render(&gpu, &scene, None, 96, 64).unwrap();
    let second = renderer.render(&gpu, &scene, None, 96, 64).unwrap();
    assert_eq!(first.image, second.image);
}

/// Regression: on WARP, two devices drawing at the same time crashed the
/// process (access violation in the driver) before all GPU work took one
/// process-wide lock. See gpu.rs.
#[test]
fn separate_devices_can_draw_at_the_same_time() {
    let (scene, _) = prepare(SCENE);
    let scene = std::sync::Arc::new(scene);
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let scene = std::sync::Arc::clone(&scene);
            std::thread::spawn(move || {
                let gpu = Gpu::new(GpuOptions { software: true }).unwrap();
                let renderer = Renderer::new(&gpu).unwrap();
                for _ in 0..3 {
                    renderer.render(&gpu, &scene, None, 128, 96).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let shared = Gpu::shared(GpuOptions { software: true }).unwrap();
    let again = Gpu::shared(GpuOptions { software: true }).unwrap();
    assert!(
        std::sync::Arc::ptr_eq(&shared, &again),
        "one shared device per kind"
    );
}

#[test]
fn cameras_are_chosen_or_explained() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, entities) = prepare(SCENE);
    let frame = renderer
        .render(&gpu, &scene, Some(entities["camera"]), 32, 32)
        .unwrap();
    assert_eq!(frame.camera, entities["camera"]);
    let problem = renderer
        .render(&gpu, &scene, Some(entities["cube"]), 32, 32)
        .unwrap_err();
    assert_eq!(problem.code, "not_a_camera");
    let (empty, _) = prepare(r#"{ "format": 1 }"#);
    let problem = renderer.render(&gpu, &empty, None, 32, 32).unwrap_err();
    assert_eq!(problem.code, "no_camera");
    assert!(problem.hint.contains("Camera"));
}

#[test]
fn what_cannot_be_drawn_is_reported_not_hidden() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, _) = prepare(
        r#"{ "format": 1, "entities": {
            "c": { "components": { "Camera": {}, "Transform": { "translation": [0, 0, 5] } } },
            "flat": { "components": { "Primitive": { "shape": "Cube" }, "Transform": { "scale": [1, 0, 1] } } },
            "hat": { "components": { "Model": { "asset": "assets/hat.glb" } } }
        } }"#,
    );
    let frame = renderer.render(&gpu, &scene, None, 32, 32).unwrap();
    assert_eq!(frame.drawn, 0);
    let codes: Vec<&str> = frame.warnings.iter().map(|w| w.code.as_ref()).collect();
    assert_eq!(codes, ["degenerate_transform", "model_not_loaded"]);
    assert!(frame.warnings.iter().all(|w| !w.hint.is_empty()));
}

#[test]
fn the_engine_shaders_check_clean_and_map_errors_to_their_module() {
    let library = engine_shaders();
    for entry in ["sky_pass", "opaque", "tonemap_pass", "shadow_pass"] {
        library
            .check(entry)
            .unwrap_or_else(|problem| panic!("{entry}: {problem}"));
    }
    let mut broken = engine_shaders();
    broken.add(
        "brdf",
        "wish_render/shaders/brdf.wgsl",
        "#import common\n\nfn distribution_ggx(n_dot_h: f32, alpha: f32) -> f32 {\n    return alpha * ;\n}\n",
    );
    let problem = broken.check("opaque").unwrap_err();
    assert_eq!(problem.code, "shader_syntax");
    assert_eq!(
        problem.location.file.as_deref(),
        Some("wish_render/shaders/brdf.wgsl")
    );
    assert_eq!(problem.location.line, Some(4));
}
