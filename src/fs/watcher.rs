//! File system watcher: tells the app which files of the loaded folder were
//! changed, created or removed, so open nodes and the file tree follow the
//! disk. Events are filtered with the file tree's skip rules, so build
//! output and `.git` churn never reach the app.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use notify::event::{EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use super::loader::is_skipped_path;

/// Paths touched since the last poll, as they are on disk now.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WatchBatch {
    /// Existing files whose content (or metadata) changed.
    pub modified: Vec<PathBuf>,
    /// Files and folders that appeared (or were renamed into place).
    pub created: Vec<PathBuf>,
    /// Files and folders that no longer exist.
    pub removed: Vec<PathBuf>,
}

impl WatchBatch {
    pub fn is_empty(&self) -> bool {
        self.modified.is_empty() && self.created.is_empty() && self.removed.is_empty()
    }
}

/// Watches a directory tree for changes and notifies the app.
pub struct FileWatcher {
    root: PathBuf,
    events: Receiver<notify::Result<notify::Event>>,
    /// Dropping it stops the watch.
    _watcher: RecommendedWatcher,
}

impl FileWatcher {
    /// Starts watching `root` recursively. `wake` runs on the watcher's
    /// thread for every event, so the UI can repaint and poll.
    pub fn new(root: &Path, wake: impl Fn() + Send + 'static) -> notify::Result<Self> {
        let (sender, events) = mpsc::channel();
        let watched = root.to_path_buf();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            // Only real changes wake the UI: the app's own reads raise
            // access events, and build output is never shown.
            let relevant = event.as_ref().is_ok_and(|e| {
                !matches!(e.kind, EventKind::Access(_) | EventKind::Other)
                    && e.paths.iter().any(|p| !is_skipped_path(p, &watched, true))
            });
            if relevant {
                let _ = sender.send(event);
                wake();
            }
        })?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        Ok(Self { root: root.to_path_buf(), events, _watcher: watcher })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Everything that happened since the last call, merged per path and
    /// checked against the disk: an editor's save (write a temp file, rename
    /// it over the original) comes out as one modified or created file.
    pub fn poll_changes(&mut self, show_dot_folders: bool) -> WatchBatch {
        let mut modified = BTreeSet::new();
        let mut created = BTreeSet::new();
        let mut removed = BTreeSet::new();
        while let Ok(event) = self.events.try_recv() {
            let Ok(event) = event else { continue };
            for path in event.paths {
                if is_skipped_path(&path, &self.root, show_dot_folders) {
                    continue;
                }
                match event.kind {
                    EventKind::Create(_) => {
                        created.insert(path);
                    }
                    EventKind::Remove(_) => {
                        removed.insert(path);
                    }
                    EventKind::Modify(ModifyKind::Name(_)) => {
                        // Either side of a rename: the disk tells which.
                        if path.exists() {
                            created.insert(path);
                        } else {
                            removed.insert(path);
                        }
                    }
                    EventKind::Modify(_) => {
                        modified.insert(path);
                    }
                    EventKind::Access(_) | EventKind::Any | EventKind::Other => {}
                }
            }
        }
        settle(modified, created, removed)
    }
}

