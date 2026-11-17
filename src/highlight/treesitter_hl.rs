//! Syntax colors from tree-sitter highlight queries, in the GitHub Dark
//! palette that v2 Code Canvas paints code with.
//!
//! syntect ships no TypeScript grammar, so TS/TSX files came out uncolored.
//! The tree-sitter grammars compiled in for the CST panel (see
//! `analysis::cst`) bring highlight queries, which this module runs and turns
//! into the same `(Style, text)` spans the canvas already draws.

use std::path::Path;

use syntect::highlighting::{Color, FontStyle, Style};
use tree_sitter::{Query, QueryCursor, StreamingIterator};

use crate::analysis::cst::CstLanguage;

/// Files larger than this are left to the fallback highlighter.
const MAX_BYTES: usize = 2 * 1024 * 1024;

/// What a span is, for coloring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Text,
    Keyword,
    Function,
    Constant,
    String,
    Comment,
}

impl Class {
    fn rgb(self) -> (u8, u8, u8) {
        match self {
            Class::Text => (0xd4, 0xd4, 0xd8),
            Class::Keyword => (0xff, 0x7b, 0x72),
            Class::Function => (0xd2, 0xa8, 0xff),
            Class::Constant => (0x79, 0xc0, 0xff),
            Class::String => (0xa5, 0xd6, 0xff),
            Class::Comment => (0x8b, 0x94, 0x9e),
        }
    }

    /// Class of a highlight capture name such as `keyword.control` or
    /// `function.method`. `None` leaves the span uncolored.
    fn of_capture(name: &str) -> Option<Class> {
        if matches!(name, "variable.builtin" | "variable.language") {
            return Some(Class::Keyword);
        }
        let head = name.split('.').next().unwrap_or(name);
        match head {
            // Generic captures that a later, more specific pattern paints over.
            "variable" | "punctuation" | "none" => Some(Class::Text),
            "keyword" | "operator" | "include" | "repeat" | "conditional" | "exception"
            | "storageclass" | "attribute" => Some(Class::Keyword),
            "function" | "constructor" | "type" | "tag" | "namespace" | "module" => {
                Some(Class::Function)
            }
            "constant" | "property" | "number" | "boolean" | "float" | "field" | "label" => {
                Some(Class::Constant)
            }
            "string" | "escape" | "character" | "regex" => Some(Class::String),
            "comment" => Some(Class::Comment),
            _ => None,
        }
    }
}

/// Highlight query texts to try for a language, best first. TypeScript's query
/// only adds to JavaScript's, so the pair is tried before TypeScript alone.
fn query_sources(lang: CstLanguage) -> Vec<String> {
    match lang {
        CstLanguage::Rust => vec![tree_sitter_rust::HIGHLIGHTS_QUERY.to_owned()],
        CstLanguage::Python => vec![tree_sitter_python::HIGHLIGHTS_QUERY.to_owned()],
        CstLanguage::Go => vec![tree_sitter_go::HIGHLIGHTS_QUERY.to_owned()],
        CstLanguage::JavaScript => vec![
            format!(
                "{}\n{}",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
            ),
            tree_sitter_javascript::HIGHLIGHT_QUERY.to_owned(),
        ],
        CstLanguage::TypeScript | CstLanguage::Tsx => vec![
            format!(
                "{}\n{}",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_typescript::HIGHLIGHTS_QUERY
            ),
            tree_sitter_typescript::HIGHLIGHTS_QUERY.to_owned(),
        ],
    }
}

fn compile(lang: CstLanguage) -> Option<Query> {
    for source in query_sources(lang) {
        match Query::new(&lang.grammar(), &source) {
            Ok(query) => return Some(query),
            Err(e) => log::debug!("highlight query for {} rejected: {e}", lang.name()),
        }
    }
    log::warn!("no usable highlight query for {}", lang.name());
    None
}

/// Runs the bundled highlight queries. Queries are compiled on first use.
#[derive(Default)]
pub struct TreeSitterHighlighter {
    queries: Vec<(CstLanguage, Option<Query>)>,
}

impl TreeSitterHighlighter {
    /// Colored lines of `content`, each ending with its `\n` like syntect's, or
    /// `None` when the file has no bundled grammar (or is too large).
    pub fn highlight(&mut self, path: &Path, content: &str) -> Option<Vec<Vec<(Style, String)>>> {
        let lang = CstLanguage::from_path(path)?;
        if content.len() > MAX_BYTES {
            return None;
        }
        if !self.queries.iter().any(|(l, _)| *l == lang) {
            self.queries.push((lang, compile(lang)));
        }
        let query = self.queries.iter().find(|(l, _)| *l == lang)?.1.as_ref()?;
        let tree = lang.parser()?.parse(content, None)?;

        let classes = classify(query, tree.root_node(), content.as_bytes());
        Some(into_lines(content, &classes))
    }
}

