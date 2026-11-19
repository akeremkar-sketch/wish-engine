//! See-through surfaces: the blended pass, on the software adapter (design
//! note M12-blended-pass.md, section 4).
//!
//! The scenes are small and flat-lit (an orthographic camera looking at
//! surfaces that face it, a sun shining along the view), so that every pixel
//! of a surface has the same colour and a mix can be measured over a window
//! of pixels in linear light.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::FRAC_1_SQRT_2;
use std::path::PathBuf;

use serde_json::{Value, json};
use wish_ecs::World;
use wish_render::{
    Frame, Gpu, GpuOptions, Look, MaterialFile, RenderScene, Renderer, RgbaImage, View, extract,
};
use wish_scene::{SceneEntities, load};
use wish_schema::Plain;

const SIZE: u32 = 64;
/// The material file the tests' see-through surfaces name.
const FILE: &str = "assets/glass.mat.json";

fn prepare(resources: &Value, entities: &Value) -> (RenderScene, SceneEntities) {
    let source = json!({ "format": 1, "resources": resources, "entities": entities }).to_string();
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    let result = load(&source, "scenes/blended.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let entities = result.document.unwrap().instantiate(&mut world).unwrap();
    wish_transform::update_global_transforms(&mut world).unwrap();
    (extract(&world).unwrap(), entities)
}

fn gpu() -> std::sync::Arc<Gpu> {
    Gpu::shared(GpuOptions { software: true }).unwrap()
}

/// The look of a material file with these fields, read through the schema
/// as the file is.
fn look(material: &Value) -> Look {
    let schema = MaterialFile::schema();
    let mut texts = wish_ecs::TextTable::default();
    let bytes = {
        let mut context = wish_schema::JsonContext::new(&mut texts);
        wish_schema::from_json(&schema, material, &mut context).unwrap()
    };
    let file = MaterialFile::decode(&bytes).unwrap();
    Look::of(&file, &|reference| {
        texts.get(reference.text()).map(str::to_owned)
    })
}

/// A renderer that holds the material file [`FILE`] with these fields.
fn renderer_with(gpu: &Gpu, material: &Value) -> Renderer {
    let mut renderer = Renderer::new(gpu).unwrap();
    renderer.set_material(FILE, look(material));
    renderer
}

/// Keeps what a test drew in `$CARGO_TARGET_TMPDIR/captures/`, to look at.
fn keep(name: &str, image: &RgbaImage) {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("captures");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join(format!("blended_{name}.png")),
        image.to_png().unwrap(),
    )
    .unwrap();
}

fn codes(frame: &Frame) -> Vec<&str> {
    frame.warnings.iter().map(|w| w.code.as_ref()).collect()
}

/// The studio every flat test uses: an orthographic camera at z = 5 looking
/// down -Z and showing 4 m, a sun shining along -Z (so every face towards
/// the camera is lit alike), and a backdrop: a wide slab at z = -1 of
/// `backdrop` colour.
fn studio(backdrop: [f32; 4]) -> Value {
    json!({
        "camera": { "components": {
            "Camera": { "projection": "Orthographic", "ortho_height": 4, "near": 0.1, "far": 50 },
            "Transform": { "translation": [0, 0, 5] } } },
        "sun": { "components": {
            "DirectionalLight": { "illuminance_lux": 40000 },
            "Transform": {} } },
        "backdrop": { "components": {
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": backdrop, "roughness": 1 },
            "Transform": { "translation": [0, 0, -1], "scale": [8, 8, 0.1] } } }
    })
}

/// A flat pane of `material` (a `Material`'s fields), 2 m by 2 m, its face
/// towards the camera at `z`: it covers the middle half of the picture.
fn pane(material: Value, z: f32) -> Value {
    json!({ "components": {
        "Primitive": { "shape": "Cube" },
        "Material": material,
        "Transform": { "translation": [0, 0, z], "scale": [2, 2, 0.05] } } })
}

fn with(mut entities: Value, name: &str, entity: Value) -> Value {
    entities[name] = entity;
    entities
}

fn draw(renderer: &Renderer, scene: &RenderScene) -> Frame {
    renderer.render(&gpu(), scene, None, SIZE, SIZE).unwrap()
}

fn lit(entities: &Value) -> Frame {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    draw(&renderer, &prepare(&json!({}), entities).0)
}

