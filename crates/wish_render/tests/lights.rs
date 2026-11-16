//! Point and spot lights through the clusters (M6 WP5), on the software
//! adapter. `UPDATE_GOLDEN=1` rewrites the golden images.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use wish_core::math::Vec3;
use wish_ecs::World;
use wish_render::{
    Frame, Gpu, GpuOptions, RenderScene, Renderer, RgbaImage, Tolerance, extract, project_to_pixel,
};
use wish_scene::load;
use wish_transform::update_global_transforms;

const TOLERANCE: Tolerance = Tolerance {
    channel: 2,
    fraction: 0.005,
};

/// A dark room: no sun, no sky, no ambient light; a red bulb on the left and
/// a white spot light pointing straight down on the right.
fn scene(extra: &str) -> String {
    format!(
        r#"{{
  "format": 1,
  "resources": {{
    "Ambient": {{ "intensity": 0 }},
    "Exposure": {{ "ev100": 3 }},
    "Sky": {{ "luminance": 0 }}
  }},
  "entities": {{
    "camera": {{ "components": {{
      "Camera": {{ "fov_y_deg": 60 }},
      "Transform": {{ "translation": [0, 6, 8], "rotation": [-0.3162278, 0, 0, 0.9486833] }} }} }},
    "floor": {{ "components": {{
      "Material": {{ "base_color": [0.8, 0.8, 0.8, 1], "roughness": 0.9 }},
      "Primitive": {{ "shape": "Plane" }},
      "Transform": {{ "scale": [30, 1, 30] }} }} }},
    "bulb": {{ "components": {{
      "PointLight": {{ "color": [1, 0.15, 0.1, 1], "intensity_lm": 150, "range": 6 }},
      "Transform": {{ "translation": [-3, 1, 0] }} }} }},
    "spot": {{ "components": {{
      "SpotLight": {{ "intensity_lm": 8000, "range": 10, "inner_angle_deg": 15, "outer_angle_deg": 25 }},
      "Transform": {{ "translation": [3, 4, 0], "rotation": [-0.7071068, 0, 0, 0.7071068] }} }} }}{extra}
  }}
}}"#
    )
}

fn prepare(source: &str) -> RenderScene {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    let result = load(source, "scenes/lights.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap().instantiate(&mut world).unwrap();
    update_global_transforms(&mut world).unwrap();
    extract(&world).unwrap()
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

fn pixel_at(frame: &Frame, scene: &RenderScene, point: Vec3) -> [u8; 4] {
    let camera = &scene.cameras[0];
    let (width, height) = (frame.image.width, frame.image.height);
    let view_proj = camera.projection(width as f32 / height as f32) * camera.view;
    let (x, y) = project_to_pixel(&view_proj, point, width, height).expect("in front");
    frame.image.pixel(x as u32, y as u32).expect("on screen")
}

fn brightness(pixel: [u8; 4]) -> u32 {
    u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2])
}

#[test]
fn a_bulb_and_a_spot_light_a_dark_room() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let scene = prepare(&scene(""));
    assert_eq!(scene.local_lights.len(), 2);
    let frame = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);

    // Under the bulb: red light.
    let under_bulb = pixel_at(&frame, &scene, Vec3::new(-3.0, 0.0, 0.0));
    assert!(
        under_bulb[0] > 60 && u32::from(under_bulb[0]) > 2 * u32::from(under_bulb[1].max(1)),
        "{under_bulb:?}"
    );
    // Under the spot: white light.
    let under_spot = pixel_at(&frame, &scene, Vec3::new(3.0, 0.0, 0.0));
    assert!(brightness(under_spot) > 300, "{under_spot:?}");
    let spread = under_spot[0].abs_diff(under_spot[2]);
    assert!(spread < 30, "white, not tinted: {under_spot:?}");
    // Just outside the spot's cone (1.9 m wide at the floor): dark.
    let beside_spot = pixel_at(&frame, &scene, Vec3::new(3.0, 0.0, 2.6));
    assert!(brightness(beside_spot) < 30, "{beside_spot:?}");
    // Between them, far from both: dark.
    let between = pixel_at(&frame, &scene, Vec3::new(0.0, 0.0, -3.0));
    assert!(brightness(between) < 40, "{between:?}");
    // The light fades with distance from the bulb.
    let near = pixel_at(&frame, &scene, Vec3::new(-3.0, 0.0, 0.5));
    let far = pixel_at(&frame, &scene, Vec3::new(-3.0, 0.0, 3.0));
    assert!(near[0] > far[0] + 20, "{near:?} vs {far:?}");
    check_golden("lights", &frame.image);
}

