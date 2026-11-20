//! Bloom (design note M12-look-2-bloom), on the software adapter.
//!
//! Bloom is off by default, and the first test is the rule the package
//! stands on: a scene that does not ask for it is drawn as it always was.
//! Every other test turns it on and measures what it should change and what
//! it should not. Pictures drawn by the same passes are compared byte for
//! byte; pictures drawn by different pipelines (with bloom and without) are
//! allowed 1/255. `UPDATE_GOLDEN=1` rewrites the golden images.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_core::math::Vec3;
use wish_ecs::World;
use wish_render::{
    Effects, Frame, Gpu, GpuOptions, RenderScene, Renderer, RgbaImage, Tolerance, extract,
    project_to_pixel,
};
use wish_scene::load;

const TOLERANCE: Tolerance = Tolerance {
    channel: 2,
    fraction: 0.005,
};

/// A sun 14 degrees above the horizon, ahead of a camera that looks along
/// -Z and 10 degrees to its right: it is in the picture.
const SUN_AHEAD: [f64; 4] = [-0.010_621_6, 0.988_769_3, 0.121_405_6, 0.086_506_1];
/// A sun from behind the camera's left shoulder.
const SUN_BEHIND: [f64; 4] = [-0.454_519_5, 0.330_454_3, 0.178_659_7, 0.806_704_6];

/// How bright an emissive value of 1 is in the picture, in multiples of a
/// white surface, at the default exposure: 30 000 cd/m² exposed for EV 14.
const WHITES_PER_EMISSIVE: f64 = 30000.0 / (1.2 * 16384.0);

fn scene(resources: Value, entities: Value) -> String {
    json!({ "format": 1, "resources": resources, "entities": entities }).to_string()
}

