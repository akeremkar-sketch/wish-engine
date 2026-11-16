//! The in-game interface over the picture (M6 WP7), on the software adapter.
//! `UPDATE_GOLDEN=1` rewrites the golden images.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use wish_ecs::World;
use wish_render::{
    Gpu, GpuOptions, RenderScene, Renderer, RgbaImage, Tolerance, UiPicture, extract, ui_elements,
};
use wish_scene::load;

const TOLERANCE: Tolerance = Tolerance {
    channel: 2,
    fraction: 0.005,
};

/// A dark scene with a panel (title, score, a button) at the top left and a
/// picture at the top right of a 1280 × 720 interface.
const SCENE: &str = r#"{
  "format": 1,
  "resources": {
    "Ambient": { "intensity": 0 },
    "Sky": { "luminance": 0 }
  },
  "entities": {
    "camera": { "components": { "Camera": {}, "Transform": {} } },
    "hud": { "components": { "UiNode": {
      "position": "Absolute", "offset": [40, 40], "padding": [20, 20, 20, 20], "gap": 10,
      "background": [0.02, 0.02, 0.05, 0.85] } } },
    "title": { "parent": "hud", "order": 0, "components": {
      "UiNode": {}, "UiText": { "text": "Wish Engine", "size": 64 } } },
    "score": { "parent": "hud", "order": 1, "components": {
      "UiNode": {}, "UiText": { "text": "Puan: 3 · Şimdi", "size": 40, "color": [1, 0.6, 0.05, 1] } } },
    "play": { "parent": "hud", "order": 2, "components": {
      "UiNode": { "padding": [10, 30, 10, 30], "background": [0.05, 0.25, 0.9, 1] },
      "UiButton": {} } },
    "play_label": { "parent": "play", "components": {
      "UiNode": {}, "UiText": { "text": "Oyna", "size": 32 } } },
    "heart": { "components": { "UiNode": {
      "position": "Absolute", "offset": [1100, 40],
      "width": { "unit": "Px", "value": 128 }, "height": { "unit": "Px", "value": 128 } },
      "UiImage": { "image": "ui/heart.png" } } }
  }
}"#;

