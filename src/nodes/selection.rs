use std::collections::HashSet;

use crate::analysis::dependencies::LineSpan;
use crate::edges::edge_store::NameSpan;

/// Stable definition identity, independent of edge IDs rebuilt by the indexer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedSymbol {
    pub node_id: u64,
    pub name_span: NameSpan,
    pub span: LineSpan,
    pub label: String,
}

impl SelectedSymbol {
    pub fn from_lsp(
        node_id: u64,
        content: &str,
        symbol: &crate::analysis::lsp::LspSymbol,
    ) -> Option<Self> {
        let range = symbol.selection_range;
        if range.start.line != range.end.line {
            return None;
        }
        let text = content.lines().nth(range.start.line)?;
        let start_byte =
            crate::analysis::dependencies::byte_offset(text, 0, range.start.character)?;
        let end_byte = crate::analysis::dependencies::byte_offset(text, 0, range.end.character)?;
        if start_byte >= end_byte {
            return None;
        }
        Some(Self {
            node_id,
            name_span: NameSpan {
                line: range.start.line,
                start_byte,
                end_byte,
            },
            span: LineSpan::of_range(symbol.range),
            label: symbol.kind.label().to_string(),
        })
    }
}

pub struct Selection {
    pub selected: HashSet<u64>,
    pub primary: Option<u64>,
    symbols: Vec<SelectedSymbol>,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            selected: HashSet::new(),
            primary: None,
            symbols: Vec::new(),
        }
    }
}

impl Selection {
    pub fn select(&mut self, id: u64) {
        self.symbols.clear();
        self.selected.clear();
        self.selected.insert(id);
        self.primary = Some(id);
    }

    pub fn toggle(&mut self, id: u64) {
        self.symbols.clear();
        if self.selected.contains(&id) {
            self.selected.remove(&id);
            if self.primary == Some(id) {
                self.primary = self.selected.iter().next().copied();
            }
        } else {
            self.selected.insert(id);
            self.primary = Some(id);
        }
    }

    pub fn select_all(&mut self, ids: impl IntoIterator<Item = u64>) {
        self.symbols.clear();
        self.selected.clear();
        self.selected.extend(ids);
        self.primary = self.selected.iter().next().copied();
    }

    pub fn clear(&mut self) {
        self.symbols.clear();
        self.selected.clear();
        self.primary = None;
    }

    pub fn is_selected(&self, id: u64) -> bool {
        self.selected.contains(&id)
    }

    pub fn count(&self) -> usize {
        self.selected.len()
    }

    pub fn selected_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.selected.iter().copied()
    }

    pub fn symbols(&self) -> &[SelectedSymbol] {
        &self.symbols
    }

    /// Drops the picked symbols, keeping the selected files.
    pub fn clear_symbols(&mut self) {
        self.symbols.clear();
    }

    /// Symbol focus takes precedence over whole-file edge highlighting. The
    /// clicked file stays selected for dragging; picking never moves the camera.
    pub fn select_symbol(&mut self, clicked_node: u64, symbol: SelectedSymbol, additive: bool) {
        if !additive {
            self.clear();
        }
        if let Some(index) = self
            .symbols
            .iter()
            .position(|s| s.node_id == symbol.node_id && s.name_span == symbol.name_span)
        {
            self.symbols.remove(index);
        } else {
            self.symbols.push(symbol);
        }
        self.selected.clear();
        self.primary = None;
        if !self.symbols.is_empty() {
            self.selected.insert(clicked_node);
            self.primary = Some(clicked_node);
        }
    }

    pub fn remove_node(&mut self, id: u64) {
        let had_symbols = !self.symbols.is_empty();
        self.symbols.retain(|s| s.node_id != id);
        self.selected.remove(&id);
        if self.primary == Some(id) {
            if had_symbols {
                // The clicked occurrence was removed, even if its definition lives elsewhere.
                self.clear();
            } else {
                self.primary = self.selected.iter().next().copied();
            }
        } else if had_symbols && self.symbols.is_empty() {
            self.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(sel: &Selection) -> Vec<u64> {
        let mut ids: Vec<u64> = sel.selected_ids().collect();
        ids.sort();
        ids
    }

    #[test]
    fn select_replaces_the_selection() {
        let mut sel = Selection::default();
        sel.select(1);
        sel.select(2);

        assert_eq!(sorted(&sel), [2]);
        assert_eq!(sel.primary, Some(2));
        assert!(!sel.is_selected(1));
    }

    #[test]
    fn toggle_adds_and_removes_and_moves_primary() {
        let mut sel = Selection::default();
        sel.toggle(1);
        sel.toggle(2);
        assert_eq!(sorted(&sel), [1, 2]);
        assert_eq!(sel.primary, Some(2));

        sel.toggle(2);
        assert_eq!(sorted(&sel), [1]);
        assert_eq!(sel.primary, Some(1), "primary falls back to a remaining id");

        sel.toggle(1);
        assert_eq!(sel.count(), 0);
        assert_eq!(sel.primary, None);
    }

    #[test]
    fn toggling_off_a_non_primary_keeps_primary() {
        let mut sel = Selection::default();
        sel.select(1);
        sel.toggle(2);
        sel.toggle(1);
        assert_eq!(sel.primary, Some(2));
    }

    #[test]
    fn select_all_and_clear() {
        let mut sel = Selection::default();
        sel.select(9);
        sel.select_all([3, 4, 5]);
        assert_eq!(sorted(&sel), [3, 4, 5]);
        assert!(sel.primary.is_some_and(|p| sel.is_selected(p)));

        sel.select_all([]);
        assert_eq!(sel.primary, None);

        sel.select(1);
        sel.clear();
        assert_eq!(sel.count(), 0);
        assert_eq!(sel.primary, None);
    }

    #[test]
    fn removing_primary_file_preserves_other_selected_files() {
        let mut sel = Selection::default();
        sel.select(1);
        sel.toggle(2);
        sel.remove_node(2);
        assert_eq!(sorted(&sel), [1]);
        assert_eq!(sel.primary, Some(1));
    }
}