fn srgb_to_linear(byte: f64) -> f64 {
    let c = byte / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(linear: f64) -> f64 {
    let c = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    c * 255.0
}

/// The mean colour of the 8x8 pixels around (x, y), in linear light.
fn mean_linear(image: &RgbaImage, (x, y): (u32, u32)) -> [f64; 3] {
    let mut sum = [0.0; 3];
    for dy in 0..8 {
        for dx in 0..8 {
            let pixel = image.pixel(x - 4 + dx, y - 4 + dy).unwrap();
            for (total, channel) in sum.iter_mut().zip(pixel) {
                *total += srgb_to_linear(f64::from(channel));
            }
        }
    }
    sum.map(|total| total / 64.0)
}

const MIDDLE: (u32, u32) = (32, 32);
const CORNER: (u32, u32) = (6, 6);

#[test]
fn a_see_through_surface_mixes_with_what_is_behind_by_its_alpha() {
    let front = [0.55, 0.12, 0.1];
    let back = [0.1, 0.25, 0.45];
    let backdrop = [back[0], back[1], back[2], 1.0];
    let pane_of = |alpha: f32| {
        pane(
            json!({ "base_color": [front[0], front[1], front[2], alpha], "roughness": 1 }),
            0.0,
        )
    };
    // The two colours on their own: the pane solid, and the backdrop alone.
    let solid = lit(&with(studio(backdrop), "pane", pane_of(1.0)));
    let alone = lit(&studio(backdrop));
    let f = mean_linear(&solid.image, MIDDLE);
    let b = mean_linear(&alone.image, MIDDLE);
    // Both are where tone mapping is a straight line (output = input -
    // 0.04), so the mix of the outputs is the mix of the light.
    for value in f.iter().chain(&b) {
        assert!((0.05..0.70).contains(value), "{f:?} {b:?}");
    }
    assert_eq!(solid.stats.blended_surfaces, 0);
    for alpha in [0.25f32, 0.5, 0.75] {
        let frame = lit(&with(studio(backdrop), "pane", pane_of(alpha)));
        keep(&format!("mix_{alpha}"), &frame.image);
        assert_eq!(frame.stats.blended_surfaces, 1);
        assert!(codes(&frame).is_empty(), "{:?}", frame.warnings);
        let measured = mean_linear(&frame.image, MIDDLE);
        let a = f64::from(alpha);
        for channel in 0..3 {
            let expected = a * f[channel] + (1.0 - a) * b[channel];
            let (expected, measured) =
                (linear_to_srgb(expected), linear_to_srgb(measured[channel]));
            assert!(
                (expected - measured).abs() <= 1.0,
                "alpha {alpha}, channel {channel}: {measured:.2} of 255, expected {expected:.2}"
            );
        }
        // Around the pane the backdrop is as it is alone.
        assert_eq!(
            frame.image.pixel(CORNER.0, CORNER.1),
            alone.image.pixel(CORNER.0, CORNER.1)
        );
    }
}

#[test]
fn a_material_file_that_blends_is_see_through_and_one_that_does_not_is_not() {
    let gpu = gpu();
    let backdrop = [0.1, 0.25, 0.45, 1.0];
    let entities = with(studio(backdrop), "pane", pane(json!({ "file": FILE }), 0.0));
    let (scene, _) = prepare(&json!({}), &entities);
    let draw_with = |material: Value| draw(&renderer_with(&gpu, &material), &scene);
    let blended = draw_with(json!({ "base_color": [0.55, 0.12, 0.1, 0.5], "alpha_mode": "Blend" }));
    let opaque = draw_with(json!({ "base_color": [0.55, 0.12, 0.1, 0.5] }));
    let plain = lit(&with(
        studio(backdrop),
        "pane",
        pane(json!({ "base_color": [0.55, 0.12, 0.1, 0.5] }), 0.0),
    ));
    assert_eq!(blended.stats.blended_surfaces, 1);
    assert_eq!(opaque.stats.blended_surfaces, 0);
    // A file's alpha does nothing with `Opaque`; with `Blend` it is the same
    // half mix as a plain Material's alpha of one half.
    assert_ne!(blended.image, opaque.image);
    assert_eq!(blended.image, plain.image);
    keep("file", &blended.image);
}

#[test]
fn nothing_asks_for_blending_nothing_changes() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let backdrop = [0.1, 0.25, 0.45, 1.0];
    let opaque = with(
        studio(backdrop),
        "pane",
        pane(json!({ "base_color": [0.55, 0.12, 0.1, 1.0] }), 0.0),
    );
    let (plain, _) = prepare(&json!({}), &opaque);
    let first = draw(&renderer, &plain);
    assert_eq!(first.stats.blended_surfaces, 0);
    // An alpha above 1 or not a number is solid, as it always was.
    let mut above = opaque.clone();
    above["pane"]["components"]["Material"]["base_color"][3] = json!(5.0);
    assert_eq!(lit(&above).image, first.image);
    let mut not_a_number = prepare(&json!({}), &opaque).0;
    for draw in &mut not_a_number.draws {
        if draw.material.base_color != [1.0; 4] {
            draw.material.base_color[3] = f32::NAN;
        }
    }
    let frame = draw(&renderer, &not_a_number);
    assert_eq!(
        (frame.image, frame.stats.blended_surfaces),
        (first.image.clone(), 0)
    );
    // A renderer that drew see-through surfaces draws the solid scene as
    // it did before.
    let glass = with(
        studio(backdrop),
        "pane",
        pane(json!({ "base_color": [0.55, 0.12, 0.1, 0.5] }), 0.0),
    );
    let blended = draw(&renderer, &prepare(&json!({}), &glass).0);
    assert_ne!(blended.image, first.image);
    let again = draw(&renderer, &plain);
    assert_eq!(again.image, first.image);
    assert_eq!(again.stats, first.stats);
}

