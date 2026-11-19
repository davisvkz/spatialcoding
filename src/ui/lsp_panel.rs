//! Right-sidebar "LSP" tab: the Symbol Graph of the pinned symbols (grouped
//! results, files and counts) and the language server's symbols for the
//! primary selected file node. Clicking a symbol row pins it exactly like a
//! click on the canvas; resting on one with Query On Hover pins a preview;
//! clicking a result focuses its line on the canvas.
//!
//! Requests go through [`LspManager`], so the panel never waits on a server:
//! it shows "Loading…" and picks the answer up on a later frame.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::analysis::lsp::manager::{LspManager, LspRequest, LspResponse, RequestId, ServerStatus};
use crate::analysis::lsp::install::InstallState;
use crate::analysis::lsp::{server_for, Availability, LspSymbol, ServerId};
use crate::state::AppState;
use crate::ui::icons;
use crate::workspace;

const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(0xef, 0x44, 0x44);

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

#[derive(Default)]
pub struct LspPanelState {
    pub manager: LspManager,
    /// The Open importers action waiting for the server's references.
    pub importers: Option<crate::importers::ImporterQuery>,
    /// Symbols pinned by a click (canvas or panel row), each with its full
    /// graph, and the hover preview of Query On Hover.
    pub pins: crate::ui::symbol_pins::PinSet,
    /// App time until which Dim Dependency Edges stays on (see
    /// [`DIM_RESTORE_DELAY`]).
    pub dim_until: f64,
    symbols: Option<SymbolsEntry>,
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
    /// Language Servers dialog: files per language, most first.
    pub language_counts: Vec<(String, usize)>,
    /// Downloads of language servers started from the dialog (TASK-56).
    pub installer: crate::analysis::lsp::install::Installer,
}

impl LspPanelState {
    /// Forgets a symbols answer that was an error (a server that was missing
    /// then), so the next sync asks again.
    pub fn retry_failed_symbols(&mut self) {
        if self.symbols.as_ref().is_some_and(|s| matches!(s.fetch, Fetch::Done(Err(_)))) {
            self.symbols = None;
        }
    }

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

    /// Clear Symbols: forgets the pins and the jump target.
    pub fn clear_symbols(&mut self) {
        self.pins.clear();
        self.target = None;
        self.hovered = None;
    }

    /// Takes an answer of the shared [`LspManager`] if it is the one the tab
    /// waits for; any other answer (stale, or another requester's) is handed
    /// back.
    pub fn receive(&mut self, id: RequestId, response: LspResponse) -> Option<LspResponse> {
        if let Some(query) = self.importers.as_mut()
            && query.receive(id, &response)
        {
            return None;
        }
        let response = self.pins.receive(id, response)?;
        match response {
            LspResponse::SymbolGraph(result) => return Some(LspResponse::SymbolGraph(result)),
            LspResponse::Symbols(result) => match &mut self.symbols {
                Some(entry) if entry.fetch == Fetch::Pending(id) => {
                    entry.fetch = Fetch::Done(result)
                }
                _ => return Some(LspResponse::Symbols(result)),
            },
            LspResponse::References(result) => return Some(LspResponse::References(result)),
        }
        None
    }

    /// True while an answer is on its way.
    pub fn is_waiting(&self) -> bool {
        self.pins.is_waiting()
            || self.importers.as_ref().is_some_and(|q| q.is_waiting())
            || self.symbols.as_ref().is_some_and(|s| s.fetch.is_pending())
    }
}

/// Seconds the dim of Dependency Edges outlasts the last active symbol.
pub const DIM_RESTORE_DELAY: f64 = 0.5;

