use std::collections::HashMap;
use std::path::PathBuf;

use crate::canvas::minimap::Minimap;
use crate::cli::StartupOptions;
use crate::canvas::renderer::CanvasRenderer;
use crate::edges::ports::{self, Port, PortKind, PortSide};
use crate::edges::renderer as edge_renderer;
use crate::fs::tree::TreeNode;
use crate::highlight::syntect_hl::Highlighter;
use crate::input;
use crate::nodes::file_node::{line_height, CODE_PADDING, CODE_PAD_X, GUTTER_RIGHT, HEADER_HEIGHT};
use crate::nodes::lod::{LodLevel, RenderMode};
use crate::nodes::selection::SelectedSymbol;
use crate::state::{AppState, Tool};
use crate::ui;
use crate::ui::cst_panel;
use crate::workspace;

/// Folders remembered for the repository dropdown.
const MAX_RECENT_FOLDERS: usize = 8;

pub struct SpatialCodingApp {
    state: AppState,
    file_tree: Option<TreeNode>,
    highlighter: Highlighter,
    minimap: Minimap,
    /// Where the panel settings are kept, when persistence is on (binary only).
    settings_path: Option<PathBuf>,
    /// The JSON last written to `settings_path`, to skip unchanged writes.
    saved_settings: String,
    /// When the settings were last compared with `saved_settings`.
    settings_checked_at: f64,
    /// `--layout` was asked: lay out again once the language servers have
    /// answered, since the dependency edges arrive in the background.
    layout_when_indexed: bool,
    /// Follows the loaded folder on disk; `None` until a folder is open or
    /// when the platform refused to watch it.
    watcher: Option<crate::fs::watcher::FileWatcher>,
    /// Wakes the UI from the watcher's thread.
    repaint: egui::Context,
    /// Tokens of the pinned symbol graphs painted this frame.
    pin_marks: Vec<ui::pin_overlay::PinMark>,
    /// The socket MCP agents reach this app through (binary only).
    #[cfg(unix)]
    bridge: Option<crate::server::bridge::Bridge>,
}

