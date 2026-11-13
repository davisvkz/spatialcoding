//! Canvas actions shared by the UI, keyboard shortcuts and startup options.
//!
//! Everything here works on [`AppState`] only, so it can be tested without a UI.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::fs::loader::{load_directory, read_file_content};
use crate::fs::tree::TreeNode;
use crate::nodes::file_node::{line_height, CODE_PADDING, HEADER_HEIGHT};
use crate::state::AppState;
use crate::ui::lsp_panel::LineHighlight;

/// Newly opened files are placed on a grid until a layout is applied.
const GRID_COLUMNS: usize = 3;
const GRID_STEP: egui::Vec2 = egui::vec2(750.0, 400.0);

/// Opens `path` as a file node, adds it to its folder and rebuilds the
/// dependency edges. Unreadable or binary files become empty nodes.
/// Returns the new node id.
pub fn open_file(state: &mut AppState, path: &Path) -> u64 {
    let content = read_file_content(path).unwrap_or_default();
    let id = add_file_node(state, path, content);
    rebuild_edges(state);
    id
}

/// Opens each path that is not already on the canvas and rebuilds the edges
/// once. Unreadable and binary files are skipped. Returns the new node ids.
pub fn open_files(state: &mut AppState, paths: &[PathBuf]) -> Vec<u64> {
    let mut open: HashSet<PathBuf> = state
        .node_store
        .file_nodes
        .values()
        .map(|n| n.path.clone())
        .collect();
    let mut ids = Vec::new();
    for path in paths {
        if !open.insert(path.clone()) {
            continue;
        }
        match read_file_content(path) {
            Ok(content) => ids.push(add_file_node(state, path, content)),
            Err(e) => log::warn!("Skipping {}: {e}", path.display()),
        }
    }
    if !ids.is_empty() {
        rebuild_edges(state);
    }
    ids
}

/// Opens every text file of the loaded folder (Ctrl+Shift+O), honoring the
/// same skip rules as the file tree. Returns the new node ids.
pub fn open_all_files(state: &mut AppState) -> Vec<u64> {
    let Some(dir) = state.loaded_dir.clone() else {
        return Vec::new();
    };
    let tree = match load_directory(&dir, state.settings.show_dot_folders) {
        Ok(tree) => tree,
        Err(e) => {
            log::error!("Failed to load {}: {e}", dir.display());
            return Vec::new();
        }
    };
    let mut paths = Vec::new();
    collect_files(&tree, &mut paths);
    open_files(state, &paths)
}

fn collect_files(node: &TreeNode, out: &mut Vec<PathBuf>) {
    match node {
        TreeNode::File { path, .. } => out.push(path.clone()),
        TreeNode::Dir { children, .. } => {
            for child in children {
                collect_files(child, out);
            }
        }
    }
}

fn add_file_node(state: &mut AppState, path: &Path, content: String) -> u64 {
    let count = state.node_store.file_nodes.len();
    let position = egui::pos2(
        (count % GRID_COLUMNS) as f32 * GRID_STEP.x,
        (count / GRID_COLUMNS) as f32 * GRID_STEP.y,
    );
    let file_id = state.node_store.add_file(path.to_path_buf(), content, position);

    if let Some(parent_dir) = path.parent() {
        let folder_id = find_or_create_folder(state, parent_dir);
        if let Some(folder) = state.node_store.get_folder_mut(folder_id)
            && !folder.children_ids.contains(&file_id)
        {
            folder.children_ids.push(file_id);
        }
    }
    file_id
}

fn find_or_create_folder(state: &mut AppState, dir: &Path) -> u64 {
    let existing = state
        .node_store
        .folder_nodes
        .values()
        .find(|folder| folder.path == dir)
        .map(|folder| folder.id);
    existing.unwrap_or_else(|| state.node_store.add_folder(dir.to_path_buf(), 0))
}

/// Removes every node, folder, edge and the selection from the canvas.
pub fn clear_canvas(state: &mut AppState) {
    state.node_store.clear();
    state.edge_store.clear();
    state.selection.clear();
}

