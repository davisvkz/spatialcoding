//! Pinned symbols: a click on the canvas asks the language server for the
//! whole graph of the symbol and pins it; Shift+click pins several, Shift+C
//! (or Clear) drops them, and each pin can be removed alone.

use std::path::{Path, PathBuf};

use crate::analysis::lsp::graph::SymbolGraph;
use crate::analysis::lsp::manager::{LspRequest, LspResponse, RequestId};
use crate::analysis::lsp::{server_for, Position};
use crate::analysis::symbol_mapping::utf16_column;
use crate::state::AppState;

/// The hint shown the first time a symbol is pinned.
pub const PIN_HINT: &str = "Shift+click to pin multiple symbols. Shift+C to clear all.";

#[derive(Debug, Clone, PartialEq)]
pub enum PinFetch {
    Pending(RequestId),
    Done(SymbolGraph),
    Failed(String),
}

/// One pinned symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct Pin {
    pub id: u64,
    /// The file node the click was on.
    pub node_id: u64,
    pub path: PathBuf,
    /// The token text (`kindOf`).
    pub name: String,
    pub position: Position,
    pub fetch: PinFetch,
    /// Asked without an egui context: nothing wakes the UI when the answer
    /// arrives, so the shell polls while it waits.
    pub poll: bool,
    /// A transient pin made by resting on a symbol (Query On Hover): it goes
    /// when the pointer leaves, and a click makes it permanent.
    pub preview: bool,
    /// Auto Open Matches already ran for this answer.
    pub auto_opened: bool,
}

/// How a new pin relates to the ones already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinMode {
    /// A plain click: the only pin.
    Replace,
    /// Shift+click: one more (or toggled off when already pinned).
    Add,
    /// Query On Hover: transient, silent, never replaces a real pin.
    Preview,
}

/// How long the pointer rests on a symbol before Query On Hover asks.
pub const HOVER_SECONDS: f64 = 0.4;

impl Pin {
    pub fn graph(&self) -> Option<&SymbolGraph> {
        match &self.fetch {
            PinFetch::Done(graph) => Some(graph),
            _ => None,
        }
    }

    /// `path:line` with a 1-based line, as the Pinned Symbols list shows it.
    pub fn label(&self) -> String {
        let file = self.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        format!("{file}:{}", self.position.line + 1)
    }
}

#[derive(Default)]
pub struct PinSet {
    pins: Vec<Pin>,
    next_id: u64,
    /// The pin tip was shown already.
    hinted: bool,
    /// Query On Hover: the token the pointer rests on and since when.
    hover: Option<(u64, Position, f64)>,
}

impl PinSet {
    pub fn pins(&self) -> &[Pin] {
        &self.pins
    }

    pub fn is_empty(&self) -> bool {
        self.pins.is_empty()
    }

    /// The panel explains pinning itself, so a click on one of its rows
    /// does not show the canvas tip.
    pub fn mark_hinted(&mut self) {
        self.hinted = true;
    }

    /// Whether any permanent pin exists.
    pub fn has_pinned(&self) -> bool {
        self.pins.iter().any(|p| !p.preview)
    }

    /// Permanent pins (the hover preview is not counted).
    pub fn len(&self) -> usize {
        self.pins.iter().filter(|p| !p.preview).count()
    }

    pub fn clear(&mut self) {
        self.pins.clear();
        self.hover = None;
    }

    /// Drops the transient pins.
    pub fn drop_previews(&mut self) {
        self.pins.retain(|p| !p.preview);
    }

    /// The files of the finished, permanent pins whose answer Auto Open
    /// Matches has not looked at yet, marking them as looked at.
    pub fn take_auto_open_files(&mut self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for pin in self.pins.iter_mut().filter(|p| !p.preview && !p.auto_opened) {
            let Some(graph) = pin.graph() else { continue };
            let files: Vec<PathBuf> = graph.files().into_iter().map(|(path, _)| path).collect();
            pin.auto_opened = true;
            for path in files {
                if !out.contains(&path) {
                    out.push(path);
                }
            }
        }
        out
    }

    /// Drops one pin.
    pub fn remove(&mut self, id: u64) {
        self.pins.retain(|p| p.id != id);
    }

