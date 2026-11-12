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
