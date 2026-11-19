//! Left side: a thin always-visible icon rail (Workspace / Changes / Search /
//! Local Server) plus a floating overlay panel for the active entry. Footer of
//! the rail holds Bug / Help / Settings / User shortcuts.

use crate::fs::tree::TreeNode;
use crate::state::{AppState, LeftPanel};
use crate::ui::icons;
use crate::ui::panel::{
    bubble_frame, cta_button, panel_header, panel_slot_frame, rail_frame, HeaderSpec, PANEL_WIDTH,
    RAIL_WIDTH,
};
use crate::ui::{changes_panel, local_server_panel, search_panel};

const PANELS: &[(&str, &str, LeftPanel)] = &[
    (icons::FOLDER_TREE, "Workspace", LeftPanel::Workspace),
    (icons::GIT_BRANCH, "Changes", LeftPanel::Changes),
    (icons::SEARCH, "Search", LeftPanel::Search),
    (icons::PLUG, "Local Server", LeftPanel::LocalServer),
];

/// The orange dot of the Local Server icon while no agent is connected.
const AGENT_DOT: egui::Color32 = egui::Color32::from_rgb(0xf9, 0x73, 0x16);

pub fn draw_sidebar_left(ctx: &egui::Context, state: &mut AppState, tree: &mut Option<TreeNode>) {
    state.panel_rect_left = None;
    state.anchor_rects.clear();
    // 1. The always-visible icon rail, icons centered.
    egui::SidePanel::left("sidebar_left_rail")
        .exact_width(RAIL_WIDTH)
        .resizable(false)
        .show_separator_line(false)
        .frame(rail_frame(&ctx.style()))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                for &(ch, label, panel) in PANELS {
                    let active = state.left_panel == Some(panel);
                    let response = icons::rail_icon_button(ui, ch, label, active);
                    if panel == LeftPanel::LocalServer && state.agents.is_empty() {
                        // v2: an orange dot while no agent is connected.
                        let dot = response.rect.right_top() + egui::vec2(-7.0, 7.0);
                        ui.painter().circle_filled(dot, 3.5, AGENT_DOT);
                    }
                    if response.clicked() {
                        state.left_panel = if active { None } else { Some(panel) };
                    }
                }
            });

            // Footer actions at the bottom of the rail. Labels are
            // rail-specific so they don't collide with the toolbar's own
            // Settings/Help buttons in accessibility queries.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                if icons::rail_icon_button(ui, icons::USER, "Sign in", false).clicked() {
                    // TODO: user/account dialog.
                }
                let settings = icons::rail_icon_button(ui, icons::SETTINGS, "Settings", state.show_settings);
                crate::commands::note_anchor(state, "settings", settings.rect);
                if settings.clicked() {
                    state.show_settings = !state.show_settings;
                }
                if icons::rail_icon_button(ui, icons::HELP, "Help", state.show_shortcuts).clicked()
                {
                    state.show_shortcuts = !state.show_shortcuts;
                }
                if icons::rail_icon_button(ui, icons::BUG, "Send feedback", state.feedback.is_some()).clicked() {
                    state.feedback = match state.feedback.take() {
                        Some(_) => None,
                        None => Some(Default::default()),
                    };
                }
            });
        });

    // 2. Docked side panel beside the rail when a panel is active. A real
    // SidePanel (not a floating Area), so scrolling it never pans the canvas.
    let Some(panel) = state.left_panel else {
        return;
    };
    let theme = state.theme.clone();
    let (icon_ch, title, _) = PANELS
        .iter()
        .copied()
        .find(|(_, _, p)| *p == panel)
        .expect("PANELS covers every LeftPanel variant");

    let shown = egui::SidePanel::left("sidebar_left_panel")
        .exact_width(PANEL_WIDTH)
        .resizable(false)
        .show_separator_line(false)
        .frame(panel_slot_frame(&ctx.style()))
        .show(ctx, |ui| {
            bubble_frame(&theme).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                // Workspace kebab: Clear repository, Show folder file counts.
                let mut spec = HeaderSpec::new(icon_ch, title);
                let counts_label = if state.settings.show_folder_counts {
                    "Hide folder file counts"
                } else {
                    "Show folder file counts"
                };
                let menu_actions = [
                    WorkspaceMenu::ClearRepository,
                    WorkspaceMenu::ToggleCounts,
                    WorkspaceMenu::ToggleIgnored,
                    WorkspaceMenu::IgnoreSettings,
                ];
                if panel == LeftPanel::Workspace {
                    spec.menu = vec![
                        crate::ui::panel::MenuItem::new(icons::TRASH, "Clear repository")
                            .disabled_if(state.loaded_dir.is_none(), "No folder is loaded"),
                        crate::ui::panel::MenuItem::new(icons::FOLDERS, counts_label),
                        crate::ui::panel::MenuItem::new(
                            icons::SLASH,
                            if state.settings.show_ignored { "Hide ignored files" } else { "Show ignored files" },
                        ),
                        crate::ui::panel::MenuItem::new(icons::SETTINGS, "Ignored file settings"),
                    ];
                    spec.menu_label = Some("Workspace menu");
                }
                let header = panel_header(ui, spec);
                if let Some(i) = header.menu_clicked {
                    match menu_actions[i] {
                        WorkspaceMenu::ClearRepository => {
                            state.pending_app_actions.push(crate::commands::AppAction::CloseFolder);
                        }
                        WorkspaceMenu::ToggleCounts => {
                            state.settings.show_folder_counts = !state.settings.show_folder_counts;
                        }
                        WorkspaceMenu::ToggleIgnored => {
                            state.settings.show_ignored = !state.settings.show_ignored;
                            state.pending_app_actions.push(crate::commands::AppAction::ReloadTree);
                        }
                        WorkspaceMenu::IgnoreSettings => state.show_ignore_settings = true,
                    }
                }
                if header.close {
                    state.left_panel = None;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match panel {
                        LeftPanel::Workspace => draw_workspace(ui, state, tree),
                        LeftPanel::Changes => changes_panel::draw_changes_panel(ui, state),
                        LeftPanel::Search => search_panel::draw_search_panel(ui, state),
                        LeftPanel::LocalServer => {
                            local_server_panel::draw_local_server_panel(ui, state)
                        }
                    });
            });
        });
    state.panel_rect_left = Some(shown.response.rect);
}

