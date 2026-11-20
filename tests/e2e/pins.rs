//! Pinned symbol graphs: a canvas click asks the language server for the
//! whole graph and pins it.

use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use spatialcoding::analysis::lsp::graph::{Group, Role};
use spatialcoding::state::RightPanel;

use crate::canvas::{symbol_canvas, token_center};
use crate::common::*;

/// Steps frames until `done`, while the language server answers.
fn wait_for(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !done(app) {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for the language server: pins {:?}",
            app.app_state().lsp.pins.pins().iter().map(|p| (&p.name, &p.fetch)).collect::<Vec<_>>()
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.step();
    }
    // Answers keep arriving from the server threads while a test runs, so
    // `run()` (which gives up after a few frames) could trip over one.
    app.step();
    app.step();
}

fn ready(app: &App, count: usize) -> bool {
    let pins = app.app_state().lsp.pins.pins();
    pins.len() == count && pins.iter().all(|p| p.graph().is_some())
}

fn pinned(app: &mut App, token: &str, modifiers: Modifiers, count: usize) {
    let at = token_center(app, token);
    click_at(app, at, modifiers);
    wait_for(app, |a| ready(a, count));
}

#[test]
fn clicking_a_symbol_pins_its_graph_and_shows_the_tip() {
    let (mut app, _fx, _uses, defs) = symbol_canvas();
    pinned(&mut app, "greet", Modifiers::NONE, 1);

    let s = app.app_state();
    let pin = &s.lsp.pins.pins()[0];
    assert_eq!(pin.name, "greet");
    let graph = pin.graph().unwrap();
    let roles: Vec<Role> = graph.entries.iter().map(|e| e.role).collect();
    assert!(roles.contains(&Role::DefSel), "the definition: {roles:?}");
    assert!(roles.contains(&Role::Ref), "a reference: {roles:?}");
    let def = graph.group(Group::Definitions).into_iter().next().unwrap();
    assert!(s.node_store.get_file(defs).is_some_and(|n| n.path == def.path), "the definition is in a.ts");
    assert!(
        s.toasts.iter().any(|t| t.text == "Shift+click to pin multiple symbols. Shift+C to clear all."),
        "the first pin shows the tip"
    );
}

#[test]
fn shift_click_pins_several_and_shift_c_clears_them() {
    let (mut app, _fx, ..) = symbol_canvas();
    pinned(&mut app, "greet", Modifiers::NONE, 1);
    pinned(&mut app, "value", Modifiers::SHIFT, 2);
    let names: Vec<_> = app.app_state().lsp.pins.pins().iter().map(|p| p.name.clone()).collect();
    assert_eq!(names, ["greet", "value"]);

    // A plain click pins only the new symbol.
    pinned(&mut app, "Client", Modifiers::NONE, 1);
    assert_eq!(app.app_state().lsp.pins.pins()[0].name, "Client");

    pinned(&mut app, "value", Modifiers::SHIFT, 2);
    let first = app.app_state().lsp.pins.pins()[0].id;
    app.app_state_mut().lsp.pins.remove(first);
    assert_eq!(app.app_state().lsp.pins.len(), 1, "one pin removed alone");

    app.press_key_modifiers(Modifiers::SHIFT, Key::C);
    app.run();
    assert!(app.app_state().lsp.pins.is_empty(), "Shift+C clears every pin");
    assert!(app.app_state().selection.symbols().is_empty());
}

#[test]
fn shift_clicking_a_pinned_symbol_unpins_it() {
    let (mut app, _fx, ..) = symbol_canvas();
    pinned(&mut app, "greet", Modifiers::NONE, 1);
    pinned(&mut app, "value", Modifiers::SHIFT, 2);
    let at = token_center(&app, "value");
    click_at(&mut app, at, Modifiers::SHIFT);
    app.run();
    assert_eq!(app.app_state().lsp.pins.len(), 1);
    assert_eq!(app.app_state().lsp.pins.pins()[0].name, "greet");
}

#[test]
fn the_panel_lists_pins_files_counts_and_grouped_results() {
    let (mut app, _fx, ..) = symbol_canvas();
    pinned(&mut app, "greet", Modifiers::NONE, 1);
    app.app_state_mut().right_panel = Some(RightPanel::LspSymbols);
    wait_for(&mut app, |a| a.query_by_label_contains("Pinned Symbols (1)").is_some());

    app.get_by_label_contains("Pinned Symbols (1)");
    app.get_by_label_contains("Files (");
    app.get_by_label_contains("Results (");
    app.get_by_label_contains("Definitions (");
    app.get_by_label_contains("References (");
    app.get_by_label("Clear 1 Symbol").click();
    app.run();
    assert!(app.app_state().lsp.pins.is_empty());
}

#[test]
fn a_function_pins_its_callers_through_the_call_hierarchy() {
    let fx = fixture();
    std::fs::write(fx.path("web/a.ts"), "export function target() {}\n").unwrap();
    std::fs::write(fx.path("web/app.ts"), "function caller() {\n  target();\n}\n").unwrap();
    let mut app = app_with(&fx);
    let uses = app.open(&fx.path("web/app.ts"));
    let defs = app.open(&fx.path("web/a.ts"));
    {
        let s = app.app_state_mut();
        s.node_store.get_file_mut(defs).unwrap().position = egui::Pos2::ZERO;
        s.node_store.get_file_mut(uses).unwrap().position = egui::Pos2::new(900.0, 0.0);
        s.viewport.zoom = 1.0;
        s.viewport.target_zoom = 1.0;
        s.left_panel = None;
    }
    app.show_at(egui::Pos2::new(350.0, 40.0), CANVAS_SPOT);
    pinned(&mut app, "target", Modifiers::NONE, 1);

    let graph = app.app_state().lsp.pins.pins()[0].graph().unwrap().clone();
    let roles: Vec<Role> = graph.entries.iter().map(|e| e.role).collect();
    for role in [Role::InCaller, Role::InCallerSel, Role::InCallsite] {
        assert!(roles.contains(&role), "{role:?} in {roles:?}");
    }
    assert!(graph.group(Group::IncomingCalls).len() >= 3);
}

