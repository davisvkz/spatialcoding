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
    // A focused text field (palette, search) owns plain keys: typing "f" or
    // "1" must not fit the view or lay the canvas out. Chords still work.
    let typing = ctx.wants_keyboard_input();

    ctx.input(|i| {
        // Ctrl+Shift+O: Open all files (file IO runs after the input lock is released)
        if i.key_pressed(egui::Key::O) && i.modifiers.command && i.modifiers.shift {
            open_all = true;
            consumed = true;
        }

        // Space held detection for pan mode
        state.space_held = i.key_down(egui::Key::Space) && !typing;

        // Shift+F: Fit to screen. The canvas fits next frame, against its own
        // rect rather than the whole window.
        if i.key_pressed(egui::Key::F) && i.modifiers.shift && !typing {
            state.pending_fit = true;
            consumed = true;
        }

        // F: Focus on mouse — move the world point under the cursor to the screen center
        if i.key_pressed(egui::Key::F) && !i.modifiers.shift && !i.modifiers.ctrl && !typing {
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
        if i.key_pressed(egui::Key::X) && i.modifiers.shift && !typing {
            crate::workspace::clear_canvas(state);
            consumed = true;
        }

        // 1: Apply layout
        if i.key_pressed(egui::Key::Num1) && !i.modifiers.command && !i.modifiers.shift && !typing {
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

/// World rects of everything shown on the canvas: file nodes outside
/// collapsed folders, and the collapsed folders' boxes.
pub(crate) fn shown_node_rects(state: &AppState) -> Vec<egui::Rect> {
    let store = &state.node_store;
    let hidden_folders = store.hidden_folders();
    let files = store
        .file_nodes
        .values()
        .filter(|n| !store.is_file_hidden(n.id))
        .map(|n| n.world_rect(state.settings.node_width, state.settings.font_size));
    let folders = store
        .folder_nodes
        .values()
        .filter(|f| !hidden_folders.contains(&f.id))
        .filter_map(|f| {
            if store.layout_collapsed(f.id) {
                f.collapsed_rect()
            } else {
                store.view.zoom_collapsed.contains(&f.id).then_some(f.bounds)
            }
        });
    files.chain(folders).collect()
}

pub(crate) fn compute_nodes_bounding_rect(state: &AppState) -> Option<egui::Rect> {
    shown_node_rects(state).into_iter().reduce(|a, b| a.union(b))
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
