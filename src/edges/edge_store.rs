use std::collections::{HashMap, HashSet};

use crate::analysis::dependencies::LineSpan;
use crate::edges::style::{EdgeStyleConfig, RelationshipType};
use crate::nodes::selection::Selection;

/// Column span of the symbol name inside a line, for inline highlight.
/// Byte offsets into the line's text, 0-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameSpan {
    pub line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// Classification of a dependency edge for the "Re-export & Barrel" toggles
/// of the v2 Code Canvas Dependency Edges panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeKind {
    /// Ordinary reference: the source statement names the target symbol.
    #[default]
    Direct,
    /// Source statement is a re-export of the target (`pub use`,
    /// `export ... from`, `from X import Y` with `__all__`).
    ReExport,
    /// Source file is a barrel (`index.{ts,tsx,js,jsx,mjs,cjs}`,
    /// `mod.rs`, `__init__.py`) and accumulates several re-exports.
    Barrel,
}

/// Graph comparison coverage is explicit: LSP-only links are not assumed
/// unchanged merely because no historical symbol graph was collected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeChange {
    #[default]
    Uncompared,
    Unchanged,
    Added,
    Deleted,
}

impl EdgeChange {
    pub fn is_changed(self) -> bool {
        matches!(self, Self::Added | Self::Deleted)
    }
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub change: EdgeChange,
    pub relationship: RelationshipType,
    pub id: u64,
    pub source_node: u64,
    pub target_node: u64,
    /// Kind of the symbol defined in the target file.
    pub label: Option<String>,
    /// Kind of the symbol containing the source statement, for the v2
    /// bi-color gradient (source-kind → target-kind). `None` when the usage
    /// is not inside a picked symbol.
    pub source_label: Option<String>,
    /// Statement of the source the edge starts at; `None`: the whole node.
    pub source_span: Option<LineSpan>,
    /// Statement of the target the edge ends at; `None`: the node header.
    pub target_span: Option<LineSpan>,
    /// Column span of the symbol name on the definition side.
    pub name_span: Option<NameSpan>,
    /// Exact identifier range on the usage side, including aliases.
    pub source_name_span: Option<NameSpan>,
    /// Classification for the re-export / barrel toggles.
    pub kind: EdgeKind,
}

impl Edge {
    pub fn is_selected(&self, selection: &Selection) -> bool {
        if selection.symbols().is_empty() {
            return selection.is_selected(self.source_node)
                || selection.is_selected(self.target_node);
        }
        selection.symbols().iter().any(|symbol| {
            (self.target_node == symbol.node_id && self.name_span == Some(symbol.name_span))
                || (self.source_node == symbol.node_id
                    && self.source_span.is_some_and(|usage| {
                        symbol.span.start <= usage.start && usage.start <= symbol.span.end
                    }))
        })
    }

    pub fn is_visible(&self, config: &EdgeStyleConfig) -> bool {
        config.relationships[self.relationship.index()].visible
            && (self.change != EdgeChange::Deleted || config.show_deleted)
            && match self.kind {
                EdgeKind::Direct => true,
                EdgeKind::ReExport => config.show_reexport_edges,
                EdgeKind::Barrel => config.show_barrel_edges,
            }
    }

    pub fn is_visible_for(&self, config: &EdgeStyleConfig, selection: &Selection) -> bool {
        self.is_visible(config) && (config.show_all || self.is_selected(selection))
    }

    pub fn is_incoming(&self, selection: &Selection) -> bool {
        if selection.symbols().is_empty() {
            return selection.is_selected(self.target_node)
                && !selection.is_selected(self.source_node);
        }
        selection
            .symbols()
            .iter()
            .any(|s| s.node_id == self.target_node && Some(s.name_span) == self.name_span)
    }
}

/// One span to highlight on a file node, with the kind for color lookup
/// and an optional inline name highlight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanHighlight {
    pub span: LineSpan,
    pub label: String,
    pub name_span: Option<NameSpan>,
}

#[derive(Debug, Clone)]
pub struct EdgeStore {
    edges: HashMap<u64, Edge>,
    next_id: u64,
}

