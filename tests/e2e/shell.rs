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

fn settle(app: &mut App) {
    // The app steps 0.1 s per frame here: a few frames let a change rest. Under
    // load background analysis can still move nodes (an auto layout), so wait
    // until the history has nothing left to settle.
    for round in 0..400 {
        app.step();
        if round >= 8 && !app.app_state().history.is_settling() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn undo_and_redo_restore_files_and_positions_and_never_fire_while_typing() {
    let fx = fixture();
    let mut app = app_with_stepping(&fx, 0.1);
    settle(&mut app);
    let main = app.open(&fx.path("src/main.rs"));
    settle(&mut app);
    let util = app.open(&fx.path("src/util.rs"));
    settle(&mut app);
    assert_eq!(app.app_state().node_store.file_nodes.len(), 2);

    // Under load a background analysis can still nudge a node (an auto
    // layout) as a step of its own; undo that first, then the opening.
    for _ in 0..4 {
        app.press_key_modifiers(Modifiers::COMMAND, Key::Z);
        settle(&mut app);
        if app.app_state().node_store.file_nodes.len() == 1 {
            break;
        }
    }
    let paths: Vec<_> = app.app_state().node_store.file_nodes.values().map(|n| n.path.clone()).collect();
    assert_eq!(paths, vec![fx.path("src/main.rs")], "undo closes the last opened file");

    for _ in 0..4 {
        app.press_key_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        settle(&mut app);
        if app.app_state().node_store.file_nodes.len() == 2 {
            break;
        }
    }
    assert_eq!(app.app_state().node_store.file_nodes.len(), 2, "redo opens it again");

    // Moving a node is one step too.
    let moved = app.app_state().node_store.file_nodes.values().find(|n| n.path == fx.path("src/main.rs")).unwrap().id;
    let before = app.node_position(moved);
    app.app_state_mut().node_store.get_file_mut(moved).unwrap().position += egui::vec2(300.0, 200.0);
    settle(&mut app);
    app.press_key_modifiers(Modifiers::COMMAND, Key::Z);
    settle(&mut app);
    let again = app.app_state().node_store.file_nodes.values().find(|n| n.path == fx.path("src/main.rs")).unwrap().id;
    assert_eq!(app.node_position(again), before, "undo puts the node back");
    let _ = (main, util);

    // Typing in a text field keeps its own Ctrl+Z: the canvas does not undo.
    app.press_key_modifiers(Modifiers::COMMAND, Key::K);
    settle(&mut app);
    let count = app.app_state().node_store.file_nodes.len();
    app.press_key_modifiers(Modifiers::COMMAND, Key::Z);
    settle(&mut app);
    assert_eq!(app.app_state().node_store.file_nodes.len(), count);
}

#[test]
fn go_to_ui_pulses_the_exact_control_when_it_is_drawn() {
    use spatialcoding::ui::command_palette::PaletteCommand;
    let fx = fixture();
    let mut app = app_with(&fx);
    app.app_state_mut().left_panel = None;
    // The Changes layout control: the panel opens, the control's own rectangle is recorded.
    app.execute_palette_command(PaletteCommand::GoToUi("diff:cycleViewMode"));
    app.step();
    app.step();
    let s = app.app_state();
    assert_eq!(s.left_panel, Some(spatialcoding::state::LeftPanel::Changes));
    let control = s.anchor_rects.get("changes:layout").copied().expect("the layout switch registered its rectangle");
    let panel = s.panel_rect_left.expect("the panel is open");
    assert!(panel.contains_rect(control) && control.height() < panel.height() / 2.0, "a control, not the whole panel");
    assert_eq!(s.ui_highlight.map(|(id, _)| id), Some("changes:layout"));

    // Settings points at the rail's own Settings button.
    app.execute_palette_command(PaletteCommand::GoToUi("ui:toggleSettings"));
    app.step();
    app.step();
    assert!(app.app_state().anchor_rects.contains_key("settings"));
}

#[test]
fn the_minimap_corner_resizes_it() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let main = app.open(&fx.path("src/main.rs"));
    let center = app.node_center(main);
    app.show_at(center, CANVAS_SPOT);
    app.app_state_mut().right_panel = None;
    app.run();
    let canvas = app.app_state().canvas_rect;
    // The card is 200x150 in the bottom-right corner; grab its top-left corner.
    let corner = Pos2::new(canvas.max.x - 200.0 - 6.0 + 4.0, canvas.max.y - 150.0 - 6.0 + 4.0);
    let view = app.app_state().viewport.visible_world_rect(canvas).center();
    drag(&mut app, corner, corner - Vec2::new(60.0, 40.0));
    assert_eq!(app.app_state().viewport.visible_world_rect(canvas).center(), view, "the drag resized, it did not pan");
    // Dragging up and left grew it: the next corner is further out.
    let bigger = Pos2::new(canvas.max.x - 260.0 - 6.0 + 4.0, canvas.max.y - 190.0 - 6.0 + 4.0);
    drag(&mut app, bigger, bigger - Vec2::new(10.0, 10.0));
    assert_eq!(app.app_state().viewport.visible_world_rect(canvas).center(), view, "the card is now 260x190 or more");
}

#[test]
fn the_feedback_dialog_builds_a_redacted_report() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.get_by_label("Send feedback").click();
    app.run();
    app.get_by_label("What happened, or what would you like to see?");
    assert!(app.get_by_label("Copy report").is_disabled(), "nothing to send yet");

    app.get_by_label("Include a diagnostic log").click();
    app.run();
    app.get_by_label_contains("Paths and your user name are redacted");
    let draft = app.app_state().feedback.clone().unwrap();
    assert!(draft.include_log);
    let report = spatialcoding::ui::feedback::report(app.app_state(), &draft);
    let root = fx.root.display().to_string();
    assert!(report.contains("SpatialCoding "), "{report}");
    assert!(!report.contains(&root), "the repository path is redacted: {report}");
    assert!(report.contains("<repo>"));

    app.get_by_label("Close").click();
    app.run();
    assert!(app.app_state().feedback.is_none());
}
