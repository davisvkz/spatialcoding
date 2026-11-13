//! TypeScript / JavaScript: ES `import`/`export … from`, dynamic `import()`,
//! CommonJS `require()`; relative specifiers plus tsconfig/jsconfig
//! `baseUrl` and `paths` aliases.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::LazyLock;

use regex::Regex;

use super::{CommentStyle, Ctx, DependencyKind, strip_comments};

/// Extensions tried, in order, for extension-less specifiers.
const EXTENSIONS: &[&str] = &[
    ".ts", ".tsx", ".d.ts", ".js", ".jsx", ".mjs", ".cjs", ".mts", ".cts",
];

static PATTERNS: LazyLock<Vec<(Regex, DependencyKind)>> = LazyLock::new(|| {
    let q = r#"['"]([^'"\n]+)['"]"#;
    vec![
        // export * from 'x' / export { a } from 'x' / export type { T } from 'x'
        (
            Regex::new(&format!(
                r"\bexport\s+(?:type\s+)?(?:\*(?:\s+as\s+[\w$]+)?|\{{[^}}]*\}})\s*from\s*{q}"
            ))
            .unwrap(),
            DependencyKind::ReExport,
        ),
        // import x from 'x' / import { a } from 'x' / import type T from 'x'
        (
            Regex::new(&format!(r#"\bimport\s+[^'"`;()]*?\bfrom\s*{q}"#)).unwrap(),
            DependencyKind::Import,
        ),
        // import 'side-effect'
        (
            Regex::new(&format!(r"\bimport\s*{q}")).unwrap(),
            DependencyKind::Import,
        ),
        // import('lazy') / require('x') / import x = require('x')
        (
            Regex::new(&format!(r"\b(?:import|require)\s*\(\s*{q}\s*\)")).unwrap(),
            DependencyKind::Import,
        ),
    ]
});

static TRAILING_COMMA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r",(\s*[}\]])").unwrap());

pub(super) fn analyze(ctx: &Ctx, file: &Path, content: &str) -> Vec<(PathBuf, DependencyKind)> {
    let code = strip_comments(content, CommentStyle::CLike);
    let mut out = Vec::new();
    for (re, kind) in PATTERNS.iter() {
        for cap in re.captures_iter(&code) {
            if let Some(t) = resolve_specifier(ctx, file, &cap[1]) {
                out.push((t, *kind));
            }
        }
    }
    out
}

fn resolve_specifier(ctx: &Ctx, file: &Path, spec: &str) -> Option<PathBuf> {
    let dir = file.parent()?;
    if spec == "." || spec == ".." || spec.starts_with("./") || spec.starts_with("../") {
        return resolve_candidate(ctx, &dir.join(spec));
    }

    let cfg = ts_config(ctx, dir)?;
    for (pattern, targets) in &cfg.paths {
        let Some(wildcard) = match_pattern(pattern, spec) else {
            continue;
        };
        for target in targets {
            let mapped = target.replacen('*', wildcard, 1);
            if let Some(t) = resolve_candidate(ctx, &cfg.paths_base.join(mapped)) {
                return Some(t);
            }
        }
    }
    cfg.base_url
        .as_ref()
        .and_then(|base| resolve_candidate(ctx, &base.join(spec)))
}

/// Node/TypeScript-style module resolution against the file set.
fn resolve_candidate(ctx: &Ctx, path: &Path) -> Option<PathBuf> {
    if let Some(t) = ctx.lookup(path) {
        return Some(t);
    }

    let raw = path.to_string_lossy();
    for ext in EXTENSIONS {
        if let Some(t) = ctx.lookup(Path::new(&format!("{raw}{ext}"))) {
            return Some(t);
        }
    }

    // TS ESM: `./x.js` refers to `./x.ts` at compile time.
    let ts_alternatives: &[&str] = match path.extension().and_then(|e| e.to_str()) {
        Some("js") => &["ts", "tsx", "d.ts"],
        Some("jsx") => &["tsx"],
        Some("mjs") => &["mts"],
        Some("cjs") => &["cts"],
        _ => &[],
    };
    for alt in ts_alternatives {
        if let Some(t) = ctx.lookup(&path.with_extension(alt)) {
            return Some(t);
        }
    }

    EXTENSIONS
        .iter()
        .find_map(|ext| ctx.lookup(&path.join(format!("index{ext}"))))
}

