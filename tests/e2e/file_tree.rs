use egui_kittest::kittest::Queryable;

use crate::common::*;

fn folder_menu(app: &mut App, label: &str) {
    let bounds = app.get_by_label(label).raw_bounds().unwrap();
    let pos = egui::pos2(
        ((bounds.x0 + bounds.x1) / 2.0) as f32,
        ((bounds.y0 + bounds.y1) / 2.0) as f32,
    );
    app.input_mut().events.push(egui::Event::PointerMoved(pos));
    for pressed in [true, false] {
        app.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        app.step();
    }
    app.run();
    app.get_by_label("Open on canvas");
    app.get_by_label("Remove from canvas");
}

#[test]
fn folder_context_menu_opens_recursively_and_removes_only_its_subtree() {
    let fx = fixture();
    std::fs::create_dir_all(fx.path("src/nested")).unwrap();
    std::fs::write(fx.path("src/nested/deep.rs"), "fn deep() {}\n").unwrap();
    let mut app = app_with(&fx);
    app.open(&fx.path("web/a.ts"));
    folder_menu(&mut app, "src (3)");
    assert!(
        app.query_by_label("main.rs").is_none(),
        "right click does not expand"
    );
    app.get_by_label("Open on canvas").click();
    app.settle_edges();
    assert!(
        app.query_by_label("Open on canvas").is_none(),
        "action closes menu"
    );
    assert_eq!(app.app_state().node_store.file_nodes.len(), 4);
    folder_menu(&mut app, "src (3)");
    app.get_by_label("Open on canvas").click();
    app.settle_edges();
    assert_eq!(
        app.app_state().node_store.file_nodes.len(),
        4,
        "no duplicates"
    );

    let id = app
        .app_state()
        .node_store
        .file_nodes
        .values()
        .find(|n| n.path == fx.path("src/main.rs"))
        .unwrap()
        .id;
    app.app_state_mut().selection.select(id);
    folder_menu(&mut app, "src (3)");
    app.get_by_label("Remove from canvas").click();
    app.settle_edges();
    let state = app.app_state();
    assert_eq!(state.node_store.file_nodes.len(), 1);
    assert_eq!(
        state.node_store.file_nodes.values().next().unwrap().path,
        fx.path("web/a.ts")
    );
    assert!(state
        .node_store
        .folder_nodes
        .values()
        .all(|n| !n.path.starts_with(fx.path("src"))));
    assert_eq!(state.selection.count(), 0);
    assert_eq!(state.edge_store.all_edges().count(), 0);
    assert!(fx.path("src/main.rs").is_file());
    assert!(fx.path("src/nested/deep.rs").is_file());
}

/// Expands `src/` by clicking its name. v2 lists the folder's children
/// directly, so the workspace root has no row of its own.
fn expand_root_and_src(app: &mut App, _fx: &Fixture) {
    app.get_by_label("src (2)").click();
    app.run();
}

#[test]
fn clicking_folder_names_expands_them() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.get_by_label("src (2)");
    assert!(
        app.query_by_label_contains("main.rs").is_none(),
        "src starts collapsed"
    );

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
    app.settle_edges();
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
    app.settle_edges();

    let s = app.app_state();
    assert_eq!(s.node_store.file_nodes.len(), 2);
    assert_eq!(s.node_store.folder_nodes.len(), 1, "both files share src/");
    let path_of = |id: u64| s.node_store.get_file(id).unwrap().path.clone();
    let edges: Vec<_> = s
        .edge_store
        .all_edges()
        .filter(|e| e.relationship == spatialcoding::edges::style::RelationshipType::References)
        .map(|e| {
            (
                path_of(e.source_node),
                path_of(e.target_node),
                e.label.clone(),
            )
        })
        .collect();
    assert_eq!(
        edges,
        [(
            fx.path("src/main.rs"),
            fx.path("src/util.rs"),
            Some("fn".into())
        )]
    );
    assert_eq!(
        s.edge_store
            .all_edges()
            .filter(|e| e.relationship == spatialcoding::edges::style::RelationshipType::Imports)
            .count(),
        1
    );
}