fn world_of(source: &str) -> World {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    wish_ui::register(&mut world).unwrap();
    let result = load(source, "scenes/bloom.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap().instantiate(&mut world).unwrap();
    wish_transform::update_global_transforms(&mut world).unwrap();
    wish_ui::layout(&mut world).unwrap();
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

fn check_golden(name: &str, image: &RgbaImage) {
    keep(name, image);
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

/// Where `point` is in the picture, in whole pixels.
fn pixel_of(frame: &Frame, scene: &RenderScene, point: Vec3) -> (u32, u32) {
    let camera = &scene.cameras[0];
    let (width, height) = (frame.image.width, frame.image.height);
    let view_proj = camera.projection(width as f32 / height as f32) * camera.view;
    let (x, y) = project_to_pixel(&view_proj, point, width, height).expect("in front");
    (x as u32, y as u32)
}

/// A pixel's red, green and blue as light: the picture's sRGB encoding
/// undone. Sums, differences and ratios of light are only true here.
fn linear(pixel: [u8; 4]) -> [f32; 3] {
    [0, 1, 2].map(|i| {
        let encoded = f32::from(pixel[i]) / 255.0;
        if encoded <= 0.04045 {
            encoded / 12.92
        } else {
            ((encoded + 0.055) / 1.055).powf(2.4)
        }
    })
}

fn light_at(frame: &Frame, (x, y): (u32, u32)) -> [f32; 3] {
    linear(frame.image.pixel(x, y).expect("on screen"))
}

/// How much more light `with` has at a pixel than `without`, per channel.
fn lift(with: &Frame, without: &Frame, at: (u32, u32)) -> [f32; 3] {
    let (a, b) = (light_at(with, at), light_at(without, at));
    [0, 1, 2].map(|i| a[i] - b[i])
}

/// Whether a pixel is as bright as the picture can show, in every channel.
fn white_hot(pixel: [u8; 4]) -> bool {
    pixel[..3].iter().all(|channel| *channel >= 250)
}

fn total(light: [f32; 3]) -> f32 {
    light[0] + light[1] + light[2]
}

/// The mean lift, all channels together, over the square of pixels that
/// reaches `reach` pixels each way from `at`. Where the glow is faint one
/// pixel's lift is a few steps of 255, and the one step of rounding that
/// adapters may differ by would move a ratio of two such lifts; the mean
/// over many pixels does not move with it.
fn lift_around(with: &Frame, without: &Frame, at: (u32, u32), reach: u32) -> f32 {
    let mut sum = 0.0;
    for y in at.1 - reach..=at.1 + reach {
        for x in at.0 - reach..=at.0 + reach {
            sum += total(lift(with, without, (x, y)));
        }
    }
    let side = 2 * reach + 1;
    sum / (side * side) as f32
}

/// The largest difference of a channel between two pictures of one size,
/// anywhere.
fn furthest_apart(a: &RgbaImage, b: &RgbaImage) -> u8 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    a.pixels
        .iter()
        .zip(&b.pixels)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

/// The mean light of a picture, all channels together.
fn mean_light(image: &RgbaImage) -> f64 {
    let (pixels, _) = image.pixels.as_chunks::<4>();
    let sum: f64 = pixels
        .iter()
        .map(|pixel| f64::from(total(linear(*pixel))))
        .sum();
    sum / pixels.len() as f64
}

// ---------------------------------------------------------------------------
// A light in the dark: a small square panel that gives off light, in front
// of a flat dark sky, with no other light in the scene. Whatever is brighter
// than the sky beside the panel is its glow. The sky is dark grey, not
// black: tone mapping bends the darkest colours (its curve has a toe below
// 0.08), and above that and below white it leaves differences of light as
// they are, so what is measured in the picture is what the bloom added.

const DARK: [f64; 4] = [0.1, 0.12, 0.16, 1.0];

fn dark_resources(bloom: Value) -> Value {
    json!({
        "Ambient": { "intensity": 0 },
        "Sky": { "zenith": DARK, "horizon": DARK, "ground": DARK },
        "Bloom": bloom
    })
}

fn camera_entity() -> Value {
    json!({ "components": {
        "Camera": { "fov_y_deg": 45 },
        "Transform": { "translation": [0, 0, 5] } } })
}

/// A flat square that gives off `emissive`, `side` meters wide, at `at`.
fn panel_entity(emissive: [f64; 3], side: f64, at: [f64; 3]) -> Value {
    json!({ "components": {
        "Material": { "base_color": [0, 0, 0, 1], "roughness": 1, "emissive": emissive },
        "Primitive": { "shape": "Cube" },
        "Transform": { "translation": at, "scale": [side, side, 0.05] } } })
}

/// The panel of most tests: 0.6 m wide, which is 29 pixels of a 320x200
/// picture, in the middle of it.
fn panel_scene(emissive: [f64; 3], bloom: Value) -> RenderScene {
    prepare(&scene(
        dark_resources(bloom),
        json!({
            "camera": camera_entity(),
            "panel": panel_entity(emissive, 0.6, [0.0, 0.0, 0.0])
        }),
    ))
}

/// About twenty times white.
const BRIGHT: [f64; 3] = [13.0, 13.0, 13.0];

/// Pixels of the 320x200 picture on the panel's row: its middle, and 7, 27
/// and 47 pixels right of its edge.
const MIDDLE: (u32, u32) = (160, 100);
const NEAR: (u32, u32) = (181, 100);
const MID: (u32, u32) = (201, 100);
const FAR: (u32, u32) = (221, 100);

#[test]
fn a_scene_that_does_not_ask_for_bloom_is_drawn_as_it_always_was() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let draw = |resources: Value| {
        let scene = prepare(&scene(
            resources,
            json!({
                "camera": camera_entity(),
                "panel": panel_entity(BRIGHT, 0.6, [0.0, 0.0, 0.0])
            }),
        ));
        renderer.render(&gpu, &scene, None, 320, 200).unwrap()
    };
    let none = json!({ "Ambient": { "intensity": 0 }, "Sky": { "zenith": DARK, "horizon": DARK, "ground": DARK } });
    let plain = draw(none.clone());
    assert_eq!(plain.stats.effects, Effects::default());
    assert!(plain.stats.effects.names().is_empty());
    assert!(plain.warnings.is_empty(), "{:?}", plain.warnings);

    // The resource present with every field at its default.
    let defaults = draw(dark_resources(json!({})));
    assert_eq!(defaults.image, plain.image);
    assert_eq!(defaults.stats, plain.stats);

    // No strength, and the fields that only matter with one set to
    // something else: still nothing.
    let spelled_out = draw(dark_resources(
        json!({ "strength": 0, "spread": 0.9, "threshold": 3 }),
    ));
    assert_eq!(spelled_out.image, plain.image);
    assert_eq!(spelled_out.stats, plain.stats);

    // With bloom the picture is another one; and a renderer that drew with
    // bloom draws the plain scene the same afterwards.
    let bloomed = draw(dark_resources(json!({ "strength": 0.2 })));
    assert_eq!(bloomed.stats.effects.names(), ["bloom"]);
    assert_ne!(bloomed.image, plain.image);
    let again = draw(none);
    assert_eq!(again.image, plain.image);
    assert_eq!(again.stats, plain.stats);
}

#[test]
fn a_bright_light_lifts_the_pixels_around_it_less_with_distance_and_in_its_colour() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let draw = |emissive: [f64; 3], bloom: Value| {
        renderer
            .render(&gpu, &panel_scene(emissive, bloom), None, 320, 200)
            .unwrap()
    };
    let plain = draw(BRIGHT, json!({}));
    let glow = draw(BRIGHT, json!({ "strength": 0.1 }));
    assert!(glow.stats.effects.bloom && !plain.stats.effects.bloom);
    assert!(glow.warnings.is_empty(), "{:?}", glow.warnings);
    check_golden("bloom_glow", &glow.image);

    // Without bloom the panel ends at its edge: the sky beside it is the
    // sky far from it.
    let sky = plain.image.pixel(5, 5).unwrap();
    for at in [NEAR, MID, FAR] {
        assert_eq!(plain.image.pixel(at.0, at.1).unwrap(), sky, "{at:?}");
    }
    assert!(white_hot(plain.image.pixel(MIDDLE.0, MIDDLE.1).unwrap()));

    // With it the sky around the panel is lifted, the more the nearer.
    let (near, mid, far) = (
        total(lift(&glow, &plain, NEAR)),
        total(lift(&glow, &plain, MID)),
        total(lift(&glow, &plain, FAR)),
    );
    assert!(
        near > mid * 1.5 && mid > far * 1.5 && far > 0.01,
        "the glow 7, 27 and 47 pixels away: {near}, {mid}, {far}"
    );
    assert!(near > 0.3, "a clear glow next to the panel: {near}");
    // The same on every side of it. The panel's middle is the corner
    // between four pixels, so the pixel as far left of it as `MID` is right
    // of it is 319 - x, and the one as far above it is in row 99 - distance.
    let left = total(lift(&glow, &plain, (319 - MID.0, MID.1)));
    let above = total(lift(&glow, &plain, (MIDDLE.0, 99 - (MID.0 - MIDDLE.0))));
    assert!(
        (left / mid - 1.0).abs() < 0.15 && (above / mid - 1.0).abs() < 0.15,
        "right {mid}, left {left}, above {above}"
    );
    // The panel itself is still white-hot.
    let middle = glow.image.pixel(MIDDLE.0, MIDDLE.1).unwrap();
    assert!(white_hot(middle), "{middle:?}");

    // The glow has the light's colour: an orange panel lifts red most and
    // blue hardly, in the proportions of its light (13 : 5 : 0.5).
    let orange = [13.0, 5.0, 0.5];
    let plain = draw(orange, json!({}));
    let glow = draw(orange, json!({ "strength": 0.1 }));
    let [red, green, blue] = lift(&glow, &plain, MID);
    assert!(
        red > 0.02 && (red / green - 2.6).abs() < 0.5 && blue < green * 0.25,
        "the orange panel's glow: {red}, {green}, {blue}"
    );
}

/// A sunlit scene with nothing brighter than white in it: a rough ground, a
/// rough red box and a rough grey pillar under the default sky.
fn calm_entities() -> Value {
    json!({
        "camera": { "components": {
            "Camera": { "fov_y_deg": 50 },
            "Transform": { "translation": [0, 1.6, 7] } } },
        "ground": { "components": {
            "Material": { "base_color": [0.45, 0.45, 0.42, 1], "roughness": 0.9 },
            "Primitive": { "shape": "Plane" },
            "Transform": { "scale": [400, 1, 400] } } },
        "box": { "components": {
            "Material": { "base_color": [0.8, 0.1, 0.08, 1], "roughness": 0.9 },
            "Primitive": { "shape": "Cube" },
            "Transform": { "translation": [-1.4, 0.5, 2], "rotation": [0, 0.258_819, 0, 0.965_925_8] } } },
        "pillar": { "components": {
            "Material": { "base_color": [0.7, 0.7, 0.72, 1], "roughness": 0.9 },
            "Primitive": { "shape": "Cube" },
            "Transform": { "translation": [2, 1.5, 0], "scale": [1, 3, 1] } } },
        "sun": { "components": {
            "DirectionalLight": { "illuminance_lux": 100000 },
            "Transform": { "rotation": SUN_BEHIND } } }
    })
}

#[test]
fn a_scene_with_nothing_bright_barely_changes_and_keeps_its_brightness() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let draw = |bloom: Value| {
        let scene = prepare(&scene(json!({ "Bloom": bloom }), calm_entities()));
        renderer.render(&gpu, &scene, None, 320, 200).unwrap()
    };
    let plain = draw(json!({}));
    // A natural strength. A pixel gives up a twentieth of its light and
    // takes a twentieth of the light around it, so none can change by more
    // than a twentieth of the difference between the brightest and the
    // darkest thing in the picture, and most hardly change at all.
    let natural = draw(json!({ "strength": 0.05 }));
    assert!(natural.stats.effects.bloom);
    keep("bloom_calm", &natural.image);
    let (before, after) = (
        plain.image.pixels.as_chunks::<4>().0,
        natural.image.pixels.as_chunks::<4>().0,
    );
    let changes: Vec<f32> = before
        .iter()
        .zip(after)
        .flat_map(|(a, b)| {
            let (a, b) = (linear(*a), linear(*b));
            [0, 1, 2].map(|i| (a[i] - b[i]).abs())
        })
        .collect();
    let most = changes.iter().copied().fold(0.0, f32::max);
    let mean = changes.iter().sum::<f32>() / changes.len() as f32;
    assert!(
        most < 0.05 && mean < 0.008,
        "light moved, as a share of white: at most {most}, on average {mean}"
    );
    let moved = natural
        .image
        .diff(&plain.image, 4)
        .unwrap()
        .differing_pixels;
    assert!(moved < 64_000 / 10, "{moved} pixels moved by more than 4");

    // Much more of it: the picture is softer, and as bright as it was. The
    // light is moved, not added.
    let dreamy = draw(json!({ "strength": 0.5 }));
    assert!(furthest_apart(&dreamy.image, &plain.image) > 20);
    let (before, after) = (mean_light(&plain.image), mean_light(&dreamy.image));
    assert!(
        (after / before - 1.0).abs() < 0.02,
        "the picture's mean light: {before} without, {after} with"
    );
}

