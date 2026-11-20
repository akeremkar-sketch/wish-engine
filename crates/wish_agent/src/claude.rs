//! Claude Code as the cockpit's agent (ADR-024): run as a subprocess on the
//! person's own account (`claude -p`, signed in once by the person), with
//! the editor's MCP endpoint as its only MCP server. No API key, nothing
//! sold: the person brings their own Claude.
//!
//! Its `stream-json` output is read line by line into [`Event`]s
//! ([`Parser`]).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};

use serde_json::{Value, json};

use crate::{Adapter, Event, Run, Task, instructions};

/// The tools Claude Code may use without asking: the editor's, and reading
/// and editing the project's files (game code).
pub const ALLOWED_TOOLS: &[&str] = &["mcp__wish", "Read", "Edit", "Write", "Glob", "Grep"];

/// Claude Code, run as `program` (`claude` by default; `WISH_CLAUDE` names
/// another).
#[derive(Clone, Debug)]
pub struct ClaudeCode {
    /// The program.
    pub program: PathBuf,
}

/// Where Claude Code is installed, if it is: `WISH_CLAUDE`, else `claude`
/// on the `PATH` (`claude.exe` or npm's `claude.cmd` on Windows), else the
/// native installer's place (`~/.local/bin`). Only looks; starts nothing.
pub fn find() -> Option<PathBuf> {
    if let Some(program) = std::env::var_os("WISH_CLAUDE").filter(|value| !value.is_empty()) {
        let program = PathBuf::from(program);
        return program.is_file().then_some(program);
    }
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    find_in(
        var("PATH").as_deref(),
        var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).as_deref(),
        var("LOCALAPPDATA").as_deref(),
    )
}

/// [`find`] without reading the environment: the folders of `path`, then
/// `home/.local/bin` (the native installer), then (Windows) the Claude
/// desktop app's own copy of the roaming folder. Windows keeps that copy
/// for packaged apps: Claude Code installed with npm from inside the
/// Claude app lands in `%LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming\npm`,
/// which programs outside the app do not see as `%APPDATA%\npm`.
pub fn find_in(
    path: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
    local_app_data: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };
    let mut folders: Vec<PathBuf> = path
        .map(|path| std::env::split_paths(path).collect())
        .unwrap_or_default();
    if let Some(home) = home {
        folders.push(PathBuf::from(home).join(".local").join("bin"));
    }
    if let Some(local) = local_app_data {
        let packages = PathBuf::from(local).join("Packages");
        let mut apps: Vec<PathBuf> = std::fs::read_dir(&packages)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("Claude_"))
            .map(|entry| entry.path().join("LocalCache").join("Roaming").join("npm"))
            .collect();
        apps.sort();
        folders.extend(apps);
    }
    folders
        .iter()
        .flat_map(|folder| names.iter().map(move |name| folder.join(name)))
        .find(|candidate| candidate.is_file())
}

impl Default for ClaudeCode {
    fn default() -> Self {
        let program = std::env::var_os("WISH_CLAUDE")
            .map(PathBuf::from)
            .or_else(find)
            .unwrap_or_else(|| {
                PathBuf::from(if cfg!(windows) {
                    "claude.exe"
                } else {
                    "claude"
                })
            });
        Self { program }
    }
}

/// The MCP configuration that points Claude Code at the editor.
pub fn mcp_config(task: &Task) -> Value {
    json!({ "mcpServers": { "wish": {
        "type": "http",
        "url": task.endpoint,
        "headers": { "Authorization": format!("Bearer {}", task.token) }
    } } })
}

/// Reads Claude Code's `stream-json` lines into events.
#[derive(Debug, Default)]
pub struct Parser {
    /// Tool calls by id, to name their answers.
    calls: BTreeMap<String, String>,
}

/// A tool's name without the MCP prefix (`mcp__wish__edit_apply` →
/// `edit_apply`).
fn short_tool(name: &str) -> String {
    name.strip_prefix("mcp__wish__").unwrap_or(name).to_owned()
}

