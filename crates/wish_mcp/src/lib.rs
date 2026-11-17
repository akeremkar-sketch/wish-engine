//! The Wish Engine MCP server.
//!
//! A thin client of the control plane (`wish_control`): every tool is a
//! catalogue tool, every answer is the same JSON the command line prints.
//! The transport is stdio: one JSON-RPC 2.0 message per line, stdout for
//! protocol only (design: `docs/design/M4-kontrol-duzlemi.md` §7).
//!
//! Protocol version 2025-06-18 (older versions the client asks for are
//! accepted and answered with the same behaviour). Requests are handled one
//! at a time, in order. `notifications/cancelled` for the request being
//! worked on (or one still waiting) stops a running `sim_step` at the next
//! tick, which then answers with `stopped_by: "cancelled"` (see [`serve`]).
//!
//! The same server also speaks Streamable HTTP ([`http::serve_http`]), for
//! clients that connect to a running engine.

pub mod http;

use std::borrow::BorrowMut;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use serde_json::{Map, Value, json};
use wish_control::{Workspace, catalog, docs};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

/// The protocol version this server speaks.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// Versions a client may ask for.
pub const SUPPORTED_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const RESOURCE_NOT_FOUND: i64 = -32002;

/// Tools that never change files or state.
/// Tools whose effect cannot be undone through `edit_history`.
const DESTRUCTIVE: &[&str] = &["capture_frame", "sim_stop", "checkpoint", "project_export"];

const READ_ONLY: &[&str] = &[
    "project_info",
    "schema_get",
    "docs_search",
    "check",
    "scene_get",
    "entity_query",
    "entity_get",
    "asset_list",
    "asset_info",
    "describe_view",
    "logs_get",
    "activity_get",
];

const INSTRUCTIONS: &str = "Wish Engine: a deterministic 3D game engine driven by these tools. Start with project_info, then read the resource wish://docs/guide (short). Change scenes with edit_apply (many operations at once, all or nothing), run them with sim_start and sim_step, and check what is on screen with describe_view and capture_frame. Every error carries a hint that says how to fix the call; read it before retrying. Game code lives in scripts/src/lib.rs (Rust; read wish://docs/sdk, and wish://docs/recipes for working pieces of games): edit it, build with script_build, prove behaviour with a scenario test (test_run). A person may be editing the same project: mind changed_by_others.";

/// One MCP session over one workspace.
pub struct Server<W: BorrowMut<Workspace> = Workspace> {
    workspace: W,
    actor: Actor,
    protocol: Option<String>,
    /// The call being worked on, for cancellations.
    busy: Arc<Busy>,
}

/// A call, as cancellations name it: the session it came from (`""` on
/// stdio) and its JSON-RPC id (ids are per session: two clients may both
/// send request 1).
type CallKey = (String, String);

/// How many cancellations and finished calls are remembered.
const REMEMBERED: usize = 1024;

/// The call being worked on, cancellations that came before their call,
/// and calls that finished, shared by the threads that read messages and
/// the one that runs calls.
pub(crate) struct Busy {
    /// Set to stop the running call at its next tick.
    pub(crate) flag: Arc<AtomicBool>,
    state: Mutex<BusyState>,
}

#[derive(Default)]
struct BusyState {
    current: Option<CallKey>,
    /// Cancelled before they started.
    pending: std::collections::VecDeque<CallKey>,
    /// Finished: a cancellation arriving late for one of these is dropped,
    /// so it cannot stop a later call that reuses the id.
    done: std::collections::VecDeque<CallKey>,
}

fn key(session: &str, id: &Value) -> CallKey {
    (session.to_owned(), id.to_string())
}

fn remember(list: &mut std::collections::VecDeque<CallKey>, key: CallKey) {
    if list.len() >= REMEMBERED {
        list.pop_front();
    }
    list.push_back(key);
}

