//! Keeps scripts/manual honest: generates a fixture with the real
//! `fixtures.sh` and checks the dependency edges that `04-edges.sh` promises.
//! Checks both static imports and symbol edges from LSP: the fake server of
//! `fake_lsp.rs` here, and real rust-analyzer in an ignored test.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use spatialcoding::analysis::dependencies::LineSpan;
use spatialcoding::analysis::lsp::manager::Launcher;
use spatialcoding::state::AppState;
use spatialcoding::workspace;

use crate::fake_lsp;

/// Runs `fixture_<name>` from scripts/manual/fixtures.sh into a temp dir,
/// opens every file of it with `launcher` as the language servers and waits
/// up to `timeout` for the dependency edges. Returns the temp dir guard, the
/// state and the root.
fn open_fixture(
    name: &str,
    launcher: Option<Launcher>,
    timeout: Duration,
) -> (tempfile::TempDir, AppState, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/manual/fixtures.sh");
    let status = Command::new("bash")
        .arg("-c")
        .arg(r#"set -euo pipefail; source "$1"; "fixture_$2" "$3""#)
        .arg("bash")
        .arg(&script)
        .arg(name)
        .arg(dir.path())
        .status()
        .expect("bash is required to run the manual fixture check");
    assert!(status.success(), "fixture_{name} failed");

    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut state = AppState {
        loaded_dir: Some(root.clone()),
        ..Default::default()
    };
    if let Some(launcher) = launcher {
        state.lsp.manager.set_launcher(launcher);
    }
    workspace::open_all_files(&mut state);
    let deadline = Instant::now() + timeout;
    while state.deps.is_waiting() {
        assert!(
            Instant::now() < deadline,
            "the language server did not answer in time"
        );
        std::thread::sleep(Duration::from_millis(10));
        workspace::poll_lsp(&mut state);
    }
    (dir, state, root)
}

fn node_id(state: &AppState, root: &Path, rel: &str) -> u64 {
    let path = root.join(rel);
    state
        .node_store
        .file_nodes
        .values()
        .find(|n| n.path == path)
        .unwrap_or_else(|| panic!("{rel} is not open"))
        .id
}

/// `(usage, definition)` lines of the edges from `source` to `target`, sorted.
fn spans(state: &AppState, root: &Path, source: &str, target: &str) -> Vec<(LineSpan, LineSpan)> {
    let (src, tgt) = (node_id(state, root, source), node_id(state, root, target));
    let mut out: Vec<_> = state
        .edge_store
        .all_edges()
        .filter(|e| e.source_node == src && e.target_node == tgt)
        .map(|e| (e.source_span.unwrap(), e.target_span.unwrap()))
        .collect();
    out.sort();
    out
}

fn span(start: usize, end: usize) -> LineSpan {
    LineSpan { start, end }
}

#[test]
fn typescript_fixture_resolves_the_config_alias_without_lsp() {
    use spatialcoding::edges::style::RelationshipType::Imports;
    let offline: Launcher = std::sync::Arc::new(|_, _| Err("offline".into()));
    let (_dir, state, root) = open_fixture("ts", Some(offline), Duration::from_secs(10));
    let (source, target) = (
        node_id(&state, &root, "src/index.ts"),
        node_id(&state, &root, "src/lib/math.ts"),
    );
    assert!(state
        .edge_store
        .all_edges()
        .any(|edge| edge.relationship == Imports
            && edge.source_node == source
            && edge.target_node == target));
}

#[test]
fn typescript_fixture_produces_and_filters_all_four_relationships() {
    use spatialcoding::edges::style::RelationshipType;
    let (_dir, mut state, root) =
        open_fixture("ts", Some(fake_lsp::launcher()), Duration::from_secs(10));
    let view = node_id(&state, &root, "src/View.tsx");
    state.edge_style.show_all = false;
    let edges: Vec<_> = state.edge_store.all_edges().collect();
    for kind in RelationshipType::ALL {
        assert!(
            edges.iter().any(|e| e.relationship == kind),
            "missing {kind:?}"
        );
        state.edge_style.relationships[kind.index()].visible = false;
        assert!(edges
            .iter()
            .filter(|e| e.relationship == kind)
            .all(|e| !e.is_visible(&state.edge_style)));
        state.edge_style.relationships[kind.index()].visible = true;
    }
    let rendered = edges
        .iter()
        .find(|e| e.source_node == view && e.relationship == RelationshipType::Renders)
        .unwrap();
    assert_eq!(rendered.source_name_span.unwrap().line, 5);
    assert!(
        rendered.name_span.is_some(),
        "definition comes from LSP, not guessed spelling"
    );
}

#[test]
fn rust_fixture_resolves_path_and_nested_modules_without_lsp() {
    let offline: Launcher = std::sync::Arc::new(|_, _| Err("offline".into()));
    let (_dir, state, root) = open_fixture("rust", Some(offline), Duration::from_secs(10));
    assert!(!spans(&state, &root, "src/main.rs", "src/net/mod.rs").is_empty());
    assert!(!spans(&state, &root, "src/main.rs", "src/inline/nested.rs").is_empty());
    assert!(!spans(&state, &root, "src/util.rs", "src/helpers/strings.rs").is_empty());
}

#[test]
fn rust_fixture_links_usage_statements_to_definitions_and_highlights_them() {
    let (_dir, mut state, root) =
        open_fixture("rust", Some(fake_lsp::launcher()), Duration::from_secs(10));

    // client.rs uses `Server` in a field (line 4) and in `Server::bind(..)`
    // (line 9), which also calls `bind` (line 7 of server.rs), and calls
    // `addr` (line 13 → 11). The `use super::server::Server;` import is not an
    // edge since the file uses `Server` elsewhere. The fake server's symbols
    // span one line.
    assert_eq!(
        spans(&state, &root, "src/net/client.rs", "src/net/server.rs"),
        [
            (span(4, 4), span(2, 2)),
            (span(9, 9), span(2, 2)),
            (span(9, 9), span(7, 7)),
            (span(13, 13), span(11, 11)),
        ]
    );

    // Selecting client.rs highlights its usages and the definitions.
    let client = node_id(&state, &root, "src/net/client.rs");
    let server = node_id(&state, &root, "src/net/server.rs");
    state.selection.select(client);
    let selected: HashSet<u64> = state.selection.selected_ids().collect();
    let highlights = state.edge_store.selected_spans(&selected);
    let has_span = |hl: &[spatialcoding::edges::edge_store::SpanHighlight], s: LineSpan| {
        hl.iter().any(|h| h.span == s)
    };
    for line in [4, 9, 13] {
        assert!(
            has_span(&highlights[&client], span(line, line)),
            "client.rs line {line}"
        );
    }
    for line in [2, 7, 11] {
        assert!(
            has_span(&highlights[&server], span(line, line)),
            "server.rs line {line}"
        );
    }

    // TASK-38: clicking bind's exact LSP name must not select Server or addr
    // just because all three definitions live in server.rs.
    let path = root.join("src/net/server.rs");
    let occurrences = state.deps.symbol_occurrences(&path, 7);
    let bind = occurrences
        .iter()
        .find(|o| o.symbol.name == "bind")
        .unwrap();
    let symbol = spatialcoding::nodes::selection::SelectedSymbol::from_lsp(
        server,
        &state.node_store.get_file(server).unwrap().content,
        bind.symbol,
    )
    .unwrap();
    state.selection.select_symbol(server, symbol, false);
    let focused = state
        .edge_store
        .focused_spans(&state.selection, &state.edge_style);
    assert!(has_span(&focused[&client], span(9, 9)));
    assert!(
        !has_span(&focused[&client], span(13, 13)),
        "addr usage stays dim"
    );
    assert_eq!(
        focused[&server].len(),
        1,
        "only bind is marked in server.rs"
    );
    let bind_edge = state
        .edge_store
        .all_edges()
        .find(|e| e.target_node == server && e.name_span.is_some_and(|n| n.line == 7))
        .unwrap();
    let name = bind_edge.source_name_span.unwrap();
    let usage_line = state
        .node_store
        .get_file(client)
        .unwrap()
        .content
        .lines()
        .nth(name.line)
        .unwrap();
    assert_eq!(
        &usage_line[name.start_byte..name.end_byte],
        "bind",
        "edge preserves the exact usage name"
    );
    let strings = state
        .node_store
        .get_file(node_id(&state, &root, "src/helpers/strings.rs"))
        .unwrap();
    assert!(strings.line_count > 50);
    assert_eq!(strings.visible_lines(), strings.line_count);
    assert!(strings
        .content
        .lines()
        .last()
        .unwrap()
        .contains("after_fifty_lines"));
    assert!(strings
        .line_anchor_y(strings.line_count - 1, state.settings.font_size)
        .is_some());
}

/// The same fixture against the real rust-analyzer:
/// `cargo test --test e2e -- --ignored rust_fixture_with_rust_analyzer`.
#[test]
#[ignore = "needs rust-analyzer on PATH and takes up to a few minutes"]
fn rust_fixture_with_rust_analyzer() {
    let (_dir, state, root) = open_fixture("rust", None, Duration::from_secs(300));
    let progress = state.deps.progress(&state.lsp.manager);
    let edges = spans(&state, &root, "src/net/client.rs", "src/net/server.rs");
    // `server: Server,` and `Server::bind(..)` → `pub struct Server { … }`
    // (lines 2-4); `Server::bind(..)` → `pub fn bind(..) { … }` (lines 7-9).
    for expected in [
        (span(4, 4), span(2, 4)),
        (span(9, 9), span(2, 4)),
        (span(9, 9), span(7, 9)),
    ] {
        assert!(
            edges.contains(&expected),
            "{expected:?} missing from {edges:?}; {progress:?}"
        );
    }
    // main.rs calls `config::Config::load`, `Client::new` and `util::shout`,
    // which util.rs only re-exports: it is defined in helpers/strings.rs.
    for target in [
        "src/config.rs",
        "src/net/client.rs",
        "src/helpers/strings.rs",
    ] {
        assert!(
            !spans(&state, &root, "src/main.rs", target).is_empty(),
            "main.rs -> {target}"
        );
    }
}
