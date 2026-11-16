//! Simulation tools: `sim_start`, `sim_step`, `sim_snapshot`, `sim_restore`,
//! `sim_stop`, and the log, `logs_get`.

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result, Severity};
use wish_scene::Actor;

use crate::expr::Expr;
use crate::param::{Args, Kind, Param};
use crate::sessions::{MAX_FRAMES, Saved, unknown_snapshot};
use crate::tool::Tool;
use crate::workspace::Workspace;

const SESSION: Param = Param::optional(
    "session",
    Kind::String,
    "The session id from sim_start, such as \"s1\" (optional when only one session is open).",
);

pub(crate) const SIM_START: Tool = Tool {
    name: "sim_start",
    title: "Start a play session",
    description: "Starts a play session: the scene turned into a live world that runs deterministically (same scene and seed, same result, every time). Sessions never change files. Returns the session id; then use sim_step to run it, entity_query / entity_get with the session to look at it, and capture_frame to see it.",
    params: &[
        Param::optional(
            "scene",
            Kind::String,
            "The scene file (default: the project's scene).",
        ),
        Param::optional(
            "seed",
            Kind::Integer {
                min: 0,
                max: i64::MAX,
            },
            "The random seed: the same seed gives the same run.",
        )
        .or("0"),
    ],
    mutates: false,
    run: sim_start,
};

pub(crate) const SIM_STEP: Tool = Tool {
    name: "sim_step",
    title: "Run the simulation",
    description: "Runs the session for `frames` ticks (60 per simulated second), or until `until` becomes true (at most max_frames). `until` is a condition like \"Transform.translation.y < 0\" or \"Stats.bounces >= 10\"; it holds when it is true for any entity or for the world's resources, and the run stops right after that tick. Answers the tick reached, the world hash (equal hashes mean identical worlds), the events that happened by type, and what looks wrong: systems that failed, entities with NaN transforms or far out of the world.",
    params: &[
        SESSION,
        Param::optional(
            "frames",
            Kind::Integer {
                min: 1,
                max: MAX_FRAMES,
            },
            "Run this many ticks (default 1, or max_frames when until is given).",
        ),
        Param::optional(
            "until",
            Kind::String,
            "Stop early when this condition holds.",
        ),
        Param::optional(
            "max_frames",
            Kind::Integer {
                min: 1,
                max: MAX_FRAMES,
            },
            "The most ticks to run when stopping on until.",
        )
        .or("10000"),
        Param::optional(
            "input",
            Kind::Object,
            "What the player does during these ticks, by action name (input.json): a button is true or false, an axis a number from -1 to 1, a two-axis action [x, y] such as {\"move\": [0, 1], \"jump\": true}. Held for every tick of the call and released afterwards; game code sees it through Actions.",
        ),
    ],
    mutates: false,
    run: sim_step,
};

pub(crate) const SIM_SNAPSHOT: Tool = Tool {
    name: "sim_snapshot",
    title: "Save or list snapshots",
    description: "Saves the session's exact state under a label so that it can be returned to (sim_restore), or lists the saved snapshots with their tick and world hash.",
    params: &[
        SESSION,
        Param::optional(
            "action",
            Kind::Choice(&["save", "list"]),
            "save a snapshot, or list them.",
        )
        .or("\"save\""),
        Param::optional(
            "label",
            Kind::String,
            "A name for the snapshot (default: snap1, snap2, ...).",
        ),
    ],
    mutates: false,
    run: sim_snapshot,
};

pub(crate) const SIM_RESTORE: Tool = Tool {
    name: "sim_restore",
    title: "Return to a snapshot",
    description: "Returns the session to a saved snapshot, or with fork starts a new session from it and leaves this one as it is (to try something two ways). Everything after the snapshot is forgotten in the restored session; its other snapshots stay.",
    params: &[
        SESSION,
        Param::required("snapshot", Kind::String, "The snapshot label."),
        Param::optional(
            "fork",
            Kind::Boolean,
            "Start a new session from the snapshot instead of changing this one.",
        ),
    ],
    mutates: false,
    run: sim_restore,
};

pub(crate) const SIM_STOP: Tool = Tool {
    name: "sim_stop",
    title: "Close a session",
    description: "Closes a play session and frees its world. Its snapshots are lost with it. Sessions are never saved to disk: to keep something found in a session, make the same change in the scene with edit_apply.",
    params: &[SESSION],
    mutates: false,
    run: sim_stop,
};

