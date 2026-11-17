//! Vector icons and the engine's mark (M9; the wishing wand, M9c): each icon is SVG path data on a
//! 24-unit grid, drawn by `vello_cpu` (the rasterizer egui draws its text
//! with) with anti-aliased curves, round caps and joins, into a texture at
//! the exact pixel size it is shown at. Textures are made once per icon and
//! size and kept by the egui context.

use std::collections::BTreeMap;

use egui::{Color32, ColorImage, Context, Id, TextureHandle, TextureOptions};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, BezPath, Cap, Circle, Join, Point, Shape as _, Stroke};
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
        Icon::Wish => &[
            Line("M4.5 19.5 L13.2 10.8"),
            Soft(
                "M16.6 3.2 L17.9 6.1 L21 6.4 L18.7 8.5 L19.4 11.6 L16.6 10 L13.9 11.6 L14.5 8.5 L12.2 6.4 L15.3 6.1 Z",
            ),
            Disc(8.6, 6.6, 1.1),
            Disc(5.6, 9.4, 0.8),
        ],
        Icon::Chat => &[
            Line(
                "M5 4.5 L19 4.5 C20.4 4.5 21.5 5.6 21.5 7 L21.5 14.5 C21.5 15.9 20.4 17 19 17 L11 17 L6.5 20.5 L6.5 17 L5 17 C3.6 17 2.5 15.9 2.5 14.5 L2.5 7 C2.5 5.6 3.6 4.5 5 4.5 Z",
            ),
            Disc(8.0, 10.8, 1.3),
            Disc(12.0, 10.8, 1.3),
            Disc(16.0, 10.8, 1.3),
        ],
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

/// Clamps to 0..1.
fn unit(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

/// Where `t` is between `a` and `b`, as 0..1.
fn span(t: f64, a: f64, b: f64) -> f64 {
    unit((t - a) / (b - a))
}

/// Fast, then gentle.
fn ease_out(x: f64) -> f64 {
    1.0 - (1.0 - unit(x)).powi(3)
}

/// Goes a little past and settles (for what pops into place).
fn back_out(x: f64) -> f64 {
    let x = unit(x);
    1.0 + 2.9 * (x - 1.0).powi(3) + 1.9 * (x - 1.0).powi(2)
}

fn rgba(color: (u8, u8, u8), alpha: f64) -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(
        color.0,
        color.1,
        color.2,
        (unit(alpha) * 255.0).round() as u8,
    )
}

/// The wand's colour (on the editor's dark grounds).
const BONE: (u8, u8, u8) = (237, 230, 216);
/// The grip and the trail (the interface's accent).
const WIN: (u8, u8, u8) = (255, 107, 44);
/// The star and the motes.
const GOLD: (u8, u8, u8) = (255, 186, 92);

/// Smooth in and out.
fn ease_in_out(x: f64) -> f64 {
    let x = unit(x);
    x * x * (3.0 - 2.0 * x)
}

/// A straight piece with round ends.
fn piece(context: &mut RenderContext, from: Point, to: Point, width: f64) {
    context.set_stroke(
        Stroke::new(width)
            .with_caps(Cap::Round)
            .with_join(Join::Round),
    );
    let mut line = BezPath::new();
    line.move_to(from);
    line.line_to(to);
    context.stroke_path(&line);
}

/// Where the wand turns (its handle's end), how long it is, its slant at
/// rest and where its wave starts (radians; y grows downward).
const HANDLE: Point = Point::new(22.0, 88.0);
const LENGTH: f64 = 70.0;
const REST: f64 = -0.86;
const FROM: f64 = -1.75;
/// The motes the trail leaves behind the tip: how far back along the
/// swing, their size and strength.
const MOTES: [(f64, f64, f64); 4] = [
    (0.3, 3.0, 0.9),
    (0.52, 2.4, 0.65),
    (0.76, 1.8, 0.42),
    (1.0, 1.3, 0.25),
];

