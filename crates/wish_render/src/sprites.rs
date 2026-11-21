//! Sprites and particles (design notes M6 §8–§9 and
//! `docs/design/1.1-sprites.md`), drawn after the 3D scene and before the
//! interface.
//!
//! Both are flat, unlit rectangles in the world: a sprite stands in its
//! entity's XY plane, a particle faces the camera. They are tested against
//! the scene's depth (a wall in front hides them) but do not write it, and
//! are drawn back to front: by `layer` (particles are on layer 0), then
//! farthest first.
//!
//! **Batches.** Each rectangle is one instance of a unit quad: its four
//! corners (worked out on the CPU, so a sprite lands on the same pixels it
//! always did), the part of its picture it shows, its tint. Pictures are
//! uploaded to the GPU once, each as a texture of its own, and kept until
//! the picture changes; consecutive rectangles (in drawing order) with the
//! same picture and the same sampling are one draw call. A thousand sprites
//! of one sheet are one draw.
//!
//! **Sheets.** A sprite whose `SpriteAnimator` names a sprite sheet shows
//! the sheet's picture, cut to the frame the animator says
//! ([`SpriteSheet`], given by whoever reads the project's files, as
//! pictures are).
//!
//! **Tilemaps.** A tilemap's tiles are rectangles of the same batches
//! ([`TileGrid`], given like sheets): one per tile, its corners worked out
//! from whole cell coordinates so that neighbours share exactly the same
//! corner values (no seams, no overlaps); chunks outside the picture are
//! skipped. A map sorts as one flat thing: every tile has the map's layer
//! and its entity's distance, and comes before the sprites of the same
//! layer and distance. With `Tilemap.y_sort`, the sprites of the map's layer
//! sort by height (design note `docs/design/1.1-tilemaps.md` §6).
//!
//! **Nine slices.** A sprite with `slice` borders drawn at a `size` other
//! than its picture's is nine rectangles: the corners keep their size, the
//! edges and the middle stretch.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use wish_core::math::{Mat4, Vec3, Vec4};
use wish_core::{Diagnostic, Entity};

use crate::extract::{CameraView, RenderScene, SpriteView, TilemapView};
use crate::gpu::Gpu;
use crate::renderer::Pool;
use crate::ui::UiPicture;

/// Bytes per instance: four corners (12 floats), the picture's part (4),
/// the colour (4), the disc flag (1).
const INSTANCE_BYTES: u64 = 84;
/// The size of the overlay uniform: one matrix.
const UNIFORM_BYTES: u64 = 64;
/// The most particles drawn in one picture.
pub const MAX_PARTICLES: usize = 100_000;
/// The most rectangles one picture draws (sprites' slices and particles
/// together).
const MAX_INSTANCES: usize = 1_000_000;
/// The texel format of sprite pictures: sRGB colours, like the interface's.
const PICTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// A sprite sheet, as the renderer needs it: its picture, its frames and
/// its clips (`wish_asset::Sheet`, given by whoever reads the project's
/// files: the renderer never touches the disk).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpriteSheet {
    /// The picture's project path (kept like any sprite picture,
    /// `Renderer::set_ui_picture`).
    pub image: String,
    /// Each frame: left, top, width, height in the picture's pixels.
    pub frames: Vec<[u32; 4]>,
    /// Each clip's frames (sheet frame numbers, in order), by the clip's
    /// name.
    pub clips: BTreeMap<String, Vec<u32>>,
}

/// The side of a tilemap chunk, in cells (the layout of
/// `wish_asset::tiles::ChunkTiles`).
pub const TILE_CHUNK: i32 = 16;

/// A tilemap, as the renderer needs it: its tileset's picture, every
/// tile's rectangle in it, and the layers' chunks (given by whoever reads
/// the project's files, like sheets: the renderer never touches the disk).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileGrid {
    /// The tileset picture's project path (kept like any sprite picture,
    /// `Renderer::set_ui_picture`).
    pub image: String,
    /// Each tile: left, top, width, height in the picture's pixels.
    pub tiles: Vec<[u32; 4]>,
    /// The layers, drawn in this order.
    pub layers: Vec<TileGridLayer>,
}

/// One layer of a [`TileGrid`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileGridLayer {
    /// Its name.
    pub name: String,
    /// Its chunks by chunk coordinates: 256 tile numbers each, row by row
    /// from the chunk's bottom row, -1 for an empty cell.
    pub chunks: BTreeMap<(i32, i32), Arc<[i32; 256]>>,
}

impl TileGrid {
    /// The tile of layer `layer` at cell `(x, y)` (-1 for none).
    pub fn tile(&self, layer: usize, x: i32, y: i32) -> i32 {
        let key = (x.div_euclid(TILE_CHUNK), y.div_euclid(TILE_CHUNK));
        let index = (y.rem_euclid(TILE_CHUNK) * TILE_CHUNK + x.rem_euclid(TILE_CHUNK)) as usize;
        self.layers
            .get(layer)
            .and_then(|layer| layer.chunks.get(&key))
            .map_or(-1, |chunk| chunk[index])
    }
}

/// Whether every number of a matrix is finite.
fn finite(matrix: &Mat4) -> bool {
    matrix.to_cols_array().iter().all(|v| v.is_finite())
}

/// The corner of cell corner `(x, y)` of a map in the world: computed the
/// same way for every tile that shares it.
fn cell_corner(view: &TilemapView, cell: [f32; 2], x: i32, y: i32) -> Vec3 {
    view.world
        .transform_point3(Vec3::new(x as f32 * cell[0], y as f32 * cell[1], 0.0))
}

