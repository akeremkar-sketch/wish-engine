//! MCP over Streamable HTTP (M8a WP8): sessions, refusals that say why,
//! notifications, cancellation from another connection, the token.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_mcp::Server;
use wish_mcp::http::{HttpOptions, serve_http};

/// A running server on a free port; stopped when dropped.
struct Running {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn start(name: &str, options: HttpOptions) -> Running {
    let dir = std::env::temp_dir().join(format!("wish-mcp-http-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "H", "wen").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&stop);
    let thread = std::thread::spawn(move || {
        // The workspace lives on the server's own thread.
        let workspace = Workspace::open(WorkspaceOptions {
            root: dir.clone(),
            example: None,
        })
        .unwrap();
        let mut server = Server::new(workspace);
        serve_http(&mut server, listener, options, stopping).unwrap();
        drop(server);
        let _ = std::fs::remove_dir_all(&dir);
    });
    Running {
        address,
        stop,
        thread: Some(thread),
    }
}

struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Answer {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|_| panic!("not JSON: {}", self.body))
    }
}

fn raw(address: SocketAddr, request: &[u8]) -> Answer {
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(request).unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    Answer {
        status,
        headers,
        body: body.to_owned(),
    }
}

fn send(address: SocketAddr, method: &str, headers: &[(&str, &str)], body: &str) -> Answer {
    let mut request = format!(
        "{method} /mcp HTTP/1.1\r\nHost: {address}\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    raw(address, request.as_bytes())
}

const JSON: (&str, &str) = ("Content-Type", "application/json");
const ACCEPT: (&str, &str) = ("Accept", "application/json, text/event-stream");

fn initialize(address: SocketAddr, extra: &[(&str, &str)]) -> (Answer, String) {
    let mut headers = vec![JSON, ACCEPT];
    headers.extend_from_slice(extra);
    let answer = send(
        address,
        "POST",
        &headers,
        &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" } } })
        .to_string(),
    );
    let session = answer.header("Mcp-Session-Id").unwrap_or("").to_owned();
    (answer, session)
}

fn call(address: SocketAddr, session: &str, id: u64, tool: &str, arguments: Value) -> Answer {
    send(
        address,
        "POST",
        &[
            JSON,
            ACCEPT,
            ("Mcp-Session-Id", session),
            ("MCP-Protocol-Version", "2025-06-18"),
        ],
        &json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": tool, "arguments": arguments } })
        .to_string(),
    )
}

#[test]
fn a_session_initializes_lists_tools_calls_one_and_ends() {
    let server = start("session", HttpOptions::default());
    let (init, session) = initialize(server.address, &[]);
    assert_eq!(init.status, 200, "{}", init.body);
    assert_eq!(init.header("Content-Type"), Some("application/json"));
    assert_eq!(init.json()["result"]["protocolVersion"], "2025-06-18");
    assert!(session.len() >= 16, "a session id: {:?}", init.headers);
    // The client says it is ready: a notification, accepted with no body.
    let ready = send(
        server.address,
        "POST",
        &[JSON, ACCEPT, ("Mcp-Session-Id", &session)],
        &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string(),
    );
    assert_eq!(ready.status, 202);
    assert!(ready.body.is_empty());
    let tools = send(
        server.address,
        "POST",
        &[JSON, ACCEPT, ("Mcp-Session-Id", &session)],
        &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }).to_string(),
    );
    assert_eq!(tools.status, 200);
    assert_eq!(
        tools.json()["result"]["tools"].as_array().unwrap().len(),
        32
    );
    let info = call(server.address, &session, 3, "project_info", json!({}));
    assert_eq!(info.status, 200);
    let content = &info.json()["result"]["structuredContent"];
    assert_eq!(content["project"]["name"], "H", "{content}");
    // Without the session, with a wrong one, after it ended.
    let without = send(
        server.address,
        "POST",
        &[JSON, ACCEPT],
        &json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list" }).to_string(),
    );
    assert_eq!(without.status, 400);
    assert!(
        without.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Mcp-Session-Id")
    );
    let wrong = call(server.address, "nope", 5, "project_info", json!({}));
    assert_eq!(wrong.status, 404);
    let ended = send(
        server.address,
        "DELETE",
        &[("Mcp-Session-Id", &session)],
        "",
    );
    assert_eq!(ended.status, 200);
    let after = call(server.address, &session, 6, "project_info", json!({}));
    assert_eq!(after.status, 404, "{}", after.body);
}

