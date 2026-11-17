//! Vector icons and the engine's mark (M9): each icon is SVG path data on a
//! 24-unit grid, drawn by `vello_cpu` (the rasterizer egui draws its text
//! with) with anti-aliased curves, round caps and joins, into a texture at
//! the exact pixel size it is shown at. Textures are made once per icon and
//! size and kept by the egui context.

use std::collections::BTreeMap;

use egui::{Color32, ColorImage, Context, Id, TextureHandle, TextureOptions};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, BezPath, Cap, Circle, Join, Shape as _, Stroke};
use vello_cpu::peniko::{Brush, Gradient};
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::style::Icon;

/// One part of an icon.
enum Part {
    /// An outline, stroked 2 units wide with round ends and corners.
    Line(&'static str),
    /// A filled area.
    Fill(&'static str),
    /// A filled area with its corners rounded (filled and stroked 1.5 wide).
    Soft(&'static str),
    /// A circle's outline: centre and radius.
    Ring(f64, f64, f64),
    /// A filled circle.
    Disc(f64, f64, f64),
}

use Part::{Disc, Fill, Line, Ring, Soft};

/// An icon's drawing, on a 24 by 24 grid.
fn parts(icon: Icon) -> &'static [Part] {
    match icon {
        Icon::Play => &[Soft(
            "M7.5 5.2 C7.5 4.4 8.4 3.9 9.1 4.4 L19.1 11.2 C19.7 11.6 19.7 12.4 19.1 12.8 L9.1 19.6 C8.4 20.1 7.5 19.6 7.5 18.8 Z",
        )],
        Icon::Pause => &[
            Soft("M6.5 5 L10 5 L10 19 L6.5 19 Z"),
            Soft("M14 5 L17.5 5 L17.5 19 L14 19 Z"),
        ],
        Icon::Step => &[
            Soft("M5 5.5 L14 12 L5 18.5 Z"),
            Soft("M16.5 5.5 L19 5.5 L19 18.5 L16.5 18.5 Z"),
        ],
        Icon::Stop => &[Soft("M6 6 L18 6 L18 18 L6 18 Z")],
        Icon::Record => &[Disc(12.0, 12.0, 6.5)],
        Icon::Undo => &[
            Line("M9 14 L4 9 L9 4"),
            Line("M4 9 L14.5 9 C17.8 9 20.5 11.7 20.5 15 C20.5 18.3 17.8 21 14.5 21 L11 21"),
        ],
        Icon::Redo => &[
            Line("M15 14 L20 9 L15 4"),
            Line("M20 9 L9.5 9 C6.2 9 3.5 11.7 3.5 15 C3.5 18.3 6.2 21 9.5 21 L13 21"),
        ],
        Icon::Move => &[
            Line("M12 3 L12 21 M3 12 L21 12"),
            Line("M9 6 L12 3 L15 6 M9 18 L12 21 L15 18 M6 9 L3 12 L6 15 M18 9 L21 12 L18 15"),
        ],
        Icon::Rotate => &[
            Line(
                "M20.5 12 C20.5 16.7 16.7 20.5 12 20.5 C7.3 20.5 3.5 16.7 3.5 12 C3.5 7.3 7.3 3.5 12 3.5 C14.6 3.5 17 4.7 18.5 6.6",
            ),
            Line("M19 2.5 L19 7 L14.5 7"),
        ],
        Icon::Scale => &[
            Line("M14 3 L21 3 L21 10"),
            Line("M21 3 L13 11"),
            Line("M3.5 12.5 L11.5 12.5 L11.5 20.5 L3.5 20.5 Z"),
        ],
        Icon::Globe => &[
            Ring(12.0, 12.0, 9.0),
            Line("M3 12 L21 12"),
            Line(
                "M12 3 C14.6 5.6 15.7 8.7 15.7 12 C15.7 15.3 14.6 18.4 12 21 C9.4 18.4 8.3 15.3 8.3 12 C8.3 8.7 9.4 5.6 12 3 Z",
            ),
        ],
        Icon::Magnet => &[
            Line("M5.5 4 L5.5 13 C5.5 16.6 8.4 19.5 12 19.5 C15.6 19.5 18.5 16.6 18.5 13 L18.5 4"),
            Line("M9.5 4 L9.5 13 C9.5 14.4 10.6 15.5 12 15.5 C13.4 15.5 14.5 14.4 14.5 13 L14.5 4"),
            Line("M5.5 8 L9.5 8 M14.5 8 L18.5 8"),
        ],
        Icon::Spark => &[Fill(
            "M12 2 C12.7 7.3 16.7 11.3 22 12 C16.7 12.7 12.7 16.7 12 22 C11.3 16.7 7.3 12.7 2 12 C7.3 11.3 11.3 7.3 12 2 Z",
        )],
        Icon::Hand => &[
            Line("M18 11.5 L18 6.5 C18 5.4 17.1 4.5 16 4.5 C14.9 4.5 14 5.4 14 6.5"),
            Line("M14 10.5 L14 4.5 C14 3.4 13.1 2.5 12 2.5 C10.9 2.5 10 3.4 10 4.5 L10 6.5"),
            Line("M10 10.5 L10 6.5 C10 5.4 9.1 4.5 8 4.5 C6.9 4.5 6 5.4 6 6.5 L6 14.5"),
            Line(
                "M18 8.5 C18 7.4 18.9 6.5 20 6.5 C21.1 6.5 22 7.4 22 8.5 L22 14 C22 18.4 18.4 22 14 22 L12.5 22 C9.7 22 8 21.2 6.5 19.7 L3 16.2 C2.2 15.4 2.2 14.1 3 13.3 C3.8 12.5 5.1 12.5 5.9 13.3 L6 14.5",
            ),
        ],
        Icon::Camera => &[
            Line(
                "M3.5 7 L14.5 7 C15.3 7 16 7.7 16 8.5 L16 15.5 C16 16.3 15.3 17 14.5 17 L3.5 17 C2.7 17 2 16.3 2 15.5 L2 8.5 C2 7.7 2.7 7 3.5 7 Z",
            ),
            Soft("M16 10.5 L21.5 7.5 L21.5 16.5 L16 13.5 Z"),
        ],
        Icon::Sun => &[
            Disc(12.0, 12.0, 4.0),
            Line("M12 2.5 L12 4.5 M12 19.5 L12 21.5 M2.5 12 L4.5 12 M19.5 12 L21.5 12"),
            Line("M5.3 5.3 L6.7 6.7 M17.3 17.3 L18.7 18.7 M5.3 18.7 L6.7 17.3 M17.3 6.7 L18.7 5.3"),
        ],
        Icon::Bulb => &[
            Fill(
                "M9 15.5 C8.7 14.4 8.1 13.6 7.3 12.8 C6.2 11.7 5.5 10.2 5.5 8.5 C5.5 4.9 8.4 2 12 2 C15.6 2 18.5 4.9 18.5 8.5 C18.5 10.2 17.8 11.7 16.7 12.8 C15.9 13.6 15.3 14.4 15 15.5 Z",
            ),
            Line("M9 18.5 L15 18.5 M10 21.5 L14 21.5"),
        ],
        Icon::Cube => &[
            Line("M12 2.5 L20.5 7.25 L20.5 16.75 L12 21.5 L3.5 16.75 L3.5 7.25 Z"),
            Line("M3.5 7.25 L12 12 L20.5 7.25 M12 12 L12 21.5"),
        ],
        Icon::Sound => &[
            Soft("M11 5 L6.5 9 L3 9 L3 15 L6.5 15 L11 19 Z"),
            Line("M15.5 8.5 C16.4 9.4 17 10.6 17 12 C17 13.4 16.4 14.6 15.5 15.5"),
            Line("M18.5 5.5 C20.2 7.2 21.2 9.5 21.2 12 C21.2 14.5 20.2 16.8 18.5 18.5"),
        ],
        Icon::Text => &[Line(
            "M5 7 L5 4.5 L19 4.5 L19 7 M12 4.5 L12 19.5 M9 19.5 L15 19.5",
        )],
        Icon::Image => &[
            Line(
                "M5 3.5 L19 3.5 C19.8 3.5 20.5 4.2 20.5 5 L20.5 19 C20.5 19.8 19.8 20.5 19 20.5 L5 20.5 C4.2 20.5 3.5 19.8 3.5 19 L3.5 5 C3.5 4.2 4.2 3.5 5 3.5 Z",
            ),
            Disc(9.0, 9.0, 1.8),
            Line("M20.5 15 L15.5 10 L5 20.5"),
        ],
        Icon::Prefab => &[
            Line("M12 2.5 L21.5 12 L12 21.5 L2.5 12 Z"),
            Soft("M12 8 L16 12 L12 16 L8 12 Z"),
        ],
        Icon::Folder => &[Soft(
            "M4 20 L20 20 C21.1 20 22 19.1 22 18 L22 8.5 C22 7.4 21.1 6.5 20 6.5 L12.2 6.5 L10.4 4.3 C10 3.8 9.5 3.5 8.9 3.5 L4 3.5 C2.9 3.5 2 4.4 2 5.5 L2 18 C2 19.1 2.9 20 4 20 Z",
        )],
        Icon::Plus => &[Line("M12 5 L12 19 M5 12 L19 12")],
        Icon::Trash => &[
            Line("M3.5 6 L20.5 6"),
            Line(
                "M8.5 6 L8.5 4.5 C8.5 3.4 9.4 2.5 10.5 2.5 L13.5 2.5 C14.6 2.5 15.5 3.4 15.5 4.5 L15.5 6",
            ),
            Line(
                "M18.5 6 L17.7 19.6 C17.6 20.7 16.7 21.5 15.6 21.5 L8.4 21.5 C7.3 21.5 6.4 20.7 6.3 19.6 L5.5 6",
            ),
            Line("M10 10.5 L10 17 M14 10.5 L14 17"),
        ],
        Icon::Target => &[
            Ring(12.0, 12.0, 7.5),
            Disc(12.0, 12.0, 2.5),
            Line("M12 1.5 L12 4.5 M12 19.5 L12 22.5 M1.5 12 L4.5 12 M19.5 12 L22.5 12"),
        ],
        Icon::Dot => &[Disc(12.0, 12.0, 3.5)],
        Icon::Screen => &[
            Line(
                "M4 3.5 L20 3.5 C21.1 3.5 22 4.4 22 5.5 L22 15.5 C22 16.6 21.1 17.5 20 17.5 L4 17.5 C2.9 17.5 2 16.6 2 15.5 L2 5.5 C2 4.4 2.9 3.5 4 3.5 Z",
            ),
            Line("M8.5 21 L15.5 21 M12 17.5 L12 21"),
            Soft("M10 7.5 L14.5 10.5 L10 13.5 Z"),
        ],
    }
}

/// White, fully opaque (icons are drawn white and tinted when shown).
fn white() -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(255, 255, 255, 255)
}

fn path(data: &str) -> BezPath {
    BezPath::from_svg(data).unwrap_or_default()
}

/// Draws an icon `size` pixels square, white on transparent.
fn rasterize(icon: Icon, size: u16) -> ColorImage {
    let mut context = RenderContext::new(size, size);
    context.set_transform(Affine::scale(f64::from(size) / 24.0));
    context.set_paint(white());
    let round = |width: f64| {
        Stroke::new(width)
            .with_caps(Cap::Round)
            .with_join(Join::Round)
    };
    for part in parts(icon) {
        match part {
            Line(data) => {
                context.set_stroke(round(2.0));
                context.stroke_path(&path(data));
            }
            Fill(data) => context.fill_path(&path(data)),
            Soft(data) => {
                let shape = path(data);
                context.fill_path(&shape);
                context.set_stroke(round(1.5));
                context.stroke_path(&shape);
            }
            Ring(x, y, r) => {
                context.set_stroke(round(2.0));
                context.stroke_path(&Circle::new((*x, *y), *r).to_path(0.05));
            }
            Disc(x, y, r) => context.fill_path(&Circle::new((*x, *y), *r).to_path(0.05)),
        }
    }
    to_image(&context, size)
}

/// The engine's mark: a four-pointed spark with curved sides in a violet
/// gradient, a softer glow behind it, and a small spark beside it.
fn rasterize_logo(size: u16) -> ColorImage {
    let mut context = RenderContext::new(size, size);
    context.set_transform(Affine::scale(f64::from(size) / 100.0));
    // The glow.
    context.set_paint(Brush::Gradient(
        Gradient::new_radial((46.0, 54.0), 46.0).with_stops([
            AlphaColor::<Srgb>::from_rgba8(139, 108, 255, 70),
            AlphaColor::<Srgb>::from_rgba8(139, 108, 255, 0),
        ]),
    ));
    context.fill_path(&Circle::new((46.0, 54.0), 46.0).to_path(0.05));
    // The spark.
    context.set_paint(Brush::Gradient(
        Gradient::new_linear((20.0, 12.0), (72.0, 96.0)).with_stops([
            AlphaColor::<Srgb>::from_rgba8(206, 190, 255, 255),
            AlphaColor::<Srgb>::from_rgba8(139, 108, 255, 255),
            AlphaColor::<Srgb>::from_rgba8(98, 64, 230, 255),
        ]),
    ));
    context.fill_path(&path(
        "M46 10 C49 36 63 50 86 54 C63 58 49 72 46 98 C43 72 29 58 6 54 C29 50 43 36 46 10 Z",
    ));
    // A small one, up and to the right.
    context.set_paint(Brush::Gradient(
        Gradient::new_linear((74.0, 4.0), (92.0, 30.0)).with_stops([
            AlphaColor::<Srgb>::from_rgba8(240, 234, 255, 255),
            AlphaColor::<Srgb>::from_rgba8(178, 160, 255, 255),
        ]),
    ));
    context.fill_path(&path(
        "M82 4 C83 12 87 16 95 17 C87 18 83 22 82 30 C81 22 77 18 69 17 C77 16 81 12 82 4 Z",
    ));
    to_image(&context, size)
}

fn to_image(context: &RenderContext, size: u16) -> ColorImage {
    let mut pixmap = Pixmap::new(size, size);
    let mut resources = Resources::new();
    context.render(&mut pixmap, &mut resources);
    let pixels = pixmap
        .data_as_u8_slice()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| Color32::from_rgba_premultiplied(px[0], px[1], px[2], px[3]))
        .collect();
    ColorImage::new([usize::from(size), usize::from(size)], pixels)
}