/// Reconciles the event kinds with what exists now.
fn settle(modified: BTreeSet<PathBuf>, created: BTreeSet<PathBuf>, removed: BTreeSet<PathBuf>) -> WatchBatch {
    let mut batch = WatchBatch::default();
    let mut seen = BTreeSet::new();
    for path in created.iter().chain(&modified).chain(&removed) {
        if !seen.insert(path.clone()) {
            continue;
        }
        if !path.exists() {
            batch.removed.push(path.clone());
        } else if created.contains(path) {
            batch.created.push(path.clone());
        } else if removed.contains(path) {
            // Deleted and written again: a new file under the same name.
            batch.created.push(path.clone());
        } else if path.is_file() {
            batch.modified.push(path.clone());
        }
    }
    batch
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn set(paths: &[&Path]) -> BTreeSet<PathBuf> {
        paths.iter().map(|p| p.to_path_buf()).collect()
    }

    #[test]
    fn settle_checks_events_against_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let (kept, edited, gone, replaced) =
            (dir.path().join("kept"), dir.path().join("edited"), dir.path().join("gone"), dir.path().join("replaced"));
        for p in [&kept, &edited, &replaced] {
            std::fs::write(p, "x").unwrap();
        }
        let batch = settle(set(&[&edited]), set(&[&kept, &gone]), set(&[&replaced]));
        assert_eq!(batch.modified, vec![edited]);
        assert_eq!(batch.created, vec![kept, replaced]);
        assert_eq!(batch.removed, vec![gone]);
    }

    #[test]
    fn a_folder_modified_event_is_not_a_file_change() {
        let dir = tempfile::tempdir().unwrap();
        let batch = settle(set(&[dir.path()]), BTreeSet::new(), BTreeSet::new());
        assert!(batch.is_empty());
    }

    /// Polls until `done` holds or five seconds pass.
    fn poll_until(watcher: &mut FileWatcher, mut done: impl FnMut(&WatchBatch) -> bool) -> WatchBatch {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut all = WatchBatch::default();
        while Instant::now() < deadline {
            let batch = watcher.poll_changes(false);
            all.modified.extend(batch.modified);
            all.created.extend(batch.created);
            all.removed.extend(batch.removed);
            if done(&all) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        all
    }

    #[test]
    fn reports_created_modified_and_removed_files_and_wakes_the_ui() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let existing = root.join("a.txt");
        std::fs::write(&existing, "one").unwrap();
        let woken = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = woken.clone();
        let mut watcher = FileWatcher::new(&root, move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })
        .unwrap();

        std::fs::write(&existing, "two").unwrap();
        let batch = poll_until(&mut watcher, |b| b.modified.contains(&existing));
        assert!(batch.modified.contains(&existing), "{batch:?}");

        let created = root.join("sub").join("b.txt");
        std::fs::create_dir(root.join("sub")).unwrap();
        // A file written into a folder before its own watch exists is not
        // reported by itself (the folder's event is: the app reloads the
        // tree), so wait for the folder first.
        let batch = poll_until(&mut watcher, |b| b.created.contains(&root.join("sub")));
        assert!(batch.created.contains(&root.join("sub")), "{batch:?}");
        std::fs::write(&created, "x").unwrap();
        let batch = poll_until(&mut watcher, |b| b.created.contains(&created) || b.modified.contains(&created));
        assert!(batch.created.contains(&created) || batch.modified.contains(&created), "{batch:?}");

        std::fs::remove_file(&existing).unwrap();
        let batch = poll_until(&mut watcher, |b| b.removed.contains(&existing));
        assert!(batch.removed.contains(&existing), "{batch:?}");
        assert!(woken.load(std::sync::atomic::Ordering::SeqCst) > 0);
    }

    #[test]
    fn ignored_folders_and_dot_files_produce_no_events() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir(root.join("target")).unwrap();
        let mut watcher = FileWatcher::new(&root, || {}).unwrap();
        std::fs::write(root.join("target").join("out.o"), "x").unwrap();
        std::fs::write(root.join(".secret"), "x").unwrap();
        let visible = root.join("seen.txt");
        std::fs::write(&visible, "x").unwrap();
        let batch = poll_until(&mut watcher, |b| b.created.contains(&visible) || b.modified.contains(&visible));
        let all: Vec<_> = batch.created.iter().chain(&batch.modified).collect();
        assert!(all.iter().all(|p| !p.starts_with(root.join("target")) && p.file_name().unwrap() != ".secret"), "{batch:?}");
        // With dot folders shown, the same file is reported.
        std::fs::write(root.join(".other"), "x").unwrap();
        let mut shown = WatchBatch::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && shown.created.is_empty() && shown.modified.is_empty() {
            let b = watcher.poll_changes(true);
            shown.created.extend(b.created);
            shown.modified.extend(b.modified);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(shown.created.iter().chain(&shown.modified).any(|p| p.ends_with(".other")), "{shown:?}");
    }
}
