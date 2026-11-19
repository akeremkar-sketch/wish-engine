//! Drawing a [`RenderScene`] into an image.
//!
//! A frame is planned on the CPU and then drawn by a fixed list of passes
//! (`passes.rs` has the table of what each reads and writes):
//!
//! 1. **Shadow:** the scene's depth as the directional light sees it: one
//!    map around everything, or cascades fitted to slices of the camera's
//!    depth when the scene asks for them.
//! 2. **Sky:** fills the HDR target with the sky gradient.
//! 3. **Opaque:** draws every surface with the glTF metallic-roughness BRDF,
//!    one directional light with its shadow, the point and spot lights of
//!    the pixel's cluster, ambient light and the sky's reflection, with a
//!    depth buffer.
//! 4. **Bloom:** when the scene asks for it, bright light spread around
//!    itself: down a chain of ever smaller pictures, back up it, and into a
//!    second HDR target that tone mapping then reads.
//! 5. **Tone mapping:** colour grading when the scene asks for it, then
//!    Khronos PBR Neutral, from the HDR target to the 8-bit sRGB picture.
//! 6. **FXAA:** when the scene asks for it, edges smoothed over the
//!    tone-mapped picture.
//! 7. **Overlay:** sprites and particles, tested against the depth buffer.
//! 8. **Interface:** the in-game interface.
//! 9. **Readback:** the picture copied to the CPU.
//!
//! The sky's light, the sun's disc, fog, bloom, grading and FXAA are each
//! off unless the scene asks for them (`look.rs`): a scene that asks for
//! none is drawn exactly as it was before they existed.
//!
//! Values are pre-exposed: the sky and opaque passes multiply by the
//! exposure before writing, so the HDR target holds displayable magnitudes.
//!
//! The parts:
//!
//! - `pipelines.rs`: layouts and pipelines, built once per renderer;
//! - `models.rs`: uploaded meshes and models, posing animated models;
//! - `draw_list.rs`: a scene's draws as draw calls, shared by every pass
//!   that draws meshes, the debug views included;
//! - `shadow.rs`: the shadow maps (one, or cascades), their sizes and the
//!   light's fitted views;
//! - `frame.rs`: the frame plan, its statistics, and its GPU resources;
//! - `look.rs`: the look settings as the numbers the shaders read;
//! - `bloom.rs`: the bloom's levels, targets and passes;
//! - `pool.rs`: targets and buffers kept across frames (and why bind groups
//!   are not);
//! - `passes.rs`: the passes;
//! - `debug.rs`: the debug views, the identity buffer and `describe`.
//!
//! Design: `docs/design/M12-render-foundation.md`; the look settings:
//! `docs/design/M12-look-1-sky-fog-grading-fxaa.md` and
//! `docs/design/M12-look-2-bloom.md`; the cascades:
//! `docs/design/M12-look-3-cascaded-shadows.md`.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use wish_core::math::{Mat4, Vec3};
use wish_core::{Diagnostic, Entity, Result};

use crate::capture::{CAPTURE_FORMAT, check_size};
use crate::components::Shape;
use crate::extract::{CameraView, RenderScene};
use crate::gpu::Gpu;
use crate::image::RgbaImage;
use crate::mesh::primitive;
use crate::shader::ShaderLibrary;
use crate::texture;

mod bloom;
mod debug;
mod draw_list;
mod frame;
mod look;
mod models;
mod passes;
mod pipelines;
mod pool;
mod shadow;
#[cfg(test)]
mod warp_probe;

pub use debug::{Description, IdBuffer, MAX_VISIBILITY_MEASUREMENTS, View, VisibleEntity};
pub use frame::RenderStats;
pub use look::Effects;
pub use pool::PoolCounts;
pub use shadow::SHADOW_MAP_SIZE;

use frame::FrameResources;
use models::{GpuMesh, GpuModel};
use pipelines::{Pipelines, material_group};
pub(crate) use pool::{Pool, TextureKey};

