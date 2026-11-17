//! Right-rail "Agent notes" panel. Visual reference: v2.codecanvas.app: a
//! hint to connect an MCP agent that writes notes with its `agent_notes`
//! tool. This app has no MCP server yet, so the link and the kebab entries
//! are shown disabled with the reason.

use crate::state::AppState;

/// Why the agent notes controls cannot be used.
pub const NO_MCP: &str = "Agent notes need an MCP agent connection, which this app does not offer yet.";

pub fn draw_agent_panel(ui: &mut egui::Ui, _state: &mut AppState) {
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.label("No notes yet.");
        ui.add_enabled(false, egui::Link::new("Connect an MCP agent"))
            .on_disabled_hover_text(NO_MCP);
        ui.label("and ask it to explain the codebase using the");
        ui.code("agent notes");
        ui.label("tool.");
    });
}
