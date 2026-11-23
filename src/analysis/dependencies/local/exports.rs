//! Which file a name imported through a re-exporting file really comes from.
//!
//! An import of `Order` from `./models` (an `index.ts` that says
//! `export { Order } from './Order'`) names the barrel, but the code that
//! defines `Order` is in `Order.ts`. For each file the analysis collects its
//! named imports and its exports ([`FileFacts`]); [`apply`] follows each
//! imported name through the re-exports, with a visited set so that cycles end,
//! and emits two links for an import that goes through a barrel:
//!
//! - the importer's link to the barrel becomes `Barrel` (the routing hop);
//! - a `ReExport` link goes from the importer straight to the provider.
//!
//! Whether a file is a barrel is not decided by its name or by how many
//! re-exports it has, but by what its exports say. Only open files have facts:
//! the chain stops at a file nobody read.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use tree_sitter::Node;

use crate::analysis::dependencies::{DependencyLink, LineSpan, LinkEvidence};
use crate::edges::edge_store::EdgeKind;
use crate::edges::style::RelationshipType;

/// How many re-exports are followed one after the other.
const MAX_CHAIN: usize = 16;

/// `(line, start byte, end byte)`, line-relative, as a link records its name.
pub(super) type Span = (usize, usize, usize);

/// The line-relative span of `start..end` in `text`, `None` when it is not on
/// one line or not on character boundaries.
pub(super) fn span_in(text: &str, start: usize, end: usize) -> Option<Span> {
    if start > end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return None;
    }
    let line = text[..start].bytes().filter(|b| *b == b'\n').count();
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    (!text[start..end].contains('\n')).then_some((line, start - line_start, end - line_start))
}

/// A name an `import` brings in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ImportedName {
    /// The name in the other file: `default`, or `*` for a namespace import.
    pub imported: String,
    /// The name it has here.
    pub local: String,
    /// Where it is written in the statement.
    pub span: Span,
}

/// An import statement that resolved to a file and names what it takes.
#[derive(Clone, Debug)]
pub(super) struct NamedImport {
    pub target: PathBuf,
    /// The span of the specifier, which is what identifies the import's link.
    pub specifier: Span,
    pub names: Vec<ImportedName>,
}

#[derive(Clone, Debug)]
pub(super) enum RawExport {
    /// A declaration, or `export default …`.
    Local { name: String },
    /// `export { imported as name } from './m'`.
    From { name: String, imported: String, from: PathBuf, line: usize },
    /// `export * from './m'`.
    All { from: PathBuf, line: usize },
    /// `export * as name from './m'`.
    Namespace { name: String, from: PathBuf, line: usize },
    /// `export { local as name }`: a name of this file, or an imported binding.
    Bound { name: String, local: String, line: usize },
}

/// A Rust `mod` declaration: the names from the crate-level module of the
/// declaring file down to the module (inline modules included), and the file
/// that holds its body (the declaring file itself for an inline module).
#[derive(Clone, Debug)]
pub(super) struct ModuleDecl {
    pub names: Vec<String>,
    pub declared_in: PathBuf,
    pub target: PathBuf,
}

/// One name a Rust `use` brings in or re-exports.
#[derive(Clone, Debug)]
pub(super) struct UseLeaf {
    /// `crate::models::Order` as `[crate, models, Order]`.
    pub path: Vec<String>,
    pub alias: Option<String>,
    /// `use a::b::*`.
    pub wildcard: bool,
    pub span: Span,
}

/// A Rust `use` declaration, its tree flattened. Which file each path names
/// needs every file's modules, so it is resolved by [`apply`].
#[derive(Clone, Debug)]
pub(super) struct RustUse {
    /// The inline modules around it, outermost first.
    pub inline: Vec<String>,
    pub public: bool,
    pub line: usize,
    pub leaves: Vec<UseLeaf>,
}

/// What one file imports by name and what it exports.
#[derive(Clone, Debug, Default)]
pub(super) struct FileFacts {
    pub imports: Vec<NamedImport>,
    pub exports: Vec<RawExport>,
    pub modules: Vec<ModuleDecl>,
    pub uses: Vec<RustUse>,
}

// ---- extraction ----

fn text_of<'t>(node: Node, text: &'t str) -> &'t str {
    node.utf8_text(text.as_bytes()).unwrap_or("")
}

fn name_node_text(node: Option<Node>, text: &str) -> Option<String> {
    let node = node?;
    // `export { "a-b" as c }` names are strings: not followed.
    (node.kind() != "string").then(|| text_of(node, text).to_owned())
}

