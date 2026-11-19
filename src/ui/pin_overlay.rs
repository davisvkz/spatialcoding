//! Painting of the pinned symbol graphs on the canvas: the colored blocks
//! and name marks of each role, the token edges from the source token, the
//! label pills at zoomed-out levels and the Out of View tags.

use std::path::Path;

use egui::{Color32, Pos2, Rect};

use crate::analysis::lsp::graph::Role;
use crate::app::clamp_or_mid;
use crate::ui::symbol_pins::Pin;

/// Most overlays of one role drawn per frame, and most edges.
pub const MAX_OVERLAYS_PER_ROLE: usize = 300;
pub const MAX_EDGES_PER_ROLE: usize = 400;

/// Alpha of the source token's white mark.
const SOURCE_ALPHA: f32 = 0.35;
/// Width of a token edge.
const EDGE_WIDTH: f32 = 1.25;

/// A painted token: the source of a pin (`entry` is `None`) or one entry.
#[derive(Debug, Clone)]
pub struct PinMark {
    pub pin: usize,
    pub entry: Option<usize>,
    pub role: Option<Role>,
    pub rect: Rect,
    /// The token text, for the label pill.
    pub label: String,
}

/// Geometry of one file node's code area on screen.
#[derive(Clone, Copy)]
pub struct CodeArea {
    pub origin: Pos2,
    pub right: f32,
    pub line_h: f32,
    pub char_w: f32,
    pub visible_lines: usize,
}

/// Byte offset in `line` of UTF-16 column `character`.
fn line_byte(line: &str, character: usize) -> usize {
    let mut units = 0;
    for (i, c) in line.char_indices() {
        if units >= character {
            return i;
        }
        units += c.len_utf16();
    }
    line.len()
}

/// Columns the text before `byte` takes (tabs count as four).
fn columns(line: &str, byte: usize) -> usize {
    line[..byte.min(line.len())].chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

/// Alpha of an entry's fill: a reference fades to .1 when the symbol has
/// incoming calls or more than five references (it would drown the rest).
fn fill_alpha(role: Role, crowded: bool) -> f32 {
    if role == Role::Ref && crowded {
        0.10
    } else {
        role.fill_alpha()
    }
}

fn crowded(pin: &Pin) -> bool {
    pin.graph().is_some_and(|g| {
        g.entries.iter().any(|e| e.role.group() == crate::analysis::lsp::graph::Group::IncomingCalls)
            || g.entries.iter().filter(|e| e.role == Role::Ref).count() > 5
    })
}

fn color(role: Role, alpha: f32) -> Color32 {
    let [r, g, b] = role.color();
    Color32::from_rgba_unmultiplied(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Paints the overlays of every pin inside the file `path` (content
/// `content`) under its code, and records the painted tokens in `marks`.
pub fn paint_node(
    painter: &egui::Painter,
    pins: &[Pin],
    path: &Path,
    content: &str,
    area: CodeArea,
    marks: &mut Vec<PinMark>,
) {
    let lines: Vec<&str> = content.lines().collect();
    let token_rect = |range: &crate::analysis::lsp::Range| -> Option<(Rect, String)> {
        let (a, b) = (range.start, range.end);
        if a.line != b.line || a.line >= area.visible_lines {
            return None;
        }
        let text = lines.get(a.line)?;
        let (s, e) = (line_byte(text, a.character), line_byte(text, b.character));
        let x0 = area.origin.x + columns(text, s) as f32 * area.char_w;
        let x1 = area.origin.x + columns(text, e).max(columns(text, s) + 1) as f32 * area.char_w;
        let y = area.origin.y + a.line as f32 * area.line_h;
        Some((Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + area.line_h)), text.get(s..e).unwrap_or("").to_string()))
    };

    let mut per_role: std::collections::HashMap<Role, usize> = std::collections::HashMap::new();
    for (pin_index, pin) in pins.iter().enumerate() {
        let Some(graph) = pin.graph() else { continue };
        let crowded = crowded(pin);
        if graph.source.path == path
            && let Some((rect, label)) = token_rect(&graph.source.range)
        {
            painter.rect_filled(rect, 1.0, Color32::from_white_alpha((SOURCE_ALPHA * 255.0) as u8));
            marks.push(PinMark { pin: pin_index, entry: None, role: None, rect, label });
        }
        for (entry_index, entry) in graph.entries.iter().enumerate().filter(|(_, e)| e.path == path) {
            let count = per_role.entry(entry.role).or_default();
            if *count >= MAX_OVERLAYS_PER_ROLE {
                continue;
            }
            *count += 1;
            let alpha = fill_alpha(entry.role, crowded);
            if entry.role.is_token() {
                if let Some((rect, label)) = token_rect(&entry.range) {
                    painter.rect_filled(rect, 1.0, color(entry.role, alpha));
                    marks.push(PinMark { pin: pin_index, entry: Some(entry_index), role: Some(entry.role), rect, label });
                }
            } else {
                // A context block: the lines of the range, across the node.
                let (start, end) = (entry.range.start.line, entry.range.end.line);
                if start >= area.visible_lines {
                    continue;
                }
                let end = end.min(area.visible_lines - 1);
                let top = area.origin.y + start as f32 * area.line_h;
                let bottom = area.origin.y + (end + 1) as f32 * area.line_h;
                let rect = Rect::from_min_max(Pos2::new(area.origin.x - 4.0, top), Pos2::new(area.right, bottom));
                painter.rect_filled(rect, 0.0, color(entry.role, alpha));
                if matches!(entry.role, Role::ImplCtx | Role::TypeDef) {
                    painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0_f32, color(entry.role, 0.55)), egui::StrokeKind::Inside);
                }
            }
        }
    }
}

