//! The debug views (ids, depth, normals, wireframe) and `describe`, on the
//! software adapter. The golden images are reviewed by eye when they are
//! rewritten with `UPDATE_GOLDEN=1`.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use wish_core::math::Vec3;
use wish_ecs::World;
use wish_render::{
    Gpu, GpuOptions, RenderScene, Renderer, RgbaImage, Tolerance, View, extract, project_to_pixel,
};
use wish_scene::load;
use wish_transform::update_global_transforms;

const TOLERANCE: Tolerance = Tolerance {
    channel: 2,
    fraction: 0.005,
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;

/// The scene of the lit golden image: a cube, a metal ball, a lamp and a
/// ground, seen from a little above.
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
        "Primitive": { "shape": "Cube" },
        "Transform": { "translation": [-1.1, 0.5, 0], "rotation": [0, 0.3826834, 0, 0.9238795] }
      }
    },
    "ground": {
      "components": {
        "Primitive": { "shape": "Plane" },
        "Transform": { "scale": [30, 1, 30] }
      }
    },
    "lamp": {
      "components": {
        "Primitive": { "shape": "Sphere" },
        "Transform": { "translation": [0, 0.25, 1.5], "scale": [0.5, 0.5, 0.5] }
      }
    },
    "orb": {
      "components": {
        "Primitive": { "shape": "Sphere" },
        "Transform": { "translation": [1.1, 0.6, 0], "scale": [1.2, 1.2, 1.2] }
      }
    }
  }
}
"#;

/// A big cube in front, a small one exactly behind it (hidden), one that
/// peeks out from behind its side, and one far outside the picture.
const OCCLUSION: &str = r#"{
  "format": 1,
  "entities": {
    "camera": {
      "components": {
        "Camera": { "fov_y_deg": 45 },
        "Transform": { "translation": [0, 1, 8] }
      }
    },
    "front": {
      "components": {
        "Primitive": { "shape": "Cube" },
        "Transform": { "translation": [0, 1, 0], "scale": [2, 2, 2] }
      }
    },
    "behind": {
      "components": {
        "Primitive": { "shape": "Cube" },
        "Transform": { "translation": [0, 1, -3], "scale": [0.5, 0.5, 0.5] }
      }
    },
    "peeking": {
      "components": {
        "Primitive": { "shape": "Cube" },
        "Transform": { "translation": [1.5, 1, -1] }
      }
    },
    "faraway": {
      "components": {
        "Primitive": { "shape": "Cube" },
        "Transform": { "translation": [100, 1, 0] }
      }
    }
  }
}
"#;

fn prepare(source: &str) -> (RenderScene, wish_scene::SceneEntities) {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    let result = load(source, "scenes/test.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let entities = result.document.unwrap().instantiate(&mut world).unwrap();
    update_global_transforms(&mut world).unwrap();
    (extract(&mut world).unwrap(), entities)
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

/// Where a world point lands in the first camera's picture.
fn pixel_of(scene: &RenderScene, point: Vec3) -> (u32, u32) {
    let camera = scene.cameras.first().unwrap();
    let view_proj = camera.projection(WIDTH as f32 / HEIGHT as f32) * camera.view;
    let (x, y) = project_to_pixel(&view_proj, point, WIDTH, HEIGHT).expect("in front");
    (x as u32, y as u32)
}

fn view(gpu: &Gpu, renderer: &Renderer, scene: &RenderScene, which: View, name: &str) -> RgbaImage {
    let frame = renderer
        .render_view(gpu, scene, None, WIDTH, HEIGHT, which)
        .unwrap();
    assert_eq!(frame.image.width, WIDTH);
    assert_eq!(frame.image.height, HEIGHT);
    keep(name, &frame.image, gpu);
    frame.image
}

#[test]
fn the_id_buffer_says_which_entity_is_at_a_pixel() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, entities) = prepare(SCENE);
    let ids = renderer
        .id_buffer(&gpu, &scene, None, WIDTH, HEIGHT)
        .unwrap();
    assert_eq!((ids.width, ids.height), (WIDTH, HEIGHT));
    assert_eq!(ids.camera, entities["camera"]);

    let at = |point: Vec3| {
        let (x, y) = pixel_of(&scene, point);
        ids.entity_at(x, y)
    };
    assert_eq!(at(Vec3::new(1.1, 0.6, 1.2)), Some(entities["orb"]));
    assert_eq!(at(Vec3::new(0.0, 0.25, 2.0)), Some(entities["lamp"]));
    assert_eq!(at(Vec3::new(-1.1, 0.5, 0.9)), Some(entities["cube"]));
    assert_eq!(at(Vec3::new(0.0, 0.0, 3.0)), Some(entities["ground"]));
    // The sky, and outside the picture.
    assert_eq!(ids.entity_at(0, 0), None);
    assert_eq!(ids.entity_at(WIDTH, 0), None);
}

#[test]
fn the_ids_view_paints_each_entity_in_its_own_colour() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, _) = prepare(SCENE);
    let image = view(&gpu, &renderer, &scene, View::Ids, "view-ids");
    let mut colours = std::collections::BTreeSet::new();
    for chunk in image.pixels.as_chunks::<4>().0.iter() {
        colours.insert([chunk[0], chunk[1], chunk[2]]);
    }
    // Four entities and the black of the sky.
    assert_eq!(colours.len(), 5, "{colours:?}");
    assert!(colours.contains(&[0, 0, 0]));
    check_golden("view-ids", &image);
}

