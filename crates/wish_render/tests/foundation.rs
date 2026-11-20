//! The render foundation (design note M12-render-foundation): statistics,
//! targets kept across frames, `RenderSettings`, masked shadow casters and
//! the warning about extra directional lights, on the software adapter.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use wish_core::Diagnostic;
use wish_core::math::Vec3;
use wish_ecs::World;
use wish_render::{
    Frame, Gpu, GpuOptions, RenderScene, RenderStats, Renderer, View, extract, project_to_pixel,
};
use wish_scene::load;
use wish_transform::update_global_transforms;

/// Triangles of the built-in shapes: a cube is 6 faces of 2, a plane is 2,
/// and the sphere (32 segments, 16 rings) has a triangle fan at each pole
/// and two triangles per cell in between.
const CUBE: u64 = 12;
const PLANE: u64 = 2;
const SPHERE: u64 = 32 * (2 * 16 - 2);

/// The scene of the lit golden image: a cube, a ground, two spheres, a sun.
const SCENE: &str = r#"{
  "format": 1,
  "entities": {
    "camera": { "components": {
      "Camera": { "fov_y_deg": 45 },
      "Transform": { "translation": [0, 1.6, 6], "rotation": [-0.0871557, 0, 0, 0.9961947] } } },
    "cube": { "components": {
      "Material": { "base_color": [0.8, 0.1, 0.08, 1], "roughness": 0.6 },
      "Primitive": { "shape": "Cube" },
      "Transform": { "translation": [-1.1, 0.5, 0], "rotation": [0, 0.3826834, 0, 0.9238795] } } },
    "ground": { "components": {
      "Material": { "base_color": [0.45, 0.45, 0.42, 1], "roughness": 0.9 },
      "Primitive": { "shape": "Plane" },
      "Transform": { "scale": [30, 1, 30] } } },
    "lamp": { "components": {
      "Material": { "base_color": [0.1, 0.1, 0.1, 1], "emissive": [0.2, 0.9, 0.3] },
      "Primitive": { "shape": "Sphere" },
      "Transform": { "translation": [0, 0.25, 1.5], "scale": [0.5, 0.5, 0.5] } } },
    "orb": { "components": {
      "Material": { "base_color": [0.95, 0.75, 0.35, 1], "metallic": 1, "roughness": 0.25 },
      "Primitive": { "shape": "Sphere" },
      "Transform": { "translation": [1.1, 0.6, 0], "scale": [1.2, 1.2, 1.2] } } },
    "sun": { "components": {
      "DirectionalLight": { "illuminance_lux": 100000 },
      "Transform": { "rotation": [-0.4545195, 0.3304543, 0.1786597, 0.8067046] } } }
  }
}
"#;

/// A 2 m block on a 12 m ground, lit by a sun that shines along +X and down
/// at 45 degrees: the shadow of the block's top is the square from x = -1 to
/// x = 1 on the ground, in open view of a camera that looks down on it.
fn block_scene(resources: &str) -> String {
    format!(
        r#"{{
  "format": 1,
  "resources": {{ {resources} }},
  "entities": {{
    "camera": {{ "components": {{
      "Camera": {{ "fov_y_deg": 45 }},
      "Transform": {{ "translation": [0, 7, 7], "rotation": [-0.3826834, 0, 0, 0.9238795] }} }} }},
    "block": {{ "components": {{
      "Primitive": {{ "shape": "Cube" }},
      "Transform": {{ "translation": [-2, 1, 0], "scale": [2, 2, 2] }} }} }},
    "ground": {{ "components": {{
      "Material": {{ "base_color": [0.6, 0.6, 0.6, 1], "roughness": 0.9 }},
      "Primitive": {{ "shape": "Plane" }},
      "Transform": {{ "scale": [12, 1, 12] }} }} }},
    "sun": {{ "components": {{
      "DirectionalLight": {{ "illuminance_lux": 100000 }},
      "Transform": {{ "rotation": [-0.2705981, -0.6532815, -0.2705981, 0.6532815] }} }} }}
  }}
}}"#
    )
}

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

fn pixel_at(frame: &Frame, scene: &RenderScene, point: Vec3) -> [u8; 4] {
    let camera = &scene.cameras[0];
    let (width, height) = (frame.image.width, frame.image.height);
    let view_proj = camera.projection(width as f32 / height as f32) * camera.view;
    let (x, y) = project_to_pixel(&view_proj, point, width, height).expect("in front");
    frame.image.pixel(x as u32, y as u32).expect("on screen")
}

fn luminance(pixel: [u8; 4]) -> i32 {
    i32::from(pixel[0]) + i32::from(pixel[1]) + i32::from(pixel[2])
}

