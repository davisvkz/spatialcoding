//! File nodes panel: its ten canvas actions, top-level symbol highlights,
//! long-file collapse and file-name labels.

use egui_kittest::kittest::Queryable;
use spatialcoding::state::RightPanel;

use crate::common::*;

fn files_panel(fx: &Fixture) -> App {
    let mut app = app_with(fx);
    app.app_state_mut().left_panel = None;
    app.app_state_mut().right_panel = Some(RightPanel::FileNodes);
    app.run();
    app
}

fn open_paths(app: &App) -> Vec<String> {
    let s = app.app_state();
    let mut names: Vec<String> = s
        .node_store
        .file_nodes
        .values()
        .map(|n| {
            n.path
                .strip_prefix(s.loaded_dir.as_ref().unwrap())
                .unwrap()
                .display()
                .to_string()
        })
        .collect();
    names.sort();
    names
}

fn git(fx: &Fixture, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&fx.root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Steps until the background `git status` answered.
fn settle_changes(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.app_state().changes.is_known() {
        assert!(
            std::time::Instant::now() < deadline,
            "git status never answered"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.step();
    }
    app.run();
}

#[test]
fn changed_file_actions_open_and_close_by_git_status() {
    let fx = fixture();
    git(&fx, &["init", "-q"]);
    git(&fx, &["add", "."]);
    git(
        &fx,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "-qm",
            "init",
        ],
    );
    std::fs::write(fx.path("src/util.rs"), "pub fn hello() { /* edited */ }\n").unwrap();
    let mut app = files_panel(&fx);
    settle_changes(&mut app);
    app.open(&fx.path("src/main.rs"));
    app.run();

    app.get_by_label("Open all changed files").click();
    app.settle_edges();
    assert_eq!(open_paths(&app), vec!["src/main.rs", "src/util.rs"]);
    assert!(
        app.get_by_label("Open all changed files").is_disabled(),
        "nothing left to open"
    );

    app.get_by_label("Close files without changes").click();
    app.settle_edges();
    assert_eq!(open_paths(&app), vec!["src/util.rs"]);
    app.get_by_label_contains("Changed (Git)");
}

#[test]
fn import_actions_follow_the_selection_both_ways() {
    let fx = fixture();
    let mut app = files_panel(&fx);
    assert!(app
        .get_by_label("Open files that import the selection")
        .is_disabled());
    let a = app.open(&fx.path("web/a.ts"));
    app.app_state_mut().selection.select(a);
    app.run();
    app.get_by_label("Open files that import the selection")
        .click();
    app.settle_edges();
    assert_eq!(open_paths(&app), vec!["web/a.ts", "web/app.ts"]);

    // The toolbar has a button of the same name; the panel's comes last.
    app.get_all_by_label("Clear all nodes")
        .last()
        .unwrap()
        .click();
    app.run();
    assert!(app.app_state().node_store.file_nodes.is_empty());
    let importer = app.open(&fx.path("web/app.ts"));
    app.app_state_mut().selection.select(importer);
    app.run();
    app.get_by_label("Open imported files").click();
    app.settle_edges();
    assert_eq!(open_paths(&app), vec!["web/a.ts", "web/app.ts"]);
}

#[test]
fn type_script_alias_opens_its_target_with_tsconfig_closed() {
    let fx = fixture();
    std::fs::write(
        fx.path("tsconfig.json"),
        r#"{"compilerOptions":{"paths":{"@web/*":["./web/*"]}}}"#,
    )
    .unwrap();
    std::fs::write(fx.path("web/app.ts"), "import { a } from '@web/a';\n").unwrap();
    let mut app = files_panel(&fx);
    app.app_state_mut()
        .lsp
        .manager
        .set_launcher(std::sync::Arc::new(|_, _| Err("offline".into())));
    let importer = app.open(&fx.path("web/app.ts"));
    app.app_state_mut().selection.select(importer);
    app.run();
    let action = app.get_by_label("Open imported files");
    assert!(!action.is_disabled());
    action.click();
    app.settle_edges();
    assert_eq!(open_paths(&app), vec!["web/a.ts", "web/app.ts"]);
    assert!(app
        .app_state()
        .edge_store
        .all_edges()
        .any(|edge| edge.relationship == spatialcoding::edges::style::RelationshipType::Imports));
}

#[test]
fn selection_actions_select_unselect_remove_and_fit() {
    let fx = fixture();
    let mut app = files_panel(&fx);
    assert!(app.get_by_label("Select all nodes").is_disabled());
    app.get_by_label("Open all files").click();
    app.settle_edges();
    let total = app.app_state().node_store.file_nodes.len();
    assert!(total >= 4, "opened the fixture's text files");

    app.get_by_label("Select all nodes").click();
    app.run();
    assert_eq!(app.selected().len(), total);
    app.get_by_label("Unselect all").click();
    app.run();
    assert!(app.selected().is_empty());

    let first = app.app_state().node_store.all_file_ids()[0];
    app.app_state_mut().selection.select(first);
    app.run();
    app.get_by_label("Remove selected nodes").click();
    app.run();
    assert_eq!(app.app_state().node_store.file_nodes.len(), total - 1);

    app.show_at(egui::Pos2::new(90_000.0, 90_000.0), CANVAS_SPOT);
    app.get_by_label("Fit to nodes").click();
    app.run();
    let s = app.app_state();
    let visible = s.viewport.visible_world_rect(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(1400.0, 900.0),
    ));
    for node in s.node_store.file_nodes.values() {
        assert!(visible.contains_rect(node.world_rect(s.settings.node_width, s.settings.font_size)));
    }
}

