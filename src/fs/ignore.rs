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

use std::path::Path;

use regex::Regex;

/// One compiled pattern.
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
    fn parse(line: &str) -> Option<Rule> {
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
        let prefix = if anchored { "^" } else { "^(?:.*/)?" };
        let itself = Regex::new(&format!("{prefix}{body}$")).ok()?;
        let below = Regex::new(&format!("{prefix}{body}/")).ok()?;
        Some(Rule { negated, dir_only, itself, below })
    }
}

impl IgnoreSet {
    /// Compiles `patterns`; invalid lines are skipped.
    pub fn new(patterns: &[String]) -> Self {
        Self {
            rules: patterns.iter().filter_map(|p| Rule::parse(p)).collect(),
            patterns: patterns.to_vec(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// Whether the path `relative` (from the repository root, `/` separated)
    /// is ignored. The last matching pattern wins, as in gitignore.
    pub fn is_ignored(&self, relative: &str, is_dir: bool) -> bool {
        let mut ignored = false;
        for rule in &self.rules {
            let hit = (rule.below.is_match(relative)) || ((!rule.dir_only || is_dir) && rule.itself.is_match(relative));
            if hit {
                ignored = !rule.negated;
            }
        }
        ignored
    }

    /// [`Self::is_ignored`] for an absolute `path` under `root`.
    pub fn is_ignored_path(&self, path: &Path, root: &Path, is_dir: bool) -> bool {
        match path.strip_prefix(root) {
            Ok(rel) => self.is_ignored(&rel.to_string_lossy().replace('\\', "/"), is_dir),
            Err(_) => false,
        }
    }
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
}
