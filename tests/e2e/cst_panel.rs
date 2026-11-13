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
fn cst_tab_asks_for_a_selection() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.app_state_mut().right_panel = Some(RightPanel::Cst);
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
    assert!(app.query_by_label_contains("\"fn\"").is_none(), "anonymous tokens hidden");

    // Nested rows open on click, and anonymous tokens can be shown.
    app.get_by_label("function_item  L3\u{2013}5").click();
    app.run();
    app.get_by_label("name: identifier \"main\"  L3");

    app.get_by_label("Show anonymous nodes").click();
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
        Some(CstHover { node_id: id, start_line: 2, end_line: 4 })
    );
    let highlight = highlight_rects(&app);
    assert_eq!(highlight.len(), 1, "one highlight painted on the canvas");
    let s = app.app_state();
    let line_h = line_height(s.settings.font_size) * s.viewport.zoom;
    assert!((highlight[0].height() - 3.0 * line_h).abs() < 0.5, "covers lines 3-5");

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
