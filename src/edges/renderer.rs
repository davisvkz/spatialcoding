use std::collections::HashSet;

use egui::{Color32, Pos2, Rect, Stroke, Vec2};

use crate::analysis::dependencies::LineSpan;
use crate::canvas::viewport::Viewport;
use crate::edges::edge_store::{EdgeKind, EdgeStore};
use crate::edges::style::{EdgeStyle, EdgeStyleConfig, GradientGapMode, OpacityMode};
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
        match edge.kind {
            EdgeKind::ReExport if !config.show_reexport_edges => continue,
            EdgeKind::Barrel if !config.show_barrel_edges => continue,
            _ => {}
        }
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
        } else if config.highlight_barrel && edge.kind == EdgeKind::Barrel {
            rgb(config.barrel_color)
        } else if config.highlight_reexport && edge.kind == EdgeKind::ReExport {
            rgb(config.reexport_color)
        } else {
            rgb(config.base_color)
        };
        let color = base_color.gamma_multiply(alpha as f32 / 255.0);
        let stroke = Stroke::new(stroke_px, color);

        // Direction the edge arrives from, for the arrow head.
        let mut arrives_from = src_screen;
        // Gradient disabled for highlight overrides: those pick a fixed
        // color that must stay readable end to end.
        let highlight_override = (config.highlight_backwards && is_tgt_sel && !is_src_sel)
            || (config.highlight_barrel && edge.kind == EdgeKind::Barrel)
            || (config.highlight_reexport && edge.kind == EdgeKind::ReExport);
        let use_gradient = config.use_gradient && edge.label.is_some() && !highlight_override;
        let (start_col, end_col) = gradient_endpoints(config, edge);
        let span_len = (tgt_screen - src_screen).length();
        let fade_frac = gradient_fade_fraction(config, span_len);
        match config.style {
            EdgeStyle::Bezier => {
                let [_, _, cp2, _] = bezier_points(src_screen, src_dir, tgt_screen, tgt_dir);
                arrives_from = cp2;
                if use_gradient {
                    draw_bezier_gradient(
                        painter, src_screen, src_dir, tgt_screen, tgt_dir,
                        stroke_px, alpha, start_col, end_col, fade_frac,
                    );
                } else {
                    draw_bezier(painter, src_screen, src_dir, tgt_screen, tgt_dir, stroke);
                }
            }
            EdgeStyle::Step => {
                if use_gradient {
                    draw_polyline_gradient(painter, &step_points(src_screen, tgt_screen),
                        stroke_px, alpha, start_col, end_col, fade_frac);
                } else {
                    draw_step(painter, src_screen, tgt_screen, stroke);
                }
            }
            EdgeStyle::Line => {
                if use_gradient {
                    draw_polyline_gradient(painter, &[src_screen, tgt_screen],
                        stroke_px, alpha, start_col, end_col, fade_frac);
                } else {
                    painter.line_segment([src_screen, tgt_screen], stroke);
                }
            }
            EdgeStyle::Pcb => {
                if use_gradient {
                    draw_polyline_gradient(painter, &pcb_points(src_screen, tgt_screen),
                        stroke_px, alpha, start_col, end_col, fade_frac);
                } else {
                    draw_pcb(painter, src_screen, tgt_screen, stroke);
                }
            }
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
        None => rgb(config.base_color),
    }
}

fn rgb(c: [f32; 3]) -> Color32 {
    Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
}

/// Source and target colors for the bi-color gradient. The source color
/// falls back to the base (edge cream) when the source symbol kind is
/// unknown, so the fade still starts somewhere visible.
fn gradient_endpoints(
    config: &EdgeStyleConfig,
    edge: &crate::edges::edge_store::Edge,
) -> (Color32, Color32) {
    let end = kind_color_for(config, edge.label.as_deref());
    let start = match edge.source_label.as_deref() {
        Some(l) => config.kind_color(l),
        None => rgb(config.base_color),
    };
    (start, end)
}

/// Fraction of the drawn span that fades between start and end colors. The
/// solid tail at each end gets `(1 - frac) / 2` of the length, matching the
/// v2 Code Canvas Gradient Gap control.
pub(crate) fn gradient_fade_fraction(config: &EdgeStyleConfig, span_len_px: f32) -> f32 {
    if !config.use_gradient || span_len_px <= 0.0 {
        return 1.0;
    }
    let raw = match config.gradient_gap_mode {
        GradientGapMode::Percent => config.gradient_gap_pct,
        GradientGapMode::Fixed => {
            if span_len_px <= config.gradient_min_dist_px {
                // Edges shorter than the min distance draw solid in the
                // target color: the gradient would be imperceptible anyway.
                return 0.0;
            }
            (span_len_px - config.gradient_min_dist_px) / span_len_px
        }
    };
    raw.clamp(0.0, 1.0)
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

/// Segmented bezier: interpolates `start_color` → `end_color` across 32
/// segments. `fade_frac` is the portion of the span that carries the fade;
/// outside it the tone stays solid (source at the start, target at the end),
/// matching the v2 Code Canvas Gradient Gap control.
#[allow(clippy::too_many_arguments)]
fn draw_bezier_gradient(
    painter: &egui::Painter,
    from: Pos2,
    from_dir: f32,
    to: Pos2,
    to_dir: f32,
    stroke_px: f32,
    alpha: u8,
    start_color: Color32,
    end_color: Color32,
    fade_frac: f32,
) {
    const SEGMENTS: usize = 32;
    let pts = bezier_points(from, from_dir, to, to_dir);
    let mut prev = pts[0];
    for i in 1..=SEGMENTS {
        let t = i as f32 / SEGMENTS as f32;
        let p = cubic(pts, t);
        let c = gradient_sample(start_color, end_color, alpha, t, fade_frac);
        painter.line_segment([prev, p], Stroke::new(stroke_px, c));
        prev = p;
    }
}

/// Same gradient semantics as `draw_bezier_gradient` but across the segments
/// of a straight polyline (`[A, corner, …, Z]`). `t` is taken by arc length
/// so the fade lands at the right spot on L-shaped Step/PCB edges.
fn draw_polyline_gradient(
    painter: &egui::Painter,
    pts: &[Pos2],
    stroke_px: f32,
    alpha: u8,
    start_color: Color32,
    end_color: Color32,
    fade_frac: f32,
) {
    if pts.len() < 2 {
        return;
    }
    let total: f32 = pts
        .windows(2)
        .map(|w| (w[1] - w[0]).length())
        .sum();
    if total <= 0.0 {
        return;
    }
    let mut travelled = 0.0;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg_len = (b - a).length();
        let t0 = travelled / total;
        let t1 = (travelled + seg_len) / total;
        let c0 = gradient_sample(start_color, end_color, alpha, t0, fade_frac);
        let c1 = gradient_sample(start_color, end_color, alpha, t1, fade_frac);
        let mid = Pos2::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
        let mid_col = lerp_color(c0, c1, 0.5);
        // Draw two halves to keep the fade smooth across the corner.
        painter.line_segment([a, mid], Stroke::new(stroke_px, c0));
        painter.line_segment([mid, b], Stroke::new(stroke_px, mid_col));
        let _ = c1;
        travelled += seg_len;
    }
}

