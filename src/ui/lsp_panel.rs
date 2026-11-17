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
use crate::ui::icons;
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
    /// Auto Open Matches already ran for this answer.
    pub auto_opened: bool,
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
    /// Symbols queried, oldest first, and the one shown (back/forward).
    history: Vec<(u64, LspSymbol)>,
    history_pos: usize,
    /// The LSP Advanced Settings card is open.
    pub show_advanced: bool,
    /// The Language Servers dialog is open, and whether it counts the
    /// files on the canvas instead of the repository's.
    pub show_servers: bool,
    pub servers_count_canvas: bool,
    /// Query On Hover: the symbol row under the pointer and since when.
    hover_since: Option<(u64, usize, f64)>,
    /// Language Servers dialog: files per language, most first.
    pub language_counts: Vec<(String, usize)>,
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
            auto_opened: false,
        });
    }

    /// Records a queried symbol: drops the forward part, then appends.
    fn push_history(&mut self, node_id: u64, symbol: &LspSymbol) {
        let same = self.history.get(self.history_pos).is_some_and(|(n, s)| {
            *n == node_id && s.selection_range.start == symbol.selection_range.start
        });
        if same {
            return;
        }
        if !self.history.is_empty() {
            self.history.truncate(self.history_pos + 1);
        }
        self.history.push((node_id, symbol.clone()));
        self.history_pos = self.history.len() - 1;
    }

    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty() && self.history_pos > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.history_pos + 1 < self.history.len()
    }

    /// Moves through the history; returns the symbol to query again.
    fn step_history(&mut self, forward: bool) -> Option<(u64, LspSymbol)> {
        if forward && self.can_go_forward() {
            self.history_pos += 1;
        } else if !forward && self.can_go_back() {
            self.history_pos -= 1;
        } else {
            return None;
        }
        self.history.get(self.history_pos).cloned()
    }

    /// Clear Symbols: forgets the references and the jump target.
    pub fn clear_symbols(&mut self) {
        self.references = None;
        self.target = None;
        self.hovered = None;
    }

    /// Takes an answer of the shared [`LspManager`] if it is the one the tab
    /// waits for; any other answer (stale, or another requester's) is handed
    /// back.
    pub fn receive(&mut self, id: RequestId, response: LspResponse) -> Option<LspResponse> {
        match response {
            LspResponse::Symbols(result) => match &mut self.symbols {
                Some(entry) if entry.fetch == Fetch::Pending(id) => {
                    entry.fetch = Fetch::Done(result)
                }
                _ => return Some(LspResponse::Symbols(result)),
            },
            LspResponse::References(result) => match &mut self.references {
                Some(refs) if matches!(refs.fetch, Fetch::Pending(p) if p == id) => {
                    refs.fetch = Fetch::Done(result.map(|locs| reference_rows(&locs)));
                }
                _ => return Some(LspResponse::References(result)),
            },
        }
        None
    }

    /// True while an answer is on its way.
    pub fn is_waiting(&self) -> bool {
        self.symbols.as_ref().is_some_and(|s| s.fetch.is_pending())
            || self
                .references
                .as_ref()
                .is_some_and(|r| r.fetch.is_pending())
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
    FindReferences(LspSymbol, bool),
    /// Query On Hover: references only, no selection change.
    HoverQuery(u64, LspSymbol),
    /// Back/forward in the history.
    Requery(u64, LspSymbol),
    Clear,
    Focus(PathBuf, usize),
}

/// v2's empty-state hint of the Symbol Graph.
const SYMBOL_GRAPH_HINT: &str =
    "Click a symbol to show LSP graph results. Shift click to select multiple symbols.";

