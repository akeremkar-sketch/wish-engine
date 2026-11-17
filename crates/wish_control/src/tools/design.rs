//! `game_design` (M9c): what the game is and what was decided about it,
//! kept in `design/game.json`: the game card (brief), the decisions that
//! hold for every change, the plan, notes from playing it and the questions
//! an agent asked the person. Agents read it before every task (the AI
//! mode gives it to them) and keep it up to date; a person edits it in the
//! AI mode's panels. One file, canonical JSON, never overwritten when it is
//! broken.

use std::path::Path;

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

/// The design file, project-relative.
pub const DESIGN_FILE: &str = "design/game.json";

/// The game card's fields, in the order people read them.
pub const BRIEF_FIELDS: &[&str] = &[
    "title", "pitch", "genre", "goal", "loop", "controls", "look", "audience", "must", "later",
];

/// The fields that are lists (of short lines).
const LIST_FIELDS: &[&str] = &["must", "later"];

/// Longest text kept in one field or line.
const MAX_TEXT: usize = 2000;
/// Most items in one list (decisions, plan, notes, questions).
const MAX_ITEMS: usize = 500;

pub(crate) const GAME_DESIGN: Tool = Tool {
    name: "game_design",
    title: "The game's design: card, decisions, plan, notes, questions",
    description: "What the game is and what was decided about it, in design/game.json. get returns all of it. set_brief sets fields of the game card (title, pitch: one sentence; genre; goal: what the player wants; loop: what the player does every few seconds; controls; look: art style and mood; audience; must and later: lists of features that must be in and that wait). decide records a decision that holds for every change (\"jump height 2.5 m\", \"pastel colours only\"), forget removes one. plan_add, plan_set (status todo, doing or done) and plan_remove keep the plan of small steps. note records a note from playing (with the scene and tick it is about), note_done closes one. ask puts a question to the person with choices (use it when a choice is theirs: difficulty, look, feel; then stop and wait), answer (a person) answers it, and with remember also records the answer as a decision. Read it before you change the game, and keep it up to date.",
    params: &[
        Param::optional(
            "action",
            Kind::Choice(&[
                "get",
                "set_brief",
                "decide",
                "forget",
                "plan_add",
                "plan_set",
                "plan_remove",
                "note",
                "note_done",
                "ask",
                "answer",
            ]),
            "What to do (default get).",
        )
        .or("\"get\""),
        Param::optional(
            "fields",
            Kind::Object,
            "For set_brief: the card's fields to set, such as { \"genre\": \"platformer\", \"must\": [\"double jump\"] }; others stay.",
        ),
        Param::optional(
            "text",
            Kind::String,
            "For decide, plan_add, note and ask: the decision, the step, the note or the question.",
        ),
        Param::optional(
            "id",
            Kind::Integer {
                min: 1,
                max: i64::MAX,
            },
            "For forget, plan_set, plan_remove, note_done and answer: which one.",
        ),
        Param::optional(
            "status",
            Kind::Choice(&["todo", "doing", "done"]),
            "For plan_set: the step's new status.",
        ),
        Param::optional(
            "options",
            Kind::StringList,
            "For ask: the choices offered (two to six short answers).",
        ),
        Param::optional("answer", Kind::String, "For answer: the person's answer."),
        Param::optional(
            "remember",
            Kind::Boolean,
            "For answer: also keep the answer as a decision.",
        ),
        Param::optional("scene", Kind::String, "For note: the scene it is about."),
        Param::optional(
            "tick",
            Kind::Integer {
                min: 0,
                max: i64::MAX,
            },
            "For note: the game's tick it is about (while playing).",
        ),
        Param::optional(
            "picture",
            Kind::String,
            "For note: a picture of that moment, a project file (such as one capture_frame saved).",
        ),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "Check the call and say what it would change, without changing the file.",
        ),
    ],
    mutates: true,
    run: game_design,
};

fn invalid(message: String, hint: &str) -> Diagnostic {
    Diagnostic::error("invalid_design", message, hint)
}

