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
    /// Draws the minimap. Returns the world point to center the view on when
    /// the user clicks or drags inside it.
    pub fn draw(
        &self,
        ui: &mut Ui,
        viewport: &Viewport,
        node_rects: &[Rect],
        canvas_rect: Rect,
    ) -> Option<Pos2> {
        if !self.enabled || node_rects.is_empty() {
            return None;
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

        // Click or drag to move the view: the pointer is the new center.
        let response = ui.interact(minimap_rect, egui::Id::new("minimap"), egui::Sense::click_and_drag());
        let target = response.interact_pointer_pos().filter(|_| response.clicked() || response.dragged());
        target.map(|pos| minimap_to_world(pos, minimap_rect.shrink(6.0), padded))
    }
}

/// The world point under `pos` of a minimap whose map area is `inner` and
/// whose world extent is `padded`.
pub fn minimap_to_world(pos: Pos2, inner: Rect, padded: Rect) -> Pos2 {
    let nx = ((pos.x - inner.min.x) / inner.width()).clamp(0.0, 1.0);
    let ny = ((pos.y - inner.min.y) / inner.height()).clamp(0.0, 1.0);
    Pos2::new(padded.min.x + nx * padded.width(), padded.min.y + ny * padded.height())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minimap_point_maps_back_to_the_world_and_clamps_at_the_edges() {
        let inner = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(100.0, 50.0));
        let padded = Rect::from_min_size(Pos2::new(-100.0, 0.0), Vec2::new(1000.0, 500.0));
        assert_eq!(minimap_to_world(inner.center(), inner, padded), padded.center());
        assert_eq!(minimap_to_world(inner.min, inner, padded), padded.min);
        assert_eq!(minimap_to_world(Pos2::new(9999.0, -50.0), inner, padded), Pos2::new(padded.max.x, padded.min.y));
    }
}
