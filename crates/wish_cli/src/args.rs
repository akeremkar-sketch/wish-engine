//! Command-line parsing.
//!
//! Hand-written rather than generated, so that every mistake is reported in
//! the engine's diagnostic format with a "did you mean" hint, the same way
//! the MCP tools will report it.

use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};

/// The commands `wen` understands.
pub const COMMANDS: &[&str] = &[
    "run", "bench", "schema", "mv", "check", "fmt", "migrate", "watch", "capture", "describe",
    "shaders", "view", "edit", "call", "do", "new", "build", "export", "play", "replay", "test",
    "pkg", "engines", "sdk", "package", "mcp", "examples", "help", "version",
];

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// What to do.
    pub command: Command,
    /// Print one machine-readable JSON document instead of text.
    pub json: bool,
}

/// What to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Run a built-in example headless for a number of ticks.
    Run {
        /// The example's name.
        example: String,
        /// How many ticks to simulate.
        frames: u64,
        /// The random seed.
        seed: u64,
        /// Record the app hash every this many ticks (0: never).
        hash_every: u64,
        /// A scene file to load instead of the example's own entities; the
        /// example then only supplies the component types.
        scene: Option<String>,
        /// The project folder that scene and prefab paths are relative to.
        root: Option<String>,
    },
    /// Measure the core on the `stress` workload.
    Bench {
        /// How many entities to create.
        entities: u32,
        /// How many ticks to time.
        ticks: u64,
        /// The random seed.
        seed: u64,
        /// Measure game code (WebAssembly) against native systems instead.
        script: bool,
        /// Measure drawing instead: `entities` surfaces, `ticks` frames.
        render: bool,
        /// With `render`: the picture's width and height.
        size: (u32, u32),
        /// With `render`: use the software adapter even if there is hardware.
        software: bool,
    },
    /// Move an asset file or folder and update the scenes that refer to it.
    Move {
        /// The current project-relative path.
        from: String,
        /// The new project-relative path.
        to: String,
        /// The example whose component types find the references.
        example: String,
        /// The project folder (default: the current folder).
        root: Option<String>,
        /// Report what would change without changing anything.
        dry_run: bool,
    },
    /// Check every document of a project.
    Check {
        /// Report only these project-relative files (all when empty).
        paths: Vec<String>,
        /// The example whose component types the documents use (default:
        /// the project's own types, game code included).
        example: Option<String>,
        /// The project folder (default: the current folder).
        root: Option<String>,
    },
    /// Rewrite documents in canonical form.
    Format {
        /// Format only these project-relative files (all when empty).
        paths: Vec<String>,
        /// The example whose component types the documents use (default:
        /// the project's own types, game code included).
        example: Option<String>,
        /// The project folder (default: the current folder).
        root: Option<String>,
        /// Only report files that are not canonical; change nothing.
        check: bool,
    },
    /// Upgrade documents of older format versions.
    Migrate {
        /// Migrate only these project-relative files (all when empty).
        paths: Vec<String>,
        /// The project folder (default: the current folder).
        root: Option<String>,
    },
    /// Load a scene and keep it in step with the files on disk.
    Watch {
        /// The example whose component types the scene uses.
        example: String,
        /// The scene file, relative to the root.
        scene: String,
        /// The project folder (default: the current folder).
        root: Option<String>,
        /// Stop after this many seconds (0: run until interrupted).
        seconds: u64,
    },
    /// Play the project's game in a window.
    Play(PlayArgs),
    Edit(EditArgs),
    /// Run the project's scenario tests (the `test_run` tool).
    Test {
        /// Only these test files (all when empty).
        paths: Vec<String>,
        /// The project folder.
        root: Option<String>,
        /// Save the pictures as the golden images.
        update_golden: bool,
    },
    /// Play a recorded replay back and compare every tick (the `replay` tool's run).
    Replay {
        /// The replay's name.
        name: String,
        /// The project folder.
        root: Option<String>,
    },
    /// Build the project's game code (the `script_build` tool).
    Build {
        /// The project folder.
        root: Option<String>,
        /// Only check that it compiles.
        dry_run: bool,
    },
    /// Export the game as a folder that runs without the engine (the
    /// `project_export` tool).
    Export {
        /// The project folder.
        root: Option<String>,
        /// The folder's and program's name.
        name: Option<String>,
        /// Replace an earlier export of that name.
        overwrite: bool,
    },
    /// Find, add, update and remove packages (the `packages` tool).
    Pkg {
        /// list, search, add, remove, update, lock, check or install.
        action: String,
        /// The words, id or folder after the action.
        rest: Vec<String>,
        /// The project folder.
        root: Option<String>,
        /// The package index's folder.
        index: Option<String>,
        /// A version requirement.
        version: Option<String>,
        /// The person's own packages instead of the project's.
        user: bool,
        /// Replace the installed version.
        replace: bool,
        /// Say what would happen without doing it.
        dry_run: bool,
    },
    /// The engine versions on this computer (the `engines` tool).
    Engines {
        /// list, pick, add or remove.
        action: String,
        /// The engine's folder (add, remove) or a project's (pick).
        folder: Option<String>,
        /// The project folder (list: which engine opens it).
        root: Option<String>,
        /// Say what would happen without doing it.
        dry_run: bool,
    },
    /// Build a package for machines without developer tools (engine development).
    Package {
        /// Where to put it (default: `package` next to wen).
        out: Option<String>,
    },
    /// Build the game SDK pack from the engine's sources (engine development).
    Sdk {
        /// Where to put it (default: `sdk` next to wen).
        out: Option<String>,
    },
    /// Create a new project folder.
    New {
        /// The folder to create the project in.
        dir: String,
        /// The project's name (default: the folder's name).
        name: Option<String>,
        /// The template to start from (`islands`), else an empty project.
        template: Option<String>,
        /// The project's kind, `2d` or `3d` (default: the template's, else 3d).
        kind: Option<String>,
    },
    /// Serve the tools over MCP on standard input and output.
    Mcp(ToolContext),
    /// Run one control-plane tool.
    Call(CallArgs),
    /// Run a list of control-plane tool calls in one process.
    Do(DoArgs),
    /// Draw a scene into a PNG without a window.
    Capture(CaptureArgs),
    /// List what a camera sees: which entities are on screen and how much.
    Describe(DescribeArgs),
    /// Open a window onto a scene.
    View(ViewArgs),
    /// Check the engine's shaders (or a developer's copies of them).
    Shaders {
        /// A folder of `<name>.wgsl` files that replace the built-in shaders.
        dir: Option<String>,
    },
    /// Print the JSON Schemas of the registered types.
    Schema {
        /// Load the types of this example; only built-in types without it.
        example: Option<String>,
    },
    /// List the built-in examples.
    Examples,
    /// Print usage.
    Help,
    /// Print the version.
    Version,
}

