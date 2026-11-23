//! The per-repository ignore list (`codecanvas/funcionalidades.md`, Ignored
//! paths), in gitignore syntax:
//!
//! - `name` matches a file or folder of that name at any depth;
//! - a pattern with a `/` inside, or a leading `/`, is anchored to the root;
//! - a trailing `/` matches only folders;
//! - `*` and `?` stay inside one name, `**` crosses folders;
//! - `!pattern` brings back what an earlier pattern ignored;
//! - a `#` line is a comment.
//!
//! Ignoring a folder takes everything below it out. Ignoring a *configuration
//! file* (`package.json`, `Cargo.toml`, `tsconfig.json`…) only hides it from
//! the tree: it keeps informing the analysis ([`is_config_file`]).
//!
//! A set made with [`IgnoreSet::new`] also hides, below the user's list:
//!
//! 1. the built-in [`DEFAULT_PATTERNS`] (build output, vendored code and
//!    lockfiles), then
//! 2. every `.gitignore` found on the way down, each scoped to its own folder.
//!
//! The last matching rule wins across the three layers, so the user's `!build`
//! brings back a folder the defaults hide. [`IgnoreSet::default`] hides nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use regex::Regex;

/// Names hidden by default, as the original Code Canvas does: build output,
/// vendored code and lockfiles. Gitignore syntax, like the user's list.
pub const DEFAULT_PATTERNS: &[&str] = &[
    "vendor", "dist", "build", ".next", "out", ".turbo", ".cache", "coverage", "storybook-static",
    "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "bun.lock", "bun.lockb", "Cargo.lock",
    "Gemfile.lock", "composer.lock", "poetry.lock", "uv.lock", "Pipfile.lock", "go.sum",
];

/// A `.gitignore` larger than this is not read.
const MAX_GITIGNORE_BYTES: u64 = 1 << 20;

static DEFAULT_RULES: LazyLock<Vec<Rule>> =
    LazyLock::new(|| DEFAULT_PATTERNS.iter().filter_map(|p| Rule::parse(p, "")).collect());

/// One compiled pattern.
#[derive(Clone)]
struct Rule {
    negated: bool,
    dir_only: bool,
    /// Matches the path itself.
    itself: Regex,
    /// Matches a path below a folder that matches.
    below: Regex,
}

/// The patterns of one repository.
#[derive(Default)]
pub struct IgnoreSet {
    rules: Vec<Rule>,
    patterns: Vec<String>,
    /// Hide the defaults and the `.gitignore` files too.
    builtin: bool,
}

/// The rules of the `.gitignore` files above a folder, shallowest first.
/// Cloning shares the rules.
#[derive(Clone, Default)]
pub struct GitignoreRules {
    rules: Arc<Vec<Rule>>,
}

impl GitignoreRules {
    /// These rules followed by the `.gitignore` of `dir`, when it has one
    /// (`root` is the repository root the patterns are matched against).
    pub fn below(&self, dir: &Path, root: &Path) -> Self {
        let Some(text) = read_gitignore(dir) else { return self.clone() };
        let base = relative(dir, root).unwrap_or_default();
        let own: Vec<Rule> = text.lines().filter_map(|line| Rule::parse(line, &base)).collect();
        if own.is_empty() {
            return self.clone();
        }
        let mut rules = (*self.rules).clone();
        rules.extend(own);
        Self { rules: Arc::new(rules) }
    }
}

/// `dir/.gitignore` when it is a regular file of a sane size.
fn read_gitignore(dir: &Path) -> Option<String> {
    let path = dir.join(".gitignore");
    let meta = std::fs::metadata(&path).ok()?;
    (meta.is_file() && meta.len() <= MAX_GITIGNORE_BYTES).then(|| std::fs::read_to_string(path).ok())?
}

/// `path` below `root`, `/` separated; `None` outside it.
fn relative(path: &Path, root: &Path) -> Option<String> {
    path.strip_prefix(root).ok().map(|rel| rel.to_string_lossy().replace('\\', "/"))
}

/// The `.gitignore` rules of every folder from the root down, read on demand
/// and kept for the watcher batch that asked.
#[derive(Default)]
pub struct GitignoreCache {
    by_dir: HashMap<PathBuf, GitignoreRules>,
}

