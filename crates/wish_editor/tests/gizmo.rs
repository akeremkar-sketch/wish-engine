//! The handles in the view (M8a WP5), dragged with the pointer like a person
//! does: the picture follows the pointer while the button is down, the scene
//! and the history are touched once, when it is let go, and Esc takes it all
//! back.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use egui::{Pos2, Rect};
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::raster::Canvas;
use wish_editor::script::run_script;

fn app(name: &str) -> EditorApp {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-g-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir,
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    // Something to move: a red cube in front of the camera.
    let added = app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "marker", "components": {
            "Transform": { "translation": [0.0, 1.0, 0.0] },
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.9, 0.1, 0.1, 1] } } }] }),
    );
    assert_eq!(added["ok"], true, "{added}");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    app.select(&["marker"]);
    play(&mut app, json!([{ "idle": 2 }]));
    app
}

fn play(app: &mut EditorApp, steps: Value) -> Canvas {
    run_script(app, &json!({ "size": [1280, 720], "steps": steps }))
        .unwrap()
        .canvas
}

fn translation(app: &mut EditorApp) -> [f64; 3] {
    let got = app.call(
        "entity_get",
        &json!({ "ids": ["marker"], "components": ["Transform"] }),
    );
    let t = &got["entities"][0]["components"]["Transform"]["translation"];
    [
        t[0].as_f64().unwrap(),
        t[1].as_f64().unwrap(),
        t[2].as_f64().unwrap(),
    ]
}

fn component(app: &mut EditorApp, name: &str) -> Value {
    let got = app.call(
        "entity_get",
        &json!({ "ids": ["marker"], "components": [name] }),
    );
    got["entities"][0]["components"][name].clone()
}

fn entries(app: &mut EditorApp) -> usize {
    app.call("edit_history", &json!({ "action": "list" }))["entries"]
        .as_array()
        .map_or(0, Vec::len)
}

fn differing(a: &Canvas, b: &Canvas) -> usize {
    a.pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.pixels.as_chunks::<4>().0)
        .filter(|(x, y)| x != y)
        .count()
}

fn at(p: Pos2) -> Value {
    json!([p.x, p.y])
}

/// The handles' places: pivot, then the ends of X, Y and Z.
fn places(app: &EditorApp) -> [Pos2; 4] {
    app.handle_places().expect("the handles are in view")
}

fn midpoint(a: Pos2, b: Pos2) -> Pos2 {
    Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)
}

/// Press on the middle of an axis handle and go on to `to`, in steps.
fn drag_steps(from: Pos2, to: Pos2) -> Value {
    let step = |t: f32| Pos2::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
    json!([
        { "events": [{ "pointer": at(from) }] },
        { "events": [{ "press": "primary" }] },
        { "events": [{ "pointer": at(step(0.34)) }] },
        { "events": [{ "pointer": at(step(0.67)) }] },
        { "events": [{ "pointer": at(to) }] }
    ])
}

fn release() -> Value {
    json!([{ "events": [{ "release": "primary" }] }, { "idle": 2 }])
}

#[test]
fn the_handles_are_drawn_on_the_selected_entity() {
    let app = app("drawn");
    let [pivot, x, y, z] = places(&app);
    let view: Rect = app.viewport();
    for p in [pivot, x, y, z] {
        assert!(view.contains(p), "{p:?} is in the view {view:?}");
    }
    // Fixed length on the screen: 110 pixels, whatever it is.
    assert!(
        ((x - pivot).length() - 110.0).abs() < 1.0,
        "{:?}",
        x - pivot
    );
    // The camera looks down -Z: X goes right, Y goes up.
    assert!(x.x > pivot.x && y.y < pivot.y);
    assert_eq!(app.handle_mode(), "move");
}

#[test]
fn dragging_a_move_handle_moves_the_entity_once_when_let_go() {
    let mut app = app("move");
    let before_entries = entries(&mut app);
    let still = play(&mut app, json!([{ "idle": 1 }]));
    let [pivot, x, ..] = places(&app);
    let from = midpoint(pivot, x);
    let to = Pos2::new(from.x + 40.0, from.y);
    play(&mut app, drag_steps(from, to));
    // While the button is down: the picture shows the move, the scene does not.
    assert!(app.dragging());
    assert_eq!(
        translation(&mut app),
        [0.0, 1.0, 0.0],
        "not changed during the drag"
    );
    assert_eq!(entries(&mut app), before_entries);
    let mid_drag = play(&mut app, json!([{ "idle": 1 }]));
    assert!(
        differing(&still, &mid_drag) > 500,
        "the picture shows the move"
    );
    // The handles went along with it (the pivot is under the pointer's move).
    let [moving, ..] = places(&app);
    assert!(
        (moving.x - (pivot.x + 40.0)).abs() < 2.5,
        "{moving:?} vs {pivot:?}"
    );
    play(&mut app, release());
    assert!(!app.dragging());
    let now = translation(&mut app);
    assert!(now[0] > 0.2, "moved along +X: {now:?}");
    assert!(
        (now[1] - 1.0).abs() < 1e-4 && now[2].abs() < 1e-4,
        "only along X: {now:?}"
    );
    assert_eq!(
        entries(&mut app),
        before_entries + 1,
        "one entry for the drag"
    );
    assert_eq!(app.status(), "moved marker");
    let history = app.call("edit_history", &json!({ "action": "list" }));
    assert!(
        history["entries"][0].to_string().contains("human"),
        "{history}"
    );
    // After it, the handles sit on the entity where it is now.
    let [after, ..] = places(&app);
    assert!((after.x - moving.x).abs() < 2.5, "{after:?} vs {moving:?}");
}

