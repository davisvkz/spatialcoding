//! The LSP tab against the in-process fake server in `fake_lsp.rs`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Modifiers, Pos2};
use egui_kittest::kittest::Queryable;
use spatialcoding::analysis::lsp::resolve_command;
use spatialcoding::nodes::file_node::{line_height, CODE_PADDING, HEADER_HEIGHT};
use spatialcoding::state::RightPanel;
use spatialcoding::ui::lsp_panel::LineHighlight;

use crate::common::*;
use crate::fake_lsp;

fn canvas_product(app: &mut App, fx: &Fixture) -> (u64, Pos2) {
    std::fs::write(fx.path("src/product.rs"), concat!(
        "pub struct Product {\n    pub id: u64,\n}\n",
        "impl Product {\n    pub fn new() -> Product {\n        Product { id: 1 }\n    }\n}\n",
        "impl Default for Product {\n    fn default() -> Self {\n        Product::new()\n    }\n}\n",
    )).unwrap();
    let id = app.open(&fx.path("src/product.rs"));
    app.app_state_mut().left_panel = None;
    app.app_state_mut().right_panel = None;
    app.app_state_mut().selection.clear();
    let font = app.app_state().settings.font_size;
    let node = app.app_state().node_store.get_file(id).unwrap();
    let origin = node.position + egui::vec2(0.0, HEADER_HEIGHT + CODE_PADDING);
    app.show_at(origin, Pos2::new(350.0, 180.0));
    // Find the actual glyph bounds, not an estimated column width.
    let at = frame_shapes(app)
        .iter()
        .find_map(|s| {
            let egui::Shape::Text(text) = s else {
                return None;
            };
            let offset = text.galley.job.text.find("Product")?;
            let chars = text.galley.job.text[..offset].chars().count();
            let left = text
                .galley
                .pos_from_ccursor(egui::text::CCursor::new(chars));
            let right = text
                .galley
                .pos_from_ccursor(egui::text::CCursor::new(chars + 7));
            let p = text.pos + egui::vec2((left.min.x + right.min.x) * 0.5, left.center().y);
            (p.y < 180.0 + line_height(font)).then_some(p)
        })
        .expect("the definition name is painted");
    (id, at)
}

#[test]
fn canvas_query_on_hover_draws_local_symbol_graph_without_selecting_or_moving() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    let (id, at) = canvas_product(&mut app, &fx);
    app.input_mut().events.push(egui::Event::PointerMoved(at));
    app.run();
    assert!(
        app.app_state().lsp.canvas_graph.graph.is_none(),
        "off by default"
    );
    let offset = app.app_state().viewport.offset;
    let zoom = app.app_state().viewport.zoom;
    app.app_state_mut().settings.lsp.query_on_hover = true;
    for _ in 0..4 {
        app.step();
    }
    settle(&mut app);
    let graph = app
        .app_state()
        .lsp
        .canvas_graph
        .graph
        .as_ref()
        .expect("hover graph");
    assert_eq!(graph.sites[0].node_id, id);
    assert_eq!((graph.sites[0].span.start, graph.sites[0].span.end), (0, 2));
    assert!(graph.sites.len() >= 5, "same-file references are included");
    assert!(
        graph
            .sites
            .iter()
            .any(|s| s.span.start == 3 && s.span.end == 7),
        "full impl block"
    );
    assert!(app.selected().is_empty());
    assert!(app.app_state().selection.symbols().is_empty());
    assert_eq!(app.app_state().viewport.offset, offset);
    assert_eq!(app.app_state().viewport.zoom, zoom);
    let shapes = frame_shapes(&app);
    assert!(shapes.iter().any(|s| matches!(s, egui::Shape::Rect(r)
        if r.fill == spatialcoding::ui::symbol_graph::DEFINITION_COLOR.gamma_multiply(0.10)
            && r.rect.height() > line_height(app.app_state().settings.font_size) * 2.5)));
    assert!(
        shapes
            .iter()
            .any(|s| matches!(s, egui::Shape::CubicBezier(_))),
        "local curved connections"
    );

    // A reference hit resolves back to the same definition, without clicking.
    let usage = graph.sites.iter().find(|s| s.name.line == 3).unwrap().name;
    let s = app.app_state();
    let node = s.node_store.get_file(id).unwrap();
    let usage_point = s.viewport.world_to_screen(
        node.position
            + egui::vec2(
                spatialcoding::nodes::file_node::CODE_PAD_X + 45.0,
                HEADER_HEIGHT
                    + CODE_PADDING
                    + (usage.line as f32 + 0.5) * line_height(s.settings.font_size),
            ),
    );
    app.input_mut()
        .events
        .push(egui::Event::PointerMoved(usage_point));
    for _ in 0..4 {
        app.step();
    }
    settle(&mut app);
    assert_eq!(
        app.app_state()
            .lsp
            .canvas_graph
            .graph
            .as_ref()
            .unwrap()
            .sites[0]
            .name
            .line,
        0
    );
    assert!(app.selected().is_empty());

    app.app_state_mut().settings.lsp.query_on_hover = false;
    app.run();
    assert!(app.app_state().lsp.canvas_graph.graph.is_none());
}

