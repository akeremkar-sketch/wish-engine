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
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use egui::{Align, Layout, Pos2, Rect, Sense, Ui, pos2, vec2};
use glam::{Quat, Vec3};
use serde_json::{Value, json};
use wish_control::{PictureSource, Workspace, parse_actor};
use wish_render::{CameraView, RgbaImage, View};
use wish_window::{Button, FlyCamera, Modifiers, Movement, Picture, Shell, ShellEvent};

use crate::Editor;
use crate::browser::{Asset, AssetDrag, BrowserActions, BrowserState, BrowserView};
use crate::gizmo::{self, Drag, Eye, Handle, Mode, Space, Target};
use crate::i18n::{t, tf};
use crate::inspector::{self, Inspected, InspectorActions, InspectorState, InspectorView};
use crate::panels::{PanelActions, Tab, Timing};
use crate::raster::Canvas;
use crate::settings::{Draft, SettingsActions};

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

/// Opens a project file in an editor outside the engine.
pub type Opener = Box<dyn FnMut(&std::path::Path) -> Result<(), String>>;

/// The default way to open a file: the program named by the `WISH_EDITOR`
/// environment variable (such as `code`), or the system's own choice.
fn open_with_system(path: &std::path::Path) -> Result<(), String> {
    use std::process::Command;
    let spawned = match std::env::var("WISH_EDITOR") {
        Ok(program) if !program.trim().is_empty() => Command::new(program.trim()).arg(path).spawn(),
        _ if cfg!(windows) => Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn(),
        _ if cfg!(target_os = "macos") => Command::new("open").arg(path).spawn(),
        _ => Command::new("xdg-open").arg(path).spawn(),
    };
    spawned.map(|_| ()).map_err(|error| error.to_string())
}

