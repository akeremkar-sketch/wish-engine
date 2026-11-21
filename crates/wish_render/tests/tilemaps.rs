//! Tilemaps (release 1.1, design note `docs/design/1.1-tilemaps.md` §6), on
//! the software adapter: two layers of tiles through the batched sprite
//! path under an orthographic camera whose pixels fit the cells, checked
//! pixel by pixel and against a golden image; chunks off screen skipped;
//! sprites over the map and sorted by height; what `describe_view` and a
//! point on the picture say. `UPDATE_GOLDEN=1` rewrites the golden image.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use wish_ecs::World;
use wish_render::{
    Gpu, GpuOptions, RenderScene, Renderer, RgbaImage, TileGrid, TileGridLayer, Tolerance,
    UiPicture, extract,
};
use wish_scene::load;

/// An orthographic camera 11.25 m tall over a 320 × 180 picture: 16 pixels
/// a meter, the world's origin at pixel (160, 90). Cells of 1 m are 16
/// pixels; the tileset's 8 × 8 tiles are drawn at 2 pixels a texel.
const SCENE: &str = r#"{
  "format": 1,
  "resources": {
    "Ambient": { "intensity": 0 },
    "Sky": { "luminance": 0 }
  },
  "entities": {
    "camera": { "components": {
      "Camera": { "projection": "Orthographic", "ortho_height": 11.25 },
      "Transform": { "translation": [0, 0, 10] } } },
    "map": { "components": {
      "Tilemap": { "map": "maps/room.tilemap.json" },
      "Transform": { "translation": [-4, -3, 0] } } },
    "a_low": { "components": {
      "Sprite": { "pixels_per_unit": 16, "rect": [0, 0, 32, 32], "color": [1, 0, 1, 1] },
      "Transform": { "translation": [6, -1, 0] } } },
    "b_high": { "components": {
      "Sprite": { "pixels_per_unit": 16, "rect": [0, 0, 32, 32], "color": [0, 1, 1, 1] },
      "Transform": { "translation": [6.5, -0.5, 0] } } }
  }
}"#;

/// The tiles' colours: tile `i` of the tileset.
const TILES: [[u8; 4]; 4] = [
    [60, 60, 70, 255],
    [200, 120, 40, 255],
    [40, 160, 220, 255],
    [230, 230, 90, 255],
];

/// The texel of tile `tile` at (x, y) from its top left: its colour, with
/// a light top left texel and a dark bottom right one (so that a tile drawn
/// upside down or shifted by a texel shows).
fn texel(tile: usize, x: u32, y: u32) -> [u8; 4] {
    if (x, y) == (0, 0) {
        [255, 255, 255, 255]
    } else if (x, y) == (7, 7) {
        [0, 0, 0, 255]
    } else {
        TILES[tile]
    }
}

/// 32 × 8: four 8 × 8 tiles side by side.
fn tileset_picture() -> UiPicture {
    let mut rgba = Vec::new();
    for y in 0..8u32 {
        for x in 0..32u32 {
            rgba.extend_from_slice(&texel((x / 8) as usize, x % 8, y));
        }
    }
    UiPicture {
        width: 32,
        height: 8,
        rgba,
    }
}

fn chunk(cells: &[(i32, i32, i32)]) -> Arc<[i32; 256]> {
    let mut chunk = [-1; 256];
    for &(x, y, tile) in cells {
        chunk[(y * 16 + x) as usize] = tile;
    }
    Arc::new(chunk)
}

/// A room 8 × 6 cells: a floor of tile 0, walls of tile 1 around it with a
/// door, a tile 2 and a tile 3 inside; and a chunk far away to the right.
fn grid() -> TileGrid {
    let mut floor = Vec::new();
    let mut walls = Vec::new();
    for x in 0..8 {
        for y in 0..6 {
            floor.push((x, y, 0));
            let edge = x == 0 || x == 7 || y == 0 || y == 5;
            if edge && !(y == 0 && (3..5).contains(&x)) {
                walls.push((x, y, 1));
            }
        }
    }
    walls.push((2, 3, 2));
    walls.push((5, 2, 3));
    TileGrid {
        image: "tiles/station.png".to_owned(),
        tiles: (0..4).map(|i| [i * 8, 0, 8, 8]).collect(),
        layers: vec![
            TileGridLayer {
                name: "floor".to_owned(),
                chunks: BTreeMap::from([((0, 0), chunk(&floor))]),
            },
            TileGridLayer {
                name: "walls".to_owned(),
                chunks: BTreeMap::from([
                    ((0, 0), chunk(&walls)),
                    ((40, 0), chunk(&[(0, 0, 1), (1, 0, 1)])),
                ]),
            },
        ],
    }
}

