//! The inspector (M8a WP4), driven by input like a person: widgets are found
//! by name through the accessibility tree, clicked, dragged and typed into.
//! Every change is one control-plane call by `human`; a component defined
//! in Wasm game code is edited with no editor code written for it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::OnceLock;

use egui::Rect;
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::script::run_script;
use wish_editor::{EditorApp, has_widget};
use wish_script::build::{SCRIPTS_ENTRY, Toolchain, pack_sdk};

fn app(name: &str) -> (EditorApp, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-i-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    (EditorApp::new(workspace), dir)
}

fn play(app: &mut EditorApp, steps: Value) {
    run_script(app, &json!({ "size": [1280, 720], "steps": steps })).unwrap();
}

/// The widget a person would use for the field `label` of the inspector:
/// the first one to the right of the label, on its row.
fn field(app: &EditorApp, label: &str) -> Rect {
    let widgets = app.widgets();
    let name = widgets
        .iter()
        .filter(|w| w.role == "Label" && w.value.as_deref() == Some(label))
        .filter(|w| w.rect.min.x > 900.0)
        .map(|w| w.rect)
        .next()
        .unwrap_or_else(|| panic!("no field '{label}' in the inspector"));
    widgets
        .iter()
        .filter(|w| {
            [
                "SpinButton",
                "CheckBox",
                "ComboBox",
                "ColorWell",
                "TextInput",
            ]
            .contains(&w.role.as_str())
        })
        .filter(|w| (w.rect.center().y - name.center().y).abs() < 8.0 && w.rect.min.x > name.max.x)
        .min_by(|a, b| a.rect.min.x.total_cmp(&b.rect.min.x))
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("no widget next to '{label}'"))
}

/// A widget whose label or value is `text` (a button, a list choice).
fn named(app: &EditorApp, text: &str) -> Rect {
    app.widgets()
        .iter()
        .filter(|w| w.role != "TextRun" && w.role != "Label")
        .find(|w| w.label.as_deref() == Some(text) || w.value.as_deref() == Some(text))
        .map(|w| w.rect)
        .unwrap_or_else(|| {
            let all: Vec<_> = app
                .widgets()
                .iter()
                .map(|w| (&w.role, &w.label, &w.value))
                .collect();
            panic!("nothing called '{text}': {all:?}")
        })
}

fn click_at(rect: Rect) -> Value {
    json!({ "events": [{ "click": [rect.center().x, rect.center().y] }] })
}

