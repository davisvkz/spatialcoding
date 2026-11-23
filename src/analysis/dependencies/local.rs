//! Evidence-only local module and document links; independent of LSP/runtime.

use rayon::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tree_sitter::{Node, Tree};

use super::{DependencyLink, LineSpan, OpenFiles};
use crate::analysis::cst::{CstLanguage, parse_tree};
use crate::edges::style::RelationshipType;

mod exports;
mod issues;
mod js_config;
mod rust_modules;
pub(crate) use issues::{ImportIssue, ImportIssueReason, ImportIssues};
pub(crate) use js_config::is_config_name;
use js_config::{Package, TsPaths, TsTarget};

pub(crate) type ConfigSources = BTreeMap<PathBuf, String>;
const MAX_CONFIG_BYTES: usize = 256 * 1024;

/// Candidate project configs, including absent paths that may exist in Git:
/// for each JS or TS file, the tsconfig, jsconfig and Vite config of every
/// folder above it; for each Go file, the go.mod.
pub(crate) fn config_paths<'a>(
    root: &'a Path,
    paths: impl IntoIterator<Item = &'a Path>,
) -> BTreeSet<PathBuf> {
    paths.into_iter()
        .filter_map(|p| {
            let names: Vec<&str> = match CstLanguage::from_path(p)? {
                CstLanguage::TypeScript | CstLanguage::Tsx | CstLanguage::JavaScript => {
                    js_config::TS_CONFIG_NAMES.iter().chain(&js_config::VITE_CONFIG_NAMES).copied().collect()
                }
                CstLanguage::Go => vec!["go.mod"],
                _ => return None,
            };
            let found: Vec<PathBuf> = ancestors(root, p)
                .flat_map(|dir| names.iter().map(|name| dir.join(name)).collect::<Vec<_>>())
                .collect();
            Some(found)
        })
        .flatten()
        .collect()
}

fn ancestors<'a>(root: &'a Path, path: &'a Path) -> impl Iterator<Item = &'a Path> {
    path.ancestors().skip(1).take_while(move |dir| dir.starts_with(root))
}

