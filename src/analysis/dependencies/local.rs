//! Evidence-only local module and document links; independent of LSP/runtime.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;
use tree_sitter::Node;

use super::{DependencyLink, LineSpan, OpenFiles};
use crate::analysis::cst::{CstLanguage, parse_tree};
use crate::edges::style::RelationshipType;

pub(crate) type ConfigSources = BTreeMap<PathBuf, String>;
const MAX_CONFIG_BYTES: usize = 256 * 1024;

/// Candidate project configs, including absent paths that may exist in Git.
pub(crate) fn config_paths<'a>(
    root: &'a Path,
    paths: impl IntoIterator<Item = &'a Path>,
) -> BTreeSet<PathBuf> {
    paths.into_iter()
        .filter(|p| matches!(CstLanguage::from_path(p), Some(
            CstLanguage::TypeScript | CstLanguage::Tsx | CstLanguage::JavaScript
        )))
        .flat_map(|p| ancestors(root, p).map(|dir| dir.join("tsconfig.json")))
        .collect()
}

fn ancestors<'a>(root: &'a Path, path: &'a Path) -> impl Iterator<Item = &'a Path> {
    path.ancestors().skip(1).take_while(move |dir| dir.starts_with(root))
}

/// Read closed configs, but prefer the snapshot for configs on the canvas.
pub(crate) fn working_configs(root: &Path, open: &OpenFiles<'_>) -> ConfigSources {
    let Ok(canonical_root) = root.canonicalize() else { return ConfigSources::new() };
    config_paths(root, open.iter().map(|(p, _)| p)).into_iter().filter_map(|path| {
        let actual = path.canonicalize().ok()?;
        if !actual.starts_with(&canonical_root) || !actual.is_file() { return None; }
        let text = if let Some((_, text)) = open.get(&path) {
            text.to_owned()
        } else {
            let mut text = String::new();
            let _ = std::fs::File::open(actual).ok()?.take(MAX_CONFIG_BYTES as u64 + 1)
                .read_to_string(&mut text).ok()?;
            text
        };
        (text.len() <= MAX_CONFIG_BYTES).then_some((path, text))
    }).collect()
}

pub fn links(root: &Path, open: &OpenFiles<'_>) -> Vec<DependencyLink> {
    links_with_configs(root, open, &working_configs(root, open))
}

/// Deterministic links within a caller-supplied snapshot; no package search,
/// config discovery, network access, or config execution.
pub(crate) fn links_with_configs(
    root: &Path, open: &OpenFiles<'_>, configs: &ConfigSources,
) -> Vec<DependencyLink> {
    let aliases: BTreeMap<_, _> = configs.iter()
        .map(|(path, text)| (path, TsPaths::parse(path, text)))
        .collect();
    let mut out = Vec::new();
    for (path, text) in open.iter().filter(|(p, _)| p.starts_with(root)) {
        if let Some((language, tree)) = parse_tree(path, text) {
            let paths = ancestors(root, path)
                .find_map(|dir| aliases.get(&dir.join("tsconfig.json")))
                .and_then(Option::as_ref);
            let mut pending = vec![tree.root_node()];
            while let Some(node) = pending.pop() {
                if !node.has_error() {
                    syntax_links(root, open, (path, text), language, node, paths, &mut out);
                }
                let mut cursor = node.walk();
                pending.extend(node.named_children(&mut cursor));
            }
        }
        match path.extension().and_then(|e| e.to_str()) {
            Some("md" | "mdx") => markdown_links(root, open, path, text, &mut out),
            Some("json") => json_links(root, open, path, text, &mut out),
            _ => {}
        }
    }
    out.sort_by(|a, b| (&a.source, a.usage.start, &a.target).cmp(&(&b.source, b.usage.start, &b.target)));
    out.dedup_by(|a, b| a.source == b.source && a.target == b.target && a.source_name_span == b.source_name_span && a.relationship == b.relationship);
    out
}

