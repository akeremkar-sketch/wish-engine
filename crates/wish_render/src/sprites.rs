//! Sprites and particles (design note M6 §8–§9), drawn after the 3D scene
//! and before the interface.
//!
//! Both are flat, unlit rectangles in the world: a sprite stands in its
//! entity's XY plane, a particle faces the camera. They are tested against
//! the scene's depth (a wall in front hides them) but do not write it, and
//! are drawn back to front: by `layer` (particles are on layer 0), then
//! farthest first. Pictures share one atlas built on the CPU, like the
//! interface's.

use std::collections::BTreeMap;

use wish_core::Entity;
use wish_core::math::{Mat4, Vec3, Vec4};

use crate::extract::{CameraView, RenderScene, SpriteView};
use crate::gpu::Gpu;
use crate::ui::{ATLAS_FORMAT, ATLAS_WIDTH, Atlas, Region, UiPicture, uv_of};

/// Bytes per vertex: position (3 floats), texture coordinate (2), colour
/// (4), disc flag (1).
const VERTEX_BYTES: u64 = 40;
/// The size of the overlay uniform: one matrix.
const UNIFORM_BYTES: u64 = 64;
/// The most particles drawn in one picture.
pub const MAX_PARTICLES: usize = 100_000;

/// A sprite's rectangle: its four corners in world space (bottom left,
/// bottom right, top right, top left) and the atlas region it shows (flips
/// applied), or `None` for plain colour.
fn sprite_quad(view: &SpriteView, pictures: &BTreeMap<String, UiPicture>) -> Option<[Vec3; 4]> {
    let sprite = &view.sprite;
    let picture = view.image.as_ref().and_then(|path| pictures.get(path));
    if view.image.is_some() && picture.is_none() {
        return None;
    }
    let [_, _, rw, rh] = sprite
        .rect
        .map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    let (width, height) = match picture {
        Some(_) if rw > 0.0 && rh > 0.0 => (rw, rh),
        Some(picture) => (picture.width as f32, picture.height as f32),
        None if rw > 0.0 && rh > 0.0 => (rw, rh),
        None => (100.0, 100.0),
    };
    let ppu = if sprite.pixels_per_unit.is_finite() && sprite.pixels_per_unit > 0.0 {
        sprite.pixels_per_unit
    } else {
        100.0
    };
    let (w, h) = (width / ppu, height / ppu);
    let [ax, ay] = sprite.anchor.map(|v| if v.is_finite() { v } else { 0.5 });
    let (left, bottom) = (-ax * w, -ay * h);
    let corners = [
        Vec3::new(left, bottom, 0.0),
        Vec3::new(left + w, bottom, 0.0),
        Vec3::new(left + w, bottom + h, 0.0),
        Vec3::new(left, bottom + h, 0.0),
    ]
    .map(|corner| view.world.transform_point3(corner));
    corners.iter().all(|c| c.is_finite()).then_some(corners)
}

/// The part of `picture` a sprite shows, in its pixels (left, top, width,
/// height), clamped to the picture.
fn source_rect(rect: [f32; 4], picture: &UiPicture) -> (u32, u32, u32, u32) {
    let [x, y, w, h] = rect.map(|v| if v.is_finite() { v.max(0.0) as u32 } else { 0 });
    if w == 0 || h == 0 {
        return (0, 0, picture.width, picture.height);
    }
    let x = x.min(picture.width.saturating_sub(1));
    let y = y.min(picture.height.saturating_sub(1));
    (
        x,
        y,
        w.min(picture.width - x).max(1),
        h.min(picture.height - y).max(1),
    )
}

/// One rectangle to draw.
struct Item {
    corners: [Vec3; 4],
    /// Texture coordinates of the four corners, or `None` for the white
    /// block (plain colour, particles).
    region: Option<(Region, bool, bool)>,
    colour: [f32; 4],
    disc: bool,
    smooth: bool,
    layer: i32,
    distance: f32,
}

