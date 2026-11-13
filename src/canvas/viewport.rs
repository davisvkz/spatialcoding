use egui::{Pos2, Rect, Vec2};

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 5.0;

#[derive(Clone)]
pub struct Viewport {
    pub offset: Vec2,
    pub zoom: f32,
    pub target_zoom: f32,
    pub zoom_center: Option<Pos2>,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            offset: Vec2::ZERO,
            zoom: 1.0,
            target_zoom: 1.0,
            zoom_center: None,
        }
    }
}

impl Viewport {
    pub fn world_to_screen(&self, world_pos: Pos2) -> Pos2 {
        Pos2::new(
            (world_pos.x + self.offset.x) * self.zoom,
            (world_pos.y + self.offset.y) * self.zoom,
        )
    }

    pub fn screen_to_world(&self, screen_pos: Pos2) -> Pos2 {
        Pos2::new(
            screen_pos.x / self.zoom - self.offset.x,
            screen_pos.y / self.zoom - self.offset.y,
        )
    }

    pub fn world_to_screen_rect(&self, rect: Rect) -> Rect {
        Rect::from_min_max(
            self.world_to_screen(rect.min),
            self.world_to_screen(rect.max),
        )
    }

    pub fn handle_input(&mut self, response: &egui::Response, canvas_rect: Rect, space_held: bool) {
        // Pan: right-click drag, or Space + left-click drag
        if response.dragged_by(egui::PointerButton::Secondary)
            || (space_held && response.dragged_by(egui::PointerButton::Primary))
        {
            let delta = response.drag_delta();
            self.offset += delta / self.zoom;
        }

        let ui = &response.ctx;
        let hovered = canvas_rect.contains(ui.input(|i| i.pointer.hover_pos()).unwrap_or_default());
        if !hovered {
            return;
        }

        let (smooth_scroll, raw_scroll, ctrl) = ui.input(|i| {
            (
                i.smooth_scroll_delta,
                i.raw_scroll_delta,
                i.modifiers.ctrl || i.modifiers.command,
            )
        });

        // Pick the best scroll delta available
        let scroll_y = if raw_scroll.y.abs() > 0.1 { raw_scroll.y } else { smooth_scroll.y };
        let scroll_x = if raw_scroll.x.abs() > 0.1 { raw_scroll.x } else { smooth_scroll.x };

        if ctrl && scroll_y.abs() > 0.1 {
            // Zoom centered on cursor
            let factor = (scroll_y * 0.005).exp();
            let new_zoom = (self.target_zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);

            if let Some(cursor) = ui.input(|i| i.pointer.hover_pos()) {
                self.zoom_center = Some(cursor);
                let world_before = self.screen_to_world(cursor);
                self.target_zoom = new_zoom;
                self.zoom = new_zoom;
                let world_after = self.screen_to_world(cursor);
                self.offset += world_after - world_before;
            }
        } else if scroll_y.abs() > 0.1 || scroll_x.abs() > 0.1 {
            // Pan via scroll (trackpad or mouse wheel)
            self.offset += Vec2::new(scroll_x, scroll_y) / self.zoom;
        }
    }

    pub fn animate(&mut self, dt: f32, smooth: bool) {
        if smooth {
            let t = (10.0 * dt).min(1.0);
            let prev_zoom = self.zoom;
            self.zoom += (self.target_zoom - self.zoom) * t;

            if let Some(center) = self.zoom_center {
                let world_point = Pos2::new(
                    center.x / prev_zoom - self.offset.x,
                    center.y / prev_zoom - self.offset.y,
                );
                self.offset = Vec2::new(
                    center.x / self.zoom - world_point.x,
                    center.y / self.zoom - world_point.y,
                );
            }

            if (self.zoom - self.target_zoom).abs() < 0.001 {
                self.zoom = self.target_zoom;
                self.zoom_center = None;
            }
        } else {
            self.zoom = self.target_zoom;
            self.zoom_center = None;
        }
    }

    pub fn fit_to_rect(&mut self, world_rect: Rect, screen_rect: Rect) {
        if world_rect.width() <= 0.0 || world_rect.height() <= 0.0 {
            return;
        }

        let padding = 40.0;
        let available = Vec2::new(
            screen_rect.width() - padding * 2.0,
            screen_rect.height() - padding * 2.0,
        );

        let scale_x = available.x / world_rect.width();
        let scale_y = available.y / world_rect.height();
        let new_zoom = scale_x.min(scale_y).clamp(MIN_ZOOM, MAX_ZOOM);

        self.zoom = new_zoom;
        self.target_zoom = new_zoom;

        let world_center = world_rect.center();
        let screen_center = screen_rect.center();
        self.offset = Vec2::new(
            screen_center.x / self.zoom - world_center.x,
            screen_center.y / self.zoom - world_center.y,
        );
        self.zoom_center = None;
    }