impl Busy {
    fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            state: Mutex::new(BusyState::default()),
        }
    }

    /// The client of `session` cancelled `wanted`: stop it if it runs, or
    /// before it starts; nothing if it already finished.
    pub(crate) fn cancel(&self, session: &str, wanted: &Value) {
        let wanted = key(session, wanted);
        if let Ok(mut state) = self.state.lock() {
            if state.current.as_ref() == Some(&wanted) {
                self.flag.store(true, Ordering::Relaxed);
            } else if !state.done.contains(&wanted) && !state.pending.contains(&wanted) {
                remember(&mut state.pending, wanted);
            }
        }
    }

    /// The call `id` of `session` starts. The current call is set and the
    /// pending cancellations checked under one lock, so a cancellation read
    /// just before or while the call starts is never missed.
    pub(crate) fn start(&self, session: &str, id: Option<&Value>) {
        self.flag.store(false, Ordering::Relaxed);
        if let Ok(mut state) = self.state.lock() {
            let started = id.map(|id| key(session, id));
            let cancelled = started.as_ref().is_some_and(|started| {
                let before = state.pending.len();
                state.pending.retain(|k| k != started);
                state.pending.len() != before
            });
            state.current = started;
            if cancelled {
                self.flag.store(true, Ordering::Relaxed);
            }
        }
    }

    /// The call is done.
    pub(crate) fn finish(&self) {
        if let Ok(mut state) = self.state.lock()
            && let Some(done) = state.current.take()
        {
            remember(&mut state.done, done);
        }
    }

    /// A session ended: its cancellations and finished calls are dropped.
    pub(crate) fn forget(&self, session: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.pending.retain(|(s, _)| s != session);
            state.done.retain(|(s, _)| s != session);
        }
    }
}

impl<W: BorrowMut<Workspace>> std::fmt::Debug for Server<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("actor", &self.actor)
            .field("protocol", &self.protocol)
            .finish_non_exhaustive()
    }
}

fn response(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

/// The answer of a tool as MCP content: the image (if any) as an image
/// block, everything else as JSON text and as structured content.
fn tool_result(mut answer: Value) -> Value {
    let image = answer
        .get_mut("image")
        .and_then(Value::as_object_mut)
        .and_then(|image| image.remove("data"))
        .and_then(|data| data.as_str().map(str::to_owned));
    let is_error = answer.get("ok") != Some(&Value::Bool(true));
    let mut content = vec![json!({ "type": "text", "text": answer.to_string() })];
    if let Some(data) = image {
        content.push(json!({ "type": "image", "data": data, "mimeType": "image/png" }));
    }
    json!({ "content": content, "structuredContent": answer, "isError": is_error })
}

fn tool_list() -> Value {
    let tools: Vec<Value> = catalog()
        .iter()
        .map(|tool| {
            let read_only = READ_ONLY.contains(&tool.name);
            json!({
                "name": tool.name,
                "title": tool.title,
                "description": tool.description,
                "inputSchema": wish_control::input_schema(tool.params),
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "ok": { "type": "boolean", "description": "true: the call worked; false: see error." },
                        "error": { "type": "object", "description": "What went wrong: code, message, location, hint, candidates, docs." }
                    },
                    "required": ["ok"]
                },
                "annotations": {
                    "title": tool.title,
                    "readOnlyHint": read_only,
                    // What cannot be undone: a picture replacing a file
                    // (`overwrite`), a play session thrown away.
                    "destructiveHint": DESTRUCTIVE.contains(&tool.name),
                    // scene_get also marks what the caller has seen.
                    "idempotentHint": read_only && tool.name != "scene_get",
                    "openWorldHint": false
                }
            })
        })
        .collect();
    json!({ "tools": tools })
}

fn prompts() -> Value {
    json!({ "prompts": [
        {
            "name": "new_scene",
            "title": "Build a scene",
            "description": "Build a scene from a description, check it runs and looks right.",
            "arguments": [{ "name": "idea", "description": "What the scene should be, in your own words.", "required": true }]
        },
        {
            "name": "reproduce_and_fix",
            "title": "Reproduce and fix a problem",
            "description": "Reproduce a problem in a play session, find the cause, fix the scene and prove the fix.",
            "arguments": [{ "name": "problem", "description": "What goes wrong.", "required": true }]
        }
    ] })
}

