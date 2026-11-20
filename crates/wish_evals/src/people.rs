//! The scorecard's "people" measure: a result is opened in the editor and
//! changed by hand, the way the editor's own tests do it (headless, input
//! events, widgets found by name in the accessibility tree). No model runs.
//!
//! The change: select an entity of the result (one the agent made or
//! changed, else any placed entity), click the x box of its `translation` in
//! the inspector, type a new value and press Enter. It passes when the value
//! is in the scene afterwards, the change entered the history as one
//! operation by `human`, and `check` reports no more errors than before.

use std::path::Path;

use serde_json::{Value, json};
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::script::run_script;

/// The editor's window for the measure: tall, so that a long inspector
/// needs no scrolling.
const SIZE: [f32; 2] = [1280.0, 2000.0];

/// The default scene's file as JSON (to find what the agent made).
pub fn scene_entities(dir: &Path) -> Value {
    let wish: Value = std::fs::read_to_string(dir.join("wish.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    let scene = wish["scene"].as_str().unwrap_or("scenes/main.scene.json");
    std::fs::read_to_string(dir.join(scene))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|s| s["entities"].clone())
        .unwrap_or(Value::Null)
}

/// The entity to change, by id within each step: the first entity the agent
/// made that has a `Transform`; else the first it changed that is not a
/// camera or a light; else the first it changed; else the first placed
/// entity that is not a camera or a light.
fn pick(before: &Value, after: &Value) -> Option<String> {
    let placed = |e: &Value| e["components"]["Transform"].is_object();
    let plain = |e: &Value| {
        !e["components"]["Camera"].is_object() && !e["components"]["DirectionalLight"].is_object()
    };
    let map = after.as_object()?;
    let new = |id: &str| before.get(id).is_none();
    let changed = |id: &str, e: &Value| before.get(id) != Some(e);
    map.iter()
        .find(|(id, e)| placed(e) && new(id))
        .or_else(|| {
            map.iter()
                .find(|(id, e)| placed(e) && plain(e) && changed(id, e))
        })
        .or_else(|| map.iter().find(|(id, e)| placed(e) && changed(id, e)))
        .or_else(|| map.iter().find(|(_, e)| placed(e) && plain(e)))
        .map(|(id, _)| id.clone())
}

fn play(app: &mut EditorApp, steps: Value) -> Result<(), String> {
    run_script(app, &json!({ "size": SIZE, "steps": steps }))
        .map(|_| ())
        .map_err(|p| p.message.clone())
}

/// The centre of the first input box to the right of the inspector's label
/// `label`.
fn field(app: &EditorApp, label: &str) -> Option<(f32, f32)> {
    let widgets = app.widgets();
    // The inspector is the right-hand panel; its width follows what it
    // shows, so its left edge is read from its heading.
    let left = widgets
        .iter()
        .find(|w| w.role == "Label" && w.value.as_deref() == Some("INSPECTOR"))
        .map_or(640.0, |w| w.rect.min.x);
    let name = widgets
        .iter()
        .filter(|w| w.role == "Label" && w.value.as_deref() == Some(label))
        .filter(|w| w.rect.min.x >= left)
        .map(|w| w.rect)
        .next()?;
    widgets
        .iter()
        .filter(|w| ["SpinButton", "TextInput"].contains(&w.role.as_str()))
        .filter(|w| (w.rect.center().y - name.center().y).abs() < 8.0 && w.rect.min.x > name.max.x)
        .min_by(|a, b| a.rect.min.x.total_cmp(&b.rect.min.x))
        .map(|w| (w.rect.center().x, w.rect.center().y))
}

fn errors(app: &mut EditorApp) -> u64 {
    app.call("check", &json!({}))["errors"]
        .as_u64()
        .unwrap_or(u64::MAX)
}

fn history_len(app: &mut EditorApp) -> usize {
    app.call("edit_history", &json!({ "action": "list" }))["entries"]
        .as_array()
        .map_or(0, Vec::len)
}

/// Opens the project in the editor and makes one change by hand. `Ok` says
/// what was changed; `Err` says what went wrong.
pub fn edit_by_hand(dir: &Path, before: &Value) -> Result<String, String> {
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .map_err(|p| format!("the editor cannot open the project: {}", p.message))?;
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    play(&mut app, json!([{ "idle": 2 }]))?;
    let after = scene_entities(dir);
    let id = pick(before, &after).ok_or("the scene has no placed entity to change")?;
    let errors_before = errors(&mut app);
    let entries_before = history_len(&mut app);
    let read_x = |app: &mut EditorApp| -> Option<f64> {
        let got = app.call(
            "entity_get",
            &json!({ "ids": [id], "components": ["Transform"] }),
        );
        got["entities"][0]["components"]["Transform"]["translation"][0].as_f64()
    };
    let old = read_x(&mut app).ok_or(format!("'{id}' has no translation to change"))?;
    app.select(&[id.as_str()]);
    play(&mut app, json!([{ "idle": 2 }]))?;
    if app.selection() != [id.as_str()] {
        return Err(format!("'{id}' cannot be selected in the editor"));
    }
    let (x, y) = field(&app, "translation").ok_or_else(|| {
        let labels: Vec<&str> = app
            .widgets()
            .iter()
            .filter(|w| w.role == "Label")
            .filter_map(|w| w.value.as_deref())
            .take(60)
            .collect();
        format!("the inspector shows no translation box for '{id}' (it shows {labels:?})")
    })?;
    let new = ((old + 0.5) * 100.0).round() / 100.0;
    play(
        &mut app,
        json!([
            { "events": [{ "click": [x, y] }] },
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": format!("{new}") }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    )?;
    let now = read_x(&mut app).ok_or(format!("'{id}' lost its translation"))?;
    if (now - new).abs() > 1e-3 {
        return Err(format!(
            "typing {new} into '{id}' translation x left it at {now} (status: {})",
            app.status()
        ));
    }
    if history_len(&mut app) != entries_before + 1 {
        return Err("the change did not enter the history as one operation".to_owned());
    }
    let history = app.call("edit_history", &json!({ "action": "list" }));
    if history["entries"][0]["actor"] != "human" {
        return Err("the change is not signed by a human".to_owned());
    }
    let errors_after = errors(&mut app);
    if errors_after > errors_before {
        return Err(format!(
            "check found {errors_after} errors after the change, {errors_before} before"
        ));
    }
    Ok(format!("{id}: translation x {old} -> {new}"))
}

/// Copies a project to `to` (for measures that change it), leaving out the
/// agent's transcript.
pub fn copy_project(from: &Path, to: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(to);
    copy_dir(from, to).map_err(|e| format!("cannot copy the project: {e}"))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    let mut entries: Vec<_> = std::fs::read_dir(from)?.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if entry.file_name() != "transcript.jsonl" {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn the_agents_entity_is_picked_first() {
        let before = json!({
            "camera": { "components": { "Camera": {}, "Transform": {} } },
            "ground": { "components": { "Transform": {} } } });
        let mut after = before.clone();
        assert_eq!(pick(&before, &after).as_deref(), Some("ground"));
        after["player"] = json!({ "components": { "Transform": { "translation": [1, 0, 0] } } });
        assert_eq!(pick(&before, &after).as_deref(), Some("player"));
        // A new entity comes before a changed one.
        after["ground"]["components"]["Material"] = json!({});
        assert_eq!(pick(&before, &after).as_deref(), Some("player"));
        // A changed entity comes before an unchanged one, a camera last.
        let mut moved = before.clone();
        moved["camera"]["components"]["Transform"] = json!({ "translation": [0, 9, 9] });
        assert_eq!(pick(&before, &moved).as_deref(), Some("camera"));
        moved["ground"]["components"]["Material"] = json!({});
        assert_eq!(pick(&before, &moved).as_deref(), Some("ground"));
        assert_eq!(
            pick(
                &before,
                &json!({ "ui": { "components": { "UiNode": {} } } })
            ),
            None
        );
    }
}
