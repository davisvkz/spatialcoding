//! Right-sidebar "Performance" panel. Visual reference: v2.codecanvas.app.

use crate::state::{AppState, PerformanceMode};
use crate::ui::panel::section;

pub fn draw_performance_panel(ui: &mut egui::Ui, state: &mut AppState) {
    section(ui, "Analysis", |ui| {
        ui.checkbox(&mut state.settings.performance.auto, "Auto");
        ui.horizontal(|ui| {
            ui.radio_value(&mut state.settings.performance.mode, PerformanceMode::Eager, "Eager");
            ui.radio_value(&mut state.settings.performance.mode, PerformanceMode::OnOpen, "On Open");
            ui.radio_value(&mut state.settings.performance.mode, PerformanceMode::Lazy, "Lazy");
        });
        ui.checkbox(&mut state.settings.performance.use_sqlite, "Use SQLite database");
    });

    section(ui, "CST Depth", |ui| {
        let mut limit_on = state.settings.performance.cst_depth_limit.is_some();
        if ui.checkbox(&mut limit_on, "Limit CST depth").changed() {
            state.settings.performance.cst_depth_limit = if limit_on { Some(6) } else { None };
        }
        if let Some(d) = state.settings.performance.cst_depth_limit.as_mut() {
            ui.horizontal(|ui| {
                ui.label("Max depth");
                ui.add(egui::DragValue::new(d).range(1..=64usize));
            });
        }
    });
}
