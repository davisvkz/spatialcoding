//! Folders panel: collapse actions, double-click on a folder, collapsed
//! boxes on the canvas and the edges routed to them.

use egui_kittest::kittest::Queryable;
use spatialcoding::state::RightPanel;

use crate::common::*;

/// The fixture with src/main.rs, src/util.rs, web/app.ts and web/a.ts open.
fn folders_panel(fx: &Fixture) -> (App, [u64; 4]) {
    let mut app = app_with(fx);
    app.app_state_mut().left_panel = None;
    app.app_state_mut().right_panel = Some(RightPanel::Folders);
    let ids = [
        app.open(&fx.path("src/main.rs")),
        app.open(&fx.path("src/util.rs")),
        app.open(&fx.path("web/app.ts")),
        app.open(&fx.path("web/a.ts")),
    ];
    app.run();
    (app, ids)
}

fn folder(app: &App, fx: &Fixture, rel: &str) -> u64 {
    let path = fx.path(rel);
    app.app_state().node_store.folder_nodes.values().find(|f| f.path == path).unwrap().id
}

fn beziers(app: &App) -> usize {
    frame_shapes(app).iter().filter(|s| matches!(s, egui::Shape::CubicBezier(_))).count()
}

#[test]
fn collapse_all_hides_files_behind_folder_boxes_and_expand_all_restores_them() {
    let fx = fixture();
    let (mut app, [main, ..]) = folders_panel(&fx);
    app.get_by_label("Collapse all folders").click();
    app.run();
    let s = app.app_state();
    assert!(s.node_store.folder_nodes.values().all(|f| f.collapsed));
    assert!(s.node_store.is_file_hidden(main));
    assert!(app.get_by_label("Collapse all folders").is_disabled());

    app.get_by_label("Expand all folders").click();
    app.run();
    assert!(!app.app_state().node_store.is_file_hidden(main));
}

#[test]
fn double_clicking_a_folder_label_band_collapses_it_and_its_box_expands_it() {
    let fx = fixture();
    let mut app = app_with_stepping(&fx, 1.0 / 60.0);
    app.app_state_mut().left_panel = None;
    let main = app.open(&fx.path("src/main.rs"));
    app.open(&fx.path("src/util.rs"));
    for _ in 0..30 {
        app.step();
    }
    let src = folder(&app, &fx, "src");
    let top_left = app.app_state().node_store.get_folder(src).unwrap().bounds.min;
    app.show_at(top_left + egui::vec2(40.0, 10.0), CANVAS_SPOT);
    double_click_at(&mut app, CANVAS_SPOT);
    assert!(app.app_state().node_store.get_folder(src).unwrap().collapsed);
    assert!(app.app_state().node_store.is_file_hidden(main));

    let center = app.app_state().node_store.get_folder(src).unwrap().bounds.center();
    app.show_at(center, CANVAS_SPOT);
    double_click_at(&mut app, CANVAS_SPOT);
    assert!(!app.app_state().node_store.get_folder(src).unwrap().collapsed);
}

#[test]
fn edges_into_a_collapsed_folder_end_on_its_box_and_merge_by_default() {
    let fx = fixture();
    std::fs::create_dir_all(fx.path("shared")).unwrap();
    std::fs::write(fx.path("shared/b.ts"), "export const b = 1;\n").unwrap();
    std::fs::write(fx.path("shared/c.ts"), "export const c = 2;\n").unwrap();
    std::fs::write(
        fx.path("web/uses.ts"),
        "import { b } from '../shared/b';\nimport { c } from '../shared/c';\nconsole.log(b, c);\n",
    )
    .unwrap();
    let mut app = app_with(&fx);
    {
        let s = app.app_state_mut();
        s.left_panel = None;
        s.edge_style.use_gradient = false;
        s.edge_style.hide_short_pct = 0.0;
        s.edge_style.show_all = true;
    }
    let uses = app.open(&fx.path("web/uses.ts"));
    app.open(&fx.path("shared/b.ts"));
    app.open(&fx.path("shared/c.ts"));
    let shared = folder(&app, &fx, "shared");
    app.app_state_mut().layout_settings.auto_layout = false;
    app.app_state_mut().pending_fit = true;
    app.run();
    let expanded = beziers(&app);
    assert!(expanded >= 2, "uses.ts links to b.ts and c.ts: {expanded}");

    spatialcoding::folder_actions::set_collapsed(app.app_state_mut(), shared, true);
    app.app_state_mut().pending_fit = true;
    app.run();
    let merged = beziers(&app);
    assert_eq!(merged, 1, "both links end on the shared box, drawn once");
    let box_rect = app.app_state().node_store.get_folder(shared).unwrap().bounds;
    let s = app.app_state();
    let screen_box = s.viewport.world_to_screen_rect(box_rect);
    let end = frame_shapes(&app)
        .iter()
        .find_map(|shape| match shape {
            egui::Shape::CubicBezier(b) => Some(b.points[3]),
            _ => None,
        })
        .unwrap();
    assert!(
        (end.x - screen_box.left()).abs() < 1.0 || (end.x - screen_box.right()).abs() < 1.0,
        "edge ends on a side of the box: {end:?} vs {screen_box:?}"
    );
    assert!(!s.node_store.is_file_hidden(uses));

    app.app_state_mut().settings.folders.merge_collapsed_edges = false;
    app.run();
    assert_eq!(beziers(&app), expanded, "unmerged: one edge per link again");
}

