//! Dependencies between open files, from the language servers.
//!
//! For each open file the [`indexer`] asks its server for the document's
//! symbols and, for each top-level symbol or type member, for its references.
//! A reference in another open file becomes a link from the statement around
//! it ([`statements`]) to the symbol's definition.

pub mod indexer;
mod statements;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::analysis::lsp::{Location, LspSymbol, Range, SymbolKind};
pub use statements::StatementFinder;

/// Links drawn per symbol; a helper called everywhere would otherwise draw
/// dozens of edges.
pub const MAX_LINKS_PER_SYMBOL: usize = 20;

/// Lines `start..=end` of a file, 0-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LineSpan {
    pub start: usize,
    pub end: usize,
}

impl LineSpan {
    /// The lines an LSP range covers; a range ending at column 0 of a line
    /// does not cover that line.
    pub fn of_range(range: Range) -> Self {
        let end = if range.end.character == 0 && range.end.line > range.start.line {
            range.end.line - 1
        } else {
            range.end.line
        };
        LineSpan {
            start: range.start.line,
            end,
        }
    }
}

/// One edge: the statement of `source` using a symbol, and the definition of
/// that symbol in `target`. Paths are the file nodes' paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyLink {
    pub source: PathBuf,
    pub target: PathBuf,
    /// The symbol's kind, e.g. `fn` or `struct`.
    pub label: String,
    pub usage: LineSpan,
    pub definition: LineSpan,
}

/// The symbols whose references become edges: every top-level symbol except
/// containers without a name of their own (rust-analyzer's `impl` blocks), and
/// the members of top-level types (methods, nested types), never their fields
/// or a function's locals.
pub fn pick_symbols(symbols: &[LspSymbol]) -> Vec<&LspSymbol> {
    let mut out = Vec::new();
    for symbol in symbols {
        if symbol.kind != SymbolKind::Other {
            out.push(symbol);
        }
        let has_members = !matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Method
                | SymbolKind::Constructor
                | SymbolKind::Variable
                | SymbolKind::Constant
        );
        if has_members {
            out.extend(
                symbol
                    .children
                    .iter()
                    .filter(|c| !matches!(c.kind, SymbolKind::Field | SymbolKind::Other)),
            );
        }
    }
    out
}

/// The open files, looked up by the paths language servers report (which may
/// differ from the nodes' paths through symlinks).
pub struct OpenFiles<'a> {
    by_path: HashMap<PathBuf, (&'a Path, &'a str)>,
}

impl<'a> OpenFiles<'a> {
    /// `files` are `(node path, content)` pairs.
    pub fn new(files: impl IntoIterator<Item = (&'a Path, &'a str)>) -> Self {
        let mut by_path = HashMap::new();
        for (path, content) in files {
            if let Ok(canonical) = std::fs::canonicalize(path) {
                by_path.insert(canonical, (path, content));
            }
            by_path.insert(path.to_path_buf(), (path, content));
        }
        Self { by_path }
    }

    /// The node path and content of the open file at `path`.
    pub fn get(&self, path: &Path) -> Option<(&'a Path, &'a str)> {
        self.by_path.get(path).copied()
    }
}

