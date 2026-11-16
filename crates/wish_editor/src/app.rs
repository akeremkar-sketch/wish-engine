//! The editor's shell: the window's layout, the view of the scene, the
//! hierarchy and the way input reaches them (M8a WP2, WP3).
//!
//! Everything the editor does to the project is a control-plane call by the
//! actor `human` ([`EditorApp::call`]): the panels never touch a scene file
//! or a world. What is shown comes from calls too (`project_info`,
//! `scene_get`, `edit_history`), so a change made by an agent or by editing
//! the file by hand shows up here the next time the editor looks. A change
//! the editor makes is one `edit_apply` (so one entry of the history, undone
//! in one step), sent with the revision it was made against: if somebody
//! changed the scene meanwhile the call is refused and the person is told,
//! instead of overwriting their work.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use egui::{Align, Layout, Pos2, Rect, Sense, Ui, pos2, vec2};
use glam::{Quat, Vec3};
use serde_json::{Value, json};
use wish_control::{PictureSource, Workspace, parse_actor};
use wish_render::{CameraView, RgbaImage, View};
use wish_window::{Button, FlyCamera, Modifiers, Movement, Picture, Shell, ShellEvent};

use crate::Editor;
use crate::browser::{Asset, AssetDrag, BrowserActions, BrowserState, BrowserView};
use crate::gizmo::{self, Drag, Eye, Handle, Mode, Space, Target};
use crate::inspector::{self, Inspected, InspectorActions, InspectorState, InspectorView};
use crate::raster::Canvas;

/// How often the scene is looked at for changes made elsewhere.
const POLL_SECONDS: f64 = 0.4;
/// How often the project as a whole is (problems, rebuilt game code).
const PROJECT_POLL_SECONDS: f64 = 2.0;
/// The game's tick rate while playing in the editor.
const TICK_RATE: f64 = 60.0;
/// At most this many ticks are run in one frame (a slow frame does not
/// make the game race to catch up).
const MAX_CATCH_UP: u64 = 8;
/// A snapshot for the timeline every this many seconds of game time.
const SNAPSHOT_SECONDS: f64 = 2.0;
/// At most this many timeline snapshots (a session holds 32).
const MAX_TIMELINE: usize = 30;

/// The game running in the editor.
struct Play {
    session: String,
    paused: bool,
    /// Ticks owed to real time.
    owed: f64,
    /// Keys and buttons held (by `input.json` name), sent to the game.
    held: BTreeSet<String>,
    /// Game time since the last timeline snapshot.
    since_snapshot: f64,
    /// The timeline: snapshot labels and their ticks.
    snapshots: Vec<(String, u64)>,
    /// Where the timeline slider is (an index into `snapshots`).
    cursor: usize,
    tick: u64,
    /// Real time since the inspector last read the live values.
    since_read: f64,
}

/// The game's name for a key, from egui's (the same, but arrows are short).
fn game_key(name: &str) -> &str {
    match name {
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        other => other,
    }
}
/// The window's backdrop where nothing is drawn.
const BACKDROP: [u8; 4] = [24, 24, 27, 255];
/// A viewport smaller than this is not drawn (a window being squeezed).
const MIN_VIEWPORT: f32 = 32.0;
/// The colour of the box around the selection in the view.
const OUTLINE: [u8; 4] = [255, 170, 40, 255];
/// A press and release this close together (pixels) is a click, not a drag.
const CLICK_SLOP: f32 = 4.0;

/// One entity of the hierarchy, flattened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The entity's id in the scene.
    pub id: String,
    /// How deep it is (0: a root).
    pub depth: usize,
    /// What components it has.
    pub components: Vec<String>,
    /// The id of its parent, if it has one.
    pub parent: Option<String>,
    /// The prefab it is an instance of, if it is one.
    pub prefab: Option<String>,
}

/// What the layout keeps between frames (text being typed).
#[derive(Default)]
struct UiState {
    /// The row being renamed and the name typed so far.
    rename: Option<(String, String)>,
    /// Where the timeline slider was put while the button is still down.
    timeline: Option<usize>,
}

/// The editor as a window's content.
pub struct EditorApp {
    /// Taken out while a frame is laid out (the layout reads the rest of
    /// the app).
    editor: Option<Editor>,
    workspace: Workspace,
    canvas: Canvas,
    project: String,
    scene: String,
    scenes: Vec<String>,
    revision: u64,
    rows: Vec<Row>,
    /// The selected entities, the first being the one the inspector shows.
    selected: Vec<String>,
    can_undo: bool,
    can_redo: bool,
    problems: (u64, u64),
    view: View,
    /// The fly camera, made from the scene's first camera on first use.
    eye: RefCell<Option<FlyCamera>>,
    movement: Movement,
    looking: bool,
    pointer: Option<(f32, f32)>,
    modifiers: Modifiers,
    /// Where a primary-button press began over the view.
    press: Option<(f32, f32)>,
    /// A click over the view waiting to pick: pixels, whether Ctrl was held.
    pending_pick: Option<(f32, f32, bool)>,
    /// Where the scene's picture goes, from the last layout.
    viewport: Rect,
    /// What the scene looked like last (kept while nothing changes).
    picture: Option<RgbaImage>,
    /// Boxes around the selected entities, in window pixels.
    outlines: Vec<Rect>,
    drawn_size: (u32, u32),
    picture_stale: bool,
    ui_state: RefCell<UiState>,
    /// The project's prefabs, and its files.
    prefabs: Vec<String>,
    files: Vec<Asset>,
    /// How many files `project_info` counted when `assets` was read.
    asset_count: u64,
    asset_selected: Option<String>,
    asset_detail: Option<Value>,
    browser_state: RefCell<BrowserState>,
    /// A component's values copied in the inspector.
    clipboard: Option<(String, Value)>,
    /// What the handles do, along which axes, and whether they snap.
    mode: Mode,
    space: Space,
    snap: bool,
    /// The camera as last drawn (where things are on the screen).
    last_camera: RefCell<Option<CameraView>>,
    /// The main selected entity's transform, for the handles.
    target: Option<(String, Target)>,
    target_for: (Vec<String>, u64),
    hovered: Option<Handle>,
    drag: Option<ActiveDrag>,
    /// The game, while it plays in the editor.
    play: Option<Play>,
    /// Component schemas from `schema_get`, by name.
    schemas: BTreeMap<String, Value>,
    /// The project's files, for asset fields.
    assets: Vec<String>,
    /// The selection as the inspector shows it, and what it was read for.
    inspected: Vec<Inspected>,
    inspected_for: (Vec<String>, u64),
    inspector_state: RefCell<InspectorState>,
    status: String,
    since_poll: f64,
    since_project_poll: f64,
    /// The game code's fingerprint when the schemas were read.
    scripts: String,
    frames: u64,
    exit: bool,
}

/// A handle being dragged: the drag, and what it has made so far.
struct ActiveDrag {
    drag: Drag,
    id: String,
    /// The transform when the drag began.
    start: Target,
    /// The transform now (shown in the preview, sent on release).
    now: Target,
    /// The change as edit operations, for the preview and the release.
    ops: Option<Value>,
}

/// What the layout decided, handed back out of egui's closure.
#[derive(Default)]
struct Actions {
    undo: bool,
    redo: bool,
    check: bool,
    reset_camera: bool,
    scene: Option<String>,
    view: Option<View>,
    quit: bool,
    /// An entity was clicked in the hierarchy: its id, and whether to toggle.
    select: Option<(String, bool)>,
    /// The empty part of the hierarchy was clicked.
    deselect: bool,
    /// Add an entity (under this parent, or at the root).
    add: Option<Option<String>>,
    delete: bool,
    /// Rename an entity: from, to.
    rename: Option<(String, String)>,
    /// Move an entity under a parent (`None`: to the root).
    reparent: Option<(String, Option<String>)>,
    /// What the inspector asked for.
    inspector: InspectorActions,
    mode: Option<Mode>,
    toggle_space: bool,
    toggle_snap: bool,
    /// What the browser asked for.
    browser: BrowserActions,
    /// A file was dropped on the view: its path, and where.
    drop: Option<(String, Pos2)>,
    /// Make a prefab of this entity and its children.
    make_prefab: Option<String>,
    play_start: bool,
    play_pause: bool,
    play_step: bool,
    play_stop: bool,
    /// Go back to this point of the timeline.
    rewind_to: Option<usize>,
}

impl Actions {
    /// Whether the layout asked for anything.
    fn any(&self) -> bool {
        self.undo
            || self.redo
            || self.check
            || self.reset_camera
            || self.quit
            || self.deselect
            || self.delete
            || self.scene.is_some()
            || self.view.is_some()
            || self.select.is_some()
            || self.add.is_some()
            || self.rename.is_some()
            || self.reparent.is_some()
            || self.inspector.any()
            || self.mode.is_some()
            || self.toggle_space
            || self.toggle_snap
            || self.browser.any()
            || self.drop.is_some()
            || self.make_prefab.is_some()
            || self.play_start
            || self.play_pause
            || self.play_step
            || self.play_stop
            || self.rewind_to.is_some()
    }
}

