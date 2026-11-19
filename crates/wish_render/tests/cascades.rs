//! Cascaded shadows (design note M12-look-3-cascaded-shadows), on the
//! software adapter, with small pictures and small shadow maps.
//!
//! Cascades are off by default, and the first test is the rule the package
//! stands on: a scene that does not ask for them is drawn by the one shadow
//! map as it always was. Every other test turns them on and measures what
//! they should change and what they should not. Shadows are measured on the
//! ground and compared with the picture without shadows. Nothing rests on
//! one pixel's rounding: a shadow and lit ground are some 140 steps of 255
//! apart in every channel, how dark a shadow is is read as a mean over 3x3
//! pixels, and an edge is where a walk along the ground crosses the middle
//! between the two. Pictures drawn through different shadow maps are never
//! asked for equal bytes.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_core::math::{Quat, Vec3};
use wish_ecs::World;
use wish_render::{
    Frame, Gpu, GpuOptions, RenderScene, Renderer, RgbaImage, View, extract, project_to_pixel,
};
use wish_scene::load;

/// The rotation that turns an entity's -Z axis (where cameras look and
/// lights shine) to `direction`.
fn facing(direction: Vec3) -> Value {
    let q = Quat::from_rotation_arc(Vec3::NEG_Z, direction.normalize());
    json!([q.x, q.y, q.z, q.w])
}

/// The sun of most scenes here: it shines along +X and down at 45 degrees,
/// so a thing's shadow lies to its +X side and is as long as the thing is
/// high.
const SUN: Vec3 = Vec3::new(1.0, -1.0, 0.0);

fn sun(direction: Vec3) -> Value {
    json!({ "components": {
        "DirectionalLight": { "illuminance_lux": 100000 },
        "Transform": { "rotation": facing(direction) } } })
}

fn ground(side: f32) -> Value {
    json!({ "components": {
        "Material": { "base_color": [0.6, 0.6, 0.6, 1], "roughness": 0.9 },
        "Primitive": { "shape": "Plane" },
        "Transform": { "scale": [side, 1, side] } } })
}

fn cube(centre: [f32; 3], size: [f32; 3]) -> Value {
    json!({ "components": {
        "Primitive": { "shape": "Cube" },
        "Transform": { "translation": centre, "scale": size } } })
}

fn camera(position: [f32; 3], looks: Vec3) -> Value {
    json!({ "components": {
        "Camera": { "fov_y_deg": 45 },
        "Transform": { "translation": position, "rotation": facing(looks) } } })
}

fn scene(settings: Value, entities: Value) -> String {
    let resources = if settings.is_null() {
        json!({})
    } else {
        json!({ "RenderSettings": settings })
    };
    json!({ "format": 1, "resources": resources, "entities": entities }).to_string()
}

/// A 2 m block on a ground `side` meters wide, seen from above and in front.
/// The shadow of the block's top is the square from x = -1 to x = 1 on the
/// ground around the origin, in open view.
fn block_scene(side: f32, settings: Value) -> String {
    scene(
        settings,
        json!({
            "camera": camera([0.0, 7.0, 7.0], Vec3::new(0.0, -1.0, -1.0)),
            "block": cube([-2.0, 1.0, 0.0], [2.0, 2.0, 2.0]),
            "ground": ground(side),
            "sun": sun(SUN),
        }),
    )
}

/// The eye of [`wall_scene`]: it stands at eye height and looks along the
/// ground.
const EYE: [f32; 3] = [0.0, 1.7, 0.0];
const EYE_LOOKS: Vec3 = Vec3::new(0.0, -0.2, -1.0);

/// A ground 400 m wide with a wall 60 m long that runs away from the camera
/// along -Z, one meter to the camera's left, 2 m high: its shadow is a strip
/// on the ground from x = -1 to x = 1, at every depth up to 60 m.
fn wall_scene(settings: Value) -> String {
    scene(
        settings,
        json!({
            "camera": camera(EYE, EYE_LOOKS),
            "ground": ground(400.0),
            "wall": cube([-1.1, 1.0, -30.0], [0.2, 2.0, 60.0]),
            "sun": sun(SUN),
        }),
    )
}

fn world_of(source: &str) -> World {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    let result = load(source, "scenes/cascades.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap().instantiate(&mut world).unwrap();
    wish_transform::update_global_transforms(&mut world).unwrap();
    world
}

fn prepare(source: &str) -> RenderScene {
    extract(&world_of(source)).unwrap()
}

fn gpu() -> Gpu {
    Gpu::new(GpuOptions { software: true }).expect("a software adapter")
}

/// Keeps what a test drew in `$CARGO_TARGET_TMPDIR/captures/`, to look at.
fn keep(name: &str, image: &RgbaImage) {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("captures");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(format!("{name}.png")), image.to_png().unwrap()).unwrap();
}

const SIZE: (u32, u32) = (320, 240);

fn draw(renderer: &Renderer, gpu: &Gpu, scene: &RenderScene) -> Frame {
    let frame = renderer.render(gpu, scene, None, SIZE.0, SIZE.1).unwrap();
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
    frame
}

fn view(renderer: &Renderer, gpu: &Gpu, scene: &RenderScene) -> Frame {
    renderer
        .render_view(gpu, scene, None, SIZE.0, SIZE.1, View::ShadowCascades)
        .unwrap()
}

/// Where `point` is in the picture, in whole pixels.
fn pixel_of(frame: &Frame, scene: &RenderScene, point: Vec3) -> (i64, i64) {
    let camera = &scene.cameras[0];
    let (width, height) = (frame.image.width, frame.image.height);
    let view_proj = camera.projection(width as f32 / height as f32) * camera.view;
    let (x, y) =
        project_to_pixel(&view_proj, point, width, height).expect("in front of the camera");
    (x as i64, y as i64)
}

fn pixel_at(frame: &Frame, scene: &RenderScene, point: Vec3) -> [u8; 4] {
    let (x, y) = pixel_of(frame, scene, point);
    frame.image.pixel(x as u32, y as u32).expect("on screen")
}

fn sum(pixel: [u8; 4]) -> f32 {
    f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2])
}

/// The brightness of the picture at `point` of the world (red, green and
/// blue added up), at the one pixel the point is in.
fn at_point(frame: &Frame, scene: &RenderScene, point: Vec3) -> f32 {
    sum(pixel_at(frame, scene, point))
}

/// The same as a mean over the 3x3 pixels around it.
fn around(frame: &Frame, scene: &RenderScene, point: Vec3) -> f32 {
    let (x, y) = pixel_of(frame, scene, point);
    let mut total = 0.0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let pixel = frame
                .image
                .pixel((x + dx) as u32, (y + dy) as u32)
                .expect("on screen");
            total += sum(pixel);
        }
    }
    total / 9.0
}

/// The largest difference between two pictures in any channel of any pixel.
fn largest_difference(a: &RgbaImage, b: &RgbaImage) -> u8 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    a.pixels
        .iter()
        .zip(&b.pixels)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

fn cascades(count: u32, distance: f32, map: u32) -> Value {
    json!({ "shadow_cascades": count, "shadow_distance": distance, "shadow_map_size": map })
}

/// Whether `dark` is a shadow next to `lit` (the measure the older shadow
/// tests use).
fn shadowed(dark: f32, lit: f32) -> bool {
    dark * 10.0 < lit * 7.0
}

