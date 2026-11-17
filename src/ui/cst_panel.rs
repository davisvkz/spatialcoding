//! Right-rail "CST" panel: the tree-sitter syntax tree of the primary
//! selected file node, filtered by the CST settings (depth, hidden types,
//! semantic nodes). Hovering a row highlights its lines on the canvas, and
//! the syntax nodes at the ends of the selection's edges are outlined
//! (painted by the app shell through [`highlight_rect`]).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::analysis::cst::{parse_cst, CstNode, CstTree};
use crate::nodes::node_store::NodeStore;
use crate::state::{AppState, CstSettings};
use crate::ui::icons;
use crate::ui::panel::{switch, Section};

const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(0xef, 0x44, 0x44);

/// The CST row under the pointer in the current frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CstHover {
    pub node_id: u64,
    /// 0-based, inclusive.
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Default)]
pub struct CstPanelState {
    cached: Option<CachedCst>,
    /// Trees of the files at the ends of selected edges, by node id.
    edge_trees: std::collections::HashMap<u64, CachedCst>,
    /// Set while the pointer is over a row; reset every frame the panel draws.
    pub hovered: Option<CstHover>,
    /// With Single Hover Symbol off: the hovered row's ancestors' lines.
    pub hover_ancestors: Vec<(usize, usize)>,
    /// "Selected Edge CST Nodes": per file node, the lines of the innermost
    /// named CST node at each end of the selection's edges.
    pub edge_nodes: std::collections::HashMap<u64, Vec<(usize, usize)>>,
    /// The settings group under the gear is open.
    pub show_settings: bool,
    /// How many times a file was parsed (cache misses).
    parses: usize,
}

struct CachedCst {
    node_id: u64,
    source_hash: u64,
    tree: Option<CstTree>,
}

fn source_hash(node: &crate::nodes::file_node::FileNode) -> u64 {
    let mut hasher = DefaultHasher::new();
    node.path.hash(&mut hasher);
    node.content.hash(&mut hasher);
    hasher.finish()
}

impl CstPanelState {
    /// Makes [`Self::tree`] match file node `node_id`, parsing only when the
    /// node or its path/content changed since the last call.
    pub fn sync(&mut self, node_store: &NodeStore, node_id: u64) {
        let Some(node) = node_store.get_file(node_id) else {
            self.cached = None;
            return;
        };
        let source_hash = source_hash(node);
        let fresh = self
            .cached
            .as_ref()
            .is_some_and(|c| c.node_id == node_id && c.source_hash == source_hash);
        if !fresh {
            self.parses += 1;
            self.cached = Some(CachedCst {
                node_id,
                source_hash,
                tree: parse_cst(&node.path, &node.content),
            });
        }
    }

    /// The tree of the last synced node, if its language has a grammar.
    pub fn tree(&self) -> Option<&CstTree> {
        self.cached.as_ref()?.tree.as_ref()
    }

    /// The (cached) tree of any file node, for edge-end highlights.
    fn tree_of(&mut self, node_store: &NodeStore, node_id: u64) -> Option<&CstTree> {
        let node = node_store.get_file(node_id)?;
        let hash = source_hash(node);
        let fresh = self.edge_trees.get(&node_id).is_some_and(|c| c.source_hash == hash);
        if !fresh {
            self.parses += 1;
            let tree = parse_cst(&node.path, &node.content);
            self.edge_trees.insert(node_id, CachedCst { node_id, source_hash: hash, tree });
        }
        self.edge_trees.get(&node_id)?.tree.as_ref()
    }

    /// Clears what the canvas paints for this panel.
    pub fn clear_highlights(&mut self) {
        self.hovered = None;
        self.hover_ancestors.clear();
        self.edge_nodes.clear();
    }
}

/// Grammar kinds that only group other nodes. "Semantic Nodes Only" lists
/// their children in their place.
pub fn is_wrapper(kind: &str) -> bool {
    kind.ends_with("_list")
        || matches!(
            kind,
            "block"
                | "statement_block"
                | "compound_statement"
                | "parameters"
                | "formal_parameters"
                | "arguments"
                | "argument_list"
                | "type_arguments"
                | "type_parameters"
                | "parenthesized_expression"
                | "expression_statement"
                | "class_body"
                | "interface_body"
        )
}

/// The rows the tree lists under `node`: its children after the Hidden
/// Types and Semantic Nodes Only filters (wrappers replaced by their own
/// listed children).
fn listed_children<'a>(node: &'a CstNode, settings: &CstSettings) -> Vec<&'a CstNode> {
    let mut out = Vec::new();
    for child in &node.children {
        if !(settings.show_hidden_types || child.named || child.is_error) {
            continue;
        }
        if settings.semantic_only && child.named && !child.is_error && is_wrapper(&child.kind) {
            out.extend(listed_children(child, settings));
        } else {
            out.push(child);
        }
    }
    out
}

