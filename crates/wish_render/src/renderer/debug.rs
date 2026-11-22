//! The debug views, the identity buffer and `describe`: what the scene looks
//! like to a program rather than to a person.
//!
//! - **ids:** every pixel holds which entity is drawn there. [`IdBuffer`]
//!   answers "which entity is at this pixel"; the `ids` view paints it in
//!   colours.
//! - **depth:** distance from the camera, white near and black far.
//! - **normals:** the surface normal in world space as a colour.
//! - **wireframe:** the triangle edges.
//! - **colliders:** the wireframe, with every physics collider drawn over it
//!   (fixed green, moving cyan, sensors yellow).
//! - **lights:** the wireframe, with where each point and spot light reaches
//!   (its range sphere or cone, in its colour).
//! - **shadow_cascades:** every surface in the colour of the shadow map its
//!   shadow is read from in the lit picture (red, green, blue, yellow from
//!   the nearest cascade; white for the single map; grey for no shadow).
//!
//! [`Renderer::describe`] turns the identity buffer into a list of what is
//! on screen: for each entity how many pixels, how much of it is visible, a
//! screen box and the distance to the camera. All of it comes from the
//! renderer's own passes, so what is reported is what is drawn.

use wish_core::math::Vec3;
use wish_core::{Diagnostic, Entity, Result};

use super::draw_list::DrawList;
use super::frame::{depth_key, light_warnings};
use super::pool::Pool;
use super::shadow::{self, CASCADES_UNIFORM_SIZE, ShadowPlan};
use super::{Bytes, DEPTH_FORMAT, Frame, RenderStats, Renderer, choose_camera};
use crate::capture::check_size;
use crate::extract::{CameraView, RenderScene};
use crate::gpu::Gpu;
use crate::image::RgbaImage;
use crate::mesh::VERTEX_STRIDE;

/// Colour debug views are written in a plain (non-sRGB) format, so that a
/// byte in the image is exactly the value the shader computed.
const COLOUR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const ID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
/// Bytes of `Debug` in `debug_pass.wgsl`: a matrix, two vectors and the
/// shadow cascades.
const DEBUG_UNIFORM_SIZE: u64 = 96 + CASCADES_UNIFORM_SIZE;

/// The most entities [`Renderer::describe`] measures a visibility fraction
/// for (each needs a render of its own).
pub const MAX_VISIBILITY_MEASUREMENTS: usize = 128;

/// What to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// The normal, lit picture.
    Lit,
    /// Each entity in its own colour.
    Ids,
    /// Distance from the camera, white near and black far.
    Depth,
    /// Surface normals in world space as colours.
    Normals,
    /// Triangle edges.
    Wireframe,
    /// The wireframe and the physics colliders.
    Colliders,
    /// The wireframe and where the point and spot lights reach.
    Lights,
    /// Which shadow map each surface's shadow is read from: the cascades in
    /// their colours, the single map white, no shadow grey.
    ShadowCascades,
}

impl View {
    /// The names of all views, as `wen capture --view` takes them.
    pub const NAMES: &'static [&'static str] = &[
        "lit",
        "ids",
        "depth",
        "normals",
        "wireframe",
        "colliders",
        "lights",
        "shadow_cascades",
    ];

    /// The view's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Lit => "lit",
            Self::Ids => "ids",
            Self::Depth => "depth",
            Self::Normals => "normals",
            Self::Wireframe => "wireframe",
            Self::Colliders => "colliders",
            Self::Lights => "lights",
            Self::ShadowCascades => "shadow_cascades",
        }
    }

    /// Reads a view name; a wrong one gets a "did you mean" hint.
    pub fn parse(name: &str) -> Result<Self> {
        Ok(match name {
            "lit" => Self::Lit,
            "ids" => Self::Ids,
            "depth" => Self::Depth,
            "normals" => Self::Normals,
            "wireframe" => Self::Wireframe,
            "colliders" => Self::Colliders,
            "lights" => Self::Lights,
            "shadow_cascades" => Self::ShadowCascades,
            _ => {
                return Err(Diagnostic::error(
                    "unknown_view",
                    format!("There is no view called '{name}'."),
                    wish_core::suggest::unknown_name_hint(name, Self::NAMES),
                )
                .with_candidates(Self::NAMES.iter().copied()));
            }
        })
    }
}