/// Read closed configs, but prefer the snapshot for configs on the canvas.
/// Besides the candidates of [`config_paths`], the local configs they extend
/// (up to a few hops) and the `package.json` of the folder's packages are read.
pub(crate) fn working_configs(root: &Path, open: &OpenFiles<'_>) -> ConfigSources {
    let Ok(canonical_root) = root.canonicalize() else { return ConfigSources::new() };
    let read = |path: &Path| -> Option<String> {
        let actual = path.canonicalize().ok()?;
        if !actual.starts_with(&canonical_root) || !actual.is_file() { return None; }
        let text = if let Some((_, text)) = open.get(path) {
            text.to_owned()
        } else {
            let mut text = String::new();
            let _ = std::fs::File::open(actual).ok()?.take(MAX_CONFIG_BYTES as u64 + 1)
                .read_to_string(&mut text).ok()?;
            text
        };
        (text.len() <= MAX_CONFIG_BYTES).then_some(text)
    };
    let mut configs = ConfigSources::new();
    let mut queue: Vec<(PathBuf, usize)> =
        config_paths(root, open.iter().map(|(p, _)| p)).into_iter().map(|p| (p, 0)).collect();
    while let Some((path, depth)) = queue.pop() {
        if configs.contains_key(&path) { continue; }
        let Some(text) = read(&path) else { continue };
        // A file reached through `extends` can have any name and extend another.
        let is_ts = depth > 0 || path.file_name().and_then(|n| n.to_str()).is_some_and(js_config::is_ts_config_name);
        if is_ts && depth < js_config::MAX_EXTENDS_DEPTH {
            queue.extend(js_config::extended_paths(root, &path, &text).into_iter().map(|p| (p, depth + 1)));
        }
        configs.insert(path, text);
    }
    for path in open.indexed_named("package.json").into_iter().take(js_config::MAX_PACKAGES) {
        if let Some(text) = read(&path) { configs.insert(path, text); }
    }
    configs
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
    /// What this file imports by name and exports (see [`exports`]).
    facts: exports::FileFacts,
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
    let name_of = |path: &Path| path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
    let aliases: BTreeMap<_, _> = configs.keys()
        .filter(|path| js_config::TS_CONFIG_NAMES.contains(&name_of(path).as_str()))
        .map(|path| (path, TsPaths::parse(root, path, configs)))
        .collect();
    let vite: BTreeMap<_, _> = configs.iter()
        .filter(|(path, _)| js_config::VITE_CONFIG_NAMES.contains(&name_of(path).as_str()))
        .map(|(path, text)| (path, js_config::vite_aliases(path, text)))
        .collect();
    let packages = js_config::packages(configs);
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
            let nearest_tsconfig = ancestors(root, path)
                .find_map(|dir| js_config::TS_CONFIG_NAMES.iter().find_map(|name| aliases.get(&dir.join(name))));
            let nearest_vite = ancestors(root, path)
                .find_map(|dir| js_config::VITE_CONFIG_NAMES.iter().find_map(|name| vite.get(&dir.join(name))));
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
                    vite: nearest_vite.map_or(&[], Vec::as_slice),
                    packages: &packages,
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
                    exports::record_rust_items(tree.root_node(), text, &mut result.facts);
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
    let mut facts: HashMap<PathBuf, exports::FileFacts> = HashMap::new();
    for (part, (path, _)) in per_file.into_iter().zip(&files) {
        result.links.extend(part.links);
        result.issues.extend(part.issues);
        facts.insert(path.to_path_buf(), part.facts);
    }
    // Imports through a re-exporting file: the barrel hop and the shortcut.
    exports::apply(&mut result.links, &mut facts);
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
    /// `resolve.alias` of the nearest Vite config, longest alias first.
    vite: &'a [(String, PathBuf)],
    /// The packages of the folder, by their `package.json`.
    packages: &'a [Package],
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
                if let Some((target, specifier)) = add_js_import(cx, value, source.start_byte() + 1, out) {
                    exports::record_js(node, text, &target, specifier, &mut out.facts);
                }
            } else if node.kind() == "export_statement" {
                exports::record_js_local_exports(node, text, &mut out.facts);
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
            if node.kind() == "use_declaration" {
                exports::record_rust_use(node, text, &mut out.facts);
            }
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
                    Ok(target) => {
                        let specifier = push(path, text, &target, module.start_byte(), module.end_byte(), RelationshipType::Imports, &mut out.links);
                        let is_init = path.file_name().is_some_and(|n| n == "__init__.py");
                        exports::record_python(node, text, &target, specifier, is_init, &mut out.facts);
                    }
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
                    Ok(target) => {
                        push(path, text, &target, source.start_byte() + 1, source.end_byte() - 1, RelationshipType::Imports, &mut out.links);
                    }
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

/// What an import resolved to, and the span its link records for it.
type Hit = (PathBuf, Option<exports::Span>);

fn add_js_import(cx: &Ctx<'_, '_>, value: &str, offset: usize, out: &mut LocalAnalysis) -> Option<Hit> {
    if value.starts_with('.') {
        return add_relative(cx, value, offset, true, out);
    }
    if value.starts_with("node:") { cx.report(out, offset, value, ImportIssueReason::External); return None; }
    if value.contains([':', '?', '\\', '%', '#']) || value.starts_with('/') {
        cx.report(out, offset, value, ImportIssueReason::Unsupported);
        return None;
    }
    // In the order a bundler tries them: the compiler's `paths`, the
    // bundler's own aliases, then the packages of this folder.
    match cx.config.paths.map(|paths| paths.target(value)) {
        Some(TsTarget::Paths(bases)) => return resolve_in_order(cx, value, offset, &bases, out),
        Some(TsTarget::Unsupported) => {
            cx.report(out, offset, value, ImportIssueReason::Unsupported);
            return None;
        }
        Some(TsTarget::NoMatch) | None => {}
    }
    if let Some(base) = js_config::vite_base(cx.config.vite, value) {
        return resolve_in_order(cx, value, offset, &[base], out);
    }
    match js_config::package_base(cx.config.packages, value) {
        Some(Ok(base)) => return resolve_in_order(cx, value, offset, &[base], out),
        Some(Err(())) => {
            cx.report(out, offset, value, ImportIssueReason::NotFound);
            return None;
        }
        None => {}
    }
    // A package, unless an alias the config could not give us.
    let reason = if cx.config.tsconfig_unreadable { ImportIssueReason::MissingContext } else { ImportIssueReason::External };
    cx.report(out, offset, value, reason);
    None
}

/// Resolves an import to the first of `bases` that exists. With a single
/// place, nothing found is a missing file. With several, "not there" must be
/// known to move to the next one, which needs the folder's file list.
fn resolve_in_order(cx: &Ctx<'_, '_>, value: &str, offset: usize, bases: &[PathBuf], out: &mut LocalAnalysis) -> Option<Hit> {
    for base in bases {
        match candidates_in(cx.root, cx.open, module_candidates(base)) {
            RelationCandidates::One(target) => {
                let span = push(cx.path, cx.text, &target, offset, offset + value.len(), RelationshipType::Imports, &mut out.links);
                return Some((target, span));
            }
            RelationCandidates::Many => {
                cx.report(out, offset, value, ImportIssueReason::Ambiguous);
                return None;
            }
            RelationCandidates::None if bases.len() > 1 && !cx.open.has_index() => {
                cx.report(out, offset, value, ImportIssueReason::MissingContext);
                return None;
            }
            RelationCandidates::None => {}
        }
    }
    cx.report(out, offset, value, ImportIssueReason::NotFound);
    None
}

/// A relative path in code (`module`) or in a document. Only code reports
/// what it could not resolve: documents link to anything.
fn add_relative(cx: &Ctx<'_, '_>, value: &str, offset: usize, module: bool, out: &mut LocalAnalysis) -> Option<Hit> {
    let value = value.split('#').next().unwrap_or("");
    if value.is_empty() { return None; }
    if value.contains([':', '?', '\\', '%']) || value.starts_with('/') {
        if module { cx.report(out, offset, value, ImportIssueReason::Unsupported); }
        return None;
    }
    if module && !value.starts_with('.') { return None; }
    let parent = cx.path.parent()?;
    let base = parent.join(value);
    if module && normalize(&base).is_none_or(|path| !path.starts_with(cx.root)) {
        cx.report(out, offset, value, ImportIssueReason::OutsideWorkspace);
        return None;
    }
    let candidates = if module { module_candidates(&base) } else { vec![base] };
    match resolve(cx.root, cx.open, candidates) {
        Ok(target) => {
            let relationship = if module { RelationshipType::Imports } else { RelationshipType::References };
            let span = push(cx.path, cx.text, &target, offset, offset + value.len(), relationship, &mut out.links);
            Some((target, span))
        }
        Err(reason) => {
            if module { cx.report(out, offset, value, reason); }
            None
        }
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

fn push(path: &Path, text: &str, target: &Path, start: usize, end: usize, relationship: RelationshipType, out: &mut Vec<DependencyLink>) -> Option<exports::Span> {
    if path == target { return None; }
    let span = exports::span_in(text, start, end)?;
    let (line, from, to) = span;
    let _ = (from, to);
    out.push(DependencyLink {
        evidence: super::LinkEvidence::LocalSyntax,
        relationship, source: path.into(), target: target.into(), label: "file".into(), source_label: None,
        usage: LineSpan { start: line, end: line }, definition: LineSpan { start: 0, end: 0 },
        name_span: None, source_name_span: Some(span),
        kind: crate::edges::edge_store::EdgeKind::Direct,
    });
    Some(span)
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
        assert_eq!(
            names,
            [
                "/p/cmd/go.mod", "/p/go.mod",
                "/p/jsconfig.json", "/p/tsconfig.json",
                "/p/vite.config.cjs", "/p/vite.config.js", "/p/vite.config.mjs", "/p/vite.config.ts",
                "/p/web/jsconfig.json", "/p/web/tsconfig.json",
                "/p/web/vite.config.cjs", "/p/web/vite.config.js", "/p/web/vite.config.mjs", "/p/web/vite.config.ts",
            ],
            "every folder from the file up to the root: go.mod for Go, the compiler and Vite configs for JS and TS"
        );
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
            vec![row("app.ts", 2, "@/gone", Why::NotFound), row("app.ts", 4, "multi/x", Why::NotFound)],
            "both fallbacks are known not to exist, as the folder's files are known"
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

    // ---- project configuration (TASK-98) ----

    fn ts_links(files: &[(&str, &str)], closed: &[&str], configs: &[(&str, &str)]) -> Vec<(String, String)> {
        let configs: ConfigSources = configs.iter().map(|(p, t)| (PathBuf::from(p), (*t).to_owned())).collect();
        let open = OpenFiles::new(files.iter().map(|(p, t)| (Path::new(p), *t)));
        let paths: Vec<PathBuf> = closed.iter().map(PathBuf::from).collect();
        let index = crate::analysis::dependencies::WorkspaceIndex::new(&paths);
        let open = open.with_index(&index);
        analyze_with_configs(Path::new("/p"), &open, &configs, &mut ParseCache::default())
            .links
            .into_iter()
            .map(|l| (l.source.to_str().unwrap().to_owned(), l.target.to_str().unwrap().to_owned()))
            .collect()
    }

    fn link(source: &str, target: &str) -> (String, String) {
        (source.to_owned(), target.to_owned())
    }

    #[test]
    fn the_first_existing_target_of_a_paths_key_wins() {
        let config = [("/p/tsconfig.json", r#"{"compilerOptions": {"paths": {"@/*": ["gen/*", "src/*"]}}}"#)];
        let app = [("/p/app.ts", "import '@/a';\n")];
        // Only the second exists: the first is known to be absent.
        assert_eq!(ts_links(&app, &["/p/src/a.ts"], &config), [link("/p/app.ts", "/p/src/a.ts")]);
        // Both exist: the first one is the target.
        assert_eq!(ts_links(&app, &["/p/gen/a.ts", "/p/src/a.ts"], &config), [link("/p/app.ts", "/p/gen/a.ts")]);
        // Without the folder's files an absent target cannot be told from a closed one.
        let open = OpenFiles::new([(Path::new("/p/app.ts"), "import '@/a';\n")]);
        let configs: ConfigSources = config.iter().map(|(p, t)| (PathBuf::from(p), t.to_string())).collect();
        let issues = analyze_with_configs(Path::new("/p"), &open, &configs, &mut ParseCache::default()).issues;
        assert_eq!(issues.count(Why::MissingContext), 1);
    }

    #[test]
    fn jsconfig_and_extends_resolve_aliases_and_the_nearest_config_wins() {
        let configs = [
            ("/p/tsconfig.base.json", r#"{"compilerOptions": {"baseUrl": ".", "paths": {"@/*": ["src/*"]}}}"#),
            ("/p/apps/web/tsconfig.json", r#"{"extends": "../../tsconfig.base"}"#),
            ("/p/apps/old/jsconfig.json", r#"{"compilerOptions": {"paths": {"~/*": ["lib/*"]}}}"#),
        ];
        let web = [("/p/apps/web/page.ts", "import '@/a';\n")];
        assert_eq!(ts_links(&web, &["/p/src/a.ts"], &configs), [link("/p/apps/web/page.ts", "/p/src/a.ts")]);
        let old = [("/p/apps/old/page.js", "import '~/b';\n")];
        assert_eq!(ts_links(&old, &["/p/apps/old/lib/b.js"], &configs), [link("/p/apps/old/page.js", "/p/apps/old/lib/b.js")]);
        // A tsconfig beats a jsconfig of the same folder.
        let both = [
            ("/p/apps/both/tsconfig.json", r#"{"compilerOptions": {"paths": {"@/*": ["ts/*"]}}}"#),
            ("/p/apps/both/jsconfig.json", r#"{"compilerOptions": {"paths": {"@/*": ["js/*"]}}}"#),
        ];
        let page = [("/p/apps/both/p.ts", "import '@/x';\n")];
        assert_eq!(ts_links(&page, &["/p/apps/both/ts/x.ts", "/p/apps/both/js/x.ts"], &both), [link("/p/apps/both/p.ts", "/p/apps/both/ts/x.ts")]);
    }

    #[test]
    fn workspace_packages_resolve_to_their_source() {
        let configs = [
            ("/p/packages/ui/package.json", r#"{"name": "@acme/ui", "exports": {".": "./src/index.ts", "./*": "./src/*.ts"}}"#),
            ("/p/packages/dist-only/package.json", r#"{"name": "built", "exports": {".": "./dist/index.js"}}"#),
        ];
        let app = ("/p/apps/web/app.ts", "import '@acme/ui';\nimport '@acme/ui/button';\nimport 'built';\nimport 'react';\n");
        let closed = ["/p/packages/ui/src/index.ts", "/p/packages/ui/src/button.ts"];
        assert_eq!(
            ts_links(&[app], &closed, &configs),
            [link("/p/apps/web/app.ts", "/p/packages/ui/src/index.ts"), link("/p/apps/web/app.ts", "/p/packages/ui/src/button.ts")]
        );
        let configs_map: ConfigSources = configs.iter().map(|(p, t)| (PathBuf::from(p), t.to_string())).collect();
        let open = OpenFiles::new([(Path::new(app.0), app.1)]);
        let paths: Vec<PathBuf> = closed.iter().map(PathBuf::from).collect();
        let index = crate::analysis::dependencies::WorkspaceIndex::new(&paths);
        let issues = analyze_with_configs(Path::new("/p"), &open.with_index(&index), &configs_map, &mut ParseCache::default()).issues;
        assert_eq!(issues.count(Why::External), 1, "react is a package that is not in the folder");
        assert_eq!(issues.items().iter().map(|i| (i.specifier.as_str(), i.reason)).collect::<Vec<_>>(), [("built", Why::NotFound)], "its dist was never built");
    }

    #[test]
    fn vite_aliases_resolve_imports_after_the_compilers_paths() {
        let configs = [
            ("/p/web/vite.config.ts", "export default { resolve: { alias: { '@': path.resolve(__dirname, 'src'), '~ui': path.resolve(__dirname, 'ui') } } };"),
            ("/p/web/tsconfig.json", r#"{"compilerOptions": {"paths": {"~ui/*": ["compiler/*"]}}}"#),
        ];
        let app = [("/p/web/app.ts", "import '@/a';\nimport '~ui/b';\n")];
        let closed = ["/p/web/src/a.ts", "/p/web/compiler/b.ts", "/p/web/ui/b.ts"];
        assert_eq!(
            ts_links(&app, &closed, &configs),
            [link("/p/web/app.ts", "/p/web/src/a.ts"), link("/p/web/app.ts", "/p/web/compiler/b.ts")],
            "paths wins over the bundler's alias for the same import"
        );
    }

    #[test]
    fn working_configs_reads_extended_files_and_workspace_packages_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let write = |rel: &str, text: &str| {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            path
        };
        write("tsconfig.base.json", r#"{"compilerOptions": {"paths": {"@/*": ["src/*"]}}}"#);
        write("configs/strict.json", r#"{"extends": "../tsconfig.base.json"}"#);
        write("apps/web/tsconfig.json", r#"{"extends": "../../configs/strict"}"#);
        write("packages/ui/package.json", r#"{"name": "@acme/ui", "main": "src/index.ts"}"#);
        write("apps/web/package.json", r#"{"name": "web"}"#);
        write("apps/web/src/app.ts", "import '@/a';\nimport '@acme/ui';\n");
        write("src/a.ts", "export {};\n");
        write("packages/ui/src/index.ts", "export {};\n");
        let app = root.join("apps/web/src/app.ts");
        let text = std::fs::read_to_string(&app).unwrap();
        let listed: Vec<PathBuf> = ["src/a.ts", "packages/ui/src/index.ts", "packages/ui/package.json", "apps/web/package.json"]
            .iter()
            .map(|p| root.join(p))
            .collect();
        let index = crate::analysis::dependencies::WorkspaceIndex::new(&listed);
        let open = OpenFiles::new([(app.as_path(), text.as_str())]).with_index(&index);

        let configs = working_configs(&root, &open);
        let keys: Vec<&str> = configs.keys().map(|p| p.strip_prefix(&root).unwrap().to_str().unwrap()).collect();
        for expected in ["apps/web/tsconfig.json", "configs/strict.json", "tsconfig.base.json", "packages/ui/package.json", "apps/web/package.json"] {
            assert!(keys.contains(&expected), "{expected} in {keys:?}");
        }
        // And what that reading is for: the imports resolve without any of those files open.
        let links = analyze_with_configs(&root, &open, &configs, &mut ParseCache::default()).links;
        let targets: Vec<PathBuf> = links.into_iter().map(|l| l.target).collect();
        assert_eq!(targets, vec![root.join("src/a.ts"), root.join("packages/ui/src/index.ts")]);

        // Without the folder's files there are no packages to read.
        let alone = OpenFiles::new([(app.as_path(), text.as_str())]);
        assert!(!working_configs(&root, &alone).keys().any(|p| p.ends_with("package.json")));
    }

    // ---- barrels and re-exports (TASK-94) ----

    use crate::edges::edge_store::EdgeKind;

    /// `(source, target, kind)` of the links of `files` (open) with `closed`
    /// in the folder, sorted.
    fn kinds(files: &[(&str, &str)], closed: &[&str]) -> Vec<(String, String, EdgeKind)> {
        let open = OpenFiles::new(files.iter().map(|(p, t)| (Path::new(p), *t)));
        let paths: Vec<PathBuf> = closed.iter().map(PathBuf::from).collect();
        let index = crate::analysis::dependencies::WorkspaceIndex::new(&paths);
        let open = open.with_index(&index);
        let mut found: Vec<_> = analyze_with_configs(Path::new("/p"), &open, &ConfigSources::new(), &mut ParseCache::default())
            .links
            .into_iter()
            .map(|l| (l.source.to_str().unwrap().to_owned(), l.target.to_str().unwrap().to_owned(), l.kind))
            .collect();
        found.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        found
    }

    fn k(source: &str, target: &str, kind: EdgeKind) -> (String, String, EdgeKind) {
        (source.to_owned(), target.to_owned(), kind)
    }

    const A_TS: (&str, &str) = ("/p/models/A.ts", "export class A {}\n");

    #[test]
    fn an_import_through_a_barrel_is_a_hop_and_a_shortcut() {
        let files = [
            ("/p/app.ts", "import { A } from './models';\n"),
            ("/p/models/index.ts", "export { A } from './A';\n"),
            A_TS,
        ];
        assert_eq!(
            kinds(&files, &[]),
            [
                k("/p/app.ts", "/p/models/A.ts", EdgeKind::ReExport),
                k("/p/app.ts", "/p/models/index.ts", EdgeKind::Barrel),
                k("/p/models/index.ts", "/p/models/A.ts", EdgeKind::Direct),
            ],
            "the barrel's own export stays a real import"
        );
        // One re-export is enough: no threshold, no file name.
        let renamed = [("/p/app.ts", "import { A } from './hub';\n"), ("/p/hub.ts", "export { A } from './models/A';\n"), A_TS];
        assert!(kinds(&renamed, &[]).contains(&k("/p/app.ts", "/p/hub.ts", EdgeKind::Barrel)));
    }

    #[test]
    fn chains_of_barrels_are_followed_to_the_definition() {
        let files = [
            ("/p/app.ts", "import { A } from './all';\n"),
            ("/p/all/index.ts", "export * from './models';\n"),
            ("/p/all/models/index.ts", "export { A } from './A';\n"),
            ("/p/all/models/A.ts", "export class A {}\n"),
        ];
        let found = kinds(&files, &[]);
        assert!(found.contains(&k("/p/app.ts", "/p/all/index.ts", EdgeKind::Barrel)));
        assert!(found.contains(&k("/p/app.ts", "/p/all/models/A.ts", EdgeKind::ReExport)), "{found:?}");
        assert_eq!(found.iter().filter(|(s, _, _)| s == "/p/app.ts").count(), 2, "no link to the barrel in the middle");
    }

    #[test]
    fn export_star_cycles_end_and_a_missing_name_gives_no_shortcut() {
        let files = [
            ("/p/app.ts", "import { Nope } from './a';\n"),
            ("/p/a/index.ts", "export * from '../b';\n"),
            ("/p/b/index.ts", "export * from '../a';\n"),
        ];
        let found = kinds(&files, &[]);
        assert!(found.iter().all(|(_, _, kind)| *kind == EdgeKind::Direct), "{found:?}");
    }

    #[test]
    fn namespace_exports_default_and_renamed_names_resolve() {
        let files = [
            ("/p/app.ts", "import { ns } from './hub';\nimport D from './hub';\nimport { B as C } from './hub';\n"),
            ("/p/hub.ts", "export * as ns from './m';\nexport { default } from './d';\nexport { A as B } from './a';\n"),
            ("/p/m.ts", "export const x = 1;\n"),
            ("/p/d.ts", "export default 1;\n"),
            ("/p/a.ts", "export const A = 1;\n"),
        ];
        let found = kinds(&files, &[]);
        for provider in ["/p/m.ts", "/p/d.ts", "/p/a.ts"] {
            assert!(found.contains(&k("/p/app.ts", provider, EdgeKind::ReExport)), "{provider} in {found:?}");
        }
        let hops: Vec<_> = found.iter().filter(|(s, t, _)| s == "/p/app.ts" && t == "/p/hub.ts").collect();
        assert_eq!(hops.len(), 3, "one link per import statement");
        assert!(hops.iter().all(|(_, _, kind)| *kind == EdgeKind::Barrel));
    }

    #[test]
    fn import_then_export_makes_a_barrel_and_a_name_of_its_own_does_not() {
        let files = [
            ("/p/app.ts", "import { A } from './hub';\nimport { Own } from './hub';\nimport { A as Again, Own as Mine } from './hub';\n"),
            ("/p/hub.ts", "import { A } from './a';\nexport { A };\nexport const Own = 1;\n"),
            ("/p/a.ts", "export const A = 1;\n"),
        ];
        let found = kinds(&files, &[]);
        assert!(found.contains(&k("/p/app.ts", "/p/a.ts", EdgeKind::ReExport)));
        let hubs: Vec<_> = found.iter().filter(|(s, t, _)| s == "/p/app.ts" && t == "/p/hub.ts").map(|(_, _, kind)| *kind).collect();
        // `A` only: a hop. `Own` only: a direct import. Both: a direct import
        // that also has the shortcut for `A`.
        assert_eq!(hubs.len(), 3);
        assert_eq!(hubs.iter().filter(|kind| **kind == EdgeKind::Barrel).count(), 1, "{hubs:?}");
        assert_eq!(hubs.iter().filter(|kind| **kind == EdgeKind::Direct).count(), 2, "{hubs:?}");
    }

    #[test]
    fn a_namespace_import_of_a_barrel_and_a_closed_barrel_stay_direct() {
        let files = [
            ("/p/app.ts", "import * as all from './hub';\n"),
            ("/p/hub.ts", "export { A } from './a';\n"),
            ("/p/a.ts", "export const A = 1;\n"),
        ];
        assert_eq!(
            kinds(&files, &[]).iter().filter(|(s, _, _)| s == "/p/app.ts").map(|l| l.2).collect::<Vec<_>>(),
            [EdgeKind::Direct],
            "a namespace takes the whole file"
        );
        // Nobody read the barrel (it is closed), so the chain stops there.
        let closed = [("/p/app.ts", "import { A } from './hub';\n"), ("/p/a.ts", "export const A = 1;\n")];
        assert_eq!(kinds(&closed, &["/p/hub.ts"]), [k("/p/app.ts", "/p/hub.ts", EdgeKind::Direct)]);
    }

    #[test]
    fn a_python_package_re_exports_what_its_init_imports() {
        let files = [
            ("/p/app.py", "from .pkg import Order\nfrom .pkg import helper\n"),
            ("/p/pkg/__init__.py", "from .models import Order\nfrom .util import helper as helper\n"),
            ("/p/pkg/models.py", "class Order: pass\n"),
            ("/p/pkg/util.py", "def helper(): pass\n"),
        ];
        let found = kinds(&files, &[]);
        assert!(found.contains(&k("/p/app.py", "/p/pkg/models.py", EdgeKind::ReExport)), "{found:?}");
        assert!(found.contains(&k("/p/app.py", "/p/pkg/util.py", EdgeKind::ReExport)), "{found:?}");
        assert!(found.contains(&k("/p/app.py", "/p/pkg/__init__.py", EdgeKind::Barrel)));
        // The same `from … import` in a module that is not an __init__ re-exports nothing.
        let plain = [
            ("/p/app.py", "from .mod import Order\n"),
            ("/p/mod.py", "from .models import Order\n"),
            ("/p/models.py", "class Order: pass\n"),
        ];
        assert!(kinds(&plain, &[]).iter().all(|(_, _, kind)| *kind == EdgeKind::Direct));
    }

    const RUST_LIB: (&str, &str) = ("/p/src/lib.rs", "mod app;\nmod models;\n");

    #[test]
    fn a_rust_use_through_a_facade_module_is_a_hop_and_a_shortcut() {
        let files = [
            RUST_LIB,
            ("/p/src/app.rs", "use crate::models::Order;\nuse std::fmt;\nuse serde::Serialize;\n"),
            ("/p/src/models/mod.rs", "mod order;\npub use self::order::Order;\n"),
            ("/p/src/models/order.rs", "pub struct Order;\n"),
        ];
        let found = kinds(&files, &[]);
        assert!(found.contains(&k("/p/src/app.rs", "/p/src/models/mod.rs", EdgeKind::Barrel)), "{found:?}");
        assert!(found.contains(&k("/p/src/app.rs", "/p/src/models/order.rs", EdgeKind::ReExport)), "{found:?}");
        assert!(found.contains(&k("/p/src/models/mod.rs", "/p/src/models/order.rs", EdgeKind::Direct)), "the `mod order;` link");
        assert_eq!(found.iter().filter(|(s, _, _)| s == "/p/src/app.rs").count(), 2, "std and a dependency draw nothing");
    }

    #[test]
    fn rust_globs_lists_and_direct_paths_resolve_through_the_module_tree() {
        let files = [
            RUST_LIB,
            ("/p/src/app.rs", "use crate::models::{Order, Customer};\nuse crate::models::order::Order as Direct;\n"),
            ("/p/src/models/mod.rs", "mod order;\nmod customer;\npub use order::*;\npub use self::customer::Customer;\n"),
            ("/p/src/models/order.rs", "pub struct Order;\n"),
            ("/p/src/models/customer.rs", "pub struct Customer;\n"),
        ];
        let found = kinds(&files, &[]);
        let from_app: Vec<_> = found.iter().filter(|(s, _, _)| s == "/p/src/app.rs").collect();
        assert!(from_app.contains(&&k("/p/src/app.rs", "/p/src/models/mod.rs", EdgeKind::Barrel)), "{from_app:?}");
        assert!(from_app.contains(&&k("/p/src/app.rs", "/p/src/models/customer.rs", EdgeKind::ReExport)));
        // `Order` comes through the glob; the direct `use` of its own module draws nothing.
        assert!(from_app.iter().any(|l| l.1 == "/p/src/models/order.rs" && l.2 == EdgeKind::ReExport), "{from_app:?}");
        assert!(from_app.iter().all(|l| l.2 != EdgeKind::Direct), "{from_app:?}");
    }

    #[test]
    fn rust_self_super_crate_and_inline_modules_pick_the_right_module() {
        let files = [
            ("/p/src/lib.rs", "mod facade;\nmod real;\nmod user;\nmod user2;\n"),
            ("/p/src/real.rs", "pub struct Thing;\n"),
            ("/p/src/facade.rs", "pub use crate::real::Thing;\n"),
            ("/p/src/user.rs", "use super::facade::Thing;\n"),
            ("/p/src/user2.rs", "mod inner {\n    use super::super::facade::Thing;\n}\n"),
        ];
        let found = kinds(&files, &[]);
        // `crate::` in the facade, `super::` in a file, and two `super::` out of an inline module.
        for user in ["/p/src/user.rs", "/p/src/user2.rs"] {
            assert!(found.contains(&k(user, "/p/src/facade.rs", EdgeKind::Barrel)), "{user}: {found:?}");
            assert!(found.contains(&k(user, "/p/src/real.rs", EdgeKind::ReExport)), "{user}: {found:?}");
        }
    }

    #[test]
    fn a_rust_path_through_a_module_nobody_read_draws_nothing() {
        // `models` is declared in a lib.rs that is not open: its module is unknown.
        let files = [("/p/src/app.rs", "use crate::models::Order;\n")];
        assert!(kinds(&files, &["/p/src/lib.rs", "/p/src/models/mod.rs"]).is_empty());
    }
}
