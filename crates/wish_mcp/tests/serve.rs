//! The stdio loop: lines as bytes, a byte-order mark, and cancellation of a
//! running call (M4 closing review).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Cursor;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_mcp::{Server, serve};

fn run(name: &str, input: Vec<u8>) -> Vec<Value> {
    let dir = std::env::temp_dir().join(format!("wish-mcp-serve-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "S", "wen").unwrap();
    let workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    let mut server = Server::new(workspace);
    let mut output = Vec::new();
    serve(&mut server, Cursor::new(input), &mut output).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn line(message: &Value) -> Vec<u8> {
    let mut bytes = message.to_string().into_bytes();
    bytes.push(b'\n');
    bytes
}

fn call(id: u64, tool: &str, arguments: Value) -> Vec<u8> {
    line(
        &json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": tool, "arguments": arguments } }),
    )
}

#[test]
fn a_byte_order_mark_and_bad_bytes_do_not_stop_the_server() {
    let mut input = vec![0xEF, 0xBB, 0xBF];
    input.extend(line(
        &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }),
    ));
    input.extend(b"\xff\xfe{ not utf-8 \xc3\x28\n");
    input.extend(call(2, "project_info", json!({})));
    let answers = run("bytes", input);
    assert_eq!(answers.len(), 3, "{answers:?}");
    assert_eq!(answers[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(answers[1]["error"]["code"], -32700);
    assert_eq!(answers[2]["id"], 2);
    assert_eq!(answers[2]["result"]["isError"], false);
}

#[test]
fn a_cancelled_long_step_stops_and_the_next_call_runs() {
    let cancel = |id: u64| {
        line(
            &json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": id } }),
        )
    };
    let mut input = call(1, "sim_start", json!({}));
    // Cancelled while it runs (or, if the notification is read first, before
    // it starts): either way it stops at a tick boundary.
    input.extend(call(2, "sim_step", json!({ "frames": 1_000_000 })));
    input.extend(cancel(2));
    // Cancelled before it was read: it never runs a tick.
    input.extend(cancel(3));
    input.extend(call(3, "sim_step", json!({ "frames": 1_000_000 })));
    // A cancellation is not sticky: the next call runs in full.
    input.extend(call(4, "sim_step", json!({ "frames": 5 })));
    let answers = run("cancel", input);
    assert_eq!(answers.len(), 4, "{answers:?}");
    let step = |n: usize| &answers[n]["result"]["structuredContent"];
    assert_eq!(step(1)["stopped_by"], "cancelled", "{}", step(1));
    assert!(step(1)["ticks_run"].as_u64().unwrap() < 1_000_000);
    assert_eq!(step(2)["stopped_by"], "cancelled", "{}", step(2));
    assert_eq!(step(2)["ticks_run"], 0);
    assert_eq!(step(3)["ticks_run"], 5, "{}", step(3));
    assert_eq!(step(3)["stopped_by"], "frames");
}
