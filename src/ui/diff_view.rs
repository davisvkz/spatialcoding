//! The diff window: one file against Git `HEAD`, or at one commit against
//! another, as the current file with marks (**Changes**), a unified list or
//! two columns side by side. Words or characters that changed inside a
//! modified line are marked when the lines are alike enough. Rows are
//! fixed-height and only the visible ones are drawn, so a diff of a whole
//! new file stays cheap.

use egui::{Align2, Color32, FontId};
use serde::{Deserialize, Serialize};

use crate::git::diff::{DiffLine, DiffLoader, DiffSource, DiffStatus, FileDiff, Hunk, LineKind};
use crate::state::{AppState, CharDiff, DiffSettings};
use crate::ui::panel::segmented;
use crate::ui::theme::Theme;

const ROW_HEIGHT: f32 = 18.0;
const GUTTER: f32 = 42.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DiffMode {
    /// The new file's lines with the added ones marked; removed lines are
    /// one flag each.
    Changes,
    #[default]
    Unified,
    Split,
}

/// A run of text inside a line and whether it changed.
pub type Segments = Vec<(String, bool)>;

/// A diff line with the changed spans of its text, when they were computed.
#[derive(Clone)]
struct Shown {
    line: DiffLine,
    segments: Option<Segments>,
}

/// One drawn row.
enum Row {
    Hunk(String),
    Unified(Shown),
    Split(Option<Shown>, Option<Shown>),
    /// Changes mode: `n` removed lines, flagged where they were.
    Removed(usize),
    /// Collapse: `n` unchanged lines left out.
    Collapsed(usize),
}

/// The changed spans of the pair of lines `old` and `new`, or `None` when
/// they are less alike than `similarity` (0-1) or the character diff is off.
/// With `show_deleted` off the removed side marks nothing.
pub fn pair_segments(old: &str, new: &str, mode: CharDiff, similarity: f32, show_deleted: bool) -> Option<(Segments, Segments)> {
    use similar::{ChangeTag, TextDiff};
    if mode == CharDiff::Off {
        return None;
    }
    let ratio = TextDiff::from_chars(old, new).ratio();
    if ratio < similarity {
        return None;
    }
    let diff = match mode {
        CharDiff::Word => TextDiff::from_words(old, new),
        _ => TextDiff::from_chars(old, new),
    };
    let (mut left, mut right): (Segments, Segments) = (Vec::new(), Vec::new());
    let push = |list: &mut Segments, text: &str, changed: bool| match list.last_mut() {
        Some((last, flag)) if *flag == changed => last.push_str(text),
        _ => list.push((text.to_string(), changed)),
    };
    for change in diff.iter_all_changes() {
        let text = change.value().trim_end_matches(['\n', '\r']);
        match change.tag() {
            ChangeTag::Equal => {
                push(&mut left, text, false);
                push(&mut right, text, false);
            }
            ChangeTag::Delete => push(&mut left, text, show_deleted),
            ChangeTag::Insert => push(&mut right, text, true),
        }
    }
    Some((left, right))
}

/// Pairs the removed and added lines of each change block row by row and
/// attaches their character diff.
fn shown_hunk(hunk: &Hunk, settings: &DiffSettings) -> Vec<Shown> {
    let mut out: Vec<Shown> = hunk.lines.iter().cloned().map(|line| Shown { line, segments: None }).collect();
    let mut i = 0;
    while i < out.len() {
        if out[i].line.kind == LineKind::Context {
            i += 1;
            continue;
        }
        let start = i;
        while i < out.len() && out[i].line.kind != LineKind::Context {
            i += 1;
        }
        let removed: Vec<usize> = (start..i).filter(|&k| out[k].line.kind == LineKind::Removed).collect();
        let added: Vec<usize> = (start..i).filter(|&k| out[k].line.kind == LineKind::Added).collect();
        for (&r, &a) in removed.iter().zip(&added) {
            let pair = pair_segments(&out[r].line.text, &out[a].line.text, settings.char_diff, settings.similarity, settings.show_deleted_chars);
            if let Some((left, right)) = pair {
                out[r].segments = Some(left);
                out[a].segments = Some(right);
            }
        }
    }
    out
}

