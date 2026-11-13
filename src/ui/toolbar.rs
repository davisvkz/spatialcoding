use crate::state::{AppState, Tool};

pub fn draw_toolbar(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        let folder_label = state
            .loaded_dir
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "No folder".into());

        if ui.button(&folder_label).clicked() {
            // TODO: open folder picker
        }

        ui.separator();

        let select_text = if state.active_tool == Tool::Select {
            egui::RichText::new("Select").strong()
        } else {
            egui::RichText::new("Select")
        };
        if ui.button(select_text).clicked() {
            state.active_tool = Tool::Select;
        }

        let move_text = if state.active_tool == Tool::Move {
            egui::RichText::new("Move").strong()
        } else {
            egui::RichText::new("Move")
        };
        if ui.button(move_text).clicked() {
            state.active_tool = Tool::Move;
        }

        ui.separator();

        let has_nodes = !state.node_store.file_nodes.is_empty()
            || !state.node_store.folder_nodes.is_empty();
        if ui
            .add_enabled(has_nodes, egui::Button::new("Clear All"))
            .clicked()
        {
            crate::workspace::clear_canvas(state);
        }

        let has_selection = state.selection.count() > 0;
        if ui
            .add_enabled(has_selection, egui::Button::new("Clear Selected"))
            .clicked()
        {
            let ids: Vec<u64> = state.selection.selected_ids().collect();
            crate::workspace::remove_nodes(state, &ids);
        }

        ui.separator();

        if ui.button("Settings").clicked() {
            state.show_settings = !state.show_settings;
        }
    });
}