#[test]
fn the_statistics_of_a_known_scene_are_its_counts() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, _) = prepare(SCENE);
    let surfaces = CUBE + PLANE + 2 * SPHERE;

    // Four surfaces, drawn once into the shadow map and once into the
    // picture, plus the sky and tone mapping (a triangle each).
    let frame = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
    assert_eq!(
        frame.stats,
        RenderStats {
            width: 320,
            height: 240,
            draw_calls: 4 + 4 + 2,
            triangles: 2 * surfaces + 2,
            shadow_casters: 4,
            // One shadow map, around everything.
            shadow_cascades: 1,
            point_lights: 0,
            spot_lights: 0,
            // Nothing is see-through.
            blended_surfaces: 0,
            // The scene asks for none of the optional effects.
            effects: wish_render::Effects::default(),
        }
    );
    assert_eq!(frame.drawn, 4);

    // The same numbers every time, and for any picture size but the size.
    let again = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
    assert_eq!(again.stats, frame.stats);
    let small = renderer.render(&gpu, &scene, None, 96, 64).unwrap();
    assert_eq!(
        small.stats,
        RenderStats {
            width: 96,
            height: 64,
            ..frame.stats
        }
    );

    // `describe` says what the lit picture of its view takes.
    let description = renderer.describe(&gpu, &scene, None, 320, 240).unwrap();
    assert_eq!(description.stats, frame.stats);

    // A debug view counts its own pass: no shadow map, no sky; the
    // wireframe draws every surface a second time as edges.
    let ids = renderer
        .render_view(&gpu, &scene, None, 320, 240, View::Ids)
        .unwrap();
    assert_eq!(
        ids.stats,
        RenderStats {
            width: 320,
            height: 240,
            draw_calls: 4,
            triangles: surfaces,
            ..RenderStats::default()
        }
    );
    let wire = renderer
        .render_view(&gpu, &scene, None, 320, 240, View::Wireframe)
        .unwrap();
    assert_eq!((wire.stats.draw_calls, wire.stats.triangles), (8, surfaces));
    let lit = renderer
        .render_view(&gpu, &scene, None, 320, 240, View::Lit)
        .unwrap();
    assert_eq!(lit.stats, frame.stats);
}

#[test]
fn lights_are_counted_when_they_light_some_part_of_the_view() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // A bulb and a spot in front of the camera, and a bulb far behind it.
    let (scene, _) = prepare(
        r#"{ "format": 1, "entities": {
            "camera": { "components": { "Camera": {}, "Transform": { "translation": [0, 2, 8] } } },
            "floor": { "components": { "Primitive": { "shape": "Plane" }, "Transform": { "scale": [20, 1, 20] } } },
            "bulb": { "components": { "PointLight": { "range": 5 }, "Transform": { "translation": [-2, 1, 0] } } },
            "spot": { "components": { "SpotLight": { "range": 6 }, "Transform": { "translation": [2, 3, 0], "rotation": [-0.7071068, 0, 0, 0.7071068] } } },
            "behind": { "components": { "PointLight": { "range": 3 }, "Transform": { "translation": [0, 1, 40] } } }
        } }"#,
    );
    assert_eq!(scene.local_lights.len(), 3);
    let frame = renderer.render(&gpu, &scene, None, 160, 120).unwrap();
    assert_eq!(
        (frame.stats.point_lights, frame.stats.spot_lights),
        (1, 1),
        "the bulb behind the camera lights nothing in view"
    );
    // No directional light: no shadow map, so the floor is drawn once.
    assert_eq!(frame.stats.shadow_casters, 0);
    assert_eq!(frame.stats.draw_calls, 1 + 2);
    assert_eq!(frame.stats.triangles, PLANE + 2);
}

#[test]
fn frames_of_one_size_share_their_targets_and_another_size_gets_its_own() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, _) = prepare(SCENE);
    let start = renderer.pool_counts();
    assert_eq!((start.textures, start.buffers), (0, 0));
    // Everything the device has made so far (the renderer's built-in shapes
    // and default textures): the device counts every texture and buffer,
    // whoever creates it, the pool or a pass on its own.
    let made_before = gpu.created();

    let first = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
    let after_first = renderer.pool_counts();
    // The HDR picture, its depth, the shadow map and the picture; the frame
    // and light uniforms, the three light lists, the draws' uniforms and
    // the picture's readback buffer.
    assert_eq!(after_first.textures, 4);
    assert_eq!(after_first.buffers, 7);
    // And that is all a frame of surfaces creates: nothing beside the pool.
    let made_first = gpu.created();
    assert_eq!(
        (
            made_first.textures - made_before.textures,
            made_first.buffers - made_before.buffers
        ),
        (4, 7)
    );

    // The same size again: nothing is created, by the pool or by anything
    // else, and the picture is the same.
    let second = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
    assert_eq!(renderer.pool_counts(), after_first);
    assert_eq!(gpu.created(), made_first, "a second frame creates nothing");
    assert_eq!(second.image, first.image);

    // Another size: an HDR picture, a depth buffer and a picture of that
    // size. The shadow map does not depend on the picture and is kept.
    let small = renderer.render(&gpu, &scene, None, 160, 120).unwrap();
    let after_small = renderer.pool_counts();
    assert_eq!(after_small.textures, after_first.textures + 3);
    assert_eq!(
        after_small.buffers,
        after_first.buffers + 1,
        "only the new picture's readback buffer: the frame's data fits the buffers there are"
    );
    assert_eq!((small.image.width, small.image.height), (160, 120));

    // Back to the first size: both are still there.
    let third = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
    assert_eq!(renderer.pool_counts(), after_small);
    assert_eq!(third.image, first.image);

    // The debug views share the depth buffer and the draws' uniform data
    // with the lit picture: an identity buffer of a size already drawn
    // makes only its own picture.
    renderer.id_buffer(&gpu, &scene, None, 320, 240).unwrap();
    let after_ids = renderer.pool_counts();
    assert_eq!(after_ids.textures, after_small.textures + 1);
    renderer.id_buffer(&gpu, &scene, None, 320, 240).unwrap();
    renderer.describe(&gpu, &scene, None, 320, 240).unwrap();
    assert_eq!(
        renderer.pool_counts().textures,
        after_ids.textures,
        "describe draws many identity pictures into one"
    );
}

