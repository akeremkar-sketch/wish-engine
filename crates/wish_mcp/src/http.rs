//! MCP over Streamable HTTP (protocol 2025-06-18), for clients that connect
//! to a running engine instead of starting `wen mcp` themselves (M8a WP8).
//!
//! One endpoint, `/mcp`:
//! - `POST` a JSON-RPC message: a request is answered with `200` and the
//!   answer as `application/json`; a notification or a response with `202`
//!   (at once, even while a long call runs).
//! - `DELETE` with the session's header ends the session.
//! - `GET` (a stream the server would start) is not offered: `405`.
//!
//! The answer to `initialize` carries an `Mcp-Session-Id`; every later
//! message must carry it (`400` without, `404` for an unknown or ended one).
//! Each session keeps its own client name, so the history says which client
//! made a change, and its own request ids (a cancellation stops only its
//! own session's call).
//!
//! Safety (the specification's own advice, and ADR-022): the server listens
//! on the loopback address unless told otherwise; `wen mcp --http` always
//! asks for a token (`Authorization: Bearer ...`; one is made up and shown
//! if none is set); a browser page from another site is refused by its
//! `Origin`, and a request for another host name by its `Host` (DNS
//! rebinding); the headers are checked before the body is read; requests,
//! headers, the time a request may take, connections, waiting calls and
//! sessions are all limited.
//!
//! No async runtime: `std::net`, one thread per connection. Calls run one at
//! a time on the caller's thread (the workspace is not shared between
//! threads), in the order they arrive.

use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

use std::borrow::BorrowMut;

use wish_control::Workspace;

use crate::{Busy, SUPPORTED_VERSIONS, Server};

/// The endpoint's path.
pub const ENDPOINT: &str = "/mcp";
/// The largest message a client may send.
pub const MAX_BODY: usize = 8 * 1024 * 1024;
/// The largest request head (request line and headers).
pub const MAX_HEAD: usize = 64 * 1024;
/// How many connections are served at once; more are refused with `503`.
pub const MAX_CONNECTIONS: usize = 64;
/// How many calls may wait for the one running; more are refused with
/// `503` (so a cancellation always gets through).
pub const MAX_WAITING: usize = 16;
/// How many sessions are kept; the oldest ends when one more begins.
pub const MAX_SESSIONS: usize = 32;
/// How long a client may take to send one whole request.
pub const REQUEST_TIME: Duration = Duration::from_secs(30);

/// How the HTTP server is set up.
#[derive(Clone, Debug, Default)]
pub struct HttpOptions {
    /// A token clients must send as `Authorization: Bearer <token>`.
    pub token: Option<String>,
    /// Origins (such as `https://example.com`) allowed besides this
    /// machine's own (`localhost`, `127.0.0.1`, `[::1]`).
    pub allowed_origins: Vec<String>,
    /// How long one request may take to arrive ([`REQUEST_TIME`] if none).
    pub request_time: Option<Duration>,
}

/// Reads an address to listen on (`127.0.0.1:7878`, `localhost:0`, `7878`)
/// and checks that it is safe with these options: beyond the loopback
/// address only with a token.
pub fn listen_address(text: &str, options: &HttpOptions) -> Result<SocketAddr> {
    let text = text.trim();
    let full = if text.chars().all(|c| c.is_ascii_digit()) && !text.is_empty() {
        format!("127.0.0.1:{text}")
    } else {
        text.replacen("localhost:", "127.0.0.1:", 1)
    };
    let address: SocketAddr = full.parse().map_err(|_| {
        Diagnostic::error(
            "invalid_http_address",
            format!("'{text}' is not an address to listen on."),
            "Write a port (7878) or an address and port (127.0.0.1:7878; 0 picks a free port).",
        )
    })?;
    if !address.ip().is_loopback() && options.token.as_deref().is_none_or(str::is_empty) {
        return Err(Diagnostic::error(
            "http_needs_token",
            format!("Listening on {address} lets other machines call the engine's tools."),
            "Set WISH_MCP_TOKEN to a secret that clients send as 'Authorization: Bearer <token>', or listen on 127.0.0.1.",
        ));
    }
    Ok(address)
}

