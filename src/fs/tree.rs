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

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> TreeNode {
        TreeNode::File {
            name: name.into(),
            path: PathBuf::from(format!("/r/{name}")),
        }
    }

    fn dir(name: &str, children: Vec<TreeNode>) -> TreeNode {
        let file_count = children.iter().map(TreeNode::file_count).sum();
        TreeNode::Dir {
            name: name.into(),
            path: PathBuf::from(format!("/r/{name}")),
            children,
            expanded: false,
            file_count,
        }
    }

    fn child_names(node: &TreeNode) -> Vec<&str> {
        match node {
            TreeNode::Dir { children, .. } => children.iter().map(TreeNode::name).collect(),
            TreeNode::File { .. } => vec![],
        }
    }

    #[test]
    fn sort_children_puts_dirs_first_then_names_case_insensitively() {
        let mut root = dir(
            "root",
            vec![
                file("b.rs"),
                dir("Zeta", vec![]),
                file("A.rs"),
                dir("alpha", vec![file("y"), file("X")]),
            ],
        );
        root.sort_children();

        assert_eq!(child_names(&root), ["alpha", "Zeta", "A.rs", "b.rs"]);
        let alpha = root.find_node(Path::new("/r/alpha")).unwrap();
        assert_eq!(child_names(alpha), ["X", "y"], "sorting must recurse");
    }

    #[test]
    fn toggle_expanded_only_affects_dirs() {
        let mut d = dir("d", vec![]);
        d.toggle_expanded();
        assert!(matches!(d, TreeNode::Dir { expanded: true, .. }));
        d.toggle_expanded();
        assert!(matches!(d, TreeNode::Dir { expanded: false, .. }));

        let mut f = file("f");
        f.toggle_expanded();
        assert!(!f.is_dir());
    }

    #[test]
    fn find_node_searches_recursively() {
        let root = dir("root", vec![dir("src", vec![file("main.rs")]), file("README")]);

        assert_eq!(root.find_node(Path::new("/r/root")).unwrap().name(), "root");
        assert_eq!(root.find_node(Path::new("/r/main.rs")).unwrap().name(), "main.rs");
        assert!(root.find_node(Path::new("/r/missing")).is_none());
    }

    #[test]
    fn file_count_is_one_for_files_and_stored_total_for_dirs() {
        let root = dir("root", vec![dir("src", vec![file("a"), file("b")]), file("c")]);
        assert_eq!(file("x").file_count(), 1);
        assert_eq!(root.file_count(), 3);
    }
}