#[test]
fn the_defaults_are_the_one_shadow_map_to_the_byte() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // Without the settings; with the two new fields spelled out at their
    // defaults; and with one map and another distance, which one map does
    // not use.
    let plain = prepare(&block_scene(12.0, Value::Null));
    assert_eq!(plain.settings.shadow_cascades, 1);
    assert_eq!(plain.settings.shadow_distance, 100.0);
    let spelled = prepare(&block_scene(
        12.0,
        json!({ "shadow_cascades": 1, "shadow_distance": 100 }),
    ));
    let other = prepare(&block_scene(
        12.0,
        json!({ "shadow_cascades": 1, "shadow_distance": 3 }),
    ));
    let first = draw(&renderer, &gpu, &plain);
    assert_eq!(first.stats.shadow_cascades, 1);
    assert_eq!(first.stats.shadow_casters, 2);
    for scene in [&spelled, &other] {
        let frame = draw(&renderer, &gpu, scene);
        assert_eq!(frame.image, first.image);
        assert_eq!(frame.stats, first.stats);
    }
    // A renderer that drew cascades draws the plain scene as it did before,
    // and a fresh renderer draws the same.
    let with = prepare(&block_scene(12.0, cascades(4, 40.0, 256)));
    let cascaded = draw(&renderer, &gpu, &with);
    assert_eq!(cascaded.stats.shadow_cascades, 4);
    assert_ne!(cascaded.image, first.image);
    assert_eq!(draw(&renderer, &gpu, &plain).image, first.image);
    let fresh = Renderer::new(&gpu).unwrap();
    assert_eq!(draw(&fresh, &gpu, &plain).image, first.image);
}

/// Walks across the far edge of the block's shadow (at x = 1) from inside
/// the shadow to the open ground in steps of 2.5 cm, and counts the steps
/// that are neither as dark as `dark` nor as lit as `lit`.
fn edge_steps(frame: &Frame, scene: &RenderScene, (dark, lit): (f32, f32)) -> usize {
    let band = (lit - dark) / 8.0;
    (0..140)
        .map(|step| {
            at_point(
                frame,
                scene,
                Vec3::new(-0.5 + step as f32 * 0.025, 0.0, 0.0),
            )
        })
        .filter(|value| *value > dark + band && *value < lit - band)
        .count()
}

#[test]
fn in_a_large_scene_a_near_shadow_is_sharp_with_cascades() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // The block on a ground 400 m wide, the reason for the package.
    let unshadowed = prepare(&block_scene(400.0, json!({ "shadows": false })));
    let open = draw(&renderer, &gpu, &unshadowed);
    let inside = Vec3::new(0.0, 0.0, 0.0);
    let outside = Vec3::new(3.0, 0.0, 0.0);
    let lit = around(&open, &unshadowed, inside);

    // Cascades of 256 texels: the one that covers the block has texels of a
    // few centimeters.
    let with = prepare(&block_scene(400.0, cascades(4, 40.0, 256)));
    let sharp = draw(&renderer, &gpu, &with);
    keep("cascades_block", &sharp.image);
    let dark = around(&sharp, &with, inside);
    assert!(shadowed(dark, lit), "in shadow: {dark} against {lit}");
    assert!((around(&sharp, &with, outside) - lit).abs() <= 6.0);
    let sharp_steps = edge_steps(&sharp, &with, (dark, lit));

    // One map of the same 256 texels has texels of a meter and a half
    // there. The block is hardly more than one of them: its shadow is gone.
    let small = prepare(&block_scene(400.0, json!({ "shadow_map_size": 256 })));
    let gone = draw(&renderer, &gpu, &small);
    keep("cascades_block_one_map_256", &gone.image);
    let small_middle = around(&gone, &small, inside);

    // One map of 2048 texels, sixteen times the texels of the four cascades
    // together, has texels of 20 cm: the shadow is there, with a wide edge.
    let large = prepare(&block_scene(400.0, json!({ "shadow_map_size": 2048 })));
    let blurred = draw(&renderer, &gpu, &large);
    keep("cascades_block_one_map_2048", &blurred.image);
    let blurred_steps = edge_steps(&blurred, &large, (dark, lit));
    let large_middle = around(&blurred, &large, inside);

    println!(
        "the shadow's edge, in steps of 2.5 cm between dark and lit: {sharp_steps} with four cascades of 256, {blurred_steps} with one map of 2048; its middle: {dark} with cascades, {large_middle} with one map of 2048, {small_middle} with one map of 256, {lit} without shadows"
    );
    // Measured: 4 steps against 15; 254, 254, 678 and 678. A texel of the
    // cascade is 3.6 cm and one of the large map 20 cm, and an edge is
    // three texels wide: about 4 and 24 steps, less what the walk's band
    // leaves out at both ends.
    assert!(sharp_steps <= 7, "a sharp edge: {sharp_steps} step(s)");
    assert!(
        blurred_steps >= 11,
        "one large map's edge is wide: {blurred_steps} step(s)"
    );
    assert!(shadowed(large_middle, lit));
    assert!(
        small_middle > lit - 30.0,
        "one small map loses the shadow: {small_middle} against {lit}"
    );
}

#[test]
fn no_seam_where_one_cascade_hands_over_to_the_next() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // Lit ground alone, to beyond the shadow distance: every cascade border
    // crosses it, and with cascades it is the picture without shadows. No
    // line, no stripes, no patch where the ground shadows itself. The same
    // under a sun that stands low.
    for (name, direction) in [("high", SUN), ("low", Vec3::new(1.0, -0.2, 0.3))] {
        let bare = scene(
            cascades(4, 40.0, 256),
            json!({
                "camera": camera(EYE, EYE_LOOKS),
                "ground": ground(400.0),
                "sun": sun(direction),
            }),
        );
        let with = prepare(&bare);
        let mut without = with.clone();
        without.settings.shadows = false;
        let a = draw(&renderer, &gpu, &with);
        let b = draw(&renderer, &gpu, &without);
        assert_eq!(a.stats.shadow_cascades, 4);
        let difference = largest_difference(&a.image, &b.image);
        assert!(
            difference <= 1,
            "{name} sun: lit ground differs from unshadowed ground by {difference}"
        );
    }

    // A shadow that runs through every cascade: the wall's strip. Up to
    // where the fade starts (36 m) it is as dark at every depth, and the
    // ground beside it as lit: no gap and no dark line at a border.
    let with = prepare(&wall_scene(cascades(4, 40.0, 256)));
    let without = prepare(&wall_scene(json!({ "shadows": false })));
    let frame = draw(&renderer, &gpu, &with);
    let open = draw(&renderer, &gpu, &without);
    keep("cascades_wall", &frame.image);
    let lit = around(&open, &without, Vec3::new(0.0, 0.0, -6.0));
    let dark = around(&frame, &with, Vec3::new(0.0, 0.0, -6.0));
    assert!(shadowed(dark, lit), "{dark} against {lit}");
    let mut darkest = f32::MAX;
    let mut lightest = f32::MIN;
    let mut depth = 4.0;
    while depth <= 30.0 {
        let strip = at_point(&frame, &with, Vec3::new(0.0, 0.0, -depth));
        darkest = darkest.min(strip);
        lightest = lightest.max(strip);
        // Beside the strip, as far to the side as the picture shows there.
        let side = (depth * 0.3).min(4.0);
        let beside = Vec3::new(1.0 + side, 0.0, -depth);
        let (here, there) = (
            at_point(&frame, &with, beside),
            at_point(&open, &without, beside),
        );
        assert!(
            (here - there).abs() <= 3.0,
            "{depth} m: beside the shadow {here}, without shadows {there}"
        );
        depth += 0.25;
    }
    println!("the strip from 4 to 30 m: {darkest} to {lightest} (unshadowed ground: about {lit})");
    // Measured: 256 to 260; the open ground brightens by as much with
    // distance.
    assert!(
        lightest - darkest <= 12.0 && shadowed(lightest, lit),
        "the strip is as dark at every depth: {darkest} to {lightest}"
    );

    // And the strip's edge is where the wall puts it at every depth, to
    // what the cascade in use there can hold. Its place is found by walking
    // out of the shadow in steps of 2 cm. An edge lies about a texel and a
    // half inside where the geometry puts it (a lookup is pushed off the
    // ground by that much, as with one map), so it is allowed two and a
    // half texels of its cascade and a pixel: the texels of these four
    // cascades of 256 are 1.7, 3.6, 7.8 and 24.5 cm. Where two cascades
    // blend, the coarser one's allowance holds. So at a border the edge
    // moves by the difference of two texel sizes over the blend band, and
    // nowhere by more.
    let colours = view(&renderer, &gpu, &with);
    let allowed = [0.065, 0.11, 0.22, 0.65];
    let middle = (dark + lit) / 2.0;
    let mut measured = [(f32::MAX, f32::MIN); 4];
    let mut depth = 5.0;
    while depth <= 30.0 {
        let edge = (0..100)
            .map(|step| 0.2 + step as f32 * 0.02)
            .find(|x| at_point(&frame, &with, Vec3::new(*x, 0.0, -depth)) > middle)
            .expect("the shadow ends");
        // The shadow of the wall's top edge (x = -1, 2 m up) falls at x = 1.
        let off = (edge - 1.0).abs();
        let cascade = cascade_of(pixel_at(&colours, &with, Vec3::new(edge, 0.0, -depth)));
        let limit = cascade.map_or(allowed[3], |cascade| allowed[cascade.min(3)]);
        assert!(
            off <= limit,
            "{depth} m, cascade {cascade:?}: the edge is at x = {edge}, {off} from where it belongs"
        );
        if let Some(cascade) = cascade.filter(|cascade| *cascade < 4) {
            let (low, high) = measured[cascade];
            measured[cascade] = (low.min(edge), high.max(edge));
        }
        depth += 0.5;
    }
    // Measured: 0.92 to 0.94 in the second cascade, 0.86 to 0.92 in the
    // third, 0.62 to 0.74 in the fourth (the edge belongs at 1).
    println!("the strip's edge (x, meters; it belongs at 1) by cascade: {measured:?}");
}

