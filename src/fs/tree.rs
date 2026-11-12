use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum TreeNode {
    File {
        name: String,
        path: PathBuf,
    },
    Dir {
        name: String,
        path: PathBuf,
        children: Vec<TreeNode>,
        expanded: bool,
        file_count: usize,
    },
}

impl TreeNode {
    pub fn name(&self) -> &str {
        match self {
            TreeNode::File { name, .. } => name,
            TreeNode::Dir { name, .. } => name,
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            TreeNode::File { path, .. } => path,
            TreeNode::Dir { path, .. } => path,
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, TreeNode::Dir { .. })
    }

    pub fn toggle_expanded(&mut self) {
        if let TreeNode::Dir { expanded, .. } = self {
            *expanded = !*expanded;
        }
    }

    pub fn sort_children(&mut self) {
        if let TreeNode::Dir { children, .. } = self {
            children.sort_by(|a, b| {
                match (a.is_dir(), b.is_dir()) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.name().to_lowercase().cmp(&b.name().to_lowercase()),
                }
            });
            for child in children.iter_mut() {
                child.sort_children();
            }
        }
    }

    pub fn file_count(&self) -> usize {
        match self {
            TreeNode::File { .. } => 1,
            TreeNode::Dir { file_count, .. } => *file_count,
        }
    }

    pub fn find_node(&self, target: &Path) -> Option<&TreeNode> {
        if self.path() == target {
            return Some(self);
        }
        if let TreeNode::Dir { children, .. } = self {
            for child in children {
                if let Some(found) = child.find_node(target) {
                    return Some(found);
                }
            }
        }
        None
    }
}
