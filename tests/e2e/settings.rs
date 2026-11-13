use egui_kittest::kittest::Queryable;

use crate::common::*;

fn app_with_settings_open() -> App {
    let mut app = app();
    app.app_state_mut().show_settings = true;
    app.run();
    app
}

#[test]
fn checkboxes_change_settings() {
    let mut app = app_with_settings_open();
    let defaults = app.app_state().settings.clone();
    assert!(defaults.show_minimap && defaults.smooth_zoom);

    app.get_by_label("Minimap").click();
    app.get_by_label("Smooth Zoom").click();
    app.run();

    let s = &app.app_state().settings;
    assert!(!s.show_minimap);
    assert!(!s.smooth_zoom);
}

#[test]
fn close_button_and_escape_close_the_dialog() {
    let mut app = app_with_settings_open();
    app.get_by_label("Close").click();
    app.run();
    assert!(!app.app_state().show_settings);
    assert!(app.query_by_label("Minimap").is_none());

    app.app_state_mut().show_settings = true;
    app.run();
    app.press_key(egui::Key::Escape);
    app.run();
    assert!(!app.app_state().show_settings);
}