impl SpatialCodingApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::icons::install(&cc.egui_ctx);
        let state = AppState::default();
        ui::theme::apply_style(&cc.egui_ctx, &state.theme);
        Self {
            state,
            file_tree: None,
            highlighter: Highlighter::new(),
            minimap: Minimap::default(),
            settings_path: None,
            saved_settings: String::new(),
            settings_checked_at: 0.0,
            layout_when_indexed: false,
            watcher: None,
            repaint: cc.egui_ctx.clone(),
            pin_marks: Vec::new(),
            #[cfg(unix)]
            bridge: None,
        }
    }

    /// Starts the socket an MCP server (`spatialcoding --mcp`) connects to.
    /// Off by default so tests do not open sockets.
    #[cfg(unix)]
    pub fn enable_bridge(&mut self) {
        self.enable_bridge_in(&crate::server::bridge::socket_dir());
    }

    /// Like [`Self::enable_bridge`], with the socket in `dir`.
    #[cfg(unix)]
    pub fn enable_bridge_in(&mut self, dir: &std::path::Path) {
        match crate::server::bridge::Bridge::start(dir, self.repaint.clone()) {
            Ok(bridge) => {
                self.state.mcp_socket = Some(bridge.path().to_path_buf());
                self.bridge = Some(bridge);
            }
            Err(e) => log::warn!("MCP bridge not started: {e}"),
        }
    }

    #[cfg(not(unix))]
    pub fn enable_bridge(&mut self) {}

    /// Answers the tool calls agents sent since the last frame.
    #[cfg(unix)]
    fn serve_agents(&mut self) {
        let Some(bridge) = &self.bridge else { return };
        self.state.agents = bridge.agents();
        for call in bridge.poll() {
            let result = crate::server::api::handle(&mut self.state, &call.tool, &call.args);
            let _ = call.reply.send(result);
        }
        self.run_app_actions();
    }

    #[cfg(not(unix))]
    fn serve_agents(&mut self) {}

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    /// Loads the panel settings from `path`, when it exists, and saves them
    /// back there whenever they change. Off unless called: tests never touch
    /// the user's settings.
    pub fn enable_settings_persistence(&mut self, path: PathBuf) {
        if let Some(saved) = crate::settings_store::load(&path) {
            saved.apply(&mut self.state);
        }
        self.saved_settings = crate::settings_store::to_json(
            &crate::settings_store::PersistedSettings::capture(&self.state),
        );
        self.settings_path = Some(path);
    }

    /// Writes the settings when they changed, at most about once a second.
    fn save_settings_if_changed(&mut self, now: f64) {
        let Some(path) = &self.settings_path else {
            return;
        };
        if now - self.settings_checked_at < 1.0 {
            return;
        }
        self.settings_checked_at = now;
        let json = crate::settings_store::to_json(
            &crate::settings_store::PersistedSettings::capture(&self.state),
        );
        if json != self.saved_settings {
            if let Err(e) = crate::settings_store::save(path, &json) {
                log::warn!("Could not save settings to {}: {e}", path.display());
            }
            self.saved_settings = json;
        }
    }

    pub fn file_tree(&self) -> Option<&TreeNode> {
        self.file_tree.as_ref()
    }

    /// Loads `dir` as the workspace, replacing whatever is on the canvas.
    pub fn open_folder(&mut self, dir: PathBuf) {
        workspace::clear_canvas(&mut self.state);
        // Asked again on the next frame, for the new folder.
        self.state.changes.reset();
        self.state.diff = None;
        self.state.folder_files_cache.replace(None);
        // The ignore list is per repository: the folder must be known first.
        self.state.loaded_dir = Some(dir.clone());
        self.file_tree = match self.load_tree(&dir) {
            Ok(tree) => Some(tree),
            Err(e) => {
                log::error!("Failed to load {}: {e}", dir.display());
                None
            }
        };
        self.watcher = start_watcher(&dir, &self.repaint);
        let recents = &mut self.state.settings.recent_folders;
        recents.retain(|d| d != &dir);
        recents.insert(0, dir.clone());
        recents.truncate(MAX_RECENT_FOLDERS);
        self.state.loaded_dir = Some(dir);
    }

    /// The Workspace tree of `dir`: without the ignored paths, unless
    /// Workspace › Show ignored files is on.
    fn load_tree(&self, dir: &std::path::Path) -> std::io::Result<TreeNode> {
        let ignore = if self.state.settings.show_ignored {
            crate::fs::ignore::IgnoreSet::default()
        } else {
            self.state.ignore_set()
        };
        crate::fs::loader::load_directory_filtered(dir, dir, self.state.settings.show_dot_folders, &ignore)
    }

    /// Applies what the watcher saw on disk to the canvas and the file tree.
    fn follow_disk(&mut self) {
        let Some(watcher) = &mut self.watcher else { return };
        let batch = watcher.poll_changes(self.state.settings.show_dot_folders);
        if batch.is_empty() {
            return;
        }
        let applied = workspace::apply_fs_changes(&mut self.state, &batch);
        for path in &applied.reloaded {
            self.highlighter.invalidate(path);
        }
        if applied.tree_stale
            && let Some(dir) = self.state.loaded_dir.clone()
            && let Ok(mut tree) = self.load_tree(&dir)
        {
            if let Some(old) = &self.file_tree {
                tree.copy_expanded_from(old);
            }
            self.file_tree = Some(tree);
        }
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
        self.state.pending_fit = !self.state.node_store.file_nodes.is_empty();
    }

    /// Runs what the command palette picked.
    pub fn execute_command(&mut self, command: ui::command_palette::PaletteCommand) {
        use ui::command_palette::PaletteCommand as C;
        match command {
            C::Command(id) => {
                if let Err(e) = crate::commands::run(&mut self.state, id, &serde_json::Value::Null) {
                    log::warn!("{e}");
                }
            }
            C::GoToUi(id) => {
                let now = self.repaint.input(|i| i.time);
                crate::commands::go_to_ui(&mut self.state, id, now);
            }
            C::GoToFile(path) => {
                workspace::focus_location(&mut self.state, &path, 0);
            }
        }
        self.run_app_actions();
    }

    /// What commands asked of the shell: the native folder dialog.
    fn run_app_actions(&mut self) {
        for action in std::mem::take(&mut self.state.pending_app_actions) {
            match action {
                crate::commands::AppAction::PickFolder => {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        self.open_folder(dir);
                    }
                }
                crate::commands::AppAction::OpenFolder(dir) => self.open_folder(dir),
                crate::commands::AppAction::ReloadTree => {
                    self.state.folder_files_cache.replace(None);
                    if let Some(dir) = self.state.loaded_dir.clone()
                        && let Ok(mut tree) = self.load_tree(&dir)
                    {
                        if let Some(old) = &self.file_tree {
                            tree.copy_expanded_from(old);
                        }
                        self.file_tree = Some(tree);
                    }
                }
                crate::commands::AppAction::CloseFolder => {
                    workspace::clear_canvas(&mut self.state);
                    self.state.changes.reset();
                    self.state.diff = None;
                    self.state.folder_files_cache.replace(None);
                    self.state.loaded_dir = None;
                    self.file_tree = None;
                    self.watcher = None;
                }
            }
        }
    }

    /// Settings → Viewport Details and Activity Indicator.
    fn draw_canvas_overlays(&self, painter: &egui::Painter, canvas_rect: egui::Rect) {
        let theme = &self.state.theme;
        let font = egui::FontId::monospace(10.5);
        if self.state.settings.show_viewport_details {
            let vp = &self.state.viewport;
            let text = format!("{:.3}x  L{}  {:.0},{:.0}", vp.zoom, LodLevel::from_zoom(vp.zoom).number(), -vp.offset.x, -vp.offset.y);
            painter.text(
                canvas_rect.left_bottom() + egui::vec2(12.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                text,
                font.clone(),
                theme.text_muted,
            );
        }
        if self.state.settings.show_activity_indicator {
            let busy = if self.state.deps.is_waiting() {
                Some("Analyzing dependencies with the language servers…")
            } else if self.state.changes.is_loading() {
                Some("Reading Git changes…")
            } else {
                None
            };
            if let Some(text) = busy {
                painter.text(
                    canvas_rect.center_bottom() + egui::vec2(0.0, -10.0),
                    egui::Align2::CENTER_BOTTOM,
                    text,
                    font,
                    theme.text_secondary,
                );
            }
        }
    }

    fn apply_layout(&mut self) {
        let input = self.state.layout_input();
        crate::layout::hierarchical::apply_layout(
            &mut self.state.node_store,
            &self.state.edge_store,
            &input,
        );
        self.state.layout_dirty = false;
    }

    fn draw_canvas(&mut self, ui: &mut egui::Ui) {
        let canvas_rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(canvas_rect, egui::Sense::click_and_drag());
        // Commands under the pointer (F, Shift+E, Ctrl+R…) read these.
        self.state.canvas_rect = canvas_rect;
        // A file or folder dragged from the Workspace tree and dropped here.
        if let Some(drag) = response.dnd_release_payload::<ui::sidebar_left::TreeDrag>() {
            if drag.0.is_dir() {
                workspace::open_folder_on_canvas(&mut self.state, &drag.0);
            } else {
                workspace::open_file(&mut self.state, &drag.0);
            }
        }
        self.state.hover_screen = ui.input(|i| i.pointer.hover_pos()).filter(|p| canvas_rect.contains(*p));

        if std::mem::take(&mut self.state.pending_fit)
            && let Some(world_rect) = input::compute_nodes_bounding_rect(&self.state)
        {
            self.state.viewport.fit_to_rect(world_rect, canvas_rect);
        }

        if let Some(world) = self.state.pending_focus.take() {
            // Jump to a reference: readable text (unless Keep Zoom), centered
            // on its line, gliding there with Smooth Transition.
            let lsp = &self.state.settings.lsp;
            let vp = &mut self.state.viewport;
            if !lsp.keep_zoom && LodLevel::from_zoom(vp.zoom) != LodLevel::L1 {
                vp.zoom = 1.0;
                vp.target_zoom = 1.0;
            }
            vp.zoom_center = None;
            let target = canvas_rect.center().to_vec2() / vp.zoom - world.to_vec2();
            if lsp.smooth_transition {
                vp.pan_target = Some(target);
            } else {
                vp.pan_target = None;
                vp.offset = target;
            }
        }

        self.state
            .viewport
            .handle_input(
                &response,
                canvas_rect,
                self.state.space_held || self.state.active_tool == crate::state::Tool::Move,
            );

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

        self.state.node_store.view = crate::folder_actions::folder_view(&self.state);
        self.update_folder_bounds();
        let zoom_collapsed = crate::folder_actions::zoom_collapsed(&self.state, self.state.viewport.zoom);
        self.state.node_store.view.zoom_collapsed = zoom_collapsed;
        self.update_canvas_symbol_hover(ui, &response, &painter);
        self.draw_folder_nodes(&painter);
        let ports = self.collect_ports();
        let graph_names = self.draw_file_nodes(&painter, canvas_rect, &ports);
        self.draw_edges(&painter);
        if let Some(graph) = &self.state.lsp.canvas_graph.graph {
            ui::symbol_graph::paint_connections(&painter, graph, &graph_names);
        }
        self.draw_ports(&painter, canvas_rect, &ports);
        // Token edges and, zoomed out, a name pill on every pinned token.
        let pills = self.state.viewport.zoom < crate::nodes::lod::THRESHOLDS[0];
        ui::pin_overlay::paint_connections(&painter, self.state.lsp.pins.pins(), &self.pin_marks, pills);
        self.draw_out_of_view_references(&painter, canvas_rect);
        self.draw_out_of_view_tags(ui, &painter, canvas_rect);

        if self.state.settings.show_minimap {
            let node_rects = self.collect_node_rects();
            if let Some(world) = self.minimap.draw(ui, &self.state.viewport, &node_rects, canvas_rect) {
                let vp = &mut self.state.viewport;
                vp.offset = canvas_rect.center().to_vec2() / vp.zoom - world.to_vec2();
                vp.pan_target = None;
            }
        }
        self.draw_canvas_overlays(&painter, canvas_rect);

        // v2 frames the canvas in a thin rounded outline.
        painter.rect_stroke(
            canvas_rect,
            10.0,
            egui::Stroke::new(1.0_f32, self.state.theme.canvas_frame_border),
            egui::StrokeKind::Inside,
        );

        if response.double_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            // Double-click a folder's label band or collapsed box: toggle it.
            let world_pos = self.state.viewport.screen_to_world(pos);
            if let Some(folder) = self.state.node_store.folder_at_position(world_pos) {
                crate::folder_actions::toggle_folder(&mut self.state, folder);
            }
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
                        let name = symbol.name_span;
                        self.state.selection.select_symbol(id, symbol, additive);
                        // The click also pins the symbol: the language server
                        // answers with its whole graph.
                        ui::symbol_pins::pin(Some(ui.ctx()), &mut self.state, id, name.line, name.start_byte, additive);
                    } else {
                        if additive {
                            self.state.selection.toggle(id);
                        } else {
                            self.state.selection.select(id);
                        }
                        // A click in a region of code (a body, an initializer,
                        // call arguments) maps to a token inside or around it.
                        if let Some((line, byte)) = self.mapped_token(id, world_pos) {
                            ui::symbol_pins::pin(Some(ui.ctx()), &mut self.state, id, line, byte, additive);
                        }
                    }
                } else if let Some(id) = self.folder_block_at(world_pos) {
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
        if dragging_node && response.drag_started_by(egui::PointerButton::Primary) {
            let picked = ui.input(|i| i.pointer.press_origin()).and_then(|pos| {
                let world = self.state.viewport.screen_to_world(pos);
                self.state.node_store.node_at_position(
                    world, self.state.settings.node_width, self.state.settings.font_size,
                ).or_else(|| self.folder_block_at(world))
            });
            match picked {
                Some(id) => self.state.selection.select(id),
                None => self.state.selection.clear(),
            }
        }
        if dragging_node {
            if let Some(id) = self.state.selection.primary {
                let delta = response.drag_delta() / self.state.viewport.zoom;
                self.state.node_store.translate_node(id, delta);
            }
        }
    }

    fn folder_block_at(&self, world: egui::Pos2) -> Option<u64> {
        let store = &self.state.node_store;
        let id = store.folder_block_at_position(world)?;
        let settings = &self.state.settings.folders;
        (store.view_collapsed(id)
            || (!settings.hide_folders && !(settings.merge_empty && is_pass_through(store, id))))
            .then_some(id)
    }

    /// Symbol Mapping for a click at `world` inside file node `node_id`: the
    /// `(line, byte column)` of the token the region maps to, if the click is
    /// in the code area.
    fn mapped_token(&self, node_id: u64, world: egui::Pos2) -> Option<(usize, usize)> {
        let node = self.state.node_store.get_file(node_id)?;
        let font_size = self.state.settings.font_size;
        let code_top = node.position.y + HEADER_HEIGHT + CODE_PADDING;
        if node.collapsed || world.y < code_top {
            return None;
        }
        let line = ((world.y - code_top) / line_height(font_size)) as usize;
        let text = node.content.lines().nth(line)?;
        let columns = ((world.x - node.position.x - CODE_PAD_X) / code_char_width(font_size)).max(0.0) as usize;
        let byte = text.char_indices().nth(columns).map_or(text.len(), |(i, _)| i);
        let token = crate::analysis::symbol_mapping::map_click(&node.path, &node.content, line, byte)?;
        Some((token.line, token.start))
    }

    /// Pick only the exact name returned by LSP, measured with the same token
    /// spans and font as painting. Whitespace and keywords select the file.
    fn symbol_at(&mut self, painter: &egui::Painter, node_id: u64, screen: egui::Pos2) -> Option<SelectedSymbol> {
        let node = self.state.node_store.get_file(node_id)?;
        let zoom = self.state.viewport.zoom;
        let lod = LodLevel::from_zoom(zoom);
        if node.collapsed || !lod.draws_code() {
            return None;
        }
        let font_size = self.state.settings.font_size;
        let world = self.state.viewport.screen_to_world(screen);
        let code_y = node.position.y + HEADER_HEIGHT + CODE_PADDING;
        if world.y < code_y { return None }
        let line = ((world.y - code_y) / line_height(font_size)).floor() as usize;
        if line >= node.visible_lines() { return None }
        let spans = self.highlighter.highlight_file(&node.path, &node.content).get(line)?;
        let code_x = self.state.viewport.world_to_screen(node.position).x + CODE_PAD_X * zoom;
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

    fn update_canvas_symbol_hover(&mut self, ui: &egui::Ui, response: &egui::Response, painter: &egui::Painter) {
        let picked = if self.state.settings.lsp.query_on_hover
            && response.hovered() && !ui.input(|i| i.pointer.any_down())
            && !self.state.space_held
        {
            ui.input(|i| i.pointer.hover_pos()).and_then(|screen| {
                let world = self.state.viewport.screen_to_world(screen);
                let id = self.state.node_store.node_at_position(
                    world, self.state.settings.node_width, self.state.settings.font_size,
                )?;
                let selected = self.symbol_at(painter, id, screen)?;
                let node = self.state.node_store.get_file(selected.node_id)?;
                let name = selected.name_span;
                let symbol = self.state.deps.symbol_occurrences(&node.path, name.line)
                    .into_iter().find(|s| s.definition_path == node.path
                        && s.definition_name_span == (name.line, name.start_byte, name.end_byte))?
                    .symbol.clone();
                Some((selected.node_id, symbol))
            })
        } else { None };
        ui::symbol_graph::update(ui.ctx(), &mut self.state, picked);
    }

    fn draw_folder_nodes(&self, painter: &egui::Painter) {
        let theme = &self.state.theme;
        let store = &self.state.node_store;
        let settings = &self.state.settings.folders;
        let hidden = store.hidden_folders();
        let zoom = self.state.viewport.zoom;
        let root = self.state.loaded_dir.clone().unwrap_or_default();
        let diff_color = egui::Color32::from_rgb(0xe2, 0xb3, 0x4a);
        let mut ids = store.all_folder_ids();
        // Outer folders first, so their labels claim space before nested ones.
        ids.sort_by_key(|&id| (store.folder_depth(id, &root), id));
        let mut labels: Vec<(egui::Rect, String, f32, egui::Color32)> = Vec::new();
        for id in ids {
            if hidden.contains(&id) {
                continue;
            }
            let Some(folder) = store.get_folder(id) else { continue };
            if !folder.bounds.is_positive() {
                continue;
            }
            let changed = settings.diff_highlight
                && crate::folder_actions::folder_has_changes(&self.state, id);
            // Zoom Fog: deeper folders fade as the view zooms out.
            let fog = if settings.zoom_fog {
                let depth = store.folder_depth(id, &root).max(1) as i32;
                (zoom / 0.5).clamp(0.1, 1.0).powi(depth).max(0.08)
            } else {
                1.0
            };
            if store.layout_collapsed(id) {
                if let Some(card) = folder.collapsed_rect() {
                    self.draw_folder_card(painter, folder, card, changed, diff_color, fog);
                }
                continue;
            }
            let screen_rect = self.state.viewport.world_to_screen_rect(folder.bounds);
            if !painter.clip_rect().intersects(screen_rect) {
                continue;
            }
            if store.view.zoom_collapsed.contains(&id) {
                self.draw_folder_card(painter, folder, folder.bounds, changed, diff_color, fog);
                continue;
            }
            // Merge Empty Folders: a folder holding only one folder and no
            // file draws nothing; its child's label carries the path.
            // Hide Folders: no frame and no name (collapsed cards still show).
            if settings.hide_folders || (settings.merge_empty && is_pass_through(store, id)) {
                continue;
            }
            // v2 folders: unfilled square outline, dim name just above it.
            let selected = self.state.selection.is_selected(id);
            let border = if selected { theme.selected_border } else if changed { diff_color } else { theme.folder_border };
            painter.rect_filled(screen_rect, 0.0, theme.folder_bg);
            painter.rect_stroke(
                screen_rect,
                0.0,
                egui::Stroke::new(if selected { 2.0_f32 } else { 1.0 }, border.gamma_multiply(if selected { 1.0 } else { fog })),
                egui::StrokeKind::Outside,
            );
            if !settings.show_folder_names {
                continue;
            }
            // Sized in world units so a nested folder's label never runs
            // into its parent's at overview zoom; hidden below Min Size.
            let label_px = (FOLDER_LABEL_WORLD_PX * zoom).min(16.0);
            if label_px < min_label_px(settings.min_size_pct).max(1.0) {
                continue;
            }
            let text = if settings.merge_empty {
                merged_label(store, id)
            } else {
                folder.label.clone()
            };
            let color = if changed { diff_color } else { theme.text_muted }.gamma_multiply(fog);
            let galley_width = painter
                .layout_no_wrap(text.clone(), egui::FontId::proportional(label_px), color)
                .size()
                .x;
            let anchor = screen_rect.left_top() - egui::vec2(0.0, 3.0);
            let rect = egui::Rect::from_min_size(
                anchor - egui::vec2(0.0, label_px * 1.2),
                egui::vec2(galley_width, label_px * 1.2),
            );
            // Merge Overlapping Labels: a label that would cover an earlier
            // one joins it instead.
            if settings.merge_overlapping_labels
                && let Some(existing) = labels.iter_mut().find(|l| l.0.intersects(rect))
            {
                existing.1.push_str(" / ");
                existing.1.push_str(&text);
                continue;
            }
            labels.push((rect, text, label_px, color));
        }
        for (rect, text, px, color) in labels {
            painter.text(
                rect.left_bottom(),
                egui::Align2::LEFT_BOTTOM,
                text,
                egui::FontId::proportional(px),
                color,
            );
        }
    }

    /// A collapsed folder: a filled card with its name and file count.
    fn draw_folder_card(
        &self,
        painter: &egui::Painter,
        folder: &crate::nodes::folder_node::FolderNode,
        world: egui::Rect,
        changed: bool,
        diff_color: egui::Color32,
        fog: f32,
    ) {
        let theme = &self.state.theme;
        let zoom = self.state.viewport.zoom;
        let screen_rect = self.state.viewport.world_to_screen_rect(world);
        if !painter.clip_rect().intersects(screen_rect) {
            return;
        }
        let rounding = (6.0 * zoom).clamp(1.0, 6.0);
        let selected = self.state.selection.is_selected(folder.id);
        let border = if selected { theme.selected_border } else if changed { diff_color } else { theme.folder_border.gamma_multiply(1.6) };
        painter.rect_filled(screen_rect, rounding, theme.node_bg.gamma_multiply(fog.max(0.5)));
        painter.rect_stroke(
            screen_rect,
            rounding,
            egui::Stroke::new(if selected { 2.0_f32 } else { 1.0 }, border.gamma_multiply(if selected { 1.0 } else { fog })),
            egui::StrokeKind::Outside,
        );
        let inside = self
            .state
            .node_store
            .file_nodes
            .values()
            .filter(|n| n.path.starts_with(&folder.path))
            .count();
        // Sized by the card, so the name stays readable in the overview: about
        // a third of its height, shrunk to fit its width, at most 16 px.
        let mut name_px = (screen_rect.height() * 0.34).min(16.0);
        let name_color = theme.text_primary.gamma_multiply(fog);
        let name_width = painter
            .layout_no_wrap(folder.label.clone(), egui::FontId::proportional(name_px), name_color)
            .size()
            .x;
        let room = screen_rect.width() - 24.0 * zoom.min(1.0);
        if name_width > room && room > 0.0 {
            name_px *= room / name_width;
        }
        if name_px < 5.0 {
            return;
        }
        let has_detail = name_px * 2.4 <= screen_rect.height();
        let text_x = screen_rect.left() + (12.0 * zoom).clamp(3.0, 12.0);
        let center_y = screen_rect.center().y;
        painter.text(
            egui::pos2(text_x, center_y - if has_detail { 1.0 } else { -name_px * 0.5 }),
            egui::Align2::LEFT_BOTTOM,
            &folder.label,
            egui::FontId::proportional(name_px),
            name_color,
        );
        if has_detail {
            painter.text(
                egui::pos2(text_x, center_y + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{inside} files · collapsed"),
                egui::FontId::proportional(name_px * 0.75),
                theme.text_muted.gamma_multiply(fog),
            );
        }
    }

    fn update_folder_bounds(&mut self) {
        self.state.node_store.update_folder_bounds(
            self.state.settings.node_width,
            self.state.settings.font_size,
            self.state.layout_settings.folder_padding,
        );
    }

    /// Ports of this frame, or none while they are off or nodes are blocks.
    fn collect_ports(&self) -> HashMap<u64, Vec<Port>> {
        let state = &self.state;
        if !state.edge_style.show_ports
            || !LodLevel::from_zoom(state.viewport.zoom).draws_code()
        {
            return HashMap::new();
        }
        let mut ports = ports::collect_ports(
            &state.edge_store,
            &state.node_store,
            &state.edge_style,
            &state.selection,
            state.settings.node_width,
            state.settings.font_size,
        );
        // Files hidden in a collapsed folder show no ports.
        ports.retain(|&id, _| !state.node_store.is_file_hidden(id));
        ports
    }

    /// Port circles on the node borders, over the edge ends they collect.
    fn draw_ports(
        &self,
        painter: &egui::Painter,
        canvas_rect: egui::Rect,
        ports: &HashMap<u64, Vec<Port>>,
    ) {
        let viewport = &self.state.viewport;
        let font_size = self.state.settings.font_size;
        let radius = (line_height(font_size) * viewport.zoom * 0.3).clamp(2.5, 6.0);
        let visible_world = viewport.visible_world_rect(canvas_rect);
        for (id, node_ports) in ports {
            let Some(node) = self.state.node_store.get_file(*id) else { continue };
            let world_rect = node.world_rect(self.state.settings.node_width, font_size);
            if !world_rect.intersects(visible_world) {
                continue;
            }
            for port in node_ports {
                let Some(y) = node.line_anchor_y(port.line, font_size) else { continue };
                let x = match port.side {
                    PortSide::Left => world_rect.left(),
                    PortSide::Right => world_rect.right(),
                };
                let center = viewport.world_to_screen(egui::pos2(x, y));
                painter.circle(
                    center,
                    radius,
                    port_color(&self.state.edge_style, port),
                    egui::Stroke::new(1.5_f32, self.state.theme.node_bg),
                );
            }
        }
    }

    fn draw_file_nodes(
        &mut self,
        painter: &egui::Painter,
        canvas_rect: egui::Rect,
        ports: &HashMap<u64, Vec<Port>>,
    ) -> Vec<(usize, egui::Rect)> {
        let mut graph_names = Vec::new();
        let mut pin_marks: Vec<ui::pin_overlay::PinMark> = Vec::new();
        let zoom = self.state.viewport.zoom;
        let lod = LodLevel::from_zoom(zoom);
        let theme = &self.state.theme;
        let node_width = self.state.settings.node_width;
        let font_size = self.state.settings.font_size;
        let visible_world = self.state.viewport.visible_world_rect(canvas_rect);
        let edge_spans = self.state.edge_store.focused_spans(&self.state.selection, &self.state.edge_style);

        let store = &self.state.node_store;
        let hidden_files: std::collections::HashSet<u64> =
            store.file_nodes.keys().copied().filter(|&id| store.is_file_hidden(id)).collect();
        for node in self.state.node_store.file_nodes.values() {
            let world_rect = node.world_rect(node_width, font_size);
            if !world_rect.intersects(visible_world) || hidden_files.contains(&node.id) {
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
            let label_size = (font_size * zoom).clamp(10.0, 14.0);
            let names = &self.state.settings.files;
            // L1-L3 show the name, L4 fades it out (0.04 → 0.01), L5 hides it.
            let label_alpha = LodLevel::header_alpha(zoom);
            if names.show_names && label_alpha > 0.0 {
                draw_fitted_label(
                    painter,
                    node.file_name(),
                    label_anchor,
                    egui::Align2::CENTER_BOTTOM,
                    screen_rect.width(),
                    label_size,
                    min_label_px(names.min_size_pct),
                    theme.text_secondary.gamma_multiply(label_alpha),
                );
            }

            if !lod.draws_code() {
                // Far away: solid block only — the floating label above is the
                // name. L5 fills the whole node with the semantic color:
                // selection or diff.
                let fill = if !lod.semantic_fill() {
                    theme.node_bg
                } else if selected {
                    semantic_selected()
                } else if self.state.changes.is_changed(&node.path) {
                    semantic_diff()
                } else {
                    theme.node_bg
                };
                painter.rect_filled(screen_rect, rounding, fill);
                painter.rect_stroke(
                    screen_rect,
                    rounding,
                    egui::Stroke::new(border_width, border_color),
                    egui::StrokeKind::Outside,
                );
                // The CST tint is the node's embedded minimap from L3 on.
                if self.state.settings.cst.show_canvas
                    && lod.cst_base_alpha() > 0.0
                    && !node.collapsed
                    && !node.content.is_empty()
                {
                    let origin = egui::pos2(
                        screen_rect.left() + CODE_PAD_X * zoom,
                        screen_rect.top() + (HEADER_HEIGHT + CODE_PADDING) * zoom,
                    );
                    let clip = painter.with_clip_rect(screen_rect.intersect(painter.clip_rect()));
                    let items = self.state.cst.tint_items(&self.state.node_store, node.id, &self.state.settings.cst);
                    let (line_h, char_w) = (line_height(font_size) * zoom, code_char_width(font_size * zoom));
                    paint_cst_tint(&clip, items, &self.state.settings.cst, lod, origin, line_h, char_w);
                }
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

            if node.collapsed {
                // File nodes › "Collapse long files": frame only, with a note.
                let note = format!("{} lines · collapsed", node.line_count);
                draw_fitted_label(
                    painter,
                    &note,
                    screen_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    screen_rect.width(),
                    (font_size * zoom).clamp(6.0, 13.0),
                    5.0,
                    theme.text_muted,
                );
                continue;
            }

            if node.content.is_empty() {
                continue;
            }

            let highlighted = self.highlighter.highlight_file(&node.path, &node.content);
            let lines = &highlighted[..highlighted.len().min(node.visible_lines())];
            let code_origin = egui::pos2(
                screen_rect.left() + CODE_PAD_X * zoom,
                header_rect.bottom() + CODE_PADDING * zoom,
            );
            let line_h = line_height(font_size) * zoom;
            let code_clip = painter.with_clip_rect(screen_rect.intersect(painter.clip_rect()));

            // Base tint of the CST by role, from L2 down (L1 only paints what
            // is hovered or selected).
            if self.state.settings.cst.show_canvas && lod.cst_base_alpha() > 0.0 {
                let char_w = code_char_width(font_size * zoom);
                let items = self.state.cst.tint_items(&self.state.node_store, node.id, &self.state.settings.cst);
                paint_cst_tint(&code_clip, items, &self.state.settings.cst, lod, code_origin, line_h, char_w);
            }

            // Under the code text: the statements joined by the edges of the
            // selected nodes, the lines of the CST or LSP row under the
            // pointer, and the line of the last reference jumped to.
            let lsp_target = self
                .state
                .lsp
                .target
                .filter(|t| self.state.selection.primary == Some(t.node_id));
            let hl_origin = egui::pos2(screen_rect.left(), code_origin.y);

            // Port rows: a band across each connected line, in the port's
            // import/symbol color, brighter when it carries the selection.
            for port in ports.get(&node.id).into_iter().flatten() {
                if let Some(rect) = cst_panel::highlight_rect(
                    hl_origin, screen_rect.width(), line_h,
                    port.line, port.line, node.visible_lines(),
                ) {
                    let color = port_color(&self.state.edge_style, port);
                    let (fill, edge) = if port.emphasized { (0.16, 0.40) } else { (0.08, 0.22) };
                    code_clip.rect_filled(rect, 0.0, color.gamma_multiply(fill));
                    code_clip.rect_stroke(
                        rect.shrink2(egui::vec2(0.0, 0.5)),
                        0.0,
                        egui::Stroke::new(1.0_f32, color.gamma_multiply(edge)),
                        egui::StrokeKind::Inside,
                    );
                }
            }

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

            // File nodes › "Highlight top-level symbols".
            if let Some(ranges) = self.state.top_level_highlights.get(&node.id) {
                for &(start, end) in ranges {
                    if let Some(rect) = cst_panel::highlight_rect(
                        hl_origin, screen_rect.width(), line_h,
                        start, end, node.visible_lines(),
                    ) {
                        code_clip.rect_filled(rect, 0.0, theme.accent.gamma_multiply(0.08));
                        let border = egui::Rect::from_min_size(
                            rect.left_top(),
                            egui::vec2(2.0 * zoom, rect.height()),
                        );
                        code_clip.rect_filled(border, 0.0, theme.accent.gamma_multiply(0.7));
                    }
                }
            }

            // CST panel: the nodes at the selection's edge ends, outlined,
            // and a hovered row's ancestors (Single Hover Symbol off).
            let show_cst = self.state.settings.cst.show_canvas;
            if show_cst {
                for &(hover, color) in &self.state.cst.mapping_hover {
                    if hover.node_id != node.id {
                        continue;
                    }
                    if let Some(rect) = cst_panel::highlight_rect(
                        hl_origin, screen_rect.width(), line_h,
                        hover.start_line, hover.end_line, node.visible_lines(),
                    ) {
                        code_clip.rect_filled(rect, 0.0, color.gamma_multiply(0.25));
                    }
                }
                for &(start, end) in self.state.cst.edge_nodes.get(&node.id).map(Vec::as_slice).unwrap_or(&[]) {
                    if let Some(rect) = cst_panel::highlight_rect(
                        hl_origin, screen_rect.width(), line_h,
                        start, end, node.visible_lines(),
                    ) {
                        code_clip.rect_stroke(
                            rect.shrink(1.0),
                            2.0,
                            egui::Stroke::new(1.0_f32, CST_EDGE_NODE_COLOR),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
                if self.state.cst.hovered.is_some_and(|h| h.node_id == node.id) {
                    for &(start, end) in &self.state.cst.hover_ancestors {
                        if let Some(rect) = cst_panel::highlight_rect(
                            hl_origin, screen_rect.width(), line_h,
                            start, end, node.visible_lines(),
                        ) {
                            code_clip.rect_filled(rect, 0.0, theme.selection.gamma_multiply(0.08));
                        }
                    }
                }
            }

            // CST/LSP hover highlights and reference-jump target.
            let pointer_lines = [
                self.state.cst.hovered.filter(|_| show_cst).map(|h| (h.node_id, h.start_line, h.end_line, 0.25)),
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

            if let Some(graph) = &self.state.lsp.canvas_graph.graph {
                let mut painted_blocks = Vec::new();
                for (index, site) in graph.sites.iter().enumerate().filter(|(_, s)| s.node_id == node.id) {
                    if !painted_blocks.contains(&site.span)
                        && let Some(rect) = ui::symbol_graph::block_rect(
                            site, code_origin, screen_rect.right() - code_origin.x,
                            line_h, node.visible_lines(),
                        )
                    {
                        ui::symbol_graph::paint_block(&code_clip, rect, index);
                        painted_blocks.push(site.span);
                    }
                    let name = site.name;
                    if let Some(spans) = lines.get(name.line)
                        && let Some((x0, x1)) = name_x_range(
                            &code_clip, lod, spans, name.start_byte, name.end_byte, font_size * zoom,
                        )
                    {
                        let rect = egui::Rect::from_min_max(
                            code_origin + egui::vec2(x0, name.line as f32 * line_h),
                            code_origin + egui::vec2(x1, (name.line + 1) as f32 * line_h),
                        );
                        let color = if index == 0 { ui::symbol_graph::DEFINITION_COLOR } else { ui::symbol_graph::NAME_COLOR };
                        code_clip.rect_filled(rect, 0.0, color.gamma_multiply(0.30));
                        graph_names.push((index, rect));
                    }
                }
            }

            // Pinned symbol graphs: blocks and name marks under the code.
            if !self.state.lsp.pins.is_empty() && lod.draws_code() {
                let area = ui::pin_overlay::CodeArea {
                    origin: code_origin,
                    right: screen_rect.right(),
                    line_h,
                    char_w: code_char_width(font_size * zoom),
                    visible_lines: node.visible_lines(),
                };
                ui::pin_overlay::paint_node(
                    &code_clip, self.state.lsp.pins.pins(), &node.path, &node.content, area, &mut pin_marks,
                );
            }

            match lod.render_mode() {
                RenderMode::Text => {
                    draw_line_numbers(
                        &code_clip,
                        lines.len(),
                        egui::pos2(screen_rect.left() + GUTTER_RIGHT * zoom, code_origin.y),
                        line_h,
                        font_size * zoom,
                        theme.line_number,
                    );
                    draw_code_text(&code_clip, lines, code_origin, line_h, font_size * zoom)
                }
                RenderMode::Minimap => {
                    draw_code_bars(&code_clip, lines, code_origin, line_h, font_size * zoom)
                }
                RenderMode::None => unreachable!(),
            }
        }
        self.pin_marks = pin_marks;
        graph_names
    }

    fn draw_edges(&mut self, painter: &egui::Painter) {
        // LSP "Dim Dependency Edges": while a symbol is active.
        let symbol_active =
            !self.state.selection.symbols().is_empty() || self.state.lsp.references.is_some()
                || self.state.lsp.canvas_graph.graph.is_some() || !self.state.lsp.pins.is_empty();
        // The dim stays 500 ms after the last symbol goes (v2 "restores with a delay").
        let now = self.repaint.input(|i| i.time);
        if symbol_active {
            self.state.lsp.dim_until = now + crate::ui::lsp_panel::DIM_RESTORE_DELAY;
        }
        let holding = !symbol_active && now < self.state.lsp.dim_until;
        if holding {
            self.repaint.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let lsp = &self.state.settings.lsp;
        let symbol_dim = (lsp.dim_edges && (symbol_active || holding)).then_some(lsp.dim_pct);
        let viewport = &self.state.viewport;
        let zoom = viewport.zoom;
        let lod = LodLevel::from_zoom(zoom);
        let font_size = self.state.settings.font_size;
        let node_width = self.state.settings.node_width;
        let highlighter = &mut self.highlighter;
        let mut symbol_rect = |node: &crate::nodes::file_node::FileNode, name: crate::edges::edge_store::NameSpan| {
            if node.collapsed || !lod.draws_code() || name.line >= node.visible_lines() {
                return None;
            }
            let spans = highlighter.highlight_file(&node.path, &node.content).get(name.line)?;
            let (x0, x1) = name_x_range(painter, lod, spans, name.start_byte, name.end_byte, font_size * zoom)?;
            let origin = viewport.world_to_screen(node.position);
            let line_h = line_height(font_size) * zoom;
            let line_top = origin.y + (HEADER_HEIGHT + CODE_PADDING) * zoom + name.line as f32 * line_h;
            let (top, height) = if lod.render_mode() == RenderMode::Text {
                let text = node.content.lines().nth(name.line)?.get(name.start_byte..name.end_byte)?;
                let height = painter.layout_no_wrap(text.to_string(), code_font(font_size * zoom), egui::Color32::PLACEHOLDER).size().y;
                (line_top, height)
            } else {
                let height = (line_h - 2.0).max(1.0);
                (line_top + (line_h - height) * 0.5, height)
            };
            let rect = egui::Rect::from_min_max(
                egui::pos2(origin.x + CODE_PAD_X * zoom + x0, top),
                egui::pos2(origin.x + CODE_PAD_X * zoom + x1, top + height),
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
            self.state.loaded_dir.as_deref(),
            &edge_renderer::CollapseRouting {
                proxies: self.state.node_store.collapse_proxies(),
                merge: self.state.settings.folders.merge_collapsed_edges,
                symbol_dim,
            },
        );
    }

    /// LSP "Out of View": a marker on the canvas border toward each file
    /// holding references that is outside the view.
    fn draw_out_of_view_references(&self, painter: &egui::Painter, canvas_rect: egui::Rect) {
        if !self.state.settings.lsp.out_of_view {
            return;
        }
        let inner = canvas_rect.shrink(14.0);
        for (center, count) in crate::ui::lsp_panel::out_of_view_markers(&self.state, canvas_rect) {
            let at = egui::pos2(center.x.clamp(inner.left(), inner.right()), center.y.clamp(inner.top(), inner.bottom()));
            painter.circle_filled(at, 9.0, self.state.theme.selection.gamma_multiply(0.85));
            painter.text(
                at,
                egui::Align2::CENTER_CENTER,
                count.to_string(),
                egui::FontId::proportional(10.0),
                egui::Color32::WHITE,
            );
        }
    }

    /// Out of View: a tag with the file name at the canvas border for every
    /// file holding a pinned entry that is off screen; clicking pans there.
    fn draw_out_of_view_tags(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, canvas_rect: egui::Rect) {
        if !self.state.settings.lsp.out_of_view {
            return;
        }
        let tags = ui::pin_overlay::out_of_view(&self.state, canvas_rect);
        for tag in tags {
            let galley = painter.layout_no_wrap(tag.name.clone(), egui::FontId::proportional(11.0), egui::Color32::WHITE);
            let rect = egui::Rect::from_center_size(tag.at, galley.size() + egui::vec2(14.0, 6.0));
            let rect = rect.translate(egui::vec2(
                (canvas_rect.left() + 4.0 - rect.left()).max(0.0) - (rect.right() - canvas_rect.right() + 4.0).max(0.0),
                0.0,
            ));
            let response = ui.interact(rect, egui::Id::new(("out_of_view", tag.node_id)), egui::Sense::click());
            let fill = if response.hovered() { self.state.theme.control_active } else { self.state.theme.control_bg };
            painter.rect_filled(rect, 6.0, fill);
            painter.rect_stroke(rect, 6.0, egui::Stroke::new(1.0_f32, self.state.theme.selection), egui::StrokeKind::Inside);
            painter.galley(rect.min + egui::vec2(7.0, 3.0), galley, egui::Color32::WHITE);
            if response.clicked() {
                let vp = &mut self.state.viewport;
                let target = canvas_rect.center().to_vec2() / vp.zoom - tag.world_center.to_vec2();
                if self.state.settings.lsp.smooth_transition {
                    vp.pan_target = Some(target);
                } else {
                    vp.offset = target;
                }
            }
        }
    }

    fn collect_node_rects(&self) -> Vec<egui::Rect> {
        input::shown_node_rects(&self.state)
    }
}

fn start_watcher(dir: &std::path::Path, ctx: &egui::Context) -> Option<crate::fs::watcher::FileWatcher> {
    let ctx = ctx.clone();
    match crate::fs::watcher::FileWatcher::new(dir, move || ctx.request_repaint()) {
        Ok(watcher) => Some(watcher),
        Err(e) => {
            log::warn!("Not watching {} for changes: {e}", dir.display());
            None
        }
    }
}

impl eframe::App for SpatialCodingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        input::handle_shortcuts(ctx, &mut self.state);
        self.run_app_actions();
        self.serve_agents();
        crate::workspace::poll_lsp(&mut self.state);
        self.follow_disk();
        if self.state.git_comparison.is_loading() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // A pinned symbol's graph is on its way: nothing else wakes the UI.
        if self.state.lsp.pins.needs_polling() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.state.diff.as_ref().is_some_and(|d| d.loader.is_loading()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // Background information: check back slowly, so an idle UI (and a
        // test's `run()`) is not kept busy while `git status` runs.
        if self.state.changes.is_loading() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        let now = ctx.input(|i| i.time);
        let changes_due = self
            .state
            .changes
            .requested_at
            .is_none_or(|at| now - at >= crate::git::changes::REFRESH_SECONDS);
        if changes_due && !self.state.changes.is_loading() {
            workspace::refresh_working_tree_changes(&mut self.state, now);
        }
        // The indexer's requests carry no context to wake the UI: check back
        // while answers are on their way.
        if self.state.deps.is_waiting() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else if std::mem::take(&mut self.layout_when_indexed) {
            self.apply_layout();
            self.state.pending_fit = true;
        } else if self.state.layout_dirty && self.state.layout_settings.auto_layout {
            // Auto Layout: files were opened or removed and their edges are in.
            workspace::apply_layout_keeping_selection(&mut self.state);
        }

        if self.state.loaded_dir.is_some() && self.file_tree.is_none() {
            if let Some(ref dir) = self.state.loaded_dir {
                match self.load_tree(dir) {
                    Ok(tree) => self.file_tree = Some(tree),
                    Err(e) => log::error!("Failed to load directory: {}", e),
                }
            }
        }

        egui::TopBottomPanel::top("toolbar")
            .exact_height(ui::toolbar::TOOLBAR_HEIGHT)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(self.state.theme.chrome_bg))
            .show(ctx, |ui| {
                ui::toolbar::draw_toolbar(ui, &mut self.state);
            });

        if !self.state.sidebars_hidden {
            ui::sidebar_left::draw_sidebar_left(ctx, &mut self.state, &mut self.file_tree);
            ui::sidebar_right::draw_sidebar_right(ctx, &mut self.state);
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(self.state.theme.chrome_bg)
                    .inner_margin(egui::Margin { left: 0, right: 0, top: 0, bottom: 4 }),
            )
            .show(ctx, |ui| {
                self.draw_canvas(ui);
            });

        ui::settings_dialog::draw_settings(ctx, &mut self.state);
        ui::settings_dialog::draw_ignore_settings(ctx, &mut self.state);
        ui::settings_dialog::draw_command_proposals(ctx, &mut self.state);
        self.run_app_actions();
        ui::shortcuts_dialog::draw_shortcuts(ctx, &mut self.state.show_shortcuts);
        ui::docs_dialog::draw(ctx, &mut self.state);
        ui::zoom_levels::draw(ctx, &mut self.state);
        ui::toast::draw(ctx, &mut self.state);
        crate::commands::draw_ui_highlight(ctx, &mut self.state);
        ui::diff_view::draw_diff_view(ctx, &mut self.state);
        if let Some(command) = ui::command_palette::draw_command_palette(ctx, &mut self.state) {
            self.execute_command(command);
        }

        if self.state.loaded_dir.is_none() {
            self.draw_welcome_screen(ctx);
        }

        let now = ctx.input(|i| i.time);
        self.save_settings_if_changed(now);
    }
}

impl SpatialCodingApp {
    fn draw_welcome_screen(&mut self, ctx: &egui::Context) {
        let theme = self.state.theme.clone();
        let recents = self.state.settings.recent_folders.clone();
        let example = example_workspace();
        let mut actions: Vec<crate::commands::AppAction> = Vec::new();
        let mut command: Option<&'static str> = None;
        egui::Area::new(egui::Id::new("welcome"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let card = |ui: &mut egui::Ui, title: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
                    egui::Frame::NONE
                        .fill(theme.panel_bg)
                        .stroke(egui::Stroke::new(1.0_f32, theme.panel_border))
                        .corner_radius(10.0)
                        .inner_margin(14.0)
                        .show(ui, |ui| {
                            ui.set_width(270.0);
                            ui.set_min_height(150.0);
                            ui.label(egui::RichText::new(title).strong().size(15.0));
                            ui.add_space(6.0);
                            add(ui);
                        });
                };
                ui.vertical_centered(|ui| {
                    ui.heading("SpatialCoding");
                    ui.label(egui::RichText::new("Spatial code visualization").color(theme.text_muted));
                    ui.add_space(12.0);
                });
                ui.horizontal_top(|ui| {
                    card(ui, "Local Server", &mut |ui| {
                        ui.label("Language servers run on this machine, one per workspace, started as child processes: rust-analyzer, typescript-language-server, pyright and gopls.");
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("Folders, diffs and file watching work without a server.").small().color(theme.text_muted));
                    });
                    card(ui, "Repository", &mut |ui| {
                        if ui.button("Open folder...").clicked() {
                            actions.push(crate::commands::AppAction::PickFolder);
                        }
                        if !recents.is_empty() {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("Recent").small().color(theme.text_muted));
                            for dir in recents.iter().take(4) {
                                let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string());
                                if ui.link(name).on_hover_text(dir.display().to_string()).clicked() {
                                    actions.push(crate::commands::AppAction::OpenFolder(dir.clone()));
                                }
                            }
                        }
                        if let Some(dir) = &example {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("Example").small().color(theme.text_muted));
                            if ui.link("showcase workspace").on_hover_text(dir.display().to_string()).clicked() {
                                actions.push(crate::commands::AppAction::OpenFolder(dir.clone()));
                            }
                        }
                    });
                });
                ui.add_space(10.0);
                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Docs").clicked() {
                            command = Some("ui:toggleDocsWindow");
                        }
                        if ui.button("Shortcuts").clicked() {
                            command = Some("ui:openShortcutsPanel");
                        }
                        if ui.button("Command Palette").clicked() {
                            command = Some("ui:toggleCommandPalette");
                        }
                    });
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Ctrl+/ for shortcuts").small().color(theme.text_muted));
                });
            });
        self.state.pending_app_actions.extend(actions);
        if let Some(id) = command {
            let _ = crate::commands::run(&mut self.state, id, &serde_json::Value::Null);
        }
        self.run_app_actions();
    }
}

