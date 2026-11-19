//! Right side: thin icon rail (Layout / LSP Symbols / Dependency Edges /
//! Folders / File nodes / CST / Agent notes / Performance) plus a docked panel
//! with a fixed header and scrollable body.

use crate::state::{AppState, RightPanel};
use crate::ui::icons;
use crate::ui::panel::{
    bubble_frame, panel_header, panel_slot_frame, rail_frame, HeaderSpec, PANEL_WIDTH, RAIL_WIDTH,
};
use crate::ui::panel_menus;
use crate::ui::{
    agent_panel, cst_panel, edges_panel, files_panel, folders_panel, layout_panel, lsp_panel,
    performance_panel,
};

const PANELS: &[(&str, &str, RightPanel)] = &[
    (icons::LAYOUT, "Layout", RightPanel::Layout),
    (icons::SYMBOLS, "LSP Symbols", RightPanel::LspSymbols),
    (icons::EDGES, "Dependency Edges", RightPanel::DependencyEdges),
    (icons::FOLDERS, "Folders", RightPanel::Folders),
    (icons::FILE_NODES, "File nodes", RightPanel::FileNodes),
    (icons::CST_TREE, "CST", RightPanel::Cst),
    (icons::AGENT_NOTES, "Agent notes", RightPanel::AgentNotes),
    (icons::PERF_GAUGE, "Performance", RightPanel::Performance),
];

pub fn draw_sidebar_right(ctx: &egui::Context, state: &mut AppState) {
    state.panel_rect_right = None;
    lsp_panel::draw_advanced_card(ctx, state);
    lsp_panel::draw_language_servers(ctx, state);
    // Cross-panel housekeeping: these highlight-caches only live while their
    // panel is open.
    if state.right_panel != Some(RightPanel::Cst) {
        state.cst.clear_highlights();
    }
    if state.right_panel != Some(RightPanel::LspSymbols) {
        state.lsp.hovered = None;
    }

    // 1. Always-visible icon rail, icons centered.
    egui::SidePanel::right("sidebar_right_rail")
        .exact_width(RAIL_WIDTH)
        .resizable(false)
        .show_separator_line(false)
        .frame(rail_frame(&ctx.style()))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                for &(ch, label, panel) in PANELS {
                    let active = state.right_panel == Some(panel);
                    if icons::rail_icon_button(ui, ch, label, active).clicked() {
                        state.right_panel = if active { None } else { Some(panel) };
                    }
                }
            });
        });

    // 2. Docked panel beside the rail.
    let Some(panel) = state.right_panel else {
        return;
    };
    let (icon_ch, title, _) = PANELS
        .iter()
        .copied()
        .find(|(_, _, p)| *p == panel)
        .expect("PANELS covers every RightPanel variant");
    let theme = state.theme.clone();

    let shown = egui::SidePanel::right("sidebar_right_panel")
        .exact_width(PANEL_WIDTH)
        .resizable(false)
        .show_separator_line(false)
        .frame(panel_slot_frame(&ctx.style()))
        .show(ctx, |ui| {
            bubble_frame(&theme).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let entries = panel_menus::menu(panel, state);
                let mut spec = HeaderSpec::new(icon_ch, title);
                spec.menu_label = panel_menus::menu_label(panel);
                spec.menu = entries.iter().map(|(item, _)| item.clone()).collect();
                if panel == RightPanel::Cst {
                    spec.switch = Some(("Toggle CST nodes", &mut state.settings.cst.show_canvas));
                }
                let header = panel_header(ui, spec);
                if let Some(i) = header.menu_clicked {
                    panel_menus::apply(entries[i].1, state);
                }
                if header.close {
                    state.right_panel = None;
                }
                // Each tab keeps its own offset. The header stays outside the
                // scroll area, and the docked SidePanel captures wheel input.
                egui::ScrollArea::vertical()
                    .id_salt(("right_panel_body", title))
                    .auto_shrink([false, false])
                    .show(ui, |ui| match panel {
                        RightPanel::Layout => layout_panel::draw_layout_panel(ui, state),
                        RightPanel::LspSymbols => lsp_panel::draw_lsp_panel(ui, state),
                        RightPanel::DependencyEdges => edges_panel::draw_edges_panel(ui, state),
                        RightPanel::Folders => folders_panel::draw_folders_panel(ui, state),
                        RightPanel::FileNodes => files_panel::draw_files_panel(ui, state),
                        RightPanel::Cst => cst_panel::draw_cst_panel(ui, state),
                        RightPanel::AgentNotes => agent_panel::draw_agent_panel(ui, state),
                        RightPanel::Performance => {
                            performance_panel::draw_performance_panel(ui, state)
                        }
                    });
            });
        });
    state.panel_rect_right = Some(shown.response.rect);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Space left for the canvas after the right sidebar draws. A SidePanel
    /// whose frame is wider than its exact width reserves the excess without
    /// painting it, which shows up as a black strip next to the canvas.
    fn canvas_rect_after(right_panel: Option<RightPanel>) -> egui::Rect {
        let ctx = egui::Context::default();
        icons::install(&ctx);
        let mut state = AppState::default();
        state.right_panel = right_panel;
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
                draw_sidebar_right(ctx, &mut state);
                canvas = ctx.available_rect();
            });
        }
        canvas
    }

    #[test]
    fn rail_and_panel_reserve_exactly_their_width() {
        assert_eq!(canvas_rect_after(None).max.x, 1280.0 - RAIL_WIDTH);
        for &(_, _, panel) in PANELS {
            assert_eq!(
                canvas_rect_after(Some(panel)).max.x,
                1280.0 - RAIL_WIDTH - PANEL_WIDTH,
                "{panel:?} panel is wider than its slot"
            );
        }
    }

}
