//! The scene view's 2D mode (release 1.1, package 8, design note
//! `docs/design/1.1-editor-2d.md` §5): a 2D project opens in it and its
//! button switches; the wheel zooms toward the pointer and the middle
//! button or Space pans, with no orbit; a click picks a sprite by its
//! drawn rectangle and its picture's alpha; a rectangle selects; the
//! handles move in X and Y only (and snap to the grid), turn about Z only,
//! each change one call by the person; Create puts things in the middle of
//! the view; F frames; maximizing keeps working.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use egui::{Pos2, Rect, pos2};
use serde_json::{Value, json};
use wish_control::project::ProjectKind;
use wish_control::scaffold::create_project_of_kind;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::script::run_script;

fn project(name: &str, kind: ProjectKind) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-2d-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project_of_kind(&dir, "Flat", "wen", kind).unwrap();
    dir
}

fn open(dir: &Path) -> EditorApp {
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    play(&mut app, json!([{ "idle": 3 }]));
    app
}

fn play(app: &mut EditorApp, steps: Value) {
    run_script(app, &json!({ "size": [1280, 720], "steps": steps })).unwrap();
}

fn named(app: &EditorApp, text: &str) -> Rect {
    app.widgets()
        .iter()
        .filter(|w| w.role != "TextRun" && w.role != "Label")
        .find(|w| w.label.as_deref() == Some(text))
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("nothing called '{text}'"))
}

fn click(app: &mut EditorApp, at: Pos2) {
    play(
        app,
        json!([{ "events": [{ "click": [at.x, at.y] }] }, { "idle": 2 }]),
    );
}

fn menu(app: &mut EditorApp, top: &str, item: &str) {
    let open = named(app, top);
    click(app, open.center());
    let item = named(app, item);
    click(app, item.center());
}

/// Where the world point (x, y) of the plane is on the window, through the
/// 2D view's eye as it is now.
fn screen(app: &EditorApp, x: f32, y: f32) -> Pos2 {
    let eye = app.flat_eye().expect("the 2D view was drawn");
    eye.screen_at(app.scene_view_area(), glam::Vec2::new(x, y))
}

/// Presses at `from`, moves to `to` in one step, lets go.
fn drag(app: &mut EditorApp, button: &str, from: Pos2, to: Pos2) {
    play(
        app,
        json!([
            { "events": [{ "pointer": [from.x, from.y] }] },
            { "events": [{ "press": button }] },
            { "events": [{ "pointer": [(from.x + to.x) * 0.5, (from.y + to.y) * 0.5] }] },
            { "events": [{ "pointer": [to.x, to.y] }] },
            { "events": [{ "release": button }] },
            { "idle": 2 }
        ]),
    );
}

/// Clicks at the world point (x, y) of the plane.
fn click_world(app: &mut EditorApp, x: f32, y: f32) {
    let at = screen(app, x, y);
    click(app, at);
}

/// Drags the left button from one world point of the plane to another.
fn drag_world(app: &mut EditorApp, from: (f32, f32), to: (f32, f32)) {
    let (from, to) = (screen(app, from.0, from.1), screen(app, to.0, to.1));
    drag(app, "primary", from, to);
}

fn transform(app: &mut EditorApp, id: &str) -> Value {
    let got = app.call(
        "entity_get",
        &json!({ "ids": [id], "components": ["Transform"] }),
    );
    assert_eq!(got["ok"], true, "{got}");
    got["entities"][0]["components"]["Transform"].clone()
}

fn close(value: &Value, want: f64) -> bool {
    value.as_f64().is_some_and(|v| (v - want).abs() < 1e-3)
}

