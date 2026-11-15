//! Read-only Git object access. Call from a worker, never a UI frame.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const MAX_FILES: usize = 512;
pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;

/// Represents a git repository connection.
pub struct GitRepo {
    root: PathBuf,
}

impl GitRepo {
    /// Discover the enclosing worktree without touching its index.
    pub fn open(path: &Path) -> Option<Self> {
        let root = command(path, &["rev-parse", "--show-toplevel"]).ok()?;
        let root = String::from_utf8(root).ok()?;
        Some(Self {
            root: PathBuf::from(root.strip_suffix('\n').unwrap_or(&root)),
        })
    }

    /// Resolve the revision once to an immutable commit, then read only blobs
    /// at the supplied paths. Missing paths are absent, not empty files.
    pub fn baseline(
        &self,
        revision: &str,
        paths: &[PathBuf],
    ) -> Result<(String, BTreeMap<PathBuf, String>), String> {
        if paths.len() > MAX_FILES {
            return Err(format!("Comparison limit: {MAX_FILES} open files"));
        }
        let commit = command(
            &self.root,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{revision}^{{commit}}"),
            ],
        )?;
        let commit = String::from_utf8(commit)
            .map_err(|e| e.to_string())?
            .trim()
            .to_string();
        let mut requested = BTreeMap::new();
        for path in paths {
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| "Open file is outside the repository")?;
            if relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Err("Non-normal repository path".into());
            }
            let relative = relative.to_str().ok_or("Non-UTF-8 repository path")?;
            requested.insert(relative.to_string(), path.clone());
        }
        if requested.is_empty() {
            return Ok((commit, BTreeMap::new()));
        }
        let mut args = vec!["ls-tree", "-rlz", "--full-tree", commit.as_str(), "--"];
        args.extend(requested.keys().map(String::as_str));
        let listing = command(&self.root, &args)?;
        let mut files = BTreeMap::new();
        let mut total = 0;
        for entry in listing.split(|b| *b == 0).filter(|s| !s.is_empty()) {
            let entry = std::str::from_utf8(entry).map_err(|_| "Non-UTF-8 tree entry")?;
            let (metadata, name) = entry.split_once('\t').ok_or("Invalid Git tree entry")?;
            let Some(path) = requested.get(name) else {
                continue;
            };
            let fields: Vec<_> = metadata.split_whitespace().collect();
            if fields.len() != 4 || !matches!(fields[0], "100644" | "100755") || fields[1] != "blob"
            {
                return Err(format!(
                    "Unsupported baseline entry: {name} (symlink/submodule)"
                ));
            }
            let size: usize = fields[3].parse().map_err(|_| "Invalid Git blob size")?;
            total += size;
            if size > MAX_FILE_BYTES || total > MAX_TOTAL_BYTES {
                return Err(
                    "Comparison exceeds text size limits (2 MiB/file, 32 MiB total)".into(),
                );
            }
            let blob = command(&self.root, &["cat-file", "blob", fields[2]])?;
            if blob.len() != size || blob.contains(&0) {
                return Err(format!("Unsupported binary baseline file: {name}"));
            }
            let text =
                String::from_utf8(blob).map_err(|_| format!("Non-UTF-8 baseline file: {name}"))?;
            files.insert(path.clone(), text);
        }
        Ok((commit, files))
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

fn command(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("--literal-pathspecs")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Cannot run Git: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr)
            .lines()
            .next()
            .unwrap_or("Git command failed")
            .to_string());
    }
    Ok(output.stdout)
}