#[test]
fn a_changed_size_never_draws_through_an_old_target() {
    let gpu = gpu();
    let (scene, _) = prepare(SCENE);
    // What each size looks like from a renderer that has drawn nothing else.
    let sizes = [
        (320, 240),
        (160, 120),
        (96, 64),
        (200, 100),
        (64, 64),
        (128, 96),
    ];
    let expected: Vec<_> = sizes
        .iter()
        .map(|(width, height)| {
            Renderer::new(&gpu)
                .unwrap()
                .render(&gpu, &scene, None, *width, *height)
                .unwrap()
                .image
        })
        .collect();
    // One renderer drawing them in turn, more sizes than the pool keeps,
    // forwards and then backwards, with identity buffers in between.
    let renderer = Renderer::new(&gpu).unwrap();
    let order = (0..sizes.len()).chain((0..sizes.len()).rev());
    for index in order {
        let (width, height) = sizes[index];
        let frame = renderer.render(&gpu, &scene, None, width, height).unwrap();
        assert_eq!(
            frame.image, expected[index],
            "{width}x{height} drawn after other sizes"
        );
        let ids = renderer
            .id_buffer(&gpu, &scene, None, height, width)
            .unwrap();
        assert_eq!((ids.width, ids.height), (height, width));
    }
}

#[test]
fn a_scene_that_changes_between_frames_is_drawn_from_its_own_data() {
    // Buffers are kept and filled again: a frame must never show what the
    // frame before it left in them.
    let gpu = gpu();
    let (full, _) = prepare(SCENE);
    let mut fewer = full.clone();
    fewer.draws.truncate(2);
    let (lights, _) = prepare(
        r#"{ "format": 1, "entities": {
            "camera": { "components": { "Camera": {}, "Transform": { "translation": [0, 2, 8] } } },
            "floor": { "components": { "Primitive": { "shape": "Plane" }, "Transform": { "scale": [20, 1, 20] } } },
            "bulb": { "components": { "PointLight": { "range": 5 }, "Transform": { "translation": [-2, 1, 0] } } }
        } }"#,
    );
    let scenes = [&full, &fewer, &lights, &full, &lights, &fewer];
    let expected: Vec<_> = scenes
        .iter()
        .map(|scene| {
            Renderer::new(&gpu)
                .unwrap()
                .render(&gpu, scene, None, 160, 120)
                .unwrap()
                .image
        })
        .collect();
    let renderer = Renderer::new(&gpu).unwrap();
    for (index, scene) in scenes.iter().enumerate() {
        let frame = renderer.render(&gpu, scene, None, 160, 120).unwrap();
        assert_eq!(frame.image, expected[index], "frame {index}");
    }
}

