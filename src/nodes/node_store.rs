use std::collections::HashMap;
use std::path::PathBuf;

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
