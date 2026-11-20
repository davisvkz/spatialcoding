//! The canvas and the file tree follow the loaded folder on disk.

use egui_kittest::kittest::Queryable;

use crate::common::*;

/// Steps frames until `done` holds, while the watcher's events arrive.
fn wait_for(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !done(app) {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.step();
    }
}

fn tree_has(app: &App, path: &std::path::Path) -> bool {
    app.state()
        .file_tree()
        .is_some_and(|t| t.find_node(path).is_some())
}

#[test]
fn an_open_file_reloads_when_it_changes_on_disk() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let id = app.open(&fx.path("src/util.rs"));
    assert_eq!(
        app.app_state().node_store.get_file(id).unwrap().line_count,
        1
    );

    std::fs::write(
        fx.path("src/util.rs"),
        "pub fn hello() {}\npub fn bye() {}\n// more\n",
    )
    .unwrap();

    wait_for(&mut app, "the node to reload", |a| {
        a.app_state().node_store.get_file(id).unwrap().line_count == 3
    });
    let node = app.app_state().node_store.get_file(id).unwrap();
    assert!(node.content.contains("pub fn bye()"));
    // The node keeps its identity and place.
    assert_eq!(app.app_state().node_store.file_nodes.len(), 1);
}

#[test]
fn dependency_endpoints_follow_an_edit_and_disappear_when_the_file_closes() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let target_path = fx.path("src/util.rs");
    let target = app.open(&target_path);
    let source = app.open(&fx.path("src/main.rs"));
    let definition_line = |app: &App| {
        app.app_state()
            .edge_store
            .all_edges()
            .filter(|edge| edge.source_node == source && edge.target_node == target)
            .find_map(|edge| edge.name_span.map(|span| span.line))
    };
    assert_eq!(definition_line(&app), Some(0));

    std::fs::write(&target_path, "// moved down\npub fn hello() {}\n").unwrap();
    wait_for(
        &mut app,
        "the dependency to move to the new definition",
        |app| definition_line(app) == Some(1),
    );

    std::fs::remove_file(&target_path).unwrap();
    wait_for(&mut app, "the deleted node to close", |app| {
        app.app_state().node_store.get_file(target).is_none()
    });
    app.settle_edges();
    assert!(app
        .app_state()
        .edge_store
        .all_edges()
        .all(|edge| edge.target_node != target));
}

#[test]
fn a_file_written_through_a_temp_file_and_renamed_still_reloads() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let id = app.open(&fx.path("src/util.rs"));

    let temp = fx.path("src/util.rs.tmp");
    std::fs::write(&temp, "pub fn replaced() {}\n").unwrap();
    std::fs::rename(&temp, fx.path("src/util.rs")).unwrap();

    wait_for(&mut app, "the node to reload", |a| {
        a.app_state()
            .node_store
            .get_file(id)
            .unwrap()
            .content
            .contains("replaced")
    });
}

#[test]
fn a_new_file_appears_in_the_file_tree() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let fresh = fx.path("src/fresh.rs");
    assert!(!tree_has(&app, &fresh));

    std::fs::write(&fresh, "fn fresh() {}\n").unwrap();

    wait_for(&mut app, "the file in the tree", |a| tree_has(a, &fresh));
}

#[test]
fn a_deleted_file_leaves_the_tree_and_closes_its_node() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/util.rs"));
    app.open(&fx.path("src/main.rs"));
    assert_eq!(app.app_state().node_store.file_nodes.len(), 2);

    std::fs::remove_file(fx.path("src/util.rs")).unwrap();

    wait_for(&mut app, "the file to leave the tree", |a| {
        !tree_has(a, &fx.path("src/util.rs"))
    });
    wait_for(&mut app, "the node to close", |a| {
        a.app_state().node_store.file_nodes.len() == 1
    });
    let remaining = app
        .app_state()
        .node_store
        .file_nodes
        .values()
        .next()
        .unwrap();
    assert_eq!(remaining.path, fx.path("src/main.rs"));
}

#[test]
fn deleting_a_folder_closes_the_nodes_inside_it() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("web/app.ts"));
    app.open(&fx.path("web/a.ts"));
    app.open(&fx.path("src/main.rs"));

    std::fs::remove_dir_all(fx.path("web")).unwrap();

    wait_for(&mut app, "the web nodes to close", |a| {
        a.app_state().node_store.file_nodes.len() == 1
    });
    assert!(!tree_has(&app, &fx.path("web")));
}

#[test]
fn the_tree_keeps_its_open_folders_when_it_reloads() {
    let fx = fixture();
    let mut app = app_with(&fx);
    // Expand `src` in the tree by clicking its row in the Workspace panel.
    app.get_by_label("src (2)").click();
    app.run();
    app.get_by_label("util.rs");

    std::fs::write(fx.path("src/extra.rs"), "fn x() {}\n").unwrap();
    wait_for(&mut app, "the new file in the tree", |a| {
        tree_has(a, &fx.path("src/extra.rs"))
    });
    app.run();
    app.get_by_label("extra.rs");
    app.get_by_label("util.rs");
}

#[test]
fn changes_in_ignored_folders_do_nothing() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let id = app.open(&fx.path("src/util.rs"));
    std::fs::write(fx.path("target/debug/more.txt"), "x").unwrap();
    // A visible change afterwards proves the earlier events were processed.
    std::fs::write(fx.path("src/util.rs"), "a\nb\n").unwrap();
    wait_for(&mut app, "the node to reload", |a| {
        a.app_state().node_store.get_file(id).unwrap().line_count == 2
    });
    assert!(!tree_has(&app, &fx.path("target/debug/more.txt")));
}