#[test]
fn refusals_say_why() {
    let server = start("refusals", HttpOptions::default());
    let (_, session) = initialize(server.address, &[]);
    // A page from another site (DNS rebinding) is refused; this machine's
    // own pages are not.
    let (evil, _) = initialize(server.address, &[("Origin", "http://evil.example")]);
    assert_eq!(evil.status, 403);
    let (local, _) = initialize(server.address, &[("Origin", "http://localhost:6274")]);
    assert_eq!(local.status, 200);
    // No stream with GET.
    let get = raw(
        server.address,
        format!("GET /mcp HTTP/1.1\r\nHost: localhost\r\nMcp-Session-Id: {session}\r\n\r\n")
            .as_bytes(),
    );
    assert_eq!(get.status, 405);
    assert_eq!(get.header("Allow"), Some("POST, DELETE"));
    // Another path.
    let other = raw(
        server.address,
        b"POST /other HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}",
    );
    assert_eq!(other.status, 404);
    // Not JSON: a JSON-RPC parse error.
    let broken = send(
        server.address,
        "POST",
        &[JSON, ("Mcp-Session-Id", &session)],
        "{ nope",
    );
    assert_eq!(broken.status, 400);
    assert_eq!(broken.json()["error"]["code"], -32700);
    // Too large, without a length, an unknown protocol version, wrong type.
    let huge = raw(
        server.address,
        b"POST /mcp HTTP/1.1\r\nContent-Length: 99999999999\r\n\r\n",
    );
    assert_eq!(huge.status, 413);
    let unsized_ = raw(
        server.address,
        b"POST /mcp HTTP/1.1\r\nHost: localhost\r\n\r\n",
    );
    assert_eq!(unsized_.status, 411);
    let version = send(
        server.address,
        "POST",
        &[
            JSON,
            ("Mcp-Session-Id", &session),
            ("MCP-Protocol-Version", "1999-01-01"),
        ],
        &json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/list" }).to_string(),
    );
    assert_eq!(version.status, 400);
    let text = send(
        server.address,
        "POST",
        &[("Content-Type", "text/plain"), ("Mcp-Session-Id", &session)],
        "{}",
    );
    assert_eq!(text.status, 415);
    // Garbage instead of HTTP.
    let garbage = raw(server.address, b"hello\r\n\r\n");
    assert_eq!(garbage.status, 400);
    // The server still works.
    let info = call(server.address, &session, 10, "project_info", json!({}));
    assert_eq!(info.status, 200);
}

#[test]
fn a_long_step_is_cancelled_from_another_connection() {
    let server = start("cancel", HttpOptions::default());
    let (_, session) = initialize(server.address, &[]);
    let started = call(server.address, &session, 2, "sim_start", json!({}));
    assert_eq!(started.status, 200);
    let address = server.address;
    let owned = session.clone();
    let long = std::thread::spawn(move || {
        call(
            address,
            &owned,
            3,
            "sim_step",
            json!({ "frames": 1_000_000 }),
        )
    });
    // Cancelled from another connection while it runs (or, if the
    // cancellation arrives first, before it starts): either way it stops at
    // a tick boundary.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let cancelled = send(
        server.address,
        "POST",
        &[JSON, ("Mcp-Session-Id", &session)],
        &json!({ "jsonrpc": "2.0", "method": "notifications/cancelled",
            "params": { "requestId": 3 } })
        .to_string(),
    );
    assert_eq!(cancelled.status, 202);
    let answer = long.join().unwrap();
    assert_eq!(answer.status, 200);
    let step = &answer.json()["result"]["structuredContent"];
    assert_eq!(step["stopped_by"], "cancelled", "{step}");
    assert!(step["ticks_run"].as_u64().unwrap() < 1_000_000);
    // The next call runs in full.
    let next = call(
        server.address,
        &session,
        4,
        "sim_step",
        json!({ "frames": 5 }),
    );
    assert_eq!(next.json()["result"]["structuredContent"]["ticks_run"], 5);
}

#[test]
fn with_a_token_only_clients_that_send_it_are_served() {
    let server = start(
        "token",
        HttpOptions {
            token: Some("s3cret".to_owned()),
            ..HttpOptions::default()
        },
    );
    let (refused, _) = initialize(server.address, &[]);
    assert_eq!(refused.status, 401);
    assert_eq!(refused.header("WWW-Authenticate"), Some("Bearer"));
    let (wrong, _) = initialize(server.address, &[("Authorization", "Bearer guess")]);
    assert_eq!(wrong.status, 401);
    let (served, session) = initialize(server.address, &[("Authorization", "Bearer s3cret")]);
    assert_eq!(served.status, 200);
    assert!(!session.is_empty());
}

fn initialize_as(address: SocketAddr, client: &str) -> String {
    let answer = send(
        address,
        "POST",
        &[JSON, ACCEPT],
        &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": { "name": client, "version": "1" } } })
        .to_string(),
    );
    assert_eq!(answer.status, 200, "{}", answer.body);
    answer.header("Mcp-Session-Id").unwrap().to_owned()
}

