use std::path::PathBuf;

use crate::canvas::minimap::Minimap;
use crate::cli::StartupOptions;
use crate::canvas::renderer::CanvasRenderer;
use crate::edges::renderer as edge_renderer;
use crate::fs::tree::TreeNode;
use crate::highlight::syntect_hl::Highlighter;
use crate::input;
use crate::nodes::file_node::{line_height, CODE_PADDING, HEADER_HEIGHT};
use crate::nodes::lod::LodLevel;
use crate::state::{AppState, Tool};
use crate::ui;
use crate::ui::cst_panel;
use crate::workspace;

pub struct SpatialCodingApp {
    state: AppState,
    file_tree: Option<TreeNode>,
    highlighter: Highlighter,
    minimap: Minimap,
    /// Fit the view to all nodes on the next frame, once the canvas size is known.
    pending_fit: bool,
    /// `--layout` was asked: lay out again once the language servers have
    /// answered, since the dependency edges arrive in the background.
    layout_when_indexed: bool,
}

impl SpatialCodingApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            state: AppState::default(),
            file_tree: None,
            highlighter: Highlighter::new(),
            minimap: Minimap::default(),
            pending_fit: false,
            layout_when_indexed: false,
        }
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    pub fn file_tree(&self) -> Option<&TreeNode> {
        self.file_tree.as_ref()
    }

    /// Loads `dir` as the workspace, replacing whatever is on the canvas.
    pub fn open_folder(&mut self, dir: PathBuf) {
        workspace::clear_canvas(&mut self.state);
        self.file_tree =
            match crate::fs::loader::load_directory(&dir, self.state.settings.show_dot_folders) {
                Ok(tree) => Some(tree),
                Err(e) => {
                    log::error!("Failed to load {}: {e}", dir.display());
                    None
                }
            };
        self.state.loaded_dir = Some(dir);
    }

    /// Applies command-line options: open the folder, open files, optionally
    /// lay them out, then fit the view to them on the first frame.
    pub fn apply_startup(&mut self, opts: &StartupOptions) {
        let Some(dir) = opts.dir.clone() else {
            return;
        };
        self.open_folder(dir);
        if opts.open_all {
            workspace::open_all_files(&mut self.state);
        }
        workspace::open_files(&mut self.state, &opts.open_paths());
        if opts.layout {
            self.apply_layout();
            self.layout_when_indexed = self.state.deps.is_waiting();
        }
        self.pending_fit = !self.state.node_store.file_nodes.is_empty();
    }

    fn apply_layout(&mut self) {
        crate::layout::hierarchical::apply_layout(
            &mut self.state.node_store,
            &self.state.edge_store,
            &self.state.layout_settings,
        );
    }

    fn draw_canvas(&mut self, ui: &mut egui::Ui) {
        let canvas_rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(canvas_rect, egui::Sense::click_and_drag());

        if std::mem::take(&mut self.pending_fit)
            && let Some(world_rect) = input::compute_nodes_bounding_rect(&self.state)
        {
            self.state.viewport.fit_to_rect(world_rect, canvas_rect);
        }

        if let Some(world) = self.state.pending_focus.take() {
            // Jump to a reference: readable text, centered on its line.
            let vp = &mut self.state.viewport;
            if LodLevel::from_zoom(vp.zoom) != LodLevel::Full {
                vp.zoom = 1.0;
                vp.target_zoom = 1.0;
            }
            vp.zoom_center = None;
            vp.offset = canvas_rect.center().to_vec2() / vp.zoom - world.to_vec2();
        }

        self.state
            .viewport
            .handle_input(&response, canvas_rect, self.state.space_held);

        let dt = ui.input(|i| i.stable_dt);
        let animating = self
            .state
            .viewport
            .animate(dt, self.state.settings.smooth_zoom);
        if animating {
            ui.ctx().request_repaint();
        }

        let painter = ui.painter_at(canvas_rect);

        CanvasRenderer::draw_background(&painter, &self.state.viewport, canvas_rect, &self.state.theme);

        self.update_folder_bounds();
        self.draw_folder_nodes(&painter);
        self.draw_file_nodes(&painter, canvas_rect);
        self.draw_edges(&painter);

        if self.state.settings.show_minimap {
            let node_rects = self.collect_node_rects();
            self.minimap
                .draw(ui, &self.state.viewport, &node_rects, canvas_rect);
        }

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let world_pos = self.state.viewport.screen_to_world(pos);
                if let Some(id) = self.state.node_store.node_at_position(
                    world_pos,
                    self.state.settings.node_width,
                    self.state.settings.font_size,
                ) {
                    if ui.input(|i| i.modifiers.shift) {
                        self.state.selection.toggle(id);
                    } else {
                        self.state.selection.select(id);
                    }
                } else {
                    self.state.selection.clear();
                }
            }
        }

        // Only a plain left-drag moves the node; right-drag and Space+drag pan the view.
        let dragging_node = response.dragged_by(egui::PointerButton::Primary)
            && !self.state.space_held
            && self.state.active_tool == Tool::Select;
        if dragging_node {
            if let Some(id) = self.state.selection.primary {
                let delta = response.drag_delta() / self.state.viewport.zoom;
                if let Some(node) = self.state.node_store.get_file_mut(id) {
                    node.position += delta;
                }
            }
        }
    }

    fn draw_folder_nodes(&self, painter: &egui::Painter) {
        let theme = &self.state.theme;
        for id in self.state.node_store.all_folder_ids() {
            if let Some(folder) = self.state.node_store.get_folder(id) {
                let screen_rect = self.state.viewport.world_to_screen_rect(folder.bounds);
                if !painter.clip_rect().intersects(screen_rect) {
                    continue;
                }
                painter.rect_filled(screen_rect, 4.0, theme.folder_bg);
                painter.rect_stroke(screen_rect, 4.0, egui::Stroke::new(1.0_f32, theme.folder_border), egui::StrokeKind::Outside);
                let label_pos = screen_rect.left_top() + egui::vec2(8.0, 4.0);
                let label = format!("{} ({})", folder.label, folder.file_count);
                painter.text(
                    label_pos,
                    egui::Align2::LEFT_TOP,
                    &label,
                    egui::FontId::proportional((11.0 * self.state.viewport.zoom).max(10.0)),
                    theme.text_secondary,
                );
            }
        }
    }

    fn update_folder_bounds(&mut self) {
        let folder_ids = self.state.node_store.all_folder_ids();
        for folder_id in folder_ids {
            let child_rects: Vec<egui::Rect> = {
                let folder = match self.state.node_store.get_folder(folder_id) {
                    Some(f) => f,
                    None => continue,
                };
                folder
                    .children_ids
                    .iter()
                    .filter_map(|&id| self.state.node_store.get_file(id))
                    .map(|n| {
                        n.world_rect(
                            self.state.settings.node_width,
                            self.state.settings.font_size,
                        )
                    })
                    .collect()
            };
            if let Some(folder) = self.state.node_store.get_folder_mut(folder_id) {
                folder.file_count = child_rects.len();
                folder.update_bounds(&child_rects, self.state.layout_settings.folder_padding);
            }
        }
    }

    fn draw_file_nodes(&mut self, painter: &egui::Painter, canvas_rect: egui::Rect) {
        let zoom = self.state.viewport.zoom;
        let lod = LodLevel::from_zoom(zoom);
        let theme = &self.state.theme;
        let node_width = self.state.settings.node_width;
        let font_size = self.state.settings.font_size;
        let visible_world = self.state.viewport.visible_world_rect(canvas_rect);
        let selected_ids: std::collections::HashSet<u64> = self.state.selection.selected_ids().collect();
        let edge_spans = self.state.edge_store.selected_spans(&selected_ids);

        for node in self.state.node_store.file_nodes.values() {
            let world_rect = node.world_rect(node_width, font_size);
            if !world_rect.intersects(visible_world) {
                continue;
            }

            let screen_rect = self.state.viewport.world_to_screen_rect(world_rect);
            let selected = self.state.selection.is_selected(node.id);
            let border_color = if selected { theme.selection } else { theme.node_border };
            let border_width: f32 = if selected { 2.0 } else { 1.0 };
            let rounding = (4.0 * zoom).clamp(1.0, 4.0);

            if lod == LodLevel::TitleOnly {
                // Far away: solid block + centered name, sized to stay readable.
                painter.rect_filled(screen_rect, rounding, theme.node_header);
                painter.rect_stroke(
                    screen_rect,
                    rounding,
                    egui::Stroke::new(border_width, border_color),
                    egui::StrokeKind::Outside,
                );
                draw_fitted_label(
                    painter,
                    node.file_name(),
                    screen_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    screen_rect.width(),
                    14.0,
                    theme.text_primary,
                );
                continue;
            }

            painter.rect_filled(screen_rect, rounding, theme.node_bg);
            painter.rect_stroke(
                screen_rect,
                rounding,
                egui::Stroke::new(border_width, border_color),
                egui::StrokeKind::Outside,
            );

            let header_rect = egui::Rect::from_min_size(
                screen_rect.left_top(),
                egui::vec2(screen_rect.width(), HEADER_HEIGHT * zoom),
            );
            let r = rounding as u8;
            painter.rect_filled(
                header_rect,
                egui::CornerRadius { nw: r, ne: r, sw: 0, se: 0 },
                theme.node_header,
            );

            let title_px = font_size * zoom;
            if title_px >= 8.0 {
                painter.text(
                    header_rect.left_center() + egui::vec2(8.0 * zoom, 0.0),
                    egui::Align2::LEFT_CENTER,
                    node.file_name(),
                    egui::FontId::monospace(title_px),
                    theme.text_primary,
                );
            } else {
                // Header text would be illegible: float a fixed-size label above the node.
                draw_fitted_label(
                    painter,
                    node.file_name(),
                    screen_rect.left_top() - egui::vec2(0.0, 2.0),
                    egui::Align2::LEFT_BOTTOM,
                    screen_rect.width(),
                    11.0,
                    theme.text_secondary,
                );
            }

            if node.content.is_empty() {
                continue;
            }

            let highlighted = self.highlighter.highlight_file(&node.path, &node.content);
            let lines = &highlighted[..highlighted.len().min(node.visible_lines())];
            let code_origin = egui::pos2(
                screen_rect.left() + 8.0 * zoom,
                header_rect.bottom() + CODE_PADDING * zoom,
            );
            let line_h = line_height(font_size) * zoom;
            let code_clip = painter.with_clip_rect(screen_rect.intersect(painter.clip_rect()));

            // Under the code text: the statements joined by the edges of the
            // selected nodes, the lines of the CST or LSP row under the
            // pointer, and the line of the last reference jumped to.
            let lsp_target = self
                .state
                .lsp
                .target
                .filter(|t| self.state.selection.primary == Some(t.node_id));
            let hl_origin = egui::pos2(screen_rect.left(), code_origin.y);
            let char_width = font_size * zoom * 0.6;

            // Edge highlights: block + left border + optional name highlight.
            if let Some(highlights) = edge_spans.get(&node.id) {
                for hl in highlights {
                    let color = self.state.edge_style.kind_color(&hl.label);
                    if let Some(rect) = cst_panel::highlight_rect(
                        hl_origin, screen_rect.width(), line_h,
                        hl.span.start, hl.span.end, node.visible_lines(),
                    ) {
                        code_clip.rect_filled(rect, 0.0, color.gamma_multiply(0.12));
                        let border = egui::Rect::from_min_size(
                            rect.left_top(),
                            egui::vec2(3.0 * zoom, rect.height()),
                        );
                        code_clip.rect_filled(border, 0.0, color.gamma_multiply(0.7));
                    }
                    if let Some(ns) = hl.name_span {
                        if ns.line < node.visible_lines() {
                            let y = code_origin.y + ns.line as f32 * line_h;
                            let x = code_origin.x + ns.start_byte as f32 * char_width;
                            let w = (ns.end_byte - ns.start_byte) as f32 * char_width;
                            let name_rect = egui::Rect::from_min_size(
                                egui::pos2(x, y),
                                egui::vec2(w, line_h),
                            );
                            code_clip.rect_filled(name_rect, 2.0, color.gamma_multiply(0.35));
                        }
                    }
                }
            }

            // CST/LSP hover highlights and reference-jump target.
            let pointer_lines = [
                self.state.cst.hovered.map(|h| (h.node_id, h.start_line, h.end_line, 0.25)),
                self.state.lsp.hovered.map(|h| (h.node_id, h.start_line, h.end_line, 0.25)),
                lsp_target.map(|t| (t.node_id, t.start_line, t.end_line, 0.4)),
            ];
            for (_, start, end, alpha) in pointer_lines.into_iter().flatten().filter(|h| h.0 == node.id) {
                if let Some(rect) = cst_panel::highlight_rect(
                    hl_origin, screen_rect.width(), line_h,
                    start, end, node.visible_lines(),
                ) {
                    code_clip.rect_filled(rect, 0.0, theme.selection.gamma_multiply(alpha));
                }
            }

            match lod {
                LodLevel::Full => {
                    draw_code_text(&code_clip, lines, code_origin, line_h, font_size * zoom)
                }
                LodLevel::Minified => {
                    draw_code_bars(&code_clip, lines, code_origin, line_h, font_size * zoom)
                }
                LodLevel::TitleOnly => unreachable!(),
            }
        }
    }

    fn draw_edges(&self, painter: &egui::Painter) {
        let selected: std::collections::HashSet<u64> = self.state.selection.selected_ids().collect();
        edge_renderer::draw_edges(
            painter,
            &self.state.edge_store,
            &self.state.node_store,
            &self.state.viewport,
            &self.state.edge_style,
            &selected,
            self.state.settings.node_width,
            self.state.settings.font_size,
        );
    }

    fn collect_node_rects(&self) -> Vec<egui::Rect> {
        self.state
            .node_store
            .all_file_ids()
            .iter()
            .filter_map(|&id| {
                self.state.node_store.get_file(id).map(|n| {
                    n.world_rect(
                        self.state.settings.node_width,
                        self.state.settings.font_size,
                    )
                })
            })
            .collect()
    }
}

