//! Right-rail "Folders" panel: folder collapse actions, the folder scope and
//! how folders open. Visual reference: v2.codecanvas.app.

use crate::folder_actions::{self, FolderAction};
use crate::git::changes::ChangesStatus;
use crate::state::AppState;
use crate::ui::icons;
use crate::ui::panel::{action_group, icon_action, section, slider_row, switch, Section};

pub fn draw_folders_panel(ui: &mut egui::Ui, state: &mut AppState) {
    section(ui, "Folder Actions", |ui| draw_actions(ui, state));

    section(ui, "Opening", |ui| {
        let folders = &mut state.settings.folders;
        switch(ui, &mut folders.open_collapsed_by_default, "Open with collapsed folders by default")
            .on_hover_text("When many files open at once, deep folders start collapsed.");
        switch(ui, &mut folders.auto_open, "Auto")
            .on_hover_text("Pick the collapse depth from the repository's depth.");
        let auto = folders.auto_open;
        if !auto {
            slider_row(ui, "Collapse depth", &mut folders.collapse_depth, 1..=12, |v| format!("{v:.0}"));
        }
        if state.loaded_dir.is_some() {
            let depth = folder_actions::repository_depth(state);
            let from = folder_actions::collapse_depth(state, depth);
            ui.weak(format!("Repository depth: {depth} levels"));
            if auto {
                ui.weak(format!("Folders from level {from} open collapsed."));
            }
        } else {
            ui.weak("Load a repository to set the collapse depth.");
        }
    });

    section(ui, "Display", |ui| {
        let folders = &mut state.settings.folders;
        switch(ui, &mut folders.hide_folders, "Hide Folders")
            .on_hover_text("No folder frames or names; collapsed folders still show their card.");
        switch(ui, &mut folders.merge_empty, "Merge Empty Folders")
            .on_hover_text("A folder holding only one folder shows as one frame named a/b.");
        switch(ui, &mut folders.merge_overlapping_labels, "Merge Overlapping Labels")
            .on_hover_text("Folder names that would overlap are joined into one.");
        switch(ui, &mut folders.show_folder_names, "Folder Names");
        ui.add_enabled_ui(folders.show_folder_names, |ui| {
            slider_row(ui, "Min Size", &mut folders.min_size_pct, 0.0..=1.0, |v| {
                format!("{:.0}%", v * 100.0)
            })
            .on_hover_text("Hide a folder name when it would be drawn smaller than this.");
        });
    });

    section(ui, "Diff", |ui| {
        switch(ui, &mut state.settings.folders.diff_highlight, "Diff Folder Highlight")
            .on_hover_text("Tint folders that hold files changed against Git HEAD.");
    });

    Section::new("Reveal Through Collapse").info(REVEAL_HELP).show(ui, |ui| {
        let folders = &mut state.settings.folders;
        switch(ui, &mut folders.reveal_selected, "Selected");
        switch(ui, &mut folders.reveal_diff_files, "Diff Files");
        switch(ui, &mut folders.reveal_diff_folders, "Diff Folders")
            .on_hover_text("Folders holding changed files never collapse.");
        switch(ui, &mut folders.reveal_backwards, "Backwards Edges")
            .on_hover_text("Files on an edge that runs right to left.");
    });

    Section::new("Advanced").collapsible(false).show(ui, |ui| {
        let folders = &mut state.settings.folders;
        switch(ui, &mut folders.collapse_edges_only, "Collapse: Edges Only")
            .on_hover_text("Collapsed folders keep showing their files; only their edges gather on the folder.");
        switch(ui, &mut folders.merge_collapsed_edges, "Merge Collapsed Edges")
            .on_hover_text("Edges that end on the same collapsed folders are drawn once.");
        switch(ui, &mut folders.auto_collapse_on_zoom, "Auto-Collapse on Zoom")
            .on_hover_text("Folders too small on screen show as solid blocks with their name.");
        switch(ui, &mut folders.zoom_fog, "Zoom Fog")
            .on_hover_text("Deeper folders fade as you zoom out.");
    });
}

/// v2's tooltip for Reveal Through Collapse.
const REVEAL_HELP: &str =
    "Nodes that have one of the following options will be visible even when the folders are collapsed.";

fn action_icon(action: FolderAction) -> &'static str {
    match action {
        FolderAction::CollapseDeepest => icons::CHEVRON_UP,
        FolderAction::ExpandShallowest => icons::CHEVRON_DOWN,
        FolderAction::CollapseLeaves => icons::CHEVRONS_UP,
        FolderAction::ExpandTopmost => icons::CHEVRONS_DOWN,
        FolderAction::CollapseAll => icons::FOLDER_CLOSED,
        FolderAction::ExpandAll => icons::FOLDER_OPEN,
        FolderAction::CollapseUnchanged => icons::FOLDER_UP,
        FolderAction::ExpandChanged => icons::FOLDER_DOWN,
        FolderAction::CloseUnchanged => icons::FOLDER_X,
        FolderAction::OpenChanged => icons::FOLDER_INPUT,
    }
}

fn draw_actions(ui: &mut egui::Ui, state: &mut AppState) {
    let changes_reason = match &state.changes.status {
        ChangesStatus::Ready => "No folder qualifies.",
        ChangesStatus::Idle | ChangesStatus::Loading => "Reading Git changes…",
        ChangesStatus::Unavailable(_) => "Git changes are unavailable for this folder.",
    };
    let applies: Vec<(FolderAction, bool)> =
        FolderAction::ALL.iter().map(|&a| (a, a.applies(state))).collect();
    let mut clicked = None;
    let mut group = |ui: &mut egui::Ui, actions: &[(FolderAction, bool)]| {
        action_group(ui, |ui| {
            for &(action, enabled) in actions {
                let reason = if action.uses_changes() { changes_reason } else { "No folder qualifies." };
                if icon_action(ui, enabled, action_icon(action), action.label(), reason) {
                    clicked = Some(action);
                }
            }
        });
    };
    ui.horizontal_wrapped(|ui| group(ui, &applies[..6]));
    ui.horizontal_wrapped(|ui| {
        group(ui, &applies[6..]);
        let has_scope = folder_actions::scope_dir(state).is_some();
        action_group(ui, |ui| {
            if icon_action(ui, has_scope, icons::SLASH, "Select folder scope", "Select a file node first.") {
                folder_actions::select_scope(state);
            }
        });
        let (files, dirs) = folder_actions::scope_files(state);
        let label = format!("Open folder scope: {} files in {dirs} folders", files.len());
        action_group(ui, |ui| {
            let text = format!("{} {}  {} {}", icons::FILE, files.len(), icons::FOLDER, dirs);
            let response = ui.add_enabled(!files.is_empty(), egui::Button::new(text).frame(false));
            let ak = label.clone();
            let enabled = !files.is_empty();
            response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, ak.clone()));
            let response = response
                .on_hover_text(&label)
                .on_disabled_hover_text("Select a file node to scope its folder.");
            if response.clicked() {
                folder_actions::open_scope(state);
            }
        });
    });
    if icons::tool_icon_button(ui, icons::REFRESH, "Rebuild edges").clicked() {
        crate::workspace::rebuild_edges(state);
    }
    ui.add(
        egui::Label::new(
            egui::RichText::new("Double-click a folder on the canvas to collapse or expand it.")
                .small()
                .weak(),
        )
        .wrap(),
    );
    if let Some(action) = clicked {
        action.run(state);
    }
}
