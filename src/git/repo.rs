//! Read-only Git object access. Call from a worker, never a UI frame.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const MAX_FILES: usize = 512;
pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;

/// How a file differs from `HEAD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeKind {
    Modified,
    Added,
    Deleted,
    Untracked,
}

impl ChangeKind {
    /// One-letter status as `git status --short` prints it.
    pub fn letter(self) -> char {
        match self {
            ChangeKind::Modified => 'M',
            ChangeKind::Added => 'A',
            ChangeKind::Deleted => 'D',
            ChangeKind::Untracked => 'U',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ChangeKind::Modified => "Modified",
            ChangeKind::Added => "Added",
            ChangeKind::Deleted => "Deleted",
            ChangeKind::Untracked => "Untracked",
        }
    }
}

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

    /// The repository's own directory and the one holding the refs shared by
    /// every worktree, both absolute. They are the same for an ordinary
    /// checkout; a linked worktree (like `docs/` here) keeps its `HEAD` and
    /// index in the first and its branches in the second.
    pub fn git_dirs(&self) -> Result<(PathBuf, PathBuf), String> {
        let out = command(
            &self.root,
            &["rev-parse", "--path-format=absolute", "--git-dir", "--git-common-dir"],
        )?;
        let text = String::from_utf8(out).map_err(|e| e.to_string())?;
        let mut lines = text.lines();
        match (lines.next(), lines.next()) {
            (Some(git_dir), Some(common_dir)) => Ok((PathBuf::from(git_dir), PathBuf::from(common_dir))),
            _ => Err("Git did not report its directories".to_owned()),
        }
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

    /// The checked-out branch, or the short commit id of a detached `HEAD`.
    /// `None` outside a repository or before the first commit of a fresh one
    /// with no readable `HEAD`.
    pub fn current_branch(&self) -> Option<String> {
        let text = |out: Vec<u8>| {
            let name = String::from_utf8(out).ok()?.trim().to_owned();
            (!name.is_empty()).then_some(name)
        };
        command(&self.root, &["symbolic-ref", "--short", "-q", "HEAD"])
            .ok()
            .and_then(text)
            .or_else(|| {
                command(&self.root, &["rev-parse", "--short", "HEAD"]).ok().and_then(text)
            })
    }

    /// Absolute paths of the files `git status` reports: staged, unstaged
    /// and untracked (not ignored), both sides of a rename. Read-only:
    /// optional locks are off, so the index is not refreshed on disk.
    pub fn changed_files(&self) -> Result<Vec<PathBuf>, String> {
        Ok(self.changed_entries()?.into_iter().map(|(path, _)| path).collect())
    }

    /// Like [`Self::changed_files`], with how each file differs from `HEAD`.
    /// A file staged and edited again reports its staged kind first.
    pub fn changed_entries(&self) -> Result<Vec<(PathBuf, ChangeKind)>, String> {
        let out = command(
            &self.root,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--no-renames"],
        )?;
        Ok(parse_porcelain_entries(&out)
            .into_iter()
            .map(|(relative, kind)| (self.root.join(relative), kind))
            .collect())
    }

    /// The text of `path` (absolute, inside the worktree) at `HEAD`; `None`
    /// when `HEAD` does not hold it (a new file, or a repository without
    /// commits). Binary, oversized and non-UTF-8 blobs are errors.
    pub fn head_text(&self, path: &Path) -> Result<Option<String>, String> {
        if command(&self.root, &["rev-parse", "--verify", "-q", "HEAD^{commit}"]).is_err() {
            return Ok(None);
        }
        let (_, mut files) = self.baseline("HEAD", &[path.to_path_buf()])?;
        Ok(files.remove(path))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Runs a read-only `git` command in the worktree and returns its output.
    pub fn run(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        command(&self.root, args)
    }
}

/// Paths of `git status --porcelain=v1 -z` entries.
#[cfg(test)]
fn parse_porcelain_z(out: &[u8]) -> Vec<String> {
    parse_porcelain_entries(out).into_iter().map(|(path, _)| path).collect()
}

/// Entries of `git status --porcelain=v1 -z` (`XY path\0`; a rename or copy
/// carries its source path as the next entry, which counts as deleted).
fn parse_porcelain_entries(out: &[u8]) -> Vec<(String, ChangeKind)> {
    let mut found = Vec::new();
    let mut entries = out.split(|b| *b == 0).filter(|e| !e.is_empty());
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        let status = &entry[..2];
        let kind = kind_of(status);
        if let Ok(path) = std::str::from_utf8(&entry[3..]) {
            found.push((path.to_owned(), kind));
        }
        if (status.contains(&b'R') || status.contains(&b'C'))
            && let Some(Ok(source)) = entries.next().map(std::str::from_utf8)
        {
            found.push((source.to_owned(), ChangeKind::Deleted));
        }
    }
    found
}

