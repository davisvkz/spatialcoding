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

#[cfg(test)]
mod tests {
    use super::*;

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
