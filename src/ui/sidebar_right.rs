//! Right side: thin icon rail (Layout / LSP / Edges / Folders / Files / CST /
//! Agent / Perf) plus a floating overlay panel for the active entry.

use crate::analysis::dependencies::indexer::ServerProgress;
use crate::analysis::lsp::manager::ServerStatus;
use crate::edges::style::{EdgeStyle, GradientGapMode, OpacityMode};
use crate::layout::settings::{HAlign, SortMode, VAlign};
use crate::state::{AppState, RightPanel};
use crate::ui::icons;
use crate::ui::panel::{bubble_frame, panel_header, section, PANEL_WIDTH, RAIL_WIDTH};
use crate::ui::{agent_panel, cst_panel, files_panel, folders_panel, lsp_panel, performance_panel};

const PANELS: &[(&str, &str, RightPanel)] = &[
    (icons::LAYOUT, "Layout", RightPanel::Layout),
    (icons::SYMBOLS, "LSP", RightPanel::LspSymbols),
    (icons::EDGES, "Edges", RightPanel::DependencyEdges),
    (icons::FOLDERS, "Folders", RightPanel::Folders),
    (icons::FILE_NODES, "Files", RightPanel::FileNodes),
    (icons::CST_TREE, "CST", RightPanel::Cst),
    (icons::AGENT_NOTES, "Agent", RightPanel::AgentNotes),
    (icons::PERF_GAUGE, "Perf", RightPanel::Performance),
];

pub fn draw_sidebar_right(ctx: &egui::Context, state: &mut AppState) {
    // Cross-panel housekeeping: these highlight-caches only live while their
    // panel is open.
    if state.right_panel != Some(RightPanel::Cst) {
        state.cst.hovered = None;
    }
    if state.right_panel != Some(RightPanel::LspSymbols) {
        state.lsp.hovered = None;
    }

    // 1. Always-visible icon rail.
    egui::SidePanel::right("sidebar_right_rail")
        .exact_width(RAIL_WIDTH)
        .resizable(false)
        .show_separator_line(false)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(6.0);
            for &(ch, label, panel) in PANELS {
                let active = state.right_panel == Some(panel);
                if icons::rail_icon_button(ui, ch, label, active).clicked() {
                    state.right_panel = if active { None } else { Some(panel) };
                }
            }
        });

    // 2. Floating overlay.
    let Some(panel) = state.right_panel else {
        return;
    };
    let (icon_ch, title, _) = PANELS
        .iter()
        .copied()
        .find(|(_, _, p)| *p == panel)
        .expect("PANELS covers every RightPanel variant");
    let theme = state.theme.clone();

    // Docked SidePanel beside the right rail, styled as a bubble via its own
    // Frame. SidePanel owns its scroll/click events so the canvas no longer
    // moves while you scroll the panel body.
    egui::SidePanel::right("sidebar_right_panel")
        .exact_width(PANEL_WIDTH)
        .resizable(false)
        .show_separator_line(false)
        .frame(bubble_frame(&theme))
        .show(ctx, |ui| {
            if panel_header(ui, icon_ch, title) {
                state.right_panel = None;
            }
            // Intentionally no outer ScrollArea here — see decision-18:
            // wrapping panel bodies in an outer ScrollArea broke AccessKit
            // hover bounds on LSP row children. Each panel manages its own
            // scrolling as needed.
            match panel {
                RightPanel::Layout => draw_layout_panel(ui, state),
                RightPanel::LspSymbols => lsp_panel::draw_lsp_panel(ui, state),
                RightPanel::DependencyEdges => draw_edges_panel(ui, state),
                RightPanel::Folders => folders_panel::draw_folders_panel(ui, state),
                RightPanel::FileNodes => files_panel::draw_files_panel(ui, state),
                RightPanel::Cst => cst_panel::draw_cst_panel(ui, state),
                RightPanel::AgentNotes => agent_panel::draw_agent_panel(ui, state),
                RightPanel::Performance => performance_panel::draw_performance_panel(ui, state),
            }
        });
}

