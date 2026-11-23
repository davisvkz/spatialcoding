//! Right-rail "Dependency Edges" panel: which relationships show, how edges
//! are drawn, the Git comparison and the barrel/re-export toggles. Visual
//! reference: v2.codecanvas.app.

use crate::analysis::dependencies::indexer::ServerProgress;
use crate::analysis::dependencies::local::{ImportIssueReason, ImportIssues};
use crate::analysis::lsp::manager::ServerStatus;
use crate::edges::edge_store::EdgeVisibility;
use crate::edges::style::{EdgeStyle, GradientGapMode, OpacityMode, RelationshipType};
use crate::state::AppState;
use crate::ui::panel::{section, segmented, slider_row, switch, Section};

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
        Some(ServerStatus::Ready) if p.done == p.files => {
            format!("{name}: {} files analyzed", p.files)
        }
        Some(ServerStatus::Ready) => format!("{name}: {counts}"),
    };
    (text, false)
}

/// v2's "Barrel and Re-export" info text, adapted to the languages this
/// app classifies.
const BARREL_HELP: &str = "Barrel files and Rust facade modules re-export symbols so they can be \
imported from one place. Barrel edges trace that intermediate public binding. Re-export edges \
connect the consumer directly to the original source.\n\nJavaScript and TypeScript use barrel \
index files, both inline (export { x } from './x') and import-then-export. Rust uses pub use in \
mod.rs, Python re-exports in __init__.py. A file counts as a barrel with at least 4 re-exports. \
Facade hops do not affect layout or cycle detection.";

