use crate::fs::tree::TreeNode;
use crate::state::{AppState, LeftPanel};

pub fn draw_sidebar_left(
    ctx: &egui::Context,
    state: &mut AppState,
    tree: &mut Option<TreeNode>,
) {
    egui::SidePanel::left("sidebar_left")
        .default_width(280.0)
        .width_range(180.0..=500.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for &tab in &[
                    LeftPanel::Workspace,
                    LeftPanel::Changes,
                    LeftPanel::Search,
                    LeftPanel::LocalServer,
                ] {
                    let label = match tab {
                        LeftPanel::Workspace => "Workspace",
                        LeftPanel::Changes => "Changes",
                        LeftPanel::Search => "Search",
                        LeftPanel::LocalServer => "Server",
                    };
                    let selected = state.left_panel == Some(tab);
                    if ui.selectable_label(selected, label).clicked() {
                        state.left_panel = Some(tab);
                    }
                }
            });

            ui.separator();

            match state.left_panel {
                Some(LeftPanel::Workspace) => {
                    if let Some(root) = tree {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            draw_tree_node(ui, root, state);
                        });
                    } else {
                        ui.label("No folder loaded");
                    }
                }
                Some(LeftPanel::Changes) => {
                    super::changes_panel::draw_changes_panel(ui);
                }
                Some(LeftPanel::Search) => {
                    super::search_panel::draw_search_panel(ui);
                }
                Some(LeftPanel::LocalServer) => {
                    ui.label("Not implemented");
                }
                None => {}
            }
        });
}

fn draw_tree_node(ui: &mut egui::Ui, node: &mut TreeNode, state: &mut AppState) {
    match node {
        TreeNode::Dir {
            name,
            path,
            children,
            expanded,
            ..
        } => {
            use egui::collapsing_header::CollapsingState;

            let file_count = count_files(children);
            let header = format!("{name} ({file_count})");
            // Keyed by path: folders with the same name must not share state.
            let id = ui.make_persistent_id(&*path);
            let (_, header_response, _) =
                CollapsingState::load_with_default_open(ui.ctx(), id, *expanded)
                    .show_header(ui, |ui| ui.selectable_label(false, &header).clicked())
                    .body(|ui| {
                        for child in children.iter_mut() {
                            draw_tree_node(ui, child, state);
                        }
                    });

            // The name toggles the folder too, not only the arrow. The open
            // state lives in egui memory, so toggle it there.
            if let Some(mut collapsing) = CollapsingState::load(ui.ctx(), id) {
                if header_response.inner {
                    collapsing.toggle(ui);
                    collapsing.store(ui.ctx());
                }
                *expanded = collapsing.is_open();
            }
        }
        TreeNode::File { name, path } => {
            let icon = file_icon(name);
            let label = format!("{icon} {name}");
            if ui.selectable_label(false, &label).clicked() {
                crate::workspace::open_file(state, path);
            }
        }
    }
}

fn count_files(children: &[TreeNode]) -> usize {
    children
        .iter()
        .map(|c| match c {
            TreeNode::File { .. } => 1,
            TreeNode::Dir { children, .. } => count_files(children),
        })
        .sum()
}

fn file_icon(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("rs") => "\u{1f9f1}",
        Some("toml") => "\u{2699}",
        Some("md") => "\u{1f4dd}",
        Some("json") => "\u{1f4cb}",
        Some("js" | "ts" | "jsx" | "tsx") => "\u{1f4dc}",
        Some("py") => "\u{1f40d}",
        Some("html" | "css") => "\u{1f310}",
        _ => "\u{1f4c4}",
    }
}
