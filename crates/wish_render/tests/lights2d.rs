//! 2D lights and vision (release 1.1, design note
//! `docs/design/1.1-lights-2d.md` §8), on the software adapter: a room of
//! tiles with a door, lit by a lamp inside and a beacon outside whose light
//! the walls cut; a player's vision that darkens what it cannot see; with
//! both off the picture is byte for byte the one without them. Golden
//! images: `lights2d_lit.png`, `lights2d_dark.png` (`UPDATE_GOLDEN=1`
//! rewrites them). The measurement of 16 lights over a 64 × 64 tilemap is
//! `measure_16_lights_over_a_64_by_64_tilemap` (by hand, release).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use wish_ecs::World;
use wish_render::{
    Gpu, GpuOptions, RenderScene, Renderer, RgbaImage, TileGrid, TileGridLayer, Tolerance,
    UiPicture, extract,
};
use wish_scene::load;
use wish_sight::MapShapes;

const MAP: &str = "maps/room.tilemap.json";
const TILES: &str = "tiles/station.png";

/// An orthographic camera 11.25 m tall over a 320 × 180 picture: 16 pixels
/// a meter, the world's origin at pixel (160, 90). The map's origin is at
/// (-6, -4): a room of 12 × 8 cells (x -6 to 6 m, y -4 to 4 m) with a door
/// in its east wall (y -1 to 1 m), and floor outside it to x = 10 m.
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
      "Transform": { "translation": [-6, -4, 0] } } },
    "player": { "components": {
      "Sprite": { "pixels_per_unit": 16, "rect": [0, 0, 16, 16], "color": [0.9, 0.2, 0.2, 1], "layer": 1 },
      "Transform": { "translation": [4, 0, 0] } } }
  }
}"#;

/// The floor's colour and the walls'.
const FLOOR: [u8; 4] = [170, 170, 170, 255];
const WALL: [u8; 4] = [90, 70, 60, 255];

/// 16 × 8: the floor tile, then the wall tile.
fn tileset_picture() -> UiPicture {
    let mut rgba = Vec::new();
    for _ in 0..8u32 {
        for x in 0..16u32 {
            rgba.extend_from_slice(if x < 8 { &FLOOR } else { &WALL });
        }
    }
    UiPicture {
        width: 16,
        height: 8,
        rgba,
    }
}

/// Whether cell (x, y) of the map is a wall: the room's edge but the door.
fn wall(x: i32, y: i32) -> bool {
    let edge = (x == 0 || x == 11 || y == 0 || y == 7) && x <= 11;
    edge && !(x == 11 && (3..5).contains(&y))
}

fn grid() -> TileGrid {
    let mut floor = [-1; 256];
    let mut walls = [-1; 256];
    for x in 0..16 {
        for y in 0..8 {
            floor[(y * 16 + x) as usize] = 0;
            if wall(x, y) {
                walls[(y * 16 + x) as usize] = 1;
            }
        }
    }
    TileGrid {
        image: TILES.to_owned(),
        tiles: vec![[0, 0, 8, 8], [8, 0, 8, 8]],
        layers: vec![
            TileGridLayer {
                name: "floor".to_owned(),
                chunks: BTreeMap::from([((0, 0), Arc::new(floor))]),
            },
            TileGridLayer {
                name: "walls".to_owned(),
                chunks: BTreeMap::from([((0, 0), Arc::new(walls))]),
            },
        ],
    }
}

/// The walls as the box merging of `wish_asset::tiles` gives them.
fn shapes() -> MapShapes {
    MapShapes {
        boxes: vec![
            [0, 0, 12, 1],
            [0, 1, 1, 7],
            [11, 1, 12, 3],
            [11, 5, 12, 7],
            [0, 7, 12, 8],
        ],
        outlines: Vec::new(),
    }
}

