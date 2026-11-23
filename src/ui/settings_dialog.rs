//! Settings window: Viewport, UI, Canvas File Nodes, Repository and Editor
//! groups, a reset per group (and for all), and the version.

use crate::state::{AppSettings, AppState};

/// Font size and node width ranges of v2 (Settings → Canvas File Nodes).
pub const FONT_SIZE_RANGE: std::ops::RangeInclusive<f32> = 8.0..=20.0;
pub const NODE_WIDTH_RANGE: std::ops::RangeInclusive<f32> = 300.0..=2100.0;

/// One group of the dialog, for resetting it alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Viewport,
    Ui,
    FileNodes,
    Repository,
    Performance,
    Editor,
}

/// Puts the settings of `group` back to their defaults.
pub fn reset_group(settings: &mut AppSettings, group: Group) {
    let d = AppSettings::default();
    match group {
        Group::Viewport => {
            settings.smooth_zoom = d.smooth_zoom;
            settings.show_viewport_details = d.show_viewport_details;
            settings.show_activity_indicator = d.show_activity_indicator;
        }
        Group::Ui => {
            settings.show_controls_bar = d.show_controls_bar;
            settings.show_minimap = d.show_minimap;
            settings.show_grid = d.show_grid;
        }
        Group::FileNodes => {
            settings.font_size = d.font_size;
            settings.node_width = d.node_width;
        }
        Group::Repository => settings.show_dot_folders = d.show_dot_folders,
        Group::Performance => settings.diff_scan_limit = d.diff_scan_limit,
        Group::Editor => settings.editor_command = d.editor_command,
    }
}

/// Every group, for "Reset all". Panel settings (CST, LSP, folders, files)
/// have their own reset in each panel's menu.
pub fn reset_all(settings: &mut AppSettings) {
    for group in [Group::Viewport, Group::Ui, Group::FileNodes, Group::Repository, Group::Performance, Group::Editor] {
        reset_group(settings, group);
    }
}

fn group_header(ui: &mut egui::Ui, title: &str, group: Group, settings: &mut AppSettings) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("↺").on_hover_text(format!("Reset {title}")).clicked() {
                reset_group(settings, group);
            }
        });
    });
}

/// Asks about a language server command a repository suggests in
/// `.codecanvas/settings.json`: once per exact command and folder.
pub fn draw_command_proposals(ctx: &egui::Context, state: &mut AppState) {
    use crate::analysis::lsp::project_config::{self, Answer};
    if let Some(root) = state.loaded_dir.clone() {
        project_config::sync_answers(&root, &state.settings.lsp_command_answers);
        state.command_proposals.extend(project_config::take_pending(&root));
    }
    let Some(proposal) = state.command_proposals.first().cloned() else { return };
    let mut answer: Option<bool> = None;
    let mut later = false;
    egui::Window::new("Run a language server command?")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.label(format!(
                "This repository asks SpatialCoding to start the {} language server with:",
                proposal.server.name()
            ));
            ui.add_space(4.0);
            ui.monospace(proposal.command.join(" "));
            ui.add_space(4.0);
            ui.weak(format!("Folder: {}", proposal.root.display()));
            ui.weak("The repository may belong to someone else: only allow commands you trust. The answer is remembered for this exact command and language in this folder; until then the default server runs.");
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Allow").clicked() {
                    answer = Some(true);
                }
                if ui.button("Never for this command").clicked() {
                    answer = Some(false);
                }
                if ui.button("Not now").clicked() {
                    later = true;
                }
            });
        });
    if answer.is_some() || later {
        state.command_proposals.remove(0);
    }
    if let Some(allowed) = answer {
        let language = project_config::language_key(proposal.server).to_string();
        state
            .settings
            .lsp_command_answers
            .retain(|a| !(a.root == proposal.root && a.language == language && a.command == proposal.command));
        state.settings.lsp_command_answers.push(Answer {
            root: proposal.root.clone(),
            language,
            command: proposal.command.clone(),
            allowed,
        });
        project_config::sync_answers(&proposal.root, &state.settings.lsp_command_answers);
        // The servers start again, now with (or without) the suggested command.
        state.lsp.manager.restart_servers();
    }
    if later {
        project_config::forget_asked(&proposal);
    }
}