/// Reads an `import_statement` or an `export_statement` of JS or TS that
/// resolved to `target`.
pub(super) fn record_js(node: Node, text: &str, target: &Path, specifier: Option<Span>, facts: &mut FileFacts) {
    let line = node.start_position().row;
    if node.kind() == "import_statement" {
        let mut names = Vec::new();
        let mut cursor = node.walk();
        let Some(clause) = node.named_children(&mut cursor).find(|n| n.kind() == "import_clause") else { return };
        let mut inner = clause.walk();
        for part in clause.named_children(&mut inner) {
            match part.kind() {
                // import Default from …
                "identifier" => names.extend(imported(part, "default", text)),
                // import * as ns from …
                "namespace_import" => {
                    let mut c = part.walk();
                    let ns = part.named_children(&mut c).find(|n| n.kind() == "identifier");
                    names.extend(ns.and_then(|n| imported(n, "*", text)));
                }
                // import { a, b as c } from …
                "named_imports" => {
                    let mut c = part.walk();
                    for spec in part.named_children(&mut c).filter(|n| n.kind() == "import_specifier") {
                        let Some(name) = name_node_text(spec.child_by_field_name("name"), text) else { continue };
                        let local = spec.child_by_field_name("alias").or(spec.child_by_field_name("name"));
                        names.extend(local.and_then(|n| imported(n, &name, text)));
                    }
                }
                _ => {}
            }
        }
        if let Some(specifier) = specifier {
            facts.imports.push(NamedImport { target: target.to_path_buf(), specifier, names });
        }
        return;
    }
    // export … from './m'
    let mut cursor = node.walk();
    let mut is_star = false;
    for child in node.children(&mut cursor) {
        match child.kind() {
            "*" => is_star = true,
            // export * as ns from './m'
            "namespace_export" => {
                let mut c = child.walk();
                let ns = child.named_children(&mut c).find(|n| n.kind() == "identifier");
                if let Some(ns) = ns {
                    facts.exports.push(RawExport::Namespace {
                        name: text_of(ns, text).to_owned(),
                        from: target.to_path_buf(),
                        line,
                    });
                    is_star = false;
                }
            }
            // export { a, b as c } from './m'
            "export_clause" => {
                let mut c = child.walk();
                for spec in child.named_children(&mut c).filter(|n| n.kind() == "export_specifier") {
                    let Some(imported) = name_node_text(spec.child_by_field_name("name"), text) else { continue };
                    let name = name_node_text(spec.child_by_field_name("alias"), text).unwrap_or_else(|| imported.clone());
                    facts.exports.push(RawExport::From { name, imported, from: target.to_path_buf(), line });
                }
            }
            _ => {}
        }
    }
    if is_star {
        facts.exports.push(RawExport::All { from: target.to_path_buf(), line });
    }
}

fn imported(local: Node, imported: &str, text: &str) -> Option<ImportedName> {
    Some(ImportedName {
        imported: imported.to_owned(),
        local: text_of(local, text).to_owned(),
        span: span_in(text, local.start_byte(), local.end_byte())?,
    })
}

/// The exports of an `export_statement` without a source: declarations,
/// `export default` and `export { a as b }` of names of this file.
pub(super) fn record_js_local_exports(node: Node, text: &str, facts: &mut FileFacts) {
    let line = node.start_position().row;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "default" => facts.exports.push(RawExport::Local { name: "default".to_owned() }),
            "export_clause" => {
                let mut c = child.walk();
                for spec in child.named_children(&mut c).filter(|n| n.kind() == "export_specifier") {
                    let Some(local) = name_node_text(spec.child_by_field_name("name"), text) else { continue };
                    let name = name_node_text(spec.child_by_field_name("alias"), text).unwrap_or_else(|| local.clone());
                    facts.exports.push(RawExport::Bound { name, local, line });
                }
            }
            _ => {}
        }
    }
    let Some(declaration) = node.child_by_field_name("declaration") else { return };
    match declaration.kind() {
        // export const a = 1, b = 2;
        "lexical_declaration" | "variable_declaration" => {
            let mut c = declaration.walk();
            for var in declaration.named_children(&mut c).filter(|n| n.kind() == "variable_declarator") {
                if let Some(name) = var.child_by_field_name("name").filter(|n| n.kind() == "identifier") {
                    facts.exports.push(RawExport::Local { name: text_of(name, text).to_owned() });
                }
            }
        }
        _ => {
            if let Some(name) = declaration.child_by_field_name("name") {
                facts.exports.push(RawExport::Local { name: text_of(name, text).to_owned() });
            }
        }
    }
}

