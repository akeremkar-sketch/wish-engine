//! Materials outside models: material files and their textures on the
//! built-in shapes, on the software adapter.
//!
//! The tests are structural: they say which colour must be where, which of
//! two pictures must be the brighter at a point, and by a wide margin, so
//! that they hold on every adapter (design note M12-materials.md, section 6).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_core::math::{Quat, Vec3};
use wish_ecs::World;
use wish_import::Texture;
use wish_render::{
    Frame, Gpu, GpuOptions, Look, MaterialData, MaterialFile, RenderScene, Renderer, RgbaImage,
    SurfaceData, Tolerance, extract,
};
use wish_scene::load;
use wish_schema::Plain;
use wish_transform::update_global_transforms;

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const YELLOW: [u8; 4] = [255, 255, 0, 255];
const GREY: [u8; 4] = [128, 128, 128, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

/// A picture of `columns` × `rows` cells of `cell` pixels each, coloured by
/// `colour(column, row)`. Cells are many pixels wide so that a sample in the
/// middle of one is that cell's colour whatever the filter does at edges.
fn cells(columns: u32, rows: u32, colour: impl Fn(u32, u32) -> [u8; 4]) -> Texture {
    let cell = 16;
    let (width, height) = (columns * cell, rows * cell);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            rgba.extend_from_slice(&colour(x / cell, y / cell));
        }
    }
    Texture {
        width,
        height,
        rgba,
    }
}

/// Red and green on top, blue and yellow below.
fn quadrants() -> Texture {
    cells(2, 2, |x, y| match (x, y) {
        (0, 0) => RED,
        (1, 0) => GREEN,
        (0, 1) => BLUE,
        _ => YELLOW,
    })
}

fn flat(colour: [u8; 4]) -> Texture {
    cells(1, 1, |_, _| colour)
}

fn scene(source: &str) -> RenderScene {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    let result = load(source, "s.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap().instantiate(&mut world).unwrap();
    update_global_transforms(&mut world).unwrap();
    extract(&world).unwrap()
}

fn gpu() -> std::sync::Arc<Gpu> {
    Gpu::shared(GpuOptions { software: true }).unwrap()
}

/// A renderer that holds `textures` (path, read as colours, pixels).
fn renderer(gpu: &Gpu, textures: &[(&str, bool, Texture)]) -> Renderer {
    let mut renderer = Renderer::new(gpu).unwrap();
    for (path, colour, texture) in textures {
        renderer
            .upload_texture(gpu, path, *colour, texture)
            .unwrap();
    }
    renderer
}

/// The material file every test scene names.
const FILE: &str = "assets/m.mat.json";

/// The look a material file with these fields holds (the JSON of its
/// `material`), read through the schema as the file is.
fn look(material: &str) -> Look {
    let schema = MaterialFile::schema();
    let mut texts = wish_ecs::TextTable::default();
    let bytes = {
        let mut context = wish_schema::JsonContext::new(&mut texts);
        let value = serde_json::from_str(material).unwrap();
        wish_schema::from_json(&schema, &value, &mut context).unwrap()
    };
    let file = MaterialFile::decode(&bytes).unwrap();
    Look::of(&file, &|reference| {
        texts.get(reference.text()).map(str::to_owned)
    })
}

fn codes(frame: &Frame) -> Vec<&str> {
    frame.warnings.iter().map(|w| w.code.as_ref()).collect()
}

/// Which of the test colours a pixel is nearest to.
fn name(pixel: [u8; 4]) -> &'static str {
    let [r, g, b, _] = pixel.map(i32::from);
    let high = r.max(g).max(b);
    let is = |channel: i32| channel * 2 > high;
    match (is(r), is(g), is(b)) {
        _ if high < 24 => "black",
        (true, false, false) => "red",
        (false, true, false) => "green",
        (false, false, true) => "blue",
        (true, true, false) => "yellow",
        (true, true, true) => "white",
        _ => "other",
    }
}

fn brightness(image: &RgbaImage, x: u32, y: u32) -> i32 {
    let [r, g, b, _] = image.pixel(x, y).unwrap().map(i32::from);
    r + g + b
}

const LOOK_DOWN: &str = "[-0.70710678, 0, 0, 0.70710678]";
const LOOK_ALONG_MINUS_X: &str = "[0, 0.70710678, 0, 0.70710678]";

