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
use crate::nodes::selection::SelectedSymbol;
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
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::icons::install(&cc.egui_ctx);
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

        CanvasRenderer::draw_background(
            &painter,
            &self.state.viewport,
            canvas_rect,
            &self.state.theme,
            self.state.settings.show_grid,
        );

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
                    let additive = ui.input(|i| i.modifiers.shift);
                    if let Some(symbol) = self.symbol_at(&painter, id, pos) {
                        self.state.selection.select_symbol(id, symbol, additive);
                    } else if additive {
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

    /// Pick only the exact name returned by LSP, measured with the same token
    /// spans and font as painting. Whitespace and keywords select the file.
    fn symbol_at(&mut self, painter: &egui::Painter, node_id: u64, screen: egui::Pos2) -> Option<SelectedSymbol> {
        let node = self.state.node_store.get_file(node_id)?;
        let zoom = self.state.viewport.zoom;
        let lod = LodLevel::from_zoom(zoom);
        if node.collapsed || lod == LodLevel::TitleOnly {
            return None;
        }
        let font_size = self.state.settings.font_size;
        let world = self.state.viewport.screen_to_world(screen);
        let code_y = node.position.y + HEADER_HEIGHT + CODE_PADDING;
        if world.y < code_y { return None }
        let line = ((world.y - code_y) / line_height(font_size)).floor() as usize;
        if line >= node.visible_lines() { return None }
        let spans = self.highlighter.highlight_file(&node.path, &node.content).get(line)?;
        let code_x = self.state.viewport.world_to_screen(node.position).x + 8.0 * zoom;
        for occurrence in self.state.deps.symbol_occurrences(&node.path, line) {
            let (_, start, end) = occurrence.name_span;
            let Some((x0, x1)) = name_x_range(painter, lod, spans, start, end, font_size * zoom) else { continue };
            if screen.x < code_x + x0 || screen.x >= code_x + x1 { continue }
            let definition = self.state.node_store.file_nodes.values()
                .find(|n| n.path == occurrence.definition_path)?;
            let (line, start_byte, end_byte) = occurrence.definition_name_span;
            return Some(SelectedSymbol {
                node_id: definition.id,
                name_span: crate::edges::edge_store::NameSpan { line, start_byte, end_byte },
                span: crate::analysis::dependencies::LineSpan::of_range(occurrence.symbol.range),
                label: occurrence.symbol.kind.label().to_string(),
            });
        }
        None
    }

    fn draw_folder_nodes(&self, painter: &egui::Painter) {
        let theme = &self.state.theme;
        for id in self.state.node_store.all_folder_ids() {
            if let Some(folder) = self.state.node_store.get_folder(id) {
                let screen_rect = self.state.viewport.world_to_screen_rect(folder.bounds);
                if !painter.clip_rect().intersects(screen_rect) {
                    continue;
                }
                // v2 folders: unfilled square outline, dim name just above it.
                painter.rect_filled(screen_rect, 0.0, theme.folder_bg);
                painter.rect_stroke(
                    screen_rect,
                    0.0,
                    egui::Stroke::new(1.0_f32, theme.folder_border),
                    egui::StrokeKind::Outside,
                );
                // Sized in world units so a nested folder's label never
                // runs into its parent's at overview zoom; hidden when tiny.
                let label_px = (FOLDER_LABEL_WORLD_PX * self.state.viewport.zoom).min(16.0);
                if label_px >= 6.0 {
                    painter.text(
                        screen_rect.left_top() - egui::vec2(0.0, 3.0),
                        egui::Align2::LEFT_BOTTOM,
                        &folder.label,
                        egui::FontId::proportional(label_px),
                        theme.text_muted,
                    );
                }
            }
        }
    }

    fn update_folder_bounds(&mut self) {
        self.state.node_store.update_folder_bounds(
            self.state.settings.node_width,
            self.state.settings.font_size,
            self.state.layout_settings.folder_padding,
        );
    }

    fn draw_file_nodes(&mut self, painter: &egui::Painter, canvas_rect: egui::Rect) {
        let zoom = self.state.viewport.zoom;
        let lod = LodLevel::from_zoom(zoom);
        let theme = &self.state.theme;
        let node_width = self.state.settings.node_width;
        let font_size = self.state.settings.font_size;
        let visible_world = self.state.viewport.visible_world_rect(canvas_rect);
        let edge_spans = self.state.edge_store.focused_spans(&self.state.selection, &self.state.edge_style);

        for node in self.state.node_store.file_nodes.values() {
            let world_rect = node.world_rect(node_width, font_size);
            if !world_rect.intersects(visible_world) {
                continue;
            }

            let screen_rect = self.state.viewport.world_to_screen_rect(world_rect);
            let selected = self.state.selection.is_selected(node.id);
            let border_color = if selected { theme.selected_border } else { theme.node_border };
            let border_width: f32 = 1.0;
            let rounding = (4.0 * zoom).clamp(1.0, 4.0);

            // v2 Code Canvas style: no interior header bar; the file name
            // floats above the rectangle as a secondary label.
            let label_anchor = screen_rect.center_top() - egui::vec2(0.0, 4.0);
            let label_size = (font_size * zoom).max(10.0).min(14.0);
            draw_fitted_label(
                painter,
                node.file_name(),
                label_anchor,
                egui::Align2::CENTER_BOTTOM,
                screen_rect.width(),
                label_size,
                theme.text_secondary,
            );

            if lod == LodLevel::TitleOnly {
                // Far away: solid block only — the floating label above is the name.
                painter.rect_filled(screen_rect, rounding, theme.node_bg);
                painter.rect_stroke(
                    screen_rect,
                    rounding,
                    egui::Stroke::new(border_width, border_color),
                    egui::StrokeKind::Outside,
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

            // Reserve the top of the body as padding (where the header bar used to sit),
            // so code lines line up with the edge anchors driven by HEADER_HEIGHT.
            let header_rect = egui::Rect::from_min_size(
                screen_rect.left_top(),
                egui::vec2(screen_rect.width(), HEADER_HEIGHT * zoom),
            );

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

            // Edge highlights: block + left border + optional name highlight.
            if let Some(highlights) = edge_spans.get(&node.id) {
                for hl in highlights {
                    let color = self.state.edge_style.kind_color(&hl.label);
                    if let Some(rect) = cst_panel::highlight_rect(
                        hl_origin, screen_rect.width(), line_h,
                        hl.span.start, hl.span.end, node.visible_lines(),
                    ) {
                        code_clip.rect_filled(rect, 0.0, color.gamma_multiply(0.07));
                        let border = egui::Rect::from_min_size(
                            rect.left_top(),
                            egui::vec2(2.0 * zoom, rect.height()),
                        );
                        code_clip.rect_filled(border, 0.0, color.gamma_multiply(0.85));
                    }
                    let name_x = hl.name_span.filter(|ns| ns.line < node.visible_lines()).and_then(|ns| {
                        let spans = lines.get(ns.line)?;
                        let x = name_x_range(&code_clip, lod, spans, ns.start_byte, ns.end_byte, font_size * zoom)?;
                        Some((ns.line, x))
                    });
                    if let Some((line, (x0, x1))) = name_x {
                        let y = code_origin.y + line as f32 * line_h;
                        let name_rect = egui::Rect::from_min_max(
                            egui::pos2(code_origin.x + x0, y),
                            egui::pos2(code_origin.x + x1, y + line_h),
                        );
                        code_clip.rect_filled(name_rect, 1.5, color.gamma_multiply(0.30));
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

    fn draw_edges(&mut self, painter: &egui::Painter) {
        let viewport = &self.state.viewport;
        let zoom = viewport.zoom;
        let lod = LodLevel::from_zoom(zoom);
        let font_size = self.state.settings.font_size;
        let node_width = self.state.settings.node_width;
        let highlighter = &mut self.highlighter;
        let mut symbol_rect = |node: &crate::nodes::file_node::FileNode, name: crate::edges::edge_store::NameSpan| {
            if node.collapsed || lod == LodLevel::TitleOnly || name.line >= node.visible_lines() {
                return None;
            }
            let spans = highlighter.highlight_file(&node.path, &node.content).get(name.line)?;
            let (x0, x1) = name_x_range(painter, lod, spans, name.start_byte, name.end_byte, font_size * zoom)?;
            let origin = viewport.world_to_screen(node.position);
            let line_h = line_height(font_size) * zoom;
            let line_top = origin.y + (HEADER_HEIGHT + CODE_PADDING) * zoom + name.line as f32 * line_h;
            let (top, height) = if lod == LodLevel::Full {
                let text = node.content.lines().nth(name.line)?.get(name.start_byte..name.end_byte)?;
                let height = painter.layout_no_wrap(text.to_string(), code_font(font_size * zoom), egui::Color32::PLACEHOLDER).size().y;
                (line_top, height)
            } else {
                let height = (line_h * 0.6).max(1.0);
                (line_top + (line_h - height) * 0.5, height)
            };
            let rect = egui::Rect::from_min_max(
                egui::pos2(origin.x + 8.0 * zoom + x0, top),
                egui::pos2(origin.x + 8.0 * zoom + x1, top + height),
            );
            viewport.world_to_screen_rect(node.world_rect(node_width, font_size)).contains_rect(rect).then_some(rect)
        };
        edge_renderer::draw_edges(
            painter,
            &self.state.edge_store,
            &self.state.node_store,
            &self.state.viewport,
            &self.state.edge_style,
            &self.state.selection,
            self.state.settings.node_width,
            self.state.settings.font_size,
            &mut symbol_rect,
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
/// Folder label height in world units: fits in the parent folder's padding.
const FOLDER_LABEL_WORLD_PX: f32 = 36.0;

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
    // Proportional, as v2's file labels; the mono advance over-estimates
    // its width, so the fit stays conservative.
    painter.text(pos, align, text, egui::FontId::proportional(px), color);
}

/// Full LOD: real syntax-highlighted text.
fn draw_code_text(
    painter: &egui::Painter,
    lines: &[HighlightedLine],
    origin: egui::Pos2,
    line_h: f32,
    font_px: f32,
) {
    let font = code_font(font_px);
    let rows = visible_code_rows(origin.y, line_h, lines.len(), painter.clip_rect());
    for (i, spans) in lines.iter().enumerate().take(rows.end).skip(rows.start) {
        let y = origin.y + i as f32 * line_h;
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
    let char_w = code_char_width(font_px);
    let bar_h = (line_h * 0.6).max(1.0);
    let per_token = line_h >= 4.0;
    let rows = visible_code_rows(origin.y, line_h, lines.len(), painter.clip_rect());
    for (i, spans) in lines.iter().enumerate().take(rows.end).skip(rows.start) {
        let y = origin.y + i as f32 * line_h + (line_h - bar_h) * 0.5;
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

/// Files retain all their rows; only painting is restricted to the viewport.
fn visible_code_rows(top: f32, line_h: f32, count: usize, clip: egui::Rect) -> std::ops::Range<usize> {
    if line_h <= 0.0 { return 0..0 }
    let start = (((clip.top() - top) / line_h).floor().max(0.0) as usize).min(count);
    let end = (((clip.bottom() - top) / line_h).ceil().max(0.0) as usize).min(count);
    start..end.max(start)
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

/// Font of the code text inside a file node, for a node font size of `font_px`.
fn code_font(font_px: f32) -> egui::FontId {
    egui::FontId::monospace(font_px * 0.85)
}

/// Approximate column width of the code font, as the minified bars use it.
fn code_char_width(font_px: f32) -> f32 {
    font_px * 0.85 * MONO_ADVANCE
}

/// Horizontal range, relative to the code origin, of bytes `start..end` of a
/// highlighted line as the current LOD draws it: measured span by span with
/// the real code font at full LOD (the same layout as [`draw_code_text`]),
/// in bar columns when minified. `None` when the range is not in the line.
fn name_x_range(
    painter: &egui::Painter,
    lod: LodLevel,
    spans: &HighlightedLine,
    start: usize,
    end: usize,
    font_px: f32,
) -> Option<(f32, f32)> {
    let char_w = code_char_width(font_px);
    let width = |s: &str| -> f32 {
        let s = s.trim_end_matches(['\n', '\r']);
        if s.is_empty() {
            return 0.0;
        }
        match lod {
            LodLevel::Full => painter
                .layout_no_wrap(s.to_string(), code_font(font_px), egui::Color32::PLACEHOLDER)
                .size()
                .x,
            _ => visual_width(s) as f32 * char_w,
        }
    };
    let x0 = x_at_byte(spans, start, &width)?;
    let x1 = x_at_byte(spans, end, &width)?;
    (x1 > x0).then_some((x0, x1))
}

/// x offset of byte `at` of the line made of `spans`: the width of every
/// span before it plus the part of the span it falls in. `None` when `at`
/// is past the line or not on a char boundary.
fn x_at_byte(spans: &HighlightedLine, at: usize, width: &dyn Fn(&str) -> f32) -> Option<f32> {
    let mut x = 0.0;
    let mut offset = 0;
    for (_, text) in spans {
        if at <= offset + text.len() {
            return Some(x + width(text.get(..at - offset)?));
        }
        x += width(text);
        offset += text.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_viewport_rows_are_painted_without_truncating_the_file() {
        let clip = egui::Rect::from_min_max(egui::pos2(0.0, 1000.0), egui::pos2(700.0, 1100.0));
        assert_eq!(visible_code_rows(0.0, 10.0, 10000, clip), 100..110);
        assert_eq!(visible_code_rows(0.0, 10.0, 105, clip), 100..105);
    }

    #[test]
    fn visual_width_counts_tabs_as_four_columns() {
        assert_eq!(visual_width(""), 0);
        assert_eq!(visual_width("abc"), 3);
        assert_eq!(visual_width("\tx"), 5);
        assert_eq!(visual_width("\t\t"), 8);
        assert_eq!(visual_width("olá"), 3, "counts chars, not bytes");
    }

    #[test]
    fn x_at_byte_sums_spans_before_the_byte_and_part_of_its_span() {
        let style = syntect::highlighting::Style::default();
        let line: HighlightedLine = vec![
            (style, "pub ".into()),
            (style, "struct".into()),
            (style, " Café {\n".into()),
        ];
        let cols = |s: &str| visual_width(s.trim_end_matches(['\n', '\r'])) as f32;
        assert_eq!(x_at_byte(&line, 0, &cols), Some(0.0));
        assert_eq!(x_at_byte(&line, 4, &cols), Some(4.0), "span boundary");
        assert_eq!(x_at_byte(&line, 6, &cols), Some(6.0), "inside a span");
        assert_eq!(x_at_byte(&line, 11, &cols), Some(11.0));
        assert_eq!(x_at_byte(&line, 16, &cols), Some(15.0), "é is one column, two bytes");
        assert_eq!(x_at_byte(&line, 15, &cols), None, "not a char boundary");
        assert_eq!(x_at_byte(&line, 99, &cols), None, "past the line");
    }

    #[test]
    fn name_mark_lines_up_with_the_drawn_glyphs() {
        let style = syntect::highlighting::Style::default();
        let text = "    pub struct SpatialCodingApp {\n";
        let line: HighlightedLine = ["    ", "pub", " ", "struct", " ", "SpatialCodingApp", " {\n"]
            .into_iter()
            .map(|s| (style, s.to_string()))
            .collect();
        let (start, end) = (15, 31);
        let font_px = 23.0;
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = egui::Painter::new(ctx.clone(), egui::LayerId::background(), egui::Rect::EVERYTHING);
            let (x0, x1) = name_x_range(&painter, LodLevel::Full, &line, start, end, font_px).unwrap();
            let galley = painter.layout_no_wrap(text.trim_end().to_string(), code_font(font_px), egui::Color32::WHITE);
            // Whole-line glyphs snap to pixels while the drawn spans are
            // summed, so allow 1 px; the old 0.6-advance guess was ~40 px off.
            let glyph_x = |i| galley.pos_from_ccursor(egui::text::CCursor::new(i)).min.x;
            assert!((x0 - glyph_x(start)).abs() <= 1.0, "start {x0} vs glyph {}", glyph_x(start));
            assert!((x1 - glyph_x(end)).abs() <= 1.0, "end {x1} vs glyph {}", glyph_x(end));
        });
    }
}
