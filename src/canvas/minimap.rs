use egui::{Color32, Pos2, Rect, Ui, Vec2};

use super::viewport::Viewport;

#[derive(Clone)]
pub struct Minimap {
    pub enabled: bool,
    pub size: Vec2,
}

impl Default for Minimap {
    fn default() -> Self {
        Self {
            enabled: true,
            size: Vec2::new(200.0, 150.0),
        }
    }
}

impl Minimap {
    pub fn draw(
        &self,
        ui: &mut Ui,
        viewport: &Viewport,
        node_rects: &[Rect],
        canvas_rect: Rect,
    ) {
        if !self.enabled || node_rects.is_empty() {
            return;
        }

        let minimap_rect = Rect::from_min_size(
            Pos2::new(
                canvas_rect.max.x - self.size.x - 6.0,
                canvas_rect.max.y - self.size.y - 6.0,
            ),
            self.size,
        );

        let painter = ui.painter_at(minimap_rect);

        // v2 minimap: dark rounded card, light blocks for nodes and a white
        // rounded outline for the viewport.
        painter.rect_filled(minimap_rect, 10.0, Color32::from_rgba_unmultiplied(0x16, 0x16, 0x18, 235));
        painter.rect_stroke(
            minimap_rect,
            10.0,
            egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0x27, 0x27, 0x2a, 178)),
            egui::StrokeKind::Inside,
        );

        let world_bounds = bounding_rect(node_rects);
        let padded = Rect::from_min_max(
            world_bounds.min - Vec2::splat(100.0),
            world_bounds.max + Vec2::splat(100.0),
        );

        let map_to_minimap = |world_pos: Pos2| -> Pos2 {
            let nx = (world_pos.x - padded.min.x) / padded.width();
            let ny = (world_pos.y - padded.min.y) / padded.height();
            let inner = minimap_rect.shrink(6.0);
            Pos2::new(
                inner.min.x + nx * inner.width(),
                inner.min.y + ny * inner.height(),
            )
        };

        for r in node_rects {
            let min = map_to_minimap(r.min);
            let max = map_to_minimap(r.max);
            let mapped = Rect::from_min_max(min, max);
            painter.rect_filled(mapped, 1.0, Color32::from_rgba_unmultiplied(0xa1, 0xa1, 0xaa, 90));
        }

        let visible = viewport.visible_world_rect(canvas_rect);
        let vis_min = map_to_minimap(visible.min);
        let vis_max = map_to_minimap(visible.max);
        let vis_rect = Rect::from_min_max(vis_min, vis_max).intersect(minimap_rect.shrink(6.0));

        if vis_rect.is_positive() {
            painter.rect_stroke(
                vis_rect,
                3.0,
                egui::Stroke::new(1.5_f32, Color32::from_white_alpha(190)),
                egui::StrokeKind::Outside,
            );
        }
    }
}

fn bounding_rect(rects: &[Rect]) -> Rect {
    let mut min = Pos2::new(f32::MAX, f32::MAX);
    let mut max = Pos2::new(f32::MIN, f32::MIN);

    for r in rects {
        min.x = min.x.min(r.min.x);
        min.y = min.y.min(r.min.y);
        max.x = max.x.max(r.max.x);
        max.y = max.y.max(r.max.y);
    }

    Rect::from_min_max(min, max)
}
