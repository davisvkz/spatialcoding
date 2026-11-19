//! The command bus: every action of the UI is a named command
//! (`canvas:openAll`, `layout:apply`, …, ids from
//! `codecanvas/atalhos-e-comandos.md`). The palette, the keyboard shortcuts
//! and the MCP server share this one registry, so a behavior is written once.
//!
//! A [`Command`] runs against [`AppState`] with optional JSON arguments and
//! returns a JSON result. What needs the app shell (a native file dialog)
//! is queued in `AppState::pending_app_actions`.

mod chord;
mod registry;

pub use chord::Chord;
pub use registry::{all, find};

use serde_json::Value;

use crate::state::AppState;

/// What the app shell does after a command ran, because only it owns the
/// file picker and the folder tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    /// Ask for a folder with the native dialog and open it.
    PickFolder,
    /// Open this folder (a recent one).
    OpenFolder(std::path::PathBuf),
    /// Close the loaded folder and empty the canvas.
    CloseFolder,
    /// Read the Workspace tree again (the ignore list or its display changed).
    ReloadTree,
}

/// The type of a command body.
pub type Run = fn(&mut AppState, &Value) -> Result<Value, String>;

/// One named action.
pub struct Command {
    /// `group:name`, as in the reference (`canvas:openAll`).
    pub id: &'static str,
    /// Palette label.
    pub title: &'static str,
    /// Group of the Shortcuts dialog (Viewport, Canvas, Selection, Diff, General);
    /// `None` keeps it out of that dialog.
    pub group: Option<&'static str>,
    /// Keyboard shortcuts, matched with exact modifiers. A chord without
    /// Ctrl or Alt is a plain key, skipped while a text field has the focus.
    pub chords: &'static [Chord],
    /// Whether it makes sense now (the palette hides it otherwise).
    pub enabled: fn(&AppState) -> bool,
    /// Control the palette's Go to UI highlights.
    pub anchor: Option<&'static str>,
    /// Shown in the palette.
    pub palette: bool,
    pub run: Run,
}

impl Command {
    /// The shortcut as text (`Ctrl+Shift+O`), empty without one.
    pub fn shortcut_label(&self) -> String {
        self.chords.iter().map(|c| c.label()).collect::<Vec<_>>().join(" / ")
    }
}

/// Runs the command `id`. Unknown ids and disabled commands are errors.
pub fn run(state: &mut AppState, id: &str, args: &Value) -> Result<Value, String> {
    let command = find(id).ok_or_else(|| format!("unknown command {id}"))?;
    if !(command.enabled)(state) {
        return Err(format!("{id} does not apply now"));
    }
    (command.run)(state, args)
}

/// Go to UI: opens what holds the control of command `id` and pulses it for
/// a moment, without running the command. `now` is the app time in seconds.
pub fn go_to_ui(state: &mut AppState, id: &str, now: f64) {
    use crate::state::{LeftPanel, RightPanel};
    let Some(anchor) = find(id).and_then(|c| c.anchor) else { return };
    state.sidebars_hidden = false;
    // "changes:settings" is the control `settings` inside the panel `changes`.
    let panel = anchor.split_once(':').map_or(anchor, |(panel, _)| panel);
    match panel {
        "settings" => state.show_settings = true,
        "layout" => state.right_panel = Some(RightPanel::Layout),
        "folders" => state.right_panel = Some(RightPanel::Folders),
        "file-nodes" => state.right_panel = Some(RightPanel::FileNodes),
        "edges" => state.right_panel = Some(RightPanel::DependencyEdges),
        "lsp" => state.right_panel = Some(RightPanel::LspSymbols),
        "cst" => state.right_panel = Some(RightPanel::Cst),
        "agent-notes" => state.right_panel = Some(RightPanel::AgentNotes),
        "performance" => state.right_panel = Some(RightPanel::Performance),
        "workspace" => state.left_panel = Some(LeftPanel::Workspace),
        "changes" => state.left_panel = Some(LeftPanel::Changes),
        "search" => state.left_panel = Some(LeftPanel::Search),
        "local-server" => state.left_panel = Some(LeftPanel::LocalServer),
        _ => {}
    }
    state.ui_highlight = Some((anchor, now));
}

/// Records where a control drew, for Go to UI.
pub fn note_anchor(state: &mut AppState, id: &'static str, rect: egui::Rect) {
    state.anchor_rects.insert(id, rect);
}

/// How long the Go to UI outline pulses, in seconds.
const HIGHLIGHT_SECONDS: f64 = 2.0;