#[test]
fn canvas_hover_graph_clears_on_whitespace_and_closed_definition() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    let (id, at) = canvas_product(&mut app, &fx);
    app.app_state_mut().settings.lsp.query_on_hover = true;
    app.input_mut().events.push(egui::Event::PointerMoved(at));
    for _ in 0..4 {
        app.step();
    }
    settle(&mut app);
    assert!(app.app_state().lsp.canvas_graph.graph.is_some());
    app.input_mut()
        .events
        .push(egui::Event::PointerMoved(Pos2::new(900.0, 700.0)));
    app.run();
    assert!(app.app_state().lsp.canvas_graph.graph.is_none());
    app.input_mut().events.push(egui::Event::PointerMoved(at));
    for _ in 0..4 {
        app.step();
    }
    settle(&mut app);
    assert!(app.app_state().lsp.canvas_graph.graph.is_some());
    app.app_state_mut()
        .node_store
        .get_file_mut(id)
        .unwrap()
        .content
        .push_str("\n// changed source\n");
    app.step();
    assert!(
        app.app_state().lsp.canvas_graph.graph.is_none(),
        "content changes invalidate the snapshot immediately"
    );
    for _ in 0..4 {
        app.step();
    }
    settle(&mut app);
    assert!(app.app_state().lsp.canvas_graph.graph.is_some());
    app.app_state_mut().node_store.file_nodes.remove(&id);
    app.run();
    assert!(app.app_state().lsp.canvas_graph.graph.is_none());
}

/// The app with `fx` loaded and the fake language server plugged in.
fn app_with_fake_lsp(fx: &Fixture) -> App {
    let mut app = app_with(fx);
    app.app_state_mut()
        .lsp
        .manager
        .set_launcher(fake_lsp::launcher());
    app
}

/// Opens `rel`, selects it with a click on the canvas and opens the LSP tab.
fn select_and_open_lsp(app: &mut App, fx: &Fixture, rel: &str) -> u64 {
    let id = app.open(&fx.path(rel));
    let center = app.node_center(id);
    app.show_at(center, CANVAS_SPOT);
    click_at(app, CANVAS_SPOT, Modifiers::NONE);
    assert_eq!(app.selected(), [id]);

    app.app_state_mut().right_panel = Some(RightPanel::Layout);
    app.run();
    app.get_by_label("LSP Symbols").click();
    settle(app);
    assert_eq!(app.app_state().right_panel, Some(RightPanel::LspSymbols));
    id
}

