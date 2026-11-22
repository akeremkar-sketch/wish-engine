//! The Wish Engine player (M9): a game in a window, in real time, as the
//! person plays it. An exported game ships this program and nothing else of
//! the engine's tools: no editor, no MCP server, no compiler (ADR-023).
//! `wen play` uses the same game with building and hot reloading on.
//!
//! The game runs as a play session of the control plane (the one `sim_start`
//! makes), stepped at 60 ticks a second with the keys held as the game's
//! actions, so it plays exactly as it does in the editor and in tests: the
//! same scene, seed and inputs give the same world, bit for bit.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_core::{Diagnostic, Entity, Result};
use wish_render::{Gpu, RenderScene, Renderer, View};
use wish_script::build::SCRIPTS_ENTRY;
use wish_window::{Content, Loaded, Pointing, ViewerOptions, fingerprint_dir};

/// Ticks per second the game runs at.
pub const TICK_RATE: f64 = 60.0;
/// The most ticks one picture may catch up on (a stall must not make the
/// game race to make up the time).
const MAX_CATCH_UP: u64 = 5;

/// How the game is played.
#[derive(Clone, Debug)]
pub struct PlayOptions {
    /// The project folder (an exported game's `game/`).
    pub root: PathBuf,
    /// The scene to start (`None`: the project's default).
    pub scene: Option<String>,
    /// The window's size.
    pub size: (u32, u32),
    /// Draw with the software adapter.
    pub software: bool,
    /// Close after this many frames (for scripts).
    pub exit_after: Option<u64>,
    /// Build the game code first and rebuild it when its sources change
    /// (`wen play`); an exported game has its code built already.
    pub develop: bool,
    /// The window's title (`None`: the project's name).
    pub title: Option<String>,
    /// Players: more than one hosts an online match over the network
    /// simulator with bots in the other seats, and the person plays the
    /// host's seat (`wen play --players`, release 1.1).
    pub players: usize,
}

/// Loads every model the scene's Model entities name from the project and
/// uploads it, and with them the pictures, material files and textures the
/// scene uses. A model that cannot be loaded is reported as a warning and
/// not drawn (the rest of the scene still is); its import warnings are
/// passed on.
pub fn load_models(
    scene: &RenderScene,
    root: &Path,
    gpu: &Gpu,
    renderer: &mut Renderer,
) -> Vec<Diagnostic> {
    load_assets(
        scene,
        root,
        gpu,
        renderer,
        &mut std::collections::BTreeMap::new(),
        &mut wish_control::render::MaterialStamps::default(),
    )
}

/// [`load_models`], remembering when pictures, material files and textures
/// were read, so that a later call reads only what changed.
fn load_assets(
    scene: &RenderScene,
    root: &Path,
    gpu: &Gpu,
    renderer: &mut Renderer,
    pictures: &mut std::collections::BTreeMap<String, wish_control::render::Stamp>,
    materials: &mut wish_control::render::MaterialStamps,
) -> Vec<Diagnostic> {
    let mut warnings = Vec::new();
    let mut paths: Vec<&str> = scene
        .draws
        .iter()
        .filter_map(|draw| draw.model_path.as_deref())
        .map(|path| path.split('#').next().unwrap_or(path))
        .collect();
    paths.sort_unstable();
    paths.dedup();
    for path in paths {
        let loaded = wish_import::load_model_file(&root.join(path), path)
            .and_then(|model| renderer.upload_model(gpu, path, &model).map(|()| model));
        match loaded {
            Ok(model) => warnings.extend(model.warnings),
            Err(problem) => warnings.push(
                Diagnostic::warning(
                    "model_not_loaded",
                    format!("The model '{path}' was not drawn: {}", problem.message),
                    problem.hint.clone(),
                )
                .at_file(path),
            ),
        }
    }
    warnings.extend(wish_control::render::load_ui_pictures(
        renderer, scene, root, pictures,
    ));
    warnings.extend(wish_control::render::load_materials(
        gpu, renderer, scene, root, materials,
    ));
    warnings
}

/// The game, as window content.
pub struct Game {
    /// The sound card, where there is one to open (Windows and Linux).
    #[cfg(any(windows, target_os = "linux"))]
    speaker: Option<wish_audio::output::Speaker>,
    workspace: Workspace,
    root: PathBuf,
    scene: Option<String>,
    develop: bool,
    players: usize,
    session: Option<String>,
    /// Real time owed to the game, in ticks.
    owed: f64,
    /// What the game reported while it ran (a system that failed).
    pub messages: Vec<String>,
    /// The pictures loaded so far, and when their files were read.
    pictures: std::collections::BTreeMap<String, wish_control::render::Stamp>,
    /// The same for material files and the textures of materials.
    materials: wish_control::render::MaterialStamps,
    /// What could not be loaded, said once.
    reported: BTreeSet<String>,
}

