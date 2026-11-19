//! The MCP server against a running app: `spatialcoding --mcp` is a separate
//! process in real use; here its protocol loop runs on a thread and talks to
//! the app through the real Unix socket bridge.

#![cfg(unix)]

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use spatialcoding::server::{bridge, mcp};

use crate::common::*;

/// Runs `requests` through the MCP loop on a thread while the app steps
/// frames to answer the bridge. Returns the replies.
fn talk(app: &mut App, dir: &std::path::Path, requests: &[Value]) -> Vec<Value> {
    let input: String = requests.iter().map(|r| format!("{r}\n")).collect();
    let dir = dir.to_path_buf();
    let worker = std::thread::spawn(move || {
        let mut backend = mcp::AppBackend::new(dir);
        let mut out = Vec::new();
        mcp::serve(input.as_bytes(), &mut out, &mut backend).unwrap();
        String::from_utf8(out).unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !worker.is_finished() {
        assert!(Instant::now() < deadline, "the MCP loop did not finish");
        std::thread::sleep(Duration::from_millis(5));
        app.step();
    }
    worker.join().unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn call(id: u64, name: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": arguments}})
}

fn text(reply: &Value) -> String {
    reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[test]
fn an_agent_drives_the_canvas_and_pushes_a_note_through_the_socket() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let sockets = tempfile::tempdir().unwrap();
    app.state_mut().enable_bridge_in(sockets.path());

    let replies = talk(
        &mut app,
        sockets.path(),
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"clientInfo": {"name": "test-agent"}}}),
            call(2, "use_command", json!({"id": "canvas:openAll"})),
            call(3, "query", json!({"what": "canvas"})),
            call(4, "use_command", json!({"id": "agentNotes:push", "args": {"title": "Tour", "markdown": "Start at [main](cc://file/src/main.rs)."}})),
            call(5, "query", json!({"what": "nodes"})),
            call(6, "use_command", json!({"id": "layout:apply"})),
            call(7, "use_command", json!({"id": "nope:nope"})),
        ],
    );

    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "spatialcoding");
    assert!(text(&replies[1]).contains("opened"), "{}", text(&replies[1]));
    assert!(app.app_state().node_store.file_nodes.len() > 1, "the agent opened the files");
    let canvas: Value = serde_json::from_str(&text(&replies[2])).unwrap();
    assert!(canvas["nodes"].as_u64().unwrap() > 1);
    assert_eq!(app.app_state().notes.all().len(), 1, "the note arrived");
    let nodes: Value = serde_json::from_str(&text(&replies[4])).unwrap();
    assert!(nodes.as_array().unwrap().iter().any(|n| n["path"] == "src/main.rs"));
    assert_eq!(replies[5]["result"]["isError"], false);
    assert_eq!(replies[6]["result"]["isError"], true, "an unknown command is an error result");
}

#[test]
fn the_local_server_panel_lists_the_connected_agent_and_stale_sockets_are_removed() {
    let mut app = app();
    let sockets = tempfile::tempdir().unwrap();
    app.state_mut().enable_bridge_in(sockets.path());
    app.run();
    // A leftover socket from a dead app, newer than the live one.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(sockets.path().join("99999.sock"), b"").unwrap();
    assert_eq!(app.app_state().mcp_socket.as_deref().map(|p| p.parent().unwrap()), Some(sockets.path()));

    talk(&mut app, sockets.path(), &[json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"clientInfo": {"name": "agent-x"}}})]);
    assert!(!sockets.path().join("99999.sock").exists(), "a socket nobody listens on is removed");
    let _ = bridge::socket_dir();
}

#[test]
fn without_a_running_app_a_tool_call_explains_it() {
    let empty = tempfile::tempdir().unwrap();
    let mut backend = mcp::AppBackend::new(empty.path().to_path_buf());
    let reply = mcp::respond(&call(1, "query", json!({"what": "canvas"})), &mut backend).unwrap();
    assert_eq!(reply["result"]["isError"], true);
    assert!(text(&reply).contains("no SpatialCoding app is running"), "{}", text(&reply));
}
