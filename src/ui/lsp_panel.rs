//! Right-sidebar "LSP" tab: the language server's symbols for the primary
//! selected file node. Clicking a symbol lists its references across the
//! workspace; clicking a reference focuses its line on the canvas.
//!
//! Requests go through [`LspManager`], so the panel never waits on a server:
//! it shows "Loading…" and picks the answer up on a later frame.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::analysis::lsp::manager::{LspManager, LspRequest, LspResponse, RequestId, ServerStatus};
use crate::analysis::lsp::{server_for, Location, LspSymbol, ServerId};
use crate::state::AppState;
use crate::workspace;

const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(0xef, 0x44, 0x44);
/// Longest line preview in the references list, in characters.
const MAX_PREVIEW: usize = 80;

/// Lines of a file node to highlight on the canvas. 0-based, inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineHighlight {
    pub node_id: u64,
    pub start_line: usize,
    pub end_line: usize,
}

/// An answer that may still be on its way.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetch<T> {
    Pending(RequestId),
    Done(Result<T, String>),
}

impl<T> Fetch<T> {
    pub fn is_pending(&self) -> bool {
        matches!(self, Fetch::Pending(_))
    }
}

struct SymbolsEntry {
    node_id: u64,
    source_hash: u64,
    fetch: Fetch<Vec<LspSymbol>>,
}

/// One usage of a symbol, with the text of its line.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceRow {
    pub path: PathBuf,
    /// 0-based.
    pub line: usize,
    pub preview: String,
}

/// The references of the symbol last clicked. They stay listed while the
/// selection moves, so several of them can be visited in turn.
pub struct References {
    pub symbol: String,
    /// The file node and line of the symbol itself.
    pub node_id: u64,
    pub line: usize,
    pub fetch: Fetch<Vec<ReferenceRow>>,
}

#[derive(Default)]
pub struct LspPanelState {
    pub manager: LspManager,
    symbols: Option<SymbolsEntry>,
    pub references: Option<References>,
    /// The symbol or reference row under the pointer; reset every frame the
    /// panel draws.
    pub hovered: Option<LineHighlight>,
    /// The line of the last reference clicked; painted while its node is the
    /// primary selection.
    pub target: Option<LineHighlight>,
}

impl LspPanelState {
    /// The symbols of the last synced node, if they arrived.
    pub fn symbols(&self) -> Option<&Fetch<Vec<LspSymbol>>> {
        self.symbols.as_ref().map(|s| &s.fetch)
    }

    /// Asks for the symbols of `node_id` unless the ones held (or on their
    /// way) are for the same node, path and content.
    pub fn sync_symbols(
        &mut self,
        ctx: Option<&egui::Context>,
        root: &Path,
        node_id: u64,
        path: &Path,
        text: &str,
    ) {
        let Some((server, language_id)) = server_for(path) else {
            self.symbols = None;
            return;
        };
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        text.hash(&mut hasher);
        let source_hash = hasher.finish();
        let fresh = self
            .symbols
            .as_ref()
            .is_some_and(|s| s.node_id == node_id && s.source_hash == source_hash);
        if fresh {
            return;
        }
        let id = self.manager.request(
            ctx,
            server,
            root,
            LspRequest::Symbols {
                path: path.to_path_buf(),
                language_id,
                text: text.to_string(),
            },
        );
        self.symbols = Some(SymbolsEntry {
            node_id,
            source_hash,
            fetch: Fetch::Pending(id),
        });
    }

    /// Asks for every use of `symbol`, declared in file node `node_id`.
    pub fn find_references(
        &mut self,
        ctx: Option<&egui::Context>,
        root: &Path,
        node_id: u64,
        path: &Path,
        text: &str,
        symbol: &LspSymbol,
    ) {
        let Some((server, language_id)) = server_for(path) else {
            return;
        };
        let id = self.manager.request(
            ctx,
            server,
            root,
            LspRequest::References {
                path: path.to_path_buf(),
                language_id,
                text: text.to_string(),
                position: symbol.selection_range.start,
            },
        );
        self.references = Some(References {
            symbol: symbol.name.clone(),
            node_id,
            line: symbol.selection_range.start.line,
            fetch: Fetch::Pending(id),
        });
    }

