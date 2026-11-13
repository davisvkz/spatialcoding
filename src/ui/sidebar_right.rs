use crate::analysis::dependencies::indexer::ServerProgress;
use crate::analysis::lsp::manager::ServerStatus;
use crate::edges::style::{EdgeStyle, OpacityMode};
use crate::layout::settings::{HAlign, SortMode, VAlign};
use crate::state::{AppState, RightPanel};
use crate::ui::{cst_panel, lsp_panel};

const PANELS: &[(&str, RightPanel)] = &[
    ("Layout", RightPanel::Layout),
    ("LSP", RightPanel::LspSymbols),
    ("Edges", RightPanel::DependencyEdges),
    ("Folders", RightPanel::Folders),
    ("Files", RightPanel::FileNodes),
    ("CST", RightPanel::Cst),
    ("Agent", RightPanel::AgentNotes),
    ("Perf", RightPanel::Performance),
];

pub fn draw_sidebar_right(ctx: &egui::Context, state: &mut AppState) {
    if state.right_panel != Some(RightPanel::Cst) {
        // The CST panel resets this each frame it draws; nothing else does.
        state.cst.hovered = None;
    }
    if state.right_panel != Some(RightPanel::LspSymbols) {
        state.lsp.hovered = None;
    }

    if state.right_panel.is_none() {
        egui::SidePanel::right("sidebar_right_collapsed")
            .exact_width(32.0)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if ui.selectable_label(false, "\u{25c0}").clicked() {
                        state.right_panel = Some(RightPanel::Layout);
                    }
                });
            });
        return;
    }

    egui::SidePanel::right("sidebar_right")
        .default_width(280.0)
        .width_range(200.0..=500.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for &(label, panel) in PANELS {
                    let active = state.right_panel == Some(panel);
                    if ui.selectable_label(active, label).clicked() {
                        if active {
                            state.right_panel = None;
                        } else {
                            state.right_panel = Some(panel);
                        }
                    }
                }
            });

            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                match state.right_panel {
                    Some(RightPanel::Layout) => draw_layout_panel(ui, state),
                    Some(RightPanel::DependencyEdges) => draw_edges_panel(ui, state),
                    Some(RightPanel::Cst) => cst_panel::draw_cst_panel(ui, state),
                    Some(RightPanel::LspSymbols) => lsp_panel::draw_lsp_panel(ui, state),
                    Some(_) => {
                        ui.label("Coming soon");
                    }
                    None => {}
                }
            });
        });
}

fn draw_layout_panel(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Layout");
    ui.separator();

    if ui.button("Apply Layout").clicked() {
        crate::layout::hierarchical::apply_layout(
            &mut state.node_store,
            &state.edge_store,
            &state.layout_settings,
        );
    }

    ui.add_space(8.0);
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

    ui.add_space(8.0);
    ui.label("Horizontal Align");
    ui.horizontal(|ui| {
        ui.radio_value(&mut state.layout_settings.h_align, HAlign::Left, "Left");
        ui.radio_value(&mut state.layout_settings.h_align, HAlign::Center, "Center");
        ui.radio_value(&mut state.layout_settings.h_align, HAlign::Right, "Right");
    });

    ui.label("Vertical Align");
    ui.horizontal(|ui| {
        ui.radio_value(&mut state.layout_settings.v_align, VAlign::Top, "Top");
        ui.radio_value(&mut state.layout_settings.v_align, VAlign::Center, "Center");
        ui.radio_value(&mut state.layout_settings.v_align, VAlign::Bottom, "Bottom");
        ui.radio_value(&mut state.layout_settings.v_align, VAlign::Fill, "Fill");
    });

    ui.add_space(8.0);
    ui.label("Sort Mode");
    ui.horizontal(|ui| {
        ui.radio_value(&mut state.layout_settings.sort, SortMode::Off, "Off");
        ui.radio_value(&mut state.layout_settings.sort, SortMode::Desc, "Desc");
        ui.radio_value(&mut state.layout_settings.sort, SortMode::Asc, "Asc");
    });
    ui.horizontal(|ui| {
        ui.radio_value(&mut state.layout_settings.sort, SortMode::Alt, "Alt");
        ui.radio_value(&mut state.layout_settings.sort, SortMode::AltRev, "AltRev");
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
    ui.heading("Dependency Edges");
    ui.separator();

    ui.label("Analysis (language servers)");
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
    ui.add_space(8.0);

    ui.label("Style");
    ui.horizontal(|ui| {
        ui.radio_value(&mut state.edge_style.style, EdgeStyle::Bezier, "Bezier");
        ui.radio_value(&mut state.edge_style.style, EdgeStyle::Step, "Step");
    });
    ui.horizontal(|ui| {
        ui.radio_value(&mut state.edge_style.style, EdgeStyle::Line, "Line");
        ui.radio_value(&mut state.edge_style.style, EdgeStyle::Pcb, "PCB");
    });

    ui.add_space(8.0);
    ui.label("Opacity Mode");
    ui.horizontal(|ui| {
        ui.radio_value(
            &mut state.edge_style.opacity_mode,
            OpacityMode::Full,
            "Full",
        );
        ui.radio_value(
            &mut state.edge_style.opacity_mode,
            OpacityMode::Depth,
            "Depth",
        );
    });

    ui.add_space(8.0);
    ui.checkbox(&mut state.edge_style.show_all, "Show All");
    ui.checkbox(
        &mut state.edge_style.highlight_backwards,
        "Highlight Backwards",
    );
    ui.checkbox(
        &mut state.edge_style.show_connection_markers,
        "Connection Markers",
    );
    ui.checkbox(&mut state.edge_style.show_arrow_heads, "Arrow Heads");
    ui.checkbox(&mut state.edge_style.only_selected, "Only Selected");
    ui.checkbox(
        &mut state.edge_style.scale_arrow_heads,
        "Scale Arrow Heads",
    );
    ui.checkbox(&mut state.edge_style.use_gradient, "Use Gradient");

    ui.add_space(8.0);
    ui.label("Min Size %");
    ui.add(egui::Slider::new(
        &mut state.edge_style.min_size_pct,
        0.0..=1.0,
    ));
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
