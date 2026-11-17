//! The diff window: one file against Git `HEAD`, as a unified list or as two
//! columns side by side. Rows are fixed-height and only the visible ones are
//! drawn, so a diff of a whole new file stays cheap.

use egui::{Align2, Color32, FontId};

use crate::git::diff::{DiffLoader, DiffLine, DiffStatus, FileDiff, LineKind};
use crate::state::AppState;
use crate::ui::panel::segmented;
use crate::ui::theme::Theme;

const ROW_HEIGHT: f32 = 18.0;
const GUTTER: f32 = 42.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffMode {
    #[default]
    Unified,
    Split,
}

/// One drawn row.
enum Row {
    Hunk(String),
    Unified(DiffLine),
    Split(Option<DiffLine>, Option<DiffLine>),
}

/// The open diff window.
pub struct DiffViewState {
    pub loader: DiffLoader,
    pub mode: DiffMode,
    rows: Vec<Row>,
    /// What `rows` was built for: the loader's answer count and the mode.
    built_for: Option<(u64, DiffMode)>,
    /// Counts the answers installed by the loader.
    revision: u64,
}

impl DiffViewState {
    pub fn open(path: std::path::PathBuf) -> Self {
        Self { loader: DiffLoader::new(path), mode: DiffMode::default(), rows: Vec::new(), built_for: None, revision: 0 }
    }

    /// Installs a finished answer; true when one arrived.
    pub fn poll(&mut self) -> bool {
        let arrived = self.loader.poll();
        if arrived {
            self.revision += 1;
        }
        arrived
    }

    /// Number of rows drawn for the current answer and mode.
    pub fn row_count(&mut self) -> usize {
        self.rebuild_rows();
        self.rows.len()
    }

    fn rebuild_rows(&mut self) {
        let key = (self.revision, self.mode);
        if self.built_for == Some(key) {
            return;
        }
        self.built_for = Some(key);
        self.rows.clear();
        let DiffStatus::Ready(diff) = &self.loader.status else { return };
        for hunk in &diff.hunks {
            let first = hunk.lines.first();
            let (old, new) = first.map_or((0, 0), |l| (l.old_no.unwrap_or(0), l.new_no.unwrap_or(0)));
            self.rows.push(Row::Hunk(format!("@@ -{old} +{new} @@")));
            match self.mode {
                DiffMode::Unified => self.rows.extend(hunk.lines.iter().cloned().map(Row::Unified)),
                DiffMode::Split => {
                    self.rows.extend(hunk.split_rows().into_iter().map(|r| Row::Split(r.left, r.right)));
                }
            }
        }
    }
}

fn added_color() -> Color32 {
    Color32::from_rgb(0x56, 0xd3, 0x64)
}

fn removed_color() -> Color32 {
    Color32::from_rgb(0xf8, 0x51, 0x49)
}

fn tint(kind: LineKind) -> Option<Color32> {
    match kind {
        LineKind::Added => Some(Color32::from_rgba_unmultiplied(0x2e, 0xa0, 0x43, 46)),
        LineKind::Removed => Some(Color32::from_rgba_unmultiplied(0xf8, 0x51, 0x49, 46)),
        LineKind::Context => None,
    }
}

/// Draws the window while `state.diff` is set. Closing it clears the state.
pub fn draw_diff_view(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut view) = state.diff.take() else { return };
    let theme = state.theme.clone();
    let title = {
        let root = state.loaded_dir.as_deref();
        let path = &view.loader.path;
        let shown = root.and_then(|r| path.strip_prefix(r).ok()).unwrap_or(path);
        format!("Diff — {}", shown.display())
    };
    let mut open = true;
    egui::Window::new(title)
        .id(egui::Id::new("diff_view_window"))
        .open(&mut open)
        .default_size([860.0, 520.0])
        .min_size([420.0, 200.0])
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            let mut mode = view.mode;
            segmented(ui, &mut mode, &[(DiffMode::Unified, "Unified"), (DiffMode::Split, "Split")]);
            view.mode = mode;
            ui.add_space(4.0);
            match view.loader.status.clone() {
                DiffStatus::Loading => {
                    ui.weak("Reading the diff…");
                }
                DiffStatus::Failed(message) => {
                    ui.colored_label(removed_color(), message);
                }
                DiffStatus::Ready(diff) => {
                    summary(ui, &theme, &diff);
                    if diff.binary {
                        ui.weak("Binary file: no lines to show");
                    } else if diff.hunks.is_empty() {
                        ui.weak("No changes against HEAD");
                    } else {
                        draw_rows(ui, &theme, &mut view);
                    }
                }
            }
        });
    if open {
        state.diff = Some(view);
    }
}

