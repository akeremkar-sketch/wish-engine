//! Importing models: glTF 2.0 files (`.gltf` with its buffers and images,
//! or a single `.glb`) into plain CPU data.
//!
//! The result, a [`Model`], knows nothing about the GPU: meshes with
//! positions, normals, texture coordinates and tangents; materials in the
//! glTF metallic-roughness model; decoded textures; and the node tree.
//! `wish_render` turns it into something to draw.
//!
//! Supported: triangle lists, strips and fans; a single set of texture
//! coordinates (`TEXCOORD_0`); base colour, metallic-roughness, normal,
//! occlusion and emissive textures; `KHR_materials_emissive_strength`.
//! Anything else a file asks for (other extensions, point and line
//! primitives, second texture-coordinate sets) is reported in
//! [`Model::warnings`], never dropped silently.
//!
//! The coordinate system is glTF's: right-handed, +Y up, -Z forward,
//! meters. Nothing is converted (ADR-021).

use std::collections::BTreeSet;
use std::path::Path;

use gltf::mesh::Mode;
use wish_core::math::{Mat4, Vec2, Vec3, Vec4};
use wish_core::{Diagnostic, Result};

/// The most nodes one model may nest (hostile files could nest forever).
pub const MAX_NODE_DEPTH: usize = 256;

/// The most pixels one decoded texture may have (16384 × 16384).
pub const MAX_TEXTURE_PIXELS: u64 = 1 << 28;

/// An imported model.
#[derive(Clone, Debug, Default)]
pub struct Model {
    /// The meshes, each a list of primitives.
    pub meshes: Vec<Mesh>,
    /// The materials.
    pub materials: Vec<Material>,
    /// The decoded textures, 8-bit RGBA.
    pub textures: Vec<Texture>,
    /// The nodes.
    pub nodes: Vec<Node>,
    /// The nodes of the default scene (or of the first scene) that have no
    /// parent, in file order.
    pub roots: Vec<usize>,
    /// Things the file asks for that were not imported.
    pub warnings: Vec<Diagnostic>,
}

/// A mesh: one or more primitives, each with its own material.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    /// The mesh's name in the file, if it has one.
    pub name: Option<String>,
    /// Its primitives.
    pub primitives: Vec<Primitive>,
}

/// A triangle list with per-vertex data. Every attribute array has one entry
/// per position.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Primitive {
    /// Vertex positions, meters.
    pub positions: Vec<Vec3>,
    /// Unit normals (generated flat when the file has none).
    pub normals: Vec<Vec3>,
    /// Texture coordinates (zero when the file has none).
    pub uvs: Vec<Vec2>,
    /// Tangents: xyz direction and w the bitangent's sign. Read from the
    /// file, or generated when the material has a normal texture and the file
    /// has no tangents; zero otherwise.
    pub tangents: Vec<Vec4>,
    /// Three indices per triangle, counter-clockwise from the front.
    pub indices: Vec<u32>,
    /// Index into [`Model::materials`]; `None` uses the default material.
    pub material: Option<usize>,
}

/// How a surface's alpha is used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlphaMode {
    /// Alpha is ignored.
    #[default]
    Opaque,
    /// Pixels with alpha below the cutoff are not drawn.
    Mask,
    /// Alpha blends with what is behind (not drawn blended yet).
    Blend,
}

/// A texture used by a material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureRef {
    /// Index into [`Model::textures`].
    pub texture: usize,
}

