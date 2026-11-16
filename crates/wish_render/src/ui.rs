//! Drawing the in-game interface (design note M6 §7) over the finished
//! picture.
//!
//! Everything is prepared on the CPU first ([`build`]): one atlas texture
//! holds a white block (plain colour), the glyphs of every text (drawn by
//! `wish_text` at the picture's scale) and the pictures; each box becomes
//! rectangles that sample it. Then one pass draws them all in the boxes'
//! order. Glyphs are placed on whole pixels and copied texel for texel, so
//! text looks the same on every adapter.

use std::collections::BTreeMap;

use wish_core::{Diagnostic, Entity, Result};

use crate::extract::{RenderScene, UiBoxView};
use crate::gpu::Gpu;

/// The atlas's width in texels.
pub(crate) const ATLAS_WIDTH: u32 = 4096;
/// The tallest the atlas grows.
pub(crate) const MAX_ATLAS_HEIGHT: u32 = 4096;
/// The atlas's texel format: pictures are sRGB; white and glyph coverage
/// (alpha) read the same either way.
pub(crate) const ATLAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Bytes per vertex: position (2 floats), texture coordinate (2), colour (4).
const VERTEX_BYTES: u64 = 32;
/// The white block at the atlas's top left, for plain colour.
const WHITE: u32 = 4;

/// A picture for the interface: 8-bit RGBA, sRGB, rows top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiPicture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// The interface of one picture, ready for the GPU.
pub(crate) struct Batch {
    /// Vertices, six per rectangle.
    pub vertices: Vec<u8>,
    /// The atlas texels, `ATLAS_WIDTH` wide.
    pub atlas: Vec<u8>,
    /// The atlas's height in texels.
    pub atlas_height: u32,
    /// What could not be drawn.
    pub warnings: Vec<Diagnostic>,
}

/// A shelf packer over a growing atlas.
struct Atlas {
    pixels: Vec<u8>,
    height: u32,
    shelf_x: u32,
    shelf_y: u32,
    shelf_height: u32,
}

impl Atlas {
    fn new() -> Self {
        let mut atlas = Self {
            pixels: Vec::new(),
            height: 0,
            shelf_x: WHITE + 1,
            shelf_y: 0,
            shelf_height: WHITE,
        };
        atlas.grow(WHITE);
        for y in 0..WHITE {
            for x in 0..WHITE {
                atlas.set(x, y, [255; 4]);
            }
        }
        atlas
    }

    fn grow(&mut self, height: u32) {
        if height > self.height {
            self.height = height;
            self.pixels.resize((ATLAS_WIDTH * height * 4) as usize, 0);
        }
    }

    fn set(&mut self, x: u32, y: u32, texel: [u8; 4]) {
        let at = ((y * ATLAS_WIDTH + x) * 4) as usize;
        if let Some(slot) = self.pixels.get_mut(at..at + 4) {
            slot.copy_from_slice(&texel);
        }
    }

    /// Room for a `width` × `height` block (one texel apart from the others),
    /// or `None` when the atlas is full.
    fn place(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width + 1 > ATLAS_WIDTH {
            return None;
        }
        if self.shelf_x + width + 1 > ATLAS_WIDTH {
            self.shelf_y += self.shelf_height + 1;
            self.shelf_x = 0;
            self.shelf_height = 0;
        }
        if self.shelf_y + height > MAX_ATLAS_HEIGHT {
            return None;
        }
        let at = (self.shelf_x, self.shelf_y);
        self.shelf_x += width + 1;
        self.shelf_height = self.shelf_height.max(height);
        self.grow(self.shelf_y + height);
        Some(at)
    }
}

/// Where a block of the atlas is: left, top, width, height in texels.
type Region = (u32, u32, u32, u32);

/// A glyph in the atlas and where it sits from its pen position.
#[derive(Clone, Copy)]
struct PlacedGlyph {
    region: Region,
    left: i32,
    top: i32,
}

struct Quads {
    bytes: Vec<u8>,
    width: f32,
    height: f32,
}