impl eframe::App for SpatialCodingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        input::handle_shortcuts(ctx, &mut self.state);
        crate::workspace::poll_lsp(&mut self.state);
        // The indexer's requests carry no context to wake the UI: check back
        // while answers are on their way.
        if self.state.deps.is_waiting() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else if std::mem::take(&mut self.layout_when_indexed) {
            self.apply_layout();
            self.pending_fit = true;
        }

        if self.state.loaded_dir.is_some() && self.file_tree.is_none() {
            if let Some(ref dir) = self.state.loaded_dir {
                match crate::fs::loader::load_directory(dir, self.state.settings.show_dot_folders) {
                    Ok(tree) => self.file_tree = Some(tree),
                    Err(e) => log::error!("Failed to load directory: {}", e),
                }
            }
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui::toolbar::draw_toolbar(ui, &mut self.state);
        });

        ui::sidebar_left::draw_sidebar_left(ctx, &mut self.state, &mut self.file_tree);
        ui::sidebar_right::draw_sidebar_right(ctx, &mut self.state);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(self.state.theme.canvas_bg))
            .show(ctx, |ui| {
                self.draw_canvas(ui);
            });

        ui::settings_dialog::draw_settings(ctx, &mut self.state);
        ui::shortcuts_dialog::draw_shortcuts(ctx, &mut self.state.show_shortcuts);
        ui::command_palette::draw_command_palette(ctx, &mut self.state.show_command_palette);

        if self.state.loaded_dir.is_none() {
            self.draw_welcome_screen(ctx);
        }
    }
}

