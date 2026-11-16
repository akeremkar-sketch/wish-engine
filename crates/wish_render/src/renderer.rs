//! Drawing a [`RenderScene`] into an image.
//!
//! A frame is three passes, in order:
//!
//! 1. **Sky:** fills the HDR target with the sky gradient.
//! 2. **Opaque:** draws every surface with the glTF metallic-roughness BRDF,
//!    one directional light, ambient light and the sky's reflection, with a
//!    depth buffer.
//! 3. **Tone mapping:** Khronos PBR Neutral, from the HDR target to the 8-bit
//!    sRGB image.
//!
//! Values are pre-exposed: the sky and opaque passes multiply by the
//! exposure before writing, so the HDR target holds displayable magnitudes.
//!
//! The order is fixed for now; shadows (WP5) and the debug outputs (WP6)
//! turn it into a small graph of passes with declared inputs and outputs.

use std::collections::BTreeMap;

use wish_core::math::{Mat4, Vec3};
use wish_core::{Diagnostic, Entity, Result};

use crate::capture::{CAPTURE_FORMAT, Target};
use crate::components::{EMISSIVE_NITS, Shape};
use crate::extract::{CameraView, MaterialData, Mesh, RenderScene};
use crate::gpu::Gpu;
use crate::image::RgbaImage;
use crate::mesh::{MeshData, VERTEX_STRIDE, primitive};
use crate::shader::ShaderLibrary;
use crate::texture;

mod debug;
pub use debug::{Description, IdBuffer, MAX_VISIBILITY_MEASUREMENTS, View, VisibleEntity};

const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Bytes of the frame uniform: four matrices and eleven vectors.
const FRAME_UNIFORM_SIZE: u64 = 432;
/// Bytes of one point or spot light in the light buffer: four vectors.
const LIGHT_STRIDE: usize = 64;
/// Side of the shadow map, in texels.
pub const SHADOW_MAP_SIZE: u32 = 2048;
const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Bytes of one draw's uniform data: two matrices and four vectors.
const DRAW_UNIFORM_SIZE: u64 = 192;
/// Distance between draws in the draw uniform buffer. 256 is a multiple of
/// every adapter's uniform offset alignment.
const DRAW_UNIFORM_STRIDE: u64 = 256;

/// The engine's own shader modules: (name, file, source).
const SHADERS: &[(&str, &str, &str)] = &[
    (
        "common",
        "wish_render/shaders/common.wgsl",
        include_str!("shaders/common.wgsl"),
    ),
    (
        "sky",
        "wish_render/shaders/sky.wgsl",
        include_str!("shaders/sky.wgsl"),
    ),
    (
        "brdf",
        "wish_render/shaders/brdf.wgsl",
        include_str!("shaders/brdf.wgsl"),
    ),
    (
        "tonemap",
        "wish_render/shaders/tonemap.wgsl",
        include_str!("shaders/tonemap.wgsl"),
    ),
    (
        "draw",
        "wish_render/shaders/draw.wgsl",
        include_str!("shaders/draw.wgsl"),
    ),
    (
        "shadow_pass",
        "wish_render/shaders/shadow_pass.wgsl",
        include_str!("shaders/shadow_pass.wgsl"),
    ),
    (
        "sky_pass",
        "wish_render/shaders/sky_pass.wgsl",
        include_str!("shaders/sky_pass.wgsl"),
    ),
    (
        "lights",
        "wish_render/shaders/lights.wgsl",
        include_str!("shaders/lights.wgsl"),
    ),
    (
        "opaque",
        "wish_render/shaders/opaque.wgsl",
        include_str!("shaders/opaque.wgsl"),
    ),
    (
        "tonemap_pass",
        "wish_render/shaders/tonemap_pass.wgsl",
        include_str!("shaders/tonemap_pass.wgsl"),
    ),
    (
        "debug_pass",
        "wish_render/shaders/debug_pass.wgsl",
        include_str!("shaders/debug_pass.wgsl"),
    ),
    (
        "gizmo_pass",
        "wish_render/shaders/gizmo_pass.wgsl",
        include_str!("shaders/gizmo_pass.wgsl"),
    ),
    (
        "ui_pass",
        "wish_render/shaders/ui_pass.wgsl",
        include_str!("shaders/ui_pass.wgsl"),
    ),
    (
        "sprite_pass",
        "wish_render/shaders/sprite_pass.wgsl",
        include_str!("shaders/sprite_pass.wgsl"),
    ),
];

/// The library of the engine's shaders.
pub fn engine_shaders() -> ShaderLibrary {
    let mut library = ShaderLibrary::new();
    for (name, file, source) in SHADERS {
        library.add(name, file, source);
    }
    library
}

/// The shader modules that make up a pipeline (the others are imported by
/// them).
pub const ENTRY_SHADERS: &[&str] = &[
    "sky_pass",
    "opaque",
    "tonemap_pass",
    "shadow_pass",
    "debug_pass",
    "gizmo_pass",
    "ui_pass",
    "sprite_pass",
];

/// Checks every pipeline shader of `library` and returns every problem found
/// (one per shader at most), without needing a GPU.
pub fn check_shaders(library: &ShaderLibrary) -> Vec<Diagnostic> {
    ENTRY_SHADERS
        .iter()
        .filter_map(|entry| library.check(entry).err())
        .collect()
}

/// The engine's shaders, with any `<name>.wgsl` found in `dir` taking the
/// place of the built-in module of that name (for developing shaders: edit
/// the file, then [`Renderer::reload_shaders`]). Returns the library and the
/// names that were replaced. A file that cannot be read is an error.
pub fn engine_shaders_from_dir(dir: &std::path::Path) -> Result<(ShaderLibrary, Vec<String>)> {
    let mut library = ShaderLibrary::new();
    let mut replaced = Vec::new();
    for (name, file, source) in SHADERS {
        let path = dir.join(format!("{name}.wgsl"));
        if path.is_file() {
            let shown = path.display().to_string();
            let text = std::fs::read_to_string(&path).map_err(|error| {
                Diagnostic::error(
                    "file_unreadable",
                    format!("Could not read the shader '{shown}': {error}."),
                    "Check that the file can be read and is UTF-8 text.",
                )
                .at_file(shown.clone())
            })?;
            library.add(name, &shown, &text);
            replaced.push((*name).to_owned());
        } else {
            library.add(name, file, source);
        }
    }
    Ok((library, replaced))
}

