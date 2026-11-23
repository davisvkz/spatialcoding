//! Tells the app when Git's own state changes: `HEAD`, the index, the packed
//! refs and the branch files. A commit, `git add` or `git checkout -b` change
//! none of the working tree's files, so the file watcher never sees them.
//!
//! Git writes each of these by creating `NAME.lock` and renaming it over
//! `NAME`, which kills a watch on the file itself. So the watcher watches the
//! directories, and the filter keeps the events about the files that matter.
//! `Access` events are dropped on purpose: the app's own `git status` opens
//! `HEAD` and the index, and reacting to that would refresh forever.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use notify::event::{EventKind, ModifyKind};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

/// Files directly in a git directory whose change means the state changed.
const STATE_FILES: &[&str] = &[
    "HEAD",
    "index",
    "packed-refs",
    "ORIG_HEAD",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
];

/// Whether `event` is a change of the repository's state (see the module
/// docs). `git_dir` and `common_dir` are as [`super::repo::GitRepo::git_dirs`].
pub fn is_git_state_event(event: &Event, git_dir: &Path, common_dir: &Path) -> bool {
    let kind_counts = match event.kind {
        EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        EventKind::Modify(_) => true,
        EventKind::Access(_) | EventKind::Any | EventKind::Other => false,
    };
    kind_counts && event.paths.iter().any(|path| is_state_path(path, git_dir, common_dir))
}

fn is_state_path(path: &Path, git_dir: &Path, common_dir: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    if name.ends_with(".lock") {
        return false;
    }
    let parent = path.parent();
    if parent == Some(git_dir) || parent == Some(common_dir) {
        return STATE_FILES.contains(&name);
    }
    // Any branch file, however deep (`feature/x`), and the reftable list.
    path.starts_with(common_dir.join("refs").join("heads"))
        || (parent == Some(common_dir.join("reftable").as_path()) && name == "tables.list")
}

/// Watches one repository's state directories.
pub struct GitStateWatcher {
    git_dir: PathBuf,
    common_dir: PathBuf,
    changed: Receiver<()>,
    /// Dropping it stops the watch.
    _watcher: RecommendedWatcher,
}

