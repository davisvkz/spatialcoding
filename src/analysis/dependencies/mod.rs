//! Static, per-language dependency analysis between source files.
//!
//! Each language module parses import-like statements and resolves them to
//! files in the analyzed file set. Only dependencies whose target is part of
//! that set are reported, so edges always connect two known file nodes.

mod go;
mod python;
mod rust;
mod typescript;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct Dependency {
    pub source: PathBuf,
    pub target: PathBuf,
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    /// The source uses items from the target (`use`, `import`, `require`).
    Import,
    /// The target uses items exported by the source.
    Export,
    /// The source re-exports items of the target (`pub use`, `export … from`).
    ReExport,
    /// The source declares the target as a child module (`mod x;`).
    Module,
}

impl DependencyKind {
    /// When the same source/target pair is found several times, the kind with
    /// the highest priority is kept.
    fn priority(self) -> u8 {
        match self {
            DependencyKind::Export => 0,
            DependencyKind::Import => 1,
            DependencyKind::Module => 2,
            DependencyKind::ReExport => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Rust,
    TypeScript,
    Python,
    Go,
}

impl Language {
    fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?;
        match ext {
            "rs" => Some(Language::Rust),
            "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => {
                Some(Language::TypeScript)
            }
            "py" | "pyi" => Some(Language::Python),
            "go" => Some(Language::Go),
            _ => None,
        }
    }
}

/// Analyze `files` (all belonging to the project at `root`) and return the
/// dependencies between them. Each `(source, target)` pair appears once.
pub fn analyze_dependencies(root: &Path, files: &[PathBuf]) -> Vec<Dependency> {
    let ctx = Ctx::new(root, files);
    let mut index: HashMap<(PathBuf, PathBuf), usize> = HashMap::new();
    let mut deps: Vec<Dependency> = Vec::new();

    for file in files {
        let Some(lang) = Language::from_path(file) else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(file) else {
            continue;
        };

        let found = match lang {
            Language::Rust => rust::analyze(&ctx, file, &content),
            Language::TypeScript => typescript::analyze(&ctx, file, &content),
            Language::Python => python::analyze(&ctx, file, &content),
            Language::Go => go::analyze(&ctx, file, &content),
        };

        for (target, kind) in found {
            if target == *file {
                continue;
            }
            let key = (file.clone(), target.clone());
            match index.get(&key) {
                Some(&i) => {
                    if kind.priority() > deps[i].kind.priority() {
                        deps[i].kind = kind;
                    }
                }
                None => {
                    index.insert(key, deps.len());
                    deps.push(Dependency {
                        source: file.clone(),
                        target,
                        kind,
                    });
                }
            }
        }
    }

    deps
}

/// Shared state for one analysis run: the file set plus caches for project
/// configuration files (Cargo.toml, tsconfig.json, go.mod, …).
pub(crate) struct Ctx {
    root: PathBuf,
    /// normalized path -> path as given by the caller
    files: HashMap<PathBuf, PathBuf>,
    /// normalized directory -> files directly inside it (caller paths)
    by_dir: HashMap<PathBuf, Vec<PathBuf>>,
    find_up_cache: RefCell<HashMap<(PathBuf, String), Option<PathBuf>>>,
    ts_configs: RefCell<HashMap<PathBuf, Option<Rc<typescript::TsConfig>>>>,
    go_modules: RefCell<HashMap<PathBuf, Option<String>>>,
}

