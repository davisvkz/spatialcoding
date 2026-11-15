//! Right-sidebar "Agent notes" panel. Visual reference: v2.codecanvas.app
//! — a short help message plus a "Connect an MCP agent" link that jumps to
//! Settings.

use crate::state::AppState;

pub fn draw_agent_panel(ui: &mut egui::Ui, state: &mut AppState) {
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.label("No notes yet.");
        if ui
            .add(egui::Link::new("Connect an MCP agent"))
            .on_hover_text("Open settings to configure the MCP connection")
            .clicked()
        {
            state.show_settings = true;
        }
        ui.label("and ask it to explain the codebase using the");
        ui.code("agent_notes");
        ui.label("tool.");
    });
}
