//! Non-blocking front end to the language servers, owned by the UI.
//!
//! Each `(server, workspace root)` gets one worker thread, started on the
//! first request. The worker owns an [`LspClient`] and runs requests one at a
//! time; results come back through [`LspManager::poll`], and the worker asks
//! egui for a repaint when one is ready. The UI thread never waits on a server.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::client::{Connection, LspClient, ProgressTokens, StderrTail};
use super::{Location, LspSymbol, Position, ServerId};

/// How long one request may take. rust-analyzer can block a first request
/// until it has loaded the workspace.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

pub type RequestId = u64;

/// Starts a server for a workspace root and returns its pipes.
pub type Launcher = Arc<dyn Fn(ServerId, &Path) -> Result<Connection, String> + Send + Sync>;

#[derive(Debug, Clone)]
pub enum LspRequest {
    Symbols {
        path: PathBuf,
        language_id: &'static str,
        text: String,
    },
    References {
        path: PathBuf,
        language_id: &'static str,
        text: String,
        position: Position,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum LspResponse {
    Symbols(Result<Vec<LspSymbol>, String>),
    References(Result<Vec<Location>, String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    Starting,
    /// Running and reporting work in progress (usually indexing).
    Indexing,
    Ready,
    Failed(String),
}

struct Job {
    id: RequestId,
    request: LspRequest,
    ctx: Option<egui::Context>,
}

#[derive(Default)]
struct Shared {
    failed: Option<String>,
    progress: Option<ProgressTokens>,
}

struct ServerHandle {
    jobs: Sender<Job>,
    shared: Arc<Mutex<Shared>>,
}

pub struct LspManager {
    launcher: Launcher,
    servers: HashMap<(ServerId, PathBuf), ServerHandle>,
    next_id: RequestId,
    results_tx: Sender<(RequestId, LspResponse)>,
    results_rx: Receiver<(RequestId, LspResponse)>,
}

impl Default for LspManager {
    fn default() -> Self {
        Self::with_launcher(Arc::new(spawn_server))
    }
}

impl LspManager {
    pub fn with_launcher(launcher: Launcher) -> Self {
        let (results_tx, results_rx) = mpsc::channel();
        Self {
            launcher,
            servers: HashMap::new(),
            next_id: 0,
            results_tx,
            results_rx,
        }
    }

    /// Replaces how servers are started, and stops the running ones.
    pub fn set_launcher(&mut self, launcher: Launcher) {
        self.launcher = launcher;
        self.servers.clear();
    }

    /// Queues `request` on the server for `root`, starting it if needed.
    /// `ctx` gets a repaint request when the answer is ready.
    pub fn request(
        &mut self,
        ctx: Option<&egui::Context>,
        server: ServerId,
        root: &Path,
        request: LspRequest,
    ) -> RequestId {
        self.next_id += 1;
        let id = self.next_id;
        let job = Job {
            id,
            request,
            ctx: ctx.cloned(),
        };
        let key = (server, root.to_path_buf());
        let (launcher, results) = (self.launcher.clone(), self.results_tx.clone());
        let handle = self
            .servers
            .entry(key)
            .or_insert_with(|| start_worker(launcher, server, root, results));
        if let Err(mpsc::SendError(job)) = handle.jobs.send(job) {
            // The worker is gone (it panicked); report instead of hanging.
            let _ = self.results_tx.send((id, failure(&job.request, "The language server worker stopped.".into())));
        }
        id
    }

    /// Answers that arrived since the last call.
    pub fn poll(&mut self) -> Vec<(RequestId, LspResponse)> {
        self.results_rx.try_iter().collect()
    }

    /// State of the server for `root`; `None` if it was never asked anything.
    pub fn status(&self, server: ServerId, root: &Path) -> Option<ServerStatus> {
        let handle = self.servers.get(&(server, root.to_path_buf()))?;
        let shared = handle.shared.lock().unwrap();
        Some(if let Some(err) = &shared.failed {
            ServerStatus::Failed(err.clone())
        } else {
            match &shared.progress {
                None => ServerStatus::Starting,
                Some(tokens) if !tokens.lock().unwrap().is_empty() => ServerStatus::Indexing,
                Some(_) => ServerStatus::Ready,
            }
        })
    }
}

fn failure(request: &LspRequest, err: String) -> LspResponse {
    match request {
        LspRequest::Symbols { .. } => LspResponse::Symbols(Err(err)),
        LspRequest::References { .. } => LspResponse::References(Err(err)),
    }
}

fn start_worker(
    launcher: Launcher,
    server: ServerId,
    root: &Path,
    results: Sender<(RequestId, LspResponse)>,
) -> ServerHandle {
    let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
    let shared = Arc::new(Mutex::new(Shared::default()));
    let worker_shared = shared.clone();
    let root = root.to_path_buf();
    let spawned = thread::Builder::new()
        .name(format!("lsp-{}", server.name().to_lowercase()))
        .spawn(move || {
            let reply = |job: &Job, response: LspResponse| {
                let _ = results.send((job.id, response));
                if let Some(ctx) = &job.ctx {
                    ctx.request_repaint();
                }
            };
            let started = launcher(server, &root)
                .and_then(|conn| LspClient::start(conn, &root, REQUEST_TIMEOUT).map_err(|e| e.to_string()));
            let mut client = match started {
                Ok(client) => client,
                Err(err) => {
                    log::warn!("{} language server: {err}", server.name());
                    worker_shared.lock().unwrap().failed = Some(err.clone());
                    for job in jobs_rx {
                        reply(&job, failure(&job.request, err.clone()));
                    }
                    return;
                }
            };
            worker_shared.lock().unwrap().progress = Some(client.progress());
            for job in jobs_rx {
                let response = run(&mut client, &job.request);
                reply(&job, response);
            }
            // The manager is gone: dropping the client shuts the server down.
        });
    if let Err(e) = spawned {
        shared.lock().unwrap().failed = Some(format!("Could not start a worker thread: {e}"));
    }
    ServerHandle {
        jobs: jobs_tx,
        shared,
    }
}

fn run(client: &mut LspClient, request: &LspRequest) -> LspResponse {
    match request {
        LspRequest::Symbols {
            path,
            language_id,
            text,
        } => LspResponse::Symbols(
            client
                .sync_document(path, language_id, text)
                .and_then(|_| client.document_symbols(path))
                .map_err(|e| e.to_string()),
        ),
        LspRequest::References {
            path,
            language_id,
            text,
            position,
        } => LspResponse::References(
            client
                .sync_document(path, language_id, text)
                .and_then(|_| client.references(path, *position))
                .map_err(|e| e.to_string()),
        ),
    }
}

/// The default [`Launcher`]: runs [`ServerId::command`] in `root` with piped
/// stdio.
pub fn spawn_server(server: ServerId, root: &Path) -> Result<Connection, String> {
    let cmd = server.command()?;
    let mut child = Command::new(&cmd[0])
        .args(&cmd[1..])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "Could not start `{}`: {e}. Set {} to the server command.",
                cmd.join(" "),
                server.env_var()
            )
        })?;
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().map(StderrTail::capture);
    Ok(Connection {
        reader: Box::new(stdout),
        writer: Box::new(stdin),
        child: Some(child),
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::lsp::protocol::{read_message, write_message};
    use serde_json::json;
    use std::io::BufReader;
    use std::time::Instant;

    /// A launcher whose servers answer `initialize`, return one symbol named
    /// after the file, and count how often they were started.
    fn counting_launcher(starts: Arc<Mutex<usize>>) -> Launcher {
        Arc::new(move |_server, _root| {
            *starts.lock().unwrap() += 1;
            let (client_reader, mut w) = std::io::pipe().unwrap();
            let (server_reader, client_writer) = std::io::pipe().unwrap();
            thread::spawn(move || {
                let mut r = BufReader::new(server_reader);
                while let Ok(Some(msg)) = read_message(&mut r) {
                    let result = match msg["method"].as_str() {
                        Some("textDocument/documentSymbol") => {
                            let uri = msg["params"]["textDocument"]["uri"].as_str().unwrap();
                            let name = uri.rsplit('/').next().unwrap();
                            let r = json!({"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}});
                            json!([{"name": name, "kind": 12, "range": r, "selectionRange": r}])
                        }
                        Some(_) if msg.get("id").is_some() => json!(null),
                        _ => continue,
                    };
                    write_message(&mut w, &json!({"jsonrpc": "2.0", "id": msg["id"], "result": result})).unwrap();
                }
            });
            Ok(Connection {
                reader: Box::new(client_reader),
                writer: Box::new(client_writer),
                child: None,
                stderr: None,
            })
        })
    }

    fn wait_for(manager: &mut LspManager, n: usize) -> Vec<(RequestId, LspResponse)> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut out = Vec::new();
        while out.len() < n && Instant::now() < deadline {
            out.extend(manager.poll());
            thread::sleep(Duration::from_millis(5));
        }
        out
    }

    fn symbols(path: &str) -> LspRequest {
        LspRequest::Symbols {
            path: PathBuf::from(path),
            language_id: "rust",
            text: String::new(),
        }
    }

    #[test]
    fn requests_run_in_the_background_on_one_server_per_root() {
        let starts = Arc::new(Mutex::new(0));
        let mut manager = LspManager::with_launcher(counting_launcher(starts.clone()));
        let root = Path::new("/proj");
        assert_eq!(manager.status(ServerId::Rust, root), None);

        let a = manager.request(None, ServerId::Rust, root, symbols("/proj/a.rs"));
        let b = manager.request(None, ServerId::Rust, root, symbols("/proj/b.rs"));
        let mut results = wait_for(&mut manager, 2);
        results.sort_by_key(|(id, _)| *id);
        let names: Vec<(RequestId, String)> = results
            .into_iter()
            .map(|(id, r)| match r {
                LspResponse::Symbols(Ok(s)) => (id, s[0].name.clone()),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(names, [(a, "a.rs".into()), (b, "b.rs".into())]);
        assert_eq!(*starts.lock().unwrap(), 1, "one server for both requests");
        assert_eq!(manager.status(ServerId::Rust, root), Some(ServerStatus::Ready));

        manager.request(None, ServerId::Rust, Path::new("/other"), symbols("/other/c.rs"));
        manager.request(None, ServerId::Go, root, symbols("/proj/d.go"));
        wait_for(&mut manager, 2);
        assert_eq!(*starts.lock().unwrap(), 3, "a new server per root and per language");
    }

    #[test]
    fn a_server_that_fails_to_start_fails_every_request() {
        let launcher: Launcher = Arc::new(|server, _| Err(format!("no server, set {}", server.env_var())));
        let mut manager = LspManager::with_launcher(launcher);
        let root = Path::new("/proj");
        manager.request(None, ServerId::Rust, root, symbols("/proj/a.rs"));
        manager.request(
            None,
            ServerId::Rust,
            root,
            LspRequest::References {
                path: "/proj/a.rs".into(),
                language_id: "rust",
                text: String::new(),
                position: Position::default(),
            },
        );
        let results = wait_for(&mut manager, 2);
        let err = "no server, set SPATIALCODING_LSP_RUST".to_string();
        assert_eq!(results[0].1, LspResponse::Symbols(Err(err.clone())));
        assert_eq!(results[1].1, LspResponse::References(Err(err.clone())));
        assert_eq!(manager.status(ServerId::Rust, root), Some(ServerStatus::Failed(err)));
    }

    #[test]
    fn spawn_server_explains_a_missing_command() {
        // SAFETY: tests in this binary don't read this variable concurrently.
        unsafe { std::env::set_var("SPATIALCODING_LSP_GO", "/nonexistent/gopls-for-test") };
        let err = spawn_server(ServerId::Go, Path::new("/")).err().unwrap();
        unsafe { std::env::remove_var("SPATIALCODING_LSP_GO") };
        assert!(err.contains("/nonexistent/gopls-for-test"), "{err}");
        assert!(err.contains("SPATIALCODING_LSP_GO"), "{err}");
    }
}
