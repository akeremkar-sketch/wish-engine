//! A lit frame before it is drawn: the *plan* (everything worked out on the
//! CPU: the draw list, the shadow map's fit, the light clusters, the uniform
//! data, the sprite and interface batches, what could not be drawn, and the
//! statistics), and the *resources* (its targets, buffers and bind groups,
//! taken from the pool).
//!
//! The passes in `passes.rs` only record commands over these two; nothing
//! about a frame is decided while it is being drawn.

use wish_core::Diagnostic;
use wish_core::math::Mat4;

use super::bloom::{BloomPlan, BloomTargets};
use super::draw_list::{Collected, DrawList};
use super::look::{Effects, GRADE_UNIFORM_SIZE, Look};
use super::models::Posed;
use super::pipelines::{LIGHT_UNIFORM_SIZE, Pipelines};
use super::pool::{Pool, TextureKey};
use super::shadow::{self, ShadowPlan};
use super::{
    Bytes, DEPTH_FORMAT, FRAME_UNIFORM_SIZE, HDR_FORMAT, LIGHT_STRIDE, Renderer, SHADOW_FORMAT,
    exposure_multiplier,
};
use crate::capture::CAPTURE_FORMAT;
use crate::cluster::{self, Clusters};
use crate::components::EMISSIVE_NITS;
use crate::extract::{CameraView, LocalLightView, RenderScene};
use crate::gpu::Gpu;

/// What drawing a picture takes, in counts. They depend only on the scene,
/// the camera and the picture's size, never on the machine or the clock, so
/// they can be compared between runs and asserted in tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// The picture's width in pixels.
    pub width: u32,
    /// The picture's height in pixels.
    pub height: u32,
    /// Draw calls of the frame, all passes together (the shadow map, the
    /// sky, the surfaces, bloom, tone mapping, FXAA, sprites and particles,
    /// the interface).
    pub draw_calls: u32,
    /// Triangles those draw calls hand to the GPU (before it culls any).
    pub triangles: u64,
    /// Draws into the shadow maps, all of them together; 0 when no shadow
    /// map is drawn. A cascade draws only the surfaces that can shadow it,
    /// so a surface counts once for each cascade it is drawn into.
    pub shadow_casters: u32,
    /// How many shadow maps the picture has: 0 without shadows, 1 for the
    /// one map around everything, 2 to 4 for cascades
    /// (`RenderSettings.shadow_cascades`). The `shadow_cascades` view
    /// reports the number of the lit picture it shows the maps of; every
    /// other debug view reports 0 (it draws and shows none).
    pub shadow_cascades: u32,
    /// Point lights that light some part of the view.
    pub point_lights: u32,
    /// Spot lights that light some part of the view.
    pub spot_lights: u32,
    /// See-through surfaces (a material that blends: a material file or a
    /// glTF material with alpha mode `Blend`, or a plain `Material` with
    /// alpha below 1); a model counts each such part. The same number in
    /// every view: the lit picture draws them back to front over the rest
    /// (a double-sided one as two draw calls, back faces first), the debug
    /// views like any surface.
    pub blended_surfaces: u32,
    /// Which optional effects the lit picture used: the sky's light, the
    /// sun's disc, fog, bloom, colour grading, FXAA.
    pub effects: Effects,
}