#[test]
fn shadows_can_be_switched_off_for_the_whole_picture() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (with, _) = prepare(&block_scene(""));
    let (off, _) = prepare(&block_scene(r#""RenderSettings": { "shadows": false }"#));
    assert!(with.settings.shadows && !off.settings.shadows);
    assert!(
        off.lights[0].shadows,
        "the light itself still asks for them"
    );
    let direction = with.lights[0].direction;
    assert!(
        (direction - Vec3::new(1.0, -1.0, 0.0).normalize()).length() < 1e-3,
        "the sun shines along +X and down: {direction:?}"
    );

    let shadowed = renderer.render(&gpu, &with, None, 320, 240).unwrap();
    let unshadowed = renderer.render(&gpu, &off, None, 320, 240).unwrap();
    assert_eq!(shadowed.stats.shadow_casters, 2);
    assert_eq!(unshadowed.stats.shadow_casters, 0);
    assert_eq!(
        unshadowed.stats.draw_calls + 2,
        shadowed.stats.draw_calls,
        "the two surfaces are not drawn into a shadow map"
    );

    // In the block's shadow the ground is dark with shadows and lit without;
    // in the open the two pictures agree.
    let inside = Vec3::new(0.0, 0.0, 0.0);
    let open = Vec3::new(3.0, 0.0, 0.0);
    let lit = luminance(pixel_at(&unshadowed, &off, open));
    let dark = luminance(pixel_at(&shadowed, &with, inside));
    assert!(dark * 10 < lit * 7, "in shadow: {dark} against {lit}");
    assert!((luminance(pixel_at(&unshadowed, &off, inside)) - lit).abs() <= 6);
    assert!((luminance(pixel_at(&shadowed, &with, open)) - lit).abs() <= 6);

    // The setting does exactly what a light that casts no shadows does.
    let mut light_off = with.clone();
    light_off.lights[0].shadows = false;
    let by_light = renderer.render(&gpu, &light_off, None, 320, 240).unwrap();
    assert_eq!(unshadowed.image, by_light.image);
}

#[test]
fn a_smaller_shadow_map_keeps_the_shadow_and_blurs_its_edge() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (fine, _) = prepare(&block_scene(""));
    let (coarse, _) = prepare(&block_scene(
        r#""RenderSettings": { "shadow_map_size": 64 }"#,
    ));
    assert_eq!(fine.settings.shadow_map_size, 2048);
    assert_eq!(coarse.settings.shadow_map_size, 64);
    let fine_frame = renderer.render(&gpu, &fine, None, 320, 240).unwrap();
    let coarse_frame = renderer.render(&gpu, &coarse, None, 320, 240).unwrap();

    // Walk across the shadow's far edge (at x = 1) from well inside the
    // shadow to well outside it in steps of 2.5 cm, and count the steps
    // that are neither dark nor lit.
    let walk = |frame: &Frame, scene: &RenderScene| {
        let at = |x: f32| luminance(pixel_at(frame, scene, Vec3::new(x, 0.0, 0.0)));
        let (dark, lit) = (at(-0.5), at(3.0));
        let band = (lit - dark) / 8;
        let between = (0..140)
            .map(|step| at(-0.5 + step as f32 * 0.025))
            .filter(|value| *value > dark + band && *value < lit - band)
            .count();
        (dark, lit, between)
    };
    let (fine_dark, fine_lit, fine_between) = walk(&fine_frame, &fine);
    let (coarse_dark, coarse_lit, coarse_between) = walk(&coarse_frame, &coarse);
    // The shadow is where it was, and as dark; the open ground is as lit.
    assert!(
        fine_dark * 10 < fine_lit * 7,
        "{fine_dark} against {fine_lit}"
    );
    assert!((coarse_dark - fine_dark).abs() <= 6 && (coarse_lit - fine_lit).abs() <= 6);
    // A map of 2048 texels over 12 m has texels of 6 mm: the edge is sharper
    // than a step of the walk (measured: 0 steps in between). At 64 a texel
    // is 19 cm, and the edge fades over many steps (measured: 18).
    assert!(fine_between <= 2, "a sharp edge: {fine_between} step(s)");
    assert!(
        coarse_between >= fine_between + 8,
        "a blurred edge: {coarse_between} step(s) against {fine_between}"
    );

    // A size outside the range (game code can write anything) is held
    // inside it instead of failing.
    let mut tiny = coarse.clone();
    tiny.settings.shadow_map_size = 0;
    let clamped = renderer.render(&gpu, &tiny, None, 320, 240).unwrap();
    assert_eq!(clamped.image, coarse_frame.image);
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

/// A 2 m square in the XY plane facing +Z with a 2x1 texture: its left half
/// (x < 0) is solid, its right half has an alpha of zero.
fn half_clear_quad(material: Value) -> wish_import::Model {
    textured_quad(material, (2, 1), &[200, 200, 200, 255, 200, 200, 200, 0])
}

/// A 2 m square in the XY plane facing +Z with a `width` x `height` texture
/// of these RGBA bytes stretched over it.
fn textured_quad(material: Value, (width, height): (u32, u32), rgba: &[u8]) -> wish_import::Model {
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
        let mut encoder = png::Encoder::new(&mut image, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(rgba)
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
    wish_import::load_model_bytes(&glb(&root, &binary), "assets/canopy.glb").unwrap()
}

#[test]
fn a_masked_caster_lets_light_through_its_holes() {
    let gpu = gpu();
    // The quad lies flat, 1.5 m above the ground, under a sun that shines
    // straight down; the camera looks under it from the side. Its solid half
    // is over x < 0, its cut-out half over x > 0.
    let (scene, _) = prepare(
        r#"{ "format": 1, "entities": {
            "camera": { "components": { "Camera": { "fov_y_deg": 50 }, "Transform": { "translation": [0, 0.8, 6] } } },
            "canopy": { "components": { "Model": { "asset": "assets/canopy.glb" },
                "Transform": { "translation": [0, 1.5, 0], "rotation": [-0.7071068, 0, 0, 0.7071068] } } },
            "ground": { "components": { "Material": { "base_color": [0.6, 0.6, 0.6, 1], "roughness": 0.9 },
                "Primitive": { "shape": "Plane" }, "Transform": { "scale": [12, 1, 12] } } },
            "sun": { "components": { "DirectionalLight": { "illuminance_lux": 100000 },
                "Transform": { "rotation": [-0.7071068, 0, 0, 0.7071068] } } }
        } }"#,
    );
    assert!((scene.lights[0].direction - Vec3::NEG_Y).length() < 1e-3);
    let texture = json!({ "baseColorTexture": { "index": 0 }, "metallicFactor": 0.0 });
    let draw = |material: Value| {
        let mut renderer = Renderer::new(&gpu).unwrap();
        renderer
            .upload_model(&gpu, "assets/canopy.glb", &half_clear_quad(material))
            .unwrap();
        let frame = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
        assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
        assert_eq!(frame.stats.shadow_casters, 2);
        frame
    };
    let under_solid = Vec3::new(-0.5, 0.0, 0.0);
    let under_hole = Vec3::new(0.5, 0.0, 0.0);
    let open = Vec3::new(2.5, 0.0, 0.0);

    let masked =
        draw(json!({ "alphaMode": "MASK", "alphaCutoff": 0.5, "pbrMetallicRoughness": texture }));
    let lit = luminance(pixel_at(&masked, &scene, open));
    let shade = luminance(pixel_at(&masked, &scene, under_solid));
    let hole = luminance(pixel_at(&masked, &scene, under_hole));
    assert!(
        shade * 10 < lit * 7,
        "under the solid half: {shade} against {lit}"
    );
    assert!(
        (hole - lit).abs() <= 6,
        "under the cut-out half the ground is lit: {hole} against {lit}"
    );

    // Without the mask the same texture's alpha means nothing: the whole
    // quad casts, as it always did.
    let opaque = draw(json!({ "pbrMetallicRoughness": texture }));
    let lit = luminance(pixel_at(&opaque, &scene, open));
    for point in [under_solid, under_hole] {
        let shade = luminance(pixel_at(&opaque, &scene, point));
        assert!(shade * 10 < lit * 7, "{point:?}: {shade} against {lit}");
    }
}

