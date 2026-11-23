//! Which files of the workspace differ from Git `HEAD`, read in the
//! background. Feeds the File nodes and Folders actions about changed files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use super::repo::{ChangeKind, GitRepo};

/// How often the app asks Git again while it runs, in seconds, when nothing
/// tells it that Git's state changed.
pub const REFRESH_SECONDS: f64 = 10.0;

/// The same, as a safety net while a Git state watcher runs: it catches what
/// the watcher can miss (overflowed events, network drives, other ref stores).
pub const FALLBACK_REFRESH_SECONDS: f64 = 60.0;

/// How long after the last Git state event the app asks again. A commit or a
/// checkout writes several files in a row; one `git status` covers them all.
pub const GIT_STATE_DEBOUNCE_SECONDS: f64 = 0.15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangesStatus {
    /// Never asked, or the workspace changed.
    Idle,
    Loading,
    Ready,
    /// Not a Git worktree, or Git is missing or failed.
    Unavailable(String),
}

/// What one `git status` worker found.
struct Snapshot {
    branch: Option<String>,
    entries: Vec<(PathBuf, ChangeKind)>,
    /// The repository's `.git` directories, for the state watcher.
    git_dirs: Option<(PathBuf, PathBuf)>,
}

/// The working tree's changed files (staged, unstaged and untracked),
/// as absolute paths, and the checked-out branch.
pub struct WorkingTreeChanges {
    pub status: ChangesStatus,
    paths: HashSet<PathBuf>,
    /// The same files, sorted by path, with how each differs from `HEAD`.
    entries: Vec<(PathBuf, ChangeKind)>,
    branch: Option<String>,
    pending: Option<Receiver<Result<Snapshot, String>>>,
    /// App time of the last request, for the periodic refresh.
    pub requested_at: Option<f64>,
    /// App time a refresh was scheduled for by a Git state event. It is only
    /// cleared by a request, so an event that arrives while one is running
    /// still gets a later one: the running status may predate the change.
    refresh_at: Option<f64>,
    git_dirs: Option<(PathBuf, PathBuf)>,
}

impl Default for WorkingTreeChanges {
    fn default() -> Self {
        Self {
            status: ChangesStatus::Idle,
            paths: HashSet::new(),
            entries: Vec::new(),
            branch: None,
            pending: None,
            requested_at: None,
            refresh_at: None,
            git_dirs: None,
        }
    }
}

