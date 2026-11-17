//! The game view (M9): beside the scene view in a wide window, the game
//! as the player sees it (the scene's camera, the game's interface); the
//! scene view keeps the editor's own camera while the game runs; the game
//! view can be hidden, and the choice is kept.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::script::run_script;

fn app(name: &str) -> (EditorApp, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-gv-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    std::fs::remove_dir_all(dir.join("scripts")).unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    (EditorApp::new(workspace), dir)
}

fn play(app: &mut EditorApp, size: [u32; 2], steps: Value) {
    run_script(app, &json!({ "size": size, "steps": steps })).unwrap();
}

/// The average colour of a part of the canvas.
fn mean(app: &EditorApp, area: egui::Rect) -> [f32; 3] {
    let canvas = app.canvas();
    let mut sum = [0.0f32; 3];
    let mut n = 0.0;
    for y in (area.min.y as u32 + 30)..(area.max.y as u32 - 10) {
        for x in (area.min.x as u32 + 10)..(area.max.x as u32 - 10) {
            let at = ((y * canvas.width + x) * 4) as usize;
            for (c, total) in sum.iter_mut().enumerate() {
                *total += f32::from(canvas.pixels[at + c]);
            }
            n += 1.0;
        }
    }
    sum.map(|v| v / n)
}

#[test]
fn the_game_view_is_beside_the_scene_view_in_a_wide_window() {
    let (mut app, dir) = app("wide");
    let prefs = dir.join("prefs").join("editor.json");
    app.use_preferences(prefs.clone());
    play(&mut app, [1600, 900], json!([{ "idle": 3 }]));
    let game = app.game_view_area().expect("shown in a wide window");
    let scene = app.scene_view_area();
    assert!(scene.max.x < game.min.x, "{scene:?} {game:?}");
    // Something is drawn in it: the sky above the ground, not the backdrop.
    let colour = mean(&app, game);
    assert!(colour[2] > 60.0, "{colour:?}");
    // A narrow window has room for the scene view only.
    play(&mut app, [1280, 720], json!([{ "idle": 2 }]));
    assert_eq!(app.game_view_area(), None);
    // Hidden by its button, and the choice is kept.
    play(&mut app, [1600, 900], json!([{ "idle": 2 }]));
    let toggle = app
        .widgets()
        .iter()
        .find(|w| w.label.as_deref() == Some("Game view"))
        .map(|w| w.rect)
        .expect("the Game view button");
    play(
        &mut app,
        [1600, 900],
        json!([{ "events": [{ "click": [toggle.center().x, toggle.center().y] }] }, { "idle": 2 }]),
    );
    assert_eq!(app.game_view_area(), None);
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    assert_eq!(saved["game_view"], false);
}

#[test]
fn while_the_game_runs_the_scene_view_keeps_the_editors_camera() {
    let (mut app, _dir) = app("playing");
    play(&mut app, [1600, 900], json!([{ "idle": 3 }]));
    let scene = app.scene_view_area();
    // The editor's camera looks somewhere else: up into the sky.
    let start = [scene.center().x, scene.center().y];
    play(
        &mut app,
        [1600, 900],
        json!([
            { "events": [{ "pointer": start }, { "press": "secondary" }] },
            { "events": [{ "mouse_delta": [0, -300] }] },
            { "events": [{ "release": "secondary" }] },
            { "idle": 2 }
        ]),
    );
    let looking_up = mean(&app, app.scene_view_area());
    play(
        &mut app,
        [1600, 900],
        json!([{ "events": [{ "key": "P", "ctrl": true }] }, { "idle": 5 }]),
    );
    assert!(app.playing());
    let game = app.game_view_area().expect("the game view");
    // The scene view still looks where the editor's camera looks; the game
    // view looks through the scene's camera (at the ground as well).
    let scene_now = mean(&app, app.scene_view_area());
    let game_now = mean(&app, game);
    let close = |a: [f32; 3], b: [f32; 3]| (0..3).all(|c| (a[c] - b[c]).abs() < 12.0);
    assert!(close(scene_now, looking_up), "{scene_now:?} {looking_up:?}");
    assert!(!close(scene_now, game_now), "{scene_now:?} {game_now:?}");
}