/// How many rows the tree lists, within the depth limit.
fn count_rows(node: &CstNode, settings: &CstSettings, depth: usize) -> usize {
    1 + if settings.limit_depth && depth >= settings.max_depth {
        0
    } else {
        listed_children(node, settings)
            .into_iter()
            .map(|c| count_rows(c, settings, depth + 1))
            .sum()
    }
}

/// The innermost named node covering `line`, as (start, end) lines.
fn innermost_at(node: &CstNode, line: usize) -> Option<(usize, usize)> {
    if line < node.start_line || line > node.end_line {
        return None;
    }
    node.children
        .iter()
        .filter(|c| c.named)
        .find_map(|c| innermost_at(c, line))
        .or(Some((node.start_line, node.end_line)))
}

/// Limit CST depth and Max depth, shared by the CST settings and
/// Performance › CST Depth.
pub fn draw_depth_controls(ui: &mut egui::Ui, settings: &mut CstSettings) {
    switch(ui, &mut settings.limit_depth, "Limit CST depth")
        .on_hover_text("List syntax nodes only down to Max depth.");
    ui.add_enabled_ui(settings.limit_depth, |ui| {
        ui.horizontal(|ui| {
            ui.label("Max depth");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let response = ui.add(egui::DragValue::new(&mut settings.max_depth).range(1..=64usize));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Max CST depth")
                });
            });
        });
    });
}

pub fn draw_cst_panel(ui: &mut egui::Ui, state: &mut AppState) {
    state.cst.clear_highlights();
    let settings = state.settings.cst.clone();

    if settings.selected_edge_nodes && settings.show_canvas {
        collect_edge_nodes(state);
    }

    let primary = state.selection.primary.filter(|id| state.node_store.get_file(*id).is_some());
    let count = primary.and_then(|id| {
        state.cst.sync(&state.node_store, id);
        state.cst.tree().map(|t| count_rows(&t.root, &settings, 0))
    });
    ui.horizontal(|ui| {
        ui.weak(format!("({})", count.unwrap_or(0)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let gear = icons::tool_icon_button(ui, icons::SETTINGS, "Toggle settings");
            if gear.clicked() {
                state.cst.show_settings = !state.cst.show_settings;
            }
        });
    });
    switch(ui, &mut state.settings.cst.selected_edge_nodes, "Selected Edge CST Nodes")
        .on_hover_text("Outline the syntax node at each end of the selection's edges.");
    if state.cst.show_settings {
        Section::new("Settings").show(ui, |ui| {
            let cst = &mut state.settings.cst;
            Section::new("CST Depth").show(ui, |ui| draw_depth_controls(ui, cst));
            switch(ui, &mut cst.show_hidden_types, "Show Hidden Types")
                .on_hover_text("Also list anonymous tokens such as fn, ( and ;.");
            switch(ui, &mut cst.semantic_only, "Semantic Nodes Only")
                .on_hover_text("Skip blocks, parameter and argument lists and similar wrappers.");
            switch(ui, &mut cst.single_hover, "Single Hover Symbol")
                .on_hover_text("Hovering a row highlights only its node, not the nodes around it.");
        });
    }
    ui.separator();

    let Some(node_id) = primary else {
        if state.node_store.file_nodes.is_empty() {
            ui.label("Open some files on the canvas to start populating the CST nodes list.");
        } else {
            ui.label("Select a file node to see its syntax tree.");
        }
        return;
    };
    let node = state.node_store.get_file(node_id).unwrap();
    ui.label(egui::RichText::new(node.file_name()).strong());
    let Some(tree) = state.cst.tree() else {
        ui.label(format!("No syntax tree: {} is not supported.", node.language));
        return;
    };
    ui.label(format!("{} \u{b7} {} nodes", tree.language.name(), tree.node_count));
    if tree.truncated {
        ui.colored_label(ERROR_COLOR, "Large file: only the first nodes are shown.");
    }

    let settings = state.settings.cst.clone();
    let mut hovered: Option<Vec<(usize, usize)>> = None;
    if let Some(tree) = state.cst.tree() {
        let id = ui.make_persistent_id(("cst", node_id));
        let mut path = Vec::new();
        draw_node(ui, &tree.root, &settings, id, 0, &mut path, &mut hovered);
    }
    if let Some(mut chain) = hovered {
        let (start_line, end_line) = chain.pop().expect("the hovered row itself");
        state.cst.hovered = Some(CstHover { node_id, start_line, end_line });
        if !settings.single_hover {
            state.cst.hover_ancestors = chain;
        }
    }
}

