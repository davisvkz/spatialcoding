//! Agent notes: markdown cards with `cc://` links that navigate the canvas.

use egui_kittest::kittest::Queryable;
use serde_json::json;
use spatialcoding::commands;
use spatialcoding::state::RightPanel;

use crate::common::*;

fn push(app: &mut App, title: &str, markdown: &str) {
    commands::run(
        app.state_mut().state_mut(),
        "agentNotes:push",
        &json!({"title": title, "markdown": markdown}),
    )
    .unwrap();
    app.run();
}

fn open_paths(app: &App) -> Vec<String> {
    let root = app.app_state().loaded_dir.clone().unwrap();
    let mut paths: Vec<String> = app
        .app_state()
        .node_store
        .file_nodes
        .values()
        .map(|n| n.path.strip_prefix(&root).unwrap().to_string_lossy().into_owned())
        .collect();
    paths.sort();
    paths
}

#[test]
fn pushing_a_note_opens_the_panel_and_renders_its_markdown() {
    let fx = fixture();
    let mut app = app_with(&fx);
    push(&mut app, "Tour", "# Start here\nThe entry is **main** and `util` helps.\n\n- first\n- second");
    assert_eq!(app.app_state().right_panel, Some(RightPanel::AgentNotes));
    app.get_by_label("Tour");
    app.get_by_label("Start here");
    app.get_by_label("first");
    app.get_by_label("main");
}

#[test]
fn clicking_a_cc_link_opens_the_file_and_hovering_only_previews() {
    let fx = fixture();
    let mut app = app_with(&fx);
    push(&mut app, "Tour", "Open [the helper](cc://file/src/util.rs) next.");
    assert!(open_paths(&app).is_empty(), "nothing opens until the link is clicked");

    app.get_by_label("the helper").hover();
    app.run();
    assert!(open_paths(&app).is_empty(), "hover previews only");

    app.get_by_label("the helper").click();
    app.run();
    assert_eq!(open_paths(&app), ["src/util.rs"]);
    assert_eq!(app.selected().len(), 1, "and it is selected");
}

#[test]
fn a_symbol_link_opens_the_file_and_pins_the_symbol() {
    let fx = fixture();
    let mut app = app_with(&fx);
    // The pin would otherwise open the files of its results (Auto Open Matches).
    app.app_state_mut().settings.lsp.auto_open_matches = false;
    push(&mut app, "Tour", "See [hello](cc://symbol/src/util.rs:1:8).");
    app.get_by_label("hello").click();
    app.step();
    app.step();
    assert_eq!(open_paths(&app), ["src/util.rs"]);
}

#[test]
fn open_files_opens_every_linked_file_and_folder_once() {
    let fx = fixture();
    let mut app = app_with(&fx);
    push(
        &mut app,
        "Tour",
        "[a](cc://file/src/main.rs) [b](cc://file/src/util.rs) [again](cc://file/src/main.rs) [lib](cc://folder/web)",
    );
    app.get_by_label("Open files").click();
    app.run();
    let paths = open_paths(&app);
    assert!(paths.contains(&"src/main.rs".to_string()) && paths.contains(&"src/util.rs".to_string()), "{paths:?}");
    assert!(paths.iter().any(|p| p.starts_with("web/")), "the folder link opened its files: {paths:?}");
    assert_eq!(paths.iter().filter(|p| *p == "src/main.rs").count(), 1, "never twice");
}

#[test]
fn notes_can_be_updated_opened_removed_and_cleared() {
    let fx = fixture();
    let mut app = app_with(&fx);
    push(&mut app, "One", "first");
    push(&mut app, "Two", "second");
    let s = app.state_mut().state_mut();
    commands::run(s, "agentNotes:updateNote", &json!({"id": 1, "title": "Uno", "markdown": "changed"})).unwrap();
    assert!(commands::run(s, "agentNotes:updateNote", &json!({"id": 9})).is_err());
    commands::run(s, "agentNotes:open", &json!({"id": 2})).unwrap();
    assert_eq!(s.notes.focused, Some(2));
    app.run();
    app.get_by_label("Uno");
    app.get_by_label("changed");

    app.get_all_by_label("✕").next().unwrap().click();
    app.run();
    assert_eq!(app.app_state().notes.all().len(), 1);

    app.get_by_label("Agent notes menu").click();
    app.run();
    app.get_by_label("Clear Agent notes").click();
    app.run();
    assert!(app.app_state().notes.is_empty());
    app.get_by_label_contains("No notes yet.");
}
