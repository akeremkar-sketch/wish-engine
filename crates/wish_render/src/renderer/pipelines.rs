//! The bind group layouts and render pipelines of the lit picture, built once
//! per renderer from the engine's shaders.

use wish_core::Result;

use super::{DEPTH_FORMAT, DRAW_UNIFORM_SIZE, FRAME_UNIFORM_SIZE, HDR_FORMAT, SHADOW_FORMAT};
use crate::capture::CAPTURE_FORMAT;
use crate::gpu::Gpu;
use crate::mesh::VERTEX_STRIDE;

/// Bytes of the light uniform of the shadow pass: one matrix.
pub(super) const LIGHT_UNIFORM_SIZE: u64 = 64;

/// The layouts, pipelines and samplers of the lit picture.
pub(super) struct Pipelines {
    /// Group 0 of the sky and opaque passes: the frame uniform, the shadow
    /// map and its sampler, and the three light lists.
    pub frame_layout: wgpu::BindGroupLayout,
    /// One draw's uniform data, picked by a dynamic offset.
    pub draw_layout: wgpu::BindGroupLayout,
    /// The HDR picture, read by the tone mapping pass.
    pub hdr_layout: wgpu::BindGroupLayout,
    /// A material's five textures and the sampler.
    pub material_layout: wgpu::BindGroupLayout,
    /// The light's matrix, for the shadow pass.
    pub light_layout: wgpu::BindGroupLayout,
    pub sky: wgpu::RenderPipeline,
    pub opaque: wgpu::RenderPipeline,
    /// The same, without back-face culling, for double-sided materials.
    pub opaque_double: wgpu::RenderPipeline,
    /// The shadow pass: depth only, from the light.
    pub shadow: wgpu::RenderPipeline,
    /// The shadow pass for alpha-masked materials: the same depth, with a
    /// fragment stage that leaves out the pixels the material cuts out.
    pub shadow_masked: wgpu::RenderPipeline,
    pub tonemap: wgpu::RenderPipeline,
    pub sampler: wgpu::Sampler,
    pub shadow_sampler: wgpu::Sampler,
}

/// A material's five textures (base colour, metallic-roughness, normal,
/// occlusion, emissive) and the sampler as one bind group.
pub(super) fn material_group(
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

impl Pipelines {
    /// Builds everything from the checked shader sources (`source_of` gives
    /// the composed WGSL of an entry shader by name).
    pub(super) fn build(gpu: &Gpu, source_of: &dyn Fn(&str) -> String) -> Result<Self> {
        gpu.checked("build the render pipelines", |gpu| {
            let device = &gpu.device;
            let module = |name: &str| {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(name),
                    source: wgpu::ShaderSource::Wgsl(source_of(name).into()),
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
                        min_binding_size: wgpu::BufferSize::new(LIGHT_UNIFORM_SIZE),
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
            // The same for alpha-masked materials: the fragment stage samples
            // the base colour's alpha and discards what the lit pass cuts
            // out, so a leaf's shadow has the leaf's shape. Opaque draws
            // never use it, so their shadows are untouched by it.
            let shadow_masked = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shadow masked"),
                layout: Some(&layout(&[&light_layout, &draw_layout, &material_layout])),
                vertex: wgpu::VertexState {
                    module: &shadow_shader,
                    entry_point: Some("vs_masked"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: VERTEX_STRIDE,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 0,
                                shader_location: 0,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x2,
                                offset: 24,
                                shader_location: 1,
                            },
                        ],
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
                fragment: Some(wgpu::FragmentState {
                    module: &shadow_shader,
                    entry_point: Some("fs_masked"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[],
                }),
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
            Self {
                frame_layout,
                draw_layout,
                hdr_layout,
                material_layout,
                light_layout,
                sky,
                opaque,
                opaque_double,
                shadow,
                shadow_masked,
                tonemap,
                sampler,
                shadow_sampler,
            }
        })
    }
}