/// Steps frames until the LSP tab has no answer on its way. Answers come from
/// a server thread, and the tab repaints while it waits, so `run()` alone
/// would give up after its 4 frames. A click can start a new request a frame
/// later (the tab follows the new selection), so wait for two quiet frames.
fn settle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut quiet = 0;
    while quiet < 2 {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the language server"
        );
        app.step();
        if app.app_state().lsp.is_waiting() {
            quiet = 0;
            std::thread::sleep(Duration::from_millis(10));
        } else {
            quiet += 1;
        }
    }
    app.run();
}

#[test]
fn lsp_tab_asks_for_a_selection() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    app.app_state_mut().right_panel = Some(RightPanel::LspSymbols);
    app.run();

    app.get_by_label("Select a file node to see its symbols.");
}

#[test]
fn files_without_a_server_say_so() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    select_and_open_lsp(&mut app, &fx, "README.md");

    app.get_by_label("No language server for Markdown.");
}

#[test]
fn lsp_tab_lists_the_symbols_of_the_selected_file() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    let id = select_and_open_lsp(&mut app, &fx, "src/main.rs");

    app.get_by_label("fn main  L3");
    app.get_by_label("Click a symbol to find its references.");

    app.get_by_label("fn main  L3").hover();
    app.run();
    assert_eq!(
        app.app_state().lsp.hovered,
        Some(LineHighlight {
            node_id: id,
            start_line: 2,
            end_line: 2
        })
    );
    app.get_by_label("Layout").click();
    app.run();
    assert_eq!(
        app.app_state().lsp.hovered,
        None,
        "other tabs drop the hover"
    );
}

#[test]
fn clicking_a_symbol_lists_references_across_files_and_a_reference_focuses_its_line() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    // This test opens main.rs by clicking a reference, not automatically.
    app.app_state_mut().settings.lsp.auto_open_matches = false;
    let util = select_and_open_lsp(&mut app, &fx, "src/util.rs");

    app.get_by_label("fn hello  L1").click();
    settle(&mut app);
    assert_eq!(
        app.app_state().selection.symbols().len(),
        1,
        "LSP rows share canvas symbol focus"
    );
    app.get_by_label("References to hello");
    app.get_by_label("2 references in 2 files");
    app.get_by_label("src/main.rs");
    app.get_by_label("src/util.rs");
    app.get_by_label("L1  pub fn hello() {}");

    // main.rs is not on the canvas yet, and the view looks elsewhere.
    assert_eq!(app.app_state().node_store.file_nodes.len(), 1);
    app.show_at(Pos2::new(-5000.0, -5000.0), CANVAS_SPOT);

    app.get_by_label("L4  util::hello();").click();
    settle(&mut app);

    let s = app.app_state();
    assert_eq!(s.node_store.file_nodes.len(), 2, "main.rs was opened");
    let main = s.selection.primary.unwrap();
    assert_ne!(main, util);
    assert!(s
        .node_store
        .get_file(main)
        .unwrap()
        .path
        .ends_with("src/main.rs"));
    assert_eq!(
        s.lsp.target,
        Some(LineHighlight {
            node_id: main,
            start_line: 3,
            end_line: 3
        })
    );
    assert!(s.pending_focus.is_none(), "the canvas applied the focus");

    // Line 4 of main.rs is now in the middle of the canvas.
    let node = s.node_store.get_file(main).unwrap();
    let rect = node.world_rect(s.settings.node_width, s.settings.font_size);
    let line_y =
        rect.top() + HEADER_HEIGHT + CODE_PADDING + 3.5 * line_height(s.settings.font_size);
    let on_screen = s
        .viewport
        .world_to_screen(Pos2::new(rect.center().x, line_y));
    assert!(
        (300.0..1100.0).contains(&on_screen.x) && (300.0..600.0).contains(&on_screen.y),
        "line shown near the canvas center: {on_screen:?}"
    );

    // The references stay listed, the target line is painted on the node.
    app.get_by_label("References to hello");
    let color = s.theme.selection.gamma_multiply(0.4);
    let painted = app
        .output()
        .shapes
        .iter()
        .any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill == color));
    assert!(painted, "target line highlighted");

    // Jumping back reuses the open util.rs node.
    app.get_by_label("L1  pub fn hello() {}").click();
    settle(&mut app);
    assert_eq!(app.app_state().selection.primary, Some(util));
    assert_eq!(app.app_state().node_store.file_nodes.len(), 2);
}

