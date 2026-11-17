//! Right-rail "Layout" panel: Apply Layout and the Advanced Settings of the
//! hierarchical layout (see `layout::hierarchical`). Visual reference:
//! v2.codecanvas.app.

use crate::layout::settings::{HAlign, SortMode, VAlign};
use crate::state::AppState;
use crate::ui::panel::{cta_button, section, segmented, slider_row};

/// v2's tooltip for Rank Breaking.
const RANK_BREAKING_HELP: &str = "Splits tall columns so nodes can spread horizontally. \
Lower values break columns more aggressively. Off keeps all nodes in a single column per rank.";

pub fn draw_layout_panel(ui: &mut egui::Ui, state: &mut AppState) {
    if cta_button(ui, "Apply Layout", true).clicked() {
        crate::layout::hierarchical::apply_layout(
            &mut state.node_store,
            &state.edge_store,
            &state.layout_settings,
        );
        state.layout_dirty = false;
    }
    if !state.layout_settings.auto_layout {
        ui.label(
            egui::RichText::new("Auto Layout is off: files keep their place until you apply a layout.")
                .small()
                .weak(),
        );
    }

    section(ui, "Advanced Settings", |ui| {
        let settings = &mut state.layout_settings;
        slider_row(ui, "Rank Breaking", &mut settings.rank_breaking, 0..=8, |v| {
            if v == 0.0 { "Off".to_owned() } else { format!("{v}") }
        })
        .on_hover_text(RANK_BREAKING_HELP);
        ui.add_space(4.0);
        ui.label("H Align");
        segmented(
            ui,
            &mut settings.h_align,
            &[(HAlign::Left, "Left"), (HAlign::Center, "Center"), (HAlign::Right, "Right")],
        );
        ui.label("V Align");
        segmented(
            ui,
            &mut settings.v_align,
            &[
                (VAlign::Top, "Top"),
                (VAlign::Center, "Center"),
                (VAlign::Bottom, "Bottom"),
                (VAlign::Fill, "Fill"),
            ],
        );
        ui.label("Sort");
        segmented(
            ui,
            &mut settings.sort,
            &[
                (SortMode::Off, "Off"),
                (SortMode::Desc, "Desc"),
                (SortMode::Asc, "Asc"),
                (SortMode::Alt, "Alt"),
                (SortMode::AltRev, "Alt Rev"),
            ],
        );
        ui.add_space(4.0);
        let whole = |v: f64| format!("{v:.0}");
        slider_row(ui, "File Spacing", &mut settings.file_spacing, 0.0..=3000.0, whole);
        slider_row(ui, "Folder Spacing", &mut settings.folder_spacing, 0.0..=3000.0, whole);
        slider_row(ui, "Folder Padding", &mut settings.folder_padding, 0.0..=3000.0, whole);
        ui.add_space(4.0);
        ui.vertical_centered(|ui| {
            if ui.add(egui::Button::new("Reset Config").frame(false)).clicked() {
                *settings = crate::layout::settings::LayoutSettings {
                    // Reset Config restores the numbers, not the menu toggle.
                    auto_layout: settings.auto_layout,
                    ..Default::default()
                };
            }
        });
    });
}

