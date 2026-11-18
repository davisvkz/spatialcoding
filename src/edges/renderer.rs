use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use std::collections::{HashMap, HashSet};

use crate::analysis::dependencies::LineSpan;
use crate::canvas::viewport::Viewport;
use crate::edges::edge_store::{EdgeChange, EdgeKind, EdgeStore, NameSpan};
use crate::edges::ports::{side_facing, PortSide};
use crate::edges::style::{EdgeStyle, EdgeStyleConfig, GradientGapMode, OpacityMode};
use crate::nodes::file_node::{FileNode, HEADER_HEIGHT};
use crate::nodes::lod::LodLevel;
use crate::nodes::node_store::NodeStore;
use crate::nodes::selection::Selection;

/// Where edges of files hidden in collapsed folders go: each hidden file's
/// collapsed folder and its box (`NodeStore::collapse_proxies`), and whether
/// edges that end up between the same two boxes are drawn once.
#[derive(Default)]
pub struct CollapseRouting {
    pub proxies: HashMap<u64, (u64, Rect)>,
    pub merge: bool,
    /// LSP "Dim Dependency Edges": while a symbol is active, edges not
    /// linked to it keep only `1 - dim` of their opacity.
    pub symbol_dim: Option<f32>,
}

/// An edge end after collapse routing: a shown file or a collapsed folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EndKey {
    File(u64),
    Folder(u64),
}

/// Shortest horizontal reach of a bezier's control points, in screen pixels,
/// so edges between vertically stacked nodes still bend outward.
const MIN_BEZIER_REACH: f32 = 40.0;

/// Visible pixel length under which an edge is dropped: `hide_short_pct=1.0`
/// hides edges shorter than this many pixels on screen.
const MIN_VISIBLE_LEN_PX: f32 = 40.0;

/// Stroke multiplier for the edges of the selection.
const SELECTED_STROKE_SCALE: f32 = 1.4;

/// Edge alpha levels (0..=255). v2 keeps the idle web faint so it reads as
/// a web rather than a wall, and the selection's edges fully opaque.
const IDLE_ALPHA: u8 = 70;
const CONNECTED_ALPHA: u8 = 235;
const UNCONNECTED_DEPTH_ALPHA: u8 = 40;
const FULL_ALPHA: u8 = 200;

/// Inverted-direction color for `highlight_backwards`: v2 picks a cool blue.
const BACKWARDS_COLOR: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6);

