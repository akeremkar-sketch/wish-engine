//! The look settings (design note M12-look-1-sky-fog-grading-fxaa): the
//! sky's light, the sun's disc, fog, colour grading and FXAA, on the
//! software adapter.
//!
//! Each effect is off by default, and the first test is the rule the
//! package stands on: a scene that asks for none of them is drawn as it
//! always was. Every other test turns one effect on and measures what it
//! should change, and that what it should not change is the same byte for
//! byte. `UPDATE_GOLDEN=1` rewrites the golden images.

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

/// A sun that shines straight down.
const SUN_DOWN: [f64; 4] = [
    -std::f64::consts::FRAC_1_SQRT_2,
    0.0,
    0.0,
    std::f64::consts::FRAC_1_SQRT_2,
];
/// A sun 14 degrees above the horizon, ahead of a camera that looks along
/// -Z and 10 degrees to its right: it is in the picture.
const SUN_AHEAD: [f64; 4] = [-0.010_621_6, 0.988_769_3, 0.121_405_6, 0.086_506_1];
/// The same sun 14 degrees below the horizon.
const SUN_SET: [f64; 4] = [0.010_621_6, 0.988_769_3, -0.121_405_6, 0.086_506_1];
/// The sun of the lit golden image: from behind the camera's left shoulder.
const SUN_BEHIND: [f64; 4] = [-0.454_519_5, 0.330_454_3, 0.178_659_7, 0.806_704_6];

/// A sky whose three colours cannot be mistaken for each other.
fn three_colour_sky() -> Value {
    json!({ "zenith": [0.1, 0.2, 0.9, 1], "horizon": [0.5, 0.5, 0.5, 1], "ground": [0.8, 0.2, 0.1, 1] })
}

fn scene(resources: Value, entities: Value) -> String {
    json!({ "format": 1, "resources": resources, "entities": entities }).to_string()
}

fn world_of(source: &str) -> World {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    wish_ui::register(&mut world).unwrap();
    let result = load(source, "scenes/look.scene.json", world.registry());
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

fn pixel_at(frame: &Frame, scene: &RenderScene, point: Vec3) -> [u8; 4] {
    let (x, y) = pixel_of(frame, scene, point);
    frame.image.pixel(x, y).expect("on screen")
}

/// A pixel's red, green and blue as numbers that can be doubled and
/// subtracted.
fn rgb(pixel: [u8; 4]) -> [i32; 3] {
    [0, 1, 2].map(|i| i32::from(pixel[i]))
}

/// A pixel's red, green and blue as light: the picture's sRGB encoding
/// undone. Ratios between colours of light are only true here (twice the
/// light is not twice the byte).
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

fn sum(pixel: [u8; 4]) -> i32 {
    i32::from(pixel[0]) + i32::from(pixel[1]) + i32::from(pixel[2])
}

/// How far apart two colours are: the largest difference of a channel.
fn apart(a: [u8; 4], b: [u8; 4]) -> i32 {
    (0..3)
        .map(|i| (i32::from(a[i]) - i32::from(b[i])).abs())
        .max()
        .unwrap()
}

/// How many pixels of two pictures of one size differ at all.
fn differing(a: &RgbaImage, b: &RgbaImage) -> u64 {
    a.diff(b, 0).expect("same size").differing_pixels
}

// ---------------------------------------------------------------------------
// The scene of the golden images: a ground, a red cube and a rough metal ball
// near the camera, a pillar and a block far away, and a low sun ahead, so
// that the camera sees the sides the sun does not reach.

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

fn look_scene(resources: Value) -> RenderScene {
    prepare(&scene(resources, look_entities()))
}

/// Every effect of this package, turned on to taste.
fn everything() -> Value {
    json!({
        "Ambient": { "source": "Sky", "sky_strength": 0.5 },
        "Sky": { "sun_size_deg": 4 },
        "Fog": { "density": 0.006, "height_falloff": 0.03 },
        "ColorGrading": { "contrast": 1.1, "saturation": 1.1, "temperature": 0.1, "vignette": 0.25 },
        "RenderSettings": { "antialiasing": "Fxaa" }
    })
}

#[test]
fn a_scene_that_asks_for_nothing_is_drawn_as_it_always_was() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let plain = renderer
        .render(&gpu, &look_scene(json!({})), None, 320, 200)
        .unwrap();
    assert_eq!(plain.stats.effects, Effects::default());
    assert!(plain.stats.effects.names().is_empty());
    assert!(plain.warnings.is_empty(), "{:?}", plain.warnings);

    // Every new resource present, every field at its default.
    let defaults = look_scene(json!({
        "Ambient": {}, "Sky": {}, "Fog": {}, "ColorGrading": {}, "RenderSettings": {}
    }));
    let frame = renderer.render(&gpu, &defaults, None, 320, 200).unwrap();
    assert_eq!(frame.image, plain.image);
    assert_eq!(frame.stats, plain.stats);

    // The defaults written out, and fields that only matter once their
    // effect is on set to something else: still nothing is on.
    let spelled_out = look_scene(json!({
        "Ambient": { "source": "Color", "sky_strength": 4 },
        "Sky": { "sun_size_deg": 0 },
        "Fog": { "density": 0, "height_falloff": 0.5, "base_height": 3, "start": 20,
                 "tint": [1, 0.2, 0.2, 1], "max_opacity": 0.5 },
        "ColorGrading": { "contrast": 1, "saturation": 1, "temperature": 0, "tint": 0, "vignette": 0 },
        "RenderSettings": { "antialiasing": "Off" }
    }));
    let frame = renderer.render(&gpu, &spelled_out, None, 320, 200).unwrap();
    assert_eq!(frame.image, plain.image);
    assert_eq!(frame.stats.effects, Effects::default());

    // Fog that hides nothing is no fog.
    let hidden = look_scene(json!({ "Fog": { "density": 0.2, "max_opacity": 0 } }));
    let frame = renderer.render(&gpu, &hidden, None, 320, 200).unwrap();
    assert_eq!(frame.image, plain.image);
    assert!(!frame.stats.effects.fog);

    // And a renderer that drew with everything on draws the plain scene the
    // same afterwards: nothing of a look is kept between frames.
    let busy = renderer
        .render(&gpu, &look_scene(everything()), None, 320, 200)
        .unwrap();
    assert_ne!(busy.image, plain.image);
    let again = renderer
        .render(&gpu, &look_scene(json!({})), None, 320, 200)
        .unwrap();
    assert_eq!(again.image, plain.image);

    check_golden("look_plain", &plain.image);
}