/// A point on the circle the tip sweeps, `r` from the handle.
fn on_swing(r: f64, angle: f64) -> Point {
    Point::new(HANDLE.x + r * angle.cos(), HANDLE.y + r * angle.sin())
}

/// A five-pointed star at `centre`, `radius` to its points, turned by
/// `turn` radians, its corners rounded by a stroke `round` wide.
fn star(
    context: &mut RenderContext,
    centre: Point,
    radius: f64,
    turn: f64,
    round: f64,
    color: AlphaColor<Srgb>,
) {
    let mut outline = BezPath::new();
    for i in 0..10u8 {
        let reach = if i % 2 == 0 { radius } else { radius * 0.5 };
        let angle = turn - std::f64::consts::FRAC_PI_2 + f64::from(i) * std::f64::consts::PI / 5.0;
        let corner = Point::new(
            centre.x + reach * angle.cos(),
            centre.y + reach * angle.sin(),
        );
        if i == 0 {
            outline.move_to(corner);
        } else {
            outline.line_to(corner);
        }
    }
    outline.close_path();
    context.set_paint(color);
    context.fill_path(&outline);
    if round > 0.0 {
        context.set_stroke(
            Stroke::new(round)
                .with_caps(Cap::Round)
                .with_join(Join::Round),
        );
        context.stroke_path(&outline);
    }
}

/// The star's size, and the rod's length up to it.
const STAR: f64 = 12.0;
const ROD: f64 = LENGTH - 6.0;

/// The engine's mark, a wishing wand (dilek çubuğu), on a 100-unit grid:
/// a bone-coloured rod with an orange grip and a soft gold five-pointed
/// star at its tip. `t` runs over its
/// start-up animation (0 to 1; 1 and after: the still mark): the wand is
/// raised and swept up, its tip drawing an arc; as it stops (with a little
/// flick) the star turns into place and lights up, and the arc falls apart
/// into motes that fade.
fn draw_wand(context: &mut RenderContext, t: f64) {
    let shown = ease_out(span(t, 0.0, 0.12));
    let swing = ease_in_out(span(t, 0.05, 0.5));
    let settle = back_out(span(t, 0.45, 0.7));
    let over = 0.16;
    let angle = if t < 0.5 {
        FROM + (REST - over - FROM) * swing
    } else {
        (REST - over) + over * settle
    };
    let tip = on_swing(ROD, angle);
    let r = ROD + 4.0;
    // The arc while sweeping, fading as the motes come.
    let apart = ease_in_out(span(t, 0.5, 0.75));
    let end = angle - 0.14;
    let start = FROM.max(end - 1.2);
    if end > start + 0.02 && shown > 0.0 && apart < 1.0 {
        let mut previous = on_swing(r, start);
        for i in 1..=50 {
            let k = f64::from(i) / 50.0;
            let next = on_swing(r, start + (end - start) * k);
            context.set_paint(rgba(WIN, 0.85 * k * shown * (1.0 - apart)));
            piece(context, previous, next, 0.6 + 3.9 * k);
            previous = next;
        }
    }
    // The motes: only in the animation; they fade before it ends, so the
    // still mark is the wand and its star alone.
    let gone = span(t, 0.8, 1.0);
    for (i, (back, size, strength)) in MOTES.iter().enumerate() {
        let delay = 0.05 * f64::from(u8::try_from(i).unwrap_or(0));
        let come = back_out(span(t, 0.5 + delay, 0.72 + delay));
        if come > 0.0 && gone < 1.0 {
            context.set_paint(rgba(GOLD, strength * unit(come) * (1.0 - gone)));
            context.fill_path(&Circle::new(on_swing(r, REST - back), size * come).to_path(0.05));
        }
    }
    // The rod and its grip.
    let along = |k: f64| HANDLE.lerp(tip, k);
    context.set_paint(rgba(BONE, shown));
    let mut previous = HANDLE;
    for i in 1..=30 {
        let k = f64::from(i) / 30.0;
        let next = along(k);
        piece(context, previous, next, 9.0 - 3.5 * k);
        previous = next;
    }
    let direction = (tip - HANDLE).normalize();
    let across = vello_cpu::kurbo::Vec2::new(-direction.y, direction.x) * 5.0;
    context.set_paint(rgba(WIN, shown));
    for k in [0.2, 0.28] {
        piece(context, along(k) - across, along(k) + across, 2.4);
    }
    // The star: small while the wand is waved, then it turns into place,
    // grows and glows.
    let centre = tip + direction * (STAR * 0.55);
    let glow = span(t, 0.5, 0.95);
    if glow > 0.0 && glow < 1.0 {
        let strength = (glow * std::f64::consts::PI).sin();
        let reach = STAR * 2.2;
        context.set_paint(Brush::Gradient(
            Gradient::new_radial(centre, reach as f32)
                .with_stops([rgba(GOLD, 0.55 * strength), rgba(GOLD, 0.0)]),
        ));
        context.fill_path(&Circle::new(centre, reach).to_path(0.05));
    }
    let grown = if t < 0.45 {
        0.45
    } else {
        back_out(span(t, 0.45, 0.68)).max(0.45)
    };
    let turning = (1.0 - span(t, 0.45, 0.7)) * std::f64::consts::TAU / 5.0;
    star(
        context,
        centre,
        STAR * grown,
        0.22 + turning,
        5.0 * grown,
        rgba(GOLD, shown),
    );
}

