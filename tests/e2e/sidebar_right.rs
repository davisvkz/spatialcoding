use egui_kittest::kittest::Queryable;
use spatialcoding::edges::style::EdgeStyle;
use spatialcoding::state::RightPanel;

use crate::common::*;

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
    assert_eq!(app.app_state().right_panel, Some(RightPanel::DependencyEdges));

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
