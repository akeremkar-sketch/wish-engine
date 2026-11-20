//! `permissions` and `proposals` (M8b WP4): how much agents may change by
//! themselves, and the changes waiting for a person.

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::policy::Mode;
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const PERMISSIONS: Tool = Tool {
    name: "permissions",
    title: "Permission mode",
    description: "How much agents may change by themselves. get returns the mode; set (a person only) changes it: suggest (an agent's change to the project waits as a proposal until a person accepts it; running the game, tests and looking still work), apply (changes are made and shown in the activity feed; the default), autonomous (the same, and the editor's cockpit does not ask between tasks).",
    params: &[
        Param::optional(
            "action",
            Kind::Choice(&["get", "set"]),
            "get the mode, or set it (a person only).",
        )
        .or("\"get\""),
        Param::optional(
            "mode",
            Kind::Choice(&["suggest", "apply", "autonomous"]),
            "For set: the new mode.",
        ),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "For set: check the call and say the mode it would set, without setting it.",
        ),
    ],
    mutates: true,
    run: permissions,
};

pub(crate) const PROPOSALS: Tool = Tool {
    name: "proposals",
    title: "Changes waiting for a person",
    description: "In the suggest permission mode an agent's change to the project is kept as a proposal instead of being made: its tool, arguments, reason and (where the tool has a dry run) what it would change. list shows them (an agent can see whether its proposals were accepted); accept (a person only) makes the change now, as the agent's, and returns the tool's answer; reject (a person only) drops it.",
    params: &[
        Param::optional(
            "action",
            Kind::Choice(&["list", "accept", "reject"]),
            "list, accept or reject.",
        )
        .or("\"list\""),
        Param::optional(
            "id",
            Kind::Integer {
                min: 1,
                max: i64::MAX,
            },
            "For accept and reject: the proposal's id.",
        ),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "For accept: what the change would do now (its tool's dry run), deciding nothing; for reject: check only.",
        ),
    ],
    mutates: true,
    run: proposals,
};

fn person_only(actor: &Actor, what: &str) -> Result<()> {
    if matches!(actor, Actor::Human) {
        return Ok(());
    }
    Err(Diagnostic::error(
        "permission_denied",
        format!("Only a person can {what}."),
        "Ask the person working with you; they decide in the editor (Project menu > Permissions, the Activity tab).",
    ))
}

fn permissions(
    workspace: &mut Workspace,
    args: &Args,
    actor: &Actor,
) -> Result<Map<String, Value>> {
    if args.str("action") == Some("set") {
        person_only(actor, "change the permission mode")?;
        let Some(mode) = args.str("mode").and_then(Mode::parse) else {
            return Err(Diagnostic::error(
                "missing_parameter",
                "set needs \"mode\": suggest, apply or autonomous.",
                "Write it like \"mode\": \"suggest\".",
            )
            .at_path("/mode"));
        };
        if args.flag("dry_run") {
            let mut out = Map::new();
            out.insert("mode".to_owned(), json!(workspace.policy.mode.name()));
            out.insert("would_set".to_owned(), json!(mode.name()));
            out.insert("dry_run".to_owned(), json!(true));
            return Ok(out);
        }
        workspace.policy.mode = mode;
    }
    let mut out = Map::new();
    out.insert("mode".to_owned(), json!(workspace.policy.mode.name()));
    let waiting = workspace
        .policy
        .proposals
        .iter()
        .filter(|p| p["status"] == "waiting")
        .count();
    out.insert("waiting".to_owned(), json!(waiting));
    Ok(out)
}

fn proposals(workspace: &mut Workspace, args: &Args, actor: &Actor) -> Result<Map<String, Value>> {
    let action = args.str("action").unwrap_or("list");
    let mut out = Map::new();
    if action == "list" {
        out.insert("mode".to_owned(), json!(workspace.policy.mode.name()));
        out.insert("proposals".to_owned(), json!(workspace.policy.proposals));
        return Ok(out);
    }
    person_only(actor, "accept or reject a proposal")?;
    let Some(id) = args.int("id").and_then(|id| u64::try_from(id).ok()) else {
        return Err(Diagnostic::error(
            "missing_parameter",
            format!("{action} needs \"id\": which proposal."),
            "proposals with action list shows them and their ids.",
        )
        .at_path("/id"));
    };
    let Some(index) = workspace
        .policy
        .proposals
        .iter()
        .position(|p| p["id"].as_u64() == Some(id))
    else {
        return Err(Diagnostic::error(
            "proposal_not_found",
            format!("There is no proposal {id}."),
            "proposals with action list shows them and their ids.",
        )
        .at_path("/id"));
    };
    if workspace.policy.proposals[index]["status"] != "waiting" {
        return Err(Diagnostic::error(
            "proposal_decided",
            format!(
                "Proposal {id} was {} already.",
                workspace.policy.proposals[index]["status"]
                    .as_str()
                    .unwrap_or("decided")
            ),
            "Only waiting proposals can be accepted or rejected.",
        ));
    }
    if args.flag("dry_run") {
        let proposal = workspace.policy.proposals[index].clone();
        let tool = proposal["tool"].as_str().unwrap_or("").to_owned();
        let agent = crate::workspace::parse_actor(proposal["actor"].as_str().unwrap_or("agent"));
        out.insert("proposal".to_owned(), json!(id));
        out.insert("dry_run".to_owned(), json!(true));
        if action == "accept" {
            out.insert(
                "would".to_owned(),
                json!(workspace.dry_run_of(&tool, &proposal["args"], &agent)),
            );
        }
        return Ok(out);
    }
    if action == "reject" {
        workspace.policy.proposals[index]["status"] = json!("rejected");
        out.insert(
            "proposal".to_owned(),
            workspace.policy.proposals[index].clone(),
        );
        return Ok(out);
    }
    // Accepted: the change is made now, as the agent's.
    let proposal = workspace.policy.proposals[index].clone();
    let tool = proposal["tool"].as_str().unwrap_or("").to_owned();
    let agent = crate::workspace::parse_actor(proposal["actor"].as_str().unwrap_or("agent"));
    let answer = workspace.call_unheld(&tool, &proposal["args"], &agent);
    let accepted = answer["ok"] == true;
    if let Some(stored) = workspace.policy.proposals.get_mut(index) {
        stored["status"] = json!(if accepted { "accepted" } else { "failed" });
        stored["answer"] = answer.clone();
    }
    out.insert("proposal".to_owned(), json!(id));
    out.insert("answer".to_owned(), answer);
    Ok(out)
}
