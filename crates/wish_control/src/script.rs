//! The project's game code: the built module, its types in every world the
//! workspace makes, and reloading it into running sessions (M5, design
//! note §7, §11).

use std::sync::Arc;

use serde_json::{Value, json};
use wish_app::System;
use wish_core::{Diagnostic, Result};
use wish_script::build::{BuildReport, GAME_WASM, SCRIPTS_ENTRY};
use wish_script::{GameModule, Limits, ScriptEngine, ScriptRuntime};

use crate::workspace::Workspace;

/// The workspace's game code state.
#[derive(Default)]
pub(crate) struct Scripts {
    engine: Option<ScriptEngine>,
    /// Where the SDK and the compiler are, if set explicitly.
    pub toolchain: Option<wish_script::build::Toolchain>,
    /// The module in use (the last good build).
    pub module: Option<Arc<GameModule>>,
    /// The last build's report.
    pub last_build: Option<BuildReport>,
    /// Why the built module on disk could not be loaded, if it could not.
    pub load_problem: Option<Diagnostic>,
    /// Bumped whenever the module changes, so cached views are rebuilt.
    pub generation: u64,
}

/// The name of the system that propagates transforms after every other one.
pub(crate) const PROPAGATION: &str = "transform_propagation";

impl Workspace {
    /// The WebAssembly engine, started on first use.
    pub(crate) fn script_engine(&mut self) -> Result<ScriptEngine> {
        if let Some(engine) = &self.scripts.engine {
            return Ok(engine.clone());
        }
        let engine = ScriptEngine::new(Limits::default())?;
        self.scripts.engine = Some(engine.clone());
        Ok(engine)
    }

    /// Runs game code with these limits instead of the defaults (tests,
    /// measurements). Takes effect for modules loaded afterwards.
    pub fn set_script_limits(&mut self, limits: Limits) -> Result<()> {
        self.scripts.engine = Some(ScriptEngine::new(limits)?);
        Ok(())
    }

    /// Uses this SDK pack and compiler for `script_build` instead of looking
    /// for them next to the program (tests, development setups).
    pub fn set_toolchain(&mut self, toolchain: wish_script::build::Toolchain) {
        self.scripts.toolchain = Some(toolchain);
    }

    /// The SDK pack and compiler `script_build` uses.
    pub(crate) fn toolchain(&self) -> Result<wish_script::build::Toolchain> {
        match &self.scripts.toolchain {
            Some(toolchain) => Ok(toolchain.clone()),
            None => wish_script::build::Toolchain::locate(),
        }
    }

    /// Loads the module a previous build left in `.wish/build`, so that a
    /// reopened project has its game types without building again.
    pub(crate) fn load_built_module(&mut self) {
        let path = self.root().join(GAME_WASM);
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let loaded = self
            .script_engine()
            .and_then(|engine| GameModule::load(&engine, GAME_WASM, &bytes));
        match loaded {
            Ok(module) => {
                if let Err(problem) = self.use_module(module) {
                    self.scripts.load_problem = Some(problem);
                }
            }
            Err(problem) => self.scripts.load_problem = Some(problem),
        }
    }

    /// Makes `module` the project's game code: its types join the type
    /// registry the documents are checked against.
    pub(crate) fn use_module(&mut self, module: Arc<GameModule>) -> Result<()> {
        let mut types = wish_app::App::new(wish_app::AppConfig::default());
        crate::types::register_all(self.example(), Some(&module), types.world_mut())?;
        self.types = types;
        self.scripts.module = Some(module);
        self.scripts.load_problem = None;
        self.scripts.generation += 1;
        Ok(())
    }

    /// The module in use, if any.
    pub(crate) fn game_module(&self) -> Option<&Arc<GameModule>> {
        self.scripts.module.as_ref()
    }

    /// Registers every type a world of this project needs: the engine's,
    /// the example's and the game module's.
    pub(crate) fn register_types(&self, world: &mut wish_ecs::World) -> Result<()> {
        crate::types::register_all(self.example(), self.game_module().map(Arc::as_ref), world)
    }

