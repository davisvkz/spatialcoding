//! Command palette (Ctrl+K): fuzzy search over app actions and the files of
//! the loaded folder. The palette only picks; [`PaletteCommand`]s are run by
//! the app, which owns the file tree and the folder picker.

use std::path::PathBuf;

use egui::{Align2, Color32, FontId, Key, Modifiers};

use crate::search::fuzzy::{fuzzy_match, FuzzyMatch};
use crate::state::{AppState, LeftPanel, RightPanel};

/// Most rows kept in the list; the list shows `VISIBLE_ROWS` and scrolls.
const MAX_ROWS: usize = 40;
const VISIBLE_ROWS: f32 = 12.0;
const ROW_HEIGHT: f32 = 26.0;
const WIDTH: f32 = 520.0;

/// What the user picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteCommand {
    OpenFolder,
    OpenAllFiles,
    FitView,
    ApplyLayout,
    ClearCanvas,
    ToggleLeft(LeftPanel),
    ToggleRight(RightPanel),
    ToggleSettings,
    ToggleShortcuts,
    ToggleMinimap,
    /// Open the diff window for the selected file node.
    ShowDiff,
    /// Show a file of the folder on the canvas, opening it when needed.
    GoToFile(PathBuf),
}

/// Text, selection and open-state of the palette.
#[derive(Debug, Default)]
pub struct PaletteState {
    pub query: String,
    pub selected: usize,
    /// The palette was drawn last frame; a fresh open resets the query.
    was_open: bool,
}

struct Entry {
    label: String,
    hint: &'static str,
    command: PaletteCommand,
    matched: FuzzyMatch,
}

/// Commands that make sense now, with their shortcut hints.
fn commands(state: &AppState) -> Vec<(String, &'static str, PaletteCommand)> {
    let has_folder = state.loaded_dir.is_some();
    let has_nodes = !state.node_store.file_nodes.is_empty();
    let mut list: Vec<(String, &'static str, PaletteCommand)> = Vec::new();
    list.push(("Open folder…".into(), "", PaletteCommand::OpenFolder));
    if has_folder {
        list.push(("Open all files".into(), "Ctrl+Shift+O", PaletteCommand::OpenAllFiles));
    }
    if has_nodes {
        list.push(("Fit view to nodes".into(), "Shift+F", PaletteCommand::FitView));
        list.push(("Apply layout".into(), "1", PaletteCommand::ApplyLayout));
        list.push(("Clear canvas".into(), "Shift+X", PaletteCommand::ClearCanvas));
    }
    if state.selection.primary.is_some() {
        list.push(("Show diff of selected file".into(), "", PaletteCommand::ShowDiff));
    }
    for (label, panel) in [
        ("Workspace", LeftPanel::Workspace),
        ("Changes", LeftPanel::Changes),
        ("Search", LeftPanel::Search),
        ("Local Server", LeftPanel::LocalServer),
    ] {
        list.push((format!("Toggle {label} panel"), "", PaletteCommand::ToggleLeft(panel)));
    }
    for (label, panel) in [
        ("Layout", RightPanel::Layout),
        ("LSP Symbols", RightPanel::LspSymbols),
        ("Dependency Edges", RightPanel::DependencyEdges),
        ("Folders", RightPanel::Folders),
        ("File Nodes", RightPanel::FileNodes),
        ("CST", RightPanel::Cst),
        ("Agent Notes", RightPanel::AgentNotes),
        ("Performance", RightPanel::Performance),
    ] {
        list.push((format!("Toggle {label} panel"), "", PaletteCommand::ToggleRight(panel)));
    }
    list.push(("Toggle minimap".into(), "", PaletteCommand::ToggleMinimap));
    list.push(("Open settings".into(), "", PaletteCommand::ToggleSettings));
    list.push(("Show shortcuts".into(), "Ctrl+/", PaletteCommand::ToggleShortcuts));
    list
}

/// Commands and, once the user types, files, filtered and ranked.
fn entries(state: &AppState, query: &str) -> Vec<Entry> {
    let mut found: Vec<Entry> = commands(state)
        .into_iter()
        .filter_map(|(label, hint, command)| {
            fuzzy_match(query, &label).map(|matched| Entry { label, hint, command, matched })
        })
        .collect();
    if !query.trim().is_empty()
        && let Some(root) = state.loaded_dir.as_deref()
    {
        let files = crate::workspace::folder_text_files(state);
        for path in files.iter() {
            let label = path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned();
            if let Some(matched) = fuzzy_match(query, &label) {
                found.push(Entry { label, hint: "file", command: PaletteCommand::GoToFile(path.clone()), matched });
            }
        }
    }
    found.sort_by_key(|entry| std::cmp::Reverse(entry.matched.score));
    found.truncate(MAX_ROWS);
    found
}

/// Draws the palette while `state.show_command_palette` is set. Returns the
/// command to run, after closing the palette.
pub fn draw_command_palette(ctx: &egui::Context, state: &mut AppState) -> Option<PaletteCommand> {
    if !state.show_command_palette {
        state.palette.was_open = false;
        return None;
    }
    let fresh = !state.palette.was_open;
    if fresh {
        state.palette.query.clear();
        state.palette.selected = 0;
    }
    state.palette.was_open = true;

    let entries = entries(state, &state.palette.query.clone());
    let last = entries.len().saturating_sub(1);
    let (down, up, enter) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::Enter),
        )
    });
    if down {
        state.palette.selected = (state.palette.selected + 1).min(last);
    }
    if up {
        state.palette.selected = state.palette.selected.saturating_sub(1);
    }
    state.palette.selected = state.palette.selected.min(last);

    let theme = state.theme.clone();
    let mut chosen: Option<PaletteCommand> = None;
    let area = egui::Area::new(egui::Id::new("command_palette"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_TOP, [0.0, 80.0])
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme.panel_bg)
                .stroke(egui::Stroke::new(1.0_f32, theme.panel_border))
                .corner_radius(10.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.set_width(WIDTH);
                    let edit = egui::TextEdit::singleline(&mut state.palette.query)
                        .hint_text("Type a command or file name…")
                        .desired_width(f32::INFINITY)
                        .id(egui::Id::new("command_palette_input"));
                    let response = ui.add(edit);
                    if fresh {
                        response.request_focus();
                    }
                    if response.changed() {
                        state.palette.selected = 0;
                    }
                    ui.add_space(6.0);
                    if entries.is_empty() {
                        ui.label(egui::RichText::new("No matching command or file").color(theme.text_muted));
                    }
                    egui::ScrollArea::vertical().max_height(VISIBLE_ROWS * ROW_HEIGHT).auto_shrink([false, true]).show(ui, |ui| {
                        for (i, entry) in entries.iter().enumerate() {
                            let active = i == state.palette.selected;
                            let row = entry_row(ui, &theme, entry, active);
                            if active && (down || up) {
                                row.scroll_to_me(None);
                            }
                            if row.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                                state.palette.selected = i;
                            }
                            if row.clicked() {
                                chosen = Some(entry.command.clone());
                            }
                        }
                    });
                });
        });

    if enter && let Some(entry) = entries.get(state.palette.selected) {
        chosen = Some(entry.command.clone());
    }
    let clicked_outside = ctx.input(|i| {
        i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p))
    });
    if chosen.is_some() || clicked_outside {
        state.show_command_palette = false;
        state.palette.was_open = false;
    }
    chosen
}

