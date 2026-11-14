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

/// Visible pixel length under which an edge is dropped: `min_size_pct=1.0`
/// hides edges shorter than this many pixels on screen.
const MIN_VISIBLE_LEN_PX: f32 = 40.0;

/// Inverted-direction color for `highlight_backwards`: v2 picks a cool blue.
const BACKWARDS_COLOR: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6);

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
    // Stroke scales softly with zoom so edges stay sub-pixel thin in overview.
    let stroke_px = 1.0 * viewport.zoom.clamp(0.6, 1.4);
    let min_len_px = config.min_size_pct * MIN_VISIBLE_LEN_PX;

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

        if (tgt_screen - src_screen).length() < min_len_px {
            continue;
        }

        let is_src_sel = selected_nodes.contains(&edge.source_node);
        let is_tgt_sel = selected_nodes.contains(&edge.target_node);
        let is_connected = is_src_sel || is_tgt_sel;

        // show_all=false + nothing connected hides edges that have no
        // relation to the current selection. only_selected further dims
        // unconnected edges when they are still drawn.
        if !config.show_all && !is_connected && !selected_nodes.is_empty() {
            continue;
        }

        let alpha = compute_alpha(config, is_connected, selected_nodes.is_empty());
        if alpha < 2 {
            continue;
        }

        let base_color = if config.highlight_backwards && is_tgt_sel && !is_src_sel {
            BACKWARDS_COLOR
        } else {
            Color32::from_rgb(
                (config.base_color[0] * 255.0) as u8,
                (config.base_color[1] * 255.0) as u8,
                (config.base_color[2] * 255.0) as u8,
            )
        };
        let color = base_color.gamma_multiply(alpha as f32 / 255.0);
        let stroke = Stroke::new(stroke_px, color);

        // Direction the edge arrives from, for the arrow head.
        let mut arrives_from = src_screen;
        let use_gradient = config.use_gradient && edge.label.is_some();
        match config.style {
            EdgeStyle::Bezier => {
                let [_, _, cp2, _] = bezier_points(src_screen, src_dir, tgt_screen, tgt_dir);
                arrives_from = cp2;
                if use_gradient {
                    draw_bezier_gradient(
                        painter, src_screen, src_dir, tgt_screen, tgt_dir,
                        stroke_px, alpha, kind_color_for(config, edge.label.as_deref()),
                    );
                } else {
                    draw_bezier(painter, src_screen, src_dir, tgt_screen, tgt_dir, stroke);
                }
            }
            EdgeStyle::Step => draw_step(painter, src_screen, tgt_screen, stroke),
            EdgeStyle::Line => { painter.line_segment([src_screen, tgt_screen], stroke); }
            EdgeStyle::Pcb => draw_pcb(painter, src_screen, tgt_screen, stroke),
        }

        if config.show_connection_markers {
            let r = stroke_px * 1.6;
            painter.circle_filled(src_screen, r, color);
            painter.circle_filled(tgt_screen, r, color);
        }

        if config.show_arrow_heads {
            let arrow_size = if config.scale_arrow_heads {
                8.0 * viewport.zoom.clamp(0.5, 1.5)
            } else {
                8.0
            };
            draw_arrow_head(painter, arrives_from, tgt_screen, color, arrow_size);
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

fn compute_alpha(config: &EdgeStyleConfig, is_connected: bool, nothing_selected: bool) -> u8 {
    if config.only_selected && !is_connected && !nothing_selected {
        return 4;
    }

    match config.opacity_mode {
        OpacityMode::Full => 200,
        OpacityMode::Depth => {
            if is_connected || nothing_selected { 200 } else { 20 }
        }
    }
}

fn kind_color_for(config: &EdgeStyleConfig, label: Option<&str>) -> Color32 {
    match label {
        Some(l) => config.kind_color(l),
        None => Color32::from_rgb(
            (config.base_color[0] * 255.0) as u8,
            (config.base_color[1] * 255.0) as u8,
            (config.base_color[2] * 255.0) as u8,
        ),
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

fn cubic(p: [Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let b0 = u * u * u;
    let b1 = 3.0 * u * u * t;
    let b2 = 3.0 * u * t * t;
    let b3 = t * t * t;
    Pos2::new(
        b0 * p[0].x + b1 * p[1].x + b2 * p[2].x + b3 * p[3].x,
        b0 * p[0].y + b1 * p[1].y + b2 * p[2].y + b3 * p[3].y,
    )
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

/// Segmented bezier: each segment interpolates the kind color from
/// `end_color` (always the source-side tone here, since v2 shows the edge
/// settling into the target tone) to itself, giving a soft two-tone fade
/// when the base color differs from the kind color.
fn draw_bezier_gradient(
    painter: &egui::Painter,
    from: Pos2,
    from_dir: f32,
    to: Pos2,
    to_dir: f32,
    stroke_px: f32,
    alpha: u8,
    end_color: Color32,
) {
    const SEGMENTS: usize = 32;
    let pts = bezier_points(from, from_dir, to, to_dir);
    let start = Color32::from_rgba_unmultiplied(end_color.r(), end_color.g(), end_color.b(), 0);
    let end = Color32::from_rgba_unmultiplied(end_color.r(), end_color.g(), end_color.b(), alpha);
    let mut prev = pts[0];
    for i in 1..=SEGMENTS {
        let t = i as f32 / SEGMENTS as f32;
        let p = cubic(pts, t);
        let c = lerp_color(start, end, t);
        painter.line_segment([prev, p], Stroke::new(stroke_px, c));
        prev = p;
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
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

fn draw_arrow_head(painter: &egui::Painter, from: Pos2, to: Pos2, color: Color32, arrow_size: f32) {
    let dir = Vec2::new(to.x - from.x, to.y - from.y);
    let len = dir.length();
    if len < 1.0 {
        return;
    }
    let dir = dir / len;
    let perp = Vec2::new(-dir.y, dir.x);

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
        let [_, cp1, cp2, _] =
            bezier_points(Pos2::new(100.0, 0.0), 1.0, Pos2::new(100.0, 300.0), 1.0);
        assert_eq!((cp1.x, cp2.x), (100.0 + MIN_BEZIER_REACH, 100.0 + MIN_BEZIER_REACH));

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
        let cfg = config(true, OpacityMode::Full);
        assert_eq!(compute_alpha(&cfg, false, false), 4, "unconnected while a selection exists");
        assert_eq!(compute_alpha(&cfg, true, false), 200);
    }

    #[test]
    fn full_opacity_ignores_selection() {
        let cfg = config(false, OpacityMode::Full);
        assert_eq!(compute_alpha(&cfg, false, true), 200);
    }

    #[test]
    fn depth_opacity_fades_unconnected_edges() {
        let cfg = config(false, OpacityMode::Depth);
        assert_eq!(compute_alpha(&cfg, true, false), 200);
        assert_eq!(compute_alpha(&cfg, false, false), 20);
        assert_eq!(compute_alpha(&cfg, false, true), 200, "no selection = all at full");
    }
}