/// Match a tsconfig `paths` key (at most one `*`) and return the text the
/// wildcard matched (empty for exact keys).
fn match_pattern<'s>(pattern: &str, spec: &'s str) -> Option<&'s str> {
    match pattern.split_once('*') {
        None => (pattern == spec).then_some(""),
        Some((prefix, suffix)) => {
            if spec.len() >= prefix.len() + suffix.len()
                && spec.starts_with(prefix)
                && spec.ends_with(suffix)
            {
                Some(&spec[prefix.len()..spec.len() - suffix.len()])
            } else {
                None
            }
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct TsConfig {
    base_url: Option<PathBuf>,
    /// Directory `paths` targets are relative to.
    paths_base: PathBuf,
    /// `(pattern, targets)`, longest pattern prefix first.
    paths: Vec<(String, Vec<String>)>,
}

/// Nearest tsconfig.json / jsconfig.json above `dir`, cached per config file.
fn ts_config(ctx: &Ctx, dir: &Path) -> Option<Rc<TsConfig>> {
    let path = ctx.find_up(dir, &["tsconfig.json", "jsconfig.json"])?;
    if let Some(hit) = ctx.ts_configs.borrow().get(&path) {
        return hit.clone();
    }
    let cfg = load_ts_config(&path, 0).map(Rc::new);
    ctx.ts_configs.borrow_mut().insert(path, cfg.clone());
    cfg
}

fn load_ts_config(path: &Path, depth: usize) -> Option<TsConfig> {
    if depth > 8 {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let json = strip_comments(&text, CommentStyle::CLike);
    let json = TRAILING_COMMA.replace_all(&json, "$1");
    let value: serde_json::Value = serde_json::from_str(&json).ok()?;
    let dir = path.parent()?.to_path_buf();

    // Inherit from a relative `extends` (package-based extends are ignored).
    let mut cfg = value
        .get("extends")
        .and_then(|e| e.as_str())
        .filter(|e| e.starts_with('.'))
        .and_then(|e| {
            let file = if e.ends_with(".json") {
                e.to_string()
            } else {
                format!("{e}.json")
            };
            load_ts_config(&dir.join(file), depth + 1)
        })
        .unwrap_or_else(|| TsConfig {
            paths_base: dir.clone(),
            ..TsConfig::default()
        });

    let options = value.get("compilerOptions");
    if let Some(base) = options.and_then(|o| o.get("baseUrl")).and_then(|b| b.as_str()) {
        let base = dir.join(base);
        cfg.base_url = Some(base.clone());
        cfg.paths_base = base;
    }
    if let Some(paths) = options.and_then(|o| o.get("paths")).and_then(|p| p.as_object()) {
        if cfg.base_url.is_none() {
            cfg.paths_base = dir.clone();
        }
        cfg.paths = paths
            .iter()
            .map(|(k, v)| {
                let targets = v
                    .as_array()
                    .map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                (k.clone(), targets)
            })
            .collect();
        cfg.paths.sort_by_key(|(k, _)| {
            std::cmp::Reverse(k.split('*').next().unwrap_or("").len())
        });
    }
    Some(cfg)
}

#[cfg(test)]
mod tests {
    use super::super::test_util::*;
    use super::*;
    use DependencyKind::*;

    #[test]
    fn resolves_relative_imports_requires_and_reexports() {
        let (dir, files) = project(&[
            (
                "src/app.ts",
                "import React from 'react';\n\
                 import { a } from './a';\n\
                 import type { T } from \"./types\";\n\
                 import './side-effect.js';\n\
                 // import { gone } from './commented';\n\
                 const lazy = () => import('./lazy');\n\
                 const legacy = require('../lib/legacy');\n\
                 import cfg = require('./cfg.json');\n",
            ),
            ("src/a.ts", "export const a = 1;\n"),
            ("src/types.d.ts", "export type T = string;\n"),
            ("src/side-effect.ts", ""),
            ("src/commented.ts", ""),
            ("src/lazy/index.tsx", ""),
            ("src/cfg.json", "{}"),
            ("lib/legacy.js", "module.exports = {};\n"),
            ("src/index.ts", "export * from './a';\nexport { T } from './types';\n"),
        ]);
        let edges = edges(dir.path(), &files);
        assert_eq!(
            edges,
            vec![
                edge("src/app.ts", "lib/legacy.js", Import),
                edge("src/app.ts", "src/a.ts", Import),
                edge("src/app.ts", "src/cfg.json", Import),
                edge("src/app.ts", "src/lazy/index.tsx", Import),
                edge("src/app.ts", "src/side-effect.ts", Import),
                edge("src/app.ts", "src/types.d.ts", Import),
                edge("src/index.ts", "src/a.ts", ReExport),
                edge("src/index.ts", "src/types.d.ts", ReExport),
            ]
        );
    }

    #[test]
    fn resolves_tsconfig_paths_and_base_url() {
        let (dir, files) = project(&[
            (
                "tsconfig.json",
                "{\n  // comment\n  \"extends\": \"./tsconfig.base\",\n  \"compilerOptions\": {\n    \"paths\": { \"@/*\": [\"src/*\"], \"#utils\": [\"src/lib/utils.ts\"], },\n  },\n}\n",
            ),
            ("tsconfig.base.json", "{ \"compilerOptions\": { \"baseUrl\": \".\" } }"),
            (
                "src/main.tsx",
                "import Button from '@/components/Button';\nimport { u } from '#utils';\nimport x from 'src/lib/x';\nimport y from 'lodash';\n",
            ),
            ("src/components/Button.tsx", ""),
            ("src/lib/utils.ts", ""),
            ("src/lib/x.js", ""),
        ]);
        let edges = edges(dir.path(), &files);
        assert_eq!(
            edges,
            vec![
                edge("src/main.tsx", "src/components/Button.tsx", Import),
                edge("src/main.tsx", "src/lib/utils.ts", Import),
                edge("src/main.tsx", "src/lib/x.js", Import),
            ]
        );
    }

    #[test]
    fn multiline_named_imports() {
        let (dir, files) = project(&[
            ("a.js", "import {\n  one,\n  two,\n} from './b.mjs';\n"),
            ("b.mts", ""),
        ]);
        assert_eq!(edges(dir.path(), &files), vec![edge("a.js", "b.mts", Import)]);
    }
}
