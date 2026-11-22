//! Scenario tests (ADR-017, M5 §10): `tests/*.test.json`.
//!
//! ```json
//! { "format": 1, "scene": "scenes/main.scene.json", "seed": 1,
//!   "steps": [
//!     { "input": { "move": [1, 0] }, "frames": 120 },
//!     { "expect": "Score.points >= 3", "message": "the player collects three coins" },
//!     { "until": "GameState.won == true", "max_frames": 600 },
//!     { "capture": { "camera": "camera", "golden": "tests/golden/win.png" } }
//!   ] }
//! ```
//!
//! A test starts a session on the scene with the seed, runs its steps in
//! order, and fails at the first step that does not hold, saying which
//! step, at which tick, and what the values in the condition were.
//!
//! A game for several players (release 1.1) says how many at the top,
//! `"players": 2`, and a run step gives the others' input by player id,
//! `{ "input": { "move": [1, 0] }, "players": { "1": { "move": [-1, 0] } }, "frames": 60 }`
//! (`input` is player 0's). Without `players` a test has one player, as
//! every test before it.
//!
//! A test of an online game (release 1.1, design note
//! `docs/design/1.1-net-tool.md` §5) says `"net"` at the top, the arguments
//! of `net host` (`players` seats, `bots`, `drive`, `auto_start`,
//! `host_plays`, `network`, `room_code`); its steps run on the networked
//! session, `players` in a run step gives input by seat, and an `expect`
//! with `"player": 3` checks what player 3's machine has. When every step
//! holds, the host's match log is played into a fresh host world and must
//! give the same world at every tick (`replay_verified`).

use std::path::Path;

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_render::{RgbaImage, Tolerance};

use crate::expr::{Expr, Val};
use crate::sessions::{LiveSubject, entity_names, name_of};
use crate::workspace::Workspace;

/// The folder scenario tests live in.
pub(crate) const FOLDER: &str = "tests";
const SUFFIX: &str = ".test.json";
/// Where a failed capture's picture goes.
const OUTPUT_FOLDER: &str = ".wish/test-output";
const MAX_STEPS: usize = 500;
const MAX_TESTS: usize = 200;
const MAX_FRAMES_PER_STEP: i64 = 100_000;

fn invalid(file: &str, path: &str, what: &str) -> Diagnostic {
    Diagnostic::error(
        "invalid_test_file",
        format!("'{file}' at {path}: {what}."),
        "A test file looks like { \"format\": 1, \"scene\": \"scenes/main.scene.json\", \"seed\": 1, \"steps\": [ { \"input\": { \"move\": [1, 0] }, \"frames\": 60 }, { \"expect\": \"Score.points >= 1\" } ] }; a step is frames, until, expect or capture.",
    )
    .at_file(file)
}

/// One step of a test.
#[derive(Clone, Debug)]
enum Step {
    Run {
        input: Option<Value>,
        players: Option<Value>,
        frames: u64,
        until: Option<String>,
    },
    Expect {
        expression: String,
        entity: Option<String>,
        message: Option<String>,
        /// The seat whose machine's world is checked (a networked test).
        player: Option<u32>,
    },
    Capture {
        camera: Option<String>,
        golden: String,
        size: (u32, u32),
        channel: u8,
        fraction: f64,
        view: String,
    },
}

#[derive(Clone, Debug)]
struct Spec {
    scene: String,
    seed: u64,
    players: usize,
    /// A networked match: the `net host` arguments (release 1.1).
    net: Option<Value>,
    steps: Vec<Step>,
}

/// The keys of a test's `net`: `net host`'s own.
const NET_KEYS: &[&str] = &[
    "auto_start",
    "bots",
    "drive",
    "host_plays",
    "network",
    "players",
    "room_code",
];

