use std::collections::HashMap;
use std::path::PathBuf;

use crate::canvas::viewport::Viewport;
use crate::edges::edge_store::EdgeStore;
use crate::edges::style::EdgeStyleConfig;
use crate::layout::settings::LayoutSettings;
use crate::nodes::node_store::NodeStore;
use crate::nodes::selection::Selection;
use crate::ui::cst_panel::CstPanelState;
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
        }
    }
}
