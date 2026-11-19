//! Git history for the Changes panel: the commits of the first-parent
//! history, the files two commits differ in (a single commit against its
//! parent, or a Commit range: the net changes between two commits) and one
//! file's diff between them. Read-only, through the `git` CLI; call from a
//! worker, never a UI frame.

use std::path::{Path, PathBuf};

use super::diff::{diff_texts_with, FileDiff, LineKind, CONTEXT_LINES};
use super::repo::{ChangeKind, GitRepo};

/// The empty tree: what a root commit is compared with.
pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Commits listed at most.
pub const MAX_COMMITS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    /// "3 days ago".
    pub when: String,
}

/// The first-parent history from `HEAD`, newest first.
pub fn commits(repo: &GitRepo, limit: usize) -> Result<Vec<Commit>, String> {
    let limit = limit.min(MAX_COMMITS).to_string();
    let out = match repo.run(&["log", "--first-parent", "-n", &limit, "--format=%H%x1f%h%x1f%s%x1f%an%x1f%ar"]) {
        Ok(out) => out,
        // A repository without commits has no history.
        Err(_) => return Ok(Vec::new()),
    };
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|line| {
            let mut f = line.split('\x1f');
            Some(Commit {
                hash: f.next()?.to_string(),
                short: f.next()?.to_string(),
                subject: f.next()?.to_string(),
                author: f.next()?.to_string(),
                when: f.next()?.to_string(),
            })
        })
        .collect())
}

/// The commit a commit is compared with: its first parent, or the empty tree.
pub fn base_of(repo: &GitRepo, commit: &str) -> String {
    repo.run(&["rev-parse", "--verify", "--end-of-options", &format!("{commit}^")])
        .ok()
        .and_then(|out| String::from_utf8(out).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| EMPTY_TREE.to_string())
}

/// The files that differ between `from` and `to` (absolute paths), with how.
/// A rename counts as a deletion and an addition.
pub fn changed_between(repo: &GitRepo, from: &str, to: &str) -> Result<Vec<(PathBuf, ChangeKind)>, String> {
    let out = repo.run(&["diff", "--name-status", "-z", "--no-renames", "--end-of-options", from, to])?;
    let mut fields = out.split(|b| *b == 0).filter(|f| !f.is_empty());
    let mut found = Vec::new();
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        let kind = match status.first() {
            Some(b'A') => ChangeKind::Added,
            Some(b'D') => ChangeKind::Deleted,
            _ => ChangeKind::Modified,
        };
        found.push((repo.root().join(String::from_utf8_lossy(path).as_ref()), kind));
    }
    found.sort();
    Ok(found)
}

/// One file's diff between two commits: `from` is the old side, `to` the new.
pub fn diff_between(repo: &GitRepo, from: &str, to: &str, path: &Path, context: usize) -> Result<FileDiff, String> {
    let text_at = |rev: &str| -> Result<Option<String>, String> {
        if rev == EMPTY_TREE {
            return Ok(None);
        }
        let (_, mut files) = repo.baseline(rev, &[path.to_path_buf()])?;
        Ok(files.remove(path))
    };
    let (old, new) = (text_at(from)?, text_at(to)?);
    let hunks = diff_texts_with(old.as_deref().unwrap_or(""), new.as_deref().unwrap_or(""), context);
    let count = |kind| hunks.iter().flat_map(|h| &h.lines).filter(|l| l.kind == kind).count();
    Ok(FileDiff {
        path: path.to_path_buf(),
        added: count(LineKind::Added),
        removed: count(LineKind::Removed),
        hunks,
        is_new: old.is_none(),
        is_deleted: new.is_none(),
        binary: false,
        old_text: old,
        new_text: new,
    })
}

