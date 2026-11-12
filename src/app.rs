use crate::canvas::minimap::Minimap;
use crate::canvas::renderer::CanvasRenderer;
use crate::edges::renderer as edge_renderer;
use crate::fs::tree::TreeNode;
use crate::highlight::syntect_hl::Highlighter;
use crate::input;
use crate::nodes::lod::LodLevel;
use crate::state::{AppState, Tool};
use crate::ui;

pub struct SpatialCodingApp {
    state: AppState,
    file_tree: Option<TreeNode>,
    highlighter: Highlighter,
    minimap: Minimap,
}

impl SpatialCodingApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            state: AppState::default(),
            file_tree: None,
            highlighter: Highlighter::new(),
            minimap: Minimap::default(),
        }
    }

    fn draw_canvas(&mut self, ui: &mut egui::Ui) {
        let canvas_rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(canvas_rect, egui::Sense::click_and_drag());

        self.state
            .viewport
            .handle_input(ui, canvas_rect, self.state.space_held);

        let dt = ui.input(|i| i.stable_dt);
        self.state
            .viewport
            .animate(dt, self.state.settings.smooth_zoom);

        let painter = ui.painter_at(canvas_rect);

        CanvasRenderer::draw_background(&painter, &self.state.viewport, canvas_rect, &self.state.theme);

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
                let lod = LodLevel::from_zoom(self.state.viewport.zoom);
                if let Some(id) = self.state.node_store.node_at_position(
                    world_pos,
                    self.state.settings.node_width,
                    self.state.settings.font_size,
                    lod,
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

        if response.dragged() && self.state.active_tool == Tool::Select {
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
                    egui::FontId::proportional(11.0 * self.state.viewport.zoom),
                    theme.text_secondary,
                );
            }
        }
    }

    fn draw_file_nodes(&mut self, painter: &egui::Painter, canvas_rect: egui::Rect) {
        let lod = LodLevel::from_zoom(self.state.viewport.zoom);
        let theme = &self.state.theme;
        let visible_world = self.state.viewport.visible_world_rect(canvas_rect);

        let file_ids = self.state.node_store.all_file_ids();
        for id in file_ids {
            let (world_rect, file_name, _position, content_key) = {
                let node = match self.state.node_store.get_file(id) {
                    Some(n) => n,
                    None => continue,
                };
                let wr = node.world_rect(self.state.settings.node_width, self.state.settings.font_size, lod);
                if !wr.intersects(visible_world) {
                    continue;
                }
                (
                    wr,
                    node.file_name().to_string(),
                    node.position,
                    node.path.clone(),
                )
            };

            let screen_rect = self.state.viewport.world_to_screen_rect(world_rect);
            let selected = self.state.selection.is_selected(id);

            let border_color = if selected {
                theme.selection
            } else {
                theme.node_border
            };
            let border_width: f32 = if selected { 2.0 } else { 1.0 };

            painter.rect_filled(screen_rect, 4.0, theme.node_bg);
            painter.rect_stroke(
                screen_rect,
                4.0,
                egui::Stroke::new(border_width, border_color),
                egui::StrokeKind::Outside,
            );

            let header_height = 24.0 * self.state.viewport.zoom;
            let header_rect = egui::Rect::from_min_size(
                screen_rect.left_top(),
                egui::vec2(screen_rect.width(), header_height),
            );
            painter.rect_filled(header_rect, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, theme.node_header);

            let text_pos = header_rect.left_top() + egui::vec2(8.0 * self.state.viewport.zoom, 4.0 * self.state.viewport.zoom);
            let font_size = (self.state.settings.font_size * self.state.viewport.zoom).max(6.0);
            painter.text(
                text_pos,
                egui::Align2::LEFT_TOP,
                &file_name,
                egui::FontId::monospace(font_size),
                theme.text_primary,
            );

            if lod == LodLevel::Full {
                if let Some(content) = self.state.file_contents.get(&content_key) {
                    let code_top = header_rect.bottom() + 4.0 * self.state.viewport.zoom;
                    let max_lines = ((screen_rect.bottom() - code_top) / (font_size * 1.4)).max(1.0) as usize;
                    let lines: Vec<&str> = content.lines().take(max_lines).collect();
                    let code_font = egui::FontId::monospace(font_size * 0.85);
                    for (i, line) in lines.iter().enumerate() {
                        let y = code_top + i as f32 * font_size * 1.3;
                        if y > screen_rect.bottom() {
                            break;
                        }
                        painter.text(
                            egui::pos2(screen_rect.left() + 8.0 * self.state.viewport.zoom, y),
                            egui::Align2::LEFT_TOP,
                            line,
                            code_font.clone(),
                            theme.text_secondary,
                        );
                    }
                }
            } else if lod == LodLevel::Summary {
                if let Some(node) = self.state.node_store.get_file(id) {
                    let info = format!("{} lines", node.line_count);
                    painter.text(
                        egui::pos2(
                            screen_rect.left() + 8.0 * self.state.viewport.zoom,
                            header_rect.bottom() + 4.0 * self.state.viewport.zoom,
                        ),
                        egui::Align2::LEFT_TOP,
                        &info,
                        egui::FontId::monospace(font_size * 0.8),
                        theme.text_muted,
                    );
                }
            }
        }
    }

    fn draw_edges(&self, painter: &egui::Painter) {
        let lod = LodLevel::from_zoom(self.state.viewport.zoom);
        let selected: std::collections::HashSet<u64> = self.state.selection.selected_ids().collect();
        edge_renderer::draw_edges(
            painter,
            &self.state.edge_store,
            &self.state.node_store,
            &self.state.viewport,
            &self.state.edge_style,
            &selected,
            lod,
            self.state.settings.node_width,
            self.state.settings.font_size,
        );
    }

    fn collect_node_rects(&self) -> Vec<egui::Rect> {
        let lod = LodLevel::from_zoom(self.state.viewport.zoom);
        self.state
            .node_store
            .all_file_ids()
            .iter()
            .filter_map(|&id| {
                self.state.node_store.get_file(id).map(|n| {
                    n.world_rect(
                        self.state.settings.node_width,
                        self.state.settings.font_size,
                        lod,
                    )
                })
            })
            .collect()
    }
}

impl eframe::App for SpatialCodingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        input::handle_shortcuts(ctx, &mut self.state);

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
                                self.state.loaded_dir = Some(dir.clone());
                                self.file_tree = None;
                                match crate::fs::loader::load_directory(
                                    &dir,
                                    self.state.settings.show_dot_folders,
                                ) {
                                    Ok(tree) => self.file_tree = Some(tree),
                                    Err(e) => log::error!("Failed to load: {}", e),
                                }
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
