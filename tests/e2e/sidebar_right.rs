use egui_kittest::kittest::Queryable;
use spatialcoding::edges::style::EdgeStyle;
use spatialcoding::state::RightPanel;

use crate::common::*;

#[test]
fn dependency_edges_explain_unresolved_rust_modules_without_lsp() {
    let fx = fixture();
    std::fs::write(
        fx.path("src/main.rs"),
        "mod util;\n#[cfg(test)] mod gated;\nmod missing;\nmod clash;\n",
    )
    .unwrap();
    std::fs::write(fx.path("src/clash.rs"), "").unwrap();
    std::fs::create_dir(fx.path("src/clash")).unwrap();
    std::fs::write(fx.path("src/clash/mod.rs"), "").unwrap();
    let mut app = app_with(&fx);
    app.app_state_mut()
        .lsp
        .manager
        .set_launcher(std::sync::Arc::new(|_, _| Err("offline".into())));
    for path in [
        "src/main.rs",
        "src/util.rs",
        "src/clash.rs",
        "src/clash/mod.rs",
    ] {
        app.open(&fx.path(path));
    }
    app.set_size(egui::vec2(1800.0, 2200.0));
    app.app_state_mut().right_panel = Some(RightPanel::DependencyEdges);
    app.run();
    app.get_by_label("Rust modules (static)");
    app.get_by_label("3 unresolved Rust modules");
    app.get_by_label_contains("gated: Conditional");
    app.get_by_label_contains("missing: No file for this module in the folder");
    app.get_by_label_contains("clash: Multiple targets in the folder");
    assert_eq!(app.app_state().edge_store.all_edges().count(), 1);

    std::fs::write(fx.path("src/missing.rs"), "").unwrap();
    let target = app.open(&fx.path("src/missing.rs"));
    app.run();
    app.get_by_label("2 unresolved Rust modules");
    // The folder's file list is read again when the watcher reports the new
    // file; do it here so the test does not wait for it.
    app.app_state().folder_files_cache.replace(None);
    spatialcoding::workspace::remove_nodes(app.app_state_mut(), &[target]);
    app.app_state_mut().edge_style.show_all = false;
    app.run();
    // The module file still exists in the folder, so closing it does not make
    // the declaration unresolved; only the link to it is no longer drawn.
    app.get_by_label("2 unresolved Rust modules");
    assert!(app.query_by_label("3 unresolved Rust modules").is_none());

    spatialcoding::workspace::clear_canvas(app.app_state_mut());
    app.run();
    assert!(app.query_by_label("Rust modules (static)").is_none());
}

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
    app.get_by_label("All").click();
    app.run();
    app.get_by_label("Levels").click();
    app.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Backwards")
        .click();
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
    assert!(deleted.is_visible(&s.edge_style));
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
        .is_visible(&s.edge_style));
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
    let toggle_count = |app: &App, name| {
        app.query_all_by_role_and_label(egui::accesskit::Role::CheckBox, name)
            .count()
    };
    assert_eq!(
        toggle_count(&app, "Selected"),
        0,
        "All keeps the panel compact"
    );
    assert_eq!(toggle_count(&app, "Highlight Backwards Edges"), 1);
    app.get_by_label("All").click();
    app.run();
    assert_eq!(
        toggle_count(&app, "References"),
        0,
        "relationships start collapsed"
    );
    app.get_by_label("Relationship types").click();
    app.run();
    app.get_by_label("References").click();
    app.get_by_label("Selected").click();
    app.get_by_label("Levels").click();
    app.run();
    app.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Backwards")
        .click();
    app.run();
    app.get_by_label("Highlight Backwards Edges").click();
    app.run();
    assert!(
        app.app_state().edge_style.use_gradient,
        "the gradient is on by default, as in v2"
    );
    let cfg = &app.app_state().edge_style;
    assert!(!cfg.show_all);
    assert!(!cfg.relationships[RelationshipType::References.index()].visible);
    assert!(!cfg.highlight_selected);
    assert!(!cfg.folder_levels && !cfg.highlight_backwards && cfg.highlight_reverse_layout);
    app.get_by_label("All").click();
    app.run();
    assert_eq!(toggle_count(&app, "Selected"), 0);
    app.get_by_label("All").click();
    app.run();
    assert!(
        !app.app_state().edge_style.relationships[RelationshipType::References.index()].visible
    );
    assert!(!app.app_state().edge_style.highlight_selected);
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
fn folder_levels_backwards_and_selection_filter_strokes_and_ports_together() {
    use spatialcoding::analysis::dependencies::LineSpan;
    use spatialcoding::edges::{ports, style::RelationshipType};

    let mut app = app();
    let state = app.app_state_mut();
    let nodes = &mut state.node_store;
    let file = |nodes: &mut spatialcoding::nodes::node_store::NodeStore, path: &str, x, y| {
        nodes.add_file(path.into(), "const x = 1;\n".into(), egui::pos2(x, y))
    };
    let a = file(nodes, "src/a/one.txt", 100.0, 100.0);
    let b = file(nodes, "src/a/two.txt", 500.0, 140.0);
    let c = file(nodes, "src/b/three.txt", 900.0, 220.0);
    for path in ["src", "src/a", "src/b"] {
        nodes.add_folder(path.into(), 0);
    }
    let row = Some(LineSpan { start: 0, end: 0 });
    state.edge_store.add_spanned(a, b, None, row, row, None);
    state.edge_store.add_spanned(c, b, None, row, row, None);
    state.settings.node_width = 150.0;
    state.edge_style.show_all = false;
    state.edge_style.folder_levels = true;
    state.edge_style.folder_depth = 0;
    state.edge_style.highlight_selected = false;
    state.edge_style.highlight_backwards = false;
    let check = |app: &mut App, count, sources: &[u64]| {
        app.show_at(egui::pos2(575.0, 180.0), egui::pos2(700.0, 450.0));
        let state = app.app_state();
        let ports = ports::collect_ports(
            &state.edge_store,
            &state.node_store,
            &state.edge_style,
            &state.selection,
            state.settings.node_width,
            state.settings.font_size,
        );
        assert_eq!(ports.contains_key(&a), sources.contains(&a));
        assert_eq!(ports.contains_key(&c), sources.contains(&c));
        assert_eq!(
            app.output()
                .shapes
                .iter()
                .filter(|shape| matches!(shape.shape, egui::Shape::CubicBezier(_)))
                .count(),
            count,
            "unexpected strokes for sources {sources:?}",
        );
    };
    check(&mut app, 1, &[a]);
    app.app_state_mut().edge_style.folder_depth = 1;
    check(&mut app, 2, &[a, c]);
    app.app_state_mut().edge_style.folder_depth = -1;
    check(&mut app, 0, &[]);
    app.app_state_mut().edge_style.folder_levels = false;
    app.app_state_mut().selection.select(a);
    check(&mut app, 0, &[]);
    app.app_state_mut().edge_style.highlight_backwards = true;
    check(&mut app, 1, &[c]);
    app.app_state_mut().edge_style.show_external = true;
    app.app_state_mut().edge_style.highlight_backwards = false;
    app.app_state_mut().edge_style.external_nesting_max = 1;
    check(&mut app, 0, &[]);
    app.app_state_mut().edge_style.external_nesting_max = 2;
    check(&mut app, 1, &[c]);
    app.app_state_mut().edge_style.relationships[RelationshipType::References.index()].visible =
        false;
    check(&mut app, 0, &[]);
    app.app_state_mut().edge_style.show_all = true;
    check(&mut app, 2, &[a, c]);
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