/// A 16 × 16 picture: its left half clear, its right half red.
fn half_picture(dir: &Path) {
    std::fs::create_dir_all(dir.join("assets/art")).unwrap();
    let mut pixels = Vec::new();
    for _ in 0..16 {
        for x in 0..16 {
            pixels.extend_from_slice(&if x < 8 {
                [0, 0, 0, 0]
            } else {
                [220, 40, 40, 255]
            });
        }
    }
    let picture = wish_render::RgbaImage {
        width: 16,
        height: 16,
        pixels,
    };
    std::fs::write(dir.join("assets/art/half.png"), picture.to_png().unwrap()).unwrap();
}

/// `hero`: the half picture at 16 pixels a meter at (2, 1) (a meter
/// square); `block`: a plain sprite a meter square at (-2, 0).
fn sprites(app: &mut EditorApp) {
    let done = app.call(
        "edit_apply",
        &json!({ "ops": [
            { "op": "spawn", "id": "hero", "components": {
                "Transform": { "translation": [2.0, 1.0, 0.0] },
                "Sprite": { "image": "assets/art/half.png", "pixels_per_unit": 16.0, "smooth": false } } },
            { "op": "spawn", "id": "block", "components": {
                "Transform": { "translation": [-2.0, 0.0, 0.0] },
                "Sprite": { "color": [0.2, 0.6, 1.0, 1.0] } } }
        ] }),
    );
    assert_eq!(done["ok"], true, "{done}");
    play(app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
}

#[test]
fn a_2d_project_opens_in_the_2d_view_and_the_button_switches() {
    let mut app = open(&project("opens", ProjectKind::D2));
    assert!(app.flat_view());
    // Over the scene's camera: its place, its height.
    let eye = app.flat_eye().unwrap();
    assert_eq!(eye.centre, glam::Vec2::ZERO);
    assert_eq!(eye.height, 10.0);
    let button = named(&app, "2D");
    assert!(app.scene_view_area().contains(button.center()));
    click(&mut app, button.center());
    assert!(!app.flat_view(), "the button switches to 3D");
    click(&mut app, button.center());
    assert!(app.flat_view(), "and back");
    // The View menu's item does the same.
    menu(&mut app, "View", "Switch the scene view between 2D and 3D");
    assert!(!app.flat_view());

    // A 3D project opens in 3D.
    let app = open(&project("opens-3d", ProjectKind::D3));
    assert!(!app.flat_view());
}

#[test]
fn the_wheel_zooms_toward_the_pointer_and_the_view_pans_without_orbiting() {
    let mut app = open(&project("moves", ProjectKind::D2));
    let area = app.scene_view_area();
    let at = area.center() + egui::vec2(120.0, -60.0);
    let before = app.flat_eye().unwrap();
    let under = before.world_at(area, at);
    play(
        &mut app,
        json!([{ "events": [{ "pointer": [at.x, at.y] }] }, { "events": [{ "scroll": [0.0, 96.0] }] }, { "idle": 2 }]),
    );
    let zoomed = app.flat_eye().unwrap();
    assert!((zoomed.height - 7.5).abs() < 1e-3, "{zoomed:?}");
    assert!(
        (zoomed.world_at(area, at) - under).length() < 1e-3,
        "the point under the pointer stays under it"
    );

    // The middle button grabs the world: it follows the pointer.
    let from = area.center();
    let to = from + egui::vec2(60.0, 20.0);
    let grabbed = zoomed.world_at(area, from);
    drag(&mut app, "middle", from, to);
    let panned = app.flat_eye().unwrap();
    assert!(
        (panned.world_at(area, to) - grabbed).length() < 1e-3,
        "{panned:?}"
    );
    assert_eq!(panned.height, zoomed.height);

    // Space and the left button grab too, and select nothing.
    play(&mut app, json!([{ "events": [{ "key": "Space" }] }]));
    let grabbed = panned.world_at(area, to);
    let back = to + egui::vec2(-90.0, 0.0);
    drag(&mut app, "primary", to, back);
    play(
        &mut app,
        json!([{ "events": [{ "key": "Space", "up": true }] }]),
    );
    let again = app.flat_eye().unwrap();
    assert!(
        (again.world_at(area, back) - grabbed).length() < 1e-3,
        "{again:?}"
    );
    assert!(app.selection().is_empty());

    // No orbit, no looking around: the eye stays top down.
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": [from.x, from.y] }] },
            { "events": [{ "press": "secondary" }] },
            { "events": [{ "mouse_delta": [80.0, 30.0] }] },
            { "events": [{ "release": "secondary" }] },
            { "idle": 2 }
        ]),
    );
    assert_eq!(app.flat_eye().unwrap(), again);
}

