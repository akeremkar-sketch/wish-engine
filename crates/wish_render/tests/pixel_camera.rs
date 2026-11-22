//! The pixel-perfect camera and sorting layers (release 1.1, package 6,
//! design note `docs/design/1.1-editor-2d.md` §2 and §3), on the software
//! adapter: a game of 80 × 45 pixels drawn at two window sizes, scaled by a
//! whole number with bars around it (each of the game's pixels an s × s
//! square, checked pixel by pixel, and golden images); a sprite moved by
//! less than a game pixel drawn on the same pixels (no shimmer); a y-sorted
//! layer and orders within a layer; what is under a point (by the sprite's
//! picture's alpha); a camera without a pixel camera drawn as before.
//! `UPDATE_GOLDEN=1` rewrites the golden images.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use wish_ecs::World;
use wish_render::{
    Gpu, GpuOptions, RenderScene, Renderer, RgbaImage, Tolerance, UiPicture, extract,
};
use wish_scene::load;

/// The camera a hair off the game's pixels; a pixel camera of 80 × 45 game
/// pixels at 8 a meter (10 × 5.625 m) with dark blue bars; an 8 × 8 sprite
/// (a meter) whose bottom left corner is a little past (1, 0.5).
const SCENE: &str = r#"{
  "format": 1,
  "resources": {
    "Ambient": { "intensity": 0 },
    "Sky": { "luminance": 0 }
  },
  "entities": {
    "camera": { "components": {
      "Camera": {},
      "PixelCamera": { "resolution": [80, 45], "pixels_per_unit": 8, "bars": [0, 0, 0.2, 1] },
      "Transform": { "translation": [0.02, -0.01, 10] } } },
    "hero": { "components": {
      "Sprite": { "image": "art/hero.png", "pixels_per_unit": 8, "anchor": [0, 0], "smooth": false },
      "Transform": { "translation": [1.03, 0.51, 0] } } }
  }
}"#;

/// The hero's texel (x, y) from its top left: every texel its own colour,
/// the top right corner clear.
fn texel(x: u32, y: u32) -> [u8; 4] {
    if (x, y) == (7, 0) {
        [0, 0, 0, 0]
    } else {
        [40 + x as u8 * 25, 40 + y as u8 * 25, 200, 255]
    }
}

fn hero() -> UiPicture {
    let mut rgba = Vec::new();
    for y in 0..8 {
        for x in 0..8 {
            rgba.extend_from_slice(&texel(x, y));
        }
    }
    UiPicture {
        width: 8,
        height: 8,
        rgba,
    }
}

fn scene_from(text: &str) -> RenderScene {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    let result = load(text, "scenes/pixel.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap().instantiate(&mut world).unwrap();
    wish_transform::update_global_transforms(&mut world).unwrap();
    extract(&world).unwrap()
}

fn scene(change: impl FnOnce(&mut serde_json::Value)) -> RenderScene {
    let mut document: serde_json::Value = serde_json::from_str(SCENE).unwrap();
    change(&mut document);
    scene_from(&document.to_string())
}

fn gpu() -> Gpu {
    Gpu::new(GpuOptions { software: true }).expect("a software adapter")
}

fn renderer(gpu: &Gpu) -> Renderer {
    let mut renderer = Renderer::new(gpu).unwrap();
    renderer.set_ui_picture("art/hero.png", hero());
    renderer.set_ui_picture("art/block.png", block());
    renderer
}

/// A 16 × 16 white square (tinted by the sprites that show it).
fn block() -> UiPicture {
    UiPicture {
        width: 16,
        height: 16,
        rgba: [255u8; 16 * 16 * 4].to_vec(),
    }
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

fn close(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).take(3).all(|(a, b)| a.abs_diff(b) <= 1)
}

/// The bars' colour: linear 0.2 blue in sRGB.
const BAR: [u8; 4] = [0, 0, 124, 255];

/// At 176 × 99 the game is drawn at 2 (160 × 90 at (8, 4)); at 250 × 140 at
/// 3 (240 × 135 at (5, 2)). The hero's corner is snapped to game pixel
/// (8, 4) from the camera, which is snapped to (0, 0): the hero covers
/// game pixels 48..56 across and 10..18 down (the camera at the game
/// picture's pixel corner (40, 22)). Every texel is an s × s square;
/// everything outside the game's picture is the bars' colour.
#[test]
fn the_game_is_scaled_by_a_whole_number_with_bars_at_two_window_sizes() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    for (width, height, scale, left, top, name) in [
        (176u32, 99u32, 2u32, 8u32, 4u32, "pixel_camera_x2"),
        (250, 140, 3, 5, 2, "pixel_camera_x3"),
    ] {
        let frame = renderer
            .render(&gpu, &scene(|_| {}), None, width, height)
            .unwrap();
        assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
        let image = &frame.image;
        let (game_w, game_h) = (80 * scale, 45 * scale);
        for y in 0..height {
            for x in 0..width {
                let inside = x >= left && x < left + game_w && y >= top && y < top + game_h;
                let got = image.pixel(x, y).unwrap();
                if !inside {
                    assert_eq!(got, BAR, "{name}: bar pixel ({x}, {y})");
                    continue;
                }
                let (gx, gy) = ((x - left) / scale, (y - top) / scale);
                let want = if (48..56).contains(&gx) && (10..18).contains(&gy) {
                    let t = texel(gx - 48, gy - 10);
                    if t[3] == 0 { [0, 0, 0, 255] } else { t }
                } else {
                    [0, 0, 0, 255]
                };
                assert!(
                    close(got, want),
                    "{name}: pixel ({x}, {y}), game pixel ({gx}, {gy}): {got:?}, not {want:?}"
                );
            }
        }
        check_golden(name, image);
    }
}

