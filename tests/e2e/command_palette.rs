use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;

use crate::common::*;

fn open_palette(app: &mut App) {
    app.press_key_modifiers(Modifiers::COMMAND, Key::K);
    app.run();
    assert!(app.app_state().show_command_palette);
}

#[test]
fn ctrl_k_opens_the_palette_with_commands_and_escape_closes_it() {
    let mut app = app();
    open_palette(&mut app);
    app.get_by_label("Show shortcuts");
    app.get_by_label("Toggle CST panel");

    app.press_key(Key::Escape);
    app.run();
    assert!(!app.app_state().show_command_palette);
    assert!(app.query_by_label("Show shortcuts").is_none());
}

#[test]
fn typing_filters_the_commands() {
    let mut app = app();
    open_palette(&mut app);
    type_text(&mut app, "cst");
    app.get_by_label("Toggle CST panel");
    assert!(app.query_by_label("Show shortcuts").is_none());
}

#[test]
fn enter_runs_the_selected_command() {
    let mut app = app();
    open_palette(&mut app);
    type_text(&mut app, "toggle cst");
    app.press_key(Key::Enter);
    app.run();
    assert!(!app.app_state().show_command_palette);
    assert_eq!(app.app_state().right_panel, Some(spatialcoding::state::RightPanel::Cst));
}

#[test]
fn arrow_keys_move_the_selection() {
    let mut app = app();
    open_palette(&mut app);
    type_text(&mut app, "toggle");
    app.press_key(Key::ArrowDown);
    app.run();
    assert_eq!(app.app_state().palette.selected, 1);
    app.press_key(Key::ArrowUp);
    app.run();
    assert_eq!(app.app_state().palette.selected, 0);
}

#[test]
fn files_of_the_folder_can_be_opened_from_the_palette() {
    let fx = fixture();
    let mut app = app_with(&fx);
    open_palette(&mut app);
    type_text(&mut app, "util.rs");
    app.get_by_label("src/util.rs");
    app.press_key(Key::Enter);
    app.run();
    let state = app.app_state();
    assert!(state.node_store.file_nodes.values().any(|n| n.path == fx.path("src/util.rs")));
    assert_eq!(state.selection.count(), 1);
}

#[test]
fn typing_in_the_palette_does_not_trigger_canvas_shortcuts() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));
    app.open(&fx.path("src/util.rs"));
    open_palette(&mut app);
    // "x" with Shift would clear the canvas, "f" would focus on the mouse.
    app.press_key_modifiers(Modifiers::SHIFT, Key::X);
    app.run();
    assert_eq!(app.app_state().node_store.file_nodes.len(), 2);
    assert!(app.app_state().show_command_palette);
}