/// The checked-in showcase workspace, when the app runs from the repository
/// (or a directory below it).
fn example_workspace() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    cwd.ancestors()
        .map(|dir| dir.join("examples").join("showcase"))
        .find(|dir| dir.is_dir())
}

type HighlightedLine = Vec<(syntect::highlighting::Style, String)>;

/// Approximate advance of one monospace glyph, as a fraction of font size.
const MONO_ADVANCE: f32 = 0.6;

/// Draws `text` with a font no larger than `max_px`, shrunk to fit `max_width`.
/// Skips drawing when it would be too small to read.
/// Folder label height in world units: fits in the parent folder's padding.
const FOLDER_LABEL_WORLD_PX: f32 = 36.0;

/// Outline of the CST node at a selected edge's end (CST panel).
const CST_EDGE_NODE_COLOR: egui::Color32 = egui::Color32::from_rgb(0x8b, 0xd5, 0xa6);

/// A folder that only holds one folder and no file of its own: with Merge
/// Empty Folders it draws nothing and its child's label says both names.
fn is_pass_through(store: &crate::nodes::node_store::NodeStore, id: u64) -> bool {
    let Some(folder) = store.get_folder(id) else { return false };
    if folder.file_count > 0 {
        return false;
    }
    let children = store.folder_nodes.values().filter(|f| store.folder_parent(f.id) == Some(id)).count();
    children == 1
}

