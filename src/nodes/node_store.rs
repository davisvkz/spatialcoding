use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::file_node::FileNode;
use super::folder_node::FolderNode;

pub struct NodeStore {
    pub file_nodes: HashMap<u64, FileNode>,
    pub folder_nodes: HashMap<u64, FolderNode>,
    /// How collapsed folders show this frame (Folders panel options).
    pub view: FolderView,
    next_id: u64,
}

/// Per-frame options that change what a collapsed folder hides, set by the
/// app from the Folders panel (Reveal Through Collapse, Advanced).
#[derive(Debug, Clone, Default)]
pub struct FolderView {
    /// Files shown even inside a collapsed folder (selected, changed, or on
    /// a backwards edge), stacked under its card.
    pub revealed_files: HashSet<u64>,
    /// Folders that never collapse (Reveal › Diff Folders).
    pub revealed_folders: HashSet<u64>,
    /// "Collapse: Edges Only": collapsed folders keep showing their files;
    /// only their edges gather on the folder's frame.
    pub edges_only: bool,
    /// Folders drawn as solid blocks because they are too small on screen
    /// ("Auto-Collapse on Zoom"); layout and boxes ignore this.
    pub zoom_collapsed: HashSet<u64>,
}

impl Default for NodeStore {
    fn default() -> Self {
        Self {
            file_nodes: HashMap::new(),
            folder_nodes: HashMap::new(),
            view: FolderView::default(),
            next_id: 1,
        }
    }
}

