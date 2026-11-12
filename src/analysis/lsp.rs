//! Language Server Protocol client for symbol information.
//! Currently a stub.

use std::path::Path;

/// A symbol found by the language server.
#[derive(Debug, Clone)]
pub struct LspSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Class,
    Variable,
    Interface,
    Enum,
    Module,
    Constant,
    Struct,
    Trait,
    Other,
}

/// Get symbols for a file.
/// Stub: returns empty vec.
pub fn get_symbols(_path: &Path) -> Vec<LspSymbol> {
    // TODO: Implement LSP client
    // - Start language server process
    // - Send textDocument/documentSymbol request
    // - Parse response into LspSymbol vec
    Vec::new()
}

/// Find references to a symbol.
/// Stub: returns empty vec.
pub fn find_references(_path: &Path, _line: usize, _column: usize) -> Vec<(std::path::PathBuf, usize, usize)> {
    // TODO: Implement textDocument/references
    Vec::new()
}
