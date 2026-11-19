//! Right-rail "Agent notes" panel: markdown cards an MCP agent pushes with
//! the `agentNotes:push` command. Links `cc://file|folder|symbol|range` in a
//! card navigate the canvas on click; hovering one only previews its target.

use crate::agent_notes::parse_link;
use crate::state::AppState;

pub fn draw_agent_panel(ui: &mut egui::Ui, state: &mut AppState) {
    if state.notes.is_empty() {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label("No notes yet. Connect an MCP agent");
            ui.code("spatialcoding --mcp");
            ui.label("(see Local Server) and ask it to explain the codebase with agent notes.");
        });
        return;
    }
    let root = state.loaded_dir.clone();
    let preview = |url: &str| {
        parse_link(url, root.as_deref()).map_or_else(|| url.to_string(), |link| link.preview())
    };
    let mut clicked: Option<String> = None;
    let mut remove = None;
    let mut open_files = None;
    let focused = state.notes.focused;
    for note in state.notes.all() {
        let selected = focused == Some(note.id);
        let frame = egui::Frame::NONE
            .fill(state.theme.node_header)
            .stroke(egui::Stroke::new(if selected { 1.5_f32 } else { 1.0 }, if selected { state.theme.accent } else { state.theme.panel_border }))
            .corner_radius(6.0)
            .inner_margin(8.0);
        let card = frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&note.title).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Remove this note").clicked() {
                        remove = Some(note.id);
                    }
                    if ui.small_button("Open files").on_hover_text("Open the files this note links to").clicked() {
                        open_files = Some(note.id);
                    }
                });
            });
            ui.separator();
            if let Some(url) = crate::ui::markdown::render(ui, &note.markdown, &preview) {
                clicked = Some(url);
            }
        });
        if selected {
            card.response.scroll_to_me(Some(egui::Align::TOP));
        }
        ui.add_space(6.0);
    }
    state.notes.focused = None;
    if let Some(id) = remove {
        state.notes.remove(id);
    }
    if let Some(id) = open_files {
        crate::workspace::open_note_targets(state, Some(id));
    }
    if let Some(url) = clicked {
        match parse_link(&url, state.loaded_dir.as_deref()) {
            Some(link) => crate::workspace::follow_link(state, &link),
            None => crate::ui::toast::push(state, format!("Cannot follow {url}")),
        }
    }
}