/// A before/after comparison: a checkpoint's picture of the scene and
/// now's.
struct Compare {
    name: String,
    before: egui::ColorImage,
    now: egui::ColorImage,
    textures: RefCell<Option<(egui::TextureHandle, egui::TextureHandle)>>,
}

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
    /// The run is being recorded as a replay (saved when it stops).
    recording: bool,
    /// Snapshots made so far (labels are never reused).
    made: u64,
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
const BACKDROP: [u8; 4] = crate::style::BACKDROP;
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
    /// Why the scene cannot be shown (message, hint), if it cannot.
    scene_error: Option<(String, String)>,
    /// The revision the next edit is based on, if not the latest seen (a
    /// drag: the one it began on).
    edit_base: Option<u64>,
    /// Resource schemas, by name (the ones a scene may hold).
    resource_schemas: BTreeMap<String, Value>,
    /// Nothing is selected: the inspector shows the scene's resources.
    inspecting_resources: bool,
    /// The scene's history, newest first.
    history: Vec<Value>,
    /// The project's checkpoints, oldest first.
    checkpoints: Vec<Value>,
    /// How much agents may change by themselves (`permissions`).
    permission_mode: String,
    /// Agents' proposals (waiting ones are shown in the Activity tab).
    proposals: Vec<Value>,
    /// The agent's panel, its task and the editor's MCP endpoint (M8b).
    cockpit: crate::cockpit::Cockpit,
    /// The AI mode is on (M9c): the game card, decisions, plan, notes and
    /// the conversation instead of the manual panels.
    ai_mode: bool,
    /// What the AI mode's panels are writing.
    ai: RefCell<crate::ai_mode::AiState>,
    /// The game's design (`game_design get`).
    design: Value,
    /// The last test run: passed, failed.
    tests: Option<(usize, usize)>,
    /// Where Claude Code is (looked for when the AI settings open).
    assistant: Option<String>,
    /// The "connect an outside agent" window is open.
    connect_open: bool,
    /// An earlier export of this name is there: the person is asked before
    /// it is replaced (its folder).
    export_confirm: Option<String>,
    /// A before/after comparison being shown: the checkpoint, its picture
    /// and now's, and their textures once made.
    compare: Option<Compare>,
    /// Record the next run as a replay.
    record: bool,
    /// The project has game code that is not built yet.
    unbuilt: bool,
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
    /// The game view is wanted beside the scene view (shown when the middle
    /// is wide enough).
    game_view: bool,
    /// Where the game view is this frame (`Rect::NOTHING`: not shown).
    game_viewport: Rect,
    /// The game view's picture: the scene's own camera, with the game's
    /// interface.
    game_picture: Option<RgbaImage>,
    /// What the game picture was drawn for (scene revision, game tick,
    /// size), so it is drawn again only when one of them changes.
    game_drawn: Option<(u64, Option<u64>, u32, u32)>,
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
    /// The settings window's draft, while it is open.
    settings: RefCell<Option<Draft>>,
    /// The bottom panel's tab.
    tab: Tab,
    /// The console's entries (from `logs_get`), and the last one read.
    log: VecDeque<Value>,
    log_seq: u64,
    /// The activity feed, oldest first, and the last entry read.
    activity: VecDeque<Value>,
    activity_seq: u64,
    /// The feed shows agents only.
    agents_only: std::cell::Cell<bool>,
    since_log: f64,
    /// What `check` found last.
    found: Vec<Value>,
    /// Frame times, for the performance tab. (Measured with the wall clock:
    /// the editor's own speed, never the simulation's.)
    timings: VecDeque<Timing>,
    /// The game's share of the frame being drawn.
    game_time: (f32, u32),
    /// How a file is opened outside the engine.
    opener: Opener,
    /// The interface's language (a code of [`crate::i18n::LANGUAGES`]).
    language: &'static str,
    /// Where the person's own editor preferences are kept (the language),
    /// if anywhere: tests keep none.
    preferences: Option<std::path::PathBuf>,
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
    /// The scene's revision when the drag began: a change made by someone
    /// else meanwhile is not overwritten.
    revision: u64,
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
    /// Show the interface in this language.
    language: Option<&'static str>,
    /// Look for problems and show them.
    show_problems: bool,
    run_tests: bool,
    /// Save a checkpoint now.
    save_checkpoint: bool,
    /// Export the game.
    export: bool,
    /// Replace the earlier export (asked).
    export_replace: bool,
    /// Keep the earlier export.
    export_cancel: bool,
    /// Set the permission mode.
    permission: Option<&'static str>,
    /// Show or hide the agent's panel.
    toggle_cockpit: bool,
    /// Show or hide the game view.
    toggle_game_view: bool,
    /// Open (true) or close (false) the "connect an outside agent" window.
    connect: Option<bool>,
    /// What the agent's panel asked for.
    cockpit: crate::cockpit::CockpitActions,
    /// Switch to the AI mode (true) or the manual one.
    switch_mode: Option<bool>,
    /// What the AI mode's panels asked for.
    ai: crate::ai_mode::AiActions,
    /// Go back to this checkpoint.
    restore_checkpoint: Option<String>,
    /// Compare this checkpoint with now.
    compare_checkpoint: Option<String>,
    /// Delete this checkpoint.
    delete_checkpoint: Option<String>,
    /// Close the before/after window.
    close_compare: bool,
    /// Run the tests and keep their pictures as the golden ones.
    update_golden: bool,
    /// Play a saved replay back.
    replay: Option<String>,
    save_picture: bool,
    toggle_record: bool,
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
    /// Open the settings window.
    open_settings: bool,
    /// Open the game code in an outside editor.
    edit_code: bool,
    /// Build the game code.
    build_code: bool,
    /// What the settings window asked for.
    settings: SettingsActions,
    /// What the bottom panel's tabs asked for.
    panels: PanelActions,
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
            || self.language.is_some()
            || self.show_problems
            || self.run_tests
            || self.save_checkpoint
            || self.export
            || self.toggle_game_view
            || self.export_replace
            || self.export_cancel
            || self.permission.is_some()
            || self.toggle_cockpit
            || self.connect.is_some()
            || self.cockpit.any()
            || self.switch_mode.is_some()
            || self.ai.any()
            || self.restore_checkpoint.is_some()
            || self.compare_checkpoint.is_some()
            || self.delete_checkpoint.is_some()
            || self.close_compare
            || self.update_golden
            || self.replay.is_some()
            || self.save_picture
            || self.toggle_record
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
            || self.open_settings
            || self.edit_code
            || self.build_code
            || self.settings.any()
            || self.panels.any()
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
            game_view: true,
            game_viewport: Rect::NOTHING,
            game_picture: None,
            game_drawn: None,
            picture_stale: true,
            ui_state: RefCell::new(UiState::default()),
            prefabs: Vec::new(),
            files: Vec::new(),
            asset_count: u64::MAX,
            asset_selected: None,
            asset_detail: None,
            browser_state: RefCell::new(BrowserState::default()),
            clipboard: None,
            settings: RefCell::new(None),
            tab: Tab::Assets,
            log: VecDeque::new(),
            log_seq: 0,
            activity: VecDeque::new(),
            activity_seq: 0,
            agents_only: std::cell::Cell::new(false),
            since_log: 0.0,
            found: Vec::new(),
            timings: VecDeque::new(),
            game_time: (0.0, 0),
            opener: Box::new(open_with_system),
            language: "en",
            preferences: None,
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
            scene_error: None,
            edit_base: None,
            resource_schemas: BTreeMap::new(),
            inspecting_resources: false,
            history: Vec::new(),
            checkpoints: Vec::new(),
            permission_mode: "apply".to_owned(),
            proposals: Vec::new(),
            connect_open: false,
            export_confirm: None,
            cockpit: crate::cockpit::Cockpit::new(Box::new(
                wish_agent::claude::ClaudeCode::default(),
            )),
            ai_mode: false,
            ai: RefCell::new(crate::ai_mode::AiState::default()),
            design: Value::Null,
            tests: None,
            assistant: None,
            compare: None,
            record: false,
            unbuilt: false,
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
        // Resources a scene may hold: not the ones the engine writes every
        // tick (time, input, the pointer).
        let resources: Vec<String> = listed["types"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| t["kind"] == "resource")
            .filter_map(|t| t["name"].as_str().map(str::to_owned))
            .filter(|name| !["Time", "ActionState", "Pointer"].contains(&name.as_str()))
            .collect();
        let full = self.call(
            "schema_get",
            &json!({ "names": resources, "detail": "full" }),
        );
        self.resource_schemas = full["types"]
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
        self.inspecting_resources = self.selected.is_empty();
        if self.selected.is_empty() {
            self.read_resources();
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
                overridden: entity["overridden"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c.as_str().map(str::to_owned))
                    .collect(),
                notes: entity["notes"].as_str().map(str::to_owned),
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

    /// The scene's resources (or the running game's), as one pseudo-entity
    /// for the inspector: their effective values.
    fn read_resources(&mut self) {
        let names: Vec<String> = match &self.play {
            Some(_) => self.resource_schemas.keys().cloned().collect(),
            None => {
                let got = self.call(
                    "scene_get",
                    &json!({ "scene": self.scene.clone(), "detail": "summary" }),
                );
                got["resources"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|n| n.as_str().map(str::to_owned))
                    .collect()
            }
        };
        let names: Vec<String> = names
            .into_iter()
            .filter(|name| self.resource_schemas.contains_key(name))
            .collect();
        let mut values = serde_json::Map::new();
        if !names.is_empty() {
            let args = match &self.play {
                Some(play) => {
                    json!({ "session": play.session.clone(), "ids": [], "resources": names })
                }
                None => json!({ "scene": self.scene.clone(), "ids": [], "resources": names }),
            };
            let got = self.call("entity_get", &args);
            if let Some(all) = got["resources"].as_object() {
                values.clone_from(all);
            }
            // While playing, only the ones the game holds.
            if self.play.is_some()
                && let Some(defaulted) = got["resources_defaulted"].as_array()
            {
                for name in defaulted.iter().filter_map(Value::as_str) {
                    values.remove(name);
                }
            }
        }
        self.inspected.push(Inspected {
            id: String::new(),
            components: values,
            ..Inspected::default()
        });
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
                        revision: self.revision,
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
                            Mode::Translate => tf("moved {id}", &[("id", &active.id)]),
                            Mode::Rotate => tf("rotated {id}", &[("id", &active.id)]),
                            Mode::Scale => tf("scaled {id}", &[("id", &active.id)]),
                        };
                        let Value::Array(ops) = ops else {
                            return true;
                        };
                        self.edit_base = Some(active.revision);
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
                self.status = t("cancelled").to_owned();
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
        if self.inspecting_resources {
            self.resource_act(asked);
            self.inspected_for = (Vec::new(), u64::MAX);
            return;
        }
        if let Some((id, text)) = asked.notes {
            {
                let op = if text.trim().is_empty() {
                    json!({ "op": "set_notes", "id": id })
                } else {
                    json!({ "op": "set_notes", "id": id, "notes": text })
                };
                self.edit(&tf("notes of {id}", &[("id", &id)]), vec![op]);
            }
            self.inspected_for = (Vec::new(), u64::MAX);
            return;
        }
        if let Some(component) = asked.revert {
            let ops: Vec<Value> = self
                .inspected
                .iter()
                .filter(|entity| entity.overridden.contains(&component))
                .filter_map(|entity| entity.id.split_once('/'))
                .map(|(instance, inner)| {
                    json!({ "op": "remove_override", "id": instance, "entity": inner,
                            "component": component })
                })
                .collect();
            self.edit(
                &tf(
                    "{component} back to the prefab's",
                    &[("component", &component)],
                ),
                ops,
            );
            self.inspected_for = (Vec::new(), u64::MAX);
            return;
        }
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
            self.edit(
                &tf(
                    "set {component}.{field}",
                    &[("component", &component), ("field", &field)],
                ),
                ops,
            );
        }
        if let Some(component) = asked.copy {
            if let Some(value) = self
                .inspected
                .first()
                .and_then(|entity| entity.components.get(&component))
            {
                self.clipboard = Some((component.clone(), value.clone()));
                self.status = tf("copied {component}", &[("component", &component)]);
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
                // Pasted values replace the component (a part's override
                // too), as Reset does.
                let ops = targets
                    .iter()
                    .map(|(id, part)| match (part, id.split_once('/')) {
                        (true, Some((instance, inner))) => json!({
                            "op": "replace_override", "id": instance, "entity": inner,
                            "component": name, "value": value.clone()
                        }),
                        _ => json!({ "op": "replace", "id": id, "component": name, "value": value.clone() }),
                    })
                    .collect();
                self.edit(&tf("pasted {name}", &[("name", &name)]), ops);
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
            self.edit(&tf("reset {component}", &[("component", &component)]), ops);
            self.inspected_for = (Vec::new(), u64::MAX);
            return;
        }
        if let Some(component) = asked.add {
            let ops = targets
                .iter()
                .map(|(id, part)| op_for(id, *part, &component, json!({})))
                .collect();
            self.edit(&tf("added {component}", &[("component", &component)]), ops);
        }
        if let Some(component) = asked.remove {
            let ops = targets
                .iter()
                .map(|(id, _)| json!({ "op": "remove", "id": id, "component": component }))
                .collect();
            self.edit(
                &tf("removed {component}", &[("component", &component)]),
                ops,
            );
        }
        self.inspected_for = (Vec::new(), u64::MAX);
    }

    /// The inspector's changes to the scene's resources.
    fn resource_act(&mut self, asked: InspectorActions) {
        if let Some((name, field, value)) = asked.set {
            let op =
                json!({ "op": "set_resource", "name": name, "value": { field.clone(): value } });
            self.edit(
                &tf(
                    "set {component}.{field}",
                    &[("component", &name), ("field", &field)],
                ),
                vec![op],
            );
        }
        if let Some(name) = asked.copy
            && let Some(value) = self
                .inspected
                .first()
                .and_then(|entity| entity.components.get(&name))
        {
            self.clipboard = Some((name.clone(), value.clone()));
            self.status = tf("copied {component}", &[("component", &name)]);
        }
        if let Some(name) = asked.paste
            && let Some((_, value)) = self.clipboard.clone().filter(|(copied, _)| *copied == name)
        {
            let op = json!({ "op": "replace_resource", "name": name, "value": value });
            self.edit(&tf("pasted {name}", &[("name", &name)]), vec![op]);
        }
        if let Some(name) = asked.reset {
            let op = json!({ "op": "replace_resource", "name": name, "value": {} });
            self.edit(&tf("reset {component}", &[("component", &name)]), vec![op]);
        }
        if let Some(name) = asked.add {
            let op = json!({ "op": "set_resource", "name": name, "value": {} });
            self.edit(&tf("added {component}", &[("component", &name)]), vec![op]);
        }
        if let Some(name) = asked.remove {
            let op = json!({ "op": "remove_resource", "name": name });
            self.edit(
                &tf("removed {component}", &[("component", &name)]),
                vec![op],
            );
        }
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
        // An agent may save checkpoints too: the menu follows.
        self.read_checkpoints();
        self.read_proposals();
        self.read_design();
        let info = self.call("project_info", &json!({}));
        if info["ok"] != true {
            self.status = info["error"]["message"]
                .as_str()
                .unwrap_or(t("the project could not be read"))
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
        let problems = (
            info["problems"]["errors"].as_u64().unwrap_or(0),
            info["problems"]["warnings"].as_u64().unwrap_or(0),
        );
        // The problems tab follows what the project has (it is not empty
        // while the status bar counts errors).
        if problems != self.problems {
            self.problems = problems;
            self.run_check();
        }
        self.unbuilt = !info["scripts"]["sources"].is_null() && info["scripts"]["module"].is_null();
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
                .unwrap_or(t("the scene could not be read"));
            let hint = got["error"]["hint"].as_str().unwrap_or("");
            let error = (message.to_owned(), hint.to_owned());
            let changed = self.scene_error.as_ref() != Some(&error);
            self.status = message.to_owned();
            self.scene_error = Some(error);
            self.rows.clear();
            return changed;
        }
        if self.scene_error.take().is_some() {
            self.status.clear();
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
        self.history = history["entries"].as_array().cloned().unwrap_or_default();
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
        let base = self.edit_base.take().unwrap_or(self.revision);
        if self.play.is_none() && base != u64::MAX {
            args["base_revision"] = json!(base);
        }
        let done = self.call("edit_apply", &args);
        let applied = done["ok"] == true;
        self.status = if applied && self.play.is_some() {
            tf(
                "{what} (live: not kept when the game stops)",
                &[("what", &what)],
            )
        } else if applied {
            what.to_owned()
        } else {
            let message = done["error"]["message"]
                .as_str()
                .unwrap_or(t("it did not work"));
            let hint = done["error"]["hint"].as_str().unwrap_or("");
            tf(
                "{what} failed: {message} {hint}",
                &[("what", &what), ("message", &message), ("hint", &hint)],
            )
        };
        // Look again either way: a refusal means the scene is not what the
        // person saw.
        self.revision = u64::MAX;
        self.refresh_scene();
        self.inspected_for = (Vec::new(), u64::MAX);
        self.picture_stale = true;
        applied
    }

    /// Replaces the way files are opened outside the engine (tests record
    /// instead of starting a program).
    pub fn set_opener(&mut self, opener: Opener) {
        self.opener = opener;
    }

    /// Shows the interface in the language `code` (`tr`, `de-DE`...).
    /// Returns whether the editor has that language (if not, nothing
    /// changes).
    pub fn set_language(&mut self, code: &str) -> bool {
        match crate::i18n::find(code) {
            Some(language) => {
                self.language = language.code;
                crate::i18n::set_language(language.code);
                self.picture_stale = true;
                true
            }
            None => false,
        }
    }

    /// The interface's language (its code).
    pub fn language(&self) -> &'static str {
        self.language
    }

    /// Keeps the person's editor preferences in `path` (a JSON file such as
    /// `{ "language": "tr" }`): reads them now, and writes them when they
    /// change. Without a file, the language comes from `WISH_LANG` or the
    /// system's; else English.
    pub fn use_preferences(&mut self, path: std::path::PathBuf) {
        let prefs = crate::prefs::read(&path);
        if let Some(shown) = prefs.get("game_view").and_then(Value::as_bool) {
            self.game_view = shown;
        }
        if let Some(on) = prefs.get("ai_mode").and_then(Value::as_bool) {
            self.ai_mode = on;
        }
        if let Some(agent) = prefs.get("agent").and_then(Value::as_object) {
            let settings = &mut self.cockpit.settings;
            settings.model = agent
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned);
            for (key, slot) in [
                ("memory", &mut settings.memory),
                ("checkpoint_first", &mut settings.checkpoint_first),
                ("files", &mut settings.files),
            ] {
                if let Some(on) = agent.get(key).and_then(Value::as_bool) {
                    *slot = on;
                }
            }
        }
        let saved = prefs
            .get("language")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let chosen = saved
            .as_deref()
            .and_then(crate::i18n::find)
            .or_else(crate::i18n::from_environment);
        if let Some(language) = chosen {
            self.set_language(language.code);
        }
        // This project becomes the first of the recent ones (the welcome
        // screen lists them); a failure to write costs only that.
        let root = self.workspace.root().to_path_buf();
        let _ = crate::prefs::update(&path, |prefs| crate::prefs::remember(prefs, &root));
        self.preferences = Some(path);
    }

    /// Shows the Agent panel (the welcome screen opens it when the person
    /// chose to work with an assistant).
    pub fn open_agent_panel(&mut self) {
        self.cockpit.open = true;
        self.ai_mode = true;
        self.picture_stale = true;
    }

    /// Whether the AI mode is on.
    pub fn ai_mode(&self) -> bool {
        self.ai_mode
    }

    /// Turns the AI mode on or off (the menu bar's switch does the same).
    pub fn set_ai_mode(&mut self, on: bool) {
        if self.ai_mode != on {
            self.ai_mode = on;
            self.picture_stale = true;
            self.game_drawn = None;
            self.save_preferences();
        }
    }

    /// The game's design as the editor last read it.
    pub fn design(&self) -> &Value {
        &self.design
    }

    /// The last task given to the agent.
    pub fn last_task(&self) -> Option<&wish_agent::Task> {
        self.cockpit.last_task.as_ref()
    }

    /// Reads the game's design (the AI mode's panels show it).
    fn read_design(&mut self) -> bool {
        let got = self.call("game_design", &json!({}));
        let design = if got["ok"] == true {
            got["design"].clone()
        } else {
            Value::Null
        };
        let changed = design != self.design;
        self.design = design;
        changed
    }

    /// Whether the Agent panel is shown.
    pub fn agent_panel_open(&self) -> bool {
        self.cockpit.open
    }

    /// Sets the status line (what the welcome screen did before the editor
    /// opened, such as building the game code).
    pub fn set_status(&mut self, status: String) {
        self.status = status;
    }

    /// Writes the preferences, if the editor keeps them (a failure only
    /// shows in the status bar: the choice still holds for this run).
    fn save_preferences(&mut self) {
        let Some(path) = self.preferences.clone() else {
            return;
        };
        let language = self.language;
        let game_view = self.game_view;
        let ai_mode = self.ai_mode;
        let settings = &self.cockpit.settings;
        let agent = json!({
            "model": settings.model,
            "memory": settings.memory,
            "checkpoint_first": settings.checkpoint_first,
            "files": settings.files,
        });
        let written = crate::prefs::update(&path, |prefs| {
            prefs.insert("language".to_owned(), json!(language));
            prefs.insert("game_view".to_owned(), json!(game_view));
            prefs.insert("ai_mode".to_owned(), json!(ai_mode));
            prefs.insert("agent".to_owned(), agent);
        });
        if let Err(error) = written {
            self.status = tf(
                "the language could not be saved: {error}",
                &[("error", &error)],
            );
        }
    }

    /// Opens a project file in an outside editor.
    fn open_file(&mut self, relative: &str) {
        let path = self.workspace.root().join(relative);
        if !path.exists() {
            self.status = tf("{relative} does not exist", &[("relative", &relative)]);
            return;
        }
        self.status = match (self.opener)(&path) {
            Ok(()) => tf("opened {relative}", &[("relative", &relative)]),
            Err(error) => tf(
                "could not open {relative}: {error} (set WISH_EDITOR to your editor, such as code)",
                &[("relative", &relative), ("error", &error)],
            ),
        };
    }

    /// Reads what was logged since the console last looked.
    fn read_log(&mut self) {
        let got = self.call("logs_get", &json!({ "since": self.log_seq }));
        if got["ok"] != true {
            return;
        }
        for entry in got["entries"].as_array().into_iter().flatten() {
            self.log.push_back(entry.clone());
            if self.log.len() > crate::panels::MAX_LOG {
                self.log.pop_front();
            }
        }
        if let Some(seq) = got["last_seq"].as_u64() {
            self.log_seq = self.log_seq.max(seq);
        }
    }

    /// Reads what was done since the feed last looked.
    fn read_activity(&mut self) {
        let got = self.call(
            "activity_get",
            &json!({ "since": self.activity_seq, "limit": 500 }),
        );
        if got["ok"] != true {
            return;
        }
        for entry in got["entries"].as_array().into_iter().flatten() {
            self.activity.push_back(entry.clone());
            if self.activity.len() > crate::panels::MAX_LOG {
                self.activity.pop_front();
            }
        }
        if let Some(seq) = got["last_seq"].as_u64() {
            self.activity_seq = self.activity_seq.max(seq);
        }
    }

    /// Replaces the way the agent runs (tests use a scripted one).
    pub fn set_agent(&mut self, agent: Box<dyn wish_agent::Adapter>) {
        self.cockpit.agent = agent;
    }

    /// The agent's transcript, one line each (`You: ...`, `edit_apply: done`).
    pub fn transcript(&self) -> Vec<String> {
        use crate::cockpit::Who;
        self.cockpit
            .transcript
            .iter()
            .map(|line| match line.who {
                Who::Person => format!("You: {}", line.text),
                Who::Agent => line.text.clone(),
                Who::Tool => crate::cockpit::tool_line(&line.text, line.ok == Some(true)),
                Who::Note => line.text.clone(),
            })
            .collect()
    }

    /// Whether an agent's task is running.
    pub fn agent_running(&self) -> bool {
        self.cockpit.run.is_some()
    }

    /// What the last task told the agent.
    pub fn last_instructions(&self) -> Option<&str> {
        self.cockpit.last_instructions.as_deref()
    }

    /// What goes with the next request.
    pub fn agent_context(&self) -> &[String] {
        &self.cockpit.context
    }

    /// The agent's calls waiting at the endpoint run now, against this
    /// editor's workspace; and what the task reported is taken in. Returns
    /// whether anything happened.
    fn cockpit_tick(&mut self) -> bool {
        let mut ran = 0;
        if let Some((endpoint, _)) = self.cockpit.mcp.as_mut() {
            ran = endpoint.serve_pending(
                &mut wish_mcp::Server::new(&mut self.workspace),
                std::time::Duration::ZERO,
            );
        }
        let (reported, changed) = self.cockpit.take_events();
        if ran > 0 {
            self.read_design();
        }
        if ran > 0 || changed {
            // The agent changed things: show them now, not at the next poll.
            self.revision = u64::MAX;
            self.refresh_scene();
            self.inspected_for = (Vec::new(), u64::MAX);
            self.picture_stale = true;
        }
        if reported && self.cockpit.run.is_none() {
            self.status = t("the agent finished").to_owned();
            self.refresh_project();
        }
        ran > 0 || reported
    }

    /// A line about an entity, for the agent: its id and where it is.
    fn describe_for_agent(&mut self, id: &str) -> String {
        let got = self.call(
            "entity_get",
            &json!({ "scene": self.scene.clone(), "ids": [id], "components": ["Transform"] }),
        );
        match got["entities"][0]["components"]["Transform"]["translation"].as_array() {
            Some(at) => {
                let at: Vec<String> = at
                    .iter()
                    .map(|v| format!("{:.2}", v.as_f64().unwrap_or(0.0)))
                    .collect();
                format!("the entity `{id}` (at {})", at.join(", "))
            }
            None => format!("the entity `{id}`"),
        }
    }

    /// The agent panel's actions.
    fn cockpit_act(&mut self, asked: &crate::cockpit::CockpitActions) {
        if let Some(index) = asked.remove_context
            && index < self.cockpit.context.len()
        {
            self.cockpit.context.remove(index);
        }
        if asked.point {
            self.cockpit.pointing = !self.cockpit.pointing;
        }
        if asked.add_selection {
            for id in self.selected.clone() {
                let line = self.describe_for_agent(&id);
                if !self.cockpit.context.contains(&line) {
                    self.cockpit.context.push(line);
                }
            }
        }
        if asked.stop
            && let Some(run) = self.cockpit.run.as_mut()
        {
            run.stop();
            self.cockpit.transcript.push(crate::cockpit::Line {
                who: crate::cockpit::Who::Note,
                text: t("Stopped. What was done stays (it is in the history).").to_owned(),
                ok: Some(false),
            });
            self.cockpit.run = None;
        }
        if asked.send && self.cockpit.run.is_none() {
            self.send_task();
        }
    }

    /// One `game_design` call as the person; the design is read again.
    /// Returns whether it worked (if not, the status bar says why).
    fn design_call(&mut self, args: Value) -> bool {
        let got = self.call("game_design", &args);
        if got["ok"] != true {
            self.status = got["error"]["message"]
                .as_str()
                .unwrap_or(t("it did not work"))
                .to_owned();
            return false;
        }
        self.read_design();
        true
    }

    /// A play-test note: the moment (the game's tick while it plays), the
    /// scene and its picture, kept under `design/notes/`.
    fn take_note(&mut self, text: String) {
        let first = self.design["next_id"].as_u64().unwrap_or(1);
        let picture = (first..first + 10_000)
            .map(|n| format!("design/notes/{n}.png"))
            .find(|path| !self.workspace.root().join(path).exists())
            .unwrap_or_else(|| format!("design/notes/{first}.png"));
        let (args, tick) = match &self.play {
            Some(play) => (
                json!({ "session": play.session.clone(), "out": picture.clone() }),
                Some(play.tick),
            ),
            None => (
                json!({ "scene": self.scene.clone(), "out": picture.clone() }),
                None,
            ),
        };
        let saved = self.call("capture_frame", &args);
        let mut note = json!({ "action": "note", "text": text, "scene": self.scene.clone() });
        if let Some(tick) = tick {
            note["tick"] = json!(tick);
        }
        if saved["ok"] == true {
            note["picture"] = json!(picture);
        }
        if self.design_call(note) {
            self.status = t("note taken").to_owned();
        }
    }

    /// The AI mode's actions.
    fn ai_act(&mut self, ai: crate::ai_mode::AiActions) {
        if let Some(fields) = ai.save_card
            && self.design_call(json!({ "action": "set_brief", "fields": fields }))
        {
            self.ai.borrow_mut().card_from = None;
            self.status = t("the game card was saved").to_owned();
        }
        if let Some(text) = ai.decide {
            self.design_call(json!({ "action": "decide", "text": text }));
        }
        if let Some(id) = ai.forget {
            self.design_call(json!({ "action": "forget", "id": id }));
        }
        if let Some(text) = ai.plan_add {
            self.design_call(json!({ "action": "plan_add", "text": text }));
        }
        if let Some((id, status)) = ai.plan_status {
            self.design_call(json!({ "action": "plan_set", "id": id, "status": status }));
        }
        if let Some(id) = ai.plan_remove {
            self.design_call(json!({ "action": "plan_remove", "id": id }));
        }
        if let Some(text) = ai.note {
            self.take_note(text);
        }
        if let Some(id) = ai.note_done {
            self.design_call(json!({ "action": "note_done", "id": id }));
        }
        if let Some((id, answer, keep)) = ai.answer {
            let question = self.design["questions"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|q| q["id"].as_u64() == Some(id))
                .and_then(|q| q["text"].as_str())
                .unwrap_or("")
                .to_owned();
            if self.design_call(
                json!({ "action": "answer", "id": id, "answer": answer, "remember": keep }),
            ) {
                self.ai.borrow_mut().own.remove(&id);
                // The agent stopped to ask: it goes on with the answer.
                self.start_task(tf(
                    "My answer to \"{question}\": {answer}. Go on.",
                    &[("question", &question), ("answer", &answer)],
                ));
            }
        }
        if let Some(request) = ai.ready {
            *self.cockpit.request.borrow_mut() = request;
        }
        if ai.draft_card {
            self.start_task(
                t("Write the game card from the game as it is now (game_design set_brief). Ask me about what you cannot tell.")
                    .to_owned(),
            );
        }
        if ai.next_step {
            let next = self.design["plan"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|step| step["status"] != "done")
                .map(|step| {
                    (
                        step["id"].as_u64().unwrap_or(0),
                        step["text"].as_str().unwrap_or("").to_owned(),
                    )
                });
            if let Some((id, text)) = next {
                self.start_task(tf(
                    "Do the next step of the plan: [{id}] {text}. Mark it doing when you start and done when it works (game_design plan_set).",
                    &[("id", &id), ("text", &text)],
                ));
            }
        }
        if ai.send_notes {
            let notes: Vec<String> = self.design["notes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|note| note["status"] == "open")
                .map(|note| {
                    let mut line = format!(
                        "- [{}] {}",
                        note["id"].as_u64().unwrap_or(0),
                        note["text"].as_str().unwrap_or("")
                    );
                    if let Some(tick) = note["tick"].as_u64() {
                        line.push_str(&format!(" (tick {tick})"));
                    }
                    if let Some(picture) = note["picture"].as_str() {
                        line.push_str(&format!(" (picture: {picture})"));
                    }
                    line
                })
                .collect();
            if !notes.is_empty() {
                self.start_task(tf(
                    "Fix what these notes from playing the game say, then close each one (game_design note_done): {notes}",
                    &[("notes", &format!("\n{}", notes.join("\n")))],
                ));
            }
        }
        if ai.fix {
            let mut problems: Vec<String> = self
                .found
                .iter()
                .take(20)
                .map(|problem| {
                    format!(
                        "- {} ({})",
                        problem["message"].as_str().unwrap_or(""),
                        problem["code"].as_str().unwrap_or("")
                    )
                })
                .collect();
            if self.unbuilt {
                problems.push(t("- the game code is not built (script_build)").to_owned());
            }
            if problems.is_empty() {
                problems.push(t("- run check and test_run, and fix what fails").to_owned());
            }
            self.start_task(tf(
                "Fix these problems, then check again (check, test_run): {problems}",
                &[("problems", &format!("\n{}", problems.join("\n")))],
            ));
        }
        if let Some(open) = ai.settings {
            if open {
                self.assistant = wish_agent::claude::find().map(|path| path.display().to_string());
            }
            self.ai.borrow_mut().settings = open;
        }
        let before = self.cockpit.settings.clone();
        if let Some(model) = ai.model {
            self.cockpit.settings.model = model;
        }
        if let Some(memory) = ai.memory {
            self.cockpit.settings.memory = memory;
            if !memory {
                self.cockpit.session = None;
            }
        }
        if let Some(first) = ai.checkpoint_first {
            self.cockpit.settings.checkpoint_first = first;
        }
        if let Some(files) = ai.files {
            self.cockpit.settings.files = files;
        }
        if self.cockpit.settings != before {
            self.save_preferences();
        }
        if ai.new_conversation {
            self.cockpit.session = None;
            self.cockpit.transcript.push(crate::cockpit::Line {
                who: crate::cockpit::Who::Note,
                text: t("A new conversation: the agent starts afresh (the game card and the decisions stay).")
                    .to_owned(),
                ok: Some(true),
            });
        }
        if let Some(scene) = ai.scene_view {
            self.ai.borrow_mut().scene_view = scene;
            self.picture_stale = true;
            self.game_drawn = None;
        }
        if ai.show_activity {
            self.set_ai_mode(false);
            self.tab = Tab::Activity;
        }
    }

    /// The editor's MCP endpoint (loopback, a fresh token), started once.
    fn ensure_endpoint(&mut self) -> Result<(), ()> {
        if self.cockpit.mcp.is_some() {
            return Ok(());
        }
        let token = wish_mcp::http::random_token();
        let started = std::net::TcpListener::bind("127.0.0.1:0").and_then(|listener| {
            wish_mcp::http::Endpoint::start(
                listener,
                wish_mcp::http::HttpOptions {
                    token: Some(token.clone()),
                    ..wish_mcp::http::HttpOptions::default()
                },
            )
        });
        match started {
            Ok(endpoint) => {
                self.cockpit.mcp = Some((endpoint, token));
                Ok(())
            }
            Err(error) => {
                self.status = tf("the agent cannot connect: {error}", &[("error", &error)]);
                Err(())
            }
        }
    }

    /// Where an outside agent connects to this editor: its endpoint's
    /// address and token (once the endpoint is started).
    pub fn mcp_endpoint(&self) -> Option<(String, String)> {
        self.cockpit
            .mcp
            .as_ref()
            .map(|(endpoint, token)| (format!("http://{}/mcp", endpoint.address()), token.clone()))
    }

    /// The window that tells how an outside agent connects to this editor.
    /// Asks before an earlier export is replaced.
    fn export_window(&self, ui: &mut Ui, actions: &mut Actions) {
        let Some(folder) = &self.export_confirm else {
            return;
        };
        egui::Window::new(t("Replace the earlier export?"))
            .collapsible(false)
            .resizable(false)
            .default_pos([420.0, 200.0])
            .show(ui.ctx(), |ui| {
                ui.label(tf(
                    "{folder} is there already. Replace it with the game as it is now? Files you added to that folder are replaced too.",
                    &[("folder", folder)],
                ));
                ui.horizontal(|ui| {
                    if ui.button(t("Replace")).clicked() {
                        actions.export_replace = true;
                    }
                    if ui.button(t("Cancel")).clicked() {
                        actions.export_cancel = true;
                    }
                });
            });
    }

    fn connect_window(&self, ui: &mut Ui, actions: &mut Actions) {
        if !self.connect_open {
            return;
        }
        let Some((url, token)) = self.mcp_endpoint() else {
            return;
        };
        let mut open = true;
        egui::Window::new(t("Connect an outside agent"))
            .open(&mut open)
            .default_width(560.0)
            .show(ui.ctx(), |ui| {
                ui.label(t(
                    "An agent connected here works in this editor, in its one history and activity feed, at the same time as the agent panel and you. While the editor is open:",
                ));
                let mut command = format!(
                    "claude mcp add --transport http wish-editor {url} --header \"Authorization: Bearer {token}\""
                );
                ui.add(egui::TextEdit::multiline(&mut command).desired_rows(2).desired_width(f32::INFINITY));
                ui.weak(tf("Address: {url}", &[("url", &url)]));
                let mut shown = token.clone();
                ui.horizontal(|ui| {
                    ui.weak(t("Token:"));
                    ui.add(egui::TextEdit::singleline(&mut shown).desired_width(300.0));
                });
                ui.weak(t(
                    "Only programs on this computer can connect, and only with the token. A new token is made each time the editor starts.",
                ));
            });
        if !open {
            actions.connect = Some(false);
        }
    }

    /// Starts the agent on the request written in the panel.
    fn send_task(&mut self) {
        let request = self.cockpit.request.borrow().trim().to_owned();
        if request.is_empty() {
            return;
        }
        if self.start_task(request) {
            self.cockpit.request.borrow_mut().clear();
        }
    }

    /// Starts the agent on `request`, with what was pointed at, the game's
    /// design and the person's AI settings; a checkpoint is saved first
    /// (unless the person turned that off). Returns whether it started.
    fn start_task(&mut self, request: String) -> bool {
        if self.cockpit.run.is_some() {
            return false;
        }
        // Before every task: a way back.
        let mut before = None;
        if self.cockpit.settings.checkpoint_first {
            let short: String = request.chars().take(40).collect();
            let saved = self.call(
                "checkpoint",
                &json!({ "action": "save", "note": tf("before: {request}", &[("request", &short)]) }),
            );
            before = saved["checkpoint"]["name"].as_str().map(str::to_owned);
            self.read_checkpoints();
        }
        if self.ensure_endpoint().is_err() {
            return false;
        }
        let Some((endpoint, token)) = &self.cockpit.mcp else {
            return false;
        };
        let endpoint = format!("http://{}/mcp", endpoint.address());
        let token = token.clone();
        self.read_design();
        let settings = self.cockpit.settings.clone();
        let task = wish_agent::Task {
            context: self.cockpit.context.clone(),
            design: wish_control::design_summary(&self.design),
            model: settings.model.clone(),
            resume: if settings.memory {
                self.cockpit.session.clone()
            } else {
                None
            },
            files: settings.files,
            ..wish_agent::Task::new(
                request.clone(),
                endpoint,
                token,
                self.workspace.root().to_path_buf(),
                self.scene.clone(),
            )
        };
        self.cockpit.last_instructions = Some(wish_agent::instructions(&task));
        if let Some(name) = before {
            self.cockpit
                .before
                .insert(self.cockpit.transcript.len(), name);
        }
        self.cockpit.transcript.push(crate::cockpit::Line {
            who: crate::cockpit::Who::Person,
            text: request,
            ok: None,
        });
        let started = self.cockpit.agent.start(&task);
        self.cockpit.last_task = Some(task);
        match started {
            Ok(run) => {
                self.cockpit.run = Some(run);
                self.cockpit.context.clear();
                self.status = t("the agent is working").to_owned();
                true
            }
            Err(problem) => {
                self.cockpit.transcript.push(crate::cockpit::Line {
                    who: crate::cockpit::Who::Note,
                    text: problem,
                    ok: Some(false),
                });
                false
            }
        }
    }

    /// Reads the permission mode and the agents' proposals.
    fn read_proposals(&mut self) {
        let got = self.call("proposals", &json!({}));
        if got["ok"] == true {
            self.permission_mode = got["mode"].as_str().unwrap_or("apply").to_owned();
            self.proposals = got["proposals"].as_array().cloned().unwrap_or_default();
        }
    }

    /// The permission mode (`suggest`, `apply`, `autonomous`).
    pub fn permission_mode(&self) -> &str {
        &self.permission_mode
    }

    /// The agents' proposals waiting for a person.
    pub fn waiting_proposals(&self) -> Vec<Value> {
        self.proposals
            .iter()
            .filter(|p| p["status"] == "waiting")
            .cloned()
            .collect()
    }

    /// The activity feed's entries, oldest first.
    pub fn activity(&self) -> Vec<Value> {
        self.activity.iter().cloned().collect()
    }

    /// Runs `check` for the problems tab.
    fn run_check(&mut self) {
        let checked = self.call("check", &json!({}));
        self.found = checked["diagnostics"]
            .as_array()
            .cloned()
            .unwrap_or_default();
    }

    /// The console's entries, oldest first.
    pub fn console(&self) -> Vec<Value> {
        self.log.iter().cloned().collect()
    }

    /// The problems `check` found last.
    pub fn problems(&self) -> &[Value] {
        &self.found
    }

    /// Starts the game from the scene being edited.
    fn start_play(&mut self) {
        if self.scene.ends_with(".prefab.json") {
            self.status = t("a prefab cannot be played; open a scene").to_owned();
            return;
        }
        let started = self.call("sim_start", &json!({ "scene": self.scene.clone() }));
        let Some(session) = started["session"].as_str().map(str::to_owned) else {
            let message = started["error"]["message"]
                .as_str()
                .unwrap_or(t("it did not start"));
            let hint = started["error"]["hint"].as_str().unwrap_or("");
            self.status = tf(
                "play failed: {message} {hint}",
                &[("message", &message), ("hint", &hint)],
            );
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
            recording: false,
            made: 0,
        });
        if self.record {
            let recorded = self.call(
                "replay",
                &json!({ "action": "record", "session": started["session"].clone() }),
            );
            if let Some(play) = self.play.as_mut() {
                play.recording = recorded["ok"] == true;
            }
        }
        self.snapshot();
        self.status = t("playing").to_owned();
        self.inspected_for = (Vec::new(), u64::MAX);
        self.picture_stale = true;
    }

    /// Stops the game: the session is closed and the scene is shown again
    /// (live changes are gone with it).
    fn stop_play(&mut self) {
        if let Some(play) = self.play.take() {
            let saved = if play.recording {
                let name = self.free_replay_name();
                let saved = self.call(
                    "replay",
                    &json!({ "action": "save", "session": play.session.clone(), "name": name.clone() }),
                );
                Some(if saved["ok"] == true {
                    tf("replay {name} saved", &[("name", &name)])
                } else {
                    let message = saved["error"]["message"]
                        .as_str()
                        .unwrap_or(t("it did not work"));
                    tf("replay not saved: {message}", &[("message", &message)])
                })
            } else {
                None
            };
            self.call("sim_stop", &json!({ "session": play.session }));
            self.status = t("stopped (live changes were not kept)").to_owned();
            // Entities made while playing are gone with the game.
            self.revision = u64::MAX;
            self.refresh_scene();
            if let Some(saved) = saved {
                self.status = format!("{}; {saved}", self.status);
            }
            self.inspected_for = (Vec::new(), u64::MAX);
            self.target_for = (Vec::new(), u64::MAX);
            self.picture_stale = true;
        }
    }

    /// The project's saved replays (their names).
    pub fn replays(&self) -> Vec<String> {
        let folder = self.workspace.root().join("replays");
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()?
                    .strip_suffix(".replay.jsonl")
                    .map(str::to_owned)
            })
            .collect();
        names.sort();
        names
    }

    /// A replay name not used yet: `take_1`, `take_2`...
    fn free_replay_name(&self) -> String {
        let used = self.replays();
        (1..10_000)
            .map(|n| format!("take_{n}"))
            .find(|name| !used.contains(name))
            .unwrap_or_else(|| "take".to_owned())
    }

    /// Plays a saved replay back and says whether the game still plays the
    /// same.
    fn run_replay(&mut self, name: &str) {
        let ran = self.call("replay", &json!({ "action": "run", "name": name }));
        if let Some(session) = ran["session"].as_str() {
            self.call("sim_stop", &json!({ "session": session }));
        }
        self.status = if ran["ok"] != true {
            let message = ran["error"]["message"]
                .as_str()
                .unwrap_or(t("it did not work"));
            tf(
                "replay {name} did not run: {message}",
                &[("name", &name), ("message", &message)],
            )
        } else if ran["passed"] == true {
            tf(
                "replay {name}: the game plays the same ({ticks} ticks)",
                &[
                    ("name", &name),
                    ("ticks", &ran["ticks_run"].as_u64().unwrap_or(0)),
                ],
            )
        } else {
            tf(
                "replay {name}: the game plays differently from tick {tick}",
                &[
                    ("name", &name),
                    ("tick", &ran["diverged_at"].as_u64().unwrap_or(0)),
                ],
            )
        };
    }

    /// Reads the project's checkpoints.
    fn read_checkpoints(&mut self) {
        let got = self.call("checkpoint", &json!({ "action": "list" }));
        self.checkpoints = got["checkpoints"].as_array().cloned().unwrap_or_default();
    }

    /// The checkpoints' names, oldest first.
    pub fn checkpoint_names(&self) -> Vec<String> {
        self.checkpoints
            .iter()
            .filter_map(|c| c["name"].as_str().map(str::to_owned))
            .collect()
    }

    /// Whether the before/after window is open, and on which checkpoint.
    pub fn comparing(&self) -> Option<&str> {
        self.compare.as_ref().map(|c| c.name.as_str())
    }

    /// The checkpoint actions of the project menu and the compare window.
    fn checkpoint_act(&mut self, actions: &Actions) {
        if actions.close_compare {
            self.compare = None;
        }
        if actions.save_checkpoint {
            let saved = self.call(
                "checkpoint",
                &json!({ "action": "save", "note": t("saved in the editor") }),
            );
            self.status = if saved["ok"] == true {
                let name = saved["checkpoint"]["name"].as_str().unwrap_or("?");
                tf("checkpoint {name} saved", &[("name", &name)])
            } else {
                let message = saved["error"]["message"]
                    .as_str()
                    .unwrap_or(t("it did not work"));
                tf("checkpoint not saved: {message}", &[("message", &message)])
            };
            self.read_checkpoints();
        }
        if let Some(name) = &actions.restore_checkpoint
            && self.play.is_none()
        {
            let back = self.call("checkpoint", &json!({ "action": "restore", "name": name }));
            self.status = if back["ok"] == true {
                tf(
                    "back to checkpoint {name} (Undo brings now back)",
                    &[("name", name)],
                )
            } else {
                let message = back["error"]["message"]
                    .as_str()
                    .unwrap_or(t("it did not work"));
                tf("could not go back: {message}", &[("message", &message)])
            };
            self.compare = None;
            self.revision = u64::MAX;
            self.refresh_scene();
            self.refresh_project();
            self.inspected_for = (Vec::new(), u64::MAX);
            self.picture_stale = true;
        }
        if let Some(name) = &actions.compare_checkpoint {
            let scene = self.scene.clone();
            let picture = |app: &mut Self, source: PictureSource<'_>| {
                app.workspace
                    .render_picture(source, None, 480, 270, View::Lit)
                    .map(|picture| {
                        egui::ColorImage::from_rgba_unmultiplied(
                            [picture.image.width as usize, picture.image.height as usize],
                            &picture.image.pixels,
                        )
                    })
            };
            let before = picture(self, PictureSource::Checkpoint(Some(&scene), name));
            let now = picture(self, PictureSource::Scene(Some(&scene)));
            match (before, now) {
                (Ok(before), Ok(now)) => {
                    self.compare = Some(Compare {
                        name: name.clone(),
                        before,
                        now,
                        textures: RefCell::new(None),
                    });
                }
                (Err(problem), _) | (_, Err(problem)) => {
                    self.status = tf(
                        "cannot compare: {message}",
                        &[("message", &problem.message)],
                    );
                }
            }
        }
        if let Some(name) = &actions.delete_checkpoint {
            let gone = self.call("checkpoint", &json!({ "action": "delete", "name": name }));
            if gone["ok"] == true {
                self.status = tf("checkpoint {name} deleted", &[("name", name)]);
            }
            if self.compare.as_ref().is_some_and(|c| c.name == *name) {
                self.compare = None;
            }
            self.read_checkpoints();
        }
    }

    /// The before/after window.
    fn compare_window(&self, ui: &mut Ui, actions: &mut Actions) {
        let Some(compare) = &self.compare else {
            return;
        };
        let mut open = true;
        egui::Window::new(tf("Before ({name}) and now", &[("name", &compare.name)]))
            .open(&mut open)
            .default_pos([240.0, 80.0])
            .show(ui.ctx(), |ui| {
                let mut textures = compare.textures.borrow_mut();
                let (before, now) = textures.get_or_insert_with(|| {
                    (
                        ui.ctx().load_texture(
                            format!("before:{}", compare.name),
                            compare.before.clone(),
                            egui::TextureOptions::LINEAR,
                        ),
                        ui.ctx().load_texture(
                            format!("now:{}", compare.name),
                            compare.now.clone(),
                            egui::TextureOptions::LINEAR,
                        ),
                    )
                });
                ui.horizontal(|ui| {
                    for (label, texture) in [(t("Before"), &*before), (t("Now"), &*now)] {
                        ui.vertical(|ui| {
                            ui.strong(label);
                            ui.image((texture.id(), vec2(360.0, 202.5)));
                        });
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .button(t("Go back to this"))
                        .on_hover_text(t(
                            "Brings the scenes back as they were; one change in the history, so Undo brings now back",
                        ))
                        .clicked()
                    {
                        actions.restore_checkpoint = Some(compare.name.clone());
                    }
                    if ui.button(t("Keep now")).clicked() {
                        actions.close_compare = true;
                    }
                });
            });
        if !open {
            actions.close_compare = true;
        }
    }

    /// Runs the project's scenario tests (keeping their pictures as the
    /// golden ones if `update`); failures go to the problems tab.
    fn run_tests(&mut self, update: bool) {
        let ran = self.call("test_run", &json!({ "update_golden": update }));
        if ran["ok"] != true {
            let message = ran["error"]["message"]
                .as_str()
                .unwrap_or(t("it did not work"));
            self.status = tf("tests did not run: {message}", &[("message", &message)]);
            return;
        }
        let results = ran["results"].as_array().cloned().unwrap_or_default();
        let failed: Vec<Value> = results
            .iter()
            .filter(|result| result["passed"] != true)
            .map(|result| {
                let failure = &result["failure"];
                json!({
                    "code": "test_failed",
                    "severity": "error",
                    "message": format!("{}: {}", result["file"].as_str().unwrap_or("?"),
                        failure["message"].as_str().unwrap_or("failed")),
                    "hint": failure["hint"].as_str().unwrap_or(""),
                })
            })
            .collect();
        self.tests = Some((results.len() - failed.len(), failed.len()));
        self.status = tf(
            "tests: {passed} passed, {failed} failed",
            &[
                ("passed", &(results.len() - failed.len())),
                ("failed", &failed.len()),
            ],
        );
        if update {
            self.status = format!("{}; {}", self.status, t("golden pictures updated"));
        }
        if !failed.is_empty() {
            self.found = failed;
            self.tab = Tab::Problems;
        }
    }

    /// Saves the view's picture as a file of the project (`captures/`).
    fn save_picture(&mut self) {
        let stem = self
            .scene
            .rsplit('/')
            .next()
            .unwrap_or("scene")
            .split('.')
            .next()
            .unwrap_or("scene")
            .to_owned();
        let out = (1..10_000)
            .map(|n| format!("captures/{stem}_{n}.png"))
            .find(|path| !self.workspace.root().join(path).exists())
            .unwrap_or_else(|| format!("captures/{stem}.png"));
        let args = match &self.play {
            Some(play) => json!({ "session": play.session.clone(), "out": out.clone() }),
            None => json!({ "scene": self.scene.clone(), "out": out.clone() }),
        };
        let saved = self.call("capture_frame", &args);
        self.status = if saved["ok"] == true {
            tf("picture saved as {path}", &[("path", &out)])
        } else {
            let message = saved["error"]["message"]
                .as_str()
                .unwrap_or(t("it did not work"));
            tf("picture not saved: {message}", &[("message", &message)])
        };
    }

    /// Saves a timeline snapshot of the running game (while there is room).
    fn snapshot(&mut self) {
        let Some(play) = self.play.as_ref() else {
            return;
        };
        if play.snapshots.len() >= MAX_TIMELINE {
            return;
        }
        let label = format!("t{}_{}", play.tick, play.made);
        let session = play.session.clone();
        let saved = self.call(
            "sim_snapshot",
            &json!({ "session": session, "action": "save", "label": label.clone() }),
        );
        if let Some(play) = self.play.as_mut()
            && saved["ok"] == true
        {
            play.snapshots.push((label, play.tick));
            play.made += 1;
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
        let started = std::time::Instant::now();
        let stepped = self.workspace.play_step(&session, ticks, &held, pointer);
        self.game_time.0 += started.elapsed().as_secs_f32() * 1000.0;
        self.game_time.1 += u32::try_from(ticks).unwrap_or(u32::MAX);
        match stepped {
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
                // What came after belongs to the run left behind.
                play.snapshots.truncate(index + 1);
                play.owed = 0.0;
                play.since_snapshot = 0.0;
            }
            self.status = tf("back at tick {tick}", &[("tick", &tick)]);
        } else {
            let message = restored["error"]["message"]
                .as_str()
                .unwrap_or(t("it did not work"));
            self.status = tf("rewind failed: {message}", &[("message", &message)]);
        }
        self.inspected_for = (Vec::new(), u64::MAX);
        self.picture_stale = true;
    }

    /// A scene id made from a file's name.
    fn id_from_path(&self, path: &str) -> String {
        let name = path.rsplit('/').next().unwrap_or(path);
        let stem = name.split('.').next().unwrap_or(name);
        // A scene id: [a-z][a-z0-9_]*, at most 64 characters (room left for
        // the `_N` that makes it unique).
        let mut clean: String = stem
            .chars()
            .map(|c| {
                let c = c.to_ascii_lowercase();
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .take(56)
            .collect();
        if !clean.starts_with(|c: char| c.is_ascii_lowercase()) {
            clean = format!("e_{clean}");
            clean.truncate(56);
        }
        self.unused_id(&clean)
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
                self.status = tf(
                    "{path} cannot be put in a scene (a prefab or a model can)",
                    &[("path", &path)],
                );
                return;
            }
        };
        if self.scene.ends_with(".prefab.json") && Asset::kind_of(path) == "prefab" {
            self.status = t("a prefab cannot hold another prefab").to_owned();
            return;
        }
        if self.edit(
            &tf("added {id} ({path})", &[("id", &id), ("path", &path)]),
            vec![op],
        ) {
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
            self.status = t("make prefab failed: the entity could not be read").to_owned();
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
            self.status = t("make prefab failed: it already contains prefab instances").to_owned();
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
                .unwrap_or(t("it did not work"));
            let hint = made["error"]["hint"].as_str().unwrap_or("");
            self.status = tf(
                "make prefab failed: {message} {hint}",
                &[("message", &message), ("hint", &hint)],
            );
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
            &tf("made prefab {path}", &[("path", &path)]),
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
    fn act(&mut self, mut actions: Actions) {
        // The history tab's buttons are the menu bar's.
        actions.undo |= actions.panels.undo;
        actions.redo |= actions.panels.redo;
        // The AI mode's conversation, task cards and health are the
        // cockpit's, the checkpoints' and the project menu's.
        actions.cockpit.send |= actions.ai.cockpit.send;
        actions.cockpit.stop |= actions.ai.cockpit.stop;
        if actions.cockpit.remove_context.is_none() {
            actions.cockpit.remove_context = actions.ai.cockpit.remove_context.take();
        }
        if let Some(name) = actions.ai.undo_task.take() {
            actions.restore_checkpoint = Some(name);
        }
        if let Some(name) = actions.ai.compare_task.take() {
            actions.compare_checkpoint = Some(name);
        }
        actions.run_tests |= std::mem::take(&mut actions.ai.run_tests);
        actions.build_code |= std::mem::take(&mut actions.ai.build);
        // Undo, redo and another scene are about the files: not while a
        // game runs (its changes are live and go with it).
        if self.play.is_some() {
            actions.undo = false;
            actions.redo = false;
            actions.scene = None;
        }
        if actions.quit {
            self.exit = true;
        }
        if actions.show_problems {
            self.run_check();
            self.tab = Tab::Problems;
        }
        if actions.run_tests {
            self.run_tests(false);
        }
        self.checkpoint_act(&actions);
        if actions.export_cancel {
            self.export_confirm = None;
            self.status = t("the earlier export was kept").to_owned();
        }
        if actions.export || actions.export_replace {
            let replace = actions.export_replace && self.export_confirm.take().is_some();
            let exported = self.call("project_export", &json!({ "overwrite": replace }));
            self.status = if exported["ok"] == true {
                let mut status = tf(
                    "game exported: {program} (zip its folder to share it)",
                    &[("program", &exported["program"].as_str().unwrap_or("?"))],
                );
                // A warning (such as the C runtime the player needs) is
                // said with it.
                for warning in exported["diagnostics"].as_array().into_iter().flatten() {
                    status.push(' ');
                    status.push_str(warning["message"].as_str().unwrap_or_default());
                }
                status
            } else if exported["error"]["code"] == "export_exists" {
                self.export_confirm = Some(
                    exported["error"]["message"]
                        .as_str()
                        .and_then(|message| message.split('\'').nth(1))
                        .unwrap_or("exports/")
                        .to_owned(),
                );
                t("an earlier export is there: replace it?").to_owned()
            } else {
                let message = exported["error"]["message"]
                    .as_str()
                    .unwrap_or(t("it did not work"));
                let hint = exported["error"]["hint"].as_str().unwrap_or("");
                tf(
                    "not exported: {message} {hint}",
                    &[("message", &message), ("hint", &hint)],
                )
            };
        }
        if actions.toggle_cockpit {
            self.cockpit.open = !self.cockpit.open;
        }
        if actions.toggle_game_view {
            self.game_view = !self.game_view;
            self.game_drawn = None;
            self.picture_stale = true;
            self.save_preferences();
        }
        match actions.connect {
            Some(true) => {
                if self.ensure_endpoint().is_ok() {
                    self.connect_open = true;
                }
            }
            Some(false) => self.connect_open = false,
            None => {}
        }
        self.cockpit_act(&actions.cockpit);
        if let Some(on) = actions.switch_mode {
            self.set_ai_mode(on);
        }
        let ai = std::mem::take(&mut actions.ai);
        if ai.any() {
            self.ai_act(ai);
        }
        if let Some(mode) = actions.permission {
            let set = self.call("permissions", &json!({ "action": "set", "mode": mode }));
            if set["ok"] == true {
                self.status = tf("agents' permission: {mode}", &[("mode", &mode_label(mode))]);
            }
            self.read_proposals();
        }
        if let Some((id, accept)) = actions.panels.decide {
            let action = if accept { "accept" } else { "reject" };
            let done = self.call("proposals", &json!({ "action": action, "id": id }));
            self.status = if done["ok"] != true {
                let message = done["error"]["message"]
                    .as_str()
                    .unwrap_or(t("it did not work"));
                tf(
                    "proposal {id}: {message}",
                    &[("id", &id), ("message", &message)],
                )
            } else if !accept {
                tf("proposal {id} rejected", &[("id", &id)])
            } else if done["answer"]["ok"] == true {
                tf("proposal {id} accepted and made", &[("id", &id)])
            } else {
                let message = done["answer"]["error"]["message"]
                    .as_str()
                    .unwrap_or(t("it did not work"));
                tf(
                    "proposal {id} accepted but failed: {message}",
                    &[("id", &id), ("message", &message)],
                )
            };
            self.read_proposals();
            self.revision = u64::MAX;
            self.refresh_scene();
            self.refresh_project();
            self.inspected_for = (Vec::new(), u64::MAX);
            self.picture_stale = true;
        }
        if actions.update_golden {
            self.run_tests(true);
        }
        if let Some(name) = actions.replay {
            self.run_replay(&name);
        }
        if actions.save_picture {
            self.save_picture();
        }
        if actions.toggle_record {
            self.record = !self.record;
        }
        if let Some((path, mut args)) = actions.browser.create.take() {
            args["path"] = json!(path);
            let made = self.call("asset_create", &args);
            self.status = if made["ok"] == true {
                tf("made {path}", &[("path", &path)])
            } else {
                let message = made["error"]["message"]
                    .as_str()
                    .unwrap_or(t("it did not work"));
                tf(
                    "could not make {path}: {message}",
                    &[("path", &path), ("message", &message)],
                )
            };
            self.refresh_project();
            self.load_assets();
            self.select_asset(&path);
        }
        if let Some(code) = actions.language {
            self.set_language(code);
            self.save_preferences();
        }
        if let Some(scene) = actions.scene
            && scene != self.scene
        {
            self.scene = scene;
            self.revision = u64::MAX;
            self.selected.clear();
            self.inspected_for = (Vec::new(), u64::MAX);
            self.target_for = (Vec::new(), u64::MAX);
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
        if let Some(path) = actions.browser.open {
            self.open_file(&path);
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
                tf(
                    "moved {from} to {to} ({updated} file(s) that use it updated)",
                    &[("from", &from), ("to", &to), ("updated", &updated)],
                )
            } else {
                let message = moved["error"]["message"]
                    .as_str()
                    .unwrap_or(t("it did not work"));
                let hint = moved["error"]["hint"].as_str().unwrap_or("");
                tf(
                    "move failed: {message} {hint}",
                    &[("message", &message), ("hint", &hint)],
                )
            };
            if moved["ok"] == true && self.asset_selected.as_deref() == Some(from.as_str()) {
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
        if let Some(tab) = actions.panels.tab {
            self.tab = tab;
            if tab == Tab::Problems {
                self.run_check();
            }
            if tab == Tab::Console {
                self.read_log();
            }
            if tab == Tab::Activity {
                self.read_activity();
                self.read_proposals();
            }
        }
        let revert = actions
            .panels
            .revert
            .map(|revision| (self.scene.clone(), revision))
            .or(actions.panels.revert_in.clone());
        if let Some((scene, revision)) = revert
            && self.play.is_none()
        {
            let done = self.call(
                "edit_history",
                &json!({ "scene": scene, "action": "revert", "revision": revision }),
            );
            self.status = if done["ok"] == true {
                tf("took back r{revision}", &[("revision", &revision)])
            } else {
                let message = done["error"]["message"]
                    .as_str()
                    .unwrap_or(t("it did not work"));
                let hint = done["error"]["hint"].as_str().unwrap_or("");
                tf(
                    "could not take back r{revision}: {message} {hint}",
                    &[
                        ("revision", &revision),
                        ("message", &message),
                        ("hint", &hint),
                    ],
                )
            };
            self.revision = u64::MAX;
            self.refresh_scene();
            self.inspected_for = (Vec::new(), u64::MAX);
            self.picture_stale = true;
        }
        if actions.panels.clear_console {
            self.log.clear();
        }
        if actions.panels.check {
            self.run_check();
        }
        if let Some((file, id)) = actions.panels.select {
            let document = file.ends_with(".scene.json") || file.ends_with(".prefab.json");
            if document && file != self.scene && self.play.is_none() {
                self.scene.clone_from(&file);
                self.revision = u64::MAX;
                self.selected.clear();
                self.inspected_for = (Vec::new(), u64::MAX);
                self.target_for = (Vec::new(), u64::MAX);
                *self.eye.borrow_mut() = None;
                self.refresh_scene();
            }
            if file.is_empty() || file == self.scene {
                self.click_entity(&id, false);
            }
        }
        if actions.edit_code {
            self.open_file("scripts/src/lib.rs");
        }
        if actions.build_code {
            let built = self.call("script_build", &json!({}));
            // A build that could not even start answers with an error.
            let problems = if built["ok"] == true {
                built["diagnostics"].as_array().cloned().unwrap_or_default()
            } else {
                vec![built["error"].clone()]
            };
            self.status = if built["passed"] == true {
                let warnings = built["warnings"].as_u64().unwrap_or(0);
                tf(
                    "game code built ({warnings} warning(s))",
                    &[("warnings", &warnings)],
                )
            } else {
                let first = problems
                    .first()
                    .and_then(|p| p["message"].as_str())
                    .unwrap_or(t("it did not build"));
                tf("game code not built: {first}", &[("first", &first)])
            };
            if built["passed"] != true {
                self.found = problems;
                self.tab = Tab::Problems;
            }
            self.refresh_project();
        }
        if actions.open_settings {
            let got = self.call("project_settings", &json!({}));
            if got["ok"] == true {
                *self.settings.borrow_mut() = Some(Draft::from_answer(&got));
            } else {
                self.status = got["error"]["message"]
                    .as_str()
                    .unwrap_or(t("the settings could not be read"))
                    .to_owned();
            }
        }
        if actions.settings.close {
            *self.settings.borrow_mut() = None;
        }
        let draft = self.settings.borrow().clone();
        if let Some(draft) = draft {
            let mut args = None;
            if actions.settings.save_project {
                args = Some(json!({ "action": "set", "name": draft.name, "scene": draft.scene }));
            } else if actions.settings.save_input {
                args = Some(json!({ "action": "set", "input": draft.input() }));
            }
            if let Some(args) = args {
                let saved = self.call("project_settings", &args);
                self.status = if saved["ok"] == true {
                    let what = saved["changed"]
                        .as_array()
                        .map(|c| {
                            c.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    tf("settings saved ({what})", &[("what", &what)])
                } else {
                    let message = saved["error"]["message"]
                        .as_str()
                        .unwrap_or(t("it did not work"));
                    let hint = saved["error"]["hint"].as_str().unwrap_or("");
                    tf(
                        "settings not saved: {message} {hint}",
                        &[("message", &message), ("hint", &hint)],
                    )
                };
                self.refresh_project();
            }
        }
        if actions.play_start && self.play.is_none() {
            self.start_play();
        }
        if actions.play_pause
            && let Some(play) = self.play.as_mut()
        {
            play.paused = !play.paused;
            play.owed = 0.0;
            self.status = if play.paused {
                t("paused")
            } else {
                t("playing")
            }
            .to_owned();
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
            if self.edit(&tf("added {id}", &[("id", &id)]), vec![op]) {
                self.selected = vec![id];
            }
        }
        // A part of a prefab instance is the prefab's: not deleted here.
        let doomed: Vec<String> = self
            .selected
            .iter()
            .filter(|id| !id.contains('/'))
            .cloned()
            .collect();
        if actions.delete && !doomed.is_empty() {
            let ops = doomed
                .iter()
                .map(|id| json!({ "op": "despawn", "id": id }))
                .collect();
            let what = if doomed.len() == 1 {
                tf("deleted {id}", &[("id", &doomed[0])])
            } else {
                tf("deleted {count} entities", &[("count", &doomed.len())])
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
                &tf("renamed {from} to {to}", &[("from", &from), ("to", &to)]),
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
                Some(parent) => tf(
                    "moved {id} under {parent}",
                    &[("id", &id), ("parent", &parent)],
                ),
                None => tf("moved {id} to the root", &[("id", &id)]),
            };
            self.edit(
                &what,
                vec![json!({ "op": "reparent", "id": id, "parent": parent })],
            );
        }
        for (wanted, action, done_text) in [
            (actions.undo, "undo", t("undo: done")),
            (actions.redo, "redo", t("redo: done")),
        ] {
            if wanted {
                let done = self.call(
                    "edit_history",
                    &json!({ "scene": self.scene.clone(), "action": action }),
                );
                self.status = if done["ok"] == true {
                    done_text.to_owned()
                } else {
                    done["error"]["message"]
                        .as_str()
                        .unwrap_or(t("it did not work"))
                        .to_owned()
                };
                self.revision = u64::MAX;
                self.refresh_scene();
            }
        }
        if actions.check {
            let checked = self.call("check", &json!({}));
            self.found = checked["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let found = self.found.len();
            self.status = if checked["passed"] == true {
                t("check: no problems").to_owned()
            } else {
                tf("check: {found} problem(s)", &[("found", &found)])
            };
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
    /// Draws the game view's picture when what it shows has changed: the
    /// scene (or the running game) through the scene's own camera, with the
    /// game's interface, as the player sees it.
    fn render_game(&mut self) {
        let area = self.game_viewport;
        if area == Rect::NOTHING || area.width() < MIN_VIEWPORT || area.height() < MIN_VIEWPORT {
            self.game_picture = None;
            self.game_drawn = None;
            return;
        }
        let (width, height) = (area.width() as u32, area.height() as u32);
        let preview = self.drag.as_ref().and_then(|active| active.ops.clone());
        let key = (
            self.revision,
            self.play.as_ref().map(|play| play.tick),
            width,
            height,
        );
        if preview.is_none() && self.game_drawn == Some(key) && self.game_picture.is_some() {
            return;
        }
        let scene = self.scene.clone();
        let session = self.play.as_ref().map(|play| play.session.clone());
        let source = match (&session, &preview) {
            (Some(id), _) => PictureSource::Session(id),
            (None, Some(ops)) => PictureSource::Preview(Some(&scene), ops),
            (None, None) => PictureSource::Scene(Some(&scene)),
        };
        self.game_picture = self
            .workspace
            .render_picture(source, None, width, height, View::Lit)
            .ok()
            .map(|picture| picture.image);
        self.game_drawn = Some(key);
    }

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
            // A running game is seen through its own camera, unless the game
            // view beside shows that: then this view keeps the editor's eye.
            let split = self.game_viewport != Rect::NOTHING;
            let eye_camera: Option<&dyn Fn(&CameraView) -> CameraView> =
                if session.is_some() && !split {
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
        let split = self.game_viewport != Rect::NOTHING;
        let ids = match &session {
            Some(id) => self.workspace.render_ids(
                PictureSource::Session(id),
                if split { Some(&camera) } else { None },
                width,
                height,
            ),
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
            if self.cockpit.pointing || self.ai_mode {
                // Show and tell: what was clicked goes with the request.
                self.cockpit.pointing = false;
                if let Some(name) = hit {
                    let line = self.describe_for_agent(&name);
                    if !self.cockpit.context.contains(&line) {
                        self.cockpit.context.push(line);
                    }
                }
            } else {
                match hit {
                    Some(name) => self.click_entity(&name, toggle),
                    None if !toggle => self.selected.clear(),
                    None => {}
                }
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
            crate::style::panel_title(ui, t("Hierarchy"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                use crate::style::{Icon, tool_button};
                if tool_button(
                    ui,
                    Icon::Trash,
                    t("Delete"),
                    false,
                    false,
                    !self.selected.is_empty(),
                    None,
                )
                .on_hover_text(t("Delete"))
                .clicked()
                {
                    actions.delete = true;
                }
                if tool_button(ui, Icon::Plus, "+", false, false, true, None)
                    .on_hover_text(t("Add an entity"))
                    .clicked()
                {
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
            && let Some(first) = self.selected.first().filter(|id| !id.contains('/'))
        {
            self.ui_state.borrow_mut().rename = Some((first.clone(), first.clone()));
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
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
                    // What kind of thing it is, at a glance.
                    let (icon, color) =
                        crate::style::entity_icon(&row.components, row.prefab.is_some());
                    let (icon_rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    crate::style::paint(ui.painter(), icon_rect, icon, color);
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
                        if ui.button(t("Add child")).clicked() {
                            actions.add = Some(Some(row.id.clone()));
                            ui.close();
                        }
                        if ui.button(t("Rename")).clicked() {
                            self.ui_state.borrow_mut().rename =
                                Some((row.id.clone(), row.id.clone()));
                            ui.close();
                        }
                        if !self.scene.ends_with(".prefab.json")
                            && self.play.is_none()
                            && ui
                                .button(t("Make prefab"))
                                .on_hover_text(t("Saves it and its children as a prefab and puts an instance in its place"))
                                .clicked()
                        {
                            actions.make_prefab = Some(row.id.clone());
                            ui.close();
                        }
                        if ui.button(t("Delete")).clicked() {
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
    fn layout(&self, ui: &mut Ui, viewport: &mut Rect, game_area: &mut Rect) -> Actions {
        let mut actions = Actions::default();
        egui::Panel::top("menu").show(ui, |ui| {
            // Shortcuts, unless the person is typing (a text box has its
            // own undo).
            if !ui.ctx().egui_wants_keyboard_input() {
                let pressed = |ui: &Ui, modifiers: egui::Modifiers, key: egui::Key| {
                    ui.input_mut(|input| {
                        input.consume_shortcut(&egui::KeyboardShortcut::new(modifiers, key))
                    })
                };
                let command = egui::Modifiers::COMMAND;
                if pressed(ui, command | egui::Modifiers::SHIFT, egui::Key::Z)
                    || pressed(ui, command, egui::Key::Y)
                {
                    actions.redo |= self.can_redo && self.play.is_none();
                } else if pressed(ui, command, egui::Key::Z) {
                    actions.undo |= self.can_undo && self.play.is_none();
                }
                if pressed(ui, command, egui::Key::P) {
                    if self.play.is_some() {
                        actions.play_stop = true;
                    } else if !self.scene.ends_with(".prefab.json") {
                        actions.play_start = true;
                    }
                }
            }
            // A narrow window puts the rest of the bar on a second line.
            ui.horizontal_wrapped(|ui| {
                ui.menu_button(self.project.as_str(), |ui| {
                    if ui
                        .button(t("Settings..."))
                        .on_hover_text(t("The project's name, default scene and input map"))
                        .clicked()
                    {
                        actions.open_settings = true;
                        ui.close();
                    }
                    if ui.button(t("Check")).on_hover_text(t("Look for problems in the whole project")).clicked() {
                        actions.check = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(t("Edit code"))
                        .on_hover_text(t("Open scripts/src/lib.rs in your code editor (WISH_EDITOR, or the system's)"))
                        .clicked()
                    {
                        actions.edit_code = true;
                        ui.close();
                    }
                    if ui
                        .button(t("Build code"))
                        .on_hover_text(t("Build the game code; a running game switches to it"))
                        .clicked()
                    {
                        actions.build_code = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(t("Run tests"))
                        .on_hover_text(t("The project's scenario tests (tests/*.test.json)"))
                        .clicked()
                    {
                        actions.run_tests = true;
                        ui.close();
                    }
                    if ui
                        .button(t("Update golden pictures"))
                        .on_hover_text(t(
                            "Run the tests and keep the pictures they take as the golden ones (look at them first)",
                        ))
                        .clicked()
                    {
                        actions.update_golden = true;
                        ui.close();
                    }
                    ui.menu_button(t("Replays"), |ui| {
                        let replays = self.replays();
                        if replays.is_empty() {
                            ui.weak(t("No replays yet: turn on Record, then Play."));
                        }
                        for name in replays {
                            if ui
                                .button(&name)
                                .on_hover_text(t("Play it back: does the game still play the same?"))
                                .clicked()
                            {
                                actions.replay = Some(name);
                                ui.close();
                            }
                        }
                    });
                    if ui
                        .button(t("Save picture"))
                        .on_hover_text(t("The view's picture, as a PNG under captures/"))
                        .clicked()
                    {
                        actions.save_picture = true;
                        ui.close();
                    }
                    ui.menu_button(t("Permissions"), |ui| {
                        for (mode, hover) in [
                            ("suggest", t("An agent's change to the project waits for you in the Activity tab")),
                            ("apply", t("Agents' changes are made; you review them in the Activity tab")),
                            ("autonomous", t("As apply, and the cockpit does not ask between an agent's tasks")),
                        ] {
                            if ui
                                .selectable_label(self.permission_mode == mode, mode_label(mode))
                                .on_hover_text(hover)
                                .clicked()
                            {
                                actions.permission = Some(mode);
                                ui.close();
                            }
                        }
                    });
                    if ui
                        .button(t("Export game"))
                        .on_hover_text(t(
                            "A folder under exports/ that runs on a machine without the engine (no editor, no compiler)",
                        ))
                        .clicked()
                    {
                        actions.export = true;
                        ui.close();
                    }
                    if ui
                        .button(t("Connect an outside agent..."))
                        .on_hover_text(t("Let Claude Code (or any MCP client) work in this editor"))
                        .clicked()
                    {
                        actions.connect = Some(true);
                        ui.close();
                    }
                    ui.menu_button(t("Checkpoints"), |ui| {
                        if ui
                            .button(t("Save checkpoint"))
                            .on_hover_text(t("Keeps the scenes, prefabs and settings as they are now"))
                            .clicked()
                        {
                            actions.save_checkpoint = true;
                            ui.close();
                        }
                        ui.separator();
                        if self.checkpoints.is_empty() {
                            ui.weak(t("No checkpoints yet."));
                        }
                        for checkpoint in self.checkpoints.iter().rev().take(12) {
                            let name = checkpoint["name"].as_str().unwrap_or("?").to_owned();
                            let label = match checkpoint["note"].as_str() {
                                Some(note) => format!("{name}: {note}"),
                                None => name.clone(),
                            };
                            ui.menu_button(label, |ui| {
                                if ui
                                    .add_enabled(self.play.is_none(), egui::Button::new(t("Go back to this")))
                                    .clicked()
                                {
                                    actions.restore_checkpoint = Some(name.clone());
                                    ui.close();
                                }
                                if ui.button(t("Compare with now")).clicked() {
                                    actions.compare_checkpoint = Some(name.clone());
                                    ui.close();
                                }
                                if ui.button(t("Delete")).clicked() {
                                    actions.delete_checkpoint = Some(name.clone());
                                    ui.close();
                                }
                            });
                        }
                    });
                    ui.separator();
                    ui.menu_button(t("Language"), |ui| {
                        for language in crate::i18n::LANGUAGES {
                            if ui
                                .selectable_label(self.language == language.code, language.name)
                                .clicked()
                            {
                                actions.language = Some(language.code);
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if ui.button(t("Quit")).clicked() {
                        actions.quit = true;
                        ui.close();
                    }
                });
                if let Some(on) = crate::ai_mode::switch(ui, self.ai_mode) {
                    actions.switch_mode = Some(on);
                }
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
                            ui.weak(t("Prefabs"));
                        }
                        for prefab in &self.prefabs {
                            if ui.selectable_label(*prefab == self.scene, prefab).clicked() {
                                actions.scene = Some(prefab.clone());
                            }
                        }
                    });
                use crate::style::{GO, Icon, RED, tool_button, tool_group};
                let editing = self.play.is_none();
                tool_group(ui, 64.0, |ui| {
                    if tool_button(ui, Icon::Undo, t("Undo"), false, false, self.can_undo && editing, None)
                        .on_hover_text("Ctrl+Z")
                        .clicked()
                    {
                        actions.undo = true;
                    }
                    if tool_button(ui, Icon::Redo, t("Redo"), false, false, self.can_redo && editing, None)
                        .on_hover_text("Ctrl+Y")
                        .clicked()
                    {
                        actions.redo = true;
                    }
                });
                if !self.ai_mode {
                egui::ComboBox::from_id_salt("view")
                    .selected_text(view_label(self.view.name()))
                    .show_ui(ui, |ui| {
                        for name in View::NAMES {
                            if ui
                                .selectable_label(*name == self.view.name(), view_label(name))
                                .on_hover_text(tf(
                                    "Called '{name}' by capture_frame and wen capture --view",
                                    &[("name", name)],
                                ))
                                .clicked()
                                && let Ok(view) = View::parse(name)
                            {
                                actions.view = Some(view);
                            }
                        }
                    });
                }
                tool_group(ui, 220.0, |ui| match &self.play {
                    None => {
                        if tool_button(
                            ui,
                            Icon::Play,
                            t("Play"),
                            true,
                            false,
                            !self.scene.ends_with(".prefab.json"),
                            Some(GO),
                        )
                        .on_hover_text(t(
                            "Run the game here; changes made while it runs are not kept (Ctrl+P)",
                        ))
                        .clicked()
                        {
                            actions.play_start = true;
                        }
                        if tool_button(ui, Icon::Record, t("Record"), true, self.record, true, Some(RED))
                            .on_hover_text(t(
                                "Record the next run as a replay (saved under replays/ when it stops)",
                            ))
                            .clicked()
                        {
                            actions.toggle_record = true;
                        }
                    }
                    Some(play) => {
                        if play.recording {
                            ui.label(egui::RichText::new(t("REC")).color(RED))
                                .on_hover_text(t("This run is being recorded as a replay"));
                        }
                        let (icon, pause) = if play.paused {
                            (Icon::Play, t("Resume"))
                        } else {
                            (Icon::Pause, t("Pause"))
                        };
                        if tool_button(ui, icon, pause, true, false, true, Some(GO)).clicked() {
                            actions.play_pause = true;
                        }
                        if tool_button(ui, Icon::Step, t("Step"), false, false, play.paused, None)
                            .on_hover_text(t("One tick"))
                            .clicked()
                        {
                            actions.play_step = true;
                        }
                        if tool_button(ui, Icon::Stop, t("Stop"), true, false, true, Some(RED)).clicked() {
                            actions.play_stop = true;
                        }
                        ui.label(
                            egui::RichText::new(tf("tick {tick}", &[("tick", &play.tick)]))
                                .color(crate::style::WEAK),
                        );
                        if play.snapshots.len() > 1 {
                            let pending = self.ui_state.borrow().timeline;
                            let mut cursor = pending.unwrap_or(play.cursor);
                            let response = ui
                                .add(
                                    egui::Slider::new(&mut cursor, 0..=play.snapshots.len() - 1)
                                        .show_value(false),
                                )
                                .on_hover_text(t("Timeline: let go to go back to that moment"));
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
                });
                if self.ai_mode {
                    return;
                }
                tool_group(ui, 250.0, |ui| {
                    for (mode, icon, name) in [
                        (Mode::Translate, Icon::Move, t("Move")),
                        (Mode::Rotate, Icon::Rotate, t("Rotate")),
                        (Mode::Scale, Icon::Scale, t("Scale")),
                    ] {
                        if tool_button(ui, icon, name, false, self.mode == mode, true, None)
                            .on_hover_text(format!("{name} ({})", t("Keys W, E, R")))
                            .clicked()
                        {
                            actions.mode = Some(mode);
                        }
                    }
                    let space = if self.space == Space::World {
                        t("World")
                    } else {
                        t("Local")
                    };
                    if tool_button(ui, Icon::Globe, space, true, false, true, None)
                        .on_hover_text(t("The handles' axes: the world's or the entity's own"))
                        .clicked()
                    {
                        actions.toggle_space = true;
                    }
                    if tool_button(ui, Icon::Magnet, t("Snap"), false, self.snap, true, None)
                        .on_hover_text(t("Half metres, 15 degrees, steps of 10 % when on"))
                        .clicked()
                    {
                        actions.toggle_snap = true;
                    }
                    if tool_button(ui, Icon::Target, t("Reset camera"), false, false, true, None)
                        .on_hover_text(t("Reset camera"))
                        .clicked()
                    {
                        actions.reset_camera = true;
                    }
                });
                tool_group(ui, 40.0, |ui| {
                    if tool_button(ui, Icon::Screen, t("Game view"), false, self.game_view, true, None)
                        .on_hover_text(t(
                            "The game as the player sees it (the scene's camera), beside the scene view",
                        ))
                        .clicked()
                    {
                        actions.toggle_game_view = true;
                    }
                });
                tool_group(ui, 150.0, |ui| {
                    if tool_button(
                        ui,
                        Icon::Chat,
                        t("Agent"),
                        true,
                        self.cockpit.open,
                        true,
                        Some(crate::style::ACCENT),
                    )
                    .on_hover_text(t("The agent's panel: ask, point at things, watch it work"))
                    .clicked()
                    {
                        actions.toggle_cockpit = true;
                    }
                });
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let (errors, warnings) = self.problems;
                // Problems in their colour; none in a quiet one.
                let color = if errors > 0 {
                    crate::style::RED
                } else if warnings > 0 {
                    crate::style::AMBER
                } else {
                    crate::style::WEAK
                };
                ui.label(
                    egui::RichText::new(tf(
                        "{errors} error(s), {warnings} warning(s)",
                        &[("errors", &errors), ("warnings", &warnings)],
                    ))
                    .color(color),
                );
                ui.separator();
                ui.add(egui::Label::new(&self.status).truncate())
                    .on_hover_text(&self.status);
            });
        });
        if self.ai_mode {
            self.ai_layout(ui, viewport, game_area, &mut actions);
            return actions;
        }
        egui::Panel::bottom("assets")
            .resizable(true)
            .default_size(170.0)
            .size_range(110.0..=420.0)
            .show(ui, |ui| {
                crate::panels::tabs(ui, self.tab, self.found.len(), &mut actions.panels);
                ui.separator();
                match self.tab {
                    Tab::Assets => {
                        let view = BrowserView {
                            assets: &self.files,
                            selected: self.asset_selected.as_deref(),
                            detail: self.asset_detail.as_ref(),
                            root: self.workspace.root(),
                            state: &self.browser_state,
                        };
                        crate::browser::panel(ui, &view, &mut actions.browser);
                    }
                    Tab::Console => crate::panels::console(ui, &self.log, &mut actions.panels),
                    Tab::Activity => {
                        let mut agents_only = self.agents_only.get();
                        crate::panels::activity(
                            ui,
                            &self.activity,
                            &self.proposals,
                            &mut agents_only,
                            &mut actions.panels,
                        );
                        self.agents_only.set(agents_only);
                    }
                    Tab::Problems => crate::panels::problems(ui, &self.found, &mut actions.panels),
                    Tab::Performance => {
                        crate::panels::performance(ui, &self.timings, self.play.is_some());
                    }
                    Tab::History => crate::panels::history(
                        ui,
                        &self.history,
                        (self.can_undo, self.can_redo),
                        &mut actions.panels,
                    ),
                }
            });
        egui::Panel::left("hierarchy")
            .default_size(220.0)
            .show(ui, |ui| self.hierarchy(ui, &mut actions));
        egui::Panel::right("inspector")
            .default_size(300.0)
            .show(ui, |ui| {
                let view = InspectorView {
                    inspected: &self.inspected,
                    schemas: if self.inspecting_resources {
                        &self.resource_schemas
                    } else {
                        &self.schemas
                    },
                    resources: self.inspecting_resources,
                    entities: self.rows.iter().map(|row| row.id.as_str()).collect(),
                    assets: &self.assets,
                    copied: self.clipboard.as_ref().map(|(name, _)| name.as_str()),
                    state: &self.inspector_state,
                };
                inspector::panel(ui, &view, &mut actions.inspector);
            });
        if self.cockpit.open {
            egui::Panel::right("cockpit")
                .default_size(320.0)
                .show(ui, |ui| {
                    crate::cockpit::panel(
                        ui,
                        &self.cockpit,
                        mode_label(&self.permission_mode),
                        &mut actions.cockpit,
                    );
                });
        }
        crate::settings::window(ui, &self.settings, &self.scenes, &mut actions.settings);
        self.compare_window(ui, &mut actions);
        self.connect_window(ui, &mut actions);
        self.export_window(ui, &mut actions);
        // The middle is not painted: the scene's picture shows through.
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let full = ui.max_rect();
                // The game view beside the scene view, when it is wanted and
                // the middle is wide enough for both.
                let split = self.game_view && full.width() >= GAME_VIEW_MIN_WIDTH;
                let area = if split {
                    let half = ((full.width() - VIEW_GAP) * 0.5).floor();
                    *game_area = Rect::from_min_max(
                        pos2(full.min.x + half + VIEW_GAP, full.min.y),
                        full.max,
                    );
                    Rect::from_min_size(full.min, vec2(half, full.height()))
                } else {
                    full
                };
                *viewport = area;
                if split {
                    view_badge(ui, area, t("Scene"));
                    view_badge(ui, *game_area, t("Game"));
                }
                // A file dragged from the browser is dropped on the view.
                let target = ui.allocate_rect(area, Sense::hover());
                if let Some(dropped) = target.dnd_release_payload::<AssetDrag>() {
                    let at = ui
                        .input(|input| input.pointer.interact_pos())
                        .unwrap_or_else(|| area.center());
                    actions.drop = Some((dropped.0.clone(), at));
                }
                if let Some((message, hint)) = &self.scene_error {
                    self.scene_problem(ui, area, message, hint, &mut actions);
                }
            });
        actions
    }

    /// The AI mode's panels: the game on the left, the conversation on the
    /// right, the live game with the plan, notes and health in the middle.
    fn ai_layout(
        &self,
        ui: &mut Ui,
        viewport: &mut Rect,
        game_area: &mut Rect,
        actions: &mut Actions,
    ) {
        let waiting = self
            .proposals
            .iter()
            .filter(|p| p["status"] == "waiting")
            .count();
        let view = crate::ai_mode::AiView {
            design: &self.design,
            cockpit: &self.cockpit,
            state: &self.ai,
            problems: self.problems,
            found: &self.found,
            unbuilt: self.unbuilt,
            tests: self.tests,
            tick: self.play.as_ref().map(|play| play.tick),
            permission: &self.permission_mode,
            waiting,
            assistant: self.assistant.as_deref(),
        };
        egui::Panel::left("ai-game")
            .default_size(290.0)
            .show(ui, |ui| {
                crate::ai_mode::game_panel(ui, &view, &mut actions.ai)
            });
        egui::Panel::right("ai-conversation")
            .default_size(370.0)
            .show(ui, |ui| {
                crate::ai_mode::conversation(ui, &view, &mut actions.ai)
            });
        egui::Panel::bottom("ai-board")
            .resizable(true)
            .default_size(230.0)
            .size_range(140.0..=480.0)
            .show(ui, |ui| crate::ai_mode::board(ui, &view, &mut actions.ai));
        crate::ai_mode::settings(ui, &view, &mut actions.ai);
        crate::settings::window(ui, &self.settings, &self.scenes, &mut actions.settings);
        self.compare_window(ui, actions);
        self.connect_window(ui, actions);
        self.export_window(ui, actions);
        let scene_view = self.ai.borrow().scene_view;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let area = ui.max_rect();
                if scene_view {
                    *viewport = area;
                } else {
                    *viewport = Rect::NOTHING;
                    *game_area = area;
                }
                crate::ai_mode::view_switch(ui, area, scene_view, &mut actions.ai);
                if let Some((message, hint)) = &self.scene_error {
                    self.scene_problem(ui, area, message, hint, actions);
                }
            });
    }

    /// A card in the middle of the view: why the scene cannot be shown, and
    /// what to do about it.
    fn scene_problem(
        &self,
        ui: &mut Ui,
        area: Rect,
        message: &str,
        hint: &str,
        actions: &mut Actions,
    ) {
        let card = Rect::from_center_size(
            area.center(),
            vec2((area.width() - 40.0).clamp(120.0, 520.0), 200.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
            egui::Frame::group(ui.style())
                .fill(ui.visuals().panel_fill)
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.heading(t("This scene cannot be shown"));
                    ui.add_space(4.0);
                    ui.label(message);
                    if !hint.is_empty() {
                        ui.weak(hint);
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if self.unbuilt
                            && ui
                                .button(t("Build code"))
                                .on_hover_text(t(
                                    "The scene uses the game code's own types: build it first",
                                ))
                                .clicked()
                        {
                            actions.build_code = true;
                        }
                        if ui.button(t("Show problems")).clicked() {
                            actions.show_problems = true;
                        }
                    });
                });
        });
    }

    /// Runs one frame: input, calls, layout, the scene's picture, painting.
    /// Returns whether something was drawn (the canvas is then new).
    pub fn frame_canvas(
        &mut self,
        size: (u32, u32),
        seconds: f64,
        events: Vec<ShellEvent>,
    ) -> bool {
        // The language is per thread; this editor's is set for its frame.
        crate::i18n::set_language(self.language);
        self.since_poll += seconds;
        self.since_project_poll += seconds;
        self.since_log += seconds;
        let mut log_changed = false;
        if (self.tab == Tab::Console || self.tab == Tab::Activity || self.play.is_some())
            && self.since_log >= 0.5
        {
            self.since_log = 0.0;
            let before = (self.log_seq, self.activity_seq);
            self.read_log();
            self.read_activity();
            self.read_proposals();
            log_changed = match self.tab {
                Tab::Console => self.log_seq != before.0,
                Tab::Activity => self.activity_seq != before.1,
                _ => false,
            };
        }
        let mut project_changed = false;
        if self.since_project_poll >= PROJECT_POLL_SECONDS {
            self.since_project_poll = 0.0;
            project_changed = self.refresh_project();
        }
        let mut changed = !events.is_empty()
            || project_changed
            || log_changed
            || self.play.as_ref().is_some_and(|play| !play.paused)
            || self.frames < 3
            || (self.canvas.width, self.canvas.height) != size
            || self.movement.any()
            || self.editor.as_ref().is_some_and(Editor::wants_repaint);
        if self.since_poll >= POLL_SECONDS {
            self.since_poll = 0.0;
            changed |= self.refresh_scene();
        }
        changed |= self.cockpit_tick() || self.cockpit.run.is_some();
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
            let frame_started = std::time::Instant::now();
            let mut scene_ms = 0.0f32;
            let mut egui_events = Some(egui_events);
            let mut seconds = seconds;
            let mut passes = 0;
            let frame = loop {
                let mut viewport = self.viewport;
                let mut game_area = Rect::NOTHING;
                let mut actions = Actions::default();
                let frame = editor.layout(
                    size,
                    seconds,
                    egui_events.take().unwrap_or_default(),
                    |ui| {
                        actions = self.layout(ui, &mut viewport, &mut game_area);
                    },
                );
                self.viewport = viewport;
                if (self.game_viewport == Rect::NOTHING) != (game_area == Rect::NOTHING) {
                    // The scene view changes camera when the game view comes
                    // or goes during a game.
                    self.picture_stale = true;
                }
                self.game_viewport = game_area;
                let acted = actions.any();
                self.act(actions);
                let render_started = std::time::Instant::now();
                let picked = self.render_viewport(seconds);
                self.render_game();
                scene_ms += render_started.elapsed().as_secs_f32() * 1000.0;
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
            if self.game_viewport != Rect::NOTHING
                && let Some(picture) = &self.game_picture
            {
                blit(&mut self.canvas, picture, self.game_viewport);
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
            let total = frame_started.elapsed().as_secs_f32() * 1000.0;
            let (game, ticks) = std::mem::take(&mut self.game_time);
            self.timings.push_back(Timing {
                interface: (total - scene_ms).max(0.0),
                scene: scene_ms,
                game,
                ticks,
            });
            if self.timings.len() > crate::panels::HISTORY {
                self.timings.pop_front();
            }
            return true;
        }
        false
    }

    /// Where the game view is, if it is shown beside the scene view.
    pub fn game_view_area(&self) -> Option<Rect> {
        (self.game_viewport != Rect::NOTHING).then_some(self.game_viewport)
    }

    /// The scene view's place (where the editor's own camera draws).
    pub fn scene_view_area(&self) -> Rect {
        self.viewport
    }

    /// The canvas as drawn last.
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }
}

/// The middle must be this wide for the game view to be shown beside the
/// scene view.
const GAME_VIEW_MIN_WIDTH: f32 = 900.0;
/// The gap between the scene view and the game view.
const VIEW_GAP: f32 = 4.0;

/// A view's name in its top-left corner ("Scene", "Game").
fn view_badge(ui: &Ui, area: Rect, name: &str) {
    let font = egui::FontId::proportional(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), font, crate::style::TEXT);
    let rect = Rect::from_min_size(area.min + vec2(8.0, 8.0), galley.size() + vec2(16.0, 6.0));
    ui.painter()
        .rect_filled(rect, 6.0, egui::Color32::from_black_alpha(150));
    ui.painter()
        .galley(rect.min + vec2(8.0, 3.0), galley, crate::style::TEXT);
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
pub(crate) fn convert(events: &[ShellEvent], pointer: Option<(f32, f32)>) -> Vec<egui::Event> {
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
            ShellEvent::Paste(text) => out.push(egui::Event::Paste(text.clone())),
            ShellEvent::Copy => out.push(egui::Event::Copy),
            ShellEvent::Cut => out.push(egui::Event::Cut),
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

    fn take_clipboard(&mut self) -> Option<String> {
        self.editor.as_mut().and_then(Editor::take_copied)
    }
}

/// What a permission mode is called in the interface.
pub(crate) fn mode_label(mode: &str) -> &'static str {
    match mode {
        "suggest" => t("Only suggest"),
        "autonomous" => t("Autonomous"),
        _ => t("Apply, I review"),
    }
}

/// What a view of the scene is called in the interface (its name, such as
/// `lit`, is what `capture_frame` and `wen capture --view` take).
fn view_label(name: &str) -> &'static str {
    match name {
        "lit" => t("Shaded"),
        "ids" => t("Entity colours"),
        "depth" => t("Depth"),
        "normals" => t("Normals"),
        "wireframe" => t("Wireframe"),
        "colliders" => t("Colliders"),
        "lights" => t("Lights"),
        _ => t("Other view"),
    }
}

/// The ids in a set, for tests that compare hierarchies.
pub fn ids(rows: &[Row]) -> BTreeSet<&str> {
    rows.iter().map(|row| row.id.as_str()).collect()
}