/// The exposure multiplier for an exposure value at ISO 100 (the standard
/// "saturation-based" relation: 1 / (1.2 · 2^EV100)).
pub fn exposure_multiplier(ev100: f32) -> f32 {
    1.0 / (1.2 * 2f32.powf(ev100))
}

/// Little-endian `f32` writer for uniform data.
#[derive(Default)]
struct Bytes(Vec<u8>);

impl Bytes {
    fn floats(&mut self, values: &[f32]) {
        for value in values {
            self.0.extend_from_slice(&value.to_le_bytes());
        }
    }
    fn mat4(&mut self, matrix: &Mat4) {
        self.floats(&matrix.to_cols_array());
    }
    fn vec4(&mut self, rgb: [f32; 3], w: f32) {
        self.floats(&[rgb[0], rgb[1], rgb[2], w]);
    }
    fn pad_to(&mut self, size: u64) {
        self.0.resize(size as usize, 0);
    }
}

#[derive(Clone)]
struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    /// The mesh's distinct edges as vertex index pairs, for the wireframe view.
    edges: wgpu::Buffer,
    edge_index_count: u32,
}

/// A buffer filled through the queue. (wgpu's `create_buffer_init` maps the
/// buffer at creation and panics if that mapping fails; filling through the
/// queue cannot panic, and a failure surfaces in the error scope instead.)
/// `contents` must be a multiple of 4 bytes long, which all engine data is.
pub(crate) fn buffer_with(
    gpu: &Gpu,
    label: &str,
    contents: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    let size = (contents.len() as u64).max(wgpu::COPY_BUFFER_ALIGNMENT);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if !contents.is_empty() {
        gpu.queue.write_buffer(&buffer, 0, contents);
    }
    buffer
}

impl GpuMesh {
    fn new(gpu: &Gpu, mesh: &MeshData, label: &str) -> Self {
        let edges = crate::mesh::edge_indices(&mesh.indices);
        Self {
            vertices: buffer_with(gpu, label, &mesh.vertex_bytes(), wgpu::BufferUsages::VERTEX),
            indices: buffer_with(gpu, label, &mesh.index_bytes(), wgpu::BufferUsages::INDEX),
            index_count: u32::try_from(mesh.indices.len()).unwrap_or(u32::MAX),
            edges: buffer_with(
                gpu,
                label,
                &edges
                    .iter()
                    .flat_map(|index| index.to_le_bytes())
                    .collect::<Vec<u8>>(),
                wgpu::BufferUsages::INDEX,
            ),
            edge_index_count: u32::try_from(edges.len()).unwrap_or(u32::MAX),
        }
    }
}

/// A light's view of the scene.
#[derive(Clone, Copy)]
struct LightFit {
    /// World space to the light's clip space.
    view_proj: Mat4,
    /// Depth bias in clip-space depth.
    depth_bias: f32,
    /// How far to push a lookup off the surface, along its normal, meters.
    normal_offset: f32,
}

/// Fits an orthographic view along direction around all casters (a
/// matrix and the corners of a local bounding box each), so that the shadow
/// map covers exactly what is drawn. None when there is nothing to cover or
/// the numbers are not finite.
fn fit_light(direction: Vec3, casters: &[(Mat4, Vec3, Vec3, u32)]) -> Option<LightFit> {
    let up = if direction.y.abs() > 0.99 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let view = wish_core::math::camera::rh::view::look_to_mat4(Vec3::ZERO, direction, up);
    let mut low = Vec3::splat(f32::MAX);
    let mut high = Vec3::splat(f32::MIN);
    for (matrix, min, max, _) in casters {
        for corner in 0..8 {
            let point = Vec3::new(
                if corner & 1 == 0 { min.x } else { max.x },
                if corner & 2 == 0 { min.y } else { max.y },
                if corner & 4 == 0 { min.z } else { max.z },
            );
            let in_light = view.transform_point3(matrix.transform_point3(point));
            low = low.min(in_light);
            high = high.max(in_light);
        }
    }
    if casters.is_empty() || !low.is_finite() || !high.is_finite() {
        return None;
    }
    // A margin of a few texels so that edges are not clipped by filtering.
    let width = (high.x - low.x).max(high.y - low.y).max(1e-3);
    let texel = width / SHADOW_MAP_SIZE as f32;
    let margin = texel * 4.0;
    // In this view the light looks down -Z: distances grow as z falls.
    let near = -high.z - margin;
    let far = -low.z + margin;
    let projection = wish_core::math::camera::rh::proj::directx::orthographic(
        low.x - margin,
        high.x + margin,
        low.y - margin,
        high.y + margin,
        near,
        far,
    );
    let depth_range = (far - near).max(1e-3);
    Some(LightFit {
        view_proj: projection * view,
        depth_bias: texel * 2.0 / depth_range,
        normal_offset: texel * 1.5,
    })
}

/// The bounding box of some points; a point at the origin if there are none.
fn bounds_of(points: &[Vec3]) -> (Vec3, Vec3) {
    let mut iter = points.iter().copied();
    let Some(first) = iter.next() else {
        return (Vec3::ZERO, Vec3::ZERO);
    };
    iter.fold((first, first), |(low, high), point| {
        (low.min(point), high.max(point))
    })
}

/// What one draw call draws: a built-in shape (with the entity's own
/// Material), or one primitive of an uploaded model (with the material from
/// its file).
enum Source<'a> {
    Shape(usize),
    Primitive(&'a GpuPrimitive),
}

/// The draw calls of a frame and their uniform data.
#[derive(Default)]
struct DrawList<'a> {
    bytes: Bytes,
    draws: Vec<(Source<'a>, u64)>,
    /// For each draw: its matrix and the corners of its local bounding box,
    /// to fit the shadow map around everything that can cast a shadow.
    casters: Vec<(Mat4, Vec3, Vec3, u32)>,
}

/// How a draw's surface is lit: the numbers the material side of the shader
/// reads.
#[derive(Clone, Copy)]
struct Surface<'a> {
    material: &'a MaterialData,
    normal_scale: f32,
    occlusion_strength: f32,
    /// 0 unless alpha masks.
    alpha_cutoff: f32,
    /// The scene draw this belongs to, plus one (0 means nothing); what the
    /// identity view writes.
    id: u32,
}