/// `from .m import a, b as c` of a Python file that resolved to `target`.
pub(super) fn record_python(node: Node, text: &str, target: &Path, specifier: Option<Span>, is_init: bool, facts: &mut FileFacts) {
    let line = node.start_position().row;
    let mut names = Vec::new();
    let mut cursor = node.walk();
    let mut wildcard = false;
    for child in node.children_by_field_name("name", &mut cursor) {
        let (name, alias) = match child.kind() {
            "dotted_name" => (child, child),
            "aliased_import" => match (child.child_by_field_name("name"), child.child_by_field_name("alias")) {
                (Some(name), Some(alias)) => (name, alias),
                _ => continue,
            },
            _ => continue,
        };
        names.extend(imported(alias, text_of(name, text), text));
    }
    let mut c = node.walk();
    if node.named_children(&mut c).any(|n| n.kind() == "wildcard_import") {
        wildcard = true;
    }
    // In `__init__.py` what a package imports is what it offers.
    if is_init {
        for name in &names {
            facts.exports.push(RawExport::From { name: name.local.clone(), imported: name.imported.clone(), from: target.to_path_buf(), line });
        }
        if wildcard {
            facts.exports.push(RawExport::All { from: target.to_path_buf(), line });
        }
    }
    if let Some(specifier) = specifier {
        facts.imports.push(NamedImport { target: target.to_path_buf(), specifier, names });
    }
}

// ---- Rust ----

/// The items a Rust file defines at its top level.
pub(super) fn record_rust_items(root: Node, text: &str, facts: &mut FileFacts) {
    let mut cursor = root.walk();
    for item in root.named_children(&mut cursor) {
        if matches!(
            item.kind(),
            "function_item" | "struct_item" | "enum_item" | "union_item" | "trait_item" | "type_item" | "const_item"
                | "static_item" | "mod_item" | "macro_definition"
        ) && let Some(name) = item.child_by_field_name("name")
        {
            facts.exports.push(RawExport::Local { name: text_of(name, text).to_owned() });
        }
    }
}

/// Reads a `use_declaration`: its visibility, the inline modules around it
/// and the leaves of its tree.
pub(super) fn record_rust_use(node: Node, text: &str, facts: &mut FileFacts) {
    let mut inline = Vec::new();
    let mut up = node.parent();
    while let Some(parent) = up {
        if parent.kind() == "mod_item"
            && let Some(name) = parent.child_by_field_name("name")
        {
            inline.push(text_of(name, text).to_owned());
        }
        up = parent.parent();
    }
    inline.reverse();
    let mut cursor = node.walk();
    let public = node.children(&mut cursor).any(|c| c.kind() == "visibility_modifier");
    let Some(argument) = node.child_by_field_name("argument") else { return };
    let mut leaves = Vec::new();
    flatten_use(argument, &[], None, false, text, &mut leaves);
    if !leaves.is_empty() {
        facts.uses.push(RustUse { inline, public, line: node.start_position().row, leaves });
    }
}

/// The segments of a path node and the node of its last segment.
fn rust_path<'t>(node: Node<'t>, text: &str) -> Option<(Vec<String>, Node<'t>)> {
    match node.kind() {
        "identifier" | "self" | "crate" | "super" | "metavariable" => Some((vec![text_of(node, text).to_owned()], node)),
        "scoped_identifier" => {
            let name = node.child_by_field_name("name")?;
            let mut segments = match node.child_by_field_name("path") {
                Some(path) => rust_path(path, text)?.0,
                None => Vec::new(),
            };
            segments.push(text_of(name, text).to_owned());
            Some((segments, name))
        }
        _ => None,
    }
}