/// Usage statements of one symbol defined in `target`, from its references:
/// `(source node path, statement lines)`, at most [`MAX_LINKS_PER_SYMBOL`].
/// References in `target` itself or in files that are not open are ignored. A
/// reference inside an import counts only when the file has no other one.
pub fn usages(
    target: &Path,
    locations: &[Location],
    open: &OpenFiles,
    finder: &mut StatementFinder,
) -> Vec<(PathBuf, LineSpan)> {
    let mut sorted: Vec<&Location> = locations.iter().collect();
    sorted.sort();
    // (source, span, is_import), in reference order per file
    let mut found: Vec<(&Path, LineSpan, bool)> = Vec::new();
    for loc in sorted {
        let Some((source, content)) = open.get(&loc.path) else {
            continue;
        };
        if source == target {
            continue;
        }
        let stmt = finder.at(source, content, loc.range.start.line, loc.range.start.character);
        if !found.iter().any(|(s, span, _)| *s == source && *span == stmt.span) {
            found.push((source, stmt.span, stmt.is_import));
        }
    }
    let uses_outside_imports = |source: &Path| found.iter().any(|(s, _, imp)| *s == source && !imp);
    found
        .iter()
        .filter(|(source, _, is_import)| !is_import || !uses_outside_imports(source))
        .take(MAX_LINKS_PER_SYMBOL)
        .map(|(source, span, _)| (source.to_path_buf(), *span))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::lsp::Position;

    fn range(line: usize, character: usize) -> Range {
        Range {
            start: Position { line, character },
            end: Position { line, character: character + 1 },
        }
    }

    fn symbol(name: &str, kind: SymbolKind, children: Vec<LspSymbol>) -> LspSymbol {
        LspSymbol {
            name: name.into(),
            detail: None,
            kind,
            range: range(0, 0),
            selection_range: range(0, 0),
            children,
        }
    }

    #[test]
    fn picks_top_level_symbols_and_type_members() {
        let symbols = vec![
            symbol("Client", SymbolKind::Struct, vec![symbol("server", SymbolKind::Field, vec![])]),
            symbol(
                "impl Client",
                SymbolKind::Other,
                vec![symbol("new", SymbolKind::Method, vec![]), symbol("greet", SymbolKind::Method, vec![])],
            ),
            symbol("run", SymbolKind::Function, vec![symbol("local", SymbolKind::Variable, vec![])]),
            symbol("Kind", SymbolKind::Enum, vec![symbol("A", SymbolKind::Field, vec![])]),
        ];
        let names: Vec<&str> = pick_symbols(&symbols).iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Client", "new", "greet", "run", "Kind"]);
    }

    #[test]
    fn line_span_of_a_range_skips_a_trailing_line_start() {
        let r = |sl, sc, el, ec| Range {
            start: Position { line: sl, character: sc },
            end: Position { line: el, character: ec },
        };
        assert_eq!(LineSpan::of_range(r(2, 0, 5, 1)), LineSpan { start: 2, end: 5 });
        assert_eq!(LineSpan::of_range(r(2, 0, 5, 0)), LineSpan { start: 2, end: 4 });
        assert_eq!(LineSpan::of_range(r(3, 4, 3, 0)), LineSpan { start: 3, end: 3 });
    }

    #[test]
    fn usages_keep_other_open_files_and_drop_imports_with_other_uses() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let (math, app, unused, closed) = (
            root.join("math.ts"),
            root.join("app.ts"),
            root.join("unused.ts"),
            root.join("closed.ts"),
        );
        let app_text = "import { add } from './math';\n\nadd(1, 2);\nconst x = add(\n  3, add(4, 5));\n";
        let contents = [
            (math.as_path(), "export function add() {}\n"),
            (app.as_path(), app_text),
            (unused.as_path(), "import { add } from './math';\n"),
        ];
        let open = OpenFiles::new(contents);
        let loc = |path: &Path, line, character| Location {
            path: path.to_path_buf(),
            range: range(line, character),
        };
        let locations = [
            loc(&math, 0, 16), // the declaration
            loc(&app, 0, 9),   // the import, dropped: app.ts uses add
            loc(&app, 2, 0),
            loc(&app, 3, 10),
            loc(&app, 4, 5), // same statement as line 3
            loc(&unused, 0, 9),
            loc(&closed, 0, 0),
        ];
        let span = |start, end| LineSpan { start, end };
        assert_eq!(
            usages(&math, &locations, &open, &mut StatementFinder::default()),
            [(app.clone(), span(2, 2)), (app.clone(), span(3, 4)), (unused.clone(), span(0, 0))]
        );
    }

    #[test]
    fn usages_are_capped_per_symbol() {
        let dir = tempfile::tempdir().unwrap();
        let (lib, app) = (dir.path().join("lib.rs"), dir.path().join("app.rs"));
        let app_text = "fn main() {\n".to_string() + &"    f();\n".repeat(30) + "}\n";
        let open = OpenFiles::new([(lib.as_path(), "pub fn f() {}\n"), (app.as_path(), app_text.as_str())]);
        let locations: Vec<Location> = (1..=30)
            .map(|line| Location {
                path: app.clone(),
                range: range(line, 4),
            })
            .collect();
        let found = usages(&lib, &locations, &open, &mut StatementFinder::default());
        assert_eq!(found.len(), MAX_LINKS_PER_SYMBOL);
    }
}