/// M8a closing review: every session keeps its own client name, so the
/// history says which client made each change.
#[test]
fn each_session_edits_as_its_own_client() {
    let server = start("actors", HttpOptions::default());
    let first = initialize_as(server.address, "claude-code");
    let second = initialize_as(server.address, "inspector");
    let spawn = |session: &str, id: &str| {
        call(
            server.address,
            session,
            2,
            "edit_apply",
            json!({ "ops": [{ "op": "spawn", "id": id, "components": { "Transform": {} } }] }),
        )
    };
    assert_eq!(spawn(&first, "from_claude").status, 200);
    assert_eq!(spawn(&second, "from_inspector").status, 200);
    let history = call(
        server.address,
        &first,
        3,
        "edit_history",
        json!({ "action": "list" }),
    );
    let entries = history.json()["result"]["structuredContent"]["entries"].clone();
    let by = |entity: &str| {
        entries
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["entities"].to_string().contains(entity))
            .map(|e| e["actor"].as_str().unwrap().to_owned())
            .unwrap()
    };
    assert!(by("from_claude").contains("claude-code"), "{entries}");
    assert!(by("from_inspector").contains("inspector"), "{entries}");
}

/// A cancellation that arrives after its call finished does not stop a
/// later call that reuses the id; one session cannot stop another's call.
#[test]
fn cancellations_belong_to_their_session_and_do_not_go_stale() {
    let server = start("stale", HttpOptions::default());
    let a = initialize_as(server.address, "a");
    let b = initialize_as(server.address, "b");
    let cancel = |session: &str, id: u64| {
        send(
            server.address,
            "POST",
            &[JSON, ("Mcp-Session-Id", session)],
            &json!({ "jsonrpc": "2.0", "method": "notifications/cancelled",
                "params": { "requestId": id } })
            .to_string(),
        )
    };
    assert_eq!(
        call(server.address, &a, 7, "sim_start", json!({})).status,
        200
    );
    assert_eq!(
        call(server.address, &a, 8, "sim_step", json!({ "frames": 3 })).status,
        200
    );
    // Too late: 8 finished. The next call 8 runs in full.
    assert_eq!(cancel(&a, 8).status, 202);
    let again = call(server.address, &a, 8, "sim_step", json!({ "frames": 4 }));
    assert_eq!(again.json()["result"]["structuredContent"]["ticks_run"], 4);
    // B cancels "its" 9 before A sends 9: A's 9 still runs.
    assert_eq!(cancel(&b, 9).status, 202);
    let mine = call(server.address, &a, 9, "sim_step", json!({ "frames": 2 }));
    assert_eq!(mine.json()["result"]["structuredContent"]["ticks_run"], 2);
}

#[test]
fn the_host_batches_and_doubled_lengths_are_refused() {
    let server = start("host", HttpOptions::default());
    let session = initialize_as(server.address, "t");
    // DNS rebinding: a page of evil.example that resolves to 127.0.0.1
    // still says Host: evil.example.
    let body = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }).to_string();
    let rebound = raw(
        server.address,
        format!(
            "POST /mcp HTTP/1.1\r\nHost: evil.example:8080\r\nContent-Type: application/json\r\nMcp-Session-Id: {session}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
    assert_eq!(rebound.status, 403);
    let batch = send(
        server.address,
        "POST",
        &[JSON, ("Mcp-Session-Id", &session)],
        &json!([{ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }]).to_string(),
    );
    assert_eq!(batch.status, 400);
    let doubled = raw(
        server.address,
        b"POST /mcp HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 40\r\n\r\n{}",
    );
    assert_eq!(doubled.status, 400);
}

/// A request that trickles in byte by byte gets the time a whole request
/// may take, not that time per byte.
#[test]
fn a_slow_request_runs_out_of_time() {
    let server = start(
        "slow",
        HttpOptions {
            request_time: Some(std::time::Duration::from_millis(600)),
            ..HttpOptions::default()
        },
    );
    let mut stream = TcpStream::connect(server.address).unwrap();
    let started = std::time::Instant::now();
    let mut cut = false;
    for byte in
        b"POST /mcp HTTP/1.1\r\nHost: localhost\r\nX-Slow: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n"
    {
        if stream.write_all(&[*byte]).is_err() {
            cut = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    // The server answers 408 and closes (the client may see the answer or
    // only the closed connection, depending on the system).
    assert!(text.starts_with("HTTP/1.1 408") || cut, "{text}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
}

/// A notification is accepted at once while a long call runs.
#[test]
fn notifications_do_not_wait_for_a_running_call() {
    let server = start("notify", HttpOptions::default());
    let session = initialize_as(server.address, "n");
    assert_eq!(
        call(server.address, &session, 2, "sim_start", json!({})).status,
        200
    );
    let address = server.address;
    let owned = session.clone();
    let long = std::thread::spawn(move || {
        call(
            address,
            &owned,
            3,
            "sim_step",
            json!({ "frames": 1_000_000 }),
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    let started = std::time::Instant::now();
    let ready = send(
        server.address,
        "POST",
        &[JSON, ("Mcp-Session-Id", &session)],
        &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string(),
    );
    assert_eq!(ready.status, 202);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    // Stop the long one.
    send(
        server.address,
        "POST",
        &[JSON, ("Mcp-Session-Id", &session)],
        &json!({ "jsonrpc": "2.0", "method": "notifications/cancelled",
            "params": { "requestId": 3 } })
        .to_string(),
    );
    assert_eq!(long.join().unwrap().status, 200);
}
