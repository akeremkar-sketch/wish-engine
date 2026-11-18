//! The passes of a lit frame, in the order `Renderer::render` runs them.
//!
//! | Pass | Reads | Writes |
//! |---|---|---|
//! | [`shadow`] | the draw list, the light's matrix | the shadow map |
//! | [`sky`] | the frame uniform | the HDR picture (all of it) |
//! | [`opaque`] | the draw list, the frame uniform, the shadow map, the light lists | the HDR picture, the depth buffer |
//! | [`tone_map`] | the HDR picture | the picture (all of it) |
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

use super::draw_list::DrawList;
use super::pool::Pool;
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

/// What the shadow pass reads and writes.
pub(super) struct Shadow<'a> {
    /// Written: the light's depth picture, cleared to "nothing in the way".
    pub map: &'a wgpu::TextureView,
    /// The depth-only pipeline of solid surfaces.
    pub solid: &'a wgpu::RenderPipeline,
    /// The pipeline of alpha-masked surfaces: it leaves out what the
    /// material cuts out.
    pub masked: &'a wgpu::RenderPipeline,
    /// The light's matrix (group 0).
    pub light: &'a wgpu::BindGroup,
    /// The draws' uniform data (group 1, by offset).
    pub draw_uniforms: &'a wgpu::BindGroup,
    /// What casts shadows; `None` when the frame has no shadow map (the pass
    /// then only clears the 1x1 map the shader is given).
    pub casters: Option<&'a DrawList<'a>>,
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
    let Some(casters) = inputs.casters else {
        return;
    };
    // Solid surfaces go through the solid pipeline exactly as they always
    // did; the pipeline changes only where the list changes kind.
    let mut current = None;
    for item in &casters.items {
        if current != Some(item.masked) {
            pass.set_pipeline(if item.masked {
                inputs.masked
            } else {
                inputs.solid
            });
            pass.set_bind_group(0, inputs.light, &[]);
            current = Some(item.masked);
        }
        pass.set_bind_group(1, inputs.draw_uniforms, &[item.offset]);
        if item.masked {
            pass.set_bind_group(2, item.textures, &[]);
        }
        pass.set_vertex_buffer(0, item.mesh.vertices.slice(..));
        pass.set_index_buffer(item.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..item.mesh.index_count, 0, 0..1);
    }
}

/// The sky pass: fills the HDR picture with the sky, replacing whatever the
/// pooled target held.
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

/// The opaque pass: every surface, lit, with a depth buffer.
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
    for item in &inputs.draws.items {
        pass.set_pipeline(if item.double_sided {
            inputs.double_sided
        } else {
            inputs.single_sided
        });
        pass.set_bind_group(1, inputs.draw_uniforms, &[item.offset]);
        pass.set_bind_group(2, item.textures, &[]);
        pass.set_vertex_buffer(0, item.mesh.vertices.slice(..));
        pass.set_index_buffer(item.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..item.mesh.index_count, 0, 0..1);
    }
}

/// The tone mapping pass: from the HDR picture to the displayable one,
/// replacing whatever the pooled picture held.
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