#[test]
fn a_fine_cut_out_still_casts_when_the_shadow_map_is_coarser_than_its_texture() {
    // Regression (closing review): the masked shadow pass sampled the base
    // colour at the mip level the *light's* view asked for. A fence or a
    // leaf card whose texture is finer than the shadow map was then read
    // from a level where alpha is the pattern's mean coverage, the cutoff
    // test failed everywhere, and the whole thing cast no shadow at all.
    let gpu = gpu();
    // The canopy of the test above, under a shadow map of 256 texels over
    // the 12 m ground: a shadow texel is 4.8 cm, a texel of the canopy's
    // 256 px texture is 0.8 cm, six times finer.
    let (scene, _) = prepare(
        r#"{ "format": 1,
          "resources": { "RenderSettings": { "shadow_map_size": 256 } },
          "entities": {
            "camera": { "components": { "Camera": { "fov_y_deg": 50 }, "Transform": { "translation": [0, 0.8, 6] } } },
            "canopy": { "components": { "Model": { "asset": "assets/canopy.glb" },
                "Transform": { "translation": [0, 1.5, 0], "rotation": [-0.7071068, 0, 0, 0.7071068] } } },
            "ground": { "components": { "Material": { "base_color": [0.6, 0.6, 0.6, 1], "roughness": 0.9 },
                "Primitive": { "shape": "Plane" }, "Transform": { "scale": [12, 1, 12] } } },
            "sun": { "components": { "DirectionalLight": { "illuminance_lux": 100000 },
                "Transform": { "rotation": [-0.7071068, 0, 0, 0.7071068] } } }
        } }"#,
    );
    assert_eq!(scene.settings.shadow_map_size, 256);
    // One texel in four is solid (every other one of every other row), so
    // from the first mip level on the alpha is a flat quarter: under the
    // cutoff of a half everywhere.
    let side = 256u32;
    let mut rgba = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let solid = x % 2 == 0 && y % 2 == 0;
            rgba.extend_from_slice(&[200, 200, 200, if solid { 255 } else { 0 }]);
        }
    }
    let texture = json!({ "baseColorTexture": { "index": 0 }, "metallicFactor": 0.0 });
    let draw = |material: Value| {
        let mut renderer = Renderer::new(&gpu).unwrap();
        renderer
            .upload_model(
                &gpu,
                "assets/canopy.glb",
                &textured_quad(material, (side, side), &rgba),
            )
            .unwrap();
        let frame = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
        assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
        frame
    };
    // The ground under the canopy, on a grid of 10 cm.
    let under = |frame: &Frame| -> Vec<i32> {
        let mut values = Vec::new();
        for row in -8..=8 {
            for column in -8..=8 {
                let point = Vec3::new(column as f32 * 0.1, 0.0, row as f32 * 0.1);
                values.push(luminance(pixel_at(frame, &scene, point)));
            }
        }
        values
    };
    let mean = |values: &[i32]| values.iter().sum::<i32>() / values.len() as i32;
    let open = Vec3::new(2.5, 0.0, 0.0);

    // As a solid sheet the canopy puts all of that ground in shade.
    let solid = draw(json!({ "pbrMetallicRoughness": texture }));
    let lit = luminance(pixel_at(&solid, &scene, open));
    let shade = mean(&under(&solid));
    assert!(shade * 10 < lit * 7, "a solid sheet: {shade} against {lit}");

    // Cut out, it lets most of the light through, and not all of it: each
    // shadow texel sees one point of the pattern, solid or not. (A map this
    // much coarser than a regular pattern shows it as patches of shade of
    // the right density, not as the pattern: that is all a map of this size
    // can hold.)
    let masked =
        draw(json!({ "alphaMode": "MASK", "alphaCutoff": 0.5, "pbrMetallicRoughness": texture }));
    assert!((luminance(pixel_at(&masked, &scene, open)) - lit).abs() <= 6);
    let values = under(&masked);
    let step = (lit - shade) / 12;
    let shaded = values.iter().filter(|value| **value < lit - step).count();
    // Measured: 66 of the 289 points, and a mean of 677 between the solid
    // sheet's 259 and open ground's 719 (with the old shader: no point, 719).
    assert!(
        shaded * 10 >= values.len(),
        "the cut-out casts some shadow: {shaded} of {} points are darker than open ground ({lit})",
        values.len()
    );
    assert!(
        mean(&values) > shade + (lit - shade) / 3,
        "and far less than the solid sheet: {} against {shade} (open ground {lit})",
        mean(&values)
    );
}