    pub fn is_waiting(&self) -> bool {
        self.pins.iter().any(|p| matches!(p.fetch, PinFetch::Pending(_)))
    }

    /// True while a pin asked without a context waits for its answer.
    pub fn needs_polling(&self) -> bool {
        self.pins.iter().any(|p| p.poll && matches!(p.fetch, PinFetch::Pending(_)))
    }

    /// Takes the answer of a pin's request; anything else is handed back.
    pub fn receive(&mut self, id: RequestId, response: LspResponse) -> Option<LspResponse> {
        let LspResponse::SymbolGraph(result) = response else { return Some(response) };
        match self.pins.iter_mut().find(|p| p.fetch == PinFetch::Pending(id)) {
            Some(pin) => {
                pin.fetch = match result {
                    Ok(graph) => PinFetch::Done(graph),
                    Err(e) => PinFetch::Failed(e),
                };
                None
            }
            None => Some(LspResponse::SymbolGraph(result)),
        }
    }

    /// Total entries and edges over the finished pins, for the panel.
    pub fn totals(&self) -> (usize, usize) {
        self.pins.iter().filter(|p| !p.preview).filter_map(Pin::graph).fold((0, 0), |(r, e), g| (r + g.entries.len(), e + g.edge_count()))
    }

    /// The files of all pins with how many entries each holds, summed.
    pub fn files(&self) -> Vec<(PathBuf, usize)> {
        let mut counts: std::collections::BTreeMap<PathBuf, usize> = std::collections::BTreeMap::new();
        for graph in self.pins.iter().filter(|p| !p.preview).filter_map(Pin::graph) {
            for (path, n) in graph.files() {
                *counts.entry(path).or_default() += n;
            }
        }
        counts.into_iter().collect()
    }
}

/// The identifier of `line` at byte `byte` (letters, digits, `_`, `$`).
pub fn word_at(line: &str, byte: usize) -> String {
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let byte = byte.min(line.len());
    let start = line[..byte].rfind(|c: char| !is_word(c)).map_or(0, |i| i + line[i..].chars().next().map_or(1, char::len_utf8));
    let end = line[byte..].find(|c: char| !is_word(c)).map_or(line.len(), |i| byte + i);
    line[start..end].to_string()
}

/// Pins the symbol at `line`/`start_byte` of file node `node_id` and asks
/// the language server for its graph. `additive` keeps the other pins
/// (and toggles this one when it is already pinned).
pub fn pin(
    ctx: Option<&egui::Context>,
    state: &mut AppState,
    node_id: u64,
    line: usize,
    start_byte: usize,
    additive: bool,
) {
    let mode = if additive { PinMode::Add } else { PinMode::Replace };
    pin_token(ctx, state, node_id, line, start_byte, mode);
}

/// [`pin`] with an explicit [`PinMode`], for the token at `line`/`start_byte`.
pub fn pin_token(
    ctx: Option<&egui::Context>,
    state: &mut AppState,
    node_id: u64,
    line: usize,
    start_byte: usize,
    mode: PinMode,
) {
    let Some(node) = state.node_store.get_file(node_id) else { return };
    let Some(text_line) = node.content.lines().nth(line) else { return };
    let position = Position { line, character: utf16_column(text_line, start_byte) };
    let name = word_at(text_line, start_byte);
    pin_at(ctx, state, node_id, position, name, mode);
}