fn world_of(source: &str) -> World {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    wish_ui::register(&mut world).unwrap();
    let result = load(source, "scenes/ui.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap().instantiate(&mut world).unwrap();
    wish_transform::update_global_transforms(&mut world).unwrap();
    wish_ui::layout(&mut world).unwrap();
    world
}

fn named(world: &World, name: &str) -> wish_core::Entity {
    world
        .entities()
        .find(|entity| {
            world
                .get::<wish_scene::instantiate::SceneId>(*entity)
                .and_then(|id| world.text(id.id))
                == Some(name)
        })
        .unwrap()
}

/// A red disc on transparency, 64 × 64.
fn heart() -> UiPicture {
    let mut rgba = Vec::new();
    for y in 0..64 {
        for x in 0..64 {
            let (dx, dy) = (x as f32 - 31.5, y as f32 - 31.5);
            let inside = dx * dx + dy * dy < 30.0 * 30.0;
            rgba.extend_from_slice(&if inside {
                [230, 20, 40, 255]
            } else {
                [0, 0, 0, 0]
            });
        }
    }
    UiPicture {
        width: 64,
        height: 64,
        rgba,
    }
}

fn gpu() -> Gpu {
    Gpu::new(GpuOptions { software: true }).expect("a software adapter")
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

fn bright(pixel: [u8; 4]) -> bool {
    pixel[0] > 200 && pixel[1] > 200 && pixel[2] > 200
}

/// How many pixels inside `[left, top, right, bottom]` pass `test`.
fn count(
    image: &RgbaImage,
    [left, top, right, bottom]: [i32; 4],
    test: impl Fn([u8; 4]) -> bool,
) -> usize {
    let mut found = 0;
    for y in top.max(0)..bottom.min(image.height as i32) {
        for x in left.max(0)..right.min(image.width as i32) {
            if test(image.pixel(x as u32, y as u32).unwrap()) {
                found += 1;
            }
        }
    }
    found
}

fn render(scene: &RenderScene, width: u32, height: u32) -> RgbaImage {
    let gpu = gpu();
    let mut renderer = Renderer::new(&gpu).unwrap();
    renderer.set_ui_picture("ui/heart.png", heart());
    let frame = renderer.render(&gpu, scene, None, width, height).unwrap();
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
    frame.image
}

#[test]
fn the_interface_is_drawn_where_the_layout_puts_it() {
    let world = world_of(SCENE);
    let scene = extract(&world).unwrap();
    assert_eq!(scene.ui.len(), 6);
    let image = render(&scene, 640, 360);

    // The 1280 × 720 interface at half size: the panel starts at (20, 20).
    let elements = ui_elements(&scene, 640, 360);
    let find = |name: &str| {
        let entity = named(&world, name);
        elements
            .iter()
            .find(|element| element.entity == entity)
            .unwrap()
            .clone()
    };
    let hud = find("hud");
    assert_eq!(&hud.bounding_box[..2], &[20, 20]);
    let title = find("title");
    assert_eq!(title.text.as_deref(), Some("Wish Engine"));
    assert_eq!(&title.bounding_box[..2], &[30, 30], "inside the padding");
    let play = find("play");
    assert_eq!(play.button, Some((false, false)));
    let heart = find("heart");
    assert_eq!(heart.bounding_box, [550, 20, 614, 84]);
    assert!(heart.image);

    // White letters inside the title's box; orange ones in the score's.
    assert!(count(&image, title.bounding_box, bright) > 100);
    let score = find("score");
    let orange = |p: [u8; 4]| p[0] > 200 && p[1] > 100 && p[1] < 230 && p[2] < 90;
    assert!(count(&image, score.bounding_box, orange) > 50);
    // The button's blue around its label.
    let [left, top, _, bottom] = play.bounding_box;
    let blue = image
        .pixel((left + 4) as u32, ((top + bottom) / 2) as u32)
        .unwrap();
    assert!(blue[2] > 200 && blue[0] < 100, "{blue:?}");
    // The picture: red in the middle, the dark sky in its corners.
    let middle = image.pixel(582, 52).unwrap();
    assert!(middle[0] > 200 && middle[1] < 80, "{middle:?}");
    let corner = image.pixel(552, 22).unwrap();
    assert!(corner[0] < 40, "{corner:?}");
    // Away from the interface the sky shows.
    assert!(image.pixel(320, 300).unwrap()[0] < 40);

    check_golden("ui", &image);
}

#[test]
fn a_hovered_button_is_lighter_and_a_pressed_one_darker() {
    let mut world = world_of(SCENE);
    let play = named(&world, "play");
    let colour = |world: &World| {
        let scene = extract(world).unwrap();
        let element = ui_elements(&scene, 640, 360)
            .into_iter()
            .find(|element| element.entity == play)
            .unwrap();
        let [left, top, _, bottom] = element.bounding_box;
        render(&scene, 640, 360)
            .pixel((left + 4) as u32, ((top + bottom) / 2) as u32)
            .unwrap()
    };
    let resting = colour(&world);
    world
        .insert(
            play,
            wish_ui::UiInteraction {
                hovered: true,
                ..Default::default()
            },
        )
        .unwrap();
    let hovered = colour(&world);
    world
        .insert(
            play,
            wish_ui::UiInteraction {
                hovered: true,
                pressed: true,
                ..Default::default()
            },
        )
        .unwrap();
    let pressed = colour(&world);
    assert!(
        hovered[2] >= resting[2] && hovered[1] > resting[1],
        "{resting:?} {hovered:?}"
    );
    assert!(pressed[2] < resting[2], "{resting:?} {pressed:?}");
}

#[test]
fn a_picture_of_another_shape_keeps_the_interface_whole_and_centred() {
    let world = world_of(SCENE);
    let scene = extract(&world).unwrap();
    // Square: the 16:9 interface is 400 × 225, 87.5 pixels from the top.
    let elements = ui_elements(&scene, 400, 400);
    let heart = elements.iter().find(|e| e.image).unwrap();
    assert_eq!(heart.bounding_box, [344, 100, 384, 140]);
    let image = render(&scene, 400, 400);
    let middle = image.pixel(364, 120).unwrap();
    assert!(middle[0] > 200, "{middle:?}");
}

#[test]
fn a_missing_picture_or_font_still_draws_the_rest() {
    let source = SCENE.replace("ui/heart.png", "ui/missing.png");
    let world = world_of(&source);
    let scene = extract(&world).unwrap();
    let gpu = gpu();
    let renderer = Renderer::new(&gpu).unwrap();
    let frame = renderer.render(&gpu, &scene, None, 320, 180).unwrap();
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
    let title = ui_elements(&scene, 320, 180)
        .into_iter()
        .find(|e| e.text.as_deref() == Some("Wish Engine"))
        .unwrap();
    assert!(count(&frame.image, title.bounding_box, bright) > 20);
}
