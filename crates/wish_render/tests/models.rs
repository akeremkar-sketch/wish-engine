//! Drawing glTF models: textures, normal maps, alpha masks, double-sided
//! materials, on the software adapter.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use wish_ecs::World;
use wish_render::{Gpu, GpuOptions, RenderScene, Renderer, RgbaImage, extract};
use wish_scene::load;
use wish_transform::update_global_transforms;

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

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn png_rgba(width: u32, height: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(data)
        .unwrap();
    out
}

/// A 2 m square in the XY plane facing +Z, centred on the origin, with UVs
/// covering the texture once, and the given material and optional texture.
/// `texture` is a 2x1 image: left pixel, right pixel.
fn quad_model(material: Value, texture: Option<[[u8; 4]; 2]>) -> wish_import::Model {
    let positions = floats(&[
        -1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 1.0, 0.0, -1.0, 1.0, 0.0,
    ]);
    let normals = floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
    let uvs = floats(&[0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0]);
    let indices: Vec<u8> = [0u16, 1, 2, 0, 2, 3]
        .iter()
        .flat_map(|i| i.to_le_bytes())
        .collect();
    let mut binary = Vec::new();
    let mut views = Vec::new();
    for part in [&positions, &normals, &uvs, &indices] {
        views.push(json!({ "buffer": 0, "byteOffset": binary.len(), "byteLength": part.len() }));
        binary.extend_from_slice(part);
    }
    let mut root = json!({
        "asset": { "version": "2.0" },
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [-1, -1, 0], "max": [1, 1, 0] },
            { "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3" },
            { "bufferView": 2, "componentType": 5126, "count": 4, "type": "VEC2" },
            { "bufferView": 3, "componentType": 5123, "count": 6, "type": "SCALAR" }
        ],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "indices": 3, "material": 0 }] }],
        "materials": [material],
        "nodes": [{ "mesh": 0 }],
        "scenes": [{ "nodes": [0] }],
        "scene": 0
    });
    if let Some([left, right]) = texture {
        let image = png_rgba(2, 1, &[left, right].concat());
        while !binary.len().is_multiple_of(4) {
            binary.push(0);
        }
        views.push(json!({ "buffer": 0, "byteOffset": binary.len(), "byteLength": image.len() }));
        binary.extend_from_slice(&image);
        let view = views.len() - 1;
        root["images"] = json!([{ "bufferView": view, "mimeType": "image/png" }]);
        root["textures"] = json!([{ "source": 0 }]);
    }
    root["bufferViews"] = Value::Array(views);
    root["buffers"] = json!([{ "byteLength": binary.len().div_ceil(4) * 4 }]);
    wish_import::load_model_bytes(&glb(&root, &binary), "assets/quad.glb").unwrap()
}

fn scene_with_model() -> RenderScene {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    let source = r#"{ "format": 1, "entities": {
        "camera": { "components": { "Camera": { "fov_y_deg": 40 }, "Transform": { "translation": [0, 0, 5] } } },
        "sun": { "components": { "DirectionalLight": { "illuminance_lux": 100000 }, "Transform": { "rotation": [0, 0, 0, 1] } } },
        "quad": { "components": { "Model": { "asset": "assets/quad.glb" } } }
    } }"#;
    let result = load(source, "s.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap().instantiate(&mut world).unwrap();
    update_global_transforms(&mut world).unwrap();
    extract(&mut world).unwrap()
}

fn draw(model: &wish_import::Model) -> (RgbaImage, Vec<String>) {
    let gpu = Gpu::shared(GpuOptions { software: true }).unwrap();
    let mut renderer = Renderer::new(&gpu).unwrap();
    renderer
        .upload_model(&gpu, "assets/quad.glb", model)
        .unwrap();
    let frame = renderer
        .render(&gpu, &scene_with_model(), None, 160, 120)
        .unwrap();
    let codes = frame.warnings.iter().map(|w| w.code.to_string()).collect();
    (frame.image, codes)
}

fn left(image: &RgbaImage) -> [u8; 4] {
    image.pixel(60, 60).unwrap()
}

fn right(image: &RgbaImage) -> [u8; 4] {
    image.pixel(100, 60).unwrap()
}

fn sky(image: &RgbaImage) -> [u8; 4] {
    image.pixel(5, 5).unwrap()
}

