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
    /// World point the canvas centers on next frame (set when jumping to a
    /// reference; the canvas size is only known while drawing).
    pub pending_focus: Option<egui::Pos2>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PerformanceMode {
    Eager,
    OnOpen,
    Lazy,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PerformanceSettings {
    pub auto: bool,
    pub mode: PerformanceMode,
    pub use_sqlite: bool,
    pub cst_depth_limit: Option<usize>,
}

impl Default for PerformanceSettings {
    fn default() -> Self {
        Self {
            auto: true,
            mode: PerformanceMode::Eager,
            use_sqlite: false,
            cst_depth_limit: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FoldersSettings {
    pub open_collapsed_by_default: bool,
    pub auto_open: bool,
    pub hide_folders: bool,
    pub merge_empty: bool,
    pub merge_overlapping_labels: bool,
    pub show_folder_names: bool,
    pub min_size_pct: f32,
    pub diff_highlight: bool,
}

impl Default for FoldersSettings {
    fn default() -> Self {
        Self {
            open_collapsed_by_default: false,
            auto_open: true,
            hide_folders: false,
            merge_empty: true,
            merge_overlapping_labels: true,
            show_folder_names: true,
            min_size_pct: 0.6,
            diff_highlight: true,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
            collapse_long: false,
            long_max_lines: 1000,
            show_names: false,
            min_size_pct: 0.5,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppSettings {
    pub smooth_zoom: bool,
    pub show_viewport_details: bool,
    pub show_activity_indicator: bool,
    pub show_controls_bar: bool,
    pub show_minimap: bool,
    pub font_size: f32,
    pub node_width: f32,
    pub show_dot_folders: bool,
    #[serde(default)]
    pub performance: PerformanceSettings,
    #[serde(default)]
    pub folders: FoldersSettings,
    #[serde(default)]
    pub files: FilesSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            smooth_zoom: true,
            show_viewport_details: false,
            show_activity_indicator: false,
            show_controls_bar: true,
            show_minimap: true,
            font_size: 12.0,
            node_width: 700.0,
            show_dot_folders: false,
            performance: PerformanceSettings::default(),
            folders: FoldersSettings::default(),
            files: FilesSettings::default(),
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
            loaded_dir: None,
            file_contents: HashMap::new(),
            space_held: false,
            settings: AppSettings::default(),
            cst: CstPanelState::default(),
            lsp: LspPanelState::default(),
            deps: DependencyIndexer::default(),
            pending_focus: None,
        }
    }
}