#[test]
fn a_missing_server_explains_how_to_configure_it() {
    let fx = fixture();
    let mut app = app_with(&fx);
    // What the default launcher reports when nothing is on PATH.
    app.app_state_mut()
        .lsp
        .manager
        .set_launcher(Arc::new(|server, _| {
            Err(resolve_command(server, |_| None, |_| false).unwrap_err())
        }));
    select_and_open_lsp(&mut app, &fx, "src/main.rs");

    app.get_by_label_contains("No Rust language server found on PATH (tried rust-analyzer)");
}

#[test]
fn scrolled_lsp_rows_keep_correct_hover_and_pointer_click_bounds() {
    let fx = fixture();
    let content: String = (0..35)
        .map(|i| format!("pub fn symbol_{i}() {{}}\n"))
        .collect();
    std::fs::write(fx.path("src/util.rs"), content).unwrap();
    let mut app = app_with_fake_lsp(&fx);
    let node = app.open(&fx.path("src/util.rs"));
    app.set_size(egui::vec2(1400.0, 400.0));
    app.app_state_mut().left_panel = None;
    app.app_state_mut().selection.select(node);
    app.app_state_mut().right_panel = Some(RightPanel::LspSymbols);
    settle(&mut app);

    wheel(
        &mut app,
        Pos2::new(1200.0, 250.0),
        egui::vec2(0.0, -10000.0),
        Modifiers::NONE,
    );
    let label = "fn symbol_34  L35";
    let bounds = app.get_by_label(label).raw_bounds().unwrap();
    assert!(
        bounds.y0 > 60.0 && bounds.y1 < 400.0,
        "last symbol is visible: {bounds:?}"
    );
    app.get_by_label(label).hover();
    app.run();
    assert_eq!(
        app.app_state().lsp.hovered,
        Some(LineHighlight {
            node_id: node,
            start_line: 34,
            end_line: 34,
        })
    );

    let center = Pos2::new(
        ((bounds.x0 + bounds.x1) * 0.5) as f32,
        ((bounds.y0 + bounds.y1) * 0.5) as f32,
    );
    click_at(&mut app, center, Modifiers::NONE);
    settle(&mut app);
    assert_eq!(
        app.app_state().lsp.references.as_ref().unwrap().symbol,
        "symbol_34"
    );
}

#[test]
fn auto_open_matches_opens_the_reference_files_up_to_max_files() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    select_and_open_lsp(&mut app, &fx, "src/util.rs");
    app.get_by_label("fn hello  L1").click();
    settle(&mut app);
    app.settle_edges();
    let names: Vec<String> = app
        .app_state()
        .node_store
        .file_nodes
        .values()
        .map(|n| n.file_name().to_owned())
        .collect();
    assert!(
        names.contains(&"main.rs".to_owned()),
        "main.rs opened by Auto Open Matches: {names:?}"
    );
}

#[test]
fn symbol_graph_history_and_clear_symbols() {
    let fx = fixture();
    std::fs::write(
        fx.path("src/util.rs"),
        "pub fn hello() {}\npub fn bye() {}\n",
    )
    .unwrap();
    let mut app = app_with_fake_lsp(&fx);
    app.app_state_mut().settings.lsp.auto_open_matches = false;
    select_and_open_lsp(&mut app, &fx, "src/util.rs");
    app.get_by_label_contains("Click a symbol to show LSP graph results.");
    assert!(app.get_by_label("Previous symbol").is_disabled());
    assert!(app.get_by_label("Clear Symbols").is_disabled());

    app.get_by_label("fn hello  L1").click();
    settle(&mut app);
    app.get_by_label("fn bye  L2").click();
    settle(&mut app);
    app.get_by_label("References to bye");

    app.get_by_label("Previous symbol").click();
    settle(&mut app);
    app.get_by_label("References to hello");
    app.get_by_label("Next symbol").click();
    settle(&mut app);
    app.get_by_label("References to bye");

    app.get_by_label("Clear Symbols").click();
    app.run();
    assert!(app.app_state().lsp.references.is_none());
    assert!(app.app_state().selection.symbols().is_empty());
    assert!(
        !app.app_state().selection.selected.is_empty(),
        "the file stays selected"
    );
}