/// The overlay of one picture, ready for the GPU.
pub(crate) struct Overlay {
    vertices: Vec<u8>,
    atlas: Vec<u8>,
    atlas_height: u32,
    /// Consecutive vertices drawn with the same sampler: smooth?, first,
    /// count.
    runs: Vec<(bool, u32, u32)>,
    view_proj: Mat4,
}

/// Prepares the sprites and particles of `scene` seen through `camera` in
/// a picture of `aspect`; `None` when there are none.
pub(crate) fn build(
    scene: &RenderScene,
    camera: &CameraView,
    aspect: f32,
    pictures: &BTreeMap<String, UiPicture>,
) -> Option<Overlay> {
    if scene.sprites.is_empty() && scene.emitters.is_empty() {
        return None;
    }
    let mut atlas = Atlas::new();
    let mut placed: BTreeMap<String, Option<Region>> = BTreeMap::new();
    let mut items: Vec<Item> = Vec::new();
    let distance = |point: Vec3| -camera.view.transform_point3(point).z;
    for view in &scene.sprites {
        let Some(corners) = sprite_quad(view, pictures) else {
            continue;
        };
        let centre = (corners[0] + corners[2]) * 0.5;
        let region = match &view.image {
            Some(path) => {
                let region = match placed.get(path) {
                    Some(region) => *region,
                    None => {
                        let region = pictures.get(path).and_then(|picture| {
                            let (ax, ay) = atlas.place(picture.width, picture.height)?;
                            for row in 0..picture.height {
                                for column in 0..picture.width {
                                    let i = ((row * picture.width + column) * 4) as usize;
                                    let mut texel = [0u8; 4];
                                    texel.copy_from_slice(picture.rgba.get(i..i + 4)?);
                                    atlas.set(ax + column, ay + row, texel);
                                }
                            }
                            Some((ax, ay, picture.width, picture.height))
                        });
                        placed.insert(path.clone(), region);
                        region
                    }
                };
                let Some((ax, ay, _, _)) = region else {
                    continue;
                };
                let Some(picture) = pictures.get(path) else {
                    continue;
                };
                let (sx, sy, sw, sh) = source_rect(view.sprite.rect, picture);
                Some((
                    (ax + sx, ay + sy, sw, sh),
                    view.sprite.flip_x,
                    view.sprite.flip_y,
                ))
            }
            None => None,
        };
        let colour = view.sprite.color.to_array();
        if !colour.iter().all(|c| c.is_finite()) {
            continue;
        }
        items.push(Item {
            corners,
            region,
            colour,
            disc: false,
            smooth: view.sprite.smooth,
            layer: view.sprite.layer,
            distance: distance(centre),
        });
    }
    // Particles face the camera.
    let to_world = camera.view.inverse();
    let right = to_world.transform_vector3(Vec3::X).normalize_or(Vec3::X);
    let up = to_world.transform_vector3(Vec3::Y).normalize_or(Vec3::Y);
    let mut particles = 0usize;
    for view in &scene.emitters {
        for particle in wish_particles::particles(&view.emitter, &view.world, scene.time) {
            if particles >= MAX_PARTICLES {
                break;
            }
            if particle.size <= 0.0 || !particle.position.is_finite() {
                continue;
            }
            particles += 1;
            let (r, u) = (right * particle.size * 0.5, up * particle.size * 0.5);
            let p = particle.position;
            items.push(Item {
                corners: [p - r - u, p + r - u, p + r + u, p - r + u],
                region: None,
                colour: particle.color,
                disc: true,
                smooth: true,
                layer: 0,
                distance: distance(p),
            });
        }
    }
    if items.is_empty() {
        return None;
    }
    // Back to front: lower layers first, then farther first.
    items.sort_by(|a, b| {
        a.layer
            .cmp(&b.layer)
            .then(b.distance.total_cmp(&a.distance))
    });
    let atlas_height = atlas.height.max(1);
    let mut vertices = Vec::with_capacity(items.len() * 6 * VERTEX_BYTES as usize);
    let mut runs: Vec<(bool, u32, u32)> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let uv: [[f32; 2]; 4] = match item.region {
            _ if item.disc => [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
            None => {
                let [u, v, _, _] = uv_of((2, 2, 0, 0), atlas_height);
                [[u, v]; 4]
            }
            Some((region, flip_x, flip_y)) => {
                let [mut u0, mut v0, mut u1, mut v1] = uv_of(region, atlas_height);
                if flip_x {
                    std::mem::swap(&mut u0, &mut u1);
                }
                if flip_y {
                    std::mem::swap(&mut v0, &mut v1);
                }
                // The picture's top is the sprite's top (+Y).
                [[u0, v1], [u1, v1], [u1, v0], [u0, v0]]
            }
        };
        let disc = if item.disc { 1.0f32 } else { 0.0 };
        for corner in [0usize, 1, 2, 0, 2, 3] {
            let p = item.corners[corner];
            for value in [p.x, p.y, p.z, uv[corner][0], uv[corner][1]]
                .into_iter()
                .chain(item.colour)
                .chain([disc])
            {
                vertices.extend_from_slice(&value.to_le_bytes());
            }
        }
        let first = u32::try_from(index * 6).unwrap_or(u32::MAX);
        match runs.last_mut() {
            Some((smooth, _, count)) if *smooth == item.smooth => *count += 6,
            _ => runs.push((item.smooth, first, 6)),
        }
    }
    Some(Overlay {
        vertices,
        atlas: atlas.pixels,
        atlas_height,
        runs,
        view_proj: camera.projection(aspect) * camera.view,
    })
}

