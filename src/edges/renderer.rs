use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use std::collections::{HashMap, HashSet};

use crate::analysis::dependencies::LineSpan;
use crate::canvas::viewport::Viewport;
use crate::edges::edge_store::{EdgeChange, EdgeKind, EdgeStore, NameSpan};
use crate::edges::geometry::{self, bezier_points, cubic};
use crate::edges::ports::{side_facing, PortSide};
use crate::edges::style::{self, EdgeStyle, EdgeStyleConfig, OpacityMode};
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

/// Visible pixel length under which an edge is dropped: `hide_short_pct=1.0`
/// hides edges shorter than this many pixels on screen.
const MIN_VISIBLE_LEN_PX: f32 = 40.0;

/// While something is selected, an edge off it keeps this share of its
/// depth opacity (the reference fades the rest of the web).
const UNCONNECTED_RATIO: f32 = 0.57;

/// Sampling of a curve into pieces: stroke pieces of a gradient edge.
const CURVE_SAMPLES: usize = 64;

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
    let zoom = viewport.zoom;
    let lod = LodLevel::from_zoom(zoom);
    let min_len_px = config.hide_short_pct * MIN_VISIBLE_LEN_PX;
    let nothing_selected = selection.count() == 0 && selection.symbols().is_empty();
    // With ports on, edges end at the port on the node border (its line's
    // y on the facing side), the point `anchor` returns; otherwise they
    // touch the identifier glyphs.
    let glyph_ends =
        !config.show_ports && LodLevel::from_zoom(viewport.zoom).draws_code();

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

            // Backwards edges run right to left on the canvas; they are red
            // when highlighted and the reference draws them thicker.
            let world_backwards = geometry::is_backwards(src_rect, tgt_rect);
            let distance = geometry::folder_distance(&src.path, &tgt.path);
            if !config.show_all
                && !(is_connected
                    || changes_highlight
                    || (config.highlight_backwards && world_backwards)
                    || distance == 0
                    || distance <= config.folder_depth)
            {
                continue;
            }
            let selected_edge = (config.highlight_selected && is_connected && !nothing_selected)
                || changes_highlight;
            let backwards = config.highlight_reverse_layout && world_backwards;

            let mut factor = geometry::depth_opacity(config.opacity_mode, distance);
            if selected_edge {
                factor = 1.0;
            } else if !nothing_selected
                && config.highlight_selected
                && config.opacity_mode == OpacityMode::Depth
            {
                factor *= UNCONNECTED_RATIO;
            }
            let mut alpha = (factor * 255.0).round() as u8;
            alpha = symbol_dimmed(alpha, is_connected, collapse.symbol_dim);
            if alpha < 2 {
                continue;
            }

            // Highlight overrides pick a fixed color that must stay readable
            // end to end, so they also turn the gradient off.
            let diff_color = if config.diff_colors {
                match edge.change {
                    EdgeChange::Added => Some(rgb(style::ADDED_COLOR)),
                    EdgeChange::Deleted => Some(rgb(style::REMOVED_COLOR)),
                    _ => None,
                }
            } else {
                None
            };
            let override_color = if diff_color.is_some() {
                diff_color
            } else if backwards {
                Some(rgb(if selected_edge {
                    style::SELECTED_BACKWARDS_COLOR
                } else {
                    style::BACKWARDS_COLOR
                }))
            } else if config.highlight_barrel && edge.kind == EdgeKind::Barrel {
                Some(rgb(config.barrel_color))
            } else if config.highlight_reexport && edge.kind == EdgeKind::ReExport {
                Some(rgb(config.reexport_color))
            } else {
                None
            };
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
                } else if lod >= LodLevel::L4 {
                    // Far out, thin edges turn into a quiet semantic tone.
                    rgb(style::SEMANTIC_FADE_COLOR)
                } else {
                    folder_colors.map(|(source, _)| source).unwrap_or_else(|| {
                        lightness(rgb(config.relationships[edge.relationship.index()].color), config.base_lightness)
                    })
                }
            });
            let color = base_color.gamma_multiply(alpha as f32 / 255.0);
            let width = geometry::edge_width_px(selected_edge, backwards, zoom);
            let stroke = Stroke::new(width, color);

            // The path of the edge on screen.
            let k = config.bezier_curvature;
            let path: Vec<Pos2> = match config.style {
                EdgeStyle::Bezier => {
                    let ctrl = bezier_points(src_screen, src_dir, tgt_screen, tgt_dir, k, zoom);
                    (0..=CURVE_SAMPLES)
                        .map(|i| cubic(ctrl, i as f32 / CURVE_SAMPLES as f32))
                        .collect()
                }
                EdgeStyle::Step => geometry::step_points(src_screen, tgt_screen),
                EdgeStyle::Line => vec![src_screen, tgt_screen],
                EdgeStyle::Pcb => geometry::pcb_points(src_screen, tgt_screen),
            };
            // Gradient gap: only plain edges at least Min Dist long (world px).
            let gap_visible = (config.use_gradient && override_color.is_none() && !selected_edge)
                .then(|| {
                    geometry::gradient_visible_fraction(
                        config.gradient_gap_mode,
                        config.gradient_gap_pct,
                        config.gradient_fixed_visible_px,
                        config.gradient_min_dist,
                        geometry::polyline_length(&path) / zoom,
                    )
                })
                .flatten();
            let arrow_len = geometry::arrow_length(zoom, config.min_size_pct, config.scale_arrow_heads);

            // Keep glyph endpoints above their own files, but never paint
            // strokes, markers or arrow heads over an unrelated file.
            let bounds = path
                .iter()
                .fold(edge_rect, |rect, point| rect.union(Rect::from_min_max(*point, *point)))
                .expand(arrow_len.max(width * 1.6) + 2.0)
                .intersect(clip);
            let regions = uncovered_regions(
                bounds,
                file_rects
                    .iter()
                    .filter(|(id, _)| *id != edge.source_node && *id != edge.target_node)
                    .map(|(_, rect)| *rect),
            );
            let paint = |painter: &egui::Painter| {
                match gap_visible {
                    Some(visible) => draw_gap_gradient(painter, &path, width, base_color, alpha, visible),
                    None if config.style == EdgeStyle::Bezier => {
                        // The solid curve is one bezier shape, flattened by egui.
                        let ctrl = bezier_points(src_screen, src_dir, tgt_screen, tgt_dir, k, zoom);
                        painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                            ctrl,
                            false,
                            Color32::TRANSPARENT,
                            stroke,
                        ));
                    }
                    None => draw_path(painter, &path, stroke),
                }

                if config.show_connection_markers {
                    let r = width * 1.6;
                    painter.circle_filled(src_screen, r, color);
                    painter.circle_filled(tgt_screen, r, color);
                }

                // No arrow on backwards edges, nor when the gap leaves no
                // stroke at the tips.
                let hidden = world_backwards || gap_visible.is_some_and(|v| v <= 0.0);
                if arrow_head_visible(config, is_connected, nothing_selected) && !hidden {
                    let arrives_from = path[path.len().saturating_sub(2)];
                    draw_arrow_head(painter, arrives_from, tgt_screen, color, arrow_len);
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

/// Arrow heads follow v2's "Arrow Heads › Only selected": with `only_selected`
/// on they mark only the edges of the current selection.
fn arrow_head_visible(
    config: &EdgeStyleConfig,
    is_connected: bool,
    nothing_selected: bool,
) -> bool {
    config.show_arrow_heads && (!config.only_selected || (is_connected && !nothing_selected))
}

fn rgb(c: [f32; 3]) -> Color32 {
    Color32::from_rgb(
        (c[0] * 255.0) as u8,
        (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8,
    )
}

/// Channels of `color` scaled by `percent` (100 keeps it).
fn lightness(color: Color32, percent: f32) -> Color32 {
    let f = (percent / 100.0).clamp(0.0, 2.0);
    let c = |v: u8| ((v as f32 * f).round()).clamp(0.0, 255.0) as u8;
    Color32::from_rgb(c(color.r()), c(color.g()), c(color.b()))
}

fn draw_path(painter: &egui::Painter, path: &[Pos2], stroke: Stroke) {
    painter.add(egui::Shape::line(path.to_vec(), stroke));
}

/// Gradient gap: the stroke is full at both tips and fades to nothing toward
/// the middle, leaving the `visible` share of each end drawn (see
/// [`geometry::gradient_alpha`]).
fn draw_gap_gradient(painter: &egui::Painter, path: &[Pos2], width: f32, base: Color32, alpha: u8, visible: f32) {
    for (a, b, t) in geometry::polyline_pieces(path, CURVE_SAMPLES) {
        let f = geometry::gradient_alpha(t, visible);
        if f <= 0.0 {
            continue;
        }
        let c = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), (alpha as f32 * f).round() as u8);
        painter.line_segment([a, b], Stroke::new(width, c));
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
    let half = geometry::arrow_half_width(arrow_size);
    let left = tip - dir * arrow_size + perp * half;
    let right = tip - dir * arrow_size - perp * half;

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

    fn config(only_selected: bool, opacity_mode: OpacityMode) -> EdgeStyleConfig {
        EdgeStyleConfig {
            only_selected,
            opacity_mode,
            ..Default::default()
        }
    }

    #[test]
    fn lightness_scales_the_channels() {
        let c = Color32::from_rgb(100, 120, 160);
        assert_eq!(lightness(c, 100.0), c);
        assert_eq!(lightness(c, 50.0), Color32::from_rgb(50, 60, 80));
        assert_eq!(lightness(c, 200.0), Color32::from_rgb(200, 240, 255));
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
    fn symbol_dim_fades_only_edges_off_the_active_symbol() {
        assert_eq!(symbol_dimmed(200, false, Some(0.8)), 40);
        assert_eq!(symbol_dimmed(200, true, Some(0.8)), 200);
        assert_eq!(symbol_dimmed(200, false, None), 200);
        assert_eq!(symbol_dimmed(200, false, Some(1.5)), 0, "clamped");
    }

}