    /// Takes the answers that arrived; stale ones are dropped.
    pub fn poll(&mut self) {
        for (id, response) in self.manager.poll() {
            match response {
                LspResponse::Symbols(result) => {
                    if let Some(entry) = &mut self.symbols
                        && entry.fetch == Fetch::Pending(id)
                    {
                        entry.fetch = Fetch::Done(result);
                    }
                }
                LspResponse::References(result) => {
                    if let Some(refs) = &mut self.references
                        && matches!(refs.fetch, Fetch::Pending(p) if p == id)
                    {
                        refs.fetch = Fetch::Done(result.map(|locs| reference_rows(&locs)));
                    }
                }
            }
        }
    }

    /// True while an answer is on its way.
    pub fn is_waiting(&self) -> bool {
        self.symbols.as_ref().is_some_and(|s| s.fetch.is_pending())
            || self.references.as_ref().is_some_and(|r| r.fetch.is_pending())
    }
}

/// Rows for `locations`, with each line's text read from disk.
pub fn reference_rows(locations: &[Location]) -> Vec<ReferenceRow> {
    let mut rows = Vec::with_capacity(locations.len());
    let mut cached: Option<(PathBuf, Vec<String>)> = None;
    for loc in locations {
        if cached.as_ref().is_none_or(|(p, _)| *p != loc.path) {
            let lines = std::fs::read_to_string(&loc.path)
                .map(|t| t.lines().map(String::from).collect())
                .unwrap_or_default();
            cached = Some((loc.path.clone(), lines));
        }
        let lines = &cached.as_ref().unwrap().1;
        let line = loc.range.start.line;
        let preview: String = lines
            .get(line)
            .map(|l| l.trim().chars().take(MAX_PREVIEW).collect())
            .unwrap_or_default();
        rows.push(ReferenceRow {
            path: loc.path.clone(),
            line,
            preview,
        });
    }
    rows
}

/// What a click in the panel asked for, applied after drawing.
enum Action {
    FindReferences(LspSymbol),
    Focus(PathBuf, usize),
}

pub fn draw_lsp_panel(ui: &mut egui::Ui, state: &mut AppState) {
    state.lsp.hovered = None;
    state.lsp.poll();
    let mut action = None;

    ui.heading("Symbols");
    ui.separator();
    match state
        .selection
        .primary
        .filter(|id| state.node_store.get_file(*id).is_some())
    {
        Some(node_id) => draw_symbols(ui, state, node_id, &mut action),
        None => {
            ui.label("Select a file node to see its symbols.");
        }
    }
    draw_references(ui, state, &mut action);

    if state.lsp.is_waiting() {
        // The worker repaints when an answer arrives; this keeps the server
        // status line current while it starts or indexes.
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }

    match action {
        Some(Action::FindReferences(symbol)) => {
            let Some(node_id) = state.selection.primary else { return };
            let Some(node) = state.node_store.get_file(node_id) else { return };
            let root = workspace_root(state, &node.path);
            state.lsp.find_references(
                Some(ui.ctx()),
                &root,
                node_id,
                &node.path,
                &node.content,
                &symbol,
            );
        }
        Some(Action::Focus(path, line)) => {
            workspace::focus_location(state, &path, line);
        }
        None => {}
    }
}

