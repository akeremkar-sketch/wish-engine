//! Sprite sheets, tint and nine slices (release 1.1, design note
//! `docs/design/1.1-sprites.md`), on the software adapter: a sheet played
//! by the animation system for a number of ticks and drawn, against golden
//! images at chosen ticks. `UPDATE_GOLDEN=1` rewrites the golden images.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use wish_ecs::World;
use wish_render::{
    Gpu, GpuOptions, RenderScene, Renderer, RgbaImage, SpriteSheet, Tolerance, UiPicture, extract,
};
use wish_scene::load;
use wish_types::time::Time;

const TOLERANCE: Tolerance = Tolerance {
    channel: 2,
    fraction: 0.005,
};

/// An orthographic camera 10 m tall over a 320 × 180 picture (18 pixels a
/// meter): a sprite walking, one falling once, a still frame tinted blue,
/// and a nine-slice panel 6 m × 3 m from a 12 × 12 picture.
const SCENE: &str = r#"{
  "format": 1,
  "resources": {
    "Ambient": { "intensity": 0 },
    "Sky": { "luminance": 0 }
  },
  "entities": {
    "camera": { "components": {
      "Camera": { "projection": "Orthographic", "ortho_height": 10 },
      "Transform": { "translation": [0, 0, 10] } } },
    "walker": { "components": {
      "Sprite": { "pixels_per_unit": 8, "smooth": false },
      "SpriteAnimator": { "sheet": "sprites/walk.sheet.json", "clip": "walk" },
      "Transform": { "translation": [-6, 2, 0] } } },
    "faller": { "components": {
      "Sprite": { "pixels_per_unit": 8, "smooth": false, "flip_x": true },
      "SpriteAnimator": { "sheet": "sprites/walk.sheet.json", "clip": "fall" },
      "Transform": { "translation": [-3, 2, 0] } } },
    "tinted": { "components": {
      "Sprite": { "pixels_per_unit": 8, "smooth": false, "color": [0.3, 0.6, 1, 1] },
      "SpriteAnimator": { "sheet": "sprites/walk.sheet.json", "frame": 2 },
      "Transform": { "translation": [0, 2, 0] } } },
    "panel": { "components": {
      "Sprite": { "image": "sprites/panel.png", "pixels_per_unit": 16, "smooth": false,
        "size": [96, 48], "slice": [4, 4, 4, 4], "layer": 1 },
      "Transform": { "translation": [3, -2, 0] } } }
  }
}"#;

/// The frames' colours: frame `i` of the sheet.
const FRAMES: [[u8; 4]; 4] = [
    [230, 40, 40, 255],
    [40, 220, 60, 255],
    [240, 240, 240, 255],
    [240, 200, 30, 255],
];

/// 64 × 16: four 16 × 16 frames side by side, each its colour with a dark
/// dot in the corner its number names (so a mirrored frame shows).
fn sheet_picture() -> UiPicture {
    let mut rgba = Vec::new();
    for y in 0..16u32 {
        for x in 0..64u32 {
            let frame = (x / 16) as usize;
            let dot = x % 16 < 4 && y < 4;
            rgba.extend_from_slice(&if dot {
                [10, 10, 10, 255]
            } else {
                FRAMES[frame]
            });
        }
    }
    UiPicture {
        width: 64,
        height: 16,
        rgba,
    }
}

/// 12 × 12: a 4-pixel dark blue border around an orange middle.
fn panel_picture() -> UiPicture {
    let mut rgba = Vec::new();
    for y in 0..12u32 {
        for x in 0..12u32 {
            let border = !(4..8).contains(&x) || !(4..8).contains(&y);
            rgba.extend_from_slice(&if border {
                [20, 30, 120, 255]
            } else {
                [250, 140, 20, 255]
            });
        }
    }
    UiPicture {
        width: 12,
        height: 12,
        rgba,
    }
}

fn sheet() -> SpriteSheet {
    SpriteSheet {
        image: "sprites/walk.png".to_owned(),
        frames: (0..4).map(|i| [i * 16, 0, 16, 16]).collect(),
        clips: BTreeMap::from([
            ("walk".to_owned(), vec![0, 1, 2, 3]),
            ("fall".to_owned(), vec![3, 2, 1]),
        ]),
    }
}

