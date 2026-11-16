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

use egui::{Align, Layout, Rect, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use wish_control::{PictureSource, Workspace, parse_actor};
use wish_render::{CameraView, RgbaImage, View};
use wish_window::{Button, FlyCamera, Modifiers, Movement, Picture, Shell, ShellEvent};

use crate::Editor;
use crate::raster::Canvas;

/// How often the project is looked at for changes made elsewhere.
const POLL_SECONDS: f64 = 0.4;
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
}

/// What the layout keeps between frames (text being typed).
#[derive(Default)]
struct UiState {
    /// The row being renamed and the name typed so far.
    rename: Option<(String, String)>,
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
    status: String,
    since_poll: f64,
    frames: u64,
    exit: bool,
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
            status: String::new(),
            since_poll: POLL_SECONDS,
            frames: 0,
            exit: false,
        };
        app.refresh_project();
        app
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
        self.selected = ids
            .iter()
            .filter(|id| self.rows.iter().any(|row| row.id == **id))
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

    /// The boxes drawn around the selection in the view (window pixels).
    pub fn outlines(&self) -> &[Rect] {
        &self.outlines
    }

    /// Reads what the title and lists need: the project, its scenes, the
    /// problems.
    fn refresh_project(&mut self) {
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
                });
                walk(&node["children"], depth + 1, Some(&id), rows);
            }
        }
        walk(&got["tree"], 0, None, &mut self.rows);
        // What was selected may be gone now.
        let known: BTreeSet<&str> = self.rows.iter().map(|row| row.id.as_str()).collect();
        self.selected.retain(|id| known.contains(id.as_str()));
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
        let mut args = json!({ "scene": self.scene.clone(), "ops": ops });
        if self.revision != u64::MAX {
            args["base_revision"] = json!(self.revision);
        }
        let done = self.call("edit_apply", &args);
        let applied = done["ok"] == true;
        self.status = if applied {
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
        applied
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
                    if *pressed {
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
        let camera = |base: &CameraView| {
            let mut eye = eye.borrow_mut();
            eye.get_or_insert_with(|| FlyCamera::from_view(base))
                .apply(base)
        };
        let scene = self.scene.clone();
        if redraw {
            let drawn = self.workspace.render_picture(
                PictureSource::Scene(Some(&scene)),
                Some(&camera),
                width,
                height,
                self.view,
            );
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
        if pick.is_none() && self.selected.is_empty() {
            self.outlines.clear();
            return false;
        }
        let ids = self.workspace.render_ids(
            PictureSource::Scene(Some(&scene)),
            Some(&camera),
            width,
            height,
        );
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
        let typing = self.ui_state.borrow().rename.is_some();
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
                if ui.button("Reset camera").clicked() {
                    actions.reset_camera = true;
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
        egui::Panel::left("hierarchy")
            .default_size(220.0)
            .show(ui, |ui| self.hierarchy(ui, &mut actions));
        egui::Panel::right("inspector")
            .default_size(260.0)
            .show(ui, |ui| {
                ui.heading("Inspector");
                ui.separator();
                match self.selected.as_slice() {
                    [] => {
                        ui.weak("Select an entity to edit its components.");
                    }
                    [one] => {
                        ui.strong(one);
                    }
                    many => {
                        ui.strong(format!("{} entities", many.len()));
                    }
                }
            });
        // The middle is not painted: the scene's picture shows through.
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                *viewport = ui.max_rect();
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
        let mut changed = !events.is_empty() || self.frames < 3 || self.movement.any();
        if self.since_poll >= POLL_SECONDS {
            self.since_poll = 0.0;
            changed |= self.refresh_scene();
        }
        self.view_input(&events);
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
            for outline in &self.outlines {
                draw_outline(&mut self.canvas, *outline, self.viewport);
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
