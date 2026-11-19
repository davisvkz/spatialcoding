//! Undo and redo of the canvas (`canvas:undo`, `canvas:redo`): which files
//! are open, where they sit and which folders are collapsed. The history is
//! kept by watching the canvas: when its snapshot changes and no drag is in
//! progress, the previous snapshot becomes an undo step (a drag is one step).

use std::path::PathBuf;

use crate::state::AppState;

/// Most steps kept.
const MAX_STEPS: usize = 100;

/// One open file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSnap {
    path: PathBuf,
    /// Position in half world px, so float noise is no change.
    x: i32,
    y: i32,
    collapsed: bool,
}

/// What undo restores.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    files: Vec<FileSnap>,
    folders: Vec<(PathBuf, bool)>,
}

impl Snapshot {
    pub fn capture(state: &AppState) -> Self {
        let mut files: Vec<FileSnap> = state
            .node_store
            .file_nodes
            .values()
            .map(|n| FileSnap {
                path: n.path.clone(),
                x: (n.position.x * 2.0).round() as i32,
                y: (n.position.y * 2.0).round() as i32,
                collapsed: n.collapsed,
            })
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let mut folders: Vec<(PathBuf, bool)> =
            state.node_store.folder_nodes.values().map(|f| (f.path.clone(), f.collapsed)).collect();
        folders.sort();
        Self { files, folders }
    }

    /// Puts the canvas back: closes files that were not open, opens the ones
    /// that were, restores positions and folder collapse.
    pub fn apply(&self, state: &mut AppState) {
        let wanted: std::collections::HashSet<&PathBuf> = self.files.iter().map(|f| &f.path).collect();
        let extra: Vec<u64> =
            state.node_store.file_nodes.values().filter(|n| !wanted.contains(&n.path)).map(|n| n.id).collect();
        crate::workspace::remove_nodes(state, &extra);
        let open: std::collections::HashSet<PathBuf> =
            state.node_store.file_nodes.values().map(|n| n.path.clone()).collect();
        let missing: Vec<PathBuf> = self.files.iter().filter(|f| !open.contains(&f.path)).map(|f| f.path.clone()).collect();
        crate::workspace::open_files(state, &missing);
        for snap in &self.files {
            if let Some(node) = state.node_store.file_nodes.values_mut().find(|n| n.path == snap.path) {
                node.position = egui::pos2(snap.x as f32 / 2.0, snap.y as f32 / 2.0);
                node.collapsed = snap.collapsed;
            }
        }
        for (path, collapsed) in &self.folders {
            let id = state.node_store.folder_nodes.values().find(|f| &f.path == path).map(|f| f.id);
            if let Some(id) = id {
                crate::folder_actions::set_many(state, &[id], *collapsed);
            }
        }
        // The restored positions are the ones to keep: no auto layout.
        state.layout_dirty = false;
    }
}

/// Seconds a changed canvas must rest before the change is a step: a layout
/// that settles over several frames after opening files is one step.
pub const SETTLE_SECONDS: f64 = 0.3;

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The snapshot at the last quiet moment.
    committed: Option<Snapshot>,
    /// A changed snapshot and since when it has not changed again.
    pending: Option<(Snapshot, f64)>,
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// True while a change waits to settle (the app keeps repainting).
    pub fn is_settling(&self) -> bool {
        self.pending.is_some()
    }

    /// Watches the canvas. Call once a frame with the app time `now`;
    /// `dragging` while a pointer button is down so a drag is one step.
    pub fn observe(&mut self, current: Snapshot, dragging: bool, now: f64) {
        let Some(committed) = &self.committed else {
            self.committed = Some(current);
            return;
        };
        if *committed == current {
            self.pending = None;
            return;
        }
        match &mut self.pending {
            Some((pending, since)) if *pending == current => {
                if !dragging && now - *since >= SETTLE_SECONDS {
                    let committed = self.committed.take().expect("checked above");
                    self.undo.push(committed);
                    if self.undo.len() > MAX_STEPS {
                        self.undo.remove(0);
                    }
                    self.redo.clear();
                    self.committed = Some(current);
                    self.pending = None;
                }
            }
            _ => self.pending = Some((current, now)),
        }
    }
}