/// The options of `wen capture`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureArgs {
    /// The example whose component types the scene uses.
    pub example: String,
    /// The scene file, relative to the root.
    pub scene: String,
    /// The project folder (default: the current folder).
    pub root: Option<String>,
    /// The scene id of the camera to draw from (default: the first camera).
    pub camera: Option<String>,
    /// Image width and height in pixels.
    pub size: (u32, u32),
    /// What to draw: `lit`, `ids`, `depth`, `normals`, `wireframe`,
    /// `colliders`, `lights` or `shadow_cascades`.
    pub view: String,
    /// Use the software adapter.
    pub software: bool,
    /// A folder of `<name>.wgsl` files that replace the built-in shaders.
    pub shaders: Option<String>,
    /// Where to write the PNG.
    pub out: String,
}

/// The options of `wen edit`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditArgs {
    /// The project folder (default: the current folder).
    pub root: Option<String>,
    /// The window's size in pixels.
    pub size: (u32, u32),
    /// Use the software adapter.
    pub software: bool,
    /// Close after this many frames (for scripts).
    pub exit_after: Option<u64>,
    /// Instead of a window: run this input script and write the last
    /// picture to `out`.
    pub headless_frame: Option<String>,
    /// Where `--headless-frame` writes its PNG.
    pub out: Option<String>,
    /// The interface's language (`tr`, `de`...); else the saved choice,
    /// `WISH_LANG` or the system's.
    pub lang: Option<String>,
    /// Open the project with the engine version it was made with, if
    /// another engine of this computer is that one (the launcher asks).
    pub pick_engine: bool,
}

/// The options of `wen play`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayArgs {
    /// The project folder (default: the current folder).
    pub root: Option<String>,
    /// The scene to play (default: the project's own).
    pub scene: Option<String>,
    /// The window's size in pixels.
    pub size: (u32, u32),
    /// Use the software adapter.
    pub software: bool,
    /// Close after this many frames (for scripts).
    pub exit_after: Option<u64>,
}

/// The options of `wen view`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewArgs {
    /// The example whose component types the scene uses.
    pub example: String,
    /// The scene file, relative to the root.
    pub scene: String,
    /// The project folder (default: the current folder).
    pub root: Option<String>,
    /// The window's size in pixels.
    pub size: (u32, u32),
    /// The view to start in.
    pub view: String,
    /// Use the software adapter.
    pub software: bool,
    /// A folder of `<name>.wgsl` files that replace the built-in shaders.
    pub shaders: Option<String>,
    /// Close after this many frames (for scripts).
    pub exit_after: Option<u64>,
}

/// The options of `wen describe`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescribeArgs {
    /// The example whose component types the scene uses.
    pub example: String,
    /// The scene file, relative to the root.
    pub scene: String,
    /// The project folder (default: the current folder).
    pub root: Option<String>,
    /// The scene id of the camera to look from (default: the first camera).
    pub camera: Option<String>,
    /// The size of the picture it measures in, in pixels.
    pub size: (u32, u32),
    /// Use the software adapter.
    pub software: bool,
    /// A folder of `<name>.wgsl` files that replace the built-in shaders.
    pub shaders: Option<String>,
}

