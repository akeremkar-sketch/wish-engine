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

/// The empty project's picture (its starting scene, drawn by the engine).
const EMPTY_PICTURE: &[u8] = include_bytes!("../assets/empty-project.png");

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
    /// Open with the mark's short animation (a click or a key skips it).
    pub splash: bool,
    /// The person's settings folder, where the list of engines is kept
    /// (`None`: only the running engine is known).
    pub engines: Option<PathBuf>,
    /// Start another engine for a project made with its version (else that
    /// is only noted: tests).
    pub hand_off: bool,
}

impl LaunchOptions {
    /// What an installed engine uses: the preferences in their usual place,
    /// the templates beside the program (or the repository's examples),
    /// `Documents/Wish Projects`, this program for `.mcp.json`, Claude Code
    /// if found.
    pub fn from_environment(software: bool) -> Self {
        let templates = wish_control::scaffold::all_templates();
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        // Documents first; but Windows' controlled folder access can keep
        // programs it does not know out of it, so the first folder that can
        // really be written to.
        let candidates = [
            documents_folder(),
            Some(home.join("Documents")).filter(|dir| dir.is_dir()),
            Some(home),
        ];
        let projects_folder = candidates
            .iter()
            .flatten()
            .map(|base| base.join("Wish Projects"))
            .find(|folder| writable(folder))
            .unwrap_or_else(|| PathBuf::from("Wish Projects"));
        Self {
            software,
            preferences: crate::preferences_path(),
            templates,
            projects_folder,
            command: std::env::current_exe()
                .ok()
                .and_then(|exe| exe.to_str().map(str::to_owned))
                .unwrap_or_else(|| "wen".to_owned()),
            claude: wish_agent::claude::find(),
            build: true,
            toolchain: None,
            splash: true,
            engines: wish_control::crash::machine_folder(),
            hand_off: true,
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
    /// The engine version each recent project was made with, when it is
    /// not this engine's.
    recent_made: Vec<Option<String>>,
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
    /// Each template's picture (`.template/thumbnail.png`), by template.
    pictures: Vec<Option<egui::TextureHandle>>,
    /// The empty project's picture.
    empty_picture: Option<egui::TextureHandle>,
    /// The pictures were read (once).
    pictures_loaded: bool,
    /// The person's theme (the editor keeps it too).
    theme: crate::style::Theme,
    /// How long the opening animation has played (`None`: it is over).
    splash: Option<f64>,
    /// The Hub (M10b): the tab shown, and what its pages show.
    tab: crate::hub::Tab,
    theme_key: String,
    themes: Vec<crate::themes::ThemeEntry>,
    /// Each theme read (the Look tab draws them every frame).
    theme_looks: Vec<Option<crate::style::Theme>>,
    /// Whether the packages were read (an empty list is an answer too).
    packages_read: bool,
    /// A package came or went: the templates are read again.
    templates_stale: bool,
    /// The person's shortcuts (the `keys` preference).
    keys: std::collections::BTreeMap<String, String>,
    user_packages: Vec<Value>,
    engine_packages: Vec<Value>,
    /// The `agent` preferences (the AI mode reads them too).
    agent: serde_json::Map<String, Value>,
    /// The folder typed on the Packages tab.
    package_path: String,
    /// The package index form (Packages tab).
    index: crate::packages_window::IndexForm,
    /// A theme was chosen: the interface takes it before its next layout.
    restyle: bool,
    /// The settings folder of the engines list.
    engines_config: Option<PathBuf>,
    /// The engines on this computer (Engine versions tab).
    engines: Vec<wish_control::engines::Engine>,
    engine_warnings: Vec<String>,
    /// Added folders that are no longer engines.
    engine_stale: Vec<PathBuf>,
    /// The folder typed on the Engine versions tab.
    engine_path: String,
}

impl Welcome {
    /// Reads the person's packages and the engine's for the Hub.
    fn read_packages(&mut self) {
        let here = Path::new(".");
        self.user_packages =
            wish_control::packages::listing(wish_control::packages::Scope::User, here, &[]).0;
        self.engine_packages = wish_control::packages::engine_packages_folder()
            .and_then(|folder| std::fs::read_dir(folder).ok())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| wish_control::packages::read_manifest(&entry.path()).ok())
            .map(|manifest| manifest.to_value())
            .collect();
        self.engine_packages
            .sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        self.themes = crate::themes::available(None);
        self.theme_looks = self
            .themes
            .iter()
            .map(|entry| crate::themes::load(&self.themes, &entry.key).ok())
            .collect();
        self.packages_read = true;
        self.read_engines();
    }

    /// Reads the engines on this computer for the Hub.
    fn read_engines(&mut self) {
        let listing = wish_control::engines::listing(self.engines_config.as_deref());
        self.engine_warnings = listing
            .warnings
            .iter()
            .map(|w| format!("{} {}", w.message, w.hint))
            .collect();
        self.engine_stale = listing.stale.into_iter().map(|(f, _)| f).collect();
        self.engines = listing.engines;
    }
}

/// What a frame of the welcome screen asked for.
#[derive(Default)]
struct Asked {
    create: bool,
    open: Option<PathBuf>,
    language: Option<&'static str>,
    /// What a Hub tab asked for.
    hub: crate::hub::HubAsked,
}

/// The window `wen edit` opens without a project: the welcome screen, then
/// the editor on the project made or chosen.
pub struct Launcher {
    options: LaunchOptions,
    welcome: Option<Box<Welcome>>,
    app: Option<Box<EditorApp>>,
    /// The engine a project was handed to (this window then closes).
    handed_off: Option<HandOff>,
}

/// A project handed to another engine: this window stays a moment, until
/// that engine is surely up; if it ends with an error at once, the
/// project opens here and says what it said.
struct HandOff {
    engine: wish_control::engines::Engine,
    root: PathBuf,
    /// The other engine (none when only recorded, in tests).
    child: Option<std::process::Child>,
    /// Where its error output goes.
    log: Option<PathBuf>,
    started: std::time::Instant,
    /// It is up (or done): the window may close.
    settled: bool,
}

/// How long a handed-off engine must keep running before this window
/// closes.
const HAND_OFF_SECONDS: f64 = 2.0;

impl Launcher {
    /// The welcome screen.
    pub fn new(options: LaunchOptions) -> Self {
        let prefs = options
            .preferences
            .as_deref()
            .map(crate::prefs::read)
            .unwrap_or_default();
        // The languages packages bring, before the person's choice is read.
        crate::themes::load_languages(None);
        let language = prefs
            .get("language")
            .and_then(Value::as_str)
            .and_then(crate::i18n::find)
            .or_else(crate::i18n::from_environment)
            .map_or("en", |language| language.code);
        let recent: Vec<String> = crate::prefs::recent(&prefs)
            .into_iter()
            .filter(|root| Path::new(root).join(wish_control::PROJECT_FILE).is_file())
            .collect();
        let here = wish_control::packages::Version::engine();
        let recent_made = recent
            .iter()
            .map(|root| {
                wish_control::engines::project_version(Path::new(root))
                    .filter(|made| *made != here)
                    .map(|made| made.to_string())
            })
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
        // The person's theme, from the engine's and their own packages.
        let theme = prefs
            .get("theme")
            .and_then(Value::as_str)
            .and_then(|key| crate::themes::load(&crate::themes::available(None), key).ok())
            .unwrap_or(crate::style::ATELIER);
        crate::style::set_theme(theme);
        let welcome = Welcome {
            editor: Editor::new(),
            canvas: Canvas::new(1, 1, crate::style::pal().backdrop),
            frames: 0,
            language,
            recent,
            recent_made,
            assistant,
            template,
            folder: options.projects_folder.display().to_string(),
            offered: name.clone(),
            name,
            open_path: String::new(),
            status: String::new(),
            pending: None,
            pointer: None,
            pictures: Vec::new(),
            empty_picture: None,
            pictures_loaded: false,
            theme,
            tab: crate::hub::Tab::Projects,
            theme_key: prefs
                .get("theme")
                .and_then(Value::as_str)
                .unwrap_or(crate::themes::DEFAULT_KEY)
                .to_owned(),
            themes: Vec::new(),
            theme_looks: Vec::new(),
            packages_read: false,
            templates_stale: false,
            keys: crate::commands::overrides_from(&prefs),
            user_packages: Vec::new(),
            engine_packages: Vec::new(),
            agent: prefs
                .get("agent")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            package_path: String::new(),
            index: crate::packages_window::IndexForm::from_prefs(&prefs),
            restyle: false,
            splash: options.splash.then_some(0.0),
            engines_config: options.engines.clone(),
            engines: Vec::new(),
            engine_warnings: Vec::new(),
            engine_stale: Vec::new(),
            engine_path: String::new(),
        };
        Self {
            options,
            welcome: Some(Box::new(welcome)),
            app: None,
            handed_off: None,
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

    /// Sets the status line (the welcome screen's, then the editor's).
    pub fn set_status(&mut self, status: String) {
        match (&mut self.app, &mut self.welcome) {
            (Some(app), _) => app.set_status(status),
            (None, Some(welcome)) => welcome.status = status,
            (None, None) => {}
        }
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

    /// The engine a project was handed to: it opens the project in its own
    /// window, and this one closes.
    pub fn handed_off(&self) -> Option<&wish_control::engines::Engine> {
        self.handed_off.as_ref().map(|hand_off| &hand_off.engine)
    }

    /// Hands the project in `root` to the engine version it was made with
    /// when that is another engine of this computer. `None`: this engine
    /// opens it; `Some(Err)`: that engine could not be started (this one
    /// opens it, saying so).
    fn hand_off(&mut self, root: &Path) -> Option<Result<(), String>> {
        let choice = wish_control::engines::choose(root, self.options.engines.as_deref());
        let engine = choice.switch()?.clone();
        let mut child = None;
        let mut log = None;
        if self.options.hand_off {
            let mut command = std::process::Command::new(engine.wen());
            // (No --lang: the choice is in the preferences, and an older
            // engine may not know a language this one has.)
            command.arg("edit").arg("--root").arg(root);
            if self.options.software {
                command.arg("--software");
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                /// The editor has its own window: no console behind it.
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                command.creation_flags(CREATE_NO_WINDOW);
            }
            // What it says on failing is kept, to show it here.
            let errors = wish_control::crash::machine_folder()
                .map(|folder| folder.join("hand-off-error.txt"))
                .and_then(|path| {
                    let _ = std::fs::create_dir_all(path.parent()?);
                    let file = std::fs::File::create(&path).ok()?;
                    Some((path, file))
                });
            let stderr = match errors {
                Some((path, file)) => {
                    log = Some(path);
                    std::process::Stdio::from(file)
                }
                None => std::process::Stdio::null(),
            };
            match command
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(stderr)
                .spawn()
            {
                Ok(started) => child = Some(started),
                Err(error) => {
                    return Some(Err(tf(
                        "Wish Engine {version} (the version this project was made with) could not be started: {error}. This engine opened it.",
                        &[("version", &engine.version), ("error", &error)],
                    )));
                }
            }
        }
        if let Some(welcome) = &mut self.welcome {
            welcome.status = tf(
                "opening {root} with Wish Engine {version}...",
                &[("root", &root.display()), ("version", &engine.version)],
            );
        }
        self.handed_off = Some(HandOff {
            engine,
            root: root.to_path_buf(),
            settled: child.is_none(),
            child,
            log,
            started: std::time::Instant::now(),
        });
        Some(Ok(()))
    }

    /// Watches a handed-off engine for its first moment: settled when it
    /// keeps running (or ends well); when it ends with an error, the
    /// project opens here with what it said.
    fn watch_hand_off(&mut self) {
        let Some(hand_off) = &mut self.handed_off else {
            return;
        };
        if hand_off.settled {
            return;
        }
        let Some(child) = &mut hand_off.child else {
            hand_off.settled = true;
            return;
        };
        match child.try_wait() {
            Ok(Some(status)) if !status.success() => {
                let said = hand_off
                    .log
                    .as_ref()
                    .and_then(|path| std::fs::read_to_string(path).ok())
                    .map(|text| text.trim().chars().take(600).collect::<String>())
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| status.to_string());
                let problem = tf(
                    "Wish Engine {version}, which this project was made with, stopped at once: {said} This engine opened it.",
                    &[("version", &hand_off.engine.version), ("said", &said)],
                );
                let root = hand_off.root.clone();
                self.handed_off = None;
                match self.open(&root, false) {
                    Ok(()) => {
                        if let Some(app) = &mut self.app {
                            app.set_status(problem);
                        }
                        self.welcome = None;
                    }
                    Err(message) => {
                        if let Some(welcome) = &mut self.welcome {
                            welcome.status = format!("{problem} {message}");
                        }
                    }
                }
            }
            Ok(Some(_)) => hand_off.settled = true,
            Ok(None) => {
                hand_off.settled = hand_off.started.elapsed().as_secs_f64() >= HAND_OFF_SECONDS;
            }
            // It cannot be watched: trust that it started.
            Err(_) => hand_off.settled = true,
        }
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
            let choice = wish_control::engines::choose(root, self.options.engines.as_deref());
            match engine_note(&choice) {
                Some(note) => note,
                None => tf("opened {root}", &[("root", &root.display())]),
            }
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
                Pending::Open(root) => match self.hand_off(&root) {
                    // Handed to another engine: the window closes.
                    Some(Ok(())) => return true,
                    Some(Err(problem)) => self.open(&root, false).map(|()| {
                        if let Some(app) = &mut self.app {
                            app.set_status(problem);
                        }
                    }),
                    None => self.open(&root, false),
                },
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
        crate::style::set_theme(welcome.theme);
        if let Some(played) = welcome.splash.as_mut() {
            *played += seconds;
            let skipped = events.iter().any(|event| {
                matches!(
                    event,
                    ShellEvent::Button { pressed: true, .. }
                        | ShellEvent::Key { pressed: true, .. }
                )
            });
            if skipped || *played >= SPLASH_SECONDS {
                welcome.splash = None;
                welcome.frames = 0;
            } else {
                let played = *played;
                return splash_frame(welcome, size, played, seconds);
            }
        }
        let changed = !events.is_empty()
            || welcome.frames < 3
            || (welcome.canvas.width, welcome.canvas.height) != size
            || welcome.editor.wants_repaint()
            || !welcome.status.is_empty() && welcome.frames < 5;
        if !changed {
            return false;
        }
        if (welcome.canvas.width, welcome.canvas.height) != size {
            welcome.canvas = Canvas::new(size.0, size.1, crate::style::pal().backdrop);
        }
        let egui_events = crate::app::convert(&events, welcome.pointer);
        for event in &events {
            match event {
                ShellEvent::Pointer { x, y } => welcome.pointer = Some((*x, *y)),
                ShellEvent::PointerLeft => welcome.pointer = None,
                _ => {}
            }
        }
        if std::mem::take(&mut welcome.templates_stale) {
            let chosen = welcome
                .template
                .and_then(|i| self.options.templates.get(i))
                .map(|template| template.id.clone());
            self.options.templates = wish_control::scaffold::all_templates();
            welcome.template = chosen.and_then(|id| {
                self.options
                    .templates
                    .iter()
                    .position(|template| template.id == id)
            });
            welcome.pictures_loaded = false;
            welcome.pictures.clear();
        }
        let templates = &self.options.templates;
        let claude = self.options.claude.as_deref();
        let mut asked = Asked::default();
        let mut editor = std::mem::take(&mut welcome.editor);
        let mut egui_events = Some(egui_events);
        let mut seconds = seconds;
        let preferences = self.options.preferences.clone();
        let frame = loop {
            if std::mem::take(&mut welcome.restyle) {
                editor.restyle();
            }
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
            let hub = std::mem::take(&mut asked.hub);
            if hub_act(welcome, hub, preferences.as_deref(), templates) {
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
            *pixel = crate::style::pal().backdrop;
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

    /// Whether the opening animation is playing.
    pub fn splash_playing(&self) -> bool {
        self.welcome.as_ref().is_some_and(|w| w.splash.is_some())
    }

    fn frame_app(&mut self, size: (u32, u32), seconds: f64, events: Vec<ShellEvent>) -> bool {
        self.app
            .as_mut()
            .is_some_and(|app| app.frame_canvas(size, seconds, events))
    }
}

impl Welcome {
    /// Reads the templates' pictures into textures, the first time.
    fn load_pictures(&mut self, context: &egui::Context, templates: &[Template]) {
        if self.pictures_loaded {
            return;
        }
        self.pictures_loaded = true;
        let options = egui::TextureOptions::LINEAR;
        self.pictures = templates
            .iter()
            .map(|template| {
                let bytes =
                    std::fs::read(template.path.join(".template").join("thumbnail.png")).ok()?;
                let image = decode_png(&bytes)?;
                Some(context.load_texture(format!("template-{}", template.id), image, options))
            })
            .collect();
        self.empty_picture = decode_png(EMPTY_PICTURE)
            .map(|image| context.load_texture("template-empty", image, options));
    }

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
    welcome.load_pictures(ui.ctx(), templates);
    if !welcome.packages_read {
        welcome.read_packages();
    }
    // As wide as the longest tab name in the person's language.
    let font = egui::TextStyle::Button.resolve(ui.style());
    let widest = crate::hub::Tab::ALL
        .iter()
        .map(|tab| {
            ui.painter()
                .layout_no_wrap(
                    tab.name().to_owned(),
                    font.clone(),
                    crate::style::pal().text,
                )
                .size()
                .x
        })
        .fold(0.0_f32, f32::max);
    // (Button padding 16 + icon 15 + gap 6, and the panel's margins 20.)
    let tabs_width = (widest + 16.0 + 15.0 + 6.0 + 20.0 + 4.0).max(150.0).ceil();
    egui::Panel::left("hub_tabs")
        .resizable(false)
        .exact_size(tabs_width)
        .frame(
            egui::Frame::new()
                .fill(crate::style::pal().panel)
                .inner_margin(egui::Margin::symmetric(10, 8)),
        )
        .show(ui, |ui| crate::hub::tabs(ui, welcome.tab, &mut asked.hub));
    egui::Panel::bottom("welcome_status")
        .frame(
            egui::Frame::new()
                .fill(crate::style::pal().raised)
                .inner_margin(egui::Margin::symmetric(16, 6)),
        )
        .show(ui, |ui| {
            let text = if welcome.status.is_empty() {
                RichText::new(t(
                    "Everything can be done by hand or with an assistant, and both land in the same history.",
                ))
                .color(crate::style::pal().weak)
            } else {
                RichText::new(welcome.status.as_str())
            };
            ui.label(text);
        });
    if welcome.tab == crate::hub::Tab::Projects {
        egui::Panel::bottom("welcome_action")
            .frame(
                egui::Frame::new()
                    .fill(crate::style::pal().panel)
                    .stroke(egui::Stroke::new(1.0, crate::style::pal().line))
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| action_bar(ui, welcome, &mut asked));
    }
    if welcome.tab != crate::hub::Tab::Projects {
        let mut path = std::mem::take(&mut welcome.package_path);
        let mut engine_path = std::mem::take(&mut welcome.engine_path);
        let config_folder = welcome.folder.clone();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(crate::style::pal().panel_backdrop()))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("hub_page")
                    .show(ui, |ui| {
                        let width = ui.available_width().min(900.0);
                        let side = ((ui.available_width() - width) * 0.5).max(0.0);
                        ui.horizontal(|ui| {
                            ui.add_space(side);
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                let view = crate::hub::HubView {
                                    theme_key: &welcome.theme_key,
                                    language: welcome.language,
                                    themes: &welcome.themes,
                                    user_packages: &welcome.user_packages,
                                    engine_packages: &welcome.engine_packages,
                                    agent: &welcome.agent,
                                    claude,
                                    templates,
                                    projects_folder: &config_folder,
                                    index: &welcome.index,
                                    engines: &welcome.engines,
                                    engine_warnings: &welcome.engine_warnings,
                                    engine_stale: &welcome.engine_stale,
                                    theme_looks: &welcome.theme_looks,
                                    keys: &welcome.keys,
                                };
                                match welcome.tab {
                                    crate::hub::Tab::Packages => {
                                        crate::hub::packages(ui, &view, &mut path, &mut asked.hub);
                                    }
                                    crate::hub::Tab::Look => {
                                        crate::hub::look(ui, &view, &mut asked.hub)
                                    }
                                    crate::hub::Tab::Ai => {
                                        crate::hub::ai(ui, &view, &mut asked.hub)
                                    }
                                    crate::hub::Tab::Engines => crate::hub::engines(
                                        ui,
                                        &view,
                                        &mut engine_path,
                                        &mut asked.hub,
                                    ),
                                    crate::hub::Tab::Learn => {
                                        crate::hub::learn(ui, &view, &mut asked.hub)
                                    }
                                    _ => crate::hub::settings(ui, &view, &mut asked.hub),
                                }
                            });
                        });
                    });
            });
        welcome.package_path = path;
        welcome.engine_path = engine_path;
        return asked;
    }
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(egui::Color32::from_rgb(
            crate::style::pal().backdrop[0],
            crate::style::pal().backdrop[1],
            crate::style::pal().backdrop[2],
        )))
        .show(ui, |ui| {
            // A faint dot grid, like drafting paper on the bench.
            let page = ui.max_rect();
            let dot = crate::style::pal().widget;
            let step = 22.0;
            let mut y = page.top() + step * 0.5;
            while y < page.bottom() {
                let mut x = page.left() + step * 0.5;
                while x < page.right() {
                    ui.painter().circle_filled(egui::pos2(x, y), 1.1, dot);
                    x += step;
                }
                y += step;
            }
            egui::ScrollArea::vertical()
                .id_salt("welcome_page")
                .show(ui, |ui| {
                    // The page is a centred column, wide enough for three
                    // template cards beside the open list.
                    let width = ui.available_width().min(1180.0);
                    let side = ((ui.available_width() - width) * 0.5).max(0.0);
                    ui.horizontal(|ui| {
                        ui.add_space(side);
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.add_space(22.0);
                            header(ui, welcome, &mut asked);
                            ui.add_space(18.0);
                            let gap = 18.0;
                            let left = ((width - gap) * 0.66).floor();
                            let right = width - gap - left;
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                ui.vertical(|ui| {
                                    ui.set_width(left);
                                    new_game(ui, welcome, templates, claude);
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(right);
                                    open_game(ui, welcome, &mut asked);
                                });
                            });
                            ui.add_space(22.0);
                        });
                    });
                });
        });
    asked
}