impl SpatialCodingApp {
    fn draw_welcome_screen(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("welcome"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(500.0);
                    ui.vertical_centered(|ui| {
                        ui.heading("SpatialCoding");
                        ui.add_space(8.0);
                        ui.label("Spatial code visualization");
                        ui.add_space(16.0);

                        if ui.button("Open folder...").clicked() {
                            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                self.open_folder(dir);
                            }
                        }

                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("Ctrl+/ for shortcuts")
                                .small()
                                .color(self.state.theme.text_muted),
                        );
                    });
                });
            });
    }
}

type HighlightedLine = Vec<(syntect::highlighting::Style, String)>;

/// Approximate advance of one monospace glyph, as a fraction of font size.
const MONO_ADVANCE: f32 = 0.6;

/// Draws `text` with a font no larger than `max_px`, shrunk to fit `max_width`.
/// Skips drawing when it would be too small to read.
fn draw_fitted_label(
    painter: &egui::Painter,
    text: &str,
    pos: egui::Pos2,
    align: egui::Align2,
    max_width: f32,
    max_px: f32,
    color: egui::Color32,
) {
    let chars = text.chars().count().max(1) as f32;
    let px = max_px.min((max_width - 4.0) / (chars * MONO_ADVANCE));
    if px < 5.0 {
        return;
    }
    painter.text(pos, align, text, egui::FontId::monospace(px), color);
}