/// A material: the glTF metallic-roughness model.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    /// The material's name in the file, if it has one.
    pub name: Option<String>,
    /// Linear RGBA colour factor.
    pub base_color: [f32; 4],
    /// Metallic factor, 0 to 1.
    pub metallic: f32,
    /// Roughness factor, 0 to 1.
    pub roughness: f32,
    /// Linear RGB emission factor, already multiplied by
    /// `KHR_materials_emissive_strength` if present.
    pub emissive: [f32; 3],
    /// Base colour texture (sRGB, with alpha).
    pub base_color_texture: Option<TextureRef>,
    /// Metallic (blue) and roughness (green) texture (linear).
    pub metallic_roughness_texture: Option<TextureRef>,
    /// Tangent-space normal texture (linear).
    pub normal_texture: Option<TextureRef>,
    /// Scale of the normal texture's x and y.
    pub normal_scale: f32,
    /// Occlusion texture, red channel (linear).
    pub occlusion_texture: Option<TextureRef>,
    /// How much the occlusion texture applies, 0 to 1.
    pub occlusion_strength: f32,
    /// Emission texture (sRGB).
    pub emissive_texture: Option<TextureRef>,
    /// Whether back faces are drawn too.
    pub double_sided: bool,
    /// How alpha is used.
    pub alpha_mode: AlphaMode,
    /// The cutoff for [`AlphaMode::Mask`].
    pub alpha_cutoff: f32,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: None,
            base_color: [1.0; 4],
            metallic: 1.0,
            roughness: 1.0,
            emissive: [0.0; 3],
            base_color_texture: None,
            metallic_roughness_texture: None,
            normal_texture: None,
            normal_scale: 1.0,
            occlusion_texture: None,
            occlusion_strength: 1.0,
            emissive_texture: None,
            double_sided: false,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
        }
    }
}

/// A decoded texture, 8-bit RGBA, rows top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Texture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// A node of the scene tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    /// The node's name in the file, if it has one.
    pub name: Option<String>,
    /// Its placement relative to its parent.
    pub local: Mat4,
    /// The mesh it shows, if any.
    pub mesh: Option<usize>,
    /// Its children (indices into [`Model::nodes`]).
    pub children: Vec<usize>,
}

/// One primitive placed in the model's space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    /// Index into [`Model::meshes`].
    pub mesh: usize,
    /// Index into that mesh's primitives.
    pub primitive: usize,
    /// Primitive space to the model's space.
    pub matrix: Mat4,
}

impl Model {
    /// Every primitive of the scene with its matrix, parents before
    /// children, in file order. The walk is iterative and bounded, so a
    /// malformed node tree cannot loop or recurse without end.
    pub fn placed(&self) -> Vec<Placed> {
        let mut out = Vec::new();
        let mut stack: Vec<(usize, Mat4, usize)> = self
            .roots
            .iter()
            .rev()
            .map(|node| (*node, Mat4::IDENTITY, 0))
            .collect();
        while let Some((index, parent, depth)) = stack.pop() {
            let Some(node) = self.nodes.get(index) else {
                continue;
            };
            if depth > MAX_NODE_DEPTH {
                continue;
            }
            let matrix = parent * node.local;
            if let Some(mesh) = node
                .mesh
                .and_then(|mesh| self.meshes.get(mesh).map(|m| (mesh, m)))
            {
                for primitive in 0..mesh.1.primitives.len() {
                    out.push(Placed {
                        mesh: mesh.0,
                        primitive,
                        matrix,
                    });
                }
            }
            for child in node.children.iter().rev() {
                stack.push((*child, matrix, depth + 1));
            }
        }
        out
    }

    /// The model's bounding box in its own space, over every placed
    /// primitive: `(min, max)`. `None` for a model with no geometry.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut bounds: Option<(Vec3, Vec3)> = None;
        for placed in self.placed() {
            let Some(primitive) = self
                .meshes
                .get(placed.mesh)
                .and_then(|mesh| mesh.primitives.get(placed.primitive))
            else {
                continue;
            };
            for position in &primitive.positions {
                let point = placed.matrix.transform_point3(*position);
                bounds = Some(match bounds {
                    Some((low, high)) => (low.min(point), high.max(point)),
                    None => (point, point),
                });
            }
        }
        bounds
    }
}

