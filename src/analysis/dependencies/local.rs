//! Evidence-only local module and document links; independent of LSP/runtime.

use rayon::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock};
use tree_sitter::{Node, Tree};

use super::{DependencyLink, LineSpan, OpenFiles};
use crate::analysis::cst::{CstLanguage, parse_tree};
use crate::edges::style::RelationshipType;

mod issues;
mod rust_modules;
pub(crate) use issues::{ImportIssue, ImportIssueReason, ImportIssues};

pub(crate) type ConfigSources = BTreeMap<PathBuf, String>;
const MAX_CONFIG_BYTES: usize = 256 * 1024;

/// Candidate project configs, including absent paths that may exist in Git.
pub(crate) fn config_paths<'a>(
    root: &'a Path,
    paths: impl IntoIterator<Item = &'a Path>,
) -> BTreeSet<PathBuf> {
    paths.into_iter()
        .filter_map(|p| {
            let config = match CstLanguage::from_path(p)? {
                CstLanguage::TypeScript | CstLanguage::Tsx | CstLanguage::JavaScript => "tsconfig.json",
                CstLanguage::Go => "go.mod",
                _ => return None,
            };
            Some(ancestors(root, p).map(move |dir| dir.join(config)))
        })
        .flatten()
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
    analyze_with_configs(root, open, configs, &mut ParseCache::default()).links
}

#[derive(Default)]
pub(crate) struct LocalAnalysis {
    pub links: Vec<DependencyLink>,
    /// Imports that gave no link, and why.
    pub issues: ImportIssues,
}

/// The syntax tree of every open file, by path and content hash.
///
/// Parsing is most of the analysis (about 0.4 ms per KB), and the analysis
/// runs again whenever the set of open files changes, so a file whose text did
/// not change keeps its tree. The trees do not depend on the other open files,
/// only the links resolved from them do.
#[derive(Default)]
pub(crate) struct ParseCache(HashMap<PathBuf, (u64, Parsed)>);

/// A file's language and tree; `None` for files without a bundled grammar.
type Parsed = Option<(CstLanguage, Arc<Tree>)>;

impl ParseCache {
    /// The trees of `files`, in order. Files not seen with this text are
    /// parsed in parallel; entries of files no longer listed are dropped.
    fn trees(&mut self, files: &[(&Path, &str)]) -> Vec<Parsed> {
        profiling::function_scope!();
        let hashes: Vec<u64> = files.par_iter().map(|(_, text)| hash_text(text)).collect();
        let missing: Vec<usize> = (0..files.len())
            .filter(|&i| self.0.get(files[i].0).is_none_or(|(hash, _)| *hash != hashes[i]))
            .collect();
        let parsed: Vec<Parsed> = missing
            .par_iter()
            .map(|&i| parse_tree(files[i].0, files[i].1).map(|(language, tree)| (language, Arc::new(tree))))
            .collect();
        for (&i, tree) in missing.iter().zip(parsed) {
            self.0.insert(files[i].0.to_path_buf(), (hashes[i], tree));
        }
        let listed: std::collections::HashSet<&Path> = files.iter().map(|(path, _)| *path).collect();
        self.0.retain(|path, _| listed.contains(path.as_path()));
        files.iter().map(|(path, _)| self.0[*path].1.clone()).collect()
    }