/// One `shape` at the origin whose `Material` names [`FILE`], lit by white
/// ambient light only, seen through an orthographic camera at `eye` turned
/// by `rotation`: the picture shows 2 m, so a 1 m shape fills its middle
/// half. In a 128 x 128 picture the shape's quadrants are at (48, 48),
/// (80, 48), (48, 80), (80, 80).
fn shape_scene(shape: &str, eye: &str, rotation: &str) -> RenderScene {
    scene(&format!(
        r#"{{ "format": 1, "entities": {{
            "camera": {{ "components": {{
                "Camera": {{ "projection": "Orthographic", "ortho_height": 2 }},
                "Transform": {{ "translation": {eye}, "rotation": {rotation} }} }} }},
            "thing": {{ "components": {{ "Primitive": {{ "shape": "{shape}" }},
                "Material": {{ "file": "{FILE}", "base_color": [0, 0, 1, 1] }} }} }}
        }}, "resources": {{
            "Ambient": {{ "color": [1, 1, 1, 1], "intensity": 60000 }},
            "Sky": {{ "zenith": [0, 0, 0, 1], "horizon": [0, 0, 0, 1], "ground": [0, 0, 0, 1] }}
        }} }}"#
    ))
}

/// Draws [`shape_scene`] with `material` as the content of [`FILE`], at
/// 128 x 128.
fn draw(
    gpu: &Gpu,
    renderer: &mut Renderer,
    (shape, material): (&str, &str),
    (eye, rotation): (&str, &str),
) -> Frame {
    renderer.set_material(FILE, look(material));
    renderer
        .render(gpu, &shape_scene(shape, eye, rotation), None, 128, 128)
        .unwrap()
}

/// The colours at the four quadrants of the shape in a 128 x 128 picture:
/// top left, top right, bottom left, bottom right.
fn quadrant_names(image: &RgbaImage) -> [&'static str; 4] {
    [(48, 48), (80, 48), (48, 80), (80, 80)].map(|(x, y)| name(image.pixel(x, y).unwrap()))
}

const QUADRANTS: &str = r#"{ "base_color_texture": "assets/q.png", "roughness": 1 }"#;

#[test]
fn a_picture_reads_the_right_way_round_on_every_shape() {
    let gpu = gpu();
    let mut renderer = renderer(&gpu, &[("assets/q.png", true, quadrants())]);
    let upright = ["red", "green", "blue", "yellow"];
    // The plane from above, by a camera whose "up" is -Z: as in an image
    // viewer.
    let views = [
        ("Plane", "[0, 5, 0]", LOOK_DOWN, "the plane from above"),
        ("Cube", "[0, 0, 5]", "[0, 0, 0, 1]", "the cube's +Z face"),
        (
            "Cube",
            "[5, 0, 0]",
            LOOK_ALONG_MINUS_X,
            "the cube's +X face",
        ),
        ("Cube", "[0, 5, 0]", LOOK_DOWN, "the cube's top"),
    ];
    for (shape, eye, rotation, what) in views {
        let frame = draw(&gpu, &mut renderer, (shape, QUADRANTS), (eye, rotation));
        assert_eq!(codes(&frame), Vec::<&str>::new(), "{what}");
        assert_eq!(quadrant_names(&frame.image), upright, "{what}");
    }
}

#[test]
fn a_picture_wraps_a_sphere_left_to_right_with_its_top_at_the_pole() {
    let gpu = gpu();
    // Four columns, two rows. Seen from +Z the sphere's middle is where the
    // third column ends, so the third column is left of it and the fourth
    // right of it.
    let wrap = cells(4, 2, |x, y| match (x, y) {
        (2, 0) => RED,
        (3, 0) => GREEN,
        (2, 1) => BLUE,
        (3, 1) => YELLOW,
        _ => GREY,
    });
    let mut renderer = renderer(&gpu, &[("assets/q.png", true, wrap)]);
    let frame = draw(
        &gpu,
        &mut renderer,
        ("Sphere", QUADRANTS),
        ("[0, 0, 5]", "[0, 0, 0, 1]"),
    );
    let seen = [(54, 54), (74, 54), (54, 74), (74, 74)]
        .map(|(x, y)| name(frame.image.pixel(x, y).unwrap()));
    assert_eq!(seen, ["red", "green", "blue", "yellow"]);
}