impl<'a> DrawList<'a> {
    fn push(
        &mut self,
        source: Source<'a>,
        world: &Mat4,
        bounds: (Vec3, Vec3),
        surface: Surface<'_>,
    ) {
        let Surface {
            material,
            normal_scale,
            occlusion_strength,
            alpha_cutoff,
            id,
        } = surface;
        self.casters.push((*world, bounds.0, bounds.1, id));
        let start = self.draws.len() as u64 * DRAW_UNIFORM_STRIDE;
        self.bytes.pad_to(start);
        self.bytes.mat4(world);
        self.bytes.mat4(&world.inverse().transpose());
        self.bytes.floats(&material.base_color);
        self.bytes
            .floats(&[material.metallic, material.roughness, 0.0, 0.0]);
        self.bytes.vec4(material.emissive, 0.0);
        self.bytes
            .floats(&[normal_scale, occlusion_strength, alpha_cutoff, id as f32]);
        self.draws.push((source, start));
    }
}

/// One primitive of an uploaded model.
#[derive(Clone)]
struct GpuPrimitive {
    mesh: GpuMesh,
    /// The material's five textures and the sampler.
    textures: wgpu::BindGroup,
    material: MaterialData,
    normal_scale: f32,
    occlusion_strength: f32,
    /// 0 unless the material masks by alpha.
    alpha_cutoff: f32,
    double_sided: bool,
    /// Primitive space to the model's space.
    matrix: Mat4,
    /// The primitive's bounding box in its own space.
    bounds: (Vec3, Vec3),
    /// Where it is in the model file (for posing animated models).
    placed: wish_import::Placed,
}

/// Posed primitives of animated draws, by draw index.
type Posed = BTreeMap<usize, Vec<GpuPrimitive>>;

/// A model on the GPU.
struct GpuModel {
    primitives: Vec<GpuPrimitive>,
    /// The model's CPU data, kept when it has skins or clips: posing needs
    /// the node tree, the keyframes and the bind-pose vertices.
    animated: Option<std::sync::Arc<wish_import::Model>>,
    /// How many of its materials blend alpha, which is not drawn blended yet.
    blended: usize,
}

/// What a frame produced.
#[derive(Debug)]
pub struct Frame {
    /// The image.
    pub image: RgbaImage,
    /// The camera it was drawn from.
    pub camera: Entity,
    /// How many draws made it into the image.
    pub drawn: usize,
    /// Things that were not drawn, and why.
    pub warnings: Vec<Diagnostic>,
}

/// What `collect` produced: the draw calls and what could not be drawn.
struct Collected<'a> {
    draw_list: DrawList<'a>,
    warnings: Vec<Diagnostic>,
}

/// The camera to draw from: the one asked for, or the scene's first.
fn choose_camera(scene: &RenderScene, camera: Option<Entity>) -> Result<&CameraView> {
    match camera {
        Some(entity) => scene
            .cameras
            .iter()
            .find(|view| view.entity == entity)
            .ok_or_else(|| {
                Diagnostic::error(
                    "not_a_camera",
                    format!("Entity {entity} has no Camera component."),
                    "Pick an entity that has a Camera, or leave the camera out to use the scene's first one.",
                )
            }),
        None => scene.cameras.first().ok_or_else(|| {
            Diagnostic::error(
                "no_camera",
                "The scene has no camera to draw from.",
                "Add an entity with a \"Camera\" component and a \"Transform\" that places it; it looks along its -Z axis.",
            )
        }),
    }
}
impl Renderer {
    /// Turns the scene's draws into draw calls. With `only`, just the draw
    /// with that index in `scene.draws` (to measure one entity on its own).
    /// The primitives of every animated model draw of `scene`, posed at its
    /// clip's time (skinned vertices bent on the CPU), by draw index. Draws
    /// of models without skins or clips are not here.
    fn pose(&self, gpu: &Gpu, scene: &RenderScene) -> Result<Posed> {
        let mut posed = Posed::new();
        for (index, draw) in scene.draws.iter().enumerate() {
            let Some((clip, time)) = &draw.animation else {
                continue;
            };
            let file = draw
                .model_path
                .as_deref()
                .unwrap_or_default()
                .split('#')
                .next()
                .unwrap_or_default();
            let Some(model) = self.models.get(file) else {
                continue;
            };
            let Some(source) = &model.animated else {
                continue;
            };
            let pose = source.pose(source.animation_index(clip), *time);
            let mut primitives = Vec::with_capacity(model.primitives.len());
            for primitive in &model.primitives {
                let mut moved = primitive.clone();
                let skinned = source.skin(&primitive.placed, &pose);
                match skinned {
                    Some((positions, normals)) => {
                        let Some(original) = source
                            .meshes
                            .get(primitive.placed.mesh)
                            .and_then(|m| m.primitives.get(primitive.placed.primitive))
                        else {
                            continue;
                        };
                        let mut data = MeshData::from(original);
                        moved.bounds = bounds_of(&positions);
                        data.positions = positions;
                        data.normals = normals;
                        moved.mesh = gpu
                            .checked("upload a posed mesh", |gpu| GpuMesh::new(gpu, &data, file))?;
                        moved.matrix = Mat4::IDENTITY;
                    }
                    None => {
                        moved.matrix = pose
                            .get(primitive.placed.node)
                            .copied()
                            .unwrap_or(primitive.matrix);
                    }
                }
                primitives.push(moved);
            }
            posed.insert(index, primitives);
        }
        Ok(posed)
    }

