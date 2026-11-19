//! The per-repository ignore list: tree, Open All, search and the dialog.

use egui_kittest::kittest::Queryable;
use spatialcoding::commands::AppAction;

use crate::common::*;

fn expand_all(app: &mut App) {
    app.get_by_label("Expand all folders").click();
    app.run();
}

#[test]
fn an_ignored_folder_leaves_the_tree_open_all_and_the_file_list() {
    let fx = fixture();
    let mut app = app_with(&fx);
    expand_all(&mut app);
    app.get_by_label("app.ts");

    let state = app.state_mut().state_mut();
    assert!(state.add_ignore_pattern("/web/".into()));
    state.pending_app_actions.push(AppAction::ReloadTree);
    app.run();
    expand_all(&mut app);
    assert!(app.query_by_label("app.ts").is_none(), "web/ is gone from the tree");
    app.get_by_label("main.rs");

    spatialcoding::commands::run(app.state_mut().state_mut(), "canvas:openAll", &serde_json::Value::Null).unwrap();
    // Opening files starts background analysis that keeps repainting; a few
    // steps are enough to settle the canvas.
    app.step();
    app.step();
    let opened: Vec<String> = app
        .app_state()
        .node_store
        .file_nodes
        .values()
        .map(|n| n.path.strip_prefix(&fx.root).unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(opened.iter().any(|p| p == "src/main.rs"));
    assert!(!opened.iter().any(|p| p.starts_with("web/")), "Open All skips ignored folders: {opened:?}");
    let listed = spatialcoding::workspace::folder_text_files(app.app_state());
    assert!(listed.iter().all(|p| !p.starts_with(fx.root.join("web"))), "so does the palette and search list");
}

#[test]
fn show_ignored_files_brings_them_back_in_the_tree() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.state_mut().state_mut().add_ignore_pattern("*.ts".into());
    app.state_mut().state_mut().pending_app_actions.push(AppAction::ReloadTree);
    app.run();
    expand_all(&mut app);
    assert!(app.query_by_label("app.ts").is_none());

    app.get_by_label("Workspace menu").click();
    app.run();
    app.get_by_label("Show ignored files").click();
    app.run();
    expand_all(&mut app);
    app.get_by_label("app.ts");
}

#[test]
fn the_ignore_dialog_lists_adds_and_removes_patterns_per_repository() {
    let fx = fixture();
    let mut app = app_with(&fx);
    app.get_by_label("Workspace menu").click();
    app.run();
    app.get_by_label("Ignored file settings").click();
    app.run();
    app.get_by_label("Nothing is ignored.");

    app.get_by_role(egui::accesskit::Role::TextInput).focus();
    type_text(&mut app, "/assets/");
    app.get_by_label("Add").click();
    app.run();
    assert_eq!(app.app_state().ignore_patterns(), ["/assets/"]);
    app.get_by_label("/assets/");

    app.get_by_label("✕").click();
    app.run();
    assert!(app.app_state().ignore_patterns().is_empty());

    // The list belongs to the repository: another folder starts empty.
    app.state_mut().state_mut().add_ignore_pattern("/x/".into());
    let other = fixture();
    app.state_mut().open_folder(other.root.clone());
    app.run();
    assert!(app.app_state().ignore_patterns().is_empty());
}

#[test]
fn ignore_lists_persist_with_the_settings_file() {
    let mut settings = spatialcoding::settings_store::PersistedSettings::default();
    settings.app.ignored.insert("/repo".into(), vec!["/dist/".into()]);
    settings.app.show_ignored = true;
    let json = spatialcoding::settings_store::to_json(&settings);
    let back: spatialcoding::settings_store::PersistedSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(back.app.ignored["/repo"], ["/dist/"]);
    assert!(back.app.show_ignored);
}

fn suggest_server(fx: &Fixture, command: &str) {
    std::fs::create_dir_all(fx.path(".codecanvas")).unwrap();
    std::fs::write(
        fx.path(".codecanvas/settings.json"),
        format!(r#"{{"languageServers": {{"rust": {{"command": ["{command}", "--stdio"]}}}}}}"#),
    )
    .unwrap();
}

#[test]
fn a_repository_suggested_server_asks_once_and_remembers_the_answer() {
    use spatialcoding::analysis::lsp::{manager::server_command, project_config, ServerId};
    let fx = fixture();
    suggest_server(&fx, "my-ra-allow");
    let mut app = app_with(&fx);

    // Starting the Rust server finds the suggestion and queues the question
    // (the default command runs meanwhile).
    let _ = server_command(ServerId::Rust, &fx.root);
    app.run();
    app.get_by_label("Run a language server command?");
    app.get_by_label_contains("my-ra-allow --stdio");
    assert_eq!(project_config::command_for(ServerId::Rust, &fx.root), None, "not allowed yet");

    app.get_by_label("Allow").click();
    app.run();
    assert!(app.query_by_label("Run a language server command?").is_none());
    assert_eq!(
        project_config::command_for(ServerId::Rust, &fx.root),
        Some(vec!["my-ra-allow".to_string(), "--stdio".to_string()])
    );
    assert_eq!(app.app_state().settings.lsp_command_answers.len(), 1, "the answer is saved with the settings");

    // The exact command in this folder is not asked again.
    let _ = server_command(ServerId::Rust, &fx.root);
    app.run();
    assert!(app.query_by_label("Run a language server command?").is_none());
}

#[test]
fn never_for_this_command_keeps_the_default_server() {
    use spatialcoding::analysis::lsp::{manager::server_command, project_config, ServerId};
    let fx = fixture();
    suggest_server(&fx, "my-ra-deny");
    let mut app = app_with(&fx);
    let _ = server_command(ServerId::Rust, &fx.root);
    app.run();
    app.get_by_label("Never for this command").click();
    app.run();
    assert_eq!(project_config::command_for(ServerId::Rust, &fx.root), None);
    assert!(!app.app_state().settings.lsp_command_answers[0].allowed);
}
