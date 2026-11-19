//! Git branch in the toolbar, the Changes panel and the diff window.

use std::process::Command;

use egui_kittest::kittest::Queryable;

use crate::common::*;

/// The demo fixture as a Git repository on branch `trunk`, committed, then
/// edited: `src/main.rs` modified, `src/new.rs` untracked, `src/util.rs`
/// deleted.
fn git_fixture() -> Fixture {
    let fx = fixture();
    let git = |args: &[&str]| {
        let out = Command::new("git").arg("-C").arg(&fx.root).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q", "-b", "trunk"]);
    git(&["add", "Cargo.toml", "README.md", "src", "web"]);
    git(&["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-qm", "init"]);
    std::fs::write(fx.path("src/main.rs"), "mod util;\n\nfn main() {\n    util::hello();\n    changed_line();\n}\n").unwrap();
    std::fs::write(fx.path("src/new.rs"), "pub fn fresh() {}\n").unwrap();
    std::fs::remove_file(fx.path("src/util.rs")).unwrap();
    fx
}

/// Steps frames until `done`, while the Git workers answer.
fn wait_for(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !done(app) {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for Git");
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.step();
    }
    app.run();
}

fn app_with_git_status(fx: &Fixture) -> App {
    let mut app = app_with(fx);
    wait_for(&mut app, |a| a.app_state().changes.is_known());
    app
}

#[test]
fn the_toolbar_shows_the_current_branch() {
    let fx = git_fixture();
    let app = app_with_git_status(&fx);
    app.get_by_label("Branch trunk");
}

#[test]
fn outside_a_repository_there_is_no_branch_and_the_panel_says_so() {
    let fx = fixture();
    let mut app = app_with(&fx);
    wait_for(&mut app, |a| a.app_state().changes.status != spatialcoding::git::changes::ChangesStatus::Loading
        && a.app_state().changes.status != spatialcoding::git::changes::ChangesStatus::Idle);
    // The temp dir can sit inside a repository; only check when it does not.
    if matches!(app.app_state().changes.status, spatialcoding::git::changes::ChangesStatus::Unavailable(_)) {
        assert!(app.query_by_label_contains("Branch ").is_none());
        app.get_by_label("Changes").click();
        app.run();
        app.get_by_label_contains("Git is unavailable here");
    }
}

#[test]
fn the_changes_panel_lists_changed_files_by_kind() {
    let fx = git_fixture();
    let mut app = app_with_git_status(&fx);
    app.get_by_label("Changes").click();
    app.run();
    app.get_by_label("src/main.rs");
    app.get_by_label("src/new.rs");
    app.get_by_label("src/util.rs");
    app.get_by_label("Modified (1)");
    app.get_by_label_contains("Untracked (");
    app.get_by_label("Deleted (1)");
    app.get_by_label_contains("On branch trunk");
    assert!(app.query_by_label("web/a.ts").is_none(), "unchanged files are not listed");
}

#[test]
fn a_clean_tree_has_nothing_to_list() {
    let fx = git_fixture();
    let git = |args: &[&str]| {
        assert!(Command::new("git").arg("-C").arg(&fx.root).args(args).output().unwrap().status.success());
    };
    git(&["add", "-A"]);
    git(&["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-qm", "all"]);
    let mut app = app_with_git_status(&fx);
    app.get_by_label("Changes").click();
    app.run();
    app.get_by_label_contains("No changes");
}

#[test]
fn clicking_a_changed_file_shows_its_diff_inline() {
    let fx = git_fixture();
    let mut app = app_with_git_status(&fx);
    app.get_by_label("Changes").click();
    app.run();
    app.get_by_label("src/main.rs").click();
    app.step();
    wait_for(&mut app, |a| {
        matches!(a.app_state().diff.as_ref().map(|d| &d.loader.status), Some(spatialcoding::git::diff::DiffStatus::Ready(_)))
    });
    app.get_by_label("Diff — src/main.rs");
    app.get_by_label("+     changed_line();");
    app.get_by_label("Modified file");
    app.get_by_label("+1");
}

#[test]
fn the_split_view_pairs_old_and_new_lines_side_by_side() {
    let fx = git_fixture();
    let mut app = app_with_git_status(&fx);
    spatialcoding::workspace::open_diff(app.app_state_mut(), fx.path("src/util.rs"));
    wait_for(&mut app, |a| {
        matches!(a.app_state().diff.as_ref().map(|d| &d.loader.status), Some(spatialcoding::git::diff::DiffStatus::Ready(_)))
    });
    // A deleted file: every line removed, nothing on the right.
    app.get_by_label("Deleted file");
    app.get_by_label("- pub fn hello() {}");
    app.get_by_label("Split").click();
    app.run();
    app.get_by_label("- pub fn hello() {} | ");
}

#[test]
fn the_diff_window_closes_and_the_palette_opens_it_for_the_selection() {
    let fx = git_fixture();
    let mut app = app_with_git_status(&fx);
    let main = app.open(&fx.path("src/main.rs"));
    app.app_state_mut().selection.select(main);
    app.execute_palette_command(spatialcoding::ui::command_palette::PaletteCommand::Command("diff:openLocalSession"));
    app.step();
    assert!(app.app_state().diff.is_some());
    wait_for(&mut app, |a| {
        matches!(a.app_state().diff.as_ref().map(|d| &d.loader.status), Some(spatialcoding::git::diff::DiffStatus::Ready(_)))
    });
    app.get_by_label("Diff — src/main.rs");
    app.app_state_mut().diff = None;
    app.run();
    assert!(app.query_by_label_contains("Diff — ").is_none());
}
