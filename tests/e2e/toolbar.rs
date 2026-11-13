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