/// The fixture plus nested/deep/inner/leaf.rs, all four files open, folders shown.
fn display_app(fx: &Fixture) -> App {
    std::fs::create_dir_all(fx.path("nested/deep/inner")).unwrap();
    std::fs::write(fx.path("nested/deep/inner/leaf.rs"), "pub fn leaf() {}\n").unwrap();
    let mut app = app_with(fx);
    {
        let s = app.app_state_mut();
        s.left_panel = None;
        s.right_panel = Some(RightPanel::Folders);
        s.viewport.zoom = 0.5;
        s.viewport.target_zoom = 0.5;
    }
    app.open(&fx.path("src/main.rs"));
    app.open(&fx.path("nested/deep/inner/leaf.rs"));
    app.app_state_mut().pending_fit = true;
    app.run();
    app
}

fn painted_texts(app: &App) -> Vec<String> {
    frame_shapes(app)
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn hide_folders_is_on_by_default_and_folder_names_follow_the_display_switches() {
    let fx = fixture();
    let mut app = display_app(&fx);
    assert!(app.app_state().settings.folders.hide_folders, "v2 default");
    assert!(!painted_texts(&app).iter().any(|t| t == "src"), "hidden folders draw no name");

    app.get_by_label("Hide Folders").click();
    app.run();
    let texts = painted_texts(&app);
    assert!(texts.iter().any(|t| t == "src"), "{texts:?}");
    // nested/deep only holds inner: Merge Empty Folders names the chain once.
    assert!(texts.iter().any(|t| t.contains("deep/inner")), "{texts:?}");

    app.get_by_label("Merge Empty Folders").click();
    app.run();
    let texts = painted_texts(&app);
    assert!(texts.iter().any(|t| t == "inner") || texts.iter().any(|t| t.contains(" / inner")), "{texts:?}");

    app.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Folder Names").click();
    app.run();
    assert!(!painted_texts(&app).iter().any(|t| t == "src"));
}

#[test]
fn a_revealed_selection_is_laid_out_under_its_collapsed_folders_card() {
    let fx = fixture();
    let mut app = display_app(&fx);
    let main = app
        .app_state()
        .node_store
        .file_nodes
        .values()
        .find(|n| n.path.ends_with("src/main.rs"))
        .unwrap()
        .id;
    app.app_state_mut().selection.select(main);
    app.get_by_label("Selected").click();
    app.run();
    assert!(app.app_state().settings.folders.reveal_selected);
    let src = folder(&app, &fx, "src");
    spatialcoding::folder_actions::set_collapsed(app.app_state_mut(), src, true);
    app.run();
    let s = app.app_state();
    assert!(!s.node_store.is_file_hidden(main), "the selection stays visible");
    let card = s.node_store.get_folder(src).unwrap().collapsed_rect().unwrap();
    let node = s.node_store.get_file(main).unwrap().world_rect(s.settings.node_width, s.settings.font_size);
    assert!(node.top() >= card.bottom() && (node.left() - card.left()).abs() < 1e-3, "{node:?} under {card:?}");
}

#[test]
fn advanced_section_folds_and_its_switches_set_the_folder_options() {
    let fx = fixture();
    let mut app = display_app(&fx);
    assert!(app.query_by_label("Zoom Fog").is_none(), "Advanced starts folded");
    app.get_by_label("Expand Advanced").click();
    app.run();
    for label in ["Collapse: Edges Only", "Auto-Collapse on Zoom", "Zoom Fog", "Merge Collapsed Edges"] {
        app.get_by_label(label).click();
        app.run();
    }
    let f = &app.app_state().settings.folders;
    assert!(f.collapse_edges_only && f.auto_collapse_on_zoom && f.zoom_fog && !f.merge_collapsed_edges);
    app.get_by_label("Reveal Through Collapse info");
}

#[test]
fn auto_collapse_on_zoom_turns_small_folders_into_blocks_without_moving_files() {
    let fx = fixture();
    let mut app = display_app(&fx);
    let positions: Vec<egui::Pos2> =
        app.app_state().node_store.file_nodes.values().map(|n| n.position).collect();
    {
        let s = app.app_state_mut();
        s.settings.folders.auto_collapse_on_zoom = true;
        s.viewport.zoom = 0.02;
        s.viewport.target_zoom = 0.02;
    }
    app.run();
    let s = app.app_state();
    assert!(!s.node_store.view.zoom_collapsed.is_empty());
    assert!(s.node_store.file_nodes.values().all(|n| s.node_store.is_file_hidden(n.id)));
    let after: Vec<egui::Pos2> = s.node_store.file_nodes.values().map(|n| n.position).collect();
    assert_eq!(positions, after, "zoom collapse never re-lays out");
    {
        let s = app.app_state_mut();
        s.viewport.zoom = 1.0;
        s.viewport.target_zoom = 1.0;
    }
    app.run();
    assert!(app.app_state().node_store.view.zoom_collapsed.is_empty());
}
