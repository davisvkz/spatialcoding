//! Canvas actions shared by the UI, keyboard shortcuts and startup options.
//!
//! Everything here works on [`AppState`] only, so it can be tested without a UI.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::fs::loader::{load_directory, read_file_content};
use crate::fs::tree::TreeNode;
use crate::analysis::dependencies::OpenFiles;
use crate::state::AppState;
use crate::ui::lsp_panel::LineHighlight;

/// Newly opened files are placed on a grid until a layout is applied.
const GRID_COLUMNS: usize = 3;
const GRID_STEP: egui::Vec2 = egui::vec2(750.0, 400.0);

/// Opens `path` as a file node, adds it to its folder and rebuilds the
/// dependency edges. Unreadable or binary files become empty nodes.
/// A file already on the canvas is never opened twice: its existing node id is
/// returned instead. Returns the node id.
pub fn open_file(state: &mut AppState, path: &Path) -> u64 {
    if let Some(id) = find_file_node(state, path) {
        return id;
    }
    let content = read_file_content(path).unwrap_or_default();
    let id = add_file_node(state, path, content);
    state.layout_dirty = true;
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
    let folders_before: HashSet<u64> = state.node_store.folder_nodes.keys().copied().collect();
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
        let mut new_folders: Vec<u64> = state
            .node_store
            .folder_nodes
            .keys()
            .copied()
            .filter(|id| !folders_before.contains(id))
            .collect();
        new_folders.sort();
        state.last_opened_folders = new_folders;
        state.layout_dirty = true;
        rebuild_edges(state);
    }
    ids
}

/// Opens many files at once (Open all, a folder scope, folders with
/// changes): like [`open_files`], then the folders it created at the
/// collapse depth start collapsed when "Open with collapsed folders by
/// default" is on.
pub fn open_files_in_bulk(state: &mut AppState, paths: &[PathBuf]) -> Vec<u64> {
    state.last_opened_folders.clear();
    let ids = open_files(state, paths);
    let new_folders = std::mem::take(&mut state.last_opened_folders);
    crate::folder_actions::collapse_new_folders(state, &new_folders, ids.len());
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
    open_files_in_bulk(state, &paths)
}

/// Opens the text files in a Workspace folder and all of its descendants.
pub fn open_folder_on_canvas(state: &mut AppState, dir: &Path) -> Vec<u64> {
    let paths: Vec<PathBuf> = folder_text_files(state)
        .iter()
        .filter(|path| path.starts_with(dir))
        .cloned()
        .collect();
    open_files_in_bulk(state, &paths)
}

