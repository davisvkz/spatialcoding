use egui::Modifiers;
use egui_kittest::kittest::Queryable;
use spatialcoding::nodes::file_node::line_height;
use spatialcoding::state::RightPanel;
use spatialcoding::ui::cst_panel::CstHover;

use crate::common::*;

/// Opens `rel`, selects it with a click on the canvas and opens the CST tab.
fn select_and_open_cst(app: &mut App, fx: &Fixture, rel: &str) -> u64 {
    let id = app.open(&fx.path(rel));
    let center = app.node_center(id);
    app.show_at(center, CANVAS_SPOT);
    click_at(app, CANVAS_SPOT, Modifiers::NONE);
    assert_eq!(app.selected(), [id]);

    app.app_state_mut().right_panel = Some(RightPanel::Layout);
    app.run();
    app.get_by_label("CST").click();
    app.run();
    assert_eq!(app.app_state().right_panel, Some(RightPanel::Cst));
    id
}

#[test]
fn mapping_lists_canvas_types_without_a_selection_and_counts_selected_files() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let main = app.open(&fx.path("src/main.rs"));
    app.open(&fx.path("src/util.rs"));
    app.app_state_mut().selection.clear();
    app.app_state_mut().right_panel = Some(RightPanel::Cst);
    app.run();
    app.get_by_label("Symbol Mapping");
    app.get_by_label("function item");
    let rows = &app.app_state().cst.mapping;
    let functions = rows.iter().find(|r| r.kind == "function_item").unwrap();
    assert_eq!(functions.selected, 0);
    assert!(functions.nodes.len() >= 2);
    let total = functions.nodes.len();

    app.app_state_mut().selection.select(main);
    app.run();
    let functions = app
        .app_state()
        .cst
        .mapping
        .iter()
        .find(|r| r.kind == "function_item")
        .unwrap();
    assert_eq!(functions.selected, 1);
    assert_eq!(functions.nodes.len(), total);
    app.get_by_label("Edit function item");
}

#[test]
fn mapping_hover_uses_custom_color_and_respects_single_hover_and_canvas_switch() {
    let fx = fixture();
    let mut app = app_with(&fx);
    select_and_open_cst(&mut app, &fx, "src/main.rs");
    app.open(&fx.path("src/util.rs"));
    app.app_state_mut()
        .settings
        .cst
        .type_colors
        .insert("function_item".into(), [240, 10, 20]);
    app.app_state_mut().settings.cst.single_hover = false;
    app.run();
    app.get_by_label("function item").hover();
    app.run();
    assert!(app.app_state().cst.mapping_hover.len() >= 2);
    let color = egui::Color32::from_rgb(240, 10, 20).gamma_multiply(0.25);
    assert!(app
        .output()
        .shapes
        .iter()
        .any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == color)));

    app.app_state_mut().settings.cst.single_hover = true;
    app.run();
    assert_eq!(app.app_state().cst.mapping_hover.len(), 1);
    app.app_state_mut().settings.cst.show_canvas = false;
    app.run();
    assert!(!app
        .output()
        .shapes
        .iter()
        .any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == color)));
    app.get_by_label("CST menu").click();
    app.run();
    app.get_by_label("Reset CST settings").click();
    app.run();
    assert!(app.app_state().settings.cst.type_colors.is_empty());
}

#[test]
fn cst_tab_asks_for_a_selection() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.app_state_mut().right_panel = Some(RightPanel::Cst);
    app.run();
    app.get_by_label("Open some files on the canvas to start populating the CST nodes list.");

    app.open(&fx.path("src/main.rs"));
    app.app_state_mut().selection.clear();
    app.run();
    app.get_by_label("Select a file node to see its syntax tree.");
}

#[test]
fn cst_tab_shows_the_tree_of_the_selected_rust_file() {
    let fx = fixture();
    let mut app = app_with(&fx);
    select_and_open_cst(&mut app, &fx, "src/main.rs");

    app.get_by_label("main.rs");
    app.get_by_label_contains("Rust \u{b7}");
    app.get_by_label("source_file  L1\u{2013}5");
    app.get_by_label("mod_item  L1");
    app.get_by_label("function_item  L3\u{2013}5");
    assert!(
        app.query_by_label_contains("\"fn\"").is_none(),
        "anonymous tokens hidden"
    );

    // Nested rows open on click, and anonymous tokens can be shown.
    app.get_by_label("function_item  L3\u{2013}5").click();
    app.run();
    app.get_by_label("name: identifier \"main\"  L3");

    app.get_by_label("Toggle settings").click();
    app.run();
    app.get_by_label("Show Hidden Types").click();
    app.run();
    app.get_by_label("fn  L3");
}