fn draw_workspace(ui: &mut egui::Ui, state: &mut AppState, tree: &mut Option<TreeNode>) {
    let Some(root) = tree else {
        ui.weak("No folder loaded");
        return;
    };
    // v2 shows an "Open All N" CTA at the top of the Workspace panel.
    let count = count_files_root(root);
    if cta_button(ui, &format!("Open All {count}"), false).clicked() {
        crate::workspace::open_all_files(state);
    }
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        if ui.small_button("Expand all folders").clicked() {
            set_all_expanded(root, true);
        }
        if ui.small_button("Collapse").clicked() {
            set_all_expanded(root, false);
        }
    });
    ui.label(egui::RichText::new("Drag a folder onto the canvas").small().weak());
    ui.add_space(4.0);
    // v2 lists the folder's children directly, without a row for the root.
    match root {
        TreeNode::Dir { children, .. } => {
            ui.spacing_mut().item_spacing.y = 0.0;
            for child in children.iter_mut() {
                draw_tree_node(ui, child, state, 0);
            }
        }
        TreeNode::File { .. } => draw_tree_node(ui, root, state, 0),
    }
}

/// What a tree row carries while dragged: the file or folder path. The canvas
/// opens it on drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeDrag(pub std::path::PathBuf);

/// Which entry of the Workspace kebab was clicked.
#[derive(Clone, Copy)]
enum WorkspaceMenu {
    ClearRepository,
    ToggleCounts,
    ToggleIgnored,
    IgnoreSettings,
}

/// Adds `path` to the repository's ignore list and reloads the tree.
fn add_to_ignore_list(state: &mut AppState, path: &std::path::Path, is_dir: bool) {
    let Some(root) = state.loaded_dir.clone() else { return };
    let Some(pattern) = crate::fs::ignore::pattern_for(path, &root, is_dir) else { return };
    if state.add_ignore_pattern(pattern.clone()) {
        crate::ui::toast::push(state, format!("Ignoring {pattern}"));
        state.pending_app_actions.push(crate::commands::AppAction::ReloadTree);
    }
}