/// Two overlapping panes, red nearer the camera than blue, both half
/// see-through, named and listed in the given order.
fn two_panes(first: &str, second: &str, red_first: bool) -> Value {
    two_panes_at(first, second, red_first, (0.5, 0.0))
}

/// The same, with red at depth `z.0` and blue at `z.1`.
fn two_panes_at(first: &str, second: &str, red_first: bool, z: (f32, f32)) -> Value {
    let red = json!({ "components": {
        "Primitive": { "shape": "Cube" },
        "Material": { "base_color": [0.6, 0.05, 0.05, 0.5], "roughness": 1 },
        "Transform": { "translation": [-0.4, 0, z.0], "scale": [2, 2, 0.05] } } });
    let blue = json!({ "components": {
        "Primitive": { "shape": "Cube" },
        "Material": { "base_color": [0.05, 0.05, 0.6, 0.5], "roughness": 1 },
        "Transform": { "translation": [0.4, 0, z.1], "scale": [2, 2, 0.05] } } });
    let (a, b) = if red_first { (red, blue) } else { (blue, red) };
    with(with(studio([0.2, 0.2, 0.2, 1.0]), first, a), second, b)
}

#[test]
fn overlapping_surfaces_are_drawn_farthest_first_whatever_their_order() {
    let pictures: Vec<Frame> = [("a", "b", true), ("a", "b", false), ("b", "a", true)]
        .into_iter()
        .map(|(first, second, red_first)| lit(&two_panes(first, second, red_first)))
        .collect();
    keep("order", &pictures[0].image);
    for frame in &pictures[1..] {
        assert_eq!(frame.image, pictures[0].image);
        assert_eq!(frame.stats.blended_surfaces, 2);
    }
    // Where they overlap the nearer red is on top: half of it, a quarter of
    // the blue behind. With blue nearer it is the other way round.
    let [r, _, b, _] = pictures[0].image.pixel(32, 32).unwrap();
    assert!(r > b + 15, "red {r}, blue {b}");
    let swapped = lit(&two_panes_at("a", "b", true, (0.0, 0.5)));
    let [r, _, b, _] = swapped.image.pixel(32, 32).unwrap();
    assert!(b > r + 15, "red {r}, blue {b}");
    // Where each is alone it is its own colour over the backdrop.
    let [r, _, b, _] = pictures[0].image.pixel(14, 32).unwrap();
    assert!(r > b + 40, "red {r}, blue {b}");
    let [r, _, b, _] = pictures[0].image.pixel(50, 32).unwrap();
    assert!(b > r + 40, "red {r}, blue {b}");
}

#[test]
fn a_solid_surface_in_front_hides_glass_and_one_behind_shows_through() {
    let backdrop = [0.2, 0.2, 0.2, 1.0];
    let glass = pane(
        json!({ "base_color": [0.1, 0.6, 0.1, 0.4], "roughness": 1 }),
        0.0,
    );
    let block = |z: f32| {
        json!({ "components": {
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.6, 0.1, 0.1, 1], "roughness": 1 },
            "Transform": { "translation": [0.5, 0, z], "scale": [1, 1, 0.2] } } })
    };
    // In front: where the block is, the picture is the picture without the
    // glass.
    let front = lit(&with(
        with(studio(backdrop), "glass", glass.clone()),
        "block",
        block(1.0),
    ));
    let front_alone = lit(&with(studio(backdrop), "block", block(1.0)));
    keep("in_front", &front.image);
    assert_eq!(front.image.pixel(42, 32), front_alone.image.pixel(42, 32));
    // Behind: the block shows through, tinted by the glass.
    let behind = lit(&with(
        with(studio(backdrop), "glass", glass.clone()),
        "block",
        block(-0.5),
    ));
    let behind_alone = lit(&with(studio(backdrop), "block", block(-0.5)));
    let glass_alone = lit(&with(studio(backdrop), "glass", glass));
    keep("behind", &behind.image);
    let seen = behind.image.pixel(42, 32).unwrap();
    assert_ne!(Some(seen), behind_alone.image.pixel(42, 32));
    assert_ne!(Some(seen), glass_alone.image.pixel(42, 32));
    // More red than the glass over the grey backdrop: the block is there.
    let over_grey = glass_alone.image.pixel(42, 32).unwrap();
    assert!(seen[0] > over_grey[0] + 20, "{seen:?} {over_grey:?}");
}

