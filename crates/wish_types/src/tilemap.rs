//! Tilemaps (design note `docs/design/1.1-tilemaps.md`): a grid of small
//! pictures from a tileset, painted in layers, whose solid tiles are walls.
//!
//! The tiles themselves are a file of their own (`*.tilemap.json`, any
//! size); the world holds a [`Tilemap`] that names it and says how it is
//! drawn. The map stands in the entity's XY plane, like a sprite, and is
//! drawn with the sprites (not lit, after the 3D scene).

use wish_core::{AssetRef, Color};
use wish_schema::component;

/// Puts a tilemap file (`*.tilemap.json`) in the world: its tiles drawn in
/// the entity's XY plane from the entity's origin (the bottom left corner
/// of cell (0, 0)), and its solid tiles walls for physics.
///
/// Paint it with the `tilemap_edit` tool or the editor's tile palette; the
/// file is the source of truth, and a running game uses the map as it was
/// when the session started.
#[component]
pub struct Tilemap {
    /// The tilemap file (`*.tilemap.json`).
    pub map: AssetRef,
    /// The size of one cell in the entity's plane, before the entity's
    /// scale: width and height.
    #[schema(unit = "m", default = [1.0, 1.0])]
    pub cell_size: [f32; 2],
    /// The sprite layer the map is drawn on: sprites with a higher layer are
    /// drawn over it, lower ones under it. The map's own layers are drawn in
    /// their file order within it.
    #[schema(range = -1000..=1000)]
    pub layer: i32,
    /// A tint over every tile (linear RGB, and alpha); white shows the
    /// tiles as they are.
    #[schema(default = Color::WHITE)]
    pub color: Color,
    /// Smooth scaling (true) or sharp pixels (false, for pixel art: no
    /// seams between tiles).
    pub smooth: bool,
    /// Sorts the sprites on this map's `layer` by height, for top-down
    /// games: a sprite lower on the map (its anchor at a smaller y) is drawn
    /// over one higher up, and every one of them over the map.
    pub y_sort: bool,
}

impl Tilemap {
    /// The size of a cell, cleaned: each side finite and above zero, or 1.
    pub fn cell(&self) -> [f32; 2] {
        self.cell_size.map(|side| {
            if side.is_finite() && side > 0.0 {
                side
            } else {
                1.0
            }
        })
    }
}
