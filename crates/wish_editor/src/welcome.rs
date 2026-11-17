//! The first-run flow (M9 WP3): what `wen edit` shows when it is not given
//! a project. The person chooses who helps (Claude Code, or nobody: the
//! engine is complete by hand), what to start from (an empty project or one
//! of the engine's templates) and a name and folder; or opens a recent
//! project. The editor then opens in the same window (a process has one
//! window loop), with the Agent panel shown if an assistant was chosen.
//!
//! Projects are made by the control plane's scaffold, as `wen new` makes
//! them; opening one is `Workspace::open`. Nothing here is a capability of
//! its own.

use std::path::{Path, PathBuf};

use egui::{RichText, Ui};
use serde_json::{Value, json};
use wish_control::scaffold::Template;
use wish_control::{Workspace, WorkspaceOptions};
use wish_window::{Picture, Shell, ShellEvent};

use crate::i18n::{t, tf};
use crate::raster::Canvas;
use crate::script::Driven;
use crate::{Editor, EditorApp, Widget};

const BACKDROP: [u8; 4] = [24, 24, 27, 255];

/// What the launcher knows before it opens.
#[derive(Clone, Debug)]
pub struct LaunchOptions {
    /// Draw scenes with the software adapter.
    pub software: bool,
    /// The person's preferences file (language, recent projects, the
    /// assistant chosen), if they are kept.
    pub preferences: Option<PathBuf>,
    /// The templates a new project can start from.
    pub templates: Vec<Template>,
    /// The folder new projects go into by default.
    pub projects_folder: PathBuf,
    /// How MCP clients start the engine (written to `.mcp.json`).
    pub command: String,
    /// Claude Code, if it is installed.
    pub claude: Option<PathBuf>,
    /// Build the game code of a new project before the editor opens (the
    /// templates' scenes use the game's own types).
    pub build: bool,
    /// The compiler and SDK for game code (else the workspace finds them).
    pub toolchain: Option<wish_script::build::Toolchain>,
}

impl LaunchOptions {
    /// What an installed engine uses: the preferences in their usual place,
    /// the templates beside the program (or the repository's examples),
    /// `Documents/Wish Projects`, this program for `.mcp.json`, Claude Code
    /// if found.
    pub fn from_environment(software: bool) -> Self {
        let templates = wish_control::scaffold::templates_folder()
            .map(|folder| wish_control::scaffold::templates(&folder))
            .unwrap_or_default();
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let base = documents_folder()
            .or_else(|| Some(home.join("Documents")).filter(|dir| dir.is_dir()))
            .unwrap_or(home);
        Self {
            software,
            preferences: crate::preferences_path(),
            templates,
            projects_folder: base.join("Wish Projects"),
            command: std::env::current_exe()
                .ok()
                .and_then(|exe| exe.to_str().map(str::to_owned))
                .unwrap_or_else(|| "wen".to_owned()),
            claude: wish_agent::claude::find(),
            build: true,
            toolchain: None,
        }
    }
}

/// Who helps the person make the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assistant {
    /// Claude Code, through the Agent panel.
    Claude,
    /// Nobody: everything by hand.
    Nobody,
}

/// What was asked for, done on the next frame (after a picture that says
/// it is being done: making a project and building its code take a while).
#[derive(Clone, Debug)]
enum Pending {
    Create,
    Open(PathBuf),
}

/// The welcome screen's state.
struct Welcome {
    editor: Editor,
    canvas: Canvas,
    frames: u64,
    language: &'static str,
    recent: Vec<String>,
    assistant: Assistant,
    /// The template chosen (`None`: an empty project).
    template: Option<usize>,
    name: String,
    /// The name offered (while `name` is still it, it follows the
    /// language).
    offered: String,
    folder: String,
    open_path: String,
    status: String,
    pending: Option<Pending>,
    /// Where the pointer is (a click without a move happens there).
    pointer: Option<(f32, f32)>,
}

/// What a frame of the welcome screen asked for.
#[derive(Default)]
struct Asked {
    create: bool,
    open: Option<PathBuf>,
    language: Option<&'static str>,
}

