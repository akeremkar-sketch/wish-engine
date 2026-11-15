//! Drawing into a texture and reading it back, without a window.

use wish_core::{Diagnostic, Result};

use crate::gpu::Gpu;
use crate::image::RgbaImage;

/// The colour format of captured images: 8-bit sRGB, what PNGs hold.
pub(crate) const CAPTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// The largest capture side, in pixels.
pub const MAX_CAPTURE_SIZE: u32 = 8192;

fn check_size(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 || width > MAX_CAPTURE_SIZE || height > MAX_CAPTURE_SIZE {
        return Err(Diagnostic::error(
            "invalid_capture_size",
            format!("A capture of {width}x{height} pixels is not possible."),
            format!("Use a width and height between 1 and {MAX_CAPTURE_SIZE}, such as 1280x720."),
        ));
    }
    Ok(())
}

/// An offscreen colour target and the buffer it is copied into.
pub(crate) struct Target {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row: u32,
}

impl Target {
    pub(crate) fn new(gpu: &Gpu, width: u32, height: u32) -> Result<Self> {
        check_size(width, height)?;
        let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        gpu.checked("create the capture target", |gpu| {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("capture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: CAPTURE_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("capture readback"),
                size: u64::from(padded_row) * u64::from(height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            Self {
                texture,
                view,
                readback,
                width,
                height,
                padded_row,
            }
        })
    }

    /// Appends the copy of the target into the readback buffer.
    pub(crate) fn copy(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Reads the copied pixels back, removing the row padding.
    pub(crate) fn read(&self, gpu: &Gpu) -> Result<RgbaImage> {
        let slice = self.readback.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        gpu.wait()?;
        let mapped = receiver.recv().ok().and_then(Result::ok);
        if mapped.is_none() {
            return Err(Diagnostic::error(
                "gpu_readback_failed",
                "The captured image could not be read back from the GPU.",
                "The device may have been lost. Restart the run; if it repeats, use --software.",
            ));
        }
        let row = (self.width * 4) as usize;
        let mut pixels = Vec::with_capacity(row * self.height as usize);
        {
            let data = slice.get_mapped_range().map_err(|error| {
                Diagnostic::error(
                    "gpu_readback_failed",
                    format!("The captured image could not be mapped: {error}."),
                    "This is a bug in wish_render; please report it.",
                )
            })?;
            for chunk in data
                .chunks(self.padded_row as usize)
                .take(self.height as usize)
            {
                pixels.extend_from_slice(chunk.get(..row).unwrap_or_default());
            }
        }
        self.readback.unmap();
        Ok(RgbaImage {
            width: self.width,
            height: self.height,
            pixels,
        })
    }
}

/// Draws the probe triangle (the WP1 spike scene) into an image.
pub fn draw_probe(gpu: &Gpu, width: u32, height: u32) -> Result<RgbaImage> {
    let target = Target::new(gpu, width, height)?;
    gpu.checked("draw the probe", |gpu| {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("probe"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/probe.wgsl").into()),
            });
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("probe"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: CAPTURE_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("probe"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("probe"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.02,
                            b: 0.03,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.draw(0..3, 0..1);
        }
        target.copy(&mut encoder);
        gpu.queue.submit([encoder.finish()]);
    })?;
    target.read(gpu)
}