impl EditorApp {
    /// An editor over a project (the workspace is the project's one door).
    pub fn new(workspace: Workspace) -> Self {
        let mut app = Self {
            editor: Some(Editor::new()),
            workspace,
            canvas: Canvas::new(1, 1, BACKDROP),
            project: String::new(),
            scene: String::new(),
            scenes: Vec::new(),
            revision: u64::MAX,
            rows: Vec::new(),
            selected: Vec::new(),
            can_undo: false,
            can_redo: false,
            problems: (0, 0),
            view: View::Lit,
            eye: RefCell::new(None),
            movement: Movement::default(),
            looking: false,
            pointer: None,
            modifiers: Modifiers::default(),
            press: None,
            pending_pick: None,
            viewport: Rect::NOTHING,
            picture: None,
            outlines: Vec::new(),
            drawn_size: (0, 0),
            picture_stale: true,
            ui_state: RefCell::new(UiState::default()),
            prefabs: Vec::new(),
            files: Vec::new(),
            asset_count: u64::MAX,
            asset_selected: None,
            asset_detail: None,
            browser_state: RefCell::new(BrowserState::default()),
            clipboard: None,
            mode: Mode::Translate,
            space: Space::World,
            snap: false,
            last_camera: RefCell::new(None),
            target: None,
            target_for: (Vec::new(), u64::MAX),
            hovered: None,
            drag: None,
            play: None,
            schemas: BTreeMap::new(),
            assets: Vec::new(),
            inspected: Vec::new(),
            inspected_for: (Vec::new(), u64::MAX),
            inspector_state: RefCell::new(InspectorState::default()),
            status: String::new(),
            since_poll: POLL_SECONDS,
            since_project_poll: 0.0,
            scripts: String::new(),
            frames: 0,
            exit: false,
        };
        app.refresh_project();
        app.load_schemas();
        app
    }