/// The pipelines of the debug views.
pub(super) struct DebugPipelines {
    layout: wgpu::BindGroupLayout,
    /// Index 0 culls back faces (single-sided materials), 1 does not.
    ids: [wgpu::RenderPipeline; 2],
    depth: [wgpu::RenderPipeline; 2],
    normals: [wgpu::RenderPipeline; 2],
    fill: [wgpu::RenderPipeline; 2],
    cascades: [wgpu::RenderPipeline; 2],
    lines: wgpu::RenderPipeline,
    /// World-space coloured lines drawn over everything (colliders, lights).
    gizmo: wgpu::RenderPipeline,
}

impl DebugPipelines {
    pub(super) fn new(
        gpu: &Gpu,
        (source, gizmo_source): (&str, &str),
        draw_layout: &wgpu::BindGroupLayout,
        material_layout: &wgpu::BindGroupLayout,
    ) -> Result<Self> {
        gpu.checked("build the debug pipelines", |gpu| {
            let device = &gpu.device;
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("debug"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(DEBUG_UNIFORM_SIZE),
                    },
                    count: None,
                }],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("debug"),
                bind_group_layouts: &[Some(&layout), Some(draw_layout), Some(material_layout)],
                immediate_size: 0,
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("debug_pass"),
                source: wgpu::ShaderSource::Wgsl(source.to_owned().into()),
            });
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
            ];
            let pipeline = |name: &str,
                            fragment: &str,
                            format: wgpu::TextureFormat,
                            topology: wgpu::PrimitiveTopology,
                            compare: wgpu::CompareFunction,
                            bias: wgpu::DepthBiasState,
                            cull_mode: Option<wgpu::Face>| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(name),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: VERTEX_STRIDE,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &attributes,
                        })],
                    },
                    primitive: wgpu::PrimitiveState {
                        topology,
                        cull_mode,
                        ..wgpu::PrimitiveState::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(
                            topology == wgpu::PrimitiveTopology::TriangleList,
                        ),
                        depth_compare: Some(compare),
                        stencil: wgpu::StencilState::default(),
                        bias,
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some(fragment),
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
            let triangles = wgpu::PrimitiveTopology::TriangleList;
            let none = wgpu::DepthBiasState::default();
            // The fill is pushed back a little so that the edges, which lie
            // exactly on the triangles, win the depth test.
            let pushed_back = wgpu::DepthBiasState {
                constant: 4,
                slope_scale: 2.0,
                clamp: 0.0,
            };
            let back = Some(wgpu::Face::Back);
            // One pipeline per culling mode, picked per draw like the lit pass.
            let both = |name: &str,
                        fragment: &str,
                        format: wgpu::TextureFormat,
                        bias: wgpu::DepthBiasState| {
                [
                    pipeline(
                        name,
                        fragment,
                        format,
                        triangles,
                        wgpu::CompareFunction::Less,
                        bias,
                        back,
                    ),
                    pipeline(
                        name,
                        fragment,
                        format,
                        triangles,
                        wgpu::CompareFunction::Less,
                        bias,
                        None,
                    ),
                ]
            };
            let gizmo_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("gizmo"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let gizmo_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("gizmo_pass"),
                source: wgpu::ShaderSource::Wgsl(gizmo_source.to_owned().into()),
            });
            let gizmo = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("gizmo lines"),
                layout: Some(&gizmo_layout),
                vertex: wgpu::VertexState {
                    module: &gizmo_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 28,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 0,
                                shader_location: 0,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 12,
                                shader_location: 1,
                            },
                        ],
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::LineList,
                    ..wgpu::PrimitiveState::default()
                },
                // Over everything: a collider inside a wall is still shown.
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: wgpu::StencilState::default(),
                    bias: none,
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &gizmo_shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOUR_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            Self {
                gizmo,
                ids: both("ids", "fs_ids", ID_FORMAT, none),
                depth: both("depth", "fs_depth", COLOUR_FORMAT, none),
                normals: both("normals", "fs_normals", COLOUR_FORMAT, none),
                fill: both("wire fill", "fs_fill", COLOUR_FORMAT, pushed_back),
                cascades: both("shadow cascades", "fs_cascades", COLOUR_FORMAT, none),
                lines: pipeline(
                    "wire lines",
                    "fs_line",
                    COLOUR_FORMAT,
                    wgpu::PrimitiveTopology::LineList,
                    wgpu::CompareFunction::LessEqual,
                    none,
                    None,
                ),
                layout,
            }
        })
    }
}