/// Everything a lit frame draws, worked out before any GPU call.
pub(super) struct FramePlan<'a> {
    /// The picture's width and height in pixels.
    pub size: (u32, u32),
    /// The surfaces, in scene order.
    pub draws: DrawList<'a>,
    /// The see-through surfaces of `draws`, as positions in it, in the order
    /// the blended pass draws them (farthest first).
    pub blended: Vec<u32>,
    /// The shadow maps to draw, if any: one, or cascades.
    pub shadow: Option<ShadowPlan>,
    /// The frame uniform (`Frame` in `common.wgsl`).
    pub frame_uniform: Vec<u8>,
    /// The light's matrix for each shadow map, for the shadow pass.
    pub light_uniform: Vec<u8>,
    /// Whether the sky pass draws a sun disc or fog with the gradient.
    pub sky_look: bool,
    /// For a frame with bloom, its levels and numbers.
    pub bloom: Option<BloomPlan>,
    /// For a graded frame, the colour grading (`Grade` in
    /// `tonemap_pass.wgsl`).
    pub grade_uniform: Option<Vec<u8>>,
    /// Whether the FXAA pass runs.
    pub fxaa: bool,
    /// The point and spot lights (`LocalLight` in `lights.wgsl`).
    pub local_lights: Vec<u8>,
    /// For each cluster, where its lights start and how many there are.
    pub cluster_ranges: Vec<u8>,
    /// The clusters' light indices.
    pub cluster_lights: Vec<u8>,
    /// Sprites and particles, if the scene has any.
    pub overlay: Option<crate::sprites::Overlay>,
    /// The interface, if the scene has one.
    pub interface: Option<crate::ui::Batch>,
    /// Things that are not drawn, and why.
    pub warnings: Vec<Diagnostic>,
    /// What the frame takes to draw.
    pub stats: RenderStats,
}