/// `canvas:undo`: the canvas as it was before the last change.
pub fn undo(state: &mut AppState) -> bool {
    let mut history = std::mem::take(&mut state.history);
    let Some(previous) = history.undo.pop() else {
        state.history = history;
        return false;
    };
    if let Some(now) = history.committed.take() {
        history.redo.push(now);
    }
    previous.apply(state);
    history.pending = None;
    history.committed = Some(Snapshot::capture(state));
    state.history = history;
    true
}

/// `canvas:redo`: what the last undo took back.
pub fn redo(state: &mut AppState) -> bool {
    let mut history = std::mem::take(&mut state.history);
    let Some(next) = history.redo.pop() else {
        state.history = history;
        return false;
    };
    if let Some(now) = history.committed.take() {
        history.undo.push(now);
    }
    next.apply(state);
    history.pending = None;
    history.committed = Some(Snapshot::capture(state));
    state.history = history;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(files: &[(&str, i32)]) -> Snapshot {
        Snapshot {
            files: files.iter().map(|(p, x)| FileSnap { path: (*p).into(), x: *x, y: 0, collapsed: false }).collect(),
            folders: Vec::new(),
        }
    }

    #[test]
    fn a_change_becomes_a_step_once_it_settles_and_a_drag_is_one_step() {
        let mut h = History::default();
        h.observe(snap(&[("a", 0)]), false, 0.0);
        assert!(!h.can_undo(), "the first snapshot is a start, not a step");
        // A drag: the position changes every frame while the button is down.
        for (i, x) in [10, 20, 30].into_iter().enumerate() {
            h.observe(snap(&[("a", x)]), true, 1.0 + i as f64 * 0.016);
        }
        h.observe(snap(&[("a", 30)]), false, 1.1);
        assert!(!h.can_undo() && h.is_settling(), "not yet: it has not rested");
        h.observe(snap(&[("a", 30)]), false, 1.1 + SETTLE_SECONDS + 0.01);
        assert_eq!(h.undo, vec![snap(&[("a", 0)])], "the whole drag is one step");
        assert!(!h.is_settling());
        h.observe(snap(&[("a", 30)]), false, 5.0);
        assert_eq!(h.undo.len(), 1, "no change, no step");
    }

    #[test]
    fn changes_that_keep_coming_are_one_step() {
        let mut h = History::default();
        h.observe(snap(&[]), false, 0.0);
        // Files open, then an auto layout moves them a few frames later.
        h.observe(snap(&[("a", 0)]), false, 1.0);
        h.observe(snap(&[("a", 50)]), false, 1.1);
        h.observe(snap(&[("a", 50)]), false, 1.1 + SETTLE_SECONDS + 0.01);
        assert_eq!(h.undo, vec![snap(&[])], "one step back to the empty canvas");
    }

    #[test]
    fn a_new_change_clears_redo_and_the_stack_is_bounded() {
        let mut h = History::default();
        h.observe(snap(&[]), false, 0.0);
        let mut t = 0.0;
        for i in 0..(MAX_STEPS as i32 + 20) {
            t += 1.0;
            h.observe(snap(&[("a", i + 1)]), false, t);
            h.observe(snap(&[("a", i + 1)]), false, t + SETTLE_SECONDS + 0.01);
        }
        assert_eq!(h.undo.len(), MAX_STEPS);
        h.redo.push(snap(&[("x", 1)]));
        t += 1.0;
        h.observe(snap(&[("b", 1)]), false, t);
        h.observe(snap(&[("b", 1)]), false, t + SETTLE_SECONDS + 0.01);
        assert!(!h.can_redo());
    }
}