/// The folder the server for `path` works on: the loaded folder, else the
/// file's own folder.
fn workspace_root(state: &AppState, path: &Path) -> PathBuf {
    state
        .loaded_dir
        .clone()
        .or_else(|| path.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

fn draw_symbols(ui: &mut egui::Ui, state: &mut AppState, node_id: u64, action: &mut Option<Action>) {
    let node = state.node_store.get_file(node_id).unwrap();
    ui.label(egui::RichText::new(node.file_name()).strong());
    let Some((server, _)) = server_for(&node.path) else {
        ui.label(format!("No language server for {}.", node.language));
        return;
    };
    let root = workspace_root(state, &node.path);
    state
        .lsp
        .sync_symbols(Some(ui.ctx()), &root, node_id, &node.path, &node.content);
    draw_status(ui, &state.lsp.manager, server, &root);

    let selected = state
        .lsp
        .references
        .as_ref()
        .filter(|r| r.node_id == node_id)
        .map(|r| (r.symbol.clone(), r.line));
    let mut hovered = None;
    match state.lsp.symbols() {
        None | Some(Fetch::Pending(_)) => {
            ui.label("Loading symbols\u{2026}");
        }
        Some(Fetch::Done(Err(err))) => {
            ui.colored_label(ERROR_COLOR, err);
        }
        Some(Fetch::Done(Ok(symbols))) if symbols.is_empty() => {
            ui.label("No symbols.");
        }
        Some(Fetch::Done(Ok(symbols))) => {
            ui.weak("Click a symbol to find its references.");
            let id = ui.make_persistent_id(("lsp-symbols", node_id));
            for (i, symbol) in symbols.iter().enumerate() {
                draw_symbol(ui, symbol, id.with(i), &selected, &mut hovered, action);
            }
        }
    }
    state.lsp.hovered = hovered.map(|(start_line, end_line)| LineHighlight {
        node_id,
        start_line,
        end_line,
    });
}

fn draw_status(ui: &mut egui::Ui, manager: &LspManager, server: ServerId, root: &Path) {
    match manager.status(server, root) {
        Some(ServerStatus::Starting) => {
            ui.weak(format!("Starting the {} language server\u{2026}", server.name()));
        }
        Some(ServerStatus::Indexing) => {
            ui.weak(format!("The {} language server is indexing\u{2026}", server.name()));
        }
        // Failures show up as the error of the request itself.
        _ => {}
    }
}

/// `fn main  L3`, with a 1-based line.
pub fn symbol_label(symbol: &LspSymbol) -> String {
    let line = symbol.selection_range.start.line + 1;
    match symbol.kind.label() {
        "" => format!("{}  L{line}", symbol.name),
        kind => format!("{kind} {}  L{line}", symbol.name),
    }
}

fn draw_symbol(
    ui: &mut egui::Ui,
    symbol: &LspSymbol,
    id: egui::Id,
    selected: &Option<(String, usize)>,
    hovered: &mut Option<(usize, usize)>,
    action: &mut Option<Action>,
) {
    let is_selected = selected
        .as_ref()
        .is_some_and(|(name, line)| *name == symbol.name && *line == symbol.selection_range.start.line);
    let text = egui::RichText::new(symbol_label(symbol)).monospace();
    let row = |ui: &mut egui::Ui| {
        let response = ui.selectable_label(is_selected, text.clone());
        if let Some(detail) = &symbol.detail {
            response.clone().on_hover_text(detail);
        }
        response
    };

    let response = if symbol.children.is_empty() {
        row(ui)
    } else {
        let mut header = None;
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
            .show_header(ui, |ui| header = Some(row(ui)))
            .body(|ui| {
                for (i, child) in symbol.children.iter().enumerate() {
                    draw_symbol(ui, child, id.with(i), selected, hovered, action);
                }
            });
        header.unwrap()
    };
    if response.hovered() {
        *hovered = Some((symbol.range.start.line, symbol.range.end.line));
    }
    if response.clicked() {
        *action = Some(Action::FindReferences(symbol.clone()));
    }
}

fn draw_references(ui: &mut egui::Ui, state: &mut AppState, action: &mut Option<Action>) {
    let Some(refs) = &state.lsp.references else {
        return;
    };
    ui.separator();
    ui.heading(format!("References to {}", refs.symbol));
    let rows = match &refs.fetch {
        Fetch::Pending(_) => {
            ui.label("Finding references\u{2026}");
            return;
        }
        Fetch::Done(Err(err)) => {
            ui.colored_label(ERROR_COLOR, err);
            return;
        }
        Fetch::Done(Ok(rows)) => rows,
    };
    if rows.is_empty() {
        ui.label("No references found.");
        return;
    }

    let files = rows.windows(2).filter(|w| w[0].path != w[1].path).count() + 1;
    ui.weak(format!(
        "{} {} in {} {}",
        rows.len(),
        if rows.len() == 1 { "reference" } else { "references" },
        files,
        if files == 1 { "file" } else { "files" },
    ));

    let mut hovered = None;
    let mut last_path: Option<&Path> = None;
    for row in rows {
        if last_path != Some(row.path.as_path()) {
            last_path = Some(&row.path);
            let shown = state
                .loaded_dir
                .as_ref()
                .and_then(|dir| row.path.strip_prefix(dir).ok())
                .unwrap_or(&row.path);
            ui.add_space(4.0);
            ui.label(egui::RichText::new(shown.display().to_string()).strong());
        }
        let label = egui::RichText::new(format!("L{}  {}", row.line + 1, row.preview)).monospace();
        let response = ui.selectable_label(false, label);
        if response.hovered() {
            hovered = open_node(state, &row.path).map(|node_id| LineHighlight {
                node_id,
                start_line: row.line,
                end_line: row.line,
            });
        }
        if response.clicked() {
            *action = Some(Action::Focus(row.path.clone(), row.line));
        }
    }
    if hovered.is_some() {
        state.lsp.hovered = hovered;
    }
}

fn open_node(state: &AppState, path: &Path) -> Option<u64> {
    state
        .node_store
        .file_nodes
        .values()
        .find(|n| n.path == path)
        .map(|n| n.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::lsp::client::Connection;
    use crate::analysis::lsp::manager::Launcher;
    use crate::analysis::lsp::{Position, Range, SymbolKind};
    use std::sync::{Arc, Mutex};

    fn symbol(name: &str, line: usize) -> LspSymbol {
        let pos = Position { line, character: 3 };
        LspSymbol {
            name: name.into(),
            detail: None,
            kind: SymbolKind::Function,
            range: Range { start: pos, end: Position { line: line + 2, character: 1 } },
            selection_range: Range { start: pos, end: pos },
            children: Vec::new(),
        }
    }

    /// A launcher that records every request and never answers.
    fn recording_launcher(requests: Arc<Mutex<usize>>) -> Launcher {
        Arc::new(move |_, _| {
            *requests.lock().unwrap() += 1;
            Err("not started".into())
        })
    }

    #[test]
    fn sync_symbols_requests_once_per_node_and_content() {
        let starts = Arc::new(Mutex::new(0));
        let mut lsp = LspPanelState {
            manager: LspManager::with_launcher(recording_launcher(starts)),
            ..Default::default()
        };
        let root = Path::new("/p");
        let path = Path::new("/p/a.rs");
        lsp.sync_symbols(None, root, 1, path, "fn a() {}");
        let first = lsp.symbols().cloned();
        lsp.sync_symbols(None, root, 1, path, "fn a() {}");
        assert_eq!(lsp.symbols().cloned(), first, "same request still pending");

        lsp.sync_symbols(None, root, 1, path, "fn b() {}");
        assert_ne!(lsp.symbols().cloned(), first, "new content, new request");

        lsp.sync_symbols(None, root, 2, Path::new("/p/README.md"), "# hi");
        assert!(lsp.symbols().is_none(), "no server for Markdown");
    }

    #[test]
    fn poll_keeps_only_answers_to_the_latest_requests() {
        let mut lsp = LspPanelState {
            manager: LspManager::with_launcher(Arc::new(|_, _| -> Result<Connection, String> {
                Err("boom".into())
            })),
            ..Default::default()
        };
        let root = Path::new("/p");
        lsp.sync_symbols(None, root, 1, Path::new("/p/a.rs"), "a");
        lsp.sync_symbols(None, root, 1, Path::new("/p/a.rs"), "b"); // the first one is stale
        lsp.find_references(None, root, 1, Path::new("/p/a.rs"), "b", &symbol("a", 0));

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while lsp.is_waiting() && std::time::Instant::now() < deadline {
            lsp.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(lsp.symbols(), Some(&Fetch::Done(Err("boom".into()))));
        let refs = lsp.references.as_ref().unwrap();
        assert_eq!((refs.symbol.as_str(), refs.line), ("a", 0));
        assert_eq!(refs.fetch, Fetch::Done(Err("boom".into())));
    }

    #[test]
    fn reference_rows_read_the_line_text() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.rs");
        std::fs::write(&a, "fn hello() {}\n\n    hello();   \n").unwrap();
        let loc = |path: &Path, line| Location {
            path: path.to_path_buf(),
            range: Range { start: Position { line, character: 0 }, end: Position::default() },
        };
        let rows = reference_rows(&[loc(&a, 0), loc(&a, 2), loc(&a, 9), loc(Path::new("/missing.rs"), 0)]);
        let previews: Vec<&str> = rows.iter().map(|r| r.preview.as_str()).collect();
        assert_eq!(previews, ["fn hello() {}", "hello();", "", ""]);
        assert_eq!(rows[1].line, 2);
    }

    #[test]
    fn symbol_label_shows_kind_name_and_one_based_line() {
        assert_eq!(symbol_label(&symbol("main", 2)), "fn main  L3");
        let block = LspSymbol { name: "impl Config".into(), kind: SymbolKind::Other, ..symbol("x", 7) };
        assert_eq!(symbol_label(&block), "impl Config  L8");
    }
}
