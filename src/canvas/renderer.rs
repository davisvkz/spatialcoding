use egui::{Color32, Painter, Pos2, Rect};

use super::viewport::Viewport;
use crate::ui::theme::Theme;

const GRID_SPACING: f32 = 50.0;

pub struct CanvasRenderer;

impl CanvasRenderer {
    pub fn draw_background(painter: &Painter, viewport: &Viewport, rect: Rect, theme: &Theme) {
        painter.rect_filled(rect, 0.0, theme.canvas_bg);

        let alpha = ((viewport.zoom - 0.15) / 0.35).clamp(0.0, 1.0);
        if alpha < 0.01 {
            return;
        }

        let dot_base_alpha = (40.0 * alpha) as u8;
        let dot_color = Color32::from_rgba_premultiplied(
            theme.grid_dot.r(),
            theme.grid_dot.g(),
            theme.grid_dot.b(),
            dot_base_alpha,
        );

        let world_rect = viewport.visible_world_rect(rect);

        let start_x = (world_rect.min.x / GRID_SPACING).floor() as i32;
        let end_x = (world_rect.max.x / GRID_SPACING).ceil() as i32;
        let start_y = (world_rect.min.y / GRID_SPACING).floor() as i32;
        let end_y = (world_rect.max.y / GRID_SPACING).ceil() as i32;

        let dot_radius = (1.2 * viewport.zoom).clamp(0.5, 2.5);

        for gx in start_x..=end_x {
            for gy in start_y..=end_y {
                let world_pos = Pos2::new(gx as f32 * GRID_SPACING, gy as f32 * GRID_SPACING);
                let screen_pos = viewport.world_to_screen(world_pos);

                if rect.contains(screen_pos) {
                    painter.circle_filled(screen_pos, dot_radius, dot_color);
                }
            }
        }
    }
}
