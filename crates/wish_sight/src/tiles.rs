//! The solid parts of tilemaps (design note `docs/design/1.1-tilemaps.md`
//! §7), shared by physics (walls that collide) and sight (walls that block
//! sight and 2D light).
//!
//! What a tilemap's solid tiles are is read from its file by whoever reads
//! the project (the control plane), merged into boxes there, and handed to
//! the world as the extension [`TileShapes`] when a session starts.

use std::collections::BTreeMap;
use std::sync::Arc;

use wish_ecs::World;

/// The collision of one tilemap file, in cells.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapShapes {
    /// Boxes of whole solid cells: lowest x and y, then one past the highest
    /// x and y.
    pub boxes: Vec<[i32; 4]>,
    /// Cells with an outlined solid tile: the cell, and the outline in tile
    /// units from the cell's bottom left corner.
    pub outlines: Vec<([i32; 2], Vec<[f32; 2]>)>,
}

impl MapShapes {
    /// Whether there is nothing solid.
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty() && self.outlines.is_empty()
    }
}

/// The collision of the project's tilemaps by file path: a world extension
/// (ADR-026), filled when a session starts ([`install_tile_shapes`]).
#[derive(Clone, Debug, Default)]
pub struct TileShapes {
    maps: BTreeMap<String, Arc<MapShapes>>,
}

impl TileShapes {
    /// The shapes of the tilemap file `path`, if known.
    pub fn get(&self, path: &str) -> Option<&Arc<MapShapes>> {
        self.maps.get(path)
    }

    /// Whether no map is known.
    pub fn is_empty(&self) -> bool {
        self.maps.is_empty()
    }
}

/// Tells the world what the solid parts of its tilemaps are (replacing what
/// it knew of the same files).
pub fn install_tile_shapes(world: &mut World, maps: impl IntoIterator<Item = (String, MapShapes)>) {
    let mut known = world.take_extension::<TileShapes>().unwrap_or_default();
    for (path, shapes) in maps {
        known.maps.insert(path, Arc::new(shapes));
    }
    world.insert_extension(known);
}
