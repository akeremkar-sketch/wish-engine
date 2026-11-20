//! A small MCP client over Streamable HTTP (std only), as the scripted
//! adapter talks to the editor's endpoint, and for tests.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{Value, json};

/// One MCP session with an endpoint.
#[derive(Debug)]
pub struct Client {
    address: String,
    token: String,
    session: String,
    next: u64,
}

/// `127.0.0.1:7878` from `http://127.0.0.1:7878/mcp`.
fn host_of(endpoint: &str) -> String {
    let rest = endpoint.strip_prefix("http://").unwrap_or(endpoint);
    rest.split('/').next().unwrap_or(rest).to_owned()
}

impl Client {
    /// Connects and says hello as `name`.
    pub fn connect(endpoint: &str, token: &str, name: &str) -> Result<Self, String> {
        let mut client = Self {
            address: host_of(endpoint),
            token: token.to_owned(),
            session: String::new(),
            next: 0,
        };
        let (answer, headers) = client.post(&json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": { "name": name, "version": "1" } }
        }))?;
        if answer.get("result").is_none() {
            return Err(format!("the editor did not accept the agent: {answer}"));
        }
        client.session = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("mcp-session-id"))
            .map(|(_, v)| v.clone())
            .ok_or("the editor gave no session")?;
        client.notify("notifications/initialized")?;
        Ok(client)
    }

    /// Calls a tool; returns its answer (the control plane's JSON).
    pub fn call(&mut self, tool: &str, arguments: &Value) -> Result<Value, String> {
        self.next += 1;
        let (answer, _) = self.post(&json!({
            "jsonrpc": "2.0", "id": self.next, "method": "tools/call",
            "params": { "name": tool, "arguments": arguments }
        }))?;
        Ok(answer["result"]["structuredContent"].clone())
    }

    fn notify(&mut self, method: &str) -> Result<(), String> {
        self.post(&json!({ "jsonrpc": "2.0", "method": method }))
            .map(|_| ())
    }

    fn post(&self, message: &Value) -> Result<(Value, Vec<(String, String)>), String> {
        let body = message.to_string();
        let mut stream = TcpStream::connect(&self.address)
            .map_err(|e| format!("cannot reach the editor: {e}"))?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(600)));
        let mut request = format!(
            "POST /mcp HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nAuthorization: Bearer {}\r\nContent-Length: {}\r\n",
            self.address,
            self.token,
            body.len()
        );
        if !self.session.is_empty() {
            request.push_str(&format!("Mcp-Session-Id: {}\r\n", self.session));
        }
        request.push_str("\r\n");
        request.push_str(&body);
        stream
            .write_all(request.as_bytes())
            .map_err(|e| format!("cannot talk to the editor: {e}"))?;
        let mut bytes = Vec::new();
        stream
            .read_to_end(&mut bytes)
            .map_err(|e| format!("no answer from the editor: {e}"))?;
        let text = String::from_utf8_lossy(&bytes);
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        let headers = head
            .split("\r\n")
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
            .collect();
        let answer = if body.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(body).map_err(|e| format!("a strange answer: {e}"))?
        };
        Ok((answer, headers))
    }
}