fn text_of(step: &Value, key: &str) -> Option<String> {
    step.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn parse(text: &str, file: &str) -> Result<Spec> {
    let value: Value = serde_json::from_str(text).map_err(|error| {
        invalid(file, "/", &error.to_string()).at_line(
            u32::try_from(error.line()).unwrap_or(0),
            u32::try_from(error.column()).ok(),
        )
    })?;
    if value.get("format") != Some(&json!(1)) {
        return Err(invalid(file, "/format", "the engine reads format 1"));
    }
    let scene = text_of(&value, "scene")
        .ok_or_else(|| invalid(file, "/scene", "a test names its scene"))?;
    let seed = value.get("seed").and_then(Value::as_u64).unwrap_or(0);
    let players = match value.get("players") {
        None => 1,
        Some(count) => count
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=wish_input::MAX_PLAYERS).contains(n))
            .ok_or_else(|| {
                invalid(
                    file,
                    "/players",
                    &format!(
                        "'players' is how many players the game has, 1 to {}",
                        wish_input::MAX_PLAYERS
                    ),
                )
            })?,
    };
    let net = match value.get("net") {
        None => None,
        Some(Value::Object(given)) => {
            if let Some(extra) = given.keys().find(|key| !NET_KEYS.contains(&key.as_str())) {
                return Err(invalid(
                    file,
                    &format!("/net/{extra}"),
                    &format!(
                        "'{extra}' is not a setting of a test's match; they are {}",
                        NET_KEYS.join(", ")
                    ),
                ));
            }
            if value.get("players").is_some() {
                return Err(invalid(
                    file,
                    "/players",
                    "a networked test says its seats in net.players, not at the top",
                ));
            }
            Some(Value::Object(given.clone()))
        }
        Some(_) => {
            return Err(invalid(
                file,
                "/net",
                "'net' is the match, like { \"players\": 6, \"bots\": 5, \"auto_start\": true, \"network\": { \"latency_ms\": 100, \"loss\": 0.05 } }",
            ));
        }
    };
    let list = value
        .get("steps")
        .and_then(Value::as_array)
        .filter(|steps| !steps.is_empty() && steps.len() <= MAX_STEPS)
        .ok_or_else(|| {
            invalid(
                file,
                "/steps",
                &format!("a test has 1 to {MAX_STEPS} steps"),
            )
        })?;
    let mut steps = Vec::new();
    for (index, step) in list.iter().enumerate() {
        let at = format!("/steps/{index}");
        let known = [
            "input",
            "players",
            "frames",
            "until",
            "max_frames",
            "expect",
            "entity",
            "message",
            "capture",
            "player",
        ];
        if let Some(object) = step.as_object()
            && let Some(extra) = object.keys().find(|key| !known.contains(&key.as_str()))
        {
            return Err(invalid(
                file,
                &format!("{at}/{extra}"),
                &format!("'{extra}' is not a step key; they are {}", known.join(", ")),
            ));
        }
        if let Some(expression) = text_of(step, "expect") {
            let player = match step.get("player") {
                None => None,
                Some(seat) => Some(
                    seat.as_u64()
                        .and_then(|s| u32::try_from(s).ok())
                        .filter(|s| *s < 16 && net.is_some())
                        .ok_or_else(|| {
                            invalid(
                                file,
                                &format!("{at}/player"),
                                "'player' is a seat (0 to 15) of a networked test (one with net)",
                            )
                        })?,
                ),
            };
            steps.push(Step::Expect {
                expression,
                entity: text_of(step, "entity"),
                message: text_of(step, "message"),
                player,
            });
        } else if let Some(capture) = step.get("capture") {
            let golden = text_of(capture, "golden").ok_or_else(|| {
                invalid(
                    file,
                    &format!("{at}/capture/golden"),
                    "a capture names its golden image",
                )
            })?;
            let number = |key: &str, default: f64| {
                capture.get(key).and_then(Value::as_f64).unwrap_or(default)
            };
            steps.push(Step::Capture {
                camera: text_of(capture, "camera"),
                golden,
                size: (
                    number("width", 320.0).clamp(16.0, 2048.0) as u32,
                    number("height", 180.0).clamp(16.0, 2048.0) as u32,
                ),
                channel: number("tolerance", 2.0).clamp(0.0, 255.0) as u8,
                fraction: number("fraction", 0.001).clamp(0.0, 1.0),
                view: text_of(capture, "view").unwrap_or_else(|| "lit".to_owned()),
            });
        } else {
            let until = text_of(step, "until");
            let frames = match (step.get("frames"), step.get("max_frames")) {
                (Some(frames), _) => frames.as_i64(),
                (None, Some(max)) => max.as_i64(),
                (None, None) if until.is_some() => Some(10_000),
                (None, None) => None,
            };
            let frames = frames
                .filter(|n| (1..=MAX_FRAMES_PER_STEP).contains(n))
                .ok_or_else(|| {
                    invalid(
                        file,
                        &format!("{at}/frames"),
                        &format!("a step runs 'frames' ticks (1 to {MAX_FRAMES_PER_STEP}), or checks 'expect', or takes a 'capture'"),
                    )
                })?;
            steps.push(Step::Run {
                input: step.get("input").cloned(),
                players: step.get("players").cloned(),
                frames: u64::try_from(frames).unwrap_or(1),
                until,
            });
        }
    }
    Ok(Spec {
        scene,
        seed,
        players,
        net,
        steps,
    })
}

