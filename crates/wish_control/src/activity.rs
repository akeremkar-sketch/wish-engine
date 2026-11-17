//! The activity log (M8b WP2): every call that changes something (by
//! anyone) and every call an agent makes (also the ones that only look),
//! in order, for the editor's activity feed and live observation of
//! agents.
//!
//! It lives in the tool layer, not in the simulation: it keeps no clock
//! (entries are numbered), and it changes nothing a tool answers except
//! `activity_get` itself.

use std::collections::VecDeque;

use serde_json::{Map, Value, json};
use wish_scene::Actor;

/// How many entries are kept (the oldest go first).
pub const MAX_ACTIVITY: usize = 2000;

/// The log.
#[derive(Debug, Default)]
pub(crate) struct Activity {
    entries: VecDeque<Value>,
    /// The number of the last entry (0 before any).
    last: u64,
}

/// Up to `max` characters of `text`, with "…" when cut.
fn short(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max).collect();
    cut.push('…');
    cut
}

/// What an `edit_apply` did, in a few words: `spawn crate, set crate.Transform`.
fn ops_summary(ops: &Value) -> String {
    let all = ops.as_array().map(Vec::as_slice).unwrap_or_default();
    let mut parts: Vec<String> = all
        .iter()
        .take(6)
        .map(|op| {
            let kind = op["op"].as_str().unwrap_or("?");
            let target = op["id"]
                .as_str()
                .or_else(|| op["name"].as_str())
                .unwrap_or("");
            match op["component"].as_str() {
                Some(component) => format!("{kind} {target}.{component}"),
                None => format!("{kind} {target}"),
            }
        })
        .collect();
    if all.len() > 6 {
        parts.push(format!("and {} more", all.len() - 6));
    }
    parts.join(", ")
}

/// A call in a few words, from its arguments.
fn summary(tool: &str, args: &Value) -> String {
    match tool {
        "edit_apply" => ops_summary(&args["ops"]),
        "edit_history" => {
            let action = args["action"].as_str().unwrap_or("list");
            match args["revision"].as_u64() {
                Some(revision) => format!("{action} r{revision}"),
                None => action.to_owned(),
            }
        }
        _ => {
            // The arguments themselves, the big ones left out.
            let small: Map<String, Value> = args
                .as_object()
                .map(|all| {
                    all.iter()
                        .filter(|(_, value)| value.to_string().len() <= 200)
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect()
                })
                .unwrap_or_default();
            if small.is_empty() {
                String::new()
            } else {
                short(&Value::Object(small).to_string(), 160)
            }
        }
    }
}

impl Activity {
    /// Notes a call and its answer.
    pub(crate) fn record(&mut self, tool: &str, args: &Value, actor: &Actor, answer: &Value) {
        self.last += 1;
        let mut entry = json!({
            "seq": self.last,
            "actor": actor.label(),
            "tool": tool,
            "ok": answer["ok"] == true,
            "summary": summary(tool, args),
        });
        for key in ["scene", "session"] {
            if let Some(value) = args[key].as_str().or_else(|| answer[key].as_str()) {
                entry[key] = json!(value);
            }
        }
        if let Some(reason) = args["reason"].as_str() {
            entry["reason"] = json!(short(reason, 500));
        }
        if answer["ok"] == true {
            // An edit's revision: its card can be taken back by it.
            if tool == "edit_apply"
                && args["dry_run"] != true
                && args["session"].is_null()
                && let Some(revision) = answer["revision"].as_u64()
            {
                entry["revision"] = json!(revision);
            }
            if let Some(out) = answer["out"].as_str() {
                entry["out"] = json!(out);
            }
            for key in ["passed", "stopped_by", "ticks_run", "diverged_at"] {
                if !answer[key].is_null() {
                    entry[key] = answer[key].clone();
                }
            }
        } else {
            entry["error"] = json!({
                "code": answer["error"]["code"],
                "message": short(answer["error"]["message"].as_str().unwrap_or(""), 300),
            });
        }
        self.entries.push_back(entry);
        while self.entries.len() > MAX_ACTIVITY {
            self.entries.pop_front();
        }
    }

    /// The entries after `since`, oldest first, at most `limit`; only
    /// `actor`'s if given (a label such as `human` or `agent:helper`, or
    /// `agents` for every agent).
    pub(crate) fn since(&self, since: u64, limit: usize, actor: Option<&str>) -> Vec<Value> {
        self.entries
            .iter()
            .filter(|entry| entry["seq"].as_u64().unwrap_or(0) > since)
            .filter(|entry| match actor {
                None => true,
                Some("agents") => entry["actor"]
                    .as_str()
                    .is_some_and(|label| label.starts_with("agent")),
                Some(wanted) => entry["actor"] == wanted,
            })
            .take(limit)
            .cloned()
            .collect()
    }

    /// The number of the last entry.
    pub(crate) fn last(&self) -> u64 {
        self.last
    }
}

/// Whether a call goes into the log: everything that changes something,
/// and everything an agent does. A person's looking (the editor reads the
/// project many times a second) is not activity.
pub(crate) fn worth_noting(tool: &str, args: &Value, mutates: bool, actor: &Actor) -> bool {
    // A tool that changes things can also just look (listing checkpoints).
    let looks = tool == "checkpoint" && args["action"] == "list";
    tool != "activity_get" && ((mutates && !looks) || !matches!(actor, Actor::Human))
}