/// Searches the package index, or adds or updates one of its packages
/// for the person (the Hub works without a project, so it calls the
/// control plane's index directly).
fn index_act(
    welcome: &mut Welcome,
    ask: crate::packages_window::IndexAsk,
    preferences: Option<&Path>,
) {
    use crate::packages_window::IndexAsk;
    use wish_control::{package_index, packages};
    let given = welcome.index.index_arg();
    if let Some(path) = preferences {
        let _ = crate::prefs::update(path, |prefs| match &given {
            Some(folder) => {
                prefs.insert("package_index".to_owned(), json!(folder));
            }
            None => {
                prefs.remove("package_index");
            }
        });
    }
    let index = package_index::index_folder(given.map(PathBuf::from))
        .and_then(|folder| package_index::read_index(&folder));
    let index = match index {
        Ok(index) => index,
        Err(problem) => {
            welcome.index.results = None;
            welcome.index.problem = Some(format!("{} {}", problem.message, problem.hint));
            return;
        }
    };
    let here = Path::new("");
    let user = packages::Scope::User;
    // The same control-plane code the packages tool runs (the Hub has no
    // project, so there is no workspace to call).
    let done = match &ask {
        IndexAsk::Search => None,
        IndexAsk::Add(id, _) => Some(
            package_index::add(&index, id, None, user, here, false, false)
                .map(|done| crate::packages_window::index_status(&Value::Object(done.answer), id)),
        ),
        IndexAsk::Update(id, _) => Some(
            package_index::update(&index, Some(id), None, user, here, false)
                .map(|done| crate::packages_window::index_status(&Value::Object(done.answer), id)),
        ),
    };
    match done {
        Some(Ok(message)) => welcome.status = message,
        Some(Err(problem)) => welcome.status = format!("{} {}", problem.message, problem.hint),
        None => {}
    }
    if !matches!(ask, IndexAsk::Search) {
        welcome.read_packages();
        crate::themes::load_languages(None);
    }
    let query = welcome.index.query.borrow().trim().to_owned();
    welcome.index.results = Some(package_index::search(&index, &query, None));
    welcome.index.problem = None;
}