pub(crate) const LOGS_GET: Tool = Tool {
    name: "logs_get",
    title: "Structured log",
    description: "The log of what happened in sessions: sessions started, systems that failed (and were switched off), and what the inspector noticed. Newest last. Pass the last seq you saw as since to get only what is new.",
    params: &[
        Param::optional("session", Kind::String, "Only this session's entries."),
        Param::optional(
            "since",
            Kind::Integer {
                min: 0,
                max: i64::MAX,
            },
            "Only entries after this seq.",
        )
        .or("0"),
        Param::optional(
            "severity",
            Kind::Choice(&["info", "warning", "error"]),
            "Only entries at least this serious.",
        )
        .or("\"info\""),
        Param::optional(
            "kinds",
            Kind::StringList,
            "Only these kinds: session, system, inspector, script (game code logs and reloads).",
        ),
        Param::optional(
            "limit",
            Kind::Integer { min: 1, max: 500 },
            "At most this many.",
        )
        .or("100"),
    ],
    mutates: false,
    run: logs_get,
};

fn sim_start(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let path = workspace.scene_path(args.str("scene"))?;
    let notes = workspace.open_document(&path, false)?;
    let seed = u64::try_from(args.int("seed").unwrap_or(0)).unwrap_or(0);
    let (id, notes) = workspace.start_session(&path, seed, notes)?;
    let session = workspace.session(&id)?;
    let mut out = Map::new();
    out.insert("session".to_owned(), json!(id));
    out.insert("scene".to_owned(), json!(path));
    out.insert("seed".to_owned(), json!(seed));
    out.insert("tick".to_owned(), json!(session.app.tick()));
    out.insert(
        "entities".to_owned(),
        json!(session.app.world().entities().count()),
    );
    out.insert(
        "world_hash".to_owned(),
        json!(session.app.hash().to_string()),
    );
    let systems: Vec<&str> = session.app.system_states().map(|(name, _)| name).collect();
    out.insert("systems".to_owned(), json!(systems));
    if !notes.is_empty() {
        out.insert("diagnostics".to_owned(), json!(notes));
    }
    Ok(out)
}

fn sim_step(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let id = workspace.session_id(args.str("session"))?;
    let until = match args.str("until") {
        Some(text) => Some(
            Expr::compile(text, workspace.registry()).map_err(|problem| {
                let column = problem.location.column;
                let mut problem = problem.at_path("/until");
                problem.location.line = None;
                problem.location.column = column;
                problem
            })?,
        ),
        None => None,
    };
    // frames when given; else max_frames when waiting for a condition; else one tick.
    let frames = match (args.int("frames"), &until) {
        (Some(frames), _) => frames,
        (None, Some(_)) => args.int("max_frames").unwrap_or(10_000),
        (None, None) => 1,
    };
    let frames = u64::try_from(frames).unwrap_or(1);
    let input = parse_input(workspace, &id, args.value("input"))?;
    let (stepped, tick, hash, entities) =
        workspace.step_session(&id, frames, until.as_ref(), &input)?;
    let mut out = Map::new();
    out.insert("session".to_owned(), json!(id));
    out.insert("tick".to_owned(), json!(tick));
    out.insert("ticks_run".to_owned(), json!(stepped.ticks_run));
    out.insert("stopped_by".to_owned(), json!(stepped.stopped_by));
    out.insert("world_hash".to_owned(), json!(hash));
    out.insert("entities".to_owned(), json!(entities));
    out.insert("events".to_owned(), json!(stepped.events));
    if !stepped.matched.is_empty() {
        out.insert("matched".to_owned(), json!(stepped.matched));
    }
    if !stepped.diagnostics.is_empty() {
        out.insert("diagnostics".to_owned(), json!(stepped.diagnostics));
    }
    Ok(out)
}