fn draw_layout_panel(ui: &mut egui::Ui, state: &mut AppState) {
    if ui
        .add(egui::Button::new("Apply Layout").min_size(egui::vec2(ui.available_width(), 28.0)))
        .clicked()
    {
        crate::layout::hierarchical::apply_layout(
            &mut state.node_store,
            &state.edge_store,
            &state.layout_settings,
        );
    }

    section(ui, "Advanced Settings", |ui| {
        ui.label("File Spacing");
        ui.add(egui::Slider::new(
            &mut state.layout_settings.file_spacing,
            50.0..=2000.0,
        ));
        ui.label("Folder Spacing");
        ui.add(egui::Slider::new(
            &mut state.layout_settings.folder_spacing,
            50.0..=2000.0,
        ));
        ui.label("Folder Padding");
        ui.add(egui::Slider::new(
            &mut state.layout_settings.folder_padding,
            0.0..=500.0,
        ));
    });

    section(ui, "Horizontal Align", |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(&mut state.layout_settings.h_align, HAlign::Left, "Left");
            ui.radio_value(&mut state.layout_settings.h_align, HAlign::Center, "Center");
            ui.radio_value(&mut state.layout_settings.h_align, HAlign::Right, "Right");
        });
    });

    section(ui, "Vertical Align", |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(&mut state.layout_settings.v_align, VAlign::Top, "Top");
            ui.radio_value(&mut state.layout_settings.v_align, VAlign::Center, "Center");
            ui.radio_value(&mut state.layout_settings.v_align, VAlign::Bottom, "Bottom");
            ui.radio_value(&mut state.layout_settings.v_align, VAlign::Fill, "Fill");
        });
    });

    section(ui, "Sort Mode", |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(&mut state.layout_settings.sort, SortMode::Off, "Off");
            ui.radio_value(&mut state.layout_settings.sort, SortMode::Desc, "Desc");
            ui.radio_value(&mut state.layout_settings.sort, SortMode::Asc, "Asc");
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut state.layout_settings.sort, SortMode::Alt, "Alt");
            ui.radio_value(&mut state.layout_settings.sort, SortMode::AltRev, "AltRev");
        });
    });
}

/// One line of the Edges tab about a language server's dependency analysis,
/// and whether it reports an error.
pub fn progress_line(p: &ServerProgress) -> (String, bool) {
    let name = p.server.name();
    if let Some(err) = &p.error {
        let first = err.lines().next().unwrap_or_default();
        return (format!("{name}: {first}"), true);
    }
    let counts = format!("{}/{} files", p.done, p.files);
    let text = match p.status {
        Some(ServerStatus::Starting) | None => format!("{name}: starting\u{2026} {counts}"),
        Some(ServerStatus::Indexing) => format!("{name}: indexing\u{2026} {counts}"),
        Some(ServerStatus::Failed(ref err)) => return (format!("{name}: {err}"), true),
        Some(ServerStatus::Ready) if p.done == p.files => format!("{name}: {} files analyzed", p.files),
        Some(ServerStatus::Ready) => format!("{name}: {counts}"),
    };
    (text, false)
}