/// Does what a Hub tab asked for; whether anything changed.
fn hub_act(
    welcome: &mut Welcome,
    hub: crate::hub::HubAsked,
    preferences: Option<&Path>,
    templates: &[Template],
) -> bool {
    let mut acted = false;
    let save = |change: &dyn Fn(&mut serde_json::Map<String, Value>)| {
        if let Some(path) = preferences {
            let _ = crate::prefs::update(path, |prefs| change(prefs));
        }
    };
    if let Some(tab) = hub.tab {
        welcome.tab = tab;
        acted = true;
    }
    if let Some(key) = hub.theme {
        match crate::themes::load(&welcome.themes, &key) {
            Ok(theme) => {
                welcome.theme = theme;
                crate::style::set_theme(theme);
                welcome.theme_key = key.clone();
                welcome.restyle = true;
                save(&|prefs| {
                    prefs.insert("theme".to_owned(), json!(key));
                });
            }
            Err(problem) => {
                welcome.status = tf(
                    "the theme could not be used: {problem}",
                    &[("problem", &problem)],
                );
            }
        }
        acted = true;
    }
    if let Some(code) = hub.language {
        welcome.set_language(code);
        save(&|prefs| {
            prefs.insert("language".to_owned(), json!(code));
        });
        acted = true;
    }
    if let Some(agent) = hub.agent {
        welcome.agent = agent.clone();
        save(&|prefs| {
            prefs.insert("agent".to_owned(), Value::Object(agent.clone()));
        });
        acted = true;
    }
    let here = Path::new(".");
    let user = wish_control::packages::Scope::User;
    if let Some(folder) = hub.install {
        let from = PathBuf::from(folder.trim());
        welcome.status = if !from.is_absolute() {
            t("Write the package folder's full path.").to_owned()
        } else {
            // The same control-plane code as the packages tool's install.
            match wish_control::packages::install_from(&from, user, here, false, false) {
                Ok(done) => {
                    welcome.package_path.clear();
                    let answer = Value::Object(done.answer);
                    let mut line = crate::packages_window::index_status(&answer, "");
                    for warning in answer["diagnostics"].as_array().into_iter().flatten() {
                        line.push_str(" · ");
                        line.push_str(warning["message"].as_str().unwrap_or(""));
                    }
                    line
                }
                Err(problem) if problem.code == "package_exists" => tf(
                    "{problem} Remove the installed one first, then install this one.",
                    &[("problem", &problem.message)],
                ),
                Err(problem) => format!("{} {}", problem.message, problem.hint),
            }
        };
        welcome.read_packages();
        crate::themes::load_languages(None);
        welcome.templates_stale = true;
        acted = true;
    }
    if let Some(ask) = hub.index {
        index_act(welcome, ask, preferences);
        welcome.templates_stale = true;
        acted = true;
    }
    if let Some(id) = hub.remove {
        welcome.status = match wish_control::packages::remove(&id, user, here) {
            Ok(_) => tf("{id} moved to the trash", &[("id", &id)]),
            Err(problem) => format!("{} {}", problem.message, problem.hint),
        };
        welcome.read_packages();
        welcome.templates_stale = true;
        acted = true;
    }
    if let Some(place) = hub.open {
        if let Err(error) = crate::app::open_with_os(Path::new(&place)) {
            welcome.status = error;
        }
        acted = true;
    }
    if let Some(id) = hub.template {
        welcome.template = templates.iter().position(|template| template.id == id);
        welcome.tab = crate::hub::Tab::Projects;
        acted = true;
    }
    if hub.add_engine.is_some() || hub.remove_engine.is_some() {
        welcome.status = engines_act(welcome, hub.add_engine, hub.remove_engine);
        welcome.read_engines();
        acted = true;
    }
    acted
}