#[test]
fn more_than_one_directional_light_is_reported() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let source = SCENE.replace(
        r#""sun": { "components": {"#,
        r#""moon": { "components": {
      "DirectionalLight": { "illuminance_lux": 40000, "color": [0.4, 0.5, 1, 1] },
      "Transform": { "rotation": [-0.7071068, 0, 0, 0.7071068] } } },
    "sun": { "components": {"#,
    );
    assert_ne!(source, SCENE, "the scene was edited");
    let (mut two, entities) = prepare(&source);
    assert_eq!(two.lights.len(), 2);
    let extra_of = |warnings: &[Diagnostic]| -> Vec<Diagnostic> {
        warnings
            .iter()
            .filter(|warning| warning.code == "extra_directional_lights")
            .cloned()
            .collect()
    };
    let frame = renderer.render(&gpu, &two, None, 160, 120).unwrap();
    let extra = extra_of(&frame.warnings);
    assert_eq!(extra.len(), 1, "{:?}", frame.warnings);
    // Without names it says which is used and which is not by their ids, in
    // the form the tools write an entity that has no name.
    let (used, unused) = (two.lights[0].entity, two.lights[1].entity);
    assert!(
        extra[0].message.contains("2 directional lights"),
        "{}",
        extra[0].message
    );
    assert!(
        extra[0]
            .message
            .contains(&format!("entity #{used}, the first in entity order")),
        "it names the light that is used: {}",
        extra[0].message
    );
    assert!(
        extra[0]
            .message
            .contains(&format!("Not drawn: entity #{unused}.")),
        "and the one that is not: {}",
        extra[0].message
    );
    assert!(extra[0].hint.contains("DirectionalLight"));
    assert!(extra[0].hint.contains("siblings by `order`, then by id"));

    // A caller that knows what the entities are called says so, and the
    // warning uses the names: an id like 5v0 cannot be acted on.
    two.names = entities
        .iter()
        .map(|(name, entity)| (*entity, name.clone()))
        .collect();
    // Both have the same `order`, so the scene's order is by id: the moon
    // comes first and is the one that lights the picture.
    assert_eq!((used, unused), (entities["moon"], entities["sun"]));
    let named = renderer.render(&gpu, &two, None, 160, 120).unwrap();
    assert_eq!(named.image, frame.image, "names change no picture");
    let extra = extra_of(&named.warnings);
    assert_eq!(extra.len(), 1, "{:?}", named.warnings);
    assert_eq!(
        extra[0].message,
        "The scene has 2 directional lights and only one lights the picture: entity 'moon', the first in entity order. Not drawn: entity 'sun'."
    );

    // Whichever way the scene is looked at says it: `describe` gives the lit
    // picture's warnings, and the debug views, which light nothing, still
    // say what the scene's lights mean for the picture.
    let description = renderer.describe(&gpu, &two, None, 160, 120).unwrap();
    assert_eq!(description.warnings, named.warnings);
    for view in [View::Ids, View::Depth, View::Wireframe, View::Lights] {
        let debug = renderer
            .render_view(&gpu, &two, None, 160, 120, view)
            .unwrap();
        assert_eq!(extra_of(&debug.warnings), extra, "{}", view.name());
    }

    // Only the first lights the picture: without the second it is the same.
    let mut one = two.clone();
    one.lights.truncate(1);
    let alone = renderer.render(&gpu, &one, None, 160, 120).unwrap();
    assert!(alone.warnings.is_empty(), "{:?}", alone.warnings);
    assert_eq!(alone.image, frame.image);

    // Many of them: four are named, the rest counted.
    let mut many = two.clone();
    let sun = many.lights[1];
    many.lights.extend(std::iter::repeat_n(sun, 5));
    let crowded = renderer.render(&gpu, &many, None, 160, 120).unwrap();
    let extra = extra_of(&crowded.warnings);
    assert!(
        extra[0].message.contains("7 directional lights")
            && extra[0]
                .message
                .ends_with("Not drawn: entities 'sun', 'sun', 'sun', 'sun' and 2 more."),
        "{}",
        extra[0].message
    );
}

