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

/// `wen package`: a folder with everything a machine without developer
/// tools needs to build and run game code (engine development).
pub fn package(out: Option<&str>, as_json: bool) -> wish_core::Result<String> {
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
    let out = out.map_or_else(|| beside.join("package"), std::path::PathBuf::from);
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
    wish_script::build::clear_output_folder(
        &out,
        &[
            "wen.exe",
            "wen",
            "sdk",
            "toolchain",
            "clean-test.ps1",
            "clean-test.wsb",
        ],
    )?;
    std::fs::create_dir_all(&out).map_err(|e| failed("create the package folder", e))?;
    std::fs::copy(&exe, out.join(exe.file_name().unwrap_or_default()))
        .map_err(|e| failed("copy wen", e))?;
    let cargo = std::env::var_os("CARGO").map_or_else(|| "cargo".into(), std::path::PathBuf::from);
    let sdk = wish_script::build::pack_sdk(
        &cargo,
        &manifest,
        &beside.join("sdk-target"),
        &out.join("sdk"),
    )?;
    let rustc = std::env::var_os("RUSTC").map_or_else(|| "rustc".into(), std::path::PathBuf::from);
    let toolchain = wish_script::build::pack_toolchain(&rustc, &out.join("toolchain"))?;
    let result = out.with_file_name(format!(
        "{}-result",
        out.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("package")
    ));
    std::fs::create_dir_all(&result).map_err(|e| failed("create the result folder", e))?;
    std::fs::write(out.join("clean-test.ps1"), SANDBOX_SCRIPT)
        .map_err(|e| failed("write clean-test.ps1", e))?;
    std::fs::write(out.join("clean-test.wsb"), sandbox_config(&out, &result))
        .map_err(|e| failed("write clean-test.wsb", e))?;
    // A program linked against the Visual C++ runtime DLL does not start on a
    // Windows that lacks it; the package needs one linked statically.
    let mut warnings = Vec::new();
    let bytes = std::fs::read(&exe).unwrap_or_default();
    let imports = imported_dlls(&bytes);
    if imports
        .iter()
        .any(|dll| dll.to_ascii_lowercase().starts_with("vcruntime"))
    {
        warnings.push(Diagnostic::warning(
            "package_needs_static_crt",
            "This wen.exe needs VCRUNTIME140.dll, which a clean Windows may not have.",
            "Build the packaged wen with the C runtime linked in: set RUSTFLAGS=-C target-feature=+crt-static, then cargo build -p wish_cli --release (into a separate --target-dir), and run that wen's `package`.",
        ));
    }
    let toolchain_mib = toolchain["bytes"].as_u64().unwrap_or(0) / (1024 * 1024);
    Ok(if as_json {
        serde_json::json!({
            "out": out.display().to_string(),
            "sdk": sdk,
            "toolchain": toolchain,
            "sandbox": out.join("clean-test.wsb").display().to_string(),
            "result": result.display().to_string(),
            "diagnostics": warnings,
        })
        .to_string()
    } else {
        let mut text: String = warnings.iter().map(|w| format!("{w}\n")).collect();
        text.push_str(&format!(
            "Package written to {} (toolchain {toolchain_mib} MiB, {}).\nClean-machine test: open {} in Windows Sandbox; the result appears in {}.",
            out.display(),
            toolchain["rustc"].as_str().unwrap_or("?"),
            out.join("clean-test.wsb").display(),
            result.display()
        ));
        text
    })
}

/// The DLLs a Windows program imports, read from its PE import table (an
/// empty list for anything that is not a PE file).
fn imported_dlls(bytes: &[u8]) -> Vec<String> {
    let u16_at = |at: usize| -> Option<usize> {
        Some(usize::from(u16::from_le_bytes(
            bytes.get(at..at + 2)?.try_into().ok()?,
        )))
    };
    let u32_at = |at: usize| -> Option<usize> {
        usize::try_from(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?)).ok()
    };
    let read = || -> Option<Vec<String>> {
        let pe = u32_at(0x3c)?;
        if bytes.get(pe..pe + 4)? != b"PE\0\0" {
            return None;
        }
        let sections = u16_at(pe + 6)?;
        let optional = pe + 24;
        let optional_size = u16_at(pe + 20)?;
        // PE32+ (0x20b) keeps the data directories 16 bytes further on.
        let directories = optional + if u16_at(optional)? == 0x20b { 112 } else { 96 };
        let import_rva = u32_at(directories + 8)?;
        let table = optional + optional_size;
        let to_offset = |rva: usize| -> Option<usize> {
            (0..sections).find_map(|i| {
                let header = table + i * 40;
                let start = u32_at(header + 12)?;
                let size = u32_at(header + 8)?.max(u32_at(header + 16)?);
                let raw = u32_at(header + 20)?;
                (rva >= start && rva < start + size).then(|| raw + rva - start)
            })
        };
        let mut names = Vec::new();
        let mut descriptor = to_offset(import_rva)?;
        while names.len() < 256 {
            let name_rva = u32_at(descriptor + 12)?;
            if name_rva == 0 {
                break;
            }
            let at = to_offset(name_rva)?;
            let end = bytes.get(at..)?.iter().position(|b| *b == 0)?;
            names.push(String::from_utf8_lossy(&bytes[at..at + end]).to_string());
            descriptor += 20;
        }
        Some(names)
    };
    read().unwrap_or_default()
}