/// Which entity is drawn at each pixel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdBuffer {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The camera it was drawn from.
    pub camera: Entity,
    ids: Vec<u32>,
    entities: Vec<Entity>,
}

impl IdBuffer {
    /// The entity drawn at pixel `(x, y)` (from the top left), if any.
    pub fn entity_at(&self, x: u32, y: u32) -> Option<Entity> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let id = *self
            .ids
            .get(y as usize * self.width as usize + x as usize)?;
        self.entities.get((id as usize).checked_sub(1)?).copied()
    }

    /// The raw identities, row by row: 0 for nothing, otherwise the position
    /// of the draw in the scene plus one.
    pub fn raw(&self) -> &[u32] {
        &self.ids
    }

    /// The identities as an image: each draw in its own colour, nothing black.
    pub fn to_image(&self) -> RgbaImage {
        let mut pixels = Vec::with_capacity(self.ids.len() * 4);
        for id in &self.ids {
            pixels.extend_from_slice(&colour_of(*id));
        }
        RgbaImage {
            width: self.width,
            height: self.height,
            pixels,
        }
    }
}

/// A colour for an identity: hues spread by the golden ratio, so that
/// neighbouring identities differ clearly.
fn colour_of(id: u32) -> [u8; 4] {
    if id == 0 {
        return [0, 0, 0, 255];
    }
    let hue = (f64::from(id) * 0.618_033_988_749_895).fract() * 6.0;
    let (saturation, value) = (0.65, 0.95);
    let chroma = value * saturation;
    let x = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (r, g, b) = match hue as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = value - chroma;
    let byte = |channel: f64| ((channel + m) * 255.0).round() as u8;
    [byte(r), byte(g), byte(b), 255]
}

/// One entity that is on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct VisibleEntity {
    /// The entity.
    pub entity: Entity,
    /// How many pixels show it.
    pub pixels: u32,
    /// The share of the entity (as far as it is inside the image) that is not
    /// hidden by other things: visible pixels over the pixels it would cover
    /// on its own. `None` when it was not measured (more than
    /// [`MAX_VISIBILITY_MEASUREMENTS`] entities are visible).
    pub visible_fraction: Option<f32>,
    /// The smallest box around its pixels: `[left, top, right, bottom]`,
    /// pixel coordinates, edges included.
    pub bounding_box: [u32; 4],
    /// Distance from the camera to the centre of the entity's bounding box,
    /// meters.
    pub distance: f32,
    /// Whether some part of it is see-through (blended): what is behind it
    /// shows through, and counts as visible there too.
    pub blended: bool,
}

/// What is on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Description {
    /// The camera it was seen from.
    pub camera: Entity,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Entities with at least one visible pixel, in scene order.
    pub visible: Vec<VisibleEntity>,
    /// Entities that are drawn but have no visible pixel: hidden behind
    /// something, or outside the image.
    pub hidden: Vec<Entity>,
    /// What drawing the lit picture of this view takes, in counts.
    pub stats: RenderStats,
    /// What the lit picture of this view does not draw, and why: the
    /// warnings [`Renderer::render`] gives for the same scene, camera and
    /// size.
    pub warnings: Vec<Diagnostic>,
}

/// What a debug view draws beside the scene's surfaces.
#[derive(Clone, Copy, Default)]
struct Overlay<'a> {
    /// Coloured lines over everything (colliders, where lights reach).
    lines: &'a [crate::gizmo::LineVertex],
    /// For the cascades view: the shadow maps of the lit picture.
    shadow: Option<&'a ShadowPlan>,
    /// Leave the see-through surfaces out (what shows through them, for
    /// `describe`); every view draws them like any surface otherwise.
    solid_only: bool,
}