    /// Reads the project's files and prefabs for the browser.
    fn load_assets(&mut self) {
        let listed = self.call("asset_list", &json!({}));
        let mut rows: Vec<Asset> = listed["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| {
                let path = a["path"].as_str()?.to_owned();
                Some(Asset {
                    kind: Asset::kind_of(&path),
                    bytes: a["bytes"].as_u64().unwrap_or(0),
                    path,
                })
            })
            .collect();
        rows.extend(self.prefabs.iter().map(|path| Asset {
            path: path.clone(),
            kind: "prefab",
            bytes: 0,
        }));
        rows.sort_by(|a, b| a.path.cmp(&b.path));
        self.files = rows;
        self.asset_detail = None;
        if let Some(path) = self.asset_selected.clone() {
            self.select_asset(&path);
        }
    }

    /// Selects a file in the browser and reads what is known about it.
    fn select_asset(&mut self, path: &str) {
        self.asset_selected = Some(path.to_owned());
        if path.ends_with(".prefab.json") {
            self.asset_detail = Some(json!({ "kind": "prefab" }));
            return;
        }
        let info = self.call("asset_info", &json!({ "path": path }));
        self.asset_detail = (info["ok"] == true).then_some(info);
    }

    /// Reads every component's schema (again after game code changed: a
    /// component defined in Wasm comes with it) and the project's files.
    fn load_schemas(&mut self) {
        let listed = self.call("schema_get", &json!({}));
        let names: Vec<String> = listed["types"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| t["kind"] == "component")
            .filter_map(|t| t["name"].as_str().map(str::to_owned))
            // Kept by the engine itself: not for hand editing.
            .filter(|name| !["GlobalTransform", "Hierarchy", "SceneId"].contains(&name.as_str()))
            .collect();
        let full = self.call("schema_get", &json!({ "names": names, "detail": "full" }));
        self.schemas = full["types"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| Some((t["name"].as_str()?.to_owned(), t["schema"].clone())))
            .collect();
        let assets = self.call("asset_list", &json!({}));
        self.assets = assets["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| a["path"].as_str().map(str::to_owned))
            .collect();
    }

    /// Reads the selected entities' components when the selection or the
    /// scene changed.
    fn refresh_inspected(&mut self) {
        let key = (self.selected.clone(), self.revision);
        if key == self.inspected_for {
            return;
        }
        self.inspected_for = key;
        self.inspected.clear();
        if self.selected.is_empty() {
            return;
        }
        let args = match &self.play {
            Some(play) => json!({ "session": play.session.clone(), "ids": self.selected.clone() }),
            None => json!({ "scene": self.scene.clone(), "ids": self.selected.clone() }),
        };
        let got = self.call("entity_get", &args);
        if got["ok"] != true {
            return;
        }
        for entity in got["entities"].as_array().into_iter().flatten() {
            self.inspected.push(Inspected {
                id: entity["id"].as_str().unwrap_or_default().to_owned(),
                components: entity["components"]
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
                prefab: entity["prefab"].as_str().map(str::to_owned),
                part: entity["from_prefab"] == true,
            });
        }
        // A component the schemas do not know yet: game code was rebuilt.
        let unknown = self
            .inspected
            .iter()
            .flat_map(|entity| entity.components.keys())
            .any(|name| !self.schemas.contains_key(name));
        if unknown {
            self.load_schemas();
        }
    }

    /// The camera as last drawn, over the picture's place on the window.
    fn eye_now(&self) -> Option<Eye> {
        let camera = (*self.last_camera.borrow())?;
        (self.viewport.width() >= MIN_VIEWPORT && self.viewport.height() >= MIN_VIEWPORT)
            .then(|| Eye::new(&camera, self.viewport))
    }

    /// Reads the main selected entity's transform and its parent's world
    /// matrix, for the handles (not for a part of a prefab instance: its
    /// handles would have to write overrides).
    fn refresh_target(&mut self) {
        let key = (self.selected.clone(), self.revision);
        if self.play.is_some() {
            // No handles on a running game.
            self.target = None;
            self.target_for = (Vec::new(), u64::MAX);
            return;
        }
        if key == self.target_for || self.drag.is_some() {
            return;
        }
        self.target_for = key;
        self.target = None;
        let Some(id) = self
            .selected
            .first()
            .filter(|id| !id.contains('/'))
            .cloned()
        else {
            return;
        };
        let got = self.call(
            "entity_get",
            &json!({ "scene": self.scene.clone(), "ids": [id.clone()],
                     "components": ["Transform", "GlobalTransform"] }),
        );
        let components = &got["entities"][0]["components"];
        let vector = |value: &Value, n: usize| -> Option<Vec<f32>> {
            let items = value.as_array()?;
            (items.len() == n).then(|| {
                items
                    .iter()
                    .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                    .collect()
            })
        };
        let (Some(t), Some(r), Some(s), Some(m)) = (
            vector(&components["Transform"]["translation"], 3),
            vector(&components["Transform"]["rotation"], 4),
            vector(&components["Transform"]["scale"], 3),
            vector(&components["GlobalTransform"]["matrix"], 16),
        ) else {
            return;
        };
        let local = glam::Mat4::from_scale_rotation_translation(
            Vec3::new(s[0], s[1], s[2]),
            Quat::from_xyzw(r[0], r[1], r[2], r[3]).normalize(),
            Vec3::new(t[0], t[1], t[2]),
        );
        let mut columns = [0.0f32; 16];
        columns.copy_from_slice(&m);
        let world = glam::Mat4::from_cols_array(&columns);
        let parent = world * local.inverse();
        self.target = Some((
            id,
            Target {
                translation: Vec3::new(t[0], t[1], t[2]),
                rotation: Quat::from_xyzw(r[0], r[1], r[2], r[3]).normalize(),
                scale: Vec3::new(s[0], s[1], s[2]),
                parent: if parent.is_finite() {
                    parent
                } else {
                    glam::Mat4::IDENTITY
                },
            },
        ));
    }

    /// The edit that makes the drag's transform: only what changed.
    fn drag_ops(id: &str, start: &Target, now: &Target) -> Value {
        let mut value = serde_json::Map::new();
        let list = |v: Vec<f32>| json!(v.iter().map(|x| f64::from(*x)).collect::<Vec<_>>());
        if now.translation != start.translation {
            value.insert(
                "translation".to_owned(),
                list(now.translation.to_array().to_vec()),
            );
        }
        if now.rotation != start.rotation {
            value.insert(
                "rotation".to_owned(),
                list(now.rotation.to_array().to_vec()),
            );
        }
        if now.scale != start.scale {
            value.insert("scale".to_owned(), list(now.scale.to_array().to_vec()));
        }
        json!([{ "op": "set", "id": id, "component": "Transform", "value": value }])
    }

    /// What the pointer does to the handles: which one it is over, taking
    /// hold, dragging, letting go, Esc. Returns whether the press was a
    /// handle's (and so no click on the view).
    fn gizmo_input(&mut self, event: &ShellEvent) -> bool {
        let Some(eye) = self.eye_now() else {
            return false;
        };
        let pointer = self.pointer.map(|(x, y)| pos2(x, y));
        match event {
            ShellEvent::Pointer { .. } => {
                if let Some(active) = self.drag.as_mut() {
                    if let Some(pointer) = pointer
                        && let Some(now) = active.drag.transform(&eye, pointer, self.snap)
                    {
                        active.ops = Some(Self::drag_ops(&active.id, &active.start, &now));
                        active.now = now;
                        self.picture_stale = true;
                    }
                } else {
                    let hovered = self.hover(&eye, pointer);
                    if hovered != self.hovered {
                        self.hovered = hovered;
                        self.picture_stale = true;
                    }
                }
            }
            ShellEvent::Button {
                button: Button::Primary,
                pressed: true,
                ..
            } if self.drag.is_none() => {
                if let (Some(handle), Some(pointer), Some((id, target))) =
                    (self.hovered, pointer, self.target.clone())
                    && let Some(drag) =
                        Drag::begin(&eye, self.mode, handle, self.space, target, pointer)
                {
                    self.drag = Some(ActiveDrag {
                        drag,
                        id,
                        start: target,
                        now: target,
                        ops: None,
                    });
                    return true;
                }
            }
            ShellEvent::Button {
                button: Button::Primary,
                pressed: false,
                ..
            } => {
                if let Some(active) = self.drag.take() {
                    self.picture_stale = true;
                    if let Some(ops) = active.ops {
                        let what = match self.mode {
                            Mode::Translate => format!("moved {}", active.id),
                            Mode::Rotate => format!("rotated {}", active.id),
                            Mode::Scale => format!("scaled {}", active.id),
                        };
                        let Value::Array(ops) = ops else {
                            return true;
                        };
                        self.edit(&what, ops);
                    }
                    return true;
                }
            }
            ShellEvent::Key {
                name,
                pressed: true,
                ..
            } if name == "Escape" && self.drag.take().is_some() => {
                self.status = "cancelled".to_owned();
                self.picture_stale = true;
            }
            _ => {}
        }
        false
    }

    /// The handle under the pointer (only over the view and with a target).
    fn hover(&self, eye: &Eye, pointer: Option<Pos2>) -> Option<Handle> {
        let pointer = pointer.filter(|p| self.viewport.contains(*p))?;
        let (_, target) = self.target.as_ref()?;
        let axes = gizmo::axes(target.world_rotation(), self.space);
        gizmo::hit_test(eye, self.mode, axes, target.pivot(), pointer)
    }

    /// The inspector's changes, as one `edit_apply` on every selected entity
    /// (a part of a prefab instance gets an override).
    fn inspector_act(&mut self, asked: InspectorActions) {
        let targets: Vec<(String, bool)> = self
            .inspected
            .iter()
            .map(|entity| (entity.id.clone(), entity.part))
            .collect();
        let op_for = |id: &str, part: bool, component: &str, value: Value| -> Value {
            match (part, id.split_once('/')) {
                (true, Some((instance, inner))) => json!({
                    "op": "set_override", "id": instance, "entity": inner,
                    "component": component, "value": value
                }),
                _ => json!({ "op": "set", "id": id, "component": component, "value": value }),
            }
        };
        if let Some((component, field, value)) = asked.set {
            let ops = targets
                .iter()
                .map(|(id, part)| {
                    op_for(
                        id,
                        *part,
                        &component,
                        json!({ field.clone(): value.clone() }),
                    )
                })
                .collect();
            self.edit(&format!("set {component}.{field}"), ops);
        }
        if let Some(component) = asked.copy {
            if let Some(value) = self
                .inspected
                .first()
                .and_then(|entity| entity.components.get(&component))
            {
                self.clipboard = Some((component.clone(), value.clone()));
                self.status = format!("copied {component}");
            }
            self.inspected_for = (Vec::new(), u64::MAX);
            return;
        }
        if let Some(component) = asked.paste {
            if let Some((name, value)) = self
                .clipboard
                .clone()
                .filter(|(name, _)| *name == component)
            {
                let ops = targets
                    .iter()
                    .map(|(id, part)| op_for(id, *part, &name, value.clone()))
                    .collect();
                self.edit(&format!("pasted {name}"), ops);
            }
            self.inspected_for = (Vec::new(), u64::MAX);
            return;
        }
        if let Some(component) = asked.reset {
            let ops = targets
                .iter()
                .map(|(id, part)| match (part, id.split_once('/')) {
                    (true, Some((instance, inner))) => json!({
                        "op": "replace_override", "id": instance, "entity": inner,
                        "component": component, "value": {}
                    }),
                    _ => json!({ "op": "replace", "id": id, "component": component, "value": {} }),
                })
                .collect();
            self.edit(&format!("reset {component}"), ops);
            self.inspected_for = (Vec::new(), u64::MAX);
            return;
        }
        if let Some(component) = asked.add {
            let ops = targets
                .iter()
                .map(|(id, part)| op_for(id, *part, &component, json!({})))
                .collect();
            self.edit(&format!("added {component}"), ops);
        }
        if let Some(component) = asked.remove {
            let ops = targets
                .iter()
                .map(|(id, _)| json!({ "op": "remove", "id": id, "component": component }))
                .collect();
            self.edit(&format!("removed {component}"), ops);
        }
        self.inspected_for = (Vec::new(), u64::MAX);
    }

    /// One control-plane call as the person at the keyboard (actor `human`).
    pub fn call(&mut self, tool: &str, args: &Value) -> Value {
        self.workspace.call(tool, args, &parse_actor("human"))
    }

    /// The project's workspace (to read what the editor does to it).
    pub fn workspace(&mut self) -> &mut Workspace {
        &mut self.workspace
    }

    /// The hierarchy as last read, flattened.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The selected entities (ids), the first being the main one.
    pub fn selection(&self) -> &[String] {
        &self.selected
    }

    /// Selects entities (as a click in the hierarchy would; unknown ids are
    /// dropped).
    pub fn select(&mut self, ids: &[&str]) {
        let known = |id: &str| {
            self.rows.iter().any(|row| row.id == id)
                || id.split_once('/').is_some_and(|(instance, _)| {
                    self.rows
                        .iter()
                        .any(|row| row.id == instance && row.prefab.is_some())
                })
        };
        self.selected = ids
            .iter()
            .filter(|id| known(id))
            .map(|id| (*id).to_owned())
            .collect();
        self.picture_stale = true;
    }

    /// The scene being edited (project-relative).
    pub fn scene(&self) -> &str {
        &self.scene
    }

    /// The last line of the status bar.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Frames drawn so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Where the scene's picture is on the window (pixels), from the last
    /// frame.
    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    /// The widgets on screen in the last frame (from the accessibility
    /// tree): role, label, value and place.
    pub fn widgets(&self) -> &[crate::Widget] {
        self.editor.as_ref().map_or(&[], |editor| editor.widgets())
    }

    /// Whether the game is running in the editor (paused or not).
    pub fn playing(&self) -> bool {
        self.play.is_some()
    }

    /// The running game's session and tick, and whether it is paused.
    pub fn play_state(&self) -> Option<(&str, u64, bool)> {
        self.play
            .as_ref()
            .map(|play| (play.session.as_str(), play.tick, play.paused))
    }

    /// The timeline: snapshot labels and ticks.
    pub fn timeline(&self) -> Vec<(String, u64)> {
        self.play
            .as_ref()
            .map(|play| play.snapshots.clone())
            .unwrap_or_default()
    }

    /// What the handles do now.
    pub fn handle_mode(&self) -> &'static str {
        match self.mode {
            Mode::Translate => "move",
            Mode::Rotate => "rotate",
            Mode::Scale => "scale",
        }
    }

    /// Whether a handle is being dragged.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Where the selected entity's handles are on the window: the pivot and
    /// the ends of the three axes (`None` without a selection or a camera).
    pub fn handle_places(&self) -> Option<[Pos2; 4]> {
        let eye = self.eye_now()?;
        let (_, target) = self.target.as_ref()?;
        let current = self.drag.as_ref().map_or(*target, |d| d.now);
        let pivot = current.pivot();
        let length = eye.handle_length(pivot);
        let axes = gizmo::axes(current.world_rotation(), self.space);
        Some([
            eye.project(pivot)?,
            eye.project(pivot + axes[0] * length)?,
            eye.project(pivot + axes[1] * length)?,
            eye.project(pivot + axes[2] * length)?,
        ])
    }

    /// The boxes drawn around the selection in the view (window pixels).
    pub fn outlines(&self) -> &[Rect] {
        &self.outlines
    }

    /// Reads what the title and lists need: the project, its scenes, the
    /// problems.
    fn refresh_project(&mut self) -> bool {
        let shown_before = (
            self.problems,
            self.scenes.clone(),
            self.prefabs.clone(),
            self.asset_count,
            self.scripts.clone(),
        );
        self.read_project();
        let shown_after = (
            self.problems,
            self.scenes.clone(),
            self.prefabs.clone(),
            self.asset_count,
            self.scripts.clone(),
        );
        shown_before != shown_after
    }

    /// The reading part of [`EditorApp::refresh_project`].
    fn read_project(&mut self) {
        let info = self.call("project_info", &json!({}));
        if info["ok"] != true {
            self.status = info["error"]["message"]
                .as_str()
                .unwrap_or("the project could not be read")
                .to_owned();
            return;
        }
        self.project = info["project"]["name"].as_str().unwrap_or("").to_owned();
        self.scenes = info["scenes"]
            .as_array()
            .map(|all| {
                all.iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        if self.scene.is_empty() {
            self.scene = info["project"]["scene"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| self.scenes.first().cloned())
                .unwrap_or_default();
        }
        self.problems = (
            info["problems"]["errors"].as_u64().unwrap_or(0),
            info["problems"]["warnings"].as_u64().unwrap_or(0),
        );
        let prefabs: Vec<String> = info["prefabs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str().map(str::to_owned))
            .collect();
        let count = info["assets"].as_u64().unwrap_or(0);
        if prefabs != self.prefabs || count != self.asset_count {
            self.prefabs = prefabs;
            self.asset_count = count;
            self.load_assets();
        }
        // Game code rebuilt (by the person, an agent, `wen build`): its
        // components come and go with it.
        let scripts = info["scripts"]["module"]["fingerprint"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if scripts != self.scripts {
            let first = self.scripts.is_empty() && self.schemas.is_empty();
            self.scripts = scripts;
            if !first {
                self.load_schemas();
                self.inspected_for = (Vec::new(), u64::MAX);
            }
        }
    }

    /// Reads the scene's hierarchy and history; returns whether it changed
    /// since the last time (by someone, anyone: the editor, an agent, a
    /// text editor).
    fn refresh_scene(&mut self) -> bool {
        let scene = self.scene.clone();
        let got = self.call("scene_get", &json!({ "scene": scene, "detail": "summary" }));
        if got["ok"] != true {
            let message = got["error"]["message"]
                .as_str()
                .unwrap_or("the scene could not be read");
            let hint = got["error"]["hint"].as_str().unwrap_or("");
            self.status = format!("{message} {hint}");
            return false;
        }
        let revision = got["revision"].as_u64().unwrap_or(0);
        if revision == self.revision {
            return false;
        }
        self.revision = revision;
        self.rows.clear();
        fn walk(nodes: &Value, depth: usize, parent: Option<&str>, rows: &mut Vec<Row>) {
            for node in nodes.as_array().into_iter().flatten() {
                let id = node["id"].as_str().unwrap_or("").to_owned();
                rows.push(Row {
                    id: id.clone(),
                    depth,
                    components: node["components"]
                        .as_array()
                        .map(|all| {
                            all.iter()
                                .filter_map(|c| c.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                    parent: parent.map(str::to_owned),
                    prefab: node["prefab"].as_str().map(str::to_owned),
                });
                walk(&node["children"], depth + 1, Some(&id), rows);
            }
        }
        walk(&got["tree"], 0, None, &mut self.rows);
        // What was selected may be gone now.
        // (A part of a prefab instance, picked in the view, stays while its
        // instance is there.)
        let known: BTreeSet<&str> = self.rows.iter().map(|row| row.id.as_str()).collect();
        let instances: BTreeSet<&str> = self
            .rows
            .iter()
            .filter(|row| row.prefab.is_some())
            .map(|row| row.id.as_str())
            .collect();
        self.selected.retain(|id| {
            known.contains(id.as_str())
                || id
                    .split_once('/')
                    .is_some_and(|(instance, _)| instances.contains(instance))
        });
        let history = self.call(
            "edit_history",
            &json!({ "scene": self.scene.clone(), "action": "list" }),
        );
        self.can_undo = history["can_undo"] == true;
        self.can_redo = history["can_redo"] == true;
        self.picture_stale = true;
        true
    }

    /// One change to the scene: `ops` as one `edit_apply`, made against the
    /// revision the person is looking at. Returns whether it was applied.
    fn edit(&mut self, what: &str, ops: Vec<Value>) -> bool {
        // While the game plays, a change is made to the running game (live,
        // not kept), never to the file.
        let mut args = match &self.play {
            Some(play) => json!({ "session": play.session.clone(), "ops": ops }),
            None => json!({ "scene": self.scene.clone(), "ops": ops }),
        };
        if self.play.is_none() && self.revision != u64::MAX {
            args["base_revision"] = json!(self.revision);
        }
        let done = self.call("edit_apply", &args);
        let applied = done["ok"] == true;
        self.status = if applied && self.play.is_some() {
            format!("{what} (live: not kept when the game stops)")
        } else if applied {
            what.to_owned()
        } else {
            let message = done["error"]["message"]
                .as_str()
                .unwrap_or("it did not work");
            let hint = done["error"]["hint"].as_str().unwrap_or("");
            format!("{what} failed: {message} {hint}")
        };
        // Look again either way: a refusal means the scene is not what the
        // person saw.
        self.revision = u64::MAX;
        self.refresh_scene();
        self.inspected_for = (Vec::new(), u64::MAX);
        self.picture_stale = true;
        applied
    }

    /// Starts the game from the scene being edited.
    fn start_play(&mut self) {
        if self.scene.ends_with(".prefab.json") {
            self.status = "a prefab cannot be played; open a scene".to_owned();
            return;
        }
        let started = self.call("sim_start", &json!({ "scene": self.scene.clone() }));
        let Some(session) = started["session"].as_str().map(str::to_owned) else {
            let message = started["error"]["message"]
                .as_str()
                .unwrap_or("it did not start");
            let hint = started["error"]["hint"].as_str().unwrap_or("");
            self.status = format!("play failed: {message} {hint}");
            return;
        };
        self.drag = None;
        self.play = Some(Play {
            session,
            paused: false,
            owed: 0.0,
            held: BTreeSet::new(),
            since_snapshot: 0.0,
            snapshots: Vec::new(),
            cursor: 0,
            tick: 0,
            since_read: 0.0,
        });
        self.snapshot();
        self.status = "playing".to_owned();
        self.inspected_for = (Vec::new(), u64::MAX);
        self.picture_stale = true;
    }

    /// Stops the game: the session is closed and the scene is shown again
    /// (live changes are gone with it).
    fn stop_play(&mut self) {
        if let Some(play) = self.play.take() {
            self.call("sim_stop", &json!({ "session": play.session }));
            self.status = "stopped (live changes were not kept)".to_owned();
            self.inspected_for = (Vec::new(), u64::MAX);
            self.target_for = (Vec::new(), u64::MAX);
            self.picture_stale = true;
        }
    }

    /// Saves a timeline snapshot of the running game (while there is room).
    fn snapshot(&mut self) {
        let Some(play) = self.play.as_ref() else {
            return;
        };
        if play.snapshots.len() >= MAX_TIMELINE {
            return;
        }
        let label = format!("t{}", play.tick);
        let session = play.session.clone();
        let saved = self.call(
            "sim_snapshot",
            &json!({ "session": session, "action": "save", "label": label.clone() }),
        );
        if let Some(play) = self.play.as_mut()
            && saved["ok"] == true
        {
            play.snapshots.push((label, play.tick));
            play.cursor = play.snapshots.len() - 1;
        }
    }

    /// Lets `ticks` of game time pass with the held keys and the pointer.
    fn run_ticks(&mut self, ticks: u64) {
        let Some(play) = self.play.as_ref() else {
            return;
        };
        let session = play.session.clone();
        let held = play.held.clone();
        let pointer = self
            .pointer
            .filter(|(x, y)| self.viewport.contains(pos2(*x, *y)))
            .map(|(x, y)| wish_control::PointerAt {
                x: x - self.viewport.min.x,
                y: y - self.viewport.min.y,
                width: self.viewport.width(),
                height: self.viewport.height(),
            });
        match self.workspace.play_step(&session, ticks, &held, pointer) {
            Ok(problems) => {
                if let Some(problem) = problems.first() {
                    self.status = format!("{problem}");
                }
            }
            Err(problem) => {
                self.status = format!("{problem}");
                if let Some(play) = self.play.as_mut() {
                    play.paused = true;
                }
            }
        }
        let tick = self.call(
            "sim_snapshot",
            &json!({ "session": session, "action": "list" }),
        )["tick"]
            .as_u64();
        if let Some(play) = self.play.as_mut() {
            play.tick = tick.unwrap_or(play.tick + ticks);
            play.since_snapshot += ticks as f64 / TICK_RATE;
        }
        self.picture_stale = true;
    }

    /// One frame of the running game: real time becomes ticks.
    fn tick_play(&mut self, seconds: f64) {
        let Some(play) = self.play.as_mut() else {
            return;
        };
        play.since_read += seconds;
        if play.since_read >= 0.25 {
            play.since_read = 0.0;
            self.inspected_for = (Vec::new(), u64::MAX);
        }
        let Some(play) = self.play.as_mut() else {
            return;
        };
        if play.paused {
            return;
        }
        play.owed += seconds * TICK_RATE;
        let whole = play.owed.floor();
        play.owed -= whole;
        let ticks = (whole as u64).min(MAX_CATCH_UP);
        if ticks == 0 {
            return;
        }
        self.run_ticks(ticks);
        if self
            .play
            .as_ref()
            .is_some_and(|p| p.since_snapshot >= SNAPSHOT_SECONDS)
        {
            if let Some(play) = self.play.as_mut() {
                play.since_snapshot = 0.0;
            }
            self.snapshot();
        }
    }

    /// Goes back to a point of the timeline; the game is paused there.
    fn rewind(&mut self, index: usize) {
        let Some(play) = self.play.as_ref() else {
            return;
        };
        let Some((label, tick)) = play.snapshots.get(index).cloned() else {
            return;
        };
        let restored = self.call(
            "sim_restore",
            &json!({ "session": play.session.clone(), "snapshot": label.clone() }),
        );
        if restored["ok"] == true {
            if let Some(play) = self.play.as_mut() {
                play.paused = true;
                play.tick = tick;
                play.cursor = index;
                play.owed = 0.0;
                play.since_snapshot = 0.0;
            }
            self.status = format!("back at tick {tick}");
        } else {
            let message = restored["error"]["message"]
                .as_str()
                .unwrap_or("it did not work");
            self.status = format!("rewind failed: {message}");
        }
        self.inspected_for = (Vec::new(), u64::MAX);
        self.picture_stale = true;
    }

    /// A scene id made from a file's name.
    fn id_from_path(&self, path: &str) -> String {
        let name = path.rsplit('/').next().unwrap_or(path);
        let stem = name.split('.').next().unwrap_or(name);
        let clean: String = stem
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.unused_id(if clean.is_empty() { "entity" } else { &clean })
    }

    /// Where a window pixel is on the ground (y = 0), or six metres along
    /// the pointer's ray when it does not meet the ground.
    fn ground_point(&self, at: Pos2) -> Vec3 {
        let Some(eye) = self.eye_now() else {
            return Vec3::ZERO;
        };
        let (origin, direction) = eye.ray(at);
        if direction.y < -1e-4 {
            let t = -origin.y / direction.y;
            if t > 0.0 && t < 500.0 {
                return origin + direction * t;
            }
        }
        origin + direction * 6.0
    }

    /// Puts a prefab or a model in the scene: a prefab becomes an instance,
    /// a model an entity with a `Model`; where the pointer dropped it, or
    /// in the middle of the view.
    fn place_asset(&mut self, path: &str, at: Option<Pos2>) {
        let place = self.ground_point(at.unwrap_or_else(|| self.viewport.center()));
        let translation = json!([place.x, place.y, place.z]);
        let id = self.id_from_path(path);
        let op = match Asset::kind_of(path) {
            "prefab" => json!({
                "op": "instantiate", "id": id, "prefab": path,
                "components": { "Transform": { "translation": translation } }
            }),
            "model" => json!({
                "op": "spawn", "id": id,
                "components": { "Transform": { "translation": translation }, "Model": { "asset": path } }
            }),
            _ => {
                self.status = format!("{path} cannot be put in a scene (a prefab or a model can)");
                return;
            }
        };
        if self.scene.ends_with(".prefab.json") && Asset::kind_of(path) == "prefab" {
            self.status = "a prefab cannot hold another prefab".to_owned();
            return;
        }
        if self.edit(&format!("added {id} ({path})"), vec![op]) {
            self.selected = vec![id];
        }
    }

    /// Makes a prefab of an entity and its children (in `prefabs/`), and puts
    /// an instance of it where they were. Two changes, in two files: the new
    /// prefab (its own history) and the scene (undoable).
    fn make_prefab(&mut self, id: &str) {
        let scene = self.scene.clone();
        let got = self.call(
            "scene_get",
            &json!({ "scene": scene, "detail": "normal", "root": id }),
        );
        let Some(node) = got["tree"].get(0).filter(|_| got["ok"] == true) else {
            self.status = "make prefab failed: the entity could not be read".to_owned();
            return;
        };
        let transform = node["components"].get("Transform").cloned();
        let mut ops = Vec::new();
        fn collect(node: &Value, parent: Option<&str>, root: bool, ops: &mut Vec<Value>) -> bool {
            if node.get("prefab").is_some() {
                return false;
            }
            let own = node["id"].as_str().unwrap_or_default();
            let name = if root { "root" } else { own };
            let mut components = node["components"].clone();
            if root {
                // The instance gets the place; the prefab's own is neutral.
                components["Transform"] = json!({});
            }
            let mut op = json!({ "op": "spawn", "id": name, "components": components });
            if let Some(parent) = parent {
                op["parent"] = json!(parent);
            }
            ops.push(op);
            for child in node["children"].as_array().into_iter().flatten() {
                if !collect(child, Some(name), false, ops) {
                    return false;
                }
            }
            true
        }
        if !collect(node, None, true, &mut ops) {
            self.status = "make prefab failed: it already contains prefab instances".to_owned();
            return;
        }
        let base = id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let path = (0..)
            .map(|n| {
                if n == 0 {
                    format!("prefabs/{base}.prefab.json")
                } else {
                    format!("prefabs/{base}_{n}.prefab.json")
                }
            })
            .find(|path| !self.prefabs.contains(path))
            .unwrap_or_default();
        let made = self.call(
            "edit_apply",
            &json!({ "scene": path.clone(), "create": true, "ops": ops }),
        );
        if made["ok"] != true {
            let message = made["error"]["message"]
                .as_str()
                .unwrap_or("it did not work");
            let hint = made["error"]["hint"].as_str().unwrap_or("");
            self.status = format!("make prefab failed: {message} {hint}");
            return;
        }
        let parent = self
            .rows
            .iter()
            .find(|row| row.id == id)
            .and_then(|row| row.parent.clone());
        let mut instance = json!({ "op": "instantiate", "id": id, "prefab": path.clone() });
        if let Some(parent) = parent {
            instance["parent"] = json!(parent);
        }
        if let Some(transform) = transform {
            instance["components"] = json!({ "Transform": transform });
        }
        let swapped = self.edit(
            &format!("made prefab {path}"),
            vec![json!({ "op": "despawn", "id": id }), instance],
        );
        self.asset_count = u64::MAX;
        self.refresh_project();
        if swapped {
            self.selected = vec![id.to_owned()];
        }
    }

    /// An id that no entity has, built on `base`.
    fn unused_id(&self, base: &str) -> String {
        let taken: BTreeSet<&str> = self.rows.iter().map(|row| row.id.as_str()).collect();
        (1..)
            .map(|n| format!("{base}_{n}"))
            .find(|id| !taken.contains(id.as_str()))
            .unwrap_or_else(|| base.to_owned())
    }

    /// Runs what the layout asked for, as calls.
    fn act(&mut self, actions: Actions) {
        if actions.quit {
            self.exit = true;
        }
        if let Some(scene) = actions.scene
            && scene != self.scene
        {
            self.scene = scene;
            self.revision = u64::MAX;
            self.selected.clear();
            *self.eye.borrow_mut() = None;
            self.refresh_scene();
        }
        if let Some(view) = actions.view {
            self.view = view;
            self.picture_stale = true;
        }
        if actions.reset_camera {
            *self.eye.borrow_mut() = None;
            self.picture_stale = true;
        }
        if let Some(path) = actions.browser.select {
            self.select_asset(&path);
        }
        if let Some(path) = actions.browser.place {
            self.place_asset(&path, None);
        }
        if let Some((path, at)) = actions.drop {
            self.place_asset(&path, Some(at));
        }
        if let Some((from, to)) = actions.browser.rename
            && from != to
            && !to.is_empty()
        {
            let moved = self.call(
                "asset_move",
                &json!({ "from": from.clone(), "to": to.clone() }),
            );
            self.status = if moved["ok"] == true {
                let updated = moved["updated"].as_array().map_or(0, Vec::len);
                format!("moved {from} to {to} ({updated} file(s) that use it updated)")
            } else {
                let message = moved["error"]["message"]
                    .as_str()
                    .unwrap_or("it did not work");
                let hint = moved["error"]["hint"].as_str().unwrap_or("");
                format!("move failed: {message} {hint}")
            };
            if self.asset_selected.as_deref() == Some(from.as_str()) {
                self.asset_selected = Some(to);
            }
            self.asset_count = u64::MAX;
            self.revision = u64::MAX;
            self.refresh_project();
            self.refresh_scene();
        }
        if let Some(id) = actions.make_prefab {
            self.make_prefab(&id);
        }
        if actions.play_start && self.play.is_none() {
            self.start_play();
        }
        if actions.play_pause
            && let Some(play) = self.play.as_mut()
        {
            play.paused = !play.paused;
            play.owed = 0.0;
            self.status = if play.paused { "paused" } else { "playing" }.to_owned();
        }
        if actions.play_step && self.play.as_ref().is_some_and(|p| p.paused) {
            self.run_ticks(1);
            self.inspected_for = (Vec::new(), u64::MAX);
        }
        if let Some(index) = actions.rewind_to {
            self.rewind(index);
        }
        if actions.play_stop {
            self.stop_play();
        }
        if let Some(mode) = actions.mode {
            self.mode = mode;
            self.hovered = None;
            self.picture_stale = true;
        }
        if actions.toggle_space {
            self.space = match self.space {
                Space::World => Space::Local,
                Space::Local => Space::World,
            };
            self.picture_stale = true;
        }
        if actions.toggle_snap {
            self.snap = !self.snap;
        }
        if let Some((id, toggle)) = actions.select {
            self.click_entity(&id, toggle);
        }
        if actions.deselect {
            self.selected.clear();
            self.picture_stale = true;
        }
        if let Some(parent) = actions.add {
            let id = self.unused_id("entity");
            let mut op = json!({ "op": "spawn", "id": id, "components": { "Transform": {} } });
            if let Some(parent) = parent {
                op["parent"] = json!(parent);
            }
            if self.edit(&format!("added {id}"), vec![op]) {
                self.selected = vec![id];
            }
        }
        if actions.delete && !self.selected.is_empty() {
            let doomed = self.selected.clone();
            let ops = doomed
                .iter()
                .map(|id| json!({ "op": "despawn", "id": id }))
                .collect();
            let what = if doomed.len() == 1 {
                format!("deleted {}", doomed[0])
            } else {
                format!("deleted {} entities", doomed.len())
            };
            if self.edit(&what, ops) {
                self.selected.clear();
            }
        }
        if let Some((from, to)) = actions.rename
            && from != to
            && !to.is_empty()
        {
            // The refresh after an edit drops ids that no longer exist, so the
            // renamed entity keeps its place in the selection by hand.
            let place = self.selected.iter().position(|id| *id == from);
            if self.edit(
                &format!("renamed {from} to {to}"),
                vec![json!({ "op": "rename", "id": from, "to": to })],
            ) && let Some(place) = place
            {
                self.selected.insert(place.min(self.selected.len()), to);
                self.picture_stale = true;
            }
        }
        if actions.inspector.any() {
            self.inspector_act(actions.inspector);
        }
        if let Some((id, parent)) = actions.reparent {
            let what = match &parent {
                Some(parent) => format!("moved {id} under {parent}"),
                None => format!("moved {id} to the root"),
            };
            self.edit(
                &what,
                vec![json!({ "op": "reparent", "id": id, "parent": parent })],
            );
        }
        for (wanted, action) in [(actions.undo, "undo"), (actions.redo, "redo")] {
            if wanted {
                let done = self.call(
                    "edit_history",
                    &json!({ "scene": self.scene.clone(), "action": action }),
                );
                self.status = if done["ok"] == true {
                    format!("{action}: done")
                } else {
                    done["error"]["message"]
                        .as_str()
                        .unwrap_or("it did not work")
                        .to_owned()
                };
                self.revision = u64::MAX;
                self.refresh_scene();
            }
        }
        if actions.check {
            let checked = self.call("check", &json!({}));
            let found = checked["diagnostics"].as_array().map_or(0, Vec::len);
            self.status = format!(
                "check: {}",
                if checked["passed"] == true {
                    "no problems".to_owned()
                } else {
                    format!("{found} problem(s)")
                }
            );
            self.refresh_project();
        }
    }

    /// A click on an entity (in the hierarchy or the view): selects it, or
    /// with Ctrl adds it to or takes it out of the selection.
    fn click_entity(&mut self, id: &str, toggle: bool) {
        if toggle {
            if let Some(at) = self.selected.iter().position(|s| s == id) {
                self.selected.remove(at);
            } else {
                self.selected.push(id.to_owned());
            }
        } else {
            self.selected = vec![id.to_owned()];
        }
        self.picture_stale = true;
    }

    /// What the plain input events mean for the view: looking around, flying,
    /// clicking to pick.
    fn view_input(&mut self, events: &[ShellEvent]) {
        for event in events {
            let over_view = |app: &Self| {
                app.pointer
                    .is_some_and(|(x, y)| app.viewport.contains(pos2(x, y)))
            };
            match event {
                ShellEvent::Pointer { x, y } => self.pointer = Some((*x, *y)),
                ShellEvent::PointerLeft => self.pointer = None,
                _ => {}
            }
            // While the game plays, the view's keys and buttons are the game's.
            if self.play.is_some() {
                let over = self
                    .pointer
                    .is_some_and(|(x, y)| self.viewport.contains(pos2(x, y)));
                let typing = self
                    .editor
                    .as_ref()
                    .is_some_and(|e| e.context().egui_wants_keyboard_input());
                if let Some(play) = self.play.as_mut() {
                    match event {
                        ShellEvent::Key { name, pressed, .. } if !typing => {
                            let key = game_key(name).to_owned();
                            if *pressed && over {
                                play.held.insert(key);
                            } else if !*pressed {
                                play.held.remove(&key);
                            }
                        }
                        ShellEvent::Modifiers(modifiers) => {
                            for (on, name) in [
                                (modifiers.shift, "Shift"),
                                (modifiers.ctrl, "Ctrl"),
                                (modifiers.alt, "Alt"),
                            ] {
                                if on && over {
                                    play.held.insert(name.to_owned());
                                } else {
                                    play.held.remove(name);
                                }
                            }
                        }
                        ShellEvent::Button {
                            button, pressed, ..
                        } => {
                            let name = match button {
                                Button::Primary => "MouseLeft",
                                Button::Secondary => "MouseRight",
                                Button::Middle => "MouseMiddle",
                            };
                            if *pressed && over {
                                play.held.insert(name.to_owned());
                            } else if !*pressed {
                                play.held.remove(name);
                            }
                        }
                        _ => {}
                    }
                }
                continue;
            }
            // The handles first: a press on one is not a click on the view.
            let took = self.gizmo_input(event);
            match event {
                ShellEvent::Button {
                    button: Button::Secondary,
                    pressed,
                    ..
                } => self.looking = *pressed && over_view(self),
                ShellEvent::Button {
                    button: Button::Primary,
                    pressed,
                    modifiers,
                } => {
                    if took {
                        self.press = None;
                    } else if *pressed {
                        self.press = self.pointer.filter(|_| over_view(self));
                    } else if let (Some(start), Some(end)) = (self.press.take(), self.pointer)
                        && (start.0 - end.0).abs() < CLICK_SLOP
                        && (start.1 - end.1).abs() < CLICK_SLOP
                        && over_view(self)
                    {
                        self.pending_pick = Some((end.0, end.1, modifiers.ctrl));
                    }
                }
                ShellEvent::MouseDelta { x, y } if self.looking => {
                    if let Some(eye) = self.eye.borrow_mut().as_mut() {
                        eye.look(*x, *y);
                        self.picture_stale = true;
                    }
                }
                ShellEvent::Key { name, pressed, .. } => {
                    let flying = self.looking;
                    let down = *pressed;
                    let typing = self
                        .editor
                        .as_ref()
                        .is_some_and(|e| e.context().egui_wants_keyboard_input());
                    if down && !flying && !typing {
                        let mode = match name.as_str() {
                            "W" => Some(Mode::Translate),
                            "E" => Some(Mode::Rotate),
                            "R" => Some(Mode::Scale),
                            _ => None,
                        };
                        if let Some(mode) = mode {
                            self.mode = mode;
                            self.hovered = None;
                            self.picture_stale = true;
                        }
                    }
                    match name.as_str() {
                        "W" if flying || !down => self.movement.forward = down,
                        "S" if flying || !down => self.movement.back = down,
                        "A" if flying || !down => self.movement.left = down,
                        "D" if flying || !down => self.movement.right = down,
                        "E" if flying || !down => self.movement.up = down,
                        "Q" if flying || !down => self.movement.down = down,
                        _ => {}
                    }
                }
                _ => {}
            }
            if let ShellEvent::Modifiers(modifiers) = event {
                self.modifiers = *modifiers;
                self.movement.fast = modifiers.shift;
            }
        }
    }

    /// The picture of the scene at the viewport's size, drawn when something
    /// changed (the scene, the camera, the size); and what a click on it
    /// means: who is under the pointer, and where the selection is.
    fn render_viewport(&mut self, seconds: f64) -> bool {
        let (width, height) = (
            self.viewport.width().max(0.0) as u32,
            self.viewport.height().max(0.0) as u32,
        );
        if self.viewport.width() < MIN_VIEWPORT || self.viewport.height() < MIN_VIEWPORT {
            self.picture = None;
            self.outlines.clear();
            self.pending_pick = None;
            return false;
        }
        if self.movement.any()
            && let Some(eye) = self.eye.borrow_mut().as_mut()
        {
            eye.step(self.movement, seconds as f32);
            self.picture_stale = true;
        }
        let redraw =
            self.picture_stale || self.drawn_size != (width, height) || self.picture.is_none();
        if !redraw && self.pending_pick.is_none() {
            return false;
        }
        let eye = &self.eye;
        let last = &self.last_camera;
        let camera = |base: &CameraView| {
            let mut eye = eye.borrow_mut();
            let applied = eye
                .get_or_insert_with(|| FlyCamera::from_view(base))
                .apply(base);
            *last.borrow_mut() = Some(applied);
            applied
        };
        let scene = self.scene.clone();
        if redraw {
            // While a handle is dragged the picture shows the change as it
            // would be, without making it.
            let preview = self.drag.as_ref().and_then(|active| active.ops.clone());
            let session = self.play.as_ref().map(|play| play.session.clone());
            let source = match (&session, &preview) {
                (Some(id), _) => PictureSource::Session(id),
                (None, Some(ops)) => PictureSource::Preview(Some(&scene), ops),
                (None, None) => PictureSource::Scene(Some(&scene)),
            };
            // A running game is seen through its own camera.
            let eye_camera: Option<&dyn Fn(&CameraView) -> CameraView> = if session.is_some() {
                None
            } else {
                Some(&camera)
            };
            let drawn = self
                .workspace
                .render_picture(source, eye_camera, width, height, self.view);
            match drawn {
                Ok(picture) => {
                    self.picture = Some(picture.image);
                    if let Some(warning) = picture.warnings.first() {
                        self.status = format!("{warning}");
                    }
                }
                Err(problem) => {
                    self.picture = None;
                    self.status = format!("{problem}");
                }
            }
            self.drawn_size = (width, height);
            self.picture_stale = false;
        }
        // Who is where, for a click and for the box around the selection.
        let pick = self.pending_pick.take();
        if self.drag.is_some() {
            return false;
        }
        if pick.is_none() && self.selected.is_empty() {
            self.outlines.clear();
            return false;
        }
        let session = self.play.as_ref().map(|play| play.session.clone());
        let ids = match &session {
            Some(id) => self
                .workspace
                .render_ids(PictureSource::Session(id), None, width, height),
            None => self.workspace.render_ids(
                PictureSource::Scene(Some(&scene)),
                Some(&camera),
                width,
                height,
            ),
        };
        let Ok(ids) = ids else {
            self.outlines.clear();
            return false;
        };
        let origin = self.viewport.min;
        let before = self.selected.clone();
        if let Some((x, y, toggle)) = pick {
            let hit = ids
                .ids
                .entity_at(
                    (x - origin.x).max(0.0) as u32,
                    (y - origin.y).max(0.0) as u32,
                )
                .and_then(|entity| ids.names.get(&entity).cloned());
            match hit {
                Some(name) => self.click_entity(&name, toggle),
                None if !toggle => self.selected.clear(),
                None => {}
            }
        }
        let mut boxes: BTreeMap<_, _> = ids
            .names
            .iter()
            .filter(|(_, name)| self.selected.contains(name))
            .map(|(entity, _)| (*entity, [i32::MAX, i32::MAX, i32::MIN, i32::MIN]))
            .collect();
        for y in (0..height).step_by(2) {
            for x in (0..width).step_by(2) {
                if let Some(entity) = ids.ids.entity_at(x, y)
                    && let Some(bounds) = boxes.get_mut(&entity)
                {
                    bounds[0] = bounds[0].min(x as i32);
                    bounds[1] = bounds[1].min(y as i32);
                    bounds[2] = bounds[2].max(x as i32);
                    bounds[3] = bounds[3].max(y as i32);
                }
            }
        }
        self.outlines = boxes
            .values()
            .filter(|b| b[0] <= b[2])
            .map(|b| {
                Rect::from_min_max(
                    pos2(origin.x + b[0] as f32, origin.y + b[1] as f32),
                    pos2(origin.x + b[2] as f32 + 2.0, origin.y + b[3] as f32 + 2.0),
                )
            })
            .collect();
        // Whether a click changed the selection (the panels show it).
        before != self.selected
    }

    /// The hierarchy panel: select, add, delete, rename, move.
    fn hierarchy(&self, ui: &mut Ui, actions: &mut Actions) {
        ui.horizontal(|ui| {
            ui.heading("Hierarchy");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(!self.selected.is_empty(), egui::Button::new("Delete"))
                    .clicked()
                {
                    actions.delete = true;
                }
                if ui.button("+").on_hover_text("Add an entity").clicked() {
                    actions.add = Some(None);
                }
            });
        });
        ui.separator();
        let typing = self.ui_state.borrow().rename.is_some() || self.play.is_some();
        if !typing
            && ui.input(|input| input.key_pressed(egui::Key::Delete))
            && !ui.ctx().egui_wants_keyboard_input()
        {
            actions.delete = true;
        }
        if !typing
            && ui.input(|input| input.key_pressed(egui::Key::F2))
            && let Some(first) = self.selected.first()
        {
            self.ui_state.borrow_mut().rename = Some((first.clone(), first.clone()));
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for row in &self.rows {
                let selected = self.selected.contains(&row.id);
                ui.horizontal(|ui| {
                    ui.add_space(row.depth as f32 * 14.0);
                    let renaming = {
                        let state = self.ui_state.borrow();
                        state
                            .rename
                            .as_ref()
                            .filter(|(id, _)| *id == row.id)
                            .map(|(_, text)| text.clone())
                    };
                    if let Some(mut text) = renaming {
                        let editor =
                            ui.add(egui::TextEdit::singleline(&mut text).desired_width(150.0));
                        if !editor.has_focus() && !editor.lost_focus() {
                            editor.request_focus();
                        }
                        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                        let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
                        if enter {
                            actions.rename = Some((row.id.clone(), text.clone()));
                            self.ui_state.borrow_mut().rename = None;
                        } else if escape || editor.lost_focus() {
                            self.ui_state.borrow_mut().rename = None;
                        } else if let Some(state) = self.ui_state.borrow_mut().rename.as_mut() {
                            state.1 = text;
                        }
                        return;
                    }
                    let response = ui
                        .selectable_label(selected, &row.id)
                        .interact(Sense::click_and_drag());
                    if response.clicked() {
                        actions.select =
                            Some((row.id.clone(), ui.input(|input| input.modifiers.command)));
                    }
                    if response.double_clicked() {
                        self.ui_state.borrow_mut().rename = Some((row.id.clone(), row.id.clone()));
                    }
                    if response.drag_started() {
                        response.dnd_set_drag_payload(row.id.clone());
                    }
                    if let Some(dragged) = response.dnd_release_payload::<String>()
                        && *dragged != row.id
                    {
                        actions.reparent = Some(((*dragged).clone(), Some(row.id.clone())));
                    }
                    response.context_menu(|ui| {
                        if ui.button("Add child").clicked() {
                            actions.add = Some(Some(row.id.clone()));
                            ui.close();
                        }
                        if ui.button("Rename").clicked() {
                            self.ui_state.borrow_mut().rename =
                                Some((row.id.clone(), row.id.clone()));
                            ui.close();
                        }
                        if !self.scene.ends_with(".prefab.json")
                            && ui
                                .button("Make prefab")
                                .on_hover_text("Saves it and its children as a prefab and puts an instance in its place")
                                .clicked()
                        {
                            actions.make_prefab = Some(row.id.clone());
                            ui.close();
                        }
                        if ui.button("Delete").clicked() {
                            actions.select = Some((row.id.clone(), false));
                            actions.delete = true;
                            ui.close();
                        }
                    });
                });
            }
            // The rest of the panel: a click there selects nothing; dropping
            // an entity there takes it out of its parent.
            let rest = ui.allocate_response(
                vec2(ui.available_width(), ui.available_height().max(40.0)),
                Sense::click(),
            );
            if rest.clicked() {
                actions.deselect = true;
            }
            if let Some(dragged) = rest.dnd_release_payload::<String>() {
                actions.reparent = Some(((*dragged).clone(), None));
            }
        });
    }

    /// The layout: menu bar, hierarchy, inspector, status bar, and the hole
    /// in the middle that the scene's picture shows through.
    fn layout(&self, ui: &mut Ui, viewport: &mut Rect) -> Actions {
        let mut actions = Actions::default();
        egui::Panel::top("menu").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(format!("Wish Editor - {}", self.project));
                ui.separator();
                egui::ComboBox::from_id_salt("scene")
                    .selected_text(&self.scene)
                    .show_ui(ui, |ui| {
                        for scene in &self.scenes {
                            if ui.selectable_label(*scene == self.scene, scene).clicked() {
                                actions.scene = Some(scene.clone());
                            }
                        }
                        if !self.prefabs.is_empty() {
                            ui.separator();
                            ui.weak("Prefabs");
                        }
                        for prefab in &self.prefabs {
                            if ui.selectable_label(*prefab == self.scene, prefab).clicked() {
                                actions.scene = Some(prefab.clone());
                            }
                        }
                    });
                ui.separator();
                if ui
                    .add_enabled(self.can_undo, egui::Button::new("Undo"))
                    .clicked()
                {
                    actions.undo = true;
                }
                if ui
                    .add_enabled(self.can_redo, egui::Button::new("Redo"))
                    .clicked()
                {
                    actions.redo = true;
                }
                ui.separator();
                egui::ComboBox::from_id_salt("view")
                    .selected_text(self.view.name())
                    .show_ui(ui, |ui| {
                        for name in View::NAMES {
                            if ui
                                .selectable_label(*name == self.view.name(), *name)
                                .clicked()
                                && let Ok(view) = View::parse(name)
                            {
                                actions.view = Some(view);
                            }
                        }
                    });
                match &self.play {
                    None => {
                        if ui
                            .add_enabled(
                                !self.scene.ends_with(".prefab.json"),
                                egui::Button::new("Play"),
                            )
                            .on_hover_text(
                                "Run the game here; changes made while it runs are not kept",
                            )
                            .clicked()
                        {
                            actions.play_start = true;
                        }
                    }
                    Some(play) => {
                        let pause = if play.paused { "Resume" } else { "Pause" };
                        if ui.button(pause).clicked() {
                            actions.play_pause = true;
                        }
                        if ui
                            .add_enabled(play.paused, egui::Button::new("Step"))
                            .on_hover_text("One tick")
                            .clicked()
                        {
                            actions.play_step = true;
                        }
                        if ui.button("Stop").clicked() {
                            actions.play_stop = true;
                        }
                        ui.label(format!("tick {}", play.tick));
                        if play.snapshots.len() > 1 {
                            let pending = self.ui_state.borrow().timeline;
                            let mut cursor = pending.unwrap_or(play.cursor);
                            let response = ui
                                .add(
                                    egui::Slider::new(&mut cursor, 0..=play.snapshots.len() - 1)
                                        .show_value(false),
                                )
                                .on_hover_text("Timeline: let go to go back to that moment");
                            // Where it is put counts when the button is let go.
                            let down = ui.input(|input| input.pointer.any_down());
                            if response.changed() {
                                self.ui_state.borrow_mut().timeline = Some(cursor);
                            }
                            if !down && let Some(to) = self.ui_state.borrow_mut().timeline.take() {
                                actions.rewind_to = Some(to);
                            }
                        }
                    }
                }
                ui.separator();
                if ui.button("Reset camera").clicked() {
                    actions.reset_camera = true;
                }
                ui.separator();
                for (mode, name) in [
                    (Mode::Translate, "Move"),
                    (Mode::Rotate, "Rotate"),
                    (Mode::Scale, "Scale"),
                ] {
                    if ui
                        .selectable_label(self.mode == mode, name)
                        .on_hover_text("Keys W, E, R")
                        .clicked()
                    {
                        actions.mode = Some(mode);
                    }
                }
                let space = if self.space == Space::World {
                    "World"
                } else {
                    "Local"
                };
                if ui
                    .button(space)
                    .on_hover_text("The handles' axes: the world's or the entity's own")
                    .clicked()
                {
                    actions.toggle_space = true;
                }
                if ui
                    .selectable_label(self.snap, "Snap")
                    .on_hover_text("Half metres, 15 degrees, steps of 10 % when on")
                    .clicked()
                {
                    actions.toggle_snap = true;
                }
                if ui.button("Check").clicked() {
                    actions.check = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Quit").clicked() {
                        actions.quit = true;
                    }
                });
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let (errors, warnings) = self.problems;
                ui.label(format!("{errors} error(s), {warnings} warning(s)"));
                ui.separator();
                ui.label(&self.status);
            });
        });
        egui::Panel::bottom("assets")
            .resizable(true)
            .default_size(190.0)
            .size_range(110.0..=420.0)
            .show(ui, |ui| {
                let view = BrowserView {
                    assets: &self.files,
                    selected: self.asset_selected.as_deref(),
                    detail: self.asset_detail.as_ref(),
                    root: self.workspace.root(),
                    state: &self.browser_state,
                };
                crate::browser::panel(ui, &view, &mut actions.browser);
            });
        egui::Panel::left("hierarchy")
            .default_size(220.0)
            .show(ui, |ui| self.hierarchy(ui, &mut actions));
        egui::Panel::right("inspector")
            .default_size(300.0)
            .show(ui, |ui| {
                let view = InspectorView {
                    inspected: &self.inspected,
                    schemas: &self.schemas,
                    entities: self.rows.iter().map(|row| row.id.as_str()).collect(),
                    assets: &self.assets,
                    copied: self.clipboard.as_ref().map(|(name, _)| name.as_str()),
                    state: &self.inspector_state,
                };
                inspector::panel(ui, &view, &mut actions.inspector);
            });
        // The middle is not painted: the scene's picture shows through.
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let area = ui.max_rect();
                *viewport = area;
                // A file dragged from the browser is dropped on the view.
                let target = ui.allocate_rect(area, Sense::hover());
                if let Some(dropped) = target.dnd_release_payload::<AssetDrag>() {
                    let at = ui
                        .input(|input| input.pointer.interact_pos())
                        .unwrap_or_else(|| area.center());
                    actions.drop = Some((dropped.0.clone(), at));
                }
            });
        actions
    }

    /// Runs one frame: input, calls, layout, the scene's picture, painting.
    /// Returns whether something was drawn (the canvas is then new).
    pub fn frame_canvas(
        &mut self,
        size: (u32, u32),
        seconds: f64,
        events: Vec<ShellEvent>,
    ) -> bool {
        self.since_poll += seconds;
        self.since_project_poll += seconds;
        let mut project_changed = false;
        if self.since_project_poll >= PROJECT_POLL_SECONDS {
            self.since_project_poll = 0.0;
            project_changed = self.refresh_project();
        }
        let mut changed = !events.is_empty()
            || project_changed
            || self.play.as_ref().is_some_and(|play| !play.paused)
            || self.frames < 3
            || self.movement.any()
            || self.editor.as_ref().is_some_and(Editor::wants_repaint);
        if self.since_poll >= POLL_SECONDS {
            self.since_poll = 0.0;
            changed |= self.refresh_scene();
        }
        self.refresh_target();
        self.view_input(&events);
        self.tick_play(seconds);
        self.refresh_inspected();
        if changed || self.picture_stale || self.pending_pick.is_some() {
            let egui_events = convert(&events, self.pointer);
            if self.canvas.width != size.0 || self.canvas.height != size.1 {
                self.canvas = Canvas::new(size.0, size.1, BACKDROP);
            }
            let Some(mut editor) = self.editor.take() else {
                return false;
            };
            let mut egui_events = Some(egui_events);
            let mut seconds = seconds;
            let mut passes = 0;
            let frame = loop {
                let mut viewport = self.viewport;
                let mut actions = Actions::default();
                let frame = editor.layout(
                    size,
                    seconds,
                    egui_events.take().unwrap_or_default(),
                    |ui| {
                        actions = self.layout(ui, &mut viewport);
                    },
                );
                self.viewport = viewport;
                let acted = actions.any();
                self.act(actions);
                let picked = self.render_viewport(seconds);
                self.refresh_inspected();
                self.refresh_target();
                passes += 1;
                // What was just done (a click, a button) changes what the
                // panels say: lay them out again before drawing.
                if (acted || picked) && passes < 3 {
                    seconds = 0.0;
                    continue;
                }
                break frame;
            };
            // Backdrop, then the scene, then the panels over it.
            for pixel in self.canvas.pixels.as_chunks_mut::<4>().0 {
                *pixel = BACKDROP;
            }
            if let Some(picture) = &self.picture {
                blit(&mut self.canvas, picture, self.viewport);
            }
            // (Not while a handle is dragged: the box would stay where the
            // entity was.)
            if self.drag.is_none() {
                for outline in &self.outlines {
                    draw_outline(&mut self.canvas, *outline, self.viewport);
                }
            }
            if let (Some(eye), Some((_, target))) = (self.eye_now(), self.target.as_ref()) {
                let current = self.drag.as_ref().map_or(*target, |d| d.now);
                gizmo::draw(
                    &mut self.canvas,
                    self.viewport,
                    &eye,
                    self.mode,
                    gizmo::axes(current.world_rotation(), self.space),
                    current.pivot(),
                    self.drag.as_ref().map(|d| d.drag.handle()).or(self.hovered),
                );
            }
            editor.paint(&frame, &mut self.canvas);
            self.editor = Some(editor);
            self.frames += 1;
            return true;
        }
        false
    }

    /// The canvas as drawn last.
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }
}

