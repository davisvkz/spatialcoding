//! Left side: a thin always-visible icon rail (Workspace / Changes / Search /
//! Local Server) plus a floating overlay panel for the active entry. Footer of
//! the rail holds Bug / Help / Settings / User shortcuts.

use crate::fs::tree::TreeNode;
use crate::state::{AppState, LeftPanel};
use crate::ui::icons;
use crate::ui::panel::{
    bubble_frame, panel_header, panel_slot_frame, rail_frame, HeaderSpec, PANEL_WIDTH, RAIL_WIDTH,
};
use crate::ui::{changes_panel, local_server_panel, search_panel};

const PANELS: &[(&str, &str, LeftPanel)] = &[
    (icons::FOLDER_TREE, "Workspace", LeftPanel::Workspace),
    (icons::GIT_BRANCH, "Changes", LeftPanel::Changes),
    (icons::SEARCH, "Search", LeftPanel::Search),
    (icons::SERVER, "Local Server", LeftPanel::LocalServer),
];

pub fn draw_sidebar_left(
    ctx: &egui::Context,
    state: &mut AppState,
    tree: &mut Option<TreeNode>,
) {
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
                    if icons::rail_icon_button(ui, ch, label, active).clicked() {
                        state.left_panel = if active { None } else { Some(panel) };
                    }
                }
            });

            // Footer actions at the bottom of the rail. Labels are
            // rail-specific so they don't collide with the toolbar's own
            // Settings/Help buttons in accessibility queries.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                if icons::rail_icon_button(ui, icons::USER, "Account", false).clicked() {
                    // TODO: user/account dialog.
                }
                if icons::rail_icon_button(ui, icons::SETTINGS, "App Preferences", state.show_settings)
                    .clicked()
                {
                    state.show_settings = !state.show_settings;
                }
                if icons::rail_icon_button(ui, icons::HELP, "Help & Shortcuts", state.show_shortcuts).clicked()
                {
                    state.show_shortcuts = !state.show_shortcuts;
                }
                icons::rail_icon_button(ui, icons::BUG, "Report Bug", false);
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

    egui::SidePanel::left("sidebar_left_panel")
        .exact_width(PANEL_WIDTH)
        .resizable(false)
        .show_separator_line(false)
        .frame(panel_slot_frame(&ctx.style()))
        .show(ctx, |ui| {
            bubble_frame(&theme).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if panel_header(ui, HeaderSpec::new(icon_ch, title)).close {
                    state.left_panel = None;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match panel {
                        LeftPanel::Workspace => draw_workspace(ui, state, tree),
                        LeftPanel::Changes => changes_panel::draw_changes_panel(ui),
                        LeftPanel::Search => search_panel::draw_search_panel(ui),
                        LeftPanel::LocalServer => local_server_panel::draw_local_server_panel(ui, state),
                    });
            });
        });
}

fn draw_workspace(ui: &mut egui::Ui, state: &mut AppState, tree: &mut Option<TreeNode>) {
    let Some(root) = tree else {
        ui.weak("No folder loaded");
        return;
    };
    // v2 shows an "Open All N" CTA at the top of the Workspace panel.
    let count = count_files_root(root);
    if ui.button(format!("Open All {count}")).clicked() {
        crate::workspace::open_all_files(state);
    }
    ui.add_space(6.0);
    draw_tree_node(ui, root, state);
}

fn count_files_root(node: &TreeNode) -> usize {
    match node {
        TreeNode::File { .. } => 1,
        TreeNode::Dir { children, .. } => count_files(children),
    }
}

fn draw_tree_node(ui: &mut egui::Ui, node: &mut TreeNode, state: &mut AppState) {
    match node {
        TreeNode::Dir {
            name,
            path,
            children,
            expanded,
            ..
        } => {
            use egui::collapsing_header::CollapsingState;

            let file_count = count_files(children);
            let header = format!("{name} ({file_count})");
            let id = ui.make_persistent_id(&*path);
            let (_, header_response, _) =
                CollapsingState::load_with_default_open(ui.ctx(), id, *expanded)
                    .show_header(ui, |ui| ui.selectable_label(false, &header).clicked())
                    .body(|ui| {
                        for child in children.iter_mut() {
                            draw_tree_node(ui, child, state);
                        }
                    });

            if let Some(mut collapsing) = CollapsingState::load(ui.ctx(), id) {
                if header_response.inner {
                    collapsing.toggle(ui);
                    collapsing.store(ui.ctx());
                }
                *expanded = collapsing.is_open();
            }
        }
        TreeNode::File { name, path } => {
            let icon = file_icon(name);
            let label = format!("{icon} {name}");
            if ui.selectable_label(false, &label).clicked() {
                crate::workspace::open_file(state, path);
            }
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

fn file_icon(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("rs") => "\u{1f9f1}",
        Some("toml") => "\u{2699}",
        Some("md") => "\u{1f4dd}",
        Some("json") => "\u{1f4cb}",
        Some("js" | "ts" | "jsx" | "tsx") => "\u{1f4dc}",
        Some("py") => "\u{1f40d}",
        Some("html" | "css") => "\u{1f310}",
        _ => "\u{1f4c4}",
    }
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
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
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
