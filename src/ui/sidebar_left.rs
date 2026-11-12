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
            children,
            expanded,
            ..
        } => {
            let file_count = count_files(children);
            let header = format!("{name} ({file_count})");
            let id = ui.make_persistent_id(name.as_str());
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                *expanded,
            )
            .show_header(ui, |ui| {
                if ui.selectable_label(false, &header).clicked() {
                    *expanded = !*expanded;
                }
            })
            .body(|ui| {
                for child in children.iter_mut() {
                    draw_tree_node(ui, child, state);
                }
            });
        }
        TreeNode::File { name, path } => {
            let icon = file_icon(name);
            let label = format!("{icon} {name}");
            if ui.selectable_label(false, &label).clicked() {
                let content = crate::fs::loader::read_file_content(path)
                    .unwrap_or_default();
                let count = state.node_store.file_nodes.len() as f32;
                let col = count % 3.0;
                let row = (count / 3.0).floor();
                let position = egui::pos2(col * 750.0, row * 400.0);
                let file_id = state.node_store.add_file(path.clone(), content, position);

                if let Some(parent_dir) = path.parent() {
                    let folder_id = find_or_create_folder(state, parent_dir);
                    if let Some(folder) = state.node_store.get_folder_mut(folder_id) {
                        if !folder.children_ids.contains(&file_id) {
                            folder.children_ids.push(file_id);
                        }
                    }
                }

                rebuild_edges(state);
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

fn find_or_create_folder(state: &mut AppState, dir: &std::path::Path) -> u64 {
    for id in state.node_store.all_folder_ids() {
        if let Some(folder) = state.node_store.get_folder(id) {
            if folder.path == dir {
                return id;
            }
        }
    }
    state.node_store.add_folder(dir.to_path_buf(), 0)
}

fn rebuild_edges(state: &mut AppState) {
    let root = match state.loaded_dir {
        Some(ref dir) => dir.clone(),
        None => return,
    };
    let files: Vec<std::path::PathBuf> = state
        .node_store
        .file_nodes
        .values()
        .map(|n| n.path.clone())
        .collect();
    let deps = crate::analysis::dependencies::analyze_dependencies(&root, &files);

    state.edge_store.clear();

    let path_to_id: std::collections::HashMap<std::path::PathBuf, u64> = state
        .node_store
        .file_nodes
        .values()
        .map(|n| (n.path.clone(), n.id))
        .collect();

    for dep in deps {
        if let (Some(&src_id), Some(&tgt_id)) =
            (path_to_id.get(&dep.source), path_to_id.get(&dep.target))
        {
            let label = format!("{:?}", dep.kind);
            state.edge_store.add(src_id, tgt_id, Some(label));
        }
    }
}
