//! Rust: `mod x;` declarations, `use` trees and inline `crate::`/`self::`/
//! `super::` paths, resolved through the module tree of the enclosing crate.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::{CommentStyle, Ctx, DependencyKind, strip_comments};

static MOD_DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"((?:#\s*\[[^\]]*\]\s*)*)(?:\bpub(?:\s*\([^)]*\))?\s+)?\bmod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;"#,
    )
    .unwrap()
});
static PATH_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"#\s*\[\s*path\s*=\s*"([^"]+)"\s*\]"#).unwrap());
static USE_DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(pub(?:\s*\([^)]*\))?\s+)?use\s+([^;]+);").unwrap()
});
static INLINE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:crate|self|super)(?:::[A-Za-z_][A-Za-z0-9_]*)+").unwrap()
});
static AS_ALIAS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+as\s+[A-Za-z_][A-Za-z0-9_]*").unwrap());

pub(super) fn analyze(ctx: &Ctx, file: &Path, content: &str) -> Vec<(PathBuf, DependencyKind)> {
    let code = strip_comments(content, CommentStyle::Rust);
    let module_dir = module_dir(file);
    let crate_src = crate_src_dir(ctx, file);
    let mut out = Vec::new();

    // Child modules: `mod name;` (optionally with `#[path = "…"]`).
    let mut child_mods: HashSet<String> = HashSet::new();
    for cap in MOD_DECL.captures_iter(&code) {
        let name = cap[2].to_string();
        let attrs = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let target = match PATH_ATTR.captures(attrs) {
            Some(p) => file.parent().and_then(|dir| ctx.lookup(&dir.join(&p[1]))),
            None => module_file(ctx, &module_dir.join(&name)),
        };
        if let Some(t) = target {
            out.push((t, DependencyKind::Module));
        }
        child_mods.insert(name);
    }

    let resolver = Resolver {
        ctx,
        module_dir: &module_dir,
        crate_src: &crate_src,
        child_mods: &child_mods,
    };

    // `use` declarations.
    for cap in USE_DECL.captures_iter(&code) {
        let kind = if cap.get(1).is_some() {
            DependencyKind::ReExport
        } else {
            DependencyKind::Import
        };
        for path in expand_use_tree(&cap[2]) {
            if let Some(t) = resolver.resolve(&path) {
                out.push((t, kind));
            }
        }
    }

    // Fully qualified paths used inline, e.g. `crate::fs::loader::read(..)`.
    // `use` declarations are removed first: their prefixes (`crate::a::{…}`)
    // would otherwise resolve to the parent module.
    let body = USE_DECL.replace_all(&code, "");
    for m in INLINE_PATH.find_iter(&body) {
        let segs: Vec<String> = m.as_str().split("::").map(str::to_string).collect();
        if let Some(t) = resolver.resolve(&segs) {
            out.push((t, DependencyKind::Import));
        }
    }

    out
}

struct Resolver<'a> {
    ctx: &'a Ctx,
    module_dir: &'a Path,
    crate_src: &'a Path,
    child_mods: &'a HashSet<String>,
}

impl Resolver<'_> {
    /// Resolve a `use` path (`crate::a::B`, `super::x`, `child::y`, …) to the
    /// file defining the deepest module named by it.
    fn resolve(&self, segs: &[String]) -> Option<PathBuf> {
        let first = segs.first()?.as_str();
        let (base, rest) = match first {
            "crate" | "$crate" => (self.crate_src.to_path_buf(), &segs[1..]),
            "self" => (self.module_dir.to_path_buf(), &segs[1..]),
            "super" => {
                let mut base = self.module_dir.to_path_buf();
                let mut n = 0;
                while segs.get(n).map(String::as_str) == Some("super") {
                    base = base.parent()?.to_path_buf();
                    n += 1;
                }
                (base, &segs[n..])
            }
            name if self.child_mods.contains(name) => (self.module_dir.to_path_buf(), segs),
            _ => return None,
        };
        self.resolve_from(&base, rest)
    }

    /// Longest prefix of `segs` (relative to the module directory `base`)
    /// that names a module file; an empty prefix is the module of `base`.
    fn resolve_from(&self, base: &Path, segs: &[String]) -> Option<PathBuf> {
        for k in (1..=segs.len()).rev() {
            let mut dir = base.to_path_buf();
            for s in &segs[..k] {
                dir.push(s);
            }
            if let Some(t) = module_file(self.ctx, &dir) {
                return Some(t);
            }
        }
        if base == self.crate_src {
            ["lib.rs", "main.rs"]
                .iter()
                .find_map(|f| self.ctx.lookup(&base.join(f)))
        } else {
            module_file(self.ctx, base)
        }
    }
}

/// The file implementing the module whose directory would be `dir`:
/// `dir.rs` or `dir/mod.rs`.
fn module_file(ctx: &Ctx, dir: &Path) -> Option<PathBuf> {
    ctx.lookup(&dir.with_extension("rs"))
        .or_else(|| ctx.lookup(&dir.join("mod.rs")))
}

