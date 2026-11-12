//! Concrete Syntax Tree visualization using tree-sitter.
//! Currently a stub.

use std::path::Path;

/// A simplified CST node for display.
#[derive(Debug, Clone)]
pub struct CstNode {
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub children: Vec<CstNode>,
}

/// Parse a file into a CST.
/// Stub: returns None.
pub fn parse_cst(_path: &Path, _content: &str) -> Option<CstNode> {
    // TODO: Implement with tree-sitter
    // - Load language grammar based on file extension
    // - Parse content into tree-sitter Tree
    // - Convert to simplified CstNode hierarchy
    None
}
