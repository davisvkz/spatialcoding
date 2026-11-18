//! Transient, debounced LSP graphs for exact identifier hover on the canvas.
//! These include intra-file references without adding self-edges to the file graph.

use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};
use std::time::Duration;

use crate::analysis::cst::parse_tree;
use crate::analysis::dependencies::{byte_offset, LineSpan};
use crate::analysis::lsp::manager::{LspRequest, LspResponse, RequestId};
use crate::analysis::lsp::{server_for, Location, LspSymbol, Range};
use crate::edges::edge_store::NameSpan;
use crate::nodes::node_store::NodeStore;
use crate::state::AppState;
use crate::ui::cst_panel::highlight_rect;

pub const DEFINITION_COLOR: egui::Color32 = egui::Color32::from_rgb(236, 72, 153);
pub const CONTEXT_COLOR: egui::Color32 = egui::Color32::from_rgb(34, 197, 94);
pub const NAME_COLOR: egui::Color32 = egui::Color32::from_rgb(234, 179, 80);
const HOVER_SECONDS: f64 = 0.4;
const MAX_GRAPH_SITES: usize = 200;

#[derive(Debug, Clone)]
pub struct GraphSite {
    pub node_id: u64,
    pub span: LineSpan,
    pub name: NameSpan,
}

#[derive(Debug)]
pub struct SymbolGraph {
    /// Definition first, followed by exact references in open files.
    pub sites: Vec<GraphSite>,
}

#[derive(Default)]
pub struct CanvasGraphState {
    pub graph: Option<SymbolGraph>,
    candidate: Option<(u64, LspSymbol, u64, f64)>,
    pending: Option<RequestId>,
    answer: Option<Vec<Location>>,
    queried: bool,
}

impl CanvasGraphState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn is_waiting(&self) -> bool {
        self.pending.is_some()
    }

    /// Only the active generation's request is accepted. Canceled/late replies
    /// remain unclaimed and cannot restore an overlay after the pointer leaves.
    pub fn receive(&mut self, id: RequestId, response: &LspResponse) -> bool {
        if self.pending != Some(id) {
            return false;
        }
        self.pending = None;
        if let LspResponse::References(Ok(locations)) = response {
            self.answer = Some(locations.clone());
        }
        true
    }
}

fn source_fingerprint(state: &AppState) -> u64 {
    let mut hash = DefaultHasher::new();
    state.loaded_dir.hash(&mut hash);
    let mut files: Vec<_> = state.node_store.file_nodes.values().collect();
    files.sort_by_key(|n| n.id);
    for node in files {
        node.id.hash(&mut hash);
        node.path.hash(&mut hash);
        node.content.hash(&mut hash);
    }
    hash.finish()
}

/// Runs before canvas painting, independently of which inspector tab is open.
/// Does not alter selection, reference history, open files, or the viewport.
pub fn update(ctx: &egui::Context, state: &mut AppState, picked: Option<(u64, LspSymbol)>) {
    let Some((node_id, symbol)) = picked.filter(|_| state.settings.lsp.query_on_hover) else {
        state.lsp.canvas_graph.clear();
        return;
    };
    let fingerprint = source_fingerprint(state);
    let now = ctx.input(|i| i.time);
    let graph = &mut state.lsp.canvas_graph;
    let same = graph
        .candidate
        .as_ref()
        .is_some_and(|(id, s, hash, _)| *id == node_id && *s == symbol && *hash == fingerprint);
    if !same {
        graph.clear();
        graph.candidate = Some((node_id, symbol.clone(), fingerprint, now));
    }
    let since = graph.candidate.as_ref().unwrap().3;
    if now - since < HOVER_SECONDS {
        ctx.request_repaint_after(Duration::from_millis(50));
        return;
    }
    if !graph.queried {
        let Some(node) = state.node_store.get_file(node_id) else {
            graph.clear();
            return;
        };
        graph.queried = true;
        if let Some((server, language_id)) = server_for(&node.path) {
            let root = state
                .loaded_dir
                .clone()
                .or_else(|| node.path.parent().map(std::path::Path::to_path_buf))
                .unwrap_or_default();
            graph.pending = Some(state.lsp.manager.request(
                Some(ctx),
                server,
                &root,
                LspRequest::References {
                    path: node.path.clone(),
                    language_id,
                    text: node.content.clone(),
                    position: symbol.selection_range.start,
                },
            ));
        }
    }
    if let Some(locations) = graph.answer.take() {
        graph.graph = build_graph(&state.node_store, node_id, &symbol, &locations);
    }
}

