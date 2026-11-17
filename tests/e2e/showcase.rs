//! The showcase workspace (`examples/showcase`, TASK-46): the relationships
//! that need no language server must be exactly the ones the README and
//! `scripts/manual/13-showcase.sh` promise. Inherits and Renders come from a
//! language server's references (see doc-7), so the real rust-analyzer checks
//! the Rust ones in an ignored test.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use spatialcoding::analysis::lsp::manager::Launcher;
use spatialcoding::edges::style::RelationshipType;
use spatialcoding::state::AppState;
use spatialcoding::workspace;

fn showcase_root() -> PathBuf {
    std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/showcase")).unwrap()
}

/// Opens every file of the showcase. `launcher` stands for the language
/// servers; `None` keeps the real ones.
fn open_showcase(launcher: Option<Launcher>, timeout: Duration) -> (AppState, PathBuf) {
    let root = showcase_root();
    let mut state = AppState {
        loaded_dir: Some(root.clone()),
        ..Default::default()
    };
    if let Some(launcher) = launcher {
        state.lsp.manager.set_launcher(launcher);
    }
    workspace::open_all_files(&mut state);
    let deadline = Instant::now() + timeout;
    loop {
        workspace::poll_lsp(&mut state);
        if !state.deps.is_waiting() {
            break;
        }
        assert!(Instant::now() < deadline, "the dependency analysis did not finish in time");
        std::thread::sleep(Duration::from_millis(10));
    }
    (state, root)
}

/// `(relationship, source, target)` of every edge, paths relative to the root.
fn edges(state: &AppState, root: &Path) -> BTreeSet<(&'static str, String, String)> {
    let rel = |id: u64| {
        let node = state.node_store.get_file(id).unwrap();
        node.path.strip_prefix(root).unwrap().to_string_lossy().into_owned()
    };
    state
        .edge_store
        .all_edges()
        .map(|e| (e.relationship.label(), rel(e.source_node), rel(e.target_node)))
        .collect()
}

fn has(set: &BTreeSet<(&'static str, String, String)>, kind: RelationshipType, from: &str, to: &str) -> bool {
    set.contains(&(kind.label(), from.to_owned(), to.to_owned()))
}

fn no_language_servers() -> Launcher {
    Arc::new(|server, _| Err(format!("no server, set {}", server.env_var())))
}

#[test]
fn every_language_and_document_has_local_links_without_a_language_server() {
    let (state, root) = open_showcase(Some(no_language_servers()), Duration::from_secs(20));
    let set = edges(&state, &root);
    use RelationshipType::{Imports, References};

    // TypeScript: static, re-export, dynamic and (aliased) imports.
    assert!(has(&set, Imports, "web/src/index.ts", "web/src/ui/App.tsx"));
    assert!(has(&set, Imports, "web/src/index.ts", "web/src/lazy/report.ts"), "dynamic import()");
    assert!(has(&set, Imports, "web/src/index.ts", "web/src/legacy/cjs.js"));
    assert!(has(&set, Imports, "web/src/models/index.ts", "web/src/models/Order.ts"), "barrel re-export");
    assert!(has(&set, Imports, "web/src/ui/index.ts", "web/src/ui/Card.tsx"));
    assert!(has(&set, Imports, "web/src/ui/ProductList.tsx", "web/src/ui/Button.tsx"));
    // Rust: `mod` declarations, including a nested module directory.
    assert!(has(&set, Imports, "server/src/main.rs", "server/src/config.rs"));
    assert!(has(&set, Imports, "server/src/main.rs", "server/src/domain/mod.rs"));
    assert!(has(&set, Imports, "server/src/net/mod.rs", "server/src/net/handlers/mod.rs"));
    assert!(has(&set, Imports, "server/src/net/handlers/mod.rs", "server/src/net/handlers/orders.rs"));
    // Rust: include_str! of a document.
    assert!(has(&set, References, "server/src/config.rs", "server/config/default.toml"));
    // Python: relative imports, at one and two levels.
    assert!(has(&set, Imports, "tools/shop/__init__.py", "tools/shop/models.py"));
    assert!(has(&set, Imports, "tools/shop/plugins/discount.py", "tools/shop/pricing.py"));
    // Go: module-local packages.
    assert!(has(&set, Imports, "service/main.go", "service/internal/store/store.go"));
    assert!(has(&set, Imports, "service/internal/pricing/pricing.go", "service/internal/store/store.go"));
    // Markdown and JSON documents.
    assert!(has(&set, References, "README.md", "docs/architecture.md"));
    assert!(has(&set, References, "docs/architecture.md", "web/src/models/Entity.ts"));
    assert!(has(&set, References, "docs/architecture.md", "README.md"));
    assert!(has(&set, References, "config/schema.json", "config/defs.json"), "$ref");
    assert!(has(&set, References, "config/schema.json", "config/base.json"), "extends");
    // The fenced link and the remote $ref make no edge.
    assert_eq!(
        set.iter().filter(|(_, s, _)| s == "docs/architecture.md").filter(|(_, _, t)| t == "README.md").count(),
        1
    );
    assert_eq!(set.iter().filter(|(_, s, _)| s == "config/schema.json").count(), 2, "the remote $ref is not a link");
    // Local links never reach ignored folders.
    assert!(!set.iter().any(|(_, _, t)| t.contains("node_modules") || t.contains("/target/")));
}

#[test]
fn the_showcase_has_barrels_and_hides_ignored_folders() {
    let root = showcase_root();
    let barrel = std::fs::read_to_string(root.join("web/src/models/index.ts")).unwrap();
    assert!(barrel.lines().filter(|l| l.starts_with("export")).count() >= 4, "a barrel needs 4+ re-exports");
    let ui = std::fs::read_to_string(root.join("web/src/ui/index.ts")).unwrap();
    assert!(ui.lines().filter(|l| l.starts_with("export")).count() >= 4);
    let tree = spatialcoding::fs::loader::load_directory(&root, true).unwrap();
    fn names(node: &spatialcoding::fs::tree::TreeNode, out: &mut Vec<String>) {
        out.push(node.name().to_owned());
        if let spatialcoding::fs::tree::TreeNode::Dir { children, .. } = node {
            children.iter().for_each(|c| names(c, out));
        }
    }
    let mut all = Vec::new();
    names(&tree, &mut all);
    assert!(all.contains(&".github".to_owned()), "dot folders show when asked");
    assert!(!all.contains(&"node_modules".to_owned()) && !all.contains(&"target".to_owned()));
}

/// Needs rust-analyzer on PATH: `cargo test --test e2e showcase -- --ignored`.
#[test]
#[ignore = "needs rust-analyzer"]
fn rust_analyzer_proves_inherits_and_references_in_the_rust_crate() {
    let (state, root) = open_showcase(None, Duration::from_secs(120));
    let set = edges(&state, &root);
    use RelationshipType::{Inherits, References};
    // `trait Priced: Entity` and `impl Priced for Product` / `impl Entity for Product`.
    assert!(has(&set, Inherits, "server/src/domain/product.rs", "server/src/domain/entity.rs"));
    assert!(has(&set, Inherits, "server/src/domain/order.rs", "server/src/domain/entity.rs"));
    assert!(has(&set, References, "server/src/net/client.rs", "server/src/domain/order.rs"));
    assert!(has(&set, References, "server/src/main.rs", "server/src/net/server.rs"));
}