/// Removes file nodes together with their edges, their folder membership,
/// any folder left empty, and their selection.
pub fn remove_nodes(state: &mut AppState, ids: &[u64]) {
    let removed: HashSet<u64> = ids.iter().copied().collect();
    for &id in ids {
        for edge_id in state.edge_store.edges_for_node(id) {
            state.edge_store.remove(edge_id);
        }
        state.node_store.remove_node(id);
        if state.selection.is_selected(id) {
            state.selection.toggle(id);
        }
    }

    let mut empty_folders = Vec::new();
    for folder in state.node_store.folder_nodes.values_mut() {
        folder.children_ids.retain(|child| !removed.contains(child));
        if folder.children_ids.is_empty() {
            empty_folders.push(folder.id);
        }
    }
    for id in empty_folders {
        state.node_store.remove_node(id);
    }
}

/// Shows line `line` (0-based) of `path`: opens the file if it is not on the
/// canvas, makes it the only selection, marks the line as the LSP target and
/// asks the canvas to center on it next frame. Returns the node id.
pub fn focus_location(state: &mut AppState, path: &Path, line: usize) -> u64 {
    let id = find_file_node(state, path).unwrap_or_else(|| open_file(state, path));
    state.selection.select(id);
    state.lsp.target = Some(LineHighlight {
        node_id: id,
        start_line: line,
        end_line: line,
    });
    if let Some(node) = state.node_store.get_file(id) {
        let font_size = state.settings.font_size;
        let rect = node.world_rect(state.settings.node_width, font_size);
        // Lines past the node's visible ones are not drawn: aim at the last one.
        let shown = line.min(node.visible_lines().saturating_sub(1));
        let y = rect.top() + HEADER_HEIGHT + CODE_PADDING + (shown as f32 + 0.5) * line_height(font_size);
        state.pending_focus = Some(egui::pos2(rect.center().x, y));
    }
    id
}

/// The node showing `path`, also when one of the two paths goes through a
/// symlink (language servers report canonical paths).
fn find_file_node(state: &AppState, path: &Path) -> Option<u64> {
    let nodes = &state.node_store.file_nodes;
    if let Some(node) = nodes.values().find(|n| n.path == path) {
        return Some(node.id);
    }
    let canonical = std::fs::canonicalize(path).ok()?;
    nodes
        .values()
        .find(|n| std::fs::canonicalize(&n.path).is_ok_and(|p| p == canonical))
        .map(|n| n.id)
}