#[test]
fn shadows_fade_out_at_the_shadow_distance() {
    let gpu = gpu();
    // Looking down on the wall from above, so that depths can be told apart
    // far away: the camera is 12 m up and looks ahead and down.
    let eye = [0.0, 12.0, 10.0];
    let looks = Vec3::new(0.0, -0.45, -1.0);
    let entities = |settings: Value| {
        scene(
            settings,
            json!({
                "camera": camera(eye, looks),
                "ground": ground(400.0),
                "wall": cube([-1.1, 1.0, -30.0], [0.2, 2.0, 60.0]),
                "sun": sun(SUN),
            }),
        )
    };
    let renderer = Renderer::new(&gpu).unwrap();
    let with = prepare(&entities(cascades(4, 40.0, 256)));
    let without = prepare(&entities(json!({ "shadows": false })));
    let frame = draw(&renderer, &gpu, &with);
    let open = draw(&renderer, &gpu, &without);
    keep("cascades_fade", &frame.image);
    // The depth of a point of the ground in front of the camera, the
    // measure the shadow distance uses.
    let forward = looks.normalize();
    let depth_of = |z: f32| forward.dot(Vec3::new(0.0, 0.0, z) - Vec3::from(eye));
    // How much of its shadow the strip has at z: 1 dark, 0 gone.
    let dark = around(&frame, &with, Vec3::new(0.0, 0.0, -2.0));
    let lit = around(&open, &without, Vec3::new(0.0, 0.0, -2.0));
    assert!(shadowed(dark, lit), "{dark} against {lit}");
    let share = |z: f32| {
        let point = Vec3::new(0.0, 0.0, z);
        let unshadowed = around(&open, &without, point);
        (unshadowed - around(&frame, &with, point)) / (unshadowed - dark)
    };
    let mut report = Vec::new();
    for z in [-10.0f32, -16.0, -20.0, -22.0, -24.0, -26.0, -30.0, -40.0] {
        report.push(format!("{:.1} m: {:.2}", depth_of(z), share(z)));
    }
    println!(
        "how much shadow is left by depth (shadow distance 40 m): {}",
        report.join(", ")
    );
    // The fade runs from 36 m to 40 m of depth.
    let z_at = |depth: f32| {
        // depth = forward . (z - eye): solve for z on the ground.
        (depth + forward.y * eye[1]) / forward.z + eye[2]
    };
    assert!((depth_of(z_at(30.0)) - 30.0).abs() < 1e-3);
    let full = share(z_at(33.0));
    let half = share(z_at(38.0));
    let none = share(z_at(42.0));
    assert!(full > 0.95, "at 33 m the shadow is whole: {full}");
    // (Half of the light, which is less than half of the picture's
    // encoded brightness: measured 0.40.)
    assert!(
        half > 0.15 && half < 0.75,
        "at 38 m it is half gone: {half}"
    );
    assert!(none.abs() < 0.03, "at 42 m it is gone: {none}");
    // Monotone: farther is never darker.
    let mut last = 1.5;
    let mut depth = 30.0;
    while depth <= 44.0 {
        let here = share(z_at(depth));
        assert!(here <= last + 0.04, "{depth} m: {here} after {last}");
        last = here;
        depth += 1.0;
    }
}

/// The x at which the block's shadow ends on the line z = `z` of the
/// ground: the first step of half a centimeter that is nearer lit than
/// dark.
fn edge_of_block(frame: &Frame, scene: &RenderScene, z: f32, (dark, lit): (f32, f32)) -> f32 {
    let middle = (dark + lit) / 2.0;
    (0..240)
        .map(|step| 0.4 + step as f32 * 0.005)
        .find(|x| at_point(frame, scene, Vec3::new(*x, 0.0, z)) > middle)
        .expect("the shadow ends")
}

/// The lines of the ground the edge is looked for on: thirteen, 10 cm
/// apart.
fn edge_lines() -> impl Iterator<Item = f32> {
    (-6..=6).map(|line| line as f32 * 0.1)
}

/// Where the edge is, as the mean over those lines: finer than the pixel
/// each line is read to.
fn mean_edge(frame: &Frame, scene: &RenderScene, levels: (f32, f32)) -> f32 {
    edge_lines()
        .map(|z| edge_of_block(frame, scene, z, levels))
        .sum::<f32>()
        / edge_lines().count() as f32
}

#[test]
fn a_shadows_edge_stays_put_when_the_camera_turns_and_moves() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // The block's shadow from close by, through cascades of 128 texels. The
    // first cascade covers the edge; a texel of its map is 5.5 cm, which is
    // 7.8 cm across this edge on the ground (the sun stands at 45 degrees),
    // and a pixel of the picture is 1.5 cm.
    //
    // If a map followed the camera freely, its texels would shift in the
    // world by as much as the middle of its slice shifts, seen from the
    // sun, and the edge with them. So each other camera is placed to shift
    // that middle by **half a texel** across the edge, the most a grid can
    // be out of step: 3.9 cm on the ground. (Across this edge the sun sees
    // x + y of the world: moving the camera 3.9 cm along x is half a texel;
    // so is turning it by three quarters of a degree, which moves the
    // slice's middle, 2.96 m ahead, by as much; and so is half of each.
    // Moving along z shifts the grid along the edge and is free.) With the
    // maps snapped to whole texels the edge must not move; without the
    // snap it moved by 3.9 to 4.2 cm in this test (tried: section 6.1 of
    // the design note).
    let shot = |eye: [f32; 3], looks: Vec3| {
        prepare(&scene(
            cascades(4, 60.0, 128),
            json!({
                "camera": camera(eye, looks),
                "block": cube([-2.0, 1.0, 0.0], [2.0, 2.0, 2.0]),
                "ground": ground(400.0),
                "sun": sun(SUN),
            }),
        ))
    };
    let first = shot([1.0, 3.0, 3.0], Vec3::new(0.0, -1.0, -1.0));
    let turned = shot([1.0, 3.0, 3.0], Vec3::new(0.0186, -1.0, -1.0));
    let moved = shot([1.0388, 3.0, 2.93], Vec3::new(0.0, -1.0, -1.0));
    let both = shot([1.0, 3.0194, 3.1], Vec3::new(0.0093, -1.0, -1.0));
    let frames: Vec<(&str, &RenderScene, Frame)> = [
        ("first", &first),
        ("turned", &turned),
        ("moved", &moved),
        ("both", &both),
    ]
    .into_iter()
    .map(|(name, scene)| (name, scene, draw(&renderer, &gpu, scene)))
    .collect();
    keep("cascades_stable", &frames[0].2.image);
    let dark = around(&frames[0].2, &first, Vec3::new(0.0, 0.0, 0.0));
    let lit = around(&frames[0].2, &first, Vec3::new(2.2, 0.0, 0.0));
    assert!(shadowed(dark, lit), "{dark} against {lit}");
    let mut edges = Vec::new();
    for (name, scene, frame) in &frames {
        // The edge is read from the first cascade in every picture, on
        // both sides of it.
        let colours = view(&renderer, &gpu, scene);
        for z in edge_lines() {
            for x in [0.7, 1.3] {
                let cascade = cascade_of(pixel_at(&colours, scene, Vec3::new(x, 0.0, z)));
                assert_eq!(cascade, Some(0), "{name}: ({x}, {z})");
            }
        }
        edges.push(mean_edge(frame, scene, (dark, lit)));
    }
    println!("the shadow's edge (x, meters; mean of 13 lines) from four cameras: {edges:?}");
    for (edge, (name, _, _)) in edges.iter().zip(&frames) {
        // Measured: 0.928, 0.926, 0.929 and 0.928 m, the four agree to
        // 0.3 cm. (With the snap taken out of `fit_cascades`: 0.899, 0.941,
        // 0.938 and 0.939, and this test fails.) Half of what an unsnapped
        // grid moves the edge by is allowed, which is also more than a
        // pixel.
        assert!(
            (edge - edges[0]).abs() <= 0.02,
            "{name}: the edge is at x = {edge}, it was at {}",
            edges[0]
        );
        // Where the block puts it, to two texels.
        assert!((edge - 1.0).abs() <= 0.12, "{name}: x = {edge}");
    }
}