impl Quads {
    /// A rectangle of the picture (pixels from the top left) showing
    /// `uv` (texel corners of the atlas) times `colour`.
    fn push(&mut self, [x0, y0, x1, y1]: [f32; 4], uv: [f32; 4], colour: [f32; 4]) {
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let nx = |x: f32| x / self.width * 2.0 - 1.0;
        let ny = |y: f32| 1.0 - y / self.height * 2.0;
        let [u0, v0, u1, v1] = uv;
        let corners = [
            (nx(x0), ny(y0), u0, v0),
            (nx(x1), ny(y0), u1, v0),
            (nx(x1), ny(y1), u1, v1),
            (nx(x0), ny(y0), u0, v0),
            (nx(x1), ny(y1), u1, v1),
            (nx(x0), ny(y1), u0, v1),
        ];
        for (x, y, u, v) in corners {
            for value in [x, y, u, v].into_iter().chain(colour) {
                self.bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
}

fn finite(values: [f32; 4]) -> bool {
    values.iter().all(|v| v.is_finite())
}

/// The texel-corner coordinates of `region` in an atlas `height` tall.
fn uv_of((x, y, w, h): Region, height: u32) -> [f32; 4] {
    let (aw, ah) = (ATLAS_WIDTH as f32, height.max(1) as f32);
    [
        x as f32 / aw,
        y as f32 / ah,
        (x + w) as f32 / aw,
        (y + h) as f32 / ah,
    ]
}

/// The colour of a box's background, lighter while the pointer is over a
/// button and darker while it is held down.
fn background_of(view: &UiBoxView) -> [f32; 4] {
    let [r, g, b, a] = view.background;
    let factor = match view.button {
        Some((_, true)) => 0.8,
        Some((true, false)) => 1.2,
        _ => 1.0,
    };
    [r * factor, g * factor, b * factor, a]
}

/// Prepares the interface of `scene` for a `width` × `height` picture.
/// `pictures` are the decoded pictures by project path; boxes asking for
/// one that is not there show their background only.
pub(crate) fn build(
    scene: &RenderScene,
    width: u32,
    height: u32,
    pictures: &BTreeMap<String, UiPicture>,
) -> Batch {
    let (w, h) = (width as f32, height as f32);
    let (scale, [left, top]) = scene.ui_settings.fit(w, h);
    let picture = |x: f32, y: f32| [left + x * scale, top + y * scale];
    let mut atlas = Atlas::new();
    let mut quads = Quads {
        bytes: Vec::new(),
        width: w,
        height: h,
    };
    let mut warnings = Vec::new();
    let mut full = false;
    // Regions are placed first and turned into texture coordinates at the
    // end, when the atlas's height is known.
    let mut pending: Vec<([f32; 4], Region, [f32; 4])> = Vec::new();
    let white: Region = (1, 1, 2, 2);
    let mut placed_pictures: BTreeMap<String, Option<Region>> = BTreeMap::new();
    let mut glyphs: BTreeMap<(String, u16, u32), Option<PlacedGlyph>> = BTreeMap::new();

    for view in &scene.ui {
        let [x, y, bw, bh] = view.rect;
        if !finite(view.rect) || !finite(view.content) {
            continue;
        }
        let background = background_of(view);
        if background[3] > 0.0 && finite(background) {
            let [x0, y0] = picture(x, y).map(f32::round);
            let [x1, y1] = picture(x + bw, y + bh).map(f32::round);
            pending.push(([x0, y0, x1, y1], white, background));
        }
        let [cx, cy, cw, ch] = view.content;
        if let Some((path, tint)) = &view.image {
            let region = match placed_pictures.get(path) {
                Some(region) => *region,
                None => {
                    let region = pictures.get(path).and_then(|image| {
                        let at = atlas.place(image.width, image.height);
                        if at.is_none() {
                            full = true;
                        }
                        let (ax, ay) = at?;
                        for row in 0..image.height {
                            for column in 0..image.width {
                                let i = ((row * image.width + column) * 4) as usize;
                                let mut texel = [0u8; 4];
                                texel.copy_from_slice(image.rgba.get(i..i + 4)?);
                                atlas.set(ax + column, ay + row, texel);
                            }
                        }
                        Some((ax, ay, image.width, image.height))
                    });
                    placed_pictures.insert(path.clone(), region);
                    region
                }
            };
            if let Some(region) = region
                && finite(*tint)
            {
                let [x0, y0] = picture(cx, cy).map(f32::round);
                let [x1, y1] = picture(cx + cw, cy + ch).map(f32::round);
                pending.push(([x0, y0, x1, y1], region, *tint));
            }
        }
        if let Some(text) = &view.text
            && finite(text.color)
        {
            let Ok(font) = scene.fonts.get(&text.font) else {
                continue;
            };
            let laid =
                wish_text::layout(font, &text.text, text.size, Some(cw), text.wrap, text.align);
            let px = text.size * scale;
            for glyph in laid.glyphs {
                let key = (font.name().to_owned(), glyph.id, px.to_bits());
                let placed = match glyphs.get(&key) {
                    Some(placed) => *placed,
                    None => {
                        let placed = wish_text::rasterize(font, glyph.id, px).and_then(|bitmap| {
                            let at = atlas.place(bitmap.width, bitmap.height);
                            if at.is_none() {
                                full = true;
                            }
                            let (ax, ay) = at?;
                            for row in 0..bitmap.height {
                                for column in 0..bitmap.width {
                                    let coverage =
                                        bitmap.coverage[(row * bitmap.width + column) as usize];
                                    atlas.set(ax + column, ay + row, [255, 255, 255, coverage]);
                                }
                            }
                            Some(PlacedGlyph {
                                region: (ax, ay, bitmap.width, bitmap.height),
                                left: bitmap.left,
                                top: bitmap.top,
                            })
                        });
                        glyphs.insert(key, placed);
                        placed
                    }
                };
                let Some(placed) = placed else {
                    continue;
                };
                let [pen_x, pen_y] = picture(cx + glyph.x, cy + glyph.y).map(f32::round);
                let x0 = pen_x + placed.left as f32;
                let y0 = pen_y + placed.top as f32;
                let (_, _, gw, gh) = placed.region;
                pending.push((
                    [x0, y0, x0 + gw as f32, y0 + gh as f32],
                    placed.region,
                    text.color,
                ));
            }
        }
    }
    if full {
        warnings.push(Diagnostic::warning(
            "ui_atlas_full",
            "The interface has more text and pictures than fit in one frame's atlas; some were not drawn.",
            "Use fewer or smaller pictures (at most 4096x4096 together), or less text at very large sizes.",
        ));
    }
    let atlas_height = atlas.height.max(1);
    for (rect, region, colour) in pending {
        let uv = if region == white {
            // The middle of the white block: plain colour, no filtering edge.
            let [u, v, _, _] = uv_of((2, 2, 0, 0), atlas_height);
            [u, v, u, v]
        } else {
            uv_of(region, atlas_height)
        };
        quads.push(rect, uv, colour);
    }
    Batch {
        vertices: quads.bytes,
        atlas: atlas.pixels,
        atlas_height,
        warnings,
    }
}

/// One element of the interface as a picture shows it, for `describe`.
#[derive(Clone, Debug, PartialEq)]
pub struct UiElement {
    /// The entity.
    pub entity: Entity,
    /// Its box in the picture: left, top, right, bottom, in pixels.
    pub bounding_box: [i32; 4],
    /// Its text, if it shows any.
    pub text: Option<String>,
    /// For a button: whether the pointer is over it, and whether it is held.
    pub button: Option<(bool, bool)>,
    /// Whether it shows a picture.
    pub image: bool,
}

/// The interface elements of `scene` that show something (a background,
/// text, a picture, or a button), in drawing order, placed in a `width` ×
/// `height` picture.
pub fn ui_elements(scene: &RenderScene, width: u32, height: u32) -> Vec<UiElement> {
    let (scale, [left, top]) = scene.ui_settings.fit(width as f32, height as f32);
    scene
        .ui
        .iter()
        .filter(|view| {
            view.background[3] > 0.0
                || view.text.as_ref().is_some_and(|t| !t.text.is_empty())
                || view.image.is_some()
                || view.button.is_some()
        })
        .map(|view| {
            let [x, y, w, h] = view.rect;
            let at = |v: f32| v.round() as i32;
            UiElement {
                entity: view.entity,
                bounding_box: [
                    at(left + x * scale),
                    at(top + y * scale),
                    at(left + (x + w) * scale),
                    at(top + (y + h) * scale),
                ],
                text: view.text.as_ref().map(|t| t.text.clone()),
                button: view.button,
                image: view.image.is_some(),
            }
        })
        .collect()
}

/// The interface pipeline.
pub(crate) struct UiPipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl UiPipeline {
    /// Builds the pipeline from the `ui_pass` shader's source, drawing into
    /// pictures of `format`.
    pub fn new(gpu: &Gpu, source: &str, format: wgpu::TextureFormat) -> Result<Self> {
        gpu.checked("create the interface pipeline", |gpu| {
            let device = &gpu.device;
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("interface"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("interface"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("ui_pass"),
                source: wgpu::ShaderSource::Wgsl(source.to_owned().into()),
            });
            let float = |location: u32, offset: u64, format| wgpu::VertexAttribute {
                format,
                offset,
                shader_location: location,
            };
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("interface"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: VERTEX_BYTES,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            float(0, 0, wgpu::VertexFormat::Float32x2),
                            float(1, 8, wgpu::VertexFormat::Float32x2),
                            float(2, 16, wgpu::VertexFormat::Float32x4),
                        ],
                    })],
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
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("interface"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..wgpu::SamplerDescriptor::default()
            });
            Self {
                pipeline,
                layout,
                sampler,
            }
        })
    }

    /// Records drawing `batch` over `target` into `encoder`. Call inside
    /// `Gpu::checked`.
    pub fn draw(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        batch: &Batch,
    ) {
        if batch.vertices.is_empty() {
            return;
        }
        let device = &gpu.device;
        let size = wgpu::Extent3d {
            width: ATLAS_WIDTH,
            height: batch.atlas_height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("interface atlas"),
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
            &batch.atlas,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS_WIDTH * 4),
                rows_per_image: Some(batch.atlas_height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("interface"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let vertices = crate::renderer::buffer_with(
            gpu,
            "interface",
            &batch.vertices,
            wgpu::BufferUsages::VERTEX,
        );
        let count = u32::try_from(batch.vertices.len() as u64 / VERTEX_BYTES).unwrap_or(0);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("interface"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..count, 0..1);
    }
}
