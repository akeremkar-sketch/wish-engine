//! glTF import, on small files built in the tests.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use wish_core::math::{Mat4, Vec3};
use wish_import::{AlphaMode, Model, load_model_bytes, load_model_file};

/// Builds a GLB: JSON chunk plus one binary chunk.
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
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
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

fn indices16(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// A quad in the XY plane: 4 vertices, 2 triangles, with normals and UVs.
/// Returns the binary data and the accessor/bufferView JSON.
fn quad() -> (Vec<u8>, Vec<Value>, Vec<Value>) {
    let positions = floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0]);
    let normals = floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
    let uvs = floats(&[0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0]);
    let indices = indices16(&[0, 1, 2, 0, 2, 3]);
    let mut binary = Vec::new();
    let mut views = Vec::new();
    for part in [&positions, &normals, &uvs, &indices] {
        views.push(json!({ "buffer": 0, "byteOffset": binary.len(), "byteLength": part.len() }));
        binary.extend_from_slice(part);
    }
    let accessors = vec![
        json!({ "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0] }),
        json!({ "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3" }),
        json!({ "bufferView": 2, "componentType": 5126, "count": 4, "type": "VEC2" }),
        json!({ "bufferView": 3, "componentType": 5123, "count": 6, "type": "SCALAR" }),
    ];
    (binary, views, accessors)
}

fn document(
    binary: &[u8],
    views: Vec<Value>,
    accessors: Vec<Value>,
    primitives: Value,
    extra: Value,
) -> Vec<u8> {
    let mut root = json!({
        "asset": { "version": "2.0" },
        "buffers": [{ "byteLength": binary.len().div_ceil(4) * 4 }],
        "bufferViews": views,
        "accessors": accessors,
        "meshes": [{ "name": "quad", "primitives": primitives }],
        "nodes": [{ "mesh": 0 }],
        "scenes": [{ "nodes": [0] }],
        "scene": 0,
    });
    if let (Some(target), Value::Object(extra)) = (root.as_object_mut(), extra) {
        target.extend(extra);
    }
    glb(&root, binary)
}

fn textured_quad(extra: Value) -> Model {
    let (binary, views, accessors) = quad();
    let primitives = json!([{ "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "indices": 3, "material": 0 }]);
    load_model_bytes(
        &document(&binary, views, accessors, primitives, extra),
        "quad.glb",
    )
    .unwrap()
}

fn png(width: u32, height: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(data)
        .unwrap();
    out
}

#[test]
fn a_quad_with_a_material_imports_completely() {
    let model = textured_quad(json!({
        "materials": [{
            "name": "gold",
            "pbrMetallicRoughness": { "baseColorFactor": [1.0, 0.8, 0.2, 1.0], "metallicFactor": 1.0, "roughnessFactor": 0.3 },
            "emissiveFactor": [0.1, 0.2, 0.3],
            "doubleSided": true,
            "alphaMode": "MASK",
            "alphaCutoff": 0.25
        }]
    }));
    assert!(model.warnings.is_empty(), "{:?}", model.warnings);
    assert_eq!(model.meshes.len(), 1);
    let primitive = &model.meshes[0].primitives[0];
    assert_eq!(primitive.positions.len(), 4);
    assert_eq!(primitive.indices, [0, 1, 2, 0, 2, 3]);
    assert_eq!(primitive.normals[0], Vec3::Z);
    assert_eq!(primitive.uvs[2].to_array(), [1.0, 0.0]);
    assert_eq!(primitive.material, Some(0));
    let material = &model.materials[0];
    assert_eq!(material.name.as_deref(), Some("gold"));
    assert_eq!(material.base_color, [1.0, 0.8, 0.2, 1.0]);
    assert_eq!((material.metallic, material.roughness), (1.0, 0.3));
    assert_eq!(material.emissive, [0.1, 0.2, 0.3]);
    assert!(material.double_sided);
    assert_eq!(
        (material.alpha_mode, material.alpha_cutoff),
        (AlphaMode::Mask, 0.25)
    );
    // No normal texture: tangents are not generated.
    assert!(primitive.tangents.iter().all(|t| t.length() == 0.0));
    let (low, high) = model.bounds().unwrap();
    assert_eq!((low, high), (Vec3::ZERO, Vec3::new(1.0, 1.0, 0.0)));
}

#[test]
fn textures_are_decoded_to_rgba_and_linked_to_materials() {
    let (mut binary, mut views, accessors) = quad();
    let colour = png(2, 1, png::ColorType::Rgb, &[255, 0, 0, 0, 255, 0]);
    let grey = png(1, 2, png::ColorType::Grayscale, &[10, 200]);
    let mut image_views = Vec::new();
    for data in [&colour, &grey] {
        while !binary.len().is_multiple_of(4) {
            binary.push(0);
        }
        image_views.push(views.len());
        views.push(json!({ "buffer": 0, "byteOffset": binary.len(), "byteLength": data.len() }));
        binary.extend_from_slice(data);
    }
    let primitives = json!([{ "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "indices": 3, "material": 0 }]);
    let bytes = document(
        &binary,
        views,
        accessors,
        primitives,
        json!({
            "images": [
                { "bufferView": image_views[0], "mimeType": "image/png" },
                { "bufferView": image_views[1], "mimeType": "image/png" }
            ],
            "textures": [{ "source": 0 }, { "source": 1 }],
            "materials": [{
                "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } },
                "occlusionTexture": { "index": 1, "strength": 0.5 },
                "normalTexture": { "index": 1, "scale": 2.0 }
            }]
        }),
    );
    let model = load_model_bytes(&bytes, "textured.glb").unwrap();
    assert!(model.warnings.is_empty(), "{:?}", model.warnings);
    assert_eq!(model.textures.len(), 2);
    let colour = &model.textures[0];
    assert_eq!((colour.width, colour.height), (2, 1));
    assert_eq!(
        colour.rgba,
        [255, 0, 0, 255, 0, 255, 0, 255],
        "RGB gains an opaque alpha"
    );
    let grey = &model.textures[1];
    assert_eq!(
        grey.rgba,
        [10, 10, 10, 255, 200, 200, 200, 255],
        "grey is spread to RGB"
    );
    let material = &model.materials[0];
    assert_eq!(material.base_color_texture.unwrap().texture, 0);
    assert_eq!(material.occlusion_texture.unwrap().texture, 1);
    assert_eq!(
        (material.occlusion_strength, material.normal_scale),
        (0.5, 2.0)
    );
    // A normal texture asks for tangents, and they are usable: unit length,
    // perpendicular to the normal, with a sign. The quad's U axis is +X.
    for tangent in &model.meshes[0].primitives[0].tangents {
        assert!(
            (tangent.truncate().length() - 1.0).abs() < 1e-5,
            "{tangent:?}"
        );
        assert!(tangent.truncate().dot(Vec3::Z).abs() < 1e-5);
        assert!(
            (tangent.truncate() - Vec3::X).length() < 1e-5,
            "{tangent:?}"
        );
        // glTF: the normal map's green channel points up in the image,
        // towards smaller V; here V runs down the quad, so up is +Y and
        // cross(normal, tangent) = Z x X = +Y already: the sign is +1.
        assert_eq!(tangent.w, 1.0, "{tangent:?}");
    }
}

#[test]
fn the_node_tree_places_primitives() {
    let (binary, views, accessors) = quad();
    let primitives =
        json!([{ "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "indices": 3 }]);
    let bytes = document(
        &binary,
        views,
        accessors,
        primitives,
        json!({
            "nodes": [
                { "name": "group", "translation": [10.0, 0.0, 0.0], "scale": [2.0, 2.0, 2.0], "children": [1, 2] },
                { "name": "a", "mesh": 0, "translation": [0.0, 1.0, 0.0] },
                { "name": "b", "mesh": 0, "children": [3] },
                { "name": "c", "mesh": 0, "translation": [0.0, 0.0, 5.0] }
            ],
            "scenes": [{ "nodes": [0] }]
        }),
    );
    let model = load_model_bytes(&bytes, "tree.glb").unwrap();
    assert_eq!(model.roots, [0]);
    let placed = model.placed();
    assert_eq!(placed.len(), 3, "a, b and c each show the quad");
    let origins: Vec<Vec3> = placed
        .iter()
        .map(|p| p.matrix.transform_point3(Vec3::ZERO))
        .collect();
    // group: x 10, scale 2. a: +1 in y (scaled to 2); b: at group; c: +5 in z (scaled to 10).
    assert_eq!(origins[0], Vec3::new(10.0, 2.0, 0.0));
    assert_eq!(origins[1], Vec3::new(10.0, 0.0, 0.0));
    assert_eq!(origins[2], Vec3::new(10.0, 0.0, 10.0));
    let (low, high) = model.bounds().unwrap();
    assert_eq!(low, Vec3::new(10.0, 0.0, 0.0));
    assert_eq!(high, Vec3::new(12.0, 4.0, 10.0));
    assert_eq!(model.nodes[1].local, Mat4::from_translation(Vec3::Y));
}

#[test]
fn a_file_without_normals_gets_flat_ones() {
    let (binary, views, accessors) = quad();
    let primitives = json!([{ "attributes": { "POSITION": 0, "TEXCOORD_0": 2 }, "indices": 3 }]);
    let model = load_model_bytes(
        &document(&binary, views, accessors, primitives, json!({})),
        "flat.glb",
    )
    .unwrap();
    let primitive = &model.meshes[0].primitives[0];
    assert_eq!(
        primitive.positions.len(),
        6,
        "each triangle gets its own vertices"
    );
    assert_eq!(primitive.indices, [0, 1, 2, 3, 4, 5]);
    assert!(
        primitive
            .normals
            .iter()
            .all(|n| (*n - Vec3::Z).length() < 1e-6),
        "{:?}",
        primitive.normals
    );
    assert_eq!(primitive.uvs.len(), 6);
}

#[test]
fn strips_and_fans_become_triangles_with_the_same_winding() {
    let (binary, views, accessors) = quad();
    for (mode, expected) in [(5, vec![0, 1, 2, 2, 1, 3]), (6, vec![0, 1, 2, 0, 2, 3])] {
        let primitives = json!([{ "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "mode": mode }]);
        let model = load_model_bytes(
            &document(
                &binary,
                views.clone(),
                accessors.clone(),
                primitives,
                json!({}),
            ),
            "mode.glb",
        )
        .unwrap();
        assert_eq!(
            model.meshes[0].primitives[0].indices, expected,
            "mode {mode}"
        );
    }
}

#[test]
fn what_is_not_supported_is_reported_not_dropped() {
    let (binary, views, accessors) = quad();
    // A point primitive, a texture on the second UV set, and a required
    // extension we do not implement.
    let primitives = json!([
        { "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "indices": 3, "material": 0 },
        { "attributes": { "POSITION": 0 }, "mode": 0 }
    ]);
    let (mut binary2, mut views2) = (binary.clone(), views.clone());
    let image = png(1, 1, png::ColorType::Rgba, &[1, 2, 3, 4]);
    while !binary2.len().is_multiple_of(4) {
        binary2.push(0);
    }
    views2.push(json!({ "buffer": 0, "byteOffset": binary2.len(), "byteLength": image.len() }));
    binary2.extend_from_slice(&image);
    let image_view = views2.len() - 1;
    let bytes = document(
        &binary2,
        views2,
        accessors,
        primitives,
        json!({
            "images": [{ "bufferView": image_view, "mimeType": "image/png" }],
            "textures": [{ "source": 0 }],
            "materials": [{ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0, "texCoord": 1 } } }],
            "extensionsUsed": ["KHR_materials_clearcoat"]
        }),
    );
    let model = load_model_bytes(&bytes, "limits.glb").unwrap();
    let codes: Vec<&str> = model.warnings.iter().map(|w| w.code.as_ref()).collect();
    assert_eq!(
        codes,
        [
            "unsupported_extension",
            "unsupported_texcoord_set",
            "unsupported_primitive_mode"
        ],
        "{:?}",
        model.warnings
    );
    assert!(
        model
            .warnings
            .iter()
            .all(|w| !w.hint.is_empty() && w.location.file.as_deref() == Some("limits.glb"))
    );
    assert!(model.materials[0].base_color_texture.is_none());
    assert_eq!(
        model.meshes[0].primitives.len(),
        1,
        "the point primitive is skipped"
    );
}

#[test]
fn a_required_extension_we_do_not_read_is_an_error_with_a_hint() {
    let (binary, views, accessors) = quad();
    let primitives = json!([{ "attributes": { "POSITION": 0 } }]);
    let bytes = document(
        &binary,
        views,
        accessors,
        primitives,
        json!({
            "extensionsUsed": ["KHR_draco_mesh_compression"],
            "extensionsRequired": ["KHR_draco_mesh_compression"]
        }),
    );
    let problem = load_model_bytes(&bytes, "draco.glb").unwrap_err();
    assert_eq!(problem.code, "unsupported_extension", "{problem}");
    assert!(problem.hint.contains("Re-export"), "{}", problem.hint);
    assert_eq!(problem.location.file.as_deref(), Some("draco.glb"));
}

#[test]
fn emissive_strength_scales_the_emission() {
    let model = textured_quad(json!({
        "materials": [{ "emissiveFactor": [0.5, 0.5, 0.0], "extensions": { "KHR_materials_emissive_strength": { "emissiveStrength": 4.0 } } }],
        "extensionsUsed": ["KHR_materials_emissive_strength"]
    }));
    assert!(model.warnings.is_empty(), "{:?}", model.warnings);
    assert_eq!(model.materials[0].emissive, [2.0, 2.0, 0.0]);
}

#[test]
fn bad_files_give_positioned_diagnostics_with_hints() {
    let cases: [(&str, Vec<u8>, &str); 3] = [
        (
            "not glTF at all",
            b"hello, this is not a model".to_vec(),
            "gltf_parse",
        ),
        (
            "a mesh that points at a missing accessor",
            glb(
                &json!({
                    "asset": { "version": "2.0" },
                    "meshes": [{ "primitives": [{ "attributes": { "POSITION": 7 } }] }]
                }),
                &[],
            ),
            "gltf_invalid",
        ),
        (
            "an unreadable image",
            {
                let (mut binary, mut views, accessors) = quad();
                while !binary.len().is_multiple_of(4) {
                    binary.push(0);
                }
                views.push(json!({ "buffer": 0, "byteOffset": binary.len(), "byteLength": 8 }));
                binary.extend_from_slice(b"notapng!");
                let view = views.len() - 1;
                document(
                    &binary,
                    views,
                    accessors,
                    json!([{ "attributes": { "POSITION": 0 } }]),
                    json!({ "images": [{ "bufferView": view, "mimeType": "image/png" }], "textures": [{ "source": 0 }] }),
                )
            },
            "gltf_image",
        ),
    ];
    for (name, bytes, code) in cases {
        let problem = load_model_bytes(&bytes, "assets/broken.glb").unwrap_err();
        assert_eq!(problem.code, code, "{name}: {problem}");
        assert_eq!(
            problem.location.file.as_deref(),
            Some("assets/broken.glb"),
            "{name}"
        );
        assert!(!problem.hint.is_empty(), "{name}");
    }
}

#[test]
fn a_gltf_file_needs_its_buffers_next_to_it() {
    let folder = std::env::temp_dir().join(format!("wish-import-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("model.gltf");
    std::fs::write(
        &file,
        r#"{ "asset": { "version": "2.0" }, "buffers": [{ "uri": "missing.bin", "byteLength": 4 }] }"#,
    )
    .unwrap();
    let problem = load_model_file(&file, "assets/model.gltf").unwrap_err();
    assert_eq!(problem.code, "gltf_file_missing");
    assert!(problem.hint.contains(".glb"));

    // With the buffer there, the same file loads.
    std::fs::write(folder.join("missing.bin"), [0u8; 4]).unwrap();
    let model = load_model_file(&file, "assets/model.gltf").unwrap();
    assert!(model.meshes.is_empty());
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_cyclic_node_tree_cannot_loop_forever() {
    let (binary, views, accessors) = quad();
    let primitives =
        json!([{ "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "indices": 3 }]);
    let bytes = document(
        &binary,
        views,
        accessors,
        primitives,
        json!({
            "nodes": [{ "mesh": 0, "children": [1] }, { "mesh": 0, "children": [0] }],
            "scenes": [{ "nodes": [0] }]
        }),
    );
    // The glTF validator may refuse this file, or it may load; either way
    // walking it ends.
    if let Ok(model) = load_model_bytes(&bytes, "cycle.glb") {
        assert!(model.placed().len() <= 2 * (wish_import::MAX_NODE_DEPTH + 2));
    }
}
