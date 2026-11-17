//! Symbol search: the top-level declarations of the folder's source files,
//! named from their syntax trees.

use std::path::{Path, PathBuf};

use crate::analysis::cst::{parse_cst, CstNode};
use crate::fs::loader::read_file_content;

use super::fuzzy::{rank, FuzzyMatch};

pub const MAX_RESULTS: usize = 200;
/// Files read for the symbol index.
const MAX_FILES: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub path: PathBuf,
    pub name: String,
    /// Grammar kind of the declaration, e.g. `function_item`.
    pub kind: String,
    /// 0-based first line.
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct SymbolHit {
    pub symbol: Symbol,
    pub matched: FuzzyMatch,
}

/// Top-level declarations of one file that have a name.
pub fn file_symbols(path: &Path, content: &str) -> Vec<Symbol> {
    let Some(tree) = parse_cst(path, content) else {
        return Vec::new();
    };
    tree.root
        .children
        .iter()
        .filter(|n| n.named && !n.is_error && !n.kind.contains("comment"))
        .filter_map(|n| {
            Some(Symbol {
                path: path.to_path_buf(),
                name: declaration_name(n, 0)?,
                kind: n.kind.clone(),
                line: n.start_line,
            })
        })
        .collect()
}

/// The identifier a declaration introduces: its `name` field, else its
/// `type` (Rust `impl Foo`), else the same inside a wrapped `declaration`
/// (`export function f`) or declarator (`const x = 1`).
fn declaration_name(node: &CstNode, depth: usize) -> Option<String> {
    if depth > 3 {
        return None;
    }
    let field = |name: &str| node.children.iter().find(|c| c.field.as_deref() == Some(name));
    for wanted in ["name", "type"] {
        if let Some(child) = field(wanted) {
            if let Some(text) = &child.text {
                return Some(text.clone());
            }
            if let Some(inner) = leaf_text(child) {
                return Some(inner);
            }
        }
    }
    if let Some(inner) = field("declaration") {
        return declaration_name(inner, depth + 1);
    }
    node.children
        .iter()
        .find(|c| c.named && c.kind.ends_with("declarator"))
        .and_then(|c| declaration_name(c, depth + 1))
}

fn leaf_text(node: &CstNode) -> Option<String> {
    node.text.clone().or_else(|| node.children.iter().find_map(leaf_text))
}

/// Symbols matching `query` across `files`, best first. Reads from disk, so
/// run it on a worker thread.
pub fn search_symbols(files: &[PathBuf], query: &str) -> Vec<SymbolHit> {
    let symbols = files
        .iter()
        .take(MAX_FILES)
        .filter_map(|p| read_file_content(p).ok().map(|text| (p, text)))
        .flat_map(|(p, text)| file_symbols(p, &text));
    rank(query, symbols, |s| s.name.clone())
        .into_iter()
        .take(MAX_RESULTS)
        .map(|(symbol, matched)| SymbolHit { symbol, matched })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(path: &str, src: &str) -> Vec<String> {
        file_symbols(Path::new(path), src).into_iter().map(|s| s.name).collect()
    }

    #[test]
    fn rust_declarations_are_named() {
        let src = "use std::fmt;\nstruct Point;\nimpl Point {}\nfn run() {}\nconst MAX: u8 = 1;\n";
        assert_eq!(names("a.rs", src), vec!["Point", "Point", "run", "MAX"]);
    }

    #[test]
    fn typescript_exports_and_variables_are_named() {
        let src = "export function build() {}\nexport const value = 1;\nclass Box {}\n";
        assert_eq!(names("a.ts", src), vec!["build", "value", "Box"]);
    }

    #[test]
    fn python_and_go_are_named() {
        assert_eq!(names("a.py", "def go():\n    pass\nclass K:\n    pass\n"), vec!["go", "K"]);
        assert_eq!(names("a.go", "package main\nfunc Start() {}\n"), vec!["Start"]);
    }

    #[test]
    fn unsupported_files_have_no_symbols() {
        assert!(names("README.md", "# title").is_empty());
    }

    #[test]
    fn search_ranks_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.rs");
        std::fs::write(&a, "fn parse_input() {}\nfn print() {}\n").unwrap();
        let hits = search_symbols(&[a], "parse");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].symbol.name, "parse_input");
    }
}