/// Directory holding the child modules of the module defined by `file`.
fn module_dir(file: &Path) -> PathBuf {
    let parent = file.parent().unwrap_or(Path::new("")).to_path_buf();
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let is_bin_root = parent.file_name().is_some_and(|n| n == "bin");
    if matches!(stem, "mod" | "lib" | "main") || is_bin_root {
        parent
    } else {
        parent.join(stem)
    }
}

/// `src/` directory of the crate containing `file` (nearest Cargo.toml),
/// falling back to `<root>/src`.
fn crate_src_dir(ctx: &Ctx, file: &Path) -> PathBuf {
    file.parent()
        .and_then(|dir| ctx.find_up(dir, &["Cargo.toml"]))
        .and_then(|manifest| manifest.parent().map(|d| d.join("src")))
        .unwrap_or_else(|| ctx.root().join("src"))
}

/// Expand a use tree such as `crate::a::{b, c::{d as e, self}, *}` into flat
/// segment lists. Globs and trailing `self` are dropped (they name the
/// enclosing module, which is what resolution needs).
fn expand_use_tree(tree: &str) -> Vec<Vec<String>> {
    let cleaned: String = AS_ALIAS
        .replace_all(tree, "")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let mut out = Vec::new();
    expand_item(&cleaned, &[], &mut out);
    out
}

fn expand_item(item: &str, prefix: &[String], out: &mut Vec<Vec<String>>) {
    let item = item.trim_start_matches("::");
    if item.is_empty() {
        return;
    }
    match item.find('{') {
        Some(open) => {
            let head = item[..open].trim_end_matches("::");
            let Some(close) = matching_brace(item, open) else {
                return;
            };
            let mut new_prefix = prefix.to_vec();
            new_prefix.extend(head.split("::").filter(|s| !s.is_empty()).map(str::to_string));
            for sub in split_top_level(&item[open + 1..close]) {
                expand_item(sub, &new_prefix, out);
            }
        }
        None => {
            let mut segs = prefix.to_vec();
            segs.extend(item.split("::").filter(|s| !s.is_empty()).map(str::to_string));
            while matches!(segs.last().map(String::as_str), Some("*") | Some("self")) {
                segs.pop();
            }
            if !segs.is_empty() {
                out.push(segs);
            }
        }
    }
}

fn matching_brace(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.char_indices().skip_while(|(i, _)| *i < open) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::super::test_util::*;
    use super::*;
    use DependencyKind::*;

    #[test]
    fn expands_nested_use_trees() {
        let paths = expand_use_tree("crate::a::{b, c::{d as e, self}, f::*}");
        let flat: Vec<String> = paths.iter().map(|p| p.join("::")).collect();
        assert_eq!(flat, vec!["crate::a::b", "crate::a::c::d", "crate::a::c", "crate::a::f"]);
    }

    #[test]
    fn resolves_mod_use_super_and_inline_paths() {
        let (dir, files) = project(&[
            ("Cargo.toml", "[package]\nname = \"demo\"\n"),
            (
                "src/main.rs",
                "mod net;\nmod util;\n\nfn main() {\n    crate::util::helpers::run();\n}\n",
            ),
            ("src/util/mod.rs", "pub mod helpers;\npub use self::helpers::run;\n"),
            (
                "src/util/helpers.rs",
                "use super::super::net::{\n    client::Client,\n    // comment\n    server,\n};\npub fn run() {}\n",
            ),
            ("src/net.rs", "pub mod client;\npub mod server;\n"),
            ("src/net/client.rs", "use crate::Config;\npub struct Client;\n"),
            ("src/net/server.rs", "use std::io; // external, ignored\n"),
        ]);
        let edges = edges(dir.path(), &files);
        assert_eq!(
            edges,
            vec![
                edge("src/main.rs", "src/net.rs", Module),
                edge("src/main.rs", "src/util/helpers.rs", Import),
                edge("src/main.rs", "src/util/mod.rs", Module),
                // `mod client;` in a non-mod.rs file lives in src/net/
                edge("src/net.rs", "src/net/client.rs", Module),
                edge("src/net.rs", "src/net/server.rs", Module),
                // item at the crate root resolves to main.rs
                edge("src/net/client.rs", "src/main.rs", Import),
                edge("src/util/helpers.rs", "src/net/client.rs", Import),
                edge("src/util/helpers.rs", "src/net/server.rs", Import),
                // `pub mod` + `pub use` of the same file: re-export wins
                edge("src/util/mod.rs", "src/util/helpers.rs", ReExport),
            ]
        );
    }

    #[test]
    fn use_of_declared_child_module_and_path_attr() {
        let (dir, files) = project(&[
            ("Cargo.toml", "[package]\nname = \"demo\"\n"),
            (
                "src/lib.rs",
                "#[path = \"impls/real.rs\"]\nmod backend;\nmod model;\nuse model::{User, Group};\n",
            ),
            ("src/impls/real.rs", ""),
            ("src/model.rs", ""),
        ]);
        let edges = edges(dir.path(), &files);
        assert_eq!(
            edges,
            vec![
                edge("src/lib.rs", "src/impls/real.rs", Module),
                edge("src/lib.rs", "src/model.rs", Module),
            ]
        );
    }
}
