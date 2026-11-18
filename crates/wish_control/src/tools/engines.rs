//! `engines` (M10b; design note M10 §15): the engine versions on this
//! computer, side by side, and which one opens a project.

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

use crate::engines;
use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const ENGINES: Tool = Tool {
    name: "engines",
    title: "Engine versions on this computer",
    description: "Engine versions are installed side by side. list shows them (the running one, the installed ones and folders the person added) and which one opens this project: the exact version in its wish.json if there is one, else the newest compatible one. pick says that for another project folder. add (a person only) adds an engine folder where it is (a folder with wen and wish-engine.json); remove (a person only) forgets an added one or moves an installed one to the trash.",
    params: &[
        Param::optional(
            "action",
            Kind::Choice(&["list", "pick", "add", "remove"]),
            "What to do (default list).",
        )
        .or("\"list\""),
        Param::optional(
            "path",
            Kind::String,
            "For add and remove: the engine's folder; for pick: a project's folder (default this project). Absolute, or relative to the project.",
        ),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "Check the call and say what it would do, without doing it.",
        ),
    ],
    mutates: true,
    run,
};

fn diagnostics(found: &[Diagnostic]) -> Value {
    json!(found.iter().map(|d| json!(**d)).collect::<Vec<_>>())
}

fn person_only(actor: &Actor) -> Result<()> {
    if !matches!(actor, Actor::Human) {
        return Err(Diagnostic::error(
            "permission_denied",
            "Only a person can add or remove engines: they are the person's own, for every project.",
            "Ask the person to do it (the Hub's Engine versions tab, or `wen engines`).",
        ));
    }
    Ok(())
}

fn path_arg(workspace: &Workspace, args: &Args, action: &str) -> Result<std::path::PathBuf> {
    let Some(path) = args.str("path").map(str::trim).filter(|p| !p.is_empty()) else {
        return Err(Diagnostic::error(
            "missing_parameter",
            format!("{action} needs \"path\": the engine's folder."),
            "Write it like \"path\": \"C:/Wish/0.2.0\" (the folder with wen and wish-engine.json).",
        )
        .at_path("/path"));
    };
    let given = std::path::Path::new(path);
    Ok(if given.is_absolute() {
        given.to_path_buf()
    } else {
        workspace.root().join(given)
    })
}

fn config() -> Result<std::path::PathBuf> {
    crate::crash::config_folder().ok_or_else(|| {
        Diagnostic::error(
            "file_unwritable",
            "There is no settings folder to keep the list of engines in.",
            "Set WISH_CONFIG_DIR to a folder the engine may write to.",
        )
    })
}

/// What the actions answer; `wen engines` (with no project) and the tool
/// share it.
pub fn act(
    action: &str,
    path: Option<&std::path::Path>,
    project: Option<&std::path::Path>,
    dry_run: bool,
) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    match action {
        "add" | "remove" => {
            let config = config()?;
            let Some(folder) = path else {
                return Err(Diagnostic::error(
                    "missing_parameter",
                    format!("{action} needs \"path\": the engine's folder."),
                    "Write it like \"path\": \"C:/Wish/0.2.0\" (the folder with wen and wish-engine.json).",
                )
                .at_path("/path"));
            };
            if action == "add" {
                let (engine, new) = engines::add(&config, folder, dry_run)?;
                out.insert("engine".to_owned(), engine.to_value());
                out.insert("added".to_owned(), json!(new && !dry_run));
                if !new {
                    out.insert("known".to_owned(), json!(true));
                }
            } else {
                let removed = engines::remove(&config, folder, dry_run)?;
                out.insert("engine".to_owned(), removed.engine.to_value());
                out.insert("removed".to_owned(), json!(!dry_run));
                out.insert(
                    "trash".to_owned(),
                    json!(removed.trash.map(|t| t.display().to_string())),
                );
            }
            if dry_run {
                out.insert("dry_run".to_owned(), json!(true));
            }
        }
        "pick" => {
            let Some(root) = path.or(project) else {
                return Err(Diagnostic::error(
                    "missing_parameter",
                    "pick needs \"path\": a project's folder.",
                    "Write the folder that has wish.json.",
                )
                .at_path("/path"));
            };
            if !root.join(crate::PROJECT_FILE).is_file() {
                return Err(Diagnostic::error(
                    "project_not_found",
                    "That folder has no wish.json.",
                    "Choose a project's folder (the one with wish.json).",
                )
                .at_path("/path"));
            }
            let config = crate::crash::config_folder();
            out = engines::choose(root, config.as_deref()).to_value();
        }
        _ => {
            let config = crate::crash::config_folder();
            let (all, warnings) = engines::engines(config.as_deref());
            out.insert(
                "running".to_owned(),
                json!(engines::running().version.to_string()),
            );
            out.insert(
                "engines".to_owned(),
                json!(
                    all.iter()
                        .map(engines::Engine::to_value)
                        .collect::<Vec<_>>()
                ),
            );
            if let Some(project) = project {
                out.insert(
                    "project".to_owned(),
                    Value::Object(engines::choose(project, config.as_deref()).to_value()),
                );
            }
            out.insert("diagnostics".to_owned(), diagnostics(&warnings));
        }
    }
    Ok(out)
}

fn run(workspace: &mut Workspace, args: &Args, actor: &Actor) -> Result<Map<String, Value>> {
    let action = args.str("action").unwrap_or("list");
    let dry_run = args.flag("dry_run");
    let root = workspace.root().to_path_buf();
    match action {
        "add" | "remove" => {
            person_only(actor)?;
            let folder = path_arg(workspace, args, action)?;
            act(action, Some(&folder), Some(&root), dry_run)
        }
        "pick" => {
            let given = args
                .str("path")
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(|_| path_arg(workspace, args, action))
                .transpose()?;
            act(action, given.as_deref(), Some(&root), dry_run)
        }
        _ => act(action, None, Some(&root), dry_run),
    }
}