#[test]
fn scale_repeats_the_picture_and_offset_slides_it() {
    let gpu = gpu();
    let halves = cells(2, 1, |x, _| if x == 0 { RED } else { GREEN });
    let mut renderer = renderer(&gpu, &[("assets/h.png", true, halves)]);
    // Across the plane's width (pixels 32 to 96 of 128), at eighths.
    let mut across = |material: &str| {
        let frame = draw(
            &gpu,
            &mut renderer,
            ("Plane", material),
            ("[0, 5, 0]", LOOK_DOWN),
        );
        [40, 56, 72, 88].map(|x| name(frame.image.pixel(x, 64).unwrap()))
    };
    assert_eq!(
        across(r#"{ "base_color_texture": "assets/h.png" }"#),
        ["red", "red", "green", "green"]
    );
    assert_eq!(
        across(r#"{ "base_color_texture": "assets/h.png", "uv_scale": [2, 1] }"#),
        ["red", "green", "red", "green"],
        "twice across"
    );
    assert_eq!(
        across(r#"{ "base_color_texture": "assets/h.png", "uv_offset": [0.5, 0] }"#),
        ["green", "green", "red", "red"],
        "slid by half a picture"
    );
}

fn quaternion(direction: Vec3) -> String {
    let [x, y, z, w] = Quat::from_rotation_arc(Vec3::NEG_Z, direction.normalize()).to_array();
    format!("[{x}, {y}, {z}, {w}]")
}

/// A plane seen from above, lit by a sun that shines along `sun`, whose
/// material file is `material` and names a normal map of one colour.
/// Returns the brightness at its middle.
fn lit_with_normal(gpu: &Gpu, normal: [u8; 4], sun: Vec3, material: &str) -> i32 {
    let mut renderer = renderer(gpu, &[("assets/n.png", false, flat(normal))]);
    renderer.set_material(FILE, look(material));
    let source = format!(
        r#"{{ "format": 1, "entities": {{
            "camera": {{ "components": {{
                "Camera": {{ "projection": "Orthographic", "ortho_height": 2 }},
                "Transform": {{ "translation": [0, 5, 0], "rotation": {LOOK_DOWN} }} }} }},
            "sun": {{ "components": {{ "DirectionalLight": {{ "shadows": false }},
                "Transform": {{ "rotation": {} }} }} }},
            "thing": {{ "components": {{ "Primitive": {{ "shape": "Plane" }},
                "Material": {{ "file": "{FILE}" }} }} }}
        }}, "resources": {{ "Ambient": {{ "intensity": 0 }},
            "Sky": {{ "zenith": [0, 0, 0, 1], "horizon": [0, 0, 0, 1], "ground": [0, 0, 0, 1] }} }} }}"#,
        quaternion(sun)
    );
    let frame = renderer
        .render(gpu, &scene(&source), None, 128, 128)
        .unwrap();
    assert_eq!(codes(&frame), Vec::<&str>::new());
    brightness(&frame.image, 64, 64)
}

const BUMPY: &str =
    r#"{ "normal_texture": "assets/n.png", "roughness": 1, "base_color": [0.18, 0.18, 0.18, 1] }"#;
const BUMPS_OFF: &str = r#"{ "normal_texture": "assets/n.png", "normal_scale": 0, "roughness": 1,
    "base_color": [0.18, 0.18, 0.18, 1] }"#;

#[test]
fn a_normal_map_bends_the_light_the_right_way_round() {
    let gpu = gpu();
    let straight = [128, 128, 255, 255];
    // Red is along the picture's width (+X on the plane), green towards the
    // picture's top (-Z on the plane).
    let (to_right, to_left) = ([228, 128, 207, 255], [28, 128, 207, 255]);
    let (to_top, to_bottom) = ([128, 228, 207, 255], [128, 28, 207, 255]);
    // A sun low in the +X sky shines towards -X.
    let from_right = Vec3::new(-1.0, -1.0, 0.0);
    let flat_lit = lit_with_normal(&gpu, straight, from_right, BUMPY);
    let towards = lit_with_normal(&gpu, to_right, from_right, BUMPY);
    let away = lit_with_normal(&gpu, to_left, from_right, BUMPY);
    assert!(
        towards > flat_lit + 30 && flat_lit > away + 30,
        "bumps facing the light are brighter: {towards} > {flat_lit} > {away}"
    );
    // A sun low in the -Z sky (the picture's top) shines towards +Z.
    let from_top = Vec3::new(0.0, -1.0, 1.0);
    let towards = lit_with_normal(&gpu, to_top, from_top, BUMPY);
    let away = lit_with_normal(&gpu, to_bottom, from_top, BUMPY);
    assert!(
        towards > flat_lit + 30 && flat_lit > away + 30,
        "green is up: {towards} > {flat_lit} > {away}"
    );
    // normal_scale 0 switches the bumps off: the same light as a flat map.
    let off = lit_with_normal(&gpu, to_left, from_right, BUMPS_OFF);
    assert!(
        (off - flat_lit).abs() <= 6,
        "no bumps: {off} against {flat_lit}"
    );
}

