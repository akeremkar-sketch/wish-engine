//! `wen`, the Wish Engine command-line tool.
//!
//! Every command can print one JSON document (`--json`) in the same envelope
//! the MCP tools use: `{ "ok": true, ... }` or `{ "ok": false, "error": <diagnostic> }`.
//! Without `--json` it prints text for people.
//!
//! ```text
//! wen run --example bounce --frames 10000 --seed 7 --json
//! wen schema --example bounce --json
//! wen examples
//! ```

mod args;
mod project_cmds;
mod scene;

use std::process::ExitCode;
use std::time::Instant;

use serde_json::{Map, Value, json};
use wish_app::{App, AppConfig};
use wish_core::{Diagnostic, Result};
use wish_ecs::{Registry, TypeInfo};
use wish_schema::json_schema;

use crate::args::{Args, Command, parse};
use crate::project_cmds::Outcome;
use crate::scene::load_scene_app;

const USAGE: &str = "\
wen - the Wish Engine command-line tool

USAGE:
    wen run --example <name> [--scene <file> [--root <folder>]] [--frames <n>] [--seed <n>]
            [--hash-every <n>] [--headless] [--json]
    wen bench [--entities <n>] [--ticks <n>] [--seed <n>] [--json]
    wen check [files...] --example <name> [--root <folder>] [--json]
    wen fmt [files...] --example <name> [--root <folder>] [--check] [--json]
    wen migrate [files...] [--root <folder>] [--json]
    wen watch --example <name> --scene <file> [--root <folder>] [--seconds <n>] [--json]
    wen mv <from> <to> --example <name> [--root <folder>] [--dry-run] [--json]
    wen schema [--example <name>] [--json]
    wen examples [--json]
    wen help | version

COMMANDS:
    run        Simulate a built-in example without a window and report its final state
               (frames default to 600, seed to 0). Runs are deterministic: the same
               seed and frame count always give the same hashes.
               With --scene, the scene file (a path relative to --root, default: the
               current folder) gives the entities and resources instead, and the
               example only supplies the component types. Prefabs are found in --root.
    bench      Time the core on many moving entities (defaults: 100000 entities, 600 ticks).
               Build with --release for meaningful numbers.
    check      Check every prefab, scene and meta file of the project (syntax, schema,
               references, asset files) and report all problems with file, line and
               column. Exits with failure if there is any error.
    fmt        Rewrite documents in canonical form; --check only lists the ones that are
               not (for CI). Files with errors are skipped and reported.
    migrate    Upgrade documents of older format versions (format 1 is the only one so far).
    watch      Load a scene and reload it when files change; an invalid change is reported
               and the last good scene stays. With --json, one JSON line per event.
    mv         Move an asset file or folder (paths relative to --root) and update every
               asset reference to it in scenes and prefabs, all or nothing. The example
               supplies the component types that tell where references are.
    schema     Print the JSON Schema of every registered component, resource and event.
    examples   List the built-in examples.

Add --json to any command for a single machine-readable JSON document.";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let json = arguments.iter().any(|argument| argument == "--json");
    match parse(&arguments).and_then(execute_any) {
        Ok(outcome) => {
            println!("{}", outcome.text);
            if outcome.success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(diagnostic) => {
            if json {
                println!("{}", json!({ "ok": false, "error": diagnostic }));
            } else {
                eprintln!("{diagnostic}");
            }
            ExitCode::FAILURE
        }
    }
}

/// Runs a parsed command. The project commands report problems in their own
/// output and decide the exit status themselves; the others succeed with
/// their output or fail with one diagnostic.
fn execute_any(args: Args) -> Result<Outcome> {
    let root_of = |root: &Option<String>| std::path::PathBuf::from(root.as_deref().unwrap_or("."));
    match &args.command {
        Command::Check {
            paths,
            example,
            root,
        } => project_cmds::check(paths, example, &root_of(root), args.json),
        Command::Format {
            paths,
            example,
            root,
            check,
        } => project_cmds::format(paths, example, &root_of(root), *check, args.json),
        Command::Migrate { paths, root } => project_cmds::migrate(paths, &root_of(root), args.json),
        Command::Watch {
            example,
            scene,
            root,
            seconds,
        } => project_cmds::watch(example, scene, &root_of(root), *seconds, args.json),
        _ => execute(args).map(|text| Outcome {
            text,
            success: true,
        }),
    }
}

