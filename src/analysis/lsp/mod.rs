//! Language Server Protocol client: document symbols and references.
//!
//! - [`protocol`]: message framing, `file://` URIs and response parsing.
//! - [`client`]: a blocking JSON-RPC client over a server's stdio.
//! - [`manager`]: the non-blocking front end the UI uses; one worker thread
//!   per running server.
//!
//! Servers are external programs. [`ServerId::command`] picks the first default
//! command found on `PATH`, or the one in `SPATIALCODING_LSP_<SERVER>`.

pub mod client;
pub mod graph;
pub mod manager;
pub mod project_config;
pub mod protocol;

use std::path::{Path, PathBuf};

use crate::analysis::cst::CstLanguage;

/// A position as LSP sends it: 0-based line, UTF-16 column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Position {
    pub line: usize,
    pub character: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// A symbol of one document, with its nested symbols.
#[derive(Debug, Clone, PartialEq)]
pub struct LspSymbol {
    pub name: String,
    pub detail: Option<String>,
    pub kind: SymbolKind,
    /// The whole symbol, body included.
    pub range: Range,
    /// The part to put the cursor on, usually the name.
    pub selection_range: Range,
    pub children: Vec<LspSymbol>,
}

/// A place in a file, as returned by `textDocument/references`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Location {
    pub path: PathBuf,
    pub range: Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Module,
    Class,
    Method,
    Field,
    Constructor,
    Enum,
    Interface,
    Function,
    Variable,
    Constant,
    Struct,
    Trait,
    Other,
}

impl SymbolKind {
    /// Maps LSP's `SymbolKind` number. LSP has no trait kind; rust-analyzer
    /// reports traits as interfaces.
    pub fn from_lsp(kind: u64) -> Self {
        match kind {
            2..=4 => SymbolKind::Module,
            5 => SymbolKind::Class,
            6 => SymbolKind::Method,
            7 | 8 | 22 => SymbolKind::Field,
            9 => SymbolKind::Constructor,
            10 => SymbolKind::Enum,
            11 => SymbolKind::Interface,
            12 => SymbolKind::Function,
            13 => SymbolKind::Variable,
            14 => SymbolKind::Constant,
            23 => SymbolKind::Struct,
            _ => SymbolKind::Other,
        }
    }

    /// Short label shown before the symbol name; empty for [`SymbolKind::Other`]
    /// (rust-analyzer's `impl` blocks, whose name already says what they are).
    pub fn label(self) -> &'static str {
        match self {
            SymbolKind::Module => "mod",
            SymbolKind::Class => "class",
            SymbolKind::Method => "method",
            SymbolKind::Field => "field",
            SymbolKind::Constructor => "ctor",
            SymbolKind::Enum => "enum",
            SymbolKind::Interface => "interface",
            SymbolKind::Function => "fn",
            SymbolKind::Variable => "var",
            SymbolKind::Constant => "const",
            SymbolKind::Struct => "struct",
            SymbolKind::Trait => "trait",
            SymbolKind::Other => "",
        }
    }
}

/// A language server the app knows how to start. One server may handle
/// several languages (TypeScript also serves JavaScript).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ServerId {
    Rust,
    TypeScript,
    Python,
    Go,
}

impl ServerId {
    pub const ALL: [ServerId; 4] = [
        ServerId::Rust,
        ServerId::TypeScript,
        ServerId::Python,
        ServerId::Go,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ServerId::Rust => "Rust",
            ServerId::TypeScript => "TypeScript",
            ServerId::Python => "Python",
            ServerId::Go => "Go",
        }
    }

    /// Environment variable that overrides the server command, e.g.
    /// `SPATIALCODING_LSP_RUST="rust-analyzer"`.
    pub fn env_var(self) -> &'static str {
        match self {
            ServerId::Rust => "SPATIALCODING_LSP_RUST",
            ServerId::TypeScript => "SPATIALCODING_LSP_TYPESCRIPT",
            ServerId::Python => "SPATIALCODING_LSP_PYTHON",
            ServerId::Go => "SPATIALCODING_LSP_GO",
        }
    }

    /// Default commands, in order of preference.
    pub fn candidates(self) -> &'static [&'static [&'static str]] {
        match self {
            ServerId::Rust => &[&["rust-analyzer"]],
            ServerId::TypeScript => &[&["typescript-language-server", "--stdio"]],
            ServerId::Python => &[
                &["pyright-langserver", "--stdio"],
                &["basedpyright-langserver", "--stdio"],
                &["pylsp"],
            ],
            ServerId::Go => &[&["gopls"]],
        }
    }

    /// The command to start this server: the env override, else the first
    /// default found on `PATH`.
    pub fn command(self) -> Result<Vec<String>, String> {
        resolve_command(self, |key| std::env::var(key).ok(), on_path)
    }
}