fn import_failure(file: &str, error: &gltf::Error) -> Diagnostic {
    let (code, hint): (&'static str, &str) = match error {
        gltf::Error::Io(_) => (
            "gltf_file_missing",
            "A .gltf file refers to separate .bin and image files; keep them next to it, or use a single .glb file.",
        ),
        gltf::Error::Image(_) | gltf::Error::UnsupportedImageEncoding => (
            "gltf_image",
            "Textures must be PNG or JPEG. Re-export the model with its textures in one of those formats.",
        ),
        gltf::Error::Validation(problems)
            if problems.iter().any(|(_, problem)| {
                matches!(problem, gltf::json::validation::Error::Unsupported)
            }) =>
        {
            (
                "unsupported_extension",
                "The model needs a glTF extension this engine does not read (for example Draco mesh compression). Re-export it without that extension.",
            )
        }
        gltf::Error::Validation(_) => (
            "gltf_invalid",
            "The file breaks the glTF 2.0 rules. Open it in a glTF validator (https://github.khronos.org/glTF-Validator/) to see where, or re-export it from the modelling tool.",
        ),
        _ => (
            "gltf_parse",
            "The file is not valid glTF 2.0. Check that it is a .gltf (JSON) or .glb file and not damaged.",
        ),
    };
    Diagnostic::error(
        code,
        format!("Could not read the glTF file: {error}."),
        hint,
    )
    .at_file(file)
}

/// Reads a glTF or GLB file from disk. External buffers and images of a
/// `.gltf` file are read from the folder it is in. `name` is the path shown
/// in diagnostics.
pub fn load_model_file(path: &Path, name: &str) -> Result<Model> {
    let imported = guarded(name, || gltf::import(path))?;
    let (document, buffers, images) = imported.map_err(|error| import_failure(name, &error))?;
    convert(name, &document, &buffers, &images)
}

/// Reads a self-contained GLB (or a `.gltf` with embedded data) from memory.
pub fn load_model_bytes(bytes: &[u8], name: &str) -> Result<Model> {
    let imported = guarded(name, || gltf::import_slice(bytes))?;
    let (document, buffers, images) = imported.map_err(|error| import_failure(name, &error))?;
    convert(name, &document, &buffers, &images)
}

/// Runs a call into the gltf crate so that a panic inside it becomes a
/// diagnostic. The crate indexes some arrays with numbers read from the file
/// before checking them (an accessor index past the end panics in its
/// validation); a hostile or damaged model must never take the engine down.
fn guarded<T>(file: &str, call: impl FnOnce() -> T) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(call)).map_err(|_| {
        Diagnostic::error(
            "gltf_invalid",
            "The glTF reader failed on this file: it refers to data that does not exist.",
            "The file is damaged or malformed. Open it in a glTF validator (https://github.khronos.org/glTF-Validator/), or re-export the model.",
        )
        .at_file(file)
    })
}

fn warn(model: &mut Model, file: &str, code: &'static str, message: String, hint: &str) {
    model
        .warnings
        .push(Diagnostic::warning(code, message, hint).at_file(file));
}

fn convert(
    file: &str,
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    images: &[gltf::image::Data],
) -> Result<Model> {
    let mut model = Model::default();

    // Extensions the file *requires* and we do not read are an error (the
    // reader refuses them). Ones it merely *uses* have a fallback in the
    // file, so the model still draws, but not as its author saw it.
    let supported: BTreeSet<&str> = ["KHR_materials_emissive_strength"].into_iter().collect();
    for extension in document.extensions_used() {
        if !supported.contains(extension) {
            warn(
                &mut model,
                file,
                "unsupported_extension",
                format!(
                    "The file uses the extension '{extension}', which is not supported; the model is drawn without it."
                ),
                "The model may look different from how it was made. Re-export without that extension to see what will be drawn.",
            );
        }
    }

    for image in images {
        model.textures.push(decode_texture(file, image)?);
    }

    for material in document.materials() {
        let converted = convert_material(&mut model, file, &material, images.len());
        model.materials.push(converted);
    }

    for mesh in document.meshes() {
        let mut converted = Mesh {
            name: mesh.name().map(str::to_owned),
            primitives: Vec::new(),
        };
        for primitive in mesh.primitives() {
            if let Some(read) = convert_primitive(&mut model, file, &mesh, &primitive, buffers) {
                converted.primitives.push(read);
            }
        }
        model.meshes.push(converted);
    }

    for node in document.nodes() {
        model.nodes.push(Node {
            name: node.name().map(str::to_owned),
            local: Mat4::from_cols_array_2d(&node.transform().matrix()),
            mesh: node.mesh().map(|mesh| mesh.index()),
            children: node.children().map(|child| child.index()).collect(),
        });
    }
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next());
    if let Some(scene) = scene {
        model.roots = scene.nodes().map(|node| node.index()).collect();
    }
    Ok(model)
}