/// What a debug view's own pass takes to draw: its surfaces, their edges
/// again in the wireframe views, and the lines drawn over them. No shadow
/// map is drawn and no light lights anything. `shadow_cascades` is how many
/// shadow maps the view shows: the lit picture's in the `shadow_cascades`
/// view (it draws none of them itself, so `shadow_casters` stays 0), none
/// in every other view.
fn view_stats(
    size: (u32, u32),
    draws: &DrawList<'_>,
    (wire, lines): (bool, bool),
    shadow_cascades: u32,
) -> RenderStats {
    let surfaces = u32::try_from(draws.items.len()).unwrap_or(u32::MAX);
    RenderStats {
        width: size.0,
        height: size.1,
        draw_calls: surfaces
            .saturating_mul(if wire { 2 } else { 1 })
            .saturating_add(u32::from(lines)),
        triangles: draws.triangles(),
        shadow_cascades,
        blended_surfaces: draws.blended(),
        ..RenderStats::default()
    }
}

impl Renderer {
    /// Draws `scene` as one of the debug (or the lit) views.
    pub fn render_view(
        &self,
        gpu: &Gpu,
        scene: &RenderScene,
        camera: Option<Entity>,
        width: u32,
        height: u32,
        view: View,
    ) -> Result<Frame> {
        if view == View::Lit {
            return self.render(gpu, scene, camera, width, height);
        }
        let camera_view = choose_camera(scene, camera)?;
        check_size(width, height)?;
        let size = (width, height);
        self.pooled(|pool| {
            let posed = self.pose(gpu, scene)?;
            let collected = self.collect(gpu, scene, None, &posed);
            let draws = &collected.draw_list;
            let drawn = draws.items.len();
            let wire = matches!(view, View::Wireframe | View::Colliders | View::Lights);
            // The cascades view shows the shadow maps of the lit picture
            // of this very view, planned the way that picture plans them.
            let shadow = (view == View::ShadowCascades).then(|| {
                shadow::plan(
                    scene,
                    camera_view,
                    width as f32 / height as f32,
                    (&draws.casters, &draws.casts()),
                    gpu.device.limits().max_texture_dimension_2d,
                )
            });
            let (image, lines) = if view == View::Ids {
                let ids = self.id_pixels(gpu, pool, camera_view, draws, size, false)?;
                let image = IdBuffer {
                    width,
                    height,
                    camera: camera_view.entity,
                    ids,
                    entities: scene.draws.iter().map(|draw| draw.entity).collect(),
                }
                .to_image();
                (image, false)
            } else {
                let lines = match view {
                    View::Colliders => crate::gizmo::colliders(scene),
                    View::Lights => crate::gizmo::lights(scene),
                    _ => Vec::new(),
                };
                let overlay = Overlay {
                    lines: &lines,
                    shadow: shadow.as_ref().and_then(|planned| planned.plan.as_ref()),
                    solid_only: false,
                };
                let mut raw =
                    self.debug_pass(gpu, pool, camera_view, draws, size, view, &overlay)?;
                // The target has no alpha of its own that means anything.
                for pixel in raw.pixels.as_chunks_mut::<4>().0 {
                    pixel[3] = 255;
                }
                (raw, !lines.is_empty())
            };
            let mut image = image;
            super::paint_bars(&mut image, camera_view);
            // A debug view lights nothing, but what the scene's lights
            // say is said in every view (an agent looking at its lights
            // through the `lights` view must still hear about a second sun).
            let mut warnings = collected.warnings;
            warnings.extend(light_warnings(scene));
            // The view of the shadow maps says how many it shows (the
            // number the lit picture of this view reports), and why there
            // are none.
            let shown = shadow
                .as_ref()
                .and_then(|planned| planned.plan.as_ref())
                .map_or(0, ShadowPlan::maps);
            warnings.extend(shadow.and_then(|planned| planned.warning));
            Ok(Frame {
                image,
                camera: camera_view.entity,
                drawn,
                stats: view_stats(size, draws, (wire, lines), shown),
                warnings,
            })
        })
    }