/// A see-through floor tile under a sun that comes in from the side (its
/// light travels down and towards +X, 30 degrees from straight down), seen
/// from straight above; a small solid block hangs above the tile's left
/// half, so its shadow falls on the tile beside it. The ground is 1 m below
/// the tile. `tile_alpha` 1 makes the tile solid.
fn shaded(resources: &Value, tile_alpha: f32) -> Frame {
    let entities = json!({
        "camera": { "components": {
            "Camera": { "projection": "Orthographic", "ortho_height": 4, "near": 0.1, "far": 50 },
            "Transform": { "translation": [0, 5, 0], "rotation": [-FRAC_1_SQRT_2, 0, 0, FRAC_1_SQRT_2] } } },
        "sun": { "components": {
            "DirectionalLight": { "illuminance_lux": 60000 },
            "Transform": { "rotation": [-0.683013, -0.183013, 0.183013, 0.683013] } } },
        "ground": { "components": {
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.3, 0.3, 0.3, 1], "roughness": 1 },
            "Transform": { "translation": [0, -1, 0], "scale": [12, 0.1, 12] } } },
        "tile": { "components": {
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.2, 0.5, 0.9, tile_alpha], "roughness": 1 },
            "Transform": { "scale": [3, 0.05, 3] } } },
        "block": { "components": {
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.6, 0.6, 0.6, 1], "roughness": 1 },
            "Transform": { "translation": [-0.8, 1.5, 0], "scale": [0.5, 0.2, 0.5] } } }
    });
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    draw(&renderer, &prepare(resources, &entities).0)
}

fn sum(pixel: [u8; 4]) -> i32 {
    pixel[..3].iter().map(|c| i32::from(*c)).sum()
}

#[test]
fn glass_receives_shadows_and_casts_none() {
    for (name, resources) in [
        ("single", json!({})),
        (
            "cascades",
            json!({ "RenderSettings": { "shadow_cascades": 3, "shadow_distance": 20 } }),
        ),
    ] {
        let frame = shaded(&resources, 0.6);
        keep(&format!("shadow_{name}"), &frame.image);
        assert_eq!(frame.stats.blended_surfaces, 1, "{name}");
        // The block's shadow falls on the tile about 0.9 m to its right
        // (x = 0.07 m, pixel 33); the tile further right (pixel 46) is lit.
        let in_shadow = frame.image.pixel(33, 32).unwrap();
        let open = frame.image.pixel(46, 32).unwrap();
        assert!(
            sum(in_shadow) + 30 < sum(open),
            "{name}: the block's shadow on the glass: {in_shadow:?} against {open:?}"
        );
        // The tile casts no shadow: the ground right of it, where the
        // tile's shadow would fall (pixel 60, row 32), is as bright as the
        // ground beyond the tile's shadow (row 4).
        let under = frame.image.pixel(60, 32).unwrap();
        let beyond = frame.image.pixel(60, 4).unwrap();
        for (a, b) in under.iter().zip(&beyond) {
            assert!(a.abs_diff(*b) <= 1, "{name}: {under:?} against {beyond:?}");
        }
        // A solid tile does cast it there, and is one more caster.
        let solid = shaded(&resources, 1.0);
        let under_solid = solid.image.pixel(60, 32).unwrap();
        assert!(
            sum(under_solid) + 30 < sum(beyond),
            "{name}: {under_solid:?} against {beyond:?}"
        );
        assert!(solid.stats.shadow_casters > frame.stats.shadow_casters);
        if name == "single" {
            assert_eq!(frame.stats.shadow_casters, 2, "the ground and the block");
            assert_eq!(frame.stats.shadow_cascades, 1);
        } else {
            assert_eq!(frame.stats.shadow_cascades, 3);
        }
    }
}

#[test]
fn glass_is_fogged_like_any_surface() {
    let backdrop = [0.1, 0.25, 0.45, 1.0];
    let glass = |alpha: f32| {
        with(
            studio(backdrop),
            "pane",
            pane(
                json!({ "base_color": [0.55, 0.12, 0.1, alpha], "roughness": 1 }),
                0.0,
            ),
        )
    };
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let fog = json!({ "Fog": { "density": 0.15, "tint": [1, 1, 1, 1] } });
    let picture = |resources: &Value, alpha: f32| {
        draw(&renderer, &prepare(resources, &glass(alpha)).0)
            .image
            .pixel(MIDDLE.0, MIDDLE.1)
            .unwrap()
    };
    let (clear, fogged) = (picture(&json!({}), 0.5), picture(&fog, 0.5));
    let (solid_clear, solid_fogged) = (picture(&json!({}), 1.0), picture(&fog, 1.0));
    assert_ne!(clear, fogged);
    // Fog moves the glass the way it moves a solid pane at the same place.
    let towards = |a: [u8; 4], b: [u8; 4]| sum(b) - sum(a);
    let (glass_change, solid_change) = (towards(clear, fogged), towards(solid_clear, solid_fogged));
    assert!(
        glass_change.signum() == solid_change.signum() && glass_change.abs() > 10,
        "glass {clear:?} -> {fogged:?}, solid {solid_clear:?} -> {solid_fogged:?}"
    );
}