fn scene(change: impl FnOnce(&mut serde_json::Value)) -> RenderScene {
    let mut document: serde_json::Value = serde_json::from_str(SCENE).unwrap();
    change(&mut document);
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    let result = load(
        &document.to_string(),
        "scenes/tiles.scene.json",
        world.registry(),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap().instantiate(&mut world).unwrap();
    wish_transform::update_global_transforms(&mut world).unwrap();
    extract(&world).unwrap()
}

fn gpu() -> Gpu {
    Gpu::new(GpuOptions { software: true }).expect("a software adapter")
}

fn renderer(gpu: &Gpu) -> Renderer {
    let mut renderer = Renderer::new(gpu).unwrap();
    renderer.set_ui_picture("tiles/station.png", tileset_picture());
    renderer.set_tilemap("maps/room.tilemap.json", grid());
    renderer
}

fn check_golden(name: &str, image: &RgbaImage) {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("captures");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(format!("{name}.png")), image.to_png().unwrap()).unwrap();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, image.to_png().unwrap()).unwrap();
        return;
    }
    let golden = RgbaImage::from_png(&std::fs::read(&path).unwrap_or_else(|_| {
        panic!(
            "no golden image at {}; run with UPDATE_GOLDEN=1",
            path.display()
        )
    }))
    .unwrap();
    let tolerance = Tolerance {
        channel: 1,
        fraction: 0.0,
    };
    let diff = image.diff(&golden, tolerance.channel).expect("same size");
    assert!(image.matches(&golden, tolerance), "{name}: {diff:?}");
}

/// The top left pixel of cell (x, y) of the map (its origin at (-4, -3) m).
fn cell_pixel(x: i32, y: i32) -> (u32, u32) {
    ((160 + (x - 4) * 16) as u32, (90 - (y - 3 + 1) * 16) as u32)
}

fn close(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).take(3).all(|(a, b)| a.abs_diff(b) <= 1)
}