#[test]
fn cst_tab_follows_the_selection_to_typescript() {
    let fx = fixture();
    let mut app = app_with(&fx);
    select_and_open_cst(&mut app, &fx, "web/app.ts");

    app.get_by_label_contains("TypeScript \u{b7}");
    app.get_by_label("program  L1\u{2013}2");
    app.get_by_label("import_statement  L1");
}

#[test]
fn unsupported_languages_say_so() {
    let fx = fixture();
    let mut app = app_with(&fx);
    select_and_open_cst(&mut app, &fx, "README.md");

    app.get_by_label("No syntax tree: Markdown is not supported.");
}

#[test]
fn hovering_a_row_highlights_its_lines_on_the_node() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let id = select_and_open_cst(&mut app, &fx, "src/main.rs");
    assert_eq!(app.app_state().cst.hovered, None);

    app.get_by_label("function_item  L3\u{2013}5").hover();
    app.run();
    assert_eq!(
        app.app_state().cst.hovered,
        Some(CstHover {
            node_id: id,
            start_line: 2,
            end_line: 4
        })
    );
    let highlight = highlight_rects(&app);
    assert_eq!(highlight.len(), 1, "one highlight painted on the canvas");
    let s = app.app_state();
    let line_h = line_height(s.settings.font_size) * s.viewport.zoom;
    assert!(
        (highlight[0].height() - 3.0 * line_h).abs() < 0.5,
        "covers lines 3-5"
    );

    // Leaving the CST tab drops the highlight.
    app.get_by_label("Layout").click();
    app.run();
    assert_eq!(app.app_state().cst.hovered, None);
    assert!(highlight_rects(&app).is_empty());
}

/// Rects painted in the CST highlight color during the last frame.
fn highlight_rects(app: &App) -> Vec<egui::Rect> {
    let color = app.app_state().theme.selection.gamma_multiply(0.25);
    app.output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(r) if r.fill == color => Some(r.rect),
            _ => None,
        })
        .collect()
}

#[test]
fn toggle_cst_nodes_switch_stops_painting_cst_highlights() {
    let fx = fixture();
    let mut app = app_with(&fx);
    select_and_open_cst(&mut app, &fx, "src/main.rs");
    app.get_by_label("Toggle CST nodes").click();
    app.run();
    assert!(!app.app_state().settings.cst.show_canvas);
    app.get_by_label("function_item  L3\u{2013}5").hover();
    app.run();
    assert!(
        app.app_state().cst.hovered.is_some(),
        "the row is still hovered"
    );
    assert!(highlight_rects(&app).is_empty(), "but nothing is painted");
}

#[test]
fn single_hover_off_also_marks_the_hovered_rows_ancestors() {
    let fx = fixture();
    let mut app = app_with(&fx);
    select_and_open_cst(&mut app, &fx, "src/main.rs");
    app.get_by_label("Toggle settings").click();
    app.run();
    app.get_by_label("Single Hover Symbol").click();
    app.run();
    app.get_by_label("function_item  L3\u{2013}5").hover();
    app.run();
    assert_eq!(
        app.app_state().cst.hover_ancestors,
        vec![(0, 4)],
        "source_file"
    );
}

#[test]
fn max_depth_limits_rows_and_the_count() {
    let fx = fixture();
    let mut app = app_with(&fx);
    select_and_open_cst(&mut app, &fx, "src/main.rs");
    app.app_state_mut().settings.cst.max_depth = 1;
    app.run();
    app.get_by_label("(2)");
    app.get_by_label("3 tree rows");
    app.app_state_mut().settings.cst.max_depth = 0;
    app.run();
    app.get_by_label("(0)");
    app.get_by_label("1 tree rows");
    assert!(app.query_by_label("mod_item  L1").is_none());
}

#[test]
fn selected_edges_outline_their_end_cst_nodes_and_reset_restores_defaults() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let main = select_and_open_cst(&mut app, &fx, "src/main.rs");
    let util = app.open(&fx.path("src/util.rs"));
    app.app_state_mut().selection.select(main);
    app.run();
    let nodes = app.app_state().cst.edge_nodes.clone();
    assert!(
        nodes.contains_key(&main) && nodes.contains_key(&util),
        "{nodes:?}"
    );

    app.get_by_label("Selected Edge CST Nodes").click();
    app.run();
    assert!(app.app_state().cst.edge_nodes.is_empty());

    app.get_by_label("CST menu").click();
    app.run();
    app.get_by_label("Reset CST settings").click();
    app.run();
    assert!(app.app_state().settings.cst.selected_edge_nodes);
}