pub fn draw_edges(
    painter: &egui::Painter,
    edges: &EdgeStore,
    nodes: &NodeStore,
    viewport: &Viewport,
    config: &EdgeStyleConfig,
    selection: &Selection,
    node_width: f32,
    font_size: f32,
    symbol_rect: &mut dyn FnMut(&FileNode, NameSpan) -> Option<Rect>,
    workspace_root: Option<&std::path::Path>,
    collapse: &CollapseRouting,
) {
    let clip = painter.clip_rect();
    let file_rects: Vec<_> = nodes
        .file_nodes
        .values()
        .filter(|node| !nodes.is_file_hidden(node.id))
        .map(|node| {
            (
                node.id,
                viewport.world_to_screen_rect(node.world_rect(node_width, font_size)),
            )
        })
        .filter(|(_, rect)| clip.intersects(*rect))
        .collect();
    // Stroke scales softly with zoom so edges stay sub-pixel thin in overview.
    let stroke_px = 1.0 * viewport.zoom.clamp(0.6, 1.4);
    let min_len_px = config.hide_short_pct * MIN_VISIBLE_LEN_PX;
    let nothing_selected = selection.count() == 0 && selection.symbols().is_empty();
    // With ports on, edges end at the port on the node border (its line's
    // y on the facing side), the point `anchor` returns; otherwise they
    // touch the identifier glyphs.
    let glyph_ends =
        !config.show_ports && LodLevel::from_zoom(viewport.zoom) != LodLevel::TitleOnly;

    // Edges merged into one per pair of ends, when a collapsed folder is one.
    let mut merged: HashSet<(bool, (EndKey, EndKey))> = HashSet::new();
    // Two passes, as v2: the dim web first, then the selection's edges on top.
    for connected_pass in [false, true] {
        for edge in edges.all_edges() {
            let is_connected = edge.is_selected(selection);
            let changes_highlight = config.highlight_changes && edge.change.is_changed();
            if (is_connected || changes_highlight) != connected_pass {
                continue;
            }
            if !edge.is_visible_for(config, selection) {
                continue;
            }
            let (Some(src), Some(tgt)) = (
                nodes.get_file(edge.source_node),
                nodes.get_file(edge.target_node),
            ) else {
                continue;
            };
            let src_proxy = collapse.proxies.get(&edge.source_node).copied();
            let tgt_proxy = collapse.proxies.get(&edge.target_node).copied();
            if src_proxy.is_some() || tgt_proxy.is_some() {
                let key = |node: u64, proxy: Option<(u64, Rect)>| {
                    proxy.map_or(EndKey::File(node), |(folder, _)| EndKey::Folder(folder))
                };
                let ends = (
                    key(edge.source_node, src_proxy),
                    key(edge.target_node, tgt_proxy),
                );
                // Both ends inside one collapsed folder: nothing to draw.
                if ends.0 == ends.1 {
                    continue;
                }
                if collapse.merge && !merged.insert((connected_pass, ends)) {
                    continue;
                }
            }

            let src_rect = src_proxy.map_or_else(|| src.world_rect(node_width, font_size), |p| p.1);
            let tgt_rect = tgt_proxy.map_or_else(|| tgt.world_rect(node_width, font_size), |p| p.1);
            let (src_point, src_dir) = match src_proxy {
                Some(_) => box_anchor(src_rect, tgt_rect.center().x),
                None => anchor(
                    src,
                    src_rect,
                    edge.source_span,
                    tgt_rect.center().x,
                    font_size,
                ),
            };
            let (tgt_point, tgt_dir) = match tgt_proxy {
                Some(_) => box_anchor(tgt_rect, src_rect.center().x),
                None => anchor(
                    tgt,
                    tgt_rect,
                    edge.target_span,
                    src_rect.center().x,
                    font_size,
                ),
            };
            let src_glyphs = glyph_ends && src_proxy.is_none();
            let tgt_glyphs = glyph_ends && tgt_proxy.is_none();

            let src_screen = edge
                .source_name_span
                .filter(|_| src_glyphs && !src.collapsed)
                .and_then(|name| symbol_rect(src, name))
                .map(|rect| glyph_anchor(rect, src_dir))
                .unwrap_or_else(|| viewport.world_to_screen(src_point));
            let tgt_screen = edge
                .name_span
                .filter(|_| tgt_glyphs && !tgt.collapsed)
                .and_then(|name| symbol_rect(tgt, name))
                .map(|rect| glyph_anchor(rect, tgt_dir))
                .unwrap_or_else(|| viewport.world_to_screen(tgt_point));
            let edge_rect = egui::Rect::from_two_pos(src_screen, tgt_screen);
            if !clip.intersects(edge_rect) {
                continue;
            }

            if (tgt_screen - src_screen).length() < min_len_px {
                continue;
            }

            let mut alpha = if changes_highlight {
                CONNECTED_ALPHA
            } else {
                compute_alpha(config, is_connected, nothing_selected)
            };
            alpha = symbol_dimmed(alpha, is_connected, collapse.symbol_dim);
            if alpha < 2 {
                continue;
            }

            // Highlight overrides pick a fixed color that must stay readable
            // end to end, so they also turn the gradient off.
            let backwards = (config.highlight_backwards && edge.is_incoming(selection))
                || (config.highlight_reverse_layout && src_rect.center().x > tgt_rect.center().x);
            let diff_color = if config.diff_colors {
                match edge.change {
                    EdgeChange::Added => Some(Color32::from_rgb(64, 204, 112)),
                    EdgeChange::Deleted => Some(Color32::from_rgb(242, 80, 96)),
                    _ => None,
                }
            } else {
                None
            };
            let override_color = if diff_color.is_some() {
                diff_color
            } else if backwards {
                Some(BACKWARDS_COLOR)
            } else if config.highlight_barrel && edge.kind == EdgeKind::Barrel {
                Some(rgb(config.barrel_color))
            } else if config.highlight_reexport && edge.kind == EdgeKind::ReExport {
                Some(rgb(config.reexport_color))
            } else {
                None
            };
            let selected_edge = (config.highlight_selected && is_connected && !nothing_selected)
                || changes_highlight;
            let folder_colors = workspace_root
                .filter(|_| config.folder_levels)
                .and_then(|root| {
                    Some((
                        rgb(config.folder_color(&src.path, root)?),
                        rgb(config.folder_color(&tgt.path, root)?),
                    ))
                });
            let base_color = override_color.unwrap_or_else(|| {
                if selected_edge {
                    rgb(config.selected_color)
                } else {
                    folder_colors.map(|(source, _)| source).unwrap_or_else(|| {
                        rgb(config.relationships[edge.relationship.index()].color)
                    })
                }
            });
            let color = base_color.gamma_multiply(alpha as f32 / 255.0);
            let width = if selected_edge {
                stroke_px * SELECTED_STROKE_SCALE
            } else {
                stroke_px
            };
            let stroke = Stroke::new(width, color);

            // Keep glyph endpoints above their own files, but never paint
            // strokes, markers or arrow heads over an unrelated file.
            let bounds = bezier_points(src_screen, src_dir, tgt_screen, tgt_dir)
                .into_iter()
                .fold(edge_rect, |rect, point| {
                    rect.union(Rect::from_min_max(point, point))
                })
                .expand(arrow_head_size(config, viewport.zoom).max(width * 1.6) + 2.0)
                .intersect(clip);
            let regions = uncovered_regions(
                bounds,
                file_rects
                    .iter()
                    .filter(|(id, _)| *id != edge.source_node && *id != edge.target_node)
                    .map(|(_, rect)| *rect),
            );
            let paint = |painter: &egui::Painter| {
                // Direction the edge arrives from, for the arrow head.
                let mut arrives_from = src_screen;
                let use_gradient = config.use_gradient
                    && edge.label.is_some()
                    && override_color.is_none()
                    && !selected_edge;
                let (start_col, end_col) =
                    folder_colors.unwrap_or_else(|| gradient_endpoints(config, edge));
                let span_len = (tgt_screen - src_screen).length();
                let fade_frac = gradient_fade_fraction(config, span_len);
                match config.style {
                    EdgeStyle::Bezier => {
                        let [_, _, cp2, _] =
                            bezier_points(src_screen, src_dir, tgt_screen, tgt_dir);
                        arrives_from = cp2;
                        if use_gradient {
                            draw_bezier_gradient(
                                painter, src_screen, src_dir, tgt_screen, tgt_dir, width, alpha,
                                start_col, end_col, fade_frac,
                            );
                        } else {
                            draw_bezier(painter, src_screen, src_dir, tgt_screen, tgt_dir, stroke);
                        }
                    }
                    EdgeStyle::Step => {
                        if use_gradient {
                            draw_polyline_gradient(
                                painter,
                                &step_points(src_screen, tgt_screen),
                                width,
                                alpha,
                                start_col,
                                end_col,
                                fade_frac,
                            );
                        } else {
                            draw_step(painter, src_screen, tgt_screen, stroke);
                        }
                    }
                    EdgeStyle::Line => {
                        if use_gradient {
                            draw_polyline_gradient(
                                painter,
                                &[src_screen, tgt_screen],
                                width,
                                alpha,
                                start_col,
                                end_col,
                                fade_frac,
                            );
                        } else {
                            painter.line_segment([src_screen, tgt_screen], stroke);
                        }
                    }
                    EdgeStyle::Pcb => {
                        if use_gradient {
                            draw_polyline_gradient(
                                painter,
                                &pcb_points(src_screen, tgt_screen),
                                width,
                                alpha,
                                start_col,
                                end_col,
                                fade_frac,
                            );
                        } else {
                            draw_pcb(painter, src_screen, tgt_screen, stroke);
                        }
                    }
                }

                if config.show_connection_markers {
                    let r = width * 1.6;
                    painter.circle_filled(src_screen, r, color);
                    painter.circle_filled(tgt_screen, r, color);
                }

                if arrow_head_visible(config, is_connected, nothing_selected) {
                    let arrow_size = arrow_head_size(config, viewport.zoom);
                    draw_arrow_head(painter, arrives_from, tgt_screen, color, arrow_size);
                }
            };
            for region in regions {
                paint(&painter.with_clip_rect(region));
            }
        }
    }
}

