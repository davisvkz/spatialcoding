//! Right-sidebar "File nodes" panel. Visual reference: v2.codecanvas.app.

use crate::state::AppState;
use crate::ui::icons;
use crate::ui::panel::section;

pub fn draw_files_panel(ui: &mut egui::Ui, state: &mut AppState) {
    section(ui, "Actions", |ui| {
        ui.horizontal_wrapped(|ui| {
            if icons::tool_icon_button(ui, icons::COPY, "Copy selected").clicked() {
                // TODO: copy selected node paths to clipboard.
            }
            if icons::tool_icon_button(ui, icons::TRASH, "Remove selected").clicked() {
                let ids: Vec<u64> = state.selection.selected_ids().collect();
                crate::workspace::remove_nodes(state, &ids);
            }
            if icons::tool_icon_button(ui, icons::REFRESH, "Rebuild edges").clicked() {
                crate::workspace::rebuild_edges(state);
            }
            if icons::tool_icon_button(ui, icons::MAXIMIZE, "Fit to selection").clicked() {
                // TODO: fit to selection (currently handled by shortcuts).
            }
        });
    });

    section(ui, "Symbols", |ui| {
        ui.checkbox(&mut state.settings.files.skip_imports, "Skip imports");
    });

    section(ui, "Long Files", |ui| {
        ui.checkbox(&mut state.settings.files.collapse_long, "Collapse long files");
        ui.horizontal(|ui| {
            ui.label("Max lines");
            ui.add(
                egui::DragValue::new(&mut state.settings.files.long_max_lines)
                    .range(50..=100_000),
            );
        });
    });

    section(ui, "File Names", |ui| {
        ui.checkbox(&mut state.settings.files.show_names, "File Names");
        ui.label("Min Size %");
        ui.add(egui::Slider::new(
            &mut state.settings.files.min_size_pct,
            0.0..=1.0,
        ));
    });

    section(ui, "Details", |ui| {
        let dir = state
            .loaded_dir
            .as_deref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "No folder".to_string());
        ui.horizontal(|ui| {
            ui.weak(dir);
        });
        let file_count = state.node_store.file_nodes.len();
        let (lines, chars) = state
            .node_store
            .file_nodes
            .values()
            .map(|n| &n.content)
            .fold((0usize, 0usize), |(l, c), src| (l + src.lines().count(), c + src.len()));
        ui.horizontal(|ui| {
            ui.label("Files");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(file_count.to_string());
            });
        });
        ui.horizontal(|ui| {
            ui.label("Lines");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(format_count(lines));
            });
        });
        ui.horizontal(|ui| {
            ui.label("Characters");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(format_count(chars));
            });
        });
    });
}

fn format_count(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
