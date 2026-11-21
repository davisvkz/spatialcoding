//! Right-rail "Performance" panel. Visual reference: v2.codecanvas.app.
//!
//! v2 can analyze eagerly, on open or lazily and keep results in SQLite
//! through its local server. This app analyzes each file when it opens and
//! keeps the results in memory, so those controls are shown disabled with
//! the reason; CST Depth is the CST panel's own depth limit.

use crate::state::AppState;
use crate::ui::panel::{disabled_switch, section, segmented, Section};

const NO_MODES: &str =
    "SpatialCoding analyzes files on open. The analysis mode is not configurable.";

/// What this app keeps and where.
const STORAGE_ABOUT: &str =
    "Syntax trees, language-server answers and dependency edges live in this \
app's memory and are rebuilt when a file changes or you open it again. Nothing is written to disk \
except your settings. Diffs larger than the Diff Scan Limit (Settings › Performance) wait behind a \
Large Diffs card.";

pub fn draw_performance_panel(ui: &mut egui::Ui, state: &mut AppState) {
    let help_id = ui.make_persistent_id("performance_storage_help");
    let mut show_help = ui.data(|data| data.get_temp(help_id).unwrap_or(false));
    Section::new("Analysis")
        .info_disclosure("About analysis storage", &mut show_help)
        .show(ui, |ui| {
            let mut auto = true;
            disabled_switch(ui, &mut auto, "Auto", NO_MODES);
            // The mode this app uses, shown but not choosable.
            let mut mode = 1u8;
            ui.add_enabled_ui(false, |ui| {
                segmented(ui, &mut mode, &[(0, "Eager"), (1, "On Open"), (2, "Lazy")]);
            })
            .response
            .on_disabled_hover_text(NO_MODES);
            let mut sqlite = false;
            disabled_switch(
                ui,
                &mut sqlite,
                "Use SQLite database",
                "Results are not stored on disk in this app.",
            );
        });

    if show_help {
        ui.add(egui::Label::new(egui::RichText::new(STORAGE_ABOUT).small().weak()).wrap());
    }
    ui.data_mut(|data| data.insert_temp(help_id, show_help));

    section(ui, "CST Depth", |ui| {
        crate::ui::cst_panel::draw_depth_controls(ui, &mut state.settings.cst)
    });
}