#[test]
fn strength_says_how_much_light_is_spread_and_spread_how_far() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let draw = |bloom: Value| {
        renderer
            .render(&gpu, &panel_scene(BRIGHT, bloom), None, 320, 200)
            .unwrap()
    };
    let plain = draw(json!({}));
    let glow_at = |frame: &Frame, at: (u32, u32)| total(lift(frame, &plain, at));

    // Twice the strength, twice the glow (where the picture is not yet
    // bright enough for the tone curve to bend it).
    let (one, two) = (
        draw(json!({ "strength": 0.05 })),
        draw(json!({ "strength": 0.1 })),
    );
    // Measured over 9x9 pixels: far from the panel the glow of the weaker
    // picture is two or three steps of 255 in a pixel.
    for at in [MID, FAR] {
        let ratio = lift_around(&two, &plain, at, 4) / lift_around(&one, &plain, at, 4);
        assert!((ratio - 2.0).abs() < 0.25, "{at:?}: {ratio}");
    }

    // A low spread keeps the glow around the light, a high one sends it
    // across the picture.
    let (tight, even, wide) = (
        draw(json!({ "strength": 0.1, "spread": 0 })),
        draw(json!({ "strength": 0.1 })),
        draw(json!({ "strength": 0.1, "spread": 1 })),
    );
    let near = [&tight, &even, &wide].map(|frame| glow_at(frame, NEAR));
    let far = [&tight, &even, &wide].map(|frame| glow_at(frame, FAR));
    assert!(
        far[2] > far[1] * 1.5 && far[1] > far[0] * 1.5,
        "47 pixels from the light, tight to wide: {far:?}"
    );
    // And the tighter, the faster it falls off on the way there.
    let steepness = [0, 1, 2].map(|i| near[i] / far[i]);
    assert!(
        steepness[0] > steepness[1] * 1.5 && steepness[1] > steepness[2] * 1.5,
        "how many times stronger at 7 pixels than at 47, tight to wide: {steepness:?}"
    );
    // The default spread is 0.5.
    let spelled_out = draw(json!({ "strength": 0.1, "spread": 0.5 }));
    assert_eq!(spelled_out.image, even.image);
}