    pub fn visible_world_rect(&self, screen_rect: Rect) -> Rect {
        Rect::from_min_max(
            self.screen_to_world(screen_rect.min),
            self.screen_to_world(screen_rect.max),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: Pos2, b: Pos2) {
        assert!((a - b).length() < 1e-3, "({}, {}) != ({}, {})", a.x, a.y, b.x, b.y);
    }

    fn viewport(offset: Vec2, zoom: f32) -> Viewport {
        Viewport {
            offset,
            zoom,
            target_zoom: zoom,
            zoom_center: None,
        }
    }

    #[test]
    fn world_and_screen_conversions_are_inverse() {
        let vp = viewport(Vec2::new(30.0, -12.5), 2.5);
        let world = Pos2::new(17.0, -4.0);
        let screen = vp.world_to_screen(world);

        assert_close(screen, Pos2::new(47.0 * 2.5, -16.5 * 2.5));
        assert_close(vp.screen_to_world(screen), world);
    }

    #[test]
    fn world_to_screen_rect_transforms_both_corners() {
        let vp = viewport(Vec2::new(1.0, 1.0), 2.0);
        let rect = vp.world_to_screen_rect(Rect::from_min_max(
            Pos2::new(0.0, 0.0),
            Pos2::new(10.0, 20.0),
        ));
        assert_close(rect.min, Pos2::new(2.0, 2.0));
        assert_close(rect.max, Pos2::new(22.0, 42.0));
    }

    #[test]
    fn visible_world_rect_is_the_screen_in_world_space() {
        let vp = viewport(Vec2::new(10.0, 0.0), 2.0);
        let visible = vp.visible_world_rect(Rect::from_min_max(
            Pos2::ZERO,
            Pos2::new(200.0, 100.0),
        ));
        assert_close(visible.min, Pos2::new(-10.0, 0.0));
        assert_close(visible.max, Pos2::new(90.0, 50.0));
    }

    #[test]
    fn fit_to_rect_centers_and_fits_the_world_rect() {
        let mut vp = Viewport {
            zoom_center: Some(Pos2::new(5.0, 5.0)),
            ..Default::default()
        };
        let world = Rect::from_min_max(Pos2::new(100.0, 50.0), Pos2::new(1100.0, 550.0));
        // 40px padding on each side leaves 500x250 => zoom 0.5.
        let screen = Rect::from_min_max(Pos2::ZERO, Pos2::new(580.0, 330.0));

        vp.fit_to_rect(world, screen);

        assert!((vp.zoom - 0.5).abs() < 1e-6);
        assert_eq!(vp.zoom, vp.target_zoom);
        assert!(vp.zoom_center.is_none());
        assert_close(vp.world_to_screen(world.center()), screen.center());
        assert!(vp.visible_world_rect(screen).contains_rect(world));
    }

    #[test]
    fn fit_to_rect_clamps_zoom() {
        let screen = Rect::from_min_max(Pos2::ZERO, Pos2::new(1000.0, 800.0));

        let mut vp = Viewport::default();
        vp.fit_to_rect(Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), screen);
        assert_eq!(vp.zoom, MAX_ZOOM);

        vp.fit_to_rect(Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0e7, 1.0e7)), screen);
        assert_eq!(vp.zoom, MIN_ZOOM);
    }

    #[test]
    fn fit_to_rect_ignores_degenerate_world_rect() {
        let mut vp = viewport(Vec2::new(3.0, 4.0), 1.5);
        let screen = Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 600.0));
        vp.fit_to_rect(Rect::from_min_max(Pos2::ZERO, Pos2::new(0.0, 100.0)), screen);

        assert_eq!(vp.zoom, 1.5);
        assert_eq!(vp.offset, Vec2::new(3.0, 4.0));
    }

    #[test]
    fn animate_without_smoothing_jumps_to_target() {
        let mut vp = Viewport {
            target_zoom: 2.0,
            zoom_center: Some(Pos2::new(10.0, 10.0)),
            ..Default::default()
        };

        vp.animate(0.016, false);

        assert_eq!(vp.zoom, 2.0);
        assert!(vp.zoom_center.is_none());
    }

    #[test]
    fn smooth_animation_keeps_cursor_anchored_and_converges() {
        let center = Pos2::new(100.0, 80.0);
        let mut vp = viewport(Vec2::new(-20.0, 15.0), 1.0);
        vp.target_zoom = 2.0;
        vp.zoom_center = Some(center);
        let anchored = vp.screen_to_world(center);

        vp.animate(0.05, true);
        assert!(vp.zoom > 1.0 && vp.zoom < 2.0, "zoom {}", vp.zoom);
        assert_close(vp.screen_to_world(center), anchored);

        for _ in 0..50 {
            vp.animate(0.05, true);
        }
        assert_eq!(vp.zoom, 2.0);
        assert!(vp.zoom_center.is_none());
        // The final snap to `target_zoom` happens after the offset is anchored,
        // so allow a sub-pixel drift on screen.
        let drift = (vp.world_to_screen(anchored) - center).length();
        assert!(drift < 0.5, "cursor drifted {drift}px");
    }
}
