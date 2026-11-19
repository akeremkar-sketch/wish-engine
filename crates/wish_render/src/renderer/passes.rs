//! The passes of a lit frame, in the order `Renderer::render` runs them.
//!
//! | Pass | Reads | Writes |
//! |---|---|---|
//! | [`shadow`] | the draw list, the light's matrix for each map | the shadow map, or the atlas of the cascades (all of it) |
//! | [`sky`] | the frame uniform | the HDR picture (all of it) |
//! | [`opaque`] | the draw list (its solid surfaces), the frame uniform, the shadow map, the light lists | the HDR picture, the depth buffer |
//! | [`blended`] (only when the frame has see-through surfaces) | the draw list's see-through surfaces back to front, the frame uniform, the shadow map, the light lists, the depth buffer | the HDR picture |
//! | the bloom's passes (only when asked for; `bloom.rs`) | the HDR picture | the bloom's levels, the bloomed picture (all of it) |
//! | [`tone_map`] | the HDR picture (with bloom: the bloomed picture), the colour grading | the picture, or with FXAA the tone-mapped picture (all of it) |
//! | [`fxaa`] (only when asked for) | the tone-mapped picture | the picture (all of it) |
//! | [`overlay`] | sprites and particles, the depth buffer | the picture |
//! | [`interface`] | the interface batch | the picture |
//! | [`copy_out`] and [`readback`] | the picture | an [`RgbaImage`] on the CPU |
//!
//! A pass is one function. It only records commands: what it reads and what
//! it writes are its parameters, taken from the frame's plan and resources
//! (`frame.rs`), and nothing is created or decided here. (The sprite and
//! interface passes take their atlas texture and their buffers from the pool
//! themselves and fill them: what is in the atlas is rebuilt on the CPU
//! every frame until sprites are kept on the GPU.)
//!
//! A new effect is a new function here (or a file of its own when it grows),
//! a target from the pool, and one line in `Renderer::render`. One rule
//! comes from the software adapter (`pool.rs`, `warp_probe.rs`): a frame
//! must not hold two bind groups of one layout over different uniform
//! buffers, unless the buffer is bound with a dynamic offset.

use wish_core::Result;

use super::draw_list::{DrawItem, DrawList};
use super::pool::Pool;
use super::shadow::{LIGHT_UNIFORM_STRIDE, ShadowPlan};
use crate::capture::Target;
use crate::gpu::Gpu;
use crate::image::RgbaImage;

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

/// One shadow map of a frame, as the shadow pass draws it.
pub(super) struct ShadowMap<'a> {
    /// The part of the texture it is drawn into: left, top and side, in
    /// texels (a cascade's tile of the atlas). `None` for the single map,
    /// which is the whole texture.
    pub area: Option<[u32; 3]>,
    /// Where its matrix is in the light uniform.
    pub light_offset: u32,
    /// The draws that go into it, as positions in the draw list; `None`
    /// for all of them.
    pub draws: Option<&'a [u32]>,
}

/// The shadow maps of a frame with this plan, in the order they are drawn:
/// the single map, each cascade, or none.
pub(super) fn shadow_maps(plan: Option<&ShadowPlan>) -> Vec<ShadowMap<'_>> {
    match plan {
        None => Vec::new(),
        Some(ShadowPlan::Single { .. }) => vec![ShadowMap {
            area: None,
            light_offset: 0,
            draws: None,
        }],
        Some(ShadowPlan::Cascades(cascades)) => cascades
            .cascades
            .iter()
            .enumerate()
            .map(|(index, cascade)| ShadowMap {
                area: Some(cascades.area(index)),
                light_offset: u32::try_from(index as u64 * LIGHT_UNIFORM_STRIDE)
                    .unwrap_or(u32::MAX),
                draws: Some(&cascade.draws),
            })
            .collect(),
    }
}

/// What the shadow pass reads and writes.
pub(super) struct Shadow<'a> {
    /// Written: the light's depth picture (for cascades: their atlas),
    /// cleared to "nothing in the way".
    pub map: &'a wgpu::TextureView,
    /// The depth-only pipeline of solid surfaces.
    pub solid: &'a wgpu::RenderPipeline,
    /// The pipeline of alpha-masked surfaces: it leaves out what the
    /// material cuts out.
    pub masked: &'a wgpu::RenderPipeline,
    /// The lights' matrices (group 0, one for each map, by offset).
    pub light: &'a wgpu::BindGroup,
    /// The draws' uniform data (group 1, by offset).
    pub draw_uniforms: &'a wgpu::BindGroup,
    /// What casts shadows.
    pub casters: &'a DrawList<'a>,
    /// The maps to draw; empty when the frame has no shadows (the pass then
    /// only clears the 1x1 map the shader is given).
    pub maps: &'a [ShadowMap<'a>],
}