/// Opens or closes every folder row of the tree.
fn set_all_expanded(node: &mut TreeNode, expanded: bool) {
    if let TreeNode::Dir { children, expanded: open, .. } = node {
        *open = expanded;
        for child in children {
            set_all_expanded(child, expanded);
        }
    }
}

fn count_files_root(node: &TreeNode) -> usize {
    match node {
        TreeNode::File { .. } => 1,
        TreeNode::Dir { children, .. } => count_files(children),
    }
}

/// Height of a tree row and the indent per level, as in v2.
const ROW_HEIGHT: f32 = 22.0;
const INDENT: f32 = 12.0;

fn draw_tree_node(ui: &mut egui::Ui, node: &mut TreeNode, state: &mut AppState, depth: usize) {
    match node {
        TreeNode::Dir {
            name,
            path,
            children,
            expanded,
            ..
        } => {
            let file_count = count_files(children);
            let label = format!("{name} ({file_count})");
            let row = TreeRow {
                depth,
                chevron: Some(*expanded),
                badge: None,
                name,
                count: state.settings.show_folder_counts.then_some(file_count),
                label: &label,
            };
            let response = tree_row(ui, &state.theme, row);
            // Drag a folder onto the canvas to open it (the canvas takes the drop).
            response.dnd_set_drag_payload(TreeDrag(path.clone()));
            if response.clicked() {
                *expanded = !*expanded;
            }
            response.context_menu(|ui| {
                if ui.button("Open on canvas").clicked() {
                    crate::workspace::open_folder_on_canvas(state, path);
                    ui.close_menu();
                }
                if ui.button("Remove from canvas").clicked() {
                    crate::workspace::remove_folder_from_canvas(state, path);
                    ui.close_menu();
                }
                if ui.button("Add to ignore list…").clicked() {
                    add_to_ignore_list(state, path, true);
                    ui.close_menu();
                }
            });
            if *expanded {
                for child in children.iter_mut() {
                    draw_tree_node(ui, child, state, depth + 1);
                }
            }
        }
        TreeNode::File { name, path } => {
            let row = TreeRow {
                depth,
                chevron: None,
                badge: Some(file_badge(name)),
                name,
                count: None,
                label: name,
            };
            let response = tree_row(ui, &state.theme, row);
            if response.clicked() {
                crate::workspace::open_file(state, path);
            }
            response.dnd_set_drag_payload(TreeDrag(path.clone()));
            response.context_menu(|ui| {
                if ui.button("Open on canvas").clicked() {
                    crate::workspace::open_file(state, path);
                    ui.close_menu();
                }
                let open_node = state.node_store.file_nodes.values().find(|n| &n.path == path).map(|n| n.id);
                if ui.add_enabled(open_node.is_some(), egui::Button::new("Remove from canvas")).clicked() {
                    if let Some(id) = open_node {
                        crate::workspace::remove_nodes(state, &[id]);
                    }
                    ui.close_menu();
                }
                if ui.button("Add to ignore list…").clicked() {
                    add_to_ignore_list(state, path, false);
                    ui.close_menu();
                }
                let editor = crate::workspace::editor_command(state).is_some();
                if ui.add_enabled(editor, egui::Button::new("Open in editor")).on_disabled_hover_text("Set an editor in Settings or $EDITOR").clicked() {
                    if let Err(e) = crate::workspace::open_in_editor(state, path) {
                        crate::ui::toast::push(state, e);
                    }
                    ui.close_menu();
                }
            });
        }
    }
}

fn count_files(children: &[TreeNode]) -> usize {
    children
        .iter()
        .map(|c| match c {
            TreeNode::File { .. } => 1,
            TreeNode::Dir { children, .. } => count_files(children),
        })
        .sum()
}

/// A small colored marker in front of a file name.
struct Badge {
    text: &'static str,
    color: egui::Color32,
    /// `text` is a Lucide glyph, not letters.
    icon: bool,
}

