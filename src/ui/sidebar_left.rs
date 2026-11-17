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
                if icons::rail_icon_button(ui, icons::USER, "Sign in", false).clicked() {
                    // TODO: user/account dialog.
                }
                if icons::rail_icon_button(ui, icons::SETTINGS, "Settings", state.show_settings)
                    .clicked()
                {
                    state.show_settings = !state.show_settings;
                }
                if icons::rail_icon_button(ui, icons::HELP, "Help", state.show_shortcuts).clicked()
                {
                    state.show_shortcuts = !state.show_shortcuts;
                }
                icons::rail_icon_button(ui, icons::BUG, "Send feedback", false);
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
                        LeftPanel::Changes => changes_panel::draw_changes_panel(ui, state),
                        LeftPanel::Search => search_panel::draw_search_panel(ui, state),
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
    if cta_button(ui, &format!("Open All {count}"), false).clicked() {
        crate::workspace::open_all_files(state);
    }
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
            path: _,
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
                count: Some(file_count),
                label: &label,
            };
            if tree_row(ui, &state.theme, row).clicked() {
                *expanded = !*expanded;
            }
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
            if tree_row(ui, &state.theme, row).clicked() {
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
fn tree_row(ui: &mut egui::Ui, theme: &crate::ui::theme::Theme, row: TreeRow<'_>) -> egui::Response {
    use egui::{Align2, FontId};
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
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
        let ch = if open { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT };
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
