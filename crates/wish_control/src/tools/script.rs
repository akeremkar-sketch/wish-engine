//! Game code: `script_build` (design note M5 §11).

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;
use wish_script::GameModule;
use wish_script::build::{GAME_WASM, SCRIPTS_ENTRY, build_game};

use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const SCRIPT_BUILD: Tool = Tool {
    name: "script_build",
    title: "Build the game code",
    description: "Compiles the project's game code (scripts/src/lib.rs, Rust with wish_sdk) to WebAssembly. Compiler errors come back as diagnostics with file, line and hint; a failed build keeps the previous code running. On success the new code's types become usable in scenes, and running sessions switch to it between two ticks without losing their state (reload, on by default). dry_run only checks that it compiles.",
    params: &[
        Param::optional(
            "reload",
            Kind::Boolean,
            "Switch running sessions to the new code (their state is kept).",
        )
        .or("true"),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "Only check that the code compiles and loads; change nothing.",
        ),
    ],
    mutates: true,
    run: script_build,
};

fn script_build(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let dry_run = args.flag("dry_run");
    let root = workspace.root().to_path_buf();
    if !root.join(SCRIPTS_ENTRY).is_file() {
        return Err(Diagnostic::error(
            "script_sources_missing",
            format!("The project has no game code: '{SCRIPTS_ENTRY}' does not exist."),
            "Game code lives in scripts/src/lib.rs (projects made with `wen new` have one). Create it with: use wish_sdk::prelude::*; #[system] functions; wish_sdk::export! { systems: [...] }.",
        )
        .at_file(SCRIPTS_ENTRY));
    }
    let toolchain = workspace.toolchain()?;
    let report = build_game(&toolchain, &root, !dry_run)?;
    let mut out = Map::new();
    let mut diagnostics = report.diagnostics.clone();
    if let Value::Object(build) = report.to_json() {
        for (key, value) in build {
            if key != "diagnostics" {
                out.insert(key, value);
            }
        }
    }
    let mut reloaded = Vec::new();
    if report.passed {
        let file = report
            .output
            .clone()
            .unwrap_or_else(|| GAME_WASM.to_owned());
        let bytes = std::fs::read(root.join(&file)).map_err(|error| {
            Diagnostic::error(
                "script_build_failed",
                format!("The built module '{file}' could not be read: {error}."),
                "Build again; if it keeps failing, check the project folder can be read.",
            )
        })?;
        let engine = workspace.script_engine()?;
        let loaded = GameModule::load(&engine, GAME_WASM, &bytes).and_then(|module| {
            // The types must fit the engine's before anything is replaced.
            let mut world = wish_ecs::World::new();
            crate::types::register_all(workspace.example(), Some(&module), &mut world)?;
            Ok(module)
        });
        match loaded {
            Ok(module) => {
                let manifest = module.manifest();
                out.insert(
                    "systems".to_owned(),
                    json!(
                        manifest
                            .systems
                            .iter()
                            .map(|s| s.name.clone())
                            .collect::<Vec<_>>()
                    ),
                );
                out.insert(
                    "fingerprint".to_owned(),
                    json!(module.fingerprint().to_string()),
                );
                if dry_run {
                    let _ = std::fs::remove_file(root.join(&file));
                } else {
                    workspace.use_module(module)?;
                    if args.flag("reload") {
                        reloaded = workspace.reload_sessions();
                    }
                }
            }
            Err(problem) => {
                out.insert("passed".to_owned(), json!(false));
                diagnostics.push(problem);
            }
        }
    }
    if !dry_run {
        workspace.scripts.last_build = Some(report);
    }
    diagnostics.extend(reloaded);
    out.insert("dry_run".to_owned(), json!(dry_run));
    out.insert("diagnostics".to_owned(), json!(diagnostics));
    Ok(out)
}
