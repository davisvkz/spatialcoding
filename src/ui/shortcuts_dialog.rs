//! Shortcuts window (Ctrl+/): generated from the command registry, so it
//! can never list a key the app does not handle.

/// Groups of the dialog, in order.
pub const GROUPS: [&str; 5] = ["Viewport", "Canvas", "Selection", "Diff", "General"];

/// Mouse and trackpad gestures, which are not commands.
fn gestures(group: &str) -> &'static [(&'static str, &'static str)] {
    match group {
        "Viewport" => &[
            ("Zoom", "Ctrl + Scroll / pinch"),
            ("Pan", "Space + Drag / Right-Click Drag / two fingers"),
        ],
        "Selection" => &[("Open from the tree", "Right click → Open on canvas")],
        _ => &[],
    }
}

/// `(action, keys)` rows of one group.
pub fn rows(group: &str) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = gestures(group)
        .iter()
        .map(|(a, k)| ((*a).to_string(), (*k).to_string()))
        .collect();
    rows.extend(
        crate::commands::all()
            .iter()
            .filter(|c| c.group == Some(group) && !c.chords.is_empty())
            .map(|c| (c.title.to_string(), c.shortcut_label())),
    );
    rows
}

pub fn draw_shortcuts(ctx: &egui::Context, show: &mut bool) {
    if !*show {
        return;
    }

    egui::Window::new("Shortcuts")
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_height(480.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                for group in GROUPS {
                    let rows = rows(group);
                    if rows.is_empty() {
                        continue;
                    }
                    ui.add_space(4.0);
                    ui.heading(group);
                    for (action, keys) in rows {
                        shortcut_row(ui, &action, &keys);
                    }
                }
            });
            ui.separator();
            if ui.button("Close").clicked() {
                *show = false;
            }
        });
}

fn shortcut_row(ui: &mut egui::Ui, action: &str, keys: &str) {
    ui.horizontal(|ui| {
        ui.label(action);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(
                egui::RichText::new(keys).monospace().strong(),
            ));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_lists_commands_with_their_keys() {
        let viewport = rows("Viewport");
        assert!(viewport.iter().any(|(a, k)| a == "Fit view to nodes" && k == "Shift+F"));
        let canvas = rows("Canvas");
        assert!(canvas.iter().any(|(_, k)| k == "Ctrl+Shift+O"));
        assert!(canvas.iter().any(|(_, k)| k == "Shift+X"));
        let selection = rows("Selection");
        assert!(selection.iter().any(|(_, k)| k == "Backspace / Delete / Ctrl+X"));
        let diff = rows("Diff");
        assert!(diff.iter().any(|(_, k)| k == "Ctrl+Alt+Shift+O"));
        let general = rows("General");
        assert!(general.iter().any(|(_, k)| k == "Ctrl+K") || general.iter().any(|(_, k)| k == "Ctrl+/"));
    }

    #[test]
    fn every_shortcut_of_the_registry_is_in_a_group() {
        for c in crate::commands::all().iter().filter(|c| !c.chords.is_empty()) {
            assert!(c.group.is_some_and(|g| GROUPS.contains(&g)), "{} has keys but no dialog group", c.id);
        }
    }
}
