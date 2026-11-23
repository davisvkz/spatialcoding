//! A blocking LSP client over a server's stdio. Every call waits for the
//! server, so only worker threads use it (see [`super::manager`]).
//!
//! A reader thread routes responses to the waiting call, answers the requests
//! servers send to the client (`workspace/configuration`, progress creation,
//! capability registration) so they don't stall, and tracks `$/progress` so
//! the UI can say a server is still indexing.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::protocol::{
    parse_document_symbols, parse_locations, path_to_uri, position_json, read_message,
    write_message,
};
use super::{Location, LspSymbol, Position, ServerSettings};

/// JSON-RPC error codes that mean "ask again later".
const CONTENT_MODIFIED: i64 = -32801;
const SERVER_CANCELLED: i64 = -32802;
const MAX_RETRIES: u32 = 8;

/// How long one symbol-graph query may take in all: the requests go out
/// together and whatever has not answered by then is cancelled and left out.
pub const QUERY_DEADLINE: Duration = Duration::from_secs(30);
const STDERR_TAIL_BYTES: usize = 2000;

/// The pipes to a running server, and its process when there is one (tests
/// connect to an in-process fake instead).
pub struct Connection {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Option<Child>,
    /// The last lines the server wrote to stderr, for error messages.
    pub stderr: Option<StderrTail>,
}

/// Keeps the end of a server's stderr.
#[derive(Clone, Default)]
pub struct StderrTail(Arc<Mutex<String>>);

impl StderrTail {
    /// Drains `stream` on a thread, keeping the last few lines.
    pub fn capture(stream: impl Read + Send + 'static) -> Self {
        let tail = StderrTail::default();
        let shared = tail.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 1024];
            let mut stream = stream;
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut text = shared.0.lock().unwrap();
                text.push_str(&String::from_utf8_lossy(&buf[..n]));
                if text.len() > STDERR_TAIL_BYTES {
                    let mut cut = text.len() - STDERR_TAIL_BYTES;
                    while !text.is_char_boundary(cut) {
                        cut += 1;
                    }
                    text.drain(..cut);
                }
            }
        });
        tail
    }

    pub fn text(&self) -> String {
        self.0.lock().unwrap().trim().to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LspError {
    /// Writing to the server failed.
    Io(String),
    /// The server exited or closed its output.
    Closed(String),
    Timeout(String),
    /// The server answered with an error.
    Rpc { code: i64, message: String },
}

impl std::fmt::Display for LspError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LspError::Io(e) => write!(f, "Could not talk to the language server: {e}"),
            LspError::Closed(tail) if tail.is_empty() => write!(f, "The language server exited."),
            LspError::Closed(tail) => write!(f, "The language server exited: {tail}"),
            LspError::Timeout(method) => write!(f, "The language server did not answer {method} in time."),
            LspError::Rpc { code, message } => write!(f, "Language server error {code}: {message}"),
        }
    }
}

type Reply = Result<Value, LspError>;
/// Waiting requests by id. The reply carries its id, so several requests can
/// share one queue.
type Pending = Arc<Mutex<HashMap<u64, Sender<(u64, Reply)>>>>;
type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// Active `$/progress` tokens; non-empty while the server is indexing.
/// rust-analyzer's `experimental/serverStatus` adds [`NOT_QUIESCENT`] while it
/// loads or indexes the workspace, even between progress reports.
pub type ProgressTokens = Arc<Mutex<HashSet<String>>>;

/// Pseudo progress token held while a server reports `quiescent: false`.
pub const NOT_QUIESCENT: &str = "$serverStatus";

pub struct LspClient {
    writer: SharedWriter,
    pending: Pending,
    closed: Arc<AtomicBool>,
    progress: ProgressTokens,
    next_id: u64,
    /// Open documents: version and content hash.
    documents: HashMap<PathBuf, (i64, u64)>,
    child: Option<Child>,
    stderr: Option<StderrTail>,
    timeout: Duration,
}

impl LspClient {
    /// Connects and runs the `initialize` handshake for workspace `root`.
    pub fn start(conn: Connection, root: &Path, timeout: Duration) -> Result<Self, LspError> {
        Self::start_with(conn, root, timeout, &ServerSettings::default())
    }