const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Bytes of the frame uniform: four matrices and seventeen vectors, and the
/// shadow cascades.
const FRAME_UNIFORM_SIZE: u64 = 528 + shadow::CASCADES_UNIFORM_SIZE;
/// Bytes of one point or spot light in the light buffer: four vectors.
const LIGHT_STRIDE: usize = 64;
const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Bytes of one draw's uniform data: two matrices and four vectors.
const DRAW_UNIFORM_SIZE: u64 = 192;
/// Distance between draws in the draw uniform buffer. 256 is a multiple of
/// every adapter's uniform offset alignment.
const DRAW_UNIFORM_STRIDE: u64 = 256;

/// The engine's own shader modules: (name, file, source).
const SHADERS: &[(&str, &str, &str)] = &[
    (
        "cascades",
        "wish_render/shaders/cascades.wgsl",
        include_str!("shaders/cascades.wgsl"),
    ),
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
        "fog",
        "wish_render/shaders/fog.wgsl",
        include_str!("shaders/fog.wgsl"),
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
        "bloom_pass",
        "wish_render/shaders/bloom_pass.wgsl",
        include_str!("shaders/bloom_pass.wgsl"),
    ),
    (
        "fxaa_pass",
        "wish_render/shaders/fxaa_pass.wgsl",
        include_str!("shaders/fxaa_pass.wgsl"),
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
    "bloom_pass",
    "tonemap_pass",
    "fxaa_pass",
    "shadow_pass",
    "debug_pass",
    "gizmo_pass",
    "ui_pass",
    "sprite_pass",
];

/// The functions the engine builds pipelines from, for each shader of
/// [`ENTRY_SHADERS`]. A shader that is valid WGSL but lacks one of them
/// would fail only when the GPU builds the pipeline, with a message that
/// names no file; [`check_entry`] finds it first.
///
/// When a pipeline starts using a new function, add it here (the test
/// `every_pipeline_function_is_in_the_table` reads the pipeline code and
/// fails if one is missing).
const ENTRY_POINTS: &[(&str, &[(naga::ShaderStage, &str)])] = {
    use naga::ShaderStage::{Fragment, Vertex};
    &[
        (
            "sky_pass",
            &[
                (Vertex, "vs_main"),
                (Fragment, "fs_main"),
                (Fragment, "fs_look"),
            ],
        ),
        ("opaque", &[(Vertex, "vs_main"), (Fragment, "fs_main")]),
        (
            "bloom_pass",
            &[
                (Vertex, "vs_main"),
                (Fragment, "fs_first"),
                (Fragment, "fs_down"),
                (Fragment, "fs_up"),
                (Fragment, "fs_composite"),
            ],
        ),
        (
            "tonemap_pass",
            &[
                (Vertex, "vs_main"),
                (Fragment, "fs_main"),
                (Fragment, "fs_graded"),
            ],
        ),
        ("fxaa_pass", &[(Vertex, "vs_main"), (Fragment, "fs_main")]),
        (
            "shadow_pass",
            &[
                (Vertex, "vs_main"),
                (Vertex, "vs_masked"),
                (Fragment, "fs_masked"),
            ],
        ),
        (
            "debug_pass",
            &[
                (Vertex, "vs_main"),
                (Fragment, "fs_ids"),
                (Fragment, "fs_depth"),
                (Fragment, "fs_normals"),
                (Fragment, "fs_fill"),
                (Fragment, "fs_line"),
                (Fragment, "fs_cascades"),
            ],
        ),
        ("gizmo_pass", &[(Vertex, "vs_main"), (Fragment, "fs_main")]),
        ("ui_pass", &[(Vertex, "vs_main"), (Fragment, "fs_main")]),
        ("sprite_pass", &[(Vertex, "vs_main"), (Fragment, "fs_main")]),
    ]
};

