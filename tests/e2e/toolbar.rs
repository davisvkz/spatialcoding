use egui_kittest::kittest::Queryable;
use spatialcoding::state::Tool;

use crate::common::*;

#[test]
fn select_and_move_buttons_switch_tools() {
    let mut app = app();
    assert_eq!(app.app_state().active_tool, Tool::Select);

    app.get_by_label("Move").click();
    app.run();
    assert_eq!(app.app_state().active_tool, Tool::Move);

    app.get_by_label("Select").click();
    app.run();
    assert_eq!(app.app_state().active_tool, Tool::Select);
}

#[test]
fn clear_buttons_are_disabled_until_there_is_something_to_clear() {
    let fx = fixture();
    let mut app = app_with(&fx);
    assert!(app.get_by_label("Clear All").is_disabled());
    assert!(app.get_by_label("Clear Selected").is_disabled());

    let main = app.open(&fx.path("src/main.rs"));
    assert!(!app.get_by_label("Clear All").is_disabled());
    assert!(app.get_by_label("Clear Selected").is_disabled());

    app.app_state_mut().selection.select(main);
    app.run();
    assert!(!app.get_by_label("Clear Selected").is_disabled());
}

#[test]
fn clear_all_removes_nodes_folders_edges_and_selection() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let main = app.open(&fx.path("src/main.rs"));
    app.open(&fx.path("src/util.rs"));
    app.app_state_mut().selection.select(main);
    app.run();

    app.get_by_label("Clear All").click();
    app.run();

    let s = app.app_state();
    assert!(s.node_store.file_nodes.is_empty());
    assert!(s.node_store.folder_nodes.is_empty());
    assert_eq!(s.edge_store.all_edges().count(), 0, "no dangling edges");
    assert_eq!(s.selection.count(), 0);
}

#[test]
fn clear_selected_removes_nodes_with_their_edges_and_empty_folders() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let main = app.open(&fx.path("src/main.rs"));
    let util = app.open(&fx.path("src/util.rs"));
    let app_ts = app.open(&fx.path("web/app.ts"));
    let a_ts = app.open(&fx.path("web/a.ts"));
    assert_eq!(
        app.app_state().edge_store.all_edges().count(),
        4,
        "two imports and two symbol references"
    );
    {
        let sel = &mut app.app_state_mut().selection;
        sel.select(util);
        sel.toggle(app_ts);
        sel.toggle(a_ts);
    }
    app.run();

    app.get_by_label("Clear Selected").click();
    app.run();

    let s = app.app_state();
    assert_eq!(s.node_store.all_file_ids(), [main]);
    assert_eq!(
        s.edge_store.all_edges().count(),
        0,
        "edges of removed nodes are gone"
    );
    assert_eq!(s.selection.count(), 0);
    let folders: Vec<_> = s.node_store.folder_nodes.values().collect();
    assert_eq!(folders.len(), 1, "web/ became empty and was removed");
    assert_eq!(folders[0].path, fx.path("src"));
    assert_eq!(folders[0].children_ids, [main]);
}

#[test]
fn settings_button_opens_the_settings_dialog() {
    let mut app = app();
    assert!(app.query_by_label("Minimap").is_none());

    app.get_by_label("Settings").click();
    app.run();
    assert!(app.app_state().show_settings);
    app.get_by_label("Minimap");

    app.get_by_label("Close").click();
    app.run();
    assert!(!app.app_state().show_settings);
}