fn entry_row(ui: &mut egui::Ui, theme: &crate::ui::theme::Theme, entry: &Entry, active: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), egui::Sense::click());
    let label = entry.label.clone();
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    if active {
        ui.painter().rect_filled(rect, 5.0, theme.control_active);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 5.0, theme.control_bg);
    }
    let font = FontId::proportional(12.5);
    let mut job = egui::text::LayoutJob::default();
    for (i, ch) in entry.label.chars().enumerate() {
        let color: Color32 = if entry.matched.indices.contains(&i) { theme.accent } else { theme.text_primary };
        job.append(&ch.to_string(), 0.0, egui::TextFormat::simple(font.clone(), color));
    }
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(egui::pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0), galley, theme.text_primary);
    if !entry.hint.is_empty() {
        ui.painter().text(
            egui::pos2(rect.right() - 8.0, rect.center().y),
            Align2::RIGHT_CENTER,
            entry.hint,
            FontId::proportional(10.5),
            theme.text_muted,
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(state: &AppState, query: &str) -> Vec<String> {
        entries(state, query).into_iter().map(|e| e.label).collect()
    }

    #[test]
    fn empty_query_lists_commands_only() {
        let state = AppState::default();
        let labels = labels(&state, "");
        assert!(labels.contains(&"Open folder…".to_owned()));
        assert!(labels.contains(&"Show shortcuts".to_owned()));
    }

    #[test]
    fn commands_that_need_nodes_are_hidden_without_them() {
        let state = AppState::default();
        assert!(!labels(&state, "").contains(&"Fit view to nodes".to_owned()));
        assert!(labels(&state, "fit").is_empty());
    }

    #[test]
    fn typing_filters_and_ranks_panel_toggles() {
        let state = AppState::default();
        let found = labels(&state, "cst");
        assert_eq!(found.first().map(String::as_str), Some("Toggle CST panel"));
        assert!(!found.contains(&"Open folder…".to_owned()));
    }
}
