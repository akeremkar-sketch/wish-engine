//! The MCP protocol, message by message: the handshake, listing and calling
//! tools, resources, prompts, and every kind of malformed message.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, catalog};
use wish_mcp::Server;

fn server(name: &str) -> (PathBuf, Server) {
    let dir = std::env::temp_dir().join(format!("wish-mcp-protocol-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "P", "wen").unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    (dir, Server::new(workspace))
}

fn send(server: &mut Server, message: Value) -> Value {
    let line = server
        .handle(&message.to_string())
        .expect("a request gets an answer");
    serde_json::from_str(&line).unwrap()
}

fn call(server: &mut Server, id: u64, tool: &str, arguments: Value) -> Value {
    send(
        server,
        json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": tool, "arguments": arguments } }),
    )
}

#[test]
fn the_handshake_negotiates_the_version_and_names_the_agent() {
    let (dir, mut server) = server("handshake");
    let answer = send(
        &mut server,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "claude-code", "version": "2" } } }),
    );
    assert_eq!(answer["id"], 1);
    let result = &answer["result"];
    assert_eq!(result["protocolVersion"], "2025-06-18");
    assert_eq!(result["serverInfo"]["name"], "wish-engine");
    assert!(result["capabilities"]["tools"].is_object());
    assert!(result["capabilities"]["resources"].is_object());
    assert!(result["capabilities"]["prompts"].is_object());
    assert!(
        result["instructions"]
            .as_str()
            .unwrap()
            .contains("project_info")
    );
    // Notifications get no answer.
    assert!(
        server
            .handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none()
    );
    assert!(
        server
            .handle(
                r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#
            )
            .is_none()
    );
    // The agent's edits carry its name.
    call(
        &mut server,
        2,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "x" }] }),
    );
    let history = call(&mut server, 3, "edit_history", json!({}));
    assert_eq!(
        history["result"]["structuredContent"]["entries"][0]["actor"],
        "agent:claude-code"
    );
    // An older version the client asks for is accepted; an unknown one gets ours.
    let older = send(
        &mut server,
        json!({ "jsonrpc": "2.0", "id": 4, "method": "initialize", "params": { "protocolVersion": "2024-11-05" } }),
    );
    assert_eq!(older["result"]["protocolVersion"], "2024-11-05");
    let future = send(
        &mut server,
        json!({ "jsonrpc": "2.0", "id": "x", "method": "initialize", "params": { "protocolVersion": "2099-01-01" } }),
    );
    assert_eq!(future["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(future["id"], "x", "string ids come back as they were");
    assert_eq!(
        send(
            &mut server,
            json!({ "jsonrpc": "2.0", "id": 5, "method": "ping" })
        )["result"],
        json!({})
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn every_catalogue_tool_is_listed_with_its_schema() {
    let (dir, mut server) = server("tools");
    let answer = send(
        &mut server,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
    );
    let tools = answer["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), catalog().len());
    assert_eq!(
        tools.len(),
        28,
        "20 tools of M4 (design note §5), script_build, replay and test_run (M5), physics_query (M6), asset_create (M7), project_settings (M8a)"
    );
    for (listed, tool) in tools.iter().zip(catalog()) {
        assert_eq!(listed["name"], tool.name);
        assert_eq!(listed["inputSchema"]["type"], "object");
        assert_eq!(listed["inputSchema"]["additionalProperties"], false);
        assert!(listed["description"].as_str().unwrap().len() > 40);
        assert!(
            listed["outputSchema"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("ok"))
        );
    }
    let read_only: Vec<&str> = tools
        .iter()
        .filter(|t| t["annotations"]["readOnlyHint"] == true)
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(read_only.contains(&"entity_query") && !read_only.contains(&"edit_apply"));
    let destructive: Vec<&str> = tools
        .iter()
        .filter(|t| t["annotations"]["destructiveHint"] == true)
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        destructive,
        ["sim_stop", "capture_frame", "checkpoint"],
        "catalogue order"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_tool_answer_is_structured_and_a_picture_is_an_image_block() {
    let (dir, mut server) = server("call");
    let info = call(&mut server, 1, "project_info", json!({}));
    let result = &info["result"];
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"]["ok"], true);
    let text: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        text, result["structuredContent"],
        "the text block is the same JSON"
    );
    // A mistake is a tool result with isError, not a protocol error, so the
    // agent reads the hint.
    let wrong = call(&mut server, 2, "schema_get", json!({ "nmaes": [] }));
    assert_eq!(wrong["result"]["isError"], true);
    assert_eq!(
        wrong["result"]["structuredContent"]["error"]["hint"],
        "Did you mean 'names'?"
    );
    // An unknown tool is a protocol error (MCP 2025-06-18), still with the
    // hint and the valid names.
    let unknown = call(&mut server, 3, "scene_gte", json!({}));
    assert_eq!(unknown["error"]["code"], -32602);
    assert_eq!(
        unknown["error"]["data"]["hint"],
        "Did you mean 'scene_get'?"
    );
    assert_eq!(unknown["error"]["data"]["code"], "unknown_tool");
    // A picture: an image block, and no base64 in the JSON.
    let picture = call(
        &mut server,
        4,
        "capture_frame",
        json!({ "width": 64, "height": 36 }),
    );
    let content = picture["result"]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2);
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["mimeType"], "image/png");
    assert!(
        content[1]["data"]
            .as_str()
            .unwrap()
            .starts_with("iVBORw0KGgo")
    );
    assert!(
        picture["result"]["structuredContent"]["image"]
            .get("data")
            .is_none()
    );
    assert_eq!(picture["result"]["structuredContent"]["image"]["width"], 64);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn resources_and_prompts() {
    let (dir, mut server) = server("resources");
    let list = send(
        &mut server,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/list" }),
    );
    let uris: Vec<&str> = list["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["uri"].as_str().unwrap())
        .collect();
    assert!(uris.contains(&"wish://docs/guide") && uris.contains(&"wish://docs/components/Camera"));
    let templates = send(
        &mut server,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/templates/list" }),
    );
    assert_eq!(
        templates["result"]["resourceTemplates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let read = |server: &mut Server, uri: &str| {
        send(
            server,
            json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/read", "params": { "uri": uri } }),
        )
    };
    let guide = read(&mut server, "wish://docs/guide");
    let text = guide["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("edit_apply") && text.contains("euler_deg"));
    assert!(
        text.len() < 12_000,
        "the guide must stay short: {} bytes",
        text.len()
    );
    let camera = read(&mut server, "wish://docs/components/Camera");
    assert!(
        camera["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("fov_y_deg")
    );
    let code = read(&mut server, "wish://docs/diagnostics/stale_revision");
    assert!(
        code["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("base_revision")
    );
    let status = read(&mut server, "wish://project/status");
    assert_eq!(
        status["result"]["contents"][0]["mimeType"],
        "application/json"
    );
    let missing = read(&mut server, "wish://docs/components/Camra");
    assert_eq!(missing["error"]["code"], -32002);
    assert_eq!(missing["error"]["data"]["hint"], "Did you mean 'Camera'?");
    // Every error code an answer can carry has its page.
    for (code, _) in wish_control::docs::DIAGNOSTICS {
        let page = read(&mut server, &format!("wish://docs/diagnostics/{code}"));
        assert!(page.get("result").is_some(), "{code}");
    }
    let prompts = send(
        &mut server,
        json!({ "jsonrpc": "2.0", "id": 4, "method": "prompts/list" }),
    );
    assert_eq!(prompts["result"]["prompts"].as_array().unwrap().len(), 2);
    let prompt = send(
        &mut server,
        json!({ "jsonrpc": "2.0", "id": 5, "method": "prompts/get", "params": { "name": "new_scene", "arguments": { "idea": "three platforms" } } }),
    );
    assert!(
        prompt["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("three platforms")
    );
    let no_prompt = send(
        &mut server,
        json!({ "jsonrpc": "2.0", "id": 6, "method": "prompts/get", "params": { "name": "x" } }),
    );
    assert_eq!(no_prompt["error"]["code"], -32602);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn malformed_messages_get_json_rpc_errors_and_the_server_stays_up() {
    let (dir, mut server) = server("errors");
    let parse = |server: &mut Server, line: &str| -> Value {
        serde_json::from_str(&server.handle(line).unwrap()).unwrap()
    };
    assert_eq!(parse(&mut server, "{ not json")["error"]["code"], -32700);
    assert_eq!(parse(&mut server, "[1, 2]")["error"]["code"], -32600);
    assert_eq!(
        parse(&mut server, r#"{"jsonrpc":"2.0","id":1}"#)["error"]["code"],
        -32600
    );
    let unknown = parse(
        &mut server,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/destroy"}"#,
    );
    assert_eq!(unknown["error"]["code"], -32601);
    assert_eq!(unknown["id"], 2);
    assert_eq!(
        parse(
            &mut server,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{}}"#
        )["error"]["code"],
        -32602
    );
    assert!(server.handle("").is_none() && server.handle("   ").is_none());
    // A response from the client to nothing: ignored.
    assert!(
        server
            .handle(r#"{"jsonrpc":"2.0","id":9,"result":{}}"#)
            .is_none()
    );
    // Still working.
    let info = call(&mut server, 4, "project_info", json!({}));
    assert_eq!(info["result"]["isError"], false);
    std::fs::remove_dir_all(&dir).unwrap();
}