/// What a click in the panel asked for, applied after drawing.
enum Action {
    /// A symbol row clicked (Shift keeps the other pins).
    Pin(LspSymbol, bool),
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
                let active = !state.selection.symbols().is_empty()
                    || !state.lsp.pins.is_empty();
                // "Clear 2 Symbols" while symbols are pinned, as v2.
                let pinned = state.lsp.pins.len();
                let name = match pinned {
                    0 => "Clear Symbols".to_string(),
                    1 => "Clear 1 Symbol".to_string(),
                    n => format!("Clear {n} Symbols"),
                };
                let clear = ui.add_enabled(active, egui::Button::new(format!("{}  {name}", icons::CLOSE)).small());
                let accessible = name.clone();
                clear.widget_info(move || {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, active, accessible.clone())
                });
                if clear.clicked() {
                    action = Some(Action::Clear);
                }
            });
        });
        if !state.lsp.pins.has_pinned() {
            ui.label(egui::RichText::new(SYMBOL_GRAPH_HINT).small().weak());
        }
        draw_pins(ui, state, &mut action);
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

    if state.lsp.is_waiting() {
        // The worker repaints when an answer arrives; this keeps the server
        // status line current while it starts or indexes.
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }

    match action {
        Some(Action::Pin(symbol, additive)) => {
            let Some(node_id) = state.selection.primary else {
                return;
            };
            pin_symbol(ui.ctx(), state, node_id, &symbol, additive);
        }
        Some(Action::Requery(node_id, symbol)) => {
            pin_symbol(ui.ctx(), state, node_id, &symbol, false);
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

/// Pinned Symbols, Files, the Results and Edges counts and the grouped
/// results of the pins (the reference's Symbol Graph panel).
fn draw_pins(ui: &mut egui::Ui, state: &mut AppState, action: &mut Option<Action>) {
    use crate::analysis::lsp::graph::Group;
    use crate::ui::symbol_pins::PinFetch;
    // A hover preview is drawn on the canvas only: listing it here would shift
    // the rows under the pointer and end the hover that made it.
    if !state.lsp.pins.has_pinned() {
        return;
    }
    let mut remove = None;
    ui.add_space(4.0);
    ui.label(egui::RichText::new(format!("Pinned Symbols ({})", state.lsp.pins.len())).strong());
    for pin in state.lsp.pins.pins().iter().filter(|p| !p.preview) {
        ui.horizontal(|ui| {
            let row = ui.add(egui::Label::new(egui::RichText::new(format!("{}  {}", pin.name, pin.label())).monospace().small()).sense(egui::Sense::click()));
            if row.clicked() {
                *action = Some(Action::Focus(pin.path.clone(), pin.position.line));
            }
            match &pin.fetch {
                PinFetch::Pending(_) => {
                    ui.weak("…");
                }
                PinFetch::Failed(e) => {
                    ui.colored_label(egui::Color32::from_rgb(0xef, 0x44, 0x44), "failed").on_hover_text(e);
                }
                PinFetch::Done(_) => {}
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕").on_hover_text("Remove this pin").clicked() {
                    remove = Some(pin.id);
                }
            });
        });
    }
    if let Some(id) = remove {
        state.lsp.pins.remove(id);
    }
    if state.lsp.pins.pins().iter().filter(|p| !p.preview).all(|p| p.graph().is_none()) {
        return;
    }
    let files = state.lsp.pins.files();
    let (results, edges) = state.lsp.pins.totals();
    ui.add_space(4.0);
    ui.label(egui::RichText::new(format!("Files ({})", files.len())).strong());
    for (path, count) in &files {
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        ui.label(egui::RichText::new(format!("{name}  {count}")).small());
    }
    ui.add_space(4.0);
    ui.label(egui::RichText::new(format!("Results ({results}) / Edges ({edges})")).strong());
    for group in Group::ALL {
        let mut rows: Vec<(crate::analysis::lsp::graph::Role, PathBuf, usize)> = Vec::new();
        for graph in state.lsp.pins.pins().iter().filter(|p| !p.preview).filter_map(|p| p.graph()) {
            rows.extend(graph.group(group).into_iter().map(|e| (e.role, e.path.clone(), e.range.start.line)));
        }
        if rows.is_empty() {
            continue;
        }
        egui::CollapsingHeader::new(format!("{} ({})", group.title(), rows.len()))
            .id_salt(("pin_group", group.title()))
            .default_open(true)
            .show(ui, |ui| {
                for (role, path, line) in rows {
                    let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    let text = format!("{}  {name}:{}", role.label(), line + 1);
                    let [r, g, b] = role.color();
                    let response = ui.add(
                        egui::Label::new(egui::RichText::new(text).monospace().small().color(egui::Color32::from_rgb(r, g, b)))
                            .sense(egui::Sense::click()),
                    );
                    if response.clicked() {
                        *action = Some(Action::Focus(path, line));
                    }
                }
            });
    }
}

/// Pins a symbol of the list (the same graph a canvas click asks for),
/// records it in the history and selects it on the canvas.
fn pin_symbol(ctx: &egui::Context, state: &mut AppState, node_id: u64, symbol: &LspSymbol, additive: bool) {
    use crate::ui::symbol_pins::{pin_at, PinMode};
    let Some(node) = state.node_store.get_file(node_id) else {
        return;
    };
    let content = node.content.clone();
    let mode = if additive { PinMode::Add } else { PinMode::Replace };
    state.lsp.pins.mark_hinted();
    pin_at(Some(ctx), state, node_id, symbol.selection_range.start, symbol.name.clone(), mode);
    state.lsp.push_history(node_id, symbol);
    if let Some(selected) = crate::nodes::selection::SelectedSymbol::from_lsp(node_id, &content, symbol) {
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

    // The row of a pinned symbol is marked.
    let selected = state
        .lsp
        .pins
        .pins()
        .iter()
        .rev()
        .find(|p| p.node_id == node_id && !p.preview)
        .map(|p| (p.name.clone(), p.position.line));
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
            ui.weak("Click a symbol to pin its graph.");
            let id = ui.make_persistent_id(("lsp-symbols", node_id));
            for (i, symbol) in symbols.iter().enumerate() {
                draw_symbol(ui, symbol, id.with(i), &selected, &mut hovered, action);
            }
        }
    }
    // Query On Hover: a row rested on for 400 ms pins a preview of its graph.
    let now = ui.input(|i| i.time);
    let token = hovered.as_ref().map(|(_, _, symbol)| (node_id, symbol.selection_range.start, symbol.name.clone()));
    if (token.is_some() || ui.ui_contains_pointer())
        && let Some(wait) = crate::ui::symbol_pins::update_hover(Some(ui.ctx()), state, token, now)
    {
        ui.ctx().request_repaint_after(Duration::from_secs_f64(wait));
    }
    state.lsp.hovered = hovered.map(|(start_line, end_line, _)| LineHighlight {
        node_id,
        start_line,
        end_line,
    });
}

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
        *action = Some(Action::Pin(
            symbol.clone(),
            ui.input(|i| i.modifiers.shift),
        ));
    }
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
                        .on_hover_text("Rest on a symbol for 0.4 s, on the canvas or in the list: its graph shows as a preview until you move away or click it.");
                    switch(ui, &mut lsp.auto_open_matches, "Auto Open Matches")
                        .on_hover_text("Open the files holding a pinned symbol's results when its graph arrives.");
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