/// The shadow pass: every surface's depth as the light sees it.
pub(super) fn shadow(encoder: &mut wgpu::CommandEncoder, inputs: &Shadow<'_>) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("shadow"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: inputs.map,
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
    for map in inputs.maps {
        // A cascade is drawn into its own tile and nowhere else.
        if let Some([left, top, side]) = map.area {
            pass.set_viewport(left as f32, top as f32, side as f32, side as f32, 0.0, 1.0);
            pass.set_scissor_rect(left, top, side, side);
        }
        // Solid surfaces go through the solid pipeline exactly as they
        // always did; the pipeline changes only where the list changes kind.
        let mut current = None;
        let mut draw = |item: &DrawItem<'_>| {
            if current != Some(item.masked) {
                pass.set_pipeline(if item.masked {
                    inputs.masked
                } else {
                    inputs.solid
                });
                pass.set_bind_group(0, inputs.light, &[map.light_offset]);
                current = Some(item.masked);
            }
            pass.set_bind_group(1, inputs.draw_uniforms, &[item.offset]);
            if item.masked {
                pass.set_bind_group(2, item.textures.as_ref(), &[]);
            }
            pass.set_vertex_buffer(0, item.mesh.vertices.slice(..));
            pass.set_index_buffer(item.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..item.mesh.index_count, 0, 0..1);
        };
        // See-through surfaces cast no shadow.
        match map.draws {
            None => inputs
                .casters
                .items
                .iter()
                .filter(|item| !item.blended)
                .for_each(&mut draw),
            Some(draws) => draws
                .iter()
                .filter_map(|index| inputs.casters.items.get(*index as usize))
                .filter(|item| !item.blended)
                .for_each(&mut draw),
        }
    }
}

/// The sky pass: fills the HDR picture with the sky, replacing whatever the
/// pooled target held. `pipeline` is the plain sky's, or the one that also
/// draws the sun's disc and the fog.
pub(super) fn sky(
    encoder: &mut wgpu::CommandEncoder,
    hdr: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    frame: &wgpu::BindGroup,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("sky"),
        color_attachments: &[colour(hdr, wgpu::LoadOp::Clear(wgpu::Color::BLACK))],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, frame, &[]);
    pass.draw(0..3, 0..1);
}

/// What the opaque pass reads and writes.
pub(super) struct Opaque<'a> {
    /// Written over the sky.
    pub hdr: &'a wgpu::TextureView,
    /// Written: cleared, then kept for the sprites and particles.
    pub depth: &'a wgpu::TextureView,
    /// The pipeline that culls back faces.
    pub single_sided: &'a wgpu::RenderPipeline,
    /// The pipeline of double-sided materials.
    pub double_sided: &'a wgpu::RenderPipeline,
    /// The frame uniform, the shadow map and the light lists (group 0).
    pub frame: &'a wgpu::BindGroup,
    /// The draws' uniform data (group 1, by offset).
    pub draw_uniforms: &'a wgpu::BindGroup,
    /// What to draw.
    pub draws: &'a DrawList<'a>,
}

/// The opaque pass: every solid surface, lit, with a depth buffer. The
/// see-through ones are left to [`blended`].
pub(super) fn opaque(encoder: &mut wgpu::CommandEncoder, inputs: &Opaque<'_>) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("opaque"),
        color_attachments: &[colour(inputs.hdr, wgpu::LoadOp::Load)],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: inputs.depth,
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
    pass.set_bind_group(0, inputs.frame, &[]);
    for item in inputs.draws.items.iter().filter(|item| !item.blended) {
        pass.set_pipeline(if item.double_sided {
            inputs.double_sided
        } else {
            inputs.single_sided
        });
        pass.set_bind_group(1, inputs.draw_uniforms, &[item.offset]);
        pass.set_bind_group(2, item.textures.as_ref(), &[]);
        pass.set_vertex_buffer(0, item.mesh.vertices.slice(..));
        pass.set_index_buffer(item.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..item.mesh.index_count, 0, 0..1);
    }
}