#[test]
fn a_click_picks_a_sprite_by_its_picture_and_a_rectangle_selects() {
    let dir = project("picks", ProjectKind::D2);
    half_picture(&dir);
    let mut app = open(&dir);
    sprites(&mut app);
    // The red half of the hero.
    click_world(&mut app, 2.25, 1.0);
    assert_eq!(app.selection(), ["hero"], "{}", app.status());
    assert_eq!(app.outlines().len(), 1);
    // Its clear half is not the hero: the click falls through to nothing.
    click_world(&mut app, 1.75, 1.0);
    assert!(app.selection().is_empty(), "{:?}", app.selection());
    // A plain sprite.
    click_world(&mut app, -2.0, 0.0);
    assert_eq!(app.selection(), ["block"]);

    // A rectangle over both selects both; over nothing, nothing.
    drag_world(&mut app, (-3.0, 2.0), (3.0, -1.0));
    let mut chosen = app.selection().to_vec();
    chosen.sort();
    assert_eq!(chosen, ["block", "hero"]);
    // The window as it looks now (not compared: kept to look at).
    let canvas = run_script(
        &mut app,
        &json!({ "size": [1280, 720], "steps": [{ "idle": 1 }] }),
    )
    .unwrap()
    .canvas;
    let picture = wish_render::RgbaImage {
        width: canvas.width,
        height: canvas.height,
        pixels: canvas.pixels.clone(),
    };
    std::fs::write(
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("editor-2d-view.png"),
        picture.to_png().unwrap(),
    )
    .unwrap();
    drag_world(&mut app, (4.0, 2.5), (5.0, 2.0));
    assert!(app.selection().is_empty(), "{:?}", app.selection());

    // Maximized (the R6 layout), the view still picks.
    menu(&mut app, "View", "Maximize the view");
    play(&mut app, json!([{ "idle": 2 }]));
    click_world(&mut app, 2.25, 1.0);
    assert_eq!(app.selection(), ["hero"]);
}