/// A sprite moved by less than half a game pixel, or the camera, lands on
/// the very same pixels: pixel art does not shimmer.
#[test]
fn moving_by_less_than_a_game_pixel_draws_the_same_pixels() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let first = renderer
        .render(&gpu, &scene(|_| {}), None, 176, 99)
        .unwrap()
        .image;
    for (hero_x, camera_x) in [(1.04, 0.02), (1.06, 0.02), (1.03, 0.05), (0.99, -0.04)] {
        let moved = renderer
            .render(
                &gpu,
                &scene(|document| {
                    document["entities"]["hero"]["components"]["Transform"]["translation"][0] =
                        serde_json::json!(hero_x);
                    document["entities"]["camera"]["components"]["Transform"]["translation"][0] =
                        serde_json::json!(camera_x);
                }),
                None,
                176,
                99,
            )
            .unwrap()
            .image;
        assert_eq!(
            moved.pixels, first.pixels,
            "hero at {hero_x}, camera at {camera_x}"
        );
    }
    // A whole game pixel to the right moves it by the scale.
    let stepped = renderer
        .render(
            &gpu,
            &scene(|document| {
                document["entities"]["hero"]["components"]["Transform"]["translation"][0] =
                    serde_json::json!(1.03 + 0.125);
            }),
            None,
            176,
            99,
        )
        .unwrap()
        .image;
    // Game pixel (48, 10) is now black, (49, 10) the hero's first texel.
    assert!(close(
        stepped.pixel(8 + 48 * 2, 4 + 10 * 2).unwrap(),
        [0, 0, 0, 255]
    ));
    assert!(close(
        stepped.pixel(8 + 49 * 2, 4 + 10 * 2).unwrap(),
        texel(0, 0)
    ));
}

/// Without bars the game is still scaled by a whole number; the rest of the
/// screen shows more of the world. A camera without a pixel camera draws as
/// it always did (the projection of `ortho_height`).
#[test]
fn without_bars_more_of_the_world_shows_and_a_plain_camera_is_unchanged() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let open = renderer
        .render(
            &gpu,
            &scene(|document| {
                document["entities"]["camera"]["components"]["PixelCamera"]["letterbox"] =
                    serde_json::json!(false);
            }),
            None,
            176,
            99,
        )
        .unwrap()
        .image;
    // The corner that was a bar is the black sky; the hero where it was.
    assert!(close(open.pixel(0, 0).unwrap(), [0, 0, 0, 255]));
    assert!(close(
        open.pixel(8 + 48 * 2, 4 + 10 * 2).unwrap(),
        texel(0, 0)
    ));
    // The same scene with a plain orthographic camera 5.625 m tall at the
    // hero's snapped place draws the old way: the hero 99 / 5.625 = 17.6
    // pixels a meter, not snapped, no bars.
    let plain = renderer
        .render(
            &gpu,
            &scene(|document| {
                let camera = &mut document["entities"]["camera"]["components"];
                camera.as_object_mut().unwrap().remove("PixelCamera");
                camera["Camera"] =
                    serde_json::json!({ "projection": "Orthographic", "ortho_height": 5.625 });
            }),
            None,
            176,
            99,
        )
        .unwrap()
        .image;
    assert!(close(plain.pixel(0, 0).unwrap(), [0, 0, 0, 255]));
    assert_ne!(plain.pixels, open.pixels);
}

