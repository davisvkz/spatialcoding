//! Dependency Edges panel: v2 defaults, the Gradient legend switch, arrow
//! Min Size, the Barrel and Re-export rows and the kebab reset.

use egui::accesskit::Toggled;
use egui_kittest::kittest::Queryable;
use spatialcoding::edges::style::{EdgeStyleConfig, MARKER_COLOR, SELECTED_COLOR};
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
fn header_switch_hides_and_restores_the_filtered_graph_without_reindexing() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let source = app.open(&fx.path("src/main.rs"));
    let target = app.open(&fx.path("src/util.rs"));
    app.set_size(egui::vec2(2400.0, 900.0));
    {
        let s = app.app_state_mut();
        s.left_panel = None;
        s.right_panel = Some(RightPanel::DependencyEdges);
        s.node_store.get_file_mut(source).unwrap().position = egui::Pos2::ZERO;
        s.node_store.get_file_mut(target).unwrap().position = egui::pos2(900.0, 0.0);
        s.selection.select(source);
        s.viewport.zoom = 1.0;
        s.viewport.target_zoom = 1.0;
    }
    app.show_at(egui::pos2(800.0, 60.0), egui::pos2(1000.0, 300.0));
    app.get_by_label("All").click();
    app.run();
    let edges = |app: &App| {
        let mut ids: Vec<_> = app
            .app_state()
            .edge_store
            .all_edges()
            .map(|e| e.id)
            .collect();
        ids.sort_unstable();
        ids
    };
    let ids = edges(&app);
    let [r, g, b] = SELECTED_COLOR.map(|c| (c * 255.0) as u8);
    let arrow_color = egui::Color32::from_rgb(r, g, b);
    let statement_color = app
        .app_state()
        .edge_style
        .kind_color("fn")
        .gamma_multiply(0.85);
    let decorations = |app: &App| {
        let mut counts = [0; 4];
        for shape in frame_shapes(app) {
            match shape {
                egui::Shape::CubicBezier(_) => counts[0] += 1,
                egui::Shape::Path(p) if p.points.len() == 3 && p.fill == arrow_color => {
                    counts[1] += 1;
                }
                egui::Shape::Circle(c) if c.fill == MARKER_COLOR.gamma_multiply(0.5) => {
                    counts[2] += 1;
                }
                egui::Shape::Rect(r) if r.fill == statement_color => counts[3] += 1,
                _ => {}
            }
        }
        counts
    };
    let before = decorations(&app);
    assert!(before.iter().all(|&n| n > 0), "{before:?}");

    app.get_by_label("Show dependency edges").click();
    app.run();
    assert_eq!(decorations(&app), [0; 4]);
    assert_eq!(edges(&app), ids);
    assert_eq!(app.selected(), [source]);
    assert!(!app.app_state().edge_style.show_all);

    app.get_by_label("Show dependency edges").click();
    app.run();
    assert_eq!(decorations(&app), before);
    assert_eq!(edges(&app), ids);
    assert!(!app.app_state().edge_style.show_all);
}

#[test]
fn hidden_dependency_edges_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let mut app = edges_panel();
    app.state_mut().enable_settings_persistence(path.clone());
    app.get_by_label("Show dependency edges").click();
    app.run();
    for _ in 0..8 {
        app.step();
    }
    let mut reopened = edges_panel();
    reopened.state_mut().enable_settings_persistence(path);
    reopened.run();
    assert_eq!(
        reopened.get_by_label("Show dependency edges").toggled(),
        Some(Toggled::False)
    );
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
    app.get_by_label("Show dependency edges").click();
    app.run();
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
    app.run();
    assert_eq!(
        app.get_by_label("Show dependency edges").toggled(),
        Some(Toggled::True)
    );
}

#[test]
fn an_import_of_a_closed_file_is_drawn_when_that_file_opens() {
    let fx = fixture();
    let mut app = app_with(&fx);
    // `web/app.ts` imports `./a`, which is in the folder but not open.
    app.open(&fx.path("web/app.ts"));
    app.settle_edges();
    assert_eq!(app.app_state().edge_store.all_edges().count(), 0, "no edge to a file that is not open");
    app.open(&fx.path("web/a.ts"));
    app.settle_edges();
    // The import itself, and the use of `a` the (fake) server reports.
    let imports: Vec<_> = app
        .app_state()
        .edge_store
        .all_edges()
        .filter(|e| e.relationship == spatialcoding::edges::style::RelationshipType::Imports)
        .collect();
    assert_eq!(imports.len(), 1, "the import appears with its target");
    let path_of = |id| app.app_state().node_store.get_file(id).unwrap().path.clone();
    assert_eq!((path_of(imports[0].source_node), path_of(imports[0].target_node)), (fx.path("web/app.ts"), fx.path("web/a.ts")));
}

#[test]
fn imports_resolve_through_extended_configs_and_workspace_packages_in_the_app() {
    let fx = fixture();
    let write = |rel: &str, text: &str| {
        let path = fx.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write("tsconfig.base.json", r#"{"compilerOptions": {"paths": {"@/*": ["web/*"]}}}"#);
    write("tsconfig.json", r#"{"extends": "./tsconfig.base.json"}"#);
    write("packages/ui/package.json", r#"{"name": "@acme/ui", "exports": {".": "./src/index.ts"}}"#);
    write("packages/ui/src/index.ts", "export const ui = 1;\n");
    write("web/app.ts", "import '@/a';\nimport '@acme/ui';\nimport 'react';\n");
    let mut app = app_with(&fx);
    // No language server: the fake one would draw these edges from the words
    // of the imports, and this test is about the static analysis.
    app.app_state_mut().lsp.manager.set_launcher(std::sync::Arc::new(|_, _| Err("offline".into())));
    for path in ["web/app.ts", "web/a.ts", "packages/ui/src/index.ts"] {
        app.open(&fx.path(path));
    }
    app.settle_edges();
    let path_of = |id| app.app_state().node_store.get_file(id).unwrap().path.clone();
    let mut targets: Vec<_> = app
        .app_state()
        .edge_store
        .all_edges()
        .filter(|e| e.relationship == spatialcoding::edges::style::RelationshipType::Imports)
        .map(|e| (path_of(e.source_node), path_of(e.target_node)))
        .collect();
    targets.sort();
    assert_eq!(
        targets,
        vec![
            (fx.path("web/app.ts"), fx.path("packages/ui/src/index.ts")),
            (fx.path("web/app.ts"), fx.path("web/a.ts")),
        ],
        "the alias comes from the extended base config and the package from its package.json"
    );
}
