//! Evidence-only local module and document links; independent of LSP/runtime.

use std::path::{Component, Path, PathBuf};
use tree_sitter::Node;

use super::{DependencyLink, LineSpan, OpenFiles};
use crate::analysis::cst::{CstLanguage, parse_tree};
use crate::edges::style::RelationshipType;

/// Deterministic links between eligible open files. No package search, network
/// access, runtime evaluation or arbitrary string-to-symbol matching.
pub fn links(root: &Path, open: &OpenFiles<'_>) -> Vec<DependencyLink> {
    let mut out = Vec::new();
    for (path, text) in open.iter().filter(|(p, _)| p.starts_with(root)) {
        if let Some((language, tree)) = parse_tree(path, text) {
            let mut pending = vec![tree.root_node()];
            while let Some(node) = pending.pop() {
                if !node.has_error() {
                    syntax_links(root, open, path, text, language, node, &mut out);
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

fn syntax_links(root: &Path, open: &OpenFiles<'_>, path: &Path, text: &str, language: CstLanguage, node: Node, out: &mut Vec<DependencyLink>) {
    let raw = |n: Node| n.utf8_text(text.as_bytes()).unwrap_or("").to_string();
    match language {
        CstLanguage::TypeScript | CstLanguage::Tsx | CstLanguage::JavaScript => {
            if matches!(node.kind(), "import_statement" | "export_statement")
                && let Some(source) = node.child_by_field_name("source")
                && let Some(value) = literal(&raw(source)) {
                add_relative(root, open, (path, text), value, source.start_byte() + 1, true, out);
            }
            // `import()` is a grammar keyword, unlike shadowable require().
            if node.kind() == "call_expression"
                && node.child_by_field_name("function").is_some_and(|n| n.kind() == "import")
                && let Some(args) = node.child_by_field_name("arguments")
                && args.named_child_count() == 1
                && let Some(source) = args.named_child(0)
                && let Some(value) = literal(&raw(source)) {
                add_relative(root, open, (path, text), value, source.start_byte() + 1, true, out);
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

fn add_relative(root: &Path, open: &OpenFiles<'_>, source: (&Path, &str), value: &str, offset: usize, module: bool, out: &mut Vec<DependencyLink>) {
    let (path, text) = source;
    let value = value.split('#').next().unwrap_or("");
    if value.is_empty() || value.contains([':', '?', '\\', '%']) || value.starts_with('/') { return; }
    if module && !value.starts_with('.') { return; }
    let Some(parent) = path.parent() else { return };
    let base = parent.join(value);
    let mut candidates = vec![base.clone()];
    if module {
        let extensions = ["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"];
        if base.extension().is_none() {
            for ext in extensions { candidates.extend([base.with_extension(ext), base.join(format!("index.{ext}"))]); }
        } else if base.extension().is_some_and(|e| e == "js" || e == "jsx" || e == "mjs" || e == "cjs") {
            for ext in ["ts", "tsx", "mts", "cts"] { candidates.push(base.with_extension(ext)); }
        }
    }
    if let Some(target) = unique(root, open, candidates) {
        let relationship = if module { RelationshipType::Imports } else { RelationshipType::References };
        push(path, text, &target, offset, offset + value.len(), relationship, out);
    }
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
