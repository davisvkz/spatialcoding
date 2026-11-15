//! The statement around a position of a source file, found with tree-sitter.
//!
//! Language servers answer with positions; an edge needs the statement there:
//! the closest ancestor whose parent holds statements. When a body (block)
//! of that statement starts after the position, the span stops before it, so a
//! type in a function signature or a decorator marks only the header.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Tree};

use super::LineSpan;
use crate::analysis::cst::{parse_tree, CstLanguage};

/// Longer statements (a big JSX `return (…)`, a long literal) shrink to the
/// line of the position itself.
const MAX_STATEMENT_LINES: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Rust,
    Ts,
    Python,
    Go,
}

impl Family {
    fn of(lang: CstLanguage) -> Self {
        match lang {
            CstLanguage::Rust => Family::Rust,
            CstLanguage::TypeScript | CstLanguage::Tsx | CstLanguage::JavaScript => Family::Ts,
            CstLanguage::Python => Family::Python,
            CstLanguage::Go => Family::Go,
        }
    }

    /// Nodes whose children are statements.
    fn is_block_like(self, kind: &str) -> bool {
        match self {
            Family::Rust => matches!(
                kind,
                "source_file"
                    | "block"
                    | "declaration_list"
                    | "field_declaration_list"
                    | "enum_variant_list"
                    | "match_block"
            ),
            Family::Ts => matches!(
                kind,
                "program"
                    | "statement_block"
                    | "class_body"
                    | "interface_body"
                    | "enum_body"
                    | "object_type"
                    | "switch_case"
                    | "switch_default"
                    | "else_clause"
            ),
            Family::Python => matches!(kind, "module" | "block"),
            Family::Go => matches!(kind, "source_file" | "statement_list"),
        }
    }

    /// The body of a compound statement (a function, a class, an `if`).
    fn is_body(self, kind: &str) -> bool {
        self.is_block_like(kind) || kind == "block"
    }

    fn is_import(self, node: Node) -> bool {
        let kind = node.kind();
        match self {
            Family::Rust => matches!(kind, "use_declaration" | "extern_crate_declaration"),
            Family::Ts => {
                kind == "import_statement"
                    || (kind == "export_statement" && node.child_by_field_name("source").is_some())
            }
            Family::Python => matches!(
                kind,
                "import_statement" | "import_from_statement" | "future_import_statement"
            ),
            Family::Go => kind == "import_declaration",
        }
    }
}

/// The statement at a position: its lines, and whether it is an import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Statement {
    pub span: LineSpan,
    pub is_import: bool,
}

/// Parses each file once and answers [`Statement`] lookups in it.
#[derive(Default)]
pub struct StatementFinder {
    trees: HashMap<PathBuf, Option<(Family, Tree)>>,
}

impl StatementFinder {
    /// Classifies an already proven binding (LSP reference), never a name
    /// search. Attributes/arguments inside JSX or class bodies stay references.
    pub fn relationship_at(
        &mut self,
        path: &Path,
        content: &str,
        line: usize,
        character: usize,
    ) -> crate::edges::style::RelationshipType {
        use crate::edges::style::RelationshipType;
        if self.at(path, content, line, character).is_import {
            return RelationshipType::Imports;
        }
        let Some((_, tree)) = self.trees.get(path).and_then(Option::as_ref) else {
            return RelationshipType::References;
        };
        let Some(offset) = byte_offset(content, line, character) else {
            return RelationshipType::References;
        };
        let Some(node) = tree.root_node().descendant_for_byte_range(offset, offset) else {
            return RelationshipType::References;
        };
        let contains =
            |container: Node| container.start_byte() <= offset && offset < container.end_byte();
        let mut current = Some(node);
        while let Some(n) = current {
            if matches!(
                n.kind(),
                "type_arguments" | "keyword_argument" | "argument_list" | "arguments"
            ) && !n.parent().is_some_and(|p| p.kind() == "class_definition")
            {
                return RelationshipType::References;
            }
            if n.kind() == "keyword_argument" {
                return RelationshipType::References;
            }
            if matches!(
                n.kind(),
                "jsx_opening_element" | "jsx_self_closing_element" | "jsx_closing_element"
            ) && n.child_by_field_name("name").is_some_and(contains)
            {
                return RelationshipType::Renders;
            }
            if matches!(
                n.kind(),
                "extends_clause" | "extends_type_clause" | "implements_clause" | "supertrait_list"
            ) {
                return RelationshipType::Inherits;
            }
            if (n.kind() == "class_definition"
                && n.child_by_field_name("superclasses").is_some_and(contains))
                || (n.kind() == "trait_item"
                    && n.child_by_field_name("bounds").is_some_and(contains))
                || (n.kind() == "impl_item" && n.child_by_field_name("trait").is_some_and(contains))
            {
                return RelationshipType::Inherits;
            }
            current = n.parent();
        }
        RelationshipType::References
    }

