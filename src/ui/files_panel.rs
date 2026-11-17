//! Right-rail "File nodes" panel: canvas actions on file nodes, top-level
//! symbol highlights, long-file collapse, file-name labels and workspace
//! details. Visual reference: v2.codecanvas.app.

use crate::git::changes::ChangesStatus;
use crate::state::AppState;
use crate::ui::icons;
use crate::ui::panel::{action_group, icon_action, section, slider_row, switch};
use crate::workspace::{self, ImportDirection};

pub fn draw_files_panel(ui: &mut egui::Ui, state: &mut AppState) {
    section(ui, "Actions", |ui| draw_actions(ui, state));

    section(ui, "Symbols", |ui| {
        ui.horizontal(|ui| {
            let selected = state.selection.count() > 0;
            let active = !state.top_level_highlights.is_empty();
            let button = ui.add_enabled_ui(selected, |ui| {
                action_button(ui, icons::LAYERS, "Highlight top-level symbols", active)
            });
            if button.inner.clicked() {
                workspace::toggle_top_level_highlights(state);
            }
            button.response.on_disabled_hover_text("Select a file node first.");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut state.settings.files.skip_imports, "Skip imports")
                    .on_hover_text("Leave import statements out of the top-level symbols.");
            });
        });
    });

    section(ui, "Long Files", |ui| {
        let files = &mut state.settings.files;
        switch(ui, &mut files.collapse_long, "Collapse long files")
            .on_hover_text("Files longer than Max lines open collapsed.");
        ui.add_enabled_ui(files.collapse_long, |ui| {
            ui.horizontal(|ui| {
                ui.label("Max lines");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let response = ui.add(
                        egui::DragValue::new(&mut files.long_max_lines).range(1..=1_000_000),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::DragValue,
                            true,
                            "Max lines before collapsing long files",
                        )
                    });
                });
            });
        });
        let collapsed = workspace::collapsed_files(state);
        let label = format!("Expand all {collapsed} collapsed files");
        if ui
            .add_enabled(
                collapsed > 0,
                egui::Button::new(label).min_size(egui::vec2(ui.available_width(), 22.0)),
            )
            .clicked()
        {
            workspace::expand_all_files(state);
        }
    });

    section(ui, "File Names", |ui| {
        let files = &mut state.settings.files;
        switch(ui, &mut files.show_names, "File Names");
        ui.add_enabled_ui(files.show_names, |ui| {
            slider_row(ui, "Min Size", &mut files.min_size_pct, 0.0..=1.0, |v| {
                format!("{:.0}%", v * 100.0)
            })
            .on_hover_text("Hide a file name when it would be drawn smaller than this.");
        });
    });

    section(ui, "Details", |ui| draw_details(ui, state));
}

fn draw_actions(ui: &mut egui::Ui, state: &mut AppState) {
    let has_nodes = !state.node_store.file_nodes.is_empty();
    let has_selection = state.selection.count() > 0;
    let changes_known = state.changes.is_known();
    let unopened_changed = workspace::unopened_changed_files(state).len();
    let unchanged = workspace::unchanged_nodes(state).len();
    let no_changes = changes_reason(&state.changes.status);

    ui.horizontal_wrapped(|ui| {
        action_group(ui, |ui| {
            if icon_action(ui, state.loaded_dir.is_some(), icons::FILES, "Open all files", "Load a folder first.") {
                workspace::open_all_files(state);
            }
            let reason = if changes_known { "No unopened changed files." } else { no_changes };
            if icon_action(ui, unopened_changed > 0, icons::FILE_DIFF, "Open all changed files", reason) {
                workspace::open_changed_files(state);
            }
            let reason = if changes_known { "Every open file has changes." } else { no_changes };
            if icon_action(ui, unchanged > 0, icons::FILE_MINUS, "Close files without changes", reason) {
                workspace::close_unchanged_files(state);
            }
        });
        action_group(ui, |ui| {
            const NEED: &str = "Select a file node first.";
            if icon_action(ui, has_selection, icons::ARROW_LEFT_TO_LINE, "Open files that import the selection", NEED) {
                workspace::open_import_neighbours(state, ImportDirection::Importers);
            }
            if icon_action(ui, has_selection, icons::ARROW_RIGHT_TO_LINE, "Open imported files", NEED) {
                workspace::open_import_neighbours(state, ImportDirection::Imported);
            }
        });
        action_group(ui, |ui| {
            if icon_action(ui, has_nodes, icons::SCAN, "Fit to nodes", "No nodes on the canvas.") {
                state.pending_fit = true;
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        action_group(ui, |ui| {
            if icon_action(ui, has_nodes, icons::CHECK_SQUARE, "Select all nodes", "No nodes on the canvas.") {
                let ids = state.node_store.all_file_ids();
                state.selection.select_all(ids);
            }
            if icon_action(ui, has_selection, icons::SQUARE, "Unselect all", "Nothing is selected.") {
                state.selection.clear();
            }
            if icon_action(ui, has_selection, icons::CLOSE, "Remove selected nodes", "Nothing is selected.") {
                let ids: Vec<u64> = state.selection.selected_ids().collect();
                workspace::remove_nodes(state, &ids);
            }
        });
        action_group(ui, |ui| {
            if icon_action(ui, has_nodes, icons::TRASH, "Clear all nodes", "No nodes on the canvas.") {
                workspace::clear_canvas(state);
            }
        });
    });
}

fn changes_reason(status: &ChangesStatus) -> &'static str {
    match status {
        ChangesStatus::Idle | ChangesStatus::Loading => "Reading Git changes…",
        ChangesStatus::Ready => "",
        ChangesStatus::Unavailable(_) => "Git changes are unavailable for this folder.",
    }
}

/// A framed icon toggle button with an accessible name.
fn action_button(ui: &mut egui::Ui, icon: &str, label: &str, active: bool) -> egui::Response {
    let response = ui.add(egui::Button::new(icons::icon(icon, 14.0)).selected(active));
    let ak = label.to_owned();
    let enabled = ui.is_enabled();
    response.widget_info(move || egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, active, ak.clone()));
    response.on_hover_text(label)
}

fn draw_details(ui: &mut egui::Ui, state: &AppState) {
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
    let row = |ui: &mut egui::Ui, label: &str, value: String| {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(value);
            });
        });
    };
    row(ui, "Files", file_count.to_string());
    row(ui, "Lines", format_count(lines));
    row(ui, "Characters", format_count(chars));
    let changes = match &state.changes.status {
        ChangesStatus::Ready => format!("{}", state.changes.paths().count()),
        ChangesStatus::Idle | ChangesStatus::Loading => "…".to_owned(),
        ChangesStatus::Unavailable(_) => "no Git".to_owned(),
    };
    row(ui, "Changed (Git)", changes);
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