#[test]
fn a_bright_light_behind_glass_blooms() {
    let entities = with(
        with(
            studio([0.05, 0.05, 0.05, 1.0]),
            "lamp",
            json!({ "components": {
                "Primitive": { "shape": "Cube" },
                "Material": { "base_color": [0, 0, 0, 1], "emissive": [30, 25, 20] },
                "Transform": { "translation": [0, 0, -0.5], "scale": [0.5, 0.5, 0.1] } } }),
        ),
        "glass",
        pane(
            json!({ "base_color": [0.3, 0.5, 0.8, 0.4], "roughness": 1 }),
            0.0,
        ),
    );
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let plain = draw(&renderer, &prepare(&json!({}), &entities).0);
    let bloomed = draw(
        &renderer,
        &prepare(&json!({ "Bloom": { "strength": 0.3 } }), &entities).0,
    );
    keep("bloom", &bloomed.image);
    // Beside the lamp (it covers pixels 28 to 35), behind the glass: the
    // glow lifts it.
    for at in [(24, 32), (39, 32), (32, 24)] {
        let (without, with) = (
            plain.image.pixel(at.0, at.1).unwrap(),
            bloomed.image.pixel(at.0, at.1).unwrap(),
        );
        assert!(
            sum(with) > sum(without) + 15,
            "{at:?}: {without:?} -> {with:?}"
        );
    }
    assert_eq!(bloomed.stats.blended_surfaces, 1);
}

#[test]
fn glass_is_picked_and_what_is_behind_it_is_still_visible() {
    let backdrop = [0.2, 0.2, 0.2, 1.0];
    let block = json!({ "components": {
        "Primitive": { "shape": "Cube" },
        "Material": { "base_color": [0.6, 0.1, 0.1, 1], "roughness": 1 },
        "Transform": { "translation": [0, 0, -0.5], "scale": [1, 1, 0.2] } } });
    let without = with(studio(backdrop), "block", block.clone());
    let glass = with(
        without.clone(),
        "glass",
        pane(
            json!({ "base_color": [0.1, 0.6, 0.1, 0.4], "roughness": 1 }),
            0.0,
        ),
    );
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, entities) = prepare(&json!({}), &glass);
    let (bare, bare_entities) = prepare(&json!({}), &without);

    // A click on the glass picks the glass, as in every editor.
    let ids = renderer.id_buffer(&gpu, &scene, None, SIZE, SIZE).unwrap();
    assert_eq!(ids.entity_at(32, 32), Some(entities["glass"]));
    assert_eq!(ids.entity_at(6, 6), Some(entities["backdrop"]));
    let view = renderer
        .render_view(&gpu, &scene, None, SIZE, SIZE, View::Ids)
        .unwrap();
    keep("ids", &view.image);
    assert_eq!(view.stats.blended_surfaces, 1);

    // What shows through the glass is visible: the block, as much of it as
    // without the glass; the glass too, and it is marked.
    let described = renderer.describe(&gpu, &scene, None, SIZE, SIZE).unwrap();
    let alone = renderer.describe(&gpu, &bare, None, SIZE, SIZE).unwrap();
    let find = |description: &wish_render::Description, entity| {
        description
            .visible
            .iter()
            .find(|visible| visible.entity == entity)
            .cloned()
            .unwrap_or_else(|| panic!("{entity} is not visible: {description:?}"))
    };
    let block_seen = find(&described, entities["block"]);
    let block_alone = find(&alone, bare_entities["block"]);
    assert_eq!(block_seen.pixels, block_alone.pixels);
    assert_eq!(block_seen.visible_fraction, block_alone.visible_fraction);
    assert_eq!(block_seen.bounding_box, block_alone.bounding_box);
    assert!(!block_seen.blended);
    let glass_seen = find(&described, entities["glass"]);
    assert!(glass_seen.blended);
    assert_eq!(glass_seen.visible_fraction, Some(1.0));
    // The backdrop behind the glass counts there too: as many pixels as
    // without the glass.
    assert_eq!(
        find(&described, entities["backdrop"]).pixels,
        find(&alone, bare_entities["backdrop"]).pixels
    );
    assert!(described.hidden.is_empty(), "{:?}", described.hidden);
    assert_eq!(described.stats.blended_surfaces, 1);
    assert_eq!(alone.stats.blended_surfaces, 0);
}