fn world_of(document: &Value) -> World {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    let result = load(
        &document.to_string(),
        "scenes/lights.scene.json",
        world.registry(),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap().instantiate(&mut world).unwrap();
    wish_transform::update_global_transforms(&mut world).unwrap();
    wish_sight::install_tile_shapes(&mut world, [(MAP.to_owned(), shapes())]);
    world
}

fn scene(change: impl FnOnce(&mut Value)) -> RenderScene {
    let mut document: Value = serde_json::from_str(SCENE).unwrap();
    change(&mut document);
    extract(&world_of(&document)).unwrap()
}

/// 2D lighting on, dim, with a warm lamp inside the room and a blue beacon
/// in the corridor outside its door.
fn lit(document: &mut Value) {
    document["resources"]["Lighting2D"] =
        json!({ "enabled": true, "ambient": [0.15, 0.15, 0.15, 1] });
    let entities = document["entities"].as_object_mut().unwrap();
    entities.insert(
        "lamp".to_owned(),
        json!({ "components": {
            "Light2D": { "color": [1.0, 0.8, 0.5, 1], "intensity": 1.2, "radius": 6 },
            "Transform": { "translation": [-3, 0, 0] } } }),
    );
    entities.insert(
        "beacon".to_owned(),
        json!({ "components": {
            "Light2D": { "color": [0.3, 0.5, 1.0, 1], "intensity": 1.5, "radius": 7 },
            "Transform": { "translation": [8, 0, 0] } } }),
    );
}

/// The player sees 5 m around itself, from just inside the door.
fn vision(document: &mut Value) {
    document["entities"]["player"]["components"]["Vision2D"] =
        json!({ "radius": 5, "softness": 1 });
}

fn gpu() -> Gpu {
    Gpu::new(GpuOptions { software: true }).expect("a software adapter")
}

fn renderer(gpu: &Gpu) -> Renderer {
    let mut renderer = Renderer::new(gpu).unwrap();
    renderer.set_ui_picture(TILES, tileset_picture());
    renderer.set_tilemap(MAP, grid());
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

/// The pixel at world (x, y).
fn at(image: &RgbaImage, x: f32, y: f32) -> [u8; 4] {
    image
        .pixel((160.0 + x * 16.0) as u32, (90.0 - y * 16.0) as u32)
        .unwrap()
}

fn brightness(pixel: [u8; 4]) -> u32 {
    pixel.iter().take(3).map(|c| u32::from(*c)).sum()
}

#[test]
fn lights_light_the_room_and_its_walls_cut_them() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let frame = renderer.render(&gpu, &scene(lit), None, 320, 180).unwrap();
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
    assert_eq!(frame.stats.effects.names(), ["lights_2d"]);
    let image = &frame.image;
    check_golden("lights2d_lit", image);
    // Far from both lights: the ambient's share of the floor.
    let dim = at(image, 4.0, 3.0);
    // By the lamp: warm and bright.
    let lamp = at(image, -3.0, 0.5);
    assert!(
        brightness(lamp) > 2 * brightness(dim),
        "{lamp:?} against {dim:?}"
    );
    assert!(lamp[0] > lamp[2], "warm: {lamp:?}");
    // In the corridor by the beacon: blue.
    let corridor = at(image, 8.0, 2.0);
    assert!(
        corridor[2] > corridor[0] && 2 * brightness(corridor) > 3 * brightness(dim),
        "{corridor:?} against {dim:?}"
    );
    // Through the door the beacon reaches into the room ...
    let doorway = at(image, 4.8, -0.7);
    assert!(doorway[2] > dim[2] + 20, "{doorway:?} against {dim:?}");
    // ... and not past the wall beside it: (4, 2.5) is within the beacon's
    // 7 m but behind the wall north of the door.
    let behind = at(image, 4.0, 2.5);
    assert!(
        brightness(behind).abs_diff(brightness(dim)) <= 6,
        "{behind:?}"
    );
    // Without shadows the same point is lit.
    let unshadowed = renderer
        .render(
            &gpu,
            &scene(|document| {
                lit(document);
                document["entities"]["beacon"]["components"]["Light2D"]["shadows"] = json!(false);
            }),
            None,
            320,
            180,
        )
        .unwrap();
    let lit_behind = at(&unshadowed.image, 4.0, 2.5);
    assert!(
        lit_behind[2] > behind[2] + 20,
        "{lit_behind:?} against {behind:?}"
    );
    // The extracted areas: the lamp's is cut by the room, the beacon's by
    // the wall.
    let lighting = &scene(lit).lighting_2d;
    assert_eq!(lighting.lights.len(), 2);
    assert!(lighting.lights.iter().all(|light| light.area.len() > 128));
}

#[test]
fn a_vision_darkens_what_the_player_cannot_see() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let plain = renderer
        .render(&gpu, &scene(|_| {}), None, 320, 180)
        .unwrap();
    let frame = renderer
        .render(&gpu, &scene(vision), None, 320, 180)
        .unwrap();
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
    assert_eq!(frame.stats.effects.names(), ["vision"]);
    let image = &frame.image;
    // Near the player, inside its radius and before the soft edge: as it
    // was.
    for (x, y) in [(3.0, 1.0), (1.5, -2.0), (6.5, 0.2), (4.0, 0.6)] {
        assert_eq!(at(image, x, y), at(&plain.image, x, y), "({x}, {y})");
    }
    // Beyond its radius: dark (the floor at a tenth of its light, under
    // the default darkness of 0.9).
    let dark =
        |x: f32, y: f32| brightness(at(image, x, y)) * 2 < brightness(at(&plain.image, x, y));
    assert!(dark(-4.0, 2.0), "{:?}", at(image, -4.0, 2.0));
    // Within its radius but behind the wall north of the door: dark too.
    assert!(dark(7.5, 2.5), "{:?}", at(image, 7.5, 2.5));
    // Through the door, the corridor: seen.
    assert_eq!(at(image, 7.5, 0.2), at(&plain.image, 7.5, 0.2));
    // The soft edge: between the two.
    let edge = brightness(at(image, -0.5, 0.0));
    assert!(
        edge > brightness(at(image, -4.0, 2.0)) && edge < brightness(at(&plain.image, -0.5, 0.0))
    );
    check_golden("lights2d_dark", image);
}

