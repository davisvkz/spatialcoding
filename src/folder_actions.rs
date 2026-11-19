//! Folders panel actions: collapse and expand folder boxes by level, by
//! changes or all at once, open or close files by folder, the folder scope,
//! and the collapse applied when many files open at once.
//!
//! Like `workspace`, everything works on [`AppState`] only.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::nodes::node_store::FolderView;
use crate::state::AppState;

/// Collapses or expands one folder, then asks for a new layout.
pub fn set_collapsed(state: &mut AppState, folder: u64, collapsed: bool) {
    let (w, f) = (state.settings.node_width, state.settings.font_size);
    state.node_store.set_folder_collapsed(folder, collapsed, w, f);
    state.layout_dirty = true;
}

/// Double-click on a folder: flip it.
pub fn toggle_folder(state: &mut AppState, folder: u64) {
    let collapsed = state.node_store.get_folder(folder).is_some_and(|f| f.collapsed);
    set_collapsed(state, folder, !collapsed);
}

pub fn set_many(state: &mut AppState, folders: &[u64], collapsed: bool) {
    let (w, f) = (state.settings.node_width, state.settings.font_size);
    // Refresh the boxes first so a collapsed folder keeps its top-left.
    state.node_store.update_folder_bounds(w, f, state.layout_settings.folder_padding);
    for &id in folders {
        state.node_store.set_folder_collapsed(id, collapsed, w, f);
    }
    if !folders.is_empty() {
        state.layout_dirty = true;
    }
}

/// Folders on the canvas that are not inside a collapsed one, with their depth.
fn shown_folders(state: &AppState) -> Vec<(u64, usize, bool)> {
    let store = &state.node_store;
    let hidden = store.hidden_folders();
    let root = state.loaded_dir.clone().unwrap_or_default();
    let mut out: Vec<(u64, usize, bool)> = store
        .folder_nodes
        .values()
        .filter(|f| !hidden.contains(&f.id))
        .map(|f| (f.id, store.folder_depth(f.id, &root), f.collapsed))
        .collect();
    out.sort();
    out
}

/// The expanded folders at the deepest shown level.
pub fn deepest_expanded(state: &AppState) -> Vec<u64> {
    let expanded: Vec<(u64, usize, bool)> = shown_folders(state).into_iter().filter(|f| !f.2).collect();
    let Some(max) = expanded.iter().map(|f| f.1).max() else { return Vec::new() };
    expanded.into_iter().filter(|f| f.1 == max).map(|f| f.0).collect()
}

/// The collapsed folders at the shallowest shown level.
pub fn shallowest_collapsed(state: &AppState) -> Vec<u64> {
    let collapsed: Vec<(u64, usize, bool)> = shown_folders(state).into_iter().filter(|f| f.2).collect();
    let Some(min) = collapsed.iter().map(|f| f.1).min() else { return Vec::new() };
    collapsed.into_iter().filter(|f| f.1 == min).map(|f| f.0).collect()
}

/// Expanded shown folders with no expanded folder inside them.
pub fn leaf_folders(state: &AppState) -> Vec<u64> {
    let store = &state.node_store;
    let shown = shown_folders(state);
    let parents_of_expanded: HashSet<u64> = shown
        .iter()
        .filter(|f| !f.2)
        .filter_map(|f| store.folder_parent(f.0))
        .collect();
    shown
        .into_iter()
        .filter(|f| !f.2 && !parents_of_expanded.contains(&f.0))
        .map(|f| f.0)
        .collect()
}

/// Collapsed folders not inside another collapsed folder.
pub fn topmost_collapsed(state: &AppState) -> Vec<u64> {
    shown_folders(state).into_iter().filter(|f| f.2).map(|f| f.0).collect()
}

/// The folder and every folder nested in it.
pub fn folder_subtree(state: &AppState, folder: u64) -> Vec<u64> {
    let Some(root) = state.node_store.get_folder(folder).map(|f| f.path.clone()) else {
        return Vec::new();
    };
    let mut ids: Vec<u64> = state
        .node_store
        .folder_nodes
        .values()
        .filter(|f| f.path.starts_with(&root))
        .map(|f| f.id)
        .collect();
    ids.sort();
    ids
}