    /// The statement of `content` (the text of `path`) at `line` and UTF-16
    /// column `character`, as language servers count them. Without a grammar
    /// it is the line itself.
    pub fn at(&mut self, path: &Path, content: &str, line: usize, character: usize) -> Statement {
        let parsed = self.trees.entry(path.to_path_buf()).or_insert_with(|| {
            parse_tree(path, content).map(|(lang, tree)| (Family::of(lang), tree))
        });
        let fallback = Statement {
            span: LineSpan {
                start: line,
                end: line,
            },
            is_import: false,
        };
        let (Some((family, tree)), Some(offset)) =
            (parsed.as_ref(), byte_offset(content, line, character))
        else {
            return fallback;
        };
        let root = tree.root_node();
        let end = (offset + 1).min(content.len());
        let node = root.descendant_for_byte_range(offset, end).unwrap_or(root);
        Statement {
            span: statement_span(*family, node),
            is_import: ancestor(node, |n| family.is_import(n)).is_some(),
        }
    }
}

/// Byte offset of `line` and UTF-16 column `character` in `content`.
pub fn byte_offset(content: &str, line: usize, character: usize) -> Option<usize> {
    let line_start = if line == 0 {
        0
    } else {
        content.match_indices('\n').nth(line - 1)?.0 + 1
    };
    let text = &content[line_start..];
    let mut units = 0;
    for (i, c) in text.char_indices() {
        if units >= character || c == '\n' {
            return Some(line_start + i);
        }
        units += c.len_utf16();
    }
    Some(content.len())
}

/// `node` or its closest ancestor matching `pred`.
fn ancestor<'t>(node: Node<'t>, pred: impl Fn(Node) -> bool) -> Option<Node<'t>> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if pred(n) {
            return Some(n);
        }
        cur = n.parent();
    }
    None
}

/// Last line a node covers; a node ending right after a newline does not
/// count the empty next line.
fn end_row(node: Node) -> usize {
    let (start, end) = (node.start_position(), node.end_position());
    if end.column == 0 && end.row > start.row {
        end.row - 1
    } else {
        end.row
    }
}

fn statement_span(family: Family, node: Node) -> LineSpan {
    let mut path = vec![node];
    let mut stmt = node;
    while let Some(parent) = stmt.parent() {
        if family.is_block_like(parent.kind()) {
            break;
        }
        stmt = parent;
        path.push(parent);
    }
    let start = stmt.start_position().row;
    let mut end = end_row(stmt);
    if let Some(body) = body_after(family, &path, node.end_byte()) {
        end = body.prev_sibling().map_or(start, end_row).clamp(start, end);
    }
    if end - start + 1 > MAX_STATEMENT_LINES {
        return LineSpan {
            start: node.start_position().row,
            end: end_row(node),
        };
    }
    LineSpan { start, end }
}

