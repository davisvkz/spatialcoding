//! Performance and Agent notes panels: what this app cannot do is shown
//! disabled with the reason; CST Depth is shared with the CST panel.

use egui_kittest::kittest::Queryable;
use spatialcoding::state::RightPanel;

use crate::common::*;

fn panel(panel: RightPanel) -> App {
    let mut app = app();
    app.app_state_mut().left_panel = None;
    app.app_state_mut().right_panel = Some(panel);
    app.run();
    app
}

#[test]
fn analysis_modes_and_sqlite_are_disabled_with_an_explanation() {
    let mut app = panel(RightPanel::Performance);
    app.get_by_label("Analysis info");
    assert!(app.get_by_label("Auto").is_disabled());
    for mode in ["Eager", "On Open", "Lazy"] {
        assert!(app.get_by_label(mode).is_disabled(), "{mode}");
    }
    assert!(app.get_by_label("Use SQLite database").is_disabled());
    app.get_by_label("Max CST depth");
    app.run();
}

#[test]
fn performance_cst_depth_is_the_cst_panels_depth() {
    let mut app = panel(RightPanel::Performance);
    assert!(app.app_state().settings.cst.limit_depth, "on by default, as v2");
    app.get_by_label("Limit CST depth").click();
    app.run();
    assert!(!app.app_state().settings.cst.limit_depth);
    app.app_state_mut().right_panel = Some(RightPanel::Cst);
    app.app_state_mut().cst.show_settings = true;
    app.run();
    let limit = app.get_by_label("Limit CST depth");
    assert_eq!(limit.toggled(), Some(egui::accesskit::Toggled::False), "the CST panel shows it off");
}

#[test]
fn agent_notes_show_their_hint_and_an_enabled_menu() {
    let mut app = panel(RightPanel::AgentNotes);
    app.get_by_label_contains("No notes yet.");
    app.get_by_label("spatialcoding --mcp");
    app.get_by_label("Agent notes menu").click();
    app.run();
    assert!(!app.get_by_label("Smooth Transition").is_disabled());
    assert!(!app.get_by_label("Clear Agent notes").is_disabled());
}
