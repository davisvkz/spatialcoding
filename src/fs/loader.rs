use std::fs;
use std::io;
use std::path::Path;

use super::tree::TreeNode;

const ALWAYS_SKIP: &[&str] = &["target", "node_modules", ".git", "__pycache__", ".DS_Store"];

/// Whether the file tree leaves out an entry with this name: build output,
/// VCS data and, unless `show_dot_folders`, dot files and folders.
pub fn is_skipped_name(name: &str, show_dot_folders: bool) -> bool {
    ALWAYS_SKIP.contains(&name) || (!show_dot_folders && name.starts_with('.'))
}

/// Whether `path` is, or lies inside, something the file tree leaves out.
/// Only the components below `root` count.
pub fn is_skipped_path(path: &Path, root: &Path, show_dot_folders: bool) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    relative.components().any(|c| is_skipped_name(&c.as_os_str().to_string_lossy(), show_dot_folders))
}

pub fn load_directory(root: &Path, show_dot_folders: bool) -> io::Result<TreeNode> {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());

    let mut children = Vec::new();

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().into_owned();

        if is_skipped_name(&file_name, show_dot_folders) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn child_names(node: &TreeNode) -> Vec<&str> {
        match node {
            TreeNode::Dir { children, .. } => children.iter().map(TreeNode::name).collect(),
            TreeNode::File { .. } => vec![],
        }
    }

    fn sample_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for d in ["src", "target", "node_modules", ".git", ".github", "__pycache__"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in [
            "src/main.rs",
            "src/lib.rs",
            "target/out.bin",
            "node_modules/pkg.js",
            ".git/HEAD",
            ".github/ci.yml",
            ".env",
            "README.md",
        ] {
            fs::write(root.join(f), "").unwrap();
        }
        dir
    }

    #[test]
    fn skips_build_dirs_and_hidden_entries_by_default() {
        let dir = sample_project();
        let tree = load_directory(dir.path(), false).unwrap();

        assert!(tree.is_dir());
        assert_eq!(child_names(&tree), ["src", "README.md"]);
        assert_eq!(tree.file_count(), 3);
        let src = tree.find_node(&dir.path().join("src")).unwrap();
        assert_eq!(child_names(src), ["lib.rs", "main.rs"]);
        assert_eq!(src.file_count(), 2);
    }

    #[test]
    fn show_dot_folders_includes_hidden_entries_but_never_git() {
        let dir = sample_project();
        let tree = load_directory(dir.path(), true).unwrap();

        assert_eq!(child_names(&tree), [".github", "src", ".env", "README.md"]);
        assert_eq!(tree.file_count(), 5);
    }

    #[test]
    fn load_directory_fails_for_missing_root() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_directory(&dir.path().join("nope"), false).is_err());
    }

    #[test]
    fn read_file_content_accepts_utf8_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "olá\nmundo\n").unwrap();
        assert_eq!(read_file_content(&path).unwrap(), "olá\nmundo\n");

        let empty = dir.path().join("empty.txt");
        fs::write(&empty, "").unwrap();
        assert_eq!(read_file_content(&empty).unwrap(), "");
    }

    #[test]
    fn read_file_content_rejects_binary_and_invalid_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("a.bin");
        fs::write(&binary, b"abc\0def").unwrap();
        assert_eq!(
            read_file_content(&binary).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );

        let latin1 = dir.path().join("b.txt");
        fs::write(&latin1, [b'o', b'l', 0xE1]).unwrap();
        assert_eq!(
            read_file_content(&latin1).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );

        assert_eq!(
            read_file_content(&dir.path().join("missing")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