fn val_json(value: &Val) -> Value {
    match value {
        Val::Null => Value::Null,
        Val::Bool(flag) => json!(flag),
        Val::Num(number) => json!(number),
        Val::Str(text) => json!(text),
        Val::List(items) => Value::Array(items.iter().map(val_json).collect()),
    }
}

/// The values a condition reads, for the world and for the entities that
/// have any of them (the first few), so a failure says what was there.
fn explain(
    world: &wish_ecs::World,
    expression: &Expr,
    only: Option<(&str, wish_core::Entity)>,
) -> Value {
    let non_null = |values: Vec<(String, Val)>| -> Map<String, Value> {
        values
            .into_iter()
            .filter(|(_, value)| *value != Val::Null)
            .map(|(path, value)| (path, val_json(&value)))
            .collect()
    };
    let world_values = non_null(expression.path_values(&LiveSubject {
        world,
        id: "",
        entity: None,
    }));
    let mut entities = Vec::new();
    let candidates: Vec<(String, wish_core::Entity)> = match only {
        Some((name, entity)) => vec![(name.to_owned(), entity)],
        None => world.entities().map(|e| (name_of(world, e), e)).collect(),
    };
    for (name, entity) in candidates {
        let values = non_null(expression.path_values(&LiveSubject {
            world,
            id: &name,
            entity: Some(entity),
        }));
        let from_entity: Map<String, Value> = values
            .into_iter()
            .filter(|(path, value)| world_values.get(path) != Some(value))
            .collect();
        if !from_entity.is_empty() {
            entities.push(json!({ "entity": name, "values": from_entity }));
            if entities.len() >= 5 {
                break;
            }
        }
    }
    json!({ "world": world_values, "entities": entities })
}

fn step_failure(step: usize, kind: &str, tick: u64, message: String) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("step".to_owned(), json!(step));
    out.insert("kind".to_owned(), json!(kind));
    out.insert("tick".to_owned(), json!(tick));
    out.insert("message".to_owned(), json!(message));
    out
}

