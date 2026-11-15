//! Shared chrome for the floating side panels: overlay frame, standard header,
//! and the labeled "section" group used throughout the right panels.
//!
//! See v2.codecanvas.app for the visual reference (left/right rails each open
//! a floating card with `[icon] [title] [⋮] [▶] [×]` header and labeled
//! groups below).

use crate::ui::icons;
use crate::ui::theme::Theme;

// A SidePanel clips its painting to `exact_width` but reserves whatever its
// frame actually measures, and it only subtracts `inner_margin` when sizing
// the content. Anything wider (content, stroke, outer margin) is reserved but
// never painted, so eframe's near-black clear color shows through. Keep every
// SidePanel frame here margin-only, with sizes that add up exactly.

/// Width of the always-visible icon rail: a 32 px button plus 4 px each side,
/// which also leaves room for the button's 1 px hover/selected expansion.
pub const RAIL_WIDTH: f32 = 40.0;
/// Width of the docked panel slot, gap included.
pub const PANEL_WIDTH: f32 = 300.0;

/// The rail's SidePanel frame: egui's default panel gray, content exactly 32 px.
pub fn rail_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style).inner_margin(egui::Margin::symmetric(4, 6))
}

/// The panel slot's SidePanel frame: the gray gap around the bubble. No
/// stroke and no outer margin, so its size adds up to `PANEL_WIDTH`.
pub fn panel_slot_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style).inner_margin(egui::Margin::same(6))
}

/// The visible rounded card, drawn inside [`panel_slot_frame`]. Never use it
/// as a SidePanel's own frame: its stroke would overflow the panel width.
pub fn bubble_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::NONE
        .fill(theme.panel_bg)
        .stroke(egui::Stroke::new(1.0_f32, theme.panel_border))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(8, 8))
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