impl GitignoreCache {
    fn rules_in(&mut self, dir: &Path, root: &Path) -> GitignoreRules {
        if let Some(rules) = self.by_dir.get(dir) {
            return rules.clone();
        }
        let above = match dir.parent() {
            Some(parent) if dir != root => self.rules_in(parent, root),
            _ => GitignoreRules::default(),
        };
        let rules = above.below(dir, root);
        self.by_dir.insert(dir.to_path_buf(), rules.clone());
        rules
    }
}

/// Configuration files that keep informing the analysis when ignored.
pub fn is_config_file(name: &str) -> bool {
    matches!(
        name,
        "package.json" | "tsconfig.json" | "jsconfig.json" | "Cargo.toml" | "go.mod" | "pyproject.toml"
            | "CMakeLists.txt" | "Package.swift" | "compile_commands.json" | "global.json"
    ) || name.ends_with(".csproj")
        || name.ends_with(".props")
        || name.ends_with(".targets")
}

fn translate(pattern: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                // `**/` matches zero or more folders; a trailing `/**` everything below.
                if chars.get(i + 2) == Some(&'/') {
                    out.push_str("(?:.*/)?");
                    i += 3;
                } else {
                    out.push_str(".*");
                    i += 2;
                }
            }
            '*' => {
                out.push_str("[^/]*");
                i += 1;
            }
            '?' => {
                out.push_str("[^/]");
                i += 1;
            }
            c => {
                out.push_str(&regex::escape(&c.to_string()));
                i += 1;
            }
        }
    }
    out
}

impl Rule {
    /// Compiles one line. `base` is the folder (relative to the repository
    /// root, `/` separated, empty for the root) whose list the line is from:
    /// the pattern only matches below it.
    fn parse(line: &str, base: &str) -> Option<Rule> {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (negated, line) = match line.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        let (dir_only, line) = match line.strip_suffix('/') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        let anchored = line.starts_with('/') || line.contains('/');
        let body = translate(line.trim_start_matches('/'));
        let scope = if base.is_empty() { String::new() } else { format!("{}/", regex::escape(base)) };
        let prefix = if anchored { format!("^{scope}") } else { format!("^{scope}(?:.*/)?") };
        let itself = Regex::new(&format!("{prefix}{body}$")).ok()?;
        let below = Regex::new(&format!("{prefix}{body}/")).ok()?;
        Some(Rule { negated, dir_only, itself, below })
    }
}

impl IgnoreSet {
    /// Compiles `patterns`; invalid lines are skipped. The set also applies
    /// the defaults and the `.gitignore` files (see the module docs).
    pub fn new(patterns: &[String]) -> Self {
        Self {
            rules: patterns.iter().filter_map(|p| Rule::parse(p, "")).collect(),
            patterns: patterns.to_vec(),
            builtin: true,
        }
    }

    /// Whether the user's list has no pattern.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Whether the set hides anything at all.
    pub fn filters(&self) -> bool {
        self.builtin || !self.rules.is_empty()
    }

    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// Whether the user's list ignores the path `relative` (from the
    /// repository root, `/` separated). The last matching pattern wins, as
    /// in gitignore.
    pub fn is_ignored(&self, relative: &str, is_dir: bool) -> bool {
        apply(&self.rules, relative, is_dir, false)
    }

    /// Whether the tree leaves `relative` out: the defaults, then `git` (the
    /// `.gitignore` files above it), then the user's list; the last
    /// matching rule wins.
    pub fn hides(&self, git: &GitignoreRules, relative: &str, is_dir: bool) -> bool {
        let mut ignored = false;
        if self.builtin {
            ignored = apply(&DEFAULT_RULES, relative, is_dir, ignored);
            ignored = apply(&git.rules, relative, is_dir, ignored);
        }
        apply(&self.rules, relative, is_dir, ignored)
    }

    /// [`Self::hides`] for the absolute `path` below `root`.
    pub fn hides_path(&self, git: &GitignoreRules, path: &Path, root: &Path, is_dir: bool) -> bool {
        relative(path, root).is_some_and(|rel| self.hides(git, &rel, is_dir))
    }