/// Three dim bars (a third of white) with dark gaps between them on the
/// left, and optionally the bright panel on the right.
fn bars_scene(panel: bool, bloom: Value) -> RenderScene {
    let dim = [0.2, 0.2, 0.2];
    let bar = |x: f64| {
        json!({ "components": {
            "Material": { "base_color": [0, 0, 0, 1], "roughness": 1, "emissive": dim },
            "Primitive": { "shape": "Cube" },
            "Transform": { "translation": [x, 0, 0], "scale": [0.2, 1.2, 0.05] } } })
    };
    let mut entities = json!({
        "camera": camera_entity(),
        "bar1": bar(-2.0), "bar2": bar(-1.6), "bar3": bar(-1.2)
    });
    if panel {
        entities["panel"] = panel_entity(BRIGHT, 0.6, [1.6, 0.0, 0.0]);
    }
    prepare(&scene(dark_resources(bloom), entities))
}

#[test]
fn a_threshold_leaves_what_is_not_bright_as_sharp_as_it_was() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let draw = |panel: bool, bloom: Value| {
        let scene = bars_scene(panel, bloom);
        let frame = renderer.render(&gpu, &scene, None, 320, 200).unwrap();
        (frame, scene)
    };
    let (plain, scene) = draw(true, json!({}));
    let (all_light, _) = draw(true, json!({ "strength": 0.3 }));
    let (bright_only, _) = draw(true, json!({ "strength": 0.3, "threshold": 2 }));
    assert!(bright_only.stats.effects.bloom);
    keep("bloom_threshold", &bright_only.image);

    // The difference between a bar and the gap beside it, as light.
    let bar = pixel_of(&plain, &scene, Vec3::new(-1.6, 0.0, 0.025));
    let gap = pixel_of(&plain, &scene, Vec3::new(-1.8, 0.0, 0.025));
    let contrast = |frame: &Frame| total(light_at(frame, bar)) - total(light_at(frame, gap));
    let (sharp, soft, kept) = (
        contrast(&plain),
        contrast(&all_light),
        contrast(&bright_only),
    );
    assert!(sharp > 0.5, "a bar stands out from the gap: {sharp}");
    // Without a threshold the bars give up a share of their light too.
    assert!(soft < sharp * 0.85, "{soft} of {sharp}");
    // With one above them they keep all of it: the panel's glow lies over
    // bar and gap alike.
    assert!((kept / sharp - 1.0).abs() < 0.05, "{kept} of {sharp}");
    // And the panel still glows.
    let beside = pixel_of(&plain, &scene, Vec3::new(1.6 + 0.45, 0.0, 0.025));
    let glow = total(lift(&bright_only, &plain, beside));
    assert!(glow > 0.2, "the glow beside the panel: {glow}");

    // A scene with nothing above the threshold is left as it is.
    let (bars, _) = draw(false, json!({}));
    let (untouched, _) = draw(false, json!({ "strength": 0.3, "threshold": 2 }));
    assert!(untouched.stats.effects.bloom);
    assert!(furthest_apart(&untouched.image, &bars.image) <= 1);
}

// ---------------------------------------------------------------------------
// With the rest of the look

fn horizon_scene(resources: Value) -> RenderScene {
    prepare(&scene(
        resources,
        json!({
            "camera": { "components": {
                "Camera": { "fov_y_deg": 60 },
                "Transform": { "translation": [0, 1.6, 0] } } },
            "ground": { "components": {
                "Material": { "base_color": [0.45, 0.45, 0.42, 1], "roughness": 0.9 },
                "Primitive": { "shape": "Plane" },
                "Transform": { "scale": [100, 1, 100] } } },
            "sun": { "components": {
                "DirectionalLight": { "color": [1, 0.5, 0.2, 1] },
                "Transform": { "rotation": SUN_AHEAD } } }
        }),
    ))
}

#[test]
fn the_sun_disc_blooms() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let disc_scene = horizon_scene(json!({ "Sky": { "sun_size_deg": 5 } }));
    let glow_scene = horizon_scene(json!({
        "Sky": { "sun_size_deg": 5 }, "Bloom": { "strength": 0.1 }
    }));
    let disc = renderer.render(&gpu, &disc_scene, None, 320, 240).unwrap();
    let glow = renderer.render(&gpu, &glow_scene, None, 320, 240).unwrap();
    assert_eq!(glow.stats.effects.names(), ["sun_disc", "bloom"]);
    keep("bloom_sun", &glow.image);

    // The disc is 5 degrees wide: a radius of about 10 pixels here. Two and
    // four radii from its middle the sky is the plain sky without bloom,
    // and brighter with it, more so nearer the disc, and in the sun's warm
    // colour.
    let eye = disc_scene.cameras[0].position;
    let (cx, cy) = pixel_of(
        &disc,
        &disc_scene,
        eye - disc_scene.lights[0].direction * 1000.0,
    );
    let near = lift(&glow, &disc, (cx + 20, cy));
    let further = lift(&glow, &disc, (cx + 40, cy));
    assert!(
        total(near) > total(further) * 1.3 && total(further) > 0.02,
        "the sky 20 and 40 pixels from the sun: {near:?}, {further:?}"
    );
    assert!(near[0] > near[2], "the glow is warm: {near:?}");
    // The disc is still the brightest thing in the picture.
    let middle = glow.image.pixel(cx, cy).unwrap();
    assert!(middle[0] > 240 && middle[2] > 225, "{middle:?}");
}