fn empty_design() -> Value {
    let brief: Map<String, Value> = BRIEF_FIELDS
        .iter()
        .map(|field| {
            let empty = if LIST_FIELDS.contains(field) {
                json!([])
            } else {
                json!("")
            };
            ((*field).to_owned(), empty)
        })
        .collect();
    json!({
        "format": 1,
        "brief": brief,
        "decisions": [],
        "plan": [],
        "notes": [],
        "questions": [],
        "next_id": 1
    })
}

/// The design file as it is (an empty design when there is none).
pub fn read_design(root: &Path) -> Result<Value> {
    let path = root.join(DESIGN_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(empty_design());
    };
    let value: Value = serde_json::from_str(&text).map_err(|error| {
        Diagnostic::error(
            "design_file_invalid",
            format!("{DESIGN_FILE} is not valid JSON: {error}."),
            "Fix the file (or delete it to start the design again); it is not overwritten while it is broken.",
        )
        .at_file(DESIGN_FILE)
    })?;
    if value["format"] != 1 || !value.is_object() {
        return Err(Diagnostic::error(
            "design_file_invalid",
            format!("{DESIGN_FILE} is not a design file of format 1."),
            "It needs \"format\": 1; fix it or delete it to start again.",
        )
        .at_file(DESIGN_FILE));
    }
    // Missing parts are added, so older files keep working.
    let mut merged = empty_design();
    for (key, part) in value.as_object().into_iter().flatten() {
        if key == "brief" {
            for (field, text) in part.as_object().into_iter().flatten() {
                merged["brief"][field] = text.clone();
            }
        } else {
            merged[key] = part.clone();
        }
    }
    Ok(merged)
}

/// A short summary of the design for an agent's instructions: the card,
/// the decisions, the open steps of the plan, the open notes and the
/// answered questions. `None` when nothing is written yet.
pub fn summary(design: &Value) -> Option<String> {
    let mut out = String::new();
    let brief = &design["brief"];
    for field in BRIEF_FIELDS {
        let line = match &brief[*field] {
            Value::String(text) if !text.trim().is_empty() => text.trim().to_owned(),
            Value::Array(items) if !items.is_empty() => items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("; "),
            _ => continue,
        };
        out.push_str(&format!("- {field}: {line}\n"));
    }
    if !out.is_empty() {
        out.insert_str(0, "The game card:\n");
    }
    let lines = |key: &str, keep: &dyn Fn(&Value) -> bool| -> Vec<String> {
        design[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| keep(item))
            .filter_map(|item| {
                Some(format!(
                    "- [{}] {}",
                    item["id"].as_u64()?,
                    item["text"].as_str()?
                ))
            })
            .collect()
    };
    let decisions = lines("decisions", &|_| true);
    if !decisions.is_empty() {
        out.push_str("Decisions (hold them in every change):\n");
        out.push_str(&(decisions.join("\n") + "\n"));
    }
    let plan = lines("plan", &|item| item["status"] != "done");
    if !plan.is_empty() {
        out.push_str("Open steps of the plan:\n");
        out.push_str(&(plan.join("\n") + "\n"));
    }
    let notes = lines("notes", &|item| item["status"] == "open");
    if !notes.is_empty() {
        out.push_str("Notes from playing the game:\n");
        out.push_str(&(notes.join("\n") + "\n"));
    }
    let answered: Vec<String> = design["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|q| {
            Some(format!(
                "- {} → {}",
                q["text"].as_str()?,
                q["answer"].as_str()?
            ))
        })
        .collect();
    if !answered.is_empty() {
        out.push_str("The person answered:\n");
        out.push_str(&(answered.join("\n") + "\n"));
    }
    (!out.is_empty()).then_some(out)
}

fn short(text: &str, path: &str) -> Result<String> {
    let text = text.trim();
    if text.chars().count() > MAX_TEXT {
        return Err(invalid(
            format!("The text is longer than {MAX_TEXT} characters."),
            "Keep it short: one decision, one step, one note.",
        )
        .at_path(path));
    }
    Ok(text.to_owned())
}

fn needs(args: &Args, name: &str, action: &str) -> Result<String> {
    match args.str(name) {
        Some(text) if !text.trim().is_empty() => short(text, &format!("/{name}")),
        _ => Err(Diagnostic::error(
            "missing_parameter",
            format!("{action} needs \"{name}\"."),
            format!("Write it like \"{name}\": \"...\"."),
        )
        .at_path(format!("/{name}"))),
    }
}