/// The textures made so far, by icon (`None`: the mark) and size.
#[derive(Clone, Default)]
struct Made(BTreeMap<(Option<u8>, u16), TextureHandle>);

fn texture(
    context: &Context,
    key: (Option<u8>, u16),
    make: impl FnOnce() -> ColorImage,
) -> TextureHandle {
    let id = Id::unique("wish-vector-icons");
    if let Some(found) = context.data(|data| {
        data.get_temp::<Made>(id)
            .and_then(|made| made.0.get(&key).cloned())
    }) {
        return found;
    }
    let handle = context.load_texture(
        format!("icon-{:?}-{}", key.0, key.1),
        make(),
        TextureOptions::LINEAR,
    );
    context.data_mut(|data| {
        data.get_temp_mut_or_default::<Made>(id)
            .0
            .insert(key, handle.clone());
    });
    handle
}

/// The texture of `icon` at `size` pixels (white; tint it when drawing).
pub(crate) fn icon(context: &Context, icon: Icon, size: u16) -> TextureHandle {
    let size = size.clamp(8, 256);
    texture(context, (Some(icon as u8), size), || rasterize(icon, size))
}

/// The engine's mark at `size` pixels, in its own colours.
pub(crate) fn logo(context: &Context, size: u16) -> TextureHandle {
    let size = size.clamp(16, 512);
    texture(context, (None, size), || rasterize_logo(size))
}