/// The rows of `diff` for `settings` and `mode`.
fn build_rows(diff: &FileDiff, mode: DiffMode, settings: &DiffSettings) -> Vec<Row> {
    let mut rows = Vec::new();
    for hunk in diff.hunks_with(settings.context) {
        let first = hunk.lines.first();
        let (old, new) = first.map_or((0, 0), |l| (l.old_no.unwrap_or(0), l.new_no.unwrap_or(0)));
        rows.push(Row::Hunk(format!("@@ -{old} +{new} @@")));
        let shown = shown_hunk(&hunk, settings);
        match mode {
            DiffMode::Unified => {
                let mut quiet = 0;
                for s in shown {
                    if settings.collapse && s.line.kind == LineKind::Context {
                        quiet += 1;
                        continue;
                    }
                    if quiet > 0 {
                        rows.push(Row::Collapsed(std::mem::take(&mut quiet)));
                    }
                    rows.push(Row::Unified(s));
                }
                if quiet > 0 {
                    rows.push(Row::Collapsed(quiet));
                }
            }
            DiffMode::Changes => {
                let (mut removed, mut quiet) = (0, 0);
                for s in shown {
                    match s.line.kind {
                        LineKind::Removed => {
                            if quiet > 0 {
                                rows.push(Row::Collapsed(std::mem::take(&mut quiet)));
                            }
                            removed += 1;
                        }
                        kind => {
                            if removed > 0 {
                                rows.push(Row::Removed(std::mem::take(&mut removed)));
                            }
                            if settings.collapse && kind == LineKind::Context {
                                quiet += 1;
                                continue;
                            }
                            if quiet > 0 {
                                rows.push(Row::Collapsed(std::mem::take(&mut quiet)));
                            }
                            rows.push(Row::Unified(s));
                        }
                    }
                }
                if removed > 0 {
                    rows.push(Row::Removed(removed));
                }
                if quiet > 0 {
                    rows.push(Row::Collapsed(quiet));
                }
            }
            DiffMode::Split => {
                // Pair like `Hunk::split_rows`, carrying the segments.
                let mut quiet = 0;
                let mut i = 0;
                while i < shown.len() {
                    if shown[i].line.kind == LineKind::Context {
                        if settings.collapse {
                            quiet += 1;
                        } else {
                            rows.push(Row::Split(Some(shown[i].clone()), Some(shown[i].clone())));
                        }
                        i += 1;
                        continue;
                    }
                    if quiet > 0 {
                        rows.push(Row::Collapsed(std::mem::take(&mut quiet)));
                    }
                    let start = i;
                    while i < shown.len() && shown[i].line.kind != LineKind::Context {
                        i += 1;
                    }
                    let block = &shown[start..i];
                    let removed: Vec<&Shown> = block.iter().filter(|l| l.line.kind == LineKind::Removed).collect();
                    let added: Vec<&Shown> = block.iter().filter(|l| l.line.kind == LineKind::Added).collect();
                    for n in 0..removed.len().max(added.len()) {
                        rows.push(Row::Split(removed.get(n).map(|l| (*l).clone()), added.get(n).map(|l| (*l).clone())));
                    }
                }
                if quiet > 0 {
                    rows.push(Row::Collapsed(quiet));
                }
            }
        }
    }
    rows
}

/// The open diff window.
pub struct DiffViewState {
    pub loader: DiffLoader,
    pub mode: DiffMode,
    rows: Vec<Row>,
    /// What `rows` was built for: the loader's answer count, the mode and
    /// the settings that shape the rows.
    built_for: Option<(u64, DiffMode, usize, CharDiff, u32, bool, bool)>,
    /// Counts the answers installed by the loader.
    revision: u64,
    /// "Show anyway" was clicked on the Large Diffs card.
    pub show_large: bool,
}

impl DiffViewState {
    pub fn open(path: std::path::PathBuf) -> Self {
        Self::from_loader(DiffLoader::new(path), DiffMode::default())
    }

    /// A diff of the file at commit `from` against commit `to`.
    pub fn open_between(path: std::path::PathBuf, from: String, to: String, context: usize, mode: DiffMode) -> Self {
        Self::from_loader(DiffLoader::with_source(path, DiffSource::Between { from, to }, context), mode)
    }

    fn from_loader(loader: DiffLoader, mode: DiffMode) -> Self {
        Self { loader, mode, rows: Vec::new(), built_for: None, revision: 0, show_large: false }
    }

    /// Installs a finished answer; true when one arrived.
    pub fn poll(&mut self) -> bool {
        let arrived = self.loader.poll();
        if arrived {
            self.revision += 1;
        }
        arrived
    }

    /// Number of rows drawn for the current answer, mode and settings.
    pub fn row_count(&mut self, settings: &DiffSettings) -> usize {
        self.rebuild_rows(settings);
        self.rows.len()
    }