fn decode_texture(file: &str, image: &gltf::image::Data) -> Result<Texture> {
    use gltf::image::Format;
    let pixels = u64::from(image.width) * u64::from(image.height);
    if pixels > MAX_TEXTURE_PIXELS {
        return Err(Diagnostic::error(
            "gltf_texture_too_large",
            format!(
                "A texture is {}x{} pixels, more than the {MAX_TEXTURE_PIXELS}-pixel limit.",
                image.width, image.height
            ),
            "Scale the texture down in the modelling tool; 4096x4096 is plenty for most surfaces.",
        )
        .at_file(file));
    }
    let count = pixels as usize;
    let data = &image.pixels;
    let channels = match image.format {
        Format::R8 => 1,
        Format::R8G8 => 2,
        Format::R8G8B8 => 3,
        Format::R8G8B8A8 => 4,
        other => {
            return Err(Diagnostic::error(
                "gltf_image",
                format!("A texture has the pixel format {other:?}, which is not supported."),
                "Textures must be 8-bit PNG or JPEG. Re-export the texture as an 8-bit image.",
            )
            .at_file(file));
        }
    };
    if data.len() < count * channels {
        return Err(Diagnostic::error(
            "gltf_image",
            "A texture's pixel data is shorter than its size says.",
            "The image is damaged; re-export the model.",
        )
        .at_file(file));
    }
    let mut rgba = Vec::with_capacity(count * 4);
    for pixel in data.chunks_exact(channels).take(count) {
        match channels {
            1 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]),
            2 => rgba.extend_from_slice(&[pixel[0], pixel[1], 0, 255]),
            3 => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            _ => rgba.extend_from_slice(pixel),
        }
    }
    Ok(Texture {
        width: image.width,
        height: image.height,
        rgba,
    })
}

fn texture_ref(
    model: &mut Model,
    file: &str,
    what: &str,
    texture: &gltf::Texture<'_>,
    tex_coord: u32,
    textures: usize,
) -> Option<TextureRef> {
    if tex_coord != 0 {
        warn(
            model,
            file,
            "unsupported_texcoord_set",
            format!(
                "The {what} texture uses texture coordinate set {tex_coord}; only set 0 is supported."
            ),
            "Re-export with the texture on the first UV set (TEXCOORD_0). The texture is left out for now.",
        );
        return None;
    }
    let index = texture.source().index();
    (index < textures).then_some(TextureRef { texture: index })
}

fn convert_material(
    model: &mut Model,
    file: &str,
    material: &gltf::Material<'_>,
    textures: usize,
) -> Material {
    let pbr = material.pbr_metallic_roughness();
    let emissive_strength = material.emissive_strength().unwrap_or(1.0);
    let [r, g, b] = material.emissive_factor();
    let mut out = Material {
        name: material.name().map(str::to_owned),
        base_color: pbr.base_color_factor(),
        metallic: pbr.metallic_factor(),
        roughness: pbr.roughness_factor(),
        emissive: [
            r * emissive_strength,
            g * emissive_strength,
            b * emissive_strength,
        ],
        double_sided: material.double_sided(),
        alpha_mode: match material.alpha_mode() {
            gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
            gltf::material::AlphaMode::Mask => AlphaMode::Mask,
            gltf::material::AlphaMode::Blend => AlphaMode::Blend,
        },
        alpha_cutoff: material.alpha_cutoff().unwrap_or(0.5),
        ..Material::default()
    };
    if let Some(info) = pbr.base_color_texture() {
        out.base_color_texture = texture_ref(
            model,
            file,
            "base colour",
            &info.texture(),
            info.tex_coord(),
            textures,
        );
    }
    if let Some(info) = pbr.metallic_roughness_texture() {
        out.metallic_roughness_texture = texture_ref(
            model,
            file,
            "metallic-roughness",
            &info.texture(),
            info.tex_coord(),
            textures,
        );
    }
    if let Some(info) = material.normal_texture() {
        out.normal_scale = info.scale();
        out.normal_texture = texture_ref(
            model,
            file,
            "normal",
            &info.texture(),
            info.tex_coord(),
            textures,
        );
    }
    if let Some(info) = material.occlusion_texture() {
        out.occlusion_strength = info.strength();
        out.occlusion_texture = texture_ref(
            model,
            file,
            "occlusion",
            &info.texture(),
            info.tex_coord(),
            textures,
        );
    }
    if let Some(info) = material.emissive_texture() {
        out.emissive_texture = texture_ref(
            model,
            file,
            "emissive",
            &info.texture(),
            info.tex_coord(),
            textures,
        );
    }
    out
}

