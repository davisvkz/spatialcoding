use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeStyle {
    Bezier,
    Step,
    Line,
    Pcb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpacityMode {
    Full,
    Depth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeStyleConfig {
    pub style: EdgeStyle,
    pub opacity_mode: OpacityMode,
    pub show_all: bool,
    pub highlight_backwards: bool,
    pub show_connection_markers: bool,
    pub show_arrow_heads: bool,
    pub only_selected: bool,
    pub scale_arrow_heads: bool,
    pub min_size_pct: f32,
    pub use_gradient: bool,
    pub base_color: [f32; 3],
}

impl Default for EdgeStyleConfig {
    fn default() -> Self {
        Self {
            style: EdgeStyle::Bezier,
            opacity_mode: OpacityMode::Depth,
            show_all: false,
            highlight_backwards: false,
            show_connection_markers: false,
            show_arrow_heads: true,
            only_selected: true,
            scale_arrow_heads: false,
            min_size_pct: 0.5,
            use_gradient: false,
            base_color: [0.5, 0.5, 0.5],
        }
    }
}