/// The class of every byte of `src`.
fn classify(query: &Query, root: tree_sitter::Node<'_>, src: &[u8]) -> Vec<Class> {
    let names = query.capture_names();
    let mut classes = vec![Class::Text; src.len()];
    let mut cursor = QueryCursor::new();
    let mut captures = cursor.captures(query, root, src);
    while let Some((m, index)) = captures.next() {
        let capture = m.captures()[*index];
        let name = names[capture.index as usize];
        if name.starts_with('_') {
            continue;
        }
        let node = capture.node;
        let range = (node.start_byte(), node.end_byte());
        // Highlight queries list generic patterns (`(identifier) @variable`)
        // before specific ones, so a later capture paints over an earlier one,
        // and narrower nodes, visited after their parents, over those.
        let Some(class) = Class::of_capture(name) else {
            continue;
        };
        if let Some(slice) = classes.get_mut(range.0..range.1.min(src.len())) {
            slice.fill(class);
        }
    }
    classes
}

fn style(class: Class) -> Style {
    let (r, g, b) = class.rgb();
    Style {
        foreground: Color { r, g, b, a: 0xff },
        background: Color { r: 0, g: 0, b: 0, a: 0 },
        font_style: FontStyle::empty(),
    }
}

/// Splits `content` into lines (keeping `\n`) and each line into runs of one
/// class.
fn into_lines(content: &str, classes: &[Class]) -> Vec<Vec<(Style, String)>> {
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let mut spans: Vec<(Style, String)> = Vec::new();
        let mut run_start = 0;
        let bytes = line.as_bytes();
        for i in 1..=bytes.len() {
            let boundary = i == bytes.len()
                || (classes[offset + i] != classes[offset + run_start] && line.is_char_boundary(i));
            if boundary {
                spans.push((style(classes[offset + run_start]), line[run_start..i].to_owned()));
                run_start = i;
            }
        }
        lines.push(spans);
        offset += line.len();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color_of(lines: &[Vec<(Style, String)>], text: &str) -> Option<(u8, u8, u8)> {
        lines
            .iter()
            .flatten()
            .find(|(_, t)| t.trim() == text)
            .map(|(s, _)| (s.foreground.r, s.foreground.g, s.foreground.b))
    }

    fn highlight(path: &str, content: &str) -> Option<Vec<Vec<(Style, String)>>> {
        TreeSitterHighlighter::default().highlight(Path::new(path), content)
    }

    #[test]
    fn typescript_gets_github_dark_colors() {
        let src = "// note\nexport function compose(a: number) {\n  return 'x'\n}\n";
        let lines = highlight("compose.ts", src).expect("typescript is supported");
        assert_eq!(color_of(&lines, "export"), Some((0xff, 0x7b, 0x72)), "keyword");
        assert_eq!(color_of(&lines, "compose"), Some((0xd2, 0xa8, 0xff)), "function");
        assert_eq!(color_of(&lines, "// note"), Some((0x8b, 0x94, 0x9e)), "comment");
        assert_eq!(color_of(&lines, "'x'"), Some((0xa5, 0xd6, 0xff)), "string");
    }

    #[test]
    fn text_and_line_structure_are_preserved() {
        let src = "fn main() {\n    let é = \"ü\";\n}\n// no newline at end";
        let lines = highlight("main.rs", src).expect("rust is supported");
        let joined: String = lines.iter().flatten().map(|(_, t)| t.as_str()).collect();
        assert_eq!(joined, src);
        assert_eq!(lines.len(), 4);
        assert!(lines[0].last().unwrap().1.ends_with('\n'));
    }

    #[test]
    fn every_bundled_grammar_has_a_working_query() {
        let samples = [
            ("a.rs", "fn a() {}"),
            ("a.ts", "const a: number = 1"),
            ("a.tsx", "const a = <div>x</div>"),
            ("a.js", "const a = 1"),
            ("a.py", "def a():\n    pass"),
            ("a.go", "package a\nfunc A() {}"),
        ];
        for (path, src) in samples {
            let lines = highlight(path, src).unwrap_or_else(|| panic!("{path} not highlighted"));
            assert!(
                lines.iter().flatten().any(|(s, _)| s.foreground.r != 0xd4 || s.foreground.g != 0xd4),
                "{path} has no colored span"
            );
        }
    }

    #[test]
    fn unknown_extensions_are_left_to_the_fallback() {
        assert!(highlight("notes.txt", "hello").is_none());
    }
}