/// Where and as whom a tool command runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolContext {
    /// The project folder (default: the current folder).
    pub root: Option<String>,
    /// A built-in example whose types to use (overrides `wish.json`).
    pub example: Option<String>,
    /// Who makes the changes (default: `human`).
    pub actor: Option<String>,
    /// Draw with the software (CPU) adapter.
    pub software: bool,
    /// `wen mcp` only: serve Streamable HTTP on this address instead of
    /// standard input and output.
    pub http: Option<String>,
}

/// The options of `wen call`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallArgs {
    /// The tool's name.
    pub tool: String,
    /// The arguments as JSON text, if given that way.
    pub json: Option<String>,
    /// The arguments as `--name value` pairs (any order).
    pub flags: Vec<(String, String)>,
    /// Where and as whom.
    pub context: ToolContext,
}

/// The options of `wen do`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DoArgs {
    /// The file of calls, or `None` / `-` for standard input.
    pub source: Option<String>,
    /// Where and as whom.
    pub context: ToolContext,
}

/// `wen call <tool> [json] [--param value ...]` and `wen do [file]`. A tool's
/// own parameters are not known here (they are checked by the control plane,
/// which gives the same errors as over MCP), so only the shared flags are.
fn parse_tool_command(command: &str, rest: &[String]) -> Result<Args> {
    let mut context = ToolContext::default();
    let mut json = false;
    let mut positional: Vec<String> = Vec::new();
    let mut flags: Vec<(String, String)> = Vec::new();
    let mut index = 0;
    while let Some(argument) = rest.get(index) {
        index += 1;
        if !argument.starts_with("--") || argument == "-" {
            positional.push(argument.clone());
            continue;
        }
        let (name, inline) = match argument.split_once('=') {
            Some((name, value)) => (name.to_owned(), Some(value.to_owned())),
            None => (argument.clone(), None),
        };
        if name == "--software" {
            context.software = true;
            continue;
        }
        if name == "--json" {
            json = true;
            continue;
        }
        // A flag's value is the next argument unless that is another flag
        // (so a boolean parameter can be written alone: `--dry-run`).
        let value = match inline {
            Some(value) => value,
            None => match rest.get(index) {
                Some(next) if !next.starts_with("--") => {
                    index += 1;
                    next.clone()
                }
                _ => String::new(),
            },
        };
        let missing = |flag: &str| {
            usage_error(
                format!("{flag} needs a value."),
                format!("Write it like {flag} <value>."),
            )
        };
        match name.as_str() {
            "--root" if !value.is_empty() => context.root = Some(value),
            "--example" if !value.is_empty() => context.example = Some(value),
            "--actor" if !value.is_empty() => context.actor = Some(value),
            "--http" if command != "mcp" => {
                return Err(usage_error(
                    "--http is only for 'wen mcp'.".to_owned(),
                    "Write it like: wen mcp --http 7878.".to_owned(),
                ));
            }
            "--http" if !value.is_empty() => context.http = Some(value),
            "--root" | "--example" | "--actor" | "--http" => return Err(missing(&name)),
            _ if command == "do" || command == "mcp" => {
                let names = ["--root", "--example", "--actor", "--json"];
                return Err(usage_error(
                    format!("'wen do' does not accept '{name}'."),
                    unknown_name_hint(&name, &names),
                )
                .with_candidates(names));
            }
            _ => flags.push((name, value)),
        }
    }
    if command == "mcp" {
        if let Some(extra) = positional.first() {
            return Err(usage_error(
                format!("'wen mcp' takes no argument '{extra}'."),
                "Write it like: wen mcp [--root <folder>] [--software] [--http <port or address>]. MCP clients start it themselves (see .mcp.json).".to_owned(),
            ));
        }
        return Ok(Args {
            command: Command::Mcp(context),
            json,
        });
    }
    let command = if command == "do" {
        if positional.len() > 1 {
            return Err(usage_error(
                "'wen do' takes one file of calls.".to_owned(),
                "Write it like: wen do calls.json (or pipe the calls in: ... | wen do -)."
                    .to_owned(),
            ));
        }
        Command::Do(DoArgs {
            source: positional.pop().filter(|source| source != "-"),
            context,
        })
    } else {
        let mut positional = positional.into_iter();
        let Some(tool) = positional.next() else {
            let names: Vec<&str> = wish_control::catalog()
                .iter()
                .map(|tool| tool.name)
                .collect();
            return Err(usage_error(
                "'wen call' needs the name of a tool.".to_owned(),
                format!(
                    "Write it like: wen call project_info. Tools: {}.",
                    names.join(", ")
                ),
            )
            .with_candidates(names));
        };
        let json_text = positional.next();
        if let Some(extra) = positional.next() {
            return Err(usage_error(
                format!("'wen call' got an extra argument '{extra}'."),
                "Give the arguments either as one JSON object or as --name value flags.".to_owned(),
            ));
        }
        Command::Call(CallArgs {
            tool,
            json: json_text,
            flags,
            context,
        })
    };
    Ok(Args { command, json })
}

