//! Left-sidebar "Local Server" panel. Visual reference: v2.codecanvas.app.
//! Shows whether the language-server backend is running and exposes the MCP
//! config snippet for agents.

use crate::state::AppState;
use crate::ui::panel::section;

const MCP_SNIPPET: &str = r#"{
  "mcpServers": {
    "spatialcoding": {
      "command": "spatialcoding",
      "args": ["--mcp"]
    }
  }
}"#;

pub fn draw_local_server_panel(ui: &mut egui::Ui, state: &mut AppState) {
    let progress = state.deps.progress(&state.lsp.manager);
    let any_server_running = !progress.is_empty() && progress.iter().any(|p| p.error.is_none());

    section(ui, "Status", |ui| {
        if let Some(socket) = &state.mcp_socket {
            ui.label(
                egui::RichText::new("MCP bridge listening")
                    .color(egui::Color32::from_rgb(0x4a, 0xde, 0x80)),
            );
            ui.weak(socket.display().to_string());
        }
        if any_server_running {
            ui.label(
                egui::RichText::new("Local server running")
                    .color(egui::Color32::from_rgb(0x4a, 0xde, 0x80)),
            );
            ui.weak("Open source files to start language servers.");
        } else {
            ui.label(
                egui::RichText::new("Local server not running")
                    .color(state.theme.accent),
            );
            ui.weak("Open a folder with source files; servers start automatically.");
        }
    });

    section(ui, "MCP", |ui| {
        ui.weak(
            "Copy this into your MCP config, or give it to your AI agent and ask it to set up the connection.",
        );
        ui.add_space(4.0);
        egui::Frame::NONE
            .fill(state.theme.node_header)
            .stroke(egui::Stroke::new(1.0_f32, state.theme.panel_border))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(MCP_SNIPPET).monospace().small())
                        .wrap(),
                );
            });
        if ui.button("Copy to clipboard").clicked() {
            ui.ctx().copy_text(MCP_SNIPPET.to_string());
        }
    });

    section(ui, "Connected agents", |ui| {
        if state.agents.is_empty() {
            ui.weak("No agent connected. Add the snippet above to your MCP config.");
        }
        for name in &state.agents {
            ui.label(format!("\u{2022} {name}"));
        }
    });

    section(ui, "Once connected, ask your agent to", |ui| {
        ui.label("\u{2022} Control the canvas for you.");
        ui.label("\u{2022} Show you features and where to find them.");
        ui.label("\u{2022} Explain how SpatialCoding works.");
    });
}