/// A folder's label with the pass-through folders above it: `a/b/c`.
fn merged_label(store: &crate::nodes::node_store::NodeStore, id: u64) -> String {
    let mut parts = vec![store.get_folder(id).map(|f| f.label.clone()).unwrap_or_default()];
    let mut parent = store.folder_parent(id);
    while let Some(p) = parent {
        if !is_pass_through(store, p) {
            break;
        }
        if let Some(f) = store.get_folder(p) {
            parts.push(f.label.clone());
        }
        parent = store.folder_parent(p);
    }
    parts.reverse();
    parts.join("/")
}

/// Smallest font size, in screen pixels, at which a name label is drawn:
/// the File nodes / Folders "Min Size" share of 10 px (50% keeps 5 px).
pub(crate) fn min_label_px(min_size_pct: f32) -> f32 {
    min_size_pct.clamp(0.0, 1.0) * 10.0
}

#[allow(clippy::too_many_arguments)]
fn draw_fitted_label(
    painter: &egui::Painter,
    text: &str,
    pos: egui::Pos2,
    align: egui::Align2,
    max_width: f32,
    max_px: f32,
    min_px: f32,
    color: egui::Color32,
) {
    let chars = text.chars().count().max(1) as f32;
    let px = max_px.min((max_width - 4.0) / (chars * MONO_ADVANCE));
    if px < min_px.max(1.0) {
        return;
    }
    // Proportional, as v2's file labels; the mono advance over-estimates
    // its width, so the fit stays conservative.
    painter.text(pos, align, text, egui::FontId::proportional(px), color);
}