impl GitStateWatcher {
    /// Starts watching. `wake` runs on the watcher's thread for every event
    /// that counts, so the UI can repaint and poll.
    pub fn new(git_dir: &Path, common_dir: &Path, wake: impl Fn() + Send + 'static) -> notify::Result<Self> {
        let (sender, changed) = mpsc::channel();
        let (dir, common) = (git_dir.to_path_buf(), common_dir.to_path_buf());
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            if event.is_ok_and(|e| is_git_state_event(&e, &dir, &common)) {
                let _ = sender.send(());
                wake();
            }
        })?;
        watcher.watch(git_dir, RecursiveMode::NonRecursive)?;
        // Optional pieces: a linked worktree has the shared directory apart,
        // a repository may keep its branches in the reftable format, and a
        // fresh one may have no branch yet.
        if common_dir != git_dir {
            watcher.watch(common_dir, RecursiveMode::NonRecursive)?;
        }
        let _ = watcher.watch(&common_dir.join("refs").join("heads"), RecursiveMode::Recursive);
        let _ = watcher.watch(&common_dir.join("reftable"), RecursiveMode::NonRecursive);
        Ok(Self { git_dir: git_dir.to_path_buf(), common_dir: common_dir.to_path_buf(), changed, _watcher: watcher })
    }

    /// The directories it watches, to tell whether the repository changed.
    pub fn dirs(&self) -> (&Path, &Path) {
        (&self.git_dir, &self.common_dir)
    }

    /// Whether the state changed since the last call.
    pub fn take_changed(&self) -> bool {
        self.changed.try_iter().count() > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, AccessMode, CreateKind, DataChange, MetadataKind, RenameMode};
    use std::process::Command;
    use std::time::{Duration, Instant};

    fn event(kind: EventKind, paths: &[&str]) -> Event {
        let mut event = Event::new(kind);
        event.paths = paths.iter().map(PathBuf::from).collect();
        event
    }

    const GIT: &str = "/r/.git";

    fn counts(kind: EventKind, paths: &[&str]) -> bool {
        is_git_state_event(&event(kind, paths), Path::new(GIT), Path::new(GIT))
    }

    #[test]
    fn the_files_git_rewrites_count_once_renamed_into_place() {
        let rename = EventKind::Modify(ModifyKind::Name(RenameMode::Both));
        assert!(counts(rename, &["/r/.git/index.lock", "/r/.git/index"]), "a rename names both paths");
        assert!(counts(EventKind::Modify(ModifyKind::Name(RenameMode::To)), &["/r/.git/HEAD"]));
        assert!(counts(EventKind::Modify(ModifyKind::Data(DataChange::Content)), &["/r/.git/packed-refs"]));
        assert!(counts(EventKind::Create(CreateKind::File), &["/r/.git/refs/heads/feature/x"]), "any depth");
        assert!(counts(EventKind::Remove(notify::event::RemoveKind::File), &["/r/.git/refs/heads/old"]));
        assert!(counts(EventKind::Create(CreateKind::File), &["/r/.git/MERGE_HEAD"]));
    }

    #[test]
    fn locks_noise_and_reads_do_not_count() {
        let write = EventKind::Modify(ModifyKind::Data(DataChange::Content));
        assert!(!counts(EventKind::Create(CreateKind::File), &["/r/.git/index.lock"]));
        assert!(!counts(write, &["/r/.git/refs/heads/main.lock"]));
        assert!(!counts(write, &["/r/.git/COMMIT_EDITMSG"]));
        assert!(!counts(write, &["/r/.git/FETCH_HEAD"]));
        assert!(!counts(write, &["/r/.git/logs/HEAD"]), "the reflog is not the state");
        assert!(!counts(write, &["/r/.git/objects/ab/cdef"]));
        assert!(!counts(write, &["/r/.git/refs/tags/v1"]), "tags do not change Changes");
        // `git status` opens and closes HEAD and the index.
        assert!(!counts(EventKind::Access(AccessKind::Open(AccessMode::Any)), &["/r/.git/index"]));
        assert!(!counts(EventKind::Access(AccessKind::Close(AccessMode::Write)), &["/r/.git/HEAD"]));
        assert!(!counts(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)), &["/r/.git/index"]));
    }

    #[test]
    fn a_linked_worktree_has_its_files_in_two_places() {
        let (git_dir, common) = (Path::new("/r/.git/worktrees/wt"), Path::new("/r/.git"));
        let write = EventKind::Modify(ModifyKind::Data(DataChange::Content));
        let at = |paths: &[&str]| is_git_state_event(&event(write, paths), git_dir, common);
        assert!(at(&["/r/.git/worktrees/wt/HEAD"]) && at(&["/r/.git/worktrees/wt/index"]));
        assert!(at(&["/r/.git/refs/heads/feat"]) && at(&["/r/.git/packed-refs"]));
        assert!(!at(&["/r/.git/worktrees/wt/index.lock"]) && !at(&["/r/.git/worktrees/wt/gitdir"]));
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn committed_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        git(&root, &["init", "-q", "-b", "trunk"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        git(&root, &["add", "a.txt"]);
        git(&root, &["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-qm", "init"]);
        (dir, root)
    }

    /// Polls until the watcher reports a change or two seconds pass.
    fn changes_within_two_seconds(watcher: &GitStateWatcher) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if watcher.take_changed() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn real_git_commands_are_seen_and_status_is_not() {
        let (_dir, root) = committed_repo();
        let repo = crate::git::repo::GitRepo::open(&root).unwrap();
        let (git_dir, common) = repo.git_dirs().unwrap();
        assert_eq!(git_dir, common, "an ordinary checkout has one directory");
        let watcher = GitStateWatcher::new(&git_dir, &common, || {}).unwrap();

        // The app's own status (`GIT_OPTIONAL_LOCKS=0`, as `repo::command`
        // sets it) only reads. A plain `git status` may refresh and rewrite
        // the index, which is a real change and would count.
        std::thread::sleep(Duration::from_millis(100));
        watcher.take_changed();
        let status = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["status", "--short"])
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .unwrap();
        assert!(status.status.success());
        assert!(!changes_within_a_moment(&watcher), "the app's git status must not count");

        std::fs::write(root.join("b.txt"), "two\n").unwrap();
        git(&root, &["add", "b.txt"]);
        assert!(changes_within_two_seconds(&watcher), "git add rewrites the index only");

        watcher.take_changed();
        git(&root, &["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-qm", "second"]);
        assert!(changes_within_two_seconds(&watcher), "git commit moves the branch");

        watcher.take_changed();
        git(&root, &["checkout", "-q", "-b", "feature/x"]);
        assert!(changes_within_two_seconds(&watcher), "a new branch touches HEAD and refs only");
    }

    fn changes_within_a_moment(watcher: &GitStateWatcher) -> bool {
        let deadline = Instant::now() + Duration::from_millis(400);
        while Instant::now() < deadline {
            if watcher.take_changed() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn a_linked_worktree_sees_its_own_commits() {
        let (_dir, root) = committed_repo();
        let wt = root.join("wt");
        git(&root, &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "feat"]);
        let repo = crate::git::repo::GitRepo::open(&wt).unwrap();
        let (git_dir, common) = repo.git_dirs().unwrap();
        assert_ne!(git_dir, common, "a linked worktree keeps HEAD apart from the shared refs");
        let watcher = GitStateWatcher::new(&git_dir, &common, || {}).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        watcher.take_changed();

        std::fs::write(wt.join("w.txt"), "w\n").unwrap();
        git(&wt, &["add", "w.txt"]);
        assert!(changes_within_two_seconds(&watcher), "its index lives in the worktree's git dir");
        watcher.take_changed();
        git(&wt, &["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-qm", "w"]);
        assert!(changes_within_two_seconds(&watcher), "its branch lives in the shared refs");
    }
}