#[test]
fn the_golden_images_of_each_effect_and_of_all_together() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let all = everything();
    for (name, resources, effect) in [
        (
            "look_sky_light",
            json!({ "Ambient": all["Ambient"] }),
            "sky_light",
        ),
        ("look_sun_disc", json!({ "Sky": all["Sky"] }), "sun_disc"),
        ("look_fog", json!({ "Fog": all["Fog"] }), "fog"),
        (
            "look_grading",
            json!({ "ColorGrading": all["ColorGrading"] }),
            "color_grading",
        ),
        (
            "look_fxaa",
            json!({ "RenderSettings": all["RenderSettings"] }),
            "fxaa",
        ),
    ] {
        let frame = renderer
            .render(&gpu, &look_scene(resources), None, 320, 200)
            .unwrap();
        assert_eq!(frame.stats.effects.names(), [effect], "{name}");
        assert!(frame.warnings.is_empty(), "{name}: {:?}", frame.warnings);
        check_golden(name, &frame.image);
    }
    let frame = renderer
        .render(&gpu, &look_scene(all), None, 320, 200)
        .unwrap();
    assert_eq!(
        frame.stats.effects.names(),
        ["sky_light", "sun_disc", "fog", "color_grading", "fxaa"]
    );
    check_golden("look_all", &frame.image);
}

// ---------------------------------------------------------------------------
// Sky light

/// A white box floating above a grey ground under a sun that shines straight
/// down, seen from below: its underside and the ground beneath it get no
/// direct light, only ambient.
fn overhang(ambient: Value) -> RenderScene {
    prepare(&scene(
        json!({ "Sky": three_colour_sky(), "Ambient": ambient }),
        json!({
            "camera": { "components": {
                "Camera": { "fov_y_deg": 60 },
                "Transform": { "translation": [0, 0.3, 4], "rotation": [0.086_55, 0, 0, 0.996_25] } } },
            "box": { "components": {
                "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [0, 2, 0], "scale": [2, 2, 2] } } },
            "ground": { "components": {
                "Material": { "base_color": [0.6, 0.6, 0.6, 1], "roughness": 0.9 },
                "Primitive": { "shape": "Plane" },
                "Transform": { "scale": [40, 1, 40] } } },
            "sun": { "components": {
                "DirectionalLight": {},
                "Transform": { "rotation": SUN_DOWN } } }
        }),
    ))
}

#[test]
fn sky_light_colours_a_surface_by_the_way_it_faces() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let flat_scene = overhang(json!({}));
    let sky_scene = overhang(json!({ "source": "Sky" }));
    assert_eq!(flat_scene.sky_light, None);
    assert_eq!(sky_scene.sky_light, Some(1.0));
    let flat = renderer.render(&gpu, &flat_scene, None, 320, 240).unwrap();
    let sky = renderer.render(&gpu, &sky_scene, None, 320, 240).unwrap();
    assert!(sky.stats.effects.sky_light && !flat.stats.effects.sky_light);

    let underside = Vec3::new(0.0, 1.0, 0.6);
    let shaded_ground = Vec3::new(0.0, 0.0, 0.6);
    let (under_flat, under_sky) = (
        pixel_at(&flat, &flat_scene, underside),
        pixel_at(&sky, &sky_scene, underside),
    );
    let (ground_flat, ground_sky) = (
        pixel_at(&flat, &flat_scene, shaded_ground),
        pixel_at(&sky, &sky_scene, shaded_ground),
    );

    // One flat colour: the underside and the shaded ground are both bluish,
    // as the ambient light is (0.5, 0.6, 0.8), whichever way they face. (The
    // sky's faint reflection adds a little of the colour each mirrors.)
    for (name, pixel) in [("underside", under_flat), ("ground", ground_flat)] {
        assert!(
            pixel[2] > pixel[1] && pixel[2] > pixel[0],
            "flat ambient on the {name}: {pixel:?}"
        );
    }
    // From the sky: the underside faces the ground's red (four fifths of
    // its light is the ground's 0.8, 0.2, 0.1 and a fifth the grey
    // horizon's: red 2.8 times green and 4 times blue), the shaded ground
    // the zenith's blue (0.1, 0.2, 0.9 with the same fifth of grey: blue 3
    // times green and 4.4 times red).
    let [r, g, b] = linear(under_sky);
    assert!(
        r > 2.2 * g && r > 3.0 * b,
        "the underside takes the ground's colour: {under_sky:?}"
    );
    let [r, g, b] = linear(ground_sky);
    assert!(
        b > 2.2 * g && b > 3.0 * r,
        "the shaded ground takes the zenith's colour: {ground_sky:?}"
    );
    // And this sky gives more light than the default ambient light does.
    assert!(
        sum(under_sky) > sum(under_flat) + 60,
        "the underside is brighter: {under_sky:?} vs {under_flat:?}"
    );

    // Half the strength is clearly darker, with the same hue.
    let half_scene = overhang(json!({ "source": "Sky", "sky_strength": 0.5 }));
    let half = renderer.render(&gpu, &half_scene, None, 320, 240).unwrap();
    let under_half = pixel_at(&half, &half_scene, underside);
    let [r, g, _] = linear(under_half);
    assert!(
        sum(under_half) < sum(under_sky) - 60 && r > 2.2 * g,
        "{under_half:?} vs {under_sky:?}"
    );

    // Nothing else changes: the sky itself is the same (left of the box,
    // down to just above the horizon, the picture is all sky).
    for y in 0..140 {
        for x in 0..60 {
            assert_eq!(flat.image.pixel(x, y), sky.image.pixel(x, y), "({x}, {y})");
        }
    }
}

#[test]
fn a_rough_metal_reflects_a_wide_part_of_the_sky() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let balls = |ambient: Value| {
        prepare(&scene(
            json!({ "Sky": three_colour_sky(), "Ambient": ambient }),
            json!({
                "camera": { "components": {
                    "Camera": { "fov_y_deg": 45 },
                    "Transform": { "translation": [0, 1, 6] } } },
                "mirror": { "components": {
                    "Material": { "metallic": 1, "roughness": 0 },
                    "Primitive": { "shape": "Sphere" },
                    "Transform": { "translation": [-1.2, 1, 0], "scale": [2, 2, 2] } } },
                "rough": { "components": {
                    "Material": { "metallic": 1, "roughness": 1 },
                    "Primitive": { "shape": "Sphere" },
                    "Transform": { "translation": [1.2, 1, 0], "scale": [2, 2, 2] } } }
            }),
        ))
    };
    let (flat_scene, sky_scene) = (balls(json!({})), balls(json!({ "source": "Sky" })));
    let flat = renderer.render(&gpu, &flat_scene, None, 320, 240).unwrap();
    let sky = renderer.render(&gpu, &sky_scene, None, 320, 240).unwrap();

    // The point of each ball nearest the camera, which is at the balls'
    // height: there the reflection goes level, to the horizon.
    let camera = Vec3::new(0.0, 1.0, 6.0);
    let nearest = |centre: Vec3| centre + (camera - centre).normalize();
    let mirror = nearest(Vec3::new(-1.2, 1.0, 0.0));
    let rough = nearest(Vec3::new(1.2, 1.0, 0.0));
    // (The pixel's middle is not exactly on the ball's equator, so the
    // reflection dips a little under the horizon and takes a little red.)
    let grey =
        |pixel: [u8; 4]| pixel[0].abs_diff(pixel[1]) <= 20 && pixel[1].abs_diff(pixel[2]) <= 20;

    // Without sky light every metal shows the gradient sharp, whatever its
    // roughness: both balls show the grey horizon.
    let (mirror_flat, rough_flat) = (
        pixel_at(&flat, &flat_scene, mirror),
        pixel_at(&flat, &flat_scene, rough),
    );
    assert!(grey(mirror_flat), "mirror {mirror_flat:?}");
    assert!(grey(rough_flat), "rough {rough_flat:?}");
    assert!(
        rough_flat[1] >= rough_flat[2],
        "no blue from above in it: {rough_flat:?}"
    );
    // With it, the mirror still does, and the rough ball gathers the blue
    // above and the red below with the grey: its green falls behind.
    let (mirror_sky, rough_sky) = (
        pixel_at(&sky, &sky_scene, mirror),
        pixel_at(&sky, &sky_scene, rough),
    );
    assert!(
        apart(mirror_sky, mirror_flat) <= 2,
        "the mirror is unchanged: {mirror_sky:?} vs {mirror_flat:?}"
    );
    assert!(
        rough_sky[1] + 12 < rough_sky[0] && rough_sky[1] + 12 < rough_sky[2],
        "the rough ball mixes the sky: {rough_sky:?} (it was {rough_flat:?})"
    );
}

