//! Line diff of one file against Git `HEAD`: the committed blob (read like
//! every other Git access, read-only through the `git` CLI) against the file
//! on disk. The diff itself is computed in Rust, so a deleted or untracked
//! file diffs against empty text.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use similar::{ChangeTag, TextDiff};

use super::repo::{GitRepo, MAX_FILE_BYTES};

/// Unchanged lines kept around each change.
pub const CONTEXT_LINES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// 1-based line numbers in `HEAD` and in the working file.
    pub old_no: Option<usize>,
    pub new_no: Option<usize>,
    pub text: String,
}

/// A run of changes with its surrounding context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub lines: Vec<DiffLine>,
}

/// One row of the side-by-side view: a removed line on the left, an added
/// line on the right, a context line on both, or a blank filler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitRow {
    pub left: Option<DiffLine>,
    pub right: Option<DiffLine>,
}

impl Hunk {
    /// Pairs removed and added lines of each change block row by row.
    pub fn split_rows(&self) -> Vec<SplitRow> {
        let mut rows = Vec::new();
        let mut i = 0;
        while i < self.lines.len() {
            if self.lines[i].kind == LineKind::Context {
                rows.push(SplitRow { left: Some(self.lines[i].clone()), right: Some(self.lines[i].clone()) });
                i += 1;
                continue;
            }
            let start = i;
            while i < self.lines.len() && self.lines[i].kind != LineKind::Context {
                i += 1;
            }
            let block = &self.lines[start..i];
            let removed: Vec<_> = block.iter().filter(|l| l.kind == LineKind::Removed).collect();
            let added: Vec<_> = block.iter().filter(|l| l.kind == LineKind::Added).collect();
            for n in 0..removed.len().max(added.len()) {
                rows.push(SplitRow { left: removed.get(n).map(|l| (*l).clone()), right: added.get(n).map(|l| (*l).clone()) });
            }
        }
        rows
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: PathBuf,
    pub hunks: Vec<Hunk>,
    pub added: usize,
    pub removed: usize,
    /// The file is new (not in `HEAD`).
    pub is_new: bool,
    /// The file no longer exists on disk.
    pub is_deleted: bool,
    /// The working file is binary, so no lines are shown.
    pub binary: bool,
    /// Both sides, so the hunks can be cut again with another context.
    pub old_text: Option<String>,
    pub new_text: Option<String>,
}

impl FileDiff {
    /// The hunks with `context` unchanged lines around each change.
    pub fn hunks_with(&self, context: usize) -> Vec<Hunk> {
        diff_texts_with(self.old_text.as_deref().unwrap_or(""), self.new_text.as_deref().unwrap_or(""), context)
    }
}

/// Hunks turning `old` into `new`, with [`CONTEXT_LINES`] of context.
pub fn diff_texts(old: &str, new: &str) -> Vec<Hunk> {
    diff_texts_with(old, new, CONTEXT_LINES)
}

/// Hunks turning `old` into `new`, with `context` unchanged lines around
/// each change.
pub fn diff_texts_with(old: &str, new: &str, context: usize) -> Vec<Hunk> {
    let diff = TextDiff::from_lines(old, new);
    diff.grouped_ops(context)
        .iter()
        .map(|group| Hunk {
            lines: group
                .iter()
                .flat_map(|op| diff.iter_changes(op))
                .map(|change| DiffLine {
                    kind: match change.tag() {
                        ChangeTag::Equal => LineKind::Context,
                        ChangeTag::Insert => LineKind::Added,
                        ChangeTag::Delete => LineKind::Removed,
                    },
                    old_no: change.old_index().map(|i| i + 1),
                    new_no: change.new_index().map(|i| i + 1),
                    text: change.value().trim_end_matches(['\n', '\r']).to_owned(),
                })
                .collect(),
        })
        .collect()
}

/// Diffs `path` (absolute, inside a Git worktree) against `HEAD`. Reads Git
/// and the disk: call it from a worker, never a UI frame.
pub fn file_diff(path: &Path) -> Result<FileDiff, String> {
    let dir = path.parent().ok_or("File has no folder")?;
    let repo = GitRepo::open(dir).ok_or("Not a Git repository")?;
    // Git reports the repository's real path; the file may be reached through a link.
    let real = std::fs::canonicalize(dir).map(|d| d.join(path.file_name().unwrap_or_default())).unwrap_or_else(|_| path.to_path_buf());
    let old = repo.head_text(&real)?;
    let (new, binary) = match std::fs::metadata(path) {
        Err(_) => (None, false),
        Ok(meta) if meta.len() as usize > MAX_FILE_BYTES => {
            return Err("File too large to diff (over 2 MiB)".into());
        }
        Ok(_) => match crate::fs::loader::read_file_content(path) {
            Ok(text) => (Some(text), false),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => (None, true),
            Err(e) => return Err(e.to_string()),
        },
    };
    if binary {
        return Ok(FileDiff {
            path: path.to_path_buf(),
            hunks: Vec::new(),
            added: 0,
            removed: 0,
            is_new: old.is_none(),
            is_deleted: false,
            binary: true,
            old_text: None,
            new_text: None,
        });
    }
    let hunks = diff_texts(old.as_deref().unwrap_or(""), new.as_deref().unwrap_or(""));
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffStatus {
    Loading,
    Ready(FileDiff),
    Failed(String),
}

/// What a diff compares.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DiffSource {
    /// The working file against `HEAD`.
    #[default]
    Working,
    /// The file at commit `from` against commit `to`.
    Between { from: String, to: String },
}

/// The diff of one file being read in the background. Asking again keeps the
/// previous answer on screen until the new one arrives.
pub struct DiffLoader {
    pub path: PathBuf,
    pub source: DiffSource,
    /// Unchanged lines around each change in a diff between commits.
    pub context: usize,
    pub status: DiffStatus,
    pending: Option<Receiver<Result<FileDiff, String>>>,
}

impl DiffLoader {
    pub fn new(path: PathBuf) -> Self {
        Self::with_source(path, DiffSource::Working, CONTEXT_LINES)
    }

