//! The list of commands. Ids and shortcuts follow
//! `codecanvas/atalhos-e-comandos.md`.

use std::sync::LazyLock;

use egui::Key;
use serde_json::{json, Value};

use super::{AppAction, Chord, Command, Run};
use crate::folder_actions::{self, FolderAction};
use crate::state::{AppState, LeftPanel, RightPanel};
use crate::workspace::{self, ImportDirection};

type Result = std::result::Result<Value, String>;

/// A `&'static [Chord]`: promoted to a constant so a command can hold it.
macro_rules! keys {
    ($($chord:expr),+ $(,)?) => {{
        const KEYS: &[Chord] = &[$($chord),+];
        KEYS
    }};
}

static REGISTRY: LazyLock<Vec<Command>> = LazyLock::new(build);

/// Every command, in palette order.
pub fn all() -> &'static [Command] {
    &REGISTRY
}

pub fn find(id: &str) -> Option<&'static Command> {
    REGISTRY.iter().find(|c| c.id == id)
}

fn always(_: &AppState) -> bool {
    true
}
fn has_nodes(s: &AppState) -> bool {
    !s.node_store.file_nodes.is_empty()
}
fn has_folder(s: &AppState) -> bool {
    s.loaded_dir.is_some()
}
fn has_selection(s: &AppState) -> bool {
    s.selection.count() > 0
}
fn has_primary(s: &AppState) -> bool {
    s.selection.primary.is_some()
}
fn has_diff(s: &AppState) -> bool {
    s.diff.is_some()
}
fn changes_known(s: &AppState) -> bool {
    s.changes.is_known()
}

fn cmd(id: &'static str, title: &'static str, run: Run) -> Command {
    Command {
        id,
        title,
        group: None,
        chords: &[],
        enabled: always,
        anchor: None,
        palette: true,
        run,
    }
}

impl Command {
    fn group(mut self, group: &'static str) -> Self {
        self.group = Some(group);
        self
    }
    fn keys(mut self, chords: &'static [Chord]) -> Self {
        self.chords = chords;
        self
    }
    fn when(mut self, enabled: fn(&AppState) -> bool) -> Self {
        self.enabled = enabled;
        self
    }
    fn anchor(mut self, anchor: &'static str) -> Self {
        self.anchor = Some(anchor);
        self
    }
    fn hidden(mut self) -> Self {
        self.palette = false;
        self
    }
}

fn ok() -> Result {
    Ok(Value::Null)
}

fn folder_action(action: FolderAction) -> Run {
    // A non-capturing closure per action keeps `Run` a plain fn pointer.
    match action {
        FolderAction::CollapseDeepest => |s, _| run_folder(s, FolderAction::CollapseDeepest),
        FolderAction::ExpandShallowest => |s, _| run_folder(s, FolderAction::ExpandShallowest),
        FolderAction::CollapseLeaves => |s, _| run_folder(s, FolderAction::CollapseLeaves),
        FolderAction::ExpandTopmost => |s, _| run_folder(s, FolderAction::ExpandTopmost),
        FolderAction::CollapseAll => |s, _| run_folder(s, FolderAction::CollapseAll),
        FolderAction::ExpandAll => |s, _| run_folder(s, FolderAction::ExpandAll),
        FolderAction::CollapseUnchanged => |s, _| run_folder(s, FolderAction::CollapseUnchanged),
        FolderAction::ExpandChanged => |s, _| run_folder(s, FolderAction::ExpandChanged),
        FolderAction::CloseUnchanged => |s, _| run_folder(s, FolderAction::CloseUnchanged),
        FolderAction::OpenChanged => |s, _| run_folder(s, FolderAction::OpenChanged),
    }
}

fn run_folder(state: &mut AppState, action: FolderAction) -> Result {
    if !action.applies(state) {
        return Err(format!("{} does not apply now", action.label()));
    }
    action.run(state);
    ok()
}

/// Shift+E / Shift+R: the folder under the pointer with what is nested in
/// it, or every folder when the pointer is not on one.
fn fold_under_pointer(state: &mut AppState, collapse: bool) -> Result {
    match folder_actions::folder_under_pointer(state) {
        Some(folder) => {
            let ids = folder_actions::folder_subtree(state, folder);
            folder_actions::set_many(state, &ids, collapse);
            ok()
        }
        None => run_folder(state, if collapse { FolderAction::CollapseAll } else { FolderAction::ExpandAll }),
    }
}

