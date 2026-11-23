//! Language Server Protocol client: document symbols and references.
//!
//! - [`protocol`]: message framing, `file://` URIs and response parsing.
//! - [`client`]: a blocking JSON-RPC client over a server's stdio.
//! - [`manager`]: the non-blocking front end the UI uses; one worker thread
//!   per running server.
//!
//! Servers are external programs. [`ServerId::command`] uses the one in
//! `SPATIALCODING_LSP_<SERVER>`, else the first default command found on
//! `PATH`, else a copy downloaded by [`install`].

pub mod client;
pub mod environment;
pub mod graph;
pub mod install;
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

/// What a server is told besides the workspace folder.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerSettings {
    /// `initializationOptions` of `initialize`; rust-analyzer reads its whole
    /// configuration from there.
    pub initialization_options: Option<serde_json::Value>,
    /// A tree of settings: answers `workspace/configuration` by section
    /// (`python.analysis` is `tree["python"]["analysis"]`) and is sent with
    /// `workspace/didChangeConfiguration`. `Null` when there is nothing to say.
    pub configuration: serde_json::Value,
}

/// Directories listed while looking for Cargo manifests, at most.
const MANIFEST_SCAN_DIRS: usize = 10_000;
/// How deep below the root manifests are looked for.
const MANIFEST_SCAN_DEPTH: usize = 6;