impl Parser {
    /// The events of one output line (none for lines that say nothing to
    /// the person, or that are not JSON).
    pub fn line(&mut self, line: &str) -> Vec<Event> {
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
            return Vec::new();
        };
        let content = message["message"]["content"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        match message["type"].as_str() {
            Some("system") if message["subtype"] == "init" => {
                let mut events = Vec::new();
                if let Some(session) = message["session_id"].as_str() {
                    events.push(Event::Session(session.to_owned()));
                }
                let wish = message["mcp_servers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|server| server["name"] == "wish");
                if let Some(server) = wish
                    && server["status"] != "connected"
                {
                    events.push(Event::Said(format!(
                        "(Claude Code could not connect to the editor: {})",
                        server["status"].as_str().unwrap_or("unknown")
                    )));
                }
                events
            }
            Some("assistant") => content
                .iter()
                .filter_map(|item| match item["type"].as_str() {
                    Some("text") => item["text"]
                        .as_str()
                        .filter(|text| !text.trim().is_empty())
                        .map(|text| Event::Said(text.to_owned())),
                    Some("tool_use") => {
                        let tool = short_tool(item["name"].as_str().unwrap_or("?"));
                        if let Some(id) = item["id"].as_str() {
                            self.calls.insert(id.to_owned(), tool.clone());
                        }
                        Some(Event::Called {
                            tool,
                            args: item["input"].clone(),
                        })
                    }
                    _ => None,
                })
                .collect(),
            Some("user") => content
                .iter()
                .filter(|item| item["type"] == "tool_result")
                .map(|item| {
                    let tool = item["tool_use_id"]
                        .as_str()
                        .and_then(|id| self.calls.get(id).cloned())
                        .unwrap_or_else(|| "?".to_owned());
                    // A tool's own failure shows in its answer's "ok".
                    let text = item["content"].to_string();
                    let ok = item["is_error"] != true && !text.contains("\\\"ok\\\":false");
                    Event::Answered { tool, ok }
                })
                .collect(),
            Some("result") => {
                let mut events = Vec::new();
                if let Some(cost) = message["total_cost_usd"].as_f64() {
                    events.push(Event::Cost(cost));
                }
                events.push(Event::Finished {
                    ok: message["is_error"] != true && message["subtype"] == "success",
                    text: message["result"].as_str().unwrap_or_default().to_owned(),
                });
                events
            }
            _ => Vec::new(),
        }
    }
}

/// Claude Code's command line for `task`, with its MCP settings at
/// `config`: print mode with the stream of events, the editor as its only
/// MCP server, the allowed tools, and the chosen model and conversation.
pub fn arguments(task: &Task, config: &Path) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = ["-p", "--output-format", "stream-json", "--verbose"]
        .iter()
        .map(Into::into)
        .collect();
    args.push("--mcp-config".into());
    args.push(config.into());
    args.push("--strict-mcp-config".into());
    args.push("--allowedTools".into());
    let allowed: &[&str] = if task.files {
        ALLOWED_TOOLS
    } else {
        &ALLOWED_TOOLS[..1]
    };
    args.extend(allowed.iter().map(Into::into));
    if let Some(model) = task.model.as_deref().filter(|model| valid_word(model)) {
        args.push("--model".into());
        args.push(model.into());
    }
    if let Some(session) = task.resume.as_deref().filter(|session| valid_word(session)) {
        args.push("--resume".into());
        args.push(session.into());
    }
    args
}

/// A model name or session id: letters, digits, `-`, `_`, `.`, `[`, `]`
/// (nothing that could be read as another option).
fn valid_word(word: &str) -> bool {
    !word.is_empty()
        && !word.starts_with('-')
        && word.len() <= 100
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '[' | ']'))
}

fn write_config(project: &Path, config: &Value) -> Result<PathBuf, String> {
    let folder = project.join(".wish").join("agent");
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("cannot write the agent's settings: {e}"))?;
    let path = folder.join("mcp.json");
    std::fs::write(&path, format!("{config:#}\n"))
        .map_err(|e| format!("cannot write the agent's settings: {e}"))?;
    Ok(path)
}