impl Workspace {
    /// Every `tests/**/*.test.json` of the project, project-relative.
    pub(crate) fn test_files(&self) -> Vec<String> {
        let mut found = Vec::new();
        let mut folders = vec![self.root().join(FOLDER)];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                let path = entry.path();
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_dir() {
                    folders.push(path);
                } else if let Ok(relative) = path.strip_prefix(self.root())
                    && let Some(text) = relative.to_str()
                    && text.ends_with(SUFFIX)
                {
                    found.push(text.replace('\\', "/"));
                }
            }
        }
        found.sort();
        found
    }

    /// Runs the tests at `paths` (all of them when empty). Pictures are drawn
    /// on the software adapter; the workspace's own choice is put back after.
    pub(crate) fn test_run(
        &mut self,
        paths: &[String],
        update_golden: bool,
        dry_run: bool,
    ) -> Result<Map<String, Value>> {
        let software = self.software;
        let result = self.test_run_inner(paths, update_golden, dry_run);
        self.set_software(software);
        result
    }

    fn test_run_inner(
        &mut self,
        paths: &[String],
        update_golden: bool,
        dry_run: bool,
    ) -> Result<Map<String, Value>> {
        let files = if paths.is_empty() {
            self.test_files()
        } else {
            let all = self.test_files();
            for path in paths {
                // Only the project's own tests: no `..`, no absolute paths.
                if !path.ends_with(SUFFIX) || !all.contains(&path.replace('\\', "/")) {
                    return Err(Diagnostic::error(
                        "test_not_found",
                        format!("'{path}' is not a test file of the project."),
                        if all.is_empty() {
                            "Tests are files such as tests/win.test.json.".to_owned()
                        } else {
                            wish_core::suggest::unknown_name_hint(
                                path,
                                &all.iter().map(String::as_str).collect::<Vec<_>>(),
                            )
                        },
                    )
                    .at_path("/paths")
                    .with_candidates(all));
                }
            }
            paths.iter().map(|p| p.replace('\\', "/")).collect()
        };
        if files.len() > MAX_TESTS {
            return Err(Diagnostic::error(
                "too_many_tests",
                format!(
                    "The project has {} tests; one run does at most {MAX_TESTS}.",
                    files.len()
                ),
                "Run a part of them with paths.",
            ));
        }
        let mut results = Vec::new();
        let mut failed = 0;
        for file in &files {
            let result = self.test_one(file, update_golden, dry_run);
            if result["passed"] != true {
                failed += 1;
            }
            results.push(result);
        }
        let mut out = Map::new();
        out.insert("passed".to_owned(), json!(failed == 0));
        out.insert("tests".to_owned(), json!(files.len()));
        out.insert("failed".to_owned(), json!(failed));
        out.insert("results".to_owned(), json!(results));
        Ok(out)
    }

    fn test_one(&mut self, file: &str, update_golden: bool, dry_run: bool) -> Value {
        let name = file
            .strip_prefix(&format!("{FOLDER}/"))
            .unwrap_or(file)
            .trim_end_matches(SUFFIX)
            .to_owned();
        let setup = || -> Result<Spec> {
            let text = std::fs::read_to_string(self.root().join(file)).map_err(|error| {
                Diagnostic::error(
                    "file_unreadable",
                    format!("Could not read '{file}': {error}."),
                    if error.kind() == std::io::ErrorKind::InvalidData {
                        "The file is not UTF-8 text (a tool wrote it in a local code page?). Save it as UTF-8, without a byte order mark."
                    } else {
                        "Check that the file can be read."
                    },
                )
                .at_file(file)
            })?;
            parse(&text, file)
        };
        let spec = match setup() {
            Ok(spec) => spec,
            Err(problem) => {
                return json!({ "file": file, "name": name, "passed": false, "ticks": 0, "failure": problem_failure("setup", &problem) });
            }
        };
        let started = (|| -> Result<String> {
            let scene = self.scene_path(Some(&spec.scene))?;
            self.open_document(&scene, false)?;
            if let Some(net) = &spec.net {
                let mut args = net.clone();
                args["action"] = json!("host");
                args["scene"] = json!(scene);
                args["seed"] = json!(spec.seed);
                let answer = self.run_tool("net", &args, &crate::parse_actor("test"))?;
                return Ok(answer["session"].as_str().unwrap_or_default().to_owned());
            }
            Ok(self
                .start_session(&scene, spec.seed, spec.players, Vec::new())?
                .0)
        })();
        let id = match started {
            Ok(id) => id,
            Err(problem) => {
                return json!({ "file": file, "name": name, "passed": false, "ticks": 0, "failure": problem_failure("setup", &problem) });
            }
        };
        let mut outcome = self.run_steps(&id, &spec, &name, update_golden, dry_run);
        let ticks = self.session(&id).map_or(0, |s| s.app.tick());
        // A match is also its replay: the host's log played into a fresh
        // host world must give the same world at every tick.
        let mut verified = None;
        if spec.net.is_some() && outcome.is_none() {
            match self.verify_match(&id) {
                Ok(None) => verified = Some(true),
                Ok(Some(tick)) => {
                    outcome = Some(step_failure(
                        spec.steps.len(),
                        "replay_diverged",
                        tick,
                        format!(
                            "The host's match log, played into a fresh host world, differs from the match at tick {tick}."
                        ),
                    ));
                }
                Err(problem) => outcome = Some(problem_failure("replay", &problem)),
            }
        }
        self.sessions.open.remove(&id);
        let mut result = json!({ "file": file, "name": name, "passed": outcome.is_none(), "ticks": ticks, "steps": spec.steps.len() });
        if let Some(verified) = verified {
            result["replay_verified"] = json!(verified);
        }
        if let Some(found) = outcome {
            result["failure"] = Value::Object(found);
        }
        result
    }

    /// Runs the steps; `None` when all held, else the first failure.
    fn run_steps(
        &mut self,
        id: &str,
        spec: &Spec,
        name: &str,
        update_golden: bool,
        dry_run: bool,
    ) -> Option<Map<String, Value>> {
        for (index, step) in spec.steps.iter().enumerate() {
            let tick = self.session(id).map_or(0, |s| s.app.tick());
            let failed = match step {
                Step::Run {
                    input,
                    players,
                    frames,
                    until,
                } => self.run_step(
                    id,
                    index,
                    (input.as_ref(), players.as_ref()),
                    *frames,
                    until.as_deref(),
                ),
                Step::Expect {
                    expression,
                    entity,
                    message,
                    player,
                } => self.expect_step(
                    id,
                    index,
                    expression,
                    (entity.as_deref(), *player),
                    message.as_deref(),
                ),
                Step::Capture {
                    camera,
                    golden,
                    size,
                    channel,
                    fraction,
                    view,
                } => self.capture_step(
                    id,
                    index,
                    name,
                    (camera.as_deref(), golden, *size, view),
                    Tolerance {
                        channel: *channel,
                        fraction: *fraction,
                    },
                    (update_golden, dry_run),
                ),
            };
            if let Some(mut found) = failed {
                found.insert(
                    "tick".to_owned(),
                    json!(self.session(id).map_or(tick, |s| s.app.tick())),
                );
                return Some(found);
            }
        }
        None
    }

    fn run_step(
        &mut self,
        id: &str,
        index: usize,
        (input, players): (Option<&Value>, Option<&Value>),
        frames: u64,
        until: Option<&str>,
    ) -> Option<Map<String, Value>> {
        let tick = self.session(id).map_or(0, |s| s.app.tick());
        let values = match crate::tools::parse_inputs(self, id, input, players) {
            Ok(values) => values,
            Err(problem) => return Some(problem_failure("input", &problem).with_step(index)),
        };
        let condition = match until {
            Some(text) => match Expr::compile(text, self.registry()) {
                Ok(expression) => Some(expression),
                Err(problem) => return Some(problem_failure("until", &problem).with_step(index)),
            },
            None => None,
        };
        let named = crate::tools::named_players(input, players, values.len());
        let stepped = match self.step_session(id, frames, condition.as_ref(), (&values, &named)) {
            Ok((stepped, ..)) => stepped,
            Err(problem) => return Some(problem_failure("step", &problem).with_step(index)),
        };
        if let Some(broken) = stepped.diagnostics.iter().find(|d| d.is_error()) {
            let mut found = step_failure(index, "system_failed", tick, broken.message.clone());
            found.insert("diagnostic".to_owned(), json!(broken));
            return Some(found);
        }
        if let (Some(text), true) = (until, stepped.stopped_by != "until") {
            let mut found = step_failure(
                index,
                "until_not_reached",
                tick,
                format!(
                    "'{text}' did not become true within {} ticks.",
                    stepped.ticks_run
                ),
            );
            if let (Some(expression), Ok(session)) = (&condition, self.session(id)) {
                found.insert(
                    "values".to_owned(),
                    explain(session.app.world(), expression, None),
                );
            }
            return Some(found);
        }
        None
    }

    fn expect_step(
        &mut self,
        id: &str,
        index: usize,
        text: &str,
        (entity, player): (Option<&str>, Option<u32>),
        message: Option<&str>,
    ) -> Option<Map<String, Value>> {
        let tick = self.session(id).map_or(0, |s| s.app.tick());
        let expression = match Expr::compile(text, self.registry()) {
            Ok(expression) => expression,
            Err(problem) => return Some(problem_failure("expect", &problem).with_step(index)),
        };
        if entity.is_none() && expression.negates_has() {
            let mut found = step_failure(
                index,
                "expect_ambiguous",
                tick,
                format!(
                    "'{text}' negates has() without naming an entity: it would hold as soon as any one entity lacks the component."
                ),
            );
            found.insert(
                "hint".to_owned(),
                json!("To say that no entity has a component, write count(Coin) == 0; to check one entity, add \"entity\": \"<id>\" to the step."),
            );
            return Some(found);
        }
        let world = match self.session_world(id, player) {
            Ok(world) => world,
            Err(problem) => return Some(problem_failure("expect", &problem).with_step(index)),
        };
        let only = match entity {
            Some(wanted) => match entity_names(world).get(wanted) {
                Some(found) => Some((wanted, *found)),
                None => {
                    let names: Vec<String> = entity_names(world).into_keys().collect();
                    let mut found = step_failure(
                        index,
                        "entity_missing",
                        tick,
                        format!("There is no entity '{wanted}' to check."),
                    );
                    found.insert("candidates".to_owned(), json!(names));
                    return Some(found);
                }
            },
            None => None,
        };
        let holds = match only {
            Some((wanted, found)) => expression.matches(&LiveSubject {
                world,
                id: wanted,
                entity: Some(found),
            }),
            None => {
                expression.matches(&LiveSubject {
                    world,
                    id: "",
                    entity: None,
                }) || world.entities().any(|e| {
                    expression.matches(&LiveSubject {
                        world,
                        id: &name_of(world, e),
                        entity: Some(e),
                    })
                })
            }
        };
        if holds {
            return None;
        }
        let mut found = step_failure(
            index,
            "expect",
            tick,
            message.map_or_else(|| format!("'{text}' does not hold."), str::to_owned),
        );
        found.insert("expression".to_owned(), json!(text));
        found.insert("values".to_owned(), explain(world, &expression, only));
        Some(found)
    }

    fn capture_step(
        &mut self,
        id: &str,
        index: usize,
        test: &str,
        (camera, golden, size, view): (Option<&str>, &str, (u32, u32), &str),
        tolerance: Tolerance,
        (update_golden, dry_run): (bool, bool),
    ) -> Option<Map<String, Value>> {
        let tick = self.session(id).map_or(0, |s| s.app.tick());
        let golden = match crate::documents::normalize_output_path(golden) {
            Ok(path) => path,
            Err(problem) => return Some(problem_failure("capture", &problem).with_step(index)),
        };
        // The same picture on every machine: golden images are drawn on the
        // software adapter (ADR-017).
        self.set_software(true);
        let mut args = json!({ "session": id, "width": size.0, "height": size.1, "view": view });
        if let Some(camera) = camera {
            args["camera"] = json!(camera);
        }
        let answer = self.call("capture_frame", &args, &crate::parse_actor("test"));
        let Some(image) = answer["image"]["data"]
            .as_str()
            .and_then(crate::render::base64_decode)
            .and_then(|bytes| RgbaImage::from_png(&bytes).ok())
        else {
            let problem = serde_json::from_value::<Diagnostic>(answer["error"].clone())
                .unwrap_or_else(|_| {
                    Diagnostic::error(
                        "capture_failed",
                        "The picture could not be taken.",
                        "Check that the scene has a camera.",
                    )
                });
            return Some(problem_failure("capture", &problem).with_step(index));
        };
        let target = self.root().join(&golden);
        let write = |path: &Path, image: &RgbaImage| -> Result<()> {
            let png = image.to_png()?;
            let relative = path.strip_prefix(self.root()).map_or_else(
                |_| golden.clone(),
                |p| p.to_string_lossy().replace('\\', "/"),
            );
            let real = crate::documents::prepare_output(self.root(), &relative)?;
            std::fs::write(real, png).map_err(|error| {
                Diagnostic::error(
                    "file_unwritable",
                    format!("Could not write '{relative}': {error}."),
                    "Check that the folder can be written to.",
                )
            })
        };
        let existing = std::fs::read(&target)
            .ok()
            .and_then(|bytes| RgbaImage::from_png(&bytes).ok());
        if update_golden {
            if !dry_run && let Err(problem) = write(&target, &image) {
                return Some(problem_failure("capture", &problem).with_step(index));
            }
            return None;
        }
        let Some(expected) = existing else {
            let mut found = step_failure(
                index,
                "golden_missing",
                tick,
                format!("The golden image '{golden}' does not exist."),
            );
            found.insert("hint".to_owned(), json!("Look at the picture (capture_frame), and if it is right, run test_run with update_golden to save it."));
            return Some(found);
        };
        let diff = image.diff(&expected, tolerance.channel);
        let fine = diff.is_some_and(|d| d.differing_fraction <= tolerance.fraction);
        if fine {
            return None;
        }
        let output = format!("{OUTPUT_FOLDER}/{}-step{index}.png", test.replace('/', "-"));
        if !dry_run {
            let _ = write(&self.root().join(&output), &image);
        }
        let mut found = step_failure(
            index,
            "capture",
            tick,
            match diff {
                Some(d) => format!(
                    "The picture differs from '{golden}': {:.2}% of the pixels differ by more than {} (the largest difference is {}).",
                    d.differing_fraction * 100.0,
                    tolerance.channel,
                    d.max_channel
                ),
                None => format!(
                    "The picture is {}x{}, the golden image '{golden}' is {}x{}.",
                    image.width, image.height, expected.width, expected.height
                ),
            },
        );
        found.insert("golden".to_owned(), json!(golden));
        found.insert(
            "actual".to_owned(),
            json!(if dry_run { Value::Null } else { json!(output) }),
        );
        found.insert("hint".to_owned(), json!("Open the picture it saved and the golden image. If the new picture is right, run test_run with update_golden."));
        Some(found)
    }
}

/// A failure that is a diagnostic.
fn problem_failure(kind: &str, problem: &Diagnostic) -> Map<String, Value> {
    let mut found = step_failure(0, kind, 0, problem.message.clone());
    found.insert("diagnostic".to_owned(), json!(problem));
    found
}

trait WithStep {
    fn with_step(self, step: usize) -> Self;
}

impl WithStep for Map<String, Value> {
    fn with_step(mut self, step: usize) -> Self {
        self.insert("step".to_owned(), json!(step));
        self
    }
}