/// The point and spot lights as the shader reads them (`LocalLight`).
fn light_bytes(lights: &[LocalLightView]) -> Vec<u8> {
    let mut bytes = Bytes::default();
    for light in lights.iter().take(cluster::MAX_LIGHTS) {
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

/// How many point lights and how many spot lights light some part of the
/// view: the ones at least one cluster lists.
fn lights_in_view(lights: &[LocalLightView], clusters: &Clusters) -> (u32, u32) {
    let mut listed = vec![false; lights.len().min(cluster::MAX_LIGHTS)];
    for index in &clusters.indices {
        if let Some(slot) = listed.get_mut(*index as usize) {
            *slot = true;
        }
    }
    let (mut points, mut spots) = (0, 0);
    for (light, _) in lights.iter().zip(&listed).filter(|(_, listed)| **listed) {
        if light.spot.is_some() {
            spots += 1;
        } else {
            points += 1;
        }
    }
    (points, spots)
}

/// The entities of `lights` by name, for a message: at most four, and how
/// many more there are.
fn named(scene: &RenderScene, lights: &[crate::extract::LightView]) -> String {
    const LISTED: usize = 4;
    let names: Vec<String> = lights
        .iter()
        .take(LISTED)
        .map(|light| scene.name_of(light.entity))
        .collect();
    let more = lights.len().saturating_sub(LISTED);
    let list = names.join(", ");
    match (lights.len(), more) {
        (1, _) => format!("entity {list}"),
        (_, 0) => format!("entities {list}"),
        _ => format!("entities {list} and {more} more"),
    }
}

/// The warning `ambient_color_unused`: `Ambient.<field>` is set and the
/// ambient light comes from the sky, which does not use it. `check` gives
/// the same warning from the scene file (`wish_asset::project`).
fn ambient_unused_warning(field: &str) -> Diagnostic {
    Diagnostic::warning(
        "ambient_color_unused",
        format!(
            "Ambient.{field} is set, but the ambient light comes from the sky (Ambient.source is Sky, the default since scene format 2), which does not use Ambient.color or Ambient.intensity: the setting changes nothing."
        ),
        "For a flat ambient light of that colour and intensity, set Ambient.source to \"Color\" (or apply the look plain, which turns every look effect off). To keep the light from the sky, remove Ambient.color and Ambient.intensity, and use Ambient.sky_strength to make it stronger or weaker.",
    )
}

/// What the scene's lights say that a picture of it cannot show, whatever
/// the camera and the view: more point and spot lights than can be drawn,
/// and directional lights beside the one that is used. Every view reports
/// them (the lit picture, the debug views and `describe`), so that whichever
/// way a scene is looked at says that one of its suns lights nothing.
pub(super) fn light_warnings(scene: &RenderScene) -> Vec<Diagnostic> {
    let mut warnings = Vec::new();
    if scene.local_lights.len() > cluster::MAX_LIGHTS {
        warnings.push(Diagnostic::warning(
            "too_many_lights",
            format!(
                "The scene has {} point and spot lights; only the first {} are drawn.",
                scene.local_lights.len(),
                cluster::MAX_LIGHTS
            ),
            "Remove lights that add little, or give them a smaller range.",
        ));
    }
    if let Some((used, extra)) = scene.lights.split_first()
        && !extra.is_empty()
    {
        warnings.push(Diagnostic::warning(
            "extra_directional_lights",
            format!(
                "The scene has {} directional lights and only one lights the picture: entity {}, the first in entity order. Not drawn: {}.",
                scene.lights.len(),
                scene.name_of(used.entity),
                named(scene, extra)
            ),
            "Keep one DirectionalLight: remove the ones that are not drawn, or remove the one that is and the next takes its place (a scene file's entities are ordered parents before their children, siblings by `order`, then by id; a prefab's entities follow their instance). For a second source of light use a PointLight or a SpotLight.",
        ));
    }
    warnings
}

/// What the lit picture of these parts takes to draw. Each line is one pass
/// of `passes.rs` and counts exactly what that pass draws. `effects` comes
/// with the number of passes the bloom is (0 without bloom).
fn lit_stats(
    size: (u32, u32),
    draws: &DrawList<'_>,
    shadow: Option<&ShadowPlan>,
    lights: (u32, u32),
    (effects, bloom_passes): (Effects, u32),
    overlay: Option<&crate::sprites::Overlay>,
    interface: Option<&crate::ui::Batch>,
) -> RenderStats {
    // The opaque pass draws the solid surfaces once each; the blended pass
    // draws each see-through one once, or twice when it is double-sided
    // (its back faces, then its front faces).
    let solid = || draws.items.iter().filter(|item| !item.blended);
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let solid_count = count(solid().count());
    let solid_triangles: u64 = solid().map(|item| item.mesh.triangles()).sum();
    let (blended_calls, blended_triangles) = draws.items.iter().filter(|item| item.blended).fold(
        (0u32, 0u64),
        |(calls, triangles), item| {
            let times = 1 + u32::from(item.double_sided);
            (
                calls.saturating_add(times),
                triangles + item.mesh.triangles() * u64::from(times),
            )
        },
    );
    let surfaces = solid_count.saturating_add(blended_calls);
    let surface_triangles = solid_triangles + blended_triangles;
    // The single map holds every solid surface; a cascade the solid ones
    // that can shadow it. See-through surfaces cast no shadow.
    let (shadow_casters, shadow_triangles) = match shadow {
        None => (0, 0),
        Some(ShadowPlan::Single { .. }) => (solid_count, solid_triangles),
        Some(ShadowPlan::Cascades(cascades)) => {
            let drawn = || {
                cascades
                    .cascades
                    .iter()
                    .flat_map(|cascade| cascade.draws.iter())
                    .filter_map(|index| draws.items.get(*index as usize))
                    .filter(|item| !item.blended)
            };
            (
                u32::try_from(drawn().count()).unwrap_or(u32::MAX),
                drawn().map(|item| item.mesh.triangles()).sum(),
            )
        }
    };
    let (overlay_draws, overlay_triangles) = overlay.map_or((0, 0), |overlay| overlay.counts());
    let (interface_draws, interface_triangles) = interface.map_or((0, 0), |batch| batch.counts());
    // The sky, tone mapping, FXAA and each pass of the bloom are one
    // triangle each.
    let fullscreen = 2 + u32::from(effects.fxaa) + bloom_passes;
    RenderStats {
        width: size.0,
        height: size.1,
        draw_calls: shadow_casters
            .saturating_add(surfaces)
            .saturating_add(fullscreen)
            .saturating_add(overlay_draws)
            .saturating_add(interface_draws),
        triangles: shadow_triangles
            + surface_triangles
            + u64::from(fullscreen)
            + overlay_triangles
            + interface_triangles,
        shadow_casters,
        shadow_cascades: shadow.map_or(0, ShadowPlan::maps),
        point_lights: lights.0,
        spot_lights: lights.1,
        blended_surfaces: draws.blended(),
        effects,
    }
}

impl Renderer {
    /// Plans the lit picture of `scene` seen through `view` at
    /// `width` × `height`. `posed` are the scene's animated models at their
    /// clips' times ([`Renderer::pose`]).
    pub(super) fn plan<'a>(
        &'a self,
        gpu: &Gpu,
        scene: &RenderScene,
        view: &CameraView,
        (width, height): (u32, u32),
        posed: &'a Posed,
    ) -> FramePlan<'a> {
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
            draw_list: draws,
            mut warnings,
        } = self.collect(gpu, scene, None, posed);
        // Sprites and particles, then the interface, are prepared on the CPU
        // and drawn over the finished picture.
        let overlay = crate::sprites::build(
            scene,
            view,
            aspect,
            (&self.ui_pictures, &self.sprite_sheets),
            &mut warnings,
        );
        let interface = (!scene.ui.is_empty())
            .then(|| crate::ui::build(scene, width, height, &self.ui_pictures));
        if let Some(batch) = &interface {
            warnings.extend(batch.warnings.iter().cloned());
        }

        // One shadow map is fitted around everything drawn; cascades are
        // fitted to slices of this view. Without either (shadows off, no
        // light, nothing to cover) the shader is told so and lights
        // everything. With cascades the single map's numbers say "no map":
        // the shader asks the cascades instead (the end of the uniform).
        let device_limit = gpu.device.limits().max_texture_dimension_2d;
        let shadow::Planned {
            plan: shadow,
            warning: shadow_warning,
        } = shadow::plan(
            scene,
            view,
            aspect,
            (&draws.casters, &draws.casts()),
            device_limit,
        );
        warnings.extend(shadow_warning);
        let (fit, map_size) = match &shadow {
            Some(ShadowPlan::Single { fit, map_size }) => (Some(*fit), *map_size),
            _ => (None, shadow::map_size(scene, device_limit)),
        };
        let light_view_proj = fit.map_or(Mat4::IDENTITY, |fit| fit.view_proj);
        frame.mat4(&light_view_proj);
        frame.floats(&[
            if fit.is_some() { 1.0 } else { 0.0 },
            fit.map_or(0.0, |fit| fit.depth_bias),
            fit.map_or(0.0, |fit| fit.normal_offset),
            1.0 / map_size as f32,
        ]);
        let clusters = cluster::build(
            &scene.local_lights,
            &view.view,
            &view.projection(aspect),
            (width, height),
            (view.near, view.far),
        );
        let lights = scene.local_lights.len().min(cluster::MAX_LIGHTS);
        warnings.extend(light_warnings(scene));
        if clusters.dropped > 0 {
            warnings.push(Diagnostic::warning(
                "light_clusters_full",
                format!(
                    "Too many lights reach the same part of the picture: {} light placements were left out, so some areas are darker than they should be.",
                    clusters.dropped
                ),
                format!(
                    "At most {} lights can light one region; spread lights out or give them a smaller range.",
                    cluster::MAX_PER_CLUSTER
                ),
            ));
        }
        frame.mat4(&view.view);
        frame.floats(&[
            clusters.tiles_x as f32,
            clusters.tiles_y as f32,
            cluster::SLICES as f32,
            lights as f32,
            clusters.near,
            view.far,
            clusters.log_scale,
            cluster::TILE_PX as f32,
        ]);
        let orthographic = view.ortho_height.is_some();
        let look = Look::of(scene, exposure, orthographic);
        look.write_frame(&mut frame, orthographic);
        shadow::write_cascades(shadow.as_ref(), &mut frame);
        frame.pad_to(FRAME_UNIFORM_SIZE);
        // A disc is drawn by default, so a scene without a sun is no mistake
        // by itself (an interior, a 2D scene, a menu): only a size someone
        // changed says that a disc was wanted. An orthographic camera never
        // shows one, so nothing is missing there.
        let sun_size = scene.sky.sun_size_deg;
        let asked = sun_size != crate::components::Sky::default().sun_size_deg;
        if scene.lights.is_empty()
            && sun_size.is_finite()
            && sun_size > 0.0
            && asked
            && !orthographic
        {
            warnings.push(Diagnostic::warning(
                "sun_disc_without_light",
                format!(
                    "The sky asks for a sun disc (Sky.sun_size_deg is {sun_size}) and the scene has no DirectionalLight, so no disc is drawn: the disc sits where that light shines from and has its colour."
                ),
                "Add an entity with a DirectionalLight (its Transform's rotation says where the sun stands), or set Sky.sun_size_deg to 0.",
            ));
        }
        // The sky lights the scene by default: a colour or an intensity
        // written for the flat ambient light then does nothing.
        if let Some(field) = scene.ambient_unused {
            warnings.push(ambient_unused_warning(field));
        }

        let cluster_ranges: Vec<u8> = clusters
            .ranges
            .iter()
            .flat_map(|[start, count]| [start.to_le_bytes(), count.to_le_bytes()])
            .flatten()
            .collect();
        let cluster_lights: Vec<u8> = if clusters.indices.is_empty() {
            vec![0; 4]
        } else {
            clusters
                .indices
                .iter()
                .flat_map(|i| i.to_le_bytes())
                .collect()
        };
        let light_uniform = match &shadow {
            Some(shadow) => shadow.light_uniform(),
            None => {
                let mut none = Bytes::default();
                none.mat4(&Mat4::IDENTITY);
                none.0
            }
        };

        let bloom = look.bloom_plan((width, height));
        let stats = lit_stats(
            (width, height),
            &draws,
            shadow.as_ref(),
            lights_in_view(&scene.local_lights, &clusters),
            (look.effects(), bloom.as_ref().map_or(0, BloomPlan::passes)),
            overlay.as_ref(),
            interface.as_ref(),
        );
        let blended = draws.back_to_front(&view.view);
        FramePlan {
            size: (width, height),
            draws,
            blended,
            shadow,
            frame_uniform: frame.0,
            light_uniform,
            sky_look: look.sky(),
            bloom,
            grade_uniform: look.grade_uniform((width, height)),
            fxaa: look.fxaa,
            local_lights: light_bytes(&scene.local_lights),
            cluster_ranges,
            cluster_lights,
            overlay,
            interface,
            warnings,
            stats,
        }
    }
}