/// [`ServerId::command`] with the environment and `PATH` lookup injected.
pub fn resolve_command(
    server: ServerId,
    env: impl Fn(&str) -> Option<String>,
    exists: impl Fn(&str) -> bool,
) -> Result<Vec<String>, String> {
    if let Some(value) = env(server.env_var()) {
        let words: Vec<String> = value.split_whitespace().map(String::from).collect();
        if words.is_empty() {
            return Err(format!("{} is set but empty.", server.env_var()));
        }
        return Ok(words);
    }
    server
        .candidates()
        .iter()
        .find(|cmd| exists(cmd[0]))
        .map(|cmd| cmd.iter().map(|s| s.to_string()).collect())
        .ok_or_else(|| {
            let names: Vec<&str> = server.candidates().iter().map(|c| c[0]).collect();
            format!(
                "No {} language server found on PATH (tried {}). Install one or set {} to its command.",
                server.name(),
                names.join(", "),
                server.env_var()
            )
        })
}

/// True if `program` is an executable file in a `PATH` directory.
fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| is_executable(&dir.join(program)))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file() || path.with_extension("exe").is_file() || path.with_extension("cmd").is_file()
}

/// The server for a file and the LSP `languageId` to open it with.
pub fn server_for(path: &Path) -> Option<(ServerId, &'static str)> {
    let jsx = path.extension().is_some_and(|e| e == "jsx");
    Some(match CstLanguage::from_path(path)? {
        CstLanguage::Rust => (ServerId::Rust, "rust"),
        CstLanguage::TypeScript => (ServerId::TypeScript, "typescript"),
        CstLanguage::Tsx => (ServerId::TypeScript, "typescriptreact"),
        CstLanguage::JavaScript if jsx => (ServerId::TypeScript, "javascriptreact"),
        CstLanguage::JavaScript => (ServerId::TypeScript, "javascript"),
        CstLanguage::Python => (ServerId::Python, "python"),
        CstLanguage::Go => (ServerId::Go, "go"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_for_maps_extensions_and_language_ids() {
        let cases = [
            ("a.rs", Some((ServerId::Rust, "rust"))),
            ("a.ts", Some((ServerId::TypeScript, "typescript"))),
            ("a.tsx", Some((ServerId::TypeScript, "typescriptreact"))),
            ("a.js", Some((ServerId::TypeScript, "javascript"))),
            ("a.jsx", Some((ServerId::TypeScript, "javascriptreact"))),
            ("a.py", Some((ServerId::Python, "python"))),
            ("a.go", Some((ServerId::Go, "go"))),
            ("README.md", None),
            ("Makefile", None),
        ];
        for (path, expected) in cases {
            assert_eq!(server_for(Path::new(path)), expected, "{path}");
        }
    }

    #[test]
    fn resolve_command_prefers_the_env_override() {
        let env = |k: &str| (k == "SPATIALCODING_LSP_RUST").then(|| "  ra-wrapper  --log x ".to_string());
        let cmd = resolve_command(ServerId::Rust, env, |_| false).unwrap();
        assert_eq!(cmd, ["ra-wrapper", "--log", "x"]);

        let empty = resolve_command(ServerId::Rust, |_| Some(" ".into()), |_| true);
        assert!(empty.unwrap_err().contains("SPATIALCODING_LSP_RUST"));
    }

    #[test]
    fn resolve_command_takes_the_first_default_on_path() {
        let cmd = resolve_command(ServerId::Python, |_| None, |p| p == "pylsp").unwrap();
        assert_eq!(cmd, ["pylsp"]);
        let cmd = resolve_command(ServerId::TypeScript, |_| None, |_| true).unwrap();
        assert_eq!(cmd, ["typescript-language-server", "--stdio"]);
    }

    #[test]
    fn resolve_command_explains_a_missing_server() {
        let err = resolve_command(ServerId::Go, |_| None, |_| false).unwrap_err();
        assert!(err.contains("gopls"), "{err}");
        assert!(err.contains("SPATIALCODING_LSP_GO"), "{err}");
    }

    #[test]
    fn symbol_kinds_map_from_lsp_numbers() {
        assert_eq!(SymbolKind::from_lsp(12), SymbolKind::Function);
        assert_eq!(SymbolKind::from_lsp(23), SymbolKind::Struct);
        assert_eq!(SymbolKind::from_lsp(6), SymbolKind::Method);
        assert_eq!(SymbolKind::from_lsp(2), SymbolKind::Module);
        assert_eq!(SymbolKind::from_lsp(99), SymbolKind::Other);
        assert_eq!(SymbolKind::Function.label(), "fn");
    }
}
