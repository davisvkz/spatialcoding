//! Search panel of the left sidebar: **Code** (the text of every file) and
//! **Files & folders** (fuzzy path match) of the loaded folder, with the
//! `.*` regex, `Aa` case, `Word`, `Ignored` and `Canvas` filters
//! (`codecanvas/ui-e-paineis.md`). Results are grouped On Canvas and Off
//! Canvas; the arrows move through them and Enter opens the active one;
//! Tab switches the mode. Code search reads the folder, so it runs on a
//! worker thread after the typing pauses.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use egui::{Align2, FontId};

use crate::search::content::{search_content_with, ContentHit, Options};
use crate::search::files::{search_paths, PathHit};
use crate::state::AppState;
use crate::ui::panel::segmented;
use crate::ui::theme::Theme;

/// Seconds the query must rest before a folder-wide search starts.
const DEBOUNCE_SECONDS: f64 = 0.25;
const ROW_HEIGHT: f32 = 36.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    /// Text inside files.
    #[default]
    Code,
    /// File and folder names.
    Files,
}

/// What a finished or running search was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    mode: SearchMode,
    query: String,
    options: Options,
    ignored: bool,
    canvas: bool,
}

struct Job {
    key: Key,
    receiver: Receiver<Result<(Vec<ContentHit>, bool), String>>,
}

/// Query, filters and the latest answers of the Search panel.
#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub mode: SearchMode,
    pub options: Options,
    /// `Ignored`: also search ignored files.
    pub include_ignored: bool,
    /// `Canvas`: only the files open on the canvas.
    pub canvas_only: bool,
    content: Vec<ContentHit>,
    content_cut: bool,
    /// Why the last code search failed (an invalid regex).
    pub error: Option<String>,
    /// What the shown answers (or the running job) were computed for.
    answered: Option<Key>,
    job: Option<Job>,
    /// App time of the last edit of the query or the filters.
    edited_at: f64,
    /// Index of the active result, for the arrows and Enter.
    pub active: usize,
}

impl SearchState {
    pub fn is_searching(&self) -> bool {
        self.job.is_some()
    }

    pub fn content_hits(&self) -> &[ContentHit] {
        &self.content
    }

    fn key(&self) -> Key {
        Key {
            mode: self.mode,
            query: self.query.trim().to_owned(),
            options: self.options,
            ignored: self.include_ignored,
            canvas: self.canvas_only,
        }
    }

    /// Installs a finished answer; true when something arrived.
    pub fn poll(&mut self) -> bool {
        let Some(job) = &self.job else { return false };
        let answer = match job.receiver.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                self.job = None;
                return true;
            }
        };
        let finished = self.job.take().expect("job was just polled");
        // Drop an answer the user has already typed past.
        if finished.key == self.key() {
            match answer {
                Ok((hits, cut)) => {
                    self.content = hits;
                    self.content_cut = cut;
                    self.error = None;
                }
                Err(message) => {
                    self.content.clear();
                    self.error = Some(message);
                }
            }
            self.answered = Some(finished.key);
        }
        true
    }

    /// Starts the worker for the current query once it has rested.
    fn start_if_due(&mut self, now: f64, files: &[PathBuf]) {
        let wanted = self.key();
        if self.mode == SearchMode::Files || self.answered.as_ref() == Some(&wanted) {
            return;
        }
        if wanted.query.is_empty() {
            self.content.clear();
            self.error = None;
            self.answered = Some(wanted);
            return;
        }
        if self.job.as_ref().is_some_and(|j| j.key == wanted) || now - self.edited_at < DEBOUNCE_SECONDS {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let files: Vec<PathBuf> = files.to_vec();
        let (query, options) = (wanted.query.clone(), wanted.options);
        std::thread::spawn(move || {
            let _ = sender.send(search_content_with(&files, &query, options));
        });
        self.job = Some(Job { key: wanted, receiver });
    }
}

/// One result row.
enum Row {
    Path(PathHit),
    Content(ContentHit),
}

impl Row {
    fn path(&self) -> &Path {
        match self {
            Row::Path(h) => &h.path,
            Row::Content(h) => &h.path,
        }
    }

    /// What clicking or Enter does: the file (or folder) and the line.
    fn target(&self) -> (PathBuf, usize, bool) {
        match self {
            Row::Path(h) => (h.path.clone(), 0, h.is_dir),
            Row::Content(h) => (h.path.clone(), h.line, false),
        }
    }
}

/// Splits rows into those whose file is open on the canvas and the rest,
/// keeping their order; this is also the order of the arrows.
fn group_rows(rows: Vec<Row>, open: &std::collections::HashSet<PathBuf>) -> (Vec<Row>, Vec<Row>) {
    rows.into_iter().partition(|r| open.contains(r.path()))
}

/// The files a search covers: the folder's, or only the open ones with
/// `Canvas`; ignored files join with `Ignored`.
fn searched_files(state: &AppState, search: &SearchState) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = if search.include_ignored {
        crate::workspace::folder_text_files_unfiltered(state)
    } else {
        crate::workspace::folder_text_files(state).as_ref().clone()
    };
    if search.canvas_only {
        let open: std::collections::HashSet<&PathBuf> = state.node_store.file_nodes.values().map(|n| &n.path).collect();
        files.retain(|f| open.contains(f));
    }
    files
}