/// Ctrl+E / Ctrl+R: every folder at the nesting level of the folder under
/// the pointer.
fn fold_level(state: &mut AppState, collapse: bool) -> Result {
    let Some(folder) = folder_actions::folder_under_pointer(state) else {
        return Err("point at a folder".into());
    };
    let root = state.loaded_dir.clone().unwrap_or_default();
    let level = state.node_store.folder_depth(folder, &root);
    let ids = folder_actions::folders_at_level(state, level, !collapse);
    folder_actions::set_many(state, &ids, collapse);
    Ok(json!({ "level": level, "folders": ids.len() }))
}

fn toggle_left(state: &mut AppState, panel: LeftPanel) {
    state.left_panel = if state.left_panel == Some(panel) { None } else { Some(panel) };
}

fn toggle_right(state: &mut AppState, panel: RightPanel) {
    state.right_panel = if state.right_panel == Some(panel) { None } else { Some(panel) };
}

fn open_left(state: &mut AppState, panel: LeftPanel) -> Result {
    state.sidebars_hidden = false;
    state.left_panel = Some(panel);
    ok()
}

/// Ctrl+S: hides the open panels, or brings back the ones it hid.
fn toggle_panels_with_memory(state: &mut AppState) -> Result {
    match state.panel_memory.take() {
        Some((left, right)) if state.left_panel.is_none() && state.right_panel.is_none() => {
            state.left_panel = left;
            state.right_panel = right;
        }
        _ => {
            if state.left_panel.is_some() || state.right_panel.is_some() {
                state.panel_memory = Some((state.left_panel.take(), state.right_panel.take()));
            }
        }
    }
    ok()
}

/// Esc: closes the topmost dialog, otherwise clears the selection.
fn escape(state: &mut AppState) -> Result {
    if state.show_command_palette {
        state.show_command_palette = false;
    } else if state.show_settings {
        state.show_settings = false;
    } else if state.show_shortcuts {
        state.show_shortcuts = false;
    } else if state.show_docs {
        state.show_docs = false;
    } else if state.diff.is_some() {
        state.diff = None;
    } else {
        state.selection.clear();
    }
    ok()
}

/// D / Shift+D: the next or previous layout of the open diff.
fn cycle_diff_mode(state: &mut AppState, forward: bool) -> Result {
    use crate::ui::diff_view::DiffMode;
    const ORDER: [DiffMode; 3] = [DiffMode::Changes, DiffMode::Unified, DiffMode::Split];
    if let Some(diff) = state.diff.as_mut() {
        let at = ORDER.iter().position(|m| *m == diff.mode).unwrap_or(1);
        diff.mode = ORDER[(at + if forward { 1 } else { ORDER.len() - 1 }) % ORDER.len()];
        state.settings.diff.mode = diff.mode;
    }
    ok()
}

fn zoom_by(state: &mut AppState, factor: f32) -> Result {
    let center = if state.canvas_rect.is_positive() { state.canvas_rect.center() } else { egui::pos2(400.0, 300.0) };
    state.viewport.zoom_by(factor, center);
    ok()
}