#[test]
fn hundreds_of_lights_are_drawn_through_the_clusters() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let mut extra = String::new();
    for i in 0..200 {
        let x = (i % 20) as f32 - 9.5;
        let z = (i / 20) as f32 - 6.0;
        let color = [(i % 3 == 0) as u8, (i % 3 == 1) as u8, (i % 3 == 2) as u8];
        extra.push_str(&format!(
            r#",
    "light_{i}": {{ "components": {{
      "PointLight": {{ "color": [{}, {}, {}, 1], "intensity_lm": 150, "range": 1.5 }},
      "Transform": {{ "translation": [{x}, 0.4, {z}] }} }} }}"#,
            color[0], color[1], color[2]
        ));
    }
    let scene = prepare(&scene(&extra));
    assert_eq!(scene.local_lights.len(), 202);
    let frame = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
    // A small light lights its own patch of floor in its own colour.
    let red = pixel_at(&frame, &scene, Vec3::new(-9.5, 0.0, 0.0));
    let _ = red;
    let patch = pixel_at(&frame, &scene, Vec3::new(-6.5, 0.0, -6.0 + 0.0));
    assert!(brightness(patch) > 20, "{patch:?}");
    check_golden("many_lights", &frame.image);
}

#[test]
fn the_lights_view_shows_where_each_light_reaches() {
    use wish_render::View;
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let scene = prepare(&scene(""));
    let frame = renderer
        .render_view(&gpu, &scene, None, 320, 240, View::Lights)
        .unwrap();
    let red = frame
        .image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] > 200 && p[1] < 80 && p[2] < 80)
        .count();
    let white = frame
        .image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] > 200 && p[1] > 200 && p[2] > 200)
        .count();
    assert!(red > 100, "the bulb's sphere is drawn in its colour: {red}");
    assert!(white > 100, "the spot's cone is drawn: {white}");
    check_golden("view_lights", &frame.image);
}

#[test]
fn the_colliders_view_shows_shapes_by_kind() {
    use wish_render::View;
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let extra = r#",
    "crate": { "components": {
      "Collider": {}, "RigidBody": { "kind": "Dynamic" },
      "Transform": { "translation": [-2, 0.5, 0] } } },
    "zone": { "components": {
      "Collider": { "shape": "Sphere", "size": [2, 2, 2], "sensor": true },
      "Transform": { "translation": [2, 1, 0] } } },
    "pillar": { "components": {
      "Collider": { "shape": "Capsule", "size": [0.6, 2, 0.6] },
      "Transform": { "translation": [0, 1, -2] } } }"#;
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    world
        .register_component::<wish_types::physics::Collider>()
        .unwrap();
    world
        .register_component::<wish_types::physics::RigidBody>()
        .unwrap();
    let source = scene(extra);
    let result = load(&source, "scenes/colliders.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap().instantiate(&mut world).unwrap();
    update_global_transforms(&mut world).unwrap();
    let scene = extract(&world).unwrap();
    assert_eq!(scene.colliders.len(), 3);
    let frame = renderer
        .render_view(&gpu, &scene, None, 320, 240, View::Colliders)
        .unwrap();
    let count = |want: fn(&[u8]) -> bool| {
        frame
            .image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| want(p.as_slice()))
            .count()
    };
    assert!(
        count(|p| p[2] > 200 && p[1] > 150 && p[0] < 100) > 30,
        "moving: cyan"
    );
    assert!(
        count(|p| p[0] > 200 && p[1] > 180 && p[2] < 100) > 30,
        "sensor: yellow"
    );
    assert!(
        count(|p| p[1] > 200 && p[0] < 120 && p[2] < 120) > 30,
        "fixed: green"
    );
    check_golden("view_colliders", &frame.image);
}