impl WorkingTreeChanges {
    /// True once a status answer arrived (it may list no files).
    pub fn is_known(&self) -> bool {
        self.status == ChangesStatus::Ready
    }

    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
    }

    pub fn is_changed(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.paths.iter()
    }

    /// Changed files with their kind, sorted by path.
    pub fn entries(&self) -> &[(PathBuf, ChangeKind)] {
        &self.entries
    }

    /// The checked-out branch (or short commit id when detached).
    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    /// The repository's `.git` directory and its shared one (see
    /// [`GitRepo::git_dirs`]), once Git answered.
    pub fn git_dirs(&self) -> Option<(&Path, &Path)> {
        self.git_dirs.as_ref().map(|(dir, common)| (dir.as_path(), common.as_path()))
    }

    /// Asks again `delay` seconds after `now`, even if an earlier call already
    /// scheduled it: the delay restarts with every event, so a burst of
    /// writes ends in one request.
    pub fn schedule_refresh(&mut self, now: f64, delay: f64) {
        self.refresh_at = Some(now + delay);
    }

    /// Whether to ask Git now (the caller also checks it is not loading):
    /// nothing was asked yet, the interval passed (`watching_git` picks the
    /// long safety-net one), or a scheduled refresh is due.
    pub fn is_due(&self, now: f64, watching_git: bool) -> bool {
        let interval = if watching_git { FALLBACK_REFRESH_SECONDS } else { REFRESH_SECONDS };
        self.requested_at.is_none_or(|at| now - at >= interval) || self.refresh_at.is_some_and(|at| now >= at)
    }

    /// Seconds until a scheduled refresh is due, to wake the UI then.
    pub fn scheduled_in(&self, now: f64) -> Option<f64> {
        self.refresh_at.map(|at| (at - now).max(0.0))
    }

    /// Forgets the answer (another workspace was loaded).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Asks Git for the changed files under `root` on a worker thread. The
    /// previous answer stays usable until the new one arrives.
    pub fn request(&mut self, root: &Path, now: f64) {
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        self.requested_at = Some(now);
        self.refresh_at = None;
        if self.status == ChangesStatus::Idle {
            self.status = ChangesStatus::Loading;
        }
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            let result = match GitRepo::open(&root) {
                Some(repo) => repo.changed_entries().map(|entries| Snapshot {
                    branch: repo.current_branch(),
                    entries,
                    git_dirs: repo.git_dirs().ok(),
                }),
                None => Err("Not a Git repository".to_owned()),
            };
            let _ = sender.send(result);
        });
    }

    /// Nonblocking; returns true when an answer was installed.
    pub fn poll(&mut self) -> bool {
        let Some(receiver) = &self.pending else {
            return false;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err("Git status worker stopped".to_owned()),
        };
        self.pending = None;
        match result {
            Ok(mut snapshot) => {
                snapshot.entries.sort();
                self.paths = snapshot.entries.iter().map(|(p, _)| p.clone()).collect();
                self.entries = snapshot.entries;
                self.branch = snapshot.branch;
                self.git_dirs = snapshot.git_dirs;
                self.status = ChangesStatus::Ready;
            }
            Err(message) => {
                self.paths.clear();
                self.entries.clear();
                self.branch = None;
                self.git_dirs = None;
                self.status = ChangesStatus::Unavailable(message);
            }
        }
        true
    }

    #[cfg(test)]
    pub fn set_for_test(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.entries = paths.into_iter().map(|p| (p, ChangeKind::Modified)).collect();
        self.entries.sort();
        self.paths = self.entries.iter().map(|(p, _)| p.clone()).collect();
        self.status = ChangesStatus::Ready;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_watcher_it_asks_every_ten_seconds() {
        assert!(WorkingTreeChanges::default().is_due(0.0, false), "never asked");
        let changes = WorkingTreeChanges { requested_at: Some(100.0), ..Default::default() };
        assert!(!changes.is_due(109.9, false));
        assert!(changes.is_due(110.0, false));
    }

    #[test]
    fn a_watcher_stretches_the_interval_to_a_safety_net() {
        let changes = WorkingTreeChanges { requested_at: Some(100.0), ..Default::default() };
        assert!(!changes.is_due(159.9, true) && changes.is_due(160.0, true));
        assert!(changes.is_due(110.0, false), "the same moment is due without a watcher");
    }

    #[test]
    fn a_scheduled_refresh_waits_for_the_debounce_and_restarts_with_each_event() {
        let mut changes = WorkingTreeChanges { requested_at: Some(100.0), ..Default::default() };
        changes.schedule_refresh(101.0, GIT_STATE_DEBOUNCE_SECONDS);
        assert!(!changes.is_due(101.1, true), "still inside the debounce");
        // A second event of the same burst pushes it back.
        changes.schedule_refresh(101.1, GIT_STATE_DEBOUNCE_SECONDS);
        assert!(!changes.is_due(101.2, true));
        assert!(changes.is_due(101.25, true));
        assert!((changes.scheduled_in(101.15).unwrap() - 0.1).abs() < 1e-9);
        assert_eq!(WorkingTreeChanges::default().scheduled_in(5.0), None);
    }

    #[test]
    fn a_request_clears_the_schedule_but_an_event_while_loading_keeps_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut changes = WorkingTreeChanges::default();
        changes.schedule_refresh(1.0, GIT_STATE_DEBOUNCE_SECONDS);
        changes.request(dir.path(), 1.2);
        assert_eq!(changes.scheduled_in(1.2), None, "the request answers the event");
        assert!(changes.is_loading());
        // The change lands while that status is running: it may predate it.
        changes.schedule_refresh(1.3, GIT_STATE_DEBOUNCE_SECONDS);
        assert!(changes.scheduled_in(1.3).is_some() && changes.is_loading());
        assert!(changes.is_due(1.5, true), "due as soon as the loading ends");
    }
}