#[test]
fn lighting_and_vision_together() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let frame = renderer
        .render(
            &gpu,
            &scene(|document| {
                lit(document);
                vision(document);
            }),
            None,
            320,
            180,
        )
        .unwrap();
    assert_eq!(frame.stats.effects.names(), ["lights_2d", "vision"]);
    // The sky, tone mapping, the light texture, the mask, the darkness and
    // the sprites' batches.
    assert!(frame.stats.draw_calls >= 6, "{:?}", frame.stats);
}

#[test]
fn off_is_the_old_picture_to_the_byte() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let old = renderer
        .render(&gpu, &scene(|_| {}), None, 320, 180)
        .unwrap();
    // Lights and the resource, lighting off, a vision disabled: nothing
    // changes but a warning.
    let off = renderer
        .render(
            &gpu,
            &scene(|document| {
                lit(document);
                document["resources"]["Lighting2D"]["enabled"] = json!(false);
                document["entities"]["player"]["components"]["Vision2D"] =
                    json!({ "enabled": false });
            }),
            None,
            320,
            180,
        )
        .unwrap();
    assert_eq!(off.image.pixels, old.image.pixels, "byte for byte");
    assert_eq!(off.stats.draw_calls, old.stats.draw_calls);
    assert!(off.stats.effects.names().is_empty());
    let codes: Vec<&str> = off.warnings.iter().map(|w| &*w.code).collect();
    assert_eq!(codes, ["light_2d_without_lighting"]);
}

#[test]
fn broken_numbers_light_nothing_and_break_nothing() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let frame = renderer
        .render(
            &gpu,
            &scene(|document| {
                lit(document);
                let lamp = &mut document["entities"]["lamp"]["components"]["Light2D"];
                lamp["radius"] = json!(0);
                document["entities"]["beacon"]["components"]["Light2D"]["intensity"] = json!(0);
                document["entities"]["player"]["components"]["Vision2D"] =
                    json!({ "radius": 3, "softness": 0 });
            }),
            None,
            320,
            180,
        )
        .unwrap();
    // No light reaches anything: where the lamp stands is as dark as a
    // corner far from it; a vision with no soft edge still draws.
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
    assert_eq!(at(&frame.image, -3.0, 0.5), at(&frame.image, -4.5, -2.5));
    let seen = at(&frame.image, 3.5, 0.5);
    assert!(
        brightness(seen) > brightness(at(&frame.image, -3.0, 0.5)),
        "{seen:?}"
    );
}