impl Default for EdgeStore {
    fn default() -> Self {
        Self {
            edges: HashMap::new(),
            next_id: 1,
        }
    }
}

impl EdgeStore {
    pub fn set_change(&mut self, id: u64, change: EdgeChange) {
        if let Some(edge) = self.edges.get_mut(&id) {
            edge.change = change;
        }
    }
    pub fn set_relationship(&mut self, id: u64, relationship: RelationshipType) {
        if let Some(edge) = self.edges.get_mut(&id) {
            edge.relationship = relationship;
        }
    }
    pub fn add(&mut self, source: u64, target: u64, label: Option<String>) -> u64 {
        self.add_spanned(source, target, label, None, None, None)
    }

    /// Adds an edge between two statements of the source and target nodes.
    pub fn add_spanned(
        &mut self,
        source: u64,
        target: u64,
        label: Option<String>,
        source_span: Option<LineSpan>,
        target_span: Option<LineSpan>,
        name_span: Option<NameSpan>,
    ) -> u64 {
        self.add_full(
            source,
            target,
            label,
            None,
            source_span,
            target_span,
            name_span,
            None,
            EdgeKind::Direct,
        )
    }

    /// Adds an edge carrying every v2 Code Canvas field: the two kinds
    /// (source/target) for the bi-color gradient, and the re-export/barrel
    /// classification for the sidebar toggles.
    #[allow(clippy::too_many_arguments)]
    pub fn add_full(
        &mut self,
        source: u64,
        target: u64,
        label: Option<String>,
        source_label: Option<String>,
        source_span: Option<LineSpan>,
        target_span: Option<LineSpan>,
        name_span: Option<NameSpan>,
        source_name_span: Option<NameSpan>,
        kind: EdgeKind,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.edges.insert(
            id,
            Edge {
                change: EdgeChange::Uncompared,
                relationship: RelationshipType::References,
                id,
                source_node: source,
                target_node: target,
                label,
                source_label,
                source_span,
                target_span,
                name_span,
                source_name_span,
                kind,
            },
        );
        id
    }

    /// Highlight data per node: both ends of every edge that touches a node
    /// in `selected`, with kind labels and exact name spans at both ends.
    /// Sorted and deduplicated by (statement span, kind, name span).
    pub fn selected_spans(&self, selected: &HashSet<u64>) -> HashMap<u64, Vec<SpanHighlight>> {
        self.spans_matching(|e| {
            selected.contains(&e.source_node) || selected.contains(&e.target_node)
        })
    }

    /// The same focus and kind-visibility policy as edge strokes. Explicitly
    /// picked declarations stay highlighted even without cross-file references.
    pub fn focused_spans(
        &self,
        selection: &Selection,
        config: &EdgeStyleConfig,
    ) -> HashMap<u64, Vec<SpanHighlight>> {
        if !config.highlight_selected && !config.highlight_changes {
            return HashMap::new();
        }
        let mut out = self.spans_matching(|e| {
            ((config.highlight_selected && e.is_selected(selection))
                || (config.highlight_changes && e.change.is_changed()))
                && e.is_visible_for(config, selection)
        });
        for symbol in selection
            .symbols()
            .iter()
            .filter(|_| config.highlight_selected)
        {
            let highlight = SpanHighlight {
                span: symbol.span,
                label: symbol.label.clone(),
                name_span: Some(symbol.name_span),
            };
            let spans = out.entry(symbol.node_id).or_default();
            if !spans.contains(&highlight) {
                spans.push(highlight);
            }
        }
        out
    }

    fn spans_matching(&self, matches: impl Fn(&Edge) -> bool) -> HashMap<u64, Vec<SpanHighlight>> {
        let mut out: HashMap<u64, Vec<SpanHighlight>> = HashMap::new();
        for e in self.edges.values() {
            if e.change == EdgeChange::Deleted || !matches(e) {
                continue;
            }
            let label = e.label.clone().unwrap_or_default();
            if let Some(span) = e.source_span {
                out.entry(e.source_node).or_default().push(SpanHighlight {
                    span,
                    label: label.clone(),
                    name_span: e.source_name_span,
                });
            }
            if let Some(span) = e.target_span {
                out.entry(e.target_node).or_default().push(SpanHighlight {
                    span,
                    label: label.clone(),
                    name_span: e.name_span,
                });
            }
        }
        for highlights in out.values_mut() {
            highlights.sort_by_key(|h| {
                (
                    h.span,
                    h.label.clone(),
                    h.name_span.map(|n| (n.line, n.start_byte, n.end_byte)),
                )
            });
            highlights.dedup();
        }
        out
    }