#[test]
fn what_cannot_be_drawn_is_named_the_way_the_scene_names_it() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (mut scene, entities) = prepare(
        r#"{ "format": 1, "entities": {
            "c": { "components": { "Camera": {}, "Transform": { "translation": [0, 0, 5] } } },
            "flat": { "components": { "Primitive": { "shape": "Cube" }, "Transform": { "scale": [1, 0, 1] } } },
            "hat": { "components": { "Model": { "asset": "assets/hat.glb" } } }
        } }"#,
    );
    scene.names = entities
        .iter()
        .filter(|(name, _)| name.as_str() != "hat")
        .map(|(name, entity)| (*entity, name.clone()))
        .collect();
    let frame = renderer.render(&gpu, &scene, None, 32, 32).unwrap();
    let messages: Vec<&str> = frame.warnings.iter().map(|w| w.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "Entity 'flat' was not drawn: its scale is zero or not finite.".to_owned(),
            // An entity nobody named keeps its id.
            format!(
                "Entity #{} was not drawn: the model 'assets/hat.glb' has not been loaded.",
                entities["hat"]
            ),
        ]
    );
}

#[test]
fn describe_and_the_debug_views_say_what_the_lit_picture_says_about_lights() {
    // Regression (closing review): `describe` planned the lit picture for
    // its statistics and threw the plan's warnings away, so a scene with
    // more lights than can be drawn had its counts cut silently.
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (mut scene, _) = prepare(
        r#"{ "format": 1, "entities": {
            "camera": { "components": { "Camera": {}, "Transform": { "translation": [0, 2, 8] } } },
            "floor": { "components": { "Primitive": { "shape": "Plane" }, "Transform": { "scale": [20, 1, 20] } } },
            "bulb": { "components": { "PointLight": { "range": 30 }, "Transform": { "translation": [0, 1, 0] } } }
        } }"#,
    );
    // 1025 bulbs in one place: one more than is drawn, and far more than
    // one part of the picture can list.
    let bulb = scene.local_lights[0];
    scene.local_lights = vec![bulb; 1025];
    let codes = |warnings: &[Diagnostic]| -> Vec<String> {
        warnings.iter().map(|w| w.code.to_string()).collect()
    };
    let frame = renderer.render(&gpu, &scene, None, 64, 48).unwrap();
    assert_eq!(
        codes(&frame.warnings),
        ["too_many_lights", "light_clusters_full"]
    );
    assert!(frame.warnings.iter().all(|w| !w.hint.is_empty()));
    let description = renderer.describe(&gpu, &scene, None, 64, 48).unwrap();
    assert_eq!(description.warnings, frame.warnings);
    assert_eq!(description.stats, frame.stats);
    // The debug views say what holds for the scene whatever the view (how
    // many lights one part of the picture lists is the lit picture's own).
    let lights = renderer
        .render_view(&gpu, &scene, None, 64, 48, View::Lights)
        .unwrap();
    assert_eq!(codes(&lights.warnings), ["too_many_lights"]);
    assert_eq!(lights.warnings[0], frame.warnings[0]);
}

#[test]
fn a_renderer_lets_go_of_its_targets_when_asked() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let (scene, _) = prepare(SCENE);
    assert_eq!(renderer.kept_bytes(), 0, "nothing is kept before a frame");
    let first = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
    // The HDR picture (8 bytes a pixel), the depth buffer (4), the picture
    // (4) and its readback buffer (4: a row of 1280 bytes needs no padding),
    // the 2048 shadow map (4), and the frame's seven small buffers.
    let targets = 320 * 240 * (8 + 4 + 4 + 4) + 2048 * 2048 * 4;
    let kept = renderer.kept_bytes();
    assert!(
        kept > targets && kept < targets + 16 * 1024,
        "{kept} bytes kept, {targets} of them targets"
    );
    let made = renderer.pool_counts();

    // A limit it is inside of lets go of nothing.
    renderer.trim(kept);
    assert_eq!(renderer.kept_bytes(), kept);
    renderer.render(&gpu, &scene, None, 320, 240).unwrap();
    assert_eq!(renderer.pool_counts(), made);

    // Under a megabyte: the largest go first, the shadow map (16 MiB) and
    // then the HDR picture (600 KiB). The next frame makes those two again
    // and draws the same picture.
    renderer.trim(1 << 20);
    let left = renderer.kept_bytes();
    assert_eq!(left, kept - 2048 * 2048 * 4 - 320 * 240 * 8);
    let again = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
    assert_eq!(renderer.pool_counts().textures, made.textures + 2);
    assert_eq!(renderer.pool_counts().buffers, made.buffers);
    assert_eq!(again.image, first.image);
    assert_eq!(renderer.kept_bytes(), kept);

    // Everything: the next frame starts from nothing, as the first did.
    renderer.trim(0);
    assert_eq!(renderer.kept_bytes(), 0);
    let fresh = renderer.render(&gpu, &scene, None, 320, 240).unwrap();
    assert_eq!(renderer.pool_counts().textures, made.textures + 2 + 4);
    assert_eq!(renderer.pool_counts().buffers, made.buffers + 7);
    assert_eq!(fresh.image, first.image);
}