/// Full LOD: the line numbers, right-aligned at `right_top.x`.
fn draw_line_numbers(
    painter: &egui::Painter,
    count: usize,
    right_top: egui::Pos2,
    line_h: f32,
    font_px: f32,
    color: egui::Color32,
) {
    let font = egui::FontId::monospace(font_px * 0.9);
    let rows = visible_code_rows(right_top.y, line_h, count, painter.clip_rect());
    for i in rows {
        painter.text(
            egui::pos2(right_top.x, right_top.y + i as f32 * line_h),
            egui::Align2::RIGHT_TOP,
            (i + 1).to_string(),
            font.clone(),
            color,
        );
    }
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

/// Paints the CST of one file as tinted rectangles: fill at the level's base
/// alpha times the type's fill alpha, and the type's border. Nodes the level
/// culls (`depth >= {2,2,2,3,3}` and fewer than `{1,1,1,100,100}` lines) and
/// hidden types are skipped.
#[allow(clippy::too_many_arguments)]
fn paint_cst_tint(
    painter: &egui::Painter,
    items: &[crate::ui::cst_panel::TintItem],
    settings: &crate::state::CstSettings,
    lod: LodLevel,
    origin: egui::Pos2,
    line_h: f32,
    char_w: f32,
) {
    let clip = painter.clip_rect();
    let mut styles: HashMap<&str, crate::analysis::cst_style::CstTypeStyle> = HashMap::new();
    for item in items {
        let lines = item.end_line + 1 - item.start_line;
        if lod.culls_cst(item.depth, lines) {
            continue;
        }
        let rect = egui::Rect::from_min_max(
            origin + egui::vec2(item.x0 as f32 * char_w, item.start_line as f32 * line_h),
            origin + egui::vec2(item.x1 as f32 * char_w, (item.end_line + 1) as f32 * line_h),
        );
        if !rect.intersects(clip) || rect.width() < 0.5 || rect.height() < 0.5 {
            continue;
        }
        let style = styles.entry(item.kind.as_str()).or_insert_with(|| settings.style_for(&item.kind));
        if !style.visible {
            continue;
        }
        let [r, g, b] = style.color;
        let alpha = (lod.cst_base_alpha() * style.fill_alpha).clamp(0.0, 1.0);
        let to_u8 = |a: f32| (a * 255.0).round() as u8;
        match style.fill {
            crate::analysis::cst_style::FillMode::Solid => {
                painter.rect_filled(rect, 0.0, egui::Color32::from_rgba_unmultiplied(r, g, b, to_u8(alpha)));
            }
            crate::analysis::cst_style::FillMode::Gradient => {
                let [r2, g2, b2] = style.color_end;
                let left = egui::Color32::from_rgba_unmultiplied(r, g, b, to_u8(alpha));
                let right = egui::Color32::from_rgba_unmultiplied(r2, g2, b2, to_u8(alpha));
                let mut mesh = egui::Mesh::default();
                let i = mesh.vertices.len() as u32;
                for (pos, color) in [
                    (rect.left_top(), left),
                    (rect.right_top(), right),
                    (rect.right_bottom(), right),
                    (rect.left_bottom(), left),
                ] {
                    mesh.vertices.push(egui::epaint::Vertex { pos, uv: egui::epaint::WHITE_UV, color });
                }
                mesh.indices.extend([i, i + 1, i + 2, i, i + 2, i + 3]);
                painter.add(egui::Shape::mesh(mesh));
            }
        }
        let stroke_alpha = to_u8(style.stroke_alpha(alpha));
        if stroke_alpha > 0 && style.border_width > 0.0 {
            let [br, bg, bb] = style.border_color;
            let stroke = egui::Stroke::new(
                style.border_width.max(0.5),
                egui::Color32::from_rgba_unmultiplied(br, bg, bb, stroke_alpha),
            );
            let s = style.sides;
            if s.top { painter.line_segment([rect.left_top(), rect.right_top()], stroke); }
            if s.right { painter.line_segment([rect.right_top(), rect.right_bottom()], stroke); }
            if s.bottom { painter.line_segment([rect.left_bottom(), rect.right_bottom()], stroke); }
            if s.left { painter.line_segment([rect.left_top(), rect.left_bottom()], stroke); }
        }
    }
}

/// Alpha of the minimap rectangles (`globalAlpha .4` in the reference).
const MINIMAP_ALPHA: f32 = 0.4;
/// L5 fill of a selected node (`#AED6FF`, the selected edge color).
fn semantic_selected() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(0xAE, 0xD6, 0xFF, 90)
}
/// L5 fill of a node with working-tree changes (added green `#4CD964`).
fn semantic_diff() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(0x4C, 0xD9, 0x64, 80)
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
    let bar_h = (line_h - 2.0).max(1.0);
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
            let color = Highlighter::style_to_egui_color(style)
                .gamma_multiply(MINIMAP_ALPHA);
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
    egui::FontId::monospace(font_px)
}

