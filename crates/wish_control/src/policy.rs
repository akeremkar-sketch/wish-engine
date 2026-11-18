//! Permission modes (M8b WP4): how much agents may change by themselves.
//!
//! - `suggest` ("only suggest"): an agent's call that would change the
//!   project is not run; it becomes a proposal (with a dry run of what it
//!   would change, where the tool has one), and a person accepts it (it
//!   then runs, as the agent's) or rejects it.
//! - `apply` ("apply, I review"): calls run; the activity feed shows them.
//! - `autonomous`: the same for the control plane; the editor's cockpit
//!   does not ask between an agent's tasks either.
//!
//! People are never held: the policy is about agents. Only a person can
//! change it. Running the game, tests and looking are never held: they do
//! not change the project.

use serde_json::{Value, json};
use wish_scene::Actor;

/// The permission mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Agents' changes wait for a person.
    Suggest,
    /// Agents' changes are made; a person reviews them in the feed.
    #[default]
    Apply,
    /// As `Apply`; the cockpit does not stop between tasks.
    Autonomous,
}

impl Mode {
    /// Its name in tools and files.
    pub fn name(self) -> &'static str {
        match self {
            Self::Suggest => "suggest",
            Self::Apply => "apply",
            Self::Autonomous => "autonomous",
        }
    }

    /// The mode named `name`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "suggest" => Some(Self::Suggest),
            "apply" => Some(Self::Apply),
            "autonomous" => Some(Self::Autonomous),
            _ => None,
        }
    }
}

/// How many proposals are kept (decided ones go first).
pub const MAX_PROPOSALS: usize = 200;

/// The policy and the proposals waiting for a person.
#[derive(Debug, Default)]
pub(crate) struct Policy {
    pub(crate) mode: Mode,
    pub(crate) proposals: Vec<Value>,
    last: u64,
}

impl Policy {
    /// Keeps a proposal; returns its number.
    pub(crate) fn propose(
        &mut self,
        actor: &Actor,
        tool: &str,
        args: &Value,
        preview: Option<Value>,
    ) -> u64 {
        self.last += 1;
        let mut proposal = json!({
            "id": self.last,
            "actor": actor.label(),
            "tool": tool,
            "args": args,
            "status": "waiting",
        });
        if let Some(reason) = args["reason"].as_str() {
            proposal["reason"] = json!(reason);
        }
        if let Some(preview) = preview {
            proposal["preview"] = preview;
        }
        self.proposals.push(proposal);
        while self.proposals.len() > MAX_PROPOSALS {
            match self.proposals.iter().position(|p| p["status"] != "waiting") {
                Some(decided) => {
                    self.proposals.remove(decided);
                }
                None => {
                    self.proposals.remove(0);
                }
            }
        }
        self.last
    }
}

/// Whether a call would change the project (files, history), so that in
/// `suggest` mode an agent's call becomes a proposal. Running the game,
/// tests, builds and looking are not.
pub(crate) fn changes_project(tool: &str, args: &Value) -> bool {
    if args["dry_run"] == true {
        return false;
    }
    let action = args["action"].as_str().unwrap_or("");
    match tool {
        "edit_apply" => args["session"].is_null(),
        "edit_history" => matches!(action, "undo" | "redo" | "revert"),
        "asset_move" | "asset_create" => true,
        "project_settings" => action == "set",
        "checkpoint" => matches!(action, "restore" | "delete"),
        "replay" => action == "save",
        "capture_frame" => !args["out"].is_null(),
        "test_run" => args["update_golden"] == true,
        "packages" => matches!(action, "install" | "remove" | "lock" | "add" | "update"),
        // Refused outright for agents (a person decides), never held.
        "permissions" | "proposals" | "engines" => false,
        _ => false,
    }
}
