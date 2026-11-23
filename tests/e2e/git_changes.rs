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

/// `git_fixture` plus two more commits: `src/main.rs` edited, then
/// `src/util.rs` and `web/a.ts` changed. Returns the fixture; the working
/// tree is clean afterwards.
fn history_fixture() -> Fixture {
    let fx = fixture();
    let git = |args: &[&str]| {
        let out = Command::new("git").arg("-C").arg(&fx.root).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    let commit = |msg: &str| git(&["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-qam", msg]);
    git(&["init", "-q", "-b", "trunk"]);
    git(&["add", "Cargo.toml", "README.md", "src", "web"]);
    commit("init");
    std::fs::write(fx.path("src/main.rs"), "mod util;\n\nfn main() {\n    util::hello();\n    second();\n}\n").unwrap();
    commit("edit main");
    std::fs::write(fx.path("src/util.rs"), "pub fn hello() {}\npub fn extra() {}\n").unwrap();
    std::fs::write(fx.path("web/a.ts"), "export const a = 2;\n").unwrap();
    commit("edit util and a");
    fx
}

fn diff_ready(a: &App) -> bool {
    matches!(a.app_state().diff.as_ref().map(|d| &d.loader.status), Some(spatialcoding::git::diff::DiffStatus::Ready(_)))
}

#[test]
fn the_commits_source_lists_history_and_a_commit_shows_its_files_and_diff() {
    let fx = history_fixture();
    let mut app = app_with_git_status(&fx);
    app.get_by_label("Changes").click();
    app.run();
    app.get_by_label("Commits").click();
    wait_for(&mut app, |a| a.query_by_label_contains("edit util and a").is_some());
    app.get_by_label_contains("edit main");
    app.get_by_label_contains("init");
    app.get_by_label("Pull requests").is_disabled();

    app.get_by_label_contains("edit util and a").click();
    wait_for(&mut app, |a| a.query_by_label("src/util.rs").is_some());
    app.get_by_label("web/a.ts");
    assert!(app.query_by_label("src/main.rs").is_none(), "only this commit's files");

    app.get_by_label("src/util.rs").click();
    wait_for(&mut app, diff_ready);
    app.get_by_label_contains("Diff — src/util.rs (");
    app.get_by_label("+ pub fn extra() {}");

    app.get_by_label("Back to working tree").click();
    app.run();
    assert_eq!(app.app_state().changes_view.source, spatialcoding::ui::changes_panel::ChangesSource::Local);
}

#[test]
fn the_commit_range_source_shows_net_changes_between_two_commits() {
    let fx = history_fixture();
    let mut app = app_with_git_status(&fx);
    let hashes = {
        let repo = spatialcoding::git::repo::GitRepo::open(&fx.root).unwrap();
        spatialcoding::git::history::commits(&repo, 10).unwrap()
    };
    app.get_by_label("Changes").click();
    app.run();
    app.get_by_label("Commit range").click();
    wait_for(&mut app, |a| a.app_state().changes_view.commits().is_some_and(|c| c.value.is_some()));
    {
        let view = &mut app.app_state_mut().changes_view;
        view.range_from = Some(hashes[2].hash.clone()); // init
        view.range_to = Some(hashes[0].hash.clone()); // edit util and a
    }
    wait_for(&mut app, |a| a.query_by_label("src/main.rs").is_some());
    app.get_by_label("src/util.rs");
    app.get_by_label("web/a.ts");

    app.get_by_label("src/main.rs").click();
    wait_for(&mut app, diff_ready);
    app.get_by_label("+     second();");
}

#[test]
fn the_layout_switch_and_the_gear_shape_the_diff() {
    use spatialcoding::state::CharDiff;
    use spatialcoding::ui::diff_view::DiffMode;
    let fx = git_fixture();
    let mut app = app_with_git_status(&fx);
    app.get_by_label("Changes").click();
    app.run();
    app.get_by_label("Split").click();
    app.run();
    assert_eq!(app.app_state().settings.diff.mode, DiffMode::Split);

    app.get_by_label("Diff settings").click();
    app.run();
    app.get_by_label("Character diff");
    app.get_by_label("Off").click();
    app.run();
    assert_eq!(app.app_state().settings.diff.char_diff, CharDiff::Off);
    app.get_by_label("Collapse unchanged lines (C)").click();
    app.run();
    assert!(app.app_state().settings.diff.collapse);
    assert_eq!(app.app_state().settings.diff.context, 3);

    // The window opens in the chosen layout, and the Changes layout flags removed lines.
    app.get_all_by_label("Changes").last().unwrap().click(); // the layout switch's own button
    app.run();
    assert_eq!(app.app_state().settings.diff.mode, DiffMode::Changes);
    spatialcoding::workspace::open_diff(app.app_state_mut(), fx.path("src/util.rs"));
    wait_for(&mut app, diff_ready);
    app.get_by_label("− 1 removed line");
}

#[test]
fn d_cycles_the_layout_and_c_collapses_unchanged_lines_while_a_diff_is_open() {
    use spatialcoding::ui::diff_view::DiffMode;
    let fx = git_fixture();
    let mut app = app_with_git_status(&fx);
    spatialcoding::workspace::open_diff(app.app_state_mut(), fx.path("src/main.rs"));
    wait_for(&mut app, diff_ready);
    assert_eq!(app.app_state().diff.as_ref().unwrap().mode, DiffMode::Unified);
    app.press_key(egui::Key::D);
    app.run();
    assert_eq!(app.app_state().diff.as_ref().unwrap().mode, DiffMode::Split);
    app.press_key_modifiers(egui::Modifiers::SHIFT, egui::Key::D);
    app.run();
    assert_eq!(app.app_state().diff.as_ref().unwrap().mode, DiffMode::Unified);
    app.press_key(egui::Key::C);
    app.run();
    assert!(app.app_state().settings.diff.collapse);
    assert!(app.query_all_by_label_contains("unchanged line").count() >= 1, "a note stands for the collapsed lines");
}

#[test]
fn a_diff_over_the_scan_limit_waits_behind_the_large_diffs_card() {
    let fx = git_fixture();
    let mut app = app_with_git_status(&fx);
    app.app_state_mut().settings.diff_scan_limit = 0;
    spatialcoding::workspace::open_diff(app.app_state_mut(), fx.path("src/main.rs"));
    wait_for(&mut app, diff_ready);
    app.get_by_label("Large Diffs");
    assert!(app.query_by_label("+     changed_line();").is_none(), "no lines yet");
    app.get_by_label("Show anyway").click();
    app.run();
    app.get_by_label("+     changed_line();");
}

// ---- Git state without polling (TASK-91) ----
//
// `git add`, `git commit` and `git checkout -b` change none of the working
// tree's files, so only the Git state watcher can notice them before the
// periodic refresh. These tests cap the simulated clock below it.

use spatialcoding::git::repo::ChangeKind;

fn git_in(dir: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=T", "-c", "user.email=t@e"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Steps frames until `done`, failing if the simulated clock gets within a
/// second of the periodic refresh that a build without the watcher would use.
fn wait_before_polling(app: &mut App, done: impl Fn(&App) -> bool) {
    let asked = app.app_state().changes.requested_at.expect("a status was asked");
    let limit = asked + spatialcoding::git::changes::REFRESH_SECONDS - 1.0;
    let real_deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if done(app) {
            return;
        }
        let now = app.ctx.input(|i| i.time);
        assert!(now < limit, "the periodic refresh would have run by now ({now:.1} >= {limit:.1})");
        assert!(std::time::Instant::now() < real_deadline, "timed out");
        std::thread::sleep(std::time::Duration::from_millis(40));
        app.step();
    }
}

fn kind_of(app: &App, rel: &str) -> Option<ChangeKind> {
    let state = app.app_state();
    let path = state.loaded_dir.as_ref().unwrap().join(rel);
    state.changes.entries().iter().find(|(p, _)| *p == path).map(|(_, k)| *k)
}

#[test]
fn git_add_shows_up_without_waiting_for_the_poll() {
    let fx = git_fixture();
    let mut app = app_with_git_status(&fx);
    assert_eq!(kind_of(&app, "src/new.rs"), Some(ChangeKind::Untracked));
    git_in(&fx.root, &["add", "src/new.rs"]);
    wait_before_polling(&mut app, |a| kind_of(a, "src/new.rs") == Some(ChangeKind::Added));
}

#[test]
fn a_commit_clears_the_changes_without_waiting_for_the_poll() {
    let fx = git_fixture();
    let mut app = app_with_git_status(&fx);
    assert!(!app.app_state().changes.entries().is_empty());
    git_in(&fx.root, &["add", "-A"]);
    git_in(&fx.root, &["commit", "-qm", "second"]);
    wait_before_polling(&mut app, |a| a.app_state().changes.is_known() && a.app_state().changes.entries().is_empty());
}

#[test]
fn a_new_branch_changes_the_toolbar_without_waiting_for_the_poll() {
    let fx = git_fixture();
    let mut app = app_with_git_status(&fx);
    app.get_by_label("Branch trunk");
    git_in(&fx.root, &["checkout", "-q", "-b", "other"]);
    wait_before_polling(&mut app, |a| a.app_state().changes.branch() == Some("other"));
    app.run();
    app.get_by_label("Branch other");
}

#[test]
fn a_linked_worktree_follows_its_own_index_and_commits() {
    let fx = git_fixture();
    git_in(&fx.root, &["add", "-A"]);
    git_in(&fx.root, &["commit", "-qm", "second"]);
    let wt = fx.root.join("wt");
    git_in(&fx.root, &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "feat"]);
    let mut app = app();
    app.state_mut().open_folder(wt.clone());
    app.run();
    wait_for(&mut app, |a| a.app_state().changes.is_known());
    assert!(app.app_state().changes.entries().is_empty());

    std::fs::write(wt.join("w.txt"), "w\n").unwrap();
    git_in(&wt, &["add", "w.txt"]);
    wait_before_polling(&mut app, |a| kind_of(a, "w.txt") == Some(ChangeKind::Added));
    git_in(&wt, &["commit", "-qm", "w"]);
    wait_before_polling(&mut app, |a| a.app_state().changes.entries().is_empty());
}

#[test]
fn a_folder_inside_the_repository_follows_the_index_of_the_whole_repository() {
    let fx = git_fixture();
    let mut app = app();
    app.state_mut().open_folder(fx.path("src"));
    app.run();
    wait_for(&mut app, |a| a.app_state().changes.is_known());
    assert_eq!(kind_of(&app, "new.rs"), Some(ChangeKind::Untracked));
    git_in(&fx.root, &["add", "src/new.rs"]);
    wait_before_polling(&mut app, |a| kind_of(a, "new.rs") == Some(ChangeKind::Added));
}