/// Full LOD: real syntax-highlighted text.
fn draw_code_text(
    painter: &egui::Painter,
    lines: &[HighlightedLine],
    origin: egui::Pos2,
    line_h: f32,
    font_px: f32,
) {
    let font = egui::FontId::monospace(font_px * 0.85);
    let bottom = painter.clip_rect().bottom();
    for (i, spans) in lines.iter().enumerate() {
        let y = origin.y + i as f32 * line_h;
        if y > bottom {
            break;
        }
        let mut x = origin.x;
        for (style, text) in spans {
            let text = text.trim_end_matches(['\n', '\r']);
            if text.is_empty() {
                continue;
            }
            let rect = painter.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_TOP,
                text,
                font.clone(),
                Highlighter::style_to_egui_color(style),
            );
            x += rect.width();
        }
    }
}

/// Minified LOD: each token drawn as a colored bar (minimap style). When lines
/// get very thin, collapses to one bar per line to keep the shape count low.
fn draw_code_bars(
    painter: &egui::Painter,
    lines: &[HighlightedLine],
    origin: egui::Pos2,
    line_h: f32,
    font_px: f32,
) {
    let char_w = font_px * 0.85 * MONO_ADVANCE;
    let bar_h = (line_h * 0.6).max(1.0);
    let per_token = line_h >= 4.0;
    let bottom = painter.clip_rect().bottom();

    for (i, spans) in lines.iter().enumerate() {
        let y = origin.y + i as f32 * line_h + (line_h - bar_h) * 0.5;
        if y > bottom {
            break;
        }
        let mut col = 0usize;
        let mut line_span: Option<(usize, usize, egui::Color32)> = None;
        for (style, text) in spans {
            let text = text.trim_end_matches(['\n', '\r']);
            let len = visual_width(text);
            let lead = visual_width(&text[..text.len() - text.trim_start().len()]);
            let trail = visual_width(&text[text.trim_end().len()..]);
            let (start, end) = (col + lead, col + len - trail);
            col += len;
            if end <= start {
                continue;
            }
            let color = Highlighter::style_to_egui_color(style).gamma_multiply(0.75);
            if per_token {
                bar(painter, origin.x, y, start, end, char_w, bar_h, color);
            } else {
                line_span = Some(match line_span {
                    Some((s, _, c)) => (s, end, c),
                    None => (start, end, color),
                });
            }
        }
        if let Some((s, e, c)) = line_span {
            bar(painter, origin.x, y, s, e, char_w, bar_h, c);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn bar(
    painter: &egui::Painter,
    x0: f32,
    y: f32,
    start: usize,
    end: usize,
    char_w: f32,
    h: f32,
    color: egui::Color32,
) {
    let rect = egui::Rect::from_min_size(
        egui::pos2(x0 + start as f32 * char_w, y),
        egui::vec2((end - start) as f32 * char_w, h),
    );
    painter.rect_filled(rect, 0.0, color);
}

/// Column width of `s`, counting tabs as 4.
fn visual_width(s: &str) -> usize {
    s.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_width_counts_tabs_as_four_columns() {
        assert_eq!(visual_width(""), 0);
        assert_eq!(visual_width("abc"), 3);
        assert_eq!(visual_width("\tx"), 5);
        assert_eq!(visual_width("\t\t"), 8);
        assert_eq!(visual_width("olá"), 3, "counts chars, not bytes");
    }
}
