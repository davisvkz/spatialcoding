//! Keyboard shortcut handling: every key press is looked up in the command
//! registry (`crate::commands`), which owns the shortcut table.

use crate::state::AppState;

/// Runs the command of each key pressed this frame. Returns true if a
/// shortcut was consumed.
pub fn handle_shortcuts(ctx: &egui::Context, state: &mut AppState) -> bool {
    // A focused text field (palette, search) owns plain keys: typing "f" or
    // "1" must not fit the view or lay the canvas out. Chords still work.
    let typing = ctx.wants_keyboard_input();
    state.space_held = ctx.input(|i| i.key_down(egui::Key::Space)) && !typing;

    // Collected before running: a command may need the context again, which
    // would deadlock inside `ctx.input`.
    let pressed: Vec<(egui::Key, egui::Modifiers)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect()
    });
    let mut consumed = false;
    for (key, modifiers) in pressed {
        let Some(command) = crate::commands::for_key(key, modifiers, typing) else { continue };
        if crate::commands::run(state, command.id, &serde_json::Value::Null).is_ok() {
            consumed = true;
        }
    }
    consumed
}

/// World rects of everything shown on the canvas: file nodes outside
/// collapsed folders, and the collapsed folders' boxes.
pub(crate) fn shown_node_rects(state: &AppState) -> Vec<egui::Rect> {
    let store = &state.node_store;
    shown_node_rects_with(state, &store.hidden_folders(), &store.hidden_files())
}

/// Like [`shown_node_rects`], with the hidden sets of the frame in
/// `node_store.view` instead of computing them again.
pub(crate) fn frame_node_rects(state: &AppState) -> Vec<egui::Rect> {
    let view = &state.node_store.view;
    shown_node_rects_with(state, &view.hidden_folders, &view.hidden_files)
}

fn shown_node_rects_with(
    state: &AppState,
    hidden_folders: &std::collections::HashSet<u64>,
    hidden_files: &std::collections::HashSet<u64>,
) -> Vec<egui::Rect> {
    let store = &state.node_store;
    let files = store
        .file_nodes
        .values()
        .filter(|n| !hidden_files.contains(&n.id))
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