/// Folders at absolute `level` (the loaded folder's children are level 1)
/// that are in the given collapse state.
pub fn folders_at_level(state: &AppState, level: usize, collapsed: bool) -> Vec<u64> {
    let root = state.loaded_dir.clone().unwrap_or_default();
    let mut ids: Vec<u64> = state
        .node_store
        .folder_nodes
        .values()
        .filter(|f| f.collapsed == collapsed && state.node_store.folder_depth(f.id, &root) == level)
        .map(|f| f.id)
        .collect();
    ids.sort();
    ids
}

/// The folder block under the pointer, if the pointer is on the canvas.
pub fn folder_under_pointer(state: &AppState) -> Option<u64> {
    let world = state.viewport.screen_to_world(state.hover_screen?);
    state.node_store.folder_block_at_position(world)
}

fn all_folders(state: &AppState, collapsed: bool) -> Vec<u64> {
    let mut ids: Vec<u64> = state
        .node_store
        .folder_nodes
        .values()
        .filter(|f| f.collapsed == collapsed)
        .map(|f| f.id)
        .collect();
    ids.sort();
    ids
}

/// Whether a folder holds a file that differs from Git HEAD.
fn has_changes(state: &AppState, path: &Path) -> bool {
    state.changes.paths().any(|p| p.starts_with(path))
}

fn folders_by_changes(state: &AppState, changed: bool, collapsed: bool) -> Vec<u64> {
    if !state.changes.is_known() {
        return Vec::new();
    }
    let mut ids: Vec<u64> = state
        .node_store
        .folder_nodes
        .values()
        .filter(|f| f.collapsed == collapsed && has_changes(state, &f.path) == changed)
        .map(|f| f.id)
        .collect();
    ids.sort();
    ids
}

/// One Folders panel action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderAction {
    CollapseDeepest,
    ExpandShallowest,
    CollapseLeaves,
    ExpandTopmost,
    CollapseAll,
    ExpandAll,
    CollapseUnchanged,
    ExpandChanged,
    CloseUnchanged,
    OpenChanged,
}

impl FolderAction {
    pub const ALL: [FolderAction; 10] = [
        FolderAction::CollapseDeepest,
        FolderAction::ExpandShallowest,
        FolderAction::CollapseLeaves,
        FolderAction::ExpandTopmost,
        FolderAction::CollapseAll,
        FolderAction::ExpandAll,
        FolderAction::CollapseUnchanged,
        FolderAction::ExpandChanged,
        FolderAction::CloseUnchanged,
        FolderAction::OpenChanged,
    ];

    /// v2's accessible name for the button.
    pub fn label(self) -> &'static str {
        match self {
            FolderAction::CollapseDeepest => "Collapse deepest level",
            FolderAction::ExpandShallowest => "Expand shallowest level",
            FolderAction::CollapseLeaves => "Collapse leaf folders",
            FolderAction::ExpandTopmost => "Expand topmost collapsed folders",
            FolderAction::CollapseAll => "Collapse all folders",
            FolderAction::ExpandAll => "Expand all folders",
            FolderAction::CollapseUnchanged => "Collapse all folders without changes",
            FolderAction::ExpandChanged => "Expand all folders with changes",
            FolderAction::CloseUnchanged => "Close folders without changes",
            FolderAction::OpenChanged => "Open all folders with changes",
        }
    }

    /// Whether it needs the Git change set.
    pub fn uses_changes(self) -> bool {
        matches!(
            self,
            FolderAction::CollapseUnchanged
                | FolderAction::ExpandChanged
                | FolderAction::CloseUnchanged
                | FolderAction::OpenChanged
        )
    }

    /// True when clicking would change something now.
    pub fn applies(self, state: &AppState) -> bool {
        match self {
            FolderAction::CloseUnchanged => !files_in_unchanged_folders(state).is_empty(),
            FolderAction::OpenChanged => !files_of_changed_folders(state).is_empty(),
            _ => !self.targets(state).is_empty(),
        }
    }

    fn targets(self, state: &AppState) -> Vec<u64> {
        match self {
            FolderAction::CollapseDeepest => deepest_expanded(state),
            FolderAction::ExpandShallowest => shallowest_collapsed(state),
            FolderAction::CollapseLeaves => leaf_folders(state),
            FolderAction::ExpandTopmost => topmost_collapsed(state),
            FolderAction::CollapseAll => all_folders(state, false),
            FolderAction::ExpandAll => all_folders(state, true),
            FolderAction::CollapseUnchanged => folders_by_changes(state, false, false),
            FolderAction::ExpandChanged => folders_by_changes(state, true, true),
            FolderAction::CloseUnchanged | FolderAction::OpenChanged => Vec::new(),
        }
    }

    pub fn run(self, state: &mut AppState) {
        match self {
            FolderAction::CloseUnchanged => {
                let ids = files_in_unchanged_folders(state);
                crate::workspace::remove_nodes(state, &ids);
            }
            FolderAction::OpenChanged => {
                let paths = files_of_changed_folders(state);
                crate::workspace::open_files_in_bulk(state, &paths);
            }
            FolderAction::ExpandAll | FolderAction::ExpandShallowest | FolderAction::ExpandTopmost => {
                let ids = self.targets(state);
                set_many(state, &ids, false);
            }
            FolderAction::ExpandChanged => {
                // Also open the way down to them.
                let mut ids = self.targets(state);
                ids.extend(ids.clone().into_iter().flat_map(|id| ancestors(state, id)));
                set_many(state, &ids, false);
            }
            _ => {
                let ids = self.targets(state);
                set_many(state, &ids, true);
            }
        }
    }
}