/// The scene of the look's golden images with a lamp in it: a ground, a red
/// cube and a rough metal ball near the camera, a pillar and a block far
/// away, a low sun ahead, and a small ball that gives off orange light.
fn look_entities() -> Value {
    json!({
        "camera": { "components": {
            "Camera": { "fov_y_deg": 50 },
            "Transform": { "translation": [0, 1.6, 7] } } },
        "ground": { "components": {
            "Material": { "base_color": [0.45, 0.45, 0.42, 1], "roughness": 0.9 },
            "Primitive": { "shape": "Plane" },
            "Transform": { "scale": [400, 1, 400] } } },
        "cube": { "components": {
            "Material": { "base_color": [0.8, 0.1, 0.08, 1], "roughness": 0.6 },
            "Primitive": { "shape": "Cube" },
            "Transform": { "translation": [-1.4, 0.5, 2], "rotation": [0, 0.258_819, 0, 0.965_925_8] } } },
        "ball": { "components": {
            "Material": { "base_color": [0.95, 0.75, 0.35, 1], "metallic": 1, "roughness": 0.7 },
            "Primitive": { "shape": "Sphere" },
            "Transform": { "translation": [1.3, 0.6, 2.5], "scale": [1.2, 1.2, 1.2] } } },
        "lamp": { "components": {
            "Material": { "base_color": [0, 0, 0, 1], "roughness": 1, "emissive": [12, 5, 1] },
            "Primitive": { "shape": "Sphere" },
            "Transform": { "translation": [-1.4, 1.35, 2], "scale": [0.5, 0.5, 0.5] } } },
        "pillar": { "components": {
            "Material": { "base_color": [0.7, 0.7, 0.72, 1] },
            "Primitive": { "shape": "Cube" },
            "Transform": { "translation": [-9, 7, -60], "scale": [6, 14, 6] } } },
        "block": { "components": {
            "Material": { "base_color": [0.2, 0.5, 0.25, 1] },
            "Primitive": { "shape": "Cube" },
            "Transform": { "translation": [14, 4, -120], "scale": [14, 8, 14] } } },
        "sun": { "components": {
            "DirectionalLight": { "color": [1, 0.92, 0.8, 1] },
            "Transform": { "rotation": SUN_AHEAD } } }
    })
}

/// Every effect of the look, bloom included.
fn everything() -> Value {
    json!({
        "Ambient": { "source": "Sky", "sky_strength": 0.5 },
        "Sky": { "sun_size_deg": 4 },
        "Fog": { "density": 0.006, "height_falloff": 0.03 },
        "Bloom": { "strength": 0.12 },
        "ColorGrading": { "contrast": 1.1, "saturation": 1.1, "temperature": 0.1, "vignette": 0.25 },
        "RenderSettings": { "antialiasing": "Fxaa" }
    })
}

#[test]
fn bloom_with_fog_grading_and_fxaa_together() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let all = everything();
    let mut without = all.clone();
    without.as_object_mut().unwrap().remove("Bloom");
    let plain_scene = prepare(&scene(without, look_entities()));
    let plain = renderer.render(&gpu, &plain_scene, None, 320, 200).unwrap();
    let frame = renderer
        .render(&gpu, &prepare(&scene(all, look_entities())), None, 320, 200)
        .unwrap();
    assert_eq!(
        frame.stats.effects.names(),
        [
            "sky_light",
            "sun_disc",
            "fog",
            "bloom",
            "color_grading",
            "fxaa"
        ]
    );
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
    check_golden("bloom_all", &frame.image);

    // The lamp glows over the sky above it, in its orange, through the fog
    // and the grading.
    let above = pixel_of(&plain, &plain_scene, Vec3::new(-1.4, 1.35 + 0.45, 2.0));
    let [red, green, blue] = lift(&frame, &plain, above);
    assert!(
        red > 0.05 && red > green && green > blue,
        "above the lamp: {red}, {green}, {blue}"
    );
    // The bloom's passes are counted: five levels at this size, two passes
    // each.
    assert_eq!(frame.stats.draw_calls, plain.stats.draw_calls + 10);
    assert_eq!(frame.stats.triangles, plain.stats.triangles + 10);
}

#[test]
fn sprites_and_the_interface_are_drawn_after_the_bloom_and_keep_their_bytes() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // A bright panel with a sprite right beside it and an interface panel
    // over the sky above and to the right of it.
    let layered = |bloom: Value| {
        prepare(&scene(
            dark_resources(bloom),
            json!({
                "camera": camera_entity(),
                "panel": panel_entity(BRIGHT, 0.6, [0.0, 0.0, 0.0]),
                "flag": { "components": {
                    "Sprite": { "color": [0.1, 0.9, 0.2, 1], "rect": [0, 0, 60, 60] },
                    "Transform": { "translation": [0.75, 0, 0] } } },
                "badge": { "components": { "UiNode": {
                    "position": "Absolute", "offset": [760, 150], "padding": [20, 20, 20, 20],
                    "background": [0.9, 0.1, 0.5, 1] } } },
                "label": { "parent": "badge", "components": {
                    "UiNode": {}, "UiText": { "text": "Sharp", "size": 96 } } }
            }),
        ))
    };
    let plain_scene = layered(json!({}));
    let plain = renderer.render(&gpu, &plain_scene, None, 320, 200).unwrap();
    let glow_scene = layered(json!({ "strength": 0.5, "spread": 1 }));
    let glow = renderer.render(&gpu, &glow_scene, None, 320, 200).unwrap();
    assert!(glow.stats.effects.bloom);
    keep("bloom_layered", &glow.image);

    // Every pixel of the interface's panel, its text's edges included, is
    // what it was, although the glow reaches under it.
    let badge = wish_render::ui_elements(&plain_scene, 320, 200)
        .into_iter()
        .find(|element| element.text.is_none())
        .expect("the badge");
    let [left, top, right, bottom] = badge.bounding_box.map(|v| v as u32);
    assert!(
        right > left + 20 && bottom > top + 10,
        "{:?}",
        badge.bounding_box
    );
    for y in top + 1..bottom - 1 {
        for x in left + 1..right - 1 {
            assert_eq!(
                glow.image.pixel(x, y).unwrap(),
                plain.image.pixel(x, y).unwrap(),
                "({x}, {y})"
            );
        }
    }
    let under_badge = ((left + right) / 2, bottom + 3);
    assert!(total(lift(&glow, &plain, under_badge)) > 0.1);

    // The sprite stands where the glow is strongest: it is its own green in
    // both pictures and the sky right next to it is lit.
    let flag = pixel_of(&plain, &plain_scene, Vec3::new(0.75, 0.0, 0.0));
    let pixel = glow.image.pixel(flag.0, flag.1).unwrap();
    assert_eq!(pixel, plain.image.pixel(flag.0, flag.1).unwrap());
    assert!(
        pixel[1] > 200 && pixel[0] < 120,
        "the flag is green: {pixel:?}"
    );
    let under_flag = (flag.0, flag.1 + 20);
    assert!(total(lift(&glow, &plain, under_flag)) > 0.1);
}