/// A secret no other program can guess: 128 bits from the system's random
/// source (through the standard library's randomly keyed hasher).
pub fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    (0..2u64)
        .map(|part| {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u64(part);
            hasher.write_u128(nanos);
            format!("{:016x}", hasher.finish())
        })
        .collect()
}

/// A message for the thread that owns the workspace.
struct Job {
    body: String,
    id: Option<Value>,
    session: String,
    reply: mpsc::Sender<Option<String>>,
}

/// What the connection threads share.
struct Shared {
    options: HttpOptions,
    /// Whether the server listens on the loopback address only (then a
    /// request must be for this machine's own name).
    loopback: bool,
    /// The sessions, oldest first.
    sessions: Mutex<VecDeque<String>>,
    busy: Arc<Busy>,
    jobs: Mutex<mpsc::Sender<Job>>,
    open: AtomicUsize,
    waiting: AtomicUsize,
}

impl Shared {
    fn known(&self, session: &str) -> bool {
        self.sessions
            .lock()
            .is_ok_and(|all| all.iter().any(|s| s == session))
    }

    fn begin(&self, session: String) {
        if let Ok(mut all) = self.sessions.lock() {
            while all.len() >= MAX_SESSIONS {
                if let Some(old) = all.pop_front() {
                    self.busy.forget(&old);
                }
            }
            all.push_back(session);
        }
    }

    fn end(&self, session: &str) -> bool {
        let ended = self.sessions.lock().is_ok_and(|mut all| {
            let before = all.len();
            all.retain(|s| s != session);
            all.len() != before
        });
        if ended {
            self.busy.forget(session);
        }
        ended
    }
}

/// A parsed request.
struct Request {
    method: String,
    path: String,
    /// Header names in lower case.
    headers: Vec<(String, String)>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// An answer.
struct Response {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Response {
    fn empty(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn json(status: u16, body: String) -> Self {
        Self {
            status,
            headers: vec![("Content-Type", "application/json".to_owned())],
            body: body.into_bytes(),
        }
    }

    /// A refusal: a JSON-RPC error body that says why and what to do.
    fn refuse(status: u16, message: &str) -> Self {
        Self::json(
            status,
            json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32600, "message": message } })
                .to_string(),
        )
    }

    fn with(mut self, name: &'static str, value: String) -> Self {
        self.headers.push((name, value));
        self
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

fn write_response(stream: &mut TcpStream, response: &Response) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n",
        response.status,
        reason(response.status),
        response.body.len()
    );
    for (name, value) in &response.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

/// Reads into `data` until the deadline: `Ok(false)` when the client
/// closed, `Err` with the refusal when the time is up or the read failed.
fn read_more(
    stream: &mut TcpStream,
    data: &mut Vec<u8>,
    deadline: Instant,
) -> std::result::Result<bool, Response> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
        return Err(Response::refuse(408, "The request took too long."));
    }
    let mut chunk = [0u8; 8192];
    match stream.read(&mut chunk) {
        Ok(0) => Ok(false),
        Ok(n) => {
            data.extend_from_slice(&chunk[..n]);
            Ok(true)
        }
        Err(_) => Err(Response::refuse(408, "The request took too long.")),
    }
}

/// Reads the request line and headers (within the limits); returns them and
/// whatever of the body came with them.
fn read_head(
    stream: &mut TcpStream,
    deadline: Instant,
) -> std::result::Result<(Request, Vec<u8>), Response> {
    let mut data = Vec::with_capacity(4096);
    let mut scanned = 0;
    let head_end = loop {
        if let Some(at) = data[scanned..].windows(4).position(|w| w == b"\r\n\r\n") {
            break scanned + at;
        }
        scanned = data.len().saturating_sub(3);
        if data.len() > MAX_HEAD {
            return Err(Response::refuse(
                431,
                "The request's headers are too large.",
            ));
        }
        if !read_more(stream, &mut data, deadline)? {
            return Err(Response::refuse(400, "The request ended early."));
        }
    };
    let head = String::from_utf8_lossy(&data[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let method = first.next().unwrap_or("").to_owned();
    let target = first.next().unwrap_or("").to_owned();
    if method.is_empty() || !target.starts_with('/') {
        return Err(Response::refuse(400, "Not an HTTP request."));
    }
    let path = target.split('?').next().unwrap_or("").to_owned();
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(Response::refuse(400, "A header line has no ':'."));
        };
        if name.is_empty() || name.ends_with([' ', '\t']) {
            return Err(Response::refuse(400, "A header name ends with a space."));
        }
        let name = name.to_ascii_lowercase();
        if (name == "content-length" || name == "transfer-encoding" || name == "host")
            && headers.iter().any(|(known, _)| *known == name)
        {
            return Err(Response::refuse(
                400,
                "A length or Host header is given twice.",
            ));
        }
        headers.push((name, value.trim().to_owned()));
    }
    let rest = data.split_off(head_end + 4);
    Ok((
        Request {
            method,
            path,
            headers,
        },
        rest,
    ))
}