/// The GPU side of a lit frame: its targets and buffers from the pool, and
/// the bind groups over them, which live for this frame only (see `pool.rs`
/// for why they are not kept).
pub(super) struct FrameResources {
    /// The picture before tone mapping, pre-exposed.
    pub hdr: wgpu::TextureView,
    /// The picture's depth buffer.
    pub depth: wgpu::TextureView,
    /// The light's depth picture: the one shadow map, the atlas of the
    /// cascades, or a 1x1 map when no shadow map is drawn.
    pub shadow_map: wgpu::TextureView,
    /// Group 0 of the sky and opaque passes.
    pub frame_group: wgpu::BindGroup,
    /// Group 0 of the shadow pass.
    pub light_group: wgpu::BindGroup,
    /// One draw's uniform data, picked by its offset.
    pub draw_group: wgpu::BindGroup,
    /// With bloom: its levels, and the picture with its bloom that tone
    /// mapping reads in place of the HDR picture.
    pub bloom: Option<BloomTargets>,
    /// The HDR picture (with bloom: the bloomed picture), for tone mapping;
    /// in a graded frame with the grading's numbers.
    pub tonemap_group: wgpu::BindGroup,
    /// With FXAA: the tone-mapped picture that tone mapping writes instead
    /// of the picture itself, and the group the FXAA pass reads it through.
    pub fxaa: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

/// A uniform buffer bound at its struct's size, whatever the pooled buffer's
/// capacity.
fn sized(buffer: &wgpu::Buffer, size: u64) -> wgpu::BindingResource<'_> {
    wgpu::BindingResource::Buffer(wgpu::BufferBinding {
        buffer,
        offset: 0,
        size: wgpu::BufferSize::new(size),
    })
}

