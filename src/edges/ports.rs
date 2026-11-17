//! Ports: the circles on a file node's border where its edges attach, one
//! per connected line and side, as in v2 Code Canvas. Each port also marks
//! its row with a band, colored by whether the line is an import or a symbol.

use std::collections::HashMap;

use crate::edges::edge_store::{Edge, EdgeKind, EdgeStore};
use crate::edges::style::{EdgeStyleConfig, RelationshipType};
use crate::nodes::node_store::NodeStore;
use crate::nodes::selection::Selection;

/// What the line at a port does: loads another module, or uses/defines a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    Import,
    Symbol,
}

/// Which border of the node the port sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Port {
    /// 0-based code line the port marks.
    pub line: usize,
    pub side: PortSide,
    pub kind: PortKind,
    /// At least one edge through this port touches the selection.
    pub emphasized: bool,
}

/// Kind of the port at the source end of `edge`: import statements,
/// re-exports and barrels load a module; anything else uses a symbol.
fn source_kind(edge: &Edge) -> PortKind {
    if edge.relationship == RelationshipType::Imports || edge.kind != EdgeKind::Direct {
        PortKind::Import
    } else {
        PortKind::Symbol
    }
}

/// Kind of the port at the target end: a named definition is a symbol; a
/// module/document target without a name is what an import loads.
fn target_kind(edge: &Edge) -> PortKind {
    if edge.name_span.is_none() && edge.relationship == RelationshipType::Imports {
        PortKind::Import
    } else {
        PortKind::Symbol
    }
}

/// The side of a node of horizontal extent `left..right` facing `toward_x`.
pub fn side_facing(left: f32, right: f32, toward_x: f32) -> PortSide {
    if toward_x >= (left + right) * 0.5 {
        PortSide::Right
    } else {
        PortSide::Left
    }
}

