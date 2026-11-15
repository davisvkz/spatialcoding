use egui_kittest::kittest::Queryable;
use spatialcoding::edges::style::EdgeStyle;
use spatialcoding::state::RightPanel;

use crate::common::*;

fn widget_rect(app: &App, label: &str) -> egui::Rect {
    let bounds = app.get_by_label(label).raw_bounds().expect("widget bounds");
    egui::Rect::from_min_max(
        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
    )
}

#[test]
fn right_panel_scroll_reaches_lower_controls_without_moving_header_or_canvas() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.set_size(egui::vec2(1400.0, 400.0));
    app.app_state_mut().left_panel = None;
    app.app_state_mut().right_panel = Some(RightPanel::Layout);
    app.run();
    let header = widget_rect(&app, "Close Panel");
    let before = widget_rect(&app, "AltRev");
    let offset = app.app_state().viewport.offset;
    let zoom = app.app_state().viewport.zoom;
    let pointer = egui::pos2(1200.0, 250.0);

    wheel(
        &mut app,
        pointer,
        egui::vec2(0.0, -600.0),
        egui::Modifiers::NONE,
    );

    let after = widget_rect(&app, "AltRev");
    assert!(
        after.top() < before.top(),
        "body did not scroll: {before:?} -> {after:?}"
    );
    assert!(
        after.bottom() < 400.0 && after.top() > header.bottom(),
        "lower control is reachable: {after:?}"
    );
    assert_eq!(
        widget_rect(&app, "Close Panel"),
        header,
        "header stays fixed"
    );
    assert_eq!(
        app.app_state().viewport.offset,
        offset,
        "wheel must not pan the canvas"
    );
    assert_eq!(app.app_state().viewport.zoom, zoom);
    click_at(&mut app, after.center(), egui::Modifiers::NONE);
    assert_eq!(
        app.app_state().layout_settings.sort,
        spatialcoding::layout::settings::SortMode::AltRev
    );
}

#[test]
fn right_panel_tabs_keep_independent_scroll_positions() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.set_size(egui::vec2(1400.0, 400.0));
    app.app_state_mut().left_panel = None;
    app.app_state_mut().right_panel = Some(RightPanel::DependencyEdges);
    app.run();
    let pointer = egui::pos2(1200.0, 250.0);
    let before = widget_rect(&app, "Symbol Colors");
    wheel(
        &mut app,
        pointer,
        egui::vec2(0.0, -400.0),
        egui::Modifiers::NONE,
    );
    let scrolled = widget_rect(&app, "Symbol Colors");
    assert!(scrolled.top() < before.top(), "Edges body must scroll");

    app.get_by_label("Layout").click();
    app.run();
    let layout_top = widget_rect(&app, "Apply Layout");
    assert!(
        layout_top.top() > widget_rect(&app, "Close Panel").bottom(),
        "new tab starts at its own top"
    );
    wheel(
        &mut app,
        pointer,
        egui::vec2(0.0, -400.0),
        egui::Modifiers::NONE,
    );
    app.get_by_label("Edges").click();
    app.run();
    assert_eq!(
        widget_rect(&app, "Symbol Colors"),
        scrolled,
        "Edges restores its own offset"
    );
}

#[test]
fn layout_rail_opens_layout_panel_and_apply_layout_arranges_nodes() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let util = app.open(&fx.path("src/util.rs"));
    let main = app.open(&fx.path("src/main.rs"));
    assert_eq!(app.app_state().right_panel, None);

    app.get_by_label("Layout").click();
    app.run();
    assert_eq!(app.app_state().right_panel, Some(RightPanel::Layout));

    app.get_by_label("Apply Layout").click();
    app.run();
    assert!(app.node_position(util).x > app.node_position(main).x);
}

#[test]
fn edges_panel_changes_edge_style() {
    let mut app = app();
    app.app_state_mut().right_panel = Some(RightPanel::Layout);
    app.run();

    app.get_by_label("Edges").click();
    app.run();
    assert_eq!(
        app.app_state().right_panel,
        Some(RightPanel::DependencyEdges)
    );

    app.get_by_label("Step").click();
    app.run();
    assert_eq!(app.app_state().edge_style.style, EdgeStyle::Step);

    // On by default (v2: arrows on the selection's edges); the click turns them off.
    assert!(app.app_state().edge_style.show_arrow_heads);
    app.get_by_label("Arrow Heads").click();
    app.run();
    assert!(!app.app_state().edge_style.show_arrow_heads);

    assert!(app.app_state().edge_style.show_symbol_pills);
    app.get_by_label("Symbol Pills").click();
    app.run();
    assert!(!app.app_state().edge_style.show_symbol_pills);
}

#[test]
fn clicking_the_active_tab_collapses_the_panel() {
    let mut app = app();
    app.app_state_mut().right_panel = Some(RightPanel::DependencyEdges);
    app.run();

    app.get_by_label("Edges").click();
    app.run();
    assert_eq!(app.app_state().right_panel, None);
    // The rail is always visible now; Layout is the first rail entry.
    app.get_by_label("Layout");
}
