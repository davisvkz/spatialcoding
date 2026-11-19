//! App shell: command bus, Go to UI, minimap, tools, welcome cards, Settings.

use egui::{Key, Modifiers, Pos2, Vec2};
use egui_kittest::kittest::Queryable;
use spatialcoding::state::{RightPanel, Tool};

use crate::common::*;

#[test]
fn every_command_runs_through_the_registry_by_id() {
    use spatialcoding::commands;
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));
    let s = app.state_mut().state_mut();
    commands::run(s, "ui:toggleCstPanel", &serde_json::Value::Null).unwrap();
    assert_eq!(s.right_panel, Some(RightPanel::Cst));
    commands::run(s, "ui:toggleCstPanel", &serde_json::Value::Null).unwrap();
    assert_eq!(s.right_panel, None);
    commands::run(s, "viewport:zoomBy", &serde_json::json!({ "factor": 0.5 })).unwrap();
    assert!(s.viewport.target_zoom < s.viewport.zoom + 1e-3 || s.viewport.zoom_center.is_some());
    assert!(commands::run(s, "no:such", &serde_json::Value::Null).is_err());
}

#[test]
fn palette_go_to_ui_opens_the_panel_without_running_the_command() {
    use spatialcoding::ui::command_palette::PaletteCommand;
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));
    app.app_state_mut().right_panel = None;

    app.execute_palette_command(PaletteCommand::GoToUi("folder:collapseAll"));
    app.step(); // the pulse keeps repainting, so `run()` would never settle
    let s = app.app_state();
    assert_eq!(s.right_panel, Some(RightPanel::Folders), "the Folders panel is shown");
    assert!(s.ui_highlight.is_some(), "and its control pulses");
    assert!(s.node_store.folder_nodes.values().all(|f| !f.collapsed), "nothing was collapsed");
}

#[test]
fn the_palette_lists_every_visible_command_with_its_shortcut() {
    let mut app = app();
    app.press_key_modifiers(Modifiers::COMMAND, Key::K);
    app.run();
    type_text(&mut app, "docs");
    app.get_by_label("Show docs");
    assert!(app.query_by_label("Show shortcuts").is_none());
}

#[test]
fn clicking_the_minimap_centers_the_view_on_that_spot() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let main = app.open(&fx.path("src/main.rs"));
    let web = app.open(&fx.path("web/app.ts"));
    app.app_state_mut().right_panel = None;
    app.app_state_mut().viewport.zoom = 0.3;
    app.app_state_mut().viewport.target_zoom = 0.3;
    let center = app.node_center(main);
    app.show_at(center, CANVAS_SPOT);

    let canvas = app.app_state().canvas_rect;
    // The minimap sits in the bottom-right corner of the canvas.
    let spot = Pos2::new(canvas.max.x - 100.0, canvas.max.y - 75.0);
    let before = app.app_state().viewport.visible_world_rect(canvas).center();
    click_at(&mut app, spot, Modifiers::NONE);
    let after = app.app_state().viewport.visible_world_rect(canvas).center();
    assert_ne!(before, after, "the view moved");
    let _ = web;
}

#[test]
fn the_move_tool_pans_with_the_left_button_and_leaves_nodes_alone() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let main = app.open(&fx.path("src/main.rs"));
    let center = app.node_center(main);
    app.show_at(center, CANVAS_SPOT);
    app.app_state_mut().active_tool = Tool::Move;
    let (offset, node) = (app.app_state().viewport.offset, app.node_position(main));

    drag(&mut app, CANVAS_SPOT, CANVAS_SPOT + Vec2::new(80.0, 40.0));
    assert_ne!(app.app_state().viewport.offset, offset, "view panned");
    assert_eq!(app.node_position(main), node, "the node did not move");
}

#[test]
fn the_welcome_screen_offers_recent_folders_and_the_docs_buttons() {
    let fx = fixture();
    let mut app = app();
    app.app_state_mut().settings.recent_folders = vec![fx.root.clone()];
    app.run();
    assert!(app.query_all_by_label("Local Server").count() >= 2, "rail icon and card");
    app.get_by_label("Repository");
    app.get_by_label(&fx.name); // the recent folder link
    app.get_by_label("Command Palette").click();
    app.run();
    assert!(app.app_state().show_command_palette);
    app.press_key(Key::Escape);
    app.run();
    app.get_by_label("Shortcuts").click();
    app.run();
    assert!(app.app_state().show_shortcuts);
}

#[test]
fn opening_a_folder_remembers_it_first_in_the_recent_list() {
    let (a, b) = (fixture(), fixture());
    let mut app = app();
    app.state_mut().open_folder(a.root.clone());
    app.state_mut().open_folder(b.root.clone());
    app.run();
    let recents = &app.app_state().settings.recent_folders;
    assert_eq!(recents.first(), Some(&b.root));
    assert_eq!(recents.get(1), Some(&a.root));
}

#[test]
fn settings_groups_reset_and_the_controls_bar_hides_the_tools() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));
    app.app_state_mut().show_settings = true;
    app.app_state_mut().settings.node_width = 1500.0;
    app.app_state_mut().settings.show_controls_bar = false;
    app.run();
    assert!(app.query_by_label("Select tool").is_none(), "Controls Bar off hides the tools");

    app.get_by_label("Reset all").click();
    app.run();
    assert_eq!(app.app_state().settings.node_width, 700.0);
    assert!(app.app_state().settings.show_controls_bar);
    app.get_by_label("Select tool");
}

#[test]
fn zoom_levels_and_docs_windows_draw_their_content() {
    let mut app = app();
    app.press_key_modifiers(Modifiers::SHIFT, Key::V);
    app.run();
    app.get_by_label_contains("Zoom Levels");
    app.press_key_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Slash);
    app.run();
    app.get_by_label("Move around");
    assert!(app.app_state().show_docs);
}

#[test]
fn the_workspace_menu_clears_the_repository_and_toggles_counts() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));
    assert!(app.app_state().settings.show_folder_counts);
    app.get_by_label("Workspace menu").click();
    app.run();
    app.get_by_label("Hide folder file counts").click();
    app.run();
    assert!(!app.app_state().settings.show_folder_counts);

    app.get_by_label("Workspace menu").click();
    app.run();
    app.get_by_label("Clear repository").click();
    app.run();
    assert!(app.app_state().loaded_dir.is_none());
    assert!(app.app_state().node_store.file_nodes.is_empty());
}

#[test]
fn expand_all_folders_opens_every_tree_row_and_the_hint_is_shown() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.get_by_label("Drag a folder onto the canvas");
    app.get_by_label("Expand all folders").click();
    app.run();
    // A nested file is listed without clicking its folders open.
    app.get_by_label("util.rs");
}

#[test]
fn dropping_a_tree_row_on_the_canvas_opens_it() {
    use spatialcoding::ui::sidebar_left::TreeDrag;
    let fx = fixture();
    let mut app = app_with(&fx);
    app.app_state_mut().right_panel = None;
    app.run();
    let path = fx.path("src/util.rs");
    egui::DragAndDrop::set_payload(&app.ctx, TreeDrag(path.clone()));
    // Release over the canvas.
    app.input_mut().events.push(egui::Event::PointerMoved(CANVAS_SPOT));
    app.step();
    click_at(&mut app, CANVAS_SPOT, Modifiers::NONE);
    app.run();
    assert!(app.app_state().node_store.file_nodes.values().any(|n| n.path == path), "the dropped file is open");
}