fn sim_snapshot(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let id = workspace.session_id(args.str("session"))?;
    let session = workspace.session_mut(&id)?;
    let mut out = Map::new();
    out.insert("session".to_owned(), json!(id));
    if args.str("action") == Some("list") {
        let list: Vec<Value> = session
            .saved
            .iter()
            .map(|s| json!({ "label": s.label, "tick": s.tick, "world_hash": s.hash }))
            .collect();
        out.insert("snapshots".to_owned(), json!(list));
        return Ok(out);
    }
    let label = match args.str("label") {
        Some(label) if !label.trim().is_empty() && label.chars().count() <= 60 => label.to_owned(),
        Some(_) => {
            return Err(Diagnostic::error(
                "invalid_parameter",
                "A snapshot label must be 1 to 60 characters.",
                "Pass something like \"before_jump\".",
            )
            .at_path("/label"));
        }
        None => {
            let mut number = 1;
            while session
                .saved
                .iter()
                .any(|s| s.label == format!("snap{number}"))
            {
                number += 1;
            }
            format!("snap{number}")
        }
    };
    if session.saved.len() >= crate::sessions::MAX_SNAPSHOTS {
        return Err(Diagnostic::error(
            "too_many_snapshots",
            format!(
                "The session already holds {} snapshots.",
                crate::sessions::MAX_SNAPSHOTS
            ),
            "Restore or fork from one you have, or start a new session (sim_stop frees them).",
        ));
    }
    if session.saved.iter().any(|s| s.label == label) {
        return Err(Diagnostic::error(
            "snapshot_exists",
            format!("The session already has a snapshot '{label}'."),
            "Choose another label; snapshots are not overwritten.",
        )
        .at_path("/label"));
    }
    let snapshot = session.app.snapshot();
    let hash = session.app.hash().to_string();
    let tick = session.app.tick();
    session.saved.push(Saved {
        label: label.clone(),
        tick,
        hash: hash.clone(),
        snapshot,
    });
    out.insert("label".to_owned(), json!(label));
    out.insert("tick".to_owned(), json!(tick));
    out.insert("world_hash".to_owned(), json!(hash));
    Ok(out)
}

fn sim_restore(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let id = workspace.session_id(args.str("session"))?;
    let label = args.str("snapshot").unwrap_or_default().to_owned();
    let mut out = Map::new();
    let target = if args.flag("fork") {
        let fork = workspace.fork_session(&id, &label)?;
        out.insert("forked_from".to_owned(), json!(id));
        fork
    } else {
        let session = workspace.session_mut(&id)?;
        let snapshot = session
            .saved
            .iter()
            .find(|s| s.label == label)
            .map(|s| s.snapshot.clone())
            .ok_or_else(|| unknown_snapshot(&label, &session.saved))?;
        session.app.restore(&snapshot)?;
        let tick = session.app.tick();
        let recorded = session
            .recording
            .as_ref()
            .map(|trail| trail.hashes.len() as u64);
        match recorded {
            Some(len) if tick <= len => {
                if let Some(trail) = session.recording.as_mut() {
                    trail.rewind(tick);
                }
            }
            Some(_) => {
                // A snapshot later than what is recorded now (ticks in
                // between were undone): the recording cannot continue.
                session.recording = None;
                out.insert(
                    "recording_stopped".to_owned(),
                    json!("The snapshot is later than the recording (the ticks before it were undone since), so recording stopped. Record again from tick 0."),
                );
            }
            None => {}
        }
        // Same state, same reports: what the inspector already said after
        // the snapshot is forgotten with the rest.
        session.reported.clear();
        id
    };
    let session = workspace.session(&target)?;
    out.insert("session".to_owned(), json!(target));
    out.insert("snapshot".to_owned(), json!(label));
    out.insert("tick".to_owned(), json!(session.app.tick()));
    out.insert(
        "world_hash".to_owned(),
        json!(session.app.hash().to_string()),
    );
    Ok(out)
}

fn sim_stop(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let id = workspace.session_id(args.str("session"))?;
    let session = workspace.sessions.open.remove(&id);
    let tick = session.as_ref().map(|s| s.app.tick());
    let stopped = Diagnostic::info(
        "session_stopped",
        format!("Session {id} was closed."),
        "Start another with sim_start.",
    );
    workspace
        .sessions
        .record(Some(&id), tick, "session", &stopped);
    let mut out = Map::new();
    out.insert("session".to_owned(), json!(id));
    out.insert("stopped_at_tick".to_owned(), json!(tick));
    out.insert(
        "open_sessions".to_owned(),
        json!(workspace.sessions.open.keys().collect::<Vec<_>>()),
    );
    Ok(out)
}

