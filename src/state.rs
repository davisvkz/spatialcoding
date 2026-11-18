use std::collections::HashMap;
use std::path::PathBuf;

use crate::analysis::dependencies::indexer::DependencyIndexer;
use crate::canvas::viewport::Viewport;
use crate::edges::edge_store::EdgeStore;
use crate::edges::style::EdgeStyleConfig;
use crate::layout::settings::LayoutSettings;
use crate::nodes::node_store::NodeStore;
use crate::nodes::selection::Selection;
use crate::ui::cst_panel::CstPanelState;
use crate::ui::lsp_panel::LspPanelState;
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftPanel {
    Workspace,
    Changes,
    Search,
    LocalServer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightPanel {
    Layout,
    LspSymbols,
    DependencyEdges,
    Folders,
    FileNodes,
    Cst,
    AgentNotes,
    Performance,
}

pub struct AppState {
    pub viewport: Viewport,
    pub node_store: NodeStore,
    pub edge_store: EdgeStore,
    pub selection: Selection,
    pub active_tool: Tool,
    pub left_panel: Option<LeftPanel>,
    pub right_panel: Option<RightPanel>,
    pub layout_settings: LayoutSettings,
    pub edge_style: EdgeStyleConfig,
    pub theme: Theme,
    pub show_settings: bool,
    pub show_shortcuts: bool,
    pub show_command_palette: bool,
    /// Query and selection of the command palette.
    pub palette: crate::ui::command_palette::PaletteState,
    /// Query, mode and answers of the left Search panel.
    pub search: crate::ui::search_panel::SearchState,
    /// The open diff window (a file against Git HEAD), if any.
    pub diff: Option<crate::ui::diff_view::DiffViewState>,
    pub loaded_dir: Option<PathBuf>,
    pub file_contents: HashMap<PathBuf, String>,
    pub space_held: bool,
    pub settings: AppSettings,
    /// CST tab: cached syntax tree of the selected node and the hovered row.
    pub cst: CstPanelState,
    /// LSP tab: language servers, symbols of the selected node, references.
    pub lsp: LspPanelState,
    /// Dependency edges from the language servers, computed in the background
    /// on `lsp.manager`.
    pub deps: DependencyIndexer,
    /// Explicitly refreshed, read-only Git comparison of local graph evidence.
    pub git_comparison: crate::git::comparison::GraphComparison,
    /// World point the canvas centers on next frame (set when jumping to a
    /// reference; the canvas size is only known while drawing).
    pub pending_focus: Option<egui::Pos2>,
    /// Fit the view to every node next frame, once the canvas size is known
    /// (startup, File nodes › "Fit to nodes", Shift+F).
    pub pending_fit: bool,
    /// Files were opened or removed since the last layout; with Auto Layout
    /// on, the app lays out again once the dependency edges have settled.
    pub layout_dirty: bool,
    /// Files that differ from Git HEAD, refreshed in the background.
    pub changes: crate::git::changes::WorkingTreeChanges,
    /// File nodes › "Highlight top-level symbols": line ranges of the
    /// top-level declarations of each highlighted node.
    pub top_level_highlights: HashMap<u64, Vec<(usize, usize)>>,
    /// Folder nodes the last `workspace::open_files` call created.
    pub last_opened_folders: Vec<u64>,
    /// Files of the loaded folder as the file tree lists them, read lazily
    /// (`workspace::folder_text_files`) and dropped when the folder changes
    /// or the Git change set is refreshed.
    pub folder_files_cache: std::cell::RefCell<Option<std::rc::Rc<Vec<PathBuf>>>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FoldersSettings {
    pub open_collapsed_by_default: bool,
    /// Collapse depth from the repository's depth ("Auto"), else `collapse_depth`.
    pub auto_open: bool,
    /// Folders this deep or deeper below the loaded folder open collapsed.
    pub collapse_depth: usize,
    /// Edges that end on the same collapsed folders are drawn once.
    pub merge_collapsed_edges: bool,
    pub hide_folders: bool,
    pub merge_empty: bool,
    pub merge_overlapping_labels: bool,
    pub show_folder_names: bool,
    pub min_size_pct: f32,
    pub diff_highlight: bool,
    /// Reveal Through Collapse: kept visible inside collapsed folders.
    pub reveal_selected: bool,
    pub reveal_diff_files: bool,
    pub reveal_diff_folders: bool,
    pub reveal_backwards: bool,
    /// Advanced.
    pub collapse_edges_only: bool,
    pub auto_collapse_on_zoom: bool,
    pub zoom_fog: bool,
}

impl Default for FoldersSettings {
    fn default() -> Self {
        Self {
            open_collapsed_by_default: false,
            reveal_selected: false,
            reveal_diff_files: false,
            reveal_diff_folders: false,
            reveal_backwards: false,
            collapse_edges_only: false,
            auto_collapse_on_zoom: false,
            zoom_fog: false,
            auto_open: true,
            collapse_depth: 2,
            merge_collapsed_edges: true,
            // v2 hides folder frames by default; collapsed cards still show.
            hide_folders: true,
            merge_empty: true,
            merge_overlapping_labels: true,
            show_folder_names: true,
            min_size_pct: 0.6,
            diff_highlight: true,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FilesSettings {
    pub skip_imports: bool,
    pub collapse_long: bool,
    pub long_max_lines: usize,
    pub show_names: bool,
    pub min_size_pct: f32,
}

impl Default for FilesSettings {
    fn default() -> Self {
        Self {
            skip_imports: true,
            collapse_long: true,
            long_max_lines: 1000,
            show_names: true,
            min_size_pct: 0.5,
        }
    }
}

/// CST panel options; the depth limit is shared with Performance › CST Depth.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CstSettings {
    /// Header switch "Toggle CST nodes": paint CST highlights on the canvas.
    pub show_canvas: bool,
    pub limit_depth: bool,
    /// Deepest level listed (the root is level 0) while `limit_depth`.
    pub max_depth: usize,
    /// List anonymous tokens (`fn`, `(`, `;`) too.
    pub show_hidden_types: bool,
    /// Skip structural wrappers (blocks, parameter and argument lists, …),
    /// listing their children in their place.
    pub semantic_only: bool,
    /// Hovering a row highlights only that node; off, its ancestors too.
    pub single_hover: bool,
    /// Outline the CST node at each end of the selection's edges.
    pub selected_edge_nodes: bool,
    /// RGB overrides by grammar kind for the Symbol Mapping swatches/highlights.
    pub type_colors: std::collections::BTreeMap<String, [u8; 3]>,
}

impl Default for CstSettings {
    fn default() -> Self {
        Self {
            show_canvas: true,
            limit_depth: true,
            max_depth: 6,
            show_hidden_types: false,
            semantic_only: true,
            single_hover: true,
            selected_edge_nodes: true,
            type_colors: Default::default(),
        }
    }
}

/// LSP Symbols › LSP Advanced Settings.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LspSettings {
    /// Resting the pointer on a symbol row asks for its references.
    pub query_on_hover: bool,
    /// Open the files holding a symbol's references when they arrive.
    pub auto_open_matches: bool,
    /// At most this many files are opened by Auto Open Matches.
    pub max_files: usize,
    /// Jumping to a reference keeps the current zoom.
    pub keep_zoom: bool,
    /// Jumping to a reference glides instead of cutting.
    pub smooth_transition: bool,
    /// Edges not linked to the active symbol are dimmed by `dim_pct`.
    pub dim_edges: bool,
    pub dim_pct: f32,
    /// Files holding the active symbol or its references stay visible in
    /// collapsed folders.
    pub reveal_through_collapse: bool,
    /// Mark references outside the view on the canvas border.
    pub out_of_view: bool,
}

impl Default for LspSettings {
    fn default() -> Self {
        Self {
            query_on_hover: false,
            auto_open_matches: true,
            max_files: 25,
            keep_zoom: false,
            smooth_transition: true,
            dim_edges: true,
            dim_pct: 0.8,
            reveal_through_collapse: false,
            out_of_view: false,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub smooth_zoom: bool,
    pub show_viewport_details: bool,
    pub show_activity_indicator: bool,
    pub show_controls_bar: bool,
    pub show_minimap: bool,
    /// Dot grid behind the canvas. Off by default: v2 draws a plain background.
    #[serde(default)]
    pub show_grid: bool,
    pub font_size: f32,
    pub node_width: f32,
    pub show_dot_folders: bool,
    #[serde(default)]
    pub folders: FoldersSettings,
    #[serde(default)]
    pub files: FilesSettings,
    pub cst: CstSettings,
    pub lsp: LspSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            smooth_zoom: true,
            show_viewport_details: false,
            show_activity_indicator: false,
            show_controls_bar: true,
            show_minimap: true,
            show_grid: false,
            font_size: 11.0,
            node_width: 650.0,
            show_dot_folders: false,
            folders: FoldersSettings::default(),
            files: FilesSettings::default(),
            cst: CstSettings::default(),
            lsp: LspSettings::default(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            viewport: Viewport::default(),
            node_store: NodeStore::default(),
            edge_store: EdgeStore::default(),
            selection: Selection::default(),
            active_tool: Tool::Select,
            left_panel: Some(LeftPanel::Workspace),
            right_panel: None,
            layout_settings: LayoutSettings::default(),
            edge_style: EdgeStyleConfig::default(),
            theme: Theme::dark(),
            show_settings: false,
            show_shortcuts: false,
            show_command_palette: false,
            palette: Default::default(),
            search: Default::default(),
            diff: None,
            loaded_dir: None,
            file_contents: HashMap::new(),
            space_held: false,
            settings: AppSettings::default(),
            cst: CstPanelState::default(),
            lsp: LspPanelState::default(),
            deps: DependencyIndexer::default(),
            git_comparison: Default::default(),
            pending_focus: None,
            pending_fit: false,
            layout_dirty: false,
            changes: Default::default(),
            top_level_highlights: HashMap::new(),
            last_opened_folders: Vec::new(),
            folder_files_cache: Default::default(),
        }
    }
}