fn world() -> World {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    wish_anim::register(&mut world).unwrap();
    world.register_resource::<Time>().unwrap();
    let result = load(SCENE, "scenes/sheets.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap().instantiate(&mut world).unwrap();
    world.insert_resource(Time::at(0, 60)).unwrap();
    wish_transform::update_global_transforms(&mut world).unwrap();
    let sheet = "sprites/walk.sheet.json".to_owned();
    wish_anim::install_sprite_clips(
        &mut world,
        [
            (
                (sheet.clone(), "walk".to_owned()),
                wish_anim::SpriteClip {
                    frames: 4,
                    fps: 8.0,
                    looping: true,
                },
            ),
            (
                (sheet, "fall".to_owned()),
                wish_anim::SpriteClip {
                    frames: 3,
                    fps: 4.0,
                    looping: false,
                },
            ),
        ],
    );
    world
}

fn gpu() -> Gpu {
    Gpu::new(GpuOptions { software: true }).expect("a software adapter")
}

fn renderer(gpu: &Gpu) -> Renderer {
    let mut renderer = Renderer::new(gpu).unwrap();
    renderer.set_ui_picture("sprites/walk.png", sheet_picture());
    renderer.set_ui_picture("sprites/panel.png", panel_picture());
    renderer.set_sprite_sheet("sprites/walk.sheet.json", sheet());
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
    let diff = image.diff(&golden, TOLERANCE.channel).expect("same size");
    assert!(image.matches(&golden, TOLERANCE), "{name}: {diff:?}");
}

/// The world after `ticks` ticks of the animation system, as drawn.
fn scene_at(ticks: u32) -> RenderScene {
    let mut world = world();
    for _ in 0..ticks {
        wish_anim::step(&mut world).unwrap();
    }
    extract(&world).unwrap()
}

/// The colour at the middle of a sprite standing at (x, y) meters, away
/// from its corner dot.
fn middle(image: &RgbaImage, x: f32, y: f32) -> [u8; 4] {
    let px = (160.0 + x * 18.0) as u32;
    let py = (90.0 - y * 18.0) as u32;
    image.pixel(px + 2, py + 2).unwrap()
}

fn near(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).take(3).all(|(a, b)| a.abs_diff(b) <= 3)
}

/// What the walker, the faller and the tinted still show at each chosen
/// tick, by sheet frame: walk 0 1 2 3 at 8 a second, looping; fall 3 2 1 at
/// 4 a second, once (then it stays on 1); the still is frame 2.
#[test]
fn an_animated_sheet_at_chosen_ticks() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    // (ticks, walker's frame, faller's frame); no tick is on a frame's edge.
    for (ticks, walker, faller) in [(0u32, 0usize, 3usize), (20, 2, 2), (70, 1, 1)] {
        let scene = scene_at(ticks);
        let frame = renderer.render(&gpu, &scene, None, 320, 180).unwrap();
        assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
        let image = &frame.image;
        assert!(
            near(middle(image, -6.0, 2.0), FRAMES[walker]),
            "tick {ticks}: the walker shows frame {walker}: {:?}",
            middle(image, -6.0, 2.0)
        );
        assert!(
            near(middle(image, -3.0, 2.0), FRAMES[faller]),
            "tick {ticks}: the faller shows frame {faller}: {:?}",
            middle(image, -3.0, 2.0)
        );
        // The tint multiplies the still frame (white): it shows the tint,
        // in the picture's sRGB.
        let tinted = middle(image, 0.0, 2.0);
        assert!(
            tinted[2] > 230 && tinted[0] < 170 && tinted[0] > 120 && tinted[1] > 180,
            "tick {ticks}: tinted {tinted:?}"
        );
        // The same pictures through describe: clip and frame.
        let elements = renderer.overlay_elements(&scene, None, 320, 180).unwrap();
        let walking = elements
            .iter()
            .find_map(|e| e.animation.clone().filter(|(clip, _, _)| clip == "walk"))
            .unwrap();
        assert_eq!(walking.2 as usize, walker, "tick {ticks}");
        check_golden(&format!("sprite_sheet_tick_{ticks}"), image);
    }
}