/// Fills [`CstPanelState::edge_nodes`] from the selection's visible edges.
fn collect_edge_nodes(state: &mut AppState) {
    let mut ends: Vec<(u64, usize)> = Vec::new();
    for edge in state.edge_store.all_edges() {
        if !edge.is_selected(&state.selection) || !edge.is_visible_for(&state.edge_style, &state.selection) {
            continue;
        }
        if let Some(span) = edge.source_span {
            ends.push((edge.source_node, span.start));
        }
        if let Some(span) = edge.target_span {
            ends.push((edge.target_node, span.start));
        }
    }
    ends.sort();
    ends.dedup();
    // Bounded: a hub file can carry hundreds of edges.
    for (node_id, line) in ends.into_iter().take(200) {
        let Some(tree) = state.cst.tree_of(&state.node_store, node_id) else { continue };
        if let Some(range) = innermost_at(&tree.root, line) {
            let ranges = state.cst.edge_nodes.entry(node_id).or_default();
            if !ranges.contains(&range) {
                ranges.push(range);
            }
        }
    }
}

fn draw_node(
    ui: &mut egui::Ui,
    node: &CstNode,
    settings: &CstSettings,
    id: egui::Id,
    depth: usize,
    path: &mut Vec<(usize, usize)>,
    hovered: &mut Option<Vec<(usize, usize)>>,
) {
    let mut text = egui::RichText::new(row_label(node)).monospace();
    if node.is_error {
        text = text.color(ERROR_COLOR);
    } else if !node.named {
        text = text.weak();
    }

    let at_limit = settings.limit_depth && depth >= settings.max_depth;
    let children = if at_limit { Vec::new() } else { listed_children(node, settings) };
    path.push((node.start_line, node.end_line));

    let response = if children.is_empty() {
        let hidden_below = at_limit && !listed_children(node, settings).is_empty();
        if hidden_below {
            ui.label(text.clone()).on_hover_text("Deeper nodes are past Max depth.")
        } else {
            ui.label(text)
        }
    } else {
        egui::CollapsingHeader::new(text)
            .id_salt(id)
            .default_open(depth == 0)
            .show(ui, |ui| {
                for (i, child) in children.into_iter().enumerate() {
                    draw_node(ui, child, settings, id.with(i), depth + 1, path, hovered);
                }
            })
            .header_response
    };
    if response.hovered() {
        *hovered = Some(path.clone());
    }
    path.pop();
}

/// `field: kind "text"  L3–L7`, with 1-based lines.
pub fn row_label(node: &CstNode) -> String {
    let mut label = String::new();
    if let Some(field) = &node.field {
        label.push_str(field);
        label.push_str(": ");
    }
    label.push_str(&node.kind);
    if let Some(text) = &node.text {
        label.push_str(&format!(" {text:?}"));
    }
    if node.start_line == node.end_line {
        label.push_str(&format!("  L{}", node.start_line + 1));
    } else {
        label.push_str(&format!("  L{}\u{2013}{}", node.start_line + 1, node.end_line + 1));
    }
    label
}

