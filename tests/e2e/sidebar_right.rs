use egui_kittest::kittest::Queryable;
use spatialcoding::edges::style::EdgeStyle;
use spatialcoding::state::RightPanel;

use crate::common::*;

#[test]
fn git_controls_compare_real_relationships_and_hide_deleted_links() {
    use spatialcoding::edges::edge_store::EdgeChange;
    let fx = fixture();
    let git = |args: &[&str]| {
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&fx.root)
            .args(args)
            .output()
            .unwrap()
            .status
            .success())
    };
    git(&["init", "-q"]);
    std::fs::write(fx.path("src/added.rs"), "pub fn added() {}\n").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-qm",
        "baseline",
    ]);
    std::fs::write(fx.path("src/main.rs"), "mod added;\nfn main() {}\n").unwrap();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));
    app.open(&fx.path("src/util.rs"));
    app.open(&fx.path("src/added.rs"));
    app.set_size(egui::vec2(1800.0, 2200.0));
    app.app_state_mut().right_panel = Some(RightPanel::DependencyEdges);
    app.run();
    assert!(app
        .get_by_role_and_label(egui::accesskit::Role::CheckBox, "Changes")
        .is_disabled());
    app.get_by_label("Refresh comparison").click();
    app.step();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.app_state().git_comparison.is_loading() {
        assert!(
            std::time::Instant::now() < deadline,
            "comparison never settled"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.step();
    }
    assert!(app.app_state().git_comparison.is_ready());
    assert!(!app
        .get_by_role_and_label(egui::accesskit::Role::CheckBox, "Changes")
        .is_disabled());
    app.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Changes")
        .click();
    app.get_by_label("Diff Colors").click();
    app.get_by_label("Deleted").click();
    app.run();
    fn changed_stroke(shape: &egui::Shape, red: bool) -> bool {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().any(|s| changed_stroke(s, red)),
            egui::Shape::CubicBezier(b) => match b.stroke.color {
                egui::epaint::ColorMode::Solid(c) => {
                    if red {
                        u16::from(c.r()) > u16::from(c.g()) * 2
                    } else {
                        u16::from(c.g()) > u16::from(c.r()) * 2
                    }
                }
                _ => false,
            },
            _ => false,
        }
    }
    app.show_at(egui::pos2(750.0, 50.0), egui::pos2(800.0, 500.0));
    assert!(
        app.output()
            .shapes
            .iter()
            .any(|s| changed_stroke(&s.shape, true)),
        "deleted edge is painted red"
    );
    assert!(
        app.output()
            .shapes
            .iter()
            .any(|s| changed_stroke(&s.shape, false)),
        "added edge is painted green"
    );
    let s = app.app_state();
    assert!(
        s.edge_style.highlight_changes && s.edge_style.diff_colors && s.edge_style.show_deleted
    );
    assert!(s
        .edge_store
        .all_edges()
        .any(|e| e.change == EdgeChange::Added));
    let deleted = s
        .edge_store
        .all_edges()
        .find(|e| e.change == EdgeChange::Deleted)
        .unwrap();
    assert!(deleted.is_visible_for(&s.edge_style, &s.selection));
    assert!(
        deleted.source_span.is_none() && deleted.source_name_span.is_none(),
        "obsolete baseline positions never attach to current identifiers"
    );
    app.get_by_label("Deleted").click();
    app.run();
    assert!(
        !app.output()
            .shapes
            .iter()
            .any(|s| changed_stroke(&s.shape, true)),
        "Deleted toggle removes the red baseline stroke"
    );
    let s = app.app_state();
    assert!(!s
        .edge_store
        .all_edges()
        .find(|e| e.change == EdgeChange::Deleted)
        .unwrap()
        .is_visible_for(&s.edge_style, &s.selection));
    spatialcoding::workspace::clear_canvas(app.app_state_mut());
    app.run();
    assert!(
        !app.app_state().git_comparison.is_ready(),
        "clearing the canvas invalidates the snapshot"
    );
}

#[test]
fn reference_visibility_controls_update_the_shared_graph_policy() {
    use spatialcoding::edges::style::{GradientGapMode, RelationshipType};
    let mut app = app();
    app.app_state_mut().right_panel = Some(RightPanel::DependencyEdges);
    app.run();
    app.get_by_label("All").click();
    app.get_by_label("References").click();
    app.get_by_label("Selected").click();
    app.get_by_label("Levels").click();
    app.get_by_label("Backwards").click();
    app.get_by_label("Highlight Backwards Edges").click();
    app.run();
    assert!(app.app_state().edge_style.use_gradient, "the gradient is on by default, as in v2");
    let cfg = &app.app_state().edge_style;
    assert!(!cfg.show_all);
    assert!(!cfg.relationships[RelationshipType::References.index()].visible);
    assert!(!cfg.highlight_selected);
    assert!(cfg.folder_levels && cfg.highlight_backwards && cfg.highlight_reverse_layout);
    app.get_by_label("Fixed").click();
    app.run();
    assert_eq!(
        app.app_state().edge_style.gradient_gap_mode,
        GradientGapMode::Fixed
    );
    assert!(app.get_by_label("Diff Colors").is_disabled());
    assert!(app.get_by_label("Deleted").is_disabled());
}

#[test]
fn missing_git_repository_reports_unavailable_without_enabling_diff_controls() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));
    app.set_size(egui::vec2(1800.0, 2200.0));
    app.app_state_mut().right_panel = Some(RightPanel::DependencyEdges);
    app.run();
    app.get_by_label("Git baseline");
    app.get_by_label("Refresh comparison").click();
    app.step();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.app_state().git_comparison.is_loading() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.step();
    }
    assert!(matches!(
        app.app_state().git_comparison.status,
        spatialcoding::git::comparison::ComparisonStatus::Unavailable(_)
    ));
    assert!(app.get_by_label("Diff Colors").is_disabled());
    app.get_by_label_contains("Git repository unavailable");
}

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
    let header = widget_rect(&app, "Close Layout");
    let before = widget_rect(&app, "Alt Rev");
    let offset = app.app_state().viewport.offset;
    let zoom = app.app_state().viewport.zoom;
    let pointer = egui::pos2(1200.0, 250.0);

    wheel(
        &mut app,
        pointer,
        egui::vec2(0.0, -600.0),
        egui::Modifiers::NONE,
    );

    let after = widget_rect(&app, "Alt Rev");
    assert!(
        after.top() < before.top(),
        "body did not scroll: {before:?} -> {after:?}"
    );
    assert!(
        after.bottom() < 400.0 && after.top() > header.bottom(),
        "lower control is reachable: {after:?}"
    );
    assert_eq!(
        widget_rect(&app, "Close Layout"),
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
        layout_top.top() > widget_rect(&app, "Close Layout").bottom(),
        "new tab starts at its own top"
    );
    wheel(
        &mut app,
        pointer,
        egui::vec2(0.0, -400.0),
        egui::Modifiers::NONE,
    );
    app.get_by_label("Dependency Edges").click();
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

    app.get_by_label("Dependency Edges").click();
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

    assert!(
        app.query_by_label("Symbol Pills").is_none(),
        "removed badges have no toggle"
    );
}

#[test]
fn clicking_the_active_tab_collapses_the_panel() {
    let mut app = app();
    app.app_state_mut().right_panel = Some(RightPanel::DependencyEdges);
    app.run();

    app.get_by_label("Dependency Edges").click();
    app.run();
    assert_eq!(app.app_state().right_panel, None);
    // The rail is always visible now; Layout is the first rail entry.
    app.get_by_label("Layout");
}