pub fn draw_lsp_panel(ui: &mut egui::Ui, state: &mut AppState) {
    state.lsp.hovered = None;
    let mut action = None;

    crate::ui::panel::section(ui, "Symbol Graph", |ui| {
        ui.horizontal(|ui| {
            let back = ui
                .add_enabled_ui(state.lsp.can_go_back(), |ui| {
                    icons::tool_icon_button(ui, icons::CHEVRON_LEFT, "Previous symbol")
                })
                .inner;
            if back.clicked() {
                action = state.lsp.step_history(false).map(|(n, s)| Action::Requery(n, s));
            }
            let forward = ui
                .add_enabled_ui(state.lsp.can_go_forward(), |ui| {
                    icons::tool_icon_button(ui, icons::CHEVRON_RIGHT, "Next symbol")
                })
                .inner;
            if forward.clicked() {
                action = state.lsp.step_history(true).map(|(n, s)| Action::Requery(n, s));
            }
            let gear_label = if state.lsp.show_advanced {
                "Hide advanced settings"
            } else {
                "Show advanced settings"
            };
            if icons::tool_icon_button(ui, icons::SETTINGS, gear_label).clicked() {
                state.lsp.show_advanced = !state.lsp.show_advanced;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let active = state.lsp.references.is_some() || !state.selection.symbols().is_empty();
                let clear = ui.add_enabled(
                    active,
                    egui::Button::new(format!("{}  Clear Symbols", icons::CLOSE)).small(),
                );
                clear.widget_info(move || {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, active, "Clear Symbols")
                });
                if clear.clicked() {
                    action = Some(Action::Clear);
                }
            });
        });
        if state.lsp.references.is_none() {
            ui.label(egui::RichText::new(SYMBOL_GRAPH_HINT).small().weak());
        }
    });

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
    if state.lsp.hover_since.is_some() {
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }

    match action {
        Some(Action::FindReferences(symbol, additive)) => {
            let Some(node_id) = state.selection.primary else {
                return;
            };
            query_symbol(ui.ctx(), state, node_id, &symbol, true, additive);
        }
        Some(Action::HoverQuery(node_id, symbol)) => {
            query_symbol(ui.ctx(), state, node_id, &symbol, false, false);
        }
        Some(Action::Requery(node_id, symbol)) => {
            query_symbol(ui.ctx(), state, node_id, &symbol, true, false);
        }
        Some(Action::Clear) => {
            state.lsp.clear_symbols();
            state.selection.clear_symbols();
        }
        Some(Action::Focus(path, line)) => {
            workspace::focus_location(state, &path, line);
        }
        None => {}
    }
}

