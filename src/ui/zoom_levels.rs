//! Zoom Levels panel (Shift+V): the L5…L1 bar with the current zoom, the
//! pointer's world position, the screen size and the visible world rectangle.

use crate::nodes::lod::{LodLevel, MAX_ZOOM, MIN_ZOOM, THRESHOLDS};
use crate::state::AppState;

/// Position of `zoom` on the bar, 0 (left, L5) to 1 (right, L1), on a log
/// scale between the zoom limits.
pub fn bar_position(zoom: f32) -> f32 {
    let (lo, hi) = (MIN_ZOOM.ln(), MAX_ZOOM.ln());
    ((zoom.max(MIN_ZOOM).ln() - lo) / (hi - lo)).clamp(0.0, 1.0)
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_zoom_levels {
        return;
    }
    let zoom = state.viewport.zoom;
    let canvas = state.canvas_rect;
    let visible = state.viewport.visible_world_rect(canvas);
    let pointer_world = state.hover_screen.map(|p| state.viewport.screen_to_world(p));
    let level = LodLevel::from_zoom(zoom);
    let mut close = false;
    let mut reset = false;
    egui::Window::new("Zoom Levels")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -24.0])
        .show(ctx, |ui| {
            ui.set_width(300.0);
            let mono = |text: String| egui::RichText::new(text).monospace().small();
            ui.horizontal(|ui| {
                ui.label(mono(format!("zoom     {zoom:.3}x  L{}", level.number())));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Close").clicked() {
                        close = true;
                    }
                    if ui.small_button("↺").on_hover_text("Reset zoom levels to defaults").clicked() {
                        reset = true;
                    }
                });
            });
            let pos = pointer_world.map_or("-".to_string(), |p| format!("{:.0}, {:.0}", p.x, p.y));
            ui.label(mono(format!("pos      {pos}")));
            ui.label(mono(format!("screen   {:.0} × {:.0}", canvas.width(), canvas.height())));
            ui.label(mono(format!(
                "visible  {:.0},{:.0} → {:.0},{:.0}",
                visible.min.x, visible.min.y, visible.max.x, visible.max.y
            )));
            ui.add_space(4.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            let bar = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 8.0));
            // L5 on the left to L1 on the right, one color per level.
            let colors = [
                egui::Color32::from_rgb(0x4b, 0x55, 0x63),
                egui::Color32::from_rgb(0xef, 0x44, 0x44),
                egui::Color32::from_rgb(0xf9, 0x73, 0x16),
                egui::Color32::from_rgb(0xea, 0xb3, 0x08),
                egui::Color32::from_rgb(0x22, 0xc5, 0x5e),
            ];
            let mut edges = vec![0.0];
            edges.extend(THRESHOLDS.iter().rev().map(|&t| bar_position(t)));
            edges.push(1.0);
            for (i, color) in colors.iter().enumerate() {
                let seg = egui::Rect::from_min_max(
                    egui::pos2(bar.left() + bar.width() * edges[i], bar.top()),
                    egui::pos2(bar.left() + bar.width() * edges[i + 1], bar.bottom()),
                );
                painter.rect_filled(seg, 0.0, *color);
                painter.text(
                    egui::pos2(seg.center().x, bar.bottom() + 8.0),
                    egui::Align2::CENTER_CENTER,
                    format!("L{}", 5 - i),
                    egui::FontId::monospace(9.0),
                    egui::Color32::from_gray(150),
                );
            }
            let x = bar.left() + bar.width() * bar_position(zoom);
            painter.circle_filled(egui::pos2(x, bar.center().y), 5.0, egui::Color32::WHITE);
        });
    if reset {
        state.viewport.zoom = 1.0;
        state.viewport.target_zoom = 1.0;
        state.viewport.zoom_center = None;
    }
    if close {
        state.show_zoom_levels = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_runs_from_the_smallest_to_the_largest_zoom() {
        assert_eq!(bar_position(MIN_ZOOM), 0.0);
        assert!((bar_position(MAX_ZOOM) - 1.0).abs() < 1e-6);
        assert!(bar_position(0.04) < bar_position(0.30));
        assert!((bar_position(0.0316) - 0.5).abs() < 0.02, "log scale: geometric middle");
    }
}