impl Ctx {
    fn new(root: &Path, files: &[PathBuf]) -> Self {
        let mut map = HashMap::new();
        let mut by_dir: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        for f in files {
            let norm = normalize(f);
            if let Some(parent) = norm.parent() {
                by_dir.entry(parent.to_path_buf()).or_default().push(f.clone());
            }
            map.insert(norm, f.clone());
        }
        Self {
            root: root.to_path_buf(),
            files: map,
            by_dir,
            find_up_cache: RefCell::new(HashMap::new()),
            ts_configs: RefCell::new(HashMap::new()),
            go_modules: RefCell::new(HashMap::new()),
        }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    /// Return the caller's path for `candidate` if it is part of the file set.
    fn lookup(&self, candidate: &Path) -> Option<PathBuf> {
        self.files.get(&normalize(candidate)).cloned()
    }

    /// Files of the set located directly inside `dir`.
    fn files_in_dir(&self, dir: &Path) -> &[PathBuf] {
        self.by_dir
            .get(&normalize(dir))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Find the nearest ancestor of `start` (inclusive) that contains one of
    /// `names`, returning the path of that file. Results are cached.
    fn find_up(&self, start: &Path, names: &[&str]) -> Option<PathBuf> {
        let key = (start.to_path_buf(), names.join("|"));
        if let Some(hit) = self.find_up_cache.borrow().get(&key) {
            return hit.clone();
        }
        let found = start.ancestors().find_map(|dir| {
            names
                .iter()
                .map(|n| dir.join(n))
                .find(|candidate| candidate.is_file())
        });
        self.find_up_cache.borrow_mut().insert(key, found.clone());
        found
    }
}

/// Lexically normalize a path: drop `.` components and fold `..` into the
/// preceding normal component.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut out: Vec<Component> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir) | Some(Component::Prefix(_)) => {}
                _ => out.push(comp),
            },
            other => out.push(other),
        }
    }
    out.iter().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommentStyle {
    /// Nested block comments, `"…"` strings, raw strings and char literals.
    Rust,
    /// `'…'`, `"…"` and `` `…` `` strings (also used for Go and JSONC).
    CLike,
}

/// Remove `//` and `/* */` comments while keeping string literals intact.
/// Newlines inside block comments are preserved so line structure survives.
pub(crate) fn strip_comments(src: &str, style: CommentStyle) -> String {
    let b: Vec<char> = src.chars().collect();
    let len = b.len();
    let mut out = String::with_capacity(src.len());
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut i = 0;

    while i < len {
        let c = b[i];
        let next = b.get(i + 1).copied();

        // Line comment
        if c == '/' && next == Some('/') {
            while i < len && b[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Block comment
        if c == '/' && next == Some('*') {
            let mut depth = 1;
            i += 2;
            while i < len && depth > 0 {
                if b[i] == '*' && b.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                } else if style == CommentStyle::Rust && b[i] == '/' && b.get(i + 1) == Some(&'*')
                {
                    depth += 1;
                    i += 2;
                } else {
                    if b[i] == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
            }
            out.push(' ');
            continue;
        }

        if style == CommentStyle::Rust {
            // Raw strings: r"…", r#"…"#, br"…"
            let prev = if i > 0 { Some(b[i - 1]) } else { None };
            let prev_ok = match prev {
                None => true,
                Some('b') => i < 2 || !is_ident(b[i - 2]),
                Some(p) => !is_ident(p),
            };
            if c == 'r' && prev_ok && matches!(next, Some('"') | Some('#')) {
                let mut j = i + 1;
                let mut hashes = 0;
                while j < len && b[j] == '#' {
                    hashes += 1;
                    j += 1;
                }
                if j < len && b[j] == '"' {
                    let start = i;
                    j += 1;
                    loop {
                        if j >= len {
                            break;
                        }
                        if b[j] == '"' && (0..hashes).all(|h| b.get(j + 1 + h) == Some(&'#')) {
                            j += 1 + hashes;
                            break;
                        }
                        j += 1;
                    }
                    out.extend(&b[start..j.min(len)]);
                    i = j;
                    continue;
                }
            }

            // Char literals ('x', '\n', '\u{1F600}'); lifetimes pass through.
            if c == '\'' {
                if next == Some('\\') {
                    let start = i;
                    let mut j = i + 3;
                    while j < len && b[j] != '\'' && b[j] != '\n' {
                        j += 1;
                    }
                    let end = if j < len && b[j] == '\'' { j + 1 } else { j };
                    out.extend(&b[start..end.min(len)]);
                    i = end;
                    continue;
                }
                if b.get(i + 2) == Some(&'\'') {
                    out.extend(&b[i..i + 3]);
                    i += 3;
                    continue;
                }
            }
        }

        let is_quote = c == '"' || (style == CommentStyle::CLike && (c == '\'' || c == '`'));
        if is_quote {
            out.push(c);
            i += 1;
            while i < len {
                let d = b[i];
                out.push(d);
                i += 1;
                if d == '\\' {
                    if i < len {
                        out.push(b[i]);
                        i += 1;
                    }
                    continue;
                }
                if d == c {
                    break;
                }
            }
            continue;
        }

        out.push(c);
        i += 1;
    }

    out
}

#[cfg(test)]
pub(crate) mod test_util {
    use super::*;

    /// Create a temporary project with the given `(relative path, content)`
    /// files. Returns the temp dir guard and the absolute paths of all files.
    pub fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, Vec<PathBuf>) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut paths = Vec::new();
        for (rel, content) in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
            paths.push(path);
        }
        (dir, paths)
    }

    /// Run the analysis and return edges as sorted `(source, target, kind)`
    /// triples with paths relative to `root`.
    pub fn edges(root: &Path, files: &[PathBuf]) -> Vec<(String, String, DependencyKind)> {
        let rel = |p: &Path| {
            p.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        };
        let mut out: Vec<_> = analyze_dependencies(root, files)
            .into_iter()
            .map(|d| (rel(&d.source), rel(&d.target), d.kind))
            .collect();
        out.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        out
    }

    pub fn edge(src: &str, tgt: &str, kind: DependencyKind) -> (String, String, DependencyKind) {
        (src.to_string(), tgt.to_string(), kind)
    }
}