/// Runs a parsed command and returns what to print.
fn execute(args: Args) -> Result<String> {
    match args.command {
        Command::Run {
            example,
            frames,
            seed,
            hash_every,
            scene,
            root,
        } => run(
            &RunOptions {
                example: &example,
                frames,
                seed,
                hash_every,
                scene: scene.as_deref(),
                root: root.as_deref(),
            },
            args.json,
        ),
        Command::Bench {
            entities,
            ticks,
            seed,
        } => bench(entities, ticks, seed, args.json),
        Command::Move {
            from,
            to,
            example,
            root,
            dry_run,
        } => move_asset(&from, &to, &example, root.as_deref(), dry_run, args.json),
        Command::Check { .. }
        | Command::Format { .. }
        | Command::Migrate { .. }
        | Command::Watch { .. } => Err(Diagnostic::error(
            "internal_error",
            "A project command was run through the simple path.",
            "This is a bug in wen. Please report it.",
        )),
        Command::Schema { example } => schema(example.as_deref(), args.json),
        Command::Examples => Ok(if args.json {
            json!({ "ok": true, "examples": wish_examples::NAMES }).to_string()
        } else {
            wish_examples::NAMES.join("\n")
        }),
        Command::Help => Ok(if args.json {
            json!({ "ok": true, "usage": USAGE }).to_string()
        } else {
            USAGE.to_owned()
        }),
        Command::Version => Ok(if args.json {
            json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }).to_string()
        } else {
            format!("wen {}", env!("CARGO_PKG_VERSION"))
        }),
    }
}

struct RunOptions<'a> {
    example: &'a str,
    frames: u64,
    seed: u64,
    hash_every: u64,
    scene: Option<&'a str>,
    root: Option<&'a str>,
}

fn run(options: &RunOptions, as_json: bool) -> Result<String> {
    let RunOptions {
        example,
        frames,
        seed,
        hash_every,
        scene,
        root,
    } = *options;
    let config = AppConfig {
        seed,
        ..AppConfig::default()
    };
    let mut warnings: Vec<Diagnostic> = Vec::new();
    let mut scene_entities: Option<usize> = None;
    let mut app = match scene {
        Some(scene) => {
            let root = std::path::Path::new(root.unwrap_or("."));
            let loaded = load_scene_app(example, scene, root, config)?;
            warnings = loaded.warnings;
            scene_entities = Some(loaded.scene_entities);
            loaded.app
        }
        None => wish_examples::build(example, config)?,
    };
    // Wall-clock time is measured here, outside the simulation; it never
    // reaches the world.
    let started = Instant::now();
    let mut failures: Vec<Diagnostic> = Vec::new();
    let mut checkpoints: Vec<Value> = Vec::new();
    while app.tick() < frames {
        failures.extend(app.step()?.failures);
        if hash_every > 0 && app.tick().is_multiple_of(hash_every) {
            checkpoints.push(json!({ "tick": app.tick(), "app_hash": app.hash().to_string() }));
        }
    }
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let world = app.world();
    if as_json {
        let mut output = json!({
            "ok": true,
            "example": example,
            "scene": scene,
            "scene_entities": scene_entities,
            "warnings": warnings,
            "seed": seed,
            "ticks": app.tick(),
            "entities": world.entity_count(),
            "world_hash": world.hash().to_string(),
            "app_hash": app.hash().to_string(),
            "failures": failures,
            "elapsed_ms": (elapsed_ms * 1000.0).round() / 1000.0,
        });
        if hash_every > 0
            && let Some(object) = output.as_object_mut()
        {
            object.insert("checkpoints".to_owned(), Value::Array(checkpoints));
        }
        return Ok(output.to_string());
    }
    let mut text = format!(
        "{example} - seed {seed} - {} ticks in {elapsed_ms:.1} ms\nentities:   {}\nworld hash: {}\napp hash:   {}",
        app.tick(),
        world.entity_count(),
        world.hash(),
        app.hash(),
    );
    for checkpoint in &checkpoints {
        text.push_str(&format!(
            "\ntick {}: {}",
            checkpoint["tick"],
            checkpoint["app_hash"].as_str().unwrap_or_default()
        ));
    }
    for problem in warnings.iter().chain(&failures) {
        text.push_str(&format!("\n{problem}"));
    }
    Ok(text)
}

/// Moves an asset (or folder) and rewrites the references to it.
fn move_asset(
    from: &str,
    to: &str,
    example: &str,
    root: Option<&str>,
    dry_run: bool,
    as_json: bool,
) -> Result<String> {
    let root = std::path::Path::new(root.unwrap_or("."));
    let mut app = App::new(AppConfig::default());
    wish_examples::register_types(example, app.world_mut())?;
    let plan = wish_asset::plan_move(root, from, to, app.world().registry())?;
    if !dry_run {
        wish_asset::apply_move(root, &plan)?;
    }
    let documents: Vec<Value> = plan
        .documents
        .iter()
        .map(|edit| json!({ "file": edit.file, "references": edit.changes }))
        .collect();
    if as_json {
        return Ok(json!({
            "ok": true,
            "from": plan.from,
            "to": plan.to,
            "dry_run": dry_run,
            "meta": plan.meta.as_ref().map(|(old, new)| json!({ "from": old, "to": new })),
            "documents": documents,
        })
        .to_string());
    }
    let verb = if dry_run { "Would move" } else { "Moved" };
    let mut text = format!("{verb} {} -> {}", plan.from, plan.to);
    if let Some((old, new)) = &plan.meta {
        text.push_str(&format!("\n  and {old} -> {new}"));
    }
    for edit in &plan.documents {
        text.push_str(&format!(
            "\n  updated {} reference(s) in {}",
            edit.changes, edit.file
        ));
    }
    Ok(text)
}