/// The alpha of a token edge by role: a reference .5 (.1 when crowded), a
/// type definition .55, an implementation .52, an incoming callsite .8 and
/// .5 for the rest.
fn edge_alpha(role: Role, crowded: bool) -> f32 {
    match role {
        Role::Ref => {
            if crowded {
                0.10
            } else {
                0.5
            }
        }
        Role::TypeDef => 0.55,
        Role::ImplSel | Role::ImplCtx => 0.52,
        Role::InCallsite => 0.8,
        _ => 0.5,
    }
}

/// Bezier edges from each pin's source token to its token entries, a label
/// pill on every token when `pills` (zoomed out). Needs the marks `paint_node`
/// recorded for all nodes.
pub fn paint_connections(painter: &egui::Painter, pins: &[Pin], marks: &[PinMark], pills: bool) {
    let mut per_role: std::collections::HashMap<Role, usize> = std::collections::HashMap::new();
    for (pin_index, pin) in pins.iter().enumerate() {
        let crowded = crowded(pin);
        let Some(source) = marks.iter().find(|m| m.pin == pin_index && m.entry.is_none()) else { continue };
        for mark in marks.iter().filter(|m| m.pin == pin_index && m.entry.is_some()) {
            let role = mark.role.expect("entry marks have a role");
            let count = per_role.entry(role).or_default();
            if *count >= MAX_EDGES_PER_ROLE {
                continue;
            }
            *count += 1;
            // Leave from the side of the source that faces the target and
            // end on the nearer side of the target, anchored at the column.
            let right = mark.rect.center().x >= source.rect.center().x;
            let start = if right { source.rect.right_center() } else { source.rect.left_center() };
            let end = if right { mark.rect.left_center() } else { mark.rect.right_center() };
            let reach = ((end.x - start.x).abs() * 0.4).max(24.0) * if right { 1.0 } else { -1.0 };
            let stroke = egui::Stroke::new(EDGE_WIDTH, color(role, edge_alpha(role, crowded)));
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [start, start + egui::vec2(reach, 0.0), end - egui::vec2(reach, 0.0), end],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
        }
    }
    if pills {
        for mark in marks.iter().filter(|m| !m.label.is_empty()) {
            let tint = mark.role.map_or(Color32::WHITE, |r| color(r, 1.0));
            let galley = painter.layout_no_wrap(mark.label.clone(), egui::FontId::proportional(11.0), Color32::WHITE);
            let size = galley.size() + egui::vec2(10.0, 4.0);
            let rect = Rect::from_min_size(mark.rect.left_top() - egui::vec2(0.0, size.y + 2.0), size);
            painter.rect_filled(rect, 5.0, Color32::from_rgba_unmultiplied(0x16, 0x16, 0x18, 235));
            painter.rect_stroke(rect, 5.0, egui::Stroke::new(1.0_f32, tint.gamma_multiply(0.8)), egui::StrokeKind::Inside);
            painter.galley(rect.min + egui::vec2(5.0, 2.0), galley, Color32::WHITE);
        }
    }
}

/// An Out of View tag: where to draw it on the canvas border and the node it
/// points at.
#[derive(Debug, Clone, PartialEq)]
pub struct OutOfView {
    pub node_id: u64,
    pub name: String,
    /// Screen point on the border of the canvas.
    pub at: Pos2,
    /// World center of the node, to pan to when the tag is clicked.
    pub world_center: Pos2,
}

/// Tags for the open files with pin entries that are not on screen.
pub fn out_of_view(state: &crate::state::AppState, canvas: Rect) -> Vec<OutOfView> {
    let pins = state.lsp.pins.pins();
    if pins.is_empty() {
        return Vec::new();
    }
    let inner = canvas.shrink(14.0);
    let visible = state.viewport.visible_world_rect(canvas);
    let mut out = Vec::new();
    for (path, _) in state.lsp.pins.files() {
        let Some(node) = state.node_store.file_nodes.values().find(|n| n.path == path) else { continue };
        if state.node_store.is_file_hidden(node.id) {
            continue;
        }
        let rect = node.world_rect(state.settings.node_width, state.settings.font_size);
        if rect.intersects(visible) {
            continue;
        }
        let center = state.viewport.world_to_screen(rect.center());
        out.push(OutOfView {
            node_id: node.id,
            name: node.file_name().to_string(),
            at: Pos2::new(
                clamp_or_mid(center.x, inner.left(), inner.right()),
                clamp_or_mid(center.y, inner.top(), inner.bottom()),
            ),
            world_center: rect.center(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_columns_map_to_bytes_and_tabs_widen() {
        assert_eq!(line_byte("ação x", 3), 5, "ç and ã are two bytes but one UTF-16 unit each");
        assert_eq!(line_byte("a🚀b", 3), 5, "an astral char is two UTF-16 units");
        assert_eq!(columns("\tfoo", 1), 4);
        assert_eq!(columns("abc", 2), 2);
    }

    #[test]
    fn references_fade_when_the_symbol_has_incoming_calls_or_many_refs() {
        assert_eq!(fill_alpha(Role::Ref, false), 0.28);
        assert_eq!(fill_alpha(Role::Ref, true), 0.10);
        assert_eq!(fill_alpha(Role::DefSel, true), 0.30);
        assert_eq!(edge_alpha(Role::Ref, false), 0.5);
        assert_eq!(edge_alpha(Role::Ref, true), 0.10);
        assert_eq!(edge_alpha(Role::InCallsite, false), 0.8);
        assert_eq!(edge_alpha(Role::TypeDef, false), 0.55);
    }
}