#[cfg(test)]
mod tests {
    use super::test_util::*;
    use super::*;

    #[test]
    fn normalize_folds_dot_segments() {
        assert_eq!(normalize(Path::new("/a/b/./../c/d")), PathBuf::from("/a/c/d"));
        assert_eq!(normalize(Path::new("a/../../b")), PathBuf::from("../b"));
    }

    #[test]
    fn strip_comments_keeps_strings() {
        let src = "let a = \"// not a comment\"; // gone\n/* block\n */ let b = 1;";
        let out = strip_comments(src, CommentStyle::Rust);
        assert!(out.contains("\"// not a comment\""));
        assert!(!out.contains("gone"));
        assert!(!out.contains("block"));
        assert_eq!(out.lines().count(), 3);
    }

    #[test]
    fn strip_comments_rust_char_and_lifetime() {
        let src = "fn f<'a>(x: &'a str) -> char { '\"' } // c\nuse crate::x;";
        let out = strip_comments(src, CommentStyle::Rust);
        assert!(out.contains("use crate::x;"));
        assert!(!out.contains("// c"));
    }

    #[test]
    fn dedups_pairs_and_ignores_files_outside_set() {
        let (dir, files) = project(&[
            ("Cargo.toml", "[package]\nname = \"x\"\n"),
            (
                "src/main.rs",
                "mod a;\nmod missing;\nuse crate::a::Thing;\nuse a::other;\nfn main() { crate::a::f(); }\n",
            ),
            ("src/a.rs", "pub fn f() {}\n"),
            ("src/unlisted.rs", ""),
        ]);
        // `unlisted.rs` exists on disk but is not part of the analyzed set.
        let set: Vec<PathBuf> = files
            .into_iter()
            .filter(|p| !p.ends_with("unlisted.rs"))
            .collect();
        let edges = edges(dir.path(), &set);
        assert_eq!(edges, vec![edge("src/main.rs", "src/a.rs", DependencyKind::Module)]);
    }
}
