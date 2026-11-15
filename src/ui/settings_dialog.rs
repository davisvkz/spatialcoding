use crate::state::AppState;

pub fn draw_settings(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_settings {
        return;
    }

    egui::Window::new("Settings")
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::CollapsingHeader::new("Viewport")
                .default_open(true)
                .show(ui, |ui| {
                    ui.checkbox(&mut state.settings.smooth_zoom, "Smooth Zoom");
                    ui.checkbox(
                        &mut state.settings.show_viewport_details,
                        "Viewport Details",
                    );
                    ui.checkbox(
                        &mut state.settings.show_activity_indicator,
                        "Activity Indicator",
                    );
                });

            egui::CollapsingHeader::new("UI")
                .default_open(true)
                .show(ui, |ui| {
                    ui.checkbox(&mut state.settings.show_controls_bar, "Controls Bar");
                    ui.checkbox(&mut state.settings.show_minimap, "Minimap");
                    ui.checkbox(&mut state.settings.show_grid, "Dot Grid");
                });

            egui::CollapsingHeader::new("Canvas File Nodes")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label("Font Size");
                    ui.add(egui::Slider::new(
                        &mut state.settings.font_size,
                        8.0..=24.0,
                    ));
                    ui.label("Node Width");
                    ui.add(egui::Slider::new(
                        &mut state.settings.node_width,
                        300.0..=1200.0,
                    ));
                });

            egui::CollapsingHeader::new("Repository")
                .default_open(true)
                .show(ui, |ui| {
                    ui.checkbox(
                        &mut state.settings.show_dot_folders,
                        "Show Dot Folders",
                    );
                });

            ui.separator();
            if ui.button("Close").clicked() {
                state.show_settings = false;
            }
        });
}