#[test]
fn the_handles_move_in_the_plane_and_turn_about_z_one_call_each() {
    let dir = project("handles", ProjectKind::D2);
    half_picture(&dir);
    let mut app = open(&dir);
    sprites(&mut app);
    app.select(&["hero"]);
    play(&mut app, json!([{ "idle": 3 }]));
    let [pivot, x_end, _, z_end] = app.handle_places().expect("handles");
    // Seen from above, Z points at the eye: its handle is not drawn.
    assert!((z_end - pivot).length() < 1.0);
    let meter = app.scene_view_area().height() / 10.0;
    let entries = |app: &mut EditorApp| {
        app.call("edit_history", &json!({ "action": "list" }))["entries"]
            .as_array()
            .unwrap()
            .clone()
    };
    let before = entries(&mut app).len();

    // The X arrow: the pointer's move up is not a move in Y.
    let on_x = pivot + (x_end - pivot) * 0.7;
    drag(
        &mut app,
        "primary",
        on_x,
        on_x + egui::vec2(2.0 * meter, -40.0),
    );
    let moved = transform(&mut app, "hero");
    assert!(close(&moved["translation"][0], 4.0), "{moved}");
    assert!(close(&moved["translation"][1], 1.0), "{moved}");
    assert!(close(&moved["translation"][2], 0.0), "{moved}");

    // The Y arrow.
    let [pivot, _, y_end, _] = app.handle_places().unwrap();
    let on_y = pivot + (y_end - pivot) * 0.7;
    drag(&mut app, "primary", on_y, on_y + egui::vec2(25.0, -meter));
    let moved = transform(&mut app, "hero");
    assert!(close(&moved["translation"][0], 4.0), "{moved}");
    assert!(close(&moved["translation"][1], 2.0), "{moved}");

    // The centre moves in the plane; with snapping on it lands on the
    // grid (half meters here: no pixel camera, no tilemap).
    let snap = named(&app, "Snap");
    click(&mut app, snap.center());
    let [pivot, ..] = app.handle_places().unwrap();
    drag(
        &mut app,
        "primary",
        pivot,
        pivot + egui::vec2(0.8 * meter, -1.3 * meter),
    );
    let moved = transform(&mut app, "hero");
    assert!(close(&moved["translation"][0], 5.0), "{moved}");
    assert!(close(&moved["translation"][1], 3.5), "{moved}");
    assert!(close(&moved["translation"][2], 0.0), "{moved}");
    click(&mut app, snap.center());

    // E: only the Z ring; a quarter turn about Z (from the right of the
    // pivot down to below it: clockwise on the screen, -90 degrees).
    play(
        &mut app,
        json!([{ "events": [{ "key": "E" }] }, { "idle": 2 }]),
    );
    assert_eq!(app.handle_mode(), "rotate");
    let [pivot, _, up, _] = app.handle_places().unwrap();
    let radius = (up - pivot).length() * 0.85;
    let start = pivot + egui::vec2(radius, 0.0);
    let end = pivot + egui::vec2(0.0, radius);
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": [start.x, start.y] }] },
            { "events": [{ "press": "primary" }] },
            { "events": [{ "pointer": [pivot.x + radius * 0.7, pivot.y + radius * 0.7] }] },
            { "events": [{ "pointer": [end.x, end.y] }] },
            { "events": [{ "release": "primary" }] },
            { "idle": 2 }
        ]),
    );
    let turned = transform(&mut app, "hero");
    let rotation = turned["rotation"].as_array().unwrap();
    assert!(
        close(&rotation[0], 0.0) && close(&rotation[1], 0.0),
        "{turned}"
    );
    assert!(
        close(&rotation[2], -std::f64::consts::FRAC_1_SQRT_2),
        "{turned}"
    );

    // Four changes, each one call by the person.
    let after = entries(&mut app);
    assert_eq!(after.len(), before + 4, "{after:?}");
    assert!(
        after
            .iter()
            .take(4)
            .all(|entry| entry.to_string().contains("human")),
        "{after:?}"
    );
}

#[test]
fn create_puts_things_in_the_middle_of_the_view_and_f_frames() {
    let dir = project("create", ProjectKind::D2);
    half_picture(&dir);
    let mut app = open(&dir);
    sprites(&mut app);
    // Slide the view so that (3, 2) is in its middle.
    let area = app.scene_view_area();
    let from = screen(&app, 3.0, 2.0);
    drag(&mut app, "middle", from, area.center());
    menu(&mut app, "Create", "Sprite");
    let made = transform(&mut app, "sprite_1");
    assert!(close(&made["translation"][0], 3.0), "{made}");
    assert!(close(&made["translation"][1], 2.0), "{made}");
    assert!(close(&made["translation"][2], 0.0), "{made}");

    // F: the hero in the middle, filling the view.
    app.select(&["hero"]);
    play(
        &mut app,
        json!([{ "idle": 2 }, { "events": [{ "pointer": [area.center().x, area.center().y] }, { "key": "F" }] }, { "idle": 2 }]),
    );
    let eye = app.flat_eye().unwrap();
    assert!(
        (eye.centre - glam::Vec2::new(2.0, 1.0)).length() < 0.1,
        "{eye:?}"
    );
    assert!(eye.height < 5.0, "{eye:?}");
    let _ = pos2(0.0, 0.0);
}
