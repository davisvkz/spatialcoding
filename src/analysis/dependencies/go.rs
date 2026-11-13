//! Go: `import "path"` and `import ( … )` blocks. Imports inside the current
//! module (per the nearest go.mod) link to every non-test file of the
//! imported package directory.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::{CommentStyle, Ctx, DependencyKind, strip_comments};

static IMPORT_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bimport\s*\(([^)]*)\)").unwrap());
static IMPORT_SINGLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\bimport\s+(?:[A-Za-z_.][\w]*\s+)?["`]([^"`]+)["`]"#).unwrap()
});
static SPEC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"["`]([^"`]+)["`]"#).unwrap());
static MODULE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*module\s+"?([^\s"]+)"?"#).unwrap());

pub(super) fn analyze(ctx: &Ctx, file: &Path, content: &str) -> Vec<(PathBuf, DependencyKind)> {
    let Some((mod_root, mod_path)) = file.parent().and_then(|d| go_module(ctx, d)) else {
        return Vec::new();
    };
    let code = strip_comments(content, CommentStyle::CLike);

    let mut specs: Vec<String> = Vec::new();
    for block in IMPORT_BLOCK.captures_iter(&code) {
        specs.extend(SPEC.captures_iter(&block[1]).map(|c| c[1].to_string()));
    }
    specs.extend(IMPORT_SINGLE.captures_iter(&code).map(|c| c[1].to_string()));

    let mut out = Vec::new();
    for spec in specs {
        let rest = if spec == mod_path {
            ""
        } else if let Some(rest) = spec.strip_prefix(&format!("{mod_path}/")) {
            rest
        } else {
            continue;
        };
        let pkg_dir = mod_root.join(rest);
        for target in ctx.files_in_dir(&pkg_dir) {
            let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".go") && !name.ends_with("_test.go") {
                out.push((target.clone(), DependencyKind::Import));
            }
        }
    }
    out
}

/// `(module root directory, module path)` from the nearest go.mod.
fn go_module(ctx: &Ctx, dir: &Path) -> Option<(PathBuf, String)> {
    let gomod = ctx.find_up(dir, &["go.mod"])?;
    let root = gomod.parent()?.to_path_buf();
    if let Some(hit) = ctx.go_modules.borrow().get(&gomod) {
        return hit.clone().map(|m| (root, m));
    }
    let module = std::fs::read_to_string(&gomod)
        .ok()
        .and_then(|text| MODULE.captures(&text).map(|c| c[1].to_string()));
    ctx.go_modules.borrow_mut().insert(gomod, module.clone());
    module.map(|m| (root, m))
}

#[cfg(test)]
mod tests {
    use super::super::test_util::*;
    use super::*;
    use DependencyKind::*;

    #[test]
    fn resolves_module_local_packages() {
        let (dir, files) = project(&[
            ("go.mod", "module example.com/demo\n\ngo 1.22\n"),
            (
                "cmd/app/main.go",
                "package main\n\nimport (\n\t\"fmt\"\n\tstore \"example.com/demo/internal/store\"\n\t_ \"example.com/demo/internal/plugins\"\n\t// \"example.com/demo/internal/gone\"\n)\n\nfunc main() { fmt.Println(store.New()) }\n",
            ),
            (
                "internal/store/store.go",
                "package store\n\nimport \"example.com/demo/internal/util\"\n",
            ),
            ("internal/store/cache.go", "package store\n"),
            ("internal/store/store_test.go", "package store\n"),
            ("internal/plugins/p.go", "package plugins\n"),
            ("internal/util/util.go", "package util\n"),
            ("internal/gone/gone.go", "package gone\n"),
        ]);
        let edges = edges(dir.path(), &files);
        assert_eq!(
            edges,
            vec![
                edge("cmd/app/main.go", "internal/plugins/p.go", Import),
                edge("cmd/app/main.go", "internal/store/cache.go", Import),
                edge("cmd/app/main.go", "internal/store/store.go", Import),
                edge("internal/store/store.go", "internal/util/util.go", Import),
            ]
        );
    }
}