/// Clicks a number box, types a value and presses Enter.
fn type_into(app: &mut EditorApp, rect: Rect, text: &str) {
    play(
        app,
        json!([
            click_at(rect),
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": text }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
}

/// "Add component...", type a name into its search box, click the match.
fn add_component(app: &mut EditorApp, name: &str) {
    let add = named(app, "Add component...");
    play(
        app,
        json!([click_at(add), { "idle": 2 }, { "events": [{ "text": name }] }, { "idle": 2 }]),
    );
    let choice = app
        .widgets()
        .iter()
        .filter(|w| w.role == "Button" && w.label.as_deref() == Some(name))
        .map(|w| w.rect)
        .next()
        .unwrap_or_else(|| panic!("'{name}' is not offered"));
    play(app, json!([click_at(choice), { "idle": 2 }]));
}

fn component(app: &mut EditorApp, id: &str, name: &str) -> Value {
    let got = app.call("entity_get", &json!({ "ids": [id], "components": [name] }));
    got["entities"][0]["components"][name].clone()
}

fn entries(app: &mut EditorApp) -> usize {
    app.call("edit_history", &json!({ "action": "list" }))["entries"]
        .as_array()
        .map_or(0, Vec::len)
}

fn select_ground(app: &mut EditorApp) {
    play(app, json!([{ "idle": 2 }]));
    app.select(&["ground"]);
    play(app, json!([{ "idle": 2 }]));
}

#[test]
fn every_engine_field_has_a_widget() {
    let (mut app, _dir) = app("widgets");
    let listed = app.call("schema_get", &json!({}));
    let names: Vec<Value> = listed["types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].clone())
        .collect();
    let full = app.call("schema_get", &json!({ "names": names, "detail": "full" }));
    let mut fallback = Vec::new();
    for t in full["types"].as_array().unwrap() {
        for (field, schema) in t["schema"]["properties"].as_object().unwrap() {
            if !has_widget(schema) {
                fallback.push(format!("{}.{field}", t["name"]));
            }
        }
    }
    assert!(
        fallback.is_empty(),
        "these fields fall back to raw JSON: {fallback:?}"
    );
}

#[test]
fn typing_a_number_is_one_call_by_a_human() {
    let (mut app, _dir) = app("type");
    select_ground(&mut app);
    let before = entries(&mut app);
    let metallic = field(&app, "metallic");
    type_into(&mut app, metallic, "0.75");
    assert_eq!(component(&mut app, "ground", "Material")["metallic"], 0.75);
    assert_eq!(entries(&mut app), before + 1);
    assert_eq!(app.status(), "set Material.metallic");
    let history = app.call("edit_history", &json!({ "action": "list" }));
    assert!(
        history["entries"][0].to_string().contains("human"),
        "{history}"
    );
}

#[test]
fn a_drag_shows_as_it_goes_and_is_sent_once_when_let_go() {
    let (mut app, _dir) = app("drag");
    select_ground(&mut app);
    let before = entries(&mut app);
    let metallic = field(&app, "metallic");
    let (x, y) = (metallic.center().x, metallic.center().y);
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": [x, y] }, { "press": "primary" }] },
            { "events": [{ "pointer": [x + 20.0, y] }] },
            { "events": [{ "pointer": [x + 40.0, y] }] },
            { "events": [{ "pointer": [x + 60.0, y] }] }
        ]),
    );
    // Still held: nothing sent yet.
    assert_eq!(entries(&mut app), before, "nothing is sent during a drag");
    assert_eq!(component(&mut app, "ground", "Material")["metallic"], 0.0);
    play(
        &mut app,
        json!([{ "events": [{ "release": "primary" }] }, { "idle": 2 }]),
    );
    let metallic = component(&mut app, "ground", "Material")["metallic"]
        .as_f64()
        .unwrap();
    assert!(metallic > 0.05, "dragged up to {metallic}");
    assert_eq!(
        entries(&mut app),
        before + 1,
        "one entry for the whole drag"
    );
}

#[test]
fn choices_check_boxes_and_adding_and_removing_components() {
    let (mut app, _dir) = app("choices");
    select_ground(&mut app);
    // A choice: Primitive.shape from Plane to Cube.
    let shape = field(&app, "shape");
    play(&mut app, json!([click_at(shape), { "idle": 1 }]));
    let cube = named(&app, "Cube");
    play(&mut app, json!([click_at(cube), { "idle": 2 }]));
    assert_eq!(component(&mut app, "ground", "Primitive")["shape"], "Cube");
    // Add a component from the list: it comes with its defaults.
    add_component(&mut app, "Collider");
    assert_eq!(component(&mut app, "ground", "Collider")["sensor"], false);
    assert_eq!(app.status(), "added Collider");
    // A check box.
    let sensor = field(&app, "sensor");
    play(&mut app, json!([click_at(sensor), { "idle": 2 }]));
    assert_eq!(component(&mut app, "ground", "Collider")["sensor"], true);
    // Remove: the "Remove component" button under the Collider header.
    let header = named(&app, "Collider");
    let remove = app
        .widgets()
        .iter()
        .filter(|w| w.label.as_deref() == Some("Remove component") && w.rect.min.y > header.max.y)
        .min_by(|a, b| a.rect.min.y.total_cmp(&b.rect.min.y))
        .map(|w| w.rect)
        .unwrap();
    play(&mut app, json!([click_at(remove), { "idle": 2 }]));
    assert!(component(&mut app, "ground", "Collider").is_null());
    assert_eq!(app.status(), "removed Collider");
}

#[test]
fn a_change_applies_to_every_selected_entity() {
    let (mut app, _dir) = app("multi");
    let added = app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "box", "components": {
            "Transform": {}, "Primitive": { "shape": "Cube" }, "Material": {} } }] }),
    );
    assert_eq!(added["ok"], true, "{added}");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    app.select(&["ground", "box"]);
    play(&mut app, json!([{ "idle": 2 }]));
    let before = entries(&mut app);
    let roughness = field(&app, "roughness");
    type_into(&mut app, roughness, "0.25");
    assert_eq!(component(&mut app, "ground", "Material")["roughness"], 0.25);
    assert_eq!(component(&mut app, "box", "Material")["roughness"], 0.25);
    assert_eq!(entries(&mut app), before + 1, "one change for both");
}

#[test]
fn a_rotation_is_edited_in_degrees() {
    let (mut app, _dir) = app("rotation");
    select_ground(&mut app);
    let yaw = named(&app, "y: 0.0 deg");
    type_into(&mut app, yaw, "90");
    let rotation = component(&mut app, "ground", "Transform")["rotation"].clone();
    // 90 degrees about Y: [0, sin 45, 0, cos 45].
    let y = rotation[1].as_f64().unwrap();
    let w = rotation[3].as_f64().unwrap();
    assert!(
        (y - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-4
            && (w - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-4,
        "{rotation}"
    );
}

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-editor");
        let _ = std::fs::remove_dir_all(&sdk);
        pack_sdk(&cargo, &manifest, &tmp.join("sdk-target"), &sdk).unwrap();
        let out = std::process::Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            .unwrap();
        let sysroot = PathBuf::from(String::from_utf8(out.stdout).unwrap().trim());
        let rustc = sysroot
            .join("bin")
            .join(if cfg!(windows) { "rustc.exe" } else { "rustc" });
        Toolchain { sdk, rustc }
    })
}