/// A line for the person when a project opens with an engine that does
/// not fit it (in the interface's language); `None` when it fits.
pub fn engine_note(choice: &wish_control::engines::Choice) -> Option<String> {
    let project = choice.project?;
    if choice.picked.is_some() {
        return None;
    }
    let here = wish_control::packages::Version::engine();
    Some(if project > here {
        tf(
            "This project was made with Wish Engine {project}, newer than this one ({here}): install {project} or newer and open it there.",
            &[("project", &project), ("here", &here)],
        )
    } else {
        tf(
            "This project was made with Wish Engine {project}; no engine compatible with it is installed, so {here} opens it. Its files may change.",
            &[("project", &project), ("here", &here)],
        )
    })
}

/// Adds or removes an engine (the Engine versions tab); the line to show.
fn engines_act(welcome: &mut Welcome, add: Option<String>, remove: Option<PathBuf>) -> String {
    let Some(config) = welcome.engines_config.clone() else {
        return t("There is no settings folder to keep the list of engines in.").to_owned();
    };
    // The same control-plane code as the engines tool (and `wen engines`).
    let act = |action: &str, folder: &Path| {
        wish_control::engines_act_in(Some(&config), action, Some(folder), None, false)
    };
    let version = |answer: &serde_json::Map<String, Value>| {
        answer["engine"]["version"]
            .as_str()
            .unwrap_or("?")
            .to_owned()
    };
    let done = if let Some(folder) = add {
        let folder = PathBuf::from(folder.trim());
        if !folder.is_absolute() {
            return t("Write the engine folder's full path.").to_owned();
        }
        act("add", &folder).map(|answer| {
            welcome.engine_path.clear();
            if answer["added"] == true {
                tf(
                    "Wish Engine {version} added",
                    &[("version", &version(&answer))],
                )
            } else {
                tf(
                    "Wish Engine {version} was already here",
                    &[("version", &version(&answer))],
                )
            }
        })
    } else if let Some(folder) = remove {
        act("remove", &folder).map(|answer| {
            if answer["stale"] == true {
                tf("{folder} was forgotten", &[("folder", &folder.display())])
            } else {
                tf(
                    "Wish Engine {version} removed",
                    &[("version", &version(&answer))],
                )
            }
        })
    } else {
        return String::new();
    };
    done.unwrap_or_else(|problem| format!("{} {}", problem.message, problem.hint))
}

