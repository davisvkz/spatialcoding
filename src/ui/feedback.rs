//! Feedback dialog (the bug icon of the left rail): a message and an
//! optional diagnostic log. The log is built here, shown in full before
//! anything leaves the app and has the user's home folder, the open
//! repository and the user name redacted. There is no feedback server: the
//! report is copied to the clipboard to paste where you report issues.

use crate::state::AppState;

/// The dialog's draft.
#[derive(Debug, Clone, Default)]
pub struct Feedback {
    pub message: String,
    pub include_log: bool,
    pub copied: bool,
}

/// `text` with the home folder, the repository folder and the user name
/// replaced by placeholders.
pub fn redact(text: &str, home: Option<&str>, repo: Option<&str>, user: Option<&str>) -> String {
    let mut out = text.to_string();
    // The longer path first: the repository usually lives under the home.
    for (needle, with) in [(repo, "<repo>"), (home, "~")] {
        if let Some(needle) = needle.filter(|n| n.len() > 1) {
            out = out.replace(needle, with);
        }
    }
    if let Some(user) = user.filter(|u| u.len() > 2) {
        out = out.replace(user, "<user>");
    }
    out
}

/// What the diagnostic log holds: versions, the platform and the state of
/// the language servers. Paths are redacted by [`redact`].
pub fn diagnostics(state: &AppState) -> String {
    let mut lines = vec![
        format!("SpatialCoding {}", env!("CARGO_PKG_VERSION")),
        format!("Platform: {} {}", std::env::consts::OS, std::env::consts::ARCH),
        format!("Open files: {}", state.node_store.file_nodes.len()),
        format!("Connected agents: {}", state.agents.len()),
    ];
    if let Some(dir) = &state.loaded_dir {
        lines.push(format!("Repository: {}", dir.display()));
    }
    for progress in state.deps.progress(&state.lsp.manager) {
        let status = progress.error.as_deref().map_or("running".to_string(), |e| format!("error: {e}"));
        lines.push(format!("Language server {:?}: {status}", progress.server));
    }
    lines.join("\n")
}

/// The report: the message, then the redacted diagnostic log when asked.
pub fn report(state: &AppState, feedback: &Feedback) -> String {
    let mut text = feedback.message.trim().to_string();
    if feedback.include_log {
        let home = std::env::var("HOME").ok();
        let user = std::env::var("USER").ok();
        let repo = state.loaded_dir.as_ref().map(|d| d.display().to_string());
        let log = redact(&diagnostics(state), home.as_deref(), repo.as_deref(), user.as_deref());
        text.push_str("\n\n--- Diagnostics ---\n");
        text.push_str(&log);
    }
    text
}

pub fn draw_feedback(ctx: &egui::Context, state: &mut AppState) {
    let Some(mut draft) = state.feedback.take() else { return };
    let mut open = true;
    let mut close = false;
    egui::Window::new("Send feedback")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label("What happened, or what would you like to see?");
            ui.add(
                egui::TextEdit::multiline(&mut draft.message)
                    .desired_rows(5)
                    .desired_width(f32::INFINITY)
                    .hint_text("Your feedback"),
            );
            ui.checkbox(&mut draft.include_log, "Include a diagnostic log");
            if draft.include_log {
                let preview = report(state, &Feedback { message: String::new(), ..draft.clone() });
                ui.weak("Paths and your user name are redacted. This is everything the log holds:");
                egui::Frame::NONE.fill(ui.visuals().extreme_bg_color).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(preview.trim()).monospace().small()).wrap());
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                let empty = draft.message.trim().is_empty() && !draft.include_log;
                if ui.add_enabled(!empty, egui::Button::new("Copy report")).clicked() {
                    let text = report(state, &draft);
                    ui.ctx().copy_text(text);
                    draft.copied = true;
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
                if draft.copied {
                    ui.weak("Copied: paste it where you report issues.");
                }
            });
        });
    if open && !close {
        state.feedback = Some(draft);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_repository_and_user_are_redacted() {
        let text = "Repository: /home/ana/work/shop\nconfig /home/ana/.config/x\nowner ana";
        let out = redact(text, Some("/home/ana"), Some("/home/ana/work/shop"), Some("ana"));
        assert_eq!(out, "Repository: <repo>\nconfig ~/.config/x\nowner <user>");
    }

    #[test]
    fn short_or_missing_values_do_not_mangle_the_text() {
        let text = "a/b and a";
        assert_eq!(redact(text, Some("/"), None, Some("a")), text, "a root home and a one-letter user are left alone");
        assert_eq!(redact(text, None, None, None), text);
    }
}