fn syntax_links(root: &Path, open: &OpenFiles<'_>, (path, text): (&Path, &str), language: CstLanguage, node: Node, paths: Option<&TsPaths>, out: &mut Vec<DependencyLink>) {
    let raw = |n: Node| n.utf8_text(text.as_bytes()).unwrap_or("").to_string();
    match language {
        CstLanguage::TypeScript | CstLanguage::Tsx | CstLanguage::JavaScript => {
            if matches!(node.kind(), "import_statement" | "export_statement")
                && let Some(source) = node.child_by_field_name("source")
                && let Some(value) = literal(&raw(source)) {
                add_js_import(root, open, (path, text), value, source.start_byte() + 1, paths, out);
            }
            // `import()` is a grammar keyword, unlike shadowable require().
            if node.kind() == "call_expression"
                && node.child_by_field_name("function").is_some_and(|n| n.kind() == "import")
                && let Some(args) = node.child_by_field_name("arguments")
                && args.named_child_count() == 1
                && let Some(source) = args.named_child(0)
                && let Some(value) = literal(&raw(source)) {
                add_js_import(root, open, (path, text), value, source.start_byte() + 1, paths, out);
            }
        }
        CstLanguage::Rust => {
            if node.kind() == "mod_item" && node.child_by_field_name("body").is_none()
                && let Some(name) = node.child_by_field_name("name") {
                // A #[path] attribute changes resolution: skip rather than lie.
                if node.prev_named_sibling().is_some_and(|n| n.kind() == "attribute_item") { return; }
                let Some(parent) = path.parent() else { return };
                let parent = if matches!(path.file_stem().and_then(|s| s.to_str()), Some("lib" | "main" | "mod")) {
                    parent.to_path_buf()
                } else { parent.join(path.file_stem().unwrap_or_default()) };
                // Inline modules change the base directory; not guessed here.
                if node.parent().is_some_and(|n| n.kind() != "source_file") { return; }
                let value = raw(name);
                let candidates = [parent.join(format!("{value}.rs")), parent.join(&value).join("mod.rs")];
                if let Some(target) = unique(root, open, candidates) {
                    push(path, text, &target, name.start_byte(), name.end_byte(), RelationshipType::Imports, out);
                }
            }
            if node.kind() == "macro_invocation"
                && node.child_by_field_name("macro").is_some_and(|n| matches!(raw(n).as_str(), "include_str" | "include_bytes")) {
                let mut cursor = node.walk();
                if let Some(tokens) = node.named_children(&mut cursor).find(|n| n.kind() == "token_tree")
                    && tokens.named_child_count() == 1
                    && let Some(source) = tokens.named_child(0)
                    && let Some(value) = literal(&raw(source)) {
                    add_relative(root, open, (path, text), value, source.start_byte() + 1, false, out);
                }
            }
        }
        CstLanguage::Python => {
            if node.kind() == "import_from_statement"
                && let Some(module) = node.child_by_field_name("module_name") {
                let value = raw(module);
                let dots = value.bytes().take_while(|b| *b == b'.').count();
                if dots == 0 { return; } // absolute packages require environment evidence
                let Some(mut base) = path.parent() else { return };
                for _ in 1..dots { let Some(parent) = base.parent() else { return }; base = parent; }
                let module_path = value[dots..].replace('.', "/");
                if module_path.is_empty() { return; }
                let base = base.join(module_path);
                if let Some(target) = unique(root, open, [base.with_extension("py"), base.join("__init__.py")]) {
                    push(path, text, &target, module.start_byte(), module.end_byte(), RelationshipType::Imports, out);
                }
            }
        }
        CstLanguage::Go => {
            if node.kind() == "import_spec"
                && let Some(source) = node.child_by_field_name("path")
                && let Some(value) = literal(&raw(source)) {
                // The nearest open go.mod establishes the module identity.
                let manifest = path.ancestors().skip(1).filter(|p| p.starts_with(root)).find_map(|p| open.get(&p.join("go.mod")));
                let Some((manifest, content)) = manifest else { return };
                let module = content.lines().find_map(|l| l.trim().strip_prefix("module ").map(str::trim));
                let Some(relative) = module.and_then(|m| value.strip_prefix(&format!("{m}/"))) else { return };
                let Some(parent) = manifest.parent() else { return };
                let directory = parent.join(relative);
                let candidates = open.iter().filter(|(p, _)| p.parent() == Some(directory.as_path()) && p.extension().is_some_and(|e| e == "go") && !p.to_string_lossy().ends_with("_test.go")).map(|(p, _)| p.to_path_buf());
                if let Some(target) = unique(root, open, candidates) {
                    push(path, text, &target, source.start_byte() + 1, source.end_byte() - 1, RelationshipType::Imports, out);
                }
            }
        }
    }
}