fn prompt_text(name: &str, arguments: &Map<String, Value>) -> Option<String> {
    let arg = |key: &str| {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("(not given)")
            .to_owned()
    };
    match name {
        "new_scene" => Some(format!(
            "Build this scene in the current Wish Engine project: {}\n\n\
Work in this order: project_info; schema_get (search) for the components you need; edit_apply to build it \
(all operations in one call where you can); describe_view to check that everything important is visible; \
capture_frame once to look at it; sim_start and sim_step for a few seconds if anything moves; check at the end. \
Report what you built and what you verified.",
            arg("idea")
        )),
        "reproduce_and_fix" => Some(format!(
            "Problem: {}\n\n\
Reproduce it first: sim_start (note the seed), sim_snapshot before the interesting moment, sim_step with until \
to stop when it happens, entity_query / entity_get to see the values. Find the cause, fix the scene with \
edit_apply, then prove the fix by restarting the session with the same seed and showing the problem no longer \
happens. Report the cause, the fix and the evidence.",
            arg("problem")
        )),
        _ => None,
    }
}

impl<W: BorrowMut<Workspace>> Server<W> {
    /// A server over `workspace` (owned, or borrowed: `&mut Workspace`).
    pub fn new(workspace: W) -> Self {
        Self {
            workspace,
            actor: Actor::Agent("mcp-client".to_owned()),
            protocol: None,
            busy: Arc::new(Busy::new()),
        }
    }

    /// The workspace (for tests).
    pub fn workspace(&mut self) -> &mut Workspace {
        self.workspace.borrow_mut()
    }