#[test]
fn a_surface_faded_to_nothing_is_not_there() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let backdrop = [0.2, 0.2, 0.2, 1.0];
    let faded = |alpha: f32| {
        prepare(
            &json!({}),
            &with(
                studio(backdrop),
                "ghost",
                pane(json!({ "base_color": [0.9, 0.9, 0.9, alpha] }), 0.0),
            ),
        )
    };
    let (scene, entities) = faded(0.0);
    let frame = draw(&renderer, &scene);
    assert_eq!(frame.stats.blended_surfaces, 1);
    let ids = renderer.id_buffer(&gpu, &scene, None, SIZE, SIZE).unwrap();
    assert_eq!(ids.entity_at(32, 32), Some(entities["backdrop"]));
    let described = renderer.describe(&gpu, &scene, None, SIZE, SIZE).unwrap();
    assert_eq!(described.hidden, [entities["ghost"]]);
    // A negative alpha is nothing as well: the same picture as 0.
    let negative = draw(&renderer, &faded(-3.0).0);
    assert_eq!(negative.image, frame.image);
    // The picture is the backdrop's where the ghost is (to a step of
    // rounding: the backdrop is not quite the same colour everywhere).
    let (middle, corner) = (
        frame.image.pixel(32, 32).unwrap(),
        frame.image.pixel(6, 6).unwrap(),
    );
    for (a, b) in middle.iter().zip(&corner) {
        assert!(a.abs_diff(*b) <= 1, "{middle:?} {corner:?}");
    }
}

#[test]
fn the_debug_views_show_glass_as_a_surface() {
    let backdrop = [0.2, 0.2, 0.2, 1.0];
    let glass = with(
        studio(backdrop),
        "glass",
        pane(json!({ "base_color": [0.1, 0.6, 0.1, 0.4] }), 0.0),
    );
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, _) = prepare(&json!({}), &glass);
    let (bare, _) = prepare(&json!({}), &studio(backdrop));
    let view = |scene: &RenderScene, view: View| {
        renderer
            .render_view(&gpu, scene, None, SIZE, SIZE, view)
            .unwrap()
    };
    // Depth: the glass is nearer than the backdrop behind it.
    let depth = view(&scene, View::Depth);
    let bare_depth = view(&bare, View::Depth);
    keep("depth", &depth.image);
    assert!(sum(depth.image.pixel(32, 32).unwrap()) > sum(bare_depth.image.pixel(32, 32).unwrap()));
    assert_eq!(depth.image.pixel(6, 6), bare_depth.image.pixel(6, 6));
    // The same count in every view; the wireframe draws the glass's edges
    // as any surface's.
    for kind in [
        View::Normals,
        View::Wireframe,
        View::Depth,
        View::Ids,
        View::Lit,
    ] {
        assert_eq!(view(&scene, kind).stats.blended_surfaces, 1, "{kind:?}");
    }
    let wire = view(&scene, View::Wireframe);
    let bare_wire = view(&bare, View::Wireframe);
    assert_eq!(wire.stats.draw_calls, bare_wire.stats.draw_calls + 2);
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

/// A 2 m square facing +Z with `material`, as a glTF model.
fn square_model(material: &Value) -> wish_import::Model {
    let floats =
        |values: &[f32]| -> Vec<u8> { values.iter().flat_map(|v| v.to_le_bytes()).collect() };
    let positions = floats(&[
        -1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 1.0, 0.0, -1.0, 1.0, 0.0,
    ]);
    let normals = floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
    let indices: Vec<u8> = [0u16, 1, 2, 0, 2, 3]
        .iter()
        .flat_map(|i| i.to_le_bytes())
        .collect();
    let mut binary = Vec::new();
    let mut views = Vec::new();
    for part in [&positions, &normals, &indices] {
        views.push(json!({ "buffer": 0, "byteOffset": binary.len(), "byteLength": part.len() }));
        binary.extend_from_slice(part);
    }
    let root = json!({
        "asset": { "version": "2.0" },
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [-1, -1, 0], "max": [1, 1, 0] },
            { "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3" },
            { "bufferView": 2, "componentType": 5123, "count": 6, "type": "SCALAR" }
        ],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0, "NORMAL": 1 }, "indices": 2, "material": 0 }] }],
        "materials": [material],
        "nodes": [{ "mesh": 0 }],
        "scenes": [{ "nodes": [0] }],
        "scene": 0,
        "bufferViews": views,
        "buffers": [{ "byteLength": binary.len().div_ceil(4) * 4 }]
    });
    wish_import::load_model_bytes(&glb(&root, &binary), "assets/square.glb").unwrap()
}