fn needs_id(args: &Args, action: &str) -> Result<u64> {
    args.int("id")
        .and_then(|id| u64::try_from(id).ok())
        .ok_or_else(|| {
            Diagnostic::error(
                "missing_parameter",
                format!("{action} needs \"id\"."),
                "game_design with action get lists everything with its id.",
            )
            .at_path("/id")
        })
}

/// The item of `list` with `id` (its index), or a not-found error.
fn find(design: &Value, list: &str, id: u64) -> Result<usize> {
    design[list]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .position(|item| item["id"].as_u64() == Some(id))
        })
        .ok_or_else(|| {
            Diagnostic::error(
                "design_item_not_found",
                format!("There is no {} with id {id}.", list.trim_end_matches('s')),
                "game_design with action get lists everything with its id.",
            )
            .at_path("/id")
        })
}

fn push(design: &mut Value, list: &str, mut item: Value) -> Result<u64> {
    let count = design[list].as_array().map_or(0, Vec::len);
    if count >= MAX_ITEMS {
        return Err(invalid(
            format!("The design already has {MAX_ITEMS} {list}."),
            "Remove the ones that no longer matter first.",
        ));
    }
    let id = design["next_id"].as_u64().unwrap_or(1);
    design["next_id"] = json!(id + 1);
    item["id"] = json!(id);
    if let Some(items) = design[list].as_array_mut() {
        items.push(item);
    }
    Ok(id)
}

fn who(actor: &Actor) -> String {
    actor.label()
}