pub fn draw_edges_panel(ui: &mut egui::Ui, state: &mut AppState) {
    let comparison_ready = state.git_comparison.is_ready();
    section(ui, "Visibility", |ui| {
        switch(ui, &mut state.edge_style.show_all, "All").on_hover_text(
            "Show all eligible edges instead of filtering by relationship type and scope. To hide every edge, use the switch beside the panel title.",
        );
        if !state.edge_style.show_all {
            visibility_filters(ui, state, comparison_ready);
        }
        ui.separator();
        switch(ui, &mut state.edge_style.highlight_reverse_layout, "Highlight Backwards Edges")
            .on_hover_text(
                "Draw in red the edges whose importer ends to the right of where the exported file starts.",
            );
    });

    import_diagnostics(ui, state.deps.import_issues());

    section(ui, "Style", |ui| {
        let style = &mut state.edge_style;
        ui.label("Style");
        segmented(
            ui,
            &mut style.style,
            &[
                (EdgeStyle::Bezier, "Bezier"),
                (EdgeStyle::Step, "Step"),
                (EdgeStyle::Line, "Line"),
                (EdgeStyle::Pcb, "PCB"),
            ],
        );
        slider_row(
            ui,
            "Curvature",
            &mut style.bezier_curvature,
            0.0..=2.0,
            |v| format!("{v:.2}"),
        )
        .on_hover_text("Bezier handle length as a multiple of the horizontal distance.");
        slider_row(
            ui,
            "Lightness",
            &mut style.base_lightness,
            0.0..=200.0,
            |v| format!("{v:.0}%"),
        );
        ui.label("Opacity");
        segmented(
            ui,
            &mut style.opacity_mode,
            &[(OpacityMode::Full, "Full"), (OpacityMode::Depth, "Depth")],
        );
        ui.add_space(4.0);
        switch(ui, &mut style.show_connection_markers, "Connection Markers");
        switch(ui, &mut style.show_arrow_heads, "Arrow Heads");
        ui.add_enabled_ui(style.show_arrow_heads, |ui| {
            ui.checkbox(&mut style.only_selected, "Only selected")
                .on_hover_text("Arrow heads only on the edges of the selection");
        });
        switch(ui, &mut style.scale_arrow_heads, "Scale Arrow Heads")
            .on_hover_text("Arrow heads shrink with the zoom, down to Min Size.");
        ui.add_enabled_ui(style.scale_arrow_heads, |ui| {
            slider_row(ui, "Min Size", &mut style.min_size_pct, 0.3..=1.0, percent);
        });
    });

    // The legend's switch and the body need the style at once: switch a copy.
    let mut use_gradient = state.edge_style.use_gradient;
    let gradient_was_on = use_gradient;
    Section::new("Gradient")
        .switch("Edge gradient", &mut use_gradient)
        .show(ui, |ui| {
            let style = &mut state.edge_style;
            ui.add_enabled_ui(gradient_was_on, |ui| {
                ui.label("Gradient Gap");
                segmented(
                    ui,
                    &mut style.gradient_gap_mode,
                    &[
                        (GradientGapMode::Percent, "%"),
                        (GradientGapMode::Fixed, "Fixed"),
                    ],
                );
                ui.add_enabled_ui(style.gradient_gap_mode == GradientGapMode::Percent, |ui| {
                    slider_row(ui, "Gap %", &mut style.gradient_gap_pct, 0.0..=1.0, percent);
                });
                ui.add_enabled_ui(style.gradient_gap_mode == GradientGapMode::Fixed, |ui| {
                    slider_row(
                        ui,
                        "Visible px",
                        &mut style.gradient_fixed_visible_px,
                        0.0..=5000.0,
                        |v| format!("{v:.0}"),
                    )
                    .on_hover_text("World px that stay visible at each end of the edge.");
                });
                slider_row(
                    ui,
                    "Min Dist",
                    &mut style.gradient_min_dist,
                    0.0..=100_000.0,
                    |v| format!("{v:.0}"),
                )
                .on_hover_text(
                    "Edges shorter than this many world px draw solid, without the gap.",
                );
            });
        });
    state.edge_style.use_gradient = use_gradient;

    section(ui, "Diff", |ui| {
        let baseline_label = ui.label("Git baseline");
        if ui
            .add(egui::TextEdit::singleline(&mut state.git_comparison.baseline).hint_text("HEAD"))
            .labelled_by(baseline_label.id)
            .changed()
        {
            crate::workspace::invalidate_git_comparison(state);
        }
        let can_refresh = state.loaded_dir.is_some()
            && !state.node_store.file_nodes.is_empty()
            && !state.git_comparison.is_loading();
        if ui
            .add_enabled(can_refresh, egui::Button::new("Refresh comparison"))
            .clicked()
        {
            crate::workspace::refresh_git_comparison(state);
        }
        let ready = state.git_comparison.is_ready();
        const NOT_READY: &str = "Refresh the Git comparison first.";
        ui.add_enabled_ui(ready, |ui| {
            switch(ui, &mut state.edge_style.diff_colors, "Diff Colors").on_hover_text(
                "Added = green; deleted = red. Other highlight colors are overridden only for proven changes.",
            );
            switch(ui, &mut state.edge_style.show_deleted, "Deleted").on_hover_text(
                "Baseline-only links use file headers, never obsolete identifier positions.",
            );
        })
        .response
        .on_disabled_hover_text(NOT_READY);
        use crate::git::comparison::ComparisonStatus;
        match &state.git_comparison.status {
            ComparisonStatus::Idle => {
                ui.weak("Not compared; refresh after changing the canvas.");
            }
            ComparisonStatus::Loading => {
                ui.spinner();
                ui.label("Comparing Git graph…");
            }
            ComparisonStatus::Unavailable(message) | ComparisonStatus::Error(message) => {
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
            ComparisonStatus::Ready => {
                if let Some(diff) = &state.git_comparison.diff {
                    let (added, deleted, unchanged) = diff.counts();
                    ui.label(format!(
                        "Baseline: {}",
                        &diff.commit[..diff.commit.len().min(12)]
                    ));
                    ui.label(format!("+{added} / −{deleted} / {unchanged} unchanged"));
                    let uncovered = state
                        .edge_store
                        .all_edges()
                        .filter(|e| e.change == crate::edges::edge_store::EdgeChange::Uncompared)
                        .count();
                    ui.weak(format!("{uncovered} symbol links not compared."));
                }
            }
        }
        ui.weak("Coverage: local module/document links, between open files only. LSP-only symbol links are not compared.");
        ui.weak("Canvas snapshot vs commit (not index). Reopen files after external edits.");
    });

    Section::new("Barrel and Re-export")
        .info(BARREL_HELP)
        .show(ui, |ui| {
            let style = &mut state.edge_style;
            facade_row(
                ui,
                (&mut style.show_reexport_edges, "Show re-export edges"),
                (&mut style.highlight_reexport, "Highlight re-export edges"),
                &mut style.reexport_color,
            );
            facade_row(
                ui,
                (&mut style.show_barrel_edges, "Show barrel edges"),
                (&mut style.highlight_barrel, "Highlight barrel edges"),
                &mut style.barrel_color,
            );
        });

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

    section(ui, "Ports", |ui| {
        let style = &mut state.edge_style;
        switch(ui, &mut style.show_ports, "Ports").on_hover_text(
            "Mark each connected line with a circle on the node border and a row band; \
             edges end at the circle. Off: edges touch the identifier.",
        );
        ui.add_enabled_ui(style.show_ports, |ui| {
            ui.horizontal(|ui| {
                ui.color_edit_button_rgb(&mut style.import_port_color);
                ui.label("Import");
                ui.color_edit_button_rgb(&mut style.symbol_port_color);
                ui.label("Symbol");
            });
        });
    });

    egui::CollapsingHeader::new("Symbol Colors")
        .default_open(true)
        .show(ui, |ui| {
            let mut counts: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
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

/// Why imports of the open files gave no edge: the ones nobody can follow
/// (packages) are only counted, the rest are listed.
fn import_diagnostics(ui: &mut egui::Ui, issues: &ImportIssues) {
    const MAX_ROWS: usize = 12;
    if issues.is_empty() {
        return;
    }
    Section::new("Unresolved imports (static)")
        .info("Imports and Rust module declarations of the open files that gave no file link, independent of the language server and visibility filters. A path resolves to any file of the folder, and the link is drawn when both files are open. Packages are only counted.")
        .show(ui, |ui| {
            match issues.unresolved() {
                0 => {}
                1 => {
                    ui.label("1 unresolved import");
                }
                n => {
                    ui.label(format!("{n} unresolved imports"));
                }
            }
            let by_reason: Vec<String> = issues
                .counts()
                .filter(|(reason, _)| *reason != ImportIssueReason::External)
                .map(|(reason, n)| format!("{} {n}", reason.short()))
                .collect();
            if by_reason.len() > 1 {
                ui.weak(by_reason.join(" \u{b7} "));
            }
            match issues.count(ImportIssueReason::External) {
                0 => {}
                1 => {
                    ui.weak("1 external import (a package, not drawn)");
                }
                n => {
                    ui.weak(format!("{n} external imports (packages, not drawn)"));
                }
            }
            for issue in issues.items().iter().take(MAX_ROWS) {
                ui.weak(format!(
                    "{}:{} \u{b7} {}: {}",
                    issue.path.display(),
                    issue.line,
                    issue.specifier,
                    issue.reason.label()
                ));
            }
            if issues.unresolved() > MAX_ROWS {
                ui.weak(format!("{} more not shown", issues.unresolved() - MAX_ROWS));
            }
        });
}

fn visibility_filters(ui: &mut egui::Ui, state: &mut AppState, comparison_ready: bool) {
    let style = &mut state.edge_style;
    egui::CollapsingHeader::new("Relationship types").show(ui, |ui| {
        for kind in RelationshipType::ALL {
            let settings = &mut style.relationships[kind.index()];
            ui.horizontal(|ui| {
                switch(ui, &mut settings.visible, kind.label());
                ui.color_edit_button_rgb(&mut settings.color);
            });
            ui.weak(kind.description());
        }
    });
    ui.weak("Highlights");
    ui.columns(2, |columns| {
        switch(&mut columns[0], &mut style.highlight_selected, "Selected").on_hover_text(
            "Show edges connected to the selection. Other filters can still show these edges.",
        );
        columns[1].add_enabled_ui(comparison_ready, |ui| {
            switch(ui, &mut style.highlight_changes, "Changes").on_disabled_hover_text(
                "Refresh the Git comparison below to show proven graph changes.",
            );
        });
    });
    switch(ui, &mut style.highlight_backwards, "Backwards")
        .on_hover_text("Show edges whose dependency direction runs right-to-left on the canvas.");
    ui.weak("Folder nesting levels");
    switch(ui, &mut style.folder_levels, "Levels")
        .on_hover_text("Show edges by bottom-up folder levels. L0 is leaf folders.");
    if style.folder_levels {
        let max = EdgeVisibility::max_folder_level(&state.node_store);
        let mut depth = style.folder_depth.clamp(-1, max);
        if slider_row(ui, "Depth", &mut depth, -1..=max, |v| {
            if v < 0.0 {
                "Off".into()
            } else {
                format!("L{v:.0}")
            }
        })
        .changed()
        {
            style.folder_depth = depth;
        }
    }
    switch(ui, &mut style.show_external, "External").on_hover_text(
        "Also show edges between files of different folders within the nesting limit.",
    );
    if style.show_external {
        slider_row(
            ui,
            "Nesting",
            &mut style.external_nesting_max,
            1..=20,
            |v| {
                if v >= 20.0 {
                    "No limit".into()
                } else {
                    format!("{v:.0}")
                }
            },
        );
    }
}

fn percent(v: f64) -> String {
    format!("{:.0}%", v * 100.0)
}

/// "Show … edges" switch with a "Highlight" checkbox and its color on the
/// right, as v2's Barrel and Re-export rows. The checkbox's accessible name
/// says what it highlights.
fn facade_row(
    ui: &mut egui::Ui,
    (show, show_label): (&mut bool, &str),
    (highlight, highlight_label): (&mut bool, &str),
    color: &mut [f32; 3],
) {
    ui.horizontal(|ui| {
        switch(ui, show, show_label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.color_edit_button_rgb(color);
            let response = ui.checkbox(highlight, "Highlight");
            let label = highlight_label.to_owned();
            let checked = *highlight;
            response.widget_info(move || {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, checked, label.clone())
            });
        });
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
        assert_eq!(
            progress_line(&progress(1, Some(ServerStatus::Indexing), None)),
            (format!("{name}: indexing\u{2026} 1/3 files"), false)
        );
        assert_eq!(
            progress_line(&progress(0, None, None)).0,
            format!("{name}: starting\u{2026} 0/3 files")
        );
        assert_eq!(
            progress_line(&progress(3, Some(ServerStatus::Ready), None)).0,
            format!("{name}: 3 files analyzed")
        );
        assert_eq!(
            progress_line(&progress(2, Some(ServerStatus::Ready), None)).0,
            format!("{name}: 2/3 files")
        );
        assert_eq!(
            progress_line(&progress(
                3,
                Some(ServerStatus::Failed("x".into())),
                Some("not found\nhint")
            )),
            (format!("{name}: not found"), true)
        );
    }
}