#[test]
fn a_gltf_blend_material_is_see_through() {
    let gpu = gpu();
    let entities = with(
        studio([0.1, 0.25, 0.45, 1.0]),
        "model",
        json!({ "components": { "Model": { "asset": "assets/square.glb" } } }),
    );
    let (scene, _) = prepare(&json!({}), &entities);
    let draw_model = |mode: &str| {
        let material = json!({ "alphaMode": mode, "pbrMetallicRoughness": {
            "baseColorFactor": [0.55, 0.12, 0.1, 0.5], "metallicFactor": 0.0, "roughnessFactor": 1.0 } });
        let mut renderer = Renderer::new(&gpu).unwrap();
        renderer
            .upload_model(&gpu, "assets/square.glb", &square_model(&material))
            .unwrap();
        draw(&renderer, &scene)
    };
    let blended = draw_model("BLEND");
    let opaque = draw_model("OPAQUE");
    keep("gltf", &blended.image);
    assert!(codes(&blended).is_empty(), "{:?}", blended.warnings);
    assert_eq!(blended.stats.blended_surfaces, 1);
    assert_eq!(opaque.stats.blended_surfaces, 0);
    // The blue backdrop shows through the red square.
    let (b, o) = (
        blended.image.pixel(32, 32).unwrap(),
        opaque.image.pixel(32, 32).unwrap(),
    );
    assert!(b[2] > o[2] + 20 && b[0] < o[0], "{b:?} against {o:?}");
}

#[test]
fn a_double_sided_see_through_box_shows_its_far_side() {
    let gpu = gpu();
    let backdrop = [0.1, 0.1, 0.1, 1.0];
    // A box seen from above one corner, so that its far inside walls are
    // in the picture.
    let entities = json!({
        "camera": { "components": {
            "Camera": { "fov_y_deg": 50 },
            "Transform": { "translation": [2.2, 2.2, 3.2], "rotation": [-0.2798, 0.3266, 0.1036, 0.8972] } } },
        "sun": { "components": {
            "DirectionalLight": { "illuminance_lux": 60000 },
            "Transform": { "rotation": [-0.3826834, 0, 0, 0.9238795] } } },
        "floor": { "components": {
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": backdrop, "roughness": 1 },
            "Transform": { "translation": [0, -1.1, 0], "scale": [10, 0.2, 10] } } },
        "box": { "components": {
            "Primitive": { "shape": "Cube" },
            "Material": { "file": FILE },
            "Transform": { "scale": [1.5, 1.5, 1.5] } } }
    });
    let (scene, _) = prepare(&json!({}), &entities);
    let draw_with = |double_sided: bool| {
        draw(
            &renderer_with(
                &gpu,
                &json!({ "base_color": [0.2, 0.7, 0.3, 0.35], "alpha_mode": "Blend",
                         "double_sided": double_sided, "roughness": 0.4 }),
            ),
            &scene,
        )
    };
    let (single, double) = (draw_with(false), draw_with(true));
    keep("double_sided", &double.image);
    assert_ne!(single.image, double.image, "the far walls are drawn");
    // Two draw calls instead of one: the back faces, then the front faces.
    assert_eq!(double.stats.draw_calls, single.stats.draw_calls + 1);
    assert_eq!(
        double.stats.triangles,
        single.stats.triangles + 12,
        "a cube's 12 triangles again"
    );
    assert_eq!(double.stats.blended_surfaces, 1);
    // In the box's middle the far walls add a second layer of green.
    let (s, d) = (
        single.image.pixel(SIZE / 2, SIZE / 2).unwrap(),
        double.image.pixel(SIZE / 2, SIZE / 2).unwrap(),
    );
    assert_ne!(s, d);
}

#[test]
fn hostile_alpha_in_a_material_file_is_cleaned() {
    let gpu = gpu();
    let entities = with(
        studio([0.1, 0.25, 0.45, 1.0]),
        "pane",
        pane(json!({ "file": FILE }), 0.0),
    );
    let (scene, _) = prepare(&json!({}), &entities);
    let blend = |alpha: f32| {
        let mut file = MaterialFile {
            base_color: wish_core::Color::rgba(0.55, 0.12, 0.1, alpha),
            alpha_mode: wish_render::AlphaMode::Blend,
            ..MaterialFile::default()
        };
        file.roughness = 1.0;
        Look::of(&file, &|_| None)
    };
    let picture = |look: Look| {
        let mut renderer = Renderer::new(&gpu).unwrap();
        renderer.set_material(FILE, look);
        draw(&renderer, &scene).image
    };
    let (full, none) = (picture(blend(1.0)), picture(blend(0.0)));
    assert_ne!(full, none);
    assert_eq!(picture(blend(f32::NAN)), full, "not a number is solid");
    assert_eq!(picture(blend(7.0)), full, "above 1 is 1");
    assert_eq!(picture(blend(-1.0)), none, "below 0 is 0");
    // A look built by code without cleaning reaches the GPU cleaned too.
    for (alpha, expected) in [(f32::NAN, &full), (f32::INFINITY, &full), (-2.0, &none)] {
        let mut look = blend(0.5);
        look.material.base_color[3] = alpha;
        assert_eq!(&picture(look), expected, "{alpha}");
    }
}

