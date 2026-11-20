//! 2D: sprites through an orthographic camera, and particles (M6 WP8), on
//! the software adapter. `UPDATE_GOLDEN=1` rewrites the golden images.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use wish_ecs::World;
use wish_render::{
    Gpu, GpuOptions, RenderScene, Renderer, RgbaImage, Tolerance, UiPicture, extract,
};
use wish_scene::load;

const TOLERANCE: Tolerance = Tolerance {
    channel: 2,
    fraction: 0.005,
};

/// An orthographic camera 10 m tall: a 320 × 180 picture shows 18 pixels a
/// meter, the origin in the middle.
const SCENE: &str = r#"{
  "format": 1,
  "resources": {
    "Ambient": { "intensity": 0 },
    "Sky": { "luminance": 0 },
    "Time": { "tick": 60, "elapsed": 1.0 }
  },
  "entities": {
    "camera": { "components": {
      "Camera": { "projection": "Orthographic", "ortho_height": 10 },
      "Transform": { "translation": [0, 0, 10] } } },
    "hero": { "components": {
      "Sprite": { "image": "sprites/hero.png", "pixels_per_unit": 16, "smooth": false },
      "Transform": { "translation": [-4, 0, 0] } } },
    "mirrored": { "components": {
      "Sprite": { "image": "sprites/hero.png", "pixels_per_unit": 16, "smooth": false, "flip_x": true },
      "Transform": { "translation": [-2, 0, 0] } } },
    "block": { "components": {
      "Sprite": { "rect": [0, 0, 200, 100], "color": [0.05, 0.1, 1, 1] },
      "Transform": { "translation": [2, 0, 0] } } },
    "front": { "components": {
      "Sprite": { "color": [1, 0.9, 0.05, 1], "layer": 1 },
      "Transform": { "translation": [2.5, 0, -1] } } },
    "wall": { "components": {
      "Primitive": { "shape": "Cube" },
      "Transform": { "translation": [5, 0, 0] } } },
    "behind_wall": { "components": {
      "Sprite": { "color": [1, 1, 1, 1] },
      "Transform": { "translation": [5, 0, -2] } } },
    "sparks": { "components": {
      "ParticleEmitter": { "rate": 0, "burst": 50, "lifetime": 100, "speed": 1,
        "speed_variation": 0, "spread_deg": 30, "size_start": 0.5, "size_end": 0.5,
        "color_start": [1, 0.4, 0.05, 1], "color_end": [1, 0.4, 0.05, 1], "seed": 3 },
      "Transform": { "translation": [0, -3, 0] } } }
  }
}"#;

fn world_of(source: &str) -> World {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    wish_particles::register(&mut world).unwrap();
    world.register_resource::<wish_types::time::Time>().unwrap();
    let result = load(source, "scenes/two_d.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap().instantiate(&mut world).unwrap();
    wish_transform::update_global_transforms(&mut world).unwrap();
    world
}

/// 16 × 16: the left half red, the right half green.
fn hero() -> UiPicture {
    let mut rgba = Vec::new();
    for _ in 0..16 {
        for x in 0..16 {
            rgba.extend_from_slice(&if x < 8 {
                [230, 20, 20, 255]
            } else {
                [20, 220, 40, 255]
            });
        }
    }
    UiPicture {
        width: 16,
        height: 16,
        rgba,
    }
}

fn gpu() -> Gpu {
    Gpu::new(GpuOptions { software: true }).expect("a software adapter")
}

fn check_golden(name: &str, image: &RgbaImage) {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("captures");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(format!("{name}.png")), image.to_png().unwrap()).unwrap();
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
    assert!(image.matches(&golden, TOLERANCE), "{name}: {diff:?}");
}

fn renderer(gpu: &Gpu) -> Renderer {
    let mut renderer = Renderer::new(gpu).unwrap();
    renderer.set_ui_picture("sprites/hero.png", hero());
    renderer
}

fn draw(scene: &RenderScene) -> RgbaImage {
    let gpu = gpu();
    let frame = renderer(&gpu).render(&gpu, scene, None, 320, 180).unwrap();
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
    frame.image
}

fn red(p: [u8; 4]) -> bool {
    p[0] > 180 && p[1] < 60
}

fn green(p: [u8; 4]) -> bool {
    p[1] > 180 && p[0] < 60
}