fn literal(raw: &str) -> Option<&str> {
    let quote = raw.as_bytes().first()?;
    if !matches!(quote, b'\'' | b'"' | b'`') || raw.as_bytes().last()? != quote { return None; }
    let value = raw.get(1..raw.len().checked_sub(1)?)?;
    (!value.contains(['\\', '\n', '\r']) && !value.contains("${")).then_some(value)
}

fn normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {},
            Component::ParentDir => { if !out.pop() { return None; } },
            _ => out.push(part.as_os_str()),
        }
    }
    Some(out)
}

fn unique(root: &Path, open: &OpenFiles<'_>, candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    let mut matches = Vec::new();
    for candidate in candidates {
        let Some(candidate) = normalize(&candidate).filter(|p| p.starts_with(root)) else { continue };
        let Some((actual, _)) = open.get(&candidate) else { continue };
        if let (Ok(root), Ok(actual)) = (root.canonicalize(), actual.canonicalize())
            && !actual.starts_with(root) { continue; }
        if !matches.contains(&actual.to_path_buf()) { matches.push(actual.to_path_buf()); }
    }
    (matches.len() == 1).then(|| matches.remove(0))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TsCompilerOptions {
    base_url: Option<PathBuf>,
    #[serde(default)]
    paths: BTreeMap<String, Vec<String>>,
}

struct TsPaths {
    base: PathBuf,
    paths: BTreeMap<String, Vec<String>>,
}

impl TsPaths {
    fn parse(path: &Path, text: &str) -> Option<Self> {
        if text.len() > MAX_CONFIG_BYTES { return None; }
        // Preserve strings (including URL-like text) while removing JSONC
        // comments and trailing commas. The result is still validated as JSON.
        static COMMENTS: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r#"("(?:[^"\\]|\\.)*")|(?s:/\*.*?\*/)|//[^\r\n]*"#).unwrap()
        });
        static TRAILING: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r#"("(?:[^"\\]|\\.)*")|,\s*([}\]])"#).unwrap()
        });
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Config {
            compiler_options: TsCompilerOptions,
        }
        let clean = COMMENTS.replace_all(text.trim_start_matches('\u{feff}'), "$1 ");
        let clean = TRAILING.replace_all(&clean, "${1}${2}");
        let options = serde_json::from_str::<Config>(&clean).ok()?.compiler_options;
        let base = options.base_url.as_deref().unwrap_or(Path::new("."));
        if !base.is_relative() { return None; }
        Some(Self { base: path.parent()?.join(base), paths: options.paths })
    }

    fn target(&self, value: &str) -> Option<PathBuf> {
        let (targets, capture) = if let Some(targets) = self.paths.get(value) {
            (targets, "")
        } else {
            let (_, targets, capture) = self.paths.iter().filter_map(|(key, targets)| {
                let (prefix, suffix) = key.split_once('*')?;
                if suffix.contains('*') { return None; }
                let capture = value.strip_prefix(prefix)?.strip_suffix(suffix)?;
                Some((prefix.len(), targets, capture))
            }).max_by_key(|(prefix, _, _)| *prefix)?;
            (targets, capture)
        };
        // Ordered fallback lists would require knowing whether each target
        // exists even when it isn't on the canvas or in this Git snapshot.
        let [target] = targets.as_slice() else { return None; };
        let target = target.replacen('*', capture, 1);
        let target = Path::new(&target);
        (target.is_relative() && !target.to_string_lossy().contains(['*', ':', '\\', '%']))
            .then(|| self.base.join(target))
    }
}

fn add_js_import(
    root: &Path, open: &OpenFiles<'_>, (path, text): (&Path, &str), value: &str,
    offset: usize, paths: Option<&TsPaths>, out: &mut Vec<DependencyLink>,
) {
    if value.starts_with('.') {
        return add_relative(root, open, (path, text), value, offset, true, out);
    }
    if value.contains([':', '?', '\\', '%', '#']) || value.starts_with('/') { return; }
    let Some(base) = paths.and_then(|paths| paths.target(value)) else { return; };
    if let Some(target) = unique(root, open, module_candidates(&base)) {
        push(path, text, &target, offset, offset + value.len(), RelationshipType::Imports, out);
    }
}

