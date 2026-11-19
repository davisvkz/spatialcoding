//! Dependency Edges panel: v2 defaults, the Gradient legend switch, arrow
//! Min Size, the Barrel and Re-export rows and the kebab reset.

use egui_kittest::kittest::Queryable;
use spatialcoding::edges::style::EdgeStyleConfig;
use spatialcoding::state::RightPanel;

use crate::common::*;

fn edges_panel() -> App {
    let mut app = app();
    app.set_size(egui::vec2(1400.0, 2400.0));
    app.app_state_mut().left_panel = None;
    app.app_state_mut().right_panel = Some(RightPanel::DependencyEdges);
    app.run();
    app
}

#[test]
fn defaults_follow_the_reference() {
    let cfg = EdgeStyleConfig::default();
    assert!(cfg.show_connection_markers && cfg.show_arrow_heads && cfg.only_selected);
    assert!(cfg.scale_arrow_heads && cfg.use_gradient && cfg.show_barrel_edges);
    assert!(!cfg.show_reexport_edges && !cfg.highlight_reexport && !cfg.highlight_barrel);
    assert_eq!((cfg.min_size_pct, cfg.gradient_gap_pct), (0.5, 0.8));
    assert_eq!(cfg.gradient_min_dist, 40_000.0);
}

#[test]
fn gradient_legend_switch_turns_the_gradient_and_its_controls_off() {
    let mut app = edges_panel();
    assert!(!app.get_by_label("Gap %").is_disabled());
    app.get_by_label("Edge gradient").click();
    app.run();
    assert!(!app.app_state().edge_style.use_gradient);
    app.run();
    assert!(app.get_by_label("Gap %").is_disabled());
    assert!(app.get_by_label("Min Dist").is_disabled());
}

#[test]
fn min_size_follows_scale_arrow_heads() {
    let mut app = edges_panel();
    assert!(!app.get_by_label("Min Size").is_disabled());
    app.get_by_label("Scale Arrow Heads").click();
    app.run();
    app.run();
    assert!(!app.app_state().edge_style.scale_arrow_heads);
    assert!(app.get_by_label("Min Size").is_disabled());
}

#[test]
fn barrel_and_reexport_rows_have_distinct_highlight_toggles_and_an_info_tooltip() {
    let mut app = edges_panel();
    app.get_by_label("Barrel and Re-export info");
    app.get_by_label("Show re-export edges").click();
    app.get_by_label("Highlight barrel edges").click();
    app.run();
    let cfg = &app.app_state().edge_style;
    assert!(cfg.show_reexport_edges && cfg.highlight_barrel && !cfg.highlight_reexport);
}

#[test]
fn kebab_reset_restores_the_edge_defaults() {
    let mut app = edges_panel();
    {
        let s = &mut app.app_state_mut().edge_style;
        s.use_gradient = false;
        s.show_reexport_edges = true;
        s.min_size_pct = 0.1;
    }
    app.run();
    app.get_by_label("Dependency Edges menu").click();
    app.run();
    app.get_by_label("Reset Dependency Edges settings").click();
    app.run();
    let cfg = &app.app_state().edge_style;
    assert!(cfg.use_gradient && !cfg.show_reexport_edges);
    assert_eq!(cfg.min_size_pct, 0.5);
}
