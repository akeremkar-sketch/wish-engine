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
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: PathBuf::from(context.root.as_deref().unwrap_or(".")),
        example: context.example.clone(),
    })?;
    if context.software {
        workspace.set_software(true);
    }
    Ok(workspace)
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

fn context_of(root: &Option<String>, example: &str, software: bool) -> ToolContext {
    ToolContext {
        root: root.clone(),
        example: Some(example.to_owned()),
        actor: None,
        software,
    }
}

/// Runs `wen capture`: the `capture_frame` tool, and the picture written to
/// the file `--out` names (anywhere the person likes; the tool itself only
/// writes inside the project).
pub fn capture(args: &crate::args::CaptureArgs, as_json: bool) -> Outcome {
    let context = context_of(&args.root, &args.example, args.software);
    let (width, height) = args.size;
    let mut tool_args = serde_json::json!({
        "scene": args.scene,
        "width": width,
        "height": height,
        "view": args.view,
    });
    if let Some(camera) = &args.camera {
        tool_args["camera"] = Value::String(camera.clone());
    }
    if let Some(shaders) = &args.shaders {
        tool_args["shaders"] = Value::String(shaders.clone());
    }
    let mut value = match open(&context) {
        Ok(mut workspace) => workspace.call("capture_frame", &tool_args, &parse_actor("human")),
        Err(problem) => envelope(Err(problem)),
    };
    if value.get("ok") == Some(&Value::Bool(true)) {
        let written = value["image"]["data"]
            .as_str()
            .and_then(wish_control::base64_decode)
            .ok_or_else(|| {
                Diagnostic::error(
                    "internal_error",
                    "The picture did not come back as base64.",
                    "This is a bug in wen. Please report it.",
                )
            })
            .and_then(|png| {
                std::fs::write(&args.out, png).map_err(|error| {
                    Diagnostic::error(
                        "file_unwritable",
                        format!("Could not write '{}': {error}.", args.out),
                        "Check that the folder exists and can be written to.",
                    )
                })
            });
        match written {
            Ok(()) => {
                if let Some(image) = value["image"].as_object_mut() {
                    image.remove("data");
                    image.insert("path".to_owned(), Value::String(args.out.clone()));
                }
            }
            Err(problem) => value = envelope(Err(problem)),
        }
    }
    let success = value.get("ok") == Some(&Value::Bool(true));
    let text = if as_json {
        value.to_string()
    } else if success {
        let mut text = format!(
            "Wrote {} ({width}x{height}, {} view) from camera '{}': {} draw(s), on {} ({}).",
            args.out,
            value["view"].as_str().unwrap_or_default(),
            value["camera"].as_str().unwrap_or_default(),
            value["drawn"],
            value["adapter"]["name"].as_str().unwrap_or_default(),
            value["adapter"]["backend"].as_str().unwrap_or_default(),
        );
        for entry in value["diagnostics"].as_array().into_iter().flatten() {
            if let Ok(problem) = serde_json::from_value::<Diagnostic>(entry.clone()) {
                text.push_str(&format!("\n{problem}"));
            }
        }
        text
    } else {
        match serde_json::from_value::<Diagnostic>(value["error"].clone()) {
            Ok(problem) => problem.to_string(),
            Err(_) => value.to_string(),
        }
    };
    Outcome { text, success }
}

/// Runs `wen describe`: the `describe_view` tool.
pub fn describe(args: &crate::args::DescribeArgs, as_json: bool) -> Outcome {
    let context = context_of(&args.root, &args.example, args.software);
    let (width, height) = args.size;
    let mut tool_args =
        serde_json::json!({ "scene": args.scene, "width": width, "height": height });
    if let Some(camera) = &args.camera {
        tool_args["camera"] = Value::String(camera.clone());
    }
    if let Some(shaders) = &args.shaders {
        tool_args["shaders"] = Value::String(shaders.clone());
    }
    shortcut(
        "describe_view",
        &tool_args,
        &context,
        as_json,
        describe_text,
    )
}