/// The window `wen edit` opens without a project: the welcome screen, then
/// the editor on the project made or chosen.
pub struct Launcher {
    options: LaunchOptions,
    welcome: Option<Box<Welcome>>,
    app: Option<Box<EditorApp>>,
}

impl Launcher {
    /// The welcome screen.
    pub fn new(options: LaunchOptions) -> Self {
        let prefs = options
            .preferences
            .as_deref()
            .map(crate::prefs::read)
            .unwrap_or_default();
        let language = prefs
            .get("language")
            .and_then(Value::as_str)
            .and_then(crate::i18n::find)
            .or_else(crate::i18n::from_environment)
            .map_or("en", |language| language.code);
        let recent = crate::prefs::recent(&prefs)
            .into_iter()
            .filter(|root| Path::new(root).join(wish_control::PROJECT_FILE).is_file())
            .collect();
        let assistant = match (
            prefs.get("assistant").and_then(Value::as_str),
            &options.claude,
        ) {
            (Some("none"), _) | (_, None) => Assistant::Nobody,
            _ => Assistant::Claude,
        };
        // The showcase is the default start: something that plays at once.
        let template = options
            .templates
            .iter()
            .position(|template| template.id == "islands");
        // (The default name is in the person's language.)
        crate::i18n::set_language(language);
        let name = first_free_name(&options.projects_folder);
        let welcome = Welcome {
            editor: Editor::new(),
            canvas: Canvas::new(1, 1, BACKDROP),
            frames: 0,
            language,
            recent,
            assistant,
            template,
            folder: options.projects_folder.display().to_string(),
            offered: name.clone(),
            name,
            open_path: String::new(),
            status: String::new(),
            pending: None,
            pointer: None,
        };
        Self {
            options,
            welcome: Some(Box::new(welcome)),
            app: None,
        }
    }

    /// The editor, once a project is open.
    pub fn app(&mut self) -> Option<&mut EditorApp> {
        self.app.as_deref_mut()
    }

    /// The widgets on screen (the welcome screen's, then the editor's).
    pub fn widgets(&self) -> &[Widget] {
        match (&self.app, &self.welcome) {
            (Some(app), _) => app.widgets(),
            (None, Some(welcome)) => welcome.editor.widgets(),
            (None, None) => &[],
        }
    }

    /// The status line (the welcome screen's, then the editor's).
    pub fn status(&self) -> &str {
        match (&self.app, &self.welcome) {
            (Some(app), _) => app.status(),
            (None, Some(welcome)) => &welcome.status,
            (None, None) => "",
        }
    }

    /// Shows the welcome screen (and the editor after it) in the language
    /// `code`; returns whether there is such a language.
    pub fn set_language(&mut self, code: &str) -> bool {
        let Some(language) = crate::i18n::find(code) else {
            return false;
        };
        if let Some(welcome) = &mut self.welcome {
            welcome.set_language(language.code);
        }
        if let Some(app) = &mut self.app {
            app.set_language(language.code);
        }
        true
    }

    /// Chooses who helps (as the radio buttons do).
    pub fn set_assistant(&mut self, assistant: Assistant) {
        if let Some(welcome) = &mut self.welcome {
            welcome.assistant = assistant;
        }
    }

    /// The folder the new project would be made in.
    pub fn new_project_folder(&self) -> Option<PathBuf> {
        let welcome = self.welcome.as_ref()?;
        Some(Path::new(welcome.folder.trim()).join(slug(&welcome.name)))
    }

