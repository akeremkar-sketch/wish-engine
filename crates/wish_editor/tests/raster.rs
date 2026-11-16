//! The editor draws headless: the same picture every time, a golden image,
//! and input that changes values (M8a WP1).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use egui::{Event, Modifiers, PointerButton, pos2};
use wish_editor::Editor;
use wish_editor::raster::Canvas;

const BACKGROUND: [u8; 4] = [27, 27, 27, 255];

/// A small panel with the widgets the inspector is made of.
fn panel(ui: &mut egui::Ui, value: &mut f32, ticked: &mut bool, name: &mut String) {
    egui::Frame::default().inner_margin(12.0).show(ui, |ui| {
        ui.heading("Inspector");
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("translation.y (m)");
            ui.add(egui::DragValue::new(value).speed(0.1).range(-10.0..=10.0));
        });
        ui.checkbox(ticked, "visible");
        ui.horizontal(|ui| {
            ui.label("name");
            ui.text_edit_singleline(name);
        });
        ui.add(egui::Slider::new(value, -10.0..=10.0).text("y"));
        let _ = ui.button("Add component");
    });
}

fn frame(
    editor: &mut Editor,
    size: (u32, u32),
    events: Vec<Event>,
    state: &mut (f32, bool, String),
) -> Canvas {
    let mut canvas = Canvas::new(size.0, size.1, BACKGROUND);
    editor.run_frame(&mut canvas, 1.0 / 60.0, events, |ui| {
        let (value, ticked, name) = state;
        panel(ui, value, ticked, name);
    });
    canvas
}

fn png_bytes(canvas: &Canvas) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, canvas.width, canvas.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&canvas.pixels).unwrap();
    }
    out
}

fn read_png(bytes: &[u8]) -> Canvas {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    Canvas {
        width: info.width,
        height: info.height,
        pixels,
    }
}

/// The golden image is exact: the rasterizer is integer geometry and plain
/// float arithmetic, the same on every machine.
fn assert_golden(name: &str, canvas: &Canvas) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, png_bytes(canvas)).unwrap();
        return;
    }
    let expected = read_png(
        &std::fs::read(&path)
            .unwrap_or_else(|_| panic!("missing golden {name}; run with UPDATE_GOLDEN=1")),
    );
    assert_eq!(
        (expected.width, expected.height),
        (canvas.width, canvas.height)
    );
    let differing = expected
        .pixels
        .iter()
        .zip(&canvas.pixels)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing, 0,
        "{name}: {differing} bytes differ from the golden image"
    );
}

fn click(x: f32, y: f32) -> Vec<Event> {
    let at = pos2(x, y);
    vec![
        Event::PointerMoved(at),
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    ]
}

#[test]
fn the_inspector_panel_matches_its_golden_image() {
    let mut editor = Editor::new();
    let mut state = (1.5, true, "crate".to_owned());
    // A few frames: the first only sets the layout up and makes the font atlas.
    let mut canvas = frame(&mut editor, (360, 200), Vec::new(), &mut state);
    for _ in 0..3 {
        canvas = frame(&mut editor, (360, 200), Vec::new(), &mut state);
    }
    assert!(
        canvas
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| *p != BACKGROUND),
        "something is drawn"
    );
    assert_golden("inspector.png", &canvas);
}

#[test]
fn two_editors_draw_the_same_picture() {
    let picture = || {
        let mut editor = Editor::new();
        let mut state = (1.5, true, "crate".to_owned());
        let mut canvas = Canvas::new(1, 1, BACKGROUND);
        for _ in 0..4 {
            canvas = frame(&mut editor, (360, 200), Vec::new(), &mut state);
        }
        canvas
    };
    assert_eq!(picture(), picture());
}

#[test]
fn a_click_changes_a_value() {
    let mut editor = Editor::new();
    let mut state = (1.5, true, "crate".to_owned());
    for _ in 0..3 {
        frame(&mut editor, (360, 200), Vec::new(), &mut state);
    }
    // The checkbox "visible" is on the third row; find it by scanning clicks
    // down the left column until the value flips (layout is deterministic).
    let before = state.1;
    let mut flipped = false;
    for y in (20..140).step_by(6) {
        frame(&mut editor, (360, 200), click(24.0, y as f32), &mut state);
        frame(&mut editor, (360, 200), Vec::new(), &mut state);
        if state.1 != before {
            flipped = true;
            break;
        }
    }
    assert!(flipped, "a click on the checkbox changed its value");
}

/// Timing for the design note (M8a §2): run by hand in release mode with
/// `cargo test --release -p wish_editor --test raster -- --ignored --nocapture`.
#[test]
#[ignore = "measures time; run by hand in release mode"]
fn frame_time_at_full_hd() {
    let mut editor = Editor::new();
    let mut state = (1.5, true, "crate".to_owned());
    let mut canvas = Canvas::new(1920, 1080, BACKGROUND);
    for _ in 0..5 {
        canvas = Canvas::new(1920, 1080, BACKGROUND);
        editor.run_frame(&mut canvas, 1.0 / 60.0, Vec::new(), |ui| {
            let (value, ticked, name) = &mut state;
            for _ in 0..6 {
                panel(ui, value, ticked, name);
            }
        });
    }
    let frames = 60;
    let (mut t_layout, mut t_canvas, mut t_paint, mut triangles) = (0.0, 0.0, 0.0, 0);
    for _ in 0..frames {
        let started = std::time::Instant::now();
        let frame = editor.layout((1920, 1080), 1.0 / 60.0, Vec::new(), |ui| {
            // Many widgets: the pessimistic case for layout and triangles.
            egui::Panel::left("hierarchy").show(ui, |ui| {
                for i in 0..60 {
                    ui.label(format!("entity_{i}"));
                }
            });
            egui::Panel::right("inspector").show(ui, |ui| {
                let (value, ticked, name) = &mut state;
                for _ in 0..8 {
                    panel(ui, value, ticked, name);
                }
            });
        });
        t_layout += started.elapsed().as_secs_f64();
        let started = std::time::Instant::now();
        canvas = Canvas::new(1920, 1080, BACKGROUND);
        t_canvas += started.elapsed().as_secs_f64();
        let started = std::time::Instant::now();
        editor.paint(&frame, &mut canvas);
        t_paint += started.elapsed().as_secs_f64();
        triangles = frame.triangles();
    }
    let each = |t: f64| t * 1000.0 / f64::from(frames);
    println!(
        "1920x1080 editor frame: layout {:.2} ms, canvas {:.2} ms, paint {:.2} ms, {triangles} triangles (budget 6 ms for layout + paint)",
        each(t_layout),
        each(t_canvas),
        each(t_paint)
    );
    assert!(canvas.width == 1920);
}