    pub fn remove(&mut self, id: u64) {
        self.edges.remove(&id);
    }

    pub fn edges_for_node(&self, node_id: u64) -> Vec<u64> {
        self.edges
            .values()
            .filter(|e| e.source_node == node_id || e.target_node == node_id)
            .map(|e| e.id)
            .collect()
    }

    pub fn all_edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges.values()
    }

    pub fn clear(&mut self) {
        self.edges.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleted_visibility_never_marks_old_ranges_in_current_code() {
        let mut store = EdgeStore::default();
        let id = store.add_spanned(
            1,
            2,
            Some("file".into()),
            Some(LineSpan { start: 9, end: 9 }),
            None,
            None,
        );
        store.set_change(id, EdgeChange::Deleted);
        let mut selection = Selection::default();
        selection.select(1);
        let mut config = EdgeStyleConfig::default();
        assert!(!store
            .all_edges()
            .next()
            .unwrap()
            .is_visible_for(&config, &selection));
        config.show_deleted = true;
        assert!(store
            .all_edges()
            .next()
            .unwrap()
            .is_visible_for(&config, &selection));
        assert!(store.focused_spans(&selection, &config).is_empty());
        store.set_change(id, EdgeChange::Added);
        config.highlight_selected = false;
        config.highlight_changes = true;
        assert!(!store
            .focused_spans(&Selection::default(), &config)
            .is_empty());
    }

    #[test]
    fn relationship_filters_and_all_apply_to_strokes_and_statement_highlights() {
        use crate::edges::style::RelationshipType;
        let mut store = EdgeStore::default();
        let id = store.add_spanned(
            1,
            2,
            Some("fn".into()),
            Some(LineSpan { start: 3, end: 3 }),
            None,
            None,
        );
        store.set_relationship(id, RelationshipType::Renders);
        let edge = store.all_edges().next().unwrap();
        let mut selection = Selection::default();
        selection.select(1);
        let mut config = EdgeStyleConfig::default();
        assert!(edge.is_visible_for(&config, &selection));
        config.relationships[RelationshipType::Renders.index()].visible = false;
        assert!(!edge.is_visible_for(&config, &selection));
        assert!(store.focused_spans(&selection, &config).is_empty());
        config.relationships[RelationshipType::Renders.index()].visible = true;
        config.show_all = false;
        assert!(edge.is_visible_for(&config, &selection));
        selection.clear();
        assert!(
            !edge.is_visible_for(&config, &selection),
            "All off with no selection hides the graph"
        );
    }

    #[test]
    fn add_assigns_sequential_ids_and_stores_endpoints() {
        let mut store = EdgeStore::default();
        let e1 = store.add(1, 2, Some("Import".into()));
        let e2 = store.add(2, 3, None);

        assert_eq!((e1, e2), (1, 2));
        let edge = store.all_edges().find(|e| e.id == e1).unwrap();
        assert_eq!((edge.source_node, edge.target_node), (1, 2));
        assert_eq!(edge.label.as_deref(), Some("Import"));
    }

    #[test]
    fn edges_for_node_matches_source_or_target() {
        let mut store = EdgeStore::default();
        let e1 = store.add(1, 2, None);
        let e2 = store.add(2, 3, None);
        store.add(4, 5, None);

        let mut for_two = store.edges_for_node(2);
        for_two.sort();
        assert_eq!(for_two, [e1, e2]);
        assert!(store.edges_for_node(9).is_empty());
    }

    #[test]
    fn remove_and_clear() {
        let mut store = EdgeStore::default();
        let e1 = store.add(1, 2, None);
        store.add(2, 3, None);

        store.remove(e1);
        store.remove(999);
        assert_eq!(store.all_edges().count(), 1);

        store.clear();
        assert_eq!(store.all_edges().count(), 0);
    }

    #[test]
    fn selected_spans_cover_both_ends_of_edges_touching_the_selection() {
        let span = |start, end| LineSpan { start, end };
        let mut store = EdgeStore::default();
        store.add_spanned(
            1,
            2,
            Some("fn".into()),
            Some(span(4, 4)),
            Some(span(0, 2)),
            None,
        );
        store.add_spanned(
            1,
            2,
            Some("fn".into()),
            Some(span(9, 9)),
            Some(span(0, 2)),
            None,
        );
        store.add_spanned(1, 3, Some("struct".into()), Some(span(1, 1)), None, None);
        store.add_spanned(4, 5, None, Some(span(7, 7)), Some(span(8, 8)), None);

        let spans = store.selected_spans(&HashSet::from([2]));
        let s1: Vec<_> = spans[&1].iter().map(|h| h.span).collect();
        assert_eq!(s1, [span(4, 4), span(9, 9)]);
        let s2: Vec<_> = spans[&2].iter().map(|h| h.span).collect();
        assert_eq!(s2, [span(0, 2)], "shared definition listed once");
        assert_eq!(spans.len(), 2, "edge 1->3 and 4->5 do not touch node 2");

        assert!(store.selected_spans(&HashSet::new()).is_empty());
    }

    #[test]
    fn symbol_focus_excludes_other_definitions_in_the_same_file_and_hidden_edges() {
        use crate::edges::style::EdgeStyleConfig;
        use crate::nodes::selection::{SelectedSymbol, Selection};
        let span = |start, end| LineSpan { start, end };
        let name = NameSpan {
            line: 0,
            start_byte: 7,
            end_byte: 12,
        };
        let mut store = EdgeStore::default();
        let chosen = store.add_spanned(
            1,
            2,
            Some("fn".into()),
            Some(span(4, 4)),
            Some(span(0, 2)),
            Some(name),
        );
        store.add_spanned(
            1,
            2,
            Some("fn".into()),
            Some(span(8, 8)),
            Some(span(5, 7)),
            Some(NameSpan { line: 5, ..name }),
        );
        let mut selection = Selection::default();
        selection.select_symbol(
            1,
            SelectedSymbol {
                node_id: 2,
                name_span: name,
                span: span(0, 2),
                label: "fn".into(),
            },
            false,
        );
        let mut config = EdgeStyleConfig::default();
        let highlights = store.focused_spans(&selection, &config);
        assert_eq!(highlights[&1].len(), 1, "unrelated symbol stays dim");
        assert_eq!(highlights[&1][0].span, span(4, 4));
        store.edges.get_mut(&chosen).unwrap().kind = EdgeKind::ReExport;
        config.show_reexport_edges = false;
        let highlights = store.focused_spans(&selection, &config);
        assert!(
            !highlights.contains_key(&1),
            "hidden edges must not leave usage highlights"
        );
        assert_eq!(
            highlights[&2].len(),
            1,
            "the selected declaration stays marked"
        );
    }

    #[test]
    fn selected_function_includes_outgoing_calls_but_not_other_functions_calls() {
        use crate::nodes::selection::{SelectedSymbol, Selection};
        let mut selection = Selection::default();
        selection.select_symbol(
            1,
            SelectedSymbol {
                node_id: 1,
                name_span: NameSpan {
                    line: 3,
                    start_byte: 3,
                    end_byte: 6,
                },
                span: LineSpan { start: 3, end: 7 },
                label: "fn".into(),
            },
            false,
        );
        let mut store = EdgeStore::default();
        let inside = store.add_spanned(1, 2, None, Some(LineSpan { start: 4, end: 4 }), None, None);
        let outside =
            store.add_spanned(1, 2, None, Some(LineSpan { start: 9, end: 9 }), None, None);
        assert!(store.edges[&inside].is_selected(&selection));
        assert!(!store.edges[&outside].is_selected(&selection));
    }
}