    /// Opens the project in `root` in the editor.
    fn open(&mut self, root: &Path, after_create: bool) -> Result<(), String> {
        let mut workspace = Workspace::open(WorkspaceOptions {
            root: root.to_path_buf(),
            example: None,
        })
        .map_err(|error| format!("{} {}", error.message, error.hint))?;
        workspace.set_software(self.options.software);
        if let Some(toolchain) = &self.options.toolchain {
            workspace.set_toolchain(toolchain.clone());
        }
        let mut app = EditorApp::new(workspace);
        let assistant = self
            .welcome
            .as_ref()
            .map_or(Assistant::Nobody, |welcome| welcome.assistant);
        if let Some(welcome) = &self.welcome {
            app.set_language(welcome.language);
        }
        if let Some(path) = &self.options.preferences {
            app.use_preferences(path.clone());
        }
        let mut status = if after_create {
            match assistant {
                Assistant::Claude => {
                    t("Project created. Tell the agent what to make in the Agent panel, or edit by hand.")
                        .to_owned()
                }
                Assistant::Nobody => t("Project created. Press Play to try it.").to_owned(),
            }
        } else {
            tf("opened {root}", &[("root", &root.display())])
        };
        if after_create && self.options.build && root.join("scripts/src/lib.rs").is_file() {
            let built = app.call("script_build", &json!({}));
            if built["passed"] != true {
                // The tool's own failure, or the compiler's first error.
                let first = built["diagnostics"]
                    .as_array()
                    .and_then(|all| all.iter().find(|d| d["severity"] == "error"))
                    .unwrap_or(&built["error"]);
                status = tf(
                    "the game code was not built: {message} {hint}",
                    &[
                        ("message", &first["message"].as_str().unwrap_or("?")),
                        ("hint", &first["hint"].as_str().unwrap_or("")),
                    ],
                );
            }
        }
        if assistant == Assistant::Claude {
            if let Some(program) = &self.options.claude {
                app.set_agent(Box::new(wish_agent::claude::ClaudeCode {
                    program: program.clone(),
                }));
            }
            app.open_agent_panel();
        }
        app.set_status(status);
        self.app = Some(Box::new(app));
        Ok(())
    }

    /// Makes the new project, then opens it.
    fn create(&mut self) -> Result<(), String> {
        let Some(welcome) = &self.welcome else {
            return Ok(());
        };
        let name = welcome.name.trim().to_owned();
        let Some(dir) = self.new_project_folder() else {
            return Ok(());
        };
        if welcome.folder.trim().is_empty() {
            return Err(t("Choose the folder the project goes in.").to_owned());
        }
        if dir.exists() && std::fs::read_dir(&dir).is_ok_and(|mut entries| entries.next().is_some())
        {
            return Err(tf(
                "{folder} already exists and is not empty: choose another name.",
                &[("folder", &dir.display())],
            ));
        }
        // Made beside its place and moved there only when complete: a
        // failure halfway leaves nothing behind.
        let making = dir.with_file_name(format!(
            ".{}.creating",
            dir.file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        ));
        let _ = std::fs::remove_dir_all(&making);
        let made = match welcome.template.and_then(|i| self.options.templates.get(i)) {
            Some(template) => wish_control::scaffold::create_from_template(
                &making,
                &name,
                &self.options.command,
                &template.path,
            ),
            None => wish_control::scaffold::create_project(&making, &name, &self.options.command),
        };
        let placed = made
            .map_err(|error| format!("{} {}", error.message, error.hint))
            .and_then(|_| {
                // An empty folder of that name gives way.
                if dir.exists() {
                    std::fs::remove_dir(&dir).map_err(|error| error.to_string())?;
                }
                std::fs::rename(&making, &dir).map_err(|error| error.to_string())
            });
        if let Err(message) = placed {
            let _ = std::fs::remove_dir_all(&making);
            return Err(message);
        }
        self.open(&dir, true)
    }

    /// Writes what the welcome screen keeps (the language, the assistant
    /// chosen).
    fn save_choices(&self) {
        let (Some(path), Some(welcome)) = (&self.options.preferences, &self.welcome) else {
            return;
        };
        let assistant = match welcome.assistant {
            Assistant::Claude => "claude",
            Assistant::Nobody => "none",
        };
        let language = welcome.language;
        let _ = crate::prefs::update(path, |prefs| {
            prefs.insert("language".to_owned(), json!(language));
            prefs.insert("assistant".to_owned(), json!(assistant));
        });
    }