#[test]
fn a_caster_outside_the_slice_still_shadows_it() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // A tower 30 m high stands 20 m behind the camera; the sun shines over
    // the camera's shoulder, so the tower's shadow reaches 10 m in front of
    // it. The tower is in no slice of the view.
    let entities = |settings: Value| {
        scene(
            settings,
            json!({
                "camera": camera(EYE, EYE_LOOKS),
                "ground": ground(400.0),
                "tower": cube([0.0, 15.0, 20.0], [2.0, 30.0, 2.0]),
                "sun": sun(Vec3::new(0.0, -1.0, -1.0)),
            }),
        )
    };
    let with = prepare(&entities(cascades(4, 40.0, 256)));
    let without = prepare(&entities(json!({ "shadows": false })));
    let frame = draw(&renderer, &gpu, &with);
    let open = draw(&renderer, &gpu, &without);
    keep("cascades_tower", &frame.image);
    // The tower is drawn into the cascades it shadows, though no slice
    // holds it: more draws than the ground's four.
    assert_eq!(frame.stats.shadow_cascades, 4);
    assert!(frame.stats.shadow_casters > 4, "{:?}", frame.stats);
    for depth in [5.0f32, 7.0, 9.0] {
        let inside = Vec3::new(0.0, 0.0, -depth);
        let beside = Vec3::new(2.0, 0.0, -depth);
        let lit = around(&open, &without, inside);
        let dark = around(&frame, &with, inside);
        assert!(shadowed(dark, lit), "{depth} m: {dark} against {lit}");
        assert!((around(&frame, &with, beside) - around(&open, &without, beside)).abs() <= 3.0);
    }
    // Beyond the shadow's tip (at 10 m) the ground is lit.
    let past = Vec3::new(0.0, 0.0, -12.0);
    assert!((around(&frame, &with, past) - around(&open, &without, past)).abs() <= 3.0);
}

fn glb(json: &Value, binary: &[u8]) -> Vec<u8> {
    let mut text = serde_json::to_vec(json).unwrap();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let mut bin = binary.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + text.len() + 8 + bin.len();
    let mut out = b"glTF".to_vec();
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

/// A fence panel: a 2 m square in the XY plane facing +Z (x and y from -1
/// to 1) with a 2x1 texture. Its left half (x < 0) is solid, its right half
/// has an alpha of zero. `mask` makes the material cut the clear half out.
fn fence(mask: bool) -> wish_import::Model {
    let floats =
        |values: &[f32]| -> Vec<u8> { values.iter().flat_map(|v| v.to_le_bytes()).collect() };
    let positions = floats(&[
        -1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 1.0, 0.0, -1.0, 1.0, 0.0,
    ]);
    let normals = floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
    let uvs = floats(&[0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0]);
    let indices: Vec<u8> = [0u16, 1, 2, 0, 2, 3]
        .iter()
        .flat_map(|i| i.to_le_bytes())
        .collect();
    let mut image = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut image, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[200, 200, 200, 255, 200, 200, 200, 0])
            .unwrap();
    }
    let mut binary = Vec::new();
    let mut views = Vec::new();
    for part in [&positions, &normals, &uvs, &indices, &image] {
        while !binary.len().is_multiple_of(4) {
            binary.push(0);
        }
        views.push(json!({ "buffer": 0, "byteOffset": binary.len(), "byteLength": part.len() }));
        binary.extend_from_slice(part);
    }
    let texture = json!({ "baseColorTexture": { "index": 0 }, "metallicFactor": 0.0 });
    let material = if mask {
        json!({ "alphaMode": "MASK", "alphaCutoff": 0.5, "doubleSided": true, "pbrMetallicRoughness": texture })
    } else {
        json!({ "doubleSided": true, "pbrMetallicRoughness": texture })
    };
    let root = json!({
        "asset": { "version": "2.0" },
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [-1, -1, 0], "max": [1, 1, 0] },
            { "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3" },
            { "bufferView": 2, "componentType": 5126, "count": 4, "type": "VEC2" },
            { "bufferView": 3, "componentType": 5123, "count": 6, "type": "SCALAR" }
        ],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "indices": 3, "material": 0 }] }],
        "materials": [material],
        "images": [{ "bufferView": 4, "mimeType": "image/png" }],
        "textures": [{ "source": 0 }],
        "nodes": [{ "mesh": 0 }],
        "scenes": [{ "nodes": [0] }],
        "scene": 0,
        "bufferViews": views,
        "buffers": [{ "byteLength": binary.len().div_ceil(4) * 4 }]
    });
    wish_import::load_model_bytes(&glb(&root, &binary), "assets/fence.glb").unwrap()
}