/// The first body starting after byte `after`, among the children and
/// grandchildren of the nodes on `path` (deepest first).
fn body_after<'t>(family: Family, path: &[Node<'t>], after: usize) -> Option<Node<'t>> {
    for n in path {
        let mut cursor = n.walk();
        for child in n.named_children(&mut cursor) {
            if child.start_byte() < after {
                continue;
            }
            if family.is_body(child.kind()) {
                return Some(child);
            }
            let mut inner = child.walk();
            if let Some(body) = child
                .named_children(&mut inner)
                .find(|gc| gc.start_byte() >= after && family.is_body(gc.kind()))
            {
                return Some(body);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relationships_follow_syntax_at_the_proven_reference_not_symbol_kind() {
        use crate::edges::style::RelationshipType::{Imports, Inherits, References, Renders};
        let cases = [
            ("a.tsx", "import { Base } from './base';", "Base", Imports),
            ("a.tsx", "class Child extends Base {}", "Base", Inherits),
            ("a.ts", "interface Child extends Base {}", "Base", Inherits),
            ("a.ts", "class Child implements Base {}", "Base", Inherits),
            (
                "a.tsx",
                "const v = <Alias value={Other} />;",
                "Alias",
                Renders,
            ),
            (
                "a.tsx",
                "const v = <Alias value={Other} />;",
                "Other",
                References,
            ),
            ("a.tsx", "const v = <UI.Button />;", "Button", Renders),
            ("a.py", "class Child(Base):\n    pass\n", "Base", Inherits),
            (
                "a.py",
                "class Child:\n    value = Base()\n",
                "Base",
                References,
            ),
            ("a.rs", "trait Child: Base {}", "Base", Inherits),
            ("a.rs", "impl Base for Child {}", "Base", Inherits),
            ("a.ts", "const value: Base = make();", "Base", References),
            (
                "a.ts",
                "class Child extends Generic<Other> {}",
                "Other",
                References,
            ),
            (
                "a.py",
                "class Child(Base, metaclass=Meta):\n    pass\n",
                "Meta",
                References,
            ),
            (
                "a.tsx",
                "// <Alias />\nconst value = 'Base';",
                "Alias",
                References,
            ),
        ];
        for (path, text, needle, expected) in cases {
            let offset = text.find(needle).unwrap();
            let character = text[..offset].encode_utf16().count();
            assert_eq!(
                StatementFinder::default().relationship_at(Path::new(path), text, 0, character),
                expected,
                "{text}: {needle}"
            );
        }
    }

    /// The statement at the first occurrence of `needle` in `content`.
    fn at(path: &str, content: &str, needle: &str) -> Statement {
        let offset = content
            .find(needle)
            .unwrap_or_else(|| panic!("{needle} not in source"));
        let line = content[..offset].matches('\n').count();
        let line_start = content[..offset].rfind('\n').map_or(0, |i| i + 1);
        let character = content[line_start..offset].encode_utf16().count();
        StatementFinder::default().at(Path::new(path), content, line, character)
    }

    fn span(start: usize, end: usize) -> LineSpan {
        LineSpan { start, end }
    }

    #[test]
    fn statements_in_bodies_and_at_top_level() {
        let src = "use crate::a::f;\n\nfn main() {\n    let x = f(1);\n    f(2);\n}\n";
        assert_eq!(
            at("a.rs", src, "f(1)"),
            Statement {
                span: span(3, 3),
                is_import: false
            }
        );
        assert_eq!(at("a.rs", src, "f(2)").span, span(4, 4));
        assert_eq!(
            at("a.rs", src, "f;"),
            Statement {
                span: span(0, 0),
                is_import: true
            }
        );
    }

    #[test]
    fn signatures_and_decorators_mark_only_the_header() {
        let ts = "export function greet(\n  u: User,\n) {\n  return u;\n}\n";
        assert_eq!(at("a.ts", ts, "User").span, span(0, 2));
        let py = "@helper\ndef run(version=VERSION):\n    return 1\n";
        assert_eq!(at("a.py", py, "helper").span, span(0, 1));
        assert_eq!(at("a.py", py, "VERSION").span, span(0, 1));
        let go = "package main\n\nfunc main() {\n\tif store.Ok() {\n\t\treturn\n\t}\n}\n";
        assert_eq!(at("a.go", go, "store").span, span(3, 3));
    }

    #[test]
    fn imports_are_flagged_per_language() {
        assert!(at("a.ts", "import { a } from './a';\n", "a }").is_import);
        assert!(at("a.ts", "export { a } from './a';\n", "a }").is_import);
        assert!(at("a.py", "from app import VERSION\n", "VERSION").is_import);
        assert!(at("a.go", "package m\n\nimport \"x/y\"\n", "x/y").is_import);
        assert!(!at("a.tsx", "const v = <Button />;\n", "Button").is_import);
    }

    #[test]
    fn long_statements_shrink_to_the_position_line() {
        let body = "  <div />\n".repeat(20);
        let src = format!("function A() {{\n  return (\n  <Button />\n{body}  );\n}}\n");
        assert_eq!(at("a.tsx", &src, "Button").span, span(2, 2));
    }

    #[test]
    fn utf16_columns_and_unknown_languages() {
        // `é` and `😀` take 1 and 2 UTF-16 units but 2 and 4 bytes.
        let src = "fn é() {}\nfn main() { let s = \"😀\"; target(); }\n";
        assert_eq!(byte_offset(src, 1, 26), Some(src.find("target").unwrap()));
        assert_eq!(byte_offset(src, 5, 0), None);
        let plain = StatementFinder::default().at(Path::new("notes.txt"), "a\nb\n", 1, 0);
        assert_eq!(
            plain,
            Statement {
                span: span(1, 1),
                is_import: false
            }
        );
    }
}