fn summary(ui: &mut egui::Ui, theme: &Theme, diff: &FileDiff) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let kind = if diff.is_new {
            "New file"
        } else if diff.is_deleted {
            "Deleted file"
        } else {
            "Modified file"
        };
        ui.label(egui::RichText::new(kind).size(11.0).color(theme.text_muted));
        ui.label(egui::RichText::new(format!("+{}", diff.added)).size(11.0).color(added_color()));
        ui.label(egui::RichText::new(format!("−{}", diff.removed)).size(11.0).color(removed_color()));
    });
    ui.add_space(2.0);
}

fn draw_rows(ui: &mut egui::Ui, theme: &Theme, view: &mut DiffViewState) {
    view.rebuild_rows();
    let mode = view.mode;
    let rows = &view.rows;
    egui::ScrollArea::vertical()
        .id_salt(("diff_rows", mode == DiffMode::Split))
        .auto_shrink([false, false])
        .show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &rows[range] {
                match row {
                    Row::Hunk(text) => hunk_row(ui, theme, text),
                    Row::Unified(line) => unified_row(ui, theme, line),
                    Row::Split(left, right) => split_row(ui, theme, left.as_ref(), right.as_ref()),
                }
            }
        });
}

fn code_font() -> FontId {
    FontId::monospace(12.0)
}

fn allocate(ui: &mut egui::Ui, label: String) -> egui::Rect {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), egui::Sense::hover());
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.clone()));
    rect
}

fn hunk_row(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let rect = allocate(ui, text.to_owned());
    ui.painter().rect_filled(rect, 0.0, theme.control_bg);
    ui.painter().text(
        egui::pos2(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        FontId::monospace(11.0),
        theme.text_muted,
    );
}

fn sign(kind: LineKind) -> &'static str {
    match kind {
        LineKind::Added => "+",
        LineKind::Removed => "-",
        LineKind::Context => " ",
    }
}

fn number(n: Option<usize>) -> String {
    n.map(|n| n.to_string()).unwrap_or_default()
}

fn text_color(theme: &Theme, kind: LineKind) -> Color32 {
    match kind {
        LineKind::Added => added_color(),
        LineKind::Removed => removed_color(),
        LineKind::Context => theme.text_primary,
    }
}

fn unified_row(ui: &mut egui::Ui, theme: &Theme, line: &DiffLine) {
    let rect = allocate(ui, format!("{} {}", sign(line.kind), line.text));
    let painter = ui.painter();
    if let Some(fill) = tint(line.kind) {
        painter.rect_filled(rect, 0.0, fill);
    }
    let mid = rect.center().y;
    for (i, n) in [line.old_no, line.new_no].into_iter().enumerate() {
        painter.text(
            egui::pos2(rect.left() + GUTTER * (i as f32 + 1.0) - 6.0, mid),
            Align2::RIGHT_CENTER,
            number(n),
            code_font(),
            theme.line_number,
        );
    }
    let x = rect.left() + GUTTER * 2.0 + 6.0;
    painter.text(egui::pos2(x, mid), Align2::LEFT_CENTER, sign(line.kind), code_font(), text_color(theme, line.kind));
    let clip = painter.with_clip_rect(egui::Rect::from_min_max(egui::pos2(x + 14.0, rect.top()), rect.right_bottom()));
    clip.text(egui::pos2(x + 14.0, mid), Align2::LEFT_CENTER, &line.text, code_font(), theme.text_primary);
}

fn split_row(ui: &mut egui::Ui, theme: &Theme, left: Option<&DiffLine>, right: Option<&DiffLine>) {
    let describe = |line: Option<&DiffLine>| line.map(|l| format!("{} {}", sign(l.kind), l.text)).unwrap_or_default();
    let rect = allocate(ui, format!("{} | {}", describe(left), describe(right)));
    let half = rect.width() / 2.0;
    let sides = [(left, rect.left(), true), (right, rect.left() + half, false)];
    for (line, x, is_left) in sides {
        let column = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(half, ROW_HEIGHT));
        let painter = ui.painter().with_clip_rect(column);
        let Some(line) = line else {
            painter.rect_filled(column, 0.0, theme.control_bg.gamma_multiply(0.5));
            continue;
        };
        if let Some(fill) = tint(line.kind) {
            painter.rect_filled(column, 0.0, fill);
        }
        let mid = rect.center().y;
        let n = if is_left { line.old_no } else { line.new_no };
        painter.text(
            egui::pos2(column.left() + GUTTER - 6.0, mid),
            Align2::RIGHT_CENTER,
            number(n),
            code_font(),
            theme.line_number,
        );
        painter.text(
            egui::pos2(column.left() + GUTTER + 6.0, mid),
            Align2::LEFT_CENTER,
            &line.text,
            code_font(),
            theme.text_primary,
        );
    }
    // The divider between the two columns.
    ui.painter().vline(rect.left() + half, rect.y_range(), egui::Stroke::new(1.0_f32, theme.panel_border));
}