/// Reads the body after the head: `Content-Length` bytes, in time.
fn read_body(
    stream: &mut TcpStream,
    request: &Request,
    mut body: Vec<u8>,
    deadline: Instant,
) -> std::result::Result<Vec<u8>, Response> {
    if request
        .header("transfer-encoding")
        .is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
    {
        return Err(Response::refuse(
            501,
            "Chunked requests are not supported: send Content-Length.",
        ));
    }
    let length = match request.header("content-length") {
        Some(value) if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
            value.parse::<usize>().unwrap_or(usize::MAX)
        }
        Some(_) => return Err(Response::refuse(400, "Content-Length is not a number.")),
        None => return Err(Response::refuse(411, "A POST needs a Content-Length.")),
    };
    if length > MAX_BODY {
        return Err(Response::refuse(
            413,
            "The message is too large (8 MiB at most).",
        ));
    }
    body.reserve(length.saturating_sub(body.len()));
    while body.len() < length {
        if !read_more(stream, &mut body, deadline)? {
            return Err(Response::refuse(400, "The message ended early."));
        }
    }
    body.truncate(length);
    Ok(body)
}

/// The host of an origin or a `Host` header, without scheme or port, in
/// lower case (`[::1]` keeps its brackets).
fn host_of(text: &str) -> Option<String> {
    let rest = text
        .strip_prefix("http://")
        .or_else(|| text.strip_prefix("https://"))
        .unwrap_or(text);
    let host = if let Some(v6) = rest.strip_prefix('[') {
        v6.split(']').next().map(|h| format!("[{h}]"))
    } else {
        rest.split([':', '/']).next().map(str::to_owned)
    };
    host.map(|h| h.to_ascii_lowercase())
        .filter(|h| !h.is_empty() && !h.contains('@'))
}

