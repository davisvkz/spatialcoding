//! The MCP server (`spatialcoding --mcp`): JSON-RPC 2.0 over stdio, one JSON
//! message per line, written by hand on serde_json (decision-8). It speaks
//! `initialize`, `ping`, `tools/list` and `tools/call`; every tool call is
//! forwarded to the running app through a [`Backend`].

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use super::api;

/// The protocol version answered when the client names none.
const DEFAULT_PROTOCOL: &str = "2024-11-05";

/// Where tool calls go: the running app.
pub trait Backend {
    fn hello(&mut self, client: &str);
    fn call(&mut self, tool: &str, args: &Value) -> Result<Value, String>;
}

/// Reads requests from `input` until it ends, writing the answers to `output`.
pub fn serve(input: impl BufRead, mut output: impl Write, backend: &mut dyn Backend) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            writeln!(output, "{}", error_reply(Value::Null, -32700, "parse error"))?;
            continue;
        };
        if let Some(reply) = respond(&message, backend) {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// The reply to one message; `None` for notifications.
pub fn respond(message: &Value, backend: &mut dyn Backend) -> Option<Value> {
    let method = message.get("method").and_then(Value::as_str)?;
    let id = message.get("id").cloned();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    // A message without an id is a notification: nothing to answer.
    let id = id?;
    let result = match method {
        "initialize" => {
            if let Some(name) = params.pointer("/clientInfo/name").and_then(Value::as_str) {
                backend.hello(name);
            }
            json!({
                "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "spatialcoding", "version": env!("CARGO_PKG_VERSION")},
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({
            "tools": api::tools().iter().map(|t| json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": (t.schema)(),
            })).collect::<Vec<_>>()
        }),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            if !api::tools().iter().any(|t| t.name == name) {
                return Some(error_reply(id, -32602, &format!("unknown tool {name}")));
            }
            match backend.call(name, &args) {
                Ok(value) => json!({"content": [{"type": "text", "text": render(&value)}], "isError": false}),
                Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
            }
        }
        _ => return Some(error_reply(id, -32601, &format!("method not found: {method}"))),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// A tool result as the text an agent reads: strings as they are, the rest
/// as pretty JSON.
fn render(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "ok".to_string(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// The real backend: a connection to the newest running app.
#[cfg(unix)]
pub struct AppBackend {
    dir: std::path::PathBuf,
    client: Option<super::bridge::Client>,
    name: Option<String>,
}

#[cfg(unix)]
impl AppBackend {
    pub fn new(dir: std::path::PathBuf) -> Self {
        Self { dir, client: None, name: None }
    }

    fn client(&mut self) -> Result<&mut super::bridge::Client, String> {
        if self.client.is_none() {
            let mut client = super::bridge::Client::connect(&self.dir)?;
            if let Some(name) = &self.name {
                client.hello(name);
            }
            self.client = Some(client);
        }
        Ok(self.client.as_mut().expect("connected above"))
    }
}

#[cfg(unix)]
impl Backend for AppBackend {
    fn hello(&mut self, client: &str) {
        self.name = Some(client.to_string());
        if let Ok(c) = self.client() {
            c.hello(client);
        }
    }

    fn call(&mut self, tool: &str, args: &Value) -> Result<Value, String> {
        let result = self.client()?.call(tool, args);
        if result.is_err() && self.client.is_some() {
            // A lost connection is retried once, against the newest app.
            self.client = None;
            if let Ok(client) = self.client() {
                return client.call(tool, args);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        names: Vec<String>,
    }
    impl Backend for Fake {
        fn hello(&mut self, client: &str) {
            self.names.push(client.to_string());
        }
        fn call(&mut self, tool: &str, args: &Value) -> Result<Value, String> {
            match tool {
                "read_docs" => Ok(json!("guide text")),
                "query" => Ok(json!({"nodes": 2, "args": args})),
                _ => Err("boom".into()),
            }
        }
    }

    fn talk(requests: &[Value]) -> (Vec<Value>, Fake) {
        let input: String = requests.iter().map(|r| format!("{r}\n")).collect();
        let mut out = Vec::new();
        let mut fake = Fake { names: Vec::new() };
        serve(input.as_bytes(), &mut out, &mut fake).unwrap();
        let replies = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        (replies, fake)
    }

    #[test]
    fn initialize_names_the_server_the_tools_and_remembers_the_client() {
        let (replies, fake) = talk(&[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26", "clientInfo": {"name": "claude-code"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        ]);
        assert_eq!(replies.len(), 2, "the notification gets no reply");
        assert_eq!(replies[0]["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(replies[0]["result"]["serverInfo"]["name"], "spatialcoding");
        assert!(replies[0]["result"]["capabilities"]["tools"].is_object());
        assert_eq!(fake.names, ["claude-code"]);
        let tools = replies[1]["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert_eq!(names, ["list_commands", "use_command", "query", "search", "lsp_servers", "read_docs"]);
        assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
    }

    #[test]
    fn tools_call_forwards_to_the_backend_and_reports_errors_as_results() {
        let (replies, _) = talk(&[
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "read_docs", "arguments": {}}}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "query", "arguments": {"what": "nodes"}}}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "use_command", "arguments": {}}}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "nope"}}),
        ]);
        assert_eq!(replies[0]["result"]["content"][0]["text"], "guide text");
        assert_eq!(replies[0]["result"]["isError"], false);
        let text = replies[1]["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("\"nodes\": 2") && text.contains("what"), "{text}");
        assert_eq!(replies[2]["result"]["isError"], true);
        assert_eq!(replies[2]["result"]["content"][0]["text"], "boom");
        assert_eq!(replies[3]["error"]["code"], -32602, "an unknown tool is a protocol error");
    }

    #[test]
    fn ping_unknown_methods_and_bad_json_are_answered() {
        let (replies, _) = talk(&[
            json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "resources/list"}),
        ]);
        assert_eq!(replies[0]["result"], json!({}));
        assert_eq!(replies[1]["error"]["code"], -32601);
        let mut out = Vec::new();
        serve("not json\n".as_bytes(), &mut out, &mut Fake { names: vec![] }).unwrap();
        let reply: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(reply["error"]["code"], -32700);
    }
}