    pub fn with_source(path: PathBuf, source: DiffSource, context: usize) -> Self {
        let mut loader = Self { path, source, context, status: DiffStatus::Loading, pending: None };
        loader.request();
        loader
    }

    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
    }

    /// Reads the diff again on a worker thread.
    pub fn request(&mut self) {
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        let path = self.path.clone();
        let (source, context) = (self.source.clone(), self.context);
        std::thread::spawn(move || {
            let result = match source {
                DiffSource::Working => file_diff(&path),
                DiffSource::Between { from, to } => path
                    .parent()
                    .and_then(GitRepo::open)
                    .ok_or_else(|| "Not a Git repository".to_string())
                    .and_then(|repo| super::history::diff_between(&repo, &from, &to, &path, context)),
            };
            let _ = sender.send(result);
        });
    }

    /// Nonblocking; true when an answer was installed.
    pub fn poll(&mut self) -> bool {
        let Some(receiver) = &self.pending else { return false };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err("Diff worker stopped".to_owned()),
        };
        self.pending = None;
        self.status = match result {
            Ok(diff) => DiffStatus::Ready(diff),
            Err(message) => DiffStatus::Failed(message),
        };
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn lines(hunk: &Hunk) -> Vec<(char, String)> {
        hunk.lines
            .iter()
            .map(|l| (match l.kind { LineKind::Context => ' ', LineKind::Added => '+', LineKind::Removed => '-' }, l.text.clone()))
            .collect()
    }

    #[test]
    fn identical_texts_have_no_hunks() {
        assert!(diff_texts("a\nb\n", "a\nb\n").is_empty());
    }