fn milliseconds(start: Instant) -> f64 {
    (start.elapsed().as_secs_f64() * 1_000_000.0).round() / 1000.0
}

/// Times the core on the `stress` workload: building the world, ticking it,
/// hashing it, and taking and restoring a snapshot.
fn bench(entities: u32, ticks: u64, seed: u64, as_json: bool) -> Result<String> {
    let config = AppConfig {
        seed,
        ..AppConfig::default()
    };
    let started = Instant::now();
    let mut app = wish_examples::stress::build_with(config, entities)?;
    let build_ms = milliseconds(started);

    let mut tick_ms: Vec<f64> = Vec::with_capacity(usize::try_from(ticks).unwrap_or(0));
    for _ in 0..ticks {
        let started = Instant::now();
        let report = app.step()?;
        tick_ms.push(milliseconds(started));
        if let Some(failure) = report.failures.into_iter().next() {
            return Err(failure);
        }
    }
    let mut sorted = tick_ms.clone();
    sorted.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| -> f64 {
        let index = ((sorted.len() as f64 - 1.0) * fraction).round() as usize;
        sorted.get(index).copied().unwrap_or(0.0)
    };
    let mean = if tick_ms.is_empty() {
        0.0
    } else {
        tick_ms.iter().sum::<f64>() / tick_ms.len() as f64
    };

    let started = Instant::now();
    let world_hash = app.world().hash();
    let hash_ms = milliseconds(started);
    let started = Instant::now();
    let snapshot = app.snapshot();
    let snapshot_ms = milliseconds(started);
    let started = Instant::now();
    app.restore(&snapshot)?;
    let restore_ms = milliseconds(started);

    let budget_ms = 1000.0 / f64::from(app.config().tick_rate_hz);
    let round = |value: f64| (value * 1000.0).round() / 1000.0;
    if as_json {
        return Ok(json!({
            "ok": true,
            "workload": "stress",
            "entities": entities,
            "ticks": ticks,
            "seed": seed,
            "build_ms": build_ms,
            "tick_ms": {
                "mean": round(mean),
                "median": round(percentile(0.5)),
                "p95": round(percentile(0.95)),
                "max": round(percentile(1.0)),
            },
            "tick_budget_ms": round(budget_ms),
            "hash_ms": hash_ms,
            "snapshot_ms": snapshot_ms,
            "restore_ms": restore_ms,
            "world_hash": world_hash.to_string(),
        })
        .to_string());
    }
    Ok(format!(
        "stress - {entities} entities - {ticks} ticks\n\
         build:    {build_ms:.1} ms\n\
         tick:     mean {mean:.3} ms, median {:.3} ms, p95 {:.3} ms, max {:.3} ms (budget at {} Hz: {budget_ms:.2} ms)\n\
         hash:     {hash_ms:.3} ms\n\
         snapshot: {snapshot_ms:.3} ms, restore {restore_ms:.3} ms\n\
         world hash: {world_hash}",
        percentile(0.5),
        percentile(0.95),
        percentile(1.0),
        app.config().tick_rate_hz,
    ))
}

fn schemas<'a>(types: impl Iterator<Item = &'a TypeInfo>) -> Value {
    let map: Map<String, Value> = types
        .map(|info| (info.name().to_owned(), json_schema(info.schema())))
        .collect();
    Value::Object(map)
}

fn registry_schemas(registry: &Registry) -> Value {
    json!({
        "components": schemas(registry.components().map(|(_, info)| info)),
        "resources": schemas(registry.resources().map(|(_, info)| info)),
        "events": schemas(registry.events().map(|(_, info)| info)),
    })
}

fn schema(example: Option<&str>, as_json: bool) -> Result<String> {
    let app = match example {
        Some(name) => wish_examples::build(name, AppConfig::default())?,
        None => App::new(AppConfig::default()),
    };
    let mut document = registry_schemas(app.world().registry());
    if as_json {
        if let Some(object) = document.as_object_mut() {
            object.insert("ok".to_owned(), Value::Bool(true));
        }
        return Ok(document.to_string());
    }
    serde_json::to_string_pretty(&document).map_err(|error| {
        Diagnostic::error(
            "internal_error",
            format!("Could not format the schemas: {error}"),
            "This is a bug in wen. Please report it.",
        )
    })
}