    fn collect<'a>(
        &'a self,
        scene: &RenderScene,
        only: Option<usize>,
        posed: &'a Posed,
    ) -> Collected<'a> {
        // What one draw call draws: a built-in shape (with the entity's own
        // `Material`), or one primitive of an uploaded model (with the
        // material from its file).
        let mut warnings = Vec::new();
        let mut draw_list = DrawList::default();
        let mut blended_models: Vec<&str> = Vec::new();
        for (index, draw) in scene.draws.iter().enumerate() {
            if only.is_some_and(|only| only != index) {
                continue;
            }
            let id = u32::try_from(index + 1).unwrap_or(u32::MAX);
            if draw.world.determinant().abs() < 1e-12 || !draw.world.is_finite() {
                warnings.push(Diagnostic::warning(
                    "degenerate_transform",
                    format!(
                        "Entity {} was not drawn: its scale is zero or not finite.",
                        draw.entity
                    ),
                    "Give its Transform (and its parents') a non-zero, finite scale.",
                ));
                continue;
            }
            match draw.mesh {
                Mesh::Primitive(shape) => draw_list.push(
                    Source::Shape(shape_index(shape)),
                    &draw.world,
                    (Vec3::splat(-0.5), Vec3::splat(0.5)),
                    Surface {
                        material: &draw.material,
                        normal_scale: 1.0,
                        occlusion_strength: 1.0,
                        alpha_cutoff: 0.0,
                        id,
                    },
                ),
                Mesh::Model => {
                    let path = draw.model_path.as_deref().unwrap_or_default();
                    // A `#sub` part names something inside the file; the
                    // whole model is drawn for now.
                    let file = path.split('#').next().unwrap_or(path);
                    let Some((key, model)) = self.models.get_key_value(file) else {
                        warnings.push(Diagnostic::warning(
                            "model_not_loaded",
                            format!(
                                "Entity {} was not drawn: the model '{file}' has not been loaded.",
                                draw.entity
                            ),
                            "Load the model before drawing (wen capture does it from the scene's assets); check the path in the Model component.",
                        )
                        .at_file(file));
                        continue;
                    };
                    if model.blended > 0 && !blended_models.contains(&key.as_str()) {
                        blended_models.push(key.as_str());
                    }
                    let primitives = posed.get(&index).unwrap_or(&model.primitives);
                    for primitive in primitives {
                        draw_list.push(
                            Source::Primitive(primitive),
                            &(draw.world * primitive.matrix),
                            primitive.bounds,
                            Surface {
                                material: &primitive.material,
                                normal_scale: primitive.normal_scale,
                                occlusion_strength: primitive.occlusion_strength,
                                alpha_cutoff: primitive.alpha_cutoff,
                                id,
                            },
                        );
                    }
                }
            }
        }
        for key in blended_models {
            warnings.push(Diagnostic::warning(
                "alpha_blend_not_blended",
                format!("The model '{key}' has materials that blend alpha; they are drawn opaque."),
                "Blended transparency arrives later (M6). Use alpha mode MASK in the material for cut-outs such as leaves.",
            ));
        }

        Collected {
            draw_list,
            warnings,
        }
    }
}
/// Everything built when a renderer is created, before it is taken apart.
struct Pipelines {
    frame_layout: wgpu::BindGroupLayout,
    draw_layout: wgpu::BindGroupLayout,
    hdr_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    light_layout: wgpu::BindGroupLayout,
    sky: wgpu::RenderPipeline,
    opaque: wgpu::RenderPipeline,
    opaque_double: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    tonemap: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    shadow_sampler: wgpu::Sampler,
}

/// Draws scenes. Holds the pipelines and the built-in meshes, so create one
/// and reuse it for many frames.
pub struct Renderer {
    frame_layout: wgpu::BindGroupLayout,
    draw_layout: wgpu::BindGroupLayout,
    hdr_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    sky: wgpu::RenderPipeline,
    opaque: wgpu::RenderPipeline,
    /// The same, without back-face culling, for double-sided materials.
    opaque_double: wgpu::RenderPipeline,
    tonemap: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    light_layout: wgpu::BindGroupLayout,
    /// The shadow pass: depth only, from the light.
    shadow: wgpu::RenderPipeline,
    shadow_sampler: wgpu::Sampler,
    /// Textures of a material that has none: white, flat normal.
    default_textures: wgpu::BindGroup,
    /// Cube, sphere, plane, in `shape_index` order.
    shapes: Vec<GpuMesh>,
    /// Uploaded models, by project path.
    models: BTreeMap<String, GpuModel>,
    /// The pipelines of the debug views.
    debug: debug::DebugPipelines,
    /// The in-game interface's pipeline.
    ui: crate::ui::UiPipeline,
    /// The sprites' and particles' pipeline.
    overlay: crate::sprites::OverlayPipeline,
    /// Pictures of the interface and of sprites, by project path.
    ui_pictures: BTreeMap<String, crate::ui::UiPicture>,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer").finish_non_exhaustive()
    }
}

fn shape_index(shape: Shape) -> usize {
    match shape {
        Shape::Cube => 0,
        Shape::Sphere => 1,
        Shape::Plane => 2,
    }
}

/// A material's five textures (base colour, metallic-roughness, normal,
/// occlusion, emissive) and the sampler as one bind group.
fn material_group(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    textures: [&wgpu::TextureView; 5],
) -> wgpu::BindGroup {
    let mut entries: Vec<wgpu::BindGroupEntry<'_>> = textures
        .iter()
        .enumerate()
        .map(|(binding, view)| wgpu::BindGroupEntry {
            binding: u32::try_from(binding).unwrap_or(0),
            resource: wgpu::BindingResource::TextureView(view),
        })
        .collect();
    entries.push(wgpu::BindGroupEntry {
        binding: 5,
        resource: wgpu::BindingResource::Sampler(sampler),
    });
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("material"),
        layout,
        entries: &entries,
    })
}

/// A read-only storage buffer for fragment shaders (the light lists).
fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// The point and spot lights as the shader reads them (`LocalLight`).
fn light_bytes(lights: &[crate::extract::LocalLightView]) -> Vec<u8> {
    let mut bytes = Bytes::default();
    for light in lights.iter().take(crate::cluster::MAX_LIGHTS) {
        bytes.vec4(light.position.to_array(), light.range);
        let (spot, direction, outer, inner) = match light.spot {
            Some((direction, inner, outer)) => (1.0, direction.to_array(), outer, inner),
            None => (0.0, [0.0, -1.0, 0.0], -1.0, -1.0),
        };
        bytes.vec4(light.intensity, spot);
        bytes.vec4(direction, outer);
        bytes.vec4([inner, 0.0, 0.0], 0.0);
    }
    if bytes.0.is_empty() {
        bytes.pad_to(LIGHT_STRIDE as u64);
    }
    bytes.0
}

fn uniform_entry(binding: u32, dynamic: bool, size: u64) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: wgpu::BufferSize::new(size),
        },
        count: None,
    }
}

impl Renderer {
    /// Checks the engine's shaders and builds the pipelines. A shader mistake
    /// comes back as a positioned diagnostic in the shader's own file.
    pub fn new(gpu: &Gpu) -> Result<Self> {
        Self::with_shaders(gpu, &engine_shaders())
    }

