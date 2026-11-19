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
}

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
}

impl PinSet {
    pub fn pins(&self) -> &[Pin] {
        &self.pins
    }

    pub fn is_empty(&self) -> bool {
        self.pins.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pins.len()
    }

    pub fn clear(&mut self) {
        self.pins.clear();
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
        self.pins.iter().filter_map(Pin::graph).fold((0, 0), |(r, e), g| (r + g.entries.len(), e + g.edge_count()))
    }

    /// The files of all pins with how many entries each holds, summed.
    pub fn files(&self) -> Vec<(PathBuf, usize)> {
        let mut counts: std::collections::BTreeMap<PathBuf, usize> = std::collections::BTreeMap::new();
        for graph in self.pins.iter().filter_map(Pin::graph) {
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
    let Some(node) = state.node_store.get_file(node_id) else { return };
    let (path, text) = (node.path.clone(), node.content.clone());
    let Some(text_line) = text.lines().nth(line) else { return };
    let position = Position { line, character: utf16_column(text_line, start_byte) };
    let name = word_at(text_line, start_byte);

    if let Some(existing) = state.lsp.pins.pins.iter().find(|p| p.path == path && p.position == position).map(|p| p.id) {
        if additive {
            state.lsp.pins.remove(existing);
        }
        return;
    }
    let Some((server, language_id)) = server_for(&path) else {
        crate::ui::toast::push(state, format!("No language server for {}. Open the LSP panel for the servers.", ext_of(&path)));
        return;
    };
    let Some(root) = state.loaded_dir.clone() else {
        crate::ui::toast::push(state, "Open a folder first: the language server needs a workspace.");
        return;
    };
    if !additive {
        state.lsp.pins.clear();
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
    pins.pins.push(Pin { id, node_id, path, name, position, fetch: PinFetch::Pending(request), poll: ctx.is_none() });
    if !std::mem::replace(&mut pins.hinted, true) {
        crate::ui::toast::push(state, PIN_HINT);
    }
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
