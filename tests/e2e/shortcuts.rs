use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;

use crate::common::*;

#[test]
fn key_1_applies_the_hierarchical_layout() {
    let fx = fixture();
    let mut app = app_with(&fx);
    // Open the dependency first so the grid would put it to the left.
    let util = app.open(&fx.path("src/util.rs"));
    let main = app.open(&fx.path("src/main.rs"));

    app.press_key(Key::Num1);
    app.run();

    let s = app.app_state();
    let column = s.settings.node_width + s.layout_settings.file_spacing;
    let (main_pos, util_pos) = (app.node_position(main), app.node_position(util));
    // Both sit in the `src/` folder box, inset by its padding.
    assert_eq!(
        main_pos.x, s.layout_settings.folder_padding,
        "importer is a root"
    );
    assert_eq!(
        util_pos.x - main_pos.x,
        column,
        "dependency is one column to the right"
    );
}

#[test]
fn shift_x_clears_nodes_edges_and_selection() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let main = app.open(&fx.path("src/main.rs"));
    app.open(&fx.path("src/util.rs"));
    app.app_state_mut().selection.select(main);

    app.press_key_modifiers(Modifiers::SHIFT, Key::X);
    app.run();

    let s = app.app_state();
    assert!(s.node_store.file_nodes.is_empty());
    assert!(s.node_store.folder_nodes.is_empty());
    assert_eq!(s.edge_store.all_edges().count(), 0);
    assert_eq!(s.selection.count(), 0);
}

#[test]
fn ctrl_slash_opens_shortcuts_and_escape_closes_them() {
    let mut app = app();
    assert!(app.query_by_label("Fit to Screen").is_none());

    app.press_key_modifiers(Modifiers::COMMAND, Key::Slash);
    app.run();
    app.get_by_label("Fit to Screen");
    assert!(app.app_state().show_shortcuts);

    app.press_key(Key::Escape);
    app.run();
    assert!(app.query_by_label("Fit to Screen").is_none());

    app.press_key_modifiers(Modifiers::COMMAND, Key::Slash);
    app.run();
    app.get_by_label("Close").click();
    app.run();
    assert!(!app.app_state().show_shortcuts);
}

#[test]
fn ctrl_shift_o_opens_all_text_files() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));

    app.press_key_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::O);
    app.settle_edges();

    let s = app.app_state();
    let mut names: Vec<&str> = s
        .node_store
        .file_nodes
        .values()
        .map(|n| n.file_name())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "Cargo.toml",
            "README.md",
            "a.ts",
            "app.ts",
            "main.rs",
            "util.rs"
        ],
        "binary, hidden and build files are skipped; main.rs is not duplicated"
    );
    assert_eq!(
        s.edge_store.all_edges().count(),
        4,
        "main->util and app->a each have import and reference edges"
    );
}