    /// Builds the pipelines from `library` instead of the built-in shaders
    /// (see [`engine_shaders_from_dir`]).
    pub fn with_shaders(gpu: &Gpu, library: &ShaderLibrary) -> Result<Self> {
        let mut modules = Vec::new();
        for entry in ENTRY_SHADERS.iter().copied() {
            modules.push((entry, library.check(entry)?.source));
        }
        let debug_source = modules
            .iter()
            .find(|(entry, _)| *entry == "debug_pass")
            .map(|(_, source)| source.clone())
            .unwrap_or_default();
        let gizmo_source = modules
            .iter()
            .find(|(entry, _)| *entry == "gizmo_pass")
            .map(|(_, source)| source.clone())
            .unwrap_or_default();
        let ui_source = modules
            .iter()
            .find(|(entry, _)| *entry == "ui_pass")
            .map(|(_, source)| source.clone())
            .unwrap_or_default();
        let sprite_source = modules
            .iter()
            .find(|(entry, _)| *entry == "sprite_pass")
            .map(|(_, source)| source.clone())
            .unwrap_or_default();
        let pipelines = gpu.checked("build the render pipelines", |gpu| {
            let device = &gpu.device;
            let module = |name: &str| {
                let source = modules
                    .iter()
                    .find(|(entry, _)| *entry == name)
                    .map(|(_, source)| source.clone())
                    .unwrap_or_default();
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(name),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                })
            };
            let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("frame"),
                entries: &[
                    uniform_entry(0, false, FRAME_UNIFORM_SIZE),
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                        count: None,
                    },
                    storage_entry(3),
                    storage_entry(4),
                    storage_entry(5),
                ],
            });
            let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("light"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(64),
                    },
                    count: None,
                }],
            });
            let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("draw"),
                entries: &[uniform_entry(0, true, DRAW_UNIFORM_SIZE)],
            });
            let hdr_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("hdr"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
            });
            let texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            };
            let material_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("material"),
                    entries: &[
                        texture_entry(0),
                        texture_entry(1),
                        texture_entry(2),
                        texture_entry(3),
                        texture_entry(4),
                        wgpu::BindGroupLayoutEntry {
                            binding: 5,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                    ],
                });
            let layout = |layouts: &[&wgpu::BindGroupLayout]| {
                let layouts: Vec<Option<&wgpu::BindGroupLayout>> =
                    layouts.iter().map(|layout| Some(*layout)).collect();
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &layouts,
                    immediate_size: 0,
                })
            };
            let fullscreen = |name: &str,
                              pipeline_layout: &wgpu::PipelineLayout,
                              format: wgpu::TextureFormat| {
                let shader = module(name);
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(name),
                    layout: Some(pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
            let sky = fullscreen("sky_pass", &layout(&[&frame_layout]), HDR_FORMAT);
            let tonemap = fullscreen("tonemap_pass", &layout(&[&hdr_layout]), CAPTURE_FORMAT);

            let opaque_shader = module("opaque");
            let attributes = [
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 12,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 24,
                    shader_location: 2,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 32,
                    shader_location: 3,
                },
            ];
            let opaque_layout = layout(&[&frame_layout, &draw_layout, &material_layout]);
            let opaque_pipeline = |name: &str, cull_mode: Option<wgpu::Face>| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(name),
                    layout: Some(&opaque_layout),
                    vertex: wgpu::VertexState {
                        module: &opaque_shader,
                        entry_point: Some("vs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: VERTEX_STRIDE,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &attributes,
                        })],
                    },
                    primitive: wgpu::PrimitiveState {
                        cull_mode,
                        ..wgpu::PrimitiveState::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::Less),
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &opaque_shader,
                        entry_point: Some("fs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: HDR_FORMAT,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
            let opaque = opaque_pipeline("opaque", Some(wgpu::Face::Back));
            let opaque_double = opaque_pipeline("opaque double-sided", None);
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("material"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                address_mode_w: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..wgpu::SamplerDescriptor::default()
            });
            // The shadow pass: depth only, from the light. Back faces are not
            // culled so that single-sided surfaces such as planes cast too; a
            // slope-scaled bias keeps surfaces from shadowing themselves.
            let shadow_shader = module("shadow_pass");
            let shadow = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shadow"),
                layout: Some(&layout(&[&light_layout, &draw_layout])),
                vertex: wgpu::VertexState {
                    module: &shadow_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: VERTEX_STRIDE,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes[..1],
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: SHADOW_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState {
                        constant: 2,
                        slope_scale: 2.0,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: None,
                multiview_mask: None,
                cache: None,
            });
            // Nearest, so that every adapter compares the same texels.
            let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("shadow"),
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                compare: Some(wgpu::CompareFunction::LessEqual),
                ..wgpu::SamplerDescriptor::default()
            });
            Pipelines {
                frame_layout,
                draw_layout,
                hdr_layout,
                material_layout,
                light_layout,
                sky,
                opaque,
                opaque_double,
                shadow,
                tonemap,
                sampler,
                shadow_sampler,
            }
        })?;
        let Pipelines {
            frame_layout,
            draw_layout,
            hdr_layout,
            material_layout,
            light_layout,
            sky,
            opaque,
            opaque_double,
            shadow,
            tonemap,
            sampler,
            shadow_sampler,
        } = pipelines;
        let default_textures = gpu.checked("create the default textures", |gpu| {
            let white = texture::solid(gpu, [255; 4], true, "white");
            let white_linear = texture::solid(gpu, [255; 4], false, "white data");
            let flat_normal = texture::solid(gpu, [128, 128, 255, 255], false, "flat normal");
            material_group(
                gpu,
                &material_layout,
                &sampler,
                [&white, &white_linear, &flat_normal, &white_linear, &white],
            )
        })?;
        // Separately, so that a broken device stops here with a diagnostic
        // instead of failing somewhere inside the next step.
        let shapes = gpu.checked("upload the built-in shapes", |gpu| {
            [Shape::Cube, Shape::Sphere, Shape::Plane]
                .into_iter()
                .map(|shape| GpuMesh::new(gpu, &primitive(shape), "primitive"))
                .collect()
        })?;
        let debug = debug::DebugPipelines::new(
            gpu,
            (&debug_source, &gizmo_source),
            &draw_layout,
            &material_layout,
        )?;
        let ui = crate::ui::UiPipeline::new(gpu, &ui_source, crate::capture::CAPTURE_FORMAT)?;
        let overlay = crate::sprites::OverlayPipeline::new(
            gpu,
            &sprite_source,
            crate::capture::CAPTURE_FORMAT,
            DEPTH_FORMAT,
        )?;
        Ok(Self {
            debug,
            ui,
            overlay,
            ui_pictures: BTreeMap::new(),
            frame_layout,
            draw_layout,
            hdr_layout,
            material_layout,
            sky,
            opaque,
            opaque_double,
            tonemap,
            sampler,
            light_layout,
            shadow,
            shadow_sampler,
            default_textures,
            shapes,
            models: BTreeMap::new(),
        })
    }

    /// Rebuilds every pipeline from `library` and swaps them in; uploaded
    /// models stay. If a shader has a mistake the error is returned and the
    /// renderer keeps drawing with the shaders it had.
    pub fn reload_shaders(&mut self, gpu: &Gpu, library: &ShaderLibrary) -> Result<()> {
        let fresh = Self::with_shaders(gpu, library)?;
        let models = std::mem::take(&mut self.models);
        let pictures = std::mem::take(&mut self.ui_pictures);
        *self = fresh;
        self.models = models;
        self.ui_pictures = pictures;
        Ok(())
    }

    /// Keeps a picture for the in-game interface or a sprite under its
    /// project path (`UiImage.image`, `Sprite.image`), replacing one of the
    /// same path.
    pub fn set_ui_picture(&mut self, path: &str, picture: crate::ui::UiPicture) {
        self.ui_pictures.insert(path.to_owned(), picture);
    }

    /// Whether a picture is kept under `path`.
    pub fn has_ui_picture(&self, path: &str) -> bool {
        self.ui_pictures.contains_key(path)
    }

    /// The sprites and particle emitters of `scene` as a `width` × `height`
    /// picture from `camera` (or the first camera) shows them: their boxes,
    /// and how many particles each emitter has alive.
    pub fn overlay_elements(
        &self,
        scene: &RenderScene,
        camera: Option<Entity>,
        width: u32,
        height: u32,
    ) -> Result<Vec<crate::sprites::OverlayElement>> {
        let view = choose_camera(scene, camera)?;
        Ok(crate::sprites::elements(
            scene,
            view,
            width,
            height,
            &self.ui_pictures,
        ))
    }

    /// Whether a model has been uploaded under `path`.
    pub fn has_model(&self, path: &str) -> bool {
        self.models.contains_key(path)
    }

    /// Uploads a model so that `Model` entities whose asset is `path` are
    /// drawn. Uploading again replaces the old copy (a changed file).
    pub fn upload_model(
        &mut self,
        gpu: &Gpu,
        path: &str,
        model: &wish_import::Model,
    ) -> Result<()> {
        // The device's limits, checked here so that a big model is refused
        // with its real cause instead of as a driver validation error.
        let limits = gpu.device.limits();
        for texture in &model.textures {
            if texture.width.max(texture.height) > limits.max_texture_dimension_2d {
                return Err(Diagnostic::error(
                    "texture_too_large_for_gpu",
                    format!(
                        "A texture in '{path}' is {}x{} pixels; this GPU takes at most {} per side.",
                        texture.width, texture.height, limits.max_texture_dimension_2d
                    ),
                    "Scale the texture down in the modelling tool (4096x4096 is plenty for most surfaces).",
                )
                .at_file(path));
            }
        }
        for mesh in &model.meshes {
            for primitive in &mesh.primitives {
                let bytes = primitive.positions.len() as u64 * VERTEX_STRIDE;
                if bytes > limits.max_buffer_size {
                    return Err(Diagnostic::error(
                        "mesh_too_large_for_gpu",
                        format!(
                            "A mesh in '{path}' needs {bytes} bytes of vertices; this GPU allows buffers of at most {}.",
                            limits.max_buffer_size
                        ),
                        "Split the mesh into several parts in the modelling tool, or reduce its detail.",
                    )
                    .at_file(path));
                }
            }
        }
        let uploaded = gpu.checked("upload a model", |gpu| {
            // Each texture is uploaded once per way it is used (colour or
            // data), because that decides how the GPU decodes it.
            let mut views: BTreeMap<(usize, bool), wgpu::TextureView> = BTreeMap::new();
            let mut view_of = |slot: Option<wish_import::TextureRef>, srgb: bool| {
                let reference = slot?;
                let texture = model.textures.get(reference.texture)?;
                Some(
                    views
                        .entry((reference.texture, srgb))
                        .or_insert_with(|| texture::upload(gpu, texture, srgb, path))
                        .clone(),
                )
            };
            let white = texture::solid(gpu, [255; 4], true, "white");
            let white_linear = texture::solid(gpu, [255; 4], false, "white data");
            let flat_normal = texture::solid(gpu, [128, 128, 255, 255], false, "flat normal");
            let default_material = wish_import::Material::default();
            // One bind group per material; primitives without one use the
            // default material.
            let mut groups = Vec::new();
            for material in model.materials.iter().chain([&default_material]) {
                let base =
                    view_of(material.base_color_texture, true).unwrap_or_else(|| white.clone());
                let mr = view_of(material.metallic_roughness_texture, false)
                    .unwrap_or_else(|| white_linear.clone());
                let normal =
                    view_of(material.normal_texture, false).unwrap_or_else(|| flat_normal.clone());
                let occlusion = view_of(material.occlusion_texture, false)
                    .unwrap_or_else(|| white_linear.clone());
                let emissive =
                    view_of(material.emissive_texture, true).unwrap_or_else(|| white.clone());
                groups.push(material_group(
                    gpu,
                    &self.material_layout,
                    &self.sampler,
                    [&base, &mr, &normal, &occlusion, &emissive],
                ));
            }
            let mut primitives = Vec::new();
            let mut blended = 0;
            for placed in model.placed() {
                let Some(source) = model
                    .meshes
                    .get(placed.mesh)
                    .and_then(|mesh| mesh.primitives.get(placed.primitive))
                else {
                    continue;
                };
                let index = source
                    .material
                    .filter(|index| *index < model.materials.len());
                let material = index
                    .and_then(|index| model.materials.get(index))
                    .unwrap_or(&default_material);
                if material.alpha_mode == wish_import::AlphaMode::Blend {
                    blended += 1;
                }
                let group_index = index.unwrap_or(model.materials.len());
                let Some(textures) = groups.get(group_index) else {
                    continue;
                };
                primitives.push(GpuPrimitive {
                    mesh: GpuMesh::new(gpu, &MeshData::from(source), path),
                    textures: textures.clone(),
                    material: MaterialData {
                        base_color: material.base_color,
                        metallic: material.metallic,
                        roughness: material.roughness,
                        emissive: material.emissive,
                    },
                    normal_scale: material.normal_scale,
                    occlusion_strength: material.occlusion_strength,
                    alpha_cutoff: if material.alpha_mode == wish_import::AlphaMode::Mask {
                        material.alpha_cutoff.max(1e-4)
                    } else {
                        0.0
                    },
                    double_sided: material.double_sided,
                    matrix: placed.matrix,
                    bounds: bounds_of(&source.positions),
                    placed,
                });
            }
            let animated = (!model.skins.is_empty() || !model.animations.is_empty())
                .then(|| std::sync::Arc::new(model.clone()));
            GpuModel {
                primitives,
                blended,
                animated,
            }
        })?;
        self.models.insert(path.to_owned(), uploaded);
        Ok(())
    }

    /// Draws `scene` from `camera` (or from its first camera) into a
    /// `width` × `height` image.
    pub fn render(
        &self,
        gpu: &Gpu,
        scene: &RenderScene,
        camera: Option<Entity>,
        width: u32,
        height: u32,
    ) -> Result<Frame> {
        let view = choose_camera(scene, camera)?;
        let target = Target::new(gpu, width, height)?;
        let posed = self.pose(gpu, scene)?;
        let aspect = width as f32 / height as f32;
        let view_proj = view.projection(aspect) * view.view;
        let exposure = exposure_multiplier(scene.ev100);

        let mut frame = Bytes::default();
        frame.mat4(&view_proj);
        frame.mat4(&view_proj.inverse());
        frame.vec4(view.position.to_array(), 1.0);
        match scene.lights.first() {
            Some(light) => {
                frame.vec4(light.direction.to_array(), 1.0);
                frame.vec4(light.radiance, 0.0);
            }
            None => {
                frame.vec4([0.0, -1.0, 0.0], 0.0);
                frame.vec4([0.0; 3], 0.0);
            }
        }
        frame.vec4(scene.ambient, 0.0);
        frame.vec4(scene.sky.zenith, 0.0);
        frame.vec4(scene.sky.horizon, 0.0);
        frame.vec4(scene.sky.ground, scene.sky.luminance);
        frame.vec4([exposure, EMISSIVE_NITS, 0.0], 0.0);

        let Collected {
            draw_list,
            mut warnings,
        } = self.collect(scene, None, &posed);
        // Sprites and particles, then the interface, are prepared on the CPU
        // and drawn over the finished picture.
        let overlay = crate::sprites::build(scene, view, aspect, &self.ui_pictures);
        let interface = (!scene.ui.is_empty())
            .then(|| crate::ui::build(scene, width, height, &self.ui_pictures));
        if let Some(batch) = &interface {
            warnings.extend(batch.warnings.iter().cloned());
        }
        let DrawList {
            mut bytes,
            draws,
            casters,
        } = draw_list;
        let draw_bytes = {
            bytes.pad_to((draws.len().max(1) as u64) * DRAW_UNIFORM_STRIDE);
            bytes
        };

        // The shadow map is fitted around everything drawn. A light that does
        // not cast shadows, or a scene with nothing in it, gets a 1x1 map that
        // is cleared to "nothing in the way".
        let fit = scene
            .lights
            .first()
            .filter(|light| light.shadows)
            .and_then(|light| fit_light(light.direction, &casters));
        frame.mat4(&fit.map_or(Mat4::IDENTITY, |fit| fit.view_proj));
        frame.floats(&[
            if fit.is_some() { 1.0 } else { 0.0 },
            fit.map_or(0.0, |fit| fit.depth_bias),
            fit.map_or(0.0, |fit| fit.normal_offset),
            1.0 / SHADOW_MAP_SIZE as f32,
        ]);
        let clusters = crate::cluster::build(
            &scene.local_lights,
            &view.view,
            &view.projection(aspect),
            (width, height),
            (view.near, view.far),
        );
        let lights = scene.local_lights.len().min(crate::cluster::MAX_LIGHTS);
        if scene.local_lights.len() > crate::cluster::MAX_LIGHTS {
            warnings.push(Diagnostic::warning(
                "too_many_lights",
                format!(
                    "The scene has {} point and spot lights; only the first {} are drawn.",
                    scene.local_lights.len(),
                    crate::cluster::MAX_LIGHTS
                ),
                "Remove lights that add little, or give them a smaller range.",
            ));
        }
        if clusters.dropped > 0 {
            warnings.push(Diagnostic::warning(
                "light_clusters_full",
                format!(
                    "Too many lights reach the same part of the picture: {} light placements were left out, so some areas are darker than they should be.",
                    clusters.dropped
                ),
                format!(
                    "At most {} lights can light one region; spread lights out or give them a smaller range.",
                    crate::cluster::MAX_PER_CLUSTER
                ),
            ));
        }
        frame.mat4(&view.view);
        frame.floats(&[
            clusters.tiles_x as f32,
            clusters.tiles_y as f32,
            crate::cluster::SLICES as f32,
            lights as f32,
            clusters.near,
            view.far,
            clusters.log_scale,
            crate::cluster::TILE_PX as f32,
        ]);
        frame.pad_to(FRAME_UNIFORM_SIZE);
        let local_light_bytes = light_bytes(&scene.local_lights);
        let range_bytes: Vec<u8> = clusters
            .ranges
            .iter()
            .flat_map(|[start, count]| [start.to_le_bytes(), count.to_le_bytes()])
            .flatten()
            .collect();
        let index_bytes: Vec<u8> = if clusters.indices.is_empty() {
            vec![0; 4]
        } else {
            clusters
                .indices
                .iter()
                .flat_map(|i| i.to_le_bytes())
                .collect()
        };
        let light_bytes = {
            let mut bytes = Bytes::default();
            bytes.mat4(&fit.map_or(Mat4::IDENTITY, |fit| fit.view_proj));
            bytes
        };

        gpu.checked("draw the frame", |gpu| {
            let device = &gpu.device;
            let texture = |label: &str, format, usage| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let hdr = texture(
                "hdr",
                HDR_FORMAT,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            )
            .create_view(&wgpu::TextureViewDescriptor::default());
            let depth = texture(
                "depth",
                DEPTH_FORMAT,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            )
            .create_view(&wgpu::TextureViewDescriptor::default());
            let frame_buffer = buffer_with(gpu, "frame", &frame.0, wgpu::BufferUsages::UNIFORM);
            let storage = wgpu::BufferUsages::STORAGE;
            let lights_buffer = buffer_with(gpu, "local lights", &local_light_bytes, storage);
            let ranges_buffer = buffer_with(gpu, "cluster ranges", &range_bytes, storage);
            let indices_buffer = buffer_with(gpu, "cluster lights", &index_bytes, storage);
            let draw_buffer = buffer_with(gpu, "draws", &draw_bytes.0, wgpu::BufferUsages::UNIFORM);
            let shadow_size = if fit.is_some() { SHADOW_MAP_SIZE } else { 1 };
            let shadow_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shadow map"),
                size: wgpu::Extent3d {
                    width: shadow_size,
                    height: shadow_size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SHADOW_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let shadow_view = shadow_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let light_buffer =
                buffer_with(gpu, "light", &light_bytes.0, wgpu::BufferUsages::UNIFORM);
            let light_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("light"),
                layout: &self.light_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: light_buffer.as_entire_binding(),
                }],
            });
            let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("frame"),
                layout: &self.frame_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: frame_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&shadow_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: lights_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: ranges_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: indices_buffer.as_entire_binding(),
                    },
                ],
            });
            let draw_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("draws"),
                layout: &self.draw_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &draw_buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(DRAW_UNIFORM_SIZE),
                    }),
                }],
            });
            let hdr_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("hdr"),
                layout: &self.hdr_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&hdr),
                }],
            });

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
            fn colour(
                view: &wgpu::TextureView,
                load: wgpu::LoadOp<wgpu::Color>,
            ) -> Option<wgpu::RenderPassColorAttachment<'_>> {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })
            }
            {
                // The shadow pass: the scene's depth seen from the light. With
                // no shadows it only clears the 1x1 map to 1.0.
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shadow"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &shadow_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                if fit.is_some() {
                    pass.set_pipeline(&self.shadow);
                    pass.set_bind_group(0, &light_group, &[]);
                    for (source, offset) in &draws {
                        let mesh = match source {
                            Source::Shape(shape) => match self.shapes.get(*shape) {
                                Some(mesh) => mesh,
                                None => continue,
                            },
                            Source::Primitive(primitive) => &primitive.mesh,
                        };
                        let offset = u32::try_from(*offset).unwrap_or(u32::MAX);
                        pass.set_bind_group(1, &draw_group, &[offset]);
                        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                    }
                }
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("sky"),
                    color_attachments: &[colour(&hdr, wgpu::LoadOp::Clear(wgpu::Color::BLACK))],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.sky);
                pass.set_bind_group(0, &frame_group, &[]);
                pass.draw(0..3, 0..1);
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("opaque"),
                    color_attachments: &[colour(&hdr, wgpu::LoadOp::Load)],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            // Kept for the sprites and particles.
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_bind_group(0, &frame_group, &[]);
                for (source, offset) in &draws {
                    let (mesh, textures, double_sided) = match source {
                        Source::Shape(shape) => {
                            let Some(mesh) = self.shapes.get(*shape) else {
                                continue;
                            };
                            (mesh, &self.default_textures, false)
                        }
                        Source::Primitive(primitive) => {
                            (&primitive.mesh, &primitive.textures, primitive.double_sided)
                        }
                    };
                    pass.set_pipeline(if double_sided {
                        &self.opaque_double
                    } else {
                        &self.opaque
                    });
                    let offset = u32::try_from(*offset).unwrap_or(u32::MAX);
                    pass.set_bind_group(1, &draw_group, &[offset]);
                    pass.set_bind_group(2, textures, &[]);
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("tonemap"),
                    color_attachments: &[colour(
                        &target.view,
                        wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    )],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.tonemap);
                pass.set_bind_group(0, &hdr_group, &[]);
                pass.draw(0..3, 0..1);
            }
            if let Some(overlay) = &overlay {
                self.overlay
                    .draw(gpu, &mut encoder, &target.view, &depth, overlay);
            }
            if let Some(batch) = &interface {
                self.ui.draw(gpu, &mut encoder, &target.view, batch);
            }
            target.copy(&mut encoder);
            gpu.queue.submit([encoder.finish()]);
        })?;
        Ok(Frame {
            image: target.read(gpu)?,
            camera: view.entity,
            drawn: draws.len(),
            warnings,
        })
    }
}