fn file_badge(name: &str) -> Badge {
    let (text, rgb, icon) = match name.rsplit('.').next() {
        Some("ts" | "tsx") => ("TS", (0x3b, 0x82, 0xf6), false),
        Some("js" | "jsx" | "mjs" | "cjs") => ("JS", (0xfa, 0xcc, 0x15), false),
        Some("json") => ("{}", (0xfa, 0xcc, 0x15), false),
        Some("rs") => ("RS", (0xf9, 0x73, 0x16), false),
        Some("py") => ("PY", (0x60, 0xa5, 0xfa), false),
        Some("go") => ("GO", (0x22, 0xd3, 0xee), false),
        Some("md") => (icons::INFO, (0x60, 0xa5, 0xfa), true),
        Some("toml" | "yaml" | "yml") => (icons::SETTINGS, (0x71, 0x71, 0x7a), true),
        _ => (icons::FILE, (0x71, 0x71, 0x7a), true),
    };
    Badge {
        text,
        color: egui::Color32::from_rgb(rgb.0, rgb.1, rgb.2),
        icon,
    }
}

struct TreeRow<'a> {
    depth: usize,
    /// `Some(open)` draws a chevron (a folder).
    chevron: Option<bool>,
    badge: Option<Badge>,
    name: &'a str,
    count: Option<usize>,
    /// Accessible name of the row.
    label: &'a str,
}

/// One 22px row of the Workspace tree: chevron or file badge, 12px name and
/// a muted count on the right.
fn tree_row(
    ui: &mut egui::Ui,
    theme: &crate::ui::theme::Theme,
    row: TreeRow<'_>,
) -> egui::Response {
    use egui::{Align2, FontId};
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click_and_drag(),
    );
    let ak_label = row.label.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, ak_label.clone())
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, 4.0, theme.control_bg);
    }
    let mid = rect.center().y;
    let mut x = rect.left() + 8.0 + row.depth as f32 * INDENT;
    if let Some(open) = row.chevron {
        let ch = if open {
            icons::CHEVRON_DOWN
        } else {
            icons::CHEVRON_RIGHT
        };
        painter.text(
            egui::pos2(x + 6.0, mid),
            Align2::CENTER_CENTER,
            ch,
            FontId::new(11.0, egui::FontFamily::Name(icons::ICON_FAMILY.into())),
            theme.text_muted,
        );
        x += 16.0;
    } else if let Some(badge) = &row.badge {
        let font = if badge.icon {
            FontId::new(11.0, egui::FontFamily::Name(icons::ICON_FAMILY.into()))
        } else {
            FontId::new(8.5, egui::FontFamily::Name(icons::MEDIUM_FAMILY.into()))
        };
        painter.text(
            egui::pos2(x + 15.0, mid),
            Align2::CENTER_CENTER,
            badge.text,
            font,
            badge.color,
        );
        x += 26.0;
    }
    let right = rect.right() - 8.0;
    let count_w = if let Some(count) = row.count {
        painter
            .text(
                egui::pos2(right, mid),
                Align2::RIGHT_CENTER,
                count.to_string(),
                FontId::proportional(10.0),
                theme.text_muted,
            )
            .width()
            + 8.0
    } else {
        0.0
    };
    let clip = painter.with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(x, rect.top()),
        egui::pos2(right - count_w, rect.bottom()),
    ));
    clip.text(
        egui::pos2(x, mid),
        Align2::LEFT_CENTER,
        row.name,
        FontId::proportional(12.0),
        theme.text_primary,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Space left for the canvas after the left sidebar draws; see the
    /// matching test in sidebar_right.rs for why this must be exact.
    fn canvas_rect_after(left_panel: Option<LeftPanel>) -> egui::Rect {
        let ctx = egui::Context::default();
        icons::install(&ctx);
        let mut state = AppState::default();
        state.left_panel = left_panel;
        let mut tree = None;
        let mut canvas = egui::Rect::NOTHING;
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                draw_sidebar_left(ctx, &mut state, &mut tree);
                canvas = ctx.available_rect();
            });
        }
        canvas
    }

    #[test]
    fn rail_and_panel_reserve_exactly_their_width() {
        assert_eq!(canvas_rect_after(None).min.x, RAIL_WIDTH);
        for &(_, _, panel) in PANELS {
            assert_eq!(
                canvas_rect_after(Some(panel)).min.x,
                RAIL_WIDTH + PANEL_WIDTH,
                "{panel:?} panel is wider than its slot"
            );
        }
    }
}