/// Which cascade the view's colour at a pixel says: 0 to 3 (red, green,
/// blue, yellow), 4 for the single map (white), 5 for no shadow (grey);
/// `None` where two cascades blend or nothing is drawn.
fn cascade_of(pixel: [u8; 4]) -> Option<usize> {
    let [r, g, b] = [pixel[0], pixel[1], pixel[2]].map(f32::from);
    let brightest = r.max(g).max(b);
    if brightest < 20.0 {
        return None;
    }
    // The view's colours, whatever their shade.
    let colours = [
        [0.90, 0.22, 0.18],
        [0.20, 0.72, 0.26],
        [0.22, 0.42, 0.95],
        [0.95, 0.80, 0.15],
        [1.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
    ];
    let mut best = None;
    for (index, colour) in colours.iter().enumerate() {
        // The shade that fits best, and how far off the colour is with it.
        let scale = (r * colour[0] + g * colour[1] + b * colour[2])
            / (colour[0] * colour[0] + colour[1] * colour[1] + colour[2] * colour[2]);
        let off = (r - scale * colour[0]).abs()
            + (g - scale * colour[1]).abs()
            + (b - scale * colour[2]).abs();
        // White and grey are one hue, told apart by how bright they are:
        // grey is 89 at its brightest, white 153 at its most shaded.
        let fits = match index {
            4 => scale > 120.0,
            5 => scale <= 120.0,
            _ => true,
        };
        if fits && off < 12.0 && best.is_none_or(|(_, least)| off < least) {
            best = Some((index, off));
        }
    }
    best.map(|(index, _)| index)
}

#[test]
fn a_masked_caster_casts_its_cut_out_shape_in_a_near_and_in_a_far_cascade() {
    let gpu = gpu();
    // Two fence panels stand across the view, one near and one far, each
    // with a solid left half and a clear right half. The sun shines from
    // behind them towards the camera, so each panel's shadow lies on the
    // ground in front of it, 2 m long.
    let entities = json!({
        "camera": camera([0.0, 6.0, 4.0], Vec3::new(0.0, -0.45, -1.0)),
        "ground": ground(400.0),
        "near": { "components": { "Model": { "asset": "assets/fence.glb" },
            "Transform": { "translation": [0, 1, -4] } } },
        "far": { "components": { "Model": { "asset": "assets/fence.glb" },
            "Transform": { "translation": [0, 1, -20] } } },
        "sun": sun(Vec3::new(0.0, -1.0, 1.0)),
    });
    let with = prepare(&scene(cascades(4, 40.0, 512), entities.clone()));
    let without = prepare(&scene(json!({ "shadows": false }), entities));
    let draw_with = |mask: bool, scene: &RenderScene| {
        let mut renderer = Renderer::new(&gpu).unwrap();
        renderer
            .upload_model(&gpu, "assets/fence.glb", &fence(mask))
            .unwrap();
        (draw(&renderer, &gpu, scene), view(&renderer, &gpu, scene))
    };
    let (masked, cascades_view) = draw_with(true, &with);
    let (solid, _) = draw_with(false, &with);
    let (open, _) = draw_with(true, &without);
    keep("cascades_fences", &masked.image);
    keep("cascades_fences_view", &cascades_view.image);
    // Each panel is drawn into the cascades it shadows by the masked
    // pipeline; the two shadows are read from different cascades.
    let mut used = Vec::new();
    for z in [-4.0f32, -20.0] {
        // One meter in front of the panel: under its solid half, under its
        // cut-out half, and beside it.
        let under_solid = Vec3::new(-0.5, 0.0, z + 1.0);
        let under_hole = Vec3::new(0.5, 0.0, z + 1.0);
        let lit = around(&open, &without, under_solid);
        let shade = around(&masked, &with, under_solid);
        let hole = around(&masked, &with, under_hole);
        assert!(
            shadowed(shade, lit),
            "z = {z}, under the solid half: {shade} against {lit}"
        );
        assert!(
            (hole - around(&open, &without, under_hole)).abs() <= 6.0,
            "z = {z}, under the cut-out half the ground is lit: {hole} against {lit}"
        );
        // Without the mask the whole panel casts, as it always did.
        for point in [under_solid, under_hole] {
            let shade = around(&solid, &with, point);
            assert!(
                shadowed(shade, lit),
                "z = {z}, {point:?}: {shade} against {lit}"
            );
        }
        used.push(cascade_of(pixel_at(&cascades_view, &with, under_solid)));
    }
    println!("the near and the far panel's shadows are read from cascades {used:?}");
    let (near, far) = (used[0].expect("one cascade"), used[1].expect("one cascade"));
    assert!(near < far && far <= 3, "{used:?}");
}

#[test]
fn cascades_work_under_an_orthographic_camera() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // The block from straight above, 12 m of ground from top to bottom of
    // the picture, on the ground 400 m wide.
    let entities = |settings: Value| {
        scene(
            settings,
            json!({
                "camera": { "components": {
                    "Camera": { "projection": "Orthographic", "ortho_height": 12 },
                    "Transform": { "translation": [0, 20, 0], "rotation": facing(Vec3::NEG_Y) } } },
                "block": cube([-2.0, 1.0, 0.0], [2.0, 2.0, 2.0]),
                "ground": ground(400.0),
                "sun": sun(SUN),
            }),
        )
    };
    let with = prepare(&entities(cascades(4, 30.0, 256)));
    assert!(with.cameras[0].ortho_height.is_some());
    let one = prepare(&entities(json!({ "shadow_map_size": 256 })));
    let without = prepare(&entities(json!({ "shadows": false })));
    let frame = draw(&renderer, &gpu, &with);
    let single = draw(&renderer, &gpu, &one);
    let open = draw(&renderer, &gpu, &without);
    keep("cascades_ortho", &frame.image);
    assert_eq!(frame.stats.shadow_cascades, 4);
    let inside = Vec3::new(0.0, 0.0, 0.0);
    let lit = around(&open, &without, inside);
    let dark = around(&frame, &with, inside);
    assert!(shadowed(dark, lit), "in shadow: {dark} against {lit}");
    // The shadow's four sides are where the block puts them: lit just
    // outside, dark just inside.
    for (out, into) in [
        (Vec3::new(1.3, 0.0, 0.0), Vec3::new(0.7, 0.0, 0.0)),
        (Vec3::new(0.0, 0.0, 1.3), Vec3::new(0.0, 0.0, 0.7)),
        (Vec3::new(0.0, 0.0, -1.3), Vec3::new(0.0, 0.0, -0.7)),
    ] {
        let (outside, within) = (around(&frame, &with, out), around(&frame, &with, into));
        assert!(
            (outside - lit).abs() <= 6.0,
            "{out:?}: {outside} against {lit}"
        );
        assert!(shadowed(within, lit), "{into:?}: {within} against {lit}");
    }
    // One map of the same size has lost the shadow in this scene.
    let lost = around(&single, &one, inside);
    println!(
        "under an orthographic camera: {dark} in the shadow with cascades, {lost} with one map of 256, {lit} without shadows"
    );
    assert!(lost > lit - 30.0, "{lost} against {lit}");
    // The view says which map each surface uses, here too.
    let colours = view(&renderer, &gpu, &with);
    assert!(
        matches!(cascade_of(pixel_at(&colours, &with, inside)), Some(0..=3)),
        "{:?}",
        pixel_at(&colours, &with, inside)
    );
}

#[test]
fn a_camera_inside_a_small_scene_has_its_shadows_with_cascades() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // The foundation's scene: a ground of 12 m. Cascades up to 300 m: the
    // first slice alone is deeper than the scene is wide, the others hold
    // nothing.
    let with = prepare(&block_scene(12.0, cascades(4, 300.0, 1024)));
    let one = prepare(&block_scene(12.0, Value::Null));
    let frame = draw(&renderer, &gpu, &with);
    let single = draw(&renderer, &gpu, &one);
    assert_eq!(frame.stats.shadow_cascades, 4);
    // Every cascade's square holds the scene, so each draws both surfaces.
    assert_eq!(frame.stats.shadow_casters, 8);
    for point in [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.8, 0.0, 0.8),
        Vec3::new(-0.8, 0.0, -0.8),
        Vec3::new(3.0, 0.0, 0.0),
        Vec3::new(1.4, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 2.0),
    ] {
        let (here, there) = (around(&frame, &with, point), around(&single, &one, point));
        assert!(
            (here - there).abs() <= 6.0,
            "{point:?}: {here} with cascades, {there} with the one map"
        );
    }
    let dark = around(&frame, &with, Vec3::new(0.0, 0.0, 0.0));
    let lit = around(&frame, &with, Vec3::new(3.0, 0.0, 0.0));
    assert!(shadowed(dark, lit), "{dark} against {lit}");
}