    /// [`Self::start`] telling the server `settings` too: its
    /// `initializationOptions`, then `workspace/didChangeConfiguration`, and
    /// the answers to its `workspace/configuration` requests.
    pub fn start_with(
        conn: Connection,
        root: &Path,
        timeout: Duration,
        settings: &ServerSettings,
    ) -> Result<Self, LspError> {
        let writer: SharedWriter = Arc::new(Mutex::new(conn.writer));
        let pending: Pending = Arc::default();
        let closed = Arc::new(AtomicBool::new(false));
        let progress: ProgressTokens = Arc::default();
        spawn_reader(
            conn.reader,
            writer.clone(),
            pending.clone(),
            closed.clone(),
            progress.clone(),
            Arc::new(settings.configuration.clone()),
        );

        let mut client = LspClient {
            writer,
            pending,
            closed,
            progress,
            next_id: 0,
            documents: HashMap::new(),
            child: conn.child,
            stderr: conn.stderr,
            timeout,
        };
        let root_uri = path_to_uri(root);
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "workspace".into());
        let mut params = json!({
            "processId": std::process::id(),
            "clientInfo": {"name": "SpatialCoding"},
            "rootUri": root_uri,
            "rootPath": root.to_string_lossy(),
            "workspaceFolders": [{"uri": root_uri, "name": name}],
            "capabilities": {
                "textDocument": {
                    "synchronization": {"didSave": false},
                    "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
                    "references": {},
                    "hover": {"contentFormat": ["markdown", "plaintext"]},
                    "definition": {"linkSupport": true},
                    "typeDefinition": {"linkSupport": true},
                    "implementation": {"linkSupport": true},
                    "callHierarchy": {}
                },
                "window": {"workDoneProgress": true},
                "workspace": {
                    "configuration": true,
                    "workspaceFolders": true,
                    "didChangeConfiguration": {}
                },
                "experimental": {"serverStatusNotification": true}
            }
        });
        if let Some(options) = &settings.initialization_options {
            params["initializationOptions"] = options.clone();
        }
        client.request("initialize", params)?;
        client.notify("initialized", json!({}))?;
        if !settings.configuration.is_null() {
            client.notify("workspace/didChangeConfiguration", json!({"settings": settings.configuration}))?;
        }
        Ok(client)
    }

    /// The server's active progress tokens (shared, updated by the reader).
    pub fn progress(&self) -> ProgressTokens {
        self.progress.clone()
    }

    pub fn is_indexing(&self) -> bool {
        !self.progress.lock().unwrap().is_empty()
    }

    /// Sends `didOpen` the first time, `didChange` (full text) when `text`
    /// changed since the last call, nothing otherwise.
    pub fn sync_document(&mut self, path: &Path, language_id: &str, text: &str) -> Result<(), LspError> {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let hash = hasher.finish();
        let uri = path_to_uri(path);
        match self.documents.get(path).copied() {
            None => {
                self.documents.insert(path.to_path_buf(), (1, hash));
                self.notify(
                    "textDocument/didOpen",
                    json!({"textDocument": {
                        "uri": uri, "languageId": language_id, "version": 1, "text": text
                    }}),
                )
            }
            Some((version, old)) if old != hash => {
                self.documents.insert(path.to_path_buf(), (version + 1, hash));
                self.notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": {"uri": uri, "version": version + 1},
                        "contentChanges": [{"text": text}]
                    }),
                )
            }
            Some(_) => Ok(()),
        }
    }

    pub fn document_symbols(&mut self, path: &Path) -> Result<Vec<LspSymbol>, LspError> {
        let result = self.request_with_retry(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": path_to_uri(path)}}),
        )?;
        Ok(parse_document_symbols(&result))
    }

    /// Every use of the symbol at `position`, its declaration included.
    pub fn references(&mut self, path: &Path, position: Position) -> Result<Vec<Location>, LspError> {
        let result = self.request_with_retry(
            "textDocument/references",
            json!({
                "textDocument": {"uri": path_to_uri(path)},
                "position": position_json(position),
                "context": {"includeDeclaration": true}
            }),
        )?;
        Ok(parse_locations(&result))
    }

    /// The graph of the symbol at `position`: hover, references, definitions,
    /// type definitions, implementations and both directions of the call
    /// hierarchy, asked together within [`QUERY_DEADLINE`]. What the server
    /// does not know, or does not answer in time, is left out; the query only
    /// fails when nothing answered.
    pub fn symbol_graph(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<super::graph::SymbolGraph, LspError> {
        self.symbol_graph_within(path, position, QUERY_DEADLINE)
    }

    /// [`Self::symbol_graph`] with its own time limit.
    pub fn symbol_graph_within(
        &mut self,
        path: &Path,
        position: Position,
        within: Duration,
    ) -> Result<super::graph::SymbolGraph, LspError> {
        use super::graph::{build, parse_call_items, parse_calls, parse_hover, parse_links, Answers};
        let deadline = Instant::now() + within;
        let at = json!({
            "textDocument": {"uri": path_to_uri(path)},
            "position": position_json(position),
        });
        let mut for_references = at.clone();
        for_references["context"] = json!({"includeDeclaration": true});
        let first = self.batch(
            &[
                ("textDocument/hover", at.clone()),
                ("textDocument/definition", at.clone()),
                ("textDocument/typeDefinition", at.clone()),
                ("textDocument/references", for_references),
                ("textDocument/implementation", at.clone()),
                ("textDocument/prepareCallHierarchy", at),
            ],
            deadline,
        );
        if first.iter().all(Result::is_err) {
            // The references are the answer people wait for: report their error.
            return Err(first.into_iter().nth(3).and_then(Result::err).unwrap_or(LspError::Io("no answer".into())));
        }
        let ok = |i: usize| first[i].as_ref().ok();
        let hover = ok(0).and_then(parse_hover);
        let definitions = ok(1).map(parse_links).unwrap_or_default();
        let type_definitions = ok(2).map(parse_links).unwrap_or_default();
        let references = ok(3).map(parse_locations).unwrap_or_default();
        let implementations = ok(4).map(parse_links).unwrap_or_default();
        let items = ok(5).map(parse_call_items).unwrap_or_default();

        // The call hierarchy needs the item the first wave found.
        let (mut incoming, mut outgoing) = (Vec::new(), Vec::new());
        if let Some(item) = items.first() {
            let for_item = json!({"item": item.raw});
            let mut calls = self
                .batch(
                    &[("callHierarchy/incomingCalls", for_item.clone()), ("callHierarchy/outgoingCalls", for_item)],
                    deadline,
                )
                .into_iter();
            incoming = calls.next().and_then(Result::ok).map(|r| parse_calls(&r, "from")).unwrap_or_default();
            outgoing = calls.next().and_then(Result::ok).map(|r| parse_calls(&r, "to")).unwrap_or_default();
        }

        // The token asked at: the reference or definition under `position`.
        let under = |range: &super::Range| range.start <= position && position <= range.end;
        let source = references
            .iter()
            .find(|l| l.path == path && under(&l.range))
            .cloned()
            .or_else(|| {
                definitions.iter().find(|l| l.path == path && under(&l.selection)).map(|l| super::Location {
                    path: l.path.clone(),
                    range: l.selection,
                })
            })
            .unwrap_or_else(|| super::Location {
                path: path.to_path_buf(),
                range: super::Range { start: position, end: position },
            });
        Ok(build(
            source,
            Answers { hover, references, definitions, type_definitions, implementations, incoming, outgoing },
        ))
    }

    /// Retries while the server says its state changed under the request
    /// (rust-analyzer does this while it indexes).
    fn request_with_retry(&mut self, method: &str, params: Value) -> Result<Value, LspError> {
        let mut attempt = 0;
        loop {
            match self.request(method, params.clone()) {
                Err(LspError::Rpc { code, .. })
                    if (code == CONTENT_MODIFIED || code == SERVER_CANCELLED) && attempt < MAX_RETRIES =>
                {
                    attempt += 1;
                    thread::sleep(Duration::from_millis(100 * u64::from(attempt)));
                }
                other => return other,
            }
        }
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, LspError> {
        self.request_timeout(method, params, self.timeout)
    }

    fn request_timeout(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, LspError> {
        let (tx, rx) = mpsc::channel();
        let id = self.start_request(method, params, &tx)?;
        // Only the pending entry keeps the queue open from here on.
        drop(tx);
        match rx.recv_timeout(timeout) {
            Ok((_, Err(LspError::Closed(_)))) | Err(RecvTimeoutError::Disconnected) => Err(self.closed_error()),
            Ok((_, reply)) => reply,
            Err(RecvTimeoutError::Timeout) => {
                self.pending.lock().unwrap().remove(&id);
                Err(LspError::Timeout(method.to_string()))
            }
        }
    }

    /// Sends a request and returns its id; the reply arrives as `(id, reply)`
    /// on `queue`.
    fn start_request(&mut self, method: &str, params: Value, queue: &Sender<(u64, Reply)>) -> Result<u64, LspError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(self.closed_error());
        }
        self.next_id += 1;
        let id = self.next_id;
        self.pending.lock().unwrap().insert(id, queue.clone());
        // The reader sets `closed` before failing the pending calls, so a call
        // registered after that is caught here instead of waiting for the timeout.
        if self.closed.load(Ordering::SeqCst) {
            self.pending.lock().unwrap().remove(&id);
            return Err(self.closed_error());
        }
        let sent = self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        if let Err(e) = sent {
            self.pending.lock().unwrap().remove(&id);
            return Err(e);
        }
        Ok(id)
    }

    /// Gives up on a request: a late answer is dropped, and the server is
    /// told it can stop working on it.
    fn cancel(&mut self, id: u64) {
        self.pending.lock().unwrap().remove(&id);
        let _ = self.notify("$/cancelRequest", json!({"id": id}));
    }

    /// Sends every call at once and collects the answers, in the order of
    /// `calls`, until all arrived or `deadline` passed. A server that says
    /// the content changed under a request (rust-analyzer while it indexes)
    /// gets it again after a short wait, without holding up the others; one
    /// that exits fails everything left. What is still unanswered at the
    /// deadline is cancelled and reported as a timeout.
    fn batch(&mut self, calls: &[(&str, Value)], deadline: Instant) -> Vec<Reply> {
        let (tx, queue) = mpsc::channel::<(u64, Reply)>();
        let mut results: Vec<Option<Reply>> = calls.iter().map(|_| None).collect();
        // id -> (call, attempt) for the requests on their way.
        let mut in_flight: HashMap<u64, (usize, u32)> = HashMap::new();
        // (when, call, attempt) of the requests to send again.
        let mut retries: Vec<(Instant, usize, u32)> = Vec::new();
        let send = |client: &mut Self,
                        in_flight: &mut HashMap<u64, (usize, u32)>,
                        results: &mut Vec<Option<Reply>>,
                        call: usize,
                        attempt: u32| {
            let (method, params) = &calls[call];
            match client.start_request(method, params.clone(), &tx) {
                Ok(id) => {
                    in_flight.insert(id, (call, attempt));
                }
                Err(e) => results[call] = Some(Err(e)),
            }
        };
        for call in 0..calls.len() {
            send(self, &mut in_flight, &mut results, call, 0);
            // A server that is gone is gone for every call.
            if matches!(results[call], Some(Err(LspError::Closed(_)))) {
                let error = results[call].clone();
                for later in &mut results[call + 1..] {
                    *later = error.clone();
                }
                break;
            }
        }
        while !in_flight.is_empty() || !retries.is_empty() {
            let now = Instant::now();
            let (due, later): (Vec<_>, Vec<_>) = retries.drain(..).partition(|(at, _, _)| *at <= now);
            retries = later;
            for (_, call, attempt) in due {
                send(self, &mut in_flight, &mut results, call, attempt);
            }
            if now >= deadline {
                break;
            }
            let next = retries.iter().map(|(at, _, _)| *at).min().map_or(deadline, |at| at.min(deadline));
            let (id, reply) = match queue.recv_timeout(next.saturating_duration_since(now)) {
                Ok(answer) => answer,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            let Some((call, attempt)) = in_flight.remove(&id) else { continue };
            match reply {
                Err(LspError::Rpc { code, .. })
                    if (code == CONTENT_MODIFIED || code == SERVER_CANCELLED) && attempt < MAX_RETRIES =>
                {
                    let wait = Duration::from_millis(100 * u64::from(attempt + 1));
                    retries.push((Instant::now() + wait, call, attempt + 1));
                }
                Err(LspError::Closed(_)) => {
                    let error = self.closed_error();
                    results[call] = Some(Err(error.clone()));
                    for (_, (other, _)) in in_flight.drain() {
                        results[other] = Some(Err(error.clone()));
                    }
                    for (_, other, _) in retries.drain(..) {
                        results[other] = Some(Err(error.clone()));
                    }
                }
                other => results[call] = Some(other),
            }
        }
        for (id, (call, _)) in std::mem::take(&mut in_flight) {
            self.cancel(id);
            results[call] = Some(Err(LspError::Timeout(calls[call].0.to_string())));
        }
        for (_, call, _) in retries {
            results[call] = Some(Err(LspError::Timeout(calls[call].0.to_string())));
        }
        results.into_iter().map(|r| r.unwrap_or_else(|| Err(LspError::Io("no answer".into())))).collect()
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<(), LspError> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    fn send(&self, msg: &Value) -> Result<(), LspError> {
        let mut writer = self.writer.lock().unwrap();
        write_message(&mut *writer, msg).map_err(|e| {
            if self.closed.load(Ordering::SeqCst) {
                self.closed_error()
            } else {
                LspError::Io(e.to_string())
            }
        })
    }

    fn closed_error(&self) -> LspError {
        // Give a dying process a moment to finish writing its error.
        thread::sleep(Duration::from_millis(50));
        LspError::Closed(self.stderr.as_ref().map(StderrTail::text).unwrap_or_default())
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if !self.closed.load(Ordering::SeqCst) {
            let _ = self.request_timeout("shutdown", Value::Null, Duration::from_secs(1));
            let _ = self.notify("exit", Value::Null);
        }
        if let Some(mut child) = self.child.take() {
            let deadline = Instant::now() + Duration::from_secs(1);
            while Instant::now() < deadline {
                if let Ok(Some(_)) = child.try_wait() {
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_reader(
    reader: Box<dyn Read + Send>,
    writer: SharedWriter,
    pending: Pending,
    closed: Arc<AtomicBool>,
    progress: ProgressTokens,
    configuration: Arc<Value>,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        while let Ok(Some(msg)) = read_message(&mut reader) {
            dispatch(&msg, &writer, &pending, &progress, &configuration);
        }
        closed.store(true, Ordering::SeqCst);
        for (id, tx) in pending.lock().unwrap().drain() {
            let _ = tx.send((id, Err(LspError::Closed(String::new()))));
        }
    });
}

/// The part of the settings tree a `workspace/configuration` item asks for:
/// the dotted `section`, or everything without one. `Null` when the tree has
/// nothing there.
fn configuration_section(tree: &Value, section: Option<&str>) -> Value {
    match section.filter(|s| !s.is_empty()) {
        None => tree.clone(),
        Some(path) => path.split('.').try_fold(tree, |node, key| node.get(key)).cloned().unwrap_or(Value::Null),
    }
}

fn dispatch(msg: &Value, writer: &SharedWriter, pending: &Pending, progress: &ProgressTokens, configuration: &Value) {
    let method = msg.get("method").and_then(Value::as_str);
    let id = msg.get("id");
    match (method, id) {
        // A response to one of our requests.
        (None, Some(id)) => {
            let Some(id) = id.as_u64() else { return };
            let Some(tx) = pending.lock().unwrap().remove(&id) else {
                return;
            };
            let reply = match msg.get("error") {
                Some(err) => Err(LspError::Rpc {
                    code: err.get("code").and_then(Value::as_i64).unwrap_or(0),
                    message: err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                }),
                None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = tx.send((id, reply));
        }
        // A request from the server: answer so it doesn't wait on us.
        (Some(method), Some(id)) => {
            let result = match method {
                "workspace/configuration" => Value::Array(
                    msg["params"]["items"]
                        .as_array()
                        .map(|items| {
                            items
                                .iter()
                                .map(|item| configuration_section(configuration, item["section"].as_str()))
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
                "workspace/workspaceFolders" => Value::Null,
                _ => Value::Null,
            };
            let reply = json!({"jsonrpc": "2.0", "id": id, "result": result});
            let _ = write_message(&mut *writer.lock().unwrap(), &reply);
        }
        (Some("experimental/serverStatus"), None) => {
            let mut tokens = progress.lock().unwrap();
            if msg["params"]["quiescent"].as_bool() == Some(false) {
                tokens.insert(NOT_QUIESCENT.to_string());
            } else {
                tokens.remove(NOT_QUIESCENT);
            }
        }
        (Some("$/progress"), None) => {
            let token = match &msg["params"]["token"] {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let mut tokens = progress.lock().unwrap();
            match msg["params"]["value"]["kind"].as_str() {
                Some("begin") => {
                    tokens.insert(token);
                }
                Some("end") => {
                    tokens.remove(&token);
                }
                _ => {}
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::PipeWriter;

    /// An in-process server driven by `script`, which gets each message the
    /// client sends and the pipe to answer on.
    fn fake_server(
        script: impl FnMut(&Value, &mut PipeWriter) + Send + 'static,
    ) -> (Connection, thread::JoinHandle<Vec<Value>>) {
        let (client_reader, mut server_writer) = std::io::pipe().unwrap();
        let (server_reader, client_writer) = std::io::pipe().unwrap();
        let handle = thread::spawn(move || {
            let mut script = script;
            let mut reader = BufReader::new(server_reader);
            let mut seen = Vec::new();
            while let Ok(Some(msg)) = read_message(&mut reader) {
                script(&msg, &mut server_writer);
                let exit = msg["method"] == "exit";
                seen.push(msg);
                if exit {
                    break;
                }
            }
            seen
        });
        let conn = Connection {
            reader: Box::new(client_reader),
            writer: Box::new(client_writer),
            child: None,
            stderr: None,
        };
        (conn, handle)
    }

    fn reply(w: &mut PipeWriter, msg: &Value, result: Value) {
        write_message(w, &json!({"jsonrpc": "2.0", "id": msg["id"], "result": result})).unwrap();
    }

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn handshake_symbols_and_shutdown() {
        let (conn, server) = fake_server(|msg, w| match msg["method"].as_str() {
            Some("initialize") => reply(w, msg, json!({"capabilities": {}})),
            Some("textDocument/documentSymbol") => reply(
                w,
                msg,
                json!([{"name": "main", "kind": 12,
                    "range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 1}},
                    "selectionRange": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 7}}}]),
            ),
            Some("shutdown") => reply(w, msg, Value::Null),
            _ => {}
        });
        let mut client = LspClient::start(conn, Path::new("/proj"), SECOND).unwrap();
        let path = Path::new("/proj/main.rs");
        client.sync_document(path, "rust", "fn main() {}").unwrap();
        client.sync_document(path, "rust", "fn main() {}").unwrap();
        client.sync_document(path, "rust", "fn main() { }").unwrap();
        let symbols = client.document_symbols(path).unwrap();
        assert_eq!(symbols[0].name, "main");
        drop(client);

        let seen = server.join().unwrap();
        let methods: Vec<&str> = seen.iter().filter_map(|m| m["method"].as_str()).collect();
        assert_eq!(
            methods,
            [
                "initialize",
                "initialized",
                "textDocument/didOpen",
                "textDocument/didChange",
                "textDocument/documentSymbol",
                "shutdown",
                "exit"
            ],
            "unchanged text is not resent"
        );
        assert_eq!(seen[0]["params"]["rootUri"], "file:///proj");
        assert_eq!(seen[3]["params"]["textDocument"]["version"], 2);
    }

    #[test]
    fn answers_server_requests_and_tracks_progress() {
        let (to_client, observed) = mpsc::channel::<Value>();
        let (conn, _server) = fake_server(move |msg, w| match msg["method"].as_str() {
            Some("initialize") => {
                // Ask the client for settings before answering, like rust-analyzer.
                write_message(w, &json!({"jsonrpc": "2.0", "id": "cfg-1", "method": "workspace/configuration",
                    "params": {"items": [{"section": "a"}, {"section": "b"}]}})).unwrap();
                write_message(w, &json!({"jsonrpc": "2.0", "method": "$/progress",
                    "params": {"token": "index", "value": {"kind": "begin", "title": "Indexing"}}})).unwrap();
                reply(w, msg, json!({"capabilities": {}}));
            }
            Some("shutdown") => reply(w, msg, Value::Null),
            None => to_client.send(msg.clone()).unwrap(),
            _ => {}
        });
        let client = LspClient::start(conn, Path::new("/proj"), SECOND).unwrap();
        let answer = observed.recv_timeout(SECOND).unwrap();
        assert_eq!(answer["id"], "cfg-1");
        assert_eq!(answer["result"], json!([null, null]));
        assert!(client.is_indexing());

        // A progress end clears it.
        let tokens = client.progress();
        tokens.lock().unwrap().clear();
        assert!(!client.is_indexing());
    }

    #[test]
    fn initialize_declares_every_request_the_client_makes_and_forwards_the_settings() {
        let (conn, server) = fake_server(|msg, w| match msg["method"].as_str() {
            Some("initialize") => reply(w, msg, json!({"capabilities": {}})),
            Some("shutdown") => reply(w, msg, Value::Null),
            _ => {}
        });
        let settings = ServerSettings {
            initialization_options: Some(json!({"linkedProjects": ["/proj/a/Cargo.toml"]})),
            configuration: json!({"python": {"analysis": {"diagnosticMode": "openFilesOnly"}}}),
        };
        drop(LspClient::start_with(conn, Path::new("/proj"), SECOND, &settings).unwrap());

        let seen = server.join().unwrap();
        let methods: Vec<&str> = seen.iter().filter_map(|m| m["method"].as_str()).collect();
        assert_eq!(
            methods,
            ["initialize", "initialized", "workspace/didChangeConfiguration", "shutdown", "exit"]
        );
        let params = &seen[0]["params"];
        let text_document = &params["capabilities"]["textDocument"];
        for link in ["definition", "typeDefinition", "implementation"] {
            assert_eq!(text_document[link]["linkSupport"], true, "{link}");
        }
        assert!(text_document["hover"]["contentFormat"].as_array().unwrap().contains(&json!("markdown")));
        assert!(text_document["callHierarchy"].is_object(), "call hierarchy is requested, so it is declared");
        assert!(text_document["references"].is_object() && text_document["documentSymbol"].is_object());
        assert_eq!(params["capabilities"]["workspace"]["configuration"], true);
        assert_eq!(params["initializationOptions"], json!({"linkedProjects": ["/proj/a/Cargo.toml"]}));
        assert_eq!(seen[2]["params"]["settings"], settings.configuration);
    }

    #[test]
    fn workspace_configuration_is_answered_by_section() {
        let (to_client, observed) = mpsc::channel::<Value>();
        let (conn, _server) = fake_server(move |msg, w| match msg["method"].as_str() {
            Some("initialize") => {
                write_message(w, &json!({"jsonrpc": "2.0", "id": "cfg", "method": "workspace/configuration",
                    "params": {"items": [{"section": "python.analysis"}, {"section": "python.nothing"}, {"section": "rust"}, {}]}}))
                    .unwrap();
                reply(w, msg, json!({"capabilities": {}}));
            }
            Some("shutdown") => reply(w, msg, Value::Null),
            None => to_client.send(msg.clone()).unwrap(),
            _ => {}
        });
        let settings = ServerSettings {
            initialization_options: None,
            configuration: json!({"python": {"analysis": {"diagnosticMode": "openFilesOnly"}}}),
        };
        let _client = LspClient::start_with(conn, Path::new("/proj"), SECOND, &settings).unwrap();
        let answer = observed.recv_timeout(SECOND).unwrap();
        assert_eq!(
            answer["result"],
            json!([{"diagnosticMode": "openFilesOnly"}, null, null, settings.configuration]),
            "unknown sections are null; an item without a section gets the whole tree"
        );
    }

    // ---- symbol_graph: every query at once (TASK-90) ----

    /// What the [`concurrent_server`] does with one request.
    enum React {
        Reply(Value),
        /// Reply from another thread after a delay, so slow answers overlap.
        After(Duration, Value),
        Fail(i64, &'static str),
        /// Never answer.
        Silent,
        /// The server process exits.
        Exit,
    }

    /// Like [`fake_server`], but a request may be answered later or from
    /// another thread, so requests in flight at once can be told apart from
    /// requests one after another.
    fn concurrent_server(
        script: impl Fn(&Value) -> React + Send + 'static,
    ) -> (Connection, thread::JoinHandle<Vec<Value>>) {
        let (client_reader, server_writer) = std::io::pipe().unwrap();
        let (server_reader, client_writer) = std::io::pipe().unwrap();
        let handle = thread::spawn(move || {
            let writer = Arc::new(Mutex::new(server_writer));
            let send = |writer: &Arc<Mutex<PipeWriter>>, message: Value| {
                let _ = write_message(&mut *writer.lock().unwrap(), &message);
            };
            let mut reader = BufReader::new(server_reader);
            let mut seen = Vec::new();
            while let Ok(Some(msg)) = read_message(&mut reader) {
                let is_exit = msg["method"] == "exit";
                let mut leave = is_exit;
                if let (Some(id), Some(_)) = (msg.get("id").cloned(), msg.get("method")) {
                    match script(&msg) {
                        React::Reply(result) => send(&writer, json!({"jsonrpc": "2.0", "id": id, "result": result})),
                        React::After(delay, result) => {
                            let writer = writer.clone();
                            thread::spawn(move || {
                                thread::sleep(delay);
                                send(&writer, json!({"jsonrpc": "2.0", "id": id, "result": result}));
                            });
                        }
                        React::Fail(code, message) => send(
                            &writer,
                            json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
                        ),
                        React::Silent => {}
                        React::Exit => leave = true,
                    }
                }
                seen.push(msg);
                if leave {
                    break;
                }
            }
            seen
        });
        let conn = Connection { reader: Box::new(client_reader), writer: Box::new(client_writer), child: None, stderr: None };
        (conn, handle)
    }

    fn span(line: usize, from: usize, to: usize) -> Value {
        json!({"start": {"line": line, "character": from}, "end": {"line": line, "character": to}})
    }

    fn location(path: &str, line: usize) -> Value {
        json!({"uri": format!("file://{path}"), "range": span(line, 3, 8)})
    }

    /// The answers of a server that knows everything about a symbol.
    fn knowing(method: &str) -> Value {
        match method {
            "textDocument/hover" => json!({"contents": {"kind": "plaintext", "value": "fn hello()"}}),
            "textDocument/references" => json!([location("/proj/a.rs", 0), location("/proj/b.rs", 4)]),
            "textDocument/definition" => json!([location("/proj/a.rs", 0)]),
            "textDocument/typeDefinition" | "textDocument/implementation" => json!([]),
            "textDocument/prepareCallHierarchy" => json!([]),
            _ => Value::Null,
        }
    }

    const ASKED_AT: Position = Position { line: 0, character: 3 };

    #[test]
    fn a_symbol_graph_asks_everything_at_once_and_includes_the_hover() {
        let (conn, server) = concurrent_server(|msg| match msg["method"].as_str().unwrap() {
            "initialize" => React::Reply(json!({"capabilities": {}})),
            "shutdown" => React::Reply(Value::Null),
            method => React::After(Duration::from_millis(300), knowing(method)),
        });
        let mut client = LspClient::start(conn, Path::new("/proj"), SECOND).unwrap();
        let started = Instant::now();
        let graph = client.symbol_graph(Path::new("/proj/a.rs"), ASKED_AT).unwrap();
        let took = started.elapsed();
        // Six requests of 300 ms one after another would take 1.8 s.
        assert!(took < Duration::from_millis(1000), "took {took:?}");
        assert_eq!(graph.hover.as_ref().map(|h| h.text.as_str()), Some("fn hello()"));
        assert!(graph.entries.iter().any(|e| e.path == Path::new("/proj/b.rs")), "{graph:?}");
        drop(client);

        let seen = server.join().unwrap();
        let asked: Vec<&str> = seen.iter().filter_map(|m| m["method"].as_str()).collect();
        for method in [
            "textDocument/hover",
            "textDocument/definition",
            "textDocument/typeDefinition",
            "textDocument/references",
            "textDocument/implementation",
            "textDocument/prepareCallHierarchy",
        ] {
            assert!(asked.contains(&method), "{method} in {asked:?}");
        }
        let references = seen.iter().find(|m| m["method"] == "textDocument/references").unwrap();
        assert_eq!(references["params"]["context"]["includeDeclaration"], true);
    }

    #[test]
    fn the_call_hierarchy_waits_for_its_item_but_shares_the_deadline() {
        let item = json!({"name": "hello", "kind": 12, "uri": "file:///proj/a.rs",
            "range": span(0, 0, 20), "selectionRange": span(0, 3, 8)});
        let caller = json!({"name": "main", "kind": 12, "uri": "file:///proj/b.rs",
            "range": span(4, 0, 9), "selectionRange": span(4, 3, 7)});
        let (conn, _server) = concurrent_server(move |msg| match msg["method"].as_str().unwrap() {
            "initialize" => React::Reply(json!({"capabilities": {}})),
            "shutdown" => React::Reply(Value::Null),
            "textDocument/prepareCallHierarchy" => React::Reply(json!([item])),
            "callHierarchy/incomingCalls" => {
                React::Reply(json!([{"from": caller, "fromRanges": [span(4, 4, 9)]}]))
            }
            "callHierarchy/outgoingCalls" => React::Silent,
            method => React::Reply(knowing(method)),
        });
        let mut client = LspClient::start(conn, Path::new("/proj"), SECOND).unwrap();
        let started = Instant::now();
        let graph = client
            .symbol_graph_within(Path::new("/proj/a.rs"), ASKED_AT, Duration::from_millis(600))
            .unwrap();
        assert!(started.elapsed() < Duration::from_millis(1500), "the unanswered outgoing calls end at the deadline");
        let roles: Vec<_> = graph.entries.iter().map(|e| e.role).collect();
        assert!(roles.contains(&super::super::graph::Role::InCaller), "incoming calls arrived: {roles:?}");
        assert!(!roles.contains(&super::super::graph::Role::OutCallee), "outgoing never answered");
    }

    #[test]
    fn what_has_not_answered_at_the_deadline_is_cancelled_and_the_rest_is_kept() {
        let (conn, server) = concurrent_server(|msg| match msg["method"].as_str().unwrap() {
            "initialize" => React::Reply(json!({"capabilities": {}})),
            "shutdown" => React::Reply(Value::Null),
            "textDocument/hover" => React::Silent,
            method => React::Reply(knowing(method)),
        });
        let mut client = LspClient::start(conn, Path::new("/proj"), SECOND).unwrap();
        let started = Instant::now();
        let graph = client
            .symbol_graph_within(Path::new("/proj/a.rs"), ASKED_AT, Duration::from_millis(400))
            .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(400), "it waited for the deadline");
        assert_eq!(graph.hover, None);
        assert!(graph.entries.iter().any(|e| e.path == Path::new("/proj/b.rs")), "the references are kept");
        drop(client);

        let seen = server.join().unwrap();
        let hover_id = seen.iter().find(|m| m["method"] == "textDocument/hover").unwrap()["id"].clone();
        let cancelled: Vec<&Value> =
            seen.iter().filter(|m| m["method"] == "$/cancelRequest").map(|m| &m["params"]["id"]).collect();
        assert_eq!(cancelled, vec![&hover_id], "only the silent request is cancelled");
    }

    #[test]
    fn the_query_fails_only_when_every_request_failed() {
        let (conn, _server) = concurrent_server(|msg| match msg["method"].as_str().unwrap() {
            "initialize" => React::Reply(json!({"capabilities": {}})),
            "shutdown" => React::Reply(Value::Null),
            "textDocument/references" => React::Fail(-32603, "references broke"),
            _ => React::Fail(-32601, "unknown"),
        });
        let mut client = LspClient::start(conn, Path::new("/proj"), SECOND).unwrap();
        let err = client.symbol_graph(Path::new("/proj/a.rs"), ASKED_AT).unwrap_err();
        assert_eq!(err, LspError::Rpc { code: -32603, message: "references broke".into() }, "the references' error");

        // One failing request is just left out: hover and the rest still arrive.
        let (conn, _server) = concurrent_server(|msg| match msg["method"].as_str().unwrap() {
            "initialize" => React::Reply(json!({"capabilities": {}})),
            "shutdown" => React::Reply(Value::Null),
            "textDocument/references" => React::Fail(-32603, "references broke"),
            method => React::Reply(knowing(method)),
        });
        let mut client = LspClient::start(conn, Path::new("/proj"), SECOND).unwrap();
        let graph = client.symbol_graph(Path::new("/proj/a.rs"), ASKED_AT).unwrap();
        assert!(graph.hover.is_some() && !graph.entries.is_empty(), "{graph:?}");
    }

    #[test]
    fn a_content_modified_request_is_asked_again_without_holding_up_the_others() {
        let definitions = std::sync::atomic::AtomicUsize::new(0);
        let (conn, server) = concurrent_server(move |msg| match msg["method"].as_str().unwrap() {
            "initialize" => React::Reply(json!({"capabilities": {}})),
            "shutdown" => React::Reply(Value::Null),
            "textDocument/definition" if definitions.fetch_add(1, Ordering::SeqCst) < 2 => {
                React::Fail(CONTENT_MODIFIED, "content modified")
            }
            method => React::Reply(knowing(method)),
        });
        let mut client = LspClient::start(conn, Path::new("/proj"), SECOND).unwrap();
        let started = Instant::now();
        let graph = client.symbol_graph(Path::new("/proj/a.rs"), ASKED_AT).unwrap();
        // 100 ms then 200 ms of waiting, in parallel with everything else.
        assert!(started.elapsed() < Duration::from_millis(1500));
        assert!(
            graph.entries.iter().any(|e| e.role == super::super::graph::Role::DefSel),
            "the third attempt answered: {graph:?}"
        );
        drop(client);
        let seen = server.join().unwrap();
        assert_eq!(seen.iter().filter(|m| m["method"] == "textDocument/definition").count(), 3);
    }

    #[test]
    fn a_server_that_exits_fails_every_request_at_once() {
        let (conn, _server) = concurrent_server(|msg| match msg["method"].as_str().unwrap() {
            "initialize" => React::Reply(json!({"capabilities": {}})),
            "textDocument/references" => React::Exit,
            _ => React::Silent,
        });
        let mut client = LspClient::start(conn, Path::new("/proj"), SECOND).unwrap();
        let started = Instant::now();
        let err = client
            .symbol_graph_within(Path::new("/proj/a.rs"), ASKED_AT, Duration::from_secs(20))
            .unwrap_err();
        assert!(matches!(err, LspError::Closed(_)), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(3), "it did not wait for the deadline");
    }

    #[test]
    fn rust_analyzer_server_status_counts_as_progress() {
        let writer: SharedWriter = Arc::new(Mutex::new(Box::new(std::io::sink())));
        let (pending, progress): (Pending, ProgressTokens) = Default::default();
        let status = |quiescent: bool| {
            json!({"jsonrpc": "2.0", "method": "experimental/serverStatus",
                "params": {"health": "ok", "quiescent": quiescent}})
        };
        dispatch(&status(false), &writer, &pending, &progress, &Value::Null);
        assert!(progress.lock().unwrap().contains(NOT_QUIESCENT));
        dispatch(&status(true), &writer, &pending, &progress, &Value::Null);
        assert!(progress.lock().unwrap().is_empty());
    }

    #[test]
    fn retries_content_modified_then_returns_references() {
        let mut attempts = 0;
        let (conn, _server) = fake_server(move |msg, w| match msg["method"].as_str() {
            Some("initialize") | Some("shutdown") => reply(w, msg, json!({})),
            Some("textDocument/references") => {
                attempts += 1;
                if attempts < 3 {
                    write_message(w, &json!({"jsonrpc": "2.0", "id": msg["id"],
                        "error": {"code": CONTENT_MODIFIED, "message": "content modified"}})).unwrap();
                } else {
                    reply(w, msg, json!([{"uri": "file:///proj/b.rs",
                        "range": {"start": {"line": 4, "character": 2}, "end": {"line": 4, "character": 6}}}]));
                }
            }
            _ => {}
        });
        let mut client = LspClient::start(conn, Path::new("/proj"), SECOND).unwrap();
        let refs = client
            .references(Path::new("/proj/a.rs"), Position { line: 0, character: 3 })
            .unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, Path::new("/proj/b.rs"));
        assert_eq!(refs[0].range.start.line, 4);
    }

    #[test]
    fn rpc_errors_timeouts_and_exits_are_reported() {
        let (conn, server) = fake_server(|msg, w| match msg["method"].as_str() {
            Some("initialize") => reply(w, msg, json!({})),
            Some("bad") => write_message(w, &json!({"jsonrpc": "2.0", "id": msg["id"],
                "error": {"code": -32601, "message": "no such method"}})).unwrap(),
            Some("die") => {
                // Close the server's output: the process "exits".
                let (_r, dead) = std::io::pipe().unwrap();
                *w = dead;
            }
            _ => {} // "slow": never answered
        });
        let mut client = LspClient::start(conn, Path::new("/proj"), Duration::from_millis(100)).unwrap();
        assert_eq!(
            client.request("bad", Value::Null),
            Err(LspError::Rpc { code: -32601, message: "no such method".into() })
        );
        assert_eq!(client.request("slow", Value::Null), Err(LspError::Timeout("slow".into())));
        assert!(matches!(client.request("die", Value::Null), Err(LspError::Closed(_))));
        assert!(matches!(client.request("again", Value::Null), Err(LspError::Closed(_))));
        drop(client);
        drop(server);
    }

    #[test]
    fn a_server_that_cannot_start_reports_its_stderr() {
        let (reader, _) = std::io::pipe().unwrap(); // closed at once: "exits"
        let (_r, writer) = std::io::pipe().unwrap();
        let (err_r, mut err_w) = std::io::pipe().unwrap();
        err_w.write_all(b"error: rust-analyzer is not installed\n").unwrap();
        drop(err_w);
        let conn = Connection {
            reader: Box::new(reader),
            writer: Box::new(writer),
            child: None,
            stderr: Some(StderrTail::capture(err_r)),
        };
        let err = LspClient::start(conn, Path::new("/p"), SECOND).err().unwrap();
        assert_eq!(err, LspError::Closed("error: rust-analyzer is not installed".into()));
        assert!(err.to_string().contains("not installed"));
    }

    /// Talks to a real rust-analyzer: `cargo test --lib -- --ignored real_rust_analyzer`.
    #[test]
    #[ignore = "needs rust-analyzer on PATH and takes a few seconds"]
    fn real_rust_analyzer_symbols_and_cross_file_references() {
        use super::super::manager::spawn_server;
        use super::super::ServerId;

        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let write = |rel: &str, text: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
            p
        };
        write("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
        let main_text = "mod util;\n\nstruct Point { x: i32 }\n\nfn main() {\n    util::hello();\n}\n";
        let util_text = "pub fn hello() {}\n\npub fn twice() {\n    hello();\n}\n";
        let main_rs = write("src/main.rs", main_text);
        let util_rs = write("src/util.rs", util_text);

        let conn = spawn_server(ServerId::Rust, &root).expect("rust-analyzer starts");
        let mut client = LspClient::start(conn, &root, Duration::from_secs(60)).unwrap();
        client.sync_document(&main_rs, "rust", main_text).unwrap();
        client.sync_document(&util_rs, "rust", util_text).unwrap();

        let symbols = client.document_symbols(&main_rs).unwrap();
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"main") && names.contains(&"Point"), "{names:?}");

        // References need the index; ask until rust-analyzer has it.
        let hello = Position { line: 0, character: 7 };
        let deadline = Instant::now() + Duration::from_secs(90);
        let refs = loop {
            let refs = client.references(&util_rs, hello).unwrap_or_default();
            if refs.iter().any(|l| l.path == main_rs) || Instant::now() > deadline {
                break refs;
            }
            thread::sleep(Duration::from_millis(500));
        };
        let found: Vec<(PathBuf, usize)> = refs.iter().map(|l| (l.path.clone(), l.range.start.line)).collect();
        assert!(found.contains(&(main_rs.clone(), 5)), "use in main.rs: {found:?}");
        assert!(found.contains(&(util_rs.clone(), 3)), "use in util.rs: {found:?}");
        assert!(found.contains(&(util_rs.clone(), 0)), "declaration: {found:?}");

        // The whole graph at once, hover included, against the real server.
        let started = Instant::now();
        let graph = client.symbol_graph(&util_rs, hello).unwrap();
        assert!(started.elapsed() < QUERY_DEADLINE, "answered before the deadline");
        let hover = graph.hover.as_ref().map(|h| h.text.as_str()).unwrap_or_default();
        assert!(hover.contains("hello"), "hover: {hover:?}");
        assert!(graph.entries.iter().any(|e| e.path == main_rs), "the use in main.rs: {:?}", graph.entries);
    }

    /// A folder of crates has no manifest at its root: only the linked
    /// projects of [`ServerSettings`] make rust-analyzer load them.
    #[test]
    #[ignore = "needs rust-analyzer on PATH and takes a few seconds"]
    fn real_rust_analyzer_loads_the_crates_of_a_folder_without_a_root_manifest() {
        use super::super::manager::spawn_server;
        use super::super::ServerId;

        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let write = |rel: &str, text: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
            p
        };
        write("svc/a/Cargo.toml", "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
        let text = "pub fn hello() {}\n\npub fn twice() {\n    hello();\n}\n";
        let lib = write("svc/a/src/lib.rs", text);

        let settings = ServerId::Rust.settings(&root);
        assert!(settings.initialization_options.is_some(), "the crate is linked");
        let conn = spawn_server(ServerId::Rust, &root).expect("rust-analyzer starts");
        let mut client = LspClient::start_with(conn, &root, Duration::from_secs(60), &settings).unwrap();
        client.sync_document(&lib, "rust", text).unwrap();

        let hello = Position { line: 0, character: 7 };
        let deadline = Instant::now() + Duration::from_secs(90);
        let refs = loop {
            let refs = client.references(&lib, hello).unwrap_or_default();
            if refs.iter().any(|l| l.range.start.line == 3) || Instant::now() > deadline {
                break refs;
            }
            thread::sleep(Duration::from_millis(500));
        };
        let lines: Vec<usize> = refs.iter().map(|l| l.range.start.line).collect();
        assert!(lines.contains(&3), "the call in twice(): {lines:?}");
    }
}
