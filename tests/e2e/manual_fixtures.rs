//! Keeps scripts/manual honest: generates each fixture with the real
//! `fixtures.sh` and checks the dependency edges that `04-edges.sh` and the
//! comments in `fixtures.sh` promise. Update all three together.

use std::path::Path;
use std::process::Command;

use spatialcoding::state::AppState;
use spatialcoding::workspace;

/// Runs `fixture_<name>` from scripts/manual/fixtures.sh into a temp dir and
/// returns the edges as sorted "source -> target" paths relative to it.
fn edges_of(name: &str) -> Vec<String> {
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
    workspace::open_all_files(&mut state);

    let rel = |id: u64| {
        let path = &state.node_store.get_file(id).unwrap().path;
        path.strip_prefix(&root).unwrap().to_string_lossy().into_owned()
    };
    let mut edges: Vec<String> = state
        .edge_store
        .all_edges()
        .map(|e| format!("{} -> {}", rel(e.source_node), rel(e.target_node)))
        .collect();
    edges.sort();
    edges
}

fn sorted(edges: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = edges.iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

#[test]
fn rust_fixture_edges() {
    assert_eq!(
        edges_of("rust"),
        sorted(&[
            "src/main.rs -> src/config.rs",
            "src/main.rs -> src/net/mod.rs",
            "src/main.rs -> src/net/client.rs",
            "src/main.rs -> src/util.rs",
            "src/config.rs -> src/util.rs",
            "src/net/mod.rs -> src/net/client.rs",
            "src/net/mod.rs -> src/net/server.rs",
            "src/net/client.rs -> src/net/server.rs",
            "src/net/client.rs -> src/config.rs",
            "src/net/server.rs -> src/util.rs",
            "src/util.rs -> src/helpers/strings.rs",
        ])
    );
}

#[test]
fn ts_fixture_edges() {
    assert_eq!(
        edges_of("ts"),
        sorted(&[
            "src/index.ts -> src/components/index.ts",
            "src/index.ts -> src/lib/math.ts",
            "src/components/index.ts -> src/components/Button.tsx",
            "src/components/Button.tsx -> src/lib/math.ts",
            "src/components/Button.tsx -> src/lib/format.ts",
            "src/legacy.js -> src/lib/math.ts",
            "src/lazy.ts -> src/lib/format.ts",
        ])
    );
}

#[test]
fn python_fixture_edges() {
    assert_eq!(
        edges_of("python"),
        sorted(&[
            "src/shop/__init__.py -> src/shop/cart.py",
            "src/shop/cart.py -> src/shop/models.py",
            "src/shop/cart.py -> src/shop/pricing.py",
            "src/shop/pricing.py -> src/shop/models.py",
            "src/shop/cli.py -> src/shop/cart.py",
        ])
    );
}

#[test]
fn go_fixture_edges() {
    assert_eq!(
        edges_of("go"),
        sorted(&[
            "main.go -> pkg/api/api.go",
            "main.go -> internal/store/store.go",
            "pkg/api/api.go -> internal/store/store.go",
        ])
    );
}