fn ancestors(state: &AppState, folder: u64) -> Vec<u64> {
    let mut out = Vec::new();
    let mut current = state.node_store.folder_parent(folder);
    while let Some(id) = current {
        out.push(id);
        current = state.node_store.folder_parent(id);
    }
    out
}

/// Open files whose directory holds no changed file.
pub fn files_in_unchanged_folders(state: &AppState) -> Vec<u64> {
    if !state.changes.is_known() {
        return Vec::new();
    }
    let mut ids: Vec<u64> = state
        .node_store
        .file_nodes
        .values()
        .filter(|n| n.path.parent().is_some_and(|dir| !has_changes(state, dir)))
        .map(|n| n.id)
        .collect();
    ids.sort();
    ids
}

/// Every file (on disk, as the file tree lists them) of each directory that
/// holds a changed file, not open yet.
pub fn files_of_changed_folders(state: &AppState) -> Vec<PathBuf> {
    let Some(root) = state.loaded_dir.as_deref() else { return Vec::new() };
    let dirs: BTreeSet<&Path> = state
        .changes
        .paths()
        .filter(|p| p.starts_with(root))
        .filter_map(|p| p.parent())
        .collect();
    let open: HashSet<&Path> = state.node_store.file_nodes.values().map(|n| n.path.as_path()).collect();
    let mut out: Vec<PathBuf> = crate::workspace::folder_text_files(state)
        .iter()
        .filter(|p| p.parent().is_some_and(|d| dirs.contains(d)) && !open.contains(p.as_path()))
        .cloned()
        .collect();
    out.sort();
    out
}

/// The folder scope: the directory of the primary selected file.
pub fn scope_dir(state: &AppState) -> Option<PathBuf> {
    let id = state.selection.primary?;
    state.node_store.get_file(id)?.path.parent().map(Path::to_path_buf)
}

/// Text files on disk under the scope, and how many directories hold them.
pub fn scope_files(state: &AppState) -> (Vec<PathBuf>, usize) {
    let Some(dir) = scope_dir(state) else { return (Vec::new(), 0) };
    let files: Vec<PathBuf> = crate::workspace::folder_text_files(state)
        .iter()
        .filter(|p| p.starts_with(&dir))
        .cloned()
        .collect();
    let dirs: BTreeSet<&Path> = files.iter().filter_map(|p| p.parent()).collect();
    let count = dirs.len();
    (files, count)
}

/// "Select folder scope": every open file under the scope directory.
pub fn select_scope(state: &mut AppState) {
    let Some(dir) = scope_dir(state) else { return };
    let ids: Vec<u64> = state
        .node_store
        .file_nodes
        .values()
        .filter(|n| n.path.starts_with(&dir))
        .map(|n| n.id)
        .collect();
    state.selection.select_all(ids);
}

/// "Open folder scope": every file on disk under the scope directory.
pub fn open_scope(state: &mut AppState) -> Vec<u64> {
    let (files, _) = scope_files(state);
    crate::workspace::open_files_in_bulk(state, &files)
}