fn is_local(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// Whether a browser page from `origin` may call: this machine's own pages,
/// and the ones the options allow.
fn origin_allowed(origin: &str, options: &HttpOptions) -> bool {
    if options
        .allowed_origins
        .iter()
        .any(|allowed| allowed.trim_end_matches('/') == origin.trim_end_matches('/'))
    {
        return true;
    }
    (origin.starts_with("http://") || origin.starts_with("https://"))
        && !origin.contains('@')
        && host_of(origin).is_some_and(|host| is_local(&host))
}

/// The two tokens compare equal, in a time that does not depend on where
/// they differ.
fn same_token(sent: &str, expected: &str) -> bool {
    let (a, b) = (sent.as_bytes(), expected.as_bytes());
    let mut differ = a.len() ^ b.len();
    for (index, byte) in b.iter().enumerate() {
        differ |= usize::from(a.get(index).copied().unwrap_or(0) ^ byte);
    }
    differ == 0
}

/// The checks that need only the headers (made before the body is read).
fn check_head(shared: &Shared, request: &Request) -> Option<Response> {
    if let Some(origin) = request.header("origin")
        && !origin_allowed(origin, &shared.options)
    {
        return Some(Response::refuse(
            403,
            "This origin may not call the engine (only pages of this machine, or ones the server allows).",
        ));
    }
    if shared.loopback
        && let Some(host) = request.header("host")
        && !host_of(host).is_some_and(|host| is_local(&host))
    {
        return Some(Response::refuse(
            403,
            "This server answers only requests for this machine (Host: localhost or 127.0.0.1).",
        ));
    }
    if let Some(token) = shared.options.token.as_deref().filter(|t| !t.is_empty()) {
        let sent = request
            .header("authorization")
            .and_then(|value| value.split_once(' '))
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
            .map(|(_, sent)| sent.trim());
        if !sent.is_some_and(|sent| same_token(sent, token)) {
            return Some(
                Response::refuse(
                    401,
                    "Send the server's token: Authorization: Bearer <token> (wen mcp --http shows it when it starts).",
                )
                .with("WWW-Authenticate", "Bearer".to_owned()),
            );
        }
    }
    if request.path != ENDPOINT && request.path != "/" {
        return Some(Response::refuse(404, "The MCP endpoint is /mcp."));
    }
    if let Some(version) = request.header("mcp-protocol-version")
        && !SUPPORTED_VERSIONS.contains(&version)
    {
        return Some(Response::refuse(
            400,
            &format!(
                "Protocol version '{version}' is not supported; use one of {}.",
                SUPPORTED_VERSIONS.join(", ")
            ),
        ));
    }
    match request.method.as_str() {
        "POST" | "DELETE" => {}
        _ => {
            return Some(
                Response::refuse(
                    405,
                    "Send messages with POST; this server does not open a stream with GET.",
                )
                .with("Allow", "POST, DELETE".to_owned()),
            );
        }
    }
    if request.method == "POST" {
        if let Some(kind) = request.header("content-type")
            && !kind.to_ascii_lowercase().starts_with("application/json")
        {
            return Some(Response::refuse(
                415,
                "Send the message as application/json.",
            ));
        }
        if let Some(accept) = request.header("accept") {
            let accept = accept.to_ascii_lowercase();
            if !accept.contains("application/json") && !accept.contains("*/*") {
                return Some(Response::refuse(
                    406,
                    "The answer is application/json: accept it.",
                ));
            }
        }
    }
    None
}

/// Answers one request (on its connection's thread).
fn answer(shared: &Shared, stream: &mut TcpStream, deadline: Instant) -> Response {
    let (request, early) = match read_head(stream, deadline) {
        Ok(read) => read,
        Err(refusal) => return refusal,
    };
    if let Some(refusal) = check_head(shared, &request) {
        return refusal;
    }
    let session = request.header("mcp-session-id").map(str::to_owned);
    if request.method == "DELETE" {
        return match session {
            Some(id) if shared.end(&id) => Response::empty(200),
            Some(_) => {
                Response::refuse(404, "No such session (it ended, or the server restarted).")
            }
            None => Response::refuse(400, "Say which session to end: Mcp-Session-Id."),
        };
    }
    let body = match read_body(stream, &request, early, deadline) {
        Ok(body) => body,
        Err(refusal) => return refusal,
    };
    let Ok(text) = String::from_utf8(body) else {
        return Response::refuse(400, "The message is not UTF-8.");
    };
    let message: Value = match serde_json::from_str(&text) {
        Ok(message) => message,
        Err(problem) => {
            return Response::json(
                400,
                json!({ "jsonrpc": "2.0", "id": null,
                    "error": { "code": -32700, "message": format!("Not valid JSON: {problem}.") } })
                .to_string(),
            );
        }
    };
    if !message.is_object() {
        return Response::refuse(
            400,
            "Send one JSON-RPC message (batches are not part of MCP 2025-06-18).",
        );
    }
    let method = message.get("method").and_then(Value::as_str);
    let id = message.get("id").cloned();
    let initializing = method == Some("initialize");
    let session = if initializing {
        // Made now, kept only if initialize works.
        random_token()
    } else {
        match session {
            Some(id) if shared.known(&id) => id,
            Some(_) => {
                return Response::refuse(
                    404,
                    "No such session (it ended, or the server restarted): initialize again.",
                );
            }
            None => {
                return Response::refuse(
                    400,
                    "Send the Mcp-Session-Id that initialize gave (or initialize first).",
                );
            }
        }
    };
    // A notification or a response needs no answer: accepted at once, even
    // while a long call runs. A cancellation is acted on now.
    if method.is_none() || id.is_none() {
        if method == Some("notifications/cancelled")
            && let Some(wanted) = message.get("params").and_then(|p| p.get("requestId"))
        {
            shared.busy.cancel(&session, wanted);
        }
        return Response::empty(202);
    }
    if shared.waiting.fetch_add(1, Ordering::Relaxed) >= MAX_WAITING {
        shared.waiting.fetch_sub(1, Ordering::Relaxed);
        return Response::refuse(
            503,
            "Too many calls are waiting for the one running; try again, or cancel it.",
        );
    }
    let (reply, answered) = mpsc::channel();
    let sent = shared.jobs.lock().ok().is_some_and(|jobs| {
        jobs.send(Job {
            body: text,
            id,
            session: session.clone(),
            reply,
        })
        .is_ok()
    });
    if !sent {
        shared.waiting.fetch_sub(1, Ordering::Relaxed);
        return Response::refuse(503, "The server is stopping.");
    }
    match answered.recv() {
        Ok(Some(body)) => {
            let worked = initializing
                && serde_json::from_str::<Value>(&body)
                    .is_ok_and(|answer| answer.get("result").is_some());
            let response = Response::json(200, body);
            if worked {
                shared.begin(session.clone());
                return response.with("Mcp-Session-Id", session);
            }
            response
        }
        Ok(None) => Response::empty(202),
        Err(_) => Response::refuse(503, "The server is stopping."),
    }
}

/// Gives a connection's place back however its thread ends.
struct Slot(Arc<Shared>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.open.fetch_sub(1, Ordering::Relaxed);
    }
}