#[test]
fn settings_out_of_range_are_held_and_bad_numbers_fall_back() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let image = |scene: &RenderScene| {
        renderer
            .render(&gpu, scene, None, SIZE.0, SIZE.1)
            .unwrap()
            .image
    };
    // Game code can write any number into the resource.
    let four = prepare(&block_scene(400.0, cascades(4, 100.0, 256)));
    let one = prepare(&block_scene(400.0, cascades(1, 100.0, 256)));
    let four_image = image(&four);
    let one_image = image(&one);
    assert_ne!(four_image, one_image);
    for (count, drawn) in [
        (0, &one_image),
        (5, &four_image),
        (99, &four_image),
        (u32::MAX, &four_image),
    ] {
        let mut scene = four.clone();
        scene.settings.shadow_cascades = count;
        assert!(image(&scene) == *drawn, "{count} cascades");
    }
    // A distance that is not a number is the default's 100 m; either
    // infinity too.
    for distance in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut scene = four.clone();
        scene.settings.shadow_distance = distance;
        assert!(image(&scene) == four_image, "a distance of {distance}");
    }
    // Too long is the longest (never beyond the camera's far plane, 1000 m
    // here), too short the shortest.
    let longest = prepare(&block_scene(400.0, cascades(4, 10000.0, 256)));
    let mut scene = four.clone();
    scene.settings.shadow_distance = 1.0e12;
    assert!(image(&scene) == image(&longest));
    let shortest = prepare(&block_scene(400.0, cascades(4, 0.1, 256)));
    let shortest_frame = renderer
        .render(&gpu, &shortest, None, SIZE.0, SIZE.1)
        .unwrap();
    for distance in [0.0, -5.0, f32::MIN_POSITIVE] {
        let mut scene = four.clone();
        scene.settings.shadow_distance = distance;
        assert!(
            image(&scene) == shortest_frame.image,
            "a distance of {distance}"
        );
    }
    // A shadow map smaller or larger than can be drawn, with cascades.
    let mut tiny = four.clone();
    tiny.settings.shadow_map_size = 0;
    let smallest = prepare(&block_scene(400.0, cascades(4, 100.0, 64)));
    assert!(image(&tiny) == image(&smallest));
}

#[test]
fn nothing_in_range_is_no_shadow_and_a_warning_that_says_what_to_do() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // The block on a ground of 4 m, ten meters from the camera; shadows up
    // to 2 m. (The ground is small so that it does not reach under the
    // camera: a ground that does is within reach of the cascades.)
    let short = prepare(&block_scene(4.0, cascades(3, 2.0, 256)));
    let without = prepare(&block_scene(4.0, json!({ "shadows": false })));
    let frame = renderer.render(&gpu, &short, None, SIZE.0, SIZE.1).unwrap();
    let open = draw(&renderer, &gpu, &without);
    assert_eq!(frame.image, open.image, "no shadow is drawn");
    assert_eq!(frame.stats.shadow_cascades, 0);
    assert_eq!(frame.stats.shadow_casters, 0);
    let [warning] = frame.warnings.as_slice() else {
        panic!("one warning: {:?}", frame.warnings);
    };
    assert_eq!(warning.code, "shadows_out_of_range");
    // In the measure the distance uses: depth in front of the camera (the
    // block's middle is 9.2 m deep in this view).
    assert!(
        warning.message.contains("shadow_cascades is 3")
            && warning.message.contains("(2 m) in front of the camera")
            && warning
                .message
                .contains("the nearest is about 9 m in front of it"),
        "{}",
        warning.message
    );
    assert!(
        warning.hint.contains("shadow_distance") && warning.hint.contains("shadow_cascades to 1"),
        "{}",
        warning.hint
    );
    // `describe` and the cascades view say the same.
    let description = renderer
        .describe(&gpu, &short, None, SIZE.0, SIZE.1)
        .unwrap();
    assert_eq!(description.warnings, frame.warnings);
    let colours = view(&renderer, &gpu, &short);
    assert_eq!(colours.warnings, frame.warnings);
    assert_eq!(colours.stats.shadow_cascades, 0, "it shows no map");
    assert_eq!(
        cascade_of(pixel_at(&colours, &short, Vec3::new(1.5, 0.0, 1.0))),
        Some(5),
        "grey: no shadow"
    );
    // The other views do not: it is about shadows.
    let depth = renderer
        .render_view(&gpu, &short, None, SIZE.0, SIZE.1, View::Depth)
        .unwrap();
    assert!(depth.warnings.is_empty(), "{:?}", depth.warnings);
    // Following the hint mends it.
    let mut longer = short.clone();
    longer.settings.shadow_distance = 30.0;
    let mended = draw(&renderer, &gpu, &longer);
    assert_eq!(mended.stats.shadow_cascades, 3);
    let mut one = short.clone();
    one.settings.shadow_cascades = 1;
    assert_eq!(draw(&renderer, &gpu, &one).stats.shadow_cascades, 1);
}

#[test]
fn the_view_shows_which_cascade_each_surface_uses() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let with = prepare(&wall_scene(cascades(4, 40.0, 256)));
    let colours = view(&renderer, &gpu, &with);
    keep("view_shadow_cascades", &colours.image);
    assert!(colours.warnings.is_empty(), "{:?}", colours.warnings);
    // It draws the surfaces once and no shadow map, and says how many maps
    // it shows: the number the lit picture of this view reports.
    assert_eq!(colours.stats.draw_calls, 2);
    assert_eq!(colours.stats.shadow_casters, 0);
    assert_eq!(colours.stats.shadow_cascades, 4);
    assert_eq!(colours.drawn, 2);
    // The other debug views show no shadow map: 0.
    let depth = renderer
        .render_view(&gpu, &with, None, SIZE.0, SIZE.1, View::Depth)
        .unwrap();
    assert_eq!(depth.stats.shadow_cascades, 0);
    // Along the ground, away from the camera: red, green, blue, yellow,
    // then grey beyond the shadow distance; each is there, in that order.
    let mut seen = Vec::new();
    let mut depth = 3.0;
    while depth <= 80.0 {
        let point = Vec3::new(0.5, 0.0, -depth);
        if let Some(cascade) = cascade_of(pixel_at(&colours, &with, point))
            && seen.last() != Some(&cascade)
        {
            seen.push(cascade);
        }
        depth += 0.25;
    }
    assert_eq!(seen, vec![0, 1, 2, 3, 5], "red, green, blue, yellow, grey");
    // The sky is not a surface.
    assert_eq!(colours.image.pixel(5, 5), Some([0, 0, 0, 255]));

    // The view says what the lit picture does: where it shows a cascade the
    // strip is in shadow, where it is grey (beyond the distance) it is lit.
    let frame = draw(&renderer, &gpu, &with);
    let without = prepare(&wall_scene(json!({ "shadows": false })));
    let open = draw(&renderer, &gpu, &without);
    for (depth, shadow) in [(6.0f32, true), (20.0, true), (50.0, false)] {
        let point = Vec3::new(0.0, 0.0, -depth);
        let (here, lit) = (
            at_point(&frame, &with, point),
            at_point(&open, &without, point),
        );
        let cascade = cascade_of(pixel_at(&colours, &with, point));
        if shadow {
            assert!(matches!(cascade, Some(0..=3)), "{depth} m: {cascade:?}");
            assert!(shadowed(here, lit), "{depth} m: {here} against {lit}");
        } else {
            assert_eq!(cascade, Some(5), "{depth} m");
            assert!((here - lit).abs() <= 3.0, "{depth} m: {here} against {lit}");
        }
    }

    // With one map every surface is white; without shadows, grey.
    let one = prepare(&wall_scene(Value::Null));
    let none = prepare(&wall_scene(json!({ "shadows": false })));
    assert_eq!(view(&renderer, &gpu, &one).stats.shadow_cascades, 1);
    assert_eq!(view(&renderer, &gpu, &none).stats.shadow_cascades, 0);
    for depth in [6.0f32, 20.0, 50.0] {
        let point = Vec3::new(0.5, 0.0, -depth);
        assert_eq!(
            cascade_of(pixel_at(&view(&renderer, &gpu, &one), &one, point)),
            Some(4),
            "{depth} m, one map"
        );
        assert_eq!(
            cascade_of(pixel_at(&view(&renderer, &gpu, &none), &none, point)),
            Some(5),
            "{depth} m, no shadows"
        );
    }
    // Two cascades: red, green, grey.
    let two = prepare(&wall_scene(cascades(2, 40.0, 256)));
    let colours = view(&renderer, &gpu, &two);
    let mut seen = Vec::new();
    let mut depth = 3.0;
    while depth <= 80.0 {
        let point = Vec3::new(0.5, 0.0, -depth);
        if let Some(cascade) = cascade_of(pixel_at(&colours, &two, point))
            && seen.last() != Some(&cascade)
        {
            seen.push(cascade);
        }
        depth += 0.25;
    }
    assert_eq!(seen, vec![0, 1, 5]);
}

