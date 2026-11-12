//! File system watcher for hot-reload of changed files.
//! Currently a stub — will be implemented with the `notify` crate.

use std::path::Path;

/// Watches a directory tree for changes and notifies the app.
pub struct FileWatcher {
    _root: std::path::PathBuf,
}

impl FileWatcher {
    /// Create a new watcher for the given root directory.
    /// Currently returns a no-op watcher.
    pub fn new(root: &Path) -> Self {
        Self {
            _root: root.to_path_buf(),
        }
    }

    /// Poll for changed file paths since last call.
    /// Stub: always returns empty vec.
    pub fn poll_changes(&mut self) -> Vec<std::path::PathBuf> {
        Vec::new()
    }

    /// Stop watching.
    pub fn stop(&mut self) {}
}