fn rank(severity: Severity) -> u8 {
    match severity {
        Severity::Info => 0,
        Severity::Warning => 1,
        Severity::Error => 2,
    }
}

fn logs_get(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let session = args.str("session").map(str::to_owned);
    let since = u64::try_from(args.int("since").unwrap_or(0)).unwrap_or(0);
    let least = match args.str("severity") {
        Some("error") => 2,
        Some("warning") => 1,
        _ => 0,
    };
    let kinds = args.strings("kinds");
    const KNOWN: &[&str] = &["session", "system", "inspector", "script", "audio"];
    for kind in &kinds {
        if !KNOWN.contains(&kind.as_str()) {
            return Err(Diagnostic::error(
                "invalid_parameter",
                format!("'{kind}' is not a log kind."),
                wish_core::suggest::unknown_name_hint(kind, KNOWN),
            )
            .at_path("/kinds")
            .with_candidates(KNOWN.iter().copied()));
        }
    }
    let limit = usize::try_from(args.int("limit").unwrap_or(100)).unwrap_or(100);
    let matching: Vec<&crate::sessions::LogEntry> = workspace
        .sessions
        .log
        .iter()
        .filter(|e| e.seq > since)
        .filter(|e| {
            session
                .as_deref()
                .is_none_or(|s| e.session.as_deref() == Some(s))
        })
        .filter(|e| rank(e.severity) >= least)
        .filter(|e| kinds.is_empty() || kinds.iter().any(|k| k == e.kind))
        .collect();
    let skipped = matching.len().saturating_sub(limit);
    let shown: Vec<Value> = matching.iter().skip(skipped).map(|e| e.to_json()).collect();
    let mut out = Map::new();
    out.insert("entries".to_owned(), json!(shown));
    if skipped > 0 {
        out.insert("older_not_shown".to_owned(), json!(skipped));
    }
    out.insert(
        "last_seq".to_owned(),
        json!(workspace.sessions.log.back().map_or(0, |e| e.seq)),
    );
    Ok(out)
}

/// What the player does during a `sim_step`: `{ "move": [0, 1], "jump": true }`
/// against the session's action names.
pub(crate) fn parse_input(
    workspace: &Workspace,
    session: &str,
    given: Option<&Value>,
) -> Result<wish_input::Values> {
    let mut values = wish_input::Values::new();
    let given = match given {
        None | Some(Value::Null) => return Ok(values),
        Some(Value::Object(given)) => given,
        Some(other) => {
            return Err(Diagnostic::error(
                "invalid_parameter",
                format!("'input' must be an object of actions and values, not {other}."),
                "Write it like {\"move\": [0, 1], \"jump\": true}: each action of input.json with its value (true for a button, a number for an axis, [x, y] for a 2D axis).",
            )
            .at_path("/input"));
        }
    };
    let map = &workspace.session(session)?.input;
    let names: Vec<&str> = map.names().collect();
    for (name, value) in given {
        let path = format!("/input/{name}");
        if map.binding(name).is_none() {
            let hint = if names.is_empty() {
                "The project has no actions yet: create input.json (wen new writes one) with actions such as \"jump\" and \"move\", then start a new session.".to_owned()
            } else {
                wish_core::suggest::unknown_name_hint(name, &names)
            };
            return Err(Diagnostic::error(
                "unknown_action",
                format!("The project has no action '{name}'."),
                hint,
            )
            .at_path(path)
            .with_candidates(names));
        }
        let number = |v: &Value| v.as_f64().filter(|n| n.is_finite() && n.abs() <= 1.0);
        let parsed = match value {
            Value::Bool(down) => Some([f32::from(u8::from(*down)), 0.0]),
            Value::Number(_) => number(value).map(|n| [n as f32, 0.0]),
            Value::Array(items) if items.len() == 2 => number(&items[0])
                .zip(number(&items[1]))
                .map(|(x, y)| [x as f32, y as f32]),
            _ => None,
        };
        let Some(parsed) = parsed else {
            return Err(Diagnostic::error(
                "invalid_parameter",
                format!("The value for action '{name}' is not understood: {value}."),
                "Use true or false for a button, a number from -1 to 1 for an axis, or [x, y] (each from -1 to 1) for a two-axis action.",
            )
            .at_path(path));
        };
        values.insert(name.clone(), parsed);
    }
    Ok(values)
}
