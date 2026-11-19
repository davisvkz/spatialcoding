//! Symbol Mapping (`codecanvas/renderizacao/lsp.md` §7): a click in a big
//! region of code (a function body, an initializer, call arguments) has no
//! symbol under it, so one meaningful token inside or around it is chosen:
//!
//! - an identifier under the pointer is the token itself;
//! - a function or class body maps to its name;
//! - an arrow function body maps to its `=>`;
//! - an initializer maps to the variable name;
//! - the body of a `for … in/of` maps to the iterable;
//! - the arguments of a call map to the called expression.

use std::path::Path;

use tree_sitter::Node;

use super::cst::parse_tree;

/// A token of the file: 0-based line and byte columns (end exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

/// Maps the click at `line`/`column` (bytes) of `content` to a token, or
/// `None` when the file has no grammar or nothing fits.
pub fn map_click(path: &Path, content: &str, line: usize, column: usize) -> Option<Token> {
    let (_, tree) = parse_tree(path, content)?;
    let point = tree_sitter::Point { row: line, column };
    let mut node = tree.root_node().descendant_for_point_range(point, point)?;
    loop {
        if is_identifier(node) {
            return token(node);
        }
        if let Some(found) = rule(node, content.as_bytes()) {
            return Some(found);
        }
        node = node.parent()?;
    }
}

fn is_identifier(node: Node) -> bool {
    node.child_count() == 0
        && node.is_named()
        && (node.kind().contains("identifier") || node.kind() == "property_identifier" || node.kind() == "field_identifier")
}

fn token(node: Node) -> Option<Token> {
    let (s, e) = (node.start_position(), node.end_position());
    (s.row == e.row).then_some(Token { line: s.row, start: s.column, end: e.column })
}

/// The first identifier of `node` (itself, or the last of a member access
/// so `a.b.c` maps to `c`).
fn leaf_identifier(node: Node) -> Option<Node> {
    if is_identifier(node) {
        return Some(node);
    }
    let mut last = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = leaf_identifier(child) {
            last = Some(found);
            if !matches!(node.kind(), "member_expression" | "field_expression" | "attribute" | "scoped_identifier" | "selector_expression") {
                break;
            }
        }
    }
    last
}

/// The token a region maps to, for the region `node` of the ancestor walk.
fn rule(node: Node, _source: &[u8]) -> Option<Token> {
    let by_field = |field: &str| node.child_by_field_name(field).and_then(leaf_identifier).and_then(token);
    match node.kind() {
        // A function or class, clicked anywhere in its body.
        "function_item" | "function_declaration" | "function_definition" | "method_definition" | "generator_function_declaration"
        | "class_declaration" | "class_definition" | "abstract_class_declaration" | "interface_declaration" | "struct_item"
        | "enum_item" | "trait_item" | "type_alias_declaration" | "type_declaration" | "method_declaration" => by_field("name"),
        // `impl Trait for Type`: the type.
        "impl_item" => by_field("type"),
        // An arrow function: its `=>`.
        "arrow_function" => {
            let mut cursor = node.walk();
            node.children(&mut cursor).find(|c| c.kind() == "=>").and_then(token)
        }
        // A declarator with an initializer: the variable.
        "variable_declarator" | "let_declaration" | "const_item" | "static_item" | "assignment" | "short_var_declaration" => {
            by_field("name").or_else(|| by_field("pattern")).or_else(|| by_field("left"))
        }
        // The body of a for-in/of: the iterable.
        "for_in_statement" | "for_statement" | "for_expression" => by_field("right").or_else(|| by_field("value")),
        // The arguments of a call: the called expression.
        "arguments" | "argument_list" => node.parent().and_then(|call| {
            call.child_by_field_name("function")
                .or_else(|| call.child_by_field_name("macro"))
                .and_then(leaf_identifier)
                .and_then(token)
        }),
        _ => None,
    }
}

/// The name token of every top-level declaration of the file (the syntax
/// root's named children, comments out, imports out when `skip_imports`):
/// what "Highlight top-level symbols" pins. A wrapper such as `export …`
/// is looked through for the declaration it holds.
pub fn top_level_tokens(path: &Path, content: &str, skip_imports: bool) -> Vec<Token> {
    let Some((_, tree)) = parse_tree(path, content) else { return Vec::new() };
    let root = tree.root_node();
    let mut cursor = root.walk();
    let mut tokens = Vec::new();
    for node in root.named_children(&mut cursor) {
        if node.kind().contains("comment") || node.is_error() {
            continue;
        }
        if skip_imports && (node.kind().contains("import") || matches!(node.kind(), "use_declaration" | "package_clause")) {
            continue;
        }
        if let Some(token) = declared_name(node, content.as_bytes(), 3) {
            tokens.push(token);
        }
    }
    tokens
}