// ---------------------------------------------------------------------------
// The sun's disc

fn horizon_scene(sky: Value, sun: Option<[f64; 4]>, ground: bool) -> RenderScene {
    let mut entities = json!({
        "camera": { "components": {
            "Camera": { "fov_y_deg": 60 },
            "Transform": { "translation": [0, 1.6, 0] } } }
    });
    if ground {
        entities["ground"] = json!({ "components": {
            "Material": { "base_color": [0.45, 0.45, 0.42, 1], "roughness": 0.9 },
            "Primitive": { "shape": "Plane" },
            "Transform": { "scale": [100, 1, 100] } } });
    }
    if let Some(rotation) = sun {
        entities["sun"] = json!({ "components": {
            "DirectionalLight": { "color": [1, 0.5, 0.2, 1] },
            "Transform": { "rotation": rotation } } });
    }
    prepare(&scene(json!({ "Sky": sky }), entities))
}

#[test]
fn the_sun_disc_is_where_the_light_comes_from() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let plain_scene = horizon_scene(json!({}), Some(SUN_AHEAD), true);
    let disc_scene = horizon_scene(json!({ "sun_size_deg": 5 }), Some(SUN_AHEAD), true);
    let plain = renderer.render(&gpu, &plain_scene, None, 320, 240).unwrap();
    let disc = renderer.render(&gpu, &disc_scene, None, 320, 240).unwrap();
    assert!(disc.stats.effects.sun_disc && !plain.stats.effects.sun_disc);
    assert!(disc.warnings.is_empty(), "{:?}", disc.warnings);

    // The light travels along `direction`; the sun stands the other way.
    let direction = disc_scene.lights[0].direction;
    assert!(
        (direction - Vec3::new(-0.1685, -0.2419, 0.9556)).length() < 1e-3,
        "{direction}"
    );
    let eye = disc_scene.cameras[0].position;
    let (cx, cy) = pixel_of(&disc, &disc_scene, eye - direction * 1000.0);

    // Its middle is the brightest thing in the picture, in the light's warm
    // colour (the tone mapping takes anything that bright nearly to white).
    let middle = disc.image.pixel(cx, cy).unwrap();
    assert!(
        middle[0] >= middle[1] && middle[1] >= middle[2] && middle[2] > 225,
        "the disc's middle: {middle:?}"
    );
    assert!(sum(middle) > sum(plain.image.pixel(cx, cy).unwrap()) + 60);

    // It is as wide as asked: 5 degrees are 18 pixels in the middle of this
    // picture (240 pixels for 60 degrees: 2 * 207.8 * tan 2.5) and about 20
    // where the sun stands, 17 degrees off the picture's axis.
    let wide = (0..320)
        .filter(|x| {
            sum(disc.image.pixel(*x, cy).unwrap()) > sum(plain.image.pixel(*x, cy).unwrap()) + 24
        })
        .count();
    assert!((17..=24).contains(&wide), "the disc is {wide} pixels wide");
    // Its edge is soft: between the full disc and the bare sky some pixels
    // are neither.
    let soft = (0..320)
        .filter(|x| {
            let lift =
                sum(disc.image.pixel(*x, cy).unwrap()) - sum(plain.image.pixel(*x, cy).unwrap());
            lift > 6 && lift < sum(middle) - sum(plain.image.pixel(cx, cy).unwrap()) - 6
        })
        .count();
    assert!(soft >= 2, "{soft} pixels of soft edge");

    // Away from it, nothing changes: twice its radius from its middle every
    // pixel is the same, the ground included.
    let mut changed_far = 0;
    for y in 0..240u32 {
        for x in 0..320u32 {
            let (dx, dy) = (x as f32 - cx as f32, y as f32 - cy as f32);
            // To 1/255: with a disc the sky is drawn by another function
            // of the sky pass, and a driver may round its gradient the
            // other way.
            if (dx * dx + dy * dy).sqrt() > 19.0
                && apart(
                    disc.image.pixel(x, y).unwrap(),
                    plain.image.pixel(x, y).unwrap(),
                ) > 1
            {
                changed_far += 1;
            }
        }
    }
    assert_eq!(changed_far, 0);

    // A wider disc is wider and, giving the same light, not brighter.
    let wider_scene = horizon_scene(json!({ "sun_size_deg": 12 }), Some(SUN_AHEAD), true);
    let wider = renderer.render(&gpu, &wider_scene, None, 320, 240).unwrap();
    let wide_12 = (0..320)
        .filter(|x| {
            sum(wider.image.pixel(*x, cy).unwrap()) > sum(plain.image.pixel(*x, cy).unwrap()) + 24
        })
        .count();
    assert!(
        (43..=56).contains(&wide_12),
        "12 degrees are {wide_12} pixels"
    );
    assert!(sum(wider.image.pixel(cx, cy).unwrap()) <= sum(middle));
}