#[test]
fn a_model_is_drawn_with_its_textures() {
    let model = quad_model(
        json!({ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 }, "metallicFactor": 0.0, "roughnessFactor": 1.0 } }),
        Some([[230, 20, 20, 255], [20, 230, 20, 255]]),
    );
    let (image, warnings) = draw(&model);
    assert!(warnings.is_empty(), "{warnings:?}");
    let (l, r) = (left(&image), right(&image));
    // The sun shines along -Z onto a quad facing +Z: it is lit, and the
    // texture's left half is red, its right half green.
    assert!(l[0] > 2 * l[1] && l[0] > 80, "left {l:?}");
    assert!(r[1] > 2 * r[0] && r[1] > 80, "right {r:?}");
    assert_ne!(sky(&image), l);
}

#[test]
fn factors_alone_colour_an_untextured_material() {
    let model = quad_model(
        json!({ "pbrMetallicRoughness": { "baseColorFactor": [0.05, 0.1, 0.45, 1.0], "metallicFactor": 0.0, "roughnessFactor": 1.0 } }),
        None,
    );
    let (image, _) = draw(&model);
    let c = left(&image);
    let wide = c.map(u16::from);
    assert!(wide[2] > 2 * wide[0] && wide[2] > wide[1] + 20, "{c:?}");
    // Both halves are the same colour.
    let d = right(&image);
    assert!(
        c.iter().zip(d).all(|(a, b)| a.abs_diff(b) <= 2),
        "{c:?} {d:?}"
    );
}

#[test]
fn alpha_mask_cuts_holes_and_opaque_ignores_alpha() {
    let texture = Some([[200, 200, 200, 255], [200, 200, 200, 0]]);
    let masked = quad_model(
        json!({ "alphaMode": "MASK", "alphaCutoff": 0.5, "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 }, "metallicFactor": 0.0 } }),
        texture,
    );
    let (image, _) = draw(&masked);
    assert_ne!(left(&image), sky(&image), "the opaque half is drawn");
    // Where alpha is 0 the sky shows through: same colour as at the same
    // height outside the quad.
    let behind = image.pixel(150, 60).unwrap();
    let hole = right(&image);
    assert!(
        behind.iter().zip(hole).all(|(a, b)| a.abs_diff(b) <= 3),
        "hole {hole:?} should show the sky {behind:?}"
    );

    let opaque = quad_model(
        json!({ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 }, "metallicFactor": 0.0 } }),
        texture,
    );
    let (image, _) = draw(&opaque);
    assert_ne!(
        right(&image),
        image.pixel(150, 60).unwrap(),
        "no mask, no hole"
    );
}

#[test]
fn double_sided_materials_show_their_back() {
    // Turn the camera around the quad by drawing it from behind: the quad's
    // front faces +Z, the camera sits at +Z, so rotate the model instead.
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    let source = r#"{ "format": 1, "entities": {
        "camera": { "components": { "Camera": {}, "Transform": { "translation": [0, 0, 5] } } },
        "quad": { "components": { "Model": { "asset": "assets/quad.glb" }, "Transform": { "rotation": [0, 1, 0, 0] } } }
    } }"#;
    let result = load(source, "s.scene.json", world.registry());
    result.document.unwrap().instantiate(&mut world).unwrap();
    update_global_transforms(&mut world).unwrap();
    let scene = extract(&mut world).unwrap();
    let gpu = Gpu::shared(GpuOptions { software: true }).unwrap();
    let mut renderer = Renderer::new(&gpu).unwrap();
    let factors = |double: bool| {
        quad_model(
            json!({ "doubleSided": double, "pbrMetallicRoughness": { "baseColorFactor": [0.9, 0.1, 0.1, 1.0], "metallicFactor": 0.0 } }),
            None,
        )
    };
    let image = |renderer: &mut Renderer, double| {
        renderer
            .upload_model(&gpu, "assets/quad.glb", &factors(double))
            .unwrap();
        renderer.render(&gpu, &scene, None, 160, 120).unwrap().image
    };
    let one_sided = image(&mut renderer, false);
    assert!(
        left(&one_sided)
            .iter()
            .zip(one_sided.pixel(150, 60).unwrap())
            .all(|(a, b)| a.abs_diff(b) <= 3),
        "a single-sided quad seen from behind is invisible"
    );
    let two_sided = image(&mut renderer, true);
    let c = left(&two_sided);
    assert!(
        c[0] > 2 * c[1],
        "a double-sided quad shows its red back: {c:?}"
    );
}