/// Triangle indices from a list of `mode` indices.
fn triangles(mode: Mode, indices: &[u32]) -> Option<Vec<u32>> {
    match mode {
        Mode::Triangles => Some(
            indices
                .as_chunks::<3>()
                .0
                .iter()
                .flatten()
                .copied()
                .collect(),
        ),
        Mode::TriangleStrip => {
            let mut out = Vec::new();
            for (index, window) in indices.windows(3).enumerate() {
                // Every second triangle is flipped to keep the winding.
                if index % 2 == 0 {
                    out.extend_from_slice(&[window[0], window[1], window[2]]);
                } else {
                    out.extend_from_slice(&[window[1], window[0], window[2]]);
                }
            }
            Some(out)
        }
        Mode::TriangleFan => {
            let (first, rest) = indices.split_first()?;
            Some(
                rest.windows(2)
                    .flat_map(|pair| [*first, pair[0], pair[1]])
                    .collect(),
            )
        }
        _ => None,
    }
}

fn convert_primitive(
    model: &mut Model,
    file: &str,
    mesh: &gltf::Mesh<'_>,
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
) -> Option<Primitive> {
    let label = mesh
        .name()
        .map_or_else(|| format!("mesh {}", mesh.index()), str::to_owned);
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| &data.0[..]));
    let Some(positions) = reader.read_positions() else {
        warn(
            model,
            file,
            "primitive_without_positions",
            format!("A primitive of '{label}' has no positions and was skipped."),
            "A primitive needs a POSITION attribute; re-export the model.",
        );
        return None;
    };
    let positions: Vec<Vec3> = positions.map(Vec3::from).collect();
    let indices: Vec<u32> = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect(),
        None => (0..u32::try_from(positions.len()).unwrap_or(u32::MAX)).collect(),
    };
    let Some(indices) = triangles(primitive.mode(), &indices) else {
        warn(
            model,
            file,
            "unsupported_primitive_mode",
            format!(
                "A primitive of '{label}' draws {:?}, which is not supported, and was skipped.",
                primitive.mode()
            ),
            "Only triangles are imported. Triangulate the model or export it as triangles.",
        );
        return None;
    };
    let normals: Option<Vec<Vec3>> = reader.read_normals().map(|n| n.map(Vec3::from).collect());
    let uvs: Vec<Vec2> = reader
        .read_tex_coords(0)
        .map(|coords| coords.into_f32().map(Vec2::from).collect())
        .unwrap_or_else(|| vec![Vec2::ZERO; positions.len()]);
    let file_tangents: Option<Vec<Vec4>> =
        reader.read_tangents().map(|t| t.map(Vec4::from).collect());
    let has_normal_texture = primitive.material().normal_texture().is_some();

    if indices
        .iter()
        .any(|index| *index as usize >= positions.len())
        || normals.as_ref().is_some_and(|n| n.len() != positions.len())
        || uvs.len() != positions.len()
    {
        warn(
            model,
            file,
            "primitive_inconsistent",
            format!(
                "A primitive of '{label}' has indices or attributes that do not match its vertex count and was skipped."
            ),
            "The file is damaged; re-export the model.",
        );
        return None;
    }

    let mut out = Primitive {
        positions,
        normals: normals.clone().unwrap_or_default(),
        uvs,
        tangents: Vec::new(),
        indices,
        material: primitive.material().index(),
    };
    if normals.is_none() {
        flat_normals(&mut out);
    }
    out.tangents = match file_tangents {
        Some(tangents) if tangents.len() == out.positions.len() => tangents,
        _ if has_normal_texture => generate_tangents(&out),
        _ => vec![Vec4::ZERO; out.positions.len()],
    };
    Some(out)
}