/// How long the opening animation lasts: the mark's animation, then a
/// moment still.
const SPLASH_SECONDS: f64 = 2.0;
/// The mark's own animation, within it.
const MARK_SECONDS: f64 = 1.4;

/// A frame of the opening animation: the wand is waved in the
/// middle, the name fades in under it.
fn splash_frame(welcome: &mut Welcome, size: (u32, u32), played: f64, seconds: f64) -> bool {
    if (welcome.canvas.width, welcome.canvas.height) != size {
        welcome.canvas = Canvas::new(size.0, size.1, crate::style::pal().backdrop);
    }
    let mut editor = std::mem::take(&mut welcome.editor);
    let t = played / MARK_SECONDS;
    let frame = editor.layout(size, seconds, Vec::new(), |ui| {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let area = ui.max_rect();
                let side = (area.height() * 0.28).clamp(96.0, 220.0);
                let mark = egui::Rect::from_center_size(
                    area.center() - egui::vec2(0.0, side * 0.25),
                    egui::vec2(side, side),
                );
                let texture = crate::vector::logo_moment(ui.ctx(), side.round() as u16, t);
                ui.painter().image(
                    texture.id(),
                    mark,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                // The name comes in as the bone settles.
                let shown = ((t - 0.55) / 0.35).clamp(0.0, 1.0) as f32;
                if shown > 0.0 {
                    let color = crate::style::pal().text.gamma_multiply(shown);
                    ui.painter().text(
                        egui::pos2(area.center().x, mark.bottom() + 18.0 + 8.0 * (1.0 - shown)),
                        egui::Align2::CENTER_TOP,
                        "Wish Engine",
                        egui::FontId::proportional((side * 0.2).clamp(22.0, 40.0)),
                        color,
                    );
                }
            });
    });
    for pixel in welcome.canvas.pixels.as_chunks_mut::<4>().0 {
        *pixel = crate::style::pal().backdrop;
    }
    editor.paint(&frame, &mut welcome.canvas);
    welcome.editor = editor;
    true
}

