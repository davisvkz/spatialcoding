//! The shared chrome of the right-rail panels: reference titles, the close
//! button, and the kebab menu with its "Reset <Panel> settings" entry.

use egui_kittest::kittest::Queryable;
use spatialcoding::state::RightPanel;

use crate::common::*;

const PANELS: &[(&str, RightPanel)] = &[
    ("Layout", RightPanel::Layout),
    ("LSP Symbols", RightPanel::LspSymbols),
    ("Dependency Edges", RightPanel::DependencyEdges),
    ("Folders", RightPanel::Folders),
    ("File nodes", RightPanel::FileNodes),
    ("CST", RightPanel::Cst),
    ("Agent notes", RightPanel::AgentNotes),
    ("Performance", RightPanel::Performance),
];

#[test]
fn rail_buttons_open_panels_titled_and_closed_like_the_reference() {
    let mut app = app();
    app.app_state_mut().left_panel = None;
    app.run();
    for &(title, panel) in PANELS {
        app.get_by_label(title).click();
        app.run();
        assert_eq!(app.app_state().right_panel, Some(panel), "{title}");
        app.get_by_label(&format!("{title} panel"));
        app.get_by_label(&format!("Close {title}")).click();
        app.run();
        assert_eq!(app.app_state().right_panel, None, "{title} closes");
    }
}

#[test]
fn kebab_reset_restores_the_panel_defaults() {
    let mut app = app();
    app.app_state_mut().left_panel = None;
    app.app_state_mut().right_panel = Some(RightPanel::Layout);
    app.app_state_mut().layout_settings.file_spacing = 1234.0;
    app.run();
    assert!(app.query_by_label("Reset Layout settings").is_none(), "menu starts closed");

    app.get_by_label("Layout menu").click();
    app.run();
    app.get_by_label("Reset Layout settings").click();
    app.run();

    let defaults = spatialcoding::layout::settings::LayoutSettings::default();
    assert_eq!(app.app_state().layout_settings.file_spacing, defaults.file_spacing);
    assert!(app.query_by_label("Reset Layout settings").is_none(), "menu closes on click");
}

#[test]
fn every_panel_but_performance_has_a_kebab() {
    let mut app = app();
    app.app_state_mut().left_panel = None;
    for &(title, panel) in PANELS {
        app.app_state_mut().right_panel = Some(panel);
        app.run();
        let menu = if panel == RightPanel::LspSymbols {
            "LSP menu".to_owned()
        } else {
            format!("{title} menu")
        };
        let found = app.query_by_label(&menu).is_some();
        assert_eq!(found, panel != RightPanel::Performance, "{menu}");
    }
}

#[derive(Default)]
struct Widgets {
    flag: bool,
    side: u8,
    body_drawn: bool,
}

fn widgets_harness() -> egui_kittest::Harness<'static, Widgets> {
    use spatialcoding::ui::panel::{segmented, switch, Section};
    egui_kittest::Harness::new_ui_state(
        |ui, w: &mut Widgets| {
            // The real app installs the icon font in `SpatialCodingApp::new`;
            // here it takes effect from the next frame, so skip this one.
            let icon_family = egui::FontFamily::Name(spatialcoding::ui::icons::ICON_FAMILY.into());
            if !ui.ctx().fonts(|f| f.families().contains(&icon_family)) {
                spatialcoding::ui::icons::install(ui.ctx());
                ui.ctx().request_repaint();
                return;
            }
            w.body_drawn = false;
            let mut header_flag = w.flag;
            Section::new("Advanced")
                .info("What advanced does")
                .switch("Advanced switch", &mut header_flag)
                .collapsible(false)
                .show(ui, |ui| {
                    ui.label("advanced body");
                });
            w.flag = header_flag;
            Section::new("Body").show(ui, |ui| {
                w.body_drawn = true;
                switch(ui, &mut w.flag, "Plain switch");
                segmented(ui, &mut w.side, &[(0, "Left"), (1, "Center"), (2, "Right")]);
            });
        },
        Widgets::default(),
    )
}

#[test]
fn collapsible_sections_start_closed_and_expand_from_their_chevron() {
    let mut h = widgets_harness();
    h.run();
    assert!(h.query_by_label("advanced body").is_none());
    h.get_by_label("Advanced info");
    h.get_by_label("Expand Advanced").click();
    h.run();
    h.get_by_label("advanced body");
    h.get_by_label("Collapse Advanced").click();
    h.run();
    assert!(h.query_by_label("advanced body").is_none());
    assert!(h.state().body_drawn);
}

#[test]
fn switches_and_segments_change_their_values() {
    let mut h = widgets_harness();
    h.run();
    h.get_by_label("Advanced switch").click();
    h.run();
    assert!(h.state().flag, "header switch");
    h.get_by_label("Plain switch").click();
    h.run();
    assert!(!h.state().flag, "body switch shares the value");
    h.get_by_label("Right").click();
    h.run();
    assert_eq!(h.state().side, 2);
}

#[test]
fn panel_settings_survive_a_restart_when_persistence_is_on() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");

    let mut first = app();
    first.state_mut().enable_settings_persistence(path.clone());
    first.app_state_mut().layout_settings.folder_padding = 321.0;
    first.app_state_mut().settings.files.long_max_lines = 77;
    // Saving is throttled to about once a second of app time.
    for _ in 0..8 {
        first.step();
    }
    assert!(path.exists(), "settings written");

    let mut second = app();
    second.state_mut().enable_settings_persistence(path);
    assert_eq!(second.app_state().layout_settings.folder_padding, 321.0);
    assert_eq!(second.app_state().settings.files.long_max_lines, 77);
}