    /// Which entity is drawn at each pixel.
    pub fn id_buffer(
        &self,
        gpu: &Gpu,
        scene: &RenderScene,
        camera: Option<Entity>,
        width: u32,
        height: u32,
    ) -> Result<IdBuffer> {
        let camera_view = choose_camera(scene, camera)?;
        check_size(width, height)?;
        self.pooled(|pool| {
            let posed = self.pose(gpu, scene)?;
            let collected = self.collect(gpu, scene, None, &posed);
            let ids = self.id_pixels(
                gpu,
                pool,
                camera_view,
                &collected.draw_list,
                (width, height),
                false,
            )?;
            Ok(IdBuffer {
                width,
                height,
                camera: camera_view.entity,
                ids,
                entities: scene.draws.iter().map(|draw| draw.entity).collect(),
            })
        })
    }

    /// What is on screen: for each visible entity its pixels, how much of it
    /// is visible, a screen box and its distance.
    pub fn describe(
        &self,
        gpu: &Gpu,
        scene: &RenderScene,
        camera: Option<Entity>,
        width: u32,
        height: u32,
    ) -> Result<Description> {
        let camera_view = choose_camera(scene, camera)?;
        check_size(width, height)?;
        self.pooled(|pool| self.describe_with(gpu, pool, scene, camera_view, (width, height)))
    }

    fn describe_with(
        &self,
        gpu: &Gpu,
        pool: &mut Pool,
        scene: &RenderScene,
        camera_view: &CameraView,
        (width, height): (u32, u32),
    ) -> Result<Description> {
        let posed = self.pose(gpu, scene)?;
        // The plan of the lit picture of this view: its draw list is what
        // the identity passes draw, and its statistics and warnings are what
        // the description says about the picture.
        let plan = self.plan(gpu, scene, camera_view, (width, height), &posed);
        let ids = self.id_pixels(gpu, pool, camera_view, &plan.draws, (width, height), false)?;
        // With see-through surfaces, what shows through them: the nearest
        // solid surface at each pixel. An entity is visible at a pixel when
        // it is the nearest surface there or the nearest solid one. Without
        // them the two are the same and only one pass is drawn.
        let behind = if plan.blended.is_empty() {
            None
        } else {
            Some(self.id_pixels(gpu, pool, camera_view, &plan.draws, (width, height), true)?)
        };
        let blended: std::collections::BTreeSet<u32> = plan
            .draws
            .items
            .iter()
            .zip(&plan.draws.casters)
            .filter(|(item, _)| item.blended)
            .map(|(_, (_, _, _, id))| *id)
            .collect();

        // Per identity: pixel count and box.
        let mut stats: std::collections::BTreeMap<u32, (u32, [u32; 4])> =
            std::collections::BTreeMap::new();
        for (index, id) in ids.iter().enumerate() {
            let solid = behind
                .as_ref()
                .and_then(|behind| behind.get(index))
                .copied()
                .filter(|solid| *solid != *id && *solid != 0);
            for id in std::iter::once(*id).chain(solid) {
                if id == 0 {
                    continue;
                }
                let (x, y) = (index as u32 % width, index as u32 / width);
                let entry = stats.entry(id).or_insert((0, [x, y, x, y]));
                entry.0 += 1;
                entry.1 = [
                    entry.1[0].min(x),
                    entry.1[1].min(y),
                    entry.1[2].max(x),
                    entry.1[3].max(y),
                ];
            }
        }

        // Distance to the centre of each entity's world-space bounding box.
        let mut boxes: std::collections::BTreeMap<u32, (Vec3, Vec3)> =
            std::collections::BTreeMap::new();
        for (matrix, low, high, id) in &plan.draws.casters {
            for corner in 0..8u8 {
                let point = matrix.transform_point3(Vec3::new(
                    if corner & 1 == 0 { low.x } else { high.x },
                    if corner & 2 == 0 { low.y } else { high.y },
                    if corner & 4 == 0 { low.z } else { high.z },
                ));
                let entry = boxes.entry(*id).or_insert((point, point));
                entry.0 = entry.0.min(point);
                entry.1 = entry.1.max(point);
            }
        }

        let drawn: std::collections::BTreeSet<u32> = boxes.keys().copied().collect();
        let mut visible = Vec::new();
        let mut measured = 0;
        for (id, (pixels, bounding_box)) in &stats {
            let Some(draw) = scene.draws.get(*id as usize - 1) else {
                continue;
            };
            let visible_fraction = if measured < MAX_VISIBILITY_MEASUREMENTS {
                measured += 1;
                let alone = self.collect(gpu, scene, Some(*id as usize - 1), &posed);
                let full = self
                    .id_pixels(
                        gpu,
                        pool,
                        camera_view,
                        &alone.draw_list,
                        (width, height),
                        false,
                    )?
                    .iter()
                    .filter(|other| **other != 0)
                    .count();
                (full > 0).then(|| (*pixels as f32 / full as f32).min(1.0))
            } else {
                None
            };
            let distance = boxes.get(id).map_or(0.0, |(low, high)| {
                ((*low + *high) * 0.5 - camera_view.position).length()
            });
            visible.push(VisibleEntity {
                entity: draw.entity,
                pixels: *pixels,
                visible_fraction,
                bounding_box: *bounding_box,
                distance,
                blended: blended.contains(id),
            });
        }
        let hidden = drawn
            .iter()
            .filter(|id| !stats.contains_key(id))
            .filter_map(|id| scene.draws.get(*id as usize - 1).map(|draw| draw.entity))
            .collect();
        Ok(Description {
            camera: camera_view.entity,
            width,
            height,
            visible,
            hidden,
            stats: plan.stats,
            warnings: plan.warnings,
        })
    }