/// Composes and checks the pipeline shader `entry` of `library`, and makes
/// sure it defines every function its pipelines are built from. A missing
/// one is reported in the shader's own file, with the function's name.
fn check_entry(library: &ShaderLibrary, entry: &str) -> Result<crate::shader::CheckedShader> {
    let checked = library.check(entry)?;
    let required = ENTRY_POINTS
        .iter()
        .find(|(name, _)| *name == entry)
        .map_or(&[][..], |(_, functions)| *functions);
    let missing: Vec<String> = required
        .iter()
        .filter(|(stage, name)| {
            !checked
                .module
                .entry_points
                .iter()
                .any(|point| point.stage == *stage && point.name == *name)
        })
        .map(|(stage, name)| {
            let stage = match stage {
                naga::ShaderStage::Vertex => "vertex",
                naga::ShaderStage::Fragment => "fragment",
                _ => "compute",
            };
            format!("`@{stage} fn {name}`")
        })
        .collect();
    if missing.is_empty() {
        return Ok(checked);
    }
    let mut problem = Diagnostic::error(
        "shader_entry_point_missing",
        format!(
            "The shader module '{entry}' does not define {}, which the engine builds its pipelines from.",
            missing.join(" and ")
        ),
        format!(
            "Add the missing function to this file. A copy of an engine shader made for an earlier version lacks what was added since: start again from the engine's own {entry}.wgsl (crates/wish_render/src/shaders) and carry your changes over."
        ),
    );
    if let Some(file) = library.file_of(entry) {
        problem = problem.at_file(file);
    }
    Err(problem)
}