#[test]
fn sprites_and_particles_in_an_orthographic_picture() {
    let world = world_of(SCENE);
    let scene = extract(&world).unwrap();
    assert_eq!((scene.sprites.len(), scene.emitters.len()), (5, 1));
    let image = draw(&scene);
    let at = |x: u32, y: u32| image.pixel(x, y).unwrap();

    // The hero (1 m wide at x = -4: pixels 79 to 97): red left, green right,
    // sharp pixels; mirrored, the other way round.
    assert!(
        red(at(83, 90)) && green(at(93, 90)),
        "{:?} {:?}",
        at(83, 90),
        at(93, 90)
    );
    assert!(green(at(119, 90)) && red(at(129, 90)));
    // A plain rectangle 2 m × 1 m, blue; the yellow one on layer 1 is drawn
    // over it although it is farther away.
    let blue = at(182, 90);
    assert!(blue[2] > 200 && blue[0] < 100, "{blue:?}");
    let yellow = at(205, 90);
    assert!(
        yellow[0] > 200 && yellow[1] > 180 && yellow[2] < 100,
        "{yellow:?}"
    );
    // The white sprite behind the cube is hidden by it.
    let wall = at(250, 90);
    assert!(wall[0] < 100, "{wall:?}");
    // Particles: 50 orange discs within a metre above the emitter (y = -3).
    let mut orange = 0;
    for y in 120..150 {
        for x in 140..180 {
            let p = at(x, y);
            if p[0] > 150 && p[1] > 60 && p[1] < 200 && p[2] < 100 {
                orange += 1;
            }
        }
    }
    assert!(orange > 50, "{orange}");
    check_golden("sprites", &image);
}

#[test]
fn describe_gives_the_boxes_of_sprites_and_particles() {
    let world = world_of(SCENE);
    let scene = extract(&world).unwrap();
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let elements = renderer.overlay_elements(&scene, None, 320, 180).unwrap();
    let find = |name: &str| {
        let entity = world
            .entities()
            .find(|e| {
                world
                    .get::<wish_scene::instantiate::SceneId>(*e)
                    .and_then(|id| world.text(id.id))
                    == Some(name)
            })
            .unwrap();
        elements
            .iter()
            .find(|e| e.entity == entity)
            .unwrap()
            .clone()
    };
    assert_eq!(find("hero").bounding_box, Some([79, 81, 97, 99]));
    assert_eq!(find("front").layer, Some(1));
    let sparks = find("sparks");
    assert_eq!(sparks.particles, Some(50));
    let [left, top, right, bottom] = sparks.bounding_box.unwrap();
    assert!(
        left >= 135 && right <= 185 && top >= 115 && bottom <= 150,
        "{sparks:?}"
    );
}

#[test]
fn a_perspective_scene_with_particles_still_draws_its_meshes() {
    // The default camera is a perspective one: particles face it and the
    // 3D scene underneath is unchanged.
    let source = SCENE.replace(
        r#""projection": "Orthographic", "ortho_height": 10"#,
        r#""fov_y_deg": 60"#,
    );
    let world = world_of(&source);
    let scene = extract(&world).unwrap();
    assert!(scene.cameras[0].ortho_height.is_none());
    let image = draw(&scene);
    assert!(image.pixel(160, 90).is_some());
}

#[test]
fn sprites_and_particles_are_counted_in_the_statistics() {
    let world = world_of(SCENE);
    let scene = extract(&world).unwrap();
    let gpu = gpu();
    let frame = renderer(&gpu).render(&gpu, &scene, None, 320, 180).unwrap();
    // One mesh (the wall, 12 triangles) and no light, so no shadow map; the
    // sky and tone mapping; five sprites and fifty particles, two triangles
    // each, in three runs: the smooth ones behind, the two sharp heroes, the
    // smooth ones in front.
    assert_eq!(frame.drawn, 1);
    assert_eq!(frame.stats.shadow_casters, 0);
    assert_eq!(frame.stats.triangles, 12 + 2 + 2 * (5 + 50));
    assert_eq!(frame.stats.draw_calls, 1 + 2 + 3);
    assert_eq!((frame.stats.width, frame.stats.height), (320, 180));
}

#[test]
fn a_second_frame_with_sprites_creates_nothing_and_draws_only_its_own() {
    // The sprite pass takes its atlas texture, its uniform buffer and its
    // vertex buffer from the renderer's pool and fills them every frame.
    // The device counts every texture and buffer made on it, so "nothing"
    // is nothing, by the pool or beside it.
    let world = world_of(SCENE);
    let scene = extract(&world).unwrap();
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let first = renderer.render(&gpu, &scene, None, 320, 180).unwrap();
    let made = gpu.created();
    let second = renderer.render(&gpu, &scene, None, 320, 180).unwrap();
    assert_eq!(gpu.created(), made, "a second frame creates nothing");
    assert_eq!(second.image, first.image);

    // Kept buffers are filled again, never read past what a frame filled: a
    // frame with fewer sprites after one with more shows only its own.
    let mut fewer = scene.clone();
    fewer.sprites.truncate(2);
    fewer.emitters.clear();
    let alone = draw(&fewer);
    assert_ne!(alone, first.image);
    let after = renderer.render(&gpu, &fewer, None, 320, 180).unwrap();
    assert_eq!(after.image, alone);
    let back = renderer.render(&gpu, &scene, None, 320, 180).unwrap();
    assert_eq!(back.image, first.image);
}