/// Approximate column width of the code font, as the minified bars use it.
fn code_char_width(font_px: f32) -> f32 {
    font_px * MONO_ADVANCE
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
        match lod.render_mode() {
            RenderMode::Text => painter
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

/// Fill of a port and its row band: the import or symbol color, brighter
/// when the port carries the selection.
fn port_color(style: &crate::edges::style::EdgeStyleConfig, port: &Port) -> egui::Color32 {
    let rgb = match port.kind {
        PortKind::Import => style.import_port_color,
        PortKind::Symbol => style.symbol_port_color,
    };
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0) as u8;
    let color = egui::Color32::from_rgb(byte(rgb[0]), byte(rgb[1]), byte(rgb[2]));
    if port.emphasized {
        color.lerp_to_gamma(egui::Color32::WHITE, 0.35)
    } else {
        color
    }
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
            let (x0, x1) = name_x_range(&painter, LodLevel::L1, &line, start, end, font_px).unwrap();
            let galley = painter.layout_no_wrap(text.trim_end().to_string(), code_font(font_px), egui::Color32::WHITE);
            // Whole-line glyphs snap to pixels while the drawn spans are
            // summed, so allow 1 px; the old 0.6-advance guess was ~40 px off.
            let glyph_x = |i| galley.pos_from_ccursor(egui::text::CCursor::new(i)).min.x;
            assert!((x0 - glyph_x(start)).abs() <= 1.0, "start {x0} vs glyph {}", glyph_x(start));
            assert!((x1 - glyph_x(end)).abs() <= 1.0, "end {x1} vs glyph {}", glyph_x(end));
        });
    }
}