    /// Handles one line of input and returns the line to send back, if any
    /// (notifications get no answer). Never panics out.
    pub fn handle(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let id = request_id(line).unwrap_or(Value::Null);
        let answer =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.handle_value(line)))
                .unwrap_or_else(|_| {
                    Some(error(
                        &id,
                        -32603,
                        "The server failed on this message; nothing was changed.",
                        None,
                    ))
                });
        answer.map(|value| value.to_string())
    }

    fn handle_value(&mut self, line: &str) -> Option<Value> {
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(problem) => {
                return Some(error(
                    &Value::Null,
                    PARSE_ERROR,
                    &format!("Not valid JSON: {problem}."),
                    None,
                ));
            }
        };
        let Some(object) = message.as_object() else {
            return Some(error(
                &Value::Null,
                INVALID_REQUEST,
                "Send one JSON-RPC message object per line (batches are not part of MCP 2025-06-18).",
                None,
            ));
        };
        let id = object.get("id").cloned();
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            // A response to something we never asked: nothing to do.
            if object.contains_key("result") || object.contains_key("error") {
                return None;
            }
            return Some(error(
                &id.unwrap_or(Value::Null),
                INVALID_REQUEST,
                "A request needs a \"method\".",
                None,
            ));
        };
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = id else {
            // A notification: initialized and cancelled need no action.
            return None;
        };
        Some(self.request(&id, method, &params))
    }

    fn request(&mut self, id: &Value, method: &str, params: &Value) -> Value {
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str);
                let chosen = asked
                    .filter(|version| SUPPORTED_VERSIONS.contains(version))
                    .unwrap_or(PROTOCOL_VERSION)
                    .to_owned();
                if let Some(name) = params
                    .get("clientInfo")
                    .and_then(|info| info.get("name"))
                    .and_then(Value::as_str)
                    .filter(|name| !name.trim().is_empty())
                {
                    self.actor = Actor::Agent(name.trim().to_owned());
                }
                self.protocol = Some(chosen.clone());
                response(
                    id,
                    json!({
                        "protocolVersion": chosen,
                        "capabilities": {
                            "tools": { "listChanged": false },
                            "resources": { "listChanged": false, "subscribe": false },
                            "prompts": { "listChanged": false }
                        },
                        "serverInfo": { "name": "wish-engine", "title": "Wish Engine", "version": env!("CARGO_PKG_VERSION") },
                        "instructions": INSTRUCTIONS
                    }),
                )
            }
            "ping" => response(id, json!({})),
            "tools/list" => response(id, tool_list()),
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Value::as_str) else {
                    return error(id, INVALID_PARAMS, "tools/call needs params.name.", None);
                };
                // An unknown tool is a protocol error in MCP 2025-06-18
                // (-32602), with the same hint and candidates as elsewhere.
                if let Err(problem) = wish_control::find_tool(name) {
                    return error(
                        id,
                        INVALID_PARAMS,
                        &format!("{} {}", problem.message, problem.hint),
                        Some(
                            json!({ "code": problem.code, "hint": problem.hint, "candidates": problem.candidates }),
                        ),
                    );
                }
                let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
                let answer = self
                    .workspace
                    .borrow_mut()
                    .call(name, &arguments, &self.actor);
                response(id, tool_result(answer))
            }
            "resources/list" => response(id, self.resource_list()),
            "resources/templates/list" => response(
                id,
                json!({ "resourceTemplates": [
                    { "uriTemplate": "wish://docs/components/{name}", "name": "component", "title": "Component, resource or event type", "description": "Fields, types, units, ranges and defaults of one type.", "mimeType": "text/markdown" },
                    { "uriTemplate": "wish://docs/diagnostics/{code}", "name": "diagnostic", "title": "Diagnostic code", "description": "What an error or warning code means.", "mimeType": "text/markdown" }
                ] }),
            ),
            "resources/read" => {
                let Some(uri) = params.get("uri").and_then(Value::as_str) else {
                    return error(id, INVALID_PARAMS, "resources/read needs params.uri.", None);
                };
                match self.read_resource(uri) {
                    Ok((mime, text)) => response(
                        id,
                        json!({ "contents": [{ "uri": uri, "mimeType": mime, "text": text }] }),
                    ),
                    Err(problem) => error(
                        id,
                        RESOURCE_NOT_FOUND,
                        &problem.message,
                        Some(
                            json!({ "uri": uri, "hint": problem.hint, "candidates": problem.candidates }),
                        ),
                    ),
                }
            }
            "prompts/list" => response(id, prompts()),
            "prompts/get" => {
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let empty = Map::new();
                let arguments = params
                    .get("arguments")
                    .and_then(Value::as_object)
                    .unwrap_or(&empty);
                match prompt_text(name, arguments) {
                    Some(text) => response(
                        id,
                        json!({ "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] }),
                    ),
                    None => error(
                        id,
                        INVALID_PARAMS,
                        &format!("There is no prompt '{name}'; prompts/list shows them."),
                        None,
                    ),
                }
            }
            other => error(
                id,
                METHOD_NOT_FOUND,
                &format!("The method '{other}' is not supported by this server."),
                Some(json!({ "supported": [
                    "initialize", "ping", "tools/list", "tools/call", "resources/list",
                    "resources/templates/list", "resources/read", "prompts/list", "prompts/get"
                ] })),
            ),
        }
    }

    fn type_names(&self) -> Vec<String> {
        let registry = self.workspace.borrow().registry();
        registry
            .components()
            .map(|(_, info)| info.name().to_owned())
            .chain(registry.resources().map(|(_, info)| info.name().to_owned()))
            .chain(registry.events().map(|(_, info)| info.name().to_owned()))
            .collect()
    }

    fn resource_list(&self) -> Value {
        let mut resources = vec![
            json!({ "uri": "wish://docs/guide", "name": "guide", "title": "Engine guide", "description": "How to work with the engine: the loop, the tools, scenes, rules. Read first.", "mimeType": "text/markdown" }),
            json!({ "uri": "wish://docs/sdk", "name": "sdk", "title": "Game code SDK", "description": "Writing game code in scripts/src/lib.rs: components, systems, queries, commands, input, export.", "mimeType": "text/markdown" }),
            json!({ "uri": "wish://docs/recipes", "name": "recipes", "title": "Game recipes", "description": "Working pieces of games (character, camera, pickups, checkpoints, moving platforms, interface, 2D, tests): code and scene operations that are tested to work.", "mimeType": "text/markdown" }),
            json!({ "uri": "wish://project/status", "name": "status", "title": "Project status", "description": "The project's current overview (the project_info answer).", "mimeType": "application/json" }),
        ];
        for name in self.type_names() {
            resources.push(json!({
                "uri": format!("wish://docs/components/{name}"),
                "name": name,
                "title": format!("{name} type"),
                "mimeType": "text/markdown"
            }));
        }
        json!({ "resources": resources })
    }

    fn read_resource(&mut self, uri: &str) -> Result<(&'static str, String)> {
        if uri == "wish://docs/guide" {
            return Ok(("text/markdown", docs::guide()));
        }
        if uri == "wish://docs/sdk" {
            return Ok(("text/markdown", docs::sdk_page()));
        }
        if uri == "wish://docs/recipes" {
            return Ok(("text/markdown", docs::recipes_page().to_owned()));
        }
        if uri == "wish://project/status" {
            let answer =
                self.workspace
                    .borrow_mut()
                    .call("project_info", &Value::Null, &self.actor);
            return Ok(("application/json", answer.to_string()));
        }
        if let Some(name) = uri.strip_prefix("wish://docs/components/") {
            if let Some(page) = docs::type_page(self.workspace.borrow().registry(), name) {
                return Ok(("text/markdown", page));
            }
            let names = self.type_names();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            return Err(Diagnostic::error(
                "unknown_type",
                format!("There is no type '{name}'."),
                wish_core::suggest::unknown_name_hint(name, &refs),
            )
            .with_candidates(refs));
        }
        if let Some(code) = uri.strip_prefix("wish://docs/diagnostics/") {
            return docs::diagnostic_page(code).map(|page| ("text/markdown", page)).ok_or_else(|| {
                Diagnostic::error(
                    "unknown_diagnostic",
                    format!("There is no page for the code '{code}'."),
                    "The diagnostic's own hint says how to fix it; docs_search finds related pages.",
                )
            });
        }
        Err(Diagnostic::error(
            "unknown_resource_uri",
            format!("There is no resource '{uri}'."),
            "resources/list and resources/templates/list show what exists, starting with wish://docs/guide.",
        ))
    }
}