/// What the blended pass reads and writes.
pub(super) struct Blended<'a> {
    /// Blended over: the sky and the solid surfaces.
    pub hdr: &'a wgpu::TextureView,
    /// The solid surfaces' depth: tested, never written, and kept for the
    /// sprites and particles.
    pub depth: &'a wgpu::TextureView,
    /// The pipeline of front faces (back faces culled).
    pub front_faces: &'a wgpu::RenderPipeline,
    /// The pipeline of back faces (front faces culled), for double-sided
    /// surfaces.
    pub back_faces: &'a wgpu::RenderPipeline,
    /// The frame uniform, the shadow map and the light lists (group 0).
    pub frame: &'a wgpu::BindGroup,
    /// The draws' uniform data (group 1, by offset).
    pub draw_uniforms: &'a wgpu::BindGroup,
    /// The draw list.
    pub draws: &'a DrawList<'a>,
    /// Its see-through surfaces, farthest first.
    pub order: &'a [u32],
}

/// The blended pass: the see-through surfaces, lit exactly as solid ones
/// are (the same shader functions), each blended over what is already in
/// the HDR picture by its alpha, farthest first. A double-sided surface is
/// drawn twice, its back faces first, so a closed see-through shape shows
/// its far side behind its near side. Run only when the frame has such a
/// surface.
pub(super) fn blended(encoder: &mut wgpu::CommandEncoder, inputs: &Blended<'_>) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("blended"),
        color_attachments: &[colour(inputs.hdr, wgpu::LoadOp::Load)],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: inputs.depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_bind_group(0, inputs.frame, &[]);
    for item in inputs
        .order
        .iter()
        .filter_map(|index| inputs.draws.items.get(*index as usize))
    {
        pass.set_bind_group(1, inputs.draw_uniforms, &[item.offset]);
        pass.set_bind_group(2, item.textures.as_ref(), &[]);
        pass.set_vertex_buffer(0, item.mesh.vertices.slice(..));
        pass.set_index_buffer(item.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        if item.double_sided {
            pass.set_pipeline(inputs.back_faces);
            pass.draw_indexed(0..item.mesh.index_count, 0, 0..1);
        }
        pass.set_pipeline(inputs.front_faces);
        pass.draw_indexed(0..item.mesh.index_count, 0, 0..1);
    }
}

/// The tone mapping pass: from the HDR picture to the displayable one,
/// replacing whatever the pooled picture held. For a graded frame `pipeline`
/// is the grading one and `hdr` carries the grading's numbers with the HDR
/// picture. In a frame with bloom `hdr` binds the bloomed picture.
pub(super) fn tone_map(
    encoder: &mut wgpu::CommandEncoder,
    picture: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    hdr: &wgpu::BindGroup,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("tonemap"),
        color_attachments: &[colour(picture, wgpu::LoadOp::Clear(wgpu::Color::BLACK))],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, hdr, &[]);
    pass.draw(0..3, 0..1);
}

/// The FXAA pass: the tone-mapped picture with its edges smoothed, replacing
/// whatever the pooled picture held. It runs before the sprites, the
/// particles and the interface are drawn: those are made of textures and
/// soft shapes that have no jagged edges to mend, and smoothing them would
/// only blur pixel art and text.
pub(super) fn fxaa(
    encoder: &mut wgpu::CommandEncoder,
    picture: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    tone_mapped: &wgpu::BindGroup,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("fxaa"),
        color_attachments: &[colour(picture, wgpu::LoadOp::Clear(wgpu::Color::BLACK))],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, tone_mapped, &[]);
    pass.draw(0..3, 0..1);
}

/// The overlay pass: sprites and particles over the finished picture, hidden
/// by what is in front of them in `depth`. Its atlas and buffers come from
/// the pool. Call inside `Gpu::checked`.
pub(super) fn overlay(
    (gpu, pool): (&Gpu, &mut Pool),
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &crate::sprites::OverlayPipeline,
    (picture, depth): (&wgpu::TextureView, &wgpu::TextureView),
    overlay: &crate::sprites::Overlay,
) {
    pipeline.draw(gpu, pool, encoder, (picture, depth), overlay);
}

/// The interface pass: the in-game interface over everything. Its atlas and
/// vertex buffer come from the pool. Call inside `Gpu::checked`.
pub(super) fn interface(
    (gpu, pool): (&Gpu, &mut Pool),
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &crate::ui::UiPipeline,
    picture: &wgpu::TextureView,
    batch: &crate::ui::Batch,
) {
    pipeline.draw(gpu, pool, encoder, picture, batch);
}

/// The first half of the readback: the copy of the picture into its buffer,
/// as the last command of the frame.
pub(super) fn copy_out(encoder: &mut wgpu::CommandEncoder, picture: &Target) {
    picture.copy(encoder);
}

/// The second half of the readback, after the frame's commands were
/// submitted: waits for the GPU and returns the pixels. Call **outside**
/// `Gpu::checked` (it takes the GPU lock itself).
pub(super) fn readback(gpu: &Gpu, picture: &Target) -> Result<RgbaImage> {
    picture.read(gpu)
}