#[test]
fn a_sun_disc_needs_a_light_above_the_horizon() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();

    // No light: the sky says a disc is wanted and none can be drawn.
    let no_light = horizon_scene(json!({ "sun_size_deg": 5 }), None, true);
    let bare = horizon_scene(json!({}), None, true);
    let frame = renderer.render(&gpu, &no_light, None, 160, 120).unwrap();
    let codes: Vec<&str> = frame.warnings.iter().map(|w| w.code.as_ref()).collect();
    assert_eq!(codes, ["sun_disc_without_light"]);
    assert!(frame.warnings[0].message.contains("Sky.sun_size_deg"));
    assert!(frame.warnings[0].hint.contains("DirectionalLight"));
    assert!(!frame.stats.effects.sun_disc);
    let plain = renderer.render(&gpu, &bare, None, 160, 120).unwrap();
    assert!(plain.warnings.is_empty());
    assert_eq!(frame.image, plain.image);
    // `describe` says it too.
    let description = renderer.describe(&gpu, &no_light, None, 160, 120).unwrap();
    assert!(
        description
            .warnings
            .iter()
            .any(|w| w.code == "sun_disc_without_light"),
        "{:?}",
        description.warnings
    );

    // A sun that has set: below the horizon the gradient is the ground, and
    // no disc is drawn into it (no ground here, to see that part).
    let set = horizon_scene(json!({ "sun_size_deg": 5 }), Some(SUN_SET), false);
    let set_plain = horizon_scene(json!({}), Some(SUN_SET), false);
    let with = renderer.render(&gpu, &set, None, 160, 120).unwrap();
    let without = renderer.render(&gpu, &set_plain, None, 160, 120).unwrap();
    assert!(with.warnings.is_empty());
    // To 1/255, as above: the two pictures come from two functions.
    let off = with.image.diff(&without.image, 1).expect("same size");
    assert_eq!(off.differing_pixels, 0);
}

// ---------------------------------------------------------------------------
// Fog