/// The name of the declaration `node` is, or of the first one it wraps
/// (`export class A {}`, `const a = 1`), up to `depth` levels down.
fn declared_name(node: Node, source: &[u8], depth: usize) -> Option<Token> {
    if let Some(token) = rule(node, source) {
        return Some(token);
    }
    if depth == 0 {
        return None;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find_map(|child| declared_name(child, source, depth - 1))
}

/// The 0-based UTF-16 column of byte `byte` of `line`, as LSP wants it.
pub fn utf16_column(line: &str, byte: usize) -> usize {
    line.get(..byte.min(line.len())).map_or(byte, |head| head.encode_utf16().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(src: &str, t: Token) -> String {
        src.lines().nth(t.line).unwrap()[t.start..t.end].to_string()
    }

    fn map(path: &str, src: &str, line: usize, col: usize) -> Option<String> {
        map_click(Path::new(path), src, line, col).map(|t| text_of(src, t))
    }

    #[test]
    fn an_identifier_under_the_pointer_is_the_token() {
        let src = "function hello() {\n  return world + 1;\n}\n";
        assert_eq!(map("a.ts", src, 1, 11).as_deref(), Some("world"));
        assert_eq!(map("a.ts", src, 0, 11).as_deref(), Some("hello"));
    }

    #[test]
    fn a_function_body_maps_to_the_function_name() {
        let src = "function area(w: number, h: number) {\n  const k = 2;\n  return w * h;\n}\n";
        // The `return` keyword line, no identifier under col 2.
        assert_eq!(map("a.ts", src, 2, 2).as_deref(), Some("area"));
        let rs = "fn compute(a: i32) -> i32 {\n    a + 1\n}\n";
        assert_eq!(map("a.rs", rs, 1, 1).as_deref(), Some("compute"));
    }

    #[test]
    fn a_class_body_maps_to_the_class_name() {
        let src = "class Client {\n  name = 'x';\n  ;\n}\n";
        assert_eq!(map("a.ts", src, 2, 2).as_deref(), Some("Client"));
    }

    #[test]
    fn an_arrow_function_body_maps_to_its_arrow() {
        let src = "const f = (a: number) => {\n  ;\n};\n";
        let token = map_click(Path::new("a.ts"), src, 1, 2).unwrap();
        assert_eq!(text_of(src, token), "=>");
    }

    #[test]
    fn an_initializer_maps_to_the_variable() {
        let src = "const total = 1 +\n  2;\n";
        assert_eq!(map("a.ts", src, 1, 2).as_deref(), Some("total"));
        let py = "result = [\n    1,\n]\n";
        assert_eq!(map("a.py", py, 1, 4).as_deref(), Some("result"));
    }

    #[test]
    fn for_of_maps_to_the_iterable_and_call_arguments_to_the_callee() {
        let src = "for (const x of items) {\n  ;\n}\nrun(1,\n   2);\n";
        assert_eq!(map("a.ts", src, 1, 2).as_deref(), Some("items"));
        assert_eq!(map("a.ts", src, 4, 3).as_deref(), Some("run"), "a number inside the arguments");
        let member = "api.client.send(\n  1);\n";
        assert_eq!(map("a.ts", member, 1, 2).as_deref(), Some("send"), "a member call maps to the last name");
    }

    #[test]
    fn nothing_fits_without_a_grammar_or_a_region() {
        assert!(map("notes.txt", "hello\n", 0, 1).is_none());
        assert!(map("a.ts", "\n\n", 1, 0).is_none());
    }

    #[test]
    fn top_level_tokens_name_every_declaration_without_imports_or_comments() {
        let src = "import { a } from './a';\n// note\nexport function f() {}\nconst x = 1;\nexport class C {}\n";
        let names: Vec<String> = top_level_tokens(Path::new("a.ts"), src, true)
            .into_iter()
            .map(|t| text_of(src, t))
            .collect();
        assert_eq!(names, ["f", "x", "C"]);
        let with_imports = top_level_tokens(Path::new("a.ts"), src, false);
        assert_eq!(with_imports.len(), 3, "an import declares no name here, so it adds none");
        let rs = "use std::fmt;\nfn a() {}\nstruct B;\n";
        let names: Vec<String> = top_level_tokens(Path::new("a.rs"), rs, true).into_iter().map(|t| text_of(rs, t)).collect();
        assert_eq!(names, ["a", "B"]);
    }

    #[test]
    fn utf16_columns_count_surrogate_pairs() {
        assert_eq!(utf16_column("ab", 1), 1);
        assert_eq!(utf16_column("a🚀b", 5), 3, "a + an astral char (2 units) = 3");
        assert_eq!(utf16_column("ação", 3), 2);
    }
}