#[test]
fn the_depth_view_is_brighter_the_nearer_a_surface_is() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, _) = prepare(SCENE);
    let image = view(&gpu, &renderer, &scene, View::Depth, "view-depth");
    let value = |point: Vec3| {
        let (x, y) = pixel_of(&scene, point);
        let pixel = image.pixel(x, y).unwrap();
        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);
        pixel[0]
    };
    let lamp = value(Vec3::new(0.0, 0.25, 2.0));
    let orb = value(Vec3::new(1.1, 0.6, 1.2));
    let near_ground = value(Vec3::new(0.0, 0.0, 3.0));
    let far_ground = value(Vec3::new(0.0, 0.0, -12.0));
    assert!(lamp > orb, "lamp {lamp}, orb {orb}");
    assert!(orb > far_ground, "orb {orb}, far ground {far_ground}");
    assert!(
        near_ground > far_ground,
        "near ground {near_ground}, far ground {far_ground}"
    );
    check_golden("view-depth", &image);
}

#[test]
fn the_normals_view_shows_which_way_surfaces_face() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, _) = prepare(SCENE);
    let image = view(&gpu, &renderer, &scene, View::Normals, "view-normals");
    // The ground faces +Y: green high, red and blue in the middle.
    let (x, y) = pixel_of(&scene, Vec3::new(0.0, 0.0, 3.0));
    let ground = image.pixel(x, y).unwrap();
    assert!(ground[1] > 240, "{ground:?}");
    assert!((i32::from(ground[0]) - 128).abs() < 12, "{ground:?}");
    assert!((i32::from(ground[2]) - 128).abs() < 12, "{ground:?}");
    // The front of the ball faces the camera (+Z): blue high.
    let (x, y) = pixel_of(&scene, Vec3::new(1.1, 0.6, 0.6));
    let orb = image.pixel(x, y).unwrap();
    assert!(orb[2] > 200, "{orb:?}");
    check_golden("view-normals", &image);
}

#[test]
fn the_wireframe_view_draws_edges_over_a_dark_fill() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, _) = prepare(SCENE);
    let image = view(&gpu, &renderer, &scene, View::Wireframe, "view-wireframe");
    let total = (WIDTH * HEIGHT) as usize;
    let edges = image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] > 200 && p[1] > 200)
        .count();
    let dark = image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] < 40 && p[1] < 40 && p[2] < 40)
        .count();
    assert!(edges > 100, "{edges} edge pixels");
    assert!(edges < total / 8, "{edges} edge pixels is too many");
    assert!(dark > total / 2, "{dark} dark pixels");
    // Nothing in between but the edges' soft ends: no lit shading leaks in.
    assert!(edges + dark > total * 95 / 100);
}

#[test]
fn describe_reports_what_is_visible_and_what_is_not() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, entities) = prepare(OCCLUSION);
    let description = renderer
        .describe(&gpu, &scene, None, WIDTH, HEIGHT)
        .unwrap();
    assert_eq!(description.camera, entities["camera"]);
    assert!(description.warnings.is_empty());

    let find = |name: &str| {
        description
            .visible
            .iter()
            .find(|v| v.entity == entities[name])
    };
    let front = find("front").expect("front is visible");
    let peeking = find("peeking").expect("peeking is visible");
    assert!(find("behind").is_none());
    assert!(find("faraway").is_none());
    assert!(description.hidden.contains(&entities["behind"]));
    assert!(description.hidden.contains(&entities["faraway"]));
    assert!(!description.hidden.contains(&entities["front"]));

    // Nothing hides the front cube; its centre is the centre of the picture,
    // eight meters from the camera.
    assert!(front.visible_fraction.unwrap() > 0.99);
    assert!((front.distance - 8.0).abs() < 0.01, "{}", front.distance);
    let [left, top, right, bottom] = front.bounding_box;
    assert!(left < WIDTH / 2 && WIDTH / 2 < right);
    assert!(top < HEIGHT / 2 && HEIGHT / 2 < bottom);
    assert!(front.pixels > 1000);

    // The cube behind the corner is partly covered by the front cube.
    let fraction = peeking.visible_fraction.unwrap();
    assert!((0.5..0.97).contains(&fraction), "{fraction}");
    assert!(peeking.distance > front.distance);

    // Scene order, and the pixel counts agree with the identity buffer.
    let order: Vec<_> = description.visible.iter().map(|v| v.entity).collect();
    let mut sorted = order.clone();
    sorted.sort_by_key(|entity| {
        scene
            .draws
            .iter()
            .position(|draw| draw.entity == *entity)
            .unwrap()
    });
    assert_eq!(order, sorted);
    let ids = renderer
        .id_buffer(&gpu, &scene, None, WIDTH, HEIGHT)
        .unwrap();
    let counted = ids.raw().iter().filter(|id| **id != 0).count();
    let reported: u32 = description.visible.iter().map(|v| v.pixels).sum();
    assert_eq!(reported as usize, counted);
}

#[test]
fn a_wrong_camera_or_view_name_comes_with_a_hint() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, entities) = prepare(SCENE);
    let problem = renderer
        .render_view(&gpu, &scene, Some(entities["cube"]), 64, 48, View::Depth)
        .unwrap_err();
    assert_eq!(problem.code, "not_a_camera");
    assert!(!problem.hint.is_empty());
    let problem = View::parse("depht").unwrap_err();
    assert_eq!(problem.code, "unknown_view");
    assert_eq!(problem.hint, "Did you mean 'depth'?");
}

#[test]
fn the_lit_view_is_the_normal_render() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, _) = prepare(SCENE);
    let lit = renderer
        .render_view(&gpu, &scene, None, 96, 72, View::Lit)
        .unwrap();
    let plain = renderer.render(&gpu, &scene, None, 96, 72).unwrap();
    assert_eq!(lit.image, plain.image);
}