/// What collapsed folders hide this frame, from the Folders panel's Reveal
/// Through Collapse and Advanced options. `zoom_collapsed` is filled by the
/// canvas once it knows the folders' screen size ([`zoom_collapsed`]).
pub fn folder_view(state: &AppState) -> FolderView {
    let folders = &state.settings.folders;
    let store = &state.node_store;
    let mut revealed_files = std::collections::HashSet::new();
    if folders.reveal_selected {
        revealed_files.extend(state.selection.selected_ids());
    }
    if folders.reveal_diff_files && state.changes.is_known() {
        revealed_files.extend(
            store.file_nodes.values().filter(|n| state.changes.is_changed(&n.path)).map(|n| n.id),
        );
    }
    if folders.reveal_backwards {
        // An edge whose source sits right of its target runs backwards.
        for edge in state.edge_store.all_edges() {
            if let (Some(src), Some(tgt)) = (store.get_file(edge.source_node), store.get_file(edge.target_node))
                && src.position.x > tgt.position.x
            {
                revealed_files.insert(src.id);
                revealed_files.insert(tgt.id);
            }
        }
    }
    if state.settings.lsp.reveal_through_collapse {
        revealed_files.extend(state.selection.symbols().iter().map(|s| s.node_id));
        for path in crate::ui::lsp_panel::reference_files(state) {
            revealed_files.extend(store.file_nodes.values().filter(|n| n.path == path).map(|n| n.id));
        }
        if let Some(refs) = &state.lsp.references {
            revealed_files.insert(refs.node_id);
        }
    }
    let revealed_folders = if folders.reveal_diff_folders && state.changes.is_known() {
        store
            .folder_nodes
            .values()
            .filter(|f| has_changes(state, &f.path))
            .map(|f| f.id)
            .collect()
    } else {
        Default::default()
    };
    FolderView {
        revealed_files,
        revealed_folders,
        edges_only: folders.collapse_edges_only,
        zoom_collapsed: Default::default(),
    }
}

/// Screen width under which "Auto-Collapse on Zoom" turns a folder into a
/// solid block.
pub const AUTO_COLLAPSE_PX: f32 = 160.0;

/// "Auto-Collapse on Zoom": the outermost shown folders narrower than
/// [`AUTO_COLLAPSE_PX`] on screen at `zoom`. Their boxes are kept, so the
/// layout does not move while zooming.
pub fn zoom_collapsed(state: &AppState, zoom: f32) -> std::collections::HashSet<u64> {
    let store = &state.node_store;
    if !state.settings.folders.auto_collapse_on_zoom {
        return Default::default();
    }
    let small = |id: u64| {
        store.get_folder(id).is_some_and(|f| {
            f.bounds.is_positive() && !store.layout_collapsed(id) && f.bounds.width() * zoom < AUTO_COLLAPSE_PX
        })
    };
    let mut out = std::collections::HashSet::new();
    for &id in store.folder_nodes.keys() {
        if !small(id) || store.layout_hidden_folders().contains(&id) {
            continue;
        }
        // Outermost only: a small folder inside a small folder stays hidden.
        let mut parent = store.folder_parent(id);
        let mut inside_small = false;
        while let Some(p) = parent {
            if small(p) {
                inside_small = true;
                break;
            }
            parent = store.folder_parent(p);
        }
        if !inside_small {
            out.insert(id);
        }
    }
    out
}

/// Whether a folder holds a changed file, for Diff Folder Highlight.
pub fn folder_has_changes(state: &AppState, folder: u64) -> bool {
    state.changes.is_known()
        && state.node_store.get_folder(folder).is_some_and(|f| has_changes(state, &f.path))
}

/// Depth of the deepest directory of the loaded folder that holds a file,
/// as the "Repository depth: N levels" note says.
pub fn repository_depth(state: &AppState) -> usize {
    let Some(root) = state.loaded_dir.as_deref() else { return 0 };
    crate::workspace::folder_text_files(state)
        .iter()
        .filter_map(|p| p.parent()?.strip_prefix(root).ok().map(|rel| rel.components().count()))
        .max()
        .unwrap_or(0)
}