impl FrameResources {
    /// Takes the targets from the pool, fills the buffers with the plan's
    /// data and builds this frame's bind groups. Call inside `Gpu::checked`.
    pub(super) fn acquire(
        gpu: &Gpu,
        pool: &mut Pool,
        pipelines: &Pipelines,
        plan: &FramePlan<'_>,
    ) -> Self {
        let device = &gpu.device;
        let (width, height) = plan.size;
        let hdr = pool.texture(
            gpu,
            TextureKey {
                name: "hdr",
                width,
                height,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
            },
        );
        let depth = pool.texture(gpu, depth_key(width, height));
        // A frame without shadows binds a 1x1 map of its own, so that it
        // does not make the real map come and go; the one map and the
        // cascades' atlas have names of their own for the same reason.
        let (name, map_width, map_height) = match &plan.shadow {
            Some(shadow) => shadow.texture(),
            None => ("no shadow", 1, 1),
        };
        let shadow_map = pool.texture(
            gpu,
            TextureKey {
                name,
                width: map_width,
                height: map_height,
                format: SHADOW_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
            },
        );

        let uniform = wgpu::BufferUsages::UNIFORM;
        let storage = wgpu::BufferUsages::STORAGE;
        let frame = pool.buffer(gpu, "frame", uniform, &plan.frame_uniform);
        let lights = pool.buffer(gpu, "local lights", storage, &plan.local_lights);
        let ranges = pool.buffer(gpu, "cluster ranges", storage, &plan.cluster_ranges);
        let indices = pool.buffer(gpu, "cluster lights", storage, &plan.cluster_lights);
        let light = pool.buffer(gpu, "light", uniform, &plan.light_uniform);

        let light_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("light"),
            layout: &pipelines.light_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: sized(&light, LIGHT_UNIFORM_SIZE),
            }],
        });
        // The light lists are bound whole: the shader reads only what this
        // frame's counts name, never what an earlier frame left behind them.
        let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &pipelines.frame_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: sized(&frame, FRAME_UNIFORM_SIZE),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&shadow_map),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&pipelines.shadow_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: lights.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: ranges.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: indices.as_entire_binding(),
                },
            ],
        });
        let draw_group = plan.draws.upload(gpu, pool, &pipelines.draw_layout);
        // Only a frame with bloom has the bloom's targets. Tone mapping then
        // reads the picture the bloom's last step writes; without bloom it
        // reads the HDR picture, as it always did.
        let bloom = plan
            .bloom
            .as_ref()
            .map(|bloom| BloomTargets::acquire(gpu, pool, pipelines, bloom, &hdr));
        let lit = bloom.as_ref().map_or(&hdr, |bloom| &bloom.bloomed);
        // Only a graded frame has the grading's buffer, and the group that
        // binds it.
        let hdr_entry = wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(lit),
        };
        let tonemap_group = match &plan.grade_uniform {
            Some(numbers) => {
                let grade = pool.buffer(gpu, "grade", uniform, numbers);
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("grade"),
                    layout: &pipelines.grade_layout,
                    entries: &[
                        hdr_entry,
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: sized(&grade, GRADE_UNIFORM_SIZE),
                        },
                    ],
                })
            }
            None => device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("hdr"),
                layout: &pipelines.hdr_layout,
                entries: &[hdr_entry],
            }),
        };
        // The picture between tone mapping and FXAA: only a frame that
        // smooths its edges has one.
        let fxaa = plan.fxaa.then(|| {
            let tone_mapped = pool.texture(
                gpu,
                TextureKey {
                    name: "tone mapped",
                    width,
                    height,
                    format: CAPTURE_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                },
            );
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("tone mapped"),
                layout: &pipelines.hdr_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&tone_mapped),
                }],
            });
            (tone_mapped, group)
        });
        Self {
            hdr,
            depth,
            shadow_map,
            frame_group,
            light_group,
            draw_group,
            bloom,
            tonemap_group,
            fxaa,
        }
    }
}

/// The depth buffer of a `width` × `height` picture: the lit picture and the
/// debug views share it.
pub(super) fn depth_key(width: u32, height: u32) -> TextureKey {
    TextureKey {
        name: "depth",
        width,
        height,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
    }
}