/// What `describe` says about a sprite or an emitter.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayElement {
    /// The entity.
    pub entity: Entity,
    /// Its box in the picture: left, top, right, bottom, in pixels; `None`
    /// when it is not in front of the camera.
    pub bounding_box: Option<[i32; 4]>,
    /// For an emitter, how many particles are alive; `None` for a sprite.
    pub particles: Option<u32>,
    /// For a sprite, its layer.
    pub layer: Option<i32>,
}

/// The box `points` cover in a `width` × `height` picture through
/// `view_proj`, if any of them is in front of the camera and the box
/// touches the picture.
fn picture_box(points: &[Vec3], view_proj: &Mat4, width: u32, height: u32) -> Option<[i32; 4]> {
    let mut low = [f32::MAX; 2];
    let mut high = [f32::MIN; 2];
    let mut any = false;
    for point in points {
        let clip: Vec4 = *view_proj * point.extend(1.0);
        if clip.w <= 1e-6 {
            continue;
        }
        let x = (clip.x / clip.w * 0.5 + 0.5) * width as f32;
        let y = (0.5 - clip.y / clip.w * 0.5) * height as f32;
        low = [low[0].min(x), low[1].min(y)];
        high = [high[0].max(x), high[1].max(y)];
        any = true;
    }
    let inside = any
        && high[0] >= 0.0
        && high[1] >= 0.0
        && low[0] <= width as f32
        && low[1] <= height as f32;
    inside.then(|| {
        [
            low[0].max(0.0).round() as i32,
            low[1].max(0.0).round() as i32,
            high[0].min(width as f32).round() as i32,
            high[1].min(height as f32).round() as i32,
        ]
    })
}