#[test]
fn advanced_settings_card_opens_from_the_gear_and_edits_lsp_settings() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    select_and_open_lsp(&mut app, &fx, "src/util.rs");
    assert!(app.query_by_label("LSP Advanced Settings").is_none());
    app.get_by_label("Show advanced settings").click();
    app.run();
    app.get_by_label("LSP Advanced Settings");
    for label in [
        "Query On Hover",
        "Auto Open Matches",
        "Keep Zoom",
        "Smooth Transition",
        "Dim Dependency Edges",
        "Reveal Through Collapse",
        "Out of View",
    ] {
        app.get_by_label(label).click();
        app.run();
    }
    let lsp = &app.app_state().settings.lsp;
    assert!(
        lsp.query_on_hover && !lsp.auto_open_matches && lsp.keep_zoom && !lsp.smooth_transition
    );
    assert!(!lsp.dim_edges && lsp.reveal_through_collapse && lsp.out_of_view);
    app.get_by_label("Close LSP Advanced Settings").click();
    app.run();
    assert!(app.query_by_label("LSP Advanced Settings").is_none());
}

#[test]
fn query_on_hover_asks_for_references_after_resting_on_a_symbol() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    app.app_state_mut().settings.lsp.query_on_hover = true;
    app.app_state_mut().settings.lsp.auto_open_matches = false;
    select_and_open_lsp(&mut app, &fx, "src/util.rs");
    app.get_by_label("fn hello  L1").hover();
    for _ in 0..4 {
        app.step();
    }
    settle(&mut app);
    app.get_by_label("References to hello");
    assert!(
        app.app_state().selection.symbols().is_empty(),
        "hovering does not select"
    );
}

#[test]
fn language_servers_dialog_lists_servers_and_file_counts() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    select_and_open_lsp(&mut app, &fx, "src/util.rs");
    app.get_by_label("LSP menu").click();
    app.run();
    app.get_by_label("Language servers").click();
    app.run();
    app.get_by_label("Repository files");
    app.get_by_label("Rust");
    app.get_by_label("TypeScript");
    assert!(app.get_by_label("Load project environment").is_disabled());
    app.get_by_label("Canvas").click();
    app.run();
    app.get_by_label("Canvas files");
    app.get_by_label("1");
}

#[test]
fn keep_zoom_and_out_of_view_markers() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    {
        let lsp = &mut app.app_state_mut().settings.lsp;
        lsp.keep_zoom = true;
        lsp.out_of_view = true;
    }
    select_and_open_lsp(&mut app, &fx, "src/util.rs");
    app.get_by_label("fn hello  L1").click();
    settle(&mut app);
    app.settle_edges();
    {
        let s = app.app_state_mut();
        s.viewport.zoom = 0.3;
        s.viewport.target_zoom = 0.3;
    }
    app.show_at(Pos2::new(-20_000.0, -20_000.0), CANVAS_SPOT);
    let canvas = egui::Rect::from_min_max(Pos2::new(40.0, 30.0), Pos2::new(1060.0, 900.0));
    let markers = spatialcoding::ui::lsp_panel::out_of_view_markers(app.app_state(), canvas);
    assert_eq!(
        markers.len(),
        2,
        "util.rs and main.rs are off screen: {markers:?}"
    );

    app.get_by_label("L4  util::hello();").click();
    settle(&mut app);
    assert_eq!(app.app_state().viewport.zoom, 0.3, "Keep Zoom");
}
