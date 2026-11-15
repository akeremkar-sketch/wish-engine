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
            "Only these kinds: session, system, inspector.",
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
    let (stepped, tick, hash, entities) = workspace.step_session(&id, frames, until.as_ref())?;
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
    const KNOWN: &[&str] = &["session", "system", "inspector"];
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