fn flatten_use(node: Node, prefix: &[String], alias: Option<String>, wildcard: bool, text: &str, out: &mut Vec<UseLeaf>) {
    match node.kind() {
        "identifier" | "self" | "crate" | "super" | "scoped_identifier" => {
            let Some((segments, last)) = rust_path(node, text) else { return };
            let Some(span) = span_in(text, last.start_byte(), last.end_byte()) else { return };
            let mut path = prefix.to_vec();
            path.extend(segments);
            out.push(UseLeaf { path, alias, wildcard, span });
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                flatten_use(child, prefix, None, false, text, out);
            }
        }
        "scoped_use_list" => {
            let mut base = prefix.to_vec();
            if let Some((segments, _)) = node.child_by_field_name("path").and_then(|p| rust_path(p, text)) {
                base.extend(segments);
            }
            if let Some(list) = node.child_by_field_name("list") {
                flatten_use(list, &base, None, false, text, out);
            }
        }
        "use_as_clause" => {
            let alias = node.child_by_field_name("alias").map(|a| text_of(a, text).to_owned());
            if let Some(path) = node.child_by_field_name("path") {
                flatten_use(path, prefix, alias, false, text, out);
            }
        }
        // use a::b::*; the path is the first named child.
        "use_wildcard" => {
            let mut cursor = node.walk();
            if let Some(path) = node.named_children(&mut cursor).next() {
                flatten_use(path, prefix, None, true, text, out);
            }
        }
        _ => {}
    }
}

/// The modules of every Rust file, found from their `mod` declarations: a
/// crate's root files are the ones no declaration points to.
struct ModuleTree {
    /// `(crate root, module path)` to the file with the module's body.
    by_path: HashMap<(PathBuf, Vec<String>), PathBuf>,
    /// A file to its crate root and the path of the module it is.
    file_module: HashMap<PathBuf, (PathBuf, Vec<String>)>,
}

