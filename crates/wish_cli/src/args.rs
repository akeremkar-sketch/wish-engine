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
    "shaders", "view", "examples", "help", "version",
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
        /// The example whose component types the documents use.
        example: String,
        /// The project folder (default: the current folder).
        root: Option<String>,
    },
    /// Rewrite documents in canonical form.
    Format {
        /// Format only these project-relative files (all when empty).
        paths: Vec<String>,
        /// The example whose component types the documents use.
        example: String,
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
    /// What to draw: `lit`, `ids`, `depth`, `normals` or `wireframe`.
    pub view: String,
    /// Use the software adapter.
    pub software: bool,
    /// A folder of `<name>.wgsl` files that replace the built-in shaders.
    pub shaders: Option<String>,
    /// Where to write the PNG.
    pub out: String,
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

    let accepted = flags_of(command);
    let mut values: Vec<(&str, Option<String>)> = Vec::new();
    let mut positional: Vec<&str> = Vec::new();
    let mut index = 0;
    while let Some(argument) = rest.get(index) {
        index += 1;
        if matches!(command, "mv" | "check" | "fmt" | "migrate") && !argument.starts_with('-') {
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
        "bench" => Command::Bench {
            entities: value_of("--entities")
                .map_or(Ok(100_000), |value| parse_number("--entities", value))?
                .try_into()
                .map_err(|_| {
                    usage_error(
                        "--entities is too large.".to_owned(),
                        "Use at most 4294967294 entities.".to_owned(),
                    )
                })?,
            ticks: value_of("--ticks").map_or(Ok(600), |value| parse_number("--ticks", value))?,
            seed: value_of("--seed").map_or(Ok(0), |value| parse_number("--seed", value))?,
        },
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
        "check" | "fmt" | "watch" => {
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
            let root = value_of("--root").map(str::to_owned);
            let paths: Vec<String> = positional.iter().map(|path| (*path).to_owned()).collect();
            match command {
                "check" => Command::Check { paths, example, root },
                "fmt" => Command::Format {
                    paths,
                    example,
                    root,
                    check: values.iter().any(|(flag, _)| *flag == "--check"),
                },
                _ => Command::Watch {
                    example,
                    scene: value_of("--scene")
                        .ok_or_else(|| {
                            usage_error(
                                "'wen watch' needs a --scene to load.".to_owned(),
                                "Add --scene <file>, a path relative to the project folder.".to_owned(),
                            )
                        })?
                        .to_owned(),
                    root,
                    seconds: value_of("--seconds")
                        .map_or(Ok(0), |value| parse_number("--seconds", value))?,
                },
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
}