/// A y-sorted layer: of two overlapping sprites, the lower one (a smaller
/// y) is drawn over the higher one, whatever their entity order; without
/// it the later entity is on top. An order within the layer wins over the
/// height. Sprites of a lower layer stay under.
const SORTED: &str = r#"{
  "format": 1,
  "resources": {
    "Ambient": { "intensity": 0 },
    "Sky": { "luminance": 0 }
  },
  "entities": {
    "camera": { "components": {
      "Camera": {},
      "PixelCamera": { "resolution": [80, 45], "pixels_per_unit": 8 },
      "Transform": { "translation": [0, 0, 10] } } },
    "layers": { "components": {
      "SortingLayer": { "name": "actors", "layer": 1, "y_sort": true } } },
    "a_low": { "components": {
      "Sprite": { "image": "art/block.png", "pixels_per_unit": 8, "layer": 1, "color": [1, 0, 0, 1], "smooth": false },
      "Transform": { "translation": [-1, -0.5, 0] } } },
    "b_high": { "components": {
      "Sprite": { "image": "art/block.png", "pixels_per_unit": 8, "layer": 1, "color": [0, 1, 0, 1], "smooth": false },
      "Transform": { "translation": [-0.5, 0, 0] } } },
    "c_ground": { "components": {
      "Sprite": { "image": "art/block.png", "pixels_per_unit": 8, "layer": 0, "color": [1, 1, 1, 1], "smooth": false },
      "Transform": { "translation": [-0.75, -0.25, 0], "scale": [2, 2, 1] } } },
    "d_lifted": { "components": {
      "Sprite": { "image": "art/block.png", "pixels_per_unit": 8, "layer": 1, "order": 1, "color": [0, 0, 1, 1], "smooth": false },
      "Transform": { "translation": [2, 1, 0] } } },
    "e_under": { "components": {
      "Sprite": { "image": "art/block.png", "pixels_per_unit": 8, "layer": 1, "color": [1, 1, 0, 1], "smooth": false },
      "Transform": { "translation": [2.5, 0.5, 0] } } }
  }
}"#;

#[test]
fn a_y_sorted_layer_puts_the_lower_sprite_in_front_and_an_order_wins() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    // At 160 × 90 (scale 2, no bars) the camera is at pixel (80, 44)... the
    // game picture's middle corner (40, 22) times 2; a meter is 16 pixels.
    let pixel = |x: f32, y: f32| ((80.0 + x * 16.0) as u32, (44.0 - y * 16.0) as u32);
    let sorted = scene_from(SORTED);
    let frame = renderer.render(&gpu, &sorted, None, 160, 90).unwrap();
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
    let image = &frame.image;
    // Where the two overlap, (-0.6, -0.1): the lower (red) one in front.
    let (x, y) = pixel(-0.6, -0.1);
    assert!(close(image.pixel(x, y).unwrap(), [255, 0, 0, 255]));
    // Where the ground shows alone, under both.
    let (x, y) = pixel(-2.5, -2.0);
    assert!(close(image.pixel(x, y).unwrap(), [255, 255, 255, 255]));
    // The blue one with order 1 is over the yellow one below it.
    let (x, y) = pixel(2.6, 0.6);
    assert!(close(image.pixel(x, y).unwrap(), [0, 0, 255, 255]));
    check_golden("pixel_camera_sorted", image);

    // What a click at the overlap finds, front first, by the pictures'
    // alpha.
    let entity_named = |scene: &RenderScene, x: f32, y: f32| {
        let (px, py) = pixel(x, y);
        renderer
            .sprites_under(scene, None, (160, 90), [px as f32 + 0.5, py as f32 + 0.5])
            .unwrap()
            .iter()
            .map(|under| under.layer * 10 + under.order)
            .collect::<Vec<_>>()
    };
    assert_eq!(entity_named(&sorted, -0.6, -0.1), vec![10, 10, 0]);

    // Without y-sorting the later entity (green) is drawn over.
    let unsorted = scene_from(&SORTED.replace("\"y_sort\": true", "\"y_sort\": false"));
    let image = renderer
        .render(&gpu, &unsorted, None, 160, 90)
        .unwrap()
        .image;
    let (x, y) = pixel(-0.6, -0.1);
    assert!(close(image.pixel(x, y).unwrap(), [0, 255, 0, 255]));
    assert_eq!(unsorted.layer_name(1), Some("actors"));
    assert!(!unsorted.y_sorted(1));
    assert!(sorted.y_sorted(1));
}

/// A click on a clear texel of a sprite falls through it.
#[test]
fn a_clear_texel_is_not_under_the_pointer() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let scene = scene(|_| {});
    // At 176 × 99: game pixel (gx, gy) is window pixel (8 + 2 gx, 4 + 2 gy).
    let at = |gx: u32, gy: u32| [(8 + 2 * gx) as f32 + 1.0, (4 + 2 * gy) as f32 + 1.0];
    let under = renderer
        .sprites_under(&scene, None, (176, 99), at(48, 10))
        .unwrap();
    assert_eq!(under.len(), 1);
    assert_eq!(under[0].texel, Some([0, 0]));
    // The top right texel (7, 0) is clear.
    assert!(
        renderer
            .sprites_under(&scene, None, (176, 99), at(55, 10))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        renderer
            .sprites_under(&scene, None, (176, 99), at(54, 10))
            .unwrap()[0]
            .texel,
        Some([6, 0])
    );
    // In the bars: nothing.
    assert!(
        renderer
            .sprites_under(&scene, None, (176, 99), [2.0, 2.0])
            .unwrap()
            .is_empty()
    );
}