// ---------------------------------------------------------------------------
// Sizes

#[test]
fn tiny_and_odd_pictures_are_drawn() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // The chain has as many levels as the height allows, at least one, and
    // each is half the one before, rounded up. Every read is held inside
    // its texture, so nothing in the picture is a colour from outside it:
    // the light is only moved, and no pixel ends up brighter than the
    // brightest or darker than the darkest of the picture without bloom.
    for (width, height, levels) in [
        (1u32, 1u32, 1u32),
        (2, 2, 1),
        (1, 48, 3),
        (48, 1, 1),
        (7, 7, 1),
        (3, 5, 1),
        (33, 17, 2),
        (321, 201, 5),
    ] {
        let plain = renderer
            .render(&gpu, &panel_scene(BRIGHT, json!({})), None, width, height)
            .unwrap();
        let glow = renderer
            .render(
                &gpu,
                &panel_scene(BRIGHT, json!({ "strength": 0.3 })),
                None,
                width,
                height,
            )
            .unwrap();
        assert!(glow.stats.effects.bloom, "{width}x{height}");
        assert!(glow.warnings.is_empty(), "{:?}", glow.warnings);
        assert_eq!(
            glow.stats.draw_calls,
            plain.stats.draw_calls + 2 * levels,
            "{width}x{height}"
        );
        let places = || (0..height).flat_map(|y| (0..width).map(move |x| (x, y)));
        for channel in 0..3 {
            let of = |frame: &Frame, (x, y): (u32, u32)| {
                i32::from(frame.image.pixel(x, y).unwrap()[channel])
            };
            let darkest = places().map(|at| of(&plain, at)).min().unwrap();
            let brightest = places().map(|at| of(&plain, at)).max().unwrap();
            for at in places() {
                let value = of(&glow, at);
                assert!(
                    value >= darkest - 1 && value <= brightest + 1,
                    "{width}x{height} at {at:?}: {value} is outside {darkest}..={brightest}"
                );
            }
        }
        // A picture of one colour has nothing to move.
        if width * height <= 4 {
            assert!(
                furthest_apart(&glow.image, &plain.image) <= 1,
                "{width}x{height}"
            );
        }
        // Where the panel is in view, it glows.
        if width > 100 {
            let beside = (width / 2 + 26, height / 2);
            assert!(total(lift(&glow, &plain, beside)) > 0.2, "{width}x{height}");
        }
    }
}

#[test]
fn the_glow_is_alike_in_a_small_picture_and_a_large_one() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // The same scene at 240x160 and at twice that. The glow at the same
    // place of the picture (a share of its height away from the panel) is
    // alike: the levels are weighted by how far they reach as a share of
    // the picture, not by their number. (Below a spread of 0.5 this holds
    // less well for pictures this small: a tight glow lives in the finest
    // levels, and a small picture does not have them.)
    let glow_at = |scale: u32, spread: f64, away: u32| {
        let (width, height) = (240 * scale, 160 * scale);
        let plain = renderer
            .render(&gpu, &panel_scene(BRIGHT, json!({})), None, width, height)
            .unwrap();
        let glow = renderer
            .render(
                &gpu,
                &panel_scene(BRIGHT, json!({ "strength": 0.1, "spread": spread })),
                None,
                width,
                height,
            )
            .unwrap();
        // Over the same share of the picture at both sizes.
        let at = (width / 2 + away * scale, height / 2);
        lift_around(&glow, &plain, at, 2 * scale)
    };
    for spread in [0.5, 0.8] {
        for away in [16, 30, 45] {
            let (small, large) = (glow_at(1, spread, away), glow_at(2, spread, away));
            assert!(
                small > 0.03 && (large / small - 1.0).abs() < 0.3,
                "spread {spread}, {away} pixels of the small picture away: {small} and {large}"
            );
        }
    }
}

