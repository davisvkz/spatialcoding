//! Python: `import a.b`, `from a.b import c`, and relative
//! `from . import x` / `from ..pkg import y` imports.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::{Ctx, DependencyKind};

static IMPORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^import\s+(.+)$").unwrap());
static FROM_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^from\s+(\.*)\s*([A-Za-z_][\w.]*)?\s*import\s+(.+)$").unwrap()
});

pub(super) fn analyze(ctx: &Ctx, file: &Path, content: &str) -> Vec<(PathBuf, DependencyKind)> {
    let Some(file_dir) = file.parent() else {
        return Vec::new();
    };
    let roots = search_roots(ctx, file_dir);
    let mut out = Vec::new();
    let mut push = |t: Option<PathBuf>| {
        if let Some(t) = t {
            out.push((t, DependencyKind::Import));
        }
    };

    for stmt in statements(content) {
        if let Some(cap) = IMPORT.captures(&stmt) {
            for part in cap[1].split(',') {
                let Some(module) = part.split_whitespace().next() else {
                    continue;
                };
                let segs: Vec<&str> = module.split('.').collect();
                push(resolve_longest(ctx, &roots, &segs));
            }
        } else if let Some(cap) = FROM_IMPORT.captures(&stmt) {
            let dots = cap[1].len();
            let module = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            let segs: Vec<&str> = module.split('.').filter(|s| !s.is_empty()).collect();
            let names: Vec<&str> = cap[3]
                .split(',')
                .filter_map(|n| n.split_whitespace().next())
                .filter(|n| *n != "*")
                .collect();

            let bases: Vec<PathBuf> = if dots > 0 {
                let mut base = file_dir.to_path_buf();
                for _ in 1..dots {
                    match base.parent() {
                        Some(p) => base = p.to_path_buf(),
                        None => break,
                    }
                }
                vec![base]
            } else {
                roots.clone()
            };

            // `from pkg import submodule` targets the submodule file; names
            // that are not submodules are attributes of the module itself.
            let mut need_module = names.is_empty();
            for name in &names {
                let mut full = segs.clone();
                full.push(name);
                match bases.iter().find_map(|b| try_module(ctx, &join(b, &full))) {
                    Some(t) => push(Some(t)),
                    None => need_module = true,
                }
            }
            if need_module {
                push(resolve_longest(ctx, &bases, &segs));
            }
        }
    }

    out
}

/// Directories absolute imports are resolved against: the parent of the
/// top-level package containing the file, the project root, `<root>/src`,
/// and the file's own directory (script-style sibling imports).
fn search_roots(ctx: &Ctx, file_dir: &Path) -> Vec<PathBuf> {
    let mut top = file_dir.to_path_buf();
    while top.join("__init__.py").is_file() {
        match top.parent() {
            Some(p) => top = p.to_path_buf(),
            None => break,
        }
    }
    let mut roots = Vec::new();
    for r in [
        top,
        ctx.root().to_path_buf(),
        ctx.root().join("src"),
        file_dir.to_path_buf(),
    ] {
        if !roots.contains(&r) {
            roots.push(r);
        }
    }
    roots
}

fn join(base: &Path, segs: &[&str]) -> PathBuf {
    let mut p = base.to_path_buf();
    p.extend(segs);
    p
}

/// `base.py`, `base.pyi` or `base/__init__.py`.
fn try_module(ctx: &Ctx, base: &Path) -> Option<PathBuf> {
    let raw = base.to_string_lossy();
    ctx.lookup(Path::new(&format!("{raw}.py")))
        .or_else(|| ctx.lookup(Path::new(&format!("{raw}.pyi"))))
        .or_else(|| ctx.lookup(&base.join("__init__.py")))
}

/// Longest prefix of `segs` that resolves to a module under one of `bases`.
/// With no segments, the package `__init__.py` of a base is used.
fn resolve_longest(ctx: &Ctx, bases: &[PathBuf], segs: &[&str]) -> Option<PathBuf> {
    if segs.is_empty() {
        return bases.iter().find_map(|b| ctx.lookup(&b.join("__init__.py")));
    }
    (1..=segs.len())
        .rev()
        .find_map(|k| bases.iter().find_map(|b| try_module(ctx, &join(b, &segs[..k]))))
}

/// Split source into logical statements: comments and string literals are
/// removed, bracketed and backslash-continued lines are joined, and `;`
/// separates statements. Parentheses are dropped (only import lists use them
/// in the statements we care about).
fn statements(src: &str) -> Vec<String> {
    let b: Vec<char> = src.chars().collect();
    let len = b.len();
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut depth: i32 = 0;
    let mut i = 0;

    while i < len {
        let c = b[i];
        match c {
            '#' => {
                while i < len && b[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '\'' | '"' => {
                let triple = b.get(i + 1) == Some(&c) && b.get(i + 2) == Some(&c);
                i += if triple { 3 } else { 1 };
                while i < len {
                    if b[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if triple {
                        if b[i] == c && b.get(i + 1) == Some(&c) && b.get(i + 2) == Some(&c) {
                            i += 3;
                            break;
                        }
                    } else if b[i] == c || b[i] == '\n' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                cur.push_str("''");
                continue;
            }
            '\\' if b.get(i + 1) == Some(&'\n') => {
                cur.push(' ');
                i += 2;
                continue;
            }
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(' ');
            }
            ')' | ']' | '}' => {
                depth = (depth - 1).max(0);
                cur.push(' ');
            }
            '\n' if depth > 0 => cur.push(' '),
            '\n' => lines.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
        i += 1;
    }
    lines.push(cur);

    lines
        .iter()
        .flat_map(|l| l.split(';'))
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::test_util::*;
    use super::*;
    use DependencyKind::*;

    #[test]
    fn resolves_absolute_relative_and_submodule_imports() {
        let (dir, files) = project(&[
            (
                "main.py",
                "import os, sys\nimport app.models as m\nfrom app import (\n    views,  # comment\n    VERSION,\n)\n\"\"\"\nimport app.fake\n\"\"\"\n",
            ),
            ("app/__init__.py", "VERSION = '1'\n"),
            ("app/models.py", "from .db import Session\nfrom . import utils\n"),
            ("app/views.py", "from ..shared.helpers import fmt; import json\n"),
            ("app/db/__init__.py", ""),
            ("app/utils.py", ""),
            ("app/fake.py", ""),
            ("shared/__init__.py", ""),
            ("shared/helpers.py", ""),
        ]);
        let edges = edges(dir.path(), &files);
        assert_eq!(
            edges,
            vec![
                edge("app/models.py", "app/db/__init__.py", Import),
                edge("app/models.py", "app/utils.py", Import),
                edge("app/views.py", "shared/helpers.py", Import),
                edge("main.py", "app/__init__.py", Import),
                edge("main.py", "app/models.py", Import),
                edge("main.py", "app/views.py", Import),
            ]
        );
    }

    #[test]
    fn src_layout_package() {
        let (dir, files) = project(&[
            ("src/pkg/__init__.py", ""),
            ("src/pkg/core.py", "from pkg.io import read\n"),
            ("src/pkg/io.py", ""),
            ("tests/test_core.py", "import pkg.core\n"),
        ]);
        let edges = edges(dir.path(), &files);
        assert_eq!(
            edges,
            vec![
                edge("src/pkg/core.py", "src/pkg/io.py", Import),
                edge("tests/test_core.py", "src/pkg/core.py", Import),
            ]
        );
    }
}
