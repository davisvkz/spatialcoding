//! Changes panel of the left sidebar: the files that differ, grouped by how
//! they differ, from one of several sources: **Local changes** (the working
//! tree against `HEAD`), **Commits** (one commit against its parent) and
//! **Commit range** (the net changes between two commits). Pull requests are
//! listed but need the GitHub client (TASK-15). A row opens the file's diff,
//! which the panel's layout switch (Changes, Unified, Split) and gear
//! (context, character diff, deleted characters, similarity, collapse) shape.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use egui::{Align2, Color32, FontId};

use crate::git::changes::ChangesStatus;
use crate::git::history::{self, Commit};
use crate::git::repo::{ChangeKind, GitRepo};
use crate::state::CharDiff;
use crate::ui::diff_view::DiffMode;
use crate::ui::panel::segmented;
use crate::state::AppState;
use crate::ui::icons;
use crate::ui::theme::Theme;

const ROW_HEIGHT: f32 = 22.0;

/// Status colors, as in the diff window and `git status`.
pub fn kind_color(kind: ChangeKind) -> Color32 {
    match kind {
        ChangeKind::Modified => Color32::from_rgb(0xe3, 0xb3, 0x41),
        ChangeKind::Added => Color32::from_rgb(0x56, 0xd3, 0x64),
        ChangeKind::Deleted => Color32::from_rgb(0xf8, 0x51, 0x49),
        ChangeKind::Untracked => Color32::from_rgb(0x60, 0xa5, 0xfa),
    }
}

/// Where the listed changes come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChangesSource {
    #[default]
    Local,
    Commits,
    Range,
}

/// A value read from Git on a worker thread.
pub struct Fetch<T> {
    receiver: Option<Receiver<Result<T, String>>>,
    pub value: Option<Result<T, String>>,
}

impl<T: Send + 'static> Fetch<T> {
    pub fn spawn(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Self {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(work());
        });
        Self { receiver: Some(receiver), value: None }
    }

    pub fn is_loading(&self) -> bool {
        self.receiver.is_some()
    }

    /// Nonblocking; true when the answer arrived.
    pub fn poll(&mut self) -> bool {
        let Some(receiver) = &self.receiver else { return false };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err("Git worker stopped".to_string()),
        };
        self.receiver = None;
        self.value = Some(result);
        true
    }
}

/// A changed file and how it changed.
pub type ChangedFile = (PathBuf, ChangeKind);

/// The compared revisions of a commit or range view: `from` is the old side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compared {
    pub from: String,
    pub to: String,
}

/// State of the Changes panel besides the working-tree change set.
#[derive(Default)]
pub struct ChangesView {
    pub source: ChangesSource,
    commits: Option<Fetch<Vec<Commit>>>,
    /// Commits source: the commit shown.
    pub commit: Option<String>,
    /// Range source: the two ends (full hashes), old and new.
    pub range_from: Option<String>,
    pub range_to: Option<String>,
    files: Option<(Compared, Fetch<Vec<ChangedFile>>)>,
    /// The gear's settings are open.
    pub show_settings: bool,
}

impl ChangesView {
    /// The revisions the file list is for, if one is chosen.
    pub fn compared(&self, repo: &GitRepo) -> Option<Compared> {
        match self.source {
            ChangesSource::Local => None,
            ChangesSource::Commits => {
                let commit = self.commit.clone()?;
                Some(Compared { from: history::base_of(repo, &commit), to: commit })
            }
            ChangesSource::Range => Some(Compared { from: self.range_from.clone()?, to: self.range_to.clone()? }),
        }
    }

    /// The compared revisions without asking Git (the base of a single
    /// commit is read on the worker).
    fn wanted(&self) -> Option<(ChangesSource, String, Option<String>)> {
        match self.source {
            ChangesSource::Local => None,
            ChangesSource::Commits => self.commit.clone().map(|c| (ChangesSource::Commits, c, None)),
            ChangesSource::Range => Some((ChangesSource::Range, self.range_to.clone()?, Some(self.range_from.clone()?))),
        }
    }