/// The `Cargo.toml` files rust-analyzer must be pointed at when `root` has no
/// manifest of its own (a folder of crates). With a manifest or a
/// `rust-project.json` in `root` its own discovery finds the workspace, and
/// linking the members as well would load them twice. Only the outermost
/// manifests count, since a workspace root lists its members itself.
pub fn rust_linked_projects(root: &Path) -> Vec<PathBuf> {
    if root.join("Cargo.toml").is_file() || root.join("rust-project.json").is_file() {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut listed = 0;
    while let Some((dir, depth)) = queue.pop_front() {
        listed += 1;
        if listed > MANIFEST_SCAN_DIRS {
            break;
        }
        let manifest = dir.join("Cargo.toml");
        if dir != root && manifest.is_file() {
            found.push(manifest);
            continue;
        }
        if depth >= MANIFEST_SCAN_DEPTH {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut children: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| !crate::fs::loader::is_skipped_name(&e.file_name().to_string_lossy(), false))
            .map(|e| e.path())
            .collect();
        children.sort();
        queue.extend(children.into_iter().map(|c| (c, depth + 1)));
    }
    found.sort();
    found
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

    /// What to tell this server for the workspace `root`.
    pub fn settings(self, root: &Path) -> ServerSettings {
        use serde_json::json;
        match self {
            ServerId::Rust => {
                let projects = rust_linked_projects(root);
                ServerSettings {
                    initialization_options: (!projects.is_empty()).then(|| json!({"linkedProjects": projects})),
                    configuration: serde_json::Value::Null,
                }
            }
            // Check the files that are open, not the whole workspace: the canvas
            // shows a few files of a big repository. Other Python servers ignore
            // these sections.
            ServerId::Python => ServerSettings {
                initialization_options: None,
                configuration: json!({
                    "python": {"analysis": {"diagnosticMode": "openFilesOnly"}},
                    "basedpyright": {"analysis": {"diagnosticMode": "openFilesOnly"}},
                }),
            },
            ServerId::TypeScript | ServerId::Go => ServerSettings::default(),
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
    /// default found on `PATH`, else the copy [`install`] downloaded.
    pub fn command(self) -> Result<Vec<String>, String> {
        self.command_in(None)
    }

    /// [`Self::command`] looking the default commands up in `path` (the
    /// project's `PATH`, see [`environment`]) instead of the app's own.
    pub fn command_in(self, path: Option<&str>) -> Result<Vec<String>, String> {
        let dir = install::install_dir();
        resolve_command_with(self, |key| std::env::var(key).ok(), |program| on_path_in(program, path), |server| {
            install::managed_command(server, &dir)
        })
    }

    /// Where this server would come from, with the lookups injected.
    pub fn availability_with(
        self,
        dir: &Path,
        env: impl Fn(&str) -> Option<String>,
        exists: impl Fn(&str) -> bool,
    ) -> Availability {
        if env(self.env_var()).is_some() {
            Availability::Override
        } else if self.candidates().iter().any(|cmd| exists(cmd[0])) {
            Availability::Path
        } else if install::managed_command(self, dir).is_some() {
            Availability::Managed
        } else {
            Availability::Missing
        }
    }
}

/// Where a language server's command comes from (see [`ServerId::command`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// `SPATIALCODING_LSP_<SERVER>` is set.
    Override,
    /// A default command is on `PATH`.
    Path,
    /// A copy downloaded into the install directory.
    Managed,
    Missing,
}

/// [`ServerId::command`] without the managed install: the environment and
/// `PATH` lookup are injected.
pub fn resolve_command(
    server: ServerId,
    env: impl Fn(&str) -> Option<String>,
    exists: impl Fn(&str) -> bool,
) -> Result<Vec<String>, String> {
    resolve_command_with(server, env, exists, |_| None)
}

/// [`ServerId::command`] with the environment, `PATH` lookup and managed
/// install injected. `PATH` wins over the managed copy, so a server the user
/// installed themselves is never shadowed.
pub fn resolve_command_with(
    server: ServerId,
    env: impl Fn(&str) -> Option<String>,
    exists: impl Fn(&str) -> bool,
    managed: impl Fn(ServerId) -> Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    if let Some(value) = env(server.env_var()) {
        let words: Vec<String> = value.split_whitespace().map(String::from).collect();
        if words.is_empty() {
            return Err(format!("{} is set but empty.", server.env_var()));
        }
        return Ok(words);
    }
    if let Some(cmd) = server.candidates().iter().find(|cmd| exists(cmd[0])) {
        return Ok(cmd.iter().map(|s| s.to_string()).collect());
    }
    managed(server).ok_or_else(|| {
        let names: Vec<&str> = server.candidates().iter().map(|c| c[0]).collect();
        format!(
            "No {} language server found on PATH (tried {}). Install it from the LSP tab's \u{22ee} menu \u{203a} Language servers, or set {} to its command.",
            server.name(),
            names.join(", "),
            server.env_var()
        )
    })
}

/// True if `program` is an executable file in a `PATH` directory.
fn on_path(program: &str) -> bool {
    on_path_in(program, None)
}

/// [`on_path`] in `path`, or in the process's `PATH` when `None`.
fn on_path_in(program: &str, path: Option<&str>) -> bool {
    match path.map(std::ffi::OsString::from).or_else(|| std::env::var_os("PATH")) {
        Some(path) => std::env::split_paths(&path).any(|dir| is_executable(&dir.join(program))),
        None => false,
    }
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

    fn tree(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            let path = dir.path().join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        dir
    }

    #[test]
    fn a_folder_of_crates_links_the_outermost_manifests_only() {
        let dir = tree(&[
            "svc/a/Cargo.toml",
            "svc/a/crates/member/Cargo.toml", // a member: svc/a lists it itself
            "svc/b/Cargo.toml",
            "tools/c/Cargo.toml",
            "svc/b/target/debug/stale/Cargo.toml", // build output
            "node_modules/pkg/Cargo.toml",
            "docs/readme.md",
        ]);
        let root = dir.path();
        let linked: Vec<PathBuf> = rust_linked_projects(root);
        let expected: Vec<PathBuf> =
            ["svc/a/Cargo.toml", "svc/b/Cargo.toml", "tools/c/Cargo.toml"].iter().map(|p| root.join(p)).collect();
        assert_eq!(linked, expected);
        let settings = ServerId::Rust.settings(root);
        assert_eq!(settings.initialization_options, Some(serde_json::json!({"linkedProjects": expected})));
    }

    #[test]
    fn a_root_with_its_own_manifest_is_left_to_rust_analyzer() {
        let workspace = tree(&["Cargo.toml", "crates/a/Cargo.toml"]);
        assert!(rust_linked_projects(workspace.path()).is_empty(), "its discovery finds the members");
        assert_eq!(ServerId::Rust.settings(workspace.path()), ServerSettings::default());
        let project = tree(&["rust-project.json", "x/Cargo.toml"]);
        assert!(rust_linked_projects(project.path()).is_empty());
        let none = tree(&["README.md"]);
        assert!(rust_linked_projects(none.path()).is_empty() && ServerId::Rust.settings(none.path()).initialization_options.is_none());
    }

    #[test]
    fn manifests_deeper_than_the_scan_depth_are_not_looked_for() {
        let shallow = tree(&["a/b/c/d/e/f/Cargo.toml"]);
        assert_eq!(rust_linked_projects(shallow.path()).len(), 1, "six folders down is still found");
        let deep = tree(&["a/b/c/d/e/f/g/Cargo.toml"]);
        assert!(rust_linked_projects(deep.path()).is_empty());
    }

    #[test]
    fn python_servers_check_open_files_only_and_the_others_need_nothing() {
        let settings = ServerId::Python.settings(Path::new("/p"));
        assert_eq!(settings.configuration["python"]["analysis"]["diagnosticMode"], "openFilesOnly");
        assert_eq!(settings.configuration["basedpyright"]["analysis"]["diagnosticMode"], "openFilesOnly");
        assert!(settings.initialization_options.is_none());
        assert_eq!(ServerId::Go.settings(Path::new("/p")), ServerSettings::default());
        assert_eq!(ServerId::TypeScript.settings(Path::new("/p")), ServerSettings::default());
    }

    #[test]
    fn default_commands_are_found_in_the_projects_path_not_only_the_apps() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("gopls");
        std::fs::write(&tool, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let project_path = dir.path().to_str().unwrap();
        let resolve = |path: Option<&str>| {
            resolve_command_with(ServerId::Go, |_| None, |p| on_path_in(p, path), |_| None)
        };
        assert_eq!(resolve(Some(project_path)), Ok(vec!["gopls".to_string()]));
        assert!(resolve(Some("/nonexistent")).is_err(), "the app's own PATH is not consulted when one is given");
        assert!(on_path_in("gopls", Some(project_path)) && !on_path_in("gopls", Some("")));
    }
}