fn module_tree(facts: &HashMap<PathBuf, FileFacts>) -> ModuleTree {
    let mut files: Vec<&PathBuf> = facts.keys().filter(|p| p.extension().is_some_and(|e| e == "rs")).collect();
    files.sort();
    let decls: Vec<&ModuleDecl> = files.iter().flat_map(|f| &facts[*f].modules).collect();
    let pointed_to: HashSet<&PathBuf> = decls.iter().filter(|d| d.target != d.declared_in).map(|d| &d.target).collect();
    let mut tree = ModuleTree { by_path: HashMap::new(), file_module: HashMap::new() };
    for file in files.iter().filter(|f| !pointed_to.contains(**f)) {
        tree.file_module.insert((*file).clone(), ((*file).clone(), Vec::new()));
        tree.by_path.insert(((*file).clone(), Vec::new()), (*file).clone());
    }
    // Modules are found parent first; a few rounds cover any nesting.
    for _ in 0..MAX_CHAIN {
        let mut grew = false;
        for decl in &decls {
            let Some((root, path)) = tree.file_module.get(&decl.declared_in).cloned() else { continue };
            let mut full = path;
            full.extend(decl.names.iter().cloned());
            if tree.by_path.insert((root.clone(), full.clone()), decl.target.clone()).is_none() {
                grew = true;
            }
            if decl.target != decl.declared_in && !tree.file_module.contains_key(&decl.target) {
                tree.file_module.insert(decl.target.clone(), (root, full));
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    tree
}

/// The file and item a `use` path names: `crate::models::Order` is
/// (`models/mod.rs`, `Order`), a path that ends on a module has no item.
/// `None` for a path outside this crate (a dependency, `std`) or one through
/// a module whose file nobody read.
fn resolve_use(tree: &ModuleTree, importer: &Path, inline: &[String], segments: &[String]) -> Option<(PathBuf, Option<String>)> {
    let (root, mut current) = tree.file_module.get(importer)?.clone();
    current.extend(inline.iter().cloned());
    let mut i = 0;
    let prefixed = matches!(segments.first()?.as_str(), "crate" | "self" | "super");
    match segments.first()?.as_str() {
        "crate" => {
            current.clear();
            i = 1;
        }
        "self" => i = 1,
        "super" => {
            while segments.get(i).is_some_and(|s| s == "super") {
                current.pop()?;
                i += 1;
            }
        }
        _ => {}
    }
    let mut file = tree.by_path.get(&(root.clone(), current.clone()))?.clone();
    let mut item = None;
    while let Some(segment) = segments.get(i) {
        let mut next = current.clone();
        next.push(segment.clone());
        match tree.by_path.get(&(root.clone(), next.clone())) {
            Some(found) => {
                current = next;
                file = found.clone();
                i += 1;
            }
            None => {
                item = Some(segment.clone());
                break;
            }
        }
    }
    // `use serde::Serialize`: the first segment is not a module of this crate.
    if !prefixed && i == 0 {
        return None;
    }
    Some((file, item))
}

/// Turns the `use` declarations of Rust files into imports, links and the
/// exports of their `pub use`.
fn resolve_rust(facts: &mut HashMap<PathBuf, FileFacts>, links: &mut Vec<DependencyLink>) -> HashSet<(PathBuf, PathBuf, Span)> {
    let tree = module_tree(facts);
    let mut added = HashSet::new();
    let mut importers: Vec<PathBuf> = facts.iter().filter(|(_, f)| !f.uses.is_empty()).map(|(p, _)| p.clone()).collect();
    importers.sort();
    for importer in importers {
        let uses = facts[&importer].uses.clone();
        let mut imports = Vec::new();
        let mut exports = Vec::new();
        for item in &uses {
            // One link per file a declaration reaches, at its first name.
            let mut groups: Vec<NamedImport> = Vec::new();
            for leaf in &item.leaves {
                let Some((file, name)) = resolve_use(&tree, &importer, &item.inline, &leaf.path) else { continue };
                let last = leaf.path.last().cloned().unwrap_or_default();
                let local = leaf.alias.clone().unwrap_or_else(|| last.clone());
                // The name the importer takes, or the module itself.
                let imported = match (&name, leaf.wildcard) {
                    (_, true) => None,
                    (Some(name), false) => Some(name.clone()),
                    (None, false) => Some("*".to_owned()),
                };
                if item.public && item.inline.is_empty() {
                    match (&name, leaf.wildcard) {
                        (_, true) => exports.push(RawExport::All { from: file.clone(), line: item.line }),
                        (Some(name), false) => exports.push(RawExport::From { name: local.clone(), imported: name.clone(), from: file.clone(), line: item.line }),
                        (None, false) => exports.push(RawExport::Namespace { name: local.clone(), from: file.clone(), line: item.line }),
                    }
                }
                let names: Vec<ImportedName> = imported
                    .into_iter()
                    .map(|imported| ImportedName { imported, local: local.clone(), span: leaf.span })
                    .collect();
                match groups.iter_mut().find(|g| g.target == file) {
                    Some(group) => group.names.extend(names),
                    None => groups.push(NamedImport { target: file, specifier: leaf.span, names }),
                }
            }
            for group in groups {
                if group.target != importer {
                    added.insert((importer.clone(), group.target.clone(), group.specifier));
                    links.push(DependencyLink {
                        evidence: LinkEvidence::LocalSyntax,
                        relationship: RelationshipType::Imports,
                        source: importer.clone(),
                        target: group.target.clone(),
                        label: "file".into(),
                        source_label: None,
                        usage: LineSpan { start: group.specifier.0, end: group.specifier.0 },
                        definition: LineSpan { start: 0, end: 0 },
                        name_span: None,
                        source_name_span: Some(group.specifier),
                        kind: EdgeKind::Direct,
                    });
                    imports.push(group);
                }
            }
        }
        let own = facts.get_mut(&importer).expect("an importer has facts");
        own.imports.extend(imports);
        own.exports.extend(exports);
    }
    added
}

// ---- resolution ----

enum Entry {
    Local,
    From { imported: String, from: PathBuf, line: usize },
    Namespace { from: PathBuf, line: usize },
}

#[derive(Default)]
struct Table {
    named: HashMap<String, Entry>,
    all: Vec<(PathBuf, usize)>,
}

fn table(facts: &FileFacts) -> Table {
    let bindings: HashMap<&str, (&PathBuf, &str)> = facts
        .imports
        .iter()
        .flat_map(|import| import.names.iter().map(move |n| (n.local.as_str(), (&import.target, n.imported.as_str()))))
        .collect();
    let mut table = Table::default();
    for export in &facts.exports {
        match export {
            RawExport::Local { name } => {
                table.named.insert(name.clone(), Entry::Local);
            }
            RawExport::From { name, imported, from, line } => {
                table.named.insert(name.clone(), Entry::From { imported: imported.clone(), from: from.clone(), line: *line });
            }
            RawExport::All { from, line } => table.all.push((from.clone(), *line)),
            RawExport::Namespace { name, from, line } => {
                table.named.insert(name.clone(), Entry::Namespace { from: from.clone(), line: *line });
            }
            RawExport::Bound { name, local, line } => {
                let entry = match bindings.get(local.as_str()) {
                    Some((from, "*")) => Entry::Namespace { from: (*from).clone(), line: *line },
                    Some((from, imported)) => Entry::From { imported: (*imported).to_owned(), from: (*from).clone(), line: *line },
                    None => Entry::Local,
                };
                table.named.insert(name.clone(), entry);
            }
        }
    }
    table
}

/// Where a name comes from, seen from the file that was asked.
struct Origin {
    provider: PathBuf,
    /// The line, in the asked file, where the name is re-exported; `None`
    /// when the asked file defines it.
    reexport_line: Option<usize>,
}

fn origin(
    tables: &HashMap<PathBuf, Table>,
    file: &Path,
    name: &str,
    visited: &mut HashSet<(PathBuf, String)>,
    depth: usize,
) -> Option<Origin> {
    if depth > MAX_CHAIN || !visited.insert((file.to_path_buf(), name.to_owned())) {
        return None;
    }
    let table = tables.get(file)?;
    if let Some(entry) = table.named.get(name) {
        return match entry {
            Entry::Local => Some(Origin { provider: file.to_path_buf(), reexport_line: None }),
            Entry::From { imported, from, line } => {
                // A file nobody read, or one that does not say, is still the
                // file the name was re-exported from.
                let provider = origin(tables, from, imported, visited, depth + 1).map_or_else(|| from.clone(), |o| o.provider);
                Some(Origin { provider, reexport_line: Some(*line) })
            }
            Entry::Namespace { from, line } => Some(Origin { provider: from.clone(), reexport_line: Some(*line) }),
        };
    }
    // `export *` re-exports everything but the default.
    if name != "default" {
        for (from, line) in &table.all {
            if let Some(found) = origin(tables, from, name, visited, depth + 1) {
                return Some(Origin { provider: found.provider, reexport_line: Some(*line) });
            }
        }
    }
    None
}

/// Tags the links of imports that go through a re-exporting file and adds the
/// shortcut to where each name is defined.
pub(super) fn apply(links: &mut Vec<DependencyLink>, facts: &mut HashMap<PathBuf, FileFacts>) {
    let rust_uses = resolve_rust(facts, links);
    let tables: HashMap<PathBuf, Table> = facts.iter().map(|(path, facts)| (path.clone(), table(facts))).collect();
    let mut shortcuts = Vec::new();
    // The importers in path order, so that the result does not depend on the map.
    let mut importers: Vec<&PathBuf> = facts.keys().collect();
    importers.sort();
    for importer in importers {
        for import in &facts[importer].imports {
            let mut through: Vec<(PathBuf, Span, usize)> = Vec::new();
            let mut direct = 0;
            for name in import.names.iter().filter(|n| n.imported != "*") {
                match origin(&tables, &import.target, &name.imported, &mut HashSet::new(), 0) {
                    Some(Origin { provider, reexport_line: Some(line) }) if provider != import.target => {
                        through.push((provider, name.span, line));
                    }
                    _ => direct += 1,
                }
            }
            // A namespace import takes the whole file, not a name.
            direct += import.names.iter().filter(|n| n.imported == "*").count();
            let Some((_, _, first_line)) = through.first().cloned() else { continue };
            if direct == 0
                && let Some(link) = links.iter_mut().find(|l| {
                    l.source == *importer
                        && l.target == import.target
                        && l.relationship == RelationshipType::Imports
                        && l.source_name_span == Some(import.specifier)
                })
            {
                link.kind = EdgeKind::Barrel;
                link.definition = LineSpan { start: first_line, end: first_line };
            }
            // One shortcut per provider, at the first name that reaches it.
            let mut seen = HashSet::new();
            for (provider, span, _) in through {
                if provider == *importer || !seen.insert(provider.clone()) {
                    continue;
                }
                shortcuts.push(DependencyLink {
                    evidence: LinkEvidence::LocalSyntax,
                    relationship: RelationshipType::Imports,
                    source: importer.clone(),
                    target: provider,
                    label: "file".into(),
                    source_label: None,
                    usage: LineSpan { start: span.0, end: span.0 },
                    definition: LineSpan { start: 0, end: 0 },
                    name_span: None,
                    source_name_span: Some(span),
                    kind: EdgeKind::ReExport,
                });
            }
        }
    }
    links.extend(shortcuts);
    // A plain `use` of Rust draws nothing of its own: the language servers
    // link the names it brings in where they are used, as before. Only a
    // `use` that goes through a facade keeps its link: that is the hop.
    links.retain(|l| {
        l.kind != EdgeKind::Direct
            || l.source_name_span.is_none_or(|span| !rust_uses.contains(&(l.source.clone(), l.target.clone(), span)))
    });
}