/// A 2 m plane 1 m above a large ground, under a sun that shines down at 45
/// degrees towards -X, seen from above (20 pixels a meter, the origin at
/// pixel (80, 60)). The plane's material is [`FILE`].
fn canopy_scene() -> RenderScene {
    let source = format!(
        r#"{{ "format": 1, "entities": {{
            "camera": {{ "components": {{
                "Camera": {{ "projection": "Orthographic", "ortho_height": 6 }},
                "Transform": {{ "translation": [0, 5, 0], "rotation": {LOOK_DOWN} }} }} }},
            "sun": {{ "components": {{ "DirectionalLight": {{}}, "Transform": {{ "rotation": {} }} }} }},
            "ground": {{ "components": {{ "Primitive": {{ "shape": "Plane" }},
                "Transform": {{ "scale": [8, 1, 8] }} }} }},
            "canopy": {{ "components": {{ "Primitive": {{ "shape": "Plane" }},
                "Transform": {{ "translation": [0, 1, 0], "scale": [2, 1, 2] }},
                "Material": {{ "file": "{FILE}" }} }} }}
        }} }}"#,
        quaternion(Vec3::new(-1.0, -1.0, 0.0))
    );
    scene(&source)
}

/// A picture that is clear on its left half (the plane's -X half).
fn half_clear() -> Texture {
    cells(2, 1, |x, _| if x == 0 { [255, 255, 255, 0] } else { WHITE })
}

fn canopy_look(alpha_mode: &str) -> Look {
    look(&format!(
        r#"{{ "base_color_texture": "assets/half.png", "alpha_mode": "{alpha_mode}" }}"#
    ))
}

#[test]
fn a_mask_cuts_the_shape_and_its_shadow() {
    let gpu = gpu();
    let mut renderer = renderer(&gpu, &[("assets/half.png", true, half_clear())]);
    let mut draw = |mode: &str| {
        renderer.set_material(FILE, canopy_look(mode));
        let frame = renderer
            .render(&gpu, &canopy_scene(), None, 160, 120)
            .unwrap();
        assert_eq!(codes(&frame), Vec::<&str>::new(), "{mode}");
        frame.image
    };
    let (solid, masked) = (draw("Opaque"), draw("Mask"));
    // The solid half (x 0 to 1) throws its shadow on the ground at x -1 to
    // 0: under the clear half, so it shows through the hole. The clear half
    // would throw its own at x -2 to -1.
    let (beside, under) = ((50, 60), (70, 60));
    let lit_ground = brightness(&masked, 20, 20);
    assert!(
        (brightness(&masked, beside.0, beside.1) - lit_ground).abs() < 12,
        "the clear half casts no shadow"
    );
    assert!(
        brightness(&solid, beside.0, beside.1) < lit_ground - 60,
        "a solid plane shades the ground beside it"
    );
    assert!(
        brightness(&masked, under.0, under.1) < lit_ground - 60,
        "the ground under the hole is seen, in the solid half's shadow"
    );
    assert!(
        brightness(&solid, under.0, under.1) > brightness(&masked, under.0, under.1) + 60,
        "a solid plane is its own lit surface there"
    );
    // The solid half is there in both.
    assert!((brightness(&solid, 90, 60) - brightness(&masked, 90, 60)).abs() < 12);
}