/// The sprites and emitters of `scene` as a `width` × `height` picture seen
/// through `camera` shows them.
pub(crate) fn elements(
    scene: &RenderScene,
    camera: &CameraView,
    width: u32,
    height: u32,
    pictures: &BTreeMap<String, UiPicture>,
) -> Vec<OverlayElement> {
    let view_proj = camera.projection(width as f32 / height.max(1) as f32) * camera.view;
    let mut found = Vec::new();
    for view in &scene.sprites {
        let bounding_box = sprite_quad(view, pictures)
            .and_then(|corners| picture_box(&corners, &view_proj, width, height));
        found.push(OverlayElement {
            entity: view.entity,
            bounding_box,
            particles: None,
            layer: Some(view.sprite.layer),
        });
    }
    for view in &scene.emitters {
        let particles = wish_particles::particles(&view.emitter, &view.world, scene.time);
        let points: Vec<Vec3> = particles
            .iter()
            .flat_map(|p| {
                let half = Vec3::splat(p.size * 0.5);
                [p.position - half, p.position + half]
            })
            .collect();
        found.push(OverlayElement {
            entity: view.entity,
            bounding_box: picture_box(&points, &view_proj, width, height),
            particles: Some(u32::try_from(particles.len()).unwrap_or(u32::MAX)),
            layer: None,
        });
    }
    found
}

/// The sprite and particle pipeline.
pub(crate) struct OverlayPipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    smooth: wgpu::Sampler,
    sharp: wgpu::Sampler,
}

impl OverlayPipeline {
    /// Builds the pipeline from the `sprite_pass` shader, drawing into
    /// pictures of `format` over a depth buffer of `depth`.
    pub fn new(
        gpu: &Gpu,
        source: &str,
        format: wgpu::TextureFormat,
        depth: wgpu::TextureFormat,
    ) -> wish_core::Result<Self> {
        gpu.checked("create the sprite pipeline", |gpu| {
            let device = &gpu.device;
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("sprites"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sprites"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("sprite_pass"),
                source: wgpu::ShaderSource::Wgsl(source.to_owned().into()),
            });
            let attribute = |location: u32, offset: u64, format| wgpu::VertexAttribute {
                format,
                offset,
                shader_location: location,
            };
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sprites"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: VERTEX_BYTES,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            attribute(0, 0, wgpu::VertexFormat::Float32x3),
                            attribute(1, 12, wgpu::VertexFormat::Float32x2),
                            attribute(2, 20, wgpu::VertexFormat::Float32x4),
                            attribute(3, 36, wgpu::VertexFormat::Float32),
                        ],
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            let sampler = |label, filter| {
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some(label),
                    mag_filter: filter,
                    min_filter: filter,
                    ..wgpu::SamplerDescriptor::default()
                })
            };
            Self {
                pipeline,
                layout,
                smooth: sampler("sprites smooth", wgpu::FilterMode::Linear),
                sharp: sampler("sprites sharp", wgpu::FilterMode::Nearest),
            }
        })
    }

    /// Records drawing `overlay` over `target`, tested against `depth`.
    /// Call inside `Gpu::checked`.
    pub fn draw(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        overlay: &Overlay,
    ) {
        let device = &gpu.device;
        let size = wgpu::Extent3d {
            width: ATLAS_WIDTH,
            height: overlay.atlas_height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite atlas"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ATLAS_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &overlay.atlas,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS_WIDTH * 4),
                rows_per_image: Some(overlay.atlas_height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let matrix: Vec<u8> = overlay
            .view_proj
            .to_cols_array()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let uniform =
            crate::renderer::buffer_with(gpu, "sprites", &matrix, wgpu::BufferUsages::UNIFORM);
        let group = |sampler: &wgpu::Sampler| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sprites"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        };
        let (smooth, sharp) = (group(&self.smooth), group(&self.sharp));
        let vertices = crate::renderer::buffer_with(
            gpu,
            "sprites",
            &overlay.vertices,
            wgpu::BufferUsages::VERTEX,
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sprites"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, vertices.slice(..));
        for (is_smooth, first, count) in &overlay.runs {
            pass.set_bind_group(0, if *is_smooth { &smooth } else { &sharp }, &[]);
            pass.draw(*first..first + count, 0..1);
        }
    }
}
