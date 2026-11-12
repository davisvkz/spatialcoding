//! Keyboard shortcut and input handling.

use crate::state::AppState;

/// Process global keyboard shortcuts.
/// Returns true if a shortcut was consumed.
pub fn handle_shortcuts(ctx: &egui::Context, state: &mut AppState) -> bool {
    let mut consumed = false;

    ctx.input(|i| {
        // Space held detection for pan mode
        state.space_held = i.key_down(egui::Key::Space);

        // Shift+F: Fit to screen
        if i.key_pressed(egui::Key::F) && i.modifiers.shift {
            if let Some(world_rect) = compute_nodes_bounding_rect(state) {
                state.viewport.fit_to_rect(world_rect, ctx.screen_rect());
            }
            consumed = true;
        }

        // F: Focus on mouse
        if i.key_pressed(egui::Key::F) && !i.modifiers.shift && !i.modifiers.ctrl {
            // Focus viewport on cursor position
            if let Some(pos) = i.pointer.hover_pos() {
                let world_pos = state.viewport.screen_to_world(pos);
                state.viewport.offset = world_pos.to_vec2() - ctx.screen_rect().center().to_vec2();
            }
            consumed = true;
        }

        // Ctrl+K: Command palette
        if i.key_pressed(egui::Key::K) && i.modifiers.command {
            state.show_command_palette = !state.show_command_palette;
            consumed = true;
        }

        // Ctrl+/: Shortcuts
        if i.key_pressed(egui::Key::Slash) && i.modifiers.command {
            state.show_shortcuts = !state.show_shortcuts;
            consumed = true;
        }

        // Shift+X: Clear canvas
        if i.key_pressed(egui::Key::X) && i.modifiers.shift {
            state.node_store.clear();
            state.edge_store.clear();
            state.selection.clear();
            consumed = true;
        }

        // 1: Apply layout
        if i.key_pressed(egui::Key::Num1) && !i.modifiers.command && !i.modifiers.shift {
            crate::layout::hierarchical::apply_layout(
                &mut state.node_store,
                &state.edge_store,
                &state.layout_settings,
            );
            consumed = true;
        }

        // Escape: close modals
        if i.key_pressed(egui::Key::Escape) {
            if state.show_settings { state.show_settings = false; consumed = true; }
            else if state.show_shortcuts { state.show_shortcuts = false; consumed = true; }
            else if state.show_command_palette { state.show_command_palette = false; consumed = true; }
            else { state.selection.clear(); consumed = true; }
        }
    });

    consumed
}

fn compute_nodes_bounding_rect(state: &AppState) -> Option<egui::Rect> {
    use crate::nodes::lod::LodLevel;
    let lod = LodLevel::from_zoom(state.viewport.zoom);
    let mut rects = Vec::new();
    for id in state.node_store.all_file_ids() {
        if let Some(node) = state.node_store.get_file(id) {
            rects.push(node.world_rect(state.settings.node_width, state.settings.font_size, lod));
        }
    }
    if rects.is_empty() {
        return None;
    }
    let mut combined = rects[0];
    for r in &rects[1..] {
        combined = combined.union(*r);
    }
    Some(combined)
}