    /// One frame of the welcome screen.
    fn welcome_frame(&mut self, size: (u32, u32), seconds: f64, events: Vec<ShellEvent>) -> bool {
        // What was asked last frame is done now (its picture said so).
        if let Some(pending) = self.welcome.as_mut().and_then(|w| w.pending.take()) {
            self.save_choices();
            let done = match pending {
                Pending::Create => self.create(),
                Pending::Open(root) => self.open(&root, false),
            };
            match done {
                Ok(()) => {
                    self.welcome = None;
                    return self.frame_app(size, seconds, events);
                }
                Err(message) => {
                    if let Some(welcome) = &mut self.welcome {
                        welcome.status = message;
                        if welcome.name == welcome.offered {
                            welcome.offered = first_free_name(Path::new(welcome.folder.trim()));
                            welcome.name = welcome.offered.clone();
                        }
                    }
                }
            }
        }
        let Some(welcome) = self.welcome.as_deref_mut() else {
            return false;
        };
        crate::i18n::set_language(welcome.language);
        let changed = !events.is_empty()
            || welcome.frames < 3
            || (welcome.canvas.width, welcome.canvas.height) != size
            || welcome.editor.wants_repaint()
            || !welcome.status.is_empty() && welcome.frames < 5;
        if !changed {
            return false;
        }
        if (welcome.canvas.width, welcome.canvas.height) != size {
            welcome.canvas = Canvas::new(size.0, size.1, BACKDROP);
        }
        let egui_events = crate::app::convert(&events, welcome.pointer);
        for event in &events {
            match event {
                ShellEvent::Pointer { x, y } => welcome.pointer = Some((*x, *y)),
                ShellEvent::PointerLeft => welcome.pointer = None,
                _ => {}
            }
        }
        let templates = &self.options.templates;
        let claude = self.options.claude.as_deref();
        let mut asked = Asked::default();
        let mut editor = std::mem::take(&mut welcome.editor);
        let mut egui_events = Some(egui_events);
        let mut seconds = seconds;
        let frame = loop {
            let frame = editor.layout(
                size,
                seconds,
                egui_events.take().unwrap_or_default(),
                |ui| {
                    asked = layout(ui, welcome, templates, claude);
                },
            );
            let mut acted = false;
            if let Some(code) = asked.language.take() {
                welcome.set_language(code);
                acted = true;
            }
            if asked.create {
                if welcome.name.trim().is_empty() {
                    welcome.status = t("Give the project a name.").to_owned();
                } else {
                    welcome.status = tf("creating {name}...", &[("name", &welcome.name.trim())]);
                    welcome.pending = Some(Pending::Create);
                }
                acted = true;
            }
            if let Some(root) = asked.open.take() {
                if root.join(wish_control::PROJECT_FILE).is_file() {
                    welcome.status = tf("opening {root}...", &[("root", &root.display())]);
                    welcome.pending = Some(Pending::Open(root));
                } else {
                    welcome.status = tf(
                        "{root} is not a project (it has no wish.json): choose its folder, or create a new project.",
                        &[("root", &root.display())],
                    );
                }
                acted = true;
            }
            asked = Asked::default();
            if acted {
                seconds = 0.0;
                continue;
            }
            break frame;
        };
        for pixel in welcome.canvas.pixels.as_chunks_mut::<4>().0 {
            *pixel = BACKDROP;
        }
        editor.paint(&frame, &mut welcome.canvas);
        welcome.editor = editor;
        welcome.frames += 1;
        if welcome.pending.is_some() {
            // Draw again at once, so the work starts.
            welcome.frames = welcome.frames.min(2);
        }
        true
    }

    fn frame_app(&mut self, size: (u32, u32), seconds: f64, events: Vec<ShellEvent>) -> bool {
        self.app
            .as_mut()
            .is_some_and(|app| app.frame_canvas(size, seconds, events))
    }
}

impl Welcome {
    /// Shows the screen in another language (the offered name too, unless
    /// the person typed their own).
    fn set_language(&mut self, code: &'static str) {
        self.language = code;
        crate::i18n::set_language(code);
        if self.name == self.offered {
            self.offered = first_free_name(Path::new(self.folder.trim()));
            self.name = self.offered.clone();
        }
    }
}

