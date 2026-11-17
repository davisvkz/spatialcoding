//! Search panel of the left sidebar: file names (fuzzy), file contents and
//! top-level symbols of the loaded folder. Files are matched on the UI thread;
//! contents and symbols read the folder, so they run on a worker thread after
//! the typing pauses. Clicking a result shows it on the canvas.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use egui::{Align2, FontId};

use crate::search::content::{search_content, ContentHit};
use crate::search::files::{search_files, FileHit};
use crate::search::fuzzy::FuzzyMatch;
use crate::search::symbols::{search_symbols, SymbolHit};
use crate::state::AppState;
use crate::ui::panel::segmented;
use crate::ui::theme::Theme;

/// Seconds the query must rest before a folder-wide search starts.
const DEBOUNCE_SECONDS: f64 = 0.25;
const ROW_HEIGHT: f32 = 36.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    #[default]
    Files,
    Content,
    Symbols,
}

enum Answer {
    Content(Vec<ContentHit>, bool),
    Symbols(Vec<SymbolHit>),
}

struct Job {
    mode: SearchMode,
    query: String,
    receiver: Receiver<Answer>,
}

/// Query, mode and the latest answers of the Search panel.
#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub mode: SearchMode,
    content: Vec<ContentHit>,
    content_cut: bool,
    symbols: Vec<SymbolHit>,
    /// What the shown answers (or the running job) were computed for.
    answered: Option<(SearchMode, String)>,
    job: Option<Job>,
    /// App time of the last edit of the query or mode, for the debounce.
    edited_at: f64,
}

impl SearchState {
    pub fn is_searching(&self) -> bool {
        self.job.is_some()
    }

    pub fn content_hits(&self) -> &[ContentHit] {
        &self.content
    }

    pub fn symbol_hits(&self) -> &[SymbolHit] {
        &self.symbols
    }

    fn wants(&self) -> (SearchMode, String) {
        (self.mode, self.query.trim().to_owned())
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
        if finished.mode == self.mode && finished.query == self.query.trim() {
            match answer {
                Answer::Content(hits, cut) => {
                    self.content = hits;
                    self.content_cut = cut;
                }
                Answer::Symbols(hits) => self.symbols = hits,
            }
            self.answered = Some((finished.mode, finished.query));
        }
        true
    }

