//! Changes panel of the left sidebar: the checked-out branch and the files
//! that differ from Git `HEAD`, grouped by how they differ. A row opens the
//! file's diff.

use std::path::PathBuf;

use egui::{Align2, Color32, FontId};

use crate::git::changes::ChangesStatus;
use crate::git::repo::ChangeKind;
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

pub fn draw_changes_panel(ui: &mut egui::Ui, state: &mut AppState) {
    let theme = state.theme.clone();
    let Some(root) = state.loaded_dir.clone() else {
        ui.weak("Open a folder to see its changes");
        return;
    };

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
