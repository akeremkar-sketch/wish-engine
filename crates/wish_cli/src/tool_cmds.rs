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

/// Reads the calls: a JSON array, or JSON objects one after another (one
/// per line, or spread over lines as written by hand).
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
    serde_json::Deserializer::from_str(text)
        .into_iter::<Value>()
        .map(|item| {
            item.map_err(|error| {
                Diagnostic::error(
                    "invalid_arguments",
                    format!("Line {} is not valid JSON: {error}.", error.line()),
                    "Write call objects one after another, such as { \"tool\": \"project_info\" }, or a JSON array of them.",
                )
                .at_line(u32::try_from(error.line()).unwrap_or(u32::MAX), None)
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
pub fn new_project(
    dir: &str,
    name: Option<&str>,
    template: Option<&str>,
    as_json: bool,
) -> Result<String> {
    let path = std::path::Path::new(dir);
    let name = name.map_or_else(|| wish_control::scaffold::name_for(path), str::to_owned);
    // The full path of this program, so that `.mcp.json` works even when
    // `wen` is not on the PATH of the program that starts it.
    let command = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.to_str().map(str::to_owned))
        .unwrap_or_else(|| "wen".to_owned());
    let written = match template {
        None => wish_control::scaffold::create_project(path, &name, &command)?,
        Some(template) => {
            let known = wish_control::scaffold::all_templates();
            let Some(found) = known.iter().find(|known| known.id == template) else {
                let ids: Vec<&str> = known.iter().map(|known| known.id.as_str()).collect();
                return Err(wish_core::Diagnostic::error(
                    "invalid_template",
                    format!("The engine has no template '{template}'."),
                    if ids.is_empty() {
                        "No templates were found: set WISH_TEMPLATES to a folder of project folders.".to_owned()
                    } else {
                        format!("Use one of: {}.", ids.join(", "))
                    },
                ));
            };
            wish_control::scaffold::create_from_template(path, &name, &command, &found.path)?
        }
    };
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
    human: &dyn Fn(&Value) -> String,
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
        http: None,
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
        &describe_text,
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
    if let Some(address) = &context.http {
        return serve_mcp_http(&mut server, address);
    }
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

/// `wen mcp --http`: MCP over Streamable HTTP until the process is stopped.
/// Clients always send a token (ADR-022): `WISH_MCP_TOKEN`, or one made up
/// now and shown. Beyond this machine only with `WISH_MCP_TOKEN` set.
/// `WISH_MCP_ORIGINS` (comma-separated) allows more browser origins than
/// this machine's own.
fn serve_mcp_http(server: &mut wish_mcp::Server, address: &str) -> std::process::ExitCode {
    let chosen = std::env::var("WISH_MCP_TOKEN")
        .ok()
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty());
    let mut options = wish_mcp::http::HttpOptions {
        token: chosen.clone(),
        allowed_origins: std::env::var("WISH_MCP_ORIGINS")
            .map(|all| {
                all.split(',')
                    .map(|origin| origin.trim().to_owned())
                    .filter(|origin| !origin.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        request_time: None,
    };
    let bound = wish_mcp::http::listen_address(address, &options).and_then(|at| {
        std::net::TcpListener::bind(at).map_err(|error| {
            wish_core::Diagnostic::error(
                "invalid_http_address",
                format!("Could not listen on {at}: {error}."),
                "Pick another port (or 0 for a free one), or stop what uses this one.",
            )
        })
    });
    let listener = match bound {
        Ok(listener) => listener,
        Err(problem) => {
            eprintln!("{problem}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let token = chosen.unwrap_or_else(wish_mcp::http::random_token);
    options.token = Some(token.clone());
    if let Ok(at) = listener.local_addr() {
        eprintln!(
            "Wish Engine MCP on http://{at}{} (Streamable HTTP, protocol {}). Stop with Ctrl+C.",
            wish_mcp::http::ENDPOINT,
            wish_mcp::PROTOCOL_VERSION
        );
        eprintln!(
            "Clients send the header 'Authorization: Bearer {token}' (set WISH_MCP_TOKEN to choose it)."
        );
    }
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    match wish_mcp::http::serve_http(server, listener, options, stop) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("The MCP server failed: {error}.");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Text for a `project_export` answer.
pub fn export_text(answer: &Value) -> String {
    format!(
        "Exported to {} ({} files, {} bytes). Run {} to play; zip the folder to share it.",
        answer["folder"].as_str().unwrap_or("?"),
        answer["files"],
        answer["bytes"],
        answer["program"].as_str().unwrap_or("?"),
    )
}

/// Text for an `engines` answer (`wen engines`).
pub fn engines_text(answer: &Value) -> String {
    let mut text = String::new();
    let dry = answer["dry_run"] == true;
    let row = |engine: &Value| {
        format!(
            "{}  {}  ({})",
            engine["version"].as_str().unwrap_or("?"),
            engine["folder"].as_str().unwrap_or("?"),
            engine["source"].as_str().unwrap_or("?"),
        )
    };
    for entry in answer["diagnostics"].as_array().into_iter().flatten() {
        if let Ok(problem) = serde_json::from_value::<Diagnostic>(entry.clone()) {
            text.push_str(&format!("{problem}\n"));
        }
    }
    if let Some(all) = answer["engines"].as_array() {
        for engine in all {
            text.push_str(&format!("  {}\n", row(engine)));
        }
    }
    let choice = if answer["opens_with"].is_object() {
        Some(answer)
    } else {
        answer.get("project").filter(|p| p.is_object())
    };
    if let Some(choice) = choice {
        text.push_str(&format!(
            "The project was made with {}; it opens with {} ({}).\n",
            choice["project_engine"]
                .as_str()
                .unwrap_or("an unknown version"),
            row(&choice["opens_with"]),
            choice["fit"].as_str().unwrap_or("?"),
        ));
    }
    if answer["engine"].is_object() {
        let verb = if answer.get("trash").is_some() {
            if dry { "Would remove" } else { "Removed" }
        } else if answer["known"] == true {
            "Already known:"
        } else if dry {
            "Would add"
        } else {
            "Added"
        };
        text.push_str(&format!("{verb} {}\n", row(&answer["engine"])));
        if let Some(trash) = answer["trash"].as_str() {
            text.push_str(&format!("  (moved to {trash})\n"));
        }
    }
    text
}

/// Text for a `packages` answer (`wen pkg`).
pub fn pkg_text(answer: &Value) -> String {
    let mut text = String::new();
    let line = |package: &Value| {
        format!(
            "  {} {}  {}",
            package["id"].as_str().unwrap_or("?"),
            package["version"]
                .as_str()
                .or(package["latest"].as_str())
                .unwrap_or("-"),
            package["name"].as_str().unwrap_or(""),
        )
    };
    let dry = answer["dry_run"] == true;
    if let Some(found) = answer["packages"].as_array() {
        if found.is_empty() {
            text.push_str("No package in the index matches.\n");
        }
        for package in found {
            let mut row = line(package);
            let installed = &package["installed"];
            for scope in ["project", "user"] {
                if let Some(version) = installed[scope].as_str() {
                    row.push_str(&format!("  [{scope} {version}]"));
                }
            }
            for scope in ["project", "user"] {
                if package["update"][scope] == true {
                    row.push_str(&format!("  ({scope} update waiting)"));
                }
            }
            text.push_str(&row);
            text.push('\n');
        }
    } else if let Some(updated) = answer["updated"].as_array() {
        for change in updated {
            text.push_str(&format!(
                "{} {} {} -> {}\n",
                if dry { "Would update" } else { "Updated" },
                change["id"].as_str().unwrap_or("?"),
                change["from"].as_str().unwrap_or("?"),
                change["to"].as_str().unwrap_or("?"),
            ));
        }
        let current = answer["current"].as_array().map_or(0, Vec::len);
        let missing = answer["not_in_index"].as_array().map_or(0, Vec::len);
        text.push_str(&format!(
            "{} updated, {current} already current, {missing} not in the index.\n",
            updated.len()
        ));
    } else if answer["project"].is_array() || answer["user"].is_array() {
        for scope in ["project", "user"] {
            let list = answer[scope].as_array().cloned().unwrap_or_default();
            text.push_str(&format!(
                "{} packages: {}\n",
                if scope == "project" {
                    "Project"
                } else {
                    "Your"
                },
                list.len()
            ));
            for package in &list {
                text.push_str(&line(package));
                text.push('\n');
            }
        }
    } else if answer["package"].is_object() {
        let package = &answer["package"];
        let what = if dry {
            "Would install"
        } else if answer["installed"] == true {
            "Installed"
        } else {
            "Package"
        };
        text.push_str(&format!(
            "{what} {} {} ({}).\n",
            package["id"].as_str().unwrap_or("?"),
            package["version"].as_str().unwrap_or("?"),
            package["license"].as_str().unwrap_or("?"),
        ));
        if let Some(replaced) = answer["replaces"].as_str() {
            text.push_str(&format!(
                "It replaces {replaced} (the old one goes to the trash).\n"
            ));
        }
    } else if answer["removed"] == true {
        text.push_str(&format!(
            "Removed {}; it is in {}.\n",
            answer["id"].as_str().unwrap_or("?"),
            answer["trash"].as_str().unwrap_or("the trash"),
        ));
    } else if answer["lock"].is_object() {
        text.push_str(if dry {
            "The lock would be written.\n"
        } else {
            "wish.lock written.\n"
        });
    }
    for entry in answer["diagnostics"].as_array().into_iter().flatten() {
        if let Ok(problem) = serde_json::from_value::<Diagnostic>(entry.clone()) {
            text.push_str(&format!("{problem}\n"));
        }
    }
    text.trim_end().to_owned()
}

/// Text for a `script_build` answer. The answer carries no duration
/// (ADR-027, rule 2): `seconds` is what the command line measured around the
/// call.
pub fn build_text(answer: &Value, seconds: f64) -> String {
    let mut text = String::new();
    for entry in answer["diagnostics"].as_array().into_iter().flatten() {
        if let Ok(problem) = serde_json::from_value::<Diagnostic>(entry.clone()) {
            text.push_str(&format!("{problem}\n"));
        }
    }
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

/// Text for a `replay` run answer.
pub fn replay_text(answer: &Value) -> String {
    let mut text = String::new();
    for entry in answer["diagnostics"].as_array().into_iter().flatten() {
        if let Ok(problem) = serde_json::from_value::<Diagnostic>(entry.clone()) {
            text.push_str(&format!("{problem}\n"));
        }
    }
    let name = answer["name"].as_str().unwrap_or_default();
    if answer["passed"] == true {
        text.push_str(&format!(
            "Replay '{name}' passed: {} tick(s) played back bit for bit (final hash {}).",
            answer["ticks_run"],
            answer["final_hash"].as_str().unwrap_or("?")
        ));
    } else if answer["diverged_at"].is_number() {
        text.push_str(&format!(
            "Replay '{name}' diverged at tick {}: expected {}, got {}.",
            answer["diverged_at"],
            answer["expected_hash"].as_str().unwrap_or("?"),
            answer["actual_hash"].as_str().unwrap_or("?")
        ));
    } else {
        text.push_str(&format!("Replay '{name}' was stopped before it ended."));
    }
    text
}

/// Text for a `test_run` answer.
pub fn test_text(answer: &Value) -> String {
    let mut text = String::new();
    for result in answer["results"].as_array().into_iter().flatten() {
        let name = result["name"].as_str().unwrap_or_default();
        if result["passed"] == true {
            text.push_str(&format!("ok      {name} ({} ticks)\n", result["ticks"]));
            continue;
        }
        let why = &result["failure"];
        text.push_str(&format!(
            "FAILED  {name}: step {} at tick {}: {}\n",
            why["step"],
            why["tick"],
            why["message"].as_str().unwrap_or_default()
        ));
        if let Some(values) = why["values"]["entities"].as_array() {
            for entity in values {
                text.push_str(&format!(
                    "          {} {}\n",
                    entity["entity"], entity["values"]
                ));
            }
        }
        if let Some(world) = why["values"]["world"].as_object()
            && !world.is_empty()
        {
            text.push_str(&format!("          world {}\n", why["values"]["world"]));
        }
        if let Some(hint) = why["hint"].as_str() {
            text.push_str(&format!("          {hint}\n"));
        }
    }
    text.push_str(&format!(
        "{} test(s), {} failed.",
        answer["tests"], answer["failed"]
    ));
    text
}

/// The Windows Sandbox configuration of the clean-machine test: the package
/// mapped read-only, a writable folder for the result, no network, and the
/// test script started at logon.
fn sandbox_config(package: &std::path::Path, result: &std::path::Path) -> String {
    format!(
        "<Configuration>\n  <Networking>Disable</Networking>\n  <MappedFolders>\n    <MappedFolder>\n      <HostFolder>{}</HostFolder>\n      <SandboxFolder>C:\\wish</SandboxFolder>\n      <ReadOnly>true</ReadOnly>\n    </MappedFolder>\n    <MappedFolder>\n      <HostFolder>{}</HostFolder>\n      <SandboxFolder>C:\\result</SandboxFolder>\n      <ReadOnly>false</ReadOnly>\n    </MappedFolder>\n  </MappedFolders>\n  <LogonCommand>\n    <Command>powershell.exe -ExecutionPolicy Bypass -File C:\\wish\\clean-test.ps1</Command>\n  </LogonCommand>\n</Configuration>\n",
        package.display(),
        result.display()
    )
}

/// What runs inside the sandbox: a fresh Windows with no developer tools.
const SANDBOX_SCRIPT: &str = r#"# Wish Engine clean-machine test (ADR-023): run inside Windows Sandbox.
# Copies the package out of the read-only mapping, makes a project, builds
# its game code with the packaged compiler, runs its tests, and writes what
# happened to C:\result.
$ErrorActionPreference = 'Continue'
$log = 'C:\result\clean-test.log'
"Wish Engine clean-machine test, $(Get-Date -Format o)" | Out-File $log
function Step($name, $block) {
    "`n== $name" | Out-File $log -Append
    $started = Get-Date
    $output = & $block 2>&1 | Out-String
    $output | Out-File $log -Append
    "exit: $LASTEXITCODE, seconds: $([math]::Round(((Get-Date) - $started).TotalSeconds, 2))" | Out-File $log -Append
    return $LASTEXITCODE
}
"`n== developer tools on this machine (expected: none)" | Out-File $log -Append
foreach ($tool in 'cargo', 'rustc', 'rustup', 'link', 'cl') {
    $found = Get-Command $tool -ErrorAction SilentlyContinue
    "$tool : $(if ($found) { $found.Source } else { 'not found' })" | Out-File $log -Append
}
Copy-Item C:\wish C:\Users\WDAGUtilityAccount\wish -Recurse
$wen = 'C:\Users\WDAGUtilityAccount\wish\wen.exe'
$results = [ordered]@{}
$results.version = Step 'wen version' { & $wen version }
$results.new = Step 'wen new' { & $wen new C:\game --name Clean }
$results.build = Step 'wen build (first)' { & $wen build --root C:\game }
(Get-Content C:\game\scripts\src\lib.rs) -replace 'default = 1.0', 'default = 2.0' | Set-Content C:\game\scripts\src\lib.rs
$results.rebuild = Step 'wen build (after an edit)' { & $wen build --root C:\game }
$results.test = Step 'wen test' { & $wen test --root C:\game }
$results.capture = Step 'capture_frame (software adapter)' { & $wen call capture_frame --root C:\game --out shot.png --width 320 --height 180 }
if (Test-Path C:\game\shot.png) { Copy-Item C:\game\shot.png C:\result\shot.png }
$results | ConvertTo-Json | Out-File C:\result\result.json
"`n== done" | Out-File $log -Append
"#;

/// The engine's icon as a Windows `.ico` (for shortcuts): the mark on its
/// tile at 16, 24, 32, 48, 64 and 256 pixels, each stored as a PNG.
pub fn icon_file() -> wish_core::Result<Vec<u8>> {
    let sizes: [u16; 6] = [16, 24, 32, 48, 64, 256];
    let mut pictures = Vec::new();
    for size in sizes {
        let (pixels, width, height) = wish_editor::window_icon(size);
        let png = wish_render::RgbaImage {
            width,
            height,
            pixels,
        }
        .to_png()?;
        pictures.push((size, png));
    }
    let count = u16::try_from(pictures.len()).unwrap_or(u16::MAX);
    let mut ico = Vec::new();
    // The header: reserved, type 1 (icon), how many.
    ico.extend_from_slice(&0u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + 16 * pictures.len();
    for (size, png) in &pictures {
        // 256 is written as 0.
        let side = u8::try_from(*size).unwrap_or(0);
        ico.extend_from_slice(&[side, side, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&u32::try_from(png.len()).unwrap_or(u32::MAX).to_le_bytes());
        ico.extend_from_slice(&u32::try_from(offset).unwrap_or(u32::MAX).to_le_bytes());
        offset += png.len();
    }
    for (_, png) in &pictures {
        ico.extend_from_slice(png);
    }
    Ok(ico)
}

/// What `wen package` writes into its folder: the only names it replaces
/// when it packages into the same folder again.
const PACKAGE_ENTRIES: &[&str] = &[
    "wen.exe",
    "wen",
    "wish_player.exe",
    "wish_player",
    "wish-engine.exe",
    "wish-engine",
    "wish-engine.ico",
    wish_control::engines::ENGINE_FILE,
    "packages",
    "sdk",
    "toolchain",
    "templates",
    "clean-test.ps1",
    "clean-test.wsb",
    "LICENSE-MIT",
    "LICENSE-APACHE",
    "THIRD_PARTY_LICENSES.md",
];

/// The SDK's compile-time macro libraries (`sdk/host/*.dll`) that need the
/// Visual C++ runtime DLL, by name.
fn macros_needing_c_runtime(sdk: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(sdk.join("host"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".dll"))
        .filter(|entry| {
            wish_control::pe::needs_c_runtime_dll(&std::fs::read(entry.path()).unwrap_or_default())
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// `wen package`: a folder with everything a machine without developer
/// tools needs to build and run game code (engine development), and beside
/// it the release archive (`.zip` on Windows, `.tar.gz` elsewhere) and
/// `SHA256SUMS` (M12 release pipeline). What is written depends on the host:
/// the icon, the Windows Sandbox trial and the C runtime checks are
/// Windows's.
pub fn package(out: Option<&str>, as_json: bool) -> wish_core::Result<String> {
    let windows = cfg!(windows);
    let exe = std::env::current_exe().map_err(|error| {
        Diagnostic::error(
            "package_failed",
            format!("The running program could not be found: {error}."),
            "Run wen from its folder.",
        )
    })?;
    let beside = exe
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    if !manifest.is_file() {
        return Err(Diagnostic::error(
            "package_failed",
            "`wen package` builds the package from the engine's sources, which are not on this machine.",
            "This command is for engine development.",
        ));
    }
    let failed = |what: &str, error: std::io::Error| {
        Diagnostic::error(
            "package_failed",
            format!("Could not {what}: {error}."),
            "Check that the package folder can be written to.",
        )
    };
    let out = out.map_or_else(|| beside.join("package"), std::path::PathBuf::from);
    let out = std::path::absolute(&out).map_err(|e| failed("find the package folder", e))?;
    // The archive and its checksum go beside the package folder.
    let into = out
        .parent()
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| {
            Diagnostic::error(
                "package_failed",
                format!("'{}' has no parent folder for the archive.", out.display()),
                "Choose a folder inside another folder for --out.",
            )
        })?;
    let time = wish_control::release::source_date_epoch()?;
    wish_script::build::clear_output_folder(&out, PACKAGE_ENTRIES)?;
    std::fs::create_dir_all(&out).map_err(|e| failed("create the package folder", e))?;
    std::fs::copy(&exe, out.join(exe.file_name().unwrap_or_default()))
        .map_err(|e| failed("copy wen", e))?;
    // The player exported games ship (M9): built beside wen.
    let player_name = if cfg!(windows) {
        "wish_player.exe"
    } else {
        "wish_player"
    };
    let mut warnings = Vec::new();
    let player = beside.join(player_name);
    if player.is_file() {
        std::fs::copy(&player, out.join(player_name)).map_err(|e| failed("copy the player", e))?;
    } else {
        warnings.push(Diagnostic::warning(
            "package_without_player",
            "The player (wish_player) is not built beside wen, so the package cannot export games.",
            "Build it first: cargo build --release -p wish_player (with the same flags as wen), then package again.",
        ));
    }
    // The program a person starts (a shortcut, a double-click): on Windows
    // without a console window, and with an icon for shortcuts. On Linux
    // the same program, built for Linux; its desktop entry and icon need an
    // install step that does not exist yet.
    let launcher_name = if windows {
        "wish-engine.exe"
    } else {
        "wish-engine"
    };
    let launcher = beside.join(launcher_name);
    if launcher.is_file() {
        std::fs::copy(&launcher, out.join(launcher_name))
            .map_err(|e| failed("copy the launcher", e))?;
    } else {
        warnings.push(Diagnostic::warning(
            "package_without_launcher",
            if windows {
                "The launcher (wish-engine) is not built beside wen, so starting the editor from a shortcut also opens a console window."
            } else {
                "The launcher (wish-engine) is not built beside wen, so the package has no program that starts the editor from a double-click."
            },
            "Build it first: cargo build -p wish_cli builds both (with the same profile and flags as wen), then package again.",
        ));
    }
    if windows {
        std::fs::write(out.join("wish-engine.ico"), icon_file()?)
            .map_err(|e| failed("write the icon", e))?;
    }
    // The engine's own packages (themes, workspaces...): `packages/` of the
    // repository, each copied as a package (hidden files left out).
    let engine_packages = std::path::Path::new(&manifest)
        .parent()
        .map(|root| root.join("packages"))
        .unwrap_or_default();
    for entry in std::fs::read_dir(&engine_packages)
        .into_iter()
        .flatten()
        .flatten()
    {
        let from = entry.path();
        if !from.join(wish_control::packages::MANIFEST).is_file() {
            continue;
        }
        let to = out.join("packages").join(entry.file_name());
        for file in wish_control::packages::package_files(&from)? {
            let target = to.join(&file);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| failed("create a package folder", e))?;
            }
            std::fs::copy(from.join(&file), &target).map_err(|e| failed("copy a package", e))?;
        }
    }
    // The templates the welcome screen offers: the engine's examples.
    let examples = std::path::Path::new(&manifest)
        .parent()
        .map(|root| root.join("examples"))
        .unwrap_or_default();
    let templates = wish_control::scaffold::templates(&examples);
    for template in &templates {
        wish_control::scaffold::copy_template(
            &template.path,
            &out.join("templates").join(&template.id),
        )?;
    }
    let cargo = std::env::var_os("CARGO").map_or_else(|| "cargo".into(), std::path::PathBuf::from);
    let sdk = wish_script::build::pack_sdk(
        &cargo,
        &manifest,
        &beside.join("sdk-target"),
        &out.join("sdk"),
    )?;
    let rustc = std::env::var_os("RUSTC").map_or_else(|| "rustc".into(), std::path::PathBuf::from);
    let toolchain = wish_script::build::pack_toolchain(&rustc, &out.join("toolchain"))?;
    if !out
        .join("toolchain")
        .join(wish_script::build::TOOLCHAIN_NOTICES)
        .is_dir()
    {
        warnings.push(Diagnostic::warning(
            "package_without_compiler_notices",
            "The packed compiler has no licence notices (share/doc/rust beside its bin folder), so the package ships it without them.",
            "Package with a rustup toolchain (its rustc component installs them), or put the compiler's COPYRIGHT and licence files into toolchain/share/doc/rust before sharing the package.",
        ));
    }
    // The engine's licences and the third-party notices.
    wish_control::release::write_licenses(&out)?;
    // The clean-machine trial (Windows Sandbox): a person's, on Windows.
    let result = out.with_file_name(format!(
        "{}-result",
        out.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("package")
    ));
    if windows {
        std::fs::create_dir_all(&result).map_err(|e| failed("create the result folder", e))?;
        std::fs::write(out.join("clean-test.ps1"), SANDBOX_SCRIPT)
            .map_err(|e| failed("write clean-test.ps1", e))?;
        std::fs::write(out.join("clean-test.wsb"), sandbox_config(&out, &result))
            .map_err(|e| failed("write clean-test.wsb", e))?;
    }
    // A program linked against the Visual C++ runtime DLL does not start on a
    // Windows that lacks it; the package needs one linked statically.
    if windows {
        let bytes = std::fs::read(&exe).unwrap_or_default();
        if player.is_file()
            && wish_control::pe::needs_c_runtime_dll(&std::fs::read(&player).unwrap_or_default())
        {
            warnings.push(Diagnostic::warning(
                "package_needs_static_crt",
                "This wish_player needs VCRUNTIME140.dll, so games exported with it may not start on a clean Windows.",
                "Build the packaged player with the C runtime linked in: set RUSTFLAGS=-C target-feature=+crt-static, then cargo build -p wish_player --release (into the same --target-dir as wen).",
            ));
        }
        if launcher.is_file()
            && wish_control::pe::needs_c_runtime_dll(&std::fs::read(&launcher).unwrap_or_default())
        {
            warnings.push(Diagnostic::warning(
                "package_needs_static_crt",
                "This wish-engine (the launcher) needs VCRUNTIME140.dll, so it may not start on a clean Windows.",
                "Build it with the C runtime linked in: set RUSTFLAGS=-C target-feature=+crt-static, then cargo build -p wish_cli --release (into the same --target-dir as wen).",
            ));
        }
        if wish_control::pe::needs_c_runtime_dll(&bytes) {
            warnings.push(Diagnostic::warning(
                "package_needs_static_crt",
                "This wen.exe needs VCRUNTIME140.dll, which a clean Windows may not have.",
                "Build the packaged wen with the C runtime linked in: set RUSTFLAGS=-C target-feature=+crt-static, then cargo build -p wish_cli --release (into a separate --target-dir), and run that wen's `package`.",
            ));
        }
        // The packed compiler loads these to build game code.
        let macros = macros_needing_c_runtime(&out.join("sdk"));
        if !macros.is_empty() {
            warnings.push(Diagnostic::warning(
                "package_needs_static_crt",
                format!(
                    "The SDK's compile-time macros ({}) need VCRUNTIME140.dll, so game code may not build on a Windows without the Visual C++ runtime.",
                    macros.join(", ")
                ),
                "Not fixed yet: cargo builds them for the host, where RUSTFLAGS does not reach when the SDK is built for wasm32 (docs/design/M12-release-pipeline.md, section 6). Until then such a machine needs the Visual C++ Redistributable.",
            ));
        }
    }
    // What makes the folder an engine the Hub can list beside others:
    // written last, so a package that stopped half way is not one.
    let version = wish_control::packages::Version::engine();
    std::fs::write(
        out.join(wish_control::engines::ENGINE_FILE),
        wish_control::engines::engine_file_text(version)?,
    )
    .map_err(|e| failed("write the engine file", e))?;
    // The release archive of the finished folder, and its checksum.
    let release = wish_control::release::write(
        &out,
        &into,
        &wish_control::release::archive_stem(&version.to_string()),
        wish_control::archive::Format::for_host(),
        time,
    )?;
    let toolchain_mib = toolchain["bytes"].as_u64().unwrap_or(0) / (1024 * 1024);
    Ok(if as_json {
        serde_json::json!({
            "out": out.display().to_string(),
            "sdk": sdk,
            "toolchain": toolchain,
            "archive": release.to_json(),
            "sandbox": windows.then(|| out.join("clean-test.wsb").display().to_string()),
            "result": windows.then(|| result.display().to_string()),
            "diagnostics": warnings,
        })
        .to_string()
    } else {
        let mut text: String = warnings.iter().map(|w| format!("{w}\n")).collect();
        text.push_str(&format!(
            "Package written to {} (toolchain {toolchain_mib} MiB, {}).\nArchive: {} ({} MiB), SHA-256 {} (in {}).",
            out.display(),
            toolchain["rustc"].as_str().unwrap_or("?"),
            release.archive.display(),
            release.bytes / (1024 * 1024),
            release.sha256,
            release.sums.display(),
        ));
        if windows {
            text.push_str(&format!(
                "\nClean-machine test: open {} in Windows Sandbox; the result appears in {}.",
                out.join("clean-test.wsb").display(),
                result.display()
            ));
        }
        text
    })
}

#[cfg(test)]
mod icon_tests {
    #[test]
    fn the_icon_file_holds_six_pngs_of_the_mark() {
        let ico = super::icon_file().unwrap();
        assert_eq!(&ico[0..6], &[0, 0, 1, 0, 6, 0]);
        let mut sides = Vec::new();
        for entry in 0..6 {
            let at = 6 + entry * 16;
            sides.push(ico[at]);
            let size = u32::from_le_bytes(ico[at + 8..at + 12].try_into().unwrap()) as usize;
            let offset = u32::from_le_bytes(ico[at + 12..at + 16].try_into().unwrap()) as usize;
            assert_eq!(&ico[offset + 1..offset + 4], b"PNG");
            assert!(offset + size <= ico.len());
        }
        assert_eq!(sides, [16, 24, 32, 48, 64, 0]);
    }
}
