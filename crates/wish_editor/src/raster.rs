//! A deterministic CPU rasterizer for egui's triangle meshes.
//!
//! The editor is drawn without the GPU (ADR-018, M8a design §2): the same
//! pixels come out on every machine, so the whole interface is tested headless
//! with golden images, and the window shows the picture the way `wish_window`
//! shows the renderer's.
//!
//! Pixels are sRGB with premultiplied alpha, blended in that (gamma) space,
//! as egui expects. Triangle edges are exact: vertex positions snap to 1/256
//! of a pixel and coverage is decided with integer arithmetic and the
//! top-left rule, so two triangles that share an edge never both paint a pixel
//! on it (a translucent shape would show a seam) and never leave a gap.

use std::collections::BTreeMap;

use egui::epaint::textures::TexturesDelta;
use egui::epaint::{ImageData, Primitive, TextureId, Vertex};
use egui::{ClippedPrimitive, TextureFilter, TextureOptions};

/// Sub-pixel precision of triangle vertices: 1/256 pixel.
const SUBPIXEL: i64 = 256;

/// A picture: 8-bit RGBA, row by row, sRGB, alpha premultiplied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub pixels: Vec<u8>,
}

impl Canvas {
    /// A canvas filled with one colour (`[r, g, b, a]`).
    pub fn new(width: u32, height: u32, fill: [u8; 4]) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..width as usize * height as usize {
            pixels.extend_from_slice(&fill);
        }
        Self {
            width,
            height,
            pixels,
        }
    }
}

/// One texture: premultiplied sRGB pixels and how to sample them.
struct Texture {
    size: [usize; 2],
    pixels: Vec<[u8; 4]>,
    options: TextureOptions,
}

/// The textures egui has made (the font atlas, images), by id.
#[derive(Default)]
pub struct Textures {
    textures: BTreeMap<TextureId, Texture>,
}