fn add_relative(root: &Path, open: &OpenFiles<'_>, source: (&Path, &str), value: &str, offset: usize, module: bool, out: &mut Vec<DependencyLink>) {
    let (path, text) = source;
    let value = value.split('#').next().unwrap_or("");
    if value.is_empty() || value.contains([':', '?', '\\', '%']) || value.starts_with('/') { return; }
    if module && !value.starts_with('.') { return; }
    let Some(parent) = path.parent() else { return };
    let base = parent.join(value);
    let candidates = if module { module_candidates(&base) } else { vec![base] };
    if let Some(target) = unique(root, open, candidates) {
        let relationship = if module { RelationshipType::Imports } else { RelationshipType::References };
        push(path, text, &target, offset, offset + value.len(), relationship, out);
    }
}

fn module_candidates(base: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![base.to_owned()];
    if base.extension().is_none() {
        for ext in ["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"] {
            candidates.extend([base.with_extension(ext), base.join(format!("index.{ext}"))]);
        }
    } else if base.extension().is_some_and(|e| e == "js" || e == "jsx" || e == "mjs" || e == "cjs") {
        for ext in ["ts", "tsx", "mts", "cts"] { candidates.push(base.with_extension(ext)); }
    }
    candidates
}

fn push(path: &Path, text: &str, target: &Path, start: usize, end: usize, relationship: RelationshipType, out: &mut Vec<DependencyLink>) {
    if path == target || !text.is_char_boundary(start) || !text.is_char_boundary(end) { return; }
    let line = text[..start].bytes().filter(|b| *b == b'\n').count();
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    if text[start..end].contains('\n') { return; }
    out.push(DependencyLink {
        evidence: super::LinkEvidence::LocalSyntax,
        relationship, source: path.into(), target: target.into(), label: "file".into(), source_label: None,
        usage: LineSpan { start: line, end: line }, definition: LineSpan { start: 0, end: 0 },
        name_span: None, source_name_span: Some((line, start - line_start, end - line_start)),
    });
}

fn markdown_links(root: &Path, open: &OpenFiles<'_>, path: &Path, text: &str, out: &mut Vec<DependencyLink>) {
    let mut fence = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let marker = &trimmed[..3];
            fence = if fence == Some(marker) { None } else { Some(marker) };
        } else if fence.is_none() && !line.starts_with("    ") {
            for (i, _) in line.match_indices("](") {
                let start = i + 2;
                if line[..i].contains('`') { continue; }
                let Some(end) = line[start..].find(')').map(|n| start + n) else { continue };
                let value = &line[start..end];
                if value.contains(char::is_whitespace) { continue; }
                add_relative(root, open, (path, text), value, offset + start, false, out);
            }
        }
        offset += line.len();
    }
}