/// Lays out the welcome screen; returns what it asked for.
fn layout(
    ui: &mut Ui,
    welcome: &mut Welcome,
    templates: &[Template],
    claude: Option<&Path>,
) -> Asked {
    let mut asked = Asked::default();
    egui::Panel::bottom("welcome_status").show(ui, |ui| {
        ui.label(if welcome.status.is_empty() {
            t("Everything can be done by hand or with an assistant, and both land in the same history.")
        } else {
            welcome.status.as_str()
        });
    });
    egui::CentralPanel::default().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.heading(t("Welcome to Wish Engine"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt("welcome_language")
                    .selected_text(
                        crate::i18n::find(welcome.language).map_or("English", |l| l.name),
                    )
                    .show_ui(ui, |ui| {
                        for language in crate::i18n::LANGUAGES {
                            if ui
                                .selectable_label(welcome.language == language.code, language.name)
                                .clicked()
                            {
                                asked.language = Some(language.code);
                            }
                        }
                    });
            });
        });
        ui.separator();
        ui.columns(2, |columns| {
            new_game(&mut columns[0], welcome, templates, claude, &mut asked);
            open_game(&mut columns[1], welcome, &mut asked);
        });
    });
    asked
}

/// The left column: a new game.
fn new_game(
    ui: &mut Ui,
    welcome: &mut Welcome,
    templates: &[Template],
    claude: Option<&Path>,
    asked: &mut Asked,
) {
    egui::ScrollArea::vertical()
        .id_salt("welcome_new")
        .show(ui, |ui| {
            ui.heading(t("Start a new game"));
            ui.add_space(6.0);
            ui.label(RichText::new(t("1. Who helps you?")).strong());
            ui.add_enabled_ui(claude.is_some(), |ui| {
                ui.radio_value(&mut welcome.assistant, Assistant::Claude, t("Claude Code"));
            });
            match claude {
                Some(program) => {
                    ui.small(tf("found: {program}", &[("program", &program.display())]));
                }
                None => {
                    ui.small(t(
                        "Claude Code was not found: install it and sign in once (claude), then open Wish Engine again.",
                    ));
                }
            }
            ui.radio_value(
                &mut welcome.assistant,
                Assistant::Nobody,
                t("Nobody: I make it by hand"),
            );
            ui.small(t(
                "Any MCP agent can connect later: Project menu, Connect an outside agent.",
            ));
            ui.add_space(8.0);
            ui.label(RichText::new(t("2. Start from")).strong());
            ui.radio_value(&mut welcome.template, None, t("Empty project"));
            ui.small(t("A camera, a sun, a ground and starter game code."));
            // The showcase first: it shows the most of the engine.
            let mut order: Vec<usize> = (0..templates.len()).collect();
            order.sort_by_key(|&index| templates[index].id != "islands");
            for index in order {
                let template = &templates[index];
                ui.radio_value(&mut welcome.template, Some(index), template.name.as_str());
                if let Some(about) = about(&template.id) {
                    ui.small(about);
                }
            }
            ui.add_space(8.0);
            ui.label(RichText::new(t("3. Name and place")).strong());
            egui::Grid::new("welcome_grid")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label(t("Name"));
                    ui.add(egui::TextEdit::singleline(&mut welcome.name).desired_width(260.0));
                    ui.end_row();
                    ui.label(t("Folder"));
                    ui.add(egui::TextEdit::singleline(&mut welcome.folder).desired_width(260.0));
                    ui.end_row();
                });
            ui.small(tf(
                "It goes in: {path}",
                &[(
                    "path",
                    &Path::new(welcome.folder.trim())
                        .join(slug(&welcome.name))
                        .display(),
                )],
            ));
            ui.add_space(6.0);
            let busy = welcome.pending.is_some();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new(RichText::new(t("Create")).strong()))
                    .clicked()
                {
                    asked.create = true;
                }
            });
        });
}