/// Pulses an outline around the panel (or dialog) Go to UI pointed at.
pub fn draw_ui_highlight(ctx: &egui::Context, state: &mut AppState) {
    let Some((anchor, since)) = state.ui_highlight else { return };
    let now = ctx.input(|i| i.time);
    let age = now - since;
    if age > HIGHLIGHT_SECONDS {
        state.ui_highlight = None;
        return;
    }
    ctx.request_repaint();
    // The exact control when it drew this frame, else its panel.
    let panel = anchor.split_once(':').map_or(anchor, |(panel, _)| panel);
    let rect = state.anchor_rects.get(anchor).copied().or(match panel {
        "settings" => None,
        "workspace" | "changes" | "search" | "local-server" => state.panel_rect_left,
        _ => state.panel_rect_right,
    });
    let Some(rect) = rect else { return };
    let pulse = ((age * 6.0).sin() * 0.5 + 0.5) as f32;
    let color = state.theme.accent.gamma_multiply(0.35 + 0.65 * pulse);
    ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("ui_highlight")))
        .rect_stroke(rect.shrink(1.0), 8.0, egui::Stroke::new(2.5_f32, color), egui::StrokeKind::Inside);
}

/// The command whose shortcut is `key` with exactly `modifiers`, if any.
pub fn for_key(key: egui::Key, modifiers: egui::Modifiers, typing: bool) -> Option<&'static Command> {
    all().iter().find(|c| {
        c.chords.iter().any(|chord| {
            let editing = chord.command && matches!(chord.key, egui::Key::A | egui::Key::X | egui::Key::Z | egui::Key::Y);
            chord.matches(key, modifiers) && !(typing && (chord.is_plain() && chord.key != egui::Key::Escape || editing))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Key, Modifiers};

    #[test]
    fn ids_are_unique_and_follow_group_colon_name() {
        let mut seen = std::collections::HashSet::new();
        for c in all() {
            assert!(c.id.split_once(':').is_some_and(|(g, n)| !g.is_empty() && !n.is_empty()), "{}", c.id);
            assert!(seen.insert(c.id), "duplicate {}", c.id);
        }
    }

    #[test]
    fn no_two_commands_share_a_chord() {
        let all = all();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                for (x, y) in a.chords.iter().flat_map(|x| b.chords.iter().map(move |y| (x, y))) {
                    assert!(x != y, "{} and {} share {}", a.id, b.id, x.label());
                }
            }
        }
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let ctrl_shift_o = Modifiers { command: true, ctrl: true, shift: true, ..Default::default() };
        let ctrl_alt_shift_o = Modifiers { alt: true, ..ctrl_shift_o };
        assert_eq!(for_key(Key::O, ctrl_shift_o, false).map(|c| c.id), Some("canvas:openAll"));
        assert_eq!(for_key(Key::O, ctrl_alt_shift_o, false).map(|c| c.id), Some("canvas:openChangedFolders"));
        assert_eq!(for_key(Key::O, Modifiers::SHIFT, false).map(|c| c.id), Some("canvas:openChanged"));
        assert_eq!(for_key(Key::O, Modifiers::ALT, false).map(|c| c.id), Some("canvas:closeUnchanged"));
        // Ctrl+Shift+X is not Shift+X.
        let ctrl_shift_x = Modifiers { command: true, ctrl: true, shift: true, ..Default::default() };
        assert!(for_key(Key::X, ctrl_shift_x, false).is_none());
        assert_eq!(for_key(Key::X, Modifiers::SHIFT, false).map(|c| c.id), Some("canvas:clearAll"));
    }

    #[test]
    fn plain_keys_are_skipped_while_typing_but_chords_still_work() {
        assert!(for_key(Key::F, Modifiers::NONE, true).is_none());
        assert!(for_key(Key::F, Modifiers::NONE, false).is_some());
        let ctrl = Modifiers { command: true, ctrl: true, ..Default::default() };
        assert!(for_key(Key::K, ctrl, true).is_some());
    }

    #[test]
    fn running_an_unknown_command_or_a_disabled_one_fails() {
        let mut state = AppState::default();
        assert!(run(&mut state, "nope:nothing", &Value::Null).is_err());
        // Nothing on the canvas: deleting the selection does not apply.
        assert!(run(&mut state, "canvas:deleteSelection", &Value::Null).is_err());
    }

    #[test]
    fn a_command_changes_state_through_the_registry() {
        let mut state = AppState::default();
        run(&mut state, "ui:openShortcutsPanel", &Value::Null).unwrap();
        assert!(state.show_shortcuts);
        run(&mut state, "ui:toggleMinimap", &Value::Null).unwrap();
        assert!(!state.settings.show_minimap);
    }
}