#[test]
fn a_material_file_gives_the_look_and_a_missing_one_falls_back() {
    let gpu = gpu();
    let mut renderer = renderer(&gpu, &[("assets/q.png", true, quadrants())]);
    // The entity's own Material is blue and names the file.
    let with_file = shape_scene("Plane", "[0, 5, 0]", LOOK_DOWN);
    // Not loaded: the entity's own numbers, and a warning that names the
    // file.
    let frame = renderer.render(&gpu, &with_file, None, 128, 128).unwrap();
    assert_eq!(codes(&frame), ["material_not_loaded"]);
    assert_eq!(frame.warnings[0].location.file.as_deref(), Some(FILE));
    assert!(frame.warnings[0].message.contains("the numbers of its own"));
    assert_eq!(name(frame.image.pixel(64, 64).unwrap()), "blue");
    // Loaded: the file's look, textures included; the entity's own colour is
    // not used.
    let mut surface = SurfaceData::default();
    surface.textures[0] = Some("assets/q.png".to_owned());
    renderer.set_material(
        FILE,
        Look {
            material: MaterialData::default(),
            surface,
        },
    );
    let frame = renderer.render(&gpu, &with_file, None, 128, 128).unwrap();
    assert_eq!(codes(&frame), Vec::<&str>::new());
    assert_eq!(
        quadrant_names(&frame.image),
        ["red", "green", "blue", "yellow"]
    );
    // A changed file (set again) is the new look at the next frame.
    renderer.set_material(FILE, look(r#"{ "base_color": [1, 0, 0, 1] }"#));
    let frame = renderer.render(&gpu, &with_file, None, 128, 128).unwrap();
    assert_eq!(quadrant_names(&frame.image), ["red"; 4]);
    renderer.remove_material(FILE);
    let frame = renderer.render(&gpu, &with_file, None, 128, 128).unwrap();
    assert_eq!(codes(&frame), ["material_not_loaded"]);
    // An entity without a file is drawn as it always was, whatever files
    // the renderer holds: the same scene with the file's name taken out.
    renderer.set_material(FILE, look(r#"{ "base_color": [1, 0, 0, 1] }"#));
    let mut own = with_file.clone();
    for draw in &mut own.draws {
        draw.material_file = None;
    }
    let frame = renderer.render(&gpu, &own, None, 128, 128).unwrap();
    assert_eq!(codes(&frame), Vec::<&str>::new());
    assert_eq!(quadrant_names(&frame.image), ["blue"; 4]);
}

#[test]
fn a_missing_texture_is_a_warning_and_the_default_of_its_slot() {
    let gpu = gpu();
    let mut renderer = renderer(&gpu, &[]);
    let green = r#"{ "base_color": [0, 1, 0, 1], "base_color_texture": "assets/gone.png",
        "normal_texture": "assets/gone.png", "roughness": 1 }"#;
    let view = ("[0, 0, 5]", "[0, 0, 0, 1]");
    let frame = draw(&gpu, &mut renderer, ("Cube", green), view);
    // One warning for each slot, each naming the file and the slot.
    assert_eq!(codes(&frame), ["texture_not_loaded", "texture_not_loaded"]);
    assert!(frame.warnings[0].message.contains("base_color_texture"));
    assert!(frame.warnings[1].message.contains("normal_texture"));
    assert!(
        frame
            .warnings
            .iter()
            .all(|w| w.location.file.as_deref() == Some("assets/gone.png"))
    );
    // The picture is the one of the same material without the textures.
    let plain = r#"{ "base_color": [0, 1, 0, 1], "roughness": 1 }"#;
    let expected = draw(&gpu, &mut renderer, ("Cube", plain), view);
    assert!(
        frame.image.matches(
            &expected.image,
            Tolerance {
                channel: 1,
                fraction: 0.0
            }
        ),
        "{:?}",
        frame.image.diff(&expected.image, 1)
    );
    // When the picture arrives it is drawn, with no renderer to make anew.
    renderer
        .upload_texture(&gpu, "assets/gone.png", true, &flat(RED))
        .unwrap();
    let frame = draw(&gpu, &mut renderer, ("Cube", green), view);
    assert_eq!(codes(&frame), ["texture_not_loaded"], "the normal map");
    // (Red picture times green colour: nothing is left.)
    assert_eq!(name(frame.image.pixel(64, 64).unwrap()), "black");
    renderer.remove_texture("assets/gone.png");
    let frame = draw(&gpu, &mut renderer, ("Cube", green), view);
    assert_eq!(codes(&frame).len(), 2);
    assert_eq!(name(frame.image.pixel(64, 64).unwrap()), "green");
}

#[test]
fn a_changed_texture_is_drawn_at_the_next_frame() {
    let gpu = gpu();
    let mut renderer = renderer(&gpu, &[("assets/q.png", true, flat(RED))]);
    let view = ("[0, 0, 5]", "[0, 0, 0, 1]");
    let frame = draw(&gpu, &mut renderer, ("Cube", QUADRANTS), view);
    assert_eq!(name(frame.image.pixel(64, 64).unwrap()), "red");
    renderer
        .upload_texture(&gpu, "assets/q.png", true, &flat(GREEN))
        .unwrap();
    let frame = draw(&gpu, &mut renderer, ("Cube", QUADRANTS), view);
    assert_eq!(name(frame.image.pixel(64, 64).unwrap()), "green");
}

#[test]
fn a_plane_is_seen_from_below_only_when_it_is_double_sided() {
    let gpu = gpu();
    let mut renderer = renderer(&gpu, &[]);
    let look_up = "[0.70710678, 0, 0, 0.70710678]";
    let mut below = |material: &str| {
        let frame = draw(
            &gpu,
            &mut renderer,
            ("Plane", material),
            ("[0, -5, 0]", look_up),
        );
        name(frame.image.pixel(64, 64).unwrap())
    };
    assert_eq!(below(r#"{ "base_color": [1, 0, 0, 1] }"#), "black");
    assert_eq!(
        below(r#"{ "base_color": [1, 0, 0, 1], "double_sided": true }"#),
        "red"
    );
}

#[test]
fn every_view_cuts_what_the_mask_cuts() {
    let gpu = gpu();
    let mut renderer = renderer(&gpu, &[("assets/half.png", true, half_clear())]);
    renderer.set_material(FILE, canopy_look("Mask"));
    let scene = canopy_scene();
    let ids = renderer.id_buffer(&gpu, &scene, None, 160, 120).unwrap();
    let canopy = scene
        .draws
        .iter()
        .find(|draw| draw.material_file.is_some())
        .unwrap()
        .entity;
    // Through the hole the ground is what is there; on the solid half the
    // canopy.
    assert_ne!(ids.entity_at(70, 60), Some(canopy));
    assert_eq!(ids.entity_at(90, 60), Some(canopy));
}

#[test]
fn broken_texture_data_is_refused_with_its_file() {
    let gpu = gpu();
    let mut renderer = renderer(&gpu, &[]);
    for broken in [
        Texture {
            width: 0,
            height: 4,
            rgba: Vec::new(),
        },
        Texture {
            width: 4,
            height: 4,
            rgba: vec![0; 7],
        },
    ] {
        let problem = renderer
            .upload_texture(&gpu, "assets/bad.png", true, &broken)
            .unwrap_err();
        assert_eq!(problem.code, "image_invalid");
        assert_eq!(problem.location.file.as_deref(), Some("assets/bad.png"));
        assert!(!problem.hint.is_empty());
    }
    assert!(!renderer.has_texture("assets/bad.png", true));
}

#[test]
fn a_picture_lists_at_most_32_material_warnings_and_counts_the_rest() {
    let gpu = gpu();
    let renderer = renderer(&gpu, &[]);
    // Forty cubes, each naming a material file that is not loaded.
    let entities: Vec<String> = (0..40)
        .map(|i| {
            format!(
                r#""c{i}": {{ "components": {{ "Primitive": {{}}, "Transform": {{ "translation": [{i}, 0, -5] }},
                "Material": {{ "file": "assets/m{i}.mat.json" }} }} }}"#
            )
        })
        .collect();
    let scene = scene(&format!(
        r#"{{ "format": 1, "entities": {{ "camera": {{ "components": {{ "Camera": {{}}, "Transform": {{}} }} }}, {} }} }}"#,
        entities.join(", ")
    ));
    let frame = renderer.render(&gpu, &scene, None, 32, 32).unwrap();
    let missing = codes(&frame)
        .iter()
        .filter(|c| **c == "material_not_loaded")
        .count();
    assert_eq!(missing, 32);
    let summary = frame
        .warnings
        .iter()
        .find(|w| w.code == "warnings_cut_short")
        .expect("a summary of the rest");
    assert!(summary.message.contains("8 more"), "{}", summary.message);
    // A file a hundred entities share is still one warning, and a few
    // problems give no summary.
    let few = scene_with_shared_file();
    let frame = renderer.render(&gpu, &few, None, 32, 32).unwrap();
    assert_eq!(codes(&frame), ["material_not_loaded"]);
}

fn scene_with_shared_file() -> RenderScene {
    let entities: Vec<String> = (0..100)
        .map(|i| {
            format!(
                r#""c{i}": {{ "components": {{ "Primitive": {{}}, "Material": {{ "file": "{FILE}" }} }} }}"#
            )
        })
        .collect();
    scene(&format!(
        r#"{{ "format": 1, "entities": {{ "camera": {{ "components": {{ "Camera": {{}}, "Transform": {{ "translation": [0, 0, 5] }} }} }}, {} }} }}"#,
        entities.join(", ")
    ))
}