#[test]
fn the_statistics_count_the_shadow_maps_and_what_is_drawn_into_them() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // The wall scene and a crate 150 m behind the camera: three surfaces.
    let entities = |settings: Value| {
        scene(
            settings,
            json!({
                "camera": camera(EYE, EYE_LOOKS),
                "crate": cube([0.0, 0.5, 150.0], [1.0, 1.0, 1.0]),
                "ground": ground(400.0),
                "wall": cube([-1.1, 1.0, -30.0], [0.2, 2.0, 60.0]),
                "sun": sun(SUN),
            }),
        )
    };
    let none = draw(
        &renderer,
        &gpu,
        &prepare(&entities(json!({ "shadows": false }))),
    );
    let one = draw(&renderer, &gpu, &prepare(&entities(Value::Null)));
    let two = draw(&renderer, &gpu, &prepare(&entities(cascades(2, 40.0, 256))));
    let four = draw(&renderer, &gpu, &prepare(&entities(cascades(4, 40.0, 256))));
    assert_eq!(
        [&none, &one, &two, &four].map(|frame| frame.stats.shadow_cascades),
        [0, 1, 2, 4]
    );
    // One map holds every surface. A cascade holds what can shadow it: the
    // ground and the wall, never the crate.
    assert_eq!(
        [&none, &one, &two, &four].map(|frame| frame.stats.shadow_casters),
        [0, 3, 4, 8]
    );
    // The draw calls are the surfaces, the shadow draws, the sky and tone
    // mapping.
    for frame in [&none, &one, &two, &four] {
        assert_eq!(frame.stats.draw_calls, 3 + frame.stats.shadow_casters + 2);
    }
    // 12 triangles a cube, 2 the ground.
    assert_eq!(four.stats.triangles, 26 + 4 * 14 + 2);
    // `describe` gives the lit picture's.
    let scene = prepare(&entities(cascades(4, 40.0, 256)));
    let description = renderer
        .describe(&gpu, &scene, None, SIZE.0, SIZE.1)
        .unwrap();
    assert_eq!(description.stats, four.stats);
}

#[test]
fn a_second_frame_with_cascades_creates_nothing() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let plain = prepare(&block_scene(400.0, json!({ "shadow_map_size": 256 })));
    let with = prepare(&block_scene(400.0, cascades(4, 40.0, 256)));
    draw(&renderer, &gpu, &plain);
    let made_plain = gpu.created();
    let first = draw(&renderer, &gpu, &with);
    let made = gpu.created();
    // The atlas, and a longer buffer for the four matrices of the light.
    assert_eq!(
        (
            made.textures - made_plain.textures,
            made.buffers - made_plain.buffers
        ),
        (1, 1)
    );
    let again = draw(&renderer, &gpu, &with);
    assert_eq!(gpu.created(), made, "a second frame creates nothing");
    assert_eq!(again.image, first.image);
    // Other settings of the same shape are other numbers: nothing new.
    let other = prepare(&block_scene(400.0, cascades(3, 25.0, 256)));
    draw(&renderer, &gpu, &other);
    assert_eq!(gpu.created(), made);
    // Back to one map: its texture was kept beside the atlas.
    draw(&renderer, &gpu, &plain);
    assert_eq!(gpu.created(), made);
    // The view of the cascades makes its own picture, once.
    view(&renderer, &gpu, &with);
    let with_view = gpu.created();
    view(&renderer, &gpu, &with);
    assert_eq!(gpu.created(), with_view);
}

/// One border of the first cascade, read from a line of pixels of the
/// `shadow_cascades` view that starts outside the cascade and ends well
/// inside it.
#[derive(Debug)]
struct Border {
    /// Pixels that show a mix of the two cascades.
    mixed: usize,
    /// The largest change of the first cascade's share from one pixel to
    /// the next.
    jump: f32,
    /// Over how many pixels the share goes from 0 to 1 (a line fitted
    /// through its middle part); 0 when there is no ramp to fit.
    width: f32,
    /// Where on the line, in pixels, that fitted line is at 0: where the
    /// cascade starts to count.
    start: f32,
}

fn border(shares: &[f32]) -> Border {
    let mixed = shares.iter().filter(|w| **w > 0.1 && **w < 0.9).count();
    let jump = shares
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max);
    let points: Vec<(f32, f32)> = shares
        .iter()
        .enumerate()
        .filter(|(_, w)| (0.15..=0.85).contains(*w))
        .map(|(at, w)| (at as f32, *w))
        .collect();
    let n = points.len() as f32;
    let (sx, sy) = points
        .iter()
        .fold((0.0, 0.0), |(sx, sy), (x, y)| (sx + x, sy + y));
    let (sxx, sxy) = points
        .iter()
        .fold((0.0, 0.0), |(sxx, sxy), (x, y)| (sxx + x * x, sxy + x * y));
    let spread = n * sxx - sx * sx;
    if points.len() < 3 || spread <= 0.0 {
        return Border {
            mixed,
            jump,
            width: 0.0,
            start: 0.0,
        };
    }
    let slope = (n * sxy - sx * sy) / spread;
    let level = (sy - slope * sx) / n;
    Border {
        mixed,
        jump,
        width: 1.0 / slope,
        start: -level / slope,
    }
}

/// The first cascade's four borders in a picture taken straight down, and
/// how wide the cascade is from its left start to its right start, in
/// pixels.
#[derive(Debug)]
struct Borders {
    left: Border,
    right: Border,
    top: Border,
    bottom: Border,
    across: f32,
}

const BAND_PICTURE: u32 = 256;

/// Looks straight down from 5.5 m on bare ground through cascades of 64
/// texels, with a lens wide enough to see all of the first cascade's map,
/// and reads the first cascade's share along the picture's middle row and
/// middle column. The sun shines along -Z and down, so the row crosses the
/// map's left and right borders (the blend band, twice) and the column its
/// far side (the fade in depth) and its lower border (the band).
fn borders_of(renderer: &Renderer, gpu: &Gpu) -> Borders {
    let source = scene(
        cascades(4, 40.0, 64),
        json!({
            "camera": { "components": {
                "Camera": { "fov_y_deg": 100 },
                "Transform": { "translation": [0, 5.5, 0], "rotation": facing(Vec3::NEG_Y) } } },
            "ground": ground(400.0),
            "sun": sun(Vec3::new(0.0, -1.0, -1.0)),
        }),
    );
    let scene = prepare(&source);
    let side = BAND_PICTURE;
    let colours = renderer
        .render_view(gpu, &scene, None, side, side, View::ShadowCascades)
        .unwrap();
    let pixel = |x: u32, y: u32| colours.image.pixel(x, y).expect("in the picture");
    let middle = side / 2;
    // The first cascade's red in the middle, the second's green at the ends
    // of both lines.
    assert_eq!(cascade_of(pixel(middle, middle)), Some(0));
    for (x, y) in [
        (1, middle),
        (side - 2, middle),
        (middle, 1),
        (middle, side - 2),
    ] {
        assert_eq!(cascade_of(pixel(x, y)), Some(1), "({x}, {y})");
    }
    // The first cascade's share of a pixel, from its red (the two colours
    // differ most there).
    let (red, green) = (
        f32::from(pixel(middle, middle)[0]),
        f32::from(pixel(1, middle)[0]),
    );
    let share = |x: u32, y: u32| (f32::from(pixel(x, y)[0]) - green) / (red - green);
    let row: Vec<f32> = (0..side).map(|x| share(x, middle)).collect();
    let column: Vec<f32> = (0..side).map(|y| share(middle, y)).collect();
    let half = middle as usize;
    let inwards = |line: &[f32]| -> (Border, Border) {
        let mut back: Vec<f32> = line[half..].to_vec();
        back.reverse();
        (border(&line[..half]), border(&back))
    };
    let (left, right) = inwards(&row);
    let (top, bottom) = inwards(&column);
    let across = (side as f32 - 1.0 - right.start) - left.start;
    Borders {
        left,
        right,
        top,
        bottom,
        across,
    }
}

