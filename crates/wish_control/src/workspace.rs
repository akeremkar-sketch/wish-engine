//! The workspace: one project, opened by one process (the MCP server, a
//! `wen do` run, later the editor), with everything the tools share.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use wish_app::{App, AppConfig};
use wish_core::{Diagnostic, Result};
use wish_ecs::Registry;
use wish_scene::Actor;

use crate::project::{ProjectConfig, read_project};
use crate::tool::{Tool, catalog};

/// How to open a workspace.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceOptions {
    /// The project folder.
    pub root: PathBuf,
    /// A built-in example whose types to use, overriding `wish.json` (the
    /// command line's `--example`, kept for projects without `wish.json`).
    pub example: Option<String>,
}

/// One open project.
pub struct Workspace {
    root: PathBuf,
    config: Option<ProjectConfig>,
    /// Why `wish.json` could not be read, if it could not.
    config_problem: Option<Diagnostic>,
    /// The example whose types are registered.
    example: Option<String>,
    /// Holds the type registry the project's documents are checked against.
    types: App,
}

impl std::fmt::Debug for Workspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workspace")
            .field("root", &self.root)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Workspace {
    /// Opens the project in `options.root`. A missing or broken `wish.json`
    /// does not stop it: the tools still work with the engine's types, and
    /// `project_info` and `check` report the problem.
    pub fn open(options: WorkspaceOptions) -> Result<Self> {
        if !options.root.is_dir() {
            return Err(Diagnostic::error(
                "project_not_found",
                format!("'{}' is not a folder.", options.root.display()),
                "Give the project folder (--root), or create a project with `wen new <folder>`.",
            ));
        }
        let (config, config_problem) = match read_project(&options.root) {
            Ok(config) => (config, None),
            Err(problem) => (None, Some(problem)),
        };
        if let Some(name) = &options.example
            && !wish_examples::NAMES.contains(&name.as_str())
        {
            return Err(Diagnostic::error(
                "unknown_example",
                format!("There is no built-in example called '{name}'."),
                wish_core::suggest::unknown_name_hint(name, wish_examples::NAMES),
            )
            .with_candidates(wish_examples::NAMES.iter().copied()));
        }
        let example = options
            .example
            .clone()
            .or_else(|| config.as_ref().and_then(|config| config.example.clone()));
        let mut types = App::new(AppConfig::default());
        crate::types::register_project_types(example.as_deref(), types.world_mut())?;
        Ok(Self {
            root: options.root,
            config,
            config_problem,
            example,
            types,
        })
    }

    /// The project folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What `wish.json` says, if the project has a readable one.
    pub fn config(&self) -> Option<&ProjectConfig> {
        self.config.as_ref()
    }

    /// Why `wish.json` could not be read.
    pub fn config_problem(&self) -> Option<&Diagnostic> {
        self.config_problem.as_ref()
    }

    /// The built-in example whose types are in use.
    pub fn example(&self) -> Option<&str> {
        self.example.as_deref()
    }

    /// The types the project's documents use.
    pub fn registry(&self) -> &Registry {
        self.types.world().registry()
    }

    /// Runs one tool and returns its answer: `{ "ok": true, ... }` or
    /// `{ "ok": false, "error": Diagnostic }`. Never fails and never panics
    /// out: a bug inside a tool comes back as an `internal_error` answer, so
    /// one bad call cannot take the server down.
    pub fn call(&mut self, tool: &str, args: &Value, actor: &Actor) -> Value {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run(tool, args, actor)
        }))
        .unwrap_or_else(|_| {
            Err(Diagnostic::error(
                "internal_error",
                format!("The tool '{tool}' failed unexpectedly."),
                "This is a bug in the engine. The project is unchanged; please report the call that caused it.",
            ))
        });
        envelope(outcome)
    }

    fn run(&mut self, name: &str, args: &Value, actor: &Actor) -> Result<Map<String, Value>> {
        let tool = find_tool(name)?;
        let args = crate::param::parse(tool.name, tool.params, args)?;
        (tool.run)(self, &args, actor)
    }
}

/// The tool called `name`, or an `unknown_tool` diagnostic.
pub fn find_tool(name: &str) -> Result<&'static Tool> {
    catalog()
        .iter()
        .find(|tool| tool.name == name)
        .ok_or_else(|| {
            let names: Vec<&str> = catalog().iter().map(|tool| tool.name).collect();
            Diagnostic::error(
                "unknown_tool",
                format!("There is no tool called '{name}'."),
                wish_core::suggest::unknown_name_hint(name, &names),
            )
            .with_candidates(names)
        })
}

/// The answer form of a tool's outcome.
pub fn envelope(outcome: Result<Map<String, Value>>) -> Value {
    match outcome {
        Ok(mut map) => {
            map.insert("ok".to_owned(), Value::Bool(true));
            Value::Object(map)
        }
        Err(problem) => json!({ "ok": false, "error": problem }),
    }
}

/// Reads an actor's name: `human`, `file`, or anything else as an agent.
pub fn parse_actor(name: &str) -> Actor {
    match name {
        "human" | "" => Actor::Human,
        "file" => Actor::File,
        other => Actor::Agent(other.to_owned()),
    }
}