/// Whether a quad with these corners may be in the picture of `view_proj`
/// (conservative: one that reaches behind the camera counts).
fn may_show(corners: &[Vec3; 4], view_proj: &Mat4) -> bool {
    let mut low = [f32::MAX; 2];
    let mut high = [f32::MIN; 2];
    let mut behind = 0;
    for corner in corners {
        let clip: Vec4 = *view_proj * corner.extend(1.0);
        if clip.w <= 1e-6 {
            behind += 1;
            continue;
        }
        let (x, y) = (clip.x / clip.w, clip.y / clip.w);
        low = [low[0].min(x), low[1].min(y)];
        high = [high[0].max(x), high[1].max(y)];
    }
    if behind == 4 {
        return false;
    }
    if behind > 0 {
        return true;
    }
    high[0] >= -1.0 && low[0] <= 1.0 && high[1] >= -1.0 && low[1] <= 1.0
}

/// The tiles of a map that may show through `view_proj`: (layer, cell x,
/// cell y, tile), layer by layer, chunk by chunk; chunks outside the
/// picture are skipped whole.
fn visible_tiles<'a>(
    view: &'a TilemapView,
    grid: &'a TileGrid,
    view_proj: &'a Mat4,
) -> impl Iterator<Item = (usize, i32, i32, u32)> + 'a {
    let cell = view.tilemap.cell();
    grid.layers
        .iter()
        .enumerate()
        .flat_map(move |(layer, tiles)| {
            tiles
                .chunks
                .iter()
                .filter(move |&(&(cx, cy), _)| {
                    let (x0, y0) = (cx * TILE_CHUNK, cy * TILE_CHUNK);
                    let (x1, y1) = (x0 + TILE_CHUNK, y0 + TILE_CHUNK);
                    may_show(
                        &[
                            cell_corner(view, cell, x0, y0),
                            cell_corner(view, cell, x1, y0),
                            cell_corner(view, cell, x1, y1),
                            cell_corner(view, cell, x0, y1),
                        ],
                        view_proj,
                    )
                })
                .flat_map(move |(&(cx, cy), chunk)| {
                    chunk.iter().enumerate().filter_map(move |(index, tile)| {
                        let tile = u32::try_from(*tile).ok()?;
                        let index = index as i32;
                        Some((
                            layer,
                            cx * TILE_CHUNK + index % TILE_CHUNK,
                            cy * TILE_CHUNK + index / TILE_CHUNK,
                            tile,
                        ))
                    })
                })
        })
}

/// The four corners of a cell of a map (bottom left, bottom right, top
/// right, top left).
fn cell_quad(view: &TilemapView, cell: [f32; 2], x: i32, y: i32) -> [Vec3; 4] {
    [
        cell_corner(view, cell, x, y),
        cell_corner(view, cell, x + 1, y),
        cell_corner(view, cell, x + 1, y + 1),
        cell_corner(view, cell, x, y + 1),
    ]
}

/// What a sprite shows once its animator is taken into account.
struct Shown<'a> {
    /// The picture's path, if any.
    image: Option<&'a str>,
    /// The part of it, as `Sprite.rect` says it.
    rect: [f32; 4],
    /// For a sprite with a sheet: the clip's name, the frame in the clip
    /// (or the still frame), and the sheet's frame number.
    frame: Option<(&'a str, u32, u32)>,
}

/// What a sprite shows: its own picture and rect, or (with a sheet that is
/// known) the sheet's picture and frame. A clip or a frame the sheet does
/// not have shows the nearest the sheet has, with a warning in `warnings`.
fn shown<'a>(
    view: &'a SpriteView,
    sheets: &'a BTreeMap<String, SpriteSheet>,
    names: &dyn Fn(Entity) -> String,
    warnings: Option<&mut Vec<Diagnostic>>,
) -> Shown<'a> {
    let own = Shown {
        image: view.image.as_deref(),
        rect: view.sprite.rect,
        frame: None,
    };
    let Some(animation) = &view.animation else {
        return own;
    };
    let Some(sheet) = sheets.get(&animation.sheet) else {
        // The loader said why (`sheet_not_loaded`).
        return own;
    };
    let last = sheet.frames.len().saturating_sub(1) as u32;
    let mut problem = None;
    let (in_clip, number) = if animation.clip.is_empty() {
        if animation.frame > last {
            problem = Some(Diagnostic::warning(
                "sprite_frame_not_found",
                format!(
                    "Entity {} shows frame {} of the sheet '{}', which has {} frames; its last frame is shown.",
                    names(view.entity),
                    animation.frame,
                    animation.sheet,
                    sheet.frames.len()
                ),
                "Without a clip, SpriteAnimator.frame is the sheet's own frame number, from 0 (left to right, then down, in a grid). asset_info on the sheet lists its frames.",
            ));
        }
        (animation.frame, animation.frame.min(last))
    } else {
        match sheet.clips.get(&animation.clip) {
            Some(frames) => {
                let at = (animation.frame as usize).min(frames.len().saturating_sub(1));
                (
                    animation.frame,
                    frames.get(at).copied().unwrap_or(0).min(last),
                )
            }
            None => {
                let names_of: Vec<&str> = sheet.clips.keys().map(String::as_str).collect();
                problem = Some(Diagnostic::warning(
                    "sprite_clip_not_found",
                    format!(
                        "Entity {} plays the clip '{}', which the sheet '{}' does not have; its first frame is shown.",
                        names(view.entity),
                        animation.clip,
                        animation.sheet
                    ),
                    format!(
                        "{} The sheet's clips are: {}.",
                        wish_core::suggest::unknown_name_hint(&animation.clip, &names_of),
                        if names_of.is_empty() {
                            "(none)".to_owned()
                        } else {
                            names_of.join(", ")
                        }
                    ),
                ));
                (0, 0)
            }
        }
    };
    if let (Some(problem), Some(warnings)) = (problem, warnings) {
        warnings.push(problem.at_file(animation.sheet.clone()));
    }
    let [x, y, w, h] = sheet.frames.get(number as usize).copied().unwrap_or([0; 4]);
    Shown {
        image: Some(sheet.image.as_str()),
        rect: [x as f32, y as f32, w as f32, h as f32],
        frame: Some((animation.clip.as_str(), in_clip, number)),
    }
}