/// A two-pixel frame around `rect`, kept inside `clip`.
fn draw_outline(canvas: &mut Canvas, rect: Rect, clip: Rect) {
    let put = |canvas: &mut Canvas, x: i32, y: i32| {
        if x >= clip.min.x as i32
            && x < clip.max.x as i32
            && y >= clip.min.y as i32
            && y < clip.max.y as i32
            && x >= 0
            && y >= 0
            && (x as u32) < canvas.width
            && (y as u32) < canvas.height
        {
            let at = (y as usize * canvas.width as usize + x as usize) * 4;
            canvas.pixels[at..at + 4].copy_from_slice(&OUTLINE);
        }
    };
    let (x0, y0, x1, y1) = (
        rect.min.x as i32,
        rect.min.y as i32,
        rect.max.x as i32,
        rect.max.y as i32,
    );
    for t in 0..2 {
        for x in x0..=x1 {
            put(canvas, x, y0 + t);
            put(canvas, x, y1 - t);
        }
        for y in y0..=y1 {
            put(canvas, x0 + t, y);
            put(canvas, x1 - t, y);
        }
    }
}

/// Copies the picture onto the canvas at the viewport's top-left corner.
fn blit(canvas: &mut Canvas, picture: &RgbaImage, at: Rect) {
    let (left, top) = (at.min.x.max(0.0) as usize, at.min.y.max(0.0) as usize);
    for y in 0..picture.height as usize {
        let to_y = top + y;
        if to_y >= canvas.height as usize {
            break;
        }
        for x in 0..picture.width as usize {
            let to_x = left + x;
            if to_x >= canvas.width as usize {
                break;
            }
            let from = (y * picture.width as usize + x) * 4;
            let to = (to_y * canvas.width as usize + to_x) * 4;
            canvas.pixels[to..to + 4].copy_from_slice(&picture.pixels[from..from + 4]);
        }
    }
}