    #[cfg(test)]
    fn tree_of(&self, path: &Path) -> Option<Arc<Tree>> {
        self.0.get(path)?.1.as_ref().map(|(_, tree)| tree.clone())
    }
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn analyze(root: &Path, open: &OpenFiles<'_>, cache: &mut ParseCache) -> LocalAnalysis {
    analyze_with_configs(root, open, &working_configs(root, open), cache)
}

fn analyze_with_configs(
    root: &Path,
    open: &OpenFiles<'_>,
    configs: &ConfigSources,
    cache: &mut ParseCache,
) -> LocalAnalysis {
    let aliases: BTreeMap<_, _> = configs.iter()
        .filter(|(path, _)| path.file_name().is_some_and(|name| name == "tsconfig.json"))
        .map(|(path, text)| (path, TsPaths::parse(path, text)))
        .collect();
    let files: Vec<(&Path, &str)> = open.iter().filter(|(p, _)| p.starts_with(root)).collect();
    let trees = cache.trees(&files);
    // Each file is analysed on its own; only the targets it resolves to need
    // the other open files, and those are read-only.
    profiling::scope!("local links of every file");
    let per_file: Vec<LocalAnalysis> = files
        .par_iter()
        .zip(trees.par_iter())
        .map(|(&(path, text), parsed)| {
            let mut result = LocalAnalysis::default();
            let nearest_tsconfig = ancestors(root, path).find_map(|dir| aliases.get(&dir.join("tsconfig.json")));
            let mut cx = Ctx {
                root,
                open,
                path,
                text,
                language: parsed.as_ref().map(|(language, _)| *language),
                config: FileConfig {
                    paths: nearest_tsconfig.and_then(Option::as_ref),
                    tsconfig_unreadable: nearest_tsconfig.is_some_and(Option::is_none),
                    go_mod: None,
                },
            };
            if let Some((language, tree)) = parsed {
                let language = *language;
                // The module identity of Go code: the nearest go.mod, read from
                // disk like a tsconfig, or the open one when it is on the canvas.
                if language == CstLanguage::Go {
                    cx.config.go_mod = ancestors(root, path).find_map(|dir| {
                        let manifest = dir.join("go.mod");
                        configs
                            .get_key_value(&manifest)
                            .map(|(path, text)| (path.as_path(), text.as_str()))
                            .or_else(|| open.get(&manifest))
                    });
                }
                if language == CstLanguage::Rust {
                    rust_modules::analyze(root, open, (path, text), tree.root_node(), &mut result);
                }
                let mut pending = vec![tree.root_node()];
                while let Some(node) = pending.pop() {
                    if !node.has_error() {
                        syntax_links(&cx, node, &mut result);
                    }
                    let mut cursor = node.walk();
                    pending.extend(node.named_children(&mut cursor));
                }
            }
            match path.extension().and_then(|e| e.to_str()) {
                Some("md" | "mdx") => markdown_links(&cx, &mut result),
                Some("json") => json_links(&cx, &mut result),
                _ => {}
            }
            result
        })
        .collect();
    let mut result = LocalAnalysis::default();
    for part in per_file {
        result.links.extend(part.links);
        result.issues.extend(part.issues);
    }
    result.links.sort_by(|a, b| (&a.source, a.usage.start, &a.target).cmp(&(&b.source, b.usage.start, &b.target)));
    result.links.dedup_by(|a, b| a.source == b.source && a.target == b.target && a.source_name_span == b.source_name_span && a.relationship == b.relationship);
    result.issues.finish();
    result
}

/// What an import is resolved with: the file it is in, the folder, and the
/// configuration nearest to the file.
struct Ctx<'a, 'o> {
    root: &'a Path,
    open: &'a OpenFiles<'o>,
    path: &'a Path,
    text: &'a str,
    /// `None` for Markdown and JSON.
    language: Option<CstLanguage>,
    config: FileConfig<'a>,
}

#[derive(Clone, Copy, Default)]
struct FileConfig<'a> {
    /// `compilerOptions.paths` of the nearest tsconfig, when it could be read.
    paths: Option<&'a TsPaths>,
    /// A tsconfig exists but could not be read (no `compilerOptions`, invalid):
    /// a bare import might be one of its aliases.
    tsconfig_unreadable: bool,
    /// The nearest go.mod: its path and text.
    go_mod: Option<(&'a Path, &'a str)>,
}

impl Ctx<'_, '_> {
    /// Records that the import written at byte `at` gave no link. A package
    /// is only counted.
    fn report(&self, out: &mut LocalAnalysis, at: usize, specifier: &str, reason: ImportIssueReason) {
        if reason == ImportIssueReason::External {
            out.issues.external();
            return;
        }
        let line = self.text.get(..at).map_or(1, |before| before.bytes().filter(|b| *b == b'\n').count() + 1);
        out.issues.report(ImportIssue {
            path: self.path.strip_prefix(self.root).unwrap_or(self.path).to_path_buf(),
            line,
            specifier: specifier.to_owned(),
            language: self.language,
            reason,
        });
    }
}