/// The right column: a game made before.
fn open_game(ui: &mut Ui, welcome: &mut Welcome, asked: &mut Asked) {
    ui.heading(t("Open a game"));
    ui.add_space(6.0);
    if welcome.recent.is_empty() {
        ui.small(t("Projects you open are listed here."));
    } else {
        ui.label(RichText::new(t("Recent projects")).strong());
        for root in &welcome.recent {
            let name = Path::new(root)
                .file_name()
                .map_or_else(|| root.clone(), |name| name.to_string_lossy().into_owned());
            if ui.button(name).on_hover_text(root.as_str()).clicked() {
                asked.open = Some(PathBuf::from(root));
            }
        }
    }
    ui.add_space(8.0);
    ui.label(RichText::new(t("A project folder")).strong());
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut welcome.open_path)
                .hint_text(t("the folder with wish.json"))
                .desired_width(260.0),
        );
        if ui.button(t("Open")).clicked() && !welcome.open_path.trim().is_empty() {
            asked.open = Some(PathBuf::from(welcome.open_path.trim()));
        }
    });
}

/// What a template of the engine is (others show only their name).
fn about(id: &str) -> Option<&'static str> {
    Some(match id {
        "islands" => t("3D platformer: islands, moving platforms, coins (the showcase)"),
        "hop" => t("3D: jump between platforms and pick up coins"),
        "collect" => t("3D, top down: walk and pick up every coin"),
        "bricks" => t("2D: break the bricks with a ball"),
        "quiz" => t("Interface only: questions and answer buttons"),
        _ => return None,
    })
}

/// A folder name for a project name: lower case letters, digits, `-`.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() {
        "my-game".to_owned()
    } else if wish_control::scaffold::reserved_on_windows(&out) {
        format!("{out}-game")
    } else {
        out
    }
}

/// The person's documents folder as the system says it is (on Windows it
/// can be moved, such as into OneDrive): the shell folder in the registry,
/// with its variables expanded.
fn documents_folder() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders",
            "/v",
            "Personal",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with("Personal"))?;
    let value = line
        .split("REG_")
        .nth(1)?
        .split_once(char::is_whitespace)?
        .1
        .trim();
    let folder = PathBuf::from(expand_variables(value));
    folder.is_dir().then_some(folder)
}

/// `%NAME%` replaced by the variable's value.
fn expand_variables(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(value) => out.push_str(&value),
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// "My game", or "My game 2"... whichever has no folder yet.
fn first_free_name(folder: &Path) -> String {
    let base = t("My game");
    (1..100)
        .map(|n| {
            if n == 1 {
                base.to_owned()
            } else {
                format!("{base} {n}")
            }
        })
        .find(|name| !folder.join(slug(name)).exists())
        .unwrap_or_else(|| base.to_owned())
}

impl Driven for Launcher {
    fn frame_canvas(&mut self, size: (u32, u32), seconds: f64, events: Vec<ShellEvent>) -> bool {
        if self.app.is_some() {
            self.frame_app(size, seconds, events)
        } else {
            self.welcome_frame(size, seconds, events)
        }
    }

    fn canvas(&self) -> &Canvas {
        match (&self.app, &self.welcome) {
            (Some(app), _) => app.canvas(),
            (None, Some(welcome)) => &welcome.canvas,
            // (There is always one of the two.)
            (None, None) => &EMPTY,
        }
    }
}

static EMPTY: std::sync::LazyLock<Canvas> =
    std::sync::LazyLock::new(|| Canvas::new(1, 1, BACKDROP));

impl Shell for Launcher {
    fn title(&self) -> String {
        match &self.app {
            Some(app) => app.title(),
            None => "Wish Engine".to_owned(),
        }
    }

    fn frame(
        &mut self,
        size: (u32, u32),
        seconds: f64,
        events: Vec<ShellEvent>,
    ) -> Option<Picture<'_>> {
        if Driven::frame_canvas(self, size, seconds, events) {
            let canvas = Driven::canvas(self);
            return Some(Picture {
                width: canvas.width,
                height: canvas.height,
                rgba: &canvas.pixels,
            });
        }
        None
    }

    fn wants_exit(&self) -> bool {
        self.app.as_ref().is_some_and(|app| app.wants_exit())
    }
}