/// Checks every pipeline shader of `library` and returns every problem found
/// (one per shader at most), without needing a GPU: mistakes in the WGSL,
/// and functions the engine's pipelines need that a shader does not define.
pub fn check_shaders(library: &ShaderLibrary) -> Vec<Diagnostic> {
    ENTRY_SHADERS
        .iter()
        .filter_map(|entry| check_entry(library, entry).err())
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
    let buffer = gpu.create_buffer(&wgpu::BufferDescriptor {
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

/// What a frame produced.
#[derive(Debug)]
pub struct Frame {
    /// The image.
    pub image: RgbaImage,
    /// The camera it was drawn from.
    pub camera: Entity,
    /// How many draws made it into the image.
    pub drawn: usize,
    /// What drawing this picture took, in counts.
    pub stats: RenderStats,
    /// Things that were not drawn, and why.
    pub warnings: Vec<Diagnostic>,
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

/// Draws scenes. Holds the pipelines, the built-in meshes, the uploaded
/// models and the targets of the last frames, so create one and reuse it for
/// many frames.
pub struct Renderer {
    /// The layouts and pipelines of the lit picture.
    pipelines: Pipelines,
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
    /// Targets and buffers kept from frame to frame. Locked for
    /// the whole of a call, readback included: two threads sharing a
    /// renderer take turns (as the GPU lock makes them do anyway). Always
    /// taken before the GPU lock.
    pool: Mutex<Pool>,
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
            modules.push((entry, check_entry(library, entry)?.source));
        }
        let source_of = |name: &str| {
            modules
                .iter()
                .find(|(entry, _)| *entry == name)
                .map(|(_, source)| source.clone())
                .unwrap_or_default()
        };
        let pipelines = Pipelines::build(gpu, &source_of)?;
        let default_textures = gpu.checked("create the default textures", |gpu| {
            let white = texture::solid(gpu, [255; 4], true, "white");
            let white_linear = texture::solid(gpu, [255; 4], false, "white data");
            let flat_normal = texture::solid(gpu, [128, 128, 255, 255], false, "flat normal");
            material_group(
                gpu,
                &pipelines.material_layout,
                &pipelines.sampler,
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
            (&source_of("debug_pass"), &source_of("gizmo_pass")),
            &pipelines.draw_layout,
            &pipelines.material_layout,
        )?;
        let ui = crate::ui::UiPipeline::new(gpu, &source_of("ui_pass"), CAPTURE_FORMAT)?;
        let overlay = crate::sprites::OverlayPipeline::new(
            gpu,
            &source_of("sprite_pass"),
            CAPTURE_FORMAT,
            DEPTH_FORMAT,
        )?;
        Ok(Self {
            pipelines,
            default_textures,
            shapes,
            models: BTreeMap::new(),
            debug,
            ui,
            overlay,
            ui_pictures: BTreeMap::new(),
            pool: Mutex::new(Pool::default()),
        })
    }

    /// Rebuilds every pipeline from `library` and swaps them in; uploaded
    /// models stay. If a shader has a mistake the error is returned and the
    /// renderer keeps drawing with the shaders it had.
    pub fn reload_shaders(&mut self, gpu: &Gpu, library: &ShaderLibrary) -> Result<()> {
        let fresh = Self::with_shaders(gpu, library)?;
        let models = std::mem::take(&mut self.models);
        let pictures = std::mem::take(&mut self.ui_pictures);
        // The pool goes with the old pipelines: its bind groups were made
        // for their layouts.
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

    /// How many textures and buffers this renderer's pool has created so
    /// far: for tests and measurements that prove frames reuse their targets
    /// (two frames of the same size and content create none). What is
    /// created beside the pool is counted by `Gpu::created`.
    #[doc(hidden)]
    pub fn pool_counts(&self) -> PoolCounts {
        self.pool().counts()
    }

    /// How many bytes of GPU memory the targets and buffers hold that this
    /// renderer keeps for its next frame.
    pub fn kept_bytes(&self) -> u64 {
        self.pool().bytes()
    }

    /// Lets go of kept targets and buffers, the least recently used first,
    /// until at most `keep_bytes` are kept (0: everything).
    ///
    /// A renderer keeps the targets of its last frames so that the next
    /// frame creates nothing: right for a window that draws many frames a
    /// second, and it holds itself to 448 MiB beside what its last frame
    /// used. A caller that draws now and then (a tool that takes one
    /// picture) calls this afterwards, so that a large picture does not stay
    /// in memory until some later call. Uploaded models and pictures are not
    /// touched.
    pub fn trim(&self, keep_bytes: u64) {
        self.pool().trim(keep_bytes);
    }

    /// The pool, locked.
    fn pool(&self) -> MutexGuard<'_, Pool> {
        self.pool.lock().unwrap_or_else(|poisoned| {
            // A panic in the middle of a frame: whatever is in the pool may
            // be half-used, so it starts empty.
            self.pool.clear_poison();
            let mut pool = poisoned.into_inner();
            pool.clear();
            pool
        })
    }

    /// Runs one call's GPU work with the pool. If it fails, the pool is
    /// emptied: after an error nothing half-used is used again (a readback
    /// buffer that is still mapped, for one). If it goes well, the pool lets
    /// go of what no longer fits its budget before the call returns, not at
    /// the start of some later call: a renderer that is not called again
    /// must not hold what it no longer needs.
    fn pooled<T>(&self, work: impl FnOnce(&mut Pool) -> Result<T>) -> Result<T> {
        let mut pool = self.pool();
        pool.begin_call();
        let result = work(&mut pool);
        if result.is_err() {
            pool.clear();
        } else {
            pool.end_call();
        }
        result
    }

    /// Draws `scene` from `camera` (or from its first camera) into a
    /// `width` x `height` image.
    pub fn render(
        &self,
        gpu: &Gpu,
        scene: &RenderScene,
        camera: Option<Entity>,
        width: u32,
        height: u32,
    ) -> Result<Frame> {
        let view = choose_camera(scene, camera)?;
        check_size(width, height)?;
        self.pooled(|pool| self.render_lit(gpu, pool, scene, view, (width, height)))
    }

    /// The lit picture: plan it, take its targets from the pool, run the
    /// passes, read it back.
    fn render_lit(
        &self,
        gpu: &Gpu,
        pool: &mut Pool,
        scene: &RenderScene,
        view: &CameraView,
        size: (u32, u32),
    ) -> Result<Frame> {
        let picture = pool.picture(gpu, "picture", size, CAPTURE_FORMAT)?;
        let posed = self.pose(gpu, scene)?;
        let plan = self.plan(gpu, scene, view, size, &posed);
        gpu.checked("draw the frame", |gpu| {
            let resources = FrameResources::acquire(gpu, pool, &self.pipelines, &plan);
            let mut encoder = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("frame"),
                });
            passes::shadow(
                &mut encoder,
                &passes::Shadow {
                    map: &resources.shadow_map,
                    solid: &self.pipelines.shadow,
                    masked: &self.pipelines.shadow_masked,
                    light: &resources.light_group,
                    draw_uniforms: &resources.draw_group,
                    casters: &plan.draws,
                    maps: &passes::shadow_maps(plan.shadow.as_ref()),
                },
            );
            passes::sky(
                &mut encoder,
                &resources.hdr,
                if plan.sky_look {
                    &self.pipelines.sky_look
                } else {
                    &self.pipelines.sky
                },
                &resources.frame_group,
            );
            passes::opaque(
                &mut encoder,
                &passes::Opaque {
                    hdr: &resources.hdr,
                    depth: &resources.depth,
                    single_sided: &self.pipelines.opaque,
                    double_sided: &self.pipelines.opaque_double,
                    frame: &resources.frame_group,
                    draw_uniforms: &resources.draw_group,
                    draws: &plan.draws,
                },
            );
            // Bloom, for a frame that asks for it: tone mapping then reads
            // the picture these passes write (the frame's tone mapping group
            // binds it).
            if let (Some(bloom), Some(targets)) = (&plan.bloom, &resources.bloom) {
                bloom::draw(&mut encoder, &self.pipelines, bloom, targets);
            }
            // With FXAA the tone-mapped picture is an intermediate one and
            // the FXAA pass writes the picture; without, tone mapping writes
            // the picture itself, as it always did.
            let tonemap = if plan.grade_uniform.is_some() {
                &self.pipelines.tonemap_graded
            } else {
                &self.pipelines.tonemap
            };
            match &resources.fxaa {
                Some((tone_mapped, group)) => {
                    passes::tone_map(&mut encoder, tone_mapped, tonemap, &resources.tonemap_group);
                    passes::fxaa(&mut encoder, &picture.view, &self.pipelines.fxaa, group);
                }
                None => passes::tone_map(
                    &mut encoder,
                    &picture.view,
                    tonemap,
                    &resources.tonemap_group,
                ),
            }
            if let Some(overlay) = &plan.overlay {
                passes::overlay(
                    (gpu, pool),
                    &mut encoder,
                    &self.overlay,
                    (&picture.view, &resources.depth),
                    overlay,
                );
            }
            if let Some(batch) = &plan.interface {
                passes::interface((gpu, pool), &mut encoder, &self.ui, &picture.view, batch);
            }
            passes::copy_out(&mut encoder, &picture);
            // Done before the GPU lock is let go: see `Gpu::finish`.
            gpu.finish(encoder)
        })??;
        Ok(Frame {
            image: passes::readback(gpu, &picture)?,
            camera: view.entity,
            drawn: plan.draws.items.len(),
            stats: plan.stats,
            warnings: plan.warnings,
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
    use std::collections::BTreeSet;

    use super::*;

    /// The `vs_...` and `fs_...` names a source file gives to pipelines.
    fn functions_named_in(source: &str) -> BTreeSet<&str> {
        let mut found = BTreeSet::new();
        for prefix in ["\"vs_", "\"fs_"] {
            let mut rest = source;
            while let Some(at) = rest.find(prefix) {
                let name = &rest[at + 1..];
                let end = name.find('"').unwrap_or(name.len());
                found.insert(&name[..end]);
                rest = &name[end..];
            }
        }
        found
    }

    #[test]
    fn every_pipeline_function_is_in_the_table() {
        // The table names exactly the shaders that make up pipelines.
        let listed: Vec<&str> = ENTRY_POINTS.iter().map(|(name, _)| *name).collect();
        assert_eq!(listed, ENTRY_SHADERS);
        // Each file that builds pipelines, with the shaders it builds them
        // from: the functions its code names are the table's.
        let files: [(&str, &str, &[&str]); 4] = [
            (
                "pipelines.rs",
                include_str!("renderer/pipelines.rs"),
                &[
                    "sky_pass",
                    "opaque",
                    "bloom_pass",
                    "tonemap_pass",
                    "fxaa_pass",
                    "shadow_pass",
                ],
            ),
            (
                "debug.rs",
                include_str!("renderer/debug.rs"),
                &["debug_pass", "gizmo_pass"],
            ),
            ("ui.rs", include_str!("ui.rs"), &["ui_pass"]),
            ("sprites.rs", include_str!("sprites.rs"), &["sprite_pass"]),
        ];
        for (file, source, shaders) in files {
            let table: BTreeSet<&str> = ENTRY_POINTS
                .iter()
                .filter(|(name, _)| shaders.contains(name))
                .flat_map(|(_, functions)| functions.iter().map(|(_, function)| *function))
                .collect();
            assert_eq!(
                functions_named_in(source),
                table,
                "{file}: the functions its pipelines use, against ENTRY_POINTS"
            );
        }
        // And the engine's own shaders define them all.
        assert!(check_shaders(&engine_shaders()).is_empty());
    }
}
