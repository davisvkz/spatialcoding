//! Right-rail "Performance" panel. Visual reference: v2.codecanvas.app.
//!
//! v2 can analyze eagerly, on open or lazily and keep results in SQLite
//! through its local server. This app analyzes each file when it opens and
//! keeps the results in memory, so those controls are shown disabled with
//! the reason; CST Depth is the CST panel's own depth limit.

use crate::state::AppState;
use crate::ui::panel::{disabled_switch, section, segmented, Section};

/// v2's info tooltip, for this app's analysis.
const ANALYSIS_HELP: &str = "Analysis runs in this app's memory: syntax trees and language-server \
answers are kept until you close it. There is no local server or database to configure.";

const NO_MODES: &str = "This app analyzes each file when it opens; Eager and Lazy analysis need \
v2's local server.";

pub fn draw_performance_panel(ui: &mut egui::Ui, state: &mut AppState) {
    Section::new("Analysis").info(ANALYSIS_HELP).show(ui, |ui| {
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

    section(ui, "CST Depth", |ui| crate::ui::cst_panel::draw_depth_controls(ui, &mut state.settings.cst));
}
