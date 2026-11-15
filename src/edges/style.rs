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

/// How the gradient fade length is measured, matching the v2 Code Canvas
/// "Gradient Gap" control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GradientGapMode {
    /// Percentage of the edge's drawn length (0..=1).
    Percent,
    /// Drop by a fixed world-pixel amount from the total; edges shorter than
    /// `gradient_min_dist_px` are drawn solid in the target color.
    Fixed,
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
    #[serde(default = "default_gradient_gap_mode")]
    pub gradient_gap_mode: GradientGapMode,
    #[serde(default = "default_gradient_gap_pct")]
    pub gradient_gap_pct: f32,
    #[serde(default = "default_gradient_min_dist_px")]
    pub gradient_min_dist_px: f32,
    pub base_color: [f32; 3],
    #[serde(default = "default_symbol_colors")]
    pub symbol_colors: HashMap<String, [f32; 3]>,
    #[serde(default = "default_true")]
    pub show_reexport_edges: bool,
    #[serde(default = "default_true")]
    pub show_barrel_edges: bool,
    #[serde(default)]
    pub highlight_reexport: bool,
    #[serde(default)]
    pub highlight_barrel: bool,
    #[serde(default = "default_reexport_color")]
    pub reexport_color: [f32; 3],
    #[serde(default = "default_barrel_color")]
    pub barrel_color: [f32; 3],
}

fn default_gradient_gap_mode() -> GradientGapMode {
    GradientGapMode::Percent
}
fn default_gradient_gap_pct() -> f32 {
    0.8
}
fn default_gradient_min_dist_px() -> f32 {
    40.0
}
fn default_true() -> bool {
    true
}
fn default_reexport_color() -> [f32; 3] {
    // Warm amber: lifted from v2's re-export highlight hue.
    [0.98, 0.72, 0.30]
}
fn default_barrel_color() -> [f32; 3] {
    // Cool cyan: distinct from re-export amber so the two highlight at once.
    [0.37, 0.80, 0.98]
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
            show_arrow_heads: false,
            only_selected: true,
            scale_arrow_heads: false,
            min_size_pct: 0.5,
            use_gradient: false,
            gradient_gap_mode: default_gradient_gap_mode(),
            gradient_gap_pct: default_gradient_gap_pct(),
            gradient_min_dist_px: default_gradient_min_dist_px(),
            // Warm cream, close to v2 Code Canvas edge tone.
            base_color: [0.88, 0.86, 0.80],
            symbol_colors: default_symbol_colors(),
            show_reexport_edges: true,
            show_barrel_edges: true,
            highlight_reexport: false,
            highlight_barrel: false,
            reexport_color: default_reexport_color(),
            barrel_color: default_barrel_color(),
        }
    }
}
