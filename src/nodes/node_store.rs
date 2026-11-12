use std::collections::HashMap;
use std::path::PathBuf;

use super::file_node::FileNode;
use super::folder_node::FolderNode;
use super::lod::LodLevel;

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
        lod: LodLevel,
    ) -> Option<u64> {
        for node in self.file_nodes.values() {
            let rect = node.world_rect(node_width, font_size, lod);
            if rect.contains(world_pos) {
                return Some(node.id);
            }
        }
        None
    }
}