    fn rebuild_rows(&mut self, settings: &DiffSettings) {
        let key = (
            self.revision,
            self.mode,
            settings.context,
            settings.char_diff,
            settings.similarity.to_bits(),
            settings.show_deleted_chars,
            settings.collapse,
        );
        if self.built_for == Some(key) {
            return;
        }
        self.built_for = Some(key);
        self.rows.clear();
        let DiffStatus::Ready(diff) = &self.loader.status else { return };
        self.rows = build_rows(diff, self.mode, settings);
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
        match &view.loader.source {
            DiffSource::Working => format!("Diff — {}", shown.display()),
            DiffSource::Between { from, to } => {
                let short = |h: &str| h.chars().take(7).collect::<String>();
                format!("Diff — {} ({} → {})", shown.display(), short(from), short(to))
            }
        }
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
            segmented(
                ui,
                &mut mode,
                &[(DiffMode::Changes, "Changes"), (DiffMode::Unified, "Unified"), (DiffMode::Split, "Split")],
            );
            view.mode = mode;
            state.settings.diff.mode = mode;
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
                    let against = match &view.loader.source {
                        DiffSource::Working => "No changes against HEAD",
                        DiffSource::Between { .. } => "No changes between these commits",
                    };
                    let changed = diff.added + diff.removed;
                    let limit = state.settings.diff_scan_limit;
                    if diff.binary {
                        ui.weak("Binary file: no lines to show");
                    } else if changed > limit && !view.show_large {
                        large_diff_card(ui, &theme, changed, limit, &mut view.show_large);
                    } else if diff.hunks.is_empty() {
                        ui.weak(against);
                    } else {
                        draw_rows(ui, &theme, &mut view, &state.settings.diff);
                    }
                }
            }
        });
    if open {
        state.diff = Some(view);
    }
}

/// The Large Diffs card: the diff changes more lines than the Diff Scan
/// Limit (Settings › Performance), so it waits for a click.
fn large_diff_card(ui: &mut egui::Ui, theme: &Theme, changed: usize, limit: usize, show: &mut bool) {
    egui::Frame::NONE
        .fill(theme.control_bg)
        .stroke(egui::Stroke::new(1.0_f32, theme.panel_border))
        .corner_radius(6.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Large Diffs").strong());
            ui.label(format!("This diff changes {changed} lines, over the limit of {limit}. Drawing it may be slow."));
            if ui.button("Show anyway").clicked() {
                *show = true;
            }
        });
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

fn draw_rows(ui: &mut egui::Ui, theme: &Theme, view: &mut DiffViewState, settings: &DiffSettings) {
    view.rebuild_rows(settings);
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
                    Row::Unified(shown) => unified_row(ui, theme, shown),
                    Row::Split(left, right) => split_row(ui, theme, left.as_ref(), right.as_ref()),
                    Row::Removed(n) => note_row(ui, &format!("− {n} removed line{}", if *n == 1 { "" } else { "s" }), removed_color()),
                    Row::Collapsed(n) => note_row(ui, &format!("⋯ {n} unchanged line{}", if *n == 1 { "" } else { "s" }), theme.text_muted),
                }
            }
        });
}