/// Convert only server-proven ranges, not textual matches. In particular, two
/// occurrences of the same spelling can belong to unrelated definitions.
fn make_site(
    store: &NodeStore,
    trees: &mut HashMap<u64, Option<tree_sitter::Tree>>,
    node_id: u64,
    range: Range,
) -> Option<GraphSite> {
    let node = store.get_file(node_id)?;
    if range.start.line != range.end.line {
        return None;
    }
    let start = byte_offset(&node.content, range.start.line, range.start.character)?;
    let end = byte_offset(&node.content, range.end.line, range.end.character)?;
    let line_start = byte_offset(&node.content, range.start.line, 0)?;
    if start >= end || end > node.content.len() {
        return None;
    }
    let tree = trees
        .entry(node_id)
        .or_insert_with(|| parse_tree(&node.path, &node.content).map(|(_, t)| t));
    let span = tree
        .as_ref()
        .and_then(|tree| context_span(tree, start))
        .unwrap_or(LineSpan {
            start: range.start.line,
            end: range.start.line,
        });
    Some(GraphSite {
        node_id,
        span,
        name: NameSpan {
            line: range.start.line,
            start_byte: start - line_start,
            end_byte: end - line_start,
        },
    })
}

fn context_span(tree: &tree_sitter::Tree, offset: usize) -> Option<LineSpan> {
    let mut current = tree.root_node().descendant_for_byte_range(offset, offset)?;
    let mut nearest = None;
    loop {
        let kind = current.kind();
        if matches!(
            kind,
            "impl_item"
                | "function_item"
                | "function_definition"
                | "function_declaration"
                | "method_definition"
                | "method_declaration"
                | "struct_item"
                | "enum_item"
                | "trait_item"
                | "class_declaration"
                | "class_definition"
                | "interface_declaration"
                | "type_declaration"
                | "lexical_declaration"
                | "const_item"
                | "let_declaration"
        ) {
            let end = current.end_position();
            let span = LineSpan {
                start: current.start_position().row,
                end: end.row.saturating_sub(usize::from(end.column == 0)),
            };
            if kind == "impl_item" {
                return Some(span);
            }
            nearest.get_or_insert(span);
        }
        let Some(parent) = current.parent() else {
            return nearest;
        };
        current = parent;
    }
}

fn build_graph(
    store: &NodeStore,
    definition: u64,
    symbol: &LspSymbol,
    locations: &[Location],
) -> Option<SymbolGraph> {
    let mut trees = HashMap::new();
    let mut sites = vec![make_site(
        store,
        &mut trees,
        definition,
        symbol.selection_range,
    )?];
    let mut sorted = locations.to_vec();
    sorted.sort();
    sorted.dedup();
    for location in sorted {
        if sites.len() >= MAX_GRAPH_SITES {
            break;
        }
        // Match canonical aliases just as dependency indexing does.
        let file = store
            .file_nodes
            .values()
            .find(|n| n.path == location.path)
            .or_else(|| {
                store
                    .file_nodes
                    .values()
                    .find(|n| std::fs::canonicalize(&n.path).ok().as_ref() == Some(&location.path))
            });
        let Some(file) = file else { continue };
        let Some(site) = make_site(store, &mut trees, file.id, location.range) else {
            continue;
        };
        if !sites
            .iter()
            .any(|s| s.node_id == site.node_id && s.name == site.name)
        {
            sites.push(site);
        }
    }
    Some(SymbolGraph { sites })
}

/// Paint blocks beneath code; returns the per-site block color for name marks.
pub fn site_color(index: usize) -> egui::Color32 {
    if index == 0 {
        DEFINITION_COLOR
    } else {
        CONTEXT_COLOR
    }
}

pub fn paint_block(painter: &egui::Painter, rect: egui::Rect, index: usize) {
    let color = site_color(index);
    painter.rect_filled(rect, 0.0, color.gamma_multiply(0.10));
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0_f32, color.gamma_multiply(0.65)),
        egui::StrokeKind::Inside,
    );
}