/// Recomputes every dependency edge between the open file nodes.
pub fn rebuild_edges(state: &mut AppState) {
    let root = match state.loaded_dir {
        Some(ref dir) => dir.clone(),
        None => return,
    };
    let files: Vec<PathBuf> = state
        .node_store
        .file_nodes
        .values()
        .map(|n| n.path.clone())
        .collect();
    let deps = crate::analysis::dependencies::analyze_dependencies(&root, &files);

    state.edge_store.clear();

    let path_to_id: HashMap<PathBuf, u64> = state
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, content: &str) -> PathBuf {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn rebuild_edges_links_dependent_file_nodes() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Cargo.toml", "[package]\nname = \"x\"\n");
        let main_rs = write(dir.path(), "src/main.rs", "mod util;\n");
        let util_rs = write(dir.path(), "src/util.rs", "");
        let app_ts = write(dir.path(), "web/app.ts", "import { a } from './a';\n");
        let a_ts = write(dir.path(), "web/a.ts", "");

        let mut state = AppState {
            loaded_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let mut ids = HashMap::new();
        for p in [&main_rs, &util_rs, &app_ts, &a_ts] {
            let id = state.node_store.add_file(p.clone(), String::new(), egui::Pos2::ZERO);
            ids.insert(p.clone(), id);
        }

        rebuild_edges(&mut state);

        let mut edges: Vec<(u64, u64, Option<String>)> = state
            .edge_store
            .all_edges()
            .map(|e| (e.source_node, e.target_node, e.label.clone()))
            .collect();
        edges.sort();
        let mut expected = vec![
            (ids[&main_rs], ids[&util_rs], Some("Module".to_string())),
            (ids[&app_ts], ids[&a_ts], Some("Import".to_string())),
        ];
        expected.sort();
        assert_eq!(edges, expected);
    }

    #[test]
    fn open_file_reads_content_places_on_grid_and_groups_by_folder() {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..4)
            .map(|i| write(dir.path(), &format!("src/f{i}.py"), &format!("x = {i}\n")))
            .collect();
        let other = write(dir.path(), "lib/g.py", "");

        let mut state = AppState {
            loaded_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let ids: Vec<u64> = paths.iter().map(|p| open_file(&mut state, p)).collect();
        open_file(&mut state, &other);

        let node = state.node_store.get_file(ids[1]).unwrap();
        assert_eq!(node.content, "x = 1\n");
        assert_eq!(node.position, egui::pos2(GRID_STEP.x, 0.0));
        assert_eq!(
            state.node_store.get_file(ids[3]).unwrap().position,
            egui::pos2(0.0, GRID_STEP.y),
            "fourth file wraps to the next row"
        );

        assert_eq!(state.node_store.folder_nodes.len(), 2);
        let src = state
            .node_store
            .folder_nodes
            .values()
            .find(|f| f.path == dir.path().join("src"))
            .unwrap();
        assert_eq!(src.children_ids, ids);
    }

    #[test]
    fn open_file_keeps_binary_files_as_empty_nodes() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("blob.bin");
        std::fs::write(&bin, b"\0\x01\x02").unwrap();

        let mut state = AppState::default();
        let id = open_file(&mut state, &bin);
        assert_eq!(state.node_store.get_file(id).unwrap().content, "");
    }

    #[test]
    fn open_files_skips_duplicates_already_open_and_binary_files() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Cargo.toml", "[package]\nname = \"x\"\n");
        let main_rs = write(dir.path(), "src/main.rs", "mod util;\n");
        let util_rs = write(dir.path(), "src/util.rs", "");
        let bin = dir.path().join("src/blob.bin");
        std::fs::write(&bin, b"\0\0").unwrap();

        let mut state = AppState {
            loaded_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let first = open_file(&mut state, &main_rs);
        let ids = open_files(
            &mut state,
            &[main_rs.clone(), util_rs.clone(), util_rs.clone(), bin],
        );

        assert_eq!(ids.len(), 1, "only util.rs is new and readable");
        assert_eq!(state.node_store.file_nodes.len(), 2);
        let edges: Vec<(u64, u64)> = state
            .edge_store
            .all_edges()
            .map(|e| (e.source_node, e.target_node))
            .collect();
        assert_eq!(edges, [(first, ids[0])]);
    }

    #[test]
    fn focus_location_opens_selects_and_aims_at_the_line() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "a.rs", "fn a() {}\n");
        let b = write(dir.path(), "b.rs", "\n\nfn b() {}\n");
        let mut state = AppState::default();
        let a_id = open_file(&mut state, &a);
        state.selection.select(a_id);

        let b_id = focus_location(&mut state, &b, 2);
        assert_ne!(a_id, b_id, "b.rs was opened");
        assert_eq!(state.selection.primary, Some(b_id));
        assert_eq!(state.selection.count(), 1);
        assert_eq!(
            state.lsp.target,
            Some(LineHighlight { node_id: b_id, start_line: 2, end_line: 2 })
        );
        let node = state.node_store.get_file(b_id).unwrap();
        let rect = node.world_rect(state.settings.node_width, state.settings.font_size);
        let lh = line_height(state.settings.font_size);
        let focus = state.pending_focus.unwrap();
        assert_eq!(focus.x, rect.center().x);
        assert!((focus.y - (rect.top() + HEADER_HEIGHT + CODE_PADDING + 2.5 * lh)).abs() < 1e-3);

        // An open file is reused, also through a symlinked path.
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(dir.path(), &link).unwrap();
        assert_eq!(focus_location(&mut state, &link.join("a.rs"), 0), a_id);
        assert_eq!(state.node_store.file_nodes.len(), 2);
        assert_eq!(state.selection.primary, Some(a_id));
    }

    #[test]
    fn open_all_files_opens_text_files_of_the_loaded_folder() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "src/a.py", "import b\n");
        write(dir.path(), "src/b.py", "");
        write(dir.path(), "README.md", "# hi\n");
        write(dir.path(), "target/out.txt", "build output");
        write(dir.path(), ".hidden/secret.txt", "dot folder");
        std::fs::write(dir.path().join("logo.png"), b"\x89PNG\0").unwrap();

        let mut state = AppState::default();
        assert!(open_all_files(&mut state).is_empty(), "no folder loaded");

        state.loaded_dir = Some(dir.path().to_path_buf());
        assert_eq!(open_all_files(&mut state).len(), 3);
        let mut names: Vec<&str> = state
            .node_store
            .file_nodes
            .values()
            .map(|n| n.file_name())
            .collect();
        names.sort();
        assert_eq!(names, ["README.md", "a.py", "b.py"]);

        assert!(open_all_files(&mut state).is_empty(), "second run opens nothing new");
    }
}
