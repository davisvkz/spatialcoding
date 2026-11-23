//! Read-only asynchronous comparison of the syntax-backed local graph.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use super::repo::{GitRepo, MAX_FILES, MAX_FILE_BYTES, MAX_TOTAL_BYTES};
use crate::analysis::dependencies::{local, DependencyLink, LinkEvidence, OpenFiles};
use crate::edges::edge_store::{EdgeChange, EdgeKind};
use crate::edges::style::RelationshipType;

/// A read-only canvas snapshot. Both sides cover the same set of open paths.
#[derive(Clone)]
pub struct GraphInput {
    pub root: PathBuf,
    pub files: BTreeMap<PathBuf, String>,
    pub(crate) configs: local::ConfigSources,
}

impl GraphInput {
    pub fn signature(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.root.hash(&mut hasher);
        self.files.hash(&mut hasher);
        self.configs.hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RelationshipKey {
    source: PathBuf,
    target: PathBuf,
    relationship: RelationshipType,
    /// An import through a barrel and the shortcut to its provider are two
    /// relationships, and neither is the same as a direct import.
    kind: EdgeKind,
}

impl RelationshipKey {
    fn of(root: &Path, link: &DependencyLink) -> Self {
        Self {
            source: link
                .source
                .strip_prefix(root)
                .unwrap_or(&link.source)
                .into(),
            target: link
                .target
                .strip_prefix(root)
                .unwrap_or(&link.target)
                .into(),
            relationship: link.relationship,
            kind: link.kind,
        }
    }
}

pub struct GraphDiff {
    pub commit: String,
    pub deleted: Vec<DependencyLink>,
    root: PathBuf,
    states: HashMap<RelationshipKey, EdgeChange>,
}

impl GraphDiff {
    fn between(
        root: &Path,
        baseline: &BTreeMap<PathBuf, String>,
        current: &BTreeMap<PathBuf, String>,
        configs: (&local::ConfigSources, &local::ConfigSources),
        commit: String,
    ) -> Self {
        let analyze = |files: &BTreeMap<PathBuf, String>, configs| {
            local::links_with_configs(
                root,
                &OpenFiles::new(files.iter().map(|(p, t)| (p.as_path(), t.as_str()))),
                configs,
            )
        };
        let before = analyze(baseline, configs.0);
        let after = analyze(current, configs.1);
        let previous: HashSet<_> = before
            .iter()
            .map(|l| RelationshipKey::of(root, l))
            .collect();
        let states: HashMap<_, _> = after
            .iter()
            .map(|l| {
                let key = RelationshipKey::of(root, l);
                let change = if previous.contains(&key) {
                    EdgeChange::Unchanged
                } else {
                    EdgeChange::Added
                };
                (key, change)
            })
            .collect();
        let mut removed = HashSet::new();
        let deleted = before
            .into_iter()
            .filter(|l| {
                let key = RelationshipKey::of(root, l);
                !states.contains_key(&key) && removed.insert(key)
            })
            .collect();
        Self {
            commit,
            root: root.into(),
            states,
            deleted,
        }
    }

    pub fn state_for(&self, link: &DependencyLink) -> EdgeChange {
        if link.evidence != LinkEvidence::LocalSyntax {
            return EdgeChange::Uncompared;
        }
        self.states
            .get(&RelationshipKey::of(&self.root, link))
            .copied()
            .unwrap_or_default()
    }

    /// Unique file-level relationships (not volatile occurrence/edge IDs).
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.states
                .values()
                .filter(|s| **s == EdgeChange::Added)
                .count(),
            self.deleted.len(),
            self.states
                .values()
                .filter(|s| **s == EdgeChange::Unchanged)
                .count(),
        )
    }
}

#[derive(Debug)]
pub enum ComparisonError {
    Unavailable(String),
    Error(String),
}

