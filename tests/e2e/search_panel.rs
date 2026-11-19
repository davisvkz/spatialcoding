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

/// Switches to Files & folders and refocuses the field.
fn files_mode(app: &mut App) {
    app.get_by_label("Files & folders").click();
    app.run();
    app.get_by_role(egui::accesskit::Role::TextInput).focus();
    app.run();
}

#[test]
fn file_search_matches_loaded_file_names_and_a_click_focuses_the_node() {
    let fx = fixture();
    let mut app = app_with(&fx);
    open_search(&mut app);
    files_mode(&mut app);
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
    files_mode(&mut app);
    type_text(&mut app, "zzzz");
    assert!(app.query_by_label("src/util.rs").is_none());
    app.get_by_label_contains("No results found");
}

#[test]
fn content_search_finds_text_inside_files_and_focuses_its_line() {
    let fx = fixture();
    let mut app = app_with(&fx);
    open_search(&mut app);
    type_text_stepping(&mut app, "console.log");
    wait_for(&mut app, |a| a.query_by_label_contains("web/app.ts:2").is_some());
    app.get_by_label_contains("web/app.ts:2").click();
    app.run();
    let state = app.app_state();
    let node = state.node_store.file_nodes.values().find(|n| n.path == fx.path("web/app.ts")).unwrap();
    assert_eq!(state.lsp.target.as_ref().map(|t| (t.node_id, t.start_line)), Some((node.id, 1)));
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

#[test]
fn files_and_folders_mode_finds_folders_and_a_click_opens_them() {
    let fx = fixture();
    let mut app = app_with(&fx);
    open_search(&mut app);
    files_mode(&mut app);
    type_text(&mut app, "web");
    app.get_by_label("web/").click();
    app.run();
    let opened: Vec<_> = app.app_state().node_store.file_nodes.values().map(|n| n.path.clone()).collect();
    assert!(opened.contains(&fx.path("web/app.ts")), "the folder's files are on the canvas: {opened:?}");
}

#[test]
fn tab_switches_between_code_and_files_and_folders() {
    let fx = fixture();
    let mut app = app_with(&fx);
    open_search(&mut app);
    assert_eq!(app.app_state().search.mode, spatialcoding::ui::search_panel::SearchMode::Code);
    app.press_key(egui::Key::Tab);
    app.run();
    assert_eq!(app.app_state().search.mode, spatialcoding::ui::search_panel::SearchMode::Files);
    app.press_key(egui::Key::Tab);
    app.run();
    assert_eq!(app.app_state().search.mode, spatialcoding::ui::search_panel::SearchMode::Code);
}

#[test]
fn the_filters_change_what_code_search_finds() {
    let fx = fixture();
    let mut app = app_with(&fx);
    open_search(&mut app);
    type_text_stepping(&mut app, "CONSOLE");
    wait_for(&mut app, |a| !a.app_state().search.content_hits().is_empty());
    let found = |app: &App| app.app_state().search.content_hits().len();
    assert_eq!(found(&app), 1, "case-insensitive by default");

    app.get_by_label("Aa").click();
    wait_for(&mut app, |a| a.app_state().search.content_hits().is_empty());

    app.get_by_label("Aa").click();
    app.get_by_label(".*").click();
    app.app_state_mut().search.query = "consol.\\.log".into();
    wait_for(&mut app, |a| a.app_state().search.content_hits().len() == 1);

    app.app_state_mut().search.query = "consol(".into();
    wait_for(&mut app, |a| a.app_state().search.error.is_some());
    app.get_by_label_contains("Invalid pattern");

    app.get_by_label(".*").click();
    app.get_by_label("Word").click();
    app.app_state_mut().search.query = "consol".into();
    wait_for(&mut app, |a| a.app_state().search.content_hits().is_empty());
}

#[test]
fn canvas_and_ignored_filters_and_the_on_off_canvas_groups() {
    let fx = fixture();
    std::fs::create_dir_all(fx.path("dist")).unwrap();
    std::fs::write(fx.path("dist/bundle.js"), "needle in a bundle\n").unwrap();
    std::fs::write(fx.path("src/util.rs"), "pub fn hello() {} // needle\n").unwrap();
    std::fs::write(fx.path("src/main.rs"), "fn main() { /* needle */ }\n").unwrap();
    let mut app = app_with(&fx);
    app.state_mut().state_mut().add_ignore_pattern("/dist/".into());
    app.app_state_mut().folder_files_cache.replace(None);
    app.open(&fx.path("src/main.rs"));
    open_search(&mut app);
    type_text_stepping(&mut app, "needle");
    wait_for(&mut app, |a| a.app_state().search.content_hits().len() == 2);
    app.get_by_label_contains("On Canvas (1)");
    app.get_by_label_contains("Off Canvas (1)");

    app.get_by_label("Ignored").click();
    app.app_state_mut().search.query = "needle ".into();
    wait_for(&mut app, |a| a.app_state().search.content_hits().len() == 3);

    app.get_by_label("Canvas").click();
    app.app_state_mut().search.query = "needle".into();
    wait_for(&mut app, |a| a.app_state().search.content_hits().len() == 1);
    assert!(app.query_by_label_contains("Off Canvas").is_none());
}

#[test]
fn arrows_move_the_active_result_and_enter_opens_it() {
    let fx = fixture();
    std::fs::write(fx.path("src/util.rs"), "pub fn hello() {} // needle\n").unwrap();
    std::fs::write(fx.path("src/main.rs"), "fn main() { /* needle */ }\n").unwrap();
    let mut app = app_with(&fx);
    open_search(&mut app);
    type_text_stepping(&mut app, "needle");
    wait_for(&mut app, |a| a.app_state().search.content_hits().len() == 2);
    assert_eq!(app.app_state().search.active, 0);
    app.press_key(egui::Key::ArrowDown);
    app.run();
    assert_eq!(app.app_state().search.active, 1);
    app.press_key(egui::Key::ArrowUp);
    app.run();
    assert_eq!(app.app_state().search.active, 0);
    app.press_key(egui::Key::ArrowDown);
    app.run();
    app.press_key(egui::Key::Enter);
    app.run();
    let second = &app.app_state().search.content_hits()[1].path.clone();
    assert!(app.app_state().node_store.file_nodes.values().any(|n| &n.path == second), "Enter opened the active result");
}

#[test]
fn keep_zoom_and_smooth_transition_live_in_the_search_panel() {
    let fx = fixture();
    let mut app = app_with(&fx);
    open_search(&mut app);
    let before = app.app_state().settings.lsp.keep_zoom;
    app.get_by_label("Keep Zoom").click();
    app.run();
    assert_eq!(app.app_state().settings.lsp.keep_zoom, !before);
    let smooth = app.app_state().settings.lsp.smooth_transition;
    app.get_by_label("Smooth Transition").click();
    app.run();
    assert_eq!(app.app_state().settings.lsp.smooth_transition, !smooth);
}