#[test]
fn bloom_is_the_same_under_an_orthographic_camera() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // The bloom works on the picture's light, whatever camera drew it. An
    // orthographic camera that shows as many meters as the perspective one
    // does at the panel's front, 4.975 m away (4.1214 from top to bottom),
    // draws the same panel over the same pixels, and the same glow around
    // it.
    let draw = |camera: Value, bloom: Value| {
        let scene = prepare(&scene(
            dark_resources(bloom),
            json!({
                "camera": { "components": {
                    "Camera": camera,
                    "Transform": { "translation": [0, 0, 5] } } },
                "panel": panel_entity(BRIGHT, 0.6, [0.0, 0.0, 0.0])
            }),
        ));
        renderer.render(&gpu, &scene, None, 320, 200).unwrap()
    };
    let glow_of = |camera: Value| {
        let plain = draw(camera.clone(), json!({}));
        let glow = draw(camera, json!({ "strength": 0.1 }));
        assert!(glow.stats.effects.bloom);
        assert!(white_hot(glow.image.pixel(MIDDLE.0, MIDDLE.1).unwrap()));
        [NEAR, MID, FAR].map(|at| total(lift(&glow, &plain, at)))
    };
    let perspective = glow_of(json!({ "fov_y_deg": 45 }));
    let orthographic = glow_of(json!({ "projection": "Orthographic", "ortho_height": 4.1214 }));
    for (flat, deep) in orthographic.iter().zip(perspective) {
        assert!(
            *flat > 0.01 && (flat / deep - 1.0).abs() < 0.1,
            "orthographic {orthographic:?}, perspective {perspective:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Bad input

/// The base colour that marks the surface a test then breaks.
const MARKED: [f32; 4] = [0.5, 0.25, 0.125, 1.0];

/// The bright panel in the dark, and left of it a small black box, 12
/// pixels wide, whose emissive colour a test sets to something a scene file
/// cannot hold.
fn broken_scene(emissive: [f32; 3], bloom: Value) -> RenderScene {
    let mut scene = prepare(&scene(
        dark_resources(bloom),
        json!({
            "camera": camera_entity(),
            "panel": panel_entity(BRIGHT, 0.6, [0.0, 0.0, 0.0]),
            "box": { "components": {
                "Material": { "base_color": MARKED, "roughness": 1 },
                "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [-1.5, 0.6, 0], "scale": [0.25, 0.25, 0.05] } } }
        }),
    ));
    let marked = scene
        .draws
        .iter_mut()
        .find(|draw| draw.material.base_color == MARKED)
        .expect("the box");
    marked.material.base_color = [0.0, 0.0, 0.0, 1.0];
    marked.material.emissive = emissive;
    scene
}

#[test]
fn a_pixel_that_is_not_a_number_does_not_spread() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let draw = |emissive: [f32; 3], bloom: Value| {
        let scene = broken_scene(emissive, bloom);
        let frame = renderer.render(&gpu, &scene, None, 320, 200).unwrap();
        (frame, scene)
    };
    // The box really puts "not a number" into the picture before tone
    // mapping: without bloom its pixels are the pure black tone mapping
    // makes of that, where a black box shows a trace of the sky's
    // reflection.
    let (black_plain, scene) = draw([0.0; 3], json!({}));
    let (broken_plain, _) = draw([f32::NAN; 3], json!({}));
    let at = pixel_of(&black_plain, &scene, Vec3::new(-1.5, 0.6, 0.025));
    assert_eq!(
        broken_plain.image.pixel(at.0, at.1).unwrap(),
        [0, 0, 0, 255]
    );
    assert_ne!(black_plain.image.pixel(at.0, at.1).unwrap(), [0, 0, 0, 255]);

    // With bloom the box counts as no light: the picture is the one of the
    // black box, and the panel's glow is all there. (Read as it is, such a
    // texel either turns every level it reaches into "not a number", or,
    // on an adapter whose `min` drops it, counts as the brightest light
    // there is: with the cleaning taken out of the shader, this test fails
    // on the software adapter by a glow over the whole picture.)
    let bloom = json!({ "strength": 0.3 });
    let (black, _) = draw([0.0; 3], bloom.clone());
    let (broken, _) = draw([f32::NAN; 3], bloom.clone());
    keep("bloom_not_a_number", &broken.image);
    for y in 0..200u32 {
        for x in 0..320u32 {
            let inside = x.abs_diff(at.0) <= 8 && y.abs_diff(at.1) <= 8;
            let (a, b) = (
                broken.image.pixel(x, y).unwrap(),
                black.image.pixel(x, y).unwrap(),
            );
            let apart = (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap();
            // On the box itself the black one has its trace of reflection.
            assert!(
                apart <= if inside { 12 } else { 1 },
                "({x}, {y}): {a:?} and {b:?}"
            );
        }
    }
    assert!(total(lift(&broken, &broken_plain, NEAR)) > 0.5);
    assert!(broken.warnings.is_empty(), "{:?}", broken.warnings);

    // Infinity counts as the brightest light there is, like any surface
    // brighter than the bloom's ceiling of 256 times white: the picture of
    // an infinite box is the picture of one 600 times white.
    let ceiling = (600.0 / WHITES_PER_EMISSIVE) as f32;
    let (very_bright, _) = draw([ceiling; 3], bloom.clone());
    let (infinite, _) = draw([f32::INFINITY; 3], bloom);
    keep("bloom_infinite", &infinite.image);
    assert!(furthest_apart(&infinite.image, &very_bright.image) <= 1);
    assert_eq!(
        infinite.image.pixel(at.0, at.1).unwrap()[..3],
        [255, 255, 255],
        "an infinite pixel shows white"
    );
    // And the rest of the picture is still a picture.
    let corner = infinite.image.pixel(315, 195).unwrap();
    assert!(corner[2] > 20 && corner[2] < 200, "{corner:?}");
}

#[test]
fn one_very_bright_pixel_does_not_bloom_like_a_light() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // Two lights that give off the same amount of light: one a pixel and a
    // half wide and a thousand times white, one six times as wide and 28
    // times white.
    let thousand = 1000.0 / WHITES_PER_EMISSIVE;
    let glow_of = |side: f64, emissive: f64| {
        let draw = |bloom: Value| {
            let scene = prepare(&scene(
                dark_resources(bloom),
                json!({
                    "camera": camera_entity(),
                    "light": panel_entity([emissive; 3], side, [0.0, 0.0, 0.0])
                }),
            ));
            renderer.render(&gpu, &scene, None, 320, 200).unwrap()
        };
        let (plain, glow) = (draw(json!({})), draw(json!({ "strength": 0.2 })));
        // The light is there, as bright as the picture can show.
        let lit = (90..110)
            .flat_map(|y| (150..170).map(move |x| (x, y)))
            .filter(|(x, y)| white_hot(plain.image.pixel(*x, *y).unwrap()))
            .count();
        // Its glow, ten pixels right of its middle.
        (lit, total(lift(&glow, &plain, (170, 100))))
    };
    let (firefly_pixels, firefly) = glow_of(0.03, thousand);
    let (area_pixels, area) = glow_of(0.18, thousand / 36.0);
    assert!((1..=4).contains(&firefly_pixels), "{firefly_pixels} pixels");
    assert!(area_pixels >= 49, "{area_pixels} pixels");
    assert!(area > 0.08, "the wide light glows: {area}");
    assert!(
        firefly < area * 0.1,
        "the single pixel is held: its glow is {firefly}, the wide light's {area}"
    );
}

#[test]
fn settings_that_are_not_numbers_draw_the_picture_of_their_fallbacks() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let draw = |scene: &RenderScene| renderer.render(&gpu, scene, None, 160, 100).unwrap();
    let plain = draw(&panel_scene(BRIGHT, json!({})));
    // The fixture is format 1, whose spread was 0.5: the engine's default
    // (0.4 since the look lock) is written out.
    let even = draw(&panel_scene(
        BRIGHT,
        json!({ "strength": 0.2, "spread": 0.4 }),
    ));

    // A scene file cannot hold these (the loader checks ranges); game code
    // can write them into the resource. A strength that is not a number is
    // no bloom.
    for strength in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -3.0] {
        let mut broken = panel_scene(BRIGHT, json!({}));
        broken.bloom.strength = strength;
        broken.bloom.spread = f32::NAN;
        broken.bloom.threshold = f32::NAN;
        let frame = draw(&broken);
        assert_eq!(frame.image, plain.image, "strength {strength}");
        assert_eq!(frame.stats.effects, Effects::default());
    }
    // The other two fall back to the engine's defaults.
    let mut broken = panel_scene(BRIGHT, json!({ "strength": 0.2 }));
    broken.bloom.spread = f32::NAN;
    broken.bloom.threshold = f32::INFINITY;
    assert_eq!(draw(&broken).image, even.image);

    // Out of range: held to the range's end, and the picture is the one of
    // that end.
    let mut extreme = panel_scene(BRIGHT, json!({}));
    extreme.bloom.strength = 7.0;
    extreme.bloom.spread = -3.0;
    extreme.bloom.threshold = 1.0e9;
    let held = panel_scene(
        BRIGHT,
        json!({ "strength": 1, "spread": 0, "threshold": 64 }),
    );
    assert_eq!(draw(&extreme).image, draw(&held).image);
    let mut extreme = panel_scene(BRIGHT, json!({ "strength": 0.2 }));
    extreme.bloom.spread = 55.0;
    extreme.bloom.threshold = -4.0;
    let held = panel_scene(BRIGHT, json!({ "strength": 0.2, "spread": 1 }));
    assert_eq!(draw(&extreme).image, draw(&held).image);
}

#[test]
fn a_second_frame_with_bloom_creates_nothing() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let plain = renderer
        .render(&gpu, &panel_scene(BRIGHT, json!({})), None, 320, 200)
        .unwrap();
    let made_plain = gpu.created();
    let scene = panel_scene(BRIGHT, json!({ "strength": 0.2 }));
    let glow = renderer.render(&gpu, &scene, None, 320, 200).unwrap();
    // 320x200 has five levels: ten more passes, each a triangle; six more
    // targets (the levels and the bloomed picture) and one small buffer.
    assert_eq!(glow.stats.draw_calls, plain.stats.draw_calls + 10);
    assert_eq!(glow.stats.triangles, plain.stats.triangles + 10);
    let made_glow = gpu.created();
    assert_eq!(
        (
            made_glow.textures - made_plain.textures,
            made_glow.buffers - made_plain.buffers
        ),
        (6, 1)
    );
    let again = renderer.render(&gpu, &scene, None, 320, 200).unwrap();
    assert_eq!(gpu.created(), made_glow, "a second frame creates nothing");
    assert_eq!(again.image, glow.image);
    // Other settings are other numbers in the same buffer and other blend
    // constants: nothing new either.
    let other = panel_scene(
        BRIGHT,
        json!({ "strength": 0.4, "spread": 0.2, "threshold": 1 }),
    );
    let frame = renderer.render(&gpu, &other, None, 320, 200).unwrap();
    assert_eq!(gpu.created(), made_glow);
    assert_ne!(frame.image, glow.image);
}