/// The part of `picture` a sprite shows, in its pixels (left, top, width,
/// height), clamped to the picture.
fn source_rect(rect: [f32; 4], picture: &UiPicture) -> (u32, u32, u32, u32) {
    let [x, y, w, h] = rect.map(|v| if v.is_finite() { v.max(0.0) as u32 } else { 0 });
    if w == 0 || h == 0 {
        return (0, 0, picture.width, picture.height);
    }
    let x = x.min(picture.width.saturating_sub(1));
    let y = y.min(picture.height.saturating_sub(1));
    (
        x,
        y,
        w.min(picture.width - x).max(1),
        h.min(picture.height - y).max(1),
    )
}

/// A number written in a sprite that may be anything game code wrote: a
/// finite number at least 0, or 0.
fn clean(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

/// One rectangle of a sprite: its corners in world space (bottom left,
/// bottom right, top right, top left) and the part of the picture it shows
/// in the picture's pixels (left, top, right, bottom), flips applied (a
/// flipped piece has its left greater than its right).
type Piece = ([Vec3; 4], [f32; 4]);

/// A sprite's rectangles (one, or up to nine when it is sliced), and the
/// four corners of the whole sprite; `None` when it cannot be drawn (its
/// picture is not loaded, or its placement is not finite).
fn sprite_pieces(
    view: &SpriteView,
    shown: &Shown<'_>,
    pictures: &BTreeMap<String, UiPicture>,
) -> Option<(Vec<Piece>, [Vec3; 4])> {
    let sprite = &view.sprite;
    let picture = shown.image.and_then(|path| pictures.get(path));
    if shown.image.is_some() && picture.is_none() {
        return None;
    }
    let [_, _, rw, rh] = shown.rect.map(clean);
    let source = picture.map(|picture| source_rect(shown.rect, picture));
    let (mut width, mut height) = match source {
        // The part of the picture shown (a rect reaching past the picture
        // is cut to it, and so is the sprite: no stretched edge pixels).
        Some((_, _, w, h)) => (w as f32, h as f32),
        None if rw > 0.0 && rh > 0.0 => (rw, rh),
        None => (100.0, 100.0),
    };
    let [sw, sh] = sprite.size.map(clean);
    if sw > 0.0 && sh > 0.0 {
        (width, height) = (sw, sh);
    }
    let ppu = if sprite.pixels_per_unit.is_finite() && sprite.pixels_per_unit > 0.0 {
        sprite.pixels_per_unit
    } else {
        100.0
    };
    let (w, h) = (width / ppu, height / ppu);
    let [ax, ay] = sprite.anchor.map(|v| if v.is_finite() { v } else { 0.5 });
    let (left, bottom) = (-ax * w, -ay * h);
    let outline = [
        Vec3::new(left, bottom, 0.0),
        Vec3::new(left + w, bottom, 0.0),
        Vec3::new(left + w, bottom + h, 0.0),
        Vec3::new(left, bottom + h, 0.0),
    ]
    .map(|corner| view.world.transform_point3(corner));
    if !outline.iter().all(|c| c.is_finite()) {
        return None;
    }
    let Some((sx, sy, spw, sph)) = source else {
        // A plain rectangle: one piece, no picture.
        return Some((vec![(outline, [0.0; 4])], outline));
    };
    let (sx, sy, spw, sph) = (sx as f32, sy as f32, spw as f32, sph as f32);
    let flip = |[l, t, r, b]: [f32; 4]| {
        let (l, r) = if sprite.flip_x { (r, l) } else { (l, r) };
        let (t, b) = if sprite.flip_y { (b, t) } else { (t, b) };
        [l, t, r, b]
    };
    if !sprite.is_sliced() {
        return Some((vec![(outline, flip([sx, sy, sx + spw, sy + sph]))], outline));
    }
    // Nine slices. Borders keep to the picture's part and to the size
    // drawn (both shrink in proportion when they do not fit).
    let [mut bl, mut bt, mut br, mut bb] = sprite.slice.map(clean);
    for (low, high, room) in [(&mut bl, &mut br, spw), (&mut bt, &mut bb, sph)] {
        if *low + *high > room {
            let scale = room / (*low + *high);
            (*low, *high) = (*low * scale, *high * scale);
        }
    }
    let fit = |low: f32, high: f32, room: f32| {
        if low + high > room && low + high > 0.0 {
            let scale = room / (low + high);
            (low * scale, high * scale)
        } else {
            (low, high)
        }
    };
    let (ol, or) = fit(bl, br, width);
    let (ot, ob) = fit(bt, bb, height);
    // Edges across, in drawn pixels from the left and in picture pixels;
    // down, from the top.
    let across = [0.0, ol, width - or, width];
    let from = [sx, sx + bl, sx + spw - br, sx + spw];
    let down = [0.0, ot, height - ob, height];
    let from_top = [sy, sy + bt, sy + sph - bb, sy + sph];
    let mut pieces = Vec::with_capacity(9);
    for row in 0..3 {
        for column in 0..3 {
            let (x0, x1) = (across[column], across[column + 1]);
            let (top, low) = (down[row], down[row + 1]);
            let (u0, u1) = (from[column], from[column + 1]);
            let (v0, v1) = (from_top[row], from_top[row + 1]);
            // Nothing to show, or nowhere to show it.
            if x1 - x0 <= 0.0 || low - top <= 0.0 || u1 - u0 <= 0.0 || v1 - v0 <= 0.0 {
                continue;
            }
            // Mirroring flips where a piece goes as well as its picture.
            let (x0, x1) = if sprite.flip_x {
                (width - x1, width - x0)
            } else {
                (x0, x1)
            };
            let (top, low) = if sprite.flip_y {
                (height - low, height - top)
            } else {
                (top, low)
            };
            // Drawn pixels from the bottom left, in meters.
            let (y0, y1) = (height - low, height - top);
            let at = |x: f32, y: f32| {
                view.world
                    .transform_point3(Vec3::new(left + x / ppu, bottom + y / ppu, 0.0))
            };
            let corners = [at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)];
            if corners.iter().all(|c| c.is_finite()) {
                pieces.push((corners, flip([u0, v0, u1, v1])));
            }
        }
    }
    Some((pieces, outline))
}

