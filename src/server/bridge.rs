//! The link between a running app and the MCP server process.
//!
//! The app listens on a Unix socket (`$XDG_RUNTIME_DIR/spatialcoding/<pid>.sock`).
//! `spatialcoding --mcp` is a separate process an agent starts over stdio; it
//! connects to the newest live socket and forwards each tool call as one
//! JSON line (`{"tool": …, "args": …}` answered by `{"ok": …}` or
//! `{"error": …}`). A connection may first say who it is
//! (`{"hello": "claude-code"}`), so the Local Server panel can list agents.
//! Calls reach the UI thread through a channel and are answered by
//! [`super::api::handle`] on the next frame.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

/// How long a call waits for the UI to answer.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// A tool call waiting for the UI thread.
pub struct Call {
    pub tool: String,
    pub args: Value,
    pub reply: Sender<Result<Value, String>>,
}

/// Agents currently connected, by the name they gave.
pub type Agents = Arc<Mutex<Vec<(u64, String)>>>;

/// The directory sockets live in.
pub fn socket_dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("spatialcoding"),
        None => std::env::temp_dir().join(format!("spatialcoding-{}", std::env::var("USER").unwrap_or_else(|_| "user".into()))),
    }
}

/// The app side: a listening socket and the calls that arrived.
pub struct Bridge {
    rx: Receiver<Call>,
    path: PathBuf,
    agents: Agents,
}

impl Bridge {
    /// Starts listening in `dir` (created if missing). `repaint` wakes the UI
    /// when a call arrives.
    pub fn start(dir: &Path, repaint: egui::Context) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        let (tx, rx) = mpsc::channel();
        let agents: Agents = Arc::default();
        let shared = agents.clone();
        thread::Builder::new().name("mcp-bridge".into()).spawn(move || {
            for (index, stream) in listener.incoming().flatten().enumerate() {
                let (tx, repaint, agents) = (tx.clone(), repaint.clone(), shared.clone());
                let id = index as u64 + 1;
                let _ = thread::Builder::new().name("mcp-connection".into()).spawn(move || serve(stream, id, tx, repaint, agents));
            }
        })?;
        Ok(Self { rx, path, agents })
    }

    /// The calls that arrived since the last poll.
    pub fn poll(&self) -> Vec<Call> {
        self.rx.try_iter().collect()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Names of the connected agents.
    pub fn agents(&self) -> Vec<String> {
        self.agents.lock().map(|a| a.iter().map(|(_, n)| n.clone()).collect()).unwrap_or_default()
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// One connection: read lines, hand calls to the UI, write the answers.
fn serve(stream: UnixStream, id: u64, calls: Sender<Call>, repaint: egui::Context, agents: Agents) {
    let Ok(mut writer) = stream.try_clone() else { return };
    let reader = BufReader::new(stream);
    for line in reader.lines().map_while(Result::ok) {
        let Ok(request) = serde_json::from_str::<Value>(&line) else { continue };
        let answer = if let Some(name) = request.get("hello").and_then(Value::as_str) {
            if let Ok(mut list) = agents.lock() {
                list.retain(|(i, _)| *i != id);
                list.push((id, name.to_string()));
            }
            repaint.request_repaint();
            json!({"ok": null})
        } else {
            let (reply, wait) = mpsc::channel();
            let tool = request.get("tool").and_then(Value::as_str).unwrap_or_default().to_string();
            let args = request.get("args").cloned().unwrap_or(Value::Null);
            if calls.send(Call { tool, args, reply }).is_err() {
                break;
            }
            repaint.request_repaint();
            match wait.recv_timeout(CALL_TIMEOUT) {
                Ok(Ok(value)) => json!({"ok": value}),
                Ok(Err(e)) => json!({"error": e}),
                Err(_) => json!({"error": "the app did not answer in time"}),
            }
        };
        if writeln!(writer, "{answer}").is_err() {
            break;
        }
    }
    if let Ok(mut list) = agents.lock() {
        list.retain(|(i, _)| *i != id);
    }
    repaint.request_repaint();
}

/// The agent side: a connection to a running app.
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Client {
    /// Connects to the newest live socket of `dir`, removing stale ones.
    pub fn connect(dir: &Path) -> Result<Self, String> {
        let mut sockets: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
            .map_err(|_| "no SpatialCoding app is running (start the app first)".to_string())?
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "sock"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        sockets.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
        for (_, path) in sockets {
            match UnixStream::connect(&path) {
                Ok(stream) => {
                    let writer = stream.try_clone().map_err(|e| e.to_string())?;
                    return Ok(Self { reader: BufReader::new(stream), writer });
                }
                Err(_) => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        Err("no SpatialCoding app is running (start the app first)".into())
    }

    fn exchange(&mut self, request: &Value) -> Result<Value, String> {
        writeln!(self.writer, "{request}").map_err(|e| format!("lost the connection to the app: {e}"))?;
        let mut line = String::new();
        let read = self.reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("the app closed the connection".into());
        }
        let answer: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        match answer.get("error").and_then(Value::as_str) {
            Some(e) => Err(e.to_string()),
            None => Ok(answer.get("ok").cloned().unwrap_or(Value::Null)),
        }
    }

    /// Tells the app who is connected.
    pub fn hello(&mut self, name: &str) {
        let _ = self.exchange(&json!({"hello": name}));
    }

    pub fn call(&mut self, tool: &str, args: &Value) -> Result<Value, String> {
        self.exchange(&json!({"tool": tool, "args": args}))
    }
}
