//! The editor's shell: the window's layout, the view of the scene and the
//! way input reaches both (M8a WP2).
//!
//! Everything the editor does to the project is a control-plane call by the
//! actor `human` ([`EditorApp::call`]): the panels never touch a scene file
//! or a world. What is shown comes from calls too (`project_info`,
//! `scene_get`, `edit_history`), so a change made by an agent or by editing
//! the file by hand shows up here the next time the editor looks.

use std::cell::RefCell;
use std::collections::BTreeSet;

use egui::{Align, Layout, Rect, Ui, pos2, vec2};
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

/// One entity of the hierarchy, flattened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The entity's id in the scene.
    pub id: String,
    /// How deep it is (0: a root).
    pub depth: usize,
    /// What components it has.
    pub components: Vec<String>,
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
    /// Where the scene's picture goes, from the last layout.
    viewport: Rect,
    /// What the scene looked like last (kept while nothing changes).
    picture: Option<RgbaImage>,
    drawn_size: (u32, u32),
    picture_stale: bool,
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
            can_undo: false,
            can_redo: false,
            problems: (0, 0),
            view: View::Lit,
            eye: RefCell::new(None),
            movement: Movement::default(),
            looking: false,
            pointer: None,
            modifiers: Modifiers::default(),
            viewport: Rect::NOTHING,
            picture: None,
            drawn_size: (0, 0),
            picture_stale: true,
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
        fn walk(nodes: &Value, depth: usize, rows: &mut Vec<Row>) {
            for node in nodes.as_array().into_iter().flatten() {
                rows.push(Row {
                    id: node["id"].as_str().unwrap_or("").to_owned(),
                    depth,
                    components: node["components"]
                        .as_array()
                        .map(|all| {
                            all.iter()
                                .filter_map(|c| c.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                });
                walk(&node["children"], depth + 1, rows);
            }
        }
        walk(&got["tree"], 0, &mut self.rows);
        let history = self.call(
            "edit_history",
            &json!({ "scene": self.scene.clone(), "action": "list" }),
        );
        self.can_undo = history["can_undo"] == true;
        self.can_redo = history["can_redo"] == true;
        self.picture_stale = true;
        true
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

    /// What the plain input events mean for the view: looking around, flying.
    fn view_input(&mut self, events: &[ShellEvent]) {
        for event in events {
            match event {
                ShellEvent::Pointer { x, y } => self.pointer = Some((*x, *y)),
                ShellEvent::PointerLeft => self.pointer = None,
                ShellEvent::Button {
                    button: Button::Secondary,
                    pressed,
                    ..
                } => {
                    let over = self
                        .pointer
                        .is_some_and(|(x, y)| self.viewport.contains(pos2(x, y)));
                    self.looking = *pressed && over;
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
            if let ShellEvent::Button { modifiers, .. } | ShellEvent::Key { modifiers, .. } = event
            {
                self.modifiers = *modifiers;
                self.movement.fast = modifiers.shift;
            }
        }
    }

    /// The picture of the scene at the viewport's size, drawn when something
    /// changed (the scene, the camera, the size).
    fn render_viewport(&mut self, seconds: f64) {
        let (width, height) = (
            self.viewport.width().max(0.0) as u32,
            self.viewport.height().max(0.0) as u32,
        );
        if self.viewport.width() < MIN_VIEWPORT || self.viewport.height() < MIN_VIEWPORT {
            self.picture = None;
            return;
        }
        if self.movement.any()
            && let Some(eye) = self.eye.borrow_mut().as_mut()
        {
            eye.step(self.movement, seconds as f32);
            self.picture_stale = true;
        }
        if !self.picture_stale && self.drawn_size == (width, height) && self.picture.is_some() {
            return;
        }
        let eye = &self.eye;
        let camera = |base: &CameraView| {
            let mut eye = eye.borrow_mut();
            eye.get_or_insert_with(|| FlyCamera::from_view(base))
                .apply(base)
        };
        let scene = self.scene.clone();
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
            .show(ui, |ui| {
                ui.heading("Hierarchy");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for row in &self.rows {
                        ui.horizontal(|ui| {
                            ui.add_space(row.depth as f32 * 14.0);
                            ui.label(&row.id);
                        });
                    }
                });
            });
        egui::Panel::right("inspector")
            .default_size(260.0)
            .show(ui, |ui| {
                ui.heading("Inspector");
                ui.separator();
                ui.weak("Select an entity to edit its components.");
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
    /// Returns the canvas when something changed.
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
        if changed || self.picture_stale {
            let egui_events = convert(&events, self.pointer);
            if self.canvas.width != size.0 || self.canvas.height != size.1 {
                self.canvas = Canvas::new(size.0, size.1, BACKDROP);
            }
            let Some(mut editor) = self.editor.take() else {
                return false;
            };
            let mut viewport = self.viewport;
            let mut actions = Actions::default();
            let frame = editor.layout(size, seconds, egui_events, |ui| {
                actions = self.layout(ui, &mut viewport);
            });
            self.viewport = viewport;
            self.act(actions);
            self.render_viewport(seconds);
            // Backdrop, then the scene, then the panels over it.
            for pixel in self.canvas.pixels.as_chunks_mut::<4>().0 {
                *pixel = BACKDROP;
            }
            if let Some(picture) = &self.picture {
                blit(&mut self.canvas, picture, self.viewport);
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