/// Disjoint clip rectangles covering the edge bounds outside file blocks.
/// Rectangular clipping applies equally to curves, gradients and decorations.
fn uncovered_regions(bounds: Rect, blockers: impl Iterator<Item = Rect>) -> Vec<Rect> {
    let mut regions = vec![bounds];
    for blocker in blockers {
        regions = regions
            .into_iter()
            .flat_map(|region| {
                let cut = region.intersect(blocker);
                if !cut.is_positive() {
                    return vec![region];
                }
                [
                    Rect::from_min_max(region.min, Pos2::new(region.max.x, cut.min.y)),
                    Rect::from_min_max(Pos2::new(region.min.x, cut.max.y), region.max),
                    Rect::from_min_max(
                        Pos2::new(region.min.x, cut.min.y),
                        Pos2::new(cut.min.x, cut.max.y),
                    ),
                    Rect::from_min_max(
                        Pos2::new(cut.max.x, cut.min.y),
                        Pos2::new(region.max.x, cut.max.y),
                    ),
                ]
                .into_iter()
                .filter(Rect::is_positive)
                .collect()
            })
            .collect();
    }
    regions
}

/// Touch the side of the actual name facing the other endpoint, not the file
/// border or a replacement label. The arrow tip stays outside the glyphs.
fn glyph_anchor(rect: Rect, outward: f32) -> Pos2 {
    if outward > 0.0 {
        rect.right_center()
    } else {
        rect.left_center()
    }
}