fn connection(shared: &Shared, mut stream: TcpStream) {
    // An accepted socket may inherit the listener's non-blocking mode.
    let _ = stream.set_nonblocking(false);
    let time = shared.options.request_time.unwrap_or(REQUEST_TIME);
    let _ = stream.set_write_timeout(Some(time));
    let response = answer(shared, &mut stream, Instant::now() + time);
    let _ = write_response(&mut stream, &response);
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

/// The client name and protocol version each session gave at `initialize`.
type SessionState = BTreeMap<String, (Actor, Option<String>)>;

/// An MCP endpoint over Streamable HTTP whose calls run wherever its
/// owner wants: [`serve_http`] runs them on its own loop; the editor runs
/// them in its frame, against its own workspace (so an agent's changes are
/// in the editor's one history, at once). Connections are read and answered
/// on their own threads. Dropping it stops listening.
pub struct Endpoint {
    shared: Arc<Shared>,
    received: mpsc::Receiver<Job>,
    stop: Arc<AtomicBool>,
    acceptor: Option<std::thread::JoinHandle<()>>,
    /// The client name and protocol version of each session.
    sessions: SessionState,
    address: SocketAddr,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl Endpoint {
    /// Starts listening on `listener`.
    pub fn start(listener: TcpListener, options: HttpOptions) -> std::io::Result<Self> {
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let (jobs, received) = mpsc::channel::<Job>();
        let shared = Arc::new(Shared {
            options,
            loopback: address.ip().is_loopback(),
            sessions: Mutex::new(VecDeque::new()),
            busy: Arc::new(Busy::new()),
            jobs: Mutex::new(jobs),
            open: AtomicUsize::new(0),
            waiting: AtomicUsize::new(0),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let accept_shared = Arc::clone(&shared);
        let accept_stop = Arc::clone(&stop);
        let acceptor = std::thread::spawn(move || {
            wish_control::crash::thread(move || {
                while !accept_stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            if accept_shared.open.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
                                let _ = stream.set_nonblocking(false);
                                let _ = write_response(
                                    &mut stream,
                                    &Response::refuse(503, "Too many connections at once."),
                                );
                                continue;
                            }
                            accept_shared.open.fetch_add(1, Ordering::Relaxed);
                            let slot = Slot(Arc::clone(&accept_shared));
                            let _ = std::thread::Builder::new()
                                .name("wish-mcp-http".to_owned())
                                .spawn(move || {
                                    wish_control::crash::thread(move || {
                                        connection(&slot.0, stream);
                                        drop(slot);
                                    })
                                });
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(10)),
                    }
                }
            })
        });
        Ok(Self {
            shared,
            received,
            stop,
            acceptor: Some(acceptor),
            sessions: SessionState::new(),
            address,
        })
    }

    /// Where it listens.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Runs the calls that are waiting (the first one waited for up to
    /// `wait`), against `server`'s workspace; returns how many ran.
    pub fn serve_pending<W: BorrowMut<Workspace>>(
        &mut self,
        server: &mut Server<W>,
        wait: Duration,
    ) -> usize {
        server
            .workspace()
            .set_cancel_flag(Arc::clone(&self.shared.busy.flag));
        let default_actor = server.actor.clone();
        let mut ran = 0;
        let mut next = if wait.is_zero() {
            self.received.try_recv().ok()
        } else {
            self.received.recv_timeout(wait).ok()
        };
        while let Some(job) = next {
            // This session's client, not the last one to initialize.
            let (actor, protocol) = self
                .sessions
                .get(&job.session)
                .cloned()
                .unwrap_or_else(|| (default_actor.clone(), None));
            server.actor = actor;
            server.protocol = protocol;
            self.shared.busy.start(&job.session, job.id.as_ref());
            let answer = server.handle(&job.body);
            self.shared.busy.finish();
            self.sessions.insert(
                job.session.clone(),
                (server.actor.clone(), server.protocol.clone()),
            );
            // Ended sessions are forgotten here too.
            if self.sessions.len() > MAX_SESSIONS
                && let Ok(live) = self.shared.sessions.lock()
            {
                self.sessions
                    .retain(|id, _| *id == job.session || live.contains(id));
            }
            self.shared.waiting.fetch_sub(1, Ordering::Relaxed);
            let _ = job.reply.send(answer);
            ran += 1;
            next = self.received.try_recv().ok();
        }
        server.actor = default_actor;
        ran
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(acceptor) = self.acceptor.take() {
            let _ = acceptor.join();
        }
    }
}

