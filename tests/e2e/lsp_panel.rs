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

/// The app with `fx` loaded and the fake language server plugged in.
fn app_with_fake_lsp(fx: &Fixture) -> App {
    let mut app = app_with(fx);
    app.app_state_mut().lsp.manager.set_launcher(fake_lsp::launcher());
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
    app.get_by_label("LSP").click();
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
        assert!(Instant::now() < deadline, "timed out waiting for the language server");
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
        Some(LineHighlight { node_id: id, start_line: 2, end_line: 2 })
    );
    app.get_by_label("Layout").click();
    app.run();
    assert_eq!(app.app_state().lsp.hovered, None, "other tabs drop the hover");
}

#[test]
fn clicking_a_symbol_lists_references_across_files_and_a_reference_focuses_its_line() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    let util = select_and_open_lsp(&mut app, &fx, "src/util.rs");

    app.get_by_label("fn hello  L1").click();
    settle(&mut app);
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
    assert!(s.node_store.get_file(main).unwrap().path.ends_with("src/main.rs"));
    assert_eq!(s.lsp.target, Some(LineHighlight { node_id: main, start_line: 3, end_line: 3 }));
    assert!(s.pending_focus.is_none(), "the canvas applied the focus");

    // Line 4 of main.rs is now in the middle of the canvas.
    let node = s.node_store.get_file(main).unwrap();
    let rect = node.world_rect(s.settings.node_width, s.settings.font_size);
    let line_y = rect.top() + HEADER_HEIGHT + CODE_PADDING + 3.5 * line_height(s.settings.font_size);
    let on_screen = s.viewport.world_to_screen(Pos2::new(rect.center().x, line_y));
    assert!(
        (300.0..1100.0).contains(&on_screen.x) && (300.0..600.0).contains(&on_screen.y),
        "line shown near the canvas center: {on_screen:?}"
    );

    // The references stay listed, the target line is painted on the node.
    app.get_by_label("References to hello");
    let color = s.theme.selection.gamma_multiply(0.4);
    let painted = app.output().shapes.iter().any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill == color));
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
    app.app_state_mut().lsp.manager.set_launcher(Arc::new(|server, _| {
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