fn syntax_links(cx: &Ctx<'_, '_>, node: Node, out: &mut LocalAnalysis) {
    let Some(language) = cx.language else { return };
    let (root, open, path, text) = (cx.root, cx.open, cx.path, cx.text);
    let raw = |n: Node| n.utf8_text(text.as_bytes()).unwrap_or("").to_string();
    match language {
        CstLanguage::TypeScript | CstLanguage::Tsx | CstLanguage::JavaScript => {
            if matches!(node.kind(), "import_statement" | "export_statement")
                && let Some(source) = node.child_by_field_name("source")
                && let Some(value) = literal(&raw(source)) {
                add_js_import(cx, value, source.start_byte() + 1, out);
            }
            // `import()` is a grammar keyword, unlike shadowable require().
            if node.kind() == "call_expression"
                && node.child_by_field_name("function").is_some_and(|n| n.kind() == "import")
                && let Some(args) = node.child_by_field_name("arguments")
                && args.named_child_count() == 1
                && let Some(source) = args.named_child(0)
                && let Some(value) = literal(&raw(source)) {
                add_js_import(cx, value, source.start_byte() + 1, out);
            }
        }
        CstLanguage::Rust => {
            if node.kind() == "macro_invocation"
                && node.child_by_field_name("macro").is_some_and(|n| matches!(raw(n).as_str(), "include_str" | "include_bytes")) {
                let mut cursor = node.walk();
                if let Some(tokens) = node.named_children(&mut cursor).find(|n| n.kind() == "token_tree")
                    && tokens.named_child_count() == 1
                    && let Some(source) = tokens.named_child(0)
                    && let Some(value) = literal(&raw(source)) {
                    add_relative(cx, value, source.start_byte() + 1, false, out);
                }
            }
        }
        CstLanguage::Python => {
            if node.kind() == "import_from_statement"
                && let Some(module) = node.child_by_field_name("module_name") {
                let value = raw(module);
                let at = module.start_byte();
                let dots = value.bytes().take_while(|b| *b == b'.').count();
                // An absolute package is a library, or needs the environment to tell.
                if dots == 0 { return cx.report(out, at, &value, ImportIssueReason::External); }
                let Some(mut base) = path.parent() else { return };
                for _ in 1..dots {
                    let Some(parent) = base.parent() else {
                        return cx.report(out, at, &value, ImportIssueReason::OutsideWorkspace);
                    };
                    base = parent;
                }
                let module_path = value[dots..].replace('.', "/");
                // `from . import x` names the package itself, not a file.
                if module_path.is_empty() { return cx.report(out, at, &value, ImportIssueReason::Unsupported); }
                let base = base.join(module_path);
                if !base.starts_with(root) { return cx.report(out, at, &value, ImportIssueReason::OutsideWorkspace); }
                match resolve(root, open, [base.with_extension("py"), base.join("__init__.py")]) {
                    Ok(target) => push(path, text, &target, module.start_byte(), module.end_byte(), RelationshipType::Imports, &mut out.links),
                    Err(reason) => cx.report(out, at, &value, reason),
                }
            }
        }
        CstLanguage::Go => {
            if node.kind() == "import_spec"
                && let Some(source) = node.child_by_field_name("path")
                && let Some(value) = literal(&raw(source)) {
                let at = source.start_byte() + 1;
                // The nearest go.mod establishes the module identity.
                let Some((manifest, content)) = cx.config.go_mod else {
                    return cx.report(out, at, value, ImportIssueReason::MissingContext);
                };
                let module = content.lines().find_map(|l| l.trim().strip_prefix("module ").map(str::trim));
                // Anything outside the module is the standard library or a dependency.
                let Some(relative) = module.and_then(|m| value.strip_prefix(&format!("{m}/"))) else {
                    return cx.report(out, at, value, ImportIssueReason::External);
                };
                let Some(parent) = manifest.parent() else { return };
                let directory = parent.join(relative);
                let candidates = open.files_in_dir(&directory).into_iter().filter(|p| p.extension().is_some_and(|e| e == "go") && !p.to_string_lossy().ends_with("_test.go"));
                match resolve(root, open, candidates) {
                    Ok(target) => push(path, text, &target, source.start_byte() + 1, source.end_byte() - 1, RelationshipType::Imports, &mut out.links),
                    // A package is a folder of files, and only one can be the target.
                    Err(ImportIssueReason::Ambiguous) => cx.report(out, at, value, ImportIssueReason::Unsupported),
                    Err(reason) => cx.report(out, at, value, reason),
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

/// The one file `candidates` mean, or why there is not one.
fn resolve(root: &Path, open: &OpenFiles<'_>, candidates: impl IntoIterator<Item = PathBuf>) -> Result<PathBuf, ImportIssueReason> {
    match candidates_in(root, open, candidates) {
        RelationCandidates::One(target) => Ok(target),
        RelationCandidates::None => Err(ImportIssueReason::NotFound),
        RelationCandidates::Many => Err(ImportIssueReason::Ambiguous),
    }
}

enum RelationCandidates {
    None,
    One(PathBuf),
    Many,
}

fn candidates_in(root: &Path, open: &OpenFiles<'_>, candidates: impl IntoIterator<Item = PathBuf>) -> RelationCandidates {
    let mut found: Option<PathBuf> = None;
    for candidate in candidates {
        let Some(candidate) = normalize(&candidate).filter(|p| p.starts_with(root)) else { continue };
        let actual = match open.get(&candidate) {
            Some((actual, _)) => {
                if let (Ok(root), Ok(real)) = (root.canonicalize(), actual.canonicalize())
                    && !real.starts_with(root) { continue; }
                actual.to_path_buf()
            }
            // A file of the folder that is not open. The index lists real
            // files below the root (symbolic links are not followed), so it
            // needs no check against the root.
            None if open.is_indexed(&candidate) => candidate,
            None => continue,
        };
        match found.as_deref() {
            Some(previous) if previous != actual => return RelationCandidates::Many,
            Some(_) => {}
            None => found = Some(actual),
        }
    }
    found.map_or(RelationCandidates::None, RelationCandidates::One)
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

    fn target(&self, value: &str) -> TsTarget {
        let (targets, capture) = if let Some(targets) = self.paths.get(value) {
            (targets, "")
        } else {
            let Some((_, targets, capture)) = self.paths.iter().filter_map(|(key, targets)| {
                let (prefix, suffix) = key.split_once('*')?;
                if suffix.contains('*') { return None; }
                let capture = value.strip_prefix(prefix)?.strip_suffix(suffix)?;
                Some((prefix.len(), targets, capture))
            }).max_by_key(|(prefix, _, _)| *prefix) else { return TsTarget::NoMatch };
            (targets, capture)
        };
        // Ordered fallback lists would require knowing whether each target
        // exists even when it isn't on the canvas or in this Git snapshot.
        let [target] = targets.as_slice() else { return TsTarget::Several };
        let target = target.replacen('*', capture, 1);
        let target = Path::new(&target);
        if target.is_relative() && !target.to_string_lossy().contains(['*', ':', '\\', '%']) {
            TsTarget::Path(self.base.join(target))
        } else {
            TsTarget::Unsupported
        }
    }
}

/// What the `paths` of a tsconfig say about an import.
enum TsTarget {
    /// No key matches: a package, or a path the config does not know.
    NoMatch,
    /// The key lists several fallbacks, which need the disk to pick from.
    Several,
    /// The target is absolute or has a form that is not followed.
    Unsupported,
    Path(PathBuf),
}

fn add_js_import(cx: &Ctx<'_, '_>, value: &str, offset: usize, out: &mut LocalAnalysis) {
    if value.starts_with('.') {
        return add_relative(cx, value, offset, true, out);
    }
    if value.starts_with("node:") { return cx.report(out, offset, value, ImportIssueReason::External); }
    if value.contains([':', '?', '\\', '%', '#']) || value.starts_with('/') {
        return cx.report(out, offset, value, ImportIssueReason::Unsupported);
    }
    let base = match cx.config.paths.map(|paths| paths.target(value)) {
        // A package, unless an alias the config could not give us.
        None | Some(TsTarget::NoMatch) if cx.config.tsconfig_unreadable => {
            return cx.report(out, offset, value, ImportIssueReason::MissingContext);
        }
        None | Some(TsTarget::NoMatch) => return cx.report(out, offset, value, ImportIssueReason::External),
        Some(TsTarget::Several) => return cx.report(out, offset, value, ImportIssueReason::MissingContext),
        Some(TsTarget::Unsupported) => return cx.report(out, offset, value, ImportIssueReason::Unsupported),
        Some(TsTarget::Path(base)) => base,
    };
    match resolve(cx.root, cx.open, module_candidates(&base)) {
        Ok(target) => push(cx.path, cx.text, &target, offset, offset + value.len(), RelationshipType::Imports, &mut out.links),
        Err(reason) => cx.report(out, offset, value, reason),
    }
}

/// A relative path in code (`module`) or in a document. Only code reports
/// what it could not resolve: documents link to anything.
fn add_relative(cx: &Ctx<'_, '_>, value: &str, offset: usize, module: bool, out: &mut LocalAnalysis) {
    let value = value.split('#').next().unwrap_or("");
    if value.is_empty() { return; }
    if value.contains([':', '?', '\\', '%']) || value.starts_with('/') {
        if module { cx.report(out, offset, value, ImportIssueReason::Unsupported); }
        return;
    }
    if module && !value.starts_with('.') { return; }
    let Some(parent) = cx.path.parent() else { return };
    let base = parent.join(value);
    if module && normalize(&base).is_none_or(|path| !path.starts_with(cx.root)) {
        return cx.report(out, offset, value, ImportIssueReason::OutsideWorkspace);
    }
    let candidates = if module { module_candidates(&base) } else { vec![base] };
    match resolve(cx.root, cx.open, candidates) {
        Ok(target) => {
            let relationship = if module { RelationshipType::Imports } else { RelationshipType::References };
            push(cx.path, cx.text, &target, offset, offset + value.len(), relationship, &mut out.links);
        }
        Err(reason) if module => cx.report(out, offset, value, reason),
        Err(_) => {}
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

fn markdown_links(cx: &Ctx<'_, '_>, out: &mut LocalAnalysis) {
    let text = cx.text;
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
                add_relative(cx, value, offset + start, false, out);
            }
        }
        offset += line.len();
    }
}

fn json_links(cx: &Ctx<'_, '_>, out: &mut LocalAnalysis) {
    let text = cx.text;
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
            if reference_value { add_relative(cx, value, start + 1, false, out); }
            reference_value = matches!(value, "$ref" | "extends") && text[i..].trim_start().starts_with(':')
                && text[i..].trim_start().strip_prefix(':').is_some_and(|s| s.trim_start().starts_with('"'));
        } else { reference_value = false; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edges::style::RelationshipType::{Imports, References};

    fn analyze_files(files: &[(&str, &str)], cache: &mut ParseCache) -> Vec<(PathBuf, PathBuf)> {
        let open = OpenFiles::new(files.iter().map(|(p, t)| (Path::new(p), *t)));
        let result = analyze_with_configs(Path::new("/p"), &open, &ConfigSources::new(), cache);
        result.links.into_iter().map(|l| (l.source, l.target)).collect()
    }

    #[test]
    fn a_warm_parse_cache_gives_the_links_of_a_cold_one() {
        let files = [
            ("/p/app.ts", "import { a } from './a';\nexport * from './b';"),
            ("/p/a.ts", "export const a = 1;"),
            ("/p/b.ts", "export const b = 1;"),
            ("/p/README.md", "[a](a.ts)"),
        ];
        let cold = analyze_files(&files, &mut ParseCache::default());
        let mut cache = ParseCache::default();
        analyze_files(&files, &mut cache);
        assert_eq!(analyze_files(&files, &mut cache), cold);
        assert_eq!(cold.len(), 3);
    }

    #[test]
    fn the_parse_cache_keeps_a_tree_until_its_text_changes() {
        let mut cache = ParseCache::default();
        let before = [("/p/app.ts", "import { a } from './a';"), ("/p/a.ts", "export const a = 1;")];
        assert_eq!(analyze_files(&before, &mut cache).len(), 1);
        let first = cache.tree_of(Path::new("/p/app.ts")).unwrap();
        // Another file opens: the tree of the unchanged one is reused.
        let more = [before[0], before[1], ("/p/b.ts", "export const b = 1;")];
        analyze_files(&more, &mut cache);
        assert!(Arc::ptr_eq(&first, &cache.tree_of(Path::new("/p/app.ts")).unwrap()));
        // Its text changes: a new tree, and the new link.
        let edited = [("/p/app.ts", "import { b } from './b';"), more[1], more[2]];
        let links = analyze_files(&edited, &mut cache);
        assert!(!Arc::ptr_eq(&first, &cache.tree_of(Path::new("/p/app.ts")).unwrap()));
        assert_eq!(links, [(PathBuf::from("/p/app.ts"), PathBuf::from("/p/b.ts"))]);
    }

    #[test]
    fn the_parse_cache_drops_files_that_are_no_longer_open() {
        let mut cache = ParseCache::default();
        analyze_files(&[("/p/app.ts", "import './a';"), ("/p/a.ts", "export {};")], &mut cache);
        assert!(cache.tree_of(Path::new("/p/a.ts")).is_some());
        analyze_files(&[("/p/app.ts", "import './a';")], &mut cache);
        assert!(cache.tree_of(Path::new("/p/a.ts")).is_none());
        assert!(cache.tree_of(Path::new("/p/app.ts")).is_some());
    }

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
    fn rust_module_paths_survive_attributes_and_follow_inline_scopes() {
        let files = [
            ("/p/src/lib.rs", r##"
#![cfg_attr(all(not(feature = "std"), not(test)), no_std)]
mod plain;
#[doc(hidden)]
// A regular comment does not detach the attribute.
mod documented;
#[macro_use]
mod macros;
mod outer { mod inner { mod nested; } }
mod inline { #[path = "other.rs"] mod explicit; }
#[path = r#"shared/actual.rs"#] mod renamed;
#[path = "relocated"] mod relocated { mod inside; }
"##),
            ("/p/src/plain.rs", "#[path = \"file_sibling.rs\"] mod explicit;\nmod inline { mod nested; }"),
            ("/p/src/documented.rs", ""),
            ("/p/src/macros.rs", ""),
            ("/p/src/outer/inner/nested/mod.rs", ""),
            ("/p/src/inline/other.rs", ""),
            ("/p/src/shared/actual.rs", ""),
            ("/p/src/relocated/inside.rs", ""),
            ("/p/src/file_sibling.rs", ""),
            ("/p/src/plain/inline/nested.rs", ""),
        ];
        let open = OpenFiles::new(files.iter().map(|(p, t)| (Path::new(p), *t)));
        let links = links(Path::new("/p"), &open);
        assert_eq!(links.iter().map(|link| link.target.as_path()).collect::<Vec<_>>(), [
            Path::new("/p/src/plain.rs"),
            Path::new("/p/src/documented.rs"),
            Path::new("/p/src/macros.rs"),
            Path::new("/p/src/outer/inner/nested/mod.rs"),
            Path::new("/p/src/inline/other.rs"),
            Path::new("/p/src/shared/actual.rs"),
            Path::new("/p/src/relocated/inside.rs"),
            Path::new("/p/src/file_sibling.rs"),
            Path::new("/p/src/plain/inline/nested.rs"),
        ]);
    }

    #[test]
    fn rust_module_paths_do_not_guess_across_conditional_or_ambiguous_scopes() {
        let files = [
            ("/p/src/lib.rs", r#"
#[cfg(feature = "optional")]
// This comment does not detach cfg.
mod optional;
#[cfg_attr(test, path = "other.rs")]
mod conditional;
#[cfg(unix)] mod gated { mod nested; }
mod ambiguous;
#[path = "../../outside.rs"] mod escape;
#[path = "gone.rs"] mod missing;
"#),
            ("/p/src/optional.rs", ""),
            ("/p/src/conditional.rs", ""),
            ("/p/src/other.rs", ""),
            ("/p/src/gated/nested.rs", ""),
            ("/p/other/lib.rs", "#![cfg_attr(test, cfg(unix))]\nmod nested;"),
            ("/p/other/nested.rs", ""),
            ("/p/src/ambiguous.rs", ""),
            ("/p/src/ambiguous/mod.rs", ""),
            ("/p/outside.rs", ""),
            ("/outside.rs", ""),
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

    /// Links of `files` (all open) when the folder also holds `closed`.
    fn analyze_with_folder(files: &[(&str, &str)], closed: &[&str]) -> Vec<(PathBuf, PathBuf)> {
        let open = OpenFiles::new(files.iter().map(|(p, t)| (Path::new(p), *t)));
        let paths: Vec<PathBuf> = closed.iter().map(PathBuf::from).collect();
        let index = crate::analysis::dependencies::WorkspaceIndex::new(&paths);
        let open = open.with_index(&index);
        let result = analyze_with_configs(Path::new("/p"), &open, &ConfigSources::new(), &mut ParseCache::default());
        result.links.into_iter().map(|l| (l.source, l.target)).collect()
    }

    #[test]
    fn an_import_resolves_to_a_file_of_the_folder_that_is_not_open() {
        let app = ("/p/web/app.ts", "import { a } from './a';\nimport './sub';\n");
        let only_app = analyze_files(&[app], &mut ParseCache::default());
        assert!(only_app.is_empty(), "without the folder, nothing to resolve to");

        let links = analyze_with_folder(&[app], &["/p/web/a.ts", "/p/web/sub/index.ts"]);
        let targets: Vec<&str> = links.iter().map(|(_, t)| t.to_str().unwrap()).collect();
        assert_eq!(targets, ["/p/web/a.ts", "/p/web/sub/index.ts"]);
        assert!(links.iter().all(|(s, _)| s == Path::new("/p/web/app.ts")), "links still start in open files");

        // Closed files are never sources.
        assert!(analyze_with_folder(&[("/p/web/other.ts", "export {};\n")], &["/p/web/app.ts", "/p/web/a.ts"]).is_empty());
    }

    #[test]
    fn the_whole_folder_decides_what_is_ambiguous() {
        let app = ("/p/app.ts", "import './x';\n");
        assert_eq!(analyze_with_folder(&[app, ("/p/x.ts", "")], &[]).len(), 1);
        // x.js exists in the folder too, though it is not open: two targets.
        assert!(analyze_with_folder(&[app, ("/p/x.ts", "")], &["/p/x.js"]).is_empty());
        // A file open on the canvas and listed in the folder is one file.
        assert_eq!(analyze_with_folder(&[app, ("/p/x.ts", "")], &["/p/x.ts"]).len(), 1);
    }

    #[test]
    fn go_packages_and_python_modules_resolve_to_closed_files_too() {
        let main = ("/p/cmd/main.go", "package main\nimport \"example.com/m/util\"\n");
        let gomod = ("/p/go.mod", "module example.com/m\n");
        let links = analyze_with_folder(&[main, gomod], &["/p/util/util.go", "/p/util/util_test.go"]);
        assert_eq!(links, vec![("/p/cmd/main.go".into(), "/p/util/util.go".into())], "tests are not the package");

        let py = ("/p/pkg/a.py", "from .b import f\n");
        assert_eq!(analyze_with_folder(&[py], &["/p/pkg/b.py"]).len(), 1);
    }

    #[test]
    fn go_mod_is_read_from_configs_without_being_open() {
        let main = ("/p/cmd/main.go", "package main\nimport \"example.com/m/util\"\n");
        let open = OpenFiles::new([(Path::new(main.0), main.1)]);
        let paths = [PathBuf::from("/p/util/util.go")];
        let index = crate::analysis::dependencies::WorkspaceIndex::new(&paths);
        let open = open.with_index(&index);
        let analyze = |configs: &ConfigSources| {
            analyze_with_configs(Path::new("/p"), &open, configs, &mut ParseCache::default()).links.len()
        };
        assert_eq!(analyze(&ConfigSources::new()), 0, "no go.mod anywhere: no module identity");
        let configs = ConfigSources::from([(PathBuf::from("/p/go.mod"), "module example.com/m\n".to_string())]);
        assert_eq!(analyze(&configs), 1);
        // A go.mod nearer the file wins over a farther one.
        let nested = ConfigSources::from([
            (PathBuf::from("/p/go.mod"), "module example.com/m\n".to_string()),
            (PathBuf::from("/p/cmd/go.mod"), "module other.org/x\n".to_string()),
        ]);
        assert_eq!(analyze(&nested), 0);
    }

    #[test]
    fn config_paths_name_the_go_mod_above_a_go_file() {
        let paths = config_paths(Path::new("/p"), [Path::new("/p/cmd/main.go"), Path::new("/p/web/a.ts"), Path::new("/p/x.py")]);
        let names: Vec<&str> = paths.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(names, ["/p/cmd/go.mod", "/p/go.mod", "/p/tsconfig.json", "/p/web/tsconfig.json"], "every folder from the file up to the root");
    }

    // ---- why an import gave no link (TASK-92) ----

    /// `(path, line, specifier, reason)` of the listed issues, and the number
    /// of packages counted, of `files` (open) with `closed` in the folder.
    fn issues_of(
        files: &[(&str, &str)],
        closed: &[&str],
        configs: &ConfigSources,
    ) -> (Vec<(String, usize, String, ImportIssueReason)>, usize) {
        let open = OpenFiles::new(files.iter().map(|(p, t)| (Path::new(p), *t)));
        let paths: Vec<PathBuf> = closed.iter().map(PathBuf::from).collect();
        let index = crate::analysis::dependencies::WorkspaceIndex::new(&paths);
        let open = open.with_index(&index);
        let issues = analyze_with_configs(Path::new("/p"), &open, configs, &mut ParseCache::default()).issues;
        let listed = issues
            .items()
            .iter()
            .map(|i| (i.path.to_str().unwrap().to_owned(), i.line, i.specifier.clone(), i.reason))
            .collect();
        (listed, issues.count(ImportIssueReason::External))
    }

    use ImportIssueReason as Why;

    fn row(path: &str, line: usize, specifier: &str, reason: Why) -> (String, usize, String, Why) {
        (path.to_owned(), line, specifier.to_owned(), reason)
    }

    #[test]
    fn js_imports_say_why_they_gave_no_link() {
        let app = "import React from 'react';\nimport fs from 'node:fs';\nimport './missing';\nimport '../../outside';\n\
                   import 'https://cdn.example/x.js';\nimport '#internal/x';\nimport './x';\nimport './fine';\n";
        let files = [("/p/src/app.ts", app), ("/p/src/x.ts", ""), ("/p/src/fine.ts", "")];
        let (listed, external) = issues_of(&files, &["/p/src/x.js"], &ConfigSources::new());
        assert_eq!(external, 2, "react and node:fs are counted, not listed");
        assert_eq!(
            listed,
            vec![
                row("src/app.ts", 3, "./missing", Why::NotFound),
                row("src/app.ts", 4, "../../outside", Why::OutsideWorkspace),
                row("src/app.ts", 5, "https://cdn.example/x.js", Why::Unsupported),
                row("src/app.ts", 6, "#internal/x", Why::Unsupported),
                row("src/app.ts", 7, "./x", Why::Ambiguous),
            ],
            "line numbers are 1-based and ./fine, which resolves, is not an issue"
        );
    }

    #[test]
    fn tsconfig_paths_tell_aliases_from_packages_and_missing_context() {
        let app = "import a from '@/a';\nimport b from '@/gone';\nimport _ from 'lodash';\nimport m from 'multi/x';\n";
        let config = r#"{"compilerOptions": {"paths": {"@/*": ["src/*"], "multi/*": ["a/*", "b/*"]}}}"#;
        let configs = ConfigSources::from([(PathBuf::from("/p/tsconfig.json"), config.to_string())]);
        let (listed, external) = issues_of(&[("/p/app.ts", app)], &["/p/src/a.ts"], &configs);
        assert_eq!(external, 1, "lodash matches no alias");
        assert_eq!(
            listed,
            vec![row("app.ts", 2, "@/gone", Why::NotFound), row("app.ts", 4, "multi/x", Why::MissingContext)],
            "a key with several fallbacks needs the disk to pick"
        );

        // A tsconfig that could not be read might hold the alias of a bare import.
        let unreadable = ConfigSources::from([(PathBuf::from("/p/tsconfig.json"), r#"{"extends": "./base.json"}"#.to_string())]);
        let (listed, external) = issues_of(&[("/p/app.ts", "import x from 'maybe-an-alias';\n")], &[], &unreadable);
        assert_eq!((listed, external), (vec![row("app.ts", 1, "maybe-an-alias", Why::MissingContext)], 0));
    }

    #[test]
    fn python_imports_say_why_they_gave_no_link() {
        let code = "from os import path\nfrom . import sibling\nfrom .gone import f\nfrom .....far import g\nfrom .fine import h\n";
        let (listed, external) = issues_of(&[("/p/pkg/a.py", code), ("/p/pkg/fine.py", "")], &[], &ConfigSources::new());
        assert_eq!(external, 1, "an absolute package");
        assert_eq!(
            listed,
            vec![
                row("pkg/a.py", 2, ".", Why::Unsupported),
                row("pkg/a.py", 3, ".gone", Why::NotFound),
                row("pkg/a.py", 4, ".....far", Why::OutsideWorkspace),
            ]
        );
    }

    #[test]
    fn go_imports_say_why_they_gave_no_link() {
        let main = "package main\nimport (\n\t\"fmt\"\n\t\"example.com/m/util\"\n\t\"example.com/m/nodir\"\n\t\"example.com/m/pkg\"\n)\n";
        let gomod = ConfigSources::from([(PathBuf::from("/p/go.mod"), "module example.com/m\n".to_string())]);
        let files = [("/p/cmd/main.go", main)];
        let closed = ["/p/util/u.go", "/p/pkg/a.go", "/p/pkg/b.go"];
        let (listed, external) = issues_of(&files, &closed, &gomod);
        assert_eq!(external, 1, "the standard library");
        assert_eq!(
            listed,
            vec![
                row("cmd/main.go", 5, "example.com/m/nodir", Why::NotFound),
                row("cmd/main.go", 6, "example.com/m/pkg", Why::Unsupported),
            ],
            "a package of several files has no single target (it is not an ambiguity)"
        );
        let (listed, _) = issues_of(&files, &closed, &ConfigSources::new());
        assert_eq!(listed.len(), 4, "without a go.mod nothing can be told: {listed:?}");
        assert!(listed.iter().all(|(_, _, _, reason)| *reason == Why::MissingContext));
    }

    #[test]
    fn rust_modules_use_the_same_reasons() {
        let main = "mod util;\nmod gone;\n#[cfg(test)] mod gated;\n#[path = \"../../escape.rs\"] mod out;\n";
        let (listed, _) = issues_of(&[("/p/src/main.rs", main), ("/p/src/util.rs", "")], &[], &ConfigSources::new());
        assert_eq!(
            listed,
            vec![
                row("src/main.rs", 2, "gone", Why::NotFound),
                row("src/main.rs", 3, "gated", Why::Conditional),
                row("src/main.rs", 4, "out", Why::OutsideWorkspace),
            ]
        );
    }

    #[test]
    fn documents_do_not_report_broken_links() {
        let md = "[gone](./nowhere.md) and [site](https://example.com) and [ok](./b.md)\n";
        let (listed, external) = issues_of(&[("/p/a.md", md), ("/p/b.md", "")], &[], &ConfigSources::new());
        assert!(listed.is_empty() && external == 0, "{listed:?}");
        assert!(issues_of(&[("/p/c.json", r#"{"$ref": "./nope.json"}"#)], &[], &ConfigSources::new()).0.is_empty());
    }

    #[test]
    fn many_issues_are_capped_but_counted_exactly() {
        use issues::{ImportIssues, MAX_ISSUES};
        let imports: String = (0..600).map(|i| format!("import './gone{i}';\n")).collect();
        let open = OpenFiles::new([(Path::new("/p/app.ts"), imports.as_str())]);
        let issues = analyze_with_configs(Path::new("/p"), &open, &ConfigSources::new(), &mut ParseCache::default()).issues;
        assert_eq!(issues.items().len(), MAX_ISSUES);
        assert_eq!((issues.count(Why::NotFound), issues.unresolved()), (600, 600), "the counts stay exact");
        assert_eq!(issues.items()[0].line, 1, "the first ones by place are kept");

        let mut sums = ImportIssues::default();
        sums.external();
        sums.external();
        assert_eq!((sums.count(Why::External), sums.unresolved(), sums.items().len()), (2, 0, 0), "packages are not listed");
    }
}
