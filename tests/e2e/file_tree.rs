use egui_kittest::kittest::Queryable;

use crate::common::*;

/// Expands the workspace root and `src/` by clicking their names.
fn expand_root_and_src(app: &mut App, fx: &Fixture) {
    app.get_by_label(&format!("{} (7)", fx.name)).click();
    app.run();
    app.get_by_label("src (2)").click();
    app.run();
}

#[test]
fn clicking_folder_names_expands_them() {
    let fx = fixture();
    let mut app = app_with(&fx);
    assert!(app.query_by_label("src (2)").is_none(), "root starts collapsed");

    expand_root_and_src(&mut app, &fx);

    app.get_by_label("web (2)");
    app.get_by_label("assets (1)");
    app.get_by_label_contains("README.md");
    app.get_by_label_contains("main.rs");
    app.get_by_label_contains("util.rs");
}

#[test]
fn hidden_and_build_folders_are_not_listed() {
    let fx = fixture();
    let mut app = app_with(&fx);
    expand_root_and_src(&mut app, &fx);

    assert!(app.query_by_label_contains(".hidden").is_none());
    assert!(app.query_by_label_contains("target").is_none());
}

#[test]
fn clicking_files_opens_nodes_folders_and_edges() {
    let fx = fixture();
    let mut app = app_with(&fx);
    expand_root_and_src(&mut app, &fx);

    app.get_by_label_contains("main.rs").click();
    app.run();
    {
        let s = app.app_state();
        assert_eq!(s.node_store.file_nodes.len(), 1);
        assert_eq!(s.node_store.folder_nodes.len(), 1);
        assert_eq!(s.edge_store.all_edges().count(), 0);
        let node = s.node_store.file_nodes.values().next().unwrap();
        assert_eq!(node.path, fx.path("src/main.rs"));
        assert_eq!(node.language, "Rust");
        assert!(node.content.contains("mod util;"));
    }

    app.get_by_label_contains("util.rs").click();
    app.run();

    let s = app.app_state();
    assert_eq!(s.node_store.file_nodes.len(), 2);
    assert_eq!(s.node_store.folder_nodes.len(), 1, "both files share src/");
    let path_of = |id: u64| s.node_store.get_file(id).unwrap().path.clone();
    let edges: Vec<_> = s
        .edge_store
        .all_edges()
        .map(|e| (path_of(e.source_node), path_of(e.target_node), e.label.clone()))
        .collect();
    assert_eq!(
        edges,
        [(fx.path("src/main.rs"), fx.path("src/util.rs"), Some("Module".into()))]
    );
}