/// What see-through surfaces cost, by the clock: run by hand, in release
/// (`cargo test --release -p wish_render --test blended
/// what_blending_costs -- --ignored --nocapture`; `WISH_SOFTWARE=1` for the
/// software adapter). 200 panes, all overlapping in the middle of the
/// picture, solid and then half see-through. Nothing is asserted: the
/// numbers depend on the machine.
#[test]
#[ignore = "a measurement, run by hand in release"]
fn what_blending_costs() {
    let software = std::env::var_os("WISH_SOFTWARE").is_some();
    let gpu = Gpu::new(GpuOptions { software }).expect("an adapter");
    eprintln!("adapter: {:?}", gpu.adapter());
    let renderer = Renderer::new(&gpu).unwrap();
    let panes = |alpha: f32| {
        let mut entities = json!({
            "camera": { "components": {
                "Camera": { "fov_y_deg": 50 },
                "Transform": { "translation": [0, 0, 8] } } },
            "sun": { "components": {
                "DirectionalLight": { "illuminance_lux": 60000 },
                "Transform": { "rotation": [-0.3826834, 0, 0, 0.9238795] } } },
            "ground": { "components": {
                "Primitive": { "shape": "Cube" },
                "Material": { "base_color": [0.4, 0.4, 0.4, 1] },
                "Transform": { "translation": [0, -3, 0], "scale": [40, 0.2, 40] } } }
        });
        for index in 0..200 {
            let t = index as f32 / 200.0;
            entities[format!("pane{index:03}")] = json!({ "components": {
                "Primitive": { "shape": "Cube" },
                "Material": { "base_color": [0.3 + 0.5 * t, 0.6 - 0.4 * t, 0.5, alpha], "roughness": 0.3 },
                "Transform": { "translation": [(t * 37.0).sin() * 1.5, (t * 23.0).cos() * 1.0, -4.0 + 8.0 * t],
                               "scale": [3, 2, 0.02] } } });
        }
        prepare(&json!({}), &entities).0
    };
    let (solid, glass) = (panes(1.0), panes(0.5));
    for (width, height) in [(1280u32, 720u32), (1920, 1080)] {
        let time = |scene: &RenderScene| {
            for _ in 0..3 {
                renderer.render(&gpu, scene, None, width, height).unwrap();
            }
            let frames = 20;
            let start = std::time::Instant::now();
            for _ in 0..frames {
                renderer.render(&gpu, scene, None, width, height).unwrap();
            }
            start.elapsed().as_secs_f64() * 1000.0 / f64::from(frames)
        };
        let (without, with) = (time(&solid), time(&glass));
        eprintln!(
            "{width}x{height}, 200 panes: {without:.2} ms a frame solid, {with:.2} ms see-through ({:+.2} ms)",
            with - without
        );
    }
}

#[test]
fn a_scene_of_glass_alone_draws_no_shadow_map() {
    // A see-through pane over a see-through ground, under a sun: nothing
    // casts, so no map is planned and no empty shadow pass is drawn, with
    // one map or with cascades, and no warning says shadows are out of
    // range.
    let entities = json!({
        "camera": { "components": {
            "Camera": { "fov_y_deg": 50 },
            "Transform": { "translation": [0, 1.5, 5] } } },
        "sun": { "components": {
            "DirectionalLight": { "illuminance_lux": 60000 },
            "Transform": { "rotation": [-0.3826834, 0, 0, 0.9238795] } } },
        "ground": { "components": {
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.4, 0.5, 0.4, 0.5] },
            "Transform": { "translation": [0, -0.1, 0], "scale": [10, 0.2, 10] } } },
        "pane": { "components": {
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.6, 0.8, 0.9, 0.3] },
            "Transform": { "translation": [0, 1, 0], "scale": [2, 2, 0.05] } } }
    });
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    for resources in [
        json!({}),
        json!({ "RenderSettings": { "shadow_cascades": 3, "shadow_distance": 30 } }),
    ] {
        let frame = draw(&renderer, &prepare(&resources, &entities).0);
        assert_eq!(frame.stats.blended_surfaces, 2, "{resources}");
        assert_eq!(
            (frame.stats.shadow_cascades, frame.stats.shadow_casters),
            (0, 0),
            "{resources}"
        );
        assert!(
            codes(&frame).is_empty(),
            "{resources}: {:?}",
            frame.warnings
        );
        // The same scene with a solid ground has a map again, and only the
        // ground is drawn into it.
        let mut solid = entities.clone();
        solid["ground"]["components"]["Material"]["base_color"][3] = json!(1.0);
        let frame = draw(&renderer, &prepare(&resources, &solid).0);
        assert!(frame.stats.shadow_cascades >= 1, "{resources}");
        assert!(frame.stats.shadow_casters >= 1, "{resources}");
        assert_eq!(frame.stats.blended_surfaces, 1);
    }
}