fn game_design(
    workspace: &mut Workspace,
    args: &Args,
    actor: &Actor,
) -> Result<Map<String, Value>> {
    let root = workspace.root().to_path_buf();
    let mut design = read_design(&root)?;
    let action = args.str("action").unwrap_or("get");
    let mut out = Map::new();
    if action == "get" {
        out.insert("file".to_owned(), json!(DESIGN_FILE));
        out.insert("design".to_owned(), design);
        return Ok(out);
    }
    match action {
        "set_brief" => {
            let Some(fields) = args.value("fields").and_then(Value::as_object) else {
                return Err(Diagnostic::error(
                    "missing_parameter",
                    "set_brief needs \"fields\": the card's fields to set.",
                    "Write it like \"fields\": { \"pitch\": \"...\", \"must\": [\"...\"] }.",
                )
                .at_path("/fields"));
            };
            for (field, value) in fields {
                if !BRIEF_FIELDS.contains(&field.as_str()) {
                    return Err(invalid(
                        format!("The game card has no field '{field}'."),
                        &wish_core::suggest::unknown_name_hint(field, BRIEF_FIELDS),
                    )
                    .at_path(format!("/fields/{field}")));
                }
                let path = format!("/fields/{field}");
                let kept = if LIST_FIELDS.contains(&field.as_str()) {
                    let Some(items) = value.as_array() else {
                        return Err(invalid(
                            format!("'{field}' is a list of short lines."),
                            "Write it like [\"double jump\", \"three levels\"].",
                        )
                        .at_path(&path));
                    };
                    let mut lines = Vec::new();
                    for item in items {
                        let Some(text) = item.as_str() else {
                            return Err(invalid(
                                format!("'{field}' holds lines of text."),
                                "Write it like [\"double jump\", \"three levels\"].",
                            )
                            .at_path(&path));
                        };
                        let text = short(text, &path)?;
                        if !text.is_empty() {
                            lines.push(json!(text));
                        }
                    }
                    Value::Array(lines)
                } else {
                    let Some(text) = value.as_str() else {
                        return Err(invalid(
                            format!("'{field}' is text."),
                            "Write it like \"pitch\": \"Hop between floating islands\".",
                        )
                        .at_path(&path));
                    };
                    json!(short(text, &path)?)
                };
                design["brief"][field] = kept;
            }
        }
        "decide" => {
            let text = needs(args, "text", action)?;
            let id = push(
                &mut design,
                "decisions",
                json!({ "text": text, "by": who(actor) }),
            )?;
            out.insert("id".to_owned(), json!(id));
        }
        "forget" => {
            let id = needs_id(args, action)?;
            let at = find(&design, "decisions", id)?;
            if let Some(items) = design["decisions"].as_array_mut() {
                items.remove(at);
            }
        }
        "plan_add" => {
            let text = needs(args, "text", action)?;
            let id = push(
                &mut design,
                "plan",
                json!({ "text": text, "status": "todo", "by": who(actor) }),
            )?;
            out.insert("id".to_owned(), json!(id));
        }
        "plan_set" => {
            let id = needs_id(args, action)?;
            let Some(status) = args.str("status") else {
                return Err(Diagnostic::error(
                    "missing_parameter",
                    "plan_set needs \"status\": todo, doing or done.",
                    "Write it like \"status\": \"done\".",
                )
                .at_path("/status"));
            };
            let at = find(&design, "plan", id)?;
            design["plan"][at]["status"] = json!(status);
        }
        "plan_remove" => {
            let id = needs_id(args, action)?;
            let at = find(&design, "plan", id)?;
            if let Some(items) = design["plan"].as_array_mut() {
                items.remove(at);
            }
        }
        "note" => {
            let text = needs(args, "text", action)?;
            let mut note = json!({ "text": text, "status": "open", "by": who(actor) });
            if let Some(scene) = args.str("scene") {
                note["scene"] = json!(scene);
            }
            if let Some(tick) = args.int("tick") {
                note["tick"] = json!(tick);
            }
            if let Some(picture) = args.str("picture") {
                let path = crate::documents::normalize_output_path(picture)
                    .map_err(|error| error.at_path("/picture"))?;
                let full = crate::documents::inside_project(&root, &path)?;
                if !full.is_file() {
                    return Err(Diagnostic::error(
                        "file_not_found",
                        format!("The picture '{path}' does not exist."),
                        "Save it first (capture_frame with \"out\"), then pass its path.",
                    )
                    .at_path("/picture"));
                }
                note["picture"] = json!(path);
            }
            let id = push(&mut design, "notes", note)?;
            out.insert("id".to_owned(), json!(id));
        }
        "note_done" => {
            let id = needs_id(args, action)?;
            let at = find(&design, "notes", id)?;
            design["notes"][at]["status"] = json!("done");
        }
        "ask" => {
            let text = needs(args, "text", action)?;
            let options: Vec<String> = args
                .strings("options")
                .into_iter()
                .map(|option| option.trim().to_owned())
                .filter(|option| !option.is_empty())
                .collect();
            if !(2..=6).contains(&options.len()) {
                return Err(invalid(
                    "A question offers two to six choices.".to_owned(),
                    "Write it like \"options\": [\"3\", \"5\", \"10\"]; the person can still answer in their own words.",
                )
                .at_path("/options"));
            }
            for option in &options {
                short(option, "/options")?;
            }
            let id = push(
                &mut design,
                "questions",
                json!({ "text": text, "options": options, "answer": null, "by": who(actor) }),
            )?;
            out.insert("id".to_owned(), json!(id));
            out.insert(
                "message".to_owned(),
                json!("Asked. Stop here: the person answers in the editor, and the answer comes with the next request."),
            );
        }
        "answer" => {
            let id = needs_id(args, action)?;
            let answer = needs(args, "answer", action)?;
            let at = find(&design, "questions", id)?;
            design["questions"][at]["answer"] = json!(answer);
            if args.flag("remember") {
                let question = design["questions"][at]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let decided = push(
                    &mut design,
                    "decisions",
                    json!({ "text": format!("{question} → {answer}"), "by": who(actor) }),
                )?;
                out.insert("decision".to_owned(), json!(decided));
            }
        }
        _ => {}
    }
    out.insert("file".to_owned(), json!(DESIGN_FILE));
    if args.flag("dry_run") {
        out.insert("would_be".to_owned(), design);
        out.insert("dry_run".to_owned(), json!(true));
        return Ok(out);
    }
    let text = wish_scene::json::to_canonical(&design)?;
    crate::documents::write_project_file(&root, DESIGN_FILE, &text)?;
    out.insert("design".to_owned(), design);
    Ok(out)
}