/// Connect measured identifier rects, never file ports. Intra-file references
/// loop through the left gutter instead of becoming zero-length self-edges.
pub fn paint_connections(
    painter: &egui::Painter,
    graph: &SymbolGraph,
    names: &[(usize, egui::Rect)],
) {
    let Some((_, definition)) = names.iter().find(|(i, _)| *i == 0) else {
        return;
    };
    for &(index, rect) in names.iter().filter(|(i, _)| *i != 0) {
        let same_file = graph.sites[index].node_id == graph.sites[0].node_id;
        let start = definition.left_center();
        let end = rect.left_center();
        let bend = if same_file {
            48.0 + (end.y - start.y).abs() * 0.15
        } else {
            (end.x - start.x).abs() * 0.4 + 32.0
        };
        let points = [
            start,
            start - egui::vec2(bend, 0.0),
            end - egui::vec2(bend, 0.0),
            end,
        ];
        painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
            points,
            false,
            egui::Color32::TRANSPARENT,
            egui::Stroke::new(1.0_f32, CONTEXT_COLOR.gamma_multiply(0.8)),
        ));
        painter.add(egui::Shape::convex_polygon(
            vec![
                end,
                end + egui::vec2(-7.0, -3.0),
                end + egui::vec2(-7.0, 3.0),
            ],
            CONTEXT_COLOR,
            egui::Stroke::NONE,
        ));
    }
}

/// Shared clipping geometry for block painting.
pub fn block_rect(
    site: &GraphSite,
    origin: egui::Pos2,
    width: f32,
    line_h: f32,
    visible: usize,
) -> Option<egui::Rect> {
    highlight_rect(
        origin,
        width,
        line_h,
        site.span.start,
        site.span.end,
        visible,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::lsp::{Position, SymbolKind};
    use std::path::PathBuf;

    fn range(line: usize, start: usize, end: usize) -> Range {
        Range {
            start: Position {
                line,
                character: start,
            },
            end: Position {
                line,
                character: end,
            },
        }
    }

    #[test]
    fn graph_keeps_only_exact_server_ranges_and_groups_full_impl_context() {
        let mut store = NodeStore::default();
        let path = PathBuf::from("/p/product.rs");
        let id = store.add_file(path.clone(), "struct Product {\n id: u64,\n}\nimpl Product {\n fn new() -> Product { Product { id: 1 } }\n}\n".into(), egui::Pos2::ZERO);
        let symbol = LspSymbol {
            name: "Product".into(),
            detail: None,
            kind: SymbolKind::Struct,
            range: range(0, 0, 16),
            selection_range: range(0, 7, 14),
            children: Vec::new(),
        };
        let locations = [
            Location {
                path: path.clone(),
                range: range(0, 7, 14),
            },
            Location {
                path: path.clone(),
                range: range(3, 5, 12),
            },
            Location {
                path: path.clone(),
                range: range(3, 5, 12),
            },
            Location {
                path: PathBuf::from("/closed.rs"),
                range: range(0, 0, 7),
            },
        ];
        let graph = build_graph(&store, id, &symbol, &locations).unwrap();
        assert_eq!(
            graph.sites.len(),
            2,
            "no textual scanning for unreported Product names"
        );
        assert_eq!(graph.sites[0].span, LineSpan { start: 0, end: 2 });
        assert_eq!(graph.sites[1].span, LineSpan { start: 3, end: 5 });
    }

    #[test]
    fn canceled_hover_rejects_late_replies() {
        let mut state = CanvasGraphState {
            pending: Some(42),
            ..Default::default()
        };
        state.clear();
        assert!(!state.receive(42, &LspResponse::References(Ok(Vec::new()))));
        assert!(state.answer.is_none() && state.graph.is_none());
    }

    #[test]
    fn exact_name_columns_convert_utf16_to_utf8_and_reject_bad_ranges() {
        let mut store = NodeStore::default();
        let id = store.add_file(
            PathBuf::from("a.rs"),
            "// 😀 Product Product\n".into(),
            egui::Pos2::ZERO,
        );
        let mut trees = HashMap::new();
        let site = make_site(&store, &mut trees, id, range(0, 6, 13)).unwrap();
        assert_eq!((site.name.start_byte, site.name.end_byte), (8, 15));
        assert!(make_site(&store, &mut trees, id, range(0, 99, 100)).is_none());
    }
}