#[test]
fn a_nine_slice_panel_keeps_its_corners() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let image = renderer
        .render(&gpu, &scene_at(0), None, 320, 180)
        .unwrap()
        .image;
    // 6 m x 3 m around (3, -2): pixels 160 + (0..6) * 18 across, 90 + (0.5..3.5)
    // * 18 down. The border is 4 picture pixels at 16 a meter: 4.5 screen
    // pixels on every side, however large the panel is drawn.
    let (left, right, top, bottom) = (160, 160 + 18 * 6, 90 + 9, 90 + 18 * 3 + 9);
    let border = [20, 30, 120, 255];
    let inside = [250, 140, 20, 255];
    for (x, y) in [
        (left + 2, top + 2),
        (right - 3, top + 2),
        (left + 2, bottom - 3),
        (right - 3, bottom - 3),
        ((left + right) / 2, top + 2),
        (left + 2, (top + bottom) / 2),
    ] {
        let p = image.pixel(x, y).unwrap();
        assert!(near(p, border), "border at ({x}, {y}): {p:?}");
    }
    for (x, y) in [
        (left + 6, top + 6),
        (right - 7, bottom - 7),
        ((left + right) / 2, (top + bottom) / 2),
    ] {
        let p = image.pixel(x, y).unwrap();
        assert!(near(p, inside), "middle at ({x}, {y}): {p:?}");
    }
    // Nine rectangles for the panel, one batch for the sheet's sprites.
    let elements = renderer
        .overlay_elements(&scene_at(0), None, 320, 180)
        .unwrap();
    let panel = elements
        .iter()
        .find(|e| e.animation.is_none() && e.particles.is_none())
        .unwrap();
    assert_eq!(panel.bounding_box, Some([160, 99, 268, 153]));
}

#[test]
fn sprites_of_one_sheet_are_one_draw_and_its_picture_is_uploaded_once() {
    let gpu = gpu();
    let renderer = renderer(&gpu);
    let scene = scene_at(0);
    let first = renderer.render(&gpu, &scene, None, 320, 180).unwrap();
    // The sky, tone mapping; the three sheet sprites in one batch (sharp
    // sampling, one picture), the panel's nine rectangles (on the layer
    // above) in another.
    assert_eq!(first.stats.draw_calls, 2 + 2, "{:?}", first.stats);
    assert_eq!(first.stats.triangles, 2 + 2 * (3 + 9));
    assert_eq!(renderer.sprite_texture_count(), 2);
    // A later tick shows other frames of the same picture: nothing new is
    // made on the GPU, by the pool or beside it.
    let made = gpu.created();
    let later = renderer
        .render(&gpu, &scene_at(20), None, 320, 180)
        .unwrap();
    assert_eq!(gpu.created(), made);
    assert_ne!(later.image, first.image);
    // A changed picture is uploaded again.
    let mut renderer = renderer;
    renderer.set_ui_picture("sprites/panel.png", sheet_picture());
    assert_eq!(renderer.sprite_texture_count(), 1);
    renderer.render(&gpu, &scene, None, 320, 180).unwrap();
    assert_eq!(renderer.sprite_texture_count(), 2);
}

#[test]
fn a_sheet_the_renderer_does_not_have_or_a_clip_it_lacks() {
    let gpu = gpu();
    let mut renderer = Renderer::new(&gpu).unwrap();
    renderer.set_ui_picture("sprites/walk.png", sheet_picture());
    renderer.set_ui_picture("sprites/panel.png", panel_picture());
    // No sheet: the animated sprites have no picture of their own and are
    // plain white rectangles; nothing fails.
    let frame = renderer.render(&gpu, &scene_at(0), None, 320, 180).unwrap();
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
    // A clip the sheet lacks: its first frame, and a warning naming the
    // entity and the clips there are.
    let mut lacking = sheet();
    lacking.clips.remove("fall");
    renderer.set_sprite_sheet("sprites/walk.sheet.json", lacking);
    let frame = renderer.render(&gpu, &scene_at(0), None, 320, 180).unwrap();
    let codes: Vec<&str> = frame.warnings.iter().map(|w| &*w.code).collect();
    assert_eq!(codes, ["sprite_clip_not_found"], "{:?}", frame.warnings);
    assert!(frame.warnings[0].hint.contains("walk"));
    assert!(near(middle(&frame.image, -3.0, 2.0), FRAMES[0]));
}
