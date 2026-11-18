//! A probe of the adapter, run by hand: do two live bind groups of one
//! layout each read their own uniform buffer?
//!
//! Measured on 18 November 2026 (Windows 10 19045, wgpu 30.0.2):
//!
//! | Adapter | Own layout each | One shared layout | Shared, dynamic offsets |
//! |---|---|---|---|
//! | Microsoft Basic Render Driver (WARP), Dx12 | each its own | **both read the buffer of the group made last** | each its own |
//! | NVIDIA GeForce RTX 3060 Ti, Dx12 | each its own | each its own | each its own |
//! | NVIDIA GeForce RTX 3060 Ti, Vulkan | each its own | each its own | each its own |
//!
//! Two bind group layouts with equal entries are one layout to the device
//! (wgpu shares them), so two renderers on one device share theirs. This is
//! why the renderer builds its bind groups in the frame that uses them and
//! keeps none (`pool.rs`), and why a frame must not hold two bind groups of
//! one layout over *different* uniform buffers unless they are bound with a
//! dynamic offset (the third column: those are found through the buffer's
//! address, not through the group's descriptors).
//!
//! Run it with
//! `cargo test -p wish_render --lib warp_probe -- --ignored --nocapture`;
//! `WISH_PROBE_HARDWARE=1` uses the hardware adapter, and wgpu's own
//! `WGPU_BACKEND=dx12` or `vulkan` picks the API.

use crate::capture::Target;
use crate::gpu::{Gpu, GpuOptions};

const SHADER: &str = "
struct Tint { colour: vec4<f32> };
@group(0) @binding(0) var<uniform> tint: Tint;
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}
@fragment fn fs_main() -> @location(0) vec4<f32> { return tint.colour; }
";

/// A pipeline that fills the picture with the colour in its uniform buffer,
/// that buffer, and a bind group over it.
struct Painter {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
    /// Whether the buffer is bound with a dynamic offset.
    dynamic: bool,
}

/// `min_binding_size` only makes the layouts of two painters differ, so that
/// the device does not share them.
fn painter(gpu: &Gpu, min_binding_size: u64, dynamic: bool) -> Painter {
    gpu.checked("build a probe", |gpu| {
        let device = &gpu.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: dynamic,
                    min_binding_size: wgpu::BufferSize::new(min_binding_size),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
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
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(64),
                }),
            }],
        });
        Painter {
            pipeline,
            buffer,
            group,
            dynamic,
        }
    })
    .unwrap()
}

/// Writes `colour` into the painter's buffer, draws with its bind group and
/// returns the colour that came out.
fn paint(gpu: &Gpu, painter: &Painter, colour: [f32; 4]) -> [u8; 4] {
    let target = Target::with_format(gpu, 4, 4, wgpu::TextureFormat::Rgba8Unorm).unwrap();
    gpu.checked("draw a probe", |gpu| {
        let bytes: Vec<u8> = colour.iter().flat_map(|v| v.to_le_bytes()).collect();
        gpu.queue.write_buffer(&painter.buffer, 0, &bytes);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&painter.pipeline);
            let offsets: &[u32] = if painter.dynamic { &[0] } else { &[] };
            pass.set_bind_group(0, &painter.group, offsets);
            pass.draw(0..3, 0..1);
        }
        target.copy(&mut encoder);
        gpu.queue.submit([encoder.finish()]);
    })
    .unwrap();
    target.read(gpu).unwrap().pixel(1, 1).unwrap()
}

#[test]
#[ignore = "a probe of the adapter, run by hand"]
fn two_bind_groups_of_one_layout_over_two_uniform_buffers() {
    let gpu = Gpu::new(GpuOptions {
        software: std::env::var_os("WISH_PROBE_HARDWARE").is_none(),
    })
    .unwrap();
    println!("adapter: {:?}", gpu.adapter());
    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
    const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
    for (case, sizes, dynamic) in [
        ("own layout each", (16, 32), false),
        ("one shared layout", (16, 16), false),
        ("one shared layout, dynamic offsets", (16, 16), true),
    ] {
        // Both painters exist before either draws.
        let first = painter(&gpu, sizes.0, dynamic);
        let second = painter(&gpu, sizes.1, dynamic);
        let drawn = [
            paint(&gpu, &first, RED),
            paint(&gpu, &second, GREEN),
            paint(&gpu, &first, BLUE),
        ];
        let own = drawn == [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
        println!(
            "{case}: the first painter drew {:?} for red and {:?} for blue (the second drew {:?} for green): {}",
            drawn[0],
            drawn[2],
            drawn[1],
            if own {
                "each group reads its own buffer"
            } else {
                "the first group reads the buffer of the group made last"
            }
        );
    }
}
