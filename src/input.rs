//! Keyboard shortcut and input handling.

use crate::state::AppState;

/// Process global keyboard shortcuts.
/// Returns true if a shortcut was consumed.
pub fn handle_shortcuts(ctx: &egui::Context, state: &mut AppState) -> bool {
    let mut consumed = false;
    let mut open_all = false;
    // Read before `ctx.input`: calling back into `ctx` inside that closure
    // re-locks the context and deadlocks.
    let screen_rect = ctx.screen_rect();

    ctx.input(|i| {
        // Ctrl+Shift+O: Open all files (file IO runs after the input lock is released)
        if i.key_pressed(egui::Key::O) && i.modifiers.command && i.modifiers.shift {
            open_all = true;
            consumed = true;
        }

        // Space held detection for pan mode
        state.space_held = i.key_down(egui::Key::Space);

        // Shift+F: Fit to screen
        if i.key_pressed(egui::Key::F) && i.modifiers.shift {
            if let Some(world_rect) = compute_nodes_bounding_rect(state) {
                state.viewport.fit_to_rect(world_rect, screen_rect);
            }
            consumed = true;
        }

        // F: Focus on mouse — move the world point under the cursor to the screen center
        if i.key_pressed(egui::Key::F) && !i.modifiers.shift && !i.modifiers.ctrl {
            if let Some(pos) = i.pointer.hover_pos() {
                let vp = &mut state.viewport;
                let world_pos = vp.screen_to_world(pos);
                vp.offset = screen_rect.center().to_vec2() / vp.zoom - world_pos.to_vec2();
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

    if open_all {
        crate::workspace::open_all_files(state);
    }

    consumed
}

pub(crate) fn compute_nodes_bounding_rect(state: &AppState) -> Option<egui::Rect> {
    let mut rects = Vec::new();
    for id in state.node_store.all_file_ids() {
        if let Some(node) = state.node_store.get_file(id) {
            rects.push(node.world_rect(state.settings.node_width, state.settings.font_size));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn bounding_rect_is_none_without_nodes() {
        assert!(compute_nodes_bounding_rect(&AppState::default()).is_none());
    }

    #[test]
    fn bounding_rect_unions_all_file_nodes() {
        let mut state = AppState::default();
        let a = state
            .node_store
            .add_file(PathBuf::from("a.rs"), "x\n".into(), egui::pos2(0.0, 0.0));
        let b = state
            .node_store
            .add_file(PathBuf::from("b.rs"), "x\n".into(), egui::pos2(1000.0, 500.0));

        let rect = compute_nodes_bounding_rect(&state).unwrap();

        let (w, f) = (state.settings.node_width, state.settings.font_size);
        let expected = state.node_store.get_file(a).unwrap().world_rect(w, f)
            .union(state.node_store.get_file(b).unwrap().world_rect(w, f));
        assert_eq!(rect, expected);
        assert_eq!(rect.min, egui::pos2(0.0, 0.0));
        assert_eq!(rect.max.x, 1000.0 + w);
    }
}