/// Whether the borders are what the cascades promise: a band at each, with
/// no jump; the bands as wide as they are meant to be against the map's
/// size; and the far side's fade as wide as the band on the ground.
fn judge(borders: &Borders) -> Result<(), String> {
    for (name, side) in [
        ("left", &borders.left),
        ("right", &borders.right),
        ("top", &borders.top),
        ("bottom", &borders.bottom),
    ] {
        if side.mixed < 5 || side.jump > 0.3 || side.width <= 0.0 {
            return Err(format!(
                "no band at the {name} border: {} mixed pixel(s), a jump of {:.2}",
                side.mixed, side.jump
            ));
        }
    }
    // A map of 64 texels counts from 2 texels inside its border (the
    // filter's footprint), over a band of 4% of 64 = 2.56 texels. So from
    // where it starts to count on the left to where it does on the right
    // are 60 texels, 23.4 bands. (Without the 2 texels: 64, 25 bands.)
    let band = (borders.left.width + borders.right.width) / 2.0;
    let bands = borders.across / band;
    let expected = (64.0 - 2.0 * 2.0) / (64.0 * 0.04);
    if (bands / expected - 1.0).abs() > 0.035 {
        return Err(format!(
            "the map is {bands:.2} bands wide where it counts, not {expected:.2}"
        ));
    }
    // Up and down the picture the sun's slant stretches the map by the
    // square root of two; the band at the lower border and the fade at the
    // far side are as wide as each other there.
    for (name, side) in [("top", &borders.top), ("bottom", &borders.bottom)] {
        let stretch = side.width / band;
        if (stretch / std::f32::consts::SQRT_2 - 1.0).abs() > 0.12 {
            return Err(format!(
                "the {name} border's band is {stretch:.2} times the side borders', not 1.41"
            ));
        }
    }
    Ok(())
}

/// A renderer whose `cascades.wgsl` has `from` replaced by `to`: the
/// engine's own shader with one thing taken out.
fn renderer_without(gpu: &Gpu, from: &str, to: &str) -> Renderer {
    let source = include_str!("../src/shaders/cascades.wgsl");
    assert_eq!(
        source.matches(from).count(),
        1,
        "cascades.wgsl no longer has `{from}`: bring this test's replacement in line with it"
    );
    let mut library = wish_render::engine_shaders();
    library.add(
        "cascades",
        "wish_render/shaders/cascades.wgsl",
        &source.replace(from, to),
    );
    Renderer::with_shaders(gpu, &library).unwrap()
}

#[test]
fn a_cascade_hands_over_across_a_band_at_its_borders_and_at_its_far_side() {
    let gpu = gpu();
    // The engine's shaders: a band at all four borders, of the right width.
    let real = borders_of(&Renderer::new(&gpu).unwrap(), &gpu);
    println!("the engine's shader: {real:?}");
    judge(&real).unwrap_or_else(|problem| panic!("{problem}: {real:?}"));
    // Measured: bands of 9.2 pixels left and right and 13.0 at the top and
    // the bottom, at most 0.11 and 0.08 from one pixel to the next; the map
    // 23.5 bands wide (without the two texels at its border: 24.9).

    // The same measurement on the engine's shader with one thing taken out
    // must not pass: that is what says this test would notice.
    // 1. A hard switch at the map's border in place of the blend band.
    let hard = borders_of(
        &renderer_without(
            &gpu,
            "let across = clamp((border - 2.0) * info.z, 0.0, 1.0);",
            "let across = select(0.0, 1.0, border >= 2.0);",
        ),
        &gpu,
    );
    println!("without the blend band: {hard:?}");
    let problem = judge(&hard).expect_err("a hard switch at the border passes");
    assert!(problem.contains("no band at the left border"), "{problem}");
    // The far side still fades, though: it is its own piece of the shader.
    assert!(hard.top.mixed >= 5 && hard.top.jump <= 0.3, "{hard:?}");

    // 2. A hard switch at the map's far side in place of the fade in depth.
    let cut = borders_of(
        &renderer_without(
            &gpu,
            "let along = clamp((1.0 - clip.z) * bias.z, 0.0, 1.0);",
            "let along = select(0.0, 1.0, clip.z <= 1.0);",
        ),
        &gpu,
    );
    println!("without the fade at the far side: {cut:?}");
    let problem = judge(&cut).expect_err("a hard switch at the far side passes");
    assert!(problem.contains("no band at the top border"), "{problem}");

    // 3. The band starting at the map's very border, without the two texels
    //    that keep the filter's footprint inside the map.
    let unguarded = borders_of(
        &renderer_without(&gpu, "(border - 2.0) * info.z", "border * info.z"),
        &gpu,
    );
    println!("without the two texels at the border: {unguarded:?}");
    let problem = judge(&unguarded).expect_err("a band at the very border passes");
    assert!(problem.contains("bands wide"), "{problem}");
}

/// What cascades cost, by the clock: run by hand, in release (`cargo test
/// --release -p wish_render --test cascades what_cascades_cost -- --ignored
/// --nocapture`; `WISH_SOFTWARE=1` for the software adapter). The time of a
/// frame, drawn and read back, with one shadow map and with two and four
/// cascades at the default map size, in two scenes of 401 surfaces: posts
/// spread over a ground 240 m wide (most of them beyond the shadow
/// distance, so the cascades draw fewer casters than the one map), and the
/// same posts packed into 50 m around the camera (every cascade draws most
/// of them). Nothing is asserted: the numbers depend on the machine.
#[test]
#[ignore = "a measurement, run by hand in release"]
fn what_cascades_cost() {
    let software = std::env::var_os("WISH_SOFTWARE").is_some();
    let gpu = Gpu::new(GpuOptions { software }).expect("an adapter");
    eprintln!("adapter: {:?}", gpu.adapter());
    let renderer = Renderer::new(&gpu).unwrap();
    let entities = |spacing: f32, settings: Value| {
        let mut entities = serde_json::Map::new();
        entities.insert("camera".to_owned(), camera(EYE, EYE_LOOKS));
        entities.insert("ground".to_owned(), ground(spacing * 20.0));
        entities.insert("sun".to_owned(), sun(Vec3::new(1.0, -1.0, 0.4)));
        for row in 0..20 {
            for column in 0..20 {
                let x = (column as f32 - 9.5) * spacing;
                let z = (row as f32 - 9.5) * spacing;
                entities.insert(
                    format!("post_{row}_{column}"),
                    cube([x, 1.5, z], [0.5, 3.0, 0.5]),
                );
            }
        }
        prepare(&scene(settings, Value::Object(entities)))
    };
    for (place, spacing) in [("spread over 240 m", 12.0), ("packed into 50 m", 2.5)] {
        let scenes = [
            ("one map", entities(spacing, Value::Null)),
            (
                "two cascades",
                entities(spacing, json!({ "shadow_cascades": 2 })),
            ),
            (
                "four cascades",
                entities(spacing, json!({ "shadow_cascades": 4 })),
            ),
        ];
        for (width, height) in [(1280u32, 720u32), (1920, 1080)] {
            for (name, scene) in &scenes {
                // A few frames first, so that the targets exist.
                let mut stats = None;
                for _ in 0..3 {
                    let frame = renderer.render(&gpu, scene, None, width, height).unwrap();
                    stats = Some(frame.stats);
                }
                let frames = if software { 10 } else { 30 };
                let start = std::time::Instant::now();
                for _ in 0..frames {
                    renderer.render(&gpu, scene, None, width, height).unwrap();
                }
                let time = start.elapsed().as_secs_f64() * 1000.0 / f64::from(frames);
                let stats = stats.expect("a frame");
                eprintln!(
                    "{place}, {width}x{height}, {name}: {time:.2} ms a frame ({} draw calls, {} of them into {} shadow map(s))",
                    stats.draw_calls, stats.shadow_casters, stats.shadow_cascades
                );
            }
        }
    }
}