fn build() -> Vec<Command> {
    let mut v: Vec<Command> = Vec::new();

    // ---- viewport -------------------------------------------------------
    v.push(
        cmd("viewport:fitNodes", "Fit view to nodes", |s, _| {
            s.pending_fit = true;
            ok()
        })
        .group("Viewport")
        .keys(keys![Chord::shift(Key::F)])
        .when(has_nodes),
    );
    v.push(
        cmd("viewport:zoomToMouse", "Zoom to 60% at the mouse", |s, _| {
            let Some(pos) = s.hover_screen else { return Err("the pointer is not on the canvas".into()) };
            let center = if s.canvas_rect.is_positive() { s.canvas_rect.center() } else { pos };
            let vp = &mut s.viewport;
            let world = vp.screen_to_world(pos);
            let zoom = crate::nodes::lod::FOCUS_ZOOM;
            vp.zoom_to(zoom, pos);
            vp.offset = center.to_vec2() / zoom - world.to_vec2();
            ok()
        })
        .group("Viewport")
        .keys(keys![Chord::new(Key::F)])
        .hidden(),
    );
    v.push(
        cmd("viewport:zoomBy", "Zoom in", |s, args| {
            let factor = args.get("factor").and_then(Value::as_f64).unwrap_or(1.25) as f32;
            zoom_by(s, factor)
        })
        .group("Viewport")
        .keys(keys![Chord::ctrl(Key::Plus).or_key(Key::Equals)]),
    );
    v.push(
        cmd("viewport:zoomOut", "Zoom out", |s, _| zoom_by(s, 0.8))
            .group("Viewport")
            .keys(keys![Chord::ctrl(Key::Minus)]),
    );
    v.push(cmd("viewport:reset", "Reset view", |s, _| {
        s.viewport = crate::canvas::viewport::Viewport::default();
        ok()
    }));

    // ---- layout ---------------------------------------------------------
    v.push(
        cmd("layout:apply", "Apply layout", |s, _| {
            workspace::apply_layout_keeping_selection(s);
            ok()
        })
        .group("Canvas")
        .keys(keys![Chord::new(Key::Num1)])
        .when(has_nodes)
        .anchor("layout"),
    );
    v.push(cmd("layout:toggleAutoLayout", "Toggle auto layout", |s, _| {
        s.layout_settings.auto_layout = !s.layout_settings.auto_layout;
        ok()
    }).anchor("layout"));

    // ---- canvas ---------------------------------------------------------
    v.push(
        cmd("canvas:openAll", "Open all files", |s, _| {
            Ok(json!({ "opened": workspace::open_all_files(s).len() }))
        })
        .group("Canvas")
        .keys(keys![Chord::ctrl_shift(Key::O)])
        .when(has_folder),
    );
    v.push(
        cmd("canvas:clearAll", "Clear canvas", |s, _| {
            workspace::clear_canvas(s);
            ok()
        })
        .group("Canvas")
        .keys(keys![Chord::shift(Key::X)])
        .when(has_nodes),
    );
    v.push(
        cmd("canvas:clearSymbols", "Clear pinned symbols", |s, _| {
            s.lsp.clear_symbols();
            s.selection.clear_symbols();
            ok()
        })
        .group("Canvas")
        .keys(keys![Chord::shift(Key::C)]),
    );
    v.push(
        cmd("canvas:selectAll", "Select all nodes", |s, _| {
            let ids = s.node_store.all_file_ids();
            s.selection.select_all(ids);
            ok()
        })
        .group("Selection")
        .keys(keys![Chord::ctrl(Key::A)])
        .when(has_nodes)
        .hidden(),
    );
    v.push(cmd("canvas:clearSelection", "Unselect all nodes", |s, _| {
        s.selection.clear();
        ok()
    })
    .when(has_selection));
    v.push(
        cmd("canvas:deleteSelection", "Remove selected nodes from the canvas", |s, _| {
            let ids: Vec<u64> = s.selection.selected_ids().collect();
            workspace::remove_nodes(s, &ids);
            Ok(json!({ "removed": ids.len() }))
        })
        .group("Selection")
        .keys(keys![Chord::new(Key::Backspace), Chord::new(Key::Delete), Chord::ctrl(Key::X)])
        .when(has_selection),
    );
    v.push(
        cmd("canvas:openChanged", "Open changed files", |s, _| {
            Ok(json!({ "opened": workspace::open_changed_files(s).len() }))
        })
        .group("Diff")
        .keys(keys![Chord::shift(Key::O)])
        .when(changes_known),
    );
    v.push(
        cmd("canvas:closeUnchanged", "Close files without changes", |s, _| {
            workspace::close_unchanged_files(s);
            ok()
        })
        .group("Diff")
        .keys(keys![Chord::alt(Key::O)])
        .when(changes_known),
    );
    v.push(
        cmd("canvas:openChangedFolders", "Open all folders with changes", |s, _| run_folder(s, FolderAction::OpenChanged))
            .group("Diff")
            .keys(keys![Chord::ctrl_alt_shift(Key::O)])
            .when(changes_known),
    );
    v.push(cmd("canvas:closeUnchangedFolders", "Close folders without changes", |s, _| run_folder(s, FolderAction::CloseUnchanged)).when(changes_known));
    v.push(
        cmd("canvas:openSelectedImporters", "Open files that import the selection", |s, _| {
            // Asks the language server; the files open when it has answered.
            Ok(json!({ "asked": crate::importers::open_importers(None, s) }))
        })
        .when(crate::importers::can_open_importers)
        .anchor("file-nodes"),
    );
    v.push(
        cmd("canvas:openSelectedImports", "Open imported files", |s, _| {
            Ok(json!({ "opened": workspace::open_import_neighbours(s, ImportDirection::Imported).len() }))
        })
        .when(crate::importers::can_open_imports)
        .anchor("file-nodes"),
    );
    v.push(
        cmd("canvas:highlightSelectedTopLevelSymbols", "Highlight top-level symbols", |s, _| {
            workspace::toggle_top_level_highlights(s, None);
            ok()
        })
        .when(has_selection)
        .anchor("file-nodes"),
    );
    v.push(cmd("canvas:fitSelection", "Select all and fit", |s, _| {
        let ids = s.node_store.all_file_ids();
        s.selection.select_all(ids);
        s.pending_fit = true;
        ok()
    }).hidden());

    v.push(
        cmd("canvas:undo", "Undo", |s, _| if crate::undo::undo(s) { ok() } else { Err("nothing to undo".into()) })
            .group("Canvas")
            .keys(keys![Chord::ctrl(Key::Z)])
            .when(|s| s.history.can_undo()),
    );
    v.push(
        cmd("canvas:redo", "Redo", |s, _| if crate::undo::redo(s) { ok() } else { Err("nothing to redo".into()) })
            .group("Canvas")
            .keys(keys![Chord::ctrl_shift(Key::Z), Chord::ctrl(Key::Y)])
            .when(|s| s.history.can_redo()),
    );

    // ---- folders --------------------------------------------------------
    for (id, title, action, key) in [
        ("folder:collapseLeaves", "Collapse leaf folders", FolderAction::CollapseLeaves, Some(Key::R)),
        ("folder:expandTopmost", "Expand topmost collapsed folders", FolderAction::ExpandTopmost, Some(Key::E)),
        ("folder:collapseDeepest", "Collapse deepest level", FolderAction::CollapseDeepest, None),
        ("folder:expandShallowest", "Expand shallowest level", FolderAction::ExpandShallowest, None),
        ("folder:collapseAll", "Collapse all folders", FolderAction::CollapseAll, None),
        ("folder:expandAll", "Expand all folders", FolderAction::ExpandAll, None),
        ("folder:collapseUnchanged", "Collapse all folders without changes", FolderAction::CollapseUnchanged, None),
        ("folder:expandChanged", "Expand all folders with changes", FolderAction::ExpandChanged, None),
    ] {
        let mut c = cmd(id, title, folder_action(action)).when(has_nodes).anchor("folders");
        if let Some(key) = key {
            c = c.group("Canvas").keys(match key {
                Key::R => keys![Chord::new(Key::R)],
                _ => keys![Chord::new(Key::E)],
            });
        }
        v.push(c);
    }
    v.push(
        cmd("folder:expand", "Expand the folder under the mouse (all without one)", |s, _| fold_under_pointer(s, false))
            .group("Canvas")
            .keys(keys![Chord::shift(Key::E)])
            .when(has_nodes)
            .hidden(),
    );
    v.push(
        cmd("folder:collapse", "Collapse the folder under the mouse (all without one)", |s, _| fold_under_pointer(s, true))
            .group("Canvas")
            .keys(keys![Chord::shift(Key::R)])
            .when(has_nodes)
            .hidden(),
    );
    v.push(
        cmd("folder:expandLevel", "Expand the level of the folder under the mouse", |s, _| fold_level(s, false))
            .group("Canvas")
            .keys(keys![Chord::ctrl(Key::E)])
            .when(has_nodes)
            .hidden(),
    );
    v.push(
        cmd("folder:collapseLevel", "Collapse the level of the folder under the mouse", |s, _| fold_level(s, true))
            .group("Canvas")
            .keys(keys![Chord::ctrl(Key::R)])
            .when(has_nodes)
            .hidden(),
    );
    v.push(
        cmd("folder:selectNextScope", "Select folder scope", |s, _| {
            folder_actions::select_scope(s);
            ok()
        })
        .group("Selection")
        .keys(keys![Chord::new(Key::Backslash)])
        .when(has_primary)
        .anchor("folders"),
    );
    v.push(
        cmd("folder:openNextScope", "Open folder scope", |s, _| {
            Ok(json!({ "opened": folder_actions::open_scope(s).len() }))
        })
        .group("Selection")
        .keys(keys![Chord::shift(Key::Backslash)])
        .when(has_primary)
        .anchor("folders"),
    );

    // ---- agent notes ----------------------------------------------------
    v.push(
        cmd("agentNotes:push", "Push an agent note", |s, args| {
            let markdown = args.get("markdown").and_then(Value::as_str).ok_or("markdown is required")?;
            let title = args.get("title").and_then(Value::as_str).unwrap_or("Note");
            let id = s.notes.push(title, markdown);
            s.right_panel = Some(RightPanel::AgentNotes);
            Ok(json!({ "id": id }))
        })
        .hidden(),
    );
    v.push(
        cmd("agentNotes:updateNote", "Update an agent note", |s, args| {
            let id = args.get("id").and_then(Value::as_u64).ok_or("id is required")?;
            let title = args.get("title").and_then(Value::as_str).map(str::to_owned);
            let markdown = args.get("markdown").and_then(Value::as_str).map(str::to_owned);
            if s.notes.update(id, title, markdown) { ok() } else { Err(format!("no note {id}")) }
        })
        .hidden(),
    );
    v.push(
        cmd("agentNotes:open", "Open an agent note", |s, args| {
            let id = args.get("id").and_then(Value::as_u64).ok_or("id is required")?;
            if s.notes.get(id).is_none() {
                return Err(format!("no note {id}"));
            }
            s.notes.focused = Some(id);
            s.right_panel = Some(RightPanel::AgentNotes);
            ok()
        })
        .hidden(),
    );
    v.push(
        cmd("agentNotes:openFiles", "Open the files of the agent notes", |s, args| {
            let id = args.get("id").and_then(Value::as_u64);
            Ok(json!({ "opened": workspace::open_note_targets(s, id) }))
        })
        .anchor("agent-notes"),
    );
    v.push(
        cmd("agentNotes:clear", "Clear agent notes", |s, _| {
            s.notes.clear();
            ok()
        })
        .anchor("agent-notes"),
    );

    // ---- diff -----------------------------------------------------------
    v.push(
        cmd("diff:openLocalSession", "Show diff of selected file", |s, _| {
            workspace::open_diff_of_selection(s);
            ok()
        })
        .when(has_primary),
    );
    v.push(
        cmd("diff:cycleViewMode", "Next diff layout", |s, _| cycle_diff_mode(s, true))
            .group("Diff")
            .anchor("changes:layout")
            .keys(keys![Chord::new(Key::D)])
            .when(has_diff)
            .hidden(),
    );
    v.push(
        cmd("diff:cycleViewModeBack", "Previous diff layout", |s, _| cycle_diff_mode(s, false))
            .group("Diff")
            .keys(keys![Chord::shift(Key::D)])
            .when(has_diff)
            .hidden(),
    );
    v.push(cmd("diff:setViewMode", "Set the diff layout", |s, args| {
        use crate::ui::diff_view::DiffMode;
        let mode = match args.get("mode").and_then(Value::as_str) {
            Some("changes") => DiffMode::Changes,
            Some("unified") => DiffMode::Unified,
            Some("split") => DiffMode::Split,
            _ => return Err("mode is changes, unified or split".into()),
        };
        s.settings.diff.mode = mode;
        if let Some(diff) = s.diff.as_mut() {
            diff.mode = mode;
        }
        ok()
    }));
    v.push(
        cmd("diff:toggleCollapsed", "Collapse unchanged lines", |s, _| {
            s.settings.diff.collapse = !s.settings.diff.collapse;
            ok()
        })
        .group("Diff")
        .keys(keys![Chord::new(Key::C)])
        .when(has_diff)
        .anchor("changes:settings"),
    );
    v.push(cmd("diff:close", "Close the diff", |s, _| {
        s.diff = None;
        ok()
    }).when(has_diff));

    // ---- ui -------------------------------------------------------------
    v.push(
        cmd("ui:toggleCommandPalette", "Command palette", |s, _| {
            s.show_command_palette = !s.show_command_palette;
            ok()
        })
        .group("General")
        .keys(keys![Chord::ctrl(Key::K)])
        .hidden(),
    );
    v.push(
        cmd("ui:openShortcutsPanel", "Show shortcuts", |s, _| {
            s.show_shortcuts = !s.show_shortcuts;
            ok()
        })
        .group("General")
        .keys(keys![Chord::ctrl(Key::Slash)]),
    );
    v.push(
        cmd("ui:toggleDocsWindow", "Show docs", |s, _| {
            s.show_docs = !s.show_docs;
            ok()
        })
        .group("General")
        .keys(keys![Chord::ctrl_shift(Key::Slash)]),
    );
    v.push(
        cmd("ui:openZoomLevelsPanel", "Zoom levels", |s, _| {
            s.show_zoom_levels = !s.show_zoom_levels;
            ok()
        })
        .group("General")
        .keys(keys![Chord::shift(Key::V)]),
    );
    v.push(
        cmd("ui:toggleMinimap", "Toggle minimap", |s, _| {
            s.settings.show_minimap = !s.settings.show_minimap;
            ok()
        })
        .group("General")
        .keys(keys![Chord::new(Key::M)])
        .anchor("settings"),
    );
    v.push(
        cmd("ui:toggleSidebars", "Toggle sidebars", |s, _| {
            s.sidebars_hidden = !s.sidebars_hidden;
            ok()
        })
        .group("General")
        .keys(keys![Chord::ctrl(Key::Period)]),
    );
    v.push(
        cmd("ui:toggleSidebarsWithMemory", "Hide or restore the open panels", |s, _| toggle_panels_with_memory(s))
            .group("General")
            .keys(keys![Chord::ctrl(Key::S)]),
    );
    v.push(
        cmd("ui:openSettings", "Open settings", |s, _| {
            s.show_settings = !s.show_settings;
            ok()
        })
        .group("General")
        .keys(keys![Chord::ctrl(Key::Comma)]),
    );
    v.push(
        cmd("ui:openWorkspacePanel", "Open Workspace panel", |s, _| open_left(s, LeftPanel::Workspace))
            .group("General")
            .keys(keys![Chord::ctrl_shift(Key::E)])
            .hidden(),
    );
    v.push(
        cmd("ui:openSearchPanel", "Open Search panel", |s, _| open_left(s, LeftPanel::Search))
            .group("General")
            .keys(keys![Chord::ctrl_shift(Key::F)])
            .hidden(),
    );
    v.push(
        cmd("ui:openChangesPanel", "Open Changes panel", |s, _| open_left(s, LeftPanel::Changes))
            .group("General")
            .keys(keys![Chord::ctrl_shift(Key::S)])
            .hidden(),
    );
    v.push(cmd("ui:openFolder", "Open folder…", |s, _| {
        s.pending_app_actions.push(AppAction::PickFolder);
        ok()
    }));
    v.push(
        cmd("ui:escape", "Close dialog or clear the selection", |s, _| escape(s))
            .group("General")
            .keys(keys![Chord::new(Key::Escape)])
            .hidden(),
    );

    macro_rules! panel_toggles {
        (left: $(($id:literal, $title:literal, $panel:expr, $anchor:literal)),* $(,)?) => {$(
            v.push(cmd($id, $title, |s, _| { toggle_left(s, $panel); ok() }).anchor($anchor));
        )*};
        (right: $(($id:literal, $title:literal, $panel:expr, $anchor:literal)),* $(,)?) => {$(
            v.push(cmd($id, $title, |s, _| { toggle_right(s, $panel); ok() }).anchor($anchor));
        )*};
    }
    panel_toggles!(left:
        ("ui:toggleWorkspacePanel", "Toggle Workspace panel", LeftPanel::Workspace, "workspace"),
        ("ui:toggleChangesPanel", "Toggle Changes panel", LeftPanel::Changes, "changes"),
        ("ui:toggleSearchPanel", "Toggle Search panel", LeftPanel::Search, "search"),
        ("ui:toggleLocalServerPanel", "Toggle Local Server panel", LeftPanel::LocalServer, "local-server"),
    );
    panel_toggles!(right:
        ("ui:toggleLayoutPanel", "Toggle Layout panel", RightPanel::Layout, "layout"),
        ("ui:toggleLspPanel", "Toggle LSP Symbols panel", RightPanel::LspSymbols, "lsp"),
        ("ui:toggleEdgesPanel", "Toggle Dependency Edges panel", RightPanel::DependencyEdges, "edges"),
        ("ui:toggleFoldersPanel", "Toggle Folders panel", RightPanel::Folders, "folders"),
        ("ui:toggleFileNodesPanel", "Toggle File Nodes panel", RightPanel::FileNodes, "file-nodes"),
        ("ui:toggleCstPanel", "Toggle CST panel", RightPanel::Cst, "cst"),
        ("ui:toggleAgentNotesPanel", "Toggle Agent Notes panel", RightPanel::AgentNotes, "agent-notes"),
        ("ui:togglePerformancePanel", "Toggle Performance panel", RightPanel::Performance, "performance"),
    );
    v
}
