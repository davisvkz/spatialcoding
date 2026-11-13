use std::collections::HashMap;

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
    #[serde(default = "default_symbol_colors")]
    pub symbol_colors: HashMap<String, [f32; 3]>,
}

pub fn default_symbol_colors() -> HashMap<String, [f32; 3]> {
    [
        ("fn",        [0.33, 0.76, 0.98]),  // blue
        ("method",    [0.40, 0.85, 0.72]),  // teal
        ("struct",    [0.98, 0.73, 0.33]),  // orange
        ("class",     [0.78, 0.56, 0.98]),  // purple
        ("interface", [0.65, 0.50, 0.98]),  // violet
        ("trait",     [0.65, 0.50, 0.98]),  // violet
        ("enum",      [0.98, 0.85, 0.37]),  // yellow
        ("const",     [0.56, 0.82, 0.49]),  // green
        ("var",       [0.56, 0.82, 0.49]),  // green
        ("mod",       [0.70, 0.70, 0.70]),  // grey
        ("ctor",      [0.40, 0.85, 0.72]),  // teal
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

impl EdgeStyleConfig {
    pub fn kind_color(&self, label: &str) -> egui::Color32 {
        let rgb = self
            .symbol_colors
            .get(label)
            .copied()
            .unwrap_or(self.base_color);
        egui::Color32::from_rgb(
            (rgb[0] * 255.0) as u8,
            (rgb[1] * 255.0) as u8,
            (rgb[2] * 255.0) as u8,
        )
    }
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
            symbol_colors: default_symbol_colors(),
        }
    }
}