/// Where an edge meets `node`: on the side facing `toward_x`, at the first
/// line of `span` when the node draws it (where its port sits), else at the
/// header. Returns the
/// point and the outward direction of that side (+1 right, -1 left).
fn anchor(
    node: &FileNode,
    rect: Rect,
    span: Option<LineSpan>,
    toward_x: f32,
    font_size: f32,
) -> (Pos2, f32) {
    let dir = match side_facing(rect.left(), rect.right(), toward_x) {
        PortSide::Right => 1.0,
        PortSide::Left => -1.0,
    };
    let x = if dir > 0.0 { rect.right() } else { rect.left() };
    let y = span
        .filter(|_| !node.collapsed)
        .and_then(|s| node.line_anchor_y(s.start, font_size))
        .unwrap_or(rect.top() + HEADER_HEIGHT * 0.5);
    (Pos2::new(x, y), dir)
}

/// LSP "Dim Dependency Edges": an edge not linked to the active symbol
/// keeps `1 - dim` of its alpha.
pub(crate) fn symbol_dimmed(alpha: u8, connected: bool, dim: Option<f32>) -> u8 {
    match dim.filter(|_| !connected) {
        Some(dim) => (alpha as f32 * (1.0 - dim.clamp(0.0, 1.0))).round() as u8,
        None => alpha,
    }
}

/// Middle of the side of a collapsed folder's box facing `toward_x`.
fn box_anchor(rect: Rect, toward_x: f32) -> (Pos2, f32) {
    match side_facing(rect.left(), rect.right(), toward_x) {
        PortSide::Right => (rect.right_center(), 1.0),
        PortSide::Left => (rect.left_center(), -1.0),
    }
}

fn compute_alpha(config: &EdgeStyleConfig, is_connected: bool, nothing_selected: bool) -> u8 {
    let is_connected = is_connected && config.highlight_selected;
    let nothing_selected = nothing_selected || !config.highlight_selected;
    match config.opacity_mode {
        OpacityMode::Full => {
            if is_connected && !nothing_selected {
                CONNECTED_ALPHA
            } else {
                FULL_ALPHA
            }
        }
        OpacityMode::Depth => {
            if nothing_selected {
                IDLE_ALPHA
            } else if is_connected {
                CONNECTED_ALPHA
            } else {
                UNCONNECTED_DEPTH_ALPHA
            }
        }
    }
}

/// Arrow heads follow v2's "Arrow Heads › Only selected": with `only_selected`
/// on they mark only the edges of the current selection.
fn arrow_head_visible(
    config: &EdgeStyleConfig,
    is_connected: bool,
    nothing_selected: bool,
) -> bool {
    config.show_arrow_heads && (!config.only_selected || (is_connected && !nothing_selected))
}

fn kind_color_for(config: &EdgeStyleConfig, label: Option<&str>) -> Color32 {
    match label {
        Some(l) => config.kind_color(l),
        None => rgb(config.base_color),
    }
}

