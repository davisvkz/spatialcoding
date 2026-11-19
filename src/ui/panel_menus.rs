//! The kebab menu of each right-rail panel, as in v2 Code Canvas: what it
//! lists and what each entry does.

use crate::state::{AppState, RightPanel};
use crate::ui::icons;
use crate::ui::panel::MenuItem;

/// What a kebab entry does when clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    ToggleAutoLayout,
    ResetLayout,
    ResetEdges,
    ResetFolders,
    ResetFiles,
    ResetCst,
    OpenLanguageServers,
    ToggleSmoothTransition,
    ClearNotes,
    /// Disabled entries; never applied.
    None,
}

/// Accessible name of a panel's kebab button; v2 says "LSP menu" for the
/// LSP Symbols panel and "<title> menu" for the others.
pub fn menu_label(panel: RightPanel) -> Option<&'static str> {
    match panel {
        RightPanel::Layout => Some("Layout menu"),
        RightPanel::LspSymbols => Some("LSP menu"),
        RightPanel::DependencyEdges => Some("Dependency Edges menu"),
        RightPanel::Folders => Some("Folders menu"),
        RightPanel::FileNodes => Some("File nodes menu"),
        RightPanel::Cst => Some("CST menu"),
        RightPanel::AgentNotes => Some("Agent notes menu"),
        RightPanel::Performance => None,
    }
}

/// The entries of `panel`'s kebab, paired with their actions.
pub fn menu(panel: RightPanel, state: &AppState) -> Vec<(MenuItem, MenuAction)> {
    let reset = |label: &str, action| (MenuItem::new(icons::ROTATE_CCW, label), action);
    match panel {
        RightPanel::Layout => {
            let auto = if state.layout_settings.auto_layout {
                MenuItem::new(icons::PAUSE, "Disable Auto Layout")
            } else {
                MenuItem::new(icons::PLAY, "Enable Auto Layout")
            };
            vec![
                (auto, MenuAction::ToggleAutoLayout),
                reset("Reset Layout settings", MenuAction::ResetLayout),
            ]
        }
        RightPanel::DependencyEdges => {
            vec![reset("Reset Dependency Edges settings", MenuAction::ResetEdges)]
        }
        RightPanel::Folders => vec![reset("Reset Folders settings", MenuAction::ResetFolders)],
        RightPanel::FileNodes => vec![reset("Reset File nodes settings", MenuAction::ResetFiles)],
        RightPanel::Cst => vec![reset("Reset CST settings", MenuAction::ResetCst)],
        RightPanel::LspSymbols => vec![(
            MenuItem::new(icons::SERVER, "Language servers"),
            MenuAction::OpenLanguageServers,
        )],
        RightPanel::AgentNotes => vec![
            (MenuItem::new(icons::PLAY, "Smooth Transition"), MenuAction::ToggleSmoothTransition),
            (MenuItem::new(icons::ROTATE_CCW, "Clear Agent notes"), MenuAction::ClearNotes),
        ],
        RightPanel::Performance => Vec::new(),
    }
}

pub fn apply(action: MenuAction, state: &mut AppState) {
    match action {
        MenuAction::ToggleAutoLayout => {
            let settings = &mut state.layout_settings;
            settings.auto_layout = !settings.auto_layout;
        }
        MenuAction::ResetLayout => state.layout_settings = Default::default(),
        MenuAction::ResetEdges => state.edge_style = Default::default(),
        MenuAction::ResetFolders => state.settings.folders = Default::default(),
        MenuAction::ResetFiles => state.settings.files = Default::default(),
        MenuAction::ResetCst => state.settings.cst = Default::default(),
        MenuAction::OpenLanguageServers => state.lsp.show_servers = true,
        MenuAction::ToggleSmoothTransition => {
            state.settings.lsp.smooth_transition = !state.settings.lsp.smooth_transition;
        }
        MenuAction::ClearNotes => state.notes.clear(),
        MenuAction::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_entries_restore_defaults() {
        let mut state = AppState::default();
        state.layout_settings.file_spacing = 1.0;
        state.edge_style.show_barrel_edges = !state.edge_style.show_barrel_edges;
        state.settings.folders.hide_folders = !state.settings.folders.hide_folders;
        state.settings.files.long_max_lines = 3;
        for panel in [
            RightPanel::Layout,
            RightPanel::DependencyEdges,
            RightPanel::Folders,
            RightPanel::FileNodes,
        ] {
            for (_, action) in menu(panel, &state) {
                apply(action, &mut state);
            }
        }
        let defaults = AppState::default();
        assert_eq!(state.layout_settings.file_spacing, defaults.layout_settings.file_spacing);
        assert_eq!(state.edge_style.show_barrel_edges, defaults.edge_style.show_barrel_edges);
        assert_eq!(state.settings.folders.hide_folders, defaults.settings.folders.hide_folders);
        assert_eq!(state.settings.files.long_max_lines, defaults.settings.files.long_max_lines);
    }
}