#[test]
fn escape_takes_a_drag_back() {
    let mut app = app("escape");
    let before_entries = entries(&mut app);
    let [pivot, x, ..] = places(&app);
    let from = midpoint(pivot, x);
    play(&mut app, drag_steps(from, Pos2::new(from.x + 60.0, from.y)));
    assert!(app.dragging());
    play(
        &mut app,
        json!([{ "events": [{ "key": "Escape" }] }, { "idle": 2 }]),
    );
    assert!(!app.dragging());
    play(&mut app, release());
    assert_eq!(translation(&mut app), [0.0, 1.0, 0.0]);
    assert_eq!(entries(&mut app), before_entries);
    assert_eq!(app.status(), "cancelled");
}

#[test]
fn snapping_moves_in_half_metres() {
    let mut app = app("snap");
    // The Snap button is in the menu bar; find it by its label.
    let snap = app
        .widgets()
        .iter()
        .find(|w| w.label.as_deref() == Some("Snap"))
        .map(|w| w.rect.center())
        .unwrap();
    play(
        &mut app,
        json!([{ "events": [{ "click": at(snap) }] }, { "idle": 2 }]),
    );
    let [pivot, x, ..] = places(&app);
    let from = midpoint(pivot, x);
    play(&mut app, drag_steps(from, Pos2::new(from.x + 53.0, from.y)));
    play(&mut app, release());
    let moved = translation(&mut app)[0];
    assert!(
        moved != 0.0 && ((moved / 0.5).round() * 0.5 - moved).abs() < 1e-4,
        "a multiple of 0.5: {moved}"
    );
}

#[test]
fn the_keys_choose_the_handles_and_a_ring_turns_the_entity() {
    let mut app = app("rotate");
    play(
        &mut app,
        json!([{ "events": [{ "key": "E" }] }, { "events": [{ "key": "E", "up": true }] }, { "idle": 2 }]),
    );
    assert_eq!(app.handle_mode(), "rotate");
    // The Z ring faces the camera: a circle round the pivot. The other two
    // rings are seen edge on (lines through the pivot), so a point 45 degrees
    // round the circle belongs to the Z ring alone.
    let [pivot, x, ..] = places(&app);
    let radius = (x - pivot).length() * 0.85;
    let on_ring = |degrees: f32| {
        let angle = degrees.to_radians();
        Pos2::new(
            pivot.x + radius * angle.cos(),
            pivot.y - radius * angle.sin(),
        )
    };
    let start = on_ring(45.0);
    let to = on_ring(135.0);
    play(&mut app, drag_steps(start, to));
    play(&mut app, release());
    let rotation = component(&mut app, "Transform")["rotation"].clone();
    let (z, w) = (rotation[2].as_f64().unwrap(), rotation[3].as_f64().unwrap());
    // A quarter turn about Z: z = sin 45, w = cos 45.
    assert!(
        (z - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.03,
        "{rotation}"
    );
    assert!(
        (w - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.03,
        "{rotation}"
    );
    assert_eq!(app.status(), "rotated marker");
}

#[test]
fn the_centre_handle_scales_everything_and_local_space_follows_the_entity() {
    let mut app = app("scale");
    play(
        &mut app,
        json!([{ "events": [{ "key": "R" }] }, { "events": [{ "key": "R", "up": true }] }, { "idle": 2 }]),
    );
    assert_eq!(app.handle_mode(), "scale");
    let [pivot, ..] = places(&app);
    let start = Pos2::new(pivot.x + 40.0, pivot.y);
    // Grab the centre (a press on it), then the pointer's distance from the
    // centre decides: from 0 pixels (guarded) outwards.
    play(
        &mut app,
        json!([{ "events": [{ "pointer": at(pivot) }] }, { "events": [{ "press": "primary" }] }]),
    );
    assert!(app.dragging(), "the centre handle was taken");
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": at(start) }] },
            { "events": [{ "pointer": at(Pos2::new(pivot.x + 80.0, pivot.y)) }] }
        ]),
    );
    play(&mut app, release());
    let scale = component(&mut app, "Transform")["scale"].clone();
    let s = scale[0].as_f64().unwrap();
    assert!(s > 1.5, "grew: {scale}");
    assert!(
        (scale[1].as_f64().unwrap() - s).abs() < 1e-4
            && (scale[2].as_f64().unwrap() - s).abs() < 1e-4,
        "all three: {scale}"
    );
}

#[test]
fn a_click_that_misses_the_handles_still_picks() {
    let mut app = app("pick-through");
    // A click on the sky clears the selection and takes the handles away.
    let view = app.viewport();
    let sky = Pos2::new(view.center().x, view.min.y + 15.0);
    play(
        &mut app,
        json!([{ "events": [{ "click": at(sky) }] }, { "idle": 2 }]),
    );
    assert!(app.selection().is_empty());
    assert!(
        app.handle_places().is_none(),
        "no handles without a selection"
    );
}