fn usage_error(message: String, hint: String) -> Diagnostic {
    Diagnostic::error("invalid_arguments", message, hint)
}

/// The flags a command accepts; `true` when the flag takes a value.
fn flags_of(command: &str) -> &'static [(&'static str, bool)] {
    match command {
        "run" => &[
            ("--example", true),
            ("--frames", true),
            ("--seed", true),
            ("--hash-every", true),
            ("--scene", true),
            ("--root", true),
            ("--headless", false),
            ("--json", false),
        ],
        "bench" => &[
            ("--entities", true),
            ("--ticks", true),
            ("--seed", true),
            ("--script", false),
            ("--render", false),
            ("--size", true),
            ("--software", false),
            ("--json", false),
        ],
        "schema" => &[("--example", true), ("--json", false)],
        "mv" => &[
            ("--example", true),
            ("--root", true),
            ("--dry-run", false),
            ("--json", false),
        ],
        "check" => &[("--example", true), ("--root", true), ("--json", false)],
        "fmt" => &[
            ("--example", true),
            ("--root", true),
            ("--check", false),
            ("--json", false),
        ],
        "migrate" => &[("--root", true), ("--json", false)],
        "watch" => &[
            ("--example", true),
            ("--scene", true),
            ("--root", true),
            ("--seconds", true),
            ("--json", false),
        ],
        "capture" => &[
            ("--example", true),
            ("--scene", true),
            ("--root", true),
            ("--camera", true),
            ("--size", true),
            ("--view", true),
            ("--software", false),
            ("--shaders", true),
            ("--out", true),
            ("--json", false),
        ],
        "shaders" => &[("--dir", true), ("--json", false)],
        "new" => &[
            ("--name", true),
            ("--template", true),
            ("--kind", true),
            ("--json", false),
        ],
        "build" => &[("--root", true), ("--dry-run", false), ("--json", false)],
        "export" => &[
            ("--root", true),
            ("--name", true),
            ("--overwrite", false),
            ("--json", false),
        ],
        "replay" => &[("--root", true), ("--json", false)],
        "test" => &[
            ("--root", true),
            ("--update-golden", false),
            ("--json", false),
        ],
        "play" => &[
            ("--root", true),
            ("--scene", true),
            ("--size", true),
            ("--software", false),
            ("--exit-after", true),
            ("--json", false),
        ],
        "edit" => &[
            ("--root", true),
            ("--size", true),
            ("--software", false),
            ("--exit-after", true),
            ("--headless-frame", true),
            ("--out", true),
            ("--lang", true),
            ("--pick-engine", false),
            ("--json", false),
        ],
        "sdk" => &[("--out", true), ("--json", false)],
        "engines" => &[("--root", true), ("--dry-run", false), ("--json", false)],
        "pkg" => &[
            ("--root", true),
            ("--index", true),
            ("--version", true),
            ("--user", false),
            ("--replace", false),
            ("--dry-run", false),
            ("--json", false),
        ],
        "package" => &[("--out", true), ("--json", false)],
        "view" => &[
            ("--example", true),
            ("--scene", true),
            ("--root", true),
            ("--size", true),
            ("--view", true),
            ("--software", false),
            ("--shaders", true),
            ("--exit-after", true),
            ("--json", false),
        ],
        "describe" => &[
            ("--example", true),
            ("--scene", true),
            ("--root", true),
            ("--camera", true),
            ("--size", true),
            ("--software", false),
            ("--shaders", true),
            ("--json", false),
        ],
        _ => &[("--json", false)],
    }
}

/// Parses `WIDTHxHEIGHT`, such as `1280x720`.
fn parse_size(value: &str) -> Result<(u32, u32)> {
    let parsed = value
        .split_once(['x', 'X'])
        .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)));
    parsed.ok_or_else(|| {
        usage_error(
            format!("--size expects WIDTHxHEIGHT, but got '{value}'."),
            "Write it like --size 1280x720.".to_owned(),
        )
    })
}

fn parse_number(flag: &str, value: &str) -> Result<u64> {
    value.parse().map_err(|_| {
        usage_error(
            format!("{flag} expects a whole number, but got '{value}'."),
            format!("Write it like {flag} 600."),
        )
    })
}

