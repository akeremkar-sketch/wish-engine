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
mod bench_render;
mod bench_script;
mod capture;
mod edit;
mod net;
mod play;
mod project_cmds;
mod scene;
mod tool_cmds;
mod types;
mod view;

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
    wen bench [--entities <n>] [--ticks <n>] [--seed <n>] [--script] [--json]
    wen bench --render [--entities <draws>] [--ticks <frames>] [--size <w>x<h>] [--software] [--json]
    wen check [files...] --example <name> [--root <folder>] [--json]
    wen fmt [files...] --example <name> [--root <folder>] [--check] [--json]
    wen migrate [files...] [--root <folder>] [--json]
    wen watch --example <name> --scene <file> [--root <folder>] [--seconds <n>] [--json]
    wen capture --example <name> --scene <file> --out <png> [--root <folder>] [--camera <id>]
                [--size <w>x<h>] [--view <view>] [--software] [--shaders <folder>] [--json]
    wen describe --example <name> --scene <file> [--root <folder>] [--camera <id>]
                 [--size <w>x<h>] [--software] [--shaders <folder>] [--json]
    wen view --example <name> --scene <file> [--root <folder>] [--size <w>x<h>]
             [--view <view>] [--software] [--shaders <folder>] [--exit-after <frames>]
    wen shaders [--dir <folder>] [--json]
    wen new <folder> [--name <name>] [--kind 2d|3d] [--template <name>] [--json]
    wen build [--root <folder>] [--dry-run] [--json]
    wen export [--root <folder>] [--name <name>] [--overwrite] [--json]
    wen test [files...] [--root <folder>] [--update-golden] [--json]
    wen replay <name> [--root <folder>] [--json]
    wen edit [--root <folder>] [--size <w>x<h>] [--software] [--exit-after <frames>]
             [--headless-frame <script.json> --out <picture.png>] [--lang <code>]
    wen play [--root <folder>] [--scene <file>] [--size <w>x<h>] [--software] [--exit-after <frames>] [--players <n>]
    wen net host [--port <port>] [--room <code>] [--players <n>] [--root <folder>] [--frames <ticks>]
    wen net join <host:port | discover> [--room <code>] [--name <name>] [--root <folder>] [--frames <ticks>]
    wen pkg [list | search [words...] | add <id> | remove <id> | update [id] | lock | check <folder>
            | install <folder>] [--index <folder>] [--version <requirement>] [--user] [--replace]
            [--dry-run] [--root <folder>] [--json]
    wen engines [list | pick [project] | add <folder> | remove <folder>] [--root <folder>]
            [--dry-run] [--json]
    wen sdk [--out <folder>] [--json]
    wen package [--out <folder>] [--json]
    wen mcp [--root <folder>] [--software] [--http <port or address>]
    wen call <tool> ['{json args}' | --<param> <value> ...] [--root <folder>] [--actor <name>]
    wen do [<file> | -] [--root <folder>] [--actor <name>]
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
               Build with --release for meaningful numbers. --script measures game code instead: the
               same system natively and as WebAssembly on 1, 1 000 and 10 000 entities (needs the SDK).
               --render measures drawing instead: a built-in scene of 2500 surfaces (--entities), a sun
               with shadows and sixteen lamps, drawn 60 times (--ticks) at 1280x720 (--size) on the
               hardware adapter if there is one (--software: the CPU rasterizer). It prints the time
               of a frame, drawn and read back, and the frame's counts (draw calls, triangles, lights).
    check      Check every prefab, scene and meta file of the project (syntax, schema,
               references, asset files) and report all problems with file, line and
               column. Exits with failure if there is any error.
    fmt        Rewrite documents in canonical form; --check only lists the ones that are
               not (for CI). Files with errors are skipped and reported.
    migrate    Upgrade scene and prefab files of an older format to the one this engine writes
               (format 2). A file keeps its meaning: every value it relied on whose default
               changed since is written into it, so it looks and plays as it did. Files of
               format 1 are read without it too (check names them); the next change made
               through the engine saves them migrated.
    watch      Load a scene and reload it when files change; an invalid change is reported
               and the last good scene stays. With --json, one JSON line per event.
    capture    Draw a scene into a PNG without a window (default 1280x720, from the first
               camera unless --camera names one). --software uses the CPU rasterizer,
               the same on every machine. Rendering never changes the simulation.
               --view picks what is drawn: lit (the picture), ids (each entity in its own
               colour), depth (white near, black far), normals (surface direction as
               colour), wireframe (triangle edges, hidden ones left out), colliders (the
               wireframe with the physics shapes), lights (the wireframe with how far lights
               reach), shadow_cascades (each surface in the colour of the shadow map it uses:
               red, green, blue, yellow from the nearest cascade; white with one map; grey
               where there is no shadow).
    describe   Say what a camera sees without looking at a picture: for each visible entity
               its pixel count, how much of it is not hidden by other things, its screen
               box and its distance; and the entities that are drawn but not visible
               (default size 640x360; each entity costs one extra render).
    view       Open a window onto a scene. Right mouse button + mouse looks around, W A S D move,
               Q / E go down / up, Shift is faster, F returns to the scene's camera, 1-8 pick
               the view (lit, ids, depth, normals, wireframe, colliders, lights,
               shadow_cascades), a left click names the entity
               under the cursor. Changed scene, prefab, model or shader files are reloaded; a
               mistake is reported and the last good picture stays. Needs a desktop session.
    shaders    Check every shader of the engine (syntax and validation, no GPU needed); --dir checks
               a folder of <name>.wgsl files in place of the built-in ones. Pass the same folder
               as --shaders to capture/describe to draw with them. Fails if there is any problem.
    new        Create a project folder: wish.json, a starting scene (camera, sun, ground), assets/,
               scripts/src/lib.rs (game code), .mcp.json (connects Claude Code and other MCP clients)
               and a project CLAUDE.md.
    build      Compile the project's game code (scripts/src/lib.rs) to WebAssembly: compiler errors
               with file, line and hint; a failed build keeps the last good code. --dry-run only
               checks that it compiles. Same as the script_build tool.
    play       Play the project's game in a window: keys and mouse buttons drive the actions of
               input.json (move, jump, ...), the game runs at 60 ticks a second. Saving
               scripts/src/lib.rs builds the new code and switches to it without stopping the game
               or losing its state; a compile error is shown and the old code keeps running. Escape
               closes the window. Needs a desktop session. Edits to scenes need a restart.
               --players N plays an online match in the same window over a simulated
               internet (50 ms each way): you play a seat as an online player, your own
               player predicted, and N-1 bots take the other seats.
    net        Online play over UDP with people on the local network (or through a relay): host
               opens a match on --port (0 picks one) and prints its room code and every change of
               the lobby; join joins one by its address and room code, or finds it with discover.
               Runs until the match closes (or --frames ticks). Same as the net tool.
    test       Run the project's scenario tests (tests/*.test.json): each plays the game from a scene and
               a seed through its steps and says which step failed, at which tick, with the values
               the condition read. Pictures are compared with golden images (software adapter);
               --update-golden saves them instead. Exits with failure if any test fails.
    replay     Play a recorded replay (replays/<name>.replay.jsonl) back in a new session and compare
               the world after every tick with the recording: passes when the game plays out exactly
               as it did, otherwise says the first tick that differs. Exits with failure if it differs.
    pkg        Packages bring themes, workspaces, languages, recipes and templates. list shows what
               the project (packages/, wish.lock) and you have installed; search finds packages in
               the package index (--index <folder>, or WISH_PACKAGE_INDEX: a folder of packages);
               add installs the newest version that fits this engine (--version narrows it);
               update brings installed packages to their newest compatible version; remove moves
               one to the trash; lock writes wish.lock again; check and install take a package
               folder. --user works on your own packages instead of the project's. Same as the
               packages tool.
    engines    Engine versions side by side. list shows the engines on this computer (this one,
               the installed ones and folders you added) and which one opens the project here;
               pick says it for another project; add remembers an engine folder where it is (a
               folder with wen and wish-engine.json); remove forgets it (an installed one goes to
               the trash). A project opens with the version it was made with, else the newest
               compatible one. Same as the engines tool.
    sdk        Engine development: build the game SDK pack from the engine's sources (needs cargo).
               Packaged engines ship it; game builds look for it next to wen or in WISH_SDK.
    package    Engine development: a folder with wen, the game SDK and the packed compiler (no cargo,
               rustup or Visual Studio needed to build game code) and the licence texts; beside
               it the release archive wish-engine-<version>-<os>-<arch> (.zip on Windows,
               .tar.gz on Linux; the same bytes every time, dated SOURCE_DATE_EPOCH if set) and
               SHA256SUMS. On Windows also clean-test.wsb, which runs a project build and its
               tests in Windows Sandbox (a machine with no developer tools).
    mcp        Serve the tools over MCP (Model Context Protocol, stdio). Claude Code and other MCP
               clients start it themselves from .mcp.json (wen new writes it); --software draws
               with the CPU adapter. Standard output carries only the protocol.
    call       Run one control-plane tool (the same tools the MCP server offers) and print its
               JSON answer, exactly as an MCP client receives it. Arguments are one JSON object
               or --name value flags. `wen call project_info` is a good start; an unknown tool
               name lists them all.
    do         Run a list of tool calls in one process, in order, one answer per line. The list
               is a JSON array or one { \"tool\": ..., \"args\": { ... } } per line, from a file or
               standard input. Simulation sessions live as long as the process, so a sequence
               such as sim_start, sim_step, capture_frame belongs in one `wen do`.
    mv         Move an asset file or folder (paths relative to --root) and update every
               asset reference to it in scenes and prefabs, all or nothing. The example
               supplies the component types that tell where references are.
    schema     Print the JSON Schema of every registered component, resource and event.
    examples   List the built-in examples.

Add --json to any command for a single machine-readable JSON document.";

fn main() -> ExitCode {
    // An unexpected error leaves a report on this computer (never sent).
    wish_control::crash::install("wen", wish_control::crash::folder().into_iter().collect());
    match wish_control::crash::guard(run_wen) {
        Ok(code) => code,
        Err(report) => {
            eprintln!("{}", wish_control::crash::told("wen", report.as_deref()));
            ExitCode::from(101)
        }
    }
}

fn run_wen() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let json = arguments.iter().any(|argument| argument == "--json");
    // The MCP server owns standard output: nothing else may be printed.
    if let Ok(Args {
        command: Command::Mcp(context),
        ..
    }) = parse(&arguments)
    {
        return tool_cmds::serve_mcp(&context);
    }
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
        } => Ok(tool_cmds::shortcut(
            "check",
            &serde_json::json!({ "paths": paths }),
            &args::ToolContext {
                root: root.clone(),
                example: example.clone(),
                actor: None,
                software: false,
                http: None,
            },
            args.json,
            &tool_cmds::check_text,
        )),
        Command::Move {
            from,
            to,
            example,
            root,
            dry_run,
        } => Ok(tool_cmds::shortcut(
            "asset_move",
            &serde_json::json!({ "from": from, "to": to, "dry_run": dry_run }),
            &args::ToolContext {
                root: root.clone(),
                example: Some(example.clone()),
                actor: None,
                software: false,
                http: None,
            },
            args.json,
            &tool_cmds::move_text,
        )),
        Command::Format {
            paths,
            example,
            root,
            check,
        } => project_cmds::format(paths, example.as_deref(), &root_of(root), *check, args.json),
        Command::Shaders { dir } => capture::shaders(dir.as_deref(), args.json),
        Command::Capture(capture) => Ok(tool_cmds::capture(capture, args.json)),
        Command::Describe(describe) => Ok(tool_cmds::describe(describe, args.json)),
        Command::New {
            dir,
            name,
            template,
            kind,
        } => tool_cmds::new_project(
            dir,
            name.as_deref(),
            template.as_deref(),
            kind.as_deref(),
            args.json,
        )
        .map(|text| Outcome {
            text,
            success: true,
        }),
        Command::Build { root, dry_run } => {
            // The answer carries no duration (ADR-027, rule 2): the command
            // line measures the call itself.
            let started = std::time::Instant::now();
            Ok(tool_cmds::shortcut(
                "script_build",
                &serde_json::json!({ "dry_run": dry_run }),
                &args::ToolContext {
                    root: root.clone(),
                    example: None,
                    actor: None,
                    software: false,
                    http: None,
                },
                args.json,
                &|answer| tool_cmds::build_text(answer, started.elapsed().as_secs_f64()),
            ))
        }
        Command::Pkg {
            action,
            rest,
            root,
            index,
            version,
            user,
            replace,
            dry_run,
        } => {
            // Folders on the command line are relative to where wen runs.
            let here = |path: &str| {
                std::env::current_dir()
                    .map(|dir| dir.join(path))
                    .unwrap_or_else(|_| std::path::PathBuf::from(path))
                    .display()
                    .to_string()
            };
            let mut call = serde_json::json!({ "action": action });
            match action.as_str() {
                "search" => call["query"] = serde_json::json!(rest.join(" ")),
                "check" | "install" => {
                    call["path"] = serde_json::json!(rest.first().map(|path| here(path)));
                }
                _ => {
                    if let Some(id) = rest.first() {
                        call["id"] = serde_json::json!(id);
                    }
                }
            }
            if let Some(index) = index {
                call["index"] = serde_json::json!(here(index));
            }
            if let Some(version) = version {
                call["version"] = serde_json::json!(version);
            }
            if *user {
                call["scope"] = serde_json::json!("user");
            }
            if *replace {
                call["replace"] = serde_json::json!(true);
            }
            if *dry_run {
                call["dry_run"] = serde_json::json!(true);
            }
            Ok(tool_cmds::shortcut(
                "packages",
                &call,
                &args::ToolContext {
                    root: root.clone(),
                    example: None,
                    actor: None,
                    software: false,
                    http: None,
                },
                args.json,
                &tool_cmds::pkg_text,
            ))
        }
        Command::Engines {
            action,
            folder,
            root,
            dry_run,
        } => {
            // Folders on the command line are relative to where wen runs.
            let here = |path: &str| {
                std::env::current_dir()
                    .map(|dir| dir.join(path))
                    .unwrap_or_else(|_| std::path::PathBuf::from(path))
            };
            let project = root
                .as_deref()
                .map(here)
                .or_else(|| std::env::current_dir().ok())
                .filter(|dir| dir.join(wish_control::PROJECT_FILE).is_file());
            let folder = folder.as_deref().map(here);
            let value = wish_control::envelope(wish_control::engines_act(
                action,
                folder.as_deref(),
                project.as_deref(),
                *dry_run,
            ));
            let success = value["ok"] == true;
            let text = if args.json {
                value.to_string()
            } else if success {
                tool_cmds::engines_text(&value)
            } else {
                match serde_json::from_value::<Diagnostic>(value["error"].clone()) {
                    Ok(problem) => problem.to_string(),
                    Err(_) => value.to_string(),
                }
            };
            Ok(project_cmds::Outcome { text, success })
        }
        Command::Export {
            root,
            name,
            overwrite,
        } => {
            let mut call = serde_json::json!({ "overwrite": overwrite });
            if let Some(name) = name {
                call["name"] = serde_json::json!(name);
            }
            Ok(tool_cmds::shortcut(
                "project_export",
                &call,
                &args::ToolContext {
                    root: root.clone(),
                    example: None,
                    actor: None,
                    software: false,
                    http: None,
                },
                args.json,
                &tool_cmds::export_text,
            ))
        }
        Command::Test {
            paths,
            root,
            update_golden,
        } => Ok(tool_cmds::shortcut(
            "test_run",
            &serde_json::json!({ "paths": paths, "update_golden": update_golden }),
            &args::ToolContext {
                root: root.clone(),
                example: None,
                actor: None,
                software: true,
                http: None,
            },
            args.json,
            &tool_cmds::test_text,
        )),
        Command::Replay { name, root } => Ok(tool_cmds::shortcut(
            "replay",
            &serde_json::json!({ "action": "run", "name": name }),
            &args::ToolContext {
                root: root.clone(),
                example: None,
                actor: None,
                software: false,
                http: None,
            },
            args.json,
            &tool_cmds::replay_text,
        )),
        Command::Package { out } => {
            tool_cmds::package(out.as_deref(), args.json).map(|text| Outcome {
                text,
                success: true,
            })
        }
        Command::Sdk { out } => {
            tool_cmds::pack_sdk(out.as_deref(), args.json).map(|text| Outcome {
                text,
                success: true,
            })
        }
        Command::Call(call) => Ok(tool_cmds::call(call)),
        Command::Do(calls) => Ok(tool_cmds::run_calls(calls)),
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
            ticks,
            script: true,
            ..
        } => bench_script::bench_script(ticks, args.json),
        Command::Bench {
            entities,
            ticks,
            render: true,
            size,
            software,
            ..
        } => bench_render::bench_render(
            &bench_render::RenderBench {
                draws: entities,
                frames: ticks,
                size,
                software,
            },
            args.json,
        ),
        Command::Bench {
            entities,
            ticks,
            seed,
            ..
        } => bench(entities, ticks, seed, args.json),
        Command::Check { .. }
        | Command::Format { .. }
        | Command::Move { .. }
        | Command::Migrate { .. }
        | Command::Shaders { .. }
        | Command::New { .. }
        | Command::Build { .. }
        | Command::Export { .. }
        | Command::Pkg { .. }
        | Command::Engines { .. }
        | Command::Replay { .. }
        | Command::Test { .. }
        | Command::Sdk { .. }
        | Command::Package { .. }
        | Command::Capture(_)
        | Command::Describe(_)
        | Command::Mcp(_)
        | Command::Call(_)
        | Command::Do(_)
        | Command::Watch { .. } => Err(Diagnostic::error(
            "internal_error",
            "A project command was run through the simple path.",
            "This is a bug in wen. Please report it.",
        )),
        Command::View(view) => view::view(&view, args.json),
        Command::Play(play) => play::play(&play, args.json),
        Command::Net(net) => net::run(&net, args.json),
        Command::Edit(edit) => edit::edit(&edit, args.json),
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
    let mut app = match example {
        Some(name) => wish_examples::build(name, AppConfig::default())?,
        None => App::new(AppConfig::default()),
    };
    types::register_engine_types(app.world_mut())?;
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