    /// Starts the worker for the current query once it has rested.
    fn start_if_due(&mut self, now: f64, files: &std::rc::Rc<Vec<PathBuf>>) {
        let wanted = self.wants();
        if self.mode == SearchMode::Files || self.answered.as_ref() == Some(&wanted) {
            return;
        }
        if wanted.1.is_empty() {
            self.content.clear();
            self.symbols.clear();
            self.answered = Some(wanted);
            return;
        }
        if self.job.as_ref().is_some_and(|j| (j.mode, j.query.as_str()) == (wanted.0, wanted.1.as_str())) {
            return;
        }
        if now - self.edited_at < DEBOUNCE_SECONDS {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let (mode, query) = wanted;
        let files: Vec<PathBuf> = files.as_ref().clone();
        let worker_query = query.clone();
        std::thread::spawn(move || {
            let answer = match mode {
                SearchMode::Content => {
                    let (hits, cut) = search_content(&files, &worker_query);
                    Answer::Content(hits, cut)
                }
                _ => Answer::Symbols(search_symbols(&files, &worker_query)),
            };
            let _ = sender.send(answer);
        });
        self.job = Some(Job { mode, query, receiver });
    }
}

/// Draws the Search panel body.
pub fn draw_search_panel(ui: &mut egui::Ui, state: &mut AppState) {
    // Taken out so the rows can borrow the rest of the state.
    let mut search = std::mem::take(&mut state.search);
    let now = ui.input(|i| i.time);
    let theme = state.theme.clone();
    let root = state.loaded_dir.clone();

    let before = search.wants();
    let mut mode = search.mode;
    segmented(
        ui,
        &mut mode,
        &[(SearchMode::Files, "Files"), (SearchMode::Content, "Content"), (SearchMode::Symbols, "Symbols")],
    );
    search.mode = mode;
    ui.add_space(6.0);
    let hint = match search.mode {
        SearchMode::Files => "Search file names",
        SearchMode::Content => "Search inside files",
        SearchMode::Symbols => "Search symbols",
    };
    let field_width = ui.available_width();
    ui.add(
        egui::TextEdit::singleline(&mut search.query)
            .hint_text(hint)
            .desired_width(field_width)
            .id(egui::Id::new("search_panel_input")),
    );
    if search.wants() != before {
        search.edited_at = now;
    }
    ui.add_space(6.0);

    let mut clicked: Option<(PathBuf, usize)> = None;
    match root {
        None => {
            ui.weak("Open a folder to search");
        }
        Some(root) => {
            let files = crate::workspace::folder_text_files(state);
            search.start_if_due(now, &files);
            if search.is_searching() {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            } else if search.answered.as_ref() != Some(&search.wants()) && search.mode != SearchMode::Files {
                // Waiting out the debounce.
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            }
            let query = search.query.trim().to_owned();
            match search.mode {
                SearchMode::Files => {
                    if query.is_empty() {
                        ui.weak("Type to find a file by name");
                    } else {
                        let hits = search_files(&root, &files, &query);
                        summary(ui, &theme, hits.len(), false, "file");
                        for hit in &hits {
                            if file_row(ui, &theme, hit).clicked() {
                                clicked = Some((hit.path.clone(), 0));
                            }
                        }
                    }
                }
                SearchMode::Content => {
                    if query.is_empty() {
                        ui.weak("Type to search the text of every file");
                    } else if search.answered.as_ref() != Some(&search.wants()) {
                        ui.weak("Searching…");
                    } else {
                        summary(ui, &theme, search.content.len(), search.content_cut, "match");
                        for hit in &search.content {
                            if content_row(ui, &theme, &root, hit).clicked() {
                                clicked = Some((hit.path.clone(), hit.line));
                            }
                        }
                    }
                }
                SearchMode::Symbols => {
                    if query.is_empty() {
                        ui.weak("Type to find a function, type or constant");
                    } else if search.answered.as_ref() != Some(&search.wants()) {
                        ui.weak("Searching…");
                    } else {
                        summary(ui, &theme, search.symbols.len(), false, "symbol");
                        for hit in &search.symbols {
                            if symbol_row(ui, &theme, &root, hit).clicked() {
                                clicked = Some((hit.symbol.path.clone(), hit.symbol.line));
                            }
                        }
                    }
                }
            }
        }
    }
    search.poll();
    state.search = search;
    if let Some((path, line)) = clicked {
        crate::workspace::focus_location(state, &path, line);
    }
}

fn summary(ui: &mut egui::Ui, theme: &Theme, count: usize, cut: bool, noun: &str) {
    let plural = if count == 1 { "" } else { "s" };
    let more = if cut { "+" } else { "" };
    let text = if count == 0 { format!("No {noun}s found") } else { format!("{count}{more} {noun}{plural}") };
    ui.label(egui::RichText::new(text).size(10.5).color(theme.text_muted));
    ui.add_space(2.0);
}

fn allocate_row(ui: &mut egui::Ui, theme: &Theme, label: String) -> (egui::Rect, egui::Response) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), egui::Sense::click());
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    if response.hovered() {
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

fn file_row(ui: &mut egui::Ui, theme: &Theme, hit: &FileHit) -> egui::Response {
    let (rect, response) = allocate_row(ui, theme, hit.relative.clone());
    let right = rect.right() - 8.0;
    let name_start = hit.relative.rfind('/').map_or(0, |i| hit.relative[..=i].chars().count());
    let name: String = hit.relative.chars().skip(name_start).collect();
    let dir: String = hit.relative.chars().take(name_start).collect();
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

fn content_row(ui: &mut egui::Ui, theme: &Theme, root: &Path, hit: &ContentHit) -> egui::Response {
    let relative = hit.path.strip_prefix(root).unwrap_or(&hit.path).to_string_lossy().into_owned();
    let (rect, response) = allocate_row(ui, theme, format!("{relative}:{} {}", hit.line + 1, hit.text));
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

fn symbol_row(ui: &mut egui::Ui, theme: &Theme, root: &Path, hit: &SymbolHit) -> egui::Response {
    let symbol = &hit.symbol;
    let relative = symbol.path.strip_prefix(root).unwrap_or(&symbol.path).to_string_lossy().into_owned();
    let (rect, response) = allocate_row(ui, theme, symbol.name.clone());
    let right = rect.right() - 8.0;
    let matched: &FuzzyMatch = &hit.matched;
    marked_text(
        ui,
        theme,
        egui::pos2(rect.left() + 8.0, rect.top() + 12.0),
        right,
        &symbol.name,
        &|i| matched.indices.contains(&i),
        12.0,
    );
    let kind = symbol.kind.replace("_declaration", "").replace("_item", "").replace("_definition", "");
    muted(
        ui,
        theme,
        egui::pos2(rect.left() + 8.0, rect.top() + 26.0),
        right,
        &format!("{kind} · {relative}:{}", symbol.line + 1),
    );
    response
}
