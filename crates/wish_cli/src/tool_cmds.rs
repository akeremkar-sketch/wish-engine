//! `wen call` and `wen do`: the control plane's tools on the command line.
//!
//! The answer is exactly what the MCP server returns as structured content
//! (same JSON, keys sorted), so a call can be tried here and repeated over
//! MCP, and the parity test can compare the two.

use std::io::Read;
use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::{Workspace, WorkspaceOptions, envelope, find_tool, from_flags, parse_actor};
use wish_core::{Diagnostic, Result};

use crate::args::{CallArgs, DoArgs, ToolContext};
use crate::project_cmds::Outcome;

fn open(context: &ToolContext) -> Result<Workspace> {
    Workspace::open(WorkspaceOptions {
        root: PathBuf::from(context.root.as_deref().unwrap_or(".")),
        example: context.example.clone(),
    })
}

fn answer(value: &Value) -> Outcome {
    Outcome {
        text: value.to_string(),
        success: value.get("ok") == Some(&Value::Bool(true)),
    }
}

fn arguments(args: &CallArgs) -> Result<Value> {
    let tool = find_tool(&args.tool)?;
    match (&args.json, args.flags.is_empty()) {
        (Some(_), false) => Err(Diagnostic::error(
            "invalid_arguments",
            "Give the arguments either as one JSON object or as --name value flags, not both.",
            format!(
                "Write it like: wen call {} '{{\"name\": ...}}', or wen call {} --name ...",
                tool.name, tool.name
            ),
        )),
        (Some(text), true) => serde_json::from_str(text).map_err(|error| {
            Diagnostic::error(
                "invalid_arguments",
                format!("The arguments are not valid JSON: {error}."),
                "Quote the JSON object for your shell, for example: wen call schema_get '{\"names\": [\"Transform\"]}'.",
            )
        }),
        (None, _) => from_flags(tool.name, tool.params, &args.flags),
    }
}

/// Runs `wen call`.
pub fn call(args: &CallArgs) -> Outcome {
    let outcome = open(&args.context).and_then(|mut workspace| {
        let value = arguments(args)?;
        let actor = parse_actor(args.context.actor.as_deref().unwrap_or("human"));
        Ok(workspace.call(&args.tool, &value, &actor))
    });
    match outcome {
        Ok(value) => answer(&value),
        Err(problem) => answer(&envelope(Err(problem))),
    }
}

/// One call of a `wen do` list, or why the entry is not a call.
fn read_call(entry: &Value, index: usize) -> Result<(String, Value)> {
    let problem = |message: String| {
        Diagnostic::error(
            "invalid_call",
            message,
            "Write each call as { \"tool\": \"<name>\", \"args\": { ... } }; args may be left out.",
        )
        .at_op(u32::try_from(index).unwrap_or(u32::MAX))
    };
    let Some(object) = entry.as_object() else {
        return Err(problem(format!("Call {index} is not a JSON object.")));
    };
    for key in object.keys() {
        if key != "tool" && key != "args" {
            return Err(problem(format!("Call {index} has an unknown key '{key}'.")));
        }
    }
    let Some(tool) = object.get("tool").and_then(Value::as_str) else {
        return Err(problem(format!("Call {index} has no \"tool\" name.")));
    };
    Ok((
        tool.to_owned(),
        object.get("args").cloned().unwrap_or(Value::Null),
    ))
}

/// Reads the calls: a JSON array, or one JSON object per line.
fn read_calls(text: &str) -> Result<Vec<Value>> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('[') {
        return match serde_json::from_str::<Value>(text) {
            Ok(Value::Array(items)) => Ok(items),
            Ok(_) | Err(_) => Err(Diagnostic::error(
                "invalid_arguments",
                "The list of calls is not a valid JSON array.",
                "Write [ { \"tool\": \"project_info\" }, ... ], or one call object per line.",
            )),
        };
    }
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(number, line)| {
            serde_json::from_str(line).map_err(|error| {
                Diagnostic::error(
                    "invalid_arguments",
                    format!("Line {} is not valid JSON: {error}.", number + 1),
                    "Write one call object per line, such as { \"tool\": \"project_info\" }.",
                )
                .at_line(u32::try_from(number + 1).unwrap_or(u32::MAX), None)
            })
        })
        .collect()
}