/// What the bloom costs, by the clock: run by hand, in release
/// (`cargo test --release -p wish_render --test bloom what_bloom_costs --
/// --ignored --nocapture`; `WISH_SOFTWARE=1` for the software adapter).
/// The time of a frame, drawn and read back, with and without bloom, at two
/// sizes. Nothing is asserted: the numbers depend on the machine.
#[test]
#[ignore = "a measurement, run by hand in release"]
fn what_bloom_costs() {
    let software = std::env::var_os("WISH_SOFTWARE").is_some();
    let gpu = Gpu::new(GpuOptions { software }).expect("an adapter");
    eprintln!("adapter: {:?}", gpu.adapter());
    let renderer = Renderer::new(&gpu).unwrap();
    let plain = prepare(&scene(json!({}), look_entities()));
    let bloomed = prepare(&scene(
        json!({ "Bloom": { "strength": 0.12 } }),
        look_entities(),
    ));
    for (width, height) in [(1280u32, 720u32), (1920, 1080)] {
        let time = |scene: &RenderScene| {
            // A few frames first, so that the targets exist.
            for _ in 0..3 {
                renderer.render(&gpu, scene, None, width, height).unwrap();
            }
            let frames = 30;
            let start = std::time::Instant::now();
            for _ in 0..frames {
                renderer.render(&gpu, scene, None, width, height).unwrap();
            }
            start.elapsed().as_secs_f64() * 1000.0 / f64::from(frames)
        };
        let (without, with) = (time(&plain), time(&bloomed));
        eprintln!(
            "{width}x{height}: {without:.2} ms a frame without bloom, {with:.2} ms with ({:+.2} ms)",
            with - without
        );
    }
}