/// Every pixel of every tile on screen is the texel it should show, at 2
/// pixels a texel: the walls over the floor, tiles 2 and 3 where they were
/// painted, nothing shifted, flipped, smeared or seamed.
#[test]
fn tiles_are_pixel_exact_under_an_orthographic_camera() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let frame = renderer
        .render(&gpu, &scene(|_| {}), None, 320, 180)
        .unwrap();
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
    let image = &frame.image;
    let grid = grid();
    let mut checked = 0;
    for x in 0..8 {
        for y in 0..6 {
            // The top layer's tile wins.
            let tile = [1, 0]
                .iter()
                .map(|layer| grid.tile(*layer, x, y))
                .find(|tile| *tile >= 0)
                .unwrap() as usize;
            let (left, top) = cell_pixel(x, y);
            for py in 0..16 {
                for px in 0..16 {
                    let got = image.pixel(left + px, top + py).unwrap();
                    let want = texel(tile, px / 2, py / 2);
                    assert!(
                        close(got, want),
                        "cell ({x}, {y}) tile {tile} pixel ({px}, {py}): {got:?}, not {want:?}"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 8 * 6 * 256);
    // Just outside the room: the black sky.
    let (left, top) = cell_pixel(0, 0);
    assert!(close(image.pixel(left - 1, top).unwrap(), [0, 0, 0, 255]));
    check_golden("tilemap", image);
}

#[test]
fn a_map_is_one_batch_and_chunks_off_screen_are_skipped() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    // Without the sprites: the sky, tone mapping and one batch of tiles.
    let frame = renderer
        .render(
            &gpu,
            &scene(|document| {
                let entities = document["entities"].as_object_mut().unwrap();
                entities.remove("a_low");
                entities.remove("b_high");
            }),
            None,
            320,
            180,
        )
        .unwrap();
    assert_eq!(frame.stats.draw_calls, 3, "{:?}", frame.stats);
    // 48 floor tiles and 26 wall, door and inside tiles; the far chunk's
    // two tiles are not drawn.
    let walls = grid().layers[1].chunks[&(0, 0)]
        .iter()
        .filter(|tile| **tile >= 0)
        .count() as u64;
    assert_eq!(frame.stats.triangles, 2 + 2 * (48 + walls));
    // Moved so that the far chunk is on screen, its tiles are drawn too.
    let moved = scene(|document| {
        document["entities"]["map"]["components"]["Transform"]["translation"] =
            serde_json::json!([-644, -3, 0]);
    });
    let frame = renderer.render(&gpu, &moved, None, 320, 180).unwrap();
    assert!(frame.stats.triangles >= 2 + 2 * 2, "{:?}", frame.stats);
    assert!(frame.stats.triangles < 2 + 2 * 48, "{:?}", frame.stats);
}

#[test]
fn sprites_stand_over_the_map_and_sort_by_height() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    // Two sprites 2 m square on the map's layer: "a_low" (magenta) and
    // "b_high" (cyan, half a meter up and right), in that entity order. By
    // distance (both at z = 0) the later one, "b_high", is drawn over.
    let overlap = (160 + 6 * 16 + 10, 90 + 16 - 2); // inside both
    let plain = renderer
        .render(&gpu, &scene(|_| {}), None, 320, 180)
        .unwrap();
    let p = plain.image.pixel(overlap.0, overlap.1).unwrap();
    assert!(close(p, [0, 255, 255, 255]), "high over low: {p:?}");
    // Sorted by height: the lower sprite is drawn over.
    let sorted = scene(|document| {
        document["entities"]["map"]["components"]["Tilemap"]["y_sort"] = serde_json::json!(true);
    });
    let frame = renderer.render(&gpu, &sorted, None, 320, 180).unwrap();
    let p = frame.image.pixel(overlap.0, overlap.1).unwrap();
    assert!(close(p, [255, 0, 255, 255]), "low over high: {p:?}");
    // A sprite over the map's floor: the sprite.
    let on_map = scene(|document| {
        document["entities"]["a_low"]["components"]["Transform"]["translation"] =
            serde_json::json!([0, 0, 0]);
    });
    let frame = renderer.render(&gpu, &on_map, None, 320, 180).unwrap();
    let p = frame.image.pixel(160 + 4, 90 - 4).unwrap();
    assert!(close(p, [255, 0, 255, 255]), "sprite over the floor: {p:?}");
}

#[test]
fn describe_and_a_point_say_what_the_map_shows() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let scene = scene(|_| {});
    let elements = renderer.tilemap_elements(&scene, None, 320, 180).unwrap();
    assert_eq!(elements.len(), 1);
    let map = &elements[0];
    assert!(map.loaded);
    assert_eq!(map.map, "maps/room.tilemap.json");
    assert_eq!(map.cells, Some([0, 0, 7, 5]));
    let walls = grid().layers[1].chunks[&(0, 0)]
        .iter()
        .filter(|tile| **tile >= 0)
        .count() as u32;
    assert_eq!(map.tiles, 48 + walls);
    assert_eq!(map.bounding_box, Some([96, 42, 224, 138]));
    // The middle of cell (2, 3): tile 0 on the floor, tile 2 on the walls.
    let (left, top) = cell_pixel(2, 3);
    let under = renderer
        .tiles_under(
            &scene,
            None,
            (320, 180),
            [left as f32 + 8.0, top as f32 + 8.0],
        )
        .unwrap();
    assert_eq!(under.len(), 1);
    assert_eq!(under[0].cell, [2, 3]);
    assert_eq!(
        under[0].layers,
        [("floor".to_owned(), 0), ("walls".to_owned(), 2)]
    );
    // Outside the room: a cell with nothing.
    let under = renderer
        .tiles_under(&scene, None, (320, 180), [10.0, 10.0])
        .unwrap();
    assert_eq!(under[0].layers[0].1, -1);
    // A map the renderer does not have is listed, not loaded, and draws
    // nothing.
    let mut empty = Renderer::new(&gpu).unwrap();
    empty.set_ui_picture("tiles/station.png", tileset_picture());
    let elements = empty.tilemap_elements(&scene, None, 320, 180).unwrap();
    assert!(!elements[0].loaded);
    assert_eq!(elements[0].tiles, 0);
    let frame = empty.render(&gpu, &scene, None, 320, 180).unwrap();
    assert!(close(frame.image.pixel(160, 89).unwrap(), [0, 0, 0, 255]));
}
