//! File-name search over the folder's files.

use std::path::{Path, PathBuf};

use super::fuzzy::{rank, FuzzyMatch};

/// Most rows a result list shows.
pub const MAX_RESULTS: usize = 200;

#[derive(Debug, Clone)]
pub struct FileHit {
    pub path: PathBuf,
    /// Path relative to the loaded folder, as displayed and matched.
    pub relative: String,
    pub matched: FuzzyMatch,
}

/// Files whose folder-relative path matches `query`, best first.
pub fn search_files(root: &Path, files: &[PathBuf], query: &str) -> Vec<FileHit> {
    let items = files.iter().map(|path| {
        let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned();
        (path.clone(), relative)
    });
    rank(query, items, |(_, relative)| relative.clone())
        .into_iter()
        .take(MAX_RESULTS)
        .map(|((path, relative), matched)| FileHit { path, relative, matched })
        .collect()
}

/// A file or folder matching a Files & folders search.
#[derive(Debug, Clone)]
pub struct PathHit {
    pub path: PathBuf,
    /// Path relative to the loaded folder.
    pub relative: String,
    pub is_dir: bool,
    pub matched: FuzzyMatch,
}

/// Files and the folders that hold them whose folder-relative path matches
/// `query`, best first (a folder wins a tie with its own files).
pub fn search_paths(root: &Path, files: &[PathBuf], query: &str) -> Vec<PathHit> {
    let mut folders: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
    for file in files {
        let mut dir = file.parent();
        while let Some(d) = dir.filter(|d| d.starts_with(root) && *d != root) {
            folders.insert(d.to_path_buf());
            dir = d.parent();
        }
    }
    let items = files.iter().map(|p| (p.clone(), false)).chain(folders.into_iter().map(|p| (p, true))).map(|(path, is_dir)| {
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
        (path, relative, is_dir)
    });
    rank(query, items, |(_, relative, _)| relative.clone())
        .into_iter()
        .take(MAX_RESULTS)
        .map(|((path, relative, is_dir), matched)| PathHit { path, relative, is_dir, matched })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_search_finds_folders_as_well_as_files() {
        let root = Path::new("/work/demo");
        let files = vec![root.join("src/main.rs"), root.join("src/util/strings.rs"), root.join("web/app.ts")];
        let hits = search_paths(root, &files, "util");
        let found: Vec<(&str, bool)> = hits.iter().map(|h| (h.relative.as_str(), h.is_dir)).collect();
        assert!(found.contains(&("src/util", true)) && found.contains(&("src/util/strings.rs", false)), "{found:?}");
        assert_eq!(found[0], ("src/util", true), "the folder ranks first");
        assert!(search_paths(root, &files, "zzz").is_empty());
    }

    #[test]
    fn matches_the_relative_path_not_the_absolute_one() {
        let root = Path::new("/work/demo");
        let files = vec![root.join("src/main.rs"), root.join("web/app.ts")];
        let hits = search_files(root, &files, "app");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].relative, "web/app.ts");
        // "demo" is in the absolute path only.
        assert!(search_files(root, &files, "demo").is_empty());
    }
}
