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
    assert!(app.query_by_label("Fit view to nodes").is_none());

    app.press_key_modifiers(Modifiers::COMMAND, Key::Slash);
    app.run();
    app.get_by_label("Fit view to nodes");
    assert!(app.app_state().show_shortcuts);

    app.press_key(Key::Escape);
    app.run();
    assert!(app.query_by_label("Fit view to nodes").is_none());

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

fn ctrl() -> Modifiers {
    Modifiers::COMMAND
}

fn press(app: &mut App, modifiers: Modifiers, key: Key) {
    app.press_key_modifiers(modifiers, key);
    app.run();
}

#[test]
fn extra_modifiers_do_not_trigger_another_shortcut() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));

    // Ctrl+Shift+X is not Shift+X: the canvas stays.
    press(&mut app, Modifiers::COMMAND | Modifiers::SHIFT, Key::X);
    assert_eq!(app.app_state().node_store.file_nodes.len(), 1);
    // Ctrl+Alt+Shift+O opens the folders with changes, not every file.
    press(&mut app, Modifiers::COMMAND | Modifiers::ALT | Modifiers::SHIFT, Key::O);
    assert_eq!(app.app_state().node_store.file_nodes.len(), 1, "not Open all");
    // Ctrl+Shift+/ is Docs, not Shortcuts.
    press(&mut app, Modifiers::COMMAND | Modifiers::SHIFT, Key::Slash);
    assert!(app.app_state().show_docs && !app.app_state().show_shortcuts);
    // Shift+X and Ctrl+Shift+O still do their own thing.
    press(&mut app, Modifiers::COMMAND | Modifiers::SHIFT, Key::O);
    assert!(app.app_state().node_store.file_nodes.len() > 1);
    press(&mut app, Modifiers::SHIFT, Key::X);
    assert!(app.app_state().node_store.file_nodes.is_empty());
}

#[test]
fn selection_shortcuts_select_all_and_remove() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let main = app.open(&fx.path("src/main.rs"));
    let util = app.open(&fx.path("src/util.rs"));

    press(&mut app, ctrl(), Key::A);
    assert_eq!(app.selected().len(), 2);
    app.app_state_mut().selection.select(main);
    press(&mut app, Modifiers::NONE, Key::Backspace);
    let ids: Vec<u64> = app.app_state().node_store.all_file_ids();
    assert_eq!(ids, vec![util], "Backspace removed the selected node");

    app.app_state_mut().selection.select(util);
    press(&mut app, Modifiers::NONE, Key::Delete);
    assert!(app.app_state().node_store.file_nodes.is_empty(), "Delete removes too");

    let b = app.open(&fx.path("src/main.rs"));
    app.app_state_mut().selection.select(b);
    press(&mut app, ctrl(), Key::X);
    assert!(app.app_state().node_store.file_nodes.is_empty(), "Ctrl+X removes too");
}

#[test]
fn general_shortcuts_toggle_windows_panels_and_the_minimap() {
    let mut app = app();
    assert!(app.app_state().settings.show_minimap);
    press(&mut app, Modifiers::NONE, Key::M);
    assert!(!app.app_state().settings.show_minimap);
    press(&mut app, Modifiers::NONE, Key::M);
    assert!(app.app_state().settings.show_minimap);

    press(&mut app, Modifiers::SHIFT, Key::V);
    assert!(app.app_state().show_zoom_levels);
    press(&mut app, Modifiers::SHIFT, Key::V);
    assert!(!app.app_state().show_zoom_levels);

    press(&mut app, ctrl(), Key::Comma);
    assert!(app.app_state().show_settings);
    press(&mut app, Modifiers::NONE, Key::Escape);
    assert!(!app.app_state().show_settings, "Esc closes the dialog first");

    use spatialcoding::state::LeftPanel;
    press(&mut app, Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
    assert_eq!(app.app_state().left_panel, Some(LeftPanel::Search));
    press(&mut app, Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    assert_eq!(app.app_state().left_panel, Some(LeftPanel::Changes));
    press(&mut app, Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    assert_eq!(app.app_state().left_panel, Some(LeftPanel::Workspace));
}

#[test]
fn ctrl_s_hides_the_open_panels_and_brings_them_back() {
    use spatialcoding::state::{LeftPanel, RightPanel};
    let mut app = app();
    app.app_state_mut().left_panel = Some(LeftPanel::Search);
    app.app_state_mut().right_panel = Some(RightPanel::Folders);

    press(&mut app, ctrl(), Key::S);
    assert_eq!((app.app_state().left_panel, app.app_state().right_panel), (None, None));
    press(&mut app, ctrl(), Key::S);
    assert_eq!(app.app_state().left_panel, Some(LeftPanel::Search));
    assert_eq!(app.app_state().right_panel, Some(RightPanel::Folders));

    // Ctrl+. hides the rails and panels without touching which are open.
    press(&mut app, ctrl(), Key::Period);
    assert!(app.app_state().sidebars_hidden);
    assert_eq!(app.app_state().left_panel, Some(LeftPanel::Search));
    press(&mut app, ctrl(), Key::Period);
    assert!(!app.app_state().sidebars_hidden);
}

#[test]
fn zoom_shortcuts_step_the_zoom_and_stay_in_range() {
    let mut app = app();
    app.app_state_mut().viewport.zoom = 0.5;
    app.app_state_mut().viewport.target_zoom = 0.5;
    press(&mut app, ctrl(), Key::Equals);
    let zoomed_in = app.app_state().viewport.target_zoom;
    assert!(zoomed_in > 0.5, "Ctrl+= zooms in: {zoomed_in}");
    press(&mut app, ctrl(), Key::Minus);
    assert!(app.app_state().viewport.target_zoom < zoomed_in, "Ctrl+- zooms out");
    for _ in 0..40 {
        press(&mut app, ctrl(), Key::Equals);
    }
    assert_eq!(app.app_state().viewport.target_zoom, 1.0, "100% is the maximum");
}

#[test]
fn folder_shortcuts_collapse_and_expand() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));
    app.open(&fx.path("web/app.ts"));
    assert!(app.app_state().node_store.folder_nodes.values().all(|f| !f.collapsed));

    press(&mut app, Modifiers::NONE, Key::R);
    assert!(app.app_state().node_store.folder_nodes.values().any(|f| f.collapsed), "R collapses leaf folders");
    press(&mut app, Modifiers::NONE, Key::E);
    assert!(app.app_state().node_store.folder_nodes.values().all(|f| !f.collapsed), "E expands them again");

    press(&mut app, Modifiers::SHIFT, Key::R);
    assert!(app.app_state().node_store.folder_nodes.values().all(|f| f.collapsed), "Shift+R collapses all without a folder under the mouse");
    press(&mut app, Modifiers::SHIFT, Key::E);
    assert!(app.app_state().node_store.folder_nodes.values().all(|f| !f.collapsed));
}

#[test]
fn shortcuts_are_skipped_while_typing_but_chords_work() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));
    app.press_key_modifiers(ctrl(), Key::K);
    app.run();
    assert!(app.app_state().show_command_palette);
    // The palette's text field has focus: M must not toggle the minimap.
    type_text(&mut app, "m");
    assert!(app.app_state().settings.show_minimap);
    // Ctrl+K is a chord: it still closes the palette.
    press(&mut app, ctrl(), Key::K);
    assert!(!app.app_state().show_command_palette);
}