/// The Install / Retry button of a server's row; disabled, with the reason
/// as its tooltip, when the machine cannot install it. True when clicked.
fn install_button(ui: &mut egui::Ui, state: &mut AppState, server: ServerId, label: &str) -> bool {
    let blocker = state.lsp.installer.blocker(server);
    let enabled = blocker.is_none();
    let response = ui.add_enabled(enabled, egui::Button::new(label).small());
    let response = match &blocker {
        Some(why) => response.on_disabled_hover_text(why),
        None => response.on_hover_text(format!(
            "Download the {} language server into {}",
            server.name(),
            state.lsp.installer.dir().display()
        )),
    };
    // One accessible name per row, so tests and screen readers can tell
    // the buttons apart.
    let name = format!("{label} {}", server.name());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name.clone()));
    response.clicked()
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
        .default_width(600.0)
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
                        state.lsp.installer.refresh_platform();
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
            let ctx = ui.ctx().clone();
            let mut to_install: Option<ServerId> = None;
            let mut to_remove: Option<ServerId> = None;
            egui::Grid::new("language_servers").num_columns(4).striped(true).show(ui, |ui| {
                for server in ServerId::ALL {
                    let availability = state.lsp.installer.availability(server);
                    ui.label(egui::RichText::new(server.name()).strong());
                    match server.command() {
                        Ok(cmd) => {
                            let text = cmd.join(" ");
                            ui.add(egui::Label::new(egui::RichText::new(&text).monospace()).truncate())
                                .on_hover_text(text);
                        }
                        Err(err) if availability == Availability::Missing => {
                            ui.colored_label(ui.visuals().weak_text_color(), "not found")
                                .on_hover_text(err);
                        }
                        Err(err) => {
                            ui.colored_label(ui.visuals().weak_text_color(), err);
                        }
                    };
                    let status = match state.lsp.manager.status(server, &root) {
                        None => "Not started".to_owned(),
                        Some(ServerStatus::Starting) => "Starting…".to_owned(),
                        Some(ServerStatus::Indexing) => "Indexing…".to_owned(),
                        Some(ServerStatus::Ready) => "Ready".to_owned(),
                        Some(ServerStatus::Failed(e)) => format!("Failed: {e}"),
                    };
                    ui.label(status);
                    ui.horizontal(|ui| {
                        match state.lsp.installer.state(server) {
                            Some(InstallState::Downloading { received, total }) => {
                                ui.spinner();
                                ui.label(match total {
                                    Some(total) if total > 0 => format!("Downloading {}%", received * 100 / total),
                                    _ => format!("Downloading {:.1} MB", received as f64 / 1e6),
                                });
                            }
                            Some(InstallState::Extracting) => {
                                ui.spinner();
                                ui.label("Extracting…");
                            }
                            Some(InstallState::Building) => {
                                ui.spinner();
                                ui.label("Building…");
                            }
                            Some(InstallState::Failed(error)) => {
                                ui.colored_label(ERROR_COLOR, "Failed").on_hover_text(error);
                                if install_button(ui, state, server, "Retry") {
                                    to_install = Some(server);
                                }
                            }
                            None => match availability {
                                Availability::Override => {
                                    ui.label("Custom command");
                                }
                                Availability::Path => {
                                    ui.label("Installed (PATH)");
                                }
                                Availability::Managed => {
                                    ui.label("Installed (SpatialCoding)");
                                    let remove = ui
                                        .small_button("Remove")
                                        .on_hover_text("Delete the copy SpatialCoding downloaded");
                                    if remove.clicked() {
                                        to_remove = Some(server);
                                    }
                                }
                                Availability::Missing => {
                                    ui.label("Not installed");
                                    if install_button(ui, state, server, "Install") {
                                        to_install = Some(server);
                                    }
                                }
                            },
                        }
                    });
                    ui.end_row();
                }
            });
            if let Some(server) = to_install {
                state.lsp.installer.install(server, Some(ctx));
            }
            if let Some(server) = to_remove {
                match state.lsp.installer.remove(server) {
                    Ok(()) => state.lsp.manager.restart_servers(),
                    Err(e) => crate::ui::toast::push(state, format!("Could not remove the {} server: {e}", server.name())),
                }
            }
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