/// One rectangle to draw.
struct Item {
    corners: [Vec3; 4],
    /// Texture coordinates: left, top, right, bottom (0 to 1).
    uv: [f32; 4],
    /// The picture, or `None` for plain colour (a white texel).
    texture: Option<String>,
    colour: [f32; 4],
    disc: bool,
    smooth: bool,
    layer: i32,
    distance: f32,
    /// In a layer whose sprites sort by height (`Tilemap.y_sort`): the
    /// height that decides (higher first); tiles come first of all.
    height: Option<f32>,
}

/// Consecutive instances drawn with one picture and one sampler.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Batch {
    /// The picture, or `None` for the white texel.
    texture: Option<String>,
    smooth: bool,
    first: u32,
    count: u32,
}

/// The overlay of one picture, ready for the GPU.
pub(crate) struct Overlay {
    instances: Vec<u8>,
    batches: Vec<Batch>,
    view_proj: Mat4,
}

impl Overlay {
    /// How many draw calls and how many triangles drawing it takes.
    pub(crate) fn counts(&self) -> (u32, u64) {
        (
            u32::try_from(self.batches.len()).unwrap_or(u32::MAX),
            self.instances.len() as u64 / INSTANCE_BYTES * 2,
        )
    }
}

/// What the renderer keeps that sprites and tilemaps show: pictures,
/// sprite sheets and tilemaps, by project path.
pub(crate) type Kept<'a> = (
    &'a BTreeMap<String, UiPicture>,
    &'a BTreeMap<String, SpriteSheet>,
    &'a BTreeMap<String, TileGrid>,
);

/// Prepares the sprites and particles of `scene` seen through `camera` in
/// a picture of `aspect`; `None` when there are none. What could not be
/// shown as written is added to `warnings`.
pub(crate) fn build(
    scene: &RenderScene,
    camera: &CameraView,
    aspect: f32,
    (pictures, sheets, grids): Kept<'_>,
    warnings: &mut Vec<Diagnostic>,
) -> Option<Overlay> {
    if scene.sprites.is_empty() && scene.emitters.is_empty() && scene.tilemaps.is_empty() {
        return None;
    }
    let mut items: Vec<Item> = Vec::new();
    let distance = |point: Vec3| -camera.view.transform_point3(point).z;
    let names = |entity: Entity| scene.name_of(entity);
    let view_proj = camera.projection(aspect) * camera.view;
    // The layers whose sprites sort by height.
    let by_height: BTreeSet<i32> = scene
        .tilemaps
        .iter()
        .filter(|view| view.tilemap.y_sort)
        .map(|view| view.tilemap.layer)
        .collect();
    // Tilemaps first: at the same layer and distance, a map is under the
    // sprites standing on it.
    for view in &scene.tilemaps {
        let Some(grid) = grids.get(&view.map) else {
            // The loader said why (`tilemap_not_loaded`).
            continue;
        };
        let Some(picture) = pictures.get(&grid.image) else {
            continue;
        };
        let colour = view.tilemap.color.to_array();
        if !colour.iter().all(|c| c.is_finite()) || !finite(&view.world) {
            continue;
        }
        let cell = view.tilemap.cell();
        let depth = distance(view.world.transform_point3(Vec3::ZERO));
        let (pw, ph) = (picture.width.max(1) as f32, picture.height.max(1) as f32);
        let height = by_height
            .contains(&view.tilemap.layer)
            .then_some(f32::INFINITY);
        for (_, x, y, tile) in visible_tiles(view, grid, &view_proj) {
            let Some([tx, ty, tw, th]) = grid.tiles.get(tile as usize).copied() else {
                // A tile the tileset lacks draws nothing (`check` says so).
                continue;
            };
            let (l, t) = (tx as f32, ty as f32);
            let (r, b) = (l + tw as f32, t + th as f32);
            items.push(Item {
                corners: cell_quad(view, cell, x, y),
                uv: [l / pw, t / ph, r / pw, b / ph],
                texture: Some(grid.image.clone()),
                colour,
                disc: false,
                smooth: view.tilemap.smooth,
                layer: view.tilemap.layer,
                distance: depth,
                height,
            });
        }
    }
    for view in &scene.sprites {
        let shown = shown(view, sheets, &names, Some(warnings));
        let Some((pieces, outline)) = sprite_pieces(view, &shown, pictures) else {
            continue;
        };
        let colour = view.sprite.color.to_array();
        if !colour.iter().all(|c| c.is_finite()) {
            continue;
        }
        let picture = shown
            .image
            .and_then(|path| pictures.get(path).map(|p| (path, p)));
        let centre = (outline[0] + outline[2]) * 0.5;
        let height = by_height
            .contains(&view.sprite.layer)
            .then(|| view.world.transform_point3(Vec3::ZERO).y);
        for (corners, [l, t, r, b]) in pieces {
            let (texture, uv) = match picture {
                Some((path, picture)) => {
                    let (pw, ph) = (picture.width.max(1) as f32, picture.height.max(1) as f32);
                    (Some(path.to_owned()), [l / pw, t / ph, r / pw, b / ph])
                }
                None => (None, [0.5; 4]),
            };
            items.push(Item {
                corners,
                uv,
                texture,
                colour,
                disc: false,
                smooth: view.sprite.smooth,
                layer: view.sprite.layer,
                distance: distance(centre),
                height,
            });
        }
    }
    // Particles face the camera.
    let to_world = camera.view.inverse();
    let right = to_world.transform_vector3(Vec3::X).normalize_or(Vec3::X);
    let up = to_world.transform_vector3(Vec3::Y).normalize_or(Vec3::Y);
    let mut particles = 0usize;
    for view in &scene.emitters {
        let budget = MAX_PARTICLES.saturating_sub(particles);
        if budget == 0 {
            break;
        }
        for particle in wish_particles::particles(&view.emitter, &view.world, scene.time, budget) {
            if particle.size <= 0.0 || !particle.position.is_finite() {
                continue;
            }
            particles += 1;
            let (r, u) = (right * particle.size * 0.5, up * particle.size * 0.5);
            let p = particle.position;
            items.push(Item {
                corners: [p - r - u, p + r - u, p + r + u, p - r + u],
                uv: [0.0, 0.0, 1.0, 1.0],
                texture: None,
                colour: particle.color,
                disc: true,
                smooth: true,
                layer: 0,
                distance: distance(p),
                height: by_height.contains(&0).then_some(p.y),
            });
        }
    }
    if items.is_empty() {
        return None;
    }
    if items.len() > MAX_INSTANCES {
        warnings.push(Diagnostic::warning(
            "sprites_cut_short",
            format!(
                "The picture has {} sprite rectangles and particles; the first {MAX_INSTANCES} (in entity order) are drawn.",
                items.len()
            ),
            "Fewer sprites or particles on screen at once: a nine-slice sprite is up to nine rectangles.",
        ));
        items.truncate(MAX_INSTANCES);
    }
    // Back to front: lower layers first, then (in a layer sorted by
    // height) higher first, then farther first. Stable, so a sliced
    // sprite's pieces and a map's tiles stay together and in order.
    items.sort_by(|a, b| {
        a.layer
            .cmp(&b.layer)
            .then_with(|| match (a.height, b.height) {
                (Some(ha), Some(hb)) => hb.total_cmp(&ha),
                _ => std::cmp::Ordering::Equal,
            })
            .then(b.distance.total_cmp(&a.distance))
    });
    let mut instances = Vec::with_capacity(items.len() * INSTANCE_BYTES as usize);
    let mut batches: Vec<Batch> = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        let disc = if item.disc { 1.0f32 } else { 0.0 };
        for value in item
            .corners
            .iter()
            .flat_map(|c| c.to_array())
            .chain(item.uv)
            .chain(item.colour)
            .chain([disc])
        {
            instances.extend_from_slice(&value.to_le_bytes());
        }
        let first = u32::try_from(index).unwrap_or(u32::MAX);
        match batches.last_mut() {
            Some(batch) if batch.smooth == item.smooth && batch.texture == item.texture => {
                batch.count += 1;
            }
            _ => batches.push(Batch {
                texture: item.texture,
                smooth: item.smooth,
                first,
                count: 1,
            }),
        }
    }
    Some(Overlay {
        instances,
        batches,
        view_proj,
    })
}