    /// [`Self::hides_path`] reading the `.gitignore` files above `path` from
    /// disk (through `cache`), for paths the loader did not walk to.
    pub fn hides_on_disk(&self, path: &Path, root: &Path, cache: &mut GitignoreCache) -> bool {
        if !self.filters() {
            return false;
        }
        let git = match path.parent() {
            Some(dir) if self.builtin => cache.rules_in(dir, root),
            _ => GitignoreRules::default(),
        };
        self.hides_path(&git, path, root, path.is_dir())
    }

    /// [`Self::is_ignored`] for an absolute `path` under `root`.
    pub fn is_ignored_path(&self, path: &Path, root: &Path, is_dir: bool) -> bool {
        match path.strip_prefix(root) {
            Ok(rel) => self.is_ignored(&rel.to_string_lossy().replace('\\', "/"), is_dir),
            Err(_) => false,
        }
    }
}

/// Runs `rules` over `relative` starting from the verdict `ignored`; the last
/// rule that matches decides.
fn apply(rules: &[Rule], relative: &str, is_dir: bool, mut ignored: bool) -> bool {
    for rule in rules {
        let hit = rule.below.is_match(relative) || ((!rule.dir_only || is_dir) && rule.itself.is_match(relative));
        if hit {
            ignored = !rule.negated;
        }
    }
    ignored
}

