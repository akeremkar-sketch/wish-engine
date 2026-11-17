//! `checkpoint` (M8b WP3): save the project as it is now, list, go back,
//! delete. The editor's cockpit saves one before every agent task.

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const CHECKPOINT: Tool = Tool {
    name: "checkpoint",
    title: "Checkpoints",
    description: "A checkpoint keeps the project's scenes, prefabs and settings (wish.json, input.json) as they are now, to go back to later (\"before the agent started\") and to compare with (capture_frame with checkpoint draws a scene as it was). save (name optional: cp_1, cp_2...; note: a sentence for people), list, restore (each scene and prefab is brought back by one change in its own history, so going back can itself be undone or taken back on its own; scenes made after the checkpoint are kept; settings files are written back), delete. Stored in the project's .wish/checkpoints.",
    params: &[
        Param::required(
            "action",
            Kind::Choice(&["save", "list", "restore", "delete"]),
            "save, list, restore or delete.",
        ),
        Param::optional(
            "name",
            Kind::String,
            "The checkpoint's name, 1 to 40 of a-z 0-9 _ - (save: optional; restore and delete: required).",
        ),
        Param::optional(
            "note",
            Kind::String,
            "For save: what the moment was, such as \"before the agent's level\".",
        ),
        Param::optional(
            "overwrite",
            Kind::Boolean,
            "For save: replace a checkpoint of that name.",
        ),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "For restore: say what would come back, without changing anything.",
        ),
    ],
    mutates: true,
    run: checkpoint,
};

fn required_name<'a>(args: &'a Args, action: &str) -> Result<&'a str> {
    args.str("name").ok_or_else(|| {
        Diagnostic::error(
            "missing_parameter",
            format!("{action} needs \"name\": which checkpoint."),
            "checkpoint with action \"list\" shows the names.",
        )
        .at_path("/name")
    })
}

fn checkpoint(workspace: &mut Workspace, args: &Args, actor: &Actor) -> Result<Map<String, Value>> {
    let action = args.str("action").unwrap_or("list");
    let mut out = Map::new();
    out.insert("action".to_owned(), json!(action));
    match action {
        "save" => {
            let note = args
                .str("note")
                .map(|n| n.chars().take(300).collect::<String>());
            let manifest = workspace.save_checkpoint(
                args.str("name"),
                note.as_deref(),
                actor,
                args.flag("overwrite"),
            )?;
            out.insert("checkpoint".to_owned(), manifest);
        }
        "restore" => {
            let name = required_name(args, "restore")?;
            let dry_run = args.flag("dry_run");
            let result = workspace.restore_checkpoint(name, actor, dry_run)?;
            if let Value::Object(fields) = result {
                out.extend(fields);
            }
            if dry_run {
                out.insert("dry_run".to_owned(), json!(true));
            }
        }
        "delete" => {
            let name = required_name(args, "delete")?;
            workspace.delete_checkpoint(name)?;
            out.insert("deleted".to_owned(), json!(name));
        }
        _ => {
            out.insert("checkpoints".to_owned(), json!(workspace.checkpoints()));
        }
    }
    Ok(out)
}
