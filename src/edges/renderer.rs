use std::collections::HashSet;

use egui::{Color32, Pos2, Stroke, Vec2};

use crate::canvas::viewport::Viewport;
use crate::edges::edge_store::EdgeStore;
use crate::edges::style::{EdgeStyle, EdgeStyleConfig, OpacityMode};
use crate::nodes::node_store::NodeStore;

pub fn draw_edges(
    painter: &egui::Painter,
    edges: &EdgeStore,
    nodes: &NodeStore,
    viewport: &Viewport,
    config: &EdgeStyleConfig,
    selected_nodes: &HashSet<u64>,
    node_width: f32,
    font_size: f32,
) {
    let clip = painter.clip_rect();
    let visible_world = viewport.visible_world_rect(clip);

    for edge in edges.all_edges() {
        let src = match nodes.get_file(edge.source_node) {
            Some(n) => n,
            None => continue,
        };
        let tgt = match nodes.get_file(edge.target_node) {
            Some(n) => n,
            None => continue,
        };

        let src_rect = src.world_rect(node_width, font_size);
        let tgt_rect = tgt.world_rect(node_width, font_size);
        let src_center = src_rect.center();
        let tgt_center = tgt_rect.center();

        let edge_rect = egui::Rect::from_two_pos(src_center, tgt_center);
        if !visible_world.intersects(edge_rect) {
            continue;
        }

        let src_screen = viewport.world_to_screen(src_center);
        let tgt_screen = viewport.world_to_screen(tgt_center);

        let alpha = compute_alpha(config, edge, selected_nodes);
        if alpha < 2 {
            continue;
        }

        let color = Color32::from_rgba_unmultiplied(
            (config.base_color[0] * 255.0) as u8,
            (config.base_color[1] * 255.0) as u8,
            (config.base_color[2] * 255.0) as u8,
            alpha,
        );
        let stroke = Stroke::new(1.5_f32, color);

        match config.style {
            EdgeStyle::Bezier => draw_bezier(painter, src_screen, tgt_screen, stroke),
            EdgeStyle::Step => draw_step(painter, src_screen, tgt_screen, stroke),
            EdgeStyle::Line => { painter.line_segment([src_screen, tgt_screen], stroke); }
            EdgeStyle::Pcb => draw_pcb(painter, src_screen, tgt_screen, stroke),
        }

        if config.show_arrow_heads {
            draw_arrow_head(painter, src_screen, tgt_screen, color);
        }
    }
}

fn compute_alpha(
    config: &EdgeStyleConfig,
    edge: &crate::edges::edge_store::Edge,
    selected_nodes: &HashSet<u64>,
) -> u8 {
    let is_connected = selected_nodes.contains(&edge.source_node)
        || selected_nodes.contains(&edge.target_node);

    if config.only_selected && !is_connected {
        return 8;
    }

    match config.opacity_mode {
        OpacityMode::Full => 200,
        OpacityMode::Depth => {
            if is_connected { 200 } else { 60 }
        }
    }
}

fn draw_bezier(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke) {
    let dx = (to.x - from.x).abs() * 0.3;
    let cp1 = Pos2::new(from.x + dx, from.y);
    let cp2 = Pos2::new(to.x - dx, to.y);

    let bezier = egui::epaint::CubicBezierShape::from_points_stroke(
        [from, cp1, cp2, to],
        false,
        Color32::TRANSPARENT,
        stroke,
    );
    painter.add(bezier);
}

fn draw_step(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke) {
    let mid_x = (from.x + to.x) * 0.5;
    let p1 = Pos2::new(mid_x, from.y);
    let p2 = Pos2::new(mid_x, to.y);
    painter.line_segment([from, p1], stroke);
    painter.line_segment([p1, p2], stroke);
    painter.line_segment([p2, to], stroke);
}

fn draw_pcb(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke) {
    let mid_y = (from.y + to.y) * 0.5;
    let p1 = Pos2::new(from.x, mid_y);
    let p2 = Pos2::new(to.x, mid_y);
    painter.line_segment([from, p1], stroke);
    painter.line_segment([p1, p2], stroke);
    painter.line_segment([p2, to], stroke);
}

fn draw_arrow_head(painter: &egui::Painter, from: Pos2, to: Pos2, color: Color32) {
    let dir = Vec2::new(to.x - from.x, to.y - from.y);
    let len = dir.length();
    if len < 1.0 {
        return;
    }
    let dir = dir / len;
    let perp = Vec2::new(-dir.y, dir.x);

    let arrow_size = 8.0_f32;
    let tip = to;
    let left = tip - dir * arrow_size + perp * (arrow_size * 0.4);
    let right = tip - dir * arrow_size - perp * (arrow_size * 0.4);

    painter.add(egui::Shape::convex_polygon(
        vec![tip, left, right],
        color,
        Stroke::NONE,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edges::edge_store::Edge;

    fn edge(source: u64, target: u64) -> Edge {
        Edge {
            id: 1,
            source_node: source,
            target_node: target,
            label: None,
        }
    }

    fn config(only_selected: bool, opacity_mode: OpacityMode) -> EdgeStyleConfig {
        EdgeStyleConfig {
            only_selected,
            opacity_mode,
            ..Default::default()
        }
    }

    #[test]
    fn only_selected_dims_unconnected_edges() {
        let selected = HashSet::from([7]);
        let cfg = config(true, OpacityMode::Full);
        assert_eq!(compute_alpha(&cfg, &edge(1, 2), &selected), 8);
        assert_eq!(compute_alpha(&cfg, &edge(7, 2), &selected), 200);
        assert_eq!(compute_alpha(&cfg, &edge(1, 7), &selected), 200);
    }

    #[test]
    fn full_opacity_ignores_selection() {
        let cfg = config(false, OpacityMode::Full);
        assert_eq!(compute_alpha(&cfg, &edge(1, 2), &HashSet::new()), 200);
    }

    #[test]
    fn depth_opacity_fades_unconnected_edges() {
        let selected = HashSet::from([1]);
        let cfg = config(false, OpacityMode::Depth);
        assert_eq!(compute_alpha(&cfg, &edge(1, 2), &selected), 200);
        assert_eq!(compute_alpha(&cfg, &edge(3, 4), &selected), 60);
    }
}
