//! Which files of the workspace differ from Git `HEAD`, read in the
//! background. Feeds the File nodes and Folders actions about changed files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use super::repo::GitRepo;

/// How often the app asks Git again while it runs, in seconds.
pub const REFRESH_SECONDS: f64 = 10.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangesStatus {
    /// Never asked, or the workspace changed.
    Idle,
    Loading,
    Ready,
    /// Not a Git worktree, or Git is missing or failed.
    Unavailable(String),
}

/// The working tree's changed files (staged, unstaged and untracked),
/// as absolute paths.
pub struct WorkingTreeChanges {
    pub status: ChangesStatus,
    paths: HashSet<PathBuf>,
    pending: Option<Receiver<Result<Vec<PathBuf>, String>>>,
    /// App time of the last request, for the periodic refresh.
    pub requested_at: Option<f64>,
}

impl Default for WorkingTreeChanges {
    fn default() -> Self {
        Self {
            status: ChangesStatus::Idle,
            paths: HashSet::new(),
            pending: None,
            requested_at: None,
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
        if self.status == ChangesStatus::Idle {
            self.status = ChangesStatus::Loading;
        }
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            let result = match GitRepo::open(&root) {
                Some(repo) => repo.changed_files(),
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
            Ok(paths) => {
                self.paths = paths.into_iter().collect();
                self.status = ChangesStatus::Ready;
            }
            Err(message) => {
                self.paths.clear();
                self.status = ChangesStatus::Unavailable(message);
            }
        }
        true
    }

    #[cfg(test)]
    pub fn set_for_test(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.paths = paths.into_iter().collect();
        self.status = ChangesStatus::Ready;
    }
}