    pub fn commits(&self) -> Option<&Fetch<Vec<Commit>>> {
        self.commits.as_ref()
    }

    /// Back to the working tree.
    pub fn back_to_local(&mut self) {
        self.source = ChangesSource::Local;
    }

    pub fn is_loading(&self) -> bool {
        self.commits.as_ref().is_some_and(Fetch::is_loading) || self.files.as_ref().is_some_and(|(_, f)| f.is_loading())
    }

    pub fn poll(&mut self) {
        if let Some(c) = &mut self.commits {
            c.poll();
        }
        if let Some((_, f)) = &mut self.files {
            f.poll();
        }
    }
}

/// Starts the reads the current source needs: the commit list, and the
/// file list of the chosen commit or range.
fn ensure_loaded(view: &mut ChangesView, root: &std::path::Path) {
    if view.source != ChangesSource::Local && view.commits.is_none() {
        let root = root.to_path_buf();
        view.commits = Some(Fetch::spawn(move || {
            history::commits(&GitRepo::open(&root).ok_or("Not a Git repository")?, history::MAX_COMMITS)
        }));
    }
    let Some((_, to, from)) = view.wanted() else {
        view.files = None;
        return;
    };
    let needed = Compared { from: from.clone().unwrap_or_default(), to: to.clone() };
    if view.files.as_ref().is_some_and(|(c, _)| *c == needed) {
        return;
    }
    let root = root.to_path_buf();
    let single = from.is_none();
    let worker = Compared { from: from.unwrap_or_default(), to };
    let key = needed;
    view.files = Some((
        key,
        Fetch::spawn(move || {
            let repo = GitRepo::open(&root).ok_or("Not a Git repository")?;
            let from = if single { history::base_of(&repo, &worker.to) } else { worker.from.clone() };
            history::changed_between(&repo, &from, &worker.to)
        }),
    ));
}

/// Layout switch and the settings gear, shared by every source.
fn draw_diff_controls(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        let mut mode = state.settings.diff.mode;
        let switch = ui.scope(|ui| {
            segmented(ui, &mut mode, &[(DiffMode::Changes, "Changes"), (DiffMode::Unified, "Unified"), (DiffMode::Split, "Split")])
        });
        crate::commands::note_anchor(state, "changes:layout", switch.response.rect);
        if mode != state.settings.diff.mode {
            state.settings.diff.mode = mode;
            if let Some(diff) = state.diff.as_mut() {
                diff.mode = mode;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if state.changes_view.show_settings { "Hide diff settings" } else { "Diff settings" };
            let gear = icons::tool_icon_button(ui, icons::SETTINGS, label);
            crate::commands::note_anchor(state, "changes:settings", gear.rect);
            if gear.clicked() {
                state.changes_view.show_settings = !state.changes_view.show_settings;
            }
        });
    });
    if !state.changes_view.show_settings {
        return;
    }
    let diff = &mut state.settings.diff;
    egui::Frame::NONE.fill(ui.visuals().faint_bg_color).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("Context");
            ui.add(egui::DragValue::new(&mut diff.context).range(0..=20)).on_hover_text("Unchanged lines around each change");
        });
        ui.horizontal(|ui| {
            ui.label("Character diff");
            segmented(ui, &mut diff.char_diff, &[(CharDiff::Word, "Word"), (CharDiff::Char, "Char"), (CharDiff::Off, "Off")]);
        });
        ui.checkbox(&mut diff.show_deleted_chars, "Show deleted chars");
        ui.horizontal(|ui| {
            ui.label("Similarity");
            ui.add(egui::Slider::new(&mut diff.similarity, 0.0..=1.0).fixed_decimals(2))
                .on_hover_text("Changed lines are compared character by character only when this alike");
        });
        ui.checkbox(&mut diff.collapse, "Collapse unchanged lines (C)");
    });
}