/// The pattern that ignores `path` (relative to `root`): anchored, with a
/// trailing `/` for a folder.
pub fn pattern_for(path: &Path, root: &Path, is_dir: bool) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
    (!rel.is_empty()).then(|| format!("/{rel}{}", if is_dir { "/" } else { "" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(patterns: &[&str]) -> IgnoreSet {
        IgnoreSet::new(&patterns.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn a_bare_name_matches_at_any_depth_and_takes_what_is_below() {
        let s = set(&["dist"]);
        assert!(s.is_ignored("dist", true));
        assert!(s.is_ignored("a/b/dist", true));
        assert!(s.is_ignored("dist/x.js", false), "below an ignored folder");
        assert!(!s.is_ignored("distance/x.js", false), "a name, not a prefix");
        assert!(!s.is_ignored("src/main.rs", false));
    }

    #[test]
    fn slashes_anchor_and_a_trailing_slash_means_folders_only() {
        let s = set(&["/docs", "src/gen/", "*.log"]);
        assert!(s.is_ignored("docs", true) && !s.is_ignored("a/docs", true), "a leading slash anchors");
        assert!(s.is_ignored("src/gen", true) && s.is_ignored("src/gen/a.rs", false));
        assert!(!s.is_ignored("src/gen", false), "gen as a file is not a folder");
        assert!(s.is_ignored("x/y/run.log", false) && !s.is_ignored("run.logger", false));
    }

    #[test]
    fn wildcards_stay_in_a_name_and_double_star_crosses_folders() {
        let s = set(&["a/*/c.txt", "**/gen/*.rs", "docs/**/*.md"]);
        assert!(s.is_ignored("a/x/c.txt", false) && !s.is_ignored("a/x/y/c.txt", false));
        assert!(s.is_ignored("gen/m.rs", false) && s.is_ignored("p/q/gen/m.rs", false));
        assert!(s.is_ignored("docs/a/b/x.md", false) && s.is_ignored("docs/x.md", false));
        assert!(set(&["file?.txt"]).is_ignored("file1.txt", false));
    }

    #[test]
    fn negation_and_comments_follow_gitignore_order() {
        let s = set(&["# build", "*.js", "!keep.js"]);
        assert!(s.is_ignored("a.js", false));
        assert!(!s.is_ignored("keep.js", false), "the last matching pattern wins");
        assert!(set(&["", "   "]).is_empty());
    }

    #[test]
    fn patterns_for_paths_and_config_files() {
        let root = Path::new("/repo");
        assert_eq!(pattern_for(Path::new("/repo/src/gen"), root, true), Some("/src/gen/".into()));
        assert_eq!(pattern_for(Path::new("/repo/a.txt"), root, false), Some("/a.txt".into()));
        assert_eq!(pattern_for(Path::new("/other/a"), root, false), None);
        assert!(is_config_file("Cargo.toml") && is_config_file("app.csproj") && !is_config_file("main.rs"));
        let s = set(&["/src/gen/"]);
        assert!(s.is_ignored_path(Path::new("/repo/src/gen/a.rs"), root, false));
    }

    fn rules(set: &IgnoreSet, git: &GitignoreRules, rel: &str, is_dir: bool) -> bool {
        set.hides(git, rel, is_dir)
    }

    #[test]
    fn defaults_hide_build_output_and_lockfiles_until_the_user_brings_them_back() {
        let none = GitignoreRules::default();
        let plain = IgnoreSet::new(&[]);
        for hidden in ["dist", "a/vendor", "build/x.js", "package-lock.json", "web/yarn.lock", "coverage"] {
            assert!(rules(&plain, &none, hidden, !hidden.contains('.')), "{hidden} is hidden by default");
        }
        assert!(!rules(&plain, &none, "src/main.rs", false) && !rules(&plain, &none, "distance", true));
        assert!(!rules(&IgnoreSet::default(), &none, "dist", true), "the empty set hides nothing");
        assert!(!IgnoreSet::default().filters() && IgnoreSet::new(&[]).filters());

        let restored = IgnoreSet::new(&["!build".to_string()]);
        assert!(!rules(&restored, &none, "build", true), "the user's list is the last word");
        assert!(rules(&restored, &none, "dist", true));
    }

    #[test]
    fn a_nested_gitignore_only_reaches_its_own_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::write(root.join("src/.gitignore"), "gen\n/local.rs\n*.tmp\n!keep.tmp\n**/snap/*.txt\n").unwrap();

        let git = GitignoreRules::default().below(root, root).below(&root.join("src"), root);
        let set = IgnoreSet::new(&[]);
        assert!(set.hides(&git, "src/gen", true) && set.hides(&git, "src/deep/gen/a.rs", false), "unanchored: any depth below src");
        assert!(set.hides(&git, "src/local.rs", false) && !set.hides(&git, "src/deep/local.rs", false), "anchored to src");
        assert!(!set.hides(&git, "gen", true) && !set.hides(&git, "other/gen", true), "outside src it means nothing");
        assert!(set.hides(&git, "src/a.tmp", false) && !set.hides(&git, "src/keep.tmp", false), "negation inside the file");
        assert!(set.hides(&git, "src/snap/a.txt", false) && set.hides(&git, "src/x/y/snap/a.txt", false), "** crosses folders");
        assert!(!set.hides(&git, "snap/a.txt", false) && !set.hides(&git, "src/snap/a.rs", false), "but only below src and only *.txt");
    }

    #[test]
    fn the_user_list_overrides_a_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "secret.txt\n").unwrap();
        let git = GitignoreRules::default().below(dir.path(), dir.path());
        assert!(IgnoreSet::new(&[]).hides(&git, "secret.txt", false));
        assert!(!IgnoreSet::new(&["!secret.txt".to_string()]).hides(&git, "secret.txt", false));
    }

    #[test]
    fn oversized_or_non_regular_gitignores_are_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".gitignore"), "x".repeat(MAX_GITIGNORE_BYTES as usize + 1)).unwrap();
        assert!(read_gitignore(root).is_none());
        std::fs::remove_file(root.join(".gitignore")).unwrap();
        std::fs::create_dir(root.join(".gitignore")).unwrap();
        assert!(read_gitignore(root).is_none(), "a folder named .gitignore");
    }

    #[test]
    fn hides_on_disk_reads_the_gitignores_above_a_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(root.join("pkg/gen")).unwrap();
        std::fs::write(root.join("pkg/.gitignore"), "gen/\n").unwrap();
        std::fs::write(root.join("pkg/gen/a.rs"), "").unwrap();
        std::fs::write(root.join("pkg/b.rs"), "").unwrap();
        let set = IgnoreSet::new(&[]);
        let mut cache = GitignoreCache::default();
        assert!(set.hides_on_disk(&root.join("pkg/gen/a.rs"), &root, &mut cache));
        assert!(!set.hides_on_disk(&root.join("pkg/b.rs"), &root, &mut cache));
        assert!(!IgnoreSet::default().hides_on_disk(&root.join("pkg/gen/a.rs"), &root, &mut cache));
    }
}