/// Asks for a symbol's references, records it in the history and, unless
/// it only came from hovering, selects it on the canvas.
fn query_symbol(
    ctx: &egui::Context,
    state: &mut AppState,
    node_id: u64,
    symbol: &LspSymbol,
    select: bool,
    additive: bool,
) {
    let Some(node) = state.node_store.get_file(node_id) else {
        return;
    };
    let root = workspace_root(state, &node.path);
    let (path, content) = (node.path.clone(), node.content.clone());
    state.lsp.find_references(Some(ctx), &root, node_id, &path, &content, symbol);
    state.lsp.push_history(node_id, symbol);
    if select
        && let Some(selected) =
            crate::nodes::selection::SelectedSymbol::from_lsp(node_id, &content, symbol)
    {
        state.selection.select_symbol(node_id, selected, additive);
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

fn draw_symbols(
    ui: &mut egui::Ui,
    state: &mut AppState,
    node_id: u64,
    action: &mut Option<Action>,
) {
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
    let mut hovered: Option<(usize, usize, LspSymbol)> = None;
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
    // Query On Hover: a row rested on for 400 ms asks for its references.
    let now = ui.input(|i| i.time);
    match (&hovered, state.settings.lsp.query_on_hover) {
        (Some((_, _, symbol)), true) => {
            let key = (node_id, symbol.selection_range.start.line);
            let since = match state.lsp.hover_since {
                Some((n, l, t)) if (n, l) == key => t,
                _ => now,
            };
            state.lsp.hover_since = Some((key.0, key.1, since));
            let current = state
                .lsp
                .references
                .as_ref()
                .is_some_and(|r| r.node_id == node_id && r.line == key.1);
            if now - since >= HOVER_QUERY_SECONDS && !current && action.is_none() {
                *action = Some(Action::HoverQuery(node_id, symbol.clone()));
            }
        }
        _ => state.lsp.hover_since = None,
    }
    state.lsp.hovered = hovered.map(|(start_line, end_line, _)| LineHighlight {
        node_id,
        start_line,
        end_line,
    });
}

/// How long the pointer rests on a symbol before Query On Hover asks.
const HOVER_QUERY_SECONDS: f64 = 0.4;

fn draw_status(ui: &mut egui::Ui, manager: &LspManager, server: ServerId, root: &Path) {
    match manager.status(server, root) {
        Some(ServerStatus::Starting) => {
            ui.weak(format!(
                "Starting the {} language server\u{2026}",
                server.name()
            ));
        }
        Some(ServerStatus::Indexing) => {
            ui.weak(format!(
                "The {} language server is indexing\u{2026}",
                server.name()
            ));
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
    hovered: &mut Option<(usize, usize, LspSymbol)>,
    action: &mut Option<Action>,
) {
    let is_selected = selected.as_ref().is_some_and(|(name, line)| {
        *name == symbol.name && *line == symbol.selection_range.start.line
    });
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
        *hovered = Some((symbol.range.start.line, symbol.range.end.line, symbol.clone()));
    }
    if response.clicked() {
        *action = Some(Action::FindReferences(
            symbol.clone(),
            ui.input(|i| i.modifiers.shift),
        ));
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
        if rows.len() == 1 {
            "reference"
        } else {
            "references"
        },
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

/// Width of the floating LSP Advanced Settings card.
const CARD_WIDTH: f32 = 290.0;

/// The floating "LSP Advanced Settings" card, left of the docked panel,
/// while the panel's gear has it open.
pub fn draw_advanced_card(ctx: &egui::Context, state: &mut AppState) {
    if !state.lsp.show_advanced || state.right_panel != Some(crate::state::RightPanel::LspSymbols) {
        return;
    }
    use crate::ui::panel::{section, slider_row, switch, PANEL_WIDTH, RAIL_WIDTH};
    let screen = ctx.screen_rect();
    let pos = egui::pos2(screen.right() - RAIL_WIDTH - PANEL_WIDTH - CARD_WIDTH - 8.0, screen.top() + 40.0);
    let theme = state.theme.clone();
    egui::Area::new(egui::Id::new("lsp_advanced_card"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            crate::ui::panel::bubble_frame(&theme).show(ui, |ui| {
                ui.set_width(CARD_WIDTH - 16.0);
                ui.horizontal(|ui| {
                    ui.label(icons::icon(icons::SETTINGS, 14.0));
                    ui.label(egui::RichText::new("LSP Advanced Settings").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::tool_icon_button(ui, icons::CLOSE, "Close LSP Advanced Settings").clicked() {
                            state.lsp.show_advanced = false;
                        }
                    });
                });
                ui.separator();
                let lsp = &mut state.settings.lsp;
                section(ui, "Query", |ui| {
                    switch(ui, &mut lsp.query_on_hover, "Query On Hover")
                        .on_hover_text("Rest the pointer on a symbol to list its references.");
                    switch(ui, &mut lsp.auto_open_matches, "Auto Open Matches")
                        .on_hover_text("Open the files holding the references when they arrive.");
                    ui.label("Max");
                    ui.horizontal(|ui| {
                        let response = ui.add(egui::DragValue::new(&mut lsp.max_files).range(1..=500usize));
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Max files")
                        });
                        ui.weak("files");
                    });
                });
                section(ui, "Navigation", |ui| {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut lsp.keep_zoom, "Keep Zoom")
                            .on_hover_text("Jumping to a reference keeps the zoom.");
                        ui.checkbox(&mut lsp.smooth_transition, "Smooth Transition")
                            .on_hover_text("Jumping to a reference glides to it.");
                    });
                });
                section(ui, "Symbol Edges", |ui| {
                    switch(ui, &mut lsp.dim_edges, "Dim Dependency Edges")
                        .on_hover_text("While a symbol is active, dim the edges not linked to it.");
                    ui.add_enabled_ui(lsp.dim_edges, |ui| {
                        slider_row(ui, "Dim %", &mut lsp.dim_pct, 0.0..=1.0, |v| format!("{:.0}%", v * 100.0));
                    });
                    switch(ui, &mut lsp.reveal_through_collapse, "Reveal Through Collapse")
                        .on_hover_text("Files holding the symbol or its references stay visible in collapsed folders.");
                    switch(ui, &mut lsp.out_of_view, "Out of View")
                        .on_hover_text("Mark references outside the view on the canvas border.");
                });
            });
        });
}