/// The engine's mark, its name, what it is, and the language.
fn header(ui: &mut Ui, welcome: &Welcome, asked: &mut Asked) {
    ui.horizontal(|ui| {
        let (logo, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
        crate::style::paint_logo(ui.painter(), logo);
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(t("Welcome to Wish Engine")).size(26.0).color(crate::style::pal().text));
            ui.label(
                RichText::new(t(
                    "Make a game by hand, with an assistant, or both: everything can be done either way.",
                ))
                .color(crate::style::pal().weak),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            egui::ComboBox::from_id_salt("welcome_language")
                .selected_text(crate::i18n::find(welcome.language).map_or("English", |l| l.name))
                .show_ui(ui, |ui| {
                    for language in crate::i18n::all_languages() {
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
}

/// A raised card with room inside.
fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(crate::style::pal().raised)
        .stroke(egui::Stroke::new(1.0, crate::style::pal().line))
        .corner_radius(egui::CornerRadius::same(5))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A step's title: its number in a circle and what it asks.
fn step_title(ui: &mut Ui, number: u8, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 3.0, crate::style::pal().accent);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            number.to_string(),
            egui::FontId::proportional(12.5),
            crate::style::on_accent(),
        );
        // The number is drawn; the title keeps only the words.
        let words = text.split_once(". ").map_or(text, |(_, rest)| rest);
        ui.label(
            RichText::new(words)
                .size(15.0)
                .color(crate::style::pal().text),
        );
    });
}

/// A card to choose (one of a set): raised, bordered in the accent colour
/// when chosen. `paint` draws what is in it. Its accessible name is `name`.
fn choice(
    ui: &mut Ui,
    size: egui::Vec2,
    chosen: bool,
    enabled: bool,
    name: &str,
    paint: impl FnOnce(&egui::Painter, egui::Rect, bool),
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, enabled, chosen, name));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if response.hovered() && enabled {
            crate::style::pal().hover
        } else if chosen {
            crate::style::pal().widget
        } else {
            crate::style::pal().raised
        };
        painter.rect_filled(rect, 4.0, fill);
        let stroke = if chosen {
            egui::Stroke::new(2.0, crate::style::pal().select)
        } else {
            egui::Stroke::new(1.0, crate::style::pal().line)
        };
        painter.rect_stroke(rect, 4.0, stroke, egui::StrokeKind::Inside);
        paint(painter, rect.shrink(12.0), enabled);
    }
    response
}