/// What `describe` says about a sprite or an emitter.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayElement {
    /// The entity.
    pub entity: Entity,
    /// Its box in the picture: left, top, right, bottom, in pixels; `None`
    /// when it is not in front of the camera.
    pub bounding_box: Option<[i32; 4]>,
    /// For an emitter, how many particles are alive; `None` for a sprite.
    pub particles: Option<u32>,
    /// For a sprite, its layer.
    pub layer: Option<i32>,
    /// For a sprite showing a sheet: the clip (empty: none), the frame its
    /// animator says (its place in the clip, or without a clip the sheet's
    /// frame), and the sheet's frame number it shows.
    pub animation: Option<(String, u32, u32)>,
}

/// The box `points` cover in a `width` × `height` picture through
/// `view_proj`, if any of them is in front of the camera and the box
/// touches the picture.
fn picture_box(points: &[Vec3], view_proj: &Mat4, width: u32, height: u32) -> Option<[i32; 4]> {
    let mut low = [f32::MAX; 2];
    let mut high = [f32::MIN; 2];
    let mut any = false;
    for point in points {
        let clip: Vec4 = *view_proj * point.extend(1.0);
        if clip.w <= 1e-6 {
            continue;
        }
        let x = (clip.x / clip.w * 0.5 + 0.5) * width as f32;
        let y = (0.5 - clip.y / clip.w * 0.5) * height as f32;
        low = [low[0].min(x), low[1].min(y)];
        high = [high[0].max(x), high[1].max(y)];
        any = true;
    }
    let inside = any
        && high[0] >= 0.0
        && high[1] >= 0.0
        && low[0] <= width as f32
        && low[1] <= height as f32;
    inside.then(|| {
        [
            low[0].max(0.0).round() as i32,
            low[1].max(0.0).round() as i32,
            high[0].min(width as f32).round() as i32,
            high[1].min(height as f32).round() as i32,
        ]
    })
}

/// The sprites and emitters of `scene` as a `width` × `height` picture seen
/// through `camera` shows them.
pub(crate) fn elements(
    scene: &RenderScene,
    camera: &CameraView,
    (width, height): (u32, u32),
    pictures: &BTreeMap<String, UiPicture>,
    sheets: &BTreeMap<String, SpriteSheet>,
) -> Vec<OverlayElement> {
    let view_proj = camera.projection(width as f32 / height.max(1) as f32) * camera.view;
    let names = |entity: Entity| scene.name_of(entity);
    let mut found = Vec::new();
    for view in &scene.sprites {
        let shown = shown(view, sheets, &names, None);
        let bounding_box = sprite_pieces(view, &shown, pictures)
            .and_then(|(_, outline)| picture_box(&outline, &view_proj, width, height));
        found.push(OverlayElement {
            entity: view.entity,
            bounding_box,
            particles: None,
            layer: Some(view.sprite.layer),
            animation: shown
                .frame
                .map(|(clip, frame, number)| (clip.to_owned(), frame, number)),
        });
    }
    for view in &scene.emitters {
        let alive = wish_particles::count(&view.emitter, scene.time);
        let particles =
            wish_particles::particles(&view.emitter, &view.world, scene.time, MAX_PARTICLES);
        let points: Vec<Vec3> = particles
            .iter()
            .flat_map(|p| {
                let half = Vec3::splat(p.size * 0.5);
                [p.position - half, p.position + half]
            })
            .collect();
        found.push(OverlayElement {
            entity: view.entity,
            bounding_box: picture_box(&points, &view_proj, width, height),
            particles: Some(alive),
            layer: None,
            animation: None,
        });
    }
    found
}

