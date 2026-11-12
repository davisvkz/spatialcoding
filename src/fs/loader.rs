use std::fs;
use std::io;
use std::path::Path;

use super::tree::TreeNode;

const ALWAYS_SKIP: &[&str] = &["target", "node_modules", ".git", "__pycache__", ".DS_Store"];

pub fn load_directory(root: &Path, show_dot_folders: bool) -> io::Result<TreeNode> {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());

    let mut children = Vec::new();

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().into_owned();

        if ALWAYS_SKIP.contains(&file_name.as_str()) {
            continue;
        }

        if !show_dot_folders && file_name.starts_with('.') {
            continue;
        }

        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            match load_directory(&path, show_dot_folders) {
                Ok(child) => children.push(child),
                Err(_) => continue,
            }
        } else if file_type.is_file() {
            children.push(TreeNode::File {
                name: file_name,
                path,
            });
        }
    }

    let file_count = children.iter().map(|c| c.file_count()).sum();

    let mut node = TreeNode::Dir {
        name,
        path: root.to_path_buf(),
        children,
        expanded: false,
        file_count,
    };
    node.sort_children();

    Ok(node)
}

pub fn read_file_content(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;

    let check_len = bytes.len().min(8192);
    if bytes[..check_len].contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file appears to be binary",
        ));
    }

    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
