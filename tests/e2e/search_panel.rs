use egui_kittest::kittest::Queryable;

use crate::common::*;

/// Opens the Search panel and focuses its field.
fn open_search(app: &mut App) {
    app.get_by_label("Search").click();
    app.run();
    app.get_by_role(egui::accesskit::Role::TextInput).focus();
    app.run();
}

/// Steps frames until `done` holds, while the worker thread answers.
fn wait_for(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !done(app) {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for search results");
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.step();
    }
    app.run();
}

#[test]
fn file_search_matches_loaded_file_names_and_a_click_focuses_the_node() {
    let fx = fixture();
    let mut app = app_with(&fx);
    open_search(&mut app);
    type_text(&mut app, "util");
    app.get_by_label("src/util.rs").click();
    app.run();
    let state = app.app_state();
    let node = state.node_store.file_nodes.values().find(|n| n.path == fx.path("src/util.rs"));
    assert!(node.is_some(), "the clicked file is opened on the canvas");
    assert_eq!(state.selection.primary, node.map(|n| n.id));
}

#[test]
fn file_search_ignores_files_that_do_not_match() {
    let fx = fixture();
    let mut app = app_with(&fx);
    open_search(&mut app);
    type_text(&mut app, "zzzz");
    assert!(app.query_by_label("src/util.rs").is_none());
    app.get_by_label_contains("No files found");
}

#[test]
fn content_search_finds_text_inside_files_and_focuses_its_line() {
    let fx = fixture();
    let mut app = app_with(&fx);
    open_search(&mut app);
    app.get_by_label("Content").click();
    app.run();
    app.get_by_role(egui::accesskit::Role::TextInput).focus();
    type_text(&mut app, "console.log");
    wait_for(&mut app, |a| a.query_by_label_contains("web/app.ts:2").is_some());
    app.get_by_label_contains("web/app.ts:2").click();
    app.run();
    let state = app.app_state();
    let node = state.node_store.file_nodes.values().find(|n| n.path == fx.path("web/app.ts")).unwrap();
    assert_eq!(state.lsp.target.as_ref().map(|t| (t.node_id, t.start_line)), Some((node.id, 1)));
}

#[test]
fn symbol_search_lists_declarations_by_name() {
    let fx = fixture();
    let mut app = app_with(&fx);
    open_search(&mut app);
    app.get_by_label("Symbols").click();
    app.run();
    app.get_by_role(egui::accesskit::Role::TextInput).focus();
    type_text(&mut app, "hello");
    wait_for(&mut app, |a| a.query_by_label("hello").is_some());
    app.get_by_label("hello").click();
    app.run();
    let state = app.app_state();
    assert!(state.node_store.file_nodes.values().any(|n| n.path == fx.path("src/util.rs")));
}

#[test]
fn typing_a_query_does_not_fit_the_view_or_clear_the_canvas() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.open(&fx.path("src/main.rs"));
    open_search(&mut app);
    // Shift+X clears the canvas when nothing is being typed.
    type_text(&mut app, "f1");
    app.press_key_modifiers(egui::Modifiers::SHIFT, egui::Key::X);
    app.run();
    assert_eq!(app.app_state().node_store.file_nodes.len(), 1);
    assert_eq!(app.app_state().search.query, "f1");
}