/// Runs `wen do`: every call in order, in one process, one answer per line.
pub fn run_calls(args: &DoArgs) -> Outcome {
    let text = match &args.source {
        Some(path) => std::fs::read_to_string(path).map_err(|error| {
            Diagnostic::error(
                "file_unreadable",
                format!("Could not read '{path}': {error}."),
                "Check the path of the file of calls.",
            )
        }),
        None => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map(|_| text)
                .map_err(|error| {
                    Diagnostic::error(
                        "file_unreadable",
                        format!("Could not read the calls from standard input: {error}."),
                        "Pipe the calls in, or give a file: wen do calls.json.",
                    )
                })
        }
    };
    let prepared = text
        .and_then(|text| read_calls(&text))
        .and_then(|calls| Ok((open(&args.context)?, calls)));
    let (mut workspace, calls) = match prepared {
        Ok(prepared) => prepared,
        Err(problem) => return answer(&envelope(Err(problem))),
    };
    let actor = parse_actor(args.context.actor.as_deref().unwrap_or("human"));
    let mut lines = Vec::with_capacity(calls.len());
    let mut success = true;
    for (index, entry) in calls.iter().enumerate() {
        let value = match read_call(entry, index) {
            Ok((tool, call_args)) => workspace.call(&tool, &call_args, &actor),
            Err(problem) => json!({ "ok": false, "error": problem }),
        };
        success &= value.get("ok") == Some(&Value::Bool(true));
        lines.push(value.to_string());
    }
    Outcome {
        text: lines.join("\n"),
        success,
    }
}

/// Runs `wen new`: writes a project folder (the scaffold lives in the
/// control plane; this only chooses how MCP clients will start `wen`).
pub fn new_project(dir: &str, name: Option<&str>, as_json: bool) -> Result<String> {
    let path = std::path::Path::new(dir);
    let name = name.map_or_else(|| wish_control::scaffold::name_for(path), str::to_owned);
    // The full path of this program, so that `.mcp.json` works even when
    // `wen` is not on the PATH of the program that starts it.
    let command = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.to_str().map(str::to_owned))
        .unwrap_or_else(|| "wen".to_owned());
    let written = wish_control::scaffold::create_project(path, &name, &command)?;
    Ok(if as_json {
        json!({ "ok": true, "root": dir, "name": name, "files": written, "mcp_command": command })
            .to_string()
    } else {
        format!(
            "Created project '{name}' in {dir}:\n  {}\nOpen it with Claude Code from that folder (the MCP server `wish` is configured in .mcp.json), or try: wen call project_info --root {dir}",
            written.join("\n  ")
        )
    })
}

/// Runs a control-plane tool for a shortcut command (`wen check`, `wen mv`):
/// the JSON answer is the tool's own (what MCP returns); without `--json`
/// `human` turns a successful answer into text and a failure prints as the
/// diagnostic it is.
pub fn shortcut(
    tool: &str,
    args: &Value,
    context: &ToolContext,
    as_json: bool,
    human: fn(&Value) -> String,
) -> Outcome {
    let value = match open(context) {
        Ok(mut workspace) => workspace.call(
            tool,
            args,
            &parse_actor(context.actor.as_deref().unwrap_or("human")),
        ),
        Err(problem) => envelope(Err(problem)),
    };
    let success = value.get("ok") == Some(&Value::Bool(true))
        && value.get("passed") != Some(&Value::Bool(false));
    let text = if as_json {
        value.to_string()
    } else if value.get("ok") == Some(&Value::Bool(true)) {
        human(&value)
    } else {
        match serde_json::from_value::<Diagnostic>(value["error"].clone()) {
            Ok(problem) => problem.to_string(),
            Err(_) => value.to_string(),
        }
    };
    Outcome { text, success }
}

/// Text for a `check` answer.
pub fn check_text(answer: &Value) -> String {
    let mut text = String::new();
    for entry in answer["diagnostics"].as_array().into_iter().flatten() {
        if let Ok(problem) = serde_json::from_value::<Diagnostic>(entry.clone()) {
            text.push_str(&format!("{problem}\n"));
        }
    }
    text.push_str(&format!(
        "{} file(s) checked: {} error(s), {} warning(s).",
        answer["files"], answer["errors"], answer["warnings"]
    ));
    text
}

/// Text for an `asset_move` answer.
pub fn move_text(answer: &Value) -> String {
    let verb = if answer["dry_run"] == true {
        "Would move"
    } else {
        "Moved"
    };
    let mut text = format!(
        "{verb} {} -> {}",
        answer["from"].as_str().unwrap_or_default(),
        answer["to"].as_str().unwrap_or_default()
    );
    if let Some(meta) = answer["meta"].as_object() {
        text.push_str(&format!(
            "\n  and {} -> {}",
            meta["from"].as_str().unwrap_or_default(),
            meta["to"].as_str().unwrap_or_default()
        ));
    }
    for edit in answer["documents"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "\n  updated {} reference(s) in {}",
            edit["references"],
            edit["file"].as_str().unwrap_or_default()
        ));
    }
    text
}