/// Sample the bi-color fade at parameter `t ∈ [0,1]`. The fade lives in the
/// centered window of width `fade_frac`; outside stays solid (start before,
/// end after). All colors carry the edge's base `alpha`.
pub(crate) fn gradient_sample(start: Color32, end: Color32, alpha: u8, t: f32, fade_frac: f32) -> Color32 {
    let frac = fade_frac.clamp(0.0, 1.0);
    let half_gap = (1.0 - frac) * 0.5;
    let fade_start = half_gap;
    let fade_end = 1.0 - half_gap;
    let local = if t <= fade_start {
        0.0
    } else if t >= fade_end {
        1.0
    } else if (fade_end - fade_start) > f32::EPSILON {
        (t - fade_start) / (fade_end - fade_start)
    } else {
        1.0
    };
    let mixed = lerp_color(start, end, local);
    Color32::from_rgba_unmultiplied(mixed.r(), mixed.g(), mixed.b(), alpha)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

fn step_points(from: Pos2, to: Pos2) -> [Pos2; 4] {
    let mid_x = (from.x + to.x) * 0.5;
    [from, Pos2::new(mid_x, from.y), Pos2::new(mid_x, to.y), to]
}

fn pcb_points(from: Pos2, to: Pos2) -> [Pos2; 4] {
    let mid_y = (from.y + to.y) * 0.5;
    [from, Pos2::new(from.x, mid_y), Pos2::new(to.x, mid_y), to]
}

fn draw_step(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke) {
    let pts = step_points(from, to);
    for w in pts.windows(2) {
        painter.line_segment([w[0], w[1]], stroke);
    }
}

fn draw_pcb(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke) {
    let pts = pcb_points(from, to);
    for w in pts.windows(2) {
        painter.line_segment([w[0], w[1]], stroke);
    }
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
    fn gradient_fade_percent_mode_returns_the_ratio_directly() {
        let mut cfg = EdgeStyleConfig::default();
        cfg.use_gradient = true;
        cfg.gradient_gap_mode = GradientGapMode::Percent;
        cfg.gradient_gap_pct = 0.6;
        assert!((gradient_fade_fraction(&cfg, 200.0) - 0.6).abs() < 1e-5);
    }

    #[test]
    fn gradient_fade_fixed_mode_drops_min_dist_then_normalizes() {
        let mut cfg = EdgeStyleConfig::default();
        cfg.use_gradient = true;
        cfg.gradient_gap_mode = GradientGapMode::Fixed;
        cfg.gradient_min_dist_px = 40.0;
        // 200 - 40 = 160 of fade over 200 of span = 0.8
        assert!((gradient_fade_fraction(&cfg, 200.0) - 0.8).abs() < 1e-5);
        // span shorter than the min distance: fade collapses so the edge
        // draws solid in the target color.
        assert_eq!(gradient_fade_fraction(&cfg, 30.0), 0.0);
    }

    #[test]
    fn gradient_fade_disabled_when_use_gradient_off() {
        let cfg = EdgeStyleConfig::default();
        assert_eq!(gradient_fade_fraction(&cfg, 200.0), 1.0);
    }

    #[test]
    fn gradient_sample_is_start_before_fade_and_end_after() {
        let a = Color32::from_rgb(200, 0, 0);
        let b = Color32::from_rgb(0, 0, 200);
        // fade_frac 0.5: solid A on 0..0.25, fade on 0.25..0.75, solid B on 0.75..1.
        assert_eq!(gradient_sample(a, b, 255, 0.1, 0.5), Color32::from_rgba_unmultiplied(200, 0, 0, 255));
        assert_eq!(gradient_sample(a, b, 255, 0.9, 0.5), Color32::from_rgba_unmultiplied(0, 0, 200, 255));
        let mid = gradient_sample(a, b, 255, 0.5, 0.5);
        assert_eq!(mid, Color32::from_rgba_unmultiplied(100, 0, 100, 255));
    }

    #[test]
    fn depth_opacity_fades_unconnected_edges() {
        let cfg = config(false, OpacityMode::Depth);
        assert_eq!(compute_alpha(&cfg, true, false), 200);
        assert_eq!(compute_alpha(&cfg, false, false), 20);
        assert_eq!(compute_alpha(&cfg, false, true), 200, "no selection = all at full");
    }
}