/// Where `point` (world space) lands in a `width` × `height` image seen
/// through `view_proj`, in pixels from the top left; `None` behind the
/// camera. Handy for tests and for `describe`.
pub fn project_to_pixel(
    view_proj: &Mat4,
    point: Vec3,
    width: u32,
    height: u32,
) -> Option<(f32, f32)> {
    let clip = *view_proj * point.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some((
        (ndc.x * 0.5 + 0.5) * width as f32,
        (0.5 - ndc.y * 0.5) * height as f32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(fit: &LightFit, point: Vec3) -> Vec3 {
        let clip = fit.view_proj * point.extend(1.0);
        clip.truncate() / clip.w
    }

    #[test]
    fn the_light_view_covers_every_corner_of_what_is_drawn() {
        let boxes = [
            (
                Mat4::from_translation(Vec3::new(3.0, 0.5, -2.0)),
                Vec3::splat(-0.5),
                Vec3::splat(0.5),
                1,
            ),
            (
                Mat4::from_scale(Vec3::new(30.0, 1.0, 30.0)),
                Vec3::splat(-0.5),
                Vec3::splat(0.5),
                1,
            ),
        ];
        for direction in [
            Vec3::NEG_Y,
            Vec3::new(0.3, -0.8, -0.5).normalize(),
            Vec3::Y,
            Vec3::NEG_Z,
        ] {
            let fit = fit_light(direction, &boxes).expect("something to cover");
            for (matrix, low, high, _) in &boxes {
                for corner in 0..8u8 {
                    let point = Vec3::new(
                        if corner & 1 == 0 { low.x } else { high.x },
                        if corner & 2 == 0 { low.y } else { high.y },
                        if corner & 4 == 0 { low.z } else { high.z },
                    );
                    let ndc = clip(&fit, matrix.transform_point3(point));
                    assert!(
                        ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z),
                        "{direction:?}: {ndc:?} is outside the shadow map"
                    );
                }
            }
            assert!(fit.depth_bias > 0.0 && fit.normal_offset > 0.0);
        }
    }

    #[test]
    fn nothing_drawn_or_bad_numbers_mean_no_shadow_map() {
        assert!(fit_light(Vec3::NEG_Y, &[]).is_none());
        let broken = [(
            Mat4::from_scale(Vec3::splat(f32::NAN)),
            Vec3::splat(-0.5),
            Vec3::splat(0.5),
            1,
        )];
        assert!(fit_light(Vec3::NEG_Y, &broken).is_none());
    }
}