#[test]
fn a_file_without_a_language_server_says_so_and_pins_nothing() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let readme = app.open(&fx.path("README.md"));
    spatialcoding::ui::symbol_pins::pin(None, app.state_mut().state_mut(), readme, 0, 2, false);
    let s = app.app_state();
    assert!(s.lsp.pins.is_empty());
    assert!(s.toasts.iter().any(|t| t.text.starts_with("No language server")), "{:?}", s.toasts);
}

/// Alphas (0-255) of the filled rectangles drawn this frame.
fn fill_alphas(app: &App) -> Vec<u8> {
    frame_shapes(app)
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Rect(r) if r.fill.a() > 0 => Some(r.fill.a()),
            _ => None,
        })
        .collect()
}

#[test]
fn pinned_roles_are_painted_with_their_alphas() {
    let (mut app, _fx, ..) = symbol_canvas();
    pinned(&mut app, "greet", Modifiers::NONE, 1);
    let alphas = fill_alphas(&app);
    // Reference .28 -> 71, definition name .30 -> 77 (the declaration block is .16 -> 41),
    // the source token's white .35 -> 89.
    for (alpha, what) in [(71, "reference"), (77, "definition name"), (89, "source token")] {
        assert!(alphas.contains(&alpha), "{what} alpha {alpha} missing in {alphas:?}");
    }
}

#[test]
fn zoomed_out_levels_name_the_pinned_tokens_with_pills() {
    let (mut app, _fx, uses, _defs) = symbol_canvas();
    pinned(&mut app, "greet", Modifiers::NONE, 1);
    app.app_state_mut().viewport.zoom = 0.2; // L2: bars, no text
    app.app_state_mut().viewport.target_zoom = 0.2;
    let center = app.node_center(uses);
    app.show_at(center, CANVAS_SPOT);
    let has_pill = frame_shapes(&app).iter().any(|s| matches!(s, egui::Shape::Text(t) if t.galley.job.text == "greet"));
    assert!(has_pill, "a name pill labels the token when the code is not text");
}

#[test]
fn highlight_top_level_symbols_pins_each_declaration() {
    let (mut app, _fx, _uses, defs) = symbol_canvas();
    app.app_state_mut().selection.select(defs);
    app.app_state_mut().right_panel = Some(RightPanel::FileNodes);
    app.run();
    app.get_by_label("Highlight top-level symbols").click();
    wait_for(&mut app, |a| ready(a, 3));
    let names: Vec<_> = app.app_state().lsp.pins.pins().iter().map(|p| p.name.clone()).collect();
    assert_eq!(names, ["value", "greet", "Client"]);

    app.get_by_label("Highlight top-level symbols").click();
    app.run();
    assert!(app.app_state().lsp.pins.is_empty(), "clicking again clears them");
}

#[test]
fn dim_dependency_edges_outlasts_the_symbols_by_half_a_second() {
    let (mut app, _fx, ..) = symbol_canvas();
    pinned(&mut app, "greet", Modifiers::NONE, 1);
    let held_until = app.app_state().lsp.dim_until;
    assert!(held_until > 0.0, "the dim was held while a symbol was active");
    app.press_key_modifiers(Modifiers::SHIFT, Key::C);
    app.step();
    assert!(app.app_state().lsp.pins.is_empty());
    assert!(app.app_state().lsp.dim_until >= held_until, "the hold is not shortened");
    assert_eq!(spatialcoding::ui::lsp_panel::DIM_RESTORE_DELAY, 0.5);
}

/// Token edges of pinned symbols: cubic Béziers with a translucent stroke.
fn token_edges(app: &App) -> usize {
    frame_shapes(app)
        .iter()
        .filter(|s| matches!(s, egui::Shape::CubicBezier(b)
            if matches!(b.stroke.color, egui::epaint::ColorMode::Solid(c) if c.a() > 0 && c.a() < 200)))
        .count()
}

#[test]
fn edges_reach_pinned_tokens_in_files_outside_the_camera() {
    let (mut app, _fx, uses, defs) = symbol_canvas();
    pinned(&mut app, "greet", Modifiers::NONE, 1);
    // Zoom in on the call until the definition's file leaves the camera.
    app.app_state_mut().viewport.zoom = 2.0;
    app.app_state_mut().viewport.target_zoom = 2.0;
    let center = app.node_center(uses);
    app.show_at(center, CANVAS_SPOT);
    let s = app.app_state();
    let defs_rect = s.node_store.get_file(defs).unwrap().world_rect(s.settings.node_width, s.settings.font_size);
    assert!(
        s.viewport.world_to_screen_rect(defs_rect).max.x < 0.0,
        "the definition's file is off screen"
    );
    let with_pin = token_edges(&app);
    app.app_state_mut().lsp.pins.clear();
    app.run();
    let without = token_edges(&app);
    assert!(with_pin > without, "the pin's edge to the off-screen token is drawn: {with_pin} vs {without}");
}
