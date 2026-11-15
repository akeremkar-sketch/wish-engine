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

use wish_core::math::{Mat4, Vec3};
use wish_core::{Diagnostic, Entity, Result};

use crate::capture::{CAPTURE_FORMAT, Target};
use crate::components::{EMISSIVE_NITS, Shape};
use crate::extract::{Mesh, RenderScene};
use crate::gpu::Gpu;
use crate::image::RgbaImage;
use crate::mesh::{MeshData, VERTEX_STRIDE, primitive};
use crate::shader::ShaderLibrary;

const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Bytes of the frame uniform: two matrices and eight vectors.
const FRAME_UNIFORM_SIZE: u64 = 256;
/// Bytes of one draw's uniform data: two matrices and three vectors.
const DRAW_UNIFORM_SIZE: u64 = 176;
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
        "sky_pass",
        "wish_render/shaders/sky_pass.wgsl",
        include_str!("shaders/sky_pass.wgsl"),
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
];

/// The library of the engine's shaders.
pub fn engine_shaders() -> ShaderLibrary {
    let mut library = ShaderLibrary::new();
    for (name, file, source) in SHADERS {
        library.add(name, file, source);
    }
    library
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

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

/// A buffer filled through the queue. (wgpu's `create_buffer_init` maps the
/// buffer at creation and panics if that mapping fails; filling through the
/// queue cannot panic, and a failure surfaces in the error scope instead.)
/// `contents` must be a multiple of 4 bytes long, which all engine data is.
fn buffer_with(gpu: &Gpu, label: &str, contents: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
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
        Self {
            vertices: buffer_with(gpu, label, &mesh.vertex_bytes(), wgpu::BufferUsages::VERTEX),
            indices: buffer_with(gpu, label, &mesh.index_bytes(), wgpu::BufferUsages::INDEX),
            index_count: u32::try_from(mesh.indices.len()).unwrap_or(u32::MAX),
        }
    }
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

/// Draws scenes. Holds the pipelines and the built-in meshes, so create one
/// and reuse it for many frames.
pub struct Renderer {
    frame_layout: wgpu::BindGroupLayout,
    draw_layout: wgpu::BindGroupLayout,
    hdr_layout: wgpu::BindGroupLayout,
    sky: wgpu::RenderPipeline,
    opaque: wgpu::RenderPipeline,
    tonemap: wgpu::RenderPipeline,
    /// Cube, sphere, plane, in [`shape_index`] order.
    shapes: Vec<GpuMesh>,
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
        let library = engine_shaders();
        let mut modules = Vec::new();
        for entry in ["sky_pass", "opaque", "tonemap_pass"] {
            modules.push((entry, library.check(entry)?.source));
        }
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
                entries: &[uniform_entry(0, false, FRAME_UNIFORM_SIZE)],
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
            ];
            let opaque = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("opaque"),
                layout: Some(&layout(&[&frame_layout, &draw_layout])),
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
                    cull_mode: Some(wgpu::Face::Back),
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
            });
            (frame_layout, draw_layout, hdr_layout, sky, opaque, tonemap)
        })?;
        let (frame_layout, draw_layout, hdr_layout, sky, opaque, tonemap) = pipelines;
        // Separately, so that a broken device stops here with a diagnostic
        // instead of failing somewhere inside the next step.
        let shapes = gpu.checked("upload the built-in shapes", |gpu| {
            [Shape::Cube, Shape::Sphere, Shape::Plane]
                .into_iter()
                .map(|shape| GpuMesh::new(gpu, &primitive(shape), "primitive"))
                .collect()
        })?;
        Ok(Self {
            frame_layout,
            draw_layout,
            hdr_layout,
            sky,
            opaque,
            tonemap,
            shapes,
        })
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
        let view = match camera {
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
                })?,
            None => scene.cameras.first().ok_or_else(|| {
                Diagnostic::error(
                    "no_camera",
                    "The scene has no camera to draw from.",
                    "Add an entity with a \"Camera\" component and a \"Transform\" that places it; it looks along its -Z axis.",
                )
            })?,
        };
        let target = Target::new(gpu, width, height)?;
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
        frame.pad_to(FRAME_UNIFORM_SIZE);

        let mut warnings = Vec::new();
        let mut draws = Vec::new();
        let mut draw_bytes = Bytes::default();
        let mut models = 0usize;
        for draw in &scene.draws {
            let Mesh::Primitive(shape) = draw.mesh else {
                models += 1;
                continue;
            };
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
            let start = draws.len() as u64 * DRAW_UNIFORM_STRIDE;
            draw_bytes.pad_to(start);
            draw_bytes.mat4(&draw.world);
            draw_bytes.mat4(&draw.world.inverse().transpose());
            draw_bytes.floats(&draw.material.base_color);
            draw_bytes.floats(&[draw.material.metallic, draw.material.roughness, 0.0, 0.0]);
            draw_bytes.vec4(draw.material.emissive, 0.0);
            draws.push((shape_index(shape), start));
        }
        if models > 0 {
            warnings.push(Diagnostic::warning(
                "model_not_drawn",
                format!("{models} model(s) were not drawn: glTF models are drawn from M3 WP4 on."),
                "Use \"Primitive\" shapes until glTF loading lands; the Model components stay valid.",
            ));
        }
        draw_bytes.pad_to((draws.len().max(1) as u64) * DRAW_UNIFORM_STRIDE);

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
            let draw_buffer = buffer_with(gpu, "draws", &draw_bytes.0, wgpu::BufferUsages::UNIFORM);
            let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("frame"),
                layout: &self.frame_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame_buffer.as_entire_binding(),
                }],
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
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.opaque);
                pass.set_bind_group(0, &frame_group, &[]);
                for (shape, offset) in &draws {
                    let Some(mesh) = self.shapes.get(*shape) else {
                        continue;
                    };
                    let offset = u32::try_from(*offset).unwrap_or(u32::MAX);
                    pass.set_bind_group(1, &draw_group, &[offset]);
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