    /// The game code part of `project_info`.
    pub(crate) fn script_status(&self) -> Value {
        let sources = self.root().join(SCRIPTS_ENTRY).is_file();
        let module = self.game_module().map(|module| {
            let manifest = module.manifest();
            json!({
                "file": GAME_WASM,
                "fingerprint": module.fingerprint().to_string(),
                "systems": manifest.systems.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
                "types": manifest
                    .types
                    .iter()
                    .filter(|t| self.engine_type(&t.schema.name).is_none())
                    .map(|t| t.schema.name.clone())
                    .collect::<Vec<_>>(),
            })
        });
        json!({
            "sources": if sources { Value::from(SCRIPTS_ENTRY) } else { Value::Null },
            "module": module,
            "last_build": self.scripts.last_build.as_ref().map(|r| json!({
                "passed": r.passed, "errors": r.errors, "warnings": r.warnings, "seconds": r.seconds,
            })),
            "problem": self.scripts.load_problem.as_ref().map(|p| p.message.clone()),
        })
    }

    /// Whether `name` is one of the engine's own types (not the game's).
    fn engine_type(&self, name: &str) -> Option<()> {
        // An app's world: it also has the app's own resources (`Time`).
        let mut app = wish_app::App::new(wish_app::AppConfig::default());
        crate::types::register_project_types(self.example(), app.world_mut()).ok()?;
        let registry = app.world().registry();
        (registry.component_id(name).is_some()
            || registry.resource_id(name).is_some()
            || registry.event_id(name).is_some())
        .then_some(())
    }

    /// Puts the module in use into every open session, between two ticks:
    /// the game's state is kept (it lives in the world), the systems are
    /// replaced (each keeps its random stream), new types are added.
    /// Returns one note per session.
    pub(crate) fn reload_sessions(&mut self) -> Vec<Diagnostic> {
        let Some(module) = self.scripts.module.clone() else {
            return Vec::new();
        };
        let ids: Vec<String> = self.sessions.open.keys().cloned().collect();
        let mut notes = Vec::new();
        for id in ids {
            let Some(session) = self.sessions.open.get_mut(&id) else {
                continue;
            };
            let note = match reload_session(session, &module) {
                Ok(changed) => {
                    notes.extend(changed);
                    Diagnostic::info(
                        "script_reloaded",
                        format!(
                            "Session {id} now runs the new game code from tick {}; its state was kept.",
                            session.app.tick()
                        ),
                        "Step it with sim_step to see the new behaviour.",
                    )
                }
                Err(problem) => Diagnostic::warning(
                    "script_reload_failed",
                    format!(
                        "Session {id} keeps running the old game code: {}",
                        problem.message
                    ),
                    format!(
                        "Restart it (sim_stop, then sim_start) to use the new code. {}",
                        problem.hint
                    ),
                ),
            };
            let tick = session.app.tick();
            self.sessions.record(Some(&id), Some(tick), "script", &note);
            notes.push(note);
        }
        notes
    }
}

/// Refuses a module with a system named like one of the engine's own (or
/// the project example's): it would replace that system, or be replaced
/// by it, without a word (M5 closing review).
pub(crate) fn check_system_names(example: Option<&str>, module: &GameModule) -> Result<()> {
    let mut reserved = vec![PROPAGATION.to_owned(), wish_physics::SYSTEM.to_owned()];
    if let Some(example) = example {
        let mut app = wish_app::App::new(wish_app::AppConfig::default());
        if wish_examples::add_systems(example, &mut app).is_ok() {
            reserved.extend(app.system_states().map(|(name, _)| name.to_owned()));
        }
    }
    for system in &module.manifest().systems {
        if reserved.contains(&system.name) {
            return Err(Diagnostic::error(
                "script_system_name_taken",
                format!(
                    "The game system '{}' has the name of one of the engine's own systems.",
                    system.name
                ),
                format!(
                    "Rename the function. Names the engine uses: {}.",
                    reserved.join(", ")
                ),
            )
            .at_file(SCRIPTS_ENTRY));
        }
    }
    Ok(())
}

/// The systems of a session for a module, and the propagation system that
/// runs after all of them.
pub(crate) fn add_game_systems(app: &mut wish_app::App, runtime: &ScriptRuntime) -> Result<()> {
    for system in runtime.systems() {
        app.replace_system(system)?;
    }
    Ok(())
}

