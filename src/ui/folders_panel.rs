//! Right-sidebar "Folders" panel. Visual reference: v2.codecanvas.app.
//!
//! The action grid at the top mirrors v2's folder-action toolbar. Only the
//! actions already implemented in `workspace` are enabled; the others are
//! stubbed (disabled) with their icon and tooltip in place.

use crate::state::AppState;
use crate::ui::icons;
use crate::ui::panel::section;

pub fn draw_folders_panel(ui: &mut egui::Ui, state: &mut AppState) {
    section(ui, "Folder Actions", |ui| {
        ui.horizontal_wrapped(|ui| {
            if icons::tool_icon_button(ui, icons::FOLDER, "Open all files").clicked() {
                crate::workspace::open_all_files(state);
            }
            if icons::tool_icon_button(ui, icons::REFRESH, "Rebuild edges").clicked() {
                crate::workspace::rebuild_edges(state);
            }
            if icons::tool_icon_button(ui, icons::TRASH, "Clear canvas").clicked() {
                crate::workspace::clear_canvas(state);
            }
            // Placeholders for parity with v2 — enabled once workspace grows the actions.
            ui.add_enabled(false, egui::Button::new(icons::icon(icons::CHEVRON_LEFT, 14.0)).frame(false));
            ui.add_enabled(false, egui::Button::new(icons::icon(icons::CHEVRON_RIGHT, 14.0)).frame(false));
            ui.add_enabled(false, egui::Button::new(icons::icon(icons::MAXIMIZE, 14.0)).frame(false));
        });
    });

    section(ui, "Opening", |ui| {
        ui.checkbox(
            &mut state.settings.folders.open_collapsed_by_default,
            "Open with collapsed folders by default",
        );
        ui.checkbox(&mut state.settings.folders.auto_open, "Auto");
    });

    section(ui, "Display", |ui| {
        ui.checkbox(&mut state.settings.folders.hide_folders, "Hide Folders");
        ui.checkbox(&mut state.settings.folders.merge_empty, "Merge Empty Folders");
        ui.checkbox(&mut state.settings.folders.merge_overlapping_labels, "Merge Overlapping Labels");
        ui.checkbox(&mut state.settings.folders.show_folder_names, "Folder Names");
        ui.label("Min Size %");
        ui.add(egui::Slider::new(
            &mut state.settings.folders.min_size_pct,
            0.0..=1.0,
        ));
    });

    section(ui, "Diff", |ui| {
        ui.checkbox(&mut state.settings.folders.diff_highlight, "Diff Folder Highlight");
    });
}
