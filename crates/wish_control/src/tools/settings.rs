//! `project_settings` (M8a WP8): the project's settings, read and changed
//! through the control plane: the name and default scene (`wish.json`) and
//! the input map (`input.json`). The editor's settings window is a face of
//! this tool, like every other panel (invariant 4).

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const PROJECT_SETTINGS: Tool = Tool {
    name: "project_settings",
    title: "Project settings",
    description: "The project's settings: its name, default scene and online play (wish.json `net`: the replicated components and the lobby) and its input map (input.json: which keys drive which actions, as the game code reads them with actions.pressed / axis2). action get (the default) returns them; action set changes what is given and writes the files canonically (name, scene, net as the whole net section, and/or input as the whole input map). The input map is checked first, with a hint for every mistake (unknown key names, bad action names). Running sessions keep their input map; new ones use the new one.",
    params: &[
        Param::optional(
            "action",
            Kind::Choice(&["get", "set"]),
            "get the settings, or set some of them.",
        )
        .or("\"get\""),
        Param::optional("name", Kind::String, "The project's name, for people."),
        Param::optional(
            "scene",
            Kind::String,
            "The default scene, such as \"scenes/main.scene.json\" (it must exist).",
        ),
        Param::optional(
            "net",
            Kind::Object,
            "The whole online play section of wish.json, like { \"replicate\": [\"Transform\", \"NetOwner\", \"Avatar\"], \"lobby\": { \"min_players\": 4, \"max_players\": 10, \"roles\": [{ \"name\": \"crew\" }, { \"name\": \"saboteur\", \"count\": 1, \"more_from\": [7] }] } }. Checked first; new matches use it.",
        ),
        Param::optional(
            "input",
            Kind::Object,
            "The whole input map, like { \"format\": 1, \"actions\": { \"jump\": { \"kind\": \"button\", \"keys\": [\"Space\"] } } }.",
        ),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "Check and report what would change, without writing.",
        ),
    ],
    mutates: true,
    run: project_settings,
};

/// The input map as JSON: the file's content, or an empty map.
fn input_json(workspace: &Workspace) -> Result<Value> {
    let path = workspace.root().join(wish_input::INPUT_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| {
            Diagnostic::error(
                "invalid_input_file",
                format!("{} is not JSON: {error}.", wish_input::INPUT_FILE),
                "Fix the file by hand, or set the whole map with project_settings action set.",
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(json!({ "format": 1, "actions": {} }))
        }
        Err(error) => Err(Diagnostic::error(
            "file_unreadable",
            format!("Could not read '{}': {error}.", wish_input::INPUT_FILE),
            "Check that the file can be read.",
        )),
    }
}

fn project_settings(
    workspace: &mut Workspace,
    args: &Args,
    _: &Actor,
) -> Result<Map<String, Value>> {
    let set = args.str("action") == Some("set");
    let mut changed = Vec::new();
    if set {
        let dry_run = args.flag("dry_run");
        if let Some(name) = args.str("name") {
            let trimmed = name.trim();
            if trimmed.is_empty() || trimmed.chars().count() > 80 {
                return Err(Diagnostic::error(
                    "invalid_project_name",
                    format!("'{name}' is not a project name."),
                    "Give 1 to 80 characters, such as \"Island Hopper\".",
                )
                .at_path("/name"));
            }
        }
        let scene = match args.str("scene") {
            Some(scene) => {
                // Spelled exactly as on disk (one file, one document).
                let path = workspace
                    .scene_path(Some(scene))
                    .map_err(|problem| problem.at_path("/scene"))?;
                if !path.ends_with(".scene.json") || !workspace.root().join(&path).is_file() {
                    return Err(Diagnostic::error(
                        "scene_not_found",
                        format!("There is no scene '{path}'."),
                        "Name a scene file of the project; project_info lists them.",
                    )
                    .at_path("/scene"));
                }
                Some(path)
            }
            None => None,
        };
        let input_text = match args.value("input") {
            Some(input) => {
                let text = wish_scene::json::to_canonical(input)?;
                // Checked as the engine will read it, and written in the
                // input map's own canonical form (as `wen new` writes it).
                let map = wish_input::InputMap::parse(&text, wish_input::INPUT_FILE)
                    .map_err(|problem| problem.at_path("/input"))?;
                Some(map.to_canonical()?)
            }
            None => None,
        };
        let net = match args.value("net") {
            Some(net) => {
                crate::project::NetSettings::from_value(net).map_err(
                    |(pointer, message, hint)| {
                        Diagnostic::error("invalid_parameter", message, hint)
                            .at_path(format!("/net{pointer}"))
                    },
                )?;
                Some(net.clone())
            }
            None => None,
        };
        let config_change = args.str("name").is_some() || scene.is_some() || net.is_some();
        if config_change && workspace.config().is_none() {
            if let Some(problem) = workspace.config_problem() {
                return Err(problem.clone());
            }
            return Err(Diagnostic::error(
                "no_project_file",
                format!(
                    "The project has no {} to keep a name and a default scene in.",
                    crate::project::PROJECT_FILE
                ),
                "Make the project with `wen new`, which writes it.",
            ));
        }
        // Both files or neither: the input map first, put back if the
        // project file cannot be written.
        let input_before =
            std::fs::read_to_string(workspace.root().join(wish_input::INPUT_FILE)).ok();
        if let Some(text) = &input_text {
            if !dry_run {
                crate::documents::write_project_file(
                    workspace.root(),
                    wish_input::INPUT_FILE,
                    text,
                )?;
            }
            changed.push("input");
        }
        if config_change {
            if !dry_run
                && let Err(problem) =
                    workspace.write_config(args.str("name").map(str::trim), scene.as_deref(), net)
            {
                if input_text.is_some() {
                    let root = workspace.root().to_path_buf();
                    match &input_before {
                        Some(before) => {
                            let _ = crate::documents::write_project_file(
                                &root,
                                wish_input::INPUT_FILE,
                                before,
                            );
                        }
                        None => {
                            let _ = std::fs::remove_file(root.join(wish_input::INPUT_FILE));
                        }
                    }
                }
                return Err(problem);
            }
            if args.str("name").is_some() {
                changed.push("name");
            }
            if scene.is_some() {
                changed.push("scene");
            }
            if args.value("net").is_some() {
                changed.push("net");
            }
        }
        if dry_run {
            let mut out = Map::new();
            out.insert("dry_run".to_owned(), json!(true));
            out.insert("would_change".to_owned(), json!(changed));
            return Ok(out);
        }
    }
    let mut out = Map::new();
    let config = workspace.config();
    out.insert(
        "project".to_owned(),
        json!({
            "name": config.map(|c| c.name.clone()),
            "scene": config.map(|c| c.scene.clone()),
        }),
    );
    out.insert(
        "net".to_owned(),
        config.and_then(|c| c.net.clone()).unwrap_or(Value::Null),
    );
    // A broken input.json does not hide the rest: its problem is a note.
    match input_json(workspace) {
        Ok(input) => {
            out.insert("input".to_owned(), input);
        }
        Err(problem) => {
            out.insert("input".to_owned(), Value::Null);
            out.insert("input_problem".to_owned(), json!(problem));
        }
    }
    if set {
        out.insert("changed".to_owned(), json!(changed));
    }
    Ok(out)
}
