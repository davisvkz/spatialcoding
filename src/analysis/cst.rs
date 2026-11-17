//! Concrete Syntax Tree parsing with tree-sitter.
//!
//! [`parse_cst`] turns a file's content into a [`CstTree`]: a plain, owned copy
//! of the tree-sitter tree that the UI can keep and walk without holding on to
//! the parser. Grammars are compiled into the binary, one crate per language
//! (see [`CstLanguage`]).

use std::path::Path;

/// Upper bound on the nodes copied from one tree. Huge generated files would
/// otherwise produce hundreds of thousands of rows; the rest is dropped and
/// [`CstTree::truncated`] is set.
pub const MAX_CST_NODES: usize = 20_000;

/// Longest leaf text preview, in characters.
const MAX_TEXT_PREVIEW: usize = 40;

/// Languages with a bundled tree-sitter grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CstLanguage {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Go,
}

impl CstLanguage {
    pub const ALL: [CstLanguage; 6] = [
        CstLanguage::Rust,
        CstLanguage::TypeScript,
        CstLanguage::Tsx,
        CstLanguage::JavaScript,
        CstLanguage::Python,
        CstLanguage::Go,
    ];

    /// The grammar for a file, chosen by its extension (case-sensitive, like
    /// the rest of the app).
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?;
        match ext {
            "rs" => Some(CstLanguage::Rust),
            "ts" | "mts" | "cts" => Some(CstLanguage::TypeScript),
            "tsx" => Some(CstLanguage::Tsx),
            "js" | "jsx" | "mjs" | "cjs" => Some(CstLanguage::JavaScript),
            "py" | "pyi" => Some(CstLanguage::Python),
            "go" => Some(CstLanguage::Go),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CstLanguage::Rust => "Rust",
            CstLanguage::TypeScript => "TypeScript",
            CstLanguage::Tsx => "TSX",
            CstLanguage::JavaScript => "JavaScript",
            CstLanguage::Python => "Python",
            CstLanguage::Go => "Go",
        }
    }

    pub(crate) fn grammar(self) -> tree_sitter::Language {
        match self {
            CstLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
            CstLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            CstLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            CstLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            CstLanguage::Python => tree_sitter_python::LANGUAGE.into(),
            CstLanguage::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }

    /// A parser ready for this language. Fails only if the grammar's ABI is
    /// not supported by the linked tree-sitter runtime.
    pub(crate) fn parser(self) -> Option<tree_sitter::Parser> {
        let mut parser = tree_sitter::Parser::new();
        match parser.set_language(&self.grammar()) {
            Ok(()) => Some(parser),
            Err(e) => {
                log::error!("tree-sitter grammar for {} failed to load: {e}", self.name());
                None
            }
        }
    }
}

/// One node of a parsed file.
#[derive(Debug, Clone)]
pub struct CstNode {
    /// Grammar node kind, e.g. `function_item` or `(` for anonymous tokens.
    pub kind: String,
    /// Field name under the parent, e.g. `name` or `body`.
    pub field: Option<String>,
    /// Named nodes are grammar rules; anonymous ones are literal tokens.
    pub named: bool,
    /// An `ERROR` node or a token the parser had to insert (`MISSING`).
    pub is_error: bool,
    /// First line, 0-based.
    pub start_line: usize,
    /// Last line the node covers, 0-based and inclusive. A node that ends
    /// right after a newline does not count the empty next line.
    pub end_line: usize,
    /// Source text of a leaf (first line, shortened), unless it equals `kind`.
    pub text: Option<String>,
    pub children: Vec<CstNode>,
}

/// A whole parsed file.
#[derive(Debug, Clone)]
pub struct CstTree {
    pub language: CstLanguage,
    pub root: CstNode,
    /// Nodes copied into `root`, at most [`MAX_CST_NODES`].
    pub node_count: usize,
    /// True when nodes beyond [`MAX_CST_NODES`] were dropped.
    pub truncated: bool,
}

/// Parse `content` with the grammar chosen from `path`'s extension.
/// Returns `None` when no grammar matches.
pub fn parse_cst(path: &Path, content: &str) -> Option<CstTree> {
    parse_cst_limited(path, content, MAX_CST_NODES)
}

/// The raw tree-sitter tree of `content`, for analyses that walk it directly.
pub(crate) fn parse_tree(path: &Path, content: &str) -> Option<(CstLanguage, tree_sitter::Tree)> {
    let language = CstLanguage::from_path(path)?;
    let tree = language.parser()?.parse(content, None)?;
    Some((language, tree))
}

fn parse_cst_limited(path: &Path, content: &str, max_nodes: usize) -> Option<CstTree> {
    let language = CstLanguage::from_path(path)?;
    let tree = language.parser()?.parse(content, None)?;

    let mut builder = Builder {
        source: content.as_bytes(),
        remaining: max_nodes,
        truncated: false,
    };
    let mut cursor = tree.walk();
    let root = builder.build(&mut cursor)?;
    Some(CstTree {
        language,
        root,
        node_count: max_nodes - builder.remaining,
        truncated: builder.truncated,
    })
}

struct Builder<'a> {
    source: &'a [u8],
    remaining: usize,
    truncated: bool,
}