/// Parses the arguments after the program name.
pub fn parse(arguments: &[String]) -> Result<Args> {
    let Some((first, rest)) = arguments.split_first() else {
        return Ok(Args {
            command: Command::Help,
            json: false,
        });
    };
    let command = match first.as_str() {
        "-h" | "--help" => "help",
        "-V" | "--version" => "version",
        other => other,
    };
    if !COMMANDS.contains(&command) {
        return Err(usage_error(
            format!("'{first}' is not a wen command."),
            unknown_name_hint(first, COMMANDS),
        )
        .with_candidates(COMMANDS.iter().copied()));
    }

    if command == "call" || command == "do" || command == "mcp" {
        return parse_tool_command(command, rest);
    }
    let accepted = flags_of(command);
    let mut values: Vec<(&str, Option<String>)> = Vec::new();
    let mut positional: Vec<&str> = Vec::new();
    let mut index = 0;
    while let Some(argument) = rest.get(index) {
        index += 1;
        if matches!(
            command,
            "mv" | "check" | "fmt" | "migrate" | "new" | "test" | "replay" | "pkg" | "engines"
        ) && !argument.starts_with('-')
        {
            positional.push(argument);
            continue;
        }
        let (name, inline_value) = match argument.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => (argument.as_str(), None),
        };
        let Some(&(flag, takes_value)) = accepted.iter().find(|(flag, _)| *flag == name) else {
            let names: Vec<&str> = accepted.iter().map(|(flag, _)| *flag).collect();
            return Err(usage_error(
                format!("'wen {command}' does not accept '{name}'."),
                unknown_name_hint(name, &names),
            )
            .with_candidates(names));
        };
        let value = if takes_value {
            match inline_value {
                Some(value) => Some(value),
                None => {
                    let value = rest.get(index).cloned().ok_or_else(|| {
                        usage_error(
                            format!("{flag} needs a value."),
                            format!("Write it like {flag} <value>, or {flag}=<value>."),
                        )
                    })?;
                    index += 1;
                    Some(value)
                }
            }
        } else if inline_value.is_some() {
            return Err(usage_error(
                format!("{flag} does not take a value."),
                format!("Write just {flag}."),
            ));
        } else {
            None
        };
        values.push((flag, value));
    }

    let value_of = |flag: &str| -> Option<&str> {
        values
            .iter()
            .rev()
            .find(|(name, _)| *name == flag)
            .and_then(|(_, value)| value.as_deref())
    };
    let json = values.iter().any(|(flag, _)| *flag == "--json");

    let command = match command {
        "run" => Command::Run {
            example: value_of("--example")
                .ok_or_else(|| {
                    usage_error(
                        "'wen run' needs an example to run, or an example whose types a --scene uses."
                            .to_owned(),
                        format!(
                            "Add --example <name>. Built-in examples: {}.",
                            wish_examples::NAMES.join(", ")
                        ),
                    )
                })?
                .to_owned(),
            frames: value_of("--frames")
                .map_or(Ok(600), |value| parse_number("--frames", value))?,
            seed: value_of("--seed").map_or(Ok(0), |value| parse_number("--seed", value))?,
            hash_every: value_of("--hash-every")
                .map_or(Ok(0), |value| parse_number("--hash-every", value))?,
            scene: value_of("--scene").map(str::to_owned),
            root: value_of("--root").map(str::to_owned),
        },
        "bench" => {
            let script = values.iter().any(|(flag, _)| *flag == "--script");
            let render = values.iter().any(|(flag, _)| *flag == "--render");
            if script && render {
                return Err(usage_error(
                    "'wen bench' measures one thing at a time: --script or --render.".to_owned(),
                    "Run it once with --script and once with --render.".to_owned(),
                ));
            }
            // Drawing is measured on a few thousand surfaces and a second of
            // frames; the core on many more entities and ticks.
            let (entities, ticks) = if render { (2500, 60) } else { (100_000, 600) };
            Command::Bench {
                entities: value_of("--entities")
                    .map_or(Ok(entities), |value| parse_number("--entities", value))?
                    .try_into()
                    .map_err(|_| {
                        usage_error(
                            "--entities is too large.".to_owned(),
                            "Use at most 4294967294 entities.".to_owned(),
                        )
                    })?,
                ticks: value_of("--ticks")
                    .map_or(Ok(ticks), |value| parse_number("--ticks", value))?,
                seed: value_of("--seed").map_or(Ok(0), |value| parse_number("--seed", value))?,
                script,
                render,
                size: value_of("--size").map_or(Ok((1280, 720)), parse_size)?,
                software: values.iter().any(|(flag, _)| *flag == "--software"),
            }
        }
        "schema" => Command::Schema {
            example: value_of("--example").map(str::to_owned),
        },
        "mv" => {
            let [from, to] = positional.as_slice() else {
                return Err(usage_error(
                    format!(
                        "'wen mv' needs exactly two paths (the current and the new one), but got {}.",
                        positional.len()
                    ),
                    "Write it like: wen mv assets/old.png assets/textures/new.png --example <name>"
                        .to_owned(),
                ));
            };
            Command::Move {
                from: (*from).to_owned(),
                to: (*to).to_owned(),
                example: value_of("--example")
                    .ok_or_else(|| {
                        usage_error(
                            "'wen mv' needs --example to know the component types that hold asset references."
                                .to_owned(),
                            format!(
                                "Add --example <name>. Built-in examples: {}.",
                                wish_examples::NAMES.join(", ")
                            ),
                        )
                    })?
                    .to_owned(),
                root: value_of("--root").map(str::to_owned),
                dry_run: values.iter().any(|(flag, _)| *flag == "--dry-run"),
            }
        }
        "check" | "fmt" => {
            let example = value_of("--example").map(str::to_owned);
            let root = value_of("--root").map(str::to_owned);
            let paths: Vec<String> = positional.iter().map(|path| (*path).to_owned()).collect();
            match command {
                "check" => Command::Check {
                    paths,
                    example,
                    root,
                },
                _ => Command::Format {
                    paths,
                    example,
                    root,
                    check: values.iter().any(|(flag, _)| *flag == "--check"),
                },
            }
        }
        "watch" => {
            let example = value_of("--example")
                .ok_or_else(|| {
                    usage_error(
                        format!("'wen {command}' needs --example to know the component types the documents use."),
                        format!(
                            "Add --example <name>. Built-in examples: {}.",
                            wish_examples::NAMES.join(", ")
                        ),
                    )
                })?
                .to_owned();
            Command::Watch {
                example,
                scene: value_of("--scene")
                    .ok_or_else(|| {
                        usage_error(
                            "'wen watch' needs a --scene to load.".to_owned(),
                            "Add --scene <file>, a path relative to the project folder.".to_owned(),
                        )
                    })?
                    .to_owned(),
                root: value_of("--root").map(str::to_owned),
                seconds: value_of("--seconds")
                    .map_or(Ok(0), |value| parse_number("--seconds", value))?,
            }
        }
        "test" => Command::Test {
            paths: positional.iter().map(|path| (*path).to_owned()).collect(),
            root: value_of("--root").map(str::to_owned),
            update_golden: values.iter().any(|(flag, _)| *flag == "--update-golden"),
        },
        "replay" => {
            if positional.len() != 1 {
                return Err(usage_error(
                    "'wen replay' needs the name of the replay to play back.".to_owned(),
                    "Write it like: wen replay jump_test [--root <folder>]. Replays are recorded with the replay tool (record, sim_step, save).".to_owned(),
                ));
            }
            Command::Replay {
                name: positional[0].to_owned(),
                root: value_of("--root").map(str::to_owned),
            }
        }
        "edit" => Command::Edit(EditArgs {
            root: value_of("--root").map(str::to_owned),
            size: value_of("--size").map_or(Ok((1280, 720)), parse_size)?,
            software: values.iter().any(|(flag, _)| *flag == "--software"),
            exit_after: value_of("--exit-after")
                .map(|value| parse_number("--exit-after", value))
                .transpose()?,
            headless_frame: value_of("--headless-frame").map(str::to_owned),
            out: value_of("--out").map(str::to_owned),
            lang: value_of("--lang").map(str::to_owned),
            pick_engine: values.iter().any(|(flag, _)| *flag == "--pick-engine"),
        }),
        "play" => Command::Play(PlayArgs {
            root: value_of("--root").map(str::to_owned),
            scene: value_of("--scene").map(str::to_owned),
            size: value_of("--size").map_or(Ok((1280, 720)), parse_size)?,
            software: values.iter().any(|(flag, _)| *flag == "--software"),
            exit_after: value_of("--exit-after")
                .map(|value| parse_number("--exit-after", value))
                .transpose()?,
        }),
        "build" => Command::Build {
            root: value_of("--root").map(str::to_owned),
            dry_run: values.iter().any(|(flag, _)| *flag == "--dry-run"),
        },
        "export" => Command::Export {
            root: value_of("--root").map(str::to_owned),
            name: value_of("--name").map(str::to_owned),
            overwrite: values.iter().any(|(flag, _)| *flag == "--overwrite"),
        },
        "package" => Command::Package {
            out: value_of("--out").map(str::to_owned),
        },
        "sdk" => Command::Sdk {
            out: value_of("--out").map(str::to_owned),
        },
        "pkg" => {
            const ACTIONS: &[&str] = &[
                "list", "search", "add", "remove", "update", "lock", "check", "install",
            ];
            let action = positional.first().copied().unwrap_or("list");
            if !ACTIONS.contains(&action) {
                return Err(usage_error(
                    format!("'{action}' is not a wen pkg action."),
                    unknown_name_hint(action, ACTIONS),
                )
                .with_candidates(ACTIONS.iter().copied()));
            }
            let rest: Vec<String> = positional.iter().skip(1).map(|w| (*w).to_owned()).collect();
            let needs = match action {
                "add" | "remove" => Some("the package's id"),
                "check" | "install" => Some("the package's folder"),
                _ => None,
            };
            if let Some(what) = needs
                && rest.len() != 1
            {
                return Err(usage_error(
                    format!("'wen pkg {action}' needs {what}."),
                    format!("Write it like: wen pkg {action} <{}>.", if action == "add" || action == "remove" { "id" } else { "folder" }),
                ));
            }
            if matches!(action, "list" | "lock") && !rest.is_empty() {
                return Err(usage_error(
                    format!("'wen pkg {action}' takes no more words."),
                    format!("Write just: wen pkg {action}."),
                ));
            }
            if action == "update" && rest.len() > 1 {
                return Err(usage_error(
                    "'wen pkg update' takes at most one package id.".to_owned(),
                    "Write: wen pkg update (every package) or wen pkg update <id>.".to_owned(),
                ));
            }
            Command::Pkg {
                action: action.to_owned(),
                rest,
                root: value_of("--root").map(str::to_owned),
                index: value_of("--index").map(str::to_owned),
                version: value_of("--version").map(str::to_owned),
                user: values.iter().any(|(flag, _)| *flag == "--user"),
                replace: values.iter().any(|(flag, _)| *flag == "--replace"),
                dry_run: values.iter().any(|(flag, _)| *flag == "--dry-run"),
            }
        }
        "engines" => {
            const ACTIONS: &[&str] = &["list", "pick", "add", "remove"];
            let action = positional.first().copied().unwrap_or("list");
            if !ACTIONS.contains(&action) {
                return Err(usage_error(
                    format!("'{action}' is not a wen engines action."),
                    unknown_name_hint(action, ACTIONS),
                )
                .with_candidates(ACTIONS.iter().copied()));
            }
            let rest: Vec<&str> = positional.iter().skip(1).copied().collect();
            let most = if action == "list" { 0 } else { 1 };
            if rest.len() > most || (matches!(action, "add" | "remove") && rest.is_empty()) {
                return Err(usage_error(
                    format!("'wen engines {action}' takes {}.", match action {
                        "list" => "no more words",
                        "pick" => "at most a project's folder",
                        _ => "one engine folder",
                    }),
                    "Write it like: wen engines add C:/Wish/0.2.0, wen engines remove <folder>, wen engines pick <project>.".to_owned(),
                ));
            }
            Command::Engines {
                action: action.to_owned(),
                folder: rest.first().map(|w| (*w).to_owned()),
                root: value_of("--root").map(str::to_owned),
                dry_run: values.iter().any(|(flag, _)| *flag == "--dry-run"),
            }
        }
        "new" => {
            if positional.len() != 1 {
                return Err(usage_error(
                    "'wen new' needs the folder to create the project in.".to_owned(),
                    "Write it like: wen new my_game (optionally --name \"My game\").".to_owned(),
                ));
            }
            Command::New {
                dir: positional[0].to_owned(),
                name: value_of("--name").map(str::to_owned),
                template: value_of("--template").map(str::to_owned),
                kind: value_of("--kind").map(str::to_owned),
            }
        }
        "migrate" => Command::Migrate {
            paths: positional.iter().map(|path| (*path).to_owned()).collect(),
            root: value_of("--root").map(str::to_owned),
        },
        "capture" => {
            let required = |flag: &str, what: &str| {
                value_of(flag).map(str::to_owned).ok_or_else(|| {
                    usage_error(
                        format!("'wen capture' needs {flag}: {what}."),
                        format!(
                            "Write it like: wen capture --example stress --scene scenes/main.scene.json --out shot.png (built-in examples: {})",
                            wish_examples::NAMES.join(", ")
                        ),
                    )
                })
            };
            Command::Capture(CaptureArgs {
                example: required("--example", "the example whose component types the scene uses")?,
                scene: required("--scene", "the scene file to draw")?,
                root: value_of("--root").map(str::to_owned),
                camera: value_of("--camera").map(str::to_owned),
                size: value_of("--size").map_or(Ok((1280, 720)), parse_size)?,
                view: value_of("--view").unwrap_or("lit").to_owned(),
                software: values.iter().any(|(flag, _)| *flag == "--software"),
                shaders: value_of("--shaders").map(str::to_owned),
                out: required("--out", "the PNG file to write")?,
            })
        }
        "describe" => {
            let required = |flag: &str, what: &str| {
                value_of(flag).map(str::to_owned).ok_or_else(|| {
                    usage_error(
                        format!("'wen describe' needs {flag}: {what}."),
                        format!(
                            "Write it like: wen describe --example stress --scene scenes/main.scene.json (built-in examples: {})",
                            wish_examples::NAMES.join(", ")
                        ),
                    )
                })
            };
            Command::Describe(DescribeArgs {
                example: required("--example", "the example whose component types the scene uses")?,
                scene: required("--scene", "the scene file to look at")?,
                root: value_of("--root").map(str::to_owned),
                camera: value_of("--camera").map(str::to_owned),
                size: value_of("--size").map_or(Ok((640, 360)), parse_size)?,
                software: values.iter().any(|(flag, _)| *flag == "--software"),
                shaders: value_of("--shaders").map(str::to_owned),
            })
        }
        "view" => {
            let required = |flag: &str, what: &str| {
                value_of(flag).map(str::to_owned).ok_or_else(|| {
                    usage_error(
                        format!("'wen view' needs {flag}: {what}."),
                        format!(
                            "Write it like: wen view --example stress --scene scenes/main.scene.json (built-in examples: {})",
                            wish_examples::NAMES.join(", ")
                        ),
                    )
                })
            };
            Command::View(ViewArgs {
                example: required("--example", "the example whose component types the scene uses")?,
                scene: required("--scene", "the scene file to look at")?,
                root: value_of("--root").map(str::to_owned),
                size: value_of("--size").map_or(Ok((1280, 720)), parse_size)?,
                view: value_of("--view").unwrap_or("lit").to_owned(),
                software: values.iter().any(|(flag, _)| *flag == "--software"),
                shaders: value_of("--shaders").map(str::to_owned),
                exit_after: value_of("--exit-after")
                    .map(|value| parse_number("--exit-after", value))
                    .transpose()?,
            })
        }
        "shaders" => Command::Shaders {
            dir: value_of("--dir").map(str::to_owned),
        },
        "examples" => Command::Examples,
        "version" => Command::Version,
        _ => Command::Help,
    };
    Ok(Args { command, json })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn parses_pkg_actions() {
        assert_eq!(
            parse(&args(
                "pkg add com.example.dusk --version ^1 --user --dry-run"
            ))
            .unwrap()
            .command,
            Command::Pkg {
                action: "add".to_owned(),
                rest: vec!["com.example.dusk".to_owned()],
                root: None,
                index: None,
                version: Some("^1".to_owned()),
                user: true,
                replace: false,
                dry_run: true,
            }
        );
        let Command::Pkg { action, rest, .. } =
            parse(&args("pkg search dark theme")).unwrap().command
        else {
            panic!("pkg");
        };
        assert_eq!((action.as_str(), rest.len()), ("search", 2));
        let Command::Pkg { action, .. } = parse(&args("pkg")).unwrap().command else {
            panic!("pkg");
        };
        assert_eq!(action, "list");
        let wrong = parse(&args("pkg instal x")).unwrap_err();
        assert!(wrong.hint.contains("install"), "{wrong:?}");
        assert!(parse(&args("pkg add")).is_err());
        assert!(parse(&args("pkg lock now")).is_err());
    }

    #[test]
    fn parses_run_with_defaults_and_both_value_styles() {
        assert_eq!(
            parse(&args("run --example bounce --json")).unwrap(),
            Args {
                command: Command::Run {
                    example: "bounce".to_owned(),
                    frames: 600,
                    seed: 0,
                    hash_every: 0,
                    scene: None,
                    root: None,
                },
                json: true,
            }
        );
        assert_eq!(
            parse(&args(
                "run --example=bounce --frames=10 --seed 7 --headless --hash-every 5"
            ))
            .unwrap()
            .command,
            Command::Run {
                example: "bounce".to_owned(),
                frames: 10,
                seed: 7,
                hash_every: 5,
                scene: None,
                root: None,
            }
        );
    }

    #[test]
    fn other_commands() {
        assert_eq!(parse(&[]).unwrap().command, Command::Help);
        assert_eq!(parse(&args("--help")).unwrap().command, Command::Help);
        assert_eq!(parse(&args("--version")).unwrap().command, Command::Version);
        assert!(parse(&args("examples --json")).unwrap().json);
        assert_eq!(
            parse(&args("schema")).unwrap().command,
            Command::Schema { example: None }
        );
    }

    #[test]
    fn mistakes_are_explained() {
        let unknown = parse(&args("rn")).unwrap_err();
        assert_eq!(unknown.code, "invalid_arguments");
        assert_eq!(unknown.hint, "Did you mean 'run'?");

        let flag = parse(&args("run --exmaple bounce")).unwrap_err();
        assert_eq!(flag.hint, "Did you mean '--example'?");

        let missing = parse(&args("run --example")).unwrap_err();
        assert_eq!(missing.message, "--example needs a value.");

        let number = parse(&args("run --example bounce --frames lots")).unwrap_err();
        assert_eq!(
            number.message,
            "--frames expects a whole number, but got 'lots'."
        );

        let no_example = parse(&args("run")).unwrap_err();
        assert!(no_example.hint.contains("bounce"), "{}", no_example.hint);

        let valued_switch = parse(&args("run --json=yes")).unwrap_err();
        assert_eq!(valued_switch.message, "--json does not take a value.");
    }

    #[test]
    fn parses_engines_actions() {
        let Command::Engines { action, folder, .. } =
            parse(&args("engines add C:/Wish/0.2.0 --dry-run"))
                .unwrap()
                .command
        else {
            panic!("engines");
        };
        assert_eq!(action, "add");
        assert_eq!(folder.as_deref(), Some("C:/Wish/0.2.0"));
        let Command::Engines { action, .. } = parse(&args("engines")).unwrap().command else {
            panic!("engines");
        };
        assert_eq!(action, "list");
        assert!(parse(&args("engines remove")).is_err());
        assert!(parse(&args("engines list extra")).is_err());
        assert!(parse(&args("engines adopt")).is_err());
        let Command::Edit(edit) = parse(&args("edit --pick-engine --root game"))
            .unwrap()
            .command
        else {
            panic!("edit");
        };
        assert!(edit.pick_engine);
    }
}
