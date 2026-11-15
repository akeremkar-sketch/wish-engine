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
mod scene;

use std::process::ExitCode;
use std::time::Instant;

use serde_json::{Map, Value, json};
use wish_app::{App, AppConfig};
use wish_core::{Diagnostic, Result};
use wish_ecs::{Registry, TypeInfo};
use wish_schema::json_schema;

use crate::args::{Args, Command, parse};
use crate::scene::load_scene_app;

const USAGE: &str = "\
wen - the Wish Engine command-line tool

USAGE:
    wen run --example <name> [--scene <file> [--root <folder>]] [--frames <n>] [--seed <n>]
            [--hash-every <n>] [--headless] [--json]
    wen bench [--entities <n>] [--ticks <n>] [--seed <n>] [--json]
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
    schema     Print the JSON Schema of every registered component, resource and event.
    examples   List the built-in examples.

Add --json to any command for a single machine-readable JSON document.";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let json = arguments.iter().any(|argument| argument == "--json");
    match parse(&arguments).and_then(execute) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
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