/// A toggle button of the filter row.
fn filter_button(ui: &mut egui::Ui, on: &mut bool, label: &str, tip: &str) -> bool {
    let response = ui.selectable_label(*on, label).on_hover_text(tip);
    if response.clicked() {
        *on = !*on;
    }
    response.clicked()
}

/// Draws the Search panel body.
pub fn draw_search_panel(ui: &mut egui::Ui, state: &mut AppState) {
    // Taken out so the rows can borrow the rest of the state.
    let mut search = std::mem::take(&mut state.search);
    let now = ui.input(|i| i.time);
    let theme = state.theme.clone();
    let root = state.loaded_dir.clone();

    let before = search.key();
    let input_id = egui::Id::new("search_panel_input");
    // Tab switches the mode while the field has the focus.
    if ui.memory(|m| m.has_focus(input_id)) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
        search.mode = if search.mode == SearchMode::Code { SearchMode::Files } else { SearchMode::Code };
    }
    let mut mode = search.mode;
    segmented(ui, &mut mode, &[(SearchMode::Code, "Code"), (SearchMode::Files, "Files & folders")]);
    search.mode = mode;
    ui.add_space(6.0);
    let hint = match search.mode {
        SearchMode::Code => "Search inside files",
        SearchMode::Files => "Search file and folder names",
    };
    let field_width = ui.available_width();
    let edit = ui.add(
        egui::TextEdit::singleline(&mut search.query)
            .hint_text(hint)
            .desired_width(field_width)
            .lock_focus(true) // Tab switches the mode, it does not move the focus
            .id(input_id),
    );
    // Enter ends the edit (the field loses the focus); it opens the active result.
    let enter_pressed = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let code = search.mode == SearchMode::Code;
        ui.add_enabled_ui(code, |ui| {
            filter_button(ui, &mut search.options.regex, ".*", "Regular expression");
            filter_button(ui, &mut search.options.case_sensitive, "Aa", "Match case");
            filter_button(ui, &mut search.options.whole_word, "Word", "Whole words only");
        });
        filter_button(ui, &mut search.include_ignored, "Ignored", "Also search ignored files");
        filter_button(ui, &mut search.canvas_only, "Canvas", "Only the files open on the canvas");
    });
    ui.horizontal(|ui| {
        let lsp = &mut state.settings.lsp;
        ui.checkbox(&mut lsp.keep_zoom, "Keep Zoom").on_hover_text("Do not change the zoom when jumping to a result");
        ui.checkbox(&mut lsp.smooth_transition, "Smooth Transition").on_hover_text("Glide to the result");
    });
    if search.key() != before {
        search.edited_at = now;
        search.active = 0;
    }
    ui.add_space(4.0);

    let mut clicked: Option<(PathBuf, usize, bool)> = None;
    match root {
        None => {
            ui.weak("Open a folder to search");
        }
        Some(root) => {
            let files = searched_files(state, &search);
            search.start_if_due(now, &files);
            let pending = search.mode == SearchMode::Code && search.answered.as_ref() != Some(&search.key());
            if search.is_searching() || pending {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            }
            let query = search.query.trim().to_owned();
            let open: std::collections::HashSet<PathBuf> = state.node_store.file_nodes.values().map(|n| n.path.clone()).collect();
            let rows: Option<Vec<Row>> = match search.mode {
                SearchMode::Files if query.is_empty() => {
                    ui.weak("Type to find a file or folder by name");
                    None
                }
                SearchMode::Files => Some(search_paths(&root, &files, &query).into_iter().map(Row::Path).collect()),
                SearchMode::Code if query.is_empty() => {
                    ui.weak("Type to search the text of every file");
                    None
                }
                SearchMode::Code if pending => {
                    ui.weak("Searching…");
                    None
                }
                SearchMode::Code => {
                    if let Some(error) = &search.error {
                        ui.colored_label(egui::Color32::from_rgb(0xef, 0x44, 0x44), format!("Invalid pattern: {error}"));
                    }
                    Some(search.content.iter().cloned().map(Row::Content).collect())
                }
            };
            if let Some(rows) = rows {
                let noun = if search.mode == SearchMode::Code { "match" } else { "result" };
                summary(ui, &theme, rows.len(), search.content_cut && search.mode == SearchMode::Code, noun);
                let (on, off) = group_rows(rows, &open);
                let total = on.len() + off.len();
                // Arrows and Enter, while the search field has the focus.
                if (edit.has_focus() || enter_pressed) && total > 0 {
                    let (down, up) = ui.input_mut(|i| {
                        (
                            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                        )
                    });
                    let enter = enter_pressed;
                    if enter {
                        edit.request_focus();
                    }
                    if down {
                        search.active = (search.active + 1).min(total - 1);
                    }
                    if up {
                        search.active = search.active.saturating_sub(1);
                    }
                    if enter {
                        let row = on.iter().chain(off.iter()).nth(search.active);
                        clicked = row.map(Row::target);
                    }
                }
                search.active = search.active.min(total.saturating_sub(1));
                let mut index = 0;
                for (title, group) in [("On Canvas", &on), ("Off Canvas", &off)] {
                    if group.is_empty() {
                        continue;
                    }
                    ui.label(egui::RichText::new(format!("{title} ({})", group.len())).strong().size(11.0));
                    for row in group {
                        let active = index == search.active;
                        let response = match row {
                            Row::Path(hit) => path_row(ui, &theme, hit, active),
                            Row::Content(hit) => content_row(ui, &theme, &root, hit, active),
                        };
                        if active && (ui.input(|i| i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowUp))) {
                            response.scroll_to_me(None);
                        }
                        if response.clicked() {
                            search.active = index;
                            clicked = Some(row.target());
                        }
                        index += 1;
                    }
                }
            }
        }
    }
    search.poll();
    state.search = search;
    if let Some((path, line, is_dir)) = clicked {
        if is_dir {
            crate::workspace::open_folder_on_canvas(state, &path);
        } else {
            crate::workspace::focus_location(state, &path, line);
        }
    }
}