/// Pins the symbol at `position` of file node `node_id`, named `name`.
pub fn pin_at(
    ctx: Option<&egui::Context>,
    state: &mut AppState,
    node_id: u64,
    position: Position,
    name: String,
    mode: PinMode,
) {
    let Some(node) = state.node_store.get_file(node_id) else { return };
    let (path, text) = (node.path.clone(), node.content.clone());

    if let Some(i) = state.lsp.pins.pins.iter().position(|p| p.path == path && p.position == position) {
        let pins = &mut state.lsp.pins;
        match mode {
            PinMode::Preview => {}
            PinMode::Add => {
                if pins.pins[i].preview {
                    pins.pins[i].preview = false;
                } else {
                    let id = pins.pins[i].id;
                    pins.remove(id);
                }
            }
            PinMode::Replace => {
                // A click on the preview keeps it as the one pin; on a pin
                // that is already there, nothing changes.
                if pins.pins[i].preview {
                    pins.pins[i].preview = false;
                    let keep = pins.pins[i].id;
                    pins.pins.retain(|p| p.id == keep);
                }
            }
        }
        return;
    }
    let quiet = mode == PinMode::Preview;
    let Some((server, language_id)) = server_for(&path) else {
        if !quiet {
            crate::ui::toast::push(state, format!("No language server for {}. Open the LSP panel for the servers.", ext_of(&path)));
        }
        return;
    };
    let Some(root) = state.loaded_dir.clone() else {
        if !quiet {
            crate::ui::toast::push(state, "Open a folder first: the language server needs a workspace.");
        }
        return;
    };
    match mode {
        PinMode::Replace => state.lsp.pins.clear(),
        PinMode::Add | PinMode::Preview => state.lsp.pins.drop_previews(),
    }
    let request = state.lsp.manager.request(
        ctx,
        server,
        &root,
        LspRequest::SymbolGraph { path: path.clone(), language_id, text, position },
    );
    let pins = &mut state.lsp.pins;
    pins.next_id += 1;
    let id = pins.next_id;
    pins.pins.push(Pin {
        id,
        node_id,
        path,
        name,
        position,
        fetch: PinFetch::Pending(request),
        poll: ctx.is_none(),
        preview: quiet,
        auto_opened: false,
    });
    if !quiet && !std::mem::replace(&mut pins.hinted, true) {
        crate::ui::toast::push(state, PIN_HINT);
    }
}

/// Query On Hover: `token` is the symbol under the pointer (file node,
/// position, name), if any. Once the pointer rests on one for
/// [`HOVER_SECONDS`] its graph is pinned as a preview; moving to another one
/// or off any drops the preview. `now` is the app time in seconds. Returns
/// how long to wait before looking again while the rest is being timed.
pub fn update_hover(
    ctx: Option<&egui::Context>,
    state: &mut AppState,
    token: Option<(u64, Position, String)>,
    now: f64,
) -> Option<f64> {
    let Some((node_id, position, name)) = token.filter(|_| state.settings.lsp.query_on_hover) else {
        state.lsp.pins.hover = None;
        state.lsp.pins.drop_previews();
        return None;
    };
    let since = match state.lsp.pins.hover {
        Some((n, p, t)) if n == node_id && p == position => t,
        _ => {
            // Another token: the old preview goes at once.
            state.lsp.pins.drop_previews();
            now
        }
    };
    state.lsp.pins.hover = Some((node_id, position, since));
    if now - since < HOVER_SECONDS {
        return Some(HOVER_SECONDS - (now - since));
    }
    pin_at(ctx, state, node_id, position, name, PinMode::Preview);
    None
}

fn ext_of(path: &Path) -> String {
    path.extension().map_or_else(|| "this file".into(), |e| format!(".{}", e.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_at_finds_the_identifier_around_a_byte() {
        assert_eq!(word_at("let kindOf = 1;", 6), "kindOf");
        assert_eq!(word_at("let kindOf = 1;", 4), "kindOf");
        assert_eq!(word_at("a.b_c$d(e)", 3), "b_c$d");
        assert_eq!(word_at("  ", 1), "");
    }

    #[test]
    fn answers_go_to_the_pin_that_asked_and_others_are_handed_back() {
        let mut set = PinSet::default();
        set.pins.push(Pin {
            id: 1,
            node_id: 7,
            path: "/p/a.rs".into(),
            name: "f".into(),
            position: Position { line: 4, character: 3 },
            fetch: PinFetch::Pending(42),
            poll: false,
            preview: false,
            auto_opened: false,
        });
        assert!(set.is_waiting());
        let other = set.receive(9, LspResponse::SymbolGraph(Err("late".into())));
        assert!(other.is_some(), "not ours");
        assert!(set.receive(42, LspResponse::SymbolGraph(Err("boom".into()))).is_none());
        assert_eq!(set.pins()[0].fetch, PinFetch::Failed("boom".into()));
        assert!(!set.is_waiting());
        assert_eq!(set.pins()[0].label(), "a.rs:5");
        set.remove(1);
        assert!(set.is_empty());
    }
}