#[test]
fn top_level_symbols_highlight_the_selected_files_declarations() {
    let fx = fixture();
    let mut app = files_panel(&fx);
    let main = app.open(&fx.path("src/main.rs"));
    app.app_state_mut().selection.select(main);
    app.run();
    app.get_by_label("Highlight top-level symbols").click();
    app.run();
    // mod util; (line 0) is an item, not an import; fn main spans lines 2-4.
    assert_eq!(
        app.app_state().top_level_highlights[&main],
        vec![(0, 0), (2, 4)]
    );
    app.get_by_label("Highlight top-level symbols").click();
    app.run();
    assert!(app.app_state().top_level_highlights.is_empty());
}

#[test]
fn long_files_open_collapsed_until_expanded() {
    let fx = fixture();
    std::fs::write(fx.path("src/util.rs"), "// line\n".repeat(40)).unwrap();
    let mut app = files_panel(&fx);
    app.app_state_mut().settings.files.long_max_lines = 20;
    app.run();
    let util = app.open(&fx.path("src/util.rs"));
    assert!(app.app_state().node_store.get_file(util).unwrap().collapsed);
    app.get_by_label("Expand all 1 collapsed files").click();
    app.run();
    assert!(!app.app_state().node_store.get_file(util).unwrap().collapsed);
    assert!(app
        .get_by_label("Expand all 0 collapsed files")
        .is_disabled());
}

#[test]
fn file_names_switch_hides_the_labels_above_nodes() {
    let fx = fixture();
    let mut app = files_panel(&fx);
    let util = app.open(&fx.path("src/util.rs"));
    let center = app.node_center(util);
    app.show_at(center, CANVAS_SPOT);
    assert_eq!(
        texts_equal(&frame_shapes(&app), "util.rs"),
        1,
        "label drawn by default"
    );
    // "File Names" is both the section legend and its switch, as in v2.
    app.get_by_role_and_label(egui::accesskit::Role::CheckBox, "File Names")
        .click();
    app.run();
    assert_eq!(texts_equal(&frame_shapes(&app), "util.rs"), 0);
}

#[test]
fn open_importers_asks_the_server_for_the_references_of_the_exported_symbols() {
    let fx = fixture();
    // `hello()` here is not an import the syntax can follow: only the
    // server's references find it.
    std::fs::write(
        fx.path("src/other.rs"),
        "fn elsewhere() {\n    hello();\n}\n",
    )
    .unwrap();
    let mut app = files_panel(&fx);
    app.app_state_mut()
        .lsp
        .manager
        .set_launcher(crate::fake_lsp::launcher());
    let util = app.open(&fx.path("src/util.rs"));
    app.app_state_mut().selection.select(util);
    app.run();

    app.get_by_label("Open files that import the selection")
        .click();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.app_state().node_store.file_nodes.len() < 3 {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out: {:?}",
            open_paths(&app)
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.step();
    }
    assert_eq!(
        open_paths(&app),
        vec!["src/main.rs", "src/other.rs", "src/util.rs"]
    );
}

#[test]
fn file_actions_are_disabled_when_they_would_open_nothing() {
    let fx = fixture();
    let mut app = files_panel(&fx);
    let readme = app.open(&fx.path("README.md"));
    app.app_state_mut().selection.select(readme);
    app.run();
    // Markdown has no language server and nothing links to it.
    assert!(app
        .get_by_label("Open files that import the selection")
        .is_disabled());
    assert!(app.get_by_label("Open imported files").is_disabled());

    // util.rs imports nothing, but a server can look for its importers.
    let util = app.open(&fx.path("src/util.rs"));
    app.app_state_mut().selection.select(util);
    app.run();
    assert!(!app
        .get_by_label("Open files that import the selection")
        .is_disabled());
    assert!(app.get_by_label("Open imported files").is_disabled());
}