fn summary(ui: &mut egui::Ui, theme: &Theme, count: usize, cut: bool, noun: &str) {
    let plural = if count == 1 { "" } else { "s" };
    let more = if cut { "+" } else { "" };
    let text = if count == 0 { format!("No {noun}s found") } else { format!("{count}{more} {noun}{plural}") };
    ui.label(egui::RichText::new(text).size(10.5).color(theme.text_muted));
    ui.add_space(2.0);
}

fn allocate_row(ui: &mut egui::Ui, theme: &Theme, label: String, active: bool) -> (egui::Rect, egui::Response) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), egui::Sense::click());
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    if active {
        ui.painter().rect_filled(rect, 4.0, theme.control_active);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, theme.control_bg);
    }
    (rect, response)
}

/// Draws `text` clipped to `rect`, coloring the characters in `marked`.
fn marked_text(
    ui: &egui::Ui,
    theme: &Theme,
    pos: egui::Pos2,
    clip_right: f32,
    text: &str,
    marked: &dyn Fn(usize) -> bool,
    size: f32,
) {
    let font = FontId::proportional(size);
    let mut job = egui::text::LayoutJob::default();
    for (i, ch) in text.chars().enumerate() {
        let color = if marked(i) { theme.accent } else { theme.text_primary };
        job.append(&ch.to_string(), 0.0, egui::TextFormat::simple(font.clone(), color));
    }
    let galley = ui.painter().layout_job(job);
    let painter = ui.painter().with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(pos.x, pos.y - 12.0),
        egui::pos2(clip_right, pos.y + 12.0),
    ));
    painter.galley(egui::pos2(pos.x, pos.y - galley.size().y / 2.0), galley, theme.text_primary);
}

fn muted(ui: &egui::Ui, theme: &Theme, pos: egui::Pos2, clip_right: f32, text: &str) {
    let painter = ui.painter().with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(pos.x, pos.y - 10.0),
        egui::pos2(clip_right, pos.y + 10.0),
    ));
    painter.text(pos, Align2::LEFT_CENTER, text, FontId::proportional(10.0), theme.text_muted);
}

fn path_row(ui: &mut egui::Ui, theme: &Theme, hit: &PathHit, active: bool) -> egui::Response {
    let shown = if hit.is_dir { format!("{}/", hit.relative) } else { hit.relative.clone() };
    let (rect, response) = allocate_row(ui, theme, shown.clone(), active);
    let right = rect.right() - 8.0;
    let trimmed = shown.trim_end_matches('/');
    let name_start = trimmed.rfind('/').map_or(0, |i| trimmed[..=i].chars().count());
    let name: String = shown.chars().skip(name_start).collect();
    let dir: String = shown.chars().take(name_start).collect();
    marked_text(
        ui,
        theme,
        egui::pos2(rect.left() + 8.0, rect.top() + 12.0),
        right,
        &name,
        &|i| hit.matched.indices.contains(&(i + name_start)),
        12.0,
    );
    muted(ui, theme, egui::pos2(rect.left() + 8.0, rect.top() + 26.0), right, if dir.is_empty() { "." } else { &dir });
    response
}

fn content_row(ui: &mut egui::Ui, theme: &Theme, root: &Path, hit: &ContentHit, active: bool) -> egui::Response {
    let relative = hit.path.strip_prefix(root).unwrap_or(&hit.path).to_string_lossy().into_owned();
    let (rect, response) = allocate_row(ui, theme, format!("{relative}:{} {}", hit.line + 1, hit.text), active);
    let right = rect.right() - 8.0;
    muted(ui, theme, egui::pos2(rect.left() + 8.0, rect.top() + 11.0), right, &format!("{relative}:{}", hit.line + 1));
    let (from, to) = hit.range;
    marked_text(
        ui,
        theme,
        egui::pos2(rect.left() + 8.0, rect.top() + 25.0),
        right,
        &hit.text,
        &|i| (from..to).contains(&i),
        11.5,
    );
    response
}