/// What `describe` says about a tilemap.
#[derive(Clone, Debug, PartialEq)]
pub struct TilemapElement {
    /// The entity.
    pub entity: Entity,
    /// The tilemap file.
    pub map: String,
    /// The sprite layer it is drawn on.
    pub layer: i32,
    /// The box its tiles on screen cover in the picture: left, top, right,
    /// bottom, in pixels; `None` when none is on screen (or the map is not
    /// loaded).
    pub bounding_box: Option<[i32; 4]>,
    /// The cells of the tiles on screen: lowest x and y, highest x and y.
    pub cells: Option<[i32; 4]>,
    /// How many tiles are on screen (over all its layers).
    pub tiles: u32,
    /// Whether the renderer has the map (its file read and its tileset's
    /// picture loaded).
    pub loaded: bool,
}

/// The tilemaps of `scene` as a `width` × `height` picture seen through
/// `camera` shows them.
pub(crate) fn tilemap_elements(
    scene: &RenderScene,
    camera: &CameraView,
    (width, height): (u32, u32),
    pictures: &BTreeMap<String, UiPicture>,
    grids: &BTreeMap<String, TileGrid>,
) -> Vec<TilemapElement> {
    let view_proj = camera.projection(width as f32 / height.max(1) as f32) * camera.view;
    let mut found = Vec::new();
    for view in &scene.tilemaps {
        let grid = grids
            .get(&view.map)
            .filter(|grid| pictures.contains_key(&grid.image));
        let mut element = TilemapElement {
            entity: view.entity,
            map: view.map.clone(),
            layer: view.tilemap.layer,
            bounding_box: None,
            cells: None,
            tiles: 0,
            loaded: grid.is_some(),
        };
        if let Some(grid) = grid
            && finite(&view.world)
        {
            let cell = view.tilemap.cell();
            let mut corners: Vec<Vec3> = Vec::new();
            for (_, x, y, tile) in visible_tiles(view, grid, &view_proj) {
                if tile as usize >= grid.tiles.len() {
                    continue;
                }
                let quad = cell_quad(view, cell, x, y);
                if picture_box(&quad, &view_proj, width, height).is_none() {
                    continue;
                }
                element.tiles += 1;
                element.cells = Some(match element.cells {
                    None => [x, y, x, y],
                    Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
                });
                corners.extend(quad);
            }
            element.bounding_box = picture_box(&corners, &view_proj, width, height);
        }
        found.push(element);
    }
    found
}

/// What is under a point of the picture on one tilemap.
#[derive(Clone, Debug, PartialEq)]
pub struct TileUnder {
    /// The tilemap's entity.
    pub entity: Entity,
    /// The cell under the point.
    pub cell: [i32; 2],
    /// Each layer's name and its tile there (-1 for none), in layer order.
    pub layers: Vec<(String, i32)>,
}

/// The cell of each loaded tilemap of `scene` under the pixel `point` of a
/// `width` × `height` picture seen through `camera`: the line through the
/// pixel meets each map's plane at one point.
pub(crate) fn tiles_under(
    scene: &RenderScene,
    camera: &CameraView,
    (width, height): (u32, u32),
    point: [f32; 2],
    grids: &BTreeMap<String, TileGrid>,
) -> Vec<TileUnder> {
    let view_proj = camera.projection(width as f32 / height.max(1) as f32) * camera.view;
    let inverse = view_proj.inverse();
    let ndc_x = point[0] / width.max(1) as f32 * 2.0 - 1.0;
    let ndc_y = 1.0 - point[1] / height.max(1) as f32 * 2.0;
    let near = inverse.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
    let far = inverse.project_point3(Vec3::new(ndc_x, ndc_y, 1.0));
    let direction = far - near;
    let mut found = Vec::new();
    for view in &scene.tilemaps {
        let Some(grid) = grids.get(&view.map) else {
            continue;
        };
        if !finite(&view.world) {
            continue;
        }
        // The map's plane: through its origin, facing its own +Z.
        let origin = view.world.transform_point3(Vec3::ZERO);
        let normal = view.world.transform_vector3(Vec3::Z);
        let across = direction.dot(normal);
        if across.abs() < 1e-9 {
            continue;
        }
        let along = (origin - near).dot(normal) / across;
        let hit = near + direction * along;
        let local = view.world.inverse().transform_point3(hit);
        let cell = view.tilemap.cell();
        let (x, y) = ((local.x / cell[0]).floor(), (local.y / cell[1]).floor());
        if !(x.is_finite() && y.is_finite()) || x.abs() > 1.0e6 || y.abs() > 1.0e6 {
            continue;
        }
        let (x, y) = (x as i32, y as i32);
        found.push(TileUnder {
            entity: view.entity,
            cell: [x, y],
            layers: grid
                .layers
                .iter()
                .enumerate()
                .map(|(index, layer)| (layer.name.clone(), grid.tile(index, x, y)))
                .collect(),
        });
    }
    found
}

/// Sprite pictures on the GPU: one texture for each picture drawn so far,
/// kept until the picture changes (`Renderer::set_ui_picture`). Locked
/// only while the GPU lock is held (after the pool's, like everything in a
/// frame), or through `&mut`.
#[derive(Default)]
pub(crate) struct SpriteTextures {
    views: Mutex<BTreeMap<String, wgpu::TextureView>>,
}

impl SpriteTextures {
    /// Forgets the texture of `path` (its picture changed).
    pub(crate) fn forget(&mut self, path: &str) {
        let views = match self.views.get_mut() {
            Ok(views) => views,
            Err(poisoned) => poisoned.into_inner(),
        };
        views.remove(path);
        self.views.clear_poison();
    }

