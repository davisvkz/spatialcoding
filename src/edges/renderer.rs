use std::collections::HashSet;

use egui::{Color32, Pos2, Rect, Stroke, Vec2};

use crate::analysis::dependencies::LineSpan;
use crate::canvas::viewport::Viewport;
use crate::edges::edge_store::EdgeStore;
use crate::edges::style::{EdgeStyle, EdgeStyleConfig, OpacityMode};
use crate::nodes::file_node::{FileNode, HEADER_HEIGHT};
use crate::nodes::node_store::NodeStore;

/// Shortest horizontal reach of a bezier's control points, in screen pixels,
/// so edges between vertically stacked nodes still bend outward.
const MIN_BEZIER_REACH: f32 = 40.0;

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
        let (src_point, src_dir) =
            anchor(src, src_rect, edge.source_span, tgt_rect.center().x, font_size);
        let (tgt_point, tgt_dir) =
            anchor(tgt, tgt_rect, edge.target_span, src_rect.center().x, font_size);

        let edge_rect = egui::Rect::from_two_pos(src_point, tgt_point);
        if !visible_world.intersects(edge_rect) {
            continue;
        }

        let src_screen = viewport.world_to_screen(src_point);
        let tgt_screen = viewport.world_to_screen(tgt_point);

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

        // Direction the edge arrives from, for the arrow head.
        let mut arrives_from = src_screen;
        match config.style {
            EdgeStyle::Bezier => {
                let [_, _, cp2, _] = bezier_points(src_screen, src_dir, tgt_screen, tgt_dir);
                arrives_from = cp2;
                draw_bezier(painter, src_screen, src_dir, tgt_screen, tgt_dir, stroke);
            }
            EdgeStyle::Step => draw_step(painter, src_screen, tgt_screen, stroke),
            EdgeStyle::Line => { painter.line_segment([src_screen, tgt_screen], stroke); }
            EdgeStyle::Pcb => draw_pcb(painter, src_screen, tgt_screen, stroke),
        }

        if config.show_arrow_heads {
            draw_arrow_head(painter, arrives_from, tgt_screen, color);
        }
    }
}

/// Where an edge meets `node`: on the side facing `toward_x`, at the first
/// line of `span` when the node draws it, else at the header. Returns the
/// point and the outward direction of that side (+1 right, -1 left).
fn anchor(
    node: &FileNode,
    rect: Rect,
    span: Option<LineSpan>,
    toward_x: f32,
    font_size: f32,
) -> (Pos2, f32) {
    let dir = if toward_x >= rect.center().x { 1.0 } else { -1.0 };
    let x = if dir > 0.0 { rect.right() } else { rect.left() };
    let y = span
        .filter(|_| !node.collapsed)
        .and_then(|s| node.line_anchor_y(s.start, font_size))
        .unwrap_or(rect.top() + HEADER_HEIGHT * 0.5);
    (Pos2::new(x, y), dir)
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

/// Cubic bezier leaving `from` and entering `to` horizontally, each control
/// point pushed out of its node's side (`*_dir`: +1 right, -1 left).
fn bezier_points(from: Pos2, from_dir: f32, to: Pos2, to_dir: f32) -> [Pos2; 4] {
    let reach = ((to.x - from.x).abs() * 0.3).max(MIN_BEZIER_REACH);
    [
        from,
        Pos2::new(from.x + reach * from_dir, from.y),
        Pos2::new(to.x + reach * to_dir, to.y),
        to,
    ]
}

fn draw_bezier(
    painter: &egui::Painter,
    from: Pos2,
    from_dir: f32,
    to: Pos2,
    to_dir: f32,
    stroke: Stroke,
) {
    let bezier = egui::epaint::CubicBezierShape::from_points_stroke(
        bezier_points(from, from_dir, to, to_dir),
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
            source_span: None,
            target_span: None,
            name_span: None,
        }
    }

    fn file(content: &str, x: f32) -> FileNode {
        FileNode::new(1, "a.rs".into(), content.to_string(), egui::pos2(x, 0.0))
    }

    #[test]
    fn anchor_sits_on_the_side_facing_the_other_node_at_the_span_line() {
        let node = file("a\nb\nc\n", 0.0);
        let rect = node.world_rect(100.0, 10.0);
        let span = Some(LineSpan { start: 2, end: 2 });
        let line_y = node.line_anchor_y(2, 10.0).unwrap();

        assert_eq!(anchor(&node, rect, span, 500.0, 10.0), (Pos2::new(100.0, line_y), 1.0));
        assert_eq!(anchor(&node, rect, span, -500.0, 10.0), (Pos2::new(0.0, line_y), -1.0));
    }

    #[test]
    fn anchor_falls_back_to_the_header() {
        let header_y = HEADER_HEIGHT * 0.5;
        let mut node = file("a\nb\n", 0.0);
        let rect = node.world_rect(100.0, 10.0);
        assert_eq!(anchor(&node, rect, None, 500.0, 10.0).0.y, header_y, "no span");
        let past_end = Some(LineSpan { start: 7, end: 7 });
        assert_eq!(anchor(&node, rect, past_end, 500.0, 10.0).0.y, header_y, "line not drawn");
        node.collapsed = true;
        let first = Some(LineSpan { start: 0, end: 0 });
        assert_eq!(anchor(&node, rect, first, 500.0, 10.0).0.y, header_y, "collapsed");
    }

    #[test]
    fn bezier_control_points_leave_each_side_outward() {
        // Both anchors on the right side (stacked nodes): the curve bulges right.
        let [_, cp1, cp2, _] =
            bezier_points(Pos2::new(100.0, 0.0), 1.0, Pos2::new(100.0, 300.0), 1.0);
        assert_eq!((cp1.x, cp2.x), (100.0 + MIN_BEZIER_REACH, 100.0 + MIN_BEZIER_REACH));

        // Source on the left of the target: out to the right, into the left.
        let [_, cp1, cp2, _] =
            bezier_points(Pos2::new(0.0, 0.0), 1.0, Pos2::new(1000.0, 0.0), -1.0);
        assert_eq!((cp1.x, cp2.x), (300.0, 700.0));
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