impl Textures {
    /// Applies a frame's texture changes: new or patched textures first, call
    /// [`Textures::free`] with the same delta after painting.
    pub fn apply(&mut self, delta: &TexturesDelta) {
        // Sorted, so that patches apply in the same order everywhere.
        let mut ids: Vec<&TextureId> = delta.set.keys().collect();
        ids.sort();
        for id in ids {
            for part in &delta.set[id] {
                let ImageData::Color(image) = &part.image;
                let pixels: Vec<[u8; 4]> = image.pixels.iter().map(|c| c.to_array()).collect();
                match part.pos {
                    None => {
                        self.textures.insert(
                            *id,
                            Texture {
                                size: image.size,
                                pixels,
                                options: part.options,
                            },
                        );
                    }
                    Some([x0, y0]) => {
                        let Some(texture) = self.textures.get_mut(id) else {
                            continue;
                        };
                        for row in 0..image.size[1] {
                            for column in 0..image.size[0] {
                                let (x, y) = (x0 + column, y0 + row);
                                if x < texture.size[0] && y < texture.size[1] {
                                    texture.pixels[y * texture.size[0] + x] =
                                        pixels[row * image.size[0] + column];
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Forgets the textures the frame is done with.
    pub fn free_ids(&mut self, ids: &[TextureId]) {
        for id in ids {
            self.textures.remove(id);
        }
    }

    /// Whether a texture is known (a mesh that names an unknown one is
    /// skipped, not an error).
    pub fn contains(&self, id: TextureId) -> bool {
        self.textures.contains_key(&id)
    }
}

/// A texel, clamped to the texture's edge, as floats 0..=255.
fn texel(texture: &Texture, x: i64, y: i64) -> [f32; 4] {
    let (w, h) = (texture.size[0] as i64, texture.size[1] as i64);
    let x = x.clamp(0, w - 1) as usize;
    let y = y.clamp(0, h - 1) as usize;
    let p = texture.pixels[y * texture.size[0] + x];
    [
        f32::from(p[0]),
        f32::from(p[1]),
        f32::from(p[2]),
        f32::from(p[3]),
    ]
}

/// The texture's colour at normalized coordinates, floats 0..=255.
fn sample(texture: &Texture, u: f32, v: f32, magnified: bool) -> [f32; 4] {
    let (w, h) = (texture.size[0] as f32, texture.size[1] as f32);
    let filter = if magnified {
        texture.options.magnification
    } else {
        texture.options.minification
    };
    let (x, y) = (u * w, v * h);
    match filter {
        TextureFilter::Nearest => texel(texture, x.floor() as i64, y.floor() as i64),
        TextureFilter::Linear => {
            let (x, y) = (x - 0.5, y - 0.5);
            let (x0, y0) = (x.floor(), y.floor());
            let (fx, fy) = (x - x0, y - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let a = texel(texture, x0, y0);
            let b = texel(texture, x0 + 1, y0);
            let c = texel(texture, x0, y0 + 1);
            let d = texel(texture, x0 + 1, y0 + 1);
            let mut out = [0.0; 4];
            for i in 0..4 {
                let top = a[i] + (b[i] - a[i]) * fx;
                let bottom = c[i] + (d[i] - c[i]) * fx;
                out[i] = top + (bottom - top) * fy;
            }
            out
        }
    }
}

fn snap(value: f32) -> i64 {
    (f64::from(value) * SUBPIXEL as f64).round() as i64
}

/// Twice the signed area of (a, b, p): which side of the edge a→b the point
/// is on. Exact (integers).
fn edge(a: (i64, i64), b: (i64, i64), p: (i64, i64)) -> i64 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

/// Whether an edge a→b is a "top" or "left" edge (counter-clockwise on a
/// y-down screen, as the triangle is made to wind): a pixel exactly on it
/// belongs to this triangle only if it is.
fn top_left(a: (i64, i64), b: (i64, i64)) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    dy < 0 || (dy == 0 && dx > 0)
}

/// Paints egui's primitives onto the canvas. `pixels_per_point` converts egui's
/// points to pixels. Callback primitives (GPU paint hooks) are skipped.
pub fn paint(
    canvas: &mut Canvas,
    textures: &Textures,
    primitives: &[ClippedPrimitive],
    pixels_per_point: f32,
) {
    for clipped in primitives {
        let Primitive::Mesh(mesh) = &clipped.primitive else {
            continue;
        };
        let Some(texture) = textures.textures.get(&mesh.texture_id) else {
            continue;
        };
        let clip = clipped.clip_rect;
        let left = ((clip.min.x * pixels_per_point).floor().max(0.0)) as i64;
        let top = ((clip.min.y * pixels_per_point).floor().max(0.0)) as i64;
        let right = ((clip.max.x * pixels_per_point).ceil()).min(canvas.width as f32) as i64;
        let bottom = ((clip.max.y * pixels_per_point).ceil()).min(canvas.height as f32) as i64;
        if left >= right || top >= bottom {
            continue;
        }
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let vertex = |i: u32| mesh.vertices.get(i as usize).copied();
            let (Some(a), Some(b), Some(c)) = (
                vertex(triangle[0]),
                vertex(triangle[1]),
                vertex(triangle[2]),
            ) else {
                continue;
            };
            triangle_fill(
                canvas,
                texture,
                [a, b, c],
                pixels_per_point,
                (left, top, right, bottom),
            );
        }
    }
}

fn triangle_fill(
    canvas: &mut Canvas,
    texture: &Texture,
    mut v: [Vertex; 3],
    ppp: f32,
    (left, top, right, bottom): (i64, i64, i64, i64),
) {
    let point = |vertex: &Vertex| (snap(vertex.pos.x * ppp), snap(vertex.pos.y * ppp));
    let mut p = [point(&v[0]), point(&v[1]), point(&v[2])];
    let mut area = edge(p[0], p[1], p[2]);
    if area == 0 {
        return;
    }
    if area < 0 {
        v.swap(1, 2);
        p.swap(1, 2);
        area = -area;
    }
    // The pixels the triangle can touch.
    let min_x = (p[0].0.min(p[1].0).min(p[2].0) / SUBPIXEL).max(left);
    let max_x = (p[0].0.max(p[1].0).max(p[2].0) / SUBPIXEL + 1).min(right - 1);
    let min_y = (p[0].1.min(p[1].1).min(p[2].1) / SUBPIXEL).max(top);
    let max_y = (p[0].1.max(p[1].1).max(p[2].1) / SUBPIXEL + 1).min(bottom - 1);
    // How much texture is covered by one pixel: a magnified texture is one
    // that has more pixels than texels.
    let uv_area = {
        let (w, h) = (texture.size[0] as f32, texture.size[1] as f32);
        let du1 = (v[1].uv.x - v[0].uv.x) * w;
        let dv1 = (v[1].uv.y - v[0].uv.y) * h;
        let du2 = (v[2].uv.x - v[0].uv.x) * w;
        let dv2 = (v[2].uv.y - v[0].uv.y) * h;
        (du1 * dv2 - dv1 * du2).abs()
    };
    let pixel_area = area as f32 / (SUBPIXEL * SUBPIXEL) as f32;
    let magnified = uv_area <= pixel_area;
    let inv_area = 1.0 / area as f32;
    // A pixel exactly on an edge is inside only for top-left edges.
    let bias = |a, b| i64::from(!top_left(a, b));
    let (bias0, bias1, bias2) = (bias(p[1], p[2]), bias(p[2], p[0]), bias(p[0], p[1]));
    // Most of an interface is flat: one colour and one texel (a rectangle's
    // fill). Those pixels need no interpolation.
    let colors = [
        v[0].color.to_array(),
        v[1].color.to_array(),
        v[2].color.to_array(),
    ];
    let flat = v[0].uv == v[1].uv
        && v[1].uv == v[2].uv
        && colors[0] == colors[1]
        && colors[1] == colors[2];
    let flat_source = flat.then(|| {
        let tex = sample(texture, v[0].uv.x, v[0].uv.y, magnified);
        let mut src = [0.0f32; 4];
        for (i, slot) in src.iter_mut().enumerate() {
            *slot = f32::from(colors[0][i]) * tex[i] / 255.0;
        }
        src
    });
    // The edge functions are linear: one step in x or y adds a constant.
    let step_x = [
        -(p[2].1 - p[1].1) * SUBPIXEL,
        -(p[0].1 - p[2].1) * SUBPIXEL,
        -(p[1].1 - p[0].1) * SUBPIXEL,
    ];
    let step_y = [
        (p[2].0 - p[1].0) * SUBPIXEL,
        (p[0].0 - p[2].0) * SUBPIXEL,
        (p[1].0 - p[0].0) * SUBPIXEL,
    ];
    let first = (
        min_x * SUBPIXEL + SUBPIXEL / 2,
        min_y * SUBPIXEL + SUBPIXEL / 2,
    );
    let mut row = [
        edge(p[1], p[2], first),
        edge(p[2], p[0], first),
        edge(p[0], p[1], first),
    ];
    for y in min_y..=max_y {
        let mut w = row;
        for x in min_x..=max_x {
            if w[0] >= bias0 && w[1] >= bias1 && w[2] >= bias2 {
                let at = (y as usize * canvas.width as usize + x as usize) * 4;
                if let Some(src) = flat_source {
                    blend(&mut canvas.pixels[at..at + 4], src);
                } else {
                    let (l0, l1, l2) = (
                        w[0] as f32 * inv_area,
                        w[1] as f32 * inv_area,
                        w[2] as f32 * inv_area,
                    );
                    let u = l0 * v[0].uv.x + l1 * v[1].uv.x + l2 * v[2].uv.x;
                    let t = l0 * v[0].uv.y + l1 * v[1].uv.y + l2 * v[2].uv.y;
                    let tex = sample(texture, u, t, magnified);
                    let mut src = [0.0f32; 4];
                    for (i, slot) in src.iter_mut().enumerate() {
                        let vertex_color = l0 * f32::from(colors[0][i])
                            + l1 * f32::from(colors[1][i])
                            + l2 * f32::from(colors[2][i]);
                        // Both are 0..=255: multiply, back to 0..=255.
                        *slot = vertex_color * tex[i] / 255.0;
                    }
                    blend(&mut canvas.pixels[at..at + 4], src);
                }
            }
            for k in 0..3 {
                w[k] += step_x[k];
            }
        }
        for k in 0..3 {
            row[k] += step_y[k];
        }
    }
}

/// Premultiplied "over": src + dst * (1 - src alpha).
fn blend(dst: &mut [u8], src: [f32; 4]) {
    let alpha = src[3] / 255.0;
    if src[3] >= 255.0 {
        for (slot, value) in dst.iter_mut().zip(src) {
            *slot = (value + 0.5).clamp(0.0, 255.0) as u8;
        }
        return;
    }
    for (slot, value) in dst.iter_mut().zip(src) {
        let out = value + f32::from(*slot) * (1.0 - alpha);
        *slot = (out + 0.5).clamp(0.0, 255.0) as u8;
    }
}