fn commit_label(c: &Commit) -> String {
    format!("{}  {}", c.short, c.subject)
}

/// The commit list or the two range pickers.
fn draw_commit_source(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme) {
    let view = &mut state.changes_view;
    let Some(commits) = view.commits.as_ref().and_then(|c| c.value.as_ref()) else {
        ui.weak("Reading the history…");
        return;
    };
    let commits = match commits {
        Ok(commits) => commits.clone(),
        Err(message) => {
            ui.weak(format!("Git is unavailable here: {message}"));
            return;
        }
    };
    if commits.is_empty() {
        ui.weak("No commits yet");
        return;
    }
    match view.source {
        ChangesSource::Commits => {
            for commit in &commits {
                let selected = view.commit.as_deref() == Some(commit.hash.as_str());
                let response = ui.selectable_label(selected, commit_label(commit)).on_hover_text(format!("{} · {}", commit.author, commit.when));
                if response.clicked() {
                    view.commit = Some(commit.hash.clone());
                }
            }
        }
        _ => {
            let pick = |ui: &mut egui::Ui, label: &str, id: &str, value: &mut Option<String>| {
                let shown = value
                    .as_ref()
                    .and_then(|h| commits.iter().find(|c| &c.hash == h))
                    .map_or("Choose a commit".to_string(), commit_label);
                ui.horizontal(|ui| {
                    ui.label(label);
                    egui::ComboBox::from_id_salt(id).selected_text(shown).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
                        for commit in &commits {
                            ui.selectable_value(value, Some(commit.hash.clone()), commit_label(commit));
                        }
                    });
                });
            };
            pick(ui, "From", "range_from", &mut view.range_from);
            pick(ui, "To", "range_to", &mut view.range_to);
            ui.weak("Net changes between the two commits, along the first-parent history.");
        }
    }
    let _ = theme;
}

pub fn draw_changes_panel(ui: &mut egui::Ui, state: &mut AppState) {
    let theme = state.theme.clone();
    let Some(root) = state.loaded_dir.clone() else {
        ui.weak("Open a folder to see its changes");
        return;
    };
    state.changes_view.poll();
    ensure_loaded(&mut state.changes_view, &root);
    if state.changes_view.is_loading() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }

    // Sources.
    ui.horizontal_wrapped(|ui| {
        let mut source = state.changes_view.source;
        segmented(
            ui,
            &mut source,
            &[(ChangesSource::Local, "Local changes"), (ChangesSource::Commits, "Commits"), (ChangesSource::Range, "Commit range")],
        );
        state.changes_view.source = source;
        ui.add_enabled(false, egui::Button::new("Pull requests").small())
            .on_disabled_hover_text("Pull requests need the GitHub client (TASK-15)");
    });
    ui.add_space(4.0);
    draw_diff_controls(ui, state);
    ui.add_space(4.0);

    if state.changes_view.source != ChangesSource::Local {
        if ui.small_button("Back to working tree").clicked() {
            state.changes_view.back_to_local();
            return;
        }
        draw_commit_source(ui, state, &theme);
        ui.add_space(4.0);
        draw_compared_files(ui, state, &theme, &root);
        return;
    }
    draw_local(ui, state, &theme, &root);
}