/// The id of a request line (`None` for notifications and unreadable lines).
fn request_id(line: &str) -> Option<Value> {
    serde_json::from_str::<Value>(line).ok()?.get("id").cloned()
}

/// Serves MCP over `input` / `output` until the input ends.
///
/// Input is read on its own thread, so that a `notifications/cancelled` for
/// the call being worked on reaches it while it runs: the cancel flag stops
/// a long `sim_step` at the next tick boundary. Lines are read as bytes
/// (invalid UTF-8 becomes a parse error for that line, not the end of the
/// server) and a leading byte-order mark is dropped.
pub fn serve<W: BorrowMut<Workspace>>(
    server: &mut Server<W>,
    mut input: impl BufRead + Send + 'static,
    mut output: impl Write,
) -> std::io::Result<()> {
    let flag = Arc::clone(&server.busy.flag);
    server.workspace().set_cancel_flag(flag);
    let (lines, received) = mpsc::channel::<String>();
    let busy = Arc::clone(&server.busy);
    let reader = std::thread::spawn(move || -> std::io::Result<()> {
        let mut first = true;
        loop {
            let mut bytes = Vec::new();
            if input.read_until(b'\n', &mut bytes)? == 0 {
                return Ok(());
            }
            let mut line = String::from_utf8_lossy(&bytes).into_owned();
            if first {
                line = line.trim_start_matches('\u{feff}').to_owned();
                first = false;
            }
            if let Ok(message) = serde_json::from_str::<Value>(&line)
                && message.get("method").and_then(Value::as_str) == Some("notifications/cancelled")
                && let Some(wanted) = message.get("params").and_then(|p| p.get("requestId"))
            {
                busy.cancel("", wanted);
            }
            if lines.send(line).is_err() {
                return Ok(());
            }
        }
    });
    for line in received {
        server.busy.start("", request_id(&line).as_ref());
        let answer = server.handle(&line);
        server.busy.finish();
        if let Some(answer) = answer {
            output.write_all(answer.as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    reader.join().unwrap_or(Ok(()))
}