fn kind_of(status: &[u8]) -> ChangeKind {
    match (status[0], status[1]) {
        (b'?', _) => ChangeKind::Untracked,
        (b'A', _) | (b'R', _) | (b'C', _) => ChangeKind::Added,
        (b'D', _) | (_, b'D') => ChangeKind::Deleted,
        _ => ChangeKind::Modified,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_entries_yield_every_path_including_rename_sources() {
        let out = b" M src/a.rs\0?? new.txt\0R  b2.rs\0b.rs\0A  dir/c d.rs\0";
        assert_eq!(
            parse_porcelain_z(out),
            vec!["src/a.rs", "new.txt", "b2.rs", "b.rs", "dir/c d.rs"]
        );
    }

    #[test]
    fn porcelain_status_letters_become_change_kinds() {
        let out = b" M a.rs\0M  b.rs\0A  c.rs\0?? d.rs\0 D e.rs\0D  f.rs\0AM g.rs\0R  h2.rs\0h.rs\0";
        let kinds: Vec<_> = parse_porcelain_entries(out).into_iter().map(|(p, k)| (p, k.letter())).collect();
        assert_eq!(
            kinds,
            [
                ("a.rs", 'M'), ("b.rs", 'M'), ("c.rs", 'A'), ("d.rs", 'U'), ("e.rs", 'D'),
                ("f.rs", 'D'), ("g.rs", 'A'), ("h2.rs", 'A'), ("h.rs", 'D'),
            ]
            .map(|(p, k)| (p.to_owned(), k))
        );
    }

    fn init_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let git = |args: &[&str]| {
            assert!(Command::new("git").arg("-C").arg(&root).args(args).output().unwrap().status.success());
        };
        git(&["init", "-q", "-b", "trunk"]);
        std::fs::write(root.join("a.rs"), "a\n").unwrap();
        git(&["add", "."]);
        git(&["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-qm", "init"]);
        (dir, root)
    }

    #[test]
    fn current_branch_names_the_branch_and_the_commit_when_detached() {
        let (_dir, root) = init_repo();
        let repo = GitRepo::open(&root).unwrap();
        assert_eq!(repo.current_branch().as_deref(), Some("trunk"));
        let status = Command::new("git").arg("-C").arg(&root).args(["checkout", "-q", "--detach"]).status().unwrap();
        assert!(status.success());
        let detached = repo.current_branch().unwrap();
        assert!(detached.len() >= 7 && detached.chars().all(|c| c.is_ascii_hexdigit()), "{detached}");
    }

    #[test]
    fn changed_entries_report_each_files_kind() {
        let (_dir, root) = init_repo();
        std::fs::write(root.join("a.rs"), "changed\n").unwrap();
        std::fs::write(root.join("new.rs"), "n\n").unwrap();
        let repo = GitRepo::open(&root).unwrap();
        let mut entries = repo.changed_entries().unwrap();
        entries.sort();
        assert_eq!(
            entries,
            vec![(root.join("a.rs"), ChangeKind::Modified), (root.join("new.rs"), ChangeKind::Untracked)]
        );
        std::fs::remove_file(root.join("a.rs")).unwrap();
        let entries = repo.changed_entries().unwrap();
        assert!(entries.contains(&(root.join("a.rs"), ChangeKind::Deleted)));
    }

    #[test]
    fn changed_files_lists_modified_and_untracked_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let git = |args: &[&str]| {
            assert!(Command::new("git").arg("-C").arg(&root).args(args).output().unwrap().status.success());
        };
        git(&["init", "-q"]);
        std::fs::write(root.join("kept.rs"), "a").unwrap();
        std::fs::write(root.join("edited.rs"), "a").unwrap();
        git(&["add", "."]);
        git(&["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-qm", "init"]);
        std::fs::write(root.join("edited.rs"), "b").unwrap();
        std::fs::write(root.join("added.rs"), "c").unwrap();
        let repo = GitRepo::open(&root).unwrap();
        let mut changed = repo.changed_files().unwrap();
        changed.sort();
        assert_eq!(changed, vec![root.join("added.rs"), root.join("edited.rs")]);
    }
}
