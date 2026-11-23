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

use std::io::{BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use super::mcp::{read_capped_line, Line, MAX_REQUEST_BYTES};

/// The longest answer the agent side reads from the app. The app is trusted
/// more than an agent, but a query over a big canvas stays far below this.
const MAX_REPLY_BYTES: usize = 64 * 1024 * 1024;

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

/// Creates `dir` for the user alone (mode 0700), or checks that an existing
/// one already is theirs: a real directory, not a link, owned by the current
/// user. The fallback `/tmp/spatialcoding-$USER` is a predictable name, so
/// another user could have made it first to read or spoof the app's socket.
fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    // SAFETY: `geteuid` takes no arguments and cannot fail.
    check_private_dir(&meta, unsafe { libc::geteuid() })?;
    if std::os::unix::fs::MetadataExt::mode(&meta) & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// A directory (not a link to one) owned by `euid`.
fn check_private_dir(meta: &std::fs::Metadata, euid: u32) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    if !meta.is_dir() {
        return Err(Error::new(ErrorKind::PermissionDenied, "the socket directory is not a plain directory"));
    }
    if std::os::unix::fs::MetadataExt::uid(meta) != euid {
        return Err(Error::new(ErrorKind::PermissionDenied, "the socket directory belongs to another user"));
    }
    Ok(())
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
        ensure_private_dir(dir)?;
        let path = dir.join(format!("{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
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
    let mut reader = BufReader::new(stream);
    loop {
        let line = match read_capped_line(&mut reader, MAX_REQUEST_BYTES) {
            Ok(Line::Text(line)) => line,
            Ok(Line::TooLong) => {
                if writeln!(writer, "{}", json!({"error": "request too large"})).is_err() {
                    break;
                }
                continue;
            }
            Ok(Line::Eof) | Err(_) => break,
        };
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
        let line = match read_capped_line(&mut self.reader, MAX_REPLY_BYTES).map_err(|e| e.to_string())? {
            Line::Text(line) => line,
            Line::Eof => return Err("the app closed the connection".into()),
            Line::TooLong => return Err("the app's answer is too large".into()),
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    #[test]
    fn the_socket_directory_is_created_private_and_a_loose_one_is_tightened() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("a").join("spatialcoding");
        ensure_private_dir(&dir).unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        ensure_private_dir(&dir).unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().mode() & 0o777, 0o700, "ours, so it is tightened");
    }

    #[test]
    fn a_link_or_a_directory_of_another_user_is_refused() {
        let base = tempfile::tempdir().unwrap();
        let real = base.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = base.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(ensure_private_dir(&link).is_err(), "a link could point into someone else's directory");

        let meta = std::fs::symlink_metadata(&real).unwrap();
        assert!(check_private_dir(&meta, meta.uid()).is_ok());
        let err = check_private_dir(&meta, meta.uid().wrapping_add(1)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        let file = base.path().join("file");
        std::fs::write(&file, "").unwrap();
        assert!(check_private_dir(&std::fs::symlink_metadata(&file).unwrap(), meta.uid()).is_err(), "a file");
    }

    #[test]
    fn the_listening_socket_is_for_the_user_alone() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("sockets");
        let bridge = Bridge::start(&dir, egui::Context::default()).unwrap();
        assert_eq!(std::fs::metadata(bridge.path()).unwrap().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
    }

    #[test]
    fn an_oversized_request_gets_an_error_and_the_connection_survives() {
        let (agent, app) = UnixStream::pair().unwrap();
        let (calls, _unused) = mpsc::channel();
        let server = thread::spawn(move || serve(app, 1, calls, egui::Context::default(), Agents::default()));

        let mut oversized = vec![b'x'; MAX_REQUEST_BYTES + 1];
        oversized.push(b'\n');
        let writer = {
            let mut agent = agent.try_clone().unwrap();
            thread::spawn(move || {
                agent.write_all(&oversized).unwrap();
                agent.write_all(b"{\"hello\": \"agent\"}\n").unwrap();
            })
        };
        let mut replies = BufReader::new(agent.try_clone().unwrap());
        let mut line = String::new();
        replies.read_line(&mut line).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["error"], "request too large");
        line.clear();
        replies.read_line(&mut line).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&line).unwrap(), json!({"ok": null}), "the next request is served");
        writer.join().unwrap();
        agent.shutdown(std::net::Shutdown::Both).unwrap();
        server.join().unwrap();
    }
}
