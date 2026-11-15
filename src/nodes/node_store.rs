use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::file_node::FileNode;
use super::folder_node::FolderNode;

pub struct NodeStore {
    pub file_nodes: HashMap<u64, FileNode>,
    pub folder_nodes: HashMap<u64, FolderNode>,
    next_id: u64,
}

impl Default for NodeStore {
    fn default() -> Self {
        Self {
            file_nodes: HashMap::new(),
            folder_nodes: HashMap::new(),
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
    /// band. Must agree with the space the hierarchical layout reserves.
    pub fn update_folder_bounds(&mut self, node_width: f32, font_size: f32, padding: f32) {
        let mut order: Vec<(usize, u64)> = self
            .folder_nodes
            .values()
            .map(|f| (f.path.components().count(), f.id))
            .collect();
        order.sort_by(|a, b| b.cmp(a));
        let parents: HashMap<u64, Option<u64>> =
            order.iter().map(|&(_, id)| (id, self.folder_parent(id))).collect();
        let mut file_rects: HashMap<u64, Vec<egui::Rect>> = HashMap::new();
        let mut direct_files: HashMap<u64, usize> = HashMap::new();
        for id in self.file_nodes.keys().copied().collect::<Vec<_>>() {
            if let Some(folder) = self.file_folder(id) {
                let rect = self.file_nodes[&id].world_rect(node_width, font_size);
                file_rects.entry(folder).or_default().push(rect);
                *direct_files.entry(folder).or_default() += 1;
            }
        }
        for (_, id) in order {
            let rects = file_rects.remove(&id).unwrap_or_default();
            let Some(folder) = self.folder_nodes.get_mut(&id) else { continue };
            folder.file_count = direct_files.get(&id).copied().unwrap_or(0);
            folder.bounds = egui::Rect::NOTHING;
            folder.update_bounds(&rects, padding);
            let bounds = folder.bounds;
            if bounds.is_positive()
                && let Some(Some(parent)) = parents.get(&id)
            {
                file_rects.entry(*parent).or_default().push(bounds);
            }
        }
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
            if rect.contains(world_pos) {
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
