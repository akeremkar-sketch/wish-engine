//! Game code: `script_build` (design note M5 §11).

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;
use wish_script::GameModule;
use wish_script::build::{CHECKED_WASM, GAME_WASM, SCRIPTS_ENTRY, build_game};

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
    // The new module is built beside the old one and checked (types,
    // systems) before it replaces anything: a module that compiles but
    // cannot be used must not become the project's build (M5 closing
    // review).
    let mut report = build_game(&toolchain, &root, false)?;
    explain_stale_sdk(workspace, &mut report.diagnostics);
    let mut out = Map::new();
    let mut reloaded = Vec::new();
    let mut fingerprint = None;
    let mut systems = None;
    if report.passed {
        let checked = root.join(CHECKED_WASM);
        let loaded = std::fs::read(&checked)
            .map_err(|error| {
                Diagnostic::error(
                    "script_build_failed",
                    format!("The built module '{CHECKED_WASM}' could not be read: {error}."),
                    "Build again; if it keeps failing, check the project folder can be read.",
                )
            })
            .and_then(|bytes| {
                let engine = workspace.script_engine()?;
                GameModule::load(&engine, GAME_WASM, &bytes)
            })
            .and_then(|module| {
                // The types and systems must fit the engine's before
                // anything is replaced.
                let mut world = wish_ecs::World::new();
                crate::types::register_all(workspace.example(), Some(&module), &mut world)?;
                crate::script::check_system_names(workspace.example(), &module)?;
                Ok(module)
            });
        let installed = loaded.and_then(|module| {
            if dry_run {
                let _ = std::fs::remove_file(&checked);
            } else {
                std::fs::rename(&checked, root.join(GAME_WASM)).map_err(|error| {
                    Diagnostic::error(
                        "script_build_failed",
                        format!("The built module could not be put in place: {error}."),
                        "Check that .wish/build can be written; the previous build stays in use.",
                    )
                })?;
            }
            Ok(module)
        });
        match installed {
            Ok(module) => {
                systems = Some(
                    module
                        .manifest()
                        .systems
                        .iter()
                        .map(|s| s.name.clone())
                        .collect::<Vec<_>>(),
                );
                fingerprint = Some(module.fingerprint().to_string());
                if !dry_run {
                    workspace.use_module(module)?;
                    if args.flag("reload") {
                        reloaded = workspace.reload_sessions();
                    }
                }
            }
            Err(problem) => {
                let _ = std::fs::remove_file(&checked);
                report.passed = false;
                report.errors += 1;
                report.output = None;
                report.diagnostics.push(problem);
            }
        }
    }
    if let Value::Object(build) = report.to_json() {
        for (key, value) in build {
            if key != "diagnostics" {
                out.insert(key, value);
            }
        }
    }
    if report.passed && !dry_run {
        out.insert("output".to_owned(), json!(GAME_WASM));
    }
    if let Some(systems) = systems {
        out.insert("systems".to_owned(), json!(systems));
    }
    if let Some(fingerprint) = fingerprint {
        out.insert("fingerprint".to_owned(), json!(fingerprint));
    }
    let mut diagnostics = report.diagnostics.clone();
    if !dry_run {
        workspace.scripts.last_build = Some(report);
    }
    diagnostics.extend(reloaded);
    out.insert("dry_run".to_owned(), json!(dry_run));
    out.insert("diagnostics".to_owned(), json!(diagnostics));
    Ok(out)
}

/// A compile error about an engine type or function
/// the SDK does not know (`cannot find type `CharacterMotion``) while the
/// engine itself has it means the SDK pack is older than the engine: say
/// so, instead of letting the author hunt for a typo (M6 closing review).
fn explain_stale_sdk(workspace: &Workspace, diagnostics: &mut [Diagnostic]) {
    let registry = workspace.registry();
    let engine_names = ["text", "text_of", "raycast"];
    for diagnostic in diagnostics.iter_mut() {
        if diagnostic.code != "script_compile_error" {
            continue;
        }
        let Some(name) = diagnostic
            .message
            .split("cannot find ")
            .nth(1)
            .and_then(|rest| rest.split('`').nth(1))
        else {
            continue;
        };
        // Engine-internal types game code never sees are not the SDK's.
        let internal = ["SceneId", "Hierarchy"].contains(&name);
        let known = !internal && registry.component_id(name).is_some()
            || !internal && registry.resource_id(name).is_some()
            || !internal && registry.event_id(name).is_some()
            || engine_names.contains(&name);
        if known {
            diagnostic.hint = format!(
                "'{name}' is part of this engine, but the game SDK the code is compiled against does not have it: the SDK pack is older than the engine. Run `wen sdk` (engine development) or reinstall the engine so that its 'sdk' folder matches."
            );
        }
    }
}