/// Serves MCP over HTTP on `listener` until `stop` is set. The calls run on
/// this thread, one at a time; connections are read and answered on their
/// own threads.
pub fn serve_http<W: BorrowMut<Workspace>>(
    server: &mut Server<W>,
    listener: TcpListener,
    options: HttpOptions,
    stop: Arc<AtomicBool>,
) -> std::io::Result<()> {
    let mut endpoint = Endpoint::start(listener, options)?;
    while !stop.load(Ordering::Relaxed) {
        endpoint.serve_pending(server, Duration::from_millis(50));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_of_this_machine_are_allowed() {
        let options = HttpOptions {
            allowed_origins: vec!["https://tools.example".to_owned()],
            ..HttpOptions::default()
        };
        for good in [
            "http://localhost:6274",
            "http://127.0.0.1",
            "https://[::1]:8080",
            "https://tools.example/",
        ] {
            assert!(origin_allowed(good, &options), "{good}");
        }
        for bad in [
            "http://evil.example",
            "http://localhost.evil.example",
            "http://evil@localhost",
            "http://localhost@evil.example",
            "null",
            "file://",
            "http://127.0.0.1.nip.io",
        ] {
            assert!(!origin_allowed(bad, &options), "{bad}");
        }
    }

    #[test]
    fn host_names_are_read_without_port() {
        assert_eq!(host_of("localhost:7878").as_deref(), Some("localhost"));
        assert_eq!(host_of("[::1]:7878").as_deref(), Some("[::1]"));
        assert_eq!(host_of("Evil.Example").as_deref(), Some("evil.example"));
        assert_eq!(host_of(""), None);
    }

    #[test]
    fn tokens_compare_whole() {
        assert!(same_token("abc", "abc"));
        assert!(!same_token("abd", "abc"));
        assert!(!same_token("ab", "abc"));
        assert!(!same_token("abcd", "abc"));
        assert!(!same_token("", "abc"));
        let (a, b) = (random_token(), random_token());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn addresses_beyond_this_machine_need_a_token() {
        let none = HttpOptions::default();
        assert_eq!(
            listen_address("7878", &none).unwrap().to_string(),
            "127.0.0.1:7878"
        );
        assert!(listen_address("localhost:0", &none).is_ok());
        assert!(listen_address("[::1]:9000", &none).is_ok());
        let wide = listen_address("0.0.0.0:7878", &none).unwrap_err();
        assert_eq!(wide.code, "http_needs_token");
        assert!(!wide.hint.is_empty());
        let with_token = HttpOptions {
            token: Some("s3cret".to_owned()),
            ..HttpOptions::default()
        };
        assert!(listen_address("0.0.0.0:7878", &with_token).is_ok());
        assert_eq!(
            listen_address("nowhere", &none).unwrap_err().code,
            "invalid_http_address"
        );
    }
}
