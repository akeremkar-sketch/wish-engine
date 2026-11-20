//! `activity_get` (M8b WP2): the activity log, as the editor's activity
//! feed shows it.

use serde_json::{Map, Value, json};
use wish_core::Result;
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const ACTIVITY_GET: Tool = Tool {
    name: "activity_get",
    title: "What everyone did",
    description: "The activity log: every call that changed something (by anyone: a person in the editor, an agent, a text editor through the file) and every call an agent made, oldest first, numbered (seq). Each entry: actor, tool, a short summary, ok, reason (if given to edit_apply), revision (an edit: take it back with edit_history revert), scene/session, out (a picture written), passed/stopped_by for tests and steps, or the error. Pass since (the last seq you saw) to get only what is new; actor to filter (\"human\", \"agent:name\", or \"agents\").",
    params: &[
        Param::optional(
            "since",
            Kind::Integer {
                min: 0,
                max: i64::MAX,
            },
            "Only entries after this seq (0: from the start).",
        )
        .or("0"),
        Param::optional(
            "limit",
            Kind::Integer { min: 1, max: 500 },
            "At most this many entries.",
        )
        .or("100"),
        Param::optional(
            "actor",
            Kind::String,
            "Only this actor's entries: \"human\", an agent's label such as \"agent:helper\", or \"agents\" for all agents.",
        ),
    ],
    mutates: false,
    run: activity_get,
};

fn activity_get(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let since = args
        .int("since")
        .and_then(|n| u64::try_from(n).ok())
        .unwrap_or(0);
    let limit = args
        .int("limit")
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(100);
    let entries = workspace.activity.since(since, limit, args.str("actor"));
    let mut out = Map::new();
    out.insert("last_seq".to_owned(), json!(workspace.activity.last()));
    out.insert("entries".to_owned(), Value::Array(entries));
    Ok(out)
}