fn rgb(c: [f32; 3]) -> Color32 {
    Color32::from_rgb(
        (c[0] * 255.0) as u8,
        (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8,
    )
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

/// Arrow-head length in screen pixels: 8 px, or with Scale Arrow Heads 8 px
/// times the zoom, never below Min Size (and never above 1.5x).
pub(crate) fn arrow_head_size(config: &EdgeStyleConfig, zoom: f32) -> f32 {
    const BASE: f32 = 8.0;
    if !config.scale_arrow_heads {
        return BASE;
    }
    let min = config.min_size_pct.clamp(0.05, 1.0);
    BASE * zoom.clamp(min, 1.5)
}

/// Fraction of the drawn span that fades between start and end colors. The
/// solid tail at each end gets `(1 - frac) / 2` of the length, matching the
/// v2 Code Canvas Gradient Gap control.
pub(crate) fn gradient_fade_fraction(config: &EdgeStyleConfig, span_len_px: f32) -> f32 {
    if !config.use_gradient || span_len_px <= 0.0 {
        return 1.0;
    }
    // Edges shorter than Min Dist draw solid in the target color: the
    // gradient would be imperceptible anyway.
    let min_dist = config.gradient_min_dist_sq.max(0.0).sqrt();
    if span_len_px <= min_dist {
        return 0.0;
    }
    let raw = match config.gradient_gap_mode {
        GradientGapMode::Percent => config.gradient_gap_pct,
        GradientGapMode::Fixed => (span_len_px - min_dist) / span_len_px,
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
    let total: f32 = pts.windows(2).map(|w| (w[1] - w[0]).length()).sum();
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
pub(crate) fn gradient_sample(
    start: Color32,
    end: Color32,
    alpha: u8,
    t: f32,
    fade_frac: f32,
) -> Color32 {
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
    Color32::from_rgba_unmultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
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
    fn unrelated_file_occludes_every_edge_style_but_not_its_endpoints() {
        let mut nodes = NodeStore::default();
        let source = nodes.add_file("a.rs".into(), "a".into(), Pos2::ZERO);
        let target = nodes.add_file("b.rs".into(), "b".into(), Pos2::new(400.0, 0.0));
        nodes.add_file("hidden/c.rs".into(), "c".into(), Pos2::new(200.0, 0.0));
        let folder = nodes.add_folder("hidden".into(), 1);
        let mut edges = EdgeStore::default();
        let name = NameSpan {
            line: 0,
            start_byte: 0,
            end_byte: 1,
        };
        edges.add_full(
            source,
            target,
            Some("fn".into()),
            None,
            None,
            None,
            Some(name),
            Some(name),
            EdgeKind::Direct,
        );
        let clip = Rect::from_min_size(Pos2::ZERO, Vec2::splat(600.0));
        let blocked = Pos2::new(250.0, HEADER_HEIGHT * 0.5);
        for style in [
            EdgeStyle::Bezier,
            EdgeStyle::Step,
            EdgeStyle::Line,
            EdgeStyle::Pcb,
        ] {
            for use_gradient in [false, true] {
                for hidden in [false, true] {
                    nodes.folder_nodes.get_mut(&folder).unwrap().collapsed = hidden;
                    let mut selection = Selection::default();
                    if !use_gradient {
                        selection.select(source);
                    }
                    let ctx = egui::Context::default();
                    let output = ctx.run(egui::RawInput::default(), |ctx| {
                        let painter = ctx
                            .layer_painter(egui::LayerId::background())
                            .with_clip_rect(clip);
                        draw_edges(
                            &painter,
                            &edges,
                            &nodes,
                            &Viewport::default(),
                            &EdgeStyleConfig {
                                style,
                                use_gradient,
                                show_ports: false,
                                ..Default::default()
                            },
                            &selection,
                            100.0,
                            10.0,
                            &mut |node, _| {
                                Some(Rect::from_min_size(
                                    node.position + Vec2::new(30.0, 10.0),
                                    Vec2::new(10.0, 8.0),
                                ))
                            },
                            None,
                            &CollapseRouting::default(),
                        );
                    });
                    assert!(!output.shapes.is_empty());
                    assert_eq!(
                        output.shapes.iter().any(|s| s.clip_rect.contains(blocked)),
                        hidden,
                        "{style:?}, gradient={use_gradient}, hidden={hidden}",
                    );
                    for endpoint in [Pos2::new(40.0, 14.0), Pos2::new(430.0, 14.0)] {
                        assert!(output.shapes.iter().any(|s| s.clip_rect.contains(endpoint)));
                    }
                }
            }
        }
    }

    #[test]
    fn overlapping_blockers_leave_disjoint_regions_and_preserve_uncovered_area() {
        let bounds = Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0));
        let blockers = [
            Rect::from_min_max(Pos2::new(20.0, 20.0), Pos2::new(60.0, 60.0)),
            Rect::from_min_max(Pos2::new(40.0, 40.0), Pos2::new(80.0, 80.0)),
        ];
        let regions = uncovered_regions(bounds, blockers.into_iter());
        assert_eq!(regions.iter().map(Rect::area).sum::<f32>(), 7200.0);
        for (index, region) in regions.iter().enumerate() {
            assert!(bounds.contains_rect(*region));
            assert!(blockers.iter().all(|b| !b.intersect(*region).is_positive()));
            assert!(regions[index + 1..]
                .iter()
                .all(|other| !other.intersect(*region).is_positive()));
        }
        assert!(uncovered_regions(bounds, [bounds].into_iter()).is_empty());
    }

    #[test]
    fn anchor_sits_on_the_side_facing_the_other_node_at_the_span_line() {
        let node = file("a\nb\nc\n", 0.0);
        let rect = node.world_rect(100.0, 10.0);
        let span = Some(LineSpan { start: 2, end: 2 });
        let line_y = node.line_anchor_y(2, 10.0).unwrap();

        assert_eq!(
            anchor(&node, rect, span, 500.0, 10.0),
            (Pos2::new(100.0, line_y), 1.0)
        );
        assert_eq!(
            anchor(&node, rect, span, -500.0, 10.0),
            (Pos2::new(0.0, line_y), -1.0)
        );
    }

    #[test]
    fn anchor_falls_back_to_the_header() {
        let header_y = HEADER_HEIGHT * 0.5;
        let mut node = file("a\nb\n", 0.0);
        let rect = node.world_rect(100.0, 10.0);
        assert_eq!(
            anchor(&node, rect, None, 500.0, 10.0).0.y,
            header_y,
            "no span"
        );
        let past_end = Some(LineSpan { start: 7, end: 7 });
        assert_eq!(
            anchor(&node, rect, past_end, 500.0, 10.0).0.y,
            header_y,
            "line not drawn"
        );
        node.collapsed = true;
        let first = Some(LineSpan { start: 0, end: 0 });
        assert_eq!(
            anchor(&node, rect, first, 500.0, 10.0).0.y,
            header_y,
            "collapsed"
        );
    }

    #[test]
    fn bezier_control_points_leave_each_side_outward() {
        let [_, cp1, cp2, _] =
            bezier_points(Pos2::new(100.0, 0.0), 1.0, Pos2::new(100.0, 300.0), 1.0);
        assert_eq!(
            (cp1.x, cp2.x),
            (100.0 + MIN_BEZIER_REACH, 100.0 + MIN_BEZIER_REACH)
        );

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
    fn full_opacity_keeps_the_web_bright_and_lifts_the_selection() {
        let cfg = config(true, OpacityMode::Full);
        assert_eq!(compute_alpha(&cfg, false, true), FULL_ALPHA);
        assert_eq!(compute_alpha(&cfg, false, false), FULL_ALPHA);
        assert_eq!(compute_alpha(&cfg, true, false), CONNECTED_ALPHA);
    }

    #[test]
    fn arrow_heads_only_on_the_selection_when_only_selected() {
        let mut cfg = config(true, OpacityMode::Depth);
        cfg.show_arrow_heads = true;
        assert!(!arrow_head_visible(&cfg, false, true), "nothing selected");
        assert!(!arrow_head_visible(&cfg, false, false), "unconnected");
        assert!(arrow_head_visible(&cfg, true, false), "selection edge");
        cfg.only_selected = false;
        assert!(arrow_head_visible(&cfg, false, true), "all edges");
        cfg.show_arrow_heads = false;
        assert!(!arrow_head_visible(&cfg, true, false), "arrows off");
    }

    #[test]
    fn glyph_anchor_touches_the_name_not_the_file_border() {
        let name = Rect::from_min_max(Pos2::new(60.0, 80.0), Pos2::new(110.0, 92.0));
        assert_eq!(glyph_anchor(name, -1.0), Pos2::new(60.0, 86.0));
        assert_eq!(glyph_anchor(name, 1.0), Pos2::new(110.0, 86.0));
    }

    #[test]
    fn gradient_fade_percent_mode_returns_the_ratio_directly() {
        let mut cfg = EdgeStyleConfig::default();
        cfg.use_gradient = true;
        cfg.gradient_gap_mode = GradientGapMode::Percent;
        cfg.gradient_gap_pct = 0.6;
        cfg.gradient_min_dist_sq = 100.0 * 100.0;
        assert!((gradient_fade_fraction(&cfg, 200.0) - 0.6).abs() < 1e-5);
        assert_eq!(
            gradient_fade_fraction(&cfg, 99.0),
            0.0,
            "shorter than Min Dist: solid"
        );
    }

    #[test]
    fn symbol_dim_fades_only_edges_off_the_active_symbol() {
        assert_eq!(symbol_dimmed(200, false, Some(0.8)), 40);
        assert_eq!(symbol_dimmed(200, true, Some(0.8)), 200);
        assert_eq!(symbol_dimmed(200, false, None), 200);
        assert_eq!(symbol_dimmed(200, false, Some(1.5)), 0, "clamped");
    }

    #[test]
    fn min_dist_default_is_v2s_40000_squared_px() {
        let cfg = EdgeStyleConfig::default();
        assert_eq!(cfg.gradient_min_dist_sq, 40_000.0);
        assert_eq!(gradient_fade_fraction(&cfg, 199.0), 0.0);
        assert!((gradient_fade_fraction(&cfg, 201.0) - cfg.gradient_gap_pct).abs() < 1e-5);
    }

    #[test]
    fn scaled_arrow_heads_shrink_with_zoom_down_to_min_size() {
        let mut cfg = EdgeStyleConfig {
            scale_arrow_heads: true,
            min_size_pct: 0.5,
            ..Default::default()
        };
        assert_eq!(arrow_head_size(&cfg, 1.0), 8.0);
        assert_eq!(arrow_head_size(&cfg, 0.1), 4.0, "clamped at Min Size");
        cfg.min_size_pct = 0.25;
        assert_eq!(arrow_head_size(&cfg, 0.1), 2.0);
        cfg.scale_arrow_heads = false;
        assert_eq!(arrow_head_size(&cfg, 0.1), 8.0);
    }

    #[test]
    fn gradient_fade_fixed_mode_drops_min_dist_then_normalizes() {
        let mut cfg = EdgeStyleConfig::default();
        cfg.use_gradient = true;
        cfg.gradient_gap_mode = GradientGapMode::Fixed;
        cfg.gradient_min_dist_sq = 40.0 * 40.0;
        // 200 - 40 = 160 of fade over 200 of span = 0.8
        assert!((gradient_fade_fraction(&cfg, 200.0) - 0.8).abs() < 1e-5);
        // span shorter than the min distance: fade collapses so the edge
        // draws solid in the target color.
        assert_eq!(gradient_fade_fraction(&cfg, 30.0), 0.0);
    }

    #[test]
    fn gradient_fade_disabled_when_use_gradient_off() {
        let cfg = EdgeStyleConfig {
            use_gradient: false,
            ..Default::default()
        };
        assert_eq!(gradient_fade_fraction(&cfg, 200.0), 1.0);
    }

    #[test]
    fn gradient_sample_is_start_before_fade_and_end_after() {
        let a = Color32::from_rgb(200, 0, 0);
        let b = Color32::from_rgb(0, 0, 200);
        // fade_frac 0.5: solid A on 0..0.25, fade on 0.25..0.75, solid B on 0.75..1.
        assert_eq!(
            gradient_sample(a, b, 255, 0.1, 0.5),
            Color32::from_rgba_unmultiplied(200, 0, 0, 255)
        );
        assert_eq!(
            gradient_sample(a, b, 255, 0.9, 0.5),
            Color32::from_rgba_unmultiplied(0, 0, 200, 255)
        );
        let mid = gradient_sample(a, b, 255, 0.5, 0.5);
        assert_eq!(mid, Color32::from_rgba_unmultiplied(100, 0, 100, 255));
    }

    #[test]
    fn depth_opacity_fades_unconnected_edges() {
        let cfg = config(false, OpacityMode::Depth);
        assert_eq!(compute_alpha(&cfg, true, false), CONNECTED_ALPHA);
        assert_eq!(compute_alpha(&cfg, false, false), UNCONNECTED_DEPTH_ALPHA);
        assert_eq!(
            compute_alpha(&cfg, false, true),
            IDLE_ALPHA,
            "no selection = faint web"
        );
    }
}