/// A thin row with a note (removed lines flag, collapsed lines).
fn note_row(ui: &mut egui::Ui, text: &str, color: Color32) {
    let rect = allocate(ui, text.to_owned());
    ui.painter().text(
        egui::pos2(rect.left() + GUTTER * 2.0 + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        FontId::monospace(11.0),
        color,
    );
}

/// Draws a line's text at `pos`, marking its changed segments.
fn code_text(painter: &egui::Painter, pos: egui::Pos2, shown: &Shown, color: Color32, mark: Color32) {
    match &shown.segments {
        None => {
            painter.text(pos, Align2::LEFT_CENTER, &shown.line.text, code_font(), color);
        }
        Some(segments) => {
            let mut job = egui::text::LayoutJob::default();
            for (text, changed) in segments {
                let mut format = egui::TextFormat::simple(code_font(), color);
                if *changed {
                    format.background = mark;
                }
                job.append(text, 0.0, format);
            }
            let galley = painter.layout_job(job);
            painter.galley(egui::pos2(pos.x, pos.y - galley.size().y / 2.0), galley, color);
        }
    }
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

fn unified_row(ui: &mut egui::Ui, theme: &Theme, shown: &Shown) {
    let line = &shown.line;
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
    code_text(&clip, egui::pos2(x + 14.0, mid), shown, theme.text_primary, mark_color(line.kind));
}

/// Background of the changed characters of a line.
fn mark_color(kind: LineKind) -> Color32 {
    match kind {
        LineKind::Added => Color32::from_rgba_unmultiplied(0x2e, 0xa0, 0x43, 110),
        LineKind::Removed => Color32::from_rgba_unmultiplied(0xf8, 0x51, 0x49, 110),
        LineKind::Context => Color32::TRANSPARENT,
    }
}

fn split_row(ui: &mut egui::Ui, theme: &Theme, left: Option<&Shown>, right: Option<&Shown>) {
    let describe = |line: Option<&Shown>| line.map(|l| format!("{} {}", sign(l.line.kind), l.line.text)).unwrap_or_default();
    let rect = allocate(ui, format!("{} | {}", describe(left), describe(right)));
    let half = rect.width() / 2.0;
    let sides = [(left, rect.left(), true), (right, rect.left() + half, false)];
    for (line, x, is_left) in sides {
        let column = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(half, ROW_HEIGHT));
        let painter = ui.painter().with_clip_rect(column);
        let Some(shown) = line else {
            painter.rect_filled(column, 0.0, theme.control_bg.gamma_multiply(0.5));
            continue;
        };
        let line = &shown.line;
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
        code_text(&painter, egui::pos2(column.left() + GUTTER + 6.0, mid), shown, theme.text_primary, mark_color(line.kind));
    }
    // The divider between the two columns.
    ui.painter().vline(rect.left() + half, rect.y_range(), egui::Stroke::new(1.0_f32, theme.panel_border));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff::diff_texts;

    fn texts(segments: &Segments) -> Vec<(&str, bool)> {
        segments.iter().map(|(t, c)| (t.as_str(), *c)).collect()
    }

    #[test]
    fn words_or_characters_that_changed_are_marked_when_the_lines_are_alike() {
        let (old, new) = pair_segments("let total = price + tax;", "let total = price * rate;", CharDiff::Word, 0.5, true).unwrap();
        assert!(texts(&old).contains(&("+", true)) && texts(&old).contains(&("tax;", true)), "{old:?}");
        assert!(texts(&new).contains(&("*", true)) && texts(&new).contains(&("rate;", true)), "{new:?}");
        assert_eq!(texts(&old)[0], ("let total = price ", false), "the common start is not marked");

        let (old, new) = pair_segments("color", "colour", CharDiff::Char, 0.5, true).unwrap();
        assert_eq!(texts(&old), vec![("color", false)], "nothing was deleted");
        assert_eq!(texts(&new), vec![("colo", false), ("u", true), ("r", false)]);
    }

    #[test]
    fn dissimilar_lines_off_mode_and_hidden_deleted_chars() {
        assert!(pair_segments("abcdef", "uvwxyz", CharDiff::Word, 0.5, true).is_none(), "too different");
        assert!(pair_segments("same", "sama", CharDiff::Off, 0.0, true).is_none(), "Off");
        let (old, _) = pair_segments("a b c", "a x c", CharDiff::Word, 0.2, false).unwrap();
        assert!(old.iter().all(|(_, changed)| !changed), "Show deleted chars off marks nothing on the removed line");
    }

    fn diff(old: &str, new: &str) -> FileDiff {
        FileDiff {
            path: "a.rs".into(),
            hunks: diff_texts(old, new),
            added: 0,
            removed: 0,
            is_new: false,
            is_deleted: false,
            binary: false,
            old_text: Some(old.into()),
            new_text: Some(new.into()),
        }
    }

    fn kinds(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| match r {
                Row::Hunk(_) => 'H',
                Row::Unified(s) => match s.line.kind {
                    LineKind::Context => ' ',
                    LineKind::Added => '+',
                    LineKind::Removed => '-',
                },
                Row::Split(..) => 'S',
                Row::Removed(_) => 'r',
                Row::Collapsed(_) => 'c',
            })
            .collect()
    }

    #[test]
    fn modes_build_unified_changes_and_split_rows_and_collapse_hides_context() {
        let d = diff("a\nb\nc\n", "a\nB\nc\n");
        let mut settings = DiffSettings::default();
        let rows = |mode, s: &DiffSettings| kinds(&build_rows(&d, mode, s));
        assert_eq!(rows(DiffMode::Unified, &settings), "H -+ ");
        assert_eq!(rows(DiffMode::Changes, &settings), "H r+ ", "removed lines are one flag");
        assert_eq!(rows(DiffMode::Split, &settings), "HSSS");
        settings.collapse = true;
        assert_eq!(rows(DiffMode::Unified, &settings), "Hc-+c");
        assert_eq!(rows(DiffMode::Changes, &settings), "Hcr+c");
        assert_eq!(rows(DiffMode::Split, &settings), "HcSc");
        // Context 0 leaves only the changed lines.
        settings.collapse = false;
        settings.context = 0;
        assert_eq!(rows(DiffMode::Unified, &settings), "H-+");
    }
}
