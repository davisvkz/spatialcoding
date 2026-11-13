//! Right-sidebar "CST" tab: the tree-sitter syntax tree of the primary
//! selected file node. Hovering a row highlights its lines on the canvas
//! (painted by the app shell through [`highlight_rect`]).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::analysis::cst::{parse_cst, CstNode, CstTree};
use crate::nodes::node_store::NodeStore;
use crate::state::AppState;

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
    /// Also list anonymous tokens (`fn`, `(`, `;`, ...).
    pub show_anonymous: bool,
    /// Set while the pointer is over a row; reset every frame the panel draws.
    pub hovered: Option<CstHover>,
    /// How many times a file was parsed (cache misses).
    parses: usize,
}

struct CachedCst {
    node_id: u64,
    source_hash: u64,
    tree: Option<CstTree>,
}

impl CstPanelState {
    /// Makes [`Self::tree`] match file node `node_id`, parsing only when the
    /// node or its path/content changed since the last call.
    pub fn sync(&mut self, node_store: &NodeStore, node_id: u64) {
        let Some(node) = node_store.get_file(node_id) else {
            self.cached = None;
            return;
        };
        let mut hasher = DefaultHasher::new();
        node.path.hash(&mut hasher);
        node.content.hash(&mut hasher);
        let source_hash = hasher.finish();

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
}

pub fn draw_cst_panel(ui: &mut egui::Ui, state: &mut AppState) {
    state.cst.hovered = None;
    ui.heading("Syntax Tree");
    ui.separator();

    let Some(node_id) = state
        .selection
        .primary
        .filter(|id| state.node_store.get_file(*id).is_some())
    else {
        ui.label("Select a file node to see its syntax tree.");
        return;
    };
    let node = state.node_store.get_file(node_id).unwrap();
    ui.label(egui::RichText::new(node.file_name()).strong());

    state.cst.sync(&state.node_store, node_id);
    let Some(tree) = state.cst.tree() else {
        ui.label(format!("No syntax tree: {} is not supported.", node.language));
        return;
    };
    ui.label(format!("{} \u{b7} {} nodes", tree.language.name(), tree.node_count));
    if tree.truncated {
        ui.colored_label(ERROR_COLOR, "Large file: only the first nodes are shown.");
    }

    ui.checkbox(&mut state.cst.show_anonymous, "Show anonymous nodes");
    ui.separator();

    let show_anonymous = state.cst.show_anonymous;
    let mut hovered = None;
    if let Some(tree) = state.cst.tree() {
        let id = ui.make_persistent_id(("cst", node_id));
        draw_node(ui, &tree.root, show_anonymous, id, true, &mut hovered);
    }
    state.cst.hovered = hovered.map(|(start_line, end_line)| CstHover {
        node_id,
        start_line,
        end_line,
    });
}

fn draw_node(
    ui: &mut egui::Ui,
    node: &CstNode,
    show_anonymous: bool,
    id: egui::Id,
    is_root: bool,
    hovered: &mut Option<(usize, usize)>,
) {
    let mut text = egui::RichText::new(row_label(node)).monospace();
    if node.is_error {
        text = text.color(ERROR_COLOR);
    } else if !node.named {
        text = text.weak();
    }

    let children: Vec<(usize, &CstNode)> = node
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| show_anonymous || c.named || c.is_error)
        .collect();

    let response = if children.is_empty() {
        ui.label(text)
    } else {
        egui::CollapsingHeader::new(text)
            .id_salt(id)
            .default_open(is_root)
            .show(ui, |ui| {
                for (i, child) in children {
                    draw_node(ui, child, show_anonymous, id.with(i), false, hovered);
                }
            })
            .header_response
    };
    if response.hovered() {
        *hovered = Some((node.start_line, node.end_line));
    }
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