/// Closes a folder subtree on the canvas only; nothing is deleted from disk.
pub fn remove_folder_from_canvas(state: &mut AppState, dir: &Path) {
    let ids: Vec<u64> = state.node_store.file_nodes.values()
        .filter(|node| node.path.starts_with(dir))
        .map(|node| node.id)
        .collect();
    remove_nodes(state, &ids);
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
    let files = &state.settings.files;
    if files.collapse_long
        && let Some(node) = state.node_store.get_file_mut(file_id)
        && node.line_count > files.long_max_lines
    {
        node.collapsed = true;
    }

    if let Some(parent_dir) = path.parent() {
        let folder_id = find_or_create_folder(state, parent_dir);
        if let Some(folder) = state.node_store.get_folder_mut(folder_id)
            && !folder.children_ids.contains(&file_id)
        {
            folder.children_ids.push(file_id);
        }
        // Directories between the loaded folder and this one get a folder too,
        // so nested boxes show the whole path (v2: `src` ⊃ `analysis` ⊃ `lsp`).
        if let Some(root) = state.loaded_dir.clone() {
            for dir in parent_dir.ancestors().skip(1) {
                if dir == root || !dir.starts_with(&root) {
                    break;
                }
                find_or_create_folder(state, dir);
            }
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
    state.git_comparison.invalidate();
    state.top_level_highlights.clear();
    state.node_store.clear();
    state.edge_store.clear();
    state.selection.clear();
}

/// Removes file nodes together with their edges, their folder membership,
/// any folder left empty, and their selection.
pub fn remove_nodes(state: &mut AppState, ids: &[u64]) {
    state.git_comparison.invalidate();
    let removed: HashSet<u64> = ids.iter().copied().collect();
    for &id in ids {
        state.top_level_highlights.remove(&id);
        for edge_id in state.edge_store.edges_for_node(id) {
            state.edge_store.remove(edge_id);
        }
        state.node_store.remove_node(id);
        state.selection.remove_node(id);
    }

    // A folder stays while any open file still lives under it, so the
    // intermediate folders of a nested path survive until it is emptied.
    let remaining: Vec<PathBuf> =
        state.node_store.file_nodes.values().map(|n| n.path.clone()).collect();
    let mut empty_folders = Vec::new();
    for folder in state.node_store.folder_nodes.values_mut() {
        folder.children_ids.retain(|child| !removed.contains(child));
        if !remaining.iter().any(|p| p.starts_with(&folder.path)) {
            empty_folders.push(folder.id);
        }
    }
    for id in empty_folders {
        state.node_store.remove_node(id);
    }
    if !ids.is_empty() {
        state.layout_dirty = true;
    }
    apply_dependency_edges(state);
}

/// Lays the canvas out with the Layout panel settings, keeping the primary
/// selection where it is on screen: the view, and a pending jump to a
/// location, move with it.
pub fn apply_layout_keeping_selection(state: &mut AppState) {
    let anchor = state.selection.primary;
    let before = anchor.and_then(|id| state.node_store.get_file(id)).map(|n| n.position);
    crate::layout::hierarchical::apply_layout(
        &mut state.node_store,
        &state.edge_store,
        &state.layout_settings,
    );
    state.layout_dirty = false;
    let after = anchor.and_then(|id| state.node_store.get_file(id)).map(|n| n.position);
    if let (Some(before), Some(after)) = (before, after) {
        let delta = after - before;
        state.viewport.offset -= delta;
        if let Some(focus) = state.pending_focus.as_mut() {
            *focus += delta;
        }
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
        let y = node.line_anchor_y(shown, font_size).unwrap_or(rect.center().y);
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

/// Hands the open file nodes to the dependency indexer, which asks their
/// language servers in the background, and redraws the edges known so far.
/// New edges arrive through [`poll_lsp`].
pub fn rebuild_edges(state: &mut AppState) {
    state.git_comparison.invalidate();
    let Some(root) = state.loaded_dir.clone() else {
        return;
    };
    let files: Vec<(&Path, &str)> = state
        .node_store
        .file_nodes
        .values()
        .map(|n| (n.path.as_path(), n.content.as_str()))
        .collect();
    state.deps.rebuild(&mut state.lsp.manager, &root, &files);
    apply_dependency_edges(state);
}

/// Hands the language servers' answers to the LSP tab and the dependency
/// indexer, and updates the edges when the indexer learned something. Called
/// every frame.
pub fn poll_lsp(state: &mut AppState) {
    for (id, response) in state.lsp.manager.poll() {
        if let Some(response) = state.lsp.receive(id, response) {
            state.deps.receive(&mut state.lsp.manager, id, response);
        }
    }
    state.deps.tick(&mut state.lsp.manager);
    auto_open_matches(state);
    if state.deps.take_changed() {
        apply_dependency_edges(state);
    }
    let refreshed = state.changes.poll();
    if let Some(diff) = &mut state.diff {
        diff.poll();
        // The Git status was read again: the file may have changed.
        if refreshed && !diff.loader.is_loading() {
            diff.loader.request();
        }
    }
    if state.git_comparison.poll() {
        // Also reject direct in-memory content changes made without a rebuild.
        let signature = comparison_input(state).map(|input| input.signature());
        if signature != state.git_comparison.requested_signature {
            state.git_comparison.invalidate();
        }
        apply_dependency_edges(state);
    }
}

/// LSP "Auto Open Matches": once a symbol's references arrive, open the
/// files holding them (at most Max files), once per answer.
fn auto_open_matches(state: &mut AppState) {
    let ready = state.lsp.references.as_ref().is_some_and(|r| {
        !r.auto_opened && matches!(r.fetch, crate::ui::lsp_panel::Fetch::Done(Ok(_)))
    });
    if !ready {
        return;
    }
    if let Some(refs) = state.lsp.references.as_mut() {
        refs.auto_opened = true;
    }
    if !state.settings.lsp.auto_open_matches {
        return;
    }
    let max = state.settings.lsp.max_files.max(1);
    let open: HashSet<PathBuf> = state.node_store.file_nodes.values().map(|n| n.path.clone()).collect();
    let paths: Vec<PathBuf> = crate::ui::lsp_panel::reference_files(state)
        .into_iter()
        .filter(|p| !open.contains(p) && p.is_file())
        .take(max)
        .collect();
    if !paths.is_empty() {
        open_files(state, &paths);
    }
}

fn comparison_input(state: &AppState) -> Option<crate::git::comparison::GraphInput> {
    Some(crate::git::comparison::GraphInput {
        root: state.loaded_dir.clone()?,
        files: state.node_store.file_nodes.values().map(|n| (n.path.clone(), n.content.clone())).collect(),
    })
}

/// No Git commands run here: only a canvas snapshot and worker dispatch.
pub fn refresh_git_comparison(state: &mut AppState) {
    use crate::git::repo::{MAX_FILES, MAX_FILE_BYTES, MAX_TOTAL_BYTES};
    let nodes = &state.node_store.file_nodes;
    if nodes.len() > MAX_FILES || nodes.values().any(|n| n.content.len() > MAX_FILE_BYTES)
        || nodes.values().map(|n| n.content.len()).sum::<usize>() > MAX_TOTAL_BYTES {
        invalidate_git_comparison(state);
        state.git_comparison.status = crate::git::comparison::ComparisonStatus::Error("Comparison limits: 512 files, 2 MiB/file, 32 MiB total.".into());
        return;
    }
    rebuild_edges(state);
    if let Some(input) = comparison_input(state) { state.git_comparison.request(input); }
}

pub fn invalidate_git_comparison(state: &mut AppState) {
    state.git_comparison.invalidate();
    apply_dependency_edges(state);
}

/// Replaces the edges with one per link the indexer knows between open files.
/// Classifies each link as Direct / ReExport / Barrel for the v2-aligned
/// Dependency Edges panel.
fn apply_dependency_edges(state: &mut AppState) {
    use crate::analysis::dependencies::classify::{classify_link, promote_barrels};
    use crate::edges::edge_store::NameSpan;

    let nodes = &state.node_store.file_nodes;
    let open = OpenFiles::new(nodes.values().map(|n| (n.path.as_path(), n.content.as_str())));
    let ids: HashMap<&Path, u64> = nodes.values().map(|n| (n.path.as_path(), n.id)).collect();
    let texts: HashMap<&Path, &str> = nodes
        .values()
        .map(|n| (n.path.as_path(), n.content.as_str()))
        .collect();

    let mut links = state.deps.links(&open);
    let mut changes: Vec<_> = links.iter().map(|l| state.git_comparison.diff.as_ref().map(|d| d.state_for(l)).unwrap_or_default()).collect();
    // Per-link classification, then a barrel promotion pass.
    let mut kinds: Vec<_> = links
        .iter()
        .map(|link| match texts.get(link.source.as_path()) {
            Some(text) => classify_link(link, text),
            None => crate::edges::edge_store::EdgeKind::Direct,
        })
        .collect();
    let sources: Vec<&Path> = links.iter().map(|l| l.source.as_path()).collect();
    kinds = promote_barrels(&kinds, &sources);
    // Historical links retain historical classification and must not inflate
    // the current file's re-export count into a barrel.
    if let Some(diff) = &state.git_comparison.diff {
        changes.extend(diff.deleted.iter().map(|_| crate::edges::edge_store::EdgeChange::Deleted));
        links.extend(diff.deleted.iter().cloned());
        kinds.extend(diff.deleted_kinds.iter().copied());
    }

    state.edge_store.clear();
    for ((link, kind), change) in links.into_iter().zip(kinds).zip(changes) {
        if let (Some(&src), Some(&tgt)) = (ids.get(link.source.as_path()), ids.get(link.target.as_path())) {
            let deleted = change == crate::edges::edge_store::EdgeChange::Deleted;
            let name_span = link.name_span.filter(|_| !deleted).map(|(line, start, end)| NameSpan {
                line,
                start_byte: start,
                end_byte: end,
            });
            let source_name_span = link.source_name_span.filter(|_| !deleted).map(|(line, start_byte, end_byte)| NameSpan {
                line, start_byte, end_byte,
            });
            let id = state.edge_store.add_full(
                src,
                tgt,
                Some(link.label),
                link.source_label,
                (!deleted).then_some(link.usage),
                (!deleted).then_some(link.definition),
                name_span,
                source_name_span,
                kind,
            );
            state.edge_store.set_relationship(id, link.relationship);
            state.edge_store.set_change(id, change);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::file_node::{line_height, CODE_PADDING, HEADER_HEIGHT};

    #[test]
    fn git_refresh_rejects_oversized_canvas_before_spawning_or_rebuilding() {
        let mut state = AppState { loaded_dir: Some("/p".into()), ..Default::default() };
        state.node_store.add_file("/p/large.ts".into(), "x".repeat(crate::git::repo::MAX_FILE_BYTES + 1), egui::Pos2::ZERO);
        refresh_git_comparison(&mut state);
        assert!(matches!(state.git_comparison.status, crate::git::comparison::ComparisonStatus::Error(_)));
        assert!(!state.git_comparison.is_loading());
        assert!(!state.deps.is_waiting());
    }

    #[test]
    fn top_level_symbols_are_the_root_declarations_without_comments_or_skipped_imports() {
        let src = "use std::fmt;\n// note\nfn a() {\n    let x = 1;\n}\nstruct B;\n";
        let path = Path::new("/p/lib.rs");
        assert_eq!(top_level_symbols(path, src, true), vec![(2, 4), (5, 5)]);
        assert_eq!(top_level_symbols(path, src, false), vec![(0, 0), (2, 4), (5, 5)]);
        assert!(top_level_symbols(Path::new("/p/notes.txt"), src, true).is_empty());
    }

    #[test]
    fn long_files_open_collapsed_and_expand_all_restores_them() {
        let dir = tempfile::tempdir().unwrap();
        let long = write(dir.path(), "long.rs", &"x\n".repeat(30));
        let short = write(dir.path(), "short.rs", "x\n");
        let mut state = state_in(dir.path());
        state.settings.files.long_max_lines = 10;
        let ids = open_files(&mut state, &[long, short]);
        let collapsed: Vec<bool> = ids.iter().map(|id| state.node_store.get_file(*id).unwrap().collapsed).collect();
        assert_eq!(collapsed, vec![true, false]);
        assert_eq!(state.node_store.get_file(ids[0]).unwrap().visible_lines(), 0);
        assert_eq!(collapsed_files(&state), 1);
        expand_all_files(&mut state);
        assert_eq!(collapsed_files(&state), 0);
        assert_eq!(state.node_store.get_file(ids[0]).unwrap().visible_lines(), 30);

        state.settings.files.collapse_long = false;
        let other = write(dir.path(), "other.rs", &"x\n".repeat(30));
        let id = open_files(&mut state, &[other])[0];
        assert!(!state.node_store.get_file(id).unwrap().collapsed, "off: opens expanded");
    }

    #[test]
    fn changed_file_actions_use_the_working_tree_change_set() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let a = write(&root, "a.rs", "x\n");
        let b = write(&root, "b.rs", "x\n");
        let mut state = state_in(&root);
        let a_id = open_files(&mut state, std::slice::from_ref(&a))[0];
        assert!(unchanged_nodes(&state).is_empty(), "unknown changes close nothing");
        state.changes.set_for_test([b.clone()]);
        assert_eq!(unopened_changed_files(&state), vec![b.clone()]);
        assert_eq!(unchanged_nodes(&state), vec![a_id]);
        let opened = open_changed_files(&mut state);
        assert_eq!(opened.len(), 1);
        close_unchanged_files(&mut state);
        let paths: Vec<PathBuf> = state.node_store.file_nodes.values().map(|n| n.path.clone()).collect();
        assert_eq!(paths, vec![b]);
    }

    #[test]
    fn import_neighbours_follow_local_imports_both_ways() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let app = write(&root, "web/app.ts", "import { a } from './a';\nconsole.log(a);\n");
        let a = write(&root, "web/a.ts", "export const a = 1;\n");
        write(&root, "web/unrelated.ts", "export const z = 2;\n");
        let mut state = state_in(&root);
        let a_id = open_files(&mut state, std::slice::from_ref(&a))[0];
        state.selection.select(a_id);
        assert_eq!(import_neighbours(&state, ImportDirection::Importers), vec![app.clone()]);
        assert!(import_neighbours(&state, ImportDirection::Imported).is_empty());

        clear_canvas(&mut state);
        let app_id = open_files(&mut state, std::slice::from_ref(&app))[0];
        state.selection.select(app_id);
        assert_eq!(import_neighbours(&state, ImportDirection::Imported), vec![a]);
    }

    #[test]
    fn top_level_highlight_toggles_for_the_same_selection_and_drops_removed_nodes() {
        let dir = tempfile::tempdir().unwrap();
        let f = write(dir.path(), "m.rs", "fn a() {}\nfn b() {}\n");
        let mut state = state_in(dir.path());
        let id = open_files(&mut state, &[f])[0];
        state.selection.select(id);
        toggle_top_level_highlights(&mut state);
        assert_eq!(state.top_level_highlights[&id], vec![(0, 0), (1, 1)]);
        toggle_top_level_highlights(&mut state);
        assert!(state.top_level_highlights.is_empty());
        toggle_top_level_highlights(&mut state);
        remove_nodes(&mut state, &[id]);
        assert!(state.top_level_highlights.is_empty());
    }

    fn write(dir: &Path, rel: &str, content: &str) -> PathBuf {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    /// A state for `dir` whose language servers fail to start at once, so
    /// unit tests never spawn a real one (the e2e tests use a fake server).
    fn state_in(dir: &Path) -> AppState {
        let mut state = AppState {
            loaded_dir: Some(dir.to_path_buf()),
            ..Default::default()
        };
        state
            .lsp
            .manager
            .set_launcher(std::sync::Arc::new(|_, _| Err("no server in unit tests".into())));
        state
    }

    #[test]
    fn rebuild_edges_asks_the_language_servers_in_the_background() {
        let dir = tempfile::tempdir().unwrap();
        let main_rs = write(dir.path(), "src/main.rs", "mod util;\n");
        let mut state = state_in(dir.path());
        state.node_store.add_file(main_rs, "mod util;\n".into(), egui::Pos2::ZERO);

        rebuild_edges(&mut state);
        assert!(state.deps.is_waiting(), "symbols requested");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.deps.is_waiting() {
            assert!(std::time::Instant::now() < deadline, "the indexer never settled");
            poll_lsp(&mut state);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(state.edge_store.all_edges().count(), 0);
        let progress = state.deps.progress(&state.lsp.manager);
        assert_eq!(progress[0].error.as_deref(), Some("no server in unit tests"));
    }

    #[test]
    fn open_file_reads_content_places_on_grid_and_groups_by_folder() {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..4)
            .map(|i| write(dir.path(), &format!("src/f{i}.py"), &format!("x = {i}\n")))
            .collect();
        let other = write(dir.path(), "lib/g.py", "");

        let mut state = state_in(dir.path());
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

        let mut state = state_in(dir.path());
        let first = open_file(&mut state, &main_rs);
        assert_eq!(open_file(&mut state, &main_rs), first, "reopening reuses the node");
        let ids = open_files(
            &mut state,
            &[main_rs.clone(), util_rs.clone(), util_rs.clone(), bin],
        );

        assert_eq!(ids.len(), 1, "only util.rs is new and readable");
        assert_eq!(state.node_store.file_nodes.len(), 2);
        assert_ne!(first, ids[0]);
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
    fn nested_files_create_their_intermediate_folders_until_removed() {
        let dir = tempfile::tempdir().unwrap();
        let deep = write(dir.path(), "src/analysis/lsp/client.rs", "fn c() {}\n");
        let top = write(dir.path(), "src/main.rs", "fn main() {}\n");
        let mut state = state_in(dir.path());
        let deep_id = open_file(&mut state, &deep);
        let top_id = open_file(&mut state, &top);

        let mut paths: Vec<PathBuf> =
            state.node_store.folder_nodes.values().map(|f| f.path.clone()).collect();
        paths.sort();
        let rel = |p: &str| dir.path().join(p);
        assert_eq!(paths, [rel("src"), rel("src/analysis"), rel("src/analysis/lsp")]);

        // `src/` keeps its own file; the now-empty deep chain goes away.
        remove_nodes(&mut state, &[deep_id]);
        let paths: Vec<PathBuf> =
            state.node_store.folder_nodes.values().map(|f| f.path.clone()).collect();
        assert_eq!(paths, [rel("src")]);
        remove_nodes(&mut state, &[top_id]);
        assert!(state.node_store.folder_nodes.is_empty());
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

        let mut state = state_in(dir.path());
        state.loaded_dir = None;
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

// -- File nodes panel actions ------------------------------------------------

/// Asks Git which files of the loaded folder changed; the answer arrives
/// through [`poll_lsp`]. `now` is the app time, for the periodic refresh.
pub fn refresh_working_tree_changes(state: &mut AppState, now: f64) {
    state.folder_files_cache.replace(None);
    if let Some(root) = state.loaded_dir.clone() {
        state.changes.request(&root, now);
    }
}

/// What [`apply_fs_changes`] did, for the parts of the app that cache more.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FsApplied {
    /// Open files whose text was replaced.
    pub reloaded: Vec<PathBuf>,
    /// Files or folders appeared or vanished: the file tree must be read again.
    pub tree_stale: bool,
}

/// Makes the canvas follow the disk: open files whose text changed are read
/// again in place, open files that were deleted (or sit in a deleted folder)
/// are closed, and the Git status is asked again. The edges are rebuilt once
/// when any text changed.
pub fn apply_fs_changes(state: &mut AppState, batch: &crate::fs::watcher::WatchBatch) -> FsApplied {
    let mut applied = FsApplied::default();
    if batch.is_empty() {
        return applied;
    }
    let mut grew_or_shrank = false;
    // A save through a temp file shows up as a created path.
    for path in batch.modified.iter().chain(&batch.created) {
        let Some(id) = find_file_node(state, path) else { continue };
        let Ok(content) = read_file_content(path) else { continue };
        let lines_before = state.node_store.get_file(id).map(|n| n.line_count);
        if state.node_store.replace_content(id, content) {
            state.top_level_highlights.remove(&id);
            grew_or_shrank |= lines_before != state.node_store.get_file(id).map(|n| n.line_count);
            applied.reloaded.push(path.clone());
        }
    }
    let gone: Vec<u64> = state
        .node_store
        .file_nodes
        .values()
        .filter(|n| batch.removed.iter().any(|r| n.path == *r || n.path.starts_with(r)))
        .map(|n| n.id)
        .collect();
    if !gone.is_empty() {
        remove_nodes(state, &gone);
    }
    if !applied.reloaded.is_empty() {
        if grew_or_shrank {
            state.layout_dirty = true;
        }
        rebuild_edges(state);
    }
    applied.tree_stale = !batch.created.is_empty() || !batch.removed.is_empty();
    // Ask Git again as soon as no request is running.
    state.folder_files_cache.replace(None);
    state.changes.requested_at = None;
    applied
}

/// Opens the diff window for `path` (a file against Git HEAD).
pub fn open_diff(state: &mut AppState, path: PathBuf) {
    state.diff = Some(crate::ui::diff_view::DiffViewState::open(path));
}

/// Opens the diff of the primary selected file node, when there is one.
pub fn open_diff_of_selection(state: &mut AppState) {
    let path = state.selection.primary.and_then(|id| state.node_store.get_file(id)).map(|n| n.path.clone());
    if let Some(path) = path {
        open_diff(state, path);
    }
}

/// Changed files of the loaded folder that are not on the canvas yet.
pub fn unopened_changed_files(state: &AppState) -> Vec<PathBuf> {
    let Some(root) = state.loaded_dir.as_deref() else {
        return Vec::new();
    };
    let open: HashSet<&Path> = state.node_store.file_nodes.values().map(|n| n.path.as_path()).collect();
    let mut paths: Vec<PathBuf> = state
        .changes
        .paths()
        .filter(|p| p.starts_with(root) && p.is_file() && !open.contains(p.as_path()))
        .cloned()
        .collect();
    paths.sort();
    paths
}

/// Opens every changed file of the folder. Returns the new node ids.
pub fn open_changed_files(state: &mut AppState) -> Vec<u64> {
    let paths = unopened_changed_files(state);
    open_files(state, &paths)
}

/// File nodes whose file has no change against Git HEAD.
pub fn unchanged_nodes(state: &AppState) -> Vec<u64> {
    if !state.changes.is_known() {
        return Vec::new();
    }
    let mut ids: Vec<u64> = state
        .node_store
        .file_nodes
        .values()
        .filter(|n| !state.changes.is_changed(&n.path))
        .map(|n| n.id)
        .collect();
    ids.sort();
    ids
}

/// Removes the file nodes without changes.
pub fn close_unchanged_files(state: &mut AppState) {
    let ids = unchanged_nodes(state);
    remove_nodes(state, &ids);
}

/// Paths of the files of the loaded folder, as the file tree lists them.
/// Read once and cached until the folder or the Git change set refreshes.
pub fn folder_text_files(state: &AppState) -> std::rc::Rc<Vec<PathBuf>> {
    if let Some(cached) = state.folder_files_cache.borrow().as_ref() {
        return cached.clone();
    }
    let mut paths = Vec::new();
    if let Some(dir) = state.loaded_dir.as_deref()
        && let Ok(tree) = load_directory(dir, state.settings.show_dot_folders)
    {
        collect_files(&tree, &mut paths);
    }
    let paths = std::rc::Rc::new(paths);
    *state.folder_files_cache.borrow_mut() = Some(paths.clone());
    paths
}

/// Every text file of the loaded folder with its content.
fn workspace_files(state: &AppState) -> Vec<(PathBuf, String)> {
    folder_text_files(state)
        .iter()
        .filter_map(|p| read_file_content(p).ok().map(|text| (p.clone(), text)))
        .collect()
}

/// Which way to follow import links from the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportDirection {
    /// Files whose imports resolve to a selected file.
    Importers,
    /// Files a selected file imports.
    Imported,
}

/// Files of the folder linked to the selected file nodes by an import that
/// resolves locally (decision-27's syntax evidence), not open yet.
pub fn import_neighbours(state: &AppState, direction: ImportDirection) -> Vec<PathBuf> {
    let selected: HashSet<PathBuf> = state
        .selection
        .selected_ids()
        .filter_map(|id| state.node_store.get_file(id))
        .map(|n| n.path.clone())
        .collect();
    let Some(root) = state.loaded_dir.clone() else {
        return Vec::new();
    };
    if selected.is_empty() {
        return Vec::new();
    }
    let files = workspace_files(state);
    let open = OpenFiles::new(files.iter().map(|(p, t)| (p.as_path(), t.as_str())));
    let opened: HashSet<&Path> = state.node_store.file_nodes.values().map(|n| n.path.as_path()).collect();
    let mut out: Vec<PathBuf> = crate::analysis::dependencies::local::links(&root, &open)
        .into_iter()
        .filter(|l| l.relationship == crate::edges::style::RelationshipType::Imports)
        .filter_map(|l| match direction {
            ImportDirection::Importers => selected.contains(&l.target).then_some(l.source),
            ImportDirection::Imported => selected.contains(&l.source).then_some(l.target),
        })
        .filter(|p| !opened.contains(p.as_path()))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Opens the files linked to the selection in `direction`.
pub fn open_import_neighbours(state: &mut AppState, direction: ImportDirection) -> Vec<u64> {
    let paths = import_neighbours(state, direction);
    open_files(state, &paths)
}

/// Line ranges (0-based, inclusive) of the top-level declarations of a
/// file: the syntax root's named children, without comments, and without
/// imports when `skip_imports`.
pub fn top_level_symbols(path: &Path, content: &str, skip_imports: bool) -> Vec<(usize, usize)> {
    let Some(tree) = crate::analysis::cst::parse_cst(path, content) else {
        return Vec::new();
    };
    tree.root
        .children
        .iter()
        .filter(|n| n.named && !n.is_error && !n.kind.contains("comment"))
        .filter(|n| !(skip_imports && is_import_kind(&n.kind)))
        .map(|n| (n.start_line, n.end_line))
        .collect()
}

fn is_import_kind(kind: &str) -> bool {
    matches!(
        kind,
        "use_declaration"
            | "extern_crate_declaration"
            | "import_statement"
            | "import_from_statement"
            | "future_import_statement"
            | "import_declaration"
            | "package_clause"
    )
}

/// File nodes › "Highlight top-level symbols": marks the top-level
/// declarations of the selected files, or clears the marks when they are
/// already shown for exactly this selection.
pub fn toggle_top_level_highlights(state: &mut AppState) {
    let mut ids: Vec<u64> = state.selection.selected_ids().collect();
    ids.sort();
    let mut shown: Vec<u64> = state.top_level_highlights.keys().copied().collect();
    shown.sort();
    if !ids.is_empty() && ids == shown {
        state.top_level_highlights.clear();
        return;
    }
    state.top_level_highlights.clear();
    let skip_imports = state.settings.files.skip_imports;
    for id in ids {
        if let Some(node) = state.node_store.get_file(id) {
            let ranges = top_level_symbols(&node.path, &node.content, skip_imports);
            state.top_level_highlights.insert(id, ranges);
        }
    }
}

/// File nodes collapsed by "Collapse long files" (or by hand).
pub fn collapsed_files(state: &AppState) -> usize {
    state.node_store.file_nodes.values().filter(|n| n.collapsed).count()
}

/// "Expand all N collapsed files".
pub fn expand_all_files(state: &mut AppState) {
    for node in state.node_store.file_nodes.values_mut() {
        node.collapsed = false;
    }
    state.layout_dirty = true;
}
