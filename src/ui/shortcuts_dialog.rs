pub fn draw_shortcuts(ctx: &egui::Context, show: &mut bool) {
    if !*show {
        return;
    }

    egui::Window::new("Shortcuts")
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.heading("Viewport");
            shortcut_row(ui, "Zoom", "Ctrl + Scroll");
            shortcut_row(ui, "Pan", "Space + Drag / Right-Click Drag");
            shortcut_row(ui, "Focus on Mouse", "F");
            shortcut_row(ui, "Fit to Screen", "Shift + F");

            ui.add_space(8.0);
            ui.heading("Canvas");
            shortcut_row(ui, "Clear Canvas", "Shift + X");
            shortcut_row(ui, "Apply Layout", "1");
            shortcut_row(ui, "Open All Files", "Ctrl + Shift + O");

            ui.separator();
            if ui.button("Close").clicked() {
                *show = false;
            }
        });
}

fn shortcut_row(ui: &mut egui::Ui, action: &str, keys: &str) {
    ui.horizontal(|ui| {
        ui.label(action);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(
                egui::RichText::new(keys).monospace().strong(),
            ));
        });
    });
}