    /// The identity of what is drawn at each pixel, as `u32`s. With
    /// `solid_only`, of the nearest solid surface (see-through ones left
    /// out): what shows through them.
    fn id_pixels(
        &self,
        gpu: &Gpu,
        pool: &mut Pool,
        camera: &CameraView,
        draws: &DrawList<'_>,
        size: (u32, u32),
        solid_only: bool,
    ) -> Result<Vec<u32>> {
        let raw = self.debug_pass(
            gpu,
            pool,
            camera,
            draws,
            size,
            View::Ids,
            &Overlay {
                solid_only,
                ..Overlay::default()
            },
        )?;
        let mut ids: Vec<u32> = raw
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32::from_le_bytes(*bytes))
            .collect();
        // A letterboxing pixel camera's bars show nothing.
        if let Some([left, top, right, bottom]) = camera.game_screen(size.0, size.1) {
            for (index, id) in ids.iter_mut().enumerate() {
                let (x, y) = (index as u32 % size.0, index as u32 / size.0);
                if x < left || x >= right || y < top || y >= bottom {
                    *id = 0;
                }
            }
        }
        Ok(ids)
    }

    /// Runs the pass of a debug view and returns the raw bytes it wrote. Its
    /// picture, depth buffer and buffers come from the pool (the depth buffer
    /// and the draws' uniform data are the lit picture's).
    #[allow(clippy::too_many_arguments)]
    fn debug_pass(
        &self,
        gpu: &Gpu,
        pool: &mut Pool,
        camera: &CameraView,
        draws: &DrawList<'_>,
        (width, height): (u32, u32),
        view: View,
        overlay: &Overlay<'_>,
    ) -> Result<RgbaImage> {
        let lines = overlay.lines;
        let wire = matches!(view, View::Wireframe | View::Colliders | View::Lights);
        let line_bytes: Vec<u8> = lines
            .iter()
            .flat_map(|vertex| vertex.iter().flat_map(|value| value.to_le_bytes()))
            .collect();
        // One picture per format, so that the identity buffer and the
        // coloured views do not push each other out of the pool.
        let (name, format) = if view == View::Ids {
            ("ids picture", ID_FORMAT)
        } else {
            ("debug picture", COLOUR_FORMAT)
        };
        let target = pool.picture(gpu, name, (width, height), format)?;
        let view_proj = camera.projection_for(width, height) * camera.view;

        let mut uniform = Bytes::default();
        uniform.mat4(&view_proj);
        uniform.vec4(camera.position.to_array(), 1.0);
        uniform.vec4([camera.near, camera.far, 0.0], 0.0);
        shadow::write_described(overlay.shadow, &mut uniform);

        gpu.checked("draw a debug view", |gpu| {
            let device = &gpu.device;
            let depth = pool.texture(gpu, depth_key(width, height));
            let uniform_buffer = pool.buffer(gpu, "debug", wgpu::BufferUsages::UNIFORM, &uniform.0);
            let debug_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("debug"),
                layout: &self.debug.layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &uniform_buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(DEBUG_UNIFORM_SIZE),
                    }),
                }],
            });
            let draw_group = draws.upload(gpu, pool, &self.pipelines.draw_layout);
            let line_buffer = (!lines.is_empty())
                .then(|| pool.buffer(gpu, "gizmo lines", wgpu::BufferUsages::VERTEX, &line_bytes));
            let clear = match view {
                View::Normals => wgpu::Color {
                    r: 0.2,
                    g: 0.2,
                    b: 0.2,
                    a: 1.0,
                },
                View::Wireframe | View::Colliders | View::Lights => wgpu::Color {
                    r: 0.02,
                    g: 0.02,
                    b: 0.03,
                    a: 1.0,
                },
                _ => wgpu::Color::BLACK,
            };
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("debug view"),
            });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("debug view"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(clear),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
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
                pass.set_bind_group(0, &debug_group, &[]);
                // Each draw with its mesh, its material's textures (for the
                // alpha cut-out) and its culling, exactly as in the lit pass.
                let triangles = match view {
                    View::Ids => &self.debug.ids,
                    View::Depth => &self.debug.depth,
                    View::Normals => &self.debug.normals,
                    View::ShadowCascades => &self.debug.cascades,
                    _ => &self.debug.fill,
                };
                for item in draws
                    .items
                    .iter()
                    .filter(|item| !(overlay.solid_only && item.blended))
                {
                    pass.set_pipeline(&triangles[usize::from(item.double_sided)]);
                    pass.set_bind_group(1, &draw_group, &[item.offset]);
                    pass.set_bind_group(2, item.textures.as_ref(), &[]);
                    pass.set_vertex_buffer(0, item.mesh.vertices.slice(..));
                    pass.set_index_buffer(item.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..item.mesh.index_count, 0, 0..1);
                }
                if wire {
                    pass.set_pipeline(&self.debug.lines);
                    for item in &draws.items {
                        pass.set_bind_group(1, &draw_group, &[item.offset]);
                        pass.set_bind_group(2, item.textures.as_ref(), &[]);
                        pass.set_vertex_buffer(0, item.mesh.vertices.slice(..));
                        pass.set_index_buffer(item.mesh.edges.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..item.mesh.edge_index_count, 0, 0..1);
                    }
                }
                if let Some(line_buffer) = &line_buffer {
                    pass.set_pipeline(&self.debug.gizmo);
                    pass.set_bind_group(0, &debug_group, &[]);
                    // Only this frame's lines: the pooled buffer may be larger.
                    pass.set_vertex_buffer(0, line_buffer.slice(..line_bytes.len() as u64));
                    pass.draw(0..u32::try_from(lines.len()).unwrap_or(0), 0..1);
                }
            }
            target.copy(&mut encoder);
            // Done before the GPU lock is let go: see `Gpu::finish`.
            gpu.finish(encoder)
        })??;
        target.read(gpu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_colours_differ_and_nothing_is_black() {
        assert_eq!(colour_of(0), [0, 0, 0, 255]);
        let colours: std::collections::BTreeSet<[u8; 4]> = (1..=64).map(colour_of).collect();
        assert_eq!(colours.len(), 64, "64 identities, 64 colours");
        assert!((1..=64).all(|id| colour_of(id)[..3].iter().any(|c| *c > 60)));
    }

    #[test]
    fn views_are_read_by_name_with_a_hint() {
        for name in View::NAMES {
            assert_eq!(View::parse(name).unwrap().name(), *name);
        }
        let problem = View::parse("wireframes").unwrap_err();
        assert_eq!(problem.code, "unknown_view");
        assert_eq!(problem.hint, "Did you mean 'wireframe'?");
    }
}