impl Builder<'_> {
    /// Copies the cursor's node and its subtree (pre-order, so the cap keeps
    /// the top of the tree). The cursor ends where it started.
    fn build(&mut self, cursor: &mut tree_sitter::TreeCursor) -> Option<CstNode> {
        if self.remaining == 0 {
            self.truncated = true;
            return None;
        }
        self.remaining -= 1;

        let node = cursor.node();
        let start = node.start_position();
        let end = node.end_position();
        let end_line = if end.column == 0 && end.row > start.row {
            end.row - 1
        } else {
            end.row
        };
        let kind = node.kind().to_string();
        let text = if node.child_count() == 0 {
            node.utf8_text(self.source)
                .ok()
                .map(preview)
                .filter(|t| !t.is_empty() && *t != kind)
        } else {
            None
        };

        let mut cst = CstNode {
            field: cursor.field_name().map(str::to_string),
            named: node.is_named(),
            is_error: node.is_error() || node.is_missing(),
            start_line: start.row,
            end_line,
            text,
            kind,
            children: Vec::new(),
        };

        if cursor.goto_first_child() {
            while let Some(child) = self.build(cursor) {
                cst.children.push(child);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            cursor.goto_parent();
        }
        Some(cst)
    }
}

/// First line of `text`, shortened to [`MAX_TEXT_PREVIEW`] characters.
fn preview(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim_end();
    if line.chars().count() > MAX_TEXT_PREVIEW {
        let mut short: String = line.chars().take(MAX_TEXT_PREVIEW - 1).collect();
        short.push('\u{2026}');
        short
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(path: &str, content: &str) -> CstTree {
        parse_cst(Path::new(path), content).unwrap_or_else(|| panic!("{path} did not parse"))
    }

    fn named_kinds(node: &CstNode) -> Vec<&str> {
        node.children
            .iter()
            .filter(|c| c.named)
            .map(|c| c.kind.as_str())
            .collect()
    }

    fn find<'a>(node: &'a CstNode, kind: &str) -> Option<&'a CstNode> {
        if node.kind == kind {
            return Some(node);
        }
        node.children.iter().find_map(|c| find(c, kind))
    }

    #[test]
    fn every_grammar_loads() {
        for lang in CstLanguage::ALL {
            assert!(lang.parser().is_some(), "{}", lang.name());
        }
    }

    #[test]
    fn each_language_parses_to_its_root_kind() {
        let cases = [
            ("a.rs", "fn main() {}\n", "source_file", "function_item"),
            ("a.ts", "const a: number = 1;\n", "program", "lexical_declaration"),
            ("a.tsx", "const v = <div>hi</div>;\n", "program", "lexical_declaration"),
            ("a.js", "function f() {}\n", "program", "function_declaration"),
            ("a.py", "def f():\n    pass\n", "module", "function_definition"),
            ("a.go", "package main\n", "source_file", "package_clause"),
        ];
        for (path, src, root, first) in cases {
            let tree = parse(path, src);
            assert_eq!(tree.root.kind, root, "{path}");
            assert_eq!(named_kinds(&tree.root), [first], "{path}");
            assert!(!tree.truncated);
        }
    }

    #[test]
    fn tsx_uses_the_jsx_grammar() {
        let src = "const v = <div>hi</div>;\n";
        assert!(find(&parse("a.tsx", src).root, "jsx_element").is_some());
        assert_eq!(CstLanguage::from_path(Path::new("a.tsx")), Some(CstLanguage::Tsx));
        assert_eq!(CstLanguage::from_path(Path::new("a.mjs")), Some(CstLanguage::JavaScript));
        assert_eq!(CstLanguage::from_path(Path::new("a.pyi")), Some(CstLanguage::Python));
    }

    #[test]
    fn unsupported_extensions_have_no_tree() {
        for path in ["README.md", "Cargo.toml", "Makefile", "a.RS"] {
            assert!(parse_cst(Path::new(path), "x").is_none(), "{path}");
        }
    }

    #[test]
    fn lines_are_zero_based_and_skip_the_trailing_newline() {
        let tree = parse("a.rs", "mod util;\n\nfn main() {\n    util::hello();\n}\n");
        assert_eq!((tree.root.start_line, tree.root.end_line), (0, 4));

        let main = find(&tree.root, "function_item").unwrap();
        assert_eq!((main.start_line, main.end_line), (2, 4));
        let module = find(&tree.root, "mod_item").unwrap();
        assert_eq!((module.start_line, module.end_line), (0, 0));
    }

    #[test]
    fn fields_named_flags_and_leaf_text() {
        let tree = parse("a.rs", "fn hello() {}\n");
        let func = find(&tree.root, "function_item").unwrap();
        let name = func.children.iter().find(|c| c.kind == "identifier").unwrap();
        assert_eq!(name.field.as_deref(), Some("name"));
        assert_eq!(name.text.as_deref(), Some("hello"));
        assert!(name.named);

        let keyword = func.children.iter().find(|c| c.kind == "fn").unwrap();
        assert!(!keyword.named);
        assert_eq!(keyword.text, None, "text equal to the kind is omitted");
    }

    #[test]
    fn syntax_errors_are_flagged() {
        let tree = parse("a.py", "def f(:\n");
        fn any_error(n: &CstNode) -> bool {
            n.is_error || n.children.iter().any(any_error)
        }
        assert!(any_error(&tree.root));
        assert!(!any_error(&parse("a.py", "x = 1\n").root));
    }

    #[test]
    fn node_cap_truncates_in_preorder() {
        let src = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let full = parse("a.rs", src);
        assert!(full.node_count > 5);

        let capped = parse_cst_limited(Path::new("a.rs"), src, 5).unwrap();
        assert_eq!(capped.node_count, 5);
        assert!(capped.truncated);
        assert_eq!(capped.root.kind, "source_file");
    }

    #[test]
    fn preview_takes_the_first_line_and_shortens_it() {
        assert_eq!(preview("abc\ndef"), "abc");
        let long = "x".repeat(100);
        let p = preview(&long);
        assert_eq!(p.chars().count(), MAX_TEXT_PREVIEW);
        assert!(p.ends_with('\u{2026}'));
    }
}