pub fn compare(input: &GraphInput, baseline: &str) -> Result<GraphDiff, ComparisonError> {
    let Some(repo) = GitRepo::open(&input.root) else {
        return Err(ComparisonError::Unavailable(
            "Git repository unavailable (or Git is not installed).".into(),
        ));
    };
    if input.files.len() > MAX_FILES
        || input.files.values().any(|t| t.len() > MAX_FILE_BYTES)
        || input.files.values().map(String::len).sum::<usize>() > MAX_TOTAL_BYTES
    {
        return Err(ComparisonError::Error(
            "Comparison limits: 512 files, 2 MiB/file, 32 MiB total.".into(),
        ));
    }
    // The configs the current side read, too: what they extend and the
    // packages of the folder have to be read from the baseline as well.
    let mut config_paths = local::config_paths(&input.root, input.files.keys().map(PathBuf::as_path));
    config_paths.extend(input.configs.keys().cloned());
    let paths: Vec<_> = input
        .files
        .keys()
        .chain(&config_paths)
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let (commit, mut files) = repo
        .baseline(baseline, &paths)
        .map_err(ComparisonError::Error)?;
    let configs = config_paths
        .into_iter()
        .filter_map(|path| files.get(&path).map(|text| (path, text.clone())))
        .collect();
    files.retain(|path, _| input.files.contains_key(path));
    Ok(GraphDiff::between(
        &input.root,
        &files,
        &input.files,
        (&configs, &input.configs),
        commit,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComparisonStatus {
    Idle,
    Loading,
    Ready,
    Unavailable(String),
    Error(String),
}

pub struct GraphComparison {
    pub baseline: String,
    pub status: ComparisonStatus,
    pub diff: Option<GraphDiff>,
    pending: Option<Receiver<Result<GraphDiff, ComparisonError>>>,
    pub requested_signature: Option<u64>,
}

impl Default for GraphComparison {
    fn default() -> Self {
        Self {
            baseline: "HEAD".into(),
            status: ComparisonStatus::Idle,
            diff: None,
            pending: None,
            requested_signature: None,
        }
    }
}

impl GraphComparison {
    pub fn is_ready(&self) -> bool {
        self.status == ComparisonStatus::Ready && self.diff.is_some()
    }
    pub fn is_loading(&self) -> bool {
        self.status == ComparisonStatus::Loading
    }

    /// Replacing the receiver is the generation boundary: an old worker can
    /// never install a result after a file set, revision or workspace change.
    pub fn invalidate(&mut self) {
        self.pending = None;
        self.diff = None;
        self.requested_signature = None;
        self.status = ComparisonStatus::Idle;
    }

    pub fn request(&mut self, input: GraphInput) {
        self.invalidate();
        self.requested_signature = Some(input.signature());
        self.status = ComparisonStatus::Loading;
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        let baseline = self.baseline.clone();
        std::thread::spawn(move || {
            let _ = sender.send(compare(&input, &baseline));
        });
    }

    /// Nonblocking; called by the UI loop. Disconnected workers become visible
    /// errors instead of leaving the controls in a permanent loading state.
    pub fn poll(&mut self) -> bool {
        let Some(receiver) = &self.pending else {
            return false;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                Err(ComparisonError::Error("Comparison worker stopped.".into()))
            }
        };
        self.pending = None;
        match result {
            Ok(diff) => {
                self.diff = Some(diff);
                self.status = ComparisonStatus::Ready;
            }
            Err(ComparisonError::Unavailable(message)) => {
                self.status = ComparisonStatus::Unavailable(message)
            }
            Err(ComparisonError::Error(message)) => self.status = ComparisonStatus::Error(message),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(root: &Path, args: &[&str]) {
        assert!(Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }

    #[test]
    fn closed_tsconfig_is_read_from_each_revision() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["init", "-q"]);
        let config = |target: &str| {
            format!(r#"{{"compilerOptions":{{"paths":{{"@value":["./{target}"]}}}}}}"#)
        };
        let tsconfig = root.join("tsconfig.json");
        std::fs::write(&tsconfig, config("a")).unwrap();
        let files: BTreeMap<_, _> = [
            ("app.ts", "import '@value';"),
            ("a.ts", "export {};"),
            ("b.ts", "export {};"),
        ]
        .into_iter()
        .map(|(path, text)| (root.join(path), text.to_owned()))
        .collect();
        for (path, text) in &files {
            std::fs::write(path, text).unwrap();
        }
        git(&root, &["add", "."]);
        git(
            &root,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@e",
                "commit",
                "-qm",
                "initial",
            ],
        );
        std::fs::write(&tsconfig, config("b")).unwrap();
        let configs = local::working_configs(
            &root,
            &OpenFiles::new(files.iter().map(|(p, t)| (p.as_path(), t.as_str()))),
        );
        let input = GraphInput {
            root: root.clone(),
            files,
            configs,
        };
        let diff = compare(&input, "HEAD").unwrap();
        assert_eq!(diff.counts(), (1, 1, 0));
        assert_eq!(diff.deleted[0].target, root.join("a.ts"));
    }

    /// The alias lives in a config the tsconfig extends: the baseline has to
    /// read that file from the revision too, not only the tsconfig.
    #[test]
    fn an_extended_config_is_read_from_each_revision() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["init", "-q"]);
        let base = |target: &str| format!(r#"{{"compilerOptions":{{"paths":{{"@value":["./{target}"]}}}}}}"#);
        let base_path = root.join("tsconfig.base.json");
        std::fs::write(&base_path, base("a")).unwrap();
        std::fs::write(root.join("tsconfig.json"), r#"{"extends": "./tsconfig.base"}"#).unwrap();
        let files: BTreeMap<_, _> = [("app.ts", "import '@value';"), ("a.ts", "export {};"), ("b.ts", "export {};")]
            .into_iter()
            .map(|(path, text)| (root.join(path), text.to_owned()))
            .collect();
        for (path, text) in &files {
            std::fs::write(path, text).unwrap();
        }
        git(&root, &["add", "."]);
        git(&root, &["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-qm", "initial"]);
        std::fs::write(&base_path, base("b")).unwrap();
        let configs = local::working_configs(
            &root,
            &OpenFiles::new(files.iter().map(|(p, t)| (p.as_path(), t.as_str()))),
        );
        assert!(configs.contains_key(&base_path), "the extended file is read from disk: {:?}", configs.keys());
        let input = GraphInput { root: root.clone(), files, configs };
        let diff = compare(&input, "HEAD").unwrap();
        assert_eq!(diff.counts(), (1, 1, 0));
        assert_eq!(diff.deleted[0].target, root.join("a.ts"));
    }

    #[test]
    fn baseline_is_a_real_commit_and_ignores_the_index_without_modifying_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        std::fs::write(root.join("app.ts"), "import './a';\n").unwrap();
        for file in ["a.ts", "b.ts", "c.ts"] {
            std::fs::write(root.join(file), "export {};\n").unwrap();
        }
        git(root, &["add", "."]);
        git(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "baseline",
            ],
        );
        // A staged addition and an unstaged removal/addition coexist.
        std::fs::write(root.join("app.ts"), "import './a';\nimport './b';\n").unwrap();
        git(root, &["add", "app.ts"]);
        let current = "import './b';\nimport './c';\n";
        std::fs::write(root.join("app.ts"), current).unwrap();
        let index_before = std::fs::read(root.join(".git/index")).unwrap();
        let input = GraphInput {
            root: root.into(),
            configs: Default::default(),
            files: [
                (root.join("app.ts"), current.into()),
                (root.join("a.ts"), "export {};\n".into()),
                (root.join("b.ts"), "export {};\n".into()),
                (root.join("c.ts"), "export {};\n".into()),
            ]
            .into_iter()
            .collect(),
        };
        let result = compare(&input, "HEAD").unwrap();
        assert_eq!(result.counts(), (2, 1, 0));
        assert_eq!(result.deleted[0].target, root.join("a.ts"));
        assert_eq!(
            std::fs::read(root.join(".git/index")).unwrap(),
            index_before
        );
        assert_eq!(
            std::fs::read_to_string(root.join("app.ts")).unwrap(),
            current
        );
        assert!(compare(&input, "--bad-option").is_err());
    }

    #[test]
    fn moving_lines_or_reordering_imports_does_not_change_relationship_identity() {
        let root = Path::new("/p");
        let before: BTreeMap<PathBuf, String> = [
            (root.join("app.ts"), "import './a';\nimport './b';\n".into()),
            (root.join("a.ts"), "".into()),
            (root.join("b.ts"), "".into()),
        ]
        .into_iter()
        .collect();
        let mut after = before.clone();
        after.insert(
            root.join("app.ts"),
            "// moved\n\nimport './b';\nimport './a';\n".into(),
        );
        let diff = GraphDiff::between(
            root,
            &before,
            &after,
            (&Default::default(), &Default::default()),
            "commit".into(),
        );
        assert_eq!(diff.counts(), (0, 0, 2));
        let mut lsp = local::links(
            root,
            &OpenFiles::new(after.iter().map(|(p, t)| (p.as_path(), t.as_str()))),
        )
        .remove(0);
        lsp.evidence = LinkEvidence::LanguageServer;
        assert_eq!(
            diff.state_for(&lsp),
            EdgeChange::Uncompared,
            "matching file pairs do not invent historical LSP coverage"
        );
    }

    #[test]
    fn removed_imports_through_a_barrel_keep_the_kind_they_had_in_the_baseline() {
        let root = Path::new("/p");
        let before: BTreeMap<PathBuf, String> = [
            ("app.ts", "import { A } from './index';\n"),
            ("index.ts", "export * from './a';\n"),
            ("a.ts", "export const A = 1;\n"),
        ]
        .into_iter()
        .map(|(path, text)| (root.join(path), text.to_owned()))
        .collect();
        let mut after = before.clone();
        after.insert(root.join("app.ts"), "// no import any more\n".into());
        let diff = GraphDiff::between(
            root,
            &before,
            &after,
            (&Default::default(), &Default::default()),
            "baseline".into(),
        );
        let mut deleted: Vec<_> = diff
            .deleted
            .iter()
            .map(|l| (l.source.file_name().unwrap().to_str().unwrap(), l.target.file_name().unwrap().to_str().unwrap(), l.kind))
            .collect();
        deleted.sort_by_key(|(source, target, _)| (*source, *target));
        assert_eq!(
            deleted,
            [("app.ts", "a.ts", EdgeKind::ReExport), ("app.ts", "index.ts", EdgeKind::Barrel)],
            "the hop and the shortcut are two relationships; the barrel's own export is unchanged"
        );
        assert_eq!(diff.counts(), (0, 2, 1), "(added, removed, same): the hop and the shortcut went, index.ts to a.ts stayed");
    }

    /// The same import compared with itself: neither the hop nor the shortcut
    /// is mistaken for a direct import of the same file.
    #[test]
    fn a_hop_and_a_direct_import_of_the_same_file_are_different_relationships() {
        let root = Path::new("/p");
        let files = |app: &str| -> BTreeMap<PathBuf, String> {
            [("app.ts", app), ("index.ts", "export { A } from './a';\n"), ("a.ts", "export const A = 1;\n")]
                .into_iter()
                .map(|(path, text)| (root.join(path), text.to_owned()))
                .collect()
        };
        // Before: A through the barrel. After: A straight from a.ts.
        let diff = GraphDiff::between(
            root,
            &files("import { A } from './index';\n"),
            &files("import { A } from './a';\n"),
            (&Default::default(), &Default::default()),
            "baseline".into(),
        );
        let (added, deleted, same) = diff.counts();
        assert_eq!((added, deleted), (1, 2), "the direct import is new; the hop and the shortcut are gone");
        assert_eq!(same, 1, "index.ts exporting from a.ts did not change");
    }

    #[test]
    fn refresh_results_are_discarded_after_invalidation() {
        let mut state = GraphComparison::default();
        let (tx, rx) = std::sync::mpsc::channel();
        state.pending = Some(rx);
        state.status = ComparisonStatus::Loading;
        state.invalidate();
        assert!(tx
            .send(Err(ComparisonError::Error("stale".into())))
            .is_err());
        assert!(!state.poll());
        assert_eq!(state.status, ComparisonStatus::Idle);
        assert!(!state.is_ready());
    }

    #[test]
    fn no_repository_and_unborn_head_are_visible_failures_not_empty_diffs() {
        let dir = tempfile::tempdir().unwrap();
        let input = GraphInput {
            configs: Default::default(),
            root: dir.path().into(),
            files: Default::default(),
        };
        assert!(matches!(
            compare(&input, "HEAD"),
            Err(ComparisonError::Unavailable(_))
        ));
        git(dir.path(), &["init", "-q"]);
        assert!(matches!(
            compare(&input, "HEAD"),
            Err(ComparisonError::Error(_))
        ));
    }
}