    /// How many pictures are on the GPU.
    pub(crate) fn len(&self) -> usize {
        self.views.lock().map_or(0, |views| views.len())
    }
}

/// Uploads `picture` as a texture of one level (sprites are drawn near
/// their pixel size; sampling is as the atlas's always was). Call inside
/// `Gpu::checked`.
fn upload(gpu: &Gpu, picture: &UiPicture, path: &str) -> Option<wgpu::TextureView> {
    let limit = gpu.device.limits().max_texture_dimension_2d;
    let expected = u64::from(picture.width) * u64::from(picture.height) * 4;
    if picture.width == 0
        || picture.height == 0
        || picture.width.max(picture.height) > limit
        || picture.rgba.len() as u64 != expected
    {
        return None;
    }
    let size = wgpu::Extent3d {
        width: picture.width,
        height: picture.height,
        depth_or_array_layers: 1,
    };
    let texture = gpu.create_texture(&wgpu::TextureDescriptor {
        label: Some(path),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: PICTURE_FORMAT,
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
        &picture.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(picture.width * 4),
            rows_per_image: Some(picture.height),
        },
        size,
    );
    Some(texture.create_view(&wgpu::TextureViewDescriptor::default()))
}

/// The sprite and particle pipeline.
pub(crate) struct OverlayPipeline {
    pipeline: wgpu::RenderPipeline,
    /// Group 0: the matrix.
    frame_layout: wgpu::BindGroupLayout,
    /// Group 1: a picture and a sampler.
    picture_layout: wgpu::BindGroupLayout,
    smooth: wgpu::Sampler,
    sharp: wgpu::Sampler,
    /// A white texel, for plain colour and particles.
    white: wgpu::TextureView,
}