/// The left column: a new game, in three steps.
fn new_game(ui: &mut Ui, welcome: &mut Welcome, templates: &[Template], claude: Option<&Path>) {
    use crate::style::Icon;
    card(ui, |ui| {
        ui.label(
            RichText::new(t("Start a new game"))
                .size(19.0)
                .color(crate::style::pal().text),
        );
        ui.add_space(10.0);

        step_title(ui, 1, t("1. Who helps you?"));
        ui.add_space(4.0);
        let width = ui.available_width();
        let half = ((width - 10.0) * 0.5).floor();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            // The program's own name; the whole path when pointed at.
            let found = claude.map(|program| {
                let name = program
                    .file_name()
                    .map_or_else(|| program.display().to_string(), |n| n.to_string_lossy().into_owned());
                tf("found: {program}", &[("program", &name)])
            });
            let claude_text = found.clone().unwrap_or_else(|| {
                t("Claude Code was not found: install it and sign in once (claude), then open Wish Engine again.")
                    .to_owned()
            });
            let response = choice(
                ui,
                egui::vec2(half, 74.0),
                welcome.assistant == Assistant::Claude,
                claude.is_some(),
                t("Claude Code"),
                |painter, rect, enabled| {
                    option_face(painter, rect, Icon::Chat, t("Claude Code"), &claude_text, enabled);
                },
            );
            let response = match claude {
                Some(program) => response.on_hover_text(program.display().to_string()),
                None => response,
            };
            if response.clicked() {
                welcome.assistant = Assistant::Claude;
            }
            let response = choice(
                ui,
                egui::vec2(half, 74.0),
                welcome.assistant == Assistant::Nobody,
                true,
                t("Nobody: I make it by hand"),
                |painter, rect, enabled| {
                    option_face(
                        painter,
                        rect,
                        Icon::Hand,
                        t("Nobody: I make it by hand"),
                        t("Any MCP agent can connect later: Project menu, Connect an outside agent."),
                        enabled,
                    );
                },
            );
            if response.clicked() {
                welcome.assistant = Assistant::Nobody;
            }
        });

        ui.add_space(14.0);
        step_title(ui, 2, t("2. Start from"));
        ui.add_space(4.0);
        // The showcase first: it shows the most of the engine; the empty
        // project last.
        let mut order: Vec<Option<usize>> = (0..templates.len()).map(Some).collect();
        order.sort_by_key(|index| index.is_none_or(|i| templates[i].id != "islands"));
        order.push(None);
        let columns = 3;
        let gap = 10.0;
        let tile_w = ((width - gap * (columns as f32 - 1.0)) / columns as f32).floor();
        let picture_h = (tile_w * 0.5).floor();
        let tile = egui::vec2(tile_w, picture_h + 66.0);
        for row in order.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for &index in row {
                    let (name, about_text, picture) = match index {
                        Some(i) => (
                            templates[i].name.as_str(),
                            about(&templates[i].id).unwrap_or(""),
                            welcome.pictures.get(i).and_then(Option::as_ref),
                        ),
                        None => (
                            t("Empty project"),
                            t("A camera, a sun, a ground and starter game code."),
                            welcome.empty_picture.as_ref(),
                        ),
                    };
                    let response = choice(
                        ui,
                        tile,
                        welcome.template == index,
                        true,
                        name,
                        |painter, rect, _| {
                            // The picture fills the top, edge to edge.
                            let outer = rect.expand(12.0);
                            let picture_rect = egui::Rect::from_min_size(
                                outer.min + egui::vec2(1.0, 1.0),
                                egui::vec2(outer.width() - 2.0, picture_h),
                            );
                            match picture {
                                Some(texture) => {
                                    painter.add(
                                        egui::epaint::RectShape::filled(
                                            picture_rect,
                                            egui::CornerRadius {
                                                nw: 3,
                                                ne: 3,
                                                sw: 0,
                                                se: 0,
                                            },
                                            egui::Color32::WHITE,
                                        )
                                        .with_texture(
                                            texture.id(),
                                            egui::Rect::from_min_max(
                                                egui::pos2(0.0, 0.0),
                                                egui::pos2(1.0, 1.0),
                                            ),
                                        ),
                                    );
                                }
                                None => {
                                    painter.rect_filled(
                                        picture_rect,
                                        3.0,
                                        crate::style::pal().widget,
                                    );
                                }
                            }
                            let text_top = picture_rect.bottom() + 8.0;
                            painter.text(
                                egui::pos2(outer.left() + 12.0, text_top),
                                egui::Align2::LEFT_TOP,
                                name,
                                egui::FontId::proportional(14.0),
                                crate::style::pal().text,
                            );
                            let galley = painter.layout(
                                about_text.to_owned(),
                                egui::FontId::proportional(11.5),
                                crate::style::pal().weak,
                                outer.width() - 24.0,
                            );
                            painter.galley(
                                egui::pos2(outer.left() + 12.0, text_top + 20.0),
                                galley,
                                crate::style::pal().weak,
                            );
                        },
                    );
                    if response.clicked() {
                        welcome.template = index;
                    }
                }
            });
            ui.add_space(gap - ui.spacing().item_spacing.y);
        }
    });
}

/// The bar at the bottom of the welcome screen: the new game's name and
/// folder and Create, always in sight whatever the window's height.
fn action_bar(ui: &mut Ui, welcome: &mut Welcome, asked: &mut Asked) {
    let width = ui.available_width().min(1180.0);
    let side = ((ui.available_width() - width) * 0.5).max(0.0);
    ui.horizontal(|ui| {
        ui.add_space(side);
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.add_space(4.0);
            step_title(ui, 3, t("3. Name and place"));
            ui.horizontal(|ui| {
                let field = ((width - 260.0) * 0.5).max(120.0);
                ui.label(RichText::new(t("Name")).color(crate::style::pal().weak));
                ui.add(
                    egui::TextEdit::singleline(&mut welcome.name)
                        .desired_width(field * 0.7)
                        .margin(egui::vec2(8.0, 5.0)),
                );
                ui.label(RichText::new(t("Folder")).color(crate::style::pal().weak));
                ui.add(
                    egui::TextEdit::singleline(&mut welcome.folder)
                        .desired_width(field * 1.3)
                        .margin(egui::vec2(8.0, 5.0)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let busy = welcome.pending.is_some();
                    let create = egui::Button::new(
                        RichText::new(t("Create"))
                            .size(15.5)
                            .color(crate::style::on_accent()),
                    )
                    .fill(crate::style::pal().accent)
                    .corner_radius(egui::CornerRadius::same(3))
                    .min_size(egui::vec2(140.0, 34.0));
                    if ui.add_enabled(!busy, create).clicked() {
                        asked.create = true;
                    }
                });
            });
            ui.label(
                RichText::new(tf(
                    "It goes in: {path}",
                    &[(
                        "path",
                        &Path::new(welcome.folder.trim())
                            .join(slug(&welcome.name))
                            .display(),
                    )],
                ))
                .small()
                .color(crate::style::pal().weak),
            );
            ui.add_space(4.0);
        });
    });
}

/// What is drawn in an assistant's card: its icon, its name, a line under.
fn option_face(
    painter: &egui::Painter,
    rect: egui::Rect,
    icon: crate::style::Icon,
    title: &str,
    detail: &str,
    enabled: bool,
) {
    let icon_rect =
        egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 2.0), egui::vec2(26.0, 26.0));
    let color = if enabled {
        crate::style::pal().accent
    } else {
        crate::style::pal().weak
    };
    crate::style::paint(painter, icon_rect, icon, color);
    let left = icon_rect.right() + 12.0;
    painter.text(
        egui::pos2(left, rect.top() + 2.0),
        egui::Align2::LEFT_TOP,
        title,
        egui::FontId::proportional(14.5),
        if enabled {
            crate::style::pal().text
        } else {
            crate::style::pal().weak
        },
    );
    let galley = painter.layout(
        detail.to_owned(),
        egui::FontId::proportional(11.5),
        crate::style::pal().weak,
        rect.right() - left,
    );
    painter.galley(
        egui::pos2(left, rect.top() + 22.0),
        galley,
        crate::style::pal().weak,
    );
}

