use std::collections::{HashMap, HashSet};

use crate::analysis::dependencies::LineSpan;

/// Column span of the symbol name inside a line, for inline highlight.
/// Byte offsets into the line's text, 0-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameSpan {
    pub line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub id: u64,
    pub source_node: u64,
    pub target_node: u64,
    pub label: Option<String>,
    /// Statement of the source the edge starts at; `None`: the whole node.
    pub source_span: Option<LineSpan>,
    /// Statement of the target the edge ends at; `None`: the node header.
    pub target_span: Option<LineSpan>,
    /// Column span of the symbol name on the definition side.
    pub name_span: Option<NameSpan>,
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
        let id = self.next_id;
        self.next_id += 1;
        self.edges.insert(
            id,
            Edge {
                id,
                source_node: source,
                target_node: target,
                label,
                source_span,
                target_span,
                name_span,
            },
        );
        id
    }

    /// Highlight data per node: both ends of every edge that touches a node
    /// in `selected`, with the kind label and the name span (on the target
    /// side only). Sorted by span, deduplicated by (span, label).
    pub fn selected_spans(&self, selected: &HashSet<u64>) -> HashMap<u64, Vec<SpanHighlight>> {
        let mut out: HashMap<u64, Vec<SpanHighlight>> = HashMap::new();
        for e in self.edges.values() {
            if !selected.contains(&e.source_node) && !selected.contains(&e.target_node) {
                continue;
            }
            let label = e.label.clone().unwrap_or_default();
            if let Some(span) = e.source_span {
                out.entry(e.source_node).or_default().push(SpanHighlight {
                    span,
                    label: label.clone(),
                    name_span: None,
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
            highlights.sort_by(|a, b| a.span.cmp(&b.span).then_with(|| a.label.cmp(&b.label)));
            highlights.dedup_by(|a, b| a.span == b.span && a.label == b.label);
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
        store.add_spanned(1, 2, Some("fn".into()), Some(span(4, 4)), Some(span(0, 2)), None);
        store.add_spanned(1, 2, Some("fn".into()), Some(span(9, 9)), Some(span(0, 2)), None);
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
}