impl OverlayPipeline {
    /// Builds the pipeline from the `sprite_pass` shader, drawing into
    /// pictures of `format` over a depth buffer of `depth`.
    pub fn new(
        gpu: &Gpu,
        source: &str,
        format: wgpu::TextureFormat,
        depth: wgpu::TextureFormat,
    ) -> wish_core::Result<Self> {
        gpu.checked("create the sprite pipeline", |gpu| {
            let device = &gpu.device;
            let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("sprites frame"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
                    },
                    count: None,
                }],
            });
            let picture_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("sprites picture"),
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
                label: Some("sprites"),
                bind_group_layouts: &[Some(&frame_layout), Some(&picture_layout)],
                immediate_size: 0,
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("sprite_pass"),
                source: wgpu::ShaderSource::Wgsl(source.to_owned().into()),
            });
            let attribute = |location: u32, offset: u64, format| wgpu::VertexAttribute {
                format,
                offset,
                shader_location: location,
            };
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sprites"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_instance"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: INSTANCE_BYTES,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            attribute(0, 0, wgpu::VertexFormat::Float32x3),
                            attribute(1, 12, wgpu::VertexFormat::Float32x3),
                            attribute(2, 24, wgpu::VertexFormat::Float32x3),
                            attribute(3, 36, wgpu::VertexFormat::Float32x3),
                            attribute(4, 48, wgpu::VertexFormat::Float32x4),
                            attribute(5, 64, wgpu::VertexFormat::Float32x4),
                            attribute(6, 80, wgpu::VertexFormat::Float32),
                        ],
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
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
            let sampler = |label, filter| {
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some(label),
                    mag_filter: filter,
                    min_filter: filter,
                    ..wgpu::SamplerDescriptor::default()
                })
            };
            Self {
                pipeline,
                frame_layout,
                picture_layout,
                smooth: sampler("sprites smooth", wgpu::FilterMode::Linear),
                sharp: sampler("sprites sharp", wgpu::FilterMode::Nearest),
                white: crate::texture::solid(gpu, [255; 4], true, "sprites white"),
            }
        })
    }

    /// Records drawing `overlay` over `target`, tested against `depth`. The
    /// uniform buffer and the instance buffer come from `pool` and are
    /// filled here every frame; pictures come from `textures` (uploaded
    /// from `pictures` the first time they are drawn). The frame's bind
    /// groups are its own: one over the uniform buffer, one for each
    /// picture and sampler. Call inside `Gpu::checked`.
    pub(crate) fn draw(
        &self,
        gpu: &Gpu,
        pool: &mut Pool,
        encoder: &mut wgpu::CommandEncoder,
        (target, depth): (&wgpu::TextureView, &wgpu::TextureView),
        overlay: &Overlay,
        (textures, pictures): (&SpriteTextures, &BTreeMap<String, UiPicture>),
    ) {
        let device = &gpu.device;
        let matrix: Vec<u8> = overlay
            .view_proj
            .to_cols_array()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let uniform = pool.buffer(gpu, "sprites", wgpu::BufferUsages::UNIFORM, &matrix);
        let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprites frame"),
            layout: &self.frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                // At the matrix's size, whatever the pooled buffer's
                // capacity.
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform,
                    offset: 0,
                    size: wgpu::BufferSize::new(UNIFORM_BYTES),
                }),
            }],
        });
        // The pictures of this frame, uploaded when first drawn.
        let mut kept = textures
            .views
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut groups: BTreeMap<(Option<&str>, bool), wgpu::BindGroup> = BTreeMap::new();
        for batch in &overlay.batches {
            let key = (batch.texture.as_deref(), batch.smooth);
            if groups.contains_key(&key) {
                continue;
            }
            let view = match &batch.texture {
                None => Some(self.white.clone()),
                Some(path) => match kept.get(path) {
                    Some(view) => Some(view.clone()),
                    None => pictures
                        .get(path)
                        .and_then(|picture| upload(gpu, picture, path))
                        .inspect(|view| {
                            kept.insert(path.clone(), view.clone());
                        }),
                },
            };
            // A picture the GPU cannot take is drawn as nothing.
            let Some(view) = view else { continue };
            let sampler = if batch.smooth {
                &self.smooth
            } else {
                &self.sharp
            };
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sprites picture"),
                layout: &self.picture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            });
            groups.insert(key, group);
        }
        drop(kept);
        let instances = pool.buffer(
            gpu,
            "sprite instances",
            wgpu::BufferUsages::VERTEX,
            &overlay.instances,
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sprites"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &frame_group, &[]);
        for batch in &overlay.batches {
            let Some(group) = groups.get(&(batch.texture.as_deref(), batch.smooth)) else {
                continue;
            };
            pass.set_bind_group(1, group, &[]);
            // The batch's instances only: the pooled buffer may be larger
            // and hold an earlier frame's beyond them.
            let start = u64::from(batch.first) * INSTANCE_BYTES;
            let end = start + u64::from(batch.count) * INSTANCE_BYTES;
            pass.set_vertex_buffer(0, instances.slice(start..end));
            pass.draw(0..6, 0..batch.count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::SpriteAnimation;
    use wish_types::sprite::Sprite;

    fn view(sprite: Sprite, animation: Option<SpriteAnimation>) -> SpriteView {
        SpriteView {
            entity: Entity::default(),
            world: Mat4::IDENTITY,
            sprite,
            image: Some("p.png".to_owned()),
            animation,
        }
    }

    fn picture(width: u32, height: u32) -> BTreeMap<String, UiPicture> {
        BTreeMap::from([(
            "p.png".to_owned(),
            UiPicture {
                width,
                height,
                rgba: vec![255; (width * height * 4) as usize],
            },
        )])
    }

    #[test]
    fn a_sliced_sprite_keeps_its_corners_and_stretches_the_rest() {
        // A 30 x 30 picture with 10 px borders drawn 90 x 60 at 10 px a
        // meter: corners 1 m, the middle 7 x 4 m.
        let sprite = Sprite {
            pixels_per_unit: 10.0,
            anchor: [0.0, 0.0],
            size: [90.0, 60.0],
            slice: [10.0; 4],
            ..Sprite::default()
        };
        let pictures = picture(30, 30);
        let sheets = BTreeMap::new();
        let v = view(sprite, None);
        let names = |_: Entity| String::new();
        let shown = shown(&v, &sheets, &names, None);
        let (pieces, outline) = sprite_pieces(&v, &shown, &pictures).unwrap();
        assert_eq!(pieces.len(), 9);
        assert_eq!(outline[2], Vec3::new(9.0, 6.0, 0.0));
        // The top left piece: 0..1 m across, 5..6 m up, the picture's top
        // left 10 x 10 pixels.
        let (corners, source) = pieces[0];
        assert_eq!(
            (corners[0], corners[2]),
            (Vec3::new(0.0, 5.0, 0.0), Vec3::new(1.0, 6.0, 0.0))
        );
        assert_eq!(source, [0.0, 0.0, 10.0, 10.0]);
        // The middle.
        let (corners, source) = pieces[4];
        assert_eq!(
            (corners[0], corners[2]),
            (Vec3::new(1.0, 1.0, 0.0), Vec3::new(8.0, 5.0, 0.0))
        );
        assert_eq!(source, [10.0, 10.0, 20.0, 20.0]);
        // Drawn smaller than its borders: the borders shrink, nothing turns
        // inside out.
        let small = view(
            Sprite {
                size: [10.0, 10.0],
                ..sprite
            },
            None,
        );
        let shown = super::shown(&small, &sheets, &names, None);
        let (pieces, _) = sprite_pieces(&small, &shown, &pictures).unwrap();
        assert_eq!(pieces.len(), 4, "the corners only");
        for (corners, _) in pieces {
            assert!(corners[2].x > corners[0].x && corners[2].y > corners[0].y);
        }
    }

    #[test]
    fn an_animator_picks_the_sheet_s_frame() {
        let sheets = BTreeMap::from([(
            "a.sheet.json".to_owned(),
            SpriteSheet {
                image: "p.png".to_owned(),
                frames: vec![[0, 0, 8, 8], [8, 0, 8, 8], [16, 0, 8, 8]],
                clips: BTreeMap::from([("walk".to_owned(), vec![2, 1])]),
            },
        )]);
        let names = |_: Entity| "'hero'".to_owned();
        let animated = |clip: &str, frame: u32| {
            view(
                Sprite::default(),
                Some(SpriteAnimation {
                    sheet: "a.sheet.json".to_owned(),
                    clip: clip.to_owned(),
                    frame,
                }),
            )
        };
        let mut warnings = Vec::new();
        let walk = animated("walk", 1);
        let shown_walk = shown(&walk, &sheets, &names, Some(&mut warnings));
        assert_eq!(shown_walk.rect, [8.0, 0.0, 8.0, 8.0]);
        assert_eq!(shown_walk.frame, Some(("walk", 1, 1)));
        let still = animated("", 2);
        assert_eq!(
            shown(&still, &sheets, &names, Some(&mut warnings)).rect[0],
            16.0
        );
        assert!(warnings.is_empty());
        let lost = animated("swim", 0);
        assert_eq!(
            shown(&lost, &sheets, &names, Some(&mut warnings)).rect[0],
            0.0
        );
        let beyond = animated("", 9);
        assert_eq!(
            shown(&beyond, &sheets, &names, Some(&mut warnings)).rect[0],
            16.0
        );
        let codes: Vec<&str> = warnings.iter().map(|w| &*w.code).collect();
        assert_eq!(codes, ["sprite_clip_not_found", "sprite_frame_not_found"]);
        // Not a sheet the renderer was given: the sprite's own picture.
        let other = view(
            Sprite::default(),
            Some(SpriteAnimation {
                sheet: "b.sheet.json".to_owned(),
                clip: String::new(),
                frame: 0,
            }),
        );
        assert!(shown(&other, &sheets, &names, None).frame.is_none());
    }
}