/// The mark at `size` pixels, at moment `t` of its animation.
fn rasterize_logo(size: u16, t: f64) -> ColorImage {
    let mut context = RenderContext::new(size, size);
    // The still mark spans about 17..83 across and 20..93 down: centred.
    context.set_transform(Affine::scale(f64::from(size) / 100.0) * Affine::translate((0.0, -6.5)));
    draw_wand(&mut context, t);
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
    texture(context, (None, size), || rasterize_logo(size, 1.0))
}

/// The application's icon, `size` pixels square: the mark on a dark tile
/// with rounded corners. Straight (not premultiplied) RGBA.
pub(crate) fn app_icon(size: u16) -> Vec<u8> {
    let size = size.clamp(16, 512);
    let mut context = RenderContext::new(size, size);
    let unit = f64::from(size) / 128.0;
    context.set_transform(Affine::scale(unit));
    context.set_paint(rgba((29, 28, 25), 1.0));
    context
        .fill_path(&vello_cpu::kurbo::RoundedRect::new(0.0, 0.0, 128.0, 128.0, 28.0).to_path(0.05));
    context.set_transform(Affine::scale(unit) * Affine::translate((14.0, 7.5)));
    draw_wand(&mut context, 1.0);
    let mut pixmap = Pixmap::new(size, size);
    let mut resources = Resources::new();
    context.render(&mut pixmap, &mut resources);
    pixmap
        .data_as_u8_slice()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| {
            let alpha = px[3];
            let straight = |c: u8| {
                if alpha == 0 {
                    0
                } else {
                    ((u16::from(c) * 255 + u16::from(alpha) / 2) / u16::from(alpha)).min(255) as u8
                }
            };
            [straight(px[0]), straight(px[1]), straight(px[2]), alpha]
        })
        .collect()
}

/// The mark at moment `t` of its start-up animation, in one texture that is
/// drawn again each time (the animation's frames are not kept).
pub(crate) fn logo_moment(context: &Context, size: u16, t: f64) -> TextureHandle {
    let size = size.clamp(16, 512);
    let image = rasterize_logo(size, t);
    let id = Id::unique("wish-logo-moment");
    let kept: Option<TextureHandle> = context.data(|data| data.get_temp(id));
    match kept {
        Some(mut handle) => {
            handle.set(image, TextureOptions::LINEAR);
            handle
        }
        None => {
            let handle = context.load_texture("logo-moment", image, TextureOptions::LINEAR);
            context.data_mut(|data| data.insert_temp(id, handle.clone()));
            handle
        }
    }
}