fn egui_modifiers(modifiers: Modifiers) -> egui::Modifiers {
    egui::Modifiers {
        alt: modifiers.alt,
        ctrl: modifiers.ctrl,
        shift: modifiers.shift,
        mac_cmd: false,
        command: modifiers.ctrl,
    }
}

/// The window's plain events as egui's.
fn convert(events: &[ShellEvent], pointer: Option<(f32, f32)>) -> Vec<egui::Event> {
    let mut out = Vec::new();
    let mut at = pointer.map_or(pos2(0.0, 0.0), |(x, y)| pos2(x, y));
    for event in events {
        match event {
            ShellEvent::Pointer { x, y } => {
                at = pos2(*x, *y);
                out.push(egui::Event::PointerMoved(at));
            }
            ShellEvent::PointerLeft => out.push(egui::Event::PointerGone),
            ShellEvent::Button {
                button,
                pressed,
                modifiers,
            } => out.push(egui::Event::PointerButton {
                pos: at,
                button: match button {
                    Button::Primary => egui::PointerButton::Primary,
                    Button::Secondary => egui::PointerButton::Secondary,
                    Button::Middle => egui::PointerButton::Middle,
                },
                pressed: *pressed,
                modifiers: egui_modifiers(*modifiers),
            }),
            ShellEvent::Scroll { x, y } => out.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(*x, *y),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: egui::Modifiers::NONE,
            }),
            ShellEvent::Key {
                name,
                pressed,
                repeat,
                modifiers,
            } => {
                if let Some(key) = egui::Key::from_name(name) {
                    out.push(egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: *pressed,
                        repeat: *repeat,
                        modifiers: egui_modifiers(*modifiers),
                    });
                }
            }
            ShellEvent::Text(text) => out.push(egui::Event::Text(text.clone())),
            ShellEvent::Modifiers(modifiers) => {
                out.push(egui::Event::ModifiersChanged(egui_modifiers(*modifiers)));
            }
            ShellEvent::MouseDelta { .. } => {}
        }
    }
    out
}

impl Shell for EditorApp {
    fn title(&self) -> String {
        format!("Wish Editor - {} - {}", self.project, self.scene)
    }

    fn frame(
        &mut self,
        size: (u32, u32),
        seconds: f64,
        events: Vec<ShellEvent>,
    ) -> Option<Picture<'_>> {
        if self.frame_canvas(size, seconds, events) {
            let canvas = &self.canvas;
            return Some(Picture {
                width: canvas.width,
                height: canvas.height,
                rgba: &canvas.pixels,
            });
        }
        None
    }

    fn wants_exit(&self) -> bool {
        self.exit
    }
}

/// The ids in a set, for tests that compare hierarchies.
pub fn ids(rows: &[Row]) -> BTreeSet<&str> {
    rows.iter().map(|row| row.id.as_str()).collect()
}