/// Gives every triangle its own vertices with the face normal, as glTF
/// asks for when a file has no normals.
fn flat_normals(primitive: &mut Primitive) {
    let mut positions = Vec::with_capacity(primitive.indices.len());
    let mut uvs = Vec::with_capacity(primitive.indices.len());
    let mut normals = Vec::with_capacity(primitive.indices.len());
    for triangle in primitive.indices.as_chunks::<3>().0 {
        let [a, b, c] = (*triangle).map(|index| index as usize);
        let (pa, pb, pc) = (
            primitive.positions[a],
            primitive.positions[b],
            primitive.positions[c],
        );
        let normal = (pb - pa).cross(pc - pa).try_normalize().unwrap_or(Vec3::Y);
        positions.extend([pa, pb, pc]);
        uvs.extend([primitive.uvs[a], primitive.uvs[b], primitive.uvs[c]]);
        normals.extend([normal; 3]);
    }
    primitive.indices = (0..u32::try_from(positions.len()).unwrap_or(u32::MAX)).collect();
    primitive.positions = positions;
    primitive.uvs = uvs;
    primitive.normals = normals;
}

/// Per-vertex tangents from the texture coordinates: each triangle's tangent
/// is added to its vertices, then made perpendicular to the normal. The sign
/// in `w` says which way the bitangent points: the bitangent is `cross(normal,
/// tangent) * w`, and it must point where the normal map's green channel does,
/// up in the image (towards smaller V). For an ordinary, non-mirrored layout`n/// `w` is +1. (Not MikkTSpace, but the same for such layouts.)
fn generate_tangents(primitive: &Primitive) -> Vec<Vec4> {
    let count = primitive.positions.len();
    let mut tangent = vec![Vec3::ZERO; count];
    let mut bitangent = vec![Vec3::ZERO; count];
    for triangle in primitive.indices.as_chunks::<3>().0 {
        let [a, b, c] = (*triangle).map(|index| index as usize);
        let edge1 = primitive.positions[b] - primitive.positions[a];
        let edge2 = primitive.positions[c] - primitive.positions[a];
        let d1 = primitive.uvs[b] - primitive.uvs[a];
        let d2 = primitive.uvs[c] - primitive.uvs[a];
        let determinant = d1.x * d2.y - d2.x * d1.y;
        if determinant.abs() < 1e-12 {
            continue;
        }
        let scale = 1.0 / determinant;
        let t = (edge1 * d2.y - edge2 * d1.y) * scale;
        // The bitangent is the direction the normal map's green channel
        // points: up in the image, which is towards *smaller* V (glTF's UV
        // origin is the top left).
        let bt = -(edge2 * d1.x - edge1 * d2.x) * scale;
        for vertex in [a, b, c] {
            tangent[vertex] += t;
            bitangent[vertex] += bt;
        }
    }
    (0..count)
        .map(|vertex| {
            let normal = primitive.normals[vertex];
            let orthogonal = tangent[vertex] - normal * normal.dot(tangent[vertex]);
            let Some(direction) = orthogonal.try_normalize() else {
                // No usable UVs here: any direction perpendicular to the normal.
                let fallback = if normal.x.abs() < 0.9 {
                    Vec3::X
                } else {
                    Vec3::Y
                };
                let direction = (fallback - normal * normal.dot(fallback)).normalize_or_zero();
                return direction.extend(1.0);
            };
            let sign = if normal.cross(direction).dot(bitangent[vertex]) < 0.0 {
                -1.0
            } else {
                1.0
            };
            direction.extend(sign)
        })
        .collect()
}
