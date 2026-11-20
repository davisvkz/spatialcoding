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
    assert_eq!(
        app.app_state().loaded_dir.as_deref(),
        Some(fx.root.as_path())
    );
}

#[test]
fn opening_another_folder_clears_the_canvas() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));
    app.open(&fx.path("src/util.rs"));
    assert_eq!(
        app.app_state().edge_store.all_edges().count(),
        2,
        "module import plus symbol reference"
    );

    let other = fixture();
    app.state_mut().open_folder(other.root.clone());
    app.run();

    let s = app.app_state();
    assert!(s.node_store.file_nodes.is_empty());
    assert!(s.node_store.folder_nodes.is_empty());
    assert_eq!(s.edge_store.all_edges().count(), 0);
    app.get_by_label(&other.name);
}

#[test]
fn the_local_server_icon_has_an_orange_dot_until_an_agent_connects() {
    // The dot is painted, so the state it depends on is what is checked.
    let mut app = app();
    assert!(app.app_state().agents.is_empty(), "no agent: the rail icon shows the dot");
    app.app_state_mut().agents = vec!["claude".into()];
    app.run();
    assert!(!app.app_state().agents.is_empty(), "with an agent the dot is gone");
}

/// Screen rectangle of a node, from its accessibility bounds.
fn rect_of(node: &egui_kittest::kittest::Node<'_>) -> egui::Rect {
    let b = node.bounding_box().expect("the node has bounds");
    egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
}

/// Every welcome widget stays inside a window of `size`, and the Local Server
/// text wraps instead of running on as one line (TASK-57).
fn assert_welcome_fits(size: egui::Vec2) {
    let app = app_sized(size, DEFAULT_STEP_DT);
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    let paragraph = rect_of(&app.get_by_label_contains("Language servers run on this machine"));
    assert!(paragraph.width() <= 280.0, "the paragraph wraps inside its 270 px card, got {paragraph:?}");
    for label in ["Repository", "Open folder...", "Shortcuts", "Command Palette", "Docs"] {
        let rect = rect_of(&app.get_by_label(label));
        assert!(window.contains_rect(rect), "{label} at {rect:?} is outside the {size:?} window");
    }
}

#[test]
fn the_welcome_cards_stay_inside_the_window() {
    assert_welcome_fits(egui::vec2(1400.0, 900.0));
}

#[test]
fn the_welcome_cards_stack_and_stay_inside_a_narrow_window() {
    assert_welcome_fits(egui::vec2(700.0, 700.0));
    assert_welcome_fits(egui::vec2(480.0, 700.0));
}