/// The files of the chosen commit or range.
fn draw_compared_files(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme, root: &std::path::Path) {
    let Some((compared, fetch)) = state.changes_view.files.as_ref() else {
        ui.weak(match state.changes_view.source {
            ChangesSource::Commits => "Choose a commit",
            _ => "Choose the two commits to compare",
        });
        return;
    };
    let compared = compared.clone();
    let entries = match &fetch.value {
        None => {
            ui.weak("Reading the changes…");
            return;
        }
        Some(Err(message)) => {
            ui.weak(format!("Git is unavailable here: {message}"));
            return;
        }
        Some(Ok(entries)) => entries.clone(),
    };
    if entries.is_empty() {
        ui.weak("No files differ");
        return;
    }
    let single = state.changes_view.source == ChangesSource::Commits;
    // The old side of a single commit is its parent, read when a file opens.
    let mut open: Option<PathBuf> = None;
    for kind in [ChangeKind::Modified, ChangeKind::Added, ChangeKind::Deleted] {
        let files: Vec<&PathBuf> = entries.iter().filter(|(_, k)| *k == kind).map(|(p, _)| p).collect();
        if files.is_empty() {
            continue;
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new(format!("{} ({})", kind.label(), files.len())).size(10.5).color(theme.text_muted));
        for path in files {
            let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned();
            if change_row(ui, theme, kind, &relative).clicked() {
                open = Some(path.clone());
            }
        }
    }
    if let Some(path) = open {
        let from = if single {
            GitRepo::open(root).map(|r| history::base_of(&r, &compared.to)).unwrap_or(compared.from.clone())
        } else {
            compared.from.clone()
        };
        crate::workspace::open_diff_between(state, path, from, compared.to);
    }
}

/// Local changes: the branch and the working-tree change set.
fn draw_local(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme, root: &std::path::Path) {
    let theme = theme.clone();
    let root = root.to_path_buf();
    ui.horizontal(|ui| {
        let branch = match state.changes.branch() {
            Some(name) => format!("On branch {name}"),
            None => "No branch".to_owned(),
        };
        ui.label(egui::RichText::new(branch).size(11.0).color(theme.text_secondary));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::tool_icon_button(ui, icons::REFRESH, "Refresh changes").clicked() {
                let now = ui.input(|i| i.time);
                crate::workspace::refresh_working_tree_changes(state, now);
            }
        });
    });
    ui.add_space(4.0);

    match &state.changes.status {
        ChangesStatus::Idle | ChangesStatus::Loading => {
            ui.weak("Reading changes…");
            return;
        }
        ChangesStatus::Unavailable(message) => {
            ui.weak(format!("Git is unavailable here: {message}"));
            return;
        }
        ChangesStatus::Ready => {}
    }
    if state.changes.entries().is_empty() {
        ui.weak("No changes: the working tree matches HEAD");
        return;
    }

    let mut open: Option<PathBuf> = None;
    for kind in [ChangeKind::Modified, ChangeKind::Added, ChangeKind::Deleted, ChangeKind::Untracked] {
        let files: Vec<&PathBuf> =
            state.changes.entries().iter().filter(|(_, k)| *k == kind).map(|(p, _)| p).collect();
        if files.is_empty() {
            continue;
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new(format!("{} ({})", kind.label(), files.len())).size(10.5).color(theme.text_muted));
        for path in files {
            let relative = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().into_owned();
            if change_row(ui, &theme, kind, &relative).clicked() {
                open = Some(path.clone());
            }
        }
    }
    if let Some(path) = open {
        crate::workspace::open_diff(state, path);
    }
}

fn change_row(ui: &mut egui::Ui, theme: &Theme, kind: ChangeKind, relative: &str) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), egui::Sense::click());
    let label = relative.to_owned();
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, theme.control_bg);
    }
    let mid = rect.center().y;
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, mid),
        Align2::CENTER_CENTER,
        kind.letter(),
        FontId::monospace(11.0),
        kind_color(kind),
    );
    let (dir, name) = match relative.rsplit_once('/') {
        Some((dir, name)) => (Some(dir), name),
        None => (None, relative),
    };
    let clip = ui.painter().with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(rect.left() + 26.0, rect.top()),
        egui::pos2(rect.right() - 6.0, rect.bottom()),
    ));
    let name_width = clip
        .text(egui::pos2(rect.left() + 26.0, mid), Align2::LEFT_CENTER, name, FontId::proportional(12.0), theme.text_primary)
        .width();
    if let Some(dir) = dir {
        clip.text(
            egui::pos2(rect.left() + 26.0 + name_width + 6.0, mid),
            Align2::LEFT_CENTER,
            dir,
            FontId::proportional(10.0),
            theme.text_muted,
        );
    }
    response
}