/// Workspace › Ignored file settings: the repository's patterns.
pub fn draw_ignore_settings(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_ignore_settings {
        return;
    }
    let mut open = true;
    let mut remove: Option<String> = None;
    let mut add: Option<String> = None;
    let repo = state.loaded_dir.as_ref().map(|d| d.display().to_string());
    let patterns = state.ignore_patterns();
    egui::Window::new("Ignored file settings")
        .open(&mut open)
        .collapsible(false)
        .default_width(380.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            match &repo {
                Some(repo) => ui.weak(format!("Repository: {repo}")),
                None => ui.weak("Open a folder to keep an ignore list for it."),
            };
            ui.weak("gitignore syntax: node_modules, /dist/, *.log, **/gen/*.rs, !keep.js. Ignoring a configuration file only hides it.");
            ui.weak("Also hidden: build output, vendored code and lockfiles (dist, build, out, vendor, coverage, *.lock…) and what each .gitignore lists. Add !name to bring one back, or use Show ignored files.");
            ui.add_space(4.0);
            for pattern in &patterns {
                ui.horizontal(|ui| {
                    ui.monospace(pattern);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").on_hover_text("Stop ignoring").clicked() {
                            remove = Some(pattern.clone());
                        }
                    });
                });
            }
            if patterns.is_empty() {
                ui.weak("Nothing is ignored.");
            }
            ui.separator();
            let id = egui::Id::new("ignore_pattern_input");
            let mut text: String = ui.memory(|m| m.data.get_temp(id)).unwrap_or_default();
            ui.horizontal(|ui| {
                let edit = ui.add_enabled(
                    repo.is_some(),
                    egui::TextEdit::singleline(&mut text).hint_text("pattern").desired_width(240.0),
                );
                let submit = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.add_enabled(repo.is_some(), egui::Button::new("Add")).clicked() || submit) && !text.trim().is_empty() {
                    add = Some(text.trim().to_string());
                    text.clear();
                }
            });
            ui.memory_mut(|m| m.data.insert_temp(id, text));
        });
    let changed = remove.is_some() || add.is_some();
    if let Some(pattern) = remove {
        state.remove_ignore_pattern(&pattern);
    }
    if let Some(pattern) = add {
        state.add_ignore_pattern(pattern);
    }
    if changed {
        state.pending_app_actions.push(crate::commands::AppAction::ReloadTree);
    }
    state.show_ignore_settings = open;
}

pub fn draw_settings(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_settings {
        return;
    }

    egui::Window::new("Settings")
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            let s = &mut state.settings;

            group_header(ui, "Viewport", Group::Viewport, s);
            ui.checkbox(&mut s.smooth_zoom, "Smooth Zoom");
            ui.checkbox(&mut s.show_viewport_details, "Viewport Details")
                .on_hover_text("Zoom, level and position over the canvas.");
            ui.checkbox(&mut s.show_activity_indicator, "Activity Indicator")
                .on_hover_text("A line at the bottom while the language servers or Git work.");
            ui.add_space(6.0);

            group_header(ui, "UI", Group::Ui, s);
            ui.checkbox(&mut s.show_controls_bar, "Controls Bar")
                .on_hover_text("The Select, Move and Clear tools next to the repository selector.");
            ui.checkbox(&mut s.show_minimap, "Minimap");
            ui.checkbox(&mut s.show_grid, "Dot Grid");
            ui.add_space(6.0);

            group_header(ui, "Canvas File Nodes", Group::FileNodes, s);
            ui.label("Font Size");
            ui.add(egui::Slider::new(&mut s.font_size, FONT_SIZE_RANGE));
            ui.label("Node Width");
            ui.add(egui::Slider::new(&mut s.node_width, NODE_WIDTH_RANGE));
            ui.add_space(6.0);

            group_header(ui, "Repository", Group::Repository, s);
            ui.checkbox(&mut s.show_dot_folders, "Show dot-prefixed folders");
            ui.add_space(6.0);

            group_header(ui, "Performance", Group::Performance, s);
            ui.label("Diff Scan Limit");
            ui.add(egui::Slider::new(&mut s.diff_scan_limit, 100..=100_000).logarithmic(true).suffix(" lines"))
                .on_hover_text("A diff changing more lines than this waits behind a Large Diffs card.");
            ui.add_space(6.0);

            group_header(ui, "Editor", Group::Editor, s);
            ui.label("Open files with");
            ui.add(
                egui::TextEdit::singleline(&mut s.editor_command)
                    .hint_text("$VISUAL, $EDITOR (a command, e.g. code -g)")
                    .desired_width(f32::INFINITY),
            );

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Reset all").clicked() {
                    reset_all(s);
                }
                ui.label(
                    egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak(),
                );
                ui.add_enabled(false, egui::Button::new("Check for updates"))
                    .on_disabled_hover_text("Updates come from your package manager or cargo install.");
            });
            ui.separator();
            if ui.button("Close").clicked() {
                state.show_settings = false;
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_resets_only_its_own_settings() {
        let mut s = AppSettings { smooth_zoom: false, show_minimap: false, font_size: 18.0, ..Default::default() };
        reset_group(&mut s, Group::Viewport);
        assert!(s.smooth_zoom && !s.show_minimap && s.font_size == 18.0);
        reset_group(&mut s, Group::FileNodes);
        assert_eq!(s.font_size, 12.0);
    }

    #[test]
    fn reset_all_restores_the_dialogs_defaults_and_keeps_panel_settings() {
        let mut s = AppSettings {
            smooth_zoom: false,
            show_controls_bar: false,
            node_width: 2000.0,
            show_dot_folders: true,
            editor_command: "code".into(),
            ..Default::default()
        };
        s.folders.hide_folders = !s.folders.hide_folders;
        let panel = s.folders.hide_folders;
        reset_all(&mut s);
        let d = AppSettings::default();
        assert_eq!((s.smooth_zoom, s.show_controls_bar, s.node_width), (d.smooth_zoom, d.show_controls_bar, d.node_width));
        assert!(!s.show_dot_folders && s.editor_command.is_empty());
        assert_eq!(s.folders.hide_folders, panel);
    }
}