/// Screen rect covering code lines `start..=end` (0-based) of a file node
/// whose first code line starts at `top_left`. Lines past the node's
/// `visible_lines` are cut off; `None` if none of the range is visible.
pub fn highlight_rect(
    top_left: egui::Pos2,
    width: f32,
    line_h: f32,
    start: usize,
    end: usize,
    visible_lines: usize,
) -> Option<egui::Rect> {
    if start >= visible_lines || end < start {
        return None;
    }
    let end = end.min(visible_lines - 1);
    Some(egui::Rect::from_min_max(
        egui::pos2(top_left.x, top_left.y + start as f32 * line_h),
        egui::pos2(top_left.x + width, top_left.y + (end + 1) as f32 * line_h),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn store_with(path: &str, content: &str) -> (NodeStore, u64) {
        let mut store = NodeStore::default();
        let id = store.add_file(PathBuf::from(path), content.to_string(), egui::Pos2::ZERO);
        (store, id)
    }

    fn rust_tree(src: &str) -> CstTree {
        parse_cst(std::path::Path::new("/p/a.rs"), src).unwrap()
    }

    fn kinds(nodes: &[&CstNode]) -> Vec<String> {
        nodes.iter().map(|n| n.kind.clone()).collect()
    }

    #[test]
    fn semantic_nodes_only_lists_a_wrappers_children_in_its_place() {
        let tree = rust_tree("fn f(a: u8) {\n    let x = a;\n}\n");
        let function = &tree.root.children[0];
        let mut settings = CstSettings { semantic_only: false, ..Default::default() };
        let plain = kinds(&listed_children(function, &settings));
        assert!(plain.contains(&"parameters".to_owned()) && plain.contains(&"block".to_owned()), "{plain:?}");
        settings.semantic_only = true;
        let semantic = kinds(&listed_children(function, &settings));
        assert!(!semantic.contains(&"block".to_owned()) && !semantic.contains(&"parameters".to_owned()));
        assert!(semantic.contains(&"parameter".to_owned()) && semantic.contains(&"let_declaration".to_owned()), "{semantic:?}");
        settings.show_hidden_types = true;
        assert!(kinds(&listed_children(function, &settings)).contains(&"fn".to_owned()));
    }

    #[test]
    fn max_depth_bounds_the_listed_rows() {
        let tree = rust_tree("mod m {\n    fn f() {\n        let x = 1;\n    }\n}\n");
        let deep = CstSettings { limit_depth: false, ..Default::default() };
        let one = CstSettings { limit_depth: true, max_depth: 1, ..Default::default() };
        assert_eq!(count_rows(&tree.root, &one, 0), 2, "source_file and mod_item");
        assert!(count_rows(&tree.root, &deep, 0) > count_rows(&tree.root, &one, 0));
    }

    #[test]
    fn innermost_named_node_covers_the_line() {
        let tree = rust_tree("fn f() {\n    let x = 1;\n}\n");
        assert_eq!(innermost_at(&tree.root, 1), Some((1, 1)), "the let statement");
        assert_eq!(innermost_at(&tree.root, 0), Some((0, 0)));
        assert_eq!(innermost_at(&tree.root, 99), None);
    }

    #[test]
    fn sync_parses_once_until_the_content_changes() {
        let (mut store, id) = store_with("a.rs", "fn a() {}\n");
        let mut cst = CstPanelState::default();

        cst.sync(&store, id);
        cst.sync(&store, id);
        assert_eq!(cst.parses, 1);
        assert_eq!(cst.tree().unwrap().root.kind, "source_file");

        store.get_file_mut(id).unwrap().content = "struct S;\n".into();
        cst.sync(&store, id);
        assert_eq!(cst.parses, 2);
        let root = &cst.tree().unwrap().root;
        assert_eq!(root.children[0].kind, "struct_item");
    }

    #[test]
    fn sync_switches_nodes_and_handles_unsupported_and_missing_ones() {
        let (mut store, rs) = store_with("a.rs", "fn a() {}\n");
        let md = store.add_file(PathBuf::from("README.md"), "# hi\n".into(), egui::Pos2::ZERO);
        let mut cst = CstPanelState::default();

        cst.sync(&store, rs);
        assert!(cst.tree().is_some());
        cst.sync(&store, md);
        assert!(cst.tree().is_none(), "Markdown has no grammar");
        cst.sync(&store, 999);
        assert!(cst.tree().is_none());
        assert_eq!(cst.parses, 2);
    }

    #[test]
    fn row_label_shows_field_kind_text_and_one_based_lines() {
        let node = CstNode {
            kind: "identifier".into(),
            field: Some("name".into()),
            named: true,
            is_error: false,
            start_line: 2,
            end_line: 2,
            text: Some("hello".into()),
            children: Vec::new(),
        };
        assert_eq!(row_label(&node), "name: identifier \"hello\"  L3");

        let block = CstNode {
            kind: "block".into(),
            field: None,
            text: None,
            end_line: 6,
            ..node
        };
        assert_eq!(row_label(&block), "block  L3\u{2013}7");
    }

    #[test]
    fn highlight_rect_covers_the_lines_and_clips_to_visible_ones() {
        let origin = egui::pos2(10.0, 100.0);
        let r = highlight_rect(origin, 50.0, 2.0, 1, 3, 10).unwrap();
        assert_eq!(r.min, egui::pos2(10.0, 102.0));
        assert_eq!(r.max, egui::pos2(60.0, 108.0));

        let clipped = highlight_rect(origin, 50.0, 2.0, 8, 20, 10).unwrap();
        assert_eq!(clipped.max.y, 120.0);

        assert!(highlight_rect(origin, 50.0, 2.0, 10, 12, 10).is_none());
        assert!(highlight_rect(origin, 50.0, 2.0, 0, 0, 0).is_none());
    }
}