fn describe_text(answer: &Value) -> String {
    let hidden: Vec<&str> = answer["hidden"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut text = format!(
        "From camera '{}' ({}x{}): {} visible, {} hidden.",
        answer["camera"].as_str().unwrap_or_default(),
        answer["width"],
        answer["height"],
        answer["visible_count"],
        hidden.len()
    );
    for entry in answer["visible"].as_array().into_iter().flatten() {
        let shown = entry["visible_percent"]
            .as_f64()
            .map_or_else(|| "not measured".to_owned(), |p| format!("{p}% visible"));
        let [left, top, right, bottom] = ["0", "1", "2", "3"]
            .map(|i| entry["bounding_box"][i.parse::<usize>().unwrap_or(0)].to_string());
        text.push_str(&format!(
            "\n  {}: {} px, {shown}, box ({left},{top})-({right},{bottom}), {:.2} m away",
            entry["entity"].as_str().unwrap_or_default(),
            entry["pixels"],
            entry["distance"].as_f64().unwrap_or(0.0)
        ));
    }
    if !hidden.is_empty() {
        text.push_str(&format!(
            "\n  not visible (hidden or outside the picture): {}",
            hidden.join(", ")
        ));
    }
    for entry in answer["diagnostics"].as_array().into_iter().flatten() {
        if let Ok(problem) = serde_json::from_value::<Diagnostic>(entry.clone()) {
            text.push_str(&format!("\n{problem}"));
        }
    }
    text
}

/// Runs `wen mcp`: the MCP server on standard input and output, until the
/// client closes the input. Problems before the server starts go to
/// standard error (standard output belongs to the protocol).
pub fn serve_mcp(context: &ToolContext) -> std::process::ExitCode {
    let workspace = match open(context) {
        Ok(workspace) => workspace,
        Err(problem) => {
            eprintln!("{problem}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let mut server = wish_mcp::Server::new(workspace);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    match wish_mcp::serve(&mut server, std::io::BufReader::new(stdin), stdout.lock()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("The MCP connection failed: {error}.");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Text for a `script_build` answer.
pub fn build_text(answer: &Value) -> String {
    let mut text = String::new();
    for entry in answer["diagnostics"].as_array().into_iter().flatten() {
        if let Ok(problem) = serde_json::from_value::<Diagnostic>(entry.clone()) {
            text.push_str(&format!("{problem}\n"));
        }
    }
    let seconds = answer["seconds"].as_f64().unwrap_or(0.0);
    if answer["passed"] == true {
        let systems = answer["systems"]
            .as_array()
            .map(|s| {
                s.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let verb = if answer["dry_run"] == true {
            "Compiles"
        } else {
            "Built"
        };
        text.push_str(&format!("{verb} in {seconds:.2} s; systems: {systems}."));
    } else {
        text.push_str(&format!(
            "Build failed: {} error(s), {} warning(s); the previous build stays in use.",
            answer["errors"], answer["warnings"]
        ));
    }
    text
}

/// `wen sdk`: builds the game SDK pack from the engine's own sources.
pub fn pack_sdk(out: Option<&str>, as_json: bool) -> wish_core::Result<String> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_default();
    let out = out.map_or_else(|| beside.join("sdk"), std::path::PathBuf::from);
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    if !manifest.is_file() {
        return Err(Diagnostic::error(
            "script_sdk_missing",
            "`wen sdk` builds the SDK from the engine's sources, which are not on this machine.",
            "Packaged engines ship the SDK in their 'sdk' folder; this command is for engine development.",
        ));
    }
    let cargo = std::env::var_os("CARGO").map_or_else(|| "cargo".into(), std::path::PathBuf::from);
    let target = beside.join("sdk-target");
    let description = wish_script::build::pack_sdk(&cargo, &manifest, &target, &out)?;
    Ok(if as_json {
        serde_json::json!({ "out": out.display().to_string(), "sdk": description }).to_string()
    } else {
        format!(
            "SDK pack written to {} (for {}).",
            out.display(),
            description["rustc"].as_str().unwrap_or("?")
        )
    })
}