impl NodeStore {
    pub fn add_file(&mut self, path: PathBuf, content: String, position: egui::Pos2) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let node = FileNode::new(id, path, content, position);
        self.file_nodes.insert(id, node);
        id
    }

    pub fn add_folder(&mut self, path: PathBuf, file_count: usize) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let node = FolderNode::new(id, path, file_count);
        self.folder_nodes.insert(id, node);
        id
    }

    pub fn remove_node(&mut self, id: u64) {
        if self.file_nodes.remove(&id).is_none() {
            self.folder_nodes.remove(&id);
        }
    }

    pub fn get_file(&self, id: u64) -> Option<&FileNode> {
        self.file_nodes.get(&id)
    }

    pub fn get_file_mut(&mut self, id: u64) -> Option<&mut FileNode> {
        self.file_nodes.get_mut(&id)
    }

    pub fn get_folder(&self, id: u64) -> Option<&FolderNode> {
        self.folder_nodes.get(&id)
    }

    pub fn get_folder_mut(&mut self, id: u64) -> Option<&mut FolderNode> {
        self.folder_nodes.get_mut(&id)
    }

    pub fn all_file_ids(&self) -> Vec<u64> {
        self.file_nodes.keys().copied().collect()
    }

    pub fn all_folder_ids(&self) -> Vec<u64> {
        self.folder_nodes.keys().copied().collect()
    }

    /// Deepest folder whose path contains `dir` (`dir` itself included).
    pub fn folder_containing(&self, dir: &Path) -> Option<u64> {
        self.folder_nodes
            .values()
            .filter(|f| dir.starts_with(&f.path))
            .max_by_key(|f| (f.path.components().count(), std::cmp::Reverse(f.id)))
            .map(|f| f.id)
    }

    /// Folder a file node is drawn inside: the deepest folder holding its
    /// parent directory.
    pub fn file_folder(&self, file_id: u64) -> Option<u64> {
        let parent = self.file_nodes.get(&file_id)?.path.parent()?;
        self.folder_containing(parent)
    }

    /// Folder each folder is nested in: the deepest strict ancestor folder.
    pub fn folder_parent(&self, folder_id: u64) -> Option<u64> {
        let path = &self.folder_nodes.get(&folder_id)?.path;
        self.folder_nodes
            .values()
            .filter(|f| f.id != folder_id && f.path != *path && path.starts_with(&f.path))
            .max_by_key(|f| (f.path.components().count(), std::cmp::Reverse(f.id)))
            .map(|f| f.id)
    }

    /// Recomputes every folder's box so nested folders sit inside their
    /// parent: deepest folders first, each one wrapping its own files and the
    /// boxes of the folders nested directly in it, plus `padding` and a label
    /// band. A collapsed folder is its card plus the revealed files under
    /// it; what it hides adds nothing. Must agree with the space the
    /// hierarchical layout reserves.
    pub fn update_folder_bounds(&mut self, node_width: f32, font_size: f32, padding: f32) {
        let mut order: Vec<(usize, u64)> = self
            .folder_nodes
            .values()
            .map(|f| (f.path.components().count(), f.id))
            .collect();
        order.sort_by(|a, b| b.cmp(a));
        let parents: HashMap<u64, Option<u64>> =
            order.iter().map(|&(_, id)| (id, self.folder_parent(id))).collect();
        let hidden_folders = self.layout_hidden_folders();
        let mut child_rects: HashMap<u64, Vec<egui::Rect>> = HashMap::new();
        let mut revealed_rects: HashMap<u64, Vec<egui::Rect>> = HashMap::new();
        let mut direct_files: HashMap<u64, usize> = HashMap::new();
        for id in self.file_nodes.keys().copied().collect::<Vec<_>>() {
            let Some(folder) = self.file_folder(id) else { continue };
            *direct_files.entry(folder).or_default() += 1;
            let rect = self.file_nodes[&id].world_rect(node_width, font_size);
            match self.layout_collapsed_ancestor(Some(folder), true) {
                None => child_rects.entry(folder).or_default().push(rect),
                Some(card) if self.view.revealed_files.contains(&id) => {
                    revealed_rects.entry(card).or_default().push(rect)
                }
                Some(_) => {}
            }
        }
        for (_, id) in order {
            let rects = child_rects.remove(&id).unwrap_or_default();
            let hidden = hidden_folders.contains(&id);
            let collapsed = self.layout_collapsed(id);
            let revealed = revealed_rects.remove(&id).unwrap_or_default();
            let Some(folder) = self.folder_nodes.get_mut(&id) else { continue };
            folder.file_count = direct_files.get(&id).copied().unwrap_or(0);
            folder.bounds = egui::Rect::NOTHING;
            if hidden {
                continue;
            }
            match folder.collapsed_rect().filter(|_| collapsed) {
                Some(card) => {
                    folder.bounds = revealed.iter().fold(card, |acc, r| acc.union(*r));
                }
                None => folder.update_bounds(&rects, padding),
            }
            let bounds = folder.bounds;
            if bounds.is_positive()
                && let Some(Some(parent)) = parents.get(&id)
            {
                child_rects.entry(*parent).or_default().push(bounds);
            }
        }
    }

    /// Whether a folder lays out and draws as its card: collapsed, not kept
    /// open by Reveal › Diff Folders, and not in "Collapse: Edges Only".
    pub fn layout_collapsed(&self, folder_id: u64) -> bool {
        !self.view.edges_only
            && !self.view.revealed_folders.contains(&folder_id)
            && self.folder_nodes.get(&folder_id).is_some_and(|f| f.collapsed)
    }

    /// Collapsed as a card, or as a block because it is too small on screen.
    pub fn view_collapsed(&self, folder_id: u64) -> bool {
        self.layout_collapsed(folder_id) || self.view.zoom_collapsed.contains(&folder_id)
    }

    /// Collapsed for edges: as `view_collapsed`, plus "Edges Only" folders.
    fn edge_collapsed(&self, folder_id: u64) -> bool {
        self.view_collapsed(folder_id)
            || (self.view.edges_only
                && !self.view.revealed_folders.contains(&folder_id)
                && self.folder_nodes.get(&folder_id).is_some_and(|f| f.collapsed))
    }

    /// Outermost folder from `start` up (`start` itself when `include_start`)
    /// for which `pred` holds.
    fn outermost(&self, start: Option<u64>, include_start: bool, pred: impl Fn(u64) -> bool) -> Option<u64> {
        let mut found = None;
        let mut current = if include_start { start } else { start.and_then(|s| self.folder_parent(s)) };
        while let Some(id) = current {
            if pred(id) {
                found = Some(id);
            }
            current = self.folder_parent(id);
        }
        found
    }

    fn layout_collapsed_ancestor(&self, start: Option<u64>, include_start: bool) -> Option<u64> {
        self.outermost(start, include_start, |id| self.layout_collapsed(id))
    }

    /// Folders inside a card: the layout skips them.
    pub fn layout_hidden_folders(&self) -> HashSet<u64> {
        self.folder_nodes
            .keys()
            .copied()
            .filter(|&id| self.layout_collapsed_ancestor(Some(id), false).is_some())
            .collect()
    }

    /// Folders not drawn this frame: inside a card or a zoom-collapsed block.
    pub fn hidden_folders(&self) -> HashSet<u64> {
        self.folder_nodes
            .keys()
            .copied()
            .filter(|&id| self.outermost(Some(id), false, |a| self.view_collapsed(a)).is_some())
            .collect()
    }

    /// The card a file is laid out under, when it is inside one.
    pub fn layout_card(&self, file_id: u64) -> Option<u64> {
        self.layout_collapsed_ancestor(self.file_folder(file_id), true)
    }

    /// The collapsed folder that hides a file node: its outermost collapsed
    /// (or zoom-collapsed) ancestor, or `None` when the file is shown,
    /// revealed files included.
    pub fn hiding_folder(&self, file_id: u64) -> Option<u64> {
        if self.view.revealed_files.contains(&file_id) {
            return None;
        }
        self.outermost(self.file_folder(file_id), true, |id| self.view_collapsed(id))
    }

    pub fn is_file_hidden(&self, file_id: u64) -> bool {
        self.hiding_folder(file_id).is_some()
    }

    /// For each file whose edges gather on a collapsed folder, that folder
    /// and the box its edges end on: the card, the zoom block, or with
    /// "Collapse: Edges Only" the folder's frame. Revealed files keep their
    /// own edge ends.
    pub fn collapse_proxies(&self) -> HashMap<u64, (u64, egui::Rect)> {
        if !self.folder_nodes.values().any(|f| f.collapsed) && self.view.zoom_collapsed.is_empty() {
            return HashMap::new();
        }
        self.file_nodes
            .keys()
            .filter(|id| !self.view.revealed_files.contains(id))
            .filter_map(|&id| {
                let folder = self.outermost(self.file_folder(id), true, |f| self.edge_collapsed(f))?;
                let node = self.folder_nodes.get(&folder)?;
                let rect = match node.collapsed_rect().filter(|_| self.layout_collapsed(folder)) {
                    Some(card) => card,
                    None => node.bounds,
                };
                rect.is_positive().then_some((id, (folder, rect)))
            })
            .collect()
    }

    /// Collapses or expands a folder. Collapsing keeps the box where the
    /// folder began, so the view does not jump.
    pub fn set_folder_collapsed(&mut self, folder_id: u64, collapsed: bool, node_width: f32, font_size: f32) {
        let origin = if collapsed {
            let bounds = self.folder_nodes.get(&folder_id).map(|f| f.bounds);
            match bounds {
                Some(b) if b.is_positive() => Some(b.min),
                _ => {
                    let path = self.folder_nodes.get(&folder_id).map(|f| f.path.clone());
                    path.and_then(|path| {
                        self.file_nodes
                            .values()
                            .filter(|n| n.path.starts_with(&path))
                            .map(|n| n.world_rect(node_width, font_size).min)
                            .reduce(|a, b| a.min(b))
                    })
                }
            }
        } else {
            None
        };
        if let Some(folder) = self.folder_nodes.get_mut(&folder_id) {
            if collapsed && !folder.collapsed {
                folder.collapsed_origin = origin.or(folder.collapsed_origin);
            }
            folder.collapsed = collapsed;
        }
    }

    /// Deepest shown folder under `world_pos`: a collapsed box anywhere, or
    /// an expanded folder's label band.
    pub fn folder_at_position(&self, world_pos: egui::Pos2) -> Option<u64> {
        let hidden = self.hidden_folders();
        self.folder_nodes
            .values()
            .filter(|f| !hidden.contains(&f.id) && f.bounds.is_positive())
            .filter(|f| {
                if self.view_collapsed(f.id) {
                    f.collapsed_rect()
                        .filter(|_| self.layout_collapsed(f.id))
                        .unwrap_or(f.bounds)
                        .contains(world_pos)
                } else {
                    let band = egui::Rect::from_min_max(
                        f.bounds.min,
                        egui::pos2(f.bounds.max.x, f.bounds.min.y + super::folder_node::FOLDER_HEADER_HEIGHT),
                    );
                    band.contains(world_pos)
                }
            })
            .max_by_key(|f| f.path.components().count())
            .map(|f| f.id)
    }

    /// Folder depth below `root` (a folder directly in it has depth 1).
    pub fn folder_depth(&self, folder_id: u64, root: &Path) -> usize {
        self.folder_nodes
            .get(&folder_id)
            .and_then(|f| f.path.strip_prefix(root).ok())
            .map_or(0, |rel| rel.components().count())
    }

    pub fn clear(&mut self) {
        self.file_nodes.clear();
        self.folder_nodes.clear();
        self.next_id = 1;
    }

    pub fn node_at_position(
        &self,
        world_pos: egui::Pos2,
        node_width: f32,
        font_size: f32,
    ) -> Option<u64> {
        for node in self.file_nodes.values() {
            let rect = node.world_rect(node_width, font_size);
            if rect.contains(world_pos) && !self.is_file_hidden(node.id) {
                return Some(node.id);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_file(store: &mut NodeStore, name: &str, at: egui::Pos2) -> u64 {
        store.add_file(PathBuf::from(name), "line\n".into(), at)
    }

    #[test]
    fn folders_nest_by_path_and_files_land_in_the_deepest_folder() {
        let mut store = NodeStore::default();
        let src = store.add_folder(PathBuf::from("r/src"), 0);
        let utils = store.add_folder(PathBuf::from("r/src/utils"), 0);
        let srcx = store.add_folder(PathBuf::from("r/srcx"), 0);
        let f = add_file(&mut store, "r/src/utils/kind.rs", egui::Pos2::ZERO);
        let g = add_file(&mut store, "r/src/lib.rs", egui::Pos2::ZERO);

        assert_eq!(store.folder_parent(utils), Some(src));
        assert_eq!(store.folder_parent(src), None);
        assert_eq!(store.folder_parent(srcx), None, "a shared name prefix is not nesting");
        assert_eq!(store.file_folder(f), Some(utils));
        assert_eq!(store.file_folder(g), Some(src));
    }

    #[test]
    fn folder_bounds_wrap_nested_folder_boxes() {
        let mut store = NodeStore::default();
        let src = store.add_folder(PathBuf::from("r/src"), 0);
        let utils = store.add_folder(PathBuf::from("r/src/utils"), 0);
        let empty = store.add_folder(PathBuf::from("r/empty"), 0);
        let f = add_file(&mut store, "r/src/utils/kind.rs", egui::pos2(0.0, 0.0));
        add_file(&mut store, "r/src/lib.rs", egui::pos2(-500.0, 0.0));
        store.update_folder_bounds(100.0, 10.0, 5.0);

        let inner = store.get_folder(utils).unwrap().bounds;
        let outer = store.get_folder(src).unwrap().bounds;
        assert!(inner.contains_rect(store.get_file(f).unwrap().world_rect(100.0, 10.0)));
        assert!(outer.contains_rect(inner));
        assert!(outer.min.x <= -505.0, "outer also wraps its own file");
        assert_eq!(store.get_folder(src).unwrap().file_count, 1, "direct files only");
        assert_eq!(store.get_folder(empty).unwrap().bounds, egui::Rect::NOTHING);
    }

    #[test]
    fn files_and_folders_share_one_id_sequence() {
        let mut store = NodeStore::default();
        let a = add_file(&mut store, "a.rs", egui::Pos2::ZERO);
        let f = store.add_folder(PathBuf::from("src"), 1);
        let b = add_file(&mut store, "b.rs", egui::Pos2::ZERO);

        assert_eq!((a, f, b), (1, 2, 3));
        assert_eq!(store.get_file(a).unwrap().id, a);
        assert_eq!(store.get_folder(f).unwrap().id, f);
        assert!(store.get_file(f).is_none());
        let mut files = store.all_file_ids();
        files.sort();
        assert_eq!(files, [a, b]);
        assert_eq!(store.all_folder_ids(), [f]);
    }

    #[test]
    fn remove_node_handles_both_kinds_and_unknown_ids() {
        let mut store = NodeStore::default();
        let a = add_file(&mut store, "a.rs", egui::Pos2::ZERO);
        let f = store.add_folder(PathBuf::from("src"), 1);

        store.remove_node(a);
        store.remove_node(f);
        store.remove_node(999);

        assert!(store.all_file_ids().is_empty());
        assert!(store.all_folder_ids().is_empty());
    }

    #[test]
    fn clear_resets_ids() {
        let mut store = NodeStore::default();
        add_file(&mut store, "a.rs", egui::Pos2::ZERO);
        store.add_folder(PathBuf::from("src"), 1);
        store.clear();

        assert!(store.all_file_ids().is_empty());
        assert_eq!(add_file(&mut store, "b.rs", egui::Pos2::ZERO), 1);
    }

    #[test]
    fn get_file_mut_edits_in_place() {
        let mut store = NodeStore::default();
        let a = add_file(&mut store, "a.rs", egui::Pos2::ZERO);
        store.get_file_mut(a).unwrap().position = egui::pos2(3.0, 4.0);
        assert_eq!(store.get_file(a).unwrap().position, egui::pos2(3.0, 4.0));
    }

    /// /r/a/x.rs, /r/a/b/y.rs and /r/c.rs with folders /r/a and /r/a/b.
    fn nested() -> (NodeStore, [u64; 3], [u64; 2]) {
        let mut store = NodeStore::default();
        let a = store.add_folder(PathBuf::from("/r/a"), 0);
        let b = store.add_folder(PathBuf::from("/r/a/b"), 0);
        let x = add_file(&mut store, "/r/a/x.rs", egui::pos2(0.0, 0.0));
        let y = add_file(&mut store, "/r/a/b/y.rs", egui::pos2(0.0, 300.0));
        let c = add_file(&mut store, "/r/c.rs", egui::pos2(2000.0, 0.0));
        (store, [x, y, c], [a, b])
    }

    #[test]
    fn a_collapsed_folder_hides_everything_inside_behind_its_outermost_box() {
        let (mut store, [x, y, c], [a, b]) = nested();
        store.update_folder_bounds(700.0, 12.0, 40.0);
        let a_top_left = store.get_folder(a).unwrap().bounds.min;
        store.set_folder_collapsed(b, true, 700.0, 12.0);
        assert_eq!(store.hiding_folder(y), Some(b));
        assert_eq!(store.hiding_folder(x), None);
        store.set_folder_collapsed(a, true, 700.0, 12.0);
        assert_eq!(store.hiding_folder(y), Some(a), "outermost collapsed ancestor");
        assert_eq!(store.hiding_folder(x), Some(a));
        assert!(!store.is_file_hidden(c));
        assert_eq!(store.hidden_folders(), HashSet::from([b]));

        store.update_folder_bounds(700.0, 12.0, 40.0);
        let box_a = store.get_folder(a).unwrap().bounds;
        assert_eq!(box_a.min, a_top_left, "collapsing keeps the folder's top-left");
        assert_eq!(box_a.size(), crate::nodes::folder_node::COLLAPSED_FOLDER_SIZE);
        assert!(!store.get_folder(b).unwrap().bounds.is_positive(), "hidden folder has no box");
        let proxies = store.collapse_proxies();
        assert_eq!(proxies.keys().copied().collect::<HashSet<_>>(), HashSet::from([x, y]));
        assert_eq!(proxies[&x], (a, box_a));

        assert_eq!(store.node_at_position(egui::pos2(10.0, 10.0), 700.0, 12.0), None, "hidden files are not hit");
        assert_eq!(store.folder_at_position(box_a.center()), Some(a));
        store.set_folder_collapsed(a, false, 700.0, 12.0);
        store.set_folder_collapsed(b, false, 700.0, 12.0);
        assert!(store.collapse_proxies().is_empty());
    }

    #[test]
    fn revealed_files_stay_shown_and_keep_their_own_edge_ends() {
        let (mut store, [x, y, _], [a, _]) = nested();
        store.update_folder_bounds(700.0, 12.0, 40.0);
        store.set_folder_collapsed(a, true, 700.0, 12.0);
        store.view.revealed_files.insert(y);
        assert!(!store.is_file_hidden(y));
        assert!(store.is_file_hidden(x));
        assert_eq!(store.layout_card(y), Some(a));
        let proxies = store.collapse_proxies();
        assert!(proxies.contains_key(&x) && !proxies.contains_key(&y));
        store.update_folder_bounds(700.0, 12.0, 40.0);
        let bounds = store.get_folder(a).unwrap().bounds;
        assert!(bounds.contains_rect(store.get_file(y).unwrap().world_rect(700.0, 12.0)), "card wraps revealed files");
    }

    #[test]
    fn diff_revealed_folders_never_collapse_and_edges_only_keeps_files_shown() {
        let (mut store, [x, _, _], [a, _]) = nested();
        store.update_folder_bounds(700.0, 12.0, 40.0);
        store.set_folder_collapsed(a, true, 700.0, 12.0);
        store.view.revealed_folders.insert(a);
        assert!(!store.layout_collapsed(a) && !store.is_file_hidden(x));
        assert!(store.collapse_proxies().is_empty());

        store.view.revealed_folders.clear();
        store.view.edges_only = true;
        store.update_folder_bounds(700.0, 12.0, 40.0);
        assert!(!store.layout_collapsed(a), "edges only: laid out expanded");
        assert!(!store.is_file_hidden(x), "edges only: files stay");
        let frame = store.get_folder(a).unwrap().bounds;
        assert_eq!(store.collapse_proxies()[&x], (a, frame), "edges gather on the frame");
    }

    #[test]
    fn zoom_collapsed_folders_hide_their_contents_without_changing_boxes() {
        let (mut store, [x, y, c], [a, b]) = nested();
        store.update_folder_bounds(700.0, 12.0, 40.0);
        let frame = store.get_folder(a).unwrap().bounds;
        store.view.zoom_collapsed.insert(a);
        store.update_folder_bounds(700.0, 12.0, 40.0);
        assert_eq!(store.get_folder(a).unwrap().bounds, frame, "the box does not change");
        assert!(store.is_file_hidden(x) && store.is_file_hidden(y) && !store.is_file_hidden(c));
        assert_eq!(store.hidden_folders(), HashSet::from([b]));
        assert_eq!(store.collapse_proxies()[&y], (a, frame));
        assert!(!store.layout_collapsed(a));
    }

    #[test]
    fn expanded_folders_are_hit_on_their_label_band_only() {
        let (mut store, _, [a, b]) = nested();
        store.update_folder_bounds(700.0, 12.0, 40.0);
        let bounds_b = store.get_folder(b).unwrap().bounds;
        assert_eq!(store.folder_at_position(bounds_b.min + egui::vec2(5.0, 5.0)), Some(b));
        assert_eq!(store.folder_at_position(bounds_b.center()), None, "inside the body: not the folder");
        let bounds_a = store.get_folder(a).unwrap().bounds;
        assert_eq!(store.folder_at_position(bounds_a.min + egui::vec2(5.0, 5.0)), Some(a));
        assert_eq!(store.folder_depth(b, Path::new("/r")), 2);
    }

    #[test]
    fn node_at_position_hit_tests_file_rects() {
        let mut store = NodeStore::default();
        let a = add_file(&mut store, "a.rs", egui::pos2(100.0, 100.0));

        assert_eq!(store.node_at_position(egui::pos2(150.0, 120.0), 700.0, 12.0), Some(a));
        assert_eq!(store.node_at_position(egui::pos2(799.0, 101.0), 700.0, 12.0), Some(a));
        assert_eq!(store.node_at_position(egui::pos2(50.0, 50.0), 700.0, 12.0), None);
        assert_eq!(store.node_at_position(egui::pos2(150.0, 1000.0), 700.0, 12.0), None);
    }
}