/// Red blocks on a wide ground: a post near the camera, a cube far away, and
/// one as far and high up.
fn fog_scene(fog: Value) -> RenderScene {
    let red = json!({ "base_color": [0.8, 0.1, 0.08, 1], "roughness": 0.6 });
    prepare(&scene(
        json!({ "Fog": fog }),
        json!({
            "camera": { "components": {
                "Camera": { "fov_y_deg": 45 },
                "Transform": { "translation": [0, 2, 0] } } },
            "ground": { "components": {
                "Material": { "base_color": [0.45, 0.45, 0.42, 1], "roughness": 0.9 },
                "Primitive": { "shape": "Plane" },
                "Transform": { "scale": [2000, 1, 2000] } } },
            "near": { "components": {
                "Material": red, "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [-1, 1, -4], "scale": [1, 2, 1] } } },
            "far": { "components": {
                "Material": red, "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [6, 3, -80], "scale": [6, 6, 6] } } },
            "high": { "components": {
                "Material": red, "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [-14, 30, -80], "scale": [6, 6, 6] } } },
            "sun": { "components": {
                "DirectionalLight": {},
                "Transform": { "rotation": SUN_BEHIND } } }
        }),
    ))
}

const NEAR: Vec3 = Vec3::new(-1.0, 1.0, -3.5);
const FAR: Vec3 = Vec3::new(6.0, 3.0, -77.0);
const HIGH: Vec3 = Vec3::new(-14.0, 30.0, -77.0);

/// How much of the way from `plain` to `fog` the pixel `fogged` has gone, 0
/// to 1, measured on the channel that has furthest to go.
fn share(plain: [u8; 4], fogged: [u8; 4], fog: [u8; 4]) -> f32 {
    let channel = (0..3)
        .max_by_key(|i| (i32::from(plain[*i]) - i32::from(fog[*i])).abs())
        .unwrap();
    let whole = f32::from(fog[channel]) - f32::from(plain[channel]);
    (f32::from(fogged[channel]) - f32::from(plain[channel])) / whole
}

#[test]
fn fog_takes_far_things_to_its_colour_and_leaves_near_ones() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let draw = |fog: Value| {
        let scene = fog_scene(fog);
        let frame = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
        (frame, scene)
    };
    let (plain, scene) = draw(json!({}));
    let at = |frame: &Frame, point: Vec3| pixel_at(frame, &scene, point);
    // The cubes are where the test thinks they are: red.
    for point in [NEAR, FAR, HIGH] {
        let pixel = at(&plain, point);
        assert!(rgb(pixel)[0] > 2 * rgb(pixel)[1], "{point}: {pixel:?}");
    }

    // What the fog's colour looks like in the picture: the far cube in a fog
    // so thick that nothing shows through. It is the horizon's colour.
    let (solid, _) = draw(json!({ "density": 1 }));
    assert!(solid.stats.effects.fog && !plain.stats.effects.fog);
    let fog = at(&solid, FAR);
    assert!(fog[2] > fog[0] && fog[0] > 120, "the fog's colour: {fog:?}");
    assert_eq!(at(&solid, HIGH), fog);
    // The sky is fogged with everything else: the whole picture beyond the
    // near cube's surroundings is that one colour.
    assert_eq!(solid.image.pixel(160, 5).unwrap(), fog);

    // A light fog: 4 m away a tenth is gone, 77 m away nine tenths.
    let (light, _) = draw(json!({ "density": 0.03 }));
    let near = share(at(&plain, NEAR), at(&light, NEAR), fog);
    let far = share(at(&plain, FAR), at(&light, FAR), fog);
    assert!(near > 0.02 && near < 0.3, "the near cube went {near}");
    assert!(far > 0.75, "the far cube went {far}");

    // Fog that starts 10 m out leaves the near cube exactly as it was.
    let (late, _) = draw(json!({ "density": 0.03, "start": 10 }));
    assert_eq!(at(&late, NEAR), at(&plain, NEAR));
    assert!(share(at(&plain, FAR), at(&late, FAR), fog) > 0.7);

    // A limit: however thick, half of the far cube still shows.
    let (limited, _) = draw(json!({ "density": 1, "max_opacity": 0.5 }));
    let half = share(at(&plain, FAR), at(&limited, FAR), fog);
    assert!(half > 0.25 && half < 0.8, "limited to half: {half}");
    assert_ne!(limited.image.pixel(160, 5).unwrap(), fog, "the sky too");
}

#[test]
fn fog_thins_with_height_and_joins_the_ground_to_the_sky() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let draw = |fog: Value| {
        let scene = fog_scene(fog);
        let frame = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
        (frame, scene)
    };
    let (plain, scene) = draw(json!({}));
    let at = |frame: &Frame, point: Vec3| pixel_at(frame, &scene, point);
    let (solid, _) = draw(json!({ "density": 1 }));
    let fog = at(&solid, FAR);

    // With a height falloff the cube 30 m up, as far away as the low one,
    // keeps much more of its colour.
    let (layered, _) = draw(json!({ "density": 0.05, "height_falloff": 0.2 }));
    keep("look_layered_fog", &layered.image);
    let low = share(at(&plain, FAR), at(&layered, FAR), fog);
    let high = share(at(&plain, HIGH), at(&layered, HIGH), fog);
    assert!(low > 0.75, "the low cube went {low}");
    assert!(
        high < low - 0.3,
        "the high cube went {high}, the low one {low}"
    );
    // The same fog without the falloff takes both.
    let (level, _) = draw(json!({ "density": 0.05 }));
    assert!(share(at(&plain, HIGH), at(&level, HIGH), fog) > 0.9);
    // Under layered fog the sky clears towards the top: the top row is
    // nearer to the plain sky than the row at the horizon is.
    // (Column 290 is to the right of every block.)
    let (top, horizon) = (5, 118);
    let lifted = |frame: &Frame, y: u32| {
        apart(
            frame.image.pixel(290, y).unwrap(),
            plain.image.pixel(290, y).unwrap(),
        )
    };
    assert!(lifted(&layered, top) > 0, "the sky is fogged a little");
    assert!(
        apart(layered.image.pixel(290, horizon).unwrap(), fog) <= 2,
        "the sky at the horizon is the fog's colour"
    );
    assert!(
        apart(layered.image.pixel(290, top).unwrap(), fog) > 20,
        "the sky overhead is not"
    );

    // The seam: without fog the far ground and the sky above it are two
    // colours; in fog (a pink one, to be sure whose colour it is) they are
    // one, in the rows on both sides of the horizon.
    let (above, below) = (
        plain.image.pixel(290, 118).unwrap(),
        plain.image.pixel(290, 123).unwrap(),
    );
    assert!(apart(above, below) > 12, "{above:?} vs {below:?}");
    let (pink, _) = draw(json!({ "density": 0.05, "tint": [1, 0.5, 0.5, 1] }));
    keep("look_pink_fog", &pink.image);
    let (above, below) = (
        pink.image.pixel(290, 118).unwrap(),
        pink.image.pixel(290, 123).unwrap(),
    );
    assert!(apart(above, below) <= 2, "{above:?} vs {below:?}");
    assert!(above[0] > above[1] + 20, "the fog is pink: {above:?}");
}

#[test]
fn the_base_height_says_where_the_fog_is_thick() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let draw = |fog: Value| {
        let scene = fog_scene(fog);
        let frame = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
        (frame, scene)
    };
    let (plain, scene) = draw(json!({}));
    let at = |frame: &Frame, point: Vec3| pixel_at(frame, &scene, point);
    let (solid, _) = draw(json!({ "density": 1 }));
    let fog = at(&solid, FAR);
    // The far cube is 3 m up and the camera 2 m. A fog that thins by a tenth
    // a meter is, at their height, three quarters of its density with its
    // base at 0, a twentieth with its base 30 m below them, and many times
    // its density with its base 30 m above.
    let went = |base: f64| {
        let (frame, _) = draw(json!({
            "density": 0.01, "height_falloff": 0.1, "base_height": base
        }));
        share(at(&plain, FAR), at(&frame, FAR), fog)
    };
    let (below, level, above) = (went(-30.0), went(0.0), went(30.0));
    assert!(below < 0.3, "a base far below leaves the cube: {below}");
    assert!(level > 0.35 && level < 0.9, "a base at the ground: {level}");
    assert!(above > 0.97, "a base far above hides it: {above}");
}

/// A wide dark red wall 20 m in front of an orthographic camera that shows
/// 20 m of it from top to bottom, in a sky of one colour.
fn wall_scene(fog: Value) -> RenderScene {
    let sky = [0.5, 0.6, 0.8, 1.0];
    prepare(&scene(
        json!({
            "Ambient": { "intensity": 0 },
            "Sky": { "zenith": sky, "horizon": sky, "ground": sky },
            "Fog": fog
        }),
        json!({
            "camera": { "components": {
                "Camera": { "projection": "Orthographic", "ortho_height": 20 },
                "Transform": {} } },
            "wall": { "components": {
                "Material": { "base_color": [0, 0, 0, 1], "roughness": 1, "emissive": [0.3, 0.02, 0.02] },
                "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [0, 0, -20.05], "scale": [200, 200, 0.1] } } }
        }),
    ))
}

#[test]
fn under_an_orthographic_camera_fog_goes_by_depth() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let draw = |fog: Value| {
        renderer
            .render(&gpu, &wall_scene(fog), None, 320, 240)
            .unwrap()
    };
    let plain = draw(json!({}));
    let solid = draw(json!({ "density": 1 }));
    let fog = solid.image.pixel(160, 120).unwrap();
    let went = |frame: &Frame, x: u32, y: u32| {
        share(
            plain.image.pixel(x, y).unwrap(),
            frame.image.pixel(x, y).unwrap(),
            fog,
        )
    };

    // The wall is 20 m deep everywhere in the picture, so the fog takes the
    // same share of it in the middle and in a corner: 1 - exp(-0.05 * 20),
    // 63% of the light. (Measured from the camera's position the corner
    // would be 26 m away and lose 73%.) The share is of the picture's
    // encoded bytes, which run ahead of the light.
    let even = draw(json!({ "density": 0.05 }));
    let (middle, corner) = (went(&even, 160, 120), went(&even, 6, 6));
    assert!(middle > 0.5 && middle < 0.95, "the middle went {middle}");
    assert!(
        (middle - corner).abs() < 0.02,
        "the middle went {middle}, the corner {corner}"
    );

    // With a height falloff each pixel's ray runs level at its own height:
    // the top of the picture (7 m above the fog's base) is clearer than the
    // bottom (7 m below it).
    let layered = draw(json!({ "density": 0.05, "height_falloff": 0.2 }));
    let (top, bottom) = (went(&layered, 160, 36), went(&layered, 160, 204));
    assert!(
        top < bottom - 0.3,
        "the top went {top}, the bottom {bottom}"
    );
}

// ---------------------------------------------------------------------------
// Colour grading

/// The scene of the lit golden image (`render.rs`).
fn basic_scene(resources: Value) -> RenderScene {
    prepare(&scene(
        resources,
        json!({
            "camera": { "components": {
                "Camera": { "fov_y_deg": 45 },
                "Transform": { "translation": [0, 1.6, 6], "rotation": [-0.087_155_7, 0, 0, 0.996_194_7] } } },
            "cube": { "components": {
                "Material": { "base_color": [0.8, 0.1, 0.08, 1], "roughness": 0.6 },
                "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [-1.1, 0.5, 0], "rotation": [0, 0.382_683_4, 0, 0.923_879_5] } } },
            "ground": { "components": {
                "Material": { "base_color": [0.45, 0.45, 0.42, 1], "roughness": 0.9 },
                "Primitive": { "shape": "Plane" },
                "Transform": { "scale": [30, 1, 30] } } },
            "orb": { "components": {
                "Material": { "base_color": [0.95, 0.75, 0.35, 1], "metallic": 1, "roughness": 0.25 },
                "Primitive": { "shape": "Sphere" },
                "Transform": { "translation": [1.1, 0.6, 0], "scale": [1.2, 1.2, 1.2] } } },
            "sun": { "components": {
                "DirectionalLight": { "illuminance_lux": 100000 },
                "Transform": { "rotation": SUN_BEHIND } } }
        }),
    ))
}

#[test]
fn grading_changes_contrast_saturation_balance_and_the_corners() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let draw = |grading: Value| {
        let scene = basic_scene(json!({ "ColorGrading": grading }));
        let frame = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
        (frame, scene)
    };
    let (plain, scene) = draw(json!({}));
    assert!(!plain.stats.effects.color_grading);
    // Lit ground in front, and ground in the orb's shadow (where the sun's
    // ray through its top lands).
    let lit = Vec3::new(0.0, 0.0, 2.5);
    let sun = scene.lights[0].direction;
    let top = Vec3::new(1.1, 1.2, 0.0);
    let shaded = top + sun * (-top.y / sun.y);
    let at = |frame: &Frame, point: Vec3| pixel_at(frame, &scene, point);
    let (lit_plain, shaded_plain) = (at(&plain, lit), at(&plain, shaded));
    assert!(sum(lit_plain) > 3 * 150 && sum(shaded_plain) < 3 * 110);

    // Saturation 0: black and white, everywhere.
    let (grey, _) = draw(json!({ "saturation": 0 }));
    assert!(grey.stats.effects.color_grading);
    let (pixels, _) = grey.image.pixels.as_chunks::<4>();
    let coloured = pixels
        .iter()
        .filter(|p| p[0].abs_diff(p[1]) > 1 || p[1].abs_diff(p[2]) > 1)
        .count();
    assert_eq!(coloured, 0, "no pixel has a colour left");
    // More of it: the red cube's top is redder.
    let cube = Vec3::new(-1.1, 1.0, 0.0);
    let (vivid, _) = draw(json!({ "saturation": 1.6 }));
    let spread = |p: [u8; 4]| i32::from(p[0]) - i32::from(p[1].min(p[2]));
    assert!(spread(at(&vivid, cube)) > spread(at(&plain, cube)) + 8);

    // Contrast: what is brighter than middle grey gets brighter, what is
    // darker gets darker.
    let (punchy, _) = draw(json!({ "contrast": 1.5 }));
    assert!(
        sum(at(&punchy, lit)) > sum(lit_plain) + 9,
        "{:?}",
        at(&punchy, lit)
    );
    assert!(sum(at(&punchy, shaded)) < sum(shaded_plain) - 9);
    let (flat, _) = draw(json!({ "contrast": 0.6 }));
    assert!(sum(at(&flat, lit)) < sum(lit_plain) - 9);
    assert!(sum(at(&flat, shaded)) > sum(shaded_plain) + 9);

    // White balance: the grey ground turns warm or cool, green or magenta.
    let warmth = |p: [u8; 4]| i32::from(p[0]) - i32::from(p[2]);
    let (warm, _) = draw(json!({ "temperature": 0.6 }));
    let (cool, _) = draw(json!({ "temperature": -0.6 }));
    assert!(warmth(at(&warm, lit)) > warmth(lit_plain) + 15);
    assert!(warmth(at(&cool, lit)) < warmth(lit_plain) - 15);
    let greenness = |p: [u8; 4]| 2 * i32::from(p[1]) - i32::from(p[0]) - i32::from(p[2]);
    let (magenta, _) = draw(json!({ "tint": 0.6 }));
    let (green, _) = draw(json!({ "tint": -0.6 }));
    assert!(greenness(at(&magenta, lit)) < greenness(lit_plain) - 15);
    assert!(greenness(at(&green, lit)) > greenness(lit_plain) + 15);

    // The vignette: the corners go dark, the middle stays.
    let (lens, _) = draw(json!({ "vignette": 1 }));
    for (x, y) in [(0, 0), (319, 0), (0, 239), (319, 239)] {
        let corner = lens.image.pixel(x, y).unwrap();
        assert!(sum(corner) <= 9, "corner ({x}, {y}): {corner:?}");
    }
    assert!(
        apart(
            lens.image.pixel(160, 120).unwrap(),
            plain.image.pixel(160, 120).unwrap()
        ) <= 1
    );
    let (edge_lens, edge_plain) = (
        lens.image.pixel(160, 2).unwrap(),
        plain.image.pixel(160, 2).unwrap(),
    );
    assert!(
        sum(edge_lens) < sum(edge_plain) - 30 && sum(edge_lens) > 60,
        "the middle of the top edge is dimmed, not black: {edge_lens:?} vs {edge_plain:?}"
    );
}

// ---------------------------------------------------------------------------
// FXAA

/// A bright tile, turned 25 degrees, in front of a flat dark sky: two
/// colours and four slanted edges.
fn tile_scene(antialiasing: &str) -> RenderScene {
    let dark = [0.02, 0.03, 0.08, 1.0];
    prepare(&scene(
        json!({
            "Ambient": { "intensity": 0 },
            "Sky": { "zenith": dark, "horizon": dark, "ground": dark },
            "RenderSettings": { "antialiasing": antialiasing }
        }),
        json!({
            "camera": { "components": {
                "Camera": { "fov_y_deg": 45 },
                "Transform": { "translation": [0, 0, 5] } } },
            "tile": { "components": {
                "Material": { "base_color": [0, 0, 0, 1], "roughness": 1, "emissive": [0.4, 0.4, 0.4] },
                "Primitive": { "shape": "Cube" },
                "Transform": { "rotation": [0, 0, 0.216_439_6, 0.976_296], "scale": [2, 2, 0.1] } } }
        }),
    ))
}

#[test]
fn fxaa_smooths_slanted_edges_and_leaves_flat_areas_alone() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let plain = renderer
        .render(&gpu, &tile_scene("Off"), None, 320, 240)
        .unwrap();
    let made_plain = gpu.created();
    let smooth = renderer
        .render(&gpu, &tile_scene("Fxaa"), None, 320, 240)
        .unwrap();
    assert!(smooth.stats.effects.fxaa && !plain.stats.effects.fxaa);

    // One more pass: a triangle over the picture, through one more target.
    assert_eq!(smooth.stats.draw_calls, plain.stats.draw_calls + 1);
    assert_eq!(smooth.stats.triangles, plain.stats.triangles + 1);
    let made_smooth = gpu.created();
    assert_eq!(
        (
            made_smooth.textures - made_plain.textures,
            made_smooth.buffers - made_plain.buffers
        ),
        (1, 0),
        "the tone-mapped picture, and nothing else"
    );
    let again = renderer
        .render(&gpu, &tile_scene("Fxaa"), None, 320, 240)
        .unwrap();
    assert_eq!(gpu.created(), made_smooth, "a second frame creates nothing");
    assert_eq!(again.image, smooth.image);

    // Without it a pixel is the tile or the sky; with it the edges have
    // pixels in between.
    let (dark, bright) = (
        sum(plain.image.pixel(5, 5).unwrap()),
        sum(plain.image.pixel(160, 120).unwrap()),
    );
    assert!(bright > dark + 300, "tile {bright}, sky {dark}");
    let between = |image: &RgbaImage| {
        let (pixels, _) = image.pixels.as_chunks::<4>();
        pixels
            .iter()
            .filter(|p| {
                let value = sum(**p);
                value > dark + 60 && value < bright - 60
            })
            .count()
    };
    assert_eq!(between(&plain.image), 0);
    let mended = between(&smooth.image);
    assert!(mended > 60, "{mended} pixels in between");

    // A pixel whose neighbours all equal it is written back as it was read,
    // to the byte; and that is most of this picture.
    let (mut flat, mut kept) = (0, 0);
    for y in 1..239u32 {
        for x in 1..319u32 {
            let centre = plain.image.pixel(x, y).unwrap();
            let uniform = (-1i32..=1).all(|dy| {
                (-1i32..=1).all(|dx| {
                    plain
                        .image
                        .pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32)
                        .unwrap()
                        == centre
                })
            });
            if uniform {
                flat += 1;
                if smooth.image.pixel(x, y).unwrap() == centre {
                    kept += 1;
                }
            }
        }
    }
    assert!(flat > 60_000, "{flat} flat pixels");
    assert_eq!(kept, flat, "every flat pixel is unchanged");
    // Only pixels at the edges changed at all.
    let changed = differing(&smooth.image, &plain.image);
    assert!(changed > 100 && changed < 3000, "{changed} pixels changed");
}

// ---------------------------------------------------------------------------
// What is drawn after the look

/// A lit scene with a sprite standing in it and an interface panel over it.
fn layered_scene(resources: Value) -> RenderScene {
    let mut entities = look_entities();
    entities["flag"] = json!({ "components": {
        "Sprite": { "color": [0.1, 0.9, 0.2, 1], "rect": [0, 0, 300, 300] },
        "Transform": { "translation": [0, 3, -20] } } });
    entities["panel"] = json!({ "components": { "UiNode": {
        "position": "Absolute", "offset": [40, 40], "padding": [20, 20, 20, 20],
        "background": [0.9, 0.1, 0.5, 1] } } });
    entities["label"] = json!({ "parent": "panel", "components": {
        "UiNode": {}, "UiText": { "text": "Sharp", "size": 96 } } });
    prepare(&scene(resources, entities))
}

#[test]
fn sprites_and_the_interface_are_drawn_after_the_look_and_keep_their_colours() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let plain_scene = layered_scene(json!({}));
    let plain = renderer.render(&gpu, &plain_scene, None, 320, 200).unwrap();
    // A thick fog, no colour, dark corners, smoothed edges.
    let look_scene = layered_scene(json!({
        "Fog": { "density": 0.2 },
        "ColorGrading": { "saturation": 0, "vignette": 1 },
        "RenderSettings": { "antialiasing": "Fxaa" }
    }));
    let look = renderer.render(&gpu, &look_scene, None, 320, 200).unwrap();
    assert_eq!(look.stats.effects.names(), ["fog", "color_grading", "fxaa"]);

    // The interface, in the picture's top left corner where the vignette is
    // strongest: every pixel of the panel, its text's edges included, is
    // what it was.
    let panel = wish_render::ui_elements(&plain_scene, 320, 200)
        .into_iter()
        .find(|element| element.text.is_none())
        .expect("the panel");
    let [left, top, right, bottom] = panel.bounding_box.map(|v| v as u32);
    assert!(
        right > left + 20 && bottom > top + 10,
        "{:?}",
        panel.bounding_box
    );
    let mut text_edges = 0;
    for y in top + 1..bottom - 1 {
        for x in left + 1..right - 1 {
            let pixel = plain.image.pixel(x, y).unwrap();
            assert_eq!(look.image.pixel(x, y).unwrap(), pixel, "({x}, {y})");
            // Neither the panel's pink nor the text's white: an edge of a
            // letter.
            if pixel[1] > 60 && pixel[1] < 200 {
                text_edges += 1;
            }
        }
    }
    assert!(text_edges > 20, "the text has edges to keep: {text_edges}");

    // The sprite, 27 m into a fog that leaves nothing of the ground there:
    // its middle is its own green in both pictures.
    let flag = Vec3::new(0.0, 3.0, -20.0);
    let pixel = pixel_at(&look, &look_scene, flag);
    assert_eq!(pixel, pixel_at(&plain, &plain_scene, flag));
    assert!(
        pixel[1] > 200 && pixel[0] < 120,
        "the flag is green: {pixel:?}"
    );
    // While the picture around them did change: the ground beside the flag
    // is fog now, and grey.
    let beside = pixel_at(&look, &look_scene, Vec3::new(6.0, 0.0, -20.0));
    assert!(beside[0].abs_diff(beside[1]) <= 1 && beside[1].abs_diff(beside[2]) <= 1);
    assert_ne!(
        beside,
        pixel_at(&plain, &plain_scene, Vec3::new(6.0, 0.0, -20.0))
    );
}

#[test]
fn fxaa_draws_pictures_one_pixel_wide_or_high() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // The pass reads its neighbours with their places held inside the
    // picture. A single pixel has no edge and stays as it was (to 1/255: it
    // is read as light and written back). A strip has the tile's two edges
    // along it: a few pixels there are blended with their neighbours, and
    // nothing in the strip is a colour from outside the picture.
    for (width, height) in [(1u32, 1u32), (1, 48), (48, 1)] {
        let plain = renderer
            .render(&gpu, &tile_scene("Off"), None, width, height)
            .unwrap();
        let smooth = renderer
            .render(&gpu, &tile_scene("Fxaa"), None, width, height)
            .unwrap();
        assert!(smooth.stats.effects.fxaa);
        assert!(smooth.warnings.is_empty(), "{:?}", smooth.warnings);
        let off = smooth.image.diff(&plain.image, 1).expect("same size");
        let most = if width * height == 1 { 0 } else { 8 };
        assert!(
            off.differing_pixels <= most,
            "{width}x{height}: {} pixels changed",
            off.differing_pixels
        );
        let places = || (0..height).flat_map(|y| (0..width).map(move |x| (x, y)));
        for channel in 0..3 {
            let of = |frame: &Frame, (x, y): (u32, u32)| {
                i32::from(frame.image.pixel(x, y).unwrap()[channel])
            };
            let darkest = places().map(|at| of(&plain, at)).min().unwrap();
            let brightest = places().map(|at| of(&plain, at)).max().unwrap();
            for at in places() {
                let value = of(&smooth, at);
                assert!(
                    value >= darkest - 1 && value <= brightest + 1,
                    "{width}x{height} at {at:?}: {value} is outside {darkest}..={brightest}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Numbers game code can write

#[test]
fn settings_that_are_not_numbers_draw_the_picture_of_their_fallbacks() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let plain = renderer
        .render(&gpu, &look_scene(json!({})), None, 160, 100)
        .unwrap();

    // A scene file cannot hold these (the loader checks ranges); game code
    // can write them into the resources.
    let mut broken = look_scene(json!({}));
    broken.sky.sun_size_deg = f32::NAN;
    broken.fog.density = f32::INFINITY;
    broken.fog.height_falloff = f32::NAN;
    broken.fog.base_height = f32::NEG_INFINITY;
    broken.fog.start = f32::NAN;
    broken.fog.max_opacity = f32::NAN;
    broken.grading.contrast = f32::NAN;
    broken.grading.saturation = f32::INFINITY;
    broken.grading.temperature = f32::NAN;
    broken.grading.tint = f32::NEG_INFINITY;
    broken.grading.vignette = f32::NAN;
    let frame = renderer.render(&gpu, &broken, None, 160, 100).unwrap();
    assert_eq!(frame.image, plain.image, "each falls back to its default");
    assert_eq!(frame.stats.effects, Effects::default());

    // Out of range: held to the range's end, and the picture is the one of
    // that end.
    let mut extreme = look_scene(json!({}));
    extreme.sky.sun_size_deg = 1.0e9;
    extreme.sky_light = Some(f32::NAN);
    extreme.fog.density = 55.0;
    extreme.fog.height_falloff = -3.0;
    extreme.fog.start = -100.0;
    extreme.fog.max_opacity = 7.0;
    extreme.grading.contrast = 1.0e9;
    extreme.grading.saturation = -4.0;
    extreme.grading.temperature = 1.0e9;
    extreme.grading.tint = -1.0e9;
    extreme.grading.vignette = 12.0;
    let held = look_scene(json!({
        "Ambient": { "source": "Sky" },
        "Sky": { "sun_size_deg": 45 },
        "Fog": { "density": 1 },
        "ColorGrading": { "contrast": 2, "saturation": 0, "temperature": 1, "tint": -1, "vignette": 1 }
    }));
    let frame = renderer.render(&gpu, &extreme, None, 160, 100).unwrap();
    let expected = renderer.render(&gpu, &held, None, 160, 100).unwrap();
    assert_eq!(frame.image, expected.image);
    assert_eq!(frame.stats.effects, expected.stats.effects);
}
