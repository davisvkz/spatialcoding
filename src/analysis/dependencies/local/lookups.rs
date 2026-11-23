//! What the analysis of one file asks the rest of the project, recorded.
//!
//! The links of a file depend on its own text, on the configuration, and on
//! which other files exist: whether `./x` is open, listed in the folder, or
//! absent, and which files a Go package holds. They never depend on the
//! contents of another file. So a file's result is still right as long as its
//! text and the configuration are the same and every question it asked has the
//! same answer. [`Lookups`] is the only way the analysis reads [`OpenFiles`],
//! so a question cannot go unrecorded; [`Lookups::replays`] asks them again.
//!
//! One thing is not asked again: whether an open file resolves outside the
//! folder (a symbolic link). That is the disk, which the cache forgets whenever
//! files appear or vanish (`ParseCache::forget_analyses`).

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::analysis::dependencies::OpenFiles;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Query {
    /// Is this path a file, and which one?
    Candidate(PathBuf),
    /// The files directly in a folder.
    Dir(PathBuf),
    /// The open go.mod at a path.
    Manifest(PathBuf),
    /// Whether the folder's files are known at all.
    HasIndex,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Answer {
    /// Open on the canvas, under this node path.
    Open(PathBuf),
    /// Listed in the folder, not open.
    Indexed,
    Absent,
    /// Sorted.
    Files(Vec<PathBuf>),
    /// The node path and a hash of the text of an open file.
    Text(Option<(PathBuf, u64)>),
    Flag(bool),
}

/// What a candidate path is, for the analysis.
pub(super) enum Presence {
    Open(PathBuf),
    Indexed,
    Absent,
}

pub(super) type Trace = HashMap<Query, Answer>;

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn answer(open: &OpenFiles<'_>, query: &Query) -> Answer {
    match query {
        Query::Candidate(path) => match open.get(path) {
            Some((actual, _)) => Answer::Open(actual.to_path_buf()),
            None if open.is_indexed(path) => Answer::Indexed,
            None => Answer::Absent,
        },
        Query::Dir(dir) => {
            let mut files = open.files_in_dir(dir);
            files.sort();
            Answer::Files(files)
        }
        Query::Manifest(path) => Answer::Text(open.get(path).map(|(node, text)| (node.to_path_buf(), hash_text(text)))),
        Query::HasIndex => Answer::Flag(open.has_index()),
    }
}

/// The open files and the folder's files, as one file's analysis sees them.
pub(super) struct Lookups<'a, 'o> {
    open: &'a OpenFiles<'o>,
    trace: RefCell<Trace>,
}

impl<'a, 'o> Lookups<'a, 'o> {
    pub fn new(open: &'a OpenFiles<'o>) -> Self {
        Self { open, trace: RefCell::default() }
    }

    fn ask(&self, query: Query) -> Answer {
        let found = answer(self.open, &query);
        self.trace.borrow_mut().insert(query, found.clone());
        found
    }

    pub fn candidate(&self, path: &Path) -> Presence {
        match self.ask(Query::Candidate(path.to_path_buf())) {
            Answer::Open(actual) => Presence::Open(actual),
            Answer::Indexed => Presence::Indexed,
            _ => Presence::Absent,
        }
    }

    /// The files directly in `dir`, open and listed, sorted.
    pub fn files_in_dir(&self, dir: &Path) -> Vec<PathBuf> {
        match self.ask(Query::Dir(dir.to_path_buf())) {
            Answer::Files(files) => files,
            _ => Vec::new(),
        }
    }

    /// The node path and text of the open file at `path`.
    pub fn open_file(&self, path: &Path) -> Option<(&'o Path, &'o str)> {
        self.ask(Query::Manifest(path.to_path_buf()));
        self.open.get(path)
    }

    pub fn has_index(&self) -> bool {
        matches!(self.ask(Query::HasIndex), Answer::Flag(true))
    }

    /// Whether every question of `trace` has the answer it had.
    pub fn replays(&self, trace: &Trace) -> bool {
        trace.iter().all(|(query, before)| answer(self.open, query) == *before)
    }

    pub fn into_trace(self) -> Trace {
        self.trace.into_inner()
    }
}