fn draw_edges_panel(ui: &mut egui::Ui, state: &mut AppState) {
    section(ui, "Analysis (language servers)", |ui| {
        let progress = state.deps.progress(&state.lsp.manager);
        if progress.is_empty() {
            ui.weak("Open source files to find their dependencies.");
        }
        for p in &progress {
            match progress_line(p) {
                (text, true) => ui.colored_label(ui.visuals().error_fg_color, text),
                (text, false) => ui.label(text),
            };
        }
    });

    section(ui, "Style", |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(&mut state.edge_style.style, EdgeStyle::Bezier, "Bezier");
            ui.radio_value(&mut state.edge_style.style, EdgeStyle::Step, "Step");
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut state.edge_style.style, EdgeStyle::Line, "Line");
            ui.radio_value(&mut state.edge_style.style, EdgeStyle::Pcb, "PCB");
        });
    });

    section(ui, "Opacity", |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(&mut state.edge_style.opacity_mode, OpacityMode::Full, "Full");
            ui.radio_value(&mut state.edge_style.opacity_mode, OpacityMode::Depth, "Depth");
        });
    });

    section(ui, "Visibility", |ui| {
        ui.checkbox(&mut state.edge_style.show_all, "Show All");
        ui.checkbox(&mut state.edge_style.highlight_backwards, "Highlight Backwards");
        ui.checkbox(&mut state.edge_style.show_connection_markers, "Connection Markers");
        ui.checkbox(&mut state.edge_style.show_arrow_heads, "Arrow Heads");
        ui.checkbox(&mut state.edge_style.only_selected, "Only Selected");
        ui.checkbox(&mut state.edge_style.scale_arrow_heads, "Scale Arrow Heads");
        ui.label("Min Size %");
        ui.add(egui::Slider::new(&mut state.edge_style.min_size_pct, 0.0..=1.0));
    });

    section(ui, "Gradient", |ui| {
        ui.checkbox(&mut state.edge_style.use_gradient, "Use Gradient");
        if state.edge_style.use_gradient {
            ui.horizontal(|ui| {
                ui.radio_value(&mut state.edge_style.gradient_gap_mode, GradientGapMode::Percent, "%");
                ui.radio_value(&mut state.edge_style.gradient_gap_mode, GradientGapMode::Fixed, "Fixed");
            });
            match state.edge_style.gradient_gap_mode {
                GradientGapMode::Percent => {
                    ui.label("Gap %");
                    ui.add(egui::Slider::new(&mut state.edge_style.gradient_gap_pct, 0.0..=1.0));
                }
                GradientGapMode::Fixed => {
                    ui.label("Min Dist (px)");
                    ui.add(egui::Slider::new(&mut state.edge_style.gradient_min_dist_px, 0.0..=1000.0));
                }
            }
        }
    });

    egui::CollapsingHeader::new("Re-export & Barrel")
        .default_open(false)
        .show(ui, |ui| {
            ui.checkbox(&mut state.edge_style.show_reexport_edges, "Show re-export edges");
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.edge_style.highlight_reexport, "Highlight");
                ui.color_edit_button_rgb(&mut state.edge_style.reexport_color);
            });
            ui.add_space(4.0);
            ui.checkbox(&mut state.edge_style.show_barrel_edges, "Show barrel edges");
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.edge_style.highlight_barrel, "Highlight");
                ui.color_edit_button_rgb(&mut state.edge_style.barrel_color);
            });
            ui.add_space(4.0);
            ui.weak("Barrel = index.{ts,tsx,js,jsx,mjs,cjs} | mod.rs | __init__.py with \u{2265} 4 re-exports.");
        });

    egui::CollapsingHeader::new("Symbol Colors")
        .default_open(true)
        .show(ui, |ui| {
            let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
            for e in state.edge_store.all_edges() {
                if let Some(label) = &e.label {
                    *counts.entry(label.as_str()).or_default() += 1;
                }
            }
            let mut kinds: Vec<String> = state.edge_style.symbol_colors.keys().cloned().collect();
            kinds.sort();
            for kind in &kinds {
                let count = counts.get(kind.as_str()).copied().unwrap_or(0);
                ui.horizontal(|ui| {
                    if let Some(color) = state.edge_style.symbol_colors.get_mut(kind.as_str()) {
                        ui.color_edit_button_rgb(color);
                    }
                    ui.label(kind.as_str());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.weak(format!("{count}"));
                    });
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::lsp::ServerId;

    fn progress(done: usize, status: Option<ServerStatus>, error: Option<&str>) -> ServerProgress {
        ServerProgress {
            server: ServerId::Rust,
            files: 3,
            done,
            status,
            error: error.map(str::to_string),
        }
    }

    #[test]
    fn progress_line_reports_indexing_counts_and_errors() {
        let name = ServerId::Rust.name();
        assert_eq!(progress_line(&progress(1, Some(ServerStatus::Indexing), None)), (format!("{name}: indexing\u{2026} 1/3 files"), false));
        assert_eq!(progress_line(&progress(0, None, None)).0, format!("{name}: starting\u{2026} 0/3 files"));
        assert_eq!(progress_line(&progress(3, Some(ServerStatus::Ready), None)).0, format!("{name}: 3 files analyzed"));
        assert_eq!(progress_line(&progress(2, Some(ServerStatus::Ready), None)).0, format!("{name}: 2/3 files"));
        assert_eq!(
            progress_line(&progress(3, Some(ServerStatus::Failed("x".into())), Some("not found\nhint"))),
            (format!("{name}: not found"), true)
        );
    }
}