/// 16 lights over a 64 × 64 tilemap of rooms (8 × 8 rooms of 8 cells, walls
/// with doors), at 1280 × 720 on the software adapter; the medians of 10
/// frames after 3 to warm up. `cargo test --release -p wish_render --test
/// lights2d measure -- --ignored --nocapture` (`WISH_BENCH_HARDWARE=1` for
/// the hardware adapter).
#[test]
#[ignore = "a measurement, by hand"]
fn measure_16_lights_over_a_64_by_64_tilemap() {
    const SIDE: i32 = 64;
    let solid = |x: i32, y: i32| {
        let on_line = |v: i32| v % 8 == 0 || v == SIDE - 1;
        let door = |v: i32| (3..5).contains(&(v % 8));
        (on_line(x) && !door(y)) || (on_line(y) && !door(x))
    };
    let mut chunks_floor = BTreeMap::new();
    let mut chunks_walls = BTreeMap::new();
    for cy in 0..SIDE / 16 {
        for cx in 0..SIDE / 16 {
            let floor = [0; 256];
            let mut walls = [-1; 256];
            for y in 0..16 {
                for x in 0..16 {
                    if solid(cx * 16 + x, cy * 16 + y) {
                        walls[(y * 16 + x) as usize] = 1;
                    }
                }
            }
            chunks_floor.insert((cx, cy), Arc::new(floor));
            chunks_walls.insert((cx, cy), Arc::new(walls));
        }
    }
    let grid = TileGrid {
        image: TILES.to_owned(),
        tiles: vec![[0, 0, 8, 8], [8, 0, 8, 8]],
        layers: vec![
            TileGridLayer {
                name: "floor".to_owned(),
                chunks: chunks_floor,
            },
            TileGridLayer {
                name: "walls".to_owned(),
                chunks: chunks_walls,
            },
        ],
    };
    // One box per solid cell (many more walls than merging gives: the
    // hard case for the polygons), and the cells of each row merged into
    // runs (close to what `wish_asset`'s merging gives a map like this).
    let mut cells = Vec::new();
    let mut runs: Vec<[i32; 4]> = Vec::new();
    for y in 0..SIDE {
        for x in 0..SIDE {
            if solid(x, y) {
                cells.push([x, y, x + 1, y + 1]);
                match runs.last_mut() {
                    Some(run) if run[1] == y && run[2] == x => run[2] = x + 1,
                    _ => runs.push([x, y, x + 1, y + 1]),
                }
            }
        }
    }
    let mut document = json!({
        "format": 1,
        "resources": { "Ambient": { "intensity": 0 }, "Sky": { "luminance": 0 } },
        "entities": {
            "camera": { "components": {
                "Camera": { "projection": "Orthographic", "ortho_height": 64 },
                "Transform": { "translation": [0, 0, 10] } } },
            "map": { "components": {
                "Tilemap": { "map": MAP },
                "Transform": { "translation": [-32, -32, 0] } } }
        }
    });
    let make = |document: &Value, boxes: &[[i32; 4]], lights: bool, shadows: bool, vision: bool| {
        let mut document = document.clone();
        if lights {
            document["resources"]["Lighting2D"] =
                json!({ "enabled": true, "ambient": [0.1, 0.1, 0.1, 1] });
            for i in 0..16 {
                let (x, y) = ((i % 4) as f32 * 16.0 - 26.0, (i / 4) as f32 * 16.0 - 26.0);
                document["entities"][format!("light{i}")] = json!({ "components": {
                    "Light2D": { "color": [1, 0.9, 0.7, 1], "radius": 10, "shadows": shadows },
                    "Transform": { "translation": [x, y, 0] } } });
            }
        }
        if vision {
            document["entities"]["player"] = json!({ "components": {
                "Vision2D": { "radius": 12 },
                "Transform": { "translation": [4.5, 4.5, 0] } } });
        }
        let world = world_of_map(&document, boxes.to_vec());
        let started = std::time::Instant::now();
        let scene = extract(&world).unwrap();
        let extract_ms = started.elapsed().as_secs_f64() * 1000.0;
        (scene, extract_ms)
    };
    document["entities"]["map"]["components"]["Tilemap"]["cell_size"] = json!([1, 1]);
    let software = std::env::var_os("WISH_BENCH_HARDWARE").is_none();
    let gpu = Gpu::new(GpuOptions { software }).expect("an adapter");
    let mut renderer = Renderer::new(&gpu).unwrap();
    renderer.set_ui_picture(TILES, tileset_picture());
    renderer.set_tilemap(MAP, grid);
    let median = |scene: &RenderScene| {
        let mut times = Vec::new();
        for tick in 0..13 {
            let started = std::time::Instant::now();
            renderer.render(&gpu, scene, None, 1280, 720).unwrap();
            if tick >= 3 {
                times.push(started.elapsed().as_secs_f64() * 1000.0);
            }
        }
        times.sort_by(f64::total_cmp);
        times[times.len() / 2]
    };
    println!(
        "adapter: {}",
        if software { "software" } else { "hardware" }
    );
    for (walls, boxes) in [("one box per solid cell", &cells), ("rows merged", &runs)] {
        println!(
            "{walls}: {} boxes, {} wall segments",
            boxes.len(),
            boxes.len() * 4
        );
        for (label, lights, shadows, vision) in [
            ("the tilemap alone", false, false, false),
            ("16 lights with shadows", true, true, false),
            ("16 lights without shadows", true, false, false),
            ("a vision alone", false, false, true),
            ("16 lights with shadows and a vision", true, true, true),
        ] {
            let (scene, extract_ms) = make(&document, boxes, lights, shadows, vision);
            let points: usize = scene.lighting_2d.lights.iter().map(|l| l.area.len()).sum();
            let frame = median(&scene);
            println!(
                "  {label}: frame median {frame:.1} ms; extraction (areas on the CPU) {extract_ms:.2} ms; {points} light area points"
            );
        }
    }
}

/// [`world_of`] for the measurement's map.
fn world_of_map(document: &Value, boxes: Vec<[i32; 4]>) -> World {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    let result = load(
        &document.to_string(),
        "scenes/measure.scene.json",
        world.registry(),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap().instantiate(&mut world).unwrap();
    wish_transform::update_global_transforms(&mut world).unwrap();
    wish_sight::install_tile_shapes(
        &mut world,
        [(
            MAP.to_owned(),
            MapShapes {
                boxes,
                outlines: Vec::new(),
            },
        )],
    );
    world
}