/// The engine's systems that run after the game's: physics (after every
/// game and example system: it moves what they pushed), then transform
/// propagation (after everything).
pub(crate) fn propagation_after_all(app: &mut wish_app::App) -> Result<()> {
    let mut names: Vec<String> = app
        .system_states()
        .map(|(name, _)| name.to_owned())
        .filter(|name| name != PROPAGATION && name != wish_physics::SYSTEM)
        .collect();
    names.sort();
    let mut physics = wish_physics::system();
    for name in &names {
        physics = physics.after(name);
    }
    app.replace_system(physics)?;
    let mut propagation = System::new(PROPAGATION, |world, _| {
        wish_transform::update_global_transforms(world)
    })
    .after(wish_physics::SYSTEM);
    for name in &names {
        propagation = propagation.after(name);
    }
    app.replace_system(propagation)
}

/// Puts `module` into a session; returns what happened to changed types.
fn reload_session(
    session: &mut crate::sessions::Session,
    module: &Arc<GameModule>,
) -> Result<Vec<Diagnostic>> {
    // The world before the new types: the session's snapshots are moved to
    // the new layout from it, so going back still works after new code.
    let before = session.app.world().clone();
    let mut changed = module.install(session.app.world_mut())?;
    let runtime = ScriptRuntime::new(Arc::clone(module));
    if let Some(old) = &session.script {
        let keep: Vec<&str> = module
            .manifest()
            .systems
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        for gone in &old.module().manifest().systems {
            if !keep.contains(&gone.name.as_str()) {
                session.app.remove_system(&gone.name);
            }
        }
    }
    add_game_systems(&mut session.app, &runtime)?;
    propagation_after_all(&mut session.app)?;
    session.script = Some(runtime);
    for saved in &mut session.saved {
        let mut world = before.clone();
        let moved = world
            .restore(saved.snapshot.world())
            .and_then(|()| module.install(&mut world));
        match moved {
            Ok(_) => saved.snapshot = session.app.rebase(&saved.snapshot, world.snapshot()),
            Err(problem) => changed.push(Diagnostic::warning(
                "script_reloaded",
                format!(
                    "Snapshot '{}' could not be moved to the new code and cannot be restored: {}",
                    saved.label, problem.message
                ),
                "Take a new snapshot with sim_snapshot.",
            )),
        }
    }
    if session.recording.take().is_some() {
        changed.push(Diagnostic::warning(
            "script_reloaded",
            "The session was recording a replay; new code stops the recording (a replay plays one version of the game).",
            "Start a new session and record again with the new code.",
        ));
    }
    Ok(changed)
}

impl Workspace {
    /// The project's actions: `input.json`, or none if the project has no
    /// such file. A broken file is an error (the same one `check` reports).
    pub(crate) fn input_map(&self) -> Result<wish_input::InputMap> {
        let path = self.root().join(wish_input::INPUT_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => wish_input::InputMap::parse(&text, wish_input::INPUT_FILE),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(wish_input::InputMap::default())
            }
            Err(error) => Err(Diagnostic::error(
                "file_unreadable",
                format!("Could not read '{}': {error}.", wish_input::INPUT_FILE),
                "Check that the file can be read.",
            )),
        }
    }
}

/// Playing a game in real time (`wen play`): the same sessions the tools
/// use, driven by a window instead of by `sim_step`.
impl Workspace {
    /// Lets `frames` ticks pass in session `id` with the named keys held
    /// (the names `input.json` uses). Returns what looks wrong (a system
    /// that failed, an inspector finding).
    pub fn play_step(
        &mut self,
        id: &str,
        frames: u64,
        keys: &std::collections::BTreeSet<String>,
    ) -> Result<Vec<Diagnostic>> {
        let values = self.session(id)?.input.evaluate(keys);
        let (stepped, ..) = self.step_session(id, frames, None, &values)?;
        Ok(stepped.diagnostics)
    }

    /// What session `id` looks like now, for drawing.
    pub fn play_scene(&self, id: &str) -> Result<wish_render::RenderScene> {
        wish_render::extract(self.session(id)?.app.world())
    }
}