impl Adapter for ClaudeCode {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn start(&self, task: &Task) -> Result<Run, String> {
        let config = write_config(&task.project, &mcp_config(task))?;
        let mut child = Command::new(&self.program)
            .current_dir(&task.project)
            .args(arguments(task, &config))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                format!(
                    "Claude Code could not be started ({}: {e}). Install it and sign in once (claude), or set WISH_CLAUDE to where it is.",
                    self.program.display()
                )
            })?;
        // The request goes in through standard input (no quoting trouble).
        if let Some(mut input) = child.stdin.take() {
            let _ = input.write_all(instructions(task).as_bytes());
        }
        let stdout = child.stdout.take().ok_or("no output from Claude Code")?;
        let mut stderr = child.stderr.take();
        let child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(Some(child)));
        let waiting = Arc::clone(&child);
        let (events, received) = mpsc::channel();
        std::thread::Builder::new()
            .name("wish-agent-claude".to_owned())
            .spawn(move || wish_control::crash::thread(move || {
                let mut parser = Parser::default();
                let mut finished = false;
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    for event in parser.line(&line) {
                        finished |= matches!(event, Event::Finished { .. });
                        let _ = events.send(event);
                    }
                }
                let status = waiting
                    .lock()
                    .ok()
                    .and_then(|mut slot| slot.take())
                    .and_then(|mut child| child.wait().ok());
                if !finished {
                    let mut said = String::new();
                    if let Some(stderr) = stderr.as_mut() {
                        let _ = stderr.read_to_string(&mut said);
                    }
                    let said: String = said.trim().chars().rev().take(400).collect::<String>().chars().rev().collect();
                    let _ = events.send(Event::Failed(match status {
                        Some(status) if !said.is_empty() => format!("Claude Code ended ({status}): {said}"),
                        Some(status) => format!("Claude Code ended ({status}) without an answer. Is it signed in? Run claude once."),
                        None => "Claude Code was stopped.".to_owned(),
                    }));
                }
            }))
            .map_err(|e| format!("could not watch Claude Code: {e}"))?;
        Ok(Run::new(
            received,
            Box::new(move || {
                if let Ok(mut slot) = child.lock()
                    && let Some(mut running) = slot.take()
                {
                    let _ = running.kill();
                    let _ = running.wait();
                }
            }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_json_lines_become_events() {
        let mut parser = Parser::default();
        assert!(parser
            .line(r#"{"type":"system","subtype":"init","mcp_servers":[{"name":"wish","status":"connected"}]}"#)
            .is_empty());
        assert_eq!(
            parser.line(r#"{"type":"system","subtype":"init","mcp_servers":[{"name":"wish","status":"failed"}]}"#),
            [Event::Said("(Claude Code could not connect to the editor: failed)".to_owned())]
        );
        let said = parser.line(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Adding a crate."},{"type":"tool_use","id":"t1","name":"mcp__wish__edit_apply","input":{"ops":[]}}]}}"#,
        );
        assert_eq!(
            said,
            [
                Event::Said("Adding a crate.".to_owned()),
                Event::Called {
                    tool: "edit_apply".to_owned(),
                    args: json!({ "ops": [] })
                }
            ]
        );
        assert_eq!(
            parser.line(r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"{\"ok\":true}"}]}]}}"#),
            [Event::Answered { tool: "edit_apply".to_owned(), ok: true }]
        );
        assert_eq!(
            parser.line(r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"{\"ok\":false}"}]}]}}"#),
            [Event::Answered { tool: "edit_apply".to_owned(), ok: false }]
        );
        assert_eq!(
            parser.line(r#"{"type":"result","subtype":"success","is_error":false,"result":"Done: a crate."}"#),
            [Event::Finished { ok: true, text: "Done: a crate.".to_owned() }]
        );
        assert!(parser.line("not json").is_empty());
        // The conversation's id and the cost.
        assert_eq!(
            parser.line(
                r#"{"type":"system","subtype":"init","session_id":"abc-123","mcp_servers":[]}"#
            ),
            [Event::Session("abc-123".to_owned())]
        );
        assert_eq!(
            parser.line(r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","total_cost_usd":0.25}"#),
            [Event::Cost(0.25), Event::Finished { ok: true, text: "ok".to_owned() }]
        );
    }

    fn words(task: &Task) -> Vec<String> {
        arguments(task, Path::new("mcp.json"))
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_model_and_the_conversation_go_on_the_command_line() {
        let mut task = Task::new("", "http://x/mcp", "t", PathBuf::from("."), "s");
        let plain = words(&task);
        assert!(!plain.contains(&"--model".to_owned()));
        assert!(!plain.contains(&"--resume".to_owned()));
        task.model = Some("opus".to_owned());
        task.resume = Some("abc-123".to_owned());
        let args = words(&task);
        let at = |flag: &str| args.iter().position(|a| a == flag).unwrap();
        assert_eq!(args[at("--model") + 1], "opus");
        assert_eq!(args[at("--resume") + 1], "abc-123");
        // Nothing that reads as another option gets through.
        task.model = Some("--dangerously-skip-permissions".to_owned());
        task.resume = Some("a b".to_owned());
        let args = words(&task);
        assert!(
            !args
                .iter()
                .any(|a| a.contains("dangerously") || a == "--resume")
        );
        // Without the project's files, only the editor's tools.
        assert!(words(&task).contains(&"Edit".to_owned()));
        task.files = false;
        let args = words(&task);
        assert!(args.contains(&"mcp__wish".to_owned()));
        assert!(!args.contains(&"Edit".to_owned()));
    }

    #[test]
    fn the_config_points_at_the_editor_with_its_token() {
        let task = Task::new(
            "",
            "http://127.0.0.1:5000/mcp",
            "abc",
            PathBuf::from("."),
            "scenes/main.scene.json",
        );
        let config = mcp_config(&task);
        assert_eq!(config["mcpServers"]["wish"]["type"], "http");
        assert_eq!(
            config["mcpServers"]["wish"]["headers"]["Authorization"],
            "Bearer abc"
        );
    }
}