    #[test]
    fn a_changed_line_is_a_removal_and_an_addition_with_numbers() {
        let hunks = diff_texts("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(hunks.len(), 1);
        assert_eq!(
            lines(&hunks[0]),
            [(' ', "a"), ('-', "b"), ('+', "B"), (' ', "c")].map(|(k, t)| (k, t.to_owned()))
        );
        let removed = &hunks[0].lines[1];
        assert_eq!((removed.old_no, removed.new_no), (Some(2), None));
        let added = &hunks[0].lines[2];
        assert_eq!((added.old_no, added.new_no), (None, Some(2)));
    }

    #[test]
    fn distant_changes_make_separate_hunks_with_three_lines_of_context() {
        let old: String = (1..=30).map(|i| format!("l{i}\n")).collect();
        let new = old.replace("l2\n", "L2\n").replace("l29\n", "L29\n");
        let hunks = diff_texts(&old, &new);
        assert_eq!(hunks.len(), 2);
        // 1 context before, change, 3 after.
        assert_eq!(hunks[0].lines.len(), 1 + 2 + 3);
        assert!(hunks[1].lines.first().is_some_and(|l| l.old_no == Some(26)));
    }

    #[test]
    fn split_rows_pair_removals_with_additions_and_fill_blanks() {
        let hunks = diff_texts("a\nb\nc\nd\n", "a\nX\nY\nZ\nd\n");
        let rows = hunks[0].split_rows();
        let text = |l: &Option<DiffLine>| l.as_ref().map(|l| l.text.clone());
        let pairs: Vec<_> = rows.iter().map(|r| (text(&r.left), text(&r.right))).collect();
        let s = |t: &str| Some(t.to_owned());
        assert_eq!(
            pairs,
            vec![(s("a"), s("a")), (s("b"), s("X")), (s("c"), s("Y")), (None, s("Z")), (s("d"), s("d"))]
        );
    }

    #[test]
    fn new_and_deleted_texts_diff_against_empty() {
        let added = diff_texts("", "x\ny\n");
        assert_eq!(added[0].lines.iter().filter(|l| l.kind == LineKind::Added).count(), 2);
        let removed = diff_texts("x\ny\n", "");
        assert!(removed[0].lines.iter().all(|l| l.kind == LineKind::Removed));
    }

    fn repo_with(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let git = |args: &[&str]| {
            assert!(Command::new("git").arg("-C").arg(&root).args(args).output().unwrap().status.success());
        };
        git(&["init", "-q"]);
        for (name, text) in files {
            std::fs::write(root.join(name), text).unwrap();
        }
        git(&["add", "."]);
        git(&["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-qm", "init"]);
        (dir, root)
    }

    #[test]
    fn file_diff_compares_head_with_the_working_file() {
        let (_dir, root) = repo_with(&[("a.rs", "one\ntwo\n")]);
        std::fs::write(root.join("a.rs"), "one\n2\nthree\n").unwrap();
        let diff = file_diff(&root.join("a.rs")).unwrap();
        assert_eq!((diff.added, diff.removed), (2, 1));
        assert!(!diff.is_new && !diff.is_deleted);
    }

    #[test]
    fn untracked_and_deleted_files_are_all_added_or_all_removed() {
        let (_dir, root) = repo_with(&[("kept.rs", "k\n"), ("gone.rs", "g1\ng2\n")]);
        std::fs::write(root.join("new.rs"), "n\n").unwrap();
        std::fs::remove_file(root.join("gone.rs")).unwrap();
        let new = file_diff(&root.join("new.rs")).unwrap();
        assert!(new.is_new);
        assert_eq!((new.added, new.removed), (1, 0));
        let gone = file_diff(&root.join("gone.rs")).unwrap();
        assert!(gone.is_deleted);
        assert_eq!((gone.added, gone.removed), (0, 2));
    }

    #[test]
    fn binary_working_files_have_no_lines() {
        let (_dir, root) = repo_with(&[("a.rs", "x\n")]);
        std::fs::write(root.join("b.bin"), b"\0\0\0").unwrap();
        let diff = file_diff(&root.join("b.bin")).unwrap();
        assert!(diff.binary && diff.hunks.is_empty());
    }

    #[test]
    fn a_repository_without_commits_diffs_against_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        assert!(Command::new("git").arg("-C").arg(&root).args(["init", "-q"]).status().unwrap().success());
        std::fs::write(root.join("a.rs"), "x\n").unwrap();
        let diff = file_diff(&root.join("a.rs")).unwrap();
        assert!(diff.is_new);
        assert_eq!(diff.added, 1);
    }

    #[test]
    fn outside_a_repository_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "x").unwrap();
        // The temp dir may sit inside a repository on some machines.
        if GitRepo::open(dir.path()).is_none() {
            assert!(file_diff(&dir.path().join("a.rs")).is_err());
        }
    }
}