/// The level from which folders open collapsed: half the repository depth,
/// but never the top level or the one below it, with Auto on; else the
/// Folders setting.
pub fn collapse_depth(state: &AppState, repo_depth: usize) -> usize {
    if state.settings.folders.auto_open {
        repo_depth.div_ceil(2).max(2)
    } else {
        state.settings.folders.collapse_depth.max(1)
    }
}

/// "Open with collapsed folders by default": after several files opened at
/// once, the folders created for them at the collapse depth or deeper start
/// collapsed (their outermost such folder shows as a box). One file opened
/// on its own never hides itself.
pub fn collapse_new_folders(state: &mut AppState, new_folders: &[u64], opened_files: usize) {
    if !state.settings.folders.open_collapsed_by_default || opened_files < 2 || new_folders.is_empty() {
        return;
    }
    let Some(root) = state.loaded_dir.clone() else { return };
    let depth = collapse_depth(state, repository_depth(state));
    let targets: Vec<u64> = new_folders
        .iter()
        .copied()
        .filter(|&id| state.node_store.folder_depth(id, &root) >= depth)
        .collect();
    set_many(state, &targets, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A loaded folder with files at depths 1-3; every file is open.
    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        for rel in ["a/one.rs", "a/b/two.rs", "a/b/c/three.rs", "d/four.rs"] {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "x\n").unwrap();
        }
        let mut state = AppState { loaded_dir: Some(root.clone()), ..Default::default() };
        state.lsp.manager.set_launcher(std::sync::Arc::new(|_, _| Err("no servers in unit tests".into())));
        state.settings.folders.open_collapsed_by_default = false;
        crate::workspace::open_all_files(&mut state);
        (dir, state)
    }

    fn folder(state: &AppState, rel: &str) -> u64 {
        let path = state.loaded_dir.as_ref().unwrap().join(rel);
        state.node_store.folder_nodes.values().find(|f| f.path == path).unwrap().id
    }

    fn collapsed(state: &AppState) -> Vec<String> {
        let root = state.loaded_dir.clone().unwrap();
        let mut out: Vec<String> = state
            .node_store
            .folder_nodes
            .values()
            .filter(|f| f.collapsed)
            .map(|f| f.path.strip_prefix(&root).unwrap().display().to_string())
            .collect();
        out.sort();
        out
    }

    #[test]
    fn level_actions_walk_the_folder_tree() {
        let (_dir, mut state) = state();
        FolderAction::CollapseDeepest.run(&mut state);
        assert_eq!(collapsed(&state), vec!["a/b/c"]);
        FolderAction::CollapseDeepest.run(&mut state);
        assert_eq!(collapsed(&state), vec!["a/b", "a/b/c"]);
        assert_eq!(topmost_collapsed(&state), vec![folder(&state, "a/b")], "a/b/c is inside a/b");
        FolderAction::ExpandShallowest.run(&mut state);
        assert_eq!(collapsed(&state), vec!["a/b/c"]);
        FolderAction::CollapseLeaves.run(&mut state);
        assert_eq!(collapsed(&state), vec!["a/b", "a/b/c", "d"], "a/b and d have no expanded folder inside");
        FolderAction::ExpandTopmost.run(&mut state);
        assert_eq!(collapsed(&state), vec!["a/b/c"]);
        FolderAction::CollapseAll.run(&mut state);
        assert_eq!(collapsed(&state), vec!["a", "a/b", "a/b/c", "d"]);
        assert!(!FolderAction::CollapseAll.applies(&state));
        FolderAction::ExpandAll.run(&mut state);
        assert!(collapsed(&state).is_empty());
        assert!(state.layout_dirty);
    }

    #[test]
    fn change_actions_use_the_git_change_set() {
        let (_dir, mut state) = state();
        let root = state.loaded_dir.clone().unwrap();
        assert!(!FolderAction::CollapseUnchanged.applies(&state), "unknown changes do nothing");
        state.changes.set_for_test([root.join("a/b/two.rs")]);
        FolderAction::CollapseUnchanged.run(&mut state);
        assert_eq!(collapsed(&state), vec!["a/b/c", "d"]);
        FolderAction::CollapseAll.run(&mut state);
        FolderAction::ExpandChanged.run(&mut state);
        assert_eq!(collapsed(&state), vec!["a/b/c", "d"], "a and a/b hold the change");

        FolderAction::CloseUnchanged.run(&mut state);
        let mut open: Vec<String> = state
            .node_store
            .file_nodes
            .values()
            .map(|n| n.path.strip_prefix(&root).unwrap().display().to_string())
            .collect();
        open.sort();
        assert_eq!(open, vec!["a/b/two.rs", "a/one.rs"], "files of folders holding a change stay");
    }

    #[test]
    fn folder_scope_is_the_selected_files_directory() {
        let (_dir, mut state) = state();
        let two = state.node_store.file_nodes.values().find(|n| n.path.ends_with("a/b/two.rs")).unwrap().id;
        let others: Vec<u64> = state.node_store.all_file_ids().into_iter().filter(|&id| id != two).collect();
        crate::workspace::remove_nodes(&mut state, &others);
        state.selection.select(two);
        let (files, dirs) = scope_files(&state);
        assert_eq!((files.len(), dirs), (2, 2), "two.rs and c/three.rs, in a/b and a/b/c");
        open_scope(&mut state);
        select_scope(&mut state);
        assert_eq!(state.selection.count(), 2);
    }

    #[test]
    fn bulk_opens_collapse_new_folders_from_the_collapse_depth() {
        let (_dir, mut state) = state();
        crate::workspace::clear_canvas(&mut state);
        assert_eq!(repository_depth(&state), 3);
        assert_eq!(collapse_depth(&state, 3), 2, "Auto: half the depth, at least 2");
        state.settings.folders.open_collapsed_by_default = true;
        crate::workspace::open_all_files(&mut state);
        assert_eq!(collapsed(&state), vec!["a/b", "a/b/c"]);

        crate::workspace::clear_canvas(&mut state);
        state.settings.folders.auto_open = false;
        state.settings.folders.collapse_depth = 3;
        crate::workspace::open_all_files(&mut state);
        assert_eq!(collapsed(&state), vec!["a/b/c"]);

        crate::workspace::clear_canvas(&mut state);
        let root = state.loaded_dir.clone().unwrap();
        crate::workspace::open_files(&mut state, &[root.join("a/b/c/three.rs"), root.join("a/b/two.rs")]);
        assert!(collapsed(&state).is_empty(), "explicit opens never hide their files");
    }

    #[test]
    fn folder_view_reveals_by_the_folders_settings() {
        let (_dir, mut state) = state();
        let root = state.loaded_dir.clone().unwrap();
        let file = |state: &AppState, rel: &str| {
            state.node_store.file_nodes.values().find(|n| n.path == root.join(rel)).unwrap().id
        };
        let one = file(&state, "a/one.rs");
        state.selection.select(one);
        state.changes.set_for_test([root.join("d/four.rs")]);
        assert!(folder_view(&state).revealed_files.is_empty(), "all reveal options off by default");
        state.settings.folders.reveal_selected = true;
        state.settings.folders.reveal_diff_files = true;
        state.settings.folders.reveal_diff_folders = true;
        state.settings.folders.collapse_edges_only = true;
        let view = folder_view(&state);
        assert_eq!(view.revealed_files, HashSet::from([one, file(&state, "d/four.rs")]));
        assert_eq!(view.revealed_folders, HashSet::from([folder(&state, "d")]));
        assert!(view.edges_only);
    }

    #[test]
    fn zoom_collapse_takes_the_outermost_small_folders() {
        let (_dir, mut state) = state();
        let (w, f) = (state.settings.node_width, state.settings.font_size);
        crate::workspace::apply_layout_keeping_selection(&mut state);
        state.node_store.update_folder_bounds(w, f, state.layout_settings.folder_padding);
        assert!(zoom_collapsed(&state, 0.001).is_empty(), "off by default");
        state.settings.folders.auto_collapse_on_zoom = true;
        let tiny = zoom_collapsed(&state, 0.001);
        let mut expected = HashSet::new();
        for f in state.node_store.folder_nodes.values() {
            if state.node_store.folder_parent(f.id).is_none() {
                expected.insert(f.id);
            }
        }
        assert_eq!(tiny, expected, "only the outermost folders turn into blocks");
        assert!(zoom_collapsed(&state, 10.0).is_empty(), "nothing is small when zoomed in");
    }
}
