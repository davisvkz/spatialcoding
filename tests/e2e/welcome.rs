use egui_kittest::kittest::Queryable;

use crate::common::*;

#[test]
fn welcome_screen_shows_until_a_folder_is_opened() {
    let mut app = app();
    app.get_by_label("Open folder...");
    app.get_by_label("No folder");
    app.get_by_label("No folder loaded");

    let fx = fixture();
    app.state_mut().open_folder(fx.root.clone());
    app.run();

    assert!(app.query_by_label("Open folder...").is_none());
    assert!(app.query_by_label("No folder loaded").is_none());
    app.get_by_label(&fx.name); // toolbar folder button
    assert_eq!(app.app_state().loaded_dir.as_deref(), Some(fx.root.as_path()));
}

#[test]
fn opening_another_folder_clears_the_canvas() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));
    app.open(&fx.path("src/util.rs"));
    assert_eq!(app.app_state().edge_store.all_edges().count(), 1);

    let other = fixture();
    app.state_mut().open_folder(other.root.clone());
    app.run();

    let s = app.app_state();
    assert!(s.node_store.file_nodes.is_empty());
    assert!(s.node_store.folder_nodes.is_empty());
    assert_eq!(s.edge_store.all_edges().count(), 0);
    app.get_by_label(&other.name);
}