/// Function-colored (`#3B82F6`) filled rectangles drawn this frame.
fn function_tints(app: &App) -> Vec<u8> {
    frame_shapes(app)
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Rect(r) if r.fill.a() > 0 && r.fill.b() > r.fill.r().saturating_mul(2) && r.fill.b() >= r.fill.g() => {
                Some(r.fill.a())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn zoomed_out_levels_tint_the_cst_by_role() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let id = app.open(&fx.path("src/util.rs"));
    app.app_state_mut().right_panel = None;
    let center = app.node_center(id);

    // L1 (zoom >= 0.30): readable text, no base tint.
    app.app_state_mut().viewport.zoom = 0.5;
    app.app_state_mut().viewport.target_zoom = 0.5;
    app.show_at(center, CANVAS_SPOT);
    assert!(function_tints(&app).is_empty(), "L1 has no CST base tint");

    // L2 (0.10 - 0.30): the function item is tinted at alpha .04.
    app.app_state_mut().viewport.zoom = 0.2;
    app.app_state_mut().viewport.target_zoom = 0.2;
    app.show_at(center, CANVAS_SPOT);
    let l2 = function_tints(&app);
    assert!(l2.contains(&10), "L2 tint alpha .04 -> 10, got {l2:?}");

    // L3 (0.04 - 0.10): alpha .10.
    app.app_state_mut().viewport.zoom = 0.06;
    app.app_state_mut().viewport.target_zoom = 0.06;
    app.show_at(center, CANVAS_SPOT);
    let l3 = function_tints(&app);
    assert!(l3.contains(&26), "L3 tint alpha .10 -> 26, got {l3:?}");

    // The Toggle CST nodes switch turns the tint off.
    app.app_state_mut().settings.cst.show_canvas = false;
    app.run();
    assert!(function_tints(&app).is_empty());
}

#[test]
fn a_hidden_type_is_not_tinted_and_a_style_override_changes_the_alpha() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let id = app.open(&fx.path("src/util.rs"));
    app.app_state_mut().right_panel = None;
    let center = app.node_center(id);
    app.app_state_mut().viewport.zoom = 0.2;
    app.app_state_mut().viewport.target_zoom = 0.2;
    app.show_at(center, CANVAS_SPOT);
    assert!(function_tints(&app).contains(&10));

    let mut style = spatialcoding::analysis::cst_style::CstTypeStyle::default_for("function_item");
    style.fill_alpha = 0.5;
    app.app_state_mut().settings.cst.type_styles.insert("function_item".into(), style.clone());
    app.run();
    assert!(function_tints(&app).contains(&5), "alpha .04 * .5 -> 5");

    style.visible = false;
    app.app_state_mut().settings.cst.type_styles.insert("function_item".into(), style);
    app.run();
    assert!(function_tints(&app).is_empty(), "a hidden type draws nothing");
}

#[test]
fn edge_cst_nodes_are_marked_only_with_three_or_fewer_files_selected() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let main = select_and_open_cst(&mut app, &fx, "src/main.rs");
    app.open(&fx.path("src/util.rs"));
    app.open(&fx.path("web/app.ts"));
    app.open(&fx.path("web/a.ts"));
    app.app_state_mut().selection.select(main);
    app.run();
    assert!(!app.app_state().cst.edge_nodes.is_empty(), "one selected file: marked");

    let ids = app.app_state().node_store.all_file_ids();
    assert!(ids.len() >= 4);
    app.app_state_mut().selection.select_all(ids);
    app.run();
    assert!(app.app_state().cst.edge_nodes.is_empty(), "four selected files: nothing is marked");
}

#[test]
fn a_type_style_editor_sets_visibility_fill_and_borders_and_resets() {
    use spatialcoding::analysis::cst_style::FillMode;
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/util.rs"));
    app.app_state_mut().selection.clear();
    app.app_state_mut().right_panel = Some(RightPanel::Cst);
    app.run();

    app.get_by_label("Style function item").click();
    app.run();
    app.get_by_label("Visible");
    app.get_by_label("4% at zoom 0.30");
    assert!(app.app_state().settings.cst.type_styles.is_empty(), "opening the editor changes nothing");

    app.get_by_label("Gradient").click();
    app.run();
    let style = app.app_state().settings.cst.type_styles["function_item"].clone();
    assert_eq!(style.fill, FillMode::Gradient);

    app.get_by_label("Visible").click();
    app.run();
    assert!(!app.app_state().settings.cst.type_styles["function_item"].visible);

    app.get_by_label("Left").click();
    app.run();
    let style = &app.app_state().settings.cst.type_styles["function_item"];
    assert!(!style.sides.left && style.sides.top, "only the left border is off");

    app.get_by_label("Reset to Default").click();
    app.run();
    assert!(app.app_state().settings.cst.type_styles.is_empty());
}