#[test]
fn a_normal_map_changes_the_lighting() {
    // Same quad, lit by a sun shining straight at it. A flat normal map
    // gives the plain result; a map that tilts the normal away from the sun
    // (towards +X, with the sun along -Z the surface still faces it a bit
    // less) darkens it.
    let flat = quad_model(
        json!({ "pbrMetallicRoughness": { "baseColorFactor": [0.8, 0.8, 0.8, 1.0], "metallicFactor": 0.0, "roughnessFactor": 1.0 } }),
        None,
    );
    let (flat_image, _) = draw(&flat);
    let plain = left(&flat_image);
    let mut tilted = flat.clone();
    // A 1x1 normal texture tilted 45 degrees towards +X: (1, 0, 1) normalised.
    tilted.textures.push(wish_import::Texture {
        width: 1,
        height: 1,
        rgba: vec![255, 128, 255, 255],
    });
    tilted.materials[0].normal_texture = Some(wish_import::TextureRef { texture: 0 });
    for primitive in &mut tilted.meshes[0].primitives {
        // The quad's U axis is +X and the image's up (the green channel)
        // is +Y: tangent +X with sign +1, as the importer generates it.
        primitive.tangents =
            vec![wish_core::math::Vec4::new(1.0, 0.0, 0.0, 1.0); primitive.positions.len()];
    }
    let (tilted_image, _) = draw(&tilted);
    let bent = left(&tilted_image);
    assert!(
        bent[0] < plain[0],
        "the tilted normal gets less direct light: {bent:?} vs {plain:?}"
    );
}

#[test]
fn shaders_can_be_reloaded_from_files_and_a_mistake_changes_nothing() {
    let gpu = Gpu::shared(GpuOptions { software: true }).unwrap();
    let model = quad_model(
        json!({ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 }, "metallicFactor": 0.0, "roughnessFactor": 1.0 } }),
        Some([[230, 20, 20, 255], [20, 230, 20, 255]]),
    );
    let mut renderer = Renderer::new(&gpu).unwrap();
    renderer
        .upload_model(&gpu, "assets/quad.glb", &model)
        .unwrap();
    let scene = scene_with_model();
    let before = renderer.render(&gpu, &scene, None, 160, 120).unwrap().image;

    // A developer's copy of the sky pass that paints it pure blue.
    let dir = std::env::temp_dir().join(format!("wish-shader-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let original = include_str!("../src/shaders/sky_pass.wgsl");
    let edited = original.replace(
        "return vec4<f32>(sky_radiance(direction) * frame.exposure.x, 1.0);",
        "return vec4<f32>(0.0, 0.0, 10000.0 * frame.exposure.x, 1.0);",
    );
    assert_ne!(original, edited);
    std::fs::write(dir.join("sky_pass.wgsl"), &edited).unwrap();
    let (library, replaced) = wish_render::engine_shaders_from_dir(&dir).unwrap();
    assert_eq!(replaced, ["sky_pass"]);
    renderer.reload_shaders(&gpu, &library).unwrap();
    assert!(renderer.has_model("assets/quad.glb"), "models survive");
    let after = renderer.render(&gpu, &scene, None, 160, 120).unwrap().image;
    let (old, new) = (sky(&before), sky(&after));
    assert!(
        new[2] > 150 && new[0] < 20,
        "the sky is now blue: {old:?} -> {new:?}"
    );
    assert_ne!(old, new);
    // The model is still drawn, with its texture.
    let l = left(&after);
    assert!(l[0] > 2 * l[1] && l[0] > 80, "left {l:?}");

    // A mistake is reported with its file and line, and nothing changes.
    std::fs::write(
        dir.join("sky_pass.wgsl"),
        edited.replace("1.0);\n}", "1.0)\n}"),
    )
    .unwrap();
    let (broken, _) = wish_render::engine_shaders_from_dir(&dir).unwrap();
    let problem = renderer.reload_shaders(&gpu, &broken).unwrap_err();
    assert_eq!(problem.code, "shader_syntax");
    assert!(
        problem
            .location
            .file
            .as_deref()
            .unwrap_or("")
            .ends_with("sky_pass.wgsl"),
        "{problem}"
    );
    assert!(!problem.hint.is_empty());
    let still = renderer.render(&gpu, &scene, None, 160, 120).unwrap().image;
    assert_eq!(sky(&still), new);
    std::fs::remove_dir_all(&dir).unwrap();
}