/// [`diff_between`] with the default context, for callers without settings.
pub fn diff_between_default(repo: &GitRepo, from: &str, to: &str, path: &Path) -> Result<FileDiff, String> {
    diff_between(repo, from, to, path, CONTEXT_LINES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A repository with three commits: init (a.rs, b.rs), edit a.rs and add
    /// c.rs, then delete b.rs and edit c.rs.
    fn repo() -> (tempfile::TempDir, GitRepo, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let git = |args: &[&str]| {
            assert!(Command::new("git").arg("-C").arg(&root).args(args).output().unwrap().status.success(), "{args:?}");
        };
        git(&["init", "-q", "-b", "trunk"]);
        let commit = |msg: &str| git(&["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-qam", msg]);
        std::fs::write(root.join("a.rs"), "one\ntwo\n").unwrap();
        std::fs::write(root.join("b.rs"), "b\n").unwrap();
        git(&["add", "."]);
        commit("init");
        std::fs::write(root.join("a.rs"), "one\n2\n").unwrap();
        std::fs::write(root.join("c.rs"), "c\n").unwrap();
        git(&["add", "."]);
        commit("edit a, add c");
        std::fs::remove_file(root.join("b.rs")).unwrap();
        std::fs::write(root.join("c.rs"), "c\nc2\n").unwrap();
        git(&["add", "-A"]);
        commit("drop b, edit c");
        let repo = GitRepo::open(&root).unwrap();
        let hashes = commits(&repo, 10).unwrap().into_iter().map(|c| c.hash).collect();
        (dir, repo, hashes)
    }

    #[test]
    fn commits_list_the_first_parent_history_newest_first() {
        let (_dir, repo, hashes) = repo();
        let list = commits(&repo, 10).unwrap();
        let subjects: Vec<&str> = list.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subjects, ["drop b, edit c", "edit a, add c", "init"]);
        assert_eq!(hashes.len(), 3);
        assert!(list[0].short.len() >= 7 && list[0].author == "T" && !list[0].when.is_empty());
        assert_eq!(commits(&repo, 2).unwrap().len(), 2);
    }

    #[test]
    fn a_commit_is_compared_with_its_parent_and_a_root_commit_with_the_empty_tree() {
        let (_dir, repo, h) = repo();
        assert_eq!(base_of(&repo, &h[0]), h[1]);
        assert_eq!(base_of(&repo, &h[2]), EMPTY_TREE);
        let root = repo.root().to_path_buf();
        let single = changed_between(&repo, &base_of(&repo, &h[0]), &h[0]).unwrap();
        assert_eq!(single, vec![(root.join("b.rs"), ChangeKind::Deleted), (root.join("c.rs"), ChangeKind::Modified)]);
        let first = changed_between(&repo, EMPTY_TREE, &h[2]).unwrap();
        assert!(first.iter().all(|(_, k)| *k == ChangeKind::Added) && first.len() == 2);
    }

    #[test]
    fn a_range_shows_the_net_changes_between_two_commits() {
        let (_dir, repo, h) = repo();
        let root = repo.root().to_path_buf();
        // init..HEAD: a.rs edited, b.rs gone, c.rs new (added then edited counts once).
        let range = changed_between(&repo, &h[2], &h[0]).unwrap();
        assert_eq!(
            range,
            vec![(root.join("a.rs"), ChangeKind::Modified), (root.join("b.rs"), ChangeKind::Deleted), (root.join("c.rs"), ChangeKind::Added)]
        );
        assert!(changed_between(&repo, &h[0], &h[0]).unwrap().is_empty());
        assert!(changed_between(&repo, "nope", &h[0]).is_err());
    }

    #[test]
    fn a_file_diff_between_commits_reads_both_sides_from_git() {
        let (_dir, repo, h) = repo();
        let a = repo.root().join("a.rs");
        let d = diff_between_default(&repo, &h[2], &h[0], &a).unwrap();
        assert_eq!((d.added, d.removed), (1, 1));
        assert!(!d.is_new && !d.is_deleted);
        let c = diff_between_default(&repo, &h[2], &h[0], &repo.root().join("c.rs")).unwrap();
        assert!(c.is_new && c.added == 2);
        let b = diff_between_default(&repo, &h[2], &h[0], &repo.root().join("b.rs")).unwrap();
        assert!(b.is_deleted && b.removed == 1);
        let from_root = diff_between_default(&repo, EMPTY_TREE, &h[2], &a).unwrap();
        assert!(from_root.is_new);
    }

    #[test]
    fn a_repository_without_commits_has_no_history() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        assert!(Command::new("git").arg("-C").arg(&root).args(["init", "-q"]).status().unwrap().success());
        let repo = GitRepo::open(&root).unwrap();
        assert!(commits(&repo, 10).unwrap().is_empty());
    }
}