/// Ports of every node, from the edges `draw_edges` would draw. Edges sharing
/// a line and side share a port; an import anywhere on it makes it an import
/// port. Collapsed nodes and lines a node does not draw get none, and their
/// edges keep attaching to the header.
pub fn collect_ports(
    edges: &EdgeStore,
    nodes: &NodeStore,
    config: &EdgeStyleConfig,
    selection: &Selection,
    node_width: f32,
    font_size: f32,
) -> HashMap<u64, Vec<Port>> {
    let mut by_key: HashMap<(u64, usize, PortSide), Port> = HashMap::new();
    let nothing_selected = selection.count() == 0 && selection.symbols().is_empty();
    for edge in edges.all_edges() {
        if !edge.is_visible_for(config, selection) {
            continue;
        }
        let (Some(src), Some(tgt)) = (
            nodes.get_file(edge.source_node),
            nodes.get_file(edge.target_node),
        ) else {
            continue;
        };
        let src_rect = src.world_rect(node_width, font_size);
        let tgt_rect = tgt.world_rect(node_width, font_size);
        let emphasized = !nothing_selected && edge.is_selected(selection);
        let ends = [
            (src, edge.source_span, tgt_rect.center().x, src_rect, source_kind(edge)),
            (tgt, edge.target_span, src_rect.center().x, tgt_rect, target_kind(edge)),
        ];
        for (node, span, toward_x, rect, kind) in ends {
            let Some(span) = span else { continue };
            if node.collapsed || span.start >= node.visible_lines() {
                continue;
            }
            let side = side_facing(rect.left(), rect.right(), toward_x);
            let port = by_key.entry((node.id, span.start, side)).or_insert(Port {
                line: span.start,
                side,
                kind,
                emphasized,
            });
            if kind == PortKind::Import {
                port.kind = PortKind::Import;
            }
            port.emphasized |= emphasized;
        }
    }
    let mut out: HashMap<u64, Vec<Port>> = HashMap::new();
    for ((node, _, _), port) in by_key {
        out.entry(node).or_default().push(port);
    }
    for ports in out.values_mut() {
        ports.sort_by_key(|p| (p.line, p.side == PortSide::Right));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::dependencies::LineSpan;
    use crate::edges::edge_store::NameSpan;

    const WIDTH: f32 = 100.0;
    const FONT: f32 = 10.0;

    fn line(n: usize) -> Option<LineSpan> {
        Some(LineSpan { start: n, end: n })
    }

    fn name(line: usize) -> Option<NameSpan> {
        Some(NameSpan { line, start_byte: 0, end_byte: 1 })
    }

    /// `a.ts` at x=0 and `b.ts` at x=500, ten lines each.
    fn two_files() -> (NodeStore, u64, u64) {
        let mut nodes = NodeStore::default();
        let content = "x\n".repeat(10);
        let a = nodes.add_file("a.ts".into(), content.clone(), egui::pos2(0.0, 0.0));
        let b = nodes.add_file("b.ts".into(), content, egui::pos2(500.0, 0.0));
        (nodes, a, b)
    }

    fn collect(edges: &EdgeStore, nodes: &NodeStore, selection: &Selection) -> HashMap<u64, Vec<Port>> {
        collect_ports(edges, nodes, &EdgeStyleConfig::default(), selection, WIDTH, FONT)
    }

    #[test]
    fn each_end_gets_a_port_on_the_side_facing_the_other_node() {
        let (nodes, a, b) = two_files();
        let mut edges = EdgeStore::default();
        edges.add_spanned(a, b, Some("fn".into()), line(2), line(5), name(5));

        let ports = collect(&edges, &nodes, &Selection::default());
        assert_eq!(
            ports[&a],
            vec![Port { line: 2, side: PortSide::Right, kind: PortKind::Symbol, emphasized: false }]
        );
        assert_eq!(
            ports[&b],
            vec![Port { line: 5, side: PortSide::Left, kind: PortKind::Symbol, emphasized: false }]
        );
    }

    #[test]
    fn edges_from_one_line_share_a_port_and_an_import_wins() {
        let (nodes, a, b) = two_files();
        let mut edges = EdgeStore::default();
        edges.add_spanned(a, b, Some("fn".into()), line(1), line(3), name(3));
        let import = edges.add_spanned(a, b, Some("fn".into()), line(1), line(7), name(7));
        edges.set_relationship(import, RelationshipType::Imports);

        let ports = collect(&edges, &nodes, &Selection::default());
        assert_eq!(ports[&a].len(), 1, "{:?}", ports[&a]);
        assert_eq!(ports[&a][0].kind, PortKind::Import);
        let targets: Vec<_> = ports[&b].iter().map(|p| (p.line, p.kind)).collect();
        assert_eq!(targets, vec![(3, PortKind::Symbol), (7, PortKind::Symbol)]);
    }

    #[test]
    fn reexports_and_module_targets_are_import_ports() {
        let (nodes, a, b) = two_files();
        let mut edges = EdgeStore::default();
        edges.add_full(
            a, b, Some("fn".into()), None, line(0), line(4), name(4), None, EdgeKind::ReExport,
        );
        let module = edges.add_spanned(b, a, None, line(8), line(0), None);
        edges.set_relationship(module, RelationshipType::Imports);

        let ports = collect(&edges, &nodes, &Selection::default());
        let kind_at = |node: u64, line: usize| {
            ports[&node].iter().find(|p| p.line == line).map(|p| p.kind)
        };
        assert_eq!(kind_at(a, 0), Some(PortKind::Import), "re-export source");
        assert_eq!(kind_at(b, 4), Some(PortKind::Symbol), "re-exported definition");
        assert_eq!(kind_at(b, 8), Some(PortKind::Import), "import statement");
        let module_edge = edges.all_edges().find(|e| e.id == module).unwrap();
        assert_eq!(target_kind(module_edge), PortKind::Import, "module target");
    }

    #[test]
    fn no_ports_for_collapsed_nodes_missing_spans_or_hidden_edges() {
        let (mut nodes, a, b) = two_files();
        let mut edges = EdgeStore::default();
        edges.add_spanned(a, b, None, None, line(20), None);
        let hidden = edges.add_spanned(a, b, None, line(1), line(1), None);
        edges.set_relationship(hidden, RelationshipType::Renders);
        let mut config = EdgeStyleConfig::default();
        config.relationships[RelationshipType::Renders.index()].visible = false;
        assert!(collect_ports(&edges, &nodes, &config, &Selection::default(), WIDTH, FONT).is_empty());

        edges.add_spanned(a, b, None, line(2), line(2), None);
        nodes.get_file_mut(b).unwrap().collapsed = true;
        let ports = collect_ports(&edges, &nodes, &config, &Selection::default(), WIDTH, FONT);
        assert!(ports.contains_key(&a) && !ports.contains_key(&b), "{ports:?}");
    }

    #[test]
    fn ports_of_selected_edges_are_emphasized() {
        let (nodes, a, b) = two_files();
        let mut edges = EdgeStore::default();
        edges.add_spanned(a, b, None, line(1), line(1), None);
        let mut selection = Selection::default();
        selection.select(a);
        let ports = collect(&edges, &nodes, &selection);
        assert!(ports.values().flatten().all(|p| p.emphasized));
    }
}