#[test]
fn targets_nobody_uses_any_more_are_dropped() {
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    // No light: no shadow map to draw, so the frames are quick.
    let (scene, _) = prepare(
        r#"{ "format": 1, "entities": {
            "camera": { "components": { "Camera": {}, "Transform": { "translation": [0, 2, 8] } } },
            "floor": { "components": { "Primitive": { "shape": "Plane" }, "Transform": { "scale": [20, 1, 20] } } }
        } }"#,
    );
    let large = renderer.render(&gpu, &scene, None, 200, 150).unwrap();
    renderer.render(&gpu, &scene, None, 32, 32).unwrap();
    let both = renderer.pool_counts();
    // A few frames of the small size later the large targets are still
    // there...
    for _ in 0..8 {
        renderer.render(&gpu, &scene, None, 32, 32).unwrap();
    }
    renderer.render(&gpu, &scene, None, 200, 150).unwrap();
    assert_eq!(renderer.pool_counts(), both);
    // ...but after a long time unused they are not kept for ever: the HDR
    // picture, the depth buffer and the picture are made again.
    for _ in 0..80 {
        renderer.render(&gpu, &scene, None, 32, 32).unwrap();
    }
    assert_eq!(renderer.pool_counts(), both, "the small size is in use");
    let again = renderer.render(&gpu, &scene, None, 200, 150).unwrap();
    assert_eq!(renderer.pool_counts().textures, both.textures + 3);
    assert_eq!(again.image, large.image);
}

/// The scene of the lit golden image seen from `count` cameras, each
/// further to the right than the one before.
fn views_from_the_side(count: usize) -> Vec<RenderScene> {
    let (scene, _) = prepare(SCENE);
    (0..count)
        .map(|step| {
            let mut moved = scene.clone();
            let camera = &mut moved.cameras[0];
            let shift = 0.6 * step as f32;
            camera.view *= wish_core::math::Mat4::from_translation(Vec3::new(-shift, 0.0, 0.0));
            camera.position.x += shift;
            moved
        })
        .collect()
}

/// What each view looks like, drawn on a device of its own by a renderer
/// that draws nothing else.
fn pictures_of(views: &[RenderScene]) -> Vec<wish_render::RgbaImage> {
    let own = gpu();
    views
        .iter()
        .map(|view| {
            Renderer::new(&own)
                .unwrap()
                .render(&own, view, None, 160, 120)
                .unwrap()
                .image
        })
        .collect()
}

#[test]
fn two_renderers_taking_turns_on_one_device_each_draw_their_own_frames() {
    // Regression (found by the editor's tests while this was built): on the
    // software adapter of Windows, a bind group over a uniform buffer that
    // one renderer kept from its last frame read the uniform buffer of
    // whichever renderer had made such a group last, so a renderer drew the
    // other one's camera. Bind groups are built in the frame that uses them.
    let gpu = Gpu::shared(GpuOptions { software: true }).unwrap();
    let views = views_from_the_side(4);
    let first = Renderer::new(&gpu).unwrap();
    let second = Renderer::new(&gpu).unwrap();
    let turns = [
        (&first, 0),
        (&second, 1),
        (&first, 2),
        (&second, 3),
        (&first, 0),
        (&second, 1),
        (&first, 1),
    ];
    let drawn: Vec<_> = turns
        .iter()
        .map(|(renderer, view)| {
            renderer
                .render(&gpu, &views[*view], None, 160, 120)
                .unwrap()
                .image
        })
        .collect();
    let expected = pictures_of(&views);
    assert_ne!(expected[0], expected[1], "the cameras differ");
    for (turn, ((_, view), image)) in turns.iter().zip(&drawn).enumerate() {
        let shows = expected.iter().position(|picture| picture == image);
        assert_eq!(
            shows,
            Some(*view),
            "turn {turn}: which view the picture shows"
        );
    }
    // The identity buffers and the debug views take turns the same way.
    let ids: Vec<_> = [(&first, 0), (&second, 3), (&first, 2), (&second, 1)]
        .iter()
        .map(|(renderer, view)| {
            (
                *view,
                renderer
                    .id_buffer(&gpu, &views[*view], None, 160, 120)
                    .unwrap(),
            )
        })
        .collect();
    let alone = Renderer::new(&gpu).unwrap();
    for (view, buffer) in ids {
        let expected = alone.id_buffer(&gpu, &views[view], None, 160, 120).unwrap();
        assert_eq!(
            buffer.raw(),
            expected.raw(),
            "the identity buffer of view {view}"
        );
    }
}

#[test]
fn renderers_on_several_threads_share_a_device_without_mixing_frames() {
    // The editor's windows, the tools and the tests of one process share one
    // device and take turns on it from several threads.
    let gpu = Gpu::shared(GpuOptions { software: true }).unwrap();
    let views = std::sync::Arc::new(views_from_the_side(6));
    let expected = std::sync::Arc::new(pictures_of(&views));
    let threads: Vec<_> = (0..4)
        .map(|thread| {
            let gpu = std::sync::Arc::clone(&gpu);
            let views = std::sync::Arc::clone(&views);
            let expected = std::sync::Arc::clone(&expected);
            std::thread::spawn(move || {
                let renderer = Renderer::new(&gpu).unwrap();
                for round in 0..12 {
                    let index = (round + thread) % views.len();
                    let frame = renderer
                        .render(&gpu, &views[index], None, 160, 120)
                        .unwrap();
                    let shows = expected.iter().position(|image| *image == frame.image);
                    assert_eq!(
                        shows,
                        Some(index),
                        "thread {thread}, round {round}: which view the picture shows"
                    );
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}
