//! The 33px top bar of v2: a folder pill, the Select and Move tools, and the
//! Clear all / Clear selected buttons, all icon-only.

use crate::state::{AppState, Tool};
use crate::ui::icons;

/// Height of the top bar, as in v2.
pub const TOOLBAR_HEIGHT: f32 = 33.0;

pub fn draw_toolbar(ui: &mut egui::Ui, state: &mut AppState) {
    let theme = state.theme.clone();
    ui.horizontal_centered(|ui| {
        // Aligned with the rails' icons: v2 puts the pill 44px from the edge.
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing.x = 8.0;

        let folder_label = state
            .loaded_dir
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "No folder".into());
        let pill = folder_pill(ui, &theme, &folder_label);
        let popup_id = egui::Id::new("repository_dropdown");
        if pill.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup_id));
        }
        egui::popup_below_widget(ui, popup_id, &pill, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(220.0);
            if ui.button("Open folder…").clicked() {
                state.pending_app_actions.push(crate::commands::AppAction::PickFolder);
            }
            let current = state.loaded_dir.clone();
            let recents: Vec<_> = state
                .settings
                .recent_folders
                .iter()
                .filter(|d| Some(*d) != current.as_ref())
                .cloned()
                .collect();
            if !recents.is_empty() {
                ui.separator();
                ui.weak("Recent");
                for dir in recents {
                    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string());
                    if ui.button(name).on_hover_text(dir.display().to_string()).clicked() {
                        state.pending_app_actions.push(crate::commands::AppAction::OpenFolder(dir));
                    }
                }
            }
            if current.is_some() {
                ui.separator();
                if ui.button("Clear repository").clicked() {
                    state.pending_app_actions.push(crate::commands::AppAction::CloseFolder);
                }
            }
        });

        if let Some(branch) = state.changes.branch().map(str::to_owned)
            && branch_pill(ui, &theme, &branch).clicked()
        {
            state.left_panel = Some(crate::state::LeftPanel::Changes);
        }

        if !state.settings.show_controls_bar {
            return;
        }
        separator(ui, &theme);

        if icons::toolbar_icon_button(ui, icons::POINTER, "Select tool", state.active_tool == Tool::Select)
            .clicked()
        {
            state.active_tool = Tool::Select;
        }
        if icons::toolbar_icon_button(ui, icons::HAND, "Move tool", state.active_tool == Tool::Move)
            .clicked()
        {
            state.active_tool = Tool::Move;
        }

        separator(ui, &theme);

        let has_nodes =
            !state.node_store.file_nodes.is_empty() || !state.node_store.folder_nodes.is_empty();
        let clear_all = ui
            .add_enabled_ui(has_nodes, |ui| {
                icons::toolbar_icon_button(ui, icons::CLOSE, "Clear all nodes", false)
            })
            .inner;
        if clear_all.clicked() {
            crate::workspace::clear_canvas(state);
        }

        let has_selection = state.selection.count() > 0;
        let clear_selected = ui
            .add_enabled_ui(has_selection, |ui| {
                icons::toolbar_icon_button(ui, icons::TRASH, "Clear selected nodes", false)
            })
            .inner;
        if clear_selected.clicked() {
            let ids: Vec<u64> = state.selection.selected_ids().collect();
            crate::workspace::remove_nodes(state, &ids);
        }
    });
}

/// The folder selector: folder glyph, name and a chevron in a 150x22 pill.
fn folder_pill(ui: &mut egui::Ui, theme: &crate::ui::theme::Theme, name: &str) -> egui::Response {
    use egui::{Align2, FontId};
    let (rect, response) = ui.allocate_exact_size(egui::vec2(150.0, 22.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if response.hovered() {
            theme.control_bg.lerp_to_gamma(egui::Color32::WHITE, 0.05)
        } else {
            theme.control_bg
        };
        let border = egui::Color32::from_white_alpha(20);
        ui.painter()
            .rect(rect, 11.0, fill, egui::Stroke::new(1.0_f32, border), egui::StrokeKind::Inside);
        let icon_font = FontId::new(12.0, egui::FontFamily::Name(icons::ICON_FAMILY.into()));
        let mid = rect.center().y;
        ui.painter().text(
            egui::pos2(rect.left() + 9.0, mid),
            Align2::LEFT_CENTER,
            icons::FOLDER,
            icon_font.clone(),
            theme.text_secondary,
        );
        ui.painter().text(
            egui::pos2(rect.right() - 9.0, mid),
            Align2::RIGHT_CENTER,
            icons::CHEVRON_DOWN,
            icon_font,
            theme.text_muted,
        );
        let clip = ui.painter().with_clip_rect(egui::Rect::from_min_max(
            egui::pos2(rect.left() + 26.0, rect.top()),
            egui::pos2(rect.right() - 24.0, rect.bottom()),
        ));
        clip.text(
            egui::pos2(rect.left() + 26.0, mid),
            Align2::LEFT_CENTER,
            name,
            FontId::proportional(11.0),
            theme.text_primary,
        );
    }
    let ak_label = name.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, ak_label.clone())
    });
    response
}

/// The checked-out Git branch: branch glyph and name; a click opens the
/// Changes panel.
fn branch_pill(ui: &mut egui::Ui, theme: &crate::ui::theme::Theme, name: &str) -> egui::Response {
    use egui::{Align2, FontId};
    let font = FontId::proportional(11.0);
    let text_width = ui.painter().layout_no_wrap(name.to_owned(), font.clone(), theme.text_primary).size().x;
    let width = (text_width + 34.0).min(180.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if response.hovered() {
            theme.control_bg.lerp_to_gamma(egui::Color32::WHITE, 0.05)
        } else {
            theme.control_bg
        };
        ui.painter().rect(
            rect,
            11.0,
            fill,
            egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(20)),
            egui::StrokeKind::Inside,
        );
        let mid = rect.center().y;
        ui.painter().text(
            egui::pos2(rect.left() + 9.0, mid),
            Align2::LEFT_CENTER,
            icons::GIT_BRANCH,
            FontId::new(12.0, egui::FontFamily::Name(icons::ICON_FAMILY.into())),
            theme.text_secondary,
        );
        let clip = ui.painter().with_clip_rect(egui::Rect::from_min_max(
            egui::pos2(rect.left() + 26.0, rect.top()),
            egui::pos2(rect.right() - 8.0, rect.bottom()),
        ));
        clip.text(egui::pos2(rect.left() + 26.0, mid), Align2::LEFT_CENTER, name, font, theme.text_primary);
    }
    let label = format!("Branch {name}");
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    response.on_hover_text("Git branch: show changes")
}

/// A 1px vertical divider between button groups.
fn separator(ui: &mut egui::Ui, theme: &crate::ui::theme::Theme) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 14.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme.panel_border);
}