impl Game {
    /// The game of the project in `options.root`.
    pub fn open(options: &PlayOptions) -> Result<Self> {
        let mut workspace = Workspace::open(WorkspaceOptions {
            root: options.root.clone(),
            example: None,
        })?;
        workspace.set_software(options.software);
        Ok(Self {
            #[cfg(any(windows, target_os = "linux"))]
            speaker: wish_audio::output::Speaker::open().ok(),
            workspace,
            root: options.root.clone(),
            scene: options.scene.clone(),
            develop: options.develop,
            players: options.players.max(1),
            session: None,
            owed: 0.0,
            messages: Vec::new(),
            pictures: std::collections::BTreeMap::new(),
            materials: wish_control::render::MaterialStamps::default(),
            reported: BTreeSet::new(),
        })
    }

    /// The project's name, for the window.
    pub fn name(&mut self) -> String {
        let info = self.run_tool("project_info", &json!({}));
        info["project"]["name"]
            .as_str()
            .unwrap_or("Wish Engine")
            .to_owned()
    }

    /// Makes the speakers match the sounds the game says are playing.
    #[cfg(any(windows, target_os = "linux"))]
    fn hear(&mut self, id: &str) {
        let Some(speaker) = self.speaker.as_mut() else {
            return;
        };
        if let Ok(voices) = self.workspace.play_voices(id) {
            let root = self.root.clone();
            speaker.sync(&voices, &|path| std::fs::read(root.join(path)).ok());
        }
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    fn hear(&mut self, _id: &str) {}

    fn run_tool(&mut self, tool: &str, args: &Value) -> Value {
        self.workspace.call(tool, args, &parse_actor("human"))
    }

    /// Builds the game code (if the project has any, and only when
    /// developing) and reports what the compiler said.
    fn build(&mut self) -> Vec<Diagnostic> {
        if !self.develop || !self.root.join(SCRIPTS_ENTRY).is_file() {
            return Vec::new();
        }
        let answer = self.run_tool("script_build", &json!({}));
        if answer["ok"] != true {
            return serde_json::from_value::<Diagnostic>(answer["error"].clone())
                .into_iter()
                .collect();
        }
        answer["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| serde_json::from_value::<Diagnostic>(d.clone()).ok())
            .collect()
    }

    fn start(&mut self) -> Result<String> {
        let mut args = json!({});
        if let Some(scene) = &self.scene {
            args["scene"] = json!(scene);
        }
        let answer = if self.players > 1 {
            args["action"] = json!("host");
            // The person plays one seat as a predicting player over a
            // simulated internet (a headless host in this process, bots in
            // the other seats), so the game feels as it will online.
            args["players"] = json!(self.players);
            args["bots"] = json!(self.players - 1);
            args["auto_start"] = json!(true);
            args["play_as"] = json!("player");
            args["network"] = wish_control::play_network();
            self.run_tool("net", &args)
        } else {
            self.run_tool("sim_start", &args)
        };
        if answer["ok"] != true {
            return Err(
                serde_json::from_value::<Diagnostic>(answer["error"].clone()).unwrap_or_else(
                    |_| {
                        Diagnostic::error(
                            "internal_error",
                            "The game could not be started.",
                            "Run `wen check` to see what is wrong with the project.",
                        )
                    },
                ),
            );
        }
        let id = answer["session"].as_str().unwrap_or("s1").to_owned();
        self.session = Some(id.clone());
        Ok(id)
    }

    fn scene_now(&mut self) -> Result<RenderScene> {
        let id = self.session.clone().unwrap_or_default();
        self.workspace.play_scene(&id)
    }
}

impl Content for Game {
    fn load(&mut self, gpu: &Gpu, renderer: &mut Renderer) -> Result<Loaded> {
        let mut warnings = self.build();
        if warnings.iter().any(Diagnostic::is_error) && self.session.is_none() {
            // Nothing runs yet and the code does not compile: say so.
            return Err(warnings.remove(0));
        }
        if self.session.is_none() {
            self.start()?;
        }
        let scene = self.scene_now()?;
        warnings.extend(load_assets(
            &scene,
            &self.root,
            gpu,
            renderer,
            &mut self.pictures,
            &mut self.materials,
        ));
        Ok(Loaded { scene, warnings })
    }

    fn refresh(
        &mut self,
        gpu: &Gpu,
        renderer: &mut Renderer,
        scene: &RenderScene,
    ) -> Vec<Diagnostic> {
        // Models the game started using since the last picture: only the
        // new ones are read (the rest are loaded already).
        let mut fresh: Vec<&str> = scene
            .draws
            .iter()
            .filter_map(|draw| draw.model_path.as_deref())
            .map(|path| path.split('#').next().unwrap_or(path))
            .filter(|path| !renderer.has_model(path) && !self.reported.contains(*path))
            .collect();
        fresh.sort_unstable();
        fresh.dedup();
        let mut problems = Vec::new();
        for path in fresh {
            if wish_core::asset::validate_asset_ref(path).is_err() {
                self.reported.insert(path.to_owned());
                continue;
            }
            let loaded = wish_import::load_model_file(&self.root.join(path), path)
                .and_then(|model| renderer.upload_model(gpu, path, &model));
            if let Err(problem) = loaded {
                self.reported.insert(path.to_owned());
                problems.push(problem);
            }
        }
        let pictures =
            wish_control::render::load_ui_pictures(renderer, scene, &self.root, &mut self.pictures);
        // Material files and textures: the ones the game started using, and
        // the ones whose files were saved since the last picture.
        let materials = wish_control::render::load_materials(
            gpu,
            renderer,
            scene,
            &self.root,
            &mut self.materials,
        );
        for problem in pictures.into_iter().chain(materials) {
            let key = problem.location.file.clone().unwrap_or_default();
            if self.reported.insert(key) {
                problems.push(problem);
            }
        }
        problems
    }

    fn name_of(&mut self, entity: Entity) -> String {
        entity.to_string()
    }

    fn fingerprint(&mut self) -> u64 {
        // The game code, when developing: a saved change builds and
        // reloads. An exported game does not change.
        if self.develop {
            fingerprint_dir(&self.root.join("scripts"), &["rs"])
        } else {
            0
        }
    }

    fn live(&self) -> bool {
        true
    }

    fn play(
        &mut self,
        keys: &BTreeSet<String>,
        pointing: Option<Pointing>,
        seconds: f64,
    ) -> Option<Result<RenderScene>> {
        let id = self.session.clone()?;
        self.owed += seconds * TICK_RATE;
        let whole = self.owed.floor();
        self.owed -= whole;
        let ticks = (whole as u64).min(MAX_CATCH_UP);
        if ticks > 0 {
            let pointer = pointing.map(|at| wish_control::PointerAt {
                x: at.x,
                y: at.y,
                width: at.width as f32,
                height: at.height as f32,
            });
            let stepped = self.workspace.play_step(&id, ticks, keys, pointer);
            self.hear(&id);
            match stepped {
                Ok(problems) => {
                    for problem in problems {
                        self.messages.push(format!("{problem}"));
                    }
                }
                Err(problem) => return Some(Err(problem)),
            }
        }
        Some(self.scene_now())
    }
}

/// What a played game reports when its window closes.
#[derive(Clone, Debug)]
pub struct Played {
    /// Frames drawn.
    pub frames: u64,
    /// Times the game code was rebuilt and reloaded.
    pub code_reloads: u64,
    /// Rebuilds that did not compile (the game went on with the old code).
    pub rejected: u64,
    /// What the game reported while it ran.
    pub messages: Vec<String>,
}

/// Plays the game in a window until it is closed.
pub fn play(options: &PlayOptions, on_message: Box<dyn FnMut(&str)>) -> Result<Played> {
    let mut game = Game::open(options)?;
    let title = options.title.clone().unwrap_or_else(|| game.name());
    let report = wish_window::run(
        ViewerOptions {
            title,
            size: options.size,
            software: options.software,
            shaders: None,
            view: View::Lit,
            exit_after_frames: options.exit_after,
            on_message,
        },
        &mut game,
    )?;
    Ok(Played {
        frames: report.frames,
        code_reloads: report.scene_reloads,
        rejected: report.rejected,
        messages: game.messages,
    })
}

/// Plays the game without a window for `frames` ticks, with no keys held,
/// and says where it ended: the world's hash (the same project gives the
/// same hash everywhere: a test that an exported game plays as the project
/// does).
pub fn play_headless(root: &Path, scene: Option<&str>, frames: u64) -> Result<Value> {
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: root.to_path_buf(),
        example: None,
    })?;
    let human = parse_actor("human");
    let mut start = json!({});
    if let Some(scene) = scene {
        start["scene"] = json!(scene);
    }
    let started = workspace.call("sim_start", &start, &human);
    let error = |answer: &Value| {
        serde_json::from_value::<Diagnostic>(answer["error"].clone()).unwrap_or_else(|_| {
            Diagnostic::error(
                "internal_error",
                "The game could not be run.",
                "Run `wen check` on the project to see what is wrong.",
            )
        })
    };
    if started["ok"] != true {
        return Err(error(&started));
    }
    let stepped = workspace.call("sim_step", &json!({ "frames": frames }), &human);
    if stepped["ok"] != true {
        return Err(error(&stepped));
    }
    Ok(json!({
        "ok": true,
        "ticks": stepped["tick"],
        "world_hash": stepped["world_hash"],
    }))
}
