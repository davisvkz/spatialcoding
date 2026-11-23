//! Layout panel: Auto Layout from the kebab, the Advanced Settings controls
//! and Reset Config.

use egui_kittest::kittest::Queryable;
use spatialcoding::layout::settings::{HAlign, LayoutSettings, SortMode, VAlign};
use spatialcoding::state::RightPanel;

use crate::common::*;

#[test]
fn auto_layout_places_importers_left_of_their_dependencies() {
    let fx = fixture();
    let mut app = app_with(&fx);
    assert!(app.app_state().layout_settings.auto_layout, "on by default, as in v2");
    // Grid placement would put the second file to the right of the first.
    let util = app.open(&fx.path("src/util.rs"));
    let main = app.open(&fx.path("src/main.rs"));
    assert!(!app.app_state().layout_dirty, "laid out once the edges settled");
    assert!(
        app.node_position(main).x < app.node_position(util).x,
        "main.rs uses util.rs, so it sits to its left"
    );
}

#[test]
fn disabling_auto_layout_from_the_kebab_keeps_new_files_where_they_open() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.app_state_mut().left_panel = None;
    app.app_state_mut().right_panel = Some(RightPanel::Layout);
    app.run();
    app.get_by_label("Layout menu").click();
    app.run();
    app.get_by_label("Disable Auto Layout").click();
    app.run();
    assert!(!app.app_state().layout_settings.auto_layout);
    app.get_by_label_contains("Auto Layout is off");

    let util = app.open(&fx.path("src/util.rs"));
    let main = app.open(&fx.path("src/main.rs"));
    assert!(
        app.node_position(main).x > app.node_position(util).x,
        "without auto layout the grid order stays"
    );

    app.get_by_label("Layout menu").click();
    app.run();
    app.get_by_label("Enable Auto Layout").click();
    app.run();
    assert!(app.app_state().layout_settings.auto_layout);
}

#[test]
fn auto_layout_keeps_the_selected_file_still_on_screen() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let util = app.open(&fx.path("src/util.rs"));
    app.app_state_mut().selection.select(util);
    let center = app.node_center(util);
    let before = app.app_state().viewport.world_to_screen(center);

    app.open(&fx.path("src/main.rs"));

    let after = app.app_state().viewport.world_to_screen(app.node_center(util));
    assert_close(after, before);
}

#[test]
fn advanced_settings_controls_edit_the_layout_settings_and_reset_config_restores_them() {
    let mut app = app();
    app.app_state_mut().left_panel = None;
    app.app_state_mut().right_panel = Some(RightPanel::Layout);
    app.run();
    // Segments with labels shared by H and V Align ("Center") are distinct
    // widgets; click the unique ones.
    app.get_by_label("Left").click();
    app.run();
    app.get_by_label("Fill").click();
    app.run();
    app.get_by_label("Alt Rev").click();
    app.run();
    app.get_by_label("Break");
    {
        let s = &app.app_state().layout_settings;
        assert_eq!(s.h_align, HAlign::Left);
        assert_eq!(s.v_align, VAlign::Fill);
        assert_eq!(s.sort, SortMode::AltRev);
    }
    app.app_state_mut().layout_settings.auto_layout = false;
    app.app_state_mut().layout_settings.file_spacing = 42.0;

    app.run();
    app.get_by_label("Reset Config").click();
    app.run();

    let s = &app.app_state().layout_settings;
    let defaults = LayoutSettings::default();
    assert_eq!((s.h_align, s.v_align, s.sort), (defaults.h_align, defaults.v_align, defaults.sort));
    assert_eq!(s.file_spacing, 500.0);
    assert_eq!((s.folder_spacing, s.folder_padding, s.rank_breaking), (500.0, 80.0, 6));
    assert!(!s.auto_layout, "Reset Config keeps the Auto Layout toggle");
}

fn layout_panel_app(fx: &Fixture) -> App {
    let mut app = app_with(fx);
    app.app_state_mut().left_panel = None;
    app.app_state_mut().right_panel = Some(RightPanel::Layout);
    app.run();
    app
}

#[test]
fn the_barycenter_switch_is_off_by_default_and_reset_config_turns_it_off() {
    let fx = fixture();
    let mut app = layout_panel_app(&fx);
    assert!(!app.app_state().layout_settings.barycenter, "off by default");

    app.get_by_label("Barycenter").click();
    app.run();
    assert!(app.app_state().layout_settings.barycenter);

    app.get_by_label("Reset Config").click();
    app.run();
    assert!(!app.app_state().layout_settings.barycenter);
}

#[test]
fn apply_layout_uses_the_canvas_node_size_and_keeps_the_selected_file_still() {
    let fx = fixture();
    let mut app = layout_panel_app(&fx);
    app.app_state_mut().layout_settings.auto_layout = false;
    app.app_state_mut().settings.node_width = 900.0;
    let util = app.open(&fx.path("src/util.rs"));
    let main = app.open(&fx.path("src/main.rs"));
    app.app_state_mut().selection.select(util);
    let before = app.app_state().viewport.world_to_screen(app.node_center(util));

    app.get_by_label("Apply Layout").click();
    app.run();

    let s = app.app_state();
    assert_eq!(
        app.node_position(util).x - app.node_position(main).x,
        900.0 + s.layout_settings.file_spacing,
        "the columns are as wide as the nodes on the canvas, not the default node width"
    );
    assert_close(s.viewport.world_to_screen(app.node_center(util)), before);
}
