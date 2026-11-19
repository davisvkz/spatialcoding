use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum RelationshipType {
    Imports,
    Inherits,
    #[default]
    References,
    Renders,
}

impl RelationshipType {
    pub const ALL: [Self; 4] = [
        Self::Imports,
        Self::Inherits,
        Self::References,
        Self::Renders,
    ];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Imports => "Imports",
            Self::Inherits => "Inherits",
            Self::References => "References",
            Self::Renders => "Renders",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Imports => "Explicitly loads a module, header or source file.",
            Self::Inherits => {
                "Explicitly inherits a base class or refines an interface or protocol."
            }
            Self::References => {
                "References a symbol or eligible local document/configuration file."
            }
            Self::Renders => "Renders a component through a proven local binding.",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RelationshipStyle {
    pub visible: bool,
    pub color: [f32; 3],
}

/// `#RRGGBB` as the `[f32; 3]` the config stores.
pub const fn hex(rgb: u32) -> [f32; 3] {
    [
        ((rgb >> 16) & 0xff) as f32 / 255.0,
        ((rgb >> 8) & 0xff) as f32 / 255.0,
        (rgb & 0xff) as f32 / 255.0,
    ]
}

/// Idle edge color of the reference (`#7684A0`, times Lightness).
pub const BASE_COLOR: [f32; 3] = hex(0x7684A0);
pub const SELECTED_COLOR: [f32; 3] = hex(0xAED6FF);
pub const BACKWARDS_COLOR: [f32; 3] = hex(0xEF4444);
pub const SELECTED_BACKWARDS_COLOR: [f32; 3] = hex(0xE85580);
/// Below L3 thin edges switch to this color.
pub const SEMANTIC_FADE_COLOR: [f32; 3] = hex(0x3E3D48);
pub const ADDED_COLOR: [f32; 3] = hex(0x4CD964);
pub const REMOVED_COLOR: [f32; 3] = hex(0xFF5F56);
pub const REEXPORT_COLOR: [f32; 3] = hex(0x38BDF8);
pub const BARREL_COLOR: [f32; 3] = hex(0xF97316);

/// Relation colors of the reference at 80 %: uses `#6AA7A0`, extends
/// `#B69B65`, inherits `#9B8AB5`, conforms `#85A87C`. Imports keep the base
/// color; the four types map as Imports = base, References = uses,
/// Inherits = extends, Renders = inherits.
fn default_relationships() -> [RelationshipStyle; 4] {
    [BASE_COLOR, hex(0xB69B65), hex(0x6AA7A0), hex(0x9B8AB5)].map(|color| RelationshipStyle {
        visible: true,
        color,
    })
}

/// Relation colors written by builds before settings version 3.
pub(crate) fn legacy_relationship_colors() -> [[f32; 3]; 4] {
    [
        [0.55, 0.61, 0.75],
        [0.67, 0.58, 0.81],
        [0.55, 0.61, 0.75],
        [0.34, 0.66, 0.62],
    ]
}

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
    /// the Min Dist (`gradient_min_dist_sq`) are drawn solid in the target color.
    Fixed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EdgeStyleConfig {
    #[serde(default)]
    pub highlight_changes: bool,
    #[serde(default)]
    pub diff_colors: bool,
    #[serde(default)]
    pub show_deleted: bool,
    #[serde(default = "default_relationships")]
    pub relationships: [RelationshipStyle; 4],
    #[serde(default = "default_true")]
    pub highlight_selected: bool,
    #[serde(default)]
    pub highlight_reverse_layout: bool,
    #[serde(default)]
    pub folder_levels: bool,
    #[serde(default = "default_folder_depth")]
    pub folder_depth: usize,
    pub style: EdgeStyle,
    pub opacity_mode: OpacityMode,
    pub show_all: bool,
    pub highlight_backwards: bool,
    pub show_connection_markers: bool,
    pub show_arrow_heads: bool,
    pub only_selected: bool,
    pub scale_arrow_heads: bool,
    /// Smallest arrow-head scale while Scale Arrow Heads shrinks them with
    /// the zoom (v2 "Min Size", 0..1).
    pub min_size_pct: f32,
    /// Hide edges shorter on screen than this share of 40 px (0 shows all).
    pub hide_short_pct: f32,
    pub use_gradient: bool,
    #[serde(default = "default_gradient_gap_mode")]
    pub gradient_gap_mode: GradientGapMode,
    #[serde(default = "default_gradient_gap_pct")]
    pub gradient_gap_pct: f32,
    /// Length in world px under which an edge draws solid: v2's "Min Dist"
    /// (default 40000, so only huge edges fade). Stored as
    /// `gradient_min_dist_sq` by earlier builds, which read it as px².
    #[serde(alias = "gradient_min_dist_sq", default = "default_gradient_min_dist")]
    pub gradient_min_dist: f32,
    /// Fixed mode: world px that stay visible at each end of the edge.
    #[serde(default = "default_gradient_fixed_visible_px")]
    pub gradient_fixed_visible_px: f32,
    /// Bezier handle length as a multiple of the horizontal distance
    /// (`h = dx * k`, 0–2).
    #[serde(default = "default_bezier_curvature")]
    pub bezier_curvature: f32,
    /// Percent (0–200) applied to the base color's channels.
    #[serde(default = "default_base_lightness")]
    pub base_lightness: f32,
    pub base_color: [f32; 3],
    /// Color of edges touching the selection (v2: light blue, fully opaque).
    #[serde(default = "default_selected_color")]
    pub selected_color: [f32; 3],
    #[serde(default = "default_symbol_colors")]
    pub symbol_colors: HashMap<String, [f32; 3]>,
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
    /// Ports: a circle on the node border and a row band at each connected
    /// line; edges then end at the port instead of the identifier.
    #[serde(default = "default_true")]
    pub show_ports: bool,
    #[serde(default = "default_import_port_color")]
    pub import_port_color: [f32; 3],
    #[serde(default = "default_symbol_port_color")]
    pub symbol_port_color: [f32; 3],
}

fn default_gradient_gap_mode() -> GradientGapMode {
    GradientGapMode::Percent
}
fn default_folder_depth() -> usize {
    1
}
fn default_gradient_gap_pct() -> f32 {
    0.8
}
fn default_gradient_min_dist() -> f32 {
    40_000.0
}
fn default_selected_color() -> [f32; 3] {
    SELECTED_COLOR
}
fn default_true() -> bool {
    true
}
fn default_reexport_color() -> [f32; 3] {
    REEXPORT_COLOR
}
fn default_import_port_color() -> [f32; 3] {
    // Muted teal: imports read apart from symbols without competing with
    // the selection's light blue.
    [0.36, 0.62, 0.66]
}
fn default_symbol_port_color() -> [f32; 3] {
    // Neutral grey, as the v2 Code Canvas ports.
    [0.48, 0.50, 0.54]
}
fn default_barrel_color() -> [f32; 3] {
    BARREL_COLOR
}
fn default_bezier_curvature() -> f32 {
    0.5
}
fn default_base_lightness() -> f32 {
    100.0
}
fn default_gradient_fixed_visible_px() -> f32 {
    1000.0
}

pub fn default_symbol_colors() -> HashMap<String, [f32; 3]> {
    [
        ("fn", [0.33, 0.76, 0.98]),        // blue
        ("method", [0.40, 0.85, 0.72]),    // teal
        ("struct", [0.98, 0.73, 0.33]),    // orange
        ("class", [0.78, 0.56, 0.98]),     // purple
        ("interface", [0.65, 0.50, 0.98]), // violet
        ("trait", [0.65, 0.50, 0.98]),     // violet
        ("enum", [0.98, 0.85, 0.37]),      // yellow
        ("const", [0.56, 0.82, 0.49]),     // green
        ("var", [0.56, 0.82, 0.49]),       // green
        ("mod", [0.70, 0.70, 0.70]),       // grey
        ("ctor", [0.40, 0.85, 0.72]),      // teal
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

impl EdgeStyleConfig {
    /// Moves colors still at the pre-v2 defaults to the reference ones
    /// (settings version 3); a color the user changed is kept.
    pub(crate) fn migrate_to_v3(&mut self) {
        let swap = |value: &mut [f32; 3], old: [f32; 3], new: [f32; 3]| {
            if *value == old {
                *value = new;
            }
        };
        swap(&mut self.base_color, [0.55, 0.61, 0.75], BASE_COLOR);
        swap(&mut self.selected_color, [0.72, 0.84, 1.0], SELECTED_COLOR);
        swap(&mut self.reexport_color, [0.98, 0.72, 0.30], REEXPORT_COLOR);
        swap(&mut self.barrel_color, [0.37, 0.80, 0.98], BARREL_COLOR);
        let (old, new) = (legacy_relationship_colors(), default_relationships());
        for ((slot, old), new) in self.relationships.iter_mut().zip(old).zip(new) {
            swap(&mut slot.color, old, new.color);
        }
        self.hide_short_pct = 0.0;
        self.min_size_pct = self.min_size_pct.clamp(0.3, 1.0);
    }
    /// A stable palette keyed by the workspace-relative folder prefix. Opening
    /// another file does not change an existing folder's color.
    pub fn folder_color(&self, file: &std::path::Path, root: &std::path::Path) -> Option<[f32; 3]> {
        let relative = file.parent()?.strip_prefix(root).ok()?;
        let mut hash = 2166136261_u32;
        for part in relative.components().take(self.folder_depth.max(1)) {
            for byte in part.as_os_str().to_string_lossy().bytes().chain(*b"/") {
                hash = (hash ^ u32::from(byte)).wrapping_mul(16777619);
            }
        }
        let palette = [
            [0.45, 0.67, 0.86],
            [0.71, 0.57, 0.85],
            [0.36, 0.74, 0.63],
            [0.86, 0.65, 0.39],
            [0.82, 0.48, 0.59],
            [0.58, 0.71, 0.40],
        ];
        Some(palette[hash as usize % palette.len()])
    }
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
            highlight_changes: false,
            diff_colors: false,
            show_deleted: false,
            relationships: default_relationships(),
            highlight_selected: true,
            highlight_reverse_layout: false,
            folder_levels: false,
            folder_depth: default_folder_depth(),
            style: EdgeStyle::Bezier,
            opacity_mode: OpacityMode::Depth,
            // v2 keeps the rest of the web visible (dimmed) around a selection.
            show_all: true,
            highlight_backwards: false,
            show_connection_markers: true,
            // Arrow heads only on the selection's edges while `only_selected`
            // is on, as v2's "Arrow Heads › Only selected".
            show_arrow_heads: true,
            only_selected: true,
            scale_arrow_heads: true,
            min_size_pct: 0.5,
            hide_short_pct: 0.0,
            use_gradient: true,
            gradient_gap_mode: default_gradient_gap_mode(),
            gradient_gap_pct: default_gradient_gap_pct(),
            gradient_min_dist: default_gradient_min_dist(),
            gradient_fixed_visible_px: default_gradient_fixed_visible_px(),
            bezier_curvature: default_bezier_curvature(),
            base_lightness: default_base_lightness(),
            base_color: BASE_COLOR,
            selected_color: default_selected_color(),
            symbol_colors: default_symbol_colors(),
            show_reexport_edges: false,
            show_barrel_edges: true,
            highlight_reexport: false,
            highlight_barrel: false,
            reexport_color: default_reexport_color(),
            barrel_color: default_barrel_color(),
            show_ports: true,
            import_port_color: default_import_port_color(),
            symbol_port_color: default_symbol_port_color(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_colors_are_workspace_relative_and_group_at_the_selected_depth() {
        use std::path::Path;
        let mut config = EdgeStyleConfig::default();
        let color =
            |cfg: &EdgeStyleConfig, path| cfg.folder_color(Path::new(path), Path::new("/repo"));
        assert_eq!(
            color(&config, "/repo/src/a/one.rs"),
            color(&config, "/repo/src/b/two.rs")
        );
        config.folder_depth = 2;
        assert_ne!(
            color(&config, "/repo/src/a/one.rs"),
            color(&config, "/repo/src/b/two.rs")
        );
        assert_eq!(color(&config, "/outside/a.rs"), None);
    }

    #[test]
    fn legacy_symbol_pill_setting_is_ignored_and_not_serialized_again() {
        let mut value = serde_json::to_value(EdgeStyleConfig::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("show_symbol_pills".into(), serde_json::json!(true));
        let config: EdgeStyleConfig = serde_json::from_value(value).unwrap();
        assert!(serde_json::to_value(config)
            .unwrap()
            .get("show_symbol_pills")
            .is_none());
    }

    #[test]
    fn legacy_configs_enable_reference_visibility_and_selection_emphasis() {
        let mut value = serde_json::to_value(EdgeStyleConfig::default()).unwrap();
        for key in [
            "highlight_changes",
            "diff_colors",
            "show_deleted",
            "relationships",
            "highlight_selected",
            "highlight_reverse_layout",
            "folder_levels",
            "folder_depth",
        ] {
            value.as_object_mut().unwrap().remove(key);
        }
        let config: EdgeStyleConfig = serde_json::from_value(value).unwrap();
        assert!(config.highlight_selected);
        assert!(config.relationships.iter().all(|style| style.visible));
        assert!(!config.folder_levels && !config.highlight_reverse_layout);
        assert_eq!(config.folder_depth, 1);
        assert!(!config.highlight_changes && !config.diff_colors && !config.show_deleted);
    }

    #[test]
    fn legacy_configs_turn_ports_on_with_default_colors() {
        let mut value = serde_json::to_value(EdgeStyleConfig::default()).unwrap();
        for key in ["show_ports", "import_port_color", "symbol_port_color"] {
            value.as_object_mut().unwrap().remove(key);
        }
        let config: EdgeStyleConfig = serde_json::from_value(value).unwrap();
        assert!(config.show_ports);
        assert_ne!(config.import_port_color, config.symbol_port_color);
    }
}
