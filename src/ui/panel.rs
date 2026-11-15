//! Shared chrome for the floating side panels: overlay frame, standard header,
//! and the labeled "section" group used throughout the right panels.
//!
//! See v2.codecanvas.app for the visual reference (left/right rails each open
//! a floating card with `[icon] [title] [⋮] [▶] [×]` header and labeled
//! groups below).

use crate::ui::icons;
use crate::ui::theme::Theme;

/// Width of the always-visible icon rail on each side.
pub const RAIL_WIDTH: f32 = 36.0;
/// Width of the floating panel overlay.
pub const PANEL_WIDTH: f32 = 300.0;

/// A rounded dark frame that matches the v2 panel card look.
pub fn overlay_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::NONE
        .fill(theme.panel_bg)
        .stroke(egui::Stroke::new(1.0_f32, theme.panel_border))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(10, 10))
}

/// Draws the `[icon] [title] [⋮] [▶] [×]` header row. Returns `true` when
/// the user clicked the close (×) button so the caller can clear its panel
/// selection.
pub fn panel_header(ui: &mut egui::Ui, icon_ch: &str, title: &str) -> bool {
    let mut close_clicked = false;
    ui.horizontal(|ui| {
        ui.label(icons::icon(icon_ch, 14.0));
        ui.add_space(4.0);
        // "<Name> panel" so the header's accessible text does not collide
        // with the rail's short label ("Layout", "Edges", …) in e2e queries.
        ui.label(egui::RichText::new(format!("{title} panel")).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Labels use "Panel"/"Demo panel" suffixes so they don't collide
            // with the Settings-dialog Close button in accessibility queries.
            if icons::tool_icon_button(ui, icons::CLOSE, "Close Panel").clicked() {
                close_clicked = true;
            }
            icons::tool_icon_button(ui, icons::PLAY, "Demo Panel");
            icons::tool_icon_button(ui, icons::KEBAB, "Panel Menu");
        });
    });
    ui.add_space(4.0);
    ui.separator();
    close_clicked
}

/// A grouped, labeled "card section" — a muted heading over a subtle inner
/// frame, to match the "Advanced Settings", "Style", "Opacity", … groups in
/// the v2 panels.
pub fn section<R>(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(label).small().weak());
    let out = egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(4, 6))
        .show(ui, |ui| add(ui))
        .inner;
    ui.add_space(2.0);
    out
}
