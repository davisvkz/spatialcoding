//! Git repository integration.
//! Currently a stub — will use git2 crate.

use std::path::{Path, PathBuf};

/// Represents a git repository connection.
pub struct GitRepo {
    _root: PathBuf,
}

impl GitRepo {
    /// Open a git repository at the given path.
    /// Stub: always returns None.
    pub fn open(_path: &Path) -> Option<Self> {
        // TODO: Use git2::Repository::open()
        None
    }

    /// Get the current branch name.
    pub fn current_branch(&self) -> Option<String> {
        None
    }

    /// Get list of changed files (working tree vs HEAD).
    pub fn changed_files(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Get unified diff for a file.
    pub fn file_diff(&self, _path: &Path) -> Option<String> {
        None
    }
}