const SPIN: &str = r#"use wish_sdk::prelude::*;

/// Turns an entity around Y.
#[component]
pub struct Spin {
    /// How fast it turns.
    #[schema(unit = "deg/s", default = 90.0)]
    pub speed: f32,
    /// Whether it turns the other way.
    pub backwards: bool,
}

#[system]
fn spin(time: Res<Time>, mut spinners: Query<(&Spin, &mut Transform)>) {
    for (spin, at) in spinners.iter_mut() {
        let way = if spin.backwards { -1.0 } else { 1.0 };
        at.rotation = at.rotation * Quat::from_rotation_y(way * spin.speed.to_radians() * time.delta);
    }
}

wish_sdk::export! {
    components: [Spin],
    systems: [spin],
}
"#;

/// The exit criterion of M8a: a component defined in Wasm is edited in the
/// inspector with no editor code written for it.
#[test]
fn a_component_defined_in_game_code_is_edited_without_editor_code() {
    let (mut app, dir) = app("wasm");
    app.workspace().set_toolchain(toolchain().clone());
    select_ground(&mut app);
    // Game code that defines a component, built while the editor is open.
    std::fs::write(dir.join(SCRIPTS_ENTRY), SPIN).unwrap();
    let built = app.call("script_build", &json!({}));
    assert_eq!(built["passed"], true, "{built}");
    // The editor notices the new game code (it looks every 2 s).
    play(&mut app, json!([{ "seconds": 2.5 }, { "idle": 2 }]));
    add_component(&mut app, "Spin");
    assert_eq!(component(&mut app, "ground", "Spin")["speed"], 90.0);
    // Its fields have widgets: a number with its unit, a check box.
    let speed = field(&app, "speed");
    type_into(&mut app, speed, "180");
    let backwards = field(&app, "backwards");
    play(&mut app, json!([click_at(backwards), { "idle": 2 }]));
    let value = component(&mut app, "ground", "Spin");
    assert_eq!(value["speed"], 180.0, "{value}");
    assert_eq!(value["backwards"], true, "{value}");
    assert!(
        app.widgets().iter().any(|w| w
            .value
            .as_deref()
            .is_some_and(|v| v.starts_with("180") && v.ends_with(" deg/s"))),
        "the unit is shown"
    );
}

/// A part of a prefab instance (picked in the view, say) is edited as an
/// override of the instance; the prefab itself is not changed.
#[test]
fn a_part_of_a_prefab_instance_is_edited_as_an_override() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("editor-i-prefab");
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    std::fs::create_dir_all(dir.join("prefabs")).unwrap();
    let prefab = r#"{ "format": 1, "entities": {
        "root": { "components": { "Transform": {} } },
        "bulb": { "parent": "root", "components": {
            "Transform": { "translation": [0, 1, 0] },
            "Primitive": { "shape": "Sphere" }, "Material": {} } } } }"#;
    std::fs::write(dir.join("prefabs/lamp.prefab.json"), prefab).unwrap();
    // The scene gets an instance (written by hand, as a person would).
    let path = dir.join("scenes/main.scene.json");
    let mut scene: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    scene["entities"]["lamp_1"] = json!({ "prefab": "prefabs/lamp.prefab.json" });
    std::fs::write(&path, serde_json::to_string_pretty(&scene).unwrap()).unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    play(&mut app, json!([{ "idle": 2 }]));
    app.select(&["lamp_1/bulb"]);
    play(&mut app, json!([{ "idle": 2 }]));
    assert_eq!(app.selection(), ["lamp_1/bulb"]);
    assert!(
        app.widgets().iter().any(|w| w
            .value
            .as_deref()
            .is_some_and(|v| v.starts_with("Part of a prefab instance"))),
        "the inspector says it is a part"
    );
    // No add/remove for a part (an override changes values only).
    assert!(
        !app.widgets()
            .iter()
            .any(|w| w.label.as_deref() == Some("Remove component"))
    );
    let metallic = field(&app, "metallic");
    type_into(&mut app, metallic, "0.5");
    assert_eq!(
        component(&mut app, "lamp_1/bulb", "Material")["metallic"],
        0.5
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("\"overrides\""),
        "kept as an override: {text}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("prefabs/lamp.prefab.json")).unwrap(),
        prefab,
        "the prefab is not changed"
    );
    // It stays selected while its instance is there.
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    assert_eq!(app.selection(), ["lamp_1/bulb"]);
}