/// The "Language Servers" dialog of the LSP kebab: file counts by
/// language, each server's command and status.
pub fn draw_language_servers(ctx: &egui::Context, state: &mut AppState) {
    if !state.lsp.show_servers {
        return;
    }
    let mut open = true;
    egui::Window::new("Language Servers")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_width(460.0)
        .open(&mut open)
        .show(ctx, |ui| {
            let canvas = state.lsp.servers_count_canvas;
            let paths: Vec<PathBuf> = if canvas {
                state.node_store.file_nodes.values().map(|n| n.path.clone()).collect()
            } else {
                workspace::folder_text_files(state).iter().cloned().collect()
            };
            let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
            for path in &paths {
                *counts.entry(crate::nodes::file_node::detect_language(path)).or_default() += 1;
            }
            let mut counts: Vec<(String, usize)> = counts.into_iter().collect();
            counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            state.lsp.language_counts = counts;
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.weak(if canvas { "Canvas files" } else { "Repository files" });
                    ui.heading(paths.len().to_string());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::tool_icon_button(ui, icons::REFRESH, "Refresh language servers").clicked() {
                        state.folder_files_cache.replace(None);
                    }
                    let mut mode = state.lsp.servers_count_canvas;
                    ui.allocate_ui(egui::vec2(170.0, 26.0), |ui| {
                        crate::ui::panel::segmented(ui, &mut mode, &[(false, "Repository"), (true, "Canvas")]);
                    });
                    state.lsp.servers_count_canvas = mode;
                });
            });
            ui.horizontal_wrapped(|ui| {
                for (language, count) in state.lsp.language_counts.iter().take(12) {
                    ui.label(format!("\u{25cf} {language} {count}"));
                }
            });
            ui.separator();
            let root = state.loaded_dir.clone().unwrap_or_default();
            egui::Grid::new("language_servers").num_columns(3).striped(true).show(ui, |ui| {
                for server in ServerId::ALL {
                    ui.label(egui::RichText::new(server.name()).strong());
                    match server.command() {
                        Ok(cmd) => ui.monospace(cmd.join(" ")),
                        Err(err) => ui.colored_label(ui.visuals().weak_text_color(), err),
                    };
                    let status = match state.lsp.manager.status(server, &root) {
                        None => "Not started".to_owned(),
                        Some(ServerStatus::Starting) => "Starting…".to_owned(),
                        Some(ServerStatus::Indexing) => "Indexing…".to_owned(),
                        Some(ServerStatus::Ready) => "Ready".to_owned(),
                        Some(ServerStatus::Failed(e)) => format!("Failed: {e}"),
                    };
                    ui.label(status);
                    ui.end_row();
                }
            });
            ui.weak("Override a server's command with its SPATIALCODING_LSP_* variable (see the docs).");
            ui.separator();
            let mut env = false;
            crate::ui::panel::disabled_switch(
                ui,
                &mut env,
                "Load project environment",
                "Servers start with SpatialCoding's own environment; loading the project's shell, \
                 version managers or direnv is not supported here.",
            );
        });
    if !open {
        state.lsp.show_servers = false;
    }
}

/// For each open file holding references whose node is outside the canvas
/// view: the node's screen center and how many references it holds.
pub fn out_of_view_markers(state: &AppState, canvas_rect: egui::Rect) -> Vec<(egui::Pos2, usize)> {
    let Some(References { fetch: Fetch::Done(Ok(rows)), .. }) = &state.lsp.references else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for path in reference_files(state) {
        let Some(node) = state.node_store.file_nodes.values().find(|n| n.path == path) else { continue };
        let rect = state
            .viewport
            .world_to_screen_rect(node.world_rect(state.settings.node_width, state.settings.font_size));
        if !rect.intersects(canvas_rect) {
            out.push((rect.center(), rows.iter().filter(|r| r.path == path).count()));
        }
    }
    out
}

/// Paths of the files holding the current references, deduplicated in order.
pub fn reference_files(state: &AppState) -> Vec<PathBuf> {
    let Some(References { fetch: Fetch::Done(Ok(rows)), .. }) = &state.lsp.references else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for row in rows {
        if !out.contains(&row.path) {
            out.push(row.path.clone());
        }
    }
    out
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
            range: Range {
                start: pos,
                end: Position {
                    line: line + 2,
                    character: 1,
                },
            },
            selection_range: Range {
                start: pos,
                end: pos,
            },
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
    fn receive_keeps_only_answers_to_the_latest_requests() {
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
        let mut handed_back = 0;
        while lsp.is_waiting() && std::time::Instant::now() < deadline {
            for (id, response) in lsp.manager.poll() {
                handed_back += usize::from(lsp.receive(id, response).is_some());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(handed_back, 1, "the stale symbols answer is not the tab's");
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
            range: Range {
                start: Position { line, character: 0 },
                end: Position::default(),
            },
        };
        let rows = reference_rows(&[
            loc(&a, 0),
            loc(&a, 2),
            loc(&a, 9),
            loc(Path::new("/missing.rs"), 0),
        ]);
        let previews: Vec<&str> = rows.iter().map(|r| r.preview.as_str()).collect();
        assert_eq!(previews, ["fn hello() {}", "hello();", "", ""]);
        assert_eq!(rows[1].line, 2);
    }

    #[test]
    fn symbol_label_shows_kind_name_and_one_based_line() {
        assert_eq!(symbol_label(&symbol("main", 2)), "fn main  L3");
        let block = LspSymbol {
            name: "impl Config".into(),
            kind: SymbolKind::Other,
            ..symbol("x", 7)
        };
        assert_eq!(symbol_label(&block), "impl Config  L8");
    }
}