fn json_links(root: &Path, open: &OpenFiles<'_>, path: &Path, text: &str, out: &mut Vec<DependencyLink>) {
    if serde_json::from_str::<serde_json::Value>(text).is_err() { return; }
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut reference_value = false;
    while i < bytes.len() {
        if bytes[i] != b'"' { i += 1; continue; }
        let start = i;
        i += 1;
        while i < bytes.len() {
            if bytes[i] == b'\\' { i += 2; continue; }
            if bytes[i] == b'"' { break; }
            i += 1;
        }
        if i >= bytes.len() { break; }
        let token = &text[start..=i];
        i += 1;
        if let Some(value) = literal(token) {
            if reference_value { add_relative(root, open, (path, text), value, start + 1, false, out); }
            reference_value = matches!(value, "$ref" | "extends") && text[i..].trim_start().starts_with(':')
                && text[i..].trim_start().strip_prefix(':').is_some_and(|s| s.trim_start().starts_with('"'));
        } else { reference_value = false; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edges::style::RelationshipType::{Imports, References};

    #[test]
    fn local_imports_are_real_even_without_a_language_server() {
        let files = [
            ("/p/app.tsx", "import { Button as Alias } from './Button.js';\nexport * from './lib';\nimport 'react';\nconst text = \"import './fake'\";"),
            ("/p/Button.tsx", "export function Button() {}"),
            ("/p/lib/index.ts", "export const x = 1;"),
            ("/p/fake.ts", ""),
        ];
        let open = OpenFiles::new(files.iter().map(|(p, t)| (Path::new(p), *t)));
        let links = links(Path::new("/p"), &open);
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|l| l.relationship == Imports));
        assert_eq!(links[0].target, Path::new("/p/Button.tsx"));
        assert_eq!(links[0].source_name_span, Some((0, 33, 44)));
        assert!(
            links[0].name_span.is_none(),
            "module import has no invented declaration name"
        );
    }

    #[test]
    fn local_resolution_rejects_ambiguous_external_and_escaping_paths() {
        let files = [
            (
                "/p/app.ts",
                "import './x';\nimport '../outside';\nimport '/etc/passwd';\nimport 'pkg';",
            ),
            ("/p/x.ts", ""),
            ("/p/x.tsx", ""),
            ("/outside.ts", ""),
        ];
        let open = OpenFiles::new(files.iter().map(|(p, t)| (Path::new(p), *t)));
        assert!(links(Path::new("/p"), &open).is_empty());
    }

    #[test]
    fn exact_and_wildcard_aliases_use_the_nearest_jsonc_config() {
        let root = Path::new("/p");
        let files = [
            ("/p/web/app.ts", "export * from '@app/exact';\nimport('@app/util');\nimport '@escape';\nimport '@ambiguous';"),
            ("/p/web/src/exact.ts", ""),
            ("/p/web/src/util/index.ts", ""),
            ("/p/web/src/ambiguous.ts", ""),
            ("/p/web/src/ambiguous.tsx", ""),
            ("/outside.ts", ""),
        ];
        let configs = BTreeMap::from([
            (root.join("tsconfig.json"), r#"{"compilerOptions":{"paths":{"@app/*":["./missing/*"]}}}"#.into()),
            (root.join("web/tsconfig.json"), r#"{
                // Use the workspace tsconfig, not the parent config.
                "compilerOptions": {"baseUrl": "src", "paths": {
                    "@app/exact": ["exact"], "@app/*": ["*"],
                    "@escape": ["../../../outside"], "@ambiguous": ["ambiguous"],
                }},
            }"#.into()),
        ]);
        let open = OpenFiles::new(files.iter().map(|(p, t)| (Path::new(p), *t)));
        let links = links_with_configs(root, &open, &configs);
        assert_eq!(links.iter().map(|link| link.target.as_path()).collect::<Vec<_>>(),
            [Path::new("/p/web/src/exact.ts"), Path::new("/p/web/src/util/index.ts")]);
    }

    #[test]
    fn bundled_languages_and_document_references_use_explicit_local_targets() {
        let files = [
            ("/p/src/main.rs", "mod util;\nconst CFG: &str = include_str!(\"../config.json\");"),
            ("/p/src/util.rs", "pub fn util() {}"),
            ("/p/shop/cart.py", "from .models import Item\nfrom external import X"),
            ("/p/shop/models.py", "class Item: pass"),
            ("/p/go.mod", "module example.com/demo\n"),
            ("/p/main.go", "package main\nimport \"example.com/demo/pkg/api\"\nimport \"fmt\""),
            ("/p/pkg/api/api.go", "package api"),
            ("/p/README.md", "[config](./config.json#settings)\n```\n[fake](./src/util.rs)\n```\n[web](https://example.com)"),
            ("/p/config.json", "{\"$ref\": \"./schema.json#/settings\"}"),
            ("/p/schema.json", "{}"),
        ];
        let open = OpenFiles::new(files.iter().map(|(p, t)| (Path::new(p), *t)));
        let links = links(Path::new("/p"), &open);
        let counts = |kind| links.iter().filter(|l| l.relationship == kind).count();
        assert_eq!(counts(Imports), 3, "{links:?}");
        assert_eq!(counts(References), 3, "{links:?}");
        assert!(links.iter().all(|l| l.source_name_span.is_some()));
    }
}