/// The right column: a game made before.
fn open_game(ui: &mut Ui, welcome: &mut Welcome, asked: &mut Asked) {
    use crate::style::Icon;
    card(ui, |ui| {
        ui.label(
            RichText::new(t("Open a game"))
                .size(19.0)
                .color(crate::style::pal().text),
        );
        ui.add_space(10.0);
        if welcome.recent.is_empty() {
            ui.label(
                RichText::new(t("Projects you open are listed here."))
                    .color(crate::style::pal().weak),
            );
        } else {
            ui.label(RichText::new(t("Recent projects")).color(crate::style::pal().weak));
            ui.add_space(2.0);
            for (number, root) in welcome.recent.iter().enumerate() {
                let made = welcome.recent_made.get(number).cloned().flatten();
                let name = Path::new(root)
                    .file_name()
                    .map_or_else(|| root.clone(), |name| name.to_string_lossy().into_owned());
                let width = ui.available_width();
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(width, 46.0), egui::Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, name.as_str())
                });
                if ui.is_rect_visible(rect) {
                    let painter = ui.painter();
                    if response.hovered() {
                        painter.rect_filled(rect, 3.0, crate::style::pal().hover);
                    }
                    let icon = egui::Rect::from_center_size(
                        egui::pos2(rect.left() + 20.0, rect.center().y),
                        egui::vec2(20.0, 20.0),
                    );
                    crate::style::paint(painter, icon, Icon::Folder, crate::style::pal().amber);
                    painter.text(
                        egui::pos2(rect.left() + 40.0, rect.top() + 7.0),
                        egui::Align2::LEFT_TOP,
                        &name,
                        egui::FontId::proportional(14.0),
                        crate::style::pal().text,
                    );
                    let line = match &made {
                        Some(version) => format!(
                            "{}  ·  {}",
                            tf("made with Wish Engine {version}", &[("version", version)]),
                            root
                        ),
                        None => root.clone(),
                    };
                    let path = painter.layout_no_wrap(
                        line,
                        egui::FontId::proportional(11.0),
                        crate::style::pal().weak,
                    );
                    let clip = egui::Rect::from_min_max(
                        egui::pos2(rect.left() + 40.0, rect.top()),
                        egui::pos2(rect.right() - 8.0, rect.bottom()),
                    );
                    painter.with_clip_rect(clip).galley(
                        egui::pos2(rect.left() + 40.0, rect.top() + 26.0),
                        path,
                        crate::style::pal().weak,
                    );
                }
                if response.on_hover_text(root.as_str()).clicked() {
                    asked.open = Some(PathBuf::from(root));
                }
            }
        }
        ui.add_space(14.0);
        ui.label(RichText::new(t("A project folder")).color(crate::style::pal().weak));
        ui.horizontal(|ui| {
            let field = ui.available_width() - 70.0;
            ui.add(
                egui::TextEdit::singleline(&mut welcome.open_path)
                    .hint_text(t("the folder with wish.json"))
                    .desired_width(field)
                    .margin(egui::vec2(8.0, 5.0)),
            );
            if ui.button(t("Open")).clicked() && !welcome.open_path.trim().is_empty() {
                asked.open = Some(PathBuf::from(welcome.open_path.trim()));
            }
        });
    });
    ui.add_space(14.0);
    card(ui, |ui| {
        ui.label(
            RichText::new(t("Good to know"))
                .size(16.0)
                .color(crate::style::pal().text),
        );
        ui.add_space(6.0);
        for (icon, color, tip) in [
            (
                Icon::Play,
                crate::style::pal().go,
                t("Ctrl+P plays the game in the view; Stop brings the scene back."),
            ),
            (
                Icon::Chat,
                crate::style::pal().accent,
                t("The Agent panel: point at things in the scene and say what to change."),
            ),
            (
                Icon::Undo,
                crate::style::pal().text,
                t("Every change, yours or an agent's, can be undone on its own (History)."),
            ),
            (
                Icon::Folder,
                crate::style::pal().amber,
                t("Project menu, Export game: a folder that runs without the engine."),
            ),
        ] {
            ui.horizontal_top(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                crate::style::paint(ui.painter(), rect.shrink(1.0), icon, color);
                ui.add(egui::Label::new(RichText::new(tip).color(crate::style::pal().weak)).wrap());
            });
            ui.add_space(4.0);
        }
    });
}

/// A PNG file's pixels as an image egui can show (none if it cannot be read).
fn decode_png(bytes: &[u8]) -> Option<egui::ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    decoder.set_limits(png::Limits {
        bytes: 16 * 1024 * 1024,
    });
    let mut reader = decoder.read_info().ok()?;
    let mut data = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut data).ok()?;
    let (width, height) = (info.width as usize, info.height as usize);
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None,
    };
    let mut rgba = Vec::with_capacity(width * height * 4);
    for pixel in data
        .get(..width * height * channels)?
        .chunks_exact(channels)
    {
        rgba.extend_from_slice(&match channels {
            4 => [pixel[0], pixel[1], pixel[2], pixel[3]],
            3 => [pixel[0], pixel[1], pixel[2], 255],
            2 => [pixel[0], pixel[0], pixel[0], pixel[1]],
            _ => [pixel[0], pixel[0], pixel[0], 255],
        });
    }
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [width, height],
        &rgba,
    ))
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

/// Whether files can be made in `folder` (made if missing): tried, not
/// guessed, since a security setting can refuse what permissions allow.
fn writable(folder: &Path) -> bool {
    let probe = folder.join(".wish-write-probe");
    let ok = std::fs::create_dir_all(folder).is_ok() && std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
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
        self.watch_hand_off();
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
    std::sync::LazyLock::new(|| Canvas::new(1, 1, crate::style::pal().backdrop));

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
        // A project handed to another engine opens in its window.
        (self
            .handed_off
            .as_ref()
            .is_some_and(|hand_off| hand_off.settled)
            && self.options.hand_off)
            || self.app.as_ref().is_some_and(|app| app.wants_exit())
    }

    fn take_clipboard(&mut self) -> Option<String> {
        match (&mut self.app, &mut self.welcome) {
            (Some(app), _) => app.take_clipboard(),
            (None, Some(welcome)) => welcome.editor.take_copied(),
            (None, None) => None,
        }
    }
}
